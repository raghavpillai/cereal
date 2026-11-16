//! Chat presentation shared by the compact note assistant and full chat surface.
//! Markdown remains data: raw HTML and unknown links are never executed.
use crate::ui::{Reach, Theme, icon, pill, round_tool, small, tool_tip};
use gpui::{prelude::*, *};
use meeting_notes_api_types::{ChatCitationDTO, ChatToolProgressDTO, ChatToolProgressDtoStatus};
use std::ops::Range;
use std::sync::{Arc, atomic::AtomicBool};

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ChatTurnAnnotations {
    #[serde(default)]
    pub tools: Vec<ChatToolProgressDTO>,
    #[serde(default)]
    pub citations: Vec<ChatCitationDTO>,
    #[serde(default)]
    pub elapsed_ms: Option<u64>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(skip)]
    expanded: bool,
    #[serde(skip)]
    expanded_tools: std::collections::HashSet<String>,
}

impl ChatTurnAnnotations {
    fn update_tool(&mut self, tool: ChatToolProgressDTO) {
        if let Some(existing) = self
            .tools
            .iter_mut()
            .find(|item| item.call_id == tool.call_id)
        {
            if existing.name == tool.name
                && (existing.status == ChatToolProgressDtoStatus::Running
                    || existing.status == tool.status)
            {
                *existing = tool;
            }
        } else if self.tools.len() < 40 {
            self.tools.push(tool);
        }
    }
}

/// The name a conversation is listed under: the start of its first
/// question, on one line, ending in an ellipsis when it has been cut.
pub(crate) fn chat_title(question: &str) -> String {
    const LIMIT: usize = 70;
    let line = question.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() <= LIMIT {
        return line;
    }
    let mut title: String = line.chars().take(LIMIT - 1).collect();
    title.truncate(title.trim_end().len());
    title.push('…');
    title
}

/// A conversation's age as the chat's lists print it: "now", "6m", "6h",
/// "1d", "2mo"; one later than `now` (another Mac's clock) as its time of
/// day. `None` for a time that cannot be read.
pub(crate) fn thread_age(updated_at: &str, now: chrono::DateTime<chrono::Utc>) -> Option<String> {
    let at = chrono::DateTime::parse_from_rfc3339(updated_at).ok()?;
    let age = now.signed_duration_since(at);
    Some(if age.num_seconds() < 0 {
        at.with_timezone(&chrono::Local).format("%-I:%M %p").to_string()
    } else if age.num_minutes() == 0 {
        "now".into()
    } else if age.num_hours() == 0 {
        format!("{}m", age.num_minutes())
    } else if age.num_days() == 0 {
        format!("{}h", age.num_hours())
    } else if age.num_days() < 30 {
        format!("{}d", age.num_days())
    } else {
        format!("{}mo", age.num_days() / 30)
    })
}

pub(crate) fn valid_citation_id(id: &str) -> bool {
    id.strip_prefix('S').is_some_and(|number| {
        number.len() <= 3
            && !number.starts_with('0')
            && number
                .parse::<u16>()
                .is_ok_and(|number| (1..=999).contains(&number))
    })
}

/// The note here that a citation names: a note of the conversation's own
/// account (`slot`), on that account's server. A citation opens in the
/// thread's account and no other. `slot` is `None` only where notes have
/// no account.
fn chat_source_note(
    notes: &[crate::model::Note],
    server_id: &str,
    server_url: &str,
    slot: Option<u16>,
) -> Option<u64> {
    notes
        .iter()
        .find(|note| {
            !note.trashed
                && note.server_id.as_deref() == Some(server_id)
                && note.server_url.as_deref() == Some(server_url)
                && slot.is_none_or(|slot| crate::accounts::handle::slot_of(note.id) == Some(slot))
        })
        .map(|note| note.id)
}

/// Whether a transcript citation's own passage is still in the note: the cited
/// segment exists, is final, starts at the cited time and still contains the
/// quote. The note's transcript revision is not compared, so transcript that
/// arrives after the answer, or an edit elsewhere in the note, does not disable
/// an older citation. A citation without a segment is left to the quote search.
fn citation_passage_intact(note: &crate::model::Note, citation: &ChatCitationDTO) -> bool {
    let Some(segment_id) = citation.segment_id.as_deref() else {
        return true;
    };
    note.server_transcript_segments.iter().any(|segment| {
        segment.id == segment_id
            && segment.is_final
            && citation
                .start_ms
                .is_none_or(|start| start == segment.start_ms)
            && segment.text.contains(&citation.quote)
    })
}

pub(crate) fn floating_workspace_chat(screen: crate::app::Screen) -> bool {
    matches!(
        screen,
        crate::app::Screen::Home
            | crate::app::Screen::MyNotes
            | crate::app::Screen::Favorites
    )
}

pub struct ActiveChat {
    pub epoch: u64,
    pub index: usize,
    pub cancel: Arc<AtomicBool>,
    started: std::time::Instant,
}
impl ActiveChat {
    fn elapsed_ms(&self) -> u64 {
        self.started.elapsed().as_millis().min(u64::MAX as u128) as u64
    }
}

/// An answer still arriving for a conversation that is not on screen, and
/// the questions waiting behind it.
pub struct ParkedChat {
    pub active: ActiveChat,
    pub queue: Vec<String>,
}

/// What a stream event did to the conversation it belongs to.
#[derive(Debug, PartialEq)]
pub(crate) enum Applied {
    /// The answer was stopped or its conversation is gone: stop reading.
    Gone,
    /// Applied; more will follow.
    Updated { loaded: bool },
    /// The answer is finished. `next` is the question that was waiting behind
    /// it; `rest` is what waits behind that one in a conversation off screen
    /// (on screen the rest stays in the queue).
    Finished {
        loaded: bool,
        next: Option<String>,
        rest: Vec<String>,
    },
}

/// A turn that has been added to its conversation and is ready to be sent.
pub(crate) struct Begun {
    pub epoch: u64,
    pub index: usize,
    pub cancel: Arc<AtomicBool>,
    pub history: Vec<serde_json::Value>,
}

/// The earlier turns sent along with a question: the last fifteen that were
/// answered in full.
fn chat_history(
    messages: &[(String, String)],
    interrupted: impl Fn(usize) -> bool,
) -> Vec<serde_json::Value> {
    messages
        .iter()
        .enumerate()
        .filter(|(i, (q, a))| {
            !interrupted(*i)
                && !a.is_empty()
                && q.encode_utf16().count() <= 20_000
                && a.encode_utf16().count() <= 20_000
        })
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .take(15)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .flat_map(|(_, (q, a))| {
            [
                serde_json::json!({"role":"user","content":q}),
                serde_json::json!({"role":"assistant","content":a}),
            ]
        })
        .collect()
}

/// Apply one stream event to the turn at `index`. True when the event marks
/// the turn as interrupted.
/// `elapsed_ms` is how long the answer has been asked for: the first of its
/// text to arrive fixes how long it "thought".
fn apply_stream_event(
    messages: &mut [(String, String)],
    annotations: &mut std::collections::HashMap<usize, ChatTurnAnnotations>,
    index: usize,
    event: crate::server::ChatStreamEvent,
    elapsed_ms: u64,
) -> bool {
    use crate::server::ChatStreamEvent;
    let Some((_, answer)) = messages.get_mut(index) else {
        return false;
    };
    match event {
        ChatStreamEvent::Delta(text) => {
            if answer.is_empty() && !text.is_empty() {
                annotations
                    .entry(index)
                    .or_default()
                    .elapsed_ms
                    .get_or_insert(elapsed_ms);
            }
            answer.push_str(&text)
        }
        ChatStreamEvent::Done => {}
        ChatStreamEvent::Tool(tool) => annotations.entry(index).or_default().update_tool(tool),
        ChatStreamEvent::Citations(citations) => {
            annotations.entry(index).or_default().citations = citations;
        }
        ChatStreamEvent::Error(error) => {
            if answer.is_empty() {
                *answer = format!("Could not complete the response. {error}");
            } else {
                annotations.entry(index).or_default().error = Some(error);
            }
            return true;
        }
    }
    false
}

/// The conversation on screen together with every conversation that is still
/// receiving an answer off screen.
///
/// An answer belongs to its conversation, not to the panel that asked for it.
/// Leaving the conversation (another note, Home, a thread from History) parks
/// its stream: the answer goes on arriving into the saved copy of the
/// conversation, and coming back takes the stream on screen again, where Stop
/// cancels it as before.
pub(crate) struct Chats<'a> {
    pub messages: &'a mut Vec<(String, String)>,
    pub annotations: &'a mut std::collections::HashMap<usize, ChatTurnAnnotations>,
    pub interrupted: &'a mut std::collections::HashSet<usize>,
    pub thread_id: &'a mut String,
    pub stream: &'a mut Option<ActiveChat>,
    /// Questions sent while an answer was arriving, oldest first.
    pub queue: &'a mut Vec<String>,
    pub archives: &'a mut Vec<ArchivedChat>,
    pub parked: &'a mut std::collections::HashMap<String, ParkedChat>,
    pub epoch: &'a mut u64,
}

impl Chats<'_> {
    /// The conversation on screen is about to be replaced: if an answer is
    /// arriving, save the conversation and let the answer go on off screen.
    /// `scope` is the note the conversation is about (0 for the workspace).
    pub fn park(&mut self, scope: u64) {
        let Some(active) = self.stream.take() else {
            return;
        };
        let entry = ArchivedChat {
            id: self.thread_id.clone(),
            note_id: scope,
            title: self
                .messages
                .first()
                .map(|(question, _)| chat_title(question))
                .unwrap_or_default(),
            messages: self.messages.clone(),
            account: None,
            interrupted: self.interrupted.iter().copied().collect(),
            annotations: self.annotations.clone(),
            updated_at: Some(chrono::Utc::now().to_rfc3339()),
        };
        upsert_chat_archive(self.archives, entry);
        self.parked.insert(
            self.thread_id.clone(),
            ParkedChat {
                active,
                queue: std::mem::take(self.queue),
            },
        );
    }

    /// The saved conversation to show for a note: the one still receiving an
    /// answer if there is one, otherwise the most recent.
    pub fn thread_for_note(&self, note_id: u64) -> Option<ArchivedChat> {
        let of_note = |chat: &&ArchivedChat| chat.note_id == note_id;
        self.archives
            .iter()
            .rev()
            .filter(of_note)
            .find(|chat| self.parked.contains_key(&chat.id))
            .or_else(|| self.archives.iter().rev().find(of_note))
            .cloned()
    }

    /// A saved conversation has just been put on screen: if its answer is
    /// still arriving, take the stream and what waits behind it with it.
    pub fn attach(&mut self) {
        self.queue.clear();
        if let Some(parked) = self.parked.remove(self.thread_id.as_str()) {
            *self.stream = Some(parked.active);
            *self.queue = parked.queue;
        }
    }

    /// Add `question` as a new turn of `thread` and mark it as being
    /// answered. A conversation off screen takes `rest` as its queue.
    pub fn begin(&mut self, thread: &str, question: String, rest: Vec<String>) -> Option<Begun> {
        let cancel = Arc::new(AtomicBool::new(false));
        let started = std::time::Instant::now();
        if self.thread_id.as_str() == thread {
            if self.stream.is_some() {
                return None;
            }
            *self.epoch = self.epoch.wrapping_add(1);
            let history = chat_history(self.messages, |i| self.interrupted.contains(&i));
            let index = self.messages.len();
            self.messages.push((question, String::new()));
            *self.stream = Some(ActiveChat {
                epoch: *self.epoch,
                index,
                cancel: cancel.clone(),
                started,
            });
            return Some(Begun {
                epoch: *self.epoch,
                index,
                cancel,
                history,
            });
        }
        if self.parked.contains_key(thread) {
            return None;
        }
        let archive = self.archives.iter_mut().find(|chat| chat.id == thread)?;
        *self.epoch = self.epoch.wrapping_add(1);
        let history = chat_history(&archive.messages, |i| archive.interrupted.contains(&i));
        let index = archive.messages.len();
        archive.messages.push((question, String::new()));
        self.parked.insert(
            thread.to_owned(),
            ParkedChat {
                active: ActiveChat {
                    epoch: *self.epoch,
                    index,
                    cancel: cancel.clone(),
                    started,
                },
                queue: rest,
            },
        );
        Some(Begun {
            epoch: *self.epoch,
            index,
            cancel,
            history,
        })
    }

    /// Apply an event of the stream started for turn `index` of `thread`,
    /// wherever that conversation is now.
    pub fn event(
        &mut self,
        thread: &str,
        epoch: u64,
        index: usize,
        event: crate::server::ChatStreamEvent,
    ) -> Applied {
        use crate::server::ChatStreamEvent;
        let done = matches!(event, ChatStreamEvent::Done | ChatStreamEvent::Error(_));
        let current = |active: &ActiveChat| active.epoch == epoch && active.index == index;
        if let Some(active) = self.stream.as_ref().filter(|active| {
            self.thread_id.as_str() == thread && current(active)
        }) {
            let elapsed = active.elapsed_ms();
            if apply_stream_event(self.messages, self.annotations, index, event, elapsed) {
                self.interrupted.insert(index);
            }
            if !done {
                return Applied::Updated { loaded: true };
            }
            if let Some(active) = self.stream.take() {
                // An answer that never wrote anything thought until the end.
                self.annotations
                    .entry(index)
                    .or_default()
                    .elapsed_ms
                    .get_or_insert(active.elapsed_ms());
            }
            let next = (!self.queue.is_empty()).then(|| self.queue.remove(0));
            return Applied::Finished {
                loaded: true,
                next,
                rest: Vec::new(),
            };
        }
        if !self
            .parked
            .get(thread)
            .is_some_and(|parked| current(&parked.active))
        {
            return Applied::Gone;
        }
        let Some(archive) = self.archives.iter_mut().find(|chat| chat.id == thread) else {
            // The saved conversation was removed while its answer arrived.
            if let Some(parked) = self.parked.remove(thread) {
                parked
                    .active
                    .cancel
                    .store(true, std::sync::atomic::Ordering::Release);
            }
            return Applied::Gone;
        };
        let elapsed = self
            .parked
            .get(thread)
            .map_or(0, |parked| parked.active.elapsed_ms());
        if apply_stream_event(
            &mut archive.messages,
            &mut archive.annotations,
            index,
            event,
            elapsed,
        ) && !archive.interrupted.contains(&index)
        {
            archive.interrupted.push(index);
        }
        if !done {
            return Applied::Updated { loaded: false };
        }
        let mut rest = Vec::new();
        if let Some(parked) = self.parked.remove(thread) {
            archive
                .annotations
                .entry(index)
                .or_default()
                .elapsed_ms
                .get_or_insert(parked.active.elapsed_ms());
            rest = parked.queue;
        }
        archive.updated_at = Some(chrono::Utc::now().to_rfc3339());
        let next = (!rest.is_empty()).then(|| rest.remove(0));
        Applied::Finished {
            loaded: false,
            next,
            rest,
        }
    }

    /// Stop the answer arriving on screen. Returns the questions that were
    /// waiting behind it (they are not sent), or `None` if nothing was
    /// arriving.
    pub fn stop(&mut self) -> Option<Vec<String>> {
        let active = self.stream.take()?;
        self.annotations
            .entry(active.index)
            .or_default()
            .elapsed_ms
            .get_or_insert(active.elapsed_ms());
        active
            .cancel
            .store(true, std::sync::atomic::Ordering::Release);
        if let Some((_, answer)) = self.messages.get_mut(active.index)
            && answer.is_empty()
        {
            *answer = "Response stopped.".into();
        }
        self.interrupted.insert(active.index);
        *self.epoch = self.epoch.wrapping_add(1);
        Some(std::mem::take(self.queue))
    }

    /// Stop every answer arriving off screen, as when the app quits or the
    /// account changes.
    pub fn stop_parked(&mut self) {
        for (thread, parked) in self.parked.drain() {
            parked
                .active
                .cancel
                .store(true, std::sync::atomic::Ordering::Release);
            let index = parked.active.index;
            if let Some(archive) = self.archives.iter_mut().find(|chat| chat.id == thread) {
                if let Some((_, answer)) = archive.messages.get_mut(index)
                    && answer.is_empty()
                {
                    *answer = "Response stopped.".into();
                }
                if !archive.interrupted.contains(&index) {
                    archive.interrupted.push(index);
                }
                archive.annotations.entry(index).or_default().elapsed_ms =
                    Some(parked.active.elapsed_ms());
            }
        }
    }
}

