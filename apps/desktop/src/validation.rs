//! Cross-feature checks for local editing, outlining, search, and restoration.
//! These tests never access the user's persisted workspace or environment.

use crate::{model::Workspace, note::local_outline};

#[test]
fn empty_notes_do_not_produce_invented_summary_content() {
    for body in ["", " \n\t\n", "#\n##\n- \n* "] {
        assert!(local_outline(body).trim().is_empty());
    }
}

#[test]
fn outline_keeps_notes_and_explicit_actions_without_inventing_facts() {
    let body = "# Launch discussion\n\n- Keep the onboarding simple\n* Maya to share the prototype Friday\nTODO: Review the calendar empty state\nThe decision is still open.";
    let outline = local_outline(body);

    for original in [
        "Launch discussion",
        "Keep the onboarding simple",
        "Maya to share the prototype Friday",
        "TODO: Review the calendar empty state",
        "The decision is still open.",
    ] {
        assert_eq!(outline.matches(original).count(), 1);
    }
    assert!(outline.contains("Action items"));
    assert!(!outline.contains("- - "));
    assert!(!outline.contains("- * "));
    assert!(!outline.contains("meeting completed"));
}

#[test]
fn outline_preserves_unicode_names_punctuation_and_combining_characters() {
    let body = "Café with Zoë 🌱\nNext step: Send résumé to 李明\n- Budget is €2,400 — not final.";
    let outline = local_outline(body);
    for line in [
        "Café with Zoë 🌱",
        "Next step: Send résumé to 李明",
        "Budget is €2,400 — not final.",
    ] {
        assert!(outline.contains(line));
    }
    assert!(!outline.contains('\u{fffd}'));
}

#[test]
fn new_note_edits_and_local_outline_remain_searchable_after_serialization() {
    let mut workspace = Workspace::default();
    let folder_id = workspace.add_folder("Research".into());
    let note_id = workspace.create_note();
    let note = workspace.note_mut(note_id).unwrap();
    note.title = "Discovery with Rowan".into();
    note.body = "A quiet launch\nTODO: Share the indigo prototype".into();
    note.enhanced = local_outline(&note.body);
    note.transcript = "Rowan 00:12\nAccessibility needs a separate review.".into();
    note.folder_id = Some(folder_id);
    note.favorite = true;

    // Simulate the serialization boundary used by the workspace file, without
    // touching the filesystem or introducing process-wide environment races.
    let saved = serde_json::to_vec(&workspace).unwrap();
    let restored: Workspace = serde_json::from_slice(&saved).unwrap();

    for query in ["ROWAN", "quiet launch", "indigo prototype", "accessibility"] {
        let results = restored.visible_notes(Some(folder_id), true, query);
        assert_eq!(results.len(), 1, "missing query: {query}");
        assert_eq!(results[0].id, note_id);
    }
    assert!(
        restored
            .note(note_id)
            .unwrap()
            .enhanced
            .contains("indigo prototype")
    );
}

#[test]
fn search_finds_each_text_source_but_respects_trash_and_restore() {
    let mut workspace = Workspace::default();
    let note_id = workspace.create_note();
    let note = workspace.note_mut(note_id).unwrap();
    note.title = "Title marker: topaz".into();
    note.body = "Body marker: cobalt".into();
    note.enhanced = "Outline marker: violet".into();
    note.transcript = "Speaker marker: amber".into();

    for query in ["topaz", "cobalt", "violet", "amber"] {
        assert_eq!(workspace.visible_notes(None, false, query).len(), 1);
    }

    workspace.note_mut(note_id).unwrap().trashed = true;
    assert!(workspace.visible_notes(None, false, "amber").is_empty());
    assert!(
        workspace.note(note_id).is_some(),
        "trash must retain the note"
    );

    workspace.note_mut(note_id).unwrap().trashed = false;
    let restored = workspace.visible_notes(None, false, "AMBER");
    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].id, note_id);
}

#[test]
fn folder_and_favorite_changes_immediately_change_search_results() {
    let mut workspace = Workspace::default();
    let first_folder = workspace.add_folder("Project A".into());
    let second_folder = workspace.add_folder("Project B".into());
    let id = workspace.create_note();
    let note = workspace.note_mut(id).unwrap();
    note.title = "Shared search phrase".into();
    note.folder_id = Some(first_folder);

    assert_eq!(
        workspace
            .visible_notes(Some(first_folder), false, "search")
            .len(),
        1
    );
    assert!(
        workspace
            .visible_notes(Some(first_folder), true, "search")
            .is_empty()
    );

    let note = workspace.note_mut(id).unwrap();
    note.folder_id = Some(second_folder);
    note.favorite = true;

    assert!(
        workspace
            .visible_notes(Some(first_folder), false, "search")
            .is_empty()
    );
    assert_eq!(
        workspace
            .visible_notes(Some(second_folder), true, "search")
            .len(),
        1
    );
}
