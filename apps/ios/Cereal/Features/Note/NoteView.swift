import SwiftUI

struct NoteView: View {
    let noteID: UUID
    @Environment(NoteStore.self) private var store
    @Environment(Router.self) private var router
    @Environment(\.dismiss) private var dismiss

    @State private var mode: Mode = .enhanced
    @State private var editingTitle = false
    @State private var editingNotes = false
    @State private var showTranscript = false
    @State private var copied = false
    @State private var generating = false
    @Environment(\.motion) private var motion

    enum Mode { case enhanced, mine }

    var body: some View {
        if let note = store.note(noteID) {
            content(note)
        } else {
            Theme.background.onAppear { dismiss() }
        }
    }

    private func content(_ note: Note) -> some View {
        RailScaffold {
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    Text(note.displayTitle)
                        .font(.noteTitle)
                        .lineSpacing(4)
                        .foregroundStyle(Theme.textPrimary)
                        .padding(.top, 18)
                        .accessibilityAddTraits(.isHeader)
                        .onTapGesture(count: 2) { editingTitle = true }

                    NoteMetaChips(date: note.createdAt, attendeeCount: max(0, note.attendees.count))
                        .padding(.top, 14)
                        .padding(.bottom, 17)

                    if let hint = note.captureRecoveryHint {
                        Label(hint, systemImage: "waveform.badge.exclamationmark")
                            .font(.footnote)
                            .foregroundStyle(Theme.textSecondary)
                            .padding(.bottom, 22)
                    }

                    if note.needsGeneration {
                        GenerateButton(title: note.generateActionTitle, busy: generating) {
                            generating = true
                            Task {
                                await store.generateNotes(note.id)
                                if store.note(note.id)?.needsGeneration == false { Haptics.shared.play(.notesReady) }
                                withAnimation(motion.settle) {
                                    generating = false
                                    mode = .enhanced
                                }
                            }
                        }
                        .padding(.bottom, 24)
                        .transition(motion.pop(anchor: .leading))
                    }

                    // One document at a time (`Replaced`), as on a server note. Large text never animates its layout.
                    let enhanced = mode == .enhanced && !note.sections.isEmpty
                    Replaced(id: enhanced) {
                        if enhanced {
                            EnhancedNotesView(sections: note.sections)
                        } else {
                            Text(note.userNotes.isEmpty ? "No notes yet." : note.userNotes)
                                .readingFont(.body)
                                .lineSpacing(6)
                                .foregroundStyle(note.userNotes.isEmpty ? Theme.textTertiary : Theme.textSecondary)
                        }
                    }
                }
                .animation(motion.exit(.settle), value: note.needsGeneration)
                .textSelection(.enabled)
                .padding(.leading, Theme.gutter)
                .padding(.trailing, 8)
                .clearsBottomChrome()
            }
            .scrollIndicators(.hidden)
        } leading: {
            GlassCircleButton(systemImage: "chevron.left") { dismiss() }
        } standalone: {
            ShareLink(item: note.plainText) { RailGlyph(systemImage: "square.and.arrow.up") }
                .glass(Circle())
        } rail: {
            Menu {
                Button("Edit title", systemImage: "pencil") { editingTitle = true }
                Button("Edit notes", systemImage: "square.and.pencil") { editingNotes = true }
                Button("View transcript", systemImage: "text.alignleft") { showTranscript = true }
                Button(mode == .enhanced ? "Show my notes" : "Show enhanced notes",
                       systemImage: mode == .enhanced ? "text.viewfinder" : "sparkles") {
                    mode = mode == .enhanced ? .mine : .enhanced
                    Haptics.shared.play(.selection)
                }
                Button("Copy notes", systemImage: "doc.on.doc") {
                    UIPasteboard.general.string = note.plainText
                    copied.toggle()
                }
                Divider()
                Button(note.hasCapture ? "Resume recording" : "Start recording", systemImage: "mic") {
                    router.newNoteID = note.id
                }
                Divider()
                Button("Move to trash", systemImage: "trash", role: .destructive) {
                    Haptics.shared.play(.destructive)
                    store.trash(note.id)
                    dismiss()
                }
            } label: {
                RailGlyph(systemImage: "ellipsis")
            }
            .menuOrder(.fixed).tint(Theme.textSecondary).glass(Circle())
            .accessibilityLabel("More")
        } bottom: {
            // On a note the pill is 4 pt narrower on each side than on Home (measured).
            AskAnythingPill { router.path.append(.chat(noteID: note.id)) }
                .padding(.horizontal, 4)

        }
        .toolbarVisibility(.visible, for: .navigationBar)
        .navigationBarBackButtonHidden()
        // The keyboard here belongs to the "Edit title" alert; the bar stays where it is.
        .ignoresSafeArea(.keyboard, edges: .bottom)
        .haptic(.done, trigger: copied)
        .sheet(isPresented: $showTranscript) { TranscriptView(note: note) }
        .sheet(isPresented: $editingNotes) { EditNotesSheet(note: note) }
        .alert("Edit title", isPresented: $editingTitle) {
            TitleEditField(note: note)
        }
    }
}

