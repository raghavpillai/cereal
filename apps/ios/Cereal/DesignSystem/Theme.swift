import SwiftUI
import UIKit

/// Every colour has a light and a dark value, measured from the reference
/// recordings (Granola's iPhone app, light by default with a Theme setting).
/// Screens use these tokens and never a literal white or black, so they follow
/// the Theme setting (`ThemePreference`) and the system appearance under Auto.
enum Theme {
    /// The page. Light: a warm off-white (#F0F0EA); dark: flat #1E1E1E.
    static let bgTop = Color(light: 0xF0F0EA, dark: 0x1E1E1E)
    static let bgBottom = Color(light: 0xECECE6, dark: 0x262626)
    /// Note tiles and other quiet fills on the page (#E7E4DC on light).
    static let surface = Color(light: 0xE7E4DC, dark: 0x292929)
    /// Cards that sit above the page: Settings groups, opaque fallbacks for glass.
    static let surfaceRaised = Color(light: 0xF6F6F2, dark: 0x333333)
    static let stroke = Color(light: .black.withAlphaComponent(0.07), dark: .white.withAlphaComponent(0.08))
    static let glassTint = Color(light: .white.withAlphaComponent(0.35), dark: .white.withAlphaComponent(0.06))
    static let divider = Color(light: 0xD4D4CE, dark: 0x2E2E2E)
    static let textPrimary = Color(light: 0x1C1C17, dark: 0xF2F2F2)
    static let textSecondary = Color(light: UIColor(hex: 0x5D5D57), dark: .white.withAlphaComponent(0.55))
    static let textTertiary = Color(light: UIColor(hex: 0xA0A09A), dark: .white.withAlphaComponent(0.35))
    /// The document and phone glyphs on note tiles (#403D38 on light).
    static let tileGlyph = Color(light: UIColor(hex: 0x403D38), dark: .white.withAlphaComponent(0.55))
    /// The small glass button that hides the keyboard.
    static let keyboardButtonTint = Color(light: .white.withAlphaComponent(0.55), dark: .black.withAlphaComponent(0.25))
    /// The sidebar panel's glass.
    static let sidebarTint = Color(light: .white.withAlphaComponent(0.6), dark: .black.withAlphaComponent(0.3))
    /// The ink of this theme: white on dark, near-black on light. For hairlines
    /// and washes written as `Theme.ink.opacity(…)` where the dark theme had
    /// `Color.white.opacity(…)`.
    static let ink = Color(light: 0x1C1C17, dark: 0xFFFFFF)
    /// Cereal gold. Brand colours come from `scripts/logo/gold_palette.py`, never by eye.
    static let accent = Color(hex: 0xE3B025)
    static let onAccent = Color(hex: 0x1A1A1A)
    /// Gold as text, links and the app's tint. On dark it is the brand gold; on
    /// light the bright gold is unreadable on the page (1.7:1), so it is darkened
    /// to a deep gold (#8A6400, 4.7:1 on #F0F0EA). Fills keep `accent`.
    static let accentText = Color(light: 0x8A6400, dark: 0xE3B025)
    /// Where the reference tints with its lime (search result tiles, matched
    /// words, the desktop-app banner in Settings) Cereal tints with its gold, mixed
    /// into the page in the same proportion.
    static let accentTint = Color(light: 0xEBE0BE, dark: 0x3A3320)
    static let accentTintGlyph = Color(light: 0x6B5000, dark: 0xE0C27A)
    static let accentHighlight = Color(light: 0xEDE6CF, dark: 0x4A3F1E)
    static let accentHighlightStroke = Color(light: 0xC2A860, dark: 0x7A6630)
    static let danger = Color(hex: 0xFF5A4F)
    static let lightButton = Color(hex: 0xECECE6)
    /// What the system gives this app's full-height sheets, measured on screen
    /// (the saved transcript, Workspaces). For a sheet that must not be left to
    /// the system material; see `glassSheet(background:)`.
    static let sheet = Color(light: 0xF0F0EA, dark: 0x1C1C1E)

