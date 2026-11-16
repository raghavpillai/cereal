@preconcurrency import AVFoundation
import AudioToolbox
import CoreAudio
import Foundation

/// Apple's voice isolation model (the `AUSoundIsolation` effect, macOS 13+)
/// applied to the microphone inside Cereal, so it is on by default without
/// the person choosing a Mic Mode. macOS gives apps no way to choose the
/// Control Center Mic Mode (`AVCaptureDevice.preferredMicrophoneMode` is
/// read-only), and that mode only applies through voice processing, whose
/// ducking of other audio can be lowered but never turned off.
///
/// The unit runs on the input tap's thread, never on the device IO thread,
/// and never touches an output device: the meeting's playback, the system
/// audio channel and the route stay exactly as they were.
///
/// Timeline: the unit holds `latencyFrames` of audio inside. The first that
/// many frames it produces after `reset` are dropped, and `flush` gets the
/// held audio out at the end, so output frame n is always input frame n.
/// One caller at a time (`MicIsolation` serialises them).
final class VoiceIsolator: @unchecked Sendable {
    /// The largest slice the unit renders at once; longer buffers are split.
    static let slice: UInt32 = 4_096
    let sampleRate: Double
    private let unit: AudioUnit
    private let mono: UnsafeMutablePointer<Float>
    private let processed: UnsafeMutablePointer<Float>
    /// Frames `process` takes at once (a second, at least 16384).
    let capacity: Int
    private var source: UnsafePointer<Float>?
    private var sampleTime: Float64 = 0
    /// Output frames still to drop: the unit's delay since `reset`.
    private var trim = 0
    private var offset = 0
    /// Seconds the unit delays its input by (about 0.0925 s at 48 kHz).
    private(set) var latency: Double = 0
    /// `latency` in frames: what is held inside the unit once it has run.
    private(set) var latencyFrames = 0
    /// Mono Float32, the format of `output`.
    let description: AudioStreamBasicDescription

    static var available: Bool {
        var component = componentDescription
        return AudioComponentFindNext(nil, &component) != nil
    }
    private static var componentDescription: AudioComponentDescription {
        AudioComponentDescription(componentType: kAudioUnitType_Effect, componentSubType: kAudioUnitSubType_AUSoundIsolation,
                                  componentManufacturer: kAudioUnitManufacturer_Apple, componentFlags: 0, componentFlagsMask: 0)
    }
    static func monoFormat(_ sampleRate: Double) -> AudioStreamBasicDescription {
        AudioStreamBasicDescription(
            mSampleRate: sampleRate, mFormatID: kAudioFormatLinearPCM,
            mFormatFlags: kAudioFormatFlagsNativeFloatPacked | kAudioFormatFlagIsNonInterleaved,
            mBytesPerPacket: 4, mFramesPerPacket: 1, mBytesPerFrame: 4, mChannelsPerFrame: 1,
            mBitsPerChannel: 32, mReserved: 0)
    }

    /// Loads the model: 0.06-0.5 s, more on a busy Mac, so `VoiceIsolatorPool`
    /// does it ahead of time. Nil when this Mac has no voice isolation unit or
    /// it refuses the rate.
    init?(sampleRate: Double) {
        var component = Self.componentDescription
        guard sampleRate.isFinite, sampleRate > 0, let found = AudioComponentFindNext(nil, &component) else { return nil }
        var instance: AudioUnit?
        guard AudioComponentInstanceNew(found, &instance) == noErr, let instance else { return nil }
        var format = Self.monoFormat(sampleRate)
        var slice = Self.slice
        let formatSize = UInt32(MemoryLayout<AudioStreamBasicDescription>.size)
        // The higher-quality model where macOS has it (15+), the standard one before.
        var kind = AudioUnitParameterValue(kAUSoundIsolationSoundType_Voice)
        if #available(macOS 15.0, *) { kind = AudioUnitParameterValue(kAUSoundIsolationSoundType_HighQualityVoice) }
        guard AudioUnitSetProperty(instance, kAudioUnitProperty_MaximumFramesPerSlice, kAudioUnitScope_Global, 0, &slice, 4) == noErr,
              AudioUnitSetProperty(instance, kAudioUnitProperty_StreamFormat, kAudioUnitScope_Input, 0, &format, formatSize) == noErr,
              AudioUnitSetProperty(instance, kAudioUnitProperty_StreamFormat, kAudioUnitScope_Output, 0, &format, formatSize) == noErr,
              AudioUnitSetParameter(instance, AudioUnitParameterID(kAUSoundIsolationParam_SoundToIsolate), kAudioUnitScope_Global, 0, kind, 0) == noErr
        else { AudioComponentInstanceDispose(instance); return nil }
        self.sampleRate = sampleRate
        unit = instance
        capacity = max(16_384, Int(sampleRate.rounded(.up)))
        mono = .allocate(capacity: capacity)
        processed = .allocate(capacity: capacity)
        description = format
        var callback = AURenderCallbackStruct(inputProc: { refCon, _, _, _, frames, data in
            let isolator = Unmanaged<VoiceIsolator>.fromOpaque(refCon).takeUnretainedValue()
            guard let data, let source = isolator.source,
                  let target = UnsafeMutableAudioBufferListPointer(data).first?.mData else { return kAudio_ParamError }
            target.assumingMemoryBound(to: Float.self).update(from: source, count: Int(frames))
            return noErr
        }, inputProcRefCon: Unmanaged.passUnretained(self).toOpaque())
        var seconds: Float64 = 0
        var size = UInt32(MemoryLayout<Float64>.size)
        guard AudioUnitSetProperty(unit, kAudioUnitProperty_SetRenderCallback, kAudioUnitScope_Input, 0, &callback,
                                   UInt32(MemoryLayout<AURenderCallbackStruct>.size)) == noErr,
              AudioUnitInitialize(unit) == noErr else {
            mono.deallocate(); processed.deallocate(); AudioComponentInstanceDispose(unit)
            return nil
        }
        _ = AudioUnitGetProperty(unit, kAudioUnitProperty_Latency, kAudioUnitScope_Global, 0, &seconds, &size)
        latency = seconds.isFinite && seconds >= 0 && seconds < 1 ? seconds : 0
        latencyFrames = Int((latency * sampleRate).rounded())
        trim = latencyFrames
    }

