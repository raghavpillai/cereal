// The parts of Cereal that belong to macOS rather than to its window: the
// menu bar item and its menu, the login item and whether the app is in the
// Dock. Rust decides everything that is shown (see `src/menubar.rs`); this
// file draws it and reports what was chosen. Notifications are in
// `Notifications.swift`.
import AppKit
import ServiceManagement

/// Called on the main thread with a command and its argument.
public typealias CerealShellCallback = @convention(c) @Sendable (UnsafePointer<CChar>, UnsafePointer<CChar>) -> Void

private struct ShellEntry: Decodable, Sendable {
    let kind: String
    let title: String?
    let subtitle: String?
    let command: String?
    let argument: String?
    let enabled: Bool?
    let checked: Bool?
    let items: [ShellEntry]?
}

private struct ShellModel: Decodable, Sendable {
    let title: String?
    let tooltip: String
    let items: [ShellEntry]
}

/// The outlines of Cereal's mark, read from the path of its SVG icon: only
/// `M`, implicit line-to pairs and `Z`, in a 24 by 24 box.
private func markOutlines(_ path: String) -> [[CGPoint]] {
    var outlines: [[CGPoint]] = []
    var current: [CGPoint] = []
    let spaced = path.replacingOccurrences(of: "Z", with: " Z ").replacingOccurrences(of: "M", with: " M ")
    for token in spaced.split(whereSeparator: { $0 == " " || $0 == "\n" }) {
        if token == "M" || token == "Z" {
            if !current.isEmpty { outlines.append(current) }
            current = []
            continue
        }
        let pair = token.split(separator: ",")
        if pair.count == 2, let x = Double(pair[0]), let y = Double(pair[1]) {
            current.append(CGPoint(x: x, y: y))
        }
    }
    if !current.isEmpty { outlines.append(current) }
    return outlines
}

@MainActor
final class MenuBar: NSObject {
    static let shared = MenuBar()
    private var item: NSStatusItem?
    private var callback: CerealShellCallback?
    private var mark: NSImage?

    func install(markPath: String, name: String, callback: CerealShellCallback) {
        self.callback = callback
        guard item == nil else { return }
        let outlines = markOutlines(markPath)
        // Drawn as vectors at whatever scale the menu bar has, and a
        // template: macOS tints it for light and dark menu bars.
        let side: CGFloat = 17
        let image = NSImage(size: NSSize(width: side, height: side), flipped: true) { _ in
            let path = NSBezierPath()
            path.windingRule = .evenOdd
            for outline in outlines {
                guard let first = outline.first else { continue }
                path.move(to: first)
                for point in outline.dropFirst() { path.line(to: point) }
                path.close()
            }
            let scale = AffineTransform(scale: side / 24)
            path.transform(using: scale)
            NSColor.black.setFill()
            path.fill()
            return true
        }
        image.isTemplate = true
        image.accessibilityDescription = "Cereal"
        mark = image
        let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
        item.autosaveName = name
        item.behavior = []
        if let button = item.button {
            button.image = image
            button.imagePosition = .imageLeading
            // Digits of one width, so the countdown does not shift its
            // neighbours every minute.
            let size = NSFont.menuBarFont(ofSize: 0).pointSize
            button.font = NSFont.monospacedDigitSystemFont(ofSize: size, weight: .regular)
            button.setAccessibilityLabel("Cereal")
        }
        let menu = NSMenu()
        menu.autoenablesItems = false
        item.menu = menu
        self.item = item
    }

    fileprivate func update(_ model: ShellModel) {
        guard let item else { return }
        if let button = item.button {
            button.title = model.title.map { " " + $0 } ?? ""
            button.toolTip = model.tooltip
        }
        if let menu = item.menu {
            menu.removeAllItems()
            fill(menu, model.items)
        }
    }

    func remove() {
        if let item { NSStatusBar.system.removeStatusItem(item) }
        item = nil
    }

    /// Where the item is on screen (points, origin bottom-left), or zeros.
    func frame() -> NSRect {
        guard let window = item?.button?.window else { return .zero }
        return window.frame
    }

