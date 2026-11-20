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
    /// What went wrong, with what was being done: "Try again" does that again.
    @State private var failure: NoteFailure?
    private var error: String? { failure?.message }
    @State private var busy = false
    @State private var editorDraft: NoteEditorDraft?
    @State private var editingTitle = false
    @State private var titleDraft = ""
    @State private var sharing = false
    @State private var sharePopover = false
    /// "Manage links…" was chosen in the popover: the sheet rises as the popover goes.
    @State private var managesLinks = false
    /// The title being saved, shown from the moment Save is tapped.
    @State private var pendingTitle: String?
    @State private var transcript = false
    @State private var showMine = false
    @State private var confirmDelete = false
    @State private var loadRevision: UInt64 = 0
    @State private var meetingContext: NoteContextDTO?
    @State private var showingMeeting = false
    /// True only for a Generate request; `busy` also covers saving and deleting.
    @State private var generating = false
    /// Something the person asked for failed (generate, delete, a new title): felt as an error.
    @State private var failures = 0
    /// Delete was confirmed: felt as the firm tap of something that cannot be undone.
    @State private var deletions = 0
    @State private var copied = Copied()
    /// "Get notified when your notes are ready" is offered (notifications never asked for).
    @State private var offersNotify = false
    @Environment(\.motion) private var motion
    @Namespace private var glass
    /// On a wide layout the note's chat opens beside it (`NoteChatPanel`) instead of over it.
    @State private var chatPanel = false
    @Environment(\.horizontalSizeClass) private var sizeClass

    init(noteID: String, showMineInitially: Bool = false) {
        self.noteID = noteID
        _showMine = State(initialValue: showMineInitially)
    }

    private var note: NoteDTO? { loaded ?? server.state.notes.first { $0.id == noteID } }

    /// The note's chat: beside the note on a wide layout, over it otherwise.
    private func openChat() {
        if sizeClass == .regular { withAnimation(motion.settle) { chatPanel = true } }
        else if let account { router.path.append(.serverChat(account, noteID: noteID)) }
    }

    var body: some View {
        let _ = PerfProbe.hit("ServerNoteView.body")
        RailScaffold {
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    if let note {
                        // An untitled note's title is the placeholder, drawn as the editor draws it.
                        // A new title takes the old one's place as the alert closes (`Replaced`), not a request later.
                        let title = pendingTitle ?? note.title
                        VStack(alignment: .leading, spacing: 0) {
                            Replaced(id: title) {
                                Text(title.isEmpty ? "Untitled note" : title)
                                    .displayFont(33).tracking(Theme.noteTitleTracking).lineSpacing(Theme.noteTitleLineSpacing).foregroundStyle(title.isEmpty ? Theme.textSecondary : Theme.textPrimary)
                                    .dynamicTypeSize(...DynamicTypeSize.accessibility1)
                                    .frame(maxWidth: .infinity, alignment: .leading)
                                    .accessibilityAddTraits(.isHeader)
                            }
                            .padding(.top, 18)
                            NoteMetaChips(date: MeetingPresentation.metadataDate(createdAt: note.createdAt, fallbackDate: note.date, meeting: meetingContext?.meeting) ?? .distantPast,
                                attendeeCount: meetingContext?.meeting.map(NoteMetaChips.others) ?? 0,
                                onMetadata: { showingMeeting = true })
                                .padding(.top, 14).padding(.bottom, 17)
                        }
                        // The page's own colour behind the head of the note: when a pull to refresh lets go, the
                        // head comes back up over the system's spinner and covers it. Without it the spinner
                        // was read through the title for as long as it took to fade.
                        .background { Theme.bgTop.padding(.horizontal, -Theme.gutter * 3).padding(.top, -6) }
                        // Cut to its room as the room opens (`unfold`): the words are never drawn across the notes moving down.
                        if let failure { errorView(failure).transition(motion.unfold) }
                        // After End, as in the reference: the step the recording is at, in a row under the chips.
                        // Notes asked for with the Generate button are being written just the same, and say so here.
                        if !showMine, let step = NoteProgressRow.text(finishing: finishingHere, generating: generatingHere || generating) {
                            VStack(alignment: .leading, spacing: 12) {
                                if offersNotify {
                                    NotifyWhenReadyCard(onNotify: { offersNotify = false; Task { await NotesReadyNotifications.request() } },
                                                        onDismiss: { offersNotify = false; NotesReadyNotifications.dismiss() })
                                        .transition(motion.unfold)
                                }
                                NoteProgressRow(text: step)
                            }
                            .padding(.bottom, 16)
                            .padding(.top, -1)
                            .transition(motion.unfold)
                            .task { offersNotify = await NotesReadyNotifications.shouldOffer() }
                        }
                        let content = showMine ? note.body : note.enhanced
                        // One document at a time: the one on screen is gone before the other arrives, and neither
                        // moves while the other can be read (`Replaced`). Large text never animates its layout.
                        Replaced(id: DocumentIdentity(showMine: showMine, revision: note.generatedRevision)) {
                            if content.isEmpty && !showMine && (finishingHere || generatingHere || generating) {
                                // The row above says it.
                                Color.clear.frame(height: 0)
                            } else if content.isEmpty {
                                Text(emptyText(note))
                                    .foregroundStyle(Theme.textTertiary).readingFont(.body)
                                    .frame(maxWidth: .infinity, alignment: .leading)
                            } else if !showMine {
                                ServerNoteMarkdown(text: content)
                            } else {
                                Text(content).readingFont(.body).lineSpacing(6)
                                    .foregroundStyle(Theme.textSecondary).textSelection(.enabled)
                                    .frame(maxWidth: .infinity, alignment: .leading)
                            }
                        }
                        if recording.isActive(ref), capturingHere || finishingHere || recording.error != nil {
                            // While recording, the live stream's line, when it has something to say; after Stop, while
                            // the stream hands over the rest, that it is finishing; then any problem.
                            VStack(alignment: .leading, spacing: 8) {
                                // After Stop the row under the title says it ("Uploading transcript..."), unless "My notes" is shown.
                                if capturingHere || finishingHere && showMine, let live = recording.transcript(for: ref) {
                                    LiveStatusLine(live: live, finishing: finishingHere)
                                } else if finishingHere && showMine {
                                    Text(LiveStatusText.finishing).font(.footnote)
                                }
                                if let message = recording.error {
                                    Text(message).font(.footnote).foregroundStyle(Theme.danger)
                                }
                            }.foregroundStyle(Theme.textTertiary).padding(.top, 16)
                                .transition(motion.rise)
                        }
                    } else {
                        // The note is on its way, or could not be fetched. Nothing says it failed before it has (`Loading`).
                        Loading(while: failure == nil) { NotePlaceholder() } then: {
                            ScreenPrompt(symbol: "doc.text", title: "This note could not be opened", detail: failure?.message,
                                         actions: [ScreenPrompt.Action(title: Wording.retry) { failure = nil; Task { await load() } }], fills: false)
                                .padding(.top, 96)
                        }
                    }
                }
                .padding(.leading, Theme.gutter).padding(.trailing, 8).clearsBottomChrome()
                .animates(failure)
                .animates(note == nil)
                .animates(recording.error)
                .animates(generatingHere || generating)
                .animates(finishingHere)
                .animates(offersNotify)
                .animates(pendingTitle)
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
            // As in the reference: a popover of Share via, Copy link and QR code; "Manage links…" opens the full sheet.
            GlassCircleButton(systemImage: "square.and.arrow.up") { sharePopover = true }
                .popover(isPresented: $sharePopover) {
                    NoteSharePopover(noteID: noteID) { managesLinks = true; sharePopover = false }
                        // The sheet rises the moment the popover has gone: no guessed wait in between.
                        .onDisappear { if managesLinks { managesLinks = false; sharing = true } }
                }
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
                Button("Copy notes", systemImage: "doc.on.doc") {
                    guard let note else { return }
                    UIPasteboard.general.string = "\(note.title)\n\n\(showMine ? note.body : note.enhanced)"
                    copied.mark()
                }
                // Which of the two documents is on the page: the one shown carries the check. Two short
                // names, where "Show enhanced notes" ran onto a second line.
                Picker("Notes shown", selection: $showMine) {
                    Label("Enhanced notes", systemImage: "sparkles").tag(false)
                    Label("My notes", systemImage: "text.viewfinder").tag(true)
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
                // A copy made from the menu is confirmed here, where the menu closes to (`Copied`).
                RailGlyph(systemImage: copied.symbol("ellipsis")).contentTransition(motion.symbolReplace)
            }.menuOrder(.fixed).tint(Theme.textSecondary).glass(Circle()).disabled(note == nil || busy)
                .confirmsCopy($copied)
                .accessibilityLabel("Note actions").accessibilityValue(copied.isOn ? "Copied" : "")
        } bottom: {
            // One glass group: Generate and the recording control split off
            // from the Ask pill and merge back into it.
            GlassChromeGroup(spacing: 12) {
                VStack(spacing: 12) {
                    // The button stays through the request and shows its own
                    // busy state, so one glass surface lasts the whole operation.
                    // After a Stop the notes are being written already, and it shows that.
                    // After End the row under the title says the notes are being written (as in the reference).
                    if canGenerate || generating {
                        GenerateButton(title: generatedBefore ? "Regenerate notes" : "Generate notes", busy: generating || generatingHere) { generate() }
                            .glassMorph("generate", in: glass)
                    }
                    HStack(alignment: .center, spacing: 12) {
                        if showsScheduledStart, let meeting = meetingContext?.meeting, MeetingPresentation.startLabel(meeting) != nil {
                            // As in the reference, an upcoming meeting's page offers one dark
                            // "Start meeting" capsule in place of the Ask pill.
                            StartMeetingButton { openCapture() }
                                .accessibilityHint("Starts recording this meeting now")
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
                        if !(chatPanel && sizeClass == .regular), !showsScheduledStart {
                            AskAnythingPill { openChat() }.accessibilityIdentifier("server-chat-open")
                                .glassMorph("ask", in: glass)
                        }
                    }
                }
            }
            .padding(.horizontal, 4)
            // Out of the way of the "Edit title" alert: the gold button otherwise glows
            // through the keyboard's glass underneath it.
            .opacity(editingTitle ? 0 : 1)
            .animates(editingTitle)
            .animation(motion.animation(.gooey, entering: canGenerate || generating), value: canGenerate || generating)
            .haptic(.error, trigger: failures)
            .haptic(.destructive, trigger: deletions)
            .animation(motion.animation(.gooey, entering: indicatorHere), value: indicatorHere)
            .animation(motion.animation(.gooey, entering: showsScheduledStart), value: showsScheduledStart)
        }
        .navigationBarBackButtonHidden()
        .capturesDismiss(into: dismiss)
        // This page has no field of its own. The keyboard here belongs to the
        // "Edit title" alert, and the bar must not ride up underneath that alert.
        .ignoresSafeArea(.keyboard, edges: .bottom)
        // Outside the line above: the chat panel's composer rides on the keyboard.
        .noteChatPanel(noteID: noteID, isPresented: $chatPanel) { if let account { router.path.append(.serverChat(account, noteID: noteID)) } }
        #if DEBUG
        // QA only: `-qa.chatPanel YES` opens the chat panel beside the note on a wide layout.
        .task { if UserDefaults.standard.bool(forKey: "qa.chatPanel") { try? await Task.sleep(for: .seconds(1)); openChat() } }
        #endif
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
                deletions += 1
                Task {
                    do {
                        let account = server.accountIdentity
                        try await server.delete(id: noteID)
                        ServerChatSessions.shared.noteDeleted(noteID, accountIdentity: account)
                        try recording.forgetDeletedNote(ref)
                        dismiss()
                    }
                    catch is CancellationError { }
                    catch { failure = NoteFailure(.delete, error); failures += 1 }
                    busy = false
                }
            }
        } message: { Text("This deletes the note and revokes its shared links on your server.") }
        .alert("Edit title", isPresented: $editingTitle) {
            TextField("Title", text: $titleDraft)
            Button("Cancel", role: .cancel) {}
            Button("Save") { saveTitle(titleDraft) }.disabled(titleDraft.count > 500)
        }
        // "Show my notes" / "Show enhanced notes": one document for the other.
        .onChange(of: showMine) { Haptics.shared.play(.selection) }
    }

    private var capturingHere: Bool { recording.isCapturing && recording.isActive(ref) }
    /// The recording here was stopped and its notes are being written from the transcript.
    private var generatingHere: Bool { recording.isGeneratingNotes(ref) }
    /// The recording here was stopped and its audio is still being handed to the server.
    private var finishingHere: Bool { recording.isFinishingTranscript(ref) }
    /// What the page says while it has no document to show.
    private func emptyText(_ note: NoteDTO) -> String {
        NoteEmptyText.text(showMine: showMine, finishing: finishingHere, generating: generatingHere || generating,
                           recording: capturingHere || note.recordingState == .recording,
                           endedWithoutTranscript: ServerRecordingController.endedWithoutTranscript(note),
                           hasTranscript: note.transcriptRevision > 0)
    }
    /// This note has a recording to return to: running, paused, or interrupted.
    private var indicatorHere: Bool { recording.isIndicator(ref) }
    private func openCapture() { if let ref { router.serverCapture = ServerCaptureTarget(ref) } }
    private var showsScheduledStart: Bool {
        guard !recording.isCapturing, note?.recordingState == .idle, let meeting = meetingContext?.meeting else { return false }
        return MeetingPresentation.startLabel(meeting) != nil
    }

    /// The list's row for this note. It says whether there is a transcript and whether notes were
    /// written from it, so the Generate button is in the bar as the page slides in instead of
    /// budding off the Ask pill once the whole note has been fetched.
    private var row: ServerNoteRow? { server.list.rows.first { $0.id == noteID } }

    private var generatedBefore: Bool { (note.map(\.generatedRevision) ?? row?.generatedRevision) != nil }

    private var canGenerate: Bool {
        guard !busy, !recording.isBusy, !recording.isCapturing, !generatingHere else { return false }
        if let note {
            return note.recordingState != .recording && NoteGenerateRule.offers(transcriptRevision: note.transcriptRevision, generatedRevision: note.generatedRevision)
        }
        // Not fetched yet: the row decides. A note being recorded is `recording.isCapturing` above
        // (on this iPhone) or loses the button when the note arrives (elsewhere).
        guard let row, failure == nil else { return false }
        return NoteGenerateRule.offers(transcriptRevision: row.transcriptRevision, generatedRevision: row.generatedRevision)
    }

    private func generate() {
        guard canGenerate, let revision = note?.transcriptRevision ?? row?.transcriptRevision else { return }
        busy = true; generating = true; failure = nil
        Task {
            defer { busy = false; generating = false }
            do {
                loaded = try await server.generate(noteID: noteID, expectedTranscriptRevision: revision)
                // The notes have arrived: the signature that also follows a recording's End.
                Haptics.shared.play(.notesReady)
            }
            catch is CancellationError { }
            catch { failure = NoteFailure(.generate, error); failures += 1 }
        }
    }

    /// The new title is on the page from the tap; it goes back if the server refuses it.
    private func saveTitle(_ title: String) {
        guard note != nil else { return }
        loadRevision &+= 1
        busy = true; failure = nil
        pendingTitle = title
        Task {
            defer { busy = false; pendingTitle = nil }
            do { loaded = try await server.update(id: noteID, NoteEditorContent.titleUpdate(title)) }
            catch is CancellationError { }
            catch { failure = NoteFailure(.title(title), error); failures += 1 }
        }
    }

    /// "Try again" does again what failed, not something else.
    private func retry(_ step: NoteFailure.Step) {
        switch step {
        case .load: failure = nil; Task { await load() }
        case .generate: generate()
        case .title(let title): saveTitle(title)
        case .delete: failure = nil; confirmDelete = true
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
            // A refresh clears a failure to load; a failed Generate keeps its message until it is tried again.
            if failure?.step == .load { failure = nil }
            if let found { meetingContext = found }
        }
        catch is CancellationError { }
        catch { if revision == loadRevision { failure = NoteFailure(.load, error) } }
    }

    /// What failed, in a sentence, and the one thing to do about it.
    private func errorView(_ failure: NoteFailure) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(failure.message).uiFont(15).foregroundStyle(Theme.textSecondary)
                .fixedSize(horizontal: false, vertical: true)
            Button(failure.step.retryTitle) { retry(failure.step) }
                .uiFont(15, weight: .medium).foregroundStyle(Theme.accentText)
                .frame(minWidth: 44, minHeight: 44, alignment: .leading).contentShape(Rectangle())
                .buttonStyle(.squash)
                .disabled(busy)
                .accessibilityIdentifier("note-retry")
        }
        .padding(.bottom, 8)
        .accessibilityIdentifier("note-error")
    }
}