    deinit {
        AudioUnitUninitialize(unit)
        AudioComponentInstanceDispose(unit)
        mono.deallocate()
        processed.deallocate()
    }

    /// Ready for a new capture: nothing from the last one comes out. Microseconds.
    func reset() {
        _ = AudioUnitReset(unit, kAudioUnitScope_Global, 0)
        sampleTime = 0
        trim = latencyFrames
        offset = 0
    }

    /// Frames of input inside the unit that have not come out yet.
    var held: Int { latencyFrames - trim }

    /// Mixes `buffer` to mono and isolates the voice in it. The result is in
    /// `output` until the next call; it can be shorter than the buffer (even
    /// empty) while the unit fills at the start. Nil when it could not be
    /// processed: the caller keeps the unprocessed buffer.
    func process(_ buffer: AVAudioPCMBuffer) -> UInt32? {
        let frames = Int(buffer.frameLength)
        guard frames > 0, frames <= capacity, let channels = buffer.floatChannelData else { return nil }
        let count = Int(buffer.format.channelCount)
        let stride = buffer.stride
        let scale = 1 / Float(max(count, 1))
        for frame in 0..<frames {
            var sum: Float = 0
            for channel in 0..<count { sum += channels[channel][frame * stride] }
            mono[frame] = sum * scale
        }
        return render(frames)
    }

    /// Isolates `frames` of silence (up to `capacity`): what fills a gap the
    /// engine left, and what pushes the held audio out at the end.
    func processSilence(_ frames: Int) -> UInt32? {
        guard frames > 0, frames <= capacity else { return frames == 0 ? 0 : nil }
        mono.update(repeating: 0, count: frames)
        return render(frames)
    }

    /// The audio still inside the unit, at the end of a capture.
    func flush() -> UInt32? { processSilence(held) }

    private func render(_ frames: Int) -> UInt32? {
        var done = 0
        while done < frames {
            let length = min(Int(Self.slice), frames - done)
            source = UnsafePointer(mono.advanced(by: done))
            var list = AudioBufferList(mNumberBuffers: 1, mBuffers: AudioBuffer(
                mNumberChannels: 1, mDataByteSize: UInt32(length * 4), mData: UnsafeMutableRawPointer(processed.advanced(by: done))))
            var flags = AudioUnitRenderActionFlags()
            var time = AudioTimeStamp()
            time.mSampleTime = sampleTime
            time.mFlags = .sampleTimeValid
            let status = AudioUnitRender(unit, &flags, &time, 0, UInt32(length), &list)
            source = nil
            guard status == noErr else { return nil }
            // A unit may answer with its own buffer instead of filling ours.
            if let data = list.mBuffers.mData, data != UnsafeMutableRawPointer(processed.advanced(by: done)) {
                processed.advanced(by: done).update(from: data.assumingMemoryBound(to: Float.self), count: length)
            }
            sampleTime += Float64(length)
            done += length
        }
        // What came out first is the unit's own delay, not the input.
        offset = min(trim, frames)
        trim -= offset
        return UInt32(frames - offset)
    }

    /// The processed samples of the last successful call.
    var output: UnsafePointer<Float> { UnsafePointer(processed.advanced(by: offset)) }

