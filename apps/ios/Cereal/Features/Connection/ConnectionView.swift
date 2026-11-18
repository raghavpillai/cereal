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

/// What is typed in the address field, read the way a person means it.
enum SignInAddress {
    /// The address with the scheme a person leaves out: `notes.example.com` is
    /// `https://notes.example.com`. An address that only exists on a private
    /// network (an IP address there, a `.local` name, a name with no dot) is
    /// plain HTTP, which is what such a server speaks.
    static func completed(_ text: String) -> String {
        let text = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty, !text.contains("://") else { return text }
        let host = hostPart(text)
        let local = ServerAddress.kind(scheme: "http", host: host) != .publicHTTP
        return (local ? "http://" : "https://") + text
    }

    /// The host of an address typed without a scheme: up to the first slash, without a port.
    private static func hostPart(_ text: String) -> String {
        var host = String(text.prefix { $0 != "/" })
        if host.hasPrefix("["), let end = host.firstIndex(of: "]") { return String(host[...end]) }
        if let colon = host.lastIndex(of: ":"), host[host.index(after: colon)...].allSatisfy(\.isNumber) { host = String(host[..<colon]) }
        return host
    }

    /// True while the text is plainly still being typed: the start of a scheme,
    /// a single word with no dot, or something that ends where a name cannot.
    /// Pressing Return asks about it anyway.
    static func isUnfinished(_ text: String) -> Bool {
        let text = text.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        if text.isEmpty { return true }
        if "https://".hasPrefix(text) || "http://".hasPrefix(text) { return true }
        let rest = text.components(separatedBy: "://").last ?? text
        if rest.isEmpty { return true }
        if let last = rest.last, ".-:".contains(last) { return true }
        // "notes", "192": one word. A name with a dot, or with a port, can be judged.
        return !text.contains("://") && !rest.contains(".") && !rest.contains(":")
    }

    /// What is wrong with an address that cannot be one, in words that name it.
    static func problem(_ text: String, error: Error) -> String {
        guard (error as? APIClientError) == .invalidInstanceURL else { return error.localizedDescription }
        let text = text.trimmingCharacters(in: .whitespacesAndNewlines)
        if text.contains(where: \.isWhitespace) { return "A server address has no spaces in it." }
        if text.contains("?") { return "Leave out the ? and everything after it." }
        if text.contains("#") { return "Leave out the # and everything after it." }
        if text.contains("@") { return "Leave out the name before the @. You sign in after the address is found." }
        return "That does not look like a server address. One looks like notes.example.com."
    }
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
            // Half a word is not a wrong address, and not one to ask about either:
            // nothing happens until the text can be judged, or Return is pressed.
            if typed, SignInAddress.isUnfinished(text) { return }
            let target: InstanceURL
            do { target = try InstanceURL(SignInAddress.completed(text)) }
            catch { self.state = .failed(SignInAddress.problem(text, error: error)); return }
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

/// The sheet for the account and its server. Signed out it is the sign-in
/// form; signed in it opens on the account, with the form one row away under
/// "Add or change server". It is drawn with the Settings cards, like every
/// page around it.
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
    /// What went wrong signing in, shown under the button that was pressed.
    @State private var signInError: String?
    /// What went wrong with an account already here, shown under its card.
    @State private var accountError: String?
    @State private var discovery = SignInDiscoveryModel()
    /// The mark and title spring up once the sheet is here.
    @State private var arrived = false
    @State private var browser = BrowserSignInSession()
    /// True while the sign-in in progress is the one in the browser, so its own button shows it.
    @State private var inBrowser = false
    /// True while it is the one with an access token.
    @State private var withToken = false
    /// Which face the sheet shows. It follows the accounts, except while a sign-in
    /// is completing: the sheet is on its way out then and leaves as it was,
    /// without a "Done" appearing in it as it goes.
    @State private var showsAccount = false
    @State private var signingIn = false
    /// The sign-in form under an account ("Add or change server").
    @State private var addingServer = false
    @State private var workspaces: AccountID?
    /// The account a sheet is asking about, with what it has on this iPhone counted when the sheet was opened.
    @State private var signingOut: Leaving?
    @State private var erasing: Leaving?
    @State private var erasingSaved: SavedAccountData?
    @FocusState private var focus: Field?
    @Environment(\.motion) private var motion