/// An edit is only a draft until Send succeeds preflight. Cancelling must leave
/// the original response and every later turn available in the current thread.
pub struct EditingChat {
    pub index: usize,
    pub previous_draft: String,
}

fn follow_up_prompt(index: usize, turns: &[(String, String)]) -> Option<String> {
    let (question, answer) = turns.get(index)?;
    if index + 1 == turns.len() {
        Some("Say more about that.".into())
    } else {
        // Explicitly identify the selected answer; sending an unqualified
        // follow-up here would otherwise refer to the newest answer instead.
        Some(format!(
            "Say more about your answer to this question: {}\n\nSelected answer:\n{}",
            question.chars().take(2000).collect::<String>(),
            answer.chars().take(6000).collect::<String>()
        ))
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct ArchivedChat {
    pub id: String,
    /// The note the conversation is about, or 0 for one across meetings.
    /// On disk a file id; in memory a handle. 0 is never either.
    #[serde(serialize_with = "archived_note_id")]
    pub note_id: u64,
    /// The account the thread belongs to, for life. Not persisted: implied
    /// by which account's `chats.json` it was read from. `None` in local
    /// mode.
    #[serde(skip)]
    pub account: Option<crate::accounts::AccountId>,
    pub title: String,
    pub messages: Vec<(String, String)>,
    pub interrupted: Vec<usize>,
    #[serde(default)]
    pub annotations: std::collections::HashMap<usize, ChatTurnAnnotations>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

fn archived_note_id<S: serde::Serializer>(id: &u64, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_u64(crate::accounts::handle::on_disk(*id))
}

/// Saved conversations kept per account at most.
const THREADS_PER_ACCOUNT: usize = 20;

/// The account a new thread belongs to: its note's, or for a conversation
/// across meetings the default account (until the scope can be chosen).
/// `None` in local mode.
fn thread_account(note_id: u64) -> Option<crate::accounts::AccountId> {
    if !crate::server::enabled() {
        return None;
    }
    if note_id == crate::accounts::handle::NO_NOTE {
        crate::accounts::default_account().map(|account| account.id)
    } else {
        crate::accounts::of_note(note_id).map(|account| account.id)
    }
}

/// The threads of one account's file as they are kept in memory: tagged
/// with the account, their note ids as handles in its slot, the newest
/// [`THREADS_PER_ACCOUNT`]. A thread about a note id that cannot be a
/// note's is left out.
fn adopt_threads(
    sessions: Vec<ArchivedChat>,
    account: &crate::accounts::AccountId,
    slot: u16,
) -> Vec<ArchivedChat> {
    let mut threads: Vec<ArchivedChat> = sessions
        .into_iter()
        .filter_map(|mut chat| {
            if chat.note_id != crate::accounts::handle::NO_NOTE {
                chat.note_id = crate::accounts::handle::handle(slot, chat.note_id)?;
            }
            chat.account = Some(account.clone());
            Some(chat)
        })
        .collect();
    let extra = threads.len().saturating_sub(THREADS_PER_ACCOUNT);
    threads.drain(..extra);
    threads
}

#[derive(serde::Serialize, serde::Deserialize)]
struct HistoryFile {
    version: u32,
    sessions: Vec<ArchivedChat>,
}

fn chat_date_group(value: Option<&str>) -> String {
    chat_date_group_at(value, chrono::Local::now())
}

fn chat_date_group_at(value: Option<&str>, now: chrono::DateTime<chrono::Local>) -> String {
    use chrono::Datelike;
    let Some(date) = value
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.with_timezone(&chrono::Local).date_naive())
    else {
        return "Earlier".into();
    };
    if date == now.date_naive() {
        "Today".into()
    } else if Some(date) == now.date_naive().pred_opt() {
        "Yesterday".into()
    } else {
        date.format(if date.year() == now.year() {
            "%B"
        } else {
            "%B %Y"
        })
        .to_string()
    }
}

/// Local mode's history file. An account's is `Paths::chats`.
fn history_path() -> Result<std::path::PathBuf, String> {
    crate::model::audio_directory(None)?
        .parent()
        .map(|p| p.join("chats.json"))
        .ok_or("Chat workspace is unavailable".into())
}
/// Local mode's saved conversations. Each account's are read when its
/// session has been verified (`load_account_chats`).
pub fn load_history() -> Vec<ArchivedChat> {
    if crate::audio::preview_mode() || crate::server::enabled() {
        return vec![];
    }
    crate::store::wait_idle();
    let read = || -> Option<Vec<ArchivedChat>> {
        let path = history_path().ok()?;
        if std::fs::metadata(&path).ok()?.len() > 4 * 1024 * 1024 {
            return None;
        }
        let history: HistoryFile = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
        if history.version != 1 {
            return None;
        }
        Some(
            history
                .sessions
                .into_iter()
                .rev()
                .take(20)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect(),
        )
    };
    read().unwrap_or_default()
}
#[cfg(test)]
fn store_history(path: &std::path::Path, sessions: &[ArchivedChat]) -> Result<(), String> {
    write_history(path, &encode_history(sessions)?)
}

/// The history file's bytes. Encoding happens where the data lives; the
/// write itself runs off the UI thread (see `store.rs`).
fn encode_history(sessions: &[ArchivedChat]) -> Result<Vec<u8>, String> {
    let bytes = serde_json::to_vec(&HistoryFile {
        version: 1,
        sessions: sessions.to_vec(),
    })
    .map_err(|_| "Could not encode chat history")?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err("Chat history exceeds its local storage limit".into());
    }
    Ok(bytes)
}

/// Atomically replace the history file, refusing to overwrite one this build
/// cannot read.
pub fn write_history(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    if path.exists() {
        let existing = std::fs::read(path).map_err(|_| "Could not read chat history")?;
        let existing: HistoryFile = serde_json::from_slice(&existing)
            .map_err(|_| "Existing chat history is unreadable; it has been preserved")?;
        if existing.version != 1 {
            return Err(
                "Existing chat history has an unsupported version; it has been preserved".into(),
            );
        }
    }
    let parent = path.parent().ok_or("Chat workspace is unavailable")?;
    std::fs::create_dir_all(parent).map_err(|_| "Could not create chat workspace")?;
    let temporary = parent.join(format!(".chat-{}.tmp", uuid::Uuid::new_v4()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .map_err(|_| "Could not save chat history")?;
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        std::fs::rename(&temporary, path)
    })()
    .map_err(|_| "Could not save chat history".to_string());
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}

fn archived_thread_scope(archives: &[ArchivedChat], thread_id: &str, proposed_note_id: u64) -> u64 {
    archives
        .iter()
        .find(|chat| chat.id == thread_id)
        .map(|chat| chat.note_id)
        .unwrap_or(proposed_note_id)
}

fn upsert_chat_archive(archives: &mut Vec<ArchivedChat>, mut entry: ArchivedChat) {
    // A thread's original note/workspace scope never changes merely because a
    // later UI navigation saved its messages while another screen was visible.
    entry.note_id = archived_thread_scope(archives, &entry.id, entry.note_id);
    // A thread belongs to one account for life.
    if let Some(existing) = archives.iter().find(|chat| chat.id == entry.id) {
        entry.account = existing.account.clone();
    }
    let account = entry.account.clone();
    archives.retain(|chat| chat.id != entry.id);
    archives.push(entry);
    // The cap is per account: one account's threads cannot push out
    // another's.
    while archives.iter().filter(|chat| chat.account == account).count() > THREADS_PER_ACCOUNT {
        let oldest = archives
            .iter()
            .position(|chat| chat.account == account)
            .expect("counted above");
        archives.remove(oldest);
    }
}

impl crate::app::Cereal {
    /// The account the conversation on screen belongs to: its note's, or
    /// for one across meetings its thread's (the default account for a new
    /// one).
    fn chat_scope(&self) -> Option<crate::accounts::Account> {
        let thread = self
            .chat_archives
            .iter()
            .find(|chat| chat.id == self.chat_thread_id)
            .and_then(|chat| chat.account.clone());
        match (self.current_note, thread) {
            (Some(note), _) => crate::accounts::of_note(note),
            (None, Some(account)) => crate::accounts::get(&account),
            (None, None) => crate::accounts::default_account(),
        }
    }

    fn local_chat_source(&self, server_id: &str) -> Option<u64> {
        let scope = self.chat_scope()?;
        chat_source_note(&self.workspace.notes, server_id, &scope.url, Some(scope.slot))
    }

    /// Read an account's saved conversations, now that its notes are loaded.
    pub fn load_account_chats(
        &mut self,
        account: &crate::accounts::Account,
        paths: &crate::accounts::Paths,
    ) {
        if crate::audio::preview_mode() {
            return;
        }
        crate::store::wait_idle();
        let file = std::fs::read(paths.chats(&account.id))
            .ok()
            .filter(|bytes| bytes.len() <= 4 * 1024 * 1024)
            .and_then(|bytes| serde_json::from_slice::<HistoryFile>(&bytes).ok())
            .filter(|history| history.version == 1);
        let Some(file) = file else {
            return;
        };
        self.chat_archives
            .retain(|chat| chat.account.as_ref() != Some(&account.id));
        self.chat_archives
            .extend(adopt_threads(file.sessions, &account.id, account.slot));
        // The history shows the union, by when each was last used.
        self.chat_archives
            .sort_by(|a, b| a.updated_at.cmp(&b.updated_at));
    }

    fn open_chat_source_note(&mut self, server_id: &str, cx: &mut Context<Self>) {
        let Some(id) = self.local_chat_source(server_id) else {
            self.show_toast(
                "This source meeting is no longer available. Refresh your notes and try again.",
                cx,
            );
            return;
        };
        self.open_note(id, cx);
        self.chat_maximized = false;
        self.assistant_open = false;
        cx.notify();
    }

    pub(crate) fn open_chat_citation(
        &mut self,
        citation: &ChatCitationDTO,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self.local_chat_source(&citation.note_id) else {
            self.show_toast(
                "This source meeting is no longer available. Refresh your notes and try again.",
                cx,
            );
            return;
        };
        if citation.source == meeting_notes_api_types::ChatCitationDtoSource::Transcript {
            if !self
                .workspace
                .note(id)
                .is_some_and(|note| citation_passage_intact(note, citation))
            {
                self.show_toast("This source quotation is no longer available.", cx);
                return;
            }
            self.jump_to_transcript_quote(
                id,
                citation.segment_id.as_deref(),
                &citation.quote,
                None,
                window,
                cx,
            );
        } else {
            self.open_note(id, cx);
            self.active_tab = if citation.source == meeting_notes_api_types::ChatCitationDtoSource::Summary {
                crate::app::NoteTab::Enhanced
            } else {
                crate::app::NoteTab::Notes
            };
            self.chat_maximized = false;
            self.assistant_open = false;
            cx.notify();
        }
    }

    /// The one line above an answer that says how it is coming along:
    /// "Thinking…" with the mark while nothing of the answer has arrived,
    /// then "Thought for Ns" once it starts, "Stopped" if it was stopped.
    /// With the meeting searches it ran, the line also opens the list of
    /// them. `None` when there is nothing to say (an old answer with no
    /// timing and no searches).
    fn render_chat_status(
        &self,
        index: usize,
        active: bool,
        thinking: bool,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let t = self.theme();
        let annotations = self.chat_annotations.get(&index);
        let has_tools = annotations.is_some_and(|item| !item.tools.is_empty());
        let expanded =
            has_tools && !thinking && annotations.is_some_and(|item| item.expanded);
        let elapsed = annotations.and_then(|item| item.elapsed_ms);
        let stopped = !active && self.chat_interrupted.contains(&index);
        // Which of the three lines it is: the change from one to the next
        // settles in rather than jumping.
        let (state, heading) = if thinking {
            (0, None)
        } else if stopped {
            (1, Some("Stopped".to_owned()))
        } else if let Some(elapsed) = elapsed {
            (2, Some(format!("Thought for {}s", (elapsed / 1000).max(1))))
        } else if has_tools {
            (3, Some("Meeting sources".to_owned()))
        } else {
            return None;
        };
        let swap = self.stage.swap(("chat-status", index as u64), state);
        let line = div()
            .id(("chat-tool-disclosure", index))
            .flex()
            .items_center()
            .gap(px(6.))
            .text_size(px(12.))
            .line_height(px(16.))
            .text_color(t.muted)
            .map(|line| match heading {
                None => line.child(crate::motion::thinking_status(
                    ("chat-thinking", self.chat_epoch),
                    t,
                    cx,
                )),
                // 3 px lower than its 16 px line, without moving the answer.
                Some(heading) => line.pt(px(3.)).mb(px(-3.)).child(heading),
            })
            // While it thinks the line only says so; the searches open from
            // it once the answer has begun.
            .when(has_tools && !thinking, |line| {
                line.cursor_default()
                    .map(|link| {
                        crate::motion::warms(
                            link,
                            crate::motion::reveal_key(("chat-tool-disclosure", index)),
                            t.muted,
                            t.text,
                        )
                    })
                    .child(icon("chevron-down", 12., t.muted).with_transformation(
                        Transformation::rotate(radians(if expanded {
                            0.
                        } else {
                            -std::f32::consts::FRAC_PI_2
                        })),
                    ))
                    .reach()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(item) = this.chat_annotations.get_mut(&index) {
                            item.expanded = !item.expanded;
                        }
                        cx.notify();
                    }))
            });
        let mut result = div()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(crate::motion::swapped(line, swap));
        let Some(annotations) = annotations.filter(|_| expanded) else {
            return Some(result);
        };
        {
            for tool in &annotations.tools {
                let call_id = tool.call_id.clone();
                let results_open = annotations.expanded_tools.contains(&call_id);
                let has_results = tool
                    .results
                    .as_ref()
                    .is_some_and(|results| !results.is_empty());
                let symbol = match tool.status {
                    ChatToolProgressDtoStatus::Completed => "check",
                    ChatToolProgressDtoStatus::Failed => "close",
                    ChatToolProgressDtoStatus::Running if active => "search",
                    ChatToolProgressDtoStatus::Running => "close",
                };
                let mut step = div()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .pl(px(8.))
                    .border_l_1()
                    .border_color(t.border)
                    .child(
                        div()
                            .id(SharedString::from(format!("chat-tool-{index}-{call_id}")))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .text_size(px(12.))
                            .text_color(t.muted)
                            .child(icon(symbol, 12., t.muted))
                            .child(tool.label.clone())
                            .when(tool.result_count.is_some() && !has_results, |row| {
                                row.child(format!(" ({})", tool.result_count.unwrap_or(0)))
                            })
                            .when(has_results, |row| {
                                row.cursor_default().child(
                                    icon("chevron-down", 11., t.muted).with_transformation(
                                        Transformation::rotate(radians(if results_open {
                                            0.
                                        } else {
                                            -std::f32::consts::FRAC_PI_2
                                        })),
                                    ),
                                )
                            })
                            .reach().on_click(cx.listener(move |this, _, _, cx| {
                                if has_results
                                    && let Some(item) = this.chat_annotations.get_mut(&index)
                                {
                                    if !item.expanded_tools.remove(&call_id) {
                                        item.expanded_tools.insert(call_id.clone());
                                    }
                                }
                                cx.notify();
                            })),
                    );
                if results_open && let Some(results) = &tool.results {
                    for meeting in results {
                        let source_id = meeting.note_id.clone();
                        step = step.child(
                            div()
                                .id(SharedString::from(format!(
                                    "chat-tool-meeting-{index}-{}-{}",
                                    tool.call_id, source_id
                                )))
                                .ml(px(20.))
                                .px(px(8.))
                                .py(px(5.))
                                .rounded(px(6.))
                                .cursor_default()
                                .child(crate::motion::tactile(t.hover).radius(6.))
                                .flex()
                                .items_center()
                                .gap(px(12.))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .text_size(px(12.))
                                        .child(if meeting.title.trim().is_empty() {
                                            "New note".into()
                                        } else {
                                            meeting.title.clone()
                                        }),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(t.muted)
                                        .child(meeting.date.clone()),
                                )
                                .reach().on_click(cx.listener(move |this, _, _, cx| {
                                    this.open_chat_source_note(&source_id, cx)
                                })),
                        );
                    }
                }
                result = result.child(step);
            }
        }
        Some(result)
    }

    fn chat_markdown(
        &self,
        index: usize,
        answer: &str,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let citations = self
            .chat_annotations
            .get(&index)
            .map(|item| item.citations.clone())
            .unwrap_or_default();
        self.sourced_chat_markdown(answer, compact, citations, cx)
    }

    fn sourced_chat_markdown(
        &self,
        answer: &str,
        compact: bool,
        citations: Vec<ChatCitationDTO>,
        cx: &mut Context<Self>,
    ) -> Div {
        let t = self.theme();
        let weak = cx.weak_entity();
        markdown_with_inline(answer, t, compact, move |text, block| {
            let (text, highlights, links) = linked_inline_with_citations(text, t, &citations);
            let ranges = links.iter().map(|(range, _)| range.clone()).collect();
            let citations = citations.clone();
            let weak = weak.clone();
            InteractiveText::new(
                ("chat-sourced-markdown", block),
                StyledText::new(text).with_highlights(highlights),
            )
            .on_click(ranges, move |clicked, window, cx| {
                let Some((_, destination)) = links.get(clicked) else {
                    return;
                };
                if let Some(id) = destination.strip_prefix("minutes-source:") {
                    if let Some(citation) = citations.iter().find(|citation| citation.id == id) {
                        let _ = weak
                            .update(cx, |this, cx| this.open_chat_citation(citation, window, cx));
                    }
                } else {
                    cx.open_url(destination);
                }
            })
        })
    }

    pub fn save_chat_history(&mut self) {
        // Navigation calls this every time. A thread that has not changed
        // since it was archived keeps its entry (and its place in the list),
        // so an unchanged history is neither re-stamped nor rewritten.
        let unchanged = self
            .chat_archives
            .iter()
            .find(|archive| archive.id == self.chat_thread_id)
            .is_some_and(|archive| {
                let mut interrupted: Vec<_> = self.chat_interrupted.iter().copied().collect();
                let mut archived = archive.interrupted.clone();
                interrupted.sort_unstable();
                archived.sort_unstable();
                archive.messages == self.chat_messages
                    && archived == interrupted
                    && serde_json::to_value(&archive.annotations).ok()
                        == serde_json::to_value(&self.chat_annotations).ok()
            });
        if !self.chat_messages.is_empty() && !unchanged {
            let scope = self.current_note.unwrap_or(0);
            let entry = ArchivedChat {
                id: self.chat_thread_id.clone(),
                note_id: scope,
                account: thread_account(scope),
                title: chat_title(&self.chat_messages[0].0),
                messages: self.chat_messages.clone(),
                interrupted: self.chat_interrupted.iter().copied().collect(),
                annotations: self.chat_annotations.clone(),
                updated_at: Some(chrono::Utc::now().to_rfc3339()),
            };
            upsert_chat_archive(&mut self.chat_archives, entry);
        }
        self.persist_chat_archives();
    }

    /// Queue the saved conversations for writing.
    pub(crate) fn persist_chat_archives(&mut self) {
        // Preview/smoke threads exercise the same in-memory identity behavior,
        // while never reading or writing a real user's private history file.
        if crate::audio::preview_mode() {
            return;
        }
        // Connected to a server with nobody signed in: there is no private
        // history to write and no account to keep it under. (A link's page
        // can be opened and left in this state.)
        if crate::server::enabled() {
            // An account's threads are kept on its server only (`chat_sync.rs`):
            // no chat file is written for it.
            return;
        }
        // Encode here, write in the background: the next render starts the
        // write (`drain_chat_history`), and identical bytes are not queued.
        match history_path().and_then(|path| {
            encode_history(&self.chat_archives).map(|bytes| (path, bytes))
        }) {
            Ok((path, bytes)) => {
                crate::store::queue_chat_history(path, bytes);
            }
            Err(error) => self.toast = Some(error),
        }
    }

    pub fn restore_archived_chat(
        &mut self,
        archive_id: &str,
        expanded: bool,
        cx: &mut Context<Self>,
    ) {
        // An answer still arriving goes on arriving in its own conversation.
        self.park_chat_stream();
        self.cancel_chat_edit(cx);
        self.save_chat_history();
        if let Some(chat) = self
            .chat_archives
            .iter()
            .find(|chat| chat.id == archive_id)
            .cloned()
        {
            if chat.note_id != 0 {
                if !self
                    .workspace
                    .note(chat.note_id)
                    .is_some_and(|note| !note.trashed)
                {
                    self.show_toast("This meeting is no longer available", cx);
                    return;
                }
                self.open_note(chat.note_id, cx);
                self.chat_maximized = expanded;
                self.assistant_open = true;
            } else {
                self.finish_note(cx);
                self.current_note = None;
                self.screen = crate::app::Screen::Chat;
                self.chat_maximized = true;
            }
            // Opening the note may have put another of its conversations on
            // screen, answer and all: that one carries on off screen too.
            self.park_chat_stream();
            self.chat_messages = chat.messages;
            self.chat_thread_id = chat.id;
            self.chat_interrupted = chat.interrupted.into_iter().collect();
            self.chat_annotations = chat.annotations;
            self.chats().attach();
        }
        self.ask
            .update(cx, |input, cx| input.set_placeholder("Ask anything", cx));
        self.chat_history_open = false;
        cx.notify();
    }

    pub fn restore_note_chat(&mut self, id: u64, cx: &mut Context<Self>) {
        self.cancel_chat_edit(cx);
        if let Some(chat) = self.chats().thread_for_note(id) {
            self.chat_messages = chat.messages;
            self.chat_interrupted = chat.interrupted.into_iter().collect();
            self.chat_annotations = chat.annotations;
            self.chat_thread_id = chat.id;
            // Its answer may still be arriving: take the stream back.
            self.chats().attach();
        } else {
            self.chat_messages.clear();
            self.chat_interrupted.clear();
            self.chat_annotations.clear();
            self.chat_thread_id = uuid::Uuid::new_v4().to_string();
        }
        let placeholder = if self.current_note.is_some()
            && !self.assistant_open
            && !self.chat_messages.is_empty()
        {
            "Continue chat"
        } else {
            "Ask anything"
        };
        self.ask
            .update(cx, |input, cx| input.set_placeholder(placeholder, cx));
    }
    pub fn close_chat_surface(&mut self, cx: &mut Context<Self>) {
        self.assistant_open = false;
        self.chat_history_open = false;
        let placeholder = if self.chat_messages.is_empty() {
            "Ask anything"
        } else {
            "Continue chat"
        };
        self.ask
            .update(cx, |input, cx| input.set_placeholder(placeholder, cx));
        cx.notify();
    }
    /// The app's chat state as one conversation on screen plus the ones
    /// still receiving an answer off screen.
    pub(crate) fn chats(&mut self) -> Chats<'_> {
        Chats {
            messages: &mut self.chat_messages,
            annotations: &mut self.chat_annotations,
            interrupted: &mut self.chat_interrupted,
            thread_id: &mut self.chat_thread_id,
            stream: &mut self.chat_stream,
            queue: &mut self.chat_queue,
            archives: &mut self.chat_archives,
            parked: &mut self.chat_parked,
            epoch: &mut self.chat_epoch,
        }
    }
    /// The conversation on screen is about to be replaced by another: an
    /// answer still arriving for it carries on off screen.
    pub fn park_chat_stream(&mut self) {
        let scope = self.current_note.unwrap_or(0);
        self.chats().park(scope);
    }
    /// Stop: cancels the answer arriving on screen and the questions waiting
    /// behind it. Their text goes back to an empty composer rather than
    /// being thrown away.
    pub fn stop_chat_stream(&mut self, cx: &mut Context<Self>) {
        if let Some(queued) = self.chats().stop() {
            if !queued.is_empty() && self.ask.read(cx).content().trim().is_empty() {
                self.ask
                    .update(cx, |input, cx| input.set_content(queued.join("\n"), cx));
            }
            self.save_chat_history();
            cx.notify();
        }
    }
    /// Stop every answer, on screen and off: the app is quitting or the
    /// account is changing.
    pub fn stop_all_chat_streams(&mut self, cx: &mut Context<Self>) {
        self.stop_chat_stream(cx);
        self.chat_queue.clear();
        self.chats().stop_parked();
    }
    /// Take a waiting question out of the queue.
    pub fn remove_queued_chat(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.chat_queue.len() {
            self.chat_queue.remove(index);
            cx.notify();
        }
    }
    pub fn clear_chat(&mut self, cx: &mut Context<Self>) {
        self.chat_editing = None;
        self.stop_chat_stream(cx);
        self.chat_queue.clear();
        self.save_chat_history();
        self.chat_thread_id = uuid::Uuid::new_v4().to_string();
        self.chat_messages.clear();
        self.chat_interrupted.clear();
        self.chat_annotations.clear();
        self.chat_history_open = false;
        // A new conversation is read from its top: in a short window the
        // empty page scrolls, and it would otherwise open where the last
        // conversation was left, with its greeting out of sight.
        self.chat_scroll.set_offset(Point::default());
        self.ask.update(cx, |input, cx| {
            input.set_content("", cx);
            input.set_placeholder("Ask anything", cx);
        });
        cx.notify();
    }
    pub fn retry_chat(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some((question, _)) = self.chat_messages.get(index).cloned() else {
            return;
        };
        self.stop_chat_stream(cx);
        self.chat_editing = Some(EditingChat {
            index,
            previous_draft: self.ask.read(cx).content(),
        });
        self.ask
            .update(cx, |input, cx| input.set_content(question, cx));
        self.submit_chat(cx);
    }
    pub fn begin_chat_edit(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some((question, _)) = self.chat_messages.get(index).cloned() else {
            return;
        };
        self.stop_chat_stream(cx);
        self.summary.dismiss();
        if self.chat_editing.is_none() {
            self.chat_editing = Some(EditingChat {
                index,
                previous_draft: self.ask.read(cx).content(),
            });
        } else if let Some(edit) = &mut self.chat_editing {
            edit.index = index;
        }
        self.ask
            .update(cx, |input, cx| input.set_content(question, cx));
        self.ask.read(cx).focus_handle.focus(window);
        cx.notify();
    }
    pub fn cancel_chat_edit(&mut self, cx: &mut Context<Self>) {
        if let Some(edit) = self.chat_editing.take() {
            self.ask
                .update(cx, |input, cx| input.set_content(edit.previous_draft, cx));
            cx.notify();
        }
    }
    pub fn apply_chat_edit(&mut self) {
        if let Some(edit) = self.chat_editing.take() {
            self.chat_messages.truncate(edit.index);
            self.chat_interrupted.retain(|i| *i < edit.index);
            self.chat_annotations.retain(|i, _| *i < edit.index);
        }
    }
    pub fn start_streaming_chat(&mut self, question: String, cx: &mut Context<Self>) {
        self.try_start_chat(question, cx);
    }

    /// Send `question` in the conversation on screen, or queue it behind the
    /// answer that is arriving. False when it could be neither sent nor
    /// queued (a toast says why).
    fn try_start_chat(&mut self, question: String, cx: &mut Context<Self>) -> bool {
        self.try_start_chat_from(question, None, cx)
    }

    /// Debug builds: send `question` and take its answer from `script`
    /// instead of a server, for captures of an answer as it arrives.
    #[cfg(debug_assertions)]
    pub(crate) fn start_scripted_chat(
        &mut self,
        question: String,
        script: std::sync::mpsc::Receiver<crate::server::ChatStreamEvent>,
        cx: &mut Context<Self>,
    ) -> bool {
        self.try_start_chat_from(question, Some(script), cx)
    }

    fn try_start_chat_from(
        &mut self,
        question: String,
        script: Option<std::sync::mpsc::Receiver<crate::server::ChatStreamEvent>>,
        cx: &mut Context<Self>,
    ) -> bool {
        if question.encode_utf16().count() > 20_000 {
            self.show_toast("Messages can contain up to 20,000 characters", cx);
            return false;
        }
        if self.chat_stream.is_some() {
            // Sent as soon as the answer that is arriving has finished.
            self.chat_queue.push(question);
            self.ask.update(cx, |input, cx| input.set_content("", cx));
            self.chat_to_bottom();
            cx.notify();
            return true;
        }
        if self.server_pending > 0 {
            self.show_toast("Wait for the current server request to finish", cx);
            return false;
        }
        let intended_scope = self.current_note.unwrap_or(0);
        if archived_thread_scope(&self.chat_archives, &self.chat_thread_id, intended_scope)
            != intended_scope
        {
            self.show_toast(
                "Reopen this conversation from History or start a new chat before sending",
                cx,
            );
            return false;
        }
        let owner = self.current_note;
        let note = owner.and_then(|id| self.workspace.note(id).cloned());
        if owner.is_some() && note.is_none() {
            return false;
        }
        // An edited or retried question replaces the saved exchange it was.
        let replacing = self.chat_editing.is_some();
        self.apply_chat_edit();
        let thread = self.chat_thread_id.clone();
        let Some(begun) = self.chats().begin(&thread, question.clone(), Vec::new()) else {
            return false;
        };
        self.ask.update(cx, |input, cx| {
            input.set_content("", cx);
            input.set_placeholder("Ask anything", cx);
        });
        if owner.is_some() {
            self.assistant_open = true;
            self.transcript_open = false;
        } else {
            if floating_workspace_chat(self.screen) {
                self.assistant_open = true;
            } else {
                self.screen = crate::app::Screen::Chat;
            }
        }
        if begun.index == 0 {
            self.chat_scroll.set_offset(Point::default());
        } else {
            self.chat_to_bottom();
        }
        self.spawn_chat_stream(thread, begun, note, question, replacing, script, cx);
        cx.notify();
        true
    }

    /// Read the answer to a turn that has just been added to `thread` and
    /// hand each event to the conversation, wherever it is by then.
    fn spawn_chat_stream(
        &mut self,
        thread: String,
        begun: Begun,
        note: Option<crate::model::Note>,
        question: String,
        replace: bool,
        script: Option<std::sync::mpsc::Receiver<crate::server::ChatStreamEvent>>,
        cx: &mut Context<Self>,
    ) {
        let saved = self.chat_thread_turn(&thread, begun.index, replace);
        let Begun {
            epoch,
            index,
            cancel,
            history,
            ..
        } = begun;
        let scope = self.chat_scope().map(|account| account.id);
        let receiver = script.unwrap_or_else(|| {
            crate::server::stream_chat_with_model(
                note,
                scope,
                question,
                history,
                cancel.clone(),
                self.selected_chat_model.clone(),
                saved,
            )
        });
        cx.spawn(async move |this, cx| {
            loop {
                let event = match receiver.try_recv() {
                    Ok(event) => event,
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(25))
                            .await;
                        continue;
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        crate::server::ChatStreamEvent::Error(
                            "The response was interrupted. Retry to continue.".into(),
                        )
                    }
                };
                let keep = this
                    .update(cx, |this, cx| {
                        this.chat_stream_event(&thread, epoch, index, event, cx)
                    })
                    .unwrap_or(false);
                if !keep {
                    // Finished, stopped, or nobody is left to read it.
                    cancel.store(true, std::sync::atomic::Ordering::Release);
                    break;
                }
            }
        })
        .detach();
    }

    /// One event of an answer. False once the answer is finished or no
    /// longer wanted.
    fn chat_stream_event(
        &mut self,
        thread: &str,
        epoch: u64,
        index: usize,
        event: crate::server::ChatStreamEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        match self.chats().event(thread, epoch, index, event) {
            Applied::Gone => false,
            Applied::Updated { loaded } => {
                if loaded {
                    cx.notify();
                }
                true
            }
            Applied::Finished {
                loaded: true, next, ..
            } => {
                self.save_chat_history();
                self.chat_turn_saved(thread, cx);
                if let Some(question) = next
                    && !self.try_start_chat(question.clone(), cx)
                {
                    // It could not be sent after all: give the text back.
                    let draft = self.ask.read(cx).content();
                    let text = if draft.trim().is_empty() {
                        question
                    } else {
                        format!("{question}\n{draft}")
                    };
                    self.ask.update(cx, |input, cx| input.set_content(text, cx));
                }
                cx.notify();
                false
            }
            Applied::Finished {
                loaded: false,
                next,
                rest,
            } => {
                if let Some(question) = next {
                    self.continue_parked_chat(thread, question, rest, cx);
                }
                self.persist_chat_archives();
                self.chat_turn_saved(thread, cx);
                cx.notify();
                false
            }
        }
    }

    /// A conversation off screen finished an answer with a question waiting
    /// behind it: send that question there.
    fn continue_parked_chat(
        &mut self,
        thread: &str,
        question: String,
        rest: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        let scope = self
            .chat_archives
            .iter()
            .find(|chat| chat.id == thread)
            .map(|chat| chat.note_id);
        let note = match scope {
            None => return,
            Some(0) => None,
            // The note the conversation is about was deleted meanwhile.
            Some(id) => match self.workspace.note(id).filter(|note| !note.trashed) {
                Some(note) => Some(note.clone()),
                None => return,
            },
        };
        if let Some(begun) = self.chats().begin(thread, question.clone(), rest) {
            self.spawn_chat_stream(thread.to_owned(), begun, note, question, false, None, cx);
        }
    }

    /// The questions waiting behind the answer that is arriving, shown under
    /// the conversation.
    pub fn render_chat_queue(
        &self,
        compact: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        if self.chat_queue.is_empty() {
            return None;
        }
        let t = self.theme();
        let (_, widest) = self.chat_question_width(compact, window);
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .mb(px(24.))
                .children(self.chat_queue.iter().enumerate().map(|(index, question)| {
                    div()
                        .flex()
                        .flex_col()
                        .items_end()
                        .gap(px(4.))
                        .when(compact, |d| d.pr(px(10.)))
                        .child(
                            div()
                                .max_w(px(widest))
                                .rounded(px(12.))
                                .rounded_br(px(3.))
                                .px(px(if compact { 11. } else { 14. }))
                                .py(px(if compact { 6. } else { 9. }))
                                .bg(t.hover)
                                .opacity(0.6)
                                .text_size(px(if compact { 14. } else { 16. }))
                                .line_height(px(if compact { 20. } else { 24. }))
                                .child(question.clone()),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(4.))
                                .h(px(22.))
                                .child(small("Queued · sends when this answer finishes", t))
                                .child(
                                    tool_tip(
                                        ("remove-queued-chat", index),
                                        "close",
                                        "Remove queued question",
                                        t,
                                    )
                                    .size(px(22.))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.remove_queued_chat(index, cx)
                                    })),
                                ),
                        )
                })),
        )
    }

    /// Everything that decides how tall a turn is drawn: its words, whether
    /// its answer is still arriving, and what is shown of its tools,
    /// sources and errors.
    fn chat_turn_key(&self, index: usize) -> u64 {
        let Some((question, answer)) = self.chat_messages.get(index) else {
            return 0;
        };
        let active = self.chat_stream.as_ref().is_some_and(|a| a.index == index);
        let notes = self.chat_annotations.get(&index).map(|item| {
            let mut open: Vec<&String> = item.expanded_tools.iter().collect();
            open.sort_unstable();
            (
                serde_json::to_string(item).unwrap_or_default(),
                item.expanded,
                open.into_iter().cloned().collect::<Vec<_>>(),
            )
        });
        crate::lazy::key((
            index,
            question,
            answer,
            active,
            self.chat_interrupted.contains(&index),
            notes,
        ))
    }

    /// The conversation's turns. Only those near the viewport are built
    /// (see `lazy`); the others keep their measured height. A long
    /// conversation with tables and code then costs a frame what its
    /// visible part costs, while scrolling and while an answer arrives.
    /// `surface` tells the three places a conversation is drawn apart
    /// (the page, the note's panel, the panel opened wide): a turn is not
    /// the same height in each.
    pub(crate) fn render_chat_turns(
        &self,
        surface: u8,
        compact: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let keys: Vec<u64> = (0..self.chat_messages.len())
            .map(|index| self.chat_turn_key(index))
            .collect();
        let (wraps_to, widest) = self.chat_question_width(compact, window);
        crate::lazy::column(
            "chat-turns",
            &keys,
            wraps_to + 100_000. * f32::from(surface),
            0.,
            &self.chat_scroll,
            window,
            |index| {
                let (question, answer) = &self.chat_messages[index];
                self.render_chat_turn(index, question, answer, compact, widest, cx)
                    .into_any_element()
            },
        )
        // As tall as its turns, whatever container it is in: see the
        // conversation's column in `render_context_chat`.
        .flex_shrink_0()
    }

    /// The width a conversation's turns wrap to, and the widest a question's
    /// bubble may be there. A question is a bubble at most 594 px wide (614
    /// in the note's panel) and never wider than its column, so that in a
    /// narrow window it wraps inside the column instead of running out of
    /// its left side.
    pub(crate) fn chat_question_width(&self, compact: bool, window: &Window) -> (f32, f32) {
        let viewport = f32::from(window.viewport_size().width);
        if compact {
            // The note's panel (also the assistant over Home, beside the
            // sidebar) is at most 682 px wide and 16 px inside the window
            // on each side; its turns have 16 px of padding.
            let beside = if self.current_note.is_none() {
                192. * self.sidebar_level
            } else {
                0.
            };
            let panel = (viewport - beside - 32.).min(682.);
            (panel, (panel - 48.).clamp(120., 614.))
        } else {
            // The page beside the sidebar: a column of at most 672 px with
            // 24 px of padding either side, and room for the scroll bar.
            let column = self.chat_column_width(window);
            (column, (column - 72.).clamp(120., 594.))
        }
    }

    /// The width of a conversation's column on a page of its own: the page
    /// beside the sidebar, up to 672 px.
    pub(crate) fn chat_column_width(&self, window: &Window) -> f32 {
        (f32::from(window.viewport_size().width) - 192. * self.sidebar_level).clamp(240., 672.)
    }

    /// Show the newest end of the conversation.
    pub(crate) fn chat_to_bottom(&self) {
        self.chat_scroll.scroll_to_bottom();
        crate::lazy::to_end("chat-turns");
    }

    pub fn render_chat_turn(
        &self,
        index: usize,
        question: &str,
        answer: &str,
        compact: bool,
        widest_question: f32,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let t = self.theme();
        let turn_hover = crate::motion::reveal_key(("chat-turn", index));
        let shown = crate::motion::revealed(turn_hover);
        let active = self.chat_stream.as_ref().is_some_and(|a| a.index == index);
        let thinking = active && answer.is_empty();
        div()
            .id(("chat-turn", index))
            .child(crate::motion::hover_probe(turn_hover))
            .min_w_0()
            .flex()
            .flex_col()
            // In a panel the parts of a turn sit closer: 10 px, which puts
            // the status line 45 px under the question as in the reference.
            .gap(px(if compact { 10. } else { 16. }))
            .mb(px(24.))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .child(
                        div()
                            .max_w(px(widest_question))
                            .rounded(px(12.))
                            .rounded_br(px(3.))
                            .px(px(if compact { 11. } else { 14. }))
                            .py(px(if compact { 6. } else { 9. }))
                            .bg(t.hover)
                            .text_size(px(if compact { 14. } else { 16. }))
                            .line_height(px(if compact { 20. } else { 24. }))
                            .child(question.to_string()),
                    ),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(4.))
                    .h(px(22.))
                    .child(
                        tool_tip(
                            ("retry-chat-question", index),
                            "chat-retry",
                            "Resend question",
                            t,
                        )
                        .size(px(24.))
                        .opacity(shown)
                        .on_click(cx.listener(move |this, _, _, cx| this.retry_chat(index, cx))),
                    )
                    .child(
                        tool_tip(("copy-chat-question", index), "copy", "Copy text", t)
                            .size(px(24.))
                            .opacity(shown)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                // Built on the click, not on every render.
                                let Some((question, _)) = this.chat_messages.get(index) else {
                                    return;
                                };
                                cx.write_to_clipboard(ClipboardItem::new_string(question.clone()));
                                crate::ui::confirm(("copy-chat-question", index), cx);
                            })),
                    )
                    .child(
                        tool_tip(("edit-chat-question", index), "edit", "Edit question", t)
                            .size(px(24.))
                            .opacity(shown)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.begin_chat_edit(index, window, cx)
                            })),
                    ),
            )
            // One status line, whether or not the answer searched meetings.
            .children(self.render_chat_status(index, active, thinking, cx))
            .when(!thinking, |container| {
                container.child(self.chat_markdown(index, answer, compact, cx))
            })
            .when_some(
                self.chat_annotations
                    .get(&index)
                    .and_then(|item| item.error.clone()),
                |container, error| {
                    container.child(
                        div()
                            .min_w_0()
                            .px(px(10.))
                            .py(px(8.))
                            .rounded(px(8.))
                            .bg(t.hover)
                            .text_size(px(12.))
                            .line_height(px(18.))
                            .text_color(t.muted)
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(error)
                            .child(
                                pill(
                                    ("retry-interrupted-chat", index),
                                    "Retry answer",
                                    "chat-retry",
                                    t,
                                )
                                .h(px(28.))
                                .on_click(
                                    cx.listener(move |this, _, _, cx| this.retry_chat(index, cx)),
                                ),
                            ),
                    )
                },
            )
            .when(!active && !answer.is_empty(), |d| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .h(px(30.))
                        .child(
                            pill(("chat-say-more", index), "Say more", "", t)
                                .h(px(28.))
                                .text_size(px(12.))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    let Some(prompt) = follow_up_prompt(index, &this.chat_messages)
                                    else {
                                        return;
                                    };
                                    this.cancel_chat_edit(cx);
                                    this.ask
                                        .update(cx, |input, cx| input.set_content(prompt, cx));
                                    this.submit_chat(cx);
                                })),
                        )
                        .child(
                            tool_tip(("copy-chat-response", index), "copy", "Copy response", t)
                                .size(px(28.))
                                .opacity(if self.chat_interrupted.contains(&index) {
                                    1.
                                } else {
                                    shown
                                })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    // The citation-rewritten copy is only
                                    // needed here, so it is built here.
                                    let Some((_, answer)) = this.chat_messages.get(index) else {
                                        return;
                                    };
                                    let copy = rewrite_citation_tokens(
                                        answer,
                                        this.chat_annotations
                                            .get(&index)
                                            .map(|item| item.citations.as_slice())
                                            .unwrap_or(&[]),
                                        |id| format!("[{}]", &id[1..]),
                                    );
                                    cx.write_to_clipboard(ClipboardItem::new_string(copy));
                                    crate::ui::confirm(("copy-chat-response", index), cx);
                                })),
                        )
                        .child(
                            tool_tip(
                                ("retry-chat-response", index),
                                "chat-retry",
                                "Retry response",
                                t,
                            )
                            .size(px(28.))
                            .opacity(if self.chat_interrupted.contains(&index) {
                                1.
                            } else {
                                shown
                            })
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.retry_chat(index, cx)),
                            ),
                        ),
                )
            })
    }

    pub fn render_chat_composer_focused(
        &self,
        focused: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        self.render_chat_composer(focused, false, cx)
    }

    /// The model the next question goes to, as its picker button shows it.
    fn chat_model_label(&self) -> String {
        self.chat_models
            .iter()
            .find(|choice| Some(crate::server::chat_model_key(choice)) == self.selected_chat_model)
            .map(|choice| choice.model.clone())
            .unwrap_or_else(|| "Auto".into())
    }

    /// The model picker's button: the model the next question goes to and
    /// a chevron. The list of the server's models opens from it; without a
    /// server there is only the instance default ("Auto").
    pub(crate) fn render_chat_model_button(
        &self,
        id: &'static str,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let t = self.theme();
        div()
            .id(id)
            .flex()
            .items_center()
            .gap(px(4.))
            .text_size(px(13.))
            .line_height(px(18.))
            .text_color(t.muted)
            .child(self.chat_model_label())
            .child(icon("chevron-down", 12., t.muted))
            .cursor_default()
            .reach()
            .on_click(cx.listener(|this, _, _, cx| {
                if crate::server::enabled() {
                    this.chat_model_picker_open = !this.chat_model_picker_open;
                    cx.notify();
                }
            }))
    }

    /// The list of models, growing out of the model button and folding back
    /// into it: above the composer's trailing end, or (`below`) under the
    /// leading end of the Chat page's composer.
    pub(crate) fn render_chat_model_picker(
        &self,
        shown: bool,
        below: bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let t = self.theme();
        let picker = self.stage.presence(
            ("chat-model-picker", 0),
            self.chat_model_picker_open && shown,
            crate::motion::SNAPPY,
        );
        if !picker.visible {
            return None;
        }
        let option = |id: ElementId, label: String| {
            div()
                .id(id)
                .px(px(10.))
                .py(px(9.))
                .rounded(px(6.))
                .cursor_default()
                .text_size(px(12.))
                .text_color(t.text)
                .child(crate::motion::tactile(t.hover).radius(6.))
                .child(label)
        };
        let list = div()
            .id("chat-model-options")
            .absolute()
            .map(|list| {
                if below {
                    list.top(px(84.)).left(px(8.))
                } else {
                    list.bottom(px(68.)).right(px(8.))
                }
            })
            .w(px(290.))
            .max_h(px(280.))
            .overflow_y_scroll()
            .p(px(6.))
            .rounded(px(12.))
            .border_1()
            .border_color(t.border)
            .bg(t.card)
            .shadow_lg()
            .occlude()
            .child(
                option("chat-model-auto".into(), "Auto · Instance default".into())
                    .reach()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.selected_chat_model = None;
                        this.chat_model_picker_open = false;
                        cx.notify();
                    })),
            )
            .children(self.chat_models.iter().enumerate().map(|(index, choice)| {
                let key = crate::server::chat_model_key(choice);
                option(("chat-model-option", index).into(), choice.model.clone())
                    .reach()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.selected_chat_model = Some(key.clone());
                        this.chat_model_picker_open = false;
                        cx.notify();
                    }))
            }));
        Some(crate::motion::pop(
            list,
            crate::motion::Pop::menu(if below { (0., 0.) } else { (1., 1.) }),
            picker,
        ))
    }

    /// The composer pill. `folded` is Home's bar at rest (not focused,
    /// nothing typed): only the field shows, and the model, attachment and
    /// voice controls grow in at its trailing end when it is focused.
    pub fn render_chat_composer(
        &self,
        focused: bool,
        folded: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let t = self.theme();
        let active = self.chat_stream.is_some();
        let can_send = !self.ask.read(cx).content().trim().is_empty() && !active;
        let composer_hover = crate::motion::reveal_key("chat-composer-pill");
        let controls = self.stage.presence(
            ("chat-composer-controls", 0),
            !folded,
            crate::motion::SNAPPY,
        );
        let cluster = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .h(px(32.))
            .when(self.chat_editing.is_none(), |d| {
                d.child(self.render_chat_model_button("chat-configured-model", cx).mr(px(2.)))
            })
            .child(
                tool_tip(
                    "chat-attachment",
                    "paperclip",
                    "Attachments are not available yet",
                    t,
                )
                .size(px(32.))
                .opacity(crate::ui::DISABLED),
            )
            .child(if active {
                // A quiet round button with a dark square, as in the
                // reference: the answer arriving is what holds the eye.
                round_tool("stop-chat-stream", "stop", "Stop response", t)
                    .size(px(32.))
                    .rounded_full()
                    .bg(t.hover)
                    .text_color(t.text)
                    .on_click(cx.listener(|this, _, _, cx| this.stop_chat_stream(cx)))
            } else if can_send {
                round_tool("send-chat-stream", "arrow-up", "Send · Enter", t)
                    .size(px(32.))
                    .rounded_full()
                    .bg(if self.chat_editing.is_some() {
                        rgb(0x835e00).into()
                    } else {
                        t.text
                    })
                    .text_color(if self.chat_editing.is_some() {
                        rgb(0xfcfcf8).into()
                    } else {
                        t.bg
                    })
                    .on_click(cx.listener(|this, _, _, cx| this.submit_chat(cx)))
            } else {
                round_tool(
                    "chat-voice-input",
                    "mic",
                    "Voice input is not configured",
                    t,
                )
                .size(px(32.))
                .rounded_full()
                .bg(t.hover)
                .opacity(crate::ui::DISABLED)
            });
        div()
            .id("chat-composer-pill")
            .relative()
            .children(self.render_chat_model_picker(!folded, false, cx))
            .w_full()
            // 58 px tall at rest, the field 21 px in from the leading edge
            // and the round button 10 px from the trailing one (measured
            // against the reference at 1.25 px per point).
            .min_h(px(58.))
            .max_h(px(170.))
            .pl(px(21.))
            .pr(px(10.))
            .py(px(12.))
            .rounded_full()
            .border_1()
            .border_color(if focused {
                crate::ui::ask_ring().0
            } else {
                crate::motion::mix(
                    t.border,
                    t.accent.opacity(0.33),
                    crate::motion::revealed(composer_hover),
                )
            })
            .bg(t.card)
            .child(crate::motion::hover_probe(composer_hover))
            .shadow(if focused {
                vec![BoxShadow {
                    color: crate::ui::ask_ring().1,
                    offset: point(px(0.), px(0.)),
                    blur_radius: px(0.),
                    spread_radius: px(3.),
                }]
            } else {
                vec![BoxShadow {
                    color: rgba(0x2929290a).into(),
                    offset: point(px(0.), px(2.)),
                    blur_radius: px(5.),
                    spread_radius: px(0.),
                }]
            })
            .flex()
            .items_center()
            .gap(px(12.))
            .child(
                div()
                    .id("chat-composer-text")
                    .flex_1()
                    .min_w_0()
                    .max_h(px(130.))
                    .overflow_y_scroll()
                    .text_size(px(14.))
                    .line_height(px(22.))
                    .child(self.ask.clone()),
            )
            // Focused, the controls grow in from the trailing edge; they
            // leave the same way when the bar folds.
            .when(controls.visible, |d| {
                d.child(crate::motion::pop(
                    cluster,
                    crate::motion::Pop::chip((1., 0.5)),
                    controls,
                ))
            })
    }

    pub fn render_chat_edit_banner(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let t = self.theme();
        div()
            .id("chat-edit-warning")
            .h(px(36.))
            .flex_shrink_0()
            .rounded_full()
            .px(px(12.))
            .bg(t.hover)
            .text_size(px(13.))
            .text_color(t.muted)
            .flex()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .flex_1()
                    .child("Everything after the edited message will be overwritten"),
            )
            .child(
                tool_tip("cancel-chat-edit", "close", "Cancel editing · Escape", t)
                    .size(px(22.))
                    .on_click(cx.listener(|this, _, _, cx| this.cancel_chat_edit(cx))),
            )
    }

    /// The recent-chats button: open the list, or close it.
    ///
    /// A press anywhere outside the open list closes it. When that press was
    /// on this button, the click it completes must not open the list again,
    /// so a list that a press closed a moment ago stays closed.
    pub(crate) fn toggle_chat_history(&mut self, cx: &mut Context<Self>) {
        let just_closed = self
            .chat_history_closed_at
            .take()
            .is_some_and(|at| at.elapsed() < std::time::Duration::from_millis(500));
        if !just_closed {
            self.chat_history_open = !self.chat_history_open;
        }
        cx.notify();
    }

    /// A press outside the recent-chats list: close it.
    pub(crate) fn dismiss_chat_history(&mut self, cx: &mut Context<Self>) {
        if self.chat_history_open {
            self.chat_history_open = false;
            self.chat_history_closed_at = Some(std::time::Instant::now());
            cx.notify();
        }
    }

    pub fn render_chat_history(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let t = self.theme();
        let mut rows = div()
            .id("chat-history-menu")
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .w(px(320.))
            .max_h(px(340.))
            .overflow_y_scroll()
            .p(px(6.))
            .rounded(px(12.))
            .bg(t.card)
            .border_1()
            .border_color(t.border)
            .shadow_lg()
            .flex()
            .flex_col();
        let archives: Vec<_> = self
            .chat_archives
            .iter()
            .enumerate()
            .rev()
            .filter(|(_, a)| {
                a.note_id == 0
                    || self
                        .workspace
                        .note(a.note_id)
                        .is_some_and(|note| !note.trashed)
            })
            .map(|(i, a)| (i, a.title.clone()))
            .collect();
        if archives.is_empty() {
            rows = rows.child(
                div()
                    .p(px(10.))
                    .text_size(px(13.))
                    .text_color(t.muted)
                    .child("No previous chats"),
            );
        }
        let mut previous_group = String::new();
        for (index, title) in archives {
            let archive = &self.chat_archives[index];
            let date_group = chat_date_group(archive.updated_at.as_deref());
            if date_group != previous_group {
                previous_group = date_group.clone();
                rows = rows.child(
                    div()
                        .px(px(6.))
                        .pt(px(6.))
                        .pb(px(4.))
                        .text_size(px(12.))
                        .text_color(t.muted)
                        .child(date_group),
                );
            }
            let note_id = archive.note_id;
            let archive_id = self.chat_archives[index].id.clone();
            rows = rows.child(
                div()
                    .id(("archived-chat", index))
                    .px(px(10.))
                    .h(px(30.))
                    .rounded(px(7.))
                    .text_size(px(13.))
                    .cursor_default()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(crate::motion::tactile(t.hover).radius(7.))
                    .child(icon(
                        if note_id == 0 { "chat" } else { "file" },
                        14.,
                        t.muted,
                    ))
                    .child(div().flex_1().min_w_0().truncate().child(title))
                    .reach().on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.restore_archived_chat(&archive_id, this.chat_maximized, cx);
                    })),
            );
        }
        rows
    }
    pub fn chat_can_scroll_down(&self) -> bool {
        // The button is 28 px and floats over the end of the conversation.
        // With less hidden than it would itself cover, it only gets in the
        // way.
        self.chat_scroll.max_offset().height + self.chat_scroll.offset().y > px(44.)
    }
    pub fn render_chat_scroll_down(&self, cx: &mut Context<Self>) -> Div {
        let t = self.theme();
        div()
            .absolute()
            .left_0()
            .right_0()
            .bottom(px(if self.chat_editing.is_some() {
                123.
            } else {
                79.
            }))
            .flex()
            .justify_center()
            .child(
                round_tool("chat-scroll-down", "arrow-down", "Scroll to latest", t)
                    .size(px(28.))
                    .rounded_full()
                    .border_1()
                    .border_color(t.border)
                    .bg(t.card)
                    .shadow_sm()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.chat_to_bottom();
                        cx.notify();
                    })),
            )
    }

    pub fn render_context_chat_header(&self, cx: &mut Context<Self>) -> Div {
        let t = self.theme();
        div()
            .relative()
            .h(px(48.))
            .flex_shrink_0()
            .px(px(8.))
            .flex()
            .items_center()
            .gap(px(12.))
            .when(self.current_note.is_some(), |d| {
                d.child(
                    round_tool("back-from-full-chat", "chevron-left", "Back to note", t)
                        .w(px(42.))
                        .h(px(28.))
                        .rounded_full()
                        .border_1()
                        .border_color(t.border)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.chat_maximized = false;
                            this.close_chat_surface(cx);
                            cx.notify();
                        })),
                )
            })
            .child(
                tool_tip("full-chat-history", "chat-history", "Recent chats", t)
                    .w_auto()
                    .px(px(8.))
                    .gap(px(6.))
                    .child("History")
                    .child(icon("chevron-down", 12., t.muted))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_chat_history(cx))),
            )
            .child(div().flex_1())
            .child(
                pill("new-full-chat", "New chat", "edit", t)
                    .h(px(28.))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.clear_chat(cx);
                        this.ask.read(cx).focus_handle.focus(window);
                    })),
            )
    }

    pub fn render_context_chat(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let t = self.theme();
        let note = self.current_note.and_then(|id| self.workspace.note(id));
        let mut content = div()
            // A width in pixels: see the conversation's page.
            .w(px(self.chat_column_width(window)))
            .px(px(24.))
            .pt(px(32.))
            .pb(px(20.))
            .flex()
            .flex_col()
            .flex_shrink_0();
        if let Some(note) = note {
            content = content.child(
                div().flex().justify_end().mb(px(16.)).child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .h(px(28.))
                        .px(px(8.))
                        .rounded(px(6.))
                        .border_1()
                        .border_color(t.border)
                        .text_size(px(12.))
                        .child(icon("file", 14., t.muted))
                        .child(if note.title.is_empty() {
                            "New note".into()
                        } else {
                            note.title.chars().take(50).collect::<String>()
                        })
                        .child(small(note.time.clone(), t)),
                ),
            );
        }
        if self.chat_messages.is_empty() {
            content = content.child(div().pt(px(80.)).text_size(px(28.)).child("Ask anything"));
        }
        if !self.chat_messages.is_empty() {
            content = content.child(self.render_chat_turns(2, false, window, cx));
        }
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .items_center()
            .bg(t.bg)
            .child(
                div()
                    .id("full-context-chat-scroll")
                    .track_scroll(&self.chat_scroll)
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_y_scroll()
                    .on_scroll_wheel(cx.listener(|_, _, _, cx| cx.notify()))
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(content),
            )
            .child(
                div()
                    .w_full()
                    .max_w(px(672.))
                    .px(px(8.))
                    .pb(px(8.))
                    .when(self.chat_editing.is_some(), |d| {
                        d.child(div().mb(px(8.)).child(self.render_chat_edit_banner(cx)))
                    })
                    .child(self.render_chat_composer_focused(
                        self.ask.read(cx).focus_handle.is_focused(window),
                        cx,
                    )),
            )
            .child(
                div()
                    .pb(px(7.))
                    .text_size(px(10.))
                    .line_height(px(12.))
                    .text_color(t.muted)
                    .child("AI can make mistakes. Check important details."),
            )
            .when(self.chat_can_scroll_down(), |d| {
                d.child(self.render_chat_scroll_down(cx))
            })
            .into_any_element()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Heading(usize, String),
    Paragraph(String),
    Item(usize, String, String),
    Quote(String),
    Code(String, String),
    Table(Vec<Vec<String>>),
    Rule,
}

