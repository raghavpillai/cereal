import SwiftUI

// MARK: - Tokens

/// The three named springs every surface uses. The desktop app defines the
/// same names and values so both products move as one.
enum MotionToken: String, CaseIterable, Sendable {
    /// Small controls, presses and icon swaps.
    case snappy
    /// Glass morphs, control clusters and moving highlights.
    case gooey
    /// Sheets, large panels and navigation-level changes.
    case settle

    var duration: Double {
        switch self {
        case .snappy: 0.28
        case .gooey: 0.42
        case .settle: 0.55
        }
    }

    var bounce: Double {
        switch self {
        case .snappy: 0.10
        case .gooey: 0.28
        case .settle: 0.15
        }
    }
}

enum MotionPhase: Sendable { case enter, exit }

/// A resolved, comparable description of an animation. Views use `animation`;
/// tests assert on the numbers.
struct MotionSpec: Equatable, Sendable {
    enum Curve: Equatable, Sendable { case spring, easeOut }
    var curve: Curve
    var duration: Double
    var bounce: Double

    var animation: Animation {
        switch curve {
        case .spring: .spring(duration: duration, bounce: bounce)
        case .easeOut: .easeOut(duration: duration)
        }
    }
}

/// Pure policy: exits are quicker than entrances and never bounce, and Reduce
/// Motion replaces every spring with one short opacity-friendly ease.
enum MotionResolver {
    static let exitDurationScale = 0.65
    static let reducedDuration = 0.16
    static let staggerStep = 0.035
    static let staggerCap = 7
    static let pressedScale: CGFloat = 0.96
    /// Whatever leaves to make room for something else fades this quickly, so
    /// what arrives in its place is never drawn over text that can still be read.
    static let leaveDuration = 0.09
    /// Sideways travel of content that changes in a direction (the day pager).
    static let stepTravel: CGFloat = 28
    /// Half a breath of a placeholder: full strength to faint.
    static let breathDuration = 0.9
    static let breathFaint = 0.45
    /// One pass of the sheen over words that say what is under way (`SheenLabel`).
    /// A sheen moves at one speed, so this is the one linear animation in the app.
    static let sheenPeriod = 1.2

    static func spec(_ token: MotionToken, phase: MotionPhase = .enter, reduceMotion: Bool) -> MotionSpec {
        if reduceMotion { return MotionSpec(curve: .easeOut, duration: reducedDuration, bounce: 0) }
        switch phase {
        case .enter: return MotionSpec(curve: .spring, duration: token.duration, bounce: token.bounce)
        case .exit: return MotionSpec(curve: .spring, duration: token.duration * exitDurationScale, bounce: 0)
        }
    }

    /// Entrance delay for the row at `index`. Capped so a long list never
    /// keeps its tail waiting, and absent entirely under Reduce Motion.
    static func staggerDelay(index: Int, reduceMotion: Bool) -> Double {
        guard !reduceMotion else { return 0 }
        return Double(min(max(index, 0), staggerCap)) * staggerStep
    }

    static func pressScale(reduceMotion: Bool, scale: CGFloat = pressedScale) -> CGFloat {
        reduceMotion ? 1 : scale
    }

    /// Reduce Motion keeps a pressed state, expressed as a dim instead of a squash.
    static func pressOpacity(reduceMotion: Bool) -> Double {
        reduceMotion ? 0.7 : 1
    }
}

// MARK: - Environment

/// Read with `@Environment(\.motion)`. Every animation and transition in the
/// app comes from here, so the Reduce Motion policy is applied in one place.
struct Motion: Equatable, Sendable {
    let reduceMotion: Bool

    var snappy: Animation { animation(.snappy) }
    var gooey: Animation { animation(.gooey) }
    var settle: Animation { animation(.settle) }

    func animation(_ token: MotionToken, _ phase: MotionPhase = .enter) -> Animation {
        MotionResolver.spec(token, phase: phase, reduceMotion: reduceMotion).animation
    }

    func exit(_ token: MotionToken) -> Animation { animation(token, .exit) }

    /// For a Boolean-like change: spring in, leave quickly without bounce.
    func animation(_ token: MotionToken, entering: Bool) -> Animation {
        animation(token, entering ? .enter : .exit)
    }