    enum Field: Hashable { case address, email, password, token }

    struct Leaving: Identifiable {
        let id: AccountID
        let host: String
        let email: String
        let summary: AccountDataSummary
    }

    private var address: String { instance.trimmingCharacters(in: .whitespacesAndNewlines) }
    /// The address as it is sent: with the scheme a person leaves out.
    private var completedAddress: String { SignInAddress.completed(instance) }

    /// The address field. Each change is handed to discovery as the field makes
    /// it, not when the view next compares values: a character typed while the
    /// form was redrawing for an earlier answer was otherwise never asked about.
    private var addressField: Binding<String> {
        Binding(get: { instance }, set: { instance = $0; discovery.check($0) })
    }

    /// Runs a sign-in. Whoever signs in is checked with their server first; an
    /// account that is here is only replaced once that has succeeded.
    private func signIn(browser viaBrowser: Bool = false, refused: String, _ work: @escaping () async throws -> AccountID) {
        busy = true; signingIn = true; inBrowser = viaBrowser; signInError = nil
        let field = focus
        Task {
            defer { busy = false; inBrowser = false; withToken = false }
            do {
                let account = try await work()
                // What was typed is cleared when the sheet has gone (`onDisappear`), not while it is still in view.
                #if DEBUG
                mode.isDemo = false
                #endif
                // A note that was waiting for this sign-in opens; otherwise Home.
                router.path = LinkNavigation.pathAfterSignIn(router.path, account: accounts.linkAccounts.first { $0.id == account })
                // Signed in: the sheet goes as this is felt.
                Haptics.shared.play(.done)
                dismiss()
                return
            } catch BrowserSignInError.cancelled {
            } catch is CancellationError {
            } catch {
                // The server's refusal of what was typed is said in those terms, not as a token's.
                signInError = (error as? APIClientError) == .unauthorized ? refused : error.localizedDescription
                Haptics.shared.play(.error)
                // The next try starts where this one was typed: the secret is cleared and its field has the keyboard.
                if field == .password || field == .email { password = ""; focus = .password }
                if field == .token { token = ""; focus = .token }
            }
            signingIn = false
        }
    }

    private func signInWithPassword() {
        let user = email.trimmingCharacters(in: .whitespacesAndNewlines), secret = password, address = completedAddress
        guard !busy, !user.isEmpty, !secret.isEmpty, !address.isEmpty else { return }
        signIn(refused: "That email and password did not match.") { try await accounts.signIn(instance: address, email: user, password: secret) }
    }

    /// Signs in on the server's own page, in the system browser, by whichever
    /// method the server offers there. The session that comes back is kept
    /// exactly as one from the password form is.
    private func continueInBrowser() {
        let address = completedAddress
        signIn(browser: true, refused: "The server did not accept that sign-in.") {
            let attempt = try BrowserSignIn(instance: InstanceURL(address))
            let answer = try await browser.answer(from: attempt.loginURL)
            return try await accounts.signIn(browser: attempt, callback: answer)
        }
    }

    private func ask(_ session: AccountSession, erase: Bool) {
        let item = Leaving(id: session.id, host: session.host, email: session.record.email, summary: accounts.dataSummary(for: session.id))
        if erase { erasing = item } else { signingOut = item }
    }

