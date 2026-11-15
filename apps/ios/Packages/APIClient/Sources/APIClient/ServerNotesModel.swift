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

/// An opt-in foundation for the existing SwiftUI app. It never seeds demo notes,
/// writes a second local source of truth, or changes the existing Cereal store.
@MainActor @Observable
public final class ServerNotesModel {
    public private(set) var state: ServerNotesState = .disconnected
    public private(set) var instance: InstanceURL?
    public private(set) var session: AuthSessionDTO?
    public let workspaces = ServerWorkspacesModel()
    public var accountIdentity: String? {
        guard let instance, let id = session?.user.id else { return nil }
        return instance.description + "|" + id.lowercased()
    }
    public var principalID: String? { session?.user.id }
    public var canAdminister: Bool { session?.user.role == .owner || session?.user.role == .admin }
    public private(set) var calendarState: ServerCalendarState = .disconnected
    /// The Home list: rows, a page at a time. `state.notes` holds only the whole
    /// notes this session has read or changed, never every note on the server.
    public private(set) var list = ServerNoteList()
    @ObservationIgnored private let settings: ConnectionSettingsStore
    @ObservationIgnored private var api: MeetingNotesAPI?
    @ObservationIgnored private var requestRevision: UInt64 = 0
    @ObservationIgnored private var connectionRevision: UInt64 = 0
    @ObservationIgnored private var calendarRequestRevision: UInt64 = 0
    /// True on a server from before the list endpoint: every note is then loaded whole, as this app used to.
    @ObservationIgnored private var wholeNoteList = false
    /// Times this device has sent a title for each note. Not secret, not saved.
    @ObservationIgnored private var titleEdits: [String: Int] = [:]
    /// Changes whenever the rows are replaced, so a page asked for before that is dropped.
    @ObservationIgnored private var listRevision: UInt64 = 0
    /// Whole notes kept for reopening. A row never holds a note's text.
    private static let notesKept = 24
    @ObservationIgnored private let makeAPI: @Sendable (InstanceCredentials) -> MeetingNotesAPI

    public init(settings: ConnectionSettingsStore = ConnectionSettingsStore(), makeAPI: @escaping @Sendable (InstanceCredentials) -> MeetingNotesAPI = { MeetingNotesAPI(credentials: $0) }) {
        self.settings = settings
        self.makeAPI = makeAPI
    }

    public func restore() async {
        do {
            guard let credentials = try settings.load() else { return }
            await establish(credentials)
        } catch { state = .failed(.secureStorage, previous: []) }
    }

    public func signIn(instance text: String, email: String, password: String) async {
        resetConnection()
        let revision = connectionRevision
        state = .loading(previous: [])
        do {
            let credentials = try await MeetingNotesAPI.signIn(instance: InstanceURL(text), email: email, password: password)
            guard revision == connectionRevision else { return }
            await establish(credentials)
        } catch {
            guard revision == connectionRevision else { return }
            state = .failed(error as? APIClientError ?? .unauthorized, previous: [])
        }
    }

    /// Finishes a sign-in that was made in the browser. The current connection is
    /// kept until the server has handed over a session; that session is then
    /// checked and saved exactly as one from a password sign-in is.
    public func signIn(browser attempt: BrowserSignIn, callback: URL, configuration: URLSessionConfiguration = .ephemeral) async throws {
        let code = try attempt.code(from: callback)
        let credentials = try await attempt.exchange(code: code, configuration: configuration)
        await establish(credentials)
    }

    public func connect(instance text: String, token: String) async {
        do {
            let credentials = try settings.credentials(instance: InstanceURL(text), enteredToken: token)
            await establish(credentials)
        } catch {
            resetConnection()
            state = .failed(error as? APIClientError ?? .invalidResponse, previous: [])
        }
    }

