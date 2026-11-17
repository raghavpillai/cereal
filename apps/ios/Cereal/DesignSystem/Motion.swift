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

    /// Content that settles upward into a list or conversation.
    var rise: AnyTransition { rise(delay: 0) }

    func rise(delay: Double) -> AnyTransition {
        guard !reduceMotion else { return fade }
        return .asymmetric(
            insertion: .offset(y: 10).combined(with: .opacity).animation(settle.delay(delay)),
            removal: .opacity.animation(exit(.snappy))
        )
    }

    /// Content that changes in a direction (the next or previous day): it
    /// arrives from that side over a short distance and the old content fades.
    func step(forward: Bool) -> AnyTransition {
        guard !reduceMotion else { return fade }
        return .asymmetric(
            insertion: .offset(x: forward ? 24 : -24).combined(with: .opacity).animation(snappy),
            removal: .opacity.animation(exit(.snappy))
        )
    }

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
        configuration.label
            .scaleEffect(pressed ? MotionResolver.pressScale(reduceMotion: motion.reduceMotion, scale: scale) : 1)
            .opacity(pressed ? MotionResolver.pressOpacity(reduceMotion: motion.reduceMotion) : 1)
            .animation(motion.animation(.snappy, entering: !pressed), value: pressed)
    }
}

/// List rows soften under the finger instead of flashing. The row a screen
/// was opened from keeps its highlight until the user is back on the list.
struct RowPressStyle: ButtonStyle {
    var isOrigin = false
    @Environment(\.motion) private var motion

    func makeBody(configuration: Configuration) -> some View {
        let pressed = configuration.isPressed
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
    static var squash: SquashButtonStyle { SquashButtonStyle() }
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
