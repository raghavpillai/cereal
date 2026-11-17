import Foundation
import Contracts

/// Generated DTOs stay intact. A local revision token changes with each metadata
/// update, allowing SwiftUI/history equality without retroactive DTO conformances.
struct ChatTurnAnnotations: Codable, Sendable, Hashable {
    private var revision = UUID()
    private(set) var tools: [ChatToolProgressDTO] = []
    private(set) var citations: [ChatCitationDTO] = []
    /// The model's own reasoning, in order; each stretch says how many of `tools`
    /// had started before it. Empty when the provider gave none.
    private(set) var reasoning: [ChatReasoningDTO] = []
    /// How long the answer was thought about: from the question to its first text
    /// (or to its end, when no text came). Nil for an answer that never said.
    private(set) var elapsedMs: Int?
    var hasContent: Bool { !tools.isEmpty || !citations.isEmpty || hasThoughts }
    /// Something to show under the answer's status line: a lookup or some reasoning.
    var hasThoughts: Bool { !tools.isEmpty || reasoning.contains { !$0.text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty } }
    static func == (lhs: Self, rhs: Self) -> Bool { lhs.revision == rhs.revision }
    func hash(into hasher: inout Hasher) { hasher.combine(revision) }

    private enum CodingKeys: String, CodingKey { case revision, tools, citations, reasoning, elapsedMs }

    init() {}

    /// Saved before reasoning and timing were kept, an answer reads without them.
    init(from decoder: any Decoder) throws {
        let values = try decoder.container(keyedBy: CodingKeys.self)
        revision = try values.decodeIfPresent(UUID.self, forKey: .revision) ?? UUID()
        tools = try values.decodeIfPresent([ChatToolProgressDTO].self, forKey: .tools) ?? []
        citations = try values.decodeIfPresent([ChatCitationDTO].self, forKey: .citations) ?? []
        reasoning = try values.decodeIfPresent([ChatReasoningDTO].self, forKey: .reasoning) ?? []
        elapsedMs = try values.decodeIfPresent(Int.self, forKey: .elapsedMs)
    }

    /// A saved answer's reasoning and timing, as the server kept them.
    init(saved reasoning: [ChatReasoningDTO], elapsedMs: Int?) {
        self.reasoning = Array(reasoning.prefix(40))
        self.elapsedMs = elapsedMs
    }

    /// The first time this answer's thinking is known to have ended. Later calls change nothing.
    mutating func markThought(elapsedMs: Int) {
        guard self.elapsedMs == nil else { return }
        self.elapsedMs = max(0, elapsedMs)
        revision = UUID()
    }

    /// The next piece of reasoning. Pieces join into one stretch until a lookup that was
    /// not there before starts (the server keeps them by the same rule, so a reopened
    /// thread reads as it did live). Bounded as on the desktop.
    mutating func addReasoning(_ text: String) {
        let stretch = 20_000
        if let last = reasoning.last, last.afterTools == tools.count {
            let room = max(0, stretch - last.text.count)
            guard room > 0, !text.isEmpty else { return }
            reasoning[reasoning.count - 1] = ChatReasoningDTO(afterTools: last.afterTools, text: last.text + String(text.prefix(room)))
        } else {
            let start = text.drop { $0.isWhitespace || $0.isNewline }
            guard !start.isEmpty, reasoning.count < 40 else { return }
            reasoning.append(ChatReasoningDTO(afterTools: tools.count, text: String(start.prefix(stretch))))
        }
        revision = UUID()
    }

    mutating func apply(_ event: ChatStreamEventDTO) {
        switch event.type {
        case .tool:
            guard let tool = event.tool else { return }
            if let index = tools.firstIndex(where: { $0.callId == tool.callId }) { tools[index] = tool }
            else { tools.append(tool); tools = Array(tools.suffix(20)) }
        case .citations:
            guard let values = event.citations else { return }
            var seen = Set<String>()
            citations = Array(values.filter { seen.insert($0.id).inserted }.prefix(40))
        default: return
        }
        revision = UUID()
    }
}

