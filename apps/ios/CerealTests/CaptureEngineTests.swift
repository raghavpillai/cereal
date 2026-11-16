import AVFoundation
import Foundation
import Testing
import APIClient
import Contracts
@testable import Cereal

// MARK: - Injected audio

/// A 200 Hz tone written against one global clock, whatever format each piece
/// is delivered in. At 16 kHz its period is exactly 80 samples, so the output's
/// zero crossings show whether any time was lost or added where formats change.
enum InjectedAudio {
    static let toneHz = 200.0

    static func buffer(format: AVAudioFormat, startingAt seconds: Double, frames: Int, amplitude: Double = 0.5) -> AVAudioPCMBuffer {
        let buffer = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: AVAudioFrameCount(frames))!
        buffer.frameLength = AVAudioFrameCount(frames)
        for index in 0..<frames {
            let value = amplitude * sin(2 * .pi * toneHz * (seconds + Double(index) / format.sampleRate))
            for channel in 0..<Int(format.channelCount) {
                if let floats = buffer.floatChannelData { floats[channel][index] = Float(value) }
                else if let ints = buffer.int16ChannelData { ints[0][index * Int(format.channelCount) + channel] = Int16(value * 32_767) }
            }
        }
        return buffer
    }

    /// Feeds `seconds` of tone in tap-sized buffers. Returns the time reached.
    @discardableResult
    static func feed(_ deliver: (AVAudioPCMBuffer) -> Void, format: AVAudioFormat, from start: Double, seconds: Double, chunk: Int = 4096) -> Double {
        let total = Int((seconds * format.sampleRate).rounded())
        var done = 0
        while done < total {
            let count = min(chunk, total - done)
            deliver(buffer(format: format, startingAt: start + Double(done) / format.sampleRate, frames: count))
            done += count
        }
        return start + seconds
    }

    static func samples(in file: URL) throws -> [Int16] {
        let data = try Data(contentsOf: file).dropFirst(ContinuousCapture.headerBytes)
        return data.withUnsafeBytes { Array($0.bindMemory(to: Int16.self)) }
    }

    /// Indices where the signal goes from negative to non-negative.
    static func upCrossings(_ samples: [Int16]) -> [Int] {
        (1..<samples.count).filter { samples[$0 - 1] < 0 && samples[$0] >= 0 }
    }

    static func directory() -> URL {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("capture-engine-\(UUID().uuidString)", isDirectory: true)
        try! FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        return url
    }

    static let builtIn = AVAudioFormat(standardFormatWithSampleRate: 48_000, channels: 1)!
    static let stereo44 = AVAudioFormat(standardFormatWithSampleRate: 44_100, channels: 2)!
    static let handsFree24 = AVAudioFormat(standardFormatWithSampleRate: 24_000, channels: 1)!
    static let handsFree16 = AVAudioFormat(commonFormat: .pcmFormatInt16, sampleRate: 16_000, channels: 1, interleaved: true)!
    static let narrowband8 = AVAudioFormat(standardFormatWithSampleRate: 8_000, channels: 1)!
}

/// An input the test drives by hand: buffers go straight into the pipeline, as the tap delivers them.
@MainActor
final class ManualCaptureInput: CaptureInput {
    let usesMicrophone = false
    private(set) var isRunning = false
    private(set) var starts = 0
    var failsToStart = false
    private var deliver: (@Sendable (AVAudioPCMBuffer) -> Void)?
    private var changed: (@MainActor () -> Void)?

    func start(deliver: @escaping @Sendable (AVAudioPCMBuffer) -> Void, changed: @escaping @MainActor () -> Void) throws {
        if failsToStart { throw Recorder.RecorderError.couldNotStart }
        self.deliver = deliver
        self.changed = changed
        isRunning = true
        starts += 1
    }

    func stop() { isRunning = false; deliver = nil }

    func push(_ buffer: AVAudioPCMBuffer) { deliver?(buffer) }
    @discardableResult
    func feed(format: AVAudioFormat, from start: Double, seconds: Double) -> Double {
        InjectedAudio.feed({ push($0) }, format: format, from: start, seconds: seconds)
    }
    /// What `AVAudioEngineConfigurationChange` does: the engine has stopped and must be started again.
    func routeChanged() { isRunning = false; changed?() }
}

// MARK: - Pipeline