    // Chat has a brighter composer, darker questions and distinct idle/active
    // action colors in the supplied mobile recordings. These are deliberately
    // separate from meeting cards and the gold recording indicator.
    static let chatComposer = Color(light: 0xFFFFFF, dark: 0x313131)
    /// Keeps the composer's glass close to the reference's bar (white on light).
    static let chatComposerTint = Color(light: .white.withAlphaComponent(0.85), dark: .black.withAlphaComponent(0.2))
    static let chatQuestion = Color(light: 0xE6E6E0, dark: 0x272727)
    /// The send button of every composer, as on the desktop: the brand's bright gold
    /// with a bold near-black arrow. Until there is something to send it is a quiet grey disc.
    static let sendGold = Color(hex: 0xE0AF2F)
    static let sendGlyph = Color(hex: 0x1D1A12)
    /// The desktop's hover fill (white at 6% on dark), the idle send and the Stop button's disc.
    static let quietFill = Color(light: .black.withAlphaComponent(0.05), dark: .white.withAlphaComponent(0.06))
    static let chatText = Color(light: 0x1C1C17, dark: 0xFFFFFF)

    /// The Ask pill: near-opaque white on light (#FEFEF8), a light glass on dark.
    static let askPillTint = Color(light: .white.withAlphaComponent(0.92), dark: .white.withAlphaComponent(0.20))
    /// The faint edge of white floating bars on light. None on dark, where glass draws its own.
    static let floatingEdge = Color(light: .black.withAlphaComponent(0.06), dark: .clear)
    /// Search and other floating text fields.
    static let fieldTint = Color(light: .white.withAlphaComponent(0.55), dark: .white.withAlphaComponent(0.08))
    /// The lighter tone under floating bottom bars.
    static let bottomFade = Color(light: 0xF6F6F2, dark: 0x2A2A2A)
    /// The bar beside an empty day's line and beside events without a colour.
    static let eventsBar = Color(light: 0xC8C8C4, dark: 0x464640)
    /// The day pager's circles on a day other than today (#E7E4DC on light).
    static let pagerFill = Color(light: 0xE7E4DC, dark: 0x2C2C2C)
    /// Settings: the rounded square behind each row's icon.
    static let iconTile = Color(light: 0xEAEAE1, dark: 0x2E2E2E)
    /// Settings: the cards that group rows (#F6F6F2 light, #242424 dark).
    static let settingsCard = Color(light: 0xF6F6F2, dark: 0x242424)
    /// The dark capsule of "Start meeting" on an upcoming event's page.
    static let startMeeting = Color(light: 0x262626, dark: 0xF0F0EA)
    static let onStartMeeting = Color(light: 0xFFFFFF, dark: 0x1C1C17)

    static let gutter: CGFloat = 16
    /// Letter spacing of note titles: the reference's serif sets about 10% wider than New York.
    static let noteTitleTracking: CGFloat = 0.8
    /// Baseline-to-baseline of a title that wraps: 42 pt in the reference.
    static let noteTitleLineSpacing: CGFloat = 2.5
    static let railWidth: CGFloat = 64

    /// Flat, as measured in the reference recordings (#F0F0EA light, #1E1E1E
    /// dark, top to bottom). The lighter tone appears only in the fade beneath a floating bar.
    static var background: some View {
        bgTop.ignoresSafeArea()
    }
}

/// The Theme setting: Light, Dark, or Auto (follow the system). Auto is the default.
enum ThemePreference: String, CaseIterable, Identifiable, Sendable {
    case light, dark, auto
    static let storageKey = "appearance.theme"
    var id: String { rawValue }
    var title: String {
        switch self {
        case .light: "Light"
        case .dark: "Dark"
        case .auto: "Auto"
        }
    }
    var symbol: String {
        switch self {
        case .light: "sun.max"
        case .dark: "moon"
        case .auto: "circle.lefthalf.filled"
        }
    }
    /// Nil leaves the choice to the system.
    var colorScheme: ColorScheme? {
        switch self {
        case .light: .light
        case .dark: .dark
        case .auto: nil
        }
    }
    /// The interface style for UIKit windows (sheets, alerts), which a SwiftUI
    /// `preferredColorScheme` does not always reach.
    var interfaceStyle: UIUserInterfaceStyle {
        switch self {
        case .light: .light
        case .dark: .dark
        case .auto: .unspecified
        }
    }
}

