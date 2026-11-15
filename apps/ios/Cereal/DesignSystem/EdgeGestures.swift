import SwiftUI
import UIKit
import os

/// The rules both edge gestures follow, as pure functions so they can be tested.
enum EdgeGestureRules {
    /// Touches that start within this distance of the leading edge can pop or
    /// open the sidebar. Wide enough for a thumb, narrow enough to stay clear
    /// of text that starts at the 16 pt gutter.
    static let edgeWidth: CGFloat = 20
    /// OS versions on which taking over the navigation controller's pop
    /// recognisers has been exercised. Anything else starts on the fallback.
    static let testedMajorVersions = 26...27

    static func startsAtLeadingEdge(x: CGFloat, width: CGFloat, rightToLeft: Bool) -> Bool {
        rightToLeft ? x >= width - edgeWidth : x <= edgeWidth
    }

    /// Never at the root (a pop with nothing to pop freezes the stack), and
    /// never while a push or pop is still animating or something is presented.
    static func mayPop(stackDepth: Int, inTransition: Bool, presenting: Bool) -> Bool {
        stackDepth > 1 && !inTransition && !presenting
    }

    static func mayOpenSidebar(stackDepth: Int, inTransition: Bool, presenting: Bool) -> Bool {
        stackDepth == 1 && !inTransition && !presenting
    }

    /// A drag into the screen that is more horizontal than vertical. A mostly
    /// vertical drag that happens to start at the edge is left to scrolling.
    static func isInwardHorizontal(dx: CGFloat, dy: CGFloat) -> Bool {
        dx > 0 && abs(dx) > abs(dy)
    }

    /// A text-selection handle at the start of a line sits inside the edge
    /// zone; dragging it must extend the selection, not go back.
    static func isTextSelectionHandle(_ view: UIView?) -> Bool {
        var current = view
        while let candidate = current {
            if candidate is any UITextSelectionHandleView { return true }
            current = candidate.superview
        }
        return false
    }

    static func takeover(hasNavigationController: Bool, hasPopRecogniser: Bool, osMajorVersion: Int) -> EdgeGestureHealth.Status {
        guard testedMajorVersions.contains(osMajorVersion) else { return .fallback("iOS \(osMajorVersion) has not been tested with the takeover") }
        guard hasNavigationController else { return .fallback("no navigation controller behind the stack") }
        guard hasPopRecogniser else { return .fallback("no interactive pop recogniser") }
        return .takeover
    }
}

/// Whether the interactive edge gestures are installed. If the expected
/// recognisers are missing (a future OS that builds navigation differently),
/// this is logged once and the app falls back to a simple edge strip, so
/// swipe-back and the sidebar are never both lost.
@MainActor @Observable final class EdgeGestureHealth {
    static let shared = EdgeGestureHealth()
    enum Status: Equatable { case unknown, takeover, fallback(String) }
    private(set) var status: Status = .unknown
    @ObservationIgnored private let log = Logger(subsystem: AppIdentity.subsystem, category: "gestures")

    func report(_ new: Status) {
        guard status != new else { return }
        status = new
        if case .fallback(let reason) = new {
            log.error("Interactive edge gestures unavailable: \(reason, privacy: .public). Using the edge strip fallback.")
        }
    }

    var usesFallback: Bool { if case .fallback = status { true } else { false } }
}

/// Every screen hides the system back button, which also switches off the
/// navigation controller's swipe-back. This turns it back on for any screen
/// deeper than the root, limited to touches that start at the leading edge.
struct SwipeBackEnabler: UIViewControllerRepresentable {
    func makeUIViewController(context: Context) -> SwipeBackController { SwipeBackController() }
    func updateUIViewController(_ controller: SwipeBackController, context: Context) {}
}

final class SwipeBackController: UIViewController {
    override func didMove(toParent parent: UIViewController?) {
        super.didMove(toParent: parent)
        if navigationController != nil { enable() }
    }

    override func viewDidAppear(_ animated: Bool) {
        super.viewDidAppear(animated)
        enable()
    }

    /// The system can reset the recognisers when it rebuilds the navigation
    /// item (rotation, a fold, a toolbar change). Re-applying is two comparisons.
    override func viewWillLayoutSubviews() {
        super.viewWillLayoutSubviews()
        if navigationController != nil { enable() }
    }

