import SwiftUI
import Contracts

struct ChatView: View {
    let noteID: UUID?
    var service: any ChatService = LocalChatService()

    @Environment(NoteStore.self) private var store
    @Environment(\.dismiss) private var dismiss
    @Environment(\.verticalSizeClass) private var verticalSizeClass
    @Environment(\.motion) private var motion
    @State private var messages: [ChatMessage] = []
    @State private var draft = ""
    @State private var model = "Auto"
    @State private var streamTask: Task<Void, Never>?
    @State private var activeReply: UUID?
    @State private var history: [ChatConversation] = []
    @State private var showHistory = false
    @State private var deltaBuffer: ChatDeltaBuffer?
    @State private var followsTail = true
    /// Height of the scroll view above the composer; see `followsChatTail`.
    @State private var room: CGFloat = 0
    @State private var readerIsScrolling = false
    @State private var sentCount = 0
    @Environment(Router.self) private var router
    @FocusState private var focused: Bool

    private var note: Note? { noteID.flatMap(store.note) }
    private var isGenerating: Bool { streamTask != nil }

    var body: some View {
        RailScaffold {
            VStack(alignment: .leading, spacing: 0) {
                Text(note?.displayTitle ?? "Ask anything")
                    .font(.system(size: 15, weight: .semibold))
                    .foregroundStyle(Theme.chatText)
                    .lineLimit(1)
                    .padding(.top, RailMetrics.chatTitleInsets(compact: verticalSizeClass == .compact).top)
                    .padding(.bottom, RailMetrics.chatTitleInsets(compact: verticalSizeClass == .compact).bottom)
                    .padding(.leading, Theme.gutter)
                    .padding(.trailing, ChatColumn.trailing)

                ScrollViewReader { proxy in
                    ScrollView {
                        // Not lazy, for the reason given in `ServerChatView`: exact heights.
                        VStack(alignment: .leading, spacing: 22) {
                            if messages.isEmpty { emptyState.transition(motion.fade) }
                            ForEach(messages) { msg in
                                ChatMessageView(message: msg).equatable().id(msg.id)
                                    .transition(msg.role == .user ? motion.pop(anchor: .bottomTrailing) : motion.rise)
                            }
                            Color.clear.frame(height: ChatTail.endSpace).id("bottom")
                        }
                        .padding(.leading, ChatColumn.leading)
                        .padding(.trailing, ChatColumn.trailing)
                        .padding(.top, 8)
                        .fillsChatRoom(room)
                        // Existing text settles without a bounce; the new question pops through its own transition.
                        .animation(motion.settle, value: messages.count)
                    }
                    .scrollIndicators(.hidden)
                    .scrollDismissesKeyboard(.interactively)
                    .onScrollPhaseChange { _, phase in
                        readerIsScrolling = phase == .tracking || phase == .interacting || phase == .decelerating
                    }
                    .onScrollGeometryChange(for: Bool.self) { geometry in
                        ChatTail.isNearEnd(visibleMaxY: geometry.visibleRect.maxY, bottomInset: geometry.contentInsets.bottom, contentHeight: geometry.contentSize.height)
                    } action: { _, nearBottom in
                        followsTail = ChatTail.follows(current: followsTail, readerIsScrolling: readerIsScrolling, nearEnd: nearBottom)
                    }
                    // Follow the reply only while the reader is at the end, through to the
                    // actions that appear under it when it finishes.
                    .followsChatTail(followsTail, room: $room)
                    .chatTopFade()
                    .onChange(of: messages.count) { followsTail = true; proxy.scrollTo("bottom", anchor: .chatEnd) }
                    // The composer floats over the conversation, so messages pass
                    // under its glass instead of being cut off above it.
                    .safeAreaBar(edge: .bottom, spacing: 0) {
                        ChatComposer(draft: $draft, model: $model, isGenerating: isGenerating, focused: $focused,
                                 onSend: send, onStop: stop)
                            .chatBarBackdrop()
                    }
                }
            }
        } leading: {
            GlassCircleButton(systemImage: "chevron.left") { stop(); dismiss() }
        } rail: {
            RailGroup {
                RailIcon(systemImage: "square.and.pencil") { newConversation() }
                RailIcon(systemImage: "clock.arrow.circlepath") { focused = false; showHistory = true }
            }
        } bottom: {
            EmptyView()
        }
        .toolbarVisibility(.visible, for: .navigationBar)
        .navigationBarBackButtonHidden()
        .onDisappear {
            if ChatStreamPolicy.shouldCancelOnDisappear(path: router.path, route: .chat(noteID: noteID)) { stop() }
        }
        .sensoryFeedback(.impact(weight: .light), trigger: sentCount)
        .sheet(isPresented: $showHistory) {
            NavigationStack {
                List {
                    if history.isEmpty {
                        ContentUnavailableView("No previous chats", systemImage: "bubble.left.and.bubble.right", description: Text("Start a new chat to keep this conversation in the session history."))
                            .listRowBackground(Color.clear)
                    }
                    ForEach(history) { conversation in
                        Button {
                            stop()
                            focused = false
                            messages = conversation.messages
                            draft = ""
                            showHistory = false
                        } label: {
                            VStack(alignment: .leading, spacing: 5) {
                                Text(conversation.title).lineLimit(2).foregroundStyle(Theme.textPrimary)
                                Text(conversation.createdAt, style: .time).font(.caption).foregroundStyle(Theme.textSecondary)
                            }
                            .padding(.vertical, 5)
                        }
                        .listRowBackground(Theme.surface)
                    }
                }
                .scrollContentBackground(.hidden)
                .background(Color.clear)
                .navigationTitle("Chat history")
                .navigationBarTitleDisplayMode(.inline)
                .toolbar { ToolbarItem(placement: .confirmationAction) { Button("Done") { showHistory = false } } }
            }
            .glassSheet()
        }
    }

