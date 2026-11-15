import SwiftUI

/// Full-screen capture surface for a note: new notes, and "Resume recording" on an existing note.
/// Lifecycle: Start → Stop (capture only) → Generate notes → Resume (same transcript) → Stop →
/// Regenerate (only when the transcript revision moved past the generated one).
struct NewNoteView: View {
    let noteID: UUID

    @Environment(NoteStore.self) private var store
    @Environment(Router.self) private var router
    @Environment(ConnectionMode.self) private var connectionMode
    @Environment(\.dismiss) private var dismiss
    @Environment(\.motion) private var motion
    @Environment(\.railMetrics) private var rail
    @Environment(\.verticalSizeClass) private var verticalSizeClass
    @Namespace private var glass
    @State private var recorder = Recorder()
    @State private var title = ""
    @State private var body_ = ""
    @State private var loaded = false
    @State private var generating = false
    @State private var generationTask: Task<Void, Never>?
    @State private var unsupportedAttachment = false
    /// Capture starts while the surface is still arriving. Controls animate
    /// only for changes after that, so nothing reflows mid-presentation.
    @State private var settled = false
    @FocusState private var focus: CaptureEditorField?


    private var note: Note? { store.note(noteID) }

    var body: some View {
        let _ = PerfProbe.hit("NewNoteView.body")
        ZStack(alignment: .bottom) {
            Theme.background

            VStack(alignment: .leading, spacing: 0) {
                CaptureNoteEditor(title: $title, text: $body_, focused: $focus,
                    date: note?.createdAt ?? .now, attendeeCount: note?.attendees.count ?? 0)
            }
            .padding(.horizontal, rail.captureGutter)
            .frame(maxHeight: .infinity, alignment: .top)
            // The controls are an inset of the editor: typed text stops above
            // them instead of running underneath, on every size of screen.
            .safeAreaInset(edge: .bottom, spacing: 0) { footer }
        }
        .captureChrome(close: close) { captureActions }
        // A full-screen cover has no swipe-to-close; keep that when it zooms from +.
        .interactiveDismissDisabled()
        .task {
            try? await Task.sleep(for: .milliseconds(350))
            settled = true
        }
        .task {
            guard !loaded, let note else { return }
            loaded = true
            title = note.title
            body_ = note.userNotes
            let captureStore = store
            let captureID = noteID
            recorder.onSegmentFinished = { segment, _ in
                captureStore.finishCapture(captureID, segment: segment)
            }
            recorder.prepare(existing: note.captureSegments)
            if connectionMode.isDemo { startSegment() }
            // Long enough for the cover to start arriving, short enough that
            // the keyboard rises with it.
            try? await Task.sleep(for: .milliseconds(150))
            if body_.isEmpty { focus = .body }
            #if DEBUG
            let args = ProcessInfo.processInfo.arguments
            if let i = args.firstIndex(of: "-autostop"), i + 1 < args.count, let secs = Double(args[i + 1]) {
                try? await Task.sleep(for: .seconds(secs))
                focus = nil
                stopSegment()
            }
            #endif
        }
        .onChange(of: title) { save() }
        .onChange(of: body_) { save() }
        .onDisappear {
            generationTask?.cancel()
            generationTask = nil
            stopSegment()
            save()
            store.flush()
            recorder.onSegmentFinished = nil
        }
        .alert("Attachments", isPresented: $unsupportedAttachment) {
            Button("OK", role: .cancel) {}
        } message: { Text("File attachments are not supported in the local demo.") }
    }

    // MARK: Chrome

    private var captureActions: some View {
        Group {
            RailIcon(systemImage: "paperclip") { unsupportedAttachment = true }
            Menu {
                if recorder.isCapturing {
                    Button("Stop recording", systemImage: "stop.circle") { stopSegment() }
                }
                Button("Delete note", systemImage: "trash", role: .destructive) {
                    recorder.stopSegment()
                    store.trash(noteID)
                    dismiss()
                }
            } label: {
                RailGlyph(systemImage: "ellipsis")
            }
            .accessibilityLabel("Recording actions")
        }
    }

