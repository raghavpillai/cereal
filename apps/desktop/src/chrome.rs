use crate::{
    app::{Cereal, Menu, Screen},
    ui::*,
};
use gpui::{prelude::*, *};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

type MenuRowAction = Rc<dyn Fn(&mut Cereal, &mut Window, &mut Context<Cereal>)>;

/// The wait before a tooltip shows; the same 500 ms GPUI uses for its own.
const TOOLTIP_DELAY: std::time::Duration = std::time::Duration::from_millis(500);

thread_local! {
    /// Bumped by every pointer change over the sidebar footer tools, so a
    /// pending tip is dropped if the pointer has moved on or clicked.
    static FOOTER_TIP_EPOCH: Cell<u64> = const { Cell::new(0) };
}

fn footer_tip_epoch() -> u64 {
    FOOTER_TIP_EPOCH.with(|epoch| {
        epoch.set(epoch.get().wrapping_add(1));
        epoch.get()
    })
}

/// Rebuilt from actual enabled rows each frame; disabled placeholders never enter
/// the keyboard list. Enter uses the identical callback as a pointer click.
#[derive(Default)]
pub struct MenuKeyboard {
    pub selected: Option<usize>,
    rows: RefCell<Vec<MenuRowAction>>,
    menu: RefCell<Option<Menu>>,
}

impl MenuKeyboard {
    pub fn reset(&mut self) {
        self.selected = None;
        self.rows.borrow_mut().clear();
        *self.menu.borrow_mut() = None;
    }

