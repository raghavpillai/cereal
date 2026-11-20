import SwiftUI

// The app's one loader and its quiet sibling. They lived with the chat chrome;
// every screen waits with them, so they are part of the design system.
//
// - `CerealLoader`: **anything that waits** (a button at work, a row, a bar).
//   A system spinner (`ProgressView`) is never used. Beside its reason use
//   `InlineProgress`; for a whole region use `Loading`.
// - `ThinkingIndicator`: the mark turning beside words that say what is being
//   done ("Thinking", a step of a recording being written up).

/// The Cereal mark beside "Thinking", moving as on the desktop: one turn a loop,
/// swung rather than ticked, breathing smaller and paler twice a turn. Still
/// with Reduce Motion.
struct ThinkingIndicator: View {
    var size: CGFloat = 13
    @Environment(\.motion) private var motion
    private static let loop: Double = 2.4

    var body: some View {
        Group {
            if motion.reduceMotion {
                mark
            } else {
                TimelineView(.animation) { context in
                    let pose = Self.pose(context.date.timeIntervalSinceReferenceDate.truncatingRemainder(dividingBy: Self.loop) / Self.loop)
                    mark.rotationEffect(.degrees(pose.turn * 360)).scaleEffect(pose.scale).opacity(pose.ink)
                }
            }
        }
        .frame(width: size, height: size)
        .accessibilityHidden(true)
    }

    private var mark: some View { CerealMarkShape().fill(style: FillStyle(eoFill: true)) }

    /// The desktop's `thinking_mark_pose`: turn (0...1), scale and ink at `progress` through the loop.
    static func pose(_ progress: Double) -> (turn: Double, scale: Double, ink: Double) {
        let p = min(max(progress, 0), 1)
        let breath = 0.5 - 0.5 * cos(2 * 2 * .pi * p)
        let turn = min(max(p - 0.55 * sin(2 * .pi * p) / (2 * .pi), 0), 1)
        return (turn, 1 - 0.12 * breath, 1 - 0.35 * breath)
    }
}

/// The Cereal mark as a loader, the desktop's `cereal_loader`: the bowl spins
/// once on a loose spring, squeezing as it goes; the flakes are flung round
/// after it, each a beat later, then hop outward in turn. Still with Reduce Motion.
struct CerealLoader: View {
    var size: CGFloat = 20
    var color: Color = Theme.accent
    /// False holds the mark at rest and draws no frames: for a loader that keeps
    /// its place in a row while there is nothing to wait for.
    var running = true
    @Environment(\.motion) private var motion
    /// Each wait starts its loop from the beginning, with the spin.
    @State private var started = Date.now
    /// Off screen (under a pushed screen, or gone) nothing is drawn.
    @State private var visible = false
    static let loop = 2.2
    private static let spin = 0.42, lag = 0.055
    /// The middle of the bowl's ring (not of its outline's bounds, which the bite cuts short),
    /// its outer radius, and the least clear page kept between it and a flake: as on the desktop.
    private static let bowl = CGPoint(x: 10.9925, y: 12.2575)
    private static let ring = 10.2925, gap = 0.55
    static let flakes: [(center: CGPoint, radius: Double)] = [(CGPoint(x: 21.835, y: 2.149), 0.7), (CGPoint(x: 22.147, y: 6.575), 1.152)]
    /// The flakes swing and hop outside the mark's 24-unit box, by up to this
    /// much on each side. They are drawn in a canvas that much larger, which
    /// takes no more room in the layout than the mark does.
    static let margin = 4.0

    struct Pose {
        var turn: Double, scale: Double
        var flakes: [(center: CGPoint, scale: Double)]
    }

