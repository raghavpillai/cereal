//! Cereal's macOS notifications, delivered by the UserNotifications
//! framework (`native/Notifications.swift`).
//!
//! Where the line is drawn: a system notification is for something that
//! happens on its own and matters while the person is in another app — a
//! meeting about to start (planned ahead in `menubar.rs`), notes that
//! finished generating, a recording that stopped or lost an input, a session
//! that ended. Whatever answers something the person just did in the window
//! ("Note moved to trash", "Copied", the result of an update check) stays a
//! toast there (`Cereal::show_toast`).
//!
//! Each kind has its own switch in Settings → Notifications. macOS is asked
//! for permission when one is first needed — a switch turned on, the end of
//! the first recording, the first generation — and a toast says why just
//! before macOS asks (its own prompt carries no reason).
//!
//! Notices that only matter away from the window (notes ready, a recording
//! problem) are not delivered while Cereal is in front with its window on
//! screen. A notice is taken out of Notification Centre once it no longer
//! holds: its note was opened, the account was signed in to again.
use crate::{
    accounts::AccountId,
    app::{Cereal, Screen},
    desktop_preferences::MeetingPreferences,
};
use gpui::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    /// One minute before a meeting starts. Planned ahead (`menubar.rs`).
    Meeting,
    /// A note's notes are ready, or could not be made.
    Notes,
    /// A recording stopped on its own, or lost the microphone or system audio.
    Recording,
    /// An account's session ended: it needs signing in to again.
    Account,
}

impl Kind {
    pub const ALL: [Kind; 4] = [Kind::Meeting, Kind::Notes, Kind::Recording, Kind::Account];

    /// Its category with macOS, and its word in what a click reports.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Meeting => "meeting",
            Kind::Notes => "notes",
            Kind::Recording => "recording",
            Kind::Account => "account",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.name() == name)
    }

    /// Whether its switch in Settings is on.
    pub fn enabled(self, preferences: &MeetingPreferences) -> bool {
        match self {
            Kind::Meeting => preferences.scheduled_notifications,
            Kind::Notes => preferences.notes_notifications,
            Kind::Recording => preferences.recording_notifications,
            Kind::Account => preferences.account_notifications,
        }
    }

    pub fn set(self, preferences: &mut MeetingPreferences, on: bool) {
        match self {
            Kind::Meeting => preferences.scheduled_notifications = on,
            Kind::Notes => preferences.notes_notifications = on,
            Kind::Recording => preferences.recording_notifications = on,
            Kind::Account => preferences.account_notifications = on,
        }
    }

    /// Its row in Settings → Notifications: id, icon, title, description.
    pub fn row(self) -> (&'static str, &'static str, &'static str, &'static str) {
        match self {
            Kind::Meeting => (
                "setting-scheduled-notifications",
                "bell",
                "Scheduled meetings",
                "Notify 1 minute before a meeting on your calendar starts. Click to open its note, or start recording from the notification.",
            ),
            Kind::Notes => (
                "setting-notes-notifications",
                "sparkles",
                "Notes ready",
                "When generated notes are ready while you are in another app. Click to open them.",
            ),
            Kind::Recording => (
                "setting-recording-notifications",
                "mic",
                "Recording problems",
                "When a recording stops on its own or loses the microphone or system audio.",
            ),
            Kind::Account => (
                "setting-account-notifications",
                "person",
                "Sign-in needed",
                "When your session on a server ends, so reminders and transcription do not stop without a word.",
            ),
        }
    }
}

/// What macOS has said about notifications from this app.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Permission {
    /// Not heard yet in this launch.
    #[default]
    Unknown,
    /// macOS has not asked the person yet.
    Undetermined,
    Granted,
    Denied,
    /// A bare executable, which macOS delivers nothing for.
    Unavailable,
}

impl Permission {
    fn of(answer: &str) -> Self {
        match answer {
            "granted" => Permission::Granted,
            "denied" => Permission::Denied,
            "undetermined" => Permission::Undetermined,
            "unavailable" => Permission::Unavailable,
            _ => Permission::Unknown,
        }
    }
}

/// Why macOS is being asked right now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ask {
    /// A switch was turned on (it is on already).
    Turn,
    /// A notice may soon be needed (see `notifications_first_need`), or the
    /// "Allow…" button.
    Need,
}

/// What the Settings page says about permission, above the switches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Standing {
    /// Allowed, or not heard yet: nothing to say.
    Allowed,
    /// macOS has not asked yet: an "Allow…" button.
    NotAsked,
    /// Asked in this launch and not allowed yet. On macOS 26 the request
    /// answers "denied" while macOS's own Allow banner is still on screen,
    /// so this is not taken as a refusal: the switches stay on, and the
    /// minute check turns them to working once Cereal is allowed.
    Waiting,
    /// Turned off for Cereal in System Settings.
    Refused,
    /// A bare executable.
    Unavailable,
}

