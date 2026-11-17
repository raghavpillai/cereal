import SwiftUI
import APIClient
import Contracts

/// Chat colours that the shared theme does not have, each with a light and a
/// dark value. Measured from the reference recordings of Granola's iPhone chat
/// (light): the recipe chips and the question bubble share one quiet fill
/// (#E6E6E0) on the page (#EEEEE8), the composer is an opaque white card with a
/// soft shadow, and its idle microphone sits on a pale tint of the accent.
extension Theme {
    /// The recipe chips above an empty conversation's composer.
    static var chatChip: Color { chatQuestion }
    /// The soft shadow under the composer card.
    static let chatComposerShadow = Color(light: .black.withAlphaComponent(0.07), dark: .black.withAlphaComponent(0.35))
    /// The chat panel beside a note on a wide layout, and the hairline at its leading edge.
    static var chatPanel: Color { bgTop }
    static var chatPanelEdge: Color { divider }
}

/// Recipes for an empty conversation about one meeting, as the reference names them.
enum ChatRecipes {
    static let note = ["List action items", "Write follow-up email", "List Q&A"]
}

/// One row of recipe chips, just above the composer of an empty conversation.
/// It scrolls sideways when the chips are wider than the column.
struct ChatRecipeChips: View {
    let recipes: [String]
    /// Leading and trailing room inside the row, so the first chip lines up with the composer's text.
    var margin: CGFloat = 18
    let onSelect: (String) -> Void

    var body: some View {
        ScrollView(.horizontal) {
            HStack(spacing: 12) {
                // In place as the screen slides in, as in the reference: no entrance of their own.
                ForEach(recipes, id: \.self) { recipe in
                    Button { onSelect(recipe) } label: {
                        Text(recipe).font(.system(size: 15)).foregroundStyle(Theme.chatText)
                            .lineLimit(1).fixedSize()
                            .padding(.horizontal, 12).frame(height: 36)
                            .background(Theme.chatChip, in: Capsule())
                            .contentShape(Capsule())
                    }
                    .buttonStyle(.squash)
                    .accessibilityIdentifier("chat-recipe")
                }
            }
        }
        .contentMargins(.horizontal, margin, for: .scrollContent)
        .scrollIndicators(.hidden)
    }
}

/// How a chat model is named and drawn in the composer's model menu: a product
/// name ("Sonnet 5.5", "GPT-6.1", "Gemini 4 Pro") from the model id the server
/// lists, and a glyph for its maker. The glyphs are Cereal's own, not the makers' marks.
enum ChatModelName {
    static func key(_ model: LanguageModelSelection) -> String { model.provider.rawValue + ":" + model.model }

    static func display(_ model: LanguageModelSelection) -> String { display(id: model.model) }

    static func display(id: String) -> String {
        let name = id.split(separator: "/").last.map(String.init) ?? id
        var words = name.split(whereSeparator: { $0 == "-" || $0 == "_" || $0 == " " }).map(String.init)
        // A trailing date ("20250514") or "latest" says nothing a person picks by.
        words.removeAll { ($0.count == 8 && $0.allSatisfy(\.isNumber)) || $0.lowercased() == "latest" }
        if words.first?.lowercased() == "claude", words.count > 1 { words.removeFirst() }
        guard !words.isEmpty else { return name }
        var parts: [String] = []
        for word in words {
            let lower = word.lowercased()
            if lower == "gpt" { parts.append("GPT"); continue }
            // Version numbers written with dashes ("5-5") are one version ("5.5").
            if word.allSatisfy(\.isNumber), let last = parts.last, last.allSatisfy({ $0.isNumber || $0 == "." }), !last.isEmpty {
                parts[parts.count - 1] = last + "." + word; continue
            }
            if let last = parts.last, last == "GPT", word.first?.isNumber == true { parts[parts.count - 1] = "GPT-" + word; continue }
            parts.append(word.first?.isLetter == true && word.count > 1 && !word.contains(where: \.isNumber) ? word.prefix(1).uppercased() + word.dropFirst() : word)
        }
        return parts.joined(separator: " ")
    }

    static func symbol(_ model: LanguageModelSelection) -> String {
        let id = model.model.lowercased()
        if id.contains("claude") || model.provider == .anthropic { return "asterisk" }
        if id.contains("gemini") || id.hasPrefix("google/") { return "sparkle" }
        if id.contains("grok") || id.hasPrefix("x-ai/") { return "slash.circle" }
        if id.contains("gpt") || id.hasPrefix("openai/") || model.provider == .openai { return "hexagon" }
        return "cpu"
    }