enum ChatCitationMarkup {
    private static let markers = try! NSRegularExpression(pattern: #"\[\[(S[1-9][0-9]{0,2})\]\]"#)
    private static let ticks = try! NSRegularExpression(pattern: #"`+"#)
    static func url(id: String, turnID: UUID) -> URL? {
        guard id.range(of: "^S[1-9][0-9]{0,2}$", options: .regularExpression) != nil else { return nil }
        return URL(string: "cereal-citation://source/\(turnID.uuidString)/\(id)")
    }
    static func resolve(_ url: URL, turnID: UUID?, citations: [ChatCitationDTO]) -> ChatCitationDTO? {
        guard let turnID else { return nil }
        return citations.first { self.url(id: $0.id, turnID: turnID) == url }
    }
    static func number(_ citation: ChatCitationDTO) -> String { String(citation.id.dropFirst()) }

    /// What a citation's chip holds in the text: its number after a figure space, so the
    /// room is one figure wider than the number and never breaks across lines. The chip is
    /// painted over it (`CitationChipRenderer`); VoiceOver and a copy read the number.
    static func room(_ id: String) -> String { "\u{2007}" + id.dropFirst() }

    /// Only issued markers receive app-local links. Code spans are never rewritten,
    /// and unknown markers cannot invent tappable sources. The random local turn
    /// ID is absent from model context, so arbitrary model links cannot impersonate it.
    static func markdown(_ text: String, turnID: UUID?, citations: [ChatCitationDTO]) -> String {
        guard let turnID else { return text }
        return rewrite(text) { id in
            guard citations.contains(where: { $0.id == id }), let url = url(id: id, turnID: turnID) else { return nil }
            return "[\(room(id))](\(url.absoluteString))"
        }
    }

    /// The text with no source marks, and no space left where one was: a conversation
    /// about one note shows none, as on the desktop (everything in it is from that note).
    static func withoutMarkers(_ text: String) -> String { rewrite(text) { _ in nil } }

    /// Each issued marker becomes a word space and what `link` gives for it, and a marker
    /// with nothing to show goes with the space before it. Code spans are left as written.
    private static func rewrite(_ text: String, link: (String) -> String?) -> String {
        func replace(_ value: String) -> String {
            var result = value
            for match in markers.matches(in: value, range: NSRange(value.startIndex..., in: value)).reversed() {
                guard var range = Range(match.range, in: result), let idRange = Range(match.range(at: 1), in: value) else { continue }
                // The space before a mark is its own: a chip brings one back, a removed mark takes it.
                while range.lowerBound > result.startIndex, result[result.index(before: range.lowerBound)] == " " {
                    range = result.index(before: range.lowerBound)..<range.upperBound
                }
                let replacement = link(String(value[idRange])).map { " " + $0 } ?? ""
                result.replaceSubrange(range, with: replacement)
            }
            return result
        }
        var output = "", cursor = text.startIndex
        var delimiter: Int?
        for match in ticks.matches(in: text, range: NSRange(text.startIndex..., in: text)) {
            guard let range = Range(match.range, in: text) else { continue }
            let part = String(text[cursor..<range.lowerBound])
            output += delimiter == nil ? replace(part) : part
            output += text[range]
            if delimiter == match.range.length { delimiter = nil }
            else if delimiter == nil { delimiter = match.range.length }
            cursor = range.upperBound
        }
        let tail = String(text[cursor...]); output += delimiter == nil ? replace(tail) : tail
        return output
    }
}


enum ChatHistoryScope {
    static func key(accountIdentity: String?, noteID: String?) -> String? {
        guard let accountIdentity, !accountIdentity.isEmpty else { return nil }
        if let noteID, UUID(uuidString: noteID) == nil { return nil }
        return accountIdentity + "|" + (noteID ?? "workspace")
    }
}
