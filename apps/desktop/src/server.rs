//! Native client of our owner-authenticated API. Provider credentials never enter
//! this process. One serial worker per account preserves Start/Stop/Resume
//! ordering; GPUI only polls completions. Failed uploads keep their immutable
//! local WAVs for retry.
//!
//! Every request here goes through the client of the account that owns the
//! thing it is about (`accounts::AccountClient`): a note's account, an
//! event's account. There is no "current" account to fall back to.
use crate::{
    accounts::{AccountClient as Client, AccountId, handle},
    app::Cereal,
    model::Note,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use gpui::*;
use meeting_notes_api_types::{AudioChunkDTO, AudioChunkState as State, ChatAnswerDTO, NoteDTO};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    fs,
    io::{BufRead, BufReader, Read, Seek, SeekFrom, Write},
    path::Path,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

pub fn enabled() -> bool {
    !crate::audio::preview_mode() && crate::accounts::server_mode()
}

/// Where the default account's calendar connections are managed: a page of
/// its own server, opened in the browser.
pub fn calendar_connections_url() -> Option<String> {
    crate::accounts::default_account()
        .filter(|account| account.shown())
        .map(|account| format!("{}/account", account.url))
}

fn capability_share_url(base_url: &str, token: &str, path: &str) -> Result<String, String> {
    if token.len() != 43
        || !token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        || path != format!("/s/{token}")
    {
        return Err("The server returned an invalid sharing link.".into());
    }
    Ok(format!("{}{path}", base_url.trim_end_matches('/')))
}

/// The calendars and events of one account, read with its own client.
pub fn load_calendar_events(
    account: &AccountId,
    from: &str,
    cancel: Arc<AtomicBool>,
) -> Result<crate::calendar::CalendarSnapshot, String> {
    let client = crate::accounts::client(account)?;
    load_calendar_events_with_client(&client, from, cancel)
}

fn load_calendar_events_with_client(
    client: &Client,
    from: &str,
    cancel: Arc<AtomicBool>,
) -> Result<crate::calendar::CalendarSnapshot, String> {
    let settings: meeting_notes_api_types::CalendarPreferencesDTO = client.request_with_cancel(
        "GET",
        "/api/v1/calendar/preferences",
        None,
        Some(cancel.clone()),
    )?;
    let look_ahead = u32::try_from(settings.look_ahead_days)
        .ok()
        .filter(|v| (1..=90).contains(v))
        .ok_or("Invalid calendar lookahead setting")?;
    let calendars =
        client.request_with_cancel("GET", "/api/v1/calendars", None, Some(cancel.clone()))?;
    let google: meeting_notes_api_types::GoogleStatusDTO = client.request_with_cancel(
        "GET",
        "/api/v1/integrations/google",
        None,
        Some(cancel.clone()),
    )?;
    // Callers generate UTC RFC3339 values, containing no arbitrary query text.
    let range = |text: &str| {
        chrono::DateTime::parse_from_rfc3339(text)
            .map(|d| d.to_utc().format("%Y-%m-%dT%H:%M:%SZ").to_string())
            .map_err(|_| "Invalid calendar date range".to_string())
    };
    let start = chrono::DateTime::parse_from_rfc3339(from)
        .map_err(|_| "Invalid calendar date range".to_string())?;
    let end = start + chrono::Duration::days(i64::from(look_ahead));
    let listed: Vec<Value> = client.request_with_cancel(
        "GET",
        &format!(
            "/api/v1/calendar/events?from={}&to={}",
            range(from)?,
            range(&end.to_rfc3339())?
        ),
        None,
        Some(cancel.clone()),
    )?;
    let (events, attendees) = calendar_events(listed)?;
    // The person's own calendars, selected or not, for Settings › Calendar.
    let mut personal = Vec::new();
    for connection in &google.connections {
        if uuid::Uuid::parse_str(&connection.id).is_err() {
            continue;
        }
        let calendars: Vec<meeting_notes_api_types::GoogleCalendarDTO> = client
            .request_with_cancel(
                "GET",
                &format!(
                    "/api/v1/integrations/google/connections/{}/calendars",
                    connection.id
                ),
                None,
                Some(cancel.clone()),
            )?;
        personal.extend(calendars);
    }
    Ok(crate::calendar::CalendarSnapshot {
        look_ahead_days: look_ahead,
        sources: calendars,
        events,
        attendees,
        personal,
        personal_linked: !google.connections.is_empty(),
        personal_issue: google.connections.iter().any(|c| {
            c.status == meeting_notes_api_types::GoogleConnectionDtoStatus::ReconnectRequired
        }),
    })
}

/// The listed events, and for each one the server counts attendees of, how
/// many other people are invited (`attendeeCount`). A server that does not
/// send the count leaves the map empty.
fn calendar_events(
    listed: Vec<Value>,
) -> Result<
    (
        Vec<meeting_notes_api_types::CalendarEventDTO>,
        HashMap<String, u32>,
    ),
    String,
> {
    let mut attendees = HashMap::new();
    let mut events = Vec::with_capacity(listed.len());
    for value in listed {
        let count = value
            .get("attendeeCount")
            .and_then(Value::as_u64)
            .and_then(|count| u32::try_from(count).ok());
        let event: meeting_notes_api_types::CalendarEventDTO = serde_json::from_value(value)
            .map_err(|_| "The server sent a calendar event this app cannot read".to_string())?;
        if let Some(count) = count {
            attendees.insert(event.id.clone(), count);
        }
        events.push(event);
    }
    Ok((events, attendees))
}

/// Select or deselect one of an account's own calendars, on its server.
pub fn select_personal_calendar(account: &AccountId, id: &str, selected: bool) -> Result<(), String> {
    uuid::Uuid::parse_str(id).map_err(|_| "Invalid calendar".to_string())?;
    let client = crate::accounts::client(account)?;
    let _: meeting_notes_api_types::GoogleCalendarDTO = client.request(
        "PATCH",
        &format!("/api/v1/integrations/google/calendars/{id}"),
        Some(json!({ "selected": selected })),
    )?;
    Ok(())
}

/// The note of a calendar event, asked of the event's own account. An
/// event id of one account is never sent to another.
pub fn note_for_calendar_event(
    account: &AccountId,
    event_id: &str,
    cancel: Arc<AtomicBool>,
) -> Result<(String, NoteDTO), String> {
    uuid::Uuid::parse_str(event_id).map_err(|_| "Invalid calendar event".to_string())?;
    let client = crate::accounts::client(account)?;
    let note = client.request_with_cancel(
        "POST",
        &format!("/api/v1/calendar/events/{event_id}/note"),
        None,
        Some(cancel),
    )?;
    Ok((client.url, note))
}

/// The note kept here for a server note of the account in `slot`: found by
/// `(slot, server id)`, with the address as a second check.
pub fn cached_note(
    workspace: &crate::model::Workspace,
    slot: u16,
    server_id: &str,
    url: &str,
) -> Option<u64> {
    workspace
        .notes
        .iter()
        .find(|n| {
            // Note ids are UUIDs: compared whatever their case.
            handle::slot_of(n.id) == Some(slot)
                && n.server_id
                    .as_deref()
                    .is_some_and(|id| id.eq_ignore_ascii_case(server_id))
                && n.server_url.as_deref() == Some(url)
        })
        .map(|n| n.id)
}

/// The note of a calendar event, kept in the event's account. `None` when
/// that account's notes are not loaded.
pub fn import_calendar_note(
    workspace: &mut crate::model::Workspace,
    slot: u16,
    remote: NoteDTO,
    url: String,
) -> Option<u64> {
    if let Some(id) = cached_note(workspace, slot, &remote.id, &url) {
        if let Some(note) = workspace.note_mut(id) {
            note.trashed = false;
            note.server_missing_since = None;
        }
        return Some(id);
    }
    let id = workspace.create_note_in(slot)?;
    if let Some(note) = workspace.note_mut(id) {
        note.trashed = false;
        note.title = remote.title.clone();
        note.body = remote.body.clone();
        if !note.enhanced_dirty {
            note.enhanced = remote.enhanced.clone();
        }
        note.transcript = transcript_text(&remote);
        note.server_transcript_segments = remote.transcript_segments;
        note.transcript_revision = remote.transcript_revision.max(0) as u64;
        note.generated_revision = remote.generated_revision.map(|v| v.max(0) as u64);
        note.date = remote.date;
        note.duration = remote.duration;
        note.time = display_created_at(&remote.created_at);
        note.server_created_at = Some(remote.created_at.clone());
        note.server_id = Some(remote.id);
        note.server_url = Some(url);
    }
    Some(id)
}

#[derive(Clone)]
pub enum MeetingCommand {
    Load,
    SetEvent(Option<String>),
    UpsertParticipant(meeting_notes_api_types::ManualParticipantInput),
}
pub struct MeetingReply {
    pub context: meeting_notes_api_types::NoteContextDTO,
    pub note: Option<NoteDTO>,
}
pub fn meeting_context(
    note: &Note,
    command: MeetingCommand,
    cancel: Arc<AtomicBool>,
) -> Result<MeetingReply, String> {
    let client = crate::accounts::client_of_note(note.id)?;
    let id = note
        .server_id
        .as_ref()
        .filter(|id| uuid::Uuid::parse_str(id).is_ok())
        .ok_or("Save this note before choosing its calendar event")?;
    if note.server_url.as_deref() != Some(client.url.as_str()) {
        return Err("This note belongs to another server".into());
    }
    let path = format!("/api/v1/notes/{id}/context");
    let refresh_note = matches!(&command, MeetingCommand::SetEvent(Some(_)));
    let context = match command {
        MeetingCommand::Load => client.request_with_cancel("GET", &path, None, Some(cancel)),
        MeetingCommand::SetEvent(event_id) => client.request_with_cancel(
            "PUT",
            &path,
            Some(
                serde_json::to_value(meeting_notes_api_types::UpdateNoteContextInput { event_id })
                    .map_err(|_| "Invalid calendar selection")?,
            ),
            Some(cancel),
        ),
        MeetingCommand::UpsertParticipant(input) => client.request_with_cancel(
            "POST",
            &format!("{path}/participants"),
            Some(serde_json::to_value(input).map_err(|_| "Invalid attendee")?),
            Some(cancel),
        ),
    }?;
    let note = if refresh_note {
        Some(client.request("GET", &format!("/api/v1/notes/{id}"), None)?)
    } else {
        None
    };
    Ok(MeetingReply { context, note })
}
/// The events a note may be attached to: those of the note's own account,
/// and no other's.
pub fn meeting_picker_events(
    note_id: u64,
    cancel: Arc<AtomicBool>,
) -> Result<Vec<meeting_notes_api_types::CalendarEventDTO>, String> {
    let client = crate::accounts::client_of_note(note_id)?;
    let now = chrono::Utc::now();
    let from = (now - chrono::Duration::days(7)).format("%Y-%m-%dT%H:%M:%SZ");
    let to = (now + chrono::Duration::days(60)).format("%Y-%m-%dT%H:%M:%SZ");
    client.request_with_cancel(
        "GET",
        &format!("/api/v1/calendar/events?from={from}&to={to}"),
        None,
        Some(cancel),
    )
}

/// Match the local twelve-hour timestamps shown by native notes, including the
/// historical local offset at creation time rather than the server's UTC hour.
fn display_created_at(value: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|date| {
            date.with_timezone(&chrono::Local)
                .format("%-I:%M %p")
                .to_string()
        })
        .unwrap_or_default()
}

/// What every save sends of a note. A note nobody has named sends no title:
/// its empty or placeholder one must never replace a title the server has
/// given it since (from its generated notes, or from a calendar event).
fn note_fields(note: &Note) -> Value {
    let mut fields = json!({"body":note.body,"date":note.date});
    if !crate::meeting::is_default_note_title(&note.title) {
        fields["title"] = json!(note.title);
    }
    fields
}

fn note_update_payload(note: &Note) -> Value {
    let mut update = note_fields(note);
    if note.enhanced_dirty {
        update["enhanced"] = json!(note.enhanced);
    }
    update
}

#[derive(Debug, PartialEq)]
pub enum ChatStreamEvent {
    Delta(String),
    Tool(meeting_notes_api_types::ChatToolProgressDTO),
    Citations(Vec<meeting_notes_api_types::ChatCitationDTO>),
    Done,
    Error(String),
}

fn decode_chat_frame(data: &str) -> Option<ChatStreamEvent> {
    // The generated name of this enum changed when the live wire types
    // joined the contracts; the alias keeps the match below as it was.
    use meeting_notes_api_types::{ChatStreamEventDTO, ChatStreamEventDtoType as Type};
    let value: ChatStreamEventDTO = serde_json::from_str(data).ok()?;
    match value.chat_stream_event_dto_type {
        Type::Delta if value.text.len() <= 1_048_576 => Some(ChatStreamEvent::Delta(value.text)),
        Type::Done => Some(ChatStreamEvent::Done),
        Type::Error => Some(ChatStreamEvent::Error(
            value.text.chars().take(1000).collect(),
        )),
        Type::Tool => value
            .tool
            .filter(|tool| {
                uuid::Uuid::parse_str(&tool.call_id).is_ok()
                    && tool.label.chars().count() <= 200
                    && tool.result_count.is_none_or(|count| count >= 0)
                    && tool.results.as_ref().is_none_or(|results| {
                        results.len() <= 20
                            && results.iter().all(|meeting| {
                                uuid::Uuid::parse_str(&meeting.note_id).is_ok()
                                    && meeting.title.chars().count() <= 2000
                                    && chrono::NaiveDate::parse_from_str(&meeting.date, "%Y-%m-%d")
                                        .is_ok()
                            })
                    })
            })
            .map(ChatStreamEvent::Tool),
        Type::Citations => value
            .citations
            .filter(|citations| {
                citations.len() <= 40
                    && citations.iter().all(|citation| {
                        crate::chat::valid_citation_id(&citation.id)
                            && uuid::Uuid::parse_str(&citation.note_id).is_ok()
                            && citation
                                .segment_id
                                .as_deref()
                                .is_none_or(|id| uuid::Uuid::parse_str(id).is_ok())
                            && !citation.quote.is_empty()
                            && citation.quote.chars().count() <= 20_000
                            && citation.transcript_revision >= 0
                            && citation.start_ms.is_none_or(|time| time >= 0)
                    })
                    && citations
                        .iter()
                        .map(|citation| &citation.id)
                        .collect::<std::collections::HashSet<_>>()
                        .len()
                        == citations.len()
            })
            .map(ChatStreamEvent::Citations),
        Type::Delta => None,
    }
}

