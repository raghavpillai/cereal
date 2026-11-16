import Foundation
import Contracts

/// A versioned owner API client. The server remains the source of truth for notes,
/// transcript revisions, and generated notes. No local speech or AI result is fabricated.
public struct MeetingNotesAPI: Sendable {
    private let credentials: InstanceCredentials
    private let session: URLSession
    private let transportState: TransportState
    /// Told when any endpoint answers 401 or 403. That alone decides nothing:
    /// whoever listens asks `/session` what the session's own state is.
    private var onUnauthorized: (@Sendable () -> Void)?

    /// The token, for this module only: to save it in the account's own
    /// Keychain item and to tell two sessions apart. It is never handed out.
    var bearerToken: String { credentials.bearerToken }
    public var instance: InstanceURL { credentials.instance }

    /// The same client, reporting refused requests to `handler`.
    func observingUnauthorized(_ handler: @escaping @Sendable () -> Void) -> MeetingNotesAPI {
        var copy = self
        copy.onUnauthorized = handler
        return copy
    }

    public init(credentials: InstanceCredentials, configuration: URLSessionConfiguration = .ephemeral) {
        self.credentials = credentials
        configuration.httpCookieAcceptPolicy = .never
        configuration.httpShouldSetCookies = false
        configuration.urlCache = nil
        configuration.requestCachePolicy = .reloadIgnoringLocalCacheData
        configuration.timeoutIntervalForRequest = 30
        // Authenticated endpoints must not redirect to another origin, or silently
        // follow a login page. Treat every redirect as an HTTP response to handle.
        self.session = URLSession(configuration: configuration, delegate: RejectRedirects(), delegateQueue: nil)
        self.transportState = TransportState(session: self.session)
    }

    public func session() async throws -> AuthSessionDTO {
        try await send("GET", route: ["session"])
    }

    /// What the server says about this session, asked of `/session` itself.
    public enum SessionVerdict: Sendable {
        case valid(AuthSessionDTO)
        /// The API's own refusal: 401 with the JSON body `{"error":"unauthorized"}`.
        /// A page from a proxy or a captive portal is not this, whatever its status.
        case refused
        case noAnswer(APIClientError)
    }

    public func verifySession() async -> SessionVerdict {
        do {
            try transportState.check()
            var request = URLRequest(url: credentials.instance.endpoint(["api", "v1", "session"]))
            request.httpMethod = "GET"
            request.timeoutInterval = 30
            request.setValue("application/json", forHTTPHeaderField: "Accept")
            request.setValue("Bearer \(credentials.bearerToken)", forHTTPHeaderField: "Authorization")
            let (data, response) = try await session.data(for: request)
            guard let response = response as? HTTPURLResponse else { return .noAnswer(.invalidResponse) }
            switch response.statusCode {
            case 200..<300:
                guard let identity = try? JSONDecoder().decode(AuthSessionDTO.self, from: data) else { return .noAnswer(.invalidJSON) }
                return .valid(identity)
            case 401:
                struct Refusal: Decodable { let error: String }
                let refused = (try? JSONDecoder().decode(Refusal.self, from: data))?.error == "unauthorized"
                return refused ? .refused : .noAnswer(.unauthorized)
            case 403: return .noAnswer(.unauthorized)
            case 404: return .noAnswer(.notFound)
            case 503: return .noAnswer(.unavailable)
            default: return .noAnswer(.httpStatus(response.statusCode))
            }
        } catch let error as URLError {
            return .noAnswer(.transport(error.code))
        } catch { return .noAnswer(error as? APIClientError ?? .invalidResponse) }
    }

    public func calendarPreferences() async throws -> CalendarPreferencesDTO {
        try await send("GET", route: ["calendar", "preferences"])
    }

