use crate::{
    app::{Cereal, Menu, Screen},
    ui::*,
};
use gpui::{prelude::*, *};

#[derive(Clone, Copy)]
enum SettingToggle {
    DarkMode,
    MenuBarEvents,
    EventsWithoutParticipants,
    Notification(crate::notifications::Kind),
    LiveIndicator,
    LoginItem,
    VoiceIsolation,
}

/// The pages of the dedicated settings mode. Only settings this app really
/// has get a tab: there are no placeholders for features that do not exist.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SettingsTab {
    #[default]
    Preferences,
    Profile,
    Calendar,
    Notifications,
    About,
}

impl SettingsTab {
    pub const ALL: [SettingsTab; 5] = [
        SettingsTab::Preferences,
        SettingsTab::Profile,
        SettingsTab::Calendar,
        SettingsTab::Notifications,
        SettingsTab::About,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SettingsTab::Preferences => "Preferences",
            // An account is one user on one server; this page is about them.
            SettingsTab::Profile => "Accounts",
            SettingsTab::Calendar => "Calendar",
            SettingsTab::Notifications => "Notifications",
            SettingsTab::About => "About",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            SettingsTab::Preferences => "sliders",
            SettingsTab::Profile => "person",
            SettingsTab::Calendar => "calendar",
            SettingsTab::Notifications => "bell",
            SettingsTab::About => "help",
        }
    }
}

impl Cereal {
    pub fn render_screen(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        match self.screen {
            Screen::Chat => self.render_chat_screen(window, cx),
            Screen::SearchResults => self.render_search_results(window, cx),
            Screen::Shared => self.render_shared_screen(window, cx),
            Screen::Settings => self.render_settings_screen(window, cx),
            _ => div().flex_1().into_any_element(),
        }
    }