    static let autoSymbol = "wand.and.sparkles"
}

/// The composer's model control: "Auto" or the chosen model with a chevron,
/// opening the "Chat model" menu with each model the server offers and Auto,
/// the default, last.
struct ChatModelMenu: View {
    @Binding var model: String
    let available: [LanguageModelSelection]

    private var label: String {
        available.first { ChatModelName.key($0) == model }.map(ChatModelName.display) ?? "Auto"
    }

    var body: some View {
        Menu {
            // Toggles in a titled section: the menu draws the check beside the chosen model's glyph.
            Section("Chat model") {
                ForEach(Array(available.enumerated()), id: \.offset) { _, choice in
                    let key = ChatModelName.key(choice)
                    Toggle(isOn: Binding(get: { model == key }, set: { if $0 { model = key } })) {
                        Label(ChatModelName.display(choice), systemImage: ChatModelName.symbol(choice))
                    }
                }
                Toggle(isOn: Binding(get: { model == "Auto" }, set: { if $0 { model = "Auto" } })) {
                    Label("Auto (default)", systemImage: ChatModelName.autoSymbol)
                }
            }
        } label: {
            HStack(spacing: 6) {
                Text(label).lineLimit(1)
                Image(systemName: "chevron.down").font(.system(size: 11, weight: .semibold))
            }
            .font(.system(size: 15, weight: .medium))
            .foregroundStyle(Theme.textSecondary)
            .padding(.leading, 16).padding(.trailing, 12.5)
            .frame(height: 40)
            .background(Theme.bgTop, in: Capsule())
        }
        .menuOrder(.fixed)
        // Monochrome glyphs, as in the reference, not the app's gold.
        .tint(Theme.textPrimary)
        .accessibilityLabel("Chat model, \(label)")
    }
}

/// The spiral beside "Thinking", drawn rather than taken from a symbol set: an
/// Archimedean spiral of two and a half turns from the centre out.
struct ThinkingSpiral: Shape {
    var turns: Double = 2.4
    func path(in rect: CGRect) -> Path {
        var path = Path()
        let center = CGPoint(x: rect.midX, y: rect.midY)
        let radius = min(rect.width, rect.height) / 2
        let steps = 160
        for step in 0...steps {
            let t = Double(step) / Double(steps)
            let angle = t * turns * 2 * .pi
            let r = radius * t
            let point = CGPoint(x: center.x + r * cos(angle), y: center.y + r * sin(angle))
            if step == 0 { path.move(to: point) } else { path.addLine(to: point) }
        }
        return path
    }
}

/// The spiral, turning while an answer is thought about.
struct ThinkingIndicator: View {
    var size: CGFloat = 13
    @State private var turning = false
    @Environment(\.motion) private var motion

    var body: some View {
        ThinkingSpiral()
            .stroke(style: StrokeStyle(lineWidth: size * 0.12, lineCap: .round, lineJoin: .round))
            .frame(width: size, height: size)
            .rotationEffect(.degrees(turning ? 360 : 0))
            .onAppear {
                guard !motion.reduceMotion else { return }
                withAnimation(.linear(duration: 1.6).repeatForever(autoreverses: false)) { turning = true }
            }
            .accessibilityHidden(true)
    }
}

/// A chat beside a note on a wide layout. `hasRail`: the window has a trailing
/// rail (an unfolded Duo), and the panel's controls go there; otherwise they
/// sit in the panel's own header.
struct ChatPanelContext {
    var hasRail: Bool
    var close: () -> Void
}

/// Where a `RailScaffold`'s navigation controls go. Beside an open chat panel
/// the rail (or top bar) belongs to the chat, and the note draws its own
/// controls at the top of its column, as the reference does on an unfolded Duo.
enum RailControlsPlacement: Equatable {
    case toolbar
    /// In the content's own top row; `hidesBar` hides the empty top bar on a layout without a rail.
    case inline(hidesBar: Bool)
}

extension EnvironmentValues {
    @Entry var railControlsPlacement: RailControlsPlacement = .toolbar
}

