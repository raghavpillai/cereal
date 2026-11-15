import ActivityKit
import SwiftUI
import WidgetKit

@main
struct CerealRecordingWidgets: WidgetBundle {
    var body: some Widget { CerealRecordingActivity() }
}

struct CerealRecordingActivity: Widget {
    /// The app's `Theme.accent` (0xE3B025), which this extension cannot import.
    private let accent = Color(red: 0xE3 / 255, green: 0xB0 / 255, blue: 0x25 / 255)

    var body: some WidgetConfiguration {
        ActivityConfiguration(for: RecordingActivityAttributes.self) { context in
            HStack(spacing: 14) {
                indicator(context)
                    .font(.system(size: 24, weight: .medium))
                    .frame(width: 36)
                VStack(alignment: .leading, spacing: 4) {
                    // A titled note is named, with what the recording is doing beneath it.
                    // A note with no title yet says what the recording is doing.
                    // A title keeps to one line; what the recording is doing may take two
                    // ("Open to check recording" does not fit beside the timer on one).
                    Text(context.state.title ?? status(context)).font(.headline).lineLimit(context.state.title == nil ? 2 : 1)
                    Text(context.state.title != nil ? status(context) : context.attributes.isPreview ? "Preview · No audio" : "Cereal · On-device audio")
                        .font(.caption).foregroundStyle(.secondary).lineLimit(1)
                }
                Spacer(minLength: 8)
                timer(context).font(.title3.monospacedDigit()).frame(maxWidth: 90, alignment: .trailing)
            }
            .padding(20)
            // The host supplies its native material on the Lock Screen.
            .activityBackgroundTint(nil)
            .activitySystemActionForegroundColor(accent)
            .widgetURL(context.attributes.recordingURL)
            .accessibilityElement(children: .combine)
        } dynamicIsland: { context in
            DynamicIsland {
                DynamicIslandExpandedRegion(.leading) {
                    Label(context.attributes.isPreview ? "Preview" : "Cereal", systemImage: context.attributes.isPreview ? "play.circle" : "mic.fill").foregroundStyle(accent)
                }
                DynamicIslandExpandedRegion(.trailing) {
                    timer(context).monospacedDigit().frame(width: 80)
                }
                DynamicIslandExpandedRegion(.bottom) {
                    HStack(alignment: .lastTextBaseline) {
                        VStack(alignment: .leading, spacing: 2) {
                            if let title = context.state.title { Text(title).font(.subheadline.weight(.semibold)).lineLimit(1) }
                            Text(status(context)).font(context.state.title == nil ? .subheadline : .caption)
                                .foregroundStyle(context.state.title == nil ? .primary : .secondary)
                        }
                        Spacer(minLength: 8)
                        Text(context.attributes.isPreview ? "Preview · No audio" : "Open recording").font(.caption).foregroundStyle(accent)
                    }
                }
            } compactLeading: {
                indicator(context).foregroundStyle(accent)
            } compactTrailing: {
                timer(context).monospacedDigit().frame(width: 52)
            } minimal: {
                indicator(context).foregroundStyle(accent)
            }
            .widgetURL(context.attributes.recordingURL)
            .keylineTint(accent)
        }
    }

    private func status(_ context: ActivityViewContext<RecordingActivityAttributes>) -> String {
        context.state.status(isStale: context.isStale, isPreview: context.attributes.isPreview)
    }

    private func indicator(_ context: ActivityViewContext<RecordingActivityAttributes>) -> some View {
        // Inherit the Lock Screen's semantic foreground. Island call sites
        // apply the gold accent explicitly against the system's dark shell.
        Image(systemName: context.state.symbol(isStale: context.isStale, isPreview: context.attributes.isPreview))
            .accessibilityLabel(status(context))
    }

    @ViewBuilder
    private func timer(_ context: ActivityViewContext<RecordingActivityAttributes>) -> some View {
        // The stopped time is written as the running one is, so pausing does not
        // change how the number looks or where it sits.
        if context.state.runsTimer(isStale: context.isStale) {
            Text(timerInterval: context.state.timerAnchor...Date.distantFuture, countsDown: false).multilineTextAlignment(.trailing)
        } else {
            Text(Duration.seconds(context.state.elapsed).formatted(.time(pattern: context.state.elapsed >= 3600 ? .hourMinuteSecond : .minuteSecond)))
        }
    }
}
