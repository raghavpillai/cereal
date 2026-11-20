use crate::{
    input::{InputChanged, InputFocused, InputSubmitted, TextInput},
    model::Workspace,
    ui::*,
};
use gpui::{prelude::*, *};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Screen {
    /// The first-run welcome (see `onboarding_view.rs`).
    Welcome,
    Login,
    Account,
    Home,
    MyNotes,
    SearchResults,
    Favorites,
    Shared,
    /// The team space: notes shown to the whole workspace (`team_space.rs`).
    TeamSpace,
    /// Someone else's note from the team space, read-only.
    TeamNote,
    /// The page a link opened: a shared note, read-only, or where a note
    /// is when the app is not signed in to its server (see `shared_view`).
    Link,
    Chat,
    Settings,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NoteTab {
    Notes,
    Enhanced,
}
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Menu {
    /// The transcript panel's footer: the sound settings popover, the
    /// microphone menu and the transcript language menu.
    RecordingSettings,
    RecordingInput,
    RecordingLanguage,
    NoteDetails,
    CalendarEvent,
    RecordingHelp,
    Workspace,
    Note,
    Share,
    Templates,
    Shortcuts,
    /// Right-click menu of a note row in a list.
    NoteRow,
    /// A sheet about an account: signing out, erasing, signing in to a
    /// signed-out account, restoring a note deleted on the server. What it
    /// says is in `Cereal::account_sheet`.
    Account,
    /// The Trash popover, opened from the sidebar's trash button.
    Trash,
    /// Settings › Appearance: System, Light or Dark.
    Theme,
    /// Settings › Members (`workspace_settings.rs`): a row's "…" menu, the
    /// "Invite teammates" dialog, and the question before removing someone.
    Member,
    Invite,
    RemoveMember,
}

/// The one thing a toast can offer besides going away: "Undo", "Show it".
#[derive(Clone)]
pub struct ToastAction {
    pub label: SharedString,
    run: std::rc::Rc<dyn Fn(&mut Cereal, &mut Window, &mut Context<Cereal>)>,
}

/// How long a toast stays, and one that offers something to press.
const TOAST_FOR: std::time::Duration = std::time::Duration::from_secs(4);
const TOAST_WITH_ACTION_FOR: std::time::Duration = std::time::Duration::from_secs(6);
/// The toast is a pill in the title bar: this tall with one line, this far
/// down, at most this wide.
const TOAST_HEIGHT: f32 = 36.;
const TOAST_TOP: f32 = 6.;
const TOAST_WIDEST: f32 = 440.;
/// Narrower than this in the bar and its words would wrap: it goes under.
const TOAST_NARROWEST: f32 = 240.;
/// How far a toast stays from the title bar's own controls.
const TOAST_CLEAR: f32 = 10.;

/// Where the toast pill goes in a window `viewport` wide whose title bar
/// has controls reaching `left` and `right` in from its edges, for content
/// `natural` wide: its width, and its centre in the title bar, or `None`
/// when it has to hang under the bar because the bar has no room for it.
///
/// With nothing else in the middle of the bar it is centred on the window.
/// While the recording's pill is there (`pill` is its width) the toast goes
/// beside it, to its right if it fits there, else to its left: never under
/// it, on top of the page.
pub fn toast_place(
    natural: f32,
    viewport: f32,
    left: f32,
    right: f32,
    pill: Option<f32>,
) -> (f32, Option<f32>) {
    let wanted = natural.min(TOAST_WIDEST);
    let least = wanted.min(TOAST_NARROWEST);
    let (from, to) = (left + TOAST_CLEAR, viewport - right - TOAST_CLEAR);
    let under = (wanted.min((viewport - 32.).max(120.)), None);
    match pill {
        None => {
            let room = 2. * (viewport / 2. - from).min(to - viewport / 2.);
            if room >= least {
                (wanted.min(room), Some(viewport / 2.))
            } else {
                under
            }
        }
        Some(pill) => {
            let after = viewport / 2. + pill / 2. + TOAST_CLEAR;
            let before = viewport / 2. - pill / 2. - TOAST_CLEAR;
            if to - after >= least {
                let width = wanted.min(to - after);
                (width, Some(after + width / 2.))
            } else if before - from >= least {
                let width = wanted.min(before - from);
                (width, Some(before - width / 2.))
            } else {
                under
            }
        }
    }
}

pub struct Cereal {
    pub workspace: Workspace,
    pub login: Entity<crate::login::LoginView>,
    pub account: Entity<crate::account::AccountView>,
    pub screen: Screen,
    pub current_note: Option<u64>,
    pub sidebar_open: bool,
    /// In a narrow window the sidebar is not beside the page: it slides out
    /// over it, on a dimmed page, while this is set (`sidebar_narrow`).
    pub sidebar_overlay: bool,
    /// Whether the window is too narrow for the sidebar beside the page
    /// (`desktop_preferences::SIDEBAR_OVERLAY_BELOW`), as of the last frame.
    pub sidebar_narrow: bool,
    pub search_open: bool,
    pub search_results_query: String,
    pub search: Entity<TextInput>,
    pub ask: Entity<TextInput>,
    pub title_input: Entity<TextInput>,
    pub editor: Entity<TextInput>,
    pub note_body_dirty: bool,
    pub note_scroll: ScrollHandle,
    pub note_calendar_month: chrono::NaiveDate,
    pub attendee_input: Entity<TextInput>,
    pub active_tab: NoteTab,
    pub summary: crate::summary::SummaryState,
    pub assistant_open: bool,
    pub transcript_open: bool,
    pub transcript_epoch: u64,
    pub transcript_scroll: ScrollHandle,
    pub transcript_last_epoch: u64,
    pub transcript_last_text: String,
    pub transcript_last_query: String,
    pub transcript_find_index: usize,
    pub transcript_find_open: bool,
    /// The live-transcript revision the panel last followed to the bottom.
    pub transcript_last_live: u64,
    /// The transcript panel's own state (see `transcript_panel`).
    pub panel: crate::transcript_panel::PanelState,
    /// Live transcription of the current recording (see `live_view.rs`).
    pub live: crate::live_view::LiveTranscript,
    pub recording: crate::recording::Recording,
    pub audio_capture: Option<crate::audio::Capture>,
    pub audio_error: Option<String>,
    pub audio_pending: Vec<crate::audio::CaptureMonitor>,
    pub audio_quitting: bool,
    /// One serial worker per account (see `server.rs`).
    pub bridges: std::collections::HashMap<crate::accounts::AccountId, crate::server::Bridge>,
    /// Accounts whose notes list is being refreshed.
    pub refreshing: std::collections::HashSet<crate::accounts::AccountId>,
    /// Accounts whose saved session is being verified, and how often each
    /// has gone unanswered.
    pub verifying: std::collections::HashSet<crate::accounts::AccountId>,
    pub verify_failures: std::collections::HashMap<crate::accounts::AccountId, u32>,
    /// Accounts whose session ended, was replaced, or whose server did not
    /// answer, since they were last Ready: when they are Ready again, what
    /// they hold unsent is sent again.
    pub interrupted: std::collections::HashSet<crate::accounts::AccountId>,
    /// "Sign in to your server first" was said for this launch.
    pub sign_in_asked: bool,
    pub accounts_watched: bool,
    /// The sign-out or erase sheet that is open (see `screens.rs`).
    pub account_sheet: Option<crate::screens::AccountSheet>,
    /// What Settings lists of signed-out and older data on this Mac, read
    /// off the UI thread and kept until an account comes or goes.
    pub saved_data: Option<crate::accounts::saved::Listing>,
    pub saved_data_loading: bool,
    /// `CEREAL_DEBUG_VIEW` has been shown (debug builds).
    pub debug_view_shown: bool,
    pub server_pending: usize,
    pub server_status: Option<String>,
    pub generation: crate::recording::Generation,
    /// The notes of that generation as they are written (see `Draft`).
    pub generation_draft: crate::recording::Draft,
    pub recording_transcript_start: Option<(u64, String)>,
    pub regeneration_notice_dismissed: Option<(u64, u64)>,
    pub recording_from: crate::recording::Phase,
    pub recording_changed_at: Option<std::time::Instant>,
    pub recording_last_click: Option<(bool, std::time::Instant)>,
    pub microphone_muted: bool,
    pub menu: Option<Menu>,
    pub visible_menu: Option<Menu>,
    pub menu_epoch: u64,
    /// The menu a press outside it closed, and which press that was ([`Cereal::press_seq`]):
    /// the click that same press completes on the menu's own button must not open it again
    /// ([`Cereal::open_menu`]). Cleared once that press is released.
    pub menu_closed_by_press: Option<(Menu, u64)>,
    /// Counts presses of the pointer in the window, so a record above belongs to one press.
    pub press_seq: u64,
    /// What a Trash row is asking ("Delete … forever?", or that the note
    /// was edited elsewhere).
    pub trash_ask: Option<crate::trash::Ask>,
    /// The trashed note a link led to: its row is highlighted and scrolled
    /// into view when the popover is next drawn.
    pub trash_reveal: Option<u64>,
    pub trash_scroll: ScrollHandle,
    /// Notes whose delete on their server has been sent and not answered.
    pub deleting_forever: std::collections::HashSet<u64>,
    pub menu_keyboard: crate::chrome::MenuKeyboard,
    pub palette: crate::chrome::PaletteKeyboard,
    pub menu_exiting: bool,
    pub toast: Option<String>,
    /// The last toast's text, kept so it can finish leaving after `toast` clears.
    pub toast_shown: String,
    pub toast_epoch: u64,
    /// The one thing the toast offers to do ("Undo"), kept with its text
    /// while it leaves.
    pub toast_action: Option<ToastAction>,
    /// Counts the toasts shown: a new one swaps its words into the pill.
    pub toast_serial: u64,
    /// Animated values for this window (see `motion::Stage`).
    pub stage: crate::motion::Stage,
    /// How much of the sidebar's width is on screen right now, 0..=1.
    pub sidebar_level: f32,
    /// The sidebar's chosen width in points (dragged at its edge, saved with
    /// this Mac's layout preferences), within its limits.
    pub sidebar_width: f32,
    /// The width the sidebar is drawn at in this window: the chosen width,
    /// less what a narrow window needs for the page (see
    /// `desktop_preferences::sidebar_width_in`).
    pub sidebar_span: f32,
    /// While the sidebar's edge is being dragged: how far the pointer was
    /// from the edge when it took hold, so the edge follows it exactly.
    pub sidebar_grab: Option<f32>,
    /// The pointer is over the sidebar's edge.
    pub sidebar_edge_hovered: bool,
    /// How open the search dialog is right now.
    pub search_pose: crate::motion::Pose,
    /// Identity of the page on screen; a change replays the page entrance.
    pub page_key: u64,
    /// Debounced, off-thread workspace saves (see `store.rs`).
    pub autosave: crate::store::Autosave,
    /// Lists derived from the workspace, reused until it changes.
    pub caches: crate::home::Caches,
    /// The list last shown, so returning to it from a note does not replay
    /// its entrance.
    pub list_shown: Option<std::mem::Discriminant<Screen>>,
    /// Screens to return to with the header back pill, most recent last.
    pub nav_history: Vec<Screen>,
    pub settings_tab: crate::screens::SettingsTab,
    /// The note a list-row menu acts on,
    /// and where that menu is anchored.
    pub row_menu: Option<crate::chrome::RowMenu>,
    pub chat_models: Vec<meeting_notes_api_types::LanguageModelSelection>,
    pub selected_chat_model: Option<String>,
    pub chat_model_picker_open: bool,
    /// The row of the open model menu the arrow keys are on.
    pub chat_model_cursor: Option<usize>,
    pub chat_messages: Vec<(String, String)>,
    pub chat_annotations: std::collections::HashMap<usize, crate::chat::ChatTurnAnnotations>,
    pub chat_epoch: u64,
    pub chat_stream: Option<crate::chat::ActiveChat>,
    /// Questions sent while an answer was arriving; each is sent in turn
    /// when the answer before it has finished.
    pub chat_queue: Vec<String>,
    /// Answers still arriving for conversations that are not on screen, by
    /// conversation id.
    pub chat_parked: std::collections::HashMap<String, crate::chat::ParkedChat>,
    pub chat_interrupted: std::collections::HashSet<usize>,
    pub chat_editing: Option<crate::chat::EditingChat>,
    pub chat_maximized: bool,
    /// The sidebar lists every conversation under Chat, not only the newest.
    pub chat_threads_expanded: bool,
    /// The sidebar's conversations are folded away under Chat (the chevron
    /// at the end of its row).
    pub chat_threads_collapsed: bool,
    pub chat_history_open: bool,
    /// When a press outside the recent-chats list last closed it (see
    /// `toggle_chat_history`).
    pub chat_history_closed_at: Option<std::time::Instant>,
    /// When a press outside the model menu last closed it (see
    /// `toggle_chat_model_picker`).
    pub chat_model_picker_closed_at: Option<std::time::Instant>,
    pub chat_scroll: ScrollHandle,
    /// The conversation as it was last drawn (`chat_follow_signature`), so a
    /// reader at its end is kept there while it grows (`render_chat_turns`).
    pub chat_follow: std::cell::Cell<u64>,
    pub chat_archives: Vec<crate::chat::ArchivedChat>,
    /// Each account's saved threads on its server (`chat_sync.rs`).
    pub chat_sync: crate::chat_sync::ChatSync,
    pub chat_thread_id: String,
    pub calendar_page: i32,
    pub calendars: crate::calendar::CalendarState,
    pub home_scroll: ScrollHandle,
    pub meeting: crate::meeting::MeetingState,
    /// Links into the app and "Shared with me" (see `shared_view.rs`).
    pub links: crate::shared_view::Links,
    /// The Share popover of a note, and "Shared with me" as the server lists it (see `share_popover.rs`).
    pub share: crate::share_popover::Share,
    /// The team space page (`team_space.rs`).
    pub team: crate::team_space::TeamSpace,
    /// Settings › Workspace: its name and its members (`workspace_settings.rs`).
    pub org: crate::workspace_settings::Organizations,
    /// "Check for updates" (see `update_view.rs`).
    pub updates: crate::update_view::Updates,
    /// The first-run welcome: its step, and what this Mac has allowed.
    pub onboarding: crate::onboarding_view::Onboarding,
    /// The menu bar item (see `menubar.rs`).
    pub menu_bar: crate::menubar::State,
    pub focus: FocusHandle,
    pub _subscriptions: Vec<Subscription>,
    /// Registered with the window on the first frame: see `render`.
    focus_lost: Option<Subscription>,
    /// Redraws when macOS switches between light and dark, for the System
    /// theme. Registered with the window on the first frame.
    appearance_watch: Option<Subscription>,
    /// Where the theme button of Settings was last painted: its menu opens
    /// from there.
    pub theme_anchor: std::rc::Rc<std::cell::Cell<Bounds<Pixels>>>,
}

actions!(
    cereal,
    [
        NewNote,
        Search,
        Escape,
        GoHome,
        ToggleSidebar,
        Save,
        CloseWindow,
        Hide,
        Quit,
        QuitCompletely,
        OpenSettings,
        CheckForUpdates,
        FocusNext,
        FocusPrevious
    ]
);

impl Cereal {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let login = cx.new(crate::login::LoginView::new);
        let account = cx.new(crate::account::AccountView::new);
        let search = cx.new(|cx| TextInput::new("Search meetings", false, cx));
        let ask = cx.new(|cx| TextInput::new("Ask anything", false, cx));
        ask.update(cx, |input, cx| input.set_chat_mode(cx));
        let title_input = cx.new(|cx| TextInput::new("New note", false, cx));
        title_input.update(cx, |input, cx| input.set_wrap(true, cx));
        let editor =
            cx.new(|cx| TextInput::new("Write notes", true, cx));
        editor.update(cx, |input, _| input.set_list_formatting(true));
        let note_scroll = ScrollHandle::new();
        editor.update(cx, |input, _| input.attach_scroll(note_scroll.clone()));
        let attendee_input = cx.new(|cx| TextInput::new("Add attendees...", false, cx));
        let menu_key_listener = cx.listener(|this, event: &KeystrokeEvent, window, cx| {
            // The Share popover keeps the arrows, Return and Escape for its picker.
            if this.handle_share_key(event, window, cx) {
                return;
            }
            // The open model menu takes the arrows, Return and Escape from
            // the composer it opened from.
            if this.handle_chat_model_key(event, cx) {
                return;
            }
            this.handle_palette_key(event, window, cx);
            if this.focus.contains_focused(window, cx) {
                this.handle_menu_key(event, window, cx);
            }
        });
        let menu_key_subscription = cx.intercept_keystrokes(menu_key_listener);
        let (links, link_subscriptions) = crate::shared_view::Links::new(cx);
        let (share, share_subscriptions) = crate::share_popover::Share::new(cx);
        let (team, team_subscriptions) = crate::team_space::TeamSpace::new(cx);
        let (org, org_subscriptions) = crate::workspace_settings::Organizations::new(cx);
        // The welcome draws the account form's fields in its own layout.
        let (account_address, account_email) = {
            let account = account.read(cx);
            (account.url.clone(), account.email.clone())
        };
        let welcome = crate::onboarding::at_launch();
        if let Some(flow) = welcome {
            crate::onboarding::remember(flow, false);
        }
        let mut subscriptions = vec![
            menu_key_subscription,
            cx.observe(&account, |this, _, cx| {
                if this.screen == Screen::Welcome {
                    cx.notify();
                }
            }),
            cx.subscribe(&account_address, |this, _, _: &InputSubmitted, cx| {
                this.onboarding_address_entered(cx)
            }),
            cx.subscribe(&account_email, |this, _, _: &InputSubmitted, cx| {
                this.onboarding_email_entered(cx)
            }),
            cx.subscribe(
                &account,
                |this, _, event: &crate::account::AccountEvent, cx| match event {
                    crate::account::AccountEvent::Back => {
                        // The sign-in a link asked for was given up.
                        this.links.after_sign_in = None;
                        this.navigate(Screen::Home, cx)
                    }
                    crate::account::AccountEvent::SignedIn(account) => {
                        this.accept_account(account.clone(), cx)
                    }
                    // Signing out always says what stays first.
                    crate::account::AccountEvent::SignOut => this.ask_sign_out_default(cx),
                },
            ),
            cx.on_app_quit(|this, cx| {
                this.stop_all_chat_streams(cx);
                this.save_chat_history();
                let monitors = this.audio_pending.clone();
                this.finish_recording_preview(cx);
                // Everything still in memory reaches the disk before exit.
                this.flush_all_blocking(cx);
                let executor = cx.background_executor().clone();
                async move {
                    // Stop wakes the worker immediately. GPUI gives quit hooks
                    // 100ms; the manifest remains independently recoverable.
                    let started = std::time::Instant::now();
                    while monitors.iter().any(|m| !m.snapshot().completed())
                        && started.elapsed() < std::time::Duration::from_millis(90)
                    {
                        executor.timer(std::time::Duration::from_millis(5)).await;
                    }
                }
            }),
            cx.subscribe(&login, |this, _, _: &crate::login::LoginCompleted, cx| {
                this.navigate(Screen::Home, cx)
            }),
            cx.subscribe(&search, |_, _, _: &InputChanged, cx| cx.notify()),
            cx.subscribe(&search, |this, _, ev: &InputSubmitted, cx| {
                if this.menu == Some(Menu::CalendarEvent) {
                    if crate::server::enabled() {
                        this.select_connected_event_from_search(cx);
                        return;
                    }
                    if let Some(id) = this
                        .calendar_events(&ev.content)
                        .first()
                        .map(|event| event.id)
                    {
                        this.select_calendar_event(id, cx);
                    }
                    return;
                }
                if !this.search_open {
                    return;
                }
                // Return is normally taken by the palette's highlighted row
                // before it reaches the field. This is what is left when the
                // palette has not drawn its rows yet: never open a note the
                // highlight is not on.
                if !ev.content.trim().is_empty() {
                    this.open_search_results(cx);
                }
            }),
            // The composer's send button and its open or folded shape
            // follow what has been typed.
            cx.subscribe(&ask, |_, _, _: &InputChanged, cx| cx.notify()),
            // The bar says "Continue chat" while a conversation is folded
            // into it: taking the field (a press, Tab) opens that
            // conversation again, on a note and on Home alike, answer still
            // arriving or not.
            cx.subscribe(&ask, |this, _, _: &InputFocused, cx| {
                if (this.current_note.is_some() || this.screen == Screen::Home)
                    && !this.assistant_open
                    && (!this.chat_messages.is_empty() || this.chat_stream.is_some())
                {
                    this.assistant_open = true;
                    this.ask
                        .update(cx, |input, cx| input.set_placeholder("Ask anything", cx));
                    cx.notify();
                }
            }),
            cx.subscribe(&ask, |this, _, _: &InputSubmitted, cx| this.submit_chat(cx)),
            cx.subscribe(&title_input, |this, _, ev: &InputChanged, cx| {
                if let Some(note) = this.current_note.and_then(|id| this.workspace.note_mut(id)) {
                    let changed = note.title != ev.content;
                    this.note_body_dirty |= changed;
                    // Unsent until a save of it is answered.
                    note.unsent_edits |= changed && crate::server::enabled();
                    note.title = ev.content.clone();
                    this.persist(cx);
                }
            }),
            cx.subscribe(&editor, |this, _, ev: &InputChanged, cx| {
                if let Some(note) = this.current_note.and_then(|id| this.workspace.note_mut(id)) {
                    let changed = note.body != ev.content;
                    this.note_body_dirty |= changed;
                    note.unsent_edits |= changed && crate::server::enabled();
                    note.body = ev.content.clone();
                    this.persist(cx);
                }
            }),
            cx.subscribe(&attendee_input, |_, _, _: &InputChanged, cx| cx.notify()),
            cx.subscribe(&attendee_input, |this, _, ev: &InputSubmitted, cx| {
                if crate::server::enabled() {
                    this.add_meeting_attendee(ev.content.clone(), cx);
                    return;
                }
                let name = ev.content.trim().to_string();
                if name.is_empty() {
                    return;
                }
                if let Some(note) = this.current_note.and_then(|id| this.workspace.note_mut(id))
                    && !note
                        .participants
                        .iter()
                        .any(|p| p.eq_ignore_ascii_case(&name))
                {
                    note.participants.push(name);
                }
                this.attendee_input
                    .update(cx, |input, cx| input.set_content("", cx));
                this.persist(cx);
            }),
        ];
        subscriptions.extend(link_subscriptions);
        subscriptions.extend(share_subscriptions);
        subscriptions.extend(team_subscriptions);
        subscriptions.extend(org_subscriptions);
        let mut this = Self {
            workspace: Workspace::load(),
            login,
            account,
            screen: if welcome.is_some() {
                Screen::Welcome
            } else {
                Screen::Home
            },
            current_note: None,
            sidebar_open: true,
            sidebar_overlay: false,
            sidebar_narrow: false,
            search_open: false,
            search_results_query: String::new(),
            search,
            ask,
            title_input,
            editor,
            note_body_dirty: false,
            note_scroll,
            note_calendar_month: chrono::Local::now().date_naive(),
            attendee_input,
            active_tab: NoteTab::Enhanced,
            summary: Default::default(),
            assistant_open: false,
            transcript_open: false,
            transcript_epoch: 0,
            transcript_scroll: ScrollHandle::new(),
            transcript_last_epoch: u64::MAX,
            transcript_last_text: String::new(),
            transcript_last_query: String::new(),
            transcript_find_index: 0,
            transcript_find_open: false,
            transcript_last_live: 0,
            panel: Default::default(),
            live: Default::default(),
            recording: Default::default(),
            audio_capture: None,
            audio_error: None,
            audio_pending: Vec::new(),
            audio_quitting: false,
            bridges: Default::default(),
            refreshing: Default::default(),
            verifying: Default::default(),
            verify_failures: Default::default(),
            interrupted: Default::default(),
            sign_in_asked: false,
            accounts_watched: false,
            account_sheet: None,
            saved_data: None,
            saved_data_loading: false,
            debug_view_shown: false,
            server_pending: 0,
            server_status: None,
            generation: Default::default(),
            generation_draft: Default::default(),
            recording_transcript_start: None,
            regeneration_notice_dismissed: None,
            recording_from: crate::recording::Phase::Idle,
            recording_changed_at: None,
            recording_last_click: None,
            microphone_muted: false,
            menu: None,
            visible_menu: None,
            menu_epoch: 0,
            menu_closed_by_press: None,
            press_seq: 0,
            trash_ask: None,
            trash_reveal: None,
            trash_scroll: ScrollHandle::new(),
            deleting_forever: Default::default(),
            menu_keyboard: Default::default(),
            palette: Default::default(),
            menu_exiting: false,
            toast: None,
            toast_shown: String::new(),
            toast_epoch: 0,
            toast_action: None,
            toast_serial: 0,
            stage: Default::default(),
            sidebar_level: 1.,
            sidebar_width: crate::desktop_preferences::sidebar_width(),
            sidebar_span: crate::desktop_preferences::sidebar_width(),
            sidebar_grab: None,
            sidebar_edge_hovered: false,
            search_pose: crate::motion::Pose::REST,
            page_key: 0,
            autosave: Default::default(),
            caches: Default::default(),
            list_shown: None,
            nav_history: Vec::new(),
            settings_tab: Default::default(),
            row_menu: None,
            chat_models: vec![],
            selected_chat_model: None,
            chat_model_picker_open: false,
            chat_model_cursor: None,
            chat_messages: vec![],
            chat_annotations: Default::default(),
            chat_epoch: 0,
            chat_stream: None,
            chat_queue: Vec::new(),
            chat_parked: Default::default(),
            chat_interrupted: Default::default(),
            chat_editing: None,
            chat_maximized: false,
            chat_threads_expanded: false,
            chat_threads_collapsed: false,
            chat_history_open: false,
            chat_history_closed_at: None,
            chat_model_picker_closed_at: None,
            chat_scroll: ScrollHandle::new(),
            chat_follow: std::cell::Cell::new(0),
            chat_archives: crate::chat::load_history(),
            chat_sync: Default::default(),
            chat_thread_id: uuid::Uuid::new_v4().to_string(),
            calendar_page: 0,
            calendars: Default::default(),
            home_scroll: ScrollHandle::new(),
            meeting: Default::default(),
            links,
            share,
            team,
            org,
            updates: Default::default(),
            onboarding: crate::onboarding_view::Onboarding::new(welcome, cx),
            menu_bar: Default::default(),
            focus: cx.focus_handle(),
            _subscriptions: subscriptions,
            focus_lost: None,
            appearance_watch: None,
            theme_anchor: Default::default(),
        };
        this.restore_note_chat(0, cx);
        this
    }
    pub fn theme(&self) -> Theme {
        Theme::new(self.workspace.settings.dark_mode)
    }
    pub fn navigate(&mut self, screen: Screen, cx: &mut Context<Self>) {
        // A row picked in the sidebar over a narrow window: it slides away.
        self.sidebar_overlay = false;
        // A link's page is shown to anyone: it says where the note is and
        // reads nothing without a session on that server.
        let screen = if crate::server::enabled()
            && !crate::accounts::any_shown()
            && !matches!(screen, Screen::Login | Screen::Link | Screen::Welcome)
        {
            Screen::Account
        } else {
            screen
        };
        let leaving_note = self.current_note.is_some();
        // Leaving a note puts another conversation on screen; an answer
        // still arriving for the note's chat carries on off screen. Between
        // other pages the workspace conversation stays where it is.
        if leaving_note {
            self.park_chat_stream();
        }
        self.save_chat_history();
        self.chat_maximized = false;
        self.summary.dismiss();
        self.finish_note(cx);
        if screen == Screen::Login {
            self.login.update(cx, |login, cx| login.reset(cx));
            self.finish_recording_preview(cx);
        }
        crate::chrome::record_navigation(&mut self.nav_history, self.screen, screen);
        self.screen = screen;
        self.current_note = None;
        if leaving_note {
            self.restore_note_chat(0, cx);
        }
        self.menu = None;
        self.search_open = false;
        if screen == Screen::Home {
            self.connect_server(cx);
        }
        // The team space is read when it is opened, and once from Home so the sidebar can name it.
        if screen == Screen::TeamSpace || (screen == Screen::Home && self.team.page.is_none()) {
            self.load_team_space(cx);
        }
        if screen == Screen::Settings {
            self.refresh_login_item();
            self.refresh_saved_data(cx);
        }
        cx.notify();
    }
    /// Record a change to the workspace and redraw. The write itself is
    /// debounced and happens off the UI thread; a failure surfaces as the same
    /// "Could not save" toast as before.
    pub fn persist(&mut self, cx: &mut Context<Self>) -> bool {
        self.schedule_save(cx);
        cx.notify();
        true
    }
    pub fn finish_note(&mut self, cx: &mut Context<Self>) {
        self.finish_enhanced_edit(cx);
        // Navigation saves editor events but never generates notes implicitly.
        if (self.note_body_dirty
            || self
                .current_note
                .and_then(|id| self.workspace.note(id))
                .is_some_and(|note| note.enhanced_dirty))
            && let Some(id) = self.current_note
        {
            self.server_action(id, crate::server::Action::Save, cx);
        }
        self.note_body_dirty = false;
        self.generation.cancel();
        // Leaving a note hands its edits to the writer straight away.
        self.flush_edits(cx);
        cx.notify();
    }
    /// Say something that needs no answer, in the title bar's pill: where
    /// it covers nothing that can be pressed. It drops out of the top of the
    /// window, stays four seconds and goes back up; a new one takes the
    /// pill over (its words swap in and the pill finds its new width).
    pub fn show_toast(&mut self, message: &str, cx: &mut Context<Self>) {
        self.raise_toast(message, None, cx);
    }

    /// [`Cereal::show_toast`] with one thing to press beside the words:
    /// "Undo" after something was removed, a way to the thing it names.
    /// Pressing it runs `action` and takes the toast away.
    pub fn show_toast_with(
        &mut self,
        message: &str,
        label: &'static str,
        action: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let action = ToastAction {
            label: label.into(),
            run: std::rc::Rc::new(action),
        };
        self.raise_toast(message, Some(action), cx);
    }

    fn raise_toast(&mut self, message: &str, action: Option<ToastAction>, cx: &mut Context<Self>) {
        self.toast_epoch = self.toast_epoch.wrapping_add(1);
        self.toast_serial = self.toast_serial.wrapping_add(1);
        let epoch = self.toast_epoch;
        let message = message.to_string();
        if self.toast.is_some() {
            // A replacement gives the pill a small push; its words swap in.
            self.stage.kick(("toast", 0), 0.8, crate::motion::GOOEY);
        } else if self.stage.target(("toast", 0)).is_none() {
            // Nothing is left of the last one: this one arrives at its own
            // size and place rather than gliding there from the last one's.
            self.stage.forget(("toast-width", 0));
            self.stage.forget(("toast-top", 0));
            self.stage.forget(("toast-centre", 0));
        }
        let stays = if action.is_some() {
            TOAST_WITH_ACTION_FOR
        } else {
            TOAST_FOR
        };
        self.toast = Some(message.clone());
        self.toast_shown = message.clone();
        self.toast_action = action;
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(stays).await;
            let _ = this.update(cx, move |this, cx| {
                if toast_should_expire(epoch, this.toast_epoch, &message, this.toast.as_deref()) {
                    this.toast = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn dismiss_toast(&mut self, cx: &mut Context<Self>) {
        self.toast_epoch = self.toast_epoch.wrapping_add(1);
        self.toast = None;
        cx.notify();
    }

    /// The toast's one action was pressed: it is done, and the toast goes.
    fn run_toast_action(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.toast.is_none() {
            return;
        }
        let action = self.toast_action.clone();
        self.dismiss_toast(cx);
        if let Some(action) = action {
            (action.run)(self, window, cx);
        }
    }

    pub fn open_note(&mut self, id: u64, cx: &mut Context<Self>) {
        self.sidebar_overlay = false;
        self.summary.dismiss();
        let changed = self.current_note != Some(id);
        if self.current_note != Some(id) {
            self.chat_maximized = false;
            self.assistant_open = false;
            // An answer still arriving carries on in its conversation.
            self.park_chat_stream();
            if crate::server::enabled() {
                self.save_chat_history();
                self.chat_messages.clear();
                self.chat_interrupted.clear();
                self.chat_annotations.clear();
            }
            self.finish_note(cx);
            self.note_body_dirty = false;
            self.transcript_open = false;
            self.transcript_find_open = false;
        }
        if self.workspace.note(id).is_some() {
            // What its notifications said is on screen now.
            self.withdraw_note_notifications(id);
        }
        if let Some(note) = self.workspace.note(id) {
            let title = note.title.clone();
            let body = note.body.clone();
            self.title_input
                .update(cx, |input, cx| input.set_content(title, cx));
            self.editor
                .update(cx, |input, cx| input.set_content(body, cx));
            self.current_note = Some(id);
            self.active_tab = if note.enhanced.trim().is_empty() {
                NoteTab::Notes
            } else {
                NoteTab::Enhanced
            };
            if changed {
                self.restore_note_chat(id, cx);
            }
            self.search_open = false;
            self.menu = None;
            if crate::server::enabled() {
                self.refresh_meeting_context(id, cx);
            }
            cx.notify();
        }
    }
    pub fn new_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if crate::server::enabled() && !crate::accounts::any_shown() {
            self.screen = Screen::Account;
            self.show_toast("Sign in before creating a server note", cx);
            return;
        }
        // A recording under way in another note is never ended by starting a
        // new one: the note opens without recording and says how to move the
        // recording here (see `offer_recording_here`).
        let recording = self.recording_elsewhere(None);
        if recording.is_none() {
            self.finish_recording_preview(cx);
        }
        self.recording_last_click = None;
        self.recording_changed_at = None;
        let Some(id) = self.create_note_without_event() else {
            self.show_toast("New notes cannot be saved for this account right now", cx);
            return;
        };
        self.workspace.note_mut(id).unwrap().title.clear();
        self.open_note(id, cx);
        self.active_tab = NoteTab::Notes;
        self.title_input
            .update(cx, |input, cx| input.set_content("", cx));
        self.persist(cx);
        match recording {
            Some(other) => self.offer_recording_here(id, other, cx),
            None => self.start_recording_preview(id, cx),
        }
        self.editor.read(cx).focus_handle.focus(window);
    }
    /// A new note with no calendar event. With accounts it goes to the
    /// default account: if that account cannot be reached the note is still
    /// made, in its slot, and is sent when it can be. Nothing falls back to
    /// another account. `None` when that account's notes are not loaded.
    pub fn create_note_without_event(&mut self) -> Option<u64> {
        if !crate::server::enabled() {
            return Some(self.workspace.create_note());
        }
        let slot = crate::accounts::registry()
            .new_note_account()
            .map(|account| account.slot)?;
        self.workspace.create_note_in(slot)
    }

    /// The meetings "Select calendar event" offers without a server: those
    /// from today on, or, when there are none, the three most recent.
    /// Typing narrows that same list, so what is on screen is what is
    /// searched.
    pub fn calendar_events(&self, query: &str) -> Vec<crate::model::Note> {
        let today = chrono::Local::now()
            .date_naive()
            .format("%Y-%m-%d")
            .to_string();
        let notes: Vec<(&str, &crate::model::Note)> = self
            .workspace
            .visible_notes(false, "")
            .into_iter()
            .map(|note| (note.date.as_str(), note))
            .collect();
        let offered = calendar_event_offer(
            &notes
                .iter()
                .map(|(date, note)| (note.id, *date, note.title.as_str()))
                .collect::<Vec<_>>(),
            self.current_note,
            &today,
            query,
        );
        offered
            .into_iter()
            .filter_map(|id| notes.iter().find(|(_, note)| note.id == id))
            .map(|(_, note)| (*note).clone())
            .collect()
    }
    pub fn select_calendar_event(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(event) = self.workspace.note(id).cloned() else {
            return;
        };
        let mut renamed = None;
        if let Some(note) = self.current_note.and_then(|id| self.workspace.note_mut(id)) {
            note.date = event.date;
            note.time = event.time;
            if note.title.trim().is_empty()
                || matches!(note.title.as_str(), "Untitled note" | "New note")
            {
                note.title = event.title;
                renamed = Some(note.title.clone());
            }
        }
        if let Some(title) = renamed {
            self.title_input
                .update(cx, |input, cx| input.set_content(title, cx));
        }
        self.menu = Some(Menu::NoteDetails);
        self.persist(cx);
    }
    pub fn copy_current(&mut self, cx: &mut Context<Self>) {
        if let Some(note) = self.current_note.and_then(|id| self.workspace.note(id)) {
            let content = match self.active_tab {
                NoteTab::Notes => &note.body,
                NoteTab::Enhanced => &note.enhanced,
            };
            cx.write_to_clipboard(ClipboardItem::new_string(format!(
                "{}\n\n{}",
                note.title, content
            )));
            self.menu = None;
            self.show_toast("Note copied to clipboard", cx);
        }
    }
    /// Show the conversation on the Chat page, remembering the page it was
    /// asked from so the back pill returns there.
    pub(crate) fn open_chat_page(&mut self) {
        crate::chrome::record_navigation(&mut self.nav_history, self.screen, Screen::Chat);
        self.screen = Screen::Chat;
        self.assistant_open = false;
    }
    pub fn submit_chat(&mut self, cx: &mut Context<Self>) {
        let question = self.ask.read(cx).content().trim().to_string();
        if question.is_empty() {
            return;
        }
        self.submit_chat_question(question, cx);
    }
    /// Submit a resolved prompt as text, bypassing composer slash commands.
    pub fn submit_chat_question(&mut self, question: String, cx: &mut Context<Self>) {
        let question = question.trim().to_owned();
        if question.is_empty() {
            return;
        }
        if crate::server::enabled() {
            self.submit_server_chat(question, cx);
            return;
        }
        self.apply_chat_edit();
        let query = question.to_lowercase();
        let todos =
            query.contains("todo") || query.contains("action") || query.contains("next step");
        let mut lines = Vec::new();
        let terms: Vec<_> = query
            .split_whitespace()
            .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()))
            .filter(|w| {
                w.len() > 3
                    && ![
                        "what", "about", "show", "tell", "have", "from", "notes", "please",
                        "anything", "decide",
                    ]
                    .contains(w)
            })
            .collect();
        let notes = self.workspace.visible_notes(false, "");
        for note in notes.iter() {
            let source = format!("{}\n{}", note.enhanced, note.body);
            let matching: Vec<_> = source
                .lines()
                .filter(|line| {
                    if todos {
                        (line.starts_with("- ") || line.starts_with("• "))
                            && (line.contains(':')
                                || line.to_lowercase().contains("review")
                                || line.to_lowercase().contains("share"))
                    } else {
                        !line.trim().is_empty()
                            && !line.starts_with('#')
                            && terms.iter().any(|term| line.to_lowercase().contains(term))
                    }
                })
                .take(3)
                .collect();
            if !matching.is_empty() {
                lines.push(format!("{}\n{}", note.title, matching.join("\n")));
            }
            if lines.len() == 3 {
                break;
            }
        }
        let answer = if lines.is_empty() {
            "I couldn’t find a matching passage in your local notes. Try a project name, a person, or “list recent todos”.".into()
        } else {
            format!(
                "Here’s what I found in your local notes:\n\n{}",
                lines.join("\n\n")
            )
        };
        self.chat_messages.push((question, answer));
        self.ask.update(cx, |input, cx| input.set_content("", cx));
        if self.current_note.is_some() {
            self.assistant_open = true;
            self.transcript_open = false;
        } else {
            if crate::chat::floating_workspace_chat(self.screen) {
                self.assistant_open = true;
            } else {
                self.open_chat_page();
            }
            self.search_open = false;
            self.menu = None;
        }
        cx.notify();
    }
    pub fn open_search_results(&mut self, cx: &mut Context<Self>) {
        let query = self.search.read(cx).content().trim().to_string();
        if query.is_empty() {
            return;
        }
        self.search_results_query = query;
        // Each search is read from its first result.
        crate::lazy::scroll("search-results").set_offset(Point::default());
        self.navigate(Screen::SearchResults, cx);
    }
    pub fn open_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.summary.dismiss();
        self.transcript_find_open = false;
        self.visible_menu = None;
        self.menu_exiting = false;
        self.menu_epoch = self.menu_epoch.wrapping_add(1);
        // It opens empty: what was searched for last time would only have to
        // be deleted before the next search can be typed.
        let reopening = !self.search_open;
        self.search.update(cx, |input, cx| {
            input.set_placeholder("Search meetings", cx);
            if reopening {
                input.set_content("", cx);
            }
        });
        self.search_open = true;
        self.menu = None;
        self.search.read(cx).focus_handle.focus(window);
        cx.notify();
    }
    /// Tab and Shift-Tab move the keyboard focus through every control and
    /// text field, in the order they are drawn. The note's body keeps Tab for
    /// itself, as it always has (it types a tab); Shift-Tab leaves it.
    fn focus_step(&mut self, back: bool, window: &mut Window, cx: &mut Context<Self>) {
        let typing_a_tab = !back
            && (self.editor.read(cx).focus_handle.is_focused(window)
                || self
                    .summary
                    .edit
                    .as_ref()
                    .is_some_and(|edit| edit.input.read(cx).focus_handle.is_focused(window)));
        if typing_a_tab {
            cx.propagate();
            return;
        }
        set_keyboard_focus(true);
        if back {
            window.focus_prev();
        } else {
            window.focus_next();
        }
        cx.notify();
    }

    /// Close the topmost thing that is open over the page, and only that
    /// one: the model menu, a source card, the palette, a menu or popover
    /// (a question inside it first, and the event picker back to the card
    /// it unfolded from), the sidebar over a narrow window, the list of
    /// recent chats. What Escape does first, wherever it is pressed.
    /// Returns whether there was something to close.
    pub(crate) fn dismiss_top(&mut self, cx: &mut Context<Self>) -> bool {
        match top_layer(
            self.chat_model_picker_open,
            self.summary.open.is_some(),
            self.search_open,
            self.menu.as_ref(),
            self.trash_ask.is_some(),
            self.sidebar_overlay,
            self.chat_history_open,
        ) {
            Some(Layer::ModelMenu) => self.chat_model_picker_open = false,
            Some(Layer::SourceCard) => self.summary.dismiss(),
            Some(Layer::Palette) => self.search_open = false,
            Some(Layer::TrashQuestion) => self.trash_ask = None,
            Some(Layer::EventPicker) => self.menu = Some(Menu::NoteDetails),
            Some(Layer::Menu) => {
                self.menu = None;
                self.menu_keyboard.selected = None;
            }
            Some(Layer::Sidebar) => self.sidebar_overlay = false,
            Some(Layer::ChatHistory) => self.chat_history_open = false,
            None => return false,
        }
        cx.notify();
        true
    }

    fn escape(&mut self, _: &Escape, window: &mut Window, cx: &mut Context<Self>) {
        // In the welcome, Escape is Back.
        if self.screen == Screen::Welcome && self.toast.is_none() && self.menu.is_none() {
            self.onboarding_back(cx);
            return;
        }
        if self.summary.edit.is_some() {
            self.finish_enhanced_edit(cx);
            self.focus.focus(window);
            return;
        }
        // One layer at a time, topmost first. The composer's popovers and
        // the sidebar leave the keyboard where it was, as a press outside
        // them does: the next Escape folds the bar.
        let keeps_focus = self.chat_model_picker_open
            || (self.sidebar_overlay && self.menu.is_none() && !self.search_open)
            || (self.chat_history_open && self.menu.is_none() && !self.search_open);
        if self.dismiss_top(cx) {
            if !keeps_focus {
                self.focus.focus(window);
            }
            return;
        }
        if self.transcript_find_open {
            self.transcript_find_end(window, cx);
        } else if self.transcript_open {
            self.transcript_open = false;
            self.transcript_epoch = self.transcript_epoch.wrapping_add(1);
        } else if self.chat_editing.is_some() {
            self.cancel_chat_edit(cx);
        } else if self.chat_maximized {
            self.chat_maximized = false;
        } else if self.assistant_open {
            self.close_chat_surface(cx);
        } else if self.current_note.is_some() {
            // With nothing left to close a note stays where it is: Escape
            // only takes the keyboard out of the field. Leaving is the back
            // pill, or ⌘1.
        } else if !matches!(self.screen, Screen::Home | Screen::Welcome | Screen::Login) {
            // With nothing left to close, Escape is the back pill: every
            // page but Home steps back to where it came from.
            self.go_back(cx);
        }
        self.focus.focus(window);
        cx.notify();
    }
}

