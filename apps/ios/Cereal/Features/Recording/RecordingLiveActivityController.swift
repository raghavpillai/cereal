// ActivityKit's Activity reference predates Sendable annotations. All operations are ordered
// through the main actor and the pending-task chain; no concurrent updates are issued.
@preconcurrency import ActivityKit
import Foundation

/// Injectable boundary keeps lifecycle tests independent from device authorization,
/// microphone hardware, widget rendering, and the user's existing activities.
@MainActor protocol RecordingActivitySession: AnyObject {
    var noteID: UUID { get }
    var id: String { get }
    var state: RecordingActivityAttributes.ContentState { get }
    var activityState: ActivityState { get }
    var isPreview: Bool { get }
    func update(_ state: RecordingActivityAttributes.ContentState, staleDate: Date?) async
    func end(_ state: RecordingActivityAttributes.ContentState) async
}

@MainActor protocol RecordingActivityBackend {
    var enabled: Bool { get }
    var activities: [any RecordingActivitySession] { get }
    func request(noteID: UUID, state: RecordingActivityAttributes.ContentState, staleDate: Date, presentation: RecordingActivityAttributes.Presentation?) throws -> any RecordingActivitySession
}

@MainActor private struct SystemRecordingActivityBackend: RecordingActivityBackend {
    var enabled: Bool { ActivityAuthorizationInfo().areActivitiesEnabled }
    var activities: [any RecordingActivitySession] { Activity<RecordingActivityAttributes>.activities.map(SystemRecordingActivitySession.init) }
    func request(noteID: UUID, state: RecordingActivityAttributes.ContentState, staleDate: Date, presentation: RecordingActivityAttributes.Presentation?) throws -> any RecordingActivitySession {
        SystemRecordingActivitySession(try Activity.request(attributes: RecordingActivityAttributes(noteID: noteID, presentation: presentation),
            content: ActivityContent(state: state, staleDate: staleDate), pushType: nil))
    }
}

@MainActor private final class SystemRecordingActivitySession: RecordingActivitySession {
    private let activity: Activity<RecordingActivityAttributes>
    init(_ activity: Activity<RecordingActivityAttributes>) { self.activity = activity }
    var noteID: UUID { activity.attributes.noteID }
    var id: String { activity.id }
    var activityState: ActivityState { activity.activityState }
    var isPreview: Bool { activity.attributes.isPreview }
    var state: RecordingActivityAttributes.ContentState { activity.content.state }
    func update(_ state: RecordingActivityAttributes.ContentState, staleDate: Date?) async {
        await activity.update(ActivityContent(state: state, staleDate: staleDate))
    }
    func end(_ state: RecordingActivityAttributes.ContentState) async {
        await activity.end(ActivityContent(state: state, staleDate: nil), dismissalPolicy: .immediate)
    }
}

/// System renders the timer; mic meters never cause Live Activity updates.
@MainActor final class RecordingLiveActivityController {
    private let backend: any RecordingActivityBackend
    private let now: () -> Date
    private let uptime: () -> TimeInterval
    private let presentation: RecordingActivityAttributes.Presentation?
    private(set) var lastError: String?
    var activitiesEnabled: Bool { backend.enabled }
    var hasActivity: Bool { activity?.activityState == .active || activity?.activityState == .stale }
    var systemIdentifier: String? { activity?.id }
    var systemState: String { activity.map { String(describing: $0.activityState) } ?? "none" }
    private var activity: (any RecordingActivitySession)?
    private var latestState: RecordingActivityAttributes.ContentState?
    private var pending: Task<Void, Never>?
    private var gate = RecordingActivityUpdateGate()

    init(backend: (any RecordingActivityBackend)? = nil,
         now: @escaping () -> Date = { .now },
         uptime: @escaping () -> TimeInterval = { ProcessInfo.processInfo.systemUptime },
         presentation: RecordingActivityAttributes.Presentation? = nil) {
        self.backend = backend ?? SystemRecordingActivityBackend()
        self.now = now; self.uptime = uptime; self.presentation = presentation
    }