struct CapturePipelineTests {
    @Test func sampleCountIsExactAndTimeIsContinuousAcrossConverterRebuilds() throws {
        let root = InjectedAudio.directory()
        defer { try? FileManager.default.removeItem(at: root) }
        let url = root.appendingPathComponent("capture-0.wav")
        let pipeline = try CapturePipeline(url: url)
        // Built-in microphone, then AirPods at 24 kHz, a stereo interface, a 16 kHz
        // hands-free route, a narrowband one, and back. Each change is a new input format.
        let route: [(AVAudioFormat, Double)] = [
            (InjectedAudio.builtIn, 2), (InjectedAudio.handsFree24, 1), (InjectedAudio.stereo44, 1.5),
            (InjectedAudio.handsFree16, 1), (InjectedAudio.narrowband8, 1), (InjectedAudio.builtIn, 2.5),
        ]
        var time = 0.0
        var expected: Int64 = 0
        for (format, seconds) in route {
            time = InjectedAudio.feed({ pipeline.append($0) }, format: format, from: time, seconds: seconds)
            expected += Int64((seconds * 16_000).rounded())
        }
        let frames = pipeline.finish()

        // Exact: every input second became 16 000 samples, with nothing lost or invented at a rebuild.
        #expect(frames == expected)
        #expect(frames == 9 * 16_000)
        #expect(pipeline.converterBuilds == route.count)
        #expect(ContinuousCapture.frames(inFile: url) == Int(expected))
        let file = try Data(contentsOf: url)
        #expect(file.prefix(44) == ContinuousCapture.header(dataBytes: Int(expected) * 2))

        // The tone's period is 80 samples. If a rebuild had dropped or repeated audio, every
        // crossing after it would be off the grid by that amount.
        let crossings = InjectedAudio.upCrossings(try InjectedAudio.samples(in: url))
        #expect(crossings.count >= 9 * 200 - 12)
        let offGrid = crossings.map { min($0 % 80, 80 - $0 % 80) }
        #expect((offGrid.max() ?? 99) <= 3)
        // And none is missing in the middle of the capture: consecutive crossings are one or two periods apart.
        #expect(zip(crossings, crossings.dropFirst()).allSatisfy { (77...163).contains($1 - $0) })
    }

    @Test func aStreamOfOneFormatBuildsOneConverterAndNeverDriftsOverTenMinutes() throws {
        let root = InjectedAudio.directory()
        defer { try? FileManager.default.removeItem(at: root) }
        let url = root.appendingPathComponent("capture-0.wav")
        let pipeline = try CapturePipeline(url: url)
        // 44.1 kHz is not a whole multiple of 16 kHz: the place a rounding drift would show.
        InjectedAudio.feed({ pipeline.append($0) }, format: InjectedAudio.stereo44, from: 0, seconds: 600, chunk: 4410)
        #expect(pipeline.converterBuilds == 1)
        // While recording, the count trails the input by what the converter is holding:
        // less than one input buffer (0.1 s here). It does not grow with the length of the capture.
        #expect(600 * 16_000 - pipeline.frames < 1_700)
        #expect(pipeline.finish() == 600 * 16_000)
    }

    @Test func pauseStopsTheFileAtAnExactSampleAndResumeAppendsThere() throws {
        let root = InjectedAudio.directory()
        defer { try? FileManager.default.removeItem(at: root) }
        let url = root.appendingPathComponent("capture-0.wav")
        let pipeline = try CapturePipeline(url: url)
        var time = InjectedAudio.feed({ pipeline.append($0) }, format: InjectedAudio.builtIn, from: 0, seconds: 1.25)
        pipeline.flush()
        // Everything heard before the pause is in the file, and the header says so.
        #expect(pipeline.frames == 20_000)
        #expect(ContinuousCapture.frames(inFile: url) == 20_000)
        #expect(try Data(contentsOf: url).prefix(44) == ContinuousCapture.header(dataBytes: 40_000))
        pipeline.flush() // twice changes nothing
        #expect(pipeline.frames == 20_000)
        // The input comes back on another route.
        time = InjectedAudio.feed({ pipeline.append($0) }, format: InjectedAudio.handsFree24, from: time, seconds: 0.75)
        #expect(pipeline.finish() == 32_000)
        #expect(pipeline.converterBuilds == 2)
        let crossings = InjectedAudio.upCrossings(try InjectedAudio.samples(in: url))
        #expect(crossings.allSatisfy { min($0 % 80, 80 - $0 % 80) <= 3 })
        // Buffers after Stop are ignored.
        pipeline.append(InjectedAudio.buffer(format: InjectedAudio.builtIn, startingAt: 0, frames: 4_096))
        #expect(pipeline.frames == 32_000)
        #expect(ContinuousCapture.frames(inFile: url) == 32_000)
    }