impl Cereal {
    /// The sidebar in the room it has beside the page: it keeps its natural
    /// width and slides out from under a shrinking clip, so its own rows
    /// never reflow. `over` draws it out of the flow, on the page's edge,
    /// for the reduced-motion fade.
    fn render_sidebar_slot(
        &self,
        sidebar: crate::motion::Pose,
        over: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let t = self.theme();
        let room = if over { 1. } else { self.sidebar_level };
        div()
            .relative()
            .when(over, |slot| slot.absolute().top_0().left_0())
            .w(px(self.sidebar_span * room))
            .h_full()
            .flex_shrink_0()
            .overflow_hidden()
            .bg(t.sidebar)
            // It thins out as it slides, so its top strip never reads as
            // colliding with the traffic lights.
            .opacity(sidebar.fade * sidebar.value.clamp(0., 1.))
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .right_0()
                    .w(px(self.sidebar_span))
                    .child(self.render_sidebar(cx)),
            )
    }

    /// The toast: a pill in the title bar, centred over the page, where it
    /// covers nothing that can be pressed (under the recording's pill when
    /// that is there, and under the bar when the bar's own controls leave
    /// it no room). It drops out of the top of the window and goes back up.
    fn render_toast(
        &self,
        pose: crate::motion::Pose,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use crate::motion::SNAPPY;
        let t = self.theme();
        let toast = self
            .toast
            .clone()
            .unwrap_or_else(|| self.toast_shown.clone());
        let (title, description) = toast
            .split_once('\n')
            .map(|(title, description)| (title.to_string(), Some(description.to_string())))
            .unwrap_or((toast.clone(), None));
        let success = title.contains("copied to clipboard")
            || title == "All changes saved"
            || title == "Note restored"
            || title.starts_with("Cereal is up to date");
        let action = self.toast_action.clone();
        let words = crate::update_view::text_width(&title, 13., FontWeight::SEMIBOLD, window).max(
            description
                .as_deref()
                .map_or(0., |text| crate::update_view::text_width(text, 12., FontWeight::NORMAL, window)),
        );
        // Its label in 8 pt of padding either side, and the gap before it.
        let action_width = action.as_ref().map_or(0., |action| {
            crate::update_view::text_width(&action.label, 13., FontWeight::MEDIUM, window).ceil()
                + 16.
                + 8.
        });
        // The outline, the padding, the mark, the words (and a point to
        // spare, so a rounding never wraps them), the action, the close
        // button, each 8 pt from the next.
        let natural = 2. + 12. + 16. + 8. + words.ceil() + 2. + action_width + 8. + 22. + 7.;
        let viewport = f32::from(window.viewport_size().width);
        let (reach_left, reach_right) = self.header_reach();
        let (width, in_bar) = toast_place(
            natural,
            viewport,
            reach_left,
            reach_right,
            self.session_pill_width(),
        );
        // In the bar; or, when the bar has no room, under it over the page.
        let (top, centre) = match in_bar {
            Some(centre) => (TOAST_TOP, centre),
            None => (
                44.,
                self.sidebar_inset() + (viewport - self.sidebar_inset()) / 2.,
            ),
        };
        let centre = self.stage.aim(("toast-centre", 0), centre, SNAPPY);
        let width = self.stage.aim(("toast-width", 0), width, SNAPPY);
        let top = self.stage.aim(("toast-top", 0), top, SNAPPY);
        let swap = self.stage.swap(("toast-words", 0), self.toast_serial);
        crate::motion::pop(
            floating(div().id("toast-notification"))
                .absolute()
                .top(px(top))
                .left(px(centre - width / 2.))
                .w(px(width))
                .min_h(px(TOAST_HEIGHT))
                .pl(px(12.))
                .pr(px(7.))
                .py(px(6.))
                .rounded(px(12.))
                .bg(t.card)
                .border_1()
                .border_color(t.border)
                .shadow_md()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(
                    div()
                        .size(px(16.))
                        .flex_shrink_0()
                        .rounded_full()
                        .border_1()
                        .border_color(t.muted)
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(10.))
                        .text_color(t.muted)
                        .child(if success {
                            icon("check", 10., t.muted).into_any_element()
                        } else {
                            div().child("i").into_any_element()
                        }),
                )
                .child(crate::motion::swapped(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_size(px(13.))
                                .line_height(px(18.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(t.text)
                                .child(title),
                        )
                        .when_some(description, |d, description| {
                            d.child(
                                div()
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .text_color(t.muted)
                                    .child(description),
                            )
                        }),
                    swap,
                ))
                .when_some(action, |toast, action| {
                    toast.child(
                        div()
                            .id("toast-action")
                            .h(px(24.))
                            .px(px(8.))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .rounded(px(6.))
                            .text_size(px(13.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(t.accent)
                            .cursor_default()
                            .child(crate::motion::tactile(t.hover).press(t.fill))
                            .child(action.label.clone())
                            .reach()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.run_toast_action(window, cx)
                            })),
                    )
                })
                .child(
                    tool_tip("dismiss-toast", "close", "Dismiss", t)
                        .size(px(22.))
                        .on_click(cx.listener(|this, _, _, cx| this.dismiss_toast(cx))),
                ),
            crate::motion::Pop::chip((0.5, 0.)),
            pose,
        )
    }
}

