import AVFoundation
import Foundation
import SwiftUI
import Testing
import UIKit
import APIClient
import Contracts
@testable import Cereal

// MARK: - A stand-in for the instance

/// The HTTP side of an instance, in memory: the note, its audio chunks and
/// every request made. Shared with the scripted live socket below, which stores
/// final segments here the way the server stores them in the note.
final class StubInstance: @unchecked Sendable {
    struct Request { let method: String; let path: String; let at: ContinuousClock.Instant }
    struct Upload { let chunkID: String; let sequence: Int; let startMs: Int; let audio: Data; let at: ContinuousClock.Instant }

    static let principal = "00000000-0000-4000-8000-000000000001"
    let noteID = UUID().uuidString.lowercased()
    private let lock = NSLock()
    private var requests: [Request] = []
    private var uploads: [Upload] = []
    private var chunkStates: [String: String] = [:]
    private var segments: [[String: Any]] = []
    private var revision = 0
    private var recordingState = "idle"
    /// Chunks with these sequences stay "processing" until released.
    var heldSequences: Set<Int> = []
    /// Sessions the server no longer accepts: every request with one is refused in the API's own words.
    var refusedTokens: Set<String> = []
    /// When set, what an upload, the list of chunks, or the note itself is answered with instead.
    var uploadStatus: Int?
    var chunksStatus: Int?
    var noteStatus: Int?
    func set(refused: Set<String>? = nil, upload: Int?? = nil, chunks: Int?? = nil, note: Int?? = nil) {
        lock.withLock {
            if let refused { refusedTokens = refused }
            if let upload { uploadStatus = upload }
            if let chunks { chunksStatus = chunks }
            if let note { noteStatus = note }
        }
    }

    var allRequests: [Request] { lock.withLock { requests } }
    var allUploads: [Upload] { lock.withLock { uploads } }
    var transcriptRevision: Int { lock.withLock { revision } }
    func count(_ method: String, suffix: String) -> Int { lock.withLock { requests.filter { $0.method == method && $0.path.hasSuffix(suffix) }.count } }
    func release() { lock.withLock { heldSequences = []; for upload in uploads { chunkStates[upload.chunkID] = "ready" } } }

    /// What the server does with a final from the live path.
    func store(segment: [String: Any]) -> Int {
        lock.withLock { segments.append(segment); revision += 1; return revision }
    }

    private func noteJSON() -> [String: Any] {
        ["id": noteID, "title": "Live session", "body": "", "enhanced": "", "date": "2026-11-14", "duration": "0:00",
         "createdAt": "2026-11-14T10:00:00.000Z", "updatedAt": "2026-11-14T10:00:00.000Z", "recordingState": recordingState,
         "generationState": "idle", "transcriptRevision": revision, "generatedRevision": NSNull(), "transcriptSegments": segments]
    }

    private func chunkJSON(_ upload: Upload) -> [String: Any] {
        ["id": upload.chunkID, "noteId": noteID, "channel": "mic", "sequence": upload.sequence, "startMs": upload.startMs,
         "durationMs": max(1, (upload.audio.count - 44) / 32), "error": NSNull(), "state": chunkStates[upload.chunkID] ?? "queued"]
    }

