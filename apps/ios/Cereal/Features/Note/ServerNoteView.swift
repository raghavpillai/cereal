import SwiftUI
import APIClient
import Contracts

/// The visible document chooses the editor target; omitted fields remain owned
/// by the server instead of being replaced from an older screen snapshot.
enum NoteEditorContent {
    case typed, enhanced
    var title: String { self == .enhanced ? "Enhanced notes" : "My notes" }
    func text(in note: NoteDTO) -> String { self == .enhanced ? note.enhanced : note.body }
    func update(title: String, text: String) -> UpdateNoteInput {
        UpdateNoteInput(body: self == .typed ? text : nil, date: nil, duration: nil,
            enhanced: self == .enhanced ? text : nil, title: title, transcriptSegments: nil)
    }
    static func titleUpdate(_ title: String) -> UpdateNoteInput {
        UpdateNoteInput(body: nil, date: nil, duration: nil, enhanced: nil, title: title, transcriptSegments: nil)
    }
}

/// Present one coherent immutable payload. A separate Boolean presentation flag
/// can initialize SwiftUI editor State before the selected content has changed.
struct NoteEditorDraft: Identifiable {
    let id = UUID()
    let title: String
    let text: String
    let content: NoteEditorContent

    init(note: NoteDTO, showMine: Bool) {
        content = !showMine && !note.enhanced.isEmpty ? .enhanced : .typed
        title = note.title
        text = content.text(in: note)
    }

    static func canSave(title: String, text: String, saving: Bool) -> Bool {
        !saving && title.count <= 500 && text.count <= 1_000_000
    }
}

struct ServerNoteView: View {
    let noteID: String
    @Environment(ServerNotesModel.self) private var server
    @Environment(ServerRecordingController.self) private var recording
    @Environment(\.account) private var account
    /// The note with its account, which is what a route, the recorder and a chat know it by.
    private var ref: NoteRef? { account.map { NoteRef(account: $0, id: noteID) } }
    @Environment(Router.self) private var router
    /// Not `@Environment(\.dismiss)`; see `BackButton`.
    @State private var dismiss = DismissHandle()
    @State private var loaded: NoteDTO?
    @State private var error: String?
    @State private var busy = false
    @State private var editorDraft: NoteEditorDraft?
    @State private var editingTitle = false
    @State private var titleDraft = ""
    @State private var sharing = false
    @State private var transcript = false
    @State private var showMine = false
    @State private var confirmDelete = false
    @State private var loadRevision: UInt64 = 0
    @State private var meetingContext: NoteContextDTO?
    @State private var showingMeeting = false
    /// True only for a Generate request; `busy` also covers saving and deleting.
    @State private var generating = false
    @State private var generatedCount = 0
    @State private var copiedCount = 0
    @Environment(\.motion) private var motion
    @Namespace private var glass

    init(noteID: String, showMineInitially: Bool = false) {
        self.noteID = noteID
        _showMine = State(initialValue: showMineInitially)
    }

    private var note: NoteDTO? { loaded ?? server.state.notes.first { $0.id == noteID } }

