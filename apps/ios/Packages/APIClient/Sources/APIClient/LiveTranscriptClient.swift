import Foundation
import Contracts

// MARK: - Seams

public enum LiveSocketMessage: Sendable {
    case text(String)
    case data(Data)
}

/// Why a socket could not be opened, or stopped working.
public enum LiveSocketError: Error, Equatable, Sendable {
    /// The server answered the upgrade request with a plain HTTP status.
    case refused(status: Int)
    /// The socket was closed with this code.
    case closed(code: Int)
    case transport
}

public protocol LiveSocket: Sendable {
    func send(text: String) async throws
    func send(data: Data) async throws
    /// Throws `LiveSocketError` once the socket is closed, including by `close`.
    func receive() async throws -> LiveSocketMessage
    func ping() async throws
    func close(code: Int)
}

public protocol LiveSocketConnector: Sendable {
    func connect() async throws -> any LiveSocket
}

/// The audio a live stream is fed from: a capture addressed by sample index.
/// The capture file is the buffer, so nothing is queued in memory.
public protocol LiveAudioSource: Sendable {
    func availableFrames() -> Int64
    func read(from frame: Int64, maxFrames: Int) throws -> Data
}

extension CaptureFileReader: LiveAudioSource {}

// MARK: - Events

/// Why the live stream of this capture has ended for good. It is the only
/// transcript a recording gets, so these are only the refusals that dialling
/// again cannot change. An outage, a limit or a server that is not ready is
/// retried for as long as the recording runs.
public enum LiveUnavailableReason: Equatable, Sendable {
    case unauthorized
    case notFound
    case notRecording
    case replaced
    case protocolError
}

public enum LiveTranscriptStatus: Equatable, Sendable {
    /// Not started.
    case off
    case connecting
    case live
    /// Sending or transcribing audio older than now. `lagMs` is the audio this device
    /// has still to send, in whole seconds; 0 when only the server is behind.
    case catchingUp(lagMs: Int)
    /// The server is not transcribing right now: its speech provider is not
    /// answering, or it turned the stream away for a while (a limit, an
    /// outage). The capture keeps growing and is sent when it is back.
    case providerUnavailable
    case paused
    case reconnecting
    /// Final for this recording.
    case unavailable(LiveUnavailableReason)
    case ended
}

/// A stretch of the capture, on the note timeline, that this device did not
/// send and that therefore has no transcript.
public struct LiveSkip: Equatable, Sendable {
    public var channel: LiveWire.Channel
    public var fromMs: Int
    public var toMs: Int
    public init(channel: LiveWire.Channel, fromMs: Int, toMs: Int) { self.channel = channel; self.fromMs = fromMs; self.toMs = toMs }
}

public enum LiveTranscriptEvent: Sendable {
    /// A connection is established. `transcriptRevision` is the note's revision on the server.
    case ready(transcriptRevision: Int, provider: String, model: String)
    case interim(LiveWire.Interim)
    case final(LiveWire.Final)
    /// Merges first, then relabels of stored segments by id.
    case speakers([LiveWire.Speakers.Merge], relabels: [LiveWire.Speakers.Relabel] = [])
    /// The server will not transcribe this stretch.
    case gap(LiveWire.Gap)
    /// This device did not send this stretch: a backlog longer than the
    /// server's resume window, or audio the server refused once it was ending.
    case skipped(LiveSkip)
    case status(LiveTranscriptStatus)
}

// MARK: - Pacing

/// Limits how fast audio is sent, a backlog included and also once the
/// recording is ending. Live audio arrives at real time and never reaches the
/// limit; a backlog goes out at `factor` times real time. At most
/// `burstSeconds` of audio can go out at once on top of that rate.
/// (A provider handed a backlog in one burst drops most of it, and the server
/// would then hold that audio as sent.)
struct LiveSendBudget: Sendable {
    let samplesPerSecond: Double
    let burst: Double
    private(set) var samples: Double

    init(sampleRate: Int, factor: Double, burstSeconds: Double = 0.5) {
        samplesPerSecond = Double(sampleRate) * max(1, factor)
        burst = Double(sampleRate) * burstSeconds
        samples = burst
    }

    mutating func refill(seconds: Double) {
        samples = min(burst, samples + max(0, seconds) * samplesPerSecond)
    }

    mutating func take(_ count: Int) -> Bool {
        guard Double(count) <= samples else { return false }
        samples -= Double(count)
        return true
    }
}

