import SwiftUI
import APIClient
import Contracts

/// What happened to a note's shared links when a request came back, as rules
/// that can be tested.
enum ShareLinkRule {
    /// The link the request just made: the one that was not in the list before.
    static func made(before: [String], after: [String]) -> String? {
        let known = Set(before)
        return after.first { !known.contains($0) }
    }
}

/// "Manage links…": what a new link includes and when it ends, the link just
/// made, and the links that exist, each with its own Revoke. Drawn with the
/// Settings cards on the sheet's colour, as every other form in a sheet.
struct ServerShareView: View {
    let noteID: String
    @Environment(ServerNotesModel.self) private var server
    @Environment(\.dismiss) private var dismiss
    @State private var includeBody = false
    @State private var includeEnhanced = true
    @State private var expire = true
    @State private var shares: [ShareDTO] = []
    /// The list has not been answered for yet.
    @State private var firstLoad = true
    @State private var url: URL?
    /// The link made here, so its row in the list can say so.
    @State private var madeID: String?
    /// What is being done, so the control that asked shows it: nothing else on the sheet changes.
    @State private var working: Work?
    /// What failed, with what it was: "Try again" does that again.
    @State private var failure: Failure?
    @State private var confirmingRevoke: ShareDTO?
    @State private var copied = Copied()
    @Environment(\.motion) private var motion
    /// What the person asked for, as felt: a link made, a link revoked, an action that failed.
    @State private var revocations = 0
    @State private var failures = 0

    enum Work: Equatable { case loading, creating, revoking(String) }
    struct Failure: Equatable { let work: Work; let message: String }

    private var busy: Bool { working != nil }
    private var nothingChosen: Bool { !includeBody && !includeEnhanced }

