import Foundation
import Testing
import Contracts
@testable import APIClient

/// A waveform with no silence and no repetition at part length, so a part
/// boundary always falls inside continuous sound: a sine sweep with a slow
/// amplitude envelope, the same shape as a sustained voiced sound.
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

/// The proof that cutting parts from one continuous capture loses nothing:
/// every part is a byte range of the capture and the ranges tile it exactly.
@MainActor @Suite struct ContinuousPartCuttingTests {
    private let principal = "00000000-0000-4000-8000-000000000001"
    private func outbox(_ root: URL) throws -> AudioOutbox {
        try AudioOutbox(instance: InstanceURL("https://example.test"), principalID: principal, root: root.appendingPathComponent("outbox"))
    }
    private func audio(of entry: AudioOutboxEntry, in outbox: AudioOutbox) throws -> Data {
        let data = try outbox.audioData(for: entry)
        #expect(data.prefix(44) == ContinuousCapture.header(dataBytes: data.count - 44))
        return data.dropFirst(44)
    }

    @Test func partsCutWhileRecordingJoinToTheCaptureByteForByte() throws {
        let root = temporaryCaptureDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let queue = try outbox(root)
        let noteID = UUID().uuidString.lowercased()
        let url = root.appendingPathComponent("capture-0.wav")
        let writer = try CaptureFileWriter(url: url)
        // 100.4 s of continuous sound with 30-second parts: three boundaries, each inside the sound.
        let partFrames = 30 * 16_000
        let total = 100 * 16_000 + 6_400
        let samples = speechLikeSamples(count: total)
        var written = 0
        var staged: [AudioOutboxEntry] = []
        var stagedAtFrames: [Int] = []
        while written < total {
            // Buffers the size a 48 kHz input tap produces after conversion.
            let size = min(1365 + written % 2, total - written)
            try Array(samples[written..<(written + size)]).withUnsafeBytes { try writer.append($0) }
            let before = written / partFrames
            written += size
            if written / partFrames > before {
                // A boundary was crossed: cut while the capture keeps growing.
                let new = try queue.stageParts(noteID: noteID, capture: url, availableFrames: written, partFrames: partFrames,
                                               isFinal: false, startMs: 754_000, sequence: queue.nextSequence(noteID: noteID))
                #expect(new.count == 1)
                staged += new
                stagedAtFrames.append(written)
            }
        }
        try writer.close()
        staged += try queue.stageParts(noteID: noteID, capture: url, availableFrames: written, partFrames: partFrames,
                                       isFinal: true, startMs: 754_000, sequence: queue.nextSequence(noteID: noteID))

        #expect(staged.count == 4)
        #expect(staged.map(\.startMs) == [754_000, 784_000, 814_000, 844_000])
        #expect(staged.map(\.durationMs) == [30_000, 30_000, 30_000, 10_400])
        #expect(staged.map(\.sequence) == [0, 1, 2, 3])
        #expect(Set(staged.map(\.id)).count == 4)
        // Each part was cut as soon as its last sample was on disk.
        #expect(stagedAtFrames.enumerated().allSatisfy { $1 >= ($0 + 1) * partFrames && $1 < ($0 + 1) * partFrames + 1367 })

        let capture = try Data(contentsOf: url).dropFirst(44)
        var joined = Data()
        for entry in staged { joined.append(try audio(of: entry, in: queue)) }
        #expect(joined.count == total * 2)
        #expect(joined == capture)
        #expect(joined == bytes(samples))

        // At each boundary the last sample of one part and the first of the next are
        // neighbours in the original signal: nothing dropped, nothing repeated.
        for index in 0..<3 {
            let end = try audio(of: staged[index], in: queue).suffix(2)
            let start = try audio(of: staged[index + 1], in: queue).prefix(2)
            let boundary = (index + 1) * partFrames
            #expect(end == bytes([samples[boundary - 1]]))
            #expect(start == bytes([samples[boundary]]))
            // The signal is loud here, so a hidden gap could not pass as silence.
            #expect(abs(Int(samples[boundary])) + abs(Int(samples[boundary - 1])) > 0)
        }
    }