/// Something the person asked of a note failed. It keeps what that was, so
/// "Try again" tries that again: after a failed Generate it generates, where it
/// used to reload the note and leave the person to find the button.
struct NoteFailure: Equatable {
    enum Step: Equatable {
        case load, generate, title(String), delete

        /// The words on the action under the message.
        var retryTitle: String { Wording.retry }
    }

    let step: Step
    let message: String

    init(_ step: Step, _ error: Error) { self.step = step; message = error.localizedDescription }
    init(_ step: Step, message: String) { self.step = step; self.message = message }
}

/// When a note offers to write its notes: it has a transcript, and its notes
/// were not written from this version of it.
enum NoteGenerateRule {
    static func offers(transcriptRevision: Int, generatedRevision: Int?) -> Bool {
        transcriptRevision > 0 && generatedRevision != transcriptRevision
    }
}

/// What a note's page says where its document would be, when it has none.
enum NoteEmptyText {
    static let typed = "No typed notes yet. Choose Edit notes to add your own."
    static let generating = "Writing your notes from the transcript…"
    static let recording = "Your notes are written from the transcript when the recording stops."
    /// Recorded, transcribed, not written up: the Generate button is under it.
    static let ready = "This meeting’s transcript is ready. Generate notes to write them up."
    static let fresh = "Record this meeting. When you stop, your notes are written from the transcript."