/// Independent, cancellable chat transport. Recording Stop/uploads never wait
/// behind an open language-model stream. The owner token travels through stdin.
pub fn chat_model_key(choice: &meeting_notes_api_types::LanguageModelSelection) -> String {
    use meeting_notes_api_types::LanguageModelSelectionProvider as Provider;
    let provider = match choice.provider { Provider::Openrouter => "openrouter", Provider::Openai => "openai", Provider::Anthropic => "anthropic" };
    format!("{provider}:{}", choice.model)
}
/// A chat about a note goes to the note's account. A chat across meetings
/// belongs to one account: `scope`, or the default account when none is
/// named (until the scope can be chosen, section 4.2).
pub fn stream_chat_with_model(
    note: Option<Note>, scope: Option<AccountId>, message: String, history: Vec<Value>, cancel: Arc<AtomicBool>, model: Option<String>,
) -> mpsc::Receiver<ChatStreamEvent> {
    let client = match (&note, &scope) {
        (Some(note), _) => crate::accounts::client_of_note(note.id),
        (None, Some(account)) => crate::accounts::client(account),
        (None, None) => crate::accounts::default_client(),
    };
    stream_chat_with_client(client, note, message, history, cancel, model)
}
fn stream_chat_with_client(
    client: Result<Client, String>,
    note: Option<Note>,
    message: String,
    history: Vec<Value>,
    cancel: Arc<AtomicBool>,
    model: Option<String>,
) -> mpsc::Receiver<ChatStreamEvent> {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let run = || -> Result<(), String> {
            let mut client = client?;
            if cancel.load(Ordering::Acquire) {
                return Ok(());
            }
            let path = if let Some(note) = note {
                let id = client.ensure(&note)?;
                // Chat includes edits still visible in the native note, even if
                // the user has not navigated away or pressed Save yet.
                let _: NoteDTO = client.request_with_cancel(
                    "PATCH",
                    &format!("/api/v1/notes/{id}"),
                    Some(note_fields(&note)),
                    Some(cancel.clone()),
                )?;
                format!("/api/v1/notes/{id}/chat/stream")
            } else {
                "/api/v1/chat/stream".into()
            };
            let mut payload = json!({"message":message,"history":history,"includeSources":true});
            if let Some(model) = &model { payload["model"] = json!(model); }
            if let Ok(time_zone) = iana_time_zone::get_timezone() {
                payload["timeZone"] = json!(time_zone);
            }
            // The stream's own transport, started by the account's client:
            // the session and the address come from the same place.
            let mut child = client.chat_stream(&path, &payload)?;
            let Some(output) = child.stdout.take() else {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Chat response unavailable".into());
            };
            let events = sender.clone();
            let stopped = cancel.clone();
            let reader = std::thread::spawn(move || {
                let mut input = BufReader::new(output);
                let mut line = String::new();
                let mut data = Vec::new();
                let mut bytes = 0;
                loop {
                    line.clear();
                    match input.read_line(&mut line) {
                        Ok(0) => break,
                        Ok(count) => {
                            bytes += count;
                            if bytes > 8 * 1024 * 1024 {
                                let _ = events.send(ChatStreamEvent::Error(
                                    "Chat response exceeded its supported size.".into(),
                                ));
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                    if stopped.load(Ordering::Acquire) {
                        return;
                    }
                    let line = line.trim_end_matches(['\r', '\n']);
                    if line.is_empty() {
                        if !data.is_empty() {
                            let frame = data.join("\n");
                            data.clear();
                            let Some(event) = decode_chat_frame(&frame) else {
                                let _ = events.send(ChatStreamEvent::Error(
                                    "The server returned an invalid chat stream.".into(),
                                ));
                                return;
                            };
                            let done =
                                matches!(event, ChatStreamEvent::Done | ChatStreamEvent::Error(_));
                            if events.send(event).is_err() || done {
                                return;
                            }
                        }
                    } else if let Some(value) = line.strip_prefix("data:") {
                        data.push(value.strip_prefix(' ').unwrap_or(value).to_string());
                    }
                }
                if !stopped.load(Ordering::Acquire) {
                    let _ = events.send(ChatStreamEvent::Error(
                        "The response was interrupted. Retry to continue.".into(),
                    ));
                }
            });
            loop {
                if cancel.load(Ordering::Acquire) {
                    let _ = child.kill();
                    let _ = child.wait();
                    break;
                }
                match child.try_wait() {
                    Ok(Some(_)) => break,
                    Ok(None) => std::thread::sleep(Duration::from_millis(25)),
                    Err(_) => {
                        let _ = child.kill();
                        let _ = child.wait();
                        break;
                    }
                }
            }
            let _ = reader.join();
            Ok(())
        };
        if let Err(error) = run()
            && !cancel.load(Ordering::Acquire)
        {
            let _ = sender.send(ChatStreamEvent::Error(error));
        }
    });
    receiver
}

#[derive(Clone)]
pub enum Action {
    Save,
    Recording(&'static str),
    Sync,
    Generate,
    // Retained for the explicitly opt-in legacy non-streaming integration test.
    // Interactive clients use the independent cancellable SSE transport.
    #[allow(dead_code)]
    Chat {
        message: String,
        history: Vec<Value>,
    },
    Share,
    RevokeShares,
}
pub struct Reply {
    pub note: NoteDTO,
    pub url: String,
    pub answer: Option<String>,
    pub share_url: Option<String>,
}
struct Job {
    note: Note,
    action: Action,
    reply: mpsc::Sender<Result<Reply, String>>,
}
pub struct Bridge {
    sender: mpsc::Sender<Job>,
}
impl Bridge {
    /// The serial worker of one account. Its client is made again whenever
    /// the account's epoch has moved (a replaced token), keeping what it
    /// knows of which notes are on the server.
    pub fn new(account: AccountId) -> Self {
        let (sender, receiver) = mpsc::channel::<Job>();
        std::thread::spawn(move || {
            let mut client: Option<Client> = None;
            while let Ok(job) = receiver.recv() {
                if client.as_ref().is_none_or(|client| client.stale()) {
                    let ids = client.take().map(|client| client.ids).unwrap_or_default();
                    client = match crate::accounts::client(&account) {
                        Ok(mut fresh) => {
                            fresh.ids = ids;
                            Some(fresh)
                        }
                        Err(error) => {
                            let _ = job.reply.send(Err(error));
                            continue;
                        }
                    };
                }
                let result = client
                    .as_mut()
                    .expect("the client was just made")
                    .run(&job.note, job.action);
                let _ = job.reply.send(result);
            }
        });
        Self { sender }
    }
    fn send(&self, note: Note, action: Action) -> mpsc::Receiver<Result<Reply, String>> {
        let (reply, receiver) = mpsc::channel();
        let _ = self.sender.send(Job {
            note,
            action,
            reply,
        });
        receiver
    }
}

impl Client {
    fn association_path(&self, local_id: u64) -> Result<std::path::PathBuf, String> {
        let root = self
            .storage_root
            .clone()
            .ok_or("This account has no place to keep recordings yet")?;
        let namespace = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, self.url.as_bytes());
        // On disk a note is always named by its file id.
        Ok(root
            .join("server-associations")
            .join(namespace.to_string())
            .join(format!("{}.txt", handle::file_id(local_id))))
    }
    fn ensure(&mut self, note: &Note) -> Result<String, String> {
        if self.stale() {
            return Err("Account changed; request cancelled".into());
        }
        // A note is only ever sent to the account it belongs to.
        if let (Some(mine), Some(theirs)) = (self.slot, handle::slot_of(note.id))
            && mine != theirs
        {
            return Err("This note belongs to another account".into());
        }
        let sidecar = self.association_path(note.id)?;
        if let Some(id) = self.ids.get(&note.id).cloned() {
            // What this worker remembers holds only while the note is still
            // attached: a note restored from "Deleted on the server" has
            // neither a server id nor a sidecar, and is created again.
            if note.server_id.is_some() || sidecar.exists() {
                return Ok(id);
            }
            self.ids.remove(&note.id);
        }
        if let Some(id) = &note.server_id {
            if note.server_url.as_deref() != Some(&self.url) {
                return Err("This note belongs to a different server. Restore its server configuration before syncing.".into());
            }
            self.ids.insert(note.id, id.clone());
            return Ok(id.clone());
        }
        // Recover the association even if the process exited before the UI saved
        // its completion. Sidecar contains identifiers only, never credentials.
        if let Ok(id) = fs::read_to_string(&sidecar)
            && uuid::Uuid::parse_str(id.trim()).is_ok()
        {
            self.ids.insert(note.id, id.trim().into());
            return Ok(id.trim().into());
        }
        let created: NoteDTO = self.request(
            "POST",
            "/api/v1/notes",
            Some(json!({"title":note.title,"body":note.body,"date":note.date})),
        )?;
        if let Some(parent) = sidecar.parent() {
            fs::create_dir_all(parent).map_err(|_| "Cannot save server note association")?;
        }
        fs::write(&sidecar, &created.id)
            .map_err(|_| "Cannot save server note association; stop before retrying")?;
        self.ids.insert(note.id, created.id.clone());
        Ok(created.id)
    }
    fn run(&mut self, note: &Note, action: Action) -> Result<Reply, String> {
        let id = self.ensure(note)?;
        let base = format!("/api/v1/notes/{id}");
        let _: NoteDTO = self.request("PATCH", &base, Some(note_update_payload(note)))?;
        let mut answer = None;
        let mut share_url = None;
        match action {
            Action::Save => {}
            Action::Recording(command) => {
                let current: NoteDTO = self.request("GET", &base, None)?;
                let command = recording_command(command, &current.recording_state);
                if command == "stop" {
                    stop_trace("the stop action has saved and read the note");
                    // Let the live transcript stream send the end of the
                    // capture first: the server ends the live session on
                    // Stop. No stream, or one that is not connected, costs
                    // no time; a stuck one costs at most this long.
                    crate::live::wait_for_handover(note.id, Duration::from_millis(2500));
                    stop_trace("the stream has handed over (or the wait ran out)");
                }
                let _: NoteDTO = self.request(
                    "POST",
                    &format!("{base}/recording"),
                    Some(json!({"command":command})),
                )?;
                if command == "stop" {
                    stop_trace("the server has been told to stop");
                    self.upload(note, &id)?;
                    stop_trace("the upload is finished: every part is ready");
                }
            }
            Action::Sync => {
                let _: NoteDTO = self.request(
                    "POST",
                    &format!("{base}/recording"),
                    Some(json!({"command":"stop"})),
                )?;
                self.upload(note, &id)?;
            }
            Action::Generate => {
                let _: NoteDTO = self.request(
                    "POST",
                    &format!("{base}/recording"),
                    Some(json!({"command":"stop"})),
                )?;
                self.upload(note, &id)?;
                let current: NoteDTO = self.request("GET", &base, None)?;
                let _: NoteDTO = self.request(
                    "POST",
                    &format!("{base}/generate"),
                    Some(json!({"expectedTranscriptRevision":current.transcript_revision})),
                )?;
            }
            Action::Chat { message, history } => {
                let reply: ChatAnswerDTO = self.request(
                    "POST",
                    &format!("{base}/chat"),
                    Some(json!({"message":message,"history":history})),
                )?;
                answer = Some(reply.answer);
            }
            Action::Share => {
                let share: meeting_notes_api_types::CreatedShareDTO = self.request(
                    "POST",
                    &format!("{base}/shares"),
                    Some(json!({"includeBody":true,"includeEnhanced":true})),
                )?;
                share_url = Some(capability_share_url(&self.url, &share.token, &share.path)?);
            }
            Action::RevokeShares => {
                let shares: Vec<meeting_notes_api_types::ShareDTO> =
                    self.request("GET", &format!("{base}/shares"), None)?;
                for share in shares.into_iter().filter(|s| s.revoked_at.is_none()) {
                    let _: meeting_notes_api_types::ShareDTO =
                        self.request("DELETE", &format!("/api/v1/shares/{}", share.id), None)?;
                }
            }
        }
        let remote = self.request("GET", &base, None)?;
        Ok(Reply {
            note: remote,
            url: self.url.clone(),
            answer,
            share_url,
        })
    }
    /// Where the identities of a note's uploaded parts are kept.
    fn mapping_root(&self, note: &Note, id: &str) -> Result<std::path::PathBuf, String> {
        let root = self
            .association_path(note.id)?
            .parent()
            .ok_or("Invalid association directory")?
            .join("uploads")
            .join(id);
        fs::create_dir_all(&root).map_err(|_| "Cannot preserve upload identities")?;
        Ok(root)
    }

    /// Where a capture sits on the note's timeline and how long its parts
    /// are. Both are decided once, the first time anything places the
    /// capture, and kept in two small files next to its part receipts; every
    /// later upload (while recording, after Stop, a retry next week) reads
    /// them, so a part always has the same identity and position.
    ///
    /// - A capture nothing has placed yet starts where the note's audio so
    ///   far ends, and is cut into [`PART_SECONDS`] parts.
    /// - A capture placed before part lengths were recorded, or whose parts
    ///   are already on the server from an older client, keeps the 60-second
    ///   parts those identities were made with.
    ///
    /// `recording` is true for the capture that is still being recorded: its
    /// files are not final and none of its parts can be on the server.
    fn capture_plan(
        &self,
        id: &str,
        capture: &crate::audio::CaptureManifest,
        existing: &HashMap<String, AudioChunkDTO>,
        mapping_root: &Path,
        offset_ms: u64,
        recording: bool,
    ) -> Result<CapturePlan, String> {
        let capture_key =
            uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, capture.capture_id.as_bytes())
                .to_string();
        let base_path = mapping_root.join(format!("capture-{capture_key}.json"));
        let parts_path = mapping_root.join(format!("capture-{capture_key}.parts.json"));
        let mut capture_base: Option<u64> = fs::read(&base_path)
            .ok()
            .and_then(|data| serde_json::from_slice(&data).ok());
        let recorded_parts: Option<u64> = fs::read(&parts_path)
            .ok()
            .and_then(|data| serde_json::from_slice(&data).ok())
            .filter(|seconds| (1..=600).contains(seconds));
        if capture_base.is_none() && recorded_parts.is_none() && !recording {
            // Migrate an already-uploaded capture using its server metadata;
            // this preserves old chunk identities across client upgrades.
            'sources: for (channel, stats) in [
                ("mic", &capture.stats.mic),
                ("system", &capture.stats.system),
            ] {
                let paths = channel_paths(stats);
                let mut relative = 0;
                for path in paths {
                    let wav = WavReader::open(Path::new(&path))?;
                    let step = wav.step;
                    for block in 0..wav.frames.div_ceil(step) {
                        let chunk_id =
                            chunk_identity(id, &capture.capture_id, channel, &path, block);
                        if let Some(c) = existing.get(&chunk_id) {
                            capture_base = Some((c.start_ms.max(0) as u64).saturating_sub(
                                relative + block * step * 1000 / wav.rate as u64,
                            ));
                            break 'sources;
                        }
                    }
                    relative += wav.duration_ms();
                }
            }
        }
        let part_seconds = match recorded_parts {
            Some(seconds) => seconds,
            // Never placed: a new capture.
            None if capture_base.is_none() => {
                save_mapping(&parts_path, &PART_SECONDS)?;
                PART_SECONDS
            }
            None => LEGACY_PART_SECONDS,
        };
        let base_ms = capture_base.unwrap_or(offset_ms);
        save_mapping(&base_path, &base_ms)?;
        Ok(CapturePlan {
            base_ms,
            part_seconds,
        })
    }

    /// The sequence number for the next part that has none yet: above every
    /// number the server holds for the note and above every number a receipt
    /// here has promised to a part, sent or not. The server refuses two parts
    /// of one channel with the same number.
    fn next_sequence(existing: &HashMap<String, AudioChunkDTO>, mapping_root: &Path) -> u64 {
        let remote = existing.values().map(|c| c.sequence.max(0) as u64 + 1);
        let promised = fs::read_dir(mapping_root)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|entry| !entry.file_name().to_string_lossy().starts_with("capture-"))
            .filter_map(|entry| fs::read(entry.path()).ok())
            .filter_map(|data| serde_json::from_slice::<UploadReceipt>(&data).ok())
            .map(|receipt| receipt.sequence + 1);
        remote.chain(promised).max().unwrap_or(0)
    }

    /// Send one part of a capture, unless the server already holds it.
    /// `start_ms` is its position on the note's timeline. Returns its ID.
    ///
    /// A part's ID comes from what it is (note, capture, channel, file,
    /// block), and its sequence number and position are written down before
    /// it is sent, so sending it again, now or after a crash, is the same
    /// request. A part the server has is left alone: it was sent while the
    /// meeting was still being recorded, or by an earlier attempt. One whose
    /// transcription failed is sent again and retried, as it always was.
    #[allow(clippy::too_many_arguments)]
    fn send_part(
        &self,
        target: &PartTarget,
        existing: &HashMap<String, AudioChunkDTO>,
        channel: &str,
        path: &str,
        block: u64,
        bytes: Vec<u8>,
        start_ms: u64,
        sequence: &mut u64,
        cancel: Option<Arc<AtomicBool>>,
    ) -> Result<String, String> {
        let chunk_id = chunk_identity(target.id, target.capture_id, channel, path, block);
        let receipt_path = target.mapping_root.join(format!("{chunk_id}.json"));
        let remote = existing.get(&chunk_id);
        let receipt: UploadReceipt = if let Some(remote) = remote {
            UploadReceipt {
                sequence: remote.sequence.max(0) as u64,
                start_ms: remote.start_ms.max(0) as u64,
            }
        } else if let Ok(data) = fs::read(&receipt_path) {
            serde_json::from_slice(&data)
                .map_err(|_| "Saved upload identity is malformed; local audio remains intact")?
        } else {
            UploadReceipt {
                sequence: *sequence,
                start_ms,
            }
        };
        save_mapping(&receipt_path, &receipt)?;
        *sequence = (*sequence).max(receipt.sequence + 1);
        if remote.is_some_and(|remote| remote.state != State::Failed) {
            return Ok(chunk_id);
        }
        let base = format!("/api/v1/notes/{}/audio/chunks", target.id);
        let chunk: AudioChunkDTO = self.request_with_cancel(
            "PUT",
            &format!("{base}/{chunk_id}"),
            Some(json!({"channel":channel,"sequence":receipt.sequence,"startMs":receipt.start_ms,"audioBase64":STANDARD.encode(bytes)})),
            cancel.clone(),
        )?;
        if chunk.state == State::Failed {
            let _: AudioChunkDTO = self.request_with_cancel(
                "POST",
                &format!("{base}/{chunk_id}/retry"),
                None,
                cancel,
            )?;
        }
        Ok(chunk_id)
    }

    fn upload(&self, note: &Note, id: &str) -> Result<(), String> {
        // Parts sent while the meeting was being recorded come from another
        // thread. It is told to stop and has let go before this one looks at
        // what the server holds, so no part is numbered twice.
        stop_part_uploads(note.id, Duration::from_secs(5));
        stop_trace("the part uploader of the meeting has let go");
        let base = format!("/api/v1/notes/{id}/audio/chunks");
        let existing: Vec<AudioChunkDTO> = self.request("GET", &base, None)?;
        stop_trace(&format!("the server holds {} parts", existing.len()));
        let existing: HashMap<_, _> = existing.into_iter().map(|c| (c.id.clone(), c)).collect();
        let mut offset_ms = existing
            .values()
            .map(|c| (c.start_ms + c.duration_ms).max(0) as u64)
            .max()
            .unwrap_or(0);
        let mapping_root = self.mapping_root(note, id)?;
        let mut sequence = Self::next_sequence(&existing, &mapping_root);
        let mut submitted = Vec::new();
        for saved in &note.audio_captures {
            let started = Instant::now();
            let capture = loop {
                let current = fs::read(&saved.manifest_path)
                    .ok()
                    .and_then(|data| {
                        serde_json::from_slice::<crate::audio::CaptureManifest>(&data).ok()
                    })
                    .unwrap_or_else(|| saved.clone());
                if current.completed() {
                    stop_trace("the capture is finalised");
                    break current;
                }
                if started.elapsed() > Duration::from_secs(15) {
                    return Err(
                        "Audio is still finalizing. Retry transcription after it has saved.".into(),
                    );
                }
                std::thread::sleep(Duration::from_millis(50));
            };
            let plan =
                self.capture_plan(id, &capture, &existing, &mapping_root, offset_ms, false)?;
            let target = PartTarget {
                id,
                capture_id: &capture.capture_id,
                mapping_root: &mapping_root,
            };
            for (channel, stats) in [
                ("mic", &capture.stats.mic),
                ("system", &capture.stats.system),
            ] {
                let paths = channel_paths(stats);
                let mut channel_offset = plan.base_ms;
                for path in paths {
                    let mut wav = WavReader::open(Path::new(&path))?.parts_of(plan.part_seconds);
                    let mut block = 0;
                    while let Some((bytes, start_ms)) = wav.next_chunk()? {
                        submitted.push(self.send_part(
                            &target,
                            &existing,
                            channel,
                            &path,
                            block,
                            bytes,
                            channel_offset + start_ms,
                            &mut sequence,
                            None,
                        )?);
                        block += 1;
                    }
                    channel_offset += wav.duration_ms();
                }
            }
            offset_ms =
                offset_ms.max(plan.base_ms + (capture.audio_seconds() * 1000.).round() as u64);
        }
        if submitted.is_empty() {
            return Ok(());
        }
        stop_trace(&format!("{} parts are on the server or sent", submitted.len()));
        let started = Instant::now();
        loop {
            let chunks: Vec<AudioChunkDTO> = self.request("GET", &base, None)?;
            let mine: Vec<_> = chunks
                .iter()
                .filter(|c| submitted.contains(&c.id))
                .collect();
            if mine.iter().any(|c| c.state == State::Failed) {
                return Err("Transcription failed. Local audio is safe; choose Retry transcription to retry.".into());
            }
            if mine.len() == submitted.len() && mine.iter().all(|c| c.state == State::Ready) {
                return Ok(());
            }
            if started.elapsed() > Duration::from_secs(180) {
                return Err("Transcription is still pending on the server. Choose Retry transcription to refresh.".into());
            }
            std::thread::sleep(Duration::from_millis(600));
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct UploadReceipt {
    sequence: u64,
    start_ms: u64,
}

/// Seconds of audio in one durable part of a capture made from now on. A
/// part is sent as soon as it is complete, while the meeting is still being
/// recorded, so this is about how long a stretch the live stream missed
/// stays missing from the transcript.
const PART_SECONDS: u64 = 30;
/// The part length of captures whose identities predate that choice.
const LEGACY_PART_SECONDS: u64 = 60;

/// A capture's place on the note timeline and the length of its parts
/// (see [`Client::capture_plan`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CapturePlan {
    base_ms: u64,
    part_seconds: u64,
}

/// What the parts of one capture are sent as part of.
struct PartTarget<'a> {
    /// The note's ID on the server.
    id: &'a str,
    capture_id: &'a str,
    mapping_root: &'a Path,
}
fn recording_command<'a>(
    requested: &'a str,
    state: &meeting_notes_api_types::RecordingState,
) -> &'a str {
    if requested == "start" && *state == meeting_notes_api_types::RecordingState::Stopped {
        "resume"
    } else {
        requested
    }
}
pub fn duration_seconds(text: &str) -> Option<f64> {
    let pieces: Vec<_> = text.split(':').collect();
    if !(2..=3).contains(&pieces.len()) {
        return None;
    }
    pieces
        .into_iter()
        .try_fold(0u64, |sum, p| {
            p.parse::<u64>()
                .ok()
                .and_then(|part| sum.checked_mul(60)?.checked_add(part))
        })
        .map(|v| v as f64)
}
/// Read-only source context has its own pending state, independent of recording
/// commands and chat. Only the selected instance's server note can be queried.
pub fn load_evidence(
    note: &Note,
    paragraph: &str,
    cancel: Arc<AtomicBool>,
) -> Result<meeting_notes_api_types::EvidenceDTO, String> {
    let client = crate::accounts::client_of_note(note.id)?;
    let id = note
        .server_id
        .as_deref()
        .ok_or("Save this note to the server before viewing transcript context.")?;
    if note.server_url.as_deref() != Some(client.url.as_str()) || uuid::Uuid::parse_str(id).is_err()
    {
        return Err("This note is not connected to the selected server.".into());
    }
    let revision =
        i64::try_from(note.transcript_revision).map_err(|_| "Invalid transcript revision")?;
    let payload = meeting_notes_api_types::EvidenceRequestDTO {
        paragraph: paragraph.to_string(),
        expected_transcript_revision: revision,
    };
    client
        .request_with_cancel(
            "POST",
            &format!("/api/v1/notes/{id}/evidence"),
            Some(serde_json::to_value(payload).map_err(|_| "Invalid source-context request")?),
            Some(cancel),
        )
        .map_err(|_| "Transcript context could not be loaded. Try again.".into())
}

