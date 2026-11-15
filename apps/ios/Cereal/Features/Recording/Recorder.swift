import AVFoundation
import Observation
import UIKit
import APIClient

enum MicPermission {
    static var isGranted: Bool { get async { AVAudioApplication.shared.recordPermission == .granted } }
    static var isUndetermined: Bool { AVAudioApplication.shared.recordPermission == .undetermined }
    static func request() async -> Bool { await AVAudioApplication.requestRecordPermission() }
}

/// Local demo provider seam. Server capture uses the authenticated, durable audio outbox.
protocol TranscriptionClient: Sendable {
    func startSegment(noteID: UUID, index: Int) async throws
    func finishSegment(noteID: UUID, segment: CaptureSegment, audioFile: URL) async throws
}

struct LocalTranscriptionClient: TranscriptionClient {
    func startSegment(noteID: UUID, index: Int) async throws {}
    func finishSegment(noteID: UUID, segment: CaptureSegment, audioFile: URL) async throws {}
}

private final class RecordingObservers: @unchecked Sendable {
    var tokens: [NSObjectProtocol] = []
    deinit { tokens.forEach(NotificationCenter.default.removeObserver) }
}

/// Native, local-only audio capture. Stop finalizes audio, never generates notes or transcripts.
///
/// This type is the recording's state, clock, audio session, Live Activity and
/// recovery journal. The audio itself is written by a `CaptureBackend`, chosen
/// once per capture by `CaptureEnginePolicy`.
@MainActor
@Observable
final class Recorder {
    enum State: Equatable { case idle, recording, paused, stopped, failed(String) }

    private(set) var state: State = .idle
    private(set) var elapsed: TimeInterval = 0
    private(set) var levels = Recorder.restingLevels
    private(set) var currentFileURL: URL?
    let isSimulated: Bool
    /// Called once for every finalized segment, including interruptions and partial failures.
    var onSegmentFinished: ((CaptureSegment, URL) -> Void)?
    /// Continuous capture only. Set before starting: the capture reports each
    /// time it grows past another multiple of this many seconds of audio.
    @ObservationIgnored var partSeconds: Int?
    /// Samples in the capture file at the boundary just crossed.
    @ObservationIgnored var onPartBoundary: ((Int64) -> Void)?
    /// True on pause, false on resume. The capture's sample count is final when this is called.
    @ObservationIgnored var onPauseChanged: ((Bool) -> Void)?

    private static let restingLevels: [Float] = Array(repeating: 0.12, count: 5)
    @ObservationIgnored private var backend: (any CaptureBackend)?
    @ObservationIgnored private let makeBackend: @MainActor () -> any CaptureBackend
    @ObservationIgnored private var meterTask: Task<Void, Never>?
    @ObservationIgnored private let observers = RecordingObservers()
    @ObservationIgnored private var activity = RecordingLiveActivityController()
    @ObservationIgnored private var activityNoteID: UUID?
    private var clock = RecordingClock()
    private var segmentIndex = 0
    private var segmentStartedAt = Date.now
    private let recordingRoot: URL?

    var isCapturing: Bool { state == .recording || state == .paused }
    /// True while the capture in progress is one continuous file that parts can be cut from.
    var capturesContinuously: Bool { backend is any ContinuousCaptureBackend }
    /// Samples (16 kHz) in the continuous capture file now, or as it was left by Stop.
    var capturedFrames: Int64 { (backend as? any ContinuousCaptureBackend)?.capturedFrames ?? lastCapturedFrames }
    @ObservationIgnored private var lastCapturedFrames: Int64 = 0
    /// Meter ticks (80 ms each) in a row on which the input could not be started again.
    @ObservationIgnored private var inputRetries = 0
    static let inputRetryLimit = 12
    /// Only audio from the device's own input is shown as a system recording.
    private var advertisesCapture: Bool {
        #if DEBUG
        // QA only: `-capture.advertise YES` shows the Live Activity for a recording
        // of a file (`-capture.inputFile`), so it can be checked without the microphone.
        if backend != nil, UserDefaults.standard.bool(forKey: "capture.advertise") { return true }
        #endif
        return backend?.usesMicrophone == true
    }

