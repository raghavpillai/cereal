import Foundation
import Testing
@testable import Cereal

struct ChatTests {
    @Test func localPreviewQuotesSavedContentWithoutInventingDecisions() async {
        let note = Note(title: "Prototype review", createdAt: .now,
                        sections: [NoteSection(heading: "Decision", bullets: ["Review the blue prototype on Monday."])])
        var answer = ""
        for await chunk in LocalChatService().reply(to: "What did we decide?", context: note) { answer += chunk }
        #expect(answer.contains("Review the blue prototype on Monday."))
        #expect(answer.contains("on-device preview"))
        #expect(!answer.contains("six months"))
        #expect(!answer.contains("must-have"))
    }

    @Test func anEmptyContextDoesNotPretendToFindMeetingContent() async {
        var answer = ""
        for await chunk in LocalChatService().reply(to: "Find my action items", context: nil) { answer += chunk }
        #expect(answer.contains("don't have meeting notes"))
        #expect(!answer.contains("### Summary"))
    }

    @Test func cancellingAChatConsumerStopsThePendingStream() async {
        let note = Note(title: "Review", createdAt: .now,
                        sections: [NoteSection(heading: "Notes", bullets: ["Saved source content"])])
        let stream = LocalChatService().reply(to: "Summarize", context: note)
        let consumer = Task {
            var chunks: [String] = []
            for await chunk in stream { chunks.append(chunk) }
            return chunks
        }
        consumer.cancel()
        let chunks = await consumer.value
        #expect(chunks.isEmpty)
    }
}
