import ActivityKit
import Foundation
import Testing
@testable import Cereal

@MainActor private final class ActivityTestClock {
    var uptime: TimeInterval = 100
    var date = Date(timeIntervalSince1970: 1_800_000_000)
    func advance(_ seconds: TimeInterval) { uptime += seconds; date += seconds }
}

@MainActor private final class ActivityTestSession: RecordingActivitySession {
    let noteID: UUID
    let id = UUID().uuidString
    var isPreview = false
    var activityState: ActivityState { state.phase == .ended ? .ended : .active }
    var state: RecordingActivityAttributes.ContentState
    var operations: [String] = []
    var staleDates: [Date?] = []
    var beforeEnd: (() async -> Void)?
    init(noteID: UUID, state: RecordingActivityAttributes.ContentState) { self.noteID = noteID; self.state = state }
    func update(_ state: RecordingActivityAttributes.ContentState, staleDate: Date?) async {
        operations.append("update-\(state.phase.rawValue)"); staleDates.append(staleDate); self.state = state
    }
    func end(_ state: RecordingActivityAttributes.ContentState) async {
        await beforeEnd?()
        operations.append("end"); self.state = state
    }
}

@MainActor private final class ActivityTestBackend: RecordingActivityBackend {
    var enabled = true
    var failsRequest = false
    var sessions: [ActivityTestSession] = []
    var requestedStaleDates: [Date] = []
    var requests = 0
    var activities: [any RecordingActivitySession] { sessions }
    func request(noteID: UUID, state: RecordingActivityAttributes.ContentState, staleDate: Date, presentation: RecordingActivityAttributes.Presentation?) throws -> any RecordingActivitySession {
        requests += 1
        if failsRequest { throw CocoaError(.featureUnsupported) }
        let session = ActivityTestSession(noteID: noteID, state: state)
        session.isPreview = presentation == .preview
        sessions.append(session); requestedStaleDates.append(staleDate)
        return session
    }
    func controller(clock: ActivityTestClock) -> RecordingLiveActivityController {
        RecordingLiveActivityController(backend: self, now: { clock.date }, uptime: { clock.uptime })
    }
}

/// A deterministic ActivityKit boundary, never a fake claim of OS recording.
/// Rendering, authorization prompts, and real microphone/background behavior need device QA.
@MainActor @Suite struct RecordingLiveActivityTests {
    @Test func startsWithCumulativeTimerAndShortStaleDeadline() throws {
        let backend = ActivityTestBackend(), clock = ActivityTestClock()
        let controller = backend.controller(clock: clock), note = UUID()
        controller.start(noteID: note, title: "Private meeting title", elapsed: 42)
        let session = try #require(backend.sessions.first)
        #expect(session.noteID == note && session.state.phase == .recording)
        #expect(session.state.elapsed == 42)
        #expect(session.state.timerAnchor == clock.date.addingTimeInterval(-42))
        #expect(backend.requestedStaleDates == [clock.date.addingTimeInterval(75)])
    }

    @Test func authorizationDisabledAndRequestFailureDoNotCreatePhantomActivity() async {
        let backend = ActivityTestBackend(), clock = ActivityTestClock()
        let controller = backend.controller(clock: clock)
        backend.enabled = false
        controller.start(noteID: UUID(), title: "Hidden", elapsed: 0)
        #expect(backend.requests == 0)
        backend.enabled = true; backend.failsRequest = true
        controller.start(noteID: UUID(), title: "Hidden", elapsed: 0)
        controller.pause(elapsed: 8); controller.resume(elapsed: 8); controller.end()
        await controller.finishPendingUpdates()
        #expect(backend.requests == 1 && backend.sessions.isEmpty)
    }

    @Test func failedInitialRequestCanRecoverWhenExistingRecordingReturnsToForeground() async throws {
        let backend = ActivityTestBackend(), clock = ActivityTestClock()
        let controller = backend.controller(clock: clock), id = UUID()
        backend.failsRequest = true
        controller.start(noteID: id, title: "", elapsed: 0)
        clock.advance(50); backend.failsRequest = false
        controller.start(noteID: id, title: "", elapsed: 50)
        await controller.finishPendingUpdates()
        #expect(backend.requests == 2 && backend.sessions.count == 1)
        #expect(backend.sessions[0].state.elapsed == 50)
        #expect(backend.sessions[0].state.timerAnchor == clock.date.addingTimeInterval(-50))
    }