pub fn standing(permission: Permission, asked_now: bool) -> Standing {
    match permission {
        Permission::Granted | Permission::Unknown => Standing::Allowed,
        Permission::Undetermined if asked_now => Standing::Waiting,
        Permission::Undetermined => Standing::NotAsked,
        Permission::Denied if asked_now => Standing::Waiting,
        Permission::Denied => Standing::Refused,
        Permission::Unavailable => Standing::Unavailable,
    }
}

/// Why a notice is wanted, said in a toast just before macOS asks.
fn reason(kind: Kind) -> &'static str {
    match kind {
        Kind::Recording => {
            "Cereal is asking macOS to send you notifications, so you hear about it if a recording stops while you are in another app."
        }
        _ => {
            "Cereal is asking macOS to send you notifications, so you know when your notes are ready while you are in another app."
        }
    }
}

/// A notice for macOS to deliver now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    /// Its identifier with macOS. A notice posted again under it replaces
    /// the one in Notification Centre.
    pub id: String,
    pub kind: Kind,
    pub title: String,
    pub body: String,
    /// What a click opens (see [`route`]).
    pub target: String,
    /// Delivered only while Cereal is not the app in front with its
    /// window on screen.
    pub background: bool,
    /// With a sound. A notice that replaces one already said has none.
    pub sound: bool,
}

impl Notice {
    fn json(&self) -> String {
        serde_json::json!({
            "id": self.id,
            "kind": self.kind.name(),
            "title": self.title,
            "body": self.body,
            "target": self.target,
            "background": self.background,
            "sound": self.sound,
        })
        .to_string()
    }
}

/// A note's target: `<account>/<note>`, or `/<note>` in local mode.
fn note_target(note: u64) -> String {
    let account = crate::accounts::of_note(note)
        .map(|account| account.id.as_str().to_owned())
        .unwrap_or_default();
    format!("{account}/{note}")
}

/// What a note is called in a notice.
fn note_title(title: &str) -> String {
    let title = title.trim();
    if title.is_empty() {
        "Untitled note".into()
    } else {
        crate::menubar::shorten(title, 80)
    }
}

pub fn notes_ready(note: u64, title: &str) -> Notice {
    Notice {
        id: format!("notes-{note}"),
        kind: Kind::Notes,
        title: "Notes ready".into(),
        body: note_title(title),
        target: note_target(note),
        background: true,
        sound: true,
    }
}

pub fn notes_failed(note: u64, title: &str, error: &str) -> Notice {
    Notice {
        id: format!("notes-{note}"),
        kind: Kind::Notes,
        title: format!("Notes for {} could not be made", note_title(title)),
        body: error.to_owned(),
        target: note_target(note),
        background: true,
        sound: true,
    }
}

/// A recording that `stopped` on its own, or lost an input and carries on.
/// One notice per note: the stop replaces the input that went first.
pub fn recording_problem(
    note: u64,
    title: &str,
    stopped: bool,
    error: Option<&str>,
    sound: bool,
) -> Notice {
    Notice {
        id: format!("recording-{note}"),
        kind: Kind::Recording,
        title: if stopped {
            "Recording stopped".into()
        } else {
            "Recording problem".into()
        },
        body: match (error, stopped) {
            (Some(error), true) => format!("{}: {error}. What was captured is saved.", note_title(title)),
            (None, true) => format!("{} stopped on its own. What was captured is saved.", note_title(title)),
            (Some(error), false) => format!("{}: {error}. The rest keeps recording.", note_title(title)),
            (None, false) => note_title(title),
        },
        target: note_target(note),
        background: true,
        sound,
    }
}

/// The identifier of an account's "Signed out" notice.
fn session_ended_id(account: Option<&AccountId>, host: &str) -> String {
    format!("account-{}", account.map(AccountId::as_str).unwrap_or(host))
}

pub fn session_ended(account: Option<&AccountId>, host: &str, recording: bool) -> Notice {
    Notice {
        id: session_ended_id(account, host),
        kind: Kind::Account,
        title: format!("Signed out of {host}"),
        body: if recording {
            "The recording continues on this Mac. Sign in again to keep transcribing it.".into()
        } else {
            "Your session ended. Sign in again to keep meeting reminders and syncing.".into()
        },
        target: account.map(|id| id.as_str().to_owned()).unwrap_or_default(),
        background: false,
        sound: true,
    }
}

/// What has been said about the recording under way, so that a problem is
/// said once: an input lost a moment before the capture stops is said
/// together with the stop, and a later notice for the same recording
/// replaces the earlier one without a sound.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RecordingNotices {
    pub note: u64,
    pub epoch: u64,
    /// What the capture already lacked when it went live (no microphone,
    /// microphone access denied): a setup state, said in the window only.
    baseline: Option<String>,
    /// Problems already said, or waiting to be.
    said: Vec<String>,
    /// A notice was delivered for this recording.
    posted: bool,
    /// The capture stopped on its own, and that was said.
    stopped: bool,
    /// The lost input waiting to be said, if the capture does not stop first.
    pub waiting: u64,
}

/// How long a lost input waits, in case the capture stops because of it.
pub const LOST_INPUT_WAIT: std::time::Duration = std::time::Duration::from_secs(6);

