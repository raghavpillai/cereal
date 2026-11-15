use crate::{
    app::{Cereal, Menu, Screen},
    ui::*,
};
use gpui::{prelude::*, *};

#[derive(Clone, Copy)]
enum SettingToggle {
    DarkMode,
    ShowUpcoming,
}

/// The pages of the dedicated settings mode. Only settings this app really
/// has get a tab: there are no placeholders for features that do not exist.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SettingsTab {
    #[default]
    Preferences,
    Profile,
    Calendar,
    About,
}

impl SettingsTab {
    pub const ALL: [SettingsTab; 4] = [
        SettingsTab::Preferences,
        SettingsTab::Profile,
        SettingsTab::Calendar,
        SettingsTab::About,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SettingsTab::Preferences => "Preferences",
            SettingsTab::Profile => "Profile",
            SettingsTab::Calendar => "Calendar",
            SettingsTab::About => "About",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            SettingsTab::Preferences => "sliders",
            SettingsTab::Profile => "person",
            SettingsTab::Calendar => "calendar",
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
            Screen::Connect => self.render_connect_screen(cx),
            Screen::Settings => self.render_settings_screen(cx),
            Screen::Trash => self.render_trash_screen(window, cx),
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
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .text_color(t.muted)
                                .cursor_pointer()
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
                    .w_full()
                    .max_w(px(674.))
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
                    .child(
                        div()
                            .mt(px(8.))
                            .child(self.render_chat_recipe_suggestions(cx).py(px(0.))),
                    )
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
        let mut column = screen_column()
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
        if self.chat_recipes_open {
            return self.render_chat_recipes_surface(window, cx);
        }
        let t = self.theme();
        let empty = self.chat_messages.is_empty();
        let mut column = div()
            .w_full()
            .max_w(px(672.))
            .px(px(24.))
            .pt(px(if empty { 112. } else { 24. }))
            .pb(px(28.))
            .flex()
            .flex_col();
        if empty {
            let greeting = crate::account::session()
                .map(|session| {
                    session
                        .user
                        .name
                        .trim()
                        .chars()
                        .take(40)
                        .collect::<String>()
                })
                .filter(|name| !name.is_empty())
                .map(|name| format!("Hi {name}, ask anything"))
                .unwrap_or_else(|| "Ask anything".into());
            let recents = self.render_workspace_chat_recents(cx);
            let recipes_gap = if recents.is_some() { 44. } else { 52. };
            column = column
                .child(
                    div()
                        .px(px(8.))
                        .mb(px(24.))
                        .font_family(DISPLAY)
                        .text_size(px(26.))
                        .line_height(px(32.))
                        .child(greeting),
                )
                .child(self.render_personal_chat_composer(window, cx))
                .child(
                    div()
                        .mt(px(12.))
                        .px(px(8.))
                        .h(px(28.))
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .child(
                            div()
                                .h(px(26.))
                                .px(px(6.))
                                .rounded(px(6.))
                                .bg(t.fill)
                                .flex()
                                .items_center()
                                .gap(px(4.))
                                .child(icon("bolt", 13., t.muted))
                                .child(icon("file", 13., t.muted))
                                .child(icon("code", 13., t.muted)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(12.))
                                .child("Connect your tools"),
                        )
                        .child(
                            div()
                                .id("workspace-chat-connect-tools")
                                .h(px(26.))
                                .px(px(12.))
                                .rounded_full()
                                .bg(t.fill)
                                .text_size(px(11.))
                                .flex()
                                .items_center()
                                .cursor_pointer()
                                .child(crate::motion::tactile(t.border).round())
                                .child("Get started")
                                .reach().on_click(cx.listener(|this, _, _, cx| {
                                    this.navigate(Screen::Connect, cx);
                                })),
                        ),
                )
                .when_some(recents, |column, recents| column.child(recents))
                .child(
                    div()
                        .mt(px(recipes_gap))
                        .px(px(8.))
                        .text_size(px(11.))
                        .line_height(px(16.))
                        .text_color(t.muted)
                        .child("Recipes"),
                )
                .child(self.render_chat_recipe_suggestions(cx));
        } else {
            for (index, (question, answer)) in self.chat_messages.iter().enumerate() {
                column = column.child(self.render_chat_turn(index, question, answer, false, cx));
            }
            if let Some(queue) = self.render_chat_queue(false, cx) {
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
            .when(self.chat_can_scroll_down(), |d| {
                d.child(self.render_chat_scroll_down(cx))
            })
            .into_any_element()
    }

    fn render_workspace_chat_recents(&self, cx: &mut Context<Self>) -> Option<Div> {
        let t = self.theme();
        let chats: Vec<_> = self
            .chat_archives
            .iter()
            .rev()
            .filter(|chat| {
                chat.note_id == 0
                    || self
                        .workspace
                        .note(chat.note_id)
                        .is_some_and(|note| !note.trashed)
            })
            .take(3)
            .collect();
        if chats.is_empty() {
            return None;
        }
        let mut section = div().mt(px(52.)).child(
            div()
                .px(px(8.))
                .h(px(16.))
                .flex()
                .items_center()
                .justify_between()
                .text_size(px(11.))
                .line_height(px(16.))
                .text_color(t.muted)
                .child("Recents")
                .child(
                    div()
                        .id("workspace-chat-all-recents")
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .cursor_pointer()
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
        let mut rows = div().mt(px(10.)).px(px(6.));
        for chat in chats {
            let archive_id = chat.id.clone();
            let updated = chat.updated_at.as_deref().and_then(|value| {
                let timestamp = chrono::DateTime::parse_from_rfc3339(value).ok()?;
                let age = chrono::Utc::now().signed_duration_since(timestamp);
                let label = if age.num_seconds() < 0 {
                    timestamp
                        .with_timezone(&chrono::Local)
                        .format("%-I:%M %p")
                        .to_string()
                } else if age.num_minutes() == 0 {
                    "now".into()
                } else if age.num_hours() == 0 {
                    format!("{}m", age.num_minutes())
                } else if age.num_days() == 0 {
                    format!("{}h", age.num_hours())
                } else if age.num_days() < 30 {
                    format!("{}d", age.num_days())
                } else {
                    format!("{}mo", age.num_days() / 30)
                };
                Some(label)
            });
            rows = rows.child(
                div()
                    .id(SharedString::from(format!("workspace-recent-{}", chat.id)))
                    .h(px(40.))
                    .rounded(px(7.))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .cursor_pointer()
                    .child(crate::motion::tactile(t.hover).radius(7.))
                    .child(
                        div()
                            .size(px(28.))
                            .rounded(px(7.))
                            .bg(t.fill)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon(
                                if chat.note_id == 0 { "chat" } else { "file" },
                                15.,
                                t.muted,
                            )),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(12.))
                            .truncate()
                            .child(chat.title.clone()),
                    )
                    .when_some(updated, |row, label| {
                        row.child(
                            div()
                                .pr(px(6.))
                                .text_size(px(11.))
                                .text_color(t.muted)
                                .child(label),
                        )
                    })
                    .reach().on_click(cx.listener(move |this, _, _, cx| {
                        this.restore_archived_chat(&archive_id, true, cx);
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
            .w_full()
            .min_h(px(82.))
            .px(px(12.))
            .pt(px(11.))
            .pb(px(7.))
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
            .gap(px(8.))
            .child(
                div()
                    .id("personal-chat-composer-text")
                    .px(px(4.))
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
                        div()
                            .id("personal-chat-configured-model")
                            .text_size(px(11.))
                            .text_color(t.muted)
                            .opacity(0.7)
                            .child(if crate::server::enabled() {
                                "Configured model"
                            } else {
                                "Local search"
                            }),
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

    fn render_connect_screen(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = self.theme();
        screen_scroller("connect-screen")
            .child(
                screen_column()
                    .child(page_title("Connect", t))
                    .child(
                        div()
                            .mt(px(9.))
                            .px(px(8.))
                            .text_color(t.muted)
                            .line_height(px(21.))
                            .child("Manage calendar connections on your own server."),
                    )
                    .child(
                        settings_card(t)
                            .mt(px(28.))
                            .child(self.calendar_connections_row(cx)),
                    ),
            )
            .into_any_element()
    }

    /// The sidebar of settings mode: who is signed in, the settings pages, and
    /// Sign out when there is a session to end.
    pub fn render_settings_sidebar(&self, cx: &mut Context<Self>) -> Div {
        let t = self.theme();
        let glide = self.stage.glide("sidebar");
        let connected = crate::server::enabled();
        let session = crate::account::session().filter(|_| connected);
        let name = self.workspace_name();
        let detail = match &session {
            Some(session) => session.user.email.clone(),
            None if connected => "Sign in to your server".into(),
            None => "Local demo on this Mac".into(),
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
                    .cursor_pointer()
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
                            .cursor_pointer()
                            .child(row_feedback(t, 8.))
                            .child(icon("share", 16., t.danger))
                            .child("Sign out")
                            .reach().on_click(cx.listener(|this, _, _, cx| this.sign_out_account(cx))),
                    ),
                )
            })
    }

    fn render_settings_screen(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = self.theme();
        let connected = crate::server::enabled();
        let tab = self.settings_tab;
        let mut column = screen_column().child(page_title(tab.label(), t));
        match tab {
            SettingsTab::Preferences => {
                column = column
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
                            .child(settings_row(
                                "globe",
                                "Processing language",
                                if connected {
                                    "Used for transcription, notes and chat."
                                } else {
                                    "Applies once you connect to your server."
                                },
                                t,
                            )
                            .child(small("English", t)))
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
                            ),
                    );
            }
            SettingsTab::Profile => {
                let session = crate::account::session().filter(|_| connected);
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
                    None if connected => (
                        "Your account".to_string(),
                        "Sign in to your server.".to_string(),
                    ),
                    None => (
                        "Personal workspace".to_string(),
                        "Notes saved locally on this Mac.".to_string(),
                    ),
                };
                column = column.child(section_label("Account", t)).child(
                    settings_card(t)
                        .child(
                            settings_row("cereal", title, detail, t).child(
                                div()
                                    .px(px(9.))
                                    .py(px(4.))
                                    .rounded_full()
                                    .bg(t.fill)
                                    .text_size(px(11.))
                                    .text_color(t.muted)
                                    .child(if session.is_some() {
                                        "CONNECTED"
                                    } else {
                                        "LOCAL DEMO"
                                    }),
                            ),
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
                        ),
                );
            }
            SettingsTab::Calendar => {
                column = column
                    .child(section_label("Home", t))
                    .child(settings_card(t).child(self.setting_toggle(
                        "setting-upcoming",
                        "calendar",
                        "Show upcoming meetings",
                        "Keep your calendar in view on Home.",
                        self.workspace.settings.show_upcoming,
                        SettingToggle::ShowUpcoming,
                        cx,
                    )))
                    .child(section_label("Connections", t))
                    .child(settings_card(t).child(self.calendar_connections_row(cx)));
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
                                    if connected {
                                        "Notes, transcripts and shared links belong to your account on your server. Audio is captured on this Mac and securely delivered for transcription when you record."
                                    } else {
                                        "Local demo notes stay on this Mac and remain separate from signed-in accounts. Sign in to use server transcription, summaries and shared notes."
                                    },
                                    t,
                                )
                                .child(small(
                                    if connected { "Your server" } else { "On this Mac" },
                                    t,
                                )),
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
                                            this.menu = Some(Menu::Shortcuts);
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
                d.cursor_pointer()
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
            .cursor_pointer()
            .child({
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
                        .size(px(14.))
                        .rounded_full()
                        .when(on == enabled, |slot| slot.child(knob.probe(on)))
                };
                div()
                    .relative()
                    .flex_shrink_0()
                    .mt(px(4.))
                    .w(px(32.))
                    .h(px(20.))
                    .p(px(3.))
                    .rounded_full()
                    .bg(t.fill)
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .rounded_full()
                            .bg(t.accent.opacity(level)),
                    )
                    .child(knob.indicator(rgb(0xffffff).into(), 7.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(slot(false))
                    .child(slot(true))
            })
            .reach().on_click(cx.listener(move |this, _, _, cx| {
                match setting {
                    SettingToggle::DarkMode => {
                        this.workspace.settings.dark_mode = !this.workspace.settings.dark_mode
                    }
                    SettingToggle::ShowUpcoming => {
                        this.workspace.settings.show_upcoming =
                            !this.workspace.settings.show_upcoming
                    }
                }
                this.persist(cx);
            }))
    }

    fn render_trash_screen(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let t = self.theme();
        // Only what a Trash row shows is copied: its id, title and date.
        let notes: Vec<_> = self
            .workspace
            .notes
            .iter()
            .filter(|note| note.trashed)
            .map(|note| (note.id, note.title.clone(), note.date.clone()))
            .collect();
        crate::motion::stats::copied(
            notes
                .iter()
                .map(|(_, title, date)| title.len() + date.len())
                .sum(),
        );
        let empty = notes.is_empty();
        let scroll = crate::lazy::scroll("trash");
        let keys: Vec<u64> = notes
            .iter()
            .map(|(id, title, _)| crate::lazy::key(("trashed", id, title)))
            .collect();
        screen_scroller("trash-screen")
            .track_scroll(&scroll)
            .child(
                screen_column()
                    .child(page_title("Trash", t))
                    .child(
                        div()
                            .mt(px(9.))
                            .text_color(t.muted)
                            .line_height(px(21.))
                            .child("Changed your mind? Restore a note to bring it back."),
                    )
                    .when(empty, |s| {
                        s.child(
                            div()
                                .mt(px(90.))
                                .flex()
                                .flex_col()
                                .items_center()
                                .gap(px(15.))
                                .child(empty_illustration("trash", t))
                                .child(
                                    div()
                                        .mt(px(8.))
                                        .font_family(DISPLAY)
                                        .text_size(px(24.))
                                        .child("A clean slate"),
                                )
                                .child(small("There’s nothing in your trash.", t)),
                        )
                    })
                    .when(!empty, |s| {
                        // Cards 8 px apart, built only near the viewport.
                        s.child(div().mt(px(28.)).child(crate::lazy::column(
                            "trash",
                            &keys,
                            f32::from(window.viewport_size().width),
                            8.,
                            &scroll,
                            window,
                            |index| {
                                let (id, title, date) = notes[index].clone();
                                div()
                                    .p(px(14.))
                                    .flex()
                                    .items_center()
                                    .gap(px(14.))
                                    .rounded(px(12.))
                                    .border_1()
                                    .border_color(t.border)
                                    .bg(t.card)
                                    .child(
                                        div()
                                            .size(px(35.))
                                            .rounded(px(8.))
                                            .bg(t.fill)
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(icon("document", 19., t.muted)),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .flex()
                                            .flex_col()
                                            .gap(px(5.))
                                            .child(title)
                                            .child(small(date, t)),
                                    )
                                    .child(
                                        pill(("restore-note", id), "Restore", "arrow-left", t)
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if let Some(note) = this.workspace.note_mut(id) {
                                                    note.trashed = false;
                                                }
                                                if this.persist(cx) {
                                                    this.show_toast("Note restored", cx);
                                                }
                                            })),
                                    )
                                    .into_any_element()
                            },
                        )))
                    }),
            )
            .into_any_element()
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

/// The 626 px content column shared with Home, so cards line up across pages.
fn screen_column() -> Div {
    div()
        .w_full()
        .max_w(px(674.))
        .px(px(24.))
        .pt(px(9.))
        .pb(px(48.))
}

/// A page's 24 px display title, inset to sit over the card text.
fn page_title(text: impl Into<SharedString>, t: Theme) -> Div {
    heading(text, t).px(px(9.))
}

fn settings_card(t: Theme) -> Div {
    div()
        .w_full()
        .rounded(px(16.))
        .border_1()
        .border_color(t.border)
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
