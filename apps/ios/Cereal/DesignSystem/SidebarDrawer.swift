import SwiftUI
import APIClient

/// State of the notes sidebar. It is hidden at rest on every layout and is
/// pulled in from the leading edge of Home, or opened with the Folders button.
@MainActor @Observable final class SidebarModel {
    static let shared = SidebarModel()

    /// 0 is closed, 1 is fully open. Follows the finger while dragging.
    private(set) var progress: CGFloat = 0
    private(set) var isDragging = false
    @ObservationIgnored var width: CGFloat = 300
    @ObservationIgnored var openAnimation: Animation = .spring(duration: 0.55, bounce: 0.15)
    @ObservationIgnored var closeAnimation: Animation = .spring(duration: 0.36, bounce: 0)
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
        settle(open: Self.shouldOpen(progress: progress, velocity: velocity))
    }

    func open() { clearDrag(); settle(open: true) }
    func close() { clearDrag(); settle(open: false) }

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

/// The sidebar: Home, Account and workspaces, Shared with me, and My notes. This wrapper is
/// the only view that follows the drag; it moves the panel and dims the page.
/// The panel's contents are a separate view that is not rebuilt per touch.
struct SidebarDrawer: View {
    @Environment(\.motion) private var motion
    @Environment(\.layoutDirection) private var layoutDirection
    @GestureState private var closing = false
    private var model: SidebarModel { SidebarModel.shared }

    var body: some View {
        GeometryReader { proxy in
            let width = min(320, proxy.size.width * 0.82)
            let direction: CGFloat = layoutDirection == .rightToLeft ? -1 : 1
            ZStack(alignment: .leading) {
                if model.isVisible {
                    Color.black.opacity(0.42 * model.progress)
                        .ignoresSafeArea()
                        .contentShape(Rectangle())
                        .onTapGesture { model.close() }
                        .accessibilityHidden(true)
                    SidebarPanel()
                        .frame(width: width)
                        // 8 pt from the top, leading and bottom edges of the screen.
                        .padding(.leading, 8).padding(.top, 8).padding(.bottom, 8)
                        .ignoresSafeArea(.container, edges: .bottom)
                        .offset(x: -(width + 16) * (1 - model.progress) * direction)
                        .accessibilityAddTraits(.isModal)
                        .accessibilityAction(.escape) { model.close() }
                }
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
            .onChange(of: width, initial: true) { _, new in model.width = new + 16 }
        }
        .allowsHitTesting(model.isVisible)
        .onChange(of: motion, initial: true) { _, motion in
            model.openAnimation = motion.settle
            model.closeAnimation = motion.exit(.settle)
        }
    }
}

/// The panel's contents. Reads the notes and the path, never the drag progress.
private struct SidebarPanel: View {
    @Environment(NoteStore.self) private var store
    @Environment(Router.self) private var router
    @Environment(ServerNotesModel.self) private var server
    @Environment(ConnectionMode.self) private var connectionMode
    @AccessibilityFocusState private var focusedOnOpen: Bool
    private var model: SidebarModel { SidebarModel.shared }

    var body: some View {
        let _ = PerfProbe.hit("SidebarPanel.body")
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 2) {
                HStack {
                    Text("Cereal").font(.display(22)).foregroundStyle(Theme.textPrimary)
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
                row("Account and workspaces", symbol: "person.crop.circle", selected: false) { router.showConnection = true }
                row("Shared with me", symbol: "link", selected: selected(.sharedWithMe)) { router.path = [.sharedWithMe] }
                Text("My notes").font(.system(size: 13, weight: .medium)).foregroundStyle(Theme.textSecondary)
                    .accessibilityAddTraits(.isHeader)
                    .padding(.horizontal, 14).padding(.top, 18).padding(.bottom, 6)
                if connectionMode.isDemo {
                    ForEach(store.activeNotes) { note in
                        row(note.displayTitle, symbol: "doc.text", selected: selected(.note(note.id))) { router.path = [.note(note.id)] }
                    }
                } else {
                    ForEach(server.list.rows) { note in
                        row(note.title.isEmpty ? "Untitled note" : note.title, symbol: "doc.text", selected: selected(.serverNote(note.id))) {
                            router.path = [.serverNote(note.id)]
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
        .glassSurface(RoundedRectangle(cornerRadius: 28, style: .continuous), tint: .black.opacity(0.3))
        .onAppear { focusedOnOpen = true }
    }

    private func row(_ title: String, symbol: String, selected: Bool, action: @escaping () -> Void) -> some View {
        Button {
            guard !model.suppressesRowTaps else { return }
            action()
            model.close()
        } label: {
            HStack(spacing: 12) {
                Image(systemName: symbol).font(.system(size: 15, weight: .medium)).frame(width: 22)
                    .accessibilityHidden(true)
                Text(title).font(.system(size: 15, weight: selected ? .semibold : .regular)).lineLimit(2)
                    .multilineTextAlignment(.leading)
                Spacer(minLength: 0)
            }
            .foregroundStyle(selected ? Theme.accent : Theme.textPrimary.opacity(0.9))
            .padding(.horizontal, 12).padding(.vertical, 11)
            .frame(minHeight: 44)
            .background(Color.white.opacity(selected ? 0.07 : 0), in: RoundedRectangle(cornerRadius: 14, style: .continuous))
            .contentShape(Rectangle())
        }
        .buttonStyle(RowPressStyle())
        .accessibilityAddTraits(selected ? .isSelected : [])
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
