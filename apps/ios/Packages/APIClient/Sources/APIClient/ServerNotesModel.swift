import Foundation
import Observation
import Contracts

public enum ServerNotesState: Sendable {
    case disconnected
    case loading(previous: [NoteDTO])
    case loaded([NoteDTO])
    case failed(APIClientError, previous: [NoteDTO])

    public var notes: [NoteDTO] {
        switch self {
        case .disconnected: []
        case .loading(let notes), .loaded(let notes), .failed(_, let notes): notes
        }
    }
}

public struct ServerCalendarSnapshot: Sendable {
    public var calendars: [CalendarDTO]
    public var events: [CalendarEventDTO]
    public var personalCalendarsLinked: Bool
    public var personalCalendarIssue: Bool
    public init(calendars: [CalendarDTO] = [], events: [CalendarEventDTO] = [], personalCalendarsLinked: Bool = false, personalCalendarIssue: Bool = false) {
        self.calendars = calendars; self.events = events
        self.personalCalendarsLinked = personalCalendarsLinked; self.personalCalendarIssue = personalCalendarIssue
    }
}

public enum ServerCalendarState: Sendable {
    case disconnected
    case loading(previous: ServerCalendarSnapshot)
    case loaded(ServerCalendarSnapshot)
    case failed(APIClientError, previous: ServerCalendarSnapshot)

    public var snapshot: ServerCalendarSnapshot {
        switch self {
        case .disconnected: .init()
        case .loading(let snapshot), .loaded(let snapshot), .failed(_, let snapshot): snapshot
        }
    }
}

/// One account's notes, as its server has them. The model holds one identity
/// and is the only way anything is asked of that account's server; it keeps no
/// session itself (the account's Keychain item does) and never seeds demo notes
/// or writes a second local source of truth.
@MainActor @Observable
public final class ServerNotesModel {
    public private(set) var state: ServerNotesState = .disconnected
    public private(set) var instance: InstanceURL?
    public private(set) var session: AuthSessionDTO?
    /// Nil until a session is being checked, and again once the account has gone.
    public private(set) var status: AccountStatus?
    /// Counts the sessions this model has taken up: the first, and each replacement.
    public private(set) var sessionGeneration = 0
    public let workspaces = ServerWorkspacesModel()
    /// The server's user id, hyphenated and lower case.
    public var principalID: String? { session.flatMap { AccountID.canonicalUserID($0.user.id) } }
    public var accountIdentity: String? {
        guard let instance, let id = principalID else { return nil }
        return instance.description + "|" + id
    }
    public var accountID: AccountID? {
        guard let instance, let id = principalID else { return nil }
        return AccountID(address: instance.description, userID: id)
    }
    public var canAdminister: Bool { session?.user.role == .owner || session?.user.role == .admin }
    public private(set) var calendarState: ServerCalendarState = .disconnected
    /// The Home list: rows, a page at a time. `state.notes` holds only the whole
    /// notes this session has read or changed, never every note on the server.
    public private(set) var list = ServerNoteList()
    /// The client of the session in use. It holds the token privately and builds every URL from its own address.
    @ObservationIgnored private(set) var connection: MeetingNotesAPI?
    /// The client requests go through. There is none while the session is known to be refused: nothing is sent.
    private var api: MeetingNotesAPI? { status == .needsSignIn ? nil : connection }
    @ObservationIgnored private var requestRevision: UInt64 = 0
    /// The account's epoch: it moves on sign-out, removal, a replaced token and a
    /// refused session. An answer to a request made before it moved is dropped.
    @ObservationIgnored private var connectionRevision: UInt64 = 0
    @ObservationIgnored private var calendarRequestRevision: UInt64 = 0
    @ObservationIgnored private var sessionCheck: Task<Void, Never>?
    /// True on a server from before the list endpoint: every note is then loaded whole, as this app used to.
    @ObservationIgnored private var wholeNoteList = false
    /// Times this device has sent a title for each note. Not secret, not saved.
    @ObservationIgnored private var titleEdits: [String: Int] = [:]
    /// Changes whenever the rows are replaced, so a page asked for before that is dropped.
    @ObservationIgnored private var listRevision: UInt64 = 0
    /// Whole notes kept for reopening. A row never holds a note's text.
    private static let notesKept = 24
    @ObservationIgnored private let makeAPI: @Sendable (InstanceCredentials) -> MeetingNotesAPI