impl RecordingNotices {
    pub fn begin(note: u64, epoch: u64, baseline: Option<String>) -> Self {
        Self {
            note,
            epoch,
            baseline,
            ..Self::default()
        }
    }

    pub fn of(&self, note: u64, epoch: u64) -> bool {
        self.note == note && self.epoch == epoch
    }

    /// The capture reports `error` while it records. Whether it is new:
    /// then it is said after [`LOST_INPUT_WAIT`] unless the capture stops.
    pub fn lost(&mut self, error: &str) -> bool {
        if self.stopped
            || self.baseline.as_deref() == Some(error)
            || self.said.iter().any(|said| said == error)
        {
            return false;
        }
        self.said.push(error.to_owned());
        self.waiting += 1;
        true
    }

    /// The wait for lost input `token` is over: with what sound to say it,
    /// or nothing (the capture stopped meanwhile, or a newer one waits).
    pub fn say_lost(&mut self, token: u64) -> Option<bool> {
        if self.stopped || token != self.waiting {
            return None;
        }
        let sound = !self.posted;
        self.posted = true;
        Some(sound)
    }

    /// The capture stopped on its own: with what sound to say it, or
    /// nothing if that was said already.
    pub fn stop(&mut self) -> Option<bool> {
        if self.stopped {
            return None;
        }
        self.stopped = true;
        let sound = !self.posted;
        self.posted = true;
        Some(sound)
    }
}

/// Where a click on a notification, or one of its actions, leads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Route {
    /// A meeting: its note, recording as well when `record`. `title` is
    /// the notification's, for a note of its own when the event cannot be
    /// opened.
    Event {
        account: Option<AccountId>,
        event: String,
        record: bool,
        title: String,
    },
    /// A note, if it is still the same account's.
    Note {
        account: Option<AccountId>,
        note: u64,
    },
    /// Signing in to an account again.
    SignIn { account: Option<AccountId> },
}

/// Read what the native side reports: `<kind>\t<action>\t<target>\t<title>`,
/// where the action is `open` for a click on the notification itself.
pub fn route(argument: &str) -> Option<Route> {
    let mut parts = argument.splitn(4, '\t');
    let kind = Kind::parse(parts.next()?)?;
    let action = parts.next()?;
    let target = parts.next().unwrap_or_default();
    let title = parts.next().unwrap_or_default().trim();
    Some(match kind {
        Kind::Meeting => {
            let (account, event) = crate::menubar::reminder_target(target);
            if event.is_empty() {
                return None;
            }
            Route::Event {
                account,
                event: event.to_owned(),
                record: action == "record",
                title: title.to_owned(),
            }
        }
        Kind::Notes | Kind::Recording => {
            let (account, note) = target.rsplit_once('/')?;
            Route::Note {
                account: AccountId::parse(account),
                note: note.parse().ok()?,
            }
        }
        Kind::Account => Route::SignIn {
            account: AccountId::parse(target),
        },
    })
}

impl Cereal {
    /// Hand `notice` to macOS if its switch is on and macOS allows it.
    /// Returns whether it was handed over.
    pub fn post_notification(&mut self, notice: Notice) -> bool {
        if !self.menu_bar.installed
            || self.menu_bar.permission != Permission::Granted
            || !notice.kind.enabled(&crate::desktop_preferences::meeting())
        {
            return false;
        }
        let json = notice.json();
        if logging() {
            eprintln!("notifications post {json}");
        }
        native::post(&json);
        true
    }

    /// Take notices that no longer hold out of Notification Centre.
    fn withdraw_notifications(&mut self, ids: Vec<String>) {
        if !self.menu_bar.installed || self.menu_bar.permission != Permission::Granted {
            return;
        }
        if logging() {
            eprintln!("notifications withdraw {ids:?}");
        }
        native::remove(&ids);
    }

    /// Note `note` is on screen: what its notices said is seen.
    pub fn withdraw_note_notifications(&mut self, note: u64) {
        self.withdraw_notifications(vec![format!("notes-{note}"), format!("recording-{note}")]);
    }

    /// `account` is signed in to again: its "Signed out" notice is over.
    pub fn withdraw_session_notification(&mut self, account: &AccountId) {
        self.withdraw_notifications(vec![session_ended_id(Some(account), "")]);
    }

    /// Notes finished generating for `note` (or failed with `error`).
    pub fn notify_notes(&mut self, note: u64, error: Option<&str>) {
        let Some(title) = self.workspace.note(note).map(|note| note.title.clone()) else {
            return;
        };
        self.post_notification(match error {
            None => notes_ready(note, &title),
            Some(error) => notes_failed(note, &title, error),
        });
    }

    /// The recording of `note` (`epoch`) has its first audio. What the
    /// capture lacks already (`error`) is a setup state, said in the window.
    pub fn recording_went_live(&mut self, note: u64, epoch: u64, error: Option<String>) {
        self.menu_bar.recording_notices = Some(RecordingNotices::begin(note, epoch, error));
        self.withdraw_notifications(vec![format!("recording-{note}")]);
    }

