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
                    Toggle(isOn: Binding(get: { model == key }, set: { if $0 { choose(key) } })) {
                        Label(ChatModelName.display(choice), systemImage: ChatModelName.symbol(choice))
                    }
                }
                Toggle(isOn: Binding(get: { model == "Auto" }, set: { if $0 { choose("Auto") } })) {
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

    /// A different model chosen from the menu is felt as a selection; the same one again is not.
    private func choose(_ key: String) {
        guard key != model else { return }
        model = key
        Haptics.shared.play(.selection)
    }
}

/// The Cereal mark, the desktop's `icons/cereal.svg` (a 24 pt box): the bowl
/// and its two flakes, straight segments filled even-odd.
struct CerealMarkShape: Shape {
    private static let outlines: [[CGPoint]] = {
        let data = "M22.535,2.125 22.517,1.989 22.473,1.859 22.404,1.741 22.314,1.637 22.205,1.554 22.082,1.493 21.949,1.458 21.812,1.449 21.676,1.467 21.546,1.511 21.427,1.579 21.324,1.67 21.241,1.779 21.18,1.902 21.144,2.034 21.135,2.171 21.153,2.307 21.197,2.437 21.266,2.556 21.357,2.659 21.465,2.743 21.588,2.803 21.721,2.839 21.858,2.848 21.994,2.83 22.124,2.786 22.243,2.717 22.346,2.627 22.429,2.518 22.49,2.395 22.526,2.262ZM23.299,6.518 23.288,6.406 23.265,6.295 23.233,6.187 23.189,6.082 23.136,5.982 23.073,5.888 23.001,5.801 22.921,5.721 22.834,5.649 22.74,5.586 22.64,5.533 22.535,5.489 22.427,5.457 22.316,5.435 22.204,5.423 22.091,5.423 21.978,5.435 21.867,5.457 21.759,5.489 21.654,5.533 21.554,5.586 21.46,5.649 21.373,5.721 21.293,5.801 21.221,5.888 21.158,5.982 21.105,6.082 21.062,6.187 21.029,6.295 21.007,6.406 20.996,6.518 20.996,6.632 21.007,6.744 21.029,6.855 21.062,6.963 21.105,7.068 21.158,7.168 21.221,7.262 21.293,7.349 21.373,7.429 21.46,7.501 21.554,7.564 21.654,7.617 21.759,7.66 21.867,7.693 21.978,7.715 22.091,7.726 22.204,7.726 22.316,7.715 22.427,7.693 22.535,7.66 22.64,7.617 22.74,7.564 22.834,7.501 22.921,7.429 23.001,7.349 23.073,7.262 23.136,7.168 23.189,7.068 23.233,6.963 23.265,6.855 23.288,6.744 23.299,6.632ZM13.882,2.719 13.884,2.616 13.859,2.521 13.808,2.433 13.735,2.361 13.682,2.329 13.62,2.305 13.171,2.197 12.834,2.13 12.502,2.075 12.168,2.031 11.833,1.998 11.497,1.976 11.16,1.965 10.823,1.965 10.486,1.976 9.983,2.014 9.648,2.052 9.321,2.101 8.818,2.196 8.327,2.315 7.843,2.458 7.524,2.566 7.214,2.683 6.743,2.882 6.289,3.102 5.845,3.344 5.414,3.608 4.996,3.892 4.593,4.196 4.205,4.52 3.833,4.862 3.479,5.223 3.253,5.472 3.039,5.724 2.724,6.128 2.434,6.541 2.163,6.968 1.914,7.407 1.687,7.859 1.483,8.321 1.301,8.792 1.143,9.272 1.008,9.759 0.897,10.252 0.811,10.75 0.749,11.251 0.712,11.755 0.7,12.26 0.713,12.765 0.75,13.269 0.812,13.771 0.898,14.268 1.009,14.761 1.144,15.248 1.303,15.728 1.484,16.199 1.689,16.661 1.917,17.112 2.166,17.552 2.436,17.979 2.727,18.392 3.038,18.79 3.368,19.172 3.717,19.538 4.083,19.887 4.465,20.217 4.864,20.527 5.277,20.818 5.704,21.088 6.143,21.337 6.595,21.564 7.057,21.769 7.528,21.951 8.008,22.109 8.495,22.244 8.982,22.353 9.319,22.415 9.82,22.485 10.323,22.53 10.822,22.55 11.165,22.55 11.67,22.53 12.173,22.484 12.667,22.415 13.004,22.353 13.497,22.243 13.979,22.11 14.305,22.005 14.779,21.83 15.239,21.636 15.549,21.489 15.996,21.254 16.432,20.998 16.854,20.721 17.258,20.427 17.526,20.214 17.909,19.883 18.27,19.539 18.509,19.293 18.845,18.916 19.162,18.522 19.456,18.119 19.647,17.834 19.91,17.403 20.152,16.959 20.298,16.662 20.345,16.553 20.359,16.469 20.355,16.389 20.333,16.307 20.293,16.234 20.24,16.174 20.097,16.054 19.972,15.939 19.776,15.733 19.668,15.604 19.566,15.468 19.471,15.328 19.382,15.183 19.252,14.934 19.142,14.675 19.068,14.458 18.979,14.13 18.934,14.052 18.875,13.99 18.798,13.941 18.708,13.911 18.45,13.889 18.223,13.861 17.996,13.822 17.771,13.771 17.405,13.663 17.046,13.525 16.701,13.358 16.37,13.164 16.179,13.034 15.996,12.896 15.653,12.6 15.581,12.559 15.506,12.536 15.452,12.53 15.396,12.532 15.338,12.543 15.286,12.561 15.237,12.588 15.19,12.623 15.152,12.662 15.119,12.708 15.094,12.756 15.077,12.808 15.004,13.192 14.915,13.515 14.823,13.773 14.687,14.08 14.525,14.375 14.34,14.657 14.133,14.922 13.905,15.17 13.66,15.396 13.445,15.566 13.168,15.755 12.877,15.919 12.63,16.037 12.318,16.157 11.997,16.251 11.73,16.309 11.397,16.356 11.061,16.375 10.724,16.366 10.389,16.331 10.058,16.268 9.734,16.178 9.418,16.062 9.112,15.92 8.819,15.754 8.54,15.565 8.278,15.353 8.035,15.121 7.811,14.869 7.608,14.6 7.428,14.316 7.271,14.016 7.19,13.833 7.118,13.647 7.017,13.326 6.943,12.996 6.896,12.661 6.877,12.323 6.879,12.122 6.89,11.924 6.931,11.586 7,11.255 7.053,11.063 7.116,10.873 7.242,10.56 7.395,10.257 7.572,9.968 7.686,9.806 7.81,9.647 8.036,9.394 8.281,9.161 8.434,9.033 8.594,8.912 8.876,8.726 9.171,8.566 9.42,8.453 9.737,8.337 10.06,8.248 10.391,8.185 10.725,8.149 11.061,8.141 11.398,8.16 11.733,8.207 11.93,8.248 12.122,8.298 12.445,8.404 12.755,8.536 13.052,8.691 13.339,8.873 13.661,9.117 13.709,9.144 13.766,9.165 13.825,9.177 13.886,9.178 13.946,9.17 14.003,9.152 14.057,9.125 14.106,9.089 14.149,9.046 14.184,8.997 14.212,8.937 14.229,8.875 14.269,8.53 14.324,8.228 14.378,8.004 14.444,7.783 14.546,7.495 14.638,7.279 14.773,7.006 14.941,6.708 14.965,6.627 14.97,6.543 14.953,6.448 14.914,6.365 14.698,6.099 14.573,5.928 14.422,5.693 14.317,5.508 14.222,5.319 14.112,5.066 14.039,4.867 13.976,4.665 13.909,4.396 13.869,4.187 13.833,3.912 13.817,3.699 13.812,3.422 13.821,3.209 13.848,2.933Z"
        return data.split(separator: "M").map { outline in
            outline.replacingOccurrences(of: "Z", with: "").split(separator: " ").compactMap { pair -> CGPoint? in
                let xy = pair.split(separator: ",").compactMap { Double($0) }
                return xy.count == 2 ? CGPoint(x: xy[0], y: xy[1]) : nil
            }
        }.filter { !$0.isEmpty }
    }()