    @Test func theHeaderOnDiskIsNeverMoreThanASecondBehindTheAudio() throws {
        let root = InjectedAudio.directory()
        defer { try? FileManager.default.removeItem(at: root) }
        let url = root.appendingPathComponent("capture-0.wav")
        let pipeline = try CapturePipeline(url: url)
        var worst = 0
        var time = 0.0
        for _ in 0..<120 {
            time = InjectedAudio.feed({ pipeline.append($0) }, format: InjectedAudio.builtIn, from: time, seconds: 0.085)
            let header = try FileHandle(forReadingFrom: url).read(upToCount: 44)!
            let inHeader = (Int(header[40]) | Int(header[41]) << 8 | Int(header[42]) << 16 | Int(header[43]) << 24) / 2
            worst = max(worst, Int(pipeline.frames) - inHeader)
        }
        #expect(worst <= 16_000 + 1_400)
        pipeline.finish()
    }

    @Test func lowDiskStopsTheCaptureBeforeTheDiskIsFullAndKeepsWhatWasRecorded() throws {
        let root = InjectedAudio.directory()
        defer { try? FileManager.default.removeItem(at: root) }
        let url = root.appendingPathComponent("capture-0.wav")
        let free = Reported()
        free.add(1 << 30)
        let failures = FailureLog()
        var options = CapturePipeline.Options()
        options.freeSpaceCheckFrames = 16_000
        options.freeBytes = { _ in free.values.last }
        let pipeline = try CapturePipeline(url: url, options: options, onFailure: { failures.add($0) })
        var time = InjectedAudio.feed({ pipeline.append($0) }, format: InjectedAudio.builtIn, from: 0, seconds: 2)
        #expect(pipeline.failure == nil)
        free.add(8 << 20) // under the 32 MB reserve
        time = InjectedAudio.feed({ pipeline.append($0) }, format: InjectedAudio.builtIn, from: time, seconds: 3)
        #expect(pipeline.failure == .lowDisk)
        #expect(failures.values == [.lowDisk])
        // It stopped at the first check, within about a second, and nothing after that was written.
        let kept = pipeline.finish()
        #expect(kept >= 2 * 16_000 && kept <= 3 * 16_000 + 1_400)
        #expect(ContinuousCapture.frames(inFile: url) == Int(kept))
        #expect(try Data(contentsOf: url).prefix(44) == ContinuousCapture.header(dataBytes: Int(kept) * 2))
        #expect(CapturePipelineFailure.lowDisk.message.contains("Storage is almost full"))
        #expect(CapturePipelineFailure.write.message == "Audio could not be saved completely. Any captured audio has been kept.")
    }

    @Test func theMeterRestsInSilenceAndRisesWithSound() throws {
        let root = InjectedAudio.directory()
        defer { try? FileManager.default.removeItem(at: root) }
        let pipeline = try CapturePipeline(url: root.appendingPathComponent("capture-0.wav"))
        #expect(pipeline.level == 0.12)
        pipeline.append(InjectedAudio.buffer(format: InjectedAudio.builtIn, startingAt: 0, frames: 4_096, amplitude: 0))
        #expect(pipeline.level == 0.12)
        pipeline.append(InjectedAudio.buffer(format: InjectedAudio.builtIn, startingAt: 0, frames: 4_096, amplitude: 0.05))
        let quiet = pipeline.level
        pipeline.append(InjectedAudio.buffer(format: InjectedAudio.builtIn, startingAt: 0, frames: 4_096, amplitude: 0.9))
        let loud = pipeline.level
        #expect(quiet > 0.12 && quiet < loud)
        // A full-scale sine is about −3 dB mean power: the same scale the recorder's meter used.
        #expect(abs(loud - powf(10, -3.9 / 35)) < 0.05)
        pipeline.finish()
    }

    @Test func aCaptureIsNeverWrittenOverAnExistingFile() throws {
        let root = InjectedAudio.directory()
        defer { try? FileManager.default.removeItem(at: root) }
        let url = root.appendingPathComponent("capture-0.wav")
        try Data("earlier capture".utf8).write(to: url)
        #expect(throws: CaptureFileError.self) { try CapturePipeline(url: url) }
        #expect(try Data(contentsOf: url) == Data("earlier capture".utf8))
    }
}