    /// The recording lost an input and carries on. Said once, a little
    /// later, unless the capture stops because of it meanwhile (then the
    /// stop says it).
    pub fn notify_recording_lost(
        &mut self,
        note: u64,
        epoch: u64,
        error: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(notices) = self
            .menu_bar
            .recording_notices
            .as_mut()
            .filter(|notices| notices.of(note, epoch))
        else {
            return;
        };
        if !notices.lost(error) {
            return;
        }
        let token = notices.waiting;
        let error = error.to_owned();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(LOST_INPUT_WAIT).await;
            let _ = this.update(cx, |this, _| {
                let recording = this.recording.note_id == Some(note)
                    && this.recording.epoch == epoch
                    && this.recording.is_active();
                let Some(sound) = this
                    .menu_bar
                    .recording_notices
                    .as_mut()
                    .filter(|notices| recording && notices.of(note, epoch))
                    .and_then(|notices| notices.say_lost(token))
                else {
                    return;
                };
                let Some(title) = this.workspace.note(note).map(|note| note.title.clone()) else {
                    return;
                };
                this.post_notification(recording_problem(note, &title, false, Some(&error), sound));
            });
        })
        .detach();
    }

    /// The capture of a live recording stopped on its own. A recording
    /// that never went live failed at its start, which the window says.
    pub fn notify_recording_stopped(&mut self, note: u64, epoch: u64, error: Option<&str>) {
        let Some(sound) = self
            .menu_bar
            .recording_notices
            .as_mut()
            .filter(|notices| notices.of(note, epoch))
            .and_then(RecordingNotices::stop)
        else {
            return;
        };
        let Some(title) = self.workspace.note(note).map(|note| note.title.clone()) else {
            return;
        };
        self.post_notification(recording_problem(note, &title, true, error, sound));
    }

    /// The person stopped the recording of `note`: a "lost input" notice
    /// about it no longer holds, and macOS may now be asked, if it never
    /// was, for the next recording's sake.
    pub fn recording_stopped_by_person(&mut self, note: u64, cx: &mut Context<Self>) {
        if self
            .menu_bar
            .recording_notices
            .take()
            .is_some_and(|notices| notices.note == note && notices.posted)
        {
            self.withdraw_notifications(vec![format!("recording-{note}")]);
        }
        self.notifications_first_need(Kind::Recording, cx);
    }

    /// An account's session ended. Said by macOS when it can be; otherwise
    /// (no permission, the switch off, a bare executable) in a toast, as
    /// before — but never over a recording, which the toast would cover.
    pub fn notify_session_ended(
        &mut self,
        account: &AccountId,
        host: &str,
        recording: bool,
        cx: &mut Context<Self>,
    ) {
        if self.post_notification(session_ended(Some(account), host, recording)) {
            return;
        }
        if recording {
            self.show_toast(
                &format!(
                    "Your session on {host} has ended. The recording continues on this Mac; sign in again to keep transcribing it."
                ),
                cx,
            );
        } else if !self.recording.is_active() {
            self.show_toast(&format!("Your session on {host} has ended. Sign in again."), cx);
        }
    }

    /// Something that will want a notice of `kind` has happened (a
    /// recording ended, a generation began). If macOS has not asked the
    /// person yet and that switch is on, it asks now, once per launch, after
    /// a toast that says why.
    pub fn notifications_first_need(&mut self, kind: Kind, cx: &mut Context<Self>) {
        if !self.menu_bar.installed
            || self.menu_bar.permission != Permission::Undetermined
            || self.menu_bar.asking.is_some()
            || self.menu_bar.asked
            || !kind.enabled(&crate::desktop_preferences::meeting())
        {
            return;
        }
        self.menu_bar.asked = true;
        self.show_toast(reason(kind), cx);
        self.request_notifications(Ask::Need);
    }

    fn request_notifications(&mut self, ask: Ask) {
        self.menu_bar.asking = Some(ask);
        self.menu_bar.asked_now = true;
        native::request();
    }

    /// Whether a switch in Settings → Notifications is shown on: it is on,
    /// and macOS delivers, or may yet (it has not been asked, or was asked
    /// in this launch and has not answered for good).
    pub fn notification_switch_on(&self, kind: Kind, preferences: &MeetingPreferences) -> bool {
        kind.enabled(preferences)
            && !matches!(
                self.notification_standing(),
                Standing::Refused | Standing::Unavailable
            )
    }

    pub fn notification_standing(&self) -> Standing {
        standing(self.menu_bar.permission, self.menu_bar.asked_now)
    }

    /// A switch in Settings → Notifications. Turned on, it is saved on at
    /// once, and macOS is asked if it does not allow notifications yet; the
    /// switch works from the moment it does (see [`Standing::Waiting`]).
    pub fn set_notification(&mut self, kind: Kind, on: bool, cx: &mut Context<Self>) {
        if on && !self.menu_bar.installed {
            self.show_toast("Notifications are available on macOS", cx);
            return;
        }
        if let Err(error) = crate::desktop_preferences::update_meeting(|p| kind.set(p, on)) {
            self.show_toast(&error, cx);
        }
        if on && self.menu_bar.permission != Permission::Granted {
            if self.menu_bar.permission == Permission::Unavailable {
                self.show_toast("Notifications need the packaged Cereal app", cx);
            } else if self.menu_bar.asking.is_none() {
                self.request_notifications(Ask::Turn);
            }
        }
        cx.notify();
    }

    /// The button in Settings → Notifications when macOS has not asked yet.
    pub fn allow_notifications(&mut self, cx: &mut Context<Self>) {
        if self.menu_bar.installed && self.menu_bar.asking.is_none() {
            self.request_notifications(Ask::Need);
        }
        cx.notify();
    }

    /// Notifications → Cereal in System Settings, where a denial is undone.
    pub fn open_notification_settings(&mut self) {
        native::open_settings();
    }

    /// What macOS allows now (read at launch and once a minute, never a
    /// prompt). It does not end a request that is waiting for its answer.
    pub fn notifications_answered(&mut self, answer: &str, cx: &mut Context<Self>) {
        let permission = Permission::of(answer);
        if logging() {
            eprintln!("notifications permission {answer}");
        }
        if permission != self.menu_bar.permission {
            self.menu_bar.permission = permission;
            cx.notify();
        }
    }

    /// The answer to a request. "denied" is not final (see
    /// [`Standing::Waiting`]): the status is read again for what holds.
    pub fn notifications_request_answered(&mut self, answer: &str, cx: &mut Context<Self>) {
        let asking = self.menu_bar.asking.take();
        if logging() {
            eprintln!("notifications request {answer} (asked for {asking:?})");
        }
        match Permission::of(answer) {
            Permission::Granted => self.menu_bar.permission = Permission::Granted,
            Permission::Unavailable => {
                self.menu_bar.permission = Permission::Unavailable;
                if asking == Some(Ask::Turn) {
                    self.show_toast("Notifications need the packaged Cereal app", cx);
                }
            }
            _ => native::status(),
        }
        cx.notify();
    }

    /// A notification, or one of its buttons, was clicked.
    pub fn notification_chosen(
        &mut self,
        argument: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(route) = route(argument) else {
            return;
        };
        if logging() {
            eprintln!("notifications route {route:?}");
        }
        self.reveal_main_window(window, cx);
        if self.screen == Screen::Welcome {
            self.show_toast("Finish setting up Cereal first", cx);
            return;
        }
        // The account and sign-in pages cover any note: a note is opened
        // from the notes (which sends to the account page again when no
        // account's notes can be shown).
        if !matches!(route, Route::SignIn { .. })
            && matches!(self.screen, Screen::Account | Screen::Login)
        {
            self.navigate(Screen::Home, cx);
        }
        match route {
            Route::Event {
                account,
                event,
                record,
                title,
            } => self.meeting_chosen(account, event, record, title, window, cx),
            Route::Note { account, note } => self.note_chosen(account, note, cx),
            Route::SignIn { account } => self.sign_in_page(account.as_ref(), cx),
        }
        cx.notify();
    }

    /// Where `account` is signed in to again.
    fn sign_in_page(&mut self, account: Option<&AccountId>, cx: &mut Context<Self>) {
        if account.and_then(crate::accounts::get).is_some() && crate::accounts::any_shown() {
            // Another account is still in use: the accounts page, where
            // this one is signed in to again.
            self.settings_tab = crate::screens::SettingsTab::Profile;
            self.navigate(Screen::Settings, cx);
        } else {
            self.navigate(Screen::Account, cx);
        }
    }

    /// A meeting reminder, or its "Start recording". Never nothing: the
    /// event's note opens (recording, when asked), or, when the event
    /// cannot be opened on this Mac, "Start recording" records in a new note
    /// named after the meeting and a click says why.
    fn meeting_chosen(
        &mut self,
        account: Option<AccountId>,
        event: String,
        record: bool,
        title: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let calendar = self
            .calendars
            .account
            .as_ref()
            .and_then(crate::accounts::get)
            .filter(|calendar| account.is_none() || account.as_ref() == Some(&calendar.id));
        let openable = calendar.as_ref().is_some_and(|calendar| calendar.is_ready());
        if record && self.recording.is_active() {
            self.show_toast("A recording is already running. Stop it to record this meeting.", cx);
            if openable && self.calendars.opening.is_none() {
                self.open_calendar_event_and(event, false, cx);
            }
            return;
        }
        if openable {
            if self.calendars.opening.is_some() {
                self.show_toast("Still opening a meeting. Try again in a moment.", cx);
            } else {
                self.open_calendar_event_and(event, record, cx);
            }
            return;
        }
        let why = match &calendar {
            Some(calendar) => format!("{} can't be reached", calendar.host()),
            None => "its calendar is not connected on this Mac now".to_owned(),
        };
        if !record {
            self.show_toast(&format!("This meeting can't be opened: {why}."), cx);
            return;
        }
        self.new_note(window, cx);
        let Some(note) = self
            .current_note
            .filter(|note| self.recording.note_id == Some(*note) && self.recording.is_active())
        else {
            // `new_note` said why it could not.
            return;
        };
        if !title.is_empty() {
            if let Some(saved) = self.workspace.note_mut(note) {
                saved.title = title.clone();
            }
            self.title_input
                .update(cx, |input, cx| input.set_content(title, cx));
            self.persist(cx);
        }
        self.show_toast(&format!("Recording in a new note: the meeting's own note can't be opened ({why})."), cx);
    }

    /// "Notes ready" or a recording notice. A note in the Trash opens the
    /// Trash; one whose account is signed out, or that is gone, says so.
    fn note_chosen(&mut self, account: Option<AccountId>, note: u64, cx: &mut Context<Self>) {
        let owner = crate::accounts::of_note(note);
        if owner.as_ref().map(|owner| &owner.id) != account.as_ref() {
            self.show_toast("This note's account is no longer on this Mac", cx);
            return;
        }
        let Some((trashed, hidden)) = self
            .workspace
            .note(note)
            .map(|found| (found.trashed, self.workspace.is_hidden(found)))
        else {
            self.show_toast("This note was deleted", cx);
            return;
        };
        if hidden {
            let host = owner.as_ref().map(|owner| owner.host()).unwrap_or_default();
            self.sign_in_page(account.as_ref(), cx);
            self.show_toast(&format!("Sign in to {host} again to open this note"), cx);
        } else if trashed {
            // Where it can be restored from.
            self.open_trash_at(note, cx);
            self.show_toast("This note is in the Trash", cx);
        } else {
            self.open_note(note, cx);
        }
    }

    /// Debug builds with their own `CEREAL_DATA_DIR`:
    /// `CEREAL_DEBUG_NOTIFY=<steps>` runs, a few seconds after launch (time
    /// to bring another app forward), comma-separated steps that post real
    /// notifications through the same paths as the app: `meeting`, `notes`,
    /// `recording`, `account`, `ask` (the first-need request), `settings`
    /// (Settings → Notifications on screen), `on-<kind>` and `off-<kind>`
    /// (what that switch does), `home`, `open` (the first note), `stop`
    /// (the recording, as its button does), `list` (what is in Notification
    /// Centre, logged) and `clear` (take this app's notifications out of
    /// Notification Centre).
    #[cfg(debug_assertions)]
    pub fn debug_notifications(&mut self, step: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((on, kind)) = step
            .strip_prefix("on-")
            .map(|kind| (true, kind))
            .or_else(|| step.strip_prefix("off-").map(|kind| (false, kind)))
            && let Some(kind) = Kind::parse(kind)
        {
            self.set_notification(kind, on, cx);
            return;
        }
        let note = self.current_note.or_else(|| self.workspace.notes.first().map(|n| n.id));
        match step {
            "meeting" => {
                // A reminder due at once, through the same path as planned ones.
                let fire = crate::menubar::now() + chrono::Duration::seconds(1);
                let starts = (fire + chrono::Duration::minutes(1)).format("%-I:%M %p");
                let reminder = serde_json::json!({
                    "id": "meeting-debug-event",
                    "title": "Design review",
                    "body": format!("Starts at {starts}"),
                    "fireAt": fire.timestamp(),
                    "eventId": "debug-event",
                });
                let change = serde_json::json!({ "remove": [], "add": [reminder] }).to_string();
                if crate::desktop_preferences::meeting().scheduled_notifications {
                    native::apply(&change);
                }
            }
            "notes" => {
                if let Some(note) = note {
                    self.notify_notes(note, None);
                }
            }
            "recording" => {
                if let Some(note) = note {
                    // A live recording whose capture then stops on its own.
                    self.recording_went_live(note, u64::MAX, None);
                    self.notify_recording_stopped(
                        note,
                        u64::MAX,
                        Some("Microphone: The input device was disconnected"),
                    );
                }
            }
            "account" => {
                self.post_notification(session_ended(None, "notes.example.com", false));
            }
            "ask" => self.notifications_first_need(Kind::Recording, cx),
            "settings" => {
                self.reveal_main_window(window, cx);
                self.settings_tab = crate::screens::SettingsTab::Notifications;
                self.navigate(Screen::Settings, cx);
            }
            "home" => self.navigate(Screen::Home, cx),
            "open" => {
                if let Some(first) = self.workspace.notes.first().map(|n| n.id) {
                    self.open_note(first, cx);
                }
            }
            "stop" => self.stop_recording_preview(cx),
            "list" => native::delivered(),
            "clear" => native::clear_delivered(),
            _ => {}
        }
    }
}