    private var emptyState: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("On-device preview")
                .font(.system(size: 13, weight: .medium))
                .foregroundStyle(Theme.textTertiary)
            Text("Preview replies show saved note excerpts. A connected chat service is not available yet.")
                .font(.system(size: 15))
                .lineSpacing(4)
                .foregroundStyle(Theme.textSecondary)
                .padding(.bottom, 8)
            ChatSuggestionChips(suggestions: suggestions) { draft = $0; send() }
        }
        .padding(.top, 8)
    }

    private var suggestions: [String] {
        note == nil
            ? ["What did I commit to this week?", "Summarize my last meeting"]
            : ["Summarize this meeting", "What are the open questions?", "Draft a follow-up email"]
    }

    private func send() {
        let prompt = draft.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !prompt.isEmpty, !isGenerating else { return }
        draft = ""
        messages.append(ChatMessage(role: .user, text: prompt))
        let reply = ChatMessage(role: .assistant, text: "", isStreaming: true)
        messages.append(reply)
        activeReply = reply.id
        sentCount += 1
        followsTail = true
        let stream = service.reply(to: prompt, context: note)
        // Tokens are gathered and shown about once a frame.
        let buffer = ChatDeltaBuffer { text in
            guard let i = messages.firstIndex(where: { $0.id == reply.id }) else { return }
            messages[i].text += text
        }
        deltaBuffer = buffer
        streamTask = Task {
            for await chunk in stream {
                guard !Task.isCancelled, activeReply == reply.id else { return }
                guard messages.contains(where: { $0.id == reply.id }) else { break }
                PerfProbe.hit("Chat.token")
                buffer.append(chunk)
            }
            buffer.flushNow()
            finish(reply.id)
        }
    }

    private func stop() {
        // Show everything received before stopping.
        deltaBuffer?.flushNow()
        deltaBuffer = nil
        streamTask?.cancel()
        if let last = messages.last, last.isStreaming { finish(last.id) }
        activeReply = nil
        streamTask = nil
    }

    private func finish(_ id: UUID) {
        guard activeReply == id else { return }
        if let i = messages.firstIndex(where: { $0.id == id }) { messages[i].isStreaming = false }
        streamTask = nil
        activeReply = nil
    }

    private func newConversation() {
        stop()
        if !messages.isEmpty {
            let title = messages.first(where: { $0.role == .user })?.text ?? "Conversation"
            history.insert(ChatConversation(title: title, messages: messages), at: 0)
        }
        withAnimation(motion.exit(.settle)) { messages.removeAll() }
        draft = ""
        focused = true
    }
}

