//! Chat presentation shared by the compact note assistant and full chat surface.
//! Markdown remains data: raw HTML and unknown links are never executed.
use crate::ui::{Reach, Theme, icon, pill, round_tool, small, tool_tip};
use gpui::{prelude::*, *};
use meeting_notes_api_types::{
    ChatCitationDTO, ChatReasoningDTO, ChatToolProgressDTO, ChatToolProgressDtoStatus,
};
use std::ops::Range;
use std::sync::{Arc, atomic::AtomicBool};

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ChatTurnAnnotations {
    #[serde(default)]
    pub tools: Vec<ChatToolProgressDTO>,
    /// The model's own reasoning, in order; each stretch says how many of
    /// `tools` had started before it. Empty when the provider gave none.
    #[serde(default)]
    pub reasoning: Vec<ChatReasoningDTO>,
    #[serde(default)]
    pub citations: Vec<ChatCitationDTO>,
    #[serde(default)]
    pub elapsed_ms: Option<u64>,
    #[serde(default)]
    pub error: Option<String>,
    /// Whether the person opened or closed the thoughts themselves. Left
    /// alone they are open while the answer is thought about and closed
    /// once it starts.
    #[serde(skip)]
    expanded: Option<bool>,
    #[serde(skip)]
    expanded_tools: std::collections::HashSet<String>,
}

/// One thing in an answer's thoughts, in the order it happened.
#[derive(Debug, PartialEq)]
pub(crate) enum Thought<'a> {
    /// A part of the model's reasoning: its heading, when it gave one, and
    /// the text under it.
    Reasoning {
        heading: Option<&'a str>,
        body: &'a str,
    },
    /// A meeting lookup: its place in `ChatTurnAnnotations::tools`.
    Step(usize),
}

/// A stretch of reasoning as its parts. Providers that summarise reasoning
/// open each part with a line that is only `**Heading**`; anything before
/// the first such line, or all of it when there is none, has no heading.
pub(crate) fn reasoning_sections(text: &str) -> Vec<(Option<&str>, &str)> {
    let mut sections = Vec::new();
    let (mut heading, mut start, mut offset) = (None, 0, 0);
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim();
        let title = trimmed
            .strip_prefix("**")
            .and_then(|rest| rest.strip_suffix("**"))
            .map(str::trim)
            .filter(|title| !title.is_empty() && !title.contains("**"));
        if let Some(title) = title {
            let body = text[start..offset].trim();
            if heading.is_some() || !body.is_empty() {
                sections.push((heading, body));
            }
            heading = Some(title);
            start = offset + line.len();
        }
        offset += line.len();
    }
    let body = text[start..].trim();
    if heading.is_some() || !body.is_empty() {
        sections.push((heading, body));
    }
    sections
}

impl ChatTurnAnnotations {
    /// The next piece of reasoning. Pieces join into one stretch until a
    /// lookup that was not there before starts (the server keeps them by the
    /// same rule, so a reopened thread reads as it did live).
    fn add_reasoning(&mut self, text: &str) {
        const STRETCH: usize = 20_000;
        let after = self.tools.len() as i64;
        let stretch = match self.reasoning.last_mut() {
            Some(last) if last.after_tools == after => last,
            _ => {
                let text = text.trim_start();
                if text.is_empty() || self.reasoning.len() >= 40 {
                    return;
                }
                self.reasoning.push(ChatReasoningDTO {
                    after_tools: after,
                    text: String::new(),
                });
                let stretch = self.reasoning.last_mut().expect("just pushed");
                stretch.text.extend(text.chars().take(STRETCH));
                return;
            }
        };
        let room = STRETCH.saturating_sub(stretch.text.chars().count());
        stretch.text.extend(text.chars().take(room));
    }

    /// Reasoning and lookups in the order they happened.
    pub(crate) fn thoughts(&self) -> Vec<Thought<'_>> {
        let mut thoughts = Vec::new();
        let mut reasoning = self.reasoning.iter().peekable();
        for step in 0..=self.tools.len() {
            while let Some(stretch) = reasoning.next_if(|stretch| {
                step == self.tools.len() || stretch.after_tools <= step as i64
            }) {
                thoughts.extend(
                    reasoning_sections(&stretch.text)
                        .into_iter()
                        .map(|(heading, body)| Thought::Reasoning { heading, body }),
                );
            }
            if step < self.tools.len() {
                thoughts.push(Thought::Step(step));
            }
        }
        thoughts
    }

    /// What the answer is doing right now, for the line that says so while
    /// it is thought about: the lookup under way, else the heading the
    /// model's reasoning last gave. `None`: nothing more telling than
    /// "Thinking…".
    pub(crate) fn live_heading(&self) -> Option<String> {
        let trail = |label: &str| format!("{}…", label.trim_end_matches(['.', '…']));
        if let Some(running) = self
            .tools
            .iter()
            .rev()
            .find(|tool| tool.status == ChatToolProgressDtoStatus::Running)
        {
            return Some(trail(&running.label));
        }
        let last = self.reasoning.last()?;
        // A heading is only shown once its line is whole.
        let whole = &last.text[..last.text.rfind('\n').map_or(0, |end| end + 1)];
        reasoning_sections(whole)
            .into_iter()
            .rev()
            .find_map(|(heading, _)| heading)
            .map(trail)
    }

    /// Open or close the thoughts as the person would from their line
    /// (`None`: leave it to the answer, open only while it is thought about).
    #[cfg(debug_assertions)]
    pub(crate) fn set_thoughts_open(&mut self, open: Option<bool>) {
        self.expanded = open;
    }

    /// Open or close one lookup's meetings, as a click on its row would.
    #[cfg(debug_assertions)]
    pub(crate) fn toggle_step(&mut self, call_id: &str) {
        if !self.expanded_tools.remove(call_id) {
            self.expanded_tools.insert(call_id.to_owned());
        }
    }

    /// Whether the turn went looking (searched or read other meetings), as
    /// opposed to reasoning over the meeting it was given: the reference says
    /// "Worked for" then, "Thought for" otherwise. A note's own read is not
    /// work.
    fn worked(&self) -> bool {
        self.tools
            .iter()
            .any(|tool| !matches!(tool.name, meeting_notes_api_types::Name::GetMeetingTranscript))
            || self.tools.len() > 1
    }

    fn has_thoughts(&self) -> bool {
        !self.tools.is_empty() || self.reasoning.iter().any(|item| !item.text.trim().is_empty())
    }

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

