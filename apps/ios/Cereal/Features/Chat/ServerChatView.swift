import SwiftUI
import APIClient
import Contracts

/// Note-scoped reference chat chrome with actual incremental server output.
struct ServerChatView: View {
    let noteID: String?
    /// Set when the chat is a panel beside its note on a wide layout (`NoteChatPanel`).
    var panel: ChatPanelContext? = nil
    @Environment(ServerNotesModel.self) private var server
    @Environment(AccountsModel.self) private var accounts
    @Environment(\.motion) private var motion
    @Environment(\.verticalSizeClass) private var verticalSizeClass
    @Namespace private var glass
    @Environment(Router.self) private var router
    /// The conversation and any answer arriving in it. It lives in `ServerChatSessions`,
    /// not here, so leaving this screen does not end the answer.
    @State private var session: ServerChatSession?
    @State private var draft = ""
    @State private var model = "Auto"
    @State private var availableModels: [LanguageModelSelection] = []
    @State private var showHistory = false
    /// This account's saved threads for this note (or all notes), read from the server when History opens.
    @State private var saved = ServerChatThreadList()
    @State private var followsTail = true
    /// Height of the scroll view above the composer; see `followsChatTail`.
    @State private var room: CGFloat = 0
    @State private var latestPressed = false
    @State private var readerIsScrolling = false
    @State private var scrollIdentity = UUID()
    @State private var selectedCitation: ChatCitationSelection?
    @State private var choosingMeeting = false
    @FocusState private var focused: Bool

    private var title: String {
        // Beside its note the panel names the conversation, not the note, as the reference does.
        if panel != nil { return "New conversation" }
        guard let noteID else { return "Ask anything" }
        let value = server.title(ofNote: noteID) ?? ""
        return value.isEmpty ? "Untitled note" : value
    }
    private var sessionKey: String { ChatHistoryScope.key(accountIdentity: server.accountIdentity, noteID: noteID) ?? "disconnected" }
    private var messages: [ChatMessage] { session?.messages ?? ServerChatView.debugSeed }
    private var busy: Bool { session?.busy ?? false }
    private var error: String? { session?.error }
    private var queue: [ServerChatSession.Queued] { session?.queue ?? [] }
    private var selectedModel: String? { model == "Auto" ? nil : model }

    var body: some View {
        scaffold
        .onAppear {
            restore()
            // The reference opens a note's empty chat ready to type: the keyboard rises as the screen slides in.
            if noteID != nil, messages.isEmpty, !Self.qaNoFocus { focused = true }
        }
        // Where focus could not be taken during the transition, it is taken once the screen is in place.
        .task {
            guard noteID != nil else { return }
            try? await Task.sleep(for: .milliseconds(panel == nil ? 450 : 380))
            if messages.isEmpty, !Task.isCancelled, !focused, !Self.qaNoFocus { focused = true }
        }
        #if DEBUG
        // QA only: `-qa.ask "question"` asks it once in a note's chat, for displays simulated touches cannot reach.
        .task {
            guard noteID != nil, let question = UserDefaults.standard.string(forKey: "qa.ask"), !question.isEmpty else { return }
            try? await Task.sleep(for: .seconds(max(1, UserDefaults.standard.double(forKey: "qa.askAfter"))))
            guard !busy, messages.isEmpty else { return }
            draft = question; send()
            // `-qa.queue "question"` types a second one while the first is being answered.
            if let second = UserDefaults.standard.string(forKey: "qa.queue"), !second.isEmpty {
                try? await Task.sleep(for: .milliseconds(600))
                draft = second; send()
            }
        }
        // QA only: `-qa.draft "text"` types a question into the composer without sending it.
        .task {
            if let typed = UserDefaults.standard.string(forKey: "qa.draft"), !typed.isEmpty { draft = typed }
        }
        // QA only: `-qa.history YES` opens the saved chats once the screen is up.
        .task {
            guard UserDefaults.standard.bool(forKey: "qa.history") else { return }
            try? await Task.sleep(for: .seconds(1.5))
            showHistory = true
        }
        #endif
        .task(id: server.accountIdentity) {
            availableModels = []; model = "Auto"
            if let models = try? await server.chatModels(), !Task.isCancelled { availableModels = models }
        }
        .haptic(.tap, trigger: session?.sentCount ?? 0)
        // An answer that failed: the one outcome of a question that is felt. Text streaming in never is.
        .haptic(.error, trigger: error) { old, new in new != nil && new != old }
        // Another account, or none: this screen lets go of the conversation it showed. Ending the
        // answers of an account that signed out is `ServerChatSessions.accountChanged`, at the root.
        .onChange(of: sessionKey) {
            selectedCitation = nil; choosingMeeting = false; session = nil; draft = ""; restore()
        }
        .sheet(isPresented: $showHistory) { historySheet.task { await saved.refresh(server: server, noteID: noteID) } }
        .sheet(item: $selectedCitation) { ChatCitationTranscriptSheet(selection: $0) }
        .sheet(isPresented: $choosingMeeting) { ChatMeetingContextPicker(onSelect: selectMeetingContext) }
    }