    var body: some View {
        let _ = PerfProbe.hit("ServerNoteView.body")
        RailScaffold {
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    if let note {
                        Text(note.title.isEmpty ? "Untitled note" : note.title)
                            .font(.noteTitle).lineSpacing(4).foregroundStyle(Theme.textPrimary).padding(.top, 18)
                            .accessibilityAddTraits(.isHeader)
                        NoteMetaChips(date: MeetingPresentation.metadataDate(createdAt: note.createdAt, fallbackDate: note.date, meeting: meetingContext?.meeting) ?? .distantPast,
                            attendeeCount: meetingContext?.meeting.map { MeetingPresentation.people($0).count } ?? 0,
                            onMetadata: { showingMeeting = true })
                            .padding(.top, 14).padding(.bottom, 17)
                        if let error { errorView(error).transition(motion.fade) }
                        let content = showMine ? note.body : note.enhanced
                        // The two documents cross-fade. Large text never animates its layout.
                        Group {
                            if content.isEmpty {
                                Text(emptyText(note))
                                    .foregroundStyle(Theme.textTertiary).readingFont(.body)
                            } else if !showMine {
                                ServerNoteMarkdown(text: content)
                            } else {
                                Text(content).readingFont(.body).lineSpacing(6)
                                    .foregroundStyle(Theme.textSecondary).textSelection(.enabled)
                                    .frame(maxWidth: .infinity, alignment: .leading)
                            }
                        }
                        // A new document fades in over nothing: the old text is removed
                        // at once so two layouts never ghost over each other.
                        .id(DocumentIdentity(showMine: showMine, revision: note.generatedRevision))
                        .transition(.asymmetric(insertion: motion.fade, removal: .identity))
                        if recording.isActive(ref), capturingHere || finishingHere || recording.error != nil {
                            // While recording, the live stream's line, when it has something to say; after Stop, while
                            // the stream hands over the rest, that it is finishing; then any problem.
                            VStack(alignment: .leading, spacing: 8) {
                                if capturingHere || finishingHere, let live = recording.transcript(for: ref) {
                                    LiveStatusLine(live: live, finishing: finishingHere)
                                } else if finishingHere {
                                    Text(LiveStatusText.finishing).font(.footnote)
                                }
                                if let message = recording.error {
                                    Text(message).font(.footnote).foregroundStyle(Theme.danger)
                                }
                            }.foregroundStyle(Theme.textTertiary).padding(.top, 16)
                                .transition(motion.rise)
                        }
                    } else if let error {
                        errorView(error).padding(.top, 28)
                    } else { InlineProgress(text: "Loading note…").padding(.top, 28) }
                }
                .padding(.leading, Theme.gutter).padding(.trailing, 8).clearsBottomChrome()
                .animation(motion.snappy, value: error)
                .animation(motion.snappy, value: note == nil)
                .animation(motion.snappy, value: recording.error)
                .animation(motion.snappy, value: generatingHere)
                .animation(motion.snappy, value: finishingHere)
                .animation(motion.snappy, value: note?.generatedRevision)
            }
            .refreshable { await load() }
            // The "Chat with meeting transcript" link at the foot of this note's generated notes opens its chat
            // here, signed in as the note's owner. Any other link goes to the system.
            .environment(\.openURL, OpenURLAction { url in
                guard let account, let link = AppLink.transcriptChat(url), link.noteID == noteID,
                      let instance = server.instance, link.origin.isInstance(instance) else { return .systemAction }
                router.path.append(.serverChat(account, noteID: noteID))
                return .handled
            })
        } leading: {
            BackButton()
        } standalone: {
            GlassCircleButton(systemImage: "square.and.arrow.up") { sharing = true }
                .disabled(note == nil || busy).accessibilityLabel("Share a read-only snapshot")
        } rail: {
            Menu {
                Button("Edit title", systemImage: "pencil") { titleDraft = note?.title ?? ""; editingTitle = true }
                Button("Edit notes", systemImage: "square.and.pencil") {
                    guard let note else { return }
                    loadRevision &+= 1
                    editorDraft = NoteEditorDraft(note: note, showMine: showMine)
                }
                Button("View transcript", systemImage: "text.alignleft") { transcript = true }
                Button(showMine ? "Show enhanced notes" : "Show my notes", systemImage: showMine ? "sparkles" : "text.viewfinder") {
                    withAnimation(motion.snappy) { showMine.toggle() }
                }
                Button("Copy notes", systemImage: "doc.on.doc") {
                    guard let note else { return }
                    UIPasteboard.general.string = "\(note.title)\n\n\(showMine ? note.body : note.enhanced)"
                    copiedCount += 1
                }
                Divider()
                Button(recording.isCapturing && recording.isActive(ref) ? "Stop recording" : note?.recordingState == .idle ? "Start recording" : "Resume recording", systemImage: "mic") {
                    if recording.isCapturing { recording.stop() }
                    else { openCapture() }
                }.disabled(recording.isBusy || recording.isCapturing && !recording.isActive(ref))
                if note?.generatedRevision != note?.transcriptRevision && (note?.transcriptRevision ?? 0) > 0 {
                    Button(note?.generatedRevision == nil ? "Generate notes" : "Regenerate notes", systemImage: "sparkles") { generate() }
                        .disabled(!canGenerate)
                }
                Divider()
                Button("Delete note", systemImage: "trash", role: .destructive) { confirmDelete = true }
                    .disabled(recording.isCapturing && recording.isActive(ref))
            } label: {
                RailGlyph(systemImage: "ellipsis")
            }.menuOrder(.fixed).tint(Theme.textSecondary).glass(Circle()).disabled(note == nil || busy)
                .accessibilityLabel("Note actions")
        } bottom: {
            // One glass group: Generate and the recording control split off
            // from the Ask pill and merge back into it.
            GlassChromeGroup(spacing: 12) {
                VStack(spacing: 12) {
                    // The button stays through the request and shows its own
                    // busy state, so one glass surface lasts the whole operation.
                    // After a Stop the notes are being written already, and it shows that.
                    if canGenerate || generating || generatingHere {
                        GenerateButton(title: note?.generatedRevision == nil ? "Generate notes" : "Regenerate notes", busy: generating || generatingHere) { generate() }
                            .glassMorph("generate", in: glass)
                    }
                    HStack(alignment: .center, spacing: 12) {
                        if showsScheduledStart, let meeting = meetingContext?.meeting, let label = MeetingPresentation.startLabel(meeting) {
                            Menu {
                                Button("Start recording now", systemImage: "mic") { openCapture() }
                                Button("Meeting details", systemImage: "calendar") { showingMeeting = true }
                            } label: {
                                HStack(spacing: 7) { Image(systemName: "waveform"); Text(label); Image(systemName: "chevron.up").font(.caption2) }
                                    .font(.system(size: 13)).foregroundStyle(Theme.textSecondary).padding(.horizontal, 14).frame(height: 48)
                                    .glassSurface(Capsule(), interactive: true)
                            }.accessibilityLabel("Scheduled meeting recording options")
                                .glassMorph("scheduled", in: glass)
                        }
                        if indicatorHere {
                            // The recording screen's pill, as the way back to it. Its clock and
                            // meter are leaf views, so this screen is not rebuilt 12 times a
                            // second. Stop is on the recording screen and in this note's menu.
                            RecordingPill(recorder: recording.recorder, onStop: {}, onStart: {}, onOpen: openCapture, zoomsToCapture: true)
                            .disabled(busy || recording.isBusy)
                            .accessibilityIdentifier("server-recording-control")
                            .glassMorph("recording", in: glass)
                        }
                        AskAnythingPill { if let account { router.path.append(.serverChat(account, noteID: noteID)) } }.accessibilityIdentifier("server-chat-open")
                            .glassMorph("ask", in: glass)
                    }
                }
            }
            .padding(.horizontal, 4)
            // Out of the way of the "Edit title" alert: the gold button otherwise glows
            // through the keyboard's glass underneath it.
            .opacity(editingTitle ? 0 : 1)
            .animation(motion.snappy, value: editingTitle)
            .animation(motion.animation(.gooey, entering: canGenerate || generating || generatingHere), value: canGenerate || generating || generatingHere)
            .sensoryFeedback(.success, trigger: generatedCount)
            .sensoryFeedback(.success, trigger: copiedCount)
            .animation(motion.animation(.gooey, entering: indicatorHere), value: indicatorHere)
            .animation(motion.animation(.gooey, entering: showsScheduledStart), value: showsScheduledStart)
        }
        .navigationBarBackButtonHidden().toolbarVisibility(.visible, for: .navigationBar)
        .capturesDismiss(into: dismiss)
        // This page has no field of its own. The keyboard here belongs to the
        // "Edit title" alert, and the bar must not ride up underneath that alert.
        .ignoresSafeArea(.keyboard, edges: .bottom)
        .task(id: "\(server.accountIdentity ?? "")/\(noteID)") { await load() }
        .sheet(isPresented: $showingMeeting) { MeetingContextSheet(noteID: noteID, initial: meetingContext) {
            meetingContext = $0
            Task { await load() }
        } }
        // The notes written after a Stop arrive with the note the server answered with; the page reads it.
        .onChange(of: generatingHere) { _, writing in if !writing && !capturingHere { Task { await load() } } }
        .onChange(of: recording.isBusy) { _, busy in if !busy { Task { await load() } } }
        .onChange(of: router.serverCapture?.ref) { old, new in
            if CaptureNavigation.noteReloads(note: ref, captureWas: old, captureIs: new) { Task { await load() } }
        }
        .sheet(item: $editorDraft) { draft in
                ServerNoteEditor(title: draft.title, body: draft.text, contentTitle: draft.content.title) { title, text in
                    loadRevision &+= 1
                    let updated = try await server.update(id: noteID, draft.content.update(title: title, text: text))
                    loadRevision &+= 1
                    loaded = updated
                }.id(draft.id)
        }
        .sheet(isPresented: $sharing) { ServerShareView(noteID: noteID) }
        .sheet(isPresented: $transcript) {
            if let note {
                // While this device records the note the sheet follows the growing transcript;
                // afterwards it shows the saved note.
                ServerTranscriptView(note: note, live: capturingHere ? recording.transcript(for: ref) : nil, recording: capturingHere)
            }
        }
        .alert("Delete this note?", isPresented: $confirmDelete) {
            Button("Cancel", role: .cancel) {}
            Button("Delete", role: .destructive) {
                loadRevision &+= 1
                busy = true
                Task {
                    do {
                        let account = server.accountIdentity
                        try await server.delete(id: noteID)
                        ServerChatSessions.shared.noteDeleted(noteID, accountIdentity: account)
                        try recording.forgetDeletedNote(ref)
                        dismiss()
                    }
                    catch is CancellationError { }
                    catch { self.error = error.localizedDescription }
                    busy = false
                }
            }
        } message: { Text("This deletes the note and revokes its shared links on your server.") }
        .alert("Edit title", isPresented: $editingTitle) {
            TextField("Title", text: $titleDraft)
            Button("Cancel", role: .cancel) {}
            Button("Save") {
                guard note != nil else { return }
                loadRevision &+= 1
                busy = true
                Task {
                    defer { busy = false }
                    do {
                        loaded = try await server.update(id: noteID, NoteEditorContent.titleUpdate(titleDraft))
                    }
                    catch { self.error = error.localizedDescription }
                }
            }.disabled(titleDraft.count > 500)
        }
    }

    private var capturingHere: Bool { recording.isCapturing && recording.isActive(ref) }
    /// The recording here was stopped and its notes are being written from the transcript.
    private var generatingHere: Bool { recording.isGeneratingNotes(ref) }
    /// The recording here was stopped and its audio is still being handed to the server.
    private var finishingHere: Bool { recording.isFinishingTranscript(ref) }
    /// What the page says while it has no document to show.
    private func emptyText(_ note: NoteDTO) -> String {
        if showMine { return "No typed notes yet. Choose Edit notes to add your own." }
        if finishingHere { return LiveStatusText.finishing }
        if generatingHere { return "Writing your notes from the transcript…" }
        if capturingHere || note.recordingState == .recording { return "Your notes are written from the transcript when the recording stops." }
        if ServerRecordingController.endedWithoutTranscript(note) { return ServerRecordingController.noTranscript }
        return "Record this meeting. When you stop, your notes are written from the transcript."
    }
    /// This note has a recording to return to: running, paused, or interrupted.
    private var indicatorHere: Bool { recording.isIndicator(ref) }
    private func openCapture() { if let ref { router.serverCapture = ServerCaptureTarget(ref) } }
    private var showsScheduledStart: Bool {
        guard !recording.isCapturing, note?.recordingState == .idle, let meeting = meetingContext?.meeting else { return false }
        return MeetingPresentation.startLabel(meeting) != nil
    }

    private var canGenerate: Bool {
        guard let note else { return false }
        return !busy && !recording.isBusy && !recording.isCapturing && !generatingHere &&
            note.recordingState != .recording &&
            note.transcriptRevision > 0 && note.generatedRevision != note.transcriptRevision
    }

    private func generate() {
        guard canGenerate, let note else { return }
        busy = true; generating = true; error = nil
        Task {
            defer { busy = false; generating = false }
            do {
                loaded = try await server.generate(noteID: noteID, expectedTranscriptRevision: note.transcriptRevision)
                generatedCount += 1
            }
            catch is CancellationError { }
            catch { self.error = error.localizedDescription }
        }
    }

    private func load() async {
        loadRevision &+= 1
        let revision = loadRevision
        do {
            // Both are asked for at once, so the meeting details (the chips under the
            // title) land with the note while the page is still sliding in, not a
            // round trip later, and the two are applied in one change. A context that
            // fails leaves the one shown; the Meeting sheet offers its own retry.
            async let context = try? server.noteContext(id: noteID)
            let document = try await server.note(id: noteID)
            let found = await context
            guard revision == loadRevision else { return }
            loaded = document
            error = nil
            if let found { meetingContext = found }
        }
        catch is CancellationError { }
        catch { if revision == loadRevision { self.error = error.localizedDescription } }
    }


    private func errorView(_ message: String) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(message).foregroundStyle(Theme.textSecondary)
            Button("Retry") { Task { await load() } }
                .frame(minWidth: 44, minHeight: 44, alignment: .leading).contentShape(Rectangle())
        }
    }
}

