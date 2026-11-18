//! Saved chats on each account's server (`/api/v1/chats`).
//!
//! A conversation belongs to one account for life, and its server is the
//! only place it is kept: the same threads are on the iPhone and the web for
//! that person, and on nobody else's. In server mode this Mac holds the
//! newest of them in memory only (`Cereal::chat_archives`); no chat file is
//! read or written for an account.
//!
//! - **Asking.** A question in a thread goes to `POST /chats/:id/stream`
//!   with its exchange number. The server saves the question and the
//!   answer as it arrives, so nothing is lost if this Mac quits mid-answer
//!   and nothing is saved twice. The thread is created first (an empty
//!   thread, idempotently by its id); the answer says how many exchanges
//!   it holds, and a new question is asked as the next of them.
//! - **Signing in.** `sync_account_chats` reads the newest page of threads
//!   and the ones not yet held here.
//! - **The thread list.** `chat_threads`, `load_more_chat_threads`,
//!   `open_chat_thread`, `rename_chat_thread`, `delete_chat_thread` and
//!   `refresh_chat_threads` are what a list of threads (the sidebar's Chat
//!   section) needs: newest first, paged, with how many more there are.
use crate::accounts::{Account, AccountId};
use crate::chat::{ArchivedChat, ChatTurnAnnotations};
use gpui::*;
use meeting_notes_api_types::{
    ChatMessageDtoRole, ChatThreadDTO, ChatThreadMessageStatus, ChatThreadSummaryDTO,
    ChatThreadsPageDTO, DeletedDTO,
};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::mpsc;
use std::time::Duration;

/// What a thread is called before it has a name.
pub const UNTITLED: &str = "Untitled thread";
/// Threads read per page of the list.
const PAGE: usize = 20;
/// Threads whose messages are held in memory, per account.
const CACHED: usize = 20;

// ------------------------------------------------------------ The interface

/// One row of the thread list.
#[derive(Clone, Debug, PartialEq)]
pub struct ThreadEntry {
    pub id: String,
    pub account: AccountId,
    /// The note the thread is about as a handle in memory, 0 for a thread
    /// across all notes, `None` for a note that is not on this Mac (it
    /// cannot be opened here).
    pub note_id: Option<u64>,
    /// `None` until the server has named it: show [`UNTITLED`].
    pub title: Option<String>,
    /// RFC 3339: when it was last asked or answered.
    pub updated_at: String,
    pub turn_count: usize,
}

impl ThreadEntry {
    pub fn display_title(&self) -> &str {
        self.title.as_deref().filter(|title| !title.trim().is_empty()).unwrap_or(UNTITLED)
    }
}

/// An account's threads as far as they have been read.
#[derive(Clone, Debug, Default)]
pub struct AccountThreads {
    /// Newest first, as the server ordered them, pages appended.
    pub threads: Vec<ThreadEntry>,
    /// Every thread the account has on its server.
    pub total: usize,
    next_cursor: Option<String>,
    pub loading: bool,
    /// The last failure, for the list to show; cleared by the next success.
    pub error: Option<String>,
}

impl AccountThreads {
    /// How many threads are on the server beyond the ones read: "Show N more".
    pub fn more(&self) -> usize {
        self.total.saturating_sub(self.threads.len())
    }
}

/// Chat sync state of the window: one entry per account.
#[derive(Default)]
pub struct ChatSync {
    accounts: HashMap<AccountId, AccountThreads>,
    /// Bumped when an account goes: answers for it that arrive later are dropped.
    epochs: HashMap<AccountId, u64>,
}

impl ChatSync {
    pub fn of(&self, account: &AccountId) -> Option<&AccountThreads> {
        self.accounts.get(account)
    }
    fn entry(&mut self, account: &AccountId) -> &mut AccountThreads {
        self.accounts.entry(account.clone()).or_default()
    }
    fn epoch(&self, account: &AccountId) -> u64 {
        self.epochs.get(account).copied().unwrap_or(0)
    }
    /// An account signed out or was removed: forget its list.
    pub fn forget(&mut self, account: &AccountId) {
        self.accounts.remove(account);
        *self.epochs.entry(account.clone()).or_default() += 1;
    }
}

/// The exchange a question is asked as, for `server::stream_chat_with_model`.
#[derive(Clone, Debug)]
pub struct ThreadTurn {
    pub thread_id: String,
    /// 0-based exchange number: the question's index in the conversation.
    /// Only an edit names it; a new question is the thread's next exchange.
    pub turn: usize,
    /// An edited or retried question: replaces exchange `turn` and the ones after it.
    pub replace: bool,
    /// The note's id on the server, `None` for a thread across all notes.
    pub note: Option<String>,
}

// ------------------------------------------------------------ Conversions

