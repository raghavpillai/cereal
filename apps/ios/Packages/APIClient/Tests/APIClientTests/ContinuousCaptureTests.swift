import Foundation
import Testing
import Contracts
@testable import APIClient

/// A waveform with no silence and no repetition, so any range of it is
/// recognisably that range: a sine sweep with a slow amplitude envelope, the
/// same shape as a sustained voiced sound.
func speechLikeSamples(count: Int, startingAt first: Int = 0) -> [Int16] {
    (first..<(first + count)).map { index in
        let time = Double(index) / 16_000
        let pitch = 140 + 60 * sin(2 * .pi * 0.31 * time)
        let envelope = 0.55 + 0.35 * sin(2 * .pi * 2.7 * time)
        let voice = sin(2 * .pi * pitch * time) + 0.4 * sin(2 * .pi * pitch * 2 * time) + 0.2 * sin(2 * .pi * pitch * 3.01 * time)
        return Int16(max(-1, min(1, voice * envelope / 1.6)) * 30_000)
    }
}

func bytes(_ samples: [Int16]) -> Data {
    samples.withUnsafeBufferPointer { Data(buffer: $0) }
}

func temporaryCaptureDirectory() -> URL {
    let root = FileManager.default.temporaryDirectory.appendingPathComponent("cereal-capture-test-\(UUID().uuidString)")
    try! FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
    return root
}

@Suite struct ContinuousCaptureTests {
    @Test func writerAppendsSamplesAndCountsThemExactly() throws {
        let root = temporaryCaptureDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let url = root.appendingPathComponent("capture-0.wav")
        let writer = try CaptureFileWriter(url: url)
        let samples = speechLikeSamples(count: 40_000)
        var written = 0
        // Uneven buffers, as a converter produces.
        for size in [1365, 1366, 1365, 4096, 7, 30_000, 1, 1800] where written < samples.count {
            let slice = Array(samples[written..<min(samples.count, written + size)])
            try slice.withUnsafeBytes { try writer.append($0) }
            written += slice.count
            #expect(writer.frames == Int64(written))
            // The file's size is the sample count at every step, before any header refresh.
            #expect(ContinuousCapture.frames(inFile: url) == written)
        }
        try writer.close()
        let file = try Data(contentsOf: url)
        #expect(file.count == 44 + written * 2)
        #expect(file.prefix(44) == ContinuousCapture.header(dataBytes: written * 2))
        #expect(file.dropFirst(44) == bytes(Array(samples.prefix(written))))
    }

    @Test func headerIsCurrentAfterEachRefreshAndValidBetweenThem() throws {
        let root = temporaryCaptureDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let url = root.appendingPathComponent("capture-0.wav")
        let writer = try CaptureFileWriter(url: url)
        func headerDataBytes() throws -> Int {
            let header = try Data(contentsOf: url).prefix(44)
            return Int(header[40]) | Int(header[41]) << 8 | Int(header[42]) << 16 | Int(header[43]) << 24
        }
        #expect(try headerDataBytes() == 0)
        try speechLikeSamples(count: 16_000).withUnsafeBytes { try writer.append($0) }
        try writer.refreshHeader()
        #expect(try headerDataBytes() == 32_000)
        #expect(writer.headerFrames == 16_000)
        try speechLikeSamples(count: 8_000).withUnsafeBytes { try writer.append($0) }
        // Between refreshes the header lags, never leads, and still describes real audio.
        #expect(try headerDataBytes() == 32_000)
        #expect(ContinuousCapture.frames(inFile: url) == 24_000)
        try writer.close()
        #expect(try headerDataBytes() == 48_000)
    }

    @Test func aCaptureCutShortByAKillIsMeasuredFromItsSizeNotItsHeader() throws {
        let root = temporaryCaptureDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let url = root.appendingPathComponent("capture-0.wav")
        let samples = speechLikeSamples(count: 50_000)
        do {
            let writer = try CaptureFileWriter(url: url)
            try Array(samples.prefix(32_000)).withUnsafeBytes { try writer.append($0) }
            try writer.refreshHeader()
            try Array(samples.suffix(18_000)).withUnsafeBytes { try writer.append($0) }
            // The process dies here: no close, no final header.
        }
        // A write that was cut in the middle of a sample leaves one stray byte.
        let handle = try FileHandle(forWritingTo: url)
        try handle.seekToEnd(); try handle.write(contentsOf: Data([0x7f])); try handle.close()
        let header = try Data(contentsOf: url).prefix(44)
        #expect(Int(header[40]) | Int(header[41]) << 8 | Int(header[42]) << 16 == 64_000) // 32 000 samples, stale
        #expect(ContinuousCapture.frames(inFile: url) == 50_000)
        let reader = try CaptureFileReader(url: url)
        #expect(reader.availableFrames() == 50_000)
        #expect(try reader.read(from: 0, maxFrames: 60_000) == bytes(samples))
    }

    @Test func aNewCaptureNeverReplacesAnExistingFile() throws {
        let root = temporaryCaptureDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let url = root.appendingPathComponent("capture-0.wav")
        try Data("unfinished capture".utf8).write(to: url)
        #expect(throws: CaptureFileError.self) { try CaptureFileWriter(url: url) }
        #expect(try Data(contentsOf: url) == Data("unfinished capture".utf8))
    }

    @Test func otherAudioFilesAreNotMeasuredAsCaptures() throws {
        let root = temporaryCaptureDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let text = root.appendingPathComponent("capture-1.wav")
        try Data(repeating: 7, count: 400).write(to: text)
        #expect(ContinuousCapture.frames(inFile: text) == nil)
        // A recorder's WAV with a chunk between the format and the audio.
        var other = Data("RIFF".utf8)
        other.append(contentsOf: [0, 0, 0, 0]); other.append(Data("WAVEFLLR".utf8)); other.append(Data(repeating: 0, count: 200))
        let recorded = root.appendingPathComponent("segment-0.wav")
        try other.write(to: recorded)
        #expect(ContinuousCapture.frames(inFile: recorded) == nil)
        #expect(throws: CaptureFileError.notACapture) { try CaptureFileReader(url: recorded) }
        #expect(ContinuousCapture.frames(inFile: root.appendingPathComponent("missing.wav")) == nil)
        #expect(ContinuousCapture.isCaptureName("capture-3.wav"))
        #expect(!ContinuousCapture.isCaptureName("segment-3.wav"))
    }

    @Test func readerFollowsAGrowingCaptureBySampleIndex() throws {
        let root = temporaryCaptureDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let url = root.appendingPathComponent("capture-0.wav")
        let writer = try CaptureFileWriter(url: url)
        let reader = try CaptureFileReader(url: url)
        #expect(reader.availableFrames() == 0)
        #expect(try reader.read(from: 0, maxFrames: 3200).isEmpty)
        let samples = speechLikeSamples(count: 10_000)
        try Array(samples.prefix(5_000)).withUnsafeBytes { try writer.append($0) }
        #expect(reader.availableFrames() == 5_000)
        #expect(try reader.read(from: 1_000, maxFrames: 3_200) == bytes(Array(samples[1_000..<4_200])))
        // A read near the end is shorter, never padded.
        #expect(try reader.read(from: 4_000, maxFrames: 3_200) == bytes(Array(samples[4_000..<5_000])))
        try Array(samples.suffix(5_000)).withUnsafeBytes { try writer.append($0) }
        #expect(reader.availableFrames() == 10_000)
        #expect(try reader.read(from: 4_000, maxFrames: 3_200) == bytes(Array(samples[4_000..<7_200])))
        #expect(try reader.read(from: 10_000, maxFrames: 100).isEmpty)
    }
}
