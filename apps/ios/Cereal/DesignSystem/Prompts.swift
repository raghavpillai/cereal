import SwiftUI

/// **Every empty state and every failure that fills a region**: a large light
/// glyph, then one or two lines in the display face, then its actions in the
/// accent colour. Home, Search, Shared with me and a shared note all use it.
/// The sizes are those of Search's empty states.
///
/// When the region is short (the keyboard is up on a landscape iPhone) the
/// glyph and then the detail line drop out instead of being cut.
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
    /// False where the prompt sits in a column of other content and takes only its own height.
    var fills = true
    @Environment(\.motion) private var motion

    var body: some View {
        Group {
            if fills {
                ViewThatFits(in: .vertical) {
                    column(symbol: true, detail: true)
                    column(symbol: false, detail: true)
                    column(symbol: false, detail: false)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                column(symbol: true, detail: true).frame(maxWidth: .infinity)
            }
        }
        .transition(motion.replace())
        .dynamicTypeSize(...DynamicTypeSize.accessibility1)
    }

    private func column(symbol showsSymbol: Bool, detail showsDetail: Bool) -> some View {
        VStack(spacing: 10) {
            if showsSymbol {
                Image(systemName: symbol)
                    .font(.system(size: 60, weight: .light))
                    .foregroundStyle(Theme.textPrimary.opacity(0.7))
                    .padding(.bottom, 12)
                    .accessibilityHidden(true)
            }
            Text(title).displayFont(25).fontWeight(.medium).foregroundStyle(Theme.textPrimary.opacity(0.7))
                .accessibilityAddTraits(.isHeader)
            if showsDetail, let detail {
                Text(detail).displayFont(25).foregroundStyle(Theme.textTertiary)
            }
            if let caption {
                Text(caption).uiFont(14).foregroundStyle(Theme.textSecondary).padding(.top, 4)
            }
            ForEach(actions) { action in
                Button(action.title, action: action.run)
                    .uiFont(16, weight: .medium).foregroundStyle(Theme.accentText)
                    .frame(minWidth: 44, minHeight: 44).contentShape(Rectangle())
                    .buttonStyle(.squash)
            }
        }
        .multilineTextAlignment(.center)
        .padding(.horizontal, Theme.gutter)
        .padding(.bottom, showsSymbol ? bottomRoom : max(0, bottomRoom - 14))
    }
}

/// **The one gold action of a form** (Sign in, Create, Invite): a full-width
/// capsule with its label centred. While it works it stays gold, with the
/// Cereal mark where the label was: a button that was just pressed must not
/// look switched off. Disable it (`.disabled`) only when it cannot be pressed.
struct PrimaryActionButton: View {
    let title: String
    var busy = false
    let action: () -> Void
    @Environment(\.isEnabled) private var isEnabled
    @Environment(\.motion) private var motion

    var body: some View {
        let live = isEnabled || busy
        Button { if !busy { action() } } label: {
            ZStack {
                Text(title).uiFont(17, weight: .medium).opacity(busy ? 0 : 1)
                if busy { CerealLoader(size: 22, color: Theme.onAccent).transition(motion.pop) }
            }
            .foregroundStyle(Theme.onAccent.opacity(live ? 1 : 0.5))
            .frame(maxWidth: .infinity).frame(minHeight: 52)
            .background(Theme.accent.opacity(live ? 1 : 0.38), in: Capsule())
            .contentShape(Capsule())
        }
        .buttonStyle(.squash)
        .animates(busy)
        .animates(isEnabled)
        .accessibilityLabel(title)
        .accessibilityValue(busy ? "Working" : "")
    }
}

/// **A menu of choices on a row** opens from the row's trailing value, and
/// only that value is the menu's label. The system turns a menu's label into
/// the menu: when the whole row was the label, the row vanished for as long
/// as the menu was open, and a glass surface used as one is taken over the
/// same way. The label reaches back across the row's empty middle, so most of
/// the row still opens it.
struct ValueMenu<Content: View>: View {
    let value: String
    var symbol: String? = nil
    var size: CGFloat = 17
    @ViewBuilder var content: Content