    fn render_search_results(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let t = self.theme();
        let query = &self.search_results_query;
        let notes = self.search_hits(query);
        // Day captions and hits, built only near the viewport (see `lazy`).
        // A hit's height depends on how its excerpt wraps, so the window's
        // width is part of what the measured heights are valid for.
        enum Entry<'a> {
            Day(&'a str),
            Hit(&'a crate::home::SearchHit),
        }
        let mut entries = Vec::with_capacity(notes.len() + 16);
        let mut last_date = "";
        for hit in notes.iter() {
            if hit.row.date != last_date {
                last_date = &hit.row.date;
                entries.push(Entry::Day(&hit.row.date));
            }
            entries.push(Entry::Hit(hit));
        }
        let keys: Vec<u64> = entries
            .iter()
            .map(|entry| match entry {
                Entry::Day(date) => crate::lazy::key(("day", date)),
                Entry::Hit(hit) => crate::lazy::key(("hit", hit.row.id, &hit.snippet)),
            })
            .collect();
        let scroll = crate::lazy::scroll("search-results");
        let mut list = div().flex().flex_col().w_full().child(crate::lazy::column(
            "search-results",
            &keys,
            f32::from(window.viewport_size().width),
            0.,
            &scroll,
            window,
            |index| match entries[index] {
                Entry::Day(date) => div()
                    .px(px(8.))
                    .pt(px(28.))
                    .pb(px(8.))
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(t.muted)
                    .child(crate::home::date_label(date))
                    .into_any_element(),
                Entry::Hit(hit) => {
                    let (note, snippet) = (&hit.row, &hit.snippet);
                    crate::motion::stats::copied(note.title.len() + snippet.len());
                    let highlights = crate::note::transcript_match_ranges(snippet, query)
                        .into_iter()
                        .map(|range| {
                            (
                                range,
                                HighlightStyle {
                                    background_color: Some(rgba(0xb9853455).into()),
                                    ..Default::default()
                                },
                            )
                        });
                    let id = note.id;
                    div()
                        .relative()
                        .child(self.list_row(id, note.title.clone(), &note.time, cx))
                        .child(
                            div()
                                .id(("search-result-preview", id))
                                .pl(px(52.))
                                .pr(px(8.))
                                // The excerpt belongs to the row above it:
                                // the next result starts a little lower.
                                .pb(px(10.))
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .text_color(t.muted)
                                .cursor_default()
                                .child(StyledText::new(snippet.clone()).with_highlights(highlights))
                                .reach().on_click(
                                    cx.listener(move |this, _, _, cx| this.open_note(id, cx)),
                                ),
                        )
                        .into_any_element()
                }
            },
        ));
        if notes.is_empty() {
            list = list.child(
                div()
                    .py(px(40.))
                    .px(px(8.))
                    .text_color(t.muted)
                    .text_size(px(14.))
                    .child("No matching meetings. Try another search."),
            );
        }
        screen_scroller("search-results-page")
            .track_scroll(&scroll)
            .pr(px(10.))
            .child(
                div()
                    // In pixels: see `page_column_width`.
                    .w(px(self.page_column_width(window)))
                    .flex_shrink_0()
                    .px(px(24.))
                    .pt(px(14.))
                    .pb(px(48.))
                    .child(
                        div()
                            .px(px(8.))
                            .child(heading(format!("Results for \"{query}\""), t)),
                    )
                    .child(div().mt(px(48.)).child(self.render_chat_composer_focused(
                        self.ask.read(cx).focus_handle.is_focused(window),
                        cx,
                    )))
                    .child(div().mt(px(40.)).h(px(1.)).bg(t.border))
                    .child(
                        div().mt(px(40.)).child(
                            div().flex().child(
                                pill("search-owned-notes", "My notes", "lock", t).bg(t.hover),
                            ),
                        ),
                    )
                    .child(list),
            )
            .into_any_element()
    }

    /// "Shared with me": the shared notes opened on this Mac, newest first,
    /// under a field for opening one from a link in hand. The list is this
    /// Mac's own and is the same whoever is signed in; a note on a server
    /// the app is not signed in to opens as a request to sign in there.
    fn render_shared_screen(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let t = self.theme();
        let visits = self.links.history.visits();
        let focused = self.links.paste.read(cx).focus_handle.is_focused(window);
        let typed = !self.links.paste.read(cx).content().trim().is_empty();
        // The field is a row of the list below it: the same height, corner
        // and insets, so its icon and text stand over the rows' tiles and
        // titles.
        let paste = div()
            .mt(px(20.))
            .h(px(44.))
            .px(px(8.))
            .flex()
            .items_center()
            .gap(px(16.))
            .rounded(px(12.))
            .border_1()
            .border_color(if self.links.problem.is_some() {
                t.danger
            } else if focused {
                t.accent
            } else {
                t.border
            })
            .bg(t.card)
            .child(
                div()
                    .size(px(28.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("link-chain", 17., t.muted)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(14.))
                    .line_height(px(20.))
                    .child(self.links.paste.clone()),
            )
            .when(typed, |field| {
                field.child(
                    pill("shared-paste-open", "Open", "", t)
                        .on_click(cx.listener(|this, _, _, cx| this.open_pasted_link(cx))),
                )
            });
        let mut column = screen_column(self.page_column_width(window))
            .child(page_title("Shared with me", t))
            .child(paste)
            .when_some(self.links.problem.clone(), |column, problem| {
                column.child(
                    div()
                        .id("shared-paste-problem")
                        .mt(px(6.))
                        .px(px(8.))
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(t.danger)
                        .child(problem),
                )
            });
        if visits.is_empty() {
            column = column.child(
                div()
                    .mt(px(58.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(14.))
                    .child(empty_illustration("person-outline", t))
                    .child(
                        div()
                            .mt(px(8.))
                            .font_family(DISPLAY)
                            .text_size(px(24.))
                            .child("Nothing shared with you yet"),
                    )
                    .child(
                        div()
                            .max_w(px(350.))
                            .text_center()
                            .text_size(px(14.))
                            .line_height(px(22.))
                            .text_color(t.muted)
                            .child("Shared notes you open are kept here."),
                    ),
            );
        }
        // Home's list: a day caption, then that day's rows.
        let mut last_day = String::new();
        for visit in visits {
            let opened = visit.opened_at.with_timezone(&chrono::Local);
            let day = opened.format("%Y-%m-%d").to_string();
            if day != last_day {
                column = column.child(
                    caption(crate::home::date_label(&day), t)
                        .px(px(8.))
                        .pt(px(25.))
                        .pb(px(2.)),
                );
                last_day = day;
            }
            let key = crate::lazy::key(visit.link.web_url());
            let hovered = crate::motion::reveal_key(("shared-visit", key));
            let (open, remove) = (visit.link.clone(), visit.link.clone());
            column = column.child(
                self.list_row_frame(
                    ("shared-visit", key),
                    hovered,
                    visit.title.clone().into(),
                    visit.link.origin.display_host(),
                    &opened.format("%-I:%M %p").to_string(),
                    tool_tip(("shared-visit-remove", key), "close", "Remove from list", t)
                        .size(px(24.))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.remove_shared_visit(&remove, cx);
                        })),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.open_link(crate::link::AppLink::Shared(open.clone()), cx)
                })),
            );
        }
        screen_scroller("shared-screen")
            .child(column)
            .into_any_element()
    }

    fn render_chat_screen(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let t = self.theme();
        let empty = self.chat_messages.is_empty();
        let mut column = div()
            // A width in pixels, not "as wide as the page, up to 672": with
            // only a maximum the column was first sized with nothing in it
            // wrapped, and kept that height once its paragraphs had wrapped,
            // so the last lines of a long answer (and what follows it) lay
            // beyond the end of the scroll range and could not be reached.
            .w(px(self.chat_column_width(window)))
            // Empty, the page's composer is 628 px wide (22 px in from the
            // 672 px column) and starts 216 px down the window, as the
            // reference measures.
            .px(px(if empty { 22. } else { 24. }))
            .pt(px(if empty { 114. } else { 24. }))
            .pb(px(28.))
            .flex()
            .flex_col()
            .flex_shrink_0();
        if empty {
            let greeting = crate::accounts::default_session()
                .map(|session| {
                    session
                        .user
                        .name
                        .split_whitespace()
                        .next()
                        .unwrap_or_default()
                        .chars()
                        .take(40)
                        .collect::<String>()
                })
                .filter(|name| !name.is_empty())
                .map(|name| format!("Hi {name}, ask anything"))
                .unwrap_or_else(|| "Ask anything".into());
            let recents = self.render_workspace_chat_recents(cx);
            column = column
                .child(
                    div()
                        .pl(px(11.))
                        .pr(px(8.))
                        .mb(px(22.))
                        .font_family(DISPLAY)
                        .text_size(px(26.))
                        .line_height(px(32.))
                        .child(greeting),
                )
                .child(self.render_personal_chat_composer(window, cx))
                .when_some(recents, |column, recents| column.child(recents));
        } else {
            column = column.child(self.render_chat_turns(0, false, window, cx));
            if let Some(queue) = self.render_chat_queue(false, window, cx) {
                column = column.child(queue);
            }
        }
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .items_center()
            .child(
                div()
                    .id("chat-messages")
                    .track_scroll(&self.chat_scroll)
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_y_scroll()
                    .on_scroll_wheel(cx.listener(|_, _, _, cx| cx.notify()))
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(column),
            )
            .when(!empty, |d| {
                d.child(
                    div()
                        .w_full()
                        .max_w(px(672.))
                        .px(px(8.))
                        .pb(px(8.))
                        .when(self.chat_editing.is_some(), |d| {
                            d.child(div().mb(px(8.)).child(self.render_chat_edit_banner(cx)))
                        })
                        .child(self.render_chat_composer_focused(
                            self.ask.read(cx).focus_handle.is_focused(window),
                            cx,
                        )),
                )
            })
            .child(
                div()
                    .pb(px(7.))
                    .text_size(px(10.))
                    .line_height(px(12.))
                    .text_color(t.muted)
                    .child("AI can make mistakes. Check important details."),
            )
            // The way down to the newest answer; an empty page has none.
            .when(!empty && self.chat_can_scroll_down(), |d| {
                d.child(self.render_chat_scroll_down(cx))
            })
            .into_any_element()
    }

    fn render_workspace_chat_recents(&self, cx: &mut Context<Self>) -> Option<Div> {
        let t = self.theme();
        let chats: Vec<_> = self.chat_threads().into_iter().take(3).collect();
        if chats.is_empty() {
            return None;
        }
        let mut section = div().mt(px(54.)).child(
            div()
                .pl(px(8.))
                .pr(px(10.))
                .h(px(16.))
                .flex()
                .items_center()
                .justify_between()
                .text_size(px(13.))
                .line_height(px(16.))
                .text_color(t.muted)
                .child("Recents")
                .child(
                    div()
                        .id("workspace-chat-all-recents")
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .cursor_default()
                        .map(|link| {
                            crate::motion::warms(
                                link,
                                crate::motion::reveal_key("workspace-chat-all-recents"),
                                t.muted,
                                t.text,
                            )
                        })
                        .child("See all")
                        .child(icon("plus", 11., t.muted))
                        .reach().on_click(cx.listener(|this, _, _, cx| {
                            this.chat_history_open = true;
                            cx.notify();
                        })),
                ),
        );
        let now = chrono::Utc::now();
        let mut rows = div().mt(px(11.)).px(px(6.));
        for chat in chats {
            let (archive_id, account) = (chat.id.clone(), chat.account.clone());
            let updated = crate::chat::thread_age(&chat.updated_at, now);
            rows = rows.child(
                div()
                    .id(SharedString::from(format!("workspace-recent-{}", chat.id)))
                    .h(px(40.5))
                    .rounded(px(7.))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .cursor_default()
                    .child(crate::motion::tactile(t.hover).radius(7.))
                    .child(
                        div()
                            .size(px(30.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(t.border)
                            .bg(t.card)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon(
                                if chat.note_id == Some(0) { "chat" } else { "file" },
                                15.,
                                t.muted,
                            )),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(13.5))
                            .truncate()
                            .child(chat.display_title().to_owned()),
                    )
                    .when_some(updated, |row, label| {
                        row.child(
                            div()
                                .pr(px(2.))
                                .text_size(px(12.5))
                                .text_color(t.muted)
                                .child(label),
                        )
                    })
                    .reach().on_click(cx.listener(move |this, _, _, cx| {
                        this.open_chat_thread(&account, &archive_id, true, cx);
                    })),
            );
        }
        section = section.child(rows);
        Some(section)
    }

    fn render_personal_chat_composer(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let t = self.theme();
        let focused = self.ask.read(cx).focus_handle.is_focused(window);
        let active = self.chat_stream.is_some();
        let can_send = !self.ask.read(cx).content().trim().is_empty() && !active;
        div()
            .id("personal-chat-composer")
            .relative()
            .children(self.render_chat_model_picker(true, true, cx))
            .w_full()
            // 82.5 px tall: the field's 22 px line 14 px down, the controls'
            // 32 px row 7 px under it.
            .min_h(px(82.5))
            .pl(px(12.))
            .pr(px(10.))
            .pt(px(14.))
            .pb(px(5.5))
            .rounded(px(18.))
            .border_1()
            .border_color(if focused { t.accent } else { t.border })
            .bg(t.card)
            .shadow_sm()
            .when(focused, |d| {
                d.shadow(vec![BoxShadow {
                    color: t.accent.opacity(0.13),
                    offset: point(px(0.), px(0.)),
                    blur_radius: px(0.),
                    spread_radius: px(3.),
                }])
            })
            .flex()
            .flex_col()
            .gap(px(7.))
            .child(
                div()
                    .id("personal-chat-composer-text")
                    .px(px(6.))
                    .min_h(px(22.))
                    .max_h(px(130.))
                    .overflow_y_scroll()
                    .text_size(px(14.))
                    .line_height(px(20.))
                    .child(self.ask.clone()),
            )
            .child(
                div()
                    .h(px(32.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        tool_tip(
                            "personal-chat-attachment",
                            "paperclip",
                            "Attachments are not available yet",
                            t,
                        )
                        .size(px(24.))
                        .cursor(CursorStyle::Arrow)
                        .opacity(DISABLED),
                    )
                    .child(
                        self.render_chat_model_button("personal-chat-configured-model", cx)
                            .ml(px(2.)),
                    )
                    .child(div().flex_1())
                    .child(if active {
                        round_tool("stop-personal-chat", "stop", "Stop response", t)
                            .size(px(32.))
                            .rounded_full()
                            .bg(t.text)
                            .text_color(t.bg)
                            .on_click(cx.listener(|this, _, _, cx| this.stop_chat_stream(cx)))
                    } else if can_send {
                        round_tool("send-chat", "arrow-up", "Send message · Enter", t)
                            .size(px(32.))
                            .rounded_full()
                            .bg(t.text)
                            .text_color(t.bg)
                            .focusable()
                            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                    this.submit_chat(cx);
                                    cx.stop_propagation();
                                }
                            }))
                            .on_click(cx.listener(|this, _, _, cx| this.submit_chat(cx)))
                    } else {
                        round_tool(
                            "personal-chat-voice-input",
                            "mic",
                            "Voice input is not configured",
                            t,
                        )
                        .size(px(32.))
                        .rounded_full()
                        .bg(t.fill)
                        .cursor(CursorStyle::Arrow)
                        .opacity(DISABLED)
                    }),
            )
            .into_any_element()
    }

    /// The sidebar of settings mode: who is signed in, the settings pages, and
    /// Sign out when there is a session to end.
    pub fn render_settings_sidebar(&self, cx: &mut Context<Self>) -> Div {
        let t = self.theme();
        let glide = self.stage.glide("sidebar");
        let connected = crate::server::enabled();
        let session = crate::accounts::default_session().filter(|_| connected);
        let name = self.workspace_name();
        let detail = match &session {
            Some(session) => session.user.email.clone(),
            None => "Sign in to your server".into(),
        };
        let avatar = div()
            .size(px(32.))
            .rounded_full()
            .bg(t.fill)
            .flex()
            .items_center()
            .justify_center();
        let avatar = match &session {
            Some(session) => avatar
                .font_family(DISPLAY)
                .text_size(px(15.))
                .text_color(t.text)
                .child(
                    session
                        .user
                        .name
                        .chars()
                        .next()
                        .map(|initial| initial.to_uppercase().collect::<String>())
                        .unwrap_or_else(|| "M".into()),
                ),
            None => avatar.child(icon("cereal", 18., t.accent)),
        };
        let mut tabs = div().pt(px(20.)).px(px(8.)).flex().flex_col().gap(px(1.));
        for tab in SettingsTab::ALL {
            tabs = tabs.child(
                div()
                    .id(tab.label())
                    .h(px(28.))
                    .px(px(6.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .rounded(px(8.))
                    .text_color(t.secondary)
                    .cursor_default()
                    .child(row_feedback(t, 8.))
                    .when(self.settings_tab == tab, |row| {
                        row.child(glide.probe(("settings", tab)))
                    })
                    .child(icon(tab.icon(), 16., t.secondary))
                    .child(div().flex_1().min_w_0().truncate().child(tab.label()))
                    .reach().on_click(cx.listener(move |this, _, _, cx| {
                        this.settings_tab = tab;
                        cx.notify();
                    })),
            );
        }
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .pt(px(4.))
                    .px(px(12.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(avatar)
                    .child(
                        div()
                            .mt(px(6.))
                            .max_w_full()
                            .truncate()
                            .font_family(DISPLAY)
                            .text_size(px(16.))
                            .line_height(px(22.))
                            .text_color(t.text)
                            .child(name),
                    )
                    .child(
                        div()
                            .mt(px(2.))
                            .max_w_full()
                            .truncate()
                            .child(small(detail, t)),
                    ),
            )
            .child(tabs)
            .child(div().flex_1())
            .when(session.is_some(), |sidebar| {
                sidebar.child(
                    div().px(px(8.)).pb(px(8.)).child(
                        div()
                            .id("settings-sign-out")
                            .h(px(28.))
                            .px(px(6.))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .rounded(px(8.))
                            .text_color(t.danger)
                            .cursor_default()
                            .child(row_feedback(t, 8.))
                            .child(icon("share", 16., t.danger))
                            .child("Sign out")
                            .reach().on_click(cx.listener(|this, _, _, cx| this.ask_sign_out_default(cx))),
                    ),
                )
            })
    }

    fn render_settings_screen(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let t = self.theme();
        let connected = crate::server::enabled();
        let tab = self.settings_tab;
        let meetings = crate::desktop_preferences::meeting();
        let mut column = screen_column(self.page_column_width(window)).child(page_title(tab.label(), t));
        match tab {
            SettingsTab::Preferences => {
                // 1 enabled, 2 waiting for approval in System Settings.
                let login = self.menu_bar.login_item;
                column = column
                    .child(section_label("General", t))
                    .child(
                        settings_card(t)
                            .child(self.setting_toggle(
                                "setting-live-indicator",
                                "mic",
                                "Live meeting indicator",
                                "Floats at the edge of your screen while Cereal records in the background.",
                                meetings.live_indicator,
                                SettingToggle::LiveIndicator,
                                cx,
                            ))
                            .child(self.setting_toggle(
                                "setting-login-item",
                                "power",
                                "Open Cereal when you log in",
                                match login {
                                    2 => "Waiting for your approval in System Settings › General › Login Items.",
                                    -1 => "Available in the installed Cereal app.",
                                    _ => "Cereal starts when you log in to this Mac.",
                                },
                                matches!(login, 1 | 2),
                                SettingToggle::LoginItem,
                                cx,
                            )),
                    )
                    .child(section_label("Appearance", t))
                    .child(settings_card(t).child(self.setting_toggle(
                        "setting-dark-mode",
                        "sun",
                        "Dark mode",
                        "A softer look for late-night notes.",
                        self.workspace.settings.dark_mode,
                        SettingToggle::DarkMode,
                        cx,
                    )))
                    .child(section_label("App icon", t))
                    .child(settings_card(t).child(div().p(px(16.)).flex().flex_col().gap(px(12.))
                        .child(small("Choose a Cereal Dock icon. Finder uses the packaged default icon.", t))
                        .child(div().flex().gap(px(12.)).children(crate::desktop_preferences::AppIcon::ALL.into_iter().map(|choice| self.app_icon_choice(choice, cx))))
                        .when_some(crate::desktop_preferences::preference_error(), |d, error| d.child(small(error, t)))
                        .when(!crate::desktop_preferences::icon_supported(), |d| d.child(small("Dock icon selection is available on macOS.", t)))
                    ))
                    .child(section_label("Notes & meetings", t))
                    .child(
                        settings_card(t)
                            .child(
                                settings_row(
                                    "mic",
                                    "Recording devices",
                                    "Inputs and outputs are chosen in macOS Sound settings.",
                                    t,
                                )
                                .child(
                                    pill("settings-open-sound", "Sound settings", "", t).on_click(
                                        cx.listener(|_, _, _, cx| {
                                            cx.open_url(
                                                crate::desktop_preferences::SOUND_SETTINGS_URL,
                                            )
                                        }),
                                    ),
                                ),
                            )
                            .child(self.setting_toggle(
                                "setting-voice-isolation",
                                "sparkles",
                                "Voice isolation",
                                "Filters background noise from your mic for clearer transcripts. Applies when recording next starts or resumes; meeting audio is never changed.",
                                meetings.voice_isolation,
                                SettingToggle::VoiceIsolation,
                                cx,
                            )),
                    );
            }
            SettingsTab::Profile => {
                let session = crate::accounts::default_session().filter(|_| connected);
                let (title, detail) = match &session {
                    Some(session) => (
                        session.user.name.clone(),
                        format!(
                            "{} · {}",
                            session.user.email,
                            match session.user.role {
                                meeting_notes_api_types::UserDtoRole::Owner => "Owner",
                                meeting_notes_api_types::UserDtoRole::Admin => "Admin",
                                meeting_notes_api_types::UserDtoRole::User => "User",
                            }
                        ),
                    ),
                    None => (
                        "Your account".to_string(),
                        "Sign in to your server.".to_string(),
                    ),
                };
                column = column.child(section_label("Account", t)).child(
                    settings_card(t)
                        .child(
                            settings_row("cereal", title, detail, t).when(session.is_some(), |row| {
                                row.child(
                                    div()
                                        .px(px(9.))
                                        .py(px(4.))
                                        .rounded_full()
                                        .bg(t.fill)
                                        .text_size(px(11.))
                                        .text_color(t.muted)
                                        .child("CONNECTED"),
                                )
                            }),
                        )
                        .child(
                            settings_row(
                                "person",
                                if session.is_some() {
                                    "Server account"
                                } else {
                                    "Sign in to a server"
                                },
                                "Your server address and session are managed on the account page.",
                                t,
                            )
                            .child(
                                pill(
                                    "settings-open-account",
                                    if session.is_some() {
                                        "Manage"
                                    } else {
                                        "Sign in"
                                    },
                                    "",
                                    t,
                                )
                                .on_click(
                                    cx.listener(|this, _, _, cx| {
                                        this.navigate(Screen::Account, cx)
                                    }),
                                ),
                            ),
                        )
                        // Erasing is its own, explicit action; signing out
                        // keeps everything on this Mac.
                        .when_some(
                            crate::accounts::default_account().filter(|_| connected),
                            |card, account| {
                                card.child(
                                    settings_row(
                                        "trash",
                                        "Remove account and erase its data",
                                        format!(
                                            "Signs out of {} and deletes what is saved for it on this Mac. The notes on the server are not affected.",
                                            account.host()
                                        ),
                                        t,
                                    )
                                    .child(
                                        pill("settings-remove-account", "Remove…", "", t).on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                this.ask_erase_account(account.id.clone(), cx)
                                            }),
                                        ),
                                    ),
                                )
                            },
                        ),
                );
                column = self.saved_data_sections(column, cx);
            }
            SettingsTab::Calendar => {
                column = column
                    .child(section_label("Display", t))
                    .child(
                        settings_card(t)
                            .child(self.setting_toggle(
                                "setting-menu-bar-events",
                                "menu-bar",
                                "Show upcoming meetings in menu bar",
                                "Your next meeting and the time until it starts, in the macOS menu bar.",
                                meetings.menu_bar_events,
                                SettingToggle::MenuBarEvents,
                                cx,
                            ))
                            .child(self.setting_toggle(
                                "setting-events-without-participants",
                                "person-outline",
                                "Show events with no participants",
                                "Coming up and the menu bar include events with nobody else invited and no call link.",
                                meetings.events_without_participants,
                                SettingToggle::EventsWithoutParticipants,
                                cx,
                            )),
                    );
                if let Some(calendars) = self.visible_calendars(cx) {
                    column = column.child(calendars);
                }
                column = column
                    .child(section_label("Connections", t))
                    .child(settings_card(t).child(self.calendar_connections_row(cx)));
            }
            SettingsTab::Notifications => {
                use crate::notifications::Kind;
                let toggle = |kind: Kind, cx: &mut Context<Self>| {
                    let (id, symbol, label, description) = kind.row();
                    self.setting_toggle(
                        id,
                        symbol,
                        label,
                        description,
                        self.notification_switch_on(kind, &meetings),
                        SettingToggle::Notification(kind),
                        cx,
                    )
                };
                let meeting = toggle(Kind::Meeting, cx);
                let notes = toggle(Kind::Notes, cx);
                let recording = toggle(Kind::Recording, cx);
                let account = toggle(Kind::Account, cx);
                column = column
                    .when_some(self.notification_permission_row(cx), |c, row| {
                        c.child(section_label("macOS", t)).child(settings_card(t).child(row))
                    })
                    .child(section_label("Meeting notifications", t))
                    .child(settings_card(t).child(meeting))
                    .child(section_label("While you are in another app", t))
                    .child(settings_card(t).child(notes).child(recording).child(account));
            }
            SettingsTab::About => {
                column = column
                    .child(section_label("This app", t))
                    .child(
                        settings_card(t)
                            .child(self.render_version_row(cx))
                            .child(
                                settings_row(
                                    "lock",
                                    "Storage",
                                    "Notes, transcripts and shared links belong to your account on your server. Audio is captured on this Mac and securely delivered for transcription when you record.",
                                    t,
                                )
                                .child(small("Your server", t)),
                            )
                            .child(
                                settings_row(
                                    "keyboard",
                                    "Keyboard shortcuts",
                                    "Make yourself at home.",
                                    t,
                                )
                                .child(
                                    pill("settings-shortcuts", "View", "", t).on_click(
                                        cx.listener(|this, _, _, cx| {
                                            this.open_menu(Menu::Shortcuts);
                                            cx.notify();
                                        }),
                                    ),
                                ),
                            )
                            .child(
                                settings_row(
                                    "sparkles",
                                    "Welcome",
                                    "Go through setting up Cereal on this Mac once more.",
                                    t,
                                )
                                .child(
                                    pill("settings-show-welcome", "Show welcome again", "", t)
                                        .flex_shrink_0()
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.show_welcome_again(cx)
                                        })),
                                ),
                            ),
                    )
                    .child(div().mt(px(20.)).px(px(8.)).child(small("Your notes stay yours.", t)));
            }
        }
        // Switching tabs settles the new page in; the key is the tab alone.
        screen_scroller("settings-screen")
            .child(crate::motion::page(
                ("settings-tab", tab as u64),
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(column),
            ))
            .into_any_element()
    }

    /// What macOS allows, when it is not simply "allowed": a way to allow
    /// notifications, that macOS is still to answer, or where they were
    /// turned off.
    fn notification_permission_row(&self, cx: &mut Context<Self>) -> Option<Div> {
        use crate::notifications::Standing;
        let t = self.theme();
        let settings = |id: &'static str| {
            pill(id, "Open System Settings", "", t)
                .flex_shrink_0()
                .on_click(cx.listener(|this, _, _, _| this.open_notification_settings()))
        };
        Some(match self.notification_standing() {
            Standing::Allowed => return None,
            Standing::NotAsked => settings_row(
                "bell",
                "Allow notifications",
                "macOS asks once. Cereal uses notifications for meetings, notes and recordings, as chosen below.",
                t,
            )
            .child(
                pill("settings-allow-notifications", "Allow…", "", t)
                    .flex_shrink_0()
                    .on_click(cx.listener(|this, _, _, cx| this.allow_notifications(cx))),
            ),
            Standing::Waiting => settings_row(
                "bell",
                "Waiting for permission",
                "Choose Allow in the notification macOS shows for Cereal. The switches below start working once it is allowed.",
                t,
            )
            .child(settings("settings-waiting-notification-settings")),
            Standing::Refused => settings_row(
                "warning",
                "Notifications are off for Cereal",
                "Turn on Allow notifications for Cereal in System Settings to get the notifications below.",
                t,
            )
            .child(settings("settings-open-notification-settings")),
            Standing::Unavailable => settings_row(
                "warning",
                "Notifications need the packaged app",
                "macOS delivers notifications only to Cereal.app, not to a bare executable.",
                t,
            ),
        })
    }

    /// Calendar management lives on the server; this row links to it.
    fn calendar_connections_row(&self, cx: &mut Context<Self>) -> Div {
        let t = self.theme();
        let url = crate::server::calendar_connections_url();
        settings_row(
            "calendar",
            "Calendars",
            "Connect multiple Google accounts or calendar feeds and choose which calendars appear in upcoming meetings.",
            t,
        )
        .map(|row| match url {
            Some(url) => row.child(
                pill("connect-calendar", "Manage calendars", "", t)
                    .on_click(cx.listener(move |_, _, _, cx| cx.open_url(&url))),
            ),
            None => row.child(small("Sign in to manage", t)),
        })
    }

    fn app_icon_choice(
        &self,
        choice: crate::desktop_preferences::AppIcon,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let t = self.theme();
        let selected = crate::desktop_preferences::selected_icon() == choice;
        div()
            .id(SharedString::from(format!(
                "app-icon-{}",
                choice.name().to_lowercase()
            )))
            .w(px(92.))
            .p(px(9.))
            .rounded(px(12.))
            .border_1()
            .border_color(if selected { t.accent } else { t.border })
            .flex()
            .flex_col()
            .items_center()
            .gap(px(7.))
            .when(crate::desktop_preferences::icon_supported(), |d| {
                d.child(crate::motion::tactile(t.hover).radius(12.))
            })
            .child(img(SharedString::from(choice.asset())).size(px(58.)))
            .child(div().text_size(px(12.)).child(choice.name()))
            .when(selected, |d| d.child(icon("check", 12., t.accent)))
            .when(crate::desktop_preferences::icon_supported(), |d| {
                d.cursor_default()
                    .reach().on_click(cx.listener(move |this, _, _, cx| {
                        match crate::desktop_preferences::select_icon(choice) {
                            Ok(()) => {
                                this.show_toast(&format!("Dock icon set to {}", choice.name()), cx)
                            }
                            Err(error) => this.show_toast(&error, cx),
                        }
                        cx.notify();
                    }))
            })
    }

    fn setting_toggle(
        &self,
        id: &'static str,
        symbol: &'static str,
        label: &'static str,
        description: &'static str,
        enabled: bool,
        setting: SettingToggle,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let t = self.theme();
        settings_row(symbol, label, description, t)
            .id(id)
            .child(self.switch(id, enabled))
            .reach().on_click(cx.listener(move |this, _, _, cx| {
                match setting {
                    SettingToggle::DarkMode => {
                        this.workspace.settings.dark_mode = !this.workspace.settings.dark_mode;
                        this.workspace.settings_changed();
                    }
                    SettingToggle::MenuBarEvents => this.set_menu_bar_events(!enabled, cx),
                    SettingToggle::EventsWithoutParticipants => {
                        this.set_meeting_preference(
                            |preferences| preferences.events_without_participants = !enabled,
                            cx,
                        );
                    }
                    SettingToggle::LiveIndicator => {
                        this.set_meeting_preference(
                            |preferences| preferences.live_indicator = !enabled,
                            cx,
                        );
                        crate::pill::refresh(cx);
                    }
                    SettingToggle::Notification(kind) => this.set_notification(kind, !enabled, cx),
                    SettingToggle::LoginItem => this.set_login_item(!enabled, cx),
                    SettingToggle::VoiceIsolation => {
                        this.set_meeting_preference(
                            |preferences| preferences.voice_isolation = !enabled,
                            cx,
                        );
                        crate::audio::prepare_voice_isolation();
                    }
                }
                this.persist(cx);
            }))
    }

    /// The switch at the end of a setting's row, the same design as a
    /// calendar's ([`Self::row_switch`]): a 32 by 20 track, grey when off,
    /// and a 16 pt knob 2 pt inside it that travels 12 pt.
    fn switch(&self, id: &'static str, enabled: bool) -> Div {
        let t = self.theme();
        // The knob is a droplet: its leading edge runs ahead and the
        // trailing edge catches up, while the track's tint follows.
        let knob = self.stage.glide(id);
        let level = self
            .stage
            .aim(
                (id, 0),
                if enabled { 1. } else { 0. },
                crate::motion::SNAPPY,
            )
            .clamp(0., 1.);
        let slot = |on: bool| {
            div()
                .size(px(16.))
                .rounded_full()
                .when(on == enabled, |slot| slot.child(knob.probe(on)))
        };
        div()
            .relative()
            .flex_shrink_0()
            .mt(px(4.))
            .w(px(32.))
            .h(px(20.))
            .p(px(2.))
            .rounded_full()
            .bg(self.switch_off())
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded_full()
                    .bg(t.accent.opacity(level)),
            )
            .child(knob.indicator(rgb(0xfcfcf8).into(), 8.))
            .flex()
            .items_center()
            .justify_between()
            .child(slot(false))
            .child(slot(true))
    }

    /// The switch on a calendar's row, as in the reference: a 32 by 20
    /// track, grey when off, and a 16 pt knob 2 pt inside it that travels
    /// 12 pt. Its knob slides without a droplet of its own.
    fn row_switch(&self, key: u64, enabled: bool) -> Div {
        let t = self.theme();
        let level = self
            .stage
            .aim(
                ("calendar-switch", key),
                if enabled { 1. } else { 0. },
                crate::motion::SNAPPY,
            )
            .clamp(0., 1.);
        div()
            .relative()
            .flex_shrink_0()
            .w(px(32.))
            .h(px(20.))
            .rounded_full()
            .bg(self.switch_off())
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded_full()
                    .bg(t.accent.opacity(level)),
            )
            .child(
                div()
                    .absolute()
                    .top(px(2.))
                    .left(px(2. + 12. * level))
                    .size(px(16.))
                    .rounded_full()
                    .bg(rgb(0xfcfcf8)),
            )
    }

    /// A switch's track when it is off: the reference's grey.
    fn switch_off(&self) -> Hsla {
        if self.workspace.settings.dark_mode {
            rgba(0xffffff40).into()
        } else {
            rgb(0xacada8).into()
        }
    }

    fn set_meeting_preference(
        &mut self,
        change: impl FnOnce(&mut crate::desktop_preferences::MeetingPreferences),
        cx: &mut Context<Self>,
    ) {
        if let Err(error) = crate::desktop_preferences::update_meeting(change) {
            self.show_toast(&error, cx);
        }
        cx.notify();
    }

    /// "Visible calendars": every calendar this account's events come from,
    /// each with a switch, under a heading with Reset. As in the reference:
    /// one card of compact rows (a bare colour square, the name, a switch)
    /// with dashed lines between them, the person's own calendar first, and
    /// a line at the end about connecting other calendars. Absent until
    /// there is a calendar.
    fn visible_calendars(&self, cx: &mut Context<Self>) -> Option<Div> {
        let t = self.theme();
        let hidden = crate::desktop_preferences::meeting().hidden_calendars;
        // (id, name, colour, shown)
        let personal = |primary: bool| {
            self.calendars
                .personal
                .iter()
                .filter(move |calendar| calendar.primary == primary)
                .map(|calendar| {
                    (
                        calendar.id.clone(),
                        calendar.name.clone(),
                        calendar.color.clone(),
                        calendar.selected,
                    )
                })
        };
        let mut rows: Vec<(String, String, String, bool)> = personal(true).collect();
        rows.extend(personal(false));
        rows.extend(
            self.calendars
                .sources
                .iter()
                .filter(|source| source.enabled)
                .map(|source| {
                    (
                        source.id.clone(),
                        source.name.clone(),
                        source.color.clone(),
                        !hidden.contains(&source.id),
                    )
                }),
        );
        if rows.is_empty() {
            return None;
        }
        let busy = self.calendars.choosing.is_some();
        let rule = || {
            div()
                .flex_shrink_0()
                .px(px(16.))
                .h(px(hairline(0.5)))
                .child(crate::calendar::dashed_rule(t.hairline).size_full())
        };
        // The line about other calendars links to the account's calendar
        // connections, so it shows only when there is an account to open.
        let connections = crate::server::calendar_connections_url();
        let count = rows.len();
        let mut card = settings_card(t);
        for (index, (id, name, color, shown)) in rows.into_iter().enumerate() {
            let key = crate::lazy::key(&id);
            card = card
                .child(
                    div()
                        .id(("visible-calendar", key))
                        .relative()
                        .h(px(49.4))
                        .flex_shrink_0()
                        .px(px(16.))
                        .flex()
                        .items_center()
                        .gap(px(16.))
                        .child(
                            div()
                                .size(px(12.))
                                .flex_shrink_0()
                                .rounded(px(3.))
                                .bg(crate::calendar::calendar_color(&color)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(px(13.))
                                .line_height(px(18.))
                                .text_color(t.text)
                                .truncate()
                                .child(name),
                        )
                        .child(self.row_switch(key, shown))
                        .when(busy, |row| row.opacity(0.6))
                        .reach()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.set_calendar_visible(id.clone(), !shown, cx)
                        })),
                )
                .when(index + 1 < count || connections.is_some(), |card| card.child(rule()));
        }
        card = card.when_some(connections, |card, url| card.child(
            div()
                .h(px(48.5))
                .px(px(16.))
                .flex()
                .items_center()
                .text_size(px(13.))
                .line_height(px(18.))
                .text_color(t.muted)
                .child("Don't see the calendar you want?\u{a0}")
                .child(
                    div()
                        .id("visible-calendars-connect")
                        .underline()
                        .child("See how to connect other calendars")
                        .reach()
                        .on_click(move |_, _, cx| cx.open_url(&url)),
                ),
        ));
        Some(
            div()
                .w_full()
                .child(
                    div()
                        .flex()
                        .items_end()
                        .justify_between()
                        .child(section_label("Visible calendars", t))
                        .child(
                            div()
                                .id("visible-calendars-reset")
                                .mb(px(6.))
                                .pr(px(17.))
                                .text_size(px(13.))
                                .line_height(px(18.))
                                .text_color(t.accent)
                                .child("Reset")
                                .reach()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.reset_visible_calendars(cx)
                                })),
                        ),
                )
                .child(card),
        )
    }
}

