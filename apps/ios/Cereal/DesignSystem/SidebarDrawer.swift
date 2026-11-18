import SwiftUI
import APIClient

/// State of the notes sidebar. It is hidden at rest on every layout and is
/// pulled in from the leading edge of Home, or opened with the sidebar button.
@MainActor @Observable final class SidebarModel {
    static let shared = SidebarModel()

    /// 0 is closed, 1 is fully open. Follows the finger while dragging.
    private(set) var progress: CGFloat = 0
    private(set) var isDragging = false
    /// Counts the times the panel went to a side it was not resting on, which is felt:
    /// a drag let go, and equally the button, the close control, the dimmed page or a row.
    private(set) var snaps = 0
    @ObservationIgnored var width: CGFloat = 300
    @ObservationIgnored var openAnimation: Animation = Motion(reduceMotion: false).settle
    @ObservationIgnored var closeAnimation: Animation = Motion(reduceMotion: false).exit(.settle)
    @ObservationIgnored private var dragStart: CGFloat = 0
    @ObservationIgnored private var dragOrigin: CGFloat = 0
    /// True during a drag and for a moment after it, so the touch that moved
    /// the sidebar cannot also activate the row it ended on.
    @ObservationIgnored private(set) var suppressesRowTaps = false
    @ObservationIgnored private var release: Task<Void, Never>?

    var isVisible: Bool { progress > 0.001 }
    var isOpen: Bool { progress > 0.999 }

    /// `translation` is the gesture's translation at the moment it becomes a
    /// drawer drag, so a drag that began as a vertical scroll does not jump.
    func beginDrag(at translation: CGFloat = 0) {
        isDragging = true
        dragStart = progress
        dragOrigin = translation
        release?.cancel()
        suppressesRowTaps = true
        Signpost.event("Sidebar.DragBegan")
    }

    func drag(translation: CGFloat) {
        if !isDragging { beginDrag(at: translation) }
        progress = Self.progress(start: dragStart, translation: translation - dragOrigin, width: width)
    }

    /// The system took the touch away (app switcher, an alert, a fold). Finish
    /// the move from wherever the panel is, as if the finger had lifted.
    func cancelDrag() {
        guard isDragging else { return }
        endDrag(velocity: 0)
    }

    func endDrag(velocity: CGFloat) {
        guard isDragging else { return }
        isDragging = false
        release = Task { [weak self] in
            try? await Task.sleep(for: .milliseconds(250))
            if !Task.isCancelled { self?.suppressesRowTaps = false }
        }
        let open = Self.shouldOpen(progress: progress, velocity: velocity)
        // Felt only when the panel moves to a side it was not already resting on.
        if Self.snapIsFelt(from: dragStart, open: open) { snaps &+= 1 }
        settle(open: open)
    }

    /// A drag that ends where it began (a peek let go) settles silently.
    nonisolated static func snapIsFelt(from start: CGFloat, open: Bool) -> Bool {
        open ? start < 0.999 : start > 0.001
    }

    func open() { move(open: true) }
    func close() { move(open: false) }

    private func move(open: Bool) {
        clearDrag()
        if Self.snapIsFelt(from: progress, open: open) { snaps &+= 1 }
        settle(open: open)
    }

    /// Opening or closing by any other means leaves no drag state behind, so
    /// rows can never be left ignoring taps.
    private func clearDrag() {
        isDragging = false
        release?.cancel()
        release = nil
        suppressesRowTaps = false
    }

    private func settle(open: Bool) {
        isDragging = false
        withAnimation(open ? openAnimation : closeAnimation) { progress = open ? 1 : 0 }
    }

    /// The panel moves exactly as far as the finger, clamped to its travel.
    nonisolated static func progress(start: CGFloat, translation: CGFloat, width: CGFloat) -> CGFloat {
        min(1, max(0, start + translation / max(width, 1)))
    }

    /// A flick decides by direction; otherwise the panel goes to the nearer side.
    nonisolated static func shouldOpen(progress: CGFloat, velocity: CGFloat) -> Bool {
        if velocity > 400 { return true }
        if velocity < -400 { return false }
        return progress > 0.4
    }
}

