import Foundation
import Testing
import Contracts
@testable import Cereal

/// The chat chrome copied from the reference's iPhone chat.
@MainActor struct ChatChromeTests {
    @Test func modelsAreNamedAsProductsNotIds() {
        #expect(ChatModelName.display(id: "claude-sonnet-5-5") == "Sonnet 5.5")
        #expect(ChatModelName.display(id: "anthropic/claude-opus-5-5-20261001") == "Opus 5.5")
        #expect(ChatModelName.display(id: "gpt-6.1") == "GPT-6.1")
        #expect(ChatModelName.display(id: "openai/gpt-6") == "GPT-6")
        #expect(ChatModelName.display(id: "google/gemini-4-pro") == "Gemini 4 Pro")
        #expect(ChatModelName.display(id: "x-ai/grok-4.7") == "Grok 4.7")
        #expect(ChatModelName.display(id: "o3") == "o3")
    }

    @Test func eachMakerHasItsOwnGlyph() {
        #expect(ChatModelName.symbol(LanguageModelSelection(model: "claude-sonnet-5-5", provider: .anthropic)) == "asterisk")
        #expect(ChatModelName.symbol(LanguageModelSelection(model: "google/gemini-4-pro", provider: .openrouter)) == "sparkle")
        #expect(ChatModelName.symbol(LanguageModelSelection(model: "x-ai/grok-4.7", provider: .openrouter)) == "slash.circle")
        #expect(ChatModelName.symbol(LanguageModelSelection(model: "gpt-6", provider: .openai)) == "hexagon")
        #expect(ChatModelName.key(LanguageModelSelection(model: "gpt-6", provider: .openai)) == "openai:gpt-6")
    }

    @Test func thePanelTakesTheMeasuredShareOfAWideWindow() {
        // An unfolded Duo: 400 of 876 pt beside the rail.
        #expect(NoteChatPanel.width(for: 876) == 400)
        #expect(NoteChatPanel.width(for: 600) == 320)
        #expect(NoteChatPanel.width(for: 1_400) == 440)
    }

    @Test func savedChatsAreGroupedByTheDayTheyWereLastUsed() {
        let calendar = Calendar(identifier: .gregorian)
        let now = calendar.date(from: DateComponents(year: 2026, month: 11, day: 17, hour: 15))!
        func day(_ offset: Int) -> Date { calendar.date(byAdding: .day, value: -offset, to: now)! }
        #expect(ChatHistorySheet.Day.of(day(0), now: now, calendar: calendar) == .today)
        #expect(ChatHistorySheet.Day.of(day(1), now: now, calendar: calendar) == .yesterday)
        #expect(ChatHistorySheet.Day.of(day(5), now: now, calendar: calendar) == .week)
        #expect(ChatHistorySheet.Day.of(day(30), now: now, calendar: calendar) == .earlier)
        #expect(ChatHistorySheet.Day.of(nil, now: now, calendar: calendar) == .earlier)
    }

    @Test func theLiveActivitySaysWhatTheAnswerIsDoing() {
        let start = Date(timeIntervalSince1970: 0)
        #expect(ChatActivityAttributes.ContentState(phase: .thinking, startedAt: start).title == "Working on it…")
        #expect(ChatActivityAttributes.ContentState(phase: .writing, startedAt: start).title == "Finishing reply")
        #expect(ChatActivityAttributes.ContentState(phase: .done, startedAt: start).isRunning == false)
        #expect(ChatActivityAttributes.ContentState(phase: .writing, startedAt: start).isRunning)
    }

    /// The session tells its Live Activity when the answer starts, when words arrive and when it ends.
    @Test func anAnswerDrivesItsLiveActivity() async throws {
        var log: [String] = []
        let hooks = ChatActivityHooks(begin: { _ in log.append("begin"); return "a1" },
                                      writing: { log.append("writing \($0)") },
                                      end: { log.append("end \($0) \($1)") })
        let session = ServerChatSession(key: "acct|note", accountIdentity: "acct", noteID: nil, state: .init(),
            events: { _, _, _, _ in
                AsyncThrowingStream { continuation in
                    continuation.yield(ChatStreamEventDTO(text: "Hello", type: .delta))
                    continuation.yield(ChatStreamEventDTO(text: "", type: .done))
                    continuation.finish()
                }
            },
            currentAccount: { "acct" }, activity: hooks) { _ in }
        #expect(session.ask("Hi", model: nil))
        for _ in 0..<100 where session.busy { try await Task.sleep(for: .milliseconds(20)) }
        #expect(log.first == "begin")
        #expect(log.contains("writing a1"))
        #expect(log.last == "end a1 true")
    }
}