    @MainActor static func text(showMine: Bool, finishing: Bool, generating: Bool, recording: Bool,
                                endedWithoutTranscript: Bool, hasTranscript: Bool) -> String {
        if showMine { return typed }
        if finishing { return LiveStatusText.finishing }
        if generating { return Self.generating }
        if recording { return Self.recording }
        if endedWithoutTranscript { return ServerRecordingController.noTranscript }
        return hasTranscript ? ready : fresh
    }
}

/// The room of a note that is on its way: a title, its chip, a few lines.
private struct NotePlaceholder: View {
    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            bar(width: 230, height: 30).padding(.top, 24)
            bar(width: 150, height: 30, radius: 15).padding(.top, 20)
            ForEach(0..<5, id: \.self) { index in
                bar(width: [300, 260, 320, 240, 180][index], height: 13).padding(.top, index == 0 ? 34 : 16)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .accessibilityElement().accessibilityLabel("Loading note")
    }

    private func bar(width: CGFloat, height: CGFloat, radius: CGFloat = 6) -> some View {
        RoundedRectangle(cornerRadius: radius, style: .continuous).fill(Theme.quietFill).frame(width: width, height: height)
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
            .uiFont(14, weight: .medium).frame(height: 48).padding(.horizontal, 15)
            .foregroundStyle(Theme.onAccent).glassSurface(Capsule(), interactive: true, tint: Theme.accent)
            .dynamicTypeSize(...DynamicTypeSize.xxxLarge)
        }
        .buttonStyle(.squash)
        .accessibilityElement(children: .combine)
        .accessibilityLabel("Stop recording")
    }
}