    /// Better Auth's signed bearer is returned in a header. The password is used
    /// only for this request; neither it nor the unsigned response token is saved.
    public static func signIn(instance: InstanceURL, email: String, password: String, configuration: URLSessionConfiguration = .ephemeral) async throws -> InstanceCredentials {
        guard !email.isEmpty, email.utf8.count <= 320, !password.isEmpty, password.utf8.count <= 4096 else { throw APIClientError.invalidToken }
        configuration.httpShouldSetCookies = false
        configuration.urlCache = nil
        let session = URLSession(configuration: configuration, delegate: RejectRedirects(), delegateQueue: nil)
        defer { session.invalidateAndCancel() }
        var request = URLRequest(url: instance.endpoint(["api", "auth", "sign-in", "email"]))
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        request.httpBody = try JSONSerialization.data(withJSONObject: ["email": email, "password": password, "rememberMe": true])
        let (_, response) = try await session.data(for: request)
        guard let response = response as? HTTPURLResponse else { throw APIClientError.invalidResponse }
        guard (200..<300).contains(response.statusCode) else { throw APIClientError.unauthorized }
        guard let token = response.value(forHTTPHeaderField: "set-auth-token"), !token.isEmpty else { throw APIClientError.invalidResponse }
        return try InstanceCredentials(instance: instance, bearerToken: token)
    }

    public func cancelRequests() {
        transportState.close()
        session.getAllTasks { tasks in tasks.forEach { $0.cancel() } }
    }

    public func signOut() async throws {
        let transport = URLSession(configuration: session.configuration, delegate: RejectRedirects(), delegateQueue: nil)
        defer { transport.invalidateAndCancel() }
        var request = URLRequest(url: credentials.instance.endpoint(["api", "auth", "sign-out"]))
        request.httpMethod = "POST"
        request.setValue("Bearer \(credentials.bearerToken)", forHTTPHeaderField: "Authorization")
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = Data("{}".utf8)
        let (_, response) = try await transport.data(for: request)
        guard let response = response as? HTTPURLResponse, (200..<300).contains(response.statusCode) else { throw APIClientError.unauthorized }
    }

    public func listNotes() async throws -> [NoteDTO] {
        try await send("GET", route: ["notes"])
    }

    /// A page of list rows: no note bodies and no transcripts. Newest meeting first.
    public func noteSummaries(limit: Int = 50, cursor: String? = nil) async throws -> NoteSummariesPageDTO {
        try await send("GET", route: ["notes", "summaries"], query: try Self.page(limit: limit, cursor: cursor))
    }

    /// Notes whose title, text, transcript or invitees contain every word of `query`, best match first.
    public func searchNotes(query: String, limit: Int = 30, cursor: String? = nil) async throws -> NoteSearchPageDTO {
        try await send("GET", route: ["notes", "search"], query: [URLQueryItem(name: "q", value: String(query.prefix(200)))] + (try Self.page(limit: limit, cursor: cursor)))
    }

    /// A cursor is the server's own opaque text; anything else is refused here rather than sent.
    private static func page(limit: Int, cursor: String?) throws -> [URLQueryItem] {
        guard (1...100).contains(limit) else { throw APIClientError.invalidResponse }
        var items = [URLQueryItem(name: "limit", value: String(limit))]
        if let cursor {
            guard !cursor.isEmpty, cursor.utf8.count <= 512, cursor.utf8.allSatisfy({ ($0 >= 48 && $0 <= 57) || ($0 >= 65 && $0 <= 90) || ($0 >= 97 && $0 <= 122) || $0 == 45 || $0 == 95 }) else { throw APIClientError.invalidResponse }
            items.append(URLQueryItem(name: "cursor", value: cursor))
        }
        return items
    }

    public func googleStatus() async throws -> GoogleStatusDTO {
        try await send("GET", route: ["integrations", "google"])
    }

    public func calendars() async throws -> [CalendarDTO] {
        try await send("GET", route: ["calendars"])
    }

    public func calendarEvents(from: Date, to: Date) async throws -> [CalendarEventDTO] {
        guard to > from, to.timeIntervalSince(from) <= 90 * 24 * 60 * 60 else { throw APIClientError.invalidResponse }
        let formatter = ISO8601DateFormatter()
        return try await send("GET", route: ["calendar", "events"], query: [
            URLQueryItem(name: "from", value: formatter.string(from: from)),
            URLQueryItem(name: "to", value: formatter.string(from: to))
        ])
    }