    /// Level meters follow the microphone at about 12 Hz, so they need a much
    /// shorter spring than the UI tokens. Nil holds the bars still.
    var waveform: Animation? {
        reduceMotion ? nil : .spring(duration: 0.2, bounce: 0.38)
    }

    func stagger(_ index: Int) -> Double {
        MotionResolver.staggerDelay(index: index, reduceMotion: reduceMotion)
    }

    // MARK: Transitions

    /// Small floating chrome that buds into place and leaves without a bounce.
    var pop: AnyTransition { pop(anchor: .center) }

    func pop(anchor: UnitPoint) -> AnyTransition {
        guard !reduceMotion else { return fade }
        return .asymmetric(
            insertion: .scale(scale: 0.6, anchor: anchor).combined(with: .opacity).animation(gooey),
            removal: .scale(scale: 0.85, anchor: anchor).combined(with: .opacity).animation(exit(.gooey))
        )
    }

    /// The quick fade of anything that leaves to make room (see `MotionResolver.leaveDuration`).
    var leave: Animation { .easeOut(duration: MotionResolver.leaveDuration) }

    /// **A row, a line or a message joining or leaving a stack** (a note in a
    /// list, an error under a field, a notice, a chat message). It rises into
    /// the space that opens for it and fades at once when it goes, so the rows
    /// closing up never cross it. The space opens and closes only if the stack
    /// that holds it animates the same change: put `.animates(ids)` on that
    /// stack, or make the change inside `withAnimation(motion.snappy)`.
    /// A `List` moves its own rows and needs neither.
    var rise: AnyTransition { rise(delay: 0) }

    func rise(delay: Double) -> AnyTransition {
        guard !reduceMotion else { return fade }
        return .asymmetric(
            insertion: .offset(y: 10).combined(with: .opacity).animation(settle.delay(delay)),
            removal: .opacity.animation(leave)
        )
    }

    /// **A block that opens between lines of text** (an error above a document,
    /// the thoughts above an answer, the meetings under a lookup, a progress
    /// row above notes). Where what lies under the new block is text, `rise`
    /// is not enough: the block is drawn at its full height from the first
    /// frame, across lines that are still moving out of its way. This one is
    /// cut to the room as the room opens, and to the room as it closes, so the
    /// two are never drawn over each other.
    ///
    /// It carries no animation of its own: it opens at the pace of whatever
    /// moves its neighbours. Put `.animates(value)` on the stack that holds it,
    /// or make the change inside `withAnimation(motion.snappy)`.
    var unfold: AnyTransition { AnyTransition(UnfoldTransition()) }

    /// **Content taking the place of other content in the same spot** (loading
    /// turning into loaded, one answer into the next, one prompt into another).
    /// The old is gone before the new can be read: it fades in
    /// `leaveDuration`, and only then does the new arrive. They are never
    /// cross-faded over each other and never slide through each other.
    /// `from` is the side the new content comes from when the change has a
    /// direction; the old leaves by the other side.
    ///
    /// Give the two states different identities (an `if`/`switch`, or `.id`)
    /// inside one container, and let that container animate: `Replaced` does
    /// all of it and is the usual way to use this.
    func replace(from edge: HorizontalEdge? = nil) -> AnyTransition {
        let pause = MotionResolver.leaveDuration
        guard !reduceMotion else {
            return .asymmetric(insertion: .opacity.animation(MotionResolver.spec(.snappy, reduceMotion: true).animation.delay(pause)),
                               removal: .opacity.animation(leave))
        }
        guard let edge else {
            return .asymmetric(insertion: .offset(y: 6).combined(with: .opacity).animation(snappy.delay(pause)),
                               removal: .opacity.animation(leave))
        }
        let travel = edge == .trailing ? MotionResolver.stepTravel : -MotionResolver.stepTravel
        return .asymmetric(
            insertion: .offset(x: travel).combined(with: .opacity).animation(snappy.delay(pause)),
            removal: .offset(x: -travel).combined(with: .opacity).animation(leave)
        )
    }

    /// Content that changes in a direction (the next or previous day): `replace`
    /// from the side being stepped towards.
    func step(forward: Bool) -> AnyTransition { replace(from: forward ? .trailing : .leading) }

    /// Two small glyphs or labels trading places inside one control.
    var swap: AnyTransition {
        guard !reduceMotion else { return fade }
        return AnyTransition(.blurReplace).animation(snappy)
    }

    var fade: AnyTransition {
        .opacity.animation(MotionResolver.spec(.snappy, reduceMotion: true).animation)
    }