    func enable() {
        let navigation = navigationController
        let status = EdgeGestureRules.takeover(
            hasNavigationController: navigation != nil,
            hasPopRecogniser: navigation?.interactivePopGestureRecognizer != nil,
            osMajorVersion: ProcessInfo.processInfo.operatingSystemVersion.majorVersion)
        EdgeGestureHealth.shared.report(status)
        guard status == .takeover, let navigation else { return }
        let delegate: SwipeBackDelegate
        if let existing = objc_getAssociatedObject(navigation, &SwipeBackDelegate.key) as? SwipeBackDelegate {
            delegate = existing
        } else {
            delegate = SwipeBackDelegate(navigation: navigation)
            // Recognisers hold their delegate weakly; the navigation controller keeps it alive.
            objc_setAssociatedObject(navigation, &SwipeBackDelegate.key, delegate, .OBJC_ASSOCIATION_RETAIN_NONATOMIC)
        }
        var recognisers = [navigation.interactivePopGestureRecognizer]
        // iOS 26 pops through a second recogniser that covers the content.
        if #available(iOS 26.0, *) { recognisers.append(navigation.interactiveContentPopGestureRecognizer) }
        for recogniser in recognisers.compactMap({ $0 }) {
            if recogniser.delegate !== delegate { recogniser.delegate = delegate }
            if !recogniser.isEnabled { recogniser.isEnabled = true }
        }
        // On the root screen the same edge pulls the notes sidebar in.
        SidebarEdgeGesture.install(on: navigation)
    }
}

final class SwipeBackDelegate: NSObject, UIGestureRecognizerDelegate {
    nonisolated(unsafe) static var key: UInt8 = 0
    weak var navigation: UINavigationController?

    init(navigation: UINavigationController) { self.navigation = navigation }

    func gestureRecognizerShouldBegin(_ gestureRecognizer: UIGestureRecognizer) -> Bool {
        guard let navigation,
              EdgeGestureRules.mayPop(stackDepth: navigation.viewControllers.count,
                                      inTransition: navigation.transitionCoordinator != nil,
                                      presenting: navigation.presentedViewController != nil) else { return false }
        // The content-wide recogniser has no direction rule of ours otherwise.
        if let pan = gestureRecognizer as? UIPanGestureRecognizer, let view = pan.view {
            let velocity = pan.velocity(in: view)
            let direction: CGFloat = view.effectiveUserInterfaceLayoutDirection == .rightToLeft ? -1 : 1
            // A recogniser that has not measured any movement yet is left to its own judgement.
            if velocity != .zero, !EdgeGestureRules.isInwardHorizontal(dx: velocity.x * direction, dy: velocity.y) { return false }
        }
        return true
    }

    /// Only from the leading edge, so the swipe never competes with horizontal
    /// scrolling, text selection or the keyboard-dismiss drag in the content.
    func gestureRecognizer(_ gestureRecognizer: UIGestureRecognizer, shouldReceive touch: UITouch) -> Bool {
        guard let view = navigation?.view, !EdgeGestureRules.isTextSelectionHandle(touch.view) else { return false }
        return EdgeGestureRules.startsAtLeadingEdge(x: touch.location(in: view).x, width: view.bounds.width,
                                                    rightToLeft: view.effectiveUserInterfaceLayoutDirection == .rightToLeft)
    }
}

/// Used only when the takeover is unavailable. A swipe in from the leading
/// edge goes back on a pushed screen and opens the sidebar on Home. It does
/// not track the finger, but neither feature disappears.
struct EdgeFallbackStrip: View {
    @Environment(Router.self) private var router
    @Environment(\.layoutDirection) private var layoutDirection

    var body: some View {
        if EdgeGestureHealth.shared.usesFallback {
            Color.clear
                .frame(width: EdgeGestureRules.edgeWidth)
                .contentShape(Rectangle())
                .ignoresSafeArea()
                .gesture(DragGesture(minimumDistance: 16).onEnded { value in
                    let dx = value.translation.width * (layoutDirection == .rightToLeft ? -1 : 1)
                    guard dx > 50, EdgeGestureRules.isInwardHorizontal(dx: dx, dy: value.translation.height) else { return }
                    if router.path.isEmpty { SidebarModel.shared.open() } else { router.path.removeLast() }
                })
                .accessibilityHidden(true)
        }
    }
}
