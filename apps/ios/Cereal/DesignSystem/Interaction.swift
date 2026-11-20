import SwiftUI
import UIKit

// MARK: - Keyboard

/// **The keyboard, where a `FocusState` alone is not enough.**
///
/// Setting a focus binding to false asks SwiftUI to end editing on its next
/// update. That is too late for code that is about to clear or read the field
/// in the same turn: the keyboard may still hold text it has not handed over
/// (an autocorrection, a suggestion being applied), and gives it to the field
/// after the field was emptied. So anything that takes the text of a field
/// and empties it (sending a chat question) ends editing through here first,
/// then reads, then clears, and sets its focus binding as well so the two agree.
@MainActor enum Keyboard {
    /// Ends editing now, whoever has the keyboard. Text the keyboard was still
    /// holding is in the field (and its binding) when this returns.
    static func dismiss() {
        UIApplication.shared.sendAction(#selector(UIResponder.resignFirstResponder), to: nil, from: nil, for: nil)
    }

    /// Makes the keyboard let go of what it was about to change, without
    /// putting it away: for a field that is emptied while it keeps the keyboard.
    static func settle() {
        guard let input = firstResponder as? UITextInput else { return }
        input.inputDelegate?.selectionWillChange(input)
        input.inputDelegate?.selectionDidChange(input)
    }

    /// Text left in the field that has the keyboard, or nil when it is empty
    /// (or nothing has the keyboard).
    static var textInFocus: String? {
        switch firstResponder {
        case let view as UITextView: view.text.isEmpty ? nil : view.text
        case let field as UITextField: (field.text ?? "").isEmpty ? nil : field.text
        default: nil
        }
    }

    /// Empties the field that has the keyboard. Only for a field whose binding
    /// is already empty: see `ChatComposer`.
    static func emptyFocusedField() {
        switch firstResponder {
        case let view as UITextView: view.text = ""
        case let field as UITextField: field.text = ""
        default: break
        }
    }

    fileprivate static weak var found: UIResponder?
    private static var firstResponder: UIResponder? {
        found = nil
        UIApplication.shared.sendAction(#selector(UIResponder.cerealReportFirstResponder), to: nil, from: nil, for: nil)
        return found
    }
}

private extension UIResponder {
    @objc func cerealReportFirstResponder() { Keyboard.found = self }
}

/// See `View.takesKeyboardOnArrival`.
private struct ArrivalFocus: UIViewRepresentable {
    let active: Bool

    func makeUIView(context: Context) -> Probe {
        let probe = Probe()
        probe.active = active
        probe.isUserInteractionEnabled = false
        return probe
    }

    func updateUIView(_ probe: Probe, context: Context) {}

    final class Probe: UIView {
        var active = false
        private var asked = false

        override func didMoveToWindow() {
            super.didMoveToWindow()
            guard active, !asked, window != nil else { return }
            asked = true
            take(tries: 12)
        }

        /// The field may be built a turn or two after this view is: it is looked for until it is there.
        private func take(tries: Int) {
            guard window != nil else { return }
            if let field = nearestField() {
                if !field.isFirstResponder { field.becomeFirstResponder() }
            } else if tries > 0 {
                DispatchQueue.main.async { [weak self] in self?.take(tries: tries - 1) }
            }
        }

        /// The text field this view was put behind: the first one under the nearest ancestor that has any.
        private func nearestField() -> UIView? {
            var ancestor = superview
            for _ in 0..<6 {
                guard let view = ancestor else { return nil }
                if let field = Self.field(in: view, depth: 6) { return field }
                ancestor = view.superview
            }
            return nil
        }

        private static func field(in view: UIView, depth: Int) -> UIView? {
            if view is UITextView || view is UITextField { return view }
            guard depth > 0 else { return nil }
            for child in view.subviews { if let found = field(in: child, depth: depth - 1) { return found } }
            return nil
        }
    }
}

extension View {
    /// **A field that has the keyboard as its screen arrives** (the composer of a note's empty chat).
    /// A `FocusState` set while a screen is being pushed is refused until the push has finished, so
    /// the keyboard came up as a second movement after the page had landed. This asks the field
    /// itself the moment it is in the window, which is before the push moves: the keyboard and the
    /// page arrive together. Goes on the `TextField`; the `FocusState` follows by itself.
    func takesKeyboardOnArrival(_ active: Bool) -> some View {
        background { if active { ArrivalFocus(active: true).frame(width: 0, height: 0).accessibilityHidden(true) } }
    }
}

// MARK: - Copying

/// **Every copy confirms the same way**: the control that copied shows a check
/// (and says "Copied", if it has words) for `Copied.shown`, with the `done`
/// haptic, then gives itself back. A menu item cannot show anything once its
/// menu has closed, so there the menu's own button shows the check.
///
///     @State private var copied = Copied()
///     Button { UIPasteboard.general.string = text; copied.mark() } label: {
///         Label(copied.title("Copy link"), systemImage: copied.symbol("link"))
///     }
///     .confirmsCopy($copied)
///
/// Give the glyph `.contentTransition(motion.symbolReplace)`.
struct Copied: Equatable {
    /// How long the check stays.
    static let shown: Duration = .milliseconds(1500)
    private(set) var count = 0
    private(set) var isOn = false

    mutating func mark() { count &+= 1; isOn = true }
    mutating func clear() { isOn = false }

    /// The control's own glyph, or the check while the copy is being confirmed.
    func symbol(_ usual: String) -> String { isOn ? "checkmark" : usual }
    /// The control's own words, or "Copied".
    func title(_ usual: String) -> String { isOn ? "Copied" : usual }
}

private struct CopyConfirmation: ViewModifier {
    @Binding var copied: Copied
    @Environment(\.motion) private var motion

    func body(content: Content) -> some View {
        content
            .animation(motion.snappy, value: copied.isOn)
            .haptic(.done, trigger: copied.count)
            .task(id: copied.count) {
                guard copied.isOn else { return }
                try? await Task.sleep(for: Copied.shown)
                if !Task.isCancelled { copied.clear() }
            }
    }
}

extension View {
    /// Plays the haptic of a copy and takes the check away again. See `Copied`.
    func confirmsCopy(_ copied: Binding<Copied>) -> some View { modifier(CopyConfirmation(copied: copied)) }
}

// MARK: - Sheets

/// **The page of a sheet that holds a form**: the Settings cards
/// (`SettingsSection`, `SettingsCard`, `CardField`, `CardNote`), 16 pt from the
/// sheet's edges, on the sheet's own colour. Present the sheet with
/// `.glassSheet(background: Theme.sheet)` and give its stack `.sheetBar()`.
struct SheetColumn<Content: View>: View {
    @ViewBuilder var content: Content
    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 0) { content }
                .padding(.horizontal, 16).padding(.top, 4).padding(.bottom, 32)
        }
        .scrollIndicators(.hidden)
        // A navigation stack paints the system's white behind its pages: the warm page goes over it.
        .background(Theme.sheet.ignoresSafeArea())
        .dynamicTypeSize(...DynamicTypeSize.accessibility1)
    }
}

