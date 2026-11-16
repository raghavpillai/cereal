import Foundation

/// One continuous microphone capture on disk: a fixed 44-byte WAV header
/// followed by 16 kHz mono PCM16 little-endian samples and nothing else.
///
/// The file is the clock of a recording. A sample's index is its position on
/// the capture timeline, and the live stream reads it through a cursor, so
/// the file is also the stream's buffer for a reconnect. Because no chunk ever follows
/// the audio, the number of samples is given by the file's size; the header is
/// kept current for other readers but is never the authority.
public enum ContinuousCapture {
    public static let sampleRate = 16_000
    public static let bytesPerFrame = 2
    public static let headerBytes = 44
    public static let filePrefix = "capture-"

    public static func isCaptureName(_ name: String) -> Bool {
        name.hasPrefix(filePrefix) && name.lowercased().hasSuffix(".wav")
    }

    public static func header(dataBytes: Int) -> Data {
        let size = UInt32(clamping: dataBytes)
        var header = Data("RIFF".utf8)
        header.appendLittleEndian(UInt32(clamping: 36 + dataBytes))
        header.append(Data("WAVEfmt ".utf8))
        header.appendLittleEndian(UInt32(16))
        header.appendLittleEndian(UInt16(1)); header.appendLittleEndian(UInt16(1))
        header.appendLittleEndian(UInt32(sampleRate)); header.appendLittleEndian(UInt32(sampleRate * bytesPerFrame))
        header.appendLittleEndian(UInt16(bytesPerFrame)); header.appendLittleEndian(UInt16(16))
        header.append(Data("data".utf8)); header.appendLittleEndian(size)
        return header
    }

    /// Whole samples held by a capture file, from its size. Nil unless the file
    /// is a regular file that starts with this format's header, so a file from
    /// another writer is never measured this way. A trailing odd byte from a
    /// write that was cut short is not a sample.
    public static func frames(inFile url: URL) -> Int? {
        // `lstat`, not URL resource values: those are cached on the URL and go
        // stale while the capture grows.
        var status = stat()
        guard lstat(url.path, &status) == 0, status.st_mode & S_IFMT == S_IFREG else { return nil }
        let size = Int(status.st_size)
        guard size >= headerBytes, let handle = try? FileHandle(forReadingFrom: url) else { return nil }
        defer { try? handle.close() }
        guard let found = try? handle.read(upToCount: headerBytes), found.count == headerBytes,
              hasFormat(found) else { return nil }
        return (size - headerBytes) / bytesPerFrame
    }

    /// True when the first 44 bytes describe this format. The two size fields
    /// are ignored: they lag the audio by up to a second while recording.
    static func hasFormat(_ header: Data) -> Bool {
        let expected = Self.header(dataBytes: 0)
        guard header.count >= headerBytes else { return false }
        let found = Data(header.prefix(headerBytes))
        return found[0..<4] == expected[0..<4] && found[8..<40] == expected[8..<40]
    }
}

public enum CaptureFileError: Error, Equatable, Sendable {
    case create(Int32)
    case write(Int32)
    case read(Int32)
    case notACapture
}

/// Appends samples to a new capture file. One owner at a time; the capture
/// pipeline serializes every call.
public final class CaptureFileWriter {
    public let url: URL
    public private(set) var frames: Int64 = 0
    /// Samples covered by the header on disk.
    public private(set) var headerFrames: Int64 = 0
    private var descriptor: Int32

    /// Creates the file and writes an empty header. Never opens an existing
    /// file: a capture left by a terminated process must not be overwritten.
    public init(url: URL) throws {
        self.url = url
        descriptor = open(url.path, O_WRONLY | O_CREAT | O_EXCL, 0o600)
        guard descriptor >= 0 else { throw CaptureFileError.create(errno) }
        do { try write(ContinuousCapture.header(dataBytes: 0), at: 0) }
        catch { Darwin.close(descriptor); descriptor = -1; throw error }
    }

    deinit { if descriptor >= 0 { Darwin.close(descriptor) } }