    public init(makeAPI: @escaping @Sendable (InstanceCredentials) -> MeetingNotesAPI = { MeetingNotesAPI(credentials: $0) }) {
        self.makeAPI = makeAPI
    }

    /// A session its own server has confirmed, with the first page of its notes.
    public struct Verified: Sendable {
        let api: MeetingNotesAPI
        public let credentials: InstanceCredentials
        public let identity: AuthSessionDTO
        /// The user's id, hyphenated and lower case.
        public let userID: String
        fileprivate let first: FirstPage

        init(api: MeetingNotesAPI, credentials: InstanceCredentials, identity: AuthSessionDTO, userID: String, first: Verified) {
            self.api = api; self.credentials = credentials; self.identity = identity; self.userID = userID; self.first = first.first
        }

        fileprivate init(api: MeetingNotesAPI, credentials: InstanceCredentials, identity: AuthSessionDTO, userID: String, page: FirstPage) {
            self.api = api; self.credentials = credentials; self.identity = identity; self.userID = userID; self.first = page
        }
    }

    public enum VerifyFailure: Error, Sendable {
        /// `/session` itself refused the session, in the API's own words.
        case refused
        case failed(APIClientError)
    }

    public enum ConnectionOutcome: Sendable {
        case ready(Verified)
        case needsSignIn
        /// The session is somebody else's. It was not taken up.
        case otherUser(AuthSessionDTO)
        case failed(APIClientError)
        /// Something else happened to the connection while this was being checked.
        case superseded
    }

    /// Resolves the stable server identity before any private data is shown,
    /// and reads the first page of notes. Nothing of the model changes.
    public static func verify(_ credentials: InstanceCredentials,
                              makeAPI: @Sendable (InstanceCredentials) -> MeetingNotesAPI) async throws -> Verified {
        let candidate = makeAPI(credentials)
        let identity: AuthSessionDTO
        switch await candidate.verifySession() {
        case .valid(let value): identity = value
        case .refused: throw VerifyFailure.refused
        case .noAnswer(let error): throw VerifyFailure.failed(error)
        }
        guard let userID = AccountID.canonicalUserID(identity.user.id) else { throw VerifyFailure.failed(.invalidResponse) }
        do {
            let first = try await Self.firstPage(candidate)
            return Verified(api: candidate, credentials: credentials, identity: identity, userID: userID, page: first)
        } catch is CancellationError { throw VerifyFailure.failed(.transport(.cancelled))
        } catch { throw VerifyFailure.failed(error as? APIClientError ?? .invalidResponse) }
    }

    /// Checks a saved or entered session with its server and takes it up. With
    /// `userID`, a session for anybody else is left alone. `accept` runs before
    /// any note is shown, so what must be saved first is saved first.
    @discardableResult
    public func connect(_ credentials: InstanceCredentials, expecting userID: String? = nil,
                        accept: (Verified) throws -> Void = { _ in }) async -> ConnectionOutcome {
        connectionRevision &+= 1
        let revision = connectionRevision
        let sameAccount = session != nil && status != nil
        if !sameAccount {
            status = .checking
            state = .loading(previous: [])
        }
        do {
            let verified = try await Self.verify(credentials, makeAPI: makeAPI)
            guard revision == connectionRevision else { verified.api.cancelRequests(); return .superseded }
            if let userID, verified.userID != userID {
                verified.api.cancelRequests()
                return .otherUser(verified.identity)
            }
            do { try accept(verified) }
            catch {
                verified.api.cancelRequests()
                return fail(error as? APIClientError ?? .secureStorage, keeping: sameAccount)
            }
            adopt(verified)
            return .ready(verified)
        } catch VerifyFailure.refused {
            guard revision == connectionRevision else { return .superseded }
            markNeedsSignIn(instance: credentials.instance)
            return .needsSignIn
        } catch {
            guard revision == connectionRevision else { return .superseded }
            if case VerifyFailure.failed(let failure) = error { return fail(failure, keeping: sameAccount) }
            return fail(.invalidResponse, keeping: sameAccount)
        }
    }