/// What can be open over the page, topmost first (see [`top_layer`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Layer {
    ModelMenu,
    SourceCard,
    Palette,
    /// A Trash row asking "Delete forever?".
    TrashQuestion,
    /// "Select calendar event", unfolded from the details card.
    EventPicker,
    Menu,
    Sidebar,
    ChatHistory,
}

/// The topmost layer open over the page, which is the one Escape closes.
pub(crate) fn top_layer(
    model_menu: bool,
    source_card: bool,
    palette: bool,
    menu: Option<&Menu>,
    trash_question: bool,
    sidebar_overlay: bool,
    chat_history: bool,
) -> Option<Layer> {
    if model_menu {
        Some(Layer::ModelMenu)
    } else if source_card {
        Some(Layer::SourceCard)
    } else if palette {
        Some(Layer::Palette)
    } else if let Some(menu) = menu {
        Some(match menu {
            Menu::Trash if trash_question => Layer::TrashQuestion,
            Menu::CalendarEvent => Layer::EventPicker,
            _ => Layer::Menu,
        })
    } else if sidebar_overlay {
        Some(Layer::Sidebar)
    } else if chat_history {
        Some(Layer::ChatHistory)
    } else {
        None
    }
}

/// Which notes "Select calendar event" offers: of `notes` (id, day, title;
/// newest first), without the note being edited, those from `today` on, or
/// failing any the three most recent; then only those whose title contains
/// `query`, at most six.
pub fn calendar_event_offer(
    notes: &[(u64, &str, &str)],
    current: Option<u64>,
    today: &str,
    query: &str,
) -> Vec<u64> {
    let others = || notes.iter().filter(|(id, _, _)| Some(*id) != current);
    let mut offered: Vec<_> = others().filter(|(_, date, _)| *date >= today).collect();
    if offered.is_empty() {
        offered = others().take(3).collect();
    }
    let query = query.trim().to_lowercase();
    offered
        .into_iter()
        .filter(|(_, _, title)| query.is_empty() || title.to_lowercase().contains(&query))
        .take(6)
        .map(|(id, _, _)| *id)
        .collect()
}

