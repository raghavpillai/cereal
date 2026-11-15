//! Connected calendars are server-owned; the local preview's sample agenda is
//! never substituted for a missing, empty or failed linked-calendar response.
use crate::{app::Cereal, ui::*};
use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, TimeZone};
use gpui::{prelude::*, *};
use meeting_notes_api_types::{CalendarDTO, CalendarEventDTO};
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
}

#[derive(Default)]
pub struct CalendarState {
    pub sources: Vec<CalendarDTO>,
    pub events: Vec<CalendarEventDTO>,
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
    Local::now()
        .date_naive()
        .checked_add_signed(Duration::days(i64::from(page) * 7))
        .unwrap_or_else(|| Local::now().date_naive())
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

fn event_on_day(event: &CalendarEventDTO, day: NaiveDate, now: DateTime<Local>) -> bool {
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
) -> Vec<&'a CalendarEventDTO> {
    events
        .iter()
        .filter(|event| {
            sources
                .iter()
                .find(|source| source.id == event.calendar_id)
                .is_none_or(|source| source.enabled)
        })
        .filter(|event| event_on_day(event, day, now))
        .collect()
}

fn event_time(event: &CalendarEventDTO) -> String {
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

fn calendar_color(text: &str) -> Hsla {
    let value = text.strip_prefix('#').unwrap_or(text);
    rgb(if value.len() == 6 {
        u32::from_str_radix(value, 16).unwrap_or(0x87e5e8)
    } else {
        0x87e5e8
    })
    .into()
}

impl Cereal {
    pub fn load_calendar_events(&mut self, cx: &mut Context<Self>) {
        if !crate::server::enabled() || crate::account::identity().is_none() {
            return;
        }
        if let Some(cancel) = self.calendars.cancel.take() {
            cancel.store(true, Ordering::Release);
        }
        let cancel = Arc::new(AtomicBool::new(false));
        self.calendars.cancel = Some(cancel.clone());
        self.calendars.epoch = self.calendars.epoch.wrapping_add(1);
        let epoch = self.calendars.epoch;
        let account_epoch = crate::account::epoch();
        self.calendars.loading = true;
        self.calendars.error = None;
        let day = page_day(self.calendar_page);
        let start = Local
            .from_local_datetime(&day.and_hms_opt(0, 0, 0).expect("valid midnight"))
            .earliest()
            .unwrap_or_else(Local::now);
        let (send, receive) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = send.send(crate::server::load_calendar_events(
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
                if epoch != this.calendars.epoch || account_epoch != crate::account::epoch() {
                    return;
                }
                this.calendars.loading = false;
                match result {
                    Ok(snapshot) => {
                        let CalendarSnapshot {
                            look_ahead_days: look_ahead,
                            sources,
                            mut events,
                            personal_linked,
                            personal_issue,
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
                        this.calendars.look_ahead_days = look_ahead;
                        this.calendars.personal_linked = personal_linked;
                        this.calendars.personal_issue = personal_issue;
                        this.calendars.sources = sources;
                        this.calendars.events = events;
                        this.calendars.loaded = true;
                    }
                    Err(error) => this.calendars.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn open_calendar_event(&mut self, event_id: String, cx: &mut Context<Self>) {
        if self.calendars.opening.is_some() {
            return;
        }
        let active_at_gesture = self
            .calendars
            .events
            .iter()
            .find(|event| event.id == event_id)
            .filter(|event| is_active_timed_event(event, chrono::Utc::now()))
            .cloned();
        let account_epoch = crate::account::epoch();
        self.calendars.opening = Some(event_id.clone());
        let cancel = Arc::new(AtomicBool::new(false));
        self.calendars.open_cancel = Some(cancel.clone());
        let (send, receive) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = send.send(crate::server::note_for_calendar_event(&event_id, cancel));
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
                if account_epoch != crate::account::epoch() {
                    return;
                }
                this.calendars.opening = None;
                this.calendars.open_cancel = None;
                match result {
                    Ok((url, note)) => {
                        let id =
                            crate::server::import_calendar_note(&mut this.workspace, note, url);
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
                        Local::now(),
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
                                                            date == Local::now().date_naive(),
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
                                                            date == Local::now().date_naive(),
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
    use super::{event_on_day, event_time, events_on_day, safe_meeting_url};
    use chrono::{Duration, Local, NaiveDate, TimeZone};
    use meeting_notes_api_types::{CalendarDTO, CalendarEventDTO};
    use serde_json::json;

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
