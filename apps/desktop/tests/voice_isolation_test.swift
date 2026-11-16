import AVFoundation
import Accelerate

/// Voice isolation on the microphone path (`MicIsolation` + `VoiceIsolator`).
/// usage: voice_isolation_test <speech.wav, mono Float32 48 kHz (`say` makes one)>
/// - the mic timeline is exact: speech comes out at the frame it went in,
///   whether isolated from the start or switched over once the unit loads;
/// - every frame comes back: output length equals input length, including
///   the audio held inside the unit at the end, an engine gap and a failure;
/// - a loaded unit makes start instant; steady noise is taken down.
/// `precondition`, not `assert`: this runs -O.
@main
struct VoiceIsolationTests {
    static let rate = 48_000.0
    static let tap = 4_800

    static func main() {
        guard VoiceIsolator.available else {
            print("Voice isolation unit not on this Mac; skipped")
            return
        }
        let speech = readSpeech(CommandLine.arguments[1])
        // Half a second of quiet, the speech, half a second of quiet, over a little noise.
        var input = [Float](repeating: 0, count: Int(rate) / 2) + speech + [Float](repeating: 0, count: Int(rate) / 2)
        var seed: UInt64 = 7
        for index in input.indices { input[index] += noise(&seed) * 0.01 }

        // Loading ahead of time: a cold unit is slow, a pooled one is free.
        let cold = Date()
        guard let probe = VoiceIsolator(sampleRate: rate) else { preconditionFailure("unit should load at 48 kHz") }
        let coldMs = Date().timeIntervalSince(cold) * 1_000
        VoiceIsolatorPool.shared.giveBack(probe)
        let warm = Date()
        let started = MicIsolation(enabled: true, sampleRate: rate, raw: mono)
        let warmMs = Date().timeIntervalSince(warm) * 1_000
        precondition(started.status.state == "on", "a pooled unit is taken at once")
        precondition(warmMs < 50, "start with a pooled unit took \(warmMs) ms")
        let latencyMs = started.status.latencyMs
        precondition(latencyMs > 0 && latencyMs < 500, "latency \(latencyMs)")

        // 1. Isolated from the start.
        let isolated = run(started, input)
        check(isolated, input, "isolated")

        // 2. Raw while the unit loads, then isolated: hold the pooled unit so
        //    a new one has to load, and switch halfway.
        guard case .ready(let held) = VoiceIsolatorPool.shared.take(sampleRate: rate) else { preconditionFailure("pooled unit") }
        let switching = MicIsolation(enabled: true, sampleRate: rate, raw: mono)
        precondition(switching.status.state == "loading")
        let half = (input.count / 2 / tap) * tap
        var output = run(switching, Array(input[..<half]), finish: false)
        precondition(output == Array(input[..<half]), "raw audio passes through unchanged while loading")
        let deadline = Date().addingTimeInterval(20)
        while !VoiceIsolatorPool.shared.isLoaded(sampleRate: rate), Date() < deadline { usleep(10_000) }
        output += run(switching, Array(input[half...]), from: half)
        precondition(switching.status.state == "on", "switched to isolation once loaded")
        check(output, input, "switched")
        precondition(lag(Array(output[half...]), Array(input[half...])) == 0, "no shift after the switch")
        VoiceIsolatorPool.shared.giveBack(held)

        // 3. The engine drops a buffer: silence of the same length keeps the timeline.
        let gapped = MicIsolation(enabled: true, sampleRate: rate, raw: mono)
        let skip = (input.count / 3 / tap) * tap
        var kept = input
        kept.replaceSubrange(skip..<(skip + tap), with: [Float](repeating: 0, count: tap))
        output = run(gapped, input, skipping: skip)
        precondition(gapped.status.gapFrames == UInt64(tap), "gap counted: \(gapped.status.gapFrames)")
        check(output, kept, "gap")

        // 4. A failure (a buffer the unit cannot take) falls back to raw on the same timeline.
        let failing = MicIsolation(enabled: true, sampleRate: rate, raw: mono)
        let before = (input.count / 2 / tap) * tap
        output = run(failing, Array(input[..<before]), finish: false)
        let oversize = Array(input[before...]) // over a second: refused by the unit
        output += run(failing, oversize, from: before, single: true)
        precondition(failing.status.state == "failed")
        precondition(output.count == input.count, "failed: \(output.count) of \(input.count) frames")
        precondition(Array(output[before...]) == oversize, "raw audio after a failure sits at its own frames")

        // 5. Steady noise is taken down. (The failed unit was not reused: wait for a new one.)
        precondition(VoiceIsolatorPool.shared.take(sampleRate: rate).isLoading, "a failed unit is never reused")
        while !VoiceIsolatorPool.shared.isLoaded(sampleRate: rate), Date() < deadline.addingTimeInterval(20) { usleep(10_000) }
        var noiseInput = [Float](repeating: 0, count: 3 * Int(rate))
        for index in noiseInput.indices { noiseInput[index] = noise(&seed) * 0.2 }
        let denoised = run(MicIsolation(enabled: true, sampleRate: rate, raw: mono), noiseInput)
        let tail = Int(rate)
        let reduction = 10 * log10(energy(Array(noiseInput[tail...])) / max(energy(Array(denoised[tail...])), 1e-20))
        precondition(reduction >= 10, "noise only fell \(reduction) dB")
        print("Voice isolation passed: speech at its own frames (lag 0, was \(Int(latencyMs.rounded())) ms), noise down \(Int(reduction)) dB, start \(String(format: "%.2f", warmMs)) ms with a loaded unit vs \(Int(coldMs)) ms loading")
    }