    fn register(
        &self,
        action: impl Fn(&mut Cereal, &mut Window, &mut Context<Cereal>) + 'static,
    ) -> (usize, MenuRowAction) {
        let action: MenuRowAction = Rc::new(action);
        let mut rows = self.rows.borrow_mut();
        let index = rows.len();
        rows.push(action.clone());
        (index, action)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MenuKey {
    Select(usize),
    Activate(usize),
    Dismiss,
}

/// Keyboard selection in the command palette. Rows register themselves in
/// render order each frame, exactly like [`MenuKeyboard`]; Return runs the
/// same callback as a click.
pub struct PaletteKeyboard {
    /// The row chosen with the arrows or the pointer. `None` until then: the
    /// palette's own default row (see [`palette_sections`]) is highlighted and
    /// Return keeps its usual meaning.
    selected: Cell<Option<usize>>,
    default: Cell<usize>,
    rows: RefCell<Vec<MenuRowAction>>,
    query: RefCell<String>,
    scroll: ScrollHandle,
    /// Bring the selected row into view on the next render (after an arrow).
    reveal: Cell<bool>,
    /// Whether the palette was open at the last render: reopening it starts
    /// from the default row, while a closing one keeps its highlight still.
    was_open: Cell<bool>,
}

impl Default for PaletteKeyboard {
    fn default() -> Self {
        Self {
            selected: Cell::new(None),
            default: Cell::new(0),
            rows: RefCell::default(),
            query: RefCell::default(),
            scroll: ScrollHandle::new(),
            reveal: Cell::new(false),
            was_open: Cell::new(false),
        }
    }
}

impl PaletteKeyboard {
    /// Select a row as an arrow key would (used by the tour).
    pub fn select(&self, index: usize) {
        self.selected.set(Some(index));
        self.reveal.set(true);
    }
}

/// How many rows of the palette can be selected, and which one is
/// highlighted before the user has chosen: the row Return already acts on.
pub fn palette_selection(sections: &[PaletteSection]) -> (usize, usize) {
    let rows = sections
        .iter()
        .flat_map(|section| &section.rows)
        .filter(|item| item.row != PaletteRow::Empty);
    let default = rows.clone().position(|item| item.selected).unwrap_or(0);
    (rows.count(), default)
}

fn menu_key(key: &str, modified: bool, selected: Option<usize>, count: usize) -> Option<MenuKey> {
    if modified {
        return None;
    }
    if key == "escape" {
        return Some(MenuKey::Dismiss);
    }
    if count == 0 {
        return None;
    }
    let selected = selected.filter(|index| *index < count);
    match key {
        "down" => Some(MenuKey::Select(
            selected.map_or(0, |index| (index + 1) % count),
        )),
        "up" => Some(MenuKey::Select(
            selected.map_or(count - 1, |index| (index + count - 1) % count),
        )),
        "enter" | "return" => selected.map(MenuKey::Activate),
        _ => None,
    }
}

impl Cereal {
    /// Arrow keys move the palette's highlight; Return activates a row the
    /// user has chosen. Everything else (typing, Escape, Return on the
    /// default row) is left to the search field.
    pub(crate) fn handle_palette_key(
        &mut self,
        event: &KeystrokeEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.search_open {
            return;
        }
        let count = self.palette.rows.borrow().len();
        let modifiers = event.keystroke.modifiers;
        let modified = modifiers.control || modifiers.alt || modifiers.shift || modifiers.platform;
        let chosen = self.palette.selected.get().filter(|index| *index < count);
        let current = chosen.unwrap_or(self.palette.default.get());
        match menu_key(&event.keystroke.key, modified, Some(current), count) {
            Some(MenuKey::Select(index)) => {
                self.palette.select(index);
                cx.notify();
                cx.stop_propagation();
            }
            // Return acts on the highlighted row whether the arrows put the
            // highlight there or it is still on the row it starts on.
            Some(MenuKey::Activate(index)) => {
                let action = self.palette.rows.borrow().get(index).cloned();
                if let Some(action) = action {
                    action(self, window, cx);
                }
                cx.stop_propagation();
            }
            _ => {}
        }
    }

    pub(crate) fn handle_menu_key(
        &mut self,
        event: &KeystrokeEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.menu.is_none() {
            return;
        }
        let rows_current = self.menu.as_ref() == self.menu_keyboard.menu.borrow().as_ref();
        let count = if rows_current {
            self.menu_keyboard.rows.borrow().len()
        } else {
            0
        };
        let modifiers = event.keystroke.modifiers;
        let modified = modifiers.control || modifiers.alt || modifiers.shift || modifiers.platform;
        let Some(command) = menu_key(
            &event.keystroke.key,
            modified,
            self.menu_keyboard.selected,
            count,
        ) else {
            return;
        };
        match command {
            MenuKey::Select(index) => {
                self.menu_keyboard.selected = Some(index);
                cx.notify();
            }
            MenuKey::Activate(index) => {
                let action = self.menu_keyboard.rows.borrow().get(index).cloned();
                if let Some(action) = action {
                    action(self, window, cx);
                }
            }
            MenuKey::Dismiss => {
                self.menu = None;
                self.menu_keyboard.selected = None;
                self.focus.focus(window);
                cx.notify();
            }
        }
        cx.stop_propagation();
    }

    pub fn render_sidebar(&self, cx: &mut Context<Self>) -> Div {
        let t = self.theme();
        let glide = self.stage.glide("sidebar");
        let settings = self.screen == Screen::Settings && self.current_note.is_none();
        let body = if settings {
            self.render_settings_sidebar(cx)
        } else {
            self.render_workspace_sidebar(cx)
        };
        div()
            .w(px(192.))
            .h_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .bg(t.sidebar)
            .border_r_1()
            .border_color(t.hairline)
            .text_size(px(13.))
            // One highlight for the whole sidebar: it stretches from the old
            // selection to the new one instead of blinking between rows.
            .child(glide.indicator(t.hover, 8.))
            .child(
                div()
                    .h(px(48.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .pl(px(88.))
                    .gap(px(4.))
                    .child(
                        tool("sidebar-toggle", "sidebar-expanded", t).on_click(cx.listener(
                            |this, _, _, cx| {
                                this.sidebar_open = false;
                                cx.notify();
                            },
                        )),
                    )
                    .child(
                        tool("sidebar-search", "search", t)
                            .on_click(cx.listener(|this, _, w, cx| this.open_search(w, cx))),
                    ),
            )
            // Entering or leaving Settings swaps the whole sidebar; the new
            // one settles in like a page.
            .child(crate::motion::page(
                ("sidebar-mode", settings as u64),
                div().flex_1().min_h_0().flex().flex_col().child(body),
            ))
    }

    /// A 20 px rounded tile holding a space's glyph.
    fn space_tile(&self, symbol: &str) -> Div {
        let t = self.theme();
        div()
            .size(px(20.))
            .flex_shrink_0()
            .rounded(px(5.))
            .border_1()
            .border_color(t.hairline)
            .bg(t.fill)
            .flex()
            .items_center()
            .justify_center()
            .child(icon(symbol, 12., t.secondary))
    }

    fn render_workspace_sidebar(&self, cx: &mut Context<Self>) -> Div {
        let t = self.theme();
        let folders_hover = crate::motion::reveal_key("personal-folders");
        let glide = self.stage.glide("sidebar");
        let mut spaces = div()
            .flex()
            .flex_col()
            .px(px(8.))
            .mt(px(19.))
            .child(
                div()
                    .h(px(26.))
                    .flex()
                    .items_center()
                    .px(px(4.))
                    .child(small("Spaces", t)),
            )
            .child(
                div()
                    .id("My notes")
                    .h(px(29.))
                    .px(px(4.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .rounded(px(8.))
                    .text_color(t.secondary)
                    .cursor_pointer()
                    .child(row_feedback(t, 8.))
                    .when(
                        self.screen == Screen::MyNotes && self.current_note.is_none(),
                        |s| s.child(glide.probe("My notes")),
                    )
                    .child(self.space_tile("lock-solid"))
                    .child(div().flex_1().min_w_0().truncate().child("My notes"))
                    .reach().on_click(cx.listener(|this, _, _, cx| this.navigate(Screen::MyNotes, cx))),
            )
            .child(
                div()
                    .id("personal-folders")
                    .h(px(29.))
                    .px(px(4.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .rounded(px(8.))
                    .text_color(t.secondary)
                    .cursor_pointer()
                    .child(row_feedback(t, 8.).reveals(folders_hover))
                    .reach().on_click(cx.listener(|this, _, _, cx| {
                        this.folders_open = !this.folders_open;
                        cx.notify();
                    }))
                    // Under the pointer the tile gives way to the disclosure
                    // chevron, and the add-folder control appears.
                    .child(
                        div()
                            .relative()
                            .size(px(20.))
                            .flex_shrink_0()
                            .child(
                                crate::motion::conceal(self.space_tile("prompt"), folders_hover)
                                    .absolute()
                                    .inset_0(),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .inset_0()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .opacity(crate::motion::revealed(folders_hover))
                                    // One chevron that turns, rather than
                                    // two glyphs swapped: right when closed.
                                    .child(
                                        icon("chevron-down", 12., t.muted).with_transformation(
                                            Transformation::rotate(percentage(
                                                self.stage
                                                    .aim(
                                                        ("folders-chevron", 0),
                                                        if self.folders_open { 1. } else { 0.75 },
                                                        crate::motion::SNAPPY,
                                                    )
                                                    .rem_euclid(1.),
                                            )),
                                        ),
                                    ),
                            ),
                    )
                    .child(div().flex_1().min_w_0().truncate().child("My folders"))
                    .child(
                        tool_tip("new-folder", "plus", "New folder", t)
                            .size(px(20.))
                            .opacity(crate::motion::revealed(folders_hover))
                            .on_click(cx.listener(|this, _, w, cx| {
                                cx.stop_propagation();
                                this.folder_input.update(cx, |input, cx| {
                                    input.set_content("", cx);
                                    input.set_placeholder("Name", cx);
                                });
                                this.menu = Some(Menu::NewFolder);
                                this.folder_input.read(cx).focus_handle.focus(w);
                                cx.notify();
                            })),
                    ),
            );
        // The folder rows unfold beneath "My folders" and fold back into it:
        // 29 px a row, closing up on the exit curve. At rest they are plain
        // children, exactly as before.
        let folders = self
            .stage
            .presence(("folders", 0), self.folders_open, crate::motion::SNAPPY);
        if folders.visible {
            let mut rows = div().flex().flex_col();
            for folder in &self.workspace.folders {
                let id = folder.id;
                rows =
                    rows.child(
                        div()
                            .id(("folder", id))
                            .ml(px(18.))
                            .h(px(29.))
                            .px(px(6.))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .rounded(px(7.))
                            .text_color(t.secondary)
                            .cursor_pointer()
                            .child(row_feedback(t, 7.))
                            .when(
                                self.screen == Screen::Folder(id) && self.current_note.is_none(),
                                |s| s.child(glide.probe(("folder", id))),
                            )
                            .child(icon("folder", 15., t.secondary))
                            // `min_w_0` lets a long name end in an ellipsis
                            // instead of being cut mid-glyph.
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .child(folder.name.clone()),
                            )
                            .reach().on_click(cx.listener(move |this, _, _, cx| {
                                this.navigate(Screen::Folder(id), cx)
                            })),
                    );
            }
            if !folders.at_rest() {
                let natural = 29. * self.workspace.folders.len() as f32;
                rows = rows
                    .h(px(natural * folders.value.clamp(0., 1.)))
                    .overflow_hidden()
                    .opacity(folders.fade);
            }
            spaces = spaces.child(rows);
        }
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .pt(px(8.))
                    .px(px(8.))
                    .flex()
                    .flex_col()
                    .gap(px(1.))
                    .child(self.nav_row("Home", "house", Screen::Home, cx))
                    .child(self.nav_row("Shared with me", "people", Screen::Shared, cx))
                    .child(self.nav_row("Chat", "speech-bubble", Screen::Chat, cx))
                    .child(self.nav_row("Connect", "connect-grid", Screen::Connect, cx)),
            )
            .child(spaces)
            .child(div().flex_1())
            .child(self.render_sidebar_footer_tools(cx))
            .child(div().mx(px(12.)).h(px(1.)).bg(t.hairline))
            .child(
                div()
                    .id("workspace-switcher")
                    .h(px(43.))
                    .pl(px(12.))
                    .pr(px(8.))
                    .pb(px(1.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .cursor_pointer()
                    .child(row_feedback(t, 0.))
                    .child(self.space_tile("prompt"))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_weight(FontWeight::MEDIUM)
                            .child(self.workspace_name()),
                    )
                    .child(icon("chevrons", 14., t.muted))
                    .reach().on_click(cx.listener(|this, _, _, cx| {
                        this.menu = if this.menu == Some(Menu::Workspace) {
                            None
                        } else {
                            Some(Menu::Workspace)
                        };
                        cx.notify();
                    })),
            )
    }

    /// The signed-in person's name, or the local workspace's name.
    pub(crate) fn workspace_name(&self) -> String {
        crate::account::session()
            .filter(|_| crate::server::enabled())
            .map(|session| session.user.name)
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| "My workspace".into())
    }

    /// Footer shortcuts. Their labels open above the icons so they never cover
    /// the workspace row beneath, and grow out of the icon like other popovers.
    fn render_sidebar_footer_tools(&self, cx: &mut Context<Self>) -> Div {
        let t = self.theme();
        let tip = self.stage.presence(
            ("footer-tip", 0),
            self.footer_tip.is_some() && self.menu.is_none(),
            crate::motion::SNAPPY,
        );
        let tool =
            |index: usize, id: &'static str, symbol: &'static str, label: &'static str| {
                div()
                    .id(id)
                    .relative()
                    .size(px(28.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(6.))
                    .reach()
                    .cursor_pointer()
                    .child(crate::motion::tactile(t.hover).press(t.fill))
                    .child(icon(symbol, 16., t.secondary))
                    // Like every other tooltip, the tip waits before it shows,
                    // so passing over the footer neither flashes a label nor
                    // re-renders the window.
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        let epoch = footer_tip_epoch();
                        if *hovered {
                            cx.spawn(async move |this, cx| {
                                cx.background_executor().timer(TOOLTIP_DELAY).await;
                                let _ = this.update(cx, |this, cx| {
                                    if FOOTER_TIP_EPOCH.with(Cell::get) == epoch {
                                        this.footer_tip = Some(index);
                                        this.footer_tip_shown = index;
                                        cx.notify();
                                    }
                                });
                            })
                            .detach();
                        } else if this.footer_tip == Some(index) {
                            this.footer_tip = None;
                            cx.notify();
                        }
                    }))
                    .when(tip.visible && self.footer_tip_shown == index, |tool| {
                        tool.child(
                            div().absolute().bottom(px(33.)).left(px(-4.)).child(
                                crate::motion::pop(
                                    div()
                                        .px(px(8.))
                                        .py(px(4.))
                                        .rounded(px(8.))
                                        .bg(t.text)
                                        .text_color(t.bg)
                                        .text_size(px(12.))
                                        .line_height(px(16.))
                                        .whitespace_nowrap()
                                        .child(label),
                                    crate::motion::Pop::chip((0.15, 1.)),
                                    tip,
                                ),
                            ),
                        )
                    })
            };
        div()
            .px(px(8.))
            .pb(px(6.))
            .flex()
            .items_center()
            .gap(px(1.))
            .child(
                tool(0, "footer-recipes", "recipe", "Recipes").on_click(cx.listener(
                    |this, _, window, cx| {
                        footer_tip_epoch();
                        this.footer_tip = None;
                        this.open_recipes_from_sidebar(window, cx);
                    },
                )),
            )
            .child(
                tool(1, "footer-shortcuts", "keyboard", "Keyboard shortcuts").on_click(
                    cx.listener(|this, _, _, cx| {
                        footer_tip_epoch();
                        this.footer_tip = None;
                        this.menu = Some(Menu::Shortcuts);
                        cx.notify();
                    }),
                ),
            )
    }

    fn nav_row(
        &self,
        label: &'static str,
        symbol: &str,
        screen: Screen,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let t = self.theme();
        div()
            .id(label)
            .h(px(28.))
            .w_full()
            .px(px(6.))
            .flex()
            .items_center()
            .gap(px(8.))
            .rounded(px(8.))
            .text_color(t.secondary)
            .cursor_pointer()
            .child(row_feedback(t, 8.))
            .when(self.screen == screen && self.current_note.is_none(), |s| {
                s.child(self.stage.glide("sidebar").probe(label))
            })
            .child(icon(symbol, 16., t.secondary))
            .child(div().flex_1().min_w_0().truncate().child(label))
            .reach().on_click(cx.listener(move |this, _, _, cx| this.navigate(screen, cx)))
    }

    pub fn render_header(&self, cx: &mut Context<Self>) -> Div {
        let t = self.theme();
        if self.current_note.is_none() && self.screen == Screen::Link {
            return self.render_link_header(cx);
        }
        if self.current_note.is_some() {
            // The session pill belongs to the other pages; coming back to
            // them it arrives again rather than already being there.
            self.stage
                .presence(("active-session-pill", 0), false, crate::motion::GOOEY);
            return div()
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
                    back_pill("back-to-notes", BackLabel::Icon("home"), t).on_click(cx.listener(
                        |this, _, window, cx| {
                            this.navigate(Screen::Home, cx);
                            this.focus.focus(window);
                        },
                    )),
                )
                .when(
                    self.current_note
                        .and_then(|id| self.workspace.note(id))
                        .is_some_and(|note| {
                            !note.body.trim().is_empty() || !note.enhanced.trim().is_empty()
                        }),
                    |d| {
                        let options = div().flex().items_center().gap(px(8.)).child(
                            tool_tip("note-options", "more", "Note options", t).on_click(cx.listener(|this, _, window, cx| {
                                this.focus.focus(window); this.menu = Some(Menu::Note); cx.notify();
                            }))
                        );
                        d.child(options.when(self.current_note.and_then(|id| self.workspace.note(id)).is_some_and(|note| !note.enhanced.trim().is_empty()), |options| options.child(
                            div().flex().gap(px(1.))
                                .child(div().id("note-share").h(px(28.)).px(px(11.)).rounded_l(px(14.)).rounded_r(px(4.)).bg(t.text).text_color(t.bg).text_size(px(13.)).flex().items_center().gap(px(5.)).cursor_pointer().map(|d|crate::motion::dims(d,crate::motion::reveal_key("note-share"),0.1)).child(icon("lock-solid",13.,t.bg)).child("Share").reach().on_click(cx.listener(|this,_,window,cx|{this.focus.focus(window);this.menu=Some(Menu::Share);cx.notify();})))
                                .child(div().id("note-copy-share-link").w(px(30.)).h(px(28.)).rounded_l(px(4.)).rounded_r(px(14.)).bg(t.text).flex().items_center().justify_center().cursor_pointer().map(|d|crate::motion::dims(d,crate::motion::reveal_key("note-copy-share-link"),0.1)).child(icon("link-chain",14.,t.bg)).reach().on_click(cx.listener(|this,_,_,cx|{if let Some(id)=this.current_note { if crate::server::enabled() { this.server_action(id,crate::server::Action::Share,cx); } else { this.show_toast("Connect to your server to share a note",cx); } }})))
                        )))
                    },
                );
        }
        div()
            .h(px(48.))
            .flex_shrink_0()
            .flex()
            .items_start()
            .justify_between()
            .px(px(8.))
            .pt(px(8.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .ml(px(80. * (1. - self.sidebar_level)))
                    .when(!self.sidebar_open, |s| {
                        s.child(tool("open-sidebar", "sidebar-collapsed", t).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.sidebar_open = true;
                                cx.notify();
                            }),
                        ))
                        .child(
                            tool("collapsed-header-search", "search", t).on_click(
                                cx.listener(|this, _, window, cx| this.open_search(window, cx)),
                            ),
                        )
                    })
                    // Every page but Home can step back to where it came from.
                    .when_some(back_label(self.screen, self.back_target()), |d, label| {
                        d.child(back_pill("header-back", label, t).on_click(cx.listener(
                            |this, _, window, cx| {
                                this.go_back(cx);
                                this.focus.focus(window);
                            },
                        )))
                    }),
            )
            .when(self.screen == Screen::Settings, |header| {
                header.child(
                    div()
                        .absolute()
                        .top(px(12.))
                        .left_0()
                        .right_0()
                        .flex()
                        .justify_center()
                        .text_size(px(13.))
                        .line_height(px(18.))
                        .text_color(t.muted)
                        .child("Settings"),
                )
            })
            .when_some(header_action(self.screen), |header, action| {
                header.child(
                    div().flex().items_center().gap(px(8.)).child(match action {
                        HeaderAction::NewNote => pill("new-note", "New note", "plus-small", t)
                            .on_click(cx.listener(|this, _, w, cx| this.new_note(w, cx))),
                        HeaderAction::NewChat => pill("new-chat", "New chat", "edit", t).on_click(
                            cx.listener(|this, _, window, cx| {
                                this.clear_chat(cx);
                                this.ask.read(cx).focus_handle.focus(window);
                            }),
                        ),
                    }),
                )
            })
            .relative()
            .map(|header| {
                // The pill arrives while a session runs elsewhere and folds
                // back up into the title bar when it stops.
                let note = self
                    .recording
                    .note_id
                    .and_then(|id| self.workspace.note(id));
                let active = self.recording.is_active() && note.is_some();
                let pose =
                    self.stage
                        .presence(("active-session-pill", 0), active, crate::motion::GOOEY);
                if let Some(note) = note.filter(|_| pose.visible) {
                    let id = note.id;
                    let title = if note.title.trim().is_empty() {
                        "New note".to_owned()
                    } else {
                        note.title.clone()
                    };
                    let width = (title.chars().count() as f32 * 6.6 + 66.).clamp(118., 240.);
                    let hover = if self.workspace.settings.dark_mode {
                        rgb(0x414140).into()
                    } else {
                        t.hover
                    };
                    header.child(crate::motion::pop(
                        div()
                            .id("active-session-pill")
                            .absolute()
                            .top(px(6.))
                            .left(relative(0.5))
                            .ml(px(-width / 2. - 96. * self.sidebar_level))
                            .w(px(width))
                            .h(px(32.))
                            .pl(px(10.))
                            .pr(px(5.))
                            .rounded_full()
                            .border_1()
                            .border_color(t.border)
                            .bg(t.card)
                            .flex()
                            .items_center()
                            .gap(px(7.))
                            .cursor(CursorStyle::Arrow)
                            .child(crate::motion::tactile(hover).round())
                            .child(crate::recording_view::recording_waveform(
                                self.recording.audio_level(),
                                rgb(0xce9d00).into(),
                            ))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(13.))
                                    .child(title),
                            )
                            .reach().on_click(cx.listener(move |this, _, window, cx| {
                                this.open_note(id, cx);
                                this.focus.focus(window);
                            }))
                            .child(
                                round_tool("active-session-stop", "close", "Stop transcript", t)
                                    .size(px(22.))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        cx.stop_propagation();
                                        this.focus.focus(window);
                                        this.stop_recording_preview(cx);
                                    })),
                            ),
                        crate::motion::Pop::chip((0.5, 0.)),
                        pose,
                    ))
                } else {
                    header
                }
            })
    }

    pub fn render_search(&self, cx: &mut Context<Self>) -> Div {
        let t = self.theme();
        let q = self.search.read(cx).content();
        let query = q.trim();
        let notes = self.search_ids(&q);
        let folders: Vec<PaletteFolder> = self
            .workspace
            .folders
            .iter()
            .map(|folder| PaletteFolder {
                id: folder.id,
                name: folder.name.clone(),
                notes: self
                    .workspace
                    .notes
                    .iter()
                    .filter(|note| note.folder_id == Some(folder.id) && !note.trashed)
                    .count(),
            })
            .collect();
        let sections = palette_sections(query, &folders, &notes);
        // One highlight shows where Return will go. It starts on the default
        // row, follows the arrow keys and the pointer, and glides between
        // rows; a new query or a closed palette returns it to the default.
        let keyboard = &self.palette;
        keyboard.rows.borrow_mut().clear();
        let reopened = self.search_open && !keyboard.was_open.replace(self.search_open);
        if reopened || *keyboard.query.borrow() != q {
            keyboard.selected.set(None);
            keyboard.query.replace(q.clone());
        }
        keyboard.was_open.set(self.search_open);
        let (count, default) = palette_selection(&sections);
        keyboard.default.set(default);
        let current = keyboard
            .selected
            .get()
            .filter(|index| *index < count)
            .unwrap_or(default);
        let glide = self.stage.glide("palette");
        let mut results = div()
            .id("search-results")
            .max_h(px(340.))
            .overflow_y_scroll()
            .track_scroll(&keyboard.scroll)
            .flex()
            .flex_col()
            .pt(px(7.))
            .pb(px(8.))
            .pl(px(11.))
            .pr(px(22.))
            .child(glide.indicator(t.hover, 8.));
        // The highlight is the first child; rows and captions follow.
        let mut children = 1;
        // Every row shares one shape: a 20 px glyph, a label, a quiet detail.
        let row = |id: ElementId| {
            div()
                .id(id)
                .h(px(36.))
                .flex_shrink_0()
                .pl(px(6.))
                .pr(px(5.))
                .rounded(px(8.))
                .flex()
                .items_center()
                .gap(px(11.))
                .text_size(px(13.))
                .cursor_pointer()
                // Squash on press; the shared highlight is the hover state.
                .child(
                    crate::motion::tactile(t.border.opacity(0.))
                        .press(t.fill)
                        .radius(8.),
                )
        };
        let action = |action: fn(&mut Self, &mut Window, &mut Context<Self>)| -> MenuRowAction {
            Rc::new(action)
        };
        let detail = |text: String| {
            div()
                .flex_shrink_0()
                .text_size(px(12.))
                .text_color(t.muted)
                .child(text)
        };
        for section in sections {
            if let Some(caption) = section.caption {
                results = results.child(
                    div()
                        .h(px(32.))
                        .flex_shrink_0()
                        .px(px(5.))
                        .flex()
                        .items_end()
                        .pb(px(5.))
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(t.muted)
                        .child(caption),
                );
                children += 1;
            }
            for item in section.rows {
                let (element, activate): (Stateful<Div>, Option<MenuRowAction>) = match item.row {
                    PaletteRow::NewNote => (
                        row("palette-new-note".into())
                            .child(icon("plus-small", 16., t.secondary).mr(px(4.)))
                            .child(div().flex_1().child("New note"))
                            .child(keycaps(&["⇧", "⌘", "N"], t)),
                        Some(action(|this, window, cx| {
                            this.search_open = false;
                            this.new_note(window, cx);
                        })),
                    ),
                    PaletteRow::SearchAll { count } => (
                        row("search-all-results".into())
                            .child(icon("search", 16., t.secondary).mr(px(4.)))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .child(format!("Search {query}")),
                            )
                            .child(detail(format!(
                                "{count} {}",
                                if count == 1 { "result" } else { "results" }
                            ))),
                        Some(action(|this, window, cx| {
                            this.open_search_results(cx);
                            this.focus.focus(window);
                        })),
                    ),
                    PaletteRow::Folder(index) => {
                        let folder = &folders[index];
                        let id = folder.id;
                        (
                            row(("palette-folder", id).into())
                                .child(icon("folder", 16., t.secondary).mr(px(4.)))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .child(folder.name.clone()),
                                )
                                .child(detail(format!(
                                    "{} {}",
                                    folder.notes,
                                    if folder.notes == 1 { "note" } else { "notes" }
                                ))),
                            Some(Rc::new(
                                move |this: &mut Self,
                                      window: &mut Window,
                                      cx: &mut Context<Self>| {
                                    this.folders_open = true;
                                    this.navigate(Screen::Folder(id), cx);
                                    this.focus.focus(window);
                                },
                            ) as MenuRowAction),
                        )
                    }
                    PaletteRow::Note(id) => {
                        let Some(note) = self.workspace.note(id) else {
                            continue;
                        };
                        let title = if note.title.trim().is_empty() {
                            "New note".to_string()
                        } else {
                            note.title.clone()
                        };
                        let highlights = crate::note::transcript_match_ranges(&title, query)
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
                        let date =
                            chrono::NaiveDate::parse_from_str(&note.timeline_date(), "%Y-%m-%d")
                                .map(|date| date.format("%b %-d").to_string())
                                .unwrap_or_else(|_| note.date.clone());
                        (
                            row(("search-result", id).into())
                                .child(icon("document", 20., t.secondary))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .child(StyledText::new(title).with_highlights(highlights)),
                                )
                                .child(detail(date)),
                            Some(Rc::new(
                                move |this: &mut Self,
                                      window: &mut Window,
                                      cx: &mut Context<Self>| {
                                    this.open_note(id, cx);
                                    this.focus.focus(window);
                                },
                            ) as MenuRowAction),
                        )
                    }
                    PaletteRow::Page(screen, label) => (
                        row(label.into())
                            .child(icon(screen_icon(screen), 16., t.secondary).mr(px(4.)))
                            .child(div().flex_1().child(label)),
                        Some(Rc::new(
                            move |this: &mut Self, window: &mut Window, cx: &mut Context<Self>| {
                                this.navigate(screen, cx);
                                this.focus.focus(window);
                            },
                        ) as MenuRowAction),
                    ),
                    PaletteRow::Empty => (
                        div()
                            .id("palette-empty")
                            .h(px(36.))
                            .px(px(8.))
                            .flex()
                            .items_center()
                            .text_size(px(13.))
                            .text_color(t.muted)
                            .child("No matching folders or meetings"),
                        None,
                    ),
                };
                let element = match activate {
                    Some(activate) => {
                        let index = {
                            let mut rows = keyboard.rows.borrow_mut();
                            rows.push(activate.clone());
                            rows.len() - 1
                        };
                        let selected = index == current;
                        if selected && keyboard.reveal.take() {
                            keyboard.scroll.scroll_to_item(children);
                        }
                        element
                            .when(selected, |row| row.child(glide.probe(index)))
                            .on_mouse_move(cx.listener(move |this, _, _, cx| {
                                if this.palette.selected.get() != Some(index) {
                                    this.palette.selected.set(Some(index));
                                    cx.notify();
                                }
                            }))
                            .on_click(
                                cx.listener(move |this, _, window, cx| activate(this, window, cx)),
                            )
                    }
                    None => element,
                };
                results = results.child(element);
                children += 1;
            }
        }
        let pose = self.search_pose;
        div()
            .absolute()
            .inset_0()
            .bg(scrim(Scrim::Palette, pose.fade))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.search_open = false;
                    this.focus.focus(window);
                    cx.notify();
                }),
            )
            // The palette drops out of the top of the window like a droplet.
            .child(crate::motion::pop(
                div()
                    .id("search-dialog")
                    .absolute()
                    .top(relative(0.25))
                    .left(relative(0.5))
                    .ml(px(-240.))
                    .w(px(480.))
                    .max_w(relative(0.9))
                    .rounded(px(12.))
                    .overflow_hidden()
                    .bg(if self.workspace.settings.dark_mode {
                        rgb(0x343434).into()
                    } else {
                        t.card
                    })
                    .border_1()
                    .border_color(t.border)
                    .shadow_lg()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .h(px(48.))
                            .px(px(16.))
                            .pt(px(1.))
                            .flex()
                            .items_center()
                            .text_size(px(13.))
                            .child(div().flex_1().min_w_0().child(self.search.clone())),
                    )
                    .child(separator(t))
                    .child(results),
                crate::motion::Pop::menu((0.5, 0.)),
                pose,
            ))
    }

    pub fn render_menu(
        &self,
        pose: crate::motion::Pose,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use crate::motion::{GOOEY, Pop, pop};
        let t = self.theme();
        let menu = self.visible_menu.clone().or(self.menu.clone()).unwrap();
        self.menu_keyboard.rows.borrow_mut().clear();
        *self.menu_keyboard.menu.borrow_mut() = Some(menu.clone());
        let mut content = div()
            .flex()
            .flex_col()
            .p(px(6.))
            .rounded(px(12.))
            .bg(t.card)
            .border_1()
            .border_color(t.border)
            .shadow_lg()
            // The row highlight follows the pointer and arrow keys as one
            // shape that stretches between rows.
            .child(self.stage.glide("menu").indicator(t.hover, 6.));
        let mut overlay = div().absolute().inset_0().on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, w, cx| {
                this.menu = None;
                this.focus.focus(w);
                cx.notify();
            }),
        );
        match menu {
            Menu::NoteDetails | Menu::CalendarEvent => {
                let anchors = self.note_metadata_anchors(window);
                // The event picker unfolds from the details card and folds
                // back into it; the card itself stays put in between.
                let picker = self.stage.presence(
                    ("event-picker", 0),
                    menu == Menu::CalendarEvent && self.menu.is_some(),
                    GOOEY,
                );
                return overlay
                    .child(
                        div()
                            .absolute()
                            .top(px(130.))
                            .left(px(anchors.details_left))
                            .child(
                                div()
                                    .relative()
                                    .child(pop(
                                        self.render_note_details(cx)
                                            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                                cx.stop_propagation()
                                            }),
                                        Pop::menu((0.15, 0.)),
                                        pose,
                                    ))
                                    .when(picker.visible, |d| {
                                        d.child(div().absolute().left(px(8.)).top(px(50.)).child(
                                            pop(
                                                self.render_event_picker(cx).on_mouse_down(
                                                    MouseButton::Left,
                                                    |_, _, cx| cx.stop_propagation(),
                                                ),
                                                Pop::menu((0.1, 0.)),
                                                picker,
                                            ),
                                        ))
                                    }),
                            ),
                    )
                    .into_any_element();
            }
            Menu::RecordingSettings | Menu::TranscriptionLanguage => {
                let language = menu == Menu::TranscriptionLanguage;
                let viewport_width = f32::from(window.viewport_size().width);
                let enhanced = self
                    .current_note
                    .and_then(|id| self.workspace.note(id))
                    .is_some_and(|note| {
                        !note.enhanced.trim().is_empty() || self.generation.is_running_for(note.id)
                    });
                let panel_width =
                    (viewport_width.min(if enhanced { 682. } else { 672. }) + 8.).min(682.);
                let popup_width = if language { 220. } else { 272. };
                let left = recording_popup_left(viewport_width, panel_width, popup_width, language);
                let panel = if language {
                    self.transcription_language_settings(cx)
                } else {
                    self.recording_settings(cx)
                };
                // Both grow upwards out of the transcript footer controls.
                return overlay
                    .child(div().absolute().left(px(left)).bottom(px(83.)).child(pop(
                        panel.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()),
                        Pop::menu((0.8, 1.)),
                        pose,
                    )))
                    .into_any_element();
            }
            Menu::Workspace => {
                let connected = crate::server::enabled();
                let session = crate::account::session().filter(|_| connected);
                content = content
                    .w(px(256.))
                    // 4 px under the last row.
                    .pb(px(3.))
                    .child(
                        div()
                            .px(px(6.))
                            .pt(px(6.))
                            .pb(px(8.))
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .child(
                                div()
                                    .size(px(28.))
                                    .flex_shrink_0()
                                    .rounded(px(7.))
                                    .border_1()
                                    .border_color(t.hairline)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon("prompt", 15., t.text)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(1.))
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .font_weight(FontWeight::MEDIUM)
                                            .truncate()
                                            .child(self.workspace_name()),
                                    )
                                    .child(small(
                                        match &session {
                                            Some(session) => session.user.email.clone(),
                                            None if connected => "Sign in to your server".into(),
                                            None => "Local demo on this Mac".into(),
                                        },
                                        t,
                                    )),
                            ),
                    )
                    .child(separator(t).my(px(4.)));
                for (label, symbol, screen) in [
                    ("Favorites", "star", Screen::Favorites),
                    ("Trash", "trash", Screen::Trash),
                ] {
                    content =
                        content.child(self.menu_row(label, symbol, cx, move |this, _, cx| {
                            this.navigate(screen, cx)
                        }));
                }
                content = content
                    .child(
                        self.menu_row("New folder", "folder-plus", cx, |this, w, cx| {
                            this.folder_input.update(cx, |input, cx| {
                                input.set_content("", cx);
                                input.set_placeholder("Name", cx);
                            });
                            this.menu = Some(Menu::NewFolder);
                            this.folder_input.read(cx).focus_handle.focus(w);
                            cx.notify();
                        }),
                    )
                    .child(separator(t).my(px(4.)))
                    .child(
                        self.menu_row("Keyboard shortcuts", "keyboard", cx, |this, _, cx| {
                            this.menu = Some(Menu::Shortcuts);
                            cx.notify();
                        }),
                    )
                    .child(
                        self.menu_row("Settings", "settings", cx, |this, _, cx| {
                            this.navigate(Screen::Settings, cx)
                        })
                        .child(div().flex_1())
                        .child(keycaps(&["⌘", ","], t)),
                    );
                overlay = overlay.child(div().absolute().bottom(px(43.)).left(px(8.)).child(pop(
                    content.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()),
                    Pop::menu((0.12, 1.)),
                    pose,
                )));
            }
            Menu::NoteRow => {
                let Some(row) = self.row_menu else {
                    return overlay.into_any_element();
                };
                // 220 px of surface inside the hairline, rows on a 28 px pitch.
                content = content.w(px(222.)).p(px(4.));
                for action in RowAction::ALL {
                    let note = row.note;
                    let run =
                        move |this: &mut Self, window: &mut Window, cx: &mut Context<Self>| {
                            this.run_row_action(action, note, window, cx)
                        };
                    // Like its counterpart, this menu is labels only.
                    content = content.child(
                        if action.destructive() {
                            self.danger_row(action.label(), "", cx, run)
                        } else {
                            self.menu_row(action.label(), "", cx, run)
                        }
                        .px(px(8.)),
                    );
                }
                let origin = popover_origin(
                    row.at,
                    size(px(222.), px(4. * 28. + 10.)),
                    window.viewport_size(),
                );
                // It opens under the pointer, like every native context menu.
                overlay = overlay.child(div().absolute().left(origin.x).top(origin.y).child(pop(
                    content.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()),
                    Pop::menu((0.05, 0.)),
                    pose,
                )));
            }
            Menu::Note | Menu::Share | Menu::MoveTo | Menu::Templates => {
                content = content.w(px(if menu == Menu::Note || menu == Menu::Templates {
                    220.
                } else {
                    230.
                }));
                if menu == Menu::Share {
                    let account = crate::account::session();
                    let owner = account
                        .as_ref()
                        .map(|session| session.user.name.clone())
                        .filter(|name| !name.trim().is_empty())
                        .unwrap_or_else(|| "Me".into());
                    let initial = owner
                        .chars()
                        .next()
                        .map(|character| character.to_uppercase().collect::<String>())
                        .unwrap_or_else(|| "M".into());
                    let connected = crate::server::enabled();
                    content = content
                        .w(px(400.))
                        .child(
                            div()
                                .px(px(10.))
                                .py(px(8.))
                                .text_size(px(12.))
                                .text_color(t.muted)
                                .child("People with access"),
                        )
                        .child(
                            div()
                                .px(px(10.))
                                .py(px(7.))
                                .flex()
                                .items_center()
                                .gap(px(10.))
                                .child(
                                    div()
                                        .size(px(28.))
                                        .rounded_full()
                                        .bg(t.fill)
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .text_size(px(12.))
                                        .child(initial),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .flex()
                                        .flex_col()
                                        .gap(px(2.))
                                        .text_size(px(13.))
                                        .child(format!("{owner} (you)"))
                                        .child(small(
                                            account
                                                .as_ref()
                                                .map(|account| account.user.email.clone())
                                                .unwrap_or_else(|| "Local demo on this Mac".into()),
                                            t,
                                        )),
                                )
                                .child(small("Owner", t)),
                        )
                        .child(separator(t).my(px(6.)))
                        // One footer row: who the link reaches, and the action.
                        .child(
                            div()
                                .px(px(10.))
                                .py(px(6.))
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .child(icon("globe", 15., t.secondary))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_size(px(13.))
                                        .line_height(px(18.))
                                        .child(
                                            "Anyone signed in to this server who has the link can read it.",
                                        ),
                                )
                                .child(if connected {
                                    pill("share-copy-link", "Copy link", "", t).on_click(
                                        cx.listener(|this, _, _, cx| {
                                            this.menu = None;
                                            if let Some(id) = this.current_note {
                                                this.server_action(
                                                    id,
                                                    crate::server::Action::Share,
                                                    cx,
                                                );
                                            }
                                        }),
                                    )
                                } else {
                                    // Sharing needs a server; say so instead
                                    // of offering a button that cannot work.
                                    pill("share-copy-link", "Copy link", "", t)
                                        .opacity(DISABLED)
                                        .cursor(CursorStyle::Arrow)
                                        .tooltip(move |_, cx| {
                                            cx.new(|_| crate::ui::ToolTip::new(
                                                "Connect to your server to share a note",
                                                t,
                                            ))
                                            .into()
                                        })
                                }),
                        )
                        .child(
                            div()
                                .px(px(10.))
                                .pt(px(2.))
                                .pb(px(6.))
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .text_color(t.muted)
                                .child("Copying creates a read-only snapshot. Your transcript stays private."),
                        );
                    if connected {
                        content = content.child(separator(t).my(px(4.))).child(self.menu_row(
                            "Revoke shared links",
                            "lock",
                            cx,
                            |this, _, cx| {
                                this.menu = None;
                                if let Some(id) = this.current_note {
                                    this.server_action(id, crate::server::Action::RevokeShares, cx);
                                }
                            },
                        ));
                    }
                } else if menu == Menu::Note {
                    if self
                        .current_note
                        .and_then(|id| self.workspace.note(id))
                        .is_some_and(|note| {
                            !note.body.trim().is_empty() && note.enhanced.trim().is_empty()
                        })
                    {
                        content = content.child(self.menu_row(
                            "Enhance notes",
                            "sparkles",
                            cx,
                            |this, _, cx| {
                                if let Some(id) = this.current_note {
                                    this.menu = None;
                                    this.enhance_note_preview(id, cx);
                                }
                            },
                        ));
                    }
                    content = content
                        .child(
                            self.menu_row(
                                if self.active_tab == crate::app::NoteTab::Enhanced {
                                    "Copy enhanced notes"
                                } else {
                                    "Copy notes"
                                },
                                "copy",
                                cx,
                                |this, _, cx| this.copy_current(cx),
                            )
                            .h(px(28.)),
                        )
                        .child(
                            self.menu_row("Export Markdown", "file", cx, |this, _, cx| {
                                this.export_note(cx)
                            })
                            .h(px(28.)),
                        )
                        .child(
                            self.menu_row("Move to folder", "folder", cx, |this, _, cx| {
                                this.search.update(cx, |input, cx| {
                                    input.set_content("", cx);
                                    input.set_placeholder("Search", cx);
                                });
                                this.menu = Some(Menu::MoveTo);
                                cx.notify();
                            })
                            .h(px(28.)),
                        )
                        .child(separator(t))
                        .child(
                            self.danger_row("Move to trash", "trash", cx, |this, _, cx| {
                                if let Some(id) = this.current_note {
                                    this.trash_note(id, cx);
                                }
                            }),
                        );
                } else if menu == Menu::MoveTo {
                    let current_folder = self
                        .menu_note()
                        .and_then(|id| self.workspace.note(id))
                        .and_then(|note| note.folder_id);
                    content = content
                        .w(px(288.))
                        .p(px(0.))
                        .gap(px(0.))
                        .overflow_hidden()
                        .text_size(px(13.))
                        .text_color(t.muted)
                        .child(
                            div()
                                .h(px(38.))
                                .px(px(12.))
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .text_size(px(13.))
                                .child(div().flex_1().min_w_0().child(self.search.clone()))
                                .child(icon("search", 14., t.muted)),
                        )
                        .child(separator(t));
                    content = content.child(
                        self.menu_row("My notes", "lock", cx, |this, _, cx| {
                            if let Some(note) =
                                this.menu_note().and_then(|id| this.workspace.note_mut(id))
                            {
                                note.folder_id = None;
                            }
                            this.menu = None;
                            this.persist(cx);
                        })
                        .h(px(44.))
                        .px(px(14.))
                        .gap(px(13.))
                        .child(div().flex_1())
                        .when(
                            self.menu_note().is_some() && current_folder.is_none(),
                            |d| d.child(icon("check", 14., t.accent)),
                        ),
                    );
                    let query = self.search.read(cx).content().trim().to_string();
                    let folded_query = query.to_lowercase();
                    for folder in self.workspace.folders.iter().filter(|folder| {
                        folded_query.is_empty()
                            || folder.name.to_lowercase().contains(&folded_query)
                    }) {
                        let id = folder.id;
                        content = content.child(
                            self.menu_row(folder.name.clone(), "folder", cx, move |this, _, cx| {
                                if let Some(note) =
                                    this.menu_note().and_then(|id| this.workspace.note_mut(id))
                                {
                                    note.folder_id = Some(id);
                                }
                                this.menu = None;
                                this.persist(cx);
                            })
                            .px(px(14.))
                            .gap(px(13.))
                            .child(div().flex_1())
                            .child(small(
                                self.workspace
                                    .notes
                                    .iter()
                                    .filter(|note| note.folder_id == Some(id) && !note.trashed)
                                    .count()
                                    .to_string(),
                                t,
                            ))
                            .when(current_folder == Some(id), |d| {
                                d.child(icon("check", 14., t.accent))
                            }),
                        );
                    }
                    let create_label = if query.is_empty() {
                        "New folder".into()
                    } else {
                        format!("Create \"{query}\"")
                    };
                    content = content.child(separator(t)).child(
                        self.menu_row(create_label, "folder-plus", cx, move |this, window, cx| {
                            this.folder_input.update(cx, |input, cx| {
                                input.set_content(query.clone(), cx);
                                input.set_placeholder("Name", cx);
                            });
                            this.menu = Some(Menu::NewFolder);
                            this.folder_input.read(cx).focus_handle.focus(window);
                            cx.notify();
                        })
                        .h(px(40.))
                        .px(px(14.))
                        .gap(px(13.))
                        .text_color(t.accent),
                    );
                } else {
                    let regenerate_menu = self.menu.clone();
                    let regenerate_epoch = self.menu_epoch;
                    let (regenerate_index, regenerate_action) =
                        self.menu_keyboard.register(|this, _, cx| {
                            this.menu = None;
                            if let Some(id) = this.current_note {
                                this.enhance_note_preview(id, cx);
                            }
                        });
                    content = content.child(
                        div()
                            .id("enhanced-menu-selected")
                            .h(px(32.))
                            .px(px(8.))
                            .rounded(px(7.))
                            .bg(t.hover)
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_size(px(13.))
                            .child(icon("sparkles", 14., t.muted))
                            .child(div().flex_1().child("Enhanced notes"))
                            .child(
                                tool_tip(
                                    "regenerate-enhanced-notes",
                                    "chat-retry",
                                    "Regenerate notes",
                                    t,
                                )
                                .size(px(26.))
                                .when(self.menu_keyboard.selected == Some(regenerate_index), |d| {
                                    d.bg(t.border)
                                })
                                .on_mouse_move(cx.listener(move |this, _, _, cx| {
                                    if this.menu_keyboard.selected != Some(regenerate_index) {
                                        this.menu_keyboard.selected = Some(regenerate_index);
                                        cx.notify();
                                    }
                                }))
                                .on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        cx.stop_propagation();
                                        if this.menu.is_some()
                                            && this.menu == regenerate_menu
                                            && this.menu_epoch == regenerate_epoch
                                        {
                                            regenerate_action(this, window, cx);
                                        }
                                    },
                                )),
                            )
                            .child(icon("check", 14., t.accent))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.active_tab = crate::app::NoteTab::Enhanced;
                                this.menu = None;
                                cx.notify();
                            })),
                    );
                    // Local outlines are this Mac's stand-in for templates; a
                    // connected server writes the notes itself.
                    if !crate::server::enabled() {
                        content = content.child(
                            div()
                                .px(px(8.))
                                .pt(px(10.))
                                .pb(px(4.))
                                .text_color(t.muted)
                                .text_size(px(12.))
                                .child("Local outlines"),
                        );
                        for label in ["Meeting notes", "Action items", "Quick overview"] {
                            content = content.child(self.menu_row(
                                label,
                                "sparkles",
                                cx,
                                move |this, _, cx| {
                                    if let Some(note) =
                                        this.current_note.and_then(|id| this.workspace.note_mut(id))
                                    {
                                        note.enhanced = match label {
                                            "Action items" => format!(
                                                "## Action items\n\n{}",
                                                note.body
                                                    .lines()
                                                    .filter(|l| (l.trim().starts_with("- ")
                                                        || l.trim().starts_with("• "))
                                                        || l.contains(':'))
                                                    .collect::<Vec<_>>()
                                                    .join("\n")
                                            ),
                                            "Quick overview" => format!(
                                                "## Quick overview\n\n{}",
                                                note.body
                                                    .lines()
                                                    .filter(|l| !l.trim().is_empty())
                                                    .take(5)
                                                    .collect::<Vec<_>>()
                                                    .join("\n\n")
                                            ),
                                            _ => crate::note::local_outline(&note.body),
                                        };
                                    }
                                    this.menu = None;
                                    this.active_tab = crate::app::NoteTab::Enhanced;
                                    if this.persist(cx) {
                                        this.show_toast(
                                            "Local outline updated from your notes",
                                            cx,
                                        );
                                    }
                                },
                            ));
                        }
                    }
                }
                overlay = overlay.child(
                    div()
                        .absolute()
                        .when(menu == Menu::Templates, |d| {
                            d.top(px(130.)).left(relative(0.5)).ml(px(-278.))
                        })
                        .when(menu == Menu::MoveTo, |d| match self.row_menu {
                            // Opened from a list row: stay where that menu was.
                            Some(row) => {
                                let origin = popover_origin(
                                    row.at,
                                    size(px(288.), px(260.)),
                                    window.viewport_size(),
                                );
                                d.left(origin.x).top(origin.y)
                            }
                            None => {
                                let anchors = self.note_metadata_anchors(window);
                                d.top(px(130.)).left(px(anchors.folder_left))
                            }
                        })
                        .when(menu == Menu::Share, |d| d.top(px(40.)).right(px(8.)))
                        .when(menu == Menu::Note, |d| {
                            let has_share = self
                                .current_note
                                .and_then(|id| self.workspace.note(id))
                                .is_some_and(|note| !note.enhanced.trim().is_empty());
                            d.top(px(40.)).right(px(if has_share { 120. } else { 8. }))
                        })
                        .child(pop(
                            content
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()),
                            // Each menu grows out of the control that opened it.
                            Pop::menu(match menu {
                                Menu::Note => (0.92, 0.),
                                Menu::Share => (0.8, 0.),
                                _ => (0.15, 0.),
                            }),
                            pose,
                        )),
                );
            }
            _ => {
                let (title,description)=match &menu {
                    Menu::NewFolder=>("Create folder".to_string(),"Name".to_string()),
                    Menu::Voice if crate::server::enabled()=>("A little less typing".into(),"Voice typing is not available yet. You can record a meeting, then stop recording to transcribe it and generate notes.".into()),
                    Menu::Voice=>("A little less typing".into(),"Voice typing is not available yet. Meeting recording saves separate microphone and computer-audio files on this Mac.".into()),
                    Menu::RecordingHelp if crate::server::enabled()=>("Recording a meeting".into(),"Start captures your microphone and computer audio separately. Stop saves the recording and transcribes it without generating notes. Choose Generate notes when you are ready. Resume keeps the same transcript; after you stop again, Regenerate notes includes the new conversation.".into()),
                    Menu::RecordingHelp=>("Local audio recording".into(),"Start records your microphone and computer audio separately. Stop saves the audio on this Mac. Transcription and generated notes are available when you connect to your server.".into()),
                    Menu::Shortcuts=>("Keyboard shortcuts".into(),"Make yourself at home.".into()),
                    _=>("Cereal".into(),"Native notes, thoughtfully simple.".into()),
                };
                content = content
                    .w(px(440.))
                    .p(px(24.))
                    .gap(px(16.))
                    .when(menu == Menu::NewFolder, |d| {
                        d.w(px(512.))
                            .max_w(relative(0.9))
                            .p(px(16.))
                            .rounded(px(16.))
                            .bg(t.bg)
                    })
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .items_center()
                            .child(if menu == Menu::NewFolder {
                                div()
                                    .font_family(FONT)
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(title)
                            } else {
                                heading(title, t)
                            })
                            .child(tool("close-dialog", "close", t).on_click(cx.listener(
                                |this, _, w, cx| {
                                    this.menu = None;
                                    this.focus.focus(w);
                                    cx.notify();
                                },
                            ))),
                    )
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(21.))
                            .text_color(t.muted)
                            .when(menu == Menu::NewFolder, |d| {
                                d.text_size(px(13.)).line_height(px(18.)).text_color(t.text)
                            })
                            .child(description),
                    );
                if menu == Menu::NewFolder {
                    let can_create = !self.folder_input.read(cx).content().trim().is_empty();
                    let focused = self.folder_input.read(cx).focus_handle.is_focused(window);
                    content =
                        content
                            .child(
                                div()
                                    .h(px(36.))
                                    .px(px(4.))
                                    .bg(t.card)
                                    .border_1()
                                    .border_color(if focused { t.accent } else { t.border })
                                    .rounded(px(6.))
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .size(px(26.))
                                            .rounded(px(5.))
                                            .bg(t.fill)
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(icon("folder", 16., t.muted)),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .text_size(px(13.))
                                            .child(self.folder_input.clone()),
                                    ),
                            )
                            .child(
                                div()
                                    .mt(px(8.))
                                    .flex()
                                    .items_center()
                                    .justify_end()
                                    .gap(px(8.))
                                    .child(
                                        pill("cancel-new-folder", "Cancel", "", t)
                                            .h(px(28.))
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.menu = None;
                                                this.focus.focus(window);
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        div()
                                            .id("create-folder")
                                            .h(px(28.))
                                            .px(px(14.))
                                            .rounded_full()
                                            .flex()
                                            .items_center()
                                            .text_size(px(13.))
                                            .bg(if can_create { t.text } else { t.fill })
                                            .text_color(if can_create { t.bg } else { t.muted })
                                            .when(can_create, |d| {
                                                crate::motion::dims(
                                                    d.cursor_pointer(),
                                                    crate::motion::reveal_key("create-folder"),
                                                    0.14,
                                                )
                                                .reach().on_click(cx.listener(|this, _, _, cx| {
                                                    this.create_folder(cx)
                                                }))
                                            })
                                            .child("Create"),
                                    ),
                            );
                } else if menu == Menu::Shortcuts {
                    for (label, key) in [
                        ("New note", "⌘ N"),
                        ("Search notes", "⌘ K / ⌘ F"),
                        ("Go home", "⌘ 1"),
                        ("Toggle sidebar", "⌘ B"),
                        ("Save notes", "⌘ S"),
                        ("Dismiss / back", "Esc"),
                        ("Undo / redo", "⌘ Z / ⇧ ⌘ Z"),
                    ] {
                        content =
                            content.child(
                                div().flex().justify_between().child(label).child(
                                    div().font_family("Inter").text_color(t.muted).child(key),
                                ),
                            );
                    }
                } else {
                    content = content.child(pill("dialog-done", "Got it", "check", t).on_click(
                        cx.listener(|this, _, w, cx| {
                            this.menu = None;
                            this.focus.focus(w);
                            cx.notify();
                        }),
                    ));
                }
                overlay = overlay
                    .bg(scrim(
                        if menu == Menu::NewFolder {
                            Scrim::Form
                        } else {
                            Scrim::Dialog
                        },
                        pose.fade,
                    ))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(pop(
                        content.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()),
                        Pop::DIALOG,
                        pose,
                    ));
            }
        }
        overlay.into_any_element()
    }

    fn menu_row(
        &self,
        label: impl Into<SharedString>,
        symbol: &str,
        cx: &mut Context<Self>,
        action: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> Stateful<Div> {
        self.menu_item(label, symbol, None, cx, action)
    }

    /// A destructive menu row: label and icon both carry the danger colour.
    fn danger_row(
        &self,
        label: impl Into<SharedString>,
        symbol: &str,
        cx: &mut Context<Self>,
        action: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> Stateful<Div> {
        let danger = self.theme().danger;
        self.menu_item(label, symbol, Some(danger), cx, action)
    }

    fn menu_item(
        &self,
        label: impl Into<SharedString>,
        symbol: &str,
        tint: Option<Hsla>,
        cx: &mut Context<Self>,
        action: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> Stateful<Div> {
        let t = self.theme();
        let label = label.into();
        let (index, action) = self.menu_keyboard.register(action);
        let open_menu = self.visible_menu.clone().or(self.menu.clone());
        let menu_epoch = self.menu_epoch;
        div()
            .id(label.clone())
            .h(px(28.))
            .px(px(6.))
            .flex()
            .items_center()
            .gap(px(8.))
            .text_size(px(13.))
            .rounded(px(6.))
            .cursor_pointer()
            .when_some(tint, |d, tint| d.text_color(tint))
            // Squash on press; the shared highlight below is the hover state.
            .child(crate::motion::tactile(t.border.opacity(0.)).press(t.fill))
            .when(self.menu_keyboard.selected == Some(index), |d| {
                d.child(self.stage.glide("menu").probe(index))
            })
            .on_mouse_move(cx.listener(move |this, _, _, cx| {
                if this.menu_keyboard.selected != Some(index) {
                    this.menu_keyboard.selected = Some(index);
                    cx.notify();
                }
            }))
            .when(!symbol.is_empty(), |row| {
                row.child(icon(
                    symbol,
                    16.,
                    tint.unwrap_or(
                        if symbol == "folder-plus" && self.menu == Some(Menu::MoveTo) {
                            t.accent
                        } else {
                            t.secondary
                        },
                    ),
                ))
            })
            .child(label)
            .on_click(cx.listener(move |this, _, window, cx| {
                if this.menu.is_some() && this.menu == open_menu && this.menu_epoch == menu_epoch {
                    action(this, window, cx);
                }
            }))
    }

    fn export_note(&mut self, cx: &mut Context<Self>) {
        let Some(note) = self.current_note.and_then(|id| self.workspace.note(id)) else {
            return;
        };
        let title = if note.title.trim().is_empty() {
            "Untitled note"
        } else {
            &note.title
        };
        let name: String = title
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == ' ' || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .take(90)
            .collect();
        let home = std::env::var_os("HOME").unwrap_or_default();
        let dir = std::path::PathBuf::from(home).join("Downloads/Cereal");
        let path = dir.join(format!(
            "{name}-{}.md",
            chrono::Local::now().format("%Y%m%d-%H%M%S-%3f")
        ));
        let body = format!(
            "# {title}\n\n{} · {}\n\n{}\n\n---\n\n{}\n",
            note.date, note.time, note.body, note.enhanced
        );
        let result = std::fs::create_dir_all(dir).and_then(|_| std::fs::write(&path, body));
        self.menu = None;
        match result {
            Ok(()) => self.show_toast("Exported to Downloads/Cereal", cx),
            Err(e) => self.show_toast(&format!("Export failed: {e}"), cx),
        }
    }
}

/// A folder as the palette lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaletteFolder {
    pub id: u64,
    pub name: String,
    pub notes: usize,
}

/// One row of the command palette.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteRow {
    NewNote,
    SearchAll {
        count: usize,
    },
    /// Index into the folders passed to [`palette_sections`].
    Folder(usize),
    Note(u64),
    Page(Screen, &'static str),
    Empty,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaletteItem {
    pub row: PaletteRow,
    /// The row Return acts on, drawn with the selection fill.
    pub selected: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaletteSection {
    pub caption: Option<&'static str>,
    pub rows: Vec<PaletteItem>,
}

const PALETTE_FOLDERS: usize = 5;
const PALETTE_NOTES: usize = 8;
const PALETTE_PAGES: [(Screen, &str); 6] = [
    (Screen::Home, "Home"),
    (Screen::MyNotes, "My notes"),
    (Screen::Shared, "Shared with me"),
    (Screen::Chat, "Chat"),
    (Screen::Settings, "Settings"),
    (Screen::Trash, "Trash"),
];

/// What the command palette shows for `query`. `notes` are the ids of the
/// notes already matching the query, best first.
///
/// Sections exist only when there is something real to put in them: there are
/// no companies or people here because the workspace has no such records.
/// The selected row is always the one Return acts on — the full-results row
/// while typing, otherwise "New note".
pub fn palette_sections(
    query: &str,
    folders: &[PaletteFolder],
    notes: &[u64],
) -> Vec<PaletteSection> {
    let query = query.trim();
    let folded = query.to_lowercase();
    let item = |row| PaletteItem {
        row,
        selected: false,
    };
    let mut sections = Vec::new();
    let mut actions = Vec::new();
    if query.is_empty() {
        actions.push(PaletteItem {
            row: PaletteRow::NewNote,
            selected: true,
        });
    } else {
        actions.push(PaletteItem {
            row: PaletteRow::SearchAll { count: notes.len() },
            selected: true,
        });
    }
    sections.push(PaletteSection {
        caption: None,
        rows: actions,
    });
    let matching_folders: Vec<_> = folders
        .iter()
        .enumerate()
        .filter(|(_, folder)| folded.is_empty() || folder.name.to_lowercase().contains(&folded))
        .take(PALETTE_FOLDERS)
        .map(|(index, _)| item(PaletteRow::Folder(index)))
        .collect();
    if !matching_folders.is_empty() {
        sections.push(PaletteSection {
            caption: Some("Folders"),
            rows: matching_folders,
        });
    }
    let meetings: Vec<_> = notes
        .iter()
        .take(PALETTE_NOTES)
        .map(|id| item(PaletteRow::Note(*id)))
        .collect();
    if !meetings.is_empty() {
        sections.push(PaletteSection {
            caption: Some("Meetings"),
            rows: meetings,
        });
    }
    // Pages are offered only once the query names one.
    let pages: Vec<_> = PALETTE_PAGES
        .iter()
        .filter(|(_, label)| !folded.is_empty() && label.to_lowercase().contains(&folded))
        .map(|(screen, label)| item(PaletteRow::Page(*screen, label)))
        .collect();
    if !pages.is_empty() {
        sections.push(PaletteSection {
            caption: Some("Go to"),
            rows: pages,
        });
    }
    if sections.len() == 1 && !query.is_empty() && notes.is_empty() {
        sections.push(PaletteSection {
            caption: None,
            rows: vec![item(PaletteRow::Empty)],
        });
    }
    sections
}

/// What the header back pill shows: the glyph of the page it returns to, or
/// plain "Back" on space pages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackLabel {
    Icon(&'static str),
    Text,
}

/// The one action a page's header offers, if any.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderAction {
    NewNote,
    NewChat,
}

/// Only Home creates notes from the header; Chat starts a new chat; every
/// other page keeps its header clear.
pub fn header_action(screen: Screen) -> Option<HeaderAction> {
    match screen {
        Screen::Home => Some(HeaderAction::NewNote),
        Screen::Chat => Some(HeaderAction::NewChat),
        _ => None,
    }
}

pub fn screen_icon(screen: Screen) -> &'static str {
    match screen {
        Screen::Shared => "people",
        Screen::Link => "link-chain",
        Screen::Chat => "speech-bubble",
        Screen::Connect => "connect-grid",
        Screen::MyNotes => "lock-solid",
        Screen::Folder(_) => "folder",
        Screen::Favorites => "star",
        Screen::Trash => "trash",
        Screen::Settings => "settings",
        Screen::SearchResults => "search",
        Screen::Home | Screen::Login | Screen::Account => "home",
    }
}

/// The back pill for `screen`, given the screen it would return to. Home has
/// none; space pages say "Back".
pub fn back_label(screen: Screen, target: Screen) -> Option<BackLabel> {
    match screen {
        Screen::Home | Screen::Login | Screen::Account => None,
        Screen::MyNotes | Screen::Folder(_) | Screen::Favorites => Some(BackLabel::Text),
        _ => Some(BackLabel::Icon(screen_icon(target))),
    }
}

/// Remember where a navigation came from so the back pill can return there.
/// Reaching Home clears the trail: Home is the root and has no back pill.
pub fn record_navigation(history: &mut Vec<Screen>, from: Screen, to: Screen) {
    if to == Screen::Home {
        history.clear();
        return;
    }
    // A link's page is passed through, not returned to: what it may show
    // depends on the account, and following the link again shows it afresh.
    if from == to || matches!(from, Screen::Login | Screen::Account | Screen::Link) {
        return;
    }
    // Returning to a page already on the trail unwinds to it instead of
    // growing a loop.
    if let Some(index) = history.iter().position(|screen| *screen == to) {
        history.truncate(index);
        return;
    }
    if history.last() != Some(&from) {
        history.push(from);
    }
    if history.len() > 24 {
        history.remove(0);
    }
}

/// Where a list-row menu is anchored and which note it acts on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RowMenu {
    pub note: u64,
    pub at: Point<Pixels>,
}

