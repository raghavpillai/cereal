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

    /// The server's results, in its order. The tile follows Home: audio still on
    /// this phone first, then the other party's initial, then a document.
    /// `title` is the newest title known for a result: a note renamed, or given
    /// a title by generating its notes, since the search ran is shown under that one.
    static func server(_ hits: [ServerNoteSearchHit], account: AccountID, onThisPhone: (NoteRef) -> Bool = { _ in false },
                       title: (ServerNoteRow) -> String = { $0.title }) -> [SearchHit] {
        hits.map { hit in
            let title = title(hit.row)
            let ref = NoteRef(account: account, id: hit.id)
            return SearchHit(route: .serverNote(ref), tile: onThisPhone(ref) ? .phone : ServerHomeGrouping.tile(for: hit.row),
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
        return attendees >= 2 ? "\(day)  •  \(attendees) attendees" : day
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
                NoteSearch.server(search?.hits ?? [], account: account, onThisPhone: { recording.isActive($0) },
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
            SearchBar(query: $query, focused: $focused) {
                RecentSearchStore.remember(query)
                recent = RecentSearchStore.load()
            }
        }
        .task {
            #if DEBUG
            // QA only: `-qa.searchQuery <words>` opens search with words already typed.
            if let words = UserDefaults.standard.string(forKey: "qa.searchQuery") { query = words }
            #endif
            begin()
            search?.update(query)
            // The keyboard rises with the push instead of after it.
            try? await Task.sleep(for: .milliseconds(120))
            focused = true
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
    @Environment(\.motion) private var motion
    /// Rows shown before "View all".
    static let firstShown = 20

    var body: some View {
        Group {
            if query.isEmpty || progress == .idle {
                RecentSearches(searches: recent, pick: pickRecent)
            } else if case .failed(let message) = progress, hits.isEmpty {
                prompt(symbol: "magnifyingglass", title: "Search could not finish", detail: message, action: retry)
            } else if hits.isEmpty, progress == .searching {
                prompt(symbol: "magnifyingglass", title: "Searching…", detail: nil)
            } else if hits.isEmpty {
                prompt(symbol: "magnifyingglass", title: "No matching notes", detail: "Try another title, phrase, or name.")
            } else {
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 0) {
                        // As in the reference: a "Previous meetings" header with "View all", then the rows.
                        HStack {
                            Text("Previous meetings").font(.system(size: 15)).foregroundStyle(Theme.textSecondary)
                                .accessibilityAddTraits(.isHeader)
                            Spacer()
                            if hits.count > Self.firstShown && !showsAll {
                                Button("View all") { withAnimation(motion.snappy) { showsAll = true }; more() }
                                    .font(.system(size: 14)).foregroundStyle(Theme.textPrimary)
                                    .padding(.horizontal, 14).frame(height: 32)
                                    .overlay(Capsule().strokeBorder(Theme.ink.opacity(0.12), lineWidth: 1))
                                    .contentShape(Capsule())
                                    .buttonStyle(.squash)
                            }
                        }
                        .padding(.bottom, 6)
                        ForEach(showsAll ? hits : Array(hits.prefix(Self.firstShown))) { hit in
                            Button { open(hit) } label: {
                                NoteListRow(tile: hit.tile, title: hit.title, subtitle: hit.subtitle,
                                            tinted: true, markedTitle: NoteSearch.markedTitle(hit.title, query: query))
                            }
                            .buttonStyle(RowPressStyle())
                            .transition(motion.fade)
                            .onAppear { if hit.id == hits.last?.id { more() } }
                        }
                        if case .failed(let message) = progress {
                            VStack(alignment: .leading, spacing: 0) {
                                Text("Search could not finish. " + message).font(.footnote)
                                Button("Retry", action: retry).frame(minHeight: 44).contentShape(Rectangle())
                            }.foregroundStyle(Theme.textSecondary).padding(.top, 8)
                        }
                    }
                    // Results for the previous words stay, dimmed, until the next arrive.
                    .opacity(progress == .searching ? 0.5 : 1)
                    .padding(.leading, Theme.gutter).padding(.trailing, 8)
                    .padding(.top, 20).clearsBottomChrome()
                }
                .scrollIndicators(.hidden)
                .scrollDismissesKeyboard(.interactively)
                .transition(motion.fade)
            }
        }
        .animation(motion.snappy, value: query.isEmpty)
        .animation(motion.snappy, value: hits.isEmpty)
        .animation(motion.snappy, value: progress)
        .onChange(of: query) { showsAll = false }
    }

    /// Centred in the space above the search field. When the keyboard leaves
    /// little room (a landscape iPhone), the icon and then the detail line drop out.
    private func prompt(symbol: String, title: String, detail: String?, action: (() -> Void)? = nil) -> some View {
        ViewThatFits(in: .vertical) {
            promptBody(symbol: symbol, title: title, detail: detail, action: action)
            promptBody(symbol: nil, title: title, detail: detail, action: action)
            promptBody(symbol: nil, title: title, detail: nil, action: action)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .transition(motion.fade)
    }

    private func promptBody(symbol: String?, title: String, detail: String?, action: (() -> Void)? = nil) -> some View {
        VStack(spacing: 10) {
            if let symbol {
                Image(systemName: symbol)
                    .font(.system(size: 60, weight: .light))
                    .foregroundStyle(Theme.textPrimary.opacity(0.7))
                    .padding(.bottom, 12)
            }
            Text(title).font(.display(25)).fontWeight(.medium).foregroundStyle(Theme.textPrimary.opacity(0.7))
                .contentTransition(.opacity)
            if let detail {
                Text(detail).font(.display(25)).foregroundStyle(Theme.textTertiary)
                    .contentTransition(.opacity)
            }
            if let action {
                Button("Retry", action: action).foregroundStyle(Theme.textSecondary)
                    .frame(minWidth: 44, minHeight: 44).contentShape(Rectangle())
            }
        }
        .multilineTextAlignment(.center)
        .padding(.horizontal, Theme.gutter)
        .padding(.bottom, symbol == nil ? 66 : 80)
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
                    Text("Recent searches").font(.system(size: 15)).foregroundStyle(Theme.textSecondary)
                        .accessibilityAddTraits(.isHeader)
                        .padding(.bottom, 6)
                    ForEach(searches, id: \.self) { search in
                        Button { pick(search) } label: {
                            HStack(spacing: 14) {
                                Image(systemName: "arrow.counterclockwise").font(.system(size: 15))
                                    .foregroundStyle(Theme.textSecondary)
                                Text("\u{201C}\(search)\u{201D}").font(.system(size: 16)).foregroundStyle(Theme.textPrimary)
                                    .lineLimit(1)
                                Spacer(minLength: 0)
                            }
                            .frame(minHeight: 44).contentShape(Rectangle())
                        }
                        .buttonStyle(RowPressStyle())
                    }
                }
            }
            .padding(.leading, Theme.gutter).padding(.trailing, 8).padding(.top, 20)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .scrollIndicators(.hidden)
        .scrollDismissesKeyboard(.interactively)
    }
}

