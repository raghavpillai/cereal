import SwiftUI
import UIKit
import UserNotifications

/// The line under a note's title while its recording is turned into notes, as
/// in the reference: a rounded grey row with a spinner and the step in words
/// ("Uploading transcript...", then "Generating notes..."), a darker band
/// sweeping across the words. 44 pt tall, level with the meta chips.
struct NoteProgressRow: View {
    let text: String

    var body: some View {
        HStack(spacing: 12) {
            ProgressView().controlSize(.small).tint(Theme.textTertiary)
            ShimmerText(text: text)
        }
        .padding(.horizontal, 15)
        .frame(height: 44)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Theme.quietFill, in: RoundedRectangle(cornerRadius: 15, style: .continuous))
        .padding(.leading, -HomeMetrics.tileOutdent)
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("note-progress-row")
    }

    /// The words for the step a stopped recording is at, or nil when there is none.
    static func text(finishing: Bool, generating: Bool) -> String? {
        if finishing { return "Uploading transcript..." }
        if generating { return "Generating notes..." }
        return nil
    }
}

/// Words with a darker band passing over them from the leading edge, about
/// once every 1.6 s. Still under Reduce Motion.
struct ShimmerText: View {
    let text: String
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        let label = Text(text).font(.system(size: 16, weight: .medium))
        label.foregroundStyle(Theme.textTertiary)
            .overlay {
                if !reduceMotion {
                    TimelineView(.animation) { context in
                        let period = 1.6
                        let phase = context.date.timeIntervalSinceReferenceDate.truncatingRemainder(dividingBy: period) / period
                        label.foregroundStyle(Theme.textSecondary)
                            .mask {
                                GeometryReader { proxy in
                                    let band = max(60, proxy.size.width * 0.45)
                                    LinearGradient(colors: [.clear, .black, .clear], startPoint: .leading, endPoint: .trailing)
                                        .frame(width: band)
                                        .offset(x: -band + (proxy.size.width + band) * phase)
                                }
                            }
                    }
                    .accessibilityHidden(true)
                }
            }
            .lineLimit(1)
    }
}

/// The card offered once a recording has ended, while notifications have not
/// been asked for: "Get notified when your notes are ready", with Notify me.
struct NotifyWhenReadyCard: View {
    let onNotify: () -> Void
    let onDismiss: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(alignment: .top) {
                Image(systemName: "bell.fill").font(.system(size: 14)).foregroundStyle(Theme.accent)
                    .accessibilityHidden(true)
                Spacer()
                Button(action: onDismiss) {
                    Image(systemName: "xmark").font(.system(size: 13, weight: .medium)).foregroundStyle(Theme.textTertiary)
                        .frame(width: 32, height: 32).contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .padding(.trailing, -8).padding(.top, -8)
                .accessibilityLabel("Not now")
            }
            Text("Get notified when your notes are ready")
                .font(.system(size: 15, weight: .semibold)).foregroundStyle(Theme.textPrimary)
                .padding(.top, 8)
            Text("Notes usually take a few minutes to generate. We'll let you know when they're ready")
                .font(.system(size: 13)).foregroundStyle(Theme.textSecondary)
                .padding(.top, 4)
                .fixedSize(horizontal: false, vertical: true)
            Button(action: onNotify) {
                Text("Notify me").font(.system(size: 15, weight: .medium))
                    .foregroundStyle(Theme.onStartMeeting)
                    .frame(maxWidth: .infinity).frame(height: 40)
                    .background(Theme.startMeeting, in: Capsule())
                    .contentShape(Capsule())
            }
            .buttonStyle(.squash)
            .padding(.top, 12)
            .accessibilityIdentifier("notify-when-ready")
        }
        .padding(14)
        .background(Theme.surfaceRaised, in: RoundedRectangle(cornerRadius: 16, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: 16, style: .continuous).strokeBorder(Theme.stroke, lineWidth: 1))
        .padding(.leading, -HomeMetrics.tileOutdent)
    }
}

/// "Your notes are ready": asked for from the card above, sent when a
/// recording's notes are written while the app is in the background.
@MainActor
enum NotesReadyNotifications {
    static let dismissedKey = "notesReady.cardDismissed"

    /// True while the card should be offered: notifications were never asked
    /// for, and the person has not closed the card.
    static func shouldOffer() async -> Bool {
        guard !UserDefaults.standard.bool(forKey: dismissedKey), !LaunchContext.isTestHost else { return false }
        return await UNUserNotificationCenter.current().notificationSettings().authorizationStatus == .notDetermined
    }

    static func request() async {
        _ = try? await UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound])
        UserDefaults.standard.set(true, forKey: dismissedKey)
    }

    static func dismiss() { UserDefaults.standard.set(true, forKey: dismissedKey) }

    /// Only the note's title leaves the app, as on the Live Activity.
    static func notesReady(title: String, noteID: String) async {
        guard UIApplication.shared.applicationState != .active else { return }
        let center = UNUserNotificationCenter.current()
        guard await center.notificationSettings().authorizationStatus == .authorized else { return }
        let content = UNMutableNotificationContent()
        let shown = title.trimmingCharacters(in: .whitespacesAndNewlines)
        content.title = "Your notes are ready"
        content.body = shown.isEmpty ? "Untitled note" : shown
        content.sound = .default
        try? await center.add(UNNotificationRequest(identifier: "notes-ready-\(noteID)", content: content, trigger: nil))
    }
}