    /// The title of the note this recorder records, for its Live Activity.
    @ObservationIgnored private var activityTitle = ""

    /// The note being recorded has a new title. Its Live Activity follows.
    func setActivityTitle(_ title: String) {
        activityTitle = title
        activity.setTitle(title)
    }

    init(simulateCapture: Bool = Recorder.allowsSimulatedCapture, recordingRoot: URL? = nil,
         backend: (@MainActor () -> any CaptureBackend)? = nil) {
        self.recordingRoot = recordingRoot
        makeBackend = backend ?? { CaptureEnginePolicy.makeBackend() }
        #if DEBUG && targetEnvironment(simulator)
        isSimulated = simulateCapture
        #else
        isSimulated = false
        #endif
        observeSession()
    }

    func prepare(existing segments: [CaptureSegment]) {
        guard !isCapturing else { return }
        clock.prepare(durations: segments.map(\.duration))
        segmentIndex = (segments.map(\.index).max() ?? -1) + 1
        elapsed = clock.completed
        state = segments.isEmpty ? .idle : .stopped
    }

    func startSegment(noteID: UUID, title: String = "") {
        guard !isCapturing else { return }
        var created: URL?
        do {
            let backend = isSimulated ? nil : makeBackend()
            guard backend?.usesMicrophone != true || AVAudioApplication.shared.recordPermission == .granted else {
                throw RecorderError.permissionRequired
            }
            let url = try Self.fileURL(noteID: noteID, index: segmentIndex, simulated: isSimulated,
                                       continuous: backend is any ContinuousCaptureBackend, root: recordingRoot)
            let startedAt = Date.now
            if let backend {
                // Commit recovery metadata before any real audio can be captured.
                try CaptureRecovery.begin(noteID: noteID, index: segmentIndex, startedAt: startedAt, audioFile: url)
                created = url
                if backend.usesMicrophone {
                    let session = AVAudioSession.sharedInstance()
                    try session.setCategory(.record, mode: .default, options: [.allowBluetoothHFP])
                    try session.setActive(true)
                }
                backend.onFailure = { [weak self, weak backend] message in
                    guard let self, let backend, self.backend === backend, isCapturing else { return }
                    fail(message)
                }
                if let continuous = backend as? any ContinuousCaptureBackend {
                    continuous.partFrames = partSeconds.map { Int64($0) * Int64(ContinuousCapture.sampleRate) }
                    continuous.onPartBoundary = { [weak self] frames in self?.onPartBoundary?(frames) }
                }
                self.backend = backend
                usesInjectedAudio = !backend.usesMicrophone
                try backend.start(writingTo: url)
                try FileManager.default.setAttributes([.protectionKey: FileProtectionType.completeUntilFirstUserAuthentication], ofItemAtPath: url.path)
            }
            currentFileURL = url
            activityNoteID = noteID
            lastCapturedFrames = 0
            inputRetries = 0
            clock.start(at: ProcessInfo.processInfo.systemUptime)
            segmentStartedAt = startedAt
            state = .recording
            // Simulated timing is never advertised as genuine system recording.
            // A later part of the same recording does not bring the title again.
            if !title.isEmpty { activityTitle = title }
            if advertisesCapture { activity.start(noteID: noteID, title: activityTitle, elapsed: elapsed) }
            startMetering()
        } catch {
            let continuous = backend is any ContinuousCaptureBackend
            _ = backend?.stop()
            backend = nil
            // A continuous capture that never started holds no audio. Leaving its
            // empty file and journal would be reported as an unreadable recording.
            if continuous, let created { Self.discardEmptyCapture(created) }
            currentFileURL = nil
            activityNoteID = nil
            deactivateSession()
            activity.end()
            state = .failed((error as? RecorderError)?.localizedDescription ?? "Recording could not start. Check your microphone and try again.")
        }
    }

    func pause() {
        guard state == .recording else { return }
        backend?.pause()
        clock.pause(at: ProcessInfo.processInfo.systemUptime)
        elapsed = clock.completed + (backend?.capturedTime ?? clock.segment)
        state = .paused
        levels = Self.restingLevels
        meterTask?.cancel()
        meterTask = nil
        if advertisesCapture { activity.pause(elapsed: elapsed) }
        deactivateSession()
        onPauseChanged?(true)
    }

