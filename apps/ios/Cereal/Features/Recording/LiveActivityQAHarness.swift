#if DEBUG
import SwiftUI
import Observation

/// Explicit OS-presentation QA. This model never constructs a Recorder, requests
/// microphone permission, opens a WAV, connects to an account, or calls an API.
@MainActor @Observable final class LiveActivityQAModel {
    enum Phase: String { case idle, running, paused, interrupted, writing, ended }
    private(set) var phase = Phase.idle
    private(set) var elapsed: TimeInterval = 0
    private(set) var systemState = "none"
    private(set) var systemIdentifier: String?
    private(set) var allowed = false
    private(set) var error: String?
    private let noteID = UUID()
    private let controller: RecordingLiveActivityController
    /// QA only: `-qa.liveActivityLook YES` draws the activity as a real recording's
    /// (no "Preview" wording), for checking how each state looks. Still no audio.
    let looksGenuine: Bool
    private var clock = RecordingClock()
    private let uptime: () -> TimeInterval

    init(controller: RecordingLiveActivityController? = nil,
         uptime: @escaping () -> TimeInterval = { ProcessInfo.processInfo.systemUptime }) {
        let genuine = controller == nil && UserDefaults.standard.bool(forKey: "qa.liveActivityLook")
        looksGenuine = genuine
        self.controller = controller ?? RecordingLiveActivityController(presentation: genuine ? nil : .preview)
        self.uptime = uptime
        refreshStatus()
    }
    func start() async {
        guard phase == .idle || phase == .ended else { return }
        // Finish the previous End before requesting another actual OS activity.
        await controller.finishPendingUpdates()
        clock.prepare(durations: []); clock.start(at: uptime()); elapsed = 0
        controller.start(noteID: noteID, title: looksGenuine ? "Design review" : "", elapsed: 0)
        if controller.hasActivity { phase = .running }
        else { _ = clock.stop(at: uptime()); phase = .idle }
        refreshStatus()
    }
    func pause() async {
        guard phase == .running else { return }
        clock.pause(at: uptime()); elapsed = clock.elapsed(at: uptime())
        controller.pause(elapsed: elapsed); phase = .paused
        await controller.finishPendingUpdates(); refreshStatus()
    }
    func resume() async {
        guard phase == .paused else { return }
        clock.resume(at: uptime()); phase = .running
        controller.resume(elapsed: elapsed)
        await controller.finishPendingUpdates(); refreshStatus()
    }
    func rotate() async {
        guard phase == .running else { return }
        _ = clock.stop(at: uptime()); clock.start(at: uptime()); elapsed = clock.elapsed(at: uptime())
        controller.start(noteID: noteID, title: "", elapsed: elapsed)
        await controller.finishPendingUpdates(); refreshStatus()
    }
    func end() {
        if phase == .running || phase == .paused {
            _ = clock.stop(at: uptime()); elapsed = clock.completed
        }
        controller.end(elapsed: elapsed); phase = .ended; refreshStatus()
    }
    /// End as a server recording ends: "Writing notes..." until `notesReady`.
    func endWritingNotes() async {
        guard phase == .running || phase == .paused else { return }
        _ = clock.stop(at: uptime()); elapsed = clock.completed
        controller.end(elapsed: elapsed, ending: .writingNotes); phase = .writing
        await controller.finishPendingUpdates(); refreshStatus()
    }
    func notesFinished(_ outcome: RecordingNotesOutcome) async {
        guard phase == .writing else { return }
        controller.notesFinished(noteID: noteID, outcome: outcome); phase = .ended
        await controller.finishPendingUpdates(); refreshStatus()
    }
    /// A recording that stopped on its own (a lost headset), as the activity shows it out of the app.
    func interrupt() async {
        guard phase == .running || phase == .paused else { return }
        _ = clock.stop(at: uptime()); elapsed = clock.completed
        controller.interrupt(elapsed: elapsed, cause: .failure); phase = .interrupted
        await controller.finishPendingUpdates(); refreshStatus()
    }
    /// Steps: start, pause, resume, rotate, end, writing, ready, failed, interrupt, cleanup, wait:<seconds>.
    func run(script: String) async {
        for step in script.split(separator: ",").map({ $0.trimmingCharacters(in: .whitespaces) }) {
            if step.hasPrefix("wait:"), let seconds = Double(step.dropFirst(5)) {
                // The clock is kept up to date while the script waits.
                let until = uptime() + seconds
                while uptime() < until { try? await Task.sleep(for: .milliseconds(500)); tick() }
                continue
            }
            switch step {
            case "start": await start()
            case "pause": await pause()
            case "resume": await resume()
            case "rotate": await rotate()
            case "end": end()
            case "writing": await endWritingNotes()
            case "ready": await notesFinished(.ready)
            case "failed": await notesFinished(.failed)
            case "interrupt": await interrupt()
            case "cleanup": await cleanup()
            default: continue
            }
        }
    }

    func cleanup() async {
        end(); await controller.finishPendingUpdates()
        if looksGenuine { await RecordingLiveActivityController.endOrphanedActivities() }
        else { await controller.endPreviewActivities() }
        refreshStatus()
    }
    func tick() {
        if phase == .running {
            elapsed = clock.elapsed(at: uptime())
            controller.heartbeat(elapsed: elapsed)
        }
        refreshStatus()
    }
    private func refreshStatus() {
        allowed = controller.activitiesEnabled
        systemState = controller.systemState; systemIdentifier = controller.systemIdentifier
        error = controller.lastError
    }
}

