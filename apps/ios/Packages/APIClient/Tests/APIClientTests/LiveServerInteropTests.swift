import Foundation
import Testing
import Contracts
@testable import APIClient

/// Explicit opt-in against a running Cereal server with live transcription on,
/// normally the in-memory development server with its fake recogniser:
///
///     LIVE_DEV_PORT=3212 bun scripts/live/dev-server.ts
///     RUN_LIVE_SOCKET_TESTS=1 CEREAL_QA_INSTANCE_URL=http://127.0.0.1:3212 \
///       CEREAL_QA_OWNER_TOKEN=<dev token> swift test --package-path Packages/APIClient --filter LiveServerInteropTests
///
/// The audio is synthetic: bursts of tone, which the fake recogniser turns
/// into words, separated by silence, which ends an utterance.
@Suite(.serialized) struct LiveServerInteropTests {
    static let enabled = ProcessInfo.processInfo.environment["RUN_LIVE_SOCKET_TESTS"] == "1"

    private func api() throws -> MeetingNotesAPI {
        let env = ProcessInfo.processInfo.environment
        return try MeetingNotesAPI(credentials: InstanceCredentials(
            instance: InstanceURL(try #require(env["CEREAL_QA_INSTANCE_URL"])), bearerToken: try #require(env["CEREAL_QA_OWNER_TOKEN"])))
    }

    /// `seconds` of a 220 Hz tone at about −12 dBFS, or of silence.
    private func audio(seconds: Double, tone: Bool, startingAt sample: Int) -> [Int16] {
        (0..<Int(seconds * 16_000)).map { tone ? Int16(8_000 * sin(2 * .pi * 220 * Double(sample + $0) / 16_000)) : 0 }
    }

    /// Appends at real-time pace, as a capture grows.
    private func record(_ samples: [Int16], into writer: CaptureFileWriter) async throws {
        var offset = 0
        while offset < samples.count {
            let count = min(1_600, samples.count - offset)
            try Array(samples[offset..<(offset + count)]).withUnsafeBytes { try writer.append($0) }
            offset += count
            try await Task.sleep(for: .milliseconds(100))
        }
    }

    @Test(.enabled(if: enabled))
    func aRealServerAcceptsTheStreamAndAnswersWithInterimAndFinalText() async throws {
        let api = try api()
        let note = try await api.createNote(CreateNoteInput(body: nil, date: nil, duration: nil, enhanced: nil, title: "Swift live interop", transcriptSegments: nil))
        _ = try await api.recording(noteID: note.id, command: .start)
        let root = temporaryCaptureDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let url = root.appendingPathComponent("capture-0.wav")
        let writer = try CaptureFileWriter(url: url)
        // The real socket, behind a connector that can cut the connection the way a network does.
        let connector = DroppableConnector(URLSessionLiveConnector(request: try api.liveTranscriptRequest(noteID: note.id)))
        let client = LiveTranscriptClient(connector: connector, source: try CaptureFileReader(url: url),
            configuration: LiveTranscriptClient.Configuration(captureID: "\(note.id)/capture-0.wav", baseMs: 0))
        let log = EventLog(); await log.follow(client)
        await client.start()

        // Three utterances: tone, then enough quiet to end each.
        for _ in 0..<3 {
            try await record(audio(seconds: 2, tone: true, startingAt: Int(writer.frames)), into: writer)
            try await record(audio(seconds: 0.8, tone: false, startingAt: 0), into: writer)
        }
        try await waitUntil("three finals", timeout: .seconds(10)) { await log.finalTexts.count >= 3 }
        #expect(await log.interimTexts.count >= 3)
        #expect(await log.statuses.contains(.live))
        // The server acknowledged everything that was sent, to within the frame still being filled.
        let recorded = writer.frames
        try await waitUntil("acks", timeout: .seconds(5)) { await client.acknowledged >= recorded - 3_200 }
        #expect(await client.statistics.skips == 0)

        // Pause and resume at the capture's sample.
        await client.pause(atSample: writer.frames)
        try await waitUntil("paused", timeout: .seconds(5)) { await client.status == .paused }
        #expect(await client.cursor == writer.frames)
        await client.resume()
        try await record(audio(seconds: 2, tone: true, startingAt: Int(writer.frames)), into: writer)
        try await record(audio(seconds: 0.8, tone: false, startingAt: 0), into: writer)
        try await waitUntil("a final after resume", timeout: .seconds(10)) { await log.finalTexts.count >= 4 }

        // A dropped connection: the client comes back as the same capture and carries on from its acknowledgement.
        let connectionsBefore = await client.statistics.connections
        connector.drop()
        try await record(audio(seconds: 2, tone: true, startingAt: Int(writer.frames)), into: writer)
        try await record(audio(seconds: 0.8, tone: false, startingAt: 0), into: writer)
        try await waitUntil("reconnected", timeout: .seconds(10)) { await client.statistics.connections == connectionsBefore + 1 }
        try await waitUntil("a final after reconnect", timeout: .seconds(15)) { await log.finalTexts.count >= 5 }

        // End: the last words arrive before the stream finishes, and the stream finishes by itself.
        try await record(audio(seconds: 1.5, tone: true, startingAt: Int(writer.frames)), into: writer)
        try writer.close()
        await client.end(atSample: writer.frames)
        await log.finished()
        #expect(await log.statuses.last == .ended)
        #expect(await client.statistics.marksSent == [.pause, .resume, .end])
        let finals = await log.finalTexts
        #expect(finals.count >= 6)

        _ = try await api.recording(noteID: note.id, command: .stop)
        let stored = try await api.note(id: note.id)
        // One transcript: what the stream reported as final is what the note holds, in order, with no duplicates.
        #expect(stored.transcriptSegments.map(\.text) == finals)
        #expect(Set(stored.transcriptSegments.map(\.id)).count == finals.count)
        #expect(stored.transcriptSegments.allSatisfy { $0.isFinal && $0.channel == .mic && $0.speakerId.hasPrefix("mic:n:") })
        // Segment times are on the capture's timeline and inside the audio that was recorded.
        let endMs = Int(writer.frames) / 16
        #expect(stored.transcriptSegments.allSatisfy { $0.startMs >= 0 && ($0.endMs ?? 0) <= endMs + 500 })
        print("Live interop: \(finals.count) finals, \(await log.interimTexts.count) interims, \(await client.statistics.framesSent) frames, \(await client.statistics.connections) connections")
        _ = try await api.deleteNote(id: note.id)
    }

    @Test(.enabled(if: enabled))
    func aNoteThatIsNotBeingRecordedIsRefusedOnceAndForGood() async throws {
        let api = try api()
        let note = try await api.createNote(CreateNoteInput(body: nil, date: nil, duration: nil, enhanced: nil, title: "Swift live refusal", transcriptSegments: nil))
        let capture = MemoryCapture()
        capture.grow(by: 16_000)
        let client = try api.liveTranscriptClient(noteID: note.id, source: capture, configuration: .init(captureID: "c", baseMs: 0))
        let log = EventLog(); await log.follow(client)
        await client.start()
        await log.finished()
        #expect(await log.statuses == [.connecting, .unavailable(.notRecording)])
        // A wrong token is refused as unauthorised, and a note that does not exist as not found.
        let env = ProcessInfo.processInfo.environment
        let stranger = try MeetingNotesAPI(credentials: InstanceCredentials(instance: InstanceURL(try #require(env["CEREAL_QA_INSTANCE_URL"])), bearerToken: "not-the-token-not-the-token-not-the-token"))
        let refused = try stranger.liveTranscriptClient(noteID: note.id, source: capture, configuration: .init(captureID: "c", baseMs: 0))
        let refusedLog = EventLog(); await refusedLog.follow(refused)
        await refused.start()
        await refusedLog.finished()
        #expect(await refusedLog.statuses.last == .unavailable(.unauthorized))
        let missing = try api.liveTranscriptClient(noteID: UUID().uuidString.lowercased(), source: capture, configuration: .init(captureID: "c", baseMs: 0))
        let missingLog = EventLog(); await missingLog.follow(missing)
        await missing.start()
        await missingLog.finished()
        #expect(await missingLog.statuses.last == .unavailable(.notFound))
        _ = try await api.deleteNote(id: note.id)
    }
}

/// Hands out real sockets and can close the one in use, as a lost network would.
private final class DroppableConnector: LiveSocketConnector, @unchecked Sendable {
    private let inner: any LiveSocketConnector
    private let lock = NSLock()
    private var current: (any LiveSocket)?
    init(_ inner: any LiveSocketConnector) { self.inner = inner }
    func connect() async throws -> any LiveSocket {
        let socket = try await inner.connect()
        lock.withLock { current = socket }
        return socket
    }
    func drop() { lock.withLock { current }?.close(code: 1001) }
}
