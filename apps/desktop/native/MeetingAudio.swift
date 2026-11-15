/*
Derived from OpenOats SystemAudioCapture.swift and MicCapture.swift.

MIT License

Copyright (c) 2026 OpenOats contributors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

*/

@preconcurrency import AVFoundation
import AudioToolbox
import CoreAudio
import Darwin
import Dispatch
import Foundation
import MeetingPCMQueue

private struct CaptureFailure: Error {
    var code: String
    var message: String
    var status: OSStatus = 0
    var json: [String: Any] { ["code": code, "message": message, "os_status": status] }
}

/// Private Float32/mono WAV writer. A valid bounded header exists before the first sample,
/// and lengths are checkpointed after every write so process termination leaves readable PCM.
private final class CheckpointedWAV {
    private let descriptor: Int32
    private var frames: UInt64 = 0
    private var lastSync = mn_pcm_uptime()
    private var closed = false
    private static let headerBytes: UInt64 = 56
    private static let maxFrames = (UInt64(UInt32.max) - 48) / 4

    init(url: URL, sampleRate: Double) throws {
        guard abs(sampleRate.rounded() - sampleRate) < 0.01 else {
            throw CaptureFailure(code: "invalid_format", message: "The device sample rate cannot be represented safely in a PCM WAV file.")
        }
        let fd = Darwin.open(url.path, O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW, S_IRUSR | S_IWUSR)
        guard fd >= 0 else {
            throw CaptureFailure(code: "output_file", message: "A new private audio file could not be created; existing files are never overwritten.", status: errno)
        }
        descriptor = fd
        var header = Data()
        func text(_ value: String) { header.append(contentsOf: value.utf8) }
        func word<T: FixedWidthInteger>(_ value: T) {
            var little = value.littleEndian
            withUnsafeBytes(of: &little) { header.append(contentsOf: $0) }
        }
        text("RIFF"); word(UInt32(48)); text("WAVEfmt "); word(UInt32(16))
        word(UInt16(3)); word(UInt16(1)); word(UInt32(sampleRate.rounded()))
        word(UInt32(sampleRate.rounded()) * 4); word(UInt16(4)); word(UInt16(32))
        text("fact"); word(UInt32(4)); word(UInt32(0)); text("data"); word(UInt32(0))
        do {
            try header.withUnsafeBytes { bytes in try writeAll(bytes.baseAddress!, count: bytes.count, offset: 0) }
        } catch {
            Darwin.close(fd)
            closed = true
            throw error
        }
    }

    deinit { try? close() }

    func write(_ samples: UnsafePointer<Float>, frames count: UInt32) throws {
        guard !closed, frames + UInt64(count) <= Self.maxFrames else {
            throw CaptureFailure(code: "file_limit", message: "The PCM WAV size limit was reached. Captured audio is kept; start a new capture segment.")
        }
        try writeAll(samples, count: Int(count) * 4, offset: off_t(Self.headerBytes + frames * 4))
        frames += UInt64(count)
        try checkpoint()
        let now = mn_pcm_uptime()
        if now - lastSync >= 1 {
            guard Darwin.fsync(descriptor) == 0 else { throw CaptureFailure(code: "write_failed", message: "Audio could not be synchronized to disk.", status: errno) }
            lastSync = now
        }
    }

    func close() throws {
        guard !closed else { return }
        var failure: (any Error)?
        do { try checkpoint() } catch { failure = error }
        if Darwin.fsync(descriptor) != 0, failure == nil {
            failure = CaptureFailure(code: "write_failed", message: "The final audio checkpoint could not be synchronized to disk.", status: errno)
        }
        if Darwin.close(descriptor) != 0, failure == nil {
            failure = CaptureFailure(code: "write_failed", message: "The captured audio file could not be closed cleanly.", status: errno)
        }
        closed = true
        if let failure { throw failure }
    }

    private func checkpoint() throws {
        func word(_ value: UInt32, offset: off_t) throws {
            var little = value.littleEndian
            try withUnsafeBytes(of: &little) { try writeAll($0.baseAddress!, count: 4, offset: offset) }
        }
        // Body bytes are already written. Every advertised length stays within real data.
        try word(UInt32(frames * 4), offset: 52)
        try word(UInt32(frames), offset: 44)
        try word(UInt32(48 + frames * 4), offset: 4)
    }

