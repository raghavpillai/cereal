import Foundation
import Testing
import Contracts
import APIClient
@testable import Cereal

/// The desktop's behaviour of 17 Nov, as the iPhone keeps it.
@MainActor struct ParityTests {
    private func tool(_ id: String, _ name: Name, _ status: ChatToolProgressDTOStatus = .completed, label: String = "Searched meetings") -> ChatToolProgressDTO {
        ChatToolProgressDTO(callId: id, label: label, name: name, resultCount: nil, results: nil, status: status)
    }

    @Test func theFoldSaysWorkedWhenTheTurnSearchedAndThoughtOtherwise() {
        var read = ChatTurnAnnotations(saved: [], elapsedMs: 2_900)
        read.apply(ChatStreamEventDTO(text: "", tool: tool("a", .getMeetingTranscript), type: .tool))
        #expect(ChatThoughts.status(read, thinking: false, stopped: false) == "Thought for 2s")
        var searched = ChatTurnAnnotations(saved: [], elapsedMs: 6_400)
        searched.apply(ChatStreamEventDTO(text: "", tool: tool("b", .searchMeetings), type: .tool))
        #expect(ChatThoughts.status(searched, thinking: false, stopped: false) == "Worked for 6s")
        // Two reads of transcripts are work too.
        var twice = read
        twice.apply(ChatStreamEventDTO(text: "", tool: tool("c", .getMeetingTranscript), type: .tool))
        #expect(ChatThoughts.worked(twice))
        // No tools at all, a quick answer: at least a second.
        #expect(ChatThoughts.status(ChatTurnAnnotations(saved: [], elapsedMs: 300), thinking: false, stopped: false) == "Thought for 1s")
        #expect(ChatThoughts.status(searched, thinking: false, stopped: true) == "Stopped")
        #expect(ChatThoughts.status(nil, thinking: false, stopped: false) == nil)
    }

    @Test func whileThinkingTheLineSaysWhatIsBeingDone() {
        var annotations = ChatTurnAnnotations()
        #expect(ChatThoughts.status(annotations, thinking: true, stopped: false) == "Thinking")
        annotations.addReasoning("**Finding the meeting**\n\nI will search")
        // A heading is shown once its line is whole.
        #expect(ChatThoughts.liveHeading(annotations) == "Finding the meeting…")
        annotations.apply(ChatStreamEventDTO(text: "", tool: tool("a", .listMeetings, .running, label: "Listing meetings."), type: .tool))
        #expect(ChatThoughts.status(annotations, thinking: true, stopped: false) == "Listing meetings…")
        annotations.addReasoning("Now read both.")
        #expect(ChatThoughts.items(annotations) == [.reasoning(heading: "Finding the meeting", body: "I will search"), .step(0), .reasoning(heading: nil, body: "Now read both.")])
    }

    @Test func theCalendarSaysWhetherTodayHadEvents() {
        #expect(ServerHomeView.emptyDay(today: true, hadEvents: false) == "No events today")
        #expect(ServerHomeView.emptyDay(today: true, hadEvents: true) == "No more events today")
        #expect(ServerHomeView.emptyDay(today: false, hadEvents: false) == "No events")
    }

