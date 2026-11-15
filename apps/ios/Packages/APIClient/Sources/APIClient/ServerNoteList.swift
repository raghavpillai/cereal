import Foundation
import Observation
import Contracts

/// One line of the server Home: what a row shows, and nothing a note page needs.
/// The server sends these a page at a time; a note's text is read when it is opened.
public struct ServerNoteRow: Identifiable, Equatable, Sendable {
    public let id: String
    public var title: String
    public var date: String
    public var createdAt: String
    public var updatedAt: String
    /// Invited people other than the account holder, in invitation order. The server caps this list.
    public var attendeeNames: [String]
    /// Everyone invited, the account holder included. Rooms and equipment are not counted.
    public var attendeeTotal: Int
    public var transcriptRevision: Int
    public var generatedRevision: Int?

    public init(id: String, title: String, date: String, createdAt: String, updatedAt: String,
                attendeeNames: [String] = [], attendeeTotal: Int = 0, transcriptRevision: Int = 0, generatedRevision: Int? = nil) {
        self.id = id; self.title = title; self.date = date; self.createdAt = createdAt; self.updatedAt = updatedAt
        self.attendeeNames = attendeeNames; self.attendeeTotal = attendeeTotal
        self.transcriptRevision = transcriptRevision; self.generatedRevision = generatedRevision
    }

    public init(_ summary: NoteSummaryDTO) {
        self.init(id: summary.id, title: summary.title, date: summary.date, createdAt: summary.createdAt, updatedAt: summary.updatedAt,
                  attendeeNames: summary.attendees.people.map(\.name), attendeeTotal: summary.attendees.total,
                  transcriptRevision: summary.transcriptRevision, generatedRevision: summary.generatedRevision)
    }

    /// A note this device has just read or changed. A full note carries no
    /// invitees, so those stay as the row it replaces had them.
    public init(_ note: NoteDTO, replacing old: ServerNoteRow? = nil) {
        self.init(id: note.id, title: note.title, date: note.date, createdAt: note.createdAt, updatedAt: note.updatedAt,
                  attendeeNames: old?.attendeeNames ?? [], attendeeTotal: old?.attendeeTotal ?? 0,
                  transcriptRevision: note.transcriptRevision, generatedRevision: note.generatedRevision)
    }

    /// The other party of a meeting, for a letter tile. Nil for a note with nobody else invited.
    public var firstAttendee: String? {
        attendeeNames.first { !$0.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }
    }

    /// The server's order: newest meeting first, then creation time, then ID.
    /// It is total, so a row has exactly one place among the others.
    public static func precedes(_ left: ServerNoteRow, _ right: ServerNoteRow) -> Bool {
        if left.date != right.date { return left.date > right.date }
        if left.createdAt != right.createdAt { return left.createdAt > right.createdAt }
        return left.id.lowercased() > right.id.lowercased()
    }
}

/// The rows loaded so far and where the next page starts.
public struct ServerNoteList: Equatable, Sendable {
    public internal(set) var rows: [ServerNoteRow] = []
    /// Every note on the server, not only the rows loaded. Nil until the first page arrives.
    public internal(set) var total: Int?
    public internal(set) var isLoadingMore = false
    /// The last request for a further page failed. Asking again retries it.
    public internal(set) var moreError: APIClientError?
    /// Goes up with every change, so a view can watch one number instead of comparing rows.
    public internal(set) var revision = 0
    var nextCursor: String?
    /// Deleted here since the last full reload. A page that was already on its way must not bring them back.
    var removed: Set<String> = []

    public init() {}
    public var hasMore: Bool { nextCursor != nil }

    mutating func replace(rows: [ServerNoteRow], total: Int, nextCursor: String?) {
        self.rows = rows; self.total = total; self.nextCursor = nextCursor
        isLoadingMore = false; moreError = nil; removed = []
        revision &+= 1
    }

    /// Adds a page after the rows already held. A note that moved between two
    /// requests can be handed over twice; it keeps its first place.
    mutating func append(rows page: [ServerNoteRow], total: Int, nextCursor: String?) {
        var known = Set(rows.map(\.id))
        for row in page where !removed.contains(row.id) && known.insert(row.id).inserted { rows.append(row) }
        self.total = total; self.nextCursor = nextCursor
        isLoadingMore = false; moreError = nil
        revision &+= 1
    }

    /// A note read or changed on this device. An existing row is updated where it
    /// stands. A new one is placed in order, unless it belongs after the last row
    /// loaded: then its own page will bring it.
    mutating func upsert(_ note: NoteDTO, insertIfMissing: Bool) {
        if let index = rows.firstIndex(where: { $0.id == note.id }) {
            let row = ServerNoteRow(note, replacing: rows[index])
            guard row != rows[index] else { return }
            rows[index] = row
        } else if insertIfMissing {
            let row = ServerNoteRow(note)
            let index = rows.firstIndex { ServerNoteRow.precedes(row, $0) } ?? rows.count
            if index == rows.count && hasMore { return }
            rows.insert(row, at: index)
            removed.remove(note.id)
            if let total { self.total = total + 1 }
        } else { return }
        revision &+= 1
    }

    mutating func remove(id: String) {
        removed.insert(id)
        guard let index = rows.firstIndex(where: { $0.id == id }) else { return }
        rows.remove(at: index)
        if let total { self.total = max(0, total - 1) }
        revision &+= 1
    }
}

/// One note a search found, in the order the server ranked it.
public struct ServerNoteSearchHit: Identifiable, Equatable, Sendable {
    public let row: ServerNoteRow
    /// The matching text when the match is not already visible in the title, with the matched words marked.
    public let excerpt: [Part]?
    public var id: String { row.id }