    private func writeAll(_ bytes: UnsafeRawPointer, count: Int, offset: off_t) throws {
        var done = 0
        while done < count {
            let amount = Darwin.pwrite(descriptor, bytes.advanced(by: done), count - done, offset + off_t(done))
            if amount < 0, errno == EINTR { continue }
            guard amount > 0 else { throw CaptureFailure(code: "write_failed", message: "Captured audio could not be written to disk.", status: errno) }
            done += amount
        }
    }
}

/// Each native callback has one producer; the dedicated writer is its sole consumer.
/// The callback retains this sink, so its preallocated ring cannot be freed underneath IO.
private final class PCMSink: @unchecked Sendable {
    let ring: OpaquePointer
    let sampleRate: Double
    let path: String
    private let queue = DispatchQueue(label: "dev.cereal.desktop.pcm-writer", qos: .utility)
    private let lock = NSLock()
    private var failure: String?
    private var timer: DispatchSourceTimer?
    private var file: CheckpointedWAV?
    private let buffer: UnsafeMutablePointer<Float>
    private var closed = false

    /// `discard` keeps levels and counters only: no file is created or written.
    init(url: URL, sampleRate: Double, discard: Bool = false) throws {
        guard sampleRate.isFinite, sampleRate >= 8_000, sampleRate <= 384_000 else {
            throw CaptureFailure(code: "invalid_format", message: "The audio device returned an unsupported sample rate.")
        }
        if !discard { self.file = try CheckpointedWAV(url: url, sampleRate: sampleRate) }
        guard let ring = mn_pcm_create(64, 8_192) else {
            throw CaptureFailure(code: "allocation", message: "Audio buffer storage could not be allocated.")
        }
        self.ring = ring
        self.sampleRate = sampleRate
        self.path = url.path
        self.buffer = .allocate(capacity: 8_192)
        let timer = DispatchSource.makeTimerSource(queue: queue)
        timer.schedule(deadline: .now(), repeating: .milliseconds(10), leeway: .milliseconds(2))
        timer.setEventHandler { [weak self] in self?.drain() }
        self.timer = timer
        timer.resume()
    }

    deinit { buffer.deallocate(); mn_pcm_destroy(ring) }

    var writeFailure: String? { lock.withLock { failure } }
    func closeInput() { mn_pcm_close(ring) }
    func stats() -> MNPCMStats { mn_pcm_stats(ring) }

    func push(_ data: UnsafePointer<AudioBufferList>, frames: UInt32, description: AudioStreamBasicDescription) {
        mn_pcm_push(ring, data, frames, description.mFormatFlags, description.mBitsPerChannel,
                    mn_pcm_uptime())
    }

    /// Invoked only after the native producer has stopped. No AVAudioFile on the callback thread.
    func finish() {
        closeInput()
        // Native IO is already stopped and conversion is bounded to 64 channels/65536 frames.
        // Wait for producer quiescence before the final drain; never close beneath a late push.
        while mn_pcm_callbacks_active(ring) > 0 { usleep(1_000) }
        queue.sync {
            guard !closed else { return }
            timer?.cancel()
            timer = nil
            drain()
            do { try file?.close() } catch {
                lock.withLock { failure = (error as? CaptureFailure)?.message ?? "The audio file could not be finalized cleanly." }
            }
            file = nil
            closed = true
        }
    }

    private func drain() {
        guard !closed else { return }
        while true {
            let count = mn_pcm_read(ring, buffer, 8_192)
            guard count > 0 else { return }
            guard writeFailure == nil else { mn_pcm_mark_discarded(ring, count); continue }
            do {
                try file?.write(buffer, frames: count)
                mn_pcm_mark_written(ring, count)
            } catch {
                lock.withLock { failure = (error as? CaptureFailure)?.message ?? "Captured audio could not be written to disk." }
                mn_pcm_mark_discarded(ring, count)
                closeInput()
            }
        }
    }
}

private struct ChannelState {
    var state = "disabled"
    var error: CaptureFailure?
    var sink: PCMSink?
    var retiredSinks: [PCMSink] = []
    var startedAt: Double = 0
    var selfExcluded = false
}