/// What a note in Trash says when its own server no longer has it.
pub const DELETED_ON_SERVER: &str = "Deleted on the server";

/// A sheet about an account. What it says is decided here, in one place,
/// from what is really on this Mac.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AccountSheet {
    /// Sign out: the session ends; everything on this Mac stays.
    SignOut {
        account: crate::accounts::AccountId,
        host: String,
        counts: crate::accounts::saved::Counts,
    },
    /// Remove an account and erase its data from this Mac.
    Erase {
        account: crate::accounts::AccountId,
        host: String,
        counts: crate::accounts::saved::Counts,
    },
    /// Erase data that no signed-in account owns: a signed-out account's
    /// (`host` from its own file) or older saved data (`host` is `None`).
    EraseSaved {
        id: crate::accounts::AccountId,
        host: Option<String>,
        counts: crate::accounts::saved::Counts,
        bytes: u64,
    },
    /// Sign in to an account that was signed out of. Its address was read
    /// from a plain file on this Mac, so it is shown in full beside the
    /// hosts already signed in to, and compared with them.
    SignIn {
        url: String,
        hosts: crate::accounts::skeleton::HostSheet,
    },
    /// Restore a note its server deleted: it is created there again.
    Restore { note: u64, host: String },
}

/// How a sheet's button looks and what it means.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SheetAction {
    Cancel,
    /// Leads to the erase sheet instead of signing out.
    EraseInstead,
    SignOut,
    Erase,
    Continue,
    Restore,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SheetText {
    pub title: String,
    pub body: String,
    /// In the order they are drawn. The default (what Return does) is
    /// `default`.
    pub buttons: Vec<(String, SheetAction)>,
    pub default: SheetAction,
    /// A sentence drawn in the warning style above the buttons.
    pub warning: Option<String>,
}