pub(crate) fn logging() -> bool {
    static LOG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *LOG.get_or_init(|| {
        crate::environment::var_os("CEREAL_MENUBAR_LOG").is_some()
            || crate::environment::var_os("CEREAL_NOTIFICATIONS_LOG").is_some()
    })
}

/// Run the `CEREAL_DEBUG_NOTIFY` steps (see `Cereal::debug_notifications`).
#[cfg(debug_assertions)]
pub fn debug_install(handle: WindowHandle<Cereal>, cx: &mut App) {
    if crate::environment::var_os("CEREAL_DATA_DIR").is_none() {
        return;
    }
    let Ok(steps) = crate::environment::var("CEREAL_DEBUG_NOTIFY") else {
        return;
    };
    let delay = crate::environment::var("CEREAL_DEBUG_NOTIFY_DELAY")
        .ok()
        .and_then(|seconds| seconds.parse().ok())
        .unwrap_or(5u64);
    cx.spawn(async move |cx| {
        for step in steps.split(',').map(str::trim).map(str::to_owned) {
            cx.background_executor()
                .timer(std::time::Duration::from_secs(delay))
                .await;
            eprintln!("notifications debug step {step}");
            if handle
                .update(cx, |view, window, cx| view.debug_notifications(&step, window, cx))
                .is_err()
            {
                break;
            }
        }
    })
    .detach();
}

