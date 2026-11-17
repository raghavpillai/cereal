import Foundation
import Testing
import APIClient
import Contracts
@testable import Cereal

/// A server that answers when the test says so.
@MainActor final class ScriptedChat {
    struct Asked { let noteID: String?; let message: String; let history: [ChatMessageDTO]; let model: String? }
    private(set) var asked: [Asked] = []
    private var answers: [AsyncThrowingStream<ChatStreamEventDTO, Error>.Continuation] = []
    var account: String? = "server|A"

    var events: ServerChatSession.Events {
        { [unowned self] noteID, message, history, model in
            asked.append(Asked(noteID: noteID, message: message, history: history, model: model))
            return AsyncThrowingStream { self.answers.append($0) }
        }
    }
    func say(_ text: String, to index: Int? = nil) {
        answers[index ?? answers.count - 1].yield(ChatStreamEventDTO(citations: nil, text: text, tool: nil, type: .delta))
    }
    func finish(_ index: Int? = nil) { answers[index ?? answers.count - 1].finish() }
    func fail(_ index: Int? = nil) { answers[index ?? answers.count - 1].finish(throwing: URLError(.networkConnectionLost)) }

    func session(in store: ServerChatSessions, note: String = "11111111-1111-4111-8111-111111111111", account: String = "server|A") -> ServerChatSession {
        let key = ChatHistoryScope.key(accountIdentity: account, noteID: note)!
        return store.session(for: key, accountIdentity: account, noteID: note, events: events, currentAccount: { [unowned self] in self.account })
    }
}

private func turns(_ history: [ChatMessageDTO]) -> [String] { history.map { ($0.role == .user ? "Q: " : "A: ") + $0.content } }

@MainActor struct ServerChatLiveSessionTests {
    @Test func anAnswerKeepsArrivingWithNoScreenAndIsCompleteOnReturn() async throws {
        let server = ScriptedChat(); let store = ServerChatSessions()
        var screen: ServerChatSession? = server.session(in: store)
        #expect(screen!.ask("What was decided?", model: nil))
        server.say("The launch is ")
        try await waitFor("first text") { screen!.messages.last?.text == "The launch is " }
        #expect(screen!.busy && screen!.messages.last?.isStreaming == true)

        // The reader leaves the chat: the screen lets go of the conversation. The store does not.
        let key = screen!.key
        screen = nil
        #expect(store.answering == [key])
        server.say("on the 27th.")
        server.finish()
        try await waitFor("the answer finished off screen") { store.answering.isEmpty }

        // Back on the chat: the same conversation, whole, and on disk as a finished answer.
        let back = server.session(in: store)
        #expect(back.messages.map(\.text) == ["What was decided?", "The launch is on the 27th."])
        #expect(!back.busy && back.messages.last?.isStreaming == false && back.messages.last?.isInterrupted == false)
        #expect(store.load(for: key).messages.last?.text == "The launch is on the 27th.")
        #expect(store.load(for: key).messages.last?.isInterrupted == false)
    }

    @Test func aQuestionAskedWhileAnAnswerArrivesWaitsAndGoesOutWhenThatAnswerEnds() async throws {
        let server = ScriptedChat(); let store = ServerChatSessions()
        let chat = server.session(in: store)
        chat.ask("What was decided?", model: nil)
        #expect(chat.ask("  And who owns it?  ", model: "anthropic:model"))
        #expect(chat.ask("When?", model: nil))
        // Nothing went to the server for them yet, and they are not part of the conversation yet.
        #expect(server.asked.map(\.message) == ["What was decided?"])
        #expect(chat.queue.map(\.text) == ["And who owns it?", "When?"])
        #expect(chat.messages.count == 2)

        // One is taken back out.
        chat.removeQueued(chat.queue[1].id)
        #expect(chat.queue.map(\.text) == ["And who owns it?"])

        server.say("The 27th."); server.finish()
        try await waitFor("the waiting question went out") { server.asked.count == 2 }
        #expect(chat.queue.isEmpty && chat.busy)
        #expect(server.asked[1].message == "And who owns it?" && server.asked[1].model == "anthropic:model")
        // It is asked with the answer before it as history.
        #expect(turns(server.asked[1].history) == ["Q: What was decided?", "A: The 27th."])
        server.say("Oliver."); server.finish()
        try await waitFor("second answer") { !chat.busy }
        #expect(chat.messages.map(\.text) == ["What was decided?", "The 27th.", "And who owns it?", "Oliver."])
    }