impl AccountSheet {
    pub fn text(&self) -> SheetText {
        let cancel = ("Cancel".to_owned(), SheetAction::Cancel);
        match self {
            Self::SignOut { host, counts, .. } => SheetText {
                title: format!("Sign out of {host}?"),
                body: format!(
                    "{} stay on this Mac, and are here again when you sign back in.",
                    counts.stays()
                ),
                buttons: vec![
                    cancel,
                    ("Remove and erase instead…".into(), SheetAction::EraseInstead),
                    ("Sign out".into(), SheetAction::SignOut),
                ],
                default: SheetAction::SignOut,
                warning: None,
            },
            Self::Erase { host, counts, .. } => SheetText {
                title: format!("Remove {host} and erase its data from this Mac?"),
                body: format!(
                    "{}. The notes on {host} are not affected.",
                    counts.goes()
                ),
                buttons: vec![cancel, (counts.erase_label(), SheetAction::Erase)],
                default: SheetAction::Cancel,
                warning: never_uploaded(counts),
            },
            Self::EraseSaved {
                host,
                counts,
                bytes,
                ..
            } => SheetText {
                title: match host {
                    Some(host) => format!("Erase what is saved on this Mac for {host}?"),
                    None => "Erase this older saved data?".into(),
                },
                body: format!(
                    "{} ({}). {}",
                    counts.goes(),
                    crate::accounts::saved::size_label(*bytes),
                    match host {
                        Some(host) => format!("The notes on {host} are not affected."),
                        None => "Notes on a server are not affected.".into(),
                    }
                ),
                buttons: vec![cancel, (counts.erase_label(), SheetAction::Erase)],
                default: SheetAction::Cancel,
                warning: never_uploaded(counts),
            },
            Self::SignIn { hosts, .. } => {
                let mut body = format!(
                    "This address was saved on this Mac. Check that it is the server you mean: {}",
                    hosts.host
                );
                if !hosts.existing.is_empty() {
                    body.push_str(&format!(
                        "\n\nYou are signed in to: {}",
                        hosts.existing.join(", ")
                    ));
                }
                SheetText {
                    title: format!("Sign in to {}?", hosts.host),
                    body,
                    buttons: vec![cancel, ("Continue".into(), SheetAction::Continue)],
                    // A look-alike of a server already in use: Cancel is
                    // what Return does.
                    default: if hosts.default_is_cancel() {
                        SheetAction::Cancel
                    } else {
                        SheetAction::Continue
                    },
                    warning: hosts.resembles.as_ref().map(|other| {
                        format!("{} looks like {other}, but it is a different server.", hosts.host)
                    }),
                }
            }
            Self::Restore { host, .. } => SheetText {
                title: "Restore this note?".into(),
                body: format!(
                    "It was deleted on {host}. Restoring creates it there again; its recording on the server is gone."
                ),
                buttons: vec![cancel, ("Restore".into(), SheetAction::Restore)],
                default: SheetAction::Restore,
                warning: None,
            },
        }
    }
}