    @Test func heartbeatThrottlingNeverDelaysPauseResumeOrEnd() async throws {
        let backend = ActivityTestBackend(), clock = ActivityTestClock()
        let controller = backend.controller(clock: clock)
        controller.start(noteID: UUID(), title: "", elapsed: 10)
        clock.advance(29); controller.heartbeat(elapsed: 39)
        await controller.finishPendingUpdates()
        let session = try #require(backend.sessions.first)
        #expect(session.operations.isEmpty)
        clock.advance(1); controller.heartbeat(elapsed: 40)
        controller.pause(elapsed: 40)
        clock.advance(120); controller.resume(elapsed: 40)
        controller.end(elapsed: 41)
        await controller.finishPendingUpdates()
        #expect(session.operations == ["update-recording", "update-paused", "update-recording", "end"])
        #expect(session.staleDates[0] == Date(timeIntervalSince1970: 1_800_000_105))
        #expect(session.staleDates[1] == nil)
        #expect(session.staleDates[2] == clock.date.addingTimeInterval(75))
        #expect(session.state.phase == .ended && session.state.elapsed == 41)
    }

    @Test func pauseResumeExcludePausedWallTimeAndFreezePausedTimer() async throws {
        let backend = ActivityTestBackend(), clock = ActivityTestClock()
        let controller = backend.controller(clock: clock)
        controller.start(noteID: UUID(), title: "", elapsed: 0)
        clock.advance(12); controller.pause(elapsed: 12)
        await controller.finishPendingUpdates()
        let session = try #require(backend.sessions.first)
        #expect(!session.state.runsTimer(isStale: false))
        #expect(session.staleDates.last! == nil)
        clock.advance(600); controller.resume(elapsed: 12)
        await controller.finishPendingUpdates()
        #expect(session.state.timerAnchor == clock.date.addingTimeInterval(-12))
        #expect(session.state.runsTimer(isStale: false))
        #expect(backend.requests == 1)
    }

    @Test func internalChunkRotationKeepsSameActivityWithoutNewBackgroundRequest() async throws {
        let backend = ActivityTestBackend(), clock = ActivityTestClock()
        let controller = backend.controller(clock: clock), id = UUID()
        controller.start(noteID: id, title: "", elapsed: 0)
        clock.advance(120)
        // Models an internal audio-chunk boundary, not a user Stop.
        controller.start(noteID: id, title: "Changed private title", elapsed: 120)
        await controller.finishPendingUpdates()
        #expect(backend.requests == 1)
        let session = try #require(backend.sessions.first)
        #expect(session.operations == ["update-recording"])
        #expect(session.state.elapsed == 120 && session.state.phase == .recording)
        #expect(session.state.timerAnchor == clock.date.addingTimeInterval(-120))
    }

    @Test func userStopEndsOnceAndLaterResumeStartsNewActivityForSameNote() async throws {
        let backend = ActivityTestBackend(), clock = ActivityTestClock()
        let controller = backend.controller(clock: clock), id = UUID()
        controller.start(noteID: id, title: "", elapsed: 4)
        controller.end(elapsed: 9); controller.end(elapsed: 999)
        controller.heartbeat(elapsed: 999); controller.pause(elapsed: 999); controller.resume(elapsed: 999)
        await controller.finishPendingUpdates()
        let first = try #require(backend.sessions.first)
        #expect(first.operations == ["end"] && first.state.elapsed == 9)
        clock.advance(100)
        controller.start(noteID: id, title: "", elapsed: 9)
        #expect(backend.requests == 2)
        #expect(backend.sessions[1].state.timerAnchor == clock.date.addingTimeInterval(-9))
    }