    @Test func aGrowingCaptureYieldsOnlyWholePartsAndTheRemainderOnlyAtTheEnd() throws {
        let root = temporaryCaptureDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let queue = try outbox(root)
        let noteID = UUID().uuidString.lowercased()
        let url = root.appendingPathComponent("capture-0.wav")
        let writer = try CaptureFileWriter(url: url)
        let part = 16_000
        try speechLikeSamples(count: 15_999).withUnsafeBytes { try writer.append($0) }
        #expect(try queue.stageParts(noteID: noteID, capture: url, availableFrames: 15_999, partFrames: part, isFinal: false, startMs: 0, sequence: 0).isEmpty)
        try speechLikeSamples(count: 1, startingAt: 15_999).withUnsafeBytes { try writer.append($0) }
        #expect(try queue.stageParts(noteID: noteID, capture: url, availableFrames: 16_000, partFrames: part, isFinal: false, startMs: 0, sequence: 0).count == 1)
        // Asking again changes nothing and writes nothing.
        let writes = queue.journalWrites
        #expect(try queue.stageParts(noteID: noteID, capture: url, availableFrames: 16_000, partFrames: part, isFinal: false, startMs: 0, sequence: 1).isEmpty)
        #expect(queue.journalWrites == writes)
        // A caller cannot cut past what is on disk.
        #expect(try queue.stageParts(noteID: noteID, capture: url, availableFrames: 64_000, partFrames: part, isFinal: false, startMs: 0, sequence: 1).isEmpty)
        try speechLikeSamples(count: 20_000, startingAt: 16_000).withUnsafeBytes { try writer.append($0) }
        try writer.close()
        let rest = try queue.stageParts(noteID: noteID, capture: url, availableFrames: 36_000, partFrames: part, isFinal: true, startMs: 0, sequence: 1)
        #expect(rest.map(\.startMs) == [1_000, 2_000])
        #expect(rest.map(\.durationMs) == [1_000, 250])
        #expect(queue.entries(noteID: noteID).count == 3)
        #expect(queue.endMs(noteID: noteID) == 2_250)
        // A capture that ends exactly on a boundary has no empty last part.
        let exact = root.appendingPathComponent("capture-1.wav")
        let second = try CaptureFileWriter(url: exact)
        try speechLikeSamples(count: 32_000).withUnsafeBytes { try second.append($0) }
        try second.close()
        #expect(try queue.stageParts(noteID: noteID, capture: exact, availableFrames: 32_000, partFrames: part, isFinal: true, startMs: 5_000, sequence: 3).map(\.durationMs) == [1_000, 1_000])
    }

    @Test func cuttingAgainAfterARelaunchRecognisesEveryPartAlreadyStagedOrUploaded() throws {
        let root = temporaryCaptureDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let noteID = UUID().uuidString.lowercased()
        let url = root.appendingPathComponent("capture-2.wav")
        let writer = try CaptureFileWriter(url: url)
        let samples = speechLikeSamples(count: 56_000)
        try Array(samples.prefix(40_000)).withUnsafeBytes { try writer.append($0) }
        let first = try outbox(root)
        let early = try first.stageParts(noteID: noteID, capture: url, availableFrames: 40_000, partFrames: 16_000, isFinal: false, startMs: 9_000, sequence: 4)
        #expect(early.map(\.sequence) == [4, 5])
        try first.update(id: early[0].id, state: .ready)
        try first.update(id: early[1].id, state: .acknowledged)
        try Array(samples.suffix(16_000)).withUnsafeBytes { try writer.append($0) }
        // The app is killed: the header still says 0 samples and nothing was finalised.

        let relaunched = try outbox(root)
        let frames = try #require(ContinuousCapture.frames(inFile: url))
        #expect(frames == 56_000)
        let late = try relaunched.stageParts(noteID: noteID, capture: url, availableFrames: frames, partFrames: 16_000,
                                             isFinal: true, startMs: 9_000, sequence: relaunched.nextSequence(noteID: noteID))
        // Only what was missing is new; identities, offsets and delivery states of the rest are untouched.
        #expect(late.map(\.startMs) == [11_000, 12_000])
        #expect(late.map(\.durationMs) == [1_000, 500])
        #expect(late.map(\.sequence) == [6, 7])
        let all = relaunched.entries(noteID: noteID).sorted { $0.startMs < $1.startMs }
        #expect(all.map(\.id).prefix(2) == early.map(\.id)[...])
        #expect(all.map(\.state) == [.ready, .acknowledged, .pending, .pending])
        var joined = Data()
        for entry in all { joined.append(try audio(of: entry, in: relaunched)) }
        #expect(joined == bytes(samples))
        // Same capture, same offsets: a third pass finds nothing to do.
        #expect(try relaunched.stageParts(noteID: noteID, capture: url, availableFrames: frames, partFrames: 16_000, isFinal: true, startMs: 9_000, sequence: 99).isEmpty)
    }

