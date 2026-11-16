//! Links into the app, and the pages they open. A `cereal://` URL from the
//! system, a row of "Shared with me" and a pasted link all arrive at
//! [`Cereal::open_link`]. A note someone shared opens as the app's own note
//! page in a read-only form; a note on a server the app is not signed in to
//! opens as a page that says where the note is.
use crate::{
    app::{Cereal, NoteTab, Screen},
    chrome::{BackLabel, back_pill, screen_icon},
    input::{InputChanged, InputSubmitted, TextInput},
    link::{Access, AppLink, NoteLink, ServerOrigin, SharedNoteLink},
    note::{NoteTabs, note_meta_chip, note_tab_label_width},
    shared::{History, SharedNote, SharedNoteError, shown},
    ui::*,
};
use gpui::{prelude::*, *};
use std::cell::RefCell;

thread_local! {
    /// URLs the system delivered that the window has not read yet. One that
    /// arrives as the app launches waits here for the window to exist.
    static INBOX: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static WINDOW: RefCell<Option<(AsyncApp, WindowHandle<Cereal>)>> = const { RefCell::new(None) };
}

/// The system asked the app to open these URLs.
pub fn receive(urls: Vec<String>) {
    INBOX.with(|inbox| inbox.borrow_mut().extend(urls));
    // Read on the next turn of the event loop, never from inside the
    // system's own call.
    if let Some((app, handle)) = WINDOW.with(|window| window.borrow().clone()) {
        app.spawn(async move |cx| {
            let _ = handle.update(cx, |view, window, cx| view.drain_links(window, cx));
        })
        .detach();
    }
}

/// The window exists: read what arrived before it did, and from now on read
/// URLs as they arrive.
pub fn install(handle: WindowHandle<Cereal>, cx: &mut App) {
    WINDOW.with(|window| *window.borrow_mut() = Some((cx.to_async(), handle)));
    let _ = handle.update(cx, |view, window, cx| view.drain_links(window, cx));
}

/// Open a web page in the default browser. Only ever a page of a server a
/// validated link or address names. A debug build given `CEREAL_BROWSER_LOG`
/// writes the address to that file instead, so a test can play the browser's
/// part without opening one.
pub fn open_in_browser(url: &str, cx: &mut App) {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return;
    }
    #[cfg(debug_assertions)]
    if let Some(path) = crate::environment::var_os("CEREAL_BROWSER_LOG").filter(|p| !p.is_empty()) {
        use std::io::Write;
        if let Ok(mut log) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(log, "{url}");
        }
        return;
    }
    cx.open_url(url);
}

/// The identity the read-only note page gives the note renderer. No note of
/// the workspace has it (it is never a handle, never looked up and never
/// written as a note's id), so nothing that acts on a workspace note applies.
use crate::accounts::handle::SHARED_NOTE_ID;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    Loading,
    Loaded(SharedNote),
    Failed(SharedNoteError),
}

/// What `Screen::Link` shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkPage {
    /// A note someone shared.
    Shared { link: SharedNoteLink, phase: Phase },
    /// A note of one's own on a server the app is not signed in to.
    Note(NoteLink),
}

impl LinkPage {
    fn link(&self) -> AppLink {
        match self {
            LinkPage::Shared { link, .. } => AppLink::Shared(link.clone()),
            LinkPage::Note(link) => AppLink::Note(link.clone()),
        }
    }
}

pub struct Links {
    /// A link that arrived before the session was known: it is followed once
    /// the account has loaded, or once it is clear nobody is signed in.
    pub pending: Option<AppLink>,
    pub page: Option<LinkPage>,
    /// The note a sign-in was started for; it opens when that sign-in ends.
    pub after_sign_in: Option<AppLink>,
    pub history: History,
    /// The "Paste link" field of Shared with me, and what was wrong with
    /// the last thing entered there.
    pub paste: Entity<TextInput>,
    pub problem: Option<String>,
    paste_length: usize,
    /// The sharer's own notes, shown in the note editor's place and face
    /// but not editable.
    body: Entity<TextInput>,
    tab: NoteTab,
    /// Counts the pages shown, so an answer that arrives for a page that has
    /// been replaced is dropped.
    pub epoch: u64,
}

