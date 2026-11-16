//! Connected calendars are server-owned; the local preview's sample agenda is
//! never substituted for a missing, empty or failed linked-calendar response.
use crate::{app::Cereal, ui::*};
use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, TimeZone};
use gpui::{prelude::*, *};
use meeting_notes_api_types::{CalendarDTO, CalendarEventDTO, GoogleCalendarDTO};
use std::collections::HashMap;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

pub struct CalendarSnapshot {
    pub look_ahead_days: u32,
    pub personal_linked: bool,
    pub personal_issue: bool,
    pub sources: Vec<CalendarDTO>,
    pub events: Vec<CalendarEventDTO>,
    /// How many other people each event has, where the server says.
    pub attendees: HashMap<String, u32>,
    /// The signed-in person's own calendars, selected or not.
    pub personal: Vec<GoogleCalendarDTO>,
}

/// Which events "Coming up" and the menu bar leave out: the same answer for
/// both, from the settings under Settings › Calendar.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EventFilter {
    /// Calendars hidden on this Mac, as `"<AccountId>/<calendarId>"`.
    pub hidden_calendars: Vec<String>,
    /// The account these events were read through: a hidden calendar hides
    /// only that account's.
    pub account: Option<crate::accounts::AccountId>,
    /// "Show events with no participants" is off.
    pub meetings_only: bool,
    pub attendees: HashMap<String, u32>,
}

/// How a calendar hidden on this Mac is recorded: with its account, since
/// two accounts on one server return the same id for a shared calendar.
pub fn hidden_key(account: Option<&crate::accounts::AccountId>, calendar_id: &str) -> String {
    match account {
        Some(account) => format!("{}/{calendar_id}", account.as_str()),
        None => calendar_id.to_owned(),
    }
}

/// Entries from before accounts are bare calendar ids: they get the
/// migrated account's id. Returns whether anything changed.
pub fn migrate_hidden_calendars(hidden: &mut [String], account: &crate::accounts::AccountId) -> bool {
    let mut changed = false;
    for entry in hidden.iter_mut().filter(|entry| !entry.contains('/')) {
        *entry = hidden_key(Some(account), entry);
        changed = true;
    }
    changed
}

/// Whether an event is a meeting with someone: it has a call link, or other
/// people are invited. `attendees` is the number of other people, where the
/// server gives one. Where it does not, a timed event is taken to be a
/// meeting and an all-day one (a holiday, a birthday) is not.
pub fn is_meeting(event: &CalendarEventDTO, attendees: Option<u32>) -> bool {
    if event
        .meeting_url
        .as_deref()
        .is_some_and(safe_meeting_url)
    {
        return true;
    }
    match attendees {
        Some(count) => count > 0,
        None => !event.all_day,
    }
}

impl EventFilter {
    pub fn shows(&self, sources: &[CalendarDTO], event: &CalendarEventDTO) -> bool {
        sources
            .iter()
            .find(|source| source.id == event.calendar_id)
            .is_none_or(|source| source.enabled)
            && !self
                .hidden_calendars
                .contains(&hidden_key(self.account.as_ref(), &event.calendar_id))
            && (!self.meetings_only
                || is_meeting(event, self.attendees.get(&event.id).copied()))
    }
}

#[derive(Default)]
pub struct CalendarState {
    /// The account these calendars and events were read through. Calendars
    /// belong to the account they were read through.
    pub account: Option<crate::accounts::AccountId>,
    pub sources: Vec<CalendarDTO>,
    pub events: Vec<CalendarEventDTO>,
    /// Today's events onward, whatever page Home is on: what the menu bar
    /// shows.
    pub agenda: Vec<CalendarEventDTO>,
    pub attendees: HashMap<String, u32>,
    pub personal: Vec<GoogleCalendarDTO>,
    /// Counts the changes to anything above.
    pub revision: u64,
    /// A calendar whose visibility is being changed on the server.
    pub choosing: Option<String>,
    agenda_epoch: u64,
    pub loading: bool,
    pub loaded: bool,
    pub look_ahead_days: u32,
    pub personal_linked: bool,
    pub personal_issue: bool,
    pub error: Option<String>,
    pub opening: Option<String>,
    pub scroll: ScrollHandle,
    epoch: u64,
    cancel: Option<Arc<AtomicBool>>,
    open_cancel: Option<Arc<AtomicBool>>,
}

impl Drop for CalendarState {
    fn drop(&mut self) {
        if let Some(cancel) = &self.cancel {
            cancel.store(true, Ordering::Release);
        }
        if let Some(cancel) = &self.open_cancel {
            cancel.store(true, Ordering::Release);
        }
    }
}

fn page_day(page: i32) -> NaiveDate {
    crate::menubar::now()
        .date_naive()
        .checked_add_signed(Duration::days(i64::from(page) * 7))
        .unwrap_or_else(|| crate::menubar::now().date_naive())
}

impl CalendarState {
    pub fn filter(&self) -> EventFilter {
        let preferences = crate::desktop_preferences::meeting();
        EventFilter {
            hidden_calendars: preferences.hidden_calendars,
            account: self.account.clone(),
            meetings_only: !preferences.events_without_participants,
            attendees: self.attendees.clone(),
        }
    }

