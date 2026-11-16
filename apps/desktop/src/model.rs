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
    pub settings: Settings,
    /// Bumped by every mutable access to notes, so lists derived
    /// from them can be cached between renders. Not persisted.
    #[serde(skip)]
    revision: u64,
    /// Server mode: one entry per account whose file has been read (or
    /// refused) in this launch, by slot. Empty in local mode, where the
    /// workspace is one file and ids are the file's own.
    #[serde(skip)]
    pub slots: std::collections::BTreeMap<u16, Slot>,
    /// Slots whose rows are filtered out of the lists (an account that needs
    /// a sign-in). A view filter only: the notes stay loaded and are written
    /// as usual.
    #[serde(skip)]
    pub hidden: std::collections::HashSet<u16>,
    /// Server mode: the device-level settings beside the accounts'
    /// directories (`settings.json`).
    #[serde(skip)]
    pub device: Option<crate::device_settings::Device>,
    /// Local mode: no new note or folder gets an id below this. A note
    /// deleted forever raises it past its own id, so a later note never
    /// takes over that id (and whatever is still filed under it, such as
    /// the note's chat). Server mode keeps its own per account (`nextId`).
    #[serde(rename = "nextId", skip_serializing_if = "is_zero")]
    next_local_id: u64,
}

fn is_zero(value: &u64) -> bool {
    *value == 0
}

/// One file a save has to write.
pub struct PendingWrite {
    pub path: PathBuf,
    pub data: Vec<u8>,
    /// An account's `workspace.json` (written with the checks that file
    /// gets); otherwise a small device file.
    pub workspace: bool,
}

/// What is known in memory about one account's `workspace.json`.
#[derive(Debug, Clone)]
pub struct Slot {
    pub account: crate::accounts::AccountId,
    /// The account's `workspace.json`.
    pub path: PathBuf,
    /// The account's `recordings` directory.
    pub recordings: PathBuf,
    /// The file id the next new note of this account takes. Persisted as
    /// `nextId`; never reused, even after a note leaves the file.
    pub next_id: u64,
    pub state: SlotState,
    /// What was last read from or handed to the writer for this file, so a
    /// slot whose notes did not change is not written again.
    pub written: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotState {
    Loaded,
    /// The file holds an id that cannot be a note's (0, 2^40 - 1 or more, or
    /// one used twice). It was not loaded and is never written: it is left
    /// exactly as it is, and the account says so.
    Unreadable(String),
    /// The account was signed out of or removed. Its notes are gone from
    /// memory and the encoder refuses to write it.
    Dead,
}

/// What an account's `workspace.json` holds. Ids in it are always file ids.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct AccountFile {
    notes: Vec<Note>,
    settings: Option<Settings>,
    #[serde(rename = "nextId")]
    next_id: u64,
}

fn note_file_id<S: serde::Serializer>(id: &u64, serializer: S) -> Result<S::Ok, S::Error> {
    // In an account's file, always the file id: a handle never reaches a
    // file. Local mode's ids are written as they are.
    serializer.serialize_u64(crate::accounts::handle::on_disk(*id))
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Note {
    #[serde(serialize_with = "note_file_id")]
    pub id: u64,
    pub title: String,
    pub body: String,
    pub enhanced: String,
    /// Unsynchronized user edits to generated notes survive reloads and late
    /// provider completions. Only an acknowledgement of this text clears it.
    #[serde(default, rename = "enhancedDirty")]
    pub enhanced_dirty: bool,
    /// The generated original of notes edited here, by which the edited
    /// blocks are told from the model's (see `authorship`). Kept with this
    /// device's copy of the note only.
    #[serde(
        default,
        rename = "enhancedAuthorship",
        skip_serializing_if = "Option::is_none"
    )]
    pub enhanced_authorship: Option<crate::authorship::Authorship>,
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
    pub favorite: bool,
    pub trashed: bool,
    /// When the note's own server answered that it no longer has the note
    /// (a 404 for the note itself, after a complete listing without it). It
    /// is then in Trash as "Deleted on the server"; nothing of it is erased.
    #[serde(default, rename = "serverMissingSince", skip_serializing_if = "Option::is_none")]
    pub server_missing_since: Option<String>,
    /// The title or the text was changed here and no save of it has been
    /// answered yet. Such a note is queued again when its account is Ready.
    #[serde(default, rename = "unsentEdits", skip_serializing_if = "std::ops::Not::not")]
    pub unsent_edits: bool,
    /// A recording was stopped here and the server has not confirmed the Stop
    /// yet; it is sent again when the account is Ready. (No audio is ever
    /// uploaded: speech is transcribed live.)
    #[serde(default, rename = "unsentCapture", skip_serializing_if = "std::ops::Not::not")]
    pub unsent_capture: bool,
    /// Stop was pressed (or the recording ended with the app) and the server
    /// has not been told yet: it is told once the live stream has delivered
    /// every captured sample, so that it transcribes all of them. Kept on
    /// disk, so a quit or a crash before then is finished at the next launch.
    #[serde(default, rename = "pendingStop", skip_serializing_if = "Option::is_none")]
    pub pending_stop: Option<PendingStop>,
    /// Captures (by ID) whose audio has not all reached the server. Their
    /// files hold the rest; a stream of them resumes where the server is.
    #[serde(default, rename = "livePending", skip_serializing_if = "Vec::is_empty")]
    pub live_pending: Vec<String>,
    /// Stretches of the recording that were captured but have no
    /// transcript: what the server lists, and what this Mac's stream was
    /// told or had to skip. Note-timeline milliseconds, merged per channel.
    #[serde(default, rename = "transcriptGaps", skip_serializing_if = "Vec::is_empty")]
    pub transcript_gaps: Vec<meeting_notes_api_types::TranscriptGapDTO>,
}

/// What a Stop pressed here still owes the server (see [`Note::pending_stop`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PendingStop {
    /// Tell the server the recording stopped.
    Stop,
    /// Tell it, then have the notes written (the Stop button).
    Generate,
}

impl Note {
    /// Something of this note exists only on this Mac: an edit, or a Stop
    /// the server has not confirmed.
    pub fn has_unsent_work(&self) -> bool {
        self.unsent_edits
            || self.unsent_capture
            || self.enhanced_dirty
            || self.pending_stop.is_some()
            || !self.live_pending.is_empty()
    }