impl Links {
    pub fn new(cx: &mut Context<Cereal>) -> (Self, Vec<Subscription>) {
        let paste = cx.new(|cx| TextInput::new("Paste link", false, cx));
        let body = cx.new(|cx| TextInput::new("", true, cx));
        body.update(cx, |input, _| input.set_read_only(true));
        let subscriptions = vec![
            cx.subscribe(&paste, |this, _, event: &InputChanged, cx| {
                this.links.problem = None;
                let grown = event.content.len().saturating_sub(this.links.paste_length);
                this.links.paste_length = event.content.len();
                // A link that arrives whole was pasted: open it without
                // another key.
                if grown > 1 && AppLink::parse_note(&event.content).is_some() {
                    this.open_pasted_link(cx);
                }
                cx.notify();
            }),
            cx.subscribe(&paste, |this, _, _: &InputSubmitted, cx| {
                this.open_pasted_link(cx)
            }),
        ];
        (
            Self {
                pending: None,
                page: None,
                after_sign_in: None,
                history: History::load(),
                paste,
                problem: None,
                paste_length: 0,
                body,
                tab: NoteTab::Enhanced,
                epoch: 0,
            },
            subscriptions,
        )
    }
}

/// What the "Paste link" field says about text that is not a note's link.
pub const NOT_A_LINK: &str =
    "That is not a link to a Cereal note. A shared link looks like https://notes.example.com/s/…";

impl Cereal {
    /// Read the URLs the system delivered since the last time.
    pub fn drain_links(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let urls = INBOX.with(|inbox| std::mem::take(&mut *inbox.borrow_mut()));
        // More than a few at once is not a person following links.
        for url in urls.iter().take(4) {
            crate::pill::reveal_main_window(window);
            window.activate_window();
            cx.activate(true);
            match AppLink::parse(url) {
                Some(link) => self.open_link(link, cx),
                // No link is dropped without a word.
                None => self.show_toast("Cereal could not open that link", cx),
            }
        }
    }

    /// Follow a link from outside the app, from "Shared with me" or from a
    /// pasted link.
    pub fn open_link(&mut self, link: AppLink, cx: &mut Context<Self>) {
        if let AppLink::Auth(answer) = link {
            // Only the sign-in that is waiting for an answer can use one.
            self.account
                .update(cx, |account, cx| account.receive_answer(answer, cx));
            return;
        }
        // The saved session is still being checked with its server: what
        // this link may open is not known until that ends.
        if crate::link::must_wait(
            crate::server::enabled(),
            crate::accounts::any_shown(),
            crate::accounts::any_checking() || !self.verifying.is_empty(),
        ) {
            self.links.pending = Some(link);
            return;
        }
        self.follow_link(link, cx);
    }

    /// The session is known now, one way or the other: follow the link that
    /// was waiting for it, or the note a sign-in was started for.
    pub fn resume_links(&mut self, cx: &mut Context<Self>) {
        let waiting = self.links.pending.take();
        let wanted = self.links.after_sign_in.take().filter(|link| {
            link.origin()
                .is_some_and(|origin| self.link_access(origin) == Access::Read)
        });
        if let Some(link) = waiting.or(wanted) {
            self.follow_link(link, cx);
        }
    }

    fn follow_link(&mut self, link: AppLink, cx: &mut Context<Self>) {
        let Some(origin) = link.origin() else { return };
        let access = self.link_access(origin);
        // A note of one's own on the signed-in server opens as itself.
        if access == Access::Read
            && let AppLink::Note(note) = &link
            && let Some(account) = self.link_reader(origin)
        {
            let id = note.note_id.clone();
            self.open_server_note(account.id, id, cx);
            return;
        }
        self.show_link_page(link, cx);
    }

    /// What a link to a note on `origin` may do, given what the app is
    /// connected to right now.
    fn link_access(&self, origin: &ServerOrigin) -> Access {
        if !crate::server::enabled() {
            return crate::link::access(origin, None, false);
        }
        // Read with an account on the link's own server, and no other.
        if let Some(reader) = self.link_reader(origin) {
            return crate::link::access(origin, Some(&reader.url), true);
        }
        let signed_in = crate::accounts::any_shown();
        let server = crate::accounts::configured_url();
        crate::link::access(origin, Some(&server), signed_in)
    }

