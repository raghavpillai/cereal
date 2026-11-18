import AVFoundation
import Observation
import UIKit
import APIClient

enum MicPermission {
    static var isGranted: Bool { get async { AVAudioApplication.shared.recordPermission == .granted } }
    static var isUndetermined: Bool { AVAudioApplication.shared.recordPermission == .undetermined }
    static func request() async -> Bool { await AVAudioApplication.requestRecordPermission() }
}

/// The recording's audio session, activated and released on one queue, in the order asked.
///
/// Both calls wait for the audio hardware (longer on a Bluetooth route), and a release also
/// tells other apps they may play again: tens to hundreds of milliseconds. A release therefore
/// never runs on the main thread, where it would hold the frame that follows End or Pause. An
/// activation waits for whatever was asked before it, so a release still in flight can never
/// land after the activation that follows it (Pause, then Resume at once).
enum RecordingAudioSession {
    private static let queue = DispatchQueue(label: AppIdentity.subsystem + ".audio-session", qos: .userInitiated)

    /// Ready to record when this returns. Fast when `warm` has already brought the session up.
    static func activate() throws {
        try queue.sync {
            let session = AVAudioSession.sharedInstance()
            try session.setCategory(.record, mode: .default, options: [.allowBluetoothHFP])
            try session.setActive(true)
        }
    }

    /// The session of a paused recording, active again.
    static func reactivate() throws {
        try queue.sync { try AVAudioSession.sharedInstance().setActive(true) }
    }

    /// Brings the session up without waiting for it, while something else is waited for (the
    /// server's answer to Start). Whoever asks must `release` it if no capture follows.
    static func warm() {
        queue.async {
            let session = AVAudioSession.sharedInstance()
            try? session.setCategory(.record, mode: .default, options: [.allowBluetoothHFP])
            try? session.setActive(true)
        }
    }

    /// Lets the session go, without waiting for it.
    static func release() {
        queue.async { try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation) }
    }
}