final class Reported: @unchecked Sendable {
    private let lock = NSLock()
    private var stored: [Int64] = []
    func add(_ value: Int64) { lock.withLock { stored.append(value) } }
    var values: [Int64] { lock.withLock { stored } }
}

final class FailureLog: @unchecked Sendable {
    private let lock = NSLock()
    private var stored: [CapturePipelineFailure] = []
    func add(_ value: CapturePipelineFailure) { lock.withLock { stored.append(value) } }
    var values: [CapturePipelineFailure] { lock.withLock { stored } }
}

// MARK: - Recorder on the engine backend

@MainActor
@Suite(.serialized)
struct EngineRecorderTests {
    private func recorder(_ input: ManualCaptureInput, root: URL) -> Recorder {
        Recorder(simulateCapture: false, recordingRoot: root, backend: { EngineCaptureBackend(input: input) })
    }

    @Test func oneContinuousCaptureFromStartToStopWithExactDurationAndClock() async throws {
        let root = InjectedAudio.directory()
        defer { try? FileManager.default.removeItem(at: root) }
        let input = ManualCaptureInput()
        let recorder = recorder(input, root: root)
        let id = UUID()
        var finished: [(CaptureSegment, URL)] = []
        recorder.onSegmentFinished = { finished.append(($0, $1)) }
        recorder.prepare(existing: [CaptureSegment(index: 0, startedAt: .now, duration: 20, fileName: "segment-0.wav")])
        recorder.startSegment(noteID: id)
        #expect(recorder.state == .recording)
        #expect(recorder.capturesContinuously)
        let file = try #require(recorder.currentFileURL)
        #expect(file.lastPathComponent == "capture-1.wav")
        // The recovery journal exists before any audio does.
        #expect(FileManager.default.fileExists(atPath: CaptureRecovery.journalURL(for: file).path))

        input.feed(format: InjectedAudio.builtIn, from: 0, seconds: 3)
        try await Task.sleep(for: .milliseconds(200)) // a meter tick
        // The clock is the sample count: earlier segments plus what is in the file.
        #expect(abs(recorder.elapsed - 23) < 0.05)
        #expect(recorder.levels.contains { $0 > 0.12 })

        let stopped = try #require(recorder.stopSegment())
        #expect(stopped.segment.index == 1)
        #expect(stopped.segment.duration == 3)
        #expect(recorder.capturedFrames == 48_000)
        #expect(recorder.elapsed == 23)
        #expect(recorder.state == .stopped)
        #expect(recorder.stopSegment() == nil)
        #expect(finished.count == 1)
        #expect(ContinuousCapture.frames(inFile: file) == 48_000)
        #expect(!input.isRunning)
    }

    @Test func pauseAndResumeKeepOneFileAndCountOnlyRecordedAudio() async throws {
        let root = InjectedAudio.directory()
        defer { try? FileManager.default.removeItem(at: root) }
        let input = ManualCaptureInput()
        let recorder = recorder(input, root: root)
        var pauses: [(Bool, Int64)] = []
        recorder.onPauseChanged = { pauses.append(($0, recorder.capturedFrames)) }
        recorder.startSegment(noteID: UUID())
        var time = input.feed(format: InjectedAudio.builtIn, from: 0, seconds: 1)
        recorder.pause()
        #expect(recorder.state == .paused)
        #expect(!input.isRunning)
        // The sample count is final at the moment the pause is announced.
        #expect(pauses.count == 1 && pauses[0].0 && pauses[0].1 == 16_000)
        #expect(recorder.elapsed == 1)
        try await Task.sleep(for: .milliseconds(150))
        #expect(recorder.elapsed == 1) // waiting is not audio
        recorder.resumeFromPause()
        #expect(recorder.state == .recording && input.isRunning && input.starts == 2)
        #expect(pauses.count == 2 && !pauses[1].0)
        time = input.feed(format: InjectedAudio.builtIn, from: time, seconds: 2)
        let stopped = try #require(recorder.stopSegment())
        #expect(stopped.segment.duration == 3)
        #expect(stopped.file.lastPathComponent == "capture-0.wav")
    }

