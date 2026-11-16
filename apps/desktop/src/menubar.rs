//! Cereal in the macOS menu bar: its mark, the next meeting and the time
//! until it starts, and a menu of today's and tomorrow's events.
//!
//! Everything shown is decided here, from the same events and filters as
//! Home's "Coming up" (see `calendar.rs`); `native/MenuBar.swift` draws it
//! and reports what was chosen. The item is redrawn when what it shows has
//! changed and once a minute for the countdown, and nothing runs in between.
use crate::app::{Cereal, Screen};
use chrono::{DateTime, Duration, Local, Timelike};
use gpui::*;
use meeting_notes_api_types::CalendarEventDTO;
use serde::Serialize;
use std::collections::HashMap;
use unicode_segmentation::UnicodeSegmentation;

/// The most of a meeting's title the menu bar shows, the ellipsis included.
pub const TITLE_LIMIT: usize = 16;
/// How long before a meeting its reminder is delivered.
const REMINDER_LEAD_SECONDS: i64 = 60;
/// Reminders are planned this far ahead, and planned again as time passes.
const REMINDER_HORIZON_HOURS: i64 = 24;

/// The time everything here goes by. A debug build started with
/// `CEREAL_FAKE_NOW=<RFC 3339 time>` begins at that time and runs on from
/// it, so a meeting about to start can be looked at without waiting for one.
pub fn now() -> DateTime<Local> {
    #[cfg(debug_assertions)]
    {
        static OFFSET: std::sync::OnceLock<Duration> = std::sync::OnceLock::new();
        let offset = OFFSET.get_or_init(|| {
            crate::environment::var("CEREAL_FAKE_NOW")
                .ok()
                .and_then(|text| DateTime::parse_from_rfc3339(&text).ok())
                .map(|fake| fake.with_timezone(&Local) - Local::now())
                .unwrap_or_else(Duration::zero)
        });
        Local::now() + *offset
    }
    #[cfg(not(debug_assertions))]
    Local::now()
}

/// How far a debug build's clock is from the real one (see [`now`]).
fn clock_offset() -> Duration {
    now() - Local::now()
}

/// A title cut to `limit` characters, the ellipsis included. The cut never
/// leaves a space or a separator hanging before the ellipsis.
pub fn shorten(title: &str, limit: usize) -> String {
    let title = title.trim();
    let graphemes: Vec<&str> = title.graphemes(true).collect();
    if graphemes.len() <= limit {
        return title.to_owned();
    }
    let kept: String = graphemes[..limit.saturating_sub(1)].concat();
    let kept = kept.trim_end_matches(|c: char| {
        c.is_whitespace()
            || matches!(
                c,
                '-' | '–' | '—' | ',' | ':' | ';' | '·' | '+' | '/' | '&' | '(' | '|'
            )
    });
    format!("{kept}…")
}

/// When a meeting starts, counted from `now`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Countdown {
    /// It has started and has not ended.
    Now,
    /// "in 11h 16m": whole minutes, rounded up, so "in 1m" lasts until the
    /// meeting starts.
    In(String),
}

pub fn countdown(start: DateTime<Local>, now: DateTime<Local>) -> Countdown {
    let seconds = (start - now).num_seconds();
    if start <= now {
        return Countdown::Now;
    }
    let minutes = (seconds.max(1) + 59) / 60;
    let (days, hours, minutes) = (minutes / 1440, minutes % 1440 / 60, minutes % 60);
    Countdown::In(if days > 0 && hours > 0 {
        format!("in {days}d {hours}h")
    } else if days > 0 {
        format!("in {days}d")
    } else if hours > 0 && minutes > 0 {
        format!("in {hours}h {minutes}m")
    } else if hours > 0 {
        format!("in {hours}h")
    } else {
        format!("in {minutes}m")
    })
}

/// The service a call link belongs to, when its host is one we know.
pub fn meeting_service(url: &str) -> Option<&'static str> {
    if !crate::calendar::safe_meeting_url(url) {
        return None;
    }
    let rest = url.split_once("://")?.1;
    let host = rest
        .split(['/', '?', '#', ':'])
        .next()?
        .to_ascii_lowercase();
    let is = |domain: &str| host == domain || host.ends_with(&format!(".{domain}"));
    Some(if is("zoom.us") || is("zoom.com") || is("zoomgov.com") {
        "Zoom"
    } else if is("teams.microsoft.com") || is("teams.live.com") || is("teams.microsoft.us") {
        "Microsoft Teams"
    } else if is("meet.google.com") {
        "Google Meet"
    } else if is("webex.com") {
        "Webex"
    } else if is("whereby.com") {
        "Whereby"
    } else if is("meet.jit.si") {
        "Jitsi"
    } else if is("gotomeeting.com") || is("goto.com") || is("gotomeet.me") {
        "GoTo Meeting"
    } else if is("bluejeans.com") {
        "BlueJeans"
    } else if is("chime.aws") {
        "Amazon Chime"
    } else if is("slack.com") {
        "Slack"
    } else if is("discord.com") || is("discord.gg") {
        "Discord"
    } else if is("facetime.apple.com") {
        "FaceTime"
    } else if is("around.co") {
        "Around"
    } else {
        return None;
    })
}

/// One event as the menu lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub id: String,
    pub title: String,
    /// "5:00 PM – 5:30 PM · Microsoft Teams", or "All day".
    pub detail: String,
}

fn row(event: &CalendarEventDTO) -> Row {
    let mut detail = crate::calendar::event_time(event);
    if let Some(service) = event.meeting_url.as_deref().and_then(meeting_service) {
        detail = format!("{detail} · {service}");
    }
    let title = event.title.trim();
    Row {
        id: event.id.clone(),
        title: if title.is_empty() {
            "Untitled event".into()
        } else {
            title.to_owned()
        },
        detail,
    }
}

/// What the menu bar says about the calendar at one moment.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Agenda {
    /// The meeting that is on, or the one that starts next, and when.
    pub next: Option<(Row, Countdown)>,
    /// The rest of today's events, then tomorrow's.
    pub today: Vec<Row>,
    pub tomorrow: Vec<Row>,
}

fn times(event: &CalendarEventDTO) -> Option<(DateTime<Local>, DateTime<Local>)> {
    if event.all_day {
        return None;
    }
    let start = DateTime::parse_from_rfc3339(&event.start_at).ok()?;
    let end = DateTime::parse_from_rfc3339(&event.end_at).ok()?;
    (end > start).then(|| (start.with_timezone(&Local), end.with_timezone(&Local)))
}