    /// The server atomically reuses an event's note; a tap never starts capture.
    public func noteForCalendarEvent(id: String) async throws -> NoteDTO {
        try validate(id)
        return try await send("POST", route: ["calendar", "events", id, "note"])
    }

    public func noteContext(id: String) async throws -> NoteContextDTO {
        try validate(id)
        return try await send("GET", route: ["notes", id, "context"])
    }

    public func updateNoteContext(id: String, eventID: String?) async throws -> NoteContextDTO {
        try validate(id)
        if let eventID { try validate(eventID) }
        return try await send("PUT", route: ["notes", id, "context"], body: JSONEncoder().encode(UpdateNoteContextInput(eventId: eventID)))
    }

    public func upsertNoteParticipant(id: String, input: ManualParticipantInput) async throws -> NoteContextDTO {
        try validate(id); try validate(input.id)
        return try await send("POST", route: ["notes", id, "context", "participants"], body: JSONEncoder().encode(input))
    }

    public func note(id: String) async throws -> NoteDTO {
        try validate(id)
        return try await send("GET", route: ["notes", id])
    }

    public func createNote(_ input: CreateNoteInput) async throws -> NoteDTO {
        try await send("POST", route: ["notes"], body: JSONEncoder().encode(input))
    }

    public func updateNote(id: String, _ input: UpdateNoteInput) async throws -> NoteDTO {
        try validate(id)
        return try await send("PATCH", route: ["notes", id], body: JSONEncoder().encode(input))
    }

    /// `ifStamp` (`NoteStamp.of`): the note is deleted only if it is still as it was read; otherwise `conflict`.
    public func deleteNote(id: String, ifStamp: String? = nil) async throws -> DeletedDTO {
        try validate(id)
        return try await send("DELETE", route: ["notes", id], query: ifStamp.map { [URLQueryItem(name: "ifStamp", value: $0)] } ?? [])
    }

    public func recording(noteID: String, command: Command) async throws -> NoteDTO {
        try validate(noteID)
        return try await send("POST", route: ["notes", noteID, "recording"], body: JSONEncoder().encode(RecordingCommandDTO(command: command)))
    }

    public func generate(noteID: String, expectedTranscriptRevision: Int) async throws -> NoteDTO {
        try validate(noteID)
        return try await send("POST", route: ["notes", noteID, "generate"], body: JSONEncoder().encode(GenerateNotesCommandDTO(expectedTranscriptRevision: expectedTranscriptRevision)), timeout: 240)
    }

    /// A model this build cannot read (a provider added on a newer server) is left out instead of failing the list.
    public func chatModels() async throws -> ChatModelsDTO {
        let list: ReadableChatModels = try await send("GET", route: ["models", "chat"])
        return ChatModelsDTO(models: list.models)
    }

    public func chat(noteID: String, message: String, history: [ChatMessageDTO], model: String? = nil) async throws -> ChatAnswerDTO {
        try validate(noteID)
        return try await send("POST", route: ["notes", noteID, "chat"], body: JSONEncoder().encode(ChatInput(history: history, includeSources: false, message: message, model: model, timeZone: TimeZone.current.identifier)), timeout: 240)
    }

    /// Actual SSE deltas from the instance. Consumer cancellation cancels the
    /// URLSession task, which closes the stream rather than inventing tokens.
    /// Compatibility text stream. Native chat consumes chatEvents so tool and
    /// citation metadata remain attached to the actual assistant turn.
    public func chatStream(noteID: String, message: String, history: [ChatMessageDTO]) throws -> AsyncThrowingStream<String, Error> {
        let events = try streamEvents(noteID: noteID, message: message, history: history, includeSources: false)
        return AsyncThrowingStream { continuation in
            let task = Task {
                do {
                    for try await event in events where event.type == .delta { continuation.yield(event.text) }
                    continuation.finish()
                } catch { continuation.finish(throwing: error) }
            }
            continuation.onTermination = { _ in task.cancel() }
        }
    }