/// The rows of a note's right-click menu, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowAction {
    CopyLink,
    Share,
    AddToFolder,
    MoveToTrash,
}

impl RowAction {
    pub const ALL: [RowAction; 4] = [
        RowAction::CopyLink,
        RowAction::Share,
        RowAction::AddToFolder,
        RowAction::MoveToTrash,
    ];

    pub fn label(self) -> &'static str {
        match self {
            RowAction::CopyLink => "Copy link",
            RowAction::Share => "Share",
            RowAction::AddToFolder => "Add to folder",
            RowAction::MoveToTrash => "Move to trash",
        }
    }

    pub fn destructive(self) -> bool {
        self == RowAction::MoveToTrash
    }
}

/// Top-left corner for a popover of `size` opened at `pointer`, kept inside
/// the window with a margin so it never opens off-screen.
pub fn popover_origin(
    pointer: Point<Pixels>,
    size: Size<Pixels>,
    viewport: Size<Pixels>,
) -> Point<Pixels> {
    let margin = px(8.);
    let fit =
        |at: Pixels, extent: Pixels, limit: Pixels| at.min(limit - extent - margin).max(margin);
    point(
        fit(pointer.x, size.width, viewport.width),
        fit(pointer.y, size.height, viewport.height),
    )
}

/// The 44×26 outlined capsule at the left of every header but Home's.
pub fn back_pill(id: &'static str, label: BackLabel, t: Theme) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(28.))
        .min_w(px(44.))
        .px(px(if label == BackLabel::Text { 8. } else { 0. }))
        .flex()
        .items_center()
        .justify_center()
        .gap(px(2.))
        .rounded_full()
        .reach()
        .border_1()
        .border_color(t.hairline)
        .text_size(px(13.))
        .text_color(t.secondary)
        .cursor_pointer()
        .child(row_feedback(t, 0.).round())
        .child(icon("chevron-left", 10., t.secondary))
        .child(match label {
            BackLabel::Icon(symbol) => icon(symbol, 15., t.secondary).into_any_element(),
            BackLabel::Text => div().child("Back").into_any_element(),
        })
}