thread_local! {
    /// Parsed answers by content. Every render shows every turn, and while
    /// one answer streams the others have not changed: they are parsed once.
    static PARSED_BLOCKS: std::cell::RefCell<std::collections::HashMap<(u64, usize), std::rc::Rc<Vec<Block>>>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// [`blocks`], remembered for sources already seen.
pub fn parsed_blocks(source: &str) -> std::rc::Rc<Vec<Block>> {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut hasher);
    let key = (hasher.finish(), source.len());
    PARSED_BLOCKS.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(parsed) = cache.get(&key) {
            return parsed.clone();
        }
        // A streaming answer adds an entry per delta; start over now and
        // then rather than keep every prefix.
        if cache.len() >= 256 {
            cache.clear();
        }
        let parsed = std::rc::Rc::new(blocks(source));
        cache.insert(key, parsed.clone());
        parsed
    })
}

pub fn blocks(source: &str) -> Vec<Block> {
    let mut result = Vec::new();
    let mut paragraph = Vec::new();
    let mut code: Option<(String, Vec<String>)> = None;
    let flush = |paragraph: &mut Vec<String>, result: &mut Vec<Block>| {
        if !paragraph.is_empty() {
            result.push(Block::Paragraph(paragraph.join(" ")));
            paragraph.clear();
        }
    };
    let mut lines_iter = source.lines().peekable();
    while let Some(line) = lines_iter.next() {
        let trimmed = line.trim();
        if let Some((language, lines)) = &mut code {
            if trimmed.starts_with("```") {
                result.push(Block::Code(language.clone(), lines.join("\n")));
                code = None;
            } else {
                lines.push(line.to_string());
            }
            continue;
        }
        if let Some(language) = trimmed.strip_prefix("```") {
            flush(&mut paragraph, &mut result);
            code = Some((language.to_string(), vec![]));
        } else if trimmed.is_empty() {
            flush(&mut paragraph, &mut result);
        } else if matches!(trimmed, "---" | "***" | "___") {
            flush(&mut paragraph, &mut result);
            result.push(Block::Rule);
        } else if let Some(quote) = trimmed.strip_prefix("> ") {
            flush(&mut paragraph, &mut result);
            result.push(Block::Quote(quote.to_string()));
        } else {
            if trimmed.contains('|') && lines_iter.peek().is_some_and(|line| table_separator(line))
            {
                flush(&mut paragraph, &mut result);
                let mut rows = vec![table_cells(trimmed)];
                lines_iter.next();
                while lines_iter
                    .peek()
                    .is_some_and(|line| !line.trim().is_empty() && line.contains('|'))
                {
                    rows.push(table_cells(lines_iter.next().unwrap()));
                }
                result.push(Block::Table(rows));
                continue;
            }
            let hashes = trimmed.chars().take_while(|c| *c == '#').count();
            if (1..=6).contains(&hashes) && trimmed.as_bytes().get(hashes) == Some(&b' ') {
                flush(&mut paragraph, &mut result);
                result.push(Block::Heading(hashes, trimmed[hashes + 1..].to_string()));
                continue;
            }
            let indent = line.chars().take_while(|c| c.is_whitespace()).count() / 2;
            let bullet = ["- ", "* ", "• "]
                .into_iter()
                .find_map(|prefix| trimmed.strip_prefix(prefix));
            let ordered = trimmed
                .find(". ")
                .filter(|end| *end > 0 && trimmed[..*end].chars().all(|c| c.is_ascii_digit()));
            if let Some(text) = bullet {
                flush(&mut paragraph, &mut result);
                result.push(Block::Item(indent, "•".into(), text.into()));
            } else if let Some(end) = ordered {
                flush(&mut paragraph, &mut result);
                result.push(Block::Item(
                    indent,
                    trimmed[..end + 1].into(),
                    trimmed[end + 2..].into(),
                ));
            } else {
                paragraph.push(trimmed.to_string());
            }
        }
    }
    if let Some((language, lines)) = code {
        result.push(Block::Code(language, lines.join("\n")));
    }
    flush(&mut paragraph, &mut result);
    result
}
fn table_cells(line: &str) -> Vec<String> {
    line.trim()
        .trim_matches('|')
        .split('|')
        .map(|c| c.trim().to_string())
        .collect()
}
fn table_separator(line: &str) -> bool {
    let cells = table_cells(line);
    cells.len() > 1
        && cells
            .iter()
            .all(|cell| cell.contains('-') && cell.chars().all(|c| matches!(c, '-' | ':' | ' ')))
}