    /// `output` as a one-buffer list, for the PCM queue.
    func withOutputList<R>(frames: UInt32, _ body: (UnsafePointer<AudioBufferList>) -> R) -> R {
        var list = AudioBufferList(mNumberBuffers: 1, mBuffers: AudioBuffer(
            mNumberChannels: 1, mDataByteSize: frames * 4, mData: UnsafeMutableRawPointer(processed.advanced(by: offset))))
        return withUnsafePointer(to: &list) { body($0) }
    }
}

/// Loaded isolators, kept between captures so starting or resuming a
/// recording never waits for the model: one idle unit per sample rate (at
/// most two rates), loaded on a background queue.
final class VoiceIsolatorPool: @unchecked Sendable {
    static let shared = VoiceIsolatorPool()
    enum Availability { case ready(VoiceIsolator), loading, unavailable }
    private let lock = NSLock()
    private let queue = DispatchQueue(label: "dev.cereal.desktop.voice-isolation-load", qos: .utility)
    private var idle: [Double: VoiceIsolator] = [:]
    private var loading: Set<Double> = []
    private var refused: Set<Double> = []

    /// Starts loading a unit for `sampleRate` unless one is loaded or loading.
    func prepare(sampleRate: Double) {
        guard sampleRate.isFinite, sampleRate > 0 else { return }
        let start: Bool = lock.withLock {
            guard idle[sampleRate] == nil, !loading.contains(sampleRate), !refused.contains(sampleRate) else { return false }
            loading.insert(sampleRate)
            return true
        }
        guard start else { return }
        queue.async { [self] in
            let isolator = VoiceIsolator.available ? VoiceIsolator(sampleRate: sampleRate) : nil
            lock.withLock {
                loading.remove(sampleRate)
                if let isolator { keep(isolator) } else { refused.insert(sampleRate) }
            }
        }
    }

    /// Never waits: a loaded unit (reset for a new capture), or why not.
    func take(sampleRate: Double) -> Availability {
        if !VoiceIsolator.available { return .unavailable }
        let found: VoiceIsolator? = lock.withLock { idle.removeValue(forKey: sampleRate) }
        if let found { found.reset(); return .ready(found) }
        if lock.withLock({ refused.contains(sampleRate) }) { return .unavailable }
        prepare(sampleRate: sampleRate)
        return .loading
    }

    /// Whether a unit for `sampleRate` is loaded and idle.
    func isLoaded(sampleRate: Double) -> Bool { lock.withLock { idle[sampleRate] != nil } }

    /// A capture is done with its unit; the next one at this rate reuses it.
    func giveBack(_ isolator: VoiceIsolator) {
        lock.withLock { keep(isolator) }
    }

    private func keep(_ isolator: VoiceIsolator) {
        guard idle[isolator.sampleRate] == nil else { return }
        if idle.count >= 2, let other = idle.keys.first { idle.removeValue(forKey: other) }
        idle[isolator.sampleRate] = isolator
    }
}

/// One microphone capture's audio on its way to the PCM queue, with or
/// without voice isolation. Every push goes through here, one at a time, so
/// the mic timeline stays exact:
/// - with isolation the unit's delay is taken off (see `VoiceIsolator`);
/// - before the unit is loaded the audio goes through raw, and it switches
///   over without moving the timeline;
/// - if the unit fails, the audio it held becomes silence of the same length;
/// - frames the engine dropped (its sample time jumped) become silence too,
///   and are counted in `gapFrames`.
final class MicIsolation: @unchecked Sendable {
    typealias Push = (UnsafePointer<AudioBufferList>, UInt32, AudioStreamBasicDescription) -> Void
    /// "off", "loading", "on", "unavailable" or "failed".
    private(set) var state: String { get { stats.withLock { stateValue } } set { stats.withLock { stateValue = newValue } } }
    private let lock = NSLock()
    private let stats = NSLock()
    private var stateValue: String
    private var latencyValue: Double = 0
    private var gapValue: UInt64 = 0
    private let sampleRate: Double
    private let raw: AudioStreamBasicDescription
    private let silenceFormat: AudioStreamBasicDescription
    private let zeros: UnsafeMutablePointer<Float>
    private static let zeroFrames = 8_192
    /// The longest engine gap made up with silence (2 s); longer ones are only counted.
    private let longestFill: Int64
    private var isolator: VoiceIsolator?
    private var nextSample: Int64?
    private var finished = false

    init(enabled: Bool, sampleRate: Double, raw: AudioStreamBasicDescription) {
        self.sampleRate = sampleRate
        self.raw = raw
        silenceFormat = VoiceIsolator.monoFormat(sampleRate)
        longestFill = Int64(sampleRate * 2)
        zeros = .allocate(capacity: Self.zeroFrames)
        zeros.initialize(repeating: 0, count: Self.zeroFrames)
        stateValue = enabled ? "loading" : "off"
        if enabled { adopt() }
    }