    public func chatEvents(noteID: String?, message: String, history: [ChatMessageDTO], model: String? = nil) throws -> AsyncThrowingStream<ChatStreamEventDTO, Error> {
        try streamEvents(noteID: noteID, message: message, history: history, includeSources: true, model: model)
    }

    private func streamEvents(noteID: String?, message: String, history: [ChatMessageDTO], includeSources: Bool, model: String? = nil) throws -> AsyncThrowingStream<ChatStreamEventDTO, Error> {
        try transportState.check()
        if let noteID { try validate(noteID) }
        let route = ["api", "v1"] + (noteID.map { ["notes", $0] } ?? []) + ["chat", "stream"]
        return try eventStream(route: route, body: JSONEncoder().encode(ChatInput(history: history, includeSources: includeSources, message: message, model: model, timeZone: TimeZone.current.identifier)))
    }

    // MARK: Saved chats (`/api/v1/chats`): the signed-in person's own threads, on every device.

    /// Threads newest first. `noteID` limits them to one note's, `workspace` to those across all notes.
    public func chatThreads(noteID: String? = nil, workspace: Bool = false, limit: Int = 20, cursor: String? = nil) async throws -> ChatThreadsPageDTO {
        if let noteID { try validate(noteID) }
        var query = try Self.page(limit: limit, cursor: cursor)
        if let noteID { query.append(URLQueryItem(name: "noteId", value: noteID.lowercased())) }
        else if workspace { query.append(URLQueryItem(name: "scope", value: "workspace")) }
        return try await send("GET", route: ["chats"], query: query)
    }

    public func chatThread(id: String) async throws -> ChatThreadDTO {
        try validate(id)
        return try await send("GET", route: ["chats", id.lowercased()])
    }

    /// Idempotent with an `id`: the same id again answers the thread as it is.
    public func createChatThread(_ input: CreateChatThreadInput) async throws -> ChatThreadDTO {
        try await send("POST", route: ["chats"], body: JSONEncoder().encode(input))
    }

    /// Store an exchange this device already has. The same exchange again changes nothing.
    public func appendChatTurn(threadID: String, _ input: AppendChatTurnInput) async throws -> ChatThreadDTO {
        try validate(threadID)
        return try await send("POST", route: ["chats", threadID.lowercased(), "turns"], body: JSONEncoder().encode(input))
    }

    public func renameChatThread(id: String, title: String) async throws -> ChatThreadDTO {
        try validate(id)
        return try await send("PATCH", route: ["chats", id.lowercased()], body: JSONEncoder().encode(UpdateChatThreadInput(title: title)))
    }

    public func deleteChatThread(id: String) async throws -> DeletedDTO {
        try validate(id)
        return try await send("DELETE", route: ["chats", id.lowercased()])
    }

    /// Ask exchange `input.turn` of a saved thread. The server saves the question and the answer as it arrives.
    public func chatThreadEvents(threadID: String, _ input: ChatThreadStreamInput) throws -> AsyncThrowingStream<ChatStreamEventDTO, Error> {
        try transportState.check()
        try validate(threadID)
        return try eventStream(route: ["api", "v1", "chats", threadID.lowercased(), "stream"], body: JSONEncoder().encode(input))
    }

