import SwiftUI
import APIClient

@main
struct CerealApp: App {
    @State private var store = NoteStore()
    @State private var router = Router()
    @State private var accounts = AppAccounts.make()
    @State private var connectionMode = ConnectionMode()
    @State private var serverRecording = ServerRecordingController(outboxRoot: AppAccounts.outboxRoot)
    @State private var sharedHistory = SharedNoteHistory()

    var body: some Scene {
        WindowGroup {
            RootView()
                .environment(store)
                .environment(router)
                .environment(accounts)
                .environment(connectionMode)
                .environment(serverRecording)
                .environment(sharedHistory)
                .preferredColorScheme(.dark)
                .tint(Theme.accent)
                #if DEBUG
                .task { FrameLog.shared.start() }
                #endif
        }
    }
}

/// How the app's accounts are kept. There is one list for the whole app;
/// nothing in it is "current".
enum AppAccounts {
    /// Several accounts at once are switched off until every view that needs
    /// them is in. The switch gates only the ways to get a second account.
    static var allowsMultiple: Bool {
        #if DEBUG
        // Debug only: `-qa-accounts` runs with two accounts taken from the environment.
        if isTwoAccountRun { return true }
        #endif
        return false
    }

    #if DEBUG
    static var isTwoAccountRun: Bool { ProcessInfo.processInfo.arguments.contains("-qa-accounts") }
    #endif

    /// Where recordings are kept. Nil is the usual place; the two-account debug
    /// run has its own, so it never touches what the signed-in account keeps.
    static var outboxRoot: URL? {
        #if DEBUG
        if isTwoAccountRun {
            return FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
                .appendingPathComponent("qa-accounts-outbox", isDirectory: true)
        }
        #endif
        return nil
    }

    @MainActor static func make() -> AccountsModel {
        #if DEBUG
        if isTwoAccountRun {
            // Sessions in memory, a list of its own: nothing the real account saved is read or changed.
            let name = "dev.cereal.mobile.qa-accounts"
            UserDefaults().removePersistentDomain(forName: name)
            return AccountsModel(defaults: UserDefaults(suiteName: name) ?? .standard, credentials: MemoryAccountStore(), legacy: nil,
                                 storage: AccountStorage(root: outboxRoot), allowsMultiple: true)
        }
        #endif
        return AccountsModel(legacy: ConnectionSettingsStore(), allowsMultiple: allowsMultiple)
    }
}

enum Route: Hashable {
    case note(UUID)
    case chat(noteID: UUID?)
    case serverNote(NoteRef)
    case serverNoteSource(NoteRef, raw: Bool)
    /// A chat in one account: about a note of it, or across its meetings.
    case serverChat(AccountID, noteID: String?)
    /// Search across the notes of the local demo, or of the accounts.
    case search
    /// A read-only note someone shared. It is read with an account on its own server.
    case sharedNote(SharedNoteLink)
    /// The shared notes opened on this device.
    case sharedWithMe
    /// A link to a note on a server this app is not signed in to.
    case noteElsewhere(NoteLink)
}

/// Sources for zoom transitions. Only modal surfaces zoom; see RootView.
enum ZoomID: Hashable {
    /// The + button on Home, which grows into the recording surface.
    case capture
}

@MainActor
@Observable
final class Router {
    var path: [Route] = [] {
        didSet {
            if let first = path.first {
                origin = first
                release?.cancel(); release = nil
            } else if origin != nil, release == nil {
                // Let the pop finish, then release the row the user came from.
                release = Task { [weak self] in
                    try? await Task.sleep(for: .milliseconds(250))
                    guard let self, !Task.isCancelled else { return }
                    if self.path.isEmpty { self.origin = nil }
                    self.release = nil
                }
            }
        }
    }
    /// The first screen opened from Home. Its row stays softly lit while that
    /// screen is open, then the highlight drains away once Home is back.
    private(set) var origin: Route?
    @ObservationIgnored private var release: Task<Void, Never>?
    var newNoteID: UUID?
    /// The server note whose recording screen is presented. The screen is one
    /// cover at the root, whoever opened it; leaving it never stops the recording.
    var serverCapture: ServerCaptureTarget?
    var showMicSheet = false
    var showConnection = false
    /// The server address the connection sheet opens with, when a page asked
    /// for a sign-in to a particular server.
    @ObservationIgnored var signInAddress: String?
}

