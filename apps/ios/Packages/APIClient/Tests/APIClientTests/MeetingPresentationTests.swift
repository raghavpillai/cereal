import Foundation
import Testing
import Contracts
@testable import APIClient

@Suite struct MeetingPresentationTests {
    private func person(_ changes: [String: Any] = [:]) throws -> MeetingParticipantDTO {
        var object: [String: Any] = ["id": UUID().uuidString, "email": "alex.smith@example.test", "displayName": NSNull(), "givenName": NSNull(), "familyName": NSNull(), "nameSource": "unknown", "isSelf": false, "isOrganizer": false, "isResource": false, "responseStatus": "unknown"]
        object.merge(changes) { _, value in value }
        return try JSONDecoder().decode(MeetingParticipantDTO.self, from: JSONSerialization.data(withJSONObject: object))
    }
    private func meeting(people: [MeetingParticipantDTO], organizer: MeetingParticipantDTO? = nil, allDay: Bool = false) -> MeetingContextDTO {
        MeetingContextDTO(allDay: allDay, calendarName: "Test calendar", endAt: "2026-11-27T00:00:00Z", eventId: UUID().uuidString, eventUrl: nil, location: nil, meetingUrl: nil, organizer: organizer, participants: people, provider: .google, startAt: "2026-11-26T00:00:00Z", timeZone: "America/Los_Angeles", title: "Fictional meeting")
    }
    @Test func participantLabelsNeverGuessNamesFromEmailOrAssignVoiceIdentity() throws {
        #expect(MeetingPresentation.name(try person()) == "alex.smith@example.test")
        #expect(MeetingPresentation.name(try person(["displayName": "Zoë 🦊", "nameSource": "calendar"])) == "Zoë 🦊")
        #expect(MeetingPresentation.name(try person(["givenName": "Alex", "familyName": "Ng", "nameSource": "contacts"])) == "Alex Ng")
        #expect(MeetingPresentation.name(try person(["email": NSNull()])) == "Unknown participant")
    }
    @Test func peopleExcludeResourcesAndDoNotDuplicateExplicitOrganizerEmail() throws {
        let attendee = try person(["email": "OWNER@example.test"])
        let organizer = try person(["email": "owner@example.test", "isOrganizer": true])
        let resource = try person(["email": "room@example.test", "isResource": true])
        let people = MeetingPresentation.people(meeting(people: [attendee, resource], organizer: organizer))
        #expect(people.count == 1)
        #expect(people.first?.id == attendee.id)
    }
    @Test func adHocMetadataPreservesCanonicalCreationTimeAndAssociatedEventsOwnTheirStart() throws {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = try #require(TimeZone(identifier: "America/New_York"))
        let created = try #require(MeetingPresentation.metadataDate(createdAt: "2026-11-13T03:17:00Z", fallbackDate: "2026-11-12", meeting: nil, calendar: calendar))
        #expect(calendar.component(.day, from: created) == 12)
        #expect(calendar.component(.hour, from: created) == 22)
        #expect(calendar.component(.minute, from: created) == 17)
        let attached = meeting(people: [])
        #expect(MeetingPresentation.metadataDate(createdAt: "2026-11-13T03:17:00Z", fallbackDate: "2026-11-12", meeting: attached, calendar: calendar) == CalendarSchedule.timestamp(attached.startAt))
        let legacy = try #require(MeetingPresentation.metadataDate(createdAt: "invalid", fallbackDate: "2026-11-12", meeting: nil, calendar: calendar))
        #expect(calendar.component(.day, from: legacy) == 12)
    }

    @Test func allDayMetadataUsesItsDateRatherThanPreviousLocalEvening() throws {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = try #require(TimeZone(identifier: "America/Los_Angeles"))
        let start = try #require(MeetingPresentation.start(meeting(people: [], allDay: true), calendar: calendar))
        #expect(calendar.component(.day, from: start) == 26)
        #expect(calendar.component(.hour, from: start) == 0)
        #expect(MeetingPresentation.startLabel(meeting(people: []), now: .distantFuture) == nil)
    }
    @Test func importedPastDaysRemainHistoricalAndFutureAssociationsStayOnLocalCreationDay() throws {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = try #require(TimeZone(identifier: "America/New_York"))
        let created = "2026-11-13T03:17:00Z" // November12 locally.
        let historical = try #require(MeetingPresentation.timelineDate(noteDate: "2026-10-15", createdAt: created, calendar: calendar))
        let future = try #require(MeetingPresentation.timelineDate(noteDate: "2026-11-20", createdAt: created, calendar: calendar))
        #expect(calendar.component(.month, from: historical) == 10 && calendar.component(.day, from: historical) == 15)
        #expect(calendar.component(.month, from: future) == 11 && calendar.component(.day, from: future) == 12)
        #expect(MeetingPresentation.timelineDate(noteDate: "invalid", createdAt: "invalid", calendar: calendar) == nil)
    }

}