@available(macOS 14.2, *)
private final class NativeAudioSession: @unchecked Sendable {
    private let control = DispatchQueue(label: "dev.cereal.desktop.audio-control", qos: .userInitiated)
    private let systemCallbacks = DispatchQueue(label: "dev.cereal.desktop.system-callback", qos: .userInteractive)
    private let lock = NSLock()
    private let output: URL
    private let flags: UInt32
    /// A listening check: levels only, nothing written to disk.
    private let discard: Bool
    private var channels = [ChannelState(), ChannelState()]
    private var cancelled = false
    private var stopped = false
    private var monitor: DispatchSourceTimer?
    private var engine: AVAudioEngine?
    private var micTapInstalled = false
    private var micObserver: NSObjectProtocol?
    private var tapID: AudioObjectID = kAudioObjectUnknown
    private var aggregateID: AudioObjectID = kAudioObjectUnknown
    private var ioProc: AudioDeviceIOProcID?
    private var selectedOutput: AudioDeviceID = kAudioObjectUnknown
    private var lastOutputCheck: Double = 0
    private var lastActivityCheck = -Double.infinity
    private var systemWatchdog = SystemAudioWatchdog()
    // Only the control queue touches these counters. A HAL start can succeed
    // before its IO callback begins delivering buffers, so allow two bounded
    // startup-only recreations without restarting the independent microphone.
    private var systemStartupRestarts = 0
    private var systemSinkSequence = 0

    init(output: URL, flags: UInt32) {
        self.output = output
        self.flags = flags & 3
        self.discard = flags & 4 != 0
        for index in 0..<2 where flags & (1 << index) != 0 { channels[index].state = "starting" }
    }

    func begin() {
        control.async { [self] in
            guard !isCancelled else { return }
            do {
                if !discard {
                    try FileManager.default.createDirectory(at: output, withIntermediateDirectories: true,
                        attributes: [.posixPermissions: 0o700])
                    let attributes = try output.resourceValues(forKeys: [.isDirectoryKey, .isSymbolicLinkKey])
                    guard attributes.isDirectory == true, attributes.isSymbolicLink != true else {
                        throw CaptureFailure(code: "output_directory", message: "The session output must be a real directory.")
                    }
                }
            } catch {
                for index in 0..<2 where flags & (1 << index) != 0 {
                    fail(index, error as? CaptureFailure ?? CaptureFailure(code: "output_directory", message: "The audio output directory could not be created."))
                }
                return
            }
            if flags & 1 != 0 { startMicrophoneIfAuthorized() }
            if flags & 2 != 0, !isCancelled {
                do { try startSystem() } catch {
                    if !isCancelled { fail(1, error as? CaptureFailure ?? CaptureFailure(code: "system_start", message: "System audio could not start.")) }
                    stopSystem()
                }
            }
            if !isCancelled { installMonitor() }
        }
    }

    private var isCancelled: Bool { lock.withLock { cancelled } }

    func stop() {
        lock.withLock { cancelled = true }
        control.sync {
            guard !lock.withLock({ stopped }) else { return }
            monitor?.cancel()
            monitor = nil
            stopMicrophone()
            stopSystem()
            let sinks = lock.withLock { channels.flatMap { $0.retiredSinks + ($0.sink.map { [$0] } ?? []) } }
            sinks.forEach { $0.finish() }
            lock.withLock {
                for index in channels.indices where channels[index].state != "disabled" && channels[index].state != "failed" {
                    if let message = channels[index].sink?.writeFailure {
                        channels[index].state = "failed"
                        channels[index].error = CaptureFailure(code: "write_failed", message: message)
                    } else { channels[index].state = "stopped" }
                }
                stopped = true
            }
        }
    }

    func snapshot() -> [String: Any] {
        let (copy, stopped) = lock.withLock { (channels, self.stopped) }
        let requested = copy.filter { $0.state != "disabled" }
        let phase: String
        if stopped { phase = "stopped" }
        else if requested.allSatisfy({ $0.state == "failed" }) { phase = "failed" }
        else if requested.contains(where: { $0.state == "recording" }) {
            phase = requested.contains(where: { $0.state == "failed" }) ? "partial" : "recording"
        } else { phase = "starting" }
        func channel(_ state: ChannelState) -> [String: Any] {
            let stats = state.sink?.stats()
            let sinks = state.retiredSinks + (state.sink.map { [$0] } ?? [])
            let allStats = sinks.map { $0.stats() }
            return ["state": state.state, "error": state.error?.json as Any? ?? NSNull(),
                "level": state.state == "recording" ? (stats?.level ?? 0) : 0,
                "frames_received": allStats.reduce(UInt64(0)) { $0 + $1.frames_received },
                "frames_written": allStats.reduce(UInt64(0)) { $0 + $1.frames_written },
                "dropped_frames": allStats.reduce(UInt64(0)) { $0 + $1.dropped_frames }, "sample_rate": state.sink?.sampleRate ?? 0,
                "channels": 1, "path": state.sink?.path as Any? ?? NSNull(),
                "paths": sinks.map(\.path),
                "last_buffer_uptime": stats?.last_buffer_uptime ?? 0,
                "self_process_excluded": state.selfExcluded]
        }
        return ["version": 1, "phase": phase, "requested_channels": flags,
                "mic": channel(copy[0]), "system": channel(copy[1])]
    }

