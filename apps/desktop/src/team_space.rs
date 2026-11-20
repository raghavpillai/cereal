//! The team space: the notes the members of a workspace show to each other,
//! and the people those meetings were with.
//!
//! The page is read from the default account's server (`/api/v1/team-space`)
//! and holds nothing of its own: which notes are in the space, who may read
//! them and who may take them out are all the server's decisions. A note of
//! one's own opens as the note it is; anyone else's opens read-only, drawn by
//! the same page that shows a shared note.
use crate::{
    accounts::AccountId,
    app::{Cereal, Screen},
    input::TextInput,
    shared::{SharedNote, SharedNoteError},
    shared_view::Phase,
    ui::*,
};
use chrono::{DateTime, Datelike, Local, NaiveDate, TimeZone};
use gpui::{prelude::*, *};
use meeting_notes_api_types::{
    TeamSpaceDTO, TeamSpaceMemberDTO, TeamSpaceNoteDTO, TeamSpacePersonDTO,
};
use std::sync::Arc;

/// What the page says under its title, and on its banner.
pub const SUBTITLE: &str = "Notes visible to your entire workspace.";
pub const BANNER_TITLE: &str = "Your team space";
pub const BANNER_TEXT: &str = "Share meeting notes with your team. Add user interviews, sales calls and team meetings so everyone stays in the loop.";

/// The page's geometry in points, measured from the reference captures.
pub mod size {
    /// The page column and its side padding, as on My notes.
    pub const COLUMN: f32 = 768.;
    pub const COLUMN_PAD: f32 = 24.;
    /// The row of members' faces above the title.
    pub const FACE: f32 = 36.;
    pub const FACE_OVERLAP: f32 = 8.;
    /// Faces drawn before the "+N" bubble.
    pub const FACES_SHOWN: usize = 8;
    pub const FACES_TOP: f32 = 4.;
    pub const FACES_TO_TILE: f32 = 36.;
    pub const TAB_HEIGHT: f32 = 26.;
    /// A person's row: a 32 pt face on a 50 pt pitch.
    pub const PERSON_ROW: f32 = 50.;
    pub const PERSON_FACE: f32 = 32.;
    pub const EMAIL_COLUMN: f32 = 159.;
    pub const COUNT_COLUMN: f32 = 68.;
    pub const DATE_COLUMN: f32 = 84.;
    pub const PICKER_WIDTH: f32 = 480.;
    pub const PICKER_LIST: f32 = 320.;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Notes,
    People,
}

/// The "Add notes" sheet: the notes ticked so far, by their handle here.
#[derive(Default)]
pub struct Picker {
    pub selected: Vec<u64>,
    pub sending: bool,
}

/// Someone else's note, open read-only.
pub struct Reading {
    pub server_id: String,
    pub phase: Phase,
}

pub struct TeamSpace {
    pub page: Option<TeamSpaceDTO>,
    /// The account the page was read with.
    pub account: Option<AccountId>,
    pub loading: bool,
    pub problem: Option<String>,
    pub tab: Tab,
    pub picker: Option<Picker>,
    pub picker_search: Entity<TextInput>,
    pub people_search: Entity<TextInput>,
    pub people_search_open: bool,
    /// People are listed by their latest note, newest first unless this is set.
    pub oldest_first: bool,
    /// The person whose notes the Notes tab is narrowed to.
    pub person: Option<TeamSpacePersonDTO>,
    pub reading: Option<Reading>,
    pub scroll: ScrollHandle,
    picker_scroll: ScrollHandle,
    /// Counts the requests made, so a late answer to an earlier one is dropped.
    epoch: u64,
}

impl TeamSpace {
    pub fn new(cx: &mut Context<Cereal>) -> (Self, Vec<Subscription>) {
        let picker_search = cx.new(|cx| TextInput::new("Search your notes", false, cx));
        let people_search = cx.new(|cx| TextInput::new("Search people", false, cx));
        let subscriptions = vec![
            cx.observe(&picker_search, |_, _, cx| cx.notify()),
            cx.observe(&people_search, |_, _, cx| cx.notify()),
        ];
        (
            Self {
                page: None,
                account: None,
                loading: false,
                problem: None,
                tab: Tab::Notes,
                picker: None,
                picker_search,
                people_search,
                people_search_open: false,
                oldest_first: false,
                person: None,
                reading: None,
                scroll: ScrollHandle::new(),
                picker_scroll: ScrollHandle::new(),
                epoch: 0,
            },
            subscriptions,
        )
    }
}

/// "<Organization> team", or "Team" while the workspace has no name here.
pub fn space_name(organization: &str) -> String {
    let organization = organization.trim();
    if organization.is_empty() {
        "Team".into()
    } else {
        format!("{organization} team")
    }
}

/// Who a meeting was with, as a row says it: "Ada", "Ada & Ben",
/// "Ada, Ben & 4 others". `others` counts everyone but the note's owner.
pub fn attendee_line(names: &[String], others: usize, owner: &str) -> String {
    let shown: Vec<&str> = names.iter().map(String::as_str).take(2).collect();
    let rest = others.saturating_sub(shown.len());
    match (shown.as_slice(), rest) {
        ([], _) => owner.to_owned(),
        ([one], 0) => (*one).to_owned(),
        ([one, two], 0) => format!("{one} & {two}"),
        (shown, 1) => format!("{} & 1 other", shown.join(", ")),
        (shown, rest) => format!("{} & {rest} others", shown.join(", ")),
    }
}

/// The clock time a row shows: when the meeting began, else when the note
/// was made, in this Mac's time zone.
pub fn row_time(note: &TeamSpaceNoteDTO) -> String {
    let stamp = note
        .meeting
        .as_ref()
        .filter(|meeting| !meeting.all_day)
        .map_or(note.created_at.as_str(), |meeting| meeting.start_at.as_str());
    DateTime::parse_from_rfc3339(stamp)
        .map(|at| at.with_timezone(&Local).format("%-I:%M %p").to_string())
        .unwrap_or_default()
}

/// A person's latest note as the table shows it: "Jul 7", with the year when
/// it is not this one.
pub fn last_note_label(date: &str, today: NaiveDate) -> String {
    match NaiveDate::parse_from_str(date, "%Y-%m-%d") {
        Ok(day) if day.year() == today.year() => day.format("%b %-d").to_string(),
        Ok(day) => day.format("%b %-d, %Y").to_string(),
        Err(_) => date.to_owned(),
    }
}

