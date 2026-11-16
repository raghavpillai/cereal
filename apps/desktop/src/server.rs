//! Native client of our owner-authenticated API. Provider credentials never enter
//! this process. One serial worker per account preserves Start/Stop/Resume
//! ordering; GPUI only polls completions. Recordings stay on this Mac as WAV
//! files; the transcript is the one the live stream delivered, and no audio
//! is uploaded.
//!
//! Every request here goes through the client of the account that owns the
//! thing it is about (`accounts::AccountClient`): a note's account, an
//! event's account. There is no "current" account to fall back to.
use crate::{
    accounts::{AccountClient as Client, AccountId, handle},
    app::Cereal,
    model::Note,
};
use gpui::*;
use meeting_notes_api_types::{ChatAnswerDTO, NoteDTO};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    fs,
    io::{BufRead, BufReader, Read, Write},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
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
        note.add_transcript_gaps(remote.transcript_gaps.clone().unwrap_or_default());
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
    /// The next piece of the model's own reasoning (`ChatReasoningDTO`).
    Reasoning(String),
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
        Type::Reasoning if value.text.len() <= 1_048_576 => {
            Some(ChatStreamEvent::Reasoning(value.text))
        }
        Type::Reasoning => None,
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

/// The chat models a server offers, leaving out any this build cannot read (a provider added on a newer
/// server) instead of losing the whole list.
pub fn chat_models_from(response: &Value) -> Vec<meeting_notes_api_types::LanguageModelSelection> {
    response.get("models").and_then(Value::as_array).map(|models| models.iter().filter_map(|model| serde_json::from_value(model.clone()).ok()).collect()).unwrap_or_default()
}
/// Independent, cancellable chat transport. Recording Stop never waits
/// behind an open language-model stream. The owner token travels through stdin.
pub fn chat_model_key(choice: &meeting_notes_api_types::LanguageModelSelection) -> String {
    use meeting_notes_api_types::LanguageModelSelectionProvider as Provider;
    let provider = match choice.provider { Provider::Openrouter => "openrouter", Provider::Openai => "openai", Provider::Anthropic => "anthropic", Provider::OpenaiCompatible => "openai_compatible" };
    format!("{provider}:{}", choice.model)
}
/// A chat about a note goes to the note's account. A chat across meetings
/// belongs to one account: `scope`, or the default account when none is
/// named (until the scope can be chosen, section 4.2).
pub fn stream_chat_with_model(
    note: Option<Note>, scope: Option<AccountId>, message: String, history: Vec<Value>, cancel: Arc<AtomicBool>, model: Option<String>,
    // The saved thread and exchange it is asked as (`chat_sync.rs`); `None`: the old endpoints.
    thread: Option<crate::chat_sync::ThreadTurn>,
) -> mpsc::Receiver<ChatStreamEvent> {
    let client = match (&note, &scope) {
        (Some(note), _) => crate::accounts::client_of_note(note.id),
        (None, Some(account)) => crate::accounts::client(account),
        (None, None) => crate::accounts::default_client(),
    };
    stream_chat_with_client(client, note, message, history, cancel, model, thread)
}
fn stream_chat_with_client(
    client: Result<Client, String>,
    note: Option<Note>,
    message: String,
    history: Vec<Value>,
    cancel: Arc<AtomicBool>,
    model: Option<String>,
    thread: Option<crate::chat_sync::ThreadTurn>,
) -> mpsc::Receiver<ChatStreamEvent> {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let run = || -> Result<(), String> {
            let mut client = client?;
            if cancel.load(Ordering::Acquire) {
                return Ok(());
            }
            let mut server_note = None;
            let path = if let Some(note) = note {
                let id = client.ensure(&note)?;
                server_note = Some(id.clone());
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
            let (path, payload) = if let Some(thread) = &thread {
                // Saved on the server as it arrives, as this exchange of the thread.
                crate::chat_sync::stream_request(&client, thread, server_note.as_deref(), &message, model.as_deref())?
            } else {
                let mut payload = json!({"message":message,"history":history,"includeSources":true});
                if let Some(model) = &model { payload["model"] = json!(model); }
                if let Ok(time_zone) = iana_time_zone::get_timezone() {
                    payload["timeZone"] = json!(time_zone);
                }
                (path, payload)
            };
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

/// How a generation is coming along, for the window to show as it happens.
#[derive(Debug, Clone, PartialEq)]
pub enum GenerateProgress {
    /// The notes have been asked for: the note is saved and stopped.
    Began,
    /// The name the server gave a note nobody had named.
    Title(String),
    /// The next piece of the notes' Markdown.
    Delta(String),
    /// This server does not stream: what was shown is dropped, and the
    /// notes arrive whole.
    Buffered,
}

const GENERATION_FAILED: &str = "Notes could not be generated. Try again.";

enum GenerationFrame {
    Progress(GenerateProgress),
    Done(Box<NoteDTO>),
    Error(String),
}

fn decode_generation_frame(data: &str) -> Option<GenerationFrame> {
    use meeting_notes_api_types::{GenerationStreamEventDTO, GenerationStreamEventDtoType as Type};
    let value: GenerationStreamEventDTO = serde_json::from_str(data).ok()?;
    match value.generation_stream_event_dto_type {
        Type::Delta if value.text.len() <= 1_048_576 => {
            Some(GenerationFrame::Progress(GenerateProgress::Delta(value.text)))
        }
        Type::Delta => None,
        Type::Title => Some(GenerationFrame::Progress(GenerateProgress::Title(
            value.text.chars().take(200).collect(),
        ))),
        Type::Done => value.note.map(|note| GenerationFrame::Done(Box::new(note))),
        Type::Error => Some(GenerationFrame::Error(value.text.chars().take(1000).collect())),
    }
}

#[derive(Clone)]
pub enum Action {
    Save,
    Recording(&'static str),
    /// Stop the recording, if it is still going, and write the notes from
    /// the transcript; a note with no transcript is left as it is (see
    /// [`Reply::untranscribed`]).
    Generate,
    // Retained for the explicitly opt-in legacy non-streaming integration test.
    // Interactive clients use the independent cancellable SSE transport.
    #[allow(dead_code)]
    Chat {
        message: String,
        history: Vec<Value>,
    },
    Share,
    // Kept for a caller that turns every link of a note off at once; the Share popover narrows what links
    // reach instead ("Only people invited").
    #[allow(dead_code)]
    RevokeShares,
}
pub struct Reply {
    pub note: NoteDTO,
    pub url: String,
    pub answer: Option<String>,
    pub share_url: Option<String>,
    /// Notes were asked for, and the recording had no transcript to write
    /// them from: nothing was generated.
    pub untranscribed: bool,
}
struct Job {
    note: Note,
    action: Action,
    reply: mpsc::Sender<Result<Reply, String>>,
    /// Who hears how a generation is coming along, if anyone.
    progress: Option<mpsc::Sender<GenerateProgress>>,
}
/// What an account's serial worker is asked to do: an [`Action`] on a note,
/// or deleting a note on the server for good (Delete forever in Trash). They
/// share one queue, so a delete runs after anything sent for the note before.
enum Work {
    Run(Job),
    Delete {
        note: Note,
        /// Delete even though the server's copy changed ("Delete anyway").
        force: bool,
        /// The server deletes only if the note is still what was read
        /// (`conditional-delete` among its features).
        conditional: bool,
        reply: mpsc::Sender<Result<(), DeleteFailure>>,
    },
}

/// Why Delete forever did not delete a note on its server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeleteFailure {
    /// The server's copy was changed (on another device) since this Mac
    /// last had it: nothing was deleted.
    Changed,
    /// Anything else, in words for the person. Nothing was deleted.
    Failed(String),
}
pub struct Bridge {
    sender: mpsc::Sender<Work>,
}
impl Bridge {
    /// The serial worker of one account. Its client is made again whenever
    /// the account's epoch has moved (a replaced token), keeping what it
    /// knows of which notes are on the server.
    pub fn new(account: AccountId) -> Self {
        let (sender, receiver) = mpsc::channel::<Work>();
        std::thread::spawn(move || {
            let mut client: Option<Client> = None;
            while let Ok(work) = receiver.recv() {
                if client.as_ref().is_none_or(|client| client.stale()) {
                    let ids = client.take().map(|client| client.ids).unwrap_or_default();
                    client = match crate::accounts::client(&account) {
                        Ok(mut fresh) => {
                            fresh.ids = ids;
                            Some(fresh)
                        }
                        Err(error) => {
                            match work {
                                Work::Run(job) => drop(job.reply.send(Err(error))),
                                Work::Delete { reply, .. } => {
                                    drop(reply.send(Err(DeleteFailure::Failed(error))))
                                }
                            }
                            continue;
                        }
                    };
                }
                let client = client.as_mut().expect("the client was just made");
                match work {
                    Work::Run(job) => {
                        let _ = job
                            .reply
                            .send(client.run_with(&job.note, job.action, job.progress.as_ref()));
                    }
                    Work::Delete { note, force, conditional, reply } => {
                        let _ = reply.send(client.delete(&note, force, conditional));
                    }
                }
            }
        });
        Self { sender }
    }
    fn send(
        &self,
        note: Note,
        action: Action,
        progress: Option<mpsc::Sender<GenerateProgress>>,
    ) -> mpsc::Receiver<Result<Reply, String>> {
        let (reply, receiver) = mpsc::channel();
        let _ = self.sender.send(Work::Run(Job {
            note,
            action,
            reply,
            progress,
        }));
        receiver
    }
    /// Delete `note` on its server for good, after whatever was sent for it
    /// before. The answer is `Ok` once the server no longer has it.
    pub(crate) fn delete(
        &self,
        note: Note,
        force: bool,
        conditional: bool,
    ) -> mpsc::Receiver<Result<(), DeleteFailure>> {
        let (reply, receiver) = mpsc::channel();
        let _ = self.sender.send(Work::Delete { note, force, conditional, reply });
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
    /// Delete a note on the server for good (Delete forever in Trash).
    /// Nothing is created or saved first: a note the server never had is
    /// simply forgotten.
    ///
    /// Unless `force`, the server's copy is read first and the note is
    /// deleted only if it is still what this Mac last had (the stamp of the
    /// last refresh, or the same text and revisions); otherwise nothing is
    /// deleted and [`DeleteFailure::Changed`] says so. With `conditional`
    /// the DELETE carries that stamp (`ifStamp`), so an edit landing between
    /// the read and the delete is not lost either (409).
    ///
    /// A 404 counts as "already deleted" only from a server that has
    /// conditional deletes, in its own words (`not_found`); any other 404
    /// (an older server, a proxy) is an error and the note stays.
    fn delete(&mut self, note: &Note, force: bool, conditional: bool) -> Result<(), DeleteFailure> {
        let failed = |message: &str| DeleteFailure::Failed(message.into());
        if self.stale() {
            return Err(failed("Account changed; request cancelled"));
        }
        if let (Some(mine), Some(theirs)) = (self.slot, handle::slot_of(note.id))
            && mine != theirs
        {
            return Err(failed("This note belongs to another account"));
        }
        if note.server_id.is_some() && note.server_url.as_deref() != Some(&self.url) {
            return Err(failed("This note belongs to a different server. Restore its server configuration before deleting it."));
        }
        let sidecar = self.association_path(note.id).map_err(DeleteFailure::Failed)?;
        let known = self
            .ids
            .get(&note.id)
            .cloned()
            .or_else(|| note.server_id.clone())
            .or_else(|| fs::read_to_string(&sidecar).ok().map(|id| id.trim().to_owned()));
        let forget = |client: &mut Self, id: Option<&str>| {
            if let (Some(id), Some(directory)) = (id, sidecar.parent()) {
                let _ = fs::remove_dir_all(directory.join("uploads").join(id));
            }
            client.ids.remove(&note.id);
            let _ = fs::remove_file(&sidecar);
        };
        let Some(known) = known else {
            // Never sent: there is nothing on the server to delete.
            forget(self, None);
            return Ok(());
        };
        let id = crate::accounts::client::path_id(&known)
            .map_err(|_| failed("This note's server id cannot be read. It is still in Trash."))?;
        let gone = |error: &crate::accounts::client::RequestError| {
            conditional
                && matches!(error, crate::accounts::client::RequestError::Status { code: 404, error: Some(code), .. } if code == "not_found")
        };
        let unconfirmed = "The server did not confirm that it no longer has this note. It is still in Trash.";
        let base = format!("/api/v1/notes/{id}");
        let mut path = base.clone();
        if !force {
            match self.send::<NoteDTO>("GET", &base, None, None) {
                Ok(remote) => {
                    let stamp = sync_stamp(&remote.updated_at, remote.transcript_revision, remote.generated_revision);
                    if note.server_sync_stamp.as_deref() != Some(stamp.as_str()) && !same_content(note, &remote) {
                        return Err(DeleteFailure::Changed);
                    }
                    if conditional {
                        path = format!("{base}?ifStamp={}", query_value(&delete_stamp(&remote)));
                    }
                }
                Err(error) if gone(&error) => {
                    forget(self, Some(&id));
                    return Ok(());
                }
                Err(error) if error.is_not_found() => return Err(failed(unconfirmed)),
                Err(error) => return Err(DeleteFailure::Failed(error.message())),
            }
        }
        match self.send::<Value>("DELETE", &path, None, None) {
            Ok(_) => {}
            Err(crate::accounts::client::RequestError::Status { code: 409, .. }) if !force => {
                return Err(DeleteFailure::Changed);
            }
            Err(error) if gone(&error) => {}
            Err(error) if error.is_not_found() => return Err(failed(unconfirmed)),
            Err(error) => return Err(DeleteFailure::Failed(error.message())),
        }
        forget(self, Some(&id));
        Ok(())
    }
    #[cfg(test)]
    fn run(&mut self, note: &Note, action: Action) -> Result<Reply, String> {
        self.run_with(note, action, None)
    }

    /// [`Client::run`], telling `progress` how a generation is coming
    /// along (see [`GenerateProgress`]).
    fn run_with(
        &mut self,
        note: &Note,
        action: Action,
        progress: Option<&mpsc::Sender<GenerateProgress>>,
    ) -> Result<Reply, String> {
        let id = self.ensure(note)?;
        let base = format!("/api/v1/notes/{id}");
        let saved: NoteDTO = self.request("PATCH", &base, Some(note_update_payload(note)))?;
        let mut answer = None;
        let mut share_url = None;
        let mut untranscribed = false;
        match action {
            Action::Save => {}
            Action::Recording(command) => {
                let current: NoteDTO = self.request("GET", &base, None)?;
                let command = recording_command(command, &current.recording_state);
                if command == "stop" {
                    self.stop(note, &base)?;
                } else {
                    let _: NoteDTO = self.request(
                        "POST",
                        &format!("{base}/recording"),
                        Some(json!({"command":command})),
                    )?;
                }
            }
            Action::Generate => {
                // A note still recording is stopped first: once the server
                // answers, the live session has stored its last finals, and
                // the notes are written from those. One that is already
                // stopped (Generate pressed after Stop) is what the save
                // above returned: nothing more is asked before the notes.
                let current = if saved.recording_state == meeting_notes_api_types::RecordingState::Stopped {
                    saved
                } else {
                    self.stop(note, &base)?;
                    self.request("GET", &base, None)?
                };
                if current
                    .transcript_segments
                    .iter()
                    .all(|segment| segment.text.trim().is_empty())
                {
                    stop_trace("there is no transcript to write notes from");
                    untranscribed = true;
                } else {
                    let written = self.generate(&base, current.transcript_revision, progress)?;
                    stop_trace("the notes are written");
                    // The note as the generation saved it: nothing to read again.
                    return Ok(Reply {
                        note: written,
                        url: self.url.clone(),
                        answer,
                        share_url,
                        untranscribed,
                    });
                }
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
            untranscribed,
        })
    }

    /// Have the notes of the note at `base` written from transcript
    /// `revision`, and return the note as saved. The notes are streamed
    /// (`/generate/stream`): `progress` hears the title and each piece of
    /// text as the model writes it. The server finishes and saves the notes
    /// whether or not this connection lasts.
    ///
    /// A stream that says nothing at all (a server older than the endpoint,
    /// or a refusal, whose reason the transport does not pass on) is asked
    /// again the buffered way, which answers with the notes or the reason.
    fn generate(
        &self,
        base: &str,
        revision: i64,
        progress: Option<&mpsc::Sender<GenerateProgress>>,
    ) -> Result<NoteDTO, String> {
        let payload = json!({"expectedTranscriptRevision": revision});
        let tell = |event: GenerateProgress| {
            if let Some(progress) = progress {
                let _ = progress.send(event);
            }
        };
        let mut child = self.event_stream_with_status(&format!("{base}/generate/stream"), &payload)?;
        let finish = |child: &mut std::process::Child| {
            let _ = child.kill();
            let _ = child.wait();
        };
        let Some(output) = child.stdout.take() else {
            finish(&mut child);
            return Err(GENERATION_FAILED.into());
        };
        tell(GenerateProgress::Began);
        let mut heard = false;
        let mut input = BufReader::new(output);
        let (mut line, mut data, mut bytes) = (String::new(), Vec::<String>::new(), 0);
        loop {
            line.clear();
            match input.read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(count) => bytes += count,
            }
            if bytes > 8 * 1024 * 1024 {
                finish(&mut child);
                return Err(GENERATION_FAILED.into());
            }
            let text = line.trim_end_matches(['\r', '\n']);
            if let Some(value) = text.strip_prefix("data:") {
                data.push(value.strip_prefix(' ').unwrap_or(value).to_string());
                continue;
            }
            if !text.is_empty() || data.is_empty() {
                continue;
            }
            let frame = data.join("\n");
            data.clear();
            heard = true;
            match decode_generation_frame(&frame) {
                Some(GenerationFrame::Progress(event)) => tell(event),
                Some(GenerationFrame::Done(note)) => {
                    finish(&mut child);
                    return Ok(*note);
                }
                Some(GenerationFrame::Error(message)) => {
                    finish(&mut child);
                    return Err(message);
                }
                None => {
                    finish(&mut child);
                    return Err(GENERATION_FAILED.into());
                }
            }
        }
        finish(&mut child);
        if heard {
            // The server carries on and saves them; the next refresh of the
            // note brings them here.
            return Err("The connection was lost while the notes were being written. They will appear here once the server has finished them.".into());
        }
        tell(GenerateProgress::Buffered);
        self.request("POST", &format!("{base}/generate"), Some(payload))
    }

    /// Tell the server the recording of `note` has stopped. The window
    /// sends this only once the live stream has delivered every captured
    /// sample (`Cereal::live_stop`): the server ends the live session on
    /// Stop, and answers once that session has stored its last finals.
    fn stop(&self, _note: &Note, base: &str) -> Result<(), String> {
        stop_trace("the stop action has saved and read the note");
        let _: NoteDTO = self.request(
            "POST",
            &format!("{base}/recording"),
            Some(json!({"command":"stop"})),
        )?;
        stop_trace("the server has been told to stop");
        Ok(())
    }

    /// Where the timeline positions of a note's captures are kept. The
    /// folder is called `uploads` because the parts that once went up were
    /// numbered there too; positions written by earlier versions are read
    /// from the same files.
    fn placement_root(&self, note: &Note, id: &str) -> Result<std::path::PathBuf, String> {
        let root = self
            .association_path(note.id)?
            .parent()
            .ok_or("Invalid association directory")?
            .join("uploads")
            .join(id);
        fs::create_dir_all(&root).map_err(|_| "Cannot preserve capture positions")?;
        Ok(root)
    }

    /// Where a capture sits on the note's timeline. Decided once, the first
    /// time the capture is streamed, and kept in a small file, so every later
    /// connection of its stream (a reconnect, a restart of the app) places
    /// its audio at the same milliseconds. A capture nothing has placed yet
    /// starts at `offset_ms`, where the note's audio so far ends.
    fn capture_base(
        capture: &crate::audio::CaptureManifest,
        placement_root: &Path,
        offset_ms: u64,
    ) -> Result<u64, String> {
        let capture_key =
            uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, capture.capture_id.as_bytes())
                .to_string();
        let base_path = placement_root.join(format!("capture-{capture_key}.json"));
        let base_ms = fs::read(&base_path)
            .ok()
            .and_then(|data| serde_json::from_slice(&data).ok())
            .unwrap_or(offset_ms);
        save_mapping(&base_path, &base_ms)?;
        Ok(base_ms)
    }
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
/// One event of the streamed source summary (`/evidence/stream`).
#[derive(Debug, PartialEq)]
pub enum EvidenceStreamEvent {
    /// The next piece of the summary's text.
    Delta(String),
    /// The validated result: the summary as finally read, and its quotations.
    Done(meeting_notes_api_types::EvidenceDTO),
    /// Instead of `Done`. Nothing streamed before it is kept.
    Error(String),
}

const EVIDENCE_FAILED: &str = "Transcript context could not be loaded. Try again.";

fn decode_evidence_frame(data: &str) -> Option<EvidenceStreamEvent> {
    use meeting_notes_api_types::{EvidenceStreamEventDTO, EvidenceStreamEventDtoType as Type};
    let value: EvidenceStreamEventDTO = serde_json::from_str(data).ok()?;
    match value.evidence_stream_event_dto_type {
        Type::Delta if value.text.len() <= 1_048_576 => Some(EvidenceStreamEvent::Delta(value.text)),
        Type::Delta => None,
        Type::Done => value
            .evidence
            .filter(|evidence| {
                evidence.summary.chars().count() <= 20_000
                    && evidence.transcript_revision >= 0
                    && evidence.quotes.len() <= 8
                    && evidence.quotes.iter().all(|quote| {
                        uuid::Uuid::parse_str(&quote.segment_id).is_ok()
                            && quote.start_ms >= 0
                            && quote.text.chars().count() <= 20_000
                    })
            })
            .map(EvidenceStreamEvent::Done),
        Type::Error => Some(EvidenceStreamEvent::Error(
            value.text.chars().take(1000).collect(),
        )),
    }
}

/// The HTTP status the transport wrote last on its stderr; `None` when it
/// got no response at all.
fn http_status(stderr: &str) -> Option<u16> {
    let code = stderr.trim_end();
    let code = &code[code.len().saturating_sub(3)..];
    code.parse().ok().filter(|code| *code >= 100)
}

/// Read-only source context has its own pending state, independent of recording
/// commands and chat. Only the selected instance's server note can be queried.
///
/// The source summary of `paragraph`, streamed: deltas as the model writes,
/// then `Done` or `Error`, on the returned channel. Setting `cancel` kills
/// the transport, which closes the request and so the provider's. A server
/// that answers the stream with nothing at all (one older than the stream
/// endpoint) is asked the buffered way instead, and its summary arrives as
/// one delta.
pub fn stream_evidence(
    note: Note,
    paragraph: String,
    cancel: Arc<AtomicBool>,
) -> mpsc::Receiver<EvidenceStreamEvent> {
    stream_evidence_with_client(crate::accounts::client_of_note(note.id), note, paragraph, cancel)
}

fn stream_evidence_with_client(
    client: Result<Client, String>,
    note: Note,
    paragraph: String,
    cancel: Arc<AtomicBool>,
) -> mpsc::Receiver<EvidenceStreamEvent> {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        // The note's server, its id there and the request, or why there is none.
        let target = || -> Result<(Client, String, Value), String> {
            let client = client?;
            let id = note
                .server_id
                .as_deref()
                .ok_or("Save this note to the server before viewing transcript context.")?;
            if note.server_url.as_deref() != Some(client.url.as_str())
                || uuid::Uuid::parse_str(id).is_err()
            {
                return Err("This note is not connected to the selected server.".into());
            }
            let revision = i64::try_from(note.transcript_revision)
                .map_err(|_| "Invalid transcript revision")?;
            let payload = serde_json::to_value(meeting_notes_api_types::EvidenceRequestDTO {
                paragraph: paragraph.clone(),
                expected_transcript_revision: revision,
            })
            .map_err(|_| "Invalid source-context request")?;
            Ok((client, id.to_owned(), payload))
        };
        let (client, id, payload) = match target() {
            Ok(target) => target,
            Err(error) => {
                let _ = sender.send(EvidenceStreamEvent::Error(error));
                return;
            }
        };
        // `None` when the stream said something (and so has already ended
        // the card), else the HTTP status it ended with, if any.
        let run = || -> Result<Option<Option<u16>>, String> {
            if cancel.load(Ordering::Acquire) {
                return Ok(None);
            }
            let mut child = client.event_stream_with_status(&format!("/api/v1/notes/{id}/evidence/stream"), &payload)?;
            let status_output = child.stderr.take();
            let Some(output) = child.stdout.take() else {
                let _ = child.kill();
                let _ = child.wait();
                return Err(EVIDENCE_FAILED.into());
            };
            let events = sender.clone();
            let stopped = cancel.clone();
            // Whether any event arrived: only a stream that said nothing can
            // be a server without the endpoint.
            let reader = std::thread::spawn(move || -> bool {
                let mut heard = false;
                let mut input = BufReader::new(output);
                let mut line = String::new();
                let mut data = Vec::new();
                let mut bytes = 0;
                loop {
                    line.clear();
                    match input.read_line(&mut line) {
                        Ok(0) | Err(_) => break,
                        Ok(count) => {
                            bytes += count;
                            if bytes > 4 * 1024 * 1024 {
                                let _ = events.send(EvidenceStreamEvent::Error(EVIDENCE_FAILED.into()));
                                return true;
                            }
                        }
                    }
                    if stopped.load(Ordering::Acquire) {
                        return true;
                    }
                    let line = line.trim_end_matches(['\r', '\n']);
                    if line.is_empty() {
                        if data.is_empty() {
                            continue;
                        }
                        let frame = data.join("\n");
                        data.clear();
                        heard = true;
                        let event = decode_evidence_frame(&frame)
                            .unwrap_or_else(|| EvidenceStreamEvent::Error(EVIDENCE_FAILED.into()));
                        let last = !matches!(event, EvidenceStreamEvent::Delta(_));
                        if events.send(event).is_err() || last {
                            return true;
                        }
                    } else if let Some(value) = line.strip_prefix("data:") {
                        data.push(value.strip_prefix(' ').unwrap_or(value).to_string());
                    }
                }
                if heard && !stopped.load(Ordering::Acquire) {
                    let _ = events.send(EvidenceStreamEvent::Error(EVIDENCE_FAILED.into()));
                }
                heard
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
            let heard = reader.join().unwrap_or(true);
            let mut status = String::new();
            if let Some(mut stderr) = status_output {
                let _ = stderr.read_to_string(&mut status);
            }
            Ok(if heard { None } else { Some(http_status(&status)) })
        };
        match run() {
            Ok(None) => {}
            _ if cancel.load(Ordering::Acquire) => {}
            // Only a server that has no such endpoint is asked the buffered
            // way. Any other silent end (refused, signed out, unavailable, or
            // a connection lost once the model may have started) is reported,
            // never asked again.
            Ok(Some(Some(404 | 405))) => {
                let buffered: Result<meeting_notes_api_types::EvidenceDTO, String> = client
                    .request_with_cancel(
                        "POST",
                        &format!("/api/v1/notes/{id}/evidence"),
                        Some(payload),
                        Some(cancel.clone()),
                    )
                    .map_err(|_| EVIDENCE_FAILED.to_string());
                let event = match buffered {
                    Ok(evidence) => {
                        let _ = sender.send(EvidenceStreamEvent::Delta(evidence.summary.clone()));
                        EvidenceStreamEvent::Done(evidence)
                    }
                    Err(error) => EvidenceStreamEvent::Error(error),
                };
                if !cancel.load(Ordering::Acquire) {
                    let _ = sender.send(event);
                }
            }
            Ok(Some(_)) => {
                let _ = sender.send(EvidenceStreamEvent::Error(EVIDENCE_FAILED.into()));
            }
            Err(error) => {
                let _ = sender.send(EvidenceStreamEvent::Error(error));
            }
        }
    });
    receiver
}

/// Where the live transcript stream of a running capture connects, and where
/// that capture sits on the note's timeline. Runs on the stream's own thread.
///
/// The timeline position is decided here, once (see [`Client::capture_base`]),
/// so every connection of the stream places the same audio at the same
/// milliseconds.
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
            language: crate::transcript_panel::stream_language(),
        }),
        // The server did not answer, or an earlier capture is not final yet.
        Err(_) => Resolved::Later,
    }
}