fn never_uploaded(counts: &crate::accounts::saved::Counts) -> Option<String> {
    match counts.never_uploaded {
        0 => None,
        1 => Some("1 recording is not on the server yet: it exists only on this Mac.".into()),
        n => Some(format!("{n} recordings are not on the server yet: they exist only on this Mac.")),
    }
}

impl Cereal {
    /// What an account's notes hold on this Mac, for the sheets.
    fn account_counts(&self, account: &crate::accounts::Account) -> crate::accounts::saved::Counts {
        let chats = self
            .chat_archives
            .iter()
            .filter(|chat| chat.account.as_ref() == Some(&account.id))
            .count();
        crate::accounts::saved::Counts::of_notes(self.workspace.notes_of(account.slot), chats)
    }

    fn open_account_sheet(&mut self, sheet: AccountSheet, cx: &mut Context<Self>) {
        self.account_sheet = Some(sheet);
        self.menu = Some(Menu::Account);
        cx.notify();
    }

    /// "Sign out", wherever it was chosen: the sheet that says what stays.
    pub fn ask_sign_out_default(&mut self, cx: &mut Context<Self>) {
        let Some(account) = crate::accounts::default_account() else {
            return;
        };
        self.ask_sign_out(account.id, cx);
    }

    pub fn ask_sign_out(&mut self, id: crate::accounts::AccountId, cx: &mut Context<Self>) {
        let Some(account) = crate::accounts::get(&id) else {
            return;
        };
        if let Some(blocked) = self.account_blocked(&account) {
            self.show_toast(blocked.message(), cx);
            return;
        }
        let counts = self.account_counts(&account);
        self.open_account_sheet(
            AccountSheet::SignOut {
                host: account.host(),
                account: id,
                counts,
            },
            cx,
        );
    }