struct ChatConversation: Identifiable, Codable {
    var id = UUID()
    var createdAt = Date()
    let title: String
    let messages: [ChatMessage]
}

struct ChatMessageView: View {
    let message: ChatMessage
    var onCitation: ((ChatCitationDTO) -> Void)? = nil
    var onMeeting: ((ChatToolMeetingDTO) -> Void)? = nil
    @State private var copied = false
    @State private var copyCount = 0
    @State private var expanded = false
    @State private var feedback: Bool?
    @Environment(\.motion) private var motion

    var body: some View {
        switch message.role {
        case .user:
            HStack {
                Spacer(minLength: 60)
                Text(message.text).readingFont(.body).foregroundStyle(Theme.chatText)
                    .padding(.horizontal, 16).padding(.vertical, 14)
                    .background(Theme.chatQuestion, in: RoundedRectangle(cornerRadius: 18, style: .continuous))
            }
        case .assistant:
            VStack(alignment: .leading, spacing: 14) {
                response
                if !message.isStreaming && !message.text.isEmpty {
                    // 40 x 44 pt targets; the glyphs keep their 40 pt pitch and
                    // the first one stays on the text column.
                    HStack(spacing: 0) {
                        Button {
                            UIPasteboard.general.string = message.text
                            copyCount += 1
                            withAnimation(motion.snappy) { copied = true }
                        } label: {
                            Image(systemName: copied ? "checkmark" : "clipboard").contentTransition(motion.symbolReplace).chatActionTarget()
                        }
                            .accessibilityLabel(copied ? "Copied answer" : "Copy answer")
                        Button { expanded = true } label: { Image(systemName: "viewfinder").chatActionTarget() }.accessibilityLabel("Expand answer")
                        Button { withAnimation(motion.snappy) { feedback = feedback == true ? nil : true } } label: {
                            Image(systemName: feedback == true ? "hand.thumbsup.fill" : "hand.thumbsup").contentTransition(motion.symbolReplace)
                                .symbolEffect(.bounce, value: feedback == true).chatActionTarget()
                        }
                            .accessibilityLabel("Mark answer helpful").accessibilityAddTraits(feedback == true ? .isSelected : [])
                        Button { withAnimation(motion.snappy) { feedback = feedback == false ? nil : false } } label: {
                            Image(systemName: feedback == false ? "hand.thumbsdown.fill" : "hand.thumbsdown").contentTransition(motion.symbolReplace)
                                .symbolEffect(.bounce, value: feedback == false).chatActionTarget()
                        }
                            .accessibilityLabel("Mark answer unhelpful").accessibilityAddTraits(feedback == false ? .isSelected : [])
                    }.font(.system(size: 15)).foregroundStyle(Theme.textTertiary).buttonStyle(SquashButtonStyle(scale: 0.86)).sensoryFeedback(.success, trigger: copyCount)
                        .padding(.leading, -11).padding(.vertical, -14)
                        // The tick confirms the copy, then gives the control back.
                        .task(id: copyCount) {
                            guard copied else { return }
                            try? await Task.sleep(for: .seconds(1.5))
                            if !Task.isCancelled { withAnimation(motion.snappy) { copied = false } }
                        }
                        .symbolEffectsRemoved(motion.reduceMotion)
                        .transition(motion.rise)
                }
            }
            // The action row settles in when a reply finishes. Saved history
            // scrolled back into view is not animated.
            .animation(motion.settle, value: message.isStreaming)
            .onChange(of: message.isStreaming) { _, streaming in
                if !streaming, !message.text.isEmpty, !message.isInterrupted {
                    AccessibilityNotification.Announcement("Answer ready").post()
                }
            }
            .textSelection(.enabled)
            .sheet(isPresented: $expanded) {
                NavigationStack {
                    ScrollView { response.padding(24).frame(maxWidth: .infinity, alignment: .leading).textSelection(.enabled) }
                        .background(Color.clear).navigationTitle("Answer").navigationBarTitleDisplayMode(.inline)
                        .toolbar { ToolbarItem(placement: .confirmationAction) { Button("Done") { expanded = false } } }
                }.glassSheet()
            }
        }
    }
    private var response: some View {
        VStack(alignment: .leading, spacing: 14) {
            if let annotations = message.annotations, !annotations.tools.isEmpty {
                ChatToolProgressView(tools: annotations.tools, streaming: message.isStreaming, interrupted: message.isInterrupted, onMeeting: openMeeting)
                    .transition(motion.rise)
            }
            if message.text.isEmpty && message.isStreaming { ThinkingIndicator().transition(motion.fade) }
            else {
                MarkdownBlocks(text: message.text, citations: message.annotations?.citations ?? [], turnID: message.annotations == nil ? nil : message.id, streaming: message.isStreaming, onCitation: openCitation)
                    .transition(motion.fade)
            }
            if message.isInterrupted {
                Label("Response interrupted", systemImage: "pause.circle").font(.caption).foregroundStyle(Theme.textSecondary)
                    .accessibilityIdentifier("chat-response-interrupted")
                    .transition(motion.fade)
            }
            if let citations = message.annotations?.citations, !citations.isEmpty {
                ChatSourceChips(citations: citations, entering: message.isStreaming, onCitation: openCitation)
                    .transition(motion.fade)
            }
        }
        .animation(motion.snappy, value: message.text.isEmpty)
        .animation(motion.snappy, value: message.isInterrupted)
        .animation(motion.settle, value: message.annotations?.tools.isEmpty ?? true)
        .animation(motion.settle, value: message.annotations?.citations.isEmpty ?? true)
    }
    private func openCitation(_ citation: ChatCitationDTO) {
        if expanded {
            expanded = false
            Task { @MainActor in try? await Task.sleep(for: .milliseconds(300)); onCitation?(citation) }
        } else { onCitation?(citation) }
    }
    private func openMeeting(_ meeting: ChatToolMeetingDTO) {
        if expanded {
            expanded = false
            Task { @MainActor in try? await Task.sleep(for: .milliseconds(300)); onMeeting?(meeting) }
        } else { onMeeting?(meeting) }
    }
}

