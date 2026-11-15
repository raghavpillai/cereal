import AVFoundation
import APIClient

enum CapturePipelineFailure: Error, Equatable, Sendable {
    /// A write to the capture file failed (most often a full disk).
    case write
    /// Free space fell below the reserve needed to finish and deliver the capture.
    case lowDisk
    case conversion

    var message: String {
        switch self {
        case .write, .conversion: "Audio could not be saved completely. Any captured audio has been kept."
        case .lowDisk: "Storage is almost full. Recording stopped and your audio has been kept."
        }
    }
}

/// From input buffers to the capture file: whatever format the input hardware
/// delivers is converted to 16 kHz mono PCM16 and appended to one continuous
/// file.
///
/// The number of samples in the file is the position on the recording's
/// timeline, so it is counted exactly: the count advances by what was written,
/// after it was written, and nothing else moves it. When the input format
/// changes in the middle of a capture (a Bluetooth route with another sample
/// rate), the old converter is drained into the file before a new one is
/// built, so no sample is lost or counted twice and the position carries on.
///
/// Buffers arrive on the audio engine's tap thread while pause and stop come
/// from the main actor; one lock serializes them. The work per buffer is one
/// conversion and one small write.
final class CapturePipeline: @unchecked Sendable {
    struct Options: Sendable {
        /// Report each time the capture grows past another multiple of this many samples.
        var partFrames: Int64?
        /// The header on disk is at most this many samples behind the audio.
        var headerRefreshFrames: Int64 = Int64(ContinuousCapture.sampleRate)
        var freeSpaceCheckFrames: Int64 = Int64(ContinuousCapture.sampleRate) * 10
        /// Room kept for the capture's last part and the delivery journal.
        var minimumFreeBytes: Int64 = 32 << 20
        var freeBytes: @Sendable (URL) -> Int64? = CapturePipeline.volumeFreeBytes
    }

    static let outputFormat = AVAudioFormat(commonFormat: .pcmFormatInt16, sampleRate: Double(ContinuousCapture.sampleRate), channels: 1, interleaved: true)!

    private let lock = NSLock()
    private let writer: CaptureFileWriter
    private let options: Options
    private let onPartBoundary: (@Sendable (Int64) -> Void)?
    private let onFailure: (@Sendable (CapturePipelineFailure) -> Void)?
    private var converter: AVAudioConverter?
    private var inputFormat: AVAudioFormat?
    private var output: AVAudioPCMBuffer?
    private var written: Int64 = 0
    private var boundaries: Int64 = 0
    private var nextFreeSpaceCheck: Int64
    private var meter: Float = 0.12
    private var failed: CapturePipelineFailure?
    private var finished = false
    private var builds = 0

    init(url: URL, options: Options = Options(),
         onPartBoundary: (@Sendable (Int64) -> Void)? = nil,
         onFailure: (@Sendable (CapturePipelineFailure) -> Void)? = nil) throws {
        writer = try CaptureFileWriter(url: url)
        self.options = options
        self.onPartBoundary = onPartBoundary
        self.onFailure = onFailure
        nextFreeSpaceCheck = 0
    }

    /// Samples in the capture file.
    var frames: Int64 { lock.withLock { written } }
    /// Input level of the latest buffer, 0.12 (rest) to 1.
    var level: Float { lock.withLock { meter } }
    var failure: CapturePipelineFailure? { lock.withLock { failed } }
    /// Converters built so far: one per input format the capture has seen.
    var converterBuilds: Int { lock.withLock { builds } }

    /// Converts and appends one input buffer. Safe to call from the tap thread.
    func append(_ buffer: AVAudioPCMBuffer) {
        var crossed: Int64?
        var problem: CapturePipelineFailure?
        lock.lock()
        if !finished, failed == nil, buffer.frameLength > 0 {
            do {
                if inputFormat == nil || buffer.format != inputFormat {
                    // A new input format: finish the old converter's audio first.
                    try drainLocked()
                    try buildConverterLocked(for: buffer.format)
                }
                try convertLocked(buffer)
                try housekeepingLocked()
                if let partFrames = options.partFrames, partFrames > 0, written / partFrames > boundaries {
                    boundaries = written / partFrames
                    crossed = written
                }
            } catch let failure as CapturePipelineFailure {
                failed = failure; problem = failure
            } catch {
                failed = .write; problem = .write
            }
        }
        lock.unlock()
        if let crossed { onPartBoundary?(crossed) }
        if let problem { onFailure?(problem) }
    }

