// ActivityKit's Activity reference predates Sendable annotations. All operations are ordered
// through the main actor and the pending-task chain; no concurrent updates are issued.
@preconcurrency import ActivityKit
import Foundation
import APIClient
import UIKit

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
    /// An update that lights up the screen with a sound. Only for a recording that stopped on its own.
    func update(_ state: RecordingActivityAttributes.ContentState, staleDate: Date?, alert: RecordingActivityAlert?) async
    func end(_ state: RecordingActivityAttributes.ContentState, dismissal: RecordingActivityDismissal) async
}

extension RecordingActivitySession {
    func update(_ state: RecordingActivityAttributes.ContentState, staleDate: Date?, alert: RecordingActivityAlert?) async {
        await update(state, staleDate: staleDate)
    }
    func end(_ state: RecordingActivityAttributes.ContentState, dismissal: RecordingActivityDismissal) async { await end(state) }
}

@MainActor protocol RecordingActivityBackend {
    var enabled: Bool { get }
    var activities: [any RecordingActivitySession] { get }
    func request(noteID: UUID, accountID: String?, state: RecordingActivityAttributes.ContentState, staleDate: Date, presentation: RecordingActivityAttributes.Presentation?) throws -> any RecordingActivitySession
}

/// Time the app may keep running after it leaves the screen, while it follows
/// the notes being written for the activity. Injectable so tests can expire it.
@MainActor protocol RecordingActivityBackgroundTime {
    func begin(expiration: @escaping @MainActor () -> Void) -> Int?
    func end(_ token: Int)
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
            content: ActivityContent(state: state, staleDate: staleDate, relevanceScore: state.relevance), pushType: nil))
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
        await update(state, staleDate: staleDate, alert: nil)
    }
    func update(_ state: RecordingActivityAttributes.ContentState, staleDate: Date?, alert: RecordingActivityAlert?) async {
        await activity.update(ActivityContent(state: state, staleDate: staleDate, relevanceScore: state.relevance), alertConfiguration: alert?.configuration)
    }
    func end(_ state: RecordingActivityAttributes.ContentState) async { await end(state, dismissal: .immediate) }
    func end(_ state: RecordingActivityAttributes.ContentState, dismissal: RecordingActivityDismissal) async {
        await activity.end(ActivityContent(state: state, staleDate: nil, relevanceScore: state.relevance), dismissalPolicy: dismissal.policy)
    }
}

@MainActor private struct SystemBackgroundTime: RecordingActivityBackgroundTime {
    func begin(expiration: @escaping @MainActor () -> Void) -> Int? {
        let identifier = UIApplication.shared.beginBackgroundTask(withName: "Recording notes") {
            MainActor.assumeIsolated { expiration() }
        }
        return identifier == .invalid ? nil : identifier.rawValue
    }
    func end(_ token: Int) { UIApplication.shared.endBackgroundTask(UIBackgroundTaskIdentifier(rawValue: token)) }
}

/// How the notes written after End turned out, as the activity may say it.
enum RecordingNotesOutcome: Equatable, Sendable {
    /// The notes are written. The only outcome that says "Notes ready".
    case ready
    /// Nothing was transcribed, so there is nothing to write.
    case noTranscript
    /// Writing them failed.
    case failed
    /// The app will write them later (the Stop has not reached the server yet).
    case deferred

    var hint: RecordingActivityAttributes.ContentState.Hint? {
        switch self {
        case .ready: nil
        case .noTranscript: .noTranscript
        case .failed: .notesFailed
        case .deferred: .notesInApp
        }
    }
}

/// What becomes of the activity when a recording stops.
enum RecordingActivityEnding: Equatable, Sendable {
    /// "Recording ended", for a moment. Nothing follows here.
    case ended
    /// "Writing notes...", until the app knows how the notes turned out.
    case writingNotes
    /// Gone at once: the recording was cancelled and its note removed.
    case discard
}