/// A thread from the server as this Mac keeps it.
pub fn archive_of(thread: &ChatThreadDTO, account: &AccountId, note_id: u64) -> ArchivedChat {
    let mut messages = Vec::new();
    let mut interrupted = Vec::new();
    let mut annotations = HashMap::new();
    let turns = thread.turn_count.max(0) as usize;
    for turn in 0..turns {
        let of = |role: ChatMessageDtoRole| {
            thread
                .messages
                .iter()
                .find(|m| m.turn == turn as i64 && m.role == role)
        };
        let question = of(ChatMessageDtoRole::User).map(|m| m.content.clone()).unwrap_or_default();
        let Some(answer) = of(ChatMessageDtoRole::Assistant) else {
            messages.push((question, "Response stopped.".into()));
            interrupted.push(turn);
            continue;
        };
        let mut notes = ChatTurnAnnotations::default();
        notes.citations = answer.citations.clone();
        notes.tools = answer.tools.clone();
        notes.reasoning = answer.reasoning.clone();
        notes.elapsed_ms = answer.elapsed_ms.and_then(|ms| u64::try_from(ms).ok());
        let text = match answer.status {
            ChatThreadMessageStatus::Complete => answer.content.clone(),
            ChatThreadMessageStatus::Failed if answer.content.is_empty() => format!(
                "Could not complete the response. {}",
                answer.error.clone().unwrap_or_else(|| "Please retry.".into())
            ),
            ChatThreadMessageStatus::Failed => {
                notes.error = answer.error.clone();
                answer.content.clone()
            }
            // Still arriving on another device, or cut off: what is there.
            ChatThreadMessageStatus::Interrupted | ChatThreadMessageStatus::Streaming => {
                if answer.content.is_empty() {
                    "Response stopped.".into()
                } else {
                    answer.content.clone()
                }
            }
        };
        if answer.status != ChatThreadMessageStatus::Complete {
            interrupted.push(turn);
        }
        if !notes.citations.is_empty() || !notes.tools.is_empty() || !notes.reasoning.is_empty() || notes.elapsed_ms.is_some() || notes.error.is_some() {
            annotations.insert(turn, notes);
        }
        messages.push((question, text));
    }
    ArchivedChat {
        id: thread.id.clone(),
        note_id,
        account: Some(account.clone()),
        title: thread.title.clone().unwrap_or_else(|| UNTITLED.into()),
        messages,
        interrupted,
        annotations,
        updated_at: Some(thread.updated_at.clone()),
    }
}

/// The note here that a server note id names, in the account's slot.
fn local_note(notes: &[crate::model::Note], server_note: Option<&str>, account: &Account) -> Option<u64> {
    let Some(server_note) = server_note else {
        return Some(crate::accounts::handle::NO_NOTE);
    };
    notes
        .iter()
        .find(|note| {
            !note.trashed
                && note.server_id.as_deref() == Some(server_note)
                && note.server_url.as_deref() == Some(account.url.as_str())
                && crate::accounts::handle::slot_of(note.id) == Some(account.slot)
        })
        .map(|note| note.id)
}

/// The server id of the note a thread is about, `Some(None)` for a thread
/// across notes, `None` when the note is not on the server (yet).
fn server_note(notes: &[crate::model::Note], note_id: u64, account: &Account) -> Option<Option<String>> {
    if note_id == crate::accounts::handle::NO_NOTE {
        return Some(None);
    }
    let note = notes.iter().find(|note| note.id == note_id)?;
    (note.server_url.as_deref() == Some(account.url.as_str()))
        .then(|| note.server_id.clone())
        .flatten()
        .map(Some)
}

fn entry_of(summary: &ChatThreadSummaryDTO, account: &Account, notes: &[crate::model::Note]) -> ThreadEntry {
    ThreadEntry {
        id: summary.id.clone(),
        account: account.id.clone(),
        note_id: local_note(notes, summary.note_id.as_deref(), account),
        title: summary.title.clone(),
        updated_at: summary.updated_at.clone(),
        turn_count: summary.turn_count.max(0) as usize,
    }
}

// ------------------------------------------------------------ Requests (blocking, off the UI thread)

fn path_id(id: &str) -> Result<String, String> {
    crate::accounts::client::path_id(id).map_err(|error| error.message())
}

/// What one sync read from the server.
pub(crate) struct Pulled {
    page: Option<ChatThreadsPageDTO>,
    /// Full threads whose cached copy is missing or out of date.
    threads: Vec<ChatThreadDTO>,
    error: Option<String>,
}