/// Stop control for a recording that continues while its note is open.
/// Reads the recorder's clock and meter in its own body.
struct RecordingStopPill: View {
    let recorder: Recorder
    let stop: () -> Void

    var body: some View {
        let _ = PerfProbe.hit("RecordingStopPill.body")
        Button(action: stop) {
            HStack(spacing: 7) {
                RecorderWaveform(recorder: recorder).accessibilityHidden(true)
                RecorderTimer(recorder: recorder, padMinutes: false).monospacedDigit()
                Image(systemName: "stop.fill").accessibilityHidden(true)
            }
            .font(.system(size: 14, weight: .medium)).frame(height: 48).padding(.horizontal, 15)
            .foregroundStyle(Theme.onAccent).glassSurface(Capsule(), interactive: true, tint: Theme.accent)
        }
        .buttonStyle(.squash)
        .accessibilityElement(children: .combine)
        .accessibilityLabel("Stop recording")
    }
}

private struct DocumentIdentity: Hashable {
    let showMine: Bool
    let revision: Int?
}

/// Native, selectable note content. Raw HTML and embedded resources remain text.
/// A note someone else shared is drawn by this same view.
struct ServerNoteMarkdown: View {
    let text: String

    var body: some View {
        let _ = PerfProbe.hit("ServerNoteMarkdown.body")
        // The same blocks as a chat answer, parsed once per text and kept: a long
        // note was split into lines and its Markdown read again on every pass.
        let blocks = ChatMarkdownCache.blocks(text)
        // Lazy: a note of several thousand lines lays out the screenful being read.
        LazyVStack(alignment: .leading, spacing: 10) {
            ForEach(Array(blocks.enumerated()), id: \.offset) { _, block in
                switch block {
                case .heading(let value):
                    Text(inline(value)).readingFont(.heading).accessibilityAddTraits(.isHeader)
                        .foregroundStyle(Theme.textPrimary).padding(.top, 10)
                case .list(let marker, let value, let depth):
                    HStack(alignment: .firstTextBaseline, spacing: 10) {
                        // A number keeps its own width, so the text of a wrapped item hangs clear of it.
                        // A sub-bullet is a hollow circle, as on the desktop and web.
                        Text(marker == "•" && depth % 2 == 1 ? "◦" : marker).foregroundStyle(Theme.textTertiary).monospacedDigit()
                        Text(inline(value)).frame(maxWidth: .infinity, alignment: .leading)
                    }.padding(.leading, CGFloat(min(4, depth) * 14))
                // A Markdown rule (the one above the chat link at the foot of generated notes) is a hairline, as on the desktop and web.
                case .paragraph(let value) where ["---", "***", "___"].contains(value.trimmingCharacters(in: .whitespaces)):
                    Rectangle().fill(Theme.textTertiary.opacity(0.35)).frame(height: 1).padding(.vertical, 10).accessibilityHidden(true)
                // An indented plain line continues the item above it (a next step's deadline and detail): it
                // starts where that item's text does, as on the desktop and web.
                case .paragraph(let value) where value.first?.isWhitespace == true:
                    let depth = min(4, max(0, value.prefix { $0.isWhitespace }.count / 2 - 1))
                    Text(inline(value.trimmingCharacters(in: .whitespaces)))
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(.leading, CGFloat(depth * 14) + 17)
                case .paragraph(let value): Text(inline(value)).frame(maxWidth: .infinity, alignment: .leading)
                case .quote(let value):
                    HStack(alignment: .top, spacing: 12) {
                        RoundedRectangle(cornerRadius: 2).fill(Theme.textTertiary).frame(width: 3)
                        Text(inline(value)).frame(maxWidth: .infinity, alignment: .leading)
                    }.fixedSize(horizontal: false, vertical: true)
                case .code(let value):
                    Text(verbatim: value).readingFont(.code).frame(maxWidth: .infinity, alignment: .leading)
                case .table(let header, let rows): MarkdownTable(header: header, rows: rows, inline: inline)
                case .space: Color.clear.frame(height: 4)
                }
            }
        }
        .readingFont(.body).lineSpacing(6).foregroundStyle(Theme.textSecondary)
        // A long document's text is replaced, never cross-faded (see `MarkdownBlocks`).
        .contentTransition(.identity)
        .textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading)
    }

    private func inline(_ text: String) -> AttributedString {
        ChatMarkdownCache.inline(text, context: "note") {
            // Markdown, plus addresses written out in full (the chat link at the foot of generated notes), linked by
            // the same rules as the desktop and web readers (WebAddresses). A link is the app's tint, underlined.
            var result = WebAddresses.markdown(text)
            for run in Array(result.runs) where run.link != nil { result[run.range].underlineStyle = .single }
            return result
        }
    }
}

