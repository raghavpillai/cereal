import Foundation
import Testing
import Contracts
@testable import APIClient

@Suite struct ChatCitationSourceTests {
    private let noteID = "00000000-0000-4000-8000-000000000001"
    private let segmentID = "3a49a4af-c660-42e9-a36a-a806fbb41e0d"
    private func note(revision: Int = 3) -> NoteDTO {
        let segment = TranscriptSegmentDTO(channel: .mic, endMs: 4000, id: segmentID, isFinal: true, speakerId: "unknown", speakerLabel: nil, startMs: 1000, text: "The release is Friday.")
        return NoteDTO(body: "", createdAt: "2026-11-13T00:00:00Z", date: "2026-11-13", duration: "", enhanced: "", generatedRevision: nil, generationState: .idle, id: noteID, recordingState: .stopped, title: "Source fixture", transcriptRevision: revision, transcriptSegments: [segment], updatedAt: "2026-11-13T00:00:00Z")
    }
    private func citation(noteID: String? = nil, segmentID: String? = nil, quote: String = "release is Friday", start: Int = 1000) -> ChatCitationDTO {
        ChatCitationDTO(id: "S1", noteId: noteID ?? self.noteID, quote: quote, segmentId: segmentID ?? self.segmentID, source: .transcript, startMs: start, title: "Source fixture", transcriptRevision: 3)
    }
    @Test func exactCitationUsesItsActualSegment() throws {
        #expect(try ChatCitationSource.transcriptSegment(citation(segmentID: segmentID.uppercased()), in: note()).id == segmentID)
    }
    @Test func aMovedNoteRevisionAloneDoesNotInvalidateAnUntouchedPassage() throws {
        #expect(try ChatCitationSource.transcriptSegment(citation(), in: note(revision: 4)).id == segmentID)
        #expect(try ChatCitationSource.transcriptSegment(citation(), in: note(revision: 40)).id == segmentID)
    }
    @Test func changedPassageNeverFallsBackToMatchingWordsOrTimestamp() {
        #expect(throws: ChatCitationSourceError.transcriptChanged) { try ChatCitationSource.transcriptSegment(citation(quote: "Monday"), in: note()) }
        #expect(throws: ChatCitationSourceError.transcriptChanged) { try ChatCitationSource.transcriptSegment(citation(start: 2000), in: note()) }
    }
    @Test func unrelatedOrRemovedSegmentIsRejected() {
        #expect(throws: ChatCitationSourceError.wrongNote) { try ChatCitationSource.transcriptSegment(citation(noteID: UUID().uuidString), in: note()) }
        #expect(throws: ChatCitationSourceError.missingSegment) { try ChatCitationSource.transcriptSegment(citation(segmentID: UUID().uuidString), in: note()) }
    }
}
