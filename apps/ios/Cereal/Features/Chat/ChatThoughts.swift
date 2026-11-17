import SwiftUI
import Contracts

/// An answer's thoughts as the desktop shows them (`render_chat_status` in
/// apps/desktop/src/chat.rs): one line above the answer that says how it is
/// coming along, and under it the model's reasoning and the meeting lookups in
/// the order they happened.
enum ChatThoughts {
    enum Item: Equatable {
        /// A part of the model's reasoning: its heading, when it gave one, and the text under it.
        case reasoning(heading: String?, body: String)
        /// A meeting lookup: its place in `ChatTurnAnnotations.tools`.
        case step(Int)
    }

    /// A stretch of reasoning as its parts. Providers that summarise reasoning open each
    /// part with a line that is only `**Heading**`; anything before the first such line,
    /// or all of it when there is none, has no heading.
    static func sections(_ text: String) -> [(heading: String?, body: String)] {
        var sections: [(heading: String?, body: String)] = []
        var heading: String?
        var body: [Substring] = []
        func close() {
            let text = body.joined(separator: "\n").trimmingCharacters(in: .whitespacesAndNewlines)
            if heading != nil || !text.isEmpty { sections.append((heading, text)) }
        }
        for line in text.split(separator: "\n", omittingEmptySubsequences: false) {
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            if trimmed.hasPrefix("**"), trimmed.hasSuffix("**"), trimmed.count > 4 {
                let title = trimmed.dropFirst(2).dropLast(2).trimmingCharacters(in: .whitespaces)
                if !title.isEmpty, !title.contains("**") {
                    close()
                    heading = title; body = []
                    continue
                }
            }
            body.append(line)
        }
        close()
        return sections
    }

    /// Reasoning and lookups in the order they happened.
    static func items(_ annotations: ChatTurnAnnotations) -> [Item] {
        var items: [Item] = []
        var reasoning = annotations.reasoning[...]
        let count = annotations.tools.count
        for step in 0...count {
            while let stretch = reasoning.first, step == count || stretch.afterTools <= step {
                reasoning = reasoning.dropFirst()
                items += sections(stretch.text).map { Item.reasoning(heading: $0.heading, body: $0.body) }
            }
            if step < count { items.append(.step(step)) }
        }
        return items
    }

    /// What the answer is doing while it is thought about: the lookup under way, else the
    /// heading the model's reasoning last gave (once its line is whole). Nil: nothing more
    /// telling than "Thinking".
    static func liveHeading(_ annotations: ChatTurnAnnotations) -> String? {
        func trail(_ label: String) -> String {
            var label = label.trimmingCharacters(in: .whitespaces)
            while let last = label.last, last == "." || last == "…" { label.removeLast() }
            return label + "…"
        }
        if let running = annotations.tools.last(where: { $0.status == .running }) { return trail(running.label) }
        guard let last = annotations.reasoning.last, let end = last.text.lastIndex(of: "\n") else { return nil }
        return sections(String(last.text[...end])).reversed().compactMap(\.heading).first.map(trail)
    }

    /// Whether the turn went looking (searched, or read other meetings), as opposed to
    /// reasoning over the meeting it was given: "Worked for" then, "Thought for" otherwise.
    /// A single read of the note's own transcript is not work.
    static func worked(_ annotations: ChatTurnAnnotations) -> Bool {
        annotations.tools.contains { $0.name != .getMeetingTranscript } || annotations.tools.count > 1
    }

    /// The status line, or nil when there is nothing to say (an old answer with no
    /// timing and no thoughts).
    static func status(_ annotations: ChatTurnAnnotations?, thinking: Bool, stopped: Bool) -> String? {
        if thinking { return annotations.flatMap(liveHeading) ?? "Thinking" }
        if stopped { return "Stopped" }
        if let elapsed = annotations?.elapsedMs {
            return "\(annotations.map(worked) == true ? "Worked" : "Thought") for \(max(1, elapsed / 1000))s"
        }
        return annotations?.hasThoughts == true ? "Meeting sources" : nil
    }

    /// A meeting's day as a list prints it beside the meeting: "Today", "Yesterday",
    /// "Nov 13", and the year only when it is not this one ("Dec 30, 2025"). A date
    /// that cannot be read is printed as it came.
    static func relativeDay(_ date: String, now: Date = .now, calendar: Calendar = .current) -> String {
        let parts = date.prefix(10).split(separator: "-").compactMap { Int($0) }
        guard parts.count == 3, let day = calendar.date(from: DateComponents(year: parts[0], month: parts[1], day: parts[2])) else { return date }
        let days = calendar.dateComponents([.day], from: calendar.startOfDay(for: day), to: calendar.startOfDay(for: now)).day ?? 0
        if days == 0 { return "Today" }
        if days == 1 { return "Yesterday" }
        let style = Date.FormatStyle(date: .omitted, time: .omitted).month(.abbreviated).day()
        return calendar.component(.year, from: day) == calendar.component(.year, from: now)
            ? day.formatted(style) : day.formatted(style.year())
    }
}

