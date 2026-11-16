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

struct ConnectionView: View {
    @Environment(ServerNotesModel.self) private var server
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
    @State private var browser = BrowserSignInSession()
    /// True while the sign-in in progress is the one in the browser, so its own button shows it.
    @State private var inBrowser = false
    @Environment(\.motion) private var motion

    private var address: String { instance.trimmingCharacters(in: .whitespacesAndNewlines) }

    /// The address field. Each change is handed to discovery as the field makes
    /// it, not when the view next compares values: a character typed while the
    /// form was redrawing for an earlier answer was otherwise never asked about.
    private var addressField: Binding<String> {
        Binding(get: { instance }, set: { instance = $0; discovery.check($0) })
    }

    private func completeConnection() {
        if case .loaded = server.state {
            mode.isDemo = false
            // A note that was waiting for this sign-in opens; otherwise Home.
            router.path = LinkNavigation.pathAfterSignIn(router.path, instance: server.instance)
            if !router.path.isEmpty { router.linkOpenedIn = .some(server.accountIdentity) }
            dismiss()
        } else if case .failed(let failure, _) = server.state { error = failure.localizedDescription }
    }

    /// Signs in on the server's own page, in the system browser, by whichever
    /// method the server offers there. The session that comes back is kept
    /// exactly as one from the password form is.
    private func continueInBrowser() {
        busy = true; inBrowser = true; error = nil
        Task {
            defer { busy = false; inBrowser = false }
            do {
                let attempt = try BrowserSignIn(instance: InstanceURL(instance))
                let answer = try await browser.answer(from: attempt.loginURL)
                try await server.signIn(browser: attempt, callback: answer)
                password = ""; token = ""
                completeConnection()
            } catch BrowserSignInError.cancelled {
            } catch is CancellationError {
            } catch { self.error = error.localizedDescription }
        }
    }

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField("https://notes.example.com", text: addressField)
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
                        HStack { Text("Checking how this server signs in…").foregroundStyle(Theme.textSecondary); Spacer(); ProgressView() }
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
                                busy = true; error = nil
                                Task {
                                    await server.signIn(instance: instance, email: email.trimmingCharacters(in: .whitespacesAndNewlines), password: password)
                                    password = ""; token = ""; busy = false
                                    completeConnection()
                                }
                            } label: {
                                HStack { Text("Sign in"); Spacer(); if busy && !inBrowser { ProgressView() } }
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
                                    HStack { Text(label); Spacer(); if inBrowser { ProgressView() } }
                                }.accessibilityIdentifier("connection-browser")
                            } else {
                                Button(action: continueInBrowser) {
                                    HStack { Text(label); Spacer(); if busy { ProgressView() } }
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
                        busy = true; error = nil
                        Task {
                            await server.connect(instance: instance, token: token)
                            token = ""; password = ""; busy = false
                            completeConnection()
                        }
                    }.disabled(token.isEmpty || instance.isEmpty || busy)
                }
                if let error {
                    Section { Text(error).foregroundStyle(Theme.danger).accessibilityAddTraits(.updatesFrequently) }
                }
                if let connected = server.instance {
                    Section("Configured instance") {
                        Text(connected.description).font(.footnote).textSelection(.enabled)
                        if let user = server.session?.user {
                            Text(user.name).font(.headline)
                            Text(user.email).font(.footnote)
                            LabeledContent("Instance role", value: user.role.rawValue.capitalized)
                                .font(.caption).foregroundStyle(Theme.textSecondary)
                        }
                        NavigationLink("Workspaces") { WorkspacesView() }
                            .accessibilityIdentifier("connection-workspaces")
                        Link("Manage calendar connections", destination: connected.url.appendingPathComponent("account"))
                        Button("Use server notes") { mode.isDemo = false; router.path = []; dismiss() }
                        Button("Sign out", role: .destructive) {
                            busy = true
                            Task {
                                do { try await server.signOut() } catch { self.error = error.localizedDescription }
                                mode.isDemo = false; router.path = []; busy = false
                            }
                        }
                    }
                }
                Section {
                    Button("Use local demo") { mode.isDemo = true; router.path = []; dismiss() }
                } footer: {
                    Text("Demo notes and recordings stay on this device. They are never automatically uploaded. They remain separate from your signed-in account.")
                }
            }
            .disabled(busy)
            .animation(motion.snappy, value: busy)
            .animation(motion.snappy, value: error)
            .animation(motion.snappy, value: discovery.state)
            .onChange(of: address, initial: true) { _, text in discovery.check(text) }
            .animation(motion.settle, value: server.instance?.description)
            .navigationTitle("Connection")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .cancellationAction) { Button("Done") { dismiss() }.disabled(busy) } }
            .interactiveDismissDisabled(busy)
            .onAppear {
                instance = router.signInAddress ?? server.instance?.description ?? ""
                discovery.check(instance, typed: false)
            }
            .onDisappear { password = ""; token = ""; router.signInAddress = nil }
        }
        .glassSheet()
    }
}