    private var footer: some View {
        VStack(spacing: 18) {
            if !recorder.isCapturing, let recovery = note?.captureRecoveryHint {
                Text(recovery).font(.system(size: 13)).foregroundStyle(Theme.textSecondary)
                    .multilineTextAlignment(.center)
                    .transition(motion.fade)
            }
            if !(verticalSizeClass == .compact && focus != nil) {
                Text(!connectionMode.isDemo ? "Server recording is unavailable" : recorder.isSimulated ? "Simulated capture · No audio or transcription" : "On-device audio · Transcription unavailable")
                    .font(.system(size: 12))
                    .foregroundStyle(Theme.textSecondary)
                    .multilineTextAlignment(.center)
                    .transition(motion.fade)
            }
            if focus == nil && recorder.isCapturing {
                Text("Let people know you're using Cereal")
                    .font(.system(size: 16))
                    .foregroundStyle(Theme.textSecondary)
                    .multilineTextAlignment(.center)
                    .transition(motion.giveWay)
            }
            if case .failed(let message) = recorder.state {
                Text(message)
                    .font(.system(size: 14))
                    .foregroundStyle(Theme.danger)
                    .multilineTextAlignment(.center)
                    .transition(motion.fade)
            }
            GlassChromeGroup(spacing: 12) {
                HStack(spacing: 12) {
                    // Named surfaces: Generate buds out of the pill when capture stops.
                    RecordingPill(recorder: recorder, onStop: stopSegment, onStart: startSegment,
                                  onResumeFromPause: resumeFromPause)
                        .disabled(!connectionMode.isDemo || generating)
                        .glassMorph("pill", in: glass)
                    if showsGenerate, let note {
                        GenerateButton(title: note.needsGeneration ? note.generateActionTitle : "View notes",
                                       busy: generating) { generateAndOpen() }
                            .glassMorph("generate", in: glass)
                    }
                    Spacer(minLength: 0)
                    if focus != nil {
                        HideKeyboardButton { focus = nil }
                            .glassMorph("keyboard", in: glass)
                    }
                }
            }
        }
        .padding(.leading, rail.captureBarLeading).padding(.trailing, 20)
        .padding(.bottom, 12)
        .animation(settled ? motion.animation(.gooey, entering: focus != nil) : nil, value: focus)
        .animation(settled ? motion.gooey : nil, value: recorder.state)
        .animation(settled ? motion.animation(.gooey, entering: showsGenerate) : nil, value: showsGenerate)
    }

    private var showsGenerate: Bool {
        !recorder.isCapturing && (note?.hasCapture ?? false)
    }

    // MARK: Lifecycle

    private func startSegment() {
        guard connectionMode.isDemo, !generating else { return }
        store.invalidateGeneration(noteID)
        recorder.startSegment(noteID: noteID, title: title)
    }

    private func resumeFromPause() {
        guard connectionMode.isDemo, !generating else { return }
        store.invalidateGeneration(noteID)
        recorder.resumeFromPause()
    }

    private func stopSegment() {
        recorder.stopSegment()
    }

    private func generateAndOpen() {
        guard !generating, !recorder.isCapturing else { return }
        focus = nil
        generating = true
        generationTask = Task {
            await store.generateNotes(noteID) // no-op when notes already match the transcript revision
            guard !Task.isCancelled, !recorder.isCapturing, connectionMode.isDemo else { return }
            generating = false
            generationTask = nil
            openNote()
        }
    }

    /// Typing updates memory at once and the file after a short pause.
    /// Stop, close and backgrounding write immediately.
    private func save() {
        guard loaded else { return }
        store.updateDraft(noteID, title: title, userNotes: body_)
    }

    /// Leaving the surface ends capture (Stop) but doesn't generate; the note shows "Generate notes".
    private func close() {
        generationTask?.cancel()
        generationTask = nil
        stopSegment()
        save()
        store.flush()
        if store.note(noteID)?.isEmpty ?? true {
            store.discardIfEmpty(noteID)
            dismiss()
        } else {
            openNote()
        }
    }

    private func openNote() {
        dismiss()
        Task {
            try? await Task.sleep(for: .milliseconds(400))
            guard router.path.last != .note(noteID) else { return }
            router.path.append(.note(noteID))
        }
    }
}

