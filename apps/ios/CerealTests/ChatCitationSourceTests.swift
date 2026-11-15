import Foundation
import Testing
import Contracts
import APIClient
@testable import Cereal

/// Whether a chat citation can still be opened, as the citation sheet decides it
/// (`ChatCitationSource`). A citation depends on its own passage only: speech
/// that arrives after the answer and edits elsewhere in the note leave it open;
/// a passage that was edited away or removed is unavailable. Invented meeting.
struct ChatCitationSourceTests {
    private let noteID = "00000000-0000-4000-8000-000000000001"
    private let ids = (1...6).map { "00000000-0000-4000-8000-00000000100\($0)" }
    private func segment(_ index: Int, _ text: String, start: Int, final: Bool = true, id: String? = nil) -> TranscriptSegmentDTO {
        TranscriptSegmentDTO(channel: index.isMultiple(of: 2) ? .system : .mic, endMs: start + 4000, id: id ?? ids[index - 1], isFinal: final,
                             speakerId: "mic:part:speaker_1", speakerLabel: "Speaker 1", startMs: start, text: text)
    }
    private var spoken: [TranscriptSegmentDTO] {
        [segment(1, "The berth fee is three hundred a night.", start: 0),
         segment(2, "We booked the crane for Tuesday.", start: 6000),
         segment(3, "Ines will confirm the harbour pilot by Friday.", start: 12000)]
    }
    private var later: [TranscriptSegmentDTO] {
        [segment(4, "Good, that settles the crane.", start: 70000),
         segment(5, "Anything else before we close?", start: 125000),
         segment(6, "No, that is everything.", start: 131000)]
    }
    private func note(_ segments: [TranscriptSegmentDTO], revision: Int, body: String = "- berth fee is 300", enhanced: String = "", recording: RecordingState = .recording) -> NoteDTO {
        NoteDTO(body: body, createdAt: "2026-03-09T10:00:00Z", date: "2026-03-09", duration: "", enhanced: enhanced, generatedRevision: nil, generationState: .idle,
                id: noteID, recordingState: recording, title: "Harbour planning", transcriptRevision: revision, transcriptSegments: segments, updatedAt: "2026-03-09T10:20:00Z")
    }
    /// Issued while the note was at revision 3, quoting part of the third segment.
    private func pilot(quote: String = "confirm the harbour pilot by Friday", segmentID: String? = nil, start: Int? = 12000, noteID: String? = nil) -> ChatCitationDTO {
        ChatCitationDTO(id: "S3", noteId: noteID ?? self.noteID, quote: quote, segmentId: segmentID ?? ids[2], source: .transcript, startMs: start, title: "Harbour planning", transcriptRevision: 3)
    }

    @Test func aCitationOpensOnTheNoteItWasIssuedFor() throws {
        #expect(try ChatCitationSource.transcriptSegment(pilot(), in: note(spoken, revision: 3)).id == ids[2])
    }

    @Test func transcriptAppendedAfterTheAnswerLeavesTheCitationOpenOnTheSamePassage() throws {
        // One part lands, then two more: the note-wide revision moves each time.
        for (revision, segments) in [(4, spoken + [later[0]]), (6, spoken + later)] {
            let found = try ChatCitationSource.transcriptSegment(pilot(), in: note(segments, revision: revision))
            #expect(found.id == ids[2] && found.startMs == 12000)
            #expect(found.text == "Ines will confirm the harbour pilot by Friday.")
        }
        // The sheet scrolls to and highlights this exact row of the grown transcript.
        let grown = note(spoken + later, revision: 6)
        let focus = try ChatCitationSource.transcriptSegment(pilot(), in: grown).id
        #expect(TranscriptPresentation.rows(grown.transcriptSegments).filter { $0.segment.id == focus }.count == 1)
    }

    @Test func unrelatedEditsDoNotDisableACitation() throws {
        // Typed notes rewritten, notes generated, another segment corrected, the meeting stopped.
        var segments = spoken + later
        segments[1] = segment(2, "We moved the crane to Wednesday.", start: 6000)
        let edited = note(segments, revision: 9, body: "- berth fee went up\n- crane: ask Ines", enhanced: "# Decisions\n- Crane on Wednesday.", recording: .stopped)
        #expect(try ChatCitationSource.transcriptSegment(pilot(), in: edited).id == ids[2])
        // Words added around the quote keep it in place.
        segments[2] = segment(3, "Right. Ines will confirm the harbour pilot by Friday, she said.", start: 12000)
        #expect(try ChatCitationSource.transcriptSegment(pilot(), in: note(segments, revision: 10)).id == ids[2])
    }

    @Test func aPassageEditedAwayIsUnavailableWithTheExistingWording() {
        var segments = spoken + later
        segments[2] = segment(3, "Ines will confirm the tug by Monday.", start: 12000)
        #expect(throws: ChatCitationSourceError.transcriptChanged) { try ChatCitationSource.transcriptSegment(pilot(), in: note(segments, revision: 7)) }
        // The same holds without any revision change: the passage decides, not the number.
        #expect(throws: ChatCitationSourceError.transcriptChanged) { try ChatCitationSource.transcriptSegment(pilot(), in: note(segments, revision: 3)) }
        #expect(ChatCitationSourceError.transcriptChanged.errorDescription == "The transcript changed after this answer. Open the current note to review it; the citation cannot jump to a different revision.")
    }

    @Test func aMovedOrUnfinishedSegmentIsUnavailable() {
        var moved = spoken, interim = spoken
        moved[2] = segment(3, "Ines will confirm the harbour pilot by Friday.", start: 13000)
        interim[2] = segment(3, "Ines will confirm the harbour pilot by Friday.", start: 12000, final: false)
        #expect(throws: ChatCitationSourceError.transcriptChanged) { try ChatCitationSource.transcriptSegment(pilot(), in: note(moved, revision: 3)) }
        #expect(throws: ChatCitationSourceError.transcriptChanged) { try ChatCitationSource.transcriptSegment(pilot(), in: note(interim, revision: 3)) }
    }

    @Test func aDeletedPassageIsUnavailableWithTheExistingWordingAndNeverFallsBackToMatchingWords() {
        #expect(throws: ChatCitationSourceError.missingSegment) { try ChatCitationSource.transcriptSegment(pilot(), in: note(Array(spoken.prefix(2)) + later, revision: 7)) }
        // The same sentence stored as a different segment is not the cited passage.
        let replaced = Array(spoken.prefix(2)) + [segment(3, "Ines will confirm the harbour pilot by Friday.", start: 12000, id: "00000000-0000-4000-8000-000000001999")]
        #expect(throws: ChatCitationSourceError.missingSegment) { try ChatCitationSource.transcriptSegment(pilot(), in: note(replaced, revision: 4)) }
        #expect(throws: ChatCitationSourceError.missingSegment) { try ChatCitationSource.transcriptSegment(pilot(), in: note([], revision: 0)) }
        #expect(ChatCitationSourceError.missingSegment.errorDescription == "The cited transcript segment is no longer available.")
    }

    @Test func aCitationNeverOpensAnotherNote() {
        let other = "00000000-0000-4000-8000-000000000002"
        #expect(throws: ChatCitationSourceError.wrongNote) { try ChatCitationSource.transcriptSegment(pilot(noteID: other), in: note(spoken + later, revision: 6)) }
    }
}