/// Formatting ranges are byte offsets, matching GPUI's shaped-text contract.
/// Unmatched delimiters stay visible while an actual streamed reply is incomplete.
pub(crate) fn inline(source: &str, theme: Theme) -> (String, Vec<(Range<usize>, HighlightStyle)>) {
    let mut output = String::new();
    let mut highlights = Vec::new();
    let mut at = 0;
    while at < source.len() {
        let rest = &source[at..];
        if rest.starts_with('\\')
            && let Some(next) = rest[1..].chars().next()
        {
            output.push(next);
            at += 1 + next.len_utf8();
            continue;
        }
        let marker = if rest.starts_with("**") {
            "**"
        } else if rest.starts_with("__") {
            "__"
        } else if rest.starts_with('`') {
            "`"
        } else if rest.starts_with('*') {
            "*"
        } else {
            ""
        };
        if !marker.is_empty()
            && let Some(end) = rest[marker.len()..].find(marker)
            && end > 0
        {
            let start = output.len();
            output.push_str(&rest[marker.len()..marker.len() + end]);
            let style = if marker.len() == 2 {
                HighlightStyle {
                    font_weight: Some(FontWeight::SEMIBOLD),
                    ..Default::default()
                }
            } else if marker == "`" {
                HighlightStyle {
                    background_color: Some(theme.hover),
                    ..Default::default()
                }
            } else {
                HighlightStyle {
                    font_style: Some(FontStyle::Italic),
                    ..Default::default()
                }
            };
            highlights.push((start..output.len(), style));
            at += marker.len() * 2 + end;
        } else {
            let next = rest.chars().next().unwrap();
            output.push(next);
            at += next.len_utf8();
        }
    }
    (output, highlights)
}