    static func endOrphanedActivities(backend: (any RecordingActivityBackend)? = nil, now: Date = .now, previewOnly: Bool = false) async {
        // Snapshot once: an activity started during an awaited orphan end is not an orphan.
        let existing = (backend ?? SystemRecordingActivityBackend()).activities
        for activity in existing where (!previewOnly || activity.isPreview) && activity.activityState != .ended && activity.activityState != .dismissed {
            var state = activity.state
            state.phase = .ended; state.updatedAt = now
            await activity.end(state)
        }
    }

    func endPreviewActivities() async { await Self.endOrphanedActivities(backend: backend, previewOnly: true) }

    func start(noteID: UUID, title: String, elapsed: TimeInterval) {
        lastError = nil
        if activity?.noteID == noteID {
            // Chunk rotation is still one recording. Re-requesting from the background
            // is disallowed by ActivityKit and would remove the recording indicator.
            update(phase: .recording, elapsed: elapsed, force: true)
            return
        }
        end()
        guard backend.enabled else { lastError = "Live Activities are disabled in Settings."; return }
        let state = contentState(phase: .recording, elapsed: elapsed)
        do {
            activity = try backend.request(noteID: noteID, state: state, staleDate: state.updatedAt.addingTimeInterval(75), presentation: presentation)
            latestState = state
            _ = gate.shouldUpdate(at: uptime(), force: true)
        } catch {
            // Recording remains valid when Live Activities are unavailable or disabled.
            activity = nil; latestState = nil
            lastError = "The system could not start this Live Activity. Open the app in the foreground and check Live Activity settings."
        }
    }

    func pause(elapsed: TimeInterval) { update(phase: .paused, elapsed: elapsed, force: true) }
    func resume(elapsed: TimeInterval) { update(phase: .recording, elapsed: elapsed, force: true) }
    func heartbeat(elapsed: TimeInterval) {
        guard let phase = latestState?.phase else { return }
        update(phase: phase, elapsed: elapsed, force: false)
    }

    func end(elapsed: TimeInterval? = nil) {
        guard let activity else { return }
        self.activity = nil
        // OS content may lag pending updates; the local state has the final elapsed time.
        var state = latestState ?? activity.state
        if let elapsed { state.elapsed = elapsed.isFinite ? max(0, elapsed) : 0 }
        state.phase = .ended; state.updatedAt = now()
        latestState = nil
        let previous = pending
        pending = Task {
            await previous?.value
            await activity.end(state)
        }
    }

    /// Await queued ActivityKit operations without relying on arbitrary test sleeps.
    func finishPendingUpdates() async { await pending?.value }

    private func update(phase: RecordingActivityAttributes.ContentState.Phase, elapsed: TimeInterval, force: Bool) {
        guard let activity, gate.shouldUpdate(at: uptime(), force: force) else { return }
        let state = contentState(phase: phase, elapsed: elapsed)
        latestState = state
        let staleDate = phase == .recording ? state.updatedAt.addingTimeInterval(75) : nil
        let previous = pending
        pending = Task {
            await previous?.value
            await activity.update(state, staleDate: staleDate)
        }
    }

    private func contentState(phase: RecordingActivityAttributes.ContentState.Phase, elapsed: TimeInterval) -> RecordingActivityAttributes.ContentState {
        let date = now()
        let elapsed = elapsed.isFinite ? max(0, elapsed) : 0
        return .init(phase: phase, elapsed: elapsed, timerAnchor: date.addingTimeInterval(-elapsed), updatedAt: date)
    }
}

/// A running server capture takes precedence over the selected browsing mode,
/// but never over the authenticated principal to which that capture belongs.
enum RecordingActivityLinkPolicy {
    static func activeServerNoteID(url: URL, activeNoteID: String?, recordingIdentity: String?, accountIdentity: String?) -> UUID? {
        guard url.scheme == AppIdentity.urlScheme, url.host == "recording", url.user == nil, url.password == nil,
              url.query == nil, url.fragment == nil, url.pathComponents.count == 2,
              let id = UUID(uuidString: url.lastPathComponent),
              let recordingIdentity, !recordingIdentity.isEmpty, recordingIdentity == accountIdentity,
              activeNoteID.flatMap(UUID.init(uuidString:)) == id else { return nil }
        return id
    }
}