    private func fail(_ channel: Int, _ error: CaptureFailure) {
        lock.withLock { channels[channel].state = "failed"; channels[channel].error = error }
    }

    private func register(_ sink: PCMSink, for channel: Int, selfExcluded: Bool = false) {
        lock.withLock {
            if let previous = channels[channel].sink { channels[channel].retiredSinks.append(previous) }
            channels[channel].sink = sink
            channels[channel].startedAt = mn_pcm_uptime()
            channels[channel].selfExcluded = selfExcluded
        }
    }

    private func startMicrophoneIfAuthorized() {
        guard !isCancelled else { return }
        var address = property(kAudioHardwarePropertyDefaultInputDevice)
        var input = AudioDeviceID(kAudioObjectUnknown)
        var size = UInt32(MemoryLayout<AudioDeviceID>.size)
        guard AudioObjectGetPropertyData(AudioObjectID(kAudioObjectSystemObject), &address, 0, nil, &size, &input) == noErr,
              input != kAudioObjectUnknown else {
            fail(0, CaptureFailure(code: "no_input", message: "No microphone input device is available. System capture can continue independently."))
            return
        }
        switch AVCaptureDevice.authorizationStatus(for: .audio) {
        case .authorized: startMicrophone()
        case .notDetermined:
            AVCaptureDevice.requestAccess(for: .audio) { [weak self] allowed in
                guard let self else { return }
                self.control.async { [self] in
                    guard !isCancelled else { return } // Late Allow cannot resurrect a stopped session.
                    if allowed { startMicrophone() }
                    else { fail(0, CaptureFailure(code: "permission_denied", message: "Microphone access was denied. Enable it in System Settings to capture your microphone.")) }
                }
            }
        default:
            fail(0, CaptureFailure(code: "permission_denied", message: "Microphone access is unavailable. Check Privacy & Security → Microphone."))
        }
    }

    private func startMicrophone() {
        guard !isCancelled else { return }
        do {
            let engine = AVAudioEngine()
            let input = engine.inputNode
            let native = input.outputFormat(forBus: 0)
            guard native.sampleRate > 0, native.channelCount > 0 else {
                throw CaptureFailure(code: "no_input", message: "The microphone has no usable audio channels.")
            }
            // OpenOats prefers a standard Float32 format, falling back to the hardware format.
            let format = AVAudioFormat(standardFormatWithSampleRate: native.sampleRate, channels: native.channelCount) ?? native
            let sink = try PCMSink(url: output.appendingPathComponent("mic.wav"), sampleRate: format.sampleRate, discard: discard)
            register(sink, for: 0)
            let description = format.streamDescription.pointee
            input.installTap(onBus: 0, bufferSize: 4_096, format: format) { buffer, _ in
                sink.push(buffer.audioBufferList, frames: buffer.frameLength, description: description)
            }
            self.engine = engine
            micTapInstalled = true
            engine.prepare()
            try engine.start()
            guard !isCancelled else { stopMicrophone(); return }
            micObserver = NotificationCenter.default.addObserver(forName: .AVAudioEngineConfigurationChange, object: engine, queue: nil) { [weak self] _ in
                guard let self else { return }
                self.control.async { [self] in
                    guard !isCancelled, self.engine != nil else { return }
                    fail(0, CaptureFailure(code: "input_changed", message: "The microphone configuration changed. Captured audio is kept; stop and retry to use the new input."))
                    stopMicrophone()
                }
            }
        } catch {
            fail(0, error as? CaptureFailure ?? CaptureFailure(code: "mic_start", message: "The microphone engine could not start. Check input availability and permission."))
            stopMicrophone()
        }
    }

    private func stopMicrophone() {
        let sink = lock.withLock { channels[0].sink }
        sink?.closeInput()
        if let observer = micObserver { NotificationCenter.default.removeObserver(observer); micObserver = nil }
        if let engine {
            if micTapInstalled { engine.inputNode.removeTap(onBus: 0); micTapInstalled = false }
            engine.stop()
            engine.reset()
        }
        engine = nil
        sink?.finish()
    }