struct LiveActivityQAHarness: View {
    @Environment(\.dismiss) private var dismiss
    @State private var model = LiveActivityQAModel()
    @State private var busy = false
    /// Runs the next control 6 s later, with time to leave the app and watch it land.
    @State private var delayed = false
    @Environment(\.scenePhase) private var scenePhase

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    Label("Preview · No audio", systemImage: "play.circle").font(.headline)
                    Text("This is a real ActivityKit presentation test. It does not record, request the microphone, upload audio, or contact the server. The widget is labeled Preview.")
                        .font(.footnote)
                }
                Section("System state") {
                    LabeledContent("Live Activities allowed", value: model.allowed ? "Yes" : "No")
                    LabeledContent("Preview phase", value: model.phase.rawValue.capitalized)
                    LabeledContent("Actual ActivityKit state", value: model.systemState)
                    LabeledContent("Preview timer", value: Duration.seconds(model.elapsed).formatted(.time(pattern: .minuteSecond)))
                    if let id = model.systemIdentifier { Text("Activity ID: \(id)").font(.caption).textSelection(.enabled).accessibilityIdentifier("live-activity-qa-id") }
                    if let error = model.error { Text(error).foregroundStyle(Theme.danger).accessibilityIdentifier("live-activity-qa-error") }
                }
                Section {
                    Button("Start preview", systemImage: "play") { run { await model.start() } }
                        .disabled(model.phase == .running || model.phase == .paused).accessibilityIdentifier("live-activity-qa-start")
                    Button("Pause preview", systemImage: "pause") { run { await model.pause() } }
                        .disabled(model.phase != .running).accessibilityIdentifier("live-activity-qa-pause")
                    Button("Resume preview", systemImage: "play") { run { await model.resume() } }
                        .disabled(model.phase != .paused).accessibilityIdentifier("live-activity-qa-resume")
                    Button("Rotate preview (same activity)", systemImage: "arrow.triangle.2.circlepath") { run { await model.rotate() } }
                        .disabled(model.phase != .running).accessibilityIdentifier("live-activity-qa-rotate")
                    Button("End preview", systemImage: "stop", role: .destructive) { model.end() }
                        .disabled(model.phase != .running && model.phase != .paused).accessibilityIdentifier("live-activity-qa-end")
                    Button("End, then write notes", systemImage: "sparkles") { run { await model.endWritingNotes() } }
                        .disabled(model.phase != .running && model.phase != .paused).accessibilityIdentifier("live-activity-qa-writing")
                    Button("Notes ready", systemImage: "checkmark.circle") { run { await model.notesFinished(.ready) } }
                        .disabled(model.phase != .writing).accessibilityIdentifier("live-activity-qa-ready")
                    Button("Notes failed", systemImage: "xmark.circle") { run { await model.notesFinished(.failed) } }
                        .disabled(model.phase != .writing).accessibilityIdentifier("live-activity-qa-failed")
                    Button("Stop on its own (headset lost)", systemImage: "exclamationmark.triangle") { run { await model.interrupt() } }
                        .disabled(model.phase != .running && model.phase != .paused).accessibilityIdentifier("live-activity-qa-interrupt")
                    Toggle("Run the next control in 6 s", isOn: $delayed).accessibilityIdentifier("live-activity-qa-delay")
                    Button("End all preview activities", systemImage: "checkmark.circle") { run { await model.cleanup() } }
                        .accessibilityIdentifier("live-activity-qa-cleanup")
                } header: { Text("Preview controls") } footer: {
                    Text("Background or lock the device to inspect the actual OS presentation. Without an audio session the app may suspend; after 75 seconds without a heartbeat the system marks the preview stale. This does not validate real microphone or background audio capture.")
                }
            }
            .disabled(busy)
            .navigationTitle("Live Activity QA").navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .confirmationAction) { Button("Done") { run { await model.cleanup(); dismiss() } } } }
            .task {
                while !Task.isCancelled {
                    model.tick()
                    do { try await Task.sleep(for: .seconds(1)) } catch { return }
                }
            }
            .onChange(of: scenePhase) { _, phase in if phase == .active { model.tick() } }
            // QA only: `-qa.liveActivityScript "start,wait:3,pause,wait:20,resume"` drives the
            // controls in order, so each state can be inspected out of the app without tapping.
            .task {
                guard let script = UserDefaults.standard.string(forKey: "qa.liveActivityScript") else { return }
                let token = UIApplication.shared.beginBackgroundTask(withName: "Live Activity QA script")
                await model.run(script: script)
                UIApplication.shared.endBackgroundTask(token)
            }
            .onDisappear { model.end() }
            .interactiveDismissDisabled(busy)
        }
    }
    private func run(_ action: @escaping @MainActor () async -> Void) {
        busy = true
        let wait = delayed
        delayed = false
        Task {
            if wait {
                // Kept running while it waits out of the app.
                let token = UIApplication.shared.beginBackgroundTask(withName: "Live Activity QA")
                try? await Task.sleep(for: .seconds(6))
                await action()
                UIApplication.shared.endBackgroundTask(token)
            } else {
                await action()
            }
            busy = false
        }
    }
}
#endif