    /// A line that gives its place to a neighbour (the capture hint, when the
    /// keyboard comes up and the status line drops into its position). It
    /// leaves at once and returns after the neighbour has moved away, so the
    /// two are never drawn over each other.
    var giveWay: AnyTransition {
        .asymmetric(
            insertion: .opacity.animation(MotionResolver.spec(.snappy, reduceMotion: true).animation.delay(reduceMotion ? 0.12 : 0.2)),
            removal: .identity
        )
    }

    // MARK: Content transitions

    var numericText: ContentTransition { reduceMotion ? .identity : .numericText() }

    var symbolReplace: ContentTransition { reduceMotion ? .identity : .symbolEffect(.replace) }
}

extension EnvironmentValues {
    var motion: Motion { Motion(reduceMotion: accessibilityReduceMotion) }
}

// MARK: - Press

/// The shared touch response: a slight squash that springs back. Glass
/// controls keep the system's interactive highlight and add this to their label.
struct SquashButtonStyle: ButtonStyle {
    var scale: CGFloat = MotionResolver.pressedScale
    @Environment(\.motion) private var motion
    @Environment(\.isEnabled) private var isEnabled

    func makeBody(configuration: Configuration) -> some View {
        let pressed = configuration.isPressed && isEnabled
        // The spring is given to the squash and the dim alone. As an animation of the whole label it
        // also took over where the label is: a button whose tap moves it (the chat's Send drops the
        // keyboard under its own bar) was carried there by its release spring, behind the bar.
        configuration.label
            .animation(motion.animation(.snappy, entering: !pressed)) { label in
                label
                    .scaleEffect(pressed ? MotionResolver.pressScale(reduceMotion: motion.reduceMotion, scale: scale) : 1)
                    .opacity(pressed ? MotionResolver.pressOpacity(reduceMotion: motion.reduceMotion) : 1)
            }
    }
}

/// List rows soften under the finger instead of flashing. The row a screen
/// was opened from keeps its highlight until the user is back on the list.
///
/// Use `RowButton` for a row in a scrolling stack: it adds what a style alone
/// cannot (a sideways drag is not a tap). Use this style directly only for a
/// row inside a `List`, which cancels presses for its own swipes.
struct RowPressStyle: ButtonStyle {
    var isOrigin = false
    /// The touch turned into a drag: the wash goes although the finger is still down.
    var cancelled = false
    @Environment(\.motion) private var motion

    func makeBody(configuration: Configuration) -> some View {
        let pressed = configuration.isPressed && !cancelled
        let lit = pressed || isOrigin
        configuration.label
            .background {
                RoundedRectangle(cornerRadius: 14, style: .continuous)
                    .fill(Theme.ink.opacity(lit ? 0.06 : 0))
                    .padding(.horizontal, -8)
            }
            .scaleEffect(pressed ? MotionResolver.pressScale(reduceMotion: motion.reduceMotion, scale: 0.98) : 1)
            .animation(motion.animation(.snappy, entering: !pressed), value: pressed)
            .animation(motion.animation(.settle, entering: isOrigin), value: isOrigin)
    }
}

/// **A full-width row that does something when tapped** (a note, an event, a
/// search result, a sidebar item, a settings row): the rounded wash, the 0.98
/// squash, and the highlight that stays while the screen it opened is up.
///
/// A scroll view only takes vertical drags, so a plain `Button` in one treats a
/// sideways swipe that ends inside it as a tap. This row gives the press up as
/// soon as the finger has travelled `strayDistance`: the wash clears and
/// lifting does nothing. That covers a swipe across a list and a drag that
/// closes the sidebar from one of its rows.
///
/// `haptic` is for a row that commits something by itself. A row that only
/// opens a screen stays silent.
struct RowButton<Label: View>: View {
    var isOrigin = false
    var haptic: HapticFeedback? = nil
    let action: () -> Void
    @ViewBuilder var label: Label
    @GestureState private var dragging = false
    @State private var strayed = false
    @State private var activations = 0
    @State private var release: Task<Void, Never>?

    /// Further than a finger wanders during a tap, less than any deliberate swipe.
    static var strayDistance: CGFloat { 10 }
    /// Whether a touch that moved by `translation` is still a tap.
    static func isTap(translation: CGSize) -> Bool {
        hypot(translation.width, translation.height) < strayDistance
    }

