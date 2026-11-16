import Foundation
import Testing
import Contracts
@testable import APIClient

private let sessionJSON = #"{"user":{"id":"00000000-0000-4000-8000-000000000001","name":"Test Owner","email":"owner@example.test","role":"owner","createdAt":"2026-11-12T00:00:00Z"},"expiresAt":null}"#
private let noteID = "3a49a4af-c660-42e9-a36a-a806fbb41e0d"
private let noteJSON = #"{"id":"3a49a4af-c660-42e9-a36a-a806fbb41e0d","title":"Server note","body":"Editable notes","enhanced":"Generated notes","date":"2026-11-12","duration":"1:12","createdAt":"2026-11-12T10:00:00.000Z","updatedAt":"2026-11-12T10:01:00.123Z","recordingState":"stopped","generationState":"ready","transcriptRevision":2,"generatedRevision":2,"transcriptSegments":[{"id":"99ad1171-3771-4ae6-81a1-44f617b663db","channel":"mic","speakerId":"self","speakerLabel":null,"text":"Hello café 🦊","isFinal":false,"startMs":0,"endMs":null}]}"#

/// The list row for `noteJSON`: the same note without its text or transcript.
private let summaryJSON = #"{"id":"3a49a4af-c660-42e9-a36a-a806fbb41e0d","title":"Server note","date":"2026-11-12","duration":"1:12","createdAt":"2026-11-12T10:00:00.000Z","updatedAt":"2026-11-12T10:01:00.123Z","recordingState":"stopped","generationState":"ready","transcriptRevision":2,"generatedRevision":2,"hasBody":true,"hasSummary":true,"hasTranscript":true,"speakerCount":1,"excerpt":"Generated notes.","meeting":null,"attendees":{"people":[],"others":0,"total":0},"organizationId":null}"#
private func summariesPage(_ rows: [String], next: String? = nil, total: Int? = nil) -> String {
    "{\"notes\":[\(rows.joined(separator: ","))],\"nextCursor\":\(next.map { "\"\($0)\"" } ?? "null"),\"total\":\(total ?? rows.count)}"
}
private func isSummaries(_ request: URLRequest) -> Bool { request.url?.path.hasSuffix("/notes/summaries") == true }

private struct Response: Sendable {
    var status = 200
    var body: String
    var failure: URLError.Code?
    var contentType = "application/json"
    var headers: [String: String] = [:]
}

/// The paths a test's requests went to.
private final class Paths: @unchecked Sendable {
    private let lock = NSLock()
    private var paths: [String] = []
    func add(_ path: String) { lock.withLock { paths.append(path) } }
    var all: [String] { lock.withLock { paths } }
}

private final class Fixtures: @unchecked Sendable {
    static let shared = Fixtures()
    private let lock = NSLock()
    private var handlers: [String: @Sendable (URLRequest) -> Response] = [:]
    func register(_ handler: @escaping @Sendable (URLRequest) -> Response) -> String {
        lock.lock(); defer { lock.unlock() }
        let id = UUID().uuidString
        handlers[id] = handler
        return id
    }
    func response(_ request: URLRequest) -> Response {
        lock.lock(); let handler = handlers[request.value(forHTTPHeaderField: "X-Test-Case") ?? ""]; lock.unlock()
        return handler?(request) ?? Response(status: 500, body: "")
    }
}

private final class StubProtocol: URLProtocol, @unchecked Sendable {
    private let stateLock = NSRecursiveLock()
    private var stopped = false
    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func startLoading() {
        let response = Fixtures.shared.response(request)
        stateLock.lock(); defer { stateLock.unlock() }
        guard !stopped else { return }
        if let failure = response.failure {
            client?.urlProtocol(self, didFailWithError: URLError(failure)); return
        }
        let http = HTTPURLResponse(url: request.url!, statusCode: response.status, httpVersion: "HTTP/1.1", headerFields: response.headers.merging(["Content-Type": response.contentType]) { _, contentType in contentType })!
        client?.urlProtocol(self, didReceive: http, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: Data(response.body.utf8))
        client?.urlProtocolDidFinishLoading(self)
    }
    override func stopLoading() { stateLock.lock(); defer { stateLock.unlock() }; stopped = true }
}

private func client(_ handler: @escaping @Sendable (URLRequest) -> Response) throws -> MeetingNotesAPI {
    let configuration = configuration(handler)
    return try MeetingNotesAPI(credentials: InstanceCredentials(instance: InstanceURL("https://example.test/base"), bearerToken: "unit-test-token"), configuration: configuration)
}