    @Test func queuedPauseThenEndUsesLatestElapsedInsteadOfLaggingOSContent() async throws {
        let backend = ActivityTestBackend(), clock = ActivityTestClock()
        let controller = backend.controller(clock: clock)
        controller.start(noteID: UUID(), title: "", elapsed: 0)
        controller.pause(elapsed: 55)
        controller.end() // Update has not run yet: the OS session still reports zero.
        await controller.finishPendingUpdates()
        let session = try #require(backend.sessions.first)
        #expect(session.operations == ["update-paused", "end"])
        #expect(session.state.phase == .ended && session.state.elapsed == 55)
    }

    @Test func switchingNoteEndsOldActivityAndNeverAppliesOldUpdatesToNewSession() async throws {
        let backend = ActivityTestBackend(), clock = ActivityTestClock()
        let controller = backend.controller(clock: clock), firstID = UUID(), nextID = UUID()
        controller.start(noteID: firstID, title: "", elapsed: 0)
        controller.pause(elapsed: 5)
        controller.start(noteID: nextID, title: "", elapsed: 0)
        controller.pause(elapsed: 2)
        await controller.finishPendingUpdates()
        #expect(backend.requests == 2)
        #expect(backend.sessions[0].noteID == firstID && backend.sessions[0].state.phase == .ended)
        #expect(backend.sessions[0].operations == ["update-paused", "end"])
        #expect(backend.sessions[1].noteID == nextID && backend.sessions[1].state.elapsed == 2)
        #expect(backend.sessions[1].operations == ["update-paused"])
    }

    @Test func orphanCleanupEndsPriorRecordingAndPausedSessionsButNotNewRequest() async throws {
        let backend = ActivityTestBackend(), clock = ActivityTestClock()
        let old = backend.controller(clock: clock), paused = backend.controller(clock: clock)
        old.start(noteID: UUID(), title: "", elapsed: 8)
        paused.start(noteID: UUID(), title: "", elapsed: 20); paused.pause(elapsed: 20)
        await paused.finishPendingUpdates()
        let first = try #require(backend.sessions.first)
        var created: RecordingLiveActivityController?
        first.beforeEnd = {
            created = backend.controller(clock: clock)
            created?.start(noteID: UUID(), title: "", elapsed: 0)
        }
        clock.advance(1)
        await RecordingLiveActivityController.endOrphanedActivities(backend: backend, now: clock.date)
        #expect(backend.sessions.count == 3)
        #expect(backend.sessions[0].state.phase == .ended && backend.sessions[1].state.phase == .ended)
        #expect(backend.sessions[0].state.updatedAt == clock.date)
        #expect(backend.sessions[2].state.phase == .recording && backend.sessions[2].operations.isEmpty)
    }

    @Test func stalePresentationNeverClaimsTheMicrophoneIsStillRecording() {
        for phase in [RecordingActivityAttributes.ContentState.Phase.recording, .paused, .ended] {
            let state = RecordingActivityAttributes.ContentState(phase: phase, elapsed: 13, timerAnchor: .now, updatedAt: .now)
            #expect(state.status(isStale: true) == "Open to check recording")
            #expect(state.symbol(isStale: true) == "exclamationmark.circle")
            #expect(!state.runsTimer(isStale: true))
            #expect(state.runsTimer(isStale: false) == (phase == .recording))
        }
    }

    @Test func activityPayloadCarriesTheTitleAndNothingElseOfTheNote() throws {
        let note = UUID(), attributes = RecordingActivityAttributes(noteID: note)
        let untitled = RecordingActivityAttributes.ContentState(phase: .recording, elapsed: 15, timerAnchor: .now, updatedAt: .now)
        let encoded = try #require(JSONSerialization.jsonObject(with: JSONEncoder().encode(untitled)) as? [String: Any])
        #expect(Set(encoded.keys) == ["phase", "elapsed", "timerAnchor", "updatedAt"])
        let titled = RecordingActivityAttributes.ContentState(phase: .recording, elapsed: 15, timerAnchor: .now, updatedAt: .now, title: "Weekly planning")
        let withTitle = try #require(JSONSerialization.jsonObject(with: JSONEncoder().encode(titled)) as? [String: Any])
        #expect(Set(withTitle.keys) == ["phase", "elapsed", "timerAnchor", "updatedAt", "title"])
        // An activity from before titles were shown still decodes.
        let old = try JSONDecoder().decode(RecordingActivityAttributes.ContentState.self, from: JSONEncoder().encode(untitled))
        #expect(old.title == nil)
        // The link names the note by its identifier only.
        #expect(attributes.recordingURL.scheme == "cereal" && attributes.recordingURL.host == "recording")
        #expect(attributes.recordingURL.lastPathComponent == note.uuidString)
        #expect(attributes.recordingURL.query == nil && attributes.recordingURL.user == nil && attributes.recordingURL.password == nil)
    }