/// Native block rendering keeps streamed Markdown selectable and preserves
/// nested lists and literal code without turning arbitrary HTML into a web view.
struct MarkdownBlocks: View {
    let text: String
    var citations: [ChatCitationDTO] = []
    var turnID: UUID? = nil
    /// New blocks of a streaming reply settle in; saved text is simply shown.
    var streaming = false
    var onCitation: ((ChatCitationDTO) -> Void)? = nil
    @Environment(\.motion) private var motion
    var body: some View {
        let _ = PerfProbe.hit("MarkdownBlocks.body")
        let blocks = ChatMarkdownCache.blocks(text)
        VStack(alignment: .leading, spacing: 10) {
            ForEach(Array(blocks.enumerated()), id: \.offset) { _, block in
                Group {
                switch block {
                case .heading(let value):
                    Text(inline(value)).font(.system(size: 17, weight: .semibold))
                        .foregroundStyle(Theme.chatText).padding(.top, 6)
                case .list(let marker, let value, let depth):
                    HStack(alignment: .firstTextBaseline, spacing: 12) {
                        Text(marker).frame(minWidth: 12, alignment: .leading)
                        Text(inline(value)).frame(maxWidth: .infinity, alignment: .leading)
                    }.padding(.leading, CGFloat(8 + depth * 14))
                case .paragraph(let value): Text(inline(value))
                case .quote(let value):
                    HStack(alignment: .top, spacing: 12) {
                        RoundedRectangle(cornerRadius: 2).fill(Theme.textTertiary).frame(width: 3)
                        Text(inline(value)).frame(maxWidth: .infinity, alignment: .leading)
                    }.fixedSize(horizontal: false, vertical: true).foregroundStyle(Theme.textSecondary)
                case .code(let value):
                    ScrollView(.horizontal) {
                        Text(verbatim: value).readingFont(.code).padding(12)
                    }.background(Theme.surface, in: RoundedRectangle(cornerRadius: 10))
                case .table(let header, let rows): MarkdownTable(header: header, rows: rows, inline: inline)
                case .space: Color.clear.frame(height: 3)
                }
                }
                // The transition carries its own animation. Text already on
                // screen is never cross-faded when the next block arrives.
                .transition(streaming ? motion.rise : .identity)
            }
        }.readingFont(.body).lineSpacing(6).foregroundStyle(Theme.chatText)
            .frame(maxWidth: .infinity, alignment: .leading)
            .environment(\.openURL, OpenURLAction { url in
                if let citation = ChatCitationMarkup.resolve(url, turnID: turnID, citations: citations) { onCitation?(citation); return .handled }
                return ["http", "https", "mailto"].contains(url.scheme?.lowercased() ?? "") ? .systemAction : .discarded
            })
    }

