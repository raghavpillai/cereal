import AVFoundation
import APIClient

/// Where a capture's audio comes from. Buffers are delivered on the input's
/// own thread; everything else happens on the main actor.
@MainActor
protocol CaptureInput: AnyObject {
    var usesMicrophone: Bool { get }
    var isRunning: Bool { get }
    /// `changed` is called when the input stopped because its route or format
    /// changed and it has to be started again.
    func start(deliver: @escaping @Sendable (AVAudioPCMBuffer) -> Void, changed: @escaping @MainActor () -> Void) throws
    func stop()
}

/// The device's input through one `AVAudioEngine` tap, in the hardware's own
/// format. Every start builds a new engine, so a start after a route change, a
/// pause or a media-services reset always sees the current hardware format.
@MainActor
final class MicrophoneInput: CaptureInput {
    let usesMicrophone = true
    private var engine: AVAudioEngine?
    private var observer: NSObjectProtocol?

    var isRunning: Bool { engine?.isRunning ?? false }

    func start(deliver: @escaping @Sendable (AVAudioPCMBuffer) -> Void, changed: @escaping @MainActor () -> Void) throws {
        stop()
        let engine = AVAudioEngine()
        let input = engine.inputNode
        let format = input.outputFormat(forBus: 0)
        // No input route: the format has no channels or no rate.
        guard format.sampleRate > 0, format.channelCount > 0 else { throw Recorder.RecorderError.couldNotStart }
        // About 85 ms at 48 kHz. The system may deliver a different size.
        input.installTap(onBus: 0, bufferSize: 4096, format: format, block: Self.tap(deliver))
        engine.prepare()
        do { try engine.start() } catch {
            input.removeTap(onBus: 0)
            throw Recorder.RecorderError.couldNotStart
        }
        // Posted when the input's route or sample rate changes; the engine has already stopped.
        observer = NotificationCenter.default.addObserver(forName: .AVAudioEngineConfigurationChange, object: engine, queue: .main) { _ in
            MainActor.assumeIsolated { changed() }
        }
        self.engine = engine
    }

    func stop() {
        if let observer { NotificationCenter.default.removeObserver(observer) }
        observer = nil
        engine?.inputNode.removeTap(onBus: 0)
        engine?.stop()
        engine = nil
    }

    /// Built outside the main actor: the block runs on the engine's tap thread.
    private nonisolated static func tap(_ deliver: @escaping @Sendable (AVAudioPCMBuffer) -> Void) -> AVAudioNodeTapBlock {
        { buffer, _ in deliver(buffer) }
    }
}

/// The capture path: one input tap feeds a pipeline that writes one continuous
/// capture file. There is no rotation and so nothing to lose at one. Parts for
/// delivery are cut from the file by sample index while it grows, and the live
/// stream reads the same file.
@MainActor
final class EngineCaptureBackend: ContinuousCaptureBackend {
    var onFailure: ((String) -> Void)?
    var partFrames: Int64?
    var onPartBoundary: ((Int64) -> Void)?
    private let input: any CaptureInput
    private var pipeline: CapturePipeline?
    private var finalFrames: Int64 = 0

    init(input: any CaptureInput = MicrophoneInput()) { self.input = input }

    var usesMicrophone: Bool { input.usesMicrophone }
    var isRunning: Bool { input.isRunning && pipeline?.failure == nil }
    var capturedFrames: Int64 { pipeline?.frames ?? finalFrames }
    var capturedTime: TimeInterval { Double(capturedFrames) / Double(ContinuousCapture.sampleRate) }
    func level() -> Float { pipeline?.level ?? 0.12 }

    func start(writingTo url: URL) throws {
        var options = CapturePipeline.Options()
        options.partFrames = partFrames
        let pipeline = try CapturePipeline(url: url, options: options,
            onPartBoundary: { [weak self] frames in Task { @MainActor in self?.onPartBoundary?(frames) } },
            onFailure: { [weak self] failure in Task { @MainActor in self?.pipelineFailed(failure) } })
        self.pipeline = pipeline
        finalFrames = 0
        do { try startInput() } catch {
            pipeline.finish()
            self.pipeline = nil
            throw error
        }
    }

