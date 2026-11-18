// ActivityKit's Activity reference predates Sendable annotations; every call is made on the main actor.
@preconcurrency import ActivityKit
import Foundation

/// The Lock Screen's "Take a note" card for each of today's meetings, as the
/// reference shows one when a meeting starts. Nothing reaches this app in the
/// background, so each card is handed to the system ahead of time with the
/// moment it should appear (iOS 26 starts it by itself); a meeting already
/// under way gets its card at once.
@MainActor enum MeetingLiveActivities {
    struct Event: Equatable {
        var account: String
        var id: String
        var title: String
        var start: Date
        var end: Date
        var color: String?
        var key: String { MeetingActivityAttributes.key(account: account, eventID: id) }
        /// One showing of one event: an event that moves is shown again at its new time.
        var stamp: String { "\(key)@\(Int(start.timeIntervalSince1970))" }
    }

    /// A card the system holds, waiting or showing.
    struct Live: Equatable {
        var key: String
        var title: String
        var start: Date
        var end: Date
    }

    struct Plan: Equatable {
        /// Cards whose meeting is over, gone, or moved, and second copies: places in `live`.
        var end: [Int] = []
        /// Cards whose meeting was renamed.
        var update: [Event] = []
        var request: [Event] = []
    }

    /// The QA sample's account. Its cards are left alone by what Home reads.
    static let previewAccount = "preview"
    static let storageKey = "liveActivities.upcomingMeetings"
    private static let shownKey = "liveActivities.upcomingMeetings.shown"
    /// How long before the meeting's start its card appears.
    static let lead: TimeInterval = 60
    /// The system holds only so many activities for one app; the next few meetings are enough.
    static let limit = 3

    /// On unless turned off in Settings › Notifications.
    static var isEnabled: Bool { UserDefaults.standard.object(forKey: storageKey) as? Bool ?? true }

    /// What to do with the cards, given today's events. Pure, so the rules can be tested.
    /// `shown` are the stamps already asked for: a card the person swiped away is not put back.
    static func plan(events: [Event], live: [Live], shown: Set<String>, now: Date, limit: Int = limit) -> Plan {
        let wanted = events.filter { $0.end > now && $0.end > $0.start }
            .sorted { $0.start == $1.start ? $0.key < $1.key : $0.start < $1.start }
            .prefix(limit)
        var plan = Plan()
        var kept = Set<String>()
        for (index, card) in live.enumerated() {
            guard let event = wanted.first(where: { $0.key == card.key }), event.start == card.start, event.end == card.end, !kept.contains(card.key) else {
                plan.end.append(index)
                continue
            }
            kept.insert(card.key)
            if event.title != card.title { plan.update.append(event) }
        }
        plan.request = wanted.filter { !kept.contains($0.key) && !shown.contains($0.stamp) }
        return plan
    }

    /// Brings the system's cards in line with today's events.
    static func sync(_ events: [Event], now: Date = .now) async {
        guard isEnabled, ActivityAuthorizationInfo().areActivitiesEnabled else { await endAll(); return }
        let activities = Activity<MeetingActivityAttributes>.activities.filter { $0.activityState != .ended && $0.activityState != .dismissed && $0.attributes.account != previewAccount }
        let live = activities.map { Live(key: $0.attributes.key, title: $0.content.state.title, start: $0.content.state.start, end: $0.content.state.end) }
        var shown = shownStamps(now: now)
        let plan = plan(events: events, live: live, shown: Set(shown.keys), now: now)
        for index in plan.end { await activities[index].end(nil, dismissalPolicy: .immediate) }
        for event in plan.update {
            guard let index = activities.indices.first(where: { !plan.end.contains($0) && activities[$0].attributes.key == event.key }) else { continue }
            await activities[index].update(content(event))
        }
        for event in plan.request {
            let attributes = MeetingActivityAttributes(account: event.account, eventID: event.id, color: event.color)
            let appears = event.start.addingTimeInterval(-lead)
            do {
                if appears > now.addingTimeInterval(5) {
                    let title = content(event).state.shownTitle
                    _ = try Activity.request(attributes: attributes, content: content(event), pushType: nil, style: .standard,
                                             alertConfiguration: AlertConfiguration(title: "\(title)", body: "Starting now. Take a note in Cereal.", sound: .default),
                                             start: appears)
                } else {
                    _ = try Activity.request(attributes: attributes, content: content(event), pushType: nil)
                }
                shown[event.stamp] = event.start.timeIntervalSince1970
            } catch { /* The system has no room, or activities were turned off: the next read tries again. */ }
        }
        UserDefaults.standard.set(shown, forKey: shownKey)
    }

    /// The person is taking this meeting's note: its card has done its work.
    static func end(account: String, eventID: String) {
        let key = MeetingActivityAttributes.key(account: account, eventID: eventID)
        for activity in Activity<MeetingActivityAttributes>.activities where activity.attributes.key == key {
            Task { await activity.end(nil, dismissalPolicy: .immediate) }
        }
    }

    static func endAll() async {
        for activity in Activity<MeetingActivityAttributes>.activities { await activity.end(nil, dismissalPolicy: .immediate) }
    }

    /// Stale at the meeting's end: the system dims a card nobody came back to end.
    private static func content(_ event: Event) -> ActivityContent<MeetingActivityAttributes.ContentState> {
        ActivityContent(state: .init(title: event.title, start: event.start, end: event.end), staleDate: event.end)
    }

    /// Stamps asked for in the last two days; older ones are forgotten.
    private static func shownStamps(now: Date) -> [String: Double] {
        let stored = UserDefaults.standard.dictionary(forKey: shownKey) as? [String: Double] ?? [:]
        return stored.filter { $0.value > now.timeIntervalSince1970 - 2 * 86_400 }
    }
}