/// The people the table lists: those matching `query` by name or address,
/// by their latest note.
pub fn listed_people<'a>(
    people: &'a [TeamSpacePersonDTO],
    query: &str,
    oldest_first: bool,
) -> Vec<&'a TeamSpacePersonDTO> {
    let query = query.trim().to_lowercase();
    let mut listed: Vec<&TeamSpacePersonDTO> = people
        .iter()
        .filter(|person| {
            query.is_empty()
                || person.name.to_lowercase().contains(&query)
                || person
                    .email
                    .as_ref()
                    .is_some_and(|email| email.to_lowercase().contains(&query))
        })
        .collect();
    listed.sort_by(|a, b| {
        let by_date = b.last_note_date.cmp(&a.last_note_date);
        let by_date = if oldest_first { by_date.reverse() } else { by_date };
        by_date.then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    listed
}

/// The fill and ink of someone's letter, chosen by who they are so that it
/// is the same wherever they appear.
fn tint(key: &str) -> (Hsla, Hsla) {
    const TINTS: [(u32, u32); 8] = [
        (0x9a5220, 0xffdcae),
        (0x5b4298, 0xe4d8ff),
        (0x414a22, 0xdbe873),
        (0xa62a5c, 0xffd5e4),
        (0x3b47b5, 0xdadeff),
        (0x4d3d20, 0xf3d47f),
        (0x23604f, 0xc3f1df),
        (0x7c3030, 0xffd2c8),
    ];
    let hash = key
        .to_lowercase()
        .bytes()
        .fold(0x811c_9dc5_u32, |hash, byte| (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193));
    let (fill, ink) = TINTS[hash as usize % TINTS.len()];
    (rgb(fill).into(), rgb(ink).into())
}

fn initial(name: &str) -> String {
    name.trim()
        .chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_else(|| "?".into())
}

/// Someone's letter on their tint: a circle, or a rounded tile (`radius`).
pub(crate) fn letter(name: &str, key: &str, size: f32, radius: Option<f32>) -> Div {
    let (fill, ink) = tint(key);
    div()
        .size(px(size))
        .flex_shrink_0()
        .map(|face| match radius {
            Some(radius) => face.rounded(px(radius)),
            None => face.rounded_full(),
        })
        .bg(fill)
        .flex()
        .items_center()
        .justify_center()
        .font_family(DISPLAY)
        .text_size(px((size * 0.52).round()))
        .line_height(px(size))
        .text_color(ink)
        .child(initial(name))
}

/// A member's picture when their account has one that has loaded.
fn picture(member: &TeamSpaceMemberDTO, cx: &mut App) -> Option<Arc<RenderImage>> {
    use meeting_notes_api_types::{AuthSessionDTO, UserDTO, UserDtoRole};
    let image = member.image.clone()?;
    crate::portrait::of(
        &AuthSessionDTO {
            expires_at: None,
            user: UserDTO {
                created_at: String::new(),
                email: member.email.clone(),
                id: member.user_id.clone(),
                image: Some(image),
                name: member.name.clone(),
                role: UserDtoRole::User,
            },
        },
        cx,
    )
}

/// A member's face: their picture (always round, drawn by `portrait`), else
/// their letter on a circle or a rounded tile (`radius`).
fn face(member: &TeamSpaceMemberDTO, size: f32, radius: Option<f32>, cx: &mut App) -> Div {
    let key = if member.email.is_empty() { &member.name } else { &member.email };
    crate::portrait::avatar(picture(member, cx), size, letter(&member.name, key, size, radius))
}

/// One of the page's capsule tabs. The selected one's fill is the page's
/// travelling highlight (`glide("team-tabs")`), never a fill of its own.
fn tab_pill(id: &'static str, label: &'static str, symbol: &str, selected: bool, t: Theme) -> Stateful<Div> {
    let ink = if selected { t.text } else { t.muted };
    div()
        .id(id)
        .h(px(size::TAB_HEIGHT))
        .px(px(11.))
        .rounded_full()
        .border_1()
        .border_color(t.hairline)
        .flex()
        .items_center()
        .gap(px(6.))
        .text_size(px(12.))
        .text_color(ink)
        .cursor_default()
        .child(crate::motion::tactile(t.hover).round())
        .child(icon(symbol, 13., if selected { t.secondary } else { t.muted }))
        .child(label)
        .reach()
}

/// A 26 pt outlined circle holding one glyph, beside "Add notes".
fn round_button(id: &'static str, symbol: &str, label: &'static str, active: bool, t: Theme) -> Stateful<Div> {
    round_tool(id, symbol, label, t)
        .size(px(size::TAB_HEIGHT))
        .border_1()
        .border_color(t.hairline)
        .when(active, |button| button.bg(t.hover))
}

thread_local! {
    /// What the lists showed of the rows that have left them, drawn while
    /// their space closes.
    static NOTES_LEFT: std::cell::RefCell<crate::lazy::Leavers<KeptNote>> =
        std::cell::RefCell::new(Default::default());
    static PEOPLE_LEFT: std::cell::RefCell<crate::lazy::Leavers<TeamSpacePersonDTO>> =
        std::cell::RefCell::new(Default::default());
    /// What was ticked in the "Add notes" sheet when it was last open: it
    /// leaves showing what it showed.
    static LAST_PICKED: std::cell::RefCell<Vec<u64>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// A caption or a row of the notes list that has left it.
#[derive(Clone)]
enum KeptNote {
    Day(String),
    Note(TeamSpaceNoteDTO),
}

impl Cereal {
    /// The space's name as the sidebar and the page say it.
    pub fn team_space_name(&self) -> String {
        match &self.team.page {
            Some(page) => space_name(&page.organization.name),
            None => space_name(&self.workspace.settings.team_space_name),
        }
    }

    /// Whether there is a team space to offer: a server to ask, or a page
    /// already in hand.
    pub fn team_space_offered(&self) -> bool {
        self.team.page.is_some() || crate::server::enabled()
    }

    /// The sidebar's row for the space, under "My notes" and drawn like it.
    pub fn team_space_row(&self, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        if !self.team_space_offered() {
            return None;
        }
        let t = self.theme();
        let selected = matches!(self.screen, Screen::TeamSpace | Screen::TeamNote)
            && self.current_note.is_none();
        Some(
            div()
                .id("Team space")
                .h(px(28.))
                .px(px(4.))
                .flex()
                .items_center()
                .gap(px(6.))
                .rounded(px(8.))
                .text_color(t.secondary)
                .cursor_default()
                .child(row_feedback(t, 8.))
                .when(selected, |row| {
                    row.child(self.stage.glide("sidebar").probe("Team space"))
                })
                // The same tile as every row under Spaces (`chrome.rs`).
                .child(self.space_tile("users"))
                .child(div().flex_1().min_w_0().truncate().child(self.team_space_name()))
                .reach()
                .on_click(cx.listener(|this, _, _, cx| this.navigate(Screen::TeamSpace, cx))),
        )
    }

    /// Read the page again from the default account's server. Without a
    /// server (the offline preview) the page is whatever was put there.
    pub fn load_team_space(&mut self, cx: &mut Context<Self>) {
        if !crate::server::enabled() || self.team.loading {
            return;
        }
        let Some(account) =
            crate::accounts::default_account().filter(|account| account.shown() && account.is_ready())
        else {
            return;
        };
        if self.team.account.as_ref() != Some(&account.id) {
            // Another account's space is not this one's.
            self.team.page = None;
            self.team.person = None;
            self.team.picker = None;
        }
        self.team.loading = true;
        self.team.problem = None;
        self.team.epoch = self.team.epoch.wrapping_add(1);
        let (epoch, seen, id) = (self.team.epoch, account.epoch(), account.id.clone());
        cx.spawn(async move |this, cx| {
            let with = id.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    crate::accounts::client(&with)?.request::<TeamSpaceDTO>("GET", "/api/v1/team-space", None)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.team.epoch != epoch {
                    return;
                }
                this.team.loading = false;
                if crate::accounts::epoch_of(&id) != Some(seen) {
                    return;
                }
                match result {
                    Ok(page) => this.accept_team_space(id, page, cx),
                    Err(error) => this.team.problem = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn accept_team_space(&mut self, account: AccountId, page: TeamSpaceDTO, cx: &mut Context<Self>) {
        // The sidebar names the space before the server has answered.
        if self.workspace.settings.team_space_name != page.organization.name {
            self.workspace.settings.team_space_name = page.organization.name.clone();
            self.persist(cx);
        }
        if let Some(person) = &self.team.person {
            let key = (person.email.clone(), person.name.clone());
            self.team.person = page
                .people
                .iter()
                .find(|listed| (listed.email.clone(), listed.name.clone()) == key)
                .cloned();
        }
        self.team.account = Some(account);
        self.team.problem = None;
        self.team.page = Some(page);
    }

    /// Notes of one's own that could be added: on the space's server, not in
    /// Trash, not already in the space, newest first.
    fn team_candidates(&self, cx: &App) -> Vec<(u64, String, String)> {
        let query = self.team.picker_search.read(cx).content().trim().to_lowercase();
        let already: Vec<&str> = self
            .team
            .page
            .iter()
            .flat_map(|page| page.notes.iter().map(|note| note.id.as_str()))
            .collect();
        let offline = !crate::server::enabled();
        let mut notes: Vec<_> = self
            .workspace
            .notes
            .iter()
            .filter(|note| !note.trashed && note.server_missing_since.is_none())
            .filter(|note| {
                offline
                    || (note.server_id.is_some()
                        && crate::accounts::of_note(note.id).map(|account| account.id) == self.team.account)
            })
            .filter(|note| {
                !note
                    .server_id
                    .as_deref()
                    .is_some_and(|id| already.iter().any(|listed| listed.eq_ignore_ascii_case(id)))
            })
            .filter(|note| query.is_empty() || note.title.to_lowercase().contains(&query))
            .collect();
        notes.sort_by(|a, b| b.date.cmp(&a.date).then_with(|| b.id.cmp(&a.id)));
        notes
            .into_iter()
            .map(|note| {
                let title = if note.title.trim().is_empty() { "New note".into() } else { note.title.clone() };
                (note.id, title, note.date.clone())
            })
            .collect()
    }

    pub fn open_team_picker(&mut self, cx: &mut Context<Self>) {
        self.team.picker = Some(Picker::default());
        self.team.picker_search.update(cx, |input, cx| input.set_content("", cx));
        self.team.picker_scroll.set_offset(Point::default());
        cx.notify();
    }

    fn toggle_team_pick(&mut self, id: u64, cx: &mut Context<Self>) {
        if let Some(picker) = &mut self.team.picker {
            match picker.selected.iter().position(|picked| *picked == id) {
                Some(at) => {
                    picker.selected.remove(at);
                }
                None => picker.selected.push(id),
            }
        }
        cx.notify();
    }

    /// Put the ticked notes in the space. The server answers with the page as
    /// it now is.
    fn add_team_picks(&mut self, cx: &mut Context<Self>) {
        let Some(picker) = &mut self.team.picker else { return };
        if picker.sending || picker.selected.is_empty() {
            return;
        }
        let ids: Vec<String> = picker
            .selected
            .iter()
            .filter_map(|id| self.workspace.note(*id).and_then(|note| note.server_id.clone()))
            .collect();
        let Some(account) = self.team.account.clone().filter(|_| crate::server::enabled() && !ids.is_empty()) else {
            self.team.picker = None;
            cx.notify();
            return;
        };
        picker.sending = true;
        self.team.epoch = self.team.epoch.wrapping_add(1);
        let epoch = self.team.epoch;
        self.team.loading = false;
        let count = ids.len();
        cx.spawn(async move |this, cx| {
            let with = account.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    crate::accounts::client(&with)?.request::<TeamSpaceDTO>(
                        "POST",
                        "/api/v1/team-space/notes",
                        Some(serde_json::json!({ "noteIds": ids })),
                    )
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(page) => {
                        if this.team.epoch == epoch {
                            this.accept_team_space(account, page, cx);
                        }
                        this.team.picker = None;
                        this.team.tab = Tab::Notes;
                        this.team.person = None;
                        let name = this.team_space_name();
                        this.show_toast(
                            &format!("Added {count} note{} to {name}", if count == 1 { "" } else { "s" }),
                            cx,
                        );
                    }
                    Err(error) => {
                        if let Some(picker) = &mut this.team.picker {
                            picker.sending = false;
                        }
                        this.show_toast(&error, cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Take a note out of the space. It leaves the list at once; the server
    /// is what ends everyone's access to it.
    pub fn remove_from_team_space(&mut self, server_id: String, cx: &mut Context<Self>) {
        let listed = self.team.page.as_mut().is_some_and(|page| {
            let before = page.notes.len();
            page.notes.retain(|note| !note.id.eq_ignore_ascii_case(&server_id));
            page.notes.len() != before
        });
        cx.notify();
        if !crate::server::enabled() {
            return;
        }
        let Some(account) = self
            .team
            .account
            .clone()
            .or_else(|| crate::accounts::default_account().map(|account| account.id))
        else {
            return;
        };
        let Ok(id) = crate::accounts::client::path_id(&server_id) else { return };
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    crate::accounts::client(&account)?.request::<serde_json::Value>(
                        "DELETE",
                        &format!("/api/v1/team-space/notes/{id}"),
                        None,
                    )
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                // A note that was never in the space answers 404: only a row
                // the page showed is worth a word.
                if listed {
                    match result {
                        Ok(_) => this.show_toast("Removed from the team space", cx),
                        Err(error) => this.show_toast(&error, cx),
                    }
                    this.load_team_space(cx);
                }
            });
        })
        .detach();
    }

    /// A note went to Trash: it is no longer shown to the team. The server
    /// has no Trash of its own, so the app takes the note out of the space.
    pub fn team_space_note_trashed(&mut self, id: u64, cx: &mut Context<Self>) {
        if !crate::server::enabled() {
            return;
        }
        let Some(server_id) = self.workspace.note(id).and_then(|note| note.server_id.clone()) else {
            return;
        };
        // Only where a space could hold it: the page's own account, or, when
        // the page was never opened, the account whose space it would be.
        let account = crate::accounts::of_note(id).map(|account| account.id);
        let space = self
            .team
            .account
            .clone()
            .or_else(|| crate::accounts::default_account().map(|account| account.id));
        if account.is_some() && account == space {
            self.remove_from_team_space(server_id, cx);
        }
    }

    /// Open a row: one's own note as itself, anyone else's read-only.
    fn open_team_note(&mut self, server_id: String, mine: bool, cx: &mut Context<Self>) {
        if !crate::server::enabled() {
            return;
        }
        if mine && let Some(account) = self.team.account.clone() {
            self.open_server_note(account, server_id, cx);
            return;
        }
        self.navigate(Screen::TeamNote, cx);
        self.team.reading = Some(Reading { server_id, phase: Phase::Loading });
        self.note_scroll.set_offset(point(px(0.), px(0.)));
        self.read_team_note(cx);
    }

    fn read_team_note(&mut self, cx: &mut Context<Self>) {
        let Some(reading) = &mut self.team.reading else { return };
        reading.phase = Phase::Loading;
        let server_id = reading.server_id.clone();
        let (Some(account), Ok(id)) = (self.team.account.clone(), crate::accounts::client::path_id(&server_id)) else {
            reading.phase = Phase::Failed(SharedNoteError::Unavailable);
            return;
        };
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result: Result<SharedNote, SharedNoteError> = cx
                .background_executor()
                .spawn(async move {
                    let client = crate::accounts::client(&account).map_err(|_| SharedNoteError::SignInRequired)?;
                    let reply = client
                        .fetch(&format!("/api/v1/team-space/notes/{id}"), crate::shared::MAXIMUM_BYTES)
                        .map_err(|failure| match failure {
                            crate::fetch::Failure::TooLarge => SharedNoteError::TooLarge,
                            crate::fetch::Failure::TimedOut => SharedNoteError::TimedOut,
                            _ => SharedNoteError::Unreachable,
                        })?;
                    crate::shared::read(&reply)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.team.reading.as_ref().is_none_or(|reading| reading.server_id != server_id) {
                    return;
                }
                let phase = match result {
                    Ok(note) => {
                        this.show_read_only_note(&note, cx);
                        Phase::Loaded(note)
                    }
                    Err(error) => Phase::Failed(error),
                };
                if let Some(reading) = &mut this.team.reading {
                    reading.phase = phase;
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Someone else's note from the space, on the page a shared note uses.
    pub fn render_team_note(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let t = self.theme();
        match self.team.reading.as_ref().map(|reading| &reading.phase) {
            Some(Phase::Loaded(note)) => self.render_shared_note(note, &self.team_space_name(), window, cx),
            Some(Phase::Loading) => self.render_shared_loading(window),
            failed => {
                let (retry, message) = match failed {
                    Some(Phase::Failed(SharedNoteError::Unavailable)) => (
                        false,
                        "This note is no longer in the team space.".to_owned(),
                    ),
                    Some(Phase::Failed(error)) => (error.is_retryable(), error.message()),
                    _ => (false, "This note is not open.".to_owned()),
                };
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(14.))
                    .pb(px(80.))
                    .child(icon("users", 32., t.muted))
                    .child(heading("This note could not be opened", t))
                    .child(small(message, t))
                    .when(retry, |page| {
                        page.child(pill("team-note-retry", "Retry", "refresh", t).on_click(cx.listener(
                            |this, _, window, cx| {
                                this.focus.focus(window);
                                this.read_team_note(cx);
                            },
                        )))
                    })
                    .into_any_element()
            }
        }
    }

    pub fn render_team_space(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let page = f32::from(window.viewport_size().width) - self.sidebar_inset() - 10.;
        let column = div()
            .w(px(page.clamp(240., size::COLUMN)))
            .px(px(size::COLUMN_PAD))
            .pb(px(110.))
            .flex()
            .flex_col()
            .flex_shrink_0()
            .child(self.team_hero(window, cx))
            .child(self.team_toolbar(cx))
            // The tab's content firms up when the tab is switched, or when
            // the page it was waiting for arrives.
            .child(crate::motion::crossfade(
                (
                    "team-body",
                    crate::lazy::key((
                        self.team.tab == Tab::People,
                        self.team.page.is_some(),
                        self.team.problem.is_some(),
                    )),
                ),
                match self.team.tab {
                    Tab::Notes => self.team_notes(cx),
                    Tab::People => self.team_people(cx),
                },
            ));
        let picker = self.stage.presence(
            ("team-picker", 0),
            self.team.picker.is_some(),
            crate::motion::GOOEY,
        );
        div()
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_hidden()
            .child(
                div()
                    .id("team-scroll")
                    .track_scroll(&self.team.scroll)
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .pr(px(10.))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(column),
            )
            .child(scroll_thumb("team-space", &self.team.scroll))
            .when(picker.visible, |root| root.child(self.team_picker(picker, cx)))
            .into_any_element()
    }

    /// The centred opening: the members' faces, the space's tile and name,
    /// the banner until it is closed, and the composer.
    fn team_hero(&self, window: &Window, cx: &mut Context<Self>) -> Div {
        let t = self.theme();
        let dark = self.workspace.settings.dark_mode;
        let members: &[TeamSpaceMemberDTO] = self.team.page.as_ref().map_or(&[], |page| &page.members);
        let extra = members.len().saturating_sub(size::FACES_SHOWN);
        let mut faces = div().h(px(size::FACE)).flex().items_center().justify_center();
        for (index, member) in members.iter().take(size::FACES_SHOWN).enumerate() {
            faces = faces.child(
                div()
                    .when(index > 0, |ring| ring.ml(px(-size::FACE_OVERLAP)))
                    .rounded_full()
                    .border_2()
                    .border_color(t.bg)
                    .child(face(member, size::FACE - 4., None, cx)),
            );
        }
        if extra > 0 {
            faces = faces.child(
                div()
                    .ml(px(-size::FACE_OVERLAP))
                    .rounded_full()
                    .border_2()
                    .border_color(t.bg)
                    .child(
                        div()
                            .size(px(size::FACE - 4.))
                            .rounded_full()
                            .bg(t.card)
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(11.))
                            .text_color(t.muted)
                            .child(format!("+{extra}")),
                    ),
            );
        }
        let focused = self.ask.read(cx).focus_handle.is_focused(window);
        let can_send = !self.ask.read(cx).content().trim().is_empty();
        // The banner is the gold of the app's accent, quiet enough to read on.
        let (fill, edge): (Hsla, Hsla) = if dark {
            (rgb(0x35321f).into(), rgb(0x4d4830).into())
        } else {
            (rgb(0xf8f1d9).into(), rgb(0xe6dbb4).into())
        };
        div()
            .w_full()
            .flex()
            .flex_col()
            .items_center()
            .pt(px(size::FACES_TOP))
            .child(faces)
            .child(
                div()
                    .mt(px(size::FACES_TO_TILE))
                    .size(px(28.))
                    .rounded(px(7.))
                    .border_1()
                    .border_color(t.hairline)
                    .bg(t.fill)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("users", 14., t.secondary)),
            )
            .child(
                div()
                    .mt(px(10.))
                    .w_full()
                    .text_center()
                    .font_family(DISPLAY)
                    .text_size(px(35.))
                    .line_height(px(44.))
                    .text_color(t.text)
                    .child(self.team_space_name()),
            )
            .child(
                div()
                    .mt(px(1.))
                    .text_size(px(13.))
                    .line_height(px(18.))
                    .text_color(t.muted)
                    .child(SUBTITLE),
            )
            .child(
                div()
                    .mt(px(9.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(6.))
                    .text_size(px(13.))
                    .line_height(px(18.))
                    .text_color(t.muted)
                    .child(icon("users", 13., t.muted))
                    .child("Your team workspace"),
            )
            // Closed, the banner fades while the space it took (its own
            // height and the 40 pt above it) closes up under it.
            .children(crate::motion::appear_on_page(
                &self.stage,
                ("team-banner", 0),
                !self.workspace.settings.team_banner_dismissed,
                div()
                    .mt(px(40.))
                    .w_full()
                    .px(px(16.))
                    .pt(px(14.))
                    .pb(px(17.))
                    .rounded(px(12.))
                    .border_1()
                    .border_color(edge)
                    .bg(fill)
                    .flex()
                    .items_start()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(BANNER_TITLE),
                            )
                            .child(
                                div()
                                    .max_w(px(520.))
                                    .text_size(px(13.))
                                    .line_height(px(19.))
                                    .text_color(t.secondary)
                                    .child(BANNER_TEXT),
                            ),
                    )
                    .child(
                        tool_tip("dismiss-team-banner", "close", "Dismiss", t)
                            .size(px(22.))
                            .mt(px(-6.))
                            .mr(px(-6.))
                            .on_click(cx.listener(|this, _, _, cx| this.dismiss_team_banner(cx))),
                    ),
            ))
            .child(
                // The composer sits 16 px in from the column.
                div().mt(px(56.)).w_full().px(px(16.)).child(
                    div()
                        .w_full()
                        .max_w(px(680.))
                        .h(px(57.))
                        .rounded_full()
                        .bg(t.card)
                        .border_1()
                        .border_color(if focused { t.accent } else { t.border })
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .pl(px(20.))
                        .pr(px(10.))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(px(14.))
                                .line_height(px(22.))
                                .child(self.ask.clone()),
                        )
                        .child(
                            crate::ui::send_button("team-ask-send", "Send · Enter", can_send, t)
                                .on_click(cx.listener(|this, _, _, cx| this.submit_chat(cx))),
                        ),
                ),
            )
            .child(div().mt(px(41.)).w_full().px(px(8.)).child(separator(t)))
    }

    /// Close the banner for good on this Mac.
    pub fn dismiss_team_banner(&mut self, cx: &mut Context<Self>) {
        self.workspace.settings.team_banner_dismissed = true;
        self.persist(cx);
        cx.notify();
    }

    /// The tabs, and at the trailing edge what can be done on the one shown.
    fn team_toolbar(&self, cx: &mut Context<Self>) -> Div {
        let t = self.theme();
        let people = self.team.tab == Tab::People;
        let searching = people && self.team.people_search_open;
        // The selected tab's fill travels from one pill to the other; what
        // belongs to People (its search and its order) grows in beside
        // "Add notes" and goes the way it came.
        let glide = self.stage.glide("team-tabs");
        let tools = self.stage.presence_on_page(("team-people-tools", 0), people, crate::motion::SNAPPY);
        let field = self.stage.presence_on_page(("team-people-search", 0), searching, crate::motion::SNAPPY);
        let from_trailing = crate::motion::Pop::chip((1., 0.5));
        div()
            .relative()
            .mt(px(55.))
            .mb(px(9.))
            .w_full()
            .h(px(size::TAB_HEIGHT))
            .flex()
            .items_center()
            .gap(px(4.))
            .child(glide.indicator(t.hover, size::TAB_HEIGHT / 2.))
            .child(
                tab_pill("team-tab-notes", "Notes", "notes-lines", !people, t)
                    .when(!people, |tab| tab.child(glide.probe("notes")))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.focus.focus(window);
                        this.team.tab = Tab::Notes;
                        cx.notify();
                    })),
            )
            .child(
                tab_pill("team-tab-people", "People", "person-outline", people, t)
                    .when(people, |tab| tab.child(glide.probe("people")))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.focus.focus(window);
                        this.team.tab = Tab::People;
                        this.team.person = None;
                        cx.notify();
                    })),
            )
            .child(div().flex_1())
            .when(field.visible, |bar| {
                bar.child(crate::motion::pop(
                    div()
                        .w(px(180.))
                        .h(px(size::TAB_HEIGHT))
                        .px(px(10.))
                        .rounded_full()
                        .border_1()
                        .border_color(t.hairline)
                        .flex()
                        .items_center()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .child(div().flex_1().min_w_0().child(self.team.people_search.clone())),
                    from_trailing,
                    field,
                ))
            })
            .when(tools.visible, |bar| {
                bar.child(crate::motion::pop(
                    round_button("team-people-search", "search", "Search people", searching, t).on_click(
                        cx.listener(|this, _, window, cx| {
                            if this.team.tab != Tab::People {
                                return;
                            }
                            this.team.people_search_open = !this.team.people_search_open;
                            if this.team.people_search_open {
                                this.team.people_search.read(cx).focus_handle.focus(window);
                            } else {
                                this.team.people_search.update(cx, |input, cx| input.set_content("", cx));
                                this.focus.focus(window);
                            }
                            cx.notify();
                        }),
                    ),
                    from_trailing,
                    tools,
                ))
                .child(crate::motion::pop(
                    round_button(
                        "team-people-sort",
                        "sliders-horizontal",
                        if self.team.oldest_first { "Oldest note first" } else { "Newest note first" },
                        false,
                        t,
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        if this.team.tab != Tab::People {
                            return;
                        }
                        this.focus.focus(window);
                        this.team.oldest_first = !this.team.oldest_first;
                        cx.notify();
                    })),
                    from_trailing,
                    tools,
                ))
            })
            .child(
                pill("team-add-notes", "Add notes", "plus-small", t)
                    .h(px(size::TAB_HEIGHT))
                    .px(px(11.))
                    .text_size(px(12.))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_team_picker(cx);
                        // What is typed next searches one's notes.
                        this.team.picker_search.read(cx).focus_handle.focus(window);
                    })),
            )
    }

    /// A centred note for a list with nothing in it.
    fn team_empty(&self, symbol: &str, title: &str, text: String, action: Option<Stateful<Div>>) -> Div {
        let t = self.theme();
        div()
            .w_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .py(px(56.))
            .gap(px(12.))
            .child(icon(symbol, 30., t.muted))
            .child(heading(title.to_owned(), t))
            .child(small(text, t).max_w(px(380.)).text_center())
            .children(action.map(|action| div().mt(px(4.)).child(action)))
    }

    /// What stands in for the lists while there is no page: why, and what to do.
    fn team_unavailable(&self, cx: &mut Context<Self>) -> Option<Div> {
        if self.team.page.is_some() {
            return None;
        }
        let t = self.theme();
        Some(if !crate::server::enabled() {
            self.team_empty(
                "users",
                "Sign in to share with your team",
                "The team space lives on your server. Connect this Mac to it to see what your team has shared.".into(),
                None,
            )
        } else if let Some(problem) = &self.team.problem {
            self.team_empty(
                "users",
                "The team space could not be loaded",
                problem.clone(),
                Some(pill("team-retry", "Retry", "refresh", t).on_click(cx.listener(|this, _, window, cx| {
                    this.focus.focus(window);
                    this.load_team_space(cx);
                }))),
            )
        } else {
            div()
                .w_full()
                .py(px(70.))
                .flex()
                .justify_center()
                .child(small("Loading the team space…", t))
        })
    }

    fn team_notes(&self, cx: &mut Context<Self>) -> Div {
        let t = self.theme();
        if let Some(unavailable) = self.team_unavailable(cx) {
            return unavailable;
        }
        let Some(page) = &self.team.page else { return div() };
        let person = self.team.person.as_ref();
        let notes: Vec<&TeamSpaceNoteDTO> = page
            .notes
            .iter()
            .filter(|note| person.is_none_or(|person| person.note_ids.iter().any(|id| id.eq_ignore_ascii_case(&note.id))))
            .collect();
        let mut list = div().w_full().flex().flex_col();
        if let Some(person) = person {
            list = list.child(
                div().mt(px(14.)).px(px(8.)).flex().items_center().gap(px(8.)).child(
                    div()
                        .id("team-person-filter")
                        .relative()
                        .h(px(24.))
                        .pl(px(10.))
                        .pr(px(6.))
                        .rounded_full()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .text_size(px(12.))
                        .text_color(t.text)
                        .cursor_default()
                        .child(crate::motion::tactile(neutral_wash(0.14)).base(t.fill).round())
                        .child(format!("Notes with {}", person.name))
                        .child(icon("close", 11., t.muted))
                        .reach()
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.focus.focus(window);
                            this.team.person = None;
                            cx.notify();
                        })),
                ),
            );
        }
        // A note taken out of the space folds shut and the rows under it
        // close up, its day's caption with it when it was the day's last.
        let mut entries: Vec<(u64, KeptNote)> = Vec::with_capacity(notes.len() + 8);
        let mut day = "";
        // (A day that comes up again further down is another caption.)
        let mut captions: std::collections::HashMap<&str, u32> = Default::default();
        for note in &notes {
            if note.date != day {
                day = &note.date;
                let again = captions.entry(day).or_default();
                entries.push((crate::lazy::key(("team-day", day, *again)), KeptNote::Day(day.to_owned())));
                *again += 1;
            }
            entries.push((crate::lazy::key(("team-note", &note.id)), KeptNote::Note((*note).clone())));
        }
        let keys: Vec<u64> = entries.iter().map(|(key, _)| *key).collect();
        let scope = crate::lazy::key(person.map(|person| (&person.email, &person.name)));
        NOTES_LEFT.with(|left| left.borrow_mut().update(crate::lazy::key((&keys, scope)), || entries.clone()));
        let drawn = self.stage.roster("team-notes", scope, &keys);
        let leaving: Vec<u64> = drawn.iter().filter(|row| !row.present).map(|row| row.key).collect();
        NOTES_LEFT.with(|left| left.borrow_mut().keep(&leaving));
        let mut listed: std::collections::HashMap<u64, KeptNote> = entries.into_iter().collect();
        for row in &drawn {
            let (kept, live) = match listed.remove(&row.key) {
                Some(kept) if row.present => (Some(kept), true),
                _ => (NOTES_LEFT.with(|left| left.borrow().left(row.key).cloned()), false),
            };
            let content = kept.map(|kept| match kept {
                KeptNote::Day(day) => caption(crate::home::date_label(&day), t)
                    .px(px(8.))
                    .pt(px(25.))
                    .pb(px(2.))
                    .into_any_element(),
                KeptNote::Note(note) => self.team_note_row(&note, live, cx).into_any_element(),
            });
            list = list.children(crate::motion::roster_row(&self.stage, "team-notes", *row, content));
        }
        // The empty space says so once its last note has gone.
        let organization = page.organization.name.trim();
        let everyone = if organization.is_empty() { "your workspace".to_owned() } else { organization.to_owned() };
        list.children(crate::motion::appear_on_page(
            &self.stage,
            ("team-notes-empty", 0),
            notes.is_empty() && drawn.is_empty(),
            self.team_empty(
                "users",
                if person.is_some() { "No notes with them here" } else { "Nothing shared with the team yet" },
                format!("Add a note and everyone in {everyone} can read it. It stays yours: only you can change it."),
                Some(pill("team-empty-add", "Add notes", "plus-small", t).on_click(cx.listener(
                    |this, _, window, cx| {
                        this.open_team_picker(cx);
                        this.team.picker_search.read(cx).focus_handle.focus(window);
                    },
                ))),
            ),
        ))
    }

    /// A note in the space: its owner's face, its title over who it was
    /// with, and when. Under the pointer the time gives way to "remove" for
    /// those who may.
    ///
    /// `live` is false for a row that has left the list and is only closing
    /// up: it answers nothing.
    fn team_note_row(&self, note: &TeamSpaceNoteDTO, live: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        let t = self.theme();
        let key = crate::lazy::key(("team-note", &note.id));
        let hovered = crate::motion::reveal_key(("team-note", key, live));
        let names: Vec<String> = note.attendees.people.iter().map(|person| person.short_name.clone()).collect();
        let subtitle = attendee_line(&names, usize::try_from(note.attendees.others).unwrap_or(0), &note.owner.name);
        let title = if note.title.trim().is_empty() { "New note".to_owned() } else { note.title.clone() };
        let time = row_time(note);
        let (id, mine) = (note.id.clone(), note.mine);
        let removed = note.id.clone();
        div()
            .id((if live { "team-note" } else { "team-note-left" }, key))
            .relative()
            .w_full()
            .h(px(44.))
            .mb(px(4.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(16.))
            .px(px(8.))
            .rounded(px(12.))
            .cursor_default()
            .map(|row| crate::motion::lift(row, self.stage.cascade_next("page")))
            .when(live, |row| row.child(row_feedback(t, 12.).reveals(hovered)).reach())
            .child(face(&note.owner, 28., Some(6.), cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(div().text_size(px(14.)).line_height(px(20.)).truncate().child(title))
                    .child(small(subtitle, t).truncate()),
            )
            .child(
                div()
                    .relative()
                    .flex_shrink_0()
                    .min_w(px(48.))
                    .h(px(24.))
                    .flex()
                    .items_center()
                    .justify_end()
                    .map(|trailing| {
                        if note.can_remove && live {
                            trailing
                                .child(crate::motion::conceal(time_label(&time, t), hovered))
                                .child(
                                    crate::motion::reveal(div(), hovered).absolute().top_0().right_0().child(
                                        tool_tip(("team-note-remove", key), "close", "Remove from team space", t)
                                            .size(px(24.))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                cx.stop_propagation();
                                                this.remove_from_team_space(removed.clone(), cx);
                                            })),
                                    ),
                                )
                        } else {
                            trailing.child(time_label(&time, t))
                        }
                    }),
            )
            .when(live, |row| {
                row.on_click(cx.listener(move |this, _, _, cx| this.open_team_note(id.clone(), mine, cx)))
            })
    }

    fn team_people(&self, cx: &mut Context<Self>) -> Div {
        let t = self.theme();
        if let Some(unavailable) = self.team_unavailable(cx) {
            return unavailable;
        }
        let Some(page) = &self.team.page else { return div() };
        let query = self.team.people_search.read(cx).content();
        let people = listed_people(&page.people, &query, self.team.oldest_first);
        if page.people.is_empty() {
            return self.team_empty(
                "person-outline",
                "No one here yet",
                "Everyone you and your team meet with appears here, once a note with its meeting's attendees is in the team space.".into(),
                None,
            );
        }
        let own = page.people_source == meeting_notes_api_types::PeopleSource::Own;
        let today = Local::now().date_naive();
        let head = |label: &'static str| {
            div()
                .text_size(px(12.))
                .line_height(px(16.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(t.secondary)
                .child(label)
        };
        let mut table = div().w_full().flex().flex_col().when(own, |table| {
            table.child(
                small("People from your own notes. Add notes to the team space to build the team's list.", t)
                    .px(px(8.))
                    .pt(px(12.)),
            )
        });
        table = table.child(
            div()
                .h(px(34.))
                .mt(px(if own { 6. } else { 14. }))
                .px(px(8.))
                .flex()
                .items_center()
                .child(head("Person").flex_1().min_w_0())
                .child(head("Email").w(px(size::EMAIL_COLUMN)).flex_shrink_0())
                .child(head("Notes").w(px(size::COUNT_COLUMN)).flex_shrink_0())
                .child(
                    div()
                        .id("team-people-last-note")
                        .relative()
                        .w(px(size::DATE_COLUMN))
                        .h(px(24.))
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .gap(px(5.))
                        .rounded(px(6.))
                        .cursor_default()
                        // The wash reaches 6 pt past the words on each side.
                        .child(
                            div()
                                .absolute()
                                .top_0()
                                .bottom_0()
                                .left(px(-6.))
                                .right(px(6.))
                                .child(crate::motion::tactile(t.hover).radius(6.)),
                        )
                        .child(head("Last note"))
                        .child(crate::motion::swapped(
                            icon(if self.team.oldest_first { "arrow-up" } else { "arrow-down" }, 11., t.muted),
                            self.stage.swap(("team-people-order", 0), u64::from(self.team.oldest_first)),
                        ))
                        .reach()
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.focus.focus(window);
                            this.team.oldest_first = !this.team.oldest_first;
                            cx.notify();
                        })),
                ),
        );
        // Rows that a search leaves out fold shut, and open again when it
        // lets them back; another order is another list.
        let key_of = |person: &TeamSpacePersonDTO| crate::lazy::key(("team-person", &person.email, &person.name));
        let keys: Vec<u64> = people.iter().map(|person| key_of(person)).collect();
        let scope = u64::from(self.team.oldest_first);
        PEOPLE_LEFT.with(|left| {
            left.borrow_mut().update(crate::lazy::key((&keys, scope)), || {
                people.iter().map(|person| (key_of(person), (*person).clone())).collect()
            })
        });
        let drawn = self.stage.roster("team-people", scope, &keys);
        let leaving: Vec<u64> = drawn.iter().filter(|row| !row.present).map(|row| row.key).collect();
        PEOPLE_LEFT.with(|left| left.borrow_mut().keep(&leaving));
        let mut listed: std::collections::HashMap<u64, &TeamSpacePersonDTO> =
            keys.iter().copied().zip(people.iter().copied()).collect();
        let nobody = people.is_empty() && drawn.is_empty();
        for row in drawn {
            let (person, live) = match listed.remove(&row.key) {
                Some(person) if row.present => (person.clone(), true),
                _ => match PEOPLE_LEFT.with(|left| left.borrow().left(row.key).cloned()) {
                    Some(person) => (person, false),
                    None => {
                        table = table.children(crate::motion::roster_row(&self.stage, "team-people", row, None::<Div>));
                        continue;
                    }
                },
            };
            let person = &person;
            let key = row.key;
            let tint_key = person.email.clone().unwrap_or_else(|| person.name.clone());
            let email = person.email.clone().unwrap_or_default();
            let chosen = person.clone();
            table = table.children(crate::motion::roster_row(
                &self.stage,
                "team-people",
                row,
                Some(
                div()
                    .id((if live { "team-person" } else { "team-person-left" }, key))
                    .relative()
                    .w_full()
                    .h(px(size::PERSON_ROW - 4.))
                    .mb(px(4.))
                    .flex_shrink_0()
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .rounded(px(12.))
                    .cursor_default()
                    .map(|row| crate::motion::lift(row, self.stage.cascade_next("page")))
                    .when(live, |row| row.child(row_feedback(t, 12.)).reach())
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .pr(px(12.))
                            .child(letter(&person.name, &tint_key, size::PERSON_FACE, None))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .child(div().text_size(px(14.)).line_height(px(19.)).truncate().child(person.name.clone()))
                                    .child(
                                        div()
                                            .text_size(px(12.5))
                                            .line_height(px(16.))
                                            .text_color(t.muted)
                                            .truncate()
                                            .child(email.clone()),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .w(px(size::EMAIL_COLUMN))
                            .flex_shrink_0()
                            .pr(px(12.))
                            .text_size(px(12.5))
                            .text_color(t.muted)
                            .truncate()
                            .child(email),
                    )
                    .child(
                        div()
                            .w(px(size::COUNT_COLUMN))
                            .flex_shrink_0()
                            .text_size(px(12.5))
                            .text_color(t.muted)
                            .child(person.notes.to_string()),
                    )
                    .child(
                        div()
                            .w(px(size::DATE_COLUMN))
                            .flex_shrink_0()
                            .text_size(px(12.5))
                            .text_color(t.muted)
                            .child(last_note_label(&person.last_note_date, today)),
                    )
                    .when(!own && live, |row| {
                        // Their notes, on the Notes tab.
                        row.on_click(cx.listener(move |this, _, window, cx| {
                            this.focus.focus(window);
                            this.team.person = Some(chosen.clone());
                            this.team.tab = Tab::Notes;
                            cx.notify();
                        }))
                    }),
                ),
            ));
        }
        table.children(crate::motion::appear_on_page(
            &self.stage,
            ("team-people-none", 0),
            nobody,
            div().py(px(40.)).flex().justify_center().child(small("No one matches that search.", t)),
        ))
    }

    /// The "Add notes" sheet: one's own notes, a search, and ticks.
    fn team_picker(&self, pose: crate::motion::Pose, cx: &mut Context<Self>) -> AnyElement {
        let t = self.theme();
        // Closed, it is drawn as it was for as long as it takes to leave.
        let open = self.team.picker.is_some();
        let closing = Picker { selected: LAST_PICKED.with(|last| last.borrow().clone()), sending: false };
        let picker = self.team.picker.as_ref().unwrap_or(&closing);
        if open {
            LAST_PICKED.with(|last| last.borrow_mut().clone_from(&picker.selected));
        }
        let candidates = self.team_candidates(cx);
        let count = picker.selected.len();
        let today = Local::now().date_naive();
        let organization = self.team.page.as_ref().map(|page| page.organization.name.trim().to_owned()).unwrap_or_default();
        let everyone = if organization.is_empty() { "your workspace".to_owned() } else { organization };
        let mut list = div()
            .id("team-picker-list")
            .track_scroll(&self.team.picker_scroll)
            .h(px(size::PICKER_LIST))
            .overflow_y_scroll()
            .px(px(8.))
            .py(px(6.))
            .flex()
            .flex_col();
        if candidates.is_empty() {
            let searching = !self.team.picker_search.read(cx).content().trim().is_empty();
            list = list.child(
                div().flex_1().flex().items_center().justify_center().child(small(
                    if searching { "No notes of yours match that search." } else { "Every note of yours is already in the team space." },
                    t,
                )),
            );
        }
        for (id, title, date) in candidates {
            let picked = picker.selected.contains(&id);
            list = list.child(
                div()
                    .id(("team-pick", id))
                    .h(px(36.))
                    .flex_shrink_0()
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .rounded(px(8.))
                    .cursor_default()
                    .child(row_feedback(t, 8.))
                    .reach()
                    .child(
                        div()
                            .size(px(16.))
                            .flex_shrink_0()
                            .rounded(px(4.5))
                            .border_1()
                            .border_color(if picked { t.accent } else { t.hairline })
                            .when(picked, |tick| tick.bg(t.accent))
                            .flex()
                            .items_center()
                            .justify_center()
                            // The check rises in as the box fills.
                            .when(picked, |tick| {
                                tick.child(crate::motion::swapped(
                                    icon("check", 11., t.bg),
                                    self.stage.swap(("team-pick", id), 1),
                                ))
                            })
                            .when(!picked, |tick| {
                                self.stage.swap(("team-pick", id), 0);
                                tick
                            }),
                    )
                    .child(div().flex_1().min_w_0().text_size(px(13.5)).truncate().child(title))
                    .child(small(last_note_label(&date, today), t).flex_shrink_0())
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle_team_pick(id, cx))),
            );
        }
        let add = if count == 0 {
            "Add notes".to_owned()
        } else {
            format!("Add {count} note{}", if count == 1 { "" } else { "s" })
        };
        let card = div()
            .id("team-picker")
            .w(px(size::PICKER_WIDTH))
            .max_w(relative(0.92))
            .rounded(px(14.))
            .bg(t.card)
            .border_1()
            .border_color(card_edge(t))
            .shadow_lg()
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(
                div()
                    .px(px(16.))
                    .pt(px(14.))
                    .pb(px(10.))
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight::MEDIUM)
                            .child(format!("Add notes to {}", self.team_space_name())),
                    )
                    .child(small(format!("Everyone in {everyone} will be able to read them. They stay yours."), t)),
            )
            .child(
                div()
                    .mx(px(12.))
                    .h(px(32.))
                    .px(px(10.))
                    .rounded(px(8.))
                    .bg(t.hover)
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_size(px(13.))
                    .line_height(px(18.))
                    .child(icon("search", 14., t.muted))
                    .child(div().flex_1().min_w_0().child(self.team.picker_search.clone())),
            )
            .child(list)
            .child(div().h(px(1.)).bg(t.hairline))
            .child(
                div()
                    .px(px(12.))
                    .py(px(10.))
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(px(8.))
                    .child(pill("team-picker-cancel", "Cancel", "", t).on_click(cx.listener(|this, _, window, cx| {
                        this.focus.focus(window);
                        this.team.picker = None;
                        cx.notify();
                    })))
                    .child(
                        pill("team-picker-add", add, "", t)
                            .bg(t.text)
                            .text_color(t.bg)
                            // It firms up with the first tick.
                            .opacity(
                                self.stage
                                    .aim(
                                        ("team-picker-add", 0),
                                        if count == 0 || picker.sending { DISABLED } else { 1. },
                                        crate::motion::SNAPPY,
                                    )
                                    .clamp(DISABLED, 1.),
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.focus.focus(window);
                                this.add_team_picks(cx);
                            })),
                    ),
            );
        // A dialog on the app's one overlay layer: the page behind it takes
        // no pointer, a press outside closes it and goes no further, and
        // Escape closes it (`Cereal::dismiss_top`).
        overlay(
            open,
            cx.listener(|this, _, window, cx| {
                this.team.picker = None;
                this.focus.focus(window);
                cx.notify();
            }),
        )
        .bg(scrim(Scrim::Dialog, pose.fade))
        .flex()
        .items_center()
        .justify_center()
        .child(crate::motion::pop(floating(card), crate::motion::Pop::DIALOG, pose))
        .into_any_element()
    }
}