    @Test func aPartIsIdentifiedBySourceNameAndByteOffsetExactlyAsRecorderPartsAre() throws {
        let root = temporaryCaptureDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let queue = try outbox(root)
        let noteID = UUID().uuidString.lowercased()
        // The same audio staged both ways under one source name gets one identity
        // per byte offset, so neither path can upload the other's part twice.
        let samples = speechLikeSamples(count: 121 * 16_000)
        let folder = root.appendingPathComponent("a"); try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let url = folder.appendingPathComponent("capture-0.wav")
        let writer = try CaptureFileWriter(url: url)
        try samples.withUnsafeBytes { try writer.append($0) }
        try writer.close()
        let whole = try queue.stage(noteID: noteID, file: url, startMs: 5_000, sequence: 0)
        let other = try outbox(root.appendingPathComponent("second"))
        let cut = try other.stageParts(noteID: noteID, capture: url, availableFrames: samples.count, partFrames: 120 * 16_000, isFinal: true, startMs: 5_000, sequence: 0)
        #expect(cut.map(\.id) == whole.map(\.id))
        #expect(cut.map(\.startMs) == whole.map(\.startMs))
        #expect(cut.map(\.durationMs) == whole.map(\.durationMs))
        #expect(try other.audioData(for: cut[1]) == queue.audioData(for: whole[1]))
        // A different part length is a different set of offsets, and so of identities, except the first.
        let third = try outbox(root.appendingPathComponent("third"))
        let short = try third.stageParts(noteID: noteID, capture: url, availableFrames: samples.count, partFrames: 30 * 16_000, isFinal: true, startMs: 5_000, sequence: 0)
        #expect(short.count == 5)
        #expect(short[0].id == whole[0].id)
        #expect(short[4].id == whole[1].id) // both start at byte offset 120 s
    }

    @Test func invalidInputIsRefusedWithoutTouchingTheQueue() throws {
        let root = temporaryCaptureDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let queue = try outbox(root)
        let noteID = UUID().uuidString.lowercased()
        let notCapture = root.appendingPathComponent("capture-0.wav")
        try Data("not audio".utf8).write(to: notCapture)
        #expect(throws: APIClientError.invalidAudio) { try queue.stageParts(noteID: noteID, capture: notCapture, availableFrames: 100, partFrames: 10, isFinal: true, startMs: 0, sequence: 0) }
        #expect(throws: APIClientError.invalidIdentifier) { try queue.stageParts(noteID: "nope", capture: notCapture, availableFrames: 100, partFrames: 10, isFinal: true, startMs: 0, sequence: 0) }
        #expect(throws: APIClientError.invalidIdentifier) { try queue.stageParts(noteID: noteID, capture: notCapture, availableFrames: 100, partFrames: 0, isFinal: true, startMs: 0, sequence: 0) }
        #expect(queue.entries.isEmpty)
        #expect(try Data(contentsOf: notCapture) == Data("not audio".utf8))
    }
}