    /// Appends whole PCM16 samples. The count advances only after the bytes
    /// are in the file, so a reader that trusts `frames` never reads ahead.
    public func append(_ samples: UnsafeRawBufferPointer) throws {
        let whole = samples.count - samples.count % ContinuousCapture.bytesPerFrame
        guard whole > 0, let base = samples.baseAddress else { return }
        try write(base, count: whole, at: Int64(ContinuousCapture.headerBytes) + frames * Int64(ContinuousCapture.bytesPerFrame))
        frames += Int64(whole / ContinuousCapture.bytesPerFrame)
    }

    /// Rewrites the two size fields so the header describes every sample written.
    public func refreshHeader() throws {
        guard headerFrames != frames else { return }
        let bytes = Int(frames) * ContinuousCapture.bytesPerFrame
        var riff = UInt32(clamping: 36 + bytes).littleEndian
        var data = UInt32(clamping: bytes).littleEndian
        try withUnsafeBytes(of: &riff) { try write($0.baseAddress!, count: 4, at: 4) }
        try withUnsafeBytes(of: &data) { try write($0.baseAddress!, count: 4, at: 40) }
        headerFrames = frames
    }

    /// Final header, pushed to storage, then closed. Safe to call twice.
    public func close() throws {
        guard descriptor >= 0 else { return }
        defer { Darwin.close(descriptor); descriptor = -1 }
        try refreshHeader()
        _ = fsync(descriptor)
    }

    private func write(_ data: Data, at offset: Int64) throws {
        try data.withUnsafeBytes { try write($0.baseAddress!, count: $0.count, at: offset) }
    }

    private func write(_ base: UnsafeRawPointer, count: Int, at offset: Int64) throws {
        guard descriptor >= 0 else { throw CaptureFileError.write(EBADF) }
        var done = 0
        while done < count {
            let written = pwrite(descriptor, base + done, count - done, off_t(offset) + off_t(done))
            if written < 0 {
                if errno == EINTR { continue }
                throw CaptureFileError.write(errno)
            }
            done += written
        }
    }
}

/// Reads samples by index from a capture that may still be growing. Reads use
/// explicit offsets, so one reader can be shared across tasks.
public final class CaptureFileReader: @unchecked Sendable {
    private let descriptor: Int32

    public init(url: URL) throws {
        guard ContinuousCapture.frames(inFile: url) != nil else { throw CaptureFileError.notACapture }
        descriptor = open(url.path, O_RDONLY)
        guard descriptor >= 0 else { throw CaptureFileError.read(errno) }
    }

    deinit { Darwin.close(descriptor) }

    /// Whole samples in the file now.
    public func availableFrames() -> Int64 {
        var status = stat()
        guard fstat(descriptor, &status) == 0 else { return 0 }
        return max(0, (Int64(status.st_size) - Int64(ContinuousCapture.headerBytes)) / Int64(ContinuousCapture.bytesPerFrame))
    }

    /// Up to `maxFrames` samples starting at `frame`, as PCM16 little-endian
    /// bytes. Shorter, or empty, when the file ends first.
    public func read(from frame: Int64, maxFrames: Int) throws -> Data {
        let wanted = Int(min(Int64(max(0, maxFrames)), max(0, availableFrames() - frame)))
        guard wanted > 0, frame >= 0 else { return Data() }
        var data = Data(count: wanted * ContinuousCapture.bytesPerFrame)
        let offset = off_t(ContinuousCapture.headerBytes) + off_t(frame) * off_t(ContinuousCapture.bytesPerFrame)
        let count = try data.withUnsafeMutableBytes { buffer -> Int in
            var done = 0
            while done < buffer.count {
                let got = pread(descriptor, buffer.baseAddress! + done, buffer.count - done, offset + off_t(done))
                if got < 0 {
                    if errno == EINTR { continue }
                    throw CaptureFileError.read(errno)
                }
                if got == 0 { break }
                done += got
            }
            return done
        }
        return data.prefix(count - count % ContinuousCapture.bytesPerFrame)
    }
}

extension Data {
    mutating func appendLittleEndian<T: FixedWidthInteger>(_ value: T) {
        var little = value.littleEndian
        Swift.withUnsafeBytes(of: &little) { append(contentsOf: $0) }
    }
}