/// A disclosure's chevron: pointing along the line while closed, turning
/// down as `pose` opens.
fn disclosure_chevron(size: f32, color: Hsla, pose: crate::motion::Pose) -> Svg {
    let level = if pose.visible { pose.value.clamp(0., 1.) } else { 0. };
    icon("chevron-down", size, color)
        .flex_shrink_0()
        .with_transformation(Transformation::rotate(radians(
            -std::f32::consts::FRAC_PI_2 * (1. - level),
        )))
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum StepState {
    Running,
    Done,
    /// It failed, or was still running when the answer stopped.
    Failed,
}

/// How a meeting lookup is drawn: its glyph, its words and its state.
#[derive(Debug, PartialEq)]
pub(crate) struct StepLook {
    pub glyph: &'static str,
    pub label: String,
    pub state: StepState,
}

/// A glyph for what the lookup did (notes for meetings found or read, bars
/// for transcript, a calendar for a listing by date) and its words in plain
/// language. A failed lookup is quiet: a struck circle and a short sentence,
/// never the server's diagnostic.
pub(crate) fn step_look(tool: &ChatToolProgressDTO, active: bool) -> StepLook {
    use meeting_notes_api_types::Name;
    let state = match tool.status {
        ChatToolProgressDtoStatus::Completed => StepState::Done,
        ChatToolProgressDtoStatus::Running if active => StepState::Running,
        ChatToolProgressDtoStatus::Running | ChatToolProgressDtoStatus::Failed => StepState::Failed,
    };
    let label = tool.label.trim();
    if state == StepState::Failed {
        let lower = label.to_lowercase();
        let label = if tool.status == ChatToolProgressDtoStatus::Running {
            "Stopped before this finished"
        } else if lower.contains("plan") {
            "Couldn't plan the search"
        } else if label.is_empty() || label.chars().count() > 40 || label.contains(';') {
            "Couldn't finish this lookup"
        } else {
            label
        };
        return StepLook {
            glyph: "step-failed",
            label: label.to_owned(),
            state,
        };
    }
    let glyph = match tool.name {
        Name::SearchMeetings | Name::GetMeetings => "step-notes",
        Name::ListMeetings => "calendar",
        Name::SearchTranscripts | Name::GetMeetingTranscript => "footer-transcript",
    };
    let label = if state == StepState::Running {
        format!("{}…", label.trim_end_matches(['.', '…']))
    } else {
        label.to_owned()
    };
    StepLook { glyph, label, state }
}

/// A meeting's day as a list prints it beside the meeting: "Today",
/// "Yesterday", "Nov 13", and the year only when it is not this one
/// ("Dec 30, 2025"). A date that cannot be read is printed as it came.
pub(crate) fn relative_day(date: &str, today: chrono::NaiveDate) -> String {
    use chrono::Datelike;
    let day = date.get(..10).unwrap_or(date);
    let Ok(day) = chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d") else {
        return date.to_owned();
    };
    match today.signed_duration_since(day).num_days() {
        0 => "Today".into(),
        1 => "Yesterday".into(),
        _ if day.year() == today.year() => day.format("%b %-d").to_string(),
        _ => day.format("%b %-d, %Y").to_string(),
    }
}

/// A model's name as people say it, from the id a provider knows it by:
/// "openai/gpt-6-luna" is "GPT-6 Luna", "claude-sonnet-5-5" is "Sonnet 5.5",
/// "gemini-4-carbon" is "Gemini 4 Carbon". The route in front of the name
/// and a trailing release date are left out, version digits are joined with
/// a point, and known initialisms are capitalised.
pub(crate) fn model_display_name(id: &str) -> String {
    let name = id.rsplit('/').next().unwrap_or(id);
    let name = name.strip_prefix("claude-").unwrap_or(name);
    let mut words: Vec<String> = Vec::new();
    let mut version = false;
    for token in name.split(['-', '_', ':', ' ']).filter(|token| !token.is_empty()) {
        let digits = token.chars().all(|c| c.is_ascii_digit());
        // A release date, or the alias for the newest release.
        if (digits && token.len() == 8) || token == "latest" {
            continue;
        }
        let short_number = digits && token.len() <= 2;
        if short_number && version {
            // "5", "5" is version 5.5.
            if let Some(last) = words.last_mut() {
                last.push('.');
                last.push_str(token);
            }
            continue;
        }
        let numeric = token.chars().next().is_some_and(|c| c.is_ascii_digit());
        if numeric && words.last().is_some_and(|last| last == "GPT") {
            // "GPT-6", as its maker writes it.
            if let Some(last) = words.last_mut() {
                last.push('-');
                last.push_str(token);
            }
            version = short_number;
            continue;
        }
        version = short_number;
        let lower = token.to_ascii_lowercase();
        words.push(match lower.as_str() {
            "gpt" | "oss" | "ai" | "xl" | "hd" | "tts" | "it" | "r1" => lower.to_ascii_uppercase(),
            // "o3", "o4": written small.
            _ if lower.len() == 2 && lower.starts_with('o') && lower.ends_with(|c: char| c.is_ascii_digit()) => lower,
            // A size: "70b", "8k".
            _ if numeric && lower.ends_with(['b', 'k', 'm']) && lower[..lower.len() - 1].chars().all(|c| c.is_ascii_digit() || c == '.') => {
                lower.to_ascii_uppercase()
            }
            _ => {
                let mut letters = token.chars();
                letters
                    .next()
                    .map(|first| first.to_uppercase().chain(letters).collect())
                    .unwrap_or_default()
            }
        });
    }
    if words.is_empty() {
        id.to_owned()
    } else {
        words.join(" ")
    }
}

/// The glyph beside a model in the menu, for who makes it: read from the
/// route in its id ("anthropic/…", "google/…") or its name, else from the
/// provider it is reached through. These are plain marks of our own, not
/// the makers' logos.
pub(crate) fn model_glyph(choice: &meeting_notes_api_types::LanguageModelSelection) -> &'static str {
    use meeting_notes_api_types::LanguageModelSelectionProvider as Provider;
    let id = choice.model.to_ascii_lowercase();
    let made_by = |names: &[&str]| names.iter().any(|name| id.contains(name));
    if made_by(&["anthropic/", "claude"]) {
        "provider-anthropic"
    } else if made_by(&["google/", "gemini", "gemma"]) {
        "google"
    } else if made_by(&["x-ai/", "xai/", "grok"]) {
        "provider-xai"
    } else if made_by(&["openai/", "gpt-"]) || id.starts_with("o1") || id.starts_with("o3") || id.starts_with("o4") {
        "provider-openai"
    } else {
        match choice.provider {
            Provider::Anthropic => "provider-anthropic",
            Provider::Openai => "provider-openai",
            Provider::Openrouter | Provider::OpenaiCompatible => "provider-other",
        }
    }
}

#[derive(Debug, PartialEq)]
pub(crate) enum ModelMenuKey {
    Move(usize),
    Choose(usize),
    Close,
}

/// What a key does in the open model menu of `count` rows with `current`
/// highlighted: the arrows move the highlight and wrap, Return chooses it,
/// Escape closes the menu. `None`: the key is not the menu's.
pub(crate) fn model_menu_key(
    key: &str,
    modified: bool,
    current: Option<usize>,
    count: usize,
) -> Option<ModelMenuKey> {
    if modified {
        return None;
    }
    let current = current.filter(|index| *index < count);
    match key {
        "escape" => Some(ModelMenuKey::Close),
        "down" if count > 0 => Some(ModelMenuKey::Move(current.map_or(0, |index| (index + 1) % count))),
        "up" if count > 0 => Some(ModelMenuKey::Move(
            current.map_or(count - 1, |index| (index + count - 1) % count),
        )),
        "enter" | "return" => current.map(ModelMenuKey::Choose),
        _ => None,
    }
}