    private func fail(_ error: APIClientError, keeping: Bool) -> ConnectionOutcome {
        if !keeping { markUnreachable(error) }
        return .failed(error)
    }

    /// Takes up a confirmed session. For the account this model already
    /// shows, the token is replaced and what is on screen stays; work that the
    /// old session's end cancelled is queued again by whoever watches `sessionGeneration`.
    func adopt(_ verified: Verified) {
        let same = accountIdentity != nil && principalID == verified.userID
            && instance == verified.credentials.instance
        connectionRevision &+= 1
        requestRevision &+= 1
        calendarRequestRevision &+= 1
        sessionCheck?.cancel(); sessionCheck = nil
        connection?.cancelRequests()
        let kept = same ? state.notes : []
        if !same {
            calendarState = .disconnected
            titleEdits = [:]
        }
        instance = verified.credentials.instance
        session = verified.identity
        let client = verified.api.observingUnauthorized { [weak self] in
            Task { @MainActor in self?.requestWasRefused() }
        }
        connection = client
        status = .ready
        workspaces.attach(client)
        show(verified.first, keeping: kept)
        sessionGeneration += 1
    }

    /// The account's server has not answered in this launch. No notes are shown.
    func markUnreachable(_ error: APIClientError) {
        status = .unreachable(error)
        state = .failed(error, previous: [])
    }

    /// The session is refused, or does not belong to this account. Nothing is
    /// sent until the person signs in again. What is on screen stays: a note
    /// that is open, and a recording in progress, are never hidden or stopped.
    func markNeedsSignIn(instance address: InstanceURL?) {
        connectionRevision &+= 1
        requestRevision &+= 1
        calendarRequestRevision &+= 1
        sessionCheck?.cancel(); sessionCheck = nil
        connection?.cancelRequests()
        if instance == nil { instance = address }
        status = .needsSignIn
        state = .failed(.unauthorized, previous: state.notes)
    }

    /// A request was answered 401 or 403. That changes nothing by itself: the
    /// account's own `/session` is asked once, and only its refusal, in the
    /// API's own JSON, ends the session here.
    private func requestWasRefused() {
        guard status == .ready, sessionCheck == nil, let connection else { return }
        let revision = connectionRevision
        sessionCheck = Task { [weak self] in
            let verdict = await connection.verifySession()
            guard let self, revision == self.connectionRevision else { return }
            self.sessionCheck = nil
            if case .refused = verdict { self.markNeedsSignIn(instance: self.instance) }
        }
    }

    /// Ends this model's part in the account: sign-out, or removal. Returns the
    /// client that was in use, so its session can be ended on its own server.
    @discardableResult
    func close() -> MeetingNotesAPI? {
        let previous = connection
        resetConnection()
        return previous
    }

    private func resetConnection() {
        connectionRevision &+= 1
        requestRevision &+= 1
        calendarRequestRevision &+= 1
        sessionCheck?.cancel(); sessionCheck = nil
        connection?.cancelRequests()
        connection = nil; instance = nil; session = nil; status = nil
        workspaces.attach(nil)
        state = .disconnected; calendarState = .disconnected
        list = ServerNoteList(); wholeNoteList = false; listRevision &+= 1
        titleEdits = [:]
    }

    fileprivate struct FirstPage: Sendable {
        var rows: [ServerNoteRow]; var total: Int; var nextCursor: String?
        /// Set only by a server that has no list endpoint.
        var wholeNotes: [NoteDTO]?
    }

