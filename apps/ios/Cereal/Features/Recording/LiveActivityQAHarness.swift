#if DEBUG
import SwiftUI
import Observation

/// Explicit OS-presentation QA. This model never constructs a Recorder, requests
/// microphone permission, opens a WAV, connects to an account, or calls an API.
@MainActor @Observable final class LiveActivityQAModel {
    enum Phase: String { case idle, running, paused, ended }
    private(set) var phase = Phase.idle
    private(set) var elapsed: TimeInterval = 0
    private(set) var systemState = "none"
    private(set) var systemIdentifier: String?
    private(set) var allowed = false
    private(set) var error: String?
    private let noteID = UUID()
    private let controller: RecordingLiveActivityController
    private var clock = RecordingClock()
    private let uptime: () -> TimeInterval

    init(controller: RecordingLiveActivityController? = nil,
         uptime: @escaping () -> TimeInterval = { ProcessInfo.processInfo.systemUptime }) {
        self.controller = controller ?? RecordingLiveActivityController(presentation: .preview)
        self.uptime = uptime
        refreshStatus()
    }
    func start() async {
        guard phase == .idle || phase == .ended else { return }
        // Finish the previous End before requesting another actual OS activity.
        await controller.finishPendingUpdates()
        clock.prepare(durations: []); clock.start(at: uptime()); elapsed = 0
        controller.start(noteID: noteID, title: "", elapsed: 0)
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
    func cleanup() async {
        end(); await controller.finishPendingUpdates()
        await controller.endPreviewActivities()
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
            .onDisappear { model.end() }
            .interactiveDismissDisabled(busy)
        }
    }
    private func run(_ action: @escaping @MainActor () async -> Void) {
        busy = true
        Task { await action(); busy = false }
    }
}
#endif