    /// QA only: `-qa.noFocus YES` leaves the keyboard down, for screenshots of the whole screen.
    private static var qaNoFocus: Bool {
        #if DEBUG
        UserDefaults.standard.bool(forKey: "qa.noFocus")
        #else
        false
        #endif
    }

    /// A full screen of its own, or a panel beside its note.
    @ViewBuilder private var scaffold: some View {
        if let panel {
            ChatPanelScaffold(panel: panel, hasMessages: !messages.isEmpty, title: title,
                              onNew: newConversation, onHistory: { focused = false; showHistory = true }) { column }
        } else {
            RailScaffold { column } leading: {
                // Going back leaves the answer arriving; it is here on return.
                BackButton()
            } rail: {
                RailGroup {
                    // As in the reference: a new chat is offered once there is a conversation to leave.
                    if !messages.isEmpty { RailIcon(systemImage: "square.and.pencil") { newConversation() } }
                    RailIcon(systemImage: "clock.arrow.circlepath") { focused = false; showHistory = true }
                    // Across notes only: a meeting's own chat has back, new chat and history, as in the reference.
                    if noteID == nil { RailIcon(systemImage: "doc.text.magnifyingglass") { focused = false; choosingMeeting = true }.accessibilityLabel("Choose a meeting context") }
                }
            } bottom: { EmptyView() }
            .toolbarVisibility(.visible, for: .navigationBar).navigationBarBackButtonHidden()
        }
    }