    var body: some View {
        NavigationStack {
            SheetColumn {
                SettingsSection("A read-only copy") {
                    SettingsToggleRow("My notes", systemImage: "text.viewfinder", isOn: $includeBody)
                    SettingsToggleRow("Enhanced notes", systemImage: "sparkles", isOn: $includeEnhanced)
                    SettingsToggleRow("Expires in 24 hours", systemImage: "clock", isOn: $expire)
                }
                CardNote("Anyone with the link can read what you include. Transcripts are never shared. Later edits stay private: make a new link to share a newer version.")
                if let failure, failure.work == .creating { failureNote(failure) }
                PrimaryActionButton(title: "Create shared link", busy: working == .creating, action: create)
                    .disabled(nothingChosen || (busy && working != .creating))
                    .padding(.top, 18)
                    .accessibilityIdentifier("share-create")
                if let url {
                    Group {
                        SettingsSection("New link") {
                            ShareLink(item: url) { SettingsRowLabel("Share link", systemImage: "square.and.arrow.up", trailing: .none) }
                                .buttonStyle(.row)
                            // The row itself says the link is on the clipboard (`Copied`).
                            RowButton { UIPasteboard.general.string = url.absoluteString; copied.mark() } label: {
                                SettingsRowLabel(copied.title("Copy link"), systemImage: copied.symbol("doc.on.doc"), trailing: .none)
                                    .contentTransition(motion.symbolReplace)
                            }
                            .confirmsCopy($copied)
                            .accessibilityIdentifier("share-copy-link")
                        }
                        CardNote("The link is shown here once. It is not kept on this iPhone.")
                    }
                    .transition(motion.rise)
                }
                SettingsSection("Existing links") {
                    Loading(while: firstLoad && failure == nil) {
                        VStack(alignment: .leading, spacing: 8) {
                            RoundedRectangle(cornerRadius: 5, style: .continuous).fill(Theme.quietFill).frame(width: 190, height: 14)
                            RoundedRectangle(cornerRadius: 5, style: .continuous).fill(Theme.quietFill).frame(width: 120, height: 11)
                        }
                        .padding(.horizontal, 18).padding(.vertical, 12)
                    } then: {
                        VStack(alignment: .leading, spacing: 0) {
                            if shares.isEmpty {
                                Text("No shared links yet.").uiFont(SettingsMetrics.label).foregroundStyle(Theme.textTertiary)
                                    .padding(.horizontal, 18).frame(minHeight: SettingsMetrics.rowHeight)
                            }
                            ForEach(Array(shares.enumerated()), id: \.element.id) { index, share in
                                linkRow(share, divider: index < shares.count - 1).transition(motion.rise)
                            }
                        }
                    }
                }
                if let failure, failure.work != .creating { failureNote(failure) }
            }
            // The new link and the rows of the list arrive and leave in place.
            .animates(url)
            .animates(shares.map { "\($0.id)\($0.revokedAt ?? "")" })
            .animates(failure)
            .animates(nothingChosen)
            // A new link made, revoked; a request that failed. Loading the list is not felt.
            .haptic(.done, trigger: url) { _, new in new != nil }
            .haptic(.destructive, trigger: revocations)
            .haptic(.error, trigger: failures)
            .haptic(.selection, trigger: [includeBody, includeEnhanced, expire])
            .navigationTitle("Share note").navigationBarTitleDisplayMode(.inline)
            .sheetBar()
            .toolbar { ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() }.disabled(working == .creating) } }
            .interactiveDismissDisabled(working == .creating)
            .task { await reload() }
        }.glassSheet(background: Theme.sheet)
    }

    /// What failed, and the way to do that again (not something else).
    private func failureNote(_ failure: Failure) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            CardNote(failure.message, color: Theme.danger)
            Button(Wording.retry) { retry(failure.work) }
                .uiFont(14, weight: .medium).foregroundStyle(Theme.accentText)
                .padding(.horizontal, 14).frame(minHeight: 44).contentShape(Rectangle())
                .buttonStyle(.squash).disabled(busy)
                .accessibilityIdentifier("share-retry")
        }
        .transition(motion.rise)
    }

    private func linkRow(_ share: ShareDTO, divider: Bool) -> some View {
        HStack(alignment: .center, spacing: 12) {
            VStack(alignment: .leading, spacing: 3) {
                HStack(spacing: 8) {
                    // To the second: two links made in the same minute are told apart.
                    Text("Created \(Self.moment(share.createdAt))").uiFont(15).foregroundStyle(share.revokedAt == nil ? Theme.textPrimary : Theme.textTertiary)
                    if share.id == madeID, share.revokedAt == nil {
                        Text("New").uiFont(12, weight: .semibold).foregroundStyle(Theme.accentText)
                            .padding(.horizontal, 7).frame(minHeight: 20)
                            .background(Theme.accentTint, in: Capsule())
                    }
                }
                Text(share.revokedAt != nil ? "Revoked" : share.expiresAt.map { "Expires \(Self.day($0))" } ?? "Does not expire")
                    .uiFont(13).foregroundStyle(Theme.textTertiary)
            }
            Spacer(minLength: 8)
            if share.revokedAt == nil {
                // Its own 44 pt target, and only it revokes: not a tap anywhere on the row.
                Button { confirmingRevoke = share } label: {
                    ZStack {
                        Text("Revoke").uiFont(15, weight: .medium).foregroundStyle(Theme.danger).opacity(working == .revoking(share.id) ? 0 : 1)
                        if working == .revoking(share.id) { CerealLoader(size: 18, color: Theme.danger).transition(motion.pop) }
                    }
                    .frame(minWidth: 44, minHeight: 44).contentShape(Rectangle())
                }
                .buttonStyle(.squash).disabled(busy)
                .animates(working)
                .accessibilityLabel("Revoke link").accessibilityIdentifier("share-revoke")
                // Revoking cannot be undone: it is asked about, from the button itself.
                .confirmationDialog("Revoke this link?", isPresented: Binding(get: { confirmingRevoke?.id == share.id }, set: { if !$0 { confirmingRevoke = nil } }), titleVisibility: .visible) {
                    Button("Revoke link", role: .destructive) { revoke(share) }
                    Button("Cancel", role: .cancel) {}
                } message: { Text("Anyone who has it will no longer be able to open the note.") }
            }
        }
        .padding(.horizontal, 18).padding(.vertical, 6)
        .frame(minHeight: SettingsMetrics.rowHeight)
        .overlay(alignment: .bottom) {
            if divider { Rectangle().fill(Theme.divider.opacity(0.7)).frame(height: 0.5).padding(.leading, 18) }
        }
    }

    /// A server timestamp as a day and time in the reader's own format, not the raw ISO text.
    static func day(_ timestamp: String) -> String {
        guard let date = Date.serverTimestamp(timestamp) else { return String(timestamp.prefix(10)) }
        return date.formatted(.dateTime.day().month(.abbreviated).year().hour().minute())
    }

    /// The same with seconds, for when a link was made.
    static func moment(_ timestamp: String) -> String {
        guard let date = Date.serverTimestamp(timestamp) else { return String(timestamp.prefix(19)) }
        return date.formatted(.dateTime.day().month(.abbreviated).hour().minute().second())
    }

    private func retry(_ work: Work) {
        switch work {
        case .loading: Task { await reload() }
        case .creating: create()
        case .revoking(let id): if let share = shares.first(where: { $0.id == id }) { revoke(share) }
        }
    }

    private func create() {
        guard !busy, !nothingChosen else { return }
        working = .creating; failure = nil
        let before = shares.map(\.id)
        Task {
            defer { working = nil }
            do {
                let expiry = expire ? Date.now.addingTimeInterval(86_400).ISO8601Format() : nil
                let made = try await server.createShare(noteID: noteID, includeBody: includeBody, includeEnhanced: includeEnhanced, expiresAt: expiry)
                // The link exists from here: a list that cannot be read afterwards does not take it away.
                url = made; copied.clear()
                if let listed = try? await server.shares(noteID: noteID) {
                    madeID = ShareLinkRule.made(before: before, after: listed.map(\.id))
                    shares = listed; firstLoad = false
                }
            } catch is CancellationError { }
            catch { failure = Failure(work: .creating, message: error.localizedDescription); failures += 1 }
        }
    }

    private func revoke(_ share: ShareDTO) {
        guard !busy else { return }
        working = .revoking(share.id); failure = nil; revocations += 1
        Task {
            defer { working = nil }
            do {
                try await server.revokeShare(id: share.id)
                // The link shown above may be the one revoked: it is taken away rather than offered dead.
                if share.id == madeID || madeID == nil { url = nil }
                shares = try await server.shares(noteID: noteID)
            } catch is CancellationError { }
            catch { failure = Failure(work: .revoking(share.id), message: error.localizedDescription); failures += 1 }
        }
    }

    private func reload() async {
        guard !busy else { return }
        working = .loading
        defer { working = nil }
        do { shares = try await server.shares(noteID: noteID); firstLoad = false; if failure?.work == .loading { failure = nil } }
        catch is CancellationError { }
        catch { failure = Failure(work: .loading, message: error.localizedDescription) }
    }
}