/// The field and its keyboard-dismiss control share one glass group, so the
/// small button buds off the field when the keyboard rises and is drawn back
/// into it when the keyboard goes.
private struct SearchBar: View {
    @Binding var query: String
    var focused: FocusState<Bool>.Binding
    var submit: () -> Void = {}
    @Environment(\.motion) private var motion
    @Namespace private var glass

    var body: some View {
        GlassChromeGroup(spacing: 10) {
            HStack(spacing: 10) {
                HStack(spacing: 10) {
                    Image(systemName: "magnifyingglass").font(.system(size: 17))
                        .foregroundStyle(Theme.textSecondary)
                    TextField("", text: $query, prompt: Text("Search by title, notes, or people").foregroundStyle(Theme.textTertiary))
                        .font(.system(size: 15)).foregroundStyle(Theme.textPrimary)
                        .focused(focused)
                        .submitLabel(.search)
                        .autocorrectionDisabled()
                        .textInputAutocapitalization(.never)
                        .onSubmit { focused.wrappedValue = false; submit() }
                        .accessibilityIdentifier("search-field")
                    if !query.isEmpty {
                        Button { query = "" } label: {
                            Image(systemName: "xmark.circle.fill").font(.system(size: 17))
                                .foregroundStyle(Theme.textSecondary)
                                .frame(width: 30, height: 30).contentShape(Rectangle().inset(by: -7))
                        }
                        .buttonStyle(SquashButtonStyle(scale: 0.86))
                        .accessibilityLabel("Clear search")
                        .transition(motion.pop)
                    }
                }
                .padding(.leading, 16).padding(.trailing, 10)
                .frame(height: 50)
                .glassSurface(Capsule(), tint: Theme.fieldTint)
                .overlay(Capsule().strokeBorder(Theme.floatingEdge, lineWidth: 1).allowsHitTesting(false))
                .glassMorph("field", in: glass)
                // As in the reference, a keyboard button stays beside the field: it lowers the
                // keyboard while it is up and raises it again when it is down.
                Button { focused.wrappedValue.toggle() } label: {
                    Image(systemName: "keyboard").font(.system(size: 17))
                        .foregroundStyle(Theme.textPrimary.opacity(0.85)).frame(width: 50, height: 50)
                        .contentShape(Circle())
                }
                .buttonStyle(.squash)
                .glassSurface(Circle(), interactive: true, tint: Theme.fieldTint)
                .overlay(Circle().strokeBorder(Theme.floatingEdge, lineWidth: 1).allowsHitTesting(false))
                .glassMorph("keyboard", in: glass)
                .accessibilityLabel(focused.wrappedValue ? "Hide keyboard" : "Show keyboard")
            }
        }
        .animation(motion.animation(.gooey, entering: focused.wrappedValue), value: focused.wrappedValue)
        .animation(motion.snappy, value: query.isEmpty)
    }
}
