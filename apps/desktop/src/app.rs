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
    Folder(u64),
    Favorites,
    Shared,
    /// The page a link opened: a shared note, read-only, or where a note
    /// is when the app is not signed in to its server (see `shared_view`).
    Link,
    Chat,
    Connect,
    Settings,
    Trash,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NoteTab {
    Notes,
    Enhanced,
}
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Menu {
    RecordingSettings,
    TranscriptionLanguage,
    NoteDetails,
    CalendarEvent,
    RecordingHelp,
    Workspace,
    Note,
    Share,
    Templates,
    MoveTo,
    NewFolder,
    Shortcuts,
    Voice,
    /// Right-click menu of a note row in a list.
    NoteRow,
}

pub struct Cereal {
    pub workspace: Workspace,
    pub login: Entity<crate::login::LoginView>,
    pub account: Entity<crate::account::AccountView>,
    pub screen: Screen,
    pub current_note: Option<u64>,
    pub sidebar_open: bool,
    pub folders_open: bool,
    pub search_open: bool,
    pub search_results_query: String,
    pub search: Entity<TextInput>,
    pub ask: Entity<TextInput>,
    pub title_input: Entity<TextInput>,
    pub editor: Entity<TextInput>,
    pub note_body_dirty: bool,
    pub note_scroll: ScrollHandle,
    pub note_calendar_month: chrono::NaiveDate,
    pub folder_input: Entity<TextInput>,
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
    /// Live transcription of the current recording (see `live_view.rs`).
    pub live: crate::live_view::LiveTranscript,
    pub recording: crate::recording::Recording,
    pub audio_capture: Option<crate::audio::Capture>,
    pub audio_error: Option<String>,
    pub audio_pending: Vec<crate::audio::CaptureMonitor>,
    pub audio_quitting: bool,
    pub server_bridge: Option<crate::server::Bridge>,
    pub server_pending: usize,
    pub server_status: Option<String>,
    pub generation: crate::recording::Generation,
    pub recording_transcript_start: Option<(u64, String)>,
    pub regeneration_notice_dismissed: Option<(u64, u64)>,
    pub recording_from: crate::recording::Phase,
    pub recording_changed_at: Option<std::time::Instant>,
    pub recording_last_click: Option<(bool, std::time::Instant)>,
    pub transcription_language: String,
    pub microphone_muted: bool,
    pub menu: Option<Menu>,
    pub visible_menu: Option<Menu>,
    pub menu_epoch: u64,
    pub menu_keyboard: crate::chrome::MenuKeyboard,
    pub palette: crate::chrome::PaletteKeyboard,
    pub menu_exiting: bool,
    pub toast: Option<String>,
    /// The last toast's text, kept so it can finish leaving after `toast` clears.
    pub toast_shown: String,
    pub toast_epoch: u64,
    /// Animated values for this window (see `motion::Stage`).
    pub stage: crate::motion::Stage,
    /// How much of the sidebar's width is on screen right now, 0..=1.
    pub sidebar_level: f32,
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
    pub list_shown: Option<(std::mem::Discriminant<Screen>, Option<u64>)>,
    /// Screens to return to with the header back pill, most recent last.
    pub nav_history: Vec<Screen>,
    pub settings_tab: crate::screens::SettingsTab,
    /// The note a list-row menu (or a folder menu opened from it) acts on,
    /// and where that menu is anchored.
    pub row_menu: Option<crate::chrome::RowMenu>,
    /// Sidebar footer tool whose label is showing, and the last one shown
    /// (kept so the label can finish fading after the pointer leaves).
    pub footer_tip: Option<usize>,
    pub footer_tip_shown: usize,
    pub chat_models: Vec<meeting_notes_api_types::LanguageModelSelection>,
    pub selected_chat_model: Option<String>,
    pub chat_model_picker_open: bool,
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
    pub chat_recipes: Vec<crate::chat::ChatRecipe>,
    pub chat_recipes_open: bool,
    pub chat_recipe_editor: Option<crate::chat::RecipeEditor>,
    /// The editor that was just closed, kept only to be drawn while it leaves.
    pub recipe_editor_leaving: Option<crate::chat::RecipeEditor>,
    pub chat_maximized: bool,
    pub chat_history_open: bool,
    /// When a press outside the recent-chats list last closed it (see
    /// `toggle_chat_history`).
    pub chat_history_closed_at: Option<std::time::Instant>,
    pub chat_scroll: ScrollHandle,
    pub chat_archives: Vec<crate::chat::ArchivedChat>,
    pub chat_thread_id: String,
    pub calendar_page: i32,
    pub calendars: crate::calendar::CalendarState,
    pub home_scroll: ScrollHandle,
    pub meeting: crate::meeting::MeetingState,
    /// Links into the app and "Shared with me" (see `shared_view.rs`).
    pub links: crate::shared_view::Links,
    /// "Check for updates" (see `update_view.rs`).
    pub updates: crate::update_view::Updates,
    /// The first-run welcome: its step, and what this Mac has allowed.
    pub onboarding: crate::onboarding_view::Onboarding,
    pub focus: FocusHandle,
    pub _subscriptions: Vec<Subscription>,
    /// Registered with the window on the first frame: see `render`.
    focus_lost: Option<Subscription>,
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
        let search = cx.new(|cx| TextInput::new("Search folders or meetings", false, cx));
        let ask = cx.new(|cx| TextInput::new("Ask anything", false, cx));
        ask.update(cx, |input, cx| input.set_chat_mode(cx));
        let title_input = cx.new(|cx| TextInput::new("New note", false, cx));
        title_input.update(cx, |input, cx| input.set_wrap(true, cx));
        let editor =
            cx.new(|cx| TextInput::new("Write notes, or press '/' for templates", true, cx));
        editor.update(cx, |input, _| input.set_list_formatting(true));
        let note_scroll = ScrollHandle::new();
        editor.update(cx, |input, _| input.attach_scroll(note_scroll.clone()));
        let folder_input = cx.new(|cx| TextInput::new("Folder name", false, cx));
        let attendee_input = cx.new(|cx| TextInput::new("Add attendees...", false, cx));
        let menu_key_listener = cx.listener(|this, event: &KeystrokeEvent, window, cx| {
            this.handle_palette_key(event, window, cx);
            if this.focus.contains_focused(window, cx) {
                this.handle_menu_key(event, window, cx);
            }
        });
        let menu_key_subscription = cx.intercept_keystrokes(menu_key_listener);
        let (links, link_subscriptions) = crate::shared_view::Links::new(cx);
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
                    crate::account::AccountEvent::SignOut => this.sign_out_account(cx),
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
            cx.subscribe(&search, |this, _, _: &InputChanged, cx| {
                if this.menu == Some(Menu::MoveTo) {
                    this.menu_keyboard.selected = None;
                }
                cx.notify();
            }),
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
            cx.subscribe(&ask, |this, _, event: &InputChanged, cx| {
                if event.content.starts_with('/') {
                    this.chat_recipes_open = true;
                    this.chat_history_open = false;
                    if this.current_note.is_some() {
                        this.assistant_open = true;
                        this.transcript_open = false;
                    }
                } else if this.chat_recipes_open {
                    this.chat_recipes_open = false;
                }
                cx.notify();
            }),
            cx.subscribe(&ask, |this, _, _: &InputFocused, cx| {
                if this.current_note.is_some()
                    && !this.assistant_open
                    && !this.chat_messages.is_empty()
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
                    this.note_body_dirty |= note.title != ev.content;
                    note.title = ev.content.clone();
                    this.persist(cx);
                }
            }),
            cx.subscribe(&editor, |this, _, ev: &InputChanged, cx| {
                if let Some(note) = this.current_note.and_then(|id| this.workspace.note_mut(id)) {
                    this.note_body_dirty |= note.body != ev.content;
                    note.body = ev.content.clone();
                    this.persist(cx);
                }
            }),
            cx.subscribe(&folder_input, |_, _, _: &InputChanged, cx| cx.notify()),
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
            cx.subscribe(&folder_input, |_, _, _: &InputChanged, cx| cx.notify()),
            cx.subscribe(&folder_input, |this, _, _: &InputSubmitted, cx| {
                this.create_folder(cx)
            }),
        ];
        subscriptions.extend(link_subscriptions);
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
            folders_open: false,
            search_open: false,
            search_results_query: String::new(),
            search,
            ask,
            title_input,
            editor,
            note_body_dirty: false,
            note_scroll,
            note_calendar_month: chrono::Local::now().date_naive(),
            folder_input,
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
            live: Default::default(),
            recording: Default::default(),
            audio_capture: None,
            audio_error: None,
            audio_pending: Vec::new(),
            audio_quitting: false,
            server_bridge: None,
            server_pending: 0,
            server_status: None,
            generation: Default::default(),
            recording_transcript_start: None,
            regeneration_notice_dismissed: None,
            recording_from: crate::recording::Phase::Idle,
            recording_changed_at: None,
            recording_last_click: None,
            transcription_language: "English (best quality)".into(),
            microphone_muted: false,
            menu: None,
            visible_menu: None,
            menu_epoch: 0,
            menu_keyboard: Default::default(),
            palette: Default::default(),
            menu_exiting: false,
            toast: None,
            toast_shown: String::new(),
            toast_epoch: 0,
            stage: Default::default(),
            sidebar_level: 1.,
            search_pose: crate::motion::Pose::REST,
            page_key: 0,
            autosave: Default::default(),
            caches: Default::default(),
            list_shown: None,
            nav_history: Vec::new(),
            settings_tab: Default::default(),
            row_menu: None,
            footer_tip: None,
            footer_tip_shown: 0,
            chat_models: vec![],
            selected_chat_model: None,
            chat_model_picker_open: false,
            chat_messages: vec![],
            chat_annotations: Default::default(),
            chat_epoch: 0,
            chat_stream: None,
            chat_queue: Vec::new(),
            chat_parked: Default::default(),
            chat_interrupted: Default::default(),
            chat_editing: None,
            chat_recipes: crate::chat::load_recipes(),
            chat_recipes_open: false,
            chat_recipe_editor: None,
            recipe_editor_leaving: None,
            chat_maximized: false,
            chat_history_open: false,
            chat_history_closed_at: None,
            chat_scroll: ScrollHandle::new(),
            chat_archives: crate::chat::load_history(),
            chat_thread_id: uuid::Uuid::new_v4().to_string(),
            calendar_page: 0,
            calendars: Default::default(),
            home_scroll: ScrollHandle::new(),
            meeting: Default::default(),
            links,
            updates: Default::default(),
            onboarding: crate::onboarding_view::Onboarding::new(welcome, cx),
            focus: cx.focus_handle(),
            _subscriptions: subscriptions,
            focus_lost: None,
        };
        this.restore_note_chat(0, cx);
        this
    }
    pub fn theme(&self) -> Theme {
        Theme::new(self.workspace.settings.dark_mode)
    }
    pub fn navigate(&mut self, screen: Screen, cx: &mut Context<Self>) {
        // A link's page is shown to anyone: it says where the note is and
        // reads nothing without a session on that server.
        let screen = if crate::server::enabled()
            && crate::account::identity().is_none()
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
    pub fn show_toast(&mut self, message: &str, cx: &mut Context<Self>) {
        self.toast_epoch = self.toast_epoch.wrapping_add(1);
        let epoch = self.toast_epoch;
        let message = message.to_string();
        let account = crate::account::epoch();
        if self.toast.is_some() {
            // A replacement toast pops again instead of silently swapping text.
            self.stage.kick(("toast", 0), 0.6, crate::motion::GOOEY);
        }
        self.toast = Some(message.clone());
        self.toast_shown = message.clone();
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_secs(4))
                .await;
            let _ = this.update(cx, move |this, cx| {
                if account == crate::account::epoch()
                    && toast_should_expire(epoch, this.toast_epoch, &message, this.toast.as_deref())
                {
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

    pub fn open_note(&mut self, id: u64, cx: &mut Context<Self>) {
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
        if crate::server::enabled() && crate::account::identity().is_none() {
            self.screen = Screen::Account;
            self.show_toast("Sign in before creating a server note", cx);
            return;
        }
        self.finish_recording_preview(cx);
        self.recording_last_click = None;
        self.recording_changed_at = None;
        let id = self.workspace.create_note();
        self.workspace.note_mut(id).unwrap().title.clear();
        if let Screen::Folder(folder) = self.screen {
            self.workspace.note_mut(id).unwrap().folder_id = Some(folder);
        }
        self.open_note(id, cx);
        self.active_tab = NoteTab::Notes;
        self.title_input
            .update(cx, |input, cx| input.set_content("", cx));
        self.persist(cx);
        self.start_recording_preview(id, cx);
        self.editor.read(cx).focus_handle.focus(window);
    }
    pub fn create_folder(&mut self, cx: &mut Context<Self>) {
        let name = self.folder_input.read(cx).content().trim().to_string();
        if name.is_empty() {
            return;
        }
        let id = self.workspace.add_folder(name);
        self.folder_input
            .update(cx, |input, cx| input.set_content("", cx));
        self.folders_open = true;
        self.navigate(Screen::Folder(id), cx);
        self.persist(cx);
    }
    pub fn calendar_events(&self, query: &str) -> Vec<crate::model::Note> {
        let today = chrono::Local::now()
            .date_naive()
            .format("%Y-%m-%d")
            .to_string();
        let mut events: Vec<_> = self
            .workspace
            .visible_notes(None, false, query)
            .into_iter()
            .filter(|note| Some(note.id) != self.current_note && note.date >= today)
            .take(6)
            .cloned()
            .collect();
        if events.is_empty() && query.trim().is_empty() {
            events = self
                .workspace
                .visible_notes(None, false, "")
                .into_iter()
                .filter(|note| Some(note.id) != self.current_note)
                .take(3)
                .cloned()
                .collect();
        }
        events
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
    pub fn submit_chat(&mut self, cx: &mut Context<Self>) {
        let question = self.ask.read(cx).content().trim().to_string();
        if question.is_empty() {
            return;
        }
        if question.starts_with('/') {
            self.submit_matching_chat_recipe(&question, cx);
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
        let notes = self.workspace.visible_notes(None, false, "");
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
                self.screen = Screen::Chat;
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
        self.navigate(Screen::SearchResults, cx);
    }
    pub fn open_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.summary.dismiss();
        self.transcript_find_open = false;
        self.visible_menu = None;
        self.menu_exiting = false;
        self.menu_epoch = self.menu_epoch.wrapping_add(1);
        self.search.update(cx, |input, cx| {
            input.set_placeholder("Search folders or meetings", cx)
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
        if self.chat_model_picker_open {
            self.chat_model_picker_open = false;
        } else if self.summary.open.is_some() {
            self.summary.dismiss();
        } else if self.chat_recipe_editor.is_some() {
            self.close_recipe_editor();
        } else if self.chat_recipes_open {
            self.close_chat_recipes(cx);
        } else if self.search_open {
            self.search_open = false;
        } else if self.menu.is_some() {
            self.menu = None;
        } else if self.transcript_find_open {
            self.transcript_find_open = false;
        } else if self.transcript_open {
            self.transcript_open = false;
            self.transcript_epoch = self.transcript_epoch.wrapping_add(1);
        } else if self.chat_history_open {
            self.chat_history_open = false;
        } else if self.chat_editing.is_some() {
            self.cancel_chat_edit(cx);
        } else if self.chat_maximized {
            self.chat_maximized = false;
        } else if self.assistant_open {
            self.close_chat_surface(cx);
        } else if self.current_note.is_some() {
            self.navigate(Screen::Home, cx);
        } else if self.screen == Screen::Link {
            self.go_back(cx);
        }
        self.focus.focus(window);
        cx.notify();
    }
}

impl Focusable for Cereal {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Cereal {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use crate::motion::{GOOEY, SETTLE};
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
                // A row's target survives only inside its own menu and the folder
                // picker opened from it; any other menu acts on the open note.
                if !(menu == Menu::NoteRow
                    || (menu == Menu::MoveTo && self.visible_menu == Some(Menu::NoteRow)))
                {
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
                Some(Menu::NewFolder | Menu::NoteDetails | Menu::CalendarEvent | Menu::MoveTo)
            )
        {
            self.focus.focus(window);
        }
        self.stage.begin();
        crate::motion::frame();
        crate::ui::set_dark(self.workspace.settings.dark_mode);
        crate::ui::provision_focus(cx);
        crate::ui::set_popover_open(self.visible_menu.is_some() || self.search_open);
        self.drain_chat_history(cx);
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
                Screen::Home | Screen::MyNotes | Screen::Folder(_) | Screen::Favorites => {
                    self.render_home(window, cx)
                }
                _ => self.render_screen(window, cx),
            }
        };
        let sidebar = self.stage.presence(
            ("sidebar", 0),
            // A link's page is a note page: it has the window to itself.
            self.sidebar_open
                && !login
                && self.screen != Screen::Link
                && (self.current_note.is_none() || self.chat_maximized),
            SETTLE,
        );
        self.sidebar_level = if sidebar.visible {
            sidebar.value.clamp(0., 1.)
        } else {
            0.
        };
        self.search_pose = self.stage.presence(("search", 0), self.search_open, GOOEY);
        let toast_pose = self
            .stage
            .presence(("toast", 0), self.toast.is_some(), GOOEY);
        let update_pose =
            self.stage
                .presence(("update-notice", 0), self.updates.notice_open(), GOOEY);
        let recipe_pose = self.stage.presence(
            ("recipe-editor", 0),
            self.chat_recipe_editor.is_some(),
            GOOEY,
        );
        if !recipe_pose.visible || self.chat_recipe_editor.is_some() {
            self.recipe_editor_leaving = None;
        }
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
            if let Screen::Folder(folder) = self.screen {
                folder.hash(&mut hasher);
            }
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
            let folder = match self.screen {
                Screen::Folder(folder) => Some(folder),
                _ => None,
            };
            if self.current_note.is_none() && self.list_shown != Some((list_key, folder)) {
                self.list_shown = Some((list_key, folder));
                self.stage.mark("page");
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
                this.sidebar_open = !this.sidebar_open;
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
            .on_action(cx.listener(|this, _: &Quit, _, cx| this.quit_after_audio_stop(cx)))
            .on_action(cx.listener(|this, _: &CheckForUpdates, _, cx| this.check_for_updates(cx)))
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
            .when(sidebar.visible, |s| {
                // The sidebar keeps its natural width and slides out from under
                // a shrinking clip, so its own rows never reflow.
                s.child(
                    div()
                        .relative()
                        // Clamped: the spring's overshoot must not shove the
                        // whole page sideways and back.
                        .w(px(192. * sidebar.value.clamp(0., 1.)))
                        .h_full()
                        .flex_shrink_0()
                        .overflow_hidden()
                        .bg(t.sidebar)
                        // It thins out as it slides, so its toolbar never
                        // reads as colliding with the traffic lights.
                        .opacity(sidebar.fade * sidebar.value.clamp(0., 1.))
                        .child(
                            div()
                                .absolute()
                                .top_0()
                                .bottom_0()
                                .right_0()
                                .w(px(192.))
                                .child(self.render_sidebar(cx)),
                        ),
                )
            })
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .when(!login && !full_chat, |s| s.child(self.render_header(cx)))
                    .when(!login && full_chat, |s| {
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
            .when(self.search_pose.visible, |s| {
                s.child(self.render_search(cx))
            })
            .when(history_pose.visible, |s| {
                s.child(
                    div()
                        .absolute()
                        .top(px(42.))
                        .left(px(192. * self.sidebar_level
                            + if self.current_note.is_some() { 62. } else { 8. }))
                        .child(crate::motion::pop(
                            self.render_chat_history(cx),
                            crate::motion::Pop::menu((0.1, 0.)),
                            history_pose,
                        ))
                        .on_mouse_down_out(cx.listener(|this, _, _, cx| this.dismiss_chat_history(cx))),
                )
            })
            .when(self.visible_menu.is_some(), |s| {
                s.child(self.render_menu(menu_pose, window, cx))
            })
            .when(recipe_pose.visible, |s| {
                s.child(self.render_chat_recipe_editor(recipe_pose, window, cx))
            })
            .when(update_pose.visible, |s| {
                s.child(self.render_update_notice(update_pose, window, cx))
            })
            .when(toast_pose.visible, |s| {
                let toast = self
                    .toast
                    .clone()
                    .unwrap_or_else(|| self.toast_shown.clone());
                let (title, description) = toast
                    .split_once('\n')
                    .map(|(title, description)| (title, Some(description)))
                    .unwrap_or((&toast, None));
                let success = title == "Note copied to clipboard"
                    || title == "Link copied to clipboard"
                    || title == "All changes saved"
                    || title.starts_with("Cereal is up to date");
                s.child(crate::motion::pop(
                    div()
                        .id("toast-notification")
                        .absolute()
                        // A toast sits above the update notice while that is up.
                        .bottom(px(if update_pose.visible {
                            16. + crate::update_view::NOTICE_HEIGHT + 8.
                        } else {
                            16.
                        }))
                        .right(px(16.))
                        .w(px(384.))
                        .max_w(window.viewport_size().width - px(32.))
                        .min_h(px(56.))
                        .pl(px(16.))
                        .pr(px(8.))
                        .py(px(10.))
                        .rounded(px(12.))
                        .bg(t.card)
                        .border_1()
                        .border_color(t.border)
                        .shadow_lg()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .flex()
                        .items_center()
                        .gap(px(12.))
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
                        .child(
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
                                        .child(title.to_string()),
                                )
                                .when_some(description, |d, description| {
                                    d.child(
                                        div()
                                            .text_size(px(12.))
                                            .line_height(px(16.))
                                            .text_color(t.muted)
                                            .child(description.to_string()),
                                    )
                                }),
                        )
                        .child(
                            tool_tip("dismiss-toast", "close", "Dismiss notification", t)
                                .size(px(20.))
                                .on_click(cx.listener(|this, _, _, cx| this.dismiss_toast(cx))),
                        ),
                    crate::motion::Pop::chip((1., 1.)),
                    toast_pose,
                ))
            });
        // With `CEREAL_FRAME_STATS`, a probe painted after everything else
        // marks the end of the frame's paint pass.
        let root = root.when(crate::motion::stats::enabled(), |root| {
            root.child(
                canvas(|_, _, _| (), |_, _, _, _| crate::motion::stats::painted())
                    .absolute()
                    .size(px(0.)),
            )
        });
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
    ]);
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
