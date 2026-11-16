import AVFoundation
import Foundation
import SwiftUI
import Testing
import UIKit
import APIClient
import Contracts
@testable import Cereal

// MARK: - A stand-in for the instance

/// The HTTP side of an instance, in memory: the note and every request made.
/// Shared with the scripted live socket below, which stores final segments
/// here the way the server stores them in the note. Generating notes writes
/// them from the stored transcript, and refuses a note with none, as the
/// server does.
final class StubInstance: @unchecked Sendable {
    struct Request { let method: String; let path: String; let at: ContinuousClock.Instant }

    static let principal = "00000000-0000-4000-8000-000000000001"
    static let generatedNotes = "## Summary\n- Written from the live transcript"
    let noteID = UUID().uuidString.lowercased()
    private let lock = NSLock()
    private var requests: [Request] = []
    private var segments: [[String: Any]] = []
    private var revision = 0
    private var generated: Int?
    private var enhanced = ""
    private var recordingState = "idle"
    /// What the note holds besides its transcript, and whether it has been deleted.
    private var title = "Live session"
    private var body = ""
    private var deleted = false
    /// The `ifStamp` of every DELETE of the note.
    private var deleteStamps: [String?] = []
    var deletions: [String?] { lock.withLock { deleteStamps } }
    func set(title: String? = nil, body: String? = nil) {
        lock.withLock {
            if let title { self.title = title }
            if let body { self.body = body }
        }
    }
    /// The `expectedTranscriptRevision` of every request to generate notes.
    private var generateRequests: [Int] = []
    /// Sessions the server no longer accepts: every request with one is refused in the API's own words.
    var refusedTokens: Set<String> = []
    /// When set, what the note itself (and its recording commands) is answered with instead.
    var noteStatus: Int?
    /// When set, what a request to generate notes is answered with instead.
    var generateStatus: Int?
    func set(refused: Set<String>? = nil, note: Int?? = nil, generate: Int?? = nil) {
        lock.withLock {
            if let refused { refusedTokens = refused }
            if let note { noteStatus = note }
            if let generate { generateStatus = generate }
        }
    }

    var allRequests: [Request] { lock.withLock { requests } }
    var transcriptRevision: Int { lock.withLock { revision } }
    var generations: [Int] { lock.withLock { generateRequests } }
    func count(_ method: String, suffix: String) -> Int { lock.withLock { requests.filter { $0.method == method && $0.path.hasSuffix(suffix) }.count } }
    /// Requests to any audio route. The app must make none.
    var audioRequests: Int { lock.withLock { requests.filter { $0.path.contains("/audio") }.count } }

    /// What the server does with a final from the live path.
    func store(segment: [String: Any]) -> Int {
        lock.withLock { segments.append(segment); revision += 1; return revision }
    }

    private func noteJSON() -> [String: Any] {
        ["id": noteID, "title": title, "body": self.body, "enhanced": enhanced, "date": "2026-11-14", "duration": "0:00",
         "createdAt": "2026-11-14T10:00:00.000Z", "updatedAt": "2026-11-14T10:00:00.000Z", "recordingState": recordingState,
         "generationState": generated == nil ? "idle" : "ready", "transcriptRevision": revision,
         "generatedRevision": generated.map { $0 as Any } ?? NSNull(), "transcriptSegments": segments]
    }