impl Focusable for Cereal {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Cereal {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use crate::motion::{GOOEY, SETTLE};
        crate::latency::painted();
        // The control that had the focus has gone (the page changed under a
        // shortcut, a row opened its note, a button removed itself). Give
        // the focus back to the window, or the keyboard would be left with
        // nothing: no shortcut reaches a control that is not there, and Tab
        // would have nowhere to start from.
        if self.focus_lost.is_none() {
            self.focus_lost = Some(cx.on_focus_lost(window, |this, window, cx| {
                this.focus.focus(window);
                cx.notify();
            }));
        }
        // The System theme follows macOS, live; Light and Dark hold.
        if self.appearance_watch.is_none() {
            self.appearance_watch =
                Some(cx.observe_window_appearance(window, |_, _, cx| cx.notify()));
        }
        self.workspace.settings.resolve_theme(matches!(
            window.appearance(),
            WindowAppearance::Dark | WindowAppearance::VibrantDark
        ));
        // The Workspace pages of Settings read their workspace when shown.
        self.organization_frame(cx);
        // The theme menu belongs to its button on the Preferences page.
        if self.menu == Some(Menu::Theme)
            && !(self.screen == Screen::Settings
                && self.current_note.is_none()
                && self.settings_tab == crate::screens::SettingsTab::Preferences)
        {
            self.menu = None;
        }
        // The Trash popover belongs to the sidebar's footer: it goes when
        // the footer does (a note opened, the sidebar hidden).
        if self.menu == Some(Menu::Trash) && !self.trash_button_shown() {
            self.menu = None;
        }
        if self.menu != Some(Menu::Trash) {
            self.trash_ask = None;
            self.trash_reveal = None;
        }
        self.sync_share(window, cx);
        match self.menu.clone() {
            Some(menu) if self.visible_menu.as_ref() != Some(&menu) || self.menu_exiting => {
                // One menu replacing another in place pops again from part-way,
                // except the event picker, which unfolds from the details card.
                let details = |menu: &Menu| matches!(menu, Menu::NoteDetails | Menu::CalendarEvent);
                if !self.menu_exiting
                    && self
                        .visible_menu
                        .as_ref()
                        .is_some_and(|previous| !(details(previous) && details(&menu)))
                {
                    self.stage.kick(("menu", 0), 0.5, GOOEY);
                }
                // A row's target survives only inside its own menu; any other
                // menu acts on the open note.
                if menu != Menu::NoteRow {
                    self.row_menu = None;
                }
                self.menu_epoch = self.menu_epoch.wrapping_add(1);
                self.menu_keyboard.reset();
                self.visible_menu = Some(menu);
                self.menu_exiting = false;
            }
            None if self.visible_menu.is_some() && !self.menu_exiting => {
                self.menu_exiting = true;
                self.menu_epoch = self.menu_epoch.wrapping_add(1);
            }
            None if self.visible_menu.is_none() => self.menu_keyboard.reset(),
            _ => {}
        }
        self.reveal_trash_row();
        // The menu stays mounted until its exit has finished, however often it
        // was interrupted on the way; no timer has to guess the duration.
        let menu_pose = self.stage.presence(("menu", 0), self.menu.is_some(), GOOEY);
        if !menu_pose.visible && self.visible_menu.is_some() {
            self.row_menu = None;
            self.visible_menu = None;
            self.menu_exiting = false;
            self.menu_keyboard.reset();
        }
        if self.menu.is_some()
            && !matches!(
                self.menu,
                // These hold a text field of their own.
                Some(Menu::NoteDetails | Menu::CalendarEvent | Menu::Share)
            )
        {
            self.focus.focus(window);
        }
        self.stage.begin();
        crate::motion::frame();
        crate::ui::set_dark(self.workspace.settings.dark_mode);
        crate::ui::set_scale(window.scale_factor());
        crate::ui::provision_focus(cx);
        // The control that opened what is open stays lit meanwhile.
        self.hold_open_triggers();
        self.drain_chat_history(cx);
        if !self.sidebar_open {
            self.sidebar_grab = None;
            self.sidebar_edge_hovered = false;
        }
        let viewport_width = f32::from(window.viewport_size().width);
        self.sidebar_narrow = viewport_width < crate::desktop_preferences::SIDEBAR_OVERLAY_BELOW;
        if !self.sidebar_narrow {
            self.sidebar_overlay = false;
        }
        // Over a narrow window the sidebar has its chosen width (it covers
        // the page rather than sharing the window with it).
        self.sidebar_span = if self.sidebar_narrow {
            crate::desktop_preferences::clamp_sidebar_width(self.sidebar_width)
        } else {
            crate::desktop_preferences::sidebar_width_in(self.sidebar_width, viewport_width)
        };
        let t = self.theme();
        let login = matches!(
            self.screen,
            Screen::Login | Screen::Account | Screen::Welcome
        );
        let full_chat =
            self.chat_maximized || (self.screen == Screen::Chat && self.current_note.is_none());
        let content = if self.screen == Screen::Welcome {
            self.render_onboarding(window, cx)
        } else if self.screen == Screen::Account {
            self.account.clone().into_any_element()
        } else if login {
            // The sign-in screen follows the app's theme.
            let dark = self.workspace.settings.dark_mode;
            if self.login.read(cx).dark_mode != dark {
                self.login.update(cx, |login, _| login.dark_mode = dark);
            }
            self.login.clone().into_any_element()
        } else if self.current_note.is_some() && self.chat_maximized {
            self.render_context_chat(window, cx)
        } else if self.current_note.is_some() {
            self.render_note(window, cx)
        } else if self.screen == Screen::Link {
            self.render_link_page(window, cx)
        } else {
            match self.screen {
                Screen::Home | Screen::MyNotes | Screen::Favorites => self.render_home(window, cx),
                Screen::TeamSpace => self.render_team_space(window, cx),
                Screen::TeamNote => self.render_team_note(window, cx),
                _ => self.render_screen(window, cx),
            }
        };
        let sidebar = self.stage.presence(
            ("sidebar", 0),
            // A link's page is a note page: it has the window to itself.
            self.sidebar_open
                && !self.sidebar_narrow
                && !login
                && self.screen != Screen::Link
                && (self.current_note.is_none() || self.chat_maximized),
            SETTLE,
        );
        // The sidebar over a narrow window, on a dimmed page.
        let overlay = self.stage.presence(
            ("sidebar-overlay", 0),
            self.sidebar_overlay
                && self.sidebar_narrow
                && !login
                && self.screen != Screen::Link
                && (self.current_note.is_none() || self.chat_maximized),
            crate::motion::PANEL,
        );
        // The room the sidebar takes beside the page. With reduced motion
        // it is given or taken back at once: the sidebar then fades over the
        // page's edge, and the page, which has jumped, firms up under the
        // same short fade instead of snapping bare.
        let reduced = crate::motion::reduce_motion();
        self.sidebar_level = self.stage.room(("sidebar", 0), sidebar);
        let sidebar_over = reduced && sidebar.visible && self.sidebar_level <= 0.;
        let page_firm = if reduced {
            let pose = self
                .stage
                .swap(("sidebar-room", 0), u64::from(self.sidebar_level > 0.5));
            crate::motion::PAGE_FROM + (1. - crate::motion::PAGE_FROM) * pose.fade.clamp(0., 1.)
        } else {
            1.
        };
        self.search_pose = self.stage.presence(("search", 0), self.search_open, GOOEY);
        let toast_pose = self
            .stage
            .presence(("toast", 0), self.toast.is_some(), GOOEY);
        // The update's mark is timed by hand: it asks for its own frames.
        // In Settings → About the update is told in the version row, and the
        // notice in the corner is not shown.
        let about = self.screen == Screen::Settings
            && self.settings_tab == crate::screens::SettingsTab::About;
        if self.updates.frame(std::time::Instant::now(), about) {
            self.stage.keep_alive();
        }
        let update_pose =
            self.stage
                .presence(("update-notice", 0), self.updates.notice_open(), GOOEY);
        let history_pose = self.stage.presence(
            ("chat-history-page", 0),
            self.chat_history_open && !self.search_open && full_chat,
            GOOEY,
        );
        // Navigation replays the page entrance; typing and data refreshes do not.
        let page_key = {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            std::mem::discriminant(&self.screen).hash(&mut hasher);
            if self.screen == Screen::Link {
                self.links.epoch.hash(&mut hasher);
            }
            (self.current_note, self.chat_maximized).hash(&mut hasher);
            hasher.finish()
        };
        if page_key != self.page_key {
            self.page_key = page_key;
            // The list cascades in only when a different list is shown, not
            // on the way back from a note to the list the user just left.
            let list_key = std::mem::discriminant(&self.screen);
            if self.current_note.is_none() && self.list_shown != Some(list_key) {
                self.list_shown = Some(list_key);
                self.stage.mark("page");
                // The lists of notes share one scroll position. A list that
                // is shown anew starts at its top, not where the one before
                // it was left.
                if matches!(
                    self.screen,
                    Screen::Home | Screen::MyNotes | Screen::Favorites
                ) {
                    self.home_scroll.set_offset(Point::default());
                }
            }
        }
        let root = div()
            .id("cereal-root")
            .key_context("Cereal")
            .track_focus(&self.focus)
            // The app's own shortcuts wait until the welcome is over: a new
            // note would start recording behind it.
            .on_action(cx.listener(|this, _: &NewNote, w, cx| {
                if this.screen != Screen::Welcome {
                    this.new_note(w, cx)
                }
            }))
            .on_action(cx.listener(|this, _: &Search, w, cx| {
                if this.screen != Screen::Welcome {
                    this.open_search(w, cx)
                }
            }))
            .on_action(cx.listener(Self::escape))
            .on_action(cx.listener(|this, _: &FocusNext, w, cx| this.focus_step(false, w, cx)))
            .on_action(cx.listener(|this, _: &FocusPrevious, w, cx| this.focus_step(true, w, cx)))
            // Any press of the pointer ends keyboard focus mode: a control
            // that is clicked takes the focus without showing a ring.
            .capture_any_mouse_down(cx.listener(|_, _, _, cx| {
                if set_keyboard_focus(false) {
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|this, _: &GoHome, _, cx| {
                if this.screen != Screen::Welcome {
                    this.navigate(Screen::Home, cx)
                }
            }))
            .on_action(cx.listener(|this, _: &OpenSettings, _, cx| {
                if this.screen != Screen::Welcome {
                    this.navigate(Screen::Settings, cx)
                }
            }))
            .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| {
                this.toggle_sidebar();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &Save, _, cx| {
                // An explicit Save writes now and reports the real outcome.
                this.autosave.retry();
                cx.notify();
                if this.flush_all_blocking(cx) {
                    if let Some(id) = this.current_note.filter(|_| crate::server::enabled()) {
                        this.server_action(id, crate::server::Action::Save, cx);
                    } else {
                        this.show_toast("All changes saved", cx);
                    }
                }
            }))
            // "Quit" leaves Cereal in the menu bar; "Quit Completely" exits
            // (see `menubar::quit_plan`).
            .on_action(cx.listener(|this, _: &Quit, window, cx| {
                this.quit_requested(crate::menubar::QuitKind::Quit, window, cx)
            }))
            .on_action(cx.listener(|this, _: &QuitCompletely, window, cx| {
                this.quit_requested(crate::menubar::QuitKind::Completely, window, cx)
            }))
            .on_action(cx.listener(|this, _: &CheckForUpdates, _, cx| this.check_for_updates(cx)))
            .on_action(cx.listener(Self::toggle_recording_key))
            .on_action(cx.listener(Self::toggle_transcript_key))
            .on_action(cx.listener(|_, _: &CloseWindow, window, cx| {
                crate::pill::hide_main_window(window, cx)
            }))
            .on_action(cx.listener(|_, _: &Hide, window, cx| {
                crate::pill::hide_main_window(window, cx);
                cx.hide();
            }))
            .relative()
            .flex()
            .size_full()
            .overflow_hidden()
            .bg(t.bg)
            .text_color(t.text)
            .font_family(FONT)
            .text_size(px(14.))
            .line_height(px(18.))
            .whitespace_normal()
            .when(sidebar.visible && !sidebar_over, |s| s.child(self.render_sidebar_slot(sidebar, false, cx)))
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .when(page_firm < 1., |page| page.opacity(page_firm))
                    // The Chat page has the pages' own header (the pill back
                    // to where it was opened from); only a note's chat opened
                    // to the full window has the chat header.
                    .when(!login && !(full_chat && self.current_note.is_some()), |s| {
                        s.child(self.render_header(cx))
                    })
                    .when(!login && full_chat && self.current_note.is_some(), |s| {
                        s.child(self.render_context_chat_header(cx))
                    })
                    .child(crate::motion::page(
                        ("page", page_key),
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_h_0()
                            .min_w_0()
                            .w_full()
                            .child(content),
                    )),
            )
            // Reduced motion, closing: the sidebar fades over the page.
            .when(sidebar_over, |s| s.child(self.render_sidebar_slot(sidebar, true, cx)))
            // The sidebar's edge: the strip that resizes it, over both the
            // sidebar and the page, under every popover.
            .children(self.render_sidebar_edge(sidebar, cx))
            .when(overlay.visible, |s| s.child(self.render_sidebar_overlay(overlay, cx)))
            // The sidebar button and search: one pair, in one place, over
            // the sidebar and the page's header alike.
            .children(self.render_window_tools(login, cx))
            .when(self.search_pose.visible, |s| {
                s.child(self.render_search(cx))
            })
            .when(history_pose.visible, |s| {
                let left = self.sidebar_inset()
                    + if self.current_note.is_some() {
                        62.
                    } else if self.screen == Screen::Chat && self.sidebar_away() {
                        // Under the bar's own History button: past the
                        // traffic lights, the two window tools and the back pill.
                        // (8 + 80, two 28 pt tools and a 43 pt pill, 5 pt apart).
                        202.
                    } else {
                        8.
                    };
                // A press outside it closes it and goes no further.
                s.child(
                    crate::ui::overlay(
                        self.chat_history_open,
                        cx.listener(|this, _, _, cx| {
                            this.chat_history_open = false;
                            cx.notify();
                        }),
                    )
                    .child(div().absolute().top(px(42.)).left(px(left)).child(
                        crate::motion::pop(
                            self.render_chat_history(cx),
                            crate::motion::Pop::menu((0.1, 0.)),
                            history_pose,
                        ),
                    )),
                )
            })
            .when(self.visible_menu.is_some(), |s| {
                s.child(self.render_menu(menu_pose, window, cx))
            })
            .when(update_pose.visible, |s| {
                s.child(self.render_update_notice(update_pose, window, cx))
            })
            .when(toast_pose.visible, |s| s.child(self.render_toast(toast_pose, window, cx)));
        let root = root.children(self.render_film_pointer());
        // With `CEREAL_FRAME_STATS`, a probe painted after everything else
        // marks the end of the frame's paint pass.
        let root = root.when(crate::motion::stats::enabled(), |root| {
            root.child(
                canvas(|_, _, _| (), |_, _, _, _| crate::motion::stats::painted())
                    .absolute()
                    .size(px(0.)),
            )
        });
        // Every press and release in the window, whatever is under the pointer (an open menu's
        // overlay covers the window and takes the press). Presses are counted before any element
        // sees them, so a menu a press closes records which press it was ([`Cereal::open_menu`]).
        // The click a press completes is handled while its release is dispatched; once that is
        // done the record has served its purpose, so a later click, a keyboard activation or a
        // programmatic open is never refused because of it.
        let view = cx.entity().downgrade();
        let root = root.child(
            canvas(
                |_, _, _| (),
                move |_, _, window, _| {
                    let down = view.clone();
                    window.on_mouse_event(move |_: &MouseDownEvent, phase, _, cx| {
                        if phase == DispatchPhase::Capture {
                            let _ = down.update(cx, |this, _| this.press_seq += 1);
                        }
                    });
                    let up = view.clone();
                    window.on_mouse_event(move |_: &MouseUpEvent, phase, window, cx| {
                        if phase == DispatchPhase::Capture {
                            let up = up.clone();
                            window.defer(cx, move |_, cx| {
                                let _ = up.update(cx, |this, _| this.menu_closed_by_press = None);
                            });
                        }
                    });
                },
            )
            .absolute()
            .size(px(0.)),
        );
        self.stage.flush(window);
        root
    }
}

