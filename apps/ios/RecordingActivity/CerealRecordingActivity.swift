import ActivityKit
import AppIntents
import SwiftUI
import WidgetKit

@main
struct CerealRecordingWidgets: WidgetBundle {
    var body: some Widget {
        CerealRecordingActivity()
        CerealChatActivity()
    }
}

/// The recording outside the app, drawn as the reference's "Taking notes..."
/// activity: the app's mark, what the recording is doing over its clock, and End.
struct CerealRecordingActivity: Widget {
    /// The app's `Theme.accent` (0xE3B025), which this extension cannot import.
    private let accent = Color(red: 0xE3 / 255, green: 0xB0 / 255, blue: 0x25 / 255)
    /// The reference's End and camera keys: dark grey rounded squares on the island's black.
    private let key = Color(white: 0.17)

    var body: some WidgetConfiguration {
        ActivityConfiguration(for: RecordingActivityAttributes.self) { context in
            HStack(spacing: 12) {
                mark(size: 44)
                VStack(alignment: .leading, spacing: 2) {
                    Text(headline(context)).font(.system(size: 17, weight: .semibold)).lineLimit(1)
                    HStack(spacing: 6) {
                        timer(context).monospacedDigit()
                        // A titled note is named beside its clock.
                        if let title = context.state.title { Text("· \(title)").lineLimit(1) }
                    }
                    .font(.system(size: 15)).foregroundStyle(.secondary)
                }
                Spacer(minLength: 8)
                endButton(context)
            }
            .padding(.horizontal, 18).padding(.vertical, 16)
            // The host supplies its native material on the Lock Screen.
            .activityBackgroundTint(nil)
            .activitySystemActionForegroundColor(accent)
            .widgetURL(context.attributes.recordingURL)
        } dynamicIsland: { context in
            DynamicIsland {
                DynamicIslandExpandedRegion(.leading) {
                    HStack(spacing: 12) {
                        mark(size: 40)
                        VStack(alignment: .leading, spacing: 1) {
                            Text(headline(context)).font(.system(size: 17, weight: .medium)).foregroundStyle(.white).lineLimit(1)
                            timer(context).monospacedDigit().font(.system(size: 15)).foregroundStyle(.white.opacity(0.55))
                        }
                    }
                    .padding(.leading, 6)
                    .frame(maxHeight: .infinity)
                }
                DynamicIslandExpandedRegion(.trailing) {
                    endButton(context).padding(.trailing, 4).frame(maxHeight: .infinity)
                }
            } compactLeading: {
                mark(size: 22)
            } compactTrailing: {
                timer(context).monospacedDigit().foregroundStyle(.white).frame(width: 46)
            } minimal: {
                mark(size: 22)
            }
            .widgetURL(context.attributes.recordingURL)
            .keylineTint(accent)
        }
    }

    /// "Taking notes...", as in the reference, while recording.
    private func headline(_ context: ActivityViewContext<RecordingActivityAttributes>) -> String {
        if context.isStale || context.attributes.isPreview { return status(context) }
        switch context.state.phase {
        case .recording: return "Taking notes..."
        case .paused: return "Paused"
        case .ended: return "Recording ended"
        }
    }

    private func status(_ context: ActivityViewContext<RecordingActivityAttributes>) -> String {
        context.state.status(isStale: context.isStale, isPreview: context.attributes.isPreview)
    }

    /// The app's mark: its icon, as a small rounded square.
    private func mark(size: CGFloat) -> some View {
        Image("CerealMark").resizable().interpolation(.high)
            .frame(width: size, height: size)
            .clipShape(RoundedRectangle(cornerRadius: size * 0.24, style: .continuous))
            .accessibilityHidden(true)
    }

    @ViewBuilder
    private func endButton(_ context: ActivityViewContext<RecordingActivityAttributes>) -> some View {
        // A preview has nothing to end; a finished recording neither.
        if !context.attributes.isPreview, context.state.phase != .ended {
            Button(intent: EndRecordingIntent(noteID: context.attributes.noteID)) {
                Text("End").font(.system(size: 17, weight: .medium)).foregroundStyle(.white)
                    .frame(width: 70, height: 44)
                    .background(key, in: RoundedRectangle(cornerRadius: 13, style: .continuous))
            }
            .buttonStyle(.plain)
            .accessibilityLabel("End recording")
        }
    }

    @ViewBuilder
    private func timer(_ context: ActivityViewContext<RecordingActivityAttributes>) -> some View {
        // The stopped time is written as the running one is, so pausing does not
        // change how the number looks or where it sits.
        if context.state.runsTimer(isStale: context.isStale) {
            Text(timerInterval: context.state.timerAnchor...Date.distantFuture, countsDown: false)
        } else {
            Text(Duration.seconds(context.state.elapsed).formatted(.time(pattern: context.state.elapsed >= 3600 ? .hourMinuteSecond : .minuteSecond)))
        }
    }
}