    var body: some View {
        Group {
            if motion.reduceMotion {
                CerealMarkShape().fill(color, style: FillStyle(eoFill: true))
            } else {
                let moving = running && visible
                TimelineView(.animation(paused: !moving)) { context in
                    let elapsed = max(context.date.timeIntervalSince(started), 0)
                    let pose = Self.pose(moving ? elapsed.truncatingRemainder(dividingBy: Self.loop) / Self.loop : 0)
                    Canvas { canvas, _ in
                        let unit = size / 24
                        canvas.translateBy(x: Self.margin * unit, y: Self.margin * unit)
                        var bowl = canvas
                        bowl.translateBy(x: Self.bowl.x * unit, y: Self.bowl.y * unit)
                        bowl.rotate(by: .radians(2 * .pi * pose.turn))
                        bowl.scaleBy(x: pose.scale, y: pose.scale)
                        bowl.translateBy(x: -Self.bowl.x * unit, y: -Self.bowl.y * unit)
                        bowl.fill(CerealMarkShape(bowlOnly: true).path(in: CGRect(x: 0, y: 0, width: size, height: size)), with: .color(color), style: FillStyle(eoFill: true))
                        for (index, flake) in pose.flakes.enumerated() {
                            let radius = Self.flakes[index].radius * flake.scale * unit
                            canvas.fill(Path(ellipseIn: CGRect(x: flake.center.x * unit - radius, y: flake.center.y * unit - radius, width: 2 * radius, height: 2 * radius)), with: .color(color))
                        }
                    }
                    .frame(width: size * (24 + 2 * Self.margin) / 24, height: size * (24 + 2 * Self.margin) / 24)
                }
                .onAppear { started = .now; visible = true }
                .onDisappear { visible = false }
                .onChange(of: running) { _, now in if now { started = .now } }
            }
        }
        .frame(width: size, height: size)
        .accessibilityHidden(true)
    }

    /// 0 → 1 → 0 across 0...1, flat at both ends; 0 outside it.
    private static func bump(_ t: Double) -> Double { (0...1).contains(t) ? pow(sin(.pi * t), 2) : 0 }

    /// The desktop's loader spring (damping 0.56): a unit step that swings past 1 and settles on it.
    private static func spring(_ t: Double) -> Double {
        guard t > 0 else { return 0 }
        guard t < 1 else { return 1 }
        let zeta = 0.56, omega = 6.9 / zeta, damped = omega * (1 - zeta * zeta).squareRoot()
        func raw(_ t: Double) -> Double { 1 - exp(-zeta * omega * t) * (cos(damped * t) + zeta * omega / damped * sin(damped * t)) }
        return raw(t) / raw(1)
    }

    /// The desktop's `cereal_loader_pose`, in the mark's 24-unit box.
    static func pose(_ progress: Double) -> Pose {
        let p = min(max(progress, 0), 1)
        let flakes = Self.flakes.enumerated().map { index, flake -> (center: CGPoint, scale: Double) in
            let after = (p - lag * Double(index + 1)) / spin
            let hop = bump((p - 0.66 - 0.11 * Double(index)) / 0.2)
            let turn = spring(after), angle = 2 * .pi * turn
            // Pulled in most on the far side of the bowl, wherever in time that is.
            let pull = bump(min(max(turn, 0), 1))
            let grow = 1 - 0.3 * pull + 0.35 * hop
            // But never onto the bowl: a flake that touches it reads as a lump on its rim.
            let away = hypot(flake.center.x - bowl.x, flake.center.y - bowl.y)
            let clear = (ring + gap + flake.radius * grow) / away
            let reach = max(1 - 0.3 * pull, min(clear, 1)) + 0.13 * hop
            let dx = (flake.center.x - bowl.x) * reach, dy = (flake.center.y - bowl.y) * reach
            return (CGPoint(x: bowl.x + dx * cos(angle) - dy * sin(angle), y: bowl.y + dx * sin(angle) + dy * cos(angle)), grow)
        }
        return Pose(turn: spring(p / spin), scale: 1 - 0.08 * bump(p / spin), flakes: flakes)
    }
}

/// **Words that say what is under way** ("Thinking", a lookup in progress,
/// "Generating notes..."): a band of the theme's ink passes over them once every
/// `MotionResolver.sheenPeriod`. Still under Reduce Motion. It takes its font and
/// colour from where it is used.
struct SheenLabel: View {
    let text: String
    @State private var phase: CGFloat = -1
    @Environment(\.motion) private var motion

    var body: some View {
        Text(text)
            .overlay {
                GeometryReader { geometry in
                    // The theme's ink: a bright sheen over grey on the dark page, a dark one on the light page.
                    LinearGradient(colors: [.clear, Theme.textPrimary.opacity(0.55), .clear], startPoint: .leading, endPoint: .trailing)
                        .frame(width: 50, height: geometry.size.height)
                        .offset(x: phase * (geometry.size.width + 50))
                }
                .mask(Text(text))
                .allowsHitTesting(false)
            }
            .onAppear {
                if !motion.reduceMotion { withAnimation(.linear(duration: MotionResolver.sheenPeriod).repeatForever(autoreverses: false)) { phase = 1 } }
            }
    }
}
