import Foundation
import Testing
import Contracts
@testable import APIClient

@Suite struct CalendarScheduleTests {
    private func event(_ id: String, start: String, end: String, allDay: Bool = false, startDate: String = "2026-03-08", endDate: String = "2026-03-09", calendar: String = "work") -> CalendarEventDTO {
        CalendarEventDTO(allDay: allDay, calendarId: calendar, calendarName: calendar, color: "#87e5e8", endAt: end, endDate: endDate, id: id, location: nil, meetingUrl: nil, noteId: nil, startAt: start, startDate: startDate, timeZone: "America/Los_Angeles", title: "Meeting \(id)")
    }

    @Test func onlyAnExplicitActiveTimedEventCanRequestCapture() throws {
        let now = try #require(CalendarSchedule.timestamp("2026-03-08T10:00:00Z"))
        let active = event("active", start: "2026-03-08T10:00:00Z", end: "2026-03-08T11:00:00Z")
        #expect(CalendarSchedule.isActiveTimedEvent(active, now: now))
        #expect(!CalendarSchedule.isActiveTimedEvent(active, now: now.addingTimeInterval(-1)))
        #expect(!CalendarSchedule.isActiveTimedEvent(active, now: now.addingTimeInterval(3600)))
        #expect(!CalendarSchedule.isActiveTimedEvent(event("all-day", start: active.startAt, end: active.endAt, allDay: true), now: now))
    }

    @Test func springForwardDayUsesLocalInstantsAndExclusiveAllDayDates() throws {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = try #require(TimeZone(identifier: "America/Los_Angeles"))
        let day = try #require(CalendarSchedule.timestamp("2026-03-08T08:00:00Z"))
        let now = try #require(CalendarSchedule.timestamp("2026-03-07T08:00:00Z"))
        let input = [
            event("before", start: "2026-03-08T07:00:00Z", end: "2026-03-08T08:00:00Z"),
            event("late", start: "2026-03-09T06:30:00Z", end: "2026-03-09T07:00:00Z"),
            event("tomorrow", start: "2026-03-09T07:00:00Z", end: "2026-03-09T08:00:00Z"),
            event("all-day", start: "2026-03-08T00:00:00Z", end: "2026-03-09T00:00:00Z", allDay: true)
        ]
        #expect(CalendarSchedule.events(input, on: day, calendar: calendar, now: now).map(\.id) == ["all-day", "late"])
        let nextDay = try #require(calendar.date(byAdding: .day, value: 1, to: day))
        #expect(nextDay.timeIntervalSince(day) == 23 * 3600)
        #expect(CalendarSchedule.events(input, on: nextDay, calendar: calendar, now: now).map(\.id) == ["tomorrow"])
    }

    @Test func overlappingSourcesStayDistinctAndFinishedTodayEventsDisappear() throws {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = try #require(TimeZone(secondsFromGMT: 0))
        let now = try #require(CalendarSchedule.timestamp("2026-03-08T12:00:00Z"))
        let input = [
            event("old", start: "2026-03-08T10:00:00Z", end: "2026-03-08T11:00:00Z"),
            event("a", start: "2026-03-08T11:30:00Z", end: "2026-03-08T12:30:00Z"),
            event("b", start: "2026-03-08T11:30:00Z", end: "2026-03-08T12:30:00Z", calendar: "personal")
        ]
        #expect(CalendarSchedule.events(input, on: now, calendar: calendar, now: now).map(\.id) == ["a", "b"])
        #expect(CalendarSchedule.events(input, on: now, calendar: calendar, now: now, enabledCalendarIDs: ["personal"]).map(\.id) == ["b"])
        #expect(CalendarSchedule.events(input, on: now, calendar: calendar, now: now, enabledCalendarIDs: []).isEmpty)
    }

    @Test func personalGoogleSourcesAreNotHiddenByTheSeparateSharedICSList() throws {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = try #require(TimeZone(secondsFromGMT: 0))
        let day = try #require(CalendarSchedule.timestamp("2026-03-08T00:00:00Z"))
        let input = [event("shared-disabled", start: "2026-03-08T10:00:00Z", end: "2026-03-08T11:00:00Z", calendar: "shared"), event("personal-selected", start: "2026-03-08T10:00:00Z", end: "2026-03-08T11:00:00Z", calendar: "google-personal")]
        #expect(CalendarSchedule.events(input, on: day, calendar: calendar, now: day, enabledCalendarIDs: [], knownCalendarIDs: ["shared"]).map(\.id) == ["personal-selected"])
    }

    @Test func timedEventsSortByInstantInsteadOfOffsetSpelling() throws {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = try #require(TimeZone(secondsFromGMT: 0))
        let day = try #require(CalendarSchedule.timestamp("2026-03-08T00:00:00Z"))
        let input = [
            event("later", start: "2026-03-08T10:00:00Z", end: "2026-03-08T11:00:00Z"),
            event("earlier", start: "2026-03-08T11:00:00+02:00", end: "2026-03-08T12:00:00+02:00")
        ]
        #expect(CalendarSchedule.events(input, on: day, calendar: calendar, now: day).map(\.id) == ["earlier", "later"])
    }

    @Test func meetingLinksRejectExecutableSchemesAndEmbeddedCredentials() {
        #expect(CalendarSchedule.meetingURL("https://meet.example/room?id=42")?.host == "meet.example")
        #expect(CalendarSchedule.meetingURL("javascript:alert(1)") == nil)
        #expect(CalendarSchedule.meetingURL("https://secret@meet.example/room") == nil)
        #expect(CalendarSchedule.meetingURL("file:///private/data") == nil)
    }
}