    @Test func theActivityIsNamedAfterItsNoteAndFollowsANewTitleWithoutTouchingTheTimer() async throws {
        let backend = ActivityTestBackend(), clock = ActivityTestClock()
        let controller = backend.controller(clock: clock), note = UUID()
        controller.start(noteID: note, title: "", elapsed: 0)
        let session = try #require(backend.sessions.first)
        // An impromptu note has no title yet: the activity says what the recording is doing.
        #expect(session.state.title == nil)
        let anchor = session.state.timerAnchor

        clock.advance(40)
        // Its notes were generated and the server named it, or the person typed a title.
        controller.setTitle("  Launch readiness\nreview  ")
        await controller.finishPendingUpdates()
        #expect(session.state.title == "Launch readiness review")
        #expect(session.state.phase == .recording && session.state.timerAnchor == anchor)
        #expect(session.operations == ["update-recording"])
        #expect(session.staleDates.last == clock.date.addingTimeInterval(75))
        // The same title again is not another update.
        controller.setTitle("Launch readiness review")
        await controller.finishPendingUpdates()
        #expect(session.operations.count == 1)

        // Pause, resume and the next part of the recording keep the name.
        controller.pause(elapsed: 40); controller.resume(elapsed: 40)
        controller.start(noteID: note, title: "", elapsed: 41)
        await controller.finishPendingUpdates()
        #expect(session.state.title == "Launch readiness review")
        #expect(backend.requests == 1)
        // A title that is cleared goes from the activity too.
        controller.setTitle("   ")
        await controller.finishPendingUpdates()
        #expect(session.state.title == nil)

        // A long title is cut to what a Lock Screen line can hold.
        #expect(RecordingActivityAttributes.ContentState.shownTitle(String(repeating: "t", count: 300))?.count == 80)
        #expect(RecordingActivityAttributes.ContentState.shownTitle("") == nil)
    }

    @Test func aTitleSetBeforeTheActivityExistsIsUsedWhenItStarts() async throws {
        let backend = ActivityTestBackend(), clock = ActivityTestClock()
        let controller = backend.controller(clock: clock), note = UUID()
        controller.start(noteID: note, title: "Weekly planning", elapsed: 3)
        #expect(backend.sessions[0].state.title == "Weekly planning")
        controller.end(elapsed: 9)
        await controller.finishPendingUpdates()
        // Another note's recording does not inherit it.
        controller.start(noteID: UUID(), title: "", elapsed: 0)
        #expect(backend.sessions[1].state.title == nil)
    }

    @Test func malformedElapsedDoesNotPoisonSystemTimerOrJSONPayload() throws {
        for value in [TimeInterval.nan, .infinity, -9] {
            let backend = ActivityTestBackend(), clock = ActivityTestClock()
            backend.controller(clock: clock).start(noteID: UUID(), title: "", elapsed: value)
            let state = try #require(backend.sessions.first?.state)
            #expect(state.elapsed == 0 && state.timerAnchor == clock.date)
            _ = try JSONEncoder().encode(state)
        }
    }
    @Test func activeServerDeepLinkRequiresSameNonemptyAccountAndExistingNoteIdentity() throws {
        let id = UUID(), url = RecordingActivityAttributes(noteID: id).recordingURL
        #expect(RecordingActivityLinkPolicy.activeServerNoteID(url: url, activeNoteID: id.uuidString.lowercased(), recordingIdentity: "server|A", accountIdentity: "server|A") == id)
        #expect(RecordingActivityLinkPolicy.activeServerNoteID(url: url, activeNoteID: id.uuidString, recordingIdentity: "server|A", accountIdentity: "server|B") == nil)
        #expect(RecordingActivityLinkPolicy.activeServerNoteID(url: url, activeNoteID: id.uuidString, recordingIdentity: nil, accountIdentity: nil) == nil)
        #expect(RecordingActivityLinkPolicy.activeServerNoteID(url: url, activeNoteID: id.uuidString, recordingIdentity: "", accountIdentity: "") == nil)
        #expect(RecordingActivityLinkPolicy.activeServerNoteID(url: url, activeNoteID: UUID().uuidString, recordingIdentity: "server|A", accountIdentity: "server|A") == nil)
        #expect(RecordingActivityLinkPolicy.activeServerNoteID(url: url, activeNoteID: id.uuidString, recordingIdentity: "other-server|A", accountIdentity: "server|A") == nil)
    }