/// The sidebar: Home, Account and server, Shared with me, and My notes. This wrapper is
/// the only view that follows the drag; it moves the panel and dims the page.
/// The panel's contents are a separate view that is not rebuilt per touch.
///
/// **A surface that slides (a drawer, a panel) stays in the tree and is moved
/// by its offset alone.** Adding or removing a glass surface lets the material
/// run its own appearance: the glass arrived late on the way in and was gone
/// on the first frame of the way out, leaving the rows to slide away as bare
/// text. At rest it sits off screen, takes no touches and is hidden from
/// accessibility.
struct SidebarDrawer: View {
    @Environment(\.motion) private var motion
    @Environment(\.layoutDirection) private var layoutDirection
    @GestureState private var closing = false
    private var model: SidebarModel { SidebarModel.shared }

    var body: some View {
        GeometryReader { proxy in
            let width = min(320, proxy.size.width * 0.82)
            let direction: CGFloat = layoutDirection == .rightToLeft ? -1 : 1
            let travel = (width + 16) * direction
            let visible = model.isVisible
            ZStack(alignment: .leading) {
                // Dims in and out with the panel, in the same animation.
                Color.black.opacity(0.42 * model.progress)
                    .ignoresSafeArea()
                    .contentShape(Rectangle())
                    .onTapGesture { model.close() }
                    .accessibilityHidden(true)
                SidebarPanel(visible: visible)
                    .frame(width: width)
                    // 8 pt from the top, leading and bottom edges of the screen.
                    .padding(.leading, 8).padding(.top, 8).padding(.bottom, 8)
                    .ignoresSafeArea(.container, edges: .bottom)
                    .offset(x: SidebarDrawer.offset(travel: travel, progress: model.progress, reduceMotion: motion.reduceMotion))
                    // Reduce Motion: the panel does not travel. Its place changes at once
                    // (an animated offset would still slide it) and it fades where it rests.
                    .transaction { if motion.reduceMotion { $0.animation = nil } }
                    .opacity(motion.reduceMotion ? model.progress : 1)
                    .accessibilityHidden(!visible)
                    .accessibilityAddTraits(.isModal)
                    .accessibilityAction(.escape) { model.close() }
            }
            // Dragging the open sidebar back towards the edge closes it, tracking the finger.
            .simultaneousGesture(
                DragGesture(minimumDistance: 12)
                    .updating($closing) { _, state, _ in state = true }
                    .onChanged { value in
                        guard model.isVisible, abs(value.translation.width) > abs(value.translation.height) || model.isDragging else { return }
                        model.drag(translation: value.translation.width * direction)
                    }
                    .onEnded { value in
                        model.endDrag(velocity: (value.predictedEndTranslation.width - value.translation.width) * 4 * direction)
                    },
                including: model.isVisible ? .all : .subviews
            )
            // A cancelled gesture never reaches onEnded; the gesture state still resets.
            .onChange(of: closing) { _, active in if !active { model.cancelDrag() } }
            .haptic(.snap, trigger: model.snaps)
            .onChange(of: width, initial: true) { _, new in model.width = new + 16 }
        }
        .allowsHitTesting(model.isVisible)
        .onChange(of: motion, initial: true) { _, motion in
            model.openAnimation = motion.settle
            model.closeAnimation = motion.exit(.settle)
        }
    }
}

extension SidebarDrawer {
    /// Where the panel is drawn at `progress`: off the leading edge by all of
    /// its travel when closed, in place when open. Under Reduce Motion it is in
    /// place whenever any of it shows, and parked off screen only when closed.
    nonisolated static func offset(travel: CGFloat, progress: CGFloat, reduceMotion: Bool) -> CGFloat {
        let progress = min(1, max(0, progress))
        if reduceMotion { return progress > 0.001 ? 0 : -travel }
        return -travel * (1 - progress)
    }
}

/// The panel's contents. Reads the notes and the path, never the drag progress.
private struct SidebarPanel: View {
    @Environment(NoteStore.self) private var store
    @Environment(Router.self) private var router
    @Environment(AccountsModel.self) private var accounts
    @Environment(ConnectionMode.self) private var connectionMode
    /// Whether any of the panel shows. It changes when the panel opens and closes, not per touch.
    let visible: Bool
    @AccessibilityFocusState private var focusedOnOpen: Bool
    private var model: SidebarModel { SidebarModel.shared }

