import SwiftUI
import APIClient
import Contracts

/// One note found by a search, already resolved to where it opens.
struct SearchHit: Identifiable, Equatable {
    let route: Route
    let tile: NoteTile
    let title: String
    let subtitle: String
    /// The matching line, when the match is not already visible in the title.
    let excerpt: String?
    /// The same line with the matched words marked, when the server found it.
    var excerptParts: [ServerNoteSearchHit.Part]? = nil
    var id: Route { route }
}

/// What the results area shows besides the results themselves.
enum SearchProgress: Equatable {
    /// Nothing searchable has been typed.
    case idle
    /// A search is waiting or on its way; earlier results stay on screen.
    case searching
    case settled
    case failed(String)
}

/// Local notes are matched here, in the local store, and never leave the
/// device. Server notes are searched by the server, which also ranks them and
/// reads transcripts and invitees; this only shapes its results into rows.
enum NoteSearch {
    static func normalized(_ query: String) -> String {
        query.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    /// Title, typed notes, enhanced notes and attendee names.
    static func local(_ notes: [Note], query: String, currentUser: Person) -> [SearchHit] {
        let query = normalized(query)
        guard !query.isEmpty else { return [] }
        return notes.compactMap { note in
            let people = note.attendees.map(\.name)
            let lines = note.userNotes.components(separatedBy: .newlines)
                + note.sections.flatMap { [$0.heading] + $0.bullets }
            let inTitle = note.title.localizedStandardContains(query)
            let line = (lines + people).first { !$0.isEmpty && $0.localizedStandardContains(query) }
            guard inTitle || line != nil else { return nil }
            return SearchHit(route: .note(note.id), tile: note.tile(currentUser: currentUser),
                             title: note.displayTitle, subtitle: subtitle(note.createdAt, attendees: note.attendees.count),
                             excerpt: inTitle ? nil : line)
        }
    }

    /// The server's results, in its order. The tile follows Home: the other
    /// party's initial, otherwise a document.
    /// `title` is the newest title known for a result: a note renamed, or given
    /// a title by generating its notes, since the search ran is shown under that one.
    static func server(_ hits: [ServerNoteSearchHit], account: AccountID,
                       title: (ServerNoteRow) -> String = { $0.title }) -> [SearchHit] {
        hits.map { hit in
            let title = title(hit.row)
            let ref = NoteRef(account: account, id: hit.id)
            return SearchHit(route: .serverNote(ref), tile: ServerHomeGrouping.tile(for: hit.row),
                      title: title.isEmpty ? "Untitled note" : title,
                      subtitle: subtitle(Date.serverTimestamp(hit.row.createdAt) ?? .distantPast, attendees: hit.row.attendeeTotal),
                      excerpt: hit.excerpt.map { $0.map(\.text).joined() }, excerptParts: hit.excerpt)
        }
    }

    /// A result's line, as the reference writes it: the day ("Tue 17 Nov"), then
    /// "4 attendees" for a meeting with others.
    static func subtitle(_ date: Date, attendees: Int, calendar: Calendar = .current) -> String {
        let format = DateFormatter()
        format.calendar = calendar; format.timeZone = calendar.timeZone
        format.dateFormat = calendar.component(.year, from: date) == calendar.component(.year, from: .now) ? "EEE d MMM" : "EEE d MMM yyyy"
        let day = format.string(from: date)
        return attendees >= 2 ? day + Wording.dot + "\(attendees) attendees" : day
    }

    /// The title with each word of the query marked by the brand's highlight, as
    /// the reference marks matches in result titles.
    static func markedTitle(_ title: String, query: String) -> AttributedString {
        var text = AttributedString(title)
        let words = normalized(query).split(whereSeparator: \.isWhitespace).map(String.init).filter { !$0.isEmpty }
        for word in words {
            var searchStart = text.startIndex
            while searchStart < text.endIndex,
                  let range = text[searchStart...].range(of: word, options: [.caseInsensitive, .diacriticInsensitive]) {
                text[range].backgroundColor = Theme.accentHighlight
                searchStart = range.upperBound
            }
        }
        return text
    }

    /// Whether the title itself shows why the note was found. When it does not, the row adds the matching line.
    static func titleShowsMatch(_ title: String, query: String) -> Bool {
        normalized(query).split(whereSeparator: \.isWhitespace)
            .contains { title.range(of: $0, options: [.caseInsensitive, .diacriticInsensitive]) != nil }
    }

    /// The line under a result's date: the matching excerpt, with its matched words
    /// marked when the server marked them. Nil when the title already shows the match.
    static func excerptLine(for hit: SearchHit, query: String) -> AttributedString? {
        guard !titleShowsMatch(hit.title, query: query) else { return nil }
        if let parts = hit.excerptParts, !parts.isEmpty { return marked(parts) }
        return hit.excerpt.flatMap { $0.isEmpty ? nil : AttributedString($0) }
    }

    /// The matching line with its matched words in the primary text colour. Size and weight do not change.
    static func marked(_ parts: [ServerNoteSearchHit.Part]) -> AttributedString {
        parts.reduce(into: AttributedString()) { text, part in
            var run = AttributedString(part.text)
            if part.isMatch { run.foregroundColor = Theme.textPrimary }
            text.append(run)
        }
    }
}

/// Full-page search. The field is a glass capsule that rides directly above
/// the keyboard; the only navigation control is Back.
struct SearchView: View {
    @Environment(NoteStore.self) private var store
    @Environment(AccountsModel.self) private var accounts
    @Environment(ServerRecordingController.self) private var recording
    @Environment(ConnectionMode.self) private var mode
    @Environment(Router.self) private var router
    @State private var query = ""
    /// The account the search is sent to. A query is text that leaves the device,
    /// so it goes only to an account in the search scope; until the scope can be
    /// chosen and results of several accounts are interleaved, that is the first
    /// Ready account in it. With one account it is that account.
    private var searched: AccountSession? {
        accounts.sessions.first { $0.isReady && (accounts.searchScope.isEmpty || accounts.searchScope.contains($0.id)) }
    }
    @State private var searchedKey: UUID?
    /// Server mode only: sends the query after a pause in typing and drops answers to older ones.
    @State private var search: ServerNoteSearch?
    @FocusState private var focused: Bool
    @State private var recent = RecentSearchStore.load()