extension Font {
    static func display(_ size: CGFloat = 34) -> Font { .system(size: size, weight: .regular, design: .serif) }
    static let rowTitle = Font.system(size: 16, weight: .semibold)
    static let rowSubtitle = Font.system(size: 14)
    static let sectionHeader = Font.system(size: 17)
    static let noteHeading = Font.system(size: 23, weight: .semibold)
    static let noteBody = Font.system(size: 16)
    /// Note titles on the note and recording screens.
    /// Measured against the reference at 3 px per point: the same cap height (23 pt) as
    /// Granola's title at 33 pt, with `Theme.noteTitleTracking` for its wider set.
    static let noteTitle = Font.system(size: 33, weight: .regular, design: .serif)
}

/// Applies the Theme setting to every window of the app.
@MainActor enum ThemeWindows {
    static func apply(_ theme: ThemePreference) {
        for scene in UIApplication.shared.connectedScenes {
            guard let scene = scene as? UIWindowScene else { continue }
            for window in scene.windows { window.overrideUserInterfaceStyle = theme.interfaceStyle }
        }
    }
}

extension Color {
    /// A colour with a value for each appearance.
    init(light: UInt32, dark: UInt32) {
        self.init(light: UIColor(hex: light), dark: UIColor(hex: dark))
    }

    init(light: UIColor, dark: UIColor) {
        self.init(uiColor: UIColor { $0.userInterfaceStyle == .dark ? dark : light })
    }

    init(hex: UInt32, opacity: Double = 1) {
        self.init(
            .sRGB,
            red: Double((hex >> 16) & 0xFF) / 255,
            green: Double((hex >> 8) & 0xFF) / 255,
            blue: Double(hex & 0xFF) / 255,
            opacity: opacity
        )
    }
}

extension UIColor {
    convenience init(hex: UInt32, alpha: CGFloat = 1) {
        self.init(red: CGFloat((hex >> 16) & 0xFF) / 255, green: CGFloat((hex >> 8) & 0xFF) / 255,
                  blue: CGFloat(hex & 0xFF) / 255, alpha: alpha)
    }
}

/// Text people read at length scales with the system text size. Sizes are the
/// measured ones at the default setting; chrome keeps fixed sizes.
enum ReadingFont {
    case body, heading, rowTitle, rowSubtitle
    /// Literal code inside a note or an answer. It grows with the text around it.
    case code

    var size: CGFloat {
        switch self {
        case .body: 16
        case .heading: 23
        case .rowTitle: 15
        case .rowSubtitle, .code: 14
        }
    }
    var weight: Font.Weight {
        switch self {
        case .body, .rowSubtitle, .code: .regular
        case .heading, .rowTitle: .semibold
        }
    }
    var style: Font.TextStyle {
        switch self {
        case .body, .code: .body
        case .heading: .title2
        case .rowTitle, .rowSubtitle: .subheadline
        }
    }
}

private struct ReadingFontModifier: ViewModifier {
    let kind: ReadingFont
    @ScaledMetric private var size: CGFloat

    init(kind: ReadingFont) {
        self.kind = kind
        _size = ScaledMetric(wrappedValue: kind.size, relativeTo: kind.style)
    }

    func body(content: Content) -> some View {
        content.font(.system(size: size, weight: kind.weight, design: kind == .code ? .monospaced : .default))
    }
}

extension View {
    func readingFont(_ kind: ReadingFont) -> some View { modifier(ReadingFontModifier(kind: kind)) }
}
