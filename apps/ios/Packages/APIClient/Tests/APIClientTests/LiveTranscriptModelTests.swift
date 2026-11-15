import Foundation
import Network
import Testing
import Contracts
@testable import APIClient

private let noteID = "3a49a4af-c660-42e9-a36a-a806fbb41e0d"
private let utteranceA = "11111111-1111-4111-8111-111111111111"
private let utteranceB = "22222222-2222-4222-8222-222222222222"

private func interim(_ utterance: String, rev: Int, _ text: String, startMs: Int = 1_000, speaker: String? = nil) -> LiveTranscriptEvent {
    .interim(LiveWire.Interim(channel: .mic, utteranceId: utterance, rev: rev, text: text, startMs: startMs, endMs: startMs + 500,
                              speakerId: speaker.map { _ in "mic:n:1" }, speakerLabel: speaker))
}

private func segment(_ id: String, _ text: String, startMs: Int, speaker: String = "mic:n:1", label: String? = "Speaker 1") -> TranscriptSegmentDTO {
    TranscriptSegmentDTO(channel: .mic, endMs: startMs + 900, id: id, isFinal: true, speakerId: speaker, speakerLabel: label, startMs: startMs, text: text)
}

private func final(_ utterance: String, revision: Int, _ segment: TranscriptSegmentDTO) -> LiveTranscriptEvent {
    .final(LiveWire.Final(utteranceId: utterance, transcriptRevision: revision, segment: segment))
}

private func note(revision: Int, _ segments: [TranscriptSegmentDTO]) -> NoteDTO {
    NoteDTO(body: "", createdAt: "2026-11-14T10:00:00.000Z", date: "2026-11-14", duration: "0:10", enhanced: "", generatedRevision: nil,
            generationState: .idle, id: noteID, recordingState: .recording, title: "Live", transcriptRevision: revision,
            transcriptSegments: segments, updatedAt: "2026-11-14T10:00:00.000Z")
}