#[cfg(not(debug_assertions))]
pub fn debug_install(_: WindowHandle<Cereal>, _: &mut App) {}

#[cfg(target_os = "macos")]
pub(crate) mod native {
    use std::ffi::{CString, c_char};

    unsafe extern "C" {
        fn cereal_notifications_status();
        fn cereal_notifications_request();
        fn cereal_notifications_apply(json: *const c_char) -> bool;
        fn cereal_notifications_post(json: *const c_char) -> bool;
        fn cereal_notifications_remove(json: *const c_char) -> bool;
        fn cereal_notifications_delivered();
        fn cereal_notifications_clear();
        fn cereal_notifications_clear_delivered();
        fn cereal_notifications_open_settings();
    }

    fn with_json(json: &str, call: unsafe extern "C" fn(*const c_char) -> bool) {
        let Ok(json) = CString::new(json) else { return };
        unsafe {
            call(json.as_ptr());
        }
    }

    pub fn status() {
        unsafe { cereal_notifications_status() }
    }
    pub fn request() {
        unsafe { cereal_notifications_request() }
    }
    /// Meeting reminders to withdraw and to plan (see `menubar.rs`).
    pub fn apply(json: &str) {
        with_json(json, cereal_notifications_apply)
    }
    pub fn post(json: &str) {
        with_json(json, cereal_notifications_post)
    }
    /// Take these identifiers out of Notification Centre and the queue.
    pub fn remove(ids: &[String]) {
        with_json(
            &serde_json::to_string(ids).unwrap_or_default(),
            cereal_notifications_remove,
        )
    }
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub fn delivered() {
        unsafe { cereal_notifications_delivered() }
    }
    /// Every reminder still waiting.
    pub fn clear() {
        unsafe { cereal_notifications_clear() }
    }
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub fn clear_delivered() {
        unsafe { cereal_notifications_clear_delivered() }
    }
    pub fn open_settings() {
        unsafe { cereal_notifications_open_settings() }
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) mod native {
    pub fn status() {}
    pub fn request() {}
    pub fn apply(_: &str) {}
    pub fn post(_: &str) {}
    pub fn remove(_: &[String]) {}
    pub fn delivered() {}
    pub fn clear() {}
    pub fn clear_delivered() {}
    pub fn open_settings() {}
}

#[cfg(test)]
mod tests {
    // Named one by one: `gpui::*` above has a `test` attribute of its own.
    use super::{
        AccountId, Kind, MeetingPreferences, Permission, RecordingNotices, Route, Standing,
        notes_failed, notes_ready, recording_problem, route, session_ended, standing,
    };

