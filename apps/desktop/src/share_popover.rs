//! The Share popover of a note: who it is shared with, the people picker,
//! and how far its links reach.
//!
//! Sharing never leaves the note's workspace. The server decides who is in
//! it and refuses anyone else; this popover only ever lists the people the
//! server sent (members of the workspace) and says what the server answered
//! when a typed address is not one of theirs.
use crate::{
    app::{Cereal, Menu, Screen},
    input::{InputChanged, TextInput},
    motion::{GOOEY, Pop, Pose, pop},
    ui::*,
};
use gpui::{prelude::*, *};
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::Duration;

/// The popover's geometry in points, measured from the reference captures
/// (1.45 px per point there: the traffic lights are 29 px apart).
pub mod size {
    /// 348 in the reference; a little more here, where the same words set wider.
    pub const CARD_WIDTH: f32 = 360.;
    /// The search field and the picker under it sit this far inside the card.
    pub const FIELD_X: f32 = 11.;
    pub const FIELD_TOP: f32 = 14.5;
    pub const FIELD_HEIGHT: f32 = 30.;
    pub const FIELD_PAD: f32 = 10.;
    /// Section captions and the rows under them start here.
    pub const TEXT_X: f32 = 14.5;
    pub const CAPTION_TOP: f32 = 12.;
    pub const CAPTION_LINE: f32 = 16.;
    pub const ROW_TOP: f32 = 3.;
    pub const ROW_HEIGHT: f32 = 38.;
    pub const SECTION_GAP: f32 = 11.;
    pub const ATTENDEE_AVATAR: f32 = 24.;
    pub const ATTENDEE_OVERLAP: f32 = 6.;
    pub const PERSON_AVATAR: f32 = 25.;
    pub const AVATAR_GAP: f32 = 10.;
    pub const SHARE_BUTTON_HEIGHT: f32 = 25.;
    pub const FOOTER_HEIGHT: f32 = 45.;
    pub const COPY_HEIGHT: f32 = 24.;
    pub const FOOTER_RIGHT: f32 = 10.;
    /// At most this many people are listed before the list scrolls.
    pub const PEOPLE_SHOWN: f32 = 5.;
    /// The picker: 5 pt under the field, a caption, then 38 pt rows whose
    /// highlight is inset 3.5 pt.
    pub const PICKER_GAP: f32 = 5.;
    pub const PICKER_CAPTION: f32 = 25.5;
    pub const PICKER_INSET: f32 = 3.5;
    pub const PICKER_AVATAR: f32 = 21.;
    pub const PICKER_ROWS: usize = 6;
}

/// How long typing rests before the workspace is searched.
const SEARCH_DELAY: Duration = Duration::from_millis(160);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Invited,
    Organization,
}

impl Level {
    pub const ALL: [Level; 2] = [Level::Invited, Level::Organization];

    /// The words of the footer and its menu. `place` is the workspace's mail
    /// domain, or its name when it has none.
    pub fn label(self, place: &str) -> String {
        match self {
            Level::Invited => "Only people invited".into(),
            Level::Organization => format!("Anyone at {place} with the link"),
        }
    }

    /// The same, for the popover's footer, where it stands beside "Copy
    /// link" and has to fit: the link is named by the button next to it.
    pub fn short_label(self, place: &str) -> String {
        match self {
            Level::Invited => "Only people invited".into(),
            Level::Organization => format!("Anyone at {place}"),
        }
    }

    fn symbol(self) -> &'static str {
        match self {
            Level::Invited => "lock",
            Level::Organization => "globe",
        }
    }

    fn wire(self) -> &'static str {
        match self {
            Level::Invited => "invited",
            Level::Organization => "organization",
        }
    }
}

/// A member of the note's workspace, as the server sent them. The flags are
/// present only where they mean something: `has_access` on an attendee,
/// `attendee` on a candidate.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Person {
    pub user_id: String,
    pub name: String,
    pub email: String,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub has_access: bool,
    #[serde(default)]
    pub attendee: bool,
}

impl Person {
    /// The name the rows show: an account without one goes by its address.
    pub fn display_name(&self) -> String {
        if self.name.trim().is_empty() {
            self.email.clone()
        } else {
            self.name.clone()
        }
    }

    pub fn initial(&self) -> String {
        self.display_name()
            .chars()
            .find(|character| character.is_alphanumeric())
            .map(|character| character.to_uppercase().collect())
            .unwrap_or_else(|| "?".into())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Organization {
    pub name: String,
    #[serde(default)]
    pub domain: Option<String>,
}

/// A note's sharing, as its owner sees it (`NoteAccessDTO`).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Access {
    pub level: Level,
    #[serde(default)]
    pub organization: Option<Organization>,
    pub owner: Person,
    #[serde(default)]
    pub people: Vec<Person>,
    #[serde(default)]
    pub attendees: Vec<Person>,
}

impl Access {
    /// What the workspace is called in a sentence: its mail domain, else its
    /// name.
    pub fn place(&self) -> String {
        self.organization
            .as_ref()
            .map(|organization| {
                organization
                    .domain
                    .clone()
                    .filter(|domain| !domain.trim().is_empty())
                    .unwrap_or_else(|| organization.name.clone())
            })
            .filter(|place| !place.trim().is_empty())
            .unwrap_or_else(|| "your organization".into())
    }

    /// "Share with attendees": its two lines, when the meeting had anyone
    /// from the workspace.
    pub fn attendee_lines(&self) -> Option<(String, String)> {
        if self.attendees.is_empty() {
            return None;
        }
        let emails: Vec<&str> = self.attendees.iter().map(|person| person.email.as_str()).collect();
        Some((
            format!("Only people from {} ({})", self.place(), self.attendees.len()),
            emails.join(", "),
        ))
    }

    /// The attendees "Share" would add.
    pub fn waiting_attendees(&self) -> Vec<String> {
        self.attendees
            .iter()
            .filter(|person| !person.has_access)
            .map(|person| person.user_id.clone())
            .collect()
    }
}

/// The members a picker lists for `query`, out of everyone it could: those
/// without access, attendees first, then whoever's name or address holds
/// every word typed. The server does this for a real note; the tour's
/// fixture and the tests use it as written here.
pub fn candidates(pool: &[Person], access: &Access, query: &str) -> Vec<Person> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    let attended = |person: &Person| access.attendees.iter().any(|attendee| attendee.user_id == person.user_id);
    let open = pool.iter().filter(|person| {
        person.user_id != access.owner.user_id
            && !access.people.iter().any(|other| other.user_id == person.user_id)
            && {
                let text = format!("{} {}", person.name, person.email).to_lowercase();
                words.iter().all(|word| text.contains(word))
            }
    });
    let (mut first, mut rest): (Vec<Person>, Vec<Person>) = open.cloned().partition(attended);
    for person in &mut first {
        person.attendee = true;
    }
    first.append(&mut rest);
    first
}