    private func eventStream(route: [String], body: Data) throws -> AsyncThrowingStream<ChatStreamEventDTO, Error> {
        var request = URLRequest(url: credentials.instance.endpoint(route))
        request.httpMethod = "POST"
        request.httpBody = body
        request.timeoutInterval = 240
        request.setValue("text/event-stream", forHTTPHeaderField: "Accept")
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.setValue("Bearer \(credentials.bearerToken)", forHTTPHeaderField: "Authorization")
        let requestToSend = request
        return AsyncThrowingStream { continuation in
            let task = Task {
                do {
                    try transportState.check()
                    try Task.checkCancellation()
                    let (bytes, response) = try await session.bytes(for: requestToSend)
                    guard let response = response as? HTTPURLResponse else { throw APIClientError.invalidResponse }
                    switch response.statusCode {
                    case 200..<300: break
                    case 401, 403: onUnauthorized?(); throw APIClientError.unauthorized
                    case 404: throw APIClientError.notFound
                    case 409: throw APIClientError.conflict
                    case 503: throw APIClientError.unavailable
                    default: throw APIClientError.httpStatus(response.statusCode)
                    }
                    guard response.value(forHTTPHeaderField: "Content-Type")?.lowercased().contains("text/event-stream") == true else { throw APIClientError.invalidResponse }
                    var total = 0
                    var metadataBytes = 0
                    var completed = false
                    for try await line in bytes.lines {
                        try transportState.check()
                        try Task.checkCancellation()
                        guard line.hasPrefix("data:") else { continue }
                        guard line.utf8.count <= 1_000_000 else { throw APIClientError.invalidResponse }
                        let payload = String(line.dropFirst(5)).trimmingCharacters(in: .whitespaces)
                        let event: ChatStreamEventDTO
                        do { event = try JSONDecoder().decode(ChatStreamEventDTO.self, from: Data(payload.utf8)) }
                        catch { throw APIClientError.invalidJSON }
                        switch event.type {
                        case .delta:
                            total += event.text.utf8.count
                            guard total <= 1_000_000 else { throw APIClientError.invalidResponse }

                        case .tool:
                            guard let tool = event.tool, UUID(uuidString: tool.callId) != nil, tool.label.count <= 200,
                                  (tool.results?.count ?? 0) <= 20, (tool.resultCount ?? 0) >= 0,
                                  tool.results?.allSatisfy({ UUID(uuidString: $0.noteId) != nil && $0.title.count <= 500 }) ?? true else { throw APIClientError.invalidJSON }
                            metadataBytes += payload.utf8.count
                        case .citations:
                            guard let citations = event.citations, citations.count <= 40, Set(citations.map(\.id)).count == citations.count,
                                  citations.allSatisfy({ $0.id.range(of: "^S[1-9][0-9]{0,2}$", options: .regularExpression) != nil && UUID(uuidString: $0.noteId) != nil && $0.transcriptRevision >= 0 && ($0.startMs ?? 0) >= 0 && $0.title.count <= 500 && ($0.segmentId.map { UUID(uuidString: $0) != nil } ?? true) && !$0.quote.isEmpty && $0.quote.count <= 20_000 }) else { throw APIClientError.invalidJSON }
                            metadataBytes += payload.utf8.count
                        // The model's reasoning is not shown here yet: the event is read and passed over.
                        case .reasoning: continue
                        case .done: completed = true
                        case .error: throw APIClientError.unavailable
                        }
                        guard metadataBytes <= 2_000_000 else { throw APIClientError.invalidResponse }
                        continuation.yield(event)
                        if completed { break }
                    }
                    guard completed else { throw APIClientError.invalidResponse }
                    continuation.finish()
                } catch {
                    if Task.isCancelled || (error as? URLError)?.code == .cancelled {
                        continuation.finish(throwing: CancellationError())
                    } else if let transport = error as? URLError {
                        continuation.finish(throwing: APIClientError.transport(transport.code))
                    } else { continuation.finish(throwing: error) }
                }
            }
            continuation.onTermination = { _ in task.cancel() }
        }
    }

    public func shares(noteID: String) async throws -> [ShareDTO] {
        try validate(noteID)
        return try await send("GET", route: ["notes", noteID, "shares"])
    }

    public func createShare(noteID: String, _ input: CreateShareInput) async throws -> CreatedShareDTO {
        try validate(noteID)
        return try await send("POST", route: ["notes", noteID, "shares"], body: JSONEncoder().encode(input))
    }

    public func revokeShare(id: String) async throws -> ShareDTO {
        try validate(id)
        return try await send("DELETE", route: ["shares", id])
    }

    /// Reconstruct the capability link on the selected instance, never on a
    /// document-supplied origin or arbitrary response path.
    public func shareURL(_ created: CreatedShareDTO) throws -> URL {
        guard created.token.range(of: "^[A-Za-z0-9_-]{43}$", options: .regularExpression) != nil,
              created.path == "/s/\(created.token)" else { throw APIClientError.invalidJSON }
        return credentials.instance.endpoint(["s", created.token])
    }