    private func startSystem() throws {
        var last = CaptureFailure(code: "tap_format", message: "The system audio tap format did not become available.", status: kAudioHardwareBadObjectError)
        for attempt in 0..<3 {
            try checkCancelled()
            if attempt > 0 { try sleepCancellable(milliseconds: 200) }
            stopSystem()
            do {
                selectedOutput = try defaultOutput()
                let uid = try outputUID(selectedOutput)
                let ownProcess = try ownProcessObject()
                let uuid = UUID()
                // Derived from OpenOats makeTapDescription: device-bound stream 0, mono mixdown,
                // private exclusive tap, unmuted output, excluding our own current HAL process.
                let tapDescription = CATapDescription()
                tapDescription.name = "Cereal System Audio"
                tapDescription.uuid = uuid
                tapDescription.processes = [ownProcess]
                tapDescription.isPrivate = true
                tapDescription.muteBehavior = .unmuted
                tapDescription.isMixdown = true
                tapDescription.isMono = true
                tapDescription.isExclusive = true
                tapDescription.deviceUID = uid
                tapDescription.stream = 0
                var status = AudioHardwareCreateProcessTap(tapDescription, &tapID)
                guard status == noErr, tapID != kAudioObjectUnknown else {
                    throw CaptureFailure(code: "tap_create", message: "System audio capture could not create a private process tap. Check System Audio Recording permission.", status: status == noErr ? kAudioHardwareBadObjectError : status)
                }
                try checkCancelled()
                let aggregate: [String: Any] = [
                    kAudioAggregateDeviceNameKey: "Cereal System Audio",
                    kAudioAggregateDeviceUIDKey: UUID().uuidString,
                    kAudioAggregateDeviceIsPrivateKey: true,
                    kAudioAggregateDeviceTapAutoStartKey: true,
                    kAudioAggregateDeviceTapListKey: [[kAudioSubTapUIDKey: uuid.uuidString,
                        kAudioSubTapDriftCompensationKey: true]],
                ]
                status = AudioHardwareCreateAggregateDevice(aggregate as CFDictionary, &aggregateID)
                guard status == noErr, aggregateID != kAudioObjectUnknown else {
                    throw CaptureFailure(code: "aggregate_create", message: "A private system-audio aggregate device could not be created.", status: status)
                }
                let description = try tapFormat()
                guard description.mFormatID == kAudioFormatLinearPCM, description.mSampleRate > 0,
                      description.mBytesPerFrame > 0, description.mChannelsPerFrame > 0 else {
                    throw CaptureFailure(code: "invalid_format", message: "The system tap did not provide a usable PCM format.")
                }
                // The monitor may re-enter startSystem after a zero-frame start.
                // Never reuse an earlier WAV path, including header-only files.
                let sequence = systemSinkSequence
                systemSinkSequence += 1
                let name = sequence == 0 ? "system.wav" : "system-retry-\(sequence).wav"
                let sink = try PCMSink(url: output.appendingPathComponent(name), sampleRate: description.mSampleRate, discard: discard)
                register(sink, for: 1, selfExcluded: true)
                status = AudioDeviceCreateIOProcIDWithBlock(&ioProc, aggregateID, systemCallbacks) { _, input, _, _, _ in
                    let buffers = UnsafeMutableAudioBufferListPointer(UnsafeMutablePointer(mutating: input))
                    guard let first = buffers.first else { return }
                    let frames = first.mDataByteSize / description.mBytesPerFrame
                    sink.push(input, frames: frames, description: description)
                }
                guard status == noErr, ioProc != nil else {
                    throw CaptureFailure(code: "io_proc", message: "The system audio IO callback could not be installed.", status: status)
                }
                try checkCancelled()
                status = AudioDeviceStart(aggregateID, ioProc)
                guard status == noErr else {
                    throw CaptureFailure(code: "system_start", message: "The system audio device could not start. Check System Audio Recording permission.", status: status)
                }
                try checkCancelled()
                systemWatchdog.reset(now: mn_pcm_uptime())
                lastActivityCheck = -Double.infinity
                return
            } catch {
                stopSystem()
                if isCancelled { throw CancellationError() }
                guard let failure = error as? CaptureFailure else { throw error }
                last = failure
                // Full-cycle retry only for HAL registration races, matching OpenOats.
                guard failure.status == kAudioHardwareBadObjectError else { throw failure }
            }
        }
        throw last
    }

    private func stopSystem() {
        let sink = lock.withLock { channels[1].sink }
        sink?.closeInput()
        if aggregateID != kAudioObjectUnknown {
            if let ioProc {
                _ = AudioDeviceStop(aggregateID, ioProc)
                _ = AudioDeviceDestroyIOProcID(aggregateID, ioProc)
            }
            ioProc = nil
            _ = AudioHardwareDestroyAggregateDevice(aggregateID)
            aggregateID = kAudioObjectUnknown
        }
        systemCallbacks.sync {} // Previously queued blocks finish before the writer's final drain.
        if tapID != kAudioObjectUnknown { _ = AudioHardwareDestroyProcessTap(tapID); tapID = kAudioObjectUnknown }
        sink?.finish()
    }

