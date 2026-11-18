import ActivityKit
import AppIntents
import SwiftUI
import WidgetKit

@main
struct CerealRecordingWidgets: WidgetBundle {
    var body: some Widget {
        CerealRecordingActivity()
        CerealChatActivity()
        CerealMeetingActivity()
    }
}

/// The recording outside the app, drawn as the reference's "Taking notes..."
/// activity: the app's mark, what the recording is doing over its clock, and End.
///
/// After End it stays to say "Writing notes..." and then "Notes ready" (only once
/// they are), with a key that opens the note; a recording that stopped on its own
/// says so and offers the note, where it resumes.
struct CerealRecordingActivity: Widget {
    typealias State = RecordingActivityAttributes.ContentState
    /// The app's `Theme.accent` (0xE3B025), which this extension cannot import.
    private let accent = Color(red: 0xE3 / 255, green: 0xB0 / 255, blue: 0x25 / 255)
    /// The reference's End and camera keys: dark grey rounded squares on the island's black.
    private let key = Color(white: 0.17)

    var body: some WidgetConfiguration {
        ActivityConfiguration(for: RecordingActivityAttributes.self) { context in
            RecordingLockScreenView(context: context, accent: accent, key: key)
                // The host supplies its native material on the Lock Screen.
                .activityBackgroundTint(nil)
                .activitySystemActionForegroundColor(accent)
                .widgetURL(context.attributes.recordingURL)
        } dynamicIsland: { context in
            DynamicIsland {
                DynamicIslandExpandedRegion(.leading) {
                    RecordingMark(size: 40, badge: context.state.badge(isStale: context.isStale), accent: accent)
                        .padding(.leading, 6).frame(maxHeight: .infinity)
                }
                // The words in the middle, where they have the room: beside the mark the
                // leading region is too narrow for "Paused" or "Taking notes...".
                DynamicIslandExpandedRegion(.center) {
                    VStack(alignment: .leading, spacing: 1) {
                        Text(headline(context)).font(.system(size: 17, weight: .medium)).foregroundStyle(.white).lineLimit(1)
                            .contentTransition(.interpolate)
                        RecordingClockText(state: context.state, isStale: context.isStale)
                            .font(.system(size: 15)).foregroundStyle(.white.opacity(0.55))
                    }
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)
                }
                DynamicIslandExpandedRegion(.trailing) {
                    RecordingKey(context: context, fill: key).padding(.trailing, 4).frame(maxHeight: .infinity)
                }
            } compactLeading: {
                RecordingMark(size: 22, badge: nil, accent: accent)
            } compactTrailing: {
                compactTrailing(context)
            } minimal: {
                minimal(context)
            }
            .widgetURL(context.attributes.recordingURL)
            .keylineTint(accent)
        }
        // The Apple Watch Smart Stack and CarPlay show the small form.
        .supplementalActivityFamilies([.small])
    }

    private func headline(_ context: ActivityViewContext<RecordingActivityAttributes>) -> String {
        context.state.headline(isStale: context.isStale, isPreview: context.attributes.isPreview)
    }

    /// Recording: the running clock, as in the reference. Paused: the stopped clock in
    /// gold beside a pause glyph. After End: a glyph for writing, ready or stopped.
    @ViewBuilder
    private func compactTrailing(_ context: ActivityViewContext<RecordingActivityAttributes>) -> some View {
        let state = context.state
        if state.runsTimer(isStale: context.isStale) {
            RecordingClockText(state: state, isStale: context.isStale)
                .foregroundStyle(.white).frame(width: 46)
        } else if state.phase == .paused && !context.isStale {
            HStack(spacing: 3) {
                Image(systemName: "pause.fill").font(.system(size: 10, weight: .bold))
                RecordingClockText(state: state, isStale: context.isStale)
            }
            .foregroundStyle(accent)
            .accessibilityElement(children: .ignore)
            .accessibilityLabel("Paused")
        } else {
            Image(systemName: state.symbol(isStale: context.isStale, isPreview: context.attributes.isPreview))
                .font(.system(size: 14, weight: .semibold))
                .foregroundStyle(accent)
                .contentTransition(.symbolEffect(.replace))
                .accessibilityLabel(state.status(isStale: context.isStale, isPreview: context.attributes.isPreview))
        }
    }

    @ViewBuilder
    private func minimal(_ context: ActivityViewContext<RecordingActivityAttributes>) -> some View {
        if context.state.phase == .recording && !context.isStale {
            RecordingMark(size: 22, badge: nil, accent: accent)
        } else {
            Image(systemName: context.state.symbol(isStale: context.isStale, isPreview: context.attributes.isPreview))
                .font(.system(size: 13, weight: .semibold))
                .foregroundStyle(accent)
                .contentTransition(.symbolEffect(.replace))
                .accessibilityLabel(context.state.status(isStale: context.isStale, isPreview: context.attributes.isPreview))
        }
    }
}