    /// Add stretches without transcript, merging those that touch or overlap
    /// on the same channel. Returns whether anything changed.
    pub fn add_transcript_gaps(
        &mut self,
        gaps: impl IntoIterator<Item = meeting_notes_api_types::TranscriptGapDTO>,
    ) -> bool {
        let before = self.transcript_gaps.clone();
        let mut all: Vec<_> = self.transcript_gaps.drain(..).chain(gaps).collect();
        all.retain(|gap| gap.to_ms > gap.from_ms);
        all.sort_by_key(|gap| (gap.channel != meeting_notes_api_types::AudioChannel::Mic, gap.from_ms));
        for gap in all {
            match self.transcript_gaps.last_mut() {
                Some(last) if last.channel == gap.channel && gap.from_ms <= last.to_ms => {
                    last.to_ms = last.to_ms.max(gap.to_ms);
                }
                _ => self.transcript_gaps.push(gap),
            }
        }
        self.transcript_gaps != before
    }

    /// The day this note sorts and is grouped under: its timeline day, but
    /// never later than tomorrow by this Mac's clock, so a server cannot pin
    /// its notes to the top with a date far in the future. An honest note
    /// is never dated after tomorrow.
    pub fn sort_day(&self) -> String {
        self.sort_day_on(Local::now().date_naive())
    }

    pub fn sort_day_on(&self, today: chrono::NaiveDate) -> String {
        let day = self.timeline_date();
        let tomorrow = today
            .succ_opt()
            .unwrap_or(today)
            .format("%Y-%m-%d")
            .to_string();
        match chrono::NaiveDate::parse_from_str(&day, "%Y-%m-%d") {
            Ok(_) if day > tomorrow => tomorrow,
            _ => day,
        }
    }

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
            enhanced_authorship: self.enhanced_authorship.clone(),
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
            favorite: self.favorite,
            trashed: self.trashed,
            server_missing_since: self.server_missing_since.clone(),
            unsent_edits: self.unsent_edits,
            unsent_capture: self.unsent_capture,
            pending_stop: self.pending_stop,
            live_pending: self.live_pending.clone(),
            transcript_gaps: Vec::new(),
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

    /// Take the title the server gave a note nobody had named (it names one
    /// when its notes are generated). Only a note whose title is still the
    /// empty or placeholder one that was sent with the request takes it: a
    /// title typed since then, or any real title, is the person's and stays.
    /// Returns whether the title changed.
    pub fn adopt_server_title(&mut self, submitted: &str, remote: &str) -> bool {
        let unnamed = crate::meeting::is_default_note_title;
        if self.title != submitted || !unnamed(&self.title) || unnamed(remote) {
            return false;
        }
        self.title = remote.to_owned();
        true
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// The colour scheme the person chose. `System` follows macOS.
    pub theme: ThemeMode,
    /// The scheme in use, resolved from `theme` ([`Settings::resolve_theme`]).
    /// This is what the rest of the app reads.
    pub dark_mode: bool,
    pub auto_enhance: bool,
    /// The note about privacy on the private space page has been closed.
    /// Kept so that it stays closed after a restart.
    pub space_banner_dismissed: bool,
    /// The same for the banner on the team space page, closed separately.
    pub team_banner_dismissed: bool,
    /// The workspace's name as its server last gave it, so the sidebar can
    /// name the team space before the server has been asked again.
    pub team_space_name: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: ThemeMode::System,
            dark_mode: false,
            auto_enhance: true,
            space_banner_dismissed: false,
            team_banner_dismissed: false,
            team_space_name: String::new(),
        }
    }
}

/// The interface colour scheme: the Mac's own, or one that is forced.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

/// Anything that is not a scheme this build knows reads as `System`, so one
/// odd value never costs the other settings in the file.
impl<'de> Deserialize<'de> for ThemeMode {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        Ok(match value.as_str() {
            Some("light") => ThemeMode::Light,
            Some("dark") => ThemeMode::Dark,
            _ => ThemeMode::System,
        })
    }
}

impl ThemeMode {
    pub const ALL: [ThemeMode; 3] = [ThemeMode::System, ThemeMode::Light, ThemeMode::Dark];

    pub fn label(self) -> &'static str {
        match self {
            ThemeMode::System => "System",
            ThemeMode::Light => "Light",
            ThemeMode::Dark => "Dark",
        }
    }

    /// Whether the interface is dark, given whether macOS is.
    pub fn dark(self, system_dark: bool) -> bool {
        match self {
            ThemeMode::System => system_dark,
            ThemeMode::Light => false,
            ThemeMode::Dark => true,
        }
    }
}

impl Settings {
    /// Choose a scheme. `dark_mode` follows at once for a forced scheme, and
    /// with the next [`Self::resolve_theme`] for `System`.
    pub fn set_theme(&mut self, theme: ThemeMode) {
        self.theme = theme;
        if theme == ThemeMode::Light || theme == ThemeMode::Dark {
            self.dark_mode = theme == ThemeMode::Dark;
        }
    }

    /// Force light or dark, whatever macOS shows (`--dark`, the tours).
    pub fn force_dark(&mut self, dark: bool) {
        self.set_theme(if dark { ThemeMode::Dark } else { ThemeMode::Light });
    }

    /// Bring `dark_mode` in line with the chosen scheme, given whether macOS
    /// is dark. Returns whether it changed.
    pub fn resolve_theme(&mut self, system_dark: bool) -> bool {
        let dark = self.theme.dark(system_dark);
        let changed = self.dark_mode != dark;
        self.dark_mode = dark;
        changed
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
        if crate::server::enabled() {
            // Each account's notes join when its session has been verified
            // in this launch (`load_account`).
            return match crate::accounts::Paths::of_environment() {
                Some(paths) => Self::for_accounts(&paths),
                None => Self::default(),
            };
        }
        local_workspace_path()
            .and_then(|path| Self::load_from(&path))
            .unwrap_or_else(|_| Self::demo())
    }

    /// The workspace of server mode before any account's notes have joined
    /// it: the device-level settings, and no notes.
    pub fn for_accounts(paths: &crate::accounts::Paths) -> Self {
        let device = crate::device_settings::Device::load(paths);
        Self {
            settings: device.settings.clone().unwrap_or_default(),
            device: Some(device),
            ..Self::default()
        }
    }

    /// Server mode: ids in memory are handles, and every account has its
    /// own file.
    pub fn by_account(&self) -> bool {
        self.device.is_some()
    }

