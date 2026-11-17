import SwiftUI
import Contracts
import APIClient

/// Full-screen native capture chrome backed by the app-wide real recorder.
///
/// The recording belongs to `ServerRecordingController`, not to this screen.
/// Opening the screen for a note that is not being recorded starts it; after
/// that the screen can be closed, swiped away or left for the chat and the
/// recording carries on. It ends only when the user chooses Stop.
/// The drags that leave the recording screen.
enum CaptureLeaveGesture {
    /// A downward drag has to start on the title or the line under it.
    static let titleZone: CGFloat = 110
    static let distance: CGFloat = 50
    static let downDistance: CGFloat = 80

    static func leaves(start: CGPoint, translation: CGSize, width: CGFloat, rightToLeft: Bool, keyboardUp: Bool) -> Bool {
        let dx = translation.width * (rightToLeft ? -1 : 1)
        if EdgeGestureRules.startsAtLeadingEdge(x: start.x, width: width, rightToLeft: rightToLeft),
           dx > distance, EdgeGestureRules.isInwardHorizontal(dx: dx, dy: translation.height) { return true }
        // With the keyboard up, a downward drag puts the keyboard away instead.
        return !keyboardUp && start.y <= titleZone && translation.height > downDistance && translation.height > abs(translation.width) * 2
    }
}

/// What an error says on screen. A cancellation is not a failure the person
/// can act on (the screen went away, or the request was superseded), so it says nothing.
enum ErrorText {
    static func isCancellation(_ error: Error) -> Bool {
        if error is CancellationError { return true }
        if let failure = error as? APIClientError, failure == .transport(.cancelled) { return true }
        let underlying = error as NSError
        return underlying.domain == NSURLErrorDomain && underlying.code == NSURLErrorCancelled
    }
    static func shown(_ error: Error) -> String? { isCancellation(error) ? nil : error.localizedDescription }
}

struct ServerCaptureView: View {
    let noteID: String
    /// The note was made for this recording: closed while it still holds nothing, it is removed.
    var discardsIfEmpty = false
    @Environment(ServerNotesModel.self) private var server
    @Environment(ServerRecordingController.self) private var recording
    @Environment(\.account) private var account
    private var ref: NoteRef? { account.map { NoteRef(account: $0, id: noteID) } }
    @Environment(Router.self) private var router
    @Environment(\.dismiss) private var dismiss
    @Environment(\.motion) private var motion
    @Environment(\.railMetrics) private var rail
    @Namespace private var glass
    @State private var title = ""
    /// The note's title as the server last gave it. While the field still holds
    /// this, the person has not typed a title, and none is sent.
    @State private var serverTitle = ""
    @State private var text = ""
    @State private var initial: NoteDTO?
    @State private var loaded = false
    @State private var saving = false
    @State private var generating = false
    @State private var error: String?
    @State private var saveTask: Task<Void, Never>?
    @State private var unsupportedAttachment = false
    @State private var meetingContext: NoteContextDTO?
    @State private var showingMeeting = false
    @State private var showingTranscript = false
    /// Controls animate only for changes after the surface has arrived.
    @State private var settled = false
    /// Typed text that has not reached the server yet.
    @State private var unsaved = false
    @State private var width: CGFloat = 0
    /// The bar has replaced the "Let people know" hint (after the first second of a new recording).
    @State private var barRevealed = false
    /// The screen opened on a recording already running: the bar is there from the start.
    @State private var revealedAtOpen = false
    /// End or Cancel was chosen: the screen is on its way out.
    @State private var leaving = false
    /// The recording began on this screen, in a note made for it, with nothing recorded before.
    @State private var freshRecording = false
    /// The start this screen asked for has finished, one way or the other.
    @State private var startSettled = false
    @Environment(\.layoutDirection) private var layoutDirection
    @FocusState private var focus: CaptureEditorField?
    private var note: NoteDTO? { server.state.notes.first { $0.id == noteID } ?? initial }
    private var ownsCapture: Bool { recording.isActive(ref) }
    private var active: Bool { ownsCapture && recording.isCapturing }

