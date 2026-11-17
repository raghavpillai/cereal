import CoreHaptics
import SwiftUI
import UIKit

// MARK: - Vocabulary

/// Every haptic the app plays, by what it means rather than how it feels.
/// One feedback answers one intent; nothing here plays on scroll, on text
/// streaming in, or on any other change the person did not ask for.
///
/// SwiftUI's `.sensoryFeedback` is the usual way to play one (see `View.haptic`).
/// `.start`, `.stop`, `.increase`, `.decrease` and `.levelChange` only play on
/// watchOS or macOS, and `.alignment` and `.pathComplete` only for a pencil or a
/// trackpad, so on iPhone the vocabulary is built from impacts, selection and
/// the three notifications.
enum HapticFeedback: Equatable, Sendable {
    /// A choice among several: a day, a theme, a model, a detent.
    case selection
    case impact(Impact, intensity: Double = 1)
    case success, warning, error

    /// `UIImpactFeedbackGenerator.FeedbackStyle`, as SwiftUI names it.
    enum Impact: Equatable, Sendable { case light, medium, heavy, soft, solid, rigid }

    // The named moments, so screens agree on what each kind of thing feels like.

    /// A message sent, a tap that commits something small.
    static let tap = HapticFeedback.impact(.light, intensity: 0.8)
    /// Something settles into place: the sidebar snaps open or shut.
    static let snap = HapticFeedback.impact(.soft, intensity: 0.6)
    /// The confirmation of something that cannot be undone: delete, erase, sign out.
    static let destructive = HapticFeedback.impact(.rigid, intensity: 1)
    /// Something copied to the clipboard, a link made, a change saved.
    static let done = HapticFeedback.success

    var sensory: SensoryFeedback {
        switch self {
        case .selection: .selection
        case .success: .success
        case .warning: .warning
        case .error: .error
        case .impact(let impact, let intensity):
            switch impact {
            case .light: .impact(weight: .light, intensity: intensity)
            case .medium: .impact(weight: .medium, intensity: intensity)
            case .heavy: .impact(weight: .heavy, intensity: intensity)
            case .soft: .impact(flexibility: .soft, intensity: intensity)
            case .solid: .impact(flexibility: .solid, intensity: intensity)
            case .rigid: .impact(flexibility: .rigid, intensity: intensity)
            }
        }
    }
}

/// The few moments that have a feel of their own, drawn with Core Haptics.
/// Each falls back to a plain feedback where Core Haptics is unavailable.
enum SignatureHaptic: CaseIterable, Sendable {
    /// Recording begins: a soft rising breath that lands on a crisp tick.
    case recordingStarted
    /// Recording ends: a firm tap that settles with a low, soft one, like a notebook closing.
    case recordingEnded
    /// The notes are written: three light taps, rising.
    case notesReady

    var fallback: HapticFeedback {
        switch self {
        case .recordingStarted: .impact(.medium)
        case .recordingEnded: .impact(.rigid, intensity: 0.8)
        case .notesReady: .success
        }
    }

    /// The pattern as plain numbers, so its shape can be tested and the
    /// Core Haptics objects built from it in one place.
    var pattern: HapticPatternSpec {
        switch self {
        case .recordingStarted:
            HapticPatternSpec(events: [
                .continuous(at: 0, duration: 0.38, intensity: 0.55, sharpness: 0.12),
                .transient(at: 0.40, intensity: 0.95, sharpness: 0.8),
            ], intensityCurve: [(0, 0.15), (0.30, 1), (0.38, 0)])
        case .recordingEnded:
            HapticPatternSpec(events: [
                .transient(at: 0, intensity: 0.85, sharpness: 0.65),
                .transient(at: 0.13, intensity: 0.5, sharpness: 0.15),
            ])
        case .notesReady:
            HapticPatternSpec(events: [
                .transient(at: 0, intensity: 0.45, sharpness: 0.35),
                .transient(at: 0.09, intensity: 0.65, sharpness: 0.5),
                .transient(at: 0.20, intensity: 0.9, sharpness: 0.7),
            ])
        }
    }
}