    func resumeFromPause() {
        guard state == .paused else { return }
        do {
            if let backend {
                if backend.usesMicrophone { try AVAudioSession.sharedInstance().setActive(true) }
                try backend.resume()
            }
            clock.resume(at: ProcessInfo.processInfo.systemUptime)
            state = .recording
            if advertisesCapture, let noteID = activityNoteID { activity.start(noteID: noteID, title: activityTitle, elapsed: elapsed) }
            startMetering()
            onPauseChanged?(false)
        } catch {
            fail("Recording could not resume. Your captured audio has been kept.")
        }
    }

    /// Returns and delivers the segment once. Every subsequent Stop is a no-op.
    @discardableResult
    func stopSegment(keepingLiveActivity: Bool = false) -> (segment: CaptureSegment, file: URL)? {
        finalize(failure: nil, keepingLiveActivity: keepingLiveActivity)
    }

    private func finalize(failure: String?, keepingLiveActivity: Bool = false) -> (segment: CaptureSegment, file: URL)? {
        guard isCapturing, let url = currentFileURL else {
            if !keepingLiveActivity { activity.end(elapsed: elapsed) }
            return nil
        }
        meterTask?.cancel()
        meterTask = nil
        let continuous = backend as? any ContinuousCaptureBackend
        // The backend closes the file and says how much audio it holds.
        let captured = backend?.stop()
        lastCapturedFrames = continuous?.capturedFrames ?? 0
        backend = nil
        let duration = clock.stop(at: ProcessInfo.processInfo.systemUptime, capturedDuration: isSimulated ? nil : captured)
        currentFileURL = nil
        activityNoteID = nil
        deactivateSession()
        if !keepingLiveActivity { activity.end(elapsed: clock.completed) }
        elapsed = clock.completed
        levels = Self.restingLevels
        state = failure.map(State.failed) ?? .stopped
        // Failed zero-length starts must not create transcript revisions.
        guard duration > 0.01 else {
            if continuous != nil, lastCapturedFrames == 0 { Self.discardEmptyCapture(url) }
            return nil
        }
        let segment = CaptureSegment(index: segmentIndex, startedAt: segmentStartedAt, duration: duration, fileName: url.lastPathComponent)
        segmentIndex += 1
        onSegmentFinished?(segment, url)
        return (segment, url)
    }

    private func fail(_ message: String) { _ = finalize(failure: message) }

    private func startMetering() {
        meterTask?.cancel()
        meterTask = Task { [weak self] in
            while !Task.isCancelled {
                guard self != nil else { return }
                self?.tick()
                try? await Task.sleep(for: .milliseconds(80))
            }
        }
    }

    private func tick() {
        guard state == .recording else { return }
        if let backend {
            if !backend.isRunning {
                // An engine stops when its route changes. A continuous capture starts its
                // input again on the route that is left (the built-in microphone, after a
                // headset goes) and carries on in the same file. A route can take a moment
                // to settle, so it is asked for about a second before the capture is given up.
                guard let continuous = backend as? any ContinuousCaptureBackend else {
                    fail("Recording stopped unexpectedly. Your captured audio has been kept. Tap to resume.")
                    return
                }
                if let message = continuous.failureMessage { fail(message); return }
                guard continuous.restartInput() else {
                    inputRetries += 1
                    if inputRetries >= Self.inputRetryLimit {
                        fail("The microphone disconnected. Your audio has been kept. Choose an input and tap to resume.")
                    }
                    return
                }
            }
            inputRetries = 0
            elapsed = clock.completed + backend.capturedTime
            levels = Array(levels.dropFirst()) + [backend.level()]
            if advertisesCapture { activity.heartbeat(elapsed: elapsed) }
        } else if isSimulated {
            elapsed = clock.elapsed(at: ProcessInfo.processInfo.systemUptime)
            levels = Self.restingLevels // Timing preview has no microphone or invented voice levels.
        }
    }