private struct TitleEditField: View {
    @Environment(NoteStore.self) private var store
    let note: Note
    @State private var text = ""
    var body: some View {
        TextField("Title", text: $text).onAppear { text = note.title }
        Button("Cancel", role: .cancel) {}
        Button("Save") { store.updateTitle(note.id, title: text) }
    }
}

struct EnhancedNotesView: View {
    let sections: [NoteSection]
    var body: some View {
        VStack(alignment: .leading, spacing: 26) {
            ForEach(sections) { section in
                VStack(alignment: .leading, spacing: 6) {
                    Text(section.heading)
                        .readingFont(.heading)
                        .accessibilityAddTraits(.isHeader)
                        .foregroundStyle(Theme.textPrimary)
                    VStack(alignment: .leading, spacing: 10) {
                        ForEach(section.bullets, id: \.self) { bullet in
                            HStack(alignment: .firstTextBaseline, spacing: 12) {
                                Circle().fill(Theme.textSecondary).frame(width: 6.5, height: 6.5)
                                    .alignmentGuide(.firstTextBaseline) { $0[.bottom] + 1 }
                                Text(markdown(bullet))
                                    .readingFont(.body)
                                    .lineSpacing(5)
                                    .foregroundStyle(Theme.textSecondary)
                            }
                            .padding(.leading, 7)
                        }
                    }
                }
            }
        }
    }

    private func markdown(_ s: String) -> AttributedString {
        (try? AttributedString(markdown: s)) ?? AttributedString(s)
    }
}

struct TranscriptView: View {
    let note: Note
    @Environment(\.dismiss) private var dismiss
    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 18) {
                    if note.transcript.isEmpty {
                        Text("No transcript yet.").foregroundStyle(Theme.textTertiary)
                    }
                    ForEach(note.transcript) { seg in
                        VStack(alignment: .leading, spacing: 4) {
                            Text("\(seg.speaker) · \(Duration.seconds(seg.startedAt).formatted(.time(pattern: .minuteSecond)))")
                                .uiFont(13, weight: .medium)
                                .foregroundStyle(Theme.textTertiary)
                            Text(seg.text).readingFont(.body).foregroundStyle(Theme.textPrimary)
                        }
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(Theme.gutter)
            }
            .navigationTitle("Transcript")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } } }
        }
        .glassSheet()
    }
}

struct EditNotesSheet: View {
    @Environment(NoteStore.self) private var store
    @Environment(\.dismiss) private var dismiss
    let note: Note
    @State private var text = ""

    var body: some View {
        NavigationStack {
            TextEditor(text: $text)
                .readingFont(.body)
                .scrollContentBackground(.hidden)
                .padding(.horizontal, 12)
                .navigationTitle("My notes")
                .navigationBarTitleDisplayMode(.inline)
                .toolbar {
                    ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
                    ToolbarItem(placement: .confirmationAction) {
                        Button("Save") { store.updateUserNotes(note.id, text: text); dismiss() }
                    }
                }
        }
        .onAppear { text = note.userNotes }
        .glassSheet()
    }
}
