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

#[test]
fn answers_of_a_server_without_the_newer_fields_still_decode_and_newer_ones_carry_them() {
    use meeting_notes_api_types::{CalendarEventDTO, HealthDTO};
    let older = json!({"status":"ok","version":"v1"});
    let health: HealthDTO = serde_json::from_value(older.clone()).unwrap();
    assert_eq!(health.features, None);
    assert_eq!(serde_json::to_value(health).unwrap(), older);
    let newer: HealthDTO = serde_json::from_value(json!({"status":"ok","version":"v1","features":["note-import","conditional-delete","meeting-identity","something-later"]})).unwrap();
    assert!(newer.features.unwrap().iter().any(|feature| feature == "note-import"));

    let event = json!({"id":"7b0b6c0e-6a8e-4d0c-9a56-0d6d3d2f9a11","calendarId":"0f4f7a52-1f0a-4f2e-8a43-5d1f1b1c2d3e","calendarName":"Work","color":"#87e5e8",
        "title":"Weekly sync","startAt":"2026-11-23T15:00:00.000Z","endAt":"2026-11-23T15:30:00.000Z","allDay":false,"startDate":"2026-11-23","endDate":"2026-11-23",
        "timeZone":null,"location":null,"meetingUrl":null,"noteId":null});
    let plain: CalendarEventDTO = serde_json::from_value(event.clone()).unwrap();
    assert_eq!((plain.meeting_uid.clone(), plain.occurrence.clone(), plain.attendee_count), (None, None, None));
    assert_eq!(serde_json::to_value(plain).unwrap(), event);
    let mut identified = event.clone();
    identified["meetingUid"] = json!("weekly-91ab@google.com");
    identified["occurrence"] = json!("instant:2026-11-23T15:00:00.000Z");
    identified["attendeeCount"] = json!(2);
    let event: CalendarEventDTO = serde_json::from_value(identified).unwrap();
    assert_eq!(event.meeting_uid.as_deref(), Some("weekly-91ab@google.com"));
    assert_eq!(event.occurrence.as_deref(), Some("instant:2026-11-23T15:00:00.000Z"));
    assert_eq!(event.attendee_count, Some(2));
    // A calendar not refreshed since the fields arrived sends them as null.
    let mut unfilled = serde_json::to_value(&event).unwrap();
    unfilled["meetingUid"] = json!(null);
    unfilled["occurrence"] = json!(null);
    unfilled["attendeeCount"] = json!(null);
    let unfilled: CalendarEventDTO = serde_json::from_value(unfilled).unwrap();
    assert_eq!((unfilled.meeting_uid, unfilled.occurrence, unfilled.attendee_count), (None, None, None));
}

#[test]
fn an_import_request_keeps_its_required_nulls_and_its_answer_decodes() {
    use meeting_notes_api_types::{ImportNoteInput, ImportedNoteDTO, ImportedRecordingState};
    let wire = json!({
        "id":"b8508589-1fbd-4f75-84e3-3b55a918ed08","title":"Q4 planning","body":"","enhanced":"","date":"2026-11-12","duration":"58:04",
        "createdAt":"2026-11-12T15:01:02.345Z","generated":true,"recordingState":"stopped","meeting":null,
        "transcriptSegments":[{"id":"1260e2cb-72d3-46d0-9ab5-596aff6f280e","channel":"mic","speakerId":"speaker_0","speakerLabel":null,"text":"Hello","isFinal":true,"startMs":0,"endMs":null}]
    });
    let input: ImportNoteInput = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(input.recording_state, ImportedRecordingState::Stopped);
    // `meeting: null` and `endMs: null` are sent, not dropped: the server requires the keys.
    assert_eq!(serde_json::to_value(input).unwrap(), wire);
    let answer: ImportedNoteDTO = serde_json::from_value(json!({
        "note":{"id":"b8508589-1fbd-4f75-84e3-3b55a918ed08","title":"Q4 planning","body":"","enhanced":"","date":"2026-11-12","duration":"58:04",
            "createdAt":"2026-11-12T15:01:02.345Z","updatedAt":"2026-11-15T12:00:00.000Z","importedAt":"2026-11-15T12:00:00.000Z",
            "recordingState":"stopped","generationState":"ready","transcriptRevision":1,"generatedRevision":1,"transcriptSegments":[]},
        "segmentIds":[{"from":"1260e2cb-72d3-46d0-9ab5-596aff6f280e","to":"5f0c5d0e-7f0a-5b6e-9c57-2d8a1c1e4f00"}]
    })).unwrap();
    assert_eq!(answer.note.imported_at.as_deref(), Some("2026-11-15T12:00:00.000Z"));
    assert_eq!(answer.segment_ids[0].from, "1260e2cb-72d3-46d0-9ab5-596aff6f280e");
}