    var body: some View {
        let _ = PerfProbe.hit("SidebarPanel.body")
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 2) {
                HStack {
                    Text("Cereal").displayFont(22).foregroundStyle(Theme.textPrimary)
                        .accessibilityAddTraits(.isHeader)
                    Spacer()
                    Button { model.close() } label: {
                        Image(systemName: "xmark").font(.system(size: 13, weight: .medium))
                            .foregroundStyle(Theme.textSecondary)
                            .frame(width: 44, height: 44).contentShape(Rectangle())
                    }
                    .buttonStyle(.squash)
                    .accessibilityLabel("Close sidebar")
                }
                .padding(.leading, 14).padding(.trailing, 2).padding(.top, 6)
                row("Home", symbol: "house", selected: router.path.isEmpty) { router.path = [] }
                    .accessibilityFocused($focusedOnOpen)
                row(Wording.accountSheet, symbol: "person.crop.circle", selected: false) { router.showConnection = true }
                row("Shared with me", symbol: "link", selected: selected(.sharedWithMe)) { router.path = [.sharedWithMe] }
                // As on the desktop, Chat opens a new conversation; the open one is kept in its history.
                if !connectionMode.isDemo, accounts.defaultSession?.notes.accountID != nil {
                    row("Chat", symbol: "bubble.left", selected: router.path.last.map(Self.isWorkspaceChat) ?? false) {
                        if let route = ChatNavigation.workspaceChat(accounts) { router.path = [route] }
                    }
                }
                Text("My notes").uiFont(13, weight: .medium).foregroundStyle(Theme.textSecondary)
                    .accessibilityAddTraits(.isHeader)
                    .padding(.horizontal, 14).padding(.top, 18).padding(.bottom, 6)
                if connectionMode.isDemo {
                    ForEach(store.activeNotes) { note in
                        row(note.displayTitle, symbol: "doc.text", selected: selected(.note(note.id))) { router.path = [.note(note.id)] }
                    }
                } else {
                    // Each account's notes, in the person's order. An account that must sign in again is left out.
                    ForEach(accounts.sessions.filter { $0.status != .needsSignIn }, id: \.key) { session in
                        if let account = session.notes.accountID {
                            ForEach(session.notes.list.rows) { note in
                                let ref = NoteRef(account: account, id: note.id)
                                row(note.title.isEmpty ? "Untitled note" : note.title, symbol: "doc.text", selected: selected(.serverNote(ref))) {
                                    router.path = [.serverNote(ref)]
                                }
                            }
                        }
                    }
                }
            }
            .padding(.horizontal, 8).padding(.bottom, 16)
        }
        .scrollIndicators(.hidden)
        .contentMargins(.bottom, 28, for: .scrollContent)
        // Rows fade out at the panel's lower edge instead of being cut by it.
        .mask(LinearGradient(stops: [.init(color: .black, location: 0), .init(color: .black, location: 0.93), .init(color: .clear, location: 1)],
                             startPoint: .top, endPoint: .bottom))
        .glassSurface(RoundedRectangle(cornerRadius: 28, style: .continuous), tint: Theme.sidebarTint)
        .onChange(of: visible) { _, shown in if shown { focusedOnOpen = true } }
        // Sidebar text grows with the text size up to the first accessibility size.
        .dynamicTypeSize(...DynamicTypeSize.accessibility1)
    }

    private func row(_ title: String, symbol: String, selected: Bool, action: @escaping () -> Void) -> some View {
        // A drag that closes the panel from a row gives that row's press up at once (`RowButton`).
        RowButton {
            guard !model.suppressesRowTaps else { return }
            action()
            model.close()
        } label: {
            HStack(spacing: 12) {
                Image(systemName: symbol).font(.system(size: 15, weight: .medium)).frame(width: 22)
                    .accessibilityHidden(true)
                Text(title).uiFont(15, weight: selected ? .semibold : .regular).lineLimit(2)
                    .multilineTextAlignment(.leading)
                Spacer(minLength: 0)
            }
            .foregroundStyle(selected ? Theme.accentText : Theme.textPrimary.opacity(0.9))
            .padding(.horizontal, 12).padding(.vertical, 11)
            .frame(minHeight: 44)
            .background(Theme.ink.opacity(selected ? 0.07 : 0), in: RoundedRectangle(cornerRadius: 14, style: .continuous))
            .contentShape(Rectangle())
        }
        .accessibilityAddTraits(selected ? .isSelected : [])
    }

    private static func isWorkspaceChat(_ route: Route) -> Bool {
        if case .serverChat(_, noteID: nil) = route { true } else { false }
    }

    private func selected(_ route: Route) -> Bool {
        if case .serverNote(let id) = route {
            return router.path.contains { value in
                switch value {
                case .serverNote(let candidate), .serverNoteSource(let candidate, _): candidate == id
                default: false
                }
            }
        }
        return router.path.contains(route)
    }
}