    var body: some View {
        let _ = PerfProbe.hit("ServerCaptureView.body")
        ZStack(alignment: .bottom) {
            Theme.background
            VStack(alignment: .leading, spacing: 0) {
                CaptureNoteEditor(title: $title, text: $text, focused: $focus,
                    date: date, attendeeCount: meetingContext?.meeting.map { MeetingPresentation.people($0).count } ?? 0, identifierPrefix: "server-capture", onMetadata: { focus = nil; showingMeeting = true })
            }.padding(.horizontal, rail.captureGutter)
                .frame(maxHeight: .infinity, alignment: .top)
                // The controls are an inset of the editor, so text never runs under them.
                .safeAreaInset(edge: .bottom, spacing: 0) {
            VStack(spacing: 18) {
                if let message = error ?? (ownsCapture ? recording.error : nil) {
                    Text(message).font(.system(size: 13)).foregroundStyle(Theme.danger).multilineTextAlignment(.center)
                        .transition(motion.fade)
                } else if active, recording.recorder.state != .paused, let live = recording.transcript(for: ref) {
                    // While recording, the only status is the live stream's, and only when
                    // it has something to say. The line follows the stream by itself.
                    LiveStatusLine(live: live, font: .system(size: 13), color: Theme.textSecondary)
                } else if finishing, let live = recording.transcript(for: ref) {
                    // After Stop, while the stream hands over what it has not sent yet.
                    LiveStatusLine(live: live, font: .system(size: 13), color: Theme.textSecondary, finishing: true)
                } else if let after = afterStopLine {
                    // After Stop: the notes being written, or why there are none.
                    VStack(spacing: 4) {
                        Text(after).font(.system(size: 13)).foregroundStyle(Theme.textSecondary).multilineTextAlignment(.center)
                        if let missing {
                            Text(missing).font(.system(size: 13)).foregroundStyle(Theme.danger).multilineTextAlignment(.center)
                        }
                    }.transition(motion.fade)
                } else if let missing {
                    // What this recording could not transcribe stays said.
                    Text(missing).font(.system(size: 13)).foregroundStyle(Theme.danger).multilineTextAlignment(.center)
                        .transition(motion.fade)
                }
                // As in the reference: while a new recording gets going, the hint stands where
                // the bar will be; the bar takes its place once the clock has run a second.
                // With the keyboard up the bar is a small chip beside the keyboard's control.
                // As in the reference (8.0 s, 30 fps): the gold capsule is there at once behind the
                // hint; the hint fades out and the controls fade in over about 0.2 s.
                ZStack {
                    if !showsHint {
                        RecordingBar(recorder: recording.recorder, compact: focus != nil,
                                     cancelWindow: offersCancel ? Self.cancelWindow : 0,
                                     fadesIn: !revealedAtOpen,
                                     onPauseToggle: pauseToggle, onCancel: cancel, onEnd: end,
                                     onHideKeyboard: { focus = nil })
                            .disabled(recording.isBusy || generating || leaving)
                            .transition(.identity)
                    }
                    if showsHint {
                        RecordingConsentHint().transition(.opacity.animation(motion.reduceMotion ? nil : .easeOut(duration: 0.2))).zIndex(1)
                    }
                }
            }.padding(.horizontal, rail.captureBarLeading)
                // The bar ends 30 pt above the screen's edge, as in the reference, over the home
                // indicator's inset; the chip rides 12 pt above the keyboard.
                .padding(.bottom, focus == nil && rail.bottomInset < RailMetrics.keyboardInsetThreshold ? RecordingBar.Metrics.screenEdge - rail.bottomInset : 12)
                .animation(settled ? motion.animation(.gooey, entering: focus != nil) : nil, value: focus)
                .animation(settled ? motion.gooey : nil, value: active)
                .animation(settled ? motion.snappy : nil, value: statusLine)
                }
        }
        // Two drags leave the screen, as they would a pushed screen or a sheet: in from the
        // leading edge, and down from the title. A full-screen cover has neither by itself.
        // They are observed alongside the editor's own gestures, so taps, scrolling and text
        // selection are unaffected, and neither stops the recording.
        .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { width = $0 }
        .simultaneousGesture(DragGesture(minimumDistance: 16).onEnded { value in
            if CaptureLeaveGesture.leaves(start: value.startLocation, translation: value.translation, width: width,
                                          rightToLeft: layoutDirection == .rightToLeft, keyboardUp: focus != nil) { close() }
        })
        .captureChrome(close: close) {
            // As in the reference: the attachment control and the menu, in one capsule.
            RailIcon(systemImage: "paperclip") { unsupportedAttachment = true }
            Menu {
                if recording.transcript(for: ref) != nil {
                    // One tap from the recording to what has been said so far.
                    Button("View transcript", systemImage: "text.alignleft") { focus = nil; showingTranscript = true }
                        .accessibilityIdentifier("server-capture-transcript")
                }
                Button("View notes", systemImage: "doc.text") { viewNotes() }
            } label: {
                RailGlyph(systemImage: "ellipsis")
            }
            .accessibilityLabel("Recording actions")
        }
        .task {
            try? await Task.sleep(for: .milliseconds(350))
            settled = true
            #if DEBUG
            // QA only: `-qa.transcriptAfter N` opens the transcript N seconds in; `-qa.hideKeyboard YES` puts the keyboard away.
            if UserDefaults.standard.bool(forKey: "qa.hideKeyboard") { try? await Task.sleep(for: .seconds(1.5)); focus = nil }
            let after = UserDefaults.standard.double(forKey: "qa.transcriptAfter")
            if after > 0 { try? await Task.sleep(for: .seconds(after)); focus = nil; showingTranscript = true }
            #endif
        }
        .task {
            guard !loaded else { return }
            do {
                // A read that was superseded (the account's connection was renewed under it) is asked again.
                var value: NoteDTO?
                for attempt in 0..<3 where value == nil {
                    do { value = try await server.note(id: noteID) }
                    catch where ErrorText.isCancellation(error) {
                        guard attempt < 2 else { throw error }
                        try await Task.sleep(for: .milliseconds(300))
                    }
                }
                guard let value else { return }
                initial = value; title = value.title; serverTitle = value.title; text = value.body; loaded = true
                // Returning to a recording in progress neither restarts it nor raises the keyboard.
                let returning = active
                if returning { barRevealed = true; revealedAtOpen = true }
                // Nothing recorded into this note before: the first seconds offer Cancel.
                freshRecording = !returning && discardsIfEmpty && value.transcriptRevision == 0
                if !returning { await recording.start(note: value, in: account) }
                startSettled = true
                // The reference opens on the page, not the keyboard: a tap on the text raises it.
                meetingContext = try? await server.noteContext(id: noteID)
            } catch { if let shown = ErrorText.shown(error) { self.error = shown } }
        }
        .sheet(isPresented: $showingMeeting) { MeetingContextSheet(noteID: noteID, initial: meetingContext) {
            meetingContext = $0
            let previous = title
            Task {
                guard let fresh = try? await server.note(id: noteID) else { return }
                if title == previous, ["", "new note", "untitled note"].contains(title.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()) { title = fresh.title; serverTitle = fresh.title }
                initial = fresh
            }
        } }
        .sheet(isPresented: $showingTranscript) {
            if let note { ServerTranscriptView(note: note, live: recording.transcript(for: ref), recording: active) }
        }
        .onChange(of: title) { queueSave() }.onChange(of: text) { queueSave() }
        // A title the server gave the note (its notes were generated) appears in
        // the field unless the person has typed one of their own, which stays.
        .onChange(of: note?.title) { _, new in
            guard loaded, let new, new != serverTitle else { return }
            if title == serverTitle { title = new }
            serverTitle = new
        }
        // Stopped from elsewhere (the Live Activity's End, the note's menu): the page
        // goes to the note, where its notes are being written, as End does.
        .onChange(of: writingNotes) { _, writing in if writing && !active { endToNote() } }
        .task(id: active) { await revealBar() }
        // Leaving this screen, by any route, does not stop the recording. What was
        // typed is sent on its way out.
        .onDisappear { saveTask?.cancel(); flushUnsaved() }
        .alert("Attachments", isPresented: $unsupportedAttachment) { Button("OK", role: .cancel) {} }
            message: { Text("File attachments are not supported by this instance yet.") }
    }