    /// Writes everything the converter still holds and brings the header up to
    /// date. The capture stops growing until the next buffer. Used for pause.
    func flush() {
        lock.lock(); defer { lock.unlock() }
        guard !finished else { return }
        try? drainLocked()
        try? writer.refreshHeader()
    }

    /// Drains, writes the final header and closes the file. Returns the sample count.
    @discardableResult
    func finish() -> Int64 {
        lock.lock(); defer { lock.unlock() }
        guard !finished else { return written }
        finished = true
        try? drainLocked()
        try? writer.close()
        return written
    }

    // MARK: Locked

    private func buildConverterLocked(for format: AVAudioFormat) throws {
        guard format.sampleRate > 0, format.channelCount > 0,
              let converter = AVAudioConverter(from: format, to: Self.outputFormat),
              let output = AVAudioPCMBuffer(pcmFormat: Self.outputFormat, frameCapacity: AVAudioFrameCount(ContinuousCapture.sampleRate)) else {
            throw CapturePipelineFailure.conversion
        }
        converter.sampleRateConverterQuality = AVAudioQuality.high.rawValue
        self.converter = converter
        self.output = output
        inputFormat = format
        builds += 1
    }

    /// Feeds `buffer` to the converter and writes all the output it gives.
    /// With no buffer, tells the converter the stream has ended so that it
    /// releases the samples inside its filter.
    private func convertLocked(_ buffer: AVAudioPCMBuffer?) throws {
        guard let converter, let output else { return }
        let feed = InputFeed(buffer)
        while true {
            output.frameLength = 0
            var error: NSError?
            let status = converter.convert(to: output, error: &error) { _, inputStatus in
                if let next = feed.take() {
                    inputStatus.pointee = .haveData
                    return next
                }
                inputStatus.pointee = feed.endsStream ? .endOfStream : .noDataNow
                return nil
            }
            if status == .error { throw CapturePipelineFailure.conversion }
            if output.frameLength > 0 { try writeLocked(output) }
            // `haveData` means the output buffer filled and more may follow.
            if status != .haveData || output.frameLength == 0 { break }
        }
    }

    private func drainLocked() throws {
        guard converter != nil else { return }
        defer { converter = nil; output = nil; inputFormat = nil }
        try convertLocked(nil)
    }

    private func writeLocked(_ buffer: AVAudioPCMBuffer) throws {
        guard let samples = buffer.int16ChannelData?[0] else { throw CapturePipelineFailure.conversion }
        let count = Int(buffer.frameLength)
        do { try writer.append(UnsafeRawBufferPointer(start: samples, count: count * ContinuousCapture.bytesPerFrame)) }
        catch { throw CapturePipelineFailure.write }
        written = writer.frames
        PerfProbe.hit("Capture.Write")
        // Mean power of the converted audio, on the scale the level meter has always used.
        var sum: Float = 0
        for index in 0..<count { let value = Float(samples[index]) / 32_768; sum += value * value }
        let power = 10 * log10f(max(sum / Float(max(1, count)), 1e-12))
        meter = max(0.12, min(1, powf(10, power / 35)))
    }

    private func housekeepingLocked() throws {
        if written - writer.headerFrames >= options.headerRefreshFrames {
            do { try writer.refreshHeader() } catch { throw CapturePipelineFailure.write }
            PerfProbe.hit("Capture.Header")
        }
        if written >= nextFreeSpaceCheck {
            nextFreeSpaceCheck = written + options.freeSpaceCheckFrames
            if let free = options.freeBytes(writer.url), free < options.minimumFreeBytes { throw CapturePipelineFailure.lowDisk }
        }
    }

    static let volumeFreeBytes: @Sendable (URL) -> Int64? = { url in
        // A fresh URL: resource values are cached on the instance.
        let directory = URL(fileURLWithPath: url.deletingLastPathComponent().path)
        return (try? directory.resourceValues(forKeys: [.volumeAvailableCapacityForImportantUsageKey]))?.volumeAvailableCapacityForImportantUsage
    }
}

/// Hands one buffer to the converter's input block, once.
private final class InputFeed: @unchecked Sendable {
    private var buffer: AVAudioPCMBuffer?
    let endsStream: Bool
    init(_ buffer: AVAudioPCMBuffer?) { self.buffer = buffer; endsStream = buffer == nil }
    func take() -> AVAudioPCMBuffer? {
        defer { buffer = nil }
        return buffer
    }
}