    private func tapFormat() throws -> AudioStreamBasicDescription {
        var address = property(kAudioTapPropertyFormat)
        var format = AudioStreamBasicDescription()
        var status: OSStatus = kAudioHardwareBadObjectError
        for _ in 0..<40 {
            try checkCancelled()
            var size = UInt32(MemoryLayout<AudioStreamBasicDescription>.size)
            status = AudioObjectGetPropertyData(tapID, &address, 0, nil, &size, &format)
            if status == noErr { return format }
            if status != kAudioHardwareBadObjectError { break }
            try sleepCancellable(milliseconds: 75)
        }
        throw CaptureFailure(code: "tap_format", message: "The process tap format did not become available within its bounded retry window.", status: status)
    }

    private func ownProcessObject() throws -> AudioObjectID {
        var pid = getpid()
        var address = property(kAudioHardwarePropertyTranslatePIDToProcessObject)
        for _ in 0..<20 {
            try checkCancelled()
            var object = AudioObjectID(kAudioObjectUnknown)
            var size = UInt32(MemoryLayout<AudioObjectID>.size)
            let status = withUnsafePointer(to: &pid) {
                AudioObjectGetPropertyData(AudioObjectID(kAudioObjectSystemObject), &address, UInt32(MemoryLayout<pid_t>.size), $0, &size, &object)
            }
            if status == noErr, object != kAudioObjectUnknown { return object }
            try sleepCancellable(milliseconds: 25)
        }
        throw CaptureFailure(code: "self_exclusion", message: "Our audio process could not be identified safely. System capture was not started without self-exclusion.")
    }

    private func defaultOutput() throws -> AudioDeviceID {
        var address = property(kAudioHardwarePropertyDefaultOutputDevice)
        var device = AudioDeviceID(kAudioObjectUnknown)
        var size = UInt32(MemoryLayout<AudioDeviceID>.size)
        let status = AudioObjectGetPropertyData(AudioObjectID(kAudioObjectSystemObject), &address, 0, nil, &size, &device)
        guard status == noErr, device != kAudioObjectUnknown else {
            throw CaptureFailure(code: "no_output", message: "No system audio output device is available.", status: status)
        }
        return device
    }

    /// HAL process activity is device-scoped and excludes this process, just like
    /// the tap itself. This runs on the control queue, never on the IO callback.
    /// A disappearing/unsupported process is unknown, not proof of silence.
    private func externalOutputActive() -> Bool? {
        func scalar<T: FixedWidthInteger>(_ object: AudioObjectID, _ selector: AudioObjectPropertySelector, initial: T) -> T? {
            var address = property(selector)
            var value = initial
            var size = UInt32(MemoryLayout<T>.size)
            let status = withUnsafeMutableBytes(of: &value) {
                AudioObjectGetPropertyData(object, &address, 0, nil, &size, $0.baseAddress!)
            }
            guard status == noErr, size == MemoryLayout<T>.size else { return nil }
            return value
        }
        func objects(_ object: AudioObjectID, _ selector: AudioObjectPropertySelector,
                     scope: AudioObjectPropertyScope = kAudioObjectPropertyScopeGlobal) -> [AudioObjectID]? {
            var address = property(selector)
            address.mScope = scope
            var size: UInt32 = 0
            guard AudioObjectGetPropertyDataSize(object, &address, 0, nil, &size) == noErr,
                  size <= 1_048_576,
                  size % UInt32(MemoryLayout<AudioObjectID>.size) == 0 else { return nil }
            if size == 0 { return [] }
            var values = [AudioObjectID](repeating: 0, count: Int(size) / MemoryLayout<AudioObjectID>.size)
            let status = values.withUnsafeMutableBytes {
                AudioObjectGetPropertyData(object, &address, 0, nil, &size, $0.baseAddress!)
            }
            guard status == noErr, Int(size) <= values.count * MemoryLayout<AudioObjectID>.size else { return nil }
            return Array(values.prefix(Int(size) / MemoryLayout<AudioObjectID>.size))
        }
        guard let processes = objects(AudioObjectID(kAudioObjectSystemObject), kAudioHardwarePropertyProcessObjectList) else { return nil }
        var unknown = false
        for process in processes {
            guard let pid = scalar(process, kAudioProcessPropertyPID, initial: pid_t(0)) else { unknown = true; continue }
            if pid == getpid() { continue }
            guard let running = scalar(process, kAudioProcessPropertyIsRunningOutput, initial: UInt32(0)) else { unknown = true; continue }
            if running == 0 { continue }
            guard let devices = objects(process, kAudioProcessPropertyDevices, scope: kAudioObjectPropertyScopeOutput) else { unknown = true; continue }
            if devices.contains(selectedOutput) { return true }
        }
        return unknown ? nil : false
    }