/// A Core Haptics pattern in numbers: AHAP's events and one intensity curve.
struct HapticPatternSpec: Equatable, Sendable {
    struct Event: Equatable, Sendable {
        enum Kind: Equatable, Sendable { case transient, continuous }
        var kind: Kind
        var time: TimeInterval
        var duration: TimeInterval
        var intensity: Float
        var sharpness: Float

        static func transient(at time: TimeInterval, intensity: Float, sharpness: Float) -> Event {
            Event(kind: .transient, time: time, duration: 0, intensity: intensity, sharpness: sharpness)
        }
        static func continuous(at time: TimeInterval, duration: TimeInterval, intensity: Float, sharpness: Float) -> Event {
            Event(kind: .continuous, time: time, duration: duration, intensity: intensity, sharpness: sharpness)
        }
    }

    var events: [Event]
    /// Scales the intensity of every event: (time, value) control points.
    var intensityCurve: [Point] = []

    struct Point: Equatable, Sendable { var time: TimeInterval; var value: Float }

    init(events: [Event], intensityCurve: [(TimeInterval, Float)] = []) {
        self.events = events
        self.intensityCurve = intensityCurve.map { Point(time: $0.0, value: $0.1) }
    }

    /// When the last event has finished.
    var duration: TimeInterval { events.map { $0.time + $0.duration }.max() ?? 0 }

    func makePattern() throws -> CHHapticPattern {
        let events = events.map { event in
            CHHapticEvent(eventType: event.kind == .transient ? .hapticTransient : .hapticContinuous, parameters: [
                CHHapticEventParameter(parameterID: .hapticIntensity, value: event.intensity),
                CHHapticEventParameter(parameterID: .hapticSharpness, value: event.sharpness),
            ], relativeTime: event.time, duration: event.duration)
        }
        let curves = intensityCurve.isEmpty ? [] : [CHHapticParameterCurve(parameterID: .hapticIntensityControl,
            controlPoints: intensityCurve.map { CHHapticParameterCurve.ControlPoint(relativeTime: $0.time, value: $0.value) },
            relativeTime: 0)]
        return try CHHapticPattern(events: events, parameterCurves: curves)
    }
}

// MARK: - When haptics may play

/// Pure rules for whether a haptic plays at all.
enum HapticGate {
    static let storageKey = "haptics.enabled"

    /// The person's own switch (Settings › Appearance › Haptics), on unless turned off.
    static func isEnabled(_ defaults: UserDefaults = .standard) -> Bool {
        defaults.object(forKey: storageKey) as? Bool ?? true
    }

    /// A haptic is felt only in the hand, so only while the app is in front;
    /// never in a test run; never when the person has turned them off.
    static func allows(enabled: Bool, appActive: Bool, testing: Bool) -> Bool {
        enabled && appActive && !testing
    }

    /// Core Haptics only where the hardware has a Taptic Engine.
    static func usesCoreHaptics(supportsHaptics: Bool, testing: Bool) -> Bool {
        supportsHaptics && !testing
    }

    static let isTesting: Bool = {
        let environment = ProcessInfo.processInfo.environment
        return environment["XCTestConfigurationFilePath"] != nil || environment["XCTestBundlePath"] != nil
            || NSClassFromString("XCTestCase") != nil
    }()
}

// MARK: - Player

/// Plays the vocabulary above outside a view's body: recording state changes,
/// which can come from the recording screen, the note's menu or the Live
/// Activity's End, and are felt once whichever of those it was.
///
/// The UIKit generators follow the System Haptics setting by themselves.
/// Core Haptics does not, so the signatures play only while the app is active
/// and the person's own Haptics switch is on; Settings › Accessibility ›
/// Touch › Vibration silences everything, Core Haptics included.
@MainActor final class Haptics {
    static let shared = Haptics()

    private var engine: CHHapticEngine?
    private var engineRunning = false
    private let supportsCoreHaptics: Bool
    /// Overridable for tests: whether anything may play now.
    var isAllowed: @MainActor () -> Bool = {
        HapticGate.allows(enabled: HapticGate.isEnabled(), appActive: UIApplication.shared.applicationState == .active,
                          testing: HapticGate.isTesting)
    }

