import Foundation
import Contracts

/// Generated DTOs stay intact. A local revision token changes with each metadata
/// update, allowing SwiftUI/history equality without retroactive DTO conformances.
struct ChatTurnAnnotations: Codable, Sendable, Hashable {
    private var revision = UUID()
    private(set) var tools: [ChatToolProgressDTO] = []
    private(set) var citations: [ChatCitationDTO] = []
    var hasContent: Bool { !tools.isEmpty || !citations.isEmpty }
    static func == (lhs: Self, rhs: Self) -> Bool { lhs.revision == rhs.revision }
    func hash(into hasher: inout Hasher) { hasher.combine(revision) }

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

    /// Only issued markers receive app-local links. Code spans are never rewritten,
    /// and unknown markers cannot invent tappable sources. The random local turn
    /// ID is absent from model context, so arbitrary model links cannot impersonate it.
    static func markdown(_ text: String, turnID: UUID?, citations: [ChatCitationDTO]) -> String {
        guard let turnID else { return text }
        let issued = Set(citations.map(\.id))
        func replace(_ value: String) -> String {
            var result = value
            for match in markers.matches(in: value, range: NSRange(value.startIndex..., in: value)).reversed() {
                guard let range = Range(match.range, in: result), let idRange = Range(match.range(at: 1), in: value) else { continue }
                let id = String(value[idRange])
                let replacement = issued.contains(id) ? "[\\[\(id.dropFirst())\\]](\(url(id: id, turnID: turnID)!.absoluteString))" : ""
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
