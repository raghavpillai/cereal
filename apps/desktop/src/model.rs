//! Local-only workspace data. This never reads Cereal's own storage or services.

use chrono::Local;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct Workspace {
    pub notes: Vec<Note>,
    pub folders: Vec<Folder>,
    pub settings: Settings,
    /// Bumped by every mutable access to notes or folders, so lists derived
    /// from them can be cached between renders. Not persisted.
    #[serde(skip)]
    revision: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Note {
    pub id: u64,
    pub title: String,
    pub body: String,
    pub enhanced: String,
    /// Unsynchronized user edits to generated notes survive reloads and late
    /// provider completions. Only an acknowledgement of this text clears it.
    #[serde(default, rename = "enhancedDirty")]
    pub enhanced_dirty: bool,
    pub transcript: String,
    /// Preserve canonical server provenance for transcript bubble alignment and
    /// diarized identities. The plain transcript remains the copy/search format.
    #[serde(default, rename = "serverTranscriptSegments")]
    pub server_transcript_segments: Vec<meeting_notes_api_types::TranscriptSegmentDTO>,
    /// Association to our configured server; local fixtures never upload implicitly.
    #[serde(rename = "serverId")]
    pub server_id: Option<String>,
    #[serde(rename = "serverUrl")]
    pub server_url: Option<String>,
    #[serde(default, rename = "serverCreatedAt")]
    pub server_created_at: Option<String>,
    /// The server's last change and revisions for the copy of this note kept
    /// here. The list refresh reads a note again only when the server's row no
    /// longer matches it.
    #[serde(default, rename = "serverSyncStamp")]
    pub server_sync_stamp: Option<String>,
    #[serde(default, rename = "meetingContext")]
    pub meeting_context: Option<meeting_notes_api_types::MeetingContextDTO>,
    #[serde(rename = "transcriptRevision", alias = "transcript_revision")]
    pub transcript_revision: u64,
    #[serde(rename = "generatedRevision", alias = "generated_revision")]
    pub generated_revision: Option<u64>,
    /// Local microphone/system capture metadata; audio never invents a transcript.
    #[serde(rename = "audioCaptures", alias = "audio_captures")]
    pub audio_captures: Vec<crate::audio::CaptureManifest>,
    /// Locally entered attendees; the current user is represented separately.
    pub participants: Vec<String>,
    /// ISO calendar date, e.g. `2026-11-12`.
    pub date: String,
    pub time: String,
    pub duration: String,
    pub folder_id: Option<u64>,
    pub favorite: bool,
    pub trashed: bool,
}

impl Note {
    /// Bytes a full clone of this note duplicates (its text and segments).
    pub fn bulk(&self) -> usize {
        self.title.len()
            + self.body.len()
            + self.enhanced.len()
            + self.transcript.len()
            + self.server_transcript_segments.len()
                * std::mem::size_of::<meeting_notes_api_types::TranscriptSegmentDTO>()
            + self.audio_captures.len() * std::mem::size_of::<crate::audio::CaptureManifest>()
    }

    /// A copy for building one frame of the note view. The view needs the
    /// summary text and the note's metadata, but of the body, the transcript
    /// and its segments only whether they are empty: the editor and the
    /// transcript panel read those from the workspace themselves. So those
    /// three are reduced to "empty or not" instead of being duplicated on
    /// every animation frame.
    pub fn frame_copy(&self) -> Self {
        let present = |text: &str| {
            if text.trim().is_empty() {
                String::new()
            } else {
                "\u{b7}".to_string()
            }
        };
        Self {
            id: self.id,
            title: self.title.clone(),
            body: present(&self.body),
            enhanced: self.enhanced.clone(),
            enhanced_dirty: self.enhanced_dirty,
            transcript: present(&self.transcript),
            server_transcript_segments: Vec::new(),
            server_id: self.server_id.clone(),
            server_url: self.server_url.clone(),
            server_created_at: self.server_created_at.clone(),
            server_sync_stamp: self.server_sync_stamp.clone(),
            meeting_context: self.meeting_context.clone(),
            transcript_revision: self.transcript_revision,
            generated_revision: self.generated_revision,
            audio_captures: Vec::new(),
            participants: self.participants.clone(),
            date: self.date.clone(),
            time: self.time.clone(),
            duration: self.duration.clone(),
            folder_id: self.folder_id,
            favorite: self.favorite,
            trashed: self.trashed,
        }
    }

    pub fn timeline_date(&self) -> String {
        let created = self
            .server_created_at
            .as_deref()
            .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
            .map(|time| {
                time.with_timezone(&Local)
                    .date_naive()
                    .format("%Y-%m-%d")
                    .to_string()
            });
        match (
            chrono::NaiveDate::parse_from_str(&self.date, "%Y-%m-%d").ok(),
            created,
        ) {
            // Imported historical notes retain their actual meeting day. A
            // future calendar association still appears on its creation day.
            (Some(_), Some(created)) => self.date.clone().min(created),
            (Some(_), None) => self.date.clone(),
            (None, Some(created)) => created,
            (None, None) => self.date.clone(),
        }
    }
    /// A response belongs to the text snapshot that was sent, not whatever the
    /// user has typed since. In particular, a late generation cannot erase edits.
    pub fn reconcile_enhanced_reply(
        &mut self,
        submitted: &str,
        submitted_dirty: bool,
        remote: &str,
        generated_revision: Option<u64>,
        allow_generation: bool,
    ) {
        if submitted_dirty && self.enhanced == submitted {
            self.enhanced_dirty = false;
        }
        if !self.enhanced_dirty
            && self.enhanced == submitted
            && allow_generation
            && (!remote.is_empty() || submitted_dirty)
        {
            self.enhanced = remote.to_owned();
            self.generated_revision = generated_revision;
        }
    }

    /// Existing enhanced notes form the baseline when migrating an older file.
    pub fn normalize_generation_revision(&mut self) {
        if !self.enhanced.trim().is_empty() && self.generated_revision.is_none() {
            self.generated_revision = Some(self.transcript_revision);
        }
    }
    pub fn replace_transcript(&mut self, transcript: impl Into<String>) -> bool {
        let transcript = transcript.into();
        if self.transcript == transcript {
            return false;
        }
        self.normalize_generation_revision();
        self.transcript = transcript;
        self.server_transcript_segments.clear();
        self.transcript_revision = self.transcript_revision.saturating_add(1);
        true
    }
    pub fn append_transcript(&mut self, block: &str) -> bool {
        if block.trim().is_empty() {
            return false;
        }
        let transcript = if self.transcript.trim().is_empty() {
            block.to_string()
        } else {
            format!("{}\n\n{}", self.transcript, block)
        };
        self.replace_transcript(transcript)
    }
    pub fn needs_regeneration(&self) -> bool {
        !self.enhanced.trim().is_empty()
            && self
                .generated_revision
                .is_some_and(|revision| self.transcript_revision > revision)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Folder {
    pub id: u64,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub dark_mode: bool,
    pub auto_enhance: bool,
    pub show_upcoming: bool,
    /// The note about privacy on the private space page has been closed.
    /// Kept so that it stays closed after a restart.
    pub space_banner_dismissed: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            dark_mode: false,
            auto_enhance: true,
            show_upcoming: true,
            space_banner_dismissed: false,
        }
    }
}

impl Workspace {
    /// Load our own local workspace, using fictional examples on first launch.
    /// An unreadable or malformed existing file stays intact; save backs up
    /// malformed JSON before replacing it.
    pub fn load() -> Self {
        // Never read the file while a write that was already handed over is
        // still on its way to disk.
        crate::store::wait_idle();
        workspace_path()
            .and_then(|path| Self::load_from(&path))
            .unwrap_or_else(|_| {
                if crate::server::enabled() {
                    Self::default()
                } else {
                    Self::demo()
                }
            })
    }

    /// The workspace as it is written to disk.
    pub fn encode(&self) -> Result<Vec<u8>, String> {
        serde_json::to_vec_pretty(self)
            .map_err(|error| format!("Cannot serialize workspace: {error}"))
    }

    pub fn demo() -> Self {
        let entries = [
            (
                1,
                "Product & design sync",
                "2026-11-12",
                "10:30 AM",
                "32 min",
                Some(101),
                true,
                "Launch checklist\n\n- Keep onboarding to three steps\n- Maya to share the updated prototypes\n- Try the new empty state with five people\n\nNext review on Monday.",
                "## Product direction\n\nThe team aligned on a simpler first-run experience. The next iteration will focus on helping people reach their first useful note sooner.\n\n## Design updates\n\n- Reduce onboarding from five steps to three\n- Use a calmer empty state with one clear action\n- Keep the keyboard shortcut visible in the new-note button\n\n## Next steps\n\n- Maya: share the updated prototype by Friday\n- Theo: arrange five usability sessions\n- Review the results together on Monday",
                "Maya  00:00\nI have the latest onboarding screens ready to share.\n\nTheo  00:18\nThe shorter flow feels much clearer. Can we bring the first note forward?\n\nMaya  00:41\nYes. I'll make that change and send the prototype on Friday.\n\nTheo  01:04\nGreat. I'll find five people to try it before our Monday review.",
            ),
            (
                2,
                "Weekly team standup",
                "2026-11-12",
                "9:00 AM",
                "24 min",
                Some(101),
                false,
                "Good progress on the release.\n\n- Search is ready for a final pass\n- Fix the calendar empty state\n- Friday release still on track",
                "## This week's focus\n\nThe release is on track for Friday. Search is ready for a final review, and the calendar empty state needs one small update.\n\n## Action items\n\n- Jules: complete the search review\n- Sam: update the calendar empty state\n- Team: flag any release blockers before Thursday afternoon",
                "Jules  00:00\nSearch is ready for a final review today.\n\nSam  00:24\nI'll finish the calendar empty state this afternoon.\n\nJules  00:45\nThat keeps us on track for Friday.",
            ),
            (
                3,
                "Customer discovery · Northstar",
                "2026-11-11",
                "2:00 PM",
                "45 min",
                Some(102),
                true,
                "How the team shares meeting notes\n\n- Most notes live in separate documents\n- Follow-ups get lost after handoff\n- Would like a shared folder for each project",
                "## Current workflow\n\nThe Northstar team keeps meeting notes in separate documents. Finding the latest decision takes time, especially when someone misses a meeting.\n\n## Opportunities\n\n- Shared project folders would make notes easier to find\n- Clear action items could reduce manual follow-up\n- Search should include the original notes as well as summaries\n\n## Follow-up\n\nSend a prototype of the shared-folder flow next week.",
                "Alex  00:00\nWe usually start a fresh document for each meeting.\n\nRiley  00:22\nWhat happens when someone needs to catch up?\n\nAlex  00:39\nThey ask in chat. A shared folder for each project would help a lot.",
            ),
            (
                4,
                "Coffee with Maya",
                "2026-11-11",
                "11:00 AM",
                "28 min",
                None,
                false,
                "A few ideas for the next sprint.\n\nMake room for polish. Save a day for small fixes. Try a walking meeting next week.",
                "## Ideas for next sprint\n\n- Set aside one day for visual polish and small fixes\n- Keep the planning session short and focused\n- Try a walking meeting for next week's catch-up",
                "Maya  00:00\nI'd love to spend a little more time on the small details next sprint.\n\nTheo  00:21\nLet's reserve a day for polish and see how it goes.",
            ),
            (
                5,
                "Engineering planning",
                "2026-11-10",
                "3:30 PM",
                "50 min",
                Some(101),
                false,
                "Priorities\n\n1. Improve startup time\n2. Ship the search improvements\n3. Review keyboard navigation\n\nKeep scope small for the next release.",
                "## Sprint priorities\n\nThe team agreed to keep the next release focused on performance and everyday usability.\n\n- Improve cold-start performance\n- Complete the search improvements\n- Review keyboard navigation across the main screens\n\n## Next steps\n\nOwners will add estimates before Thursday's planning check-in.",
                "Sam  00:00\nThe startup work is the largest item on the list.\n\nJules  00:17\nWe should leave room for keyboard navigation too.\n\nSam  00:34\nAgreed. Let's add estimates before Thursday.",
            ),
            (
                6,
                "Brand exploration",
                "2026-11-10",
                "10:00 AM",
                "35 min",
                Some(103),
                false,
                "Quiet, useful, human.\n\nExplore warmer neutrals and a more editorial type direction. Share two directions next week.",
                "## Creative direction\n\nThe next exploration should feel quiet, useful, and human.\n\n- Explore a warmer neutral palette\n- Pair straightforward interface text with editorial headings\n- Share two distinct directions at the next review",
                "Maya  00:00\nThe quieter direction feels closer to what we're aiming for.\n\nRiley  00:20\nLet's see two options with warmer neutrals next week.",
            ),
        ];

        Self {
            notes: entries
                .into_iter()
                .map(
                    |(
                        id,
                        title,
                        date,
                        time,
                        duration,
                        folder_id,
                        favorite,
                        body,
                        enhanced,
                        transcript,
                    )| {
                        Note {
                            id,
                            title: title.into(),
                            body: body.into(),
                            enhanced: enhanced.into(),
                            enhanced_dirty: false,
                            transcript: transcript.into(),
                            server_transcript_segments: Vec::new(),
                            server_id: None,
                            server_url: None,
                            server_created_at: None,
                            server_sync_stamp: None,
                            meeting_context: None,
                            transcript_revision: 1,
                            generated_revision: Some(1),
                            audio_captures: Vec::new(),
                            participants: Vec::new(),
                            date: date.into(),
                            time: time.into(),
                            duration: duration.into(),
                            folder_id,
                            favorite,
                            trashed: false,
                        }
                    },
                )
                .collect(),
            folders: vec![
                Folder {
                    id: 101,
                    name: "Team meetings".into(),
                },
                Folder {
                    id: 102,
                    name: "Customer conversations".into(),
                },
                Folder {
                    id: 103,
                    name: "Ideas & inspiration".into(),
                },
            ],
            settings: Settings::default(),
            revision: 0,
        }
    }

    pub fn create_note(&mut self) -> u64 {
        self.revision = self.revision.wrapping_add(1);
        let id = self.next_id();
        let now = Local::now();
        self.notes.insert(
            0,
            Note {
                id,
                title: "Untitled note".into(),
                date: now.format("%Y-%m-%d").to_string(),
                time: now.format("%-I:%M %p").to_string(),
                duration: "0 min".into(),
                ..Note::default()
            },
        );
        id
    }

    pub fn note(&self, id: u64) -> Option<&Note> {
        self.notes.iter().find(|note| note.id == id)
    }

    pub fn note_mut(&mut self, id: u64) -> Option<&mut Note> {
        self.revision = self.revision.wrapping_add(1);
        self.notes.iter_mut().find(|note| note.id == id)
    }

    /// Identity of the notes and folders as they are now: changes whenever
    /// they may have changed, and whenever the workspace itself is replaced.
    pub fn revision(&self) -> (u64, usize, usize, usize) {
        (
            self.revision,
            self.notes.len(),
            self.folders.len(),
            self.notes.as_ptr() as usize,
        )
    }

    pub fn add_folder(&mut self, name: String) -> u64 {
        self.revision = self.revision.wrapping_add(1);
        let id = self.next_id();
        let name = name.trim();
        self.folders.push(Folder {
            id,
            name: if name.is_empty() {
                "Untitled folder".into()
            } else {
                name.into()
            },
        });
        id
    }

    /// All untrashed notes, optionally restricted by folder, favorite, and text.
    /// Results are newest first; searching covers the title and all note text.
    pub fn visible_notes(
        &self,
        folder_id: Option<u64>,
        favorites_only: bool,
        query: &str,
    ) -> Vec<&Note> {
        let query = query.trim().to_lowercase();
        let mut notes: Vec<_> = self
            .notes
            .iter()
            .filter(|note| {
                !note.trashed
                    && folder_id.is_none_or(|folder| note.folder_id == Some(folder))
                    && (!favorites_only || note.favorite)
                    && (query.is_empty()
                        || [&note.title, &note.body, &note.enhanced, &note.transcript]
                            .iter()
                            .any(|text| text.to_lowercase().contains(&query)))
            })
            .collect();
        notes.sort_by(|a, b| {
            b.timeline_date()
                .cmp(&a.timeline_date())
                .then_with(|| b.server_created_at.cmp(&a.server_created_at))
                .then_with(|| b.id.cmp(&a.id))
        });
        notes
    }

    fn next_id(&self) -> u64 {
        let used = |id| {
            self.notes.iter().any(|note| note.id == id)
                || self.folders.iter().any(|folder| folder.id == id)
        };
        let highest = self
            .notes
            .iter()
            .map(|note| note.id)
            .chain(self.folders.iter().map(|folder| folder.id))
            .max()
            .unwrap_or(0);
        if let Some(next) = highest.checked_add(1) {
            return next;
        }
        // Imported data can contain u64::MAX; use a free gap without overflow.
        (1..u64::MAX).find(|id| !used(*id)).unwrap_or(0)
    }

    fn load_from(path: &Path) -> Result<Self, String> {
        let data = fs::read(path).map_err(|error| format!("Cannot read workspace: {error}"))?;
        let mut workspace: Self = serde_json::from_slice(&data)
            .map_err(|error| format!("Cannot parse workspace: {error}"))?;
        // Native finalization can complete during app shutdown after GPUI's last
        // workspace save. The owner reference already exists; reconcile metadata.
        for note in &mut workspace.notes {
            for saved in &mut note.audio_captures {
                if let Ok(data) = fs::read(&saved.manifest_path)
                    && let Ok(manifest) =
                        serde_json::from_slice::<crate::audio::CaptureManifest>(&data)
                    && manifest.note_id == note.id
                    && manifest.capture_id == saved.capture_id
                {
                    *saved = manifest;
                }
            }
        }
        Ok(workspace)
    }

    #[cfg(test)]
    fn save_to(&self, path: &Path) -> Result<(), String> {
        write_workspace(path, &self.encode()?, true)
    }
}

/// Where the current account's workspace lives.
pub fn workspace_file() -> Result<PathBuf, String> {
    workspace_path()
}

/// Atomically replace the workspace file with `data`: write a private
/// temporary file, sync it, rename it into place.
///
/// When `unknown_previous` is set (the first write to this file in this
/// process) whatever is on disk is inspected first: malformed JSON is copied
/// aside before it is replaced, and a file that cannot be read is never
/// overwritten. Later writes replace a file this process itself wrote, so the
/// check is not repeated on every save.
pub fn write_workspace(path: &Path, data: &[u8], unknown_previous: bool) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or("Workspace path has no parent directory")?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("Cannot create workspace directory: {error}"))?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("Cannot timestamp workspace: {error}"))?
        .as_nanos();

    if unknown_previous {
        // Retain malformed existing data, and never treat permission failures
        // as permission to replace an unknown file.
        match fs::read(path) {
            Ok(previous) if serde_json::from_slice::<Workspace>(&previous).is_err() => {
                let backup = parent.join(format!("workspace.corrupt-{stamp}.json"));
                write_new(&backup, &previous)
                    .map_err(|error| format!("Cannot preserve malformed workspace: {error}"))?;
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("Cannot inspect existing workspace: {error}")),
        }
    }

    let temporary = parent.join(format!(".workspace-{}-{stamp}.tmp", std::process::id()));
    let result = write_new(&temporary, data)
        .map_err(|error| format!("Cannot write workspace: {error}"))
        .and_then(|()| {
            fs::rename(&temporary, path)
                .map_err(|error| format!("Cannot replace workspace: {error}"))
        });
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub fn account_cache_path(root: &Path, identity: &str) -> PathBuf {
    let namespace = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, identity.as_bytes());
    root.join("servers")
        .join(namespace.to_string())
        .join("workspace.json")
}
fn workspace_path() -> Result<PathBuf, String> {
    let root = if let Some(directory) =
        crate::environment::var_os("CEREAL_DATA_DIR").filter(|v| !v.is_empty())
    {
        PathBuf::from(directory)
    } else {
        let home = std::env::var_os("HOME").ok_or("Cannot determine home directory")?;
        PathBuf::from(home).join("Library/Application Support/Cereal")
    };
    if crate::server::enabled() {
        let identity = crate::account::identity()
            .ok_or("Sign in before accessing private cached notes or audio")?;
        return Ok(account_cache_path(&root, &identity));
    }
    Ok(root.join("workspace.json"))
}