    /// Built once per block and citation set, then served from the cache.
    private func inline(_ s: String) -> AttributedString {
        ChatMarkdownCache.inline(s, context: cacheContext) {
            var result = (try? AttributedString(markdown: ChatCitationMarkup.markdown(s, turnID: turnID, citations: citations), options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace))) ?? AttributedString(s)
            for run in Array(result.runs) {
                if let link = run.link, !["http", "https", "mailto"].contains(link.scheme?.lowercased() ?? ""), ChatCitationMarkup.resolve(link, turnID: turnID, citations: citations) == nil { result[run.range].link = nil }
            }
            return result
        }
    }

    /// Links resolve against this turn's own citations, so they are part of the key.
    private var cacheContext: String {
        (turnID?.uuidString ?? "-") + "|" + citations.map { "\($0.id):\($0.noteId):\($0.segmentId ?? ""):\($0.transcriptRevision):\($0.source.rawValue)" }.joined(separator: ",")
    }
}

extension ChatMessageView: Equatable {
    /// Lets the list skip replies whose message has not changed while another
    /// one streams. The callbacks are bound to the message's id and are not compared.
    nonisolated static func == (lhs: ChatMessageView, rhs: ChatMessageView) -> Bool { lhs.message == rhs.message }
}

private extension View {
    func chatActionTarget() -> some View {
        frame(width: 40, height: 44).contentShape(Rectangle())
    }
}

enum ChatMarkdownBlock: Equatable, Sendable {
    case heading(String), list(marker: String, text: String, depth: Int), paragraph(String), quote(String), code(String), space
    /// A pipe table: the cells of its header row, when it has one, and of each row under it.
    case table(header: [String], rows: [[String]])
}

