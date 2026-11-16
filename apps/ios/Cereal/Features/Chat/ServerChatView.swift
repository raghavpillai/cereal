import SwiftUI
import APIClient
import Contracts

/// Note-scoped reference chat chrome with actual incremental server output.
struct ServerChatView: View {
    let noteID: String?
    @Environment(ServerNotesModel.self) private var server
    @Environment(\.dismiss) private var dismiss
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
    private var suggestions: [String] {
        noteID == nil ? ["What are my recent todos?", "What did I commit to this week?", "Summarize my last meeting"] : ["Summarize this meeting", "What are the action items?", "List the open questions"]
    }

    var body: some View {
        RailScaffold {
            VStack(alignment: .leading, spacing: 0) {
                Text(title).font(.system(size: 15, weight: .semibold))
                    .foregroundStyle(Theme.chatText).lineLimit(1)
                    .padding(.top, RailMetrics.chatTitleInsets(compact: verticalSizeClass == .compact).top)
                    .padding(.bottom, RailMetrics.chatTitleInsets(compact: verticalSizeClass == .compact).bottom).padding(.leading, Theme.gutter)
                    // A long title ends where the messages do, 16 pt from the edge.
                    .padding(.trailing, ChatColumn.trailing)
                if noteID == nil { Text("Across your private notes").font(.caption).foregroundStyle(Theme.textSecondary).padding(.leading, Theme.gutter).padding(.bottom, 8) }
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
                                ChatMessageView(message: message, onCitation: { openCitation($0, turnID: message.id) }, onMeeting: { openMeeting($0, turnID: message.id) })
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
                    .safeAreaBar(edge: .bottom, spacing: 0) {
                        GlassChromeGroup(spacing: 16) {
                            ChatComposer(draft: $draft, model: $model, isGenerating: busy, focused: $focused,
                                onSend: send, onStop: stop, serverMode: true, queuesWhileGenerating: true, identifierPrefix: "server-chat")
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
        } leading: {
            // Going back leaves the answer arriving; it is here on return.
            GlassCircleButton(systemImage: "chevron.left") { dismiss() }
        } rail: {
            RailGroup {
                RailIcon(systemImage: "square.and.pencil") { newConversation() }
                RailIcon(systemImage: "clock.arrow.circlepath") { focused = false; showHistory = true }
                RailIcon(systemImage: "doc.text.magnifyingglass") { focused = false; choosingMeeting = true }.accessibilityLabel("Choose a meeting context")
            }
        } bottom: { EmptyView() }
        .toolbarVisibility(.visible, for: .navigationBar).navigationBarBackButtonHidden()
        .onAppear { restore() }
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
        #endif
        .task(id: server.accountIdentity) {
            availableModels = []; model = "Auto"
            if let models = try? await server.chatModels(), !Task.isCancelled { availableModels = models }
        }
        .sensoryFeedback(.impact(weight: .light), trigger: session?.sentCount ?? 0)
        // Another account, or none: this screen lets go of the conversation it showed. Ending the
        // answers of an account that signed out is `ServerChatSessions.accountChanged`, at the root.
        .onChange(of: sessionKey) {
            selectedCitation = nil; choosingMeeting = false; session = nil; draft = ""; restore()
        }
        .sheet(isPresented: $showHistory) { historySheet }
        .sheet(item: $selectedCitation) { ChatCitationTranscriptSheet(selection: $0) }
        .sheet(isPresented: $choosingMeeting) { ChatMeetingContextPicker(onSelect: selectMeetingContext) }
    }

    private var showsLatest: Bool { ChatTail.showsJumpToLatest(hasMessages: !messages.isEmpty, followsTail: followsTail) }

    private var emptyState: some View {
        VStack(alignment: .leading, spacing: 12) {
            ChatSuggestionChips(suggestions: suggestions) { draft = $0; send() }
        }.padding(.top, 8)
    }

    private var visibleHistory: [ChatConversation] { session?.conversationsForHistory ?? [] }

    private var historySheet: some View {
        NavigationStack {
            List {
                if let message = ServerChatSessions.shared.persistenceError {
                    Text(message).font(.footnote).foregroundStyle(Theme.textSecondary)
                        .listRowBackground(Color.clear)
                }
                if visibleHistory.isEmpty {
                    ContentUnavailableView("No previous chats", systemImage: "bubble.left.and.bubble.right")
                        .listRowBackground(Color.clear)
                }
                ForEach(visibleHistory) { conversation in
                    Button {
                        focused = false
                        session?.select(conversation)
                        resetScrollPosition()
                        draft = ""; showHistory = false
                    } label: {
                        VStack(alignment: .leading, spacing: 5) {
                            Text(conversation.title).lineLimit(1).foregroundStyle(Theme.textPrimary)
                            Text(conversation.createdAt, style: .time).font(.caption).foregroundStyle(Theme.textSecondary)
                        }.padding(.vertical, 5)
                    }.listRowBackground(Theme.surface)
                }
            }
            .scrollContentBackground(.hidden).background(Color.clear)
            .navigationTitle("Chat history").navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .confirmationAction) { Button("Done") { showHistory = false } } }
        }.glassSheet()
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
            router.path.append(.serverChat(note.id))
        }
    }
    private func openMeeting(_ meeting: ChatToolMeetingDTO, turnID: UUID) {
        guard messages.first(where: { $0.id == turnID })?.annotations?.tools.contains(where: { $0.results?.contains(where: { $0.noteId == meeting.noteId }) == true }) == true,
              session?.key == sessionKey, server.accountIdentity != nil, let id = UUID(uuidString: meeting.noteId) else { return }
        focused = false
        router.path.append(.serverNote(id.uuidString.lowercased()))
    }
    private func openCitation(_ citation: ChatCitationDTO, turnID: UUID) {
        guard messages.first(where: { $0.id == turnID })?.annotations?.citations.contains(where: { $0.id == citation.id && $0.noteId == citation.noteId && $0.source == citation.source && $0.segmentId == citation.segmentId && $0.transcriptRevision == citation.transcriptRevision }) == true,
              session?.key == sessionKey, let account = server.accountIdentity,
              let id = UUID(uuidString: citation.noteId) else { return }
        focused = false
        if citation.source == .transcript { selectedCitation = ChatCitationSelection(citation: citation, accountIdentity: account) }
        else { router.path.append(.serverNoteSource(id.uuidString.lowercased(), raw: citation.source == .notes)) }
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
    static let shared = ServerChatSessions(persistenceURL: FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first?.appendingPathComponent("Cereal/server-chat-history.json"))
    struct ConversationState: Codable { var messages: [ChatMessage] = []; var history: [ChatConversation] = [] }
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
                currentAccount: { [weak server] in server?.accountIdentity }, background: .application)
    }

    func session(for key: String, accountIdentity: String, noteID: String?, events: @escaping ServerChatSession.Events,
                 currentAccount: @escaping @MainActor () -> String?, background: ChatBackgroundTime = .none) -> ServerChatSession {
        liveOrder.removeAll { $0 == key }; liveOrder.append(key)
        if let existing = live[key] { return existing }
        let session = ServerChatSession(key: key, accountIdentity: accountIdentity, noteID: noteID, state: load(for: key),
                                        events: events, currentAccount: currentAccount, background: background) { [weak self] session in
            self?.save(messages: session.messages, history: session.history, for: session.key)
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
        for (key, session) in live where session.accountIdentity != identity {
            session.end(); live[key] = nil; liveOrder.removeAll { $0 == key }
        }
    }

    /// The note was deleted: an answer arriving about it ends.
    func noteDeleted(_ noteID: String, accountIdentity: String?) {
        guard let key = ChatHistoryScope.key(accountIdentity: accountIdentity, noteID: noteID), let session = live[key] else { return }
        session.end(); live[key] = nil; liveOrder.removeAll { $0 == key }
    }

    /// Answers arriving now. For tests.
    var answering: [String] { live.filter { $0.value.busy }.map(\.key).sorted() }

    /// Switching history is a transition, not destructive replacement: preserve
    /// the outgoing thread before restoring the chosen one.
    static func select(_ conversation: ChatConversation, current: [ChatMessage], history: [ChatConversation]) -> ConversationState {
        finished(ConversationState(messages: conversation.messages, history: archive(current, in: history)))
    }

    static func archive(_ messages: [ChatMessage], in history: [ChatConversation]) -> [ChatConversation] {
        guard messages.contains(where: { !$0.text.isEmpty }) else { return Array(history.prefix(20)) }
        let firstQuestion = messages.first(where: { $0.role == .user })?.id ?? messages.first?.id
        let existing = history.first { ($0.messages.first(where: { $0.role == .user })?.id ?? $0.messages.first?.id) == firstQuestion }
        let current = ChatConversation(id: existing?.id ?? UUID(), createdAt: existing?.createdAt ?? .now,
            title: messages.first(where: { $0.role == .user })?.text ?? "Conversation", messages: messages)
        return Array(([current] + history.filter { $0.id != current.id && ($0.messages.first(where: { $0.role == .user })?.id ?? $0.messages.first?.id) != firstQuestion }).prefix(20))
    }

    func save(messages: [ChatMessage], history: [ChatConversation], for key: String) {
        values[key] = Self.finished(ConversationState(messages: Array(messages.suffix(200)), history: Self.archive(messages, in: history)))
        order.removeAll { $0 == key }; order.append(key)
        while order.count > 64 { values.removeValue(forKey: order.removeFirst()) }
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
            ChatConversation(id: $0.id, createdAt: $0.createdAt, title: $0.title, messages: sanitize($0.messages))
        })
    }
}

extension ServerChatView {
    /// `-screen server-chat-sample` shows a fixed, fictional conversation for
    /// layout checks without an account or a network request. Debug builds only.
    static var debugSeed: [ChatMessage] {
        #if DEBUG
        guard ProcessInfo.processInfo.arguments.contains("server-chat-sample") else { return [] }
        return [
            ChatMessage(role: .user, text: "What was decided about the regatta date?"),
            ChatMessage(role: .assistant, text: "The regatta was set for **Saturday the 19th** because it avoids the ferry maintenance window.\n\n- Priya books the slipway\n- Tomas orders twelve buoys"),
        ]
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
