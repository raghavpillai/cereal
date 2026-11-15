import SwiftUI
import APIClient
import Contracts

struct ServerShareView: View {
    let noteID: String
    @Environment(ServerNotesModel.self) private var server
    @Environment(\.dismiss) private var dismiss
    @State private var includeBody = false
    @State private var includeEnhanced = true
    @State private var expire = true
    @State private var shares: [ShareDTO] = []
    @State private var url: URL?
    @State private var busy = false
    @State private var error: String?
    @Environment(\.motion) private var motion

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    Toggle("Include my notes", isOn: $includeBody)
                    Toggle("Include enhanced notes", isOn: $includeEnhanced)
                    Toggle("Expire after 24 hours", isOn: $expire)
                    Button("Create shared link") {
                        busy = true; error = nil
                        Task {
                            do {
                                let expiry = expire ? Date.now.addingTimeInterval(86_400).ISO8601Format() : nil
                                url = try await server.createShare(noteID: noteID, includeBody: includeBody, includeEnhanced: includeEnhanced, expiresAt: expiry)
                                shares = try await server.shares(noteID: noteID)
                            } catch is CancellationError { }
                            catch { self.error = error.localizedDescription }
                            busy = false
                        }
                    }.buttonStyle(.glassProminent).tint(Theme.accent)
                        .disabled(busy || (!includeBody && !includeEnhanced))
                } header: { Text("Read-only snapshot") } footer: {
                    Text("Anyone with the link can read the selected content. Transcripts are never shared. Later edits stay private; create a new link to share a newer version.")
                }
                if let url {
                    Section {
                        ShareLink(item: url) { Label("Share link", systemImage: "square.and.arrow.up") }
                        Button("Copy link", systemImage: "doc.on.doc") { UIPasteboard.general.string = url.absoluteString }
                    } footer: { Text("The link is only available here after creation. It is not saved to app preferences.") }
                }
                if busy { Section { ProgressView("Updating shared links…") } }
                if let error { Section { Text(error).foregroundStyle(Theme.danger); Button("Retry") { Task { await reload() } }.disabled(busy) } }
                Section("Existing links") {
                    if shares.isEmpty { Text("No shared links.").foregroundStyle(Theme.textTertiary) }
                    ForEach(shares, id: \.id) { share in
                        VStack(alignment: .leading, spacing: 8) {
                            Text("Created \(share.createdAt.prefix(10))").font(.subheadline)
                            if share.revokedAt != nil {
                                Text("Revoked").font(.caption).foregroundStyle(Theme.textTertiary)
                            } else {
                                Text(share.expiresAt.map { "Expires \($0.prefix(10))" } ?? "No expiration")
                                    .font(.caption).foregroundStyle(Theme.textTertiary)
                                Button("Revoke link", role: .destructive) {
                                    busy = true; error = nil
                                    Task {
                                        do {
                                            try await server.revokeShare(id: share.id)
                                            // A newly created URL may have been revoked. Remove
                                            // it rather than offering an unusable capability.
                                            url = nil
                                            shares = try await server.shares(noteID: noteID)
                                        } catch is CancellationError { }
                                        catch { self.error = error.localizedDescription }
                                        busy = false
                                    }
                                }.disabled(busy)
                            }
                        }.padding(.vertical, 4)
                    }
                }
            }
            // The new link, progress and existing-links rows slide into the form.
            .animation(motion.snappy, value: busy)
            .animation(motion.settle, value: url)
            .animation(motion.settle, value: shares.map(\.id))
            .animation(motion.snappy, value: error)
            .navigationTitle("Share note").navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() }.disabled(busy) } }
            .interactiveDismissDisabled(busy)
            .task { await reload() }
        }.glassSheet()
    }

    private func reload() async {
        guard !busy else { return }
        busy = true
        do { shares = try await server.shares(noteID: noteID); error = nil }
        catch is CancellationError { }
        catch { self.error = error.localizedDescription }
        busy = false
    }
}
