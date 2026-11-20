import SwiftUI
import APIClient
import Contracts

/// A citation's place in an answer's text, with the number its chip shows. `id`
/// keeps two chips side by side apart when the text is laid out.
struct CitationChipMark: TextAttribute {
    let id: Int
    let number: String
}

/// Paints each source mark of an answer as the desktop does (`citation_chips`):
/// a light grey disc (a capsule for two or more digits) in the room the mark has
/// in the text, centred on its line, with its number in small grey figures. A
/// text background cannot draw it (it fills the whole line box) and text cannot
/// change size within a line, so the room is laid out by the text and the chip is
/// painted there; the room's own figures are not drawn.
struct CitationChipRenderer: TextRenderer {
    /// The desktop's chip: a quiet fill with a little of the text's colour in it,
    /// dark on the light page and light on the dark one, so it reads on both.
    static let fill = Color(light: .black.withAlphaComponent(0.07), dark: .white.withAlphaComponent(0.1))
    /// The desktop's muted ink, a step darker on the light page.
    static let ink = Color(light: 0x76766F, dark: 0x9E9E99)

    func draw(layout: Text.Layout, in context: inout GraphicsContext) {
        for line in layout {
            // One chip per mark on the line, over every run its room was laid out in.
            var chips: [(mark: CitationChipMark, rect: CGRect, height: CGFloat)] = []
            for run in line {
                guard let mark = run[CitationChipMark.self] else { context.draw(run); continue }
                let bounds = run.typographicBounds
                if let last = chips.last, last.mark.id == mark.id {
                    chips[chips.count - 1].rect = last.rect.union(bounds.rect)
                } else {
                    chips.append((mark, bounds.rect, bounds.ascent + bounds.descent))
                }
            }
            for chip in chips {
                let tall = min(chip.height * 0.78, 18)
                let rect = CGRect(x: chip.rect.minX, y: chip.rect.midY - tall / 2, width: max(chip.rect.width, tall), height: tall)
                context.fill(Capsule().path(in: rect), with: .color(Self.fill))
                context.draw(Text(chip.mark.number).font(.system(size: 11)).foregroundStyle(Self.ink),
                             at: CGPoint(x: rect.midX, y: rect.midY), anchor: .center)
            }
        }
    }

    /// The text of a block with its source marks made into chips' rooms. Each mark keeps
    /// its link, so a tap on it opens the source.
    static func text(_ attributed: AttributedString) -> Text {
        var result = Text(verbatim: "")
        var plain = AttributedString()
        var marks = 0
        for run in attributed.runs {
            let slice = AttributedString(attributed[run.range])
            guard run.link?.scheme == "cereal-citation" else { plain.append(slice); continue }
            if !plain.characters.isEmpty { result = Text("\(result)\(Text(plain))"); plain = AttributedString() }
            let number = String(slice.characters).trimmingCharacters(in: .whitespaces)
            result = Text("\(result)\(Text(slice).customAttribute(CitationChipMark(id: marks, number: number)))")
            marks += 1
        }
        if !plain.characters.isEmpty { result = Text("\(result)\(Text(plain))") }
        return result
    }
}

struct ChatCitationSelection: Identifiable {
    let citation: ChatCitationDTO
    let accountIdentity: String
    var id: String { "\(citation.noteId)|\(citation.segmentId ?? "")|\(citation.transcriptRevision)|\(citation.id)" }
}

/// Loads the current owner-authorized note, then resolves precisely the cited
/// segment in it (`ChatCitationSource`): transcript that arrived after the answer
/// does not matter, a cited passage that was edited or removed does. A stale
/// citation never silently jumps to other text.
struct ChatCitationTranscriptSheet: View {
    let selection: ChatCitationSelection
    @Environment(ServerNotesModel.self) private var server
    @Environment(\.dismiss) private var dismiss
    @State private var note: NoteDTO?
    @State private var error: String?
    @State private var resolvedSegmentID: String?
    @Environment(\.motion) private var motion
    var body: some View {
        Group {
            if let note {
                ServerTranscriptView(note: note, focusSegmentID: resolvedSegmentID)
            } else {
                NavigationStack {
                    // Nothing for the first moment, then the mark; "unavailable" only once it is known (`Loading`).
                    Loading(while: error == nil, alignment: .center) {
                        InlineProgress(text: "Loading the cited transcript…")
                    } then: {
                        ScreenPrompt(symbol: "text.magnifyingglass", title: "Source unavailable", detail: error, bottomRoom: 40)
                    }
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .background(Theme.sheet.ignoresSafeArea())
                    .navigationTitle(selection.citation.title).navigationBarTitleDisplayMode(.inline)
                    .sheetBar()
                    .toolbar { ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } } }
                }
            }
        }
        // Loading turns into the cited transcript, or into the error, with a cross-fade.
        .animates(note?.id)
        .animates(error)
        .task(id: selection.id) {
            guard selection.accountIdentity == server.accountIdentity else { error = "Sign in to the account that owns this source."; return }
            do {
                let source = try await server.note(id: selection.citation.noteId)
                try Task.checkCancellation()
                guard selection.accountIdentity == server.accountIdentity else { throw CancellationError() }
                let segment = try ChatCitationSource.transcriptSegment(selection.citation, in: source)
                resolvedSegmentID = segment.id
                note = source
            } catch is CancellationError { }
            catch { self.error = error.localizedDescription }
        }
        // The source could not be opened: felt, as any request that fails is.
        .haptic(.error, trigger: error) { old, new in new != nil && new != old }
        .onChange(of: server.accountIdentity) { _, identity in
            if identity != selection.accountIdentity { note = nil; dismiss() }
        }
        .glassSheet(background: Theme.sheet)
    }
}