pub fn init(cx: &mut App) {
    cx.bind_keys([
        // Both ⌘N and ⇧⌘N make a new note. The later binding is the one the
        // menu bar shows.
        KeyBinding::new("cmd-n", NewNote, Some("Cereal")),
        KeyBinding::new("cmd-shift-n", NewNote, Some("Cereal")),
        KeyBinding::new("cmd-k", Search, Some("Cereal")),
        KeyBinding::new("cmd-f", Search, Some("Cereal")),
        KeyBinding::new("escape", Escape, Some("Cereal")),
        KeyBinding::new("tab", FocusNext, Some("Cereal")),
        KeyBinding::new("shift-tab", FocusPrevious, Some("Cereal")),
        KeyBinding::new("cmd-1", GoHome, Some("Cereal")),
        KeyBinding::new("cmd-,", OpenSettings, Some("Cereal")),
        KeyBinding::new("cmd-b", ToggleSidebar, Some("Cereal")),
        KeyBinding::new("cmd-s", Save, Some("Cereal")),
        KeyBinding::new("cmd-w", CloseWindow, Some("Cereal")),
        KeyBinding::new("cmd-h", Hide, Some("Cereal")),
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-alt-q", QuitCompletely, None),
    ]);
    crate::recording_view::init(cx);
}