    var body: some View {
        Button {
            guard !strayed else { return }
            activations &+= 1
            action()
        } label: { label }
            .buttonStyle(RowPressStyle(isOrigin: isOrigin, cancelled: strayed))
            .simultaneousGesture(DragGesture(minimumDistance: Self.strayDistance).updating($dragging) { _, state, _ in state = true })
            // Gesture state also resets when the system takes the touch away, where `onEnded` never runs.
            .onChange(of: dragging) { _, now in
                release?.cancel()
                if now { strayed = true; return }
                // The button's own action can arrive just after the drag ends; it is still that drag.
                release = Task {
                    try? await Task.sleep(for: .milliseconds(120))
                    if !Task.isCancelled { strayed = false }
                }
            }
            .haptic(trigger: activations) { _, _ in haptic }
    }
}

/// For a control whose visible body is drawn by another view (for example a
/// transparent hit target over a menu label): reports touch-down so that
/// body can squash itself.
struct PressReportingStyle: ButtonStyle {
    @Binding var pressed: Bool

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .onChange(of: configuration.isPressed) { _, isPressed in pressed = isPressed }
    }
}

extension ButtonStyle where Self == SquashButtonStyle {
    /// **Any button that is not a list row**: it squashes a little under the finger and springs back.
    static var squash: SquashButtonStyle { SquashButtonStyle() }
}

extension ButtonStyle where Self == RowPressStyle {
    /// A row inside a `List`. Anywhere else use `RowButton`.
    static var row: RowPressStyle { RowPressStyle() }
}

// MARK: - Glass morphing

/// Identity for one glass surface inside a `GlassChromeGroup`.
struct GlassMorphTarget: Equatable {
    let id: String
    let namespace: Namespace.ID
}

extension EnvironmentValues {
    @Entry var glassMorphTarget: GlassMorphTarget?
}

extension View {
    /// Names the glass surface of a control inside a `GlassChromeGroup` so it
    /// splits from and merges back into its neighbours instead of cross-fading.
    /// Apply to a control that owns exactly one `glass`/`glassSurface`.
    func glassMorph(_ id: String, in namespace: Namespace.ID) -> some View {
        environment(\.glassMorphTarget, GlassMorphTarget(id: id, namespace: namespace))
    }
}

// MARK: - Staggered entrance

/// One per list. Rows that appear during the first moments of the list rise in
/// sequence; anything revealed later by scrolling or a refresh is simply there.
@MainActor final class EntranceGate {
    private var isOpen: Bool
    private var closing: Task<Void, Never>?

    init(open: Bool = true) { isOpen = open }

    /// The list starts over (a first account has just signed in): its rows rise in again.
    func reopen() {
        closing?.cancel(); closing = nil
        isOpen = true
    }

    func admit() -> Bool {
        guard isOpen else { return false }
        if closing == nil {
            closing = Task { [weak self] in
                try? await Task.sleep(for: .milliseconds(350))
                self?.isOpen = false
            }
        }
        return true
    }
}

private struct StaggeredEntrance: ViewModifier {
    let index: Int
    let gate: EntranceGate
    @Environment(\.motion) private var motion
    @State private var shown = false

    func body(content: Content) -> some View {
        content
            .opacity(shown ? 1 : 0)
            .offset(y: shown ? 0 : 12)
            .onAppear {
                guard !shown else { return }
                if gate.admit(), !motion.reduceMotion {
                    withAnimation(motion.settle.delay(motion.stagger(index))) { shown = true }
                } else {
                    shown = true
                }
            }
    }
}

extension View {
    func staggeredEntrance(_ index: Int, gate: EntranceGate) -> some View {
        modifier(StaggeredEntrance(index: index, gate: gate))
    }
}

// MARK: - Animating a change

private struct TokenAnimation<Value: Equatable>: ViewModifier {
    let value: Value
    let token: MotionToken
    @Environment(\.motion) private var motion
    func body(content: Content) -> some View { content.animation(motion.animation(token), value: value) }
}

extension View {
    /// **The one way to animate a container when a value changes**: its rows
    /// joining and leaving, its lines appearing, its size. It takes a token,
    /// never a duration, and Reduce Motion is applied here.
    ///
    /// It goes on the stack that holds what changes, not on the thing that
    /// changes: a row that fades while its neighbours jump has it one level too low.
    func animates<Value: Equatable>(_ value: Value, _ token: MotionToken = .snappy) -> some View {
        modifier(TokenAnimation(value: value, token: token))
    }
}

