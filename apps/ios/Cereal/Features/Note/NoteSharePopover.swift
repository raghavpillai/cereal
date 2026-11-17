import SwiftUI
import UIKit
import CoreImage.CIFilterBuiltins
import APIClient

/// The note's Share popover, as in the reference: three round actions (Share via,
/// Copy link, QR code) over the link's access ("Anyone with the link").
///
/// A Cereal share is a read-only snapshot link made on request. The popover
/// makes one, of the generated notes, the first time an action needs it, and
/// keeps it while it is open; "Manage links…" opens the full sheet (what a link
/// includes, its expiry, and revoking).
struct NoteSharePopover: View {
    let noteID: String
    var onManage: () -> Void
    @Environment(ServerNotesModel.self) private var server
    @Environment(\.motion) private var motion
    @State private var url: URL?
    @State private var busy: Action?
    @State private var copied = false
    @State private var showingQR = false
    @State private var sharing: SharedLink?
    @State private var error: String?
    /// Links made here expire, as the full sheet's do by default.
    @State private var expires = true

    enum Action: Hashable { case share, copy, qr }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            if showingQR, let url {
                qr(url).transition(.opacity)
            } else {
                HStack(alignment: .top, spacing: 14) {
                    action(.share, title: "Share via", systemImage: "square.and.arrow.up")
                    action(.copy, title: copied ? "Copied" : "Copy link", systemImage: copied ? "checkmark" : "link")
                    action(.qr, title: "QR code", systemImage: "qrcode")
                }
                .padding(.horizontal, 2)
                if let error {
                    Text(error).font(.system(size: 13)).foregroundStyle(Theme.danger).padding(.top, 10)
                        .fixedSize(horizontal: false, vertical: true)
                }
                access.padding(.top, 18)
            }
        }
        .padding(.horizontal, 22).padding(.top, 22).padding(.bottom, 18)
        .frame(width: 330, alignment: .leading)
        .animation(motion.snappy, value: showingQR)
        .animation(motion.snappy, value: copied)
        .animation(motion.snappy, value: error)
        // The link is on the clipboard; making it failed.
        .haptic(.done, trigger: copied) { _, now in now }
        .haptic(.error, trigger: error) { old, new in new != nil && new != old }
        .presentationCompactAdaptation(.popover)
        .sheet(item: $sharing) { ActivitySheet(items: [$0.url]).ignoresSafeArea() }
    }

    private func action(_ kind: Action, title: String, systemImage: String) -> some View {
        Button { run(kind) } label: {
            VStack(spacing: 9) {
                ZStack {
                    if busy == kind { ProgressView().transition(motion.swap) }
                    else { Image(systemName: systemImage).font(.system(size: 22, weight: .regular)).transition(motion.swap) }
                }
                .foregroundStyle(Theme.textPrimary)
                .frame(width: 66, height: 66)
                .background(Theme.quietFill, in: Circle())
                Text(title).font(.system(size: 13)).foregroundStyle(Theme.textPrimary).lineLimit(1).fixedSize()
            }
            .frame(minWidth: 76)
            .contentShape(Rectangle())
        }
        .buttonStyle(.squash)
        .disabled(busy != nil)
        .accessibilityIdentifier("share-\(kind)")
    }

    private var access: some View {
        HStack(spacing: 14) {
            Image(systemName: "globe").font(.system(size: 22, weight: .light)).foregroundStyle(Theme.textPrimary)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 1) {
                Text("Link access").font(.system(size: 13)).foregroundStyle(Theme.textSecondary)
                Menu {
                    Button("Anyone with the link", systemImage: "checkmark") {}
                    Toggle("Expires after 24 hours", isOn: $expires)
                    Divider()
                    Button("Manage links…", systemImage: "link.badge.plus", action: onManage)
                } label: {
                    HStack(spacing: 6) {
                        Text("Anyone with the link").font(.system(size: 16)).foregroundStyle(Theme.textPrimary)
                        Image(systemName: "chevron.down").font(.system(size: 11, weight: .semibold)).foregroundStyle(Theme.textSecondary)
                    }
                }
                .menuOrder(.fixed)
                .tint(Theme.textPrimary)
                .accessibilityLabel("Link access: anyone with the link")
            }
        }
    }

    private func qr(_ url: URL) -> some View {
        VStack(spacing: 14) {
            if let image = Self.qrImage(url.absoluteString) {
                Image(uiImage: image).interpolation(.none).resizable().scaledToFit()
                    .frame(width: 200, height: 200)
                    .padding(12).background(.white, in: RoundedRectangle(cornerRadius: 14, style: .continuous))
                    .accessibilityLabel("QR code for the shared link")
            }
            Button("Done") { showingQR = false }.font(.system(size: 16, weight: .medium)).tint(Theme.textPrimary)
        }
        .frame(maxWidth: .infinity)
    }

    private func run(_ kind: Action) {
        error = nil
        Task {
            guard let link = await link(for: kind) else { return }
            switch kind {
            case .share: sharing = SharedLink(url: link)
            case .copy:
                UIPasteboard.general.url = link
                copied = true
                try? await Task.sleep(for: .seconds(1.6))
                copied = false
            case .qr: showingQR = true
            }
        }
    }

    private func link(for kind: Action) async -> URL? {
        if let url { return url }
        busy = kind
        defer { busy = nil }
        do {
            let expiry = expires ? Date.now.addingTimeInterval(86_400).ISO8601Format() : nil
            let made = try await server.createShare(noteID: noteID, includeBody: false, includeEnhanced: true, expiresAt: expiry)
            url = made
            return made
        } catch is CancellationError { return nil }
        catch { self.error = error.localizedDescription; return nil }
    }

    static func qrImage(_ text: String) -> UIImage? {
        let filter = CIFilter.qrCodeGenerator()
        filter.message = Data(text.utf8)
        filter.correctionLevel = "M"
        guard let output = filter.outputImage?.transformed(by: CGAffineTransform(scaleX: 8, y: 8)),
              let cg = CIContext().createCGImage(output, from: output.extent) else { return nil }
        return UIImage(cgImage: cg)
    }
}

private struct SharedLink: Identifiable { let url: URL; var id: String { url.absoluteString } }

/// The system share sheet for "Share via".
private struct ActivitySheet: UIViewControllerRepresentable {
    let items: [Any]
    func makeUIViewController(context: Context) -> UIActivityViewController {
        UIActivityViewController(activityItems: items, applicationActivities: nil)
    }
    func updateUIViewController(_ controller: UIActivityViewController, context: Context) {}
}