/// A delayed dismissal belongs only to the exact toast that scheduled it.
fn toast_should_expire(
    scheduled_epoch: u64,
    current_epoch: u64,
    scheduled_message: &str,
    current_message: Option<&str>,
) -> bool {
    scheduled_epoch == current_epoch && current_message == Some(scheduled_message)
}

#[cfg(test)]
mod layer_tests {
    use super::{Layer, Menu, calendar_event_offer, toast_place, top_layer};

    #[test]
    fn escape_closes_the_topmost_layer_and_only_that_one() {
        let top = |model, card, palette, menu: Option<Menu>, asking, sidebar, history| {
            top_layer(model, card, palette, menu.as_ref(), asking, sidebar, history)
        };
        // Nothing is open: nothing to close.
        assert_eq!(top(false, false, false, None, false, false, false), None);
        // A menu over the sidebar that slid over a narrow window: the menu
        // goes first and the sidebar stays.
        assert_eq!(
            top(false, false, false, Some(Menu::Workspace), false, true, false),
            Some(Layer::Menu)
        );
        assert_eq!(top(false, false, false, None, false, true, false), Some(Layer::Sidebar));
        // The palette is above any menu; the model menu above everything.
        assert_eq!(
            top(false, false, true, Some(Menu::Note), false, true, true),
            Some(Layer::Palette)
        );
        assert_eq!(
            top(true, true, true, Some(Menu::Note), false, true, true),
            Some(Layer::ModelMenu)
        );
        assert_eq!(top(false, true, false, None, false, false, true), Some(Layer::SourceCard));
        assert_eq!(top(false, false, false, None, false, false, true), Some(Layer::ChatHistory));
        // The event picker folds back into the details card it came from;
        // the card itself is an ordinary layer and closes on the next one.
        assert_eq!(
            top(false, false, false, Some(Menu::CalendarEvent), false, false, false),
            Some(Layer::EventPicker)
        );
        assert_eq!(
            top(false, false, false, Some(Menu::NoteDetails), false, false, false),
            Some(Layer::Menu)
        );
        // A Trash row that is asking lets go of its question first; the
        // question belongs to Trash alone.
        assert_eq!(
            top(false, false, false, Some(Menu::Trash), true, false, false),
            Some(Layer::TrashQuestion)
        );
        assert_eq!(
            top(false, false, false, Some(Menu::Trash), false, false, false),
            Some(Layer::Menu)
        );
        assert_eq!(
            top(false, false, false, Some(Menu::Note), true, false, false),
            Some(Layer::Menu)
        );
    }