    /// The first page of rows. A server from before the list endpoint reads that
    /// route as a note ID and refuses it; it is then asked for every note, whole.
    private static func firstPage(_ api: MeetingNotesAPI) async throws -> FirstPage {
        do {
            let page = try await api.noteSummaries()
            return FirstPage(rows: page.notes.map { ServerNoteRow($0) }, total: page.total, nextCursor: page.nextCursor, wholeNotes: nil)
        } catch let error as APIClientError where error == .notFound || error == .httpStatus(400) {
            let notes = try await api.listNotes()
            return FirstPage(rows: notes.map { ServerNoteRow($0) }.sorted(by: ServerNoteRow.precedes), total: notes.count, nextCursor: nil, wholeNotes: notes)
        }
    }

    /// `kept` are whole notes from before a refresh. One whose row has since
    /// changed on the server is out of date and is dropped; the note page reads it again.
    private func show(_ first: FirstPage, keeping kept: [NoteDTO]) {
        wholeNoteList = first.wholeNotes != nil
        listRevision &+= 1
        list.replace(rows: first.rows, total: first.total, nextCursor: first.nextCursor)
        if let notes = first.wholeNotes { state = .loaded(notes); return }
        let current = Dictionary(first.rows.map { ($0.id, $0.updatedAt) }, uniquingKeysWith: { first, _ in first })
        state = .loaded(kept.filter { note in current[note.id].map { $0 == note.updatedAt } ?? true })
    }

    /// The next page of rows. Does nothing while one is on its way or when there is none.
    public func loadMore() async {
        guard let api, let cursor = list.nextCursor, !list.isLoadingMore else { return }
        let revision = listRevision
        list.isLoadingMore = true; list.moreError = nil
        do {
            let page = try await api.noteSummaries(cursor: cursor)
            guard revision == listRevision else { return }
            list.append(rows: page.notes.map { ServerNoteRow($0) }, total: page.total, nextCursor: page.nextCursor)
        } catch {
            guard revision == listRevision else { return }
            list.isLoadingMore = false
            if !(error is CancellationError) { list.moreError = error as? APIClientError ?? .invalidResponse }
        }
    }

    /// One page of search results, best match first. A server without the search
    /// endpoint is searched through the whole notes this app already holds.
    public func search(query: String, cursor: String?) async throws -> ServerNoteSearchPage {
        guard let api else { throw APIClientError.unauthorized }
        if wholeNoteList { return .local(state.notes, query: query) }
        let revision = connectionRevision
        let page = try await api.searchNotes(query: query, cursor: cursor)
        guard revision == connectionRevision else { throw CancellationError() }
        return ServerNoteSearchPage(hits: page.results.map { ServerNoteSearchHit($0) }, total: page.total, nextCursor: page.nextCursor)
    }

    /// A row's title, or a whole note's when the row is not loaded.
    public func title(ofNote id: String) -> String? {
        list.rows.first { $0.id == id }?.title ?? state.notes.first { $0.id == id }?.title
    }

    /// Replaces the whole notes held without changing whether the list is loading, loaded or failed.
    private func hold(_ notes: [NoteDTO]) {
        switch state {
        case .disconnected: break
        case .loading: state = .loading(previous: notes)
        case .loaded: state = .loaded(notes)
        case .failed(let error, _): state = .failed(error, previous: notes)
        }
    }

    /// Keeps a whole note for reopening, newest first, and brings its row up to date.
    private func remember(_ note: NoteDTO, insertRow: Bool) {
        let others = state.notes.filter { $0.id != note.id }
        hold([note] + (wholeNoteList ? others : Array(others.prefix(Self.notesKept - 1))))
        list.upsert(note, insertIfMissing: insertRow)
    }