pub fn inline_text(source: &str, theme: Theme) -> StyledText {
    let (text, highlights) = inline(source, theme);
    StyledText::new(text).with_highlights(highlights)
}

type LinkedInline = (
    String,
    Vec<(Range<usize>, HighlightStyle)>,
    Vec<(Range<usize>, String)>,
);

#[cfg(test)]
fn linked_inline(source: &str, theme: Theme) -> LinkedInline {
    linked_inline_with_citations(source, theme, &[])
}

/// The mark a citation is shown as: its number in a circle. Unicode has
/// those up to 50 in three runs (① to ⑳, ㉑ to ㉟, ㊱ to ㊿). A long meeting
/// goes past that; from 51 the number is written out in brackets, so that
/// two marks side by side do not read as one number.
fn citation_mark(number: u32) -> String {
    let circled = match number {
        1..=20 => char::from_u32(0x2460 + number - 1),
        21..=35 => char::from_u32(0x3251 + number - 21),
        36..=50 => char::from_u32(0x32b1 + number - 36),
        _ => None,
    };
    circled.map_or_else(|| format!("({number})"), String::from)
}

fn citation_tokens(source: &str, citations: &[ChatCitationDTO]) -> String {
    rewrite_citation_tokens(source, citations, |id| {
        let label = citation_mark(id[1..].parse::<u32>().unwrap_or(0));
        format!("[{label}](minutes-source:{id})")
    })
}

