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
                             title: note.displayTitle, subtitle: note.rowSubtitle, excerpt: inTitle ? nil : line)
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
                      subtitle: (Date.serverTimestamp(hit.row.createdAt) ?? .distantPast).formatted(date: .omitted, time: .shortened),
                      excerpt: hit.excerpt.map { $0.map(\.text).joined() }, excerptParts: hit.excerpt)
        }
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

    private var hits: [SearchHit] {
        mode.isDemo
            ? NoteSearch.local(store.activeNotes, query: query, currentUser: store.currentUser)
            : searchedAccount.map { account, notes in
                NoteSearch.server(search?.hits ?? [], account: account, onThisPhone: { recording.isActive($0) || recording.hasPendingAudio($0) },
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
                          retry: { search?.retry() }, more: { Task { await search?.loadMore() } }) { hit in
                focused = false
                router.path.append(hit.route)
            }
        } leading: {
            BackButton()
        } bottom: {
            SearchBar(query: $query, focused: $focused)
        }
        .task {
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
    @Environment(\.motion) private var motion

    var body: some View {
        Group {
            if query.isEmpty || progress == .idle {
                prompt(symbol: "magnifyingglass", title: "Search your notes", detail: "Search by title, notes, or people.")
            } else if case .failed(let message) = progress, hits.isEmpty {
                prompt(symbol: "magnifyingglass", title: "Search could not finish", detail: message, action: retry)
            } else if hits.isEmpty, progress == .searching {
                prompt(symbol: "magnifyingglass", title: "Searching…", detail: nil)
            } else if hits.isEmpty {
                prompt(symbol: "magnifyingglass", title: "No matching notes", detail: "Try another title, phrase, or name.")
            } else {
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 0) {
                        ForEach(hits) { hit in
                            Button { open(hit) } label: {
                                VStack(alignment: .leading, spacing: 0) {
                                    NoteListRow(tile: hit.tile, title: hit.title, subtitle: hit.subtitle)
                                    if let excerpt = hit.excerpt {
                                        Text(hit.excerptParts.map(NoteSearch.marked) ?? (try? AttributedString(markdown: excerpt)) ?? AttributedString(excerpt))
                                            .readingFont(.rowSubtitle).foregroundStyle(Theme.textSecondary)
                                            .lineLimit(2).padding(.leading, 62).padding(.bottom, 8)
                                            // It grows with the row above it, to the same limit.
                                            .dynamicTypeSize(...DynamicTypeSize.accessibility1)
                                    }
                                }
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

/// The field and its keyboard-dismiss control share one glass group, so the
/// small button buds off the field when the keyboard rises and is drawn back
/// into it when the keyboard goes.
private struct SearchBar: View {
    @Binding var query: String
    var focused: FocusState<Bool>.Binding
    @Environment(\.motion) private var motion
    @Namespace private var glass

    var body: some View {
        GlassChromeGroup(spacing: 10) {
            HStack(spacing: 10) {
                HStack(spacing: 10) {
                    Image(systemName: "magnifyingglass").font(.system(size: 17))
                        .foregroundStyle(Theme.textSecondary)
                    TextField("", text: $query, prompt: Text("Search").foregroundStyle(Theme.textTertiary))
                        .font(.system(size: 17)).foregroundStyle(Theme.textPrimary)
                        .focused(focused)
                        .submitLabel(.search)
                        .autocorrectionDisabled()
                        .textInputAutocapitalization(.never)
                        .onSubmit { focused.wrappedValue = false }
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
                .glassSurface(Capsule(), tint: .white.opacity(0.08))
                .glassMorph("field", in: glass)
                if focused.wrappedValue {
                    HideKeyboardButton { focused.wrappedValue = false }
                        .glassMorph("keyboard", in: glass)
                }
            }
        }
        .animation(motion.animation(.gooey, entering: focused.wrappedValue), value: focused.wrappedValue)
        .animation(motion.snappy, value: query.isEmpty)
    }
}