/// The line above an answer and the thoughts that open under it. While nothing of the
/// answer has arrived the line turns and says what is being done ("Thinking", then
/// the lookup under way); once the answer starts it settles into "Thought for Ns", or
/// "Worked for Ns" when the turn searched. The thoughts are open while the answer is
/// thought about, fold away when it starts, and open and close from the line after that.
struct ChatThoughtsView: View {
    let annotations: ChatTurnAnnotations?
    /// Nothing of the answer has arrived yet.
    let thinking: Bool
    /// The answer is still arriving.
    let streaming: Bool
    let stopped: Bool
    var onMeeting: ((ChatToolMeetingDTO) -> Void)?
    /// The person's own choice; left alone the thoughts are open only while thinking.
    /// (Debug QA: `-qa.thoughtsOpen YES` opens every answer's thoughts, for screenshots.)
    @State private var expanded: Bool? = Self.openForQA
    private static var openForQA: Bool? {
        #if DEBUG
        UserDefaults.standard.bool(forKey: "qa.thoughtsOpen") ? true : nil
        #else
        nil
        #endif
    }
    @State private var openSteps: Set<String> = []
    @Environment(\.motion) private var motion

    private var hasThoughts: Bool { annotations?.hasThoughts == true }
    private var open: Bool { hasThoughts && (expanded ?? thinking) }