    private var column: some View {
            VStack(alignment: .leading, spacing: 0) {
                // The splash says hello in its own words; the bar's title is for a conversation.
                Text(splash ? " " : title).font(.system(size: 15, weight: .semibold))
                    .foregroundStyle(Theme.chatText).lineLimit(1)
                    .padding(.top, RailMetrics.chatTitleInsets(compact: verticalSizeClass == .compact).top)
                    .padding(.bottom, RailMetrics.chatTitleInsets(compact: verticalSizeClass == .compact).bottom).padding(.leading, Theme.gutter)
                    // A long title ends where the messages do, 16 pt from the edge.
                    .padding(.trailing, ChatColumn.trailing)
                    .accessibilityHidden(splash)
                    // Beside a note without a rail the panel's controls share the title's row, which is then tall enough for them.
                    .frame(maxWidth: .infinity, minHeight: panel.map { $0.controlsInRail ? 0 : 52 }, alignment: .leading)
                    .overlay(alignment: .trailing) {
                        if let panel, !panel.controlsInRail {
                            ChatPanelHeaderControls(hasMessages: !messages.isEmpty, onNew: newConversation,
                                                    onHistory: { focused = false; showHistory = true }, onClose: panel.close)
                                .padding(.trailing, ChatColumn.trailing)
                        }
                    }
                if noteID == nil && !splash { Text("Across your private notes").font(.caption).foregroundStyle(Theme.textSecondary).padding(.leading, Theme.gutter).padding(.bottom, 8) }
                ScrollViewReader { proxy in
                    ScrollView {
                        // Not a lazy stack. A lazy one guesses the height of rows that are off
                        // screen from those that are on it, and an answer several screens tall
                        // next to a one-line question makes that guess wrong by hundreds of
                        // points, in both directions, whenever rows come and go. A conversation
                        // held at its end then rests at the end of the guess: past the last line
                        // when the keyboard comes up. A conversation is a bounded number of rows
                        // and each row is equatable, so laying all of them out costs little.
                        VStack(alignment: .leading, spacing: 22) {
                            if messages.isEmpty { emptyState.transition(motion.fade) }
                            ForEach(messages) { message in
                                ChatMessageView(message: message, onCitation: { openCitation($0, turnID: message.id) }, onMeeting: { openMeeting($0, turnID: message.id) },
                                                showsSources: noteID == nil)
                                    .equatable().id(message.id)
                                    .transition(message.role == .user ? motion.pop(anchor: .bottomTrailing) : motion.rise)
                            }
                            if let error {
                                VStack(alignment: .leading, spacing: 0) {
                                    Text(error).font(.footnote).foregroundStyle(Theme.textSecondary)
                                    Button("Retry question", systemImage: "arrow.clockwise") { retryLastQuestion() }
                                        .font(.footnote).disabled(busy)
                                        .frame(minHeight: 44).contentShape(Rectangle())
                                }.accessibilityIdentifier("server-chat-error")
                                    .transition(motion.rise)
                            }
                            // Questions typed while an answer was arriving wait here, in order.
                            ForEach(queue) { waiting in
                                QueuedQuestionRow(text: waiting.text) { session?.removeQueued(waiting.id) }
                                    .transition(motion.pop(anchor: .bottomTrailing))
                            }
                            Color.clear.frame(height: ChatTail.endSpace).id("bottom")
                        }
                        // Same column as the local chat: 16 leading and 8 trailing inside the scaffold's padding.
                        .padding(.leading, ChatColumn.leading).padding(.trailing, ChatColumn.trailing).padding(.top, 8)
                        .fillsChatRoom(room)
                        // Existing text settles without a bounce; the new question pops through its own transition.
                        .animation(motion.settle, value: messages.count)
                        .animation(motion.snappy, value: error)
                        .animation(motion.snappy, value: queue)
                    }
                    .id(scrollIdentity)
                    .scrollIndicators(.hidden).scrollDismissesKeyboard(.interactively)
                    .onScrollPhaseChange { _, phase in
                        readerIsScrolling = phase == .tracking || phase == .interacting || phase == .decelerating
                    }
                    .onScrollGeometryChange(for: Bool.self) { geometry in
                        ChatTail.isNearEnd(visibleMaxY: geometry.visibleRect.maxY, bottomInset: geometry.contentInsets.bottom, contentHeight: geometry.contentSize.height)
                    } action: { _, nearBottom in
                        followsTail = ChatTail.follows(current: followsTail, readerIsScrolling: readerIsScrolling, nearEnd: nearBottom)
                    }
                    // A reader at the end stays at the end, whatever made the conversation
                    // taller or its room smaller, and finishes with the whole answer in view.
                    .followsChatTail(followsTail, room: $room)
                    .chatTopFade()
                    .onChange(of: messages.count) {
                        if followsTail && !messages.isEmpty { withAnimation(motion.snappy) { proxy.scrollTo("bottom", anchor: .chatEnd) } }
                    }
                    .onChange(of: queue.count) {
                        if followsTail && !messages.isEmpty { withAnimation(motion.snappy) { proxy.scrollTo("bottom", anchor: .chatEnd) } }
                    }
                    // 44 pt target for the jump-to-latest control drawn in the bar below.
                    .overlay(alignment: .bottom) {
                        if showsLatest {
                            Button {
                                followsTail = true
                                withAnimation(motion.snappy) { proxy.scrollTo("bottom", anchor: .chatEnd) }
                            } label: { Color.clear.frame(width: 44, height: 44).contentShape(Rectangle()) }
                                .buttonStyle(PressReportingStyle(pressed: $latestPressed))
                                .accessibilityLabel("Scroll to latest message")
                                .padding(.bottom, ChatColumn.latestGap - 4)
                        }
                    }
                    // The composer floats over the conversation, so messages pass
                    // under its glass. It shares one glass group with the
                    // jump-to-latest control, which buds off its top edge.
                    // The bar draws its own fade into the page (`chatBarBackdrop`). The system's
                    // edge effect under it showed as a paler band with a hard top edge.
                    .scrollEdgeEffectHidden(true, for: .bottom)
                    .safeAreaBar(edge: .bottom, spacing: 0) {
                        GlassChromeGroup(spacing: 16) {
                            ChatComposer(draft: $draft, model: $model, isGenerating: busy, focused: $focused,
                                onSend: send, onStop: stop, queuesWhileGenerating: true, availableModels: availableModels,
                                identifierPrefix: "server-chat",
                                recipes: noteID != nil && messages.isEmpty && queue.isEmpty ? ChatRecipes.note : [],
                                onRecipe: { draft = $0; send() })
                                .glassMorph("composer", in: glass)
                                // The control is drawn here so its glass buds off the composer, on a
                                // zero-height line along the composer's top edge. The bar does not take
                                // touches above its own frame, so the button itself is the transparent
                                // target laid over the conversation below.
                                .overlay(alignment: .top) {
                                    Color.clear.frame(height: 0).overlay(alignment: .bottom) {
                                        if showsLatest {
                                            Image(systemName: "arrow.down").font(.system(size: 14, weight: .medium))
                                                .foregroundStyle(Theme.textPrimary).frame(width: 36, height: 36)
                                                .glassSurface(Circle(), interactive: true)
                                                .glassMorph("latest", in: glass)
                                                .scaleEffect(latestPressed && !motion.reduceMotion ? 0.9 : 1)
                                                .animation(motion.snappy, value: latestPressed)
                                                .padding(.bottom, ChatColumn.latestGap)
                                                .allowsHitTesting(false).accessibilityHidden(true)
                                        }
                                    }
                                }
                        }
                        .animation(motion.animation(.gooey, entering: showsLatest), value: showsLatest)
                        .chatBarBackdrop(raised: showsLatest)
                        .animation(motion.snappy, value: showsLatest)
                    }
                }
            }
    }