    var body: some View {
        Menu { content } label: {
            HStack(spacing: 6) {
                Spacer(minLength: 0)
                if let symbol { Image(systemName: symbol).font(.system(size: size - 2)) }
                Text(value).uiFont(size).lineLimit(1)
                Image(systemName: "chevron.up.chevron.down").font(.system(size: size - 5, weight: .medium))
            }
            .foregroundStyle(Theme.textPrimary)
            .frame(maxWidth: .infinity, minHeight: 44, alignment: .trailing)
            .contentShape(Rectangle())
        }
        .menuOrder(.fixed)
    }
}

/// **A floating text field above the keyboard** (Search, Paste link): one
/// glass capsule with a glyph, the field and a clear control, and a keyboard
/// button beside it that lowers the keyboard while it is up and raises it when
/// it is down. The two share a glass group, so the button buds off the field.
///
/// `submit` returns whether the keyboard should stay up: a field whose text
/// was refused keeps it, so the person can correct what they typed.
struct FieldBar: View {
    let symbol: String
    let prompt: String
    @Binding var text: String
    var focused: FocusState<Bool>.Binding
    var submitLabel: SubmitLabel = .search
    var keyboard: UIKeyboardType = .default
    var contentType: UITextContentType? = nil
    var clearLabel = "Clear"
    var fieldLabel: String? = nil
    var identifier = "field"
    var submit: () -> Bool = { false }
    @Environment(\.motion) private var motion
    @Namespace private var glass

    var body: some View {
        GlassChromeGroup(spacing: 10) {
            HStack(spacing: 10) {
                HStack(spacing: 10) {
                    Image(systemName: symbol).font(.system(size: 17))
                        .foregroundStyle(Theme.textSecondary)
                    TextField("", text: $text, prompt: Text(prompt).foregroundStyle(Theme.textTertiary))
                        .uiFont(15).foregroundStyle(Theme.textPrimary)
                        .focused(focused)
                        .keyboardType(keyboard).textContentType(contentType)
                        .submitLabel(submitLabel)
                        .autocorrectionDisabled()
                        .textInputAutocapitalization(.never)
                        .onSubmit { focused.wrappedValue = submit() }
                        .accessibilityLabel(fieldLabel ?? prompt)
                        .accessibilityIdentifier(identifier)
                    if !text.isEmpty {
                        Button { text = "" } label: {
                            Image(systemName: "xmark.circle.fill").font(.system(size: 17))
                                .foregroundStyle(Theme.textSecondary)
                                .frame(width: 30, height: 30).contentShape(Rectangle().inset(by: -7))
                        }
                        .buttonStyle(SquashButtonStyle(scale: 0.86))
                        .accessibilityLabel(clearLabel)
                        .transition(motion.pop)
                    }
                }
                .padding(.leading, 16).padding(.trailing, 10)
                .frame(minHeight: 50)
                .glassSurface(Capsule(), tint: Theme.fieldTint)
                .overlay(Capsule().strokeBorder(Theme.floatingEdge, lineWidth: 1).allowsHitTesting(false))
                .glassMorph("field", in: glass)
                // As in the reference, a keyboard button stays beside the field.
                Button { focused.wrappedValue.toggle() } label: {
                    Image(systemName: "keyboard").font(.system(size: 17))
                        .foregroundStyle(Theme.textPrimary.opacity(0.85)).frame(width: 50, height: 50)
                        .contentShape(Circle())
                }
                .buttonStyle(.squash)
                .glassSurface(Circle(), interactive: true, tint: Theme.fieldTint)
                .overlay(Circle().strokeBorder(Theme.floatingEdge, lineWidth: 1).allowsHitTesting(false))
                .glassMorph("keyboard", in: glass)
                .accessibilityLabel(focused.wrappedValue ? "Hide keyboard" : "Show keyboard")
            }
        }
        .animation(motion.animation(.gooey, entering: focused.wrappedValue), value: focused.wrappedValue)
        .animates(text.isEmpty)
        .dynamicTypeSize(...DynamicTypeSize.accessibility1)
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
            .uiFont(17, weight: .medium)
            .foregroundStyle(Theme.onAccent)
            .padding(.horizontal, 20)
            .frame(height: 64)
            .glassSurface(Capsule(), interactive: true, tint: Theme.accent)
        }
        .buttonStyle(.squash)
    }
}