    var body: some View {
        if let label = ChatThoughts.status(annotations, thinking: thinking, stopped: stopped) {
            VStack(alignment: .leading, spacing: 0) {
                Button {
                    guard hasThoughts else { return }
                    withAnimation(motion.settle) { expanded = !open }
                } label: {
                    HStack(spacing: 6) {
                        if thinking {
                            Image(systemName: "rays").symbolEffect(.rotate, options: .repeating, isActive: !motion.reduceMotion)
                                .transition(motion.fade)
                        }
                        ZStack(alignment: .leading) {
                            if thinking { SheenLabel(text: label).id(label).transition(motion.swap) }
                            else { Text(label).id(label).transition(motion.swap) }
                        }
                        if hasThoughts {
                            Image(systemName: "chevron.right").font(.system(size: 11, weight: .semibold))
                                .rotationEffect(.degrees(open ? 90 : 0))
                        }
                    }
                    .font(.system(size: 15)).foregroundStyle(Theme.textTertiary)
                    .frame(minHeight: 30, alignment: .leading).contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .animation(motion.snappy, value: label)
                .accessibilityLabel(label)
                .accessibilityValue(hasThoughts ? (open ? "Expanded" : "Collapsed") : "")
                .accessibilityHint(hasThoughts ? (open ? "Hides the thoughts behind this answer" : "Shows the thoughts behind this answer") : "")
                .accessibilityIdentifier("chat-thoughts-status")
                if open, let annotations { rail(annotations).transition(motion.rise) }
            }
            .animation(motion.settle, value: open)
        }
    }

    /// One rail down the leading edge, as far as the thoughts go.
    private func rail(_ annotations: ChatTurnAnnotations) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            ForEach(Array(ChatThoughts.items(annotations).enumerated()), id: \.offset) { _, item in
                switch item {
                case .reasoning(let heading, let body): reasoning(heading: heading, body: body)
                case .step(let index): step(annotations.tools[index])
                }
            }
        }
        .padding(.leading, 12).padding(.top, 6).padding(.bottom, 2)
        .overlay(alignment: .leading) { Rectangle().fill(Theme.textTertiary.opacity(0.35)).frame(width: 1) }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    /// A part of the model's reasoning: its heading in fuller ink, then its paragraphs quieter.
    private func reasoning(heading: String?, body: String) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            if let heading { Text(Self.inline(heading)).foregroundStyle(Theme.textPrimary.opacity(0.8)) }
            ForEach(Array(body.components(separatedBy: "\n\n").map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }.filter { !$0.isEmpty }.enumerated()), id: \.offset) { _, paragraph in
                Text(Self.inline(paragraph)).foregroundStyle(Theme.textSecondary)
            }
        }
        .font(.system(size: 14)).lineSpacing(3)
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    /// One meeting lookup: a glyph for what it did, what it found, and (when it found
    /// meetings) a chevron that opens them, one to a row with its day at the trailing edge.
    private func step(_ tool: ChatToolProgressDTO) -> some View {
        let look = StepLook(tool, streaming: streaming)
        let results = tool.results ?? []
        let showing = !results.isEmpty && openSteps.contains(tool.callId)
        return VStack(alignment: .leading, spacing: 6) {
            Button {
                guard !results.isEmpty else { return }
                withAnimation(motion.settle) {
                    if openSteps.remove(tool.callId) == nil { openSteps.insert(tool.callId) }
                }
            } label: {
                HStack(spacing: 7) {
                    Image(systemName: look.glyph).font(.system(size: 13)).frame(width: 16)
                    if look.running { SheenLabel(text: look.label).lineLimit(1) } else { Text(look.label).lineLimit(1) }
                    if !results.isEmpty {
                        Image(systemName: "chevron.right").font(.system(size: 10, weight: .semibold))
                            .rotationEffect(.degrees(showing ? 90 : 0))
                    }
                }
                .font(.system(size: 14))
                .foregroundStyle(look.failed ? Theme.textTertiary : Theme.textSecondary)
                .frame(minHeight: 24, alignment: .leading).contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityValue(results.isEmpty ? "" : (showing ? "Expanded" : "Collapsed"))
            .accessibilityIdentifier("chat-tool-\(tool.callId)")
            if showing {
                VStack(alignment: .leading, spacing: 0) {
                    ForEach(results, id: \.noteId) { meeting in
                        Button { onMeeting?(meeting) } label: {
                            HStack(spacing: 12) {
                                Text(meeting.title.trimmingCharacters(in: .whitespaces).isEmpty ? "New note" : meeting.title)
                                    .lineLimit(1).foregroundStyle(Theme.textSecondary)
                                Spacer(minLength: 4)
                                Text(ChatThoughts.relativeDay(meeting.date)).font(.system(size: 13)).foregroundStyle(Theme.textTertiary)
                            }
                            .font(.system(size: 14)).padding(.horizontal, 8).frame(minHeight: 36).contentShape(Rectangle())
                        }
                        .buttonStyle(.plain).disabled(onMeeting == nil)
                        .accessibilityLabel("Open meeting \(meeting.title), \(ChatThoughts.relativeDay(meeting.date))")
                    }
                }
                .padding(.leading, 6)
                .overlay(alignment: .leading) { Rectangle().fill(Theme.textTertiary.opacity(0.35)).frame(width: 1) }
                .padding(.leading, 7.5)
                .transition(motion.rise)
            }
        }
    }

    private static func inline(_ text: String) -> AttributedString {
        (try? AttributedString(markdown: text, options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace))) ?? AttributedString(text)
    }

    /// How a lookup is shown: its glyph, its words, whether it is under way or did not finish.
    private struct StepLook {
        let glyph: String
        let label: String
        let running: Bool
        let failed: Bool

        init(_ tool: ChatToolProgressDTO, streaming: Bool) {
            let label = tool.label.trimmingCharacters(in: .whitespaces)
            running = tool.status == .running && streaming
            failed = tool.status == .failed || (tool.status == .running && !streaming)
            if failed {
                glyph = "exclamationmark.circle"
                if tool.status == .running { self.label = "Stopped before this finished" }
                else if label.lowercased().contains("plan") { self.label = "Couldn't plan the search" }
                else if label.isEmpty || label.count > 40 || label.contains(";") { self.label = "Couldn't finish this lookup" }
                else { self.label = label }
                return
            }
            switch tool.name {
            case .searchMeetings, .getMeetings: glyph = "doc.text"
            case .listMeetings: glyph = "calendar"
            case .searchTranscripts, .getMeetingTranscript: glyph = "text.alignleft"
            }
            var shown = label
            if running {
                while let last = shown.last, last == "." || last == "…" { shown.removeLast() }
                shown += "…"
            }
            self.label = shown
        }
    }
}

/// Words with a sheen passing over them, for what is under way.
struct SheenLabel: View {
    let text: String
    @State private var phase: CGFloat = -1
    @Environment(\.motion) private var motion

    var body: some View {
        Text(text)
            .overlay {
                GeometryReader { geometry in
                    LinearGradient(colors: [.clear, .white.opacity(0.5), .clear], startPoint: .leading, endPoint: .trailing)
                        .frame(width: 50, height: geometry.size.height)
                        .offset(x: phase * (geometry.size.width + 50))
                }
                .mask(Text(text))
                .allowsHitTesting(false)
            }
            .onAppear {
                if !motion.reduceMotion { withAnimation(.linear(duration: 1.2).repeatForever(autoreverses: false)) { phase = 1 } }
            }
    }
}