    @Test func aRouteChangeRestartsTheInputAndTheCaptureCarriesOnInTheSameFile() async throws {
        let root = InjectedAudio.directory()
        defer { try? FileManager.default.removeItem(at: root) }
        let input = ManualCaptureInput()
        let recorder = recorder(input, root: root)
        var finished = 0
        recorder.onSegmentFinished = { _, _ in finished += 1 }
        recorder.startSegment(noteID: UUID())
        let file = try #require(recorder.currentFileURL)
        var time = input.feed(format: InjectedAudio.builtIn, from: 0, seconds: 2)
        // AirPods connect: the engine reports a configuration change and stops.
        input.routeChanged()
        #expect(input.isRunning && input.starts == 2)
        #expect(recorder.state == .recording)
        time = input.feed(format: InjectedAudio.handsFree24, from: time, seconds: 2)
        // And back to the built-in microphone at another rate.
        input.routeChanged()
        time = input.feed(format: InjectedAudio.stereo44, from: time, seconds: 1)
        try await Task.sleep(for: .milliseconds(120))
        #expect(recorder.state == .recording)
        #expect(finished == 0) // not a new segment: the same capture
        let stopped = try #require(recorder.stopSegment())
        #expect(stopped.file == file)
        #expect(stopped.segment.duration == 5)
        let crossings = InjectedAudio.upCrossings(try InjectedAudio.samples(in: file))
        #expect(crossings.allSatisfy { min($0 % 80, 80 - $0 % 80) <= 3 })
    }

    @Test func aHeadsetDisappearingMidCaptureCarriesOnWithTheBuiltInMicrophone() async throws {
        let root = InjectedAudio.directory()
        defer { try? FileManager.default.removeItem(at: root) }
        let input = ManualCaptureInput()
        let recorder = recorder(input, root: root)
        var finished = 0
        recorder.onSegmentFinished = { _, _ in finished += 1 }
        recorder.startSegment(noteID: UUID())
        let file = try #require(recorder.currentFileURL)
        // Recording through AirPods: a 24 kHz hands-free route.
        var time = input.feed(format: InjectedAudio.handsFree24, from: 0, seconds: 2)
        #expect(recorder.capturedFrames >= 2 * 16_000 - 400)
        // They go back in their case. The engine reports a configuration change and stops;
        // the route that is left is the built-in microphone at 48 kHz.
        input.routeChanged()
        #expect(recorder.state == .recording)
        #expect(input.isRunning && input.starts == 2)
        time = input.feed(format: InjectedAudio.builtIn, from: time, seconds: 3)
        try await Task.sleep(for: .milliseconds(200))
        #expect(recorder.state == .recording)
        #expect(finished == 0) // the same capture, not a new segment
        #expect(abs(recorder.elapsed - 5) < 0.05)
        let stopped = try #require(recorder.stopSegment())
        #expect(stopped.file == file)
        // No sample was lost or added where the input changed: 5 s is exactly 80 000 samples
        // and the tone stays on its grid across the change.
        #expect(stopped.segment.duration == 5)
        #expect(ContinuousCapture.frames(inFile: file) == 80_000)
        let crossings = InjectedAudio.upCrossings(try InjectedAudio.samples(in: file))
        #expect(crossings.count >= 5 * 200 - 4)
        #expect(crossings.allSatisfy { min($0 % 80, 80 - $0 % 80) <= 3 })
    }

    @Test func anEngineThatStoppedWithoutSayingSoIsStartedAgainByTheMeterTick() async throws {
        let root = InjectedAudio.directory()
        defer { try? FileManager.default.removeItem(at: root) }
        let input = ManualCaptureInput()
        let recorder = recorder(input, root: root)
        var finished = 0
        recorder.onSegmentFinished = { _, _ in finished += 1 }
        recorder.startSegment(noteID: UUID())
        var time = input.feed(format: InjectedAudio.handsFree24, from: 0, seconds: 1)
        // The route went away and the notification has not arrived (or never does).
        input.stop()
        try await Task.sleep(for: .milliseconds(250))
        #expect(recorder.state == .recording)
        #expect(input.isRunning && input.starts == 2)
        time = input.feed(format: InjectedAudio.builtIn, from: time, seconds: 1)
        #expect(finished == 0)
        #expect(recorder.stopSegment()?.segment.duration == 2)
    }