    /// The account a link on `origin` is read with: the first, in account
    /// order, on that origin (with no path in its address) that is Ready.
    fn link_reader(&self, origin: &ServerOrigin) -> Option<crate::accounts::Account> {
        let registry = crate::accounts::registry();
        let on_origin: Vec<crate::accounts::Account> =
            registry.on(origin).into_iter().cloned().collect();
        on_origin
            .into_iter()
            .find(|account| account.shown() && account.is_ready())
    }

    fn show_link_page(&mut self, link: AppLink, cx: &mut Context<Self>) {
        let page = match link {
            AppLink::Shared(link) => LinkPage::Shared {
                link,
                phase: Phase::Loading,
            },
            AppLink::Note(link) => LinkPage::Note(link),
            AppLink::Auth(_) => return,
        };
        self.navigate(Screen::Link, cx);
        self.links.epoch = self.links.epoch.wrapping_add(1);
        self.links.tab = NoteTab::Enhanced;
        self.links.page = Some(page);
        self.note_scroll.set_offset(point(px(0.), px(0.)));
        self.load_shared_note(cx);
        cx.notify();
    }

    /// Read the shared note on the page, when the app is signed in to its
    /// server. Without such a session nothing is asked of anyone.
    fn load_shared_note(&mut self, cx: &mut Context<Self>) {
        let Some(LinkPage::Shared { link, .. }) = &self.links.page else {
            return;
        };
        let link = link.clone();
        if self.link_access(&link.origin) != Access::Read {
            return;
        }
        if let Some(LinkPage::Shared { phase, .. }) = &mut self.links.page {
            *phase = Phase::Loading;
        }
        let epoch = self.links.epoch;
        let Some(reader) = self.link_reader(&link.origin) else {
            return;
        };
        let account = reader.epoch();
        cx.spawn(async move |this, cx| {
            let asked = link.clone();
            let with = reader.id.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    let client = crate::accounts::client(&with).ok();
                    crate::shared::fetch(&asked, client.as_ref())
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.links.epoch != epoch
                    || crate::accounts::epoch_of(&reader.id) != Some(account)
                {
                    return;
                }
                let phase = match result {
                    Ok(note) => {
                        this.links
                            .history
                            .record(&link, &note.display_title(), chrono::Utc::now());
                        if let Err(error) = this.links.history.save() {
                            eprintln!("{error}");
                        }
                        let body = shown(&note.body).0.to_owned();
                        this.links
                            .body
                            .update(cx, |input, cx| input.set_content(body, cx));
                        this.links.tab = if note.enhanced.trim().is_empty() {
                            NoteTab::Notes
                        } else {
                            NoteTab::Enhanced
                        };
                        Phase::Loaded(note)
                    }
                    Err(error) => Phase::Failed(error),
                };
                if let Some(LinkPage::Shared { phase: current, .. }) = &mut this.links.page {
                    *current = phase;
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn retry_shared_note(&mut self, cx: &mut Context<Self>) {
        self.links.epoch = self.links.epoch.wrapping_add(1);
        self.load_shared_note(cx);
        cx.notify();
    }

    /// Open the app's sign-in with the address of the server the page's note
    /// is on. The note opens when that sign-in ends.
    fn sign_in_for_link(&mut self, cx: &mut Context<Self>) {
        let Some(link) = self.links.page.as_ref().map(LinkPage::link) else {
            return;
        };
        let Some(server) = link.origin().map(ServerOrigin::url) else {
            return;
        };
        self.account
            .update(cx, |account, cx| account.sign_in_to(server, cx));
        self.navigate(Screen::Account, cx);
        self.links.after_sign_in = Some(link);
        cx.notify();
    }

    fn open_link_in_browser(&mut self, cx: &mut Context<Self>) {
        if let Some(url) = self.links.page.as_ref().and_then(|page| page.link().web_url()) {
            open_in_browser(&url, cx);
        }
    }

    fn copy_shared_link(&mut self, cx: &mut Context<Self>) {
        if let Some(url) = self.links.page.as_ref().and_then(|page| page.link().web_url()) {
            cx.write_to_clipboard(ClipboardItem::new_string(url));
            self.show_toast("Link copied to clipboard", cx);
        }
    }

    /// Open what is in the "Paste link" field: a web link to a note or its
    /// `cereal://` form.
    pub fn open_pasted_link(&mut self, cx: &mut Context<Self>) {
        let text = self.links.paste.read(cx).content();
        if text.trim().is_empty() {
            return;
        }
        let Some(link) = AppLink::parse_note(&text) else {
            self.links.problem = Some(NOT_A_LINK.into());
            cx.notify();
            return;
        };
        self.links.problem = None;
        self.links.paste_length = 0;
        self.links
            .paste
            .update(cx, |input, cx| input.set_content("", cx));
        self.open_link(link, cx);
    }

    pub fn remove_shared_visit(&mut self, link: &SharedNoteLink, cx: &mut Context<Self>) {
        if self.links.history.remove(link)
            && let Err(error) = self.links.history.save()
        {
            self.show_toast(&error, cx);
        }
        cx.notify();
    }

    /// The header of a link's page: a note's header, with the way back to
    /// where the link was opened from and, for a note that is being read,
    /// what can be done with its link.
    pub fn render_link_header(&self, cx: &mut Context<Self>) -> Div {
        let t = self.theme();
        // As on a note: the session pill belongs to the other pages.
        self.stage
            .presence(("active-session-pill", 0), false, crate::motion::GOOEY);
        let reading = matches!(
            &self.links.page,
            Some(LinkPage::Shared { link, phase: Phase::Loaded(_) })
                if self.link_access(&link.origin) == Access::Read
        );
        div()
            .h(px(48.))
            .flex_shrink_0()
            .flex()
            .items_start()
            .justify_between()
            // Clear the traffic lights, easing as the sidebar slides away.
            .pl(px(88. - 80. * self.sidebar_level))
            .pr(px(8.))
            .pt(px(8.))
            .child(
                back_pill(
                    "link-back",
                    BackLabel::Icon(screen_icon(self.back_target())),
                    t,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.go_back(cx);
                    this.focus.focus(window);
                })),
            )
            .when(reading, |header| {
                header.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            pill("shared-copy-link", "Copy link", "link-chain", t).on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.focus.focus(window);
                                    this.copy_shared_link(cx);
                                }),
                            ),
                        )
                        .child(
                            pill("shared-open-browser", "Open in browser", "globe", t).on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.focus.focus(window);
                                    this.open_link_in_browser(cx);
                                }),
                            ),
                        ),
                )
            })
    }

    pub fn render_link_page(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(page) = &self.links.page else {
            return div().flex_1().into_any_element();
        };
        let (origin, shared) = match page {
            LinkPage::Shared { link, phase } => (&link.origin, Some(phase)),
            LinkPage::Note(link) => (&link.origin, None),
        };
        let host = origin.display_host();
        let browser = |this: &Self, cx: &mut Context<Self>| {
            pill("link-open-browser", "Open in browser", "globe", this.theme()).on_click(
                cx.listener(|this, _, window, cx| {
                    this.focus.focus(window);
                    this.open_link_in_browser(cx);
                }),
            )
        };
        let access = match (self.link_access(origin), shared) {
            // The server no longer accepts the session it gave this app.
            (Access::Read, Some(Phase::Failed(SharedNoteError::SignInRequired))) => {
                Access::SignIn { offered: true }
            }
            (access, _) => access,
        };
        let phase = match (access, shared) {
            (Access::SignIn { offered }, _) => {
                return self.render_sign_in_prompt(origin, offered, cx);
            }
            // A note of one's own on the signed-in server opens as itself;
            // this page is only ever shown for one that is elsewhere.
            (Access::Read, None) => return div().flex_1().into_any_element(),
            (Access::Read, Some(phase)) => phase,
        };
        match phase {
            Phase::Loaded(note) => self.render_shared_note(note, &host, window, cx),
            Phase::Loading => self.render_shared_loading(window),
            Phase::Failed(SharedNoteError::Unavailable) => link_prompt(
                "link-chain",
                "This shared note is not available",
                Some("The link may have expired or been turned off.".into()),
                Some(host),
                vec![browser(self, cx).into_any_element()],
                self.theme(),
            ),
            Phase::Failed(SharedNoteError::TooLarge) => link_prompt(
                "link-chain",
                "This shared note is too large to open here",
                None,
                Some(host),
                vec![browser(self, cx).into_any_element()],
                self.theme(),
            ),
            Phase::Failed(error) => {
                let mut actions = Vec::new();
                if error.is_retryable() {
                    actions.push(
                        primary_pill("shared-retry", "Retry", self.theme())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.focus.focus(window);
                                this.retry_shared_note(cx);
                            }))
                            .into_any_element(),
                    );
                }
                actions.push(browser(self, cx).into_any_element());
                link_prompt(
                    "link-chain",
                    "This shared note could not be loaded",
                    Some(error.message()),
                    Some(host),
                    actions,
                    self.theme(),
                )
            }
        }
    }

    /// The page for a note this app cannot read: it is on a server the app
    /// is not signed in to. It says which server, and offers the browser.
    /// Where signing in would not replace an account in use, it offers that
    /// too, with the server's address filled in.
    fn render_sign_in_prompt(
        &self,
        origin: &ServerOrigin,
        offered: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = self.theme();
        // The server the app is signed in to, when it is another one.
        let elsewhere = crate::accounts::default_account()
            .filter(|account| crate::server::enabled() && account.shown())
            .and_then(|account| account.origin)
            .filter(|current| current != origin)
            .map(|current| format!("You are signed in to {}.", current.display_host()));
        let browser = pill("link-open-browser", "Open in browser", "globe", t).on_click(
            cx.listener(|this, _, window, cx| {
                this.focus.focus(window);
                this.open_link_in_browser(cx);
            }),
        );
        let mut actions = Vec::new();
        if offered {
            actions.push(
                primary_pill("link-sign-in", "Sign in", t)
                    .on_click(cx.listener(|this, _, _, cx| this.sign_in_for_link(cx)))
                    .into_any_element(),
            );
        }
        actions.push(browser.into_any_element());
        link_prompt(
            "lock",
            format!("This note is on {}", origin.display_host()),
            Some("Sign in to that server to view it.".into()),
            elsewhere,
            actions,
            t,
        )
    }

    fn render_shared_loading(&self, window: &Window) -> AnyElement {
        let t = self.theme();
        div()
            .flex_1()
            .min_h_0()
            .w_full()
            .child(
                div()
                    .w(window.viewport_size().width.min(px(672.)))
                    .mx_auto()
                    .px(px(32.))
                    .pt(px(8.))
                    .child(
                        div()
                            .h(px(28.))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .text_size(px(13.))
                            .text_color(t.muted)
                            .child(crate::motion::spinner(t.muted))
                            .child("Loading shared note…"),
                    ),
            )
            .into_any_element()
    }

    /// A shared note, laid out as the note page lays out a note of one's
    /// own: the same title, the same row of capsules, the same documents in
    /// the same faces. Nothing on it edits, asks or records.
    fn render_shared_note(
        &self,
        note: &SharedNote,
        host: &str,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = self.theme();
        let has_enhanced = !note.enhanced.trim().is_empty();
        let has_body = !note.body.trim().is_empty();
        let document_width =
            window
                .viewport_size()
                .width
                .min(px(if has_enhanced { 682. } else { 672. }));
        let body_width = (document_width - px(64.)).max(px(100.));
        let display_tab = if has_enhanced && (self.links.tab == NoteTab::Enhanced || !has_body) {
            NoteTab::Enhanced
        } else {
            NoteTab::Notes
        };
        let (enhanced, enhanced_whole) = shown(&note.enhanced);
        let (_, body_whole) = shown(&note.body);
        let whole = match display_tab {
            NoteTab::Enhanced => enhanced_whole,
            NoteTab::Notes => body_whole,
        };
        let content = match display_tab {
            NoteTab::Enhanced => {
                let document = crate::model::Note {
                    id: SHARED_NOTE_ID,
                    enhanced: enhanced.to_owned(),
                    ..Default::default()
                };
                crate::motion::stats::copied(document.enhanced.len());
                self.render_note_markdown(&document, true, t, body_width, window, cx)
                    .into_any_element()
            }
            NoteTab::Notes if has_body => div()
                .w(body_width)
                .text_size(px(16.))
                .line_height(px(24.))
                .child(self.links.body.clone())
                .into_any_element(),
            NoteTab::Notes => div()
                .w(body_width)
                .text_size(px(16.))
                .line_height(px(24.))
                .text_color(t.muted)
                .child("The sender shared this note without any text.")
                .into_any_element(),
        };
        let day = note
            .date
            .map(|date| crate::shared::day_label(date, chrono::Local::now().date_naive()));
        let views = (has_enhanced && has_body).then(|| {
            self.note_view_tabs(
                NoteTabs {
                    notes_label: "Notes",
                    // The same 12 px either side of the icon and label as
                    // "My notes" has.
                    notes_width: (note_tab_label_width("Notes", window) + 45.).round(),
                    menu: false,
                },
                display_tab,
                window,
                cx,
                |this, tab, _, window, cx| {
                    this.focus.focus(window);
                    this.links.tab = tab;
                    cx.notify();
                },
            )
        });
        let document = div()
            .flex()
            .flex_col()
            .w(document_width)
            .min_w_0()
            .max_w(px(682.))
            .mx_auto()
            .px(px(32.))
            .pt(px(8.))
            .pb(px(64.))
            .child(
                div()
                    .w(body_width)
                    .font_family(DISPLAY)
                    .text_size(px(24.))
                    .line_height(px(28.))
                    .mb(px(14.))
                    .child(note.display_title()),
            )
            .child(
                div()
                    .h(px(28.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .ml(px(-4.))
                    .mb(px(20.))
                    .when_some(views, |row, views| row.child(views))
                    .when_some(day, |row, day| {
                        row.child(
                            note_meta_chip("shared-note-day", t)
                                .child(icon("calendar-dots", 14., t.muted))
                                .child(day),
                        )
                    })
                    .child(
                        div()
                            .id("shared-note-provenance")
                            .h(px(28.))
                            .min_w_0()
                            .px(px(11.))
                            .rounded_full()
                            .border_1()
                            .border_color(t.hairline)
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_size(px(13.))
                            .text_color(t.muted)
                            .child(icon("lock", 14., t.muted))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .child(format!("Shared · read-only · {host}")),
                            ),
                    ),
            )
            // As on a note: switching documents settles in rather than
            // swapping abruptly.
            .child(crate::motion::crossfade(
                ("shared-tab-content", display_tab as u64),
                div().child(content),
            ))
            .when(!whole, |document| {
                document.child(
                    div()
                        .mt(px(20.))
                        .flex()
                        .flex_col()
                        .items_start()
                        .gap(px(8.))
                        .child(small("This note is longer than the app shows.", t))
                        .child(
                            pill("shared-open-rest", "Open in browser", "globe", t).on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.focus.focus(window);
                                    this.open_link_in_browser(cx);
                                }),
                            ),
                        ),
                )
            });
        div()
            .relative()
            .flex()
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_hidden()
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(
                        div()
                            .id("shared-note-scroll")
                            .track_scroll(&self.note_scroll)
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .child(document),
                    )
                    .child(crate::ui::scroll_thumb("shared-note", &self.note_scroll)),
            )
            .into_any_element()
    }
}