enum ChatMarkdownParser {
    static func blocks(_ text: String) -> [ChatMarkdownBlock] {
        var blocks: [ChatMarkdownBlock] = []
        var code: [String]? = nil
        for raw in text.components(separatedBy: "\n") {
            let line = raw.trimmingCharacters(in: .whitespaces)
            if line.hasPrefix("```") {
                if let value = code { blocks.append(.code(value.joined(separator: "\n"))); code = nil }
                else { code = [] }
                continue
            }
            if code != nil { code!.append(raw); continue }
            if line.isEmpty { blocks.append(.space); continue }
            let hashes = line.prefix { $0 == "#" }.count
            if (1...6).contains(hashes), line.dropFirst(hashes).first == " " {
                blocks.append(.heading(String(line.dropFirst(hashes + 1)))); continue
            }
            if line.hasPrefix("> ") { blocks.append(.quote(String(line.dropFirst(2)))); continue }
            if let cells = tableCells(line) {
                // Rows gather into the table above them. The line of dashes under the
                // first row makes that row the header and is not a row itself.
                if case .table(let header, let rows)? = blocks.last {
                    if isTableRule(cells) {
                        if header.isEmpty, rows.count == 1 { blocks[blocks.count - 1] = .table(header: rows[0], rows: []) }
                    } else { blocks[blocks.count - 1] = .table(header: header, rows: rows + [cells]) }
                } else if !isTableRule(cells) { blocks.append(.table(header: [], rows: [cells])) }
                continue
            }
            let depth = min(6, raw.prefix { $0.isWhitespace }.count / 2)
            if ["- ", "* ", "• "].contains(where: line.hasPrefix) {
                blocks.append(.list(marker: "•", text: String(line.dropFirst(2)), depth: depth)); continue
            }
            let digits = line.prefix { $0.isNumber }
            if !digits.isEmpty, let punctuation = line.dropFirst(digits.count).first,
               [".", ")"].contains(punctuation), line.dropFirst(digits.count + 1).first == " " {
                blocks.append(.list(marker: String(digits) + String(punctuation), text: String(line.dropFirst(digits.count + 2)), depth: depth)); continue
            }
            blocks.append(.paragraph(raw))
        }
        if let code { blocks.append(.code(code.joined(separator: "\n"))) }
        return blocks
    }

    /// The cells of a table line (`| a | b |`), or nil for any other line.
    static func tableCells(_ line: String) -> [String]? {
        guard line.hasPrefix("|"), line.count > 1 else { return nil }
        var cells = line.split(separator: "|", omittingEmptySubsequences: false).map { $0.trimmingCharacters(in: .whitespaces) }
        cells.removeFirst()
        if cells.last?.isEmpty == true { cells.removeLast() }
        return cells.isEmpty ? nil : cells
    }

    /// The `|---|:--:|` line between a table's header and its rows.
    static func isTableRule(_ cells: [String]) -> Bool {
        cells.allSatisfy { cell in cell.contains("-") && cell.allSatisfy { $0 == "-" || $0 == ":" } }
    }
}

/// A Markdown table, in a note or an answer: the header in the primary colour,
/// a hairline under each row, on the same surface as a code block. Columns
/// share the width and their text wraps, so a table never runs off the page.
struct MarkdownTable: View {
    let header: [String]
    let rows: [[String]]
    let inline: (String) -> AttributedString
    @Environment(\.displayScale) private var displayScale

    private var columns: Int { max(header.count, rows.map(\.count).max() ?? 0) }