/// Where the live transcript stream of a running capture connects, and where
/// that capture sits on the note's timeline. Runs on the stream's own thread.
///
/// The timeline position is decided here, once, by the rule `Client::upload`
/// uses for the parts it sends after Stop, and is saved in the same mapping
/// file, which `upload` then reads. So the stream and the durable parts place
/// the same audio at the same milliseconds, and the server can tell which
/// parts the stream already covered.
pub fn live_target(note: &Note, capture_id: &str) -> crate::live::Resolved {
    use crate::live::Resolved;
    // The note's own account, however long the stream has been waiting: a
    // session that has ended leaves it waiting for the sign-in.
    let client = match crate::accounts::client_of_note(note.id) {
        Ok(client) => client,
        Err(_) if crate::accounts::of_note(note.id).is_some() => return Resolved::Later,
        Err(_) => return Resolved::Never,
    };
    let Some(id) = note
        .server_id
        .as_deref()
        .filter(|id| uuid::Uuid::parse_str(id).is_ok())
    else {
        return Resolved::Never;
    };
    if note.server_url.as_deref() != Some(client.url.as_str()) {
        return Resolved::Never;
    }
    match client.live_capture_base(note, id, capture_id) {
        Ok(base_ms) => Resolved::Ready(crate::live::Target {
            endpoint: client.live(id),
            capture_id: capture_id.to_owned(),
            base_ms,
        }),
        // The server did not answer, or an earlier capture is not final yet.
        Err(_) => Resolved::Later,
    }
}

/// What the uploader of a recording's parts tells the window.
pub enum PartEvent {
    /// The server finished a part and the note's transcript moved on: the
    /// transcript as the server now has it. This is how text the live stream
    /// did not deliver (a stretch it missed, or all of it on a server
    /// without live transcription) reaches the window before Stop.
    Transcript {
        revision: u64,
        segments: Vec<meeting_notes_api_types::TranscriptSegmentDTO>,
    },
}

/// Part uploaders that are running: the note they belong to, the flag that
/// stops them, and whether they have let go.
type RunningUploader = (u64, Arc<AtomicBool>, Arc<(Mutex<bool>, Condvar)>);
static PART_UPLOADERS: Mutex<Vec<RunningUploader>> = Mutex::new(Vec::new());

/// Stop the part uploader of `note`, if one is running, and wait (at most
/// `limit`) until it has let go of whatever request it was in.
/// For the live probe (`CEREAL_LIVE_PROBE`): where the time between Stop
/// and a finished upload goes, as lines on standard error with the time of
/// day.
pub(crate) fn stop_trace(event: &str) {
    if crate::environment::var_os("CEREAL_LIVE_PROBE").is_some() {
        eprintln!("stop path {}: {event}", chrono::Local::now().format("%H:%M:%S%.3f"));
    }
}

fn stop_part_uploads(note: u64, limit: Duration) {
    let running: Vec<_> = PART_UPLOADERS
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .iter()
        .filter(|(owner, _, _)| *owner == note)
        .map(|(_, cancel, done)| (cancel.clone(), done.clone()))
        .collect();
    for (cancel, done) in running {
        cancel.store(true, Ordering::Release);
        let finished = done.0.lock().unwrap_or_else(|poison| poison.into_inner());
        let _ = done.1.wait_timeout_while(finished, limit, |finished| !*finished);
    }
}

/// Uploads the parts of one capture as they become complete, while the
/// meeting is still being recorded. It has its own thread and its own
/// client, so it never waits behind (or holds up) the serial worker that
/// carries Start, Stop and saves.
///
/// Nothing depends on it. A part it did not get to is sent by the upload
/// after Stop, with the same identity; a part it did send is not sent again.
pub struct PartUploads {
    cancel: Arc<AtomicBool>,
    pub events: mpsc::Receiver<PartEvent>,
}

impl PartUploads {
    /// Follow capture `capture_id` of `note` (which must already be on the
    /// server) through `source`.
    pub fn start(note: Note, capture_id: String, source: crate::live::Source) -> Self {
        let id = note.id;
        Self::start_with(
            move || crate::accounts::client_of_note(id),
            note,
            capture_id,
            source,
        )
    }

    fn start_with(
        connect: impl FnMut() -> Result<Client, String> + Send + 'static,
        note: Note,
        capture_id: String,
        source: crate::live::Source,
    ) -> Self {
        let cancel = Arc::new(AtomicBool::new(false));
        let done = Arc::new((Mutex::new(false), Condvar::new()));
        let (sender, events) = mpsc::channel();
        let registered = (note.id, cancel.clone(), done.clone());
        PART_UPLOADERS
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .push(registered);
        let (stop, finished) = (cancel.clone(), done.clone());
        let spawned = std::thread::Builder::new()
            .name("part-uploads".into())
            .spawn(move || {
                upload_parts(connect, &note, &capture_id, &source, &stop, &sender);
                finish_part_uploads(&finished);
            });
        if spawned.is_err() {
            finish_part_uploads(&done);
        }
        Self { cancel, events }
    }

    /// Stop after the request in flight, if any, has been abandoned.
    pub fn stop(&self) {
        self.cancel.store(true, Ordering::Release);
    }

    #[cfg(test)]
    pub(crate) fn uploads_events(&self) -> Vec<PartEvent> {
        self.events.try_iter().collect()
    }
}

impl Drop for PartUploads {
    fn drop(&mut self) {
        self.stop();
    }
}

fn finish_part_uploads(done: &Arc<(Mutex<bool>, Condvar)>) {
    *done.0.lock().unwrap_or_else(|poison| poison.into_inner()) = true;
    done.1.notify_all();
    PART_UPLOADERS
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .retain(|(_, _, other)| !Arc::ptr_eq(other, done));
}

/// How long after sending a part the server is first asked whether it has
/// finished it (shorter under test, where parts are two seconds long).
const FIRST_PART_CHECK: Duration = Duration::from_millis(if cfg!(test) { 500 } else { 4000 });

/// A part that has been sent and is waiting for the server to finish it.
struct SentPart {
    id: String,
    due: Instant,
    wait: Duration,
}

/// What the uploader knows once it has reached the server.
struct Uploading {
    client: Client,
    plan: CapturePlan,
    mapping_root: std::path::PathBuf,
    existing: HashMap<String, AudioChunkDTO>,
    sequence: u64,
}

fn upload_parts(
    mut connect: impl FnMut() -> Result<Client, String>,
    note: &Note,
    capture_id: &str,
    source: &crate::live::Source,
    cancel: &Arc<AtomicBool>,
    events: &mpsc::Sender<PartEvent>,
) {
    let Some(id) = note
        .server_id
        .as_deref()
        .filter(|id| uuid::Uuid::parse_str(id).is_ok())
    else {
        return;
    };
    let chunks = format!("/api/v1/notes/{id}/audio/chunks");
    let stopped = || cancel.load(Ordering::Acquire);
    let mut uploading: Option<Uploading> = None;
    // The next part of each file that has not been looked at.
    let mut next: HashMap<(&'static str, String), u64> = HashMap::new();
    let mut sent: Vec<SentPart> = Vec::new();
    let mut revision: Option<i64> = None;
    let (mut failures, mut retry_at) = (0u32, Instant::now());
    while !stopped() {
        std::thread::sleep(Duration::from_millis(250));
        let now = Instant::now();
        if now < retry_at {
            continue;
        }
        let Some(manifest) = source.manifest() else {
            continue;
        };
        let mut pass = || -> Result<(), String> {
            if uploading.is_none() {
                let client = connect()?;
                if note.server_url.as_deref().is_some_and(|url| url != client.url) {
                    return Err("This note belongs to another server".into());
                }
                let existing: Vec<AudioChunkDTO> =
                    client.request_with_cancel("GET", &chunks, None, Some(cancel.clone()))?;
                let existing: HashMap<_, _> =
                    existing.into_iter().map(|c| (c.id.clone(), c)).collect();
                let plan = client.live_capture_plan(note, id, capture_id, &existing)?;
                let mapping_root = client.mapping_root(note, id)?;
                let sequence = Client::next_sequence(&existing, &mapping_root);
                uploading = Some(Uploading {
                    client,
                    plan,
                    mapping_root,
                    existing,
                    sequence,
                });
            }
            let up = uploading.as_mut().expect("the uploader was just prepared");
            let target = PartTarget {
                id,
                capture_id,
                mapping_root: &up.mapping_root,
            };
            // Every part that is complete: all of a file that has stopped
            // growing, and of the one still growing the whole parts so far.
            // The last, short part of the capture is the after-Stop upload's.
            for (channel, stats) in [("mic", &manifest.stats.mic), ("system", &manifest.stats.system)]
            {
                let paths = channel_paths(stats);
                let mut channel_offset = up.plan.base_ms;
                for (index, path) in paths.iter().enumerate() {
                    let closed = index + 1 < paths.len();
                    let Ok(wav) = WavReader::open(Path::new(path)) else {
                        break; // Its header is not on disk yet.
                    };
                    let mut wav = wav.parts_of(up.plan.part_seconds);
                    let complete = if closed {
                        wav.frames.div_ceil(wav.step)
                    } else {
                        wav.frames / wav.step
                    };
                    let block = next.entry((channel, path.clone())).or_insert(0);
                    while *block < complete {
                        if stopped() {
                            return Ok(());
                        }
                        let Some((bytes, start_ms)) = wav.chunk(*block)? else {
                            break;
                        };
                        let part = up.client.send_part(
                            &target,
                            &up.existing,
                            channel,
                            path,
                            *block,
                            bytes,
                            channel_offset + start_ms,
                            &mut up.sequence,
                            Some(cancel.clone()),
                        )?;
                        sent.push(SentPart {
                            id: part,
                            due: Instant::now() + FIRST_PART_CHECK,
                            wait: FIRST_PART_CHECK,
                        });
                        *block += 1;
                    }
                    if !closed {
                        break;
                    }
                    channel_offset += wav.duration_ms();
                }
            }
            // Has the server finished any of them? It may hold a part back
            // until the live stream has had its say, so ask less and less
            // often: after 4, 8, 16 and then every 30 seconds.
            let now = Instant::now();
            if sent.iter().any(|part| now >= part.due) {
                let states: Vec<AudioChunkDTO> =
                    up.client
                        .request_with_cancel("GET", &chunks, None, Some(cancel.clone()))?;
                let mut landed = false;
                sent.retain_mut(|part| {
                    match states.iter().find(|chunk| chunk.id == part.id).map(|chunk| &chunk.state) {
                        Some(State::Ready) => {
                            landed = true;
                            false
                        }
                        // Left for the upload after Stop, which retries it
                        // and reports it if it fails again.
                        Some(State::Failed) => false,
                        _ => {
                            if now >= part.due {
                                part.wait = (part.wait * 2).min(Duration::from_secs(30));
                                part.due = now + part.wait;
                            }
                            true
                        }
                    }
                });
                if landed {
                    let remote: NoteDTO = up.client.request_with_cancel(
                        "GET",
                        &format!("/api/v1/notes/{id}"),
                        None,
                        Some(cancel.clone()),
                    )?;
                    if revision != Some(remote.transcript_revision) {
                        revision = Some(remote.transcript_revision);
                        let _ = events.send(PartEvent::Transcript {
                            revision: remote.transcript_revision.max(0) as u64,
                            segments: remote.transcript_segments,
                        });
                    }
                }
            }
            Ok(())
        };
        match pass() {
            Ok(()) => failures = 0,
            Err(_) if stopped() => break,
            Err(_) => {
                // The network, most likely. Look at the server afresh when
                // it is back: after 1, 2, 4, 8, then every 15 seconds.
                uploading = None;
                failures += 1;
                let delay = [1, 2, 4, 8, 15][(failures as usize - 1).min(4)];
                retry_at = Instant::now() + Duration::from_secs(delay);
            }
        }
        if manifest.completed() {
            break; // Everything left is the after-Stop upload's.
        }
    }
}

impl Client {
    /// The plan of capture `capture_id` while it is still being recorded:
    /// the captures before it are placed first, exactly as the loop of
    /// [`Client::upload`] will place them, and then this one after them.
    fn live_capture_plan(
        &self,
        note: &Note,
        id: &str,
        capture_id: &str,
        existing: &HashMap<String, AudioChunkDTO>,
    ) -> Result<CapturePlan, String> {
        let mut offset_ms = existing
            .values()
            .map(|c| (c.start_ms + c.duration_ms).max(0) as u64)
            .max()
            .unwrap_or(0);
        let mapping_root = self.mapping_root(note, id)?;
        for saved in &note.audio_captures {
            let live = saved.capture_id == capture_id;
            let capture = if live {
                saved.clone()
            } else {
                let current = fs::read(&saved.manifest_path)
                    .ok()
                    .and_then(|data| {
                        serde_json::from_slice::<crate::audio::CaptureManifest>(&data).ok()
                    })
                    .unwrap_or_else(|| saved.clone());
                if !current.completed() {
                    return Err("An earlier capture is still being finalised".into());
                }
                current
            };
            let plan = self.capture_plan(id, &capture, existing, &mapping_root, offset_ms, live)?;
            if live {
                return Ok(plan);
            }
            offset_ms =
                offset_ms.max(plan.base_ms + (capture.audio_seconds() * 1000.).round() as u64);
        }
        Err("The capture does not belong to this note".into())
    }

    /// Note-timeline position of the first sample of capture `capture_id`,
    /// while it is still being recorded.
    fn live_capture_base(&self, note: &Note, id: &str, capture_id: &str) -> Result<u64, String> {
        let existing: Vec<AudioChunkDTO> =
            self.request("GET", &format!("/api/v1/notes/{id}/audio/chunks"), None)?;
        let existing: HashMap<_, _> = existing.into_iter().map(|c| (c.id.clone(), c)).collect();
        Ok(self
            .live_capture_plan(note, id, capture_id, &existing)?
            .base_ms)
    }
}

fn channel_paths(stats: &crate::audio::ChannelStats) -> Vec<String> {
    if stats.paths.is_empty() {
        stats.path.clone().into_iter().collect()
    } else {
        stats.paths.clone()
    }
}
fn chunk_identity(
    note_id: &str,
    capture_id: &str,
    channel: &str,
    path: &str,
    block: u64,
) -> String {
    uuid::Uuid::new_v5(
        &uuid::Uuid::NAMESPACE_URL,
        format!("{note_id}:{capture_id}:{channel}:{path}:{block}").as_bytes(),
    )
    .to_string()
}
fn save_mapping<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let bytes = serde_json::to_vec(value).map_err(|_| "Cannot encode upload identity")?;
    if let Ok(previous) = fs::read(path) {
        return if previous == bytes {
            Ok(())
        } else {
            Err(
                "Saved upload identity conflicts with the server; local audio remains intact"
                    .into(),
            )
        };
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "Cannot determine upload timestamp")?
        .as_nanos();
    let temporary = path.with_extension(format!("{}.{}.tmp", std::process::id(), stamp));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = match options.open(&temporary) {
        Ok(mut file) => {
            file.write_all(&bytes)
                .and_then(|_| file.sync_all())
                .map_err(|_| "Cannot preserve upload identity")?;
            match fs::hard_link(&temporary, path) {
                Ok(()) => Ok(()),
                Err(e)
                    if e.kind() == std::io::ErrorKind::AlreadyExists
                        && fs::read(path).ok().as_deref() == Some(bytes.as_slice()) =>
                {
                    Ok(())
                }
                Err(_) => Err("Cannot preserve immutable upload identity".into()),
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            if fs::read(path).map_err(|_| "Cannot read upload identity")? == bytes {
                Ok(())
            } else {
                Err(
                    "Saved upload identity conflicts with the server; local audio remains intact"
                        .into(),
                )
            }
        }
        Err(_) => Err("Cannot preserve upload identity".into()),
    };
    let _ = fs::remove_file(temporary);
    result
}

/// Convert bounded 60-second ranges to PCM16 WAV. Reading a multi-hour capture
/// never loads the whole file or silently truncates at an API upload limit.
struct WavReader {
    file: fs::File,
    rate: u32,
    format: u16,
    bits: u16,
    data_start: u64,
    frames: u64,
    position: u64,
    /// Frames in one part.
    step: u64,
}
impl WavReader {
    /// Cut into parts of `seconds` instead of the 60 seconds of old.
    fn parts_of(mut self, seconds: u64) -> Self {
        // Stay below curl's configuration-line limit after base64 expansion,
        // as well as the API's raw audio limit, even at 192 kHz.
        self.step = (self.rate as u64 * seconds.max(1)).min(3_000_000);
        self
    }
    /// Part `block` on its own, wherever the reader was.
    fn chunk(&mut self, block: u64) -> Result<Option<(Vec<u8>, u64)>, String> {
        self.position = (block * self.step).min(self.frames);
        self.next_chunk()
    }
    fn open(path: &Path) -> Result<Self, String> {
        let mut file = fs::File::open(path).map_err(|_| "Saved audio file is unavailable")?;
        let mut header = [0u8; 12];
        file.read_exact(&mut header)
            .map_err(|_| "Invalid WAV header")?;
        if &header[..4] != b"RIFF" || &header[8..] != b"WAVE" {
            return Err("Unsupported saved audio container".into());
        }
        let length = file
            .metadata()
            .map_err(|_| "Cannot inspect saved audio")?
            .len();
        let mut format = None;
        let mut data = None;
        while file.stream_position().map_err(|_| "Cannot read WAV")? + 8 <= length {
            let mut h = [0u8; 8];
            file.read_exact(&mut h).map_err(|_| "Invalid WAV chunk")?;
            let size = u32::from_le_bytes(h[4..8].try_into().unwrap()) as u64;
            let start = file.stream_position().map_err(|_| "Cannot read WAV")?;
            if start + size > length {
                return Err("Saved WAV has incomplete audio; wait for finalization".into());
            }
            if &h[..4] == b"fmt " {
                if size < 16 {
                    return Err("Invalid WAV format".into());
                }
                let mut f = [0u8; 16];
                file.read_exact(&mut f).map_err(|_| "Invalid WAV format")?;
                format = Some((
                    u16::from_le_bytes(f[..2].try_into().unwrap()),
                    u16::from_le_bytes(f[2..4].try_into().unwrap()),
                    u32::from_le_bytes(f[4..8].try_into().unwrap()),
                    u16::from_le_bytes(f[14..16].try_into().unwrap()),
                ));
            } else if &h[..4] == b"data" {
                data = Some((start, size));
            }
            file.seek(SeekFrom::Start(start + size + (size % 2)))
                .map_err(|_| "Cannot seek saved audio")?;
        }
        let (code, channels, rate, bits) = format.ok_or("Missing WAV format")?;
        let (data_start, bytes) = data.ok_or("Missing WAV samples")?;
        if channels != 1
            || !(8000..=192000).contains(&rate)
            || !matches!((code, bits), (1, 16) | (3, 32))
            || bytes % (bits as u64 / 8) != 0
        {
            return Err("Unsupported saved WAV encoding".into());
        }
        Ok(Self {
            file,
            rate,
            format: code,
            bits,
            data_start,
            frames: bytes / (bits as u64 / 8),
            position: 0,
            step: (rate as u64 * LEGACY_PART_SECONDS).min(3_000_000),
        })
    }
    fn duration_ms(&self) -> u64 {
        self.frames * 1000 / self.rate as u64
    }
    fn next_chunk(&mut self) -> Result<Option<(Vec<u8>, u64)>, String> {
        if self.position == self.frames {
            return Ok(None);
        }
        let count = (self.frames - self.position).min(self.step);
        let start_ms = self.position * 1000 / self.rate as u64;
        self.file
            .seek(SeekFrom::Start(
                self.data_start + self.position * (self.bits as u64 / 8),
            ))
            .map_err(|_| "Cannot seek saved audio")?;
        let mut source = vec![0u8; count as usize * (self.bits as usize / 8)];
        self.file
            .read_exact(&mut source)
            .map_err(|_| "Cannot read saved audio")?;
        let pcm: Vec<u8> = if self.format == 1 {
            source
        } else {
            source
                .chunks_exact(4)
                .flat_map(|v| {
                    let value = f32::from_le_bytes(v.try_into().unwrap());
                    let sample = if value.is_finite() {
                        (value.clamp(-1., 1.) * 32767.).round() as i16
                    } else {
                        0
                    };
                    sample.to_le_bytes()
                })
                .collect()
        };
        let mut bytes = Vec::with_capacity(44 + pcm.len());
        bytes.extend(b"RIFF");
        bytes.extend((36 + pcm.len() as u32).to_le_bytes());
        bytes.extend(b"WAVEfmt ");
        bytes.extend(16u32.to_le_bytes());
        bytes.extend(1u16.to_le_bytes());
        bytes.extend(1u16.to_le_bytes());
        bytes.extend(self.rate.to_le_bytes());
        bytes.extend((self.rate * 2).to_le_bytes());
        bytes.extend(2u16.to_le_bytes());
        bytes.extend(16u16.to_le_bytes());
        bytes.extend(b"data");
        bytes.extend((pcm.len() as u32).to_le_bytes());
        bytes.extend(pcm);
        self.position += count;
        Ok(Some((bytes, start_ms)))
    }
}

/// Whole notes handed to the window at a time while the list refreshes.
const SYNC_BATCH: usize = 16;
/// Rows asked for per list request: the most the server gives.
const SYNC_PAGE: usize = 100;

/// What the list refresh sends to the window while it works.
enum SyncMessage {
    Notes {
        url: String,
        notes: Vec<NoteDTO>,
        /// Whole notes read so far, of those that needed reading.
        read: usize,
        wanted: usize,
    },
    /// The ids the listing held. A note marked "Deleted on the server" that
    /// is listed again loses the mark.
    Listed(std::collections::HashSet<String>),
    /// Notes the server answered 404 for, asked one by one after a complete
    /// listing that did not hold them.
    Missing(Vec<String>),
    Finished(Result<Vec<meeting_notes_api_types::LanguageModelSelection>, String>),
}

/// What tells the copy of a note kept here from the server's current one: any
/// edit moves the note's last change, and speech or generation move a revision.
fn sync_stamp(updated_at: &str, transcript_revision: i64, generated_revision: Option<i64>) -> String {
    format!(
        "{updated_at}|{transcript_revision}|{}",
        generated_revision.map_or_else(|| "-".to_string(), |revision| revision.to_string())
    )
}

/// The notes to read whole: those that are new here, and those whose row on the
/// server no longer matches the copy kept. `kept` maps a server note ID to its stamp.
fn notes_to_read(
    rows: &[meeting_notes_api_types::NoteSummaryDTO],
    kept: &HashMap<String, String>,
) -> Vec<String> {
    rows.iter()
        .filter(|row| {
            kept.get(&row.id).map(String::as_str)
                != Some(
                    sync_stamp(&row.updated_at, row.transcript_revision, row.generated_revision)
                        .as_str(),
                )
        })
        .map(|row| row.id.clone())
        .collect()
}

/// An account's notes list as the server gave it.
struct Listing {
    rows: Vec<meeting_notes_api_types::NoteSummaryDTO>,
    /// Every page was read **and no row was skipped**. A row whose id is not
    /// a UUID is dropped, and makes the listing incomplete: nothing may be
    /// concluded from a note's absence in it.
    complete: bool,
}

/// The most notes one refresh asks for one by one because a complete
/// listing did not hold them. The rest wait for the next refresh.
const MISSING_CHECKS: usize = 500;

/// What one refresh found out about notes that are gone from the server.
#[derive(Debug, Default, PartialEq, Eq)]
struct Gone {
    /// The server answered 404 for the note itself.
    missing: Vec<String>,
}

/// Refresh the notes kept here from the server. The list comes as rows without
/// text, a page at a time; a note is read whole only when it is new or has
/// changed, so a refresh with nothing new downloads no note at all and no
/// response comes near the transport's size limit. (The old request returned
/// every note with its transcript in one response, which that limit refuses
/// once a history passes 8 MB.)
///
/// A server from before the list endpoint refuses the first request; it is then
/// asked for every note at once, as before. If the server cannot be reached,
/// that request fails too and its error is the one reported.
///
/// Notes that are gone from the server (`candidates`: the cached notes of
/// this account that are not open and have nothing unsent). After a
/// **complete** listing, each candidate that is not in it is asked for
/// directly, and only a 404 from that request says it is gone. A short
/// listing, a skipped row, a failed request or any other answer concludes
/// nothing. Nothing is ever deleted here.
fn refresh_notes(
    client: &Client,
    kept: &HashMap<String, String>,
    candidates: &[String],
    mut deliver: impl FnMut(Vec<NoteDTO>, usize, usize),
    mut listed: impl FnMut(std::collections::HashSet<String>),
) -> Result<Gone, String> {
    let Ok(listing) = client.note_rows() else {
        let notes: Vec<NoteDTO> = client.request("GET", "/api/v1/notes", None)?;
        let total = notes.len();
        listed(notes.iter().map(|note| note.id.to_ascii_lowercase()).collect());
        deliver(notes, total, total);
        return Ok(Gone::default());
    };
    let rows = listing.rows;
    let present: std::collections::HashSet<String> =
        rows.iter().map(|row| row.id.to_ascii_lowercase()).collect();
    listed(present.clone());
    let wanted = notes_to_read(&rows, kept);
    let mut read = 0;
    for ids in wanted.chunks(SYNC_BATCH) {
        let mut notes = Vec::with_capacity(ids.len());
        for id in ids {
            match client.send::<NoteDTO>("GET", &format!("/api/v1/notes/{id}"), None, None) {
                Ok(note) => notes.push(note),
                // Deleted on the server since its row was listed.
                Err(error) if error.is_not_found() => {}
                Err(error) => {
                    // What was read before the failure is still worth keeping.
                    if !notes.is_empty() {
                        read += notes.len();
                        deliver(notes, read, wanted.len());
                    }
                    return Err(error.message());
                }
            }
        }
        read += ids.len();
        deliver(notes, read, wanted.len());
    }
    let mut gone = Gone::default();
    if !listing.complete {
        return Ok(gone);
    }
    let mut found = Vec::new();
    for id in candidates
        .iter()
        .filter(|id| !present.contains(&id.to_ascii_lowercase()) && uuid::Uuid::parse_str(id).is_ok())
        .take(MISSING_CHECKS)
    {
        match client.send::<NoteDTO>("GET", &format!("/api/v1/notes/{id}"), None, None) {
            // Only this answer says the note is gone.
            Err(error) if error.is_not_found() => gone.missing.push(id.to_ascii_lowercase()),
            // It is there after all: the listing was short.
            Ok(note) => found.push(note),
            // No answer, a 5xx, a 403, an HTML page: nothing is concluded.
            Err(_) => {}
        }
    }
    if !found.is_empty() {
        let total = found.len();
        deliver(found, total, total);
    }
    Ok(gone)
}

impl Client {
    /// Every row of the account's notes list, newest meeting first. IDs and
    /// cursors go into request paths, so anything that is not one is refused.
    fn note_rows(&self) -> Result<Listing, String> {
        let mut rows: Vec<meeting_notes_api_types::NoteSummaryDTO> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut cursor: Option<String> = None;
        let mut complete = true;
        // Far more pages than any account has; a server that never ends its list cannot hold the refresh forever.
        for _ in 0..10_000 {
            let path = match &cursor {
                Some(cursor) => format!("/api/v1/notes/summaries?limit={SYNC_PAGE}&cursor={cursor}"),
                None => format!("/api/v1/notes/summaries?limit={SYNC_PAGE}"),
            };
            let page: meeting_notes_api_types::NoteSummariesPageDTO =
                self.request("GET", &path, None)?;
            for row in page.notes {
                if uuid::Uuid::parse_str(&row.id).is_err() {
                    // Dropped, and not silently: the listing no longer
                    // says which notes exist.
                    complete = false;
                } else if seen.insert(row.id.clone()) {
                    rows.push(row);
                }
            }
            match page.next_cursor {
                None => return Ok(Listing { rows, complete }),
                Some(next)
                    if !next.is_empty()
                        && next.len() <= 512
                        && next
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_') =>
                {
                    cursor = Some(next)
                }
                Some(_) => return Err("Server response does not match the shared API contract".into()),
            }
        }
        Err("The server's notes list did not end".into())
    }
}

/// Where the association of a note with its server note is kept, under its
/// account's `recordings` directory. Identifiers only, never credentials.
pub fn association_file(recordings: &Path, url: &str, note_id: u64) -> std::path::PathBuf {
    let namespace = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, url.as_bytes());
    recordings
        .join("server-associations")
        .join(namespace.to_string())
        .join(format!("{}.txt", handle::file_id(note_id)))
}