    /// Cancel is offered for this long at the start of a new recording, as in the reference.
    static let cancelWindow: TimeInterval = 5
    private var offersCancel: Bool { freshRecording && ownsCapture }
    private var showsHint: Bool { !barRevealed && focus == nil && error == nil && recording.error == nil }

    /// The hint gives way to the bar once the recording's clock has run a second
    /// (at once when the recording stopped or failed, so its controls are there).
    private func revealBar() async {
        while !barRevealed, !Task.isCancelled {
            let state = recording.recorder.state
            let running = state == .recording || state == .paused
            if (running && recording.recorder.elapsed >= 1) || (startSettled && !running) {
                withAnimation(motion.reduceMotion ? nil : .easeOut(duration: 0.2)) { barRevealed = true }
                return
            }
            try? await Task.sleep(for: .milliseconds(100))
        }
    }

    private func pauseToggle() {
        switch recording.recorder.state {
        case .recording: recording.recorder.pause()
        case .paused: recording.recorder.resumeFromPause()
        case .idle, .stopped, .failed: start()
        }
    }

    /// End: the recording stops and the screen goes to the note, where its
    /// transcript is handed over and its notes written.
    private func end() {
        guard active else { viewNotes(); return }
        stop()
        endToNote()
    }

    /// Cancel: the screen closes at once and the note made for this recording
    /// is removed. A note something was typed in is kept, as after End.
    private func cancel() {
        guard active, !leaving else { return }
        let typedTitle = title == serverTitle ? "" : title
        guard typedTitle.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
              text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { end(); return }
        leaving = true; focus = nil; saveTask?.cancel(); unsaved = false
        let recording = recording, ref = ref
        Task { await recording.cancel(ref) }
        router.serverCapture = nil
        dismiss()
    }