    #[test]
    fn typing_in_the_event_picker_narrows_the_list_that_is_on_screen() {
        let notes = [
            (5, "2026-11-12", "Weekly team standup"),
            (4, "2026-11-12", "Product & design sync"),
            (3, "2026-11-11", "Coffee with Maya"),
            (2, "2026-11-11", "Customer discovery"),
            (1, "2026-11-10", "Brand exploration"),
        ];
        // Nothing from today on: the three most recent, without the note
        // being edited.
        let today = "2026-11-18";
        assert_eq!(calendar_event_offer(&notes, Some(5), today, ""), [4, 3, 2]);
        // Typing narrows those same three (it used to find nothing at all).
        assert_eq!(calendar_event_offer(&notes, Some(5), today, "cof"), [3]);
        assert_eq!(calendar_event_offer(&notes, Some(5), today, " Cof "), [3]);
        assert_eq!(calendar_event_offer(&notes, Some(5), today, "c"), [4, 3, 2]);
        assert_eq!(calendar_event_offer(&notes, Some(5), today, "PRODUCT"), [4]);
        // What is not on the list is not found by typing its name.
        assert!(calendar_event_offer(&notes, Some(5), today, "brand").is_empty());
        assert!(calendar_event_offer(&notes, Some(5), today, "zzz").is_empty());
        // With meetings from today on, those are the list, six at most.
        let coming: Vec<(u64, &str, &str)> = (0..9).map(|id| (id, "2026-11-20", "Planning")).collect();
        let offered = calendar_event_offer(&coming, Some(0), today, "");
        assert_eq!(offered, [1, 2, 3, 4, 5, 6]);
        assert_eq!(calendar_event_offer(&coming, None, today, "plan").len(), 6);
        let mixed = [(2, "2026-11-19", "Roadmap"), (1, "2026-11-01", "Retro")];
        assert_eq!(calendar_event_offer(&mixed, None, today, ""), [2]);
        assert!(calendar_event_offer(&mixed, None, today, "retro").is_empty());
    }

