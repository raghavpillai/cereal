import SwiftUI
import APIClient

@main
struct CerealApp: App {
    @State private var store = NoteStore()
    @State private var router = Router()
    @State private var server = ServerNotesModel()
    @State private var connectionMode = ConnectionMode()
    @State private var serverRecording = ServerRecordingController()

    var body: some Scene {
        WindowGroup {
            RootView()
                .environment(store)
                .environment(router)
                .environment(server)
                .environment(connectionMode)
                .environment(serverRecording)
                .preferredColorScheme(.dark)
                .tint(Theme.accent)
        }
    }
}

enum Route: Hashable {
    case note(UUID)
    case chat(noteID: UUID?)
    case serverNote(String)
    case serverNoteSource(String, raw: Bool)
    case serverChat(String?)
    /// Search across the notes of the current mode (local demo or signed-in account).
    case search
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
}

struct RootView: View {
    @Environment(NoteStore.self) private var store
    @Environment(Router.self) private var router
    @Environment(ServerNotesModel.self) private var server
    @Environment(ConnectionMode.self) private var connectionMode
    @Environment(ServerRecordingController.self) private var serverRecording
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
                    case .serverNote(let id): ServerNoteView(noteID: id)
                    case .serverNoteSource(let id, let raw): ServerNoteView(noteID: id, showMineInitially: raw)
                    case .serverChat(let id): ServerChatView(noteID: id)
                    case .search: SearchView()
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
            if await configureQAIfRequested() { restoring = false; await orphans; return }
            let interval = Signpost.ui.beginInterval("Launch.Restore")
            await server.restore()
            Signpost.ui.endInterval("Launch.Restore", interval)
            restoring = false
            await orphans
        }
        .fullScreenCover(item: $router.serverCapture) { target in
            RailMetricsReader { NavigationStack { ServerCaptureView(noteID: target.id) } }
                .zoomDestination(ZoomID.capture, enabled: CaptureNavigation.zoomsFromSource(path: router.path, noteID: target.id))
                .environment(\.zoomNamespace, zoom)
        }
        .task(id: server.accountIdentity) {
            router.path = []; router.newNoteID = nil; router.serverCapture = nil
            // Answers still arriving for an account that is no longer signed in end here.
            ServerChatSessions.shared.accountChanged(to: server.accountIdentity)
            #if DEBUG
            // After the reset above, so a `-screen` deep link is not cleared by it.
            openDebugScreen(first: !debugScreenOpened)
            debugScreenOpened = true
            if ProcessInfo.processInfo.arguments.contains("server-chat-sample") { router.path = [.serverChat(nil)] }
            #endif
            await serverRecording.attach(to: server)
            #if DEBUG
            await runQARecordingFlowIfRequested()
            #endif
        }
        .onOpenURL { url in
            #if DEBUG
            if url.scheme == AppIdentity.urlScheme, url.host == "live-activity-preview" {
                showLiveActivityQA = true
                return
            }
            #endif
            guard url.scheme == AppIdentity.urlScheme, url.host == "recording" else { return }
            // A server recording can stay active while the user browses the
            // local demo. Its Live Activity must return to its own account/note.
            if let id = RecordingActivityLinkPolicy.activeServerNoteID(url: url,
                activeNoteID: serverRecording.activeNoteID,
                recordingIdentity: serverRecording.instanceIdentity,
                accountIdentity: server.accountIdentity) {
                connectionMode.isDemo = false
                router.path = [.serverNote(id.uuidString.lowercased())]
                return
            }
            if !connectionMode.isDemo, let id = UUID(uuidString: url.lastPathComponent) {
                router.path = [.serverNote(id.uuidString.lowercased())]
                return
            }
            guard connectionMode.isDemo,
                  let id = UUID(uuidString: url.lastPathComponent), store.note(id) != nil else { return }
            guard router.newNoteID != id else { return }
            // A Live Activity opens the existing note without starting or
            // resuming capture. An already presented capture stays intact.
            guard router.newNoteID == nil else { return }
            router.path = [.note(id)]
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
    let id: String
}

/// Where the screens around a recording lead. Pure, so the rules can be tested:
/// a recording ends only when the user stops it, never because a screen went away.
enum CaptureNavigation {
    /// The stack after asking a question from the recording screen: the note,
    /// then its chat, so Back from the chat lands on the note being recorded.
    static func pathForChat(from path: [Route], noteID: String) -> [Route] {
        var result = path
        if result.last == .serverChat(noteID) { return result }
        if result.last != .serverNote(noteID) { result.append(.serverNote(noteID)) }
        result.append(.serverChat(noteID))
        return result
    }

    /// The stack after closing the recording screen. While the recording runs
    /// the user returns to where they were. Once it has been stopped, closing
    /// lands on the note, where its notes are generated.
    static func pathAfterClosing(from path: [Route], noteID: String, recording: Bool) -> [Route] {
        recording || path.last == .serverNote(noteID) ? path : path + [.serverNote(noteID)]
    }

    /// The note's page reads its note again when the recording screen over it
    /// closes. What was typed there and the notes generated there were saved by
    /// the recording screen; the page behind it still held what it had loaded
    /// before, and went on offering "Generate notes" for notes that existed.
    static func noteReloads(noteID: String, captureWas old: String?, captureIs new: String?) -> Bool {
        old == noteID && new != noteID
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
    static func zoomsFromSource(path: [Route], noteID: String) -> Bool {
        path.isEmpty || path.last == .serverNote(noteID)
    }
}

extension RootView {
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
            await server.connect(instance: environment["CEREAL_QA_INSTANCE_URL"] ?? "", token: environment["CEREAL_QA_OWNER_TOKEN"] ?? "")
            if server.instance != nil, let id = environment["CEREAL_QA_NOTE_ID"], UUID(uuidString: id) != nil {
                router.path = [.serverNote(id)]
            }
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
        server.showQAState(state)
        if arguments[index + 1] == "server-chat-sample" { router.path = [.serverChat(nil)] }
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
        guard server.instance != nil, router.serverCapture == nil, serverRecording.indicatorNoteID == nil,
              let flow = UserDefaults.standard.string(forKey: "qa.flow"), flow.hasPrefix("record"),
              let note = try? await server.create(title: "", body: "") else { return }
        router.serverCapture = ServerCaptureTarget(id: note.id)
        guard flow != "record" else { return }
        let delay = max(2, UserDefaults.standard.double(forKey: "qa.leaveAfter"))
        try? await Task.sleep(for: .seconds(delay))
        switch flow {
        case "record-note": router.path = [.serverNote(note.id)]
        case "record-chat": router.path = CaptureNavigation.pathForChat(from: [], noteID: note.id)
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
}

extension UUID: @retroactive Identifiable {
    public var id: UUID { self }
}