/// For the live probe (`CEREAL_LIVE_PROBE`): where the time between Stop
/// and the notes being asked for goes, as lines on standard error with the
/// time of day.
pub(crate) fn stop_trace(event: &str) {
    if crate::environment::var_os("CEREAL_LIVE_PROBE").is_some() {
        eprintln!("stop path {}: {event}", chrono::Local::now().format("%H:%M:%S%.3f"));
    }
}

impl Client {
    /// Note-timeline position of the first sample of capture `capture_id`,
    /// while it is still being recorded: after the note's transcript so far
    /// (which may come from another device), and after every earlier capture
    /// of the note here, each placed as it was when it was streamed.
    fn live_capture_base(&self, note: &Note, id: &str, capture_id: &str) -> Result<u64, String> {
        let remote: NoteDTO = self.request("GET", &format!("/api/v1/notes/{id}"), None)?;
        let offset_ms = remote
            .transcript_segments
            .iter()
            .map(|segment| segment.end_ms.unwrap_or(segment.start_ms).max(0) as u64)
            .max()
            .unwrap_or(0);
        self.capture_base_after(note, id, capture_id, offset_ms)
    }

    /// The position of capture `capture_id` when the note's audio from
    /// elsewhere ends at `offset_ms`: the captures before it are placed
    /// first, in order, and then this one after them.
    fn capture_base_after(
        &self,
        note: &Note,
        id: &str,
        capture_id: &str,
        mut offset_ms: u64,
    ) -> Result<u64, String> {
        let placement_root = self.placement_root(note, id)?;
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
                // One that a quit or a crash left unfinished is as final as
                // it will get.
                if !current.settled() {
                    return Err("An earlier capture is still being finalised".into());
                }
                current
            };
            let base_ms = Self::capture_base(&capture, &placement_root, offset_ms)?;
            if live {
                return Ok(base_ms);
            }
            offset_ms = offset_ms.max(base_ms + (capture.audio_seconds() * 1000.).round() as u64);
        }
        Err("The capture does not belong to this note".into())
    }
}