/// The meeting the menu bar counts down to: the one that is on now (the
/// latest to have started, when several are) or else the next to start.
/// All-day events are never counted down to.
pub fn next_meeting<'a>(
    events: &[&'a CalendarEventDTO],
    now: DateTime<Local>,
) -> Option<&'a CalendarEventDTO> {
    let timed = || {
        events
            .iter()
            .filter_map(|event| times(event).map(|(start, end)| (*event, start, end)))
    };
    timed()
        .filter(|(_, start, end)| *start <= now && now < *end)
        .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.id.cmp(&a.0.id)))
        .or_else(|| {
            timed()
                .filter(|(_, start, _)| *start > now)
                .min_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.id.cmp(&b.0.id)))
        })
        .map(|(event, _, _)| event)
}

/// `events` are the ones Home would show (already filtered), in its order.
pub fn agenda(events: &[&CalendarEventDTO], now: DateTime<Local>) -> Agenda {
    let next = next_meeting(events, now);
    let today = now.date_naive();
    let on = |day| -> Vec<Row> {
        events
            .iter()
            .filter(|event| crate::calendar::event_on_day(event, day, now))
            .filter(|event| next.is_none_or(|next| next.id != event.id))
            .map(|event| row(event))
            .collect()
    };
    Agenda {
        next: next.and_then(|event| {
            let (start, _) = times(event)?;
            Some((row(event), countdown(start, now)))
        }),
        today: on(today),
        tomorrow: today.succ_opt().map(on).unwrap_or_default(),
    }
}

/// One line of the menu, as the native side reads it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Entry {
    /// "header" (a grey caption), "event", "action", "separator", "submenu".
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub argument: Option<String>,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<Entry>,
}

impl Entry {
    fn separator() -> Self {
        Self {
            kind: "separator",
            ..Default::default()
        }
    }
    fn header(title: impl Into<String>) -> Self {
        Self {
            kind: "header",
            title: Some(title.into()),
            ..Default::default()
        }
    }
    fn action(title: impl Into<String>, command: &'static str) -> Self {
        Self {
            kind: "action",
            title: Some(title.into()),
            command: Some(command),
            enabled: true,
            ..Default::default()
        }
    }
    fn event(row: &Row) -> Self {
        Self {
            kind: "event",
            title: Some(row.title.clone()),
            subtitle: Some(row.detail.clone()),
            command: Some("event"),
            argument: Some(row.id.clone()),
            enabled: true,
            ..Default::default()
        }
    }
}

/// The whole item: the text beside the mark and the menu under it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Model {
    /// "Fronti… · in 11h 16m". Without it only the mark shows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub tooltip: String,
    pub items: Vec<Entry>,
}

/// What the menu shows besides the calendar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Surroundings {
    /// "Show upcoming meetings in menu bar" is on.
    pub show_events: bool,
    pub version: String,
    /// "Latest version", "Update available", …
    pub update_status: &'static str,
    /// Whether "Check for updates" can be chosen now.
    pub can_check: bool,
    /// A debug build says so, so it is never taken for the installed app.
    pub debug: bool,
}

pub fn model(agenda: &Agenda, around: &Surroundings) -> Model {
    let mut items = Vec::new();
    let mut title = None;
    if around.show_events {
        if let Some((row, countdown)) = &agenda.next {
            let (caption, when) = match countdown {
                Countdown::Now => ("Happening now".to_owned(), "now".to_owned()),
                Countdown::In(when) => (format!("Starts {when}"), when.clone()),
            };
            title = Some(format!("{} · {when}", shorten(&row.title, TITLE_LIMIT)));
            items.extend([
                Entry::header(caption),
                Entry::event(row),
                Entry::separator(),
            ]);
        }
        for (caption, rows) in [("Today", &agenda.today), ("Tomorrow", &agenda.tomorrow)] {
            if rows.is_empty() {
                continue;
            }
            items.push(Entry::header(caption));
            items.extend(rows.iter().map(Entry::event));
            items.push(Entry::separator());
        }
    }
    let name = if around.debug {
        format!("Cereal v{} (debug build)", around.version)
    } else {
        format!("Cereal v{}", around.version)
    };
    items.extend([
        Entry::action("Open Cereal", "open"),
        Entry::action("New Note", "new-note"),
        Entry::action("Settings", "settings"),
        Entry::header(name),
        Entry::header(around.update_status),
        Entry {
            enabled: around.can_check,
            ..Entry::action("Check for updates", "update")
        },
        Entry::separator(),
        Entry::action("Quit", "quit"),
        Entry {
            kind: "submenu",
            title: Some("Quit Options".into()),
            enabled: true,
            items: vec![
                Entry::action(
                    if around.show_events {
                        "Don't show events in the menu bar"
                    } else {
                        "Show events in the menu bar"
                    },
                    "toggle-events",
                ),
                Entry::action("Quit Completely", "quit-completely"),
            ],
            ..Default::default()
        },
    ]);
    if around.debug {
        title = Some(match title {
            Some(title) => format!("{title} · dev"),
            None => "dev".into(),
        });
    }
    Model {
        title,
        tooltip: if around.debug {
            "Cereal (debug build)".into()
        } else {
            "Cereal".into()
        },
        items,
    }
}

/// The two ways out of the app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuitKind {
    /// "Quit", in the menus and on ⌘Q.
    Quit,
    /// "Quit Completely".
    Completely,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuitPlan {
    /// The window closes and the app leaves the Dock; the menu bar item,
    /// meeting reminders and a recording in progress carry on.
    StayInMenuBar,
    /// The app exits. A recording in progress is stopped and its audio is
    /// on disk before it does.
    Exit { finish_recording: bool },
}

/// "Quit" leaves Cereal in the menu bar only while there is a menu bar item
/// to come back through; without one it is the same as "Quit Completely".
/// Neither loses a recording: one carries on, the other waits for its audio.
pub fn quit_plan(kind: QuitKind, menu_bar_item: bool, recording: bool) -> QuitPlan {
    match kind {
        QuitKind::Quit if menu_bar_item => QuitPlan::StayInMenuBar,
        _ => QuitPlan::Exit {
            finish_recording: recording,
        },
    }
}