impl Cereal {
    /// The screen the back pill returns to.
    pub fn back_target(&self) -> Screen {
        self.nav_history.last().copied().unwrap_or(Screen::Home)
    }

    pub fn go_back(&mut self, cx: &mut Context<Self>) {
        let target = self.nav_history.pop().unwrap_or(Screen::Home);
        let trail = std::mem::take(&mut self.nav_history);
        self.navigate(target, cx);
        // Stepping back must not record the page just left as somewhere to
        // return to.
        self.nav_history = if target == Screen::Home {
            Vec::new()
        } else {
            trail
        };
    }

    pub fn open_settings(&mut self, tab: crate::screens::SettingsTab, cx: &mut Context<Self>) {
        self.settings_tab = tab;
        self.navigate(Screen::Settings, cx);
    }

    pub fn open_recipes_from_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.current_note.is_none() && self.screen != Screen::Chat {
            self.navigate(Screen::Chat, cx);
        }
        self.open_chat_recipes(window, cx);
    }

    /// The note a note-scoped menu acts on: the row that was right-clicked, or
    /// the open note.
    pub fn menu_note(&self) -> Option<u64> {
        self.row_menu.map(|menu| menu.note).or(self.current_note)
    }

    pub fn open_row_menu(&mut self, note: u64, at: Point<Pixels>, cx: &mut Context<Self>) {
        self.row_menu = Some(RowMenu { note, at });
        self.menu = Some(Menu::NoteRow);
        cx.notify();
    }

    /// Move a note to Trash exactly as the note menu does.
    pub fn trash_note(&mut self, id: u64, cx: &mut Context<Self>) {
        if self.recording.note_id == Some(id) {
            self.finish_recording_preview(cx);
            self.transcript_open = false;
            self.transcript_find_open = false;
        }
        let leave = self.current_note == Some(id);
        if let Some(note) = self.workspace.note_mut(id) {
            note.trashed = true;
        }
        self.menu = None;
        if leave {
            self.navigate(Screen::Home, cx);
        }
        if self.persist(cx) {
            self.show_toast("Note moved to trash", cx);
        }
    }

    pub fn run_row_action(
        &mut self,
        action: RowAction,
        note: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            RowAction::CopyLink => {
                self.menu = None;
                if crate::server::enabled() {
                    self.server_action(note, crate::server::Action::Share, cx);
                } else {
                    self.show_toast("Connect to your server to share a note", cx);
                }
            }
            RowAction::Share => {
                self.row_menu = None;
                self.open_note(note, cx);
                self.focus.focus(window);
                self.menu = Some(Menu::Share);
                cx.notify();
            }
            RowAction::AddToFolder => {
                // The folder picker opens in place, still acting on this row.
                self.search.update(cx, |input, cx| {
                    input.set_content("", cx);
                    input.set_placeholder("Search", cx);
                });
                self.menu = Some(Menu::MoveTo);
                self.search.read(cx).focus_handle.focus(window);
                cx.notify();
            }
            RowAction::MoveToTrash => self.trash_note(note, cx),
        }
    }
}