    fn accept(&mut self, snapshot: CalendarSnapshot, home: bool, agenda: bool) {
        let CalendarSnapshot {
            look_ahead_days,
            sources,
            mut events,
            personal_linked,
            personal_issue,
            attendees,
            personal,
        } = snapshot;
        events.sort_by(|a, b| {
            b.all_day
                .cmp(&a.all_day)
                .then_with(|| {
                    DateTime::parse_from_rfc3339(&a.start_at)
                        .ok()
                        .cmp(&DateTime::parse_from_rfc3339(&b.start_at).ok())
                })
                .then_with(|| a.id.cmp(&b.id))
        });
        self.look_ahead_days = look_ahead_days;
        self.personal_linked = personal_linked;
        self.personal_issue = personal_issue;
        self.sources = sources;
        self.personal = personal;
        self.attendees.extend(attendees);
        if agenda {
            self.agenda = events.clone();
        }
        if home {
            self.events = events;
            self.loaded = true;
        }
        self.revision = self.revision.wrapping_add(1);
    }
}

/// The note this Mac already has for an event the server says has one.
/// The note kept here for an event of the account in `slot`.
pub fn local_note_for_event(
    workspace: &crate::model::Workspace,
    event: &CalendarEventDTO,
    slot: u16,
    server: &str,
) -> Option<u64> {
    let note_id = event.note_id.as_deref()?;
    crate::server::cached_note(workspace, slot, note_id, server)
        .filter(|id| workspace.note(*id).is_some_and(|note| !note.trashed))
}

pub fn is_active_timed_event(event: &CalendarEventDTO, now: DateTime<chrono::Utc>) -> bool {
    if event.all_day {
        return false;
    }
    match (
        DateTime::parse_from_rfc3339(&event.start_at),
        DateTime::parse_from_rfc3339(&event.end_at),
    ) {
        (Ok(start), Ok(end)) => start <= now && now < end,
        _ => false,
    }
}

pub(crate) fn event_on_day(event: &CalendarEventDTO, day: NaiveDate, now: DateTime<Local>) -> bool {
    if event.all_day {
        let day = day.format("%Y-%m-%d").to_string();
        return event.start_date <= day && day < event.end_date;
    }
    let (Ok(start), Ok(end)) = (
        DateTime::parse_from_rfc3339(&event.start_at),
        DateTime::parse_from_rfc3339(&event.end_at),
    ) else {
        return false;
    };
    if end <= start || (day == now.date_naive() && end <= now) {
        return false;
    }
    let local_start = start.with_timezone(&Local);
    let local_end = end.with_timezone(&Local);
    local_start.date_naive() <= day
        && (local_end.date_naive() > day
            || (local_end.date_naive() == day && local_end.time() != chrono::NaiveTime::MIN))
}

fn events_on_day<'a>(
    sources: &[CalendarDTO],
    events: &'a [CalendarEventDTO],
    day: NaiveDate,
    now: DateTime<Local>,
    filter: &EventFilter,
) -> Vec<&'a CalendarEventDTO> {
    events
        .iter()
        .filter(|event| filter.shows(sources, event))
        .filter(|event| event_on_day(event, day, now))
        .collect()
}

pub(crate) fn event_time(event: &CalendarEventDTO) -> String {
    if event.all_day {
        return "All day".into();
    }
    match (
        DateTime::parse_from_rfc3339(&event.start_at),
        DateTime::parse_from_rfc3339(&event.end_at),
    ) {
        (Ok(start), Ok(end)) => format!(
            "{} – {}",
            start.with_timezone(&Local).format("%-I:%M %p"),
            end.with_timezone(&Local).format("%-I:%M %p")
        ),
        _ => "Time unavailable".into(),
    }
}

pub(crate) fn safe_meeting_url(text: &str) -> bool {
    let Some(rest) = text
        .strip_prefix("https://")
        .or_else(|| text.strip_prefix("http://"))
    else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    !authority.is_empty()
        && !authority.contains('@')
        && !text.chars().any(|c| c.is_whitespace() || c.is_control())
}

pub(crate) fn calendar_color(text: &str) -> Hsla {
    let value = text.strip_prefix('#').unwrap_or(text);
    rgb(if value.len() == 6 {
        u32::from_str_radix(value, 16).unwrap_or(0x87e5e8)
    } else {
        0x87e5e8
    })
    .into()
}

impl Cereal {
    /// The account "Coming up" is read through: the default account, when
    /// it is Ready. (One list per account, merged, comes with several
    /// accounts.)
    fn calendar_account(&self) -> Option<crate::accounts::Account> {
        crate::accounts::default_account().filter(|account| account.shown() && account.is_ready())
    }