    /// A snapshot that someone with an account on this server shared. The
    /// session goes to this server and to no other: a link that names any
    /// other origin is refused here, before there is a request to carry it,
    /// and the request is built on the signed-in instance, never on the link.
    public func sharedNote(_ link: SharedNoteLink) async throws -> SharedNote {
        try transportState.check()
        try Task.checkCancellation()
        guard link.origin.isInstance(credentials.instance) else { throw SharedNoteError.signInRequired }
        var request = URLRequest(url: credentials.instance.endpoint(["api", "v1", "shared", link.token]))
        request.httpMethod = "GET"
        request.timeoutInterval = 20
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        request.setValue("Bearer \(credentials.bearerToken)", forHTTPHeaderField: "Authorization")
        let note: SharedNote
        do { note = try await SharedNoteReader.read(request, session: session) }
        catch SharedNoteError.signInRequired { onUnauthorized?(); throw SharedNoteError.signInRequired }
        try transportState.check()
        return note
    }

    private func validate(_ id: String) throws {
        guard UUID(uuidString: id) != nil else { throw APIClientError.invalidIdentifier }
    }

    private func send<Output: Decodable & Sendable>(
        _ method: String, route: [String], body: Data? = nil, timeout: TimeInterval = 30, query: [URLQueryItem] = []
    ) async throws -> Output {
        try transportState.check()
        try Task.checkCancellation()
        var components = URLComponents(url: credentials.instance.endpoint(["api", "v1"] + route), resolvingAgainstBaseURL: false)
        if !query.isEmpty { components?.queryItems = query }
        guard let url = components?.url else { throw APIClientError.invalidResponse }
        var request = URLRequest(url: url)
        request.httpMethod = method
        request.httpBody = body
        request.timeoutInterval = timeout
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        request.setValue("Bearer \(credentials.bearerToken)", forHTTPHeaderField: "Authorization")
        if body != nil { request.setValue("application/json", forHTTPHeaderField: "Content-Type") }
        let data: Data
        let response: URLResponse
        do {
            (data, response) = try await session.data(for: request)
        } catch let error as URLError {
            if Task.isCancelled || error.code == .cancelled { throw CancellationError() }
            throw APIClientError.transport(error.code)
        }
        try transportState.check()
        try Task.checkCancellation()
        guard let response = response as? HTTPURLResponse else { throw APIClientError.invalidResponse }
        switch response.statusCode {
        case 200..<300: break
        case 401, 403: onUnauthorized?(); throw APIClientError.unauthorized
        case 404: throw APIClientError.notFound
        case 409: throw APIClientError.conflict
        case 503: throw APIClientError.unavailable
        default: throw APIClientError.httpStatus(response.statusCode)
        }
        do { return try JSONDecoder().decode(Output.self, from: data) }
        catch { throw APIClientError.invalidJSON }
    }
}


final class RejectRedirects: NSObject, URLSessionTaskDelegate, Sendable {
    func urlSession(
        _ session: URLSession, task: URLSessionTask,
        willPerformHTTPRedirection response: HTTPURLResponse,
        newRequest request: URLRequest,
        completionHandler: @escaping @Sendable (URLRequest?) -> Void
    ) { completionHandler(nil) }
}

/// Closing a client is separate from invalidating URLSession: async-let children
/// may still be entering a request when an account switch cancels their parent.
/// Creating a task on an invalidated URLSession raises an Objective-C exception.
private final class TransportState: @unchecked Sendable {
    private let lock = NSLock()
    private var closed = false
    private let session: URLSession
    init(session: URLSession) { self.session = session }
    deinit { session.invalidateAndCancel() }
    func close() { lock.lock(); defer { lock.unlock() }; closed = true }
    func check() throws {
        lock.lock(); defer { lock.unlock() }
        if closed { throw CancellationError() }
    }
}