/// A notification planned for a meeting.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reminder {
    pub event_id: String,
    pub title: String,
    pub body: String,
    /// One minute before the meeting starts.
    pub fire_at: DateTime<Local>,
}

/// The reminders to have waiting at `now`: one for every timed meeting that
/// has not started and starts within a day. A meeting already inside its
/// last minute is still reminded of (at once); one that has started is not.
pub fn reminders(events: &[&CalendarEventDTO], now: DateTime<Local>) -> Vec<Reminder> {
    let mut planned: Vec<Reminder> = events
        .iter()
        .filter_map(|event| {
            let (start, _) = times(event)?;
            (start > now && start - now <= Duration::hours(REMINDER_HORIZON_HOURS)).then(|| {
                let row = row(event);
                let mut body = format!("Starts at {}", start.format("%-I:%M %p"));
                if let Some(service) = event.meeting_url.as_deref().and_then(meeting_service) {
                    body = format!("{body} · {service}");
                }
                Reminder {
                    event_id: event.id.clone(),
                    title: row.title,
                    body,
                    fire_at: start - Duration::seconds(REMINDER_LEAD_SECONDS),
                }
            })
        })
        .collect();
    planned.sort_by(|a, b| {
        a.fire_at
            .cmp(&b.fire_at)
            .then_with(|| a.event_id.cmp(&b.event_id))
    });
    planned.truncate(32);
    planned
}

/// What to change with macOS so that the reminders `waiting` there (event
/// id to the second each fires) become the ones `wanted`: the event ids to
/// withdraw, and the reminders to add or move. One that is already due is
/// never withdrawn: it is macOS's to deliver.
pub fn reminder_changes<'a>(
    waiting: &HashMap<String, i64>,
    wanted: &'a [Reminder],
    now: DateTime<Local>,
) -> (Vec<String>, Vec<&'a Reminder>) {
    let mut remove: Vec<String> = waiting
        .iter()
        .filter(|(id, fire_at)| {
            **fire_at > now.timestamp() && !wanted.iter().any(|reminder| reminder.event_id == **id)
        })
        .map(|(id, _)| id.clone())
        .collect();
    remove.sort();
    let add = wanted
        .iter()
        .filter(|reminder| waiting.get(&reminder.event_id) != Some(&reminder.fire_at.timestamp()))
        .collect();
    (remove, add)
}

/// What macOS has said about notifications from this app.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Permission {
    #[default]
    Unknown,
    Granted,
    Denied,
}

/// The item's state in the main window's view.
#[derive(Default)]
pub struct State {
    pub installed: bool,
    /// The model last handed to the native side.
    pushed: String,
    /// What the model was last built from.
    signature: Option<(u64, u64, &'static str, bool, u64, bool)>,
    /// Reminders waiting with macOS: event id to the second they fire.
    reminders: HashMap<String, i64>,
    /// "Scheduled meetings" was just turned on and macOS has not answered.
    asking: bool,
    pub permission: Permission,
    /// The login item's state (see `native::login_item_status`).
    pub login_item: i32,
}

fn logging() -> bool {
    static LOG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *LOG.get_or_init(|| crate::environment::var_os("CEREAL_MENUBAR_LOG").is_some())
}

impl Cereal {
    fn menu_bar_events(&self) -> Vec<&CalendarEventDTO> {
        if !crate::server::enabled() || crate::account::identity().is_none() {
            return Vec::new();
        }
        let filter = self.calendars.filter();
        self.calendars
            .agenda
            .iter()
            .filter(|event| filter.shows(&self.calendars.sources, event))
            .collect()
    }

    fn menu_bar_model(&self) -> Model {
        let preferences = crate::desktop_preferences::meeting();
        let agenda = if preferences.menu_bar_events {
            agenda(&self.menu_bar_events(), now())
        } else {
            Agenda::default()
        };
        model(
            &agenda,
            &Surroundings {
                show_events: preferences.menu_bar_events,
                version: crate::update::current_version().to_owned(),
                update_status: self.updates.status(),
                can_check: self.updates.row().2,
                debug: cfg!(debug_assertions),
            },
        )
    }

    /// Redraw the item if what it shows has changed, and keep the planned
    /// reminders in step with the calendar.
    pub fn refresh_menu_bar(&mut self) {
        if !self.menu_bar.installed {
            return;
        }
        let model = self.menu_bar_model();
        let json = serde_json::to_string(&model).unwrap_or_default();
        if json != self.menu_bar.pushed {
            if logging() {
                eprintln!("menubar model {json}");
            }
            native::update(&json);
            self.menu_bar.pushed = json;
        }
        self.sync_reminders();
    }

    fn sync_reminders(&mut self) {
        let wanted = if crate::desktop_preferences::meeting().scheduled_notifications {
            reminders(&self.menu_bar_events(), now())
        } else {
            Vec::new()
        };
        let (remove, add) = reminder_changes(&self.menu_bar.reminders, &wanted, now());
        let remove: Vec<String> = remove.iter().map(|id| format!("meeting-{id}")).collect();
        if remove.is_empty() && add.is_empty() {
            return;
        }
        // macOS works to the real clock, whatever a debug build's says.
        let offset = clock_offset().num_seconds();
        let change = serde_json::json!({
            "remove": remove,
            "add": add.iter().map(|reminder| serde_json::json!({
                "id": format!("meeting-{}", reminder.event_id),
                "title": reminder.title,
                "body": reminder.body,
                "fireAt": reminder.fire_at.timestamp() - offset,
                "eventId": reminder.event_id,
            })).collect::<Vec<_>>(),
        })
        .to_string();
        if logging() {
            eprintln!("menubar reminders {change}");
        }
        native::notifications_apply(&change);
        self.menu_bar.reminders = wanted
            .into_iter()
            .map(|reminder| (reminder.event_id, reminder.fire_at.timestamp()))
            .collect();
    }

    /// Runs after every change to the view; looks again only when one of
    /// the few things the item is drawn from has changed.
    fn menu_bar_observed(&mut self) {
        let signature = (
            self.calendars.revision,
            crate::desktop_preferences::meeting_revision(),
            self.updates.status(),
            self.updates.row().2,
            crate::account::epoch(),
            crate::account::identity().is_some(),
        );
        if self.menu_bar.signature != Some(signature) {
            self.menu_bar.signature = Some(signature);
            self.refresh_menu_bar();
        }
    }