    /// Resolve the stable server identity before loading any private data. A
    /// rejected replacement connection never keeps the former user's notes visible.
    private func establish(_ credentials: InstanceCredentials) async {
        resetConnection()
        let revision = connectionRevision
        state = .loading(previous: [])
        let candidate = makeAPI(credentials)
        do {
            let identity = try await candidate.session()
            guard UUID(uuidString: identity.user.id) != nil else { throw APIClientError.invalidResponse }
            let first = try await Self.firstPage(candidate)
            try Task.checkCancellation()
            guard revision == connectionRevision else { return }
            try settings.save(credentials)
            instance = credentials.instance
            session = identity
            api = candidate
            workspaces.attach(candidate)
            show(first, keeping: [])
        } catch is CancellationError {
            guard revision == connectionRevision else { return }
            state = .disconnected
        } catch {
            guard revision == connectionRevision else { return }
            state = .failed(error as? APIClientError ?? .invalidResponse, previous: [])
        }
    }

    private func resetConnection() {
        connectionRevision &+= 1
        requestRevision &+= 1
        calendarRequestRevision &+= 1
        api?.cancelRequests()
        api = nil; instance = nil; session = nil
        workspaces.attach(nil)
        state = .disconnected; calendarState = .disconnected
        list = ServerNoteList(); wholeNoteList = false; listRevision &+= 1
        titleEdits = [:]
    }

    private struct FirstPage {
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

    public func signOut() async throws {
        let previous = api
        resetConnection()
        try settings.clear()
        try await previous?.signOut()
    }

    public func refresh() async {
        guard let api else { state = .disconnected; return }
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

    public func delete(id: String) async throws {
        guard let api else { throw APIClientError.unauthorized }
        let revision = connectionRevision
        let result = try await api.deleteNote(id: id)
        guard revision == connectionRevision else { throw CancellationError() }
        guard result.deleted else { throw APIClientError.invalidResponse }
        requestRevision &+= 1
        state = .loaded(state.notes.filter { $0.id != id })
        list.remove(id: id)
    }

    /// True when the app holds a session on the server a link names.
    public func isSignedIn(to origin: ServerOrigin) -> Bool {
        guard session != nil, let instance else { return false }
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

    /// An instance identity is mandatory: an outbox from another connection must never upload here.
    public func upload(_ entry: AudioOutboxEntry, from outbox: AudioOutbox) async throws -> AudioChunkDTO {
        guard let api, instance == outbox.instance, principalID?.lowercased() == outbox.principalID else { throw APIClientError.unauthorized }
        let revision = connectionRevision
        // Validate on the main actor; read the 4 MB chunk away from it.
        let file = try outbox.audioFile(for: entry)
        let wav = try await Task.detached(priority: .utility) { try Data(contentsOf: file) }.value
        guard revision == connectionRevision else { throw CancellationError() }
        let receipt = try await api.uploadAudio(noteID: entry.noteID, chunkID: entry.id, channel: entry.channel,
            sequence: entry.sequence, startMs: entry.startMs, wav: wav)
        guard revision == connectionRevision else { throw CancellationError() }
        return receipt
    }

    public func audioChunks(noteID: String) async throws -> [AudioChunkDTO] {
        guard let api else { throw APIClientError.unauthorized }
        let revision = connectionRevision
        let chunks = try await api.audioChunks(noteID: noteID)
        guard revision == connectionRevision else { throw CancellationError() }
        return chunks
    }

    public func retryAudio(noteID: String, chunkID: String) async throws -> AudioChunkDTO {
        guard let api else { throw APIClientError.unauthorized }
        let revision = connectionRevision
        let receipt = try await api.retryAudio(noteID: noteID, chunkID: chunkID)
        guard revision == connectionRevision else { throw CancellationError() }
        return receipt
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

    public func disconnect() throws {
        resetConnection()
        try settings.clear()
    }

    #if DEBUG
    /// Static QA states are labeled by the host app and contain no credentials,
    /// API transport or document data. This entry point is absent in Release.
    public func showQAState(_ state: ServerNotesState) {
        connectionRevision &+= 1
        requestRevision &+= 1
        api = nil
        instance = nil
        self.session = nil
        workspaces.attach(nil)
        self.calendarState = .disconnected
        self.state = state
        wholeNoteList = false; listRevision &+= 1
        list.replace(rows: state.notes.map { ServerNoteRow($0) }.sorted(by: ServerNoteRow.precedes), total: state.notes.count, nextCursor: nil)
    }
    #endif
}
