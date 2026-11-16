import ActivityKit
import Foundation
import Testing
import APIClient
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
    var requestedAccounts: [String?] = []
    func request(noteID: UUID, accountID: String?, state: RecordingActivityAttributes.ContentState, staleDate: Date, presentation: RecordingActivityAttributes.Presentation?) throws -> any RecordingActivitySession {
        requests += 1
        requestedAccounts.append(accountID)
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
    // MARK: Link policy vectors

    private static let accountA = AccountID(address: "https://a.example.test", userID: "00000000-0000-4000-8000-00000000000a")
    private static let accountB = AccountID(address: "https://b.example.test", userID: "00000000-0000-4000-8000-00000000000b")

    @Test func theRecordingLinkCarriesItsAccountAndNothingElse() throws {
        let id = UUID()
        // From before accounts, and for the local demo: the note alone.
        let old = RecordingActivityAttributes(noteID: id).recordingURL
        #expect(old.absoluteString == "cereal://recording/\(id.uuidString)")
        #expect(RecordingActivityLinkPolicy.link(old) == .init(noteID: id, account: nil))
        // With an account: exactly one parameter, `a`, of 64 hex characters.
        let new = RecordingActivityAttributes(noteID: id, accountID: Self.accountA.raw).recordingURL
        #expect(new.absoluteString == "cereal://recording/\(id.uuidString)?a=\(Self.accountA.raw)")
        #expect(RecordingActivityLinkPolicy.link(new) == .init(noteID: id, account: Self.accountA))
        // Something that is not an account id never reaches the link.
        for bad in ["", "sam@example.test", "../../x", String(repeating: "A", count: 64), String(repeating: "a", count: 63)] {
            #expect(RecordingActivityAttributes(noteID: id, accountID: bad).recordingURL == old, "\(bad)")
        }
        // An activity started before accounts existed still decodes, and one started now encodes its account.
        let legacy = try JSONDecoder().decode(RecordingActivityAttributes.self, from: Data(#"{"noteID":"\#(id.uuidString)"}"#.utf8))
        #expect(legacy.accountID == nil && legacy.recordingURL == old)
        let state = try JSONDecoder().decode(RecordingActivityAttributes.ContentState.self,
            from: Data(#"{"phase":"recording","elapsed":3,"timerAnchor":0,"updatedAt":0}"#.utf8))
        #expect(state.accountLabel == nil && state.accountHost == nil && state.title == nil)
        let round = try JSONDecoder().decode(RecordingActivityAttributes.self, from: JSONEncoder().encode(RecordingActivityAttributes(noteID: id, accountID: Self.accountA.raw)))
        #expect(round.accountID == Self.accountA.raw)
    }

    @Test func aRecordingLinkOpensTheRunningRecordingOrTheNoteInItsOwnAccount() throws {
        let id = UUID(), note = id.uuidString.lowercased()
        let a = Self.accountA, b = Self.accountB
        let active = NoteRef(account: a, id: note)
        func target(_ text: String, active: NoteRef? = nil, accounts: [AccountID] = [a, b], ledger: [AccountID: Set<String>] = [:]) throws -> RecordingActivityLinkPolicy.Target {
            let url = try #require(URL(string: text))
            let link = try #require(RecordingActivityLinkPolicy.link(url))
            return RecordingActivityLinkPolicy.target(link, active: active, accounts: accounts, ledger: { ledger[$0] ?? [] })
        }
        let bare = "cereal://recording/\(id.uuidString)"
        // The recording in progress: its note, when the link names its account or none.
        #expect(try target(bare, active: active) == .note(active))
        #expect(try target(bare + "?a=\(a.raw)", active: active) == .note(active))
        // The stored id may be in either case.
        #expect(try target(bare, active: NoteRef(account: a, id: id.uuidString)) == .note(NoteRef(account: a, id: id.uuidString)))
        // The same note id in another account is another note: it opens there, not in the recording's account.
        #expect(try target(bare + "?a=\(b.raw)", active: active) == .note(NoteRef(account: b, id: note)))
        // No recording, or a recording of another note: the note in the account the link names, if it is here.
        #expect(try target(bare + "?a=\(b.raw)") == .note(NoteRef(account: b, id: note)))
        #expect(try target(bare + "?a=\(b.raw)", active: NoteRef(account: a, id: UUID().uuidString.lowercased())) == .note(NoteRef(account: b, id: note)))
        // An account that is no longer here: Home. A link never leads into another account.
        #expect(try target(bare + "?a=\(b.raw)", accounts: [a]) == .home)
        #expect(try target(bare + "?a=\(b.raw)", active: active, accounts: [a]) == .home)
        // A link from before accounts is looked up in each account's capture ledger, in the person's order.
        #expect(try target(bare, ledger: [b: [note]]) == .note(NoteRef(account: b, id: note)))
        #expect(try target(bare, ledger: [a: [note], b: [note]]) == .note(NoteRef(account: a, id: note)))
        #expect(try target(bare, ledger: [b: [UUID().uuidString.lowercased()]]) == .home)
        #expect(try target(bare, accounts: []) == .home)
    }

    @Test func aRecordingLinkWithAnythingElseInItIsRefused() throws {
        let id = UUID(), a = Self.accountA.raw
        // The last one is well formed but uses the scheme of builds before the production identity.
        for raw in ["https://recording/\(id)", "cereal://other/\(id)", "cereal://recording/extra/\(id)", "cereal://recording/\(id)?token=synthetic",
                    "cereal://user@recording/\(id)", "cereal://recording/\(id)#other", "minutes://recording/\(id)", "cereal://recording/not-a-note",
                    "cereal://recording/\(id)?a=", "cereal://recording/\(id)?a=\(a.dropLast())", "cereal://recording/\(id)?a=\(a.uppercased())",
                    "cereal://recording/\(id)?a=\(a)&a=\(a)", "cereal://recording/\(id)?a=\(a)&token=synthetic", "cereal://recording/\(id)?b=\(a)",
                    "cereal://recording/\(id)?a=\(a)#x", "cereal://recording:80/\(id)?a=\(a)", "cereal://recording/\(id)?a=%61\(a.dropFirst())",
                    "cereal://recording/\(id)?"] {
            let url = try #require(URL(string: raw))
            #expect(RecordingActivityLinkPolicy.link(url) == nil, "\(raw)")
        }
    }

    @Test func theActivityNamesTheAccountOnlyWhenThereAreSeveral() async throws {
        let backend = ActivityTestBackend(), clock = ActivityTestClock()
        let controller = RecordingLiveActivityController(backend: backend, now: { clock.date }, uptime: { clock.uptime })
        let note = UUID()
        // One account: the link carries it, and nothing is shown about it.
        controller.start(noteID: note, title: "Planning", elapsed: 0, account: RecordingActivityAccount(id: Self.accountA.raw))
        #expect(backend.requestedAccounts == [Self.accountA.raw])
        #expect(backend.sessions.last?.state.accountLabel == nil && backend.sessions.last?.state.accountHost == nil)
        // A new part of the same recording does not know the account and must not blank it.
        controller.start(noteID: note, title: "", elapsed: 30)
        controller.pause(elapsed: 31)
        await controller.finishPendingUpdates()
        #expect(backend.requests == 1)
        controller.end(elapsed: 31)
        await controller.finishPendingUpdates()
        // Several accounts: the label and the host travel with the state. Never an email or a token.
        controller.start(noteID: UUID(), title: "Planning", elapsed: 0,
                         account: RecordingActivityAccount(id: Self.accountB.raw, label: "Personal", host: "cereal.example.test"))
        let state = try #require(backend.sessions.last?.state)
        #expect(state.accountLabel == "Personal" && state.accountHost == "cereal.example.test")
        let encoded = String(decoding: try JSONEncoder().encode(state), as: UTF8.self)
        #expect(!encoded.contains("@") && !encoded.lowercased().contains("token"))
        // The local demo has no account.
        controller.end()
        controller.start(noteID: UUID(), title: "", elapsed: 0)
        #expect(backend.requestedAccounts.last == .some(nil))
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
