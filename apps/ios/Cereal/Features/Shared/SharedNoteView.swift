import SwiftUI
import APIClient

/// A note someone shared, read-only. A shared note is read with an account on
/// its own server, so it opens here only while the app is signed in to that
/// server; the session is sent there and nowhere else. For a note on any other
/// server, or with nobody signed in, the page says where the note is and
/// offers the browser, and nothing is fetched.
struct SharedNoteView: View {
    let link: SharedNoteLink
    @Environment(ServerNotesModel.self) private var server
    @Environment(ConnectionMode.self) private var mode
    @Environment(SharedNoteHistory.self) private var history
    @Environment(\.dismiss) private var dismiss
    @Environment(\.openURL) private var openURL
    @Environment(\.motion) private var motion
    @State private var phase: Phase = .loading
    @State private var copiedCount = 0

    enum Phase: Equatable {
        case loading
        case loaded(SharedNote)
        case failed(SharedNoteError)
    }

    private var access: LinkNavigation.SharedAccess {
        LinkNavigation.sharedAccess(to: link.origin, instance: server.instance, signedIn: server.session != nil, demo: mode.isDemo)
    }

    var body: some View {
        Group {
            switch access {
            case .signIn(let offered): ServerSignInPrompt(origin: link.origin, webURL: link.webURL, offersSignIn: offered)
            // The server no longer accepts the session it gave this app.
            case .read where phase == .failed(.signInRequired): ServerSignInPrompt(origin: link.origin, webURL: link.webURL, offersSignIn: true)
            case .read: reader
            }
        }
        // Signing in, signing out and leaving the local demo each change what this page may read.
        .task(id: "\(server.accountIdentity ?? "")|\(mode.isDemo)|\(link.webURL.absoluteString)") { await load() }
    }

    private var reader: some View {
        RailScaffold {
            Group {
                switch phase {
                case .loaded(let note):
                    ScrollView { SharedNoteDocument(note: note, host: link.origin.displayHost) { openURL(link.webURL) } }
                        .refreshable { await load() }
                        .transition(motion.fade)
                case .loading:
                    ProgressView("Loading shared note…")
                        .padding(.top, 28).padding(.leading, Theme.gutter)
                        .frame(maxWidth: .infinity, alignment: .leading)
                case .failed(let error):
                    failure(error)
                }
            }
            .animation(motion.snappy, value: phase)
        } leading: {
            GlassCircleButton(systemImage: "chevron.left") { dismiss() }
        } standalone: {
            ShareLink(item: link.webURL) { RailGlyph(systemImage: "square.and.arrow.up") }
                .buttonStyle(.plain).glass(Circle())
                .disabled(phase == .failed(.unavailable))
                .accessibilityLabel("Share link")
        } rail: {
            Menu {
                Button("Open in browser", systemImage: "safari") { openURL(link.webURL) }
                Button("Copy link", systemImage: "link") {
                    UIPasteboard.general.string = link.webURL.absoluteString
                    copiedCount += 1
                }
                if case .loaded(let note) = phase {
                    Button("Copy notes", systemImage: "doc.on.doc") {
                        UIPasteboard.general.string = [note.title, note.enhanced, note.body].filter { !$0.isEmpty }.joined(separator: "\n\n")
                        copiedCount += 1
                    }
                }
            } label: {
                RailGlyph(systemImage: "ellipsis")
            }.menuOrder(.fixed).tint(Theme.textSecondary).glass(Circle())
                .accessibilityLabel("Shared note actions")
        } bottom: {
            EmptyView()
        }
        .sensoryFeedback(.success, trigger: copiedCount)
    }

    @ViewBuilder private func failure(_ error: SharedNoteError) -> some View {
        let browser = ScreenPrompt.Action(title: "Open in browser") { openURL(link.webURL) }
        let host = link.origin.displayHost
        switch error {
        case .unavailable:
            ScreenPrompt(symbol: "link", title: "This shared note is not available",
                         detail: "The link may have expired or been turned off.", caption: host, actions: [browser], bottomRoom: 40)
        case .tooLarge:
            ScreenPrompt(symbol: "link", title: "This shared note is too large to open here", caption: host, actions: [browser], bottomRoom: 40)
        default:
            ScreenPrompt(symbol: "link", title: "This shared note could not be loaded", detail: error.localizedDescription, caption: host,
                         actions: [ScreenPrompt.Action(title: "Retry") { phase = .loading; Task { await load() } }, browser], bottomRoom: 40)
        }
    }

    private func load() async {
        // Without a session on the link's own server nothing is asked of anyone.
        guard access == .read else { phase = .loading; return }
        if phase == .failed(.signInRequired) { phase = .loading }
        do {
            let note = try await server.sharedNote(link)
            phase = .loaded(note)
            history.record(link, title: note.title.isEmpty ? "Untitled note" : note.title)
        } catch is CancellationError {
        } catch let error as SharedNoteError {
            // A refresh that could not reach the server leaves the note on screen.
            if case .loaded = phase, error.isRetryable { return }
            phase = .failed(error)
        } catch { phase = .failed(.invalidResponse) }
    }
}