/// A pill as a page's main action: the same capsule, filled with the ink.
fn primary_pill(id: &'static str, label: &'static str, t: Theme) -> Stateful<Div> {
    pill(id, label, "", t).bg(t.text).text_color(t.bg)
}

/// A page that has one thing to say and a few things to do about it, set
/// out as the app's empty pages are: an illustration, a display line, a
/// sentence, a caption and a row of actions.
fn link_prompt(
    symbol: &str,
    title: impl Into<SharedString>,
    detail: Option<String>,
    caption: Option<String>,
    actions: Vec<AnyElement>,
    t: Theme,
) -> AnyElement {
    div()
        .flex_1()
        .min_h_0()
        .w_full()
        .flex()
        .flex_col()
        .items_center()
        .child(
            div()
                .w_full()
                .max_w(px(674.))
                .px(px(24.))
                .mt(px(115.))
                .flex()
                .flex_col()
                .items_center()
                .gap(px(14.))
                .child(crate::screens::empty_illustration(symbol, t))
                .child(
                    div()
                        .mt(px(8.))
                        .max_w(px(520.))
                        .text_center()
                        .font_family(DISPLAY)
                        .text_size(px(24.))
                        .line_height(px(28.))
                        .child(title.into()),
                )
                .when_some(detail, |prompt, detail| {
                    prompt.child(
                        div()
                            .max_w(px(350.))
                            .text_center()
                            .text_size(px(14.))
                            .line_height(px(22.))
                            .text_color(t.muted)
                            .child(detail),
                    )
                })
                .when_some(caption, |prompt, caption| {
                    prompt.child(small(caption, t).text_center())
                })
                .child(
                    div()
                        .mt(px(10.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .children(actions),
                ),
        )
        .into_any_element()
}
