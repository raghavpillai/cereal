import SwiftUI
import APIClient

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
    @Environment(\.motion) private var motion

    private func completeConnection() {
        if case .loaded = server.state {
            mode.isDemo = false; router.path = []; dismiss()
        } else if case .failed(let failure, _) = server.state { error = failure.localizedDescription }
    }

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField("https://notes.example.com", text: $instance)
                        .textContentType(.URL).keyboardType(.URL)
                        .textInputAutocapitalization(.never).autocorrectionDisabled()
                        .accessibilityLabel("Instance URL")
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
                        HStack { Text("Sign in"); Spacer(); if busy { ProgressView() } }
                    }.buttonStyle(.glassProminent).tint(Theme.accent)
                        .disabled(busy || email.isEmpty || password.isEmpty || instance.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                } header: { Text("Your server") } footer: {
                    Text("Sign in to your account on your self-hosted server. Your session is kept in this device’s Keychain. Passwords are never saved. HTTPS is required except for localhost development.")
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
            .animation(motion.settle, value: server.instance?.description)
            .navigationTitle("Connection")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .cancellationAction) { Button("Done") { dismiss() }.disabled(busy) } }
            .interactiveDismissDisabled(busy)
            .onAppear { instance = server.instance?.description ?? "" }
            .onDisappear { password = ""; token = "" }
        }
        .glassSheet()
    }
}