/// Write `value` to `path` once: a file that is already there must hold the
/// same value, so a decision made earlier is never silently replaced.
fn save_mapping<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let bytes = serde_json::to_vec(value).map_err(|_| "Cannot encode capture position")?;
    if let Ok(previous) = fs::read(path) {
        return if previous == bytes {
            Ok(())
        } else {
            Err(
                "Saved capture position conflicts with this one; local audio remains intact"
                    .into(),
            )
        };
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "Cannot determine the time")?
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
                .map_err(|_| "Cannot preserve capture position")?;
            match fs::hard_link(&temporary, path) {
                Ok(()) => Ok(()),
                Err(e)
                    if e.kind() == std::io::ErrorKind::AlreadyExists
                        && fs::read(path).ok().as_deref() == Some(bytes.as_slice()) =>
                {
                    Ok(())
                }
                Err(_) => Err("Cannot preserve capture position".into()),
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            if fs::read(path).map_err(|_| "Cannot read capture position")? == bytes {
                Ok(())
            } else {
                Err(
                    "Saved capture position conflicts with this one; local audio remains intact"
                        .into(),
                )
            }
        }
        Err(_) => Err("Cannot preserve capture position".into()),
    };
    let _ = fs::remove_file(temporary);
    result
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

/// The server's copy holds nothing this Mac does not: the same text,
/// generated notes and revisions.
fn same_content(note: &Note, remote: &NoteDTO) -> bool {
    note.title == remote.title
        && note.body == remote.body
        && note.enhanced == remote.enhanced
        && note.transcript_revision == remote.transcript_revision.max(0) as u64
        && note.generated_revision == remote.generated_revision.map(|revision| revision.max(0) as u64)
}

