import Foundation
import Contracts

public enum ChatCitationSourceError: Error, LocalizedError, Equatable, Sendable {
    case wrongNote, transcriptChanged, missingSegment
    public var errorDescription: String? {
        switch self {
        case .wrongNote: "This citation does not belong to this note."
        case .transcriptChanged: "The transcript changed after this answer. Open the current note to review it; the citation cannot jump to a different revision."
        case .missingSegment: "The cited transcript segment is no longer available."
        }
    }
}

public enum ChatCitationSource {
    /// A transcript citation opens while its own passage is still there: the same
    /// segment, final, at the cited start time, with text that still contains the
    /// quote. The note-wide transcript revision is not compared: it moves whenever
    /// speech is appended during a meeting or something else in the note is edited,
    /// and neither touches the cited passage. Never fall back to a nearby
    /// timestamp or to matching words in another segment.
    public static func transcriptSegment(_ citation: ChatCitationDTO, in note: NoteDTO) throws -> TranscriptSegmentDTO {
        guard UUID(uuidString: citation.noteId) == UUID(uuidString: note.id), UUID(uuidString: note.id) != nil else { throw ChatCitationSourceError.wrongNote }
        guard citation.source == .transcript, let id = citation.segmentId, let uuid = UUID(uuidString: id),
              let segment = note.transcriptSegments.first(where: { UUID(uuidString: $0.id) == uuid }) else { throw ChatCitationSourceError.missingSegment }
        guard segment.isFinal else { throw ChatCitationSourceError.transcriptChanged }
        if let start = citation.startMs, start != segment.startMs { throw ChatCitationSourceError.transcriptChanged }
        guard segment.text.contains(citation.quote) else { throw ChatCitationSourceError.transcriptChanged }
        return segment
    }
}