fn recording_popup_left(
    viewport_width: f32,
    panel_width: f32,
    popup_width: f32,
    language: bool,
) -> f32 {
    let panel_right = (viewport_width + panel_width.min(viewport_width)) / 2.;
    let right_inset = if language { 12. } else { 128. };
    (panel_right - right_inset - popup_width)
        .clamp(12., (viewport_width - popup_width - 12.).max(12.))
}

#[cfg(test)]
mod recording_popup_tests {
    use super::recording_popup_left;

    #[test]
    fn footer_menus_follow_the_transcript_controls_when_resizing() {
        let audio = recording_popup_left(1200., 682., 272., false);
        let language = recording_popup_left(1200., 682., 220., true);
        assert_eq!(audio + 272., 813.);
        assert_eq!(language + 220., 929.);
        assert_eq!(audio - recording_popup_left(760., 682., 272., false), 220.);
        assert_eq!(
            language - recording_popup_left(760., 682., 220., true),
            220.
        );
        assert!(recording_popup_left(360., 360., 272., false) >= 12.);
        assert!(recording_popup_left(360., 360., 220., true) + 220. <= 348.);
    }
}

#[cfg(test)]
mod menu_keyboard_tests {
    use super::{MenuKey, menu_key};

    #[test]
    fn enabled_menu_rows_wrap_without_activating_during_navigation() {
        assert_eq!(menu_key("down", false, None, 3), Some(MenuKey::Select(0)));
        assert_eq!(menu_key("up", false, None, 3), Some(MenuKey::Select(2)));
        assert_eq!(
            menu_key("down", false, Some(2), 3),
            Some(MenuKey::Select(0))
        );
        assert_eq!(menu_key("up", false, Some(0), 3), Some(MenuKey::Select(2)));
        assert_eq!(
            menu_key("enter", false, Some(1), 3),
            Some(MenuKey::Activate(1))
        );
        assert_eq!(
            menu_key("down", false, Some(1), 1),
            Some(MenuKey::Select(0))
        );
    }

