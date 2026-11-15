import AVFoundation

/// The previous capture path, unchanged in behaviour: `AVAudioRecorder` writes
/// 16 kHz mono PCM16 to one WAV per segment, and the owner of a long recording
/// stops and restarts it to produce parts. Whatever is said during a restart
/// is not recorded, which is why the engine replaced it. Kept for one release
/// behind `CaptureEnginePolicy`.
@MainActor
final class LegacyRecorderBackend: NSObject, CaptureBackend, AVAudioRecorderDelegate {
    let usesMicrophone = true
    var onFailure: ((String) -> Void)?
    private var recorder: AVAudioRecorder?
    private var url: URL?
    private var lastDeviceDuration: TimeInterval = 0

    func start(writingTo url: URL) throws {
        let audio = try AVAudioRecorder(url: url, settings: [
            AVFormatIDKey: kAudioFormatLinearPCM,
            AVSampleRateKey: 16_000,
            AVNumberOfChannelsKey: 1,
            AVLinearPCMBitDepthKey: 16,
            AVLinearPCMIsFloatKey: false,
        ])
        audio.isMeteringEnabled = true
        audio.delegate = self
        guard audio.prepareToRecord(), audio.record() else { throw Recorder.RecorderError.couldNotStart }
        recorder = audio
        self.url = url
        lastDeviceDuration = 0
    }

    func pause() {
        recorder?.pause()
        lastDeviceDuration = max(lastDeviceDuration, recorder?.currentTime ?? 0)
    }

    func resume() throws {
        guard recorder?.record() == true else { throw Recorder.RecorderError.couldNotStart }
    }

    func stop() -> TimeInterval? {
        guard let recorder else { return nil }
        let measured = max(lastDeviceDuration, recorder.currentTime)
        recorder.delegate = nil
        recorder.stop()
        self.recorder = nil
        // Delegate completion can reset currentTime. The finalized WAV is authoritative when
        // readable, with the most recent device position retained for partial-write failures.
        if let url, let audio = try? AVAudioFile(forReading: url), audio.processingFormat.sampleRate > 0 {
            return Double(audio.length) / audio.processingFormat.sampleRate
        }
        return measured
    }

    var isRunning: Bool { recorder?.isRecording ?? false }

    var capturedTime: TimeInterval {
        let time = recorder?.currentTime ?? 0
        lastDeviceDuration = max(lastDeviceDuration, time)
        return time
    }

    func level() -> Float {
        guard let recorder else { return 0.12 }
        recorder.updateMeters()
        return max(0.12, min(1, powf(10, recorder.averagePower(forChannel: 0) / 35)))
    }

    nonisolated func audioRecorderDidFinishRecording(_ recorder: AVAudioRecorder, successfully flag: Bool) {
        let identifier = ObjectIdentifier(recorder)
        Task { @MainActor [weak self] in
            guard let self, let current = self.recorder, ObjectIdentifier(current) == identifier else { return }
            onFailure?(flag ? "Recording has ended. Tap to resume." : "Recording failed. Any captured audio has been kept.")
        }
    }

    nonisolated func audioRecorderEncodeErrorDidOccur(_ recorder: AVAudioRecorder, error: (any Error)?) {
        let identifier = ObjectIdentifier(recorder)
        Task { @MainActor [weak self] in
            guard let self, let current = self.recorder, ObjectIdentifier(current) == identifier else { return }
            onFailure?("Audio could not be saved completely. Any captured audio has been kept.")
        }
    }
}
