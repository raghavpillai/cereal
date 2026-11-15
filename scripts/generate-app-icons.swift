// Rasterises the committed Cereal logo masters into every bitmap the apps ship:
// the desktop Dock icon PNGs, the packaged ICNS, the iOS app icon set and the
// web app's touch icon.
// The artwork itself is original and is drawn by scripts/logo/cereal_logo.py.
import AppKit
import Foundation
import ImageIO
import UniformTypeIdentifiers

let root = URL(fileURLWithPath: CommandLine.arguments.dropFirst().first ?? FileManager.default.currentDirectoryPath, isDirectory: true)
let desktopIcons = root.appendingPathComponent("apps/desktop/assets/app-icons", isDirectory: true)
let iosMasters = root.appendingPathComponent("scripts/logo/ios", isDirectory: true)
let iosIconSet = root.appendingPathComponent("apps/ios/Cereal/Resources/Assets.xcassets/AppIcon.appiconset", isDirectory: true)
let webAssets = root.appendingPathComponent("apps/web/src/assets", isDirectory: true)

struct IconError: Error, CustomStringConvertible { let description: String }

/// Draws an SVG master at an exact pixel size. iOS rejects icons with an alpha
/// channel, so opaque renders use a context that has none to write.
func render(_ master: URL, pixels: Int, opaque: Bool, to destination: URL) throws {
    guard let image = NSImage(contentsOf: master) else { throw IconError(description: "Cannot read \(master.path)") }
    let alpha: CGImageAlphaInfo = opaque ? .noneSkipLast : .premultipliedLast
    guard let context = CGContext(
        data: nil, width: pixels, height: pixels, bitsPerComponent: 8, bytesPerRow: 0,
        space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: alpha.rawValue
    ) else { throw IconError(description: "Cannot create a \(pixels) px bitmap") }
    context.interpolationQuality = .high
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(cgContext: context, flipped: false)
    image.draw(in: NSRect(x: 0, y: 0, width: pixels, height: pixels), from: .zero, operation: .sourceOver, fraction: 1)
    NSGraphicsContext.restoreGraphicsState()
    guard let bitmap = context.makeImage(),
          let writer = CGImageDestinationCreateWithURL(destination as CFURL, UTType.png.identifier as CFString, 1, nil)
    else { throw IconError(description: "Cannot write \(destination.path)") }
    CGImageDestinationAddImage(writer, bitmap, nil)
    guard CGImageDestinationFinalize(writer) else { throw IconError(description: "Cannot write \(destination.path)") }
}

// Desktop: one 1024 px PNG per Dock icon choice, plus a small one for the
// Settings picker (58 pt tiles), which would alias if it scaled the large PNG.
for variant in ["gold", "midnight", "paper"] {
    let master = desktopIcons.appendingPathComponent("cereal-\(variant).svg")
    try render(master, pixels: 1024, opaque: false, to: desktopIcons.appendingPathComponent("cereal-\(variant).png"))
    try render(master, pixels: 116, opaque: false, to: desktopIcons.appendingPathComponent("cereal-\(variant)-thumb.png"))
}

// Desktop: the packaged ICNS, each size drawn from the vector rather than downscaled.
let work = FileManager.default.temporaryDirectory.appendingPathComponent("cereal-icons-\(UUID().uuidString)", isDirectory: true)
let iconset = work.appendingPathComponent("Cereal.iconset", isDirectory: true)
try FileManager.default.createDirectory(at: iconset, withIntermediateDirectories: true)
defer { try? FileManager.default.removeItem(at: work) }
let gold = desktopIcons.appendingPathComponent("cereal-gold.svg")
for points in [16, 32, 128, 256, 512] {
    try render(gold, pixels: points, opaque: false, to: iconset.appendingPathComponent("icon_\(points)x\(points).png"))
    try render(gold, pixels: points * 2, opaque: false, to: iconset.appendingPathComponent("icon_\(points)x\(points)@2x.png"))
}
let iconutil = Process()
iconutil.executableURL = URL(fileURLWithPath: "/usr/bin/iconutil")
iconutil.arguments = ["-c", "icns", iconset.path, "-o", desktopIcons.appendingPathComponent("app.icns").path]
try iconutil.run()
iconutil.waitUntilExit()
guard iconutil.terminationStatus == 0 else { throw IconError(description: "iconutil failed") }

// iOS: light, dark and tinted appearances of the single-size app icon.
try FileManager.default.createDirectory(at: iosIconSet, withIntermediateDirectories: true)
for name in ["AppIcon", "AppIcon-Dark", "AppIcon-Tinted"] {
    try render(
        iosMasters.appendingPathComponent("\(name).svg"), pixels: 1024, opaque: true,
        to: iosIconSet.appendingPathComponent("\(name).png")
    )
}

// Web: the home-screen icon, the full-bleed light appearance at 180 px with no alpha.
try render(
    iosMasters.appendingPathComponent("AppIcon.svg"), pixels: 180, opaque: true,
    to: webAssets.appendingPathComponent("apple-touch-icon.png")
)