    const ACCOUNT: &str = "6f1c2a3b-4d5e-5f60-8172-839405a6b7c8";

    #[test]
    fn a_click_and_each_action_lead_where_they_say() {
        let account = AccountId::parse(ACCOUNT);
        assert!(account.is_some());
        // A meeting: a click opens its note, the button records as well.
        assert_eq!(
            route(&format!("meeting\topen\t{ACCOUNT}/evt-1")),
            Some(Route::Event {
                account: account.clone(),
                event: "evt-1".into(),
                record: false,
                title: String::new(),
            })
        );
        // The button records; the title names a note of its own if needed.
        assert_eq!(
            route(&format!("meeting\trecord\t{ACCOUNT}/evt-1\tDesign review")),
            Some(Route::Event {
                account: account.clone(),
                event: "evt-1".into(),
                record: true,
                title: "Design review".into(),
            })
        );
        // A reminder from before accounts names its event alone.
        assert_eq!(
            route("meeting\topen\tevt-2"),
            Some(Route::Event {
                account: None,
                event: "evt-2".into(),
                record: false,
                title: String::new(),
            })
        );
        // Notes ready and a recording problem open their note, the button
        // and the click alike; local mode's notes have no account.
        assert_eq!(
            route(&format!("notes\topen\t{ACCOUNT}/1099511627777")),
            Some(Route::Note {
                account: account.clone(),
                note: 1_099_511_627_777
            })
        );
        assert_eq!(
            route("recording\topen\t/42"),
            Some(Route::Note {
                account: None,
                note: 42
            })
        );
        assert_eq!(
            route(&format!("account\tsign-in\t{ACCOUNT}")),
            Some(Route::SignIn {
                account: account.clone()
            })
        );
        // Anything else is nobody's.
        for argument in ["", "update\topen\tx", "notes\topen\tnot-a-note", "meeting\topen\t"] {
            assert_eq!(route(argument), None, "{argument:?}");
        }
    }