    private func observeSession() {
        observe(AVAudioSession.interruptionNotification, valueKey: AVAudioSessionInterruptionTypeKey) { [weak self] raw in
            if raw == AVAudioSession.InterruptionType.began.rawValue {
                self?.fail("Recording was interrupted. Your audio has been kept. Tap to resume when ready.")
            }
        }
        observe(AVAudioSession.routeChangeNotification, valueKey: AVAudioSessionRouteChangeReasonKey) { [weak self] reason in
            guard let self, backend?.usesMicrophone == true, isCapturing else { return }
            if RouteChangePolicy.endsCapture(inputDeviceRemoved: reason == AVAudioSession.RouteChangeReason.oldDeviceUnavailable.rawValue,
                                             noInputAvailable: AVAudioSession.sharedInstance().currentRoute.inputs.isEmpty,
                                             continuous: backend is any ContinuousCaptureBackend) {
                fail("The microphone disconnected. Your audio has been kept. Choose an input and tap to resume.")
            }
        }
        observe(AVAudioSession.mediaServicesWereResetNotification) { [weak self] _ in
            self?.fail("The audio system restarted. Your audio has been kept. Tap to resume.")
        }
        observe(UIApplication.didBecomeActiveNotification) { [weak self] _ in
            guard let self else { return }
            tick()
            if advertisesCapture, state == .recording, let noteID = activityNoteID {
                // Initial Activity.request can fail if server setup finishes after
                // backgrounding. Retrying the indicator never restarts audio.
                activity.start(noteID: noteID, title: activityTitle, elapsed: elapsed)
            }
        }
        observe(UIApplication.willTerminateNotification) { [weak self] _ in self?.stopSegment() }
    }

    private func observe(_ name: Notification.Name, valueKey: String? = nil,
                         action: @escaping @MainActor @Sendable (UInt?) -> Void) {
        observers.tokens.append(NotificationCenter.default.addObserver(forName: name, object: nil, queue: .main) { notification in
            let value = valueKey.flatMap { notification.userInfo?[$0] as? UInt }
            MainActor.assumeIsolated { action(value) }
        })
    }

    /// Set when a capture starts from something other than the microphone; the audio session is then left alone.
    @ObservationIgnored private var usesInjectedAudio = false

    private func deactivateSession() {
        if !isSimulated, !usesInjectedAudio { try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation) }
    }

    /// Removes a continuous capture that holds no samples, with its journal.
    private static func discardEmptyCapture(_ url: URL) {
        if FileManager.default.fileExists(atPath: url.path) {
            guard ContinuousCapture.frames(inFile: url) == 0 else { return }
            try? FileManager.default.removeItem(at: url)
        }
        CaptureRecovery.acknowledge(CaptureRecovery.journalURL(for: url))
    }

    static var allowsSimulatedCapture: Bool {
        #if DEBUG && targetEnvironment(simulator)
        RecordingLaunchPolicy.requestsSimulation(arguments: ProcessInfo.processInfo.arguments, debugSimulator: true)
        #else
        false
        #endif
    }

    /// `segment-N.wav` is one recorder segment; `capture-N.wav` is one continuous capture.
    static func fileURL(noteID: UUID, index: Int, simulated: Bool = false, continuous: Bool = false, root: URL? = nil) throws -> URL {
        let base = root ?? FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("recordings", isDirectory: true)
        let dir = base.appendingPathComponent(noteID.uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true,
            attributes: [.protectionKey: FileProtectionType.completeUntilFirstUserAuthentication])
        var resource = dir
        var values = URLResourceValues()
        values.isExcludedFromBackup = true
        try resource.setResourceValues(values)
        let stem = continuous ? "capture" : "segment"
        let candidate = dir.appendingPathComponent(simulated ? "simulated-segment-\(index).preview" : "\(stem)-\(index).wav")
        // An unfinished segment from a terminated process must never be overwritten.
        if FileManager.default.fileExists(atPath: candidate.path) ||
            FileManager.default.fileExists(atPath: CaptureRecovery.journalURL(for: candidate).path) {
            return dir.appendingPathComponent("\(stem)-\(index)-\(UUID().uuidString).wav")
        }
        return candidate
    }

    enum RecorderError: LocalizedError {
        case couldNotStart, permissionRequired
        var errorDescription: String? {
            switch self {
            case .couldNotStart: "Recording could not start. Check that a microphone is available."
            case .permissionRequired: "Allow microphone access in Settings, then tap to start recording."
            }
        }
    }
}