    @Test func aRouteChangeThatLeavesNoInputAtAllEndsTheCaptureAndKeepsTheAudio() async throws {
        let root = InjectedAudio.directory()
        defer { try? FileManager.default.removeItem(at: root) }
        let input = ManualCaptureInput()
        let recorder = recorder(input, root: root)
        var finished: [CaptureSegment] = []
        recorder.onSegmentFinished = { segment, _ in finished.append(segment) }
        recorder.startSegment(noteID: UUID())
        input.feed(format: InjectedAudio.builtIn, from: 0, seconds: 1.5)
        input.failsToStart = true
        input.routeChanged()
        // A route can take a moment to settle, so the capture is not given up at once...
        try await Task.sleep(for: .milliseconds(300))
        #expect(recorder.state == .recording)
        #expect(finished.isEmpty)
        // ...but after about a second with no input it ends, with everything recorded kept.
        try await waitFor("capture given up", timeout: .seconds(4)) { !recorder.isCapturing }
        #expect(recorder.state == .failed("The microphone disconnected. Your audio has been kept. Choose an input and tap to resume."))
        #expect(finished.map(\.duration) == [1.5])
        #expect(recorder.stopSegment() == nil)
    }

    @Test func anInputThatComesBackWithinTheGraceCarriesOn() async throws {
        let root = InjectedAudio.directory()
        defer { try? FileManager.default.removeItem(at: root) }
        let input = ManualCaptureInput()
        let recorder = recorder(input, root: root)
        recorder.startSegment(noteID: UUID())
        var time = input.feed(format: InjectedAudio.handsFree24, from: 0, seconds: 1)
        input.failsToStart = true
        input.routeChanged()
        try await Task.sleep(for: .milliseconds(350)) // the new route is not ready yet
        input.failsToStart = false
        try await waitFor("input restarted") { input.isRunning }
        time = input.feed(format: InjectedAudio.builtIn, from: time, seconds: 1)
        #expect(recorder.state == .recording)
        #expect(recorder.stopSegment()?.segment.duration == 2)
    }

    @Test func interruptionAndMediaServicesResetFinalizeOnceAndRequireAnExplicitResume() async throws {
        let root = InjectedAudio.directory()
        defer { try? FileManager.default.removeItem(at: root) }
        for (name, key, value, message) in [
            (AVAudioSession.interruptionNotification, AVAudioSessionInterruptionTypeKey, AVAudioSession.InterruptionType.began.rawValue,
             "Recording was interrupted. Your audio has been kept. Tap to resume when ready."),
            (AVAudioSession.mediaServicesWereResetNotification, "", UInt(0), "The audio system restarted. Your audio has been kept. Tap to resume."),
        ] {
            let input = ManualCaptureInput()
            let recorder = recorder(input, root: root)
            let id = UUID()
            var finished: [(CaptureSegment, URL)] = []
            recorder.onSegmentFinished = { finished.append(($0, $1)) }
            recorder.startSegment(noteID: id)
            input.feed(format: InjectedAudio.builtIn, from: 0, seconds: 2)
            NotificationCenter.default.post(name: name, object: nil, userInfo: key.isEmpty ? nil : [key: value])
            #expect(recorder.state == .failed(message))
            #expect(!input.isRunning)
            #expect(finished.count == 1)
            #expect(finished[0].0.duration == 2)
            // The capture file is complete and valid.
            #expect(ContinuousCapture.frames(inFile: finished[0].1) == 32_000)
            #expect(try Data(contentsOf: finished[0].1).prefix(44) == ContinuousCapture.header(dataBytes: 64_000))
            NotificationCenter.default.post(name: AVAudioSession.interruptionNotification, object: nil,
                userInfo: [AVAudioSessionInterruptionTypeKey: AVAudioSession.InterruptionType.ended.rawValue])
            #expect(!recorder.isCapturing) // never resumes by itself
            // The user's Resume starts a new capture after it, on the same clock.
            recorder.startSegment(noteID: id)
            #expect(recorder.state == .recording)
            #expect(recorder.currentFileURL?.lastPathComponent == "capture-1.wav")
            input.feed(format: InjectedAudio.builtIn, from: 2, seconds: 1)
            #expect(recorder.stopSegment()?.segment.duration == 1)
            #expect(recorder.elapsed == 3)
        }
    }