@MainActor @Suite struct LiveTranscriptModelTests {
    @Test func interimTextIsRevisedInPlaceAndTheFinalTakesOverItsLine() {
        let transcript = LiveTranscript(noteID: noteID)
        transcript.merge(note: note(revision: 4, [segment("aaaaaaaa-0000-4000-8000-000000000001", "Earlier.", startMs: 0)]))
        #expect(transcript.apply(interim(utteranceA, rev: 1, "so if")) == false)
        let utterance = transcript.utterances[0]
        transcript.apply(interim(utteranceA, rev: 2, "so if we move"))
        transcript.apply(interim(utteranceA, rev: 3, "so if we move the pilot", speaker: "Speaker 1"))
        // One utterance object for the whole utterance: only its text changed.
        #expect(transcript.utterances.count == 1)
        #expect(transcript.utterances[0] === utterance)
        #expect(utterance.text == "so if we move the pilot")
        #expect(utterance.speakerLabel == "Speaker 1")
        // An older revision arriving late is ignored.
        transcript.apply(interim(utteranceA, rev: 2, "so if we move"))
        #expect(utterance.text == "so if we move the pilot")
        let linesBefore = transcript.lines.map(\.id)
        #expect(linesBefore == ["aaaaaaaa-0000-4000-8000-000000000001", utteranceA])

        let stored = segment("aaaaaaaa-0000-4000-8000-000000000002", "So if we move the pilot to March.", startMs: 1_000)
        #expect(transcript.apply(final(utteranceA, revision: 5, stored)) == false)
        #expect(transcript.utterances.isEmpty)
        #expect(transcript.segments.map(\.text) == ["Earlier.", "So if we move the pilot to March."])
        // The line keeps its identity, so the view settles in place; the stored segment keeps its own id for citations.
        #expect(transcript.lines.map(\.id) == linesBefore)
        #expect(transcript.lineID(forSegment: stored.id) == utteranceA)
        if case .stored(let row, _) = transcript.lines[1] { #expect(row.segment.id == stored.id && row.segment.isFinal) } else { Issue.record("expected a stored line") }
        // Interim text that arrives after its final is not shown again.
        transcript.apply(interim(utteranceA, rev: 9, "so if we move the pilot to March"))
        #expect(transcript.utterances.isEmpty)
    }

    @Test func oneUtteranceCanBecomeSeveralStoredSegments() {
        let transcript = LiveTranscript(noteID: noteID)
        transcript.apply(interim(utteranceA, rev: 1, "yes agreed and what about cost"))
        transcript.apply(final(utteranceA, revision: 1, segment("aaaaaaaa-0000-4000-8000-000000000001", "Yes, agreed.", startMs: 1_000)))
        transcript.apply(final(utteranceA, revision: 1, segment("aaaaaaaa-0000-4000-8000-000000000002", "And what about cost?", startMs: 2_000, speaker: "mic:n:2", label: "Speaker 2")))
        #expect(transcript.utterances.isEmpty)
        #expect(transcript.segments.map(\.text) == ["Yes, agreed.", "And what about cost?"])
        #expect(transcript.lines.map(\.id) == [utteranceA, "aaaaaaaa-0000-4000-8000-000000000002"])
        #expect(Set(transcript.lines.map(\.id)).count == 2)
    }

    @Test func aNoteReadAddsWhatTheBatchPathStoredAndNeverRemovesLiveText() {
        let transcript = LiveTranscript(noteID: noteID)
        transcript.merge(note: note(revision: 2, [segment("aaaaaaaa-0000-4000-8000-000000000001", "One.", startMs: 0)]))
        transcript.apply(final(utteranceA, revision: 3, segment("aaaaaaaa-0000-4000-8000-000000000003", "Three.", startMs: 60_000)))
        // A read taken before that final, which also carries a part the batch path filled in.
        transcript.merge(note: note(revision: 3, [
            segment("aaaaaaaa-0000-4000-8000-000000000001", "One.", startMs: 0),
            segment("aaaaaaaa-0000-4000-8000-000000000002", "Two.", startMs: 30_000),
        ]))
        #expect(transcript.segments.map(\.text) == ["One.", "Two.", "Three."])
        #expect(transcript.transcriptRevision == 3)
        // Interim segments in a note are not stored text.
        let draft = TranscriptSegmentDTO(channel: .mic, endMs: nil, id: "aaaaaaaa-0000-4000-8000-000000000009", isFinal: false, speakerId: "", speakerLabel: nil, startMs: 90_000, text: "draft")
        transcript.merge(note: note(revision: 3, [draft]))
        #expect(transcript.segments.count == 3)
        // Another note's read is ignored.
        let other = NoteDTO(body: "", createdAt: "", date: "", duration: "", enhanced: "", generatedRevision: nil, generationState: .idle,
                            id: UUID().uuidString, recordingState: .idle, title: "", transcriptRevision: 9, transcriptSegments: [segment(UUID().uuidString, "Elsewhere.", startMs: 0)], updatedAt: "")
        transcript.merge(note: other)
        #expect(transcript.segments.count == 3)
    }

    @Test func aRevisionTheEventsDoNotExplainAsksForANoteRead() {
        let transcript = LiveTranscript(noteID: noteID)
        // Connecting to a note that already has a transcript.
        #expect(transcript.apply(.ready(transcriptRevision: 41, provider: "fake", model: "fake")) == true)
        transcript.merge(note: note(revision: 41, []))
        #expect(transcript.apply(.ready(transcriptRevision: 41, provider: "fake", model: "fake")) == false)
        #expect(transcript.apply(final(utteranceA, revision: 42, segment("aaaaaaaa-0000-4000-8000-000000000001", "A.", startMs: 0))) == false)
        // Revision 43 was something else (a durable part transcribed by the batch path).
        #expect(transcript.apply(final(utteranceB, revision: 44, segment("aaaaaaaa-0000-4000-8000-000000000002", "B.", startMs: 5_000))) == true)
    }

    @Test func speakerMergesRelabelStoredSegments() {
        let transcript = LiveTranscript(noteID: noteID)
        transcript.merge(note: note(revision: 3, [
            segment("aaaaaaaa-0000-4000-8000-000000000001", "One.", startMs: 0, speaker: "mic:n:2", label: "Speaker 2"),
            segment("aaaaaaaa-0000-4000-8000-000000000002", "Two.", startMs: 1_000, speaker: "mic:n:4", label: "Speaker 4"),
            segment("aaaaaaaa-0000-4000-8000-000000000003", "Three.", startMs: 2_000, speaker: "mic:n:5", label: "Speaker 5"),
        ]))
        transcript.apply(.speakers([.init(from: "mic:n:4", to: "mic:n:2"), .init(from: "mic:n:5", to: "mic:n:7")]))
        #expect(transcript.segments.map(\.speakerId) == ["mic:n:2", "mic:n:2", "mic:n:7"])
        #expect(transcript.segments.map(\.speakerLabel) == ["Speaker 2", "Speaker 2", "Speaker 7"])
        #expect(transcript.segments.map(\.text) == ["One.", "Two.", "Three."])
    }

    @Test func statusFollowsTheStreamAndNothingInterimOutlivesIt() {
        let transcript = LiveTranscript(noteID: noteID)
        #expect(transcript.status == .off)
        transcript.apply(.status(.live))
        transcript.apply(interim(utteranceA, rev: 1, "unfinished"))
        transcript.apply(.status(.catchingUp(lagMs: 4_000)))
        #expect(transcript.status == .catchingUp(lagMs: 4_000))
        #expect(transcript.utterances.count == 1) // a slow stream keeps its interim text
        transcript.apply(.status(.ended))
        #expect(transcript.utterances.isEmpty)
        transcript.apply(interim(utteranceB, rev: 1, "more"))
        transcript.apply(.status(.unavailable(.providerUnavailable)))
        #expect(transcript.utterances.isEmpty)
    }

    @Test func anEmptyInterimWithdrawsItsUtterance() {
        let transcript = LiveTranscript(noteID: noteID)
        transcript.apply(interim(utteranceA, rev: 1, "maybe"))
        transcript.apply(interim(utteranceB, rev: 1, "other"))
        transcript.apply(interim(utteranceA, rev: 2, ""))
        #expect(transcript.utterances.map(\.id) == [utteranceB])
        // Withdrawing something that is not shown changes nothing.
        transcript.apply(interim(utteranceA, rev: 3, ""))
        #expect(transcript.utterances.count == 1)
        // The recogniser may start the same utterance again.
        transcript.apply(interim(utteranceA, rev: 4, "maybe so"))
        #expect(transcript.utterances.map(\.text) == ["other", "maybe so"])
    }

    @Test func interimCaptionLeavesOutASpeakerItDoesNotHave() {
        #expect(TranscriptPresentation.interimCaption(channel: .mic, startMs: 72_000, speakerLabel: nil) == "Microphone · 1:12 · Interim")
        #expect(TranscriptPresentation.interimCaption(channel: .system, startMs: 0, speakerLabel: " Speaker 3 ") == "Speaker 3 · System audio · 0:00 · Interim")
    }
}

// MARK: - The real socket against a local server

/// A WebSocket server on localhost, built on Network.framework.
private final class LocalLiveServer: @unchecked Sendable {
    private let listener: NWListener
    private let queue = DispatchQueue(label: "local-live-server")
    private let lock = NSLock()
    private var connections: [NWConnection] = []
    private(set) var headers: [(name: String, value: String)] = []
    private(set) var texts: [String] = []
    private(set) var frames: [Data] = []
    var onText: (@Sendable (String, LocalLiveServer) -> Void)?

