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

    /// The loader starts and ends each loop as the still mark, so the wrap has no
    /// seam, and its flakes stay inside the canvas it is drawn in.
    @Test func theLoaderRestsAsTheMarkAndStaysInsideItsCanvas() {
        for end in [0.0, 1.0] {
            let pose = CerealLoader.pose(end)
            #expect(abs(pose.turn - end) < 1e-9 && abs(pose.scale - 1) < 1e-9)
            for (index, flake) in pose.flakes.enumerated() {
                let rest = CerealLoader.flakes[index].center
                #expect(abs(flake.center.x - rest.x) < 1e-6 && abs(flake.center.y - rest.y) < 1e-6 && abs(flake.scale - 1) < 1e-9)
            }
        }
        // The last frame before the wrap is where the first frame after it is.
        let before = CerealLoader.pose(1 - 1 / (CerealLoader.loop * 120)), after = CerealLoader.pose(0)
        #expect(abs(before.turn - 1) < 1e-4 && abs(before.scale - after.scale) < 1e-4)
        for index in before.flakes.indices {
            #expect(abs(before.flakes[index].center.x - after.flakes[index].center.x) < 1e-2)
            #expect(abs(before.flakes[index].center.y - after.flakes[index].center.y) < 1e-2)
        }
        var swungPast = false
        var previous = CerealLoader.pose(0)
        for tick in 0...1000 {
            let pose = CerealLoader.pose(Double(tick) / 1000)
            swungPast = swungPast || pose.turn > 1.01
            // No jump from one moment to the next: under a twentieth of a turn per thousandth of a loop.
            #expect(abs(pose.turn - previous.turn) < 0.05)
            for (index, flake) in pose.flakes.enumerated() {
                let radius = CerealLoader.flakes[index].radius * flake.scale
                #expect(flake.center.x - radius > -CerealLoader.margin && flake.center.x + radius < 24 + CerealLoader.margin)
                #expect(flake.center.y - radius > -CerealLoader.margin && flake.center.y + radius < 24 + CerealLoader.margin)
                #expect(hypot(flake.center.x - previous.flakes[index].center.x, flake.center.y - previous.flakes[index].center.y) < 2)
            }
            previous = pose
        }
        #expect(swungPast)
    }
}