/// What is queued again for a note when its account is Ready: the capture
/// (which saves the text on its way) or the edit.
///
/// A capture is sent again only when the account comes back (`returning`:
/// its session ended, was replaced, or its server could not be reached), and
/// only once every capture of the note is final; an edit is always sent.
pub fn requeue_action(note: &Note, returning: bool) -> Option<Action> {
    if note.trashed && note.server_missing_since.is_some() {
        return None;
    }
    let captures_final = !note.audio_captures.is_empty()
        && note.audio_captures.iter().all(|capture| {
            std::fs::read(&capture.manifest_path)
                .ok()
                .and_then(|data| serde_json::from_slice::<crate::audio::CaptureManifest>(&data).ok())
                .unwrap_or_else(|| capture.clone())
                .completed()
        });
    if returning && note.unsent_capture && captures_final {
        Some(Action::Sync)
    } else if note.unsent_edits || note.enhanced_dirty {
        Some(Action::Save)
    } else {
        None
    }
}

/// Put a note in Trash as "Deleted on the server": its own server answered
/// 404 for it. Its text, folder, favourite and capture files stay. A note
/// that is open, or has anything unsent, is left alone.
pub fn mark_missing(note: &mut Note, now: &str) -> bool {
    if note.has_unsent_work() || note.server_missing_since.is_some() {
        return false;
    }
    note.server_missing_since = Some(now.to_owned());
    note.trashed = true;
    true
}

/// A note marked "Deleted on the server" is listed again: both marks go.
pub fn mark_listed(note: &mut Note) -> bool {
    if note.server_missing_since.take().is_some() {
        note.trashed = false;
        return true;
    }
    false
}

/// Restore a "Deleted on the server" note from Trash: it becomes a note of
/// this Mac that is created again on the server at the next save. Returns
/// the address it was on, for the sidecar to be removed.
pub fn detach_from_server(note: &mut Note) -> Option<String> {
    note.server_missing_since.take()?;
    note.trashed = false;
    note.server_id = None;
    note.server_sync_stamp = None;
    note.unsent_edits = true;
    note.server_url.take()
}

impl Cereal {
    /// Server-backed notes use their own workspace. Loading a remote list never
    /// imports or rewrites the legacy local demo workspace.
    ///
    /// Each account is verified on its own, and each account that is Ready
    /// refreshes its own slice of the workspace with its own client.
    pub fn connect_server(&mut self, cx: &mut Context<Self>) {
        if !enabled() {
            return;
        }
        self.verify_accounts(cx);
        let ready: Vec<_> = crate::accounts::all()
            .into_iter()
            .filter(|account| account.is_ready() && account.verified_this_launch)
            .collect();
        if ready.is_empty() {
            return;
        }
        self.load_calendar_events(cx);
        if self.server_pending > 0 {
            return;
        }
        for account in ready {
            self.refresh_account(&account, cx);
        }
    }