    var body: some View {
        Grid(alignment: .topLeading, horizontalSpacing: 14, verticalSpacing: 8) {
            if !header.isEmpty {
                row(header).fontWeight(.semibold).foregroundStyle(Theme.textPrimary)
            }
            ForEach(Array(rows.enumerated()), id: \.offset) { index, cells in
                if index > 0 || !header.isEmpty {
                    Rectangle().fill(Theme.stroke).frame(height: 1 / displayScale).gridCellUnsizedAxes(.horizontal)
                }
                row(cells)
            }
        }
        .lineSpacing(3)
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Theme.surface, in: RoundedRectangle(cornerRadius: 10))
        .accessibilityElement(children: .contain)
    }

    private func row(_ cells: [String]) -> some View {
        GridRow {
            ForEach(0..<columns, id: \.self) { column in
                Text(inline(column < cells.count ? cells[column] : ""))
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
    }
}

struct ThinkingIndicator: View {
    @State private var phase: CGFloat = -1
    @Environment(\.motion) private var motion
    private var reduceMotion: Bool { motion.reduceMotion }
    var body: some View {
        HStack(spacing: 8) {
            Image(systemName: "rays").symbolEffect(.rotate, options: .repeating, isActive: !reduceMotion)
            Text("Thinking")
                .overlay {
                    GeometryReader { geometry in
                        LinearGradient(colors: [.clear, .white.opacity(0.5), .clear], startPoint: .leading, endPoint: .trailing)
                            .frame(width: 50, height: geometry.size.height)
                            .offset(x: phase * (geometry.size.width + 50))
                    }
                    .mask(Text("Thinking").font(.system(size: 15)))
                }
        }
        .font(.system(size: 15))
        .foregroundStyle(Theme.textTertiary)
        .onAppear {
            if !reduceMotion { withAnimation(.linear(duration: 1.2).repeatForever(autoreverses: false)) { phase = 1 } }
        }
        .accessibilityElement(children: .combine)
        .accessibilityLabel("Thinking")
    }
}

/// What the composer's one round button does.
enum ChatComposerRule {
    enum Action: Hashable { case voice, send, stop }

    /// While an answer arrives the button stops it. Where questions can wait their
    /// turn (`queues`), text in the field makes it a send button instead, and Stop
    /// is back as soon as the field is empty.
    static func action(isGenerating: Bool, hasText: Bool, queues: Bool) -> Action {
        if isGenerating && !(queues && hasText) { return .stop }
        return hasText ? .send : .voice
    }
}

struct ChatComposer: View {
    @Binding var draft: String
    @Binding var model: String
    let isGenerating: Bool
    var focused: FocusState<Bool>.Binding
    let onSend: () -> Void
    let onStop: () -> Void
    var serverMode = false
    /// With text typed while an answer arrives, the button sends (the caller queues the
    /// question) and Stop is offered again once the field is empty.
    var queuesWhileGenerating = false
    var availableModels: [LanguageModelSelection] = []
    var identifierPrefix = "chat"
    @State private var unavailableAction: String?
    @Environment(\.motion) private var motion
    @Environment(\.railMetrics) private var rail
    @Environment(\.verticalSizeClass) private var verticalSizeClass
    private var hasText: Bool { !draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }
    private typealias Action = ChatComposerRule.Action
    private var action: Action { ChatComposerRule.action(isGenerating: isGenerating, hasText: hasText, queues: queuesWhileGenerating) }
    private var stops: Bool { action == .stop }
    private var sends: Bool { action == .send }

    var body: some View {
        // Insets measured from the reference: the text sits 19 pt in, 18 pt
        // from the top and 20 pt above the controls; the controls sit 7 pt from the
        // sides and 8 pt from the bottom, 4 pt apart.
        VStack(alignment: .leading, spacing: 0) {
            TextField("", text: $draft, prompt: Text("Ask anything").foregroundStyle(Theme.textTertiary), axis: .vertical)
                .font(.system(size: 16))
                .foregroundStyle(Theme.chatText)
                .lineLimit(1...5)
                .focused(focused)
                .submitLabel(.send)
                .onSubmit(onSend)
                .accessibilityLabel("Ask anything")
                .accessibilityIdentifier("\(identifierPrefix)-composer")
                .padding(.horizontal, 19)
                .padding(.top, RailMetrics.composerFieldInsets(compact: verticalSizeClass == .compact).top)
                .padding(.bottom, RailMetrics.composerFieldInsets(compact: verticalSizeClass == .compact).bottom)
            HStack(spacing: 4) {
                Button {
                    unavailableAction = serverMode ? "File attachments are not supported by this instance yet. Your note and transcript are already included in the conversation." : "Attachments need a connected chat service. This preview uses only saved note text."
                } label: {
                    Image(systemName: "paperclip")
                        .font(.system(size: 19))
                        .foregroundStyle(Theme.textSecondary)
                        .frame(width: 40, height: 40)
                        .background(Theme.bgTop, in: Circle())
                }
                .buttonStyle(SquashButtonStyle())
                .accessibilityLabel("Attach a file")
                Menu {
                    Button(serverMode ? "Auto" : "On-device preview", systemImage: model == "Auto" ? "checkmark" : "circle") { model = "Auto" }
                    ForEach(Array(availableModels.enumerated()), id: \.offset) { _, choice in
                        let key = choice.provider.rawValue + ":" + choice.model
                        Button(choice.model + " · " + choice.provider.rawValue, systemImage: model == key ? "checkmark" : "circle") { model = key }
                    }
                    Button("About chat") {
                        unavailableAction = serverMode ? "Auto uses the model configured on your server." : "This preview returns saved note excerpts on your device. Model selection becomes available with a connected chat service."
                    }
                } label: {
                    HStack(spacing: 6) {
                        Text(availableModels.first(where: { $0.provider.rawValue + ":" + $0.model == model })?.model ?? model)
                        Image(systemName: "chevron.down").font(.system(size: 11, weight: .semibold))
                    }
                    .font(.system(size: 15, weight: .medium))
                    .foregroundStyle(Theme.textSecondary)
                    .padding(.leading, 16).padding(.trailing, 12.5)
                    .frame(height: 40)
                    .background(Theme.bgTop, in: Capsule())
                }
                Spacer()
                Button {
                    if stops { onStop() } else if sends { onSend() } else {
                        focused.wrappedValue = true
                        unavailableAction = "Voice input is not connected to chat yet. You can use the microphone on your keyboard to dictate a message."
                    }
                } label: {
                    Image(systemName: stops ? "stop.fill" : sends ? "arrow.up" : "mic")
                        .font(.system(size: stops ? 13 : 17, weight: .semibold))
                        .foregroundStyle(stops || sends ? Theme.chatActionGlyph : Theme.chatAction)
                        .contentTransition(motion.symbolReplace)
                        .frame(width: 40, height: 40)
                        .background(stops || sends ? Theme.chatAction : Theme.chatMicrophone, in: Circle())
                        // Voice, send and stop are one droplet: it dips and
                        // springs back as the glyph and colour change.
                        .keyframeAnimator(initialValue: 1.0, trigger: action) { content, scale in
                            content.scaleEffect(motion.reduceMotion ? 1 : scale)
                        } keyframes: { _ in
                            SpringKeyframe(0.84, duration: 0.11, spring: .snappy)
                            SpringKeyframe(1.0, duration: 0.42, spring: Spring(duration: 0.42, bounce: 0.34))
                        }
                }
                .buttonStyle(SquashButtonStyle())
                .accessibilityLabel(stops ? "Stop answer" : sends ? (isGenerating ? "Queue message" : "Send message") : "Voice input")
                .accessibilityIdentifier("\(identifierPrefix)-send")
            }
            .padding(.horizontal, 7)
            .padding(.bottom, verticalSizeClass == .compact ? 6 : 8)
        }
        // One material for the composer; its inset controls do not add nested glass.
        .glassSurface(RoundedRectangle(cornerRadius: 24, style: .continuous), tint: Theme.chatComposerTint)
        .animation(motion.snappy, value: action)
        // The bar is wider than the text column and sits 12 pt above the
        // keyboard or the home indicator. Both chat screens get the same margins.
        .padding(.leading, rail.composerLeading)
        .padding(.trailing, rail.composerTrailing)
        .padding(.bottom, verticalSizeClass == .compact ? 6 : RailMetrics.composerBottom)
        .alert(serverMode ? "Chat" : "Chat preview", isPresented: Binding(get: { unavailableAction != nil }, set: { if !$0 { unavailableAction = nil } })) {
            Button("OK", role: .cancel) { unavailableAction = nil }
        } message: { Text(unavailableAction ?? "") }
    }
}