/// The note's chat beside it on a wide layout (regular width): the note keeps
/// the leading part of the window and the chat takes about 46% of it, as on an
/// unfolded Duo. It slides in from the trailing edge. If the window becomes
/// compact while it is open (a fold), `onCompact` takes the conversation on to
/// the full-screen chat; the conversation itself lives in `ServerChatSessions`.
struct NoteChatPanel: ViewModifier {
    let noteID: String
    @Binding var isPresented: Bool
    var onCompact: () -> Void
    @Environment(\.horizontalSizeClass) private var sizeClass
    @Environment(\.motion) private var motion
    @Environment(\.displayScale) private var displayScale

    /// The panel's share of the window, measured on an unfolded Duo: 400 of 876 pt.
    static func width(for available: CGFloat) -> CGFloat {
        min(440, max(320, (available * 0.457).rounded()))
    }

    func body(content: Content) -> some View {
        GeometryReader { proxy in
            let hasRail = proxy.safeAreaInsets.trailing - proxy.safeAreaInsets.leading > RailMetrics.railThreshold
            let open = isPresented && sizeClass == .regular
            HStack(spacing: 0) {
                content
                    .environment(\.railControlsPlacement, open ? .inline(hidesBar: !(hasRail && ChatPanelContext.railToolbar)) : .toolbar)
                if open {
                    ServerChatView(noteID: noteID, panel: ChatPanelContext(hasRail: hasRail) {
                        withAnimation(motion.exit(.settle)) { isPresented = false }
                    })
                    .frame(width: Self.width(for: proxy.size.width))
                    .background(Theme.chatPanel.ignoresSafeArea())
                    .overlay(alignment: .leading) {
                        Rectangle().fill(Theme.chatPanelEdge).frame(width: 1 / displayScale).ignoresSafeArea()
                    }
                    .transition(motion.reduceMotion ? .opacity : .move(edge: .trailing))
                }
            }
            .animation(motion.animation(.settle, entering: open), value: open)
        }
        .onChange(of: sizeClass) { _, now in
            if now == .compact, isPresented { isPresented = false; onCompact() }
        }
    }
}

extension View {
    func noteChatPanel(noteID: String, isPresented: Binding<Bool>, onCompact: @escaping () -> Void) -> some View {
        modifier(NoteChatPanel(noteID: noteID, isPresented: isPresented, onCompact: onCompact))
    }
}

extension ChatPanelContext {
    /// The system can put toolbar controls in a Duo's trailing rail only with the
    /// iOS 27.1 SDK. Without it the top bar spans the whole window, over the note
    /// as well, so the panel keeps its controls in its own header instead.
    static var railToolbar: Bool {
        #if compiler(>=6.4)
        if #available(iOS 27.1, *) { return true }
        #endif
        return false
    }

    /// The panel's controls go to the window's rail (an unfolded Duo, built with the rail APIs).
    var controlsInRail: Bool { hasRail && Self.railToolbar }
}

/// The chat panel's frame beside a note: the page colour, the same text column
/// as the full-screen chat, and its controls in the rail when there is one
/// (Close, then New chat and History in one capsule), as on an unfolded Duo.
struct ChatPanelScaffold<Content: View>: View {
    let panel: ChatPanelContext
    let hasMessages: Bool
    let title: String
    let onNew: () -> Void
    let onHistory: () -> Void
    @ViewBuilder var content: Content

    var body: some View {
        GeometryReader { proxy in
            let metrics = RailMetrics(contentWidth: proxy.size.width,
                                      leadingInset: proxy.safeAreaInsets.leading,
                                      trailingInset: proxy.safeAreaInsets.trailing,
                                      bottomInset: proxy.safeAreaInsets.bottom)
            ZStack {
                Theme.chatPanel.ignoresSafeArea()
                content
                    .safeAreaPadding(.leading, metrics.extraLeading)
                    .safeAreaPadding(.trailing, metrics.extraTrailing)
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            }
            .environment(\.railMetrics, metrics)
        }
        .toolbar {
            if panel.controlsInRail {
                ToolbarItemGroup(placement: .topBarLeading) {
                    GlassCircleButton(systemImage: "xmark", action: panel.close)
                        .accessibilityLabel("Close chat")
                        .environment(\.cerealNativeNavigationChrome, true)
                }
                .cerealAdaptiveAxis()
                ToolbarItemGroup(placement: .topBarTrailing) {
                    RailGroup {
                        if hasMessages { RailIcon(systemImage: "square.and.pencil", action: onNew) }
                        RailIcon(systemImage: "clock.arrow.circlepath", action: onHistory)
                    }
                    .environment(\.cerealNativeNavigationChrome, true)
                }
                .cerealAdaptiveAxis()
            }
        }
    }
}