fn rewrite_citation_tokens(
    source: &str,
    citations: &[ChatCitationDTO],
    render: impl Fn(&str) -> String,
) -> String {
    let mut output = String::new();
    let mut cursor = 0;
    while let Some(relative) = source[cursor..].find("[[") {
        let start = cursor + relative;
        let Some(end) = source[start + 2..].find("]]").map(|end| start + 2 + end) else {
            break;
        };
        let id = &source[start + 2..end];
        output.push_str(&source[cursor..start]);
        let escaped = source[..start]
            .bytes()
            .rev()
            .take_while(|byte| *byte == b'\\')
            .count()
            % 2
            == 1;
        let in_code = source[..start].bytes().filter(|byte| *byte == b'`').count() % 2 == 1;
        if !escaped && !in_code && valid_citation_id(id) {
            if citations.iter().any(|citation| citation.id == id) {
                output.push_str(&render(id));
            }
        } else {
            output.push_str(&source[start..end + 2]);
        }
        cursor = end + 2;
    }
    output.push_str(&source[cursor..]);
    output
}

fn linked_inline_with_citations(
    source: &str,
    theme: Theme,
    citations: &[ChatCitationDTO],
) -> LinkedInline {
    let prepared = citation_tokens(source, citations);
    let source = prepared.as_str();
    let mut output = String::new();
    let mut highlights = Vec::new();
    let mut links = Vec::new();
    let mut start = 0;
    let mut search = 0;
    while let Some(relative) = source[search..].find('[') {
        let open = search + relative;
        let Some(end_label) = source[open + 1..].find(']').map(|i| open + 1 + i) else {
            break;
        };
        if !source[end_label..].starts_with("](") {
            search = open + 1;
            continue;
        }
        let Some(end_url) = source[end_label + 2..].find(')').map(|i| end_label + 2 + i) else {
            break;
        };
        let url = &source[end_label + 2..end_url];
        let is_source = url
            .strip_prefix("minutes-source:")
            .is_some_and(|id| citations.iter().any(|citation| citation.id == id));
        if !(url.starts_with("https://") || url.starts_with("http://") || is_source)
            || url.chars().any(char::is_whitespace)
        {
            search = open + 1;
            continue;
        }
        for (part, text) in [&source[start..open], &source[open + 1..end_label]]
            .into_iter()
            .enumerate()
        {
            let offset = output.len();
            let (rendered, ranges) = inline(text, theme);
            output.push_str(&rendered);
            highlights.extend(
                ranges
                    .into_iter()
                    .map(|(range, style)| ((range.start + offset)..(range.end + offset), style)),
            );
            if part == 1 {
                links.push((offset..output.len(), url.to_string()));
            }
        }
        start = end_url + 1;
        search = start;
    }
    let offset = output.len();
    let (text, ranges) = inline(&source[start..], theme);
    output.push_str(&text);
    highlights.extend(
        ranges
            .into_iter()
            .map(|(range, style)| ((range.start + offset)..(range.end + offset), style)),
    );
    for (range, _) in &links {
        highlights.push((
            range.clone(),
            HighlightStyle {
                color: Some(theme.accent),
                ..Default::default()
            },
        ));
    }
    (output, highlights, links)
}
fn markdown_with_inline(
    source: &str,
    theme: Theme,
    compact: bool,
    render_inline: impl Fn(&str, usize) -> InteractiveText,
) -> Div {
    let mut result = div()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(12.))
        .text_color(theme.text)
        .text_size(px(if compact { 14. } else { 16. }))
        .line_height(px(if compact { 22. } else { 24. }));
    for (index, block) in parsed_blocks(source).iter().cloned().enumerate() {
        result = result.child(match block {
            Block::Heading(level, text) => div()
                .mt(px(if level < 3 { 8. } else { 2. }))
                .font_weight(FontWeight::SEMIBOLD)
                .text_size(px(if compact {
                    16.
                } else if level < 3 {
                    20.
                } else {
                    16.
                }))
                .child(render_inline(&text, index)),
            Block::Paragraph(text) => div().child(render_inline(&text, index)),
            Block::Item(indent, marker, text) => div()
                .ml(px(indent.min(5) as f32 * 16.))
                .flex()
                .items_start()
                .gap(px(8.))
                .child(
                    div()
                        .w(px((marker.chars().count() as f32 * 8.).max(14.)))
                        .whitespace_nowrap()
                        .flex_shrink_0()
                        .child(marker),
                )
                .child(div().flex_1().min_w_0().child(render_inline(&text, index))),
            Block::Quote(text) => div()
                .pl(px(12.))
                .border_l_2()
                .border_color(theme.border)
                .text_color(theme.muted)
                .child(render_inline(&text, index)),
            Block::Code(language, text) => div()
                .rounded(px(8.))
                .bg(theme.hover)
                .p(px(12.))
                .flex()
                .flex_col()
                .gap(px(8.))
                .when(!language.is_empty(), |d| {
                    d.child(
                        div()
                            .text_size(px(11.))
                            .text_color(theme.muted)
                            .child(language),
                    )
                })
                .child(
                    div()
                        .font_family("Menlo")
                        .text_size(px(12.))
                        .line_height(px(18.))
                        .child(text),
                ),
            Block::Table(rows) => div()
                .rounded(px(8.))
                .border_1()
                .border_color(theme.border)
                .overflow_hidden()
                .flex()
                .flex_col()
                .children(rows.into_iter().enumerate().map(|(row, cells)| {
                    div()
                        .flex()
                        .when(row == 0, |d| {
                            d.bg(theme.hover).font_weight(FontWeight::SEMIBOLD)
                        })
                        .children(cells.into_iter().enumerate().map(|(column, cell)| {
                            div()
                                .flex_1()
                                .min_w_0()
                                .px(px(8.))
                                .py(px(6.))
                                .border_b_1()
                                .border_color(theme.border)
                                .child(render_inline(&cell, index * 10000 + row * 100 + column))
                        }))
                })),
            Block::Rule => div().h(px(1.)).my(px(4.)).bg(theme.border),
        });
    }
    result
}

#[cfg(test)]
mod tests {
    use super::{
        ArchivedChat, Block, HistoryFile, archived_thread_scope, blocks,
        follow_up_prompt, inline, linked_inline, store_history, upsert_chat_archive,
    };
    use super::{
        ChatTurnAnnotations, chat_source_note, citation_tokens, linked_inline_with_citations,
        valid_citation_id,
    };
    use crate::ui::Theme;
    #[test]
    fn a_conversation_is_listed_under_the_start_of_its_first_question() {
        use super::chat_title;
        assert_eq!(chat_title("List recent todos"), "List recent todos");
        // On one line, whatever the question's own line breaks.
        assert_eq!(chat_title("  Summarise\nthe   launch plan "), "Summarise the launch plan");
        // Cut to seventy characters, and it says so.
        let long = "Summarise the launch plan in a table, with the code we discussed, and keep going";
        let title = chat_title(long);
        assert!(title.chars().count() <= 70 && title.chars().count() >= 60);
        assert!(title.ends_with('…'));
        assert!(long.starts_with(title.trim_end_matches('…')));
        // Seventy characters exactly are kept whole, in any script.
        let exact = "é".repeat(70);
        assert_eq!(chat_title(&exact), exact);
    }

    #[test]
    fn an_answer_already_seen_is_not_parsed_again() {
        let source = "## Plan\n\n- one\n- two";
        let first = super::parsed_blocks(source);
        assert_eq!(*first, blocks(source));
        // The same text hands back the same parse, not a new one.
        assert!(std::rc::Rc::ptr_eq(&first, &super::parsed_blocks(source)));
        // A streamed delta is different text and parses on its own.
        let longer = super::parsed_blocks("## Plan\n\n- one\n- two\n- three");
        assert!(!std::rc::Rc::ptr_eq(&first, &longer));
        assert_eq!(longer.len(), first.len() + 1);
    }