/// Whether what was typed is a whole address: Return then asks the server
/// to share with it, and the server says so when it is not a member's.
pub fn is_address(text: &str) -> bool {
    let text = text.trim();
    match text.split_once('@') {
        Some((name, host)) => {
            !name.is_empty()
                && host.contains('.')
                && !host.starts_with('.')
                && !host.ends_with('.')
                && !text.contains(char::is_whitespace)
                && !host.contains('@')
        }
        None => false,
    }
}

/// A query on its way into a request address.
fn encoded(text: &str) -> String {
    let mut encoded = String::with_capacity(text.len() * 3);
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

/// The fill and ink of a person's letter, the same for the same address
/// wherever they are listed.
fn tint(person: &Person) -> (Hsla, Hsla) {
    const TINTS: [(u32, u32); 6] = [
        (0xb0306f, 0xffe3f1),
        (0x5a45b5, 0xe9e3ff),
        (0x4f4a1c, 0xe9d98a),
        (0x1f6f5c, 0xd6f5ea),
        (0x2f5fa8, 0xddebff),
        (0xa5522a, 0xffe4d2),
    ];
    let key = if person.email.is_empty() { &person.name } else { &person.email };
    let hash = key
        .to_lowercase()
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3));
    let (fill, ink) = TINTS[(hash % TINTS.len() as u64) as usize];
    (rgb(fill).into(), rgb(ink).into())
}

/// Made-up people for the tour's stills: a workspace of six, two of whom
/// were at the meeting.
pub fn tour_fixture() -> (Access, Vec<Person>) {
    let person = |number: u32, name: &str, email: &str| Person {
        user_id: format!("7a000000-0000-4000-8000-{number:012}"),
        name: name.into(),
        email: email.into(),
        ..Person::default()
    };
    let owner = person(1, "Sam Carter", "sam@example.test");
    let attendees = vec![
        person(2, "Maya Okafor", "maya@example.test"),
        person(3, "Theo Lindqvist", "theo@example.test"),
    ];
    let mut workspace = vec![owner.clone()];
    workspace.extend(attendees.iter().cloned());
    workspace.extend([
        person(4, "Priya Raman", "priya@example.test"),
        person(5, "Marcus Bell", "marcus@example.test"),
        person(6, "Imani Clarke", "imani@example.test"),
    ]);
    (
        Access {
            level: Level::Invited,
            organization: Some(Organization { name: "Example".into(), domain: Some("example.test".into()) }),
            owner,
            people: Vec::new(),
            attendees,
        },
        workspace,
    )
}

/// One row of "Shared with me" that came from the server: a note somebody
/// shared with this account by name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SharedEntry {
    pub origin: crate::link::ServerOrigin,
    pub note_id: String,
    pub title: String,
    pub owner: String,
    pub shared_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// What a request for a note's sharing is.
enum Ask {
    Load,
    Add { user_ids: Vec<String>, emails: Vec<String> },
    Remove(String),
    Level(Level),
}

/// The line under the field that says what was refused.
const PROBLEM: crate::motion::TrackKey = ("share-problem", 0);

pub struct Share {
    /// "Enter name or email".
    pub field: Entity<TextInput>,
    /// Whether the popover was open at the last frame, and for which note.
    open: bool,
    note: Option<u64>,
    pub access: Option<Access>,
    loading: bool,
    /// A change is on its way to the server: the controls wait for it.
    busy: bool,
    pub problem: Option<String>,
    /// The picker under the field, the people it lists, and the query they
    /// answer ("" is Suggested).
    pub picker: bool,
    listed: Vec<Person>,
    answered: String,
    selected: usize,
    pub level_menu: bool,
    level_selected: usize,
    /// Counts the popover's openings and its searches, so an answer that
    /// arrives for an earlier one is dropped.
    epoch: u64,
    search_epoch: u64,
    /// The tour's stand-in for a server: everyone in its workspace. With it
    /// set nothing is ever requested.
    fixture: Option<Vec<Person>>,
    /// "Shared with me", as the default account's server last listed it.
    pub shared_with_me: Vec<SharedEntry>,
    shared_asked: bool,
}

impl Share {
    pub fn new(cx: &mut Context<Cereal>) -> (Self, Vec<Subscription>) {
        let field = cx.new(|cx| TextInput::new("Enter name or email", false, cx));
        let subscriptions = vec![cx.subscribe(&field, |this, _, _: &InputChanged, cx| {
            this.share_query_changed(cx);
        })];
        (
            Self {
                field,
                open: false,
                note: None,
                access: None,
                loading: false,
                busy: false,
                problem: None,
                picker: false,
                listed: Vec::new(),
                answered: String::new(),
                selected: 0,
                level_menu: false,
                level_selected: 0,
                epoch: 0,
                search_epoch: 0,
                fixture: None,
                shared_with_me: Vec::new(),
                shared_asked: false,
            },
            subscriptions,
        )
    }

    /// Show the popover from made-up people instead of a server (the tour).
    pub fn preview(&mut self, access: Access, workspace: Vec<Person>) {
        self.access = Some(access);
        self.fixture = Some(workspace);
        self.problem = None;
    }
}

impl Cereal {
    /// Once a frame, before anything is drawn: the popover has just opened
    /// (read the note's sharing, put the caret in the field) or just closed.
    pub(crate) fn sync_share(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let open = self.menu == Some(Menu::Share);
        if open && (!self.share.open || self.share.note != self.current_note) {
            self.share.open = true;
            self.share.note = self.current_note;
            self.share.epoch = self.share.epoch.wrapping_add(1);
            self.share.picker = false;
            self.share.level_menu = false;
            self.share.listed.clear();
            self.share.answered.clear();
            self.share.selected = 0;
            self.share.busy = false;
            self.share.loading = false;
            if self.share.fixture.is_none() {
                self.share.access = None;
            }
            // What was refused belongs to the attempt, not to the note: a
            // popover that is opened again says nothing about the last one.
            self.share.problem = None;
            self.stage.forget(PROBLEM);
            self.share.field.update(cx, |input, cx| input.set_content("", cx));
            self.share.field.read(cx).focus_handle.focus(window);
            self.share_load(cx);
        } else if !open && self.share.open {
            self.share.open = false;
            self.share.picker = false;
            self.share.level_menu = false;
            self.share.epoch = self.share.epoch.wrapping_add(1);
            self.share.search_epoch = self.share.search_epoch.wrapping_add(1);
        }
        // A note that had never been sent has an id on the server now.
        if open
            && self.share.access.is_none()
            && !self.share.loading
            && self.share.problem.is_none()
            && self.share_server_id().is_some()
        {
            self.share_load(cx);
        }
        if self.screen == Screen::Shared {
            if !self.share.shared_asked {
                self.share.shared_asked = true;
                self.load_shared_with_me(cx);
            }
        } else {
            self.share.shared_asked = false;
        }
    }