    init() throws {
        let options = NWProtocolWebSocket.Options()
        options.autoReplyPing = true
        let parameters = NWParameters.tcp
        parameters.defaultProtocolStack.applicationProtocols.insert(options, at: 0)
        listener = try NWListener(using: parameters, on: .any)
        options.setClientRequestHandler(queue) { [weak self] _, additionalHeaders in
            self?.lock.withLock { self?.headers = additionalHeaders }
            return NWProtocolWebSocket.Response(status: .accept, subprotocol: nil)
        }
        listener.newConnectionHandler = { [weak self] connection in
            guard let self else { return }
            lock.withLock { connections.append(connection) }
            connection.start(queue: queue)
            receive(on: connection)
        }
    }

    func start() async throws -> Int {
        try await withCheckedThrowingContinuation { continuation in
            listener.stateUpdateHandler = { [listener] state in
                switch state {
                case .ready: continuation.resume(returning: Int(listener.port!.rawValue)); listener.stateUpdateHandler = nil
                case .failed(let error): continuation.resume(throwing: error); listener.stateUpdateHandler = nil
                default: break
                }
            }
            listener.start(queue: queue)
        }
    }

    var received: (headers: [(name: String, value: String)], texts: [String], frames: [Data]) {
        lock.lock(); defer { lock.unlock() }
        return (headers, texts, frames)
    }