    @Test func anEventSaysItsTimesOnTheDevicesClockAndWhoIsInvited() throws {
        func event(_ start: String, _ end: String, attendees: Int? = nil, allDay: Bool = false) throws -> CalendarEventDTO {
            let count = attendees.map { ",\"attendeeCount\":\($0)" } ?? ""
            let json = ##"{"id":"e","calendarId":"c","calendarName":"Work","title":"Sync","startAt":"\##(start)","endAt":"\##(end)","startDate":"","endDate":"","allDay":\##(allDay),"color":"#87E5E8","status":"confirmed"\##(count)}"##
            return try JSONDecoder().decode(CalendarEventDTO.self, from: Data(json.utf8))
        }
        let utc = TimeZone(identifier: "UTC")!
        // A device on the 24-hour clock: "15:30 – 16:30 · 14 attendees".
        let day = Locale(identifier: "en_GB")
        #expect(ServerHomeView.eventSubtitle(try event("2026-11-18T15:30:00Z", "2026-11-18T16:30:00Z", attendees: 14), timeZone: utc, locale: day) == "15:30 – 16:30 · 14 attendees")
        // The locale writes its own morning ("09:05" or "9:05"); there is no AM or PM on this clock.
        let morning = ServerHomeView.eventSubtitle(try event("2026-11-18T09:05:00Z", "2026-11-18T09:35:00Z"), timeZone: utc, locale: day)
        #expect(morning.contains("9:05 – ") && morning.hasSuffix("9:35") && !morning.uppercased().contains("AM"))
        // Only the account holder: no count.
        #expect(ServerHomeView.eventSubtitle(try event("2026-11-18T09:05:00Z", "2026-11-18T09:35:00Z", attendees: 1), timeZone: utc, locale: day) == morning)
        // A device on the 12-hour clock gets the same clock as the note rows under the events.
        let twelve = ServerHomeView.eventSubtitle(try event("2026-11-18T15:30:00Z", "2026-11-18T16:30:00Z"), timeZone: utc, locale: Locale(identifier: "en_US"))
        #expect(twelve.contains("3:30") && twelve.contains("4:30") && twelve.contains("PM") && !twelve.contains("15:30"))
        #expect(ServerHomeView.eventSubtitle(try event("2026-11-18T00:00:00Z", "2026-11-19T00:00:00Z", attendees: 3, allDay: true), timeZone: utc) == "All day · 3 attendees")
    }

    @Test func homeDaysAreWrittenAsInTheReference() throws {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = TimeZone(identifier: "UTC")!
        let wednesday = try #require(Date.serverTimestamp("2026-11-18T12:00:00Z"))
        #expect(HomeDayTitle.string(wednesday, calendar: calendar, locale: Locale(identifier: "en_US")) == "Wed, 18 Nov")
        let now = try #require(Date.serverTimestamp("2026-11-17T12:00:00Z"))
        let friday = try #require(Date.serverTimestamp("2026-11-13T00:00:00Z"))
        #expect(ServerHomeGrouping.title(for: friday, now: now, calendar: calendar) == "Fri 13 Nov")
    }

    @Test func aNoteMadeAheadOfItsMeetingWaitsOnThatDayAsADraft() throws {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = TimeZone(identifier: "UTC")!
        let now = try #require(Date.serverTimestamp("2026-11-17T16:00:00Z"))
        let draft = ServerNoteRow(id: "d", title: "Design review", date: "2026-11-18", createdAt: "2026-11-17T15:50:00Z", updatedAt: "2026-11-17T15:50:00Z",
                                  attendeeNames: ["Sam"], attendeeTotal: 14)
        let recorded = ServerNoteRow(id: "r", title: "Planning", date: "2026-11-19", createdAt: "2026-11-17T10:00:00Z", updatedAt: "2026-11-17T10:00:00Z",
                                     transcriptRevision: 2)
        let today = ServerNoteRow(id: "t", title: "Standup", date: "2026-11-17", createdAt: "2026-11-17T09:00:00Z", updatedAt: "2026-11-17T09:00:00Z")
        let sections = ServerHomeGrouping.sections([draft, recorded, today], now: now, calendar: calendar)
        #expect(sections.map(\.title) == ["Wed 18 Nov", "Earlier today"])
        #expect(sections[0].rows.map(\.id) == ["d"] && sections[0].rows[0].isDraft)
        #expect(sections[0].rows[0].subtitle == "14 attendees")
        // Something recorded keeps the day it was made, and is an ordinary row.
        #expect(sections[1].rows.map(\.id) == ["r", "t"] && !sections[1].rows.contains(where: \.isDraft))
    }

    @Test func chatInNavigationOpensANewConversationAndKeepsTheOldOne() {
        let sessions = ServerChatSessions()
        let key = "server|A|workspace"
        sessions.save(messages: [ChatMessage(role: .user, text: "Recent todos"), ChatMessage(role: .assistant, text: "Two.")], history: [], for: key)
        sessions.freshConversation(for: key)
        #expect(sessions.load(for: key).messages.isEmpty)
        #expect(sessions.load(for: key).history.first?.title == "Recent todos")
    }

    @Test func theRecordingPillShowsAShortTitle() {
        #expect(RecordingPill.shortTitle("  ") == "New note")
        #expect(RecordingPill.shortTitle("Halden pilot check-in") == "Halden pilot check-in")
        #expect(RecordingPill.shortTitle(String(repeating: "a", count: 40)).count == 24)
    }
}