    pub fn load_calendar_events(&mut self, cx: &mut Context<Self>) {
        if !crate::server::enabled() {
            return;
        }
        let Some(account) = self.calendar_account() else {
            return;
        };
        if let Some(cancel) = self.calendars.cancel.take() {
            cancel.store(true, Ordering::Release);
        }
        let cancel = Arc::new(AtomicBool::new(false));
        self.calendars.cancel = Some(cancel.clone());
        self.calendars.epoch = self.calendars.epoch.wrapping_add(1);
        let epoch = self.calendars.epoch;
        let account_epoch = account.epoch();
        let account_id = account.id.clone();
        self.calendars.loading = true;
        self.calendars.error = None;
        let day = page_day(self.calendar_page);
        // Home on its first page shows today onward: the menu bar's events.
        let home_is_today = self.calendar_page == 0;
        let start = Local
            .from_local_datetime(&day.and_hms_opt(0, 0, 0).expect("valid midnight"))
            .earliest()
            .unwrap_or_else(Local::now);
        let (send, receive) = mpsc::channel();
        let asked = account_id.clone();
        std::thread::spawn(move || {
            let _ = send.send(crate::server::load_calendar_events(
                &asked,
                &start.to_rfc3339(),
                cancel,
            ));
        });
        cx.spawn(async move |this, cx| {
            let result = loop {
                match receive.try_recv() {
                    Ok(result) => break result,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        break Err("Calendar worker stopped".into());
                    }
                    Err(mpsc::TryRecvError::Empty) => {
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(50))
                            .await
                    }
                }
            };
            let _ = this.update(cx, |this, cx| {
                if epoch != this.calendars.epoch
                    || crate::accounts::epoch_of(&account_id) != Some(account_epoch)
                {
                    return;
                }
                this.calendars.loading = false;
                match result {
                    Ok(snapshot) => {
                        this.calendars.account = Some(account_id.clone());
                        this.calendars.accept(snapshot, true, home_is_today)
                    }
                    Err(error) => this.calendars.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Read today's events again for the menu bar. With Home on its first
    /// page that is Home's own refresh; on another page it is a read of its
    /// own, and Home is left as it is.
    pub fn refresh_agenda(&mut self, cx: &mut Context<Self>) {
        if !crate::server::enabled() {
            return;
        }
        let Some(account) = self.calendar_account() else {
            return;
        };
        if self.calendar_page == 0 {
            self.load_calendar_events(cx);
            return;
        }
        self.calendars.agenda_epoch = self.calendars.agenda_epoch.wrapping_add(1);
        let epoch = self.calendars.agenda_epoch;
        let account_epoch = account.epoch();
        let account_id = account.id.clone();
        let start = Local
            .from_local_datetime(
                &crate::menubar::now()
                    .date_naive()
                    .and_hms_opt(0, 0, 0)
                    .expect("valid midnight"),
            )
            .earliest()
            .unwrap_or_else(Local::now);
        let (send, receive) = mpsc::channel();
        let asked = account_id.clone();
        std::thread::spawn(move || {
            let _ = send.send(crate::server::load_calendar_events(
                &asked,
                &start.to_rfc3339(),
                Arc::new(AtomicBool::new(false)),
            ));
        });
        cx.spawn(async move |this, cx| {
            let result = loop {
                match receive.try_recv() {
                    Ok(result) => break result,
                    Err(mpsc::TryRecvError::Disconnected) => return,
                    Err(mpsc::TryRecvError::Empty) => {
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(50))
                            .await
                    }
                }
            };
            let _ = this.update(cx, |this, cx| {
                if epoch != this.calendars.agenda_epoch
                    || crate::accounts::epoch_of(&account_id) != Some(account_epoch)
                {
                    return;
                }
                if let Ok(snapshot) = result {
                    this.calendars.account = Some(account_id.clone());
                    this.calendars.accept(snapshot, false, true);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Show or hide one calendar. A shared calendar is hidden on this Mac;
    /// one of the person's own is selected or not on the server, for every
    /// device.
    pub fn set_calendar_visible(&mut self, id: String, visible: bool, cx: &mut Context<Self>) {
        if !self.calendars.personal.iter().any(|calendar| calendar.id == id) {
            // Hidden on this Mac, for the account it was read through.
            let key = hidden_key(self.calendars.account.as_ref(), &id);
            let result = crate::desktop_preferences::update_meeting(|preferences| {
                preferences.hidden_calendars.retain(|hidden| *hidden != key);
                if !visible {
                    preferences.hidden_calendars.push(key.clone());
                }
            });
            if let Err(error) = result {
                self.show_toast(&error, cx);
            }
            cx.notify();
            return;
        }
        self.select_personal_calendars(vec![(id, visible)], cx);
    }

    /// "Reset": every shared calendar shows again, and of the person's own
    /// only the primary ones are selected.
    pub fn reset_visible_calendars(&mut self, cx: &mut Context<Self>) {
        // Only this account's calendars: another account's stay as they are.
        let prefix = hidden_key(self.calendars.account.as_ref(), "");
        if let Err(error) = crate::desktop_preferences::update_meeting(|p| {
            p.hidden_calendars
                .retain(|hidden| !prefix.is_empty() && !hidden.starts_with(&prefix))
        }) {
            self.show_toast(&error, cx);
        }
        let changes: Vec<(String, bool)> = self
            .calendars
            .personal
            .iter()
            .filter(|calendar| calendar.selected != calendar.primary)
            .map(|calendar| (calendar.id.clone(), calendar.primary))
            .collect();
        if !changes.is_empty() {
            self.select_personal_calendars(changes, cx);
        }
        cx.notify();
    }

    fn select_personal_calendars(&mut self, changes: Vec<(String, bool)>, cx: &mut Context<Self>) {
        if self.calendars.choosing.is_some() {
            return;
        }
        // A calendar's selection is changed on the server it was read from.
        let Some(account) = self
            .calendars
            .account
            .as_ref()
            .and_then(crate::accounts::get)
        else {
            return;
        };
        self.calendars.choosing = changes.first().map(|(id, _)| id.clone());
        let account_epoch = account.epoch();
        let account_id = account.id.clone();
        let (send, receive) = mpsc::channel();
        let requested = changes.clone();
        let asked = account_id.clone();
        std::thread::spawn(move || {
            let result = requested.iter().try_for_each(|(id, selected)| {
                crate::server::select_personal_calendar(&asked, id, *selected)
            });
            let _ = send.send(result);
        });
        cx.spawn(async move |this, cx| {
            let result = loop {
                match receive.try_recv() {
                    Ok(result) => break result,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        break Err("Calendar worker stopped".into());
                    }
                    Err(mpsc::TryRecvError::Empty) => {
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(50))
                            .await
                    }
                }
            };
            let _ = this.update(cx, |this, cx| {
                this.calendars.choosing = None;
                if crate::accounts::epoch_of(&account_id) != Some(account_epoch) {
                    return;
                }
                match result {
                    Ok(()) => {
                        for (id, selected) in changes {
                            if let Some(calendar) =
                                this.calendars.personal.iter_mut().find(|c| c.id == id)
                            {
                                calendar.selected = selected;
                            }
                        }
                        // The server reads a newly selected calendar at once.
                        this.refresh_agenda(cx);
                    }
                    Err(error) => this.show_toast(&error, cx),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(crate) fn open_calendar_event(&mut self, event_id: String, cx: &mut Context<Self>) {
        if self.calendars.opening.is_some() {
            return;
        }
        // An event is opened in the account it was read through. One whose
        // account is not Ready is refused with that account named, never
        // offered to another.
        let Some(account) = self
            .calendars
            .account
            .as_ref()
            .and_then(crate::accounts::get)
        else {
            return;
        };
        if !account.is_ready() {
            self.show_toast(&format!("{} can't be reached.", account.host()), cx);
            return;
        }
        let (slot, server) = (account.slot, account.url.clone());
        let known = self
            .calendars
            .events
            .iter()
            .chain(&self.calendars.agenda)
            .find(|event| event.id == event_id)
            .cloned();
        let active_at_gesture = known
            .clone()
            .filter(|event| is_active_timed_event(event, chrono::Utc::now()));
        // An event whose note is already on this Mac opens it without
        // asking the server for anything.
        if let Some(id) = known
            .as_ref()
            .and_then(|event| local_note_for_event(&self.workspace, event, slot, &server))
        {
            self.open_note(id, cx);
            if active_at_gesture.is_some() && !self.recording.is_active() {
                self.start_recording_preview(id, cx);
            }
            return;
        }
        let account_epoch = account.epoch();
        let account_id = account.id.clone();
        self.calendars.opening = Some(event_id.clone());
        let opened = event_id.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        self.calendars.open_cancel = Some(cancel.clone());
        let (send, receive) = mpsc::channel();
        let asked = account_id.clone();
        std::thread::spawn(move || {
            let _ = send.send(crate::server::note_for_calendar_event(&asked, &event_id, cancel));
        });
        cx.spawn(async move |this, cx| {
            let result = loop {
                match receive.try_recv() {
                    Ok(result) => break result,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        break Err("Calendar meeting worker stopped".into());
                    }
                    Err(mpsc::TryRecvError::Empty) => {
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(50))
                            .await
                    }
                }
            };
            let _ = this.update(cx, |this, cx| {
                this.calendars.opening = None;
                this.calendars.open_cancel = None;
                if crate::accounts::epoch_of(&account_id) != Some(account_epoch) {
                    return;
                }
                match result {
                    Ok((url, note)) => {
                        // The event has its note from now on: opening it
                        // again finds this one.
                        let server_id = note.id.clone();
                        for event in this
                            .calendars
                            .events
                            .iter_mut()
                            .chain(this.calendars.agenda.iter_mut())
                            .filter(|event| event.id == opened)
                        {
                            event.note_id = Some(server_id.clone());
                        }
                        let Some(id) = crate::server::import_calendar_note(
                            &mut this.workspace,
                            slot,
                            note,
                            url,
                        ) else {
                            this.show_toast("This note could not be kept on this Mac.", cx);
                            return;
                        };
                        this.persist(cx);
                        this.open_note(id, cx);
                        if active_at_gesture
                            .as_ref()
                            .is_some_and(|event| is_active_timed_event(event, chrono::Utc::now()))
                            && !this.recording.is_active()
                        {
                            this.start_recording_preview(id, cx);
                        }
                    }
                    Err(error) => this.show_toast(&error, cx),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub fn connected_calendar(&self, cx: &mut Context<Self>) -> Div {
        let t = self.theme();
        let day = page_day(self.calendar_page);
        let filter = self.calendars.filter();
        let mut rows = div()
            .id("connected-calendar-events")
            .track_scroll(&self.calendars.scroll)
            .max_h(px(302.))
            .overflow_y_scroll()
            .w_full()
            .rounded(px(17.))
            .bg(t.card)
            .border_1()
            .border_color(t.border)
            .flex()
            .flex_col();
        if let Some(error) = &self.calendars.error {
            rows = rows.child(
                div()
                    .p(px(16.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(small(
                        format!("Calendar events could not refresh. {error}"),
                        t,
                    ))
                    .child(
                        pill("calendar-retry", "Retry", "refresh", t)
                            .on_click(cx.listener(|this, _, _, cx| this.load_calendar_events(cx))),
                    ),
            );
        } else if self.calendars.loading && !self.calendars.loaded {
            rows = rows.child(
                div()
                    .p(px(20.))
                    .child(small("Loading linked calendars…", t)),
            );
        }
        if self.calendars.loaded {
            let enabled = self.calendars.sources.iter().filter(|c| c.enabled).count();
            if enabled == 0 && !self.calendars.personal_linked && self.calendars.events.is_empty() {
                rows = rows.child(
                    div()
                        .p(px(20.))
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child("Connect calendars to see upcoming meetings")
                        .child(small("Link multiple calendars in the admin portal.", t)),
                );
            } else {
                if self.calendars.personal_issue
                    || self
                        .calendars
                        .sources
                        .iter()
                        .any(|c| c.enabled && c.last_sync_error.is_some())
                {
                    rows = rows.child(div().px(px(16.)).py(px(10.)).child(small(
                        "A linked calendar needs attention; saved events may be out of date.",
                        t,
                    )));
                }
                let mut visible = 0;
                for delta in 0..i64::from(self.calendars.look_ahead_days) {
                    let date = day + Duration::days(delta);
                    let events = events_on_day(
                        &self.calendars.sources,
                        &self.calendars.events,
                        date,
                        crate::menubar::now(),
                        &filter,
                    );
                    if events.is_empty() && delta != 0 {
                        continue;
                    }
                    if events.is_empty()
                        && self.calendars.error.is_none()
                        && !self.calendars.loading
                    {
                        rows = rows.child(
                            div()
                                .min_h(px(60.))
                                .flex_shrink_0()
                                .px(px(16.))
                                .py(px(10.))
                                .flex()
                                .items_center()
                                .gap(px(12.))
                                .child(
                                    div()
                                        .w(px(140.))
                                        .flex_shrink_0()
                                        .flex()
                                        .items_center()
                                        .child(
                                            div()
                                                .font_family(DISPLAY)
                                                .w(px(40.))
                                                .text_size(px(27.))
                                                .line_height(px(32.))
                                                .child(date.day().to_string()),
                                        )
                                        .child(
                                            div()
                                                .flex()
                                                .flex_col()
                                                .text_size(px(11.))
                                                .line_height(px(12.))
                                                .child(
                                                    div()
                                                        .flex()
                                                        .items_center()
                                                        .gap(px(4.))
                                                        .child(date.format("%B").to_string())
                                                        .when(
                                                            date == crate::menubar::now().date_naive(),
                                                            |d| {
                                                                d.child(
                                                                    div()
                                                                        .size(px(5.))
                                                                        .rounded_full()
                                                                        .bg(rgb(0xc84f34)),
                                                                )
                                                            },
                                                        ),
                                                )
                                                .child(small(date.format("%a").to_string(), t)),
                                        ),
                                )
                                .child(div().w(px(3.)).h(px(28.)).rounded_full().bg(t.muted))
                                .child(small(
                                    if self.calendar_page == 0 {
                                        "No events today"
                                    } else {
                                        "No events on this day"
                                    },
                                    t,
                                )),
                        );
                    }
                    for (index, event) in events.into_iter().enumerate() {
                        if visible >= 100 {
                            break;
                        }
                        let id = event.id.clone();
                        let opening = self.calendars.opening.as_deref() == Some(id.as_str());
                        let mut event_row = div()
                            .id(SharedString::from(format!("linked-event-{id}-{delta}")))
                            .relative()
                            .min_h(px(60.))
                            .px(px(16.))
                            .py(px(10.))
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .when(self.calendars.opening.is_none(), |row| {
                                row.child(crate::motion::tactile(t.hover).radius(0.))
                            });
                        event_row = event_row
                            .child(
                                div()
                                    .w(px(140.))
                                    .flex_shrink_0()
                                    .flex()
                                    .items_center()
                                    .when(index == 0, |d| {
                                        d.child(
                                            div()
                                                .font_family(DISPLAY)
                                                .w(px(40.))
                                                .text_size(px(27.))
                                                .line_height(px(32.))
                                                .child(date.day().to_string()),
                                        )
                                        .child(
                                            div()
                                                .flex()
                                                .flex_col()
                                                .text_size(px(11.))
                                                .line_height(px(12.))
                                                .child(
                                                    div()
                                                        .flex()
                                                        .items_center()
                                                        .gap(px(4.))
                                                        .child(date.format("%B").to_string())
                                                        .when(
                                                            date == crate::menubar::now().date_naive(),
                                                            |d| {
                                                                d.child(
                                                                    div()
                                                                        .size(px(5.))
                                                                        .rounded_full()
                                                                        .bg(rgb(0xc84f34)),
                                                                )
                                                            },
                                                        ),
                                                )
                                                .child(small(date.format("%a").to_string(), t)),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .w(px(3.))
                                    .h(px(28.))
                                    .rounded_full()
                                    .bg(calendar_color(&event.color)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.))
                                    .child(
                                        div()
                                            .text_size(px(14.))
                                            .line_height(px(18.))
                                            .truncate()
                                            .child(if opening {
                                                "Opening meeting…".into()
                                            } else {
                                                event.title.clone()
                                            }),
                                    )
                                    .child(small(
                                        format!("{} · {}", event_time(event), event.calendar_name),
                                        t,
                                    ))
                                    .when_some(
                                        event.location.as_ref().filter(|value| !value.is_empty()),
                                        |d, location| d.child(small(location.clone(), t)),
                                    ),
                            );
                        if let Some(url) = event
                            .meeting_url
                            .as_ref()
                            .filter(|url| safe_meeting_url(url))
                        {
                            let url = url.clone();
                            event_row = event_row.child(
                                pill(
                                    SharedString::from(format!("join-event-{}-{delta}", event.id)),
                                    "Join",
                                    "external-link",
                                    t,
                                )
                                .h(px(28.))
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .on_click(cx.listener(
                                    move |_, _, _, cx| {
                                        cx.stop_propagation();
                                        cx.open_url(&url);
                                    },
                                )),
                            )
                        }
                        if self.calendars.opening.is_none() {
                            event_row = event_row.cursor_pointer().reach().on_click(cx.listener(
                                move |this, _, _, cx| this.open_calendar_event(id.clone(), cx),
                            ));
                        }
                        rows = rows.child(event_row);
                        visible += 1;
                    }
                }
                if visible == 100 {
                    rows = rows.child(
                        div()
                            .p(px(12.))
                            .child(small("Showing the first 100 event occurrences.", t)),
                    );
                }
            }
        }
        let heading_hover = crate::motion::reveal_key("linked-coming-up-heading");
        let controls = div()
            .flex()
            .gap(px(1.))
            .child(
                tool_tip(
                    "manage-calendars",
                    "sliders-horizontal",
                    "Calendar connections",
                    t,
                )
                .opacity(crate::motion::revealed(heading_hover))
                .on_click(cx.listener(|_, _, _, cx| {
                    if let Some(url) = crate::server::calendar_connections_url() {
                        cx.open_url(&url);
                    }
                })),
            )
            .child(
                tool_tip("linked-calendar-refresh", "refresh", "Refresh calendars", t)
                    .opacity(crate::motion::revealed(heading_hover))
                    .on_click(cx.listener(|this, _, _, cx| this.load_calendar_events(cx))),
            )
            .child(
                tool("linked-calendar-prev", "chevron-left", t).on_click(cx.listener(
                    |this, _, _, cx| {
                        this.calendar_page = this.calendar_page.saturating_sub(1);
                        this.load_calendar_events(cx);
                    },
                )),
            )
            .child(
                tool("linked-calendar-next", "chevron-right", t).on_click(cx.listener(
                    |this, _, _, cx| {
                        this.calendar_page = this.calendar_page.saturating_add(1);
                        this.load_calendar_events(cx);
                    },
                )),
            );
        let title = if self.calendar_page == 0 {
            "Coming up".into()
        } else {
            day.format("%B %-d").to_string()
        };
        div()
            .pt(px(10.))
            .w_full()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .child(
                div()
                    .id("linked-coming-up-heading")
                    .child(crate::motion::hover_probe(heading_hover))
                    .h(px(37.))
                    .px(px(8.))
                    .flex()
                    .items_start()
                    .justify_between()
                    .child(heading(title, t))
                    .child(controls),
            )
            .child(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        EventFilter, event_on_day, event_time, hidden_key, migrate_hidden_calendars,
        safe_meeting_url,
    };
    use chrono::{Duration, Local, NaiveDate, TimeZone};
    use meeting_notes_api_types::{CalendarDTO, CalendarEventDTO};
    use serde_json::json;

    fn events_on_day<'a>(
        sources: &[CalendarDTO],
        events: &'a [CalendarEventDTO],
        day: NaiveDate,
        now: chrono::DateTime<Local>,
    ) -> Vec<&'a CalendarEventDTO> {
        super::events_on_day(sources, events, day, now, &EventFilter::default())
    }
    fn source(id: &str, enabled: bool) -> CalendarDTO {
        serde_json::from_value(json!({"id":id,"name":id,"provider":"ics","color":"#87e5e8","enabled":enabled,"sourceHost":"calendar.example","lastSyncedAt":null,"lastSyncError":null,"eventCount":1,"createdAt":"2026-11-12T00:00:00Z"})).unwrap()
    }
    fn event(id: &str, calendar: &str, all_day: bool, start: &str, end: &str) -> CalendarEventDTO {
        serde_json::from_value(json!({"id":id,"calendarId":calendar,"calendarName":calendar,"color":"#87e5e8","title":id,"startAt":start,"endAt":end,"allDay":all_day,"startDate":"2026-11-26","endDate":"2026-11-28","timeZone":null,"location":null,"meetingUrl":null,"noteId":null})).unwrap()
    }
    #[test]
    fn capture_requires_an_explicit_active_timed_event_not_future_past_or_all_day() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-11-26T10:00:00Z")
            .unwrap()
            .to_utc();
        let active = event(
            "active",
            "work",
            false,
            "2026-11-26T10:00:00Z",
            "2026-11-26T11:00:00Z",
        );
        assert!(super::is_active_timed_event(&active, now));
        assert!(!super::is_active_timed_event(
            &active,
            now - chrono::Duration::seconds(1)
        ));
        assert!(!super::is_active_timed_event(
            &active,
            now + chrono::Duration::hours(1)
        ));
        let mut all_day = active;
        all_day.all_day = true;
        assert!(!super::is_active_timed_event(&all_day, now));
    }

    #[test]
    fn date_only_events_keep_their_local_dates_and_exclusive_multi_day_end() {
        let event = event(
            "holiday",
            "work",
            true,
            "2026-11-26T00:00:00Z",
            "2026-11-28T00:00:00Z",
        );
        let now = Local::now();
        for (day, expected) in [(25, false), (26, true), (27, true), (28, false)] {
            assert_eq!(
                event_on_day(&event, NaiveDate::from_ymd_opt(2026, 11, day).unwrap(), now),
                expected
            );
        }
        assert_eq!(event_time(&event), "All day");
    }
    #[test]
    fn overlapping_sources_remain_distinct_but_disabled_sources_do_not_appear() {
        let day = NaiveDate::from_ymd_opt(2026, 11, 26).unwrap();
        let events = vec![
            event(
                "first",
                "work",
                true,
                "2026-11-26T00:00:00Z",
                "2026-11-28T00:00:00Z",
            ),
            event(
                "second",
                "personal",
                true,
                "2026-11-26T00:00:00Z",
                "2026-11-28T00:00:00Z",
            ),
        ];
        let mut sources = vec![source("work", true), source("personal", true)];
        assert_eq!(events_on_day(&sources, &events, day, Local::now()).len(), 2);
        sources[0].enabled = false;
        let selected = events_on_day(&sources, &events, day, Local::now());
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].id, "second");
    }
    #[test]
    fn personal_selected_sources_do_not_require_an_instance_ics_subscription() {
        let day = NaiveDate::from_ymd_opt(2026, 11, 26).unwrap();
        let events = vec![
            event(
                "disabled",
                "shared",
                true,
                "2026-11-26T00:00:00Z",
                "2026-11-28T00:00:00Z",
            ),
            event(
                "personal",
                "google",
                true,
                "2026-11-26T00:00:00Z",
                "2026-11-28T00:00:00Z",
            ),
        ];
        let visible = events_on_day(&[source("shared", false)], &events, day, Local::now());
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].id, "personal");
    }

    #[test]
    fn timed_events_hide_after_end_and_midnight_end_does_not_spill_into_next_day() {
        let start = Local::now().date_naive().and_hms_opt(10, 0, 0).unwrap();
        let start = Local.from_local_datetime(&start).earliest().unwrap();
        let now = start + Duration::hours(2);
        let old = event(
            "old",
            "work",
            false,
            &start.to_rfc3339(),
            &(start + Duration::hours(1)).to_rfc3339(),
        );
        assert!(!event_on_day(&old, now.date_naive(), now));
        let midnight = Local
            .from_local_datetime(
                &now.date_naive()
                    .succ_opt()
                    .unwrap()
                    .and_hms_opt(0, 0, 0)
                    .unwrap(),
            )
            .earliest()
            .unwrap();
        let until_midnight = event(
            "end-exclusive",
            "work",
            false,
            &start.to_rfc3339(),
            &midnight.to_rfc3339(),
        );
        assert!(event_on_day(&until_midnight, now.date_naive(), now));
        assert!(!event_on_day(&until_midnight, midnight.date_naive(), now));
    }
    #[test]
    fn events_with_nobody_else_and_no_call_link_can_be_left_out() {
        let day = NaiveDate::from_ymd_opt(2026, 11, 26).unwrap();
        let start = Local
            .from_local_datetime(&day.and_hms_opt(10, 0, 0).unwrap())
            .earliest()
            .unwrap();
        let now = start - Duration::hours(1);
        let timed = |id: &str| {
            event(
                id,
                "work",
                false,
                &start.to_rfc3339(),
                &(start + Duration::hours(1)).to_rfc3339(),
            )
        };
        let mut call = timed("call");
        call.meeting_url = Some("https://meet.example/room".into());
        let mut holiday_with_link = event(
            "offsite",
            "work",
            true,
            "2026-11-26T00:00:00Z",
            "2026-11-28T00:00:00Z",
        );
        holiday_with_link.meeting_url = Some("https://meet.example/offsite".into());
        let mut unsafe_link = timed("unsafe-link");
        unsafe_link.meeting_url = Some("javascript:alert(1)".into());
        let events = vec![
            event(
                "holiday",
                "work",
                true,
                "2026-11-26T00:00:00Z",
                "2026-11-28T00:00:00Z",
            ),
            holiday_with_link,
            timed("focus"),
            timed("alone"),
            timed("invited"),
            unsafe_link,
            call,
        ];
        let shown = |filter: &EventFilter| -> Vec<String> {
            super::events_on_day(&[], &events, day, now, filter)
                .into_iter()
                .map(|event| event.id.clone())
                .collect()
        };
        // On (the default): everything shows, as it always has.
        assert_eq!(shown(&EventFilter::default()).len(), 7);
        // Off, where the server does not count attendees: an all-day event
        // without a call link goes, every timed event stays.
        let unknown = EventFilter {
            meetings_only: true,
            ..Default::default()
        };
        assert_eq!(
            shown(&unknown),
            ["offsite", "focus", "alone", "invited", "unsafe-link", "call"]
        );
        // Off, where it does: an event with nobody else stays only for its
        // call link, and a link that is not a web address does not count.
        let counted = EventFilter {
            meetings_only: true,
            attendees: [("focus", 0), ("alone", 0), ("invited", 2), ("unsafe-link", 0), ("call", 0)]
                .into_iter()
                .map(|(id, count)| (id.to_string(), count))
                .collect(),
            ..Default::default()
        };
        assert_eq!(shown(&counted), ["offsite", "invited", "call"]);
        // A calendar hidden on this Mac shows nothing, whatever else is set.
        let hidden = EventFilter {
            hidden_calendars: vec!["work".into()],
            ..Default::default()
        };
        assert!(shown(&hidden).is_empty());
        // Hidden calendars are recorded with their account: the same
        // calendar id of another account is not hidden.
        let account = crate::accounts::AccountId::of(
            "https://one.example",
            "00000000-0000-4000-8000-000000000001",
        );
        let other = crate::accounts::AccountId::of(
            "https://one.example",
            "00000000-0000-4000-8000-000000000002",
        );
        let keyed = EventFilter {
            hidden_calendars: vec![hidden_key(Some(&account), "work")],
            account: Some(account.clone()),
            ..Default::default()
        };
        assert!(shown(&keyed).is_empty());
        let theirs = EventFilter {
            account: Some(other),
            ..keyed.clone()
        };
        assert_eq!(shown(&theirs), shown(&EventFilter::default()));
        assert!(!shown(&theirs).is_empty());
        // Entries from before accounts get the migrated account's id, once.
        let mut saved = vec!["work".to_string(), hidden_key(Some(&account), "home")];
        assert!(migrate_hidden_calendars(&mut saved, &account));
        assert_eq!(saved, [hidden_key(Some(&account), "work"), hidden_key(Some(&account), "home")]);
        assert!(!migrate_hidden_calendars(&mut saved, &account));
    }

    #[test]
    fn an_event_whose_note_is_already_here_opens_that_note() {
        let mut workspace = crate::model::Workspace::with_slots(&[1, 2]);
        let remote: meeting_notes_api_types::NoteDTO = serde_json::from_value(json!({
            "id":"3a49a4af-c660-42e9-a36a-a806fbb41e0d","title":"Meeting","body":"","enhanced":"","date":"2026-11-26","duration":"","createdAt":"2026-11-26T10:00:00Z","updatedAt":"2026-11-26T10:00:00Z","recordingState":"idle","generationState":"idle","transcriptRevision":0,"generatedRevision":null,"transcriptSegments":[]
        })).unwrap();
        let mut meeting = event(
            "meeting",
            "work",
            false,
            "2026-11-26T10:00:00Z",
            "2026-11-26T11:00:00Z",
        );
        let server = "https://one.example";
        // Never opened: there is nothing here to open yet.
        assert_eq!(super::local_note_for_event(&workspace, &meeting, 1, server), None);
        // Opened once: the server's note arrives and the event remembers it.
        let first =
            crate::server::import_calendar_note(&mut workspace, 1, remote.clone(), server.into())
                .unwrap();
        meeting.note_id = Some(remote.id.clone());
        let count = workspace.notes.len();
        // Opened again, either way round: the same note, and no new one.
        assert_eq!(
            super::local_note_for_event(&workspace, &meeting, 1, server),
            Some(first)
        );
        assert_eq!(
            crate::server::import_calendar_note(&mut workspace, 1, remote, server.into()),
            Some(first)
        );
        assert_eq!(workspace.notes.len(), count);
        // The same event on another server is another note, and so is the
        // same event id in another account on the same server.
        assert_eq!(
            super::local_note_for_event(&workspace, &meeting, 1, "https://other.example"),
            None
        );
        assert_eq!(super::local_note_for_event(&workspace, &meeting, 2, server), None);
        // A note in the Trash is not opened in place; the server's answer
        // restores it (see `import_calendar_note`).
        workspace.note_mut(first).unwrap().trashed = true;
        assert_eq!(super::local_note_for_event(&workspace, &meeting, 1, server), None);
    }

    #[test]
    fn join_links_cannot_execute_other_schemes_or_hide_credentials() {
        assert!(safe_meeting_url("https://meet.example/room?id=1"));
        for url in [
            "javascript:alert(1)",
            "file:///private/data",
            "https://secret@meet.example/room",
            "https://",
            "https://meet.example/\nroom",
        ] {
            assert!(!safe_meeting_url(url), "{url}");
        }
    }
}