    @Test func aCaptureThatCanNoLongerBeWrittenEndsWithItsOwnReasonNotARestart() async throws {
        let root = InjectedAudio.directory()
        defer { try? FileManager.default.removeItem(at: root) }
        // The recorder path, and anything that is not a continuous capture, still ends
        // when its input stops by itself.
        final class StoppingBackend: CaptureBackend {
            let usesMicrophone = false
            var onFailure: ((String) -> Void)?
            var isRunning = false
            var capturedTime: TimeInterval = 1
            func start(writingTo url: URL) throws { isRunning = true; try Data(count: 64).write(to: url) }
            func pause() {}
            func resume() throws {}
            func stop() -> TimeInterval? { isRunning = false; return capturedTime }
            func level() -> Float { 0.12 }
        }
        let backend = StoppingBackend()
        let recorder = Recorder(simulateCapture: false, recordingRoot: root, backend: { backend })
        var finished = 0
        recorder.onSegmentFinished = { _, _ in finished += 1 }
        recorder.startSegment(noteID: UUID())
        #expect(!recorder.capturesContinuously)
        backend.isRunning = false
        try await Task.sleep(for: .milliseconds(250))
        #expect(recorder.state == .failed("Recording stopped unexpectedly. Your captured audio has been kept. Tap to resume."))
        #expect(finished == 1)
    }

    @Test func aStartThatFailsLeavesNoEmptyCaptureOrJournalBehind() throws {
        let root = InjectedAudio.directory()
        defer { try? FileManager.default.removeItem(at: root) }
        let input = ManualCaptureInput()
        input.failsToStart = true
        let recorder = recorder(input, root: root)
        let id = UUID()
        recorder.startSegment(noteID: id)
        #expect(recorder.state == .failed("Recording could not start. Check that a microphone is available."))
        let folder = root.appendingPathComponent(id.uuidString)
        #expect((try? FileManager.default.contentsOfDirectory(atPath: folder.path))?.isEmpty ?? true)
        #expect(CaptureRecovery.scan(roots: [root]).isEmpty)
        // A capture stopped before any audio arrived is the same.
        input.failsToStart = false
        recorder.startSegment(noteID: id)
        #expect(recorder.stopSegment() == nil)
        #expect(CaptureRecovery.scan(roots: [root]).isEmpty)
    }
}

// MARK: - Crash recovery

@MainActor
struct ContinuousCaptureRecoveryTests {
    @Test func aCaptureKilledMidMeetingIsMeasuredToItsLastWrittenSample() throws {
        let root = InjectedAudio.directory()
        defer { try? FileManager.default.removeItem(at: root) }
        let noteID = UUID()
        let recordings = root.appendingPathComponent("recordings", isDirectory: true)
        let url = try Recorder.fileURL(noteID: noteID, index: 0, continuous: true, root: recordings)
        #expect(url.lastPathComponent == "capture-0.wav")
        let startedAt = Date(timeIntervalSince1970: 1_000)
        let journal = try CaptureRecovery.begin(noteID: noteID, index: 0, startedAt: startedAt, audioFile: url)
        let pipeline = try CapturePipeline(url: url)
        // 72.3 seconds are recorded, then the process is killed: no flush, no final header.
        InjectedAudio.feed({ pipeline.append($0) }, format: InjectedAudio.builtIn, from: 0, seconds: 72.3)
        let writtenAtKill = try #require(ContinuousCapture.frames(inFile: url))
        let header = try Data(contentsOf: url).prefix(44)
        let inHeader = (Int(header[40]) | Int(header[41]) << 8 | Int(header[42]) << 16 | Int(header[43]) << 24) / 2
        // The header is behind the audio, as it would be after a real kill.
        #expect(inHeader < writtenAtKill)
        // What reached the file is all of the audio but the converter's last few milliseconds.
        #expect(Int(72.3 * 16_000) - writtenAtKill < 400)

        let found = CaptureRecovery.scan(roots: [recordings])
        #expect(found.count == 1)
        let capture = try #require(found.first)
        #expect(capture.noteID == noteID && capture.index == 0 && capture.startedAt == startedAt)
        #expect(capture.journalFile == journal)
        // Measured from the file, not the stale header: nothing between the two is lost.
        #expect(capture.duration == Double(writtenAtKill) / 16_000)
        // Recovery read the file and changed nothing in it.
        #expect(try Data(contentsOf: url).prefix(44) == header)
        withExtendedLifetime(pipeline) {}
    }

    @Test func recoveryStillReadsRecorderSegmentsFromTheirHeader() throws {
        let root = InjectedAudio.directory()
        defer { try? FileManager.default.removeItem(at: root) }
        let noteID = UUID()
        let url = try Recorder.fileURL(noteID: noteID, index: 0, root: root)
        #expect(url.lastPathComponent == "segment-0.wav")
        var wav = ContinuousCapture.header(dataBytes: 16_000)
        wav.append(Data(repeating: 0, count: 16_000))
        try wav.write(to: url)
        #expect(CaptureRecovery.scan(roots: [root]).first?.duration == 0.5)
        // A file with the capture name but another writer's contents is not measured by size.
        let odd = try Recorder.fileURL(noteID: noteID, index: 1, continuous: true, root: root)
        try Data(repeating: 9, count: 5_000).write(to: odd)
        #expect(CaptureRecovery.readableDuration(odd) == nil)
    }
}

