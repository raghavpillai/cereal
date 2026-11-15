import SwiftUI

enum Theme {
    static let bgTop = Color(hex: 0x1E1E1E)
    static let bgBottom = Color(hex: 0x262626)
    static let surface = Color(hex: 0x292929)
    static let surfaceRaised = Color(hex: 0x333333)
    static let stroke = Color.white.opacity(0.08)
    static let glassTint = Color.white.opacity(0.06)
    static let divider = Color(hex: 0x2E2E2E)
    static let textPrimary = Color(hex: 0xF2F2F2)
    static let textSecondary = Color.white.opacity(0.55)
    static let textTertiary = Color.white.opacity(0.35)
    /// Cereal gold. Brand colours come from `scripts/logo/gold_palette.py`, never by eye.
    static let accent = Color(hex: 0xE3B025)
    static let onAccent = Color(hex: 0x1A1A1A)
    static let danger = Color(hex: 0xFF5A4F)
    static let lightButton = Color(hex: 0xECECE6)
    /// What the system gives this app's full-height sheets in the dark theme,
    /// measured on screen (the saved transcript, Workspaces). For a sheet that
    /// must not be left to the system material; see `glassSheet(background:)`.
    static let sheet = Color(hex: 0x1C1C1E)

    // Chat has a brighter composer, darker questions and distinct idle/active
    // action colors in the supplied mobile recordings. These are deliberately
    // separate from meeting cards and the gold recording indicator.
    static let chatComposer = Color(hex: 0x313131)
    /// Keeps the composer's glass close to the reference's dark bar.
    static let chatComposerTint = Color.black.opacity(0.2)
    static let chatQuestion = Color(hex: 0x272727)
    static let chatAction = Color(hex: 0x755400)
    static let chatActionGlyph = Color.black
    static let chatMicrophone = Color(hex: 0xF0E2C3)
    static let chatText = Color.white

    static let gutter: CGFloat = 16
    static let railWidth: CGFloat = 64

    /// Flat, as measured in the reference recordings (#1E1E1E from top to
    /// bottom). The lighter tone appears only in the fade beneath a floating bar.
    static var background: some View {
        bgTop.ignoresSafeArea()
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
    static let noteTitle = Font.system(size: 32, weight: .regular, design: .serif)
}

extension Color {
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

/// Text people read at length scales with the system text size. Sizes are the
/// measured ones at the default setting; chrome keeps fixed sizes.
enum ReadingFont {
    case body, heading, rowTitle, rowSubtitle

    var size: CGFloat {
        switch self {
        case .body: 16
        case .heading: 23
        case .rowTitle: 15
        case .rowSubtitle: 14
        }
    }
    var weight: Font.Weight {
        switch self {
        case .body, .rowSubtitle: .regular
        case .heading, .rowTitle: .semibold
        }
    }
    var style: Font.TextStyle {
        switch self {
        case .body: .body
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
        content.font(.system(size: size, weight: kind.weight))
    }
}

extension View {
    func readingFont(_ kind: ReadingFont) -> some View { modifier(ReadingFontModifier(kind: kind)) }
}