    pub fn ask_erase_account(&mut self, id: crate::accounts::AccountId, cx: &mut Context<Self>) {
        let Some(account) = crate::accounts::get(&id) else {
            return;
        };
        if let Some(blocked) = self.account_blocked(&account) {
            self.show_toast(blocked.message(), cx);
            return;
        }
        let counts = self.account_counts(&account);
        self.open_account_sheet(
            AccountSheet::Erase {
                host: account.host(),
                account: id,
                counts,
            },
            cx,
        );
    }

    /// Restore a note from Trash. One its server deleted asks first: it
    /// becomes a note of this Mac that is created there again.
    pub fn restore_from_trash(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(note) = self.workspace.note(id) else {
            return;
        };
        if note.server_missing_since.is_some() {
            let host = note
                .server_url
                .as_deref()
                .map(crate::accounts::host_of)
                .unwrap_or_else(|| "the server".into());
            self.open_account_sheet(AccountSheet::Restore { note: id, host }, cx);
            return;
        }
        if let Some(note) = self.workspace.note_mut(id) {
            note.trashed = false;
        }
        if self.persist(cx) {
            self.show_toast("Note restored", cx);
        }
    }

    /// The restore of a "Deleted on the server" note was confirmed: its
    /// server id, address, mark and sidecar go, so the next save creates it
    /// on the server again.
    pub(crate) fn restore_deleted_note(&mut self, id: u64, cx: &mut Context<Self>) {
        let recordings = crate::accounts::handle::slot_of(id)
            .and_then(|slot| self.workspace.slots.get(&slot))
            .map(|slot| slot.recordings.clone());
        let Some(note) = self.workspace.note_mut(id) else {
            return;
        };
        if let Some(url) = crate::server::detach_from_server(note)
            && let Some(recordings) = recordings
        {
            let _ = std::fs::remove_file(crate::server::association_file(&recordings, &url, id));
        }
        self.persist(cx);
        self.show_toast("Note restored", cx);
        self.server_action(id, crate::server::Action::Save, cx);
    }

    /// Run what a sheet's button means.
    fn account_sheet_chosen(&mut self, action: SheetAction, window: &mut Window, cx: &mut Context<Self>) {
        let sheet = self.account_sheet.take();
        self.menu = None;
        self.focus.focus(window);
        match (sheet, action) {
            (Some(AccountSheet::SignOut { account, .. }), SheetAction::SignOut) => {
                self.sign_out_account(account, cx)
            }
            (Some(AccountSheet::SignOut { account, .. }), SheetAction::EraseInstead) => {
                self.ask_erase_account(account, cx)
            }
            (Some(AccountSheet::Erase { account, .. }), SheetAction::Erase) => {
                self.erase_account(account, cx)
            }
            (Some(AccountSheet::EraseSaved { id, .. }), SheetAction::Erase) => {
                self.erase_saved_data(id, cx)
            }
            (Some(AccountSheet::SignIn { url, .. }), SheetAction::Continue) => {
                self.account
                    .update(cx, |form, cx| form.sign_in_to(url, cx));
                self.navigate(Screen::Account, cx);
                // Local mode has no gate to the form: go there directly.
                self.screen = Screen::Account;
            }
            (Some(AccountSheet::Restore { note, .. }), SheetAction::Restore) => {
                self.restore_deleted_note(note, cx)
            }
            _ => {}
        }
        cx.notify();
    }