struct RootView: View {
    @Environment(NoteStore.self) private var store
    @Environment(Router.self) private var router
    @Environment(AccountsModel.self) private var accounts
    @Environment(ConnectionMode.self) private var connectionMode
    @Environment(ServerRecordingController.self) private var serverRecording
    @Environment(SharedNoteHistory.self) private var sharedHistory
    @Environment(\.openURL) private var openURL
    /// A link to a note of one's own that arrived before the saved session was
    /// restored. Where it leads depends on which server that session is for.
    @State private var pendingLink: AppLink?
    /// A tap on a recording's Live Activity that started the app. The note it
    /// leads to is opened once the session is back, so that the account's
    /// arrival does not close it again.
    @State private var pendingRecording: URL?
    #if DEBUG
    @State private var showLiveActivityQA = false
    @State private var debugScreenOpened = false
    #endif
    @Namespace private var zoom
    /// True until the saved session has been restored (or found absent) at launch.
    @State private var restoring = true

    var body: some View {
        @Bindable var router = router
        AdaptiveNotesNavigation(path: $router.path) {
            Group {
                if connectionMode.isDemo { HomeView() }
                else { ServerHomeView() }
            }
                .navigationDestination(for: Route.self) { route in
                    // Pushes keep the system transition. A zoom transition adds a
                    // pull-down-to-close gesture that cannot be turned off; it
                    // overrides pull-to-refresh in a note and hijacks the drag
                    // that dismisses the keyboard in chat.
                    switch route {
                    case .note(let id): NoteView(noteID: id)
                    case .chat(let id): ChatView(noteID: id)
                    case .serverNote(let ref): AccountScope(ref.account) { ServerNoteView(noteID: ref.id) }
                    case .serverNoteSource(let ref, let raw): AccountScope(ref.account) { ServerNoteView(noteID: ref.id, showMineInitially: raw) }
                    case .serverChat(let account, let id): AccountScope(account) { ServerChatView(noteID: id) }
                    case .search: SearchView()
                    case .sharedNote(let link): SharedNoteView(link: link)
                    case .sharedWithMe: SharedWithMeView()
                    case .noteElsewhere(let link): NoteElsewhereView(link: link)
                    }
                }
                .toolbarVisibility(.visible, for: .navigationBar)
        }
        .environment(\.zoomNamespace, zoom)
        .environment(\.restoringConnection, restoring)
        .overlay { SidebarDrawer() }
        .fullScreenCover(item: $router.newNoteID) { id in
            RailMetricsReader { NavigationStack { NewNoteView(noteID: id) } }
                // The + button is only on Home; resuming from a note uses the system cover.
                .zoomDestination(ZoomID.capture, enabled: router.path.isEmpty)
                .environment(\.zoomNamespace, zoom)
        }
        .task {
            // Clearing stale Live Activities must not hold up restoring the session.
            async let orphans: Void = RecordingLiveActivityController.endOrphanedActivities()
            #if DEBUG
            // QA only: `-qa.openURL <url>` hands the app a URL as the system does when a
            // link starts it, before the saved session is back. For a simulator in which
            // another app answers to the same scheme.
            if let url = UserDefaults.standard.string(forKey: "qa.openURL").flatMap(URL.init(string:)) { receive(url) }
            #endif
            // A recording must be stopped before its account can go, so its Live Activity never outlives it.
            accounts.removalBlock = { [serverRecording] in serverRecording.removalBlock(for: $0) }
            accounts.onRemoved = { [sharedHistory, accounts] record, erased in
                guard erased else { return }
                // What only that account could read goes with it.
                if let identity = record.identity { ServerChatSessions.shared.erase(accountIdentity: identity) }
                if let origin = record.origin, accounts.sessions(on: origin).isEmpty { sharedHistory.removeAll(on: origin) }
            }
            serverRecording.accountPresentation = { [accounts] id in
                guard accounts.showsAccountLabels, let session = accounts.session(id) else { return nil }
                return (accounts.label(for: id), session.host)
            }
            if await configureQAIfRequested() { restoring = false; openPendingLink(); await orphans; return }
            let interval = Signpost.ui.beginInterval("Launch.Restore")
            await accounts.restoreAll()
            Signpost.ui.endInterval("Launch.Restore", interval)
            restoring = false
            openPendingLink()
            await orphans
        }
        .fullScreenCover(item: $router.serverCapture) { target in
            RailMetricsReader { NavigationStack { AccountScope(target.ref.account) { ServerCaptureView(noteID: target.ref.id) } } }
                .zoomDestination(ZoomID.capture, enabled: CaptureNavigation.zoomsFromSource(path: router.path, note: target.ref))
                .environment(\.zoomNamespace, zoom)
        }
        .onChange(of: accounts.sessions.map(\.id)) { old, new in
            // Screens of an account that has gone are closed. Everything else stays where it is.
            let removed = Set(old).subtracting(new)
            guard !removed.isEmpty else { return }
            router.path = LinkNavigation.path(router.path, removing: removed)
            if let capture = router.serverCapture, removed.contains(capture.ref.account) { router.serverCapture = nil }
        }
        .task(id: accounts.signature) {
            // Answers still arriving for an account that is no longer here end.
            ServerChatSessions.shared.accountsChanged(to: Set(accounts.sessions.compactMap { $0.notes.accountIdentity }))
            #if DEBUG
            openDebugScreen(first: !debugScreenOpened)
            debugScreenOpened = true
            if ProcessInfo.processInfo.arguments.contains("server-chat-sample"), let account = accounts.defaultSession?.id {
                router.path = [.serverChat(account, noteID: nil)]
            }
            #endif
            await serverRecording.sync(with: accounts.sessions.map(\.notes))
            #if DEBUG
            await runQARecordingFlowIfRequested()
            #endif
        }
        .onOpenURL { receive($0) }
        // A universal link is the web form of the same links; one this app
        // cannot read goes on to the browser it was meant for.
        .onContinueUserActivity(NSUserActivityTypeBrowsingWeb) { activity in
            guard let url = activity.webpageURL else { return }
            if !open(url) { openURL(url) }
        }
        #if DEBUG
        .fullScreenCover(isPresented: $showLiveActivityQA) { LiveActivityQAHarness() }
        #endif
        .sheet(isPresented: $router.showConnection) { ConnectionView() }
        .sheet(isPresented: $router.showMicSheet) {
            MicPermissionSheet()
                .presentationDetents([.height(MicPermissionSheet.detentHeight)])
                .presentationDragIndicator(.visible)
                .presentationCornerRadius(32)
                .glassSheet()
                .zoomDestination(ZoomID.capture, enabled: router.path.isEmpty)
                .environment(\.zoomNamespace, zoom)
        }
    }
}