    private var showsLatest: Bool { ChatTail.showsJumpToLatest(hasMessages: !messages.isEmpty, followsTail: followsTail) }

    /// The chat across meetings with nothing asked yet: a new conversation's splash.
    private var splash: Bool { noteID == nil && messages.isEmpty }

    /// "Hi Sam, ask anything", with the account's first name, as on the desktop.
    private var greeting: String {
        let name = accounts.sessions.first { $0.notes === server }?.record.name ?? ""
        let first = name.split(whereSeparator: \.isWhitespace).first.map { String($0.prefix(40)) } ?? ""
        return first.isEmpty ? "Ask anything" : "Hi \(first), ask anything"
    }

    @ViewBuilder private var emptyState: some View {
        if noteID == nil {
            // As on the desktop: the greeting, then the newest saved chats. The composer is the bar below.
            VStack(alignment: .leading, spacing: 0) {
                Text(greeting).font(.display(26)).foregroundStyle(Theme.textPrimary)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityAddTraits(.isHeader)
                    .padding(.top, 24)
                if !recents.isEmpty {
                    HStack {
                        Text("Recents").accessibilityAddTraits(.isHeader)
                        Spacer()
                        Button { focused = false; showHistory = true } label: {
                            Text("See all").frame(minHeight: 44).contentShape(Rectangle())
                        }.buttonStyle(.plain).accessibilityIdentifier("server-chat-see-all")
                    }
                    .font(.system(size: 14)).foregroundStyle(Theme.textSecondary)
                    .padding(.top, 40)
                    VStack(alignment: .leading, spacing: 0) {
                        ForEach(recents, id: \.id) { thread in
                            Button { openSaved(thread) } label: {
                                HStack(spacing: 12) {
                                    Image(systemName: thread.noteId == nil ? "bubble.left" : "doc.text")
                                        .font(.system(size: 14)).foregroundStyle(Theme.textSecondary)
                                        .frame(width: 32, height: 32)
                                        .background(Theme.surface, in: RoundedRectangle(cornerRadius: 8, style: .continuous))
                                        .overlay(RoundedRectangle(cornerRadius: 8, style: .continuous).strokeBorder(Theme.stroke, lineWidth: 1))
                                    Text(ChatThreadTurns.title(thread)).font(.system(size: 15)).foregroundStyle(Theme.textPrimary).lineLimit(1)
                                    Spacer(minLength: 8)
                                    if let at = CalendarSchedule.timestamp(thread.updatedAt) {
                                        Text(ChatThreadTurns.age(of: at)).font(.system(size: 13)).monospacedDigit()
                                            .foregroundStyle(Theme.textSecondary)
                                            .accessibilityLabel(ChatThreadTurns.spokenAge(of: at))
                                    }
                                }
                                .frame(minHeight: 48).contentShape(Rectangle())
                            }
                            .buttonStyle(RowPressStyle())
                            .accessibilityIdentifier("server-chat-recent")
                        }
                    }
                    .transition(motion.fade)
                }
            }
            .animation(motion.snappy, value: recents.map(\.id))
            .task { await saved.refresh(server: server, noteID: nil) }
        }
        // A meeting's empty chat is empty: its recipes sit in one row above the composer.
    }

    /// The three newest saved chats, for the splash.
    private var recents: [ChatThreadSummaryDTO] { Array(saved.listed.prefix(3)) }

    /// Conversations kept on this iPhone only. Those saved on the server are listed from there instead.
    private var visibleHistory: [ChatConversation] { (session?.conversationsForHistory ?? []).filter { $0.threadID == nil } }

    private func openSaved(_ summary: ChatThreadSummaryDTO) {
        focused = false
        Task { @MainActor in
            do {
                let thread = try await server.chatThread(id: summary.id)
                session?.open(thread)
                resetScrollPosition()
                draft = ""; showHistory = false
            } catch { saved.error = error.localizedDescription }
        }
    }

    private var historySheet: some View {
        ChatHistorySheet(saved: saved, local: visibleHistory, error: ServerChatSessions.shared.persistenceError,
                         onOpen: openSaved,
                         onOpenLocal: { conversation in
                             focused = false
                             session?.select(conversation)
                             resetScrollPosition()
                             draft = ""; showHistory = false
                         },
                         onDelete: { id in Task { await saved.delete(id, server: server) } },
                         onMore: { Task { await saved.loadMore(server: server, noteID: noteID) } },
                         onClose: { showHistory = false })
    }

    /// Sends the question, or queues it behind the answer that is arriving.
    private func send() {
        guard let session else { return }
        guard session.ask(draft, model: selectedModel) else { return }
        draft = ""; followsTail = true
        // A question that goes out now puts the keyboard away; one that waits leaves it for the next.
        if session.queue.isEmpty { focused = false }
    }