    private var hits: [SearchHit] {
        mode.isDemo
            ? NoteSearch.local(store.activeNotes, query: query, currentUser: store.currentUser)
            : searchedAccount.map { account, notes in
                NoteSearch.server(search?.hits ?? [], account: account,
                                  title: notes.latestTitle(of:))
            } ?? []
    }

    /// The account the results on screen came from, while it is still here.
    private var searchedAccount: (AccountID, ServerNotesModel)? {
        guard let session = accounts.sessions.first(where: { $0.key == searchedKey }), let id = session.notes.accountID else { return nil }
        return (id, session.notes)
    }

    private func begin() {
        guard !mode.isDemo, let session = searched, session.key != searchedKey else { return }
        search?.cancel()
        searchedKey = session.key
        search = ServerNoteSearch(server: session.notes)
        search?.update(query)
    }

    private var progress: SearchProgress {
        guard !mode.isDemo else { return .settled }
        switch search?.phase ?? .idle {
        case .idle: return .idle
        case .searching: return .searching
        case .loaded: return .settled
        case .failed(let error): return .failed(error.localizedDescription)
        }
    }

    var body: some View {
        RailScaffold {
            SearchResults(query: NoteSearch.normalized(query), hits: hits, progress: progress,
                          retry: { search?.retry() }, more: { Task { await search?.loadMore() } }, open: { hit in
                focused = false
                RecentSearchStore.remember(query)
                recent = RecentSearchStore.load()
                router.path.append(hit.route)
            }, recent: recent, pickRecent: { query = $0 })
        } leading: {
            BackButton()
        } bottom: {
            FieldBar(symbol: "magnifyingglass", prompt: "Search by title, notes, or people", text: $query, focused: $focused,
                     clearLabel: "Clear search", identifier: "search-field") {
                RecentSearchStore.remember(query)
                recent = RecentSearchStore.load()
                return false
            }
        }
        // Asked for as the page appears, so the keyboard rises with the push and not after it.
        .onAppear { focused = true }
        .task {
            #if DEBUG
            // QA only: `-qa.searchQuery <words>` opens search with words already typed.
            if let words = UserDefaults.standard.string(forKey: "qa.searchQuery") { query = words }
            #endif
            begin()
            search?.update(query)
        }
        .onChange(of: query) { _, text in search?.update(text) }
        // Another account's results are never shown under this one.
        .onChange(of: accounts.signature) {
            guard !mode.isDemo, searched?.key != searchedKey else { return }
            query = ""; search?.update("")
            if searched == nil { search = nil; searchedKey = nil } else { begin() }
        }
    }
}

/// What the search page shows. One at a time, each replacing the last (`Replaced`).
enum SearchFace: Hashable {
    /// Nothing searchable typed: the recent searches.
    case recent
    case failed
    /// A first search on its way, with nothing to show yet.
    case waiting
    /// An answer with no matches.
    case none
    case results

