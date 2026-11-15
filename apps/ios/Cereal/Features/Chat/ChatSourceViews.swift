import SwiftUI
import APIClient
import Contracts

struct ChatToolProgressView: View {
    let tools: [ChatToolProgressDTO]
    let streaming: Bool
    let interrupted: Bool
    var onMeeting: ((ChatToolMeetingDTO) -> Void)?
    @Environment(\.motion) private var motion
    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            ForEach(tools, id: \.callId) { tool in
                DisclosureGroup {
                    if let results = tool.results {
                        if results.isEmpty { Text("No meetings returned").font(.caption).foregroundStyle(Theme.textSecondary) }
                        ForEach(results, id: \.noteId) { meeting in
                            Button { onMeeting?(meeting) } label: {
                                VStack(alignment: .leading, spacing: 3) {
                                    Text(meeting.title.isEmpty ? "Untitled note" : meeting.title).lineLimit(2)
                                    Text(meeting.date).font(.caption).foregroundStyle(Theme.textSecondary)
                                }.frame(maxWidth: .infinity, alignment: .leading).padding(.vertical, 5)
                            }.buttonStyle(.plain).disabled(onMeeting == nil)
                                .accessibilityLabel("Open meeting \(meeting.title), \(meeting.date)")
                        }
                    } else {
                        Text(status(tool)).font(.caption).foregroundStyle(Theme.textSecondary)
                    }
                } label: {
                    HStack(spacing: 8) {
                        ZStack {
                            if tool.status == .running && streaming { ProgressView().controlSize(.mini).transition(motion.swap) }
                            else { Image(systemName: tool.status == .completed ? "checkmark" : "minus.circle").transition(motion.swap) }
                        }
                        .animation(motion.snappy, value: tool.status == .running && streaming)
                        Text(tool.label).lineLimit(2)
                        if tool.status == .running && !streaming { Text(interrupted ? "Stopped" : "Incomplete").font(.caption2) }
                        if tool.status == .failed { Text("Failed").font(.caption2) }
                        Spacer(minLength: 4)
                        if let count = tool.resultCount { Text("\(count)").monospacedDigit() }
                    }.font(.system(size: 13)).foregroundStyle(Theme.textSecondary)
                }
                .tint(Theme.textSecondary).padding(10)
                .background(Theme.surface, in: RoundedRectangle(cornerRadius: 10))
                .accessibilityIdentifier("chat-tool-\(tool.callId)")
                .transition(motion.rise)
            }
        }
        .animation(streaming ? motion.settle : nil, value: tools.count)
    }
    private func status(_ tool: ChatToolProgressDTO) -> String {
        switch tool.status {
        case .completed: "Completed"
        case .failed: "Could not complete this lookup"
        case .running: streaming ? "Working" : interrupted ? "Stopped" : "Lookup did not finish"
        }
    }
}

struct ChatSourceChips: View {
    let citations: [ChatCitationDTO]
    var onCitation: ((ChatCitationDTO) -> Void)?
    @State private var entrance: EntranceGate

    /// `entering` is true while the reply that owns these sources is still
    /// arriving. Saved conversations show their chips at once, however often
    /// they scroll in and out of view.
    init(citations: [ChatCitationDTO], entering: Bool = false, onCitation: ((ChatCitationDTO) -> Void)? = nil) {
        self.citations = citations
        self.onCitation = onCitation
        _entrance = State(initialValue: EntranceGate(open: entering))
    }

    var body: some View {
        // Flat surfaces: these scroll with the conversation, and glass is kept
        // for floating chrome.
        ScrollView(.horizontal) {
                HStack(spacing: 7) {
                    ForEach(Array(citations.enumerated()), id: \.element.id) { index, citation in
                        Button { onCitation?(citation) } label: {
                            HStack(spacing: 6) {
                                Text(ChatCitationMarkup.number(citation)).fontWeight(.semibold)
                                Text(citation.title.isEmpty ? "Untitled note" : citation.title).lineLimit(1)
                                    .frame(maxWidth: 180, alignment: .leading).fixedSize(horizontal: true, vertical: false)
                                Image(systemName: citation.source == .transcript ? "text.alignleft" : "doc.text")
                            }.font(.caption).padding(.horizontal, 10).padding(.vertical, 7)
                                .background(Theme.surface, in: Capsule())
                                .overlay(Capsule().strokeBorder(Theme.stroke, lineWidth: 1))
                                .contentShape(Capsule().inset(by: -8))
                        }.buttonStyle(.squash).foregroundStyle(Theme.textSecondary).disabled(onCitation == nil)
                            // The same fallback the chip shows; VoiceOver read an untitled note as "Source 3: , transcript".
                            .accessibilityLabel("Source \(ChatCitationMarkup.number(citation)): \(citation.title.isEmpty ? "Untitled note" : citation.title), \(citation.source.rawValue)")
                            .accessibilityIdentifier("chat-source-\(citation.id)")
                            .staggeredEntrance(index, gate: entrance)
                    }
                }
        }.scrollIndicators(.hidden)
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
                    Group {
                        if let error {
                            ContentUnavailableView("Source unavailable", systemImage: "text.magnifyingglass", description: Text(error))
                        } else { ProgressView("Loading cited transcript") }
                    }
                    .navigationTitle(selection.citation.title).navigationBarTitleDisplayMode(.inline)
                    .toolbar { ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } } }
                }
            }
        }
        // Loading turns into the cited transcript, or into the error, with a cross-fade.
        .animation(motion.snappy, value: note?.id)
        .animation(motion.snappy, value: error)
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
        .onChange(of: server.accountIdentity) { _, identity in
            if identity != selection.accountIdentity { note = nil; dismiss() }
        }
        .glassSheet()
    }
}

struct ChatMeetingContextPicker: View {
    let onSelect: (ServerNoteRow) -> Void
    @Environment(ServerNotesModel.self) private var server
    @Environment(\.dismiss) private var dismiss
    @State private var query = ""
    var body: some View {
        NavigationStack {
            List {
                // The rows Home has loaded, by title; reaching the end asks for the next page.
                ForEach(server.list.rows.filter { query.isEmpty || $0.title.localizedCaseInsensitiveContains(query) }) { note in
                    Button { onSelect(note) } label: {
                        VStack(alignment: .leading, spacing: 4) {
                            Text(note.title.isEmpty ? "Untitled note" : note.title).foregroundStyle(Theme.textPrimary)
                            Text(note.date).font(.caption).foregroundStyle(Theme.textSecondary)
                        }.padding(.vertical, 5)
                    }.listRowBackground(Theme.surface)
                        .onAppear { if note.id == server.list.rows.last?.id { Task { await server.loadMore() } } }
                }
            }
            .searchable(text: $query, prompt: "Choose a meeting")
            .scrollContentBackground(.hidden).background(Color.clear)
            .navigationTitle("Meeting context").navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } } }
        }.glassSheet()
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
                Text(suggestion).font(.system(size: 15)).foregroundStyle(Theme.textSecondary)
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