    /// Stop ends the answer and brings the questions that were waiting back to the composer.
    private func stop() {
        guard let session else { return }
        returnToComposer(session.stop())
    }

    private func returnToComposer(_ waiting: [String]) {
        guard !waiting.isEmpty, draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return }
        draft = waiting.joined(separator: "\n")
    }

    private func selectMeetingContext(_ note: ServerNoteRow) {
        choosingMeeting = false
        let account = server.accountIdentity
        Task { @MainActor in
            try? await Task.sleep(for: .milliseconds(300))
            guard !Task.isCancelled, let account, server.accountIdentity == account,
                  server.title(ofNote: note.id) != nil else { return }
            if let id = server.accountID { router.path.append(.serverChat(id, noteID: note.id)) }
        }
    }
    private func openMeeting(_ meeting: ChatToolMeetingDTO, turnID: UUID) {
        guard messages.first(where: { $0.id == turnID })?.annotations?.tools.contains(where: { $0.results?.contains(where: { $0.noteId == meeting.noteId }) == true }) == true,
              session?.key == sessionKey, let owner = server.accountID, let id = UUID(uuidString: meeting.noteId) else { return }
        focused = false
        // A meeting an answer names opens in the thread's own account.
        router.path.append(.serverNote(NoteRef(account: owner, id: id.uuidString.lowercased())))
    }
    private func openCitation(_ citation: ChatCitationDTO, turnID: UUID) {
        guard messages.first(where: { $0.id == turnID })?.annotations?.citations.contains(where: { $0.id == citation.id && $0.noteId == citation.noteId && $0.source == citation.source && $0.segmentId == citation.segmentId && $0.transcriptRevision == citation.transcriptRevision }) == true,
              session?.key == sessionKey, let account = server.accountIdentity, let owner = server.accountID,
              let id = UUID(uuidString: citation.noteId) else { return }
        focused = false
        if citation.source == .transcript { selectedCitation = ChatCitationSelection(citation: citation, accountIdentity: account) }
        else { router.path.append(.serverNoteSource(NoteRef(account: owner, id: id.uuidString.lowercased()), raw: citation.source == .notes)) }
    }

    private func retryLastQuestion() { session?.retryLastQuestion(model: selectedModel) }
    private func newConversation() {
        guard let session else { return }
        let waiting = session.newConversation()
        draft = ""; returnToComposer(waiting); resetScrollPosition(); focused = true
    }
    private func resetScrollPosition() {
        followsTail = true; readerIsScrolling = false
        // A new thread must not inherit the previous long response's content offset.
        scrollIdentity = UUID()
    }
    /// Takes up the conversation for this account and note, with whatever arrived while this screen was away.
    private func restore() {
        guard session == nil, let account = server.accountIdentity, sessionKey != "disconnected" else { return }
        session = ServerChatSessions.shared.session(for: sessionKey, accountIdentity: account, noteID: noteID, server: server)
        resetScrollPosition()
    }
}

/// Where Chat in the app's navigation leads.
enum ChatNavigation {
    /// The chat across meetings of the default account, on a new conversation's splash
    /// (the one that was open is kept in the history). Nil without an account to ask.
    @MainActor static func workspaceChat(_ accounts: AccountsModel) -> Route? {
        guard let session = accounts.defaultSession, let account = session.notes.accountID else { return nil }
        if let key = ChatHistoryScope.key(accountIdentity: session.notes.accountIdentity, noteID: nil) {
            ServerChatSessions.shared.freshConversation(for: key)
        }
        return .serverChat(account, noteID: nil)
    }
}

/// A question that waits for the answer before it. Dimmed, in the place it
/// will take, with a way to take it back out.
struct QueuedQuestionRow: View {
    let text: String
    let remove: () -> Void

    var body: some View {
        VStack(alignment: .trailing, spacing: 6) {
            HStack {
                Spacer(minLength: 60)
                Text(text).readingFont(.body).foregroundStyle(Theme.chatText)
                    .padding(.horizontal, 16).padding(.vertical, 14)
                    .background(Theme.chatQuestion, in: RoundedRectangle(cornerRadius: 18, style: .continuous))
                    .opacity(0.5)
            }
            HStack(spacing: 2) {
                Text("Queued · sends when this answer finishes").font(.caption).foregroundStyle(Theme.textSecondary)
                Button(action: remove) {
                    Image(systemName: "xmark.circle.fill").font(.system(size: 15)).foregroundStyle(Theme.textSecondary)
                        .frame(width: 44, height: 44).contentShape(Rectangle())
                }
                .buttonStyle(.squash)
                .accessibilityLabel("Remove queued question")
            }
            // The 44 pt target must not push the caption away from its question.
            .padding(.vertical, -14).padding(.trailing, -12)
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("server-chat-queued")
    }
}

/// Private local history is isolated by instance URL and note UUID. A bounded,
/// protected file stores conversation text, never owner/provider credentials.
@MainActor final class ServerChatSessions {
    /// Saved chats are the server's (`ServerChatThreads.swift`): conversations are held in memory only, and
    /// no history file is read or written.
    static let shared = ServerChatSessions(persistenceURL: nil)
    struct ConversationState: Codable { var messages: [ChatMessage] = []; var history: [ChatConversation] = []; var threadID: String? = nil }
    private struct HistoryFile: Codable { var version = 1; var values: [String: ConversationState]; var order: [String] }
    private var values: [String: ConversationState] = [:]
    private var order: [String] = []
    private let persistenceURL: URL?
    private var preserveUnreadable = false
    private(set) var persistenceError: String?
    /// Conversations in use, each with any answer still arriving in it, by account and note.
    private var live: [String: ServerChatSession] = [:]
    private var liveOrder: [String] = []

