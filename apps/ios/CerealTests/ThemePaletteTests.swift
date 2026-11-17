import SwiftUI
import Testing
import UIKit
@testable import Cereal

/// The brand accent is Cereal gold. Values come from `scripts/logo/gold_palette.py`
/// (OKLCH, lightness and chroma of the old lime kept, hue moved to gold), so the
/// contrast each colour had in its role is the contrast it still has.
struct ThemePaletteTests {
    /// The colour as drawn in the dark theme, or in `style`.
    private func hex(_ color: Color, _ style: UIUserInterfaceStyle = .dark) -> UInt32 {
        var red: CGFloat = 0, green: CGFloat = 0, blue: CGFloat = 0, alpha: CGFloat = 0
        UIColor(color).resolvedColor(with: UITraitCollection(userInterfaceStyle: style)).getRed(&red, green: &green, blue: &blue, alpha: &alpha)
        return UInt32((red * 255).rounded()) << 16 | UInt32((green * 255).rounded()) << 8 | UInt32((blue * 255).rounded())
    }

    /// WCAG relative luminance of an sRGB colour.
    private func luminance(_ hex: UInt32) -> Double {
        func linear(_ value: UInt32) -> Double {
            let c = Double(value & 0xFF) / 255
            return c <= 0.04045 ? c / 12.92 : pow((c + 0.055) / 1.055, 2.4)
        }
        return 0.2126 * linear(hex >> 16) + 0.7152 * linear(hex >> 8) + 0.0722 * linear(hex)
    }

    private func contrast(_ a: UInt32, _ b: UInt32) -> Double {
        let (light, dark) = (max(luminance(a), luminance(b)), min(luminance(a), luminance(b)))
        return (light + 0.05) / (dark + 0.05)
    }

    @Test func theBrandColoursAreTheGoldPalette() {
        #expect(hex(Theme.accent) == 0xE3B025)
        // The send button's gold and its near-black arrow, as on the desktop.
        #expect(hex(Theme.sendGold) == 0xE0AF2F)
        #expect(hex(Theme.sendGlyph) == 0x1D1A12)
        // Neutrals are not brand colours and did not move.
        #expect(hex(Theme.onAccent) == 0x1A1A1A)
        #expect(hex(Theme.lightButton) == 0xECECE6)
        #expect(hex(Theme.danger) == 0xFF5A4F)
        #expect(hex(Theme.sheet) == 0x1C1C1E)
    }

    @Test func textOnTheAccentAndTheAccentOnDarkBackgroundsStayReadable() {
        // 8.70 and 8.34; the lime they replace measured 8.97 and 8.59.
        #expect(contrast(hex(Theme.onAccent), hex(Theme.accent)) > 8.5)
        #expect(contrast(hex(Theme.accent), hex(Theme.bgTop)) > 8)
        // The lightest surface gold text sits on (sheets).
        #expect(contrast(hex(Theme.accent), hex(Theme.surfaceRaised)) > 6)
        // The send arrow on its gold disc.
        #expect(contrast(hex(Theme.sendGlyph), hex(Theme.sendGold)) > 7)
    }

    @Test func theLightThemeIsTheReferencesWarmOffWhiteAndGoldTextStaysReadableOnIt() {
        // Measured from the reference recordings.
        #expect(hex(Theme.bgTop, .light) == 0xF0F0EA)
        #expect(hex(Theme.surface, .light) == 0xE7E4DC)
        #expect(hex(Theme.textPrimary, .light) == 0x1C1C17)
        #expect(hex(Theme.textSecondary, .light) == 0x5D5D57)
        #expect(contrast(hex(Theme.textPrimary, .light), hex(Theme.bgTop, .light)) > 14)
        #expect(contrast(hex(Theme.textSecondary, .light), hex(Theme.bgTop, .light)) > 5.5)
        // Gold as text is darkened on light; on dark it is the brand gold.
        #expect(contrast(hex(Theme.accentText, .light), hex(Theme.bgTop, .light)) > 4.5)
        #expect(hex(Theme.accentText) == hex(Theme.accent))
        // The dark theme did not move.
        #expect(hex(Theme.bgTop) == 0x1E1E1E && hex(Theme.textPrimary) == 0xF2F2F2)
        // The send button and the + are the same gold on both.
        #expect(hex(Theme.sendGold, .light) == hex(Theme.sendGold) && hex(Theme.accent, .light) == hex(Theme.accent))
    }

    @Test func theThemeSettingDefaultsToAutoAndAutoFollowsTheSystem() {
        #expect(ThemePreference(rawValue: UserDefaults(suiteName: "theme-\(UUID())")?.string(forKey: ThemePreference.storageKey) ?? "auto") == .auto)
        #expect(ThemePreference.auto.colorScheme == nil && ThemePreference.light.colorScheme == .light && ThemePreference.dark.colorScheme == .dark)
        #expect(ThemePreference.allCases.map(\.title) == ["Light", "Dark", "Auto"])
    }

    @Test func noLetterTileIsGreen() {
        // Five pastels chosen by the letter; A...E reach every one of them.
        let tiles = Set(["A", "B", "C", "D", "E"].map(NoteTile.colour(for:)))
        #expect(tiles == [0xD9D2F0, 0xF3E3A1, 0xF2D6E1, 0xF2D3C4, 0xCFE0F3])
    }
}