extension View {
    /// **The navigation bar of a sheet whose page scrolls**: the bar is the
    /// sheet's own colour, so lines that scroll up are gone before they reach
    /// its title. Without it the title is drawn straight over the text.
    /// Goes on the page inside the sheet's `NavigationStack`.
    func sheetBar() -> some View {
        toolbarBackground(Theme.sheet, for: .navigationBar)
            .toolbarBackgroundVisibility(.visible, for: .navigationBar)
    }

    /// **A bar a sheet draws for itself over its scrolling page** (a header in
    /// a top inset, a search field in a bottom one): the sheet's colour behind
    /// the bar out to the sheet's edge, thinning over `fade` points towards the
    /// page, so no row is read through the bar or seen beyond it. The sheet
    /// must be the same colour: `.glassSheet(background: Theme.sheet)`.
    func sheetBarBackdrop(_ edge: VerticalEdge, fade: CGFloat = 16) -> some View {
        background {
            VStack(spacing: 0) {
                if edge == .bottom {
                    LinearGradient(colors: [Theme.sheet.opacity(0), Theme.sheet], startPoint: .top, endPoint: .bottom).frame(height: fade)
                }
                Theme.sheet
                if edge == .top {
                    LinearGradient(colors: [Theme.sheet, Theme.sheet.opacity(0)], startPoint: .top, endPoint: .bottom).frame(height: fade)
                }
            }
            .padding(edge == .top ? .bottom : .top, -fade)
            .ignoresSafeArea(edges: edge == .top ? .top : .bottom)
            .allowsHitTesting(false)
        }
    }
}
