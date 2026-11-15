import Foundation

// Local demo and on-device capture models. Connected screens use generated
// `Contracts` DTOs from packages/contracts; these types are never wire contracts
// and local fixtures/audio are not imported into a server instance automatically.

struct Person: Codable, Hashable, Identifiable, Sendable {
    var id: UUID = UUID()
    var name: String
    var email: String?
}

struct NoteSection: Codable, Hashable, Identifiable, Sendable {
    var id: UUID = UUID()
    var heading: String
    /// Inline markdown (italics, bold) per bullet.
    var bullets: [String]
}

struct TranscriptSegment: Codable, Hashable, Identifiable, Sendable {
    var id: UUID = UUID()
    var speaker: String
    var startedAt: TimeInterval
    var text: String
}

struct Note: Codable, Hashable, Identifiable, Sendable {
    var id: UUID = UUID()
    var title: String
    var createdAt: Date
    var attendees: [Person] = []
    /// AI-enhanced notes.
    var sections: [NoteSection] = []
    /// What the user typed during the meeting.
    var userNotes: String = ""
    var transcript: [TranscriptSegment] = []
    var trashed = false

    // Capture lifecycle: Start → Stop → Generate → Resume (same transcript) → Stop → Regenerate.
    /// Audio segments appended to this note's single transcript, in order.
    var captureSegments: [CaptureSegment] = []
    /// Bumped each time a capture segment is stopped, or the transcript otherwise changes.
    var transcriptRevision = 0
    /// The transcript revision the current enhanced notes were generated from.
    var notesGeneratedRevision: Int?
    /// Startup recovery status. Keeps unfinished captures visible without claiming transcription.
    var captureRecoveryHint: String?

    var displayTitle: String { title.isEmpty ? "Untitled note" : title }

    var hasCapture: Bool { transcriptRevision > 0 }
    /// True when there's captured audio the enhanced notes don't reflect yet.
    var needsGeneration: Bool { hasCapture && notesGeneratedRevision != transcriptRevision }
    var generateActionTitle: String { notesGeneratedRevision == nil ? "Generate notes" : "Regenerate notes" }

    var plainText: String {
        var out = "\(displayTitle)\n\n"
        for s in sections {
            out += "\(s.heading)\n" + s.bullets.map { "• \($0)" }.joined(separator: "\n") + "\n\n"
        }
        if !userNotes.isEmpty { out += userNotes }
        return out
    }
}

struct CaptureSegment: Codable, Hashable, Sendable {
    var index: Int
    var startedAt: Date
    var duration: TimeInterval
    var fileName: String
}

struct CalendarEvent: Codable, Hashable, Identifiable, Sendable {
    var id: UUID = UUID()
    var title: String
    var start: Date
    var end: Date
}

struct ChatMessage: Identifiable, Hashable, Sendable, Codable {
    enum Role: String, Sendable, Codable { case user, assistant }
    var id: UUID = UUID()
    var role: Role
    var text: String
    var isStreaming = false
    var isInterrupted = false
    var annotations: ChatTurnAnnotations? = nil
}

extension Note {
    private enum CodingKeys: String, CodingKey {
        case id, title, createdAt, attendees, sections, userNotes, transcript, trashed
        case captureSegments, transcriptRevision, notesGeneratedRevision, captureRecoveryHint
    }

    /// Tolerates notes saved before newer fields existed.
    init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(UUID.self, forKey: .id)
        title = try c.decode(String.self, forKey: .title)
        createdAt = try c.decode(Date.self, forKey: .createdAt)
        attendees = try c.decodeIfPresent([Person].self, forKey: .attendees) ?? []
        sections = try c.decodeIfPresent([NoteSection].self, forKey: .sections) ?? []
        userNotes = try c.decodeIfPresent(String.self, forKey: .userNotes) ?? ""
        transcript = try c.decodeIfPresent([TranscriptSegment].self, forKey: .transcript) ?? []
        trashed = try c.decodeIfPresent(Bool.self, forKey: .trashed) ?? false
        captureSegments = try c.decodeIfPresent([CaptureSegment].self, forKey: .captureSegments) ?? []
        transcriptRevision = try c.decodeIfPresent(Int.self, forKey: .transcriptRevision) ?? 0
        notesGeneratedRevision = try c.decodeIfPresent(Int.self, forKey: .notesGeneratedRevision)
        captureRecoveryHint = try c.decodeIfPresent(String.self, forKey: .captureRecoveryHint)
    }
}