    #[test]
    fn a_toast_sits_in_the_title_bar_clear_of_its_controls_or_under_it() {
        // A wide window: the pill has its content's width, centred in the bar.
        assert_eq!(toast_place(260., 1280., 240., 112., None), (260., Some(640.)));
        // Never wider than the widest, however much it has to say.
        assert_eq!(toast_place(900., 1280., 148., 112., None), (440., Some(640.)));
        // Controls reaching far in from one side squeeze it, both ways:
        // it is centred, so the nearer side decides.
        let (width, centre) = toast_place(400., 900., 308., 112., None);
        assert_eq!(centre, Some(450.));
        assert!((width - 2. * (450. - 308. - 10.)).abs() < 0.01, "{width}");
        assert_eq!(toast_place(400., 900., 112., 308., None), (width, centre));
        // It stays clear of them on both sides.
        assert!(450. - width / 2. >= 308. + 10. - 0.01);
        // Too little room between them: it hangs under the bar instead,
        // as wide as it wants, inside the window.
        assert_eq!(toast_place(300., 760., 308., 112., None), (300., None));
        assert_eq!(toast_place(900., 400., 148., 112., None), (368., None));
        // A short toast fits where a long one would not.
        assert_eq!(toast_place(180., 760., 270., 112., None), (180., Some(380.)));
    }

    #[test]
    fn a_toast_goes_beside_the_recordings_pill_never_over_or_under_it() {
        // The pill is 124 pt wide in the middle of a 1240 pt bar: the toast
        // starts a gap to its right and ends short of the bar's controls.
        let (width, centre) = toast_place(300., 1240., 131., 154., Some(124.));
        let centre = centre.unwrap();
        assert_eq!(width, 300.);
        assert_eq!(centre - width / 2., 620. + 62. + 10.);
        assert!(centre + width / 2. <= 1240. - 154. - 10.);
        // No room on the right (wide controls there): it goes to the left,
        // ending a gap before the pill.
        let (width, centre) = toast_place(300., 1240., 131., 500., Some(124.));
        assert_eq!(centre.unwrap() + width / 2., 620. - 62. - 10.);
        assert!(centre.unwrap() - width / 2. >= 131. + 10.);
        // Squeezed on the right but still readable: narrower, in the bar.
        let (width, centre) = toast_place(440., 1240., 131., 154., Some(240.));
        assert!(centre.is_some() && (240. ..440.).contains(&width), "{width}");
        // No room on either side: under the bar.
        assert_eq!(toast_place(300., 760., 200., 150., Some(240.)), (300., None));
    }
}

#[cfg(test)]
mod toast_tests {
    use super::toast_should_expire;

    #[test]
    fn stale_toast_timer_cannot_dismiss_a_newer_identical_message() {
        assert!(!toast_should_expire(
            1,
            2,
            "Note copied to clipboard",
            Some("Note copied to clipboard")
        ));
        assert!(toast_should_expire(
            2,
            2,
            "Note copied to clipboard",
            Some("Note copied to clipboard")
        ));
    }

    #[test]
    fn dismissal_and_direct_error_replacement_invalidate_a_pending_timer() {
        assert!(!toast_should_expire(1, 1, "Note copied to clipboard", None));
        assert!(!toast_should_expire(
            1,
            1,
            "Note copied to clipboard",
            Some("Could not save changes")
        ));
        assert!(!toast_should_expire(1, 2, "Note copied to clipboard", None));
    }
}