// Organization membership is independent from instance administration and private notes.
extension MeetingNotesAPI {
    public func organizations() async throws -> OrganizationsDTO { try await send("GET", route: ["organizations"]) }
    public func organization(id: String) async throws -> OrganizationDetailsDTO {
        try validate(id); return try await send("GET", route: ["organizations", id])
    }
    public func createOrganization(_ input: CreateOrganizationInput) async throws -> OrganizationDTO {
        try await send("POST", route: ["organizations"], body: JSONEncoder().encode(input))
    }
    public func updateOrganization(id: String, _ input: UpdateOrganizationInput) async throws -> OrganizationDTO {
        try validate(id); return try await send("PATCH", route: ["organizations", id], body: JSONEncoder().encode(input))
    }
    public func setActiveOrganization(id: String) async throws -> OrganizationsDTO {
        try validate(id); return try await send("POST", route: ["organizations", "active"], body: JSONEncoder().encode(SetActiveOrganizationInput(organizationId: id)))
    }
    public func inviteOrganizationMember(id: String, _ input: InviteOrganizationMemberInput) async throws -> OrganizationInvitationDTO {
        try validate(id); return try await send("POST", route: ["organizations", id, "invitations"], body: JSONEncoder().encode(input))
    }
    public func cancelOrganizationInvitation(id: String, invitationID: String) async throws -> DeletedDTO {
        try validate(id); try validate(invitationID)
        return try await send("DELETE", route: ["organizations", id, "invitations", invitationID])
    }
    public func updateOrganizationMember(id: String, memberID: String, _ input: UpdateOrganizationMemberRoleInput) async throws -> OrganizationMemberDTO {
        try validate(id); try validate(memberID)
        return try await send("PATCH", route: ["organizations", id, "members", memberID], body: JSONEncoder().encode(input))
    }
    public func removeOrganizationMember(id: String, memberID: String) async throws -> DeletedDTO {
        try validate(id); try validate(memberID)
        return try await send("DELETE", route: ["organizations", id, "members", memberID])
    }
    public func organizationInvitations() async throws -> OrganizationInvitationsDTO {
        try await send("GET", route: ["organization-invitations"])
    }
    public func acceptOrganizationInvitation(id: String) async throws -> OrganizationDTO {
        try validate(id); return try await send("POST", route: ["organization-invitations", id, "accept"])
    }
    public func rejectOrganizationInvitation(id: String) async throws -> DeletedDTO {
        try validate(id); return try await send("POST", route: ["organization-invitations", id, "reject"])
    }
}

// Live transcription is a WebSocket beside the HTTP API, authenticated by the same bearer.
extension MeetingNotesAPI {
    /// The upgrade request for a note's live socket. `http` instances become
    /// `ws`, `https` become `wss`; nothing else about the origin changes.
    func liveTranscriptRequest(noteID: String) throws -> URLRequest {
        try validate(noteID)
        let endpoint = credentials.instance.endpoint(["api", "v1", "notes", noteID, "live"])
        guard var components = URLComponents(url: endpoint, resolvingAgainstBaseURL: false) else { throw APIClientError.invalidInstanceURL }
        components.scheme = components.scheme == "http" ? "ws" : "wss"
        guard let url = components.url else { throw APIClientError.invalidInstanceURL }
        var request = URLRequest(url: url)
        request.setValue("Bearer \(credentials.bearerToken)", forHTTPHeaderField: "Authorization")
        return request
    }

    /// Streams `source` to the note's live transcription socket. The client
    /// does nothing until it is started.
    public func liveTranscriptClient(noteID: String, source: any LiveAudioSource,
                                     configuration: LiveTranscriptClient.Configuration) throws -> LiveTranscriptClient {
        try transportState.check()
        return LiveTranscriptClient(connector: URLSessionLiveConnector(request: try liveTranscriptRequest(noteID: noteID)),
                                    source: source, configuration: configuration)
    }
}

/// `ChatModelsDTO` read one model at a time, keeping the ones whose provider this build knows.
struct ReadableChatModels: Decodable, Sendable {
    let models: [LanguageModelSelection]
    private enum CodingKeys: String, CodingKey { case models }
    private struct Entry: Decodable { let model: LanguageModelSelection?
        init(from decoder: Decoder) throws { model = try? LanguageModelSelection(from: decoder) } }
    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        models = try container.decode([Entry].self, forKey: .models).compactMap(\.model)
    }
}