/// Local demo provider seam. Server capture is transcribed by its live stream.
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
    /// True on pause, false on resume. The capture's sample count is final when this is called.
    @ObservationIgnored var onPauseChanged: ((Bool) -> Void)?

    private static let restingLevels: [Float] = Array(repeating: 0.12, count: 5)
    @ObservationIgnored private var backend: (any CaptureBackend)?
    @ObservationIgnored private let makeBackend: @MainActor () -> any CaptureBackend
    @ObservationIgnored private var meterTask: Task<Void, Never>?
    @ObservationIgnored private let observers = RecordingObservers()
    @ObservationIgnored private let activity: RecordingLiveActivityController
    /// Plays what each change of the recording feels like. Called once per change,
    /// whichever screen or control asked for it, so nothing is felt twice.
    @ObservationIgnored var playHaptic: @MainActor (RecordingHaptic) -> Void = RecordingFeedback.play
    @ObservationIgnored private var activityNoteID: UUID?
    private var clock = RecordingClock()
    private var segmentIndex = 0
    private var segmentStartedAt = Date.now
    private let recordingRoot: URL?

    var isCapturing: Bool { state == .recording || state == .paused }
    /// True while the capture in progress is one continuous file that a live stream can read.
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
    /// The account of the note this recorder records. Nil for the local demo.
    @ObservationIgnored var activityAccount: RecordingActivityAccount?

    /// The note being recorded has a new title. Its Live Activity follows.
    func setActivityTitle(_ title: String) {
        activityTitle = title
        activity.setTitle(title)
    }

    /// `activity`: the Live Activity this recorder reports to. A recording owner that
    /// makes a new recorder for each recording passes the same one, so its activity
    /// lasts from one recording to the next and is never doubled.
    init(simulateCapture: Bool = Recorder.allowsSimulatedCapture, recordingRoot: URL? = nil,
         backend: (@MainActor () -> any CaptureBackend)? = nil, activity: RecordingLiveActivityController? = nil) {
        self.recordingRoot = recordingRoot
        self.activity = activity ?? RecordingLiveActivityController()
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
                if backend.usesMicrophone { try RecordingAudioSession.activate() }
                backend.onFailure = { [weak self, weak backend] message in
                    guard let self, let backend, self.backend === backend, isCapturing else { return }
                    fail(message, cause: .failure)
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
            if advertisesCapture { activity.start(noteID: noteID, title: activityTitle, elapsed: elapsed, account: activityAccount) }
            startMetering()
            if !continuesRecording { playHaptic(.started) }
            continuesRecording = false
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
            activity.end(ending: .discard)
            state = .failed((error as? RecorderError)?.localizedDescription ?? "Recording could not start. Check your microphone and try again.")
            playHaptic(.failed)
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
        playHaptic(.paused)
    }

    func resumeFromPause() {
        guard state == .paused else { return }
        do {
            if let backend {
                if backend.usesMicrophone { try RecordingAudioSession.reactivate() }
                try backend.resume()
            }
            clock.resume(at: ProcessInfo.processInfo.systemUptime)
            state = .recording
            if advertisesCapture, let noteID = activityNoteID { activity.start(noteID: noteID, title: activityTitle, elapsed: elapsed, account: activityAccount) }
            startMetering()
            onPauseChanged?(false)
            playHaptic(.resumed)
        } catch {
            fail("Recording could not resume. Your captured audio has been kept.", cause: .failure)
        }
    }

    /// Why a recording stopped without being asked to.
    enum FailureCause: Equatable, Sendable {
        /// A call or Siri took the audio session.
        case interruption
        /// The microphone went away: a headset disconnected and no input is left.
        case inputLost
        /// The audio system or the capture failed.
        case failure
    }

    /// Returns and delivers the segment once. Every subsequent Stop is a no-op.
    /// `activity` says what the Live Activity does next: "Recording ended", "Writing
    /// notes..." while the owner writes them, or gone at once for a cancelled recording.
    @discardableResult
    func stopSegment(activity ending: RecordingActivityEnding = .ended) -> (segment: CaptureSegment, file: URL)? {
        finalize(failure: nil, ending: ending)
    }

    /// An internal boundary of one recording (a new part follows at once): the
    /// Live Activity is left as it is and nothing is felt.
    @discardableResult
    func stopSegment(keepingLiveActivity: Bool) -> (segment: CaptureSegment, file: URL)? {
        let finished = finalize(failure: nil, ending: keepingLiveActivity ? nil : .ended)
        continuesRecording = keepingLiveActivity && finished != nil
        return finished
    }
    /// The last stop was an internal boundary: the next start is the same recording going on.
    @ObservationIgnored private var continuesRecording = false

    /// `ending`: nil leaves the Live Activity alone (an internal boundary).
    private func finalize(failure: String?, cause: FailureCause = .failure, ending: RecordingActivityEnding? = .ended) -> (segment: CaptureSegment, file: URL)? {
        guard isCapturing, let url = currentFileURL else {
            // A stop after the recording had already stopped (an interruption, then End)
            // still moves the activity on; it never starts a new one.
            if failure == nil, let ending { activity.end(elapsed: elapsed, ending: ending) }
            return nil
        }
        meterTask?.cancel()
        meterTask = nil
        // Felt with the press, before the input is stopped and the file closed (tens of milliseconds).
        if failure != nil { playHaptic(RecordingHaptic.stopped(by: cause)) }
        else if let ending { playHaptic(ending == .discard ? .discarded : .ended) }
        let continuous = backend as? any ContinuousCaptureBackend
        // The backend closes the file and says how much audio it holds.
        let captured = backend?.stop()
        lastCapturedFrames = continuous?.capturedFrames ?? 0
        backend = nil
        let duration = clock.stop(at: ProcessInfo.processInfo.systemUptime, capturedDuration: isSimulated ? nil : captured)
        currentFileURL = nil
        activityNoteID = nil
        deactivateSession()
        if failure != nil {
            activity.interrupt(elapsed: clock.completed, cause: cause == .interruption ? .interruption : .failure)
        } else if let ending {
            activity.end(elapsed: clock.completed, ending: ending)
        }
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

    private func fail(_ message: String, cause: FailureCause) { _ = finalize(failure: message, cause: cause) }

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
                    fail("Recording stopped unexpectedly. Your captured audio has been kept. Tap to resume.", cause: .failure)
                    return
                }
                if let message = continuous.failureMessage { fail(message, cause: .failure); return }
                guard continuous.restartInput() else {
                    inputRetries += 1
                    if inputRetries >= Self.inputRetryLimit {
                        fail("The microphone disconnected. Your audio has been kept. Choose an input and tap to resume.", cause: .inputLost)
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
                self?.fail("Recording was interrupted. Your audio has been kept. Tap to resume when ready.", cause: .interruption)
            }
        }
        observe(AVAudioSession.routeChangeNotification, valueKey: AVAudioSessionRouteChangeReasonKey) { [weak self] reason in
            guard let self, backend?.usesMicrophone == true, isCapturing else { return }
            if RouteChangePolicy.endsCapture(inputDeviceRemoved: reason == AVAudioSession.RouteChangeReason.oldDeviceUnavailable.rawValue,
                                             noInputAvailable: AVAudioSession.sharedInstance().currentRoute.inputs.isEmpty,
                                             continuous: backend is any ContinuousCaptureBackend) {
                fail("The microphone disconnected. Your audio has been kept. Choose an input and tap to resume.", cause: .inputLost)
            }
        }
        observe(AVAudioSession.mediaServicesWereResetNotification) { [weak self] _ in
            self?.fail("The audio system restarted. Your audio has been kept. Tap to resume.", cause: .failure)
        }
        observe(UIApplication.didBecomeActiveNotification) { [weak self] _ in
            guard let self else { return }
            tick()
            if advertisesCapture, state == .recording, let noteID = activityNoteID {
                // Initial Activity.request can fail if server setup finishes after
                // backgrounding. Retrying the indicator never restarts audio.
                activity.start(noteID: noteID, title: activityTitle, elapsed: elapsed, account: activityAccount)
            }
            // A recording that stopped on its own is now said on its screen.
            activity.appBecameActive(isCapturing: isCapturing)
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
        // Off the main thread: the input has already stopped, and the state this is called for
        // (stopped, paused, failed) must not wait for the hardware to be let go.
        if !isSimulated, !usesInjectedAudio { RecordingAudioSession.release() }
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
