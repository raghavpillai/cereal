import Foundation
import Testing
import Contracts
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
        #expect(ServerHomeView.emptyDay(today: false, hadEvents: false) == "No events on this day")
    }

    @Test func aSharedMeridiemIsSaidOnce() throws {
        func event(_ start: String, _ end: String) throws -> CalendarEventDTO {
            let json = ##"{"id":"e","calendarId":"c","calendarName":"Work","title":"Sync","startAt":"\##(start)","endAt":"\##(end)","startDate":"","endDate":"","allDay":false,"color":"#87E5E8","status":"confirmed"}"##
            return try JSONDecoder().decode(CalendarEventDTO.self, from: Data(json.utf8))
        }
        let us = Locale(identifier: "en_US"), utc = TimeZone(identifier: "UTC")!
        let same = ServerHomeView.eventTimePieces(try event("2026-11-17T10:30:00Z", "2026-11-17T11:00:00Z"), locale: us, timeZone: utc)
        #expect(same.map(\.text) == ["10:30 – 11:00", "AM"] && same.map(\.meridiem) == [false, true])
        let across = ServerHomeView.eventTimePieces(try event("2026-11-17T11:30:00Z", "2026-11-17T12:15:00Z"), locale: us, timeZone: utc)
        #expect(across.map(\.text) == ["11:30", "AM", " – 12:15", "PM"])
        // A 24-hour clock has nothing to set smaller.
        let day = ServerHomeView.eventTimePieces(try event("2026-11-17T14:00:00Z", "2026-11-17T15:00:00Z"), locale: Locale(identifier: "de_DE"), timeZone: utc)
        #expect(day.count == 1 && !day[0].meridiem)
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
