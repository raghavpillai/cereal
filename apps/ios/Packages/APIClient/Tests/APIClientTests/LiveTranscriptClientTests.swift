import Foundation
import Testing
import Contracts
@testable import APIClient

// MARK: - Fakes

/// A capture in memory. Sample `i` has the value `i` (truncated to 16 bits),
/// so every frame the client sends can be checked against its claimed index.
final class MemoryCapture: LiveAudioSource, @unchecked Sendable {
    private let lock = NSLock()
    private var frames: Int64 = 0
    private(set) var largestRead = 0

    func grow(by count: Int) { lock.withLock { frames += Int64(count) } }
    func availableFrames() -> Int64 { lock.withLock { frames } }
    func read(from frame: Int64, maxFrames: Int) throws -> Data {
        let count = lock.withLock { () -> Int in
            largestRead = max(largestRead, maxFrames)
            return Int(min(Int64(maxFrames), max(0, frames - frame)))
        }
        return MemoryCapture.pcm(from: frame, count: count)
    }
    static func pcm(from frame: Int64, count: Int) -> Data {
        bytes((0..<count).map { Int16(truncatingIfNeeded: frame + Int64($0)) })
    }
}

/// One server-side connection: records what the client sent and plays the server's part.
final class FakeSocket: LiveSocket, @unchecked Sendable {
    struct Frame: Equatable { let first: Int64; let samples: Int }
    struct Script {
        /// The sample the server asks for, given the client's hello. Nil: echo `resumeFromSample`.
        var acceptFrom: (@Sendable (LiveWire.Hello) -> Int64)?
        var resumeWindowMs = 30_000
        var transcriptRevision = 0
        var sendsReady = true
        var acknowledges = true
        /// Closes with this code once this many audio frames have arrived.
        var closeAfterFrames: (count: Int, code: Int)?
        /// After `mark end`: the messages to send, then a close with `endClose`.
        var onEnd: [String]? = []
        /// 1000 is the orderly close that ends a capture's stream; 1012 is a provider lost at the last moment.
        var endClose = 1000
        var sendFails = false
        /// How long the connection takes to accept one audio frame.
        var audioDelay: Duration?
    }

    private let lock = NSLock()
    private let script: Script
    private var inbox: [Result<LiveSocketMessage, LiveSocketError>] = []
    private var waiter: CheckedContinuation<LiveSocketMessage, Error>?
    private var closedWith: Int?
    private(set) var hello: LiveWire.Hello?
    private(set) var frames: [Frame] = []
    /// When each frame arrived.
    private(set) var arrivals: [ContinuousClock.Instant] = []
    private(set) var marks: [LiveWire.Mark] = []
    private(set) var pings = 0
    private(set) var contentMatchesIndex = true
    /// Text and audio in arrival order, to check what was sent before what.
    private(set) var order: [String] = []

    init(script: Script) { self.script = script }

    var isClosed: Bool { lock.withLock { closedWith != nil } }
    var snapshot: (frames: [Frame], marks: [LiveWire.Mark], order: [String]) { lock.withLock { (frames, marks, order) } }
    var timedFrames: [(frame: Frame, at: ContinuousClock.Instant)] { lock.withLock { Array(zip(frames, arrivals)).map { ($0.0, $0.1) } } }
    var receivedHello: LiveWire.Hello? { lock.withLock { hello } }
    var pingCount: Int { lock.withLock { pings } }
    var contentIsCorrect: Bool { lock.withLock { contentMatchesIndex } }
    var throughSample: Int64 { lock.withLock { frames.map { $0.first + Int64($0.samples) }.max() ?? 0 } }

    func push(_ json: String) { deliver(.success(.text(json))) }
    func serverClose(_ code: Int) { deliver(.failure(.closed(code: code))) }
    func drop() { deliver(.failure(.transport)) }

    private func deliver(_ item: Result<LiveSocketMessage, LiveSocketError>) {
        lock.lock()
        if case .failure(let error) = item, case .closed(let code) = error { closedWith = closedWith ?? code }
        if case .failure(.transport) = item { closedWith = closedWith ?? -1 }
        if let waiter {
            self.waiter = nil
            lock.unlock()
            waiter.resume(with: item.mapError { $0 as Error })
        } else {
            inbox.append(item)
            lock.unlock()
        }
    }