private func configuration(automaticSession: Bool = true, _ handler: @escaping @Sendable (URLRequest) -> Response) -> URLSessionConfiguration {
    let configuration = URLSessionConfiguration.ephemeral
    configuration.protocolClasses = [StubProtocol.self]
    configuration.httpAdditionalHeaders = ["X-Test-Case": Fixtures.shared.register { request in
        if automaticSession && request.url?.path.hasSuffix("/api/v1/session") == true { return Response(body: sessionJSON) }
        if automaticSession && request.url?.path.hasSuffix("/api/v1/integrations/google") == true { return Response(body: #"{"configured":false,"redirectUri":null,"connections":[]}"#) }
        return handler(request)
    }]
    return configuration
}

private func requestBody(_ request: URLRequest) -> Data {
    if let data = request.httpBody { return data }
    guard let stream = request.httpBodyStream else { return Data() }
    stream.open()
    defer { stream.close() }
    var result = Data()
    var buffer = [UInt8](repeating: 0, count: 1024)
    while true {
        let count = stream.read(&buffer, maxLength: buffer.count)
        if count <= 0 { break }
        result.append(contentsOf: buffer.prefix(count))
    }
    return result
}

@Suite struct APIClientTests {
    @Test func emailSignInUsesSignedHeaderAndNeverPersistsPasswordOrUnsignedToken() async throws {
        let config = configuration(automaticSession: false) { request in
            #expect(request.url?.path == "/base/api/auth/sign-in/email")
            #expect(request.httpMethod == "POST")
            #expect(request.value(forHTTPHeaderField: "Authorization") == nil)
            let input = (try? JSONSerialization.jsonObject(with: requestBody(request))) as? [String: Any]
            #expect(input?["email"] as? String == "user@example.test")
            #expect(input?["password"] as? String == "synthetic-test-password")
            #expect(input?["rememberMe"] as? Bool == true)
            return Response(body: #"{"token":"unsigned-body-token","redirect":false}"#, headers: ["set-auth-token":"signed-native-session"])
        }
        let credentials = try await MeetingNotesAPI.signIn(instance: InstanceURL("https://example.test/base"), email: "user@example.test", password: "synthetic-test-password", configuration: config)
        #expect(credentials.bearerToken == "signed-native-session")
        #expect(!credentials.description.contains("signed-native-session"))
    }

    @Test func calendarContextRemovalEncodesExplicitNullAndUsesCanonicalParticipantRoute() async throws {
        let api = try client { request in
            #expect(request.value(forHTTPHeaderField: "Authorization") == "Bearer unit-test-token")
            #expect(request.url?.path.hasPrefix("/base/api/v1/notes/\(noteID)/context") == true)
            let payload = (try? JSONSerialization.jsonObject(with: requestBody(request))) as? [String: Any]
            if request.httpMethod == "PUT" { #expect(payload?["eventId"] is NSNull) }
            if request.httpMethod == "POST" {
                #expect(request.url?.path.hasSuffix("/context/participants") == true)
                #expect(payload?["displayName"] as? String == "Explicit Test Name")
            }
            return Response(body: "{\"noteId\":\"\(noteID)\",\"meeting\":null}")
        }
        #expect(try await api.noteContext(id: noteID).meeting == nil)
        #expect(try await api.updateNoteContext(id: noteID, eventID: nil).meeting == nil)
        let input = ManualParticipantInput(displayName: "Explicit Test Name", email: nil, familyName: nil, givenName: nil, id: noteID)
        #expect(try await api.upsertNoteParticipant(id: noteID, input: input).meeting == nil)
    }

    @Test func calendarsUseAuthenticatedCanonicalRoutesAndEncodedTimeWindow() async throws {
        let api = try client { request in
            #expect(request.value(forHTTPHeaderField: "Authorization") == "Bearer unit-test-token")
            if request.url?.path == "/base/api/v1/calendars" {
                return Response(body: ##"[{"id":"3a49a4af-c660-42e9-a36a-a806fbb41e0d","name":"Work","provider":"ics","color":"#87e5e8","enabled":true,"sourceHost":"calendar.example","lastSyncedAt":null,"lastSyncError":null,"eventCount":0,"createdAt":"2026-11-12T10:00:00Z"}]"##)
            }
            #expect(request.url?.path == "/base/api/v1/calendar/events")
            let query = URLComponents(url: request.url!, resolvingAgainstBaseURL: false)?.queryItems
            #expect(query?.first(where: { $0.name == "from" })?.value == "2026-03-08T08:00:00Z")
            #expect(query?.first(where: { $0.name == "to" })?.value == "2026-03-09T07:00:00Z")
            return Response(body: "[]")
        }
        let calendars = try await api.calendars()
        #expect(calendars.first?.name == "Work")
        let start = try #require(CalendarSchedule.timestamp("2026-03-08T08:00:00Z"))
        let end = try #require(CalendarSchedule.timestamp("2026-03-09T07:00:00Z"))
        #expect(try await api.calendarEvents(from: start, to: end).isEmpty)
        await #expect(throws: APIClientError.invalidResponse) { try await api.calendarEvents(from: end, to: start) }
    }

    @Test func eventNoteUsesIdempotentServerCommandWithoutStartingRecording() async throws {
        let api = try client { request in
            #expect(request.url?.path == "/base/api/v1/calendar/events/\(noteID)/note")
            #expect(request.httpMethod == "POST")
            #expect(requestBody(request).isEmpty)
            return Response(body: noteJSON)
        }
        let note = try await api.noteForCalendarEvent(id: noteID)
        #expect(note.id == noteID)
        await #expect(throws: APIClientError.invalidIdentifier) { try await api.noteForCalendarEvent(id: "../notes") }
    }

    @Test func listDecodesServerOwnedNotesAndExplicitSourceChannel() async throws {
        let api = try client { request in
            #expect(request.url?.path == "/base/api/v1/notes")
            #expect(request.httpMethod == "GET")
            #expect(request.value(forHTTPHeaderField: "Authorization") == "Bearer unit-test-token")
            #expect(request.url?.query == nil)
            return Response(body: "[\(noteJSON)]")
        }
        let notes = try await api.listNotes()
        #expect(notes.count == 1)
        #expect(notes[0].id == noteID)
        #expect(notes[0].transcriptSegments[0].text == "Hello café 🦊")
        #expect(notes[0].transcriptSegments[0].speakerLabel == nil)
        #expect(!notes[0].transcriptSegments[0].isFinal)
        #expect(notes[0].transcriptSegments[0].endMs == nil)
    }

    @Test func detailUsesUUIDRouteAndDecodesFractionalTimestampAsWireString() async throws {
        let api = try client { request in
            #expect(request.url?.path == "/base/api/v1/notes/\(noteID)")
            return Response(body: noteJSON)
        }
        let note = try await api.note(id: noteID)
        #expect(note.updatedAt == "2026-11-12T10:01:00.123Z")
        await #expect(throws: APIClientError.invalidIdentifier) { try await api.note(id: "../another-owner") }
    }

    @Test func writesUseTypedJSONAndDoNotInventCaptureOrGenerationActions() async throws {
        let create = try client { request in
            #expect(request.httpMethod == "POST")
            #expect(request.url?.path == "/base/api/v1/notes")
            #expect(request.value(forHTTPHeaderField: "Content-Type") == "application/json")
            let payload = try? JSONSerialization.jsonObject(with: requestBody(request)) as? [String: String]
            #expect(payload?["title"] == "Created")
            #expect(payload?["body"] == "Body")
            #expect(payload?["transcriptSegments"] == nil)
            return Response(body: noteJSON)
        }
        _ = try await create.createNote(CreateNoteInput(body: "Body", date: nil, duration: nil, enhanced: nil, title: "Created", transcriptSegments: nil))
        let update = try client { request in
            #expect(request.httpMethod == "PATCH")
            #expect(request.url?.path == "/base/api/v1/notes/\(noteID)")
            let payload = try? JSONSerialization.jsonObject(with: requestBody(request)) as? [String: String]
            #expect(payload?.count == 1)
            #expect(payload?["title"] == "Renamed")
            return Response(body: noteJSON)
        }
        _ = try await update.updateNote(id: noteID, UpdateNoteInput(body: nil, date: nil, duration: nil, enhanced: nil, title: "Renamed", transcriptSegments: nil))
    }

    @Test func generatedEnumsKeepTheirSnakeCaseWireValues() throws {
        let json = noteJSON.replacingOccurrences(of: #""generationState":"ready""#, with: #""generationState":"analyzing_transcript""#)
        let note = try JSONDecoder().decode(NoteDTO.self, from: Data(json.utf8))
        #expect(note.generationState == .analyzingTranscript)
    }

    @Test func requiredNullableTranscriptFieldsEncodeExplicitNull() throws {
        let segment = TranscriptSegmentDTO(channel: .mic, endMs: nil, id: noteID, isFinal: false, speakerId: "self", speakerLabel: nil, startMs: 0, text: "Partial")
        let object = try #require(JSONSerialization.jsonObject(with: JSONEncoder().encode(segment)) as? [String: Any])
        #expect(object["endMs"] is NSNull)
        #expect(object["speakerLabel"] is NSNull)
    }

    @Test func sharesUseOwnerRoutesAndValidatedSameInstanceCapabilityURLs() async throws {
        let token = String(repeating: "a", count: 43)
        let metadata = #"{"id":"3a49a4af-c660-42e9-a36a-a806fbb41e0d","noteId":"3a49a4af-c660-42e9-a36a-a806fbb41e0d","createdAt":"2026-11-12T10:00:00Z","expiresAt":null,"revokedAt":null}"#
        let api = try client { request in
            #expect(request.value(forHTTPHeaderField: "Authorization") == "Bearer unit-test-token")
            if request.httpMethod == "POST" {
                #expect(request.url?.path == "/base/api/v1/notes/\(noteID)/shares")
                let payload = try? JSONSerialization.jsonObject(with: requestBody(request)) as? [String: Any]
                #expect(payload?["includeBody"] as? Bool == false)
                #expect(payload?["includeEnhanced"] as? Bool == true)
                return Response(body: "{\"share\":\(metadata),\"token\":\"\(token)\",\"path\":\"/s/\(token)\"}")
            }
            if request.httpMethod == "DELETE" {
                #expect(request.url?.path == "/base/api/v1/shares/\(noteID)")
                return Response(body: metadata)
            }
            #expect(request.url?.path == "/base/api/v1/notes/\(noteID)/shares")
            return Response(body: "[\(metadata)]")
        }
        let created = try await api.createShare(noteID: noteID, CreateShareInput(expiresAt: nil, includeBody: false, includeEnhanced: true))
        #expect(try api.shareURL(created).absoluteString == "https://example.test/base/s/\(token)")
        #expect(try await api.shares(noteID: noteID).count == 1)
        _ = try await api.revokeShare(id: noteID)
        let injected = CreatedShareDTO(path: "https://other.example/s/\(token)", share: created.share, token: token)
        #expect(throws: APIClientError.invalidJSON) { try api.shareURL(injected) }
        await #expect(throws: APIClientError.invalidIdentifier) { try await api.shares(noteID: "../../other") }
    }

    @Test(arguments: [401, 403, 404, 503, 302])
    func mapsHTTPFailureWithoutDecodingServerMessage(status: Int) async throws {
        let api = try client { _ in Response(status: status, body: #"{"message":"private backend detail"}"#) }
        let expected: APIClientError = switch status {
        case 401, 403: .unauthorized
        case 404: .notFound
        case 503: .unavailable
        default: .httpStatus(status)
        }
        await #expect(throws: expected) { try await api.listNotes() }
    }

    @Test func malformedJSONAndOfflineAreDistinct() async throws {
        let malformed = try client { _ in Response(body: "not json") }
        await #expect(throws: APIClientError.invalidJSON) { try await malformed.listNotes() }
        let offline = try client { _ in Response(body: "", failure: .notConnectedToInternet) }
        await #expect(throws: APIClientError.transport(.notConnectedToInternet)) { try await offline.listNotes() }
    }

    @Test(arguments: ["http://localhost:3000", "http://127.0.0.1:3000", "http://[::1]:3000", "https://example.com/meetings/"])
    func acceptsHTTPSOrExplicitLoopback(value: String) throws {
        _ = try InstanceURL(value)
    }

    @Test(arguments: ["http://example.com", "ftp://example.com", "https://user:password@example.com", "https://example.com?token=secret", "https://example.com/#secret", "https://example.com/../other", "https://example.com/%2e%2e/other", "https://example.com:70000", "not a URL"])
    func rejectsInsecureOrSecretBearingInstanceURLs(value: String) {
        #expect(throws: (any Error).self) { try InstanceURL(value) }
    }

    @Test func credentialsAreRedactedAndCannotInjectHeaders() throws {
        let instance = try InstanceURL("https://example.com")
        let credentials = try InstanceCredentials(instance: instance, bearerToken: "sample-secret")
        #expect(!String(describing: credentials).contains("sample-secret"))
        #expect(!String(reflecting: credentials).contains("sample-secret"))
        #expect(throws: APIClientError.invalidToken) {
            try InstanceCredentials(instance: instance, bearerToken: "token\r\nInjected: yes")
        }
    }

    @Test func pipelineUsesGeneratedContractsAndAuthenticatedRoutesAndUploadsNoAudio() async throws {
        let paths = Paths()
        let api = try client { request in
            paths.add(request.url?.path ?? "")
            #expect(request.value(forHTTPHeaderField: "Authorization") == "Bearer unit-test-token")
            let path = request.url?.path ?? ""
            let body = (try? JSONSerialization.jsonObject(with: requestBody(request))) as? [String: Any]
            if path.hasSuffix("/recording") {
                #expect(request.httpMethod == "POST")
                #expect(body?["command"] as? String == "resume")
                return Response(body: noteJSON)
            }
            if path.hasSuffix("/generate") {
                #expect(body?["expectedTranscriptRevision"] as? Int == 2)
                #expect(request.timeoutInterval >= 120)
                return Response(body: noteJSON)
            }
            if path.hasSuffix("/chat") {
                #expect(body?["message"] as? String == "What changed?")
                #expect((body?["history"] as? [[String: String]])?.first?["role"] == "user")
                return Response(body: #"{"answer":"The owner approved the plan."}"#)
            }
            Issue.record("Unexpected request to \(path)")
            return Response(status: 404, body: #"{"error":"not_found","message":"No"}"#)
        }
        _ = try await api.recording(noteID: noteID, command: .resume)
        _ = try await api.generate(noteID: noteID, expectedTranscriptRevision: 2)
        #expect(try await api.chat(noteID: noteID, message: "What changed?", history: [ChatMessageDTO(content: "Plan?", role: .user)]).answer == "The owner approved the plan.")
        // Recording to notes is the live stream and these calls; nothing touches the audio routes.
        #expect(!paths.all.contains { $0.contains("/audio") })
    }
}

@MainActor @Suite struct ServerNotesModelTests {
    @Test func calendarFailureKeepsLastSnapshotAndDisconnectRejectsLateCompletion() async throws {
        let domain = "MeetingNotes-calendar-state-\(UUID().uuidString)"
        let defaults = try #require(UserDefaults(suiteName: domain))
        defer { defaults.removePersistentDomain(forName: domain) }
        let fixture = CalendarResponseState()
        let model = ServerNotesModel(makeAPI: { credentials in
            MeetingNotesAPI(credentials: credentials, configuration: configuration { request in
                if isSummaries(request) { return Response(body: summariesPage([])) }
                if request.url?.path.hasSuffix("/notes") == true { return Response(body: "[]") }
                let mode = fixture.value
                if mode == 1 { return Response(status: 503, body: "{}") }
                if mode == 2 { Thread.sleep(forTimeInterval: 0.15) }
                if request.url?.path.hasSuffix("/calendars") == true {
                    return Response(body: ##"[{"id":"work","name":"Work","provider":"ics","color":"#87e5e8","enabled":true,"sourceHost":"calendar.example","lastSyncedAt":null,"lastSyncError":null,"eventCount":0,"createdAt":"2026-11-12T10:00:00Z"}]"##)
                }
                return Response(body: "[]")
            })
        })
        await model.connect(instance: "https://example.test", token: "unit-test-token")
        let from = try #require(CalendarSchedule.timestamp("2026-11-26T00:00:00Z"))
        let to = from.addingTimeInterval(24*3600)
        await model.refreshCalendars(from: from, to: to)
        #expect(model.calendarState.snapshot.calendars.first?.name == "Work")
        fixture.value = 1
        await model.refreshCalendars(from: from, to: to)
        guard case .failed(.unavailable, let previous) = model.calendarState else { Issue.record("Calendar fetch failures must not masquerade as an empty day"); return }
        #expect(previous.calendars.first?.name == "Work")
        fixture.value = 2
        let refresh = Task { await model.refreshCalendars(from: from, to: to) }
        for _ in 0..<50 {
            if case .loading = model.calendarState { break }
            await Task.yield()
        }
        try model.disconnect()
        await refresh.value
        guard case .disconnected = model.calendarState else { Issue.record("A completion from the disconnected instance leaked into the calendar"); return }
        #expect(model.calendarState.snapshot.calendars.isEmpty)
    }

    @Test func switchingUsersOnOneInstanceClearsPrivateStateAndRejectsOtherUsersOutbox() async throws {
        let domain = "MeetingNotes-account-isolation-\(UUID().uuidString)"
        let defaults = try #require(UserDefaults(suiteName: domain))
        defer { defaults.removePersistentDomain(forName: domain) }
        let userA = "00000000-0000-4000-8000-000000000001"
        let userB = "00000000-0000-4000-8000-000000000002"
        let model = ServerNotesModel(makeAPI: { credentials in
            MeetingNotesAPI(credentials: credentials, configuration: configuration(automaticSession: false) { request in
                let isB = request.value(forHTTPHeaderField: "Authorization") == "Bearer token-user-b"
                if request.url?.path.hasSuffix("/session") == true {
                    return Response(body: sessionJSON.replacingOccurrences(of: userA, with: isB ? userB : userA))
                }
                if isSummaries(request) { return Response(body: summariesPage(isB ? [] : [summaryJSON])) }
                if request.url?.path.hasSuffix(noteID) == true { return Response(body: noteJSON) }
                return Response(body: isB ? "[]" : "[\(noteJSON)]")
            })
        })
        await model.connect(instance: "https://example.test", token: "token-user-a")
        let first = model.accountIdentity
        #expect(model.list.rows.count == 1)
        _ = try await model.note(id: noteID)
        #expect(model.state.notes.count == 1)
        await model.connect(instance: "https://example.test", token: "token-user-b")
        #expect(model.accountIdentity != first)
        #expect(model.principalID == userB)
        #expect(model.list.rows.isEmpty && model.list.total == 0)
        #expect(model.state.notes.isEmpty)
    }

    @Test func partialEnhancedEditUsesCanonicalPatchWithoutReplacingTypedOrCaptureFields() async throws {
        let domain = "MeetingNotes-partial-update-tests-\(UUID().uuidString)"
        let defaults = try #require(UserDefaults(suiteName: domain))
        defer { defaults.removePersistentDomain(forName: domain) }
        let model = ServerNotesModel(makeAPI: { credentials in
            MeetingNotesAPI(credentials: credentials, configuration: configuration { request in
                guard request.httpMethod == "PATCH" else { return Response(body: summariesPage([summaryJSON])) }
                let payload = (try? JSONSerialization.jsonObject(with: requestBody(request))) as? [String: String]
                #expect(payload == ["enhanced": "Edited enhanced notes"])
                #expect(payload?["body"] == nil)
                #expect(payload?["transcriptSegments"] == nil)
                return Response(body: noteJSON.replacingOccurrences(of: "Generated notes", with: "Edited enhanced notes"))
            })
        })
        await model.connect(instance: "https://example.test", token: "unit-test-token")
        let result = try await model.update(id: noteID, UpdateNoteInput(body: nil, date: nil, duration: nil,
            enhanced: "Edited enhanced notes", title: nil, transcriptSegments: nil))
        #expect(result.enhanced == "Edited enhanced notes")
        #expect(result.body == "Editable notes")
        #expect(result.transcriptRevision == 2)
        #expect(result.generatedRevision == 2)
        #expect(result.transcriptSegments.first?.text == "Hello café 🦊")
    }

    @Test func connectThenCreateEditDeleteUpdatesOnlyServerState() async throws {
        let domain = "MeetingNotes-model-tests-\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: domain)!
        defer { defaults.removePersistentDomain(forName: domain) }
        let model = ServerNotesModel(makeAPI: { credentials in
            MeetingNotesAPI(credentials: credentials, configuration: configuration { request in
                switch request.httpMethod {
                case "POST": Response(body: noteJSON.replacingOccurrences(of: "Server note", with: "Created"))
                case "PATCH": Response(body: noteJSON.replacingOccurrences(of: "Server note", with: "Renamed"))
                case "DELETE": Response(body: #"{"deleted":true}"#)
                default: Response(body: summariesPage([summaryJSON]))
                }
            })
        })
        #expect(model.state.notes.isEmpty && model.list.rows.isEmpty)
        await model.connect(instance: "https://example.test", token: "unit-test-token")
        // Connecting loads rows, not whole notes.
        #expect(model.list.rows.first?.title == "Server note")
        #expect(model.state.notes.isEmpty)
        let created = try await model.create(title: "Created", body: "Body")
        #expect(created.title == "Created")
        #expect(model.state.notes.count == 1)
        #expect(model.list.rows.map(\.title) == ["Created"])
        _ = try await model.update(id: noteID, title: "Renamed", body: "Body")
        #expect(model.state.notes.first?.title == "Renamed")
        #expect(model.list.rows.map(\.title) == ["Renamed"])
        try await model.delete(id: noteID)
        #expect(model.state.notes.isEmpty)
        #expect(model.list.rows.isEmpty && model.list.total == 0)
        try model.disconnect()
        #expect(model.instance == nil)
        await #expect(throws: APIClientError.unauthorized) { try await model.create(title: "No connection", body: "") }
    }

    @Test func rejectedConnectionDoesNotPersistCredentialsOrSeedDemoNotes() async throws {
        let domain = "MeetingNotes-rejected-tests-\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: domain)!
        defer { defaults.removePersistentDomain(forName: domain) }
        let model = ServerNotesModel(makeAPI: { credentials in
            MeetingNotesAPI(credentials: credentials, configuration: configuration { _ in Response(status: 401, body: "{}") })
        })
        await model.connect(instance: "https://example.test", token: "rejected-token")
        #expect(model.instance == nil)
        #expect(model.state.notes.isEmpty)
        guard case .failed(.unauthorized, _) = model.state else { Issue.record("Expected an unauthorized connection state"); return }
    }
}

private final class CalendarResponseState: @unchecked Sendable {
    private let lock = NSLock()
    private var stored = 0
    var value: Int {
        get { lock.lock(); defer { lock.unlock() }; return stored }
        set { lock.lock(); defer { lock.unlock() }; stored = newValue }
    }
}

private final class MemoryTokens: BearerTokenStore, @unchecked Sendable {
    private let lock = NSLock()
    private var token: String?
    func load() throws -> String? { lock.lock(); defer { lock.unlock() }; return token }
    func save(_ token: String) throws { lock.lock(); defer { lock.unlock() }; self.token = token }
    func clear() throws { lock.lock(); defer { lock.unlock() }; token = nil }
}

@MainActor @Suite struct ConnectionStoreTests {
    @Test func preferencesNeverContainTokenAndBlankFieldCannotReuseItForAnotherServer() throws {
        let domain = "MeetingNotes-tests-\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: domain)!
        defer { defaults.removePersistentDomain(forName: domain) }
        let store = ConnectionSettingsStore(defaults: defaults, tokens: MemoryTokens())
        let first = try InstanceURL("https://first.example")
        let credentials = try InstanceCredentials(instance: first, bearerToken: "test-only-secret")
        try store.save(credentials)
        #expect(defaults.string(forKey: "meetingNotes.instanceURL") == "https://first.example")
        #expect(!String(describing: defaults.persistentDomain(forName: domain)).contains("test-only-secret"))
        #expect(try store.load()?.instance == first)
        #expect(throws: APIClientError.invalidToken) {
            try store.credentials(instance: InstanceURL("https://other.example"), enteredToken: "")
        }
        _ = try store.credentials(instance: first, enteredToken: "")
        try store.clear()
        #expect(try store.load() == nil)
    }
}

@Suite struct StreamingChatTests {
    @Test func authenticatesAndDecodesOnlyRealUnicodeDeltasUntilDone() async throws {
        let api = try client { request in
            #expect(request.url?.path == "/base/api/v1/notes/\(noteID)/chat/stream")
            #expect(request.httpMethod == "POST")
            #expect(request.value(forHTTPHeaderField: "Authorization") == "Bearer unit-test-token")
            #expect(request.value(forHTTPHeaderField: "Accept") == "text/event-stream")
            return Response(body: "data: {\"type\":\"delta\",\"text\":\"Café \"}\n\ndata: {\"type\":\"delta\",\"text\":\"🦊 Monday\"}\n\ndata: {\"type\":\"done\",\"text\":\"\"}\n\n", contentType: "text/event-stream")
        }
        var deltas: [String] = []
        for try await text in try api.chatStream(noteID: noteID, message: "When?", history: []) { deltas.append(text) }
        #expect(deltas == ["Café ", "🦊 Monday"])
    }

    @Test(arguments: ["data: {\"type\":\"delta\",\"text\":\"Partial\"}\n\n", "data: {\"type\":\"error\",\"text\":\"Do not expose upstream key\"}\n\n"])
    func truncatedAndProviderErrorStreamsFailInsteadOfClaimingCompletion(body: String) async throws {
        let api = try client { _ in Response(body: body, contentType: "text/event-stream") }
        var failed = false
        do { for try await _ in try api.chatStream(noteID: noteID, message: "Question", history: []) {} }
        catch let error as APIClientError {
            failed = true
            #expect(!error.localizedDescription.contains("upstream key"))
        }
        #expect(failed)
    }

    @Test func streamingAuthFailureNeverDecodesProviderBody() async throws {
        let api = try client { _ in Response(status: 401, body: "private server body") }
        do {
            for try await _ in try api.chatStream(noteID: noteID, message: "Question", history: []) {}
            Issue.record("Expected authentication failure")
        } catch let error as APIClientError { #expect(error == .unauthorized) }
    }
}

private let organizationID = "00000000-0000-4000-8000-000000000011"
private let memberID = "00000000-0000-4000-8000-000000000012"
private let invitationID = "00000000-0000-4000-8000-000000000013"
private let organizationJSON = #"{"id":"00000000-0000-4000-8000-000000000011","name":"Fictional workspace","slug":"fictional-workspace","role":"owner","createdAt":"2026-11-13T00:00:00Z"}"#
private let memberJSON = #"{"id":"00000000-0000-4000-8000-000000000012","organizationId":"00000000-0000-4000-8000-000000000011","userId":"00000000-0000-4000-8000-000000000002","name":"Fictional Member","email":"member@example.test","role":"member","createdAt":"2026-11-13T00:00:00Z"}"#
private let invitationJSON = #"{"id":"00000000-0000-4000-8000-000000000013","organizationId":"00000000-0000-4000-8000-000000000011","organizationName":"Fictional workspace","email":"member@example.test","role":"member","status":"pending","inviterId":"00000000-0000-4000-8000-000000000001","createdAt":"2026-11-13T00:00:00Z","expiresAt":"2026-11-20T00:00:00Z"}"#

@Suite struct WorkspaceAPITests {
    @Test func workspaceRoutesUseSignedBearerAndCanonicalDTOs() async throws {
        let api = try client { request in
            #expect(request.value(forHTTPHeaderField: "Authorization") == "Bearer unit-test-token")
            #expect(request.value(forHTTPHeaderField: "Cookie") == nil)
            let path = request.url!.path
            let input = (try? JSONSerialization.jsonObject(with: requestBody(request))) as? [String: String]
            if path.hasSuffix("/organizations/active") {
                #expect(request.httpMethod == "POST"); #expect(input?["organizationId"] == organizationID)
                return Response(body: "{\"organizations\":[\(organizationJSON)],\"activeOrganizationId\":\"\(organizationID)\"}")
            }
            if path.hasSuffix("/organizations") {
                if request.httpMethod == "GET" { return Response(body: "{\"organizations\":[\(organizationJSON)],\"activeOrganizationId\":null}") }
                #expect(request.httpMethod == "POST"); #expect(input?["slug"] == "fictional-workspace")
                return Response(body: organizationJSON)
            }
            #expect(path == "/base/api/v1/organizations/\(organizationID)")
            if request.httpMethod == "PATCH" { #expect(input?["name"] == "Renamed"); return Response(body: organizationJSON) }
            #expect(request.httpMethod == "GET")
            return Response(body: "{\"organization\":\(organizationJSON),\"members\":[\(memberJSON)],\"invitations\":[\(invitationJSON)]}")
        }
        #expect(try await api.organizations().activeOrganizationId == nil)
        #expect(try await api.createOrganization(CreateOrganizationInput(name: "Fictional workspace", slug: "fictional-workspace")).role == .owner)
        #expect(try await api.organization(id: organizationID).members.first?.role == .member)
        #expect(try await api.updateOrganization(id: organizationID, UpdateOrganizationInput(name: "Renamed")).id == organizationID)
        #expect(try await api.setActiveOrganization(id: organizationID).activeOrganizationId == organizationID)
    }

    @Test func membershipAndInvitationCommandsDoNotTargetPrivateNotes() async throws {
        let api = try client { request in
            let path = request.url!.path
            #expect(path.hasPrefix("/base/api/v1/organizations/") || path.hasPrefix("/base/api/v1/organization-invitations"))
            #expect(!path.contains("/notes"))
            let input = (try? JSONSerialization.jsonObject(with: requestBody(request))) as? [String: String]
            if request.httpMethod == "DELETE" || path.hasSuffix("/reject") { return Response(body: #"{"deleted":true}"#) }
            if path.hasSuffix("/accept") { return Response(body: organizationJSON) }
            if path.hasSuffix("/members/\(memberID)") {
                #expect(request.httpMethod == "PATCH"); #expect(input?["role"] == "admin")
                return Response(body: memberJSON)
            }
            if path.hasSuffix("/organization-invitations") { return Response(body: "{\"invitations\":[\(invitationJSON)]}") }
            #expect(path.hasSuffix("/invitations")); #expect(input?["email"] == "member@example.test"); #expect(input?["role"] == "member")
            return Response(body: invitationJSON)
        }
        #expect(try await api.organizationInvitations().invitations.first?.status == .pending)
        #expect(try await api.inviteOrganizationMember(id: organizationID, InviteOrganizationMemberInput(email: "member@example.test", role: .member)).id == invitationID)
        _ = try await api.cancelOrganizationInvitation(id: organizationID, invitationID: invitationID)
        _ = try await api.updateOrganizationMember(id: organizationID, memberID: memberID, UpdateOrganizationMemberRoleInput(role: .admin))
        _ = try await api.removeOrganizationMember(id: organizationID, memberID: memberID)
        _ = try await api.acceptOrganizationInvitation(id: invitationID)
        _ = try await api.rejectOrganizationInvitation(id: invitationID)
        await #expect(throws: APIClientError.invalidIdentifier) { try await api.organization(id: "../notes") }
        await #expect(throws: APIClientError.invalidIdentifier) { try await api.removeOrganizationMember(id: organizationID, memberID: "../notes") }
    }

    @Test func organizationRolesRemainSeparateFromInstanceRoles() {
        let owner = WorkspacePermissions(role: "owner"), admin = WorkspacePermissions(role: "admin"), member = WorkspacePermissions(role: "member")
        #expect(owner.canManage && owner.canAssignAdmin)
        #expect(owner.canManageMember(role: "admin") && owner.canManageMember(role: "member"))
        #expect(!owner.canManageMember(role: "owner"))
        #expect(admin.canManage && !admin.canAssignAdmin)
        #expect(admin.canManageMember(role: "member") && !admin.canManageMember(role: "admin") && !admin.canManageMember(role: "owner"))
        #expect(!member.canManage && !member.canManageMember(role: "member"))
        #expect(!WorkspacePermissions(role: "user").canManage)
    }

    @MainActor @Test func workspaceSelectionPreservesPrivateNoteAccountAndClearsOnDisconnect() async throws {
        let domain = "Cereal-workspace-tests-\(UUID().uuidString)"
        let defaults = try #require(UserDefaults(suiteName: domain))
        defer { defaults.removePersistentDomain(forName: domain) }
        let selection = CalendarResponseState()
        let model = ServerNotesModel(makeAPI: { credentials in
            MeetingNotesAPI(credentials: credentials, configuration: configuration { request in
                let path = request.url!.path
                if path.hasSuffix("/notes/summaries") { return Response(body: summariesPage([summaryJSON])) }
                if path.hasSuffix("/notes") { return Response(body: "[\(noteJSON)]") }
                if path.hasSuffix("/organizations/active") { selection.value = 1 }
                if path.hasSuffix("/organizations") || path.hasSuffix("/organizations/active") {
                    let active = selection.value == 1 ? "\"\(organizationID)\"" : "null"
                    return Response(body: "{\"organizations\":[\(organizationJSON)],\"activeOrganizationId\":\(active)}")
                }
                if path.hasSuffix("/organization-invitations") { return Response(body: #"{"invitations":[]}"#) }
                return Response(status: 500, body: "{}")
            })
        })
        await model.connect(instance: "https://example.test", token: "unit-test-token")
        let account = model.accountIdentity
        await model.workspaces.refresh()
        #expect(model.workspaces.organizations.count == 1)
        await model.workspaces.activate(id: organizationID)
        #expect(model.workspaces.activeOrganizationID == organizationID)
        #expect(model.accountIdentity == account)
        #expect(model.list.rows.first?.id == noteID)
        #expect(model.list.rows.first?.transcriptRevision == 2)
        try model.disconnect()
        #expect(model.workspaces.organizations.isEmpty && model.workspaces.activeOrganizationID == nil)
        #expect(model.workspaces.invitations.isEmpty && !model.workspaces.loaded)
    }

    @MainActor @Test func disconnectedWorkspaceModelRejectsLateMembershipResponse() async throws {
        let model = ServerWorkspacesModel()
        model.attach(try client { request in
            Thread.sleep(forTimeInterval: 0.04)
            if request.url?.path.hasSuffix("/organization-invitations") == true { return Response(body: #"{"invitations":[]}"#) }
            return Response(body: "{\"organizations\":[\(organizationJSON)],\"activeOrganizationId\":\"\(organizationID)\"}")
        })
        let pending = Task { await model.refresh() }
        for _ in 0..<50 { if model.busy { break }; await Task.yield() }
        model.attach(nil)
        await pending.value
        #expect(model.organizations.isEmpty && model.invitations.isEmpty)
        #expect(model.activeOrganizationID == nil && !model.loaded && !model.busy && model.error == nil)
    }

    @MainActor @Test func forbiddenWorkspaceActionSurfacesFailureWithoutOptimisticSelection() async throws {
        let model = ServerWorkspacesModel()
        model.attach(try client { _ in Response(status: 403, body: #"{"error":"forbidden","message":"Workspace role does not allow this"}"#) })
        await model.activate(id: organizationID)
        #expect(model.activeOrganizationID == nil)
        #expect(model.error?.contains("workspace role") == true)
        #expect(!model.busy)
    }
}

@Suite struct SourceAwareStreamingTests {
    @Test func actualToolEventsAndIssuedSourcesRemainTypedAndOptInWithLocalTimeZone() async throws {
        let body = """
        data: {"type":"tool","text":"","tool":{"callId":"\(memberID)","name":"search_meetings","label":"Searching meetings","status":"running"}}

        data: {"type":"tool","text":"","tool":{"callId":"\(memberID)","name":"search_meetings","label":"Found meetings","status":"completed","resultCount":1,"results":[{"noteId":"\(noteID)","title":"Actual returned title","date":"2026-11-13"}]}}

        data: {"type":"delta","text":"Friday [[S1]]."}

        data: {"type":"citations","text":"","citations":[{"id":"S1","noteId":"\(noteID)","title":"Actual returned title","source":"notes","segmentId":null,"startMs":null,"quote":"Friday","transcriptRevision":2}]}

        data: {"type":"done","text":""}

        """
        let api = try client { request in
            let object = (try? JSONSerialization.jsonObject(with: requestBody(request))) as? [String: Any]
            #expect(object?["includeSources"] as? Bool != nil)
            #expect(object?["timeZone"] as? String == TimeZone.current.identifier)
            #expect(request.value(forHTTPHeaderField: "Authorization") == "Bearer unit-test-token")
            let response = object?["includeSources"] as? Bool == true ? body : "data: {\"type\":\"delta\",\"text\":\"Friday.\"}\n\ndata: {\"type\":\"done\",\"text\":\"\"}\n\n"
            return Response(body: response, contentType: "text/event-stream")
        }
        var events: [ChatStreamEventDTO] = []
        for try await event in try api.chatEvents(noteID: noteID, message: "When?", history: []) { events.append(event) }
        #expect(events.map(\.type) == [.tool, .tool, .delta, .citations, .done])
        #expect(events[1].tool?.results?.first?.title == "Actual returned title")
        #expect(events[1].tool?.results?.first?.date == "2026-11-13")
        #expect(events[3].citations?.first?.id == "S1")
        var deltas: [String] = []
        for try await delta in try api.chatStream(noteID: noteID, message: "When?", history: []) { deltas.append(delta) }
        #expect(deltas == ["Friday."])
    }

    @Test(arguments: [
        #"{"type":"tool","text":""}"#,
        #"{"type":"tool","text":"","tool":{"callId":"not-a-uuid","name":"search_meetings","label":"Synthetic","status":"running"}}"#,
        #"{"type":"citations","text":"","citations":[{"id":"S0","noteId":"00000000-0000-4000-8000-000000000001","title":"Invalid","source":"notes","segmentId":null,"startMs":null,"quote":"Friday","transcriptRevision":0}]}"#
    ])
    func malformedMetadataDoesNotBecomeAUsableSource(payload: String) async throws {
        let api = try client { _ in Response(body: "data: \(payload)\n\ndata: {\"type\":\"done\",\"text\":\"\"}\n\n", contentType: "text/event-stream") }
        await #expect(throws: APIClientError.invalidJSON) {
            for try await _ in try api.chatEvents(noteID: noteID, message: "When?", history: []) { }
        }
    }

    @Test func disconnectedClientCannotBeginAnotherSourceAwareRequest() throws {
        let api = try client { _ in Issue.record("Closed transport must not send a request"); return Response(body: "") }
        api.cancelRequests()
        #expect(throws: CancellationError.self) { try api.chatEvents(noteID: noteID, message: "Question", history: []) }
    }
}


@Suite struct WorkspaceChatTransportTests {
    @Test func globalChatUsesAccountWorkspaceRouteAndStructuredSourcesWithoutAnInventedNoteID() async throws {
        let api = try client { request in
            #expect(request.url?.path == "/base/api/v1/chat/stream")
            #expect(!request.url!.path.contains("/notes/"))
            let payload = (try? JSONSerialization.jsonObject(with: requestBody(request))) as? [String: Any]
            #expect(payload?["includeSources"] as? Bool == true)
            #expect(payload?["timeZone"] as? String == TimeZone.current.identifier)
            return Response(body: "data: {\"type\":\"delta\",\"text\":\"Actual workspace answer\"}\n\ndata: {\"type\":\"done\",\"text\":\"\"}\n\n", contentType: "text/event-stream")
        }
        var answer = ""
        for try await event in try api.chatEvents(noteID: nil, message: "Recent todos", history: []) where event.type == .delta { answer += event.text }
        #expect(answer == "Actual workspace answer")
    }
}

@Test func instanceChatChoicesAndSelectedModelStayOnTheAuthenticatedTransport() async throws {
    let api = try client { request in
        #expect(request.value(forHTTPHeaderField: "Authorization") == "Bearer unit-test-token")
        if request.url?.path.hasSuffix("/models/chat") == true {
            return Response(body: #"{"models":[{"provider":"openai","model":"gpt-test"}]}"#)
        }
        let body = try! JSONSerialization.jsonObject(with: requestBody(request)) as! [String: Any]
        #expect(body["model"] as? String == "openai:gpt-test")
        return Response(body: "data: {\"type\":\"delta\",\"text\":\"Selected answer\"}\n\ndata: {\"type\":\"done\",\"text\":\"\"}\n\n", contentType: "text/event-stream")
    }
    let choices = try await api.chatModels()
    #expect(choices.models.first?.provider == .openai)
    var text = ""
    for try await event in try api.chatEvents(noteID: nil, message: "Question", history: [], model: "openai:gpt-test") where event.type == .delta { text += event.text }
    #expect(text == "Selected answer")
}

@Test func chatChoicesKeepTheModelsThisBuildCanReadAndSkipTheRest() async throws {
    let api = try client { _ in
        Response(body: #"{"models":[{"provider":"openai_compatible","model":"llama3.2:3b"},{"provider":"a_future_provider","model":"x"},{"provider":"openrouter","model":"openai/gpt-6-sol"}]}"#)
    }
    let choices = try await api.chatModels()
    #expect(choices.models.map(\.model) == ["llama3.2:3b", "openai/gpt-6-sol"])
    #expect(choices.models.first?.provider == .openaiCompatible)
}

// MARK: - Paged list and server search

private final class RequestLog: @unchecked Sendable {
    private let lock = NSLock()
    private var stored: [String] = []
    func record(_ request: URLRequest) {
        lock.lock(); defer { lock.unlock() }
        stored.append((request.httpMethod ?? "GET") + " " + (request.url?.path ?? "") + (request.url?.query.map { "?" + $0 } ?? ""))
    }
    var all: [String] { lock.lock(); defer { lock.unlock() }; return stored }
    func count(_ fragment: String) -> Int { all.filter { $0.contains(fragment) }.count }
}

private func uuid(_ n: Int) -> String { "00000000-0000-4000-8000-" + String(format: "%012d", n) }
private func summary(_ n: Int, title: String? = nil, date: String = "2026-11-12", attendees: String = #"{"people":[],"others":0,"total":0}"#) -> String {
    #"{"id":"\#(uuid(n))","title":"\#(title ?? "Note \(n)")","date":"\#(date)","duration":"","createdAt":"\#(date)T10:00:\#(String(format: "%02d", n % 60)).000Z","updatedAt":"\#(date)T10:00:\#(String(format: "%02d", n % 60)).000Z","recordingState":"idle","generationState":"idle","transcriptRevision":0,"generatedRevision":null,"hasBody":false,"hasSummary":false,"hasTranscript":false,"speakerCount":0,"excerpt":"","meeting":null,"attendees":\#(attendees),"organizationId":null}"#
}
private func wholeNote(_ n: Int, title: String, date: String = "2026-11-12", body: String = "") -> String {
    #"{"id":"\#(uuid(n))","title":"\#(title)","body":"\#(body)","enhanced":"","date":"\#(date)","duration":"","createdAt":"\#(date)T10:00:\#(String(format: "%02d", n % 60)).000Z","updatedAt":"\#(date)T11:00:00.000Z","recordingState":"idle","generationState":"idle","transcriptRevision":0,"generatedRevision":null,"transcriptSegments":[]}"#
}
private func searchPage(_ results: [String], next: String? = nil, total: Int? = nil) -> String {
    "{\"results\":[\(results.joined(separator: ","))],\"nextCursor\":\(next.map { "\"\($0)\"" } ?? "null"),\"total\":\(total ?? results.count),\"terms\":[]}"
}
private func found(_ n: Int, title: String, excerpt: String = "null") -> String {
    #"{"note":\#(summary(n, title: title)),"matchedIn":["title"],"title":[{"text":"\#(title)","hit":true}],"excerpt":\#(excerpt)}"#
}
private func cursor(of request: URLRequest) -> String? {
    URLComponents(url: request.url!, resolvingAgainstBaseURL: false)?.queryItems?.first { $0.name == "cursor" }?.value
}
private func queryText(of request: URLRequest) -> String? {
    URLComponents(url: request.url!, resolvingAgainstBaseURL: false)?.queryItems?.first { $0.name == "q" }?.value
}
@MainActor private func connected(_ handler: @escaping @Sendable (URLRequest) -> Response) async throws -> ServerNotesModel {
    let domain = "Cereal-list-tests-\(UUID().uuidString)"
    let defaults = try #require(UserDefaults(suiteName: domain))
    let model = ServerNotesModel(makeAPI: { credentials in
        MeetingNotesAPI(credentials: credentials, configuration: configuration(handler))
    })
    await model.connect(instance: "https://example.test", token: "unit-test-token")
    defaults.removePersistentDomain(forName: domain)
    return model
}
@MainActor private func eventually(_ condition: @MainActor () -> Bool) async -> Bool {
    for _ in 0..<400 { if condition() { return true }; try? await Task.sleep(for: .milliseconds(5)) }
    return condition()
}

@Suite struct NoteListTransportTests {
    @Test func summariesAndSearchUseTheirOwnRoutesAndDecodeAttendees() async throws {
        let log = RequestLog()
        let people = #"{"people":[{"name":"Grace Hopper","shortName":"Grace","initials":"GH"},{"name":"alan@example.test","shortName":"alan","initials":"AE"}],"others":6,"total":7}"#
        let api = try client { request in
            log.record(request)
            #expect(request.httpMethod == "GET")
            #expect(request.value(forHTTPHeaderField: "Authorization") == "Bearer unit-test-token")
            if request.url?.path == "/base/api/v1/notes/search" { return Response(body: searchPage([found(1, title: "Roadmap", excerpt: #"{"source":"transcript","parts":[{"text":"the ","hit":false},{"text":"roadmap","hit":true}],"segmentId":"\#(uuid(9))","startMs":1200}"#)], next: "s_2", total: 9)) }
            #expect(request.url?.path == "/base/api/v1/notes/summaries")
            return Response(body: summariesPage([summary(1, attendees: people)], next: "abc_-123", total: 40))
        }
        let page = try await api.noteSummaries(limit: 2, cursor: "abc_-")
        #expect(page.total == 40 && page.nextCursor == "abc_-123")
        #expect(page.notes[0].attendees.people.map(\.name) == ["Grace Hopper", "alan@example.test"])
        #expect(page.notes[0].attendees.total == 7 && page.notes[0].attendees.others == 6)
        let row = ServerNoteRow(page.notes[0])
        #expect(row.firstAttendee == "Grace Hopper" && row.attendeeTotal == 7)
        let results = try await api.searchNotes(query: "road map " + String(repeating: "x", count: 300))
        #expect(results.total == 9 && results.nextCursor == "s_2")
        let hit = ServerNoteSearchHit(results.results[0])
        #expect(hit.excerpt == [.init(text: "the ", isMatch: false), .init(text: "roadmap", isMatch: true)])
        #expect(log.all[0] == "GET /base/api/v1/notes/summaries?limit=2&cursor=abc_-")
        #expect(log.all[1].hasPrefix("GET /base/api/v1/notes/search?q=road%20map%20x"))
        #expect(log.all[1].hasSuffix("&limit=30"))
        // The query is cut to what the server accepts.
        #expect(log.all[1].components(separatedBy: "x").count - 1 == 191)
    }

    @Test func aCursorOrPageSizeTheServerDidNotIssueIsNeverSent() async throws {
        let log = RequestLog()
        let api = try client { request in log.record(request); return Response(body: summariesPage([])) }
        for cursor in ["", "a b", "é", "a/b", "a&x=1", String(repeating: "a", count: 513)] {
            await #expect(throws: APIClientError.invalidResponse) { try await api.noteSummaries(cursor: cursor) }
            await #expect(throws: APIClientError.invalidResponse) { try await api.searchNotes(query: "plan", cursor: cursor) }
        }
        for limit in [0, -1, 101] { await #expect(throws: APIClientError.invalidResponse) { try await api.noteSummaries(limit: limit) } }
        #expect(log.all.isEmpty)
    }
}

@MainActor @Suite struct ServerNoteListTests {
    @Test func homeLoadsOnePageThenTheNextOnDemandWithoutDuplicatesOrTheWholeList() async throws {
        let log = RequestLog()
        let model = try await connected { request in
            log.record(request)
            guard isSummaries(request) else { return Response(status: 500, body: "{}") }
            switch cursor(of: request) {
            case nil: return Response(body: summariesPage([summary(9), summary(8), summary(7)], next: "p2", total: 7))
            // The note that was last on the first page is handed over again, as after an edit between requests.
            case "p2": return Response(body: summariesPage([summary(7), summary(6), summary(5)], next: "p3", total: 7))
            default: return Response(body: summariesPage([summary(4), summary(3)], total: 7))
            }
        }
        #expect(model.list.rows.map(\.id) == [9, 8, 7].map(uuid))
        #expect(model.list.total == 7 && model.list.hasMore)
        #expect(model.state.notes.isEmpty)
        let before = model.list.revision
        await model.loadMore()
        #expect(model.list.rows.map(\.id) == [9, 8, 7, 6, 5].map(uuid))
        #expect(model.list.revision > before)
        await model.loadMore()
        #expect(model.list.rows.map(\.id) == [9, 8, 7, 6, 5, 4, 3].map(uuid))
        #expect(!model.list.hasMore && !model.list.isLoadingMore && model.list.moreError == nil)
        await model.loadMore()
        #expect(log.count("/notes/summaries") == 3)
        #expect(log.all.filter { $0.hasPrefix("GET /api/v1/notes/summaries") }.map { $0.components(separatedBy: "?").last } == ["limit=50", "limit=50&cursor=p2", "limit=50&cursor=p3"])
        // The whole list was never downloaded.
        #expect(!log.all.contains("GET /api/v1/notes"))
    }

    @Test func asecondRequestForTheNextPageWhileOneIsOnItsWayDoesNothing() async throws {
        let log = RequestLog()
        let model = try await connected { request in
            log.record(request)
            if cursor(of: request) == nil { return Response(body: summariesPage([summary(2)], next: "p2", total: 2)) }
            Thread.sleep(forTimeInterval: 0.08)
            return Response(body: summariesPage([summary(1)], total: 2))
        }
        async let first: Void = model.loadMore()
        async let second: Void = model.loadMore()
        _ = await (first, second)
        #expect(log.count("cursor=p2") == 1)
        #expect(model.list.rows.count == 2)
    }

    @Test func aFailedNextPageKeepsTheRowsAndIsRetriedByAskingAgain() async throws {
        let fail = CalendarResponseState()
        fail.value = 1
        let model = try await connected { request in
            if cursor(of: request) == nil { return Response(body: summariesPage([summary(2)], next: "p2", total: 2)) }
            return fail.value == 1 ? Response(status: 503, body: "{}") : Response(body: summariesPage([summary(1)], total: 2))
        }
        await model.loadMore()
        #expect(model.list.moreError == .unavailable)
        #expect(model.list.rows.count == 1 && model.list.hasMore && !model.list.isLoadingMore)
        guard case .loaded = model.state else { Issue.record("A failed further page must not put the whole list into an error state"); return }
        fail.value = 0
        await model.loadMore()
        #expect(model.list.moreError == nil && model.list.rows.count == 2 && !model.list.hasMore)
    }

    @Test func refreshStartsAgainFromTheTopAndDropsAPageThatWasOnItsWay() async throws {
        let round = CalendarResponseState()
        let model = try await connected { request in
            if cursor(of: request) != nil { Thread.sleep(forTimeInterval: 0.15); return Response(body: summariesPage([summary(1, title: "Stale page")], total: 3)) }
            defer { round.value += 1 }
            return round.value == 0 ? Response(body: summariesPage([summary(3), summary(2)], next: "p2", total: 3))
                : Response(body: summariesPage([summary(5, title: "Newest"), summary(3)], next: "p2b", total: 4))
        }
        let more = Task { await model.loadMore() }
        #expect(await eventually { model.list.isLoadingMore })
        await model.refresh()
        await more.value
        #expect(model.list.rows.map(\.title) == ["Newest", "Note 3"])
        #expect(model.list.total == 4 && model.list.hasMore && !model.list.isLoadingMore)
        guard case .loaded = model.state else { Issue.record("Expected a loaded list after refresh"); return }
    }

    @Test func changesMadeOnThisPhoneReachTheRowsWithoutAReload() async throws {
        let people = #"{"people":[{"name":"Grace Hopper","shortName":"Grace","initials":"GH"}],"others":1,"total":2}"#
        let model = try await connected { request in
            switch request.httpMethod {
            case "POST": return Response(body: wholeNote(50, title: "Created", date: "2026-11-13"))
            case "PATCH": return Response(body: wholeNote(8, title: "Renamed"))
            case "DELETE": return Response(body: #"{"deleted":true}"#)
            default: break
            }
            if cursor(of: request) == nil { return Response(body: summariesPage([summary(9), summary(8, attendees: people)], next: "p2", total: 4)) }
            Thread.sleep(forTimeInterval: 0.1)
            return Response(body: summariesPage([summary(8), summary(7), summary(6)], total: 4))
        }
        // A new note is the newest: it goes to the top and is counted.
        _ = try await model.create(title: "Created", body: "")
        #expect(model.list.rows.map(\.title) == ["Created", "Note 9", "Note 8"])
        #expect(model.list.total == 5)
        // An edit changes the row where it stands and keeps what a whole note does not carry.
        _ = try await model.update(id: uuid(8), title: "Renamed", body: "")
        #expect(model.list.rows.map(\.title) == ["Created", "Note 9", "Renamed"])
        #expect(model.list.rows[2].firstAttendee == "Grace Hopper" && model.list.rows[2].attendeeTotal == 2)
        // A note deleted while the next page is on its way does not come back with that page.
        let more = Task { await model.loadMore() }
        #expect(await eventually { model.list.isLoadingMore })
        try await model.delete(id: uuid(8))
        await more.value
        #expect(model.list.rows.map(\.title) == ["Created", "Note 9", "Note 7", "Note 6"])
        #expect(!model.list.rows.contains { $0.id == uuid(8) })
    }

    @Test func readingANoteKeepsItForReopeningAndNeverAddsARow() async throws {
        let model = try await connected { request in
            if isSummaries(request) { return Response(body: summariesPage([summary(9), summary(8)], next: "p2", total: 40)) }
            let id = request.url!.lastPathComponent, n = Int(id.suffix(12)) ?? 0
            return Response(body: wholeNote(n, title: n == 8 ? "Retitled elsewhere" : "Opened \(n)", date: "2026-01-01"))
        }
        // A note far down the list, opened from a search or a link.
        let opened = try await model.note(id: uuid(3))
        #expect(opened.title == "Opened 3")
        #expect(model.state.notes.map(\.id) == [uuid(3)])
        #expect(model.list.rows.map(\.id) == [uuid(9), uuid(8)] && model.list.total == 40)
        // A loaded row follows what the server now says about its note.
        _ = try await model.note(id: uuid(8))
        #expect(model.list.rows.map(\.title) == ["Note 9", "Retitled elsewhere"])
        #expect(model.title(ofNote: uuid(3)) == "Opened 3" && model.title(ofNote: uuid(9)) == "Note 9" && model.title(ofNote: uuid(77)) == nil)
        // Whole notes are a small recent set, not a second copy of the server.
        for n in 100..<140 { _ = try await model.note(id: uuid(n)) }
        #expect(model.state.notes.count == 24)
        #expect(model.state.notes.first?.id == uuid(139))
        // After a refresh, a kept note whose row changed on the server is out of date and is dropped.
        _ = try await model.note(id: uuid(9))
        await model.refresh()
        #expect(!model.state.notes.contains { $0.id == uuid(9) })
        #expect(model.state.notes.contains { $0.id == uuid(139) })
    }

    @Test func aServerWithoutTheListEndpointIsReadWholeAsBeforeAndSearchedHere() async throws {
        for status in [400, 404] {
            let log = RequestLog()
            let model = try await connected { request in
                log.record(request)
                if isSummaries(request) || request.url?.path.hasSuffix("/notes/search") == true { return Response(status: status, body: #"{"error":"invalid_request","message":"x"}"#) }
                return Response(body: "[\(wholeNote(1, title: "Older", date: "2026-11-01", body: "budget line")),\(wholeNote(2, title: "Newer", date: "2026-11-09"))]")
            }
            #expect(model.list.rows.map(\.title) == ["Newer", "Older"])
            #expect(model.list.total == 2 && !model.list.hasMore)
            #expect(model.state.notes.count == 2)
            await model.loadMore()
            let page = try await model.search(query: "budget", cursor: nil)
            #expect(page.hits.map(\.row.title) == ["Older"])
            #expect(page.hits.first?.excerpt?.map(\.text) == ["budget line"])
            #expect(log.count("/notes/search") == 0 && log.count("/notes/summaries") == 1)
            await model.refresh()
            #expect(model.list.rows.count == 2 && model.state.notes.count == 2)
        }
        // Any other failure is a failure, not an older server.
        let broken = try await connected { _ in Response(status: 503, body: "{}") }
        guard case .failed(.unavailable, _) = broken.state else { Issue.record("Expected the connection to fail"); return }
        #expect(broken.list.rows.isEmpty)
    }

    @Test func rowsKeepTheServersOrderAndPlaceANewNoteInIt() {
        func row(_ id: Int, _ date: String, _ time: String) -> ServerNoteRow { ServerNoteRow(id: uuid(id), title: "", date: date, createdAt: "\(date)T\(time)Z", updatedAt: "") }
        let rows = [row(1, "2026-11-12", "09:00:00.000"), row(3, "2026-11-13", "09:00:00.000"), row(2, "2026-11-13", "09:00:00.000"), row(4, "2026-11-13", "08:00:00.000")]
        #expect(rows.sorted(by: ServerNoteRow.precedes).map(\.id) == [3, 2, 4, 1].map(uuid))
        #expect(!ServerNoteRow.precedes(rows[0], rows[0]))
    }
}

@MainActor @Suite struct ServerNoteSearchTests {
    @Test func typingSendsOneRequestAfterThePauseForWhatTheFieldHoldsThen() async throws {
        let log = RequestLog()
        let model = try await connected { request in
            log.record(request)
            if isSummaries(request) { return Response(body: summariesPage([])) }
            return Response(body: searchPage([found(1, title: "Harrowgate renewal"), found(2, title: "Harrowgate kickoff")], total: 2))
        }
        let search = ServerNoteSearch(server: model, pause: .milliseconds(40))
        for text in ["h", "ha", "har", "harrow", "harrowgate"] { search.update(text) }
        #expect(search.phase == .searching && search.hits.isEmpty)
        #expect(await eventually { search.phase == .loaded })
        #expect(search.hits.map(\.row.title) == ["Harrowgate renewal", "Harrowgate kickoff"])
        #expect(search.answered == "harrowgate" && search.total == 2 && !search.hasMore)
        #expect(log.all.filter { $0.contains("/notes/search") } == ["GET /api/v1/notes/search?q=harrowgate&limit=30"])
        // The same text again, or only different spacing around it, asks nothing.
        search.update("  harrowgate ")
        try await Task.sleep(for: .milliseconds(90))
        #expect(log.count("/notes/search") == 1 && search.phase == .loaded)
    }

    @Test func anAnswerToAnOlderQueryIsNeverShown() async throws {
        let log = RequestLog()
        let model = try await connected { request in
            log.record(request)
            if isSummaries(request) { return Response(body: summariesPage([])) }
            if queryText(of: request) == "slow" { Thread.sleep(forTimeInterval: 0.25); return Response(body: searchPage([found(1, title: "Slow answer")])) }
            return Response(body: searchPage([found(2, title: "Fast answer")]))
        }
        let search = ServerNoteSearch(server: model, pause: .milliseconds(10))
        search.update("slow")
        #expect(await eventually { log.count("q=slow") == 1 })
        search.update("fast")
        #expect(await eventually { search.phase == .loaded })
        #expect(search.hits.map(\.row.title) == ["Fast answer"] && search.answered == "fast")
        try await Task.sleep(for: .milliseconds(350))
        #expect(search.hits.map(\.row.title) == ["Fast answer"] && search.answered == "fast")
    }

    @Test func earlierResultsStayUntilTheNextArriveAndAnEmptyFieldClearsThem() async throws {
        let log = RequestLog()
        let model = try await connected { request in
            log.record(request)
            if isSummaries(request) { return Response(body: summariesPage([])) }
            Thread.sleep(forTimeInterval: 0.05)
            return Response(body: queryText(of: request) == "plan" ? searchPage([found(1, title: "Plan")]) : searchPage([]))
        }
        let search = ServerNoteSearch(server: model, pause: .milliseconds(10))
        search.update("plan")
        #expect(await eventually { search.phase == .loaded })
        search.update("plan zebra")
        #expect(search.phase == .searching && search.hits.count == 1 && search.answered == "plan")
        #expect(await eventually { search.phase == .loaded })
        #expect(search.hits.isEmpty && search.total == 0 && search.answered == "plan zebra")
        // Nothing searchable: no request, no results, at once.
        search.update("plan")
        search.update("p")
        #expect(search.phase == .idle && search.hits.isEmpty && search.answered.isEmpty)
        try await Task.sleep(for: .milliseconds(120))
        #expect(search.phase == .idle)
        #expect(log.count("/notes/search") == 2)
        for text in ["", "  ", "a", "%", "_ -", "1"] { #expect(!ServerNoteSearch.isSearchable(text)) }
        // One character of a script other than ASCII letters and digits is a word, as on the server.
        for text in ["ab", "日", "a b2", "v2.1", "É"] { #expect(ServerNoteSearch.isSearchable(text)) }
    }

    @Test func aFailedSearchSaysSoAndCanBeTriedAgainAndFurtherPagesFollow() async throws {
        let fail = CalendarResponseState()
        fail.value = 1
        let log = RequestLog()
        let model = try await connected { request in
            log.record(request)
            if isSummaries(request) { return Response(body: summariesPage([])) }
            if fail.value == 1 { return Response(status: 503, body: "{}") }
            return cursor(of: request) == nil ? Response(body: searchPage([found(1, title: "One"), found(2, title: "Two")], next: "s2", total: 3))
                : Response(body: searchPage([found(2, title: "Two"), found(3, title: "Three")], total: 3))
        }
        let search = ServerNoteSearch(server: model, pause: .milliseconds(5))
        search.update("roadmap")
        #expect(await eventually { search.phase == .failed(.unavailable) })
        await search.loadMore()
        fail.value = 0
        search.retry()
        #expect(await eventually { search.phase == .loaded })
        #expect(search.hits.count == 2 && search.hasMore && search.total == 3)
        await search.loadMore()
        #expect(search.hits.map(\.row.title) == ["One", "Two", "Three"])
        #expect(!search.hasMore && !search.isLoadingMore)
        await search.loadMore()
        #expect(log.all.filter { $0.contains("cursor=s2") }.count == 1)
    }
}