    #[test]
    fn notices_say_what_happened_and_replace_their_own_kind_for_a_note() {
        let ready = notes_ready(42, "  ");
        assert_eq!((ready.title.as_str(), ready.body.as_str()), ("Notes ready", "Untitled note"));
        assert!(ready.background, "In front, the window already shows the notes");
        let failed = notes_failed(42, "Weekly sync", "The model is unavailable");
        assert_eq!(failed.id, ready.id, "A failure replaces the earlier notice for that note");
        assert_eq!(failed.body, "The model is unavailable");

        let lost = recording_problem(7, "Standup", false, Some("System audio: lost"), true);
        let stopped = recording_problem(7, "Standup", true, Some("Microphone: disconnected"), false);
        assert_eq!(lost.id, stopped.id, "The stop replaces the lost input");
        assert_eq!(stopped.title, "Recording stopped");
        assert!(stopped.body.ends_with("What was captured is saved."));
        assert!(stopped.background, "In front, the window shows the recording's error");
        assert!(lost.sound && !stopped.sound);

        let account = AccountId::parse(ACCOUNT);
        let ended = session_ended(account.as_ref(), "notes.example.com", true);
        assert_eq!(ended.title, "Signed out of notes.example.com");
        assert!(ended.body.contains("recording continues"));
        assert_eq!(
            route(&format!("account\tsign-in\t{}", ended.target)),
            Some(Route::SignIn { account })
        );
        let json: serde_json::Value = serde_json::from_str(&ready.json()).unwrap();
        assert_eq!(json["kind"], "notes");
        assert_eq!(json["background"], true);
    }

    #[test]
    fn each_switch_controls_its_own_kind() {
        let mut all_on = MeetingPreferences::default();
        for kind in Kind::ALL {
            assert_eq!(Kind::parse(kind.name()), Some(kind));
            kind.set(&mut all_on, true);
        }
        for kind in Kind::ALL {
            let mut preferences = all_on.clone();
            kind.set(&mut preferences, false);
            for other in Kind::ALL {
                assert_eq!(other.enabled(&preferences), other != kind, "{kind:?} off, {other:?}");
            }
        }
    }

    #[test]
    fn a_recording_problem_is_said_once_and_a_stop_takes_in_the_input_lost_before_it() {
        // Set up without a microphone: said in the window, never notified.
        let mut notices = RecordingNotices::begin(7, 1, Some("Microphone: denied".into()));
        assert!(notices.of(7, 1) && !notices.of(7, 2) && !notices.of(8, 1));
        assert!(!notices.lost("Microphone: denied"));
        // An input lost while recording waits; the same report again is not new.
        assert!(notices.lost("System audio: lost"));
        let first = notices.waiting;
        assert!(!notices.lost("System audio: lost"));
        // The capture stops before the wait is over: one notice, with sound,
        // and the lost input is not said on its own.
        assert_eq!(notices.stop(), Some(true));
        assert_eq!(notices.say_lost(first), None);
        assert_eq!(notices.stop(), None, "A stop is said once");
        assert!(!notices.lost("Microphone: lost"), "Nothing after the stop");

        // An input lost and still lost after the wait is said, with sound; a
        // later stop replaces it without one.
        let mut notices = RecordingNotices::begin(7, 2, None);
        assert!(notices.lost("Microphone: lost"));
        assert!(notices.lost("Microphone: lost · System audio: lost"));
        let older = notices.waiting - 1;
        assert_eq!(notices.say_lost(older), None, "Only the newest waiting report is said");
        assert_eq!(notices.say_lost(notices.waiting), Some(true));
        assert_eq!(notices.stop(), Some(false));
    }

    #[test]
    fn a_denial_while_macos_is_still_asking_is_waiting_not_refused() {
        assert_eq!(standing(Permission::Granted, true), Standing::Allowed);
        assert_eq!(standing(Permission::Unknown, false), Standing::Allowed);
        assert_eq!(standing(Permission::Undetermined, false), Standing::NotAsked);
        assert_eq!(standing(Permission::Undetermined, true), Standing::Waiting);
        // macOS 26 answers "denied" with its Allow banner still on screen.
        assert_eq!(standing(Permission::Denied, true), Standing::Waiting);
        // Not asked in this launch: turned off in System Settings.
        assert_eq!(standing(Permission::Denied, false), Standing::Refused);
        assert_eq!(standing(Permission::Unavailable, true), Standing::Unavailable);
    }

    #[test]
    fn permission_words_from_macos() {
        assert_eq!(Permission::of("granted"), Permission::Granted);
        assert_eq!(Permission::of("denied"), Permission::Denied);
        assert_eq!(Permission::of("undetermined"), Permission::Undetermined);
        assert_eq!(Permission::of("unavailable"), Permission::Unavailable);
        assert_eq!(Permission::of(""), Permission::Unknown);
    }
}