    func send(text: String) async throws {
        try checkOpen()
        let data = Data(text.utf8)
        if let hello = try? JSONDecoder().decode(LiveWire.Hello.self, from: data), hello.type == "hello" {
            lock.withLock { self.hello = hello; order.append("hello") }
            guard script.sendsReady else { return }
            let accept = script.acceptFrom?(hello) ?? hello.streams[0].resumeFromSample
            push(#"{"type":"ready","sessionId":"7f0c9d7e-51f4-4c4b-9d0c-0d6f1f1c2a11","provider":"fake","model":"fake-live","streams":[{"channel":"mic","acceptFromSample":\#(accept)}],"resumeWindowMs":\#(script.resumeWindowMs),"transcriptRevision":\#(script.transcriptRevision)}"#)
        } else if let mark = try? JSONDecoder().decode(LiveWire.Mark.self, from: data), mark.type == "mark" {
            lock.withLock { marks.append(mark); order.append("mark:\(mark.kind.rawValue)") }
            if mark.kind == .end, let messages = script.onEnd {
                messages.forEach(push)
                serverClose(script.endClose)
            }
        }
    }

    func send(data: Data) async throws {
        try checkOpen()
        if let delay = script.audioDelay { try? await Task.sleep(for: delay) }
        guard let frame = LiveWire.parseAudioFrame(data) else { serverClose(LiveWire.CloseCode.malformed); return }
        let samples = frame.pcm.count / 2
        let count = lock.withLock { () -> Int in
            if frame.pcm != MemoryCapture.pcm(from: frame.firstSample, count: samples) { contentMatchesIndex = false }
            frames.append(Frame(first: frame.firstSample, samples: samples))
            arrivals.append(.now)
            order.append("audio")
            return frames.count
        }
        if script.acknowledges {
            push(#"{"type":"ack","channel":"mic","throughSample":\#(frame.firstSample + Int64(samples)),"transcribedThroughMs":0}"#)
        }
        if let close = script.closeAfterFrames, count == close.count { serverClose(close.code) }
    }

    private func checkOpen() throws {
        try lock.withLock {
            if script.sendFails { throw LiveSocketError.transport }
            if let closedWith { throw closedWith > 0 ? LiveSocketError.closed(code: closedWith) : LiveSocketError.transport }
        }
    }

    func receive() async throws -> LiveSocketMessage {
        try await withCheckedThrowingContinuation { continuation in
            lock.lock()
            if !inbox.isEmpty {
                let item = inbox.removeFirst()
                lock.unlock()
                continuation.resume(with: item.mapError { $0 as Error })
            } else if let closedWith {
                lock.unlock()
                continuation.resume(throwing: closedWith > 0 ? LiveSocketError.closed(code: closedWith) : LiveSocketError.transport)
            } else {
                waiter = continuation
                lock.unlock()
            }
        }
    }

    func ping() async throws { try checkOpen(); lock.withLock { pings += 1 } }

    /// The client closing its end.
    func close(code: Int) {
        lock.lock()
        closedWith = closedWith ?? code
        let pending = waiter
        waiter = nil
        lock.unlock()
        pending?.resume(throwing: LiveSocketError.closed(code: code))
    }
}

/// Hands out one scripted connection per dial.
final class FakeConnector: LiveSocketConnector, @unchecked Sendable {
    enum Step { case accept(FakeSocket.Script), refuse(LiveSocketError) }
    private let lock = NSLock()
    private var steps: [Step]
    private let fallback: Step
    private(set) var sockets: [FakeSocket] = []
    private(set) var dials: [ContinuousClock.Instant] = []

    init(_ steps: [Step], then fallback: Step = .accept(FakeSocket.Script())) {
        self.steps = steps
        self.fallback = fallback
    }

    var dialCount: Int { lock.withLock { dials.count } }
    var dialTimes: [ContinuousClock.Instant] { lock.withLock { dials } }
    func socket(_ index: Int) -> FakeSocket? { lock.withLock { index < sockets.count ? sockets[index] : nil } }
    var last: FakeSocket? { lock.withLock { sockets.last } }

    func connect() async throws -> any LiveSocket {
        let step = lock.withLock { () -> Step in
            dials.append(.now)
            return steps.isEmpty ? fallback : steps.removeFirst()
        }
        switch step {
        case .refuse(let error): throw error
        case .accept(let script):
            let socket = FakeSocket(script: script)
            lock.withLock { sockets.append(socket) }
            return socket
        }
    }
}

/// Collects a client's events on the side.
actor EventLog {
    private(set) var events: [LiveTranscriptEvent] = []
    private var task: Task<Void, Never>?
    func follow(_ client: LiveTranscriptClient) {
        task = Task { for await event in client.events { self.record(event) } }
    }
    private func record(_ event: LiveTranscriptEvent) { events.append(event) }
    var statuses: [LiveTranscriptStatus] {
        events.compactMap { if case .status(let status) = $0 { status } else { nil } }
    }
    var interimTexts: [String] {
        events.compactMap { if case .interim(let interim) = $0 { interim.text } else { nil } }
    }
    var finalTexts: [String] {
        events.compactMap { if case .final(let final) = $0 { final.segment.text } else { nil } }
    }
    func finished() async { await task?.value }
}

func waitUntil(_ what: String = "condition", timeout: Duration = .seconds(5), _ condition: @Sendable () async -> Bool) async throws {
    let deadline = ContinuousClock.now + timeout
    while ContinuousClock.now < deadline {
        if await condition() { return }
        try await Task.sleep(for: .milliseconds(2))
    }
    Issue.record("Timed out waiting for \(what)")
    throw CancellationError()
}

private func fastConfiguration(baseMs: Int = 754_000) -> LiveTranscriptClient.Configuration {
    var configuration = LiveTranscriptClient.Configuration(captureID: "note/capture-0.wav", baseMs: baseMs)
    configuration.tick = .milliseconds(2)
    configuration.catchUpFactor = 4_000 // Pacing has its own test; the others should not wait for it.
    configuration.backoff = [.milliseconds(10), .milliseconds(20), .milliseconds(40)]
    configuration.endGrace = .milliseconds(300)
    configuration.handshakeTimeout = .milliseconds(300)
    return configuration
}

// MARK: - Tests

@Suite struct LiveTranscriptClientTests {
    @Test func helloThenAudioFramesFromTheFileCursorInOrderWithNothingHeldBack() async throws {
        let capture = MemoryCapture()
        let connector = FakeConnector([])
        let client = LiveTranscriptClient(connector: connector, source: capture, configuration: fastConfiguration())
        await client.start()
        try await waitUntil("hello") { connector.last?.receivedHello != nil }
        let hello = try #require(connector.last?.receivedHello)
        #expect(hello.protocol == 1)
        #expect(hello.captureId == "note/capture-0.wav")
        #expect(hello.streams == [LiveWire.Stream(channel: .mic, sampleRate: 16_000, baseMs: 754_000, resumeFromSample: 0)])

        // The capture grows the way an input tap feeds it; less than a frame is not sent.
        capture.grow(by: 1_365)
        try await Task.sleep(for: .milliseconds(30))
        #expect(connector.last?.snapshot.frames.isEmpty == true)
        for _ in 0..<40 { capture.grow(by: 1_365); try await Task.sleep(for: .milliseconds(3)) }
        let total = Int64(41 * 1_365)
        try await waitUntil("audio sent") { total - (connector.last?.throughSample ?? 0) < 1_600 }
        let socket = try #require(connector.last)
        let frames = socket.snapshot.frames
        // Contiguous sample indices from 0, 100 to 200 ms each, content matching the index.
        #expect(frames.first?.first == 0)
        #expect(zip(frames, frames.dropFirst()).allSatisfy { $0.first + Int64($0.samples) == $1.first })
        #expect(frames.allSatisfy { (1_600...3_200).contains($0.samples) })
        #expect(socket.contentIsCorrect)
        // The file is the buffer: one frame at a time was read into memory.
        #expect(capture.largestRead <= 3_200)
        try await waitUntil("acks") { await client.acknowledged == socket.throughSample }
        #expect(await client.cursor == socket.throughSample)
        await client.cancel()
    }

    @Test func reconnectResumesFromTheLastAcknowledgedSampleWithoutGapOrDuplicate() async throws {
        let capture = MemoryCapture()
        // The first connection dies after five frames; the server acknowledged each of them.
        let connector = FakeConnector([.accept(.init(closeAfterFrames: (5, 1006)))])
        let client = LiveTranscriptClient(connector: connector, source: capture, configuration: fastConfiguration())
        let log = EventLog(); await log.follow(client)
        capture.grow(by: 5 * 3_200)
        await client.start()
        try await waitUntil("second dial") { connector.dialCount == 2 && connector.last?.receivedHello != nil }
        let first = try #require(connector.socket(0))
        let second = try #require(connector.socket(1))
        #expect(first.throughSample == 16_000)
        // The hello of the reconnect carries the last ack and the same capture.
        #expect(second.receivedHello?.streams[0].resumeFromSample == 16_000)
        #expect(second.receivedHello?.captureId == first.receivedHello?.captureId)
        capture.grow(by: 8_000)
        try await waitUntil("resumed audio") { second.throughSample == 24_000 }
        #expect(second.snapshot.frames.first?.first == 16_000)
        // Every sample was sent exactly once across the two connections.
        let all = first.snapshot.frames + second.snapshot.frames
        #expect(zip(all, all.dropFirst()).allSatisfy { $0.first + Int64($0.samples) == $1.first })
        #expect(first.contentIsCorrect && second.contentIsCorrect)
        #expect(await client.statistics.skips == 0)
        let statuses = await log.statuses
        #expect(statuses.contains(.reconnecting))
        #expect(statuses.last == .live)
        await client.cancel()
    }

    @Test func theServerMayAskForALaterSampleAndTheClientFollowsIt() async throws {
        let capture = MemoryCapture()
        // The server took more than it acknowledged before the drop.
        let connector = FakeConnector([.accept(.init(acknowledges: false, closeAfterFrames: (3, 1011))),
                                       .accept(.init(acceptFrom: { _ in 9_600 }))])
        let client = LiveTranscriptClient(connector: connector, source: capture, configuration: fastConfiguration())
        capture.grow(by: 9_600)
        await client.start()
        try await waitUntil("second connection") { connector.socket(1)?.receivedHello != nil }
        let second = try #require(connector.socket(1))
        #expect(second.receivedHello?.streams[0].resumeFromSample == 0) // nothing was acknowledged
        capture.grow(by: 6_400)
        try await waitUntil("continues") { second.throughSample == 16_000 }
        #expect(second.snapshot.frames.first?.first == 9_600)
        await client.cancel()
    }

    @Test func aBacklogInsideTheResumeWindowIsReplayedInFull() async throws {
        let capture = MemoryCapture()
        let connector = FakeConnector([.accept(.init(closeAfterFrames: (2, 1006))), .refuse(.transport), .refuse(.transport)])
        var configuration = fastConfiguration()
        configuration.catchUpFactor = 20 // slow enough for the replay to be observed
        let client = LiveTranscriptClient(connector: connector, source: capture, configuration: configuration)
        let log = EventLog(); await log.follow(client)
        capture.grow(by: 6_400)
        await client.start()
        try await waitUntil("drop") { connector.socket(0)?.isClosed == true }
        // 20 seconds are recorded while the connection is down: within the 30-second window.
        capture.grow(by: 20 * 16_000)
        try await waitUntil("caught up") { connector.socket(1)?.throughSample == 6_400 + 20 * 16_000 }
        let second = try #require(connector.socket(1))
        #expect(second.snapshot.frames.first?.first == 6_400)
        #expect(await client.statistics.skips == 0)
        #expect(connector.dialCount == 4)
        #expect(await log.statuses.contains { if case .catchingUp = $0 { true } else { false } })
        try await waitUntil("live again") { await client.status == .live }
        await client.cancel()
    }

    @Test func aBacklogLongerThanTheResumeWindowKeepsNineTenthsOfTheWindow() async throws {
        let capture = MemoryCapture()
        let connector = FakeConnector([.accept(.init(closeAfterFrames: (2, 1006))), .refuse(.transport)])
        let client = LiveTranscriptClient(connector: connector, source: capture, configuration: fastConfiguration())
        let log = EventLog(); await log.follow(client)
        capture.grow(by: 6_400)
        await client.start()
        try await waitUntil("drop") { connector.socket(0)?.isClosed == true }
        // 45 seconds pass without a connection.
        capture.grow(by: 45 * 16_000)
        let end = Int64(6_400 + 45 * 16_000)
        try await waitUntil("current again") { connector.socket(1)?.throughSample == end }
        let second = try #require(connector.socket(1))
        // The hello still reports the true resume point; the jump is in the frame index, which is how the server learns of the gap.
        #expect(second.receivedHello?.streams[0].resumeFromSample == 6_400)
        // The window is 30 s: the newest 27 s are replayed, the rest has no transcript.
        #expect(second.snapshot.frames.first?.first == end - 27 * 16_000)
        #expect(await client.statistics.skips == 1)
        // Exactly which stretch was skipped is recorded and reported, on the note's timeline.
        #expect(await client.statistics.skippedSamples == [6_400..<(end - 27 * 16_000)])
        let skips = await log.events.compactMap { if case .skipped(let skip) = $0 { skip } else { nil } }
        #expect(skips == [LiveSkip(channel: .mic, fromMs: 754_000 + 400, toMs: 754_000 + Int((end - 27 * 16_000) / 16))])
        #expect(second.contentIsCorrect)
        await client.cancel()
    }

    @Test func pauseResumeAndEndAreMarkedAtTheExactSampleAfterTheAudioBeforeThem() async throws {
        let capture = MemoryCapture()
        let finalJSON = #"{"type":"final","utteranceId":"11111111-1111-4111-8111-111111111111","transcriptRevision":3,"segment":{"id":"22222222-2222-4222-8222-222222222222","channel":"mic","speakerId":"mic:n:1","speakerLabel":"Speaker 1","text":"last words","isFinal":true,"startMs":1000,"endMs":2000}}"#
        let connector = FakeConnector([.accept(.init(onEnd: [finalJSON]))])
        let client = LiveTranscriptClient(connector: connector, source: capture, configuration: fastConfiguration())
        let log = EventLog(); await log.follow(client)
        await client.start()
        capture.grow(by: 10_000)
        try await waitUntil("first audio") { (connector.last?.throughSample ?? 0) >= 9_600 }
        let socket = try #require(connector.last)

        // Pause with 400 samples not yet sent: less than a frame, sent anyway, then the mark.
        await client.pause(atSample: 10_000)
        try await waitUntil("pause mark") { socket.snapshot.marks.count == 1 }
        #expect(socket.throughSample == 10_000)
        #expect(socket.snapshot.marks[0] == LiveWire.Mark(kind: .pause, atSample: ["mic": 10_000]))
        #expect(socket.snapshot.order.last == "mark:pause")
        try await waitUntil("paused status") { await client.status == .paused }
        let framesWhilePaused = socket.snapshot.frames.count
        try await Task.sleep(for: .milliseconds(30))
        #expect(socket.snapshot.frames.count == framesWhilePaused)

        // Resume continues at the same sample index; the mark precedes the new audio.
        await client.resume()
        capture.grow(by: 4_000)
        try await waitUntil("resumed audio") { socket.throughSample == 14_000 || socket.throughSample >= 13_200 }
        let order = socket.snapshot.order
        let resumeIndex = try #require(order.firstIndex(of: "mark:resume"))
        #expect(order[(resumeIndex + 1)...].contains("audio"))
        #expect(socket.snapshot.marks[1] == LiveWire.Mark(kind: .resume, atSample: ["mic": 10_000]))
        #expect(socket.snapshot.frames[framesWhilePaused].first == 10_000)

        // End: the remainder, the mark, then the server's last final before the stream finishes.
        capture.grow(by: 123)
        await client.end(atSample: 14_123)
        #expect(socket.throughSample == 14_123)
        #expect(socket.snapshot.marks.last == LiveWire.Mark(kind: .end, atSample: ["mic": 14_123]))
        #expect(socket.snapshot.order.last == "mark:end")
        await log.finished()
        #expect(await log.finalTexts == ["last words"])
        #expect(await log.statuses.last == .ended)
        #expect(await client.statistics.marksSent == [.pause, .resume, .end])
        #expect(socket.contentIsCorrect)
    }

    @Test func aSlowConnectionStillDeliversTheWholeCaptureBeforeTheStreamEnds() async throws {
        // Ten seconds of capture on a connection that takes 40 ms per frame: 50 frames, 2 s, to send it all.
        // Nothing is given up after a grace period: the stream is the only transcript.
        let capture = MemoryCapture()
        let connector = FakeConnector([.accept(.init(audioDelay: .milliseconds(40)))])
        let client = LiveTranscriptClient(connector: connector, source: capture, configuration: fastConfiguration())
        let log = EventLog(); await log.follow(client)
        await client.start()
        capture.grow(by: 160_000)
        try await waitUntil("first audio") { (connector.last?.throughSample ?? 0) > 0 }
        let socket = try #require(connector.last)

        let delivered = await client.end(atSample: 160_000)
        await log.finished()
        let confirmed = await client.delivered
        #expect(delivered && confirmed)
        #expect(socket.throughSample == 160_000)
        #expect(socket.snapshot.marks == [LiveWire.Mark(kind: .end, atSample: ["mic": 160_000])])
        #expect(socket.snapshot.order.last == "mark:end")
        #expect(await log.statuses.last == .ended)
        #expect(socket.contentIsCorrect)
    }

    @Test func aSixtySecondBacklogIsReplayedWholeContiguousAndNeverFasterThanThreeTimesRealTimeAlsoWhileEnding() async throws {
        // Real pacing: the default three times real time with half a second of burst.
        let capture = MemoryCapture()
        let window = 900_000
        let connector = FakeConnector([.accept(.init(resumeWindowMs: window, closeAfterFrames: (2, 1006))), .refuse(.transport)],
                                      then: .accept(.init(resumeWindowMs: window)))
        var configuration = fastConfiguration()
        configuration.catchUpFactor = 3
        let client = LiveTranscriptClient(connector: connector, source: capture, configuration: configuration)
        capture.grow(by: 6_400)
        await client.start()
        try await waitUntil("drop") { connector.socket(0)?.isClosed == true }
        // A minute is recorded while the network is away.
        capture.grow(by: 60 * 16_000)
        let total = Int64(6_400 + 60 * 16_000)
        try await waitUntil("replay under way") { (connector.socket(1)?.throughSample ?? 0) > 6_400 }
        let replay = try #require(connector.socket(1))
        // Some way into the replay the recording stops: the rest goes out while ending, at the same pace.
        try await Task.sleep(for: .seconds(8))
        let sentBeforeEnd = replay.throughSample
        #expect(sentBeforeEnd < total)
        let delivered = await client.end(atSample: total)
        #expect(delivered)

        // Complete and contiguous across both connections: every sample once, in order.
        let first = try #require(connector.socket(0)).snapshot.frames
        let frames = first + replay.snapshot.frames
        #expect(frames.first?.first == 0 && frames.last.map { $0.first + Int64($0.samples) } == total)
        #expect(zip(frames, frames.dropFirst()).allSatisfy { $0.first + Int64($0.samples) == $1.first })
        #expect(replay.contentIsCorrect)
        #expect(await client.statistics.skips == 0)
        // Never faster than three times real time plus half a second, over any stretch, before and after `end`.
        let timed = replay.timedFrames
        let rate = 3.0 * 16_000, burst = 0.5 * 16_000, tolerance = 0.05 * rate
        var worst = 0.0
        for i in timed.indices {
            var samples = 0.0
            for j in i..<timed.count {
                samples += Double(timed[j].frame.samples)
                let elapsed = timed[j].at - timed[i].at
                let seconds = Double(elapsed.components.seconds) + Double(elapsed.components.attoseconds) / 1e18
                worst = max(worst, samples - (burst + rate * seconds))
            }
        }
        #expect(worst <= tolerance, "sent \(worst) samples more than three times real time allows")
        let span = timed.last!.at - timed.first!.at
        #expect(span >= .seconds(19)) // 59.6 s of audio past the burst, at 48 000 samples a second
        #expect(replay.snapshot.marks.map(\.kind) == [.end])
    }

    @Test func stopDuringAnOutageWaitsForTheNetworkAndHandsOverEverythingBeforeItReturns() async throws {
        let capture = MemoryCapture()
        // The network goes at 0.4 s of audio and is away for a dozen dials after Stop.
        let connector = FakeConnector([.accept(.init(closeAfterFrames: (2, 1006)))] + Array(repeating: .refuse(.transport), count: 12))
        var configuration = fastConfiguration()
        configuration.backoff = [.milliseconds(10), .milliseconds(20), .milliseconds(40)]
        let client = LiveTranscriptClient(connector: connector, source: capture, configuration: configuration)
        let log = EventLog(); await log.follow(client)
        capture.grow(by: 6_400)
        await client.start()
        try await waitUntil("drop") { connector.socket(0)?.isClosed == true }
        capture.grow(by: 15 * 16_000)
        let total = Int64(6_400 + 15 * 16_000)
        let delivered = await client.end(atSample: total)
        #expect(delivered)
        #expect(connector.dialCount == 14)
        let last = try #require(connector.socket(1))
        #expect(last.snapshot.frames.first?.first == 6_400 && last.throughSample == total && last.contentIsCorrect)
        #expect(last.snapshot.marks == [LiveWire.Mark(kind: .end, atSample: ["mic": total])])
        await log.finished()
        // While it waited, it said it was reconnecting, never that it had given up.
        let statuses = await log.statuses
        #expect(statuses.contains(.reconnecting) && statuses.last == .ended)
        #expect(!statuses.contains { if case .unavailable = $0 { true } else { false } })
    }

    @Test func aProviderLostAfterEndIsDialledAgainAndOnlyAnOrderlyCloseEndsTheStream() async throws {
        // The server took everything, but its provider was lost before the last words were stored:
        // it closes with 1012, and the next `ready` asks for the audio it has no transcript for.
        let capture = MemoryCapture()
        let connector = FakeConnector([.accept(.init(endClose: 1012)), .accept(.init(acceptFrom: { _ in 9_600 }))])
        let client = LiveTranscriptClient(connector: connector, source: capture, configuration: fastConfiguration())
        capture.grow(by: 16_000)
        await client.start()
        try await waitUntil("sent") { connector.socket(0)?.throughSample == 16_000 }
        let delivered = await client.end(atSample: 16_000)
        #expect(delivered)
        let first = try #require(connector.socket(0)), second = try #require(connector.socket(1))
        #expect(first.snapshot.marks.map(\.kind) == [.end])
        #expect(second.snapshot.frames.first?.first == 9_600 && second.throughSample == 16_000)
        #expect(second.snapshot.marks.map(\.kind) == [.end])
        #expect(connector.dialCount == 2)
    }

    @Test func aNoteStoppedElsewhereWhileEndingSaysExactlyWhichStretchWasNotSent() async throws {
        let capture = MemoryCapture()
        // The connection is lost, and while the client waits to dial again the note is stopped on another device.
        let connector = FakeConnector([.accept(.init(closeAfterFrames: (2, 1006)))], then: .refuse(.refused(status: 409)))
        var configuration = fastConfiguration(baseMs: 10_000)
        configuration.backoff = [.seconds(30)]
        let client = LiveTranscriptClient(connector: connector, source: capture, configuration: configuration)
        let log = EventLog(); await log.follow(client)
        capture.grow(by: 6_400)
        await client.start()
        try await waitUntil("drop") { connector.socket(0)?.isClosed == true }
        capture.grow(by: 32_000)
        let delivered = await client.end(atSample: 38_400)
        await log.finished()
        #expect(!delivered)
        #expect(await client.status == .unavailable(.notRecording))
        #expect(await client.statistics.skippedSamples == [6_400..<38_400])
        let skips = await log.events.compactMap { if case .skipped(let skip) = $0 { skip } else { nil } }
        #expect(skips == [LiveSkip(channel: .mic, fromMs: 10_400, toMs: 12_400)])
    }

    @Test func aPauseThatHappenedWhileDisconnectedIsToldToTheNextConnection() async throws {
        let capture = MemoryCapture()
        let connector = FakeConnector([.accept(.init(closeAfterFrames: (1, 1006)))])
        let client = LiveTranscriptClient(connector: connector, source: capture, configuration: fastConfiguration())
        capture.grow(by: 3_200)
        await client.start()
        try await waitUntil("drop") { connector.socket(0)?.isClosed == true }
        capture.grow(by: 5_000)
        await client.pause(atSample: 8_200)
        try await waitUntil("pause on reconnect") { connector.socket(1)?.snapshot.marks.first?.kind == .pause }
        let second = try #require(connector.socket(1))
        #expect(second.throughSample == 8_200)
        #expect(second.snapshot.marks[0].atSample == ["mic": 8_200])
        await client.cancel()
    }

    @Test func aServerThatTurnsTheStreamAwayIsDialledAgainUntilItTakesIt() async throws {
        // Not configured (or not ready), a limit, an outage: each is retried, and then the stream goes on.
        let connector = FakeConnector([.refuse(.refused(status: 503)), .refuse(.refused(status: 429)), .refuse(.refused(status: 503))])
        let capture = MemoryCapture()
        let client = LiveTranscriptClient(connector: connector, source: capture, configuration: fastConfiguration())
        let log = EventLog(); await log.follow(client)
        capture.grow(by: 6_400)
        await client.start()
        try await waitUntil("taken after three refusals") { connector.socket(0)?.throughSample == 6_400 }
        #expect(connector.dialCount == 4)
        // While it is turned away the line says so, not "reconnecting", and it is never final.
        let statuses = await log.statuses
        #expect(statuses.first == .connecting && statuses.contains(.providerUnavailable))
        #expect(!statuses.contains(.reconnecting))
        #expect(!statuses.contains { if case .unavailable = $0 { true } else { false } })
        try await waitUntil("live") { await client.status == .live }
        await client.cancel()
    }

    @Test func refusalsAndLimitsEndTheStreamButLostConnectionsAndSlowClientsRetry() {
        typealias Client = LiveTranscriptClient
        // A server that is not ready, a limit and an unavailable provider are dialled again.
        #expect(Client.terminal(.refused(status: 503), serverError: nil) == nil)
        #expect(Client.terminal(.refused(status: 429), serverError: nil) == nil)
        #expect(Client.terminal(.refused(status: 401), serverError: nil) == .unauthorized)
        #expect(Client.terminal(.refused(status: 403), serverError: nil) == .unauthorized)
        #expect(Client.terminal(.refused(status: 404), serverError: nil) == .notFound)
        #expect(Client.terminal(.refused(status: 409), serverError: nil) == .notRecording)
        #expect(Client.terminal(.refused(status: 426), serverError: nil) == .protocolError)
        #expect(Client.terminal(.refused(status: 502), serverError: nil) == nil)
        #expect(Client.terminal(.closed(code: 4400), serverError: nil) == .protocolError)
        #expect(Client.terminal(.closed(code: 4401), serverError: nil) == .unauthorized)
        #expect(Client.terminal(.closed(code: 4404), serverError: nil) == .notFound)
        #expect(Client.terminal(.closed(code: 4409), serverError: nil) == .replaced)
        for code in [1000, 1001, 1006, 1011, LiveWire.CloseCode.serviceRestart, 4408, 4429, 4503] { #expect(Client.terminal(.closed(code: code), serverError: nil) == nil) }
        #expect(Client.terminal(.transport, serverError: nil) == nil)
        // The `error` message that precedes a refusal close is honoured when the code itself is lost.
        let malformed = LiveWire.ServerError(code: "malformed", message: "", retryable: false, retryAfterMs: nil)
        #expect(Client.terminal(.transport, serverError: malformed) == .protocolError)
        let overLimit = LiveWire.ServerError(code: "over_limit", message: "", retryable: false, retryAfterMs: nil)
        let provider = LiveWire.ServerError(code: "provider_unavailable", message: "", retryable: false, retryAfterMs: nil)
        #expect(Client.terminal(.transport, serverError: overLimit) == nil)
        #expect(Client.terminal(.transport, serverError: provider) == nil)
        let slow = LiveWire.ServerError(code: "client_too_slow", message: "", retryable: true, retryAfterMs: 500)
        #expect(Client.terminal(.transport, serverError: slow) == nil)
        // Of the ones dialled again, these are the server turning the stream away, not a lost connection.
        #expect(Client.serverIsBusy(.refused(status: 503), serverError: nil) && Client.serverIsBusy(.refused(status: 429), serverError: nil))
        #expect(Client.serverIsBusy(.closed(code: 4429), serverError: nil) && Client.serverIsBusy(.closed(code: 4503), serverError: nil))
        #expect(Client.serverIsBusy(.transport, serverError: overLimit) && Client.serverIsBusy(.transport, serverError: provider))
        #expect(!Client.serverIsBusy(.transport, serverError: nil) && !Client.serverIsBusy(.closed(code: 1006), serverError: nil))
        #expect(!Client.serverIsBusy(.refused(status: 502), serverError: nil))
    }

    @Test func aLimitOrProviderCloseAfterConnectingIsDialledAgainAndAReplacedCaptureIsNot() async throws {
        for code in [4429, 4503] {
            let connector = FakeConnector([.accept(.init(closeAfterFrames: (1, code)))])
            let capture = MemoryCapture()
            let client = LiveTranscriptClient(connector: connector, source: capture, configuration: fastConfiguration())
            let log = EventLog(); await log.follow(client)
            capture.grow(by: 3_200)
            await client.start()
            // The second connection resumes from what the first acknowledged, and takes what follows.
            try await waitUntil("dialled again after \(code)") { connector.socket(1)?.receivedHello != nil }
            #expect(connector.socket(1)?.receivedHello?.streams[0].resumeFromSample == 3_200)
            #expect(await log.statuses.contains(.providerUnavailable))
            capture.grow(by: 3_200)
            try await waitUntil("the rest after \(code)") { connector.socket(1)?.throughSample == 6_400 }
            #expect(connector.socket(1)?.snapshot.frames.first?.first == 3_200)
            #expect(connector.socket(1)?.contentIsCorrect == true)
            await client.cancel()
        }
        // Another connection took this capture over: that one is final.
        let connector = FakeConnector([.accept(.init(closeAfterFrames: (1, 4409)))])
        let capture = MemoryCapture()
        let client = LiveTranscriptClient(connector: connector, source: capture, configuration: fastConfiguration())
        let log = EventLog(); await log.follow(client)
        capture.grow(by: 3_200)
        await client.start()
        await log.finished()
        #expect(connector.dialCount == 1)
        #expect(await log.statuses.last == .unavailable(.replaced))
    }

    @Test func aServerRestartingTheStreamIsDialledAgainAtOnceAndGetsTheBacklog() async throws {
        // The provider session was lost: `error provider_unavailable` (retryable), then close 1012.
        let capture = MemoryCapture()
        let connector = FakeConnector([.accept(.init(closeAfterFrames: (1, 1012)))])
        var configuration = fastConfiguration()
        configuration.backoff = [.seconds(30)] // any back-off at all would fail this test
        let client = LiveTranscriptClient(connector: connector, source: capture, configuration: configuration)
        let log = EventLog(); await log.follow(client)
        capture.grow(by: 3_200)
        await client.start()
        try await waitUntil("dialled again") { connector.socket(1)?.receivedHello != nil }
        let times = connector.dialTimes
        #expect(times[1] - times[0] < .seconds(2))
        #expect(connector.socket(1)?.receivedHello?.streams[0].resumeFromSample == 3_200)
        capture.grow(by: 3_200)
        try await waitUntil("the rest") { connector.socket(1)?.throughSample == 6_400 }
        #expect(!(await log.statuses).contains { if case .unavailable = $0 { true } else { false } })
        await client.cancel()
    }

    @Test func theWaitBetweenDialsGrowsToThirtySecondsAndStaysThere() {
        let delays = LiveTranscriptClient.Configuration(captureID: "c", baseMs: 0).backoff
        #expect(delays.first == .milliseconds(500))
        #expect(delays.last == .seconds(30))
        #expect(zip(delays, delays.dropFirst()).allSatisfy { $0 < $1 })
    }

    @Test func reconnectsBackOffAndASuccessfulConnectionResetsTheDelay() async throws {
        let capture = MemoryCapture()
        let connector = FakeConnector([.refuse(.transport), .refuse(.transport), .refuse(.transport), .refuse(.transport),
                                       .accept(.init(closeAfterFrames: (1, 1006))), .refuse(.transport)])
        var configuration = fastConfiguration()
        configuration.backoff = [.milliseconds(20), .milliseconds(40), .milliseconds(80)]
        let client = LiveTranscriptClient(connector: connector, source: capture, configuration: configuration)
        capture.grow(by: 3_200)
        await client.start()
        try await waitUntil("seven dials") { connector.dialCount == 7 }
        let times = connector.dialTimes
        let gaps = zip(times, times.dropFirst()).map { $1 - $0 }
        // 20, 40, 80, 80 (the last step repeats), then a connection; after it the delay starts over at 20.
        #expect(gaps[0] >= .milliseconds(20) && gaps[0] < .milliseconds(40))
        #expect(gaps[1] >= .milliseconds(40) && gaps[1] < .milliseconds(80))
        #expect(gaps[2] >= .milliseconds(80))
        #expect(gaps[3] >= .milliseconds(80))
        #expect(gaps[4] >= .milliseconds(20) && gaps[4] < .milliseconds(80))
        #expect(gaps[5] >= .milliseconds(40))
        await client.cancel()
    }

    @Test func serverTextStatusAndGapsReachTheApp() async throws {
        let capture = MemoryCapture()
        let connector = FakeConnector([.accept(.init(transcriptRevision: 41))])
        var configuration = fastConfiguration()
        configuration.serverBehindNotice = .milliseconds(30)
        let client = LiveTranscriptClient(connector: connector, source: capture, configuration: configuration)
        let log = EventLog(); await log.follow(client)
        await client.start()
        try await waitUntil("ready") { connector.last?.receivedHello != nil }
        let socket = try #require(connector.last)
        // A stream is briefly "behind" as it starts; that is not worth a status line...
        socket.push(#"{"type":"status","channel":"mic","state":"catching_up","lagMs":900}"#)
        socket.push(#"{"type":"status","channel":"mic","state":"live","lagMs":0}"#)
        try await Task.sleep(for: .milliseconds(60))
        #expect(await log.statuses.contains { if case .catchingUp = $0 { true } else { false } } == false)
        socket.push(#"{"type":"interim","channel":"mic","utteranceId":"11111111-1111-4111-8111-111111111111","rev":1,"text":"so if we","startMs":833100,"endMs":834000}"#)
        socket.push(#"{"type":"interim","channel":"mic","utteranceId":"11111111-1111-4111-8111-111111111111","rev":2,"text":"so if we move the pilot","startMs":833100,"endMs":835000,"speakerId":"mic:n:3","speakerLabel":"Speaker 3"}"#)
        socket.push(#"{"type":"status","channel":"mic","state":"provider_unavailable","lagMs":0}"#)
        socket.push(#"{"type":"gap","channel":"mic","fromMs":840000,"toMs":871000,"reason":"provider_unavailable"}"#)
        socket.push(#"{"type":"something_new","value":1}"#)
        socket.push("not json")
        // ...but one that stays behind is.
        socket.push(#"{"type":"status","channel":"mic","state":"catching_up","lagMs":19000}"#)
        try await waitUntil("behind for a while") { await log.statuses.contains(.catchingUp(lagMs: 0)) }
        socket.push(#"{"type":"status","channel":"mic","state":"live","lagMs":0}"#)
        try await waitUntil("events") {
            let statuses = await log.statuses
            return statuses.last == .live && statuses.contains(.providerUnavailable)
        }
        #expect(await log.interimTexts == ["so if we", "so if we move the pilot"])
        // The server being behind is reported without a figure: the figure is only ever this device's unsent audio.
        #expect(await log.statuses.contains(.catchingUp(lagMs: 0)))
        let events = await log.events
        #expect(events.contains { if case .ready(41, "fake", "fake-live") = $0 { true } else { false } })
        #expect(events.contains { if case .gap(let gap) = $0 { gap.reason == "provider_unavailable" && gap.toMs == 871_000 } else { false } })
        // The connection survived a message type it does not know and a frame that is not JSON.
        #expect(connector.dialCount == 1)
        await client.cancel()
    }

    @Test func aServerThatNeverAnswersHelloIsDroppedAndDialledAgain() async throws {
        let capture = MemoryCapture()
        let connector = FakeConnector([.accept(.init(sendsReady: false))])
        let client = LiveTranscriptClient(connector: connector, source: capture, configuration: fastConfiguration())
        capture.grow(by: 3_200)
        await client.start()
        try await waitUntil("redial") { connector.socket(1)?.throughSample == 3_200 }
        #expect(connector.socket(0)?.snapshot.frames.isEmpty == true) // no audio before `ready`
        #expect(connector.socket(0)?.isClosed == true)
        await client.cancel()
    }

    @Test func anIdleConnectionIsPinged() async throws {
        let capture = MemoryCapture()
        let connector = FakeConnector([])
        var configuration = fastConfiguration()
        configuration.pingAfterIdle = .milliseconds(20)
        let client = LiveTranscriptClient(connector: connector, source: capture, configuration: configuration)
        await client.start()
        try await waitUntil("pings") { (connector.last?.pingCount ?? 0) >= 2 }
        await client.cancel()
    }

    @Test func endingWithoutAConnectionAndNothingUnsentFinishesAtOnce() async throws {
        let capture = MemoryCapture()
        let connector = FakeConnector([], then: .refuse(.transport))
        var configuration = fastConfiguration()
        configuration.backoff = [.seconds(30)]
        let client = LiveTranscriptClient(connector: connector, source: capture, configuration: configuration)
        let log = EventLog(); await log.follow(client)
        await client.start()
        try await waitUntil("first dial") { connector.dialCount == 1 }
        let began = ContinuousClock.now
        await client.end(atSample: 0)
        await log.finished()
        #expect(ContinuousClock.now - began < .seconds(1))
        #expect(await log.statuses.last == .ended)
        // Nothing can be said on a connection that is not there, and no connection is made only to say it.
        #expect(await client.statistics.marksSent.isEmpty)
        #expect(connector.dialCount == 1)
    }

    @Test func endingWhileDisconnectedWithAudioUnsentDialsAtOnceAndHandsItOver() async throws {
        // The stream is cut and waiting 30 s to dial again when the recording stops.
        let capture = MemoryCapture()
        let connector = FakeConnector([.accept(.init(closeAfterFrames: (2, 1006))), .refuse(.transport)])
        var configuration = fastConfiguration()
        configuration.backoff = [.milliseconds(10), .seconds(30)]
        configuration.endGrace = .seconds(2)
        let client = LiveTranscriptClient(connector: connector, source: capture, configuration: configuration)
        capture.grow(by: 6_400)
        await client.start()
        try await waitUntil("drop") { connector.socket(0)?.isClosed == true }
        try await waitUntil("the long wait") { connector.dialCount == 2 }
        capture.grow(by: 16_000)
        let began = ContinuousClock.now
        await client.end(atSample: 22_400)
        // It did not wait out the 30 s: it dialled, sent the rest and said `end` at the capture's length.
        #expect(ContinuousClock.now - began < .seconds(4))
        let last = try #require(connector.socket(1))
        #expect(last.throughSample == 22_400 && last.contentIsCorrect)
        #expect(last.snapshot.marks.map(\.kind) == [.end])
        #expect(last.snapshot.marks.first?.atSample["mic"] == 22_400)
    }

    @Test func replayIsPacedAtTheCatchUpFactor() {
        #expect(LiveTranscriptClient.Configuration(captureID: "c", baseMs: 0).catchUpFactor == 3)
        #expect(LiveTranscriptClient.Configuration(captureID: "c", baseMs: 0).burstSeconds == 0.5)
        var budget = LiveSendBudget(sampleRate: 16_000, factor: 3)
        // At most half a second of audio at once, then never more than three times real time.
        var sent = 0
        while budget.take(1_600) { sent += 1_600 }
        #expect(sent == 8_000)
        for _ in 0..<100 {
            budget.refill(seconds: 0.1)
            while budget.take(1_600) { sent += 1_600 }
        }
        #expect(sent == 8_000 + 10 * 48_000)
        // Idle time does not bank more than the burst.
        budget.refill(seconds: 60)
        var burst = 0
        while budget.take(1_600) { burst += 1_600 }
        #expect(burst == 8_000)
    }
}

@Suite struct LiveWireTests {
    @Test func audioFrameHasTheTwelveByteHeaderOfTheSpecification() throws {
        let pcm = bytes([1, -2, 300])
        let frame = LiveWire.audioFrame(channel: .mic, firstSample: 0x0102_0304_0506, pcm: pcm)
        #expect(frame.count == 12 + 6)
        #expect(Array(frame.prefix(12)) == [1, 0, 0, 0, 0x06, 0x05, 0x04, 0x03, 0x02, 0x01, 0, 0])
        #expect(Array(frame.suffix(6)) == [0x01, 0x00, 0xfe, 0xff, 0x2c, 0x01]) // little-endian samples, untouched
        #expect(LiveWire.audioFrame(channel: .system, firstSample: 0, pcm: pcm)[1] == 1)
        let parsed = try #require(LiveWire.parseAudioFrame(frame))
        #expect(parsed.channel == .mic)
        #expect(parsed.firstSample == 0x0102_0304_0506)
        #expect(parsed.pcm == pcm)
        // 200 ms at 16 kHz is far below the 64 KiB frame limit.
        #expect(LiveWire.audioFrame(channel: .mic, firstSample: 0, pcm: Data(count: 6_400)).count == 6_412)
        // The server's refusals: no audio, odd length, wrong version, reserved bytes, unknown channel, too large.
        #expect(LiveWire.parseAudioFrame(Data(frame.prefix(12))) == nil)
        #expect(LiveWire.parseAudioFrame(frame + Data([0])) == nil)
        var bad = frame; bad[0] = 2
        #expect(LiveWire.parseAudioFrame(bad) == nil)
        bad = frame; bad[2] = 1
        #expect(LiveWire.parseAudioFrame(bad) == nil)
        bad = frame; bad[1] = 2
        #expect(LiveWire.parseAudioFrame(bad) == nil)
        #expect(LiveWire.parseAudioFrame(LiveWire.audioFrame(channel: .mic, firstSample: 0, pcm: Data(count: 65_536))) == nil)
    }

    @Test func helloAndMarksEncodeAsTheServerSchemaExpects() throws {
        let hello = LiveWire.Hello(captureId: "c", language: nil, streams: [
            LiveWire.Stream(channel: .mic, sampleRate: 16_000, baseMs: 754_000, resumeFromSample: 1_280_000)
        ])
        #expect(try LiveWire.encode(hello) == #"{"captureId":"c","protocol":1,"streams":[{"baseMs":754000,"channel":"mic","encoding":"pcm_s16le","resumeFromSample":1280000,"sampleRate":16000}],"type":"hello"}"#)
        var withLanguage = hello; withLanguage.language = "en"
        #expect(try LiveWire.encode(withLanguage).contains(#""language":"en""#))
        #expect(try LiveWire.encode(LiveWire.Mark(kind: .pause, atSample: ["mic": 1_234_567])) == #"{"atSample":{"mic":1234567},"kind":"pause","type":"mark"}"#)
        #expect(try LiveWire.encode(LiveWire.Mark(kind: .end, atSample: ["mic": 5])).contains(#""kind":"end""#))
    }

    @Test func everyServerMessageOfTheSpecificationDecodes() throws {
        guard case .ready(let ready) = try LiveWire.decode(#"{"type":"ready","sessionId":"7f0c9d7e-51f4-4c4b-9d0c-0d6f1f1c2a11","provider":"elevenlabs","model":"scribe_v3_live","streams":[{"channel":"mic","acceptFromSample":0},{"channel":"system","acceptFromSample":12}],"resumeWindowMs":30000,"transcriptRevision":41}"#) else { Issue.record("ready"); return }
        #expect(ready.streams[1].acceptFromSample == 12 && ready.resumeWindowMs == 30_000 && ready.transcriptRevision == 41)
        guard case .ack(let ack) = try LiveWire.decode(#"{"type":"ack","channel":"mic","throughSample":1280000,"transcribedThroughMs":832400}"#) else { Issue.record("ack"); return }
        #expect(ack.throughSample == 1_280_000 && ack.transcribedThroughMs == 832_400)
        guard case .interim(let interim) = try LiveWire.decode(#"{"type":"interim","channel":"system","utteranceId":"u","rev":7,"text":"so if we move the pilot to","startMs":833100,"endMs":835000,"speakerId":"system:n:3","speakerLabel":"Speaker 3"}"#) else { Issue.record("interim"); return }
        #expect(interim.channel == .system && interim.rev == 7 && interim.speakerLabel == "Speaker 3")
        guard case .final(let final) = try LiveWire.decode(#"{"type":"final","utteranceId":"u","transcriptRevision":42,"segment":{"id":"99ad1171-3771-4ae6-81a1-44f617b663db","channel":"mic","speakerId":"mic:n:1","speakerLabel":"Speaker 1","text":"Hello","isFinal":true,"startMs":10,"endMs":900}}"#) else { Issue.record("final"); return }
        #expect(final.transcriptRevision == 42 && final.segment.text == "Hello" && final.segment.isFinal)
        guard case .speakers(let speakers) = try LiveWire.decode(#"{"type":"speakers","merges":[{"from":"system:n:4","to":"system:n:2"}]}"#) else { Issue.record("speakers"); return }
        #expect(speakers.merges == [.init(from: "system:n:4", to: "system:n:2")] && speakers.relabels == nil)
        guard case .speakers(let relabelled) = try LiveWire.decode(#"{"type":"speakers","merges":[],"relabels":[{"segmentId":"s1","speakerId":"mic:n:2","speakerLabel":"Room speaker 1"}]}"#) else { Issue.record("relabels"); return }
        #expect(relabelled.relabels == [.init(segmentId: "s1", speakerId: "mic:n:2", speakerLabel: "Room speaker 1")])
        guard case .gap(let gap) = try LiveWire.decode(#"{"type":"gap","channel":"mic","fromMs":840000,"toMs":871000,"reason":"resume_window_exceeded"}"#) else { Issue.record("gap"); return }
        #expect(gap.fromMs == 840_000 && gap.reason == "resume_window_exceeded")
        guard case .status(let status) = try LiveWire.decode(#"{"type":"status","channel":"mic","state":"catching_up","lagMs":1800}"#) else { Issue.record("status"); return }
        #expect(status.streamState == .catchingUp && status.lagMs == 1_800)
        guard case .status(let later) = try LiveWire.decode(#"{"type":"status","channel":"mic","state":"a_later_state","lagMs":0}"#) else { Issue.record("status"); return }
        #expect(later.streamState == nil)
        guard case .error(let error) = try LiveWire.decode(#"{"type":"error","code":"provider_unavailable","message":"…","retryable":true,"retryAfterMs":5000}"#) else { Issue.record("error"); return }
        #expect(error.retryable && error.retryAfterMs == 5_000)
        guard case .unknown("later") = try LiveWire.decode(#"{"type":"later"}"#) else { Issue.record("unknown"); return }
        #expect(throws: APIClientError.invalidJSON) { try LiveWire.decode(#"{"type":"ack"}"#) }
        #expect(throws: APIClientError.invalidJSON) { try LiveWire.decode("[]") }
    }
}