    /// Bring the main window forward, and the app back into the Dock.
    pub fn reveal_main_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        crate::pill::reveal_main_window(window);
        window.activate_window();
        cx.activate(true);
    }

    /// "Quit" and "Quit Completely", wherever they were chosen.
    pub fn quit_requested(&mut self, kind: QuitKind, window: &mut Window, cx: &mut Context<Self>) {
        match quit_plan(kind, self.menu_bar.installed, self.recording.is_active()) {
            QuitPlan::StayInMenuBar => {
                // Everything typed so far is on disk before the window goes.
                self.finish_note(cx);
                crate::pill::hide_main_window(window, cx);
                native::set_dock_visible(false);
            }
            QuitPlan::Exit { .. } => self.quit_after_audio_stop(cx),
        }
    }

    pub fn set_menu_bar_events(&mut self, show: bool, cx: &mut Context<Self>) {
        match crate::desktop_preferences::update_meeting(|p| p.menu_bar_events = show) {
            Ok(()) => cx.notify(),
            Err(error) => self.show_toast(&error, cx),
        }
    }

    /// "Scheduled meetings" in Settings. macOS is asked for permission only
    /// here, when the setting is turned on.
    pub fn set_scheduled_notifications(&mut self, on: bool, cx: &mut Context<Self>) {
        if !on {
            self.menu_bar.asking = false;
            if let Err(error) =
                crate::desktop_preferences::update_meeting(|p| p.scheduled_notifications = false)
            {
                self.show_toast(&error, cx);
            }
            cx.notify();
            return;
        }
        if !self.menu_bar.installed {
            self.show_toast("Meeting reminders are available on macOS", cx);
            return;
        }
        self.menu_bar.asking = true;
        native::notifications_request();
        cx.notify();
    }

    fn notifications_answered(&mut self, answer: &str, cx: &mut Context<Self>) {
        let asking = std::mem::take(&mut self.menu_bar.asking);
        let granted = answer == "granted";
        self.menu_bar.permission = match answer {
            "granted" => Permission::Granted,
            "denied" => Permission::Denied,
            _ => Permission::Unknown,
        };
        let enabled = crate::desktop_preferences::meeting().scheduled_notifications;
        if granted && asking && !enabled {
            if let Err(error) =
                crate::desktop_preferences::update_meeting(|p| p.scheduled_notifications = true)
            {
                self.show_toast(&error, cx);
            }
        } else if !granted && (asking || enabled) {
            // The setting never claims reminders macOS will not deliver.
            let _ =
                crate::desktop_preferences::update_meeting(|p| p.scheduled_notifications = false);
            if asking {
                self.show_toast(
                    match answer {
                        "unavailable" => "Meeting reminders need the packaged Cereal app",
                        _ => "Allow notifications for Cereal in System Settings to get meeting reminders",
                    },
                    cx,
                );
            }
        }
        cx.notify();
    }

    /// "Open Cereal when you log in".
    pub fn set_login_item(&mut self, on: bool, cx: &mut Context<Self>) {
        if let Err(error) = native::login_item_set(on) {
            self.show_toast(&error, cx);
        }
        self.menu_bar.login_item = native::login_item_status();
        if logging() {
            eprintln!("menubar login-item status {}", self.menu_bar.login_item);
        }
        cx.notify();
    }

    pub fn refresh_login_item(&mut self) {
        self.menu_bar.login_item = native::login_item_status();
    }

    fn menu_bar_command(
        &mut self,
        command: &str,
        argument: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if logging() {
            eprintln!("menubar command {command} {argument}");
        }
        let welcome = self.screen == Screen::Welcome;
        match command {
            "open" => self.reveal_main_window(window, cx),
            "event" | "open-event" => {
                self.reveal_main_window(window, cx);
                if !welcome && !argument.is_empty() {
                    self.open_calendar_event(argument.to_owned(), cx);
                }
            }
            "new-note" => {
                self.reveal_main_window(window, cx);
                if !welcome {
                    self.new_note(window, cx);
                }
            }
            "settings" => {
                self.reveal_main_window(window, cx);
                if !welcome {
                    self.navigate(Screen::Settings, cx);
                }
            }
            "update" => {
                self.reveal_main_window(window, cx);
                self.check_for_updates(cx);
            }
            "toggle-events" => {
                let show = !crate::desktop_preferences::meeting().menu_bar_events;
                self.set_menu_bar_events(show, cx);
            }
            "quit" => self.quit_requested(QuitKind::Quit, window, cx),
            "quit-completely" => self.quit_requested(QuitKind::Completely, window, cx),
            "notifications" => self.notifications_answered(argument, cx),
            _ => {}
        }
    }

    fn menu_bar_tick(&mut self, cx: &mut Context<Self>) {
        self.refresh_menu_bar();
        // The calendar is read again every five minutes, so a meeting added
        // while the window is closed still reaches the menu.
        if now().minute() % 5 == 0 {
            self.refresh_agenda(cx);
        }
    }
}

/// Put the app back in the Dock (after "Quit" took it out).
pub fn show_in_dock() {
    native::set_dock_visible(true);
}

/// Install the item, once, after the main window exists.
/// The app menu's commands when the main window is not the key window
/// (closed with ⌘W, or behind the recording pill): the window's own handlers
/// are not reached then, and macOS would show the items greyed out. Each
/// brings the window back and then does what it says.
fn install_window_commands(handle: WindowHandle<Cereal>, cx: &mut App) {
    fn command<A: gpui::Action>(
        handle: WindowHandle<Cereal>,
        cx: &mut App,
        run: impl Fn(&mut Cereal, &mut Window, &mut Context<Cereal>) + 'static,
    ) {
        cx.on_action(move |_: &A, cx| {
            let _ = handle.update(cx, |view, window, cx| {
                view.reveal_main_window(window, cx);
                if view.screen != Screen::Welcome {
                    run(view, window, cx);
                }
                cx.notify();
            });
        });
    }
    command::<crate::app::NewNote>(handle, cx, |view, window, cx| view.new_note(window, cx));
    command::<crate::app::Search>(handle, cx, |view, window, cx| view.open_search(window, cx));
    command::<crate::app::OpenSettings>(handle, cx, |view, _, cx| {
        view.navigate(Screen::Settings, cx)
    });
    command::<crate::app::GoHome>(handle, cx, |view, _, cx| view.navigate(Screen::Home, cx));
    // An update can be looked for from the welcome too.
    cx.on_action(move |_: &crate::app::CheckForUpdates, cx| {
        let _ = handle.update(cx, |view, window, cx| {
            view.reveal_main_window(window, cx);
            view.check_for_updates(cx);
        });
    });
    // With no window to close or hide, these have nothing to do; handled
    // here so that "Hide Cereal" still hides the app's other windows.
    cx.on_action(|_: &crate::app::Hide, cx| cx.hide());
}