    private func receive(on connection: NWConnection) {
        connection.receiveMessage { [weak self] data, context, _, error in
            guard let self, error == nil, let data else { return }
            let metadata = context?.protocolMetadata(definition: NWProtocolWebSocket.definition) as? NWProtocolWebSocket.Metadata
            switch metadata?.opcode {
            case .text:
                let text = String(decoding: data, as: UTF8.self)
                lock.withLock { texts.append(text) }
                onText?(text, self)
            case .binary: lock.withLock { frames.append(data) }
            default: break
            }
            receive(on: connection)
        }
    }

    func send(_ text: String) {
        let metadata = NWProtocolWebSocket.Metadata(opcode: .text)
        let context = NWConnection.ContentContext(identifier: "text", metadata: [metadata])
        lock.withLock { connections.last }?.send(content: Data(text.utf8), contentContext: context, completion: .idempotent)
    }

    func close(code: UInt16) {
        let metadata = NWProtocolWebSocket.Metadata(opcode: .close)
        metadata.closeCode = .privateCode(code)
        let context = NWConnection.ContentContext(identifier: "close", metadata: [metadata])
        lock.withLock { connections.last }?.send(content: nil, contentContext: context, isComplete: true, completion: .idempotent)
    }

    func stop() { listener.cancel(); lock.withLock { connections }.forEach { $0.cancel() } }
}

/// A plain HTTP server that refuses the upgrade with one status, as the Cereal server does before upgrading.
private final class RefusingServer: @unchecked Sendable {
    private let listener: NWListener
    private let queue = DispatchQueue(label: "refusing-server")
    private let lock = NSLock()
    private var request = ""
    init(status: Int, reason: String, body: String) throws {
        listener = try NWListener(using: .tcp, on: .any)
        listener.newConnectionHandler = { [weak self] connection in
            guard let self else { return }
            connection.start(queue: queue)
            connection.receive(minimumIncompleteLength: 1, maximumLength: 65_536) { data, _, _, _ in
                self.lock.withLock { self.request = String(decoding: data ?? Data(), as: UTF8.self) }
                let response = "HTTP/1.1 \(status) \(reason)\r\nContent-Type: application/json\r\nContent-Length: \(body.utf8.count)\r\nConnection: close\r\n\r\n\(body)"
                connection.send(content: Data(response.utf8), completion: .contentProcessed { _ in connection.cancel() })
            }
        }
    }
    func start() async throws -> Int {
        try await withCheckedThrowingContinuation { continuation in
            listener.stateUpdateHandler = { [listener] state in
                switch state {
                case .ready: continuation.resume(returning: Int(listener.port!.rawValue)); listener.stateUpdateHandler = nil
                case .failed(let error): continuation.resume(throwing: error); listener.stateUpdateHandler = nil
                default: break
                }
            }
            listener.start(queue: queue)
        }
    }
    var requestText: String { lock.withLock { request } }
    func stop() { listener.cancel() }
}

@Suite(.serialized) struct LiveSocketTransportTests {
    private func api(port: Int) throws -> MeetingNotesAPI {
        try MeetingNotesAPI(credentials: InstanceCredentials(instance: InstanceURL("http://127.0.0.1:\(port)"), bearerToken: "test-bearer-token"))
    }

    @Test func theUpgradeRequestAuthenticatesWithTheBearerHeaderAndNothingElse() throws {
        let local = try api(port: 3000).liveTranscriptRequest(noteID: noteID)
        #expect(local.url?.absoluteString == "ws://127.0.0.1:3000/api/v1/notes/\(noteID)/live")
        #expect(local.value(forHTTPHeaderField: "Authorization") == "Bearer test-bearer-token")
        #expect(local.value(forHTTPHeaderField: "Origin") == nil)
        #expect(local.value(forHTTPHeaderField: "Cookie") == nil)
        let hosted = try MeetingNotesAPI(credentials: InstanceCredentials(instance: InstanceURL("https://notes.example.test/base/"), bearerToken: "t"))
        #expect(try hosted.liveTranscriptRequest(noteID: noteID).url?.absoluteString == "wss://notes.example.test/base/api/v1/notes/\(noteID)/live")
        #expect(throws: APIClientError.invalidIdentifier) { try hosted.liveTranscriptRequest(noteID: "../admin") }
    }