/// The note as it was read, the way a conditional delete names it
/// (`updatedAt|transcriptRevision|generatedRevision`, the last empty when
/// there are no generated notes).
fn delete_stamp(remote: &NoteDTO) -> String {
    format!(
        "{}|{}|{}",
        remote.updated_at,
        remote.transcript_revision,
        remote.generated_revision.map_or_else(String::new, |revision| revision.to_string())
    )
}

/// A value for a query string: everything but unreserved characters is
/// percent-encoded.
fn query_value(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (byte as char).to_string(),
            _ => format!("%{byte:02X}"),
        })
        .collect()
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

/// What is queued again for a note when its account is Ready: the Stop of a
/// recording the server has not confirmed (which saves the text on its way),
/// or the edit.
///
/// A Stop is sent again only when the account comes back (`returning`: its
/// session ended, was replaced, or its server could not be reached), and only
/// once every capture of the note is final; an edit is always sent. No audio
/// is sent: the transcript is what the live stream delivered.
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
    // A Stop that waits for its capture to be delivered goes through
    // `Cereal::live_deliver` instead, which sends it once the server has it.
    let delivering = note.pending_stop.is_some() || !note.live_pending.is_empty();
    if returning && note.unsent_capture && captures_final && !delivering {
        Some(Action::Recording("stop"))
    } else if note.unsent_edits || note.enhanced_dirty {
        Some(Action::Save)
    } else {
        None
    }
}