/// `Motion.unfold`. The content keeps its own height throughout (text is never
/// laid out again as the room grows); only the window onto it changes.
private struct UnfoldTransition: Transition {
    func body(content: Content, phase: TransitionPhase) -> some View {
        content
            .fixedSize(horizontal: false, vertical: true)
            .frame(height: phase.isIdentity ? nil : 0, alignment: .top)
            .clipped()
            .opacity(phase.isIdentity ? 1 : 0)
    }
}

private struct Closing: ViewModifier {
    let shown: Bool
    @Environment(\.motion) private var motion

    func body(content: Content) -> some View {
        content
            .opacity(shown ? 1 : 0)
            // Gone before the room has closed far; back at the room's own pace.
            .animation(shown ? motion.snappy : motion.leave, value: shown)
            .fixedSize(horizontal: false, vertical: true)
            .frame(height: shown ? nil : 0, alignment: .top)
            .clipped()
            .animation(motion.snappy, value: shown)
            .allowsHitTesting(shown)
            .accessibilityHidden(!shown)
    }
}

extension View {
    /// **A row of a bar that itself moves** (the recipe chips on the chat
    /// composer, which rides the keyboard; a line of a floating bar). Such a row
    /// must not leave by a transition: a view that is leaving is drawn where it
    /// was, and the bar travels on without it. It stays in the bar instead and
    /// closes: it fades in `leaveDuration` while its room closes with `snappy`.
    ///
    /// Keep drawing the row's last content while `shown` is false (hold it in
    /// `@State`), so it has something to fade.
    func closes(unless shown: Bool) -> some View { modifier(Closing(shown: shown)) }
}

/// **Content that replaces other content in the same place**, as one view:
/// whatever `content` draws for one `id` leaves (`Motion.replace`) before what
/// it draws for the next arrives. `from` gives the change a direction.
///
/// Use it where a value selects what a region shows: a day's events, a
/// prompt, a result. Size the region so that both states fit (a fixed or
/// minimum frame), or its neighbours move when the taller one goes.
struct Replaced<ID: Hashable, Content: View>: View {
    let id: ID
    var from: HorizontalEdge? = nil
    var alignment: Alignment = .topLeading
    @ViewBuilder var content: Content
    @Environment(\.motion) private var motion

    var body: some View {
        ZStack(alignment: alignment) {
            content.id(id).transition(motion.replace(from: from))
        }
        .animation(motion.snappy, value: id)
    }
}

// MARK: - Waiting for an answer

/// What a region shows while an answer is on its way, decided in one place.
///
/// - For the first `grace` nothing is drawn: most answers arrive within it,
///   and a placeholder that lasts 150 ms reads as a flicker.
/// - After that the placeholder appears and stays at least `minimumShown`.
/// - The content, which includes any "nothing here" or "sign in" message, is
///   not built until the answer has arrived. A region can therefore never say
///   "empty" about something it has not been told yet.
enum LoadingPolicy {
    static let grace: Duration = .milliseconds(300)
    static let minimumShown: Duration = .milliseconds(350)

    enum Stage: Equatable, Sendable {
        /// Waiting, within the grace: the placeholder's room is kept, nothing is drawn.
        case quiet
        case placeholder
        case content
    }

    static func initial(isLoading: Bool) -> Stage { isLoading ? .quiet : .content }

    /// How much longer a placeholder that has been up for `shown` must stay once the answer is in.
    static func hold(afterShownFor shown: Duration) -> Duration {
        shown >= minimumShown ? .zero : minimumShown - shown
    }
}

/// **The one way a region waits for an answer.** See `LoadingPolicy`.
///
/// `while` is true only while there is nothing to show yet. A refresh of
/// something already on screen is not loading: keep showing it.
///
///     Loading(while: list.isFirstLoad) { PlaceholderRows() } then: {
///         if rows.isEmpty { ScreenPrompt(…) } else { Rows(rows) }
///     }
///
/// The placeholder breathes (`breathes()`), holds the room the content will
/// take, and gives way with `Motion.replace`.
struct Loading<Placeholder: View, Content: View>: View {
    private let isLoading: Bool
    private let alignment: Alignment
    private let placeholder: Placeholder
    private let content: () -> Content
    @State private var stage: LoadingPolicy.Stage
    @State private var shownAt: ContinuousClock.Instant?
    @Environment(\.motion) private var motion