    @Test func aWaitingQuestionStillGoesOutWhenTheAnswerBeforeItFails() async throws {
        let server = ScriptedChat(); let store = ServerChatSessions()
        let chat = server.session(in: store)
        chat.ask("First?", model: nil); chat.ask("Second?", model: nil)
        server.say("Half an ans"); try await waitFor("text") { chat.messages.last?.text == "Half an ans" }
        server.fail()
        try await waitFor("second asked") { server.asked.count == 2 }
        #expect(chat.messages[1].isInterrupted && chat.messages[1].text == "Half an ans")
        // The partial answer the reader saw goes back to the server with its question.
        #expect(turns(server.asked[1].history) == ["Q: First?", "A: Half an ans"])
    }

    @Test func stopEndsTheAnswerKeepsWhatWasShownAndGivesTheWaitingQuestionsBack() async throws {
        let server = ScriptedChat(); let store = ServerChatSessions()
        let chat = server.session(in: store)
        chat.ask("Summarise the meeting.", model: nil)
        server.say("Three things were ")
        try await waitFor("text") { chat.messages.last?.text == "Three things were " }
        chat.ask("And the budget?", model: nil); chat.ask("And hiring?", model: nil)

        #expect(chat.stop() == ["And the budget?", "And hiring?"])
        #expect(!chat.busy && chat.queue.isEmpty)
        #expect(chat.messages.map(\.text) == ["Summarise the meeting.", "Three things were "])
        #expect(chat.messages[1].isInterrupted && !chat.messages[1].isStreaming)
        // Text that arrives after Stop is not shown, and no waiting question is sent.
        server.say("decided."); server.finish()
        try await Task.sleep(for: .milliseconds(120))
        #expect(chat.messages[1].text == "Three things were ")
        #expect(server.asked.count == 1)
        #expect(store.load(for: chat.key).messages.last?.isInterrupted == true)
    }

    @Test func anAnswerNeverLandsInAnotherAccountsOrAnotherNotesConversation() async throws {
        let server = ScriptedChat(); let store = ServerChatSessions()
        let noteA = "11111111-1111-4111-8111-111111111111", noteB = "22222222-2222-4222-8222-222222222222"
        let a = server.session(in: store, note: noteA), b = server.session(in: store, note: noteB)
        #expect(a !== b && a.key != b.key)
        a.ask("About A?", model: nil); b.ask("About B?", model: nil)
        #expect(server.asked.map(\.noteID) == [noteA, noteB])
        server.say("Answer for A.", to: 0); server.say("Answer for B.", to: 1)
        try await waitFor("both") { a.messages.last?.text == "Answer for A." && b.messages.last?.text == "Answer for B." }
        #expect(a.messages.map(\.text) == ["About A?", "Answer for A."])
        #expect(b.messages.map(\.text) == ["About B?", "Answer for B."])

        // Someone else signs in on this device while both answers are arriving.
        server.account = "server|B"
        store.accountChanged(to: "server|B")
        #expect(!a.busy && !b.busy && store.answering.isEmpty)
        server.say(" More for A.", to: 0); server.finish(0); server.say(" More for B.", to: 1); server.finish(1)
        try await Task.sleep(for: .milliseconds(120))
        #expect(a.messages.last?.text == "Answer for A." && b.messages.last?.text == "Answer for B.")
        // The new account's conversation about the same note is a different, empty one.
        let other = server.session(in: store, note: noteA, account: "server|B")
        #expect(other !== a && other.messages.isEmpty)
        // And the old account's session can no longer ask.
        #expect(!a.ask("Still there?", model: nil))
        #expect(server.asked.count == 2)
    }