/// The panel's controls in its own header row, where there is no rail for them.
struct ChatPanelHeaderControls: View {
    let hasMessages: Bool
    let onNew: () -> Void
    let onHistory: () -> Void
    let onClose: () -> Void
    @Environment(\.motion) private var motion

    var body: some View {
        HStack(spacing: 8) {
            if hasMessages {
                GlassCircleButton(systemImage: "square.and.pencil", size: 36, glyphSize: 14, action: onNew)
                    .transition(motion.pop)
            }
            GlassCircleButton(systemImage: "clock.arrow.circlepath", size: 36, glyphSize: 14, action: onHistory)
            GlassCircleButton(systemImage: "xmark", size: 36, glyphSize: 14, action: onClose)
                .accessibilityLabel("Close chat")
        }
        .animation(motion.snappy, value: hasMessages)
    }
}

/// The saved chats, as the reference's "Chat History" sheet shows them: a close
/// button, the chats under the day they were last used ("Today", "Yesterday",
/// "Previous 7 days", "Earlier"), each a title in bold with its age at the end of
/// the line, and a search field that narrows them by title.
struct ChatHistorySheet: View {
    let saved: ServerChatThreadList
    let local: [ChatConversation]
    let error: String?
    let onOpen: (ChatThreadSummaryDTO) -> Void
    let onOpenLocal: (ChatConversation) -> Void
    let onDelete: (String) -> Void
    let onMore: () -> Void
    let onClose: () -> Void
    @State private var query = ""

    private struct Row: Identifiable {
        let id: String
        let title: String
        let subtitle: String?
        let date: Date?
        let thread: ChatThreadSummaryDTO?
        let conversation: ChatConversation?
    }

    enum Day: Int, CaseIterable {
        case today, yesterday, week, earlier
        var title: String {
            switch self {
            case .today: "Today"
            case .yesterday: "Yesterday"
            case .week: "Previous 7 days"
            case .earlier: "Earlier"
            }
        }
        static func of(_ date: Date?, now: Date = .now, calendar: Calendar = .current) -> Day {
            guard let date else { return .earlier }
            if calendar.isDateInToday(date) { return .today }
            if calendar.isDateInYesterday(date) { return .yesterday }
            let days = calendar.dateComponents([.day], from: calendar.startOfDay(for: date), to: calendar.startOfDay(for: now)).day ?? 99
            return days < 7 ? .week : .earlier
        }
    }

    private var rows: [Row] {
        let threads = saved.listed.map { thread in
            Row(id: thread.id, title: ChatThreadTurns.title(thread), subtitle: nil,
                date: CalendarSchedule.timestamp(thread.updatedAt), thread: thread, conversation: nil)
        }
        let kept = local.map { conversation in
            Row(id: conversation.id.uuidString, title: conversation.title,
                subtitle: conversation.messages.last(where: { $0.role == .assistant && !$0.text.isEmpty }).map { ChatCitationMarkup.withoutMarkers($0.text).split(whereSeparator: \.isNewline).first.map(String.init) ?? "" },
                date: conversation.createdAt, thread: nil, conversation: conversation)
        }
        let all = (threads + kept).sorted { ($0.date ?? .distantPast) > ($1.date ?? .distantPast) }
        let needle = query.trimmingCharacters(in: .whitespaces)
        return needle.isEmpty ? all : all.filter { $0.title.localizedCaseInsensitiveContains(needle) || ($0.subtitle?.localizedCaseInsensitiveContains(needle) ?? false) }
    }