struct RecordingPill: View {
    let recorder: Recorder
    let onStop: () -> Void
    let onStart: () -> Void
    var onResumeFromPause: (() -> Void)? = nil
    /// When set, the pill is the recording indicator on another screen: one
    /// tap returns to the recording screen, and it offers no Pause or Stop.
    var onOpen: (() -> Void)? = nil
    /// The recording screen grows out of this pill and returns into it.
    var zoomsToCapture = false
    @Environment(\.motion) private var motion

    @State private var pressed = false

    var body: some View {
        // This body reads only the recorder's state. The clock and the level
        // meter, which change 12 times a second, are read by the two leaf
        // views inside `pill`, so the glass, menu and button are not rebuilt.
        let _ = PerfProbe.hit("RecordingPill.body")
        // The pill is drawn once and stays on screen in every state. The menu
        // is an invisible target placed above the glass, not inside it: a menu
        // inside a glass surface takes that surface over as its own morph
        // source and hides it until the menu has finished closing.
        pill
            // Inside the glass, like the + button's: the source is the shape itself.
            .zoomSource(ZoomID.capture, cornerRadius: 24, enabled: zoomsToCapture)
            .scaleEffect(pressed && (onOpen != nil || !recorder.isCapturing) ? MotionResolver.pressScale(reduceMotion: motion.reduceMotion) : 1)
            .animation(motion.animation(.snappy, entering: !pressed), value: pressed)
            // One persistent surface: the tint and width flow between states
            // while only the glyph and timer inside trade places.
            .glassSurface(Capsule(), interactive: true, tint: pillColor)
            .animation(motion.gooey, value: recorder.state)
            .overlay {
                if let onOpen {
                    Button(action: onOpen) { Color.clear.contentShape(Capsule()) }
                        .buttonStyle(PressReportingStyle(pressed: $pressed))
                        .accessibilityLabel("Return to recording")
                        .accessibilityValue(recorder.state == .paused ? "Paused" : recorder.isCapturing ? "Recording" : "Stopped")
                } else {
                Menu {
                    if recorder.state == .recording {
                        Button("Pause", systemImage: "pause.fill") { recorder.pause() }
                    } else {
                        Button("Resume", systemImage: "play.fill") { (onResumeFromPause ?? recorder.resumeFromPause)() }
                    }
                    Button("Stop", systemImage: "stop.fill", role: .destructive, action: onStop)
                } label: { Color.clear.contentShape(Capsule()) }
                .menuOrder(.fixed)
                .accessibilityLabel("Recording controls")
                .accessibilityValue(recorder.state == .paused ? "Paused" : "Recording")
                .accessibilityHidden(!recorder.isCapturing)
                .allowsHitTesting(recorder.isCapturing)
                }
            }
            .overlay {
                if onOpen == nil, !recorder.isCapturing {
                    // Stopped or failed: tapping resumes capture into the same transcript.
                    Button(action: onStart) { Color.clear.contentShape(Capsule()) }
                        .buttonStyle(PressReportingStyle(pressed: $pressed))
                        .accessibilityLabel("Resume recording")
                }
            }
            // The user is usually looking at the meeting, not the screen.
            // As an indicator it stays quiet: the recording screen's own pill speaks.
            .sensoryFeedback(trigger: recorder.state) { old, new in
                onOpen == nil ? RecordingFeedback.feedback(from: old, to: new) : nil
            }
    }

    private var pill: some View {
        HStack(spacing: 10) {
            ZStack {
                switch recorder.state {
                case .recording:
                    RecorderWaveform(recorder: recorder).transition(motion.swap)
                case .paused:
                    // Resting bars look like a quiet room; a pause glyph says paused.
                    Image(systemName: "pause.fill").font(.system(size: 14))
                        .foregroundStyle(Theme.onAccent)
                        .frame(width: 20.5, height: 16)
                        .transition(motion.swap)
                case .idle, .stopped, .failed:
                    Image(systemName: "mic.fill").font(.system(size: 16))
                        .foregroundStyle(Theme.textPrimary)
                        .transition(motion.swap)
                }
            }
            .accessibilityHidden(true)
            RecorderTimer(recorder: recorder)
                .font(.system(size: 18).monospacedDigit())
                .foregroundStyle(recorder.isCapturing ? Theme.onAccent : Theme.textPrimary)
        }
        .padding(.horizontal, 18)
        .frame(height: 48)
        .contentShape(Capsule())
    }