    /// Refresh one account's notes from its own server.
    fn refresh_account(&mut self, account: &crate::accounts::Account, cx: &mut Context<Self>) {
        if !self.refreshing.insert(account.id.clone()) {
            return;
        }
        let Some(slot_loaded) = self
            .workspace
            .slots
            .get(&account.slot)
            .map(|slot| slot.state == crate::model::SlotState::Loaded)
        else {
            self.refreshing.remove(&account.id);
            return;
        };
        if !slot_loaded {
            // Its file was refused: nothing is read into it or written.
            self.refreshing.remove(&account.id);
            return;
        }
        let account_epoch = account.epoch();
        let (id, slot, url) = (account.id.clone(), account.slot, account.url.clone());
        self.server_pending += 1;
        self.server_status = Some("Loading server notes…".into());
        // What is already here, so the worker reads whole only the notes that are new or have changed.
        let mut kept = HashMap::new();
        let mut candidates = Vec::new();
        let recording = self.recording.note_id.filter(|_| self.recording.is_active());
        for note in self.workspace.notes_of(slot) {
            let (Some(server_id), Some(server_url)) = (&note.server_id, &note.server_url) else {
                continue;
            };
            if *server_url != url {
                continue;
            }
            if let Some(stamp) = &note.server_sync_stamp {
                kept.insert(server_id.clone(), stamp.clone());
            }
            if self.current_note != Some(note.id)
                && recording != Some(note.id)
                && !note.has_unsent_work()
                && note.server_missing_since.is_none()
            {
                candidates.push(server_id.clone());
            }
        }
        // The chat models are those of the account cross-meeting chat asks.
        let wants_models = crate::accounts::default_account().is_some_and(|default| default.id == id);
        let (sender, receiver) = mpsc::channel();
        let account_id = id.clone();
        std::thread::spawn(move || {
            let result = crate::accounts::client(&account_id).and_then(|client| {
                let gone = refresh_notes(
                    &client,
                    &kept,
                    &candidates,
                    |notes, read, wanted| {
                        let _ = sender.send(SyncMessage::Notes {
                            url: client.url.clone(),
                            notes,
                            read,
                            wanted,
                        });
                    },
                    |present| {
                        let _ = sender.send(SyncMessage::Listed(present));
                    },
                )?;
                if !gone.missing.is_empty() {
                    let _ = sender.send(SyncMessage::Missing(gone.missing));
                }
                let models = if wants_models {
                    client.request::<meeting_notes_api_types::ChatModelsDTO>("GET", "/api/v1/models/chat", None).map(|response| response.models).unwrap_or_default()
                } else {
                    Vec::new()
                };
                Ok(models)
            });
            let _ = sender.send(SyncMessage::Finished(result));
        });
        cx.spawn(async move |this, cx| {
            loop {
                let message = match receiver.try_recv() {
                    Ok(message) => message,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        SyncMessage::Finished(Err("Server connection worker stopped".into()))
                    }
                    Err(mpsc::TryRecvError::Empty) => {
                        cx.background_executor()
                            .timer(Duration::from_millis(100))
                            .await;
                        continue;
                    }
                };
                let finished = matches!(message, SyncMessage::Finished(_));
                let _ = this.update(cx, |this, cx| {
                    if finished {
                        this.refreshing.remove(&id);
                        this.server_pending = this.server_pending.saturating_sub(1);
                    }
                    if crate::accounts::epoch_of(&id) != Some(account_epoch) {
                        // The account was signed out of, or its session was
                        // replaced or ended: what was on its way is dropped.
                        if finished && this.server_pending == 0 {
                            this.server_status = None;
                        }
                        return;
                    }
                    match message {
                        // Notes arrive a few at a time, so a first sync of a long history fills the list as it goes.
                        SyncMessage::Notes {
                            url,
                            notes,
                            read,
                            wanted,
                        } => {
                            for remote in notes {
                                this.apply_server_note(remote, slot, &url);
                            }
                            if read < wanted {
                                this.server_status =
                                    Some(format!("Loading server notes… {read} of {wanted}"));
                            }
                            cx.notify();
                        }
                        SyncMessage::Listed(present) => {
                            let relisted: Vec<u64> = this
                                .workspace
                                .notes_of(slot)
                                .filter(|note| {
                                    note.server_missing_since.is_some()
                                        && note.server_url.as_deref() == Some(url.as_str())
                                        && note.server_id.as_ref().is_some_and(|id| present.contains(&id.to_ascii_lowercase()))
                                })
                                .map(|note| note.id)
                                .collect();
                            for note_id in relisted {
                                if let Some(note) = this.workspace.note_mut(note_id) {
                                    mark_listed(note);
                                }
                            }
                        }
                        SyncMessage::Missing(missing) => {
                            let now = chrono::Utc::now().to_rfc3339();
                            let recording = this.recording.note_id.filter(|_| this.recording.is_active());
                            let gone: Vec<u64> = this
                                .workspace
                                .notes_of(slot)
                                .filter(|note| {
                                    note.server_url.as_deref() == Some(url.as_str())
                                        && note.server_id.as_ref().is_some_and(|id| missing.contains(&id.to_ascii_lowercase()))
                                        && this.current_note != Some(note.id)
                                        && recording != Some(note.id)
                                })
                                .map(|note| note.id)
                                .collect();
                            for note_id in gone {
                                if let Some(note) = this.workspace.note_mut(note_id) {
                                    mark_missing(note, &now);
                                }
                            }
                            cx.notify();
                        }
                        SyncMessage::Finished(result) => {
                            match result {
                                Ok(models) => {
                                    if wants_models {
                                        this.chat_models = models;
                                        if !this.chat_models.iter().any(|choice| Some(chat_model_key(choice)) == this.selected_chat_model) { this.selected_chat_model = None; }
                                    }
                                    if this.server_pending == 0 {
                                        this.server_status = None;
                                    }
                                }
                                Err(error) => {
                                    this.server_status = Some(error.clone());
                                    this.show_toast(&error, cx);
                                }
                            }
                            // Also after a failure: the notes read before it are kept, and the next refresh continues from them.
                            this.persist(cx);
                        }
                    }
                });
                if finished {
                    break;
                }
            }
        })
        .detach();
    }

    /// Open the note that `account`'s server calls `server_id` (a link to it
    /// was followed). One that has not been listed here yet is read from
    /// that server first, with that account's client; one the server does
    /// not have for this account says so.
    pub fn open_server_note(&mut self, account: AccountId, server_id: String, cx: &mut Context<Self>) {
        let Some(account) = crate::accounts::get(&account) else {
            return;
        };
        let (slot, url) = (account.slot, account.url.clone());
        if let Some(id) = cached_note(&self.workspace, slot, &server_id, &url) {
            self.open_note(id, cx);
            return;
        }
        let Ok(path_id) = crate::accounts::client::path_id(&server_id) else {
            return;
        };
        let account_epoch = account.epoch();
        let account_id = account.id.clone();
        cx.spawn(async move |this, cx| {
            // Asked of the note's own server, with that account's session.
            let asked = account_id.clone();
            let reply = cx
                .background_executor()
                .spawn(async move {
                    crate::accounts::client(&asked)
                        .map_err(|_| crate::fetch::Failure::Transport)?
                        .fetch(&format!("/api/v1/notes/{path_id}"), 8 * 1024 * 1024)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if crate::accounts::epoch_of(&account_id) != Some(account_epoch) {
                    return;
                }
                let remote = match reply {
                    Ok(reply) if reply.status == 200 => {
                        serde_json::from_slice::<NoteDTO>(&reply.body).ok()
                    }
                    Ok(reply) if matches!(reply.status, 403 | 404 | 410) => None,
                    Ok(reply) => {
                        let status = reply.status;
                        this.show_toast(&format!("Server rejected the request (HTTP {status})"), cx);
                        return;
                    }
                    Err(_) => {
                        this.show_toast("Could not reach this server", cx);
                        return;
                    }
                };
                match remote.filter(|remote| remote.id.eq_ignore_ascii_case(&server_id)) {
                    Some(remote) => {
                        let server_id = remote.id.clone();
                        this.apply_server_note(remote, slot, &url);
                        this.persist(cx);
                        if let Some(id) = cached_note(&this.workspace, slot, &server_id, &url) {
                            this.open_note(id, cx);
                        }
                    }
                    // Not a note of this account, or not a note any more.
                    None => this.show_toast("This note is no longer available.", cx),
                }
            });
        })
        .detach();
    }

    /// Bring the copy of a server note kept here up to date, creating it in
    /// its account's slot when it is new. The note open for editing is left
    /// as it is, and text that has not been sent yet is not replaced.
    fn apply_server_note(&mut self, remote: NoteDTO, slot: u16, url: &str) {
        let Some(id) = cached_note(&self.workspace, slot, &remote.id, url)
            .or_else(|| self.workspace.create_note_in(slot))
        else {
            return;
        };
        if self.current_note == Some(id) {
            return;
        }
        if let Some(note) = self.workspace.note_mut(id) {
            note.server_sync_stamp = Some(sync_stamp(
                &remote.updated_at,
                remote.transcript_revision,
                remote.generated_revision,
            ));
            // It is on the server: a "Deleted on the server" mark goes.
            mark_listed(note);
            if !note.unsent_edits {
                note.title = remote.title.clone();
                note.body = remote.body.clone();
            }
            if !note.enhanced_dirty {
                note.enhanced = remote.enhanced.clone();
            }
            note.transcript = transcript_text(&remote);
            note.server_transcript_segments = remote.transcript_segments.clone();
            note.transcript_revision = remote.transcript_revision.max(0) as u64;
            note.generated_revision = remote.generated_revision.map(|v| v.max(0) as u64);
            note.date = remote.date;
            note.duration = remote.duration;
            note.time = display_created_at(&remote.created_at);
            note.server_created_at = Some(remote.created_at.clone());
            note.server_id = Some(remote.id);
            note.server_url = Some(url.to_string());
        }
    }

    /// Every note of an account with an unsent capture or edit is queued
    /// again: the account has just become Ready (its session was verified,
    /// replaced, or signed in to again).
    pub fn requeue_unsent(
        &mut self,
        account: &crate::accounts::Account,
        returning: bool,
        cx: &mut Context<Self>,
    ) {
        let recording = self.recording.note_id.filter(|_| self.recording.is_active());
        let queue: Vec<(u64, Action)> = self
            .workspace
            .notes_of(account.slot)
            .filter(|note| recording != Some(note.id) && self.current_note != Some(note.id))
            .filter_map(|note| Some((note.id, requeue_action(note, returning)?)))
            .collect();
        for (id, action) in queue {
            self.server_action(id, action, cx);
        }
    }

    /// Send something about a note to the account it belongs to, through
    /// that account's serial worker. Nothing falls back to another account.
    pub fn server_action(&mut self, id: u64, action: Action, cx: &mut Context<Self>) {
        if !enabled() {
            return;
        }
        let Some(account) = crate::accounts::of_note(id) else {
            return;
        };
        if !account.shown() {
            // Its session has ended (or was never verified in this launch):
            // nothing is sent. What is unsent stays marked on the note and
            // is queued again after the sign-in.
            return;
        }
        let account_epoch = account.epoch();
        let account_id = account.id.clone();
        let Some(note) = self.workspace.note(id).cloned() else {
            return;
        };
        let generating = matches!(&action, Action::Generate);
        let chat = matches!(&action, Action::Chat { .. });
        // An action that ends with every part of the note's audio sent.
        let uploads = matches!(
            &action,
            Action::Recording("stop") | Action::Sync | Action::Generate
        );
        let chat_index = self.chat_messages.len().checked_sub(1);
        let epoch = if generating {
            Some(self.generation.begin(id, note.transcript_revision))
        } else {
            None
        };
        let submitted_enhanced = note.enhanced.clone();
        let submitted_enhanced_dirty = note.enhanced_dirty;
        let submitted_title = note.title.clone();
        let submitted_body = note.body.clone();
        let receiver = self
            .bridges
            .entry(account_id.clone())
            .or_insert_with(|| Bridge::new(account_id.clone()))
            .send(note, action);
        self.server_pending += 1;
        self.server_status = Some(
            if generating {
                "Generating notes on server…"
            } else if chat {
                "Thinking…"
            } else {
                "Syncing audio and transcript…"
            }
            .into(),
        );
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = loop {
                match receiver.try_recv() {
                    Ok(result) => break result,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        break Err("Server worker stopped. Local audio is preserved.".into());
                    }
                    Err(mpsc::TryRecvError::Empty) => {
                        cx.background_executor()
                            .timer(Duration::from_millis(100))
                            .await
                    }
                }
            };
            let _ = this.update(cx, |this, cx| {
                this.server_pending = this.server_pending.saturating_sub(1);
                if crate::accounts::epoch_of(&account_id) != Some(account_epoch) {
                    // The account's session was replaced or ended, or it
                    // was signed out of. The note keeps its unsent marks
                    // and is queued again when the account is Ready.
                    if this.server_pending == 0 {
                        this.server_status = None;
                    }
                    if generating && epoch == Some(this.generation.epoch) {
                        this.generation.cancel();
                    }
                    cx.notify();
                    return;
                }
                match result {
                    Ok(reply) => {
                        if let Some(url) = reply.share_url {
                            cx.write_to_clipboard(ClipboardItem::new_string(url));
                            this.show_toast(
                                "Published snapshot link copied. The transcript remains private.",
                                cx,
                            );
                        }
                        let live_note = this.live.streams(id);
                        let recording_now =
                            this.recording.note_id == Some(id) && this.recording.is_active();
                        let mut named = None;
                        if let Some(note) = this.workspace.note_mut(id) {
                            note.server_id = Some(reply.note.id.clone());
                            note.server_url = Some(reply.url);
                            // The server has what was sent: what is unsent
                            // now is only what changed since.
                            if note.title == submitted_title && note.body == submitted_body {
                                note.unsent_edits = false;
                            }
                            if uploads && !recording_now {
                                note.unsent_capture = false;
                            }
                            // The server names a note nobody has named when
                            // its notes are generated: the list row and the
                            // title field show that name at once, and later
                            // saves send it rather than the placeholder.
                            if note.adopt_server_title(&submitted_title, &reply.note.title) {
                                named = Some(note.title.clone());
                            }
                            if this.recording.note_id != Some(id) || !this.recording.is_active() {
                                note.duration = reply.note.duration.clone();
                            }
                            // While this note is being transcribed live, text
                            // arrives on the stream; a reply that was read
                            // before the newest of it must not take it away.
                            let stale = live_note
                                && (reply.note.transcript_revision.max(0) as u64)
                                    < note.transcript_revision;
                            if !stale
                                && (!reply.note.transcript_segments.is_empty()
                                    || note.transcript.is_empty())
                            {
                                note.transcript = transcript_text(&reply.note);
                                note.server_transcript_segments =
                                    reply.note.transcript_segments.clone();
                                note.transcript_revision =
                                    reply.note.transcript_revision.max(0) as u64;
                            }
                            // A generation canceled by navigation/capture cannot replace
                            // the current editing surface. Server keeps its own guard.
                            note.reconcile_enhanced_reply(
                                &submitted_enhanced,
                                submitted_enhanced_dirty,
                                &reply.note.enhanced,
                                reply.note.generated_revision.map(|v| v.max(0) as u64),
                                !generating || epoch == Some(this.generation.epoch),
                            );
                        }
                        if let Some(title) = named
                            && this.current_note == Some(id)
                        {
                            this.title_input
                                .update(cx, |input, cx| input.set_content(title, cx));
                        }
                        // The server has the note in the recording state:
                        // the live transcript stream can connect.
                        if reply.note.recording_state
                            == meeting_notes_api_types::RecordingState::Recording
                        {
                            this.live_connect(id, cx);
                        }
                        if let Some(answer) = reply.answer
                            && this.current_note == Some(id)
                            && let Some((_, response)) =
                                chat_index.and_then(|i| this.chat_messages.get_mut(i))
                        {
                            *response = answer;
                        }
                        if let Some(epoch) = epoch
                            && this.generation.epoch == epoch
                        {
                            this.generation.writing(epoch);
                            this.generation.phase = crate::recording::GenerationPhase::Ready;
                        }
                        this.server_status = if this.server_pending == 0 {
                            None
                        } else {
                            Some("Syncing with server…".into())
                        };
                        this.persist(cx);
                        if this.current_note == Some(id) {
                            this.refresh_meeting_context(id, cx);
                        }
                    }
                    Err(error) => {
                        if generating && epoch == Some(this.generation.epoch) {
                            this.generation.cancel();
                        }
                        if chat
                            && this.current_note == Some(id)
                            && let Some((_, answer)) =
                                chat_index.and_then(|i| this.chat_messages.get_mut(i))
                        {
                            *answer = format!("Could not complete the request: {error}");
                        }
                        this.server_status = Some(error.clone());
                        this.show_toast(&error, cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
    pub fn submit_server_chat(&mut self, question: String, cx: &mut Context<Self>) {
        self.start_streaming_chat(question, cx);
    }
}

fn transcript_text(note: &NoteDTO) -> String {
    crate::note::server_transcript_utterances(&note.transcript_segments)
        .iter()
        .map(|s| {
            let label = s.speaker.as_deref().unwrap_or("Unlabeled speaker");
            format!(
                "{label}  {}\n{}",
                s.timestamp.as_deref().unwrap_or("00:00"),
                s.text
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// The pieces of this module that a test elsewhere needs to stand in for the
/// app: a client for a local fake server, the part uploader on it, and Stop.
#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    fn client(url: &str, root: &Path) -> Client {
        Client::for_test(url.to_owned(), "fictional-part-upload-owner-token-0000", Some(root))
    }

    /// Record, before anything has placed the capture, that its parts are
    /// `seconds` long: a meeting's worth of 30-second parts is too long for
    /// a test, and the length is read from this file like any other.
    pub(crate) fn set_part_seconds(url: &str, root: &Path, note: &Note, capture_id: &str, seconds: u64) {
        let id = note.server_id.as_deref().unwrap();
        let mapping_root = client(url, root).mapping_root(note, id).unwrap();
        let key = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, capture_id.as_bytes());
        save_mapping(&mapping_root.join(format!("capture-{key}.parts.json")), &seconds).unwrap();
    }

    pub(crate) fn part_uploads(
        url: &str,
        root: &Path,
        note: Note,
        capture_id: &str,
        source: crate::live::Source,
    ) -> PartUploads {
        let (url, root) = (url.to_owned(), root.to_path_buf());
        PartUploads::start_with(
            move || Ok(client(&url, &root)),
            note,
            capture_id.to_owned(),
            source,
        )
    }

    /// What the serial worker does for Stop: the recording command, then the
    /// durable upload. Returns the note as the server then has it.
    pub(crate) fn stop_recording(url: &str, root: &Path, note: &Note) -> Result<NoteDTO, String> {
        client(url, root)
            .run(note, Action::Recording("stop"))
            .map(|reply| reply.note)
    }

    pub(crate) fn live_url(url: &str, note_id: &str) -> String {
        crate::accounts::client::live_url(url, note_id)
    }
}

#[cfg(test)]
mod tests {
    use super::{Client, WavReader};
    use std::fs;

    #[test]
    fn share_capabilities_cannot_change_origin_or_smuggle_an_arbitrary_path() {
        let token = "a".repeat(43);
        assert_eq!(
            super::capability_share_url("https://notes.example/", &token, &format!("/s/{token}"))
                .unwrap(),
            format!("https://notes.example/s/{token}")
        );
        for path in [
            "@other.example/x",
            "//other.example/x",
            "/s/other",
            "/admin",
            "/s/../admin",
        ] {
            assert!(super::capability_share_url("https://notes.example", &token, path).is_err());
        }
        assert!(super::capability_share_url("https://notes.example", "short", "/s/short").is_err());
        assert!(
            super::capability_share_url(
                "https://notes.example",
                &"/".repeat(43),
                &format!("/s/{}", "/".repeat(43))
            )
            .is_err()
        );
    }

    #[test]
    fn chat_metadata_frames_require_actual_typed_tools_and_bounded_unique_citation_ids() {
        use super::{ChatStreamEvent, decode_chat_frame};
        let tool = serde_json::json!({"callId":"419d65d1-7a2c-44ca-84cc-ad489522d561","name":"search_meetings","status":"completed","label":"Found 2 meetings","resultCount":2,"results":[{"noteId":"419d65d1-7a2c-44ca-84cc-ad489522d562","title":"Café planning","date":"2026-11-13"}]});
        let event = decode_chat_frame(
            &serde_json::json!({"type":"tool","text":"","tool":tool}).to_string(),
        );
        let Some(ChatStreamEvent::Tool(actual)) = event else {
            panic!("Expected typed tool metadata");
        };
        assert_eq!(actual.results.unwrap()[0].title, "Café planning");
        assert!(decode_chat_frame(r#"{"type":"tool","text":""}"#).is_none());
        let citation = serde_json::json!({"id":"S1","noteId":"419d65d1-7a2c-44ca-84cc-ad489522d561","title":"Café planning","source":"transcript","segmentId":"419d65d1-7a2c-44ca-84cc-ad489522d562","startMs":18000,"quote":"Mira owns café 🦊.","transcriptRevision":7});
        let Some(ChatStreamEvent::Citations(actual)) = decode_chat_frame(
            &serde_json::json!({"type":"citations","text":"","citations":[citation.clone()]})
                .to_string(),
        ) else {
            panic!("Expected citations");
        };
        assert_eq!(actual[0].quote, "Mira owns café 🦊.");
        assert!(decode_chat_frame(&serde_json::json!({"type":"citations","text":"","citations":[citation.clone(),citation.clone()]}).to_string()).is_none());
        for invalid in [
            serde_json::json!({"id":"S0"}),
            serde_json::json!({"segmentId":"not-an-id"}),
            serde_json::json!({"transcriptRevision":-1}),
        ] {
            let mut value = citation.clone();
            for (key, val) in invalid.as_object().unwrap() {
                value[key] = val.clone();
            }
            assert!(
                decode_chat_frame(
                    &serde_json::json!({"type":"citations","text":"","citations":[value]})
                        .to_string()
                )
                .is_none()
            );
        }
    }

    #[test]
    fn transcript_copy_keeps_speaker_source_and_independent_part_scope() {
        let mut remote: meeting_notes_api_types::NoteDTO = serde_json::from_value(serde_json::json!({
            "id":"3a49a4af-c660-42e9-a36a-a806fbb41e0d","title":"Meeting","body":"","enhanced":"","date":"2026-11-26","duration":"","createdAt":"2026-11-26T10:00:00Z","updatedAt":"2026-11-26T10:00:00Z","recordingState":"idle","generationState":"idle","transcriptRevision":0,"generatedRevision":null,"transcriptSegments":[]
        })).unwrap();
        let segment =
            |channel, speaker_id: &str, text: &str| meeting_notes_api_types::TranscriptSegmentDTO {
                id: text.into(),
                channel,
                speaker_id: speaker_id.into(),
                speaker_label: Some("Speaker 1".into()),
                text: text.into(),
                start_ms: 18000,
                end_ms: Some(19000),
                is_final: true,
            };
        remote.transcript_segments = vec![
            segment(
                meeting_notes_api_types::AudioChannel::Mic,
                "mic:419d65d1-7a2c-44ca-84cc-ad489522d561:speaker_0",
                "In-room first part.",
            ),
            segment(
                meeting_notes_api_types::AudioChannel::Mic,
                "mic:419d65d1-7a2c-44ca-84cc-ad489522d562:speaker_0",
                "Independent second part.",
            ),
            segment(
                meeting_notes_api_types::AudioChannel::System,
                "system:419d65d1-7a2c-44ca-84cc-ad489522d561:speaker_0",
                "System first part.",
            ),
        ];
        assert_eq!(
            super::transcript_text(&remote),
            "Speaker 1 · Mic · Part 1  00:18\nIn-room first part.\n\nSpeaker 1 · Mic · Part 2  00:18\nIndependent second part.\n\nSpeaker 1 · System · Part 1  00:18\nSystem first part."
        );
        assert_eq!(
            remote.transcript_segments[0].speaker_label.as_deref(),
            Some("Speaker 1"),
            "Display formatting must not alter canonical provider metadata"
        );
    }

    #[test]
    fn calendar_transport_uses_owner_routes_and_configured_lookahead() {
        use std::{
            collections::HashMap,
            io::{Read, Write},
            net::TcpListener,
            sync::{Arc, atomic::AtomicBool},
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let fixtures = [
                (
                    "/api/v1/calendar/preferences",
                    serde_json::json!({"lookAheadDays":30}).to_string(),
                ),
                ("/api/v1/calendars", "[]".into()),
                (
                    "/api/v1/integrations/google",
                    serde_json::json!({"configured":false,"redirectUri":null,"connections":[]})
                        .to_string(),
                ),
                (
                    "/api/v1/calendar/events?from=2026-11-12T05:00:00Z&to=2026-12-12T05:00:00Z",
                    "[]".into(),
                ),
            ];
            for (path, body) in fixtures {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                    .unwrap();
                let mut headers = Vec::new();
                while !headers.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    stream.read_exact(&mut byte).unwrap();
                    headers.push(byte[0]);
                    assert!(headers.len() < 10000);
                }
                let headers = String::from_utf8(headers).unwrap();
                assert!(
                    headers.starts_with(&format!("GET {path} HTTP/1.1\r\n")),
                    "Unexpected calendar route"
                );
                assert!(headers.contains("Authorization: Bearer fixture-calendar-owner\r\n"));
                write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
            }
        });
        let client = Client::for_test(format!("http://{address}"), "fixture-calendar-owner", None);
        let snapshot = super::load_calendar_events_with_client(
            &client,
            "2026-11-12T00:00:00-05:00",
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        assert_eq!(snapshot.look_ahead_days, 30);
        assert!(
            snapshot.sources.is_empty() && snapshot.events.is_empty() && !snapshot.personal_linked
        );
        server.join().unwrap();
    }

    #[test]
    fn listed_events_carry_an_attendee_count_only_where_the_server_sends_one() {
        let event = |id: &str| {
            serde_json::json!({"id":id,"calendarId":"c","calendarName":"Work","color":"#87e5e8","title":"Meeting","startAt":"2026-11-26T10:00:00Z","endAt":"2026-11-26T11:00:00Z","allDay":false,"startDate":"2026-11-26","endDate":"2026-11-26","timeZone":null,"location":null,"meetingUrl":null,"noteId":null})
        };
        let mut counted = event("counted");
        counted["attendeeCount"] = serde_json::json!(3);
        let mut alone = event("alone");
        alone["attendeeCount"] = serde_json::json!(0);
        let (events, attendees) =
            super::calendar_events(vec![counted, alone, event("unknown")]).unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(attendees.get("counted"), Some(&3));
        assert_eq!(attendees.get("alone"), Some(&0));
        assert_eq!(attendees.get("unknown"), None);
        assert!(super::calendar_events(vec![serde_json::json!({"id": 1})]).is_err());
    }

    #[test]
    fn calendar_note_reuses_server_identity_and_preserves_unsaved_local_edits() {
        let remote: meeting_notes_api_types::NoteDTO = serde_json::from_value(serde_json::json!({
            "id":"3a49a4af-c660-42e9-a36a-a806fbb41e0d","title":"Meeting","body":"","enhanced":"","date":"2026-11-26","duration":"","createdAt":"2026-11-26T10:00:00Z","updatedAt":"2026-11-26T10:00:00Z","recordingState":"idle","generationState":"idle","transcriptRevision":0,"generatedRevision":null,"transcriptSegments":[]
        })).unwrap();
        let mut workspace = crate::model::Workspace::with_slots(&[1, 2]);
        let id = super::import_calendar_note(
            &mut workspace,
            1,
            remote.clone(),
            "https://one.example".into(),
        )
        .unwrap();
        // In memory the note is known by a handle in its account's slot.
        assert_eq!(crate::accounts::handle::split(id), Some((1, 1)));
        let note = workspace.note_mut(id).unwrap();
        note.body = "Unsaved typed notes".into();
        note.enhanced_dirty = true;
        note.enhanced = "Unsaved enhanced edit".into();
        note.trashed = true;
        assert_eq!(
            super::import_calendar_note(
                &mut workspace,
                1,
                remote.clone(),
                "https://one.example".into()
            ),
            Some(id)
        );
        assert_eq!(workspace.notes.len(), 1);
        let note = workspace.note_mut(id).unwrap();
        assert_eq!(note.body, "Unsaved typed notes");
        assert_eq!(note.enhanced, "Unsaved enhanced edit");
        assert!(!note.trashed);
        let other = super::import_calendar_note(
            &mut workspace,
            1,
            remote.clone(),
            "https://other.example".into(),
        );
        assert_ne!(
            other,
            Some(id),
            "An event note from another instance is never reused"
        );
        // The same note id from another account, even on the same server,
        // is another note: everything is keyed with the account.
        let theirs =
            super::import_calendar_note(&mut workspace, 2, remote, "https://one.example".into())
                .unwrap();
        assert_ne!(theirs, id);
        assert_eq!(crate::accounts::handle::slot_of(theirs), Some(2));
        // An account whose notes are not loaded keeps nothing.
        let unloaded: meeting_notes_api_types::NoteDTO = serde_json::from_value(serde_json::json!({
            "id":"3a49a4af-c660-42e9-a36a-a806fbb41e0e","title":"Meeting","body":"","enhanced":"","date":"2026-11-26","duration":"","createdAt":"2026-11-26T10:00:00Z","updatedAt":"2026-11-26T10:00:00Z","recordingState":"idle","generationState":"idle","transcriptRevision":0,"generatedRevision":null,"transcriptSegments":[]
        })).unwrap();
        assert_eq!(
            super::import_calendar_note(&mut workspace, 9, unloaded, "https://one.example".into()),
            None
        );
    }

    #[test]
    fn a_generated_title_is_adopted_and_the_placeholder_is_never_sent_back() {
        // Generate was asked for a note nobody had named, in each of the
        // forms an unnamed note takes here.
        for placeholder in ["", "Untitled note", "New note", "  untitled NOTE "] {
            let mut note = crate::model::Note {
                title: placeholder.into(),
                body: "typed".into(),
                ..Default::default()
            };
            // The request carries no title, so the server's is not replaced.
            let sent = super::note_update_payload(&note);
            assert!(sent.get("title").is_none(), "{placeholder:?}");
            assert_eq!(sent["body"], "typed");
            assert!(super::note_fields(&note).get("title").is_none());
            // The reply carries the generated title: the note takes it.
            assert!(note.adopt_server_title(placeholder, "Flour supplier switch to Windrush"));
            assert_eq!(note.title, "Flour supplier switch to Windrush");
            // Every later save sends the name, not the placeholder.
            assert_eq!(
                super::note_update_payload(&note)["title"],
                "Flour supplier switch to Windrush"
            );
            assert_eq!(super::note_fields(&note)["title"], "Flour supplier switch to Windrush");
        }
        // A title typed while the request was on its way is the person's.
        let mut typed = crate::model::Note {
            title: "Budget review".into(),
            ..Default::default()
        };
        assert!(!typed.adopt_server_title("", "Hiring plan and budget for Q3"));
        assert_eq!(typed.title, "Budget review");
        // So is one that was there all along.
        assert!(!typed.adopt_server_title("Budget review", "Hiring plan and budget for Q3"));
        assert_eq!(typed.title, "Budget review");
        // A placeholder typed over another since the request is a change too.
        let mut retyped = crate::model::Note {
            title: "New note".into(),
            ..Default::default()
        };
        assert!(!retyped.adopt_server_title("", "Hiring plan and budget for Q3"));
        assert_eq!(retyped.title, "New note");
        // A reply that has no title to give changes nothing.
        let mut unnamed = crate::model::Note::default();
        for remote in ["", "Untitled note", "New note"] {
            assert!(!unnamed.adopt_server_title("", remote), "{remote:?}");
            assert_eq!(unnamed.title, "");
        }
    }

    #[test]
    fn enhanced_patch_is_explicit_and_can_save_an_intentional_empty_edit() {
        let mut note = crate::model::Note {
            title: "Meeting".into(),
            enhanced: "Cached generated text".into(),
            ..Default::default()
        };
        assert!(
            super::note_update_payload(&note).get("enhanced").is_none(),
            "Unedited cached text must not overwrite another client's newer notes"
        );
        note.enhanced_dirty = true;
        note.enhanced.clear();
        assert_eq!(super::note_update_payload(&note)["enhanced"], "");
    }

    #[test]
    fn chat_frame_validation_preserves_unicode_and_rejects_unknown_events() {
        use super::{ChatStreamEvent, decode_chat_frame};
        assert_eq!(
            decode_chat_frame(r#"{"type":"delta","text":"Olá 🦊\n"}"#),
            Some(ChatStreamEvent::Delta("Olá 🦊\n".into()))
        );
        assert_eq!(
            decode_chat_frame(r#"{"type":"done","text":""}"#),
            Some(ChatStreamEvent::Done)
        );
        assert_eq!(
            decode_chat_frame(r#"{"type":"secret","text":"ignored"}"#),
            None
        );
        assert_eq!(decode_chat_frame("not json"), None);
    }

    fn accept_test_request(listener: std::net::TcpListener) -> std::net::TcpStream {
        use std::io::{Read, Write};
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut headers = Vec::new();
        let mut byte = [0; 1];
        while !headers.ends_with(b"\r\n\r\n") {
            stream.read_exact(&mut byte).unwrap();
            headers.push(byte[0]);
            assert!(headers.len() < 10000);
        }
        let headers = String::from_utf8(headers).unwrap();
        let length: usize = headers
            .lines()
            .find_map(|line| {
                line.to_lowercase()
                    .strip_prefix("content-length:")
                    .map(|s| s.trim().parse().unwrap())
            })
            .unwrap_or(0);
        if headers.to_lowercase().contains("expect: 100-continue") {
            stream.write_all(b"HTTP/1.1 100 Continue\r\n\r\n").unwrap();
        }
        let mut body = vec![0; length];
        stream.read_exact(&mut body).unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        if let Some(audio) = body.get("audioBase64") {
            assert_eq!(audio.as_str().unwrap().len(), 8_000_000);
        }
        stream
    }

    #[test]
    fn native_transport_accepts_realistic_eight_megabyte_audio_json() {
        use std::{collections::HashMap, io::Write, net::TcpListener};
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let mut socket = accept_test_request(listener);
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 11\r\nConnection: close\r\n\r\n{\"ok\":true}").unwrap();
        });
        let client = Client::for_test(format!("http://{address}"), "fictional-large-upload-owner", None);
        let response: serde_json::Value = client
            .request(
                "PUT",
                "/api/v1/fixture",
                Some(serde_json::json!({"audioBase64":"A".repeat(8_000_000)})),
            )
            .unwrap();
        assert_eq!(response["ok"], true);
        server.join().unwrap();
    }

    #[test]
    fn native_chat_stream_delivers_fragmented_deltas_before_completion() {
        use super::{ChatStreamEvent, stream_chat_with_client};
        use std::{
            collections::HashMap,
            io::Write,
            net::TcpListener,
            sync::{Arc, atomic::AtomicBool},
            time::Duration,
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let mut socket = accept_test_request(listener);
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n: keepalive\n\n").unwrap();
            for text in ["Olá", " 🦊"] {
                let frame = format!(
                    "data: {}\n\n",
                    serde_json::json!({"type":"delta","text":text})
                );
                for bytes in frame.as_bytes().chunks(3) {
                    socket.write_all(bytes).unwrap();
                }
                std::thread::sleep(Duration::from_millis(60));
            }
            socket
                .write_all(b"data: {\"type\":\"done\",\"text\":\"\"}\n\n")
                .unwrap();
        });
        let client = Client::for_test(format!("http://{address}"), "fictional-stream-owner", None);
        let receiver = stream_chat_with_client(
            Ok(client),
            None,
            "Question".into(),
            vec![],
            Arc::new(AtomicBool::new(false)),
            None,
        );
        assert_eq!(
            receiver.recv_timeout(Duration::from_secs(2)).unwrap(),
            ChatStreamEvent::Delta("Olá".into())
        );
        assert_eq!(
            receiver.recv_timeout(Duration::from_secs(2)).unwrap(),
            ChatStreamEvent::Delta(" 🦊".into())
        );
        assert_eq!(
            receiver.recv_timeout(Duration::from_secs(2)).unwrap(),
            ChatStreamEvent::Done
        );
        server.join().unwrap();
    }

    #[test]
    fn stop_chat_closes_the_native_http_connection() {
        use super::{ChatStreamEvent, stream_chat_with_client};
        use std::{
            collections::HashMap,
            io::{Read, Write},
            net::TcpListener,
            sync::{
                Arc,
                atomic::{AtomicBool, Ordering},
            },
            time::Duration,
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let mut socket = accept_test_request(listener);
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\ndata: {\"type\":\"delta\",\"text\":\"first\"}\n\n").unwrap();
            let mut byte = [0; 1];
            let closed = socket.read(&mut byte);
            assert!(matches!(closed, Ok(0)) || closed.is_err());
        });
        let client = Client::for_test(format!("http://{address}"), "fictional-cancel-owner", None);
        let cancel = Arc::new(AtomicBool::new(false));
        let receiver =
            stream_chat_with_client(Ok(client), None, "Question".into(), vec![], cancel.clone(), None);
        assert_eq!(
            receiver.recv_timeout(Duration::from_secs(2)).unwrap(),
            ChatStreamEvent::Delta("first".into())
        );
        cancel.store(true, Ordering::Release);
        assert_eq!(
            receiver.recv_timeout(Duration::from_secs(2)),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected)
        );
        server.join().unwrap();
    }
    /// Explicit opt-in: real authenticated requests and provider credits, using
    /// only the fictional WAV supplied by the caller. Never runs in normal CI.
    #[test]
    #[ignore = "requires a deployed server, owner token and synthetic QA audio"]
    fn live_server_record_transcribe_generate_resume_chat() {
        use super::{Action, Client};
        use crate::{
            audio::{CaptureManifest, ChannelStats, NativeStats},
            model::Note,
        };
        let audio =
            std::env::var("MEETING_QA_AUDIO").expect("Set MEETING_QA_AUDIO to synthetic PCM WAV");
        let resume_audio = std::env::var("MEETING_QA_RESUME").unwrap_or_else(|_| audio.clone());
        assert!(
            crate::environment::var_os("CEREAL_DATA_DIR").is_some(),
            "Use an isolated CEREAL_DATA_DIR for this test"
        );
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let mut client = crate::accounts::default_client().unwrap();
        let mut first = Note {
            id: stamp,
            title: "Fictional desktop provider pipeline acceptance".into(),
            body: "Synthetic test conversation; no real meeting content.".into(),
            date: chrono::Utc::now().date_naive().to_string(),
            ..Default::default()
        };
        let created = client.run(&first, Action::Recording("start")).unwrap();
        let id = created.note.id.clone();
        first.server_id = Some(id.clone());
        first.server_url = Some(created.url.clone());
        let make_capture = |local_id: u64, name: &str, path: &str| {
            let wav = WavReader::open(std::path::Path::new(path)).unwrap();
            CaptureManifest {
                version: 1,
                capture_id: format!("synthetic-{stamp}-{name}"),
                note_id: local_id,
                directory: std::path::PathBuf::from(path).parent().unwrap().into(),
                manifest_path: std::path::PathBuf::from(format!(
                    "/tmp/no-live-manifest-{stamp}-{name}.json"
                )),
                started_at: chrono::Utc::now().to_rfc3339(),
                finished_at: Some(chrono::Utc::now().to_rfc3339()),
                error: None,
                stats: NativeStats {
                    version: 1,
                    phase: "stopped".into(),
                    requested_channels: 2,
                    mic: ChannelStats {
                        state: "disabled".into(),
                        ..Default::default()
                    },
                    system: ChannelStats {
                        state: "stopped".into(),
                        sample_rate: wav.rate as f64,
                        channels: 1,
                        frames_received: wav.frames,
                        frames_written: wav.frames,
                        path: Some(path.into()),
                        paths: vec![path.into()],
                        ..Default::default()
                    },
                },
            }
        };
        let outcome = (|| -> Result<serde_json::Value, String> {
            first
                .audio_captures
                .push(make_capture(first.id, "first", &audio));
            let stopped = client.run(&first, Action::Recording("stop"))?;
            if stopped.note.transcript_segments.is_empty() {
                return Err("No real transcript arrived".into());
            }
            let transcript = stopped
                .note
                .transcript_segments
                .iter()
                .map(|s| s.text.as_str())
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase();
            let expected_terms = std::env::var("MEETING_QA_TRANSCRIPT_TERMS").unwrap_or_default();
            if expected_terms
                .split(',')
                .map(str::trim)
                .filter(|term| !term.is_empty())
                .any(|term| !transcript.contains(&term.to_lowercase()))
            {
                return Err("The actual desktop transcript did not contain every requested synthetic fixture fact".into());
            }
            let generated = client.run(&first, Action::Generate)?;
            if generated.note.enhanced.is_empty()
                || generated.note.generated_revision != Some(generated.note.transcript_revision)
            {
                return Err("Generation did not consume the real transcript revision".into());
            }
            // Simulate a second device that imported the same note with no local
            // capture history. Its first new recording must resume, allocate the
            // next sequence and start after the first device's remote timeline.
            let mut imported = Note {
                id: stamp + 1,
                title: first.title.clone(),
                body: first.body.clone(),
                date: first.date.clone(),
                server_id: Some(id.clone()),
                server_url: Some(created.url.clone()),
                ..Default::default()
            };
            let mut second = crate::accounts::default_client()?;
            second.run(&imported, Action::Recording("start"))?;
            imported
                .audio_captures
                .push(make_capture(imported.id, "second", &resume_audio));
            let resumed = second.run(&imported, Action::Recording("stop"))?;
            if resumed.note.id != id
                || resumed.note.transcript_revision <= generated.note.transcript_revision
            {
                return Err("Resume did not append to the same note".into());
            }
            let before: Vec<meeting_notes_api_types::AudioChunkDTO> =
                second.request("GET", &format!("/api/v1/notes/{id}/audio/chunks"), None)?;
            // A retry must not create new transcript segments or advance revision.
            let retried = second.run(&imported, Action::Sync)?;
            if retried.note.transcript_revision != resumed.note.transcript_revision {
                return Err("Retry duplicated transcript content".into());
            }
            let mut chunks = before;
            chunks.sort_by_key(|c| c.sequence);
            if chunks.len() < 2
                || chunks
                    .windows(2)
                    .any(|w| w[1].start_ms < w[0].start_ms + w[0].duration_ms)
            {
                return Err("Cross-device audio chunks overlap or are missing".into());
            }
            let regenerated = second.run(&imported, Action::Generate)?;
            if regenerated.note.generated_revision != Some(resumed.note.transcript_revision) {
                return Err("Regeneration revision was stale".into());
            }
            let chatted = second.run(
                &imported,
                Action::Chat {
                    message: "What actions and decisions were discussed?".into(),
                    history: Vec::new(),
                },
            )?;
            if chatted.answer.as_ref().is_none_or(|a| a.trim().is_empty()) {
                return Err("Chat returned no answer".into());
            }
            Ok(
                serde_json::json!({"sameNote":true,"chunks":chunks.len(),"firstRevision":generated.note.transcript_revision,"resumedRevision":resumed.note.transcript_revision,"retryDidNotDuplicate":true,"crossDeviceTimelineOrdered":true,"enhancedCharacters":regenerated.note.enhanced.len(),"chatCharacters":chatted.answer.unwrap().len()}),
            )
        })();
        let cleanup =
            client.request::<serde_json::Value>("DELETE", &format!("/api/v1/notes/{id}"), None);
        assert!(cleanup.is_ok(), "Could not delete synthetic QA fixture");
        eprintln!(
            "Desktop live provider verification: {}",
            outcome.expect("Desktop provider integration failed")
        );
    }
    #[test]
    fn native_transport_preserves_unicode_json_and_owner_authentication() {
        use std::{
            collections::HashMap,
            io::{Read, Write},
            net::TcpListener,
            time::{Duration, Instant},
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let server = std::thread::spawn(move || {
            let started = Instant::now();
            let (mut stream, _) = loop {
                match listener.accept() {
                    Ok(value) => break value,
                    Err(e)
                        if e.kind() == std::io::ErrorKind::WouldBlock
                            && started.elapsed() < Duration::from_secs(5) =>
                    {
                        std::thread::sleep(Duration::from_millis(5))
                    }
                    Err(e) => panic!("Fixture server failed: {e}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut received = Vec::new();
            let mut buffer = [0u8; 1024];
            let header_end = loop {
                let count = stream.read(&mut buffer).unwrap();
                assert!(count > 0);
                received.extend_from_slice(&buffer[..count]);
                if let Some(end) = received.windows(4).position(|v| v == b"\r\n\r\n") {
                    break end + 4;
                }
            };
            let headers = String::from_utf8(received[..header_end].to_vec()).unwrap();
            assert!(headers.starts_with("POST /api/v1/fixture HTTP/1.1\r\n"));
            assert!(
                headers.contains("Authorization: Bearer fixture-owner-credential-only-for-test")
            );
            let length: usize = headers
                .lines()
                .find_map(|l| {
                    l.to_lowercase()
                        .strip_prefix("content-length:")
                        .map(|v| v.trim().parse().unwrap())
                })
                .unwrap();
            while received.len() < header_end + length {
                let count = stream.read(&mut buffer).unwrap();
                assert!(count > 0);
                received.extend_from_slice(&buffer[..count]);
            }
            let body: serde_json::Value =
                serde_json::from_slice(&received[header_end..header_end + length]).unwrap();
            assert_eq!(body["message"], "Olá \"team\"\nAction: résumé");
            stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 11\r\nConnection: close\r\n\r\n{\"ok\":true}").unwrap();
        });
        let client = Client::for_test(format!("http://{address}"), "fixture-owner-credential-only-for-test", None);
        let reply: serde_json::Value = client
            .request(
                "POST",
                "/api/v1/fixture",
                Some(serde_json::json!({"message":"Olá \"team\"\nAction: résumé"})),
            )
            .unwrap();
        assert_eq!(reply["ok"], true);
        server.join().unwrap();
    }
    fn fixture(format: u16, rate: u32, bits: u16, samples: &[u8]) -> std::path::PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let p =
            std::env::temp_dir().join(format!("meeting-wav-{}-{stamp}.wav", std::process::id()));
        let mut b = Vec::new();
        b.extend(b"RIFF");
        b.extend((36 + samples.len() as u32).to_le_bytes());
        b.extend(b"WAVEfmt ");
        b.extend(16u32.to_le_bytes());
        b.extend(format.to_le_bytes());
        b.extend(1u16.to_le_bytes());
        b.extend(rate.to_le_bytes());
        b.extend((rate * (bits as u32 / 8)).to_le_bytes());
        b.extend((bits / 8).to_le_bytes());
        b.extend(bits.to_le_bytes());
        b.extend(b"data");
        b.extend((samples.len() as u32).to_le_bytes());
        b.extend(samples);
        fs::write(&p, b).unwrap();
        p
    }
    #[test]
    fn float_capture_conversion_preserves_samples_and_sanitizes_nonfinite() {
        let samples: Vec<_> = [0f32, 0.5, -0.5, 2., f32::NAN]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect();
        let p = fixture(3, 48000, 32, &samples);
        let mut w = WavReader::open(&p).unwrap();
        let (bytes, offset) = w.next_chunk().unwrap().unwrap();
        assert_eq!(offset, 0);
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(bytes.len(), 54);
        let output: Vec<_> = bytes[44..]
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes(b.try_into().unwrap()))
            .collect();
        assert_eq!(output, vec![0, 16384, -16384, 32767, 0]);
        assert!(w.next_chunk().unwrap().is_none());
        fs::remove_file(p).unwrap();
    }
    #[test]
    fn long_recording_splits_without_truncation_and_preserves_offsets() {
        let p = fixture(1, 16000, 16, &vec![0u8; (16000 * 60 + 1) * 2]);
        let mut w = WavReader::open(&p).unwrap();
        let a = w.next_chunk().unwrap().unwrap();
        let b = w.next_chunk().unwrap().unwrap();
        assert_eq!(a.1, 0);
        assert_eq!(a.0.len(), 44 + 16000 * 60 * 2);
        assert_eq!(b.1, 60000);
        assert_eq!(b.0.len(), 46);
        assert!(w.next_chunk().unwrap().is_none());
        fs::remove_file(p).unwrap();
    }
    #[test]
    fn incomplete_audio_is_rejected_instead_of_silently_uploading_a_prefix() {
        let p = fixture(3, 48000, 32, &0.5f32.to_le_bytes());
        let mut b = fs::read(&p).unwrap();
        b.truncate(b.len() - 1);
        fs::write(&p, b).unwrap();
        assert!(WavReader::open(&p).is_err());
        fs::remove_file(p).unwrap();
    }
    #[test]
    fn retry_chunk_identity_is_stable_and_source_scoped() {
        let key = "note-a:capture-one:system:file:0";
        let a = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, key.as_bytes());
        assert_eq!(
            a,
            uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, key.as_bytes())
        );
        assert_ne!(
            a,
            uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, b"note-a:capture-one:mic:file:0")
        );
    }
}

#[cfg(test)]
mod evidence_transport_tests {
    use super::Client;
    use std::{
        collections::HashMap,
        io::Read,
        net::TcpListener,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
            mpsc,
        },
        time::{Duration, Instant},
    };

    #[test]
    fn closing_pending_evidence_cancels_the_actual_native_http_child() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (accepted, wait_for_accept) = mpsc::channel();
        let (release, wait_for_release) = mpsc::channel();
        let fixture = std::thread::spawn(move || {
            let (mut connection, _) = listener.accept().unwrap();
            connection
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut input = [0u8; 1024];
            assert!(connection.read(&mut input).unwrap() > 0);
            accepted.send(()).unwrap();
            let _ = wait_for_release.recv_timeout(Duration::from_secs(10));
        });
        let cancel = Arc::new(AtomicBool::new(false));
        let request_cancel = cancel.clone();
        let request = std::thread::spawn(move || {
            let client = Client::for_test(format!("http://{address}"), "fictional-evidence-transport-owner-token", None);
            client.request_with_cancel::<serde_json::Value>(
                "GET",
                "/api/v1/pending",
                None,
                Some(request_cancel),
            )
        });
        wait_for_accept
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        let began = Instant::now();
        cancel.store(true, Ordering::Release);
        let result = request.join().unwrap();
        assert_eq!(result.unwrap_err(), "Request cancelled");
        assert!(began.elapsed() < Duration::from_secs(5));
        release.send(()).unwrap();
        fixture.join().unwrap();
    }

    #[test]
    fn the_live_socket_is_on_the_notes_own_server_with_tls_wherever_requests_have_it() {
        let id = "419d65d1-7a2c-44ca-84cc-ad489522d561";
        assert_eq!(
            crate::accounts::client::live_url("https://notes.example", id),
            format!("wss://notes.example/api/v1/notes/{id}/live")
        );
        assert_eq!(
            crate::accounts::client::live_url("http://127.0.0.1:3000", id),
            format!("ws://127.0.0.1:3000/api/v1/notes/{id}/live")
        );
    }

    /// A WAV of `frames` silent PCM16 frames at `rate`.
    fn silent_wav(path: &std::path::Path, rate: u32, frames: u32) {
        let mut bytes = Vec::new();
        bytes.extend(b"RIFF");
        bytes.extend((36 + frames * 2).to_le_bytes());
        bytes.extend(b"WAVEfmt ");
        bytes.extend(16u32.to_le_bytes());
        bytes.extend(1u16.to_le_bytes());
        bytes.extend(1u16.to_le_bytes());
        bytes.extend(rate.to_le_bytes());
        bytes.extend((rate * 2).to_le_bytes());
        bytes.extend(2u16.to_le_bytes());
        bytes.extend(16u16.to_le_bytes());
        bytes.extend(b"data");
        bytes.extend((frames * 2).to_le_bytes());
        bytes.resize(bytes.len() + frames as usize * 2, 0);
        std::fs::write(path, bytes).unwrap();
    }

    /// Part identities are decided once per capture and never change: a new
    /// capture is cut into 30-second parts, one that was placed or uploaded
    /// before part lengths were recorded keeps its 60-second parts, and
    /// either way a later run reads the same answer from disk.
    #[test]
    fn a_capture_keeps_the_part_length_it_was_first_given() {
        use super::{AudioChunkDTO, WavReader};
        use crate::audio::{CaptureManifest, ChannelStats, NativeStats};
        use std::collections::HashMap;
        let root = std::env::temp_dir().join(format!("cereal-part-plan-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let client = Client::for_test("http://127.0.0.1:9", "fictional-part-plan-owner-token-00000", Some(&root));
        let id = "419d65d1-7a2c-44ca-84cc-ad489522d561";
        let note = crate::model::Note { id: 5, server_id: Some(id.into()), ..Default::default() };
        let mapping_root = client.mapping_root(&note, id).unwrap();
        let wav = root.join("mic.wav");
        silent_wav(&wav, 8_000, 8_000 * 150); // two and a half minutes
        let capture = |name: &str| CaptureManifest {
            version: 1,
            capture_id: name.into(),
            note_id: 5,
            directory: root.clone(),
            manifest_path: root.join(format!("{name}.json")),
            started_at: "2026-11-14T10:00:00Z".into(),
            finished_at: Some("2026-11-14T10:03:00Z".into()),
            error: None,
            stats: NativeStats {
                requested_channels: 1,
                mic: ChannelStats {
                    sample_rate: 8_000.,
                    frames_written: 8_000 * 150,
                    path: Some(wav.to_string_lossy().into_owned()),
                    paths: vec![wav.to_string_lossy().into_owned()],
                    ..Default::default()
                },
                ..Default::default()
            },
        };
        let none = HashMap::new();
        // A capture nothing has placed: 30-second parts, written down.
        let new = capture("capture-new");
        let plan = client.capture_plan(id, &new, &none, &mapping_root, 7_000, false).unwrap();
        assert_eq!(plan, super::CapturePlan { base_ms: 7_000, part_seconds: 30 });
        // Asked again later, with the note's audio now ending elsewhere.
        assert_eq!(client.capture_plan(id, &new, &none, &mapping_root, 99_000, false).unwrap(), plan);
        let reader = WavReader::open(&wav).unwrap().parts_of(plan.part_seconds);
        assert_eq!(reader.frames.div_ceil(reader.step), 5);
        // A capture an older client placed (a position, no part length):
        // it keeps the 60-second parts its identities were made with.
        let placed = capture("capture-placed");
        let key = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, b"capture-placed");
        super::save_mapping(&mapping_root.join(format!("capture-{key}.json")), &3_000u64).unwrap();
        let plan = client.capture_plan(id, &placed, &none, &mapping_root, 99_000, false).unwrap();
        assert_eq!(plan, super::CapturePlan { base_ms: 3_000, part_seconds: 60 });
        // A capture whose parts are on the server from before any of this
        // was written down: found by its 60-second identity, kept at 60.
        let uploaded = capture("capture-uploaded");
        let second = super::chunk_identity(id, "capture-uploaded", "mic", &wav.to_string_lossy(), 1);
        let on_server: HashMap<String, AudioChunkDTO> = [(
            second.clone(),
            serde_json::from_value(serde_json::json!({"id": second, "noteId": id, "channel": "mic", "sequence": 4,
                "startMs": 500_000, "durationMs": 60_000, "state": "ready", "error": null})).unwrap(),
        )]
        .into();
        let plan = client.capture_plan(id, &uploaded, &on_server, &mapping_root, 99_000, false).unwrap();
        assert_eq!(plan, super::CapturePlan { base_ms: 440_000, part_seconds: 60 });
        let mut reader = WavReader::open(&wav).unwrap().parts_of(plan.part_seconds);
        assert_eq!(reader.frames.div_ceil(reader.step), 3);
        // A part can be read on its own, as the uploader does while the file
        // grows: the second 60 seconds start a minute in.
        let (bytes, start_ms) = reader.chunk(1).unwrap().unwrap();
        assert_eq!((bytes.len(), start_ms), (44 + 8_000 * 60 * 2, 60_000));
        assert_eq!(reader.chunk(2).unwrap().unwrap().0.len(), 44 + 8_000 * 30 * 2);
        assert!(reader.chunk(3).unwrap().is_none());
        // A part's number is above every number the server holds and every
        // number already promised to a part that has not got there yet.
        assert_eq!(Client::next_sequence(&none, &mapping_root), 0);
        assert_eq!(Client::next_sequence(&on_server, &mapping_root), 5);
        let promised = super::chunk_identity(id, "capture-new", "mic", &wav.to_string_lossy(), 0);
        super::save_mapping(
            &mapping_root.join(format!("{promised}.json")),
            &super::UploadReceipt { sequence: 11, start_ms: 7_000 },
        )
        .unwrap();
        assert_eq!(Client::next_sequence(&on_server, &mapping_root), 12);
        // A part the server already holds is not sent again (this client
        // points at a closed port: sending would fail), and keeps the number
        // and position the server has for it.
        let mut sequence = 12;
        let target = super::PartTarget { id, capture_id: "capture-uploaded", mapping_root: &mapping_root };
        let sent = client.send_part(&target, &on_server, "mic", &wav.to_string_lossy(), 1, vec![0; 64], 0, &mut sequence, None);
        assert_eq!(sent, Ok(second));
        assert_eq!(sequence, 12);
        std::fs::remove_dir_all(root).unwrap();
    }

    /// The live stream decides where a capture sits on the note's timeline
    /// while it is still recording; the durable upload must then place the
    /// same capture's parts at the same milliseconds. Both go through one
    /// mapping file, written by whichever comes first (here: the stream).
    #[test]
    fn a_live_capture_takes_the_timeline_position_the_upload_will_use() {
        use crate::{
            audio::{CaptureManifest, ChannelStats, NativeStats},
            model::Note,
        };
        use std::{
            collections::HashMap,
            fs,
            io::{Read, Write},
            net::TcpListener,
        };
        let root = std::env::temp_dir().join(format!("cereal-live-base-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let id = "419d65d1-7a2c-44ca-84cc-ad489522d561";
        // The server is asked for the note's parts twice. The first time it
        // has none. The second time (a later connection, after another
        // device added audio) it lists a part that ends much later.
        let server = std::thread::spawn(move || {
            for body in [
                "[]".to_string(),
                serde_json::json!([{"id":"419d65d1-7a2c-44ca-84cc-ad489522d570","noteId":id,"channel":"mic",
                    "sequence":0,"startMs":0,"durationMs":900_000,"state":"ready","error":null}])
                .to_string(),
            ] {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = Vec::new();
                let mut byte = [0; 1];
                while !request.ends_with(b"\r\n\r\n") {
                    stream.read_exact(&mut byte).unwrap();
                    request.push(byte[0]);
                }
                let request = String::from_utf8(request).unwrap();
                assert!(request.starts_with(&format!("GET /api/v1/notes/{id}/audio/chunks ")));
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            }
        });
        let capture = |name: &str, finished: bool, frames: u64| {
            let directory = root.join(name);
            fs::create_dir_all(&directory).unwrap();
            let manifest = CaptureManifest {
                version: 1,
                capture_id: name.into(),
                note_id: 77,
                manifest_path: directory.join("manifest.json"),
                directory,
                started_at: "2026-11-14T10:00:00Z".into(),
                finished_at: finished.then(|| "2026-11-14T10:01:00Z".into()),
                error: None,
                stats: NativeStats {
                    version: 1,
                    phase: if finished { "stopped" } else { "recording" }.into(),
                    requested_channels: 1,
                    mic: ChannelStats {
                        sample_rate: 48_000.,
                        frames_written: frames,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            };
            fs::write(&manifest.manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
            manifest
        };
        // An earlier capture of 12.5 s that was never uploaded, then the one
        // being recorded now.
        let note = Note {
            id: 77,
            server_id: Some(id.into()),
            audio_captures: vec![
                capture("capture-earlier", true, 600_000),
                capture("capture-live", false, 4_800),
            ],
            ..Default::default()
        };
        let client = Client::for_test(format!("http://{address}"), "fictional-live-base-owner-token-0000", Some(&root));
        assert_eq!(client.live_capture_base(&note, id, "capture-live"), Ok(12_500));
        // What `upload` will read for each capture when the recording stops.
        let saved = |name: &str| -> u64 {
            let key = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, name.as_bytes());
            let path = client
                .association_path(77)
                .unwrap()
                .parent()
                .unwrap()
                .join("uploads")
                .join(id)
                .join(format!("capture-{key}.json"));
            serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
        };
        assert_eq!((saved("capture-earlier"), saved("capture-live")), (0, 12_500));
        // Decided once: a later look at a server that now holds more audio
        // does not move a capture that is already streaming.
        assert_eq!(client.live_capture_base(&note, id, "capture-live"), Ok(12_500));
        assert_eq!((saved("capture-earlier"), saved("capture-live")), (0, 12_500));
        server.join().unwrap();
        fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod note_sync_tests {
    use super::{Client, notes_to_read, refresh_notes, sync_stamp};
    use meeting_notes_api_types::NoteSummaryDTO;
    use std::{
        collections::HashMap,
        io::{Read, Write},
        net::TcpListener,
        sync::{Arc, Mutex},
        time::{Duration, Instant},
    };

    fn id(n: u32) -> String {
        format!("00000000-0000-4000-8000-{n:012}")
    }
    fn row_json(n: u32, updated: &str, transcript: i64) -> String {
        format!(
            r#"{{"id":"{}","title":"Note {n}","date":"2026-11-12","duration":"","createdAt":"2026-11-12T10:00:00.000Z","updatedAt":"{updated}","recordingState":"stopped","generationState":"idle","transcriptRevision":{transcript},"generatedRevision":null,"hasBody":true,"hasSummary":false,"hasTranscript":false,"speakerCount":0,"excerpt":"","meeting":null,"attendees":{{"people":[],"others":0,"total":0}},"organizationId":null,"folders":[]}}"#,
            id(n)
        )
    }
    fn note_json(n: u32, updated: &str, transcript: i64) -> String {
        format!(
            r#"{{"id":"{}","title":"Note {n}","body":"Body of {n}","enhanced":"","date":"2026-11-12","duration":"","createdAt":"2026-11-12T10:00:00.000Z","updatedAt":"{updated}","recordingState":"stopped","generationState":"idle","transcriptRevision":{transcript},"generatedRevision":null,"transcriptSegments":[]}}"#,
            id(n)
        )
    }
    fn page_json(rows: &[String], next: Option<&str>) -> String {
        format!(
            r#"{{"notes":[{}],"nextCursor":{},"total":{}}}"#,
            rows.join(","),
            next.map_or("null".to_string(), |cursor| format!("\"{cursor}\"")),
            rows.len()
        )
    }
    fn rows(json: &[String]) -> Vec<NoteSummaryDTO> {
        json.iter()
            .map(|row| serde_json::from_str(row).unwrap())
            .collect()
    }

    /// A server for the real transport: answers each request by its path until
    /// the client has gone quiet, and returns the request lines it saw.
    fn serve(
        answer: impl Fn(&str) -> (u16, String) + Send + 'static,
    ) -> (Client, Arc<Mutex<Vec<String>>>, std::thread::JoinHandle<()>, Arc<Mutex<bool>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let done = Arc::new(Mutex::new(false));
        let (log, stop) = (seen.clone(), done.clone());
        let server = std::thread::spawn(move || {
            let started = Instant::now();
            while !*stop.lock().unwrap() && started.elapsed() < Duration::from_secs(30) {
                let mut stream = match listener.accept() {
                    Ok((stream, _)) => stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(error) => panic!("Fixture server failed: {error}"),
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut received = Vec::new();
                let mut buffer = [0u8; 1024];
                while !received.windows(4).any(|window| window == b"\r\n\r\n") {
                    let count = stream.read(&mut buffer).unwrap();
                    assert!(count > 0);
                    received.extend_from_slice(&buffer[..count]);
                }
                let headers = String::from_utf8_lossy(&received).to_string();
                assert!(headers.contains("Authorization: Bearer fixture-owner-credential-only-for-test"));
                let line = headers.lines().next().unwrap().to_string();
                let path = line.split(' ').nth(1).unwrap().to_string();
                log.lock().unwrap().push(format!("{} {path}", line.split(' ').next().unwrap()));
                let (status, body) = answer(&path);
                write!(
                    stream,
                    "HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            }
        });
        let client = Client::for_test(format!("http://{address}"), "fixture-owner-credential-only-for-test", None);
        (client, seen, server, done)
    }

    #[test]
    fn a_note_is_read_again_only_when_its_row_no_longer_matches_the_copy_kept() {
        let listed = rows(&[
            row_json(1, "2026-11-12T10:00:00.000Z", 1),
            row_json(2, "2026-11-12T11:00:00.000Z", 1),
            row_json(3, "2026-11-12T10:00:00.000Z", 2),
            row_json(4, "2026-11-12T10:00:00.000Z", 1),
        ]);
        let kept = HashMap::from([
            (id(1), sync_stamp("2026-11-12T10:00:00.000Z", 1, None)),
            // Edited on the server since.
            (id(2), sync_stamp("2026-11-12T10:00:00.000Z", 1, None)),
            // More speech transcribed since.
            (id(3), sync_stamp("2026-11-12T10:00:00.000Z", 1, None)),
            // Kept here, and no longer listed by the server: nothing to read.
            (id(9), sync_stamp("2026-11-12T10:00:00.000Z", 1, None)),
        ]);
        assert_eq!(notes_to_read(&listed, &kept), vec![id(2), id(3), id(4)]);
        assert_eq!(notes_to_read(&listed, &HashMap::new()).len(), 4);
        assert_ne!(
            sync_stamp("2026-11-12T10:00:00.000Z", 1, None),
            sync_stamp("2026-11-12T10:00:00.000Z", 1, Some(1))
        );
        assert_ne!(sync_stamp("a", 1, Some(0)), sync_stamp("a", 1, None));
    }

    #[test]
    fn refresh_pages_the_rows_and_downloads_only_new_and_changed_notes() {
        let (client, seen, server, done) = serve(|path| {
            if path == "/api/v1/notes/summaries?limit=100" {
                return (
                    200,
                    page_json(
                        &[
                            row_json(1, "2026-11-12T10:00:00.000Z", 1),
                            row_json(2, "2026-11-12T12:00:00.000Z", 1),
                        ],
                        Some("page_2-x"),
                    ),
                );
            }
            if path == "/api/v1/notes/summaries?limit=100&cursor=page_2-x" {
                // A note that moved between the two requests is listed twice; it is still one note.
                return (
                    200,
                    page_json(
                        &[
                            row_json(2, "2026-11-12T12:00:00.000Z", 1),
                            row_json(3, "2026-11-12T10:00:00.000Z", 1),
                            row_json(4, "2026-11-12T10:00:00.000Z", 1),
                        ],
                        None,
                    ),
                );
            }
            // Deleted on the server after it was listed.
            if path == format!("/api/v1/notes/{}", id(4)) {
                return (404, r#"{"error":"not_found","message":"Note not found"}"#.into());
            }
            for n in 1..=3 {
                if path == format!("/api/v1/notes/{}", id(n)) {
                    return (200, note_json(n, "2026-11-12T12:00:00.000Z", 1));
                }
            }
            (500, "{}".into())
        });
        let kept = HashMap::from([
            (id(1), sync_stamp("2026-11-12T10:00:00.000Z", 1, None)),
            (id(2), sync_stamp("2026-11-12T10:00:00.000Z", 1, None)),
        ]);
        let mut delivered = Vec::new();
        let mut progress = Vec::new();
        refresh_notes(&client, &kept, &[], |notes, read, wanted| {
            delivered.extend(notes.into_iter().map(|note| (note.id, note.body)));
            progress.push((read, wanted));
        }, |_| {})
        .unwrap();
        *done.lock().unwrap() = true;
        server.join().unwrap();
        // Note 1 is unchanged and is never downloaded; 2 changed, 3 is new, 4 was deleted meanwhile.
        assert_eq!(
            delivered,
            vec![(id(2), "Body of 2".to_string()), (id(3), "Body of 3".to_string())]
        );
        assert_eq!(progress, vec![(3, 3)]);
        assert_eq!(
            *seen.lock().unwrap(),
            vec![
                "GET /api/v1/notes/summaries?limit=100".to_string(),
                "GET /api/v1/notes/summaries?limit=100&cursor=page_2-x".to_string(),
                format!("GET /api/v1/notes/{}", id(2)),
                format!("GET /api/v1/notes/{}", id(3)),
                format!("GET /api/v1/notes/{}", id(4)),
            ]
        );
    }

    #[test]
    fn a_refresh_with_nothing_new_downloads_no_note() {
        let (client, seen, server, done) = serve(|path| {
            if path.starts_with("/api/v1/notes/summaries") {
                (200, page_json(&[row_json(1, "2026-11-12T10:00:00.000Z", 1)], None))
            } else {
                (500, "{}".into())
            }
        });
        let kept = HashMap::from([(id(1), sync_stamp("2026-11-12T10:00:00.000Z", 1, None))]);
        let mut batches = 0;
        refresh_notes(&client, &kept, &[], |_, _, _| batches += 1, |_| {}).unwrap();
        *done.lock().unwrap() = true;
        server.join().unwrap();
        assert_eq!(batches, 0);
        assert_eq!(*seen.lock().unwrap(), vec!["GET /api/v1/notes/summaries?limit=100".to_string()]);
    }

    #[test]
    fn a_server_without_the_list_endpoint_is_asked_for_every_note_as_before() {
        let (client, seen, server, done) = serve(|path| {
            if path.starts_with("/api/v1/notes/summaries") {
                // What a server from before the endpoint answers: the route reads as a note ID.
                return (400, r#"{"_tag":"HttpApiDecodeError","message":"Expected a UUID"}"#.into());
            }
            if path == "/api/v1/notes" {
                return (
                    200,
                    format!(
                        "[{},{}]",
                        note_json(1, "2026-11-12T10:00:00.000Z", 1),
                        note_json(2, "2026-11-12T10:00:00.000Z", 1)
                    ),
                );
            }
            (500, "{}".into())
        });
        let mut delivered = Vec::new();
        refresh_notes(&client, &HashMap::new(), &[], |notes, read, wanted| {
            delivered.push((notes.len(), read, wanted))
        }, |_| {})
        .unwrap();
        *done.lock().unwrap() = true;
        server.join().unwrap();
        assert_eq!(delivered, vec![(2, 2, 2)]);
        assert_eq!(
            *seen.lock().unwrap(),
            vec![
                "GET /api/v1/notes/summaries?limit=100".to_string(),
                "GET /api/v1/notes".to_string()
            ]
        );
    }

    #[test]
    fn a_failure_part_way_keeps_what_was_read_and_reports_the_error() {
        let (client, _seen, server, done) = serve(|path| {
            if path.starts_with("/api/v1/notes/summaries") {
                return (
                    200,
                    page_json(
                        &[
                            row_json(1, "2026-11-12T10:00:00.000Z", 1),
                            row_json(2, "2026-11-12T10:00:00.000Z", 1),
                        ],
                        None,
                    ),
                );
            }
            if path == format!("/api/v1/notes/{}", id(1)) {
                return (200, note_json(1, "2026-11-12T10:00:00.000Z", 1));
            }
            (503, r#"{"error":"unavailable","message":"The note service is temporarily unavailable"}"#.into())
        });
        let mut delivered = Vec::new();
        let result = refresh_notes(&client, &HashMap::new(), &[], |notes, read, wanted| {
            delivered.push((notes.into_iter().map(|note| note.id).collect::<Vec<_>>(), read, wanted))
        }, |_| {});
        *done.lock().unwrap() = true;
        server.join().unwrap();
        assert_eq!(
            result.map(|_| ()),
            Err("The note service is temporarily unavailable".to_string())
        );
        assert_eq!(delivered, vec![(vec![id(1)], 1, 2)]);
    }

    #[test]
    fn a_cursor_that_is_not_one_is_never_put_into_a_request() {
        let (client, seen, server, done) = serve(|path| {
            if path == "/api/v1/notes/summaries?limit=100" {
                (200, page_json(&[row_json(1, "2026-11-12T10:00:00.000Z", 1)], Some("x&limit=1 /../admin")))
            } else {
                (500, "{}".into())
            }
        });
        assert!(client.note_rows().is_err());
        *done.lock().unwrap() = true;
        server.join().unwrap();
        assert_eq!(seen.lock().unwrap().len(), 1);
    }
}


/// Notes of an account against its own server: the Trash rule, a restored
/// note, and ids that are never reused.
#[cfg(test)]
mod account_tests {
    use super::*;
    // `gpui::*` re-exports its own `test` attribute; these are plain unit tests.
    use std::prelude::v1::test;
    use crate::accounts::client::testing::{Fake, json};

    const TOKEN: &str = "fictional-account-note-token-000000000000";

    fn note_json(id: &str, title: &str) -> String {
        serde_json::json!({
            "id": id, "title": title, "body": "", "enhanced": "", "date": "2026-11-16", "duration": "",
            "createdAt": "2026-11-16T10:00:00Z", "updatedAt": "2026-11-16T10:00:00Z",
            "recordingState": "idle", "generationState": "idle", "transcriptRevision": 0,
            "generatedRevision": null, "transcriptSegments": []
        })
        .to_string()
    }

    fn row(id: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id, "title": "Row", "date": "2026-11-16", "duration": "",
            "createdAt": "2026-11-16T10:00:00Z", "updatedAt": "2026-11-16T10:00:00Z",
            "recordingState": "idle", "generationState": "idle", "transcriptRevision": 0,
            "generatedRevision": null, "hasBody": true, "hasSummary": false, "hasTranscript": false,
            "speakerCount": 0, "excerpt": "", "meeting": null,
            "attendees": {"people": [], "others": 0, "total": 0}, "organizationId": null, "folders": []
        })
    }

    fn page(rows: &[serde_json::Value]) -> String {
        serde_json::json!({"notes": rows, "nextCursor": null, "total": rows.len()}).to_string()
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("cereal-server-{name}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        root
    }

    const A: &str = "3a49a4af-c660-42e9-a36a-a806fbb41e01";
    const B: &str = "3a49a4af-c660-42e9-a36a-a806fbb41e02";
    const C: &str = "3a49a4af-c660-42e9-a36a-a806fbb41e03";
    const D: &str = "3a49a4af-c660-42e9-a36a-a806fbb41e04";

    #[test]
    fn v4_only_a_404_for_the_note_itself_after_a_complete_listing_marks_it_gone() {
        // The server lists A only. Of the notes kept here, B is gone (404),
        // C is there after all (the listing was short), and D's request
        // fails: only B is concluded to be gone.
        let server = Fake::start(|request| match request.path.as_str() {
            path if path.starts_with("/api/v1/notes/summaries") => json(200, page(&[row(A)])),
            path if path == format!("/api/v1/notes/{B}") => json(404, r#"{"error":"not_found","message":"Note not found"}"#),
            path if path == format!("/api/v1/notes/{C}") => json(200, note_json(C, "Still here")),
            path if path == format!("/api/v1/notes/{D}") => (503, "text/html", "<html>busy</html>".into(), vec![]),
            path if path == format!("/api/v1/notes/{A}") => json(200, note_json(A, "Listed")),
            _ => json(500, "{}"),
        });
        let client = Client::for_test(&server.url, TOKEN, None);
        let mut delivered = Vec::new();
        let mut listed = std::collections::HashSet::new();
        // Candidates are compared whatever the case of their ids.
        let candidates = [A.to_uppercase(), B.into(), C.into(), D.into()];
        let gone = refresh_notes(
            &client,
            &HashMap::new(),
            &candidates,
            |notes, _, _| delivered.extend(notes.into_iter().map(|note| note.id)),
            |present| listed = present,
        )
        .unwrap();
        assert_eq!(gone.missing, [B.to_string()]);
        assert_eq!(delivered, [A.to_string(), C.to_string()]);
        assert!(listed.contains(A));
        assert!(!server.paths().contains(&format!("GET /api/v1/notes/{}", A.to_uppercase())), "a listed note is not asked for");

        // A listing with a row whose id is not a UUID is incomplete:
        // nothing is asked, nothing is concluded.
        let skipped = Fake::start(|request| {
            if request.path.starts_with("/api/v1/notes/summaries") {
                json(200, page(&[row(A), row("not-a-uuid")]))
            } else {
                json(404, r#"{"error":"not_found","message":"Note not found"}"#)
            }
        });
        let client = Client::for_test(&skipped.url, TOKEN, None);
        let gone = refresh_notes(&client, &HashMap::new(), &[B.into()], |_, _, _| {}, |_| {}).unwrap();
        assert!(gone.missing.is_empty());
        assert!(!skipped.paths().contains(&format!("GET /api/v1/notes/{B}")));

        // A listing that fails part way concludes nothing either.
        let failing = Fake::start(|_| json(503, r#"{"error":"unavailable"}"#));
        let client = Client::for_test(&failing.url, TOKEN, None);
        assert!(refresh_notes(&client, &HashMap::new(), &[B.into()], |_, _, _| {}, |_| {}).is_err());

        // The marks: nothing is erased, a note with unsent work is left
        // alone, and a note that is listed again comes back.
        let mut note = Note {
            id: 7,
            title: "Gone on the server".into(),
            body: "Its text".into(),
            server_id: Some(B.into()),
            folder_id: Some(3),
            favorite: true,
            ..Default::default()
        };
        assert!(mark_missing(&mut note, "2026-11-16T10:00:00Z"));
        assert!(note.trashed);
        assert_eq!(note.server_missing_since.as_deref(), Some("2026-11-16T10:00:00Z"));
        assert_eq!((note.body.as_str(), note.folder_id, note.favorite), ("Its text", Some(3), true));
        assert!(mark_listed(&mut note));
        assert!(!note.trashed && note.server_missing_since.is_none());
        let mut unsent = Note { unsent_edits: true, ..Default::default() };
        assert!(!mark_missing(&mut unsent, "now"));
        assert!(!unsent.trashed);
        let mut capturing = Note { unsent_capture: true, ..Default::default() };
        assert!(!mark_missing(&mut capturing, "now"));
    }

    #[test]
    fn r1_a_restored_note_deleted_on_the_server_is_created_there_again() {
        let created = "3a49a4af-c660-42e9-a36a-a806fbb41e99";
        let server = Fake::start(move |request| match (request.method.as_str(), request.path.as_str()) {
            ("POST", "/api/v1/notes") => json(200, note_json(created, "Restored")),
            (_, path) if path == format!("/api/v1/notes/{created}") => json(200, note_json(created, "Restored")),
            // The old note is gone.
            _ => json(404, r#"{"error":"not_found","message":"Note not found"}"#),
        });
        let root = scratch("r1");
        let mut client = Client::for_test(&server.url, TOKEN, Some(&root));
        let id = handle::handle(1, 5).unwrap();
        let mut note = Note {
            id,
            title: "Restored".into(),
            body: "Kept text".into(),
            server_id: Some(B.into()),
            server_url: Some(server.url.clone()),
            ..Default::default()
        };
        // The worker had known it, and its sidecar was on disk.
        client.ids.insert(id, B.into());
        let sidecar = association_file(&root, &server.url, id);
        fs::create_dir_all(sidecar.parent().unwrap()).unwrap();
        fs::write(&sidecar, B).unwrap();
        // Trash marked it; restore detaches it.
        mark_missing(&mut note, "2026-11-16T10:00:00Z");
        let url = detach_from_server(&mut note).unwrap();
        fs::remove_file(association_file(&root, &url, id)).unwrap();
        assert!(!note.trashed && note.server_id.is_none() && note.server_url.is_none());
        assert!(note.unsent_edits, "it is saved at once");
        let reply = client.run(&note, Action::Save).unwrap();
        assert_eq!(reply.note.id, created);
        assert_eq!(
            server.paths(),
            [
                "POST /api/v1/notes".to_string(),
                format!("PATCH /api/v1/notes/{created}"),
                format!("GET /api/v1/notes/{created}"),
            ]
        );
        assert_eq!(fs::read_to_string(&sidecar).unwrap(), created);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn h3_a_new_note_is_created_on_the_server_and_adopts_no_sidecar_of_an_old_id() {
        let created = "3a49a4af-c660-42e9-a36a-a806fbb41e98";
        let server = Fake::start(move |request| match (request.method.as_str(), request.path.as_str()) {
            ("POST", "/api/v1/notes") => json(200, note_json(created, "New")),
            _ => json(200, note_json(created, "New")),
        });
        let root = scratch("h3");
        // The old note 3 left a sidecar and a capture directory behind.
        let old = handle::handle(1, 3).unwrap();
        let old_sidecar = association_file(&root, &server.url, old);
        fs::create_dir_all(old_sidecar.parent().unwrap()).unwrap();
        fs::write(&old_sidecar, A).unwrap();
        fs::create_dir_all(root.join("note-3")).unwrap();
        // Its id is never handed out again (`model::account_tests::h3_…`),
        // so the next note is 4, which nothing on disk names.
        let mut client = Client::for_test(&server.url, TOKEN, Some(&root));
        let note = Note { id: handle::handle(1, 4).unwrap(), title: "New".into(), ..Default::default() };
        assert_eq!(client.ensure(&note).unwrap(), created);
        assert_eq!(server.paths(), ["POST /api/v1/notes"]);
        assert_eq!(fs::read_to_string(association_file(&root, &server.url, note.id)).unwrap(), created);
        assert_eq!(fs::read_to_string(&old_sidecar).unwrap(), A, "the old one is untouched");
        // On disk the sidecar is named by the file id.
        assert!(association_file(&root, &server.url, note.id).ends_with("4.txt"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_note_is_only_ever_sent_to_its_own_account() {
        let server = Fake::start(|_| json(200, "{}"));
        let root = scratch("own");
        let mut client = Client::for_test(&server.url, TOKEN, Some(&root));
        client.slot = Some(1);
        let theirs = Note { id: handle::handle(2, 1).unwrap(), ..Default::default() };
        assert!(client.ensure(&theirs).is_err());
        let elsewhere = Note {
            id: handle::handle(1, 1).unwrap(),
            server_id: Some(A.into()),
            server_url: Some("https://other.example".into()),
            ..Default::default()
        };
        assert!(client.ensure(&elsewhere).is_err());
        assert!(server.requests().is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn s3_what_was_recorded_while_the_session_had_ended_is_sent_after_sign_in() {
        let directory = scratch("s3");
        let manifest_path = directory.join("manifest.json");
        let manifest: crate::audio::CaptureManifest = serde_json::from_value(serde_json::json!({
            "version": 1, "capture_id": "capture-1", "note_id": 1, "directory": directory,
            "manifest_path": manifest_path, "started_at": "2026-11-16T10:00:00Z",
            "finished_at": null, "stats": {}, "error": null
        }))
        .unwrap();
        fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        let note = Note {
            id: handle::handle(1, 1).unwrap(),
            audio_captures: vec![manifest.clone()],
            unsent_capture: true,
            ..Default::default()
        };
        // Still recording: nothing is queued.
        assert!(requeue_action(&note, true).is_none());
        // Stopped: the capture is final.
        let mut finished = manifest;
        finished.finished_at = Some("2026-11-16T10:40:00Z".into());
        fs::write(&manifest_path, serde_json::to_vec(&finished).unwrap()).unwrap();
        // Back after the sign-in, its parts are sent with the identities
        // they always had (the upload after Stop).
        assert!(matches!(requeue_action(&note, true), Some(Action::Sync)));
        // At an ordinary launch a capture is not sent by itself, as before.
        assert!(requeue_action(&note, false).is_none());
        // An unsent edit is sent either way.
        let edited = Note { unsent_edits: true, ..note.clone() };
        assert!(matches!(requeue_action(&edited, false), Some(Action::Save)));
        // A note its server deleted is not sent at all.
        let gone = Note { trashed: true, server_missing_since: Some("now".into()), ..note };
        assert!(requeue_action(&gone, true).is_none());
        let _ = fs::remove_dir_all(directory);
    }
}