struct ServerNoteEditor: View {
    @Environment(\.dismiss) private var dismiss
    @State var title = ""
    @State var text = ""
    let save: @MainActor (String, String) async throws -> Void
    let contentTitle: String
    @State private var saving = false
    @State private var error: String?

    init(title: String = "", body: String = "", contentTitle: String = "My notes", save: @escaping @MainActor (String, String) async throws -> Void) {
        _title = State(initialValue: title)
        _text = State(initialValue: body)
        self.save = save
        self.contentTitle = contentTitle
    }

    var body: some View {
        NavigationStack {
            Form {
                Section("Title") { TextField("Untitled note", text: $title) }
                Section(contentTitle) {
                    TextEditor(text: $text).frame(minHeight: 240)
                        .accessibilityLabel("Note text")
                        .accessibilityAction(named: Text("Save notes")) { saveChanges() }
                        .accessibilityAction(named: Text("Cancel editing")) { cancelEditing() }
                }
                if let error { Section { Text(error).foregroundStyle(Theme.danger) } }
                if title.count > 500 || text.count > 1_000_000 {
                    Section { Text("Use a title of 500 characters or fewer and notes of 1,000,000 characters or fewer.").foregroundStyle(Theme.danger) }
                }
            }
            .disabled(saving)
            .navigationTitle("Edit server note").navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel", action: cancelEditing).disabled(saving)
                        .accessibilityLabel("Cancel editing")
                        .accessibilityIdentifier("server-note-editor-cancel")
                        .keyboardShortcut(.escape, modifiers: [])
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button(action: saveChanges) { if saving { ProgressView() } else { Text("Save") } }
                    .disabled(!NoteEditorDraft.canSave(title: title, text: text, saving: saving))
                    .accessibilityLabel("Save notes")
                    .accessibilityIdentifier("server-note-editor-save")
                    .keyboardShortcut(.return, modifiers: .command)
                }
            }
            .interactiveDismissDisabled(saving)
        }.glassSheet()
    }

    private func cancelEditing() {
        guard !saving else { return }
        dismiss()
    }

    private func saveChanges() {
        guard NoteEditorDraft.canSave(title: title, text: text, saving: saving) else { return }
        saving = true; error = nil
        Task {
            do { try await save(title, text); dismiss() }
            catch is CancellationError { }
            catch { self.error = error.localizedDescription }
            saving = false
        }
    }
}