// MARK: - Client

/// Streams one capture to the live transcription socket and reports what the
/// server says about it.
///
/// The client owns a cursor into the capture, a sample index. It sends frames
/// from the cursor, so a slow or absent connection only lets the cursor fall
/// behind the end of the capture; nothing is dropped and at most one frame is
/// in memory. After a drop it reconnects with back-off and tells the server
/// where its acknowledgements ended; the server answers with the sample it
/// wants next. The capture file holds every sample since the capture began,
/// so any backlog can be read again. A backlog within the server's resume
/// window is replayed at three times real time, never faster, also once the
/// recording is ending. A longer one is cut, because the server would not
/// take it: the client replays the newest nine tenths of the window, and the
/// older part, which has no transcript, is reported as `skipped`.
///
/// This stream is the recording's only transcript. Nothing else uploads or
/// transcribes the audio, so it is never given up while the recording runs,
/// except for the refusals in `terminal`. `end` does not give up either: it
/// returns once the server has confirmed every sample of the capture
/// (`delivered`), or when the server refuses the stream for good.
public actor LiveTranscriptClient {
    public struct Configuration: Sendable {
        /// Stable for one capture; the same value marks a reconnect.
        public var captureID: String
        public var channel: LiveWire.Channel
        public var sampleRate: Int
        /// Note-timeline position of sample 0 of the capture.
        public var baseMs: Int
        public var language: String?
        /// Frames carry 100 to 200 ms of audio.
        public var minFrameSamples: Int
        public var maxFrameSamples: Int
        public var tick: Duration
        public var catchUpFactor: Double
        /// Audio that may go out at once on top of `catchUpFactor`, in seconds.
        public var burstSeconds: Double
        /// Waits between dials after failures in a row. The last is repeated for
        /// as long as the recording runs.
        public var backoff: [Duration]
        /// Once the recording is ending, someone is waiting for the notes: the
        /// waits between dials are never longer than this.
        public var endingBackoffCap: Duration
        /// Where to continue when a backlog is skipped: this far before the end of the capture.
        public var tailAfterSkipMs: Int
        /// How far past the server's resume window a connected stream may fall before it skips.
        public var skipSlackMs: Int
        public var handshakeTimeout: Duration
        /// How long to wait, after `mark: end`, for the server to store its last
        /// text and close the connection. A connection that stays open longer is
        /// dropped and dialled again, and the next `ready` says what is missing.
        public var endGrace: Duration
        public var pingAfterIdle: Duration
        /// How long the server must report itself behind before that is worth saying.
        public var serverBehindNotice: Duration

        public init(captureID: String, baseMs: Int, channel: LiveWire.Channel = .mic,
                    sampleRate: Int = ContinuousCapture.sampleRate, language: String? = nil) {
            self.captureID = captureID
            self.baseMs = baseMs
            self.channel = channel
            self.sampleRate = sampleRate
            self.language = language
            minFrameSamples = sampleRate / 10
            maxFrameSamples = sampleRate / 5
            tick = .milliseconds(100)
            // Three times real time with half a second of burst: a backlog shrinks by 2 s a second,
            // and the provider is never handed audio faster than it transcribes it.
            catchUpFactor = 3
            burstSeconds = 0.5
            backoff = [.milliseconds(500), .seconds(1), .seconds(2), .seconds(4), .seconds(8), .seconds(16), .seconds(30)]
            endingBackoffCap = .seconds(5)
            tailAfterSkipMs = 5_000
            skipSlackMs = 5_000
            handshakeTimeout = .seconds(10)
            endGrace = .seconds(15)
            pingAfterIdle = .seconds(15)
            serverBehindNotice = .seconds(3)
        }
    }

    /// Counters for tests and diagnostics.
    public struct Statistics: Equatable, Sendable {
        public var connections = 0
        public var framesSent = 0
        public var samplesSent: Int64 = 0
        /// Times a backlog was skipped instead of replayed.
        public var skips = 0
        /// Exactly what was not sent, as sample ranges of the capture.
        public var skippedSamples: [Range<Int64>] = []
        public var marksSent: [LiveWire.MarkKind] = []
    }

    public nonisolated let events: AsyncStream<LiveTranscriptEvent>
    private let continuation: AsyncStream<LiveTranscriptEvent>.Continuation
    private let connector: any LiveSocketConnector
    private let source: any LiveAudioSource
    private let configuration: Configuration

    private enum Phase: Equatable {
        case running
        case paused(at: Int64)
        case ending(at: Int64)
    }

    /// Next sample to send.
    public private(set) var cursor: Int64 = 0
    /// The server's last `ack.throughSample`: one past the last sample it has taken.
    public private(set) var acknowledged: Int64 = 0
    public private(set) var statistics = Statistics()
    public private(set) var status: LiveTranscriptStatus = .off
    private var phase: Phase = .running
    private var resumeAt: Int64?
    /// The last mark the server was sent, on any connection, was a pause.
    private var serverPaused = false
    /// This connection has been told about the current pause.
    private var pauseMarked = false
    private var endMarkSent = false
    private var resumeWindowMs = 30_000
    /// True once the server has confirmed every sample of the ended capture:
    /// it closed the connection in order after `mark: end`, with its
    /// acknowledgements at the end of the capture.
    public private(set) var delivered = false
    /// Shared by every connection, so a reconnect never adds a burst of its own.
    private var lastRefill: ContinuousClock.Instant?
    private var serverState: LiveWire.StreamState?
    private var serverBehindSince: ContinuousClock.Instant?
    private var lastServerError: LiveWire.ServerError?
    private var budget: LiveSendBudget
    private var runTask: Task<Void, Never>?
    /// The wait between two dials. `end` cuts it short.
    private var backoffWait: Task<Void, Never>?
    private var socket: (any LiveSocket)?
    private var everConnected = false
    private var finished = false

    public init(connector: any LiveSocketConnector, source: any LiveAudioSource, configuration: Configuration) {
        self.connector = connector
        self.source = source
        self.configuration = configuration
        budget = LiveSendBudget(sampleRate: configuration.sampleRate, factor: configuration.catchUpFactor, burstSeconds: configuration.burstSeconds)
        let stream = AsyncStream.makeStream(of: LiveTranscriptEvent.self, bufferingPolicy: .unbounded)
        events = stream.stream
        continuation = stream.continuation
    }

    // MARK: Control

    public func start() {
        guard runTask == nil, !finished else { return }
        runTask = Task { await self.run() }
    }

    /// The capture stopped growing at `sample` (the number of samples it holds).
    public func pause(atSample sample: Int64) {
        guard case .running = phase, !finished else { return }
        phase = .paused(at: sample)
        resumeAt = nil
        pauseMarked = false
    }

    public func resume() {
        guard case .paused(let sample) = phase, !finished else { return }
        phase = .running
        resumeAt = sample
    }

    /// The capture is complete at `sample`: the number of samples it holds,
    /// never the number sent so far. Sends what is left at the paced rate,
    /// says `end`, and waits for the server to store its last text and close.
    /// Returns true once the server has confirmed the whole capture, which can
    /// take as long as the network is away: a lost connection is dialled again
    /// at once and then every few seconds, and the next `ready` says where to
    /// continue. Returns false only when the server refused the stream for
    /// good, or the stream was cancelled.
    ///
    /// A stream that was never started is started here, so a capture left by an
    /// earlier launch is delivered the same way.
    @discardableResult
    public func end(atSample sample: Int64) async -> Bool {
        guard !finished else { return delivered }
        phase = .ending(at: sample)
        // Nothing was captured and nothing is connected: there is nothing to deliver.
        if sample <= 0, socket == nil {
            delivered = true
            stop(.ended)
            return true
        }
        if runTask == nil { start() }
        backoffWait?.cancel()
        await runTask?.value
        return delivered
    }

    /// Stops at once, without telling the server.
    public func cancel() { stop(.ended) }

    // MARK: Connection loop

    private func run() async {
        var failures = 0
        var serverBusy = false
        while !Task.isCancelled, !finished {
            var connected = false
            if delivered { break }
            // While the server is turning streams away, that is what the line says, not "reconnecting".
            if !serverBusy { report(everConnected ? .reconnecting : .connecting) }
            var failure = LiveSocketError.transport
            lastServerError = nil
            do {
                let socket = try await connector.connect()
                self.socket = socket
                defer { socket.close(code: 1000); self.socket = nil }
                try await handshake(socket)
                failures = 0
                connected = true
                try await session(socket)
                stop(.ended)
                return
            } catch let error as LiveSocketError { failure = error }
            catch { failure = .transport }
            guard !finished, !Task.isCancelled else { return }
            if let reason = Self.terminal(failure, serverError: lastServerError) {
                // The note is no longer being recorded on the server (it was stopped elsewhere): what
                // this device had not handed over will never be transcribed. That is said, not hidden.
                if reason == .notRecording, case .ending(let sample) = phase, acknowledged < sample {
                    recordSkip(from: acknowledged, to: sample)
                }
                stop(.unavailable(reason))
                return
            }
            serverBusy = Self.serverIsBusy(failure, serverError: lastServerError)
            report(serverBusy ? .providerUnavailable : .reconnecting)
            // A server restarting this stream (its provider session was lost) wants it back at once:
            // the first dial after a working connection is immediate, any after that back off.
            let restart = connected && failure == .closed(code: LiveWire.CloseCode.serviceRestart)
            var delay = restart ? .zero : configuration.backoff[min(failures, configuration.backoff.count - 1)]
            if case .ending = phase { delay = min(delay, configuration.endingBackoffCap) }
            failures += 1
            let wait = Task { _ = try? await Task.sleep(for: delay) }
            backoffWait = wait
            await wait.value
            backoffWait = nil
        }
        stop(.ended)
    }

    /// `hello`, then the server's `ready`, which says where to send from.
    private func handshake(_ socket: any LiveSocket) async throws {
        let hello = LiveWire.Hello(captureId: configuration.captureID, language: configuration.language, streams: [
            LiveWire.Stream(channel: configuration.channel, sampleRate: configuration.sampleRate,
                            baseMs: configuration.baseMs, resumeFromSample: acknowledged)
        ])
        try await socket.send(text: LiveWire.encode(hello))
        let timeout = configuration.handshakeTimeout
        let ready = try await withThrowingTaskGroup(of: LiveWire.Ready.self) { group in
            group.addTask { try await self.awaitReady(socket) }
            group.addTask {
                try await Task.sleep(for: timeout)
                socket.close(code: 1000)
                throw LiveSocketError.transport
            }
            defer { group.cancelAll() }
            return try await group.next()!
        }
        statistics.connections += 1
        everConnected = true
        serverState = nil; serverBehindSince = nil; pauseMarked = false; endMarkSent = false
        resumeWindowMs = ready.resumeWindowMs
        let accept = ready.streams.first { $0.channel == configuration.channel }?.acceptFromSample ?? acknowledged
        let end = sendLimit()
        acknowledged = min(max(0, accept), end)
        cursor = acknowledged
        skipBacklogIfTooLong(end: end, slackMs: 0)
        continuation.yield(.ready(transcriptRevision: ready.transcriptRevision, provider: ready.provider, model: ready.model))
    }

    private func awaitReady(_ socket: any LiveSocket) async throws -> LiveWire.Ready {
        while true {
            guard case .text(let text) = try await socket.receive(), let message = try? LiveWire.decode(text) else { continue }
            switch message {
            case .ready(let ready): return ready
            case .error(let error): lastServerError = error
            default: continue
            }
        }
    }

    /// Runs until the stream ends or the connection fails.
    private func session(_ socket: any LiveSocket) async throws {
        try await withThrowingTaskGroup(of: Void.self) { group in
            group.addTask { try await self.receiveLoop(socket) }
            group.addTask { try await self.sendLoop(socket) }
            // Closing the socket is what releases a pending receive.
            defer { socket.close(code: 1000); group.cancelAll() }
            try await group.next()
        }
    }

    // MARK: Receiving

    private func receiveLoop(_ socket: any LiveSocket) async throws {
        while true {
            let message: LiveSocketMessage
            do { message = try await socket.receive() }
            catch {
                // After `end` the server stores its last text and closes in order. Only that close, with
                // every sample acknowledged, is the end of the capture's stream. Any other end of the
                // connection (a provider lost at the last moment, the network) is dialled again, and the
                // next `ready` says what the server still wants.
                if endMarkSent, case .ending(let sample) = phase, acknowledged >= sample,
                   (error as? LiveSocketError) == .closed(code: 1000) {
                    delivered = true
                    return
                }
                throw error
            }
            guard case .text(let text) = message, let decoded = try? LiveWire.decode(text) else { continue }
            handle(decoded)
        }
    }

    private func handle(_ message: LiveWire.ServerMessage) {
        switch message {
        case .ack(let ack) where ack.channel == configuration.channel:
            acknowledged = max(acknowledged, ack.throughSample)
        case .interim(let interim): continuation.yield(.interim(interim))
        case .final(let final): continuation.yield(.final(final))
        case .speakers(let speakers): continuation.yield(.speakers(speakers.merges, relabels: speakers.relabels ?? []))
        case .gap(let gap): continuation.yield(.gap(gap))
        case .status(let status) where status.channel == configuration.channel:
            if status.streamState == .catchingUp { if serverBehindSince == nil { serverBehindSince = .now } }
            else { serverBehindSince = nil }
            serverState = status.streamState
            refreshStatus()
        case .error(let error): lastServerError = error
        case .ack, .status, .ready, .unknown: break
        }
    }

    // MARK: Sending

    /// The sample the stream may send up to: the end of the capture, or the mark.
    private func sendLimit() -> Int64 {
        let available = source.availableFrames()
        switch phase {
        case .running: return available
        case .paused(let sample), .ending(let sample): return min(sample, available)
        }
    }

    private func milliseconds(_ samples: Int64) -> Int {
        Int(max(0, samples) * 1000 / Int64(configuration.sampleRate))
    }

    /// A backlog longer than the resume window is not replayed: sending jumps to
    /// `tailAfterSkipMs` before the end of the capture. The server sees the
    /// sample index jump and records a gap.
    private func skipBacklogIfTooLong(end: Int64, slackMs: Int) {
        guard milliseconds(end - cursor) > resumeWindowMs + slackMs else { return }
        // As much of the backlog as the window allows (nine tenths of it, so the stream is inside the window once
        // it resumes): the live transcript is the only one, so every second replayed is a second kept.
        let keepMs = max(configuration.tailAfterSkipMs, resumeWindowMs - resumeWindowMs / 10)
        let tail = Int64(keepMs) * Int64(configuration.sampleRate) / 1000
        let target = max(cursor, end - tail)
        guard target > cursor else { return }
        recordSkip(from: cursor, to: target)
        cursor = target
        statistics.skips += 1
    }

    /// Notes, and tells the app, exactly which stretch of the capture is not sent.
    private func recordSkip(from first: Int64, to last: Int64) {
        guard last > first else { return }
        statistics.skippedSamples.append(first..<last)
        continuation.yield(.skipped(LiveSkip(channel: configuration.channel,
            fromMs: configuration.baseMs + milliseconds(first), toMs: configuration.baseMs + milliseconds(last))))
    }

    private func sendLoop(_ socket: any LiveSocket) async throws {
        let clock = ContinuousClock()
        var lastActivity = clock.now
        var endDeadline: ContinuousClock.Instant?
        // Time without a connection earns nothing beyond the burst the budget already holds.
        lastRefill = clock.now
        while true {
            try Task.checkCancellation()
            let now = clock.now
            budget.refill(seconds: Self.seconds(now - (lastRefill ?? now)))
            lastRefill = now

            // A resume mark goes out before any audio recorded after it.
            if case .running = phase, serverPaused, let sample = resumeAt {
                try await sendMark(.resume, at: sample, on: socket)
                serverPaused = false
                resumeAt = nil
                lastActivity = clock.now
            }

            let limit = sendLimit()
            skipBacklogIfTooLong(end: limit, slackMs: configuration.skipSlackMs)
            let draining = phase != .running
            while cursor < limit {
                let wanted = Int(min(Int64(configuration.maxFrameSamples), limit - cursor))
                // While recording, wait for a whole frame. A mark sends what is left.
                guard draining || wanted >= configuration.minFrameSamples else { break }
                // Every frame is paced, an ending stream's included.
                guard budget.take(wanted) else { break }
                let first = cursor
                let pcm = try source.read(from: first, maxFrames: wanted)
                guard !pcm.isEmpty else { break }
                try await socket.send(data: LiveWire.audioFrame(channel: configuration.channel, firstSample: first, pcm: pcm))
                cursor = first + Int64(pcm.count / ContinuousCapture.bytesPerFrame)
                statistics.framesSent += 1
                statistics.samplesSent += Int64(pcm.count / ContinuousCapture.bytesPerFrame)
                lastActivity = clock.now
            }

            if cursor >= limit {
                switch phase {
                case .paused(let sample) where !pauseMarked && cursor >= limit:
                    try await sendMark(.pause, at: sample, on: socket)
                    serverPaused = true
                    pauseMarked = true
                    lastActivity = clock.now
                case .ending(let sample) where !endMarkSent:
                    try await sendMark(.end, at: sample, on: socket)
                    endMarkSent = true
                    endDeadline = clock.now + configuration.endGrace
                default: break
                }
            }
            // The server did not close after `end`: drop this connection and dial again.
            if let endDeadline, clock.now >= endDeadline { throw LiveSocketError.transport }
            if clock.now - lastActivity >= configuration.pingAfterIdle {
                try await socket.ping()
                lastActivity = clock.now
            }
            refreshStatus()
            try await Task.sleep(for: configuration.tick)
        }
    }

    private func sendMark(_ kind: LiveWire.MarkKind, at sample: Int64, on socket: any LiveSocket) async throws {
        let mark = LiveWire.Mark(kind: kind, atSample: [configuration.channel.rawValue: sample])
        try await socket.send(text: LiveWire.encode(mark))
        statistics.marksSent.append(kind)
    }

    // MARK: Status

    private func refreshStatus() {
        guard socket != nil, !finished else { return }
        if case .paused = phase { report(.paused); return }
        if serverState == .providerUnavailable { report(.providerUnavailable); return }
        // The figure is the audio this device has not sent yet, which it knows exactly.
        // The server's own lag counts audio whose text is not final yet, which is not
        // the same thing and rises while a backlog is replayed; it only says "catching up".
        let unsent = milliseconds(source.availableFrames() - cursor)
        if unsent > 2_000 {
            report(.catchingUp(lagMs: unsent / 1000 * 1000))
        } else if serverState == .catchingUp, let since = serverBehindSince,
                  ContinuousClock.now - since >= configuration.serverBehindNotice {
            // Said only once it has lasted: every stream is briefly "behind" as it starts.
            report(.catchingUp(lagMs: 0))
        } else {
            report(.live)
        }
    }

    private func report(_ value: LiveTranscriptStatus) {
        guard status != value, !finished else { return }
        status = value
        continuation.yield(.status(value))
    }

    private func stop(_ final: LiveTranscriptStatus) {
        guard !finished else { return }
        report(final)
        finished = true
        socket?.close(code: 1000)
        backoffWait?.cancel()
        runTask?.cancel()
        continuation.finish()
    }

    // MARK: Policy

    /// The reason a failure ends the live stream for this recording, or nil
    /// when the client should dial again. Only what dialling again cannot
    /// change is final: a malformed stream, a session that is not accepted, a
    /// note that is gone or not being recorded, and another connection that
    /// took the capture over. A limit, an unavailable provider, a server that
    /// is not ready (503, which is also an instance without a live model), a
    /// lost connection, a server restart and "client too slow" are retried.
    static func terminal(_ failure: LiveSocketError, serverError: LiveWire.ServerError?) -> LiveUnavailableReason? {
        switch failure {
        case .refused(let status):
            switch status {
            case 401, 403: return .unauthorized
            case 404: return .notFound
            case 409: return .notRecording
            case 408, 429, 500...599: return nil
            default: return .protocolError
            }
        case .closed(let code):
            switch code {
            case LiveWire.CloseCode.malformed: return .protocolError
            case LiveWire.CloseCode.unauthenticated: return .unauthorized
            case LiveWire.CloseCode.notFound: return .notFound
            case LiveWire.CloseCode.replaced: return .replaced
            default: return nil
            }
        case .transport:
            // An `error` message precedes every refusal close; honour it if the code was lost.
            guard let serverError, !serverError.retryable else { return nil }
            switch serverError.code {
            case "malformed": return .protocolError
            case "unauthenticated": return .unauthorized
            case "not_found": return .notFound
            case "replaced": return .replaced
            default: return nil
            }
        }
    }

    /// True when the server turned the stream away for now (a limit, its
    /// provider, an outage) rather than the connection being lost.
    static func serverIsBusy(_ failure: LiveSocketError, serverError: LiveWire.ServerError?) -> Bool {
        switch failure {
        case .refused(let status): return status == 429 || status == 503
        case .closed(let code): return code == LiveWire.CloseCode.overLimit || code == LiveWire.CloseCode.providerUnavailable
        case .transport: return serverError.map { ["over_limit", "provider_unavailable"].contains($0.code) } ?? false
        }
    }

    private static func seconds(_ duration: Duration) -> Double {
        Double(duration.components.seconds) + Double(duration.components.attoseconds) / 1e18
    }
}