    init() {
        supportsCoreHaptics = HapticGate.usesCoreHaptics(supportsHaptics: CHHapticEngine.capabilitiesForHardware().supportsHaptics,
                                                         testing: HapticGate.isTesting)
    }

    func play(_ feedback: HapticFeedback) {
        guard isAllowed() else { return }
        let view = Self.keyWindow
        switch feedback {
        case .selection:
            let generator = view.map(UISelectionFeedbackGenerator.init(view:)) ?? UISelectionFeedbackGenerator()
            generator.selectionChanged()
        case .success, .warning, .error:
            let generator = view.map(UINotificationFeedbackGenerator.init(view:)) ?? UINotificationFeedbackGenerator()
            generator.notificationOccurred(feedback == .success ? .success : feedback == .warning ? .warning : .error)
        case .impact(let impact, let intensity):
            let style: UIImpactFeedbackGenerator.FeedbackStyle = switch impact {
            case .light: .light
            case .medium, .solid: .medium
            case .heavy: .heavy
            case .soft: .soft
            case .rigid: .rigid
            }
            let generator = view.map { UIImpactFeedbackGenerator(style: style, view: $0) } ?? UIImpactFeedbackGenerator(style: style)
            generator.impactOccurred(intensity: min(1, max(0, intensity)))
        }
    }

    func play(_ signature: SignatureHaptic) {
        guard isAllowed() else { return }
        guard supportsCoreHaptics, let engine = startedEngine(),
              let player = try? engine.makePlayer(with: signature.pattern.makePattern()) else {
            play(signature.fallback)
            return
        }
        do { try player.start(atTime: CHHapticTimeImmediate) } catch {
            // The engine went away under us (audio session reset, app moved): say it plainly instead.
            engineRunning = false
            play(signature.fallback)
        }
    }

    /// Starts the engine ahead of a signature that is about to be needed (the
    /// recording screen opening), so the first one plays on time.
    func prepare() {
        guard supportsCoreHaptics, isAllowed() else { return }
        _ = startedEngine()
    }

    private func startedEngine() -> CHHapticEngine? {
        if engine == nil {
            guard let made = try? CHHapticEngine() else { return nil }
            // Haptics only: the engine never touches the recording's audio session.
            made.playsHapticsOnly = true
            made.isAutoShutdownEnabled = true
            made.stoppedHandler = { [weak self] _ in
                Task { @MainActor in self?.engineRunning = false }
            }
            made.resetHandler = { [weak self] in
                // The haptic server restarted: the next signature starts the engine again.
                Task { @MainActor in self?.engineRunning = false }
            }
            engine = made
        }
        guard let engine else { return nil }
        if !engineRunning {
            do { try engine.start(); engineRunning = true } catch { return nil }
        }
        return engine
    }

    private static var keyWindow: UIWindow? {
        UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }
            .first { $0.activationState == .foregroundActive }?.keyWindow
    }
}

// MARK: - Views

extension View {
    /// Plays `feedback` each time `trigger` changes, unless the person has turned haptics off.
    func haptic<T: Equatable>(_ feedback: HapticFeedback, trigger: T) -> some View {
        modifier(HapticModifier(trigger: trigger) { _, _ in feedback })
    }

    /// Plays `feedback` when `trigger` changes and `condition` holds for the change.
    func haptic<T: Equatable>(_ feedback: HapticFeedback, trigger: T, condition: @escaping (T, T) -> Bool) -> some View {
        modifier(HapticModifier(trigger: trigger) { old, new in condition(old, new) ? feedback : nil })
    }

    /// Plays whatever `feedback` returns for the change, if anything.
    func haptic<T: Equatable>(trigger: T, _ feedback: @escaping (T, T) -> HapticFeedback?) -> some View {
        modifier(HapticModifier(trigger: trigger, feedback: feedback))
    }
}

private struct HapticModifier<T: Equatable>: ViewModifier {
    let trigger: T
    let feedback: (T, T) -> HapticFeedback?
    @AppStorage(HapticGate.storageKey) private var enabled = true

    func body(content: Content) -> some View {
        content.sensoryFeedback(trigger: trigger) { old, new in
            enabled ? feedback(old, new)?.sensory : nil
        }
    }
}
