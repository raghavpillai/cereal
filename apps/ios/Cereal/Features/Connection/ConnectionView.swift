import SwiftUI
import APIClient

/// What the sign-in form knows about the server whose address is in the field.
enum SignInDiscovery: Equatable {
    /// No address yet, or one that is still being typed.
    case idle
    case checking
    case ready(SignInMethods)
    case failed(String)
}

/// Asks the server whose address is in the field how people sign in to it,
/// once typing has paused. Only the answer for the address now in the field is
/// ever shown: each change starts a new question, and an answer to an earlier
/// one, however late it arrives, is dropped.
@MainActor @Observable
final class SignInDiscoveryModel {
    private(set) var state: SignInDiscovery = .idle
    @ObservationIgnored private var address = ""
    @ObservationIgnored private var question = 0
    @ObservationIgnored private var task: Task<Void, Never>?
    @ObservationIgnored private let pause: Duration
    @ObservationIgnored private let methods: @Sendable (InstanceURL) async throws -> SignInMethods

    init(pause: Duration = .milliseconds(500),
         methods: @escaping @Sendable (InstanceURL) async throws -> SignInMethods = { try await MeetingNotesAPI.signInMethods(instance: $0) }) {
        self.pause = pause
        self.methods = methods
    }

    /// The address in the field is now `text`. `again` asks about the same address once more.
    /// `typed` is false for an address nobody is typing (the one the sheet opens with): it is
    /// asked about at once, so the form's rows arrive while the sheet does, not after it has landed.
    func check(_ text: String, again: Bool = false, typed: Bool = true) {
        let text = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard again || text != address else { return }
        address = text
        question += 1
        let asked = question
        task?.cancel()
        // What was offered for the old address is withdrawn at once.
        state = .idle
        guard !text.isEmpty else { task = nil; return }
        task = Task { [weak self, pause, methods] in
            if typed { try? await Task.sleep(for: pause) }
            guard let self, asked == self.question else { return }
            let target: InstanceURL
            do { target = try InstanceURL(text) }
            catch { self.state = .failed(error.localizedDescription); return }
            self.state = .checking
            do {
                let found = try await methods(target)
                guard asked == self.question else { return }
                self.state = .ready(found)
            } catch {
                // A question that was replaced ends here too, cancelled or not.
                guard asked == self.question else { return }
                self.state = .failed(error.localizedDescription)
            }
        }
    }
}

/// The words of the two ways an account leaves this iPhone. Pure, so they can
/// be tested: the counts are what is really in the account's directory, and
/// the server is always named by its host.
enum AccountRemovalText {
    static func recordings(_ count: Int) -> String { count == 1 ? "1 recording" : "\(count) recordings" }

    static func signOutTitle(host: String) -> String { "Sign out of \(host)?" }

    /// What stays. The iPhone keeps no notes of an account, only the recordings made on it.
    static func signOutMessage(_ summary: AccountDataSummary, host: String, email: String) -> String {
        var lines: [String] = []
        if summary.recordings == 0 {
            lines.append("Your notes stay on \(host). No recordings from this account are saved on this iPhone.")
        } else {
            let stay = summary.recordings == 1 ? "stays" : "stay"
            let are = summary.recordings == 1 ? "is" : "are"
            lines.append("\(recordings(summary.recordings)) \(stay) on this iPhone, and \(are) here again when you sign back in.")
            if summary.unsent > 0 {
                lines.append(summary.unsent == 1 ? "1 of them has audio that was never uploaded." : "\(summary.unsent) of them have audio that was never uploaded.")
            }
        }
        if !email.isEmpty { lines.append("Signed in as \(email), as reported by \(host).") }
        return lines.joined(separator: "\n")
    }

    static func eraseTitle(host: String) -> String { "Remove \(host) and erase its data from this iPhone?" }

    /// What goes, counted. The notes on the server are never part of it.
    static func eraseMessage(_ summary: AccountDataSummary, host: String) -> String {
        var first: String
        if summary.recordings == 0 { first = "No recordings from this account are saved here." }
        else {
            first = "\(recordings(summary.recordings)) saved here"
            if summary.unsent > 0 { first += summary.unsent == 1 ? ", 1 of which was never uploaded" : ", \(summary.unsent) of which were never uploaded" }
            first += "."
        }
        var lines = [first]
        if summary.deletedOnServer > 0 {
            lines.append(summary.deletedOnServer == 1 ? "1 is of a note that was deleted on \(host)." : "\(summary.deletedOnServer) are of notes that were deleted on \(host).")
        }
        lines.append("Chats with this account on this iPhone are erased too. The notes on \(host) are not affected.")
        return lines.joined(separator: "\n")
    }

