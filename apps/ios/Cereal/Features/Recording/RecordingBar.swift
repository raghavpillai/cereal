import SwiftUI

/// The recording screen's control bar, as in the reference (Granola's iPhone app):
/// one capsule as wide as the text column, 64 pt tall, holding a 56 pt Pause
/// circle, the level meter and clock, and a Cancel or End capsule. The
/// reference's lime becomes Cereal's gold, with near-black glyphs and words.
///
/// - Recording: a gold capsule; the controls are lighter gold glass inside it.
/// - Paused: the gold drains to plain glass, Pause turns into Play and the meter
///   rests as a row of dots. The words keep their place.
/// - Typing (`compact`): the bar folds to a small gold chip of the meter and the
///   clock at the leading edge; the keyboard's own control sits at the trailing edge.
///
/// Cancel is offered for the first seconds of a recording made for a new note
/// (`cancelWindow`); End from then on.
struct RecordingBar: View {
    let recorder: Recorder
    /// The keyboard is up: the bar is the small chip.
    var compact = false
    /// Cancel is offered while the recording is younger than this many seconds. Zero: End only.
    var cancelWindow: TimeInterval = 0
    /// The controls fade in over the capsule, which is there at once (the bar's entrance).
    var fadesIn = false
    let onPauseToggle: () -> Void
    let onCancel: () -> Void
    let onEnd: () -> Void
    var onHideKeyboard: (() -> Void)? = nil

    @Environment(\.motion) private var motion
    /// Flips to End once the recording has run for `cancelWindow` seconds. Kept in
    /// state and set from a task, so the clock's ticks never rebuild this view.
    @State private var cancelExpired = false
    @State private var contentShown = false

    /// Measured in the reference at 3 px per point.
    enum Metrics {
        static let height: CGFloat = 64
        static let control: CGFloat = 56
        static let inset: CGFloat = 4
        static let trailingWidth: CGFloat = 97
        static let chipHeight: CGFloat = 47
        /// From the bar's lower edge to the screen's.
        static let screenEdge: CGFloat = 30
    }

    /// The reference's lime bar (0xA9BA41) as Cereal's gold, and its lighter
    /// lime controls (0xC8DA52) as a lighter gold.
    static let gold = Theme.sendGold
    static let goldControl = Color(hex: 0xF2C64A)
    static let goldRim = Color(hex: 0xC2931F)
    static let ink = Theme.sendGlyph
    /// End: the reference's near-black capsule (0x252525) with white words.
    static let endFill = Color(hex: 0x252525)

    private var tinted: Bool { recorder.state == .recording }
    private var showsCancel: Bool { cancelWindow > 0 && !cancelExpired }

    var body: some View {
        let _ = PerfProbe.hit("RecordingBar.body")
        HStack(spacing: 12) {
            bar
            if compact {
                Spacer(minLength: 0)
                if let onHideKeyboard {
                    HideKeyboardButton(action: onHideKeyboard)
                        .transition(motion.pop(anchor: .trailing))
                }
            }
        }
        .animation(motion.gooey, value: compact)
        .animation(motion.snappy, value: recorder.state)
        .animation(motion.snappy, value: showsCancel)
        .onAppear {
            guard fadesIn, !contentShown else { contentShown = true; return }
            withAnimation(motion.reduceMotion ? nil : .easeOut(duration: 0.22)) { contentShown = true }
        }
        .task(id: TaskKey(state: recorder.state, window: cancelWindow)) { await watchCancelWindow() }
        .sensoryFeedback(trigger: recorder.state) { old, new in RecordingFeedback.feedback(from: old, to: new) }
    }

    private var bar: some View {
        HStack(spacing: 0) {
            if !compact {
                leading.transition(.opacity.combined(with: .scale(scale: 0.7, anchor: .leading)))
                    .opacity(contentShown ? 1 : 0)
                Spacer(minLength: 8)
            }
            meter.opacity(contentShown ? 1 : 0)
            if !compact {
                Spacer(minLength: 8)
                trailing.transition(.opacity.combined(with: .scale(scale: 0.7, anchor: .trailing)))
                    .opacity(contentShown ? 1 : 0)
            }
        }
        .padding(.horizontal, compact ? 14 : Metrics.inset)
        .frame(height: compact ? Metrics.chipHeight : Metrics.height)
        .frame(maxWidth: compact ? nil : .infinity)
        .background { surface }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("recording-bar")
    }