/// Made-up people and notes for the layout stills (`CEREAL_TOUR=p-fix-team`):
/// nobody here exists.
#[allow(dead_code)]
pub fn tour_page(empty: bool) -> TeamSpaceDTO {
    use meeting_notes_api_types::{
        NoteAttendeeDTO, NoteAttendeesDTO, NoteMeetingTimeDTO, Organization, OrganizationDtoRole, PeopleSource,
    };
    let member = |name: &str| TeamSpaceMemberDTO {
        user_id: uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_DNS, name.as_bytes()).to_string(),
        name: name.to_owned(),
        email: format!("{}@example.test", name.split(' ').next().unwrap_or(name).to_lowercase()),
        image: None,
    };
    let members: Vec<TeamSpaceMemberDTO> = [
        "Odette Ward", "Arlo Finch", "Orla Byrne", "Idris Vale", "Wren Castillo", "Sanna Holt", "Rafe Lindqvist",
        "Bea Okafor", "Milo Arden", "Tess Marlow", "Juno Petrov", "Cal Ashby", "Nell Imai",
    ]
    .iter()
    .map(|name| member(name))
    .collect();
    let attendee = |name: &str| NoteAttendeeDTO {
        name: name.to_owned(),
        short_name: name.split(' ').next().unwrap_or(name).to_owned(),
        initials: initial(name),
    };
    let note = |title: &str, date: &str, start: &str, owner: usize, with: &[&str], others: i64, mine: bool| TeamSpaceNoteDTO {
        id: uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_DNS, title.as_bytes()).to_string(),
        title: title.to_owned(),
        date: date.to_owned(),
        created_at: format!("{date}T{start}:00.000Z"),
        meeting: Some(NoteMeetingTimeDTO {
            all_day: false,
            start_at: Local
                .from_local_datetime(
                    &NaiveDate::parse_from_str(date, "%Y-%m-%d")
                        .unwrap_or_default()
                        .and_time(chrono::NaiveTime::parse_from_str(start, "%H:%M").unwrap_or_default()),
                )
                .single()
                .map(|at| at.to_rfc3339())
                .unwrap_or_default(),
            end_at: String::new(),
        }),
        attendees: NoteAttendeesDTO {
            people: with.iter().map(|name| attendee(name)).collect(),
            others,
            total: others + 1,
        },
        owner: members[owner].clone(),
        added_at: format!("{date}T23:00:00.000Z"),
        mine,
        can_remove: mine,
    };
    let notes = vec![
        note("Acme // Northwind in office", "2026-07-07", "22:30", 5, &["Sanna", "Dario"], 6, false),
        note(
            "Platform architecture: modularity, naming, and release trains",
            "2026-06-29",
            "14:31",
            3,
            &["Lena Brandt"],
            1,
            true,
        ),
        note("Customer interview: onboarding friction", "2026-06-29", "10:00", 1, &["Arlo", "Priya", "Tomas"], 3, false),
    ];
    let person = |name: &str, email: &str, notes: i64, date: &str| TeamSpacePersonDTO {
        name: name.to_owned(),
        email: Some(email.to_owned()),
        initials: initial(name),
        notes,
        last_note_date: date.to_owned(),
        note_ids: Vec::new(),
    };
    let people = vec![
        person("Odette Ward", "odette@example.test", 1, "2026-09-30"),
        person("Lena Brandt", "lena@example.test", 1, "2026-09-22"),
        person("Dario", "dario@northwind.example.test", 1, "2026-07-07"),
        person("Bryn", "bryn@northwind.example.test", 1, "2026-07-07"),
        person("Delphine", "delphine@northwind.example.test", 1, "2026-07-07"),
        person("Sanna Holt", "sanna@example.test", 2, "2026-07-07"),
        person("Wren Castillo", "wren@example.test", 1, "2026-07-07"),
    ];
    TeamSpaceDTO {
        members,
        notes: if empty { Vec::new() } else { notes },
        organization: Organization {
            id: "00000000-0000-4000-8000-000000000002".into(),
            name: "Acme".into(),
        },
        people: if empty { Vec::new() } else { people },
        people_source: PeopleSource::Team,
        role: OrganizationDtoRole::Member,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // `gpui::*` re-exports its own `test` attribute; these are plain unit tests.
    use core::prelude::v1::test;

    #[test]
    fn the_space_is_named_after_its_workspace() {
        assert_eq!(space_name("Acme"), "Acme team");
        assert_eq!(space_name("  Acme  "), "Acme team");
        assert_eq!(space_name(""), "Team");
        assert_eq!(space_name("   "), "Team");
    }

    #[test]
    fn a_row_says_who_the_meeting_was_with() {
        let names = |names: &[&str]| names.iter().map(|name| (*name).to_owned()).collect::<Vec<_>>();
        assert_eq!(attendee_line(&[], 0, "Ada"), "Ada");
        assert_eq!(attendee_line(&names(&["Ben"]), 1, "Ada"), "Ben");
        assert_eq!(attendee_line(&names(&["Ben", "Cy"]), 2, "Ada"), "Ben & Cy");
        assert_eq!(attendee_line(&names(&["Ben", "Cy", "Di"]), 3, "Ada"), "Ben, Cy & 1 other");
        assert_eq!(attendee_line(&names(&["Ben", "Cy", "Di", "Ed"]), 6, "Ada"), "Ben, Cy & 4 others");
        assert_eq!(attendee_line(&names(&["Ben"]), 3, "Ada"), "Ben & 2 others");
    }

    #[test]
    fn a_persons_latest_note_names_its_year_only_when_it_is_not_this_one() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 16).unwrap();
        assert_eq!(last_note_label("2026-07-07", today), "Jul 7");
        assert_eq!(last_note_label("2025-12-31", today), "Dec 31, 2025");
        assert_eq!(last_note_label("soon", today), "soon");
    }

    fn person(name: &str, email: Option<&str>, date: &str) -> TeamSpacePersonDTO {
        TeamSpacePersonDTO {
            name: name.into(),
            email: email.map(str::to_owned),
            initials: initial(name),
            notes: 1,
            last_note_date: date.into(),
            note_ids: Vec::new(),
        }
    }

    #[test]
    fn people_are_listed_by_their_latest_note_and_found_by_name_or_address() {
        let people = vec![
            person("Kai Example", Some("kai@example.test"), "2026-07-07"),
            person("Noor Sample", Some("noor@example.test"), "2026-09-30"),
            person("Abe Sample", None, "2026-07-07"),
        ];
        let names = |listed: Vec<&TeamSpacePersonDTO>| listed.into_iter().map(|person| person.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(listed_people(&people, "", false)), ["Noor Sample", "Abe Sample", "Kai Example"]);
        assert_eq!(names(listed_people(&people, "", true)), ["Abe Sample", "Kai Example", "Noor Sample"]);
        assert_eq!(names(listed_people(&people, " SAMPLE ", false)), ["Noor Sample", "Abe Sample"]);
        assert_eq!(names(listed_people(&people, "kai@", false)), ["Kai Example"]);
        assert!(listed_people(&people, "nobody", false).is_empty());
    }

    #[test]
    fn someone_keeps_their_tint_and_their_letter() {
        assert_eq!(tint("kai@example.test"), tint("KAI@example.test"));
        assert_eq!(initial("  kai example"), "K");
        assert_eq!(initial("…"), "?");
    }

    /// The page offers Notes and People and nothing to do with files or
    /// integrations, whatever the reference it was drawn from shows.
    #[test]
    fn the_page_has_no_files_and_no_integrations() {
        let source = include_str!("team_space.rs");
        let page = &source[..source.find("#[cfg(test)]").unwrap()];
        for absent in ["\"Files\"", "\"Integrations\"", "paperclip", "All recipes"] {
            assert!(!page.contains(absent), "{absent} is on the page");
        }
    }
}
