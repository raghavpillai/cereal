import ActivityKit
import AppIntents
import SwiftUI
import WidgetKit

/// A chat answer finishing while the person is elsewhere, as the reference
/// shows it: the app's mark, what the answer is doing with a line under it,
/// and a ring with a stop square that turns while it runs, then a check.
struct CerealChatActivity: Widget {
    /// The app's `Theme.sendGold` (0xE0AF2F), which this extension cannot import.
    private let gold = Color(red: 0xE0 / 255, green: 0xAF / 255, blue: 0x2F / 255)

    var body: some WidgetConfiguration {
        ActivityConfiguration(for: ChatActivityAttributes.self) { context in
            HStack(spacing: 14) {
                mark(size: 44)
                VStack(alignment: .leading, spacing: 2) {
                    Text(context.state.title).font(.headline).lineLimit(1)
                    Text(context.state.subtitle).font(.subheadline).foregroundStyle(.secondary).lineLimit(1)
                }
                Spacer(minLength: 8)
                control(context, size: 44)
            }
            .padding(.horizontal, 20).padding(.vertical, 16)
            .activityBackgroundTint(nil)
            .activitySystemActionForegroundColor(gold)
            .widgetURL(ChatActivityAttributes.openURL)
            .accessibilityElement(children: .contain)
        } dynamicIsland: { context in
            DynamicIsland {
                DynamicIslandExpandedRegion(.leading) {
                    mark(size: 40).padding(.leading, 4)
                }
                DynamicIslandExpandedRegion(.center) {
                    VStack(alignment: .leading, spacing: 2) {
                        Text(context.state.title).font(.headline).lineLimit(1)
                        Text(context.state.subtitle).font(.subheadline).foregroundStyle(.secondary).lineLimit(1)
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
                DynamicIslandExpandedRegion(.trailing) {
                    control(context, size: 40).padding(.trailing, 4)
                }
            } compactLeading: {
                mark(size: 22)
            } compactTrailing: {
                status(context, size: 20)
            } minimal: {
                status(context, size: 20)
            }
            .widgetURL(ChatActivityAttributes.openURL)
            .keylineTint(gold)
        }
    }

    private func mark(size: CGFloat) -> some View {
        Image("CerealMark").resizable().interpolation(.high)
            .frame(width: size, height: size)
            .clipShape(RoundedRectangle(cornerRadius: size * 0.24, style: .continuous))
            .accessibilityHidden(true)
    }

    /// While the answer arrives: a turning ring around a stop square, which stops it.
    /// Afterwards: a check, or a pause mark when it was stopped.
    @ViewBuilder
    private func control(_ context: ActivityViewContext<ChatActivityAttributes>, size: CGFloat) -> some View {
        if context.state.isRunning && !context.isStale {
            Button(intent: StopChatAnswerIntent(answerID: context.attributes.answerID)) {
                ZStack {
                    ring(context, size: size)
                    RoundedRectangle(cornerRadius: 3, style: .continuous).fill(.primary).frame(width: size * 0.3, height: size * 0.3)
                }
                .frame(width: size, height: size)
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Stop answer")
        } else {
            status(context, size: size)
        }
    }

    @ViewBuilder
    private func status(_ context: ActivityViewContext<ChatActivityAttributes>, size: CGFloat) -> some View {
        if context.state.isRunning && !context.isStale {
            ring(context, size: size)
        } else {
            Image(systemName: context.state.phase == .done ? "checkmark.circle" : "pause.circle")
                .resizable().scaledToFit().fontWeight(.light)
                .frame(width: size, height: size)
                .accessibilityLabel(context.state.title)
        }
    }

    /// The system draws and turns the ring by itself; the activity is not updated for it.
    private func ring(_ context: ActivityViewContext<ChatActivityAttributes>, size: CGFloat) -> some View {
        ProgressView(timerInterval: context.state.startedAt...context.state.startedAt.addingTimeInterval(45), countsDown: false) { EmptyView() } currentValueLabel: { EmptyView() }
            .progressViewStyle(.circular)
            .tint(.primary)
            .frame(width: size, height: size)
            .accessibilityLabel(context.state.title)
    }
}