    /// A nil URL is deliberately memory-only for deterministic tests/previews.
    init(persistenceURL: URL? = nil) {
        self.persistenceURL = persistenceURL
        guard let url = persistenceURL, FileManager.default.fileExists(atPath: url.path) else { return }
        do {
            let handle = try FileHandle(forReadingFrom: url)
            defer { try? handle.close() }
            let data = try handle.read(upToCount: 2_000_001) ?? Data()
            guard data.count <= 2_000_000 else { throw CocoaError(.fileReadCorruptFile) }
            let saved = try JSONDecoder().decode(HistoryFile.self, from: data)
            guard saved.version == 1, saved.values.count <= 64, saved.order.count <= 64 else { throw CocoaError(.fileReadCorruptFile) }
            values = saved.values.mapValues(Self.finished)
            order = saved.order.filter { values[$0] != nil }
        } catch {
            preserveUnreadable = true
            persistenceError = "Saved chat history could not be read. The original file has been preserved; new chats remain available for this session."
        }
    }
    func load(for key: String) -> ConversationState { values[key] ?? ConversationState() }

    /// The conversation for this account and note. The same object for as long as it is in
    /// use, so a screen that comes back finds the answer that kept arriving while it was away.
    func session(for key: String, accountIdentity: String, noteID: String?, server: ServerNotesModel) -> ServerChatSession {
        session(for: key, accountIdentity: accountIdentity, noteID: noteID,
                events: { [weak server] noteID, message, history, model in
                    guard let server else { throw APIClientError.unauthorized }
                    return try server.chatEvents(noteID: noteID, message: message, history: history, model: model)
                },
                threadEvents: { [weak server] create, turn, replace, message, model in
                    guard let server else { throw APIClientError.unauthorized }
                    return try server.chatThreadEvents(create: create, turn: turn, replace: replace, message: message, model: model)
                },
                currentAccount: { [weak server] in server?.accountIdentity }, background: .application, activity: .system)
    }

    func session(for key: String, accountIdentity: String, noteID: String?, events: @escaping ServerChatSession.Events,
                 threadEvents: ServerChatSession.ThreadEvents? = nil,
                 currentAccount: @escaping @MainActor () -> String?, background: ChatBackgroundTime = .none,
                 activity: ChatActivityHooks = .none) -> ServerChatSession {
        liveOrder.removeAll { $0 == key }; liveOrder.append(key)
        if let existing = live[key] { return existing }
        let session = ServerChatSession(key: key, accountIdentity: accountIdentity, noteID: noteID, state: load(for: key),
                                        events: events, threadEvents: threadEvents, currentAccount: currentAccount, background: background,
                                        activity: activity) { [weak self] session in
            self?.save(messages: session.messages, history: session.history, threadID: session.threadID, for: session.key)
        }
        live[key] = session
        // Idle conversations beyond the newest few are let go; they are on disk. One with an answer arriving never is.
        for old in liveOrder.dropLast(8) where live[old]?.busy != true && live[old]?.queue.isEmpty != false {
            live[old] = nil; liveOrder.removeAll { $0 == old }
        }
        return session
    }

    /// The signed-in account changed, or there is none: every answer arriving for
    /// any other account ends, and nothing more is written to its conversation.
    func accountChanged(to identity: String?) {
        accountsChanged(to: identity.map { [$0] } ?? [])
    }

    /// The accounts on this device are now `identities`: every answer arriving
    /// for an account that is not among them ends, and nothing more is written
    /// to its conversation. The others carry on.
    func accountsChanged(to identities: Set<String>) {
        for (key, session) in live where !identities.contains(session.accountIdentity) {
            session.end(); live[key] = nil; liveOrder.removeAll { $0 == key }
        }
    }

    /// The account was removed and its data erased: its saved conversations go too.
    func erase(accountIdentity: String) {
        accountsChanged(to: Set(live.values.map(\.accountIdentity)).subtracting([accountIdentity]))
        let prefix = accountIdentity + "|"
        let gone = values.keys.filter { $0.hasPrefix(prefix) }
        guard !gone.isEmpty else { return }
        for key in gone { values[key] = nil }
        order.removeAll { $0.hasPrefix(prefix) }
        persist()
    }