pub fn install(handle: WindowHandle<Cereal>, cx: &mut App) {
    install_window_commands(handle, cx);
    if !native::supported() || std::env::args().any(|argument| argument == "--smoke-test") {
        return;
    }
    let commands = cx.to_async();
    native::install(Box::new(move |command, argument| {
        // Queued: never run inside the menu's own event.
        commands
            .spawn(async move |cx| {
                let _ = handle.update(cx, |view, window, cx| {
                    view.menu_bar_command(&command, &argument, window, cx)
                });
            })
            .detach();
    }));
    let _ = handle.update(cx, |view, _, cx| {
        view.menu_bar.installed = true;
        view.menu_bar.login_item = native::login_item_status();
        let observed = cx.observe_self(|view, _| view.menu_bar_observed());
        view._subscriptions.push(observed);
        view.menu_bar_observed();
        view.refresh_menu_bar();
        if crate::desktop_preferences::meeting().scheduled_notifications {
            native::notifications_status();
        }
    });
    // ⌘Q with the window closed has no window to handle it.
    cx.on_action(move |_: &crate::app::Quit, cx| {
        let _ = handle.update(cx, |view, window, cx| {
            view.quit_requested(QuitKind::Quit, window, cx)
        });
    });
    cx.on_action(move |_: &crate::app::QuitCompletely, cx| {
        let _ = handle.update(cx, |view, window, cx| {
            view.quit_requested(QuitKind::Completely, window, cx)
        });
    });
    cx.on_app_quit(|_| {
        // Nothing of the app outlives it: no item, no reminder left to
        // fire after "Quit Completely".
        native::notifications_clear();
        native::remove();
        async {}
    })
    .detach();
    // Once a minute, on the minute, for the countdown.
    cx.spawn(async move |cx| {
        loop {
            let current = now();
            let wait = 60_000
                - i64::from(current.second()) * 1000
                - i64::from(current.timestamp_subsec_millis().min(999))
                + 20;
            cx.background_executor()
                .timer(std::time::Duration::from_millis(
                    wait.clamp(20, 60_020) as u64
                ))
                .await;
            if handle
                .update(cx, |view, _, cx| view.menu_bar_tick(cx))
                .is_err()
            {
                break;
            }
        }
    })
    .detach();
}

#[cfg(target_os = "macos")]
mod native {
    use std::ffi::{CStr, CString, c_char};

    type Callback = extern "C" fn(*const c_char, *const c_char);
    unsafe extern "C" {
        fn cereal_menubar_install(mark: *const c_char, name: *const c_char, callback: Callback);
        fn cereal_menubar_update(json: *const c_char) -> bool;
        fn cereal_menubar_remove();
        fn cereal_menubar_frame(output: *mut f64);
        fn cereal_app_set_dock_visible(visible: bool);
        fn cereal_notifications_status();
        fn cereal_notifications_request();
        fn cereal_notifications_apply(json: *const c_char) -> bool;
        fn cereal_notifications_clear();
        fn cereal_login_item_status() -> i32;
        fn cereal_login_item_set(enabled: bool, error: *mut c_char, capacity: usize) -> bool;
    }

    type Handler = Box<dyn Fn(String, String)>;
    thread_local! {
        static HANDLER: std::cell::RefCell<Option<Handler>> = const { std::cell::RefCell::new(None) };
    }

    /// The native side calls this on the main thread.
    extern "C" fn picked(command: *const c_char, argument: *const c_char) {
        let text = |pointer: *const c_char| {
            if pointer.is_null() {
                String::new()
            } else {
                unsafe { CStr::from_ptr(pointer) }
                    .to_string_lossy()
                    .into_owned()
            }
        };
        let (command, argument) = (text(command), text(argument));
        HANDLER.with(|handler| {
            if let Some(handler) = handler.borrow().as_ref() {
                handler(command, argument);
            }
        });
    }

    pub fn supported() -> bool {
        true
    }

    /// The outline of Cereal's mark, from its own icon.
    fn mark_path() -> String {
        let svg = include_str!("../assets/icons/cereal.svg");
        svg.split(" d=\"")
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .unwrap_or_default()
            .to_owned()
    }

    pub fn install(handler: Handler) {
        HANDLER.with(|slot| *slot.borrow_mut() = Some(handler));
        let mark = CString::new(mark_path()).unwrap_or_default();
        // A debug build keeps its own place in the menu bar.
        let name = CString::new(if cfg!(debug_assertions) {
            "dev.cereal.desktop.menubar.debug"
        } else {
            "dev.cereal.desktop.menubar"
        })
        .unwrap_or_default();
        unsafe { cereal_menubar_install(mark.as_ptr(), name.as_ptr(), picked) }
    }

    pub fn update(json: &str) {
        let Ok(json) = CString::new(json) else { return };
        unsafe {
            cereal_menubar_update(json.as_ptr());
        }
        if super::logging() {
            let mut frame = [0f64; 4];
            unsafe { cereal_menubar_frame(frame.as_mut_ptr()) };
            eprintln!(
                "menubar frame {} {} {} {}",
                frame[0], frame[1], frame[2], frame[3]
            );
        }
    }

    pub fn remove() {
        unsafe { cereal_menubar_remove() }
    }

    pub fn set_dock_visible(visible: bool) {
        unsafe { cereal_app_set_dock_visible(visible) }
    }

    pub fn notifications_status() {
        unsafe { cereal_notifications_status() }
    }

    pub fn notifications_request() {
        unsafe { cereal_notifications_request() }
    }

    pub fn notifications_apply(json: &str) {
        let Ok(json) = CString::new(json) else { return };
        unsafe {
            cereal_notifications_apply(json.as_ptr());
        }
    }

    pub fn notifications_clear() {
        unsafe { cereal_notifications_clear() }
    }