    func respond(_ request: URLRequest, body: Data) -> (Int, Data) {
        lock.lock(); defer { lock.unlock() }
        let path = request.url?.path ?? ""
        let method = request.httpMethod ?? "GET"
        requests.append(Request(method: method, path: path, at: .now))
        func json(_ value: Any) -> (Int, Data) { (200, try! JSONSerialization.data(withJSONObject: value)) }
        func refusal(_ status: Int) -> (Int, Data) { (status, Data(#"{"error":"\#(status == 401 ? "unauthorized" : "not_found")","message":"Stub"}"#.utf8)) }
        let token = request.value(forHTTPHeaderField: "Authorization").map { String($0.dropFirst("Bearer ".count)) } ?? ""
        if refusedTokens.contains(token) { return refusal(401) }
        if path.hasSuffix("/api/auth/sign-out") { return json([String: Any]()) }
        if let noteStatus, path.hasSuffix("/notes/\(noteID)") || path.hasSuffix("/notes/\(noteID)/recording") { return refusal(noteStatus) }
        if let chunksStatus, path.hasSuffix("/audio/chunks") { return refusal(chunksStatus) }
        if let uploadStatus, method == "PUT", path.contains("/audio/chunks/") { return refusal(uploadStatus) }
        if path.hasSuffix("/api/v1/session") {
            return json(["user": ["id": Self.principal, "name": "Test Owner", "email": "owner@example.test", "role": "owner", "createdAt": "2026-11-12T00:00:00Z"], "expiresAt": NSNull()])
        }
        if path.hasSuffix("/api/v1/notes") { return json([noteJSON()]) }
        if path.hasSuffix("/notes/\(noteID)") { return json(noteJSON()) } // GET, and PATCH from the editor
        if path.hasSuffix("/notes/\(noteID)/recording") {
            let command = ((try? JSONSerialization.jsonObject(with: body)) as? [String: Any])?["command"] as? String
            recordingState = command == "stop" ? "stopped" : "recording"
            return json(noteJSON())
        }
        if path.hasSuffix("/audio/chunks") { return json(uploads.map(chunkJSON)) }
        if method == "PUT", let range = path.range(of: "/audio/chunks/") {
            let chunkID = String(path[range.upperBound...])
            let input = (try? JSONSerialization.jsonObject(with: body)) as? [String: Any] ?? [:]
            let sequence = input["sequence"] as? Int ?? -1
            let upload = Upload(chunkID: chunkID, sequence: sequence, startMs: input["startMs"] as? Int ?? -1,
                                audio: Data(base64Encoded: input["audioBase64"] as? String ?? "") ?? Data(), at: .now)
            if !uploads.contains(where: { $0.chunkID == chunkID }) { uploads.append(upload) }
            chunkStates[chunkID] = heldSequences.contains(sequence) ? "processing" : "ready"
            return json(chunkJSON(upload))
        }
        return (404, Data(#"{"error":"not_found","message":"No such route in the stub"}"#.utf8))
    }
}

private final class StubInstanceProtocol: URLProtocol, @unchecked Sendable {
    nonisolated(unsafe) static var instances: [String: StubInstance] = [:]
    static let lock = NSLock()
    static func register(_ instance: StubInstance) -> String {
        let key = UUID().uuidString
        lock.withLock { instances[key] = instance }
        return key
    }
    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func startLoading() {
        let key = request.value(forHTTPHeaderField: "X-Stub-Instance") ?? ""
        guard let instance = Self.lock.withLock({ Self.instances[key] }) else {
            client?.urlProtocol(self, didFailWithError: URLError(.badServerResponse)); return
        }
        var body = request.httpBody ?? Data()
        if let stream = request.httpBodyStream {
            stream.open(); defer { stream.close() }
            var buffer = [UInt8](repeating: 0, count: 65_536)
            while true {
                let count = stream.read(&buffer, maxLength: buffer.count)
                if count <= 0 { break }
                body.append(contentsOf: buffer.prefix(count))
            }
        }
        let (status, data) = instance.respond(request, body: body)
        let response = HTTPURLResponse(url: request.url!, statusCode: status, httpVersion: "HTTP/1.1", headerFields: ["Content-Type": "application/json"])!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: data)
        client?.urlProtocolDidFinishLoading(self)
    }
    override func stopLoading() {}
}

private final class MemoryTokens: BearerTokenStore, @unchecked Sendable {
    private let lock = NSLock()
    private var token: String?
    func load() throws -> String? { lock.withLock { token } }
    func save(_ token: String) throws { lock.withLock { self.token = token } }
    func clear() throws { lock.withLock { token = nil } }
}

// MARK: - A scripted live socket

/// The server's side of the live socket, driven by the audio it receives: an
/// interim revision for every frame and a final every `framesPerUtterance`
/// frames. Interim text carries a marker that final text never does.
final class ScriptedLiveServer: LiveSocketConnector, @unchecked Sendable {
    static let interimMarker = "…interim-draft"
    let instance: StubInstance
    let framesPerUtterance: Int
    private let lock = NSLock()
    private(set) var connections = 0
    private(set) var frames = 0
    private(set) var samples: Int64 = 0
    private(set) var interims = 0
    private(set) var finals = 0
    private(set) var marks: [String] = []
    /// `atSample` of every `end` mark, for the microphone channel.
    private(set) var endSamples: [Int64] = []
    var endMarks: [Int64] { lock.withLock { endSamples } }
    var refusal: LiveSocketError?

    init(instance: StubInstance, framesPerUtterance: Int = 20) {
        self.instance = instance
        self.framesPerUtterance = framesPerUtterance
    }

    var counts: (connections: Int, frames: Int, samples: Int64, interims: Int, finals: Int, marks: [String]) {
        lock.withLock { (connections, frames, samples, interims, finals, marks) }
    }

    func connect() async throws -> any LiveSocket {
        if let refusal { throw refusal }
        lock.withLock { connections += 1 }
        return Socket(server: self)
    }

    fileprivate func noteEnd(_ sample: Int64) { lock.withLock { endSamples.append(sample) } }

    fileprivate func note(frames added: Int, samples count: Int64, interims i: Int, finals f: Int, mark: String? = nil) {
        lock.withLock { frames += added; samples += count; interims += i; finals += f; if let mark { marks.append(mark) } }
    }

    private final class Socket: LiveSocket, @unchecked Sendable {
        private let server: ScriptedLiveServer
        private let lock = NSLock()
        private var inbox: [LiveSocketMessage] = []
        private var waiter: CheckedContinuation<LiveSocketMessage, Error>?
        private var closed: Int?
        private var utterance = UUID().uuidString.lowercased()
        private var revision = 0
        private var framesInUtterance = 0
        private var utteranceStartMs = 0
        private var words: [String] = []
        private var baseMs = 0
        private static let vocabulary = ["so", "if", "we", "move", "the", "pilot", "to", "March", "then", "billing", "can", "ship", "first", "and", "support", "has", "two", "weeks", "to", "prepare"]

        init(server: ScriptedLiveServer) { self.server = server }

        private func push(_ json: String) {
            lock.lock()
            if let waiter { self.waiter = nil; lock.unlock(); waiter.resume(returning: .text(json)) }
            else { inbox.append(.text(json)); lock.unlock() }
        }

        func send(text: String) async throws {
            try check()
            let object = (try? JSONSerialization.jsonObject(with: Data(text.utf8))) as? [String: Any] ?? [:]
            if object["type"] as? String == "hello" {
                let stream = (object["streams"] as? [[String: Any]])?.first
                baseMs = stream?["baseMs"] as? Int ?? 0
                let resume = stream?["resumeFromSample"] as? Int ?? 0
                push(#"{"type":"ready","sessionId":"7f0c9d7e-51f4-4c4b-9d0c-0d6f1f1c2a11","provider":"fake","model":"fake-live","streams":[{"channel":"mic","acceptFromSample":\#(resume)}],"resumeWindowMs":30000,"transcriptRevision":\#(server.instance.transcriptRevision)}"#)
                push(#"{"type":"status","channel":"mic","state":"live","lagMs":0}"#)
            } else if object["type"] as? String == "mark", let kind = object["kind"] as? String {
                server.note(frames: 0, samples: 0, interims: 0, finals: 0, mark: kind)
                if kind == "end" {
                    let sample = ((object["atSample"] as? [String: Any])?["mic"] as? NSNumber)?.int64Value ?? -1
                    server.noteEnd(sample)
                    if !words.isEmpty {
                        finalize(endMs: utteranceStartMs + 1)
                        server.note(frames: 0, samples: 0, interims: 0, finals: 1)
                    }
                    close(code: 1000)
                }
            }
        }

        func send(data: Data) async throws {
            try check()
            guard let frame = LiveWire.parseAudioFrame(data) else { close(code: 4400); return }
            let count = Int64(frame.pcm.count / 2)
            let endMs = baseMs + Int((frame.firstSample + count) * 1000 / 16_000)
            if words.isEmpty { utteranceStartMs = baseMs + Int(frame.firstSample * 1000 / 16_000) }
            words.append(Self.vocabulary[(framesInUtterance + words.count) % Self.vocabulary.count])
            revision += 1
            framesInUtterance += 1
            push(#"{"type":"ack","channel":"mic","throughSample":\#(frame.firstSample + count),"transcribedThroughMs":\#(utteranceStartMs)}"#)
            push(#"{"type":"interim","channel":"mic","utteranceId":"\#(utterance)","rev":\#(revision),"text":"\#(words.joined(separator: " ")) \#(ScriptedLiveServer.interimMarker)","startMs":\#(utteranceStartMs),"endMs":\#(endMs),"speakerId":"mic:n:1","speakerLabel":"Speaker 1"}"#)
            var finals = 0
            if framesInUtterance >= server.framesPerUtterance { finalize(endMs: endMs); finals = 1 }
            server.note(frames: 1, samples: count, interims: 1, finals: finals)
        }

        private func finalize(endMs: Int) {
            let id = UUID().uuidString.lowercased()
            let text = words.joined(separator: " ").prefix(1).uppercased() + words.joined(separator: " ").dropFirst() + "."
            let segment: [String: Any] = ["id": id, "channel": "mic", "speakerId": "mic:n:1", "speakerLabel": "Speaker 1", "text": text,
                                          "isFinal": true, "startMs": utteranceStartMs, "endMs": endMs]
            let revision = server.instance.store(segment: segment)
            let json = String(decoding: try! JSONSerialization.data(withJSONObject: segment), as: UTF8.self)
            push(#"{"type":"final","utteranceId":"\#(utterance)","transcriptRevision":\#(revision),"segment":\#(json)}"#)
            utterance = UUID().uuidString.lowercased()
            self.revision = 0
            framesInUtterance = 0
            words = []
        }

        private func check() throws {
            if let code = lock.withLock({ closed }) { throw LiveSocketError.closed(code: code) }
        }

        func receive() async throws -> LiveSocketMessage {
            try await withCheckedThrowingContinuation { continuation in
                lock.lock()
                if !inbox.isEmpty { let next = inbox.removeFirst(); lock.unlock(); continuation.resume(returning: next) }
                else if let closed { lock.unlock(); continuation.resume(throwing: LiveSocketError.closed(code: closed)) }
                else { waiter = continuation; lock.unlock() }
            }
        }

        func ping() async throws { try check() }

        func close(code: Int) {
            lock.lock()
            closed = closed ?? code
            // Messages already queued are still delivered; the close follows them.
            let pending = inbox.isEmpty ? waiter : nil
            if pending != nil { waiter = nil }
            lock.unlock()
            pending?.resume(throwing: LiveSocketError.closed(code: code))
        }
    }
}

// MARK: - Harness

@MainActor
struct LiveSessionHarness {
    let instance = StubInstance()
    let input = ManualCaptureInput()
    let root = InjectedAudio.directory()
    let live: ScriptedLiveServer
    let server: ServerNotesModel
    let controller: ServerRecordingController

    /// `controller`: a recorder that already exists, to give it a second account on another server.
    init(framesPerUtterance: Int = 20, host: String = "example.test", controller existing: ServerRecordingController? = nil) async throws {
        live = ScriptedLiveServer(instance: instance, framesPerUtterance: framesPerUtterance)
        let key = StubInstanceProtocol.register(instance)
        let suite = "live-session-\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        server = ServerNotesModel(makeAPI: { credentials in
            let configuration = URLSessionConfiguration.ephemeral
            configuration.protocolClasses = [StubInstanceProtocol.self]
            configuration.httpAdditionalHeaders = ["X-Stub-Instance": key]
            return MeetingNotesAPI(credentials: credentials, configuration: configuration)
        })
        let input = input, live = live
        controller = existing ?? ServerRecordingController(outboxRoot: root.appendingPathComponent("outbox"),
            makeBackend: { EngineCaptureBackend(input: input) },
            makeLiveClient: { _, _, source, configuration in
                // The session is replayed faster than real time, so the stream must not be held to real-time pacing.
                var fast = configuration
                fast.tick = .milliseconds(2)
                fast.catchUpFactor = 10_000
                fast.endGrace = .milliseconds(500)
                return LiveTranscriptClient(connector: live, source: source, configuration: fast)
            })
        await server.connect(instance: "https://\(host)", token: "unit-test-token")
        try #require(server.instance != nil)
        if existing == nil { await controller.attach(to: server) }
    }

    func note() async throws -> NoteDTO { try await server.note(id: instance.noteID) }

    /// Every file the recording wrote on this device, for checks on what was persisted.
    func persistedFiles() -> [URL] {
        (FileManager.default.enumerator(at: root, includingPropertiesForKeys: nil)?.allObjects as? [URL] ?? [])
            .filter { (try? $0.resourceValues(forKeys: [.isRegularFileKey]))?.isRegularFile == true }
    }
}

// MARK: - Tests

@MainActor
@Suite(.serialized)
struct LiveCaptureSessionTests {
    @Test func partsAreUploadedAsTheyAreCutAndNeverWaitForAnEarlierPartsTranscription() async throws {
        UserDefaults.standard.set(2, forKey: "capture.partSeconds")
        defer { UserDefaults.standard.removeObject(forKey: "capture.partSeconds") }
        let harness = try await LiveSessionHarness()
        defer { try? FileManager.default.removeItem(at: harness.root) }
        harness.live.refusal = .refused(status: 503) // no live model: the parts are on their own
        harness.instance.heldSequences = [0]         // the first part's transcription never finishes
        await harness.controller.start(note: try await harness.note())
        #expect(harness.controller.recorder.state == .recording)
        #expect(harness.controller.error == nil)

        var time = 0.0
        for _ in 0..<7 {
            time = harness.input.feed(format: InjectedAudio.builtIn, from: time, seconds: 1)
            try await Task.sleep(for: .milliseconds(120))
        }
        // Three parts were cut at 2, 4 and 6 seconds and all three reached the server while
        // the first is still being transcribed and the recording is still going.
        try await waitFor("three uploads") { harness.instance.allUploads.count == 3 }
        #expect(harness.controller.recorder.state == .recording)
        #expect(harness.instance.allUploads.map(\.sequence) == [0, 1, 2])
        #expect(harness.instance.allUploads.map(\.startMs) == [0, 2_000, 4_000])
        #expect(harness.controller.hasPendingAudio(noteID: harness.instance.noteID)) // part 0 is not ready
        // Silent fallback: one attempt at the live socket, no error, parts wording.
        #expect(harness.live.counts.connections == 0)
        #expect(harness.controller.error == nil)
        #expect(harness.controller.transcript?.status == .unavailable(.notConfigured))
        #expect(LiveStatusText.line(.unavailable(.notConfigured)) == nil)
        #expect(harness.controller.status == "Recording on device · Transcript updates after each audio chunk")

        harness.controller.stop()
        try await waitFor("final part and Stop") { harness.instance.allUploads.count == 4 && harness.instance.count("POST", suffix: "/recording") == 2 }
        // Every byte of the capture was uploaded exactly once, in order.
        let folder = harness.root.appendingPathComponent("outbox")
        let capture = try #require(FileManager.default.enumerator(at: folder, includingPropertiesForKeys: nil)?.allObjects
            .compactMap { $0 as? URL }.first { $0.lastPathComponent == "capture-0.wav" })
        var joined = Data()
        for upload in harness.instance.allUploads.sorted(by: { $0.sequence < $1.sequence }) { joined.append(upload.audio.dropFirst(44)) }
        #expect(joined == (try Data(contentsOf: capture).dropFirst(44)))
        #expect(joined.count == 7 * 32_000)
        harness.instance.release()
        try await waitFor("delivery complete") { !harness.controller.hasPendingAudio(noteID: harness.instance.noteID) && !harness.controller.isSyncing }
        // Nothing was uploaded twice.
        #expect(harness.instance.count("PUT", suffix: "") == 0 || Set(harness.instance.allUploads.map(\.chunkID)).count == 4)
        #expect(harness.instance.allRequests.filter { $0.method == "PUT" }.count == 4)
    }

    @Test func aLiveSessionShowsInterimThenFinalTextAndHandsOverItsLastWordsBeforeStop() async throws {
        let harness = try await LiveSessionHarness(framesPerUtterance: 10)
        defer { try? FileManager.default.removeItem(at: harness.root) }
        await harness.controller.start(note: try await harness.note())
        let transcript = try #require(harness.controller.transcript)
        var time = 0.0
        time = harness.input.feed(format: InjectedAudio.builtIn, from: time, seconds: 1)
        try await waitFor("interim text") { transcript.utterances.first?.text.contains(ScriptedLiveServer.interimMarker) == true }
        #expect(transcript.segments.isEmpty)
        #expect(harness.controller.status == "Recording on device · Live transcript")
        #expect(transcript.status == .live)
        let line = try #require(transcript.lines.first?.id)

        time = harness.input.feed(format: InjectedAudio.builtIn, from: time, seconds: 1.5)
        try await waitFor("first final") { transcript.segments.count == 1 }
        // The interim line became the stored line in place.
        #expect(transcript.lines.first?.id == line)
        #expect(!transcript.segments[0].text.contains(ScriptedLiveServer.interimMarker))

        // Pause and resume are marked on the stream at the capture's exact sample.
        harness.controller.recorder.pause()
        try await waitFor("pause mark") { harness.live.counts.marks == ["pause"] }
        #expect(harness.live.counts.samples == harness.controller.recorder.capturedFrames)
        try await waitFor("paused status") { transcript.status == .paused }
        harness.controller.recorder.resumeFromPause()
        time = harness.input.feed(format: InjectedAudio.builtIn, from: time, seconds: 1.2)
        try await waitFor("resume mark") { harness.live.counts.marks == ["pause", "resume"] }

        harness.controller.stop()
        // Stop reaches the server only after the stream said `end` and its last final was applied.
        try await waitFor("Stop") { harness.instance.count("POST", suffix: "/recording") == 2 }
        #expect(harness.live.counts.marks == ["pause", "resume", "end"])
        #expect(harness.live.counts.samples == harness.controller.recorder.capturedFrames)
        #expect(harness.controller.recorder.capturedFrames == Int64(3.7 * 16_000))
        // `end` carries the length of the capture as the closed file holds it.
        #expect(harness.live.endMarks == [Int64(3.7 * 16_000)])
        #expect(transcript.utterances.isEmpty)
        #expect(transcript.segments.count == harness.live.counts.finals)
        #expect(transcript.status == .ended)
        // The one durable part still went out: the live stream never replaces it.
        try await waitFor("part") { harness.instance.allUploads.count == 1 }
        #expect(harness.instance.allUploads[0].audio.count == 44 + Int(3.7 * 32_000))
    }

    @Test func anInterruptionEndsTheStreamAtTheCaptureLengthAndALaterStopSaysNothingMore() async throws {
        let harness = try await LiveSessionHarness(framesPerUtterance: 10)
        defer { try? FileManager.default.removeItem(at: harness.root) }
        await harness.controller.start(note: try await harness.note())
        let transcript = try #require(harness.controller.transcript)
        _ = harness.input.feed(format: InjectedAudio.builtIn, from: 0, seconds: 2.3)
        try await waitFor("live text") { !transcript.utterances.isEmpty || !transcript.segments.isEmpty }

        // A phone call: the capture is closed by the system, not by Stop.
        NotificationCenter.default.post(name: AVAudioSession.interruptionNotification, object: nil,
            userInfo: [AVAudioSessionInterruptionTypeKey: AVAudioSession.InterruptionType.began.rawValue])
        #expect(!harness.controller.isCapturing)
        let captured = harness.controller.recorder.capturedFrames
        #expect(captured == Int64(2.3 * 16_000))
        try await waitFor("end mark") { harness.live.endMarks.count == 1 }
        #expect(harness.live.endMarks == [captured])
        #expect(harness.live.counts.samples == captured)

        // The user then stops the recording. The stream has already ended; there is no second mark,
        // and none that says the capture is empty.
        harness.controller.stop()
        try await waitFor("Stop") { harness.instance.count("POST", suffix: "/recording") >= 2 }
        #expect(harness.live.endMarks == [captured])
        #expect(harness.live.counts.marks == ["end"])
    }

    /// Counts, not times. The session is ten minutes of audio and of server
    /// text, replayed quickly; what is counted depends on events, not on speed,
    /// except the clock and meter leaves, which tick on the wall clock.
    @Test func tenMinuteLiveSessionRendersOnlyTheTailAndPersistsNoInterimText() async throws {
        let harness = try await LiveSessionHarness()
        defer { try? FileManager.default.removeItem(at: harness.root) }
        let router = Router()
        let scene = try #require(UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }.first)
        let window = UIWindow(windowScene: scene)
        let noteID = harness.instance.noteID
        // The real recording screen, which starts the recording itself, and the transcript beside it.
        window.rootViewController = UIHostingController(rootView: VStack(spacing: 0) {
            RailMetricsReader { NavigationStack { ServerCaptureView(noteID: noteID) } }
            TranscriptHost(noteID: noteID).frame(height: 320)
        }
        // The screen is one account's, as it is wherever the app shows it.
        .environment(\.account, harness.server.accountID)
        .environment(harness.server).environment(harness.controller).environment(router))
        window.isHidden = false
        defer { window.isHidden = true }
        try await waitFor("recording", timeout: .seconds(10)) { harness.controller.recorder.state == .recording && harness.controller.transcript != nil }
        let transcript = try #require(harness.controller.transcript)
        var time = harness.input.feed(format: InjectedAudio.builtIn, from: 0, seconds: 2)
        try await waitFor("live") { transcript.status == .live && !transcript.utterances.isEmpty }
        try await Task.sleep(for: .milliseconds(400)) // first layout
        let journalWritesBefore = harness.controller.outboxJournalWrites
        let before = harness.live.counts
        PerfProbe.reset()
        let began = ContinuousClock.now

        let seconds = 598 // with the two above: ten minutes
        for second in 0..<seconds {
            time = harness.input.feed(format: InjectedAudio.builtIn, from: time, seconds: 1)
            // Let the stream, the socket's text and the run loop work through this second.
            let target = Int64(2 + second + 1) * 16_000
            try await waitFor("stream at \(second)") { harness.live.counts.samples >= target - 1_600 }
            try await Task.sleep(for: .milliseconds(12))
        }
        // Nineteen boundaries were crossed while recording. The twentieth lies on the very last
        // sample, which is in the file only once Stop has drained the converter.
        try await waitFor("parts uploaded") { harness.instance.allUploads.count == 19 }
        try await Task.sleep(for: .milliseconds(300))
        let wall = ContinuousClock.now - began
        let after = harness.live.counts
        let interims = after.interims - before.interims
        let finals = after.finals - before.finals

        let counts = [
            "ServerCaptureView.body", "RecordingPill.body", "LiveTranscriptList.body", "LiveTranscriptLine.body",
            "LiveTailFollower.body", "LiveStatusLine.body", "RecorderTimer.body", "Test.TranscriptHost.body",
            "Capture.Write", "Capture.Header", "Captures.Save",
        ].reduce(into: [String: Int]()) { $0[$1] = PerfProbe.count($1) }
        let journalWrites = harness.controller.outboxJournalWrites - journalWritesBefore
        // Saves of the typed note during the session (opening the screen saves once, before it).
        let patches = harness.instance.allRequests.filter { $0.method == "PATCH" && $0.at > began }.count
        print("""
        LIVE-SESSION-MEASUREMENT simulated=600s wall=\(wall) interims=\(interims) finals=\(finals) frames=\(after.frames - before.frames) \
        parts=\(harness.instance.allUploads.count) outboxJournalWrites=\(journalWrites) notePatches=\(patches) \
        noteReads=\(harness.instance.allRequests.filter { $0.method == "GET" && $0.path.hasSuffix("/notes/\(noteID)") && $0.at > began }.count) \
        chunkLists=\(harness.instance.allRequests.filter { $0.method == "GET" && $0.path.hasSuffix("/audio/chunks") && $0.at > began }.count) \
        \(counts.sorted { $0.key < $1.key }.map { "\($0.key)=\($0.value)" }.joined(separator: " "))
        """)

        // The session really was ten minutes of several-times-a-second interim text.
        #expect(interims >= 2_900 && finals >= 145)
        #expect(harness.controller.recorder.capturedFrames >= 600 * 16_000 - 400)

        // Rendering. Nothing around the transcript was rebuilt by text arriving:
        #expect(counts["RecordingPill.body"] == 0)
        #expect(counts["ServerCaptureView.body"]! <= 4)
        #expect(counts["Test.TranscriptHost.body"] == 0)
        // The list rebuilt only when a line began or became final, never per revision:
        #expect(counts["LiveTranscriptList.body"]! <= 2 * finals + 8)
        // Per interim revision, one line body ran (the tail), plus the new and settled lines at each final:
        #expect(counts["LiveTranscriptLine.body"]! >= interims / 2)
        #expect(counts["LiveTranscriptLine.body"]! <= interims + 24 * (finals + 4))
        #expect(counts["LiveStatusLine.body"]! <= 6)

        // Writes. The capture file grows by one small append per input buffer and one header
        // refresh a second; the delivery journal and the capture ledger move once per part.
        #expect(counts["Capture.Header"]! <= 600)
        #expect(counts["Capture.Write"]! <= 600 * 13)
        #expect(journalWrites <= 2 * 19 + 2)
        #expect(counts["Captures.Save"]! <= 2)
        // Nothing the user typed was saved because of the transcript, and no interim text was written anywhere.
        #expect(patches == 0)
        for file in harness.persistedFiles() where file.pathExtension == "json" {
            #expect(!String(decoding: try Data(contentsOf: file), as: UTF8.self).contains(ScriptedLiveServer.interimMarker))
        }

        harness.controller.stop()
        try await waitFor("Stop") { harness.instance.count("POST", suffix: "/recording") == 2 && harness.instance.allUploads.count == 20 }
        #expect(harness.controller.recorder.capturedFrames == 600 * 16_000)
        // The ten minutes arrived as twenty contiguous parts that join to the capture byte for byte.
        let uploads = harness.instance.allUploads.sorted { $0.sequence < $1.sequence }
        #expect(uploads.map(\.startMs) == (0..<20).map { $0 * 30_000 })
        let capture = try #require(harness.persistedFiles().first { $0.lastPathComponent == "capture-0.wav" })
        var joined = Data()
        for upload in uploads { joined.append(upload.audio.dropFirst(44)) }
        #expect(joined == (try Data(contentsOf: capture).dropFirst(44)))
    }
}

/// Stands for a screen that shows the transcript: it must not rebuild when text arrives.
private struct TranscriptHost: View {
    let noteID: String
    @Environment(ServerRecordingController.self) private var recording
    var body: some View {
        let _ = PerfProbe.hit("Test.TranscriptHost.body")
        if let live = recording.transcript(for: recording.ref(noteID)) { LiveTranscriptList(live: live) } else { Color.clear }
    }
}

@MainActor
func waitFor(_ what: String, timeout: Duration = .seconds(8), _ condition: @MainActor () -> Bool) async throws {
    let deadline = ContinuousClock.now + timeout
    while ContinuousClock.now < deadline {
        if condition() { return }
        try await Task.sleep(for: .milliseconds(4))
    }
    Issue.record("Timed out waiting for \(what)")
    throw CancellationError()
}

struct LiveStatusTextTests {
    @Test func theStatusLineSpeaksOnlyWhenThereIsSomethingToSay() {
        #expect(LiveStatusText.line(.off) == nil)
        #expect(LiveStatusText.line(.connecting) == nil)
        #expect(LiveStatusText.line(.live) == nil)
        #expect(LiveStatusText.line(.ended) == nil)
        #expect(LiveStatusText.line(.catchingUp(lagMs: 4_000)) == "Catching up…")
        #expect(LiveStatusText.line(.catchingUp(lagMs: 18_000)) == "Catching up · about 18 s behind")
        #expect(LiveStatusText.line(.providerUnavailable) == "Live transcript paused · audio is still being recorded")
        #expect(LiveStatusText.line(.reconnecting) == "Reconnecting…")
        #expect(LiveStatusText.line(.paused) == "Paused")
        #expect(LiveStatusText.line(.lowDataMode) == "Live transcript is off in Low Data Mode")
        // Not configured is not a failure and says nothing; a limit or an outage says one quiet line.
        #expect(LiveStatusText.line(.unavailable(.notConfigured)) == nil)
        #expect(LiveStatusText.line(.unavailable(.overLimit)) == "Live transcript unavailable · text follows from the saved audio")
        #expect(LiveStatusText.line(.unavailable(.providerUnavailable)) == "Live transcript unavailable · text follows from the saved audio")
        #expect(LiveStatusText.isStreaming(.live) && LiveStatusText.isStreaming(.reconnecting))
        #expect(!LiveStatusText.isStreaming(.unavailable(.notConfigured)) && !LiveStatusText.isStreaming(.off))
    }
}