    @Test func anAnswerStopsBeingAppliedTheMomentTheAccountIsNoLongerTheOneThatAsked() async throws {
        // No sign-out notice reached the store: the stream itself checks before it writes.
        let server = ScriptedChat(); let store = ServerChatSessions()
        let chat = server.session(in: store)
        chat.ask("What was decided?", model: nil)
        server.say("The launch"); try await waitFor("text") { chat.messages.last?.text == "The launch" }
        server.account = nil
        server.say(" is on the 27th."); server.finish()
        try await waitFor("ended") { !chat.busy }
        #expect(chat.messages.last?.text == "The launch")
    }

    @Test func deletingTheNoteEndsTheAnswerAboutIt() async throws {
        let server = ScriptedChat(); let store = ServerChatSessions()
        let note = "11111111-1111-4111-8111-111111111111"
        let chat = server.session(in: store, note: note)
        chat.ask("What was decided?", model: nil); chat.ask("And then?", model: nil)
        store.noteDeleted(note, accountIdentity: "server|A")
        #expect(!chat.busy && chat.queue.isEmpty && store.answering.isEmpty)
        server.say("Late text."); server.finish()
        try await Task.sleep(for: .milliseconds(120))
        #expect(server.asked.count == 1)
        #expect(chat.messages.allSatisfy { $0.text != "Late text." })
    }

    @Test func aNewChatEndsTheAnswerAndKeepsTheOldConversationInHistory() async throws {
        let server = ScriptedChat(); let store = ServerChatSessions()
        let chat = server.session(in: store)
        chat.ask("What was decided?", model: nil); chat.ask("Waiting question", model: nil)
        server.say("The 27th"); try await waitFor("text") { chat.messages.last?.text == "The 27th" }
        #expect(chat.newConversation() == ["Waiting question"])
        #expect(chat.messages.isEmpty && !chat.busy)
        #expect(chat.conversationsForHistory.first?.messages.map(\.text) == ["What was decided?", "The 27th"])
        #expect(chat.conversationsForHistory.first?.messages.last?.isInterrupted == true)
    }

    @Test func historySendsAPartialAnswerTheReaderSawAndLeavesOutAQuestionThatGotNoText() {
        let messages = [
            ChatMessage(role: .user, text: "One?"), ChatMessage(role: .assistant, text: "Answer one."),
            // Stopped before any text: the question goes too, so the thread matches at the exchange before.
            ChatMessage(role: .user, text: "Two?"),
            ChatMessage(role: .user, text: "Three?"), ChatMessage(role: .assistant, text: "Half of thr", isInterrupted: true),
            // Still arriving: not history yet.
            ChatMessage(role: .user, text: "Four?"), ChatMessage(role: .assistant, text: "Arriv", isStreaming: true),
        ]
        #expect(turns(ChatHistoryRule.history(from: messages)) == ["Q: One?", "A: Answer one.", "Q: Three?", "A: Half of thr"])
        // Whole exchanges, newest last, and never starting with an answer.
        let long = (0..<30).flatMap { [ChatMessage(role: .user, text: "Q\($0)"), ChatMessage(role: .assistant, text: "A\($0)")] }
        let sent = ChatHistoryRule.history(from: long)
        #expect(sent.count == 20 && sent.first?.role == .user && sent.first?.content == "Q20" && sent.last?.content == "A29")
        #expect(ChatHistoryRule.history(from: long, limit: 5).count == 4)
    }

    @Test func theComposerButtonQueuesWhenThereIsTextAndStopsWhenThereIsNone() {
        typealias Rule = ChatComposerRule
        #expect(Rule.action(isGenerating: false, hasText: false, queues: true) == .idle)
        #expect(Rule.action(isGenerating: false, hasText: true, queues: true) == .send)
        #expect(Rule.action(isGenerating: true, hasText: false, queues: true) == .stop)
        // A question typed while an answer arrives is sent to wait its turn.
        #expect(Rule.action(isGenerating: true, hasText: true, queues: true) == .send)
        // Where nothing can wait (the on-device preview chat), the button stays Stop.
        #expect(Rule.action(isGenerating: true, hasText: true, queues: false) == .stop)
    }
}