    pub fn login_item_status() -> i32 {
        unsafe { cereal_login_item_status() }
    }

    pub fn login_item_set(enabled: bool) -> Result<(), String> {
        let mut error = [0 as c_char; 512];
        if unsafe { cereal_login_item_set(enabled, error.as_mut_ptr(), error.len()) } {
            return Ok(());
        }
        let message = unsafe { CStr::from_ptr(error.as_ptr()) }
            .to_string_lossy()
            .into_owned();
        Err(if message.is_empty() {
            "macOS could not change the login item".into()
        } else {
            message
        })
    }
}

#[cfg(not(target_os = "macos"))]
mod native {
    pub fn supported() -> bool {
        false
    }
    pub fn install(_: Box<dyn Fn(String, String)>) {}
    pub fn update(_: &str) {}
    pub fn remove() {}
    pub fn set_dock_visible(_: bool) {}
    pub fn notifications_status() {}
    pub fn notifications_request() {}
    pub fn notifications_apply(_: &str) {}
    pub fn notifications_clear() {}
    pub fn login_item_status() -> i32 {
        -1
    }
    pub fn login_item_set(_: bool) -> Result<(), String> {
        Err("Opening at login is available on macOS".into())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Agenda, Countdown, QuitKind, QuitPlan, Row, Surroundings, TITLE_LIMIT, agenda, countdown,
        meeting_service, model, quit_plan, reminder_changes, reminders, shorten,
    };
    use chrono::{DateTime, Duration, Local, TimeZone};
    use meeting_notes_api_types::CalendarEventDTO;
    use serde_json::json;
    use unicode_segmentation::UnicodeSegmentation;

    fn at(day: u32, hour: u32, minute: u32, second: u32) -> DateTime<Local> {
        Local
            .with_ymd_and_hms(2026, 11, day, hour, minute, second)
            .earliest()
            .unwrap()
    }

    fn timed(
        id: &str,
        start: DateTime<Local>,
        minutes: i64,
        url: Option<&str>,
    ) -> CalendarEventDTO {
        serde_json::from_value(json!({
            "id": id, "calendarId": "work", "calendarName": "Work", "color": "#87e5e8",
            "title": id, "startAt": start.to_rfc3339(),
            "endAt": (start + Duration::minutes(minutes)).to_rfc3339(), "allDay": false,
            "startDate": start.format("%Y-%m-%d").to_string(),
            "endDate": start.format("%Y-%m-%d").to_string(),
            "timeZone": null, "location": null, "meetingUrl": url, "noteId": null
        }))
        .unwrap()
    }

    fn all_day(id: &str, start: &str, end: &str) -> CalendarEventDTO {
        serde_json::from_value(json!({
            "id": id, "calendarId": "work", "calendarName": "Work", "color": "#87e5e8",
            "title": id, "startAt": format!("{start}T00:00:00Z"), "endAt": format!("{end}T00:00:00Z"),
            "allDay": true, "startDate": start, "endDate": end,
            "timeZone": null, "location": null, "meetingUrl": null, "noteId": null
        }))
        .unwrap()
    }

    fn around(show_events: bool) -> Surroundings {
        Surroundings {
            show_events,
            version: "0.1.0".into(),
            update_status: "Latest version",
            can_check: true,
            debug: false,
        }
    }

    fn ids(rows: &[Row]) -> Vec<&str> {
        rows.iter().map(|row| row.id.as_str()).collect()
    }

    #[test]
    fn countdown_rounds_up_to_whole_minutes_and_turns_to_now_at_the_start() {
        let start = at(16, 11, 30, 0);
        let text = |now| match countdown(start, now) {
            Countdown::Now => "now".to_owned(),
            Countdown::In(text) => text,
        };
        assert_eq!(text(at(16, 0, 14, 0)), "in 11h 16m");
        // Inside the last minute it still says a minute, never "in 0m".
        assert_eq!(text(at(16, 11, 29, 1)), "in 1m");
        assert_eq!(text(at(16, 11, 29, 59)), "in 1m");
        assert_eq!(text(at(16, 11, 29, 0)), "in 1m");
        assert_eq!(text(at(16, 11, 28, 59)), "in 2m");
        // An hour exactly has no minutes to name; a second more has one.
        assert_eq!(text(at(16, 10, 30, 0)), "in 1h");
        assert_eq!(text(at(16, 10, 30, 1)), "in 1h");
        assert_eq!(text(at(16, 10, 29, 59)), "in 1h 1m");
        assert_eq!(text(at(16, 10, 31, 0)), "in 59m");
        assert_eq!(text(at(16, 11, 30, 0)), "now");
        assert_eq!(text(at(16, 11, 45, 0)), "now");
        // Across midnight the count is of time, not of calendar days.
        assert_eq!(text(at(15, 23, 50, 0)), "in 11h 40m");
        assert_eq!(text(at(15, 11, 30, 0)), "in 1d");
        assert_eq!(text(at(15, 9, 0, 0)), "in 1d 2h");
    }

    #[test]
    fn titles_are_cut_to_fit_without_a_dangling_separator() {
        assert_eq!(shorten("R&D Weekly", TITLE_LIMIT), "R&D Weekly");
        assert_eq!(shorten("Exactly sixteen!", TITLE_LIMIT), "Exactly sixteen!");
        let cut = shorten("Frontier Labs + Self-hosted Models", TITLE_LIMIT);
        assert_eq!(cut, "Frontier Labs…");
        assert!(cut.graphemes(true).count() <= TITLE_LIMIT);
        assert_eq!(
            shorten("Raghav Pillai - Schedule a Time to Meet", TITLE_LIMIT),
            "Raghav Pillai…"
        );
        // Characters made of several code points are never split.
        let flags = "🇮🇳".repeat(20);
        let cut = shorten(&flags, TITLE_LIMIT);
        assert_eq!(cut.graphemes(true).count(), TITLE_LIMIT);
        assert!(cut.ends_with('…'));
        assert_eq!(shorten("  padded  ", TITLE_LIMIT), "padded");
    }

