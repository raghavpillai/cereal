import Foundation
import Testing
@testable import Cereal

@MainActor struct MeetingLiveActivityTests {
    typealias Cards = MeetingLiveActivities
    let now = Date(timeIntervalSince1970: 1_800_000_000)

    func event(_ id: String, in minutes: Double, title: String = "Weekly") -> Cards.Event {
        let start = now.addingTimeInterval(minutes * 60)
        return Cards.Event(account: "a", id: id, title: title, start: start, end: start.addingTimeInterval(1800), color: nil)
    }

    func live(_ event: Cards.Event) -> Cards.Live { Cards.Live(key: event.key, title: event.title, start: event.start, end: event.end) }

    @Test func theNextMeetingsGetCardsAndFinishedOnesDoNot() {
        let events = [event("late", in: 300), event("over", in: -90), event("running", in: -10), event("soon", in: 20), event("later", in: 120)]
        let plan = Cards.plan(events: events, live: [], shown: [], now: now)
        #expect(plan.request.map(\.id) == ["running", "soon", "later"])
        #expect(plan.end.isEmpty && plan.update.isEmpty)
    }

    @Test func aCardAlreadyHeldIsNotAskedForAgainAndARenameUpdatesIt() {
        let soon = event("soon", in: 20)
        var renamed = soon; renamed.title = "Weekly sync"
        #expect(Cards.plan(events: [soon], live: [live(soon)], shown: [soon.stamp], now: now) == Cards.Plan())
        let plan = Cards.plan(events: [renamed], live: [live(soon)], shown: [soon.stamp], now: now)
        #expect(plan.update == [renamed] && plan.request.isEmpty && plan.end.isEmpty)
    }

    @Test func aCardSwipedAwayIsNotPutBack() {
        let soon = event("soon", in: 20)
        #expect(Cards.plan(events: [soon], live: [], shown: [soon.stamp], now: now).request.isEmpty)
    }

    @Test func aMovedMeetingLosesItsCardAndGetsOneAtTheNewTime() {
        let old = event("soon", in: 20), moved = event("soon", in: 45)
        let plan = Cards.plan(events: [moved], live: [live(old)], shown: [old.stamp], now: now)
        #expect(plan.end == [0])
        #expect(plan.request == [moved])
    }

    @Test func cardsOfMeetingsThatEndedOrWentAreEndedAndSoAreSecondCopies() {
        let soon = event("soon", in: 20), gone = event("gone", in: 30), over = event("over", in: -90)
        let plan = Cards.plan(events: [soon, over], live: [live(soon), live(gone), live(over), live(soon)], shown: [], now: now)
        #expect(plan.end == [1, 2, 3])
        #expect(plan.request.isEmpty)
    }

    @Test func theTakeANoteLinkNamesItsAccountAndEvent() throws {
        let attributes = MeetingActivityAttributes(account: "abc", eventID: "ev/1 x", color: "#7A8F3A")
        let target = try #require(MeetingActivityAttributes.target(attributes.takeNoteURL))
        #expect(target.account == "abc" && target.eventID == "ev/1 x")
        #expect(MeetingActivityAttributes.target(URL(string: "cereal://meeting?account=abc")!) == nil)
        #expect(MeetingActivityAttributes.target(URL(string: "cereal://chat-answer")!) == nil)
        #expect(MeetingActivityAttributes.rgb("#FF0080")?.red == 1)
        #expect(MeetingActivityAttributes.rgb("blue") == nil)
    }

    @Test func timesAreWrittenAsOnHome() {
        let zone = TimeZone(identifier: "UTC")!
        let start = Date(timeIntervalSince1970: 1_800_000_000 - 1_800_000_000.truncatingRemainder(dividingBy: 86_400) + 15.5 * 3600)
        let state = MeetingActivityAttributes.ContentState(title: " \n", start: start, end: start.addingTimeInterval(3600))
        #expect(state.timeRange(timeZone: zone) == "15:30 – 16:30")
        #expect(state.startTime(timeZone: zone) == "15:30")
        #expect(state.shownTitle == "Meeting")
    }
}
