import Foundation

/// What `Recorder` needs from an audio implementation: one file per capture,
/// written from Start to Stop. `Recorder` owns everything else (state, the
/// clock, the audio session, the Live Activity, the recovery journal), so the
/// two implementations share no code and none of each other's assumptions.
@MainActor
protocol CaptureBackend: AnyObject {
    /// False only for audio that does not come from the device's input (QA playback of a file).
    var usesMicrophone: Bool { get }
    /// Called once when capture stops by itself: a write that failed, an input that went away.
    var onFailure: ((String) -> Void)? { get set }
    func start(writingTo url: URL) throws
    func pause()
    func resume() throws
    /// Finalizes the file. Returns the audio it holds, in seconds.
    func stop() -> TimeInterval?
    var isRunning: Bool { get }
    /// Audio captured so far, in seconds.
    var capturedTime: TimeInterval { get }
    /// Input level for the meter, 0.12 (rest) to 1.
    func level() -> Float
}

/// A backend that appends to one continuous capture file whose sample count is
/// exact at every moment. A live stream reads it by sample index, and the file
/// is what the stream replays from after a reconnect.
@MainActor
protocol ContinuousCaptureBackend: CaptureBackend {
    /// Samples (16 kHz) in the capture file now.
    var capturedFrames: Int64 { get }
    /// Starts the input again on whatever route is current, after the one it
    /// was using went away. The capture file and its sample position are
    /// untouched. False when there is no input left, or the capture has failed.
    func restartInput() -> Bool
    /// Why the capture can no longer be written, if that is why it stopped.
    var failureMessage: String? { get }
}

/// What a change of audio route does to a capture in progress.
///
/// The recorder path ends the capture when the input in use goes away, as it
/// always has. The engine carries on with the built-in microphone: a headset
/// leaving is a change of input, not the end of the meeting. Both end when no
/// input is left at all.
enum RouteChangePolicy {
    static func endsCapture(inputDeviceRemoved: Bool, noInputAvailable: Bool, continuous: Bool) -> Bool {
        noInputAvailable || (inputDeviceRemoved && !continuous)
    }
}

/// The one switch between the two capture implementations.
///
/// The default is the engine: a single input tap that writes one continuous
/// capture. The previous `AVAudioRecorder` path stays selectable for the local
/// demo, in case a device shows a regression:
///
///     defaults key   capture.legacyRecorder = YES   (app domain dev.cereal.mobile)
///     launch args    -capture.legacyRecorder YES
///
/// The choice is read when a capture starts and applies to that capture only.
/// A recording to a server always uses the engine: its only transcript is the
/// live stream, which reads a continuous capture.
enum CaptureEnginePolicy {
    static let legacyRecorderKey = "capture.legacyRecorder"

    static func usesLegacyRecorder(defaults: UserDefaults = .standard) -> Bool {
        defaults.bool(forKey: legacyRecorderKey)
    }

    #if DEBUG
    /// QA only: `-capture.inputFile <path to a WAV>` records that file, played
    /// at real-time pace through the same pipeline, in place of the microphone.
    static func injectedAudioFile(defaults: UserDefaults = .standard) -> URL? {
        guard let path = defaults.string(forKey: "capture.inputFile"), !path.isEmpty,
              FileManager.default.isReadableFile(atPath: path) else { return nil }
        return URL(fileURLWithPath: path)
    }
    #endif

    @MainActor
    static func makeBackend() -> any CaptureBackend {
        #if DEBUG
        if injectedAudioFile() != nil { return makeContinuousBackend() }
        #endif
        return usesLegacyRecorder() ? LegacyRecorderBackend() : makeContinuousBackend()
    }

    /// The engine, whatever the legacy key says: for a recording to a server.
    @MainActor
    static func makeContinuousBackend() -> any ContinuousCaptureBackend {
        #if DEBUG
        if let file = injectedAudioFile() { return EngineCaptureBackend(input: FileCaptureInput(url: file)) }
        #endif
        return EngineCaptureBackend()
    }
}
