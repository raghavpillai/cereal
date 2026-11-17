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
/// The sidebar's navigation glyphs, in points.
const NAV_GLYPH: f32 = 18.;
/// The window's own controls beside the traffic lights (the sidebar toggle
/// and search), in points.
const WINDOW_TOOL: f32 = 18.;
/// The strip over the sidebar's edge that takes hold of it, in points.
const SIDEBAR_EDGE: f32 = 6.;

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

    pub(crate) fn register(
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
    /// Open `menu` from its button. A press outside an open menu closes it; when
    /// that press was on this menu's own button, the click it completes must not
    /// open the menu again, so the button works as a toggle. Only the click that
    /// completes that same press is refused, however long it is held: the record
    /// names the press, and is dropped when the press is released. Returns whether
    /// it opened.
    pub(crate) fn open_menu(&mut self, menu: Menu) -> bool {
        let press = self.press_seq;
        let just_closed = self
            .menu_closed_by_press
            .take()
            .is_some_and(|(closed, by)| closed == menu && by == press);
        if !just_closed {
            self.menu = Some(menu);
        }
        !just_closed
    }

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
            // A Trash row that is asking keeps the keys: the arrows stay on
            // it and Return lets go of the question.
            MenuKey::Select(_) | MenuKey::Activate(_)
                if self.trash_key_while_asking(matches!(command, MenuKey::Select(_)), cx) => {}
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
            // A Trash row asking to delete forever is let go of first.
            MenuKey::Dismiss if self.menu == Some(Menu::Trash) && self.trash_ask.is_some() => {
                self.trash_ask = None;
                cx.notify();
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

    /// The sidebar button and its shortcut: beside the page, the sidebar
    /// opens or closes; in a narrow window it slides over the page or away.
    pub fn toggle_sidebar(&mut self) {
        if self.sidebar_narrow {
            self.sidebar_overlay = !self.sidebar_overlay;
        } else {
            self.sidebar_open = !self.sidebar_open;
        }
    }

    /// The sidebar over a narrow window, as in the reference: it slides in
    /// from the left edge with rounded outer corners and a shadow, over the
    /// page dimmed behind it. A press on the page closes it, as do Escape and
    /// any row it opens.
    pub(crate) fn render_sidebar_overlay(
        &self,
        pose: crate::motion::Pose,
        cx: &mut Context<Self>,
    ) -> Div {
        let t = self.theme();
        let level = pose.value.clamp(0., 1.);
        let width = self.sidebar_span;
        div()
            .absolute()
            .inset_0()
            .child(
                div()
                    .id("sidebar-overlay-scrim")
                    .absolute()
                    .inset_0()
                    .bg(hsla(0., 0., 0., if self.workspace.settings.dark_mode { 0.4 } else { 0.28 } * pose.fade.clamp(0., 1.)))
                    .occlude()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.sidebar_overlay = false;
                            cx.notify();
                        }),
                    ),
            )
            .child(
                div()
                    .id("sidebar-overlay")
                    .absolute()
                    .top_0()
                    .bottom_0()
                    // Slid out from under the window's left edge; the spring's
                    // overshoot is held at the edge.
                    .left(px(-(width + 24.) * (1. - level)))
                    .w(px(width))
                    .occlude()
                    .rounded_r(px(12.))
                    .overflow_hidden()
                    .shadow(vec![BoxShadow {
                        color: hsla(0., 0., 0., 0.18 * pose.fade.clamp(0., 1.)),
                        offset: point(px(4.), px(0.)),
                        blur_radius: px(24.),
                        spread_radius: px(0.),
                    }])
                    .child(self.render_sidebar(cx).border_r_0()),
            )
    }

    /// The sidebar's on-screen width right now: its width as far as it has
    /// slid out. What the page beside it starts at.
    pub(crate) fn sidebar_inset(&self) -> f32 {
        self.sidebar_span * self.sidebar_level
    }

    /// The sidebar's edge: a 6 pt strip over the hairline that resizes the
    /// sidebar. Under the pointer, and while it is dragged, a 2 pt line in
    /// the brand gold fades in and the pointer becomes the left-right
    /// resize cursor (the only place in the app that changes it). The edge
    /// follows the pointer exactly, between the limits; a double-click
    /// returns to the default width. The width is saved when the drag ends.
    pub(crate) fn render_sidebar_edge(
        &self,
        sidebar: crate::motion::Pose,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let dragging = self.sidebar_grab.is_some();
        // Only beside a sidebar that is out and at rest.
        let present = sidebar.visible && self.sidebar_open && self.sidebar_level >= 0.999;
        let lit = self.stage.presence(
            ("sidebar-edge", 0),
            present && (dragging || self.sidebar_edge_hovered),
            crate::motion::SNAPPY,
        );
        if !present {
            return Vec::new();
        }
        let t = self.theme();
        let finish = |this: &mut Self, cx: &mut Context<Self>| {
            if this.sidebar_grab.take().is_some() {
                if let Err(error) = crate::desktop_preferences::set_sidebar_width(this.sidebar_width) {
                    this.show_toast(&error, cx);
                }
                cx.notify();
            }
        };
        let strip = div()
            .id("sidebar-edge")
            .absolute()
            .top_0()
            .bottom_0()
            .left(px(self.sidebar_span - SIDEBAR_EDGE / 2.))
            .w(px(SIDEBAR_EDGE))
            .cursor(CursorStyle::ResizeLeftRight)
            .occlude()
            // Where it is drawn, for scripted runs that press it.
            .child(crate::onboarding_view::place("sidebar-edge"))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                this.sidebar_edge_hovered = *hovered;
                cx.notify();
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    if event.click_count >= 2 {
                        this.sidebar_grab = None;
                        this.sidebar_width = crate::desktop_preferences::SIDEBAR_DEFAULT;
                        if let Err(error) =
                            crate::desktop_preferences::set_sidebar_width(this.sidebar_width)
                        {
                            this.show_toast(&error, cx);
                        }
                    } else {
                        this.sidebar_grab = Some(f32::from(event.position.x) - this.sidebar_span);
                    }
                    cx.notify();
                }),
            )
            .when(lit.visible, |strip| {
                strip.child(
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left(px((SIDEBAR_EDGE - 2.) / 2.))
                        .w(px(2.))
                        .bg(t.accent.opacity(lit.fade.clamp(0., 1.))),
                )
            });
        let mut layers = vec![strip.into_any_element()];
        if let Some(grab) = self.sidebar_grab {
            // While it is held the whole window follows the pointer, so the
            // drag and its cursor survive the pointer outrunning the strip.
            layers.push(
                div()
                    .id("sidebar-edge-drag")
                    .absolute()
                    .inset_0()
                    .cursor(CursorStyle::ResizeLeftRight)
                    .occlude()
                    .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, window, cx| {
                        if event.pressed_button != Some(MouseButton::Left) {
                            // The button came up somewhere it was not seen.
                            finish(this, cx);
                            return;
                        }
                        let width = crate::desktop_preferences::sidebar_width_in(
                            f32::from(event.position.x) - grab,
                            f32::from(window.viewport_size().width),
                        );
                        if width != this.sidebar_width {
                            this.sidebar_width = width;
                            cx.notify();
                        }
                    }))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| finish(this, cx)),
                    )
                    .on_mouse_up_out(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| finish(this, cx)),
                    )
                    .into_any_element(),
            );
        }
        layers
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
            .w(px(self.sidebar_span))
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
                    // On the traffic lights' centre line (22 pt down), where
                    // the same two controls sit in the header when the
                    // sidebar is hidden.
                    .pb(px(4.))
                    .pl(px(88.))
                    .gap(px(4.))
                    .child(
                        tool_sized("sidebar-toggle", "sidebar-expanded", WINDOW_TOOL, t).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.toggle_sidebar();
                                cx.notify();
                            }),
                        ),
                    )
                    .child(
                        tool_sized("sidebar-search", "search", WINDOW_TOOL, t)
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
    pub(crate) fn space_tile(&self, symbol: &str) -> Div {
        let t = self.theme();
        div()
            .size(px(20.))
            .flex_shrink_0()
            .rounded(px(5.))
            .border_1()
            .border_color(t.hairline)
            // A step under the resting fill, with a quiet glyph: the tile
            // marks the row, the label names it.
            .bg(t.fill.opacity(0.8))
            .flex()
            .items_center()
            .justify_center()
            .child(icon(symbol, 12., t.muted))
    }

    fn render_workspace_sidebar(&self, cx: &mut Context<Self>) -> Div {
        let t = self.theme();
        let glide = self.stage.glide("sidebar");
        let (threads, thread_selected) = self.render_sidebar_chat_threads(cx);
        let spaces = div()
            .flex()
            .flex_col()
            .px(px(8.))
            .mt(px(18.))
            // Rows 28 pt tall on a 29 pt pitch, as the navigation above.
            .gap(px(1.))
            .child(
                div()
                    .h(px(26.))
                    .flex()
                    .items_center()
                    .px(px(4.))
                    // A heading in the labels' own ink, a size down.
                    .child(
                        small("Spaces", t)
                            .text_color(t.secondary)
                            .font_weight(FontWeight::MEDIUM),
                    ),
            )
            .child(
                div()
                    .id("My notes")
                    .h(px(28.))
                    .px(px(4.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .rounded(px(8.))
                    .text_color(t.secondary)
                    .cursor_default()
                    .child(row_feedback(t, 8.))
                    .when(
                        self.screen == Screen::MyNotes && self.current_note.is_none(),
                        |s| s.child(glide.probe("My notes")),
                    )
                    .child(self.space_tile("lock-solid"))
                    .child(div().flex_1().min_w_0().truncate().child("My notes"))
                    .reach().on_click(cx.listener(|this, _, _, cx| this.navigate(Screen::MyNotes, cx))),
            )
            // The team space, under it (see `team_space.rs`).
            .children(self.team_space_row(cx));
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
                    .child(self.nav_row("Home", "house", Screen::Home, true, cx))
                    .child(self.nav_row("Shared with me", "people", Screen::Shared, true, cx))
                    // The conversation on the page carries the highlight in
                    // Chat's place while it is listed beneath.
                    .child(self.nav_row("Chat", "speech-bubble", Screen::Chat, !thread_selected, cx))
                    .children(threads),
            )
            .child(spaces)
            .child(div().flex_1())
            .child(self.render_sidebar_footer_tools(cx))
            // Quieter than the sidebar's own edge: it parts two rows, not
            // two surfaces.
            .child(div().mx(px(12.)).h(px(1.)).flex_shrink_0().bg(t.hairline.opacity(0.65)))
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
                    .cursor_default()
                    .child(row_feedback(t, 0.))
                    // Where it is drawn, for scripted runs that press it.
                    .child(crate::onboarding_view::place("workspace-switcher"))
                    .child(self.space_tile("prompt"))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_weight(FontWeight::MEDIUM)
                            .child(self.workspace_name()),
                    )
                    .child(icon("chevrons-switcher", 16., t.muted))
                    .reach().on_click(cx.listener(|this, _, _, cx| {
                        if this.menu == Some(Menu::Workspace) {
                            this.menu = None;
                        } else {
                            this.open_menu(Menu::Workspace);
                        }
                        cx.notify();
                    })),
            )
    }

    /// The signed-in person's name, or the local workspace's name.
    pub(crate) fn workspace_name(&self) -> String {
        crate::accounts::default_session()
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
        // The trash button stays lit while its popover is up.
        let trash_open = self.visible_menu == Some(Menu::Trash) && !self.menu_exiting;
        let tool =
            |index: usize, id: &'static str, symbol: &'static str, label: &'static str, lit: bool| {
                div()
                    .id(id)
                    .relative()
                    .size(px(28.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(8.))
                    .reach()
                    .cursor_default()
                    .when(lit, |tool| tool.bg(t.hover))
                    .child(crate::motion::tactile(t.hover).press(t.fill).radius(8.))
                    .child(icon(symbol, 16., t.muted))
                    // Where it is drawn, for scripted runs that press it.
                    .child(crate::onboarding_view::place(id))
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
                tool(0, "footer-shortcuts", "keyboard", "Keyboard shortcuts", false).on_click(
                    cx.listener(|this, _, _, cx| {
                        footer_tip_epoch();
                        this.footer_tip = None;
                        this.open_menu(Menu::Shortcuts);
                        cx.notify();
                    }),
                ),
            )
            .child(
                tool(1, "footer-trash", "trash", "Trash", trash_open).on_click(cx.listener(
                    |this, _, _, cx| {
                        footer_tip_epoch();
                        this.footer_tip = None;
                        this.toggle_trash(cx);
                    },
                )),
            )
    }

    /// Under Chat, while the Chat page is open: its conversations, newest
    /// first, each with its age; the eighth faded when more follow, and
    /// "Show N more" to list the rest. They unfold beneath Chat and fold
    /// back into it, 29 px a row, as the folders do.
    ///
    /// Also says whether one of the rows is the conversation on the page:
    /// that row then has the sidebar's highlight, not Chat.
    fn render_sidebar_chat_threads(&self, cx: &mut Context<Self>) -> (Option<Div>, bool) {
        const SHOWN: usize = 8;
        let t = self.theme();
        let glide = self.stage.glide("sidebar");
        let mut selected = false;
        let open = self.screen == Screen::Chat && self.current_note.is_none();
        let threads = self.chat_threads();
        let pose = self.stage.presence(
            ("sidebar-chat-threads", 0),
            open && !threads.is_empty(),
            crate::motion::SNAPPY,
        );
        if !pose.visible {
            return (None, false);
        }
        let listed = if self.chat_threads_expanded {
            threads.len()
        } else {
            threads.len().min(SHOWN)
        };
        // Those read but not listed, and those the servers have not sent.
        let more = threads.len() - listed + self.chat_threads_more();
        let now = chrono::Utc::now();
        let count = listed;
        let accounts: Vec<_> = threads.iter().fold(Vec::new(), |mut accounts, thread| {
            if !accounts.contains(&thread.account) {
                accounts.push(thread.account.clone());
            }
            accounts
        });
        let mut rows = div().flex().flex_col().gap(px(1.));
        let fading = |index: usize| more > 0 && index + 1 == count;
        for (index, thread) in threads.into_iter().take(listed).enumerate() {
            let (id, account) = (thread.id.clone(), thread.account.clone());
            let key = crate::lazy::key(&thread.id);
            let shown = self.chat_thread_id == thread.id && !self.chat_messages.is_empty();
            rows = rows.child(
                div()
                    .id(("sidebar-chat-thread", key))
                    .h(px(28.))
                    .w_full()
                    .pl(px(28.))
                    .pr(px(4.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .rounded(px(8.))
                    // The conversation on the page has the sidebar's
                    // highlight; its title stays in the rows' own ink.
                    .text_color(t.secondary)
                    .cursor_default()
                    .child(row_feedback(t, 8.))
                    .when(shown && open, |row| {
                        selected = true;
                        row.child(glide.probe(("chat-thread", key)))
                    })
                    // The last row fades out when the list goes on (its
                    // words, not the highlight under them).
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .when(fading(index), |title| title.opacity(0.45))
                            .child(thread.display_title().to_owned()),
                    )
                    // Its age, a step quieter than any label: read after
                    // the title, never before it.
                    .children(crate::chat::thread_age(&thread.updated_at, now).map(|age| {
                        div()
                            .flex_shrink_0()
                            .text_size(px(12.))
                            // (A point lower: the smaller figures sit on the
                            // title's baseline, not its centre line.)
                            .pt(px(1.))
                            .text_color(t.muted.opacity(0.7))
                            .when(fading(index), |age| age.opacity(0.45))
                            .child(age)
                    }))
                    .reach()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open_chat_thread(&account, &id, true, cx)
                    })),
            );
        }
        if more > 0 {
            rows = rows.child(
                div()
                    .id("sidebar-chat-threads-more")
                    .h(px(28.))
                    .w_full()
                    // The arrow centred 30 pt in, under the navigation
                    // glyphs' right half; its label 43 pt in.
                    .pl(px(14.))
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .rounded(px(8.))
                    .text_color(t.secondary)
                    .cursor_default()
                    .child(row_feedback(t, 8.))
                    .child(icon("arrow-down", 16., t.secondary))
                    .child(div().flex_1().min_w_0().truncate().child(format!("Show {more} more")))
                    .reach()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        // All of those read, and the next page from each
                        // server that has more.
                        this.chat_threads_expanded = true;
                        for account in &accounts {
                            this.load_more_chat_threads(account, cx);
                        }
                        cx.notify();
                    })),
            );
        }
        if !pose.at_rest() {
            let natural = 29. * (count + usize::from(more > 0)) as f32;
            rows = rows
                .h(px(natural * pose.value.clamp(0., 1.)))
                .overflow_hidden()
                .opacity(pose.fade);
        }
        (Some(rows), selected)
    }

    fn nav_row(
        &self,
        label: &'static str,
        symbol: &str,
        screen: Screen,
        // Whether the row may carry the highlight when its page is open
        // (Chat hands it to the conversation listed beneath).
        highlight: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let t = self.theme();
        div()
            .id(label)
            .h(px(28.))
            .w_full()
            // An 18 pt glyph centred 22 pt in, its label 38 pt in.
            .pl(px(5.))
            .pr(px(6.))
            .flex()
            .items_center()
            .gap(px(7.))
            .rounded(px(8.))
            .text_color(t.secondary)
            .cursor_default()
            .child(row_feedback(t, 8.))
            .when(
                highlight && self.screen == screen && self.current_note.is_none(),
                |s| s.child(self.stage.glide("sidebar").probe(label)),
            )
            // A size up from the toolbar glyphs and a step quieter than the
            // label, so the words lead and the glyphs mark the rows.
            .child(icon(symbol, NAV_GLYPH, t.muted))
            .child(div().flex_1().min_w_0().truncate().child(label))
            .reach().on_click(cx.listener(move |this, _, _, cx| {
                this.navigate(screen, cx);
                // Chat opens on a new conversation, as in the reference: the
                // one that was open is kept in Recents (an answer still
                // arriving for it goes on off screen).
                if screen == Screen::Chat && !this.chat_messages.is_empty() {
                    this.park_chat_stream();
                    this.clear_chat(cx);
                }
            }))
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
                                this.focus.focus(window); this.open_menu(Menu::Note); cx.notify();
                            }))
                        );
                        d.child(options.when(self.current_note.and_then(|id| self.workspace.note(id)).is_some_and(|note| !note.enhanced.trim().is_empty()), |options| options.child(
                            div().flex().gap(px(1.))
                                .child(div().id("note-share").h(px(28.)).px(px(11.)).rounded_l(px(14.)).rounded_r(px(4.)).bg(t.text).text_color(t.bg).text_size(px(13.)).flex().items_center().gap(px(5.)).cursor_default().map(|d|crate::motion::dims(d,crate::motion::reveal_key("note-share"),0.1)).child(icon("lock-solid",13.,t.bg)).child("Share").reach().on_click(cx.listener(|this,_,window,cx|{this.focus.focus(window);this.menu=Some(Menu::Share);cx.notify();})))
                                .child(div().id("note-copy-share-link").w(px(30.)).h(px(28.)).rounded_l(px(4.)).rounded_r(px(14.)).bg(t.text).flex().items_center().justify_center().cursor_default().map(|d|crate::motion::dims(d,crate::motion::reveal_key("note-copy-share-link"),0.1)).child(icon("link-chain",14.,t.bg)).reach().on_click(cx.listener(|this,_,_,cx|{if let Some(id)=this.current_note { if crate::server::enabled() { this.server_action(id,crate::server::Action::Share,cx); } else { this.show_toast("Connect to your server to share a note",cx); } }})))
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
                    .when(!self.sidebar_open || (self.sidebar_narrow && self.sidebar_level <= 0. && self.current_note.is_none()), |s| {
                        s.child(tool_sized("open-sidebar", "sidebar-collapsed", WINDOW_TOOL, t).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.toggle_sidebar();
                                cx.notify();
                            }),
                        ))
                        .child(
                            tool_sized("collapsed-header-search", "search", WINDOW_TOOL, t).on_click(
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
            // An empty Chat page is already a new chat: its header stays
            // clear, as in the reference.
            .when_some(
                header_action(self.screen).filter(|action| {
                    *action != HeaderAction::NewChat || !self.chat_messages.is_empty()
                }),
                |header, action| {
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
            },
            )
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
                            .ml(px(-width / 2. - self.sidebar_inset() / 2.))
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
                                crate::recording_view::WAVEFORM.into(),
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
        let sections = palette_sections(query, &notes);
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
                .cursor_default()
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
                    PaletteRow::Trash => (
                        row("Trash".into())
                            .child(icon("trash", 16., t.secondary).mr(px(4.)))
                            .child(div().flex_1().child("Trash")),
                        Some(Rc::new(
                            move |this: &mut Self, window: &mut Window, cx: &mut Context<Self>| {
                                this.focus.focus(window);
                                this.open_trash(cx);
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
                            .child("No matching meetings"),
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
                // The press may be on the button that opened this menu: remember it, so
                // the click it completes closes the menu rather than opening it again.
                if let Some(closed) = this.menu.take() {
                    this.menu_closed_by_press = Some((closed, this.press_seq));
                }
                this.focus.focus(w);
                cx.notify();
            }),
        );
        match menu {
            Menu::Trash => {
                return overlay
                    .child(self.render_trash_popover(pose, window, cx))
                    .into_any_element();
            }
            // Who the note is shared with, the people picker and the link's
            // reach: `share_popover.rs`.
            Menu::Share => {
                return overlay
                    .child(self.render_share_popover(pose, window, cx))
                    .into_any_element();
            }
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
            Menu::RecordingSettings | Menu::RecordingInput | Menu::RecordingLanguage => {
                let viewport_width = f32::from(window.viewport_size().width);
                let enhanced = self
                    .current_note
                    .and_then(|id| self.workspace.note(id))
                    .is_some_and(|note| {
                        !note.enhanced.trim().is_empty() || self.generation.is_running_for(note.id)
                    });
                let panel_width =
                    (viewport_width.min(if enhanced { 682. } else { 672. }) + 8.).min(682.);
                // Each grows upwards out of its own control in the
                // transcript panel's footer.
                let (panel, popup_width, inset) = self.recording_popover(&menu, cx);
                let left = recording_popup_left(viewport_width, panel_width, popup_width)
                    - (inset - 12.).min(viewport_width);
                let left = left.clamp(12., (viewport_width - popup_width - 12.).max(12.));
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
                let session = crate::accounts::default_session().filter(|_| connected);
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
                                            None => "Sign in to your server".into(),
                                        },
                                        t,
                                    )),
                            ),
                    )
                    .child(separator(t).my(px(4.)));
                // Trash has its own button at the foot of the sidebar.
                content = content.child(self.menu_row("Favorites", "star", cx, |this, _, cx| {
                    this.navigate(Screen::Favorites, cx)
                }));
                content = content
                    .child(separator(t).my(px(4.)))
                    .child(
                        self.menu_row("Keyboard shortcuts", "keyboard", cx, |this, _, cx| {
                            this.open_menu(Menu::Shortcuts);
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
                    size(px(222.), px(RowAction::ALL.len() as f32 * 28. + 10.)),
                    window.viewport_size(),
                );
                // It opens under the pointer, like every native context menu.
                overlay = overlay.child(div().absolute().left(origin.x).top(origin.y).child(pop(
                    content.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()),
                    Pop::menu((0.05, 0.)),
                    pose,
                )));
            }
            Menu::Theme => {
                // 178 px of surface inside the hairline, rows on a 28 px
                // pitch, a check at the end of the scheme in use.
                let current = self.workspace.settings.theme;
                let width = 180.;
                content = content.w(px(width)).p(px(4.));
                for mode in crate::model::ThemeMode::ALL {
                    content = content.child(
                        self.menu_row(mode.label(), "", cx, move |this, _, cx| {
                            this.menu = None;
                            this.workspace.settings.set_theme(mode);
                            this.workspace.settings_changed();
                            this.persist(cx);
                            cx.notify();
                        })
                        .px(px(8.))
                        .child(div().flex_1())
                        .when(mode == current, |row| row.child(icon("check", 14., t.secondary))),
                    );
                }
                // Over its button, a little proud of it at the top and the
                // trailing edge, as a pop-up button's menu is.
                let button = self.theme_anchor.get();
                let origin = popover_origin(
                    point(button.right() + px(4.) - px(width), button.top() - px(4.)),
                    size(px(width), px(crate::model::ThemeMode::ALL.len() as f32 * 28. + 10.)),
                    window.viewport_size(),
                );
                overlay = overlay.child(div().absolute().left(origin.x).top(origin.y).child(pop(
                    content.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()),
                    Pop::menu((0.9, 0.)),
                    pose,
                )));
            }
            Menu::Note | Menu::Templates => {
                content = content.w(px(220.));
                if menu == Menu::Note {
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
                        .child(separator(t))
                        .child(
                            self.danger_row("Move to trash", "trash", cx, |this, _, cx| {
                                if let Some(id) = this.current_note {
                                    this.trash_note(id, cx);
                                }
                            }),
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
                                _ => (0.15, 0.),
                            }),
                            pose,
                        )),
                );
            }
            _ => {
                let sheet = (menu == Menu::Account)
                    .then(|| self.account_sheet.as_ref().map(|sheet| sheet.text()))
                    .flatten();
                let (title,description)=match &menu {
                    Menu::Account=>sheet.as_ref().map(|sheet|(sheet.title.clone(),sheet.body.clone())).unwrap_or_default(),
                    Menu::RecordingHelp=>("Recording a meeting".into(),"Start captures your microphone and computer audio separately. Stop saves the recording and transcribes it without generating notes. Choose Generate notes when you are ready. Resume keeps the same transcript; after you stop again, Regenerate notes includes the new conversation.".into()),
                    Menu::Shortcuts=>("Keyboard shortcuts".into(),"Make yourself at home.".into()),
                    _=>("Cereal".into(),"Native notes, thoughtfully simple.".into()),
                };
                content = content
                    .w(px(440.))
                    .p(px(24.))
                    .gap(px(16.))
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .items_center()
                            // An account sheet names a host in full: its
                            // title wraps rather than leave the dialog.
                            .when(menu == Menu::Account, |row| row.items_start().gap(px(12.)))
                            .child(if menu == Menu::Account {
                                div().flex_1().min_w_0().child(heading(title, t))
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
                            .child(description),
                    );
                if let Some(sheet) = sheet {
                    content = content.child(self.account_sheet_actions(&sheet, cx));
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
                    .bg(scrim(Scrim::Dialog, pose.fade))
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

    pub(crate) fn menu_row(
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
            .cursor_default()
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
                    tint.unwrap_or(t.secondary),
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

/// One row of the command palette.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteRow {
    NewNote,
    SearchAll {
        count: usize,
    },
    Note(u64),
    Page(Screen, &'static str),
    /// Trash is not a page: the row opens the sidebar's Trash popover.
    Trash,
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

const PALETTE_NOTES: usize = 8;
const PALETTE_PAGES: [(Screen, &str); 5] = [
    (Screen::Home, "Home"),
    (Screen::MyNotes, "My notes"),
    (Screen::Shared, "Shared with me"),
    (Screen::Chat, "Chat"),
    (Screen::Settings, "Settings"),
];

/// What the command palette shows for `query`. `notes` are the ids of the
/// notes already matching the query, best first.
///
/// Sections exist only when there is something real to put in them: there are
/// no companies or people here because the workspace has no such records.
/// The selected row is always the one Return acts on — the full-results row
/// while typing, otherwise "New note".
pub fn palette_sections(query: &str, notes: &[u64]) -> Vec<PaletteSection> {
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
        .chain((!folded.is_empty() && "trash".contains(&folded)).then(|| item(PaletteRow::Trash)))
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
        Screen::TeamSpace | Screen::TeamNote => "users",
        Screen::Link => "link-chain",
        Screen::Chat => "speech-bubble",
        Screen::MyNotes => "lock-solid",
        Screen::Favorites => "star",
        Screen::Settings => "settings",
        Screen::SearchResults => "search",
        Screen::Home | Screen::Login | Screen::Account | Screen::Welcome => "home",
    }
}

/// The back pill for `screen`, given the screen it would return to. Home has
/// none; space pages say "Back".
pub fn back_label(screen: Screen, target: Screen) -> Option<BackLabel> {
    match screen {
        Screen::Home | Screen::Login | Screen::Account | Screen::Welcome => None,
        Screen::MyNotes | Screen::Favorites | Screen::TeamSpace => Some(BackLabel::Text),
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
    if from == to
        || matches!(
            from,
            Screen::Login | Screen::Account | Screen::Link | Screen::Welcome
        )
    {
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
    MoveToTrash,
}

impl RowAction {
    pub const ALL: [RowAction; 3] = [
        RowAction::CopyLink,
        RowAction::Share,
        RowAction::MoveToTrash,
    ];

    pub fn label(self) -> &'static str {
        match self {
            RowAction::CopyLink => "Copy link",
            RowAction::Share => "Share",
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
        .min_w(px(43.))
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
        .cursor_default()
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
        // A note in Trash is no longer shown to the team.
        self.team_space_note_trashed(id, cx);
        // Nor read by the people it is shared with, until it is restored.
        self.share_note_trashed(id, true, cx);
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
            RowAction::MoveToTrash => self.trash_note(note, cx),
        }
    }
}

/// The audio settings button is the last of the transcript footer controls,
/// inside the footer's 12 px padding: its menu ends where the button does.
fn recording_popup_left(viewport_width: f32, panel_width: f32, popup_width: f32) -> f32 {
    let panel_right = (viewport_width + panel_width.min(viewport_width)) / 2.;
    let right_inset = 12.;
    (panel_right - right_inset - popup_width)
        .clamp(12., (viewport_width - popup_width - 12.).max(12.))
}

#[cfg(test)]
mod recording_popup_tests {
    use super::recording_popup_left;

    #[test]
    fn footer_menus_follow_the_transcript_controls_when_resizing() {
        let audio = recording_popup_left(1200., 682., 272.);
        assert_eq!(audio + 272., 929.);
        assert_eq!(audio - recording_popup_left(760., 682., 272.), 220.);
        assert!(recording_popup_left(360., 360., 272.) >= 12.);
        assert!(recording_popup_left(360., 360., 272.) + 272. <= 348.);
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

    fn rows(sections: &[PaletteSection], caption: Option<&str>) -> Vec<PaletteRow> {
        sections
            .iter()
            .filter(|section| section.caption == caption)
            .flat_map(|section| section.rows.iter().map(|item| item.row))
            .collect()
    }

    #[test]
    fn empty_palette_offers_new_note_then_meetings() {
        let sections = palette_sections("  ", &[4, 7, 9]);
        let captions: Vec<_> = sections.iter().map(|section| section.caption).collect();
        assert_eq!(captions, [None, Some("Meetings")]);
        assert_eq!(rows(&sections, None), [PaletteRow::NewNote]);
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
        let sections = palette_sections("team", &[7]);
        assert_eq!(rows(&sections, None), [PaletteRow::SearchAll { count: 1 }]);
        assert!(sections[0].rows[0].selected);
        assert_eq!(rows(&sections, Some("Meetings")), [PaletteRow::Note(7)]);
        assert!(
            sections
                .iter()
                .all(|section| section.caption != Some("Go to"))
        );
        // Pages appear only once the query names one.
        let pages = palette_sections("sett", &[]);
        assert_eq!(
            rows(&pages, Some("Go to")),
            [PaletteRow::Page(Screen::Settings, "Settings")]
        );
        // Trash is the sidebar's popover, not a page.
        assert_eq!(rows(&palette_sections("tra", &[]), Some("Go to")), [PaletteRow::Trash]);
    }

    #[test]
    fn palette_never_invents_sections_and_caps_long_lists() {
        // No companies or people exist in the workspace, so none are listed.
        for query in ["", "a", "zzz"] {
            for section in palette_sections(query, &[1, 2]) {
                assert!(matches!(
                    section.caption,
                    None | Some("Meetings" | "Go to")
                ));
                assert!(!section.rows.is_empty());
            }
        }
        let many: Vec<u64> = (0..40).collect();
        let sections = palette_sections("", &many);
        assert_eq!(rows(&sections, Some("Meetings")).len(), PALETTE_NOTES);
        // Nothing matches: say so rather than showing an empty panel.
        let none = palette_sections("zzz", &[]);
        assert_eq!(none.last().unwrap().rows[0].row, PaletteRow::Empty);
        assert_eq!(rows(&none, None)[0], PaletteRow::SearchAll { count: 0 });
    }

    #[test]
    fn the_palette_highlight_starts_on_the_row_return_already_acts_on() {
        // Empty query: New note, then meetings. New note leads and is the
        // default.
        let sections = palette_sections("", &[4, 5, 6]);
        let (count, default) = palette_selection(&sections);
        assert_eq!(default, 0);
        assert_eq!(count, 1 + 3);
        // Typing: the full-results row leads and is the default.
        let typed = palette_sections("team", &[7]);
        assert_eq!(palette_selection(&typed).1, 0);
        // The "nothing matches" line is not a row the arrows can land on.
        let none = palette_sections("zzz", &[]);
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
            ["Copy link", "Share", "Move to trash"]
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
            Screen::MyNotes,
            Screen::Favorites,
            Screen::Settings,
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
            back_label(Screen::Favorites, Screen::MyNotes),
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
        let pages = [
            Screen::MyNotes,
            Screen::SearchResults,
            Screen::Favorites,
            Screen::Shared,
            Screen::Chat,
            Screen::Settings,
        ];
        for step in 0..100 {
            record_navigation(
                &mut trail,
                pages[step % pages.len()],
                pages[(step * 3 + 1) % pages.len()],
            );
        }
        assert!(trail.len() <= 24);
    }

    #[test]
    fn settings_mode_has_only_real_pages_and_starts_on_preferences() {
        assert_eq!(SettingsTab::default(), SettingsTab::Preferences);
        let labels: Vec<_> = SettingsTab::ALL.iter().map(|tab| tab.label()).collect();
        assert_eq!(
            labels,
            ["Preferences", "Accounts", "Calendar", "Notifications", "About"]
        );
        // Each page has its own glyph and none stands in for an unbuilt feature.
        let mut icons: Vec<_> = SettingsTab::ALL.iter().map(|tab| tab.icon()).collect();
        icons.dedup();
        assert_eq!(icons.len(), SettingsTab::ALL.len());
        for missing in [
            "Billing",
            "Members",
            "Referrals",
            "Templates",
        ] {
            assert!(!labels.contains(&missing));
        }
    }
}