    /// The note was deleted: an answer arriving about it ends.
    func noteDeleted(_ noteID: String, accountIdentity: String?) {
        guard let key = ChatHistoryScope.key(accountIdentity: accountIdentity, noteID: noteID), let session = live[key] else { return }
        session.end(); live[key] = nil; liveOrder.removeAll { $0 == key }
    }

    /// Opening Chat from Home or the sidebar starts on a new conversation's splash, as
    /// on the desktop: the one that was open is kept in the history (and, once asked
    /// in, on the server). One whose answer is still arriving is shown as it is,
    /// since this device keeps one conversation going per chat.
    func freshConversation(for key: String) {
        if let session = live[key] {
            if !session.busy, session.queue.isEmpty, !session.messages.isEmpty { session.newConversation() }
            return
        }
        let state = load(for: key)
        guard !state.messages.isEmpty else { return }
        save(messages: [], history: Self.archive(state.messages, in: state.history, threadID: state.threadID), threadID: nil, for: key)
    }

    /// Answers arriving now. For tests.
    var answering: [String] { live.filter { $0.value.busy }.map(\.key).sorted() }

    /// Switching history is a transition, not destructive replacement: preserve
    /// the outgoing thread before restoring the chosen one.
    static func select(_ conversation: ChatConversation, current: [ChatMessage], history: [ChatConversation], currentThreadID: String? = nil) -> ConversationState {
        finished(ConversationState(messages: conversation.messages, history: archive(current, in: history, threadID: currentThreadID), threadID: conversation.threadID))
    }

    static func archive(_ messages: [ChatMessage], in history: [ChatConversation], threadID: String? = nil) -> [ChatConversation] {
        guard messages.contains(where: { !$0.text.isEmpty }) else { return Array(history.prefix(20)) }
        let firstQuestion = messages.first(where: { $0.role == .user })?.id ?? messages.first?.id
        let existing = history.first { ($0.messages.first(where: { $0.role == .user })?.id ?? $0.messages.first?.id) == firstQuestion }
        let current = ChatConversation(id: existing?.id ?? UUID(), createdAt: existing?.createdAt ?? .now,
            title: messages.first(where: { $0.role == .user })?.text ?? "Conversation", messages: messages, threadID: threadID ?? existing?.threadID)
        return Array(([current] + history.filter { $0.id != current.id && ($0.messages.first(where: { $0.role == .user })?.id ?? $0.messages.first?.id) != firstQuestion }).prefix(20))
    }

    func save(messages: [ChatMessage], history: [ChatConversation], threadID: String? = nil, for key: String) {
        values[key] = Self.finished(ConversationState(messages: Array(messages.suffix(200)), history: Self.archive(messages, in: history, threadID: threadID), threadID: threadID))
        order.removeAll { $0 == key }; order.append(key)
        while order.count > 64 { values.removeValue(forKey: order.removeFirst()) }
        persist()
    }

    private func persist() {
        guard let url = persistenceURL, !preserveUnreadable else { return }
        do {
            let data = try JSONEncoder().encode(HistoryFile(values: values, order: order))
            guard data.count <= 2_000_000 else {
                persistenceError = "Chat history is too large to save. Your current conversation remains available for this session."
                return
            }
            try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
            try data.write(to: url, options: [.atomic, .completeFileProtectionUntilFirstUserAuthentication])
            try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: url.path)
            var destination = url
            var resource = URLResourceValues(); resource.isExcludedFromBackup = true
            try destination.setResourceValues(resource)
            persistenceError = nil
        } catch {
            persistenceError = "Chat history could not be saved. Your current conversation remains available for this session."
        }
    }
    private static func finished(_ state: ConversationState) -> ConversationState {
        func sanitize(_ messages: [ChatMessage]) -> [ChatMessage] {
            messages.suffix(200).filter { !$0.text.isEmpty || $0.annotations?.hasContent == true }.map { message in
                var saved = message
                if saved.isStreaming { saved.isInterrupted = true; saved.isStreaming = false }
                return saved
            }
        }
        return ConversationState(messages: sanitize(state.messages), history: state.history.prefix(20).map {
            ChatConversation(id: $0.id, createdAt: $0.createdAt, title: $0.title, messages: sanitize($0.messages), threadID: $0.threadID)
        }, threadID: state.threadID)
    }
}