/// Two RFC 3339 times that name the same instant, however they are written.
fn same_time(a: &str, b: &str) -> bool {
    match (chrono::DateTime::parse_from_rfc3339(a), chrono::DateTime::parse_from_rfc3339(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// Read the newest page, and fetch the threads among it that are not held
/// here as they are now.
pub(crate) fn pull(client: &crate::accounts::AccountClient, cached: HashMap<String, Option<String>>) -> Pulled {
    let mut pulled = Pulled { page: None, threads: Vec::new(), error: None };
    match client.send::<ChatThreadsPageDTO>("GET", &format!("/api/v1/chats?limit={PAGE}"), None, None) {
        Ok(page) => {
            for summary in page.threads.iter().take(CACHED) {
                if cached.get(&summary.id).is_some_and(|at| at.as_deref().is_some_and(|at| same_time(at, &summary.updated_at))) {
                    continue;
                }
                let Ok(id) = path_id(&summary.id) else { continue };
                match client.send::<ChatThreadDTO>("GET", &format!("/api/v1/chats/{id}"), None, None) {
                    Ok(thread) => pulled.threads.push(thread),
                    Err(error) if error.is_not_found() => {}
                    // One thread that cannot be read (too large, a fault
                    // on the server) must not hide the ones after it: it is
                    // skipped and the rest of the page is still fetched.
                    Err(error) => {
                        pulled.error.get_or_insert(error.message());
                    }
                }
            }
            pulled.page = Some(page);
        }
        Err(error) => pulled.error = Some(error.message()),
    }
    pulled
}

/// Before a question is streamed: make sure the thread is on the server and
/// learn how many exchanges it holds. Returns the stream request.
pub(crate) fn stream_request(
    client: &crate::accounts::AccountClient,
    turn: &ThreadTurn,
    server_note: Option<&str>,
    message: &str,
    model: Option<&str>,
) -> Result<(String, Value), String> {
    // The note's id on the server as it was just made sure of: a new note may not have had one.
    let note = server_note.map(str::to_owned).or_else(|| turn.note.clone());
    let thread: ChatThreadDTO = client
        .send("POST", "/api/v1/chats", Some(json!({"id": turn.thread_id, "noteId": note})), None)
        .map_err(|error| match error.status() {
            Some(409) => "This conversation cannot be saved on your server. Start a new chat.".to_owned(),
            Some(404) => "This meeting is no longer on your server.".to_owned(),
            _ => error.message(),
        })?;
    let stored = thread.turn_count.max(0) as usize;
    // A new question is the next exchange the server has; an edit replaces the one it names.
    let (at, replace) = if turn.replace && turn.turn < stored { (turn.turn, true) } else { (stored, false) };
    let id = path_id(&thread.id)?;
    let mut payload = json!({"turn": at, "message": message, "includeSources": true});
    if replace {
        payload["replace"] = json!(true);
    }
    if let Some(model) = model {
        payload["model"] = json!(model);
    }
    if let Ok(zone) = iana_time_zone::get_timezone() {
        payload["timeZone"] = json!(zone);
    }
    Ok((format!("/api/v1/chats/{id}/stream"), payload))
}

async fn answer<T: Send + 'static>(receiver: mpsc::Receiver<T>, cx: &mut AsyncApp) -> Option<T> {
    loop {
        match receiver.try_recv() {
            Ok(value) => return Some(value),
            Err(mpsc::TryRecvError::Disconnected) => return None,
            Err(mpsc::TryRecvError::Empty) => cx.background_executor().timer(Duration::from_millis(50)).await,
        }
    }
}

/// Merge what the server holds into the cache of one account, newest kept.
/// Threads with an answer arriving here, or on screen while one arrives,
/// are left as they are. Returns whether anything changed.
pub(crate) fn merge_cache(
    archives: &mut Vec<ArchivedChat>,
    account: &AccountId,
    fresh: Vec<ArchivedChat>,
    gone: &HashSet<String>,
    busy: &HashSet<String>,
) -> bool {
    let mut changed = false;
    let before = archives.len();
    archives.retain(|chat| chat.account.as_ref() != Some(account) || !gone.contains(&chat.id) || busy.contains(&chat.id));
    changed |= archives.len() != before;
    for thread in fresh {
        if busy.contains(&thread.id) {
            continue;
        }
        archives.retain(|chat| chat.id != thread.id);
        archives.push(thread);
        changed = true;
    }
    archives.sort_by(|a, b| a.updated_at.cmp(&b.updated_at));
    // Only the newest are kept for reading offline; the rest are a page away.
    while archives.iter().filter(|chat| chat.account.as_ref() == Some(account)).count() > CACHED {
        let Some(oldest) = archives
            .iter()
            .position(|chat| chat.account.as_ref() == Some(account) && !busy.contains(&chat.id))
        else {
            break;
        };
        archives.remove(oldest);
    }
    changed
}

impl crate::app::Cereal {
    /// Conversations with an answer arriving: the cache must not replace them.
    fn chats_in_flight(&self) -> HashSet<String> {
        let mut busy: HashSet<String> = self.chat_parked.keys().cloned().collect();
        if self.chat_stream.is_some() || self.chat_editing.is_some() {
            busy.insert(self.chat_thread_id.clone());
        }
        busy
    }

    /// The exchange a question in `thread` is asked as; `None` on this Mac
    /// only (no server), or for a thread whose account is gone. `index` is the
    /// question's place in the conversation, `replace` whether it replaces
    /// a question that was there (an edit or a retry).
    pub(crate) fn chat_thread_turn(&self, thread: &str, index: usize, replace: bool) -> Option<ThreadTurn> {
        if !crate::server::enabled() {
            return None;
        }
        let on_screen = self.chat_thread_id == thread;
        let archive = self.chat_archives.iter().find(|chat| chat.id == thread);
        let account = match (on_screen, archive.and_then(|chat| chat.account.clone())) {
            (_, Some(account)) => crate::accounts::get(&account)?,
            (true, None) => match self.current_note {
                Some(note) => crate::accounts::of_note(note)?,
                None => crate::accounts::default_account()?,
            },
            (false, None) => return None,
        };
        let note_id = match archive {
            Some(chat) => chat.note_id,
            None => self.current_note.unwrap_or(crate::accounts::handle::NO_NOTE),
        };
        // A note not on the server yet gets its id when the question is sent (`server.rs`).
        let note = server_note(&self.workspace.notes, note_id, &account).flatten();
        Some(ThreadTurn { thread_id: thread.to_owned(), turn: index, replace, note })
    }

    /// The thread list across every account, newest first: what has been
    /// read from the servers, with the conversations asked since that the
    /// list has not been read again for.
    pub fn chat_threads(&self) -> Vec<ThreadEntry> {
        let mut rows: Vec<ThreadEntry> = Vec::new();
        let mut listed = HashSet::new();
        for threads in self.chat_sync.accounts.values() {
            for entry in &threads.threads {
                listed.insert(entry.id.clone());
                // This Mac's copy may be newer (a question since the list was read).
                let local = self.chat_archives.iter().find(|chat| chat.id == entry.id);
                let mut entry = entry.clone();
                if let Some(at) = local.and_then(|chat| chat.updated_at.clone())
                    && at > entry.updated_at
                {
                    entry.updated_at = at;
                    entry.turn_count = entry.turn_count.max(local.map_or(0, |chat| chat.messages.len()));
                }
                // A thread nothing was ever asked in (its first question
                // never reached the server) is not worth a row.
                if entry.turn_count == 0 {
                    continue;
                }
                rows.push(entry);
            }
        }
        for chat in &self.chat_archives {
            if listed.contains(&chat.id) || chat.messages.is_empty() {
                continue;
            }
            let Some(account) = chat.account.clone() else { continue };
            rows.push(ThreadEntry {
                id: chat.id.clone(),
                account,
                note_id: Some(chat.note_id),
                title: Some(chat.title.clone()).filter(|title| !title.is_empty() && title != UNTITLED),
                updated_at: chat.updated_at.clone().unwrap_or_default(),
                turn_count: chat.messages.len(),
            });
        }
        rows.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then_with(|| b.id.cmp(&a.id)));
        rows
    }

    /// How many threads the servers hold beyond `chat_threads`.
    pub fn chat_threads_more(&self) -> usize {
        self.chat_sync.accounts.values().map(AccountThreads::more).sum()
    }

    /// An account's session is verified and its notes loaded: read its
    /// newest threads from its server.
    pub fn sync_account_chats(&mut self, account: &Account, cx: &mut Context<Self>) {
        if crate::audio::preview_mode() || !crate::server::enabled() {
            return;
        }
        let cached: HashMap<String, Option<String>> = self
            .chat_archives
            .iter()
            .filter(|chat| chat.account.as_ref() == Some(&account.id))
            .map(|chat| (chat.id.clone(), chat.updated_at.clone()))
            .collect();
        let Ok(client) = crate::accounts::client(&account.id) else { return };
        let (id, epoch) = (account.id.clone(), self.chat_sync.epoch(&account.id));
        self.chat_sync.entry(&id).loading = true;
        let (send, receive) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = send.send(pull(&client, cached));
        });
        cx.spawn(async move |this, cx| {
            let Some(pulled) = answer(receive, cx).await else { return };
            let _ = this.update(cx, |this, cx| {
                if this.chat_sync.epoch(&id) != epoch {
                    return;
                }
                this.take_pulled(&id, pulled);
                cx.notify();
            });
        })
        .detach();
    }

    fn take_pulled(&mut self, id: &AccountId, pulled: Pulled) {
        let Some(account) = crate::accounts::get(id) else { return };
        let threads = self.chat_sync.entry(id);
        threads.loading = false;
        threads.error = pulled.error.clone();
        let Some(page) = &pulled.page else { return };
        let busy = self.chats_in_flight();
        // Held here, not listed, and new enough that it would be: deleted elsewhere.
        let listed: HashSet<&str> = page.threads.iter().map(|t| t.id.as_str()).collect();
        let oldest = page.threads.last().map(|t| t.updated_at.clone());
        let gone: HashSet<String> = self
            .chat_archives
            .iter()
            .filter(|chat| chat.account.as_ref() == Some(id) && !listed.contains(chat.id.as_str()))
            .filter(|chat| {
                page.next_cursor.is_none()
                    || oldest.as_ref().is_some_and(|oldest| chat.updated_at.as_ref().is_some_and(|at| at >= oldest))
            })
            .map(|chat| chat.id.clone())
            .collect();
        let threads = self.chat_sync.entry(id);
        threads.threads = page.threads.iter().map(|s| entry_of(s, &account, &self.workspace.notes)).collect();
        threads.total = page.total.max(0) as usize;
        threads.next_cursor = page.next_cursor.clone();
        let fresh: Vec<ArchivedChat> = pulled
            .threads
            .iter()
            .filter_map(|thread| {
                let note = local_note(&self.workspace.notes, thread.note_id.as_deref(), &account)?;
                Some(archive_of(thread, id, note))
            })
            .collect();
        // The conversation on screen follows the server when nothing is happening in it.
        if !busy.contains(&self.chat_thread_id) {
            if let Some(shown) = fresh.iter().find(|chat| chat.id == self.chat_thread_id) {
                self.chat_messages = shown.messages.clone();
                self.chat_interrupted = shown.interrupted.iter().copied().collect();
                self.chat_annotations = shown.annotations.clone();
            } else if gone.contains(&self.chat_thread_id) {
                self.chat_messages.clear();
                self.chat_interrupted.clear();
                self.chat_annotations.clear();
                self.chat_thread_id = uuid::Uuid::new_v4().to_string();
            }
        }
        merge_cache(&mut self.chat_archives, id, fresh, &gone, &busy);
    }

    /// Read the newest threads of every account whose server keeps chats
    /// again: after an answer has been saved, or when the list is shown.
    pub fn refresh_chat_threads(&mut self, cx: &mut Context<Self>) {
        for account in crate::accounts::all() {
            if account.user_id.is_some() && self.workspace.slots.contains_key(&account.slot) {
                self.sync_account_chats(&account, cx);
            }
        }
    }

    /// An answer in `thread` has just been saved on the server: its title
    /// may follow a moment later.
    pub(crate) fn chat_turn_saved(&mut self, thread: &str, cx: &mut Context<Self>) {
        let Some(account) = self
            .chat_archives
            .iter()
            .find(|chat| chat.id == thread)
            .and_then(|chat| chat.account.clone())
            .and_then(|id| crate::accounts::get(&id))
        else {
            return;
        };
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(3)).await;
            let _ = this.update(cx, |this, cx| this.sync_account_chats(&account, cx));
        })
        .detach();
    }

    /// The next page of an account's threads ("Show N more").
    pub fn load_more_chat_threads(&mut self, account: &AccountId, cx: &mut Context<Self>) {
        let Some(found) = crate::accounts::get(account) else { return };
        let Some(threads) = self.chat_sync.accounts.get_mut(account) else { return };
        let Some(cursor) = threads.next_cursor.clone().filter(|_| !threads.loading) else { return };
        threads.loading = true;
        let Ok(client) = crate::accounts::client(account) else { return };
        let (id, epoch) = (account.clone(), self.chat_sync.epoch(account));
        let (send, receive) = mpsc::channel();
        std::thread::spawn(move || {
            let page = client.send::<ChatThreadsPageDTO>("GET", &format!("/api/v1/chats?limit={PAGE}&cursor={cursor}"), None, None);
            let _ = send.send(page.map_err(|error| error.message()));
        });
        cx.spawn(async move |this, cx| {
            let Some(page) = answer(receive, cx).await else { return };
            let _ = this.update(cx, |this, cx| {
                if this.chat_sync.epoch(&id) != epoch {
                    return;
                }
                let notes = &this.workspace.notes;
                let threads = this.chat_sync.accounts.entry(id.clone()).or_default();
                threads.loading = false;
                match page {
                    Ok(page) => {
                        for summary in &page.threads {
                            if !threads.threads.iter().any(|t| t.id == summary.id) {
                                threads.threads.push(entry_of(summary, &found, notes));
                            }
                        }
                        threads.total = page.total.max(0) as usize;
                        threads.next_cursor = page.next_cursor;
                        threads.error = None;
                    }
                    Err(error) => threads.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Open a thread of the list. One kept on this Mac opens at once; any
    /// other is read from its server first.
    pub fn open_chat_thread(&mut self, account: &AccountId, id: &str, expanded: bool, cx: &mut Context<Self>) {
        if self.chat_archives.iter().any(|chat| chat.id == id) {
            self.restore_archived_chat(id, expanded, cx);
            return;
        }
        let (Some(found), Ok(client), Ok(path)) = (crate::accounts::get(account), crate::accounts::client(account), path_id(id)) else {
            self.show_toast("This chat is not available", cx);
            return;
        };
        let (send, receive) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = send.send(client.send::<ChatThreadDTO>("GET", &format!("/api/v1/chats/{path}"), None, None));
        });
        let (account, epoch) = (account.clone(), self.chat_sync.epoch(account));
        cx.spawn(async move |this, cx| {
            let Some(result) = answer(receive, cx).await else { return };
            let _ = this.update(cx, |this, cx| {
                if this.chat_sync.epoch(&account) != epoch {
                    return;
                }
                let thread = match result {
                    Ok(thread) => thread,
                    Err(error) => {
                        let message = if error.is_not_found() { "This chat was deleted".to_owned() } else { error.message() };
                        this.show_toast(&message, cx);
                        return;
                    }
                };
                // A chat about a note that is not here (in Trash, or never
                // loaded on this Mac) still opens: by itself, as a full chat.
                // Its server goes on answering it about its own note.
                let note = local_note(&this.workspace.notes, thread.note_id.as_deref(), &found)
                    .unwrap_or(crate::accounts::handle::NO_NOTE);
                let id = thread.id.clone();
                let busy = this.chats_in_flight();
                let mut archives = std::mem::take(&mut this.chat_archives);
                merge_cache(&mut archives, &account, vec![archive_of(&thread, &account, note)], &HashSet::new(), &busy);
                // An older thread than the cache keeps is still opened: put it back if it was trimmed.
                if !archives.iter().any(|chat| chat.id == id) {
                    archives.insert(0, archive_of(&thread, &account, note));
                }
                this.chat_archives = archives;
                this.restore_archived_chat(&id, expanded, cx);
            });
        })
        .detach();
    }

    /// Rename a thread, on its server and here.
    pub fn rename_chat_thread(&mut self, account: &AccountId, id: &str, title: String, cx: &mut Context<Self>) {
        let title: String = title.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(200).collect();
        if title.is_empty() {
            return;
        }
        let (Ok(client), Ok(path)) = (crate::accounts::client(account), path_id(id)) else { return };
        let (send, receive) = mpsc::channel();
        let body = json!({"title": title});
        std::thread::spawn(move || {
            let _ = send.send(client.send::<ChatThreadDTO>("PATCH", &format!("/api/v1/chats/{path}"), Some(body), None));
        });
        let (account, id) = (account.clone(), id.to_owned());
        cx.spawn(async move |this, cx| {
            let Some(result) = answer(receive, cx).await else { return };
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(thread) => {
                        let name = thread.title.clone();
                        if let Some(threads) = this.chat_sync.accounts.get_mut(&account)
                            && let Some(entry) = threads.threads.iter_mut().find(|t| t.id == id)
                        {
                            entry.title = name.clone();
                        }
                        if let Some(chat) = this.chat_archives.iter_mut().find(|chat| chat.id == id) {
                            chat.title = name.unwrap_or_else(|| UNTITLED.into());
                        }
                    }
                    Err(error) => this.show_toast(&error.message(), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Delete a thread on its server and from this Mac. An answer arriving
    /// in it is stopped first.
    pub fn delete_chat_thread(&mut self, account: &AccountId, id: &str, cx: &mut Context<Self>) {
        let (Ok(client), Ok(path)) = (crate::accounts::client(account), path_id(id)) else { return };
        let (send, receive) = mpsc::channel();
        std::thread::spawn(move || {
            let result = client.send::<DeletedDTO>("DELETE", &format!("/api/v1/chats/{path}"), None, None);
            // Already gone is as good as deleted.
            let _ = send.send(result.map(|_| ()).or_else(|error| if error.is_not_found() { Ok(()) } else { Err(error.message()) }));
        });
        let (account, id) = (account.clone(), id.to_owned());
        cx.spawn(async move |this, cx| {
            let Some(result) = answer(receive, cx).await else { return };
            let _ = this.update(cx, |this, cx| {
                if let Err(error) = result {
                    this.show_toast(&error, cx);
                    return;
                }
                if let Some(threads) = this.chat_sync.accounts.get_mut(&account)
                    && let Some(at) = threads.threads.iter().position(|t| t.id == id)
                {
                    threads.threads.remove(at);
                    threads.total = threads.total.saturating_sub(1);
                }
                if this.chat_thread_id == id {
                    this.clear_chat(cx);
                }
                // Removing it from the cache also ends an answer still arriving in it (`Chats::event`).
                this.chat_archives.retain(|chat| chat.id != id);
                cx.notify();
            });
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::client::testing::{Fake, json as reply};

    fn account() -> AccountId {
        AccountId::of("http://127.0.0.1:1", "50000000-0000-4000-8000-000000000001")
    }

    fn citation() -> Value {
        json!({"id":"S1","noteId":"51000000-0000-4000-8000-000000000001","title":"Standup","source":"transcript","segmentId":null,"startMs":1200,"quote":"We ship Monday","transcriptRevision":3})
    }

    /// A message row as the server answers it.
    fn row(turn: usize, role: &str, content: &str, status: &str, extra: Value) -> Value {
        let mut row = json!({"id": uuid::Uuid::new_v4().to_string(), "turn": turn, "role": role, "content": content, "status": status});
        for key in ["error", "model", "elapsedMs"] {
            row[key] = extra.get(key).cloned().unwrap_or(Value::Null);
        }
        for key in ["citations", "tools", "reasoning"] {
            row[key] = extra.get(key).cloned().unwrap_or(json!([]));
        }
        row["createdAt"] = json!("2026-10-02T10:00:00.000Z");
        row["updatedAt"] = json!("2026-10-02T10:00:00.000Z");
        row
    }

    fn thread_json(id: &str, turns: i64, messages: Vec<Value>) -> Value {
        let mut thread = json!({"id": id, "noteId": null, "title": null, "titleSource": null, "turnCount": turns});
        thread["createdAt"] = json!("2026-10-02T10:00:00.000Z");
        thread["updatedAt"] = json!("2026-10-02T10:00:00.000Z");
        thread["messages"] = json!(messages);
        thread
    }

    #[::core::prelude::v1::test]
    fn a_thread_from_the_server_reads_as_the_conversation_with_every_kind_of_answer() {
        let v = |text: &str| -> Value { serde_json::from_str(text).unwrap() };
        let first = format!(
            r#"{{"citations":[{}],"tools":[{{"callId":"52000000-0000-4000-8000-000000000001","name":"search_meetings","status":"completed","label":"Searched","resultCount":1}}],"reasoning":[{{"afterTools":0,"text":"**Finding the release**\n\nSearch first."}},{{"afterTools":1,"text":"Enough to answer."}}],"elapsedMs":1500}}"#,
            citation()
        );
        let script: [(&str, &str, &str, &str); 12] = [
            ("user", "When do we ship?", "complete", "{}"),
            ("assistant", "Monday [[S1]].", "complete", first.as_str()),
            ("user", "Who decided?", "complete", "{}"),
            ("assistant", "Priya, so", "interrupted", "{}"),
            ("user", "And the budget?", "complete", "{}"),
            ("assistant", "", "failed", r#"{"error":"Server unavailable"}"#),
            ("user", "Stop this", "complete", "{}"),
            ("assistant", "", "interrupted", "{}"),
            ("user", "Sources?", "complete", "{}"),
            ("assistant", "Partly", "failed", r#"{"error":"A source changed while answering."}"#),
            ("user", "Still going on the iPhone", "complete", "{}"),
            ("assistant", "So far", "streaming", "{}"),
        ];
        let messages: Vec<Value> = script.iter().enumerate().map(|(i, (role, content, status, extra))| row(i / 2, role, content, status, v(extra))).collect();
        let thread: ChatThreadDTO = serde_json::from_value(thread_json("53000000-0000-4000-8000-000000000001", 6, messages)).unwrap();
        let chat = archive_of(&thread, &account(), 0);
        let texts: Vec<_> = chat.messages.iter().map(|(q, a)| (q.as_str(), a.as_str())).collect();
        assert_eq!(texts, vec![
            ("When do we ship?", "Monday [[S1]]."),
            ("Who decided?", "Priya, so"),
            ("And the budget?", "Could not complete the response. Server unavailable"),
            ("Stop this", "Response stopped."),
            ("Sources?", "Partly"),
            ("Still going on the iPhone", "So far"),
        ]);
        assert_eq!(chat.interrupted, vec![1, 2, 3, 4, 5]);
        assert_eq!(chat.annotations[&0].citations[0].id, "S1");
        assert_eq!(chat.annotations[&0].tools.len(), 1);
        assert_eq!(chat.annotations[&0].elapsed_ms, Some(1500));
        // The thoughts read back in the order they happened.
        assert_eq!(
            chat.annotations[&0].thoughts(),
            [
                crate::chat::Thought::Reasoning { heading: Some("Finding the release"), body: "Search first." },
                crate::chat::Thought::Step(0),
                crate::chat::Thought::Reasoning { heading: None, body: "Enough to answer." },
            ]
        );
        assert_eq!(chat.annotations[&4].error.as_deref(), Some("A source changed while answering."));
        assert_eq!(chat.title, UNTITLED);
        assert_eq!(chat.account, Some(account()));
        assert_eq!(chat.updated_at.as_deref(), Some("2026-10-02T10:00:00.000Z"));
    }

    #[::core::prelude::v1::test]
    fn the_cache_follows_the_server_but_never_under_an_answer_that_is_arriving() {
        let mine = account();
        let other = AccountId::of("http://127.0.0.1:2", "50000000-0000-4000-8000-000000000002");
        let chat = |id: &str, account: &AccountId, at: &str| ArchivedChat { id: id.into(), note_id: 0, account: Some(account.clone()), title: id.into(),
            messages: vec![("q".into(), "a".into())], interrupted: vec![], annotations: HashMap::new(), updated_at: Some(at.into()) };
        let mut archives = vec![chat("deleted", &mine, "2026-01-01"), chat("streaming", &mine, "2026-01-02"), chat("theirs", &other, "2026-01-03"), chat("stale", &mine, "2026-01-04")];
        let mut newer = chat("stale", &mine, "2026-02-01");
        newer.messages.push(("q2".into(), "a2 from the iPhone".into()));
        let gone = HashSet::from(["deleted".to_owned(), "streaming".to_owned(), "theirs".to_owned()]);
        let busy = HashSet::from(["streaming".to_owned()]);
        assert!(merge_cache(&mut archives, &mine, vec![newer, chat("streaming", &mine, "2026-03-01")], &gone, &busy));
        let ids: Vec<_> = archives.iter().map(|c| c.id.as_str()).collect();
        // Another account's thread is not this account's to delete; one with an answer arriving is left alone.
        assert_eq!(ids, vec!["streaming", "theirs", "stale"]);
        assert_eq!(archives[0].updated_at.as_deref(), Some("2026-01-02"));
        assert_eq!(archives[2].messages.len(), 2);
        // Only the newest are held per account.
        let many: Vec<_> = (0..30).map(|i| chat(&format!("t{i:02}"), &mine, &format!("2026-05-{:02}", i + 1))).collect();
        merge_cache(&mut archives, &mine, many, &HashSet::new(), &HashSet::new());
        assert_eq!(archives.iter().filter(|c| c.account.as_ref() == Some(&mine)).count(), CACHED);
        assert!(archives.iter().any(|c| c.id == "theirs"));
        assert!(archives.iter().any(|c| c.id == "t29") && !archives.iter().any(|c| c.id == "t00"));
    }

    #[::core::prelude::v1::test]
    fn a_question_creates_its_empty_thread_first_and_is_asked_as_its_next_exchange() {
        let id = "55000000-0000-4000-8000-000000000001";
        let stored = std::sync::Arc::new(std::sync::atomic::AtomicI64::new(2));
        let count = stored.clone();
        let fake = Fake::start(move |_| reply(200, thread_json(id, count.load(std::sync::atomic::Ordering::Acquire), vec![]).to_string()));
        let client = crate::accounts::AccountClient::for_test(fake.url.clone(), "token", None);
        let turn = |at: usize, replace: bool| ThreadTurn { thread_id: id.into(), turn: at, replace, note: Some("54000000-0000-4000-8000-000000000001".into()) };
        let (path, payload) = stream_request(&client, &turn(2, false), None, "Next?", Some("m")).unwrap();
        assert_eq!(path, format!("/api/v1/chats/{id}/stream"));
        assert_eq!((payload["turn"].clone(), payload["message"].clone(), payload["model"].clone()), (json!(2), json!("Next?"), json!("m")));
        assert!(payload.get("replace").is_none());
        let create: Value = serde_json::from_str(&fake.requests()[0].body).unwrap();
        assert_eq!(create, json!({"id": id, "noteId": "54000000-0000-4000-8000-000000000001"}));
        // The iPhone asked meanwhile: the question is the exchange after that one.
        stored.store(3, std::sync::atomic::Ordering::Release);
        assert_eq!(stream_request(&client, &turn(2, false), None, "Next?", None).unwrap().1["turn"], json!(3));
        // An edit of exchange 1 replaces it; an edit of one the server never had is a new question.
        let (_, edit) = stream_request(&client, &turn(1, true), None, "Edited", None).unwrap();
        assert_eq!((edit["turn"].clone(), edit["replace"].clone()), (json!(1), json!(true)));
        let (_, beyond) = stream_request(&client, &turn(7, true), None, "Edited", None).unwrap();
        assert_eq!((beyond["turn"].clone(), beyond.get("replace")), (json!(3), None));
        // The note's id as the stream just made sure of wins over the one remembered.
        stream_request(&client, &turn(3, false), Some("56000000-0000-4000-8000-000000000001"), "Q", None).unwrap();
        let last: Value = serde_json::from_str(&fake.requests().last().unwrap().body).unwrap();
        assert_eq!(last["noteId"], "56000000-0000-4000-8000-000000000001");
    }

    #[::core::prelude::v1::test]
    fn signing_in_reads_the_newest_threads_and_only_fetches_what_changed() {
        let id = "56000000-0000-4000-8000-000000000001";
        let fake = Fake::start(move |seen| match (seen.method.as_str(), seen.path.as_str()) {
            ("GET", path) if path.starts_with("/api/v1/chats?") => {
                let mut summary = json!({"id": id, "noteId": null, "title": "Shipping date", "titleSource": "generated", "turnCount": 0});
                summary["createdAt"] = json!("2026-10-02T10:00:00.000Z");
                summary["updatedAt"] = json!("2026-10-03T10:00:00.000Z");
                reply(200, json!({"threads": [summary], "nextCursor": null, "total": 1}).to_string())
            }
            ("GET", path) if path == format!("/api/v1/chats/{id}") => reply(200, thread_json(id, 0, vec![]).to_string()),
            _ => reply(404, "{}"),
        });
        let client = crate::accounts::AccountClient::for_test(fake.url.clone(), "token", None);
        let pulled = pull(&client, HashMap::new());
        assert!(pulled.error.is_none());
        assert_eq!(pulled.page.as_ref().unwrap().total, 1);
        assert_eq!(pulled.threads.len(), 1);
        // Held here as the server has it (written another way, the same instant): not fetched again.
        let cached = HashMap::from([(id.to_owned(), Some("2026-10-03T10:00:00+00:00".to_owned()))]);
        assert!(pull(&client, cached).threads.is_empty());
        assert_eq!(fake.paths().iter().filter(|p| p.starts_with("GET /api/v1/chats/")).count(), 1);
        assert!(fake.requests().iter().all(|seen| seen.method == "GET"));
    }

    /// Against a running server, as one of its people: `CHATS_E2E_URL`, and
    /// `CHATS_E2E_TOKEN_FILE` holding a session token. Run with `--ignored`.
    #[::core::prelude::v1::test]
    #[ignore]
    fn e2e_a_question_from_the_mac_is_saved_on_the_server_once() {
        use std::io::BufRead;
        let url = std::env::var("CHATS_E2E_URL").expect("CHATS_E2E_URL");
        let token = std::fs::read_to_string(std::env::var("CHATS_E2E_TOKEN_FILE").expect("CHATS_E2E_TOKEN_FILE")).unwrap();
        let client = crate::accounts::AccountClient::for_test(url, token.trim(), None);
        let id = uuid::Uuid::new_v4().to_string();
        let ask = |question: &str| {
            let turn = ThreadTurn { thread_id: id.clone(), turn: 0, replace: false, note: None };
            let (path, payload) = stream_request(&client, &turn, None, question, None).unwrap();
            let mut child = client.chat_stream(&path, &payload).unwrap();
            let done = std::io::BufReader::new(child.stdout.take().unwrap()).lines().any(|line| line.unwrap().contains(r#""type":"done""#));
            let _ = child.wait();
            assert!(done);
        };
        ask("When do we ship, asked from the Mac?");
        ask("And who decided, asked from the Mac?");
        let thread: ChatThreadDTO = client.send("GET", &format!("/api/v1/chats/{id}"), None, None).unwrap();
        assert_eq!(thread.turn_count, 2);
        assert_eq!(thread.messages.len(), 4);
        let last = thread.messages.last().unwrap();
        assert_eq!(last.status, ChatThreadMessageStatus::Complete);
        assert!(thread.messages[1].content.contains("history 0"), "{}", thread.messages[1].content);
        assert!(last.content.contains("history 1"), "{}", last.content);
        let pulled = pull(&client, HashMap::new());
        assert_eq!(pulled.page.unwrap().threads.iter().filter(|t| t.id == id).count(), 1);
        println!("e2e thread {id}: {} exchanges; last answer: {}", thread.turn_count, last.content.trim());
    }

    #[::core::prelude::v1::test]
    fn untitled_threads_say_so() {
        let entry = ThreadEntry { id: "a".into(), account: account(), note_id: Some(0), title: None, updated_at: String::new(), turn_count: 0 };
        assert_eq!(entry.display_title(), "Untitled thread");
        let threads = AccountThreads { threads: vec![entry], total: 20, ..Default::default() };
        assert_eq!(threads.more(), 19);
    }
}