    static func eraseButton(_ summary: AccountDataSummary) -> String {
        guard summary.unsent > 0 else { return "Erase" }
        return summary.unsent == 1 ? "Erase, including 1 recording that exists nowhere else" : "Erase, including \(summary.unsent) recordings that exist nowhere else"
    }

    /// One line for a directory of data no account here owns.
    static func saved(_ item: SavedAccountData, now: Date = .now) -> String {
        var parts = [recordings(item.summary.recordings)]
        if item.summary.unsent > 0 { parts.append("\(item.summary.unsent) never uploaded") }
        parts.append(ByteCountFormatter.string(fromByteCount: item.bytes, countStyle: .file))
        if let changed = item.lastChanged { parts.append("last changed " + changed.formatted(date: .abbreviated, time: .omitted)) }
        return parts.joined(separator: " · ")
    }
}

struct ConnectionView: View {
    @Environment(AccountsModel.self) private var accounts
    @Environment(ConnectionMode.self) private var mode
    @Environment(Router.self) private var router
    @Environment(\.dismiss) private var dismiss
    @State private var instance = ""
    @State private var token = ""
    @State private var email = ""
    @State private var password = ""
    @State private var advanced = false
    @State private var busy = false
    @State private var error: String?
    @State private var discovery = SignInDiscoveryModel()
    /// The "Checking…" row waits a moment before it appears. Most servers answer
    /// within it, and the form then changes once (to its sign-in rows) instead of
    /// twice while the sheet is still rising.
    @State private var checkingShown = false
    /// The mark and title spring up once the sheet is here.
    @State private var arrived = false
    @State private var browser = BrowserSignInSession()
    /// True while the sign-in in progress is the one in the browser, so its own button shows it.
    @State private var inBrowser = false
    /// The account a sheet is asking about, with what it has on this iPhone counted when the sheet was opened.
    @State private var signingOut: Leaving?
    @State private var erasing: Leaving?
    @State private var erasingSaved: SavedAccountData?
    @Environment(\.motion) private var motion

    struct Leaving: Identifiable {
        let id: AccountID
        let host: String
        let email: String
        let summary: AccountDataSummary
    }

    private var address: String { instance.trimmingCharacters(in: .whitespacesAndNewlines) }

    /// The address field. Each change is handed to discovery as the field makes
    /// it, not when the view next compares values: a character typed while the
    /// form was redrawing for an earlier answer was otherwise never asked about.
    private var addressField: Binding<String> {
        Binding(get: { instance }, set: { instance = $0; discovery.check($0) })
    }

    /// Runs a sign-in. Whoever signs in is checked with their server first; an
    /// account that is here is only replaced once that has succeeded.
    private func signIn(browser viaBrowser: Bool = false, _ work: @escaping () async throws -> AccountID) {
        busy = true; inBrowser = viaBrowser; error = nil
        Task {
            defer { busy = false; inBrowser = false }
            do {
                let account = try await work()
                password = ""; token = ""
                #if DEBUG
                mode.isDemo = false
                #endif
                // A note that was waiting for this sign-in opens; otherwise Home.
                router.path = LinkNavigation.pathAfterSignIn(router.path, account: accounts.linkAccounts.first { $0.id == account })
                // Signed in: the sheet goes as this is felt.
                Haptics.shared.play(.done)
                dismiss()
            } catch BrowserSignInError.cancelled {
            } catch is CancellationError {
            } catch {
                password = ""; token = ""
                self.error = error.localizedDescription
                Haptics.shared.play(.error)
            }
        }
    }

    /// Signs in on the server's own page, in the system browser, by whichever
    /// method the server offers there. The session that comes back is kept
    /// exactly as one from the password form is.
    private func continueInBrowser() {
        signIn(browser: true) {
            let attempt = try BrowserSignIn(instance: InstanceURL(instance))
            let answer = try await browser.answer(from: attempt.loginURL)
            return try await accounts.signIn(browser: attempt, callback: answer)
        }
    }