    #[test]
    fn the_menu_lists_the_next_meeting_then_the_rest_of_today_and_tomorrow() {
        let now = at(16, 0, 14, 0);
        let events = [
            all_day("holiday", "2026-11-16", "2026-11-17"),
            timed("frontier", at(16, 11, 30, 0), 60, None),
            timed(
                "schedule",
                at(16, 17, 0, 0),
                30,
                Some("https://teams.microsoft.com/l/meetup-join/1"),
            ),
            timed(
                "zealot",
                at(16, 19, 30, 0),
                30,
                Some("https://us02web.zoom.us/j/1"),
            ),
            timed("weekly", at(17, 14, 0, 0), 60, None),
            timed("later", at(18, 15, 0, 0), 30, None),
        ];
        let events: Vec<&CalendarEventDTO> = events.iter().collect();
        let agenda = agenda(&events, now);
        let (next, when) = agenda.next.clone().unwrap();
        assert_eq!(next.id, "frontier");
        assert_eq!(when, Countdown::In("in 11h 16m".into()));
        // The next meeting has its own place and is not listed again; an
        // all-day event is listed but never counted down to.
        assert_eq!(ids(&agenda.today), ["holiday", "schedule", "zealot"]);
        assert_eq!(ids(&agenda.tomorrow), ["weekly"]);
        assert_eq!(agenda.today[0].detail, "All day");
        assert_eq!(
            agenda.today[1].detail,
            "5:00 PM – 5:30 PM · Microsoft Teams"
        );
        assert_eq!(agenda.today[2].detail, "7:30 PM – 8:00 PM · Zoom");
        assert_eq!(agenda.tomorrow[0].detail, "2:00 PM – 3:00 PM");

        let model = model(&agenda, &around(true));
        assert_eq!(model.title.as_deref(), Some("frontier · in 11h 16m"));
        let titles: Vec<&str> = model
            .items
            .iter()
            .map(|entry| entry.title.as_deref().unwrap_or("-"))
            .collect();
        assert_eq!(
            titles,
            [
                "Starts in 11h 16m",
                "frontier",
                "-",
                "Today",
                "holiday",
                "schedule",
                "zealot",
                "-",
                "Tomorrow",
                "weekly",
                "-",
                "Open Cereal",
                "New Note",
                "Settings",
                "Cereal v0.1.0",
                "Latest version",
                "Check for updates",
                "-",
                "Quit",
                "Quit Options",
            ]
        );
        let event = &model.items[5];
        assert_eq!(
            (event.kind, event.command, event.argument.as_deref()),
            ("event", Some("event"), Some("schedule"))
        );
        // The version and the update status are captions: they cannot be chosen.
        assert!(model.items[14].kind == "header" && model.items[15].kind == "header");
        let options: Vec<_> = model.items[19]
            .items
            .iter()
            .map(|entry| (entry.title.as_deref().unwrap(), entry.command.unwrap()))
            .collect();
        assert_eq!(
            options,
            [
                ("Don't show events in the menu bar", "toggle-events"),
                ("Quit Completely", "quit-completely")
            ]
        );
    }

    #[test]
    fn events_that_ended_are_gone_and_one_in_progress_reads_now() {
        let events = [
            timed("ended", at(16, 9, 0, 0), 30, None),
            timed("running", at(16, 10, 0, 0), 60, None),
            timed("after", at(16, 12, 0, 0), 30, None),
        ];
        let events: Vec<&CalendarEventDTO> = events.iter().collect();
        let agenda_at = |now| agenda(&events, now);
        let during = agenda_at(at(16, 10, 20, 0));
        assert_eq!(during.next.as_ref().unwrap().0.id, "running");
        assert_eq!(during.next.as_ref().unwrap().1, Countdown::Now);
        assert_eq!(ids(&during.today), ["after"]);
        let shown = model(&during, &around(true));
        assert_eq!(shown.title.as_deref(), Some("running · now"));
        assert_eq!(shown.items[0].title.as_deref(), Some("Happening now"));
        // The moment it ends, the next meeting takes its place.
        let ended = agenda_at(at(16, 11, 0, 0));
        assert_eq!(ended.next.as_ref().unwrap().0.id, "after");
        assert_eq!(
            ended.next.as_ref().unwrap().1,
            Countdown::In("in 1h".into())
        );
        assert!(ended.today.is_empty());
        // With everything over, only the mark is left.
        let over = agenda_at(at(16, 13, 0, 0));
        assert_eq!(over, Agenda::default());
        assert_eq!(model(&over, &around(true)).title, None);
    }

    #[test]
    fn a_meeting_after_midnight_is_counted_down_to_from_the_evening_before() {
        let events = [
            timed("late", at(16, 23, 30, 0), 60, None),
            timed("early", at(17, 0, 10, 0), 30, None),
        ];
        let events: Vec<&CalendarEventDTO> = events.iter().collect();
        let before = agenda(&events, at(16, 23, 0, 0));
        assert_eq!(before.next.as_ref().unwrap().0.id, "late");
        assert_eq!(ids(&before.tomorrow), ["early"]);
        // "late" is on, and runs past midnight.
        let evening = agenda(&events, at(16, 23, 50, 0));
        assert_eq!(evening.next.as_ref().unwrap().0.id, "late");
        assert_eq!(evening.next.as_ref().unwrap().1, Countdown::Now);
        let events_without_late: Vec<&CalendarEventDTO> = events[1..].to_vec();
        let waiting = agenda(&events_without_late, at(16, 23, 50, 0));
        assert_eq!(
            waiting.next.as_ref().unwrap().1,
            Countdown::In("in 20m".into())
        );
        assert!(waiting.today.is_empty() && waiting.tomorrow.is_empty());
        let midnight = agenda(&events, at(17, 0, 15, 0));
        assert_eq!(midnight.next.as_ref().unwrap().0.id, "early");
        assert_eq!(ids(&midnight.today), ["late"]);
    }

    #[test]
    fn hiding_events_leaves_the_mark_and_the_app_menu() {
        let events = [timed("frontier", at(16, 11, 30, 0), 60, None)];
        let events: Vec<&CalendarEventDTO> = events.iter().collect();
        let hidden = model(&Agenda::default(), &around(false));
        assert_eq!(hidden.title, None);
        assert_eq!(hidden.items[0].title.as_deref(), Some("Open Cereal"));
        let toggle = &hidden.items.last().unwrap().items[0];
        assert_eq!(toggle.title.as_deref(), Some("Show events in the menu bar"));
        // Even handed an agenda, nothing of it is drawn while events are off.
        let agenda = agenda(&events, at(16, 0, 14, 0));
        assert_eq!(model(&agenda, &around(false)), hidden);
        // A debug build is told apart in the bar, in the menu and in its tooltip.
        let debug = model(
            &agenda,
            &Surroundings {
                debug: true,
                ..around(true)
            },
        );
        assert_eq!(debug.title.as_deref(), Some("frontier · in 11h 16m · dev"));
        assert_eq!(debug.tooltip, "Cereal (debug build)");
        assert!(
            debug
                .items
                .iter()
                .any(|entry| entry.title.as_deref() == Some("Cereal v0.1.0 (debug build)"))
        );
        let serialized = serde_json::to_value(&debug).unwrap();
        assert_eq!(serialized["items"][1]["argument"], "frontier");
        assert_eq!(
            serialized["items"][2],
            json!({"kind": "separator", "enabled": false})
        );
    }

