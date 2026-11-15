import SwiftUI
import APIClient

/// "Shared with me": the shared notes opened on this device, and a field for
/// opening one from a link in hand. The list is this device's own and is the
/// same whoever is signed in; a note on a server the app is not signed in to
/// opens as a request to sign in there.
struct SharedWithMeView: View {
    @Environment(SharedNoteHistory.self) private var history
    @Environment(Router.self) private var router
    @Environment(ServerNotesModel.self) private var server
    @Environment(ConnectionMode.self) private var mode
    @Environment(\.dismiss) private var dismiss
    @Environment(\.motion) private var motion
    @State private var text = ""
    @State private var problem: String?
    @FocusState private var focused: Bool

    var body: some View {
        RailScaffold {
            Group {
                if history.visits.isEmpty {
                    VStack(alignment: .leading, spacing: 0) {
                        SharedWithMeTitle().padding(.leading, Theme.gutter)
                        ScreenPrompt(symbol: "link", title: "Nothing shared with you yet", detail: "Shared notes you open are kept here.")
                    }
                } else {
                    SharedVisitList(visits: history.visits, open: { router.open(.shared($0.link), server: server, mode: mode) },
                                    remove: { visit in withAnimation(motion.exit(.settle)) { history.remove(visit.id) } })
                        .transition(motion.fade)
                }
            }
            .animation(motion.snappy, value: history.visits.isEmpty)
        } leading: {
            GlassCircleButton(systemImage: "chevron.left") { dismiss() }
        } bottom: {
            VStack(alignment: .leading, spacing: 8) {
                if let problem {
                    Text(problem).font(.footnote).foregroundStyle(Theme.danger)
                        .padding(.horizontal, 16).transition(motion.fade)
                        .accessibilityIdentifier("paste-link-problem")
                }
                PasteLinkBar(text: $text, focused: $focused, submit: openTyped)
            }
            .animation(motion.snappy, value: problem)
        }
        .onChange(of: text) { old, new in
            problem = nil
            // A link that arrives whole was pasted: open it without another tap.
            if new.count - old.count > 1, pastedLink(new) != nil { openTyped() }
        }
    }

    private func pastedLink(_ text: String) -> AppLink? {
        switch AppLink(text: text) {
        case .shared(let link)?: .shared(link)
        case .note(let link)?: .note(link)
        default: nil
        }
    }

    private func openTyped() {
        guard !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return }
        guard let link = pastedLink(text) else {
            problem = "That is not a link to a Cereal note. A shared link looks like https://notes.example.com/s/…"
            return
        }
        text = ""; problem = nil; focused = false
        router.open(link, server: server, mode: mode)
    }
}

/// The page title, where Home has "Today": the same face, size and spacing.
private struct SharedWithMeTitle: View {
    var body: some View {
        Text("Shared with me")
            .font(.display(HomeMetrics.titleSize)).foregroundStyle(Theme.textPrimary)
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
            SharedWithMeTitle()
                .listRowInsets(Self.insets).listRowSeparator(.hidden).listRowBackground(Color.clear)
            ForEach(visits) { visit in
                Button { open(visit) } label: {
                    NoteListRow(tile: .document, title: visit.title, subtitle: "\(visit.host) • \(visit.openedAt.noteMetaString)")
                }
                .buttonStyle(RowPressStyle())
                .listRowInsets(Self.insets).listRowSeparator(.hidden).listRowBackground(Color.clear)
                .swipeActions(edge: .trailing, allowsFullSwipe: true) {
                    Button(role: .destructive) { remove(visit) } label: { Label("Remove", systemImage: "trash") }
                        .tint(Theme.danger)
                }
                .accessibilityIdentifier("shared-visit")
            }
        }
        .listStyle(.plain)
        .scrollContentBackground(.hidden)
        .scrollIndicators(.hidden)
        .scrollDismissesKeyboard(.interactively)
        .environment(\.defaultMinListRowHeight, 0)
        .contentMargins(.bottom, chrome + 24, for: .scrollContent)
    }
}

/// A glass capsule above the keyboard, as on Search. A link is pasted or typed
/// into it and opened with the keyboard's Go key.
private struct PasteLinkBar: View {
    @Binding var text: String
    var focused: FocusState<Bool>.Binding
    let submit: () -> Void
    @Environment(\.motion) private var motion
    @Namespace private var glass

    var body: some View {
        GlassChromeGroup(spacing: 10) {
            HStack(spacing: 10) {
                HStack(spacing: 10) {
                    Image(systemName: "link").font(.system(size: 17))
                        .foregroundStyle(Theme.textSecondary)
                    TextField("", text: $text, prompt: Text("Paste link").foregroundStyle(Theme.textTertiary))
                        .font(.system(size: 17)).foregroundStyle(Theme.textPrimary)
                        .focused(focused)
                        .keyboardType(.URL).textContentType(.URL)
                        .submitLabel(.go)
                        .autocorrectionDisabled()
                        .textInputAutocapitalization(.never)
                        .onSubmit(submit)
                        .accessibilityLabel("Paste link")
                        .accessibilityIdentifier("paste-link-field")
                    if !text.isEmpty {
                        Button { text = "" } label: {
                            Image(systemName: "xmark.circle.fill").font(.system(size: 17))
                                .foregroundStyle(Theme.textSecondary)
                                .frame(width: 30, height: 30).contentShape(Rectangle().inset(by: -7))
                        }
                        .buttonStyle(SquashButtonStyle(scale: 0.86))
                        .accessibilityLabel("Clear link")
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
        .animation(motion.snappy, value: text.isEmpty)
    }
}