    private func outputUID(_ device: AudioDeviceID) throws -> String {
        var address = property(kAudioDevicePropertyDeviceUID)
        var uid: Unmanaged<CFString>?
        var size = UInt32(MemoryLayout<Unmanaged<CFString>?>.size)
        let status = AudioObjectGetPropertyData(device, &address, 0, nil, &size, &uid)
        guard status == noErr, let uid else {
            throw CaptureFailure(code: "output_uid", message: "The default output device could not be identified.", status: status)
        }
        return uid.takeUnretainedValue() as String
    }

    private func property(_ selector: AudioObjectPropertySelector) -> AudioObjectPropertyAddress {
        AudioObjectPropertyAddress(mSelector: selector, mScope: kAudioObjectPropertyScopeGlobal, mElement: kAudioObjectPropertyElementMain)
    }

    private func checkCancelled() throws { if isCancelled { throw CancellationError() } }
    private func sleepCancellable(milliseconds: Int) throws {
        var remaining = milliseconds
        while remaining > 0 {
            try checkCancelled()
            let chunk = min(10, remaining)
            usleep(useconds_t(chunk * 1_000))
            remaining -= chunk
        }
        try checkCancelled()
    }

    private func installMonitor() {
        let timer = DispatchSource.makeTimerSource(queue: control)
        timer.schedule(deadline: .now(), repeating: .milliseconds(200), leeway: .milliseconds(20))
        timer.setEventHandler { [weak self] in self?.monitorChannels() }
        monitor = timer
        timer.resume()
    }

    private func monitorChannels() {
        guard !isCancelled else { return }
        let now = mn_pcm_uptime()
        let snapshot = lock.withLock { channels }
        if (snapshot[1].state == "starting" || snapshot[1].state == "recording"), now - lastActivityCheck >= 1 {
            lastActivityCheck = now
            systemWatchdog.observe(outputActive: externalOutputActive(), now: now)
        }
        for index in 0..<2 where snapshot[index].state == "starting" || snapshot[index].state == "recording" {
            guard let sink = snapshot[index].sink else { continue } // Pending mic consent is not active capture.
            let stats = sink.stats()
            var failure: CaptureFailure?
            if let message = sink.writeFailure { failure = CaptureFailure(code: "write_failed", message: message) }
            else if stats.invalid_format != 0 { failure = CaptureFailure(code: "invalid_format", message: "The capture device changed to an unsupported PCM format.") }
            else if stats.frames_received == 0,
                    index == 1 ? systemWatchdog.timedOut(framesReceived: 0, startedAt: snapshot[index].startedAt, lastBufferAt: 0, now: now) : now - snapshot[index].startedAt > 10 {
                failure = CaptureFailure(code: "no_frames", message: index == 0 ? "No microphone frames arrived. Check microphone access and the input device." : "No system audio frames arrived. Check System Audio Recording permission and the default output device.")
            } else if stats.frames_received > 0,
                      index == 1 ? systemWatchdog.timedOut(framesReceived: stats.frames_received, startedAt: snapshot[index].startedAt, lastBufferAt: stats.last_buffer_uptime, now: now) : now - stats.last_buffer_uptime > 4 {
                failure = CaptureFailure(code: "capture_stalled", message: "Audio capture stopped delivering frames. Captured audio is kept; stop and retry.")
            } else if stats.frames_received > 0 {
                lock.withLock { channels[index].state = "recording" }
            }
            if let failure {
                if index == 1, failure.code == "no_frames", systemStartupRestarts < 2 {
                    systemStartupRestarts += 1
                    stopSystem()
                    guard !isCancelled else { return }
                    lock.withLock { channels[1].state = "starting"; channels[1].error = nil }
                    do {
                        try sleepCancellable(milliseconds: 200)
                        try startSystem()
                    } catch {
                        if !isCancelled {
                            fail(1, error as? CaptureFailure ?? CaptureFailure(code: "system_start", message: "System audio could not restart after its bounded startup retry."))
                        }
                        stopSystem()
                    }
                    continue
                }
                fail(index, failure)
                if index == 0 { stopMicrophone() } else { stopSystem() }
            }
        }
        if selectedOutput != kAudioObjectUnknown, now - lastOutputCheck >= 1,
           lock.withLock({ channels[1].state == "recording" || channels[1].state == "starting" }) {
            lastOutputCheck = now
            if (try? defaultOutput()) != selectedOutput {
                fail(1, CaptureFailure(code: "output_changed", message: "The system output device changed. Captured audio is kept; stop and retry to use the new output."))
                stopSystem()
            }
        }
    }
}