    /// Read one account's notes into the workspace, each id becoming a
    /// handle in the account's slot. Returns how many notes joined.
    ///
    /// If any id in the file is 0, above the highest a file may hold, or
    /// used twice, the file is **not loaded and not written**: it is left
    /// exactly as it is and the reason is returned. Nothing is renumbered.
    pub fn load_account(
        &mut self,
        slot: u16,
        account: &crate::accounts::AccountId,
        paths: &crate::accounts::Paths,
    ) -> Result<usize, String> {
        use crate::accounts::handle;
        if let Some(existing) = self.slots.get(&slot) {
            return match &existing.state {
                SlotState::Loaded => Ok(0),
                SlotState::Unreadable(reason) => Err(reason.clone()),
                SlotState::Dead => Err("That account was removed".into()),
            };
        }
        if handle::handle(slot, 1).is_none() {
            return Err("That account has no place in memory".into());
        }
        crate::store::wait_idle();
        let path = paths.workspace(account);
        let recordings = paths.recordings(account);
        let mut entry = Slot {
            account: account.clone(),
            path: path.clone(),
            recordings: recordings.clone(),
            next_id: 1,
            state: SlotState::Loaded,
            written: None,
        };
        let refuse = |this: &mut Self, mut entry: Slot, reason: String| {
            entry.state = SlotState::Unreadable(reason.clone());
            this.slots.insert(slot, entry);
            Err(reason)
        };
        let bytes = match fs::read(&path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return refuse(self, entry, format!("Cannot read workspace: {error}")),
        };
        let mut file = match &bytes {
            Some(bytes) => match serde_json::from_slice::<AccountFile>(bytes) {
                Ok(file) => file,
                // Malformed JSON keeps today's rule: the first save copies
                // it aside before replacing it (`write_workspace`).
                Err(_) => AccountFile::default(),
            },
            None => AccountFile::default(),
        };
        let mut seen = std::collections::HashSet::new();
        if let Some(bad) = file
            .notes
            .iter()
            .find(|note| !(1..=handle::MAX_FILE_ID).contains(&note.id) || !seen.insert(note.id))
        {
            let reason = format!(
                "a note in it has the id {}, which no note of this file can have",
                bad.id
            );
            return refuse(self, entry, reason);
        }
        // Never an id that was ever used: the stored high-water mark, the
        // notes in the file, and the names an older build may have left on
        // disk after rewriting the file without the mark.
        entry.next_id = file
            .next_id
            .max(file.notes.iter().map(|note| note.id + 1).max().unwrap_or(1))
            .max(highest_id_on_disk(&recordings) + 1)
            .max(1);
        // Device-level settings are seeded from the first account read.
        if let Some(device) = &mut self.device {
            if device.settings.is_none() {
                let seeded = file.settings.clone().unwrap_or_default();
                device.settings = Some(seeded.clone());
                // A `--dark` launch has already chosen; keep what is shown.
                let forced = self.settings.theme == ThemeMode::Dark;
                self.settings = seeded;
                if forced {
                    self.settings.force_dark(true);
                }
            }
        }
        // Native finalization can complete during app shutdown after GPUI's
        // last workspace save. The owner reference already exists; reconcile
        // metadata. On disk a manifest names its note by file id.
        for note in &mut file.notes {
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
        let count = file.notes.len();
        for mut note in file.notes {
            // A capture's manifest keeps naming its note by file id, in
            // memory as on disk.
            note.id = handle::handle(slot, note.id).expect("checked above");
            self.notes.push(note);
        }
        self.revision = self.revision.wrapping_add(1);
        self.slots.insert(slot, entry);
        // What is on disk is what would be written now: nothing to write
        // until something changes. (A file from an older build differs, and
        // is brought up to date by the first save.)
        if let (Some(bytes), Ok(encoded)) = (bytes, self.encode_slot(slot)) {
            let same = bytes == encoded;
            if let Some(entry) = self.slots.get_mut(&slot) {
                entry.written = same.then(|| crate::store::fingerprint(&encoded));
            }
        }
        Ok(count)
    }

    /// One account's file as it is written: its notes with their file ids,
    /// a copy of the settings, and the id high-water mark.
    pub fn encode_slot(&self, slot: u16) -> Result<Vec<u8>, String> {
        struct Notes<'a>(&'a [Note], u16);
        impl Serialize for Notes<'_> {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.collect_seq(self.0.iter().filter(|note| {
                    crate::accounts::handle::slot_of(note.id) == Some(self.1)
                }))
            }
        }
        #[derive(Serialize)]
        struct File<'a> {
            notes: Notes<'a>,
            settings: &'a Settings,
            #[serde(rename = "nextId")]
            next_id: u64,
        }
        let entry = self.slots.get(&slot).ok_or("That account's notes are not loaded")?;
        if entry.state != SlotState::Loaded {
            return Err("That account's notes are not written".into());
        }
        crate::accounts::handle::encoding_account_file(|| serde_json::to_vec_pretty(&File {
            notes: Notes(&self.notes, slot),
            settings: &self.settings,
            next_id: entry.next_id,
        }))
        .map_err(|error| format!("Cannot serialize workspace: {error}"))
    }

    /// A new note of the account in `slot`. Its id is the account's next
    /// file id, which is never one that was used before. `None` when the
    /// account's notes are not loaded or it has run out of ids.
    pub fn create_note_in(&mut self, slot: u16) -> Option<u64> {
        let entry = self.slots.get_mut(&slot)?;
        if entry.state != SlotState::Loaded {
            return None;
        }
        let id = crate::accounts::handle::handle(slot, entry.next_id)?;
        entry.next_id += 1;
        self.revision = self.revision.wrapping_add(1);
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
        Some(id)
    }

    /// An account was signed out of or removed: its notes leave memory and
    /// its slot is dead, so nothing writes its file again.
    pub fn drop_slot(&mut self, slot: u16) {
        self.revision = self.revision.wrapping_add(1);
        self.notes
            .retain(|note| crate::accounts::handle::slot_of(note.id) != Some(slot));
        if let Some(entry) = self.slots.get_mut(&slot) {
            entry.state = SlotState::Dead;
        }
        self.hidden.remove(&slot);
    }

    /// Show or hide an account's rows in the lists. A view filter.
    pub fn set_hidden(&mut self, slot: u16, hidden: bool) {
        let changed = if hidden {
            self.hidden.insert(slot)
        } else {
            self.hidden.remove(&slot)
        };
        if changed {
            self.revision = self.revision.wrapping_add(1);
        }
    }

    /// The person changed a setting: in server mode the settings are this
    /// Mac's from now on, whether or not an account ever seeded them.
    pub fn settings_changed(&mut self) {
        if let Some(device) = &mut self.device {
            device.settings_touched(&self.settings);
        }
    }

    /// Whether a note's row is filtered out of the lists.
    pub fn is_hidden(&self, note: &Note) -> bool {
        !self.hidden.is_empty()
            && crate::accounts::handle::slot_of(note.id).is_some_and(|slot| self.hidden.contains(&slot))
    }

    /// The notes of one account, as they are in memory.
    pub fn notes_of(&self, slot: u16) -> impl Iterator<Item = &Note> {
        self.notes
            .iter()
            .filter(move |note| crate::accounts::handle::slot_of(note.id) == Some(slot))
    }

    /// What has to reach the disk in server mode: each loaded, live slot
    /// whose notes changed, and the device settings if they changed. A slot that
    /// is not loaded, or was marked dead by sign-out or removal, is never
    /// written. Hidden slots are loaded and are written normally.
    pub fn pending_writes(&mut self) -> Result<Vec<PendingWrite>, String> {
        let mut jobs = Vec::new();
        let slots: Vec<u16> = self
            .slots
            .iter()
            .filter(|(_, entry)| entry.state == SlotState::Loaded)
            .map(|(slot, _)| *slot)
            .collect();
        for slot in slots {
            let data = self.encode_slot(slot)?;
            let print = crate::store::fingerprint(&data);
            let entry = self.slots.get_mut(&slot).expect("listed above");
            if entry.written != Some(print) {
                entry.written = Some(print);
                jobs.push(PendingWrite {
                    path: entry.path.clone(),
                    data,
                    workspace: true,
                });
            }
        }
        if let Some(device) = &mut self.device {
            for (path, data) in device.pending(&self.settings) {
                jobs.push(PendingWrite {
                    path,
                    data,
                    workspace: false,
                });
            }
        }
        Ok(jobs)
    }

    /// A write failed: its content is still only in memory, so the next
    /// save must hand it over again.
    pub fn forget_written(&mut self, path: &Path) {
        for entry in self.slots.values_mut() {
            if entry.path == path {
                entry.written = None;
            }
        }
        if let Some(device) = &mut self.device {
            device.forget(path);
        }
    }

    /// Why an account's notes are not shown, when its file was refused.
    pub fn unreadable(&self, slot: u16) -> Option<&str> {
        match &self.slots.get(&slot)?.state {
            SlotState::Unreadable(reason) => Some(reason),
            _ => None,
        }
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
                            favorite,
                            trashed: false,
                            ..Note::default()
                        }
                    },
                )
                .collect(),
            settings: Settings::default(),
            ..Self::default()
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

    /// Take a note out of the workspace for good (Delete forever in
    /// Trash). Its id is never given to another note.
    pub fn remove_note(&mut self, id: u64) -> Option<Note> {
        let index = self.notes.iter().position(|note| note.id == id)?;
        self.revision = self.revision.wrapping_add(1);
        if !self.by_account() {
            self.next_local_id = self.next_local_id.max(id.saturating_add(1));
        }
        Some(self.notes.remove(index))
    }

    pub fn note(&self, id: u64) -> Option<&Note> {
        self.notes.iter().find(|note| note.id == id)
    }

    pub fn note_mut(&mut self, id: u64) -> Option<&mut Note> {
        self.revision = self.revision.wrapping_add(1);
        self.notes.iter_mut().find(|note| note.id == id)
    }

    /// Identity of the notes as they are now: changes whenever they may
    /// have changed, and whenever the workspace itself is replaced.
    pub fn revision(&self) -> (u64, usize, usize) {
        (
            self.revision,
            self.notes.len(),
            self.notes.as_ptr() as usize,
        )
    }

    /// All untrashed notes, optionally restricted by favorite and text.
    /// Results are newest first; searching covers the title and all note text.
    pub fn visible_notes(&self, favorites_only: bool, query: &str) -> Vec<&Note> {
        let query = query.trim().to_lowercase();
        let mut notes: Vec<_> = self
            .notes
            .iter()
            .filter(|note| {
                !note.trashed
                    && !self.is_hidden(note)
                    && (!favorites_only || note.favorite)
                    && (query.is_empty()
                        || [&note.title, &note.body, &note.enhanced, &note.transcript]
                            .iter()
                            .any(|text| text.to_lowercase().contains(&query)))
            })
            .collect();
        let today = Local::now().date_naive();
        notes.sort_by(|a, b| {
            b.sort_day_on(today)
                .cmp(&a.sort_day_on(today))
                .then_with(|| b.server_created_at.cmp(&a.server_created_at))
                .then_with(|| b.id.cmp(&a.id))
        });
        notes
    }

    fn next_id(&self) -> u64 {
        let used = |id| self.notes.iter().any(|note| note.id == id);
        let highest = self
            .notes
            .iter()
            .map(|note| note.id)
            .max()
            .unwrap_or(0);
        if let Some(next) = highest.checked_add(1) {
            return next.max(self.next_local_id);
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

    /// A server-mode workspace whose accounts in `slots` are loaded and
    /// empty, with no files behind them.
    #[cfg(test)]
    pub(crate) fn with_slots(slots: &[u16]) -> Self {
        let mut workspace = Self::default();
        for slot in slots {
            let account = crate::accounts::AccountId::of(
                "https://test.example",
                &uuid::Uuid::from_u128(u128::from(*slot)).hyphenated().to_string(),
            );
            let root = std::env::temp_dir().join(format!("cereal-no-such-{}", account.as_str()));
            workspace.slots.insert(
                *slot,
                Slot {
                    account,
                    path: root.join("workspace.json"),
                    recordings: root.join("recordings"),
                    next_id: 1,
                    state: SlotState::Loaded,
                    written: None,
                },
            );
        }
        workspace
    }

    #[cfg(test)]
    fn save_to(&self, path: &Path) -> Result<(), String> {
        write_workspace(path, &self.encode()?, true)
    }
}

/// Where local mode's workspace lives ("this Mac only").
pub fn workspace_file() -> Result<PathBuf, String> {
    local_workspace_path()
}

/// The highest note id any name under an account's `recordings` directory
/// carries: `note-<n>` capture directories and `server-associations/*/<n>.txt`
/// sidecars. An id that has a name on disk was used, and is never given to
/// another note, even if an older build rewrote the file without `nextId`.
fn highest_id_on_disk(recordings: &Path) -> u64 {
    let names = |directory: &Path| -> Vec<String> {
        fs::read_dir(directory)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| entry.file_name().into_string().ok())
            .collect()
    };
    let captures = names(recordings)
        .into_iter()
        .filter_map(|name| name.strip_prefix("note-")?.parse::<u64>().ok());
    let sidecars = names(&recordings.join("server-associations"))
        .into_iter()
        .flat_map(|namespace| names(&recordings.join("server-associations").join(namespace)))
        .filter_map(|name| name.strip_suffix(".txt")?.parse::<u64>().ok());
    captures
        .chain(sidecars)
        .filter(|id| *id <= crate::accounts::handle::MAX_FILE_ID)
        .max()
        .unwrap_or(0)
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

#[cfg(test)]
pub fn account_cache_path(root: &Path, identity: &str) -> PathBuf {
    let namespace = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, identity.as_bytes());
    root.join("servers")
        .join(namespace.to_string())
        .join("workspace.json")
}
fn local_workspace_path() -> Result<PathBuf, String> {
    let root = if let Some(directory) =
        crate::environment::var_os("CEREAL_DATA_DIR").filter(|v| !v.is_empty())
    {
        PathBuf::from(directory)
    } else {
        let home = std::env::var_os("HOME").ok_or("Cannot determine home directory")?;
        PathBuf::from(home).join("Library/Application Support/Cereal")
    };
    Ok(root.join("workspace.json"))
}