    private func ask(_ session: AccountSession, erase: Bool) {
        let item = Leaving(id: session.id, host: session.host, email: session.record.email, summary: accounts.dataSummary(for: session.id))
        if erase { erasing = item } else { signingOut = item }
    }

    private func leave(_ item: Leaving, erase: Bool) {
        busy = true; error = nil
        // Signing out and erasing are confirmed in an alert first; this is that confirmation.
        Haptics.shared.play(.destructive)
        Task {
            do { if erase { try await accounts.erase(item.id) } else { try await accounts.signOut(item.id) } }
            catch { self.error = error.localizedDescription; Haptics.shared.play(.error) }
            if accounts.sessions.isEmpty { router.path = [] }
            busy = false
        }
    }

    /// A button's loader. It keeps its place in the label, so the row is the same
    /// height with and without it, and buds in when the wait starts: a view that
    /// is inserted into a list row's label is not given its transition.
    private func rowLoader(_ shown: Bool, color: Color) -> some View {
        CerealLoader(size: 20, color: color, running: shown)
            .scaleEffect(shown || motion.reduceMotion ? 1 : 0.6).opacity(shown ? 1 : 0)
            .animation(motion.animation(.gooey, entering: shown), value: shown)
    }

    var body: some View {
        NavigationStack {
            Form {
                // The mark leads the sheet, alive: it is the wait as well as the name.
                Section {
                    VStack(spacing: 10) {
                        CerealLoader(size: 52)
                        Text(accounts.sessions.isEmpty ? "Sign in to Cereal" : "Your account")
                            .font(.system(.title2, design: .serif)).foregroundStyle(Theme.textPrimary)
                    }
                    .frame(maxWidth: .infinity)
                    .padding(.top, 8)
                    .scaleEffect(arrived || motion.reduceMotion ? 1 : 0.7).opacity(arrived ? 1 : 0)
                    .listRowBackground(Color.clear)
                    .listRowInsets(EdgeInsets())
                }
                Section {
                    // Verbatim: as a localized key the example address is read as Markdown,
                    // which makes it a link and draws it in the tint colour.
                    TextField(text: addressField, prompt: Text(verbatim: "https://notes.example.com")) { Text("Server address") }
                        .textContentType(.URL).keyboardType(.URL)
                        .textInputAutocapitalization(.never).autocorrectionDisabled()
                        .accessibilityLabel("Instance URL")
                        .accessibilityIdentifier("connection-instance")
                    if (try? InstanceURL(address)).map(ServerAddress.kind(of:)) == .privateNetwork {
                        Text(ServerAddress.notEncrypted).font(.footnote).foregroundStyle(Theme.textSecondary)
                            .accessibilityIdentifier("connection-not-encrypted")
                    }
                    switch discovery.state {
                    case .idle: EmptyView()
                    case .checking:
                        if checkingShown {
                            HStack { Text("Checking how this server signs in…").foregroundStyle(Theme.textSecondary); Spacer(); CerealLoader(size: 20, color: Theme.textSecondary) }
                                .transition(motion.rise)
                        }
                    case .failed(let message):
                        Text(message).foregroundStyle(Theme.danger)
                        Button("Try again") { discovery.check(address, again: true) }
                    case .ready(let methods):
                        if methods.password {
                            TextField("Email", text: $email)
                                .textContentType(.username).keyboardType(.emailAddress)
                                .textInputAutocapitalization(.never).autocorrectionDisabled()
                            SecureField("Password", text: $password).textContentType(.password)
                            Button {
                                let address = instance, user = email.trimmingCharacters(in: .whitespacesAndNewlines), secret = password
                                signIn { try await accounts.signIn(instance: address, email: user, password: secret) }
                            } label: {
                                HStack { Text("Sign in"); Spacer(); rowLoader(busy && !inBrowser, color: Theme.textPrimary) }
                            }.buttonStyle(.glassProminent).tint(Theme.accent)
                                .foregroundStyle(busy || email.isEmpty || password.isEmpty || address.isEmpty ? Theme.textTertiary : Theme.onAccent)
                                .disabled(busy || email.isEmpty || password.isEmpty || address.isEmpty)
                                // The line under the button starts with the others, not at the button's own text.
                                .alignmentGuide(.listRowSeparatorLeading) { $0[.leading] }
                        }
                        if let label = methods.browserLabel {
                            if methods.password {
                                // Beside the password form the browser is the other way in.
                                Button(action: continueInBrowser) {
                                    HStack { Text(label); Spacer(); rowLoader(inBrowser, color: Theme.textSecondary) }
                                }.accessibilityIdentifier("connection-browser")
                            } else {
                                Button(action: continueInBrowser) {
                                    HStack { Text(label); Spacer(); rowLoader(busy, color: Theme.textPrimary) }
                                }.buttonStyle(.glassProminent).tint(Theme.accent)
                                    .foregroundStyle(busy ? Theme.textTertiary : Theme.onAccent)
                                    .alignmentGuide(.listRowSeparatorLeading) { $0[.leading] }
                                    .accessibilityIdentifier("connection-browser")
                            }
                        }
                    }
                } header: { Text("Your server") } footer: {
                    Text("Sign in to your account on your self-hosted server. Your session is kept in this device’s Keychain. Passwords are never saved. Use HTTPS, or plain HTTP on a private network.")
                }
                DisclosureGroup("Advanced: access token", isExpanded: $advanced) {
                    SecureField("Instance access token", text: $token)
                        .textInputAutocapitalization(.never).autocorrectionDisabled()
                    Button("Connect with token") {
                        let address = instance, secret = token
                        signIn { try await accounts.connect(instance: address, token: secret) }
                    }.disabled(token.isEmpty || instance.isEmpty || busy)
                }
                if let error {
                    Section { Text(error).foregroundStyle(Theme.danger).accessibilityAddTraits(.updatesFrequently) }
                }
                if let problem = accounts.storageError {
                    Section { Text(problem).font(.footnote).foregroundStyle(Theme.danger) }
                }
                // One section per account, in the person's order.
                ForEach(accounts.sessions, id: \.key) { session in
                    Section(accounts.showsAccountLabels ? session.host : "Configured instance") {
                        Text(session.record.instanceURL).font(.footnote).textSelection(.enabled)
                        if !session.record.name.isEmpty || !session.record.email.isEmpty {
                            Text(session.record.name).font(.headline)
                            Text(session.record.email).font(.footnote)
                        }
                        switch session.status {
                        case .ready:
                            if let user = session.notes.session?.user {
                                LabeledContent("Instance role", value: user.role.rawValue.capitalized)
                                    .font(.caption).foregroundStyle(Theme.textSecondary)
                            }
                            NavigationLink("Workspaces") { AccountScope(session.id) { WorkspacesView() } }
                                .accessibilityIdentifier("connection-workspaces")
                            if let connected = session.notes.instance {
                                Link("Manage calendar connections", destination: connected.url.appendingPathComponent("account"))
                            }
                        case .checking:
                            HStack { Text("Checking your session…").foregroundStyle(Theme.textSecondary); Spacer(); CerealLoader(size: 20, color: Theme.textSecondary) }
                        case .unreachable(let failure):
                            Text(failure.localizedDescription).font(.footnote).foregroundStyle(Theme.textSecondary)
                            Button("Try again") { Task { await accounts.retryUnreachable() } }
                        case .needsSignIn:
                            Text("Sign in again above to continue. Nothing is sent to \(session.host) until you do.")
                                .font(.footnote).foregroundStyle(Theme.textSecondary)
                                .accessibilityIdentifier("connection-needs-sign-in")
                        }
                        Button("Sign out", role: .destructive) { ask(session, erase: false) }
                            .accessibilityIdentifier("connection-sign-out")
                        Button("Remove account and erase its data…", role: .destructive) { ask(session, erase: true) }
                            .accessibilityIdentifier("connection-erase")
                    }
                }
                if !accounts.signedOut.isEmpty {
                    Section {
                        ForEach(accounts.signedOut) { item in
                            VStack(alignment: .leading, spacing: 4) {
                                // What the file in the directory says. The address is shown, never trusted.
                                Text(item.host ?? "Unknown server")
                                if let email = item.account?.email, !email.isEmpty { Text(email).font(.footnote).foregroundStyle(Theme.textSecondary) }
                                Text(AccountRemovalText.saved(item)).font(.caption).foregroundStyle(Theme.textSecondary)
                                HStack(spacing: 20) {
                                    Button("Sign in") {
                                        // The address goes into the field above, in full, for the person to see before anything is sent.
                                        instance = item.account?.url ?? ""
                                        discovery.check(instance, typed: false)
                                    }.buttonStyle(.borderless)
                                    Button("Erase…", role: .destructive) { erasingSaved = item }.buttonStyle(.borderless)
                                }.padding(.top, 2)
                            }
                        }
                    } header: { Text("Signed out, with data on this iPhone") }
                        .accessibilityIdentifier("connection-signed-out")
                }
                if !accounts.olderData.isEmpty {
                    Section {
                        ForEach(accounts.olderData) { item in
                            VStack(alignment: .leading, spacing: 4) {
                                Text(AccountRemovalText.saved(item)).font(.footnote)
                                Button("Erase…", role: .destructive) { erasingSaved = item }.buttonStyle(.borderless).padding(.top, 2)
                            }
                        }
                    } header: { Text("Older saved data") } footer: {
                        Text("Recordings saved by an earlier version, for accounts that are not signed in here. They are kept until you erase them.")
                    }
                }
            }
            .disabled(busy)
            .animation(motion.snappy, value: busy)
            .animation(motion.snappy, value: error)
            .animation(motion.snappy, value: discovery.state)
            .animation(motion.snappy, value: checkingShown)
            .task(id: discovery.state == .checking) {
                checkingShown = false
                guard discovery.state == .checking else { return }
                try? await Task.sleep(for: .milliseconds(300))
                if !Task.isCancelled { checkingShown = true }
            }
            .onChange(of: address, initial: true) { _, text in discovery.check(text) }
            .animation(motion.settle, value: accounts.signature)
            .navigationTitle("Connection")
            .navigationBarTitleDisplayMode(.inline)
            // The app needs a server: with no account here there is nothing to go back to.
            .toolbar { if !accounts.sessions.isEmpty { ToolbarItem(placement: .cancellationAction) { Button("Done") { dismiss() }.disabled(busy) } } }
            .interactiveDismissDisabled(busy || accounts.sessions.isEmpty)
            .onAppear {
                instance = router.signInAddress ?? accounts.sessions.first?.record.instanceURL ?? ""
                discovery.check(instance, typed: false)
                accounts.refreshSavedData()
            }
            // After the first frame, as the sheet climbs: set in `onAppear` the change is
            // part of that first frame, and the mark is simply there.
            .task {
                try? await Task.sleep(for: .milliseconds(50))
                withAnimation(motion.gooey) { arrived = true }
            }
            .onDisappear { password = ""; token = ""; router.signInAddress = nil }
            // Sign out: says what stays, and offers the other action in the same place.
            .alert(AccountRemovalText.signOutTitle(host: signingOut?.host ?? ""), isPresented: Binding(get: { signingOut != nil }, set: { if !$0 { signingOut = nil } }), presenting: signingOut) { item in
                Button("Sign out", role: .destructive) { leave(item, erase: false) }
                Button("Remove and erase instead…") {
                    // The second sheet opens once this one has closed.
                    Task { try? await Task.sleep(for: .milliseconds(350)); erasing = item }
                }
                Button("Cancel", role: .cancel) {}
            } message: { item in Text(AccountRemovalText.signOutMessage(item.summary, host: item.host, email: item.email)) }
            .alert(AccountRemovalText.eraseTitle(host: erasing?.host ?? ""), isPresented: Binding(get: { erasing != nil }, set: { if !$0 { erasing = nil } }), presenting: erasing) { item in
                Button(AccountRemovalText.eraseButton(item.summary), role: .destructive) { leave(item, erase: true) }
                Button("Cancel", role: .cancel) {}
            } message: { item in Text(AccountRemovalText.eraseMessage(item.summary, host: item.host)) }
            .alert("Erase this data from this iPhone?", isPresented: Binding(get: { erasingSaved != nil }, set: { if !$0 { erasingSaved = nil } }), presenting: erasingSaved) { item in
                Button(AccountRemovalText.eraseButton(item.summary), role: .destructive) {
                    Haptics.shared.play(.destructive)
                    do { try accounts.eraseSaved(item) } catch { self.error = error.localizedDescription; Haptics.shared.play(.error) }
                }
                Button("Cancel", role: .cancel) {}
            } message: { item in
                Text((item.host.map { "Saved for \($0). " } ?? "") + AccountRemovalText.saved(item) + ". Notes on a server are not affected.")
            }
        }
        .glassSheet()
    }
}