    #[test]
    fn menu_keys_preserve_text_edits_and_reject_missing_or_stale_rows() {
        for key in ["left", "right", "home", "end", "a", "backspace", "tab"] {
            assert_eq!(menu_key(key, false, Some(0), 3), None);
        }
        assert_eq!(menu_key("down", true, Some(0), 3), None);
        assert_eq!(menu_key("enter", false, None, 3), None);
        assert_eq!(menu_key("enter", false, Some(4), 3), None);
        assert_eq!(menu_key("down", false, None, 0), None);
        assert_eq!(menu_key("escape", false, None, 0), Some(MenuKey::Dismiss));
    }
}

#[cfg(test)]
mod chrome_tests {
    use super::*;
    // `gpui::*` re-exports its own `test` attribute; these are plain unit tests.
    use crate::screens::SettingsTab;
    use std::prelude::v1::test;

    fn folders() -> Vec<PaletteFolder> {
        [
            "Team meetings",
            "Customer conversations",
            "Ideas & inspiration",
        ]
        .into_iter()
        .enumerate()
        .map(|(index, name)| PaletteFolder {
            id: index as u64 + 10,
            name: name.into(),
            notes: index + 1,
        })
        .collect()
    }

    fn rows(sections: &[PaletteSection], caption: Option<&str>) -> Vec<PaletteRow> {
        sections
            .iter()
            .filter(|section| section.caption == caption)
            .flat_map(|section| section.rows.iter().map(|item| item.row))
            .collect()
    }