    init(while isLoading: Bool, alignment: Alignment = .topLeading,
         @ViewBuilder placeholder: () -> Placeholder, @ViewBuilder then content: @escaping () -> Content) {
        self.isLoading = isLoading
        self.alignment = alignment
        self.placeholder = placeholder()
        self.content = content
        _stage = State(initialValue: LoadingPolicy.initial(isLoading: isLoading))
    }

    var body: some View {
        ZStack(alignment: alignment) {
            switch stage {
            case .quiet:
                placeholder.hidden().accessibilityHidden(true)
            case .placeholder:
                placeholder.breathes().transition(motion.replace())
            case .content:
                content().transition(motion.replace())
            }
        }
        .task(id: isLoading) {
            if isLoading {
                if stage == .content { stage = .quiet }
                guard stage == .quiet else { return }
                try? await Task.sleep(for: LoadingPolicy.grace)
                guard !Task.isCancelled else { return }
                shownAt = .now
                withAnimation(motion.snappy) { stage = .placeholder }
            } else {
                guard stage != .content else { return }
                if stage == .placeholder, let shownAt {
                    try? await Task.sleep(for: LoadingPolicy.hold(afterShownFor: ContinuousClock.now - shownAt))
                    guard !Task.isCancelled else { return }
                }
                shownAt = nil
                // An answer within the grace simply appears: there was nothing to replace.
                withAnimation(motion.snappy) { stage = .content }
            }
        }
    }
}

private struct Breathing: ViewModifier {
    @Environment(\.motion) private var motion
    @State private var faint = false

    func body(content: Content) -> some View {
        content
            .opacity(faint ? MotionResolver.breathFaint : 1)
            .animation(motion.reduceMotion ? nil : .easeInOut(duration: MotionResolver.breathDuration).repeatForever(autoreverses: true), value: faint)
            .onAppear { if !motion.reduceMotion { faint = true } }
    }
}

extension View {
    /// **A placeholder that stands for content on its way** breathes, so it
    /// reads as waiting and not as an empty thing. Still under Reduce Motion.
    /// `Loading` applies it to its placeholder; apply it yourself only to a
    /// placeholder drawn some other way.
    func breathes() -> some View { modifier(Breathing()) }
}

// MARK: - Zoom navigation

extension EnvironmentValues {
    /// Shared by the rows and controls that open a screen and the screen itself.
    @Entry var zoomNamespace: Namespace.ID?
}

private struct ZoomSource<ID: Hashable>: ViewModifier {
    let id: ID
    var cornerRadius: CGFloat
    var enabled = true
    @Environment(\.zoomNamespace) private var namespace

    @ViewBuilder func body(content: Content) -> some View {
        if let namespace, enabled {
            content.matchedTransitionSource(id: id, in: namespace) {
                $0.clipShape(RoundedRectangle(cornerRadius: cornerRadius, style: .continuous))
            }
        } else {
            content
        }
    }
}

private struct ZoomDestination<ID: Hashable>: ViewModifier {
    let id: ID
    let enabled: Bool
    @Environment(\.zoomNamespace) private var namespace
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    /// Decided once. Flipping the setting while a screen is open must not
    /// rebuild that screen and drop its state.
    @State private var decided: Bool?

    @ViewBuilder func body(content: Content) -> some View {
        let zooms = decided ?? (enabled && !reduceMotion)
        Group {
            if zooms, let namespace {
                content.navigationTransition(.zoom(sourceID: id, in: namespace))
            } else {
                content
            }
        }
        .onAppear { if decided == nil { decided = zooms } }
    }
}

extension View {
    /// The row or control a screen grows out of.
    /// Only one view may be the source of an ID at a time; `enabled` hands it over.
    func zoomSource(_ id: some Hashable, cornerRadius: CGFloat = 16, enabled: Bool = true) -> some View {
        modifier(ZoomSource(id: id, cornerRadius: cornerRadius, enabled: enabled))
    }

    /// The screen that grows out of `zoomSource(id)`. Falls back to the system
    /// transition under Reduce Motion.
    func zoomDestination(_ id: some Hashable, enabled: Bool = true) -> some View {
        modifier(ZoomDestination(id: id, enabled: enabled))
    }
}