private func copyCString(_ string: String, to buffer: UnsafeMutablePointer<CChar>?, capacity: Int) -> Int {
    let bytes = Array(string.utf8CString)
    if let buffer, capacity > 0 {
        if capacity >= bytes.count { _ = bytes.withUnsafeBufferPointer { memcpy(buffer, $0.baseAddress!, bytes.count) } }
        else { buffer[0] = 0 }
    }
    return bytes.count
}

@_cdecl("mn_audio_start")
public func mn_audio_start(_ path: UnsafePointer<CChar>?, _ flags: UInt32,
                           _ error: UnsafeMutablePointer<CChar>?, _ errorCapacity: Int) -> UnsafeMutableRawPointer? {
    _ = copyCString("", to: error, capacity: errorCapacity)
    guard #available(macOS 14.2, *) else {
        _ = copyCString("Core Audio process tap capture requires macOS 14.2 or later.", to: error, capacity: errorCapacity)
        return nil
    }
    guard let path, let directory = String(validatingCString: path), !directory.isEmpty,
          directory.utf8.count <= 4_096, flags & 3 != 0, flags & ~UInt32(7) == 0 else {
        _ = copyCString("A valid output directory and microphone/system channel flags are required.", to: error, capacity: errorCapacity)
        return nil
    }
    let session = NativeAudioSession(output: URL(fileURLWithPath: directory, isDirectory: true), flags: flags)
    let handle = Unmanaged.passRetained(session).toOpaque()
    session.begin()
    return handle
}

@_cdecl("mn_audio_poll")
public func mn_audio_poll(_ handle: UnsafeMutableRawPointer?, _ output: UnsafeMutablePointer<CChar>?, _ capacity: Int) -> Int {
    guard #available(macOS 14.2, *), let handle else { return 0 }
    let session = Unmanaged<NativeAudioSession>.fromOpaque(handle).takeUnretainedValue()
    guard let data = try? JSONSerialization.data(withJSONObject: session.snapshot(), options: [.sortedKeys]),
          let string = String(data: data, encoding: .utf8) else { return 0 }
    return copyCString(string, to: output, capacity: capacity)
}

@_cdecl("mn_audio_stop")
public func mn_audio_stop(_ handle: UnsafeMutableRawPointer?, _ output: UnsafeMutablePointer<CChar>?, _ capacity: Int) -> Int {
    guard #available(macOS 14.2, *), let handle else { return 0 }
    Unmanaged<NativeAudioSession>.fromOpaque(handle).takeUnretainedValue().stop()
    return mn_audio_poll(handle, output, capacity)
}

@_cdecl("mn_audio_destroy")
public func mn_audio_destroy(_ handle: UnsafeMutableRawPointer?) {
    guard #available(macOS 14.2, *), let handle else { return }
    let reference = Unmanaged<NativeAudioSession>.fromOpaque(handle)
    reference.takeUnretainedValue().stop()
    reference.release()
}

/// What macOS says about a capture permission, without asking for it.
/// Channel 0, the microphone: the `AVAuthorizationStatus` (0 not determined,
/// 1 restricted, 2 denied, 3 authorised). Channel 1, system audio: 0 granted,
/// 1 denied, 2 not asked. macOS has no public call for the second, so its
/// own preflight is looked up at run time and only ever read; -1 when it is
/// not there, and the caller then goes by what a listening check hears.
@_cdecl("mn_permission_status")
public func mn_permission_status(_ channel: UInt32) -> Int32 {
    if channel == 0 { return Int32(AVCaptureDevice.authorizationStatus(for: .audio).rawValue) }
    typealias Preflight = @convention(c) (CFString, CFDictionary?) -> Int
    guard let library = dlopen("/System/Library/PrivateFrameworks/TCC.framework/Versions/A/TCC", RTLD_NOW),
          let symbol = dlsym(library, "TCCAccessPreflight") else { return -1 }
    let status = unsafeBitCast(symbol, to: Preflight.self)("kTCCServiceAudioCapture" as CFString, nil)
    return (0...2).contains(status) ? Int32(status) : 2
}