    public struct Part: Equatable, Sendable {
        public let text: String
        public let isMatch: Bool
        public init(text: String, isMatch: Bool) { self.text = text; self.isMatch = isMatch }
    }

    public init(row: ServerNoteRow, excerpt: [Part]?) { self.row = row; self.excerpt = excerpt }

    public init(_ result: NoteSearchResultDTO) {
        self.init(row: ServerNoteRow(result.note), excerpt: result.excerpt.map { $0.parts.map { Part(text: $0.text, isMatch: $0.hit) } })
    }
}

public enum ServerNoteSearchPhase: Equatable, Sendable {
    /// Nothing searchable in the field.
    case idle
    /// A search is waiting out the pause in typing, or on its way. Earlier results stay visible.
    case searching
    case loaded
    case failed(APIClientError)
}

/// Search as the user types: a pause before each request, the request for an
/// older query cancelled, and its answer never shown. Results are the server's;
/// a server without the search endpoint is searched through the notes already held.
@MainActor @Observable
public final class ServerNoteSearch {
    public private(set) var phase: ServerNoteSearchPhase = .idle
    public private(set) var hits: [ServerNoteSearchHit] = []
    /// The query the hits answer. It trails the field while a newer search is pending.
    public private(set) var answered = ""
    public private(set) var total = 0
    public private(set) var isLoadingMore = false
    public var hasMore: Bool { nextCursor != nil }

    @ObservationIgnored private let server: ServerNotesModel
    @ObservationIgnored private let pause: Duration
    @ObservationIgnored private var task: Task<Void, Never>?
    @ObservationIgnored private var wanted = ""
    @ObservationIgnored private var nextCursor: String?
    @ObservationIgnored private var generation: UInt64 = 0

    public init(server: ServerNotesModel, pause: Duration = .milliseconds(250)) {
        self.server = server
        self.pause = pause
    }

    /// The words the server looks for. A lone ASCII letter or digit is not one.
    public static func isSearchable(_ query: String) -> Bool {
        query.precomposedStringWithCompatibilityMapping.lowercased()
            .split(whereSeparator: { !($0.isLetter || $0.isNumber || "'’._@+-".contains($0)) })
            .map { $0.drop(while: { !($0.isLetter || $0.isNumber) }) }
            .contains { word in word.count > 1 || word.unicodeScalars.contains { !$0.isASCII } }
    }

    /// Tell the search what the field now holds.
    public func update(_ text: String) {
        let query = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard query != wanted else { return }
        wanted = query
        generation &+= 1
        task?.cancel()
        guard Self.isSearchable(query) else {
            task = nil; nextCursor = nil
            hits = []; answered = ""; total = 0; isLoadingMore = false; phase = .idle
            return
        }
        phase = .searching
        isLoadingMore = false
        let generation = generation, pause = pause
        task = Task { [weak self] in
            try? await Task.sleep(for: pause)
            guard !Task.isCancelled else { return }
            await self?.run(query, generation: generation)
        }
    }

    /// Ask again now, after a failure.
    public func retry() {
        guard Self.isSearchable(wanted) else { return }
        generation &+= 1
        task?.cancel()
        phase = .searching
        let generation = generation, query = wanted
        task = Task { [weak self] in await self?.run(query, generation: generation) }
    }

    /// The next page of the results on screen.
    public func loadMore() async {
        guard phase == .loaded, !isLoadingMore, let cursor = nextCursor, answered == wanted else { return }
        let generation = generation, query = wanted
        isLoadingMore = true
        defer { if generation == self.generation { isLoadingMore = false } }
        guard let page = try? await server.search(query: query, cursor: cursor), generation == self.generation else { return }
        var known = Set(hits.map(\.id))
        hits.append(contentsOf: page.hits.filter { known.insert($0.id).inserted })
        total = page.total; nextCursor = page.nextCursor
    }

    public func cancel() {
        generation &+= 1
        task?.cancel(); task = nil
        if phase == .searching { phase = hits.isEmpty && answered.isEmpty ? .idle : .loaded }
    }

    private func run(_ query: String, generation: UInt64) async {
        do {
            let page = try await server.search(query: query, cursor: nil)
            guard generation == self.generation, !Task.isCancelled else { return }
            hits = page.hits; total = page.total; nextCursor = page.nextCursor
            answered = query; phase = .loaded
        } catch is CancellationError {
        } catch {
            guard generation == self.generation else { return }
            phase = .failed(error as? APIClientError ?? .invalidResponse)
        }
    }
}

/// One page of search results, from the server or from the notes held on an older one.
public struct ServerNoteSearchPage: Sendable {
    public let hits: [ServerNoteSearchHit]
    public let total: Int
    public let nextCursor: String?
}

extension ServerNoteSearchPage {
    /// Title, typed notes and enhanced notes of whole notes already in memory:
    /// what search was before the server had an endpoint for it.
    static func local(_ notes: [NoteDTO], query: String) -> ServerNoteSearchPage {
        let hits: [ServerNoteSearchHit] = notes.compactMap { note in
            let inTitle = note.title.localizedStandardContains(query)
            let line = (note.body + "\n" + note.enhanced).components(separatedBy: .newlines)
                .first { !$0.isEmpty && $0.localizedStandardContains(query) }
            guard inTitle || line != nil else { return nil }
            let excerpt = inTitle ? nil : line.map { [ServerNoteSearchHit.Part(text: $0.trimmingCharacters(in: .whitespaces), isMatch: false)] }
            return ServerNoteSearchHit(row: ServerNoteRow(note), excerpt: excerpt)
        }
        return ServerNoteSearchPage(hits: hits, total: hits.count, nextCursor: nil)
    }
}
