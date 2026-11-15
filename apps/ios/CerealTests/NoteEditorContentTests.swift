import Contracts
import Foundation
import Testing
@testable import Cereal

struct NoteEditorContentTests {
    @Test func allSaveEntryPointsSharePendingAndPayloadBoundaryGuards() {
        #expect(NoteEditorDraft.canSave(title: String(repeating: "t", count: 500), text: String(repeating: "n", count: 1_000_000), saving: false))
        #expect(!NoteEditorDraft.canSave(title: "Cedar", text: "Edited notes", saving: true))
        #expect(!NoteEditorDraft.canSave(title: String(repeating: "t", count: 501), text: "Notes", saving: false))
        #expect(!NoteEditorDraft.canSave(title: "Cedar", text: String(repeating: "n", count: 1_000_001), saving: false))
    }

    private func note(body: String = "", enhanced: String = "# Decision\n- Ship Friday") -> NoteDTO {
        NoteDTO(body: body, createdAt: "2026-11-12T23:15:00Z", date: "2026-11-12", duration: "", enhanced: enhanced,
            generatedRevision: 1, generationState: .ready, id: UUID().uuidString, recordingState: .stopped,
            title: "Cedar", transcriptRevision: 1, transcriptSegments: [], updatedAt: "2026-11-12T23:15:00Z")
    }

    @Test func enhancedPresentationStartsWithDisplayedDocumentWhenTypedBodyIsEmpty() {
        let document = note()
        let presentation = NoteEditorDraft(note: document, showMine: false)
        #expect(document.body.isEmpty)
        #expect(presentation.content == .enhanced)
        #expect(presentation.title == document.title)
        #expect(presentation.text == document.enhanced)
        #expect(!presentation.text.isEmpty)
        #expect(NoteEditorDraft(note: document, showMine: false).id != presentation.id)
    }

    @Test func myNotesAndUngeneratedDocumentsStartWithTypedText() {
        let displayedTyped = NoteEditorDraft(note: note(body: "My notes"), showMine: true)
        #expect(displayedTyped.content == .typed)
        #expect(displayedTyped.text == "My notes")
        let ungenerated = NoteEditorDraft(note: note(body: "Before generation", enhanced: ""), showMine: false)
        #expect(ungenerated.content == .typed)
        #expect(ungenerated.text == "Before generation")
    }

    @Test func titleOnlyUpdateNeverSendsStaleDocumentSnapshots() throws {
        let update = NoteEditorContent.titleUpdate("Renamed Cedar")
        let json = try #require(JSONSerialization.jsonObject(with: JSONEncoder().encode(update)) as? [String: String])
        #expect(json == ["title": "Renamed Cedar"])
        #expect(update.body == nil && update.enhanced == nil && update.transcriptSegments == nil)
    }

    @Test func editingEnhancedNotesDoesNotReplaceTypedNotesOrLifecycleFields() throws {
        let update = NoteEditorContent.enhanced.update(title: "Cedar", text: "# Decision\n- Ship Friday")
        let json = try #require(JSONSerialization.jsonObject(with: JSONEncoder().encode(update)) as? [String: String])
        #expect(json == ["title": "Cedar", "enhanced": "# Decision\n- Ship Friday"])
        #expect(update.body == nil)
        #expect(update.transcriptSegments == nil)
        #expect(update.date == nil)
        #expect(update.duration == nil)
    }

    @Test func editingTypedNotesPreservesTheGeneratedDocument() throws {
        let update = NoteEditorContent.typed.update(title: "Cedar", text: "My own notes")
        let json = try #require(JSONSerialization.jsonObject(with: JSONEncoder().encode(update)) as? [String: String])
        #expect(json == ["title": "Cedar", "body": "My own notes"])
        #expect(update.enhanced == nil)
    }

    @Test func sharedTimestampParsingAcceptsBothWireFormsAndRejectsInvalidDates() {
        let fraction = Date.serverTimestamp("2026-11-12T23:15:00.000Z")
        #expect(fraction != nil)
        #expect(fraction == Date.serverTimestamp("2026-11-12T23:15:00Z"))
        #expect(fraction == Date.serverTimestamp("2026-11-12T18:15:00-05:00"))
        #expect(Date.serverTimestamp("not a date") == nil)
    }
}