    public func refresh() async {
        // An account whose session is refused or unconfirmed keeps saying so; there is nothing to ask.
        guard let api else { if status == nil { state = .disconnected }; return }
        requestRevision &+= 1
        let revision = requestRevision
        let previous = state.notes
        state = .loading(previous: previous)
        do {
            let first = try await Self.firstPage(api)
            try Task.checkCancellation()
            guard requestRevision == revision else { return }
            show(first, keeping: previous)
        } catch is CancellationError {
            guard requestRevision == revision else { return }
            state = .loaded(previous)
        } catch {
            guard requestRevision == revision else { return }
            state = .failed(error as? APIClientError ?? .invalidResponse, previous: previous)
        }
    }

    public func note(id: String) async throws -> NoteDTO {
        guard let api else { throw APIClientError.unauthorized }
        let revision = connectionRevision
        let note = try await api.note(id: id)
        guard revision == connectionRevision else { throw CancellationError() }
        // Reading a note is not a change to the list: its row is refreshed if it is loaded, never added.
        remember(note, insertRow: false)
        return note
    }

    /// Reads a note for a caller that keeps its own copy, such as the recorder
    /// following a transcript as it grows. Unlike `note(id:)` it changes
    /// nothing that is shared: not the notes kept for reopening and not the
    /// list. A read like that happens every few seconds during a meeting, and
    /// each change to the shared state redraws every screen that shows the list.
    public func noteSnapshot(id: String) async throws -> NoteDTO {
        guard let api else { throw APIClientError.unauthorized }
        let revision = connectionRevision
        let note = try await api.note(id: id)
        guard revision == connectionRevision else { throw CancellationError() }
        return note
    }

    public func refreshCalendars(from: Date, to: Date) async {
        guard let api else { calendarState = .disconnected; return }
        calendarRequestRevision &+= 1
        let request = calendarRequestRevision
        let connection = connectionRevision
        let previous = calendarState.snapshot
        calendarState = .loading(previous: previous)
        do {
            async let calendars = api.calendars()
            async let events = api.calendarEvents(from: from, to: to)
            async let personal = api.googleStatus()
            let status = try await personal
            let snapshot = try await ServerCalendarSnapshot(calendars: calendars, events: events, personalCalendarsLinked: !status.connections.isEmpty, personalCalendarIssue: status.connections.contains { $0.status == .reconnectRequired })
            try Task.checkCancellation()
            guard request == calendarRequestRevision, connection == connectionRevision else { return }
            calendarState = .loaded(snapshot)
        } catch is CancellationError {
            guard request == calendarRequestRevision, connection == connectionRevision else { return }
            calendarState = .loaded(previous)
        } catch {
            guard request == calendarRequestRevision, connection == connectionRevision else { return }
            calendarState = .failed(error as? APIClientError ?? .invalidResponse, previous: previous)
        }
    }

    /// The version this account's server is, and where its apps are.
    public func downloads() async throws -> DownloadsDTO {
        guard let api else { throw APIClientError.unauthorized }
        return try await api.downloads()
    }

    public func noteForCalendarEvent(id: String) async throws -> NoteDTO {
        guard let api else { throw APIClientError.unauthorized }
        let revision = connectionRevision
        let note = try await api.noteForCalendarEvent(id: id)
        try accept(note, revision: revision)
        return note
    }

    public func noteContext(id: String) async throws -> NoteContextDTO {
        guard let api else { throw APIClientError.unauthorized }
        let revision = connectionRevision
        let context = try await api.noteContext(id: id)
        guard revision == connectionRevision else { throw CancellationError() }
        guard context.noteId == id else { throw APIClientError.invalidResponse }
        return context
    }

    public func updateNoteContext(id: String, eventID: String?) async throws -> NoteContextDTO {
        guard let api else { throw APIClientError.unauthorized }
        let revision = connectionRevision
        let context = try await api.updateNoteContext(id: id, eventID: eventID)
        guard revision == connectionRevision else { throw CancellationError() }
        guard context.noteId == id else { throw APIClientError.invalidResponse }
        return context
    }

    public func addNoteParticipant(id: String, input: ManualParticipantInput) async throws -> NoteContextDTO {
        guard let api else { throw APIClientError.unauthorized }
        let revision = connectionRevision
        let context = try await api.upsertNoteParticipant(id: id, input: input)
        guard revision == connectionRevision else { throw CancellationError() }
        guard context.noteId == id else { throw APIClientError.invalidResponse }
        return context
    }