    static let mono = VoiceIsolator.monoFormat(rate)
}

extension VoiceIsolatorPool.Availability {
    var isLoading: Bool { if case .loading = self { true } else { false } }
}

extension VoiceIsolationTests {
    /// Feeds `samples` as consecutive tap buffers stamped from `from`.
    static func run(_ isolation: MicIsolation, _ samples: [Float], from start: Int = 0, finish: Bool = true,
                    skipping: Int? = nil, single: Bool = false) -> [Float] {
        var output: [Float] = []
        let push: MicIsolation.Push = { list, frames, format in
            precondition(format.mChannelsPerFrame == 1 && format.mBitsPerChannel == 32)
            let data = list.pointee.mBuffers.mData!.assumingMemoryBound(to: Float.self)
            output.append(contentsOf: UnsafeBufferPointer(start: data, count: Int(frames)))
        }
        let format = AVAudioFormat(standardFormatWithSampleRate: rate, channels: 1)!
        let size = single ? samples.count : tap
        var at = 0
        while at < samples.count {
            let count = min(size, samples.count - at)
            if at == skipping { at += count; continue } // never delivered: the engine dropped it
            let buffer = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: AVAudioFrameCount(count))!
            buffer.frameLength = AVAudioFrameCount(count)
            samples.withUnsafeBufferPointer { buffer.floatChannelData![0].update(from: $0.baseAddress! + at, count: count) }
            isolation.deliver(buffer, at: AVAudioTime(sampleTime: AVAudioFramePosition(start + at), atRate: rate), push: push)
            at += count
        }
        if finish { isolation.finish(push: push) }
        return output
    }

    static func check(_ output: [Float], _ input: [Float], _ name: String) {
        precondition(output.count == input.count, "\(name): \(output.count) of \(input.count) frames")
        let found = lag(output, input)
        precondition(found == 0, "\(name): speech came out \(found) frames late")
    }

    /// The shift of `output` against `input` (frames, ±0.15 s) that lines them up best.
    static func lag(_ output: [Float], _ input: [Float]) -> Int {
        let reach = Int(rate * 0.15)
        let length = min(output.count, input.count) - 2 * reach
        precondition(length > Int(rate) / 2, "too short to align")
        var best = (lag: 0, score: -Float.infinity)
        output.withUnsafeBufferPointer { out in
            input.withUnsafeBufferPointer { inp in
                for shift in -reach...reach {
                    var score: Float = 0
                    vDSP_dotpr(out.baseAddress! + reach + shift, 1, inp.baseAddress! + reach, 1, &score, vDSP_Length(length))
                    if score > best.score { best = (shift, score) }
                }
            }
        }
        return best.lag
    }

    static func energy(_ samples: [Float]) -> Double { samples.reduce(0) { $0 + Double($1 * $1) } }

    static func noise(_ seed: inout UInt64) -> Float {
        seed = seed &* 6364136223846793005 &+ 1442695040888963407
        return Float(seed >> 40) / Float(1 << 24) - 0.5
    }

    static func readSpeech(_ path: String) -> [Float] {
        guard let file = try? AVAudioFile(forReading: URL(fileURLWithPath: path)),
              file.processingFormat.sampleRate == rate, file.processingFormat.channelCount == 1,
              let buffer = AVAudioPCMBuffer(pcmFormat: file.processingFormat, frameCapacity: AVAudioFrameCount(file.length)),
              (try? file.read(into: buffer)) != nil else { preconditionFailure("need mono 48 kHz speech at \(path)") }
        return Array(UnsafeBufferPointer(start: buffer.floatChannelData![0], count: Int(buffer.frameLength)))
    }
}