/// The note itself, laid out as the note page lays out a note of one's own:
/// the same title, the same outlined capsule of facts, the same reading text.
private struct SharedNoteDocument: View {
    let note: SharedNote
    let host: String
    let openInBrowser: () -> Void
    @Environment(\.displayScale) private var displayScale

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(note.title.isEmpty ? "Untitled note" : note.title)
                .font(.noteTitle).lineSpacing(4).foregroundStyle(Theme.textPrimary).padding(.top, 18)
                .accessibilityAddTraits(.isHeader)
            if day != nil || note.duration != nil {
                NoteMetaCapsule {
                    if let day {
                        Image(systemName: "calendar").font(.system(size: 14.5))
                        Text(day)
                    }
                    if let duration = note.duration {
                        Image(systemName: "clock").font(.system(size: 14)).padding(.leading, day == nil ? 0 : 9)
                        Text(duration)
                    }
                }
                .padding(.leading, -HomeMetrics.tileOutdent)
                .padding(.top, 14)
                .accessibilityElement(children: .combine)
            }
            HStack(spacing: 6) {
                Image(systemName: "lock").font(.system(size: 12)).accessibilityHidden(true)
                Text("Shared · read-only · \(host)")
            }
            .font(.system(size: 13)).foregroundStyle(Theme.textSecondary)
            .padding(.top, 12).padding(.bottom, 17)
            .accessibilityIdentifier("shared-note-provenance")

            if note.enhanced.isEmpty && note.body.isEmpty {
                Text("The sender shared this note without any text.")
                    .foregroundStyle(Theme.textTertiary).readingFont(.body)
            }
            let enhanced = SharedNote.shown(note.enhanced), typed = SharedNote.shown(note.body)
            if !note.enhanced.isEmpty { ServerNoteMarkdown(text: enhanced.text) }
            if !note.body.isEmpty {
                if !note.enhanced.isEmpty {
                    // The sharer's own notes follow the generated ones, under their own heading.
                    Rectangle().fill(Theme.divider).frame(height: 1 / displayScale)
                        .padding(.top, 28).padding(.bottom, 24)
                    HomeSectionHeader(title: "Notes").padding(.bottom, 6)
                }
                ServerNoteMarkdown(text: typed.text)
            }
            if !enhanced.isWhole || !typed.isWhole {
                VStack(alignment: .leading, spacing: 0) {
                    Text("This note is longer than the app shows.").font(.footnote)
                    Button("Open in browser", action: openInBrowser)
                        .frame(minHeight: 44).contentShape(Rectangle())
                }.foregroundStyle(Theme.textSecondary).padding(.top, 20)
            }
        }
        .padding(.leading, Theme.gutter).padding(.trailing, 8).padding(.bottom, 24)
    }

    /// The meeting's day, worded as the note page words a date. The year is
    /// given when it is not this one; a shared note can be an old one.
    private var day: String? {
        let calendar = Calendar.current
        guard let components = note.date, let date = calendar.date(from: components) else { return nil }
        if calendar.isDateInToday(date) { return "Today" }
        if calendar.isDateInYesterday(date) { return "Yesterday" }
        let sameYear = calendar.component(.year, from: date) == calendar.component(.year, from: .now)
        return date.formatted(sameYear ? .dateTime.day().month(.abbreviated) : .dateTime.day().month(.abbreviated).year())
    }
}

/// A link to a note of one's own on a server this app is not signed in to.
struct NoteElsewhereView: View {
    let link: NoteLink
    @Environment(ServerNotesModel.self) private var server
    @Environment(ConnectionMode.self) private var mode

    var body: some View {
        ServerSignInPrompt(origin: link.origin, webURL: link.webURL,
                           offersSignIn: LinkNavigation.offersSignIn(signedIn: server.session != nil, demo: mode.isDemo))
    }
}

/// The page for a note this app cannot read: it is on a server the app is not
/// signed in to. It says which server, and offers the browser. Where signing
/// in would not replace an account in use (nobody is signed in, or the local
/// demo is showing), it offers that too, with the server's address filled in;
/// after the sign-in the note opens.
struct ServerSignInPrompt: View {
    let origin: ServerOrigin
    let webURL: URL
    let offersSignIn: Bool
    @Environment(ServerNotesModel.self) private var server
    @Environment(Router.self) private var router
    @Environment(\.dismiss) private var dismiss
    @Environment(\.openURL) private var openURL

    var body: some View {
        RailScaffold {
            ScreenPrompt(symbol: "lock", title: "This note is on \(origin.displayHost)", detail: "Sign in to that server to view it.",
                         caption: elsewhere, actions: offersSignIn ? [ScreenPrompt.Action(title: "Open in browser") { openURL(webURL) }] : [])
        } leading: {
            GlassCircleButton(systemImage: "chevron.left") { dismiss() }
        } bottom: {
            if offersSignIn {
                PrimaryBarButton(title: "Sign in", systemImage: "person.crop.circle") { router.signIn(to: origin) }
                    .accessibilityIdentifier("server-sign-in")
            } else {
                PrimaryBarButton(title: "Open in browser", systemImage: "safari") { openURL(webURL) }
            }
        }
    }

    /// The server the app is signed in to, when it is another one.
    private var elsewhere: String? {
        guard server.session != nil, let current = server.instance.flatMap(ServerOrigin.init(_:)), current != origin else { return nil }
        return "You are signed in to \(current.displayHost)."
    }
}