    public func searchCalendarEvents(from: Date, to: Date) async throws -> [CalendarEventDTO] {
        guard let api else { throw APIClientError.unauthorized }
        let revision = connectionRevision
        let events = try await api.calendarEvents(from: from, to: to)
        guard revision == connectionRevision else { throw CancellationError() }
        return events
    }

    public func create(title: String, body: String) async throws -> NoteDTO {
        guard let api else { throw APIClientError.unauthorized }
        let revision = connectionRevision
        let note = try await api.createNote(CreateNoteInput(body: body, date: nil, duration: nil, enhanced: nil, title: title, transcriptSegments: nil))
        try accept(note, revision: revision)
        return note
    }

    public func update(id: String, title: String, body: String) async throws -> NoteDTO {
        try await update(id: id, UpdateNoteInput(body: body, date: nil, duration: nil, enhanced: nil, title: title, transcriptSegments: nil))
    }

    /// Accept the generated partial update directly, so an enhanced-note editor
    /// cannot overwrite unrelated typed notes or transcript lifecycle fields.
    public func update(id: String, _ input: UpdateNoteInput) async throws -> NoteDTO {
        guard let api else { throw APIClientError.unauthorized }
        let revision = connectionRevision
        // Counted before the request leaves: a generation that ends while it is
        // in flight must already know the person has named the note.
        if input.title != nil { titleEdits[id, default: 0] += 1 }
        let note = try await api.updateNote(id: id, input)
        try accept(note, revision: revision)
        return note
    }

    public func delete(id: String, ifStamp: String? = nil) async throws {
        guard let api else { throw APIClientError.unauthorized }
        let revision = connectionRevision
        let result = try await api.deleteNote(id: id, ifStamp: ifStamp)
        guard revision == connectionRevision else { throw CancellationError() }
        guard result.deleted else { throw APIClientError.invalidResponse }
        requestRevision &+= 1
        state = .loaded(state.notes.filter { $0.id != id })
        list.remove(id: id)
    }

    /// True when the app holds a session on the server a link names.
    public func isSignedIn(to origin: ServerOrigin) -> Bool {
        guard session != nil, status == .ready, let instance else { return false }
        return origin.isInstance(instance)
    }

    /// A note someone else on this server shared. Without a session on the
    /// link's own server nothing is asked of anyone.
    public func sharedNote(_ link: SharedNoteLink) async throws -> SharedNote {
        guard let api, isSignedIn(to: link.origin) else { throw SharedNoteError.signInRequired }
        let revision = connectionRevision
        let note = try await api.sharedNote(link)
        guard revision == connectionRevision else { throw CancellationError() }
        return note
    }

    public func shares(noteID: String) async throws -> [ShareDTO] {
        guard let api else { throw APIClientError.unauthorized }
        let revision = connectionRevision
        let shares = try await api.shares(noteID: noteID)
        guard revision == connectionRevision else { throw CancellationError() }
        return shares
    }

    public func createShare(noteID: String, includeBody: Bool, includeEnhanced: Bool, expiresAt: String?) async throws -> URL {
        guard let api else { throw APIClientError.unauthorized }
        let revision = connectionRevision
        let created = try await api.createShare(noteID: noteID, CreateShareInput(expiresAt: expiresAt, includeBody: includeBody, includeEnhanced: includeEnhanced))
        guard revision == connectionRevision else { throw CancellationError() }
        return try api.shareURL(created)
    }

    public func revokeShare(id: String) async throws {
        guard let api else { throw APIClientError.unauthorized }
        let revision = connectionRevision
        _ = try await api.revokeShare(id: id)
        guard revision == connectionRevision else { throw CancellationError() }
    }

    public func recording(noteID: String, command: Command) async throws -> NoteDTO {
        guard let api else { throw APIClientError.unauthorized }
        let revision = connectionRevision
        let note = try await api.recording(noteID: noteID, command: command)
        try accept(note, revision: revision)
        return note
    }