/// A server note's recording screen, as something to present.
struct ServerCaptureTarget: Identifiable, Hashable {
    let ref: NoteRef
    var id: NoteRef { ref }
    init(_ ref: NoteRef) { self.ref = ref }
}

/// A screen of one account. Everything inside asks that account's own model,
/// which is the only way to its server; when the account has gone, so has the screen.
struct AccountScope<Content: View>: View {
    @Environment(AccountsModel.self) private var accounts
    private let account: AccountID
    private let content: Content

    init(_ account: AccountID, @ViewBuilder content: () -> Content) {
        self.account = account
        self.content = content()
    }

    var body: some View {
        if let session = accounts.session(account) {
            content.environment(session.notes).environment(\.account, account)
        } else {
            ContentUnavailableView("This account is no longer on this iPhone", systemImage: "person.crop.circle.badge.xmark")
        }
    }
}

/// Where the screens around a recording lead. Pure, so the rules can be tested:
/// a recording ends only when the user stops it, never because a screen went away.
enum CaptureNavigation {
    /// The stack after asking a question from the recording screen: the note,
    /// then its chat, so Back from the chat lands on the note being recorded.
    static func pathForChat(from path: [Route], note: NoteRef) -> [Route] {
        var result = path
        if result.last == .serverChat(note.account, noteID: note.id) { return result }
        if result.last != .serverNote(note) { result.append(.serverNote(note)) }
        result.append(.serverChat(note.account, noteID: note.id))
        return result
    }

    /// The stack after closing the recording screen. While the recording runs
    /// the user returns to where they were. Once it has been stopped, closing
    /// lands on the note, where its notes are generated.
    static func pathAfterClosing(from path: [Route], note: NoteRef, recording: Bool) -> [Route] {
        recording || path.last == .serverNote(note) ? path : path + [.serverNote(note)]
    }

    /// The note's page reads its note again when the recording screen over it
    /// closes. What was typed there and the notes generated there were saved by
    /// the recording screen; the page behind it still held what it had loaded
    /// before, and went on offering "Generate notes" for notes that existed.
    static func noteReloads(note: NoteRef?, captureWas old: NoteRef?, captureIs new: NoteRef?) -> Bool {
        note != nil && old == note && new != note
    }