    deinit {
        if let isolator { VoiceIsolatorPool.shared.giveBack(isolator) }
        zeros.deallocate()
    }

    /// For the capture's statistics: state, delay taken off (ms) and frames
    /// the engine dropped.
    var status: (state: String, latencyMs: Double, gapFrames: UInt64) {
        stats.withLock { (stateValue, latencyValue * 1_000, gapValue) }
    }

    func deliver(_ buffer: AVAudioPCMBuffer, at when: AVAudioTime, push: Push) {
        lock.lock(); defer { lock.unlock() }
        guard !finished, buffer.frameLength > 0 else { return }
        if when.isSampleTimeValid {
            let start = when.sampleTime
            if let expected = nextSample, start > expected {
                let missing = start - expected
                stats.withLock { gapValue += UInt64(missing) }
                silence(Int(min(missing, longestFill)), push)
            }
            nextSample = start + Int64(buffer.frameLength)
        }
        if isolator == nil, state == "loading" { adopt() }
        if let isolator {
            if let frames = isolator.process(buffer) {
                if frames > 0 { isolator.withOutputList(frames: frames) { push($0, frames, isolator.description) } }
                return
            }
            fail(push)
        }
        push(buffer.audioBufferList, buffer.frameLength, raw)
    }

    /// The end of the capture: the audio still inside the unit comes out,
    /// and the unit goes back to the pool. Nothing is delivered afterwards.
    func finish(push: Push) {
        lock.lock(); defer { lock.unlock() }
        guard !finished else { return }
        finished = true
        guard let isolator else { return }
        let held = isolator.held
        if let frames = isolator.flush() {
            if frames > 0 { isolator.withOutputList(frames: frames) { push($0, frames, isolator.description) } }
            self.isolator = nil
            VoiceIsolatorPool.shared.giveBack(isolator)
        } else {
            self.isolator = nil
            pushZeros(held, push)
        }
    }

    /// Takes a loaded unit if there is one. Called under `lock` or from init.
    private func adopt() {
        switch VoiceIsolatorPool.shared.take(sampleRate: sampleRate) {
        case .ready(let found):
            isolator = found
            stats.withLock { stateValue = "on"; latencyValue = found.latency }
        case .unavailable: state = "unavailable"
        case .loading: break
        }
    }

    private func fail(_ push: Push) {
        guard let isolator else { return }
        // Its held input is lost; the same length of silence keeps the timeline.
        let held = isolator.held
        self.isolator = nil // A unit that failed is not reused.
        stats.withLock { stateValue = "failed"; latencyValue = 0 }
        pushZeros(held, push)
    }

    private func silence(_ frames: Int, _ push: Push) {
        var left = frames
        if let isolator {
            while left > 0 {
                let chunk = min(left, isolator.capacity)
                guard let out = isolator.processSilence(chunk) else { fail(push); break }
                if out > 0 { isolator.withOutputList(frames: out) { push($0, out, isolator.description) } }
                left -= chunk
            }
            if self.isolator != nil { return }
        }
        pushZeros(left, push)
    }

    private func pushZeros(_ frames: Int, _ push: Push) {
        var left = frames
        while left > 0 {
            let chunk = min(left, Self.zeroFrames)
            var list = AudioBufferList(mNumberBuffers: 1, mBuffers: AudioBuffer(
                mNumberChannels: 1, mDataByteSize: UInt32(chunk * 4), mData: UnsafeMutableRawPointer(zeros)))
            withUnsafePointer(to: &list) { push($0, UInt32(chunk), silenceFormat) }
            left -= chunk
        }
    }
}

/// Loads voice isolation for the default microphone ahead of the first
/// recording (at launch, when the setting is on), so starting never waits.
@_cdecl("mn_voice_isolation_prepare")
public func mn_voice_isolation_prepare() {
    var address = AudioObjectPropertyAddress(mSelector: kAudioHardwarePropertyDefaultInputDevice,
                                             mScope: kAudioObjectPropertyScopeGlobal, mElement: kAudioObjectPropertyElementMain)
    var device = AudioDeviceID(kAudioObjectUnknown)
    var size = UInt32(MemoryLayout<AudioDeviceID>.size)
    var rate: Float64 = 48_000
    if AudioObjectGetPropertyData(AudioObjectID(kAudioObjectSystemObject), &address, 0, nil, &size, &device) == noErr,
       device != kAudioObjectUnknown {
        address.mSelector = kAudioDevicePropertyNominalSampleRate
        var nominal: Float64 = 0
        size = UInt32(MemoryLayout<Float64>.size)
        if AudioObjectGetPropertyData(device, &address, 0, nil, &size, &nominal) == noErr, nominal > 0 { rate = nominal }
    }
    VoiceIsolatorPool.shared.prepare(sampleRate: rate)
}