    /// Plain glass underneath, the gold laid over it while recording: pausing
    /// drains the gold away rather than swapping one surface for another.
    private var surface: some View {
        ZStack {
            // Only while paused: under the gold its edge would show as a halo as it materialises.
            if !(tinted || compact) { Capsule().fill(.clear).glassSurface(Capsule()).transition(.opacity) }
            Capsule().fill(Self.gold)
                .overlay(Capsule().strokeBorder(Self.goldRim.opacity(0.55), lineWidth: 1.5))
                .opacity(tinted || compact ? 1 : 0)
        }
        .compositingGroup()
        .shadow(color: .black.opacity(0.10), radius: 10, y: 4)
    }

    private var ink: Color { tinted || compact ? Self.ink : Theme.textPrimary }

    private var meter: some View {
        HStack(spacing: compact ? 8 : 10) {
            WaveformBars(levels: recorder.levels, active: recorder.state == .recording, color: ink, style: .bar)
                .accessibilityHidden(true)
            RecorderTimer(recorder: recorder)
                .font(.system(size: compact ? 18 : 20).monospacedDigit())
                .foregroundStyle(ink)
        }
        .fixedSize()
    }

    private var leading: some View {
        Button(action: onPauseToggle) {
            ZStack {
                if recorder.state == .recording {
                    PauseGlyph().transition(motion.swap)
                } else {
                    Image(systemName: "play.fill").font(.system(size: 21)).offset(x: 1.5).transition(motion.swap)
                }
            }
            .foregroundStyle(tinted ? Color.black : Theme.textPrimary)
            .frame(width: Metrics.control, height: Metrics.control)
            .contentShape(Circle())
        }
        .buttonStyle(.plain)
        .glassSurface(Circle(), interactive: true, tint: tinted ? Self.goldControl : nil)
        .accessibilityLabel(recorder.state == .recording ? "Pause recording" : "Resume recording")
        .accessibilityIdentifier("recording-bar-pause")
    }

    @ViewBuilder private var trailing: some View {
        if showsCancel {
            Button(action: onCancel) {
                Text("Cancel").font(.system(size: 18)).foregroundStyle(ink)
                    .frame(width: Metrics.trailingWidth, height: Metrics.control)
                    .contentShape(Capsule())
            }
            .buttonStyle(.plain)
            .glassSurface(Capsule(), interactive: true, tint: tinted ? Self.goldControl : nil)
            .accessibilityLabel("Cancel recording")
            .accessibilityHint("Stops the recording and removes this note")
            .accessibilityIdentifier("recording-bar-cancel")
            .transition(.opacity)
        } else {
            Button(action: onEnd) {
                Text("End").font(.system(size: 18)).foregroundStyle(.white)
                    .frame(width: Metrics.trailingWidth, height: Metrics.control)
                    .background(Self.endFill, in: Capsule())
                    .contentShape(Capsule())
            }
            .buttonStyle(.squash)
            .accessibilityLabel("End recording")
            .accessibilityIdentifier("recording-bar-end")
            .transition(.opacity)
        }
    }

    private struct TaskKey: Equatable { let state: Recorder.State; let window: TimeInterval }

    /// Reads the clock off the view's body: a few times a second until Cancel's time is up.
    private func watchCancelWindow() async {
        guard cancelWindow > 0, !cancelExpired else { return }
        while !Task.isCancelled {
            if recorder.elapsed >= cancelWindow { cancelExpired = true; return }
            // A paused recording's clock stands still; so does Cancel's time.
            guard recorder.state == .recording else { return }
            try? await Task.sleep(for: .milliseconds(250))
        }
    }
}

/// The reference's Pause glyph: two heavy rounded bars, heavier than SF Symbols' pause.
struct PauseGlyph: View {
    var body: some View {
        HStack(spacing: 3.5) {
            RoundedRectangle(cornerRadius: 1.4).frame(width: 5.2, height: 16.5)
            RoundedRectangle(cornerRadius: 1.4).frame(width: 5.2, height: 16.5)
        }
        .accessibilityHidden(true)
    }
}

/// The hint that stands where the bar will be while a new recording gets going:
/// "Let people know you're using Cereal", centred on two lines.
struct RecordingConsentHint: View {
    var body: some View {
        Text("Let people know you're using Cereal")
            .font(.system(size: 18))
            .foregroundStyle(Theme.textPrimary)
            .multilineTextAlignment(.center)
            .frame(maxWidth: 230)
            .frame(maxWidth: .infinity)
            .frame(height: RecordingBar.Metrics.height)
            .accessibilityIdentifier("recording-consent-hint")
    }
}