    /// The indicator stays while there is a recording to return to: running,
    /// paused, or ended by something other than the user (an interruption),
    /// which the recording screen explains and resumes from.
    static func showsIndicator(activeNoteID: String?, state: Recorder.State) -> Bool {
        guard activeNoteID != nil else { return false }
        switch state {
        case .recording, .paused, .failed: return true
        case .idle, .stopped: return false
        }
    }

    /// The recording screen grows out of the + button or the indicator, which
    /// are on Home and on the note being recorded.
    static func zoomsFromSource(path: [Route], note: NoteRef) -> Bool {
        path.isEmpty || path.last == .serverNote(note)
    }
}

extension RootView {
    /// A `cereal://` URL from the system: a note or shared-note link, or the
    /// Live Activity's link back to the note being recorded.
    fileprivate func receive(_ url: URL) {
        #if DEBUG
        if url.scheme == AppIdentity.urlScheme, url.host == "live-activity-preview" {
            showLiveActivityQA = true
            return
        }
        #endif
        if open(url) { return }
        guard let link = RecordingActivityLinkPolicy.link(url) else { return }
        if restoring { pendingRecording = url; return }
        // A server recording can stay active while the user browses the
        // local demo. Its Live Activity returns to its own account and note.
        switch RecordingActivityLinkPolicy.target(link, active: serverRecording.active, accounts: accounts.sessions.map(\.id),
                                                  ledger: { serverRecording.ledger(of: $0) }) {
        case .note(let ref) where serverRecording.active == ref || !connectionMode.isDemo:
            connectionMode.isDemo = false
            router.path = [.serverNote(ref)]
        case .note, .home:
            guard connectionMode.isDemo, store.note(link.noteID) != nil else {
                if !connectionMode.isDemo { router.path = [] }
                return
            }
            guard router.newNoteID != link.noteID else { return }
            // A Live Activity opens the existing note without starting or
            // resuming capture. An already presented capture stays intact.
            guard router.newNoteID == nil else { return }
            router.path = [.note(link.noteID)]
        }
    }

    /// Follows a shared-note or note link, in its web or `cereal://` form.
    /// False for any other URL, which is left to whoever handles it.
    fileprivate func open(_ url: URL) -> Bool {
        guard let link = AppLink(url) else { return false }
        switch link {
        // An answer to a sign-in goes to the sheet that asked; one arriving here has nobody waiting.
        case .auth: break
        case .note where restoring: pendingLink = link
        case .note, .shared: router.open(link, accounts: accounts, mode: connectionMode)
        }
        return true
    }

    fileprivate func openPendingLink() {
        if let url = pendingRecording { pendingRecording = nil; receive(url) }
        guard let link = pendingLink else { return }
        pendingLink = nil
        router.open(link, accounts: accounts, mode: connectionMode)
    }

    /// Explicit Debug-only QA opt-in. Values are never logged, placed in
    /// arguments, or written to a provisioning file. Successful connection
    /// follows the normal Keychain flow used by the settings screen.
    fileprivate func configureQAIfRequested() async -> Bool {
        #if DEBUG
        let arguments = ProcessInfo.processInfo.arguments
        if arguments.contains("-qa-live-activity") {
            connectionMode.isDemo = true
            showLiveActivityQA = true
            return true
        }
        if arguments.contains("-qa-connect") {
            connectionMode.isDemo = false
            let environment = ProcessInfo.processInfo.environment
            await accounts.restoreAll()
            let account = try? await accounts.connect(instance: environment["CEREAL_QA_INSTANCE_URL"] ?? "", token: environment["CEREAL_QA_OWNER_TOKEN"] ?? "")
            if let account, let id = environment["CEREAL_QA_NOTE_ID"], UUID(uuidString: id) != nil {
                router.path = [.serverNote(NoteRef(account: account, id: id))]
            }
            return true
        }
        if arguments.contains("-qa-accounts") {
            // Two accounts, for the first slice of several: `CEREAL_QA_INSTANCE_URL`/`_OWNER_TOKEN`
            // and the same with `_2`. `-qa.newNoteAccount 2` makes the second the one new notes go to.
            connectionMode.isDemo = false
            let environment = ProcessInfo.processInfo.environment
            _ = try? await accounts.connect(instance: environment["CEREAL_QA_INSTANCE_URL"] ?? "", token: environment["CEREAL_QA_OWNER_TOKEN"] ?? "")
            let second = try? await accounts.connect(instance: environment["CEREAL_QA_INSTANCE_URL_2"] ?? "", token: environment["CEREAL_QA_OWNER_TOKEN_2"] ?? "")
            if UserDefaults.standard.integer(forKey: "qa.newNoteAccount") == 2, let second { accounts.setDefault(second) }
            return true
        }
        guard let index = arguments.firstIndex(of: "-screen"), index + 1 < arguments.count else { return false }
        let state: ServerNotesState
        let label: String
        switch arguments[index + 1] {
        case "server-loading": state = .loading(previous: []); label = "QA preview · loading"
        case "server-error": state = .failed(.unavailable, previous: []); label = "QA preview · connection error"
        case "server-empty": state = .loaded([]); label = "QA preview · empty notes"
        case "server-chat-sample": state = .loaded([]); label = "QA preview · sample chat"
        default: return false
        }
        connectionMode.isDemo = false
        connectionMode.previewLabel = label
        accounts.showQAState(state)
        if arguments[index + 1] == "server-chat-sample", let account = accounts.defaultSession?.id { router.path = [.serverChat(account, noteID: nil)] }
        return true
        #else
        return false
        #endif
    }

