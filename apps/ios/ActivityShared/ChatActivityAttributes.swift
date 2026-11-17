import ActivityKit
import AppIntents
import Foundation

/// A chat answer still arriving, shown as a Live Activity while the person is
/// elsewhere, as the reference does ("Working on it…", "Finishing reply", then a
/// check). Nothing of the conversation enters ActivityKit: not the question, the
/// answer, the note's title or the account. `answerID` is a random identifier
/// that only this app's process can map back to its conversation.
struct ChatActivityAttributes: ActivityAttributes {
    struct ContentState: Codable, Hashable {
        enum Phase: String, Codable, Hashable {
            /// Nothing of the answer yet.
            case thinking
            /// The answer is arriving.
            case writing
            /// The answer is complete.
            case done
            /// It was stopped or failed; what arrived is kept in the chat.
            case stopped
        }
        var phase: Phase
        /// When the question went out; the ring turns from here.
        var startedAt: Date

        var title: String {
            switch phase {
            case .thinking: "Working on it…"
            case .writing: "Finishing reply"
            case .done: "Reply ready"
            case .stopped: "Reply stopped"
            }
        }

        var subtitle: String {
            switch phase {
            case .thinking: "Cereal is preparing a response"
            case .writing: "Cereal is working on the reply"
            case .done: "Open Cereal to read it"
            case .stopped: "Open Cereal to see what arrived"
            }
        }

        var isRunning: Bool { phase == .thinking || phase == .writing }
    }

    var answerID: String

    /// `cereal://chat-answer` opens the app; the chat that was open is where it was left.
    static let openURL = URL(string: "cereal://chat-answer")!
}

/// Stop, from the Live Activity. It runs in the app's process, which ends the answer.
struct StopChatAnswerIntent: LiveActivityIntent {
    static let title: LocalizedStringResource = "Stop answer"
    static let isDiscoverable = false

    @Parameter(title: "Answer") var answerID: String

    init() {}
    init(answerID: String) { self.answerID = answerID }

    func perform() async throws -> some IntentResult {
        let id = answerID
        await MainActor.run { ChatActivityStop.handler?(id) }
        return .result()
    }
}

/// Set by the app at launch. The widget extension never runs the intent, so it stays nil there.
@MainActor enum ChatActivityStop {
    static var handler: ((String) -> Void)?
}