/// The Lock Screen, StandBy, and (small) the Apple Watch Smart Stack and CarPlay.
private struct RecordingLockScreenView: View {
    let context: ActivityViewContext<RecordingActivityAttributes>
    let accent: Color
    let key: Color
    @Environment(\.activityFamily) private var family

    private var state: RecordingActivityAttributes.ContentState { context.state }
    private var headline: String { state.headline(isStale: context.isStale, isPreview: context.attributes.isPreview) }
    private var badge: String? { state.badge(isStale: context.isStale) }

    var body: some View {
        if family == .small {
            // Small and glanceable: no keys, a tap opens the note on the iPhone.
            HStack(spacing: 8) {
                RecordingMark(size: 28, badge: badge, accent: accent)
                VStack(alignment: .leading, spacing: 0) {
                    Text(headline).font(.system(size: 14, weight: .semibold)).lineLimit(1)
                        .contentTransition(.interpolate)
                    RecordingClockText(state: state, isStale: context.isStale)
                        .font(.system(size: 13)).foregroundStyle(.secondary)
                }
                Spacer(minLength: 0)
            }
            .padding(10)
        } else {
            HStack(spacing: 12) {
                RecordingMark(size: 44, badge: badge, accent: accent)
                VStack(alignment: .leading, spacing: 2) {
                    Text(headline).font(.system(size: 17, weight: .semibold)).lineLimit(1)
                        .contentTransition(.interpolate)
                    // One run of text: a running timer laid out on its own reserves room for
                    // hours, which pushed the title away from the clock.
                    // A titled note is named beside its clock; what to do comes first.
                    (RecordingClockText.text(state, isStale: context.isStale)
                        + Text(state.secondary.map { "  ·  \($0)" } ?? ""))
                        .lineLimit(1)
                        .font(.system(size: 15)).foregroundStyle(.secondary)
                }
                Spacer(minLength: 8)
                RecordingKey(context: context, fill: key)
            }
            .padding(.horizontal, 18).padding(.vertical, 16)
        }
    }
}

/// The app's mark: its icon as a small rounded square, with a gold badge on its
/// corner for a paused, stopped, writing or ready recording.
private struct RecordingMark: View {
    let size: CGFloat
    let badge: String?
    let accent: Color

    var body: some View {
        Image("CerealMark").resizable().interpolation(.high)
            .frame(width: size, height: size)
            .clipShape(RoundedRectangle(cornerRadius: size * 0.24, style: .continuous))
            .overlay(alignment: .bottomTrailing) {
                if let badge {
                    let side = max(14, size * 0.42)
                    Image(systemName: badge)
                        .font(.system(size: side * 0.5, weight: .bold))
                        .foregroundStyle(.black)
                        .contentTransition(.symbolEffect(.replace))
                        .frame(width: side, height: side)
                        .background(accent, in: Circle())
                        .overlay(Circle().strokeBorder(.black.opacity(0.25), lineWidth: 1))
                        .offset(x: side * 0.28, y: side * 0.28)
                }
            }
            .accessibilityHidden(true)
    }
}

/// The clock: running while recording (the system draws it, so no updates are
/// needed), stopped otherwise. The stopped time is written as the running one is,
/// so pausing does not change how the number looks or where it sits.
private struct RecordingClockText: View {
    let state: RecordingActivityAttributes.ContentState
    let isStale: Bool

    var body: some View {
        Self.text(state, isStale: isStale).contentTransition(.numericText())
    }

    static func text(_ state: RecordingActivityAttributes.ContentState, isStale: Bool) -> Text {
        if state.runsTimer(isStale: isStale) {
            return Text(timerInterval: state.timerAnchor...Date.distantFuture, countsDown: false).monospacedDigit()
        }
        return Text(Duration.seconds(state.elapsed).formatted(.time(pattern: state.elapsed >= 3600 ? .hourMinuteSecond : .minuteSecond)))
            .monospacedDigit()
    }
}

/// The activity's one key, a dark rounded square as in the reference: End while
/// the microphone is in use or held; Open once there is a note to go to (notes
/// ready, or a recording to resume). None while the notes are being written.
private struct RecordingKey: View {
    let context: ActivityViewContext<RecordingActivityAttributes>
    let fill: Color

    var body: some View {
        // A preview has nothing to end or open.
        if !context.attributes.isPreview {
            if context.state.isLive {
                Button(intent: EndRecordingIntent(noteID: context.attributes.noteID)) { label("End") }
                    .buttonStyle(.plain)
                    .accessibilityLabel("End recording")
            } else if context.state.offersOpen && !context.isStale {
                Link(destination: context.attributes.recordingURL) { label("Open") }
                    .accessibilityLabel(context.state.phase == .ready ? "Open notes" : "Open recording")
            }
        }
    }

    private func label(_ text: String) -> some View {
        Text(text).font(.system(size: 17, weight: .medium)).foregroundStyle(.white)
            .frame(width: 70, height: 44)
            .background(fill, in: RoundedRectangle(cornerRadius: 13, style: .continuous))
    }
}