    #if DEBUG
    /// QA only, with `-qa-connect`: `-qa.flow record` creates a note and opens its
    /// recording screen; `record-home`, `record-note` and `record-chat` then leave that
    /// screen after `-qa.leaveAfter` seconds for Home, the note or the note's chat, with
    /// the recording still running. For displays that simulated touches cannot reach.
    fileprivate func runQARecordingFlowIfRequested() async {
        guard let session = accounts.defaultSession, session.isReady, router.serverCapture == nil, serverRecording.indicator == nil,
              let flow = UserDefaults.standard.string(forKey: "qa.flow"), flow.hasPrefix("record"),
              let created = try? await session.notes.create(title: "", body: "") else { return }
        let note = NoteRef(account: session.id, id: created.id)
        router.serverCapture = ServerCaptureTarget(note)
        guard flow != "record" else { return }
        let delay = max(2, UserDefaults.standard.double(forKey: "qa.leaveAfter"))
        try? await Task.sleep(for: .seconds(delay))
        switch flow {
        case "record-note": router.path = [.serverNote(note)]
        case "record-chat": router.path = CaptureNavigation.pathForChat(from: [], note: note)
        default: break
        }
        router.serverCapture = nil
    }
    #endif

    /// `-screen note|chat|search|sidebar|new|mic` deep-links for screenshots and UI tests.
    /// Navigation targets are re-applied when the account identity settles;
    /// targets that create a note or present a sheet run once.
    fileprivate func openDebugScreen(first isFirst: Bool) {
        let args = ProcessInfo.processInfo.arguments
        guard let i = args.firstIndex(of: "-screen"), i + 1 < args.count else { return }
        let target = args[i + 1]
        guard ["home", "note", "chat", "search", "sidebar", "new", "mic", "regenerate"].contains(target) else { return }
        guard isFirst || ["note", "chat", "search", "sidebar"].contains(target) else { return }
        connectionMode.isDemo = true
        let first = store.activeNotes.first { !$0.sections.isEmpty }?.id
        switch target {
        case "note": if let first { router.path = [.note(first)] }
        case "chat":
            if let first { router.path = [.note(first), .chat(noteID: first)] }
        case "search": router.path = [.search]
        case "sidebar": router.path = []; SidebarModel.shared.open()
        case "new": router.newNoteID = store.createNote().id
        case "mic": router.showMicSheet = true
        case "regenerate":
            // Start → Stop → Generate → Resume → Stop: the note should offer "Regenerate notes".
            var note = store.createNote()
            note.title = "Design review"
            note.userNotes = "Ship the capture pill\nMove chat history behind the rail"
            store.update(note)
            store.finishCapture(note.id, segment: CaptureSegment(index: 0, startedAt: .now, duration: 95, fileName: "segment-0.wav"))
            Task {
                await store.generateNotes(note.id)
                store.finishCapture(note.id, segment: CaptureSegment(index: 1, startedAt: .now, duration: 40, fileName: "segment-1.wav"))
                router.path = [.note(note.id)]
            }
        default: break
        }
    }
}

extension EnvironmentValues {
    @Entry var restoringConnection = false
    /// The account of the screen this view is part of. Nil outside an account's screens.
    @Entry var account: AccountID? = nil
}

extension UUID: @retroactive Identifiable {
    public var id: UUID { self }
}