struct ServerTranscriptView: View {
    let note: NoteDTO
    var focusSegmentID: String? = nil
    var expectedRevision: Int? = nil
    /// The note's transcript as it grows, when this device is recording it.
    /// With it, the sheet shows new text as it arrives instead of `note`'s snapshot.
    var live: LiveTranscript? = nil
    var recording = false
    @State private var landed = false
    @Environment(\.motion) private var motion
    @Environment(\.dismiss) private var dismiss
    var body: some View {
        NavigationStack {
            Group {
                if let live, focusSegmentID == nil {
                    LiveTranscriptList(live: live, recording: recording)
                } else if let expectedRevision, expectedRevision != note.transcriptRevision {
                    ContentUnavailableView("Transcript changed", systemImage: "text.magnifyingglass", description: Text("The citation belongs to another revision. It cannot jump to different text."))
                } else {
                    ScrollViewReader { proxy in
                        ScrollView {
                            LazyVStack(alignment: .leading, spacing: 20) {
                                if note.transcriptSegments.isEmpty { Text("No transcript is saved for this note.") }
                                // The lines, and in their place the stretches that were captured and never transcribed.
                                ForEach(TranscriptPresentation.entries(note)) { entry in
                                    switch entry {
                                    case .gap(let gap): TranscriptGapRow(gap: gap).id(gap.id)
                                    case .row(let row): savedRow(row)
                                    }
                                }
                            }.textSelection(.enabled).padding(Theme.gutter).frame(maxWidth: .infinity, alignment: .leading)
                        }
                        .task(id: focusSegmentID) {
                            guard let focusSegmentID else { return }
                            landed = false
                            // A lazy list can only be taken to a row beyond the first screen once it
                            // has been laid out, so the request is made after the sheet's first frame
                            // and once more when it has settled. A long transcript opens on its cited line.
                            try? await Task.sleep(for: .milliseconds(60))
                            proxy.scrollTo(focusSegmentID, anchor: .center)
                            try? await Task.sleep(for: .milliseconds(250))
                            proxy.scrollTo(focusSegmentID, anchor: .center)
                            withAnimation(motion.settle) { landed = true }
                        }
                    }
                }
            }
            .navigationTitle(live != nil && recording ? "Transcript" : "Saved transcript").navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } } }
        }
        // One colour whether or not a recording is running behind it.
        .glassSheet(background: Theme.sheet)
    }

    private func savedRow(_ row: TranscriptPresentation.Row) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(row.caption).font(.caption).foregroundStyle(Theme.textSecondary)
            Text(row.segment.text).readingFont(.body).opacity(row.segment.isFinal ? 1 : 0.6)
        }
        .padding(focusSegmentID == row.segment.id ? 10 : 0)
        // The cited line is brighter as the sheet lands on it, then settles.
        .background(focusSegmentID == row.segment.id ? Theme.accent.opacity(landed ? 0.12 : 0.30) : .clear, in: RoundedRectangle(cornerRadius: 8))
        .accessibilityElement(children: .combine).id(row.segment.id)
        .accessibilityIdentifier("transcript-segment-\(row.segment.id)")
    }
}
