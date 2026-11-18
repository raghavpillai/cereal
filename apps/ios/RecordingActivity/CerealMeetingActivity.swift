import ActivityKit
import SwiftUI
import WidgetKit

/// A meeting that is starting, as the reference shows it on the Lock Screen: a
/// card with the calendar's bar, the title over its times, and one wide
/// "Take a note" key that opens the event's note in the app and starts recording.
struct CerealMeetingActivity: Widget {
    /// The app's `Theme.accent` (0xE3B025), which this extension cannot import.
    private let accent = Color(red: 0xE3 / 255, green: 0xB0 / 255, blue: 0x25 / 255)

    var body: some WidgetConfiguration {
        ActivityConfiguration(for: MeetingActivityAttributes.self) { context in
            MeetingLockScreenCard(heading: { heading(context, titleSize: 17, timeSize: 15, ink: $0) }, key: takeNote(context, height: 50))
                .activitySystemActionForegroundColor(accent)
                .widgetURL(context.attributes.takeNoteURL)
        } dynamicIsland: { context in
            DynamicIsland {
                DynamicIslandExpandedRegion(.center) {
                    heading(context, titleSize: 16, timeSize: 14, ink: .white).padding(.horizontal, 6)
                }
                DynamicIslandExpandedRegion(.bottom) {
                    takeNote(context, height: 44).padding(.top, 4)
                }
            } compactLeading: {
                mark(size: 20).foregroundStyle(accent)
            } compactTrailing: {
                Text(context.state.startTime()).font(.system(size: 14, weight: .medium)).monospacedDigit().foregroundStyle(accent)
            } minimal: {
                mark(size: 18).foregroundStyle(accent)
            }
            .widgetURL(context.attributes.takeNoteURL)
            .keylineTint(accent)
        }
    }

    private func heading(_ context: ActivityViewContext<MeetingActivityAttributes>, titleSize: CGFloat, timeSize: CGFloat, ink: Color) -> some View {
        HStack(spacing: 10) {
            RoundedRectangle(cornerRadius: 2, style: .continuous).fill(bar(context.attributes.color)).frame(width: 4)
            VStack(alignment: .leading, spacing: 2) {
                Text(context.state.shownTitle).font(.system(size: titleSize, weight: .semibold)).foregroundStyle(ink).lineLimit(1)
                Text(context.state.timeRange()).font(.system(size: timeSize)).monospacedDigit().foregroundStyle(ink.opacity(0.6)).lineLimit(1)
            }
            Spacer(minLength: 0)
        }
        .fixedSize(horizontal: false, vertical: true)
        .accessibilityElement(children: .combine)
    }

    private func takeNote(_ context: ActivityViewContext<MeetingActivityAttributes>, height: CGFloat) -> some View {
        Link(destination: context.attributes.takeNoteURL) {
            HStack(spacing: 8) {
                mark(size: 20)
                Text("Take a note").font(.system(size: 17, weight: .semibold))
            }
            .foregroundStyle(Color(white: 0.11))
            .frame(maxWidth: .infinity).frame(height: height)
            .background(accent, in: Capsule())
        }
        .accessibilityLabel("Take a note for \(context.state.shownTitle)")
    }

    /// The event's calendar colour, as on Home; gold when the calendar gives none.
    private func bar(_ color: String?) -> Color {
        guard let rgb = MeetingActivityAttributes.rgb(color) else { return accent }
        return Color(red: rgb.red, green: rgb.green, blue: rgb.blue)
    }

    private func mark(size: CGFloat) -> some View {
        CerealMarkShape().fill(style: FillStyle(eoFill: true)).frame(width: size, height: size).accessibilityHidden(true)
    }
}

/// The Lock Screen card on the system's own translucent material, which follows the
/// phone's appearance; the ink is the system's, so it reads on both. The bar and key are the same
/// in Light and Dark.
private struct MeetingLockScreenCard<Heading: View, Key: View>: View {
    @ViewBuilder var heading: (Color) -> Heading
    let key: Key

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            heading(.primary)
            key
        }
        .padding(16)
        .activityBackgroundTint(nil)
    }
}