/// Put a note in Trash as "Deleted on the server": its own server answered
/// 404 for it. Its text, favourite and capture files stay. A note
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
                    client.request::<Value>("GET", "/api/v1/models/chat", None).map(|response| chat_models_from(&response)).unwrap_or_default()
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
            // A note in the Trash opens the Trash, not itself.
            self.open_linked_note(id, cx);
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
                            this.open_linked_note(id, cx);
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
            note.add_transcript_gaps(remote.transcript_gaps.clone().unwrap_or_default());
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
        // Captures that a quit, a crash or a lost session left part-way to
        // the server, and Stops that were waiting for them: streamed again
        // from where the server is, and the Stop sent after.
        let owed: Vec<u64> = self
            .workspace
            .notes_of(account.slot)
            .filter(|note| recording != Some(note.id))
            .filter(|note| note.pending_stop.is_some() || !note.live_pending.is_empty())
            .filter(|note| !(note.trashed && note.server_missing_since.is_some()))
            .map(|note| note.id)
            .collect();
        for id in owed {
            self.live_deliver(id, None, cx);
        }
    }

    /// Send something about a note to the account it belongs to, through
    /// that account's serial worker. Nothing falls back to another account.
    pub fn server_action(&mut self, id: u64, action: Action, cx: &mut Context<Self>) {
        self.server_action_sent(id, action, cx);
    }

    /// [`Cereal::server_action`], saying whether the action went to the
    /// account's worker.
    pub fn server_action_sent(&mut self, id: u64, action: Action, cx: &mut Context<Self>) -> bool {
        if !enabled() {
            return false;
        }
        let Some(account) = crate::accounts::of_note(id) else {
            return false;
        };
        if !account.shown() {
            // Its session has ended (or was never verified in this launch):
            // nothing is sent. What is unsent stays marked on the note and
            // is queued again after the sign-in.
            return false;
        }
        let account_epoch = account.epoch();
        let account_id = account.id.clone();
        let Some(note) = self.workspace.note(id).cloned() else {
            return false;
        };
        let generating = matches!(&action, Action::Generate);
        let chat = matches!(&action, Action::Chat { .. });
        // An action that ends with the server told the recording stopped.
        let stops = matches!(&action, Action::Recording("stop") | Action::Generate);
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
        // A generation says how it is coming along: its notes are shown as
        // they are written (`generation_draft`).
        let (progress_sender, progress) = if let Some(epoch) = epoch {
            self.generation_draft.begin(id, epoch);
            let (sender, receiver) = mpsc::channel();
            (Some(sender), Some(receiver))
        } else {
            (None, None)
        };
        let progress_title = submitted_title.clone();
        let receiver = self
            .bridges
            .entry(account_id.clone())
            .or_insert_with(|| Bridge::new(account_id.clone()))
            .send(note, action, progress_sender);
        self.server_pending += 1;
        // Saves, Start and Stop go by without a word, and a generation is
        // on the page itself (the chip, then the notes as they are written).
        self.server_status = if chat {
            Some("Thinking…".into())
        } else {
            None
        };
        cx.notify();
        cx.spawn(async move |this, cx| {
            // What the generation has said since the last look, onto the page.
            let heard = |this: &WeakEntity<Self>, cx: &mut AsyncApp| {
                let (Some(progress), Some(epoch)) = (progress.as_ref(), epoch) else {
                    return;
                };
                let events: Vec<GenerateProgress> = progress.try_iter().collect();
                let _ = this.update(cx, |this, cx| {
                    for event in events {
                        this.generation_progress(id, epoch, &progress_title, event, cx);
                    }
                    this.generation_draft_step(epoch, cx);
                });
            };
            let result = loop {
                match receiver.try_recv() {
                    Ok(result) => break result,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        break Err("Server worker stopped. Local audio is preserved.".into());
                    }
                    Err(mpsc::TryRecvError::Empty) => {
                        // A generation is followed frame by frame; anything
                        // else is looked at ten times a second, as before.
                        let wait = if progress.is_some() { 16 } else { 100 };
                        cx.background_executor()
                            .timer(Duration::from_millis(wait))
                            .await;
                        heard(&this, cx);
                    }
                }
            };
            heard(&this, cx);
            let _ = this.update(cx, |this, cx| {
                this.server_pending = this.server_pending.saturating_sub(1);
                if stops {
                    // Answered either way: a Stop still pending is sent
                    // again when the account is next ready.
                    this.live.stopping.remove(&id);
                }
                if crate::accounts::epoch_of(&account_id) != Some(account_epoch) {
                    // The account's session was replaced or ended, or it
                    // was signed out of. The note keeps its unsent marks
                    // and is queued again when the account is Ready.
                    if this.server_pending == 0 {
                        this.server_status = None;
                    }
                    if generating && epoch == Some(this.generation.epoch) {
                        this.generation.cancel();
                        this.generation_draft.clear();
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
                            if stops && !recording_now {
                                note.unsent_capture = false;
                                // Nothing was owed when this Stop went out
                                // (and nothing has been recorded since).
                                if note.live_pending.is_empty() {
                                    note.pending_stop = None;
                                }
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
                            // What the server lists as captured but never
                            // transcribed, with what this Mac's stream was
                            // told of.
                            note.add_transcript_gaps(reply.note.transcript_gaps.clone().unwrap_or_default());
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
                            if reply.untranscribed {
                                // Nothing to write notes from: say so, and
                                // leave the note as it is.
                                this.generation.cancel();
                                this.generation_draft.clear();
                            } else {
                                this.generation.writing(epoch);
                                this.generation.phase = crate::recording::GenerationPhase::Ready;
                                // What is on the page stays; the rest of the
                                // saved notes is written out after it.
                                if this.generation_draft.of(epoch) {
                                    let saved = this
                                        .workspace
                                        .note(id)
                                        .map(|note| note.enhanced.clone())
                                        .unwrap_or_default();
                                    this.generation_draft.finish(&saved);
                                }
                                this.notify_notes(id, None);
                            }
                        }
                        // What another action still on its way says stays.
                        if generating && reply.untranscribed {
                            this.server_status = Some(NO_TRANSCRIPT.into());
                        } else if this.server_pending == 0 || generating || chat {
                            this.server_status = None;
                        }
                        this.persist(cx);
                        if this.current_note == Some(id) {
                            this.refresh_meeting_context(id, cx);
                        }

                    }
                    Err(error) => {
                        if generating && epoch == Some(this.generation.epoch) {
                            this.generation.cancel();
                            this.generation_draft.clear();
                            this.notify_notes(id, Some(&error));
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
            // The end of the notes is still being written out.
            if let Some(epoch) = epoch {
                loop {
                    cx.background_executor()
                        .timer(Duration::from_millis(16))
                        .await;
                    match this.update(cx, |this, cx| this.generation_draft_step(epoch, cx)) {
                        Ok(true) => {}
                        _ => break,
                    }
                }
            }
        })
        .detach();
        true
    }
    pub fn submit_server_chat(&mut self, question: String, cx: &mut Context<Self>) {
        self.start_streaming_chat(question, cx);
    }
}

/// What a note says when Stop (or Generate) found no transcript to write
/// notes from: the live stream delivered nothing for this recording.
pub const NO_TRANSCRIPT: &str = "No transcript was captured for this recording";

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

#[cfg(test)]
mod tests {
    use super::Client;

    #[test]
    fn chat_models_skip_what_this_build_cannot_read() {
        use super::{chat_model_key, chat_models_from};
        use serde_json::json;
        let models = chat_models_from(&json!({"models": [
            {"provider": "openai_compatible", "model": "llama3.2:3b"},
            {"provider": "openrouter", "model": "openai/gpt-6-sol"},
            {"provider": "some_future_provider", "model": "x"},
        ]}));
        assert_eq!(models.iter().map(chat_model_key).collect::<Vec<_>>(), vec!["openai_compatible:llama3.2:3b", "openrouter:openai/gpt-6-sol"]);
        assert!(chat_models_from(&json!({"unexpected": true})).is_empty());
    }

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
        // The model's reasoning, piece by piece, whitespace kept.
        assert_eq!(
            decode_chat_frame(r#"{"type":"reasoning","text":"\n\n**Searching** café"}"#),
            Some(ChatStreamEvent::Reasoning("\n\n**Searching** café".into()))
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
            stream_chat_with_client(Ok(client), None, "Question".into(), vec![], cancel.clone(), None, None);
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

    fn evidence_note(address: std::net::SocketAddr) -> crate::model::Note {
        crate::model::Note {
            id: 9,
            server_id: Some("419d65d1-7a2c-44ca-84cc-ad489522d561".into()),
            server_url: Some(format!("http://{address}")),
            transcript_revision: 3,
            ..Default::default()
        }
    }

    /// One request off `listener`: its request line and body.
    fn read_request(listener: &TcpListener) -> (std::net::TcpStream, String, serde_json::Value) {
        let (mut stream, _) = listener.accept().unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut head = Vec::new();
        let mut byte = [0; 1];
        while !head.ends_with(b"\r\n\r\n") {
            stream.read_exact(&mut byte).unwrap();
            head.push(byte[0]);
        }
        let head = String::from_utf8(head).unwrap();
        let length: usize = head
            .lines()
            .find_map(|line| line.to_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse().unwrap()))
            .unwrap_or(0);
        let mut body = vec![0; length];
        stream.read_exact(&mut body).unwrap();
        let line = head.lines().next().unwrap().to_owned();
        (stream, line, serde_json::from_slice(&body).unwrap())
    }

    #[test]
    fn streamed_evidence_arrives_word_by_word_then_done_with_its_quotations() {
        use super::{EvidenceStreamEvent as Event, stream_evidence_with_client};
        use std::io::Write;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let segment = "00000000-0000-4000-8000-000000000001";
        let server = std::thread::spawn(move || {
            let (mut socket, line, body) = read_request(&listener);
            assert!(line.starts_with("POST /api/v1/notes/419d65d1-7a2c-44ca-84cc-ad489522d561/evidence/stream "));
            assert_eq!(body, serde_json::json!({"paragraph":"Ship Monday","expectedTranscriptRevision":3}));
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n: keepalive\n\n").unwrap();
            for text in ["Zoë ", "moved it 🦊"] {
                let frame = format!("data: {}\n\n", serde_json::json!({"type":"delta","text":text}));
                for bytes in frame.as_bytes().chunks(3) {
                    socket.write_all(bytes).unwrap();
                }
                std::thread::sleep(Duration::from_millis(40));
            }
            let done = serde_json::json!({"type":"done","text":"","evidence":{"summary":"Zoë moved it 🦊","quotes":[{"segmentId":segment,"text":"We moved it.","speakerLabel":"Speaker 4","startMs":47000}],"transcriptRevision":3}});
            socket.write_all(format!("data: {done}\n\n").as_bytes()).unwrap();
        });
        let client = Client::for_test(format!("http://{address}"), "fictional-evidence-stream-owner", None);
        let receiver = stream_evidence_with_client(Ok(client), evidence_note(address), "Ship Monday".into(), Arc::new(AtomicBool::new(false)));
        let next = || receiver.recv_timeout(Duration::from_secs(3)).unwrap();
        assert_eq!(next(), Event::Delta("Zoë ".into()));
        assert_eq!(next(), Event::Delta("moved it 🦊".into()));
        let Event::Done(evidence) = next() else { panic!("expected done") };
        assert_eq!(evidence.summary, "Zoë moved it 🦊");
        assert_eq!(evidence.quotes[0].segment_id, segment);
        assert_eq!(evidence.transcript_revision, 3);
        server.join().unwrap();
    }

    #[test]
    fn closing_streamed_evidence_closes_the_connection() {
        use super::{EvidenceStreamEvent as Event, stream_evidence_with_client};
        use std::io::Write;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut socket, _, _) = read_request(&listener);
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\ndata: {\"type\":\"delta\",\"text\":\"first\"}\n\n").unwrap();
            let mut byte = [0; 1];
            let closed = socket.read(&mut byte);
            assert!(matches!(closed, Ok(0)) || closed.is_err());
        });
        let client = Client::for_test(format!("http://{address}"), "fictional-evidence-cancel-owner", None);
        let cancel = Arc::new(AtomicBool::new(false));
        let receiver = stream_evidence_with_client(Ok(client), evidence_note(address), "Ship Monday".into(), cancel.clone());
        assert_eq!(receiver.recv_timeout(Duration::from_secs(3)).unwrap(), Event::Delta("first".into()));
        cancel.store(true, Ordering::Release);
        assert_eq!(receiver.recv_timeout(Duration::from_secs(3)), Err(mpsc::RecvTimeoutError::Disconnected));
        server.join().unwrap();
    }

    #[test]
    fn a_server_without_the_stream_is_asked_the_buffered_way() {
        missing_stream_endpoint("404 Not Found");
        missing_stream_endpoint("405 Method Not Allowed");
    }

    fn missing_stream_endpoint(status: &'static str) {
        use super::{EvidenceStreamEvent as Event, stream_evidence_with_client};
        use std::io::Write;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut socket, line, _) = read_request(&listener);
            assert!(line.contains("/evidence/stream "));
            socket.write_all(format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}").as_bytes()).unwrap();
            drop(socket);
            let (mut socket, line, body) = read_request(&listener);
            assert!(line.starts_with("POST /api/v1/notes/419d65d1-7a2c-44ca-84cc-ad489522d561/evidence "));
            assert_eq!(body["expectedTranscriptRevision"], 3);
            let answer = serde_json::json!({"summary":"Buffered","quotes":[],"transcriptRevision":3}).to_string();
            socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}", answer.len()).as_bytes()).unwrap();
        });
        let client = Client::for_test(format!("http://{address}"), "fictional-evidence-fallback-owner", None);
        let receiver = stream_evidence_with_client(Ok(client), evidence_note(address), "Ship Monday".into(), Arc::new(AtomicBool::new(false)));
        let next = || receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(next(), Event::Delta("Buffered".into()));
        assert!(matches!(next(), Event::Done(evidence) if evidence.summary == "Buffered"));
        server.join().unwrap();
    }

    /// A stream that ends before its first event with `status` (or, with
    /// `None`, with the connection dropped): reported as an error, and the
    /// server is not asked again.
    fn silent_stream_is_not_asked_again(status: Option<&'static str>) {
        use super::{EvidenceStreamEvent as Event, stream_evidence_with_client};
        use std::io::Write;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut socket, line, _) = read_request(&listener);
            assert!(line.contains("/evidence/stream "));
            if let Some(status) = status {
                socket.write_all(format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}").as_bytes()).unwrap();
            }
            drop(socket);
            // No second request follows.
            listener.set_nonblocking(true).unwrap();
            let until = std::time::Instant::now() + Duration::from_millis(1500);
            while std::time::Instant::now() < until {
                assert!(listener.accept().is_err(), "the buffered route was asked after {status:?}");
                std::thread::sleep(Duration::from_millis(20));
            }
        });
        let client = Client::for_test(format!("http://{address}"), "fictional-evidence-silent-owner", None);
        let receiver = stream_evidence_with_client(Ok(client), evidence_note(address), "Ship Monday".into(), Arc::new(AtomicBool::new(false)));
        assert!(matches!(receiver.recv_timeout(Duration::from_secs(5)).unwrap(), Event::Error(_)));
        server.join().unwrap();
        assert!(receiver.recv_timeout(Duration::from_millis(200)).is_err());
    }

    #[test]
    fn only_a_missing_endpoint_is_asked_the_buffered_way() {
        silent_stream_is_not_asked_again(Some("400 Bad Request"));
        silent_stream_is_not_asked_again(Some("401 Unauthorized"));
        silent_stream_is_not_asked_again(Some("503 Service Unavailable"));
        // The model may have started: a lost connection is never re-asked.
        silent_stream_is_not_asked_again(None);
        assert_eq!(super::http_status("curl: (22) The requested URL returned error: 404\n404"), Some(404));
        assert_eq!(super::http_status("000"), None);
        assert_eq!(super::http_status(""), None);
    }

    #[test]
    fn evidence_frames_are_checked_before_they_reach_the_card() {
        use super::{EvidenceStreamEvent as Event, decode_evidence_frame};
        assert_eq!(decode_evidence_frame(r#"{"type":"delta","text":"a"}"#), Some(Event::Delta("a".into())));
        assert_eq!(decode_evidence_frame(r#"{"type":"error","text":"No"}"#), Some(Event::Error("No".into())));
        // A done without its result, or with a quotation that is not a segment id, is refused.
        assert_eq!(decode_evidence_frame(r#"{"type":"done","text":""}"#), None);
        assert_eq!(decode_evidence_frame(r#"{"type":"done","text":"","evidence":{"summary":"s","quotes":[{"segmentId":"nope","text":"t","speakerLabel":null,"startMs":0}],"transcriptRevision":1}}"#), None);
        assert_eq!(decode_evidence_frame(r#"{"type":"surprise","text":""}"#), None);
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

    /// The live stream decides where a capture sits on the note's timeline
    /// while it is still recording: after the note's transcript so far and
    /// after the earlier captures here. Decided once, and written down, so
    /// a later connection of the same stream places it at the same spot.
    #[test]
    fn a_live_capture_takes_its_timeline_position_once() {
        use crate::{
            audio::{CaptureManifest, ChannelStats, NativeStats},
            model::Note,
        };
        use std::{
            fs,
            io::{Read, Write},
            net::TcpListener,
        };
        let root = std::env::temp_dir().join(format!("cereal-live-base-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let id = "419d65d1-7a2c-44ca-84cc-ad489522d561";
        let note_json = |end_ms: i64| {
            serde_json::json!({
                "id": id, "title": "", "body": "", "enhanced": "", "date": "2026-11-16", "duration": "",
                "createdAt": "2026-11-16T10:00:00Z", "updatedAt": "2026-11-16T10:00:00Z",
                "recordingState": "recording", "generationState": "idle", "transcriptRevision": 1,
                "generatedRevision": null,
                "transcriptSegments": [{"id": "419d65d1-7a2c-44ca-84cc-ad489522d570", "channel": "mic",
                    "speakerId": "mic:n:1", "speakerLabel": "Me", "startMs": 0, "endMs": end_ms,
                    "text": "Earlier words.", "isFinal": true}]
            })
            .to_string()
        };
        // The note is read twice. The first time its transcript ends at 4 s
        // (before the earlier capture here ends). The second time (a later
        // connection, after another device added to it) much later.
        let bodies = [note_json(4_000), note_json(900_000)];
        let server = std::thread::spawn(move || {
            for body in bodies {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = Vec::new();
                let mut byte = [0; 1];
                while !request.ends_with(b"\r\n\r\n") {
                    stream.read_exact(&mut byte).unwrap();
                    request.push(byte[0]);
                }
                let request = String::from_utf8(request).unwrap();
                // The note itself: never its audio parts.
                assert!(request.starts_with(&format!("GET /api/v1/notes/{id} ")), "{request}");
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
        // An earlier capture of 12.5 s, then the one being recorded now.
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
        assert_eq!(client.live_capture_base(&note, id, "capture-live"), Ok(16_500));
        // Where each capture was written down: the earlier one after the
        // transcript, this one after the earlier one.
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
        assert_eq!((saved("capture-earlier"), saved("capture-live")), (4_000, 16_500));
        // Decided once: a later look at a note that now holds more does not
        // move a capture that is already streaming.
        assert_eq!(client.live_capture_base(&note, id, "capture-live"), Ok(16_500));
        assert_eq!((saved("capture-earlier"), saved("capture-live")), (4_000, 16_500));
        // A capture an earlier version placed keeps its position.
        let older = Note { audio_captures: vec![capture("capture-older", false, 0)], ..note.clone() };
        let key = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, b"capture-older");
        super::save_mapping(
            &client.placement_root(&older, id).unwrap().join(format!("capture-{key}.json")),
            &3_000u64,
        )
        .unwrap();
        assert_eq!(client.capture_base_after(&older, id, "capture-older", 99_000), Ok(3_000));
        // A capture not of this note has no position.
        assert!(client.capture_base_after(&note, id, "capture-elsewhere", 0).is_err());
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
            r#"{{"id":"{}","title":"Note {n}","date":"2026-11-12","duration":"","createdAt":"2026-11-12T10:00:00.000Z","updatedAt":"{updated}","recordingState":"stopped","generationState":"idle","transcriptRevision":{transcript},"generatedRevision":null,"hasBody":true,"hasSummary":false,"hasTranscript":false,"speakerCount":0,"excerpt":"","meeting":null,"attendees":{{"people":[],"others":0,"total":0}},"organizationId":null}}"#,
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
            "attendees": {"people": [], "others": 0, "total": 0}, "organizationId": null
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
            favorite: true,
            ..Default::default()
        };
        assert!(mark_missing(&mut note, "2026-11-16T10:00:00Z"));
        assert!(note.trashed);
        assert_eq!(note.server_missing_since.as_deref(), Some("2026-11-16T10:00:00Z"));
        assert_eq!((note.body.as_str(), note.favorite), ("Its text", true));
        assert!(mark_listed(&mut note));
        assert!(!note.trashed && note.server_missing_since.is_none());
        let mut unsent = Note { unsent_edits: true, ..Default::default() };
        assert!(!mark_missing(&mut unsent, "now"));
        assert!(!unsent.trashed);
        let mut capturing = Note { unsent_capture: true, ..Default::default() };
        assert!(!mark_missing(&mut capturing, "now"));
    }

    const NOT_FOUND: &str = r#"{"error":"not_found","message":"Note not found"}"#;

    /// A trashed note of this server, as the last refresh left it.
    fn trashed(id: u64, server_id: &str, url: &str, title: &str) -> Note {
        Note {
            id,
            title: title.into(),
            trashed: true,
            server_id: Some(server_id.into()),
            server_url: Some(url.into()),
            server_sync_stamp: Some(sync_stamp("2026-11-16T10:00:00Z", 0, None)),
            ..Default::default()
        }
    }

    #[test]
    fn t1_delete_forever_deletes_on_the_server_and_forgets_the_association() {
        let server = Fake::start(move |request| match (request.method.as_str(), request.path.as_str()) {
            ("GET", path) if path == format!("/api/v1/notes/{A}") => json(200, note_json(A, "Kept as read")),
            ("DELETE", path) if path.starts_with(&format!("/api/v1/notes/{A}")) => json(200, r#"{"deleted":true}"#),
            (_, path) if path.starts_with(&format!("/api/v1/notes/{B}")) => json(404, NOT_FOUND),
            _ => json(500, r#"{"error":"internal","message":"Server is down"}"#),
        });
        let root = scratch("t1");
        let mut client = Client::for_test(&server.url, TOKEN, Some(&root));
        let on_server = handle::handle(1, 5).unwrap();
        let note = trashed(on_server, A, &server.url, "Kept as read");
        client.ids.insert(on_server, A.into());
        let sidecar = association_file(&root, &server.url, on_server);
        fs::create_dir_all(sidecar.parent().unwrap().join("uploads").join(A)).unwrap();
        fs::write(&sidecar, A).unwrap();
        client.delete(&note, false, true).unwrap();
        assert!(!sidecar.exists() && !client.ids.contains_key(&on_server));
        assert!(!sidecar.parent().unwrap().join("uploads").join(A).exists());
        // Known only by its sidecar, and already gone: the server says so in
        // its own words, and it has conditional deletes.
        let by_sidecar = handle::handle(1, 6).unwrap();
        let sidecar = association_file(&root, &server.url, by_sidecar);
        fs::write(&sidecar, B).unwrap();
        client.delete(&Note { id: by_sidecar, trashed: true, ..Default::default() }, false, true).unwrap();
        assert!(!sidecar.exists());
        // Never sent: nothing is asked of the server.
        client.delete(&Note { id: handle::handle(1, 7).unwrap(), trashed: true, ..Default::default() }, false, true).unwrap();
        assert_eq!(
            server.paths(),
            [
                format!("GET /api/v1/notes/{A}"),
                // The stamp of what was read: updatedAt|transcriptRevision|(no generated notes).
                format!("DELETE /api/v1/notes/{A}?ifStamp=2026-11-16T10%3A00%3A00Z%7C0%7C"),
                format!("GET /api/v1/notes/{B}"),
            ],
            "nothing is created or saved first"
        );
        // A failure keeps everything, so the note stays in Trash.
        let failing = handle::handle(1, 8).unwrap();
        let sidecar = association_file(&root, &server.url, failing);
        let other = "3a49a4af-c660-42e9-a36a-a806fbb41e03";
        fs::write(&sidecar, other).unwrap();
        let error = client.delete(&Note { id: failing, trashed: true, ..Default::default() }, false, true).unwrap_err();
        assert_eq!(error, DeleteFailure::Failed("Server is down".into()));
        assert!(sidecar.exists());
        // Another server's note is never sent here.
        let elsewhere = trashed(handle::handle(1, 9).unwrap(), A, "https://elsewhere.example.com", "x");
        assert!(client.delete(&elsewhere, false, true).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn t2_a_note_edited_elsewhere_since_it_was_trashed_is_not_deleted_unless_asked_again() {
        let edited = "3a49a4af-c660-42e9-a36a-a806fbb41e04";
        let raced = "3a49a4af-c660-42e9-a36a-a806fbb41e05";
        let server = Fake::start(move |request| match (request.method.as_str(), request.path.as_str()) {
            // Edited on another device: a new title and a later change.
            ("GET", path) if path == format!("/api/v1/notes/{edited}") => json(
                200,
                note_json(edited, "Edited on the iPhone")
                    .replace(r#""updatedAt":"2026-11-16T10:00:00Z""#, r#""updatedAt":"2026-11-16T11:00:00Z""#),
            ),
            ("DELETE", path) if path == format!("/api/v1/notes/{edited}") => json(200, r#"{"deleted":true}"#),
            // Unchanged when read, edited before the delete arrived.
            ("GET", path) if path == format!("/api/v1/notes/{raced}") => json(200, note_json(raced, "As read")),
            ("DELETE", path) if path.starts_with(&format!("/api/v1/notes/{raced}?ifStamp=")) => json(
                409,
                r#"{"error":"conflict","message":"The note changed since it was read, so it was not deleted"}"#,
            ),
            _ => json(500, r#"{"error":"internal","message":"unexpected"}"#),
        });
        let root = scratch("t2");
        let mut client = Client::for_test(&server.url, TOKEN, Some(&root));
        let mut note = trashed(handle::handle(1, 5).unwrap(), edited, &server.url, "As this Mac had it");
        note.server_sync_stamp = Some(sync_stamp("2026-11-16T09:00:00Z", 0, None));
        assert_eq!(client.delete(&note, false, true), Err(DeleteFailure::Changed));
        assert!(
            !server.paths().iter().any(|path| path.starts_with("DELETE")),
            "nothing is deleted while the server's copy is newer"
        );
        // "Delete anyway": no read, no condition.
        client.delete(&note, true, true).unwrap();
        assert_eq!(server.paths().last().unwrap(), &format!("DELETE /api/v1/notes/{edited}"));
        // The same text as here, though the stamp is old (this Mac saved it
        // since the last refresh): deleted, on condition.
        let mut same = trashed(handle::handle(1, 6).unwrap(), raced, &server.url, "As read");
        same.server_sync_stamp = None;
        assert_eq!(client.delete(&same, false, true), Err(DeleteFailure::Changed), "409: it changed in between");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn t3_a_404_deletes_nothing_here_unless_the_server_says_it_has_no_such_note() {
        let server = Fake::start(move |_| json(404, "<html>Not here</html>"));
        let root = scratch("t3");
        let mut client = Client::for_test(&server.url, TOKEN, Some(&root));
        let id = handle::handle(1, 5).unwrap();
        let note = trashed(id, A, &server.url, "Kept");
        let sidecar = association_file(&root, &server.url, id);
        fs::create_dir_all(sidecar.parent().unwrap()).unwrap();
        fs::write(&sidecar, A).unwrap();
        // A proxy's page, and the API's own words from a server without
        // conditional deletes: neither is taken as "already deleted".
        assert!(matches!(client.delete(&note, false, true), Err(DeleteFailure::Failed(_))));
        let api = Fake::start(move |_| json(404, NOT_FOUND));
        let mut older = Client::for_test(&api.url, TOKEN, Some(&root));
        let note = trashed(id, A, &api.url, "Kept");
        let sidecar_two = association_file(&root, &api.url, id);
        fs::create_dir_all(sidecar_two.parent().unwrap()).unwrap();
        fs::write(&sidecar_two, A).unwrap();
        assert!(matches!(older.delete(&note, false, false), Err(DeleteFailure::Failed(_))));
        assert!(sidecar.exists() && sidecar_two.exists());
        let _ = fs::remove_dir_all(root);
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
    fn s3_the_stop_of_a_recording_made_while_the_session_had_ended_is_sent_after_sign_in() {
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
        // Back after the sign-in, the server is told it stopped. No audio
        // goes with it: the transcript is what the live stream delivered.
        assert!(matches!(requeue_action(&note, true), Some(Action::Recording("stop"))));
        // At an ordinary launch a Stop is not sent by itself, as before.
        assert!(requeue_action(&note, false).is_none());
        // An unsent edit is sent either way.
        let edited = Note { unsent_edits: true, ..note.clone() };
        assert!(matches!(requeue_action(&edited, false), Some(Action::Save)));
        // A note its server deleted is not sent at all.
        let gone = Note { trashed: true, server_missing_since: Some("now".into()), ..note };
        assert!(requeue_action(&gone, true).is_none());
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn a_streamed_generation_is_heard_in_order_and_ends_with_the_saved_note() {
        let saved = serde_json::json!({
            "id": A, "title": "Launch date", "body": "", "enhanced": "# Launch\n- Monday", "date": "2026-11-16",
            "duration": "", "createdAt": "2026-11-16T10:00:00Z", "updatedAt": "2026-11-16T10:00:00Z",
            "recordingState": "stopped", "generationState": "ready", "transcriptRevision": 3,
            "generatedRevision": 3, "transcriptSegments": []
        });
        let frames = [
            serde_json::json!({"type": "title", "text": "Launch date"}),
            serde_json::json!({"type": "delta", "text": "# Launch\n"}),
            serde_json::json!({"type": "delta", "text": "- Monday"}),
            serde_json::json!({"type": "done", "text": "", "note": saved}),
        ];
        let stream: String = frames.iter().map(|frame| format!("data: {frame}\n\n")).collect();
        let server = Fake::start(move |request| match request.path.as_str() {
            path if path.ends_with("/generate/stream") => (200, "text/event-stream", stream.clone(), Vec::new()),
            _ => json(500, "{}"),
        });
        let root = scratch("generate-stream");
        let client = Client::for_test(&server.url, TOKEN, Some(&root));
        let (sender, heard) = mpsc::channel();
        let note = client
            .generate(&format!("/api/v1/notes/{A}"), 3, Some(&sender))
            .unwrap();
        assert_eq!(note.enhanced, "# Launch\n- Monday");
        assert_eq!(
            heard.try_iter().collect::<Vec<_>>(),
            [
                GenerateProgress::Began,
                GenerateProgress::Title("Launch date".into()),
                GenerateProgress::Delta("# Launch\n".into()),
                GenerateProgress::Delta("- Monday".into()),
            ]
        );
        // An error frame ends it with the server's words.
        let failing = Fake::start(|_| {
            (200, "text/event-stream", "data: {\"type\":\"delta\",\"text\":\"# A\"}\n\ndata: {\"type\":\"error\",\"text\":\"Notes could not be generated. Try again.\"}\n\n".into(), Vec::new())
        });
        let client = Client::for_test(&failing.url, TOKEN, Some(&root));
        assert_eq!(
            client.generate(&format!("/api/v1/notes/{A}"), 3, None).unwrap_err(),
            "Notes could not be generated. Try again."
        );
        let _ = fs::remove_dir_all(root);
    }

    /// Stop goes straight on to the notes: the server is told the recording
    /// stopped (once the stream has handed over), and the notes are written
    /// from the transcript it then has. No audio is uploaded, and nothing
    /// waits for parts. A recording with no transcript is not sent to be
    /// written up at all.
    #[test]
    fn stop_writes_the_notes_from_the_live_transcript_and_uploads_no_audio() {
        let with_text = |id: &str, segments: serde_json::Value| {
            let mut note: serde_json::Value = serde_json::from_str(&note_json(id, "Standup")).unwrap();
            note["transcriptRevision"] = 3.into();
            note["transcriptSegments"] = segments;
            note.to_string()
        };
        let spoken = serde_json::json!([{"id": "3a49a4af-c660-42e9-a36a-a806fbb41e70", "channel": "mic",
            "speakerId": "mic:n:1", "speakerLabel": "Me", "startMs": 0, "endMs": 1_500,
            "text": "Ship it on Friday.", "isFinal": true}]);
        let silent = serde_json::json!([{"id": "3a49a4af-c660-42e9-a36a-a806fbb41e71", "channel": "system",
            "speakerId": "system:n:1", "speakerLabel": "Speaker 1", "startMs": 0, "endMs": 1_500,
            "text": "  ", "isFinal": true}]);
        let (a, b) = (with_text(A, spoken), with_text(B, silent));
        let server = Fake::start(move |request| match request.path.as_str() {
            path if path.starts_with(&format!("/api/v1/notes/{A}")) => json(200, a.clone()),
            path if path.starts_with(&format!("/api/v1/notes/{B}")) => json(200, b.clone()),
            _ => json(500, "{}"),
        });
        let root = scratch("stop");
        let mut client = Client::for_test(&server.url, TOKEN, Some(&root));
        let note = |local: u64, id: &str| Note {
            id: handle::handle(1, local).unwrap(),
            title: "Standup".into(),
            server_id: Some(id.into()),
            server_url: Some(server.url.clone()),
            ..Default::default()
        };
        let reply = client.run(&note(1, A), Action::Generate).unwrap();
        assert!(!reply.untranscribed);
        assert_eq!(
            server.paths(),
            [
                format!("PATCH /api/v1/notes/{A}"),
                format!("POST /api/v1/notes/{A}/recording"),
                format!("GET /api/v1/notes/{A}"),
                // The streamed endpoint first; this fake answers it without
                // a stream, so the notes are asked for the buffered way.
                format!("POST /api/v1/notes/{A}/generate/stream"),
                format!("POST /api/v1/notes/{A}/generate"),
            ]
        );
        let bodies: Vec<String> = server.seen.lock().unwrap().iter().map(|seen| seen.body.clone()).collect();
        assert_eq!(serde_json::from_str::<serde_json::Value>(&bodies[1]).unwrap(), serde_json::json!({"command": "stop"}));
        for body in &bodies[3..5] {
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(body).unwrap(),
                serde_json::json!({"expectedTranscriptRevision": 3})
            );
        }
        // Nothing was said: the note is left as it is, and says so.
        server.seen.lock().unwrap().clear();
        let reply = client.run(&note(2, B), Action::Generate).unwrap();
        assert!(reply.untranscribed);
        assert_eq!(
            server.paths(),
            [
                format!("PATCH /api/v1/notes/{B}"),
                format!("POST /api/v1/notes/{B}/recording"),
                format!("GET /api/v1/notes/{B}"),
                format!("GET /api/v1/notes/{B}"),
            ]
        );
        // A Stop on its own (another recording takes over) only stops.
        server.seen.lock().unwrap().clear();
        let reply = client.run(&note(1, A), Action::Recording("stop")).unwrap();
        assert!(!reply.untranscribed);
        assert_eq!(
            server.paths(),
            [
                format!("PATCH /api/v1/notes/{A}"),
                format!("GET /api/v1/notes/{A}"),
                format!("POST /api/v1/notes/{A}/recording"),
                format!("GET /api/v1/notes/{A}"),
            ]
        );
        assert!(!server.paths().iter().any(|path| path.contains("/audio")));
        let _ = fs::remove_dir_all(root);
    }
}
