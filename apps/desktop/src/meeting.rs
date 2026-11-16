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
        let account = crate::account::epoch();
        let submitted_title = note.title.clone();
        let manual = matches!(&command, MeetingCommand::UpsertParticipant(_));
        let selected = matches!(&command, MeetingCommand::SetEvent(_));
        if self.meeting.note_id != Some(id) {
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
                if epoch != this.meeting.epoch || account != crate::account::epoch() {
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
                            this.menu = Some(Menu::NoteDetails);
                        }
                        this.persist(cx);
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
        self.search.update(cx, |i, cx| {
            i.set_content("", cx);
            i.set_placeholder("Search event", cx)
        });
        self.menu = Some(Menu::CalendarEvent);
        self.search.read(cx).focus_handle.focus(window);
        if let Some(c) = self.meeting.picker_cancel.take() {
            c.store(true, Ordering::Release);
        }
        let cancel = Arc::new(AtomicBool::new(false));
        self.meeting.picker_cancel = Some(cancel.clone());
        self.meeting.picker_epoch = self.meeting.picker_epoch.wrapping_add(1);
        let epoch = self.meeting.picker_epoch;
        let account = crate::account::epoch();
        self.meeting.events_loading = true;
        self.meeting.events_error = None;
        let (send, receive) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = send.send(crate::server::meeting_picker_events(cancel));
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
                if epoch != this.meeting.picker_epoch || account != crate::account::epoch() {
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
            self.meeting.error = Some("Choose a calendar event before adding attendees".into());
            cx.notify();
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
        let context = self
            .current_note
            .and_then(|id| self.workspace.note(id))
            .and_then(|n| n.meeting_context.as_ref());
        let mut body = div()
            .w(px(280.))
            .rounded(px(12.))
            .bg(t.card)
            .border_1()
            .border_color(t.border)
            .shadow_lg()
            .overflow_hidden()
            .flex()
            .flex_col();
        body = body.child(
            div()
                .id("connected-select-calendar-event")
                .px(px(12.))
                .py(px(10.))
                .flex()
                .flex_col()
                .gap(px(3.))
                .cursor_pointer()
                .child(crate::motion::tactile(t.hover).radius(0.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(div().size(px(9.)).rounded(px(3.)).bg(rgb(0x87e5e8)))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(13.))
                                .child(
                                    context
                                        .map(|m| m.title.clone())
                                        .unwrap_or_else(|| "Select calendar event".into()),
                                ),
                        )
                        .child(icon("chevron-down", 12., t.muted)),
                )
                .when_some(context, |d, m| {
                    d.child(
                        div()
                            .pl(px(17.))
                            .text_size(px(11.))
                            .text_color(t.muted)
                            .child(meeting_time(m)),
                    )
                })
                .when(context.is_none(), |d| {
                    let timestamp = self
                        .current_note
                        .and_then(|id| self.workspace.note(id))
                        .map(|note| {
                            note.server_created_at
                                .as_deref()
                                .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
                                .map(|time| {
                                    time.with_timezone(&Local)
                                        .format("%a, %b %-d · %-I:%M %p")
                                        .to_string()
                                })
                                .unwrap_or_else(|| format!("{} · {}", note.date, note.time))
                        })
                        .unwrap_or_default();
                    d.child(
                        div()
                            .text_size(px(11.))
                            .text_color(t.muted)
                            .child(timestamp),
                    )
                })
                .reach()
                .on_click(
                    cx.listener(|this, _, window, cx| this.open_connected_event_picker(window, cx)),
                ),
        );
        if let Some(context) = context {
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
                if let Some(url) = url {
                    links = links.child(
                        pill(
                            key,
                            label,
                            if label == "Join" { "video" } else { "calendar" },
                            t,
                        )
                        .flex_1()
                        .h(px(32.))
                        .on_click(cx.listener(move |_, _, _, cx| cx.open_url(&url))),
                    );
                } else {
                    links = links.child(
                        pill(
                            key,
                            label,
                            if label == "Join" { "video" } else { "calendar" },
                            t,
                        )
                        .flex_1()
                        .h(px(32.))
                        .opacity(0.4),
                    );
                }
            }
            body = body.child(links).child(separator(t));
        }
        if self.meeting.loading {
            body = body.child(
                div()
                    .px(px(12.))
                    .py(px(7.))
                    .child(small("Updating meeting…", t)),
            );
        }
        if let Some(error) = &self.meeting.error {
            body = body.child(div().px(px(12.)).py(px(8.)).child(small(error.clone(), t)));
        }
        body = body
            .child(
                div()
                    .h(px(40.))
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .text_size(px(13.))
                    .when(context.is_some(), |d| d.child(self.attendee_input.clone()))
                    .when(context.is_none(), |d| {
                        d.child(small("Choose an event to add attendees", t))
                    }),
            )
            .child(separator(t));
        let mut list = div()
            .id("meeting-participants")
            .track_scroll(&self.meeting.participants_scroll)
            .max_h(px(260.))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .py(px(5.));
        let people = context.map(participants).unwrap_or_default();
        list = list.child(
            div().px(px(8.)).py(px(4.)).child(small(
                context
                    .map(|m| m.calendar_name.clone())
                    .unwrap_or_else(|| "Attendees".into()),
                t,
            )),
        );
        if people.is_empty() && context.is_none() {
            let name = crate::account::session()
                .map(|session| session.user.name)
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| "Me".into());
            let initial = name
                .chars()
                .next()
                .map(|character| character.to_uppercase().collect::<String>())
                .unwrap_or_else(|| "M".into());
            list = list.child(
                div()
                    .px(px(8.))
                    .py(px(5.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .size(px(20.))
                            .rounded_full()
                            .bg(t.fill)
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(10.))
                            .child(initial),
                    )
                    .child(div().text_size(px(13.)).child(format!("{name} (me)"))),
            );
        } else if people.is_empty() {
            list = list.child(div().px(px(12.)).py(px(8.)).child(small(
                if context.is_some() {
                    "No attendee details available"
                } else {
                    "No calendar event selected"
                },
                t,
            )));
        }
        for person in people {
            let name = participant_name(person);
            let initial = name
                .chars()
                .next()
                .map(|c| c.to_uppercase().collect::<String>())
                .unwrap_or_else(|| "?".into());
            let subtitle = if person.is_self {
                " (me)"
            } else if person.is_organizer {
                " · Organizer"
            } else {
                ""
            };
            list = list.child(
                div()
                    .px(px(8.))
                    .py(px(5.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .size(px(21.))
                            .rounded_full()
                            .bg(t.fill)
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(11.))
                            .child(initial),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(12.))
                            .child(format!("{name}{subtitle}")),
                    ),
            );
        }
        body.child(list)
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
                    .cursor_pointer()
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
                        .cursor_pointer()
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
    use super::{meeting_time, participant_name, participants, start_label};
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
