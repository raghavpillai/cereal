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

struct ServerCaptureView: View {
    let noteID: String
    @Environment(ServerNotesModel.self) private var server
    @Environment(ServerRecordingController.self) private var recording
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
    @Environment(\.layoutDirection) private var layoutDirection
    @FocusState private var focus: CaptureEditorField?
    private var note: NoteDTO? { server.state.notes.first { $0.id == noteID } ?? initial }
    private var ownsCapture: Bool { recording.activeNoteID == noteID }
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
                } else if active, let live = recording.transcript(for: noteID) {
                    // While recording, the only status is the live stream's, and only when
                    // it has something to say. The line follows the stream by itself.
                    LiveStatusLine(live: live, font: .system(size: 13), color: Theme.textSecondary)
                } else if !active && ownsCapture && recording.isSyncing {
                    Text("Transcribing...").font(.system(size: 13)).foregroundStyle(Theme.textSecondary)
                        .transition(motion.fade)
                }
                if focus == nil && active {
                    Text("Let people know you're using Cereal").font(.system(size: 16)).foregroundStyle(Theme.textSecondary)
                        .multilineTextAlignment(.center).transition(motion.giveWay)
                }
                GlassChromeGroup(spacing: 12) {
                    HStack(spacing: 12) {
                        RecordingPill(recorder: recording.recorder, onStop: stop, onStart: start,
                            onResumeFromPause: { recording.recorder.resumeFromPause() })
                            .disabled(recording.isBusy || generating)
                            .glassMorph("pill", in: glass)
                        if showsGenerate, let note {
                            GenerateButton(title: note.generatedRevision == note.transcriptRevision ? "View notes" : note.generatedRevision == nil ? "Generate notes" : "Regenerate notes", busy: generating) { generate() }
                                .disabled(recording.isSyncing)
                                .glassMorph("generate", in: glass)
                        }
                        if active && focus == nil {
                            // The note's own way into chat, while the meeting is still going.
                            // It gives its place to the keyboard control while typing.
                            AskAnythingPill { ask() }
                                .disabled(saving)
                                .accessibilityIdentifier("server-capture-ask")
                                .glassMorph("ask", in: glass)
                        } else {
                            Spacer(minLength: 0)
                        }
                        if focus != nil {
                            HideKeyboardButton { focus = nil }
                                .glassMorph("keyboard", in: glass)
                        }
                    }
                    // Without the keyboard the row is as tall as its tallest control (Ask and
                    // Generate, 64 pt) whether or not one is showing. The pill dropped 8 pt when
                    // a recording stopped and rose again when Generate arrived.
                    .frame(minHeight: focus == nil ? 64 : nil)
                }
            }.padding(.leading, rail.captureBarLeading).padding(.trailing, 20).padding(.bottom, 12)
                .animation(settled ? motion.animation(.gooey, entering: focus != nil) : nil, value: focus)
                .animation(settled ? motion.gooey : nil, value: active)
                .animation(settled ? motion.animation(.gooey, entering: showsGenerate) : nil, value: showsGenerate)
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
            if recording.transcript(for: noteID) != nil {
                // One tap from the recording to what has been said so far.
                RailIcon(systemImage: "text.alignleft") { focus = nil; showingTranscript = true }
                    .accessibilityLabel("View transcript")
                    .accessibilityIdentifier("server-capture-transcript")
            }
            RailIcon(systemImage: "paperclip") { unsupportedAttachment = true }
            Menu {
                if active { Button("Stop recording", systemImage: "stop.circle") { stop() } }
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
                let value = try await server.note(id: noteID)
                initial = value; title = value.title; serverTitle = value.title; text = value.body; loaded = true
                // Returning to a recording in progress neither restarts it nor raises the keyboard.
                let returning = active
                if !returning { await recording.start(note: value) }
                if text.isEmpty && !returning { focus = .body }
                meetingContext = try? await server.noteContext(id: noteID)
            } catch { self.error = error.localizedDescription }
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
            if let note { ServerTranscriptView(note: note, live: recording.transcript(for: noteID), recording: active) }
        }
        .onChange(of: title) { queueSave() }.onChange(of: text) { queueSave() }
        // A title the server gave the note (its notes were generated) appears in
        // the field unless the person has typed one of their own, which stays.
        .onChange(of: note?.title) { _, new in
            guard loaded, let new, new != serverTitle else { return }
            if title == serverTitle { title = new }
            serverTitle = new
        }
        // Leaving this screen, by any route, does not stop the recording. What was
        // typed is sent on its way out.
        .onDisappear { saveTask?.cancel(); flushUnsaved() }
        .alert("Attachments", isPresented: $unsupportedAttachment) { Button("OK", role: .cancel) {} }
            message: { Text("File attachments are not supported by this instance yet.") }
    }

    private var showsGenerate: Bool {
        guard !active, let note else { return false }
        return note.transcriptRevision > 0 && !recording.hasPendingAudio(noteID: noteID)
    }
    /// Drives the fade between the error, "Transcribing..." and nothing.
    private var statusLine: String {
        error ?? (ownsCapture ? recording.error : nil) ?? (!active && ownsCapture && recording.isSyncing ? "syncing" : "")
    }
    private var date: Date {
        note.flatMap { MeetingPresentation.metadataDate(createdAt: $0.createdAt, fallbackDate: $0.date, meeting: meetingContext?.meeting) } ?? .now
    }
    private func start() { if let note { Task { await recording.start(note: note) } } }
    private func stop() { focus = nil; if active { recording.stop() } }
    private func queueSave() {
        guard loaded else { return }
        unsaved = true
        saveTask?.cancel()
        saveTask = Task {
            do { try await Task.sleep(for: .milliseconds(400)); try await save() }
            catch is CancellationError { }
            catch { self.error = error.localizedDescription }
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
            } catch { self.error = error.localizedDescription }
        }
    }
    /// What typing on this screen changes. The title is sent only when it is
    /// not the one the server gave: a field the person never touched must not
    /// write its old (often empty) title over one the server has since given the note.
    static func edit(title: String, serverTitle: String, text: String) -> UpdateNoteInput {
        UpdateNoteInput(body: text, date: nil, duration: nil, enhanced: nil, title: title == serverTitle ? nil : title, transcriptSegments: nil)
    }

    private func close() {
        leave { CaptureNavigation.pathAfterClosing(from: router.path, noteID: noteID, recording: active) }
    }
    private func viewNotes() {
        leave { CaptureNavigation.pathAfterClosing(from: router.path, noteID: noteID, recording: false) }
    }
    private func ask() {
        leave { CaptureNavigation.pathForChat(from: router.path, noteID: noteID) }
    }
    private func generate() {
        guard !active, !generating, let note else { return }
        if note.generatedRevision == note.transcriptRevision { close(); return }
        generating = true; focus = nil; saveTask?.cancel()
        Task {
            defer { generating = false }
            do {
                try await save()
                _ = try await server.generate(noteID: noteID, expectedTranscriptRevision: note.transcriptRevision)
                router.path = CaptureNavigation.pathAfterClosing(from: router.path, noteID: noteID, recording: false)
                router.serverCapture = nil
                dismiss()
            } catch { self.error = error.localizedDescription }
        }
    }
}