/// Whether a question asked on `screen` is answered in the panel floating
/// over that page. Only Home: a question asked on a space's page (My notes,
/// Favorites) opens the Chat page, as the reference does, with the back
/// pill returning to the space.
pub(crate) fn floating_workspace_chat(screen: crate::app::Screen) -> bool {
    matches!(screen, crate::app::Screen::Home)
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
        ChatStreamEvent::Reasoning(text) => {
            // Reasoning that trails in after the answer began is not shown.
            if answer.is_empty() {
                annotations.entry(index).or_default().add_reasoning(&text)
            }
        }
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
        Some("Can you provide more details".into())
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

    /// The line above an answer that says how it is coming along, and the
    /// thoughts behind it.
    ///
    /// While nothing of the answer has arrived the line is the mark, turning,
    /// and what is being done with a sheen across it: "Thinking…", then the
    /// lookup under way or the heading the model's reasoning last gave. Once
    /// the answer starts it settles into "Thought for Ns" ("Stopped" if it
    /// was stopped).
    ///
    /// Under it, the thoughts: the model's reasoning and the meeting lookups
    /// in the order they happened, on one rail. They are open while the
    /// answer is thought about, fold away when it starts, and open and close
    /// from the line after that. `None` when there is nothing to say (an old
    /// answer with no timing and no thoughts).
    fn render_chat_status(
        &self,
        index: usize,
        active: bool,
        thinking: bool,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let t = self.theme();
        let annotations = self.chat_annotations.get(&index);
        let has_thoughts = annotations.is_some_and(ChatTurnAnnotations::has_thoughts);
        let open =
            has_thoughts && annotations.and_then(|item| item.expanded).unwrap_or(thinking);
        let elapsed = annotations.and_then(|item| item.elapsed_ms);
        let stopped = !active && self.chat_interrupted.contains(&index);
        // Which line it is: the change from one to the next settles in
        // rather than jumping.
        let (state, heading) = if thinking {
            let live = annotations.and_then(ChatTurnAnnotations::live_heading);
            let state = live.as_ref().map_or(0, |live| crate::lazy::key(live) | 8);
            (state, live.unwrap_or_else(|| "Thinking…".to_owned()))
        } else if stopped {
            (1, "Stopped".to_owned())
        } else if let Some(elapsed) = elapsed {
            let verb = if annotations.is_some_and(ChatTurnAnnotations::worked) {
                "Worked"
            } else {
                "Thought"
            };
            (2, format!("{verb} for {}s", (elapsed / 1000).max(1)))
        } else if has_thoughts {
            (3, "Meeting sources".to_owned())
        } else {
            return None;
        };
        let turn = index as u64;
        let swap = self.stage.swap(("chat-status", turn), state);
        let mark = self
            .stage
            .presence(("chat-thinking-mark", turn), thinking, crate::motion::SNAPPY);
        let body = self
            .stage
            .presence(("chat-thoughts", turn), open, crate::motion::SETTLE);
        let hover = crate::motion::reveal_key(("chat-tool-disclosure", index));
        let ink = crate::motion::mix(t.muted, t.text, crate::motion::revealed(hover));
        let label = div()
            .min_w_0()
            .truncate()
            .text_color(if has_thoughts { ink } else { t.muted })
            .map(|label| {
                if thinking {
                    label.child(crate::motion::sheen_label(
                        ("chat-thinking", self.chat_epoch),
                        heading,
                        13.,
                        t.muted,
                        t.text,
                        cx,
                    ))
                } else {
                    label.child(heading)
                }
            });
        let line = div()
            .id(("chat-tool-disclosure", index))
            .flex()
            .items_center()
            .h(px(20.))
            .text_size(px(13.))
            .line_height(px(18.))
            // The mark leaves by closing up, so the words slide to the edge.
            .when(mark.visible, |line| {
                let level = mark.value.clamp(0., 1.);
                // Room for its corners as it turns: 20 px square, the
                // mark centred in it.
                line.child(
                    div()
                        .flex_shrink_0()
                        .w(px(24. * level))
                        .h(px(20.))
                        .overflow_hidden()
                        .opacity(mark.fade)
                        .child(
                            div()
                                .size(px(20.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(crate::motion::thinking_mark(15., t.text)),
                        ),
                )
            })
            .child(crate::motion::swapped(label, swap))
            .when(has_thoughts, |line| {
                line.child(crate::motion::hover_probe(hover))
                    .cursor_default()
                    .child(disclosure_chevron(12., ink, body).ml(px(4.)))
                    .reach()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(item) = this.chat_annotations.get_mut(&index) {
                            item.expanded = Some(!open);
                        }
                        cx.notify();
                    }))
            });
        let mut result = div().min_w_0().flex().flex_col().child(line);
        let Some(annotations) = annotations.filter(|_| body.visible) else {
            return Some(result);
        };
        // One rail down the leading edge, as far as the thoughts go.
        let mut rail = div()
            .min_w_0()
            .mt(px(8.))
            .mb(px(2.))
            .pl(px(12.))
            .border_l_1()
            .border_color(t.border)
            .flex()
            .flex_col()
            .gap(px(10.));
        let today = chrono::Local::now().date_naive();
        for (position, thought) in annotations.thoughts().into_iter().enumerate() {
            rail = rail.child(match thought {
                Thought::Reasoning { heading, body } => {
                    self.render_chat_reasoning(index, position, heading, body)
                }
                Thought::Step(step) => {
                    self.render_chat_step(index, &annotations.tools[step], annotations, active, today, cx)
                }
            });
        }
        result = result.children(crate::motion::fold(
            &self.stage,
            ("chat-thoughts", turn),
            body,
            rail,
        ));
        Some(result)
    }

    /// A part of the model's reasoning: its heading in fuller ink, then its
    /// paragraphs in the quiet ink of a caption.
    fn render_chat_reasoning(
        &self,
        index: usize,
        position: usize,
        heading: Option<&str>,
        body: &str,
    ) -> AnyElement {
        let t = self.theme();
        div()
            .id(("chat-reasoning", index * 1000 + position))
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(6.))
            .text_size(px(13.))
            .line_height(px(19.))
            .when_some(heading, |part, heading| {
                part.child(
                    div()
                        .text_color(crate::motion::mix(t.muted, t.text, 0.7))
                        .child(inline_text(heading, t)),
                )
            })
            .children(
                body.split("\n\n")
                    .map(str::trim)
                    .filter(|paragraph| !paragraph.is_empty())
                    .map(|paragraph| {
                        div()
                            .min_w_0()
                            .text_color(t.muted)
                            .child(inline_text(paragraph, t))
                    }),
            )
            .into_any_element()
    }

    /// One meeting lookup: a glyph for what it did, what it found, and (when
    /// it found meetings) a chevron that opens them, one to a row with its
    /// day at the trailing edge. A row opens its note.
    fn render_chat_step(
        &self,
        index: usize,
        tool: &ChatToolProgressDTO,
        annotations: &ChatTurnAnnotations,
        active: bool,
        today: chrono::NaiveDate,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = self.theme();
        let call_id = tool.call_id.clone();
        let results = tool.results.as_deref().unwrap_or(&[]);
        let has_results = !results.is_empty();
        let look = step_look(tool, active);
        let key = crate::lazy::key((index, &call_id));
        let pose = self.stage.presence(
            ("chat-step", key),
            has_results && annotations.expanded_tools.contains(&call_id),
            crate::motion::SETTLE,
        );
        let hover = crate::motion::reveal_key(("chat-step", key));
        let quiet = look.state == StepState::Failed;
        let rest = if quiet { crate::motion::mix(t.muted, t.bg, 0.3) } else { t.muted };
        let ink = if has_results {
            crate::motion::mix(rest, t.text, crate::motion::revealed(hover))
        } else {
            rest
        };
        let row = div()
            .id(SharedString::from(format!("chat-tool-{index}-{call_id}")))
            .flex()
            .items_center()
            .gap(px(7.))
            .h(px(20.))
            .text_size(px(13.))
            .line_height(px(18.))
            .text_color(ink)
            .child(icon(look.glyph, 14., ink).flex_shrink_0())
            .child(div().min_w_0().truncate().map(|label| {
                if look.state == StepState::Running {
                    label.child(crate::motion::sheen_label(
                        SharedString::from(format!("chat-step-sheen-{index}-{call_id}")),
                        look.label.clone(),
                        13.,
                        t.muted,
                        t.text,
                        cx,
                    ))
                } else {
                    label.child(look.label.clone())
                }
            }))
            .when(has_results, |row| {
                row.cursor_default()
                    .child(crate::motion::hover_probe(hover))
                    .child(disclosure_chevron(11., ink, pose).ml(px(-2.)))
                    .reach()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(item) = this.chat_annotations.get_mut(&index)
                            && !item.expanded_tools.remove(&call_id)
                        {
                            item.expanded_tools.insert(call_id.clone());
                        }
                        cx.notify();
                    }))
            });
        let mut step = div().min_w_0().flex().flex_col().child(row);
        if pose.visible {
            // Their own rail, under the middle of the glyph.
            let mut list = div()
                .min_w_0()
                .mt(px(6.))
                .ml(px(6.5))
                .pl(px(6.))
                .border_l_1()
                .border_color(t.border)
                .flex()
                .flex_col();
            for meeting in results {
                let source_id = meeting.note_id.clone();
                let row_hover = crate::motion::reveal_key(("chat-step-meeting", key, &source_id));
                let warmth = crate::motion::revealed(row_hover);
                list = list.child(
                    div()
                        .id(SharedString::from(format!(
                            "chat-tool-meeting-{index}-{}-{source_id}",
                            tool.call_id
                        )))
                        .relative()
                        .h(px(26.))
                        .px(px(8.))
                        .rounded(px(6.))
                        .cursor_default()
                        .child(crate::motion::tactile(t.hover).radius(6.).reveals(row_hover))
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .text_size(px(13.))
                        .line_height(px(18.))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_color(crate::motion::mix(t.muted, t.text, warmth))
                                .child(if meeting.title.trim().is_empty() {
                                    "New note".into()
                                } else {
                                    meeting.title.clone()
                                }),
                        )
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_size(px(12.))
                                .text_color(t.muted)
                                .child(relative_day(&meeting.date, today)),
                        )
                        .reach()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.open_chat_source_note(&source_id, cx)
                        })),
                );
            }
            step = step.children(crate::motion::fold(&self.stage, ("chat-step", key), pose, list));
        }
        step.into_any_element()
    }

    fn chat_markdown(
        &self,
        index: usize,
        answer: &str,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        // A conversation about one note shows no source marks: everything
        // in it comes from that note. Across notes a mark says which meeting
        // a statement came from.
        let citations = self
            .chat_annotations
            .get(&index)
            .filter(|_| self.current_note.is_none())
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
            let ranges: Vec<_> = links.iter().map(|(range, _)| range.clone()).collect();
            let citations = citations.clone();
            let weak = weak.clone();
            let styled = StyledText::new(text).with_highlights(highlights);
            // Each source's place in the text and its number, for its chip.
            let marks: Vec<_> = links
                .iter()
                .filter_map(|(range, destination)| {
                    let number = destination.strip_prefix("minutes-source:S")?.parse::<u32>().ok()?;
                    Some((range.clone(), number))
                })
                .collect();
            let chips = (!marks.is_empty()).then(|| citation_chips(styled.layout().clone(), marks, t));
            let text = InteractiveText::new(("chat-sourced-markdown", block), styled)
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
            });
            div().relative().children(chips).child(text).into_any_element()
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
            // A chat whose note is gone from here (in Trash, or deleted)
            // still opens, by itself, as a full chat.
            let beside_note = chat.note_id != 0
                && self
                    .workspace
                    .note(chat.note_id)
                    .is_some_and(|note| !note.trashed);
            if beside_note {
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
                self.open_chat_page();
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
        // A reader at the end of the conversation stays there while it grows:
        // an answer arriving, its thinking, a new question. One who scrolled
        // up is left where they are, and nothing moves while the conversation
        // stands still (opening a fold does not pull the page down).
        let signature = self.chat_follow_signature();
        let grew = self.chat_follow.replace(signature) != signature;
        if (grew || self.chat_stream.is_some()) && chat_at_end(&self.chat_scroll) {
            self.chat_to_bottom();
        }
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
                self.sidebar_inset()
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
        (f32::from(window.viewport_size().width) - self.sidebar_inset()).clamp(240., 672.)
    }

    /// What the conversation holds, for telling when it has grown: its turns
    /// and the length of the newest one.
    pub(crate) fn chat_follow_signature(&self) -> u64 {
        let last = self
            .chat_messages
            .last()
            .map_or(0, |(question, answer)| question.len() + answer.len());
        ((self.chat_messages.len() as u64) << 40) ^ last as u64
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
                            .rounded(px(if compact { 10. } else { 12. }))
                            // On a page of its own the bubble is 33 pt tall
                            // around 15 pt text, 16 pt in at the sides, as
                            // the reference measures.
                            .px(px(if compact { 11. } else { 16. }))
                            .py(px(6.))
                            .bg(t.hover)
                            .text_size(px(if compact { 14. } else { 15. }))
                            .line_height(px(if compact { 20. } else { 21. }))
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
                        // On a page of its own: a 31 pt pill in full ink,
                        // 35 pt under the answer's last line (the reference).
                        .h(px(if compact { 30. } else { 33. }))
                        .when(!compact, |row| row.mt(px(10.)))
                        .child(
                            pill(("chat-say-more", index), "Say more", "", t)
                                .h(px(if compact { 28. } else { 31. }))
                                .when(!compact, |pill| pill.px(px(14.)).border_color(t.border))
                                .text_size(px(if compact { 13. } else { 14. }))
                                .text_color(if compact { t.secondary } else { t.text })
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
                                    // Copied text carries no source marks.
                                    let copy = rewrite_citation_tokens(answer, &[], |_| String::new());
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

    /// The model the next question goes to: the one the person picked, else
    /// the instance default, which the server lists first. `None` until the
    /// server's models are known.
    pub(crate) fn chat_model_choice(&self) -> Option<&meeting_notes_api_types::LanguageModelSelection> {
        self.chat_models
            .iter()
            .find(|choice| Some(crate::server::chat_model_key(choice)) == self.selected_chat_model)
            .or_else(|| self.chat_models.first())
    }

    /// That model's name, as its picker button shows it. Without a server's
    /// list there is no name to show, only that the server chooses.
    fn chat_model_label(&self) -> String {
        self.chat_model_choice()
            .map(|choice| model_display_name(&choice.model))
            .unwrap_or_else(|| "Auto".into())
    }

    /// The model picker's button: the model the next question goes to and
    /// a small chevron. The list of the server's models opens from it.
    pub(crate) fn render_chat_model_button(
        &self,
        id: &'static str,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let t = self.theme();
        let hover = crate::motion::reveal_key(("chat-model-button", id));
        let warmth = crate::motion::revealed(hover).max(if self.chat_model_picker_open { 1. } else { 0. });
        let ink = crate::motion::mix(t.muted, t.text, warmth);
        div()
            .id(id)
            .relative()
            .flex()
            .items_center()
            .gap(px(3.))
            .h(px(28.))
            .px(px(8.))
            .rounded(px(8.))
            .text_size(px(13.))
            .line_height(px(18.))
            .text_color(ink)
            .child(crate::motion::tactile(t.hover).radius(8.).reveals(hover))
            .child(self.chat_model_label())
            .child(icon("chevron-down", 11., ink))
            .cursor_default()
            .reach()
            .on_click(cx.listener(|this, _, _, cx| {
                if crate::server::enabled() || !this.chat_models.is_empty() {
                    this.toggle_chat_model_picker(cx);
                }
            }))
    }

    /// Choose row `index` of the model menu and close it. The first row is
    /// the instance default: choosing it names no model, so the question
    /// follows the default if an administrator changes it.
    fn choose_chat_model(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(choice) = self.chat_models.get(index) {
            self.selected_chat_model = (index > 0).then(|| crate::server::chat_model_key(choice));
        }
        self.chat_model_picker_open = false;
        self.chat_model_cursor = None;
        cx.notify();
    }

    /// The model menu's keys while it is open: the arrows move the
    /// highlight, Return chooses it, Escape closes the menu. True when the
    /// key was the menu's.
    pub(crate) fn handle_chat_model_key(
        &mut self,
        event: &KeystrokeEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.chat_model_picker_open {
            return false;
        }
        let modifiers = event.keystroke.modifiers;
        let modified = modifiers.control || modifiers.alt || modifiers.shift || modifiers.platform;
        let count = self.chat_models.len();
        let current = self.chat_model_cursor.or_else(|| {
            let chosen = self.chat_model_choice()?;
            self.chat_models.iter().position(|choice| choice == chosen)
        });
        match model_menu_key(&event.keystroke.key, modified, current, count) {
            Some(ModelMenuKey::Move(index)) => {
                self.chat_model_cursor = Some(index);
                cx.notify();
            }
            Some(ModelMenuKey::Choose(index)) => self.choose_chat_model(index, cx),
            Some(ModelMenuKey::Close) => {
                self.chat_model_picker_open = false;
                self.chat_model_cursor = None;
                cx.notify();
            }
            None => return false,
        }
        cx.stop_propagation();
        true
    }

    /// The list of models, growing out of the model button and folding back
    /// into it: above the button and ending where it ends, or (`below`)
    /// under the leading end of the Chat page's composer. A quiet "Models"
    /// heading, then a row a model: a glyph for who makes it, its name, a
    /// "Default" tag on the instance default, and a check on the one in use.
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
        let chosen = self
            .chat_model_choice()
            .and_then(|chosen| self.chat_models.iter().position(|choice| choice == chosen));
        let cursor = self.chat_model_cursor;
        let list = div()
            .id("chat-model-options")
            .absolute()
            .map(|list| {
                if below {
                    list.top(px(84.)).left(px(8.))
                } else {
                    // Clear of the pill's top edge, ending where the button ends.
                    list.bottom(px(64.)).right(px(46.))
                }
            })
            .w(px(232.))
            .max_h(px(320.))
            .overflow_y_scroll()
            .p(px(4.))
            .rounded(px(10.))
            .border_1()
            .border_color(t.border)
            .bg(t.card)
            .shadow(vec![
                BoxShadow {
                    color: hsla(0., 0., 0., 0.16),
                    offset: point(px(0.), px(8.)),
                    blur_radius: px(24.),
                    spread_radius: px(0.),
                },
                BoxShadow {
                    color: hsla(0., 0., 0., 0.08),
                    offset: point(px(0.), px(1.)),
                    blur_radius: px(3.),
                    spread_radius: px(0.),
                },
            ])
            .occlude()
            .cursor_default()
            // A press anywhere else closes the menu, and only the menu.
            .on_mouse_down_out(cx.listener(|this, _, window, cx| {
                // The press closes the menu and nothing else: the field
                // keeps the keyboard, so the bar stays open until the next.
                if this.chat_model_picker_open {
                    window.prevent_default();
                }
                this.dismiss_chat_model_picker(cx)
            }))
            .child(
                div()
                    .h(px(26.))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .text_size(px(12.))
                    .text_color(t.muted)
                    .child("Models"),
            )
            .children(self.chat_models.iter().enumerate().map(|(index, choice)| {
                let selected = chosen == Some(index);
                // The row the arrows are on, else the one in use.
                let lit = cursor.map_or(selected, |cursor| cursor == index);
                div()
                    .id(("chat-model-option", index))
                    .relative()
                    .h(px(28.))
                    .px(px(8.))
                    .rounded(px(6.))
                    .cursor_default()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_size(px(13.5))
                    .line_height(px(18.))
                    .text_color(t.text)
                    .when(lit, |row| row.bg(t.hover))
                    .child(crate::motion::tactile(t.hover).radius(6.))
                    .child(icon(model_glyph(choice), 14., t.secondary).flex_shrink_0())
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .child(model_display_name(&choice.model)),
                    )
                    .when(index == 0, |row| {
                        row.child(
                            div()
                                .flex_shrink_0()
                                .h(px(16.))
                                .px(px(5.))
                                .rounded(px(4.))
                                .bg(t.hover)
                                .flex()
                                .items_center()
                                .text_size(px(10.))
                                .line_height(px(12.))
                                .text_color(t.muted)
                                .child("Default"),
                        )
                    })
                    .child(div().flex_1())
                    .when(selected, |row| {
                        row.child(icon("check", 13., t.muted).flex_shrink_0())
                    })
                    .reach()
                    .on_click(cx.listener(move |this, _, _, cx| this.choose_chat_model(index, cx)))
            }));
        // Painted after everything else and outside any clipping: Home's
        // floating chat panel is shorter than the menu while it is empty,
        // and would otherwise cut its first rows off.
        Some(
            deferred(crate::motion::pop(
                list,
                crate::motion::Pop::menu(if below { (0., 0.) } else { (1., 1.) }),
                picker,
            ))
            .with_priority(2)
            .into_any_element(),
        )
    }

    /// The composer pill. `folded` is Home's bar at rest (not focused,
    /// nothing typed): only the field shows, and the model and send controls
    /// come in at its trailing end when it is focused.
    pub fn render_chat_composer(
        &self,
        focused: bool,
        folded: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        self.render_chat_composer_ringed(focused, folded, false, cx)
    }

    /// The composer as Home's Ask bar has it: its focus halo is drawn inside
    /// the pill's outline, so the pill fills the same box focused and not
    /// and stands evenly inside its tray.
    pub fn render_chat_composer_in_tray(
        &self,
        focused: bool,
        folded: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        self.render_chat_composer_ringed(focused, folded, true, cx)
    }

    /// Nothing about the pill's layout depends on the focus: the same box,
    /// the same padding, the same text position. The gold line and its halo
    /// are painted over it (`ask_ring_overlay`) and swell in with the ring's
    /// track, so taking the focus cannot move the text or the caret line.
    fn render_chat_composer_ringed(
        &self,
        focused: bool,
        folded: bool,
        inset: bool,
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
        let ring = crate::motion::ask_shape(self.stage.presence(
            ("chat-composer-ring", 0),
            focused,
            crate::motion::ASK_TRAY,
        ));
        let (line, halo) = crate::ui::ask_ring();
        let edge = crate::motion::mix(
            t.border,
            t.accent.opacity(0.33),
            crate::motion::revealed(composer_hover),
        );
        let cluster = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .h(px(32.))
            .when(self.chat_editing.is_none(), |d| {
                d.child(self.render_chat_model_button("chat-configured-model", cx).mr(px(2.)))
            })
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
                crate::ui::send_button("send-chat-stream", "Send · Enter", true, t)
                    .on_click(cx.listener(|this, _, _, cx| this.submit_chat(cx)))
            } else {
                // Nothing typed yet: the send arrow, dimmed.
                crate::ui::send_button("send-chat-stream-idle", "Type a question to send", false, t)
            });
        div()
            .id("chat-composer-pill")
            .relative()
            .children(self.render_chat_model_picker(!folded, false, cx))
            .w_full()
            // 58 px tall at rest, the field 21 px in from the leading edge
            // and the round button 10 px from the trailing one (measured
            // against the reference at 1.25 px per point).
            .min_h(px(crate::motion::ASK_PILL))
            .max_h(px(170.))
            .pl(px(21.))
            .pr(px(10.))
            .py(px(12.))
            .rounded_full()
            .border_1()
            // The resting edge gives way to the gold line as it arrives. In
            // a tray the line is drawn further in, and the edge it leaves
            // dissolves into the tray, which is the pill's own colour.
            .border_color(if inset {
                edge.opacity(edge.a * (1. - ring.ring_fade))
            } else {
                crate::motion::mix(edge, line, ring.ring_fade)
            })
            .bg(t.card)
            .child(crate::motion::hover_probe(composer_hover))
            .shadow(vec![BoxShadow {
                color: hsla(0., 0., 0.16, 0.04 * (1. - ring.ring_fade)),
                offset: point(px(0.), px(2.)),
                blur_radius: px(5.),
                spread_radius: px(0.),
            }])
            .when(ring.ring_fade > 0., |pill| {
                pill.child(ask_ring_overlay(
                    ring.ring,
                    line.opacity(line.a * ring.ring_fade),
                    halo.opacity(halo.a * ring.ring_fade),
                    inset,
                ))
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
            // Focused, the controls come in from the trailing edge; they
            // leave the same way when the bar folds. They are laid out past
            // the placeholder, which stays where it is.
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

    /// The model button: open the menu, or close it. A press outside the
    /// open menu closes it; when that press was on the button, the click it
    /// completes must not open the menu again (as `toggle_chat_history`).
    pub(crate) fn toggle_chat_model_picker(&mut self, cx: &mut Context<Self>) {
        let just_closed = self
            .chat_model_picker_closed_at
            .take()
            .is_some_and(|at| at.elapsed() < std::time::Duration::from_millis(500));
        if !just_closed {
            self.chat_model_picker_open = !self.chat_model_picker_open;
        }
        self.chat_model_cursor = None;
        cx.notify();
    }

    /// Whether a popover of the composer (the model menu, the recent chats)
    /// is open, or was closed by the press being handled now. A press
    /// outside the composer that closes one of them does only that, as
    /// Escape does; the next one folds the bar.
    pub(crate) fn chat_popover_open(&self) -> bool {
        let now = |at: Option<std::time::Instant>| {
            at.is_some_and(|at| at.elapsed() < std::time::Duration::from_millis(80))
        };
        self.chat_model_picker_open
            || self.chat_history_open
            || now(self.chat_model_picker_closed_at)
            || now(self.chat_history_closed_at)
    }

    /// A press outside the model menu: close it.
    pub(crate) fn dismiss_chat_model_picker(&mut self, cx: &mut Context<Self>) {
        if self.chat_model_picker_open {
            self.chat_model_picker_open = false;
            self.chat_model_cursor = None;
            self.chat_model_picker_closed_at = Some(std::time::Instant::now());
            cx.notify();
        }
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
            // A chat about a note in Trash is still listed: it opens by itself.
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
    /// Scroll to latest: a filled capsule with a down arrow, centred and
    /// floating just above the composer on a soft shadow. Only there while
    /// the end of the conversation is out of sight (`chat_can_scroll_down`).
    pub fn render_chat_scroll_down(&self, cx: &mut Context<Self>) -> Div {
        let t = self.theme();
        div()
            .absolute()
            .left_0()
            .right_0()
            .bottom(px(if self.chat_editing.is_some() {
                118.
            } else {
                74.
            }))
            .flex()
            .justify_center()
            .child(
                round_tool("chat-scroll-down", "arrow-down", "Scroll to latest", t)
                    .w(px(36.))
                    .h(px(26.))
                    .rounded_full()
                    .bg(crate::motion::mix(t.card, t.text, 0.12))
                    .text_color(t.text)
                    .shadow(vec![BoxShadow {
                        color: hsla(0., 0., 0., 0.16),
                        offset: point(px(0.), px(3.)),
                        blur_radius: px(10.),
                        spread_radius: px(0.),
                    }])
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

/// The room a citation takes in the text, where its chip is painted
/// (`citation_chips`): figures one more than its digits, drawn invisible
/// (`CITATION_ROOM`), a disc for one digit and a capsule for more. Figures
/// because the text face's spaces are too narrow to be relied on. The number
/// itself is painted on the chip, smaller than the text, as the reference
/// sets it.
fn citation_room(number: u32) -> String {
    CITATION_ROOM.repeat(number.to_string().len() + 1)
}
/// What a citation's room is made of: a figure, drawn in no colour.
const CITATION_ROOM: &str = "0";

/// The chips of a paragraph's sources: a light grey disc (a capsule for two
/// or more digits) in the room each one has in the text, centred on its
/// line, with its number in small grey figures. A text background cannot
/// draw them (it fills the whole line box) and text cannot change size
/// within a line, so both are painted where the text laid the room out.
fn citation_chips(layout: TextLayout, marks: Vec<(std::ops::Range<usize>, u32)>, t: Theme) -> AnyElement {
    let chip = crate::motion::mix(t.hover, t.text, 0.04);
    let ink = t.muted;
    canvas(
        |_, _, _| (),
        move |_, _, window, cx| {
            let line = layout.line_height();
            let tall = (line * 0.78).min(px(18.));
            for (range, number) in &marks {
                let (Some(start), Some(end)) = (
                    layout.position_for_index(range.start),
                    layout.position_for_index(range.end),
                ) else {
                    continue;
                };
                // A mark the line breaks inside is left without a chip.
                if start.y != end.y {
                    continue;
                }
                let wide = end.x - start.x;
                let top = start.y + (line - tall) / 2.;
                window.paint_quad(
                    fill(Bounds::new(point(start.x, top), size(wide, tall)), chip)
                        .corner_radii(tall / 2.),
                );
                let label = SharedString::from(number.to_string());
                let run = TextRun {
                    len: label.len(),
                    font: gpui::font(crate::ui::FONT),
                    color: ink,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                let shaped = window
                    .text_system()
                    .shape_line(label, px(11.), &[run], None);
                let origin = point(start.x + (wide - shaped.width) / 2., top);
                let _ = shaped.paint(origin, tall, window, cx);
            }
        },
    )
    .absolute()
    .size_full()
    .into_any_element()
}

/// Each mark is a small chip: its raised number padded by a thin space on
/// either side (the chip's fill covers them), set a hair apart from the
/// words and from the next mark, so two side by side never read as one
/// number.
fn citation_tokens(source: &str, citations: &[ChatCitationDTO]) -> String {
    rewrite_citation_tokens(source, citations, |id| {
        let room = citation_room(id[1..].parse::<u32>().unwrap_or(0));
        // A word space before each chip, so two side by side stay apart.
        format!(" [{room}](minutes-source:{id})")
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
            // The space the model put before a marker goes with it: a mark
            // is set against its sentence, and a withdrawn one leaves no gap.
            if output.ends_with(' ') && !output.ends_with("  ") {
                output.pop();
            }
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
    // In an answer's softer running text, bold is full ink.
    for (_, style) in &mut highlights {
        if style.font_weight.is_some() && style.color.is_none() {
            style.color = Some(theme.text);
        }
    }
    for (range, url) in &links {
        // A source's room is drawn in no colour: its chip and number are
        // painted (`citation_chips`). A link to a page is in the accent.
        let source = url.starts_with("minutes-source:");
        highlights.push((
            range.clone(),
            HighlightStyle {
                // Colours blend over the text's own, so the room is faded out
                // entirely rather than coloured clear.
                color: (!source).then_some(theme.accent),
                fade_out: source.then_some(1.),
                font_weight: source.then_some(FontWeight::NORMAL),
                ..Default::default()
            },
        ));
    }
    (output, highlights, links)
}
/// Distance between an answer's blocks, and between the items of one list.
const ANSWER_GAP: f32 = 15.;
const ITEM_GAP: f32 = 4.;

/// The ink an answer's running text is set in: full ink, as the reference
/// sets it (its answers measure #292929, the same as its titles).
pub(crate) fn answer_ink(theme: Theme) -> Hsla {
    theme.text
}

fn markdown_with_inline(
    source: &str,
    theme: Theme,
    compact: bool,
    render_inline: impl Fn(&str, usize) -> AnyElement,
) -> Div {
    let mut result = div()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(ANSWER_GAP))
        .text_color(answer_ink(theme))
        .text_size(px(if compact { 14. } else { 16. }))
        .line_height(px(if compact { 22. } else { 24. }));
    let mut after_item = false;
    for (index, block) in parsed_blocks(source).iter().cloned().enumerate() {
        let item = matches!(block, Block::Item(..));
        // Items of one list sit close together: 4 px apart, not a
        // paragraph's distance.
        let close = item && after_item;
        after_item = item;
        result = result.child(match block {
            Block::Heading(level, text) => div()
                .mt(px(if level < 3 { 8. } else { 2. }))
                .text_color(theme.text)
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
                .when(close, |item| item.mt(px(ITEM_GAP - ANSWER_GAP)))
                .flex()
                .items_start()
                .gap(px(8.))
                .child(if marker == "•" {
                    // A small quiet dot, centred on the first line.
                    div()
                        .w(px(14.))
                        .h(px(if compact { 22. } else { 24. }))
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .size(px(4.))
                                .rounded_full()
                                .bg(crate::motion::mix(theme.muted, theme.bg, 0.25)),
                        )
                } else {
                    div()
                        .w(px((marker.chars().count() as f32 * 8.).max(14.)))
                        .whitespace_nowrap()
                        .flex_shrink_0()
                        .text_color(theme.muted)
                        .child(marker)
                })
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

/// The focused composer's gold line and the hard-edged halo round it,
/// painted over the pill without taking part in its layout. `width` is how
/// far the halo has swelled. Outside the pill by default; `inset`, both are
/// drawn inside the pill's outline (the halo fills its outer `width`, the
/// line runs along the halo's inner edge), so the pill's box is the same
/// with and without them.
fn ask_ring_overlay(width: f32, line: Hsla, halo: Hsla, inset: bool) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let width = px(width.max(0.));
            let (band, edge) = if inset {
                (bounds, bounds.inset(width))
            } else {
                (bounds.dilate(width), bounds)
            };
            let round = |rect: Bounds<Pixels>| Corners::all(rect.size.height / 2.);
            if width > px(0.) {
                window.paint_quad(quad(
                    band,
                    round(band),
                    transparent_black(),
                    Edges::all(width),
                    halo,
                    BorderStyle::default(),
                ));
            }
            window.paint_quad(quad(
                edge,
                round(edge),
                transparent_black(),
                Edges::all(px(1.)),
                line,
                BorderStyle::default(),
            ));
        },
    )
    // Over the pill's whole box, border included: an absolute child is
    // placed inside its parent's 1 px border.
    .absolute()
    .top(px(-1.))
    .left(px(-1.))
    .right(px(-1.))
    .bottom(px(-1.))
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
    use super::{
        ChatToolProgressDTO, ModelMenuKey, StepLook, StepState, Thought, model_display_name,
        model_glyph, model_menu_key, reasoning_sections, relative_day, rewrite_citation_tokens,
        step_look,
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
                .contains(" 00.")
        );
        let mut annotations = ChatTurnAnnotations::default();
        annotations.error = Some(
            "A source changed while answering. Retry to use the current meeting notes.".into(),
        );
        let rendered =
            linked_inline_with_citations(partial, Theme::new(true), &annotations.citations);
        assert_eq!(
            rendered.0,
            // The space before a withdrawn mark goes with it.
            "Café launches Friday. Keep this partial response."
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
    fn only_homes_quick_chat_floats_and_every_other_page_opens_the_chat_page() {
        use crate::app::Screen;
        assert!(super::floating_workspace_chat(Screen::Home));
        for screen in [
            Screen::MyNotes,
            Screen::Favorites,
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
        assert_eq!(text, "Café 🦊 00 and docs.");
        assert_eq!(links.len(), 2);
        assert_eq!(&text[links[0].0.clone()], "00");
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
        // Any number is written the same way, in raised digits; a hair
        // space before each keeps neighbours from running together
        // ("[[S25]][[S26]]" must not read "2526"), and none is ever
        // written out in brackets.
        assert_eq!([1, 10, 999].map(super::citation_room), ["00", "000", "0000"]);
        let later = vec![citation("S25"), citation("S26"), citation("S51"), citation("S52")];
        let (text, _, links) = linked_inline_with_citations(
            "Moved. [[S25]][[S26]] Later. [[S51]][[S52]]",
            Theme::new(true),
            &later,
        );
        assert_eq!(text, "Moved. 000 000 Later. 000 000");
        assert!(!text.contains('('));
        // With no sources (a conversation about one note) the markers and
        // the space before them go, and nothing is left in their place.
        let (bare, _, none) = linked_inline_with_citations("Moved. [[S25]][[S26]] Later. [[S51]]", Theme::new(true), &[]);
        assert_eq!(bare, "Moved. Later.");
        assert!(none.is_empty());
        assert_eq!(rewrite_citation_tokens("Done. [[S57]][[S58]]", &[], |_| String::new()), "Done.");
        assert_eq!(links.len(), 4);
        for id in ["S0", "S01", "S1000", "S-1", "S1/other", "s1", "S"] {
            assert!(!valid_citation_id(id));
        }
        assert!(valid_citation_id("S999"));
    }
    fn step(call: &str, name: &str, status: &str, label: &str) -> ChatToolProgressDTO {
        serde_json::from_value(serde_json::json!({"callId": call, "name": name, "status": status, "label": label})).unwrap()
    }

    #[test]
    fn reasoning_and_lookups_are_kept_in_the_order_they_happened() {
        let mut notes = ChatTurnAnnotations::default();
        // Pieces join into one stretch until a new lookup starts.
        notes.add_reasoning("  **Finding the meeting**\n\nI will search");
        notes.add_reasoning(" for it first.");
        notes.update_tool(step("a", "search_meetings", "running", "Searching meetings"));
        // The same lookup finishing is not a new one.
        notes.update_tool(step("a", "search_meetings", "completed", "Found 2 meetings"));
        notes.add_reasoning("\n\nNow read both.");
        notes.update_tool(step("b", "get_meetings", "completed", "Looked deeper into 2 notes"));
        assert_eq!(
            notes.reasoning.iter().map(|item| (item.after_tools, item.text.as_str())).collect::<Vec<_>>(),
            [(0, "**Finding the meeting**\n\nI will search for it first."), (1, "Now read both.")]
        );
        assert_eq!(
            notes.thoughts(),
            [
                Thought::Reasoning { heading: Some("Finding the meeting"), body: "I will search for it first." },
                Thought::Step(0),
                Thought::Reasoning { heading: None, body: "Now read both." },
                Thought::Step(1),
            ]
        );
        // As the server stores it, so a reopened thread reads the same.
        let stored = serde_json::to_value(&notes).unwrap();
        assert_eq!(stored["reasoning"][1], serde_json::json!({"afterTools": 1, "text": "Now read both."}));
        let restored: ChatTurnAnnotations = serde_json::from_value(stored).unwrap();
        assert_eq!(restored.thoughts(), notes.thoughts());
        // Reasoning after the last lookup comes last; none at all is just
        // the lookups, and nothing is made up.
        notes.add_reasoning("**Answering**\n\nEnough to answer.");
        assert_eq!(notes.thoughts().last(), Some(&Thought::Reasoning { heading: Some("Answering"), body: "Enough to answer." }));
        let mut plain = ChatTurnAnnotations::default();
        plain.update_tool(step("a", "list_meetings", "completed", "Found 3 meetings"));
        assert_eq!(plain.thoughts(), [Thought::Step(0)]);
        assert!(plain.has_thoughts() && !ChatTurnAnnotations::default().has_thoughts());
        // A stretch is bounded, as the server bounds it.
        let mut long = ChatTurnAnnotations::default();
        long.add_reasoning(&"é".repeat(30_000));
        assert_eq!(long.reasoning[0].text.chars().count(), 20_000);
    }

    #[test]
    fn a_stretch_of_reasoning_splits_at_its_bold_heading_lines() {
        assert_eq!(
            reasoning_sections("Before any heading.\n**One**\n\nFirst body with **bold** inside.\n\nSecond paragraph.\n\n**Two**\nBody two."),
            [
                (None, "Before any heading."),
                (Some("One"), "First body with **bold** inside.\n\nSecond paragraph."),
                (Some("Two"), "Body two."),
            ]
        );
        assert_eq!(reasoning_sections("Just thinking aloud."), [(None, "Just thinking aloud.")]);
        assert_eq!(reasoning_sections("**Only a heading**"), [(Some("Only a heading"), "")]);
        assert!(reasoning_sections(" \n ").is_empty());
    }

    #[test]
    fn the_thinking_line_names_what_is_being_done() {
        let mut notes = ChatTurnAnnotations::default();
        assert_eq!(notes.live_heading(), None);
        // A heading shows once its line is whole, not letter by letter.
        notes.add_reasoning("**Searching for rec");
        assert_eq!(notes.live_heading(), None);
        notes.add_reasoning("ent dates**\n\nI need");
        assert_eq!(notes.live_heading().as_deref(), Some("Searching for recent dates…"));
        // A lookup under way is more telling than the last heading.
        notes.update_tool(step("a", "search_meetings", "running", "Searching meetings"));
        assert_eq!(notes.live_heading().as_deref(), Some("Searching meetings…"));
        notes.update_tool(step("a", "search_meetings", "completed", "Found 2 meetings"));
        assert_eq!(notes.live_heading().as_deref(), Some("Searching for recent dates…"));
    }

    #[test]
    fn a_lookup_has_a_fitting_glyph_and_a_failure_is_quiet_plain_language() {
        let look = |name, status, label, active| step_look(&step("a", name, status, label), active);
        assert_eq!(look("search_meetings", "completed", "Found 2 meetings", false), StepLook { glyph: "step-notes", label: "Found 2 meetings".into(), state: StepState::Done });
        assert_eq!(look("get_meeting_transcript", "completed", "Read 2 transcripts", false).glyph, "footer-transcript");
        assert_eq!(look("list_meetings", "completed", "Found 4 meetings", false).glyph, "calendar");
        assert_eq!(look("search_meetings", "running", "Searching meetings", true), StepLook { glyph: "step-notes", label: "Searching meetings…".into(), state: StepState::Running });
        // The server's diagnostic is never printed as it came.
        let failed = look("search_meetings", "failed", "Search planning unavailable; using direct meeting lookup", false);
        assert_eq!(failed, StepLook { glyph: "step-failed", label: "Couldn't plan the search".into(), state: StepState::Failed });
        assert_eq!(look("get_meetings", "failed", "Couldn't finish this lookup", false).label, "Couldn't finish this lookup");
        assert_eq!(look("get_meetings", "failed", "", false).label, "Couldn't finish this lookup");
        // Still running when the answer was stopped.
        assert_eq!(look("get_meetings", "running", "Reading meeting notes", false).label, "Stopped before this finished");
    }

    #[test]
    fn a_meetings_day_is_relative_and_names_the_year_only_when_it_differs() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 11, 16).unwrap();
        assert_eq!(relative_day("2026-11-16", today), "Today");
        assert_eq!(relative_day("2026-11-15", today), "Yesterday");
        assert_eq!(relative_day("2026-11-13", today), "Nov 13");
        assert_eq!(relative_day("2026-01-02", today), "Jan 2");
        assert_eq!(relative_day("2025-12-30", today), "Dec 30, 2025");
        assert_eq!(relative_day("2026-11-17", today), "Nov 17");
        assert_eq!(relative_day("2026-11-16T09:00:00Z", today), "Today");
        assert_eq!(relative_day("soon", today), "soon");
    }

    #[test]
    fn a_model_is_shown_by_its_name_not_its_id() {
        for (id, name) in [
            ("gpt-6-luna", "GPT-6 Luna"),
            ("openai/gpt-6-sol", "GPT-6 Sol"),
            ("gpt-6.1-sol", "GPT-6.1 Sol"),
            ("anthropic/claude-sonnet-5-5", "Sonnet 5.5"),
            ("claude-fable-5-2", "Fable 5.2"),
            ("claude-opus-5-5-20270115", "Opus 5.5"),
            ("google/gemini-4-carbon", "Gemini 4 Carbon"),
            ("x-ai/grok-4.7", "Grok 4.7"),
            ("o3-mini", "o3 Mini"),
            ("llama3.2:3b", "Llama3.2 3B"),
            ("mistral-large-latest", "Mistral Large"),
        ] {
            assert_eq!(model_display_name(id), name, "{id}");
        }
        let choice = |provider: &str, model: &str| -> meeting_notes_api_types::LanguageModelSelection {
            serde_json::from_value(serde_json::json!({"provider": provider, "model": model})).unwrap()
        };
        assert_eq!(model_glyph(&choice("openrouter", "anthropic/claude-sonnet-5-5")), "provider-anthropic");
        assert_eq!(model_glyph(&choice("openai", "gpt-6-luna")), "provider-openai");
        assert_eq!(model_glyph(&choice("openrouter", "google/gemini-4-carbon")), "google");
        assert_eq!(model_glyph(&choice("openrouter", "x-ai/grok-4.7")), "provider-xai");
        assert_eq!(model_glyph(&choice("openai_compatible", "llama3.2:3b")), "provider-other");
        assert_eq!(model_glyph(&choice("anthropic", "fable-next")), "provider-anthropic");
    }

    #[test]
    fn the_model_menu_answers_the_arrows_return_and_escape() {
        use ModelMenuKey::{Choose, Close, Move};
        assert_eq!(model_menu_key("down", false, Some(0), 3), Some(Move(1)));
        assert_eq!(model_menu_key("down", false, Some(2), 3), Some(Move(0)));
        assert_eq!(model_menu_key("up", false, Some(0), 3), Some(Move(2)));
        assert_eq!(model_menu_key("down", false, None, 3), Some(Move(0)));
        assert_eq!(model_menu_key("enter", false, Some(1), 3), Some(Choose(1)));
        assert_eq!(model_menu_key("enter", false, None, 3), None);
        assert_eq!(model_menu_key("escape", false, Some(1), 3), Some(Close));
        assert_eq!(model_menu_key("down", true, Some(1), 3), None);
        assert_eq!(model_menu_key("down", false, None, 0), None);
        assert_eq!(model_menu_key("a", false, Some(1), 3), None);
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
        annotations.expanded = Some(true);
        annotations.expanded_tools.insert("runtime-only".into());
        let json = serde_json::to_value(&annotations).unwrap();
        assert!(json.get("expanded").is_none());
        assert!(json.get("expanded_tools").is_none());
        let restored: ChatTurnAnnotations = serde_json::from_value(json).unwrap();
        assert_eq!(restored.citations[0].quote, "Mira owns café 🦊.");
        assert_eq!(restored.elapsed_ms, Some(3123));
        assert!(restored.expanded.is_none());
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
            Some("Can you provide more details")
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

/// Whether the conversation shows its end: within a few points of the bottom,
/// as the last frame laid it out. A wheel turn up of any size leaves it.
pub(crate) fn chat_at_end(scroll: &ScrollHandle) -> bool {
    scroll.max_offset().height + scroll.offset().y <= px(8.)
}