    @Test func activeServerDeepLinkRejectsUntrustedRouteOrEmbeddedSecrets() throws {
        let id = UUID()
        // The last one is well formed but uses the scheme of builds before the production identity.
        for raw in ["https://recording/\(id)", "cereal://other/\(id)", "cereal://recording/extra/\(id)", "cereal://recording/\(id)?token=synthetic", "cereal://user@recording/\(id)", "cereal://recording/\(id)#other", "minutes://recording/\(id)"] {
            let url = try #require(URL(string: raw))
            #expect(RecordingActivityLinkPolicy.activeServerNoteID(url: url, activeNoteID: id.uuidString, recordingIdentity: "server|A", accountIdentity: "server|A") == nil)
        }
    }

    @Test func previewOnlyCleanupLeavesActualCaptureActivityUntouched() async throws {
        let backend = ActivityTestBackend(), clock = ActivityTestClock()
        let genuine = backend.controller(clock: clock)
        let preview = RecordingLiveActivityController(backend: backend, now: { clock.date }, uptime: { clock.uptime }, presentation: .preview)
        genuine.start(noteID: UUID(), title: "", elapsed: 2)
        preview.start(noteID: UUID(), title: "", elapsed: 3)
        await preview.endPreviewActivities()
        #expect(backend.sessions[0].state.phase == .recording && backend.sessions[0].operations.isEmpty)
        #expect(backend.sessions[1].isPreview && backend.sessions[1].state.phase == .ended)
        #expect(backend.sessions[1].operations == ["end"])
    }

    @Test func previewWidgetTextAndLinkNeverClaimAudioCapture() throws {
        let attributes = RecordingActivityAttributes(noteID: UUID(), presentation: .preview)
        #expect(attributes.isPreview && attributes.recordingURL.host == "live-activity-preview")
        let old = try JSONDecoder().decode(RecordingActivityAttributes.self, from: Data("{\"noteID\":\"\(UUID().uuidString)\"}".utf8))
        #expect(!old.isPreview && old.recordingURL.host == "recording")
        for phase in [RecordingActivityAttributes.ContentState.Phase.recording, .paused, .ended] {
            let state = RecordingActivityAttributes.ContentState(phase: phase, elapsed: 0, timerAnchor: .now, updatedAt: .now)
            #expect(state.status(isStale: false, isPreview: true).lowercased().contains("preview"))
            #expect(state.status(isStale: true, isPreview: true) == "Open to check preview")
            #expect(state.symbol(isStale: false, isPreview: true) != "mic.fill")
        }
    }

    #if DEBUG
    @Test func osPreviewHarnessUsesActivityBoundaryAndPreservesTimerOnPauseAndRotate() async throws {
        let backend = ActivityTestBackend(), clock = ActivityTestClock()
        let controller = RecordingLiveActivityController(backend: backend, now: { clock.date }, uptime: { clock.uptime }, presentation: .preview)
        let model = LiveActivityQAModel(controller: controller, uptime: { clock.uptime })
        await model.start(); clock.advance(12); await model.pause()
        #expect(model.phase == .paused && model.elapsed == 12)
        clock.advance(200); await model.resume(); clock.advance(5); await model.rotate()
        #expect(model.elapsed == 17 && backend.requests == 1)
        #expect(model.systemIdentifier == backend.sessions[0].id && backend.sessions[0].isPreview)
        await model.cleanup()
        #expect(model.phase == .ended && model.systemState == "none")
        #expect(backend.sessions[0].operations.filter { $0 == "end" }.count == 1)
    }
    #endif

}
