import Foundation

/// The boundary the real backend will implement (`POST /chat` streaming over SSE/WebSocket).
protocol ChatService: Sendable {
    func reply(to prompt: String, context: Note?) -> AsyncStream<String>
}

/// Explicit on-device preview. Only returns saved source text, never invented
/// commitments, decisions, or a simulated server response.
struct LocalChatService: ChatService {
    func reply(to prompt: String, context: Note?) -> AsyncStream<String> {
        let answer = Self.compose(prompt: prompt, note: context)
        return AsyncStream { continuation in
            let task = Task {
                try? await Task.sleep(for: .milliseconds(250))
                guard !Task.isCancelled else { continuation.finish(); return }
                for word in answer.split(separator: " ", omittingEmptySubsequences: false) {
                    if Task.isCancelled { break }
                    continuation.yield(String(word) + " ")
                    try? await Task.sleep(for: .milliseconds(28))
                }
                continuation.finish()
            }
            continuation.onTermination = { _ in task.cancel() }
        }
    }

    private static func compose(prompt: String, note: Note?) -> String {
        guard let note, !note.sections.isEmpty else {
            return "I don't have meeting notes to draw on yet. Record a meeting, or open a note and ask from there."
        }
        var out = "### Saved note excerpt\n"
        for s in note.sections.prefix(2) {
            out += "\n**\(s.heading)**\n" + s.bullets.prefix(3).map { "- \($0)" }.joined(separator: "\n") + "\n"
        }
        return out
    }
}