    @Test func aRealSocketCarriesHelloBinaryFramesServerTextAndCustomCloseCodes() async throws {
        let server = try LocalLiveServer()
        let port = try await server.start()
        defer { server.stop() }
        let connector = URLSessionLiveConnector(request: try api(port: port).liveTranscriptRequest(noteID: noteID))
        let socket = try await connector.connect()
        // The request headers themselves are checked byte for byte in `aRefusedUpgradeReportsItsHTTPStatus`.
        #expect(!server.received.headers.contains { ["origin", "cookie"].contains($0.name.lowercased()) })

        try await socket.send(text: #"{"type":"hello"}"#)
        let frame = LiveWire.audioFrame(channel: .mic, firstSample: 48_000, pcm: MemoryCapture.pcm(from: 48_000, count: 3_200))
        try await socket.send(data: frame)
        try await socket.ping()
        try await waitUntil("frames at the server") { server.received.frames.count == 1 && server.received.texts.count == 1 }
        #expect(server.received.frames[0] == frame)
        #expect(server.received.texts == [#"{"type":"hello"}"#])

        server.send(#"{"type":"ack","channel":"mic","throughSample":51200,"transcribedThroughMs":0}"#)
        guard case .text(let text) = try await socket.receive(), case .ack(let ack) = try LiveWire.decode(text) else { Issue.record("expected an ack"); return }
        #expect(ack.throughSample == 51_200)

        // An application close code arrives as that code, not as a generic failure.
        server.close(code: 4429)
        await #expect(throws: LiveSocketError.closed(code: 4429)) { _ = try await socket.receive() }
        socket.close(code: 1000)
    }

    @Test func aRefusedUpgradeReportsItsHTTPStatus() async throws {
        for status in [503, 401, 404, 409, 429] {
            let server = try RefusingServer(status: status, reason: "Refused", body: #"{"error":"unavailable","message":"Live transcription is not configured"}"#)
            let port = try await server.start()
            let connector = URLSessionLiveConnector(request: try api(port: port).liveTranscriptRequest(noteID: noteID))
            await #expect(throws: LiveSocketError.refused(status: status)) { _ = try await connector.connect() }
            // It was a WebSocket upgrade for the live route, with the bearer and no cookie.
            let request = server.requestText.lowercased()
            #expect(request.hasPrefix("get /api/v1/notes/\(noteID)/live http/1.1"))
            #expect(request.contains("upgrade: websocket"))
            #expect(request.contains("authorization: bearer test-bearer-token"))
            #expect(!request.contains("cookie:"))
            #expect(!request.contains("origin:"))
            server.stop()
        }
    }

    @Test func aClientAgainstARealSocketFallsBackSilentlyWhenLiveIsNotConfigured() async throws {
        let server = try RefusingServer(status: 503, reason: "Service Unavailable", body: #"{"error":"unavailable","message":"Live transcription is not configured"}"#)
        let port = try await server.start()
        defer { server.stop() }
        let capture = MemoryCapture()
        capture.grow(by: 16_000)
        let client = try api(port: port).liveTranscriptClient(noteID: noteID, source: capture,
            configuration: LiveTranscriptClient.Configuration(captureID: "c", baseMs: 0))
        let log = EventLog(); await log.follow(client)
        await client.start()
        await log.finished()
        #expect(await log.statuses == [.connecting, .unavailable(.notConfigured)])
    }

    @Test func anUnreachableServerIsATransportFailure() async throws {
        // A port nothing listens on.
        let server = try RefusingServer(status: 503, reason: "x", body: "")
        let port = try await server.start()
        server.stop()
        try await Task.sleep(for: .milliseconds(100))
        let connector = URLSessionLiveConnector(request: try api(port: port).liveTranscriptRequest(noteID: noteID), openTimeout: .seconds(3))
        await #expect(throws: LiveSocketError.transport) { _ = try await connector.connect() }
    }
}