    #[test]
    fn empty_palette_offers_new_note_then_folders_then_meetings() {
        let sections = palette_sections("  ", &folders(), &[4, 7, 9]);
        let captions: Vec<_> = sections.iter().map(|section| section.caption).collect();
        assert_eq!(captions, [None, Some("Folders"), Some("Meetings")]);
        assert_eq!(rows(&sections, None), [PaletteRow::NewNote]);
        assert_eq!(
            rows(&sections, Some("Folders")),
            [
                PaletteRow::Folder(0),
                PaletteRow::Folder(1),
                PaletteRow::Folder(2)
            ]
        );
        // Return makes a new note, so the action row is the selected one.
        let selected: Vec<_> = sections
            .iter()
            .flat_map(|section| &section.rows)
            .filter(|item| item.selected)
            .map(|item| item.row)
            .collect();
        assert_eq!(selected, [PaletteRow::NewNote]);
    }

    #[test]
    fn typed_palette_selects_full_results_and_filters_every_section() {
        let sections = palette_sections("team", &folders(), &[7]);
        assert_eq!(rows(&sections, None), [PaletteRow::SearchAll { count: 1 }]);
        assert!(sections[0].rows[0].selected);
        assert_eq!(rows(&sections, Some("Folders")), [PaletteRow::Folder(0)]);
        assert_eq!(rows(&sections, Some("Meetings")), [PaletteRow::Note(7)]);
        assert!(
            sections
                .iter()
                .all(|section| section.caption != Some("Go to"))
        );
        // Pages appear only once the query names one.
        let pages = palette_sections("sett", &folders(), &[]);
        assert_eq!(
            rows(&pages, Some("Go to")),
            [PaletteRow::Page(Screen::Settings, "Settings")]
        );
    }