    fn share_server_id(&self) -> Option<String> {
        let note = self.workspace.note(self.share.note?)?;
        let id = note.server_id.as_deref()?;
        crate::accounts::client::path_id(id).ok()
    }

    fn share_load(&mut self, cx: &mut Context<Self>) {
        if self.share.fixture.is_some() {
            return;
        }
        if !crate::server::enabled() {
            self.share.problem = Some("Connect to your server to share a note".into());
            return;
        }
        let Some(note) = self.share.note else { return };
        if self.share_server_id().is_none() {
            // Never sent: sharing begins once the server has the note.
            self.share.loading = false;
            if !self.server_action_sent(note, crate::server::Action::Save, cx) {
                self.share.problem = Some("Sign in to your server to share this note".into());
            }
            return;
        }
        self.share_ask(Ask::Load, cx);
    }

    /// Ask the note's server for its sharing, or to change it. The answer is
    /// always the sharing as it now stands.
    fn share_ask(&mut self, ask: Ask, cx: &mut Context<Self>) {
        let (Some(note), Some(id)) = (self.share.note, self.share_server_id()) else {
            return;
        };
        let client = match crate::accounts::client_of_note(note) {
            Ok(client) => client,
            Err(error) => {
                self.share.problem = Some(error);
                cx.notify();
                return;
            }
        };
        let loading = matches!(ask, Ask::Load);
        let added = matches!(ask, Ask::Add { .. });
        if loading {
            self.share.loading = true;
        } else {
            self.share.busy = true;
        }
        self.share.problem = None;
        let epoch = self.share.epoch;
        let base = format!("/api/v1/notes/{id}/access");
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let (method, path, body) = match ask {
                        Ask::Load => ("GET", base, None),
                        Ask::Add { user_ids, emails } => (
                            "POST",
                            format!("{base}/people"),
                            Some(json!({"userIds": user_ids, "emails": emails})),
                        ),
                        Ask::Remove(user) => {
                            let user = crate::accounts::client::path_id(&user).map_err(|error| error.message())?;
                            ("DELETE", format!("{base}/people/{user}"), None)
                        }
                        Ask::Level(level) => ("PATCH", base, Some(json!({"level": level.wire()}))),
                    };
                    let answer: Value = client.request(method, &path, body)?;
                    serde_json::from_value::<Access>(answer)
                        .map_err(|_| "Server response does not match the shared API contract".to_string())
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.share.epoch != epoch {
                    return;
                }
                this.share.loading = false;
                this.share.busy = false;
                match result {
                    Ok(access) => {
                        this.share.access = Some(access);
                        if added {
                            this.share_added(cx);
                        }
                    }
                    Err(error) => {
                        // What the server said is shown where the picker was.
                        if added {
                            this.share.picker = false;
                        }
                        this.share.problem = Some(error);
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// People were added: the field empties and the picker closes.
    fn share_added(&mut self, cx: &mut Context<Self>) {
        self.share.picker = false;
        self.share.listed.clear();
        self.share.answered.clear();
        self.share.selected = 0;
        self.share.search_epoch = self.share.search_epoch.wrapping_add(1);
        self.share.field.update(cx, |input, cx| input.set_content("", cx));
    }

    /// The field's text changed: the picker opens on what was typed.
    pub(crate) fn share_query_changed(&mut self, cx: &mut Context<Self>) {
        if self.menu != Some(Menu::Share) {
            return;
        }
        self.share.problem = None;
        self.share.picker = true;
        self.share.level_menu = false;
        self.share_search(SEARCH_DELAY, cx);
        cx.notify();
    }

    /// Open the picker on what the field holds (nothing: Suggested).
    pub(crate) fn share_open_picker(&mut self, cx: &mut Context<Self>) {
        self.share.level_menu = false;
        if !self.share.picker {
            self.share.picker = true;
            self.share_search(Duration::ZERO, cx);
        }
        cx.notify();
    }

    /// List the members matching the field, after `delay` without another
    /// keystroke.
    fn share_search(&mut self, delay: Duration, cx: &mut Context<Self>) {
        self.share.search_epoch = self.share.search_epoch.wrapping_add(1);
        let search = self.share.search_epoch;
        let query = self.share.field.read(cx).content().trim().to_owned();
        if let (Some(pool), Some(access)) = (&self.share.fixture, &self.share.access) {
            self.share.listed = candidates(pool, access, &query);
            self.share.answered = query;
            self.share.selected = 0;
            return;
        }
        let (Some(note), Some(id)) = (self.share.note, self.share_server_id()) else {
            return;
        };
        let Ok(client) = crate::accounts::client_of_note(note) else {
            return;
        };
        let asked = query.clone();
        cx.spawn(async move |this, cx| {
            if !delay.is_zero() {
                cx.background_executor().timer(delay).await;
                let current = this
                    .update(cx, |this, _| this.share.search_epoch == search)
                    .unwrap_or(false);
                if !current {
                    return;
                }
            }
            let result = cx
                .background_executor()
                .spawn(async move {
                    #[derive(Deserialize)]
                    struct Wire {
                        people: Vec<Person>,
                    }
                    let path = format!("/api/v1/notes/{id}/access/candidates?q={}", encoded(&asked));
                    client.request::<Wire>("GET", &path, None).map(|wire| wire.people)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.share.search_epoch != search {
                    return;
                }
                match result {
                    Ok(people) => {
                        this.share.listed = people;
                        this.share.answered = query;
                        this.share.selected = 0;
                    }
                    Err(error) => this.share.problem = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Share the note with these members, or (by address) with whoever the
    /// server finds to be one.
    fn share_add(&mut self, user_ids: Vec<String>, emails: Vec<String>, cx: &mut Context<Self>) {
        if self.share.busy || (user_ids.is_empty() && emails.is_empty()) {
            return;
        }
        if let (Some(pool), Some(access)) = (self.share.fixture.clone(), self.share.access.as_mut()) {
            for person in pool.iter().filter(|person| user_ids.contains(&person.user_id)) {
                if !access.people.iter().any(|other| other.user_id == person.user_id) {
                    access.people.push(person.clone());
                }
            }
            for attendee in &mut access.attendees {
                attendee.has_access |= user_ids.contains(&attendee.user_id);
            }
            if let Some(email) = emails.first() {
                let place = access.place();
                self.share.problem = Some(format!(
                    "{email} is not in {place}. Notes can only be shared with people in {place}."
                ));
                self.share.picker = false;
            } else {
                self.share_added(cx);
            }
            cx.notify();
            return;
        }
        self.share_ask(Ask::Add { user_ids, emails }, cx);
    }

    pub(crate) fn share_with_attendees(&mut self, cx: &mut Context<Self>) {
        let waiting = self
            .share
            .access
            .as_ref()
            .map(Access::waiting_attendees)
            .unwrap_or_default();
        self.share_add(waiting, Vec::new(), cx);
    }

    fn share_remove(&mut self, user: String, cx: &mut Context<Self>) {
        if self.share.busy {
            return;
        }
        if self.share.fixture.is_some() {
            if let Some(access) = self.share.access.as_mut() {
                access.people.retain(|person| person.user_id != user);
                for attendee in &mut access.attendees {
                    if attendee.user_id == user {
                        attendee.has_access = false;
                    }
                }
            }
            cx.notify();
            return;
        }
        self.share_ask(Ask::Remove(user), cx);
    }

    fn share_set_level(&mut self, level: Level, cx: &mut Context<Self>) {
        self.share.level_menu = false;
        if self.share.busy || self.share.access.as_ref().is_none_or(|access| access.level == level) {
            cx.notify();
            return;
        }
        if self.share.fixture.is_some() {
            if let Some(access) = self.share.access.as_mut() {
                access.level = level;
            }
            cx.notify();
            return;
        }
        self.share_ask(Ask::Level(level), cx);
    }

    /// Return in the field: the highlighted person, or the address typed.
    fn share_submit(&mut self, cx: &mut Context<Self>) {
        let typed = self.share.field.read(cx).content().trim().to_owned();
        let current = self.share.answered == typed;
        if self.share.picker
            && current
            && let Some(person) = self.share.listed.get(self.share.selected).cloned()
        {
            self.share_add(vec![person.user_id], Vec::new(), cx);
        } else if is_address(&typed) {
            self.share_add(Vec::new(), vec![typed], cx);
        }
    }

    /// The keys the popover keeps for itself. True when it used the key.
    pub(crate) fn handle_share_key(
        &mut self,
        event: &KeystrokeEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.menu != Some(Menu::Share) {
            return false;
        }
        let modifiers = event.keystroke.modifiers;
        if modifiers.control || modifiers.alt || modifiers.shift || modifiers.platform {
            return false;
        }
        let step = |index: usize, count: usize, down: bool| {
            if count == 0 {
                0
            } else if down {
                (index + 1) % count
            } else {
                (index + count - 1) % count
            }
        };
        let in_field = self.share.field.read(cx).focus_handle.is_focused(window);
        match event.keystroke.key.as_str() {
            // One thing closes at a time: the menu, the picker, the popover.
            "escape" => {
                if self.share.level_menu {
                    self.share.level_menu = false;
                } else if self.share.picker {
                    self.share.picker = false;
                } else {
                    self.menu = None;
                    self.focus.focus(window);
                }
            }
            key @ ("down" | "up") => {
                let down = key == "down";
                if self.share.level_menu {
                    self.share.level_selected = step(self.share.level_selected, Level::ALL.len(), down);
                } else if !in_field {
                    return false;
                } else if !self.share.picker {
                    self.share_open_picker(cx);
                } else {
                    let count = self.share.listed.len().min(size::PICKER_ROWS);
                    self.share.selected = step(self.share.selected, count, down);
                }
            }
            "enter" | "return" => {
                if self.share.level_menu {
                    let level = Level::ALL[self.share.level_selected.min(Level::ALL.len() - 1)];
                    self.share_set_level(level, cx);
                } else if in_field {
                    self.share_submit(cx);
                } else {
                    return false;
                }
            }
            _ => return false,
        }
        cx.notify();
        cx.stop_propagation();
        true
    }

    /// A note went to Trash or came back: nobody it is shared with reads it
    /// while it is there. The server is told; the note itself is unchanged.
    pub(crate) fn share_note_trashed(&mut self, id: u64, trashed: bool, cx: &mut Context<Self>) {
        if !crate::server::enabled() {
            return;
        }
        let Some(server_id) = self
            .workspace
            .note(id)
            .and_then(|note| note.server_id.as_deref())
            .and_then(|id| crate::accounts::client::path_id(id).ok())
        else {
            return;
        };
        let Ok(client) = crate::accounts::client_of_note(id) else {
            return;
        };
        cx.background_executor()
            .spawn(async move {
                let path = format!("/api/v1/notes/{server_id}/access");
                if let Err(error) = client.request::<Value>("PATCH", &path, Some(json!({"suspended": trashed}))) {
                    eprintln!("sharing of a note in Trash could not be updated: {error}");
                }
            })
            .detach();
    }

    /// "Shared with me" on the default account's server: the notes shared
    /// with this account by name.
    fn load_shared_with_me(&mut self, cx: &mut Context<Self>) {
        if !crate::server::enabled() {
            return;
        }
        let Some(account) = crate::accounts::default_account().filter(|account| account.shown()) else {
            return;
        };
        let (Ok(client), Some(origin)) = (
            crate::accounts::client(&account.id),
            crate::link::ServerOrigin::of_server(&account.url),
        ) else {
            return;
        };
        let epoch = account.epoch();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    #[derive(Deserialize)]
                    #[serde(rename_all = "camelCase")]
                    struct Entry {
                        id: String,
                        #[serde(default)]
                        title: String,
                        owner: Person,
                        #[serde(default)]
                        shared_at: Option<chrono::DateTime<chrono::Utc>>,
                    }
                    #[derive(Deserialize)]
                    struct Wire {
                        notes: Vec<Entry>,
                    }
                    client.request::<Wire>("GET", "/api/v1/shared-with-me", None).map(|wire| {
                        wire.notes
                            .into_iter()
                            .filter_map(|entry| {
                                Some(SharedEntry {
                                    origin: origin.clone(),
                                    note_id: crate::accounts::client::path_id(&entry.id).ok()?,
                                    title: entry.title.chars().take(500).collect(),
                                    owner: entry.owner.display_name().chars().take(200).collect(),
                                    shared_at: entry.shared_at,
                                })
                            })
                            .take(200)
                            .collect::<Vec<_>>()
                    })
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if crate::accounts::epoch_of(&account.id) != Some(epoch) {
                    return;
                }
                // A server that cannot be asked right now leaves the list as it was.
                if let Ok(entries) = result {
                    this.share.shared_with_me = entries;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// The rows "Shared with me" shows for notes shared by name, under their
    /// caption. Nothing when there are none.
    pub(crate) fn render_shared_by_name(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.share.shared_with_me.is_empty() {
            return None;
        }
        let t = self.theme();
        let mut rows = div().flex().flex_col().child(
            caption("Shared with you", t)
                .px(px(8.))
                .pt(px(25.))
                .pb(px(2.)),
        );
        for entry in &self.share.shared_with_me {
            let key = crate::lazy::key(&entry.note_id);
            let hovered = crate::motion::reveal_key(("shared-by-name", key));
            let title = if entry.title.trim().is_empty() {
                "New note".to_string()
            } else {
                entry.title.clone()
            };
            let when = entry
                .shared_at
                .map(|at| at.with_timezone(&chrono::Local).format("%b %-d").to_string())
                .unwrap_or_default();
            let (origin, note_id) = (entry.origin.clone(), entry.note_id.clone());
            rows = rows.child(
                self.list_row_frame(
                    ("shared-by-name", key),
                    hovered,
                    title.into(),
                    format!("Shared by {}", entry.owner),
                    &when,
                    div().id(("shared-by-name-action", key)),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.open_granted_note(origin.clone(), note_id.clone(), cx);
                })),
            );
        }
        Some(rows.into_any_element())
    }

    /// A person's picture (`portrait.rs`), or their letter on their colour.
    /// `own` is the signed-in account's session, for the owner's row.
    fn share_avatar(
        &self,
        person: &Person,
        own: Option<&meeting_notes_api_types::AuthSessionDTO>,
        edge: f32,
        cx: &mut Context<Self>,
    ) -> Div {
        // The picture helper reads an account's session. For anyone else one
        // is made up around the address the server sent for that person.
        let picture = match own.filter(|session| session.user.image.is_some()) {
            Some(session) => crate::portrait::of(session, cx),
            None => person
                .image
                .as_deref()
                .and_then(|image| {
                    serde_json::from_value::<meeting_notes_api_types::AuthSessionDTO>(json!({
                        "expiresAt": null,
                        "user": {"createdAt": "", "email": person.email, "id": person.user_id, "name": person.name, "role": "user", "image": image}
                    }))
                    .ok()
                })
                .and_then(|session| crate::portrait::of(&session, cx)),
        };
        let (fill, ink) = tint(person);
        crate::portrait::avatar(
            picture,
            edge,
            div()
                .size(px(edge))
                .flex_shrink_0()
                .rounded_full()
                .bg(fill)
                .text_color(ink)
                .font_family(DISPLAY)
                .text_size(px((edge * 0.52).round()))
                .line_height(px(edge))
                .flex()
                .items_center()
                .justify_center()
                .child(person.initial()),
        )
    }

    /// A name over an address: the two lines of every person row.
    fn share_lines(&self, name: AnyElement, email: String) -> Div {
        let t = self.theme();
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .text_size(px(13.))
                    .line_height(px(17.))
                    .truncate()
                    .child(name),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(t.muted)
                    .truncate()
                    .child(email),
            )
    }

    pub(crate) fn render_share_popover(
        &self,
        pose: Pose,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use size::*;
        let t = self.theme();
        let hairline = 1. / window.scale_factor().max(1.);
        let share = &self.share;
        let focused = share.field.read(cx).focus_handle.is_focused(window);
        let access = share.access.as_ref();
        let place = access.map(Access::place).unwrap_or_else(|| "your organization".into());
        let waiting = share.busy || share.loading;
        let caption = |text: &'static str| {
            div()
                .px(px(TEXT_X))
                .pt(px(CAPTION_TOP))
                .text_size(px(12.))
                .line_height(px(CAPTION_LINE))
                .text_color(t.muted)
                .child(text)
        };
        let (ring, halo) = ask_ring();
        let field = div()
            .id("share-field")
            .mx(px(FIELD_X))
            .mt(px(FIELD_TOP))
            .h(px(FIELD_HEIGHT))
            .px(px(FIELD_PAD))
            .flex_shrink_0()
            .flex()
            .items_center()
            .rounded_full()
            .bg(t.card)
            .border_1()
            .border_color(if focused { ring } else { t.hairline })
            .when(focused, |field| {
                field.shadow(vec![BoxShadow {
                    color: halo,
                    offset: point(px(0.), px(0.)),
                    blur_radius: px(0.),
                    spread_radius: px(3.),
                }])
            })
            .text_size(px(13.))
            .line_height(px(18.))
            .child(div().flex_1().min_w_0().child(share.field.clone()))
            // A press in the field opens the picker; the field itself takes
            // the caret.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.share_open_picker(cx);
                    cx.stop_propagation();
                }),
            );
        let mut card = div()
            .w(px(CARD_WIDTH))
            .flex()
            .flex_col()
            .rounded(px(POPOVER_RADIUS))
            .bg(t.card)
            .border(px(hairline))
            .border_color(t.hairline.opacity(1.5))
            .shadow(vec![
                BoxShadow {
                    color: hsla(0., 0., 0., 0.12),
                    offset: point(px(0.), px(8.)),
                    blur_radius: px(16.),
                    spread_radius: px(0.),
                },
                BoxShadow {
                    color: hsla(0., 0., 0., 0.04),
                    offset: point(px(0.), px(2.)),
                    blur_radius: px(1.),
                    spread_radius: px(0.),
                },
            ])
            .text_size(px(13.))
            .text_color(t.text)
            .child(crate::onboarding_view::place("share-popover"))
            .child(field);
        // What was refused folds in under the field and folds away again:
        // the rows below it slide, and the card follows.
        card = card.children(match &share.problem {
            Some(problem) => crate::motion::appear(
                &self.stage,
                PROBLEM,
                true,
                div()
                    .id("share-problem")
                    .px(px(TEXT_X))
                    .pt(px(8.))
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(t.danger)
                    .child(problem.clone()),
            ),
            None => crate::motion::vacate(&self.stage, PROBLEM),
        });
        // Share with attendees: one press for everyone at the meeting who is
        // in the workspace.
        if let Some((_, emails)) = access.and_then(Access::attendee_lines) {
            let people = &access.expect("lines come from the access").attendees;
            let all_shared = people.iter().all(|person| person.has_access);
            let mut faces = div().flex().flex_shrink_0().items_center();
            for (index, person) in people.iter().take(3).enumerate() {
                // Each face is ringed in the card's colour where it laps the last.
                faces = faces.child(
                    div()
                        .when(index > 0, |face| face.ml(px(-ATTENDEE_OVERLAP)))
                        .rounded_full()
                        .border(px(1.5))
                        .border_color(t.card)
                        .child(self.share_avatar(person, None, ATTENDEE_AVATAR, cx)),
                );
            }
            let action = if all_shared {
                div()
                    .id("share-attendees-done")
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap(px(4.))
                    .text_size(px(12.))
                    .text_color(t.muted)
                    .child(icon("check", 13., t.muted))
                    .child("Shared")
            } else {
                div()
                    .id("share-attendees")
                    .relative()
                    .h(px(SHARE_BUTTON_HEIGHT))
                    .px(px(11.))
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .rounded_full()
                    .bg(t.text)
                    .text_color(t.bg)
                    .text_size(px(13.))
                    .cursor_default()
                    .reach()
                    .child(crate::motion::tactile(neutral_wash(0.14)).press(neutral_wash(0.12)).round())
                    .child("Share")
                    .when(waiting, |button| button.opacity(DISABLED))
                    .on_click(cx.listener(|this, _, _, cx| this.share_with_attendees(cx)))
            };
            // "Share" becomes "Shared" in place.
            let action = crate::motion::swapped(
                action,
                self.stage.swap(("share-attendees-action", 0), all_shared as u64),
            );
            card = card.child(caption("Share with attendees")).child(
                div()
                    .mt(px(ROW_TOP))
                    .h(px(ROW_HEIGHT))
                    .pl(px(TEXT_X - 1.5))
                    .pr(px(TEXT_X))
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap(px(AVATAR_GAP))
                    .child(faces)
                    .child(self.share_lines(
                        // The count stays in view however long the place's name is.
                        div()
                            .flex()
                            .gap(px(4.))
                            .child(div().min_w_0().truncate().child(format!("Only people from {place}")))
                            .child(div().flex_shrink_0().child(format!("({})", people.len())))
                            .into_any_element(),
                        emails,
                    ))
                    .child(action),
            );
        }
        // People with access: the owner, then everyone the note is shared with.
        let session = self
            .share
            .note
            .and_then(crate::accounts::session_of_note)
            // The tour's stills show the fixture account's picture when one is given.
            .or_else(|| self.share.fixture.as_ref().and_then(|_| crate::portrait::tour_session(cx)));
        let owner = access.map(|access| access.owner.clone()).or_else(|| {
            session.as_ref().map(|session| Person {
                user_id: session.user.id.clone(),
                name: session.user.name.clone(),
                email: session.user.email.clone(),
                image: session.user.image.clone(),
                ..Person::default()
            })
        });
        let mut people = div()
            .id("share-people")
            .mt(px(ROW_TOP))
            .max_h(px(ROW_HEIGHT * PEOPLE_SHOWN))
            .overflow_y_scroll()
            .flex()
            .flex_col();
        let person_row = |person: &Person,
                          own: Option<&meeting_notes_api_types::AuthSessionDTO>,
                          trailing: AnyElement,
                          name: AnyElement,
                          this: &Self,
                          cx: &mut Context<Self>| {
            div()
                .h(px(ROW_HEIGHT))
                .px(px(TEXT_X))
                .flex()
                .flex_shrink_0()
                .items_center()
                .gap(px(AVATAR_GAP))
                .child(this.share_avatar(person, own, PERSON_AVATAR, cx))
                .child(this.share_lines(name, person.email.clone()))
                .child(trailing)
        };
        match &owner {
            Some(owner) => {
                let name = div()
                    .flex()
                    .gap(px(4.))
                    .child(div().truncate().child(owner.display_name()))
                    .child(div().flex_shrink_0().text_color(t.muted).child("(you)"))
                    .into_any_element();
                let role = div()
                    .flex_shrink_0()
                    .pr(px(5.5))
                    .text_size(px(12.))
                    .text_color(t.muted)
                    .child("Owner")
                    .into_any_element();
                people = people.child(person_row(owner, session.as_ref(), role, name, self, cx));
            }
            None => {
                people = people.child(
                    div()
                        .h(px(ROW_HEIGHT))
                        .px(px(TEXT_X))
                        .flex()
                        .items_center()
                        .text_size(px(12.))
                        .text_color(t.muted)
                        .child(if share.loading { "Loading…" } else { "Sign in to your server" }),
                );
            }
        }
        // The list makes room for someone added and closes up after
        // someone removed.
        let shared_with = access.map(|access| access.people.as_slice()).unwrap_or_default();
        let keys: Vec<u64> = shared_with
            .iter()
            .map(|person| crate::lazy::key(&person.user_id))
            .collect();
        let rows = self.stage.roster(
            "share-people",
            crate::lazy::key(("share-people", share.note, share.epoch)),
            &keys,
        );
        for row in rows {
            let Some(person) = shared_with
                .iter()
                .find(|person| crate::lazy::key(&person.user_id) == row.key)
            else {
                // Removed: nothing left to draw, its space closes.
                people = people.children(crate::motion::roster_row(
                    &self.stage,
                    "share-people",
                    row,
                    None::<Div>,
                ));
                continue;
            };
            let user = person.user_id.clone();
            let key = row.key;
            let remove = tool_tip(("share-remove", key), "close", "Remove access", t)
                .size(px(24.))
                .flex_shrink_0()
                .when(waiting, |button| button.opacity(DISABLED))
                .on_click(cx.listener(move |this, _, _, cx| this.share_remove(user.clone(), cx)))
                .into_any_element();
            let name = div().truncate().child(person.display_name()).into_any_element();
            people = people.children(crate::motion::roster_row(
                &self.stage,
                "share-people",
                row,
                Some(person_row(person, None, remove, name, self, cx)),
            ));
        }
        card = card.child(caption("People with access").when(access.and_then(Access::attendee_lines).is_some(), |caption| {
            caption.pt(px(SECTION_GAP))
        }));
        card = card.child(people);
        // The footer: how far a link reaches, and the link.
        let level = access.map(|access| access.level).unwrap_or(Level::Invited);
        let can_change = access.is_some_and(|access| access.organization.is_some()) && !waiting;
        let level_button = div()
            .id("share-level")
            .relative()
            .h(px(COPY_HEIGHT))
            .px(px(5.))
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(5.))
            .rounded(px(6.))
            .cursor_default()
            .reach()
            .child(crate::motion::tactile(t.hover).press(t.fill).radius(6.))
            .child(icon(level.symbol(), 14., t.muted))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(px(12.5))
                    .text_color(t.secondary)
                    .child(level.short_label(&place)),
            )
            .child(icon("chevron-down", 10., t.muted))
            .when(!can_change, |button| button.opacity(DISABLED))
            .on_click(cx.listener(move |this, _, _, cx| {
                if !can_change {
                    return;
                }
                this.share.picker = false;
                this.share.level_menu = !this.share.level_menu;
                this.share.level_selected = Level::ALL.iter().position(|option| *option == level).unwrap_or(0);
                cx.notify();
            }));
        let connected = crate::server::enabled() || share.fixture.is_some();
        let copy = pill("share-copy-link", "Copy link", "link-chain", t)
            .h(px(COPY_HEIGHT))
            .px(px(9.))
            .gap(px(5.))
            .flex_shrink_0()
            .when(!connected, |button| button.opacity(DISABLED))
            .on_click(cx.listener(move |this, _, _, cx| {
                if !connected {
                    this.show_toast("Connect to your server to share a note", cx);
                    return;
                }
                if let Some(id) = this.current_note {
                    this.server_action(id, crate::server::Action::Share, cx);
                }
            }));
        card = card
            .child(div().mt(px(SECTION_GAP)).flex_shrink_0().h(px(hairline)).bg(t.hairline))
            .child(
                div()
                    .h(px(FOOTER_HEIGHT))
                    .pl(px(TEXT_X - 6.))
                    .pr(px(FOOTER_RIGHT))
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .child(level_button)
                    .child(copy),
            );
        // The picker lies over the card from just under the field.
        let picker = self.stage.presence(("share-picker", 0), share.picker && self.menu == Some(Menu::Share), GOOEY);
        let mut over: Vec<AnyElement> = Vec::new();
        if picker.visible {
            over.push(
                div()
                    .absolute()
                    .left(px(FIELD_X))
                    .right(px(FIELD_X))
                    .top(px(FIELD_TOP + FIELD_HEIGHT + PICKER_GAP))
                    .child(pop(self.render_share_picker(&place, cx), Pop::menu((0.5, 0.)), picker))
                    .into_any_element(),
            );
        }
        // The level menu drops from the footer's control.
        let menu = self.stage.presence(("share-level", 0), share.level_menu && self.menu == Some(Menu::Share), GOOEY);
        if menu.visible {
            let highlight = self.stage.glide("share-level");
            if !self.stage.continuous(("share-level-menu", 0)) {
                highlight.rest();
            }
            let mut options = div()
                .occlude()
                .min_w(px(250.))
                .p(px(4.))
                .flex()
                .flex_col()
                .rounded(px(10.))
                .bg(self.share_surface())
                .border_1()
                .border_color(t.border)
                .shadow_lg()
                .occlude().on_any_mouse_down(|_, _, cx| cx.stop_propagation())
                .child(highlight.indicator(t.hover, 6.));
            for (index, option) in Level::ALL.into_iter().enumerate() {
                options = options.child(
                    div()
                        .id(("share-level-option", index))
                        .relative()
                        .h(px(30.))
                        .px(px(8.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .rounded(px(6.))
                        .cursor_default()
                        .when(share.level_selected == index, |row| row.child(highlight.probe(index)))
                        .child(icon(option.symbol(), 14., t.muted))
                        .child(div().flex_1().text_size(px(13.)).child(option.label(&place)))
                        .child(if option == level {
                            icon("check", 13., t.text).into_any_element()
                        } else {
                            div().w(px(13.)).into_any_element()
                        })
                        .on_mouse_move(cx.listener(move |this, _, _, cx| {
                            if this.share.level_selected != index {
                                this.share.level_selected = index;
                                cx.notify();
                            }
                        }))
                        .on_click(cx.listener(move |this, _, _, cx| this.share_set_level(option, cx))),
                );
            }
            over.push(
                div()
                    .absolute()
                    .left(px(TEXT_X - 6.))
                    .top(relative(1.))
                    .mt(px(-7.))
                    .child(pop(options, Pop::menu((0.1, 0.)), menu))
                    .into_any_element(),
            );
        }
        let card = card.on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                // A press on the card itself closes whatever lies over it.
                if this.share.picker || this.share.level_menu {
                    this.share.picker = false;
                    this.share.level_menu = false;
                    cx.notify();
                }
                cx.stop_propagation();
            }),
        );
        // What lies over the card is drawn after all of it: a card's own
        // outline is drawn over its children.
        div()
            .absolute()
            .top(px(40.))
            .right(px(8.))
            .child(pop(div().relative().child(card).children(over), Pop::menu((0.8, 0.)), pose))
            .into_any_element()
    }

    /// The fill of what lies over the card: a step lighter than it in the
    /// dark theme, the card's own white in the light one.
    fn share_surface(&self) -> Hsla {
        if self.workspace.settings.dark_mode {
            rgb(0x40403f).into()
        } else {
            self.theme().card
        }
    }

    fn render_share_picker(&self, place: &str, cx: &mut Context<Self>) -> Div {
        use size::*;
        let t = self.theme();
        let share = &self.share;
        let searching = !share.answered.is_empty();
        // One highlight that travels between the rows, under the pointer
        // and under the arrow keys alike.
        let highlight = self.stage.glide("share-picker");
        if !self.stage.continuous(("share-picker-list", 0)) {
            highlight.rest();
        }
        let mut list = div()
            .occlude()
            .w_full()
            .pb(px(PICKER_INSET))
            .flex()
            .flex_col()
            .rounded(px(10.))
            .bg(self.share_surface())
            .border_1()
            .border_color(t.border)
            .shadow_lg()
            .occlude().on_any_mouse_down(|_, _, cx| cx.stop_propagation())
            .child(highlight.indicator(t.hover, 6.))
            .child(
                div()
                    .h(px(PICKER_CAPTION))
                    .px(px(FIELD_PAD))
                    .pt(px(3.5))
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .text_size(px(12.))
                    .text_color(t.muted)
                    .child(if searching { "Results" } else { "Suggested" }),
            );
        if share.listed.is_empty() {
            let typed = share.field.read(cx).content().trim().to_owned();
            let words = if !share.answered.is_empty() || typed.is_empty() {
                if searching {
                    format!("No one at {place} matches “{}”", share.answered)
                } else {
                    format!("Everyone at {place} already has access")
                }
            } else {
                "Searching…".to_string()
            };
            list = list.child(
                div()
                    .min_h(px(ROW_HEIGHT))
                    .px(px(FIELD_PAD))
                    .pb(px(6.))
                    .flex()
                    .items_center()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(t.muted)
                    .child(words),
            );
        }
        for (index, person) in share.listed.iter().take(PICKER_ROWS).enumerate() {
            let user = person.user_id.clone();
            let key = crate::lazy::key(&person.user_id);
            list = list.child(
                div()
                    .id(("share-candidate", key))
                    .relative()
                    .mx(px(PICKER_INSET))
                    .h(px(ROW_HEIGHT))
                    .px(px(FIELD_PAD - PICKER_INSET))
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap(px(7.))
                    .rounded(px(6.))
                    .cursor_default()
                    .when(share.selected == index, |row| row.child(highlight.probe(key)))
                    .child(self.share_avatar(person, None, PICKER_AVATAR, cx))
                    .child(self.share_lines(
                        div().truncate().child(person.display_name()).into_any_element(),
                        person.email.clone(),
                    ))
                    // The highlight is one: the pointer moves it as the arrows do.
                    .on_mouse_move(cx.listener(move |this, _, _, cx| {
                        if this.share.selected != index {
                            this.share.selected = index;
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.share_add(vec![user.clone()], Vec::new(), cx);
                    })),
            );
        }
        list
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // `gpui::*` re-exports its own `test` attribute; these are plain unit tests.
    use std::prelude::v1::test;

    fn person(name: &str) -> Person {
        Person {
            user_id: format!("id-of-{}", name.to_lowercase().replace(' ', "-")),
            name: name.into(),
            email: format!("{}@example.test", name.to_lowercase().replace(' ', ".")),
            ..Person::default()
        }
    }

    fn access() -> Access {
        Access {
            level: Level::Invited,
            organization: Some(Organization { name: "Acme".into(), domain: Some("example.test".into()) }),
            owner: person("Sam Carter"),
            people: vec![],
            attendees: vec![
                Person { has_access: false, ..person("Maya Okafor") },
                Person { has_access: false, ..person("Theo Lind") },
            ],
        }
    }

    #[test]
    fn the_server_s_answer_is_read_with_only_what_the_popover_shows() {
        let wire = r#"{
            "noteId": "3f0a1b7e-6f0e-4a55-9f7c-0d2c7f1f6a10", "level": "organization", "suspended": false,
            "organization": {"id": "3f0a1b7e-6f0e-4a55-9f7c-0d2c7f1f6a11", "name": "Acme", "domain": null},
            "owner": {"userId": "a", "name": "Sam Carter", "email": "sam@example.test"},
            "people": [{"userId": "b", "name": "", "email": "maya@example.test", "addedAt": "2026-11-16T10:00:00.000Z", "image": "https://lh3.googleusercontent.com/a/x"}],
            "attendees": [{"userId": "b", "name": "Maya", "email": "maya@example.test", "hasAccess": true}],
            "somethingNewer": 1
        }"#;
        let access: Access = serde_json::from_str(wire).unwrap();
        assert_eq!(access.level, Level::Organization);
        // No domain to name: the workspace goes by its name.
        assert_eq!(access.place(), "Acme");
        assert_eq!(access.people[0].display_name(), "maya@example.test");
        assert_eq!(access.people[0].initial(), "M");
        assert!(access.attendees[0].has_access);
        assert!(access.waiting_attendees().is_empty());
        assert!(serde_json::from_str::<Access>(r#"{"level": "public", "owner": {"userId": "a", "name": "", "email": ""}}"#).is_err());
    }

    #[test]
    fn the_two_levels_are_worded_for_the_workspace_and_there_is_no_third() {
        assert_eq!(Level::ALL.len(), 2);
        assert_eq!(Level::Invited.label("example.test"), "Only people invited");
        assert_eq!(Level::Organization.label("example.test"), "Anyone at example.test with the link");
        // The footer's wording leaves the link to the button beside it, so
        // it fits there.
        assert_eq!(Level::Invited.short_label("example.test"), "Only people invited");
        assert_eq!(Level::Organization.short_label("example.test"), "Anyone at example.test");
        assert_eq!(Level::Invited.wire(), "invited");
        assert_eq!(Level::Organization.wire(), "organization");
    }

    #[test]
    fn share_with_attendees_names_the_workspace_and_counts_who_is_in_it() {
        let mut access = access();
        assert_eq!(
            access.attendee_lines(),
            Some((
                "Only people from example.test (2)".into(),
                "maya.okafor@example.test, theo.lind@example.test".into()
            ))
        );
        assert_eq!(access.waiting_attendees().len(), 2);
        access.attendees[0].has_access = true;
        assert_eq!(access.waiting_attendees(), vec![access.attendees[1].user_id.clone()]);
        access.attendees.clear();
        assert_eq!(access.attendee_lines(), None);
        access.organization = None;
        assert_eq!(access.place(), "your organization");
    }

    #[test]
    fn the_picker_lists_attendees_first_and_never_the_owner_or_someone_with_access() {
        let mut access = access();
        let pool = vec![
            person("Sam Carter"),
            person("Priya Raman"),
            person("Theo Lind"),
            person("Jonas Weber"),
            person("Maya Okafor"),
        ];
        let names = |access: &Access, query: &str| -> Vec<String> {
            candidates(&pool, access, query).into_iter().map(|person| person.name).collect()
        };
        assert_eq!(names(&access, ""), ["Theo Lind", "Maya Okafor", "Priya Raman", "Jonas Weber"]);
        assert!(candidates(&pool, &access, "")[0].attendee);
        assert!(!candidates(&pool, &access, "")[2].attendee);
        assert_eq!(names(&access, "  PRI "), ["Priya Raman"]);
        assert_eq!(names(&access, "weber jonas"), ["Jonas Weber"]);
        assert_eq!(names(&access, "example.test"), ["Theo Lind", "Maya Okafor", "Priya Raman", "Jonas Weber"]);
        assert!(names(&access, "sam").is_empty());
        assert!(names(&access, "elsewhere").is_empty());
        access.people.push(person("Priya Raman"));
        assert_eq!(names(&access, ""), ["Theo Lind", "Maya Okafor", "Jonas Weber"]);
    }

    #[test]
    fn only_a_whole_address_is_sent_to_the_server_as_one() {
        for address in ["kai@elsewhere.test", " kai@example.test ", "a.b+c@mail.example.test"] {
            assert!(is_address(address), "{address}");
        }
        for other in ["", "kai", "kai@", "@example.test", "kai@example", "kai @example.test", "kai@example.", "kai@.test", "a@b@c.test"] {
            assert!(!is_address(other), "{other}");
        }
        assert_eq!(encoded("maya o@example.test&x=1/../#"), "maya%20o%40example.test%26x%3D1%2F..%2F%23");
        assert!(crate::accounts::client::check_path(&format!("/api/v1/notes/x/access/candidates?q={}", encoded("a@b.test //x"))).is_ok());
    }

    #[test]
    fn a_person_keeps_one_colour_and_a_letter() {
        let maya = person("Maya Okafor");
        assert_eq!(tint(&maya), tint(&Person { name: "Someone Else".into(), ..maya.clone() }));
        assert_eq!(tint(&maya), tint(&Person { email: maya.email.to_uppercase(), ..maya.clone() }));
        assert_eq!(maya.initial(), "M");
        assert_eq!(Person { name: " ".into(), email: "".into(), ..Person::default() }.initial(), "?");
        assert_eq!(Person { name: "(éa)".into(), ..Person::default() }.initial(), "É");
    }
}