    #[test]
    fn withdrawn_citations_disappear_from_partial_answers_and_copy_while_retry_error_survives_history()
     {
        let partial = "Café launches Friday [[S1]]. Keep this partial response.";
        assert!(
            linked_inline_with_citations(partial, Theme::new(true), &[citation("S1")])
                .0
                .contains('①')
        );
        let mut annotations = ChatTurnAnnotations::default();
        annotations.error = Some(
            "A source changed while answering. Retry to use the current meeting notes.".into(),
        );
        let rendered =
            linked_inline_with_citations(partial, Theme::new(true), &annotations.citations);
        assert_eq!(
            rendered.0,
            "Café launches Friday . Keep this partial response."
        );
        assert!(rendered.2.is_empty());
        let copied = super::rewrite_citation_tokens(partial, &annotations.citations, |id| {
            format!("[{}]", &id[1..])
        });
        assert!(!copied.contains("[[") && !copied.contains("minutes-source:"));
        let restored: ChatTurnAnnotations =
            serde_json::from_slice(&serde_json::to_vec(&annotations).unwrap()).unwrap();
        assert_eq!(restored.error, annotations.error);
        assert_eq!(
            partial, "Café launches Friday [[S1]]. Keep this partial response.",
            "Presentation must not rewrite stored response text or destroy a partial answer"
        );
    }
    #[test]
    fn collection_quick_chats_float_but_sidebar_chat_remains_a_full_page() {
        use crate::app::Screen;
        for screen in [
            Screen::Home,
            Screen::MyNotes,
            Screen::Favorites,
        ] {
            assert!(super::floating_workspace_chat(screen));
        }
        for screen in [
            Screen::Chat,
            Screen::SearchResults,
            Screen::Settings,
            Screen::Account,
            Screen::Shared,
        ] {
            assert!(!super::floating_workspace_chat(screen));
        }
    }
    fn citation(id: &str) -> meeting_notes_api_types::ChatCitationDTO {
        meeting_notes_api_types::ChatCitationDTO {
            id: id.into(),
            note_id: "419d65d1-7a2c-44ca-84cc-ad489522d561".into(),
            title: "Café planning".into(),
            source: meeting_notes_api_types::ChatCitationDtoSource::Transcript,
            segment_id: Some("419d65d1-7a2c-44ca-84cc-ad489522d562".into()),
            start_ms: Some(18000),
            quote: "Mira owns café 🦊.".into(),
            transcript_revision: 7,
        }
    }
    #[test]
    fn only_issued_citation_tokens_become_clickable_and_unicode_ranges_stay_exact() {
        let citations = vec![citation("S1")];
        let (text, _, links) = linked_inline_with_citations(
            "**Café** 🦊 [[S1]] and [[S9]] [docs](https://example.test).",
            Theme::new(true),
            &citations,
        );
        assert_eq!(text, "Café 🦊 ① and  docs.");
        assert_eq!(links.len(), 2);
        assert_eq!(&text[links[0].0.clone()], "①");
        assert_eq!(links[0].1, "minutes-source:S1");
        assert_eq!(&text[links[1].0.clone()], "docs");
        assert_eq!(links[1].1, "https://example.test");
        assert_eq!(
            citation_tokens("`[[S1]]` \\[[S1]] partial [[S", &citations),
            "`[[S1]]` \\[[S1]] partial [[S"
        );
        assert!(
            linked_inline_with_citations(
                "[fake](minutes-source:S9) [bad](javascript:alert)",
                Theme::new(true),
                &citations
            )
            .2
            .is_empty()
        );
        // Every mark is one glyph up to 50, so neighbours cannot run
        // together ("[[S25]][[S26]]" used to read "2526"); beyond that the
        // number is in brackets.
        assert_eq!(
            [1, 20, 21, 35, 36, 50].map(super::citation_mark).concat(),
            "①⑳㉑㉟㊱㊿"
        );
        let later = vec![citation("S25"), citation("S26"), citation("S51"), citation("S52")];
        let (text, _, links) = linked_inline_with_citations(
            "Moved. [[S25]][[S26]] Later. [[S51]][[S52]]",
            Theme::new(true),
            &later,
        );
        assert_eq!(text, "Moved. ㉕㉖ Later. (51)(52)");
        assert_eq!(links.len(), 4);
        for id in ["S0", "S01", "S1000", "S-1", "S1/other", "s1", "S"] {
            assert!(!valid_citation_id(id));
        }
        assert!(valid_citation_id("S999"));
    }
    #[test]
    fn tool_updates_are_monotonic_and_history_keeps_sources_without_open_disclosures() {
        use meeting_notes_api_types::{
            ChatToolProgressDTO, ChatToolProgressDtoStatus as Status, Name,
        };
        let mut annotations = ChatTurnAnnotations::default();
        let mut tool = ChatToolProgressDTO {
            call_id: "419d65d1-7a2c-44ca-84cc-ad489522d561".into(),
            name: Name::SearchMeetings,
            status: Status::Running,
            label: "Searching meetings".into(),
            result_count: None,
            results: None,
        };
        annotations.update_tool(tool.clone());
        tool.status = Status::Completed;
        tool.result_count = Some(2);
        annotations.update_tool(tool.clone());
        tool.status = Status::Running;
        annotations.update_tool(tool.clone());
        assert_eq!(annotations.tools.len(), 1);
        assert_eq!(annotations.tools[0].status, Status::Completed);
        assert_eq!(annotations.tools[0].result_count, Some(2));
        tool.name = Name::GetMeetings;
        tool.status = Status::Completed;
        annotations.update_tool(tool);
        assert_eq!(annotations.tools[0].name, Name::SearchMeetings);
        annotations.citations = vec![citation("S1")];
        annotations.elapsed_ms = Some(3123);
        annotations.expanded = true;
        annotations.expanded_tools.insert("runtime-only".into());
        let json = serde_json::to_value(&annotations).unwrap();
        assert!(json.get("expanded").is_none());
        assert!(json.get("expanded_tools").is_none());
        let restored: ChatTurnAnnotations = serde_json::from_value(json).unwrap();
        assert_eq!(restored.citations[0].quote, "Mira owns café 🦊.");
        assert_eq!(restored.elapsed_ms, Some(3123));
        assert!(!restored.expanded);
        assert!(restored.expanded_tools.is_empty());
    }
    #[test]
    fn a_citation_depends_on_its_own_passage_not_on_the_note_revision() {
        use meeting_notes_api_types::{AudioChannel, TranscriptSegmentDTO};
        let cited = citation("S1");
        let segment = |id: &str, text: &str, start_ms: i64| TranscriptSegmentDTO {
            id: id.into(),
            channel: AudioChannel::Mic,
            speaker_id: "mic:part:speaker_1".into(),
            speaker_label: None,
            text: text.into(),
            is_final: true,
            start_ms,
            end_ms: Some(start_ms + 4_000),
        };
        let passage = segment(
            "419d65d1-7a2c-44ca-84cc-ad489522d562",
            "Right. Mira owns café 🦊. Next topic.",
            18_000,
        );
        let earlier = segment("segment-earlier", "Welcome, everyone.", 2_000);
        let note =
            |segments: Vec<TranscriptSegmentDTO>, transcript_revision: u64| crate::model::Note {
                server_transcript_segments: segments,
                transcript_revision,
                ..Default::default()
            };
        // As issued, at the revision the citation carries.
        let issued = note(vec![earlier.clone(), passage.clone()], 7);
        assert!(super::citation_passage_intact(&issued, &cited));
        // Three more parts of the meeting have landed since.
        let grown = note(
            vec![
                earlier.clone(),
                passage.clone(),
                segment("segment-later-1", "Anything else?", 70_000),
                segment("segment-later-2", "No, that is all.", 125_000),
                segment("segment-later-3", "Thanks, everyone.", 131_000),
            ],
            10,
        );
        assert!(super::citation_passage_intact(&grown, &cited));
        // The transcript panel lands on the same passage in the grown transcript.
        assert_eq!(
            crate::note::transcript_quote_target(
                &grown,
                cited.segment_id.as_deref(),
                &cited.quote,
                None
            ),
            Ok(1)
        );
        // An unrelated segment was corrected.
        let mut unrelated = grown.clone();
        unrelated.server_transcript_segments[0].text = "Welcome back, everyone.".into();
        unrelated.transcript_revision = 11;
        assert!(super::citation_passage_intact(&unrelated, &cited));
        // The cited passage itself was edited away, moved, is not final, or was deleted.
        let mut edited = grown.clone();
        edited.server_transcript_segments[1].text = "Right. Noor owns the café.".into();
        assert!(!super::citation_passage_intact(&edited, &cited));
        assert_eq!(
            crate::note::transcript_quote_target(
                &edited,
                cited.segment_id.as_deref(),
                &cited.quote,
                None
            ),
            Err("This source quotation is no longer available.")
        );
        let mut moved = grown.clone();
        moved.server_transcript_segments[1].start_ms = 19_000;
        assert!(!super::citation_passage_intact(&moved, &cited));
        let mut interim = grown.clone();
        interim.server_transcript_segments[1].is_final = false;
        assert!(!super::citation_passage_intact(&interim, &cited));
        let mut deleted = grown.clone();
        deleted.server_transcript_segments.remove(1);
        assert!(!super::citation_passage_intact(&deleted, &cited));
        // The same words stored as another segment are not the cited passage.
        let mut replaced = grown.clone();
        replaced.server_transcript_segments[1].id = "segment-replacement".into();
        assert!(!super::citation_passage_intact(&replaced, &cited));
    }
    #[test]
    fn citations_never_resolve_to_a_deleted_note_or_another_server() {
        let note = crate::model::Note {
            id: 41,
            server_id: Some("note-one".into()),
            server_url: Some("https://one.example".into()),
            ..Default::default()
        };
        assert_eq!(
            chat_source_note(&[note.clone()], "note-one", "https://one.example", None),
            Some(41)
        );
        assert_eq!(
            chat_source_note(&[note.clone()], "note-one", "https://two.example", None),
            None
        );
        assert_eq!(
            chat_source_note(&[note.clone()], "note-other", "https://one.example", None),
            None
        );
        assert_eq!(
            chat_source_note(
                &[crate::model::Note {
                    trashed: true,
                    ..note
                }],
                "note-one",
                "https://one.example",
                None
            ),
            None
        );
    }
    #[test]
    fn older_answer_follow_up_names_the_selected_question_and_answer() {
        let turns = vec![
            ("Who owns café?".into(), "Mira owns café 🦊.".into()),
            ("When is release?".into(), "Friday.".into()),
        ];
        let prompt = follow_up_prompt(0, &turns).unwrap();
        assert!(prompt.contains("Who owns café?"));
        assert!(prompt.contains("Mira owns café 🦊."));
        assert!(!prompt.contains("Friday"));
        assert_eq!(
            follow_up_prompt(1, &turns).as_deref(),
            Some("Say more about that.")
        );
        assert!(follow_up_prompt(2, &turns).is_none());
    }
    #[test]
    fn selected_answer_follow_up_is_bounded_without_splitting_unicode() {
        let turns = vec![
            ("🦊".repeat(4000), "é".repeat(10000)),
            ("Latest".into(), "Other".into()),
        ];
        let prompt = follow_up_prompt(0, &turns).unwrap();
        assert_eq!(prompt.matches('🦊').count(), 2000);
        assert_eq!(prompt.matches('é').count(), 6000);
        assert!(prompt.encode_utf16().count() < 20_000);
    }
    #[test]
    fn response_blocks_preserve_nested_lists_code_and_plain_html() {
        let b = blocks(
            "## Actions\n\n- **Mira** owns it\n  1. Review café 🦊\n\n```rust\nlet x = 1;\n```\n\n<script>alert(1)</script>",
        );
        assert_eq!(b[0], Block::Heading(2, "Actions".into()));
        assert_eq!(b[2], Block::Item(1, "1.".into(), "Review café 🦊".into()));
        assert_eq!(b[3], Block::Code("rust".into(), "let x = 1;".into()));
        assert_eq!(b[4], Block::Paragraph("<script>alert(1)</script>".into()));
    }
    #[test]
    fn inline_ranges_are_unicode_safe_and_streaming_delimiters_are_preserved() {
        let (text, ranges) = inline("🦊 **café** and *résumé* and `x`", Theme::new(true));
        assert_eq!(text, "🦊 café and résumé and x");
        assert_eq!(&text[ranges[0].0.clone()], "café");
        assert_eq!(&text[ranges[1].0.clone()], "résumé");
        assert_eq!(
            inline("partial **bold", Theme::new(true)).0,
            "partial **bold"
        );
        assert_eq!(inline("\\*literal\\*", Theme::new(true)).0, "*literal*");
    }
    #[test]
    fn private_history_retains_session_context_and_preserves_corrupt_original() {
        let root =
            std::env::temp_dir().join(format!("meeting-chat-history-{}", uuid::Uuid::new_v4()));
        let path = root.join("chats.json");
        let sessions = vec![ArchivedChat {
            id: "thread-one".into(),
            note_id: 41,
            account: None,
            title: "Café 🦊".into(),
            messages: vec![("Who owns it?".into(), "Mira.".into())],
            interrupted: vec![],
            annotations: Default::default(),
            updated_at: Some("2026-11-13T12:00:00Z".into()),
        }];
        store_history(&path, &sessions).unwrap();
        let stored: HistoryFile = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(stored.sessions[0].note_id, 41);
        assert_eq!(stored.sessions[0].messages[0].1, "Mira.");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        std::fs::write(&path, b"corrupt existing data").unwrap();
        assert!(store_history(&path, &sessions).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"corrupt existing data");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn saving_an_existing_thread_never_changes_its_note_or_workspace_scope() {
        let entry = |id: &str, note_id, question: &str| ArchivedChat {
            id: id.into(),
            note_id,
            account: None,
            title: question.into(),
            messages: vec![(question.into(), "Answer from its original context".into())],
            interrupted: vec![],
            annotations: Default::default(),
            updated_at: None,
        };
        let mut archives = vec![entry("note-thread", 41, "Original note question")];
        upsert_chat_archive(
            &mut archives,
            entry("note-thread", 0, "Updated note question"),
        );
        assert_eq!(archives.len(), 1);
        assert_eq!(
            archives[0].note_id, 41,
            "Saving after a note exit cannot reclassify this thread as workspace chat"
        );
        assert_eq!(archives[0].messages[0].0, "Updated note question");
        assert_ne!(
            archived_thread_scope(&archives, "note-thread", 0),
            0,
            "Actual request preflight must reject this stale workspace scope"
        );
        upsert_chat_archive(
            &mut archives,
            entry("workspace-thread", 0, "Workspace question"),
        );
        upsert_chat_archive(
            &mut archives,
            entry("workspace-thread", 93, "Updated workspace question"),
        );
        assert_eq!(
            archives
                .iter()
                .find(|chat| chat.id == "workspace-thread")
                .unwrap()
                .note_id,
            0
        );
        assert_eq!(
            archived_thread_scope(&archives, "new-thread", 93),
            93,
            "A new thread can intentionally select its initial scope"
        );
    }

    #[test]
    fn thread_ages_read_as_the_lists_print_them() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-11-16T18:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let ago = |minutes: i64| (now - chrono::Duration::minutes(minutes)).to_rfc3339();
        assert_eq!(super::thread_age(&ago(0), now).as_deref(), Some("now"));
        assert_eq!(super::thread_age(&ago(6), now).as_deref(), Some("6m"));
        assert_eq!(super::thread_age(&ago(6 * 60 + 5), now).as_deref(), Some("6h"));
        assert_eq!(super::thread_age(&ago(26 * 60), now).as_deref(), Some("1d"));
        assert_eq!(super::thread_age(&ago(65 * 24 * 60), now).as_deref(), Some("2mo"));
        assert_eq!(super::thread_age("", now), None);
    }

    #[test]
    fn an_archived_thread_without_a_date_is_listed_as_earlier() {
        let old: ArchivedChat = serde_json::from_str(
            r#"{"id":"legacy","note_id":41,"title":"Notes","messages":[],"interrupted":[]}"#,
        )
        .unwrap();
        assert!(old.updated_at.is_none());
        assert_eq!(super::chat_date_group(old.updated_at.as_deref()), "Earlier");
    }

    #[test]
    fn history_dates_use_local_days_and_preserve_unknown_dates() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-11-13T18:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Local);
        let today = now.to_rfc3339();
        let yesterday = (now - chrono::Duration::days(1)).to_rfc3339();
        assert_eq!(super::chat_date_group_at(Some(&today), now), "Today");
        assert_eq!(
            super::chat_date_group_at(Some(&yesterday), now),
            "Yesterday"
        );
        assert_eq!(super::chat_date_group_at(None, now), "Earlier");
        assert_eq!(
            super::chat_date_group_at(Some("not-a-date"), now),
            "Earlier"
        );
        assert_eq!(
            super::chat_date_group_at(Some("2025-09-15T18:00:00Z"), now),
            "September 2025"
        );
    }
    #[test]
    fn table_and_link_rendering_retains_structure_without_active_unsafe_schemes() {
        assert_eq!(
            blocks("| Owner | Due |\n| --- | :---: |\n| Mira | Friday |"),
            vec![Block::Table(vec![
                vec!["Owner".into(), "Due".into()],
                vec!["Mira".into(), "Friday".into()]
            ])]
        );
        let (text, _, links) = linked_inline(
            "See [café 🦊](https://example.com) and [bad](javascript:alert)",
            Theme::new(true),
        );
        assert_eq!(links.len(), 1);
        assert_eq!(&text[links[0].0.clone()], "café 🦊");
        assert!(text.contains("[bad](javascript:alert)"));
    }
}

/// An answer belongs to its conversation: it keeps arriving while the
/// conversation is off screen, and a question sent meanwhile waits its turn.
#[cfg(test)]
mod stream_tests {
    use super::{ActiveChat, Applied, ArchivedChat, Begun, Chats, ChatTurnAnnotations, ParkedChat};
    use crate::server::ChatStreamEvent::{Delta, Done, Error};
    use std::collections::{HashMap, HashSet};
    use std::prelude::v1::test;
    use std::sync::atomic::Ordering;

    /// The chat state the app keeps, and the two things it does with it:
    /// put another conversation on screen, and send what the composer holds.
    #[derive(Default)]
    struct Desk {
        note: u64,
        messages: Vec<(String, String)>,
        annotations: HashMap<usize, ChatTurnAnnotations>,
        interrupted: HashSet<usize>,
        thread_id: String,
        stream: Option<ActiveChat>,
        queue: Vec<String>,
        archives: Vec<ArchivedChat>,
        parked: HashMap<String, ParkedChat>,
        epoch: u64,
        threads: u32,
    }