    /// Generating notes for a note without a title also gives it one, and the
    /// note that comes back carries it: the note page, the list and search show
    /// it from here on. A title the person typed on this device while the notes
    /// were being generated is theirs. The answer may have been written before
    /// their title arrived, so the note is read again rather than shown as answered.
    public func generate(noteID: String, expectedTranscriptRevision: Int) async throws -> NoteDTO {
        guard let api else { throw APIClientError.unauthorized }
        let revision = connectionRevision
        let titled = titleEdits[noteID]
        var note = try await api.generate(noteID: noteID, expectedTranscriptRevision: expectedTranscriptRevision)
        guard revision == connectionRevision else { throw CancellationError() }
        if titleEdits[noteID] != titled { note = try await api.note(id: noteID) }
        try accept(note, revision: revision)
        return note
    }

    /// The title to show for a row that came from somewhere other than the
    /// list (a search result): the newest one this device has seen for that
    /// note, so a note just renamed or given a title here is not shown under its old one.
    public func latestTitle(of row: ServerNoteRow) -> String {
        let known: [(title: String, updatedAt: String)] = [
            list.rows.first { $0.id == row.id }.map { ($0.title, $0.updatedAt) },
            state.notes.first { $0.id == row.id }.map { ($0.title, $0.updatedAt) }
        ].compactMap { $0 }
        let stamp = CalendarSchedule.timestamp(row.updatedAt) ?? .distantPast
        let newer = known.filter { (CalendarSchedule.timestamp($0.updatedAt) ?? .distantPast) > stamp }
        return newer.max { (CalendarSchedule.timestamp($0.updatedAt) ?? .distantPast) < (CalendarSchedule.timestamp($1.updatedAt) ?? .distantPast) }?.title ?? row.title
    }

    public func chat(noteID: String, message: String, history: [ChatMessageDTO]) async throws -> ChatAnswerDTO {
        guard let api else { throw APIClientError.unauthorized }
        let revision = connectionRevision
        let answer = try await api.chat(noteID: noteID, message: message, history: history)
        guard revision == connectionRevision else { throw CancellationError() }
        return answer
    }

    public func chatModels() async throws -> [LanguageModelSelection] {
        guard let api else { throw APIClientError.unauthorized }
        let revision = connectionRevision
        let result = try await api.chatModels()
        guard revision == connectionRevision else { throw CancellationError() }
        return result.models
    }

    public func chatEvents(noteID: String?, message: String, history: [ChatMessageDTO], model: String? = nil) throws -> AsyncThrowingStream<ChatStreamEventDTO, Error> {
        guard let api else { throw APIClientError.unauthorized }
        let revision = connectionRevision
        let stream = try api.chatEvents(noteID: noteID, message: message, history: history, model: model)
        return AsyncThrowingStream { continuation in
            let task = Task {
                do {
                    for try await event in stream {
                        try Task.checkCancellation()
                        guard revision == connectionRevision else { throw CancellationError() }
                        continuation.yield(event)
                    }
                    guard revision == connectionRevision else { throw CancellationError() }
                    continuation.finish()
                } catch { continuation.finish(throwing: error) }
            }
            continuation.onTermination = { _ in task.cancel() }
        }
    }

    // MARK: Saved chats

    public func chatThreads(noteID: String?, limit: Int = 20, cursor: String? = nil) async throws -> ChatThreadsPageDTO {
        guard let api else { throw APIClientError.unauthorized }
        let revision = connectionRevision
        let page = try await api.chatThreads(noteID: noteID, workspace: noteID == nil, limit: limit, cursor: cursor)
        guard revision == connectionRevision else { throw CancellationError() }
        return page
    }

    public func chatThread(id: String) async throws -> ChatThreadDTO {
        guard let api else { throw APIClientError.unauthorized }
        let revision = connectionRevision
        let thread = try await api.chatThread(id: id)
        guard revision == connectionRevision else { throw CancellationError() }
        return thread
    }