    #[test]
    fn palette_never_invents_sections_and_caps_long_lists() {
        // No companies or people exist in the workspace, so none are listed.
        for query in ["", "a", "zzz"] {
            for section in palette_sections(query, &folders(), &[1, 2]) {
                assert!(matches!(
                    section.caption,
                    None | Some("Folders" | "Meetings" | "Go to")
                ));
                assert!(!section.rows.is_empty());
            }
        }
        let many: Vec<u64> = (0..40).collect();
        let sections = palette_sections("", &[], &many);
        assert_eq!(rows(&sections, Some("Meetings")).len(), PALETTE_NOTES);
        assert!(
            sections
                .iter()
                .all(|section| section.caption != Some("Folders"))
        );
        // Nothing matches: say so rather than showing an empty panel.
        let none = palette_sections("zzz", &folders(), &[]);
        assert_eq!(none.last().unwrap().rows[0].row, PaletteRow::Empty);
        assert_eq!(rows(&none, None)[0], PaletteRow::SearchAll { count: 0 });
    }

    #[test]
    fn the_palette_highlight_starts_on_the_row_return_already_acts_on() {
        // Empty query: New note, five folders at most, then meetings. New
        // note leads and is the default.
        let sections = palette_sections("", &folders(), &[4, 5, 6]);
        let (count, default) = palette_selection(&sections);
        assert_eq!(default, 0);
        assert_eq!(count, 1 + rows(&sections, Some("Folders")).len() + 3);
        // Typing: the full-results row leads and is the default.
        let typed = palette_sections("team", &folders(), &[7]);
        assert_eq!(palette_selection(&typed).1, 0);
        // The "nothing matches" line is not a row the arrows can land on.
        let none = palette_sections("zzz", &folders(), &[]);
        assert_eq!(palette_selection(&none), (1, 0));
        // Arrows start from that default and wrap at both ends.
        assert_eq!(
            super::menu_key("down", false, Some(default), count),
            Some(super::MenuKey::Select(default + 1))
        );
        assert_eq!(
            super::menu_key("up", false, Some(0), count),
            Some(super::MenuKey::Select(count - 1))
        );
    }

    #[test]
    fn row_menu_lists_existing_actions_in_order_with_one_destructive() {
        let labels: Vec<_> = RowAction::ALL.iter().map(|action| action.label()).collect();
        assert_eq!(
            labels,
            ["Copy link", "Share", "Add to folder", "Move to trash"]
        );
        let destructive: Vec<_> = RowAction::ALL
            .into_iter()
            .filter(|action| action.destructive())
            .collect();
        assert_eq!(destructive, [RowAction::MoveToTrash]);
        assert_eq!(RowAction::ALL.last(), Some(&RowAction::MoveToTrash));
    }

    #[test]
    fn popovers_open_at_the_pointer_but_stay_inside_the_window() {
        let viewport = size(px(1200.), px(840.));
        let menu = size(px(222.), px(122.));
        assert_eq!(
            popover_origin(point(px(640.), px(557.)), menu, viewport),
            point(px(640.), px(557.))
        );
        // Near the bottom-right corner it slides back in, keeping a margin.
        assert_eq!(
            popover_origin(point(px(1190.), px(830.)), menu, viewport),
            point(px(970.), px(710.))
        );
        assert_eq!(
            popover_origin(point(px(-20.), px(2.)), menu, viewport),
            point(px(8.), px(8.))
        );
    }

    #[test]
    fn header_offers_new_note_only_on_home_and_a_back_pill_everywhere_else() {
        assert_eq!(header_action(Screen::Home), Some(HeaderAction::NewNote));
        assert_eq!(header_action(Screen::Chat), Some(HeaderAction::NewChat));
        for screen in [
            Screen::Shared,
            Screen::Connect,
            Screen::MyNotes,
            Screen::Folder(3),
            Screen::Favorites,
            Screen::Settings,
            Screen::Trash,
            Screen::SearchResults,
        ] {
            assert_eq!(header_action(screen), None, "{screen:?}");
            assert!(back_label(screen, Screen::Home).is_some(), "{screen:?}");
        }
        assert_eq!(back_label(Screen::Home, Screen::Home), None);
        // Space pages say "Back"; other pages show where they return to.
        assert_eq!(
            back_label(Screen::MyNotes, Screen::Home),
            Some(BackLabel::Text)
        );
        assert_eq!(
            back_label(Screen::Folder(1), Screen::MyNotes),
            Some(BackLabel::Text)
        );
        assert_eq!(
            back_label(Screen::Settings, Screen::Chat),
            Some(BackLabel::Icon("speech-bubble"))
        );
        assert_eq!(
            back_label(Screen::Shared, Screen::Home),
            Some(BackLabel::Icon("home"))
        );
    }

    #[test]
    fn navigation_trail_returns_step_by_step_and_never_loops() {
        let mut trail = Vec::new();
        record_navigation(&mut trail, Screen::Home, Screen::Chat);
        record_navigation(&mut trail, Screen::Chat, Screen::Settings);
        assert_eq!(trail, [Screen::Home, Screen::Chat]);
        // Staying put, or arriving from sign-in, records nothing.
        record_navigation(&mut trail, Screen::Settings, Screen::Settings);
        record_navigation(&mut trail, Screen::Account, Screen::Settings);
        assert_eq!(trail, [Screen::Home, Screen::Chat]);
        // A link's page is reached from somewhere and returns there, but is
        // never itself a place to return to.
        record_navigation(&mut trail, Screen::Settings, Screen::Link);
        assert_eq!(trail, [Screen::Home, Screen::Chat, Screen::Settings]);
        record_navigation(&mut trail, Screen::Link, Screen::Account);
        assert_eq!(trail, [Screen::Home, Screen::Chat, Screen::Settings]);
        trail.pop();
        // Revisiting a page on the trail unwinds to it instead of looping.
        record_navigation(&mut trail, Screen::Settings, Screen::Chat);
        assert_eq!(trail, [Screen::Home]);
        // Home is the root: reaching it clears the trail.
        record_navigation(&mut trail, Screen::Chat, Screen::MyNotes);
        record_navigation(&mut trail, Screen::MyNotes, Screen::Home);
        assert!(trail.is_empty());
        // The trail is bounded however long the session runs.
        for folder in 0..100 {
            record_navigation(
                &mut trail,
                Screen::Folder(folder),
                Screen::Folder(folder + 1),
            );
        }
        assert!(trail.len() <= 24);
    }

    #[test]
    fn settings_mode_has_only_real_pages_and_starts_on_preferences() {
        assert_eq!(SettingsTab::default(), SettingsTab::Preferences);
        let labels: Vec<_> = SettingsTab::ALL.iter().map(|tab| tab.label()).collect();
        assert_eq!(labels, ["Preferences", "Profile", "Calendar", "About"]);
        // Each page has its own glyph and none stands in for an unbuilt feature.
        let mut icons: Vec<_> = SettingsTab::ALL.iter().map(|tab| tab.icon()).collect();
        icons.dedup();
        assert_eq!(icons.len(), SettingsTab::ALL.len());
        for missing in [
            "Notifications",
            "Billing",
            "Members",
            "Referrals",
            "Templates",
        ] {
            assert!(!labels.contains(&missing));
        }
    }
}