    var body: some View {
        NavigationStack {
            List {
                if let message = error ?? saved.error {
                    Text(message).font(.footnote).foregroundStyle(Theme.textSecondary).listRowBackground(Color.clear)
                }
                let rows = rows
                if rows.isEmpty {
                    ContentUnavailableView(saved.loading ? "Loading chats" : query.isEmpty ? "No previous chats" : "No chats match",
                                           systemImage: "bubble.left.and.bubble.right")
                        .listRowBackground(Color.clear).listRowSeparator(.hidden)
                }
                ForEach(Day.allCases, id: \.self) { day in
                    let section = rows.filter { Day.of($0.date) == day }
                    if !section.isEmpty {
                        Section {
                            ForEach(section) { row in
                                Button { if let thread = row.thread { onOpen(thread) } else if let conversation = row.conversation { onOpenLocal(conversation) } } label: {
                                    rowLabel(row)
                                }
                                .buttonStyle(RowPressStyle())
                                .listRowBackground(Color.clear).listRowSeparator(.hidden)
                                .listRowInsets(EdgeInsets(top: 0, leading: 20, bottom: 0, trailing: 20))
                                .accessibilityIdentifier("server-chat-thread")
                                .swipeActions {
                                    if let thread = row.thread { Button("Delete", role: .destructive) { onDelete(thread.id) } }
                                }
                            }
                        } header: {
                            Text(day.title).font(.system(size: 13)).foregroundStyle(Theme.textSecondary).textCase(nil)
                        }
                    }
                }
                if saved.more > 0, query.isEmpty {
                    Button("Show \\(saved.more) more", action: onMore)
                        .font(.footnote).disabled(saved.loading)
                        .listRowBackground(Color.clear).listRowSeparator(.hidden)
                        .accessibilityIdentifier("server-chat-more")
                }
            }
            .listStyle(.plain)
            .scrollContentBackground(.hidden).background(Color.clear)
            // The field at the foot of the sheet, as in the reference. Drawn here rather than with
            // `searchable`, whose place and form change from one SDK to the next (a toolbar button on 27).
            .safeAreaInset(edge: .bottom, spacing: 0) { searchField }
            // The header is drawn here: the close button leads and the title is centred, as in the
            // reference. A toolbar item's side differs between SDKs and devices (trailing on a Duo with 27.1).
            .safeAreaInset(edge: .top, spacing: 0) { header }
            .toolbar(.hidden, for: .navigationBar)
        }
        .presentationDetents([.medium, .large])
        .glassSheet()
    }

    private var header: some View {
        ZStack {
            Text("Chat History").font(.headline).foregroundStyle(Theme.textPrimary).accessibilityAddTraits(.isHeader)
            HStack {
                GlassCircleButton(systemImage: "xmark", size: 44, glyphSize: 17, action: onClose)
                    .accessibilityLabel("Close")
                Spacer()
            }
        }
        .padding(.horizontal, 16).padding(.top, 16).padding(.bottom, 8)
    }

    private var searchField: some View {
        HStack(spacing: 8) {
            Image(systemName: "magnifyingglass").foregroundStyle(Theme.textSecondary)
            TextField("Search", text: $query).textInputAutocapitalization(.never).autocorrectionDisabled()
                .submitLabel(.search).accessibilityIdentifier("chat-history-search")
            if !query.isEmpty {
                Button("Clear search", systemImage: "xmark.circle.fill") { query = "" }
                    .labelStyle(.iconOnly).foregroundStyle(Theme.textTertiary)
            }
        }
        .font(.system(size: 17))
        .padding(.horizontal, 16).frame(height: 48)
        .glassSurface(Capsule(), interactive: true, tint: Theme.fieldTint)
        .padding(.horizontal, 20).padding(.bottom, 8)
    }

    private func rowLabel(_ row: Row) -> some View {
        VStack(alignment: .leading, spacing: 3) {
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text(row.title).font(.system(size: 15, weight: .semibold)).foregroundStyle(Theme.textPrimary).lineLimit(1)
                Spacer(minLength: 8)
                if row.subtitle == nil, let date = row.date { age(date) }
            }
            if let subtitle = row.subtitle {
                HStack(alignment: .firstTextBaseline, spacing: 8) {
                    Text(subtitle).font(.system(size: 14)).foregroundStyle(Theme.textSecondary).lineLimit(1)
                    Spacer(minLength: 8)
                    if let date = row.date { age(date) }
                }
            }
        }
        .padding(.vertical, 9)
        .frame(maxWidth: .infinity, alignment: .leading)
        .contentShape(Rectangle())
    }

    private func age(_ date: Date) -> some View {
        Text("· " + ChatThreadTurns.age(of: date)).font(.system(size: 13)).monospacedDigit()
            .foregroundStyle(Theme.textSecondary)
            .accessibilityLabel(ChatThreadTurns.spokenAge(of: date))
    }
}