    impl Desk {
        fn on_note(note: u64) -> Self {
            let mut desk = Self::default();
            desk.note = note;
            desk.thread_id = "thread-1".into();
            desk.threads = 1;
            desk
        }
        fn chats(&mut self) -> Chats<'_> {
            Chats {
                messages: &mut self.messages,
                annotations: &mut self.annotations,
                interrupted: &mut self.interrupted,
                thread_id: &mut self.thread_id,
                stream: &mut self.stream,
                queue: &mut self.queue,
                archives: &mut self.archives,
                parked: &mut self.parked,
                epoch: &mut self.epoch,
            }
        }
        /// Leave for another note (0: Home and the other pages), as
        /// `navigate` and `open_note` do.
        fn go(&mut self, note: u64) {
            let scope = self.note;
            self.chats().park(scope);
            self.note = note;
            match self.chats().thread_for_note(note) {
                Some(chat) => {
                    self.messages = chat.messages;
                    self.interrupted = chat.interrupted.into_iter().collect();
                    self.annotations = chat.annotations;
                    self.thread_id = chat.id;
                    self.chats().attach();
                }
                None => {
                    self.messages.clear();
                    self.interrupted.clear();
                    self.annotations.clear();
                    self.threads += 1;
                    self.thread_id = format!("thread-{}", self.threads);
                }
            }
        }
        /// Return in the composer: send, or wait behind the answer arriving.
        fn send(&mut self, question: &str) -> Option<(String, Begun)> {
            if self.stream.is_some() {
                self.queue.push(question.into());
                return None;
            }
            let thread = self.thread_id.clone();
            let begun = self.chats().begin(&thread, question.into(), Vec::new())?;
            Some((thread, begun))
        }
        fn archived(&self, thread: &str) -> &ArchivedChat {
            self.archives.iter().find(|chat| chat.id == thread).unwrap()
        }
    }

    #[test]
    fn an_answer_keeps_arriving_off_screen_and_is_complete_when_the_user_comes_back() {
        let mut desk = Desk::on_note(7);
        let (thread, turn) = desk.send("What was decided?").unwrap();
        assert_eq!(
            desk.chats().event(&thread, turn.epoch, turn.index, Delta("Three ".into())),
            Applied::Updated { loaded: true }
        );

        // Home: the note's conversation leaves the screen mid-answer.
        desk.go(0);
        assert!(desk.stream.is_none() && desk.messages.is_empty());
        assert!(!turn.cancel.load(Ordering::Acquire), "leaving must not cancel");
        assert_eq!(
            desk.chats().event(&thread, turn.epoch, turn.index, Delta("steps, ".into())),
            Applied::Updated { loaded: false }
        );

        // Another note, and the answer still arrives.
        desk.go(9);
        assert_eq!(
            desk.chats().event(&thread, turn.epoch, turn.index, Delta("by Friday.".into())),
            Applied::Updated { loaded: false }
        );
        assert_eq!(
            desk.chats().event(&thread, turn.epoch, turn.index, Done),
            Applied::Finished { loaded: false, next: None, rest: vec![] }
        );
        assert!(desk.parked.is_empty());
        let saved = desk.archived(&thread);
        assert_eq!(saved.note_id, 7);
        assert_eq!(saved.messages[0].1, "Three steps, by Friday.");
        assert!(saved.interrupted.is_empty());
        assert!(saved.annotations[&0].elapsed_ms.is_some());

        // Back on the note: the whole answer, nothing still arriving.
        desk.go(7);
        assert_eq!(desk.thread_id, thread);
        assert_eq!(desk.messages, [("What was decided?".into(), "Three steps, by Friday.".into())]);
        assert!(desk.stream.is_none() && desk.interrupted.is_empty());
        // A late event of the finished stream changes nothing.
        assert_eq!(desk.chats().event(&thread, turn.epoch, turn.index, Delta("x".into())), Applied::Gone);
    }

    #[test]
    fn thinking_ends_when_the_first_of_the_answer_arrives_and_its_time_is_kept() {
        let mut desk = Desk::on_note(0);
        let (thread, turn) = desk.send("List recent todos").unwrap();
        // Nothing of the answer yet: no time, so the turn says "Thinking…".
        desk.chats().event(&thread, turn.epoch, turn.index, Delta(String::new()));
        assert!(desk.annotations.get(&0).and_then(|item| item.elapsed_ms).is_none());
        std::thread::sleep(std::time::Duration::from_millis(30));
        desk.chats().event(&thread, turn.epoch, turn.index, Delta("Here ".into()));
        let thought = desk.annotations[&0].elapsed_ms.expect("set by the first text");
        assert!(thought >= 30);
        // Writing the rest takes longer, but the answer thought for as long
        // as it took to start.
        std::thread::sleep(std::time::Duration::from_millis(30));
        desk.chats().event(&thread, turn.epoch, turn.index, Delta("they are.".into()));
        desk.chats().event(&thread, turn.epoch, turn.index, Done);
        assert_eq!(desk.annotations[&0].elapsed_ms, Some(thought));
        // An answer that never wrote anything thought until it ended.
        let (thread, turn) = desk.send("And the decisions?").unwrap();
        desk.chats().event(&thread, turn.epoch, turn.index, Done);
        assert!(desk.annotations[&1].elapsed_ms.is_some());
    }

    #[test]
    fn coming_back_mid_answer_puts_the_stream_on_screen_again_and_stop_still_cancels_it() {
        let mut desk = Desk::on_note(7);
        let (thread, turn) = desk.send("Summarise the risks").unwrap();
        desk.chats().event(&thread, turn.epoch, turn.index, Delta("Two risks: ".into()));
        desk.go(3);
        desk.chats().event(&thread, turn.epoch, turn.index, Delta("cost".into()));

        desk.go(7);
        assert!(desk.stream.is_some(), "the answer is arriving on screen again");
        assert!(desk.parked.is_empty());
        assert_eq!(desk.messages[0].1, "Two risks: cost");
        assert_eq!(
            desk.chats().event(&thread, turn.epoch, turn.index, Delta(" and".into())),
            Applied::Updated { loaded: true }
        );
        assert_eq!(desk.messages[0].1, "Two risks: cost and");

        assert_eq!(desk.chats().stop(), Some(vec![]));
        assert!(turn.cancel.load(Ordering::Acquire));
        assert!(desk.stream.is_none() && desk.interrupted.contains(&0));
        assert_eq!(desk.messages[0].1, "Two risks: cost and", "what arrived is kept");
        assert_eq!(desk.chats().event(&thread, turn.epoch, turn.index, Delta(" time".into())), Applied::Gone);
        assert_eq!(desk.chats().stop(), None, "nothing left to stop");
    }

    #[test]
    fn a_workspace_answer_and_a_note_answer_arrive_side_by_side() {
        let mut desk = Desk::on_note(0);
        let (workspace, first) = desk.send("List recent todos").unwrap();
        desk.go(7);
        let (note, second) = desk.send("Who owns the rollout?").unwrap();
        assert_ne!(workspace, note);
        desk.chats().event(&workspace, first.epoch, first.index, Delta("Todo one".into()));
        desk.chats().event(&note, second.epoch, second.index, Delta("Maya".into()));
        assert_eq!(desk.messages[0].1, "Maya");
        assert_eq!(desk.archived(&workspace).messages[0].1, "Todo one");
        desk.chats().event(&workspace, first.epoch, first.index, Done);
        desk.chats().event(&note, second.epoch, second.index, Done);
        desk.go(0);
        assert_eq!(desk.thread_id, workspace);
        assert_eq!(desk.messages[0].1, "Todo one");
    }

    #[test]
    fn a_question_sent_while_an_answer_arrives_waits_and_is_sent_when_it_finishes() {
        let mut desk = Desk::on_note(7);
        let (thread, turn) = desk.send("What was decided?").unwrap();
        desk.chats().event(&thread, turn.epoch, turn.index, Delta("Three steps.".into()));

        // Return while the answer is arriving: nothing is sent yet.
        assert!(desk.send("And who owns it?").is_none());
        assert_eq!(desk.queue, ["And who owns it?"]);
        assert_eq!(desk.messages.len(), 1);

        let finished = desk.chats().event(&thread, turn.epoch, turn.index, Done);
        assert_eq!(
            finished,
            Applied::Finished { loaded: true, next: Some("And who owns it?".into()), rest: vec![] }
        );
        assert!(desk.queue.is_empty());
        let (_, next) = desk.send("And who owns it?").unwrap();
        assert_eq!(next.index, 1);
        assert_eq!(desk.messages[1], ("And who owns it?".into(), String::new()));
        // The finished answer is part of what the next question is sent with.
        assert_eq!(next.history.len(), 2);
        assert_eq!(next.history[1]["content"], "Three steps.");
    }

    #[test]
    fn stop_cancels_the_waiting_questions_too_and_hands_their_text_back() {
        let mut desk = Desk::on_note(7);
        let (thread, turn) = desk.send("What was decided?").unwrap();
        desk.send("And who owns it?");
        desk.send("By when?");
        assert_eq!(
            desk.chats().stop(),
            Some(vec!["And who owns it?".to_string(), "By when?".to_string()])
        );
        assert!(desk.queue.is_empty() && desk.stream.is_none());
        assert_eq!(desk.messages.len(), 1, "no waiting question was sent");
        assert_eq!(desk.messages[0].1, "Response stopped.");
        assert_eq!(desk.chats().event(&thread, turn.epoch, turn.index, Done), Applied::Gone);
    }

    #[test]
    fn a_waiting_question_that_was_removed_is_not_sent() {
        let mut desk = Desk::on_note(7);
        let (thread, turn) = desk.send("What was decided?").unwrap();
        desk.send("And who owns it?");
        desk.queue.remove(0);
        assert_eq!(
            desk.chats().event(&thread, turn.epoch, turn.index, Done),
            Applied::Finished { loaded: true, next: None, rest: vec![] }
        );
        assert_eq!(desk.messages.len(), 1);
    }

    #[test]
    fn a_waiting_question_is_sent_off_screen_when_its_conversation_is_not_showing() {
        let mut desk = Desk::on_note(7);
        let (thread, first) = desk.send("What was decided?").unwrap();
        desk.send("And who owns it?");
        desk.send("By when?");
        desk.go(0);
        assert!(desk.queue.is_empty(), "the queue left with its conversation");
        desk.chats().event(&thread, first.epoch, first.index, Delta("Three steps.".into()));

        let Applied::Finished { loaded: false, next: Some(question), rest } =
            desk.chats().event(&thread, first.epoch, first.index, Done)
        else {
            panic!("the first answer should finish off screen with a question waiting");
        };
        assert_eq!((question.as_str(), rest.as_slice()), ("And who owns it?", ["By when?".to_string()].as_slice()));
        let second = desk.chats().begin(&thread, question, rest).unwrap();
        assert_eq!(second.index, 1);
        assert_eq!(second.history[1]["content"], "Three steps.");
        desk.chats().event(&thread, second.epoch, second.index, Delta("Maya.".into()));

        // An answer that fails does not hold up what waits behind it.
        let Applied::Finished { loaded: false, next: Some(question), rest } =
            desk.chats().event(&thread, second.epoch, second.index, Error("offline".into()))
        else {
            panic!("the second answer should end with the last question waiting");
        };
        let third = desk.chats().begin(&thread, question, rest).unwrap();
        desk.chats().event(&thread, third.epoch, third.index, Delta("Friday.".into()));

        // Back mid-way through the third answer: it finishes on screen.
        desk.go(7);
        assert!(desk.stream.is_some());
        assert_eq!(
            desk.chats().event(&thread, third.epoch, third.index, Done),
            Applied::Finished { loaded: true, next: None, rest: vec![] }
        );
        let answers: Vec<_> = desk.messages.iter().map(|(_, answer)| answer.as_str()).collect();
        assert_eq!(answers, ["Three steps.", "Maya.", "Friday."]);
        assert_eq!(desk.interrupted, HashSet::from([1]));
        assert_eq!(desk.annotations[&1].error.as_deref(), Some("offline"));
    }

    #[test]
    fn quitting_stops_the_answers_arriving_off_screen_and_marks_them() {
        let mut desk = Desk::on_note(7);
        let (thread, turn) = desk.send("What was decided?").unwrap();
        desk.send("And who owns it?");
        desk.go(0);
        desk.chats().stop_parked();
        assert!(turn.cancel.load(Ordering::Acquire));
        assert!(desk.parked.is_empty());
        let saved = desk.archived(&thread);
        assert_eq!(saved.messages, [("What was decided?".into(), "Response stopped.".into())]);
        assert_eq!(saved.interrupted, [0]);
        assert_eq!(desk.chats().event(&thread, turn.epoch, turn.index, Done), Applied::Gone);
    }

    #[test]
    fn an_answer_whose_saved_conversation_was_deleted_is_cancelled() {
        let mut desk = Desk::on_note(7);
        let (thread, turn) = desk.send("What was decided?").unwrap();
        desk.go(0);
        desk.archives.retain(|chat| chat.id != thread);
        assert_eq!(desk.chats().event(&thread, turn.epoch, turn.index, Delta("x".into())), Applied::Gone);
        assert!(turn.cancel.load(Ordering::Acquire));
        assert!(desk.parked.is_empty());
    }
}

#[cfg(test)]
mod account_tests {
    use super::*;
    // `gpui::*` re-exports its own `test` attribute; these are plain unit tests.
    use std::prelude::v1::test;
    use crate::accounts::{AccountId, handle};

    fn thread(id: &str, note_id: u64, at: &str) -> ArchivedChat {
        ArchivedChat {
            id: id.into(),
            note_id,
            account: None,
            title: id.into(),
            messages: vec![("Question".into(), "Answer".into())],
            interrupted: vec![],
            annotations: Default::default(),
            updated_at: Some(at.into()),
        }
    }

    #[test]
    fn h6_a_thread_keeps_its_account_and_one_account_cannot_push_out_another() {
        let (a, b) = (
            AccountId::of("https://a.example", "00000000-0000-4000-8000-000000000001"),
            AccountId::of("https://b.example", "00000000-0000-4000-8000-000000000002"),
        );
        // Read from A's file: a thread across meetings and one about note 7.
        let threads = adopt_threads(
            vec![thread("across", 0, "2026-11-15T10:00:00Z"), thread("about-7", 7, "2026-11-15T11:00:00Z")],
            &a,
            1,
        );
        assert_eq!(threads[0].note_id, handle::NO_NOTE, "0 is never tagged");
        assert_eq!(threads[1].note_id, handle::handle(1, 7).unwrap());
        assert!(threads.iter().all(|chat| chat.account.as_ref() == Some(&a)));
        // Written back: file ids, and the account is implied by the file.
        let bytes = handle::encoding_account_file(|| encode_history(&threads)).unwrap();
        let file: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(file["sessions"][0]["note_id"], 0);
        assert_eq!(file["sessions"][1]["note_id"], 7);
        assert!(file["sessions"][0].get("account").is_none());
        let back = adopt_threads(serde_json::from_value(file["sessions"].clone()).unwrap(), &a, 1);
        assert_eq!(back.iter().map(|chat| (chat.id.clone(), chat.note_id)).collect::<Vec<_>>(),
                   threads.iter().map(|chat| (chat.id.clone(), chat.note_id)).collect::<Vec<_>>());
        // Twenty-five new threads in B do not remove A's.
        let mut archives = threads;
        for n in 0..25 {
            let mut entry = thread(&format!("b-{n}"), 0, "2026-11-16T10:00:00Z");
            entry.account = Some(b.clone());
            upsert_chat_archive(&mut archives, entry);
        }
        assert_eq!(archives.iter().filter(|chat| chat.account.as_ref() == Some(&a)).count(), 2);
        assert_eq!(archives.iter().filter(|chat| chat.account.as_ref() == Some(&b)).count(), THREADS_PER_ACCOUNT);
        // Saving a thread again never changes its account.
        let mut again = thread("across", 0, "2026-11-16T12:00:00Z");
        again.account = Some(b.clone());
        upsert_chat_archive(&mut archives, again);
        assert_eq!(archives.iter().find(|chat| chat.id == "across").unwrap().account.as_ref(), Some(&a));
        // A file holding more than the cap keeps its newest.
        let many: Vec<_> = (0..30).map(|n| thread(&format!("a-{n}"), 0, "2026-11-16T10:00:00Z")).collect();
        let kept = adopt_threads(many, &a, 1);
        assert_eq!(kept.len(), THREADS_PER_ACCOUNT);
        assert_eq!(kept[0].id, "a-10");
    }

}