    func respond(_ request: URLRequest, body: Data) -> (Int, Data) {
        lock.lock(); defer { lock.unlock() }
        let path = request.url?.path ?? ""
        let method = request.httpMethod ?? "GET"
        requests.append(Request(method: method, path: path, at: .now))
        func json(_ value: Any) -> (Int, Data) { (200, try! JSONSerialization.data(withJSONObject: value)) }
        func refusal(_ status: Int) -> (Int, Data) {
            let code = status == 401 ? "unauthorized" : status == 400 ? "invalid_request" : "not_found"
            return (status, Data(#"{"error":"\#(code)","message":"Stub"}"#.utf8))
        }
        let token = request.value(forHTTPHeaderField: "Authorization").map { String($0.dropFirst("Bearer ".count)) } ?? ""
        if refusedTokens.contains(token) { return refusal(401) }
        if path.hasSuffix("/api/auth/sign-out") { return json([String: Any]()) }
        if let noteStatus, path.hasSuffix("/notes/\(noteID)") || path.hasSuffix("/notes/\(noteID)/recording") { return refusal(noteStatus) }
        if path.hasSuffix("/api/v1/session") {
            return json(["user": ["id": Self.principal, "name": "Test Owner", "email": "owner@example.test", "role": "owner", "createdAt": "2026-11-12T00:00:00Z"], "expiresAt": NSNull()])
        }
        if path.hasSuffix("/api/v1/notes") { return json(deleted ? [] : [noteJSON()]) }
        if deleted, path.contains("/notes/\(noteID)") { return refusal(404) }
        if method == "DELETE", path.hasSuffix("/notes/\(noteID)") {
            let stamp = URLComponents(url: request.url!, resolvingAgainstBaseURL: false)?.queryItems?.first { $0.name == "ifStamp" }?.value
            deleteStamps.append(stamp)
            // As the server does: a note that is no longer as it was read is not deleted.
            let current = "2026-11-14T10:00:00.000Z|\(revision)|\(generated.map(String.init) ?? "")"
            guard stamp == nil || stamp == current else { return (409, Data(#"{"error":"conflict","message":"Stub"}"#.utf8)) }
            deleted = true
            return json(["deleted": true])
        }
        if path.hasSuffix("/notes/\(noteID)") { return json(noteJSON()) } // GET, and PATCH from the editor
        if path.hasSuffix("/notes/\(noteID)/recording") {
            let command = ((try? JSONSerialization.jsonObject(with: body)) as? [String: Any])?["command"] as? String
            recordingState = command == "stop" ? "stopped" : "recording"
            return json(noteJSON())
        }
        if method == "POST", path.hasSuffix("/notes/\(noteID)/generate") {
            let expected = ((try? JSONSerialization.jsonObject(with: body)) as? [String: Any])?["expectedTranscriptRevision"] as? Int ?? -1
            generateRequests.append(expected)
            if let generateStatus { return refusal(generateStatus) }
            // "This recording has no transcript to write notes from."
            if segments.isEmpty { return refusal(400) }
            if expected != revision { return (409, Data(#"{"error":"conflict","message":"Stub"}"#.utf8)) }
            generated = revision
            enhanced = Self.generatedNotes
            return json(noteJSON())
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
            push(#"{"type":"interim","channel":"mic","utteranceId":"\#(utterance)","rev":\#(revision),"text":"\#(words.joined(separator: " ")) \#(ScriptedLiveServer.interimMarker)","startMs":\#(utteranceStartMs),"endMs":\#(endMs),"speakerId":"mic:n:1","speakerLabel":"Me"}"#)
            var finals = 0
            if framesInUtterance >= server.framesPerUtterance { finalize(endMs: endMs); finals = 1 }
            server.note(frames: 1, samples: count, interims: 1, finals: finals)
        }

        private func finalize(endMs: Int) {
            let id = UUID().uuidString.lowercased()
            let text = words.joined(separator: " ").prefix(1).uppercased() + words.joined(separator: " ").dropFirst() + "."
            let segment: [String: Any] = ["id": id, "channel": "mic", "speakerId": "mic:n:1", "speakerLabel": "Me", "text": text,
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

/// Each harness's scripted live server, by the notes model of its account: a
/// recorder shared by two harnesses streams each account to its own server.
@MainActor enum ScriptedLiveServers {
    static var byModel: [ObjectIdentifier: ScriptedLiveServer] = [:]
}

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
        ScriptedLiveServers.byModel[ObjectIdentifier(server)] = live
        controller = existing ?? ServerRecordingController(outboxRoot: root.appendingPathComponent("outbox"),
            makeBackend: { EngineCaptureBackend(input: input) },
            makeLiveClient: { server, _, source, configuration in
                // The session is replayed faster than real time, so the stream must not be held to real-time pacing.
                var fast = configuration
                fast.tick = .milliseconds(2)
                fast.catchUpFactor = 10_000
                fast.endGrace = .milliseconds(500)
                return LiveTranscriptClient(connector: ScriptedLiveServers.byModel[ObjectIdentifier(server)] ?? live, source: source, configuration: fast)
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
    @Test func stopGoesStraightToNotesWrittenFromTheLiveTranscriptAndNoAudioIsUploaded() async throws {
        let harness = try await LiveSessionHarness(framesPerUtterance: 10)
        defer { try? FileManager.default.removeItem(at: harness.root) }
        await harness.controller.start(note: try await harness.note())
        let ref = try #require(harness.controller.ref(harness.instance.noteID))
        let transcript = try #require(harness.controller.transcript)
        _ = harness.input.feed(format: InjectedAudio.builtIn, from: 0, seconds: 2)
        try await waitFor("live finals") { !transcript.segments.isEmpty }
        let capture = try #require(harness.controller.recorder.currentFileURL)
        #expect(FileManager.default.fileExists(atPath: capture.path))

        harness.controller.stop()
        // No syncing or delivery state: the notes are being written from the moment of Stop.
        #expect(harness.controller.isGeneratingNotes(ref))
        try await waitFor("notes written") { !harness.controller.isGeneratingNotes(ref) }
        #expect(harness.controller.error == nil)
        // Start, Stop (after the stream's last words), then one request for notes, for the transcript Stop answered with.
        let posts = harness.instance.allRequests.filter { $0.method == "POST" }.map { $0.path.hasSuffix("/recording") ? "recording" : $0.path.hasSuffix("/generate") ? "generate" : $0.path }
        #expect(posts == ["recording", "recording", "generate"])
        #expect(harness.instance.generations == [harness.instance.transcriptRevision])
        #expect(harness.live.counts.marks == ["end"])
        // The note on screen has its notes.
        let shown = try #require(harness.server.state.notes.first { $0.id == ref.id })
        #expect(shown.enhanced == StubInstance.generatedNotes && shown.generatedRevision == shown.transcriptRevision)
        #expect(!ServerRecordingController.endedWithoutTranscript(shown))

        // No audio was sent anywhere but the live stream, and the capture went once the stream was done with it.
        #expect(harness.instance.audioRequests == 0)
        try await waitFor("capture removed") { !FileManager.default.fileExists(atPath: capture.path) }
        #expect(harness.persistedFiles().allSatisfy { $0.pathExtension != "wav" && !$0.lastPathComponent.hasSuffix(".capture.json") })
    }

    @Test func aRecordingThatCouldNotStartLeavesNoEmptyNoteBehind() async throws {
        let harness = try await LiveSessionHarness()
        defer { try? FileManager.default.removeItem(at: harness.root) }
        harness.instance.set(title: "")
        harness.input.failsToStart = true
        await harness.controller.start(note: try await harness.note())
        let ref = try #require(harness.controller.ref(harness.instance.noteID))
        #expect(harness.controller.error == "Recording could not start. Check that a microphone is available.")

        // Something typed on the recording screen keeps the note, and nothing is asked of the server.
        #expect(await harness.controller.discardIfNothingRecorded(ref, typedText: "Agenda") == false)
        #expect(await harness.controller.discardIfNothingRecorded(ref, typedTitle: "Weekly sync") == false)
        #expect(harness.instance.deletions.isEmpty)

        // Nothing recorded and nothing typed: the note goes, and only if it is still as it was read.
        #expect(await harness.controller.discardIfNothingRecorded(ref, typedTitle: " ", typedText: "\n"))
        #expect(harness.instance.deletions == ["2026-11-14T10:00:00.000Z|0|"])
        #expect(harness.server.state.notes.isEmpty)
        #expect(harness.controller.activeNoteID == nil && harness.controller.error == nil)
        // The server was told to stop before the note went.
        #expect(harness.instance.count("POST", suffix: "/recording") == 2)
    }

    @Test func aNoteThatHoldsAnythingIsNeverDiscarded() async throws {
        // Text or a title on the server's copy (typed on another device, say) keeps the note.
        for (title, body) in [("", "Typed elsewhere"), ("Weekly sync", "")] {
            let harness = try await LiveSessionHarness()
            defer { try? FileManager.default.removeItem(at: harness.root) }
            harness.instance.set(title: "")
            harness.input.failsToStart = true
            await harness.controller.start(note: try await harness.note())
            harness.instance.set(title: title, body: body)
            #expect(await harness.controller.discardIfNothingRecorded(harness.controller.ref(harness.instance.noteID)) == false)
            #expect(harness.instance.deletions.isEmpty)
        }

        // A recording that did start, with audio and a transcript, is kept: while it runs and after Stop.
        let harness = try await LiveSessionHarness(framesPerUtterance: 10)
        defer { try? FileManager.default.removeItem(at: harness.root) }
        harness.instance.set(title: "")
        await harness.controller.start(note: try await harness.note())
        let ref = try #require(harness.controller.ref(harness.instance.noteID))
        _ = harness.input.feed(format: InjectedAudio.builtIn, from: 0, seconds: 2)
        try await waitFor("live finals") { harness.controller.transcript?.segments.isEmpty == false }
        #expect(await harness.controller.discardIfNothingRecorded(ref) == false)
        harness.controller.stop()
        try await waitFor("notes written") { !harness.controller.isGeneratingNotes(ref) }
        #expect(await harness.controller.discardIfNothingRecorded(ref) == false)
        #expect(harness.instance.deletions.isEmpty)

        // The rule itself: any one thing on the note is enough to keep it.
        func note(title: String = "", body: String = "", enhanced: String = "", duration: String = "0:00", revision: Int = 0,
                  generated: Int? = nil, segments: [TranscriptSegmentDTO] = [], state: RecordingState = .stopped) -> NoteDTO {
            NoteDTO(body: body, createdAt: "2026-11-14T10:00:00.000Z", date: "2026-11-14", duration: duration, enhanced: enhanced,
                    generatedRevision: generated, generationState: .idle, id: "n", recordingState: state, title: title,
                    transcriptRevision: revision, transcriptSegments: segments, updatedAt: "2026-11-14T10:00:00.000Z")
        }
        #expect(ServerRecordingController.holdsNothing(note()))
        #expect(ServerRecordingController.holdsNothing(note(title: " New note ", state: .idle)))
        for kept in [note(title: "Weekly sync"), note(body: "x"), note(enhanced: "## Summary"), note(duration: "0:03"),
                     note(revision: 1), note(generated: 0), note(state: .recording)] {
            #expect(!ServerRecordingController.holdsNothing(kept))
        }
        #expect(ServerRecordingController.stamp(note(revision: 3, generated: 2)) == "2026-11-14T10:00:00.000Z|3|2")
    }

    @Test func aCancellationIsNeverShownAsAnError() async throws {
        #expect(ErrorText.shown(CancellationError()) == nil)
        #expect(ErrorText.shown(URLError(.cancelled)) == nil)
        #expect(ErrorText.shown(APIClientError.transport(.cancelled)) == nil)
        #expect(ErrorText.shown(NSError(domain: NSURLErrorDomain, code: NSURLErrorCancelled)) == nil)
        #expect(ErrorText.shown(APIClientError.notFound) == APIClientError.notFound.localizedDescription)
        #expect(ErrorText.shown(URLError(.timedOut)) != nil)

        // Starting a recording whose read of the note is cancelled says nothing, and records nothing.
        let harness = try await LiveSessionHarness()
        defer { try? FileManager.default.removeItem(at: harness.root) }
        let note = try await harness.note()
        let start = Task { await harness.controller.start(note: note) }
        start.cancel()
        await start.value
        #expect(harness.controller.error == nil)
        #expect(!harness.controller.isCapturing)
    }

    @Test func stopWithNothingTranscribedAsksForNoNotesAndSaysSo() async throws {
        let harness = try await LiveSessionHarness()
        defer { try? FileManager.default.removeItem(at: harness.root) }
        // The server takes no stream for this note, so nothing is transcribed while audio is captured.
        harness.live.refusal = .refused(status: 409)
        await harness.controller.start(note: try await harness.note())
        let ref = try #require(harness.controller.ref(harness.instance.noteID))
        _ = harness.input.feed(format: InjectedAudio.builtIn, from: 0, seconds: 2)
        try await waitFor("stream refused") { harness.controller.transcript?.status == .unavailable(.notRecording) }
        #expect(LiveStatusText.line(.unavailable(.notRecording)) != nil)

        harness.controller.stop()
        #expect(harness.controller.isGeneratingNotes(ref))
        try await waitFor("settled") { !harness.controller.isGeneratingNotes(ref) }
        #expect(harness.instance.count("POST", suffix: "/recording") == 2)
        // Generate was never asked for, nothing hangs, and the note says why it has no notes.
        #expect(harness.instance.generations.isEmpty)
        #expect(harness.controller.error == nil)
        let shown = try #require(harness.server.state.notes.first { $0.id == ref.id })
        #expect(ServerRecordingController.endedWithoutTranscript(shown))
        #expect(ServerRecordingController.noTranscript == "No transcript was captured for this recording.")
        #expect(harness.instance.audioRequests == 0)
    }

    @Test func notesThatCannotBeWrittenEndTheBusyStateAndSayWhy() async throws {
        let harness = try await LiveSessionHarness(framesPerUtterance: 10)
        defer { try? FileManager.default.removeItem(at: harness.root) }
        harness.instance.set(generate: 503)
        await harness.controller.start(note: try await harness.note())
        let ref = try #require(harness.controller.ref(harness.instance.noteID))
        _ = harness.input.feed(format: InjectedAudio.builtIn, from: 0, seconds: 3)
        try await waitFor("live finals") { harness.controller.transcript?.segments.isEmpty == false }
        harness.controller.stop()
        try await waitFor("settled") { !harness.controller.isGeneratingNotes(ref) }
        #expect(harness.instance.generations.count == 1)
        #expect(harness.controller.error?.hasPrefix("Notes could not be generated.") == true)
        // The note can be generated by hand: it is stopped, has a transcript and no notes for it.
        let shown = try #require(harness.server.state.notes.first { $0.id == ref.id })
        #expect(shown.recordingState == .stopped && shown.transcriptRevision > 0 && shown.generatedRevision == nil)
    }

    @Test func aLiveStreamTheServerTurnsAwayKeepsTryingAndLosesNothingWhenItIsTaken() async throws {
        let harness = try await LiveSessionHarness(framesPerUtterance: 10)
        defer { try? FileManager.default.removeItem(at: harness.root) }
        // The provider is down when the meeting starts.
        harness.live.refusal = .refused(status: 503)
        await harness.controller.start(note: try await harness.note())
        let transcript = try #require(harness.controller.transcript)
        var time = harness.input.feed(format: InjectedAudio.builtIn, from: 0, seconds: 1)
        try await waitFor("turned away") { transcript.status == .providerUnavailable }
        #expect(LiveStatusText.line(.providerUnavailable) == "Live transcript paused · audio is saved on this iPhone")
        time = harness.input.feed(format: InjectedAudio.builtIn, from: time, seconds: 1)
        // It comes back: the stream connects and sends everything captured meanwhile, from the first sample.
        harness.live.refusal = nil
        try await waitFor("caught up", timeout: .seconds(10)) { harness.live.counts.samples == harness.controller.recorder.capturedFrames && transcript.status == .live }
        #expect(harness.live.counts.connections == 1)
        // Everything in the file so far; the converter's last few milliseconds reach it at Stop.
        #expect(harness.controller.recorder.capturedFrames >= 2 * 16_000 - 400)
        _ = time
        harness.controller.stop()
        #expect(harness.controller.recorder.capturedFrames == 2 * 16_000)
        try await waitFor("notes") { harness.instance.generations.count == 1 }
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
        // The notes are then written from what the stream stored.
        try await waitFor("notes") { harness.instance.generations == [harness.instance.transcriptRevision] }
        #expect(harness.instance.audioRequests == 0)
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
        // Saves of the typed note during the session (opening the screen saves once, before it).
        let patches = harness.instance.allRequests.filter { $0.method == "PATCH" && $0.at > began }.count
        print("""
        LIVE-SESSION-MEASUREMENT simulated=600s wall=\(wall) interims=\(interims) finals=\(finals) frames=\(after.frames - before.frames) \
        notePatches=\(patches) \
        noteReads=\(harness.instance.allRequests.filter { $0.method == "GET" && $0.path.hasSuffix("/notes/\(noteID)") && $0.at > began }.count) \
        audioRequests=\(harness.instance.audioRequests) \
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
        // refresh a second; the capture ledger does not move while recording.
        #expect(counts["Capture.Header"]! <= 600)
        #expect(counts["Capture.Write"]! <= 600 * 13)
        #expect(counts["Captures.Save"]! <= 2)
        #expect(harness.instance.audioRequests == 0)
        // Nothing the user typed was saved because of the transcript, and no interim text was written anywhere.
        #expect(patches == 0)
        for file in harness.persistedFiles() where file.pathExtension == "json" {
            #expect(!String(decoding: try Data(contentsOf: file), as: UTF8.self).contains(ScriptedLiveServer.interimMarker))
        }

        harness.controller.stop()
        try await waitFor("Stop and notes") { harness.instance.count("POST", suffix: "/recording") == 2 && harness.instance.generations.count == 1 }
        #expect(harness.controller.recorder.capturedFrames == 600 * 16_000)
        // All ten minutes reached the server through the stream alone, every sample once.
        #expect(harness.live.counts.samples == 600 * 16_000)
        #expect(harness.instance.audioRequests == 0)
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
        #expect(LiveStatusText.line(.providerUnavailable) == "Live transcript paused · audio is saved on this iPhone")
        #expect(LiveStatusText.line(.reconnecting) == "Reconnecting · audio is saved on this iPhone")
        #expect(LiveStatusText.line(.paused) == "Paused")
        // The stream is the only transcript: when it has ended for good, that is said, never left silent.
        #expect(LiveStatusText.line(.unavailable(.unauthorized)) == "Live transcript stopped · sign in again to transcribe")
        for reason in [LiveUnavailableReason.notFound, .notRecording, .replaced, .protocolError] {
            #expect(LiveStatusText.line(.unavailable(reason)) == "Live transcript stopped · speech is no longer transcribed")
        }
        #expect(LiveStatusText.isStreaming(.live) && LiveStatusText.isStreaming(.reconnecting) && LiveStatusText.isStreaming(.providerUnavailable))
        #expect(!LiveStatusText.isStreaming(.unavailable(.notRecording)) && !LiveStatusText.isStreaming(.off))
    }
}
