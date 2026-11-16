//! Calendar associations and participants are server-owned note context, separate
//! from transcripts. Calendar names never establish a diarized voice identity.
use crate::{
    app::{Cereal, Menu},
    server::MeetingCommand,
    ui::*,
};
use chrono::{DateTime, Local};
use gpui::{prelude::*, *};
use meeting_notes_api_types::{
    CalendarEventDTO, ManualParticipantInput, MeetingContextDTO, MeetingParticipantDTO,
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

#[derive(Default)]
pub struct MeetingState {
    note_id: Option<u64>,
    pub loading: bool,
    pub error: Option<String>,
    epoch: u64,
    cancel: Option<Arc<AtomicBool>>,
    pub events: Vec<CalendarEventDTO>,
    pub events_loading: bool,
    pub events_error: Option<String>,
    picker_epoch: u64,
    picker_cancel: Option<Arc<AtomicBool>>,
    participants_scroll: ScrollHandle,
    events_scroll: ScrollHandle,
    /// A name entered while the note had no calendar event to keep it with.
    pending_attendee: Option<String>,
}
impl Drop for MeetingState {
    fn drop(&mut self) {
        if let Some(c) = &self.cancel {
            c.store(true, Ordering::Release);
        }
        if let Some(c) = &self.picker_cancel {
            c.store(true, Ordering::Release);
        }
    }
}
pub fn is_default_note_title(title: &str) -> bool {
    matches!(
        title.trim().to_lowercase().as_str(),
        "" | "new note" | "untitled note"
    )
}
pub fn meeting_date(meeting: &MeetingContextDTO) -> Option<chrono::NaiveDate> {
    let start = DateTime::parse_from_rfc3339(&meeting.start_at).ok()?;
    Some(if meeting.all_day {
        start.date_naive()
    } else {
        start.with_timezone(&Local).date_naive()
    })
}
pub fn participant_name(person: &MeetingParticipantDTO) -> String {
    if let Some(name) = person
        .display_name
        .as_ref()
        .filter(|s| !s.trim().is_empty())
    {
        return name.clone();
    }
    let name = [
        person.given_name.as_deref().unwrap_or(""),
        person.family_name.as_deref().unwrap_or(""),
    ]
    .into_iter()
    .filter(|s| !s.trim().is_empty())
    .collect::<Vec<_>>()
    .join(" ");
    if !name.is_empty() {
        return name;
    }
    person
        .email
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Unknown participant".into())
}
pub fn participants(meeting: &MeetingContextDTO) -> Vec<&MeetingParticipantDTO> {
    let mut people: Vec<_> = meeting
        .participants
        .iter()
        .filter(|p| !p.is_resource)
        .collect();
    if let Some(organizer) = meeting.organizer.as_ref().filter(|p| !p.is_resource) {
        if !people.iter().any(|p| {
            p.id == organizer.id
                || p.email
                    .as_ref()
                    .zip(organizer.email.as_ref())
                    .is_some_and(|(a, b)| a.eq_ignore_ascii_case(b))
        }) {
            people.insert(0, organizer);
        }
    }
    people
}
pub fn meeting_time(meeting: &MeetingContextDTO) -> String {
    let Ok(start) = DateTime::parse_from_rfc3339(&meeting.start_at) else {
        return "Time unavailable".into();
    };
    let start = start.with_timezone(&Local);
    if meeting.all_day {
        let date = DateTime::parse_from_rfc3339(&meeting.start_at)
            .map(|d| d.date_naive().format("%a, %b %-d").to_string())
            .unwrap_or_default();
        return format!("{date} · All day");
    }
    let end = DateTime::parse_from_rfc3339(&meeting.end_at)
        .map(|d| d.with_timezone(&Local).format("%-I:%M %p").to_string())
        .unwrap_or_default();
    format!(
        "{} · {} – {end}",
        start.format("%a, %b %-d"),
        start.format("%-I:%M")
    )
}
/// What the recording control says of a meeting that has not been recorded:
/// "Starts 5:00 PM" until it begins and "Started 5:00 PM" while it is on.
/// Nothing once it is over, or for an all-day event that has begun.
pub fn start_label(meeting: &MeetingContextDTO, now: DateTime<Local>) -> Option<String> {
    let start = DateTime::parse_from_rfc3339(&meeting.start_at).ok()?;
    if meeting.all_day {
        return (start.date_naive() > now.date_naive())
            .then(|| format!("Starts {}", start.format("%b %-d")));
    }
    let start = start.with_timezone(&Local);
    let time = start.format("%-I:%M %p");
    if start > now {
        return Some(if start.date_naive() == now.date_naive() {
            format!("Starts {time}")
        } else {
            // Another day: the time alone would read as today's.
            format!("Starts {}, {time}", start.format("%b %-d"))
        });
    }
    let end = DateTime::parse_from_rfc3339(&meeting.end_at).ok()?;
    (now < end).then(|| format!("Started {time}"))
}

/// A person in the details card under a note's date chip.
pub(crate) struct DetailsPerson {
    pub name: String,
    /// What follows the name in the secondary ink: "(me)", "· Organizer".
    pub aside: &'static str,
    pub picture: Option<Arc<RenderImage>>,
    /// Typed in by hand on a note kept here: its row can take it off again.
    pub removable: bool,
}

/// What the details card shows; see `Cereal::render_details_card`.
pub(crate) struct DetailsCard {
    /// The chosen calendar event's title; none reads "Select calendar event".
    pub title: Option<String>,
    pub when: String,
    /// The chosen event's Join and Calendar buttons.
    pub links: Option<Div>,
    pub status: Vec<String>,
    pub people: Vec<DetailsPerson>,
    /// What the list says when it has nobody.
    pub empty: Option<&'static str>,
}

/// When a note without a calendar event is from, as the details card says
/// it: "Mon, Nov 16 · 8:51 PM". The server's creation time when there is
/// one, else the note's own day and time; never the day as it is stored
/// ("2026-11-16").
pub fn note_when(created: Option<DateTime<Local>>, date: &str, time: &str) -> String {
    if let Some(created) = created {
        return created.format("%a, %b %-d · %-I:%M %p").to_string();
    }
    let day = chrono::NaiveDate::parse_from_str(date.trim(), "%Y-%m-%d")
        .map(|day| day.format("%a, %b %-d").to_string())
        .unwrap_or_else(|_| date.trim().to_owned());
    match (day.is_empty(), time.trim().is_empty()) {
        (false, false) => format!("{day} · {}", time.trim()),
        (false, true) => day,
        (true, _) => time.trim().to_owned(),
    }
}

/// The card's time line in 12 pt secondary ink, its AM/PM set as the small
/// capitals every other time in the app has.
fn when_line(when: &str, t: Theme) -> Div {
    let (digits, meridiem) = time_parts(when);
    div()
        .flex()
        .items_baseline()
        .gap(px(3.))
        .whitespace_nowrap()
        .overflow_hidden()
        .text_size(px(12.))
        .line_height(px(16.))
        .text_color(t.muted)
        .child(digits.to_string())
        .when_some(meridiem, |line, meridiem| {
            line.child(
                div()
                    .text_size(px(7.5))
                    .line_height(px(16.))
                    .font_weight(FontWeight::MEDIUM)
                    .child(meridiem.to_uppercase()),
            )
        })
}

/// Whether a person is one the "Add attendees" field is asking for: every
/// word typed is in their name, whatever the case. Nothing typed asks for
/// everyone.
pub fn matches_person(name: &str, query: &str) -> bool {
    let name = name.to_lowercase();
    query
        .split_whitespace()
        .all(|word| name.contains(&word.to_lowercase()))
}

/// The known people the field offers to add: those it asks for who are not
/// on the note already, each once, in the order they are known, at most
/// `limit`. Nothing typed offers nobody.
pub fn attendee_suggestions(
    query: &str,
    known: &[String],
    present: &[String],
    limit: usize,
) -> Vec<String> {
    if query.trim().is_empty() {
        return Vec::new();
    }
    let mut offered: Vec<String> = Vec::new();
    for name in known {
        let name = name.trim();
        let listed = |names: &[String]| names.iter().any(|other| other.trim().eq_ignore_ascii_case(name));
        if name.is_empty() || !matches_person(name, query) || listed(present) || listed(&offered) {
            continue;
        }
        offered.push(name.to_owned());
        if offered.len() == limit {
            break;
        }
    }
    offered
}

fn safe_url(value: Option<&String>) -> Option<String> {
    value
        .filter(|s| crate::calendar::safe_meeting_url(s))
        .cloned()
}

impl Cereal {
    pub fn refresh_meeting_context(&mut self, id: u64, cx: &mut Context<Self>) {
        if self.meeting.loading && self.meeting.note_id == Some(id) {
            return;
        }
        self.run_meeting_command(id, MeetingCommand::Load, cx);
    }
    fn run_meeting_command(&mut self, id: u64, command: MeetingCommand, cx: &mut Context<Self>) {
        let Some(note) = self
            .workspace
            .note(id)
            .filter(|n| n.server_id.is_some())
            .cloned()
        else {
            return;
        };
        if let Some(c) = self.meeting.cancel.take() {
            c.store(true, Ordering::Release);
        }
        let cancel = Arc::new(AtomicBool::new(false));
        self.meeting.cancel = Some(cancel.clone());
        self.meeting.epoch = self.meeting.epoch.wrapping_add(1);
        let epoch = self.meeting.epoch;
        let account = crate::accounts::note_epoch(id);
        let submitted_title = note.title.clone();
        let manual = matches!(&command, MeetingCommand::UpsertParticipant(_));
        let selected = matches!(&command, MeetingCommand::SetEvent(_));
        if self.meeting.note_id != Some(id) {
            self.meeting.pending_attendee = None;
            self.attendee_input
                .update(cx, |input, cx| input.set_content("", cx));
            self.meeting
                .participants_scroll
                .set_offset(Point::default());
        }
        self.meeting.note_id = Some(id);
        self.meeting.loading = true;
        self.meeting.error = None;
        let (send, receive) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = send.send(crate::server::meeting_context(&note, command, cancel));
        });
        cx.spawn(async move |this, cx| {
            let result = loop {
                match receive.try_recv() {
                    Ok(r) => break r,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        break Err("Meeting context worker stopped".into());
                    }
                    Err(mpsc::TryRecvError::Empty) => {
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(40))
                            .await
                    }
                }
            };
            let _ = this.update(cx, |this, cx| {
                if epoch != this.meeting.epoch || account != crate::accounts::note_epoch(id) {
                    return;
                }
                this.meeting.loading = false;
                match result {
                    Ok(reply) => {
                        let context = reply.context;
                        let mut canonical_title = None;
                        if let Some(note) = this.workspace.note_mut(id) {
                            if note.server_id.as_deref() != Some(context.note_id.as_str()) {
                                this.meeting.error =
                                    Some("The server returned context for another note".into());
                                cx.notify();
                                return;
                            }
                            note.meeting_context = context.meeting;
                            if note.title == submitted_title
                                && is_default_note_title(&submitted_title)
                            {
                                if let Some(remote) = reply.note {
                                    note.title = remote.title.clone();
                                    canonical_title = Some(remote.title);
                                }
                            }
                        }
                        if let Some(title) = canonical_title {
                            this.title_input
                                .update(cx, |input, cx| input.set_content(title, cx));
                        }
                        if manual {
                            this.attendee_input
                                .update(cx, |i, cx| i.set_content("", cx));
                        }
                        if selected {
                            // Not a button: the event picker's answer arriving. It opens directly.
                            this.menu = Some(Menu::NoteDetails);
                        }
                        this.persist(cx);
                        // A name typed before the note had an event is added to it now,
                        // if it is still what the field says.
                        let waiting = this.meeting.pending_attendee.take().filter(|name| {
                            selected && this.attendee_input.read(cx).content().trim() == name.as_str()
                        });
                        if let Some(name) = waiting {
                            cx.notify();
                            this.add_meeting_attendee(name, cx);
                            return;
                        }
                    }
                    Err(error) => this.meeting.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    pub fn open_connected_event_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.current_note.is_none() {
            return;
        }
        self.show_connected_event_picker(cx);
        self.search.read(cx).focus_handle.focus(window);
    }
    /// The picker and its events, with the keyboard left where it is.
    fn show_connected_event_picker(&mut self, cx: &mut Context<Self>) {
        // The picker lists the events of the note's own account.
        let Some(note_id) = self.current_note else {
            return;
        };
        self.search.update(cx, |i, cx| {
            i.set_content("", cx);
            i.set_placeholder("Search event", cx)
        });
        self.menu = Some(Menu::CalendarEvent);
        if let Some(c) = self.meeting.picker_cancel.take() {
            c.store(true, Ordering::Release);
        }
        let cancel = Arc::new(AtomicBool::new(false));
        self.meeting.picker_cancel = Some(cancel.clone());
        self.meeting.picker_epoch = self.meeting.picker_epoch.wrapping_add(1);
        let epoch = self.meeting.picker_epoch;
        let account = crate::accounts::note_epoch(note_id);
        self.meeting.events_loading = true;
        self.meeting.events_error = None;
        let (send, receive) = mpsc::channel();
        std::thread::spawn(move || {
            // Only the note's own account's events: an event id of one
            // account is never sent to another.
            let _ = send.send(crate::server::meeting_picker_events(note_id, cancel));
        });
        cx.spawn(async move |this, cx| {
            let result = loop {
                match receive.try_recv() {
                    Ok(r) => break r,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        break Err("Calendar search worker stopped".into());
                    }
                    Err(mpsc::TryRecvError::Empty) => {
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(40))
                            .await
                    }
                }
            };
            let _ = this.update(cx, |this, cx| {
                if epoch != this.meeting.picker_epoch
                    || account != crate::accounts::note_epoch(note_id)
                {
                    return;
                }
                this.meeting.events_loading = false;
                match result {
                    Ok(events) => this.meeting.events = events,
                    Err(error) => this.meeting.events_error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    pub fn select_connected_event_from_search(&mut self, cx: &mut Context<Self>) {
        let query = self.search.read(cx).content().to_lowercase();
        if let Some(event) = self.meeting.events.iter().find(|e| {
            format!("{} {}", e.title, e.calendar_name)
                .to_lowercase()
                .contains(&query)
        }) {
            self.choose_meeting_event(Some(event.id.clone()), cx);
        }
    }
    fn choose_meeting_event(&mut self, event_id: Option<String>, cx: &mut Context<Self>) {
        if let Some(id) = self.current_note {
            self.run_meeting_command(id, MeetingCommand::SetEvent(event_id), cx);
        }
    }
    pub fn add_meeting_attendee(&mut self, label: String, cx: &mut Context<Self>) {
        let label = label.trim();
        if label.is_empty() || self.meeting.loading {
            return;
        }
        let Some(id) = self.current_note else { return };
        let Some(note) = self.workspace.note(id) else {
            return;
        };
        if note.meeting_context.is_none() {
            // Attendees are kept with the note's calendar event. Without one
            // the name waits in the field while the event is chosen, and is
            // added as soon as it is.
            self.meeting.error = Some("Choose the calendar event to add attendees to".into());
            self.meeting.pending_attendee = Some(label.to_owned());
            self.show_connected_event_picker(cx);
            return;
        }
        if label.chars().count() > 200 {
            self.meeting.error = Some("Use an attendee name of at most 200 characters".into());
            cx.notify();
            return;
        }
        let email =
            (label.contains('@') && !label.contains(char::is_whitespace)).then(|| label.to_owned());
        let key = format!(
            "manual-note-participant:{}:{label}",
            note.server_id.as_deref().unwrap_or("")
        );
        let participant = ManualParticipantInput {
            id: uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, key.as_bytes()).to_string(),
            display_name: label.to_owned(),
            email,
            given_name: None,
            family_name: None,
        };
        self.run_meeting_command(id, MeetingCommand::UpsertParticipant(participant), cx);
    }
    pub fn render_connected_note_details(&self, cx: &mut Context<Self>) -> Div {
        let t = self.theme();
        let note = self.current_note.and_then(|id| self.workspace.note(id));
        let context = note.and_then(|n| n.meeting_context.as_ref());
        let when = match context {
            Some(meeting) => meeting_time(meeting),
            None => note
                .map(|note| {
                    let created = note
                        .server_created_at
                        .as_deref()
                        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
                        .map(|time| time.with_timezone(&Local));
                    note_when(created, &note.date, &note.time)
                })
                .unwrap_or_default(),
        };
        let links = context.map(|context| {
            let mut links = div().px(px(11.)).pb(px(12.)).flex().gap(px(8.));
            for (key, label, url) in [
                (
                    "meeting-join",
                    "Join",
                    safe_url(context.meeting_url.as_ref()),
                ),
                (
                    "meeting-calendar",
                    "Calendar",
                    safe_url(context.event_url.as_ref()),
                ),
            ] {
                let symbol = if label == "Join" { "video" } else { "calendar" };
                let link = pill(key, label, symbol, t).flex_1().h(px(32.));
                links = links.child(match url {
                    Some(url) => link.on_click(cx.listener(move |_, _, _, cx| cx.open_url(&url))),
                    None => link.opacity(0.4),
                });
            }
            links
        });
        let mut status = Vec::new();
        if self.meeting.loading {
            status.push("Updating meeting…".to_string());
        }
        status.extend(self.meeting.error.clone());
        let session = self
            .current_note
            .and_then(crate::accounts::session_of_note);
        let picture = session
            .as_ref()
            .and_then(|session| crate::portrait::of(session, cx));
        let mut people: Vec<DetailsPerson> = context
            .map(participants)
            .unwrap_or_default()
            .into_iter()
            .map(|person| DetailsPerson {
                name: participant_name(person),
                aside: if person.is_self {
                    "(me)"
                } else if person.is_organizer {
                    "· Organizer"
                } else {
                    ""
                },
                picture: picture.clone().filter(|_| person.is_self),
                removable: false,
            })
            .collect();
        // Without an event the note still has the person who wrote it.
        if context.is_none() {
            people.push(DetailsPerson {
                name: session
                    .map(|session| session.user.name)
                    .filter(|name| !name.trim().is_empty())
                    .unwrap_or_else(|| "Me".into()),
                aside: "(me)",
                picture,
                removable: false,
            });
        }
        self.render_details_card(
            DetailsCard {
                title: context.map(|meeting| meeting.title.clone()),
                when,
                links,
                status,
                people,
                empty: context.map(|_| "No attendee details available"),
            },
            cx,
        )
    }

    /// The organization the people of a note belong to, as the details card
    /// heads its list: the team space's name once it is known, else nothing.
    fn details_group(&self) -> String {
        let name = match &self.team.page {
            Some(page) => page.organization.name.trim(),
            None => self.workspace.settings.team_space_name.trim(),
        };
        if name.is_empty() {
            "Attendees".into()
        } else {
            name.to_owned()
        }
    }

    /// Everyone this app has a name for besides the note's own attendees:
    /// the members of the team space and the people its meetings were with.
    fn known_people(&self) -> Vec<String> {
        let Some(page) = &self.team.page else {
            return Vec::new();
        };
        page.members
            .iter()
            .map(|member| member.name.clone())
            .chain(page.people.iter().map(|person| person.name.clone()))
            .collect()
    }

    /// The event picker, from the details card's first row.
    fn open_event_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if crate::server::enabled() {
            return self.open_connected_event_picker(window, cx);
        }
        self.search.update(cx, |input, cx| {
            input.set_content("", cx);
            input.set_placeholder("Search event...", cx);
        });
        self.open_menu(Menu::CalendarEvent);
        self.search.read(cx).focus_handle.focus(window);
        cx.notify();
    }

    /// The card under a note's date chip: the calendar event (or the row
    /// that chooses one) over the time, the field that adds an attendee,
    /// and who was there. One card for a note kept here and one kept on a
    /// server; only where its rows come from differs.
    pub(crate) fn render_details_card(&self, card: DetailsCard, cx: &mut Context<Self>) -> Div {
        let t = self.theme();
        let query = self.attendee_input.read(cx).content().to_string();
        let typing = !query.trim().is_empty();
        let mut body = div()
            .w(px(280.))
            .rounded(px(POPOVER_RADIUS))
            .bg(t.card)
            .border_1()
            .border_color(t.border)
            .shadow_lg()
            .overflow_hidden()
            .flex()
            .flex_col();
        body = body.child(
            div()
                .id("note-select-calendar-event")
                .h(px(58.))
                .flex_shrink_0()
                .pl(px(12.))
                .pr(px(14.))
                .pt(px(10.))
                .flex()
                .flex_col()
                .gap(px(2.))
                .cursor_default()
                .child(crate::motion::tactile(t.bg).radius(0.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .child(
                            div().flex_1().min_w_0().truncate().child(
                                card.title
                                    .unwrap_or_else(|| "Select calendar event".into()),
                            ),
                        )
                        .child(icon("chevron-down", 12., t.muted)),
                )
                .child(when_line(&card.when, t))
                .reach()
                .on_click(cx.listener(|this, _, window, cx| this.open_event_picker(window, cx))),
        );
        if let Some(links) = card.links {
            body = body.child(links);
        }
        body = body.child(separator(t));
        for status in card.status {
            body = body.child(div().px(px(12.)).py(px(7.)).child(small(status, t)));
        }
        body = body
            .child(
                div()
                    .h(px(40.))
                    .flex_shrink_0()
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .text_size(px(14.))
                    // The placeholder is a step quieter than a typed name.
                    .when(!typing, |row| row.opacity(0.8))
                    .child(self.attendee_input.clone()),
            )
            .child(separator(t));
        let present: Vec<String> = card.people.iter().map(|person| person.name.clone()).collect();
        let suggestions = attendee_suggestions(&query, &self.known_people(), &present, 5);
        let list = div()
            .id("meeting-participants")
            .track_scroll(&self.meeting.participants_scroll)
            .max_h(px(260.))
            .overflow_y_scroll()
            .pt(px(8.))
            .pb(px(5.))
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(
                div()
                    .px(px(8.))
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(t.muted)
                    .child(self.details_group()),
            );
        let avatar = |name: &str, picture: Option<Arc<RenderImage>>| {
            let initial = name
                .chars()
                .next()
                .map(|c| c.to_uppercase().collect::<String>())
                .unwrap_or_else(|| "?".into());
            crate::portrait::avatar(
                picture,
                20.,
                div()
                    .size(px(20.))
                    .flex_shrink_0()
                    .rounded_full()
                    .bg(t.fill)
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(10.))
                    .text_color(t.muted)
                    .child(initial),
            )
        };
        let mut rows = div().flex().flex_col().gap(px(3.));
        let mut shown = 0;
        for (index, person) in card.people.into_iter().enumerate() {
            if !matches_person(&person.name, &query) {
                continue;
            }
            shown += 1;
            // "Name (me)": the aside in the secondary ink, on the name's line.
            let label = if person.aside.is_empty() {
                person.name.clone()
            } else {
                format!("{} {}", person.name, person.aside)
            };
            let aside = person.name.len()..label.len();
            let label = StyledText::new(label).with_highlights([(
                aside,
                HighlightStyle {
                    color: Some(t.muted),
                    ..Default::default()
                },
            )]);
            let remove = person.name.clone();
            rows = rows.child(
                div()
                    .id(("note-attendee", index))
                    .h(px(30.))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .rounded(px(5.))
                    .when(person.removable, |row| {
                        row.child(crate::motion::tactile(t.hover).radius(5.))
                    })
                    .child(avatar(&person.name, person.picture))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(13.))
                            .child(label),
                    )
                    .when(person.removable, |row| {
                        row.child(
                            tool_tip(("remove-note-attendee", index), "close", "Remove attendee", t)
                                .size(px(22.))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(note) =
                                        this.current_note.and_then(|id| this.workspace.note_mut(id))
                                    {
                                        note.participants.retain(|name| name != &remove);
                                    }
                                    this.persist(cx);
                                })),
                        )
                    }),
            );
        }
        // Someone known who is not on the note yet: the row adds them.
        for (index, name) in suggestions.into_iter().enumerate() {
            shown += 1;
            let add = name.clone();
            rows = rows.child(
                div()
                    .id(("suggested-attendee", index))
                    .h(px(30.))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .rounded(px(5.))
                    .cursor_default()
                    .child(crate::motion::tactile(t.hover).radius(5.))
                    .child(avatar(&name, None))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(13.))
                            .child(name),
                    )
                    .child(icon("plus", 12., t.muted))
                    .reach()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let content = add.clone();
                        this.attendee_input.update(cx, |input, cx| {
                            input.set_content(content.clone(), cx);
                            cx.emit(crate::input::InputSubmitted { content });
                        });
                    })),
            );
        }
        if shown == 0 {
            let line: SharedString = if typing {
                format!("Press Return to add “{}”", query.trim()).into()
            } else {
                card.empty.unwrap_or("No attendees yet").into()
            };
            rows = rows.child(div().h(px(30.)).px(px(8.)).flex().items_center().child(small(line, t)));
        }
        body.child(list.child(rows))
    }
    pub fn render_connected_event_picker(&self, cx: &mut Context<Self>) -> Div {
        let t = self.theme();
        let query = self.search.read(cx).content().to_lowercase();
        let associated = self
            .current_note
            .and_then(|id| self.workspace.note(id))
            .is_some_and(|note| note.meeting_context.is_some());
        let mut list = div()
            .id("connected-event-results")
            .track_scroll(&self.meeting.events_scroll)
            .max_h(px(260.))
            .overflow_y_scroll()
            .flex()
            .flex_col();
        if self.meeting.events_loading {
            list = list.child(div().p(px(12.)).child(small("Loading calendar events…", t)));
        }
        if let Some(error) = &self.meeting.events_error {
            list = list.child(div().p(px(12.)).child(small(error.clone(), t)));
        }
        let matching: Vec<_> = self
            .meeting
            .events
            .iter()
            .filter(|e| {
                format!("{} {}", e.title, e.calendar_name)
                    .to_lowercase()
                    .contains(&query)
            })
            .collect();
        if !self.meeting.events_loading
            && self.meeting.events_error.is_none()
            && matching.is_empty()
        {
            list = list.child(
                div()
                    .p(px(12.))
                    .child(small("No events found in this date range", t)),
            );
        }
        for event in matching.into_iter().take(200) {
            let id = event.id.clone();
            let date = if event.all_day {
                chrono::NaiveDate::parse_from_str(&event.start_date, "%Y-%m-%d")
                    .map(|d| format!("{} · All day", d.format("%a, %b %-d")))
                    .unwrap_or_default()
            } else {
                DateTime::parse_from_rfc3339(&event.start_at)
                    .map(|d| {
                        d.with_timezone(&Local)
                            .format("%a, %b %-d · %-I:%M %p")
                            .to_string()
                    })
                    .unwrap_or_default()
            };
            list = list.child(
                div()
                    .id(SharedString::from(format!("select-meeting-{id}")))
                    .px(px(12.))
                    .py(px(9.))
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .cursor_default()
                    .child(crate::motion::tactile(t.hover).radius(0.))
                    .child(div().text_size(px(13.)).child(event.title.clone()))
                    .child(small(format!("{date} · {}", event.calendar_name), t))
                    .reach().on_click(cx.listener(move |this, _, _, cx| {
                        this.choose_meeting_event(Some(id.clone()), cx)
                    })),
            );
        }
        div()
            .w(px(266.))
            .rounded(px(crate::ui::POPOVER_RADIUS))
            .bg(t.card)
            .border_1()
            .border_color(t.border)
            .shadow_lg()
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(40.))
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .child(self.search.clone()),
            )
            .child(separator(t))
            .when(associated, |d| {
                d.child(
                    div()
                        .id("remove-meeting-association")
                        .px(px(12.))
                        .py(px(9.))
                        .text_size(px(13.))
                        .cursor_default()
                        .child(crate::motion::tactile(t.hover).radius(0.))
                        .child("Remove calendar event")
                        .reach().on_click(
                            cx.listener(|this, _, _, cx| this.choose_meeting_event(None, cx)),
                        ),
                )
                .child(separator(t))
            })
            .child(list)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        attendee_suggestions, matches_person, meeting_time, note_when, participant_name,
        participants, start_label,
    };
    use chrono::{Duration, Local, TimeZone};
    use meeting_notes_api_types::{MeetingContextDTO, MeetingParticipantDTO};
    use serde_json::json;
    fn person(email: Option<&str>, name: Option<&str>, resource: bool) -> MeetingParticipantDTO {
        serde_json::from_value(json!({"id":"00000000-0000-4000-8000-000000000001","email":email,"displayName":name,"givenName":null,"familyName":null,"nameSource":"unknown","isSelf":false,"isOrganizer":false,"isResource":resource,"responseStatus":"unknown"})).unwrap()
    }
    fn meeting(people: Vec<MeetingParticipantDTO>) -> MeetingContextDTO {
        serde_json::from_value(json!({"eventId":"00000000-0000-4000-8000-000000000002","provider":"ics","title":"Fictional meeting","calendarName":"Work","startAt":"2026-11-26T00:00:00Z","endAt":"2026-11-27T00:00:00Z","allDay":true,"timeZone":null,"location":null,"meetingUrl":null,"eventUrl":null,"organizer":null,"participants":people})).unwrap()
    }
    #[test]
    fn a_note_without_an_event_says_its_day_in_words_never_as_it_is_stored() {
        let created = Local.with_ymd_and_hms(2026, 11, 16, 20, 51, 0).earliest().unwrap();
        assert_eq!(note_when(Some(created), "2026-11-16", "8:50 PM"), "Mon, Nov 16 · 8:51 PM");
        // Not on a server yet: the note's own day and time.
        assert_eq!(note_when(None, "2026-11-16", "8:50 PM"), "Mon, Nov 16 · 8:50 PM");
        assert_eq!(note_when(None, "2026-11-16", ""), "Mon, Nov 16");
        assert_eq!(note_when(None, "", "8:50 PM"), "8:50 PM");
        // A day that is not a date is said as it is rather than dropped.
        assert_eq!(note_when(None, "Today", "8:50 PM"), "Today · 8:50 PM");
        assert_eq!(crate::ui::time_parts("Mon, Nov 16 · 8:51 PM"), ("Mon, Nov 16 · 8:51", Some("PM")));
        assert_eq!(crate::ui::time_parts("Thu, Nov 26 · All day"), ("Thu, Nov 26 · All day", None));
    }
    #[test]
    fn the_attendee_field_filters_who_is_listed_and_offers_known_people_once() {
        assert!(matches_person("Maya Example", ""));
        assert!(matches_person("Maya Example", "  "));
        assert!(matches_person("Maya Example", "may"));
        assert!(matches_person("Maya Example", "exam MAYA"));
        assert!(!matches_person("Maya Example", "maya z"));
        let known: Vec<String> = ["Maya Example", "Zoë Test", "maya example", "Max Sample", " "]
            .into_iter()
            .map(String::from)
            .collect();
        // Nothing typed offers nobody.
        assert!(attendee_suggestions("", &known, &[], 5).is_empty());
        assert_eq!(attendee_suggestions("ma", &known, &[], 5), ["Maya Example", "Max Sample"]);
        assert_eq!(attendee_suggestions("ma", &known, &[], 1), ["Maya Example"]);
        // Someone on the note already is not offered again.
        assert_eq!(attendee_suggestions("ma", &known, &["MAYA EXAMPLE".into()], 5), ["Max Sample"]);
        assert_eq!(attendee_suggestions("zoë", &known, &[], 5), ["Zoë Test"]);
        assert!(attendee_suggestions("nobody", &known, &[], 5).is_empty());
    }
    #[test]
    fn names_are_explicit_data_or_email_fallback_never_email_name_guesses() {
        assert_eq!(
            participant_name(&person(Some("alex.smith@example.test"), None, false)),
            "alex.smith@example.test"
        );
        assert_eq!(
            participant_name(&person(
                Some("alex.smith@example.test"),
                Some("Zoë 🦊"),
                false
            )),
            "Zoë 🦊"
        );
        assert_eq!(
            participant_name(&person(None, None, false)),
            "Unknown participant"
        );
    }
    #[test]
    fn attendee_count_excludes_resources_and_deduplicates_explicit_organizer_email() {
        let mut context = meeting(vec![
            person(Some("a@example.test"), None, false),
            person(Some("room@example.test"), None, true),
        ]);
        let mut organizer = person(Some("A@example.test"), None, false);
        organizer.id = "different-id".into();
        organizer.is_organizer = true;
        context.organizer = Some(organizer);
        assert_eq!(participants(&context).len(), 1);
    }
    #[test]
    fn all_day_metadata_keeps_the_calendar_date_and_invalid_time_does_not_invent_a_schedule() {
        let mut context = meeting(vec![]);
        assert_eq!(meeting_time(&context), "Thu, Nov 26 · All day");
        context.start_at = "not-a-time".into();
        assert!(start_label(&context, Local::now()).is_none());
    }
    #[test]
    fn the_recording_control_says_when_a_meeting_starts_then_that_it_started() {
        let start = Local.with_ymd_and_hms(2026, 11, 16, 17, 0, 0).earliest().unwrap();
        let mut context = meeting(vec![]);
        context.all_day = false;
        context.start_at = start.to_rfc3339();
        context.end_at = (start + Duration::minutes(30)).to_rfc3339();
        let label = |now| start_label(&context, now);
        assert_eq!(label(start - Duration::hours(5)).as_deref(), Some("Starts 5:00 PM"));
        assert_eq!(label(start - Duration::seconds(1)).as_deref(), Some("Starts 5:00 PM"));
        // From the minute it begins until it ends.
        assert_eq!(label(start).as_deref(), Some("Started 5:00 PM"));
        assert_eq!(
            label(start + Duration::minutes(29)).as_deref(),
            Some("Started 5:00 PM")
        );
        // Over: the control is an ordinary one again.
        assert_eq!(label(start + Duration::minutes(30)), None);
        assert_eq!(label(start + Duration::days(2)), None);
        // On an earlier day the date is part of it.
        assert_eq!(
            label(start - Duration::days(1)).as_deref(),
            Some("Starts Nov 16, 5:00 PM")
        );
        // An all-day event is named by its date, until that date.
        let all_day = meeting(vec![]);
        let before = Local.with_ymd_and_hms(2026, 11, 20, 9, 0, 0).earliest().unwrap();
        assert_eq!(start_label(&all_day, before).as_deref(), Some("Starts Nov 26"));
        let on_the_day = Local.with_ymd_and_hms(2026, 11, 26, 9, 0, 0).earliest().unwrap();
        assert_eq!(start_label(&all_day, on_the_day), None);
    }
}