    /// The buttons of an account sheet, in the dialog's own button row.
    pub(crate) fn account_sheet_actions(&self, sheet: &SheetText, cx: &mut Context<Self>) -> Div {
        let t = self.theme();
        let mut row = div()
            .mt(px(8.))
            .flex()
            .flex_wrap()
            .items_center()
            .justify_end()
            .gap(px(8.));
        // The default is the filled button. A sheet that erases draws Erase
        // filled in the danger colour, and Return does not erase.
        let erases = sheet.buttons.iter().any(|(_, action)| *action == SheetAction::Erase);
        for (index, (label, action)) in sheet.buttons.iter().enumerate() {
            let action = *action;
            let filled = if erases {
                action == SheetAction::Erase
            } else {
                action == sheet.default
            };
            let button = if filled {
                // The dialog's filled button.
                div()
                    .id(("account-sheet-action", index))
                    .h(px(28.))
                    .px(px(14.))
                    .rounded_full()
                    .flex()
                    .items_center()
                    .text_size(px(13.))
                    .bg(if erases { t.danger } else { t.text })
                    .text_color(t.bg)
                    .cursor_default()
                    .child(label.clone())
            } else {
                pill(("account-sheet-action", index), label.clone(), "", t).h(px(28.))
            };
            row = row.child(
                crate::motion::dims(
                    button,
                    crate::motion::reveal_key(("account-sheet-action", index)),
                    0.14,
                )
                .reach()
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.account_sheet_chosen(action, window, cx)
                })),
            );
        }
        let Some(warning) = &sheet.warning else {
            return row;
        };
        // As under the address field when an address would travel in the
        // clear (`account.rs`).
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(8.))
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(t.warning_ink)
                    .child(icon("warning", 16., t.warning_ink))
                    .child(div().flex_1().min_w_0().child(warning.clone())),
            )
            .child(row)
    }

    /// Read what is on this Mac of accounts that are not signed in, off the
    /// UI thread (sizes are measured by walking the directories).
    pub fn refresh_saved_data(&mut self, cx: &mut Context<Self>) {
        if self.saved_data_loading || crate::audio::preview_mode() {
            return;
        }
        let Some(paths) = crate::accounts::Paths::of_environment() else {
            return;
        };
        let live: Vec<_> = crate::accounts::all()
            .into_iter()
            .map(|account| account.id)
            .collect();
        self.saved_data_loading = true;
        cx.spawn(async move |this, cx| {
            let listing = cx
                .background_executor()
                .spawn(async move { crate::accounts::saved::listing(&paths, &live) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.saved_data_loading = false;
                this.saved_data = Some(listing);
                cx.notify();
            });
        })
        .detach();
    }

    fn ask_erase_saved(&mut self, id: crate::accounts::AccountId, cx: &mut Context<Self>) {
        let Some(saved) = self.saved_data.as_ref().and_then(|listing| {
            listing
                .signed_out
                .iter()
                .chain(&listing.older)
                .find(|saved| saved.id == id)
        }) else {
            return;
        };
        let sheet = AccountSheet::EraseSaved {
            host: saved.host(),
            counts: saved.counts,
            bytes: saved.bytes,
            id,
        };
        self.open_account_sheet(sheet, cx);
    }

    fn erase_saved_data(&mut self, id: crate::accounts::AccountId, cx: &mut Context<Self>) {
        let Some(paths) = crate::accounts::Paths::of_environment() else {
            return;
        };
        let live: Vec<_> = crate::accounts::all()
            .into_iter()
            .map(|account| account.id)
            .collect();
        if crate::accounts::saved::erase(&paths, &id, &live) {
            let _ = crate::desktop_preferences::update_meeting(|preferences| {
                crate::desktop_preferences::forget_account(preferences, &id)
            });
            self.show_toast("Erased from this Mac", cx);
        } else {
            self.show_toast("That data could not be erased", cx);
        }
        self.saved_data = None;
        self.refresh_saved_data(cx);
    }

    /// For `CEREAL_DEBUG_VIEW`: open one of the sheets about saved data.
    #[cfg(debug_assertions)]
    pub(crate) fn debug_saved_sheet(
        &mut self,
        view: &str,
        listing: &crate::accounts::saved::Listing,
        cx: &mut Context<Self>,
    ) {
        match view {
            "erase-saved" => {
                if let Some(saved) = listing.older.first().or(listing.signed_out.first()) {
                    self.ask_erase_saved(saved.id.clone(), cx);
                }
            }
            _ => {
                let wanted = if view == "sign-in-warning" { 1 } else { 0 };
                if let Some(file) = listing
                    .signed_out
                    .iter()
                    .filter_map(|saved| saved.described.as_ref())
                    .nth(wanted)
                    .or(listing.signed_out.first().and_then(|saved| saved.described.as_ref()))
                {
                    self.ask_sign_in_saved(file.url.clone(), cx);
                }
            }
        }
    }

    /// "Sign in" on a signed-out account's row. The address comes from a
    /// plain file, so it is shown and compared before anything is asked of
    /// it; nothing is sent from here.
    fn ask_sign_in_saved(&mut self, url: String, cx: &mut Context<Self>) {
        let existing: Vec<String> = crate::accounts::all()
            .iter()
            .map(|account| account.host())
            .collect();
        let hosts = crate::accounts::skeleton::host_sheet(&crate::accounts::host_of(&url), &existing);
        self.open_account_sheet(AccountSheet::SignIn { url, hosts }, cx);
    }

    /// "Signed out, with data on this Mac" and "Older saved data", under
    /// the account card. Each is absent when there is nothing to list.
    fn saved_data_sections(&self, mut column: Div, cx: &mut Context<Self>) -> Div {
        let t = self.theme();
        let Some(listing) = &self.saved_data else {
            return column;
        };
        // Getting a second account is switched off for now: with someone
        // signed in, a signed-out account can be erased but not signed in to.
        let can_sign_in = crate::accounts::all().is_empty() || crate::accounts::multiple();
        if !listing.signed_out.is_empty() {
            let mut card = settings_card(t);
            for (index, saved) in listing.signed_out.iter().enumerate() {
                let Some(file) = &saved.described else {
                    continue;
                };
                let (id, url) = (saved.id.clone(), file.url.clone());
                let mut detail = file.email.clone();
                if !detail.is_empty() {
                    detail.push_str(" · ");
                }
                detail.push_str(&format!(
                    "{}, {}",
                    saved.counts.stays(),
                    crate::accounts::saved::size_label(saved.bytes)
                ));
                card = card.child(
                    settings_row("globe", crate::accounts::host_of(&file.url), detail, t)
                        .when(can_sign_in, |row| {
                            row.child(
                                pill(("saved-sign-in", index), "Sign in", "", t)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.ask_sign_in_saved(url.clone(), cx)
                                    })),
                            )
                        })
                        .child(
                            pill(("saved-erase", index), "Erase…", "", t)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.ask_erase_saved(id.clone(), cx)
                                })),
                        ),
                );
            }
            column = column
                .child(section_label("Signed out, with data on this Mac", t))
                .child(card);
        }
        if !listing.older.is_empty() {
            let mut card = settings_card(t);
            for (index, saved) in listing.older.iter().enumerate() {
                let id = saved.id.clone();
                let changed = saved
                    .changed
                    .and_then(|seconds| chrono::DateTime::from_timestamp(seconds as i64, 0))
                    .map(|time| {
                        time.with_timezone(&chrono::Local)
                            .format("%-d %b %Y")
                            .to_string()
                    })
                    .unwrap_or_else(|| "an unknown date".into());
                card = card.child(
                    settings_row(
                        "folder",
                        "Saved by an earlier version of Cereal",
                        format!(
                            "{} · {} · last changed {changed}",
                            saved.counts.stays(),
                            crate::accounts::saved::size_label(saved.bytes)
                        ),
                        t,
                    )
                    .child(
                        pill(("older-erase", index), "Erase…", "", t)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.ask_erase_saved(id.clone(), cx)
                            })),
                    ),
                );
            }
            column = column
                .child(section_label("Older saved data", t))
                .child(card);
        }
        column
    }
}

fn screen_scroller(id: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        // Same scrollbar gutter as Home, so cards share one left edge.
        .pr(px(10.))
        .flex()
        .flex_col()
        .items_center()
}

/// The 626 px content column shared with Home, so cards line up across
/// pages, `width` wide (see `Cereal::page_column_width`).
fn screen_column(width: f32) -> Div {
    div()
        .w(px(width))
        .flex_shrink_0()
        .px(px(24.))
        .pt(px(9.))
        .pb(px(48.))
}