/// Rules for which of the app's recording activities may stand beside a new one.
enum RecordingActivityExclusivity {
    /// What is left of an earlier recording (notes being written, a stop that
    /// asked for the app) gives way to a new recording; a running one is never
    /// ended from here.
    static func givesWay(_ phase: RecordingActivityAttributes.ContentState.Phase) -> Bool {
        switch phase {
        case .writing, .interrupted, .ready, .ended: true
        case .recording, .paused: false
        }
    }
}

/// System renders the timer; mic meters never cause Live Activity updates.
@MainActor final class RecordingLiveActivityController {
    private let backend: any RecordingActivityBackend
    private let backgroundTime: any RecordingActivityBackgroundTime
    private let now: () -> Date
    private let uptime: () -> TimeInterval
    private let appActive: @MainActor () -> Bool
    private let readyHold: Duration
    private let presentation: RecordingActivityAttributes.Presentation?
    private(set) var lastError: String?
    var activitiesEnabled: Bool { backend.enabled }
    var hasActivity: Bool { activity?.activityState == .active || activity?.activityState == .stale }
    var systemIdentifier: String? { activity?.id }
    var systemState: String { activity.map { String(describing: $0.activityState) } ?? "none" }
    /// What the activity says now, as this app last set it.
    var phase: RecordingActivityAttributes.ContentState.Phase? { activity == nil ? nil : latestState?.phase }
    var noteID: UUID? { activity?.noteID }
    private var activity: (any RecordingActivitySession)?
    private var latestState: RecordingActivityAttributes.ContentState?
    /// The recorded note's title as the activity shows it. Nil for a note without one.
    private var title: String?
    /// The account of the note being recorded. Nil for the local demo.
    private var account: RecordingActivityAccount?
    private var pending: Task<Void, Never>?
    private var gate = RecordingActivityUpdateGate()
    /// Background time held while the notes are followed after End.
    private var following: Int?

    init(backend: (any RecordingActivityBackend)? = nil,
         backgroundTime: (any RecordingActivityBackgroundTime)? = nil,
         now: @escaping () -> Date = { .now },
         uptime: @escaping () -> TimeInterval = { ProcessInfo.processInfo.systemUptime },
         appActive: @escaping @MainActor () -> Bool = { UIApplication.shared.applicationState == .active },
         readyHold: Duration = RecordingActivityTiming.readyHold,
         presentation: RecordingActivityAttributes.Presentation? = nil) {
        self.backend = backend ?? SystemRecordingActivityBackend()
        self.backgroundTime = backgroundTime ?? SystemBackgroundTime()
        self.now = now; self.uptime = uptime; self.appActive = appActive; self.readyHold = readyHold; self.presentation = presentation
    }

    static func endOrphanedActivities(backend: (any RecordingActivityBackend)? = nil, now: Date = .now, previewOnly: Bool = false) async {
        await end(leftoverActivities(backend: backend, previewOnly: previewOnly), now: now)
    }

    /// The activities there are now, taken at once. An activity started after this
    /// (a recording, the QA harness) is not a leftover, however soon after launch.
    static func leftoverActivities(backend: (any RecordingActivityBackend)? = nil, previewOnly: Bool = false) -> [any RecordingActivitySession] {
        (backend ?? SystemRecordingActivityBackend()).activities.filter {
            (!previewOnly || $0.isPreview) && $0.activityState != .ended && $0.activityState != .dismissed
        }
    }

    static func end(_ leftovers: [any RecordingActivitySession], now: Date = .now) async {
        for activity in leftovers {
            var state = activity.state
            state.phase = .ended; state.updatedAt = now
            await activity.end(state, dismissal: .immediate)
        }
    }

    func endPreviewActivities() async { await Self.endOrphanedActivities(backend: backend, previewOnly: true) }