    public func renameChatThread(id: String, title: String) async throws -> ChatThreadDTO {
        guard let api else { throw APIClientError.unauthorized }
        return try await api.renameChatThread(id: id, title: title)
    }

    public func deleteChatThread(id: String) async throws {
        guard let api else { throw APIClientError.unauthorized }
        _ = try await api.deleteChatThread(id: id)
    }

    /// Ask a question in the saved thread `create.id`, creating the (empty) thread first if the server does not
    /// have it. A new question is the thread's next exchange on the server; with `replace`, exchange `turn` and
    /// the ones after it are replaced (a retry).
    public func chatThreadEvents(create: CreateChatThreadInput, turn: Int, replace: Bool, message: String, model: String? = nil) throws -> AsyncThrowingStream<ChatStreamEventDTO, Error> {
        guard let api, let id = create.id else { throw APIClientError.unauthorized }
        let revision = connectionRevision
        return AsyncThrowingStream { continuation in
            let task = Task { @MainActor in
                do {
                    let thread = try await api.createChatThread(create)
                    guard revision == self.connectionRevision else { throw CancellationError() }
                    let replacing = replace && turn < thread.turnCount
                    let stream = try api.chatThreadEvents(threadID: id, ChatThreadStreamInput(includeSources: true, message: message, model: model, replace: replacing ? true : nil, timeZone: TimeZone.current.identifier, turn: replacing ? turn : thread.turnCount))
                    for try await event in stream {
                        try Task.checkCancellation()
                        guard revision == self.connectionRevision else { throw CancellationError() }
                        continuation.yield(event)
                    }
                    guard revision == self.connectionRevision else { throw CancellationError() }
                    continuation.finish()
                } catch { continuation.finish(throwing: error) }
            }
            continuation.onTermination = { _ in task.cancel() }
        }
    }

    public func chatStream(noteID: String, message: String, history: [ChatMessageDTO]) throws -> AsyncThrowingStream<String, Error> {
        guard let api else { throw APIClientError.unauthorized }
        let revision = connectionRevision
        let stream = try api.chatStream(noteID: noteID, message: message, history: history)
        return AsyncThrowingStream { continuation in
            let task = Task {
                do {
                    for try await text in stream {
                        try Task.checkCancellation()
                        guard revision == connectionRevision else { throw CancellationError() }
                        continuation.yield(text)
                    }
                    guard revision == connectionRevision else { throw CancellationError() }
                    continuation.finish()
                } catch { continuation.finish(throwing: error) }
            }
            continuation.onTermination = { _ in task.cancel() }
        }
    }

    /// A live transcription stream for a note that is being recorded. It is
    /// bound to the connection that created it: the caller cancels it when the
    /// account changes.
    public func liveTranscriptClient(noteID: String, source: any LiveAudioSource,
                                     configuration: LiveTranscriptClient.Configuration) throws -> LiveTranscriptClient {
        guard let api else { throw APIClientError.unauthorized }
        return try api.liveTranscriptClient(noteID: noteID, source: source, configuration: configuration)
    }

    private func accept(_ note: NoteDTO, revision: UInt64) throws {
        guard revision == connectionRevision else { throw CancellationError() }
        requestRevision &+= 1
        state = .loaded(state.notes)
        remember(note, insertRow: true)
    }

    #if DEBUG
    /// Static QA states are labeled by the host app and contain no credentials,
    /// API transport or document data. This entry point is absent in Release.
    public func showQAState(_ state: ServerNotesState) {
        connectionRevision &+= 1
        requestRevision &+= 1
        connection = nil
        instance = nil
        self.session = nil
        status = .ready
        workspaces.attach(nil)
        self.calendarState = .disconnected
        self.state = state
        wholeNoteList = false; listRevision &+= 1
        list.replace(rows: state.notes.map { ServerNoteRow($0) }.sorted(by: ServerNoteRow.precedes), total: state.notes.count, nextCursor: nil)
    }
    #endif
}
