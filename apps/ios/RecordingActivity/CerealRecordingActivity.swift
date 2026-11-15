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
                    Text(status(context)).font(.headline)
                    Text(context.attributes.isPreview ? "Preview · No audio" : "Cereal · On-device audio")
                        .font(.caption).foregroundStyle(.secondary)
                }
                Spacer(minLength: 8)
                timer(context).font(.title3.monospacedDigit()).frame(maxWidth: 90)
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
                    HStack {
                        Text(status(context)).font(.subheadline)
                        Spacer()
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
        if context.state.runsTimer(isStale: context.isStale) {
            Text(timerInterval: context.state.timerAnchor...Date.distantFuture, countsDown: false)
        } else {
            Text(Duration.seconds(context.state.elapsed).formatted(.time(pattern: .minuteSecond(padMinuteToLength: 2))))
        }
    }
}