    func path(in rect: CGRect) -> Path {
        let scale = min(rect.width, rect.height) / 24
        let origin = CGPoint(x: rect.midX - 12 * scale, y: rect.midY - 12 * scale)
        var path = Path()
        for outline in Self.outlines {
            path.addLines(outline.map { CGPoint(x: origin.x + $0.x * scale, y: origin.y + $0.y * scale) })
            path.closeSubpath()
        }
        return path
    }
}

/// The Cereal mark beside "Thinking", moving as on the desktop: one turn a loop,
/// swung rather than ticked, breathing smaller and paler twice a turn. Still
/// with Reduce Motion.
struct ThinkingIndicator: View {
    var size: CGFloat = 13
    @Environment(\.motion) private var motion
    private static let loop: Double = 2.4

    var body: some View {
        Group {
            if motion.reduceMotion {
                mark
            } else {
                TimelineView(.animation) { context in
                    let pose = Self.pose(context.date.timeIntervalSinceReferenceDate.truncatingRemainder(dividingBy: Self.loop) / Self.loop)
                    mark.rotationEffect(.degrees(pose.turn * 360)).scaleEffect(pose.scale).opacity(pose.ink)
                }
            }
        }
        .frame(width: size, height: size)
        .accessibilityHidden(true)
    }

    private var mark: some View { CerealMarkShape().fill(style: FillStyle(eoFill: true)) }

    /// The desktop's `thinking_mark_pose`: turn (0...1), scale and ink at `progress` through the loop.
    static func pose(_ progress: Double) -> (turn: Double, scale: Double, ink: Double) {
        let p = min(max(progress, 0), 1)
        let breath = 0.5 - 0.5 * cos(2 * 2 * .pi * p)
        let turn = min(max(p - 0.55 * sin(2 * .pi * p) / (2 * .pi), 0), 1)
        return (turn, 1 - 0.12 * breath, 1 - 0.35 * breath)
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
        // The panel settling in beside the note, or leaving it; not when a fold closes it.
        .haptic(.snap, trigger: isPresented) { _, _ in sizeClass == .regular }
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
    @State private var detent = PresentationDetent.medium

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
            // Counted in days from `now`, not from the clock, so a list opened
            // just before midnight is grouped by the moment it was drawn.
            let days = calendar.dateComponents([.day], from: calendar.startOfDay(for: date), to: calendar.startOfDay(for: now)).day ?? 99
            switch days {
            case ...0: return .today
            case 1: return .yesterday
            case 2..<7: return .week
            default: return .earlier
            }
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
                                    if let thread = row.thread {
                                        Button("Delete", role: .destructive) { Haptics.shared.play(.destructive); onDelete(thread.id) }
                                    }
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
        .presentationDetents([.medium, .large], selection: $detent)
        // Pulled up to full height or back to half: felt as the detent it is.
        .haptic(.selection, trigger: detent)
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