struct ChatMeetingContextPicker: View {
    let onSelect: (ServerNoteRow) -> Void
    @Environment(ServerNotesModel.self) private var server
    @Environment(\.dismiss) private var dismiss
    @State private var query = ""
    @State private var chosen = 0
    var body: some View {
        // The rows Home has loaded, by title; reaching the end asks for the next page.
        let matches = server.list.rows.filter { query.isEmpty || $0.title.localizedCaseInsensitiveContains(query) }
        NavigationStack {
            List {
                ForEach(matches) { note in
                    Button { chosen += 1; onSelect(note) } label: {
                        VStack(alignment: .leading, spacing: 3) {
                            Text(note.title.isEmpty ? "Untitled note" : note.title).uiFont(16, weight: .medium).foregroundStyle(Theme.textPrimary).lineLimit(2)
                            Text(Self.day(note)).uiFont(13).foregroundStyle(Theme.textSecondary)
                        }
                        .padding(.vertical, 9).frame(maxWidth: .infinity, alignment: .leading).contentShape(Rectangle())
                    }
                    .buttonStyle(.row)
                    .listRowBackground(Color.clear).listRowSeparator(.hidden)
                    .listRowInsets(EdgeInsets(top: 0, leading: 20, bottom: 0, trailing: 20))
                    .accessibilityIdentifier("chat-meeting-choice")
                    .onAppear { if note.id == server.list.rows.last?.id { Task { await server.loadMore() } } }
                }
            }
            .listStyle(.plain)
            .overlay {
                // A search that finds nothing says so, as Search does.
                if matches.isEmpty {
                    ScreenPrompt(symbol: "magnifyingglass", title: query.isEmpty ? "No meetings yet" : "No meetings match", bottomRoom: 40)
                }
            }
            .animates(matches.isEmpty)
            .searchable(text: $query, prompt: "Search meetings")
            .scrollContentBackground(.hidden).background(Theme.sheet.ignoresSafeArea())
            .navigationTitle("Choose a meeting").navigationBarTitleDisplayMode(.inline)
            .sheetBar()
            .toolbar { ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } } }
            // A meeting chosen: a selection, made.
            .haptic(.selection, trigger: chosen)
        }
        .glassSheet(background: Theme.sheet)
        .dynamicTypeSize(...DynamicTypeSize.accessibility1)
    }

    /// The meeting's day as Home writes it ("Today 7:47 AM", "12 Nov 7:47 AM"), not the raw ISO date.
    private static func day(_ note: ServerNoteRow) -> String {
        let created = Date.serverTimestamp(note.createdAt)
        guard let day = MeetingPresentation.timelineDate(noteDate: note.date, createdAt: note.createdAt) else { return created?.noteMetaString ?? note.date }
        // A note made on its meeting's day has a time; an imported one has only its day.
        if let created, Calendar.current.isDate(created, inSameDayAs: day) { return created.noteMetaString }
        return day.formatted(.dateTime.weekday(.abbreviated).day().month(.abbreviated))
    }
}

/// Starter questions for an empty conversation. They rise in sequence once.
struct ChatSuggestionChips: View {
    let suggestions: [String]
    let onSelect: (String) -> Void
    @State private var entrance = EntranceGate()
    var body: some View {
        ForEach(Array(suggestions.enumerated()), id: \.element) { index, suggestion in
            Button { onSelect(suggestion) } label: {
                Text(suggestion).uiFont(15).foregroundStyle(Theme.textSecondary)
                    .padding(.horizontal, 14).padding(.vertical, 10)
                    .background(Theme.surface, in: Capsule())
                    .overlay(Capsule().strokeBorder(Theme.stroke, lineWidth: 1))
                    .contentShape(Capsule().inset(by: -3))
            }
            .buttonStyle(.squash)
            .staggeredEntrance(index, gate: entrance)
        }
    }
}