    /// Closes the screen over the note's own page.
    private func endToNote() {
        guard !leaving else { return }
        leaving = true; focus = nil
        if let ref {
            let target = CaptureNavigation.pathAfterClosing(from: router.path, note: ref, recording: false)
            if router.path != target { router.path = target }
        }
        router.serverCapture = nil
        dismiss()
    }

    /// After Stop, until they are written: the notes this recording's Stop is writing.
    private var writingNotes: Bool { ownsCapture && recording.isGeneratingNotes(ref) }
    /// After Stop, while the capture's audio is still being handed to the server.
    private var finishing: Bool { ownsCapture && !active && recording.isFinishingTranscript(ref) }
    /// The stretches of this recording that could not be transcribed, once it has stopped.
    private var missing: String? {
        guard ownsCapture, !active, let live = recording.transcript(for: ref) else { return nil }
        return TranscriptPresentation.untranscribedText(live.gaps)
    }
    /// The line under the editor once the recording has stopped.
    private var afterStopLine: String? {
        guard !active, ownsCapture, let note else { return nil }
        if finishing { return LiveStatusText.finishing }
        if writingNotes { return "Writing your notes from the transcript…" }
        return ServerRecordingController.endedWithoutTranscript(note) ? ServerRecordingController.noTranscript : nil
    }
    /// Drives the fade between the error, the line after Stop and nothing.
    private var statusLine: String {
        error ?? (ownsCapture ? recording.error : nil) ?? afterStopLine ?? ""
    }
    private var date: Date {
        note.flatMap { MeetingPresentation.metadataDate(createdAt: $0.createdAt, fallbackDate: $0.date, meeting: meetingContext?.meeting) } ?? .now
    }
    private func start() { if let note { Task { await recording.start(note: note, in: account) } } }
    /// Stops the recording. Its notes are written from the transcript straight
    /// away, after what was typed here has been saved.
    private func stop() {
        focus = nil
        guard active else { return }
        saveTask?.cancel()
        recording.stop(beforeNotes: { _ = try? await save() })
    }
    private func queueSave() {
        guard loaded else { return }
        unsaved = true
        saveTask?.cancel()
        saveTask = Task {
            do { try await Task.sleep(for: .milliseconds(400)); try await save() }
            catch { if let shown = ErrorText.shown(error) { self.error = shown } }
        }
    }
    private func save() async throws {
        guard title.count <= 500, text.count <= 1_000_000 else { throw APIClientError.invalidJSON }
        let sent = (title, text)
        let saved = try await server.update(id: noteID, Self.edit(title: title, serverTitle: serverTitle, text: text))
        if title == sent.0 { serverTitle = saved.title }
        if sent == (title, text) { unsaved = false }
        error = nil
    }
    /// For a screen that went away without its own save (a swipe).
    private func flushUnsaved() {
        guard loaded, unsaved, title.count <= 500, text.count <= 1_000_000 else { return }
        let server = server, noteID = noteID, edit = Self.edit(title: title, serverTitle: serverTitle, text: text)
        Task { _ = try? await server.update(id: noteID, edit) }
    }
    /// Saves what was typed, then shows `path` with this screen gone. The recording is untouched.
    private func leave(to path: @escaping @MainActor () -> [Route]) {
        guard !saving, !generating else { return }
        focus = nil
        guard loaded else { router.serverCapture = nil; dismiss(); return }
        saveTask?.cancel(); saving = true
        Task {
            defer { saving = false }
            do {
                try await save()
                let target = path()
                if router.path != target { router.path = target }
                router.serverCapture = nil
                dismiss()
            } catch { if let shown = ErrorText.shown(error) { self.error = shown } }
        }
    }
    /// What typing on this screen changes. The title is sent only when it is
    /// not the one the server gave: a field the person never touched must not
    /// write its old (often empty) title over one the server has since given the note.
    static func edit(title: String, serverTitle: String, text: String) -> UpdateNoteInput {
        UpdateNoteInput(body: text, date: nil, duration: nil, enhanced: nil, title: title == serverTitle ? nil : title, transcriptSegments: nil)
    }

    private func close() {
        guard discardsIfEmpty, !active, !saving, !generating else { closeKeepingNote(); return }
        focus = nil; saveTask?.cancel(); saving = true
        Task {
            // Only what the person typed counts as typed: a title the server gave is judged from the server's copy.
            let gone = await recording.discardIfNothingRecorded(ref, typedTitle: title == serverTitle ? "" : title, typedText: text)
            saving = false
            guard gone else { closeKeepingNote(); return }
            unsaved = false
            router.serverCapture = nil
            dismiss()
        }
    }
    private func closeKeepingNote() {
        leave { ref.map { CaptureNavigation.pathAfterClosing(from: router.path, note: $0, recording: active) } ?? router.path }
    }
    private func viewNotes() {
        leave { ref.map { CaptureNavigation.pathAfterClosing(from: router.path, note: $0, recording: false) } ?? router.path }
    }
}