// MARK: - Edge gesture

/// A pan that starts at the leading edge of the root screen and drags the
/// sidebar in under the finger. It is separate from the system pop gesture:
/// it only receives touches when there is nothing to pop, so going back always
/// wins on a pushed screen. Touches that start anywhere else never reach it,
/// so list scrolling, row swipes and text selection are untouched.
@MainActor final class SidebarEdgeGesture: NSObject, UIGestureRecognizerDelegate {
    nonisolated(unsafe) static var key: UInt8 = 0
    static let edgeWidth = EdgeGestureRules.edgeWidth
    weak var navigation: UINavigationController?
    let recogniser = UIPanGestureRecognizer()

    init(navigation: UINavigationController) {
        self.navigation = navigation
        super.init()
        recogniser.addTarget(self, action: #selector(pan(_:)))
        recogniser.delegate = self
        recogniser.maximumNumberOfTouches = 1
        navigation.view.addGestureRecognizer(recogniser)
    }

    static func install(on navigation: UINavigationController) {
        guard objc_getAssociatedObject(navigation, &key) == nil else { return }
        objc_setAssociatedObject(navigation, &key, SidebarEdgeGesture(navigation: navigation), .OBJC_ASSOCIATION_RETAIN_NONATOMIC)
    }

    private var direction: CGFloat {
        navigation?.view.effectiveUserInterfaceLayoutDirection == .rightToLeft ? -1 : 1
    }

    @objc private func pan(_ gesture: UIPanGestureRecognizer) {
        let model = SidebarModel.shared
        let translation = gesture.translation(in: gesture.view).x * direction
        switch gesture.state {
        case .began: model.beginDrag(at: 0); model.drag(translation: translation)
        case .changed: model.drag(translation: translation)
        case .ended, .cancelled, .failed: model.endDrag(velocity: gesture.velocity(in: gesture.view).x * direction)
        default: break
        }
    }

    func gestureRecognizer(_ gestureRecognizer: UIGestureRecognizer, shouldReceive touch: UITouch) -> Bool {
        // Only on the root screen, and never while a push or pop is still
        // settling: just after a pop commits the stack already has one screen.
        guard let navigation, !SidebarModel.shared.isVisible,
              EdgeGestureRules.mayOpenSidebar(stackDepth: navigation.viewControllers.count,
                                              inTransition: navigation.transitionCoordinator != nil,
                                              presenting: navigation.presentedViewController != nil),
              !EdgeGestureRules.isTextSelectionHandle(touch.view) else { return false }
        let view: UIView = navigation.view
        return EdgeGestureRules.startsAtLeadingEdge(x: touch.location(in: view).x, width: view.bounds.width, rightToLeft: direction < 0)
    }

    /// Only a sideways drag into the screen; a vertical one is left to the list.
    func gestureRecognizerShouldBegin(_ gestureRecognizer: UIGestureRecognizer) -> Bool {
        guard let pan = gestureRecognizer as? UIPanGestureRecognizer else { return false }
        let velocity = pan.velocity(in: pan.view)
        return EdgeGestureRules.isInwardHorizontal(dx: velocity.x * direction, dy: velocity.y)
    }

    /// For a touch that began at the edge, the list waits to see whether this
    /// is a sidebar drag before it starts scrolling.
    func gestureRecognizer(_ gestureRecognizer: UIGestureRecognizer, shouldBeRequiredToFailBy other: UIGestureRecognizer) -> Bool {
        other.view is UIScrollView
    }
}