    private func leave(_ item: Leaving, erase: Bool) {
        busy = true; accountError = nil
        // Signing out and erasing are confirmed in an alert first; this is that confirmation.
        Haptics.shared.play(.destructive)
        Task {
            do { if erase { try await accounts.erase(item.id) } else { try await accounts.signOut(item.id) } }
            catch { accountError = error.localizedDescription; Haptics.shared.play(.error) }
            if accounts.sessions.isEmpty { router.path = [] }
            busy = false
        }
    }

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    Replaced(id: showsAccount) {
                        VStack(alignment: .leading, spacing: 0) {
                            if showsAccount { accountPage } else { signInPage }
                        }
                    }
                    savedData
                }
                .padding(.horizontal, 16).padding(.top, 8).padding(.bottom, 32)
                // Rows of the form arrive and leave as the server answers; errors open their own space.
                .animates(discovery.state)
                .animates(signInError)
                .animates(accountError)
                .animates(addingServer)
                .animates(advanced)
                .animates(accounts.signature, .settle)
            }
            .scrollIndicators(.hidden)
            .scrollDismissesKeyboard(.interactively)
            // A navigation stack paints the system's white behind its pages: the warm page goes over it.
            .background(Theme.sheet.ignoresSafeArea())
            // A wait blocks touches without greying the form: the pressed button shows the work.
            .allowsHitTesting(!busy)
            .dynamicTypeSize(...DynamicTypeSize.accessibility1)
            .navigationTitle(showsAccount ? Wording.accountSheet : "")
            .navigationBarTitleDisplayMode(.inline)
            // The app needs a server: with no account here there is nothing to go back to.
            .toolbar { if showsAccount { ToolbarItem(placement: .cancellationAction) { Button("Done") { dismiss() }.disabled(busy) } } }
            .interactiveDismissDisabled(busy || accounts.sessions.isEmpty)
            .navigationDestination(item: $workspaces) { account in
                // A pushed page takes its environment from the stack, not from the row that opened it.
                AccountScope(account) { WorkspacesView() }
            }
            .onAppear {
                showsAccount = !accounts.sessions.isEmpty
                let again = accounts.sessions.first { $0.status == .needsSignIn }
                instance = router.signInAddress ?? accounts.sessions.first?.record.instanceURL ?? ""
                // The app knows who is signing in again: the email is already there.
                if let known = router.signInEmail ?? again?.record.email, email.isEmpty { email = known }
                addingServer = router.signInAddress != nil || again != nil
                discovery.check(instance, typed: false)
                accounts.refreshSavedData()
            }
            .onChange(of: accounts.sessions.isEmpty) { _, none in
                guard !signingIn else { return }
                showsAccount = !none
                if none { addingServer = false; discovery.check(instance, again: true, typed: false) }
            }
            .onDisappear { password = ""; token = ""; router.signInAddress = nil; router.signInEmail = nil }
            .haptic(.error, trigger: discovery.state) { _, new in if case .failed = new { true } else { false } }
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
                    do { try accounts.eraseSaved(item) } catch { accountError = error.localizedDescription; Haptics.shared.play(.error) }
                }
                Button("Cancel", role: .cancel) {}
            } message: { item in
                Text((item.host.map { "Saved for \($0). " } ?? "") + AccountRemovalText.saved(item) + ". Notes on a server are not affected.")
            }
        }
        // The warm page of every other screen, not the system's white sheet.
        .glassSheet(background: Theme.sheet)
    }

    // MARK: Signed out

    /// The mark leads the sheet, alive: it is the wait as well as the name.
    @ViewBuilder private var signInPage: some View {
        VStack(spacing: 10) {
            CerealLoader(size: 52)
            Text("Sign in to Cereal").displayFont(24).foregroundStyle(Theme.textPrimary)
                .accessibilityAddTraits(.isHeader)
        }
        .frame(maxWidth: .infinity)
        .padding(.top, 20).padding(.bottom, 6)
        .scaleEffect(arrived || motion.reduceMotion ? 1 : 0.7).opacity(arrived ? 1 : 0)
        // From the header's own first frame, so there is always a state to spring from:
        // at launch the sheet is up before a timer in the form could have fired.
        .onAppear { withAnimation(motion.gooey.delay(0.12)) { arrived = true } }
        signInForm
    }

    /// The address, what its server offers, and the way in. Used signed out, and under an account.
    @ViewBuilder private var signInForm: some View {
        let methods: SignInMethods? = if case .ready(let found) = discovery.state { found } else { nil }
        let takesPassword = methods?.password == true
        SettingsSection("Server address") {
            // Verbatim: as a localized key the example address is read as Markdown,
            // which makes it a link and draws it in the tint colour.
            CardField(divider: takesPassword) {
                TextField(text: addressField, prompt: Text(verbatim: "notes.example.com").foregroundStyle(Theme.textTertiary)) { Text("Server address") }
                    .textContentType(.URL).keyboardType(.URL)
                    .textInputAutocapitalization(.never).autocorrectionDisabled()
                    .submitLabel(takesPassword ? .next : .go)
                    .focused($focus, equals: .address)
                    // Return asks about the address as it stands, finished or not.
                    .onSubmit { if takesPassword { focus = .email } else { discovery.check(instance, again: true, typed: false) } }
                    .accessibilityLabel("Server address")
                    .accessibilityIdentifier("connection-instance")
            }
            // The one place the form waits: nothing for a moment, then the loader, then what the server offers.
            Loading(while: discovery.state == .checking) {
                HStack(spacing: 10) {
                    CerealLoader(size: 20, color: Theme.textSecondary)
                    Text("Checking how this server signs in…").uiFont(15).foregroundStyle(Theme.textSecondary)
                    Spacer(minLength: 0)
                }
                .padding(.horizontal, 18).frame(minHeight: SettingsMetrics.rowHeight)
            } then: {
                VStack(alignment: .leading, spacing: 0) {
                    if case .failed(let message) = discovery.state {
                        VStack(alignment: .leading, spacing: 0) {
                            Text(message).uiFont(15).foregroundStyle(Theme.danger)
                                .accessibilityIdentifier("connection-address-problem")
                            Button(Wording.retry) { discovery.check(instance, again: true, typed: false) }
                                .uiFont(15, weight: .medium).foregroundStyle(Theme.accentText)
                                .frame(minHeight: 44).contentShape(Rectangle()).buttonStyle(.squash)
                        }
                        .padding(.horizontal, 18).padding(.top, 4)
                    }
                    if takesPassword {
                        CardField {
                            TextField("Email", text: $email, prompt: Text("Email").foregroundStyle(Theme.textTertiary))
                                .accessibilityLabel("Email")
                                .textContentType(.username).keyboardType(.emailAddress)
                                .textInputAutocapitalization(.never).autocorrectionDisabled()
                                .submitLabel(.next).focused($focus, equals: .email)
                                .onSubmit { focus = .password }
                                .accessibilityIdentifier("connection-email")
                        }
                        CardField(divider: false) {
                            SecureField("Password", text: $password, prompt: Text("Password").foregroundStyle(Theme.textTertiary))
                                .accessibilityLabel("Password")
                                .textContentType(.password)
                                .submitLabel(.go).focused($focus, equals: .password)
                                .onSubmit { signInWithPassword() }
                                .accessibilityIdentifier("connection-password")
                        }
                    }
                }
            }
        }
        if (try? InstanceURL(completedAddress)).map(ServerAddress.kind(of:)) == .privateNetwork {
            CardNote(ServerAddress.notEncrypted).accessibilityIdentifier("connection-not-encrypted")
        }
        if let methods {
            VStack(alignment: .leading, spacing: 0) {
                if methods.password {
                    PrimaryActionButton(title: "Sign in", busy: busy && !inBrowser && !withToken, action: signInWithPassword)
                        .disabled(email.isEmpty || password.isEmpty || address.isEmpty)
                        .accessibilityIdentifier("connection-sign-in")
                }
                if let label = methods.browserLabel, !methods.password {
                    PrimaryActionButton(title: label, busy: inBrowser, action: continueInBrowser)
                        .accessibilityIdentifier("connection-browser")
                }
                // Directly under the button that was pressed.
                if let signInError {
                    Text(signInError).uiFont(15).foregroundStyle(Theme.danger)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(.horizontal, 14).padding(.top, 10)
                        .accessibilityIdentifier("connection-sign-in-error")
                        .transition(motion.rise)
                }
                if let label = methods.browserLabel, methods.password {
                    // Beside the password form the browser is the other way in.
                    Button(action: continueInBrowser) {
                        HStack(spacing: 8) {
                            Text(label).uiFont(16, weight: .medium)
                            if inBrowser { CerealLoader(size: 18, color: Theme.accentText).transition(motion.pop) }
                        }
                        .foregroundStyle(Theme.accentText)
                        .frame(maxWidth: .infinity, minHeight: 48).contentShape(Rectangle())
                    }
                    .buttonStyle(.squash)
                    .padding(.top, 6)
                    .accessibilityIdentifier("connection-browser")
                }
            }
            .padding(.top, 14)
            .animates(inBrowser)
            .transition(motion.rise)
        }
        CardNote("Sign in to your account on your self-hosted server. Your session is kept in this iPhone’s Keychain. Passwords are never saved. Use HTTPS, or plain HTTP on a private network.")
        Button { advanced.toggle() } label: {
            HStack(spacing: 6) {
                Text("Use an access token instead").uiFont(15)
                Image(systemName: "chevron.down").font(.system(size: 11, weight: .semibold))
                    .rotationEffect(.degrees(advanced ? 180 : 0))
            }
            .foregroundStyle(Theme.textSecondary)
            .frame(minHeight: 44).contentShape(Rectangle())
        }
        .buttonStyle(.squash)
        .padding(.horizontal, 14).padding(.top, 6)
        .accessibilityIdentifier("connection-advanced")
        if advanced {
            VStack(alignment: .leading, spacing: 0) {
                SettingsCard {
                    CardField(divider: false) {
                        SecureField("Access token", text: $token, prompt: Text("Access token").foregroundStyle(Theme.textTertiary))
                            .textInputAutocapitalization(.never).autocorrectionDisabled()
                            .focused($focus, equals: .token)
                            .accessibilityIdentifier("connection-token")
                    }
                }
                Button("Connect with token") {
                    let address = completedAddress, secret = token
                    focus = .token; withToken = true
                    signIn(refused: "The server did not accept that access token.") { try await accounts.connect(instance: address, token: secret) }
                }
                .uiFont(16, weight: .medium).foregroundStyle(token.isEmpty || address.isEmpty ? Theme.textTertiary : Theme.accentText)
                .frame(maxWidth: .infinity, minHeight: 48).contentShape(Rectangle()).buttonStyle(.squash)
                .disabled(token.isEmpty || address.isEmpty)
            }
            .transition(motion.rise)
        }
    }

    // MARK: Signed in

    /// One block per account, in the person's order: who it is, what it can open, and the two ways it leaves.
    @ViewBuilder private var accountPage: some View {
        ForEach(accounts.sessions, id: \.key) { session in
            VStack(spacing: 6) {
                AvatarView(name: session.record.name, size: 72)
                    .padding(.bottom, 6)
                if !session.record.name.isEmpty {
                    Text(session.record.name).displayFont(24).foregroundStyle(Theme.textPrimary)
                        .accessibilityAddTraits(.isHeader)
                }
                if !session.record.email.isEmpty {
                    Text(session.record.email).uiFont(15).foregroundStyle(Theme.textSecondary)
                }
                Text(session.record.instanceURL).uiFont(13).foregroundStyle(Theme.textTertiary)
                    .textSelection(.enabled)
                    .accessibilityIdentifier("connection-server")
            }
            .multilineTextAlignment(.center)
            .frame(maxWidth: .infinity)
            .padding(.top, 16)
            SettingsSection(accounts.showsAccountLabels ? session.host : "Account") {
                switch session.status {
                case .ready:
                    SettingsRow("Workspaces", systemImage: "person.2") { workspaces = session.id }
                        .accessibilityIdentifier("connection-workspaces")
                    if let connected = session.notes.instance {
                        Link(destination: connected.url.appendingPathComponent("account")) {
                            SettingsRowLabel("Calendar connections", systemImage: "calendar", trailing: .external)
                        }
                        .buttonStyle(.row)
                    }
                    if let user = session.notes.session?.user {
                        SettingsRowLabel("Server role", systemImage: "key", trailing: .text(user.role.rawValue.capitalized))
                            .accessibilityElement(children: .combine)
                    }
                case .checking:
                    HStack(spacing: 10) {
                        CerealLoader(size: 20, color: Theme.textSecondary)
                        Text("Checking your session…").uiFont(15).foregroundStyle(Theme.textSecondary)
                    }
                    .padding(.horizontal, 18).frame(minHeight: SettingsMetrics.rowHeight)
                case .unreachable(let failure):
                    VStack(alignment: .leading, spacing: 0) {
                        Text(failure.localizedDescription).uiFont(15).foregroundStyle(Theme.textSecondary)
                        Button(Wording.retry) { Task { await accounts.retryUnreachable() } }
                            .uiFont(15, weight: .medium).foregroundStyle(Theme.accentText)
                            .frame(minHeight: 44).contentShape(Rectangle()).buttonStyle(.squash)
                    }
                    .padding(.horizontal, 18).padding(.top, 8)
                case .needsSignIn:
                    Text("Sign in again below to continue. Nothing is sent to \(session.host) until you do.")
                        .uiFont(15).foregroundStyle(Theme.textSecondary)
                        .padding(.horizontal, 18).padding(.vertical, 10)
                        .accessibilityIdentifier("connection-needs-sign-in")
                }
            }
            // Signing in again is what this account needs: the form comes first, filled in, not under the ways to leave.
            if session.status == .needsSignIn {
                VStack(alignment: .leading, spacing: 0) { signInForm }
            }
            SettingsCard {
                SettingsRow("Sign out", systemImage: "rectangle.portrait.and.arrow.right", trailing: .none, destructive: true) { ask(session, erase: false) }
                    .accessibilityIdentifier("connection-sign-out")
                SettingsRow("Remove and erase data…", systemImage: "trash", trailing: .none, destructive: true) { ask(session, erase: true) }
                    .accessibilityIdentifier("connection-erase")
            }
            .padding(.top, 14)
        }
        if let accountError {
            CardNote(accountError, color: Theme.danger).accessibilityAddTraits(.updatesFrequently).transition(motion.rise)
        }
        if let problem = accounts.storageError { CardNote(problem, color: Theme.danger) }
        if !accounts.sessions.contains(where: { $0.status == .needsSignIn }) {
        SettingsCard {
            RowButton { addingServer.toggle() } label: {
                HStack(spacing: 14) {
                    SettingsIconTile(systemImage: "server.rack")
                    Text("Add or change server").uiFont(SettingsMetrics.label).foregroundStyle(Theme.textPrimary)
                    Spacer(minLength: 8)
                    Image(systemName: "chevron.down").font(.system(size: 13, weight: .medium)).foregroundStyle(Theme.textSecondary)
                        .rotationEffect(.degrees(addingServer ? 180 : 0))
                }
                .padding(.leading, 11).padding(.trailing, 18)
                .frame(minHeight: SettingsMetrics.rowHeight).contentShape(Rectangle())
            }
            .accessibilityIdentifier("connection-add-server")
        }
        .padding(.top, 22)
        if addingServer {
            VStack(alignment: .leading, spacing: 0) { signInForm }.transition(motion.rise)
        }
        }
    }

    // MARK: Data left on this iPhone

    @ViewBuilder private var savedData: some View {
        if !accounts.signedOut.isEmpty {
            SettingsSection("Signed out, with data on this iPhone") {
                ForEach(accounts.signedOut) { item in
                    VStack(alignment: .leading, spacing: 4) {
                        // What the file in the directory says. The address is shown, never trusted.
                        Text(item.host ?? "Unknown server").uiFont(SettingsMetrics.label).foregroundStyle(Theme.textPrimary)
                        if let email = item.account?.email, !email.isEmpty { Text(email).uiFont(14).foregroundStyle(Theme.textSecondary) }
                        Text(AccountRemovalText.saved(item)).uiFont(13).foregroundStyle(Theme.textSecondary)
                        HStack(spacing: 24) {
                            Button("Sign in") {
                                // The address goes into the field, in full, for the person to see before anything is sent.
                                instance = item.account?.url ?? ""
                                if let known = item.account?.email, !known.isEmpty { email = known }
                                if showsAccount { addingServer = true }
                                discovery.check(instance, typed: false)
                            }
                            .foregroundStyle(Theme.accentText)
                            .frame(minWidth: 44, minHeight: 44).contentShape(Rectangle())
                            Button("Erase…") { erasingSaved = item }.foregroundStyle(Theme.danger)
                                .frame(minWidth: 44, minHeight: 44).contentShape(Rectangle())
                        }
                        .uiFont(15, weight: .medium).buttonStyle(.squash)
                    }
                    .padding(.horizontal, 18).padding(.top, 8)
                }
            }
            .accessibilityIdentifier("connection-signed-out")
        }
        if !accounts.olderData.isEmpty {
            SettingsSection("Older saved data") {
                ForEach(accounts.olderData) { item in
                    VStack(alignment: .leading, spacing: 0) {
                        Text(AccountRemovalText.saved(item)).uiFont(15).foregroundStyle(Theme.textPrimary)
                        Button("Erase…") { erasingSaved = item }
                            .uiFont(15, weight: .medium).foregroundStyle(Theme.danger)
                            .frame(minWidth: 44, minHeight: 44).contentShape(Rectangle()).buttonStyle(.squash)
                    }
                    .padding(.horizontal, 18).padding(.top, 8)
                }
            }
            CardNote("Recordings saved by an earlier version, for accounts that are not signed in here. They are kept until you erase them.")
        }
    }
}