    /// An empty `title` means "as it was": a resume or a new part of the same
    /// recording does not know the title and must not blank it.
    func start(noteID: UUID, title: String, elapsed: TimeInterval, account: RecordingActivityAccount? = nil) {
        lastError = nil
        if activity?.noteID != noteID || account != nil { self.account = account }
        let shown = RecordingActivityAttributes.ContentState.shownTitle(title)
        if let activity, activity.noteID == noteID, activity.activityState == .active || activity.activityState == .stale {
            if let shown { self.title = shown }
            stopFollowing()
            // Chunk rotation is still one recording, and so is a resume after a stop or
            // an interruption. Re-requesting from the background is disallowed by
            // ActivityKit and would remove the recording indicator.
            update(phase: .recording, elapsed: elapsed, force: true)
            return
        }
        finish(discarded: true)
        self.title = shown
        guard backend.enabled else { lastError = "Live Activities are disabled in Settings."; return }
        endLeftovers(except: noteID)
        let state = contentState(phase: .recording, elapsed: elapsed)
        do {
            activity = try backend.request(noteID: noteID, accountID: account?.id, state: state, staleDate: state.staleDate(from: state.updatedAt)!, presentation: presentation)
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
        guard var state = latestState, activity != nil else { return }
        state.title = shown; state.updatedAt = now()
        send(state)
    }

    func pause(elapsed: TimeInterval) { update(phase: .paused, elapsed: elapsed, force: true) }
    func resume(elapsed: TimeInterval) { update(phase: .recording, elapsed: elapsed, force: true) }
    func heartbeat(elapsed: TimeInterval) {
        // Only a recording has a clock to keep from going stale.
        guard let phase = latestState?.phase, phase == .recording || phase == .paused else { return }
        update(phase: phase, elapsed: elapsed, force: false)
    }

    /// The recording stopped without being asked to. Out of the app the activity
    /// stays, says so and offers the note (with an alert when the person needs to
    /// act); in the app, where the screen says so, it ends after a short while.
    func interrupt(elapsed: TimeInterval, cause: RecordingActivityAlert.Cause) {
        guard activity != nil, latestState.map({ $0.phase == .recording || $0.phase == .paused }) ?? false else { return }
        var state = contentState(phase: .interrupted, elapsed: elapsed)
        state.hint = .openToResume
        let active = appActive()
        if active {
            state.phase = .ended
            finish(state)
        } else {
            send(state, alert: RecordingActivityAlert.forStop(cause, appActive: false))
        }
    }

    /// The app is in front again. A recording that stopped on its own has been
    /// seen on its screen there, so its activity has said what it had to.
    func appBecameActive(isCapturing: Bool) {
        guard !isCapturing, latestState?.phase == .interrupted else { return }
        finish(discarded: true)
    }

    /// The recording ended (End, Stop, Cancel). `ending` says what the activity does next.
    func end(elapsed: TimeInterval? = nil, ending: RecordingActivityEnding = .ended) {
        guard let activity else { return }
        // OS content may lag pending updates; the local state has the final elapsed time.
        var state = latestState ?? activity.state
        if let elapsed { state.elapsed = elapsed.isFinite ? max(0, elapsed) : 0 }
        state.timerAnchor = now().addingTimeInterval(-state.elapsed)
        state.updatedAt = now()
        switch ending {
        case .discard:
            finish(discarded: true, elapsed: state.elapsed)
        case .ended:
            // Writing, ready or an interruption already say more than "ended".
            guard state.phase == .recording || state.phase == .paused || state.phase == .interrupted else { return }
            state.phase = .ended; state.hint = nil
            finish(state)
        case .writingNotes:
            guard state.phase != .writing else { return }
            state.phase = .writing; state.hint = nil
            send(state)
            startFollowing()
        }
    }

    /// How the notes written after End turned out. Only "ready" says ready.
    func notesFinished(noteID: UUID, outcome: RecordingNotesOutcome) {
        guard let activity, activity.noteID == noteID, var state = latestState, state.phase == .writing else { return }
        state.updatedAt = now()
        switch outcome {
        case .ready:
            state.phase = .ready; state.hint = nil
            // Seen in the Dynamic Island for a moment, then left on the Lock Screen for a few minutes.
            self.activity = nil; latestState = nil
            let previous = pending, hold = readyHold, now = now
            let token = following; following = nil
            pending = Task { [backgroundTime] in
                await previous?.value
                await activity.update(state, staleDate: nil, alert: nil)
                if hold > .zero { try? await Task.sleep(for: hold) }
                await activity.end(state, dismissal: RecordingActivityDismissal.forEnding(state, discarded: false, now: now()))
                if let token { backgroundTime.end(token) }
            }
        case .noTranscript, .failed, .deferred:
            state.phase = .ended; state.hint = outcome.hint
            finish(state)
        }
    }

    /// Await queued ActivityKit operations without relying on arbitrary test sleeps.
    func finishPendingUpdates() async { await pending?.value }

    // MARK: Internals

    private func update(phase: RecordingActivityAttributes.ContentState.Phase, elapsed: TimeInterval, force: Bool) {
        guard activity != nil, gate.shouldUpdate(at: uptime(), force: force) else { return }
        send(contentState(phase: phase, elapsed: elapsed))
    }

    /// Queues an update of the activity; updates and ends apply in the order they were made.
    private func send(_ state: RecordingActivityAttributes.ContentState, alert: RecordingActivityAlert? = nil) {
        guard let activity else { return }
        latestState = state
        let staleDate = state.staleDate(from: state.updatedAt)
        let previous = pending
        pending = Task {
            await previous?.value
            await activity.update(state, staleDate: staleDate, alert: alert)
        }
    }

    /// Ends the activity with `state`, kept on the Lock Screen as long as it says something true.
    private func finish(_ state: RecordingActivityAttributes.ContentState) {
        guard let activity else { return }
        self.activity = nil; latestState = nil
        stopFollowing()
        let dismissal = RecordingActivityDismissal.forEnding(state, discarded: false, now: now())
        let previous = pending
        pending = Task {
            await previous?.value
            await activity.end(state, dismissal: dismissal)
        }
    }

    /// Ends the activity at once: replaced, cancelled, or seen in the app.
    private func finish(discarded: Bool, elapsed: TimeInterval? = nil) {
        guard let activity else { return }
        var state = latestState ?? activity.state
        if let elapsed { state.elapsed = elapsed }
        state.phase = .ended; state.updatedAt = now()
        self.activity = nil; latestState = nil
        stopFollowing()
        let previous = pending
        pending = Task {
            await previous?.value
            await activity.end(state, dismissal: .immediate)
        }
    }

    /// Activities of earlier recordings that are only saying something about
    /// them give way to a new recording, so there is one recording activity.
    private func endLeftovers(except noteID: UUID) {
        guard presentation == nil else { return }
        let leftovers = backend.activities.filter { other in
            !other.isPreview && (other.activityState == .active || other.activityState == .stale)
                && RecordingActivityExclusivity.givesWay(other.state.phase)
        }
        guard !leftovers.isEmpty else { return }
        let date = now(), previous = pending
        pending = Task {
            await previous?.value
            for other in leftovers {
                var state = other.state
                state.phase = .ended; state.updatedAt = date
                await other.end(state, dismissal: .immediate)
            }
        }
    }

    /// Keeps the app running a little after it leaves the screen, so the notes'
    /// outcome reaches the activity. If that time runs out first, the activity
    /// says the notes are in the app rather than staying on "Writing notes...".
    private func startFollowing() {
        guard following == nil else { return }
        following = backgroundTime.begin { [weak self] in self?.lostTrack() }
    }

    private func stopFollowing() {
        guard let token = following else { return }
        following = nil
        backgroundTime.end(token)
    }

    private func lostTrack() {
        guard var state = latestState, state.phase == .writing else { stopFollowing(); return }
        state.phase = .ended; state.hint = .notesInApp; state.updatedAt = now()
        finish(state)
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
