// ActivityKit's Activity reference predates Sendable annotations. All operations are ordered
// through the main actor and the pending-task chain; no concurrent updates are issued.
@preconcurrency import ActivityKit
import Foundation
import APIClient

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
    func request(noteID: UUID, accountID: String?, state: RecordingActivityAttributes.ContentState, staleDate: Date, presentation: RecordingActivityAttributes.Presentation?) throws -> any RecordingActivitySession
}

/// The account a recording goes to, as its Live Activity may show it. The
/// label and host are given only when there are several accounts.
struct RecordingActivityAccount: Equatable {
    let id: String
    var label: String?
    var host: String?
}

@MainActor private struct SystemRecordingActivityBackend: RecordingActivityBackend {
    var enabled: Bool { ActivityAuthorizationInfo().areActivitiesEnabled }
    var activities: [any RecordingActivitySession] { Activity<RecordingActivityAttributes>.activities.map(SystemRecordingActivitySession.init) }
    func request(noteID: UUID, accountID: String?, state: RecordingActivityAttributes.ContentState, staleDate: Date, presentation: RecordingActivityAttributes.Presentation?) throws -> any RecordingActivitySession {
        SystemRecordingActivitySession(try Activity.request(attributes: RecordingActivityAttributes(noteID: noteID, presentation: presentation, accountID: accountID),
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
    /// The recorded note's title as the activity shows it. Nil for a note without one.
    private var title: String?
    /// The account of the note being recorded. Nil for the local demo.
    private var account: RecordingActivityAccount?
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

    /// An empty `title` means "as it was": a resume or a new part of the same
    /// recording does not know the title and must not blank it.
    func start(noteID: UUID, title: String, elapsed: TimeInterval, account: RecordingActivityAccount? = nil) {
        lastError = nil
        if activity?.noteID != noteID || account != nil { self.account = account }
        let shown = RecordingActivityAttributes.ContentState.shownTitle(title)
        if activity?.noteID == noteID {
            if let shown { self.title = shown }
            // Chunk rotation is still one recording. Re-requesting from the background
            // is disallowed by ActivityKit and would remove the recording indicator.
            update(phase: .recording, elapsed: elapsed, force: true)
            return
        }
        end()
        self.title = shown
        guard backend.enabled else { lastError = "Live Activities are disabled in Settings."; return }
        let state = contentState(phase: .recording, elapsed: elapsed)
        do {
            activity = try backend.request(noteID: noteID, accountID: account?.id, state: state, staleDate: state.updatedAt.addingTimeInterval(75), presentation: presentation)
            latestState = state
            _ = gate.shouldUpdate(at: uptime(), force: true)
        } catch {
            // Recording remains valid when Live Activities are unavailable or disabled.
            activity = nil; latestState = nil
            lastError = "The system could not start this Live Activity. Open the app in the foreground and check Live Activity settings."
        }
    }

    /// The note was given a title, or a different one, while it is being recorded.
    /// The timer is untouched: only the name on the activity changes.
    func setTitle(_ text: String) {
        let shown = RecordingActivityAttributes.ContentState.shownTitle(text)
        guard shown != title else { return }
        title = shown
        guard let activity, var state = latestState else { return }
        state.title = shown; state.updatedAt = now()
        latestState = state
        let staleDate = state.phase == .recording ? state.updatedAt.addingTimeInterval(75) : nil
        let previous = pending
        pending = Task {
            await previous?.value
            await activity.update(state, staleDate: staleDate)
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
        return .init(phase: phase, elapsed: elapsed, timerAnchor: date.addingTimeInterval(-elapsed), updatedAt: date, title: title,
                     accountLabel: account?.label, accountHost: account?.host)
    }
}

/// Where a tap on a recording's Live Activity leads. Pure, so the rules can be
/// tested. The link only ever opens a note: it starts, stops and sends nothing.
enum RecordingActivityLinkPolicy {
    struct Link: Equatable {
        let noteID: UUID
        /// The account the activity named. Nil in a link from before accounts.
        let account: AccountID?
    }

    enum Target: Equatable {
        case note(NoteRef)
        /// Nothing here matches the link. Home, or the local demo's own note of that id.
        case home
    }

    /// `cereal://recording/<note uuid>` with at most one parameter, `a`, of 64
    /// lower-case hex characters. Anything else is not a recording link.
    static func link(_ url: URL) -> Link? {
        guard url.scheme == AppIdentity.urlScheme, url.host == "recording", url.user == nil, url.password == nil,
              url.port == nil, url.fragment == nil, url.pathComponents.count == 2,
              let id = UUID(uuidString: url.lastPathComponent),
              let components = URLComponents(url: url, resolvingAgainstBaseURL: false) else { return nil }
        guard components.query != nil else { return Link(noteID: id, account: nil) }
        guard let items = components.queryItems, items.count == 1, items[0].name == "a",
              let account = items[0].value.flatMap(AccountID.init(_:)), components.percentEncodedQuery == "a=" + account.raw else { return nil }
        return Link(noteID: id, account: account)
    }

    /// - The recording in progress, when the link is for its note and names its
    ///   account or none: a running recording comes before anything else.
    /// - Otherwise the note in the account the link names, if that account is here.
    /// - A link without an account is looked up in each account's capture ledger.
    /// - Otherwise Home.
    static func target(_ link: Link, active: NoteRef?, accounts: [AccountID], ledger: (AccountID) -> Set<String>) -> Target {
        let id = link.noteID.uuidString.lowercased()
        if let active, UUID(uuidString: active.id) == link.noteID, link.account == nil || link.account == active.account {
            return .note(active)
        }
        if let account = link.account {
            return accounts.contains(account) ? .note(NoteRef(account: account, id: id)) : .home
        }
        if let owner = accounts.first(where: { ledger($0).contains(id) }) { return .note(NoteRef(account: owner, id: id)) }
        return .home
    }
}
