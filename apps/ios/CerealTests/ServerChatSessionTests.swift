import Testing
import Foundation
@testable import Cereal

@MainActor struct ServerChatSessionTests {
    @Test func completedCurrentConversationAppearsInHistoryWithoutStartingAnotherChat() {
        let sessions = ServerChatSessions()
        let current = [ChatMessage(role: .user, text: "What did we decide?"), ChatMessage(role: .assistant, text: "Ship on Monday.")]
        sessions.save(messages: current, history: [], for: "https://one.test|user-A|note-A")
        let saved = sessions.load(for: "https://one.test|user-A|note-A")
        #expect(saved.history.count == 1)
        #expect(saved.history.first?.messages == current)
        #expect(sessions.load(for: "https://one.test|user-B|note-A").history.isEmpty)
    }

    @Test func followupTurnsUpdateOneStableHistoryThreadAndInterruptedRepliesStayInterrupted() {
        let question = ChatMessage(role: .user, text: "First question")
        let first = [question, ChatMessage(role: .assistant, text: "First answer")]
        let archived = ServerChatSessions.archive(first, in: [])
        let followup = first + [ChatMessage(role: .user, text: "Followup"), ChatMessage(role: .assistant, text: "Partial", isStreaming: true)]
        let updated = ServerChatSessions.archive(followup, in: archived)
        #expect(updated.count == 1)
        #expect(updated.first?.id == archived.first?.id)
        let selected = ServerChatSessions.select(updated[0], current: followup, history: updated)
        #expect(selected.messages.last?.isStreaming == false)
        #expect(selected.messages.last?.isInterrupted == true)
    }

    @Test func returningToMeetingRestoresOnlyItsConversation() {
        let sessions = ServerChatSessions()
        let message = ChatMessage(role: .user, text: "Monday launch?")
        sessions.save(messages: [message], history: [], for: "https://one.test|note-A")
        #expect(sessions.load(for: "https://one.test|note-A").messages == [message])
        #expect(sessions.load(for: "https://one.test|note-B").messages.isEmpty)
        #expect(sessions.load(for: "https://two.test|note-A").messages.isEmpty)
    }

    @Test func startingNewChatArchivesWithoutMixingMeetings() {
        let sessions = ServerChatSessions()
        let messages = [ChatMessage(role: .user, text: "What changed?"), ChatMessage(role: .assistant, text: "The launch moved to Monday.")]
        let history = [ChatConversation(title: "What changed?", messages: messages)]
        sessions.save(messages: [], history: history, for: "https://one.test|note-A")
        #expect(sessions.load(for: "https://one.test|note-A").messages.isEmpty)
        #expect(sessions.load(for: "https://one.test|note-A").history.first?.messages == messages)
        #expect(sessions.load(for: "https://one.test|note-B").history.isEmpty)
    }

    @Test func protectedHistorySurvivesRelaunchWithoutRestartingPartialAnswers() throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        let url = directory.appendingPathComponent("history.json")
        let sessions = ServerChatSessions(persistenceURL: url)
        let question = ChatMessage(role: .user, text: "What changed?")
        let partial = ChatMessage(role: .assistant, text: "The launch", isStreaming: true)
        sessions.save(messages: [question, partial], history: [], for: "https://one.test|note-A")
        let restored = ServerChatSessions(persistenceURL: url)
        let messages = restored.load(for: "https://one.test|note-A").messages
        #expect(messages.count == 2)
        #expect(messages.first == question)
        #expect(messages.last?.text == partial.text)
        #expect(messages.last?.isStreaming == false)
        #expect(messages.last?.isInterrupted == true)
        #expect(restored.load(for: "https://two.test|note-A").messages.isEmpty)
        let permissions = try FileManager.default.attributesOfItem(atPath: url.path)[.posixPermissions] as? NSNumber
        #expect(permissions?.intValue == 0o600)
        #expect(restored.persistenceError == nil)
    }

    @Test func corruptHistoryIsPreservedWhenNewConversationsAreSaved() throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let url = directory.appendingPathComponent("history.json")
        let original = Data("unreadable original".utf8)
        try original.write(to: url)
        let sessions = ServerChatSessions(persistenceURL: url)
        sessions.save(messages: [ChatMessage(role: .user, text: "New question")], history: [], for: "https://one.test|note-A")
        #expect(sessions.persistenceError != nil)
        #expect(try Data(contentsOf: url) == original)
        #expect(sessions.load(for: "https://one.test|note-A").messages.first?.text == "New question")
    }

    @Test func historyBoundsOldMeetingScopesAndRetainsNewestConversation() {
        let sessions = ServerChatSessions()
        for index in 0..<65 {
            sessions.save(messages: [ChatMessage(role: .user, text: "Question \(index)")], history: [], for: "https://one.test|\(index)")
        }
        #expect(sessions.load(for: "https://one.test|0").messages.isEmpty)
        #expect(sessions.load(for: "https://one.test|64").messages.first?.text == "Question 64")
    }

    @Test func switchingFromBBackToAPreservesBothThreadsAcrossRelaunch() throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        let url = directory.appendingPathComponent("history.json")
        let a = [ChatMessage(role: .user, text: "Question A"), ChatMessage(role: .assistant, text: "Answer A")]
        let b = [ChatMessage(role: .user, text: "Question B"), ChatMessage(role: .assistant, text: "Answer B")]
        let history = ServerChatSessions.archive(a, in: [])
        let selected = ServerChatSessions.select(history[0], current: b, history: history)
        #expect(selected.messages == a)
        #expect(selected.history.contains { $0.messages == b })
        let deduplicated = ServerChatSessions.archive(a, in: selected.history)
        #expect(deduplicated.filter { $0.messages == a }.count == 1)
        #expect(deduplicated.filter { $0.messages == b }.count == 1)
        let sessions = ServerChatSessions(persistenceURL: url)
        sessions.save(messages: selected.messages, history: deduplicated, for: "https://one.test|meeting")
        let reloaded = ServerChatSessions(persistenceURL: url).load(for: "https://one.test|meeting")
        #expect(reloaded.messages == a)
        #expect(reloaded.history.contains { $0.messages == b })
    }
}
