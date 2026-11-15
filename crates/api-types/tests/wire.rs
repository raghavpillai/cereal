use meeting_notes_api_types::{CreateNoteInput, GenerationState, NoteDTO};
use serde_json::json;

#[test]
fn note_round_trip_preserves_states_nullable_values_and_source_segments() {
    let wire = json!({
        "id":"b8508589-1fbd-4f75-84e3-3b55a918ed08", "title":"Design review — 東京", "body":"Draft", "enhanced":"",
        "date":"2026-11-12", "duration":"00:42", "createdAt":"2026-11-12T10:00:00Z", "updatedAt":"2026-11-12T10:00:42Z",
        "recordingState":"stopped", "generationState":"analyzing_transcript", "transcriptRevision":2, "generatedRevision":null,
        "transcriptSegments":[{"id":"1260e2cb-72d3-46d0-9ab5-596aff6f280e", "channel":"system", "speakerId":"remote-1", "speakerLabel":null,
        "text":"Hello 👋", "isFinal":false, "startMs":100, "endMs":null}]
    });
    let note: NoteDTO = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(note.generation_state, GenerationState::AnalyzingTranscript);
    assert_eq!(serde_json::to_value(note).unwrap(), wire);
}

#[test]
fn omitted_optional_create_fields_stay_omitted() {
    let wire = json!({"title":"Only the required field"});
    let input: CreateNoteInput = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(input).unwrap(), wire);
}