impl Cereal {
    /// The width of a page's content column: the page beside the sidebar,
    /// less the scroll bar's gutter, up to 674 px. It is given to the column
    /// in pixels. A column that was only told "as wide as the page, up to
    /// 674" was first sized with none of its text wrapped and kept that
    /// height, so each line that then wrapped (a description in Settings, an
    /// excerpt under a search result) took its height off the end of the
    /// page: the bottom padding first, then the last rows, could not be
    /// scrolled into view.
    pub(crate) fn page_column_width(&self, window: &Window) -> f32 {
        (f32::from(window.viewport_size().width) - 192. * self.sidebar_level - 10.)
            .clamp(240., 674.)
    }
}

/// A page's 24 px display title, inset to sit over the card text.
fn page_title(text: impl Into<SharedString>, t: Theme) -> Div {
    heading(text, t).px(px(9.))
}

fn settings_card(t: Theme) -> Div {
    div()
        .w_full()
        .rounded(px(16.))
        .border(px(hairline(0.5)))
        .border_color(card_edge(t))
        .bg(t.card)
        .overflow_hidden()
}

fn section_label(label: &'static str, t: Theme) -> Div {
    div()
        .mt(px(26.))
        .mb(px(6.))
        .px(px(9.))
        .text_size(px(13.))
        .line_height(px(18.))
        .font_family(FONT)
        .text_color(t.muted)
        .child(label)
}

/// One setting: a 32 px icon tile, a 13 px title over a 12 px description,
/// and whatever control the caller appends at the trailing edge. Rows sit on
/// a 64 px pitch with no dividers between them.
pub(crate) fn settings_row(
    symbol: &str,
    title: impl Into<SharedString>,
    description: impl Into<SharedString>,
    t: Theme,
) -> Div {
    div()
        .relative()
        .min_h(px(64.))
        .px(px(16.))
        .py(px(12.))
        .flex()
        .items_center()
        .gap(px(14.))
        .child(
            div()
                .size(px(32.))
                .flex_shrink_0()
                .rounded(px(8.))
                .bg(t.hover)
                .flex()
                .items_center()
                .justify_center()
                .child(icon(
                    symbol,
                    16.,
                    if symbol == "cereal" {
                        t.accent
                    } else {
                        t.secondary
                    },
                )),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                // Optically the text sits a pixel low against its tile.
                .pt(px(2.))
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .text_size(px(13.))
                        .line_height(px(18.))
                        .text_color(t.text)
                        .child(title.into()),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(t.muted)
                        .child(description.into()),
                ),
        )
}

pub(crate) fn empty_illustration(symbol: &str, t: Theme) -> Div {
    div()
        .relative()
        .size(px(90.))
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .absolute()
                .top(px(11.))
                .left(px(7.))
                .w(px(63.))
                .h(px(65.))
                .rounded(px(16.))
                .border_1()
                .border_color(t.border)
                .bg(t.fill),
        )
        .child(
            div()
                .relative()
                .w(px(63.))
                .h(px(65.))
                .ml(px(15.))
                .mt(px(3.))
                .rounded(px(16.))
                .border_1()
                .border_color(t.border)
                .bg(t.card)
                .shadow_sm()
                .flex()
                .items_center()
                .justify_center()
                .child(icon(symbol, 31., t.accent)),
        )
}

#[cfg(test)]
mod account_sheet_tests {
    use super::*;
    // `gpui::*` re-exports its own `test` attribute; these are plain unit tests.
    use std::prelude::v1::test;
    use crate::accounts::{AccountId, migration::AccountFile, saved};

    #[test]
    fn o3_the_sign_out_sheet_says_what_stays_and_offers_erasing_instead() {
        let counts = saved::Counts { notes: 214, recordings: 41, ..Default::default() };
        let sheet = AccountSheet::SignOut {
            account: AccountId::of("https://notes.acme.com", "00000000-0000-4000-8000-000000000001"),
            host: "notes.acme.com".into(),
            counts,
        }
        .text();
        assert_eq!(sheet.title, "Sign out of notes.acme.com?");
        assert_eq!(
            sheet.body,
            "214 notes and 41 recordings stay on this Mac, and are here again when you sign back in."
        );
        let labels: Vec<_> = sheet.buttons.iter().map(|(label, _)| label.as_str()).collect();
        assert_eq!(labels, ["Cancel", "Remove and erase instead…", "Sign out"]);
        assert_eq!(sheet.default, SheetAction::SignOut);
        // The erase sheet counts what goes, says what is not affected, and
        // names recordings that exist nowhere else on its button.
        let erase = AccountSheet::Erase {
            account: AccountId::of("https://notes.acme.com", "00000000-0000-4000-8000-000000000001"),
            host: "notes.acme.com".into(),
            counts: saved::Counts { notes: 214, favorites: 12, chats: 9, recordings: 41, never_uploaded: 3 },
        }
        .text();
        assert_eq!(
            erase.body,
            "214 notes saved here · 12 favourites · 9 chats · 41 recordings, 3 of which are not on the server yet. The notes on notes.acme.com are not affected."
        );
        assert_eq!(erase.buttons[1].0, "Erase, including 3 recordings that exist nowhere else");
        assert_eq!(erase.default, SheetAction::Cancel, "erasing is never what Return does");
        assert_eq!(
            erase.warning.as_deref(),
            Some("3 recordings are not on the server yet: they exist only on this Mac.")
        );
    }

    #[test]
    fn o1_signed_out_data_is_listed_and_signing_in_to_it_shows_the_host_first() {
        let root = std::env::temp_dir().join(format!("cereal-o1-{}", uuid::Uuid::new_v4()));
        let paths = crate::accounts::Paths::at(&root);
        let user = "00000000-0000-4000-8000-000000000001";
        // A signed-out account that describes itself, one planted with a
        // look-alike of a host in use, one from before accounts, and the
        // account that is signed in.
        let honest = AccountId::of("https://old.example.com", user);
        let planted = AccountId::of("https://notes.\u{0430}cme.com", user);
        let older = AccountId::of("https://Notes.Acme.com", user);
        let live = AccountId::of("https://notes.acme.com", user);
        for (id, url) in [(&honest, "https://old.example.com"), (&planted, "https://notes.\u{0430}cme.com")] {
            AccountFile { id: id.as_str().into(), url: url.into(), user_id: user.into(), email: "r@old.example".into(), label: String::new() }
                .write(&paths.account(id))
                .unwrap();
            std::fs::write(paths.workspace(id), br#"{"notes":[{"id":1,"title":"Kept"}]}"#).unwrap();
        }
        std::fs::create_dir_all(paths.recordings(&older).join("note-1")).unwrap();
        std::fs::write(paths.workspace(&older), br#"{"notes":[{"id":1},{"id":2}]}"#).unwrap();
        std::fs::create_dir_all(paths.account(&live)).unwrap();
        std::fs::write(paths.workspace(&live), b"{}").unwrap();
        let listing = saved::listing(&paths, &[live.clone()]);
        let signed_out: Vec<_> = listing.signed_out.iter().map(|saved| saved.id.clone()).collect();
        assert_eq!(signed_out.len(), 2);
        assert!(signed_out.contains(&honest) && signed_out.contains(&planted));
        assert_eq!(listing.older.len(), 1, "a directory without account.json is older saved data");
        assert_eq!(listing.older[0].id, older);
        assert_eq!(listing.older[0].counts.notes, 2);
        assert!(listing.older[0].bytes > 0 && listing.older[0].changed.is_some());
        // [Sign in] on the honest one: the host in full, beside the hosts in use.
        let hosts = |url: &str| crate::accounts::skeleton::host_sheet(&crate::accounts::host_of(url), &["notes.acme.com".to_string()]);
        let plain = AccountSheet::SignIn { url: "https://old.example.com".into(), hosts: hosts("https://old.example.com") }.text();
        assert!(plain.body.contains("old.example.com") && plain.body.contains("notes.acme.com"));
        assert_eq!(plain.default, SheetAction::Continue);
        assert!(plain.warning.is_none());
        // On the planted look-alike: a warning, and Cancel is the default.
        let warned = AccountSheet::SignIn { url: "https://notes.\u{0430}cme.com".into(), hosts: hosts("https://notes.\u{0430}cme.com") }.text();
        assert_eq!(warned.default, SheetAction::Cancel);
        assert!(warned.warning.unwrap().contains("looks like notes.acme.com, but it is a different server"));
        // Erasing is refused for the signed-in account's directory and done
        // for the others.
        assert!(!saved::erase(&paths, &live, &[live.clone()]));
        assert!(paths.account(&live).exists());
        assert!(saved::erase(&paths, &older, &[live.clone()]));
        assert!(!paths.account(&older).exists());
        let _ = std::fs::remove_dir_all(root);
    }
}