extension NoteMetaChips {
    /// The people in a meeting other than the account holder: what the chip counts after "Me".
    static func others(_ meeting: MeetingContextDTO) -> Int {
        MeetingPresentation.people(meeting).filter { !$0.isSelf }.count
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
            ForEach(Array(blocks.enumerated()), id: \.offset) { index, block in
                switch block {
                case .heading(let value):
                    Text(inline(value)).readingFont(.heading).accessibilityAddTraits(.isHeader)
                        .foregroundStyle(Theme.textPrimary).padding(.top, index == 0 ? 0 : 16)
                case .list(let marker, let value, let depth):
                    HStack(alignment: .firstTextBaseline, spacing: Self.markerGap(marker)) {
                        // A number keeps its own width, so the text of a wrapped item hangs clear of it.
                        // A sub-bullet is a hollow circle, as on the desktop and web.
                        Self.markerView(marker, depth: depth).foregroundStyle(Self.isBullet(marker) ? Theme.textSecondary : Theme.textTertiary)
                        Text(inline(value)).frame(maxWidth: .infinity, alignment: .leading)
                    }.padding(.leading, Self.indent(marker, depth: depth))
                // An indented plain line continues the item above it (a next step's deadline and detail): it
                // starts where that item's text does, drawn 4 pt up under it, as on the desktop and web. The
                // item's marker holds the same room, unseen, so a number's width is matched too.
                case .continuation(let marker, let value, let depth):
                    HStack(alignment: .firstTextBaseline, spacing: Self.markerGap(marker)) {
                        Self.markerView(marker, depth: depth).hidden().accessibilityHidden(true)
                        Text(inline(value)).frame(maxWidth: .infinity, alignment: .leading)
                    }.padding(.leading, Self.indent(marker, depth: depth)).padding(.top, -4)
                // A Markdown rule (the one above the chat link at the foot of generated notes) is a hairline, as on the desktop and web.
                case .paragraph(let value) where ["---", "***", "___"].contains(value.trimmingCharacters(in: .whitespaces)):
                    Rectangle().fill(Theme.textTertiary.opacity(0.35)).frame(height: 1).padding(.vertical, 10).accessibilityHidden(true)
                // Indented text that follows no list item is a plain paragraph, not a block of spaces.
                case .paragraph(let value): Text(inline(value.trimmingCharacters(in: .whitespaces))).frame(maxWidth: .infinity, alignment: .leading)
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

    /// A list marker as the note draws it: a sub-bullet is a hollow circle.
    static func marker(_ marker: String, depth: Int) -> String { marker == "•" && depth % 2 == 1 ? "◦" : marker }

    /// Bullets as in the reference: a solid round dot, a hollow ring one level in, centred
    /// in a 14 pt column 3 pt inside the text column; the item's words start 24 pt in,
    /// and each level is 24 pt deeper. A number keeps its own width.
    static func isBullet(_ marker: String) -> Bool { marker == "•" }
    @ViewBuilder static func markerView(_ marker: String, depth: Int) -> some View {
        if isBullet(marker) {
            Text(depth % 2 == 1 ? "○" : "●").font(.system(size: depth % 2 == 1 ? 8.5 : 7.5)).baselineOffset(2.5)
                .frame(width: 14).accessibilityHidden(true)
        } else {
            Text(marker).monospacedDigit()
        }
    }
    static func markerGap(_ marker: String) -> CGFloat { isBullet(marker) ? 7 : 10 }
    static func indent(_ marker: String, depth: Int) -> CGFloat { CGFloat(min(4, depth)) * 24 + (isBullet(marker) ? 3 : 0) }

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
    @State private var saves = 0
    @Environment(\.motion) private var motion

    init(title: String = "", body: String = "", contentTitle: String = "My notes", save: @escaping @MainActor (String, String) async throws -> Void) {
        _title = State(initialValue: title)
        _text = State(initialValue: body)
        self.save = save
        self.contentTitle = contentTitle
    }

    /// Enhanced notes are kept as Markdown, and that is what is edited here.
    private var isMarkdown: Bool { contentTitle == NoteEditorContent.enhanced.title }
    private var tooLong: Bool { title.count > 500 || text.count > 1_000_000 }

    var body: some View {
        NavigationStack {
            // The Settings cards on the sheet's own colour, as every other form in a sheet. The text takes
            // the height that is left, so a long note scrolls in its card and the title stays above it.
            VStack(alignment: .leading, spacing: 0) {
                SettingsSection("Title") {
                    CardField(divider: false) {
                        TextField("Title", text: $title, prompt: Text("Untitled note").foregroundStyle(Theme.textTertiary))
                            .submitLabel(.done).accessibilityIdentifier("server-note-editor-title")
                    }
                }
                Text(contentTitle).uiFont(SettingsMetrics.header).foregroundStyle(Theme.textSecondary)
                    .padding(.leading, 12).padding(.top, 22).padding(.bottom, 9)
                    .accessibilityAddTraits(.isHeader)
                // Markdown source is set in the code face: plainly the text behind the page, not a page that failed to draw.
                TextEditor(text: $text).readingFont(isMarkdown ? .code : .body).lineSpacing(4)
                    .foregroundStyle(Theme.textPrimary).tint(Theme.accent)
                    .scrollContentBackground(.hidden)
                    .padding(.horizontal, 12).padding(.vertical, 8)
                    .frame(maxHeight: .infinity)
                    .background(Theme.settingsCard, in: RoundedRectangle(cornerRadius: SettingsMetrics.cardRadius, style: .continuous))
                    .overlay(RoundedRectangle(cornerRadius: SettingsMetrics.cardRadius, style: .continuous).strokeBorder(Theme.stroke, lineWidth: 1).allowsHitTesting(false))
                    .accessibilityLabel("Note text")
                    .accessibilityIdentifier("server-note-editor-text")
                    .accessibilityAction(named: Text("Save notes")) { saveChanges() }
                    .accessibilityAction(named: Text("Cancel editing")) { cancelEditing() }
                if isMarkdown { CardNote("Written in Markdown: # starts a heading, - a list item, **bold**.") }
                if let error { CardNote(error, color: Theme.danger).transition(motion.rise) }
                if tooLong {
                    CardNote("Use a title of 500 characters or fewer and notes of 1,000,000 characters or fewer.", color: Theme.danger)
                        .transition(motion.rise)
                }
            }
            .padding(.horizontal, 16).padding(.bottom, 12)
            .background(Theme.sheet.ignoresSafeArea())
            .animates(error)
            .animates(tooLong)
            .allowsHitTesting(!saving)
            .dynamicTypeSize(...DynamicTypeSize.accessibility1)
            .navigationTitle("Edit notes").navigationBarTitleDisplayMode(.inline)
            .sheetBar()
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel", action: cancelEditing).disabled(saving)
                        .accessibilityLabel("Cancel editing")
                        .accessibilityIdentifier("server-note-editor-cancel")
                        .keyboardShortcut(.escape, modifiers: [])
                }
                ToolbarItem(placement: .confirmationAction) {
                    // Save keeps its place and its width while it works, with the mark where the word was.
                    Button(action: saveChanges) {
                        ZStack {
                            Text("Save").fontWeight(.semibold).opacity(saving ? 0 : 1)
                            if saving { CerealLoader(size: 18, color: Theme.accentText).transition(motion.pop) }
                        }
                    }
                    .animates(saving)
                    .disabled(!NoteEditorDraft.canSave(title: title, text: text, saving: saving))
                    .accessibilityLabel("Save notes")
                    .accessibilityIdentifier("server-note-editor-save")
                    .keyboardShortcut(.return, modifiers: .command)
                }
            }
            .interactiveDismissDisabled(saving)
            // Saved, or saving failed: the two outcomes of this sheet.
            .haptic(.error, trigger: error) { old, new in new != nil && new != old }
            .haptic(.done, trigger: saves)
        }.glassSheet(background: Theme.sheet)
    }

    private func cancelEditing() {
        guard !saving else { return }
        dismiss()
    }

    private func saveChanges() {
        guard NoteEditorDraft.canSave(title: title, text: text, saving: saving) else { return }
        saving = true; error = nil
        Task {
            do { try await save(title, text); saves += 1; dismiss() }
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
                    ScreenPrompt(symbol: "text.magnifyingglass", title: "The transcript has changed", detail: "This source points at an earlier version of it.", bottomRoom: 40)
                } else {
                    ScrollViewReader { proxy in
                        ScrollView {
                            LazyVStack(alignment: .leading, spacing: 20) {
                                if note.transcriptSegments.isEmpty { Text("No transcript is saved for this note.").foregroundStyle(Theme.textSecondary) }
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
            .background(Theme.sheet.ignoresSafeArea())
            .navigationTitle(live != nil && recording ? "Transcript" : "Saved transcript").navigationBarTitleDisplayMode(.inline)
            // Lines are gone before they reach the title.
            .sheetBar()
            .toolbar { ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } } }
        }
        // One colour whether or not a recording is running behind it.
        .glassSheet(background: Theme.sheet)
    }

    private func savedRow(_ row: TranscriptPresentation.Row) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(row.caption).uiFont(12, relativeTo: .caption).foregroundStyle(Theme.textSecondary)
            Text(row.segment.text).readingFont(.body).foregroundStyle(Theme.textPrimary).opacity(row.segment.isFinal ? 1 : 0.6)
        }
        .padding(focusSegmentID == row.segment.id ? 10 : 0)
        // The cited line is brighter as the sheet lands on it, then settles.
        .background(focusSegmentID == row.segment.id ? Theme.accent.opacity(landed ? 0.12 : 0.30) : .clear, in: RoundedRectangle(cornerRadius: 8))
        .accessibilityElement(children: .combine).id(row.segment.id)
        .accessibilityIdentifier("transcript-segment-\(row.segment.id)")
    }
}
