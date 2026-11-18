import SwiftUI
import APIClient

/// "Shared with me": the shared notes opened on this device, and a field for
/// opening one from a link in hand. The list is this device's own and is the
/// same whoever is signed in; a note on a server the app is not signed in to
/// opens as a request to sign in there.
struct SharedWithMeView: View {
    @Environment(SharedNoteHistory.self) private var history
    @Environment(Router.self) private var router
    @Environment(AccountsModel.self) private var accounts
    @Environment(ConnectionMode.self) private var mode
    @Environment(\.motion) private var motion
    @State private var text = ""
    @State private var problem: String?
    @FocusState private var focused: Bool

    var body: some View {
        RailScaffold {
            // One list in both states. It used to be swapped for the empty prompt in the same
            // animated change that deleted its last row, and the collection view under it
            // then refused the update ("invalid number of items in section 0"): a crash.
            // The list stays; the prompt is drawn over it once it has nothing to show.
            SharedVisitList(visits: history.visits, open: { router.open(.shared($0.link), accounts: accounts, mode: mode) },
                            remove: { visit in withAnimation(motion.exit(.settle)) { history.remove(visit.id) } })
                .overlay {
                    if history.visits.isEmpty {
                        ScreenPrompt(symbol: "link", title: "Nothing shared with you yet", detail: "Shared notes you open are kept here.")
                            .allowsHitTesting(false)
                    }
                }
                .animates(history.visits.isEmpty)
        } leading: {
            BackButton()
        } bottom: {
            VStack(alignment: .leading, spacing: 8) {
                if let problem {
                    Text(problem).font(.footnote).foregroundStyle(Theme.danger)
                        .padding(.horizontal, 16).transition(motion.rise)
                        .accessibilityIdentifier("paste-link-problem")
                }
                FieldBar(symbol: "link", prompt: "Paste link", text: $text, focused: $focused, submitLabel: .go,
                         keyboard: .URL, contentType: .URL, clearLabel: "Clear link", identifier: "paste-link-field", submit: openTyped)
            }
            .animates(problem)
        }
        .haptic(.error, trigger: problem) { old, new in new != nil && new != old }
        .onChange(of: text) { old, new in
            problem = nil
            // A link that arrives whole was pasted: open it without another tap.
            if new.count - old.count > 1, pastedLink(new) != nil { _ = openTyped() }
        }
    }

    private func pastedLink(_ text: String) -> AppLink? {
        switch AppLink(text: text) {
        case .shared(let link)?: .shared(link)
        case .note(let link)?: .note(link)
        default: nil
        }
    }

    /// Opens what is in the field. True when it was refused: the keyboard stays up so it can be corrected.
    private func openTyped() -> Bool {
        guard !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return false }
        guard let link = pastedLink(text) else {
            problem = "That is not a link to a Cereal note. A shared link looks like https://notes.example.com/s/…"
            return true
        }
        text = ""; problem = nil; focused = false
        router.open(link, accounts: accounts, mode: mode)
        return false
    }
}

/// The page title, where Home has "Today": the same face, size and spacing.
private struct SharedWithMeTitle: View {
    var body: some View {
        Text("Shared with me")
            .displayFont(HomeMetrics.titleSize).foregroundStyle(Theme.textPrimary)
            .accessibilityAddTraits(.isHeader)
            .padding(.leading, 1).padding(.top, 35).padding(.bottom, 23)
    }
}

/// Home's rows in a list, so that a row can be swiped away.
private struct SharedVisitList: View {
    let visits: [SharedNoteVisit]
    let open: (SharedNoteVisit) -> Void
    let remove: (SharedNoteVisit) -> Void
    @Environment(\.bottomChromeHeight) private var chrome
    /// The text column of every screen: 16 pt in, 8 pt before the scaffold's own trailing margin.
    private static let insets = EdgeInsets(top: 0, leading: Theme.gutter, bottom: 0, trailing: 8)

    var body: some View {
        List {
            // The title is a section of its own: rows come and go in the other one,
            // so a removal never changes the count of the section the title is in.
            Section {
                SharedWithMeTitle()
                    .listRowInsets(Self.insets).listRowSeparator(.hidden).listRowBackground(Color.clear)
            }
            Section {
                ForEach(visits) { visit in
                    Button { open(visit) } label: {
                        NoteListRow(tile: .document, title: visit.title, subtitle: visit.host + Wording.dot + visit.openedAt.noteMetaString)
                    }
                    .buttonStyle(.row)
                    .listRowInsets(Self.insets).listRowSeparator(.hidden).listRowBackground(Color.clear)
                    .swipeActions(edge: .trailing, allowsFullSwipe: true) {
                        Button(role: .destructive) { Haptics.shared.play(.destructive); remove(visit) } label: { Label("Remove", systemImage: "trash") }
                            .tint(Theme.danger)
                    }
                    .contextMenu {
                        Button("Open", systemImage: "arrow.up.forward.app") { open(visit) }
                        ShareLink(item: visit.link.webURL) { Label("Share link", systemImage: "square.and.arrow.up") }
                        Button("Remove", systemImage: "trash", role: .destructive) { Haptics.shared.play(.destructive); remove(visit) }
                    }
                    .accessibilityIdentifier("shared-visit")
                }
            }
        }
        .listSectionSpacing(0)
        .listStyle(.plain)
        .scrollContentBackground(.hidden)
        .scrollIndicators(.hidden)
        .scrollDismissesKeyboard(.interactively)
        .environment(\.defaultMinListRowHeight, 0)
        .contentMargins(.bottom, chrome + 24, for: .scrollContent)
    }
}