// MARK: - The switch

@MainActor
struct CaptureEnginePolicyTests {
    @Test func aLostInputDeviceEndsTheRecorderPathButNotTheEngineAndNoInputEndsBoth() {
        // Headset or Bluetooth route gone, built-in microphone still there.
        #expect(RouteChangePolicy.endsCapture(inputDeviceRemoved: true, noInputAvailable: false, continuous: false))
        #expect(!RouteChangePolicy.endsCapture(inputDeviceRemoved: true, noInputAvailable: false, continuous: true))
        // Nothing left to record from.
        #expect(RouteChangePolicy.endsCapture(inputDeviceRemoved: true, noInputAvailable: true, continuous: true))
        #expect(RouteChangePolicy.endsCapture(inputDeviceRemoved: false, noInputAvailable: true, continuous: true))
        #expect(RouteChangePolicy.endsCapture(inputDeviceRemoved: false, noInputAvailable: true, continuous: false))
        // A new device arriving ends neither.
        #expect(!RouteChangePolicy.endsCapture(inputDeviceRemoved: false, noInputAvailable: false, continuous: false))
        #expect(!RouteChangePolicy.endsCapture(inputDeviceRemoved: false, noInputAvailable: false, continuous: true))
    }

    @Test func theEngineIsTheDefaultAndOneKeySelectsTheRecorder() throws {
        let name = "capture-policy-\(UUID().uuidString)"
        let defaults = try #require(UserDefaults(suiteName: name))
        defer { defaults.removePersistentDomain(forName: name) }
        #expect(!CaptureEnginePolicy.usesLegacyRecorder(defaults: defaults))
        defaults.set(true, forKey: "capture.legacyRecorder")
        #expect(CaptureEnginePolicy.usesLegacyRecorder(defaults: defaults))
        #expect(CaptureEnginePolicy.legacyRecorderKey == "capture.legacyRecorder")
        // The two implementations differ in exactly one capability.
        #expect(EngineCaptureBackend() is any ContinuousCaptureBackend)
        #expect(!((LegacyRecorderBackend() as any CaptureBackend) is any ContinuousCaptureBackend))
        // Unless the key is set in this process, a recorder gets the engine.
        if !CaptureEnginePolicy.usesLegacyRecorder() { #expect(CaptureEnginePolicy.makeBackend() is EngineCaptureBackend) }
        // A recording to a server, whose only transcript is the live stream, always gets the engine.
        #expect(CaptureEnginePolicy.makeContinuousBackend() is EngineCaptureBackend)
    }

    @Test func aCapturesPlaceOnTheNoteTimelineIsFixedWhenItBegins() throws {
        var history = ServerCaptureHistory(baseOffsetMs: 1_000, segments: [])
        history.prepareForResume(remoteDurationMs: 61_000)
        // The capture's place on the note timeline is known before any audio exists.
        #expect(history.begin(fileName: "capture-0.wav") == 61_000)
        // When it finishes, it keeps that place, and the next capture follows it.
        history.append(CaptureSegment(index: 0, startedAt: .now, duration: 95.5, fileName: "capture-0.wav"))
        #expect(history.segmentOffsets?["capture-0.wav"] == 61_000)
        #expect(history.nextStartMs == 156_500)
        #expect(history.begin(fileName: "capture-1.wav") == 156_500)
        // The ledger round-trips, and one an earlier version wrote, with its delivery fields, still decodes.
        let restored = try JSONDecoder().decode(ServerCaptureHistory.self, from: JSONEncoder().encode(history))
        #expect(restored.segmentOffsets == history.segmentOffsets && restored.nextStartMs == 156_500)
        let old = try JSONDecoder().decode(ServerCaptureHistory.self, from: Data(#"{"baseOffsetMs":0,"nextSequence":4,"segments":[],"partSeconds":{"capture-0.wav":30},"serverMissingSince":0}"#.utf8))
        #expect(old.baseOffsetMs == 0 && old.needsStop == nil && old.needsNotes == nil)
    }
}