pub fn audio_directory() -> Result<PathBuf, String> {
    Ok(workspace_path()?
        .parent()
        .ok_or("Workspace has no parent")?
        .join("recordings"))
}

fn write_new(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(data)?;
    file.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_copy_keeps_what_the_note_view_reads_without_the_bulk() {
        let workspace = Workspace::demo();
        let note = workspace
            .notes
            .iter()
            .find(|note| !note.transcript.is_empty() && !note.enhanced.is_empty())
            .unwrap();
        let copy = note.frame_copy();
        // Everything the note page draws or decides on is intact…
        assert_eq!(copy.id, note.id);
        assert_eq!(copy.title, note.title);
        assert_eq!(copy.enhanced, note.enhanced);
        assert_eq!((&copy.date, &copy.time), (&note.date, &note.time));
        assert_eq!(copy.folder_id, note.folder_id);
        assert_eq!(copy.transcript_revision, note.transcript_revision);
        assert_eq!(copy.needs_regeneration(), note.needs_regeneration());
        // …including whether there is a body or a transcript at all…
        assert_eq!(
            copy.transcript.trim().is_empty(),
            note.transcript.trim().is_empty()
        );
        assert_eq!(copy.body.trim().is_empty(), note.body.trim().is_empty());
        // …but the transcript, body and segments themselves are not copied.
        assert!(copy.transcript.len() <= 2 && copy.body.len() <= 2);
        assert!(copy.server_transcript_segments.is_empty() && copy.audio_captures.is_empty());
        assert!(copy.bulk() < note.bulk());
        let blank = Note::default().frame_copy();
        assert!(blank.transcript.is_empty() && blank.body.is_empty());
    }

    #[test]
    fn future_meeting_notes_stay_in_their_creation_day_group() {
        let future = super::Note {
            date: "2030-11-16".into(),
            server_created_at: Some("2026-11-13T12:00:00Z".into()),
            ..Default::default()
        };
        assert!(future.timeline_date().starts_with("2026-11-"));
        assert_eq!(
            future.date, "2030-11-16",
            "Meeting metadata date is independent of Home's creation timeline"
        );
    }

    #[test]
    fn imported_historical_notes_keep_their_meeting_day_and_sort_before_todays_notes() {
        let older = Note {
            id: 1,
            date: "2026-11-12".into(),
            server_created_at: Some("2026-11-13T18:00:00Z".into()),
            ..Default::default()
        };
        let today = Note {
            id: 2,
            date: "2026-11-13".into(),
            server_created_at: Some("2026-11-13T12:00:00Z".into()),
            ..Default::default()
        };
        assert_eq!(older.timeline_date(), "2026-11-12");
        let workspace = Workspace {
            notes: vec![older, today],
            ..Default::default()
        };
        assert_eq!(
            workspace
                .visible_notes(None, false, "")
                .iter()
                .map(|note| note.id)
                .collect::<Vec<_>>(),
            vec![2, 1]
        );
    }

    #[test]
    fn private_workspace_namespace_is_stable_per_user_and_separate_on_the_same_server() {
        let root = std::path::Path::new("/fixture/cache");
        let first = super::account_cache_path(
            root,
            "https://notes.example|00000000-0000-4000-8000-000000000001",
        );
        let second = super::account_cache_path(
            root,
            "https://notes.example|00000000-0000-4000-8000-000000000002",
        );
        assert_ne!(first, second);
        assert_eq!(
            first,
            super::account_cache_path(
                root,
                "https://notes.example|00000000-0000-4000-8000-000000000001"
            )
        );
        assert!(!first.to_string_lossy().contains("notes.example"));
    }

    #[test]
    fn late_enhanced_acknowledgements_and_generation_preserve_newer_user_edits() {
        let mut note = Note {
            enhanced: "New user edit 🦊".into(),
            enhanced_dirty: true,
            generated_revision: Some(1),
            ..Default::default()
        };
        note.reconcile_enhanced_reply(
            "Old submitted text",
            true,
            "Old submitted text",
            Some(1),
            true,
        );
        assert_eq!(note.enhanced, "New user edit 🦊");
        assert!(note.enhanced_dirty);
        note.reconcile_enhanced_reply(
            "Old submitted text",
            false,
            "Late generated text",
            Some(2),
            true,
        );
        assert_eq!(note.enhanced, "New user edit 🦊");
        assert_eq!(note.generated_revision, Some(1));
        note.reconcile_enhanced_reply("New user edit 🦊", true, "New user edit 🦊", Some(1), true);
        assert!(!note.enhanced_dirty);
        let restored: Note = serde_json::from_str(&serde_json::to_string(&note).unwrap()).unwrap();
        assert_eq!(restored.enhanced, note.enhanced);
        assert!(!restored.enhanced_dirty);
    }

    #[test]
    fn deliberate_empty_enhanced_edit_is_acknowledged_without_restoring_old_notes() {
        let mut note = Note {
            enhanced_dirty: true,
            ..Default::default()
        };
        note.reconcile_enhanced_reply("", true, "", Some(3), true);
        assert!(note.enhanced.is_empty());
        assert!(!note.enhanced_dirty);
        assert_eq!(note.generated_revision, Some(3));
        note.enhanced = "Unsent change".into();
        note.enhanced_dirty = true;
        let restored: Note = serde_json::from_str(&serde_json::to_string(&note).unwrap()).unwrap();
        assert!(restored.enhanced_dirty);
        assert_eq!(restored.enhanced, "Unsent change");
    }

    fn test_directory(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("cereal-{name}-{}-{nonce}", std::process::id()))
    }

    #[test]
    fn typed_server_transcript_provenance_round_trips_and_legacy_notes_remain_valid() {
        let mut note: Note =
            serde_json::from_str(r#"{"id":1,"transcript":"Original words"}"#).unwrap();
        assert!(note.server_transcript_segments.is_empty());
        note.server_transcript_segments
            .push(meeting_notes_api_types::TranscriptSegmentDTO {
                id: "16ff59c2-79fa-433f-bba5-dbdd372acb11".into(),
                channel: meeting_notes_api_types::AudioChannel::Mic,
                speaker_id: "mic:chunk:speaker_1".into(),
                speaker_label: Some("Speaker 1".into()),
                text: "Original words".into(),
                is_final: true,
                start_ms: 1000,
                end_ms: Some(2000),
            });
        let json = serde_json::to_string(&note).unwrap();
        assert!(json.contains("serverTranscriptSegments"));
        let restored: Note = serde_json::from_str(&json).unwrap();
        assert_eq!(
            restored.server_transcript_segments,
            note.server_transcript_segments
        );
        assert!(!note.replace_transcript("Original words"));
        assert_eq!(note.server_transcript_segments.len(), 1);
        assert!(note.replace_transcript("A local/offline replacement"));
        assert!(
            note.server_transcript_segments.is_empty(),
            "Old speaker metadata must not describe replacement text"
        );
    }

    #[test]
    fn workspace_round_trips_edits_and_settings() {
        let directory = test_directory("roundtrip");
        let path = directory.join("workspace.json");
        let mut workspace = Workspace::demo();
        workspace.note_mut(1).unwrap().body = "An edited note, with café and 🌱".into();
        workspace.settings.dark_mode = true;
        // A workspace written before the banner could be closed for good
        // has no such setting: the banner is shown.
        assert!(!workspace.settings.space_banner_dismissed);
        let older: Settings = serde_json::from_str(r#"{"dark_mode":true}"#).unwrap();
        assert!(older.dark_mode && !older.space_banner_dismissed);
        workspace.settings.space_banner_dismissed = true;
        workspace.save_to(&path).unwrap();
        let restored = Workspace::load_from(&path).unwrap();
        assert_eq!(
            restored.note(1).unwrap().body,
            "An edited note, with café and 🌱"
        );
        assert!(restored.settings.dark_mode);
        // Closing the space banner survives a restart.
        assert!(restored.settings.space_banner_dismissed);
        assert_eq!(restored.folders.len(), workspace.folders.len());
        assert_eq!(restored.notes.len(), workspace.notes.len());
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn search_combines_folder_favorites_and_trash_filters() {
        let mut workspace = Workspace::demo();
        workspace
            .note_mut(1)
            .unwrap()
            .transcript
            .push_str(" UNIQUE QUERY");
        assert_eq!(
            workspace
                .visible_notes(Some(101), true, " unique query ")
                .iter()
                .map(|note| note.id)
                .collect::<Vec<_>>(),
            vec![1]
        );
        assert!(
            workspace
                .visible_notes(Some(102), true, "unique query")
                .is_empty()
        );
        workspace.note_mut(1).unwrap().trashed = true;
        assert!(
            workspace
                .visible_notes(None, false, "unique query")
                .is_empty()
        );
        let dates: Vec<_> = workspace
            .visible_notes(None, false, "")
            .iter()
            .map(|note| note.date.clone())
            .collect();
        assert!(dates.windows(2).all(|dates| dates[0] >= dates[1]));
    }

    #[test]
    fn creates_unique_note_and_folder_ids_even_after_import() {
        let mut workspace = Workspace::demo();
        let folder = workspace.add_folder("  Research  ".into());
        let first = workspace.create_note();
        let second = workspace.create_note();
        assert_ne!(folder, first);
        assert_ne!(first, second);
        assert_eq!(workspace.folders.last().unwrap().name, "Research");
        assert_eq!(workspace.note(second).unwrap().title, "Untitled note");
        workspace.notes[0].id = u64::MAX;
        let gap = workspace.create_note();
        assert_ne!(gap, u64::MAX);
        assert_eq!(
            workspace.notes.iter().filter(|note| note.id == gap).count(),
            1
        );
    }

    #[test]
    fn missing_fields_receive_safe_defaults() {
        let workspace: Workspace =
            serde_json::from_str(r#"{"notes":[{"id":7,"title":"Imported"}]}"#).unwrap();
        assert_eq!(workspace.notes[0].id, 7);
        assert!(!workspace.notes[0].trashed);
        assert!(!workspace.settings.dark_mode);
        assert!(workspace.settings.auto_enhance);
        assert!(workspace.settings.show_upcoming);
        assert!(workspace.folders.is_empty());
    }

    #[test]
    fn legacy_notes_default_participants_and_unicode_attendees_persist() {
        let mut workspace: Workspace = serde_json::from_str(
            r#"{"notes":[{"id":7,"title":"Imported meeting","body":"Existing notes"}]}"#,
        )
        .unwrap();
        assert!(workspace.note(7).unwrap().participants.is_empty());

        let attendees = vec![
            "Zoë Chen".to_string(),
            "李明".to_string(),
            "Maya 🌱".to_string(),
        ];
        workspace.note_mut(7).unwrap().participants = attendees.clone();
        let directory = test_directory("participants");
        let path = directory.join("workspace.json");
        workspace.save_to(&path).unwrap();
        let restored = Workspace::load_from(&path).unwrap();

        assert_eq!(restored.note(7).unwrap().participants, attendees);
        assert_eq!(restored.note(7).unwrap().body, "Existing notes");
        assert_eq!(restored.note(7).unwrap().title, "Imported meeting");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn malformed_workspace_is_preserved_before_save() {
        let directory = test_directory("corrupt");
        let path = directory.join("workspace.json");
        fs::create_dir_all(&directory).unwrap();
        fs::write(&path, b"{ unfinished user data").unwrap();
        assert!(Workspace::load_from(&path).is_err());
        Workspace::demo().save_to(&path).unwrap();
        assert!(Workspace::load_from(&path).is_ok());
        let backup = fs::read_dir(&directory)
            .unwrap()
            .filter_map(Result::ok)
            .find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("workspace.corrupt-")
            })
            .unwrap();
        assert_eq!(fs::read(backup.path()).unwrap(), b"{ unfinished user data");
        fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn transcript_revisions_preserve_old_files_and_flag_only_new_content() {
        let mut note: Note = serde_json::from_str(r#"{"id":7,"body":"Existing notes","enhanced":"Saved outline","transcript":"Me 00:00\nFirst thought"}"#).unwrap();
        assert_eq!(note.transcript_revision, 0);
        assert_eq!(note.generated_revision, None);
        assert!(!note.needs_regeneration());
        assert!(!note.replace_transcript(note.transcript.clone()));
        assert!(note.append_transcript("Someone else  00:20\nA new decision"));
        assert_eq!(note.transcript_revision, 1);
        assert_eq!(note.generated_revision, Some(0));
        assert!(note.needs_regeneration());
        let stored = serde_json::to_value(&note).unwrap();
        assert_eq!(stored["transcriptRevision"], 1);
        assert_eq!(stored["generatedRevision"], 0);
        let loaded: Note = serde_json::from_value(stored).unwrap();
        assert_eq!(loaded.enhanced, "Saved outline");
        assert!(loaded.needs_regeneration());
    }
}