    /// `settledEmpty` is whether the last answer found nothing: while it did, further
    /// letters leave "No matching notes" in place until something matches.
    static func showing(query: String, hasHits: Bool, progress: SearchProgress, settledEmpty: Bool) -> SearchFace {
        if query.isEmpty || progress == .idle { return .recent }
        if hasHits { return .results }
        if case .failed = progress { return .failed }
        return progress == .searching && !settledEmpty ? .waiting : .none
    }
}

private struct SearchResults: View {
    let query: String
    let hits: [SearchHit]
    var progress: SearchProgress = .settled
    var retry: () -> Void = {}
    var more: () -> Void = {}
    let open: (SearchHit) -> Void
    var recent: [String] = []
    var pickRecent: (String) -> Void = { _ in }
    @State private var showsAll = false
    /// The last answer found nothing. Further letters then leave "No matching notes"
    /// where it is until something matches, instead of trading it for "Searching…".
    @State private var settledEmpty = false
    /// The results are waiting on a newer answer, and have been for longer than the grace.
    @State private var dimmed = false
    @Environment(\.motion) private var motion
    /// Rows shown before "View all".
    static let firstShown = 20

    var body: some View {
        let face = SearchFace.showing(query: query, hasHits: !hits.isEmpty, progress: progress, settledEmpty: settledEmpty)
        Replaced(id: face) {
            switch face {
            case .recent:
                RecentSearches(searches: recent, pick: pickRecent)
            case .failed:
                ScreenPrompt(symbol: "magnifyingglass", title: "Search could not finish",
                             detail: { if case .failed(let message) = progress { message } else { nil } }(),
                             actions: [.init(title: Wording.retry, run: retry)])
            case .waiting:
                // A first search waits like anything else: nothing for a moment, the prompt only if the wait is real.
                Loading(while: true, alignment: .center) {
                    ScreenPrompt(symbol: "magnifyingglass", title: "Searching…")
                } then: { EmptyView() }
            case .none:
                ScreenPrompt(symbol: "magnifyingglass", title: "No matching notes", detail: "Try another title, phrase, or name.")
            case .results:
                results
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .onChange(of: query) { showsAll = false }
        .onChange(of: progress) { _, now in
            if now == .settled { settledEmpty = hits.isEmpty }
            else if now == .idle { settledEmpty = false }
        }
        .task(id: progress == .searching) {
            guard progress == .searching else { dimmed = false; return }
            try? await Task.sleep(for: LoadingPolicy.grace)
            if !Task.isCancelled { dimmed = true }
        }
    }

    private var results: some View {
        let shown = showsAll ? hits : Array(hits.prefix(Self.firstShown))
        return ScrollView {
            LazyVStack(alignment: .leading, spacing: 0) {
                // As in the reference: a "Previous meetings" header with "View all", then the rows.
                HStack {
                    Text("Previous meetings").uiFont(15).foregroundStyle(Theme.textSecondary)
                        .accessibilityAddTraits(.isHeader)
                    Spacer()
                    if hits.count > Self.firstShown && !showsAll {
                        Button("View all") { showsAll = true; more() }
                            .uiFont(14).foregroundStyle(Theme.textPrimary)
                            .padding(.horizontal, 14).frame(height: 32)
                            .overlay(Capsule().strokeBorder(Theme.ink.opacity(0.12), lineWidth: 1))
                            .contentShape(Capsule())
                            .buttonStyle(.squash)
                    }
                }
                .padding(.bottom, 6)
                ForEach(shown) { hit in
                    RowButton { open(hit) } label: {
                        NoteListRow(tile: hit.tile, title: hit.title, subtitle: hit.subtitle,
                                    tinted: true, markedTitle: NoteSearch.markedTitle(hit.title, query: query),
                                    excerpt: NoteSearch.excerptLine(for: hit, query: query))
                    }
                    .onAppear { if hit.id == hits.last?.id { more() } }
                }
                if case .failed(let message) = progress {
                    VStack(alignment: .leading, spacing: 0) {
                        Text("Search could not finish. " + message).font(.footnote)
                        Button(Wording.retry, action: retry)
                            .fontWeight(.medium).foregroundStyle(Theme.accentText)
                            .frame(minHeight: 44).contentShape(Rectangle())
                    }.foregroundStyle(Theme.textSecondary).padding(.top, 8)
                }
            }
            // One answer replaces the last in place. Results are re-ranked on every letter;
            // rows that travelled to their new places crossed each other, text over text.
            .transaction(value: shown.map(\.id)) { $0.animation = nil }
            .padding(.leading, Theme.gutter).padding(.trailing, 8)
            .padding(.top, 20).clearsBottomChrome()
        }
        .scrollIndicators(.hidden)
        .scrollDismissesKeyboard(.interactively)
        // Results for the previous words stay until the next arrive, and dim only if that takes a while.
        .opacity(dimmed ? 0.5 : 1)
        .animates(dimmed)
    }
}

/// Searches made on this iPhone, newest first. They never leave the device.
enum RecentSearchStore {
    static let key = "search.recent"
    static let limit = 6
    static func load(_ defaults: UserDefaults = .standard) -> [String] { defaults.stringArray(forKey: key) ?? [] }
    static func remember(_ query: String, in defaults: UserDefaults = .standard) {
        let query = NoteSearch.normalized(query)
        guard !query.isEmpty else { return }
        var list = load(defaults).filter { $0.caseInsensitiveCompare(query) != .orderedSame }
        list.insert(query, at: 0)
        defaults.set(Array(list.prefix(limit)), forKey: key)
    }
}

/// "Recent searches" at the top of an empty search: each earlier query in
/// quotes beside a history glyph. Nothing is drawn before the first search.
private struct RecentSearches: View {
    let searches: [String]
    let pick: (String) -> Void

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 0) {
                if !searches.isEmpty {
                    Text("Recent searches").uiFont(15).foregroundStyle(Theme.textSecondary)
                        .accessibilityAddTraits(.isHeader)
                        .padding(.bottom, 6)
                    ForEach(searches, id: \.self) { search in
                        RowButton { pick(search) } label: {
                            HStack(spacing: 14) {
                                Image(systemName: "arrow.counterclockwise").font(.system(size: 15))
                                    .foregroundStyle(Theme.textSecondary)
                                Text("\u{201C}\(search)\u{201D}").uiFont(16).foregroundStyle(Theme.textPrimary)
                                    .lineLimit(1)
                                Spacer(minLength: 0)
                            }
                            .frame(minHeight: 44).contentShape(Rectangle())
                        }
                    }
                }
            }
            .padding(.leading, Theme.gutter).padding(.trailing, 8).padding(.top, 20)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .scrollIndicators(.hidden)
        .scrollDismissesKeyboard(.interactively)
        .dynamicTypeSize(...DynamicTypeSize.accessibility1)
    }
}