    private var pillColor: Color {
        switch recorder.state {
        case .recording: Theme.accent
        case .paused: Theme.accent.opacity(0.55)
        case .idle, .stopped: Theme.surfaceRaised
        case .failed: Theme.danger.opacity(0.8)
        }
    }
}

/// Haptics for recording state changes.
enum RecordingFeedback {
    static func feedback(from old: Recorder.State, to new: Recorder.State) -> SensoryFeedback? {
        switch (old, new) {
        case (_, .recording) where old != .recording: .impact(weight: .medium)
        case (.recording, .paused): .impact(weight: .light)
        case (.recording, .stopped), (.paused, .stopped): .success
        case (_, .failed): .error
        default: nil
        }
    }
}

/// The only views that follow the recorder's clock and meter. Everything
/// around them depends on the recorder's state alone.
struct RecorderTimer: View {
    let recorder: Recorder
    var padMinutes = true
    var body: some View {
        let _ = PerfProbe.hit("RecorderTimer.body")
        RecordingTimerText(elapsed: recorder.elapsed, padMinutes: padMinutes)
            .accessibilityLabel("Elapsed time")
            .accessibilityValue(Duration.seconds(recorder.elapsed).formatted(.units(allowed: [.hours, .minutes, .seconds], width: .wide)))
            .accessibilityAddTraits(.updatesFrequently)
    }
}

struct RecorderWaveform: View {
    let recorder: Recorder
    var body: some View {
        WaveformBars(levels: recorder.levels, active: recorder.state == .recording)
    }
}

/// Digits roll once per second. The recorder publishes elapsed time far more
/// often, so the animation is keyed to the whole second.
struct RecordingTimerText: View {
    let elapsed: TimeInterval
    var padMinutes = true
    @Environment(\.motion) private var motion

    var body: some View {
        Text(Duration.seconds(elapsed).formatted(.time(pattern: padMinutes ? .minuteSecond(padMinuteToLength: 2) : .minuteSecond)))
            .contentTransition(motion.numericText)
            .animation(motion.reduceMotion ? nil : motion.snappy, value: Int(elapsed))
    }
}

struct GenerateButton: View {
    let title: String
    let busy: Bool
    let action: () -> Void
    @Environment(\.motion) private var motion

    var body: some View {
        Button(action: action) {
            HStack(spacing: 8) {
                ZStack {
                    if busy { ProgressView().tint(Theme.onAccent).transition(motion.swap) }
                    else { Image(systemName: "sparkles").transition(motion.swap) }
                }
                // The two labels trade places like the glyph beside them. Interpolating
                // the text instead left "Generate notes" readable under "Generating…"
                // for the length of the spring.
                ZStack(alignment: .leading) {
                    if busy { Text("Generating…").fixedSize().transition(motion.swap) }
                    else { Text(title).fixedSize().transition(motion.swap) }
                }
            }
            .font(.system(size: 17, weight: .medium))
            .foregroundStyle(Theme.onAccent)
            .padding(.horizontal, 20)
            .frame(height: 64)
            .glassSurface(Capsule(), interactive: true, tint: Theme.accent)
        }
        .buttonStyle(.squash)
        .disabled(busy)
        .animation(motion.gooey, value: busy)
    }
}

struct WaveformBars: View {
    let levels: [Float]
    let active: Bool
    @Environment(\.motion) private var motion

    var body: some View {
        HStack(spacing: 2) {
            ForEach(levels.indices, id: \.self) { i in
                Capsule()
                    .fill(Theme.onAccent)
                    .frame(width: 2.5, height: 4 + 12 * CGFloat(active ? levels[i] : 0.1))
            }
        }
        .frame(height: 16)
        // Runs only while the recorder publishes new levels.
        .animation(motion.waveform, value: levels)
        .animation(motion.waveform, value: active)
    }
}
