import Foundation
import Testing
import APIClient
import Contracts
@testable import Cereal

/// A server that keeps chats: records how each question was asked and answers when told.
@MainActor final class ScriptedThreads {
    struct Asked { let create: CreateChatThreadInput; let turn: Int; let replace: Bool; let message: String }
    private(set) var asked: [Asked] = []
    private var answers: [AsyncThrowingStream<ChatStreamEventDTO, Error>.Continuation] = []

    var threadEvents: ServerChatSession.ThreadEvents {
        { [unowned self] create, turn, replace, message, _ in
            asked.append(Asked(create: create, turn: turn, replace: replace, message: message))
            return AsyncThrowingStream { self.answers.append($0) }
        }
    }
    func answer(_ text: String) {
        answers.last?.yield(ChatStreamEventDTO(text: text, type: .delta))
        answers.last?.finish()
    }
    func session(in store: ServerChatSessions, note: String? = "11111111-1111-4111-8111-111111111111") -> ServerChatSession {
        let key = ChatHistoryScope.key(accountIdentity: "server|A", noteID: note)!
        return store.session(for: key, accountIdentity: "server|A", noteID: note, events: { _, _, _, _ in
            Issue.record("A server that keeps chats is asked through its threads")
            return AsyncThrowingStream { $0.finish() }
        }, threadEvents: threadEvents, currentAccount: { "server|A" })
    }
}

private func message(_ turn: Int, _ role: ChatMessageDTORole, _ content: String, _ status: ChatThreadMessageStatus = .complete, error: String? = nil) -> ChatThreadMessageDTO {
    ChatThreadMessageDTO(citations: [], content: content, createdAt: "2026-11-01T10:00:00.000Z", elapsedMs: nil, error: error, id: UUID().uuidString.lowercased(),
                         model: nil, reasoning: [], role: role, status: status, tools: [], turn: turn, updatedAt: "2026-11-01T10:00:00.000Z")
}

@MainActor struct ServerChatThreadsTests {
    @Test func noChatHistoryFileIsReadOrWritten() {
        #expect(ServerChatSessions.shared.persistenceError == nil)
        let sessions = ServerChatSessions()
        sessions.save(messages: [ChatMessage(role: .user, text: "Q")], history: [], for: "server|A|workspace")
        #expect(sessions.persistenceError == nil)
    }

    @Test func eachQuestionIsAskedAsTheNextExchangeOfOneThreadThatCarriesWhatCameBefore() async throws {
        let server = ScriptedThreads(); let store = ServerChatSessions()
        let session = server.session(in: store)
        #expect(session.threadID == nil)
        #expect(session.ask("When do we ship?", model: nil))
        let id = try #require(session.threadID)
        server.answer("Monday.")
        try await waitFor("first answer") { !session.busy }
        #expect(session.ask("Who decided?", model: nil))
        server.answer("Priya.")
        try await waitFor("second answer") { !session.busy }
        #expect(server.asked.map(\.turn) == [0, 1])
        #expect(server.asked.allSatisfy { $0.create.id == id && $0.create.noteId == "11111111-1111-4111-8111-111111111111" && !$0.replace })
        // The thread is created empty: what was said is the server's, not sent again from here.
        #expect(server.asked.allSatisfy { $0.create.title == nil })
        #expect(store.load(for: session.key).threadID == id)
    }

    @Test func aRetryReplacesItsExchangeAndANewChatIsANewThread() async throws {
        let server = ScriptedThreads(); let store = ServerChatSessions()
        let session = server.session(in: store)
        session.ask("First", model: nil); server.answer("One")
        try await waitFor("answer") { !session.busy }
        let first = try #require(session.threadID)
        session.retryLastQuestion(model: nil); server.answer("One again")
        try await waitFor("retry") { !session.busy }
        #expect(server.asked.last?.turn == 0)
        #expect(server.asked.last?.replace == true)
        session.newConversation()
        #expect(session.threadID == nil)
        session.ask("Something else", model: nil); server.answer("Else")
        try await waitFor("new chat") { !session.busy }
        #expect(session.threadID != first)
        #expect(server.asked.last?.turn == 0 && server.asked.last?.replace == false)
        // The earlier conversation keeps its thread in the history and continues it when chosen again.
        let earlier = try #require(session.conversationsForHistory.first { $0.threadID == first })
        session.select(earlier)
        #expect(session.threadID == first)
    }

    @Test func aSavedThreadFromAnotherDeviceOpensAndContinuesAtItsNextExchange() async throws {
        let server = ScriptedThreads(); let store = ServerChatSessions()
        let session = server.session(in: store, note: nil)
        let thread = ChatThreadDTO(createdAt: "2026-11-01T10:00:00.000Z", id: "22222222-2222-4222-8222-222222222222", messages: [
            message(0, .user, "Asked on the Mac"), message(0, .assistant, "Answered on the Mac"),
            message(1, .user, "Cut off"), message(1, .assistant, "Half", .interrupted),
            message(2, .user, "Broken"), message(2, .assistant, "", .failed, error: "Chat could not finish. Please retry."),
        ], noteId: nil, title: "Mac thread", titleSource: .generated, turnCount: 3, updatedAt: "2026-11-01T10:00:00.000Z")
        session.open(thread)
        #expect(session.threadID == "22222222-2222-4222-8222-222222222222")
        #expect(session.messages.map(\.text) == ["Asked on the Mac", "Answered on the Mac", "Cut off", "Half", "Broken", "Chat could not finish. Please retry."])
        #expect(session.messages.map(\.isInterrupted) == [false, false, false, true, false, true])
        session.ask("Continued on the iPhone", model: nil); server.answer("Yes")
        try await waitFor("answer") { !session.busy }
        #expect(server.asked.last?.turn == 3)
        #expect(server.asked.last?.create.id == "22222222-2222-4222-8222-222222222222")
    }

    @Test func untitledThreadsSaySo() {
        let summary = ChatThreadSummaryDTO(createdAt: "2026-11-01T10:00:00.000Z", id: UUID().uuidString, noteId: nil, title: nil, titleSource: nil, turnCount: 1, updatedAt: "2026-11-01T10:00:00.000Z")
        #expect(ChatThreadTurns.title(summary) == "Untitled thread")
    }

    @Test func emptyUnnamedThreadsAreNotListed() {
        func summary(title: String?, turns: Int) -> ChatThreadSummaryDTO {
            ChatThreadSummaryDTO(createdAt: "2026-11-01T10:00:00.000Z", id: UUID().uuidString, noteId: nil, title: title, titleSource: nil, turnCount: turns, updatedAt: "2026-11-01T10:00:00.000Z")
        }
        #expect(ChatThreadTurns.isEmpty(summary(title: nil, turns: 0)))
        #expect(ChatThreadTurns.isEmpty(summary(title: "  ", turns: 0)))
        #expect(!ChatThreadTurns.isEmpty(summary(title: nil, turns: 1)))
        #expect(!ChatThreadTurns.isEmpty(summary(title: "Budget", turns: 0)))
    }

    @Test func threadAgesAreCompact() {
        let now = Date(timeIntervalSince1970: 1_800_000_000)
        func age(_ seconds: TimeInterval) -> String { ChatThreadTurns.age(of: now.addingTimeInterval(-seconds), now: now) }
        #expect(age(20) == "now")
        #expect(age(5 * 60 + 40) == "5m")
        #expect(age(6 * 3600 + 120) == "6h")
        #expect(age(26 * 3600) == "1d")
        #expect(age(-30) == "now")
        #expect(ChatThreadTurns.spokenAge(of: now.addingTimeInterval(-60), now: now) == "1 minute ago")
    }
}