    private func fill(_ menu: NSMenu, _ entries: [ShellEntry]) {
        for entry in entries {
            switch entry.kind {
            case "separator":
                menu.addItem(.separator())
            case "header":
                let row = NSMenuItem(title: entry.title ?? "", action: nil, keyEquivalent: "")
                row.isEnabled = false
                menu.addItem(row)
            case "submenu":
                let row = NSMenuItem(title: entry.title ?? "", action: nil, keyEquivalent: "")
                let submenu = NSMenu()
                submenu.autoenablesItems = false
                fill(submenu, entry.items ?? [])
                row.submenu = submenu
                menu.addItem(row)
            default:
                let row = NSMenuItem(
                    title: entry.title ?? "", action: #selector(picked(_:)), keyEquivalent: "")
                row.target = self
                row.representedObject = [entry.command ?? "", entry.argument ?? ""]
                row.isEnabled = entry.enabled ?? true
                row.state = (entry.checked ?? false) ? .on : .off
                if let subtitle = entry.subtitle, !subtitle.isEmpty {
                    if #available(macOS 14.4, *) {
                        row.subtitle = subtitle
                    } else {
                        let text = NSMutableAttributedString(
                            string: entry.title ?? "",
                            attributes: [.font: NSFont.menuFont(ofSize: 0)])
                        text.append(
                            NSAttributedString(
                                string: "\n" + subtitle,
                                attributes: [
                                    .font: NSFont.menuFont(ofSize: NSFont.smallSystemFontSize),
                                    .foregroundColor: NSColor.secondaryLabelColor,
                                ]))
                        row.attributedTitle = text
                    }
                }
                menu.addItem(row)
            }
        }
    }

    @objc private func picked(_ sender: NSMenuItem) {
        guard let pair = sender.representedObject as? [String], pair.count == 2 else { return }
        send(pair[0], pair[1])
    }

    func send(_ command: String, _ argument: String) {
        guard let callback else { return }
        command.withCString { command in
            argument.withCString { argument in callback(command, argument) }
        }
    }
}

func decoded<T: Decodable>(_ type: T.Type, _ json: UnsafePointer<CChar>) -> T? {
    try? JSONDecoder().decode(type, from: Data(String(cString: json).utf8))
}

@_cdecl("cereal_menubar_install")
public func cerealMenuBarInstall(
    _ markPath: UnsafePointer<CChar>, _ name: UnsafePointer<CChar>, _ callback: CerealShellCallback
) {
    let path = String(cString: markPath)
    let name = String(cString: name)
    MainActor.assumeIsolated {
        MenuBar.shared.install(markPath: path, name: name, callback: callback)
        Notifications.shared.listen()
    }
}

@_cdecl("cereal_menubar_update")
public func cerealMenuBarUpdate(_ json: UnsafePointer<CChar>) -> Bool {
    guard let model = decoded(ShellModel.self, json) else { return false }
    MainActor.assumeIsolated { MenuBar.shared.update(model) }
    return true
}

@_cdecl("cereal_menubar_remove")
public func cerealMenuBarRemove() {
    MainActor.assumeIsolated { MenuBar.shared.remove() }
}

/// The item's frame on screen as x, y, width, height (points, origin at the
/// bottom-left of the primary display). For captures.
@_cdecl("cereal_menubar_frame")
public func cerealMenuBarFrame(_ output: UnsafeMutablePointer<Double>) {
    let frame = MainActor.assumeIsolated { MenuBar.shared.frame() }
    output[0] = frame.origin.x
    output[1] = frame.origin.y
    output[2] = frame.size.width
    output[3] = frame.size.height
}

/// In the Dock and the app switcher, or only in the menu bar.
@_cdecl("cereal_app_set_dock_visible")
public func cerealAppSetDockVisible(_ visible: Bool) {
    MainActor.assumeIsolated {
        let policy: NSApplication.ActivationPolicy = visible ? .regular : .accessory
        if NSApp.activationPolicy() != policy {
            NSApp.setActivationPolicy(policy)
        }
        if !visible { NSApp.deactivate() }
    }
}

/// The login item's state: 0 not registered, 1 enabled, 2 waiting for
/// approval in System Settings, 3 not found, -1 when this is not an app
/// bundle (a bare executable cannot be a login item).
@_cdecl("cereal_login_item_status")
public func cerealLoginItemStatus() -> Int32 {
    guard Bundle.main.bundleURL.pathExtension == "app" else { return -1 }
    switch SMAppService.mainApp.status {
    case .notRegistered: return 0
    case .enabled: return 1
    case .requiresApproval: return 2
    case .notFound: return 3
    @unknown default: return 3
    }
}

/// Register or unregister the app as a login item. Returns false and writes
/// what macOS said when it refused.
@_cdecl("cereal_login_item_set")
public func cerealLoginItemSet(
    _ enabled: Bool, _ error: UnsafeMutablePointer<CChar>, _ capacity: Int
) -> Bool {
    guard Bundle.main.bundleURL.pathExtension == "app" else {
        _ = "Only the packaged app can open at login".withCString { strlcpy(error, $0, capacity) }
        return false
    }
    do {
        if enabled {
            try SMAppService.mainApp.register()
        } else {
            try SMAppService.mainApp.unregister()
        }
        return true
    } catch let problem {
        _ = problem.localizedDescription.withCString { strlcpy(error, $0, capacity) }
        return false
    }
}