    #[test]
    fn call_links_name_their_service_and_nothing_else_does() {
        for (url, service) in [
            (
                "https://teams.microsoft.com/l/meetup-join/19%3a",
                Some("Microsoft Teams"),
            ),
            ("https://us02web.zoom.us/j/123?pwd=1", Some("Zoom")),
            ("https://meet.google.com/abc-defg-hij", Some("Google Meet")),
            ("https://acme.webex.com/meet/someone", Some("Webex")),
            ("https://example.com/room", None),
            ("https://notzoom.us.example.com/j/1", None),
            ("https://evilzoom.us/j/1", None),
            ("javascript:alert(1)", None),
            ("https://user@zoom.us/j/1", None),
        ] {
            assert_eq!(meeting_service(url), service, "{url}");
        }
    }

    #[test]
    fn quit_stays_in_the_menu_bar_and_quit_completely_exits_without_losing_a_recording() {
        // "Quit": the window goes and the item stays; a recording carries on.
        assert_eq!(
            quit_plan(QuitKind::Quit, true, false),
            QuitPlan::StayInMenuBar
        );
        assert_eq!(
            quit_plan(QuitKind::Quit, true, true),
            QuitPlan::StayInMenuBar
        );
        // Without an item to come back through, "Quit" really quits.
        assert_eq!(
            quit_plan(QuitKind::Quit, false, true),
            QuitPlan::Exit {
                finish_recording: true
            }
        );
        assert_eq!(
            quit_plan(QuitKind::Completely, true, true),
            QuitPlan::Exit {
                finish_recording: true
            }
        );
        assert_eq!(
            quit_plan(QuitKind::Completely, true, false),
            QuitPlan::Exit {
                finish_recording: false
            }
        );
    }

    #[test]
    fn reminders_are_planned_a_minute_before_each_meeting_of_the_coming_day() {
        let events = [
            all_day("holiday", "2026-11-16", "2026-11-17"),
            timed("running", at(16, 9, 0, 0), 60, None),
            timed(
                "soon",
                at(16, 9, 30, 30),
                30,
                Some("https://meet.google.com/abc"),
            ),
            timed("noon", at(16, 12, 0, 0), 30, None),
            timed("far", at(17, 12, 0, 0), 30, None),
        ];
        let events: Vec<&CalendarEventDTO> = events.iter().collect();
        // The clock is whatever the caller says it is.
        let planned = reminders(&events, at(16, 9, 30, 0));
        let fire: Vec<_> = planned
            .iter()
            .map(|reminder| (reminder.event_id.as_str(), reminder.fire_at))
            .collect();
        assert_eq!(
            fire,
            [("soon", at(16, 9, 29, 30)), ("noon", at(16, 11, 59, 0))],
            "No reminder for an all-day event, one that has started, or one more than a day away"
        );
        assert_eq!(planned[0].body, "Starts at 9:30 AM · Google Meet");
        assert_eq!(planned[1].title, "noon");
        // Once it has started there is nothing left to remind of.
        assert!(
            reminders(&events, at(16, 9, 30, 30))
                .iter()
                .all(|r| r.event_id != "soon")
        );
        // A day later the far one has come into range.
        assert_eq!(reminders(&events, at(16, 12, 0, 0))[0].event_id, "far");
    }

    #[test]
    fn reminders_with_macos_follow_the_calendar_without_repeating_one_already_due() {
        let events = [
            timed("standup", at(16, 9, 30, 0), 15, None),
            timed("review", at(16, 14, 0, 0), 60, None),
        ];
        let listed: Vec<&CalendarEventDTO> = events.iter().collect();
        let mut waiting = std::collections::HashMap::new();
        let mut settle = |wanted: &[super::Reminder], now| {
            let (remove, add) = reminder_changes(&waiting, wanted, now);
            let summary = (
                remove.clone(),
                add.iter().map(|r| r.event_id.clone()).collect::<Vec<_>>(),
            );
            waiting = wanted
                .iter()
                .map(|r| (r.event_id.clone(), r.fire_at.timestamp()))
                .collect();
            summary
        };
        let ids = |ids: &[&str]| ids.iter().map(|id| id.to_string()).collect::<Vec<_>>();
        // First look at the day: both are handed to macOS.
        let morning = at(16, 8, 0, 0);
        assert_eq!(
            settle(&reminders(&listed, morning), morning),
            (ids(&[]), ids(&["standup", "review"]))
        );
        // A minute later nothing has changed, so nothing is sent.
        let later = at(16, 8, 1, 0);
        assert_eq!(
            settle(&reminders(&listed, later), later),
            (ids(&[]), ids(&[]))
        );
        // The review moves: its reminder is replaced, the other is left.
        let moved = [
            events[0].clone(),
            timed("review", at(16, 15, 0, 0), 60, None),
        ];
        let moved: Vec<&CalendarEventDTO> = moved.iter().collect();
        assert_eq!(
            settle(&reminders(&moved, later), later),
            (ids(&[]), ids(&["review"]))
        );
        // The standup has begun: its reminder was due a minute before and is
        // not withdrawn; the cancelled review's is.
        let begun = at(16, 9, 30, 0);
        let only_standup: Vec<&CalendarEventDTO> = vec![&events[0]];
        assert_eq!(
            settle(&reminders(&only_standup, begun), begun),
            (ids(&["review"]), ids(&[]))
        );
        // Turning the setting off withdraws whatever is still to come.
        let early = at(16, 8, 2, 0);
        let _ = settle(&reminders(&listed, early), early);
        assert_eq!(settle(&[], early), (ids(&["review", "standup"]), ids(&[])));
    }
}
