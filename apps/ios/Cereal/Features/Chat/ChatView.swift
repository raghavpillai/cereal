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
                            if messages.isEmpty && note == nil { emptyState.transition(motion.fade) }
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
                    // The bar draws its own fade into the page (`chatBarBackdrop`). The system's
                    // edge effect under it showed as a paler band with a hard top edge.
                    .scrollEdgeEffectHidden(true, for: .bottom)
                    .safeAreaBar(edge: .bottom, spacing: 0) {
                        ChatComposer(draft: $draft, model: $model, isGenerating: isGenerating, focused: $focused,
                                 onSend: send, onStop: stop,
                                 recipes: note != nil && messages.isEmpty ? ChatRecipes.note : [],
                                 onRecipe: { draft = $0; send() })
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
        .haptic(.tap, trigger: sentCount)
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
    /// The server thread it is saved as, when the server keeps chats.
    var threadID: String? = nil
}

struct ChatMessageView: View {
    let message: ChatMessage
    var onCitation: ((ChatCitationDTO) -> Void)? = nil
    var onMeeting: ((ChatToolMeetingDTO) -> Void)? = nil
    /// False in a conversation about one note: everything in it comes from that note, so
    /// its answers carry no source marks, as on the desktop.
    var showsSources = true
    @State private var copied = false
    @State private var copyCount = 0
    @State private var expanded = false
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
                            // Copied text carries no source marks, as on the desktop.
                            UIPasteboard.general.string = ChatCitationMarkup.withoutMarkers(message.text)
                            copyCount += 1
                            withAnimation(motion.snappy) { copied = true }
                        } label: {
                            Image(systemName: copied ? "checkmark" : "clipboard").contentTransition(motion.symbolReplace).chatActionTarget()
                        }
                            .accessibilityLabel(copied ? "Copied answer" : "Copy answer")
                        Button { expanded = true } label: { Image(systemName: "viewfinder").chatActionTarget() }.accessibilityLabel("Expand answer")
                    }.font(.system(size: 15)).foregroundStyle(Theme.textTertiary).buttonStyle(SquashButtonStyle(scale: 0.86)).haptic(.done, trigger: copyCount)
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
            // One status line, whether or not the answer searched meetings: "Thinking" and what is
            // being done, then "Thought for Ns" or "Worked for Ns", with the thoughts under it.
            ChatThoughtsView(annotations: message.annotations, thinking: message.text.isEmpty && message.isStreaming,
                             streaming: message.isStreaming, stopped: message.isInterrupted && !message.isStreaming, onMeeting: openMeeting)
                .padding(.bottom, -4)
            if !message.text.isEmpty {
                MarkdownBlocks(text: message.text, citations: showsSources ? message.annotations?.citations ?? [] : [],
                               turnID: message.annotations == nil ? nil : message.id, streaming: message.isStreaming,
                               showsSources: showsSources, onCitation: openCitation)
                    .transition(motion.fade)
            }
            if message.isInterrupted {
                Label("Response interrupted", systemImage: "pause.circle").font(.caption).foregroundStyle(Theme.textSecondary)
                    .accessibilityIdentifier("chat-response-interrupted")
                    .transition(motion.fade)
            }
        }
        .animation(motion.snappy, value: message.text.isEmpty)
        .animation(motion.snappy, value: message.isInterrupted)
        .animation(motion.settle, value: message.annotations?.tools.isEmpty ?? true)
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
    /// False: source marks are taken out of the text, with the space before them.
    var showsSources = true
    var onCitation: ((ChatCitationDTO) -> Void)? = nil
    @Environment(\.motion) private var motion
    /// Source marks are painted as chips (`CitationChipRenderer`) only where there are any.
    private var chips: Bool { showsSources && turnID != nil && !citations.isEmpty }
    var body: some View {
        let _ = PerfProbe.hit("MarkdownBlocks.body")
        let blocks = ChatMarkdownCache.blocks(text)
        VStack(alignment: .leading, spacing: 10) {
            ForEach(Array(blocks.enumerated()), id: \.offset) { _, block in
                Group {
                switch block {
                case .heading(let value):
                    text(value).font(.system(size: 17, weight: .semibold))
                        .foregroundStyle(Theme.chatText).padding(.top, 6)
                case .list(let marker, let value, let depth):
                    HStack(alignment: .firstTextBaseline, spacing: 12) {
                        Text(marker).frame(minWidth: 12, alignment: .leading)
                        text(value).frame(maxWidth: .infinity, alignment: .leading)
                    }.padding(.leading, CGFloat(8 + depth * 14))
                // Under its item's text, as the desktop and web draw it: the same row with the marker not shown.
                case .continuation(let marker, let value, let depth):
                    HStack(alignment: .firstTextBaseline, spacing: 12) {
                        Text(marker).frame(minWidth: 12, alignment: .leading).hidden().accessibilityHidden(true)
                        text(value).frame(maxWidth: .infinity, alignment: .leading)
                    }.padding(.leading, CGFloat(8 + depth * 14)).padding(.top, -4)
                case .paragraph(let value): text(value)
                case .quote(let value):
                    HStack(alignment: .top, spacing: 12) {
                        RoundedRectangle(cornerRadius: 2).fill(Theme.textTertiary).frame(width: 3)
                        text(value).frame(maxWidth: .infinity, alignment: .leading)
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
            .modifier(CitationChipsModifier(on: chips))
            // Text whose words change (sources arriving turn "[1]" into links, the
            // last block growing) is replaced, never cross-faded. A cross-fade of
            // text is drawn on the CPU every frame: when a long answer's sources
            // arrived inside the settle animation, every paragraph was redrawn for
            // the length of the spring and the screen ran at about 20 frames a second.
            .contentTransition(.identity)
            .frame(maxWidth: .infinity, alignment: .leading)
            .environment(\.openURL, OpenURLAction { url in
                if let citation = ChatCitationMarkup.resolve(url, turnID: turnID, citations: citations) { onCitation?(citation); return .handled }
                return ["http", "https", "mailto"].contains(url.scheme?.lowercased() ?? "") ? .systemAction : .discarded
            })
    }

    /// A block's text, with its source marks as chips' rooms where there are sources.
    private func text(_ s: String) -> Text {
        chips ? CitationChipRenderer.text(inline(s)) : Text(inline(s))
    }

    /// Built once per block and citation set, then served from the cache.
    private func inline(_ s: String) -> AttributedString {
        ChatMarkdownCache.inline(s, context: cacheContext) {
            let source = showsSources ? ChatCitationMarkup.markdown(s, turnID: turnID, citations: citations) : ChatCitationMarkup.withoutMarkers(s)
            var result = (try? AttributedString(markdown: source, options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace))) ?? AttributedString(s)
            for run in Array(result.runs) {
                if let link = run.link, !["http", "https", "mailto"].contains(link.scheme?.lowercased() ?? ""), ChatCitationMarkup.resolve(link, turnID: turnID, citations: citations) == nil { result[run.range].link = nil }
            }
            return result
        }
    }

    /// Links resolve against this turn's own citations, so they are part of the key.
    private var cacheContext: String {
        (showsSources ? "" : "plain|") + (turnID?.uuidString ?? "-") + "|" + citations.map { "\($0.id):\($0.noteId):\($0.segmentId ?? ""):\($0.transcriptRevision):\($0.source.rawValue)" }.joined(separator: ",")
    }
}

/// Lets the text of an answer with sources draw their chips; other text is drawn as usual.
/// Selectable text is drawn by the system and ignores a renderer (measured: the marks came
/// out as plain gold link digits), so such an answer is copied with its Copy button instead.
private struct CitationChipsModifier: ViewModifier {
    let on: Bool
    func body(content: Content) -> some View {
        if on { content.textRenderer(CitationChipRenderer()).textSelection(.disabled) } else { content }
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
    /// An indented plain line under a list item (a next step's deadline and detail): it is drawn
    /// where that item's text starts, so it carries the item's marker and depth (the marker is not shown).
    case continuation(marker: String, text: String, depth: Int)
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
            // An indented plain line straight under a list item continues that item.
            if raw.first?.isWhitespace == true {
                switch blocks.last {
                case .list(let marker, _, let depth)?, .continuation(let marker, _, let depth)?:
                    blocks.append(.continuation(marker: marker, text: line, depth: depth)); continue
                default: break
                }
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

/// What the composer's one round button does.
enum ChatComposerRule {
    /// `idle`: nothing typed yet, the send arrow dimmed, as on the desktop.
    enum Action: Hashable { case idle, send, stop }

    /// While an answer arrives the button stops it. Where questions can wait their
    /// turn (`queues`), text in the field makes it a send button instead, and Stop
    /// is back as soon as the field is empty.
    static func action(isGenerating: Bool, hasText: Bool, queues: Bool) -> Action {
        if isGenerating && !(queues && hasText) { return .stop }
        return hasText ? .send : .idle
    }
}

struct ChatComposer: View {
    @Binding var draft: String
    @Binding var model: String
    let isGenerating: Bool
    var focused: FocusState<Bool>.Binding
    let onSend: () -> Void
    let onStop: () -> Void
    /// With text typed while an answer arrives, the button sends (the caller queues the
    /// question) and Stop is offered again once the field is empty.
    var queuesWhileGenerating = false
    var availableModels: [LanguageModelSelection] = []
    var identifierPrefix = "chat"
    /// Recipe chips shown in one row just above the composer (an empty conversation).
    var recipes: [String] = []
    var onRecipe: ((String) -> Void)? = nil
    @Environment(\.motion) private var motion
    @Environment(\.railMetrics) private var rail
    @Environment(\.verticalSizeClass) private var verticalSizeClass
    private var hasText: Bool { !draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }
    private typealias Action = ChatComposerRule.Action
    private var action: Action { ChatComposerRule.action(isGenerating: isGenerating, hasText: hasText, queues: queuesWhileGenerating) }
    private var stops: Bool { action == .stop }
    private var sends: Bool { action == .send }

    var body: some View {
        VStack(spacing: 12) {
            if !recipes.isEmpty, let onRecipe {
                // The row is as wide as the composer and cut at its edges, as in the reference.
                ChatRecipeChips(recipes: recipes, margin: 2, onSelect: onRecipe)
                    .padding(.leading, rail.composerLeading).padding(.trailing, rail.composerTrailing)
                    .transition(motion.fade)
            }
            card
        }
        .animation(motion.snappy, value: recipes.isEmpty)
        // The bar is wider than the text column and sits just above the home
        // indicator, or the keyboard. Both chat screens get the same margins.
        .padding(.bottom, verticalSizeClass == .compact ? 6 : rail.composerBottom)
    }

    private var card: some View {
        // Insets measured from the reference: the text sits 19 pt in, 18 pt
        // from the top and 20 pt above the controls; the controls sit 7 pt from the
        // sides and 8 pt from the bottom, 4 pt apart.
        VStack(alignment: .leading, spacing: 0) {
            TextField("", text: $draft, prompt: Text("Ask anything").foregroundStyle(Theme.textTertiary), axis: .vertical)
                .font(.system(size: 16))
                .foregroundStyle(Theme.chatText)
                .tint(Theme.accent)
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
                // No attachments in chat: the model control leads the row.
                ChatModelMenu(model: $model, available: availableModels)
                Spacer()
                actionButton
            }
            .padding(.horizontal, 7)
            .padding(.bottom, verticalSizeClass == .compact ? 6 : 8)
        }
        // An opaque card, as in the reference: white on the light page, a lifted grey on the dark one.
        .background(Theme.chatComposer, in: RoundedRectangle(cornerRadius: 24, style: .continuous))
        .shadow(color: Theme.chatComposerShadow, radius: 14, y: 3)
        .animation(motion.snappy, value: action)
        .padding(.leading, rail.composerLeading)
        .padding(.trailing, rail.composerTrailing)
    }

    /// One round button: a gold disc with a bold dark arrow that sends, the same disc with
    /// a square that stops an answer arriving, and a quiet grey disc until there is
    /// something to send (where the reference has a microphone; chat takes no dictation).
    private var actionButton: some View {
        Button {
            if stops { onStop() } else if sends { onSend() }
        } label: {
            Image(systemName: stops ? "stop.fill" : "arrow.up")
                .font(.system(size: stops ? 13 : 18, weight: .bold))
                .foregroundStyle(stops || sends ? Theme.sendGlyph : Theme.textSecondary)
                .contentTransition(motion.symbolReplace)
                .frame(width: 40, height: 40)
                .background(stops || sends ? Theme.sendGold : Theme.quietFill, in: Circle())
                .opacity(action == .idle ? 0.45 : 1)
                // Send and stop are one droplet: it dips and springs
                // back as the glyph and colour change.
                .keyframeAnimator(initialValue: 1.0, trigger: action) { content, scale in
                    content.scaleEffect(motion.reduceMotion ? 1 : scale)
                } keyframes: { _ in
                    SpringKeyframe(0.84, duration: 0.11, spring: .snappy)
                    SpringKeyframe(1.0, duration: 0.42, spring: Spring(duration: 0.42, bounce: 0.34))
                }
        }
        .buttonStyle(SquashButtonStyle())
        .disabled(action == .idle)
        .accessibilityLabel(stops ? "Stop answer" : sends ? (isGenerating ? "Queue message" : "Send message") : "Send message")
        .accessibilityHint(action == .idle ? "Type a question to send" : "")
        .accessibilityIdentifier("\(identifierPrefix)-send")
    }
}