    /// The input stops and the converter is drained, so the file stops growing
    /// at an exact sample. Resume appends at that sample.
    func pause() {
        input.stop()
        pipeline?.flush()
    }

    func resume() throws { try startInput() }

    func stop() -> TimeInterval? {
        input.stop()
        guard let pipeline else { return nil }
        finalFrames = pipeline.finish()
        self.pipeline = nil
        return Double(finalFrames) / Double(ContinuousCapture.sampleRate)
    }

    private func startInput() throws {
        guard let pipeline else { throw Recorder.RecorderError.couldNotStart }
        try input.start(deliver: { buffer in pipeline.append(buffer) },
                        changed: { [weak self] in self?.inputChanged() })
    }

    /// A route change (AirPods connecting or leaving, a different sample rate)
    /// stops the engine. A new engine starts on the route that is current now,
    /// which after a headset leaves is the built-in microphone; the pipeline
    /// rebuilds its converter at the first buffer and the position in the file
    /// is unchanged.
    /// If the new route is not ready yet, the recorder's meter tick asks again
    /// for about a second before it gives the capture up.
    private func inputChanged() {
        guard let pipeline, pipeline.failure == nil else { return }
        _ = restartInput()
    }

    func restartInput() -> Bool {
        guard let pipeline, pipeline.failure == nil else { return false }
        input.stop()
        do { try startInput(); return true } catch { return false }
    }

    var failureMessage: String? { pipeline?.failure?.message }

    private func pipelineFailed(_ failure: CapturePipelineFailure) {
        guard pipeline != nil else { return }
        onFailure?(failure.message)
    }
}

#if DEBUG
/// QA input: plays a WAV file at real-time pace in place of the microphone, so
/// a simulator run exercises the same pipeline, parts, uploads and live stream
/// with known audio. Loops when the file ends. Absent from Release builds.
@MainActor
final class FileCaptureInput: CaptureInput {
    let usesMicrophone = false
    private let url: URL
    private let player: FilePlayer
    private(set) var isRunning = false

    init(url: URL) {
        self.url = url
        player = FilePlayer(url: url)
    }

    func start(deliver: @escaping @Sendable (AVAudioPCMBuffer) -> Void, changed: @escaping @MainActor () -> Void) throws {
        guard player.start(deliver) else { throw Recorder.RecorderError.couldNotStart }
        isRunning = true
    }

    func stop() {
        player.stop()
        isRunning = false
    }
}

/// Reads on its own queue; the position in the file survives pause and resume.
private final class FilePlayer: @unchecked Sendable {
    private let url: URL
    private let queue = DispatchQueue(label: AppIdentity.subsystem + ".capture.file-input")
    private var file: AVAudioFile?
    private var timer: DispatchSourceTimer?
    private var started = ContinuousClock.now
    private var delivered: Int64 = 0

    init(url: URL) { self.url = url }

    func start(_ deliver: @escaping @Sendable (AVAudioPCMBuffer) -> Void) -> Bool {
        queue.sync {
            if file == nil { file = try? AVAudioFile(forReading: url) }
            guard let file, file.length > 0 else { return false }
            started = .now
            delivered = 0
            let rate = file.processingFormat.sampleRate
            let timer = DispatchSource.makeTimerSource(queue: queue)
            timer.schedule(deadline: .now() + .milliseconds(50), repeating: .milliseconds(50), leeway: .milliseconds(5))
            timer.setEventHandler { [weak self] in
                guard let self else { return }
                // Deliver what real time has earned since the start, so the pace does not drift.
                let elapsed = ContinuousClock.now - started
                let due = Int64((Double(elapsed.components.seconds) + Double(elapsed.components.attoseconds) / 1e18) * rate) - delivered
                guard due > 0, let buffer = AVAudioPCMBuffer(pcmFormat: file.processingFormat, frameCapacity: AVAudioFrameCount(due)) else { return }
                if file.framePosition >= file.length { file.framePosition = 0 }
                guard (try? file.read(into: buffer, frameCount: AVAudioFrameCount(due))) != nil, buffer.frameLength > 0 else { return }
                delivered += Int64(buffer.frameLength)
                deliver(buffer)
            }
            self.timer = timer
            timer.resume()
            return true
        }
    }

    func stop() {
        queue.sync {
            timer?.cancel()
            timer = nil
        }
    }
}
#endif