/// Where captures are kept: an account's `recordings` directory for a note
/// of an account, local mode's own otherwise. Paths take the account; there
/// is no "current" one.
pub fn audio_directory(account: Option<&crate::accounts::AccountId>) -> Result<PathBuf, String> {
    match account {
        Some(account) => crate::accounts::Paths::of_environment()
            .map(|paths| paths.recordings(account))
            .ok_or_else(|| "Cannot determine home directory".to_string()),
        None if crate::server::enabled() => {
            Err("Sign in before accessing private cached notes or audio".into())
        }
        None => Ok(local_workspace_path()?
            .parent()
            .ok_or("Workspace has no parent")?
            .join("recordings")),
    }
}

/// The capture directory of a note in memory: its account's, by its handle.
pub fn audio_directory_of(note_id: u64) -> Result<PathBuf, String> {
    if !crate::server::enabled() {
        return audio_directory(None);
    }
    let account = crate::accounts::of_note(note_id)
        .filter(|account| account.user_id.is_some())
        .ok_or("This note does not belong to an account on this Mac")?;
    audio_directory(Some(&account.id))
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
                .visible_notes(false, "")
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
        workspace.settings.force_dark(true);
        // A workspace written before the banner could be closed for good
        // has no such setting: the banner is shown.
        assert!(!workspace.settings.space_banner_dismissed);
        let older: Settings = serde_json::from_str(r#"{"dark_mode":true}"#).unwrap();
        assert!(older.dark_mode && !older.space_banner_dismissed);
        // It has no scheme either: it follows macOS, and a value from a
        // build this one does not know does too.
        assert_eq!(older.theme, ThemeMode::System);
        let newer: Settings =
            serde_json::from_str(r#"{"theme":"sepia","auto_enhance":false}"#).unwrap();
        assert!(newer.theme == ThemeMode::System && !newer.auto_enhance);
        workspace.settings.space_banner_dismissed = true;
        workspace.save_to(&path).unwrap();
        let restored = Workspace::load_from(&path).unwrap();
        assert_eq!(
            restored.note(1).unwrap().body,
            "An edited note, with café and 🌱"
        );
        assert!(restored.settings.dark_mode);
        assert_eq!(restored.settings.theme, ThemeMode::Dark);
        // Closing the space banner survives a restart.
        assert!(restored.settings.space_banner_dismissed);
        assert_eq!(restored.notes.len(), workspace.notes.len());
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn search_combines_favorites_and_trash_filters() {
        let mut workspace = Workspace::demo();
        workspace
            .note_mut(1)
            .unwrap()
            .transcript
            .push_str(" UNIQUE QUERY");
        workspace.note_mut(2).unwrap().body.push_str(" unique query");
        assert_eq!(
            workspace
                .visible_notes(true, " unique query ")
                .iter()
                .map(|note| note.id)
                .collect::<Vec<_>>(),
            vec![1]
        );
        assert_eq!(workspace.visible_notes(false, "unique query").len(), 2);
        workspace.note_mut(1).unwrap().trashed = true;
        assert!(workspace.visible_notes(true, "unique query").is_empty());
        let dates: Vec<_> = workspace
            .visible_notes(false, "")
            .iter()
            .map(|note| note.date.clone())
            .collect();
        assert!(dates.windows(2).all(|dates| dates[0] >= dates[1]));
    }

    #[test]
    fn creates_unique_note_ids_even_after_import() {
        let mut workspace = Workspace::demo();
        let first = workspace.create_note();
        let second = workspace.create_note();
        assert_ne!(first, second);
        assert_eq!(workspace.note(second).unwrap().title, "Untitled note");
        workspace.notes[0].id = u64::MAX;
        let gap = workspace.create_note();
        assert_ne!(gap, u64::MAX);
        assert_eq!(
            workspace.notes.iter().filter(|note| note.id == gap).count(),
            1
        );
    }

    /// A fresh install follows macOS, live; Light and Dark hold whatever
    /// macOS shows; and the choice is what is saved.
    #[test]
    fn the_theme_follows_macos_unless_one_is_forced() {
        let mut settings = Settings::default();
        assert_eq!(settings.theme, ThemeMode::System);
        assert!(settings.resolve_theme(true) && settings.dark_mode);
        assert!(!settings.resolve_theme(true), "nothing changed");
        assert!(settings.resolve_theme(false) && !settings.dark_mode);
        settings.set_theme(ThemeMode::Dark);
        assert!(settings.dark_mode, "a forced scheme applies at once");
        assert!(!settings.resolve_theme(false) && settings.dark_mode);
        settings.set_theme(ThemeMode::Light);
        assert!(!settings.resolve_theme(true) && !settings.dark_mode);
        settings.force_dark(true);
        assert_eq!(settings.theme, ThemeMode::Dark);
        // Back to System: the scheme in use stays until macOS is asked.
        settings.set_theme(ThemeMode::System);
        assert!(settings.dark_mode);
        assert!(settings.resolve_theme(false) && !settings.dark_mode);
        for (theme, saved) in [
            (ThemeMode::System, "system"),
            (ThemeMode::Light, "light"),
            (ThemeMode::Dark, "dark"),
        ] {
            settings.set_theme(theme);
            let json = serde_json::to_value(&settings).unwrap();
            assert_eq!(json["theme"], saved);
            let read: Settings = serde_json::from_value(json).unwrap();
            assert_eq!(read.theme, theme);
            assert_eq!(theme.label().to_lowercase(), saved);
        }
        assert_eq!(ThemeMode::ALL.map(ThemeMode::label), ["System", "Light", "Dark"]);
    }

    #[test]
    fn missing_fields_receive_safe_defaults() {
        let workspace: Workspace =
            serde_json::from_str(r#"{"notes":[{"id":7,"title":"Imported"}]}"#).unwrap();
        assert_eq!(workspace.notes[0].id, 7);
        assert!(!workspace.notes[0].trashed);
        assert!(!workspace.settings.dark_mode);
        assert!(workspace.settings.auto_enhance);
    }

    /// Notes that were in folders are not lost: each one loads and is listed
    /// in My notes (Trash keeps what was trashed), and saving keeps them all.
    #[test]
    fn a_workspace_with_notes_in_folders_loads_and_lists_every_note() {
        let directory = std::env::temp_dir().join(format!("cereal-folders-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("workspace.json");
        let fixture = include_str!("../fixtures/workspace-with-folders.json");
        fs::write(&path, fixture).unwrap();
        let listed = |workspace: &Workspace| -> Vec<String> {
            workspace.visible_notes(false, "").iter().map(|note| note.title.clone()).collect()
        };
        let expected = [
            "Kickoff with Northstar",
            "Hiring sync",
            "Filed in a folder that no longer exists",
            "Never filed",
        ];
        let workspace = Workspace::load_from(&path).unwrap();
        assert_eq!(workspace.notes.len(), 5, "no note is dropped");
        assert_eq!(listed(&workspace), expected);
        assert_eq!(
            workspace.visible_notes(true, "").iter().map(|note| note.id).collect::<Vec<_>>(),
            [1],
            "a favorite in a folder is still a favorite"
        );
        assert!(workspace.note(5).unwrap().trashed, "a trashed note stays in Trash");
        assert!(workspace.settings.dark_mode && workspace.settings.space_banner_dismissed);
        // Searching finds notes that were filed.
        assert_eq!(workspace.visible_notes(false, "candidates").len(), 1);
        workspace.save_to(&path).unwrap();
        let written: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert!(written.get("folders").is_none());
        assert!(written["notes"].as_array().unwrap().iter().all(|note| note.get("folder_id").is_none()));
        let again = Workspace::load_from(&path).unwrap();
        assert_eq!(again.notes.len(), 5);
        assert_eq!(listed(&again), expected);
        fs::remove_dir_all(directory).unwrap();
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

/// The account side of the workspace: handles, per-account files, the id
/// high-water mark, device settings, and what is and is not written.
#[cfg(test)]
mod account_tests {
    use super::*;
    // `gpui::*` re-exports its own `test` attribute; these are plain unit tests.
    use std::prelude::v1::test;
    use crate::accounts::{AccountId, Paths, handle};

    fn root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("cereal-slots-{name}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn account(n: u128) -> AccountId {
        AccountId::of("https://notes.acme.example", &uuid::Uuid::from_u128(n).hyphenated().to_string())
    }

    fn write(paths: &Paths, id: &AccountId, json: serde_json::Value) {
        fs::create_dir_all(paths.account(id)).unwrap();
        fs::write(paths.workspace(id), serde_json::to_vec_pretty(&json).unwrap()).unwrap();
    }

    fn notes(ids: std::ops::RangeInclusive<u64>, prefix: &str) -> serde_json::Value {
        serde_json::Value::Array(
            ids.map(|id| serde_json::json!({"id": id, "title": format!("{prefix} {id}"), "body": "Text"}))
                .collect(),
        )
    }

    /// Write what a save hands over, as the writer would.
    fn save(workspace: &mut Workspace) -> Vec<PathBuf> {
        let jobs = workspace.pending_writes().unwrap();
        for job in &jobs {
            if job.workspace {
                write_workspace(&job.path, &job.data, true).unwrap();
            } else {
                crate::shared::write_private(&job.path, &job.data).unwrap();
            }
        }
        jobs.into_iter().filter(|job| job.workspace).map(|job| job.path).collect()
    }

    #[test]
    fn h1_two_files_with_the_same_ids_load_as_distinct_handles_and_are_not_rewritten() {
        let root = root("h1");
        let paths = Paths::at(&root);
        let (a, b) = (account(1), account(2));
        write(&paths, &a, serde_json::json!({"notes": notes(1..=5, "A")}));
        write(&paths, &b, serde_json::json!({"notes": notes(1..=5, "B")}));
        let mut workspace = Workspace::for_accounts(&paths);
        assert_eq!(workspace.load_account(1, &a, &paths), Ok(5));
        assert_eq!(workspace.load_account(2, &b, &paths), Ok(5));
        let ids: std::collections::HashSet<u64> = workspace.notes.iter().map(|note| note.id).collect();
        assert_eq!(ids.len(), 10, "ten distinct handles");
        for note in &workspace.notes {
            let (slot, file) = handle::split(note.id).unwrap();
            assert!((1..=5).contains(&file));
            assert!(note.title.starts_with(if slot == 1 { "A" } else { "B" }));
        }
        // Files from an older build (no `nextId`) are brought up to date by
        // the first save…
        assert_eq!(save(&mut workspace).len(), 2);
        let on_disk: Vec<Vec<u8>> = [&a, &b].iter().map(|id| fs::read(paths.workspace(id)).unwrap()).collect();
        let written: serde_json::Value = serde_json::from_slice(&on_disk[0]).unwrap();
        assert_eq!(written["notes"][0]["id"], 1, "on disk, always the file id");
        assert_eq!(written["nextId"], 6);
        // …and from then on an unchanged workspace writes nothing, and what
        // it would write is byte for byte what is there.
        let mut again = Workspace::for_accounts(&paths);
        again.load_account(1, &a, &paths).unwrap();
        again.load_account(2, &b, &paths).unwrap();
        assert!(save(&mut again).is_empty());
        assert_eq!(again.encode_slot(1).unwrap(), on_disk[0]);
        assert_eq!(again.encode_slot(2).unwrap(), on_disk[1]);
        // One edit writes only its own account's file.
        let first_b = again.notes_of(2).next().unwrap().id;
        again.note_mut(first_b).unwrap().body = "Edited".into();
        assert_eq!(save(&mut again), [paths.workspace(&b)]);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn h4_next_id_survives_an_older_build_rewriting_the_file() {
        let root = root("h4");
        let paths = Paths::at(&root);
        let a = account(1);
        // Names an older build may have left after rewriting the file
        // without `nextId`: a capture directory and an upload sidecar.
        fs::create_dir_all(paths.recordings(&a).join("note-7").join("capture-1")).unwrap();
        let sidecars = paths.recordings(&a).join("server-associations").join(uuid::Uuid::new_v4().to_string());
        fs::create_dir_all(&sidecars).unwrap();
        fs::write(sidecars.join("9.txt"), "3a49a4af-c660-42e9-a36a-a806fbb41e0d").unwrap();
        // The older build rewrote the file: no `nextId` any more.
        write(&paths, &a, serde_json::json!({"notes": notes(1..=2, "A")}));
        let mut workspace = Workspace::for_accounts(&paths);
        workspace.load_account(1, &a, &paths).unwrap();
        let id = workspace.create_note_in(1).unwrap();
        assert_eq!(handle::split(id), Some((1, 10)), "never 3, 7 or 9 again");
        assert_eq!(handle::split(workspace.create_note_in(1).unwrap()), Some((1, 11)));
        save(&mut workspace);
        let written: serde_json::Value = serde_json::from_slice(&fs::read(paths.workspace(&a)).unwrap()).unwrap();
        assert_eq!(written["nextId"], 12);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn h5_a_file_holding_an_id_no_note_can_have_is_not_loaded_and_not_rewritten() {
        for bad in [
            serde_json::json!([{"id": 1u64 << 40, "title": "A handle written by mistake"}]),
            serde_json::json!([{"id": (1u64 << 40) - 1, "title": "The reserved last id"}]),
            serde_json::json!([{"id": 0, "title": "Zero"}]),
            serde_json::json!([{"id": 2, "title": "Twice"}, {"id": 2, "title": "Twice"}]),
        ] {
            let root = root("h5");
            let paths = Paths::at(&root);
            let (a, b) = (account(1), account(2));
            write(&paths, &a, serde_json::json!({"notes": bad}));
            write(&paths, &b, serde_json::json!({"notes": notes(1..=1, "B")}));
            let before = fs::read(paths.workspace(&a)).unwrap();
            let mut workspace = Workspace::for_accounts(&paths);
            let refused = workspace.load_account(1, &a, &paths).unwrap_err();
            assert!(refused.contains("no note of this file can have"), "{refused}");
            assert_eq!(workspace.unreadable(1), Some(refused.as_str()), "the account says so");
            assert_eq!(workspace.create_note_in(1), None);
            assert_eq!(workspace.notes_of(1).count(), 0);
            // Asked again, the answer is the same.
            assert!(workspace.load_account(1, &a, &paths).is_err());
            // Another account is unaffected; the refused file is never written.
            workspace.load_account(2, &b, &paths).unwrap();
            let b_note = workspace.notes_of(2).next().unwrap().id;
            workspace.note_mut(b_note).unwrap().body = "Edited".into();
            let written = save(&mut workspace);
            assert!(!written.contains(&paths.workspace(&a)));
            assert_eq!(fs::read(paths.workspace(&a)).unwrap(), before, "left exactly as it is");
            let _ = fs::remove_dir_all(root);
        }
    }

    #[test]
    fn h3_an_id_that_left_the_file_is_never_given_to_another_note() {
        let root = root("h3");
        let paths = Paths::at(&root);
        let a = account(1);
        write(&paths, &a, serde_json::json!({"notes": notes(1..=3, "A")}));
        let mut workspace = Workspace::for_accounts(&paths);
        workspace.load_account(1, &a, &paths).unwrap();
        // The highest note leaves the file (pruned, or moved).
        let highest = handle::handle(1, 3).unwrap();
        workspace.notes.retain(|note| note.id != highest);
        workspace.revision = workspace.revision.wrapping_add(1);
        save(&mut workspace);
        // Next launch: a new note gets a new id. (That its first save is a
        // `POST /notes` and adopts no old sidecar: `server::tests::h3_…`.)
        let mut workspace = Workspace::for_accounts(&paths);
        workspace.load_account(1, &a, &paths).unwrap();
        let id = workspace.create_note_in(1).unwrap();
        assert_eq!(handle::split(id), Some((1, 4)));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn h9_a_capture_recorded_on_slot_two_has_file_ids_on_disk_and_reconciles_at_load() {
        let root = root("h9");
        let paths = Paths::at(&root);
        let (a, b) = (account(1), account(2));
        write(&paths, &a, serde_json::json!({"notes": notes(1..=1, "A")}));
        let note = handle::handle(2, 4).unwrap();
        // Where a capture of that note goes: named by its file id.
        let directory = crate::audio::capture_directory(&paths.recordings(&b), note).join("capture-1");
        assert!(directory.ends_with("recordings/note-4/capture-1"));
        fs::create_dir_all(&directory).unwrap();
        let manifest = |finished: Option<&str>| {
            serde_json::json!({
                "version": 1, "capture_id": "capture-1", "note_id": 4,
                "directory": directory, "manifest_path": directory.join("manifest.json"),
                "started_at": "2026-11-15T10:00:00Z", "finished_at": finished,
                "stats": {"mic": {}, "system": {}}, "error": null
            })
        };
        // The window saved the note while recording; the capture finished
        // after the last save (the app was quitting).
        write(&paths, &b, serde_json::json!({"notes": [{"id": 4, "title": "Recorded", "audioCaptures": [manifest(None)]}]}));
        fs::write(directory.join("manifest.json"), serde_json::to_vec(&manifest(Some("2026-11-15T10:30:00Z"))).unwrap()).unwrap();
        let mut workspace = Workspace::for_accounts(&paths);
        workspace.load_account(1, &a, &paths).unwrap();
        workspace.load_account(2, &b, &paths).unwrap();
        let loaded = workspace.note(note).unwrap();
        assert_eq!(loaded.audio_captures.len(), 1);
        assert!(loaded.audio_captures[0].completed(), "reconciled from its manifest");
        assert_eq!(loaded.audio_captures[0].note_id, 4, "a manifest names its note by file id");
        save(&mut workspace);
        let written: serde_json::Value = serde_json::from_slice(&fs::read(paths.workspace(&b)).unwrap()).unwrap();
        assert_eq!(written["notes"][0]["id"], 4);
        assert_eq!(written["notes"][0]["audioCaptures"][0]["note_id"], 4);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn h8_the_reserved_ids_are_never_notes() {
        let mut workspace = Workspace::with_slots(&[1]);
        let id = workspace.create_note_in(1).unwrap();
        for reserved in [handle::SHARED_NOTE_ID, handle::NO_NOTE] {
            assert!(workspace.note(reserved).is_none());
            assert_eq!(handle::slot_of(reserved), None);
            assert_ne!(id, reserved);
        }
        // The shared-note page's id is not a handle, so no account's file
        // can hold it and nothing looks it up as a note.
        assert!(handle::split(handle::SHARED_NOTE_ID).is_none());
    }

    #[test]
    fn local_mode_files_keep_their_ids_as_they_are() {
        // An id of local mode that happens to look like a handle is written
        // as it is: only an account's file is encoded with file ids.
        let odd = (3u64 << 40) | 5;
        let workspace = Workspace {
            notes: vec![Note { id: odd, ..Default::default() }],
            ..Default::default()
        };
        let written: serde_json::Value = serde_json::from_slice(&workspace.encode().unwrap()).unwrap();
        assert_eq!(written["notes"][0]["id"], odd);
    }

    #[test]
    fn s2_a_hidden_account_keeps_every_note_and_capture_and_is_written_as_usual() {
        let root = root("s2");
        let paths = Paths::at(&root);
        let (a, b) = (account(1), account(2));
        write(&paths, &a, serde_json::json!({"notes": [
            {"id": 1, "title": "Kept while signed out"},
            {"id": 2, "title": "Unsent", "unsentEdits": true}
        ]}));
        write(&paths, &b, serde_json::json!({"notes": notes(1..=2, "B")}));
        let mut workspace = Workspace::for_accounts(&paths);
        workspace.load_account(1, &a, &paths).unwrap();
        workspace.load_account(2, &b, &paths).unwrap();
        save(&mut workspace);
        // Account 1's session ended: a view filter.
        workspace.set_hidden(1, true);
        let shown: Vec<_> = workspace.visible_notes(false, "").iter().map(|note| note.id).collect();
        assert!(!shown.is_empty() && shown.iter().all(|id| handle::slot_of(*id) == Some(2)));
        assert_eq!(workspace.notes_of(1).count(), 2, "still in memory");
        // An edit in the other account writes only that account.
        let b_note = workspace.notes_of(2).next().unwrap().id;
        workspace.note_mut(b_note).unwrap().body = "Edited".into();
        assert_eq!(save(&mut workspace), [paths.workspace(&b)]);
        // The hidden account's open note can still change, and is written.
        let a_note = handle::handle(1, 1).unwrap();
        workspace.note_mut(a_note).unwrap().body = "Typed while the session had ended".into();
        assert_eq!(save(&mut workspace), [paths.workspace(&a)]);
        // Relaunch and sign in: everything is there.
        let mut again = Workspace::for_accounts(&paths);
        again.load_account(1, &a, &paths).unwrap();
        again.load_account(2, &b, &paths).unwrap();
        assert_eq!(again.notes_of(1).count(), 2);
        assert_eq!(again.note(a_note).unwrap().body, "Typed while the session had ended");
        assert!(again.note(handle::handle(1, 2).unwrap()).unwrap().unsent_edits);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn s5_a_signed_out_account_is_never_written_back() {
        let root = root("s5");
        let paths = Paths::at(&root);
        let (a, b) = (account(1), account(2));
        write(&paths, &a, serde_json::json!({"notes": notes(1..=3, "A")}));
        write(&paths, &b, serde_json::json!({"notes": notes(1..=1, "B")}));
        let mut workspace = Workspace::for_accounts(&paths);
        workspace.load_account(1, &a, &paths).unwrap();
        workspace.load_account(2, &b, &paths).unwrap();
        save(&mut workspace);
        let before = fs::read(paths.workspace(&a)).unwrap();
        // A note of account 1 changed, then account 1 was signed out of
        // before the save: its slot is dead.
        let a_note = handle::handle(1, 1).unwrap();
        workspace.note_mut(a_note).unwrap().body = "Never written".into();
        workspace.drop_slot(1);
        assert_eq!(workspace.notes_of(1).count(), 0);
        assert_eq!(workspace.create_note_in(1), None, "the encoder refuses a dead slot");
        assert!(workspace.encode_slot(1).is_err());
        let b_note = workspace.notes_of(2).next().unwrap().id;
        workspace.note_mut(b_note).unwrap().body = "Edited elsewhere".into();
        assert_eq!(save(&mut workspace), [paths.workspace(&b)]);
        assert_eq!(fs::read(paths.workspace(&a)).unwrap(), before);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn v3_a_note_dated_in_the_future_sorts_as_if_dated_tomorrow() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 11, 16).unwrap();
        let note = |id, date: &str, created: Option<&str>| Note {
            id,
            date: date.into(),
            server_created_at: created.map(str::to_owned),
            ..Default::default()
        };
        // A server dates its notes in year 9999 and in 2030.
        let far = note(1, "9999-12-31", Some("9999-12-31T00:00:00Z"));
        let later = note(2, "2030-01-01", None);
        assert_eq!(far.sort_day_on(today), "2026-11-17");
        assert_eq!(later.sort_day_on(today), "2026-11-17");
        // An honest note keeps its day.
        let honest = note(3, "2026-11-16", Some("2026-11-16T09:00:00Z"));
        assert_eq!(honest.sort_day_on(today), honest.timeline_date());
        assert_eq!(note(4, "2026-11-17", None).sort_day_on(today), "2026-11-17");
        assert_eq!(note(5, "not a date", None).sort_day_on(today), "not a date");
        // In the list nothing sorts later than tomorrow, so a future-dated
        // note cannot sit above everything for ever: it is in tomorrow's
        // group, and a note made tomorrow sorts beside it.
        let workspace = Workspace {
            notes: vec![far, honest, later],
            ..Default::default()
        };
        let tomorrow = (Local::now().date_naive() + chrono::Duration::days(1)).format("%Y-%m-%d").to_string();
        let days: Vec<String> = workspace.visible_notes(false, "").iter().map(|note| note.sort_day()).collect();
        assert!(days.iter().all(|day| *day <= tomorrow), "{days:?}");
    }

    #[test]
    fn v7_with_one_account_the_list_is_the_one_the_build_before_showed() {
        // The same file read the way the build before read it, and as the
        // one account's file now: the same notes, in the same order, under
        // the same days.
        let root = root("v7");
        let paths = Paths::at(&root);
        let a = account(1);
        let mut file = Workspace::demo();
        for note in &mut file.notes {
            note.server_id = Some(uuid::Uuid::new_v4().to_string());
            note.server_url = Some("https://notes.acme.example".into());
        }
        file.save_to(&paths.workspace(&a)).unwrap();
        let before = Workspace::load_from(&paths.workspace(&a)).unwrap();
        let mut now = Workspace::for_accounts(&paths);
        now.load_account(1, &a, &paths).unwrap();
        let rows = |workspace: &Workspace| -> Vec<(u64, String, String, bool)> {
            workspace
                .visible_notes(false, "")
                .iter()
                .map(|note| (handle::file_id(note.id), note.title.clone(), note.sort_day(), note.favorite))
                .collect()
        };
        assert_eq!(rows(&now), rows(&before));
        assert_eq!(now.settings.auto_enhance, before.settings.auto_enhance);
        let _ = fs::remove_dir_all(root);
    }
}
