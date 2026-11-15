import SwiftUI

/// A screen with nothing to list or show yet says so in the middle of the page:
/// a large light glyph, then one or two lines in the display face. The sizes
/// are those of Search's empty states.
struct ScreenPrompt: View {
    struct Action: Identifiable {
        let title: String
        let run: () -> Void
        var id: String { title }
    }

    let symbol: String
    let title: String
    var detail: String? = nil
    /// A small line under the text, for a fact such as a server's name.
    var caption: String? = nil
    var actions: [Action] = []
    /// Room kept below the text for the screen's floating bar.
    var bottomRoom: CGFloat = 80
    @Environment(\.motion) private var motion

    var body: some View {
        VStack(spacing: 10) {
            Image(systemName: symbol)
                .font(.system(size: 60, weight: .light))
                .foregroundStyle(Theme.textPrimary.opacity(0.7))
                .padding(.bottom, 12)
                .accessibilityHidden(true)
            Text(title).font(.display(25)).fontWeight(.medium).foregroundStyle(Theme.textPrimary.opacity(0.7))
                .accessibilityAddTraits(.isHeader)
            if let detail {
                Text(detail).font(.display(25)).foregroundStyle(Theme.textTertiary)
            }
            if let caption {
                Text(caption).font(.system(size: 14)).foregroundStyle(Theme.textSecondary).padding(.top, 4)
            }
            ForEach(actions) { action in
                Button(action.title, action: action.run).foregroundStyle(Theme.textSecondary)
                    .frame(minWidth: 44, minHeight: 44).contentShape(Rectangle())
            }
        }
        .multilineTextAlignment(.center)
        .padding(.horizontal, Theme.gutter)
        .padding(.bottom, bottomRoom)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .transition(motion.fade)
    }
}

/// The one gold action of a screen, in its floating bottom bar. The same
/// capsule as "Generate notes": 64 pt tall, 17 pt medium text.
struct PrimaryBarButton: View {
    let title: String
    let systemImage: String
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: 8) {
                Image(systemName: systemImage)
                Text(title).fixedSize()
            }
            .font(.system(size: 17, weight: .medium))
            .foregroundStyle(Theme.onAccent)
            .padding(.horizontal, 20)
            .frame(height: 64)
            .glassSurface(Capsule(), interactive: true, tint: Theme.accent)
        }
        .buttonStyle(.squash)
    }
}