extension ServerChatView {
    /// `-screen server-chat-sample` shows a fixed, fictional conversation for
    /// layout checks without an account or a network request. Debug builds only.
    static var debugSeed: [ChatMessage] {
        #if DEBUG
        guard ProcessInfo.processInfo.arguments.contains("server-chat-sample") else { return [] }
        // A fictional company and team. The first answer searched (sources, lookups, reasoning:
        // "Worked for"), the second only read the meeting's transcript ("Thought for").
        func meeting(_ id: Int, _ title: String, _ date: String) -> ChatToolMeetingDTO {
            ChatToolMeetingDTO(date: date, noteId: String(format: "00000000-0000-4000-8000-%012d", id), title: title)
        }
        func source(_ id: String, _ note: Int, _ title: String) -> ChatCitationDTO {
            ChatCitationDTO(id: id, noteId: String(format: "00000000-0000-4000-8000-%012d", note), quote: "", segmentId: nil, source: .notes, startMs: nil, title: title, transcriptRevision: 1)
        }
        let today = Date.now.formatted(.iso8601.year().month().day())
        var worked = ChatTurnAnnotations(saved: [
            ChatReasoningDTO(afterTools: 0, text: "**Finding the pilot calls**\n\nThe dock pilot came up in more than one meeting, so I will search for it first."),
            ChatReasoningDTO(afterTools: 1, text: "**Reading both calls**\n\nThe later call has the pricing decision; the earlier one has the dock counts."),
        ], elapsedMs: 6_400)
        worked.apply(ChatStreamEventDTO(text: "", tool: ChatToolProgressDTO(callId: "qa-1", label: "Searched meetings for the dock pilot", name: .searchMeetings, resultCount: 2,
            results: [meeting(1, "Halden pilot check-in", today), meeting(2, "Halden pricing call", "2026-11-12")], status: .completed), type: .tool))
        worked.apply(ChatStreamEventDTO(text: "", tool: ChatToolProgressDTO(callId: "qa-2", label: "Read 2 meetings", name: .getMeetings, resultCount: 2, results: nil, status: .completed), type: .tool))
        worked.apply(ChatStreamEventDTO(citations: [source("S1", 1, "Halden pilot check-in"), source("S2", 2, "Halden pricing call"), source("S12", 2, "Halden pricing call")], text: "", type: .citations))
        var thought = ChatTurnAnnotations(saved: [], elapsedMs: 2_900)
        thought.apply(ChatStreamEventDTO(text: "", tool: ChatToolProgressDTO(callId: "qa-3", label: "Read the transcript", name: .getMeetingTranscript, resultCount: nil, results: nil, status: .completed), type: .tool))
        var messages = [
            ChatMessage(role: .user, text: "What did we decide about the dock pilot?"),
            ChatMessage(role: .assistant, text: "The pilot is staying at **€42/dock/month**, with a longer trial instead of a discount [[S2]]. Usage is up: 3 of 5 docks now book in the app [[S1]][[S12]].\n\n# Next Steps\n- Send the yard system integration spec (Owen)\n  Due Friday, so engineering can scope the sync before the next call.\n- Book onsite training for the night shifts (Priya)\n  Both night leads asked for it; aim for the week of the 14th.", annotations: worked),
            ChatMessage(role: .user, text: "Who is running the training?"),
            ChatMessage(role: .assistant, text: "Priya is booking it, with the two night leads joining the first session.", annotations: thought),
        ]
        // `-qa.chatSeed thinking`: a third question still being thought about, a lookup under way.
        if UserDefaults.standard.string(forKey: "qa.chatSeed") == "thinking" {
            var live = ChatTurnAnnotations(saved: [ChatReasoningDTO(afterTools: 0, text: "**Checking the calendar**\n\nLooking for the training dates.\n")], elapsedMs: nil)
            live.apply(ChatStreamEventDTO(text: "", tool: ChatToolProgressDTO(callId: "qa-4", label: "Listing meetings this month", name: .listMeetings, resultCount: nil, results: nil, status: .running), type: .tool))
            messages += [ChatMessage(role: .user, text: "When is the first session?"), ChatMessage(role: .assistant, text: "", isStreaming: true, annotations: live)]
        }
        return messages
        #else
        return []
        #endif
    }
}

#if DEBUG
/// QA only, with `-perf-probe`: logs how long a question took to its first
/// shown text and to its finished answer, measured from the tap on send.
/// Read with `log show --predicate 'eventMessage CONTAINS "QACHAT"'`.
struct ChatTimingLog {
    private let start = ContinuousClock.now
    private let label: String

    init(question: String) {
        label = String(question.prefix(40))
        guard PerfProbe.enabled else { return }
        NSLog("QACHAT sent \"%@\"", label)
    }

    func mark(_ event: String) {
        guard PerfProbe.enabled else { return }
        let elapsed = ContinuousClock.now - start
        let ms = Double(elapsed.components.seconds) * 1000 + Double(elapsed.components.attoseconds) / 1e15
        NSLog("QACHAT %@ %.0f ms \"%@\"", event, ms, label)
    }
}
#endif
