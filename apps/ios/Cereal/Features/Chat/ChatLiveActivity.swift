// ActivityKit's Activity reference predates Sendable annotations; every call is made on the main actor.
@preconcurrency import ActivityKit
import Foundation
import UIKit

/// What a conversation tells the system about an answer it is waiting for, so
/// the answer can be followed from the Lock Screen and the Dynamic Island once
/// the person leaves the app. Injected, so tests and previews start nothing.
struct ChatActivityHooks {
    /// The question went out. Returns the activity's answer ID, or nil when none was started.
    var begin: @MainActor (_ stop: @escaping @MainActor () -> Void) -> String?
    /// The first words of the answer arrived.
    var writing: @MainActor (String) -> Void
    /// The answer ended: finished (`true`) or stopped or failed.
    var end: @MainActor (String, _ finished: Bool) -> Void

    static let none = ChatActivityHooks(begin: { _ in nil }, writing: { _ in }, end: { _, _ in })

    @MainActor static let system = ChatActivityHooks(
        begin: { ChatLiveActivities.shared.begin(stop: $0) },
        writing: { ChatLiveActivities.shared.update($0, phase: .writing) },
        end: { ChatLiveActivities.shared.end($0, finished: $1) })
}

/// The chat answers shown as Live Activities: one for each answer arriving.
/// While the app is in front the system does not show its own activity in the
/// Dynamic Island, so the activity is only seen once the person leaves; an
/// answer that ends while they are still here takes its activity with it at once.
@MainActor final class ChatLiveActivities {
    static let shared = ChatLiveActivities()

    private var activities: [String: Activity<ChatActivityAttributes>] = [:]
    private var stops: [String: @MainActor () -> Void] = [:]

    private init() {
        // Stop in the activity ends that answer, as Stop in the composer does.
        ChatActivityStop.handler = { [weak self] id in self?.stops[id]?() }
    }

    func begin(stop: @escaping @MainActor () -> Void) -> String? {
        guard ActivityAuthorizationInfo().areActivitiesEnabled else { return nil }
        // Activities left by an earlier run of the app (it was closed while an answer arrived) go now.
        for leftover in Activity<ChatActivityAttributes>.activities where activities[leftover.attributes.answerID] == nil {
            Task { await leftover.end(nil, dismissalPolicy: .immediate) }
        }
        let id = UUID().uuidString.lowercased()
        let state = ChatActivityAttributes.ContentState(phase: .thinking, startedAt: .now)
        do {
            activities[id] = try Activity.request(attributes: ChatActivityAttributes(answerID: id),
                                                  content: ActivityContent(state: state, staleDate: .now.addingTimeInterval(5 * 60)),
                                                  pushType: nil)
            stops[id] = stop
            return id
        } catch { return nil }
    }

    func update(_ id: String, phase: ChatActivityAttributes.ContentState.Phase) {
        guard let activity = activities[id], activity.content.state.phase != phase else { return }
        var state = activity.content.state
        state.phase = phase
        Task { await activity.update(ActivityContent(state: state, staleDate: .now.addingTimeInterval(5 * 60))) }
    }

    func end(_ id: String, finished: Bool) {
        guard let activity = activities.removeValue(forKey: id) else { return }
        stops[id] = nil
        var state = activity.content.state
        state.phase = finished ? .done : .stopped
        let content = ActivityContent(state: state, staleDate: nil)
        guard UIApplication.shared.applicationState != .active else {
            // Here in the app the answer is on screen: the activity goes at once.
            Task { await activity.end(content, dismissalPolicy: .immediate) }
            return
        }
        // Away from the app the check is shown for a few seconds, as in the reference. The Dynamic
        // Island drops an activity the moment it ends, so it is updated first and ended after, with a
        // little background time so the app is not suspended in between.
        var task = UIBackgroundTaskIdentifier.invalid
        task = UIApplication.shared.beginBackgroundTask(withName: "chat-activity-end") { UIApplication.shared.endBackgroundTask(task) }
        let held = task
        Task { @MainActor in
            await activity.update(content)
            try? await Task.sleep(for: .seconds(Self.checkShown))
            await activity.end(content, dismissalPolicy: .after(.now.addingTimeInterval(4)))
            if held != .invalid { UIApplication.shared.endBackgroundTask(held) }
        }
    }

    /// How long the check stays in the Dynamic Island after the answer ends.
    static let checkShown: Double = 3
}
