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

/// The sidebar's navigation glyphs, in points.
const NAV_GLYPH: f32 = 18.;
/// The window's own controls beside the traffic lights (the sidebar toggle
/// and search), in points.
const WINDOW_TOOL: f32 = 18.;
/// The gap between the two, and what the pair and the gap after it take in
/// the header when the sidebar is away (two 28 pt tools 4 pt apart, and 5 pt
/// to whatever follows).
const WINDOW_TOOLS_GAP: f32 = 4.;
const WINDOW_TOOLS_SPAN: f32 = 28. + WINDOW_TOOLS_GAP + 28. + 5.;
/// The strip over the sidebar's edge that takes hold of it, in points.
const SIDEBAR_EDGE: f32 = 6.;

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
            // One layer at a time: the event picker folds back into the
            // details card it came out of, and the next Escape closes that.
            MenuKey::Dismiss => {
                self.dismiss_top(cx);
                self.focus.focus(window);
            }
        }
        cx.stop_propagation();
    }

    /// Whether the sidebar is not beside the page (closed, or the window is
    /// too narrow for it): the header then carries the sidebar button.
    pub(crate) fn sidebar_away(&self) -> bool {
        !self.sidebar_open
            || (self.sidebar_narrow && self.sidebar_level <= 0. && self.current_note.is_none())
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
        let level = pose.value.clamp(0., 1.);
        let width = self.sidebar_span;
        overlay(
            self.sidebar_overlay,
            cx.listener(|this, _, _, cx| {
                this.sidebar_overlay = false;
                cx.notify();
            }),
        )
        .bg(scrim(Scrim::Sidebar, pose.fade))
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
                        let viewport = f32::from(window.viewport_size().width);
                        let width = crate::desktop_preferences::sidebar_width_in(
                            f32::from(event.position.x) - grab,
                            viewport,
                        );
                        if width != this.sidebar_width {
                            // A tap where it meets its usual width and where it
                            // stops, as Finder's column edges do.
                            use crate::desktop_preferences::{
                                sidebar_width_in, SIDEBAR_DEFAULT, SIDEBAR_MAX, SIDEBAR_MIN,
                            };
                            if crate::haptics::edge_tap(
                                this.sidebar_width,
                                width,
                                sidebar_width_in(SIDEBAR_DEFAULT, viewport),
                                SIDEBAR_MIN,
                                sidebar_width_in(SIDEBAR_MAX, viewport),
                            ) {
                                crate::haptics::play(crate::haptics::Haptic::Alignment);
                            }
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
            // The strip beside the traffic lights. The sidebar button and
            // search are drawn over it by the window, once, so they stay
            // where they are while the sidebar slides away beneath them
            // (see `render_window_tools`).
            .child(div().h(px(48.)).flex_shrink_0())
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
        let (threads, thread_selected, threads_level) = self.render_sidebar_chat_threads(cx);
        let spaces = div()
            .flex()
            .flex_col()
            .flex_shrink_0()
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
            // The team space, under it (see `team_space.rs`). It is offered
            // once a server is signed in to: the row opens its own space
            // under "My notes" rather than popping in, and closes it again.
            .children(match self.team_space_row(cx) {
                Some(row) => {
                    crate::motion::appear(&self.stage, ("sidebar-team-space", 0), true, row)
                }
                None => crate::motion::vacate(&self.stage, ("sidebar-team-space", 0)),
            });
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
                    // The navigation gives way before Spaces and the footer
                    // do: a long list of conversations scrolls in the room
                    // that is left, and they stay where they are.
                    .min_h_0()
                    .gap(px(1.))
                    .child(self.nav_row("Home", "house", Screen::Home, true, cx))
                    .child(self.nav_row("Shared with me", "people", Screen::Shared, true, cx))
                    // The conversation on the page carries the highlight in
                    // Chat's place while it is listed beneath.
                    .child(
                        self.nav_row("Chat", "speech-bubble", Screen::Chat, !thread_selected, cx)
                            // Its conversations fold away under it: a chevron at
                            // the row's end, pointing down while they are out and
                            // turning to the right as they fold.
                            .when_some(threads_level, |row, level| {
                                row.child(
                                    div()
                                        .id("sidebar-chat-fold")
                                        .size(px(20.))
                                        .flex_shrink_0()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded(px(6.))
                                        .child(row_feedback(t, 6.))
                                        .child(
                                            icon("chevron-down", 12., t.muted.opacity(0.8))
                                                .with_transformation(Transformation::rotate(
                                                    radians(-std::f32::consts::FRAC_PI_2 * (1. - level)),
                                                )),
                                        )
                                        // On the press, which the row never sees: the
                                        // row's own click opens a new conversation.
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(|this, _, _, cx| {
                                                cx.stop_propagation();
                                                this.chat_threads_collapsed = !this.chat_threads_collapsed;
                                                cx.notify();
                                            }),
                                        ),
                                )
                            }),
                    )
                    .children(threads.map(|rows| {
                        let scroll = crate::lazy::scroll("sidebar-chats");
                        div()
                            .relative()
                            .flex()
                            .flex_col()
                            .min_h_0()
                            .child(
                                div()
                                    .id("sidebar-chat-scroll")
                                    .flex()
                                    .flex_col()
                                    .min_h_0()
                                    .overflow_y_scroll()
                                    .track_scroll(&scroll)
                                    .child(rows.flex_shrink_0()),
                            )
                            .child(scroll_thumb("sidebar-chats", &scroll))
                    })),
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
                    .flex_shrink_0()
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
                    // The workspace: the organisation's mark, as on the team space.
                    .child(self.space_tile("users"))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_weight(FontWeight::MEDIUM)
                            .child(self.workspace_name()),
                    )
                    // An update is waiting: the menu has its row.
                    .when(self.updates.known().is_some(), |row| row.child(self.updates.badge(6.)))
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

    /// Footer shortcuts. Their tooltips stand above the icons, clear of the
    /// workspace row beneath (the app's one tooltip, see `ui::tip`).
    fn render_sidebar_footer_tools(&self, cx: &mut Context<Self>) -> Div {
        let t = self.theme();
        let tool = |id: &'static str, symbol: &'static str, label: &'static str| {
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
                // The trash button stays lit while its popover is up (see
                // `hold_open_triggers`).
                .child(crate::motion::tactile(t.hover).press(t.fill).radius(8.))
                .child(icon(symbol, 16., t.muted))
                // Where it is drawn, for scripted runs that press it.
                .child(crate::onboarding_view::place(id))
                .child(tip(label, t).above())
        };
        div()
            .px(px(8.))
            .pb(px(6.))
            .flex()
            .flex_shrink_0()
            .items_center()
            .gap(px(1.))
            .child(
                tool("footer-shortcuts", "keyboard", "Keyboard shortcuts").on_click(cx.listener(
                    |this, _, _, cx| {
                        this.open_menu(Menu::Shortcuts);
                        cx.notify();
                    },
                )),
            )
            .child(
                tool("footer-trash", "trash", "Trash")
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_trash(cx))),
            )
    }

    /// Under Chat, while the Chat page is open: its conversations, newest
    /// first, each with its age; the eighth faded when more follow, and
    /// "Show N more" to list the rest. They unfold beneath Chat and fold
    /// back into it, 29 px a row, as the folders do.
    ///
    /// Also says whether one of the rows is the conversation on the page:
    /// that row then has the sidebar's highlight, not Chat.
    ///
    /// The list folds away and back with the chevron at the end of Chat's
    /// row (`chat_threads_collapsed`); the third value is how far open it
    /// is, for that chevron, when there is a list to fold.
    fn render_sidebar_chat_threads(&self, cx: &mut Context<Self>) -> (Option<Div>, bool, Option<f32>) {
        const SHOWN: usize = 8;
        let t = self.theme();
        let glide = self.stage.glide("sidebar");
        let mut selected = false;
        let open = self.screen == Screen::Chat && self.current_note.is_none();
        let threads = self.chat_threads();
        let foldable = open && !threads.is_empty();
        let pose = self.stage.presence(
            ("sidebar-chat-threads", 0),
            foldable && !self.chat_threads_collapsed,
            crate::motion::GOOEY,
        );
        let level = foldable.then(|| pose.value.clamp(0., 1.));
        if !pose.visible {
            return (None, false, level);
        }
        // Folded away, Chat has the highlight again.
        let open = open && !self.chat_threads_collapsed;
        // The list is the newest eight until "Show more"; the rest open
        // beneath them on the fold's own spring and close up the same way.
        // While they are out, or on their way back in, all are drawn.
        let short = threads.len().min(SHOWN);
        let longer = threads.len() > SHOWN;
        let rest = self.stage.presence(
            ("sidebar-chat-more", 0),
            foldable && longer && self.chat_threads_expanded,
            crate::motion::GOOEY,
        );
        let all = rest.visible;
        let listed = if all { threads.len() } else { short };
        // Those the servers have not sent yet.
        let unsent = self.chat_threads_more();
        // Those read but not listed, and those the servers have not sent.
        let more = threads.len() - listed + unsent;
        // Rows in the short list and in the whole one, with what ends each
        // ("Show N more"; "Show less").
        let short_rows = short + usize::from(threads.len() - short + unsent > 0);
        let long_rows = threads.len() + usize::from(unsent > 0) + 1;
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
        // A conversation that is new makes room for itself among the rows,
        // and one that is deleted leaves a space that closes up.
        let mut keys = Vec::with_capacity(listed);
        let mut drawn = std::collections::HashMap::with_capacity(listed);
        for (index, thread) in threads.into_iter().take(listed).enumerate() {
            let (id, account) = (thread.id.clone(), thread.account.clone());
            let key = crate::lazy::key(&thread.id);
            let shown = self.chat_thread_id == thread.id && !self.chat_messages.is_empty();
            keys.push(key);
            drawn.insert(
                key,
                div()
                    .id(("sidebar-chat-thread", key))
                    .h(px(28.))
                    .w_full()
                    .flex_shrink_0()
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
        for row in self.stage.roster("sidebar-chats", u64::from(all), &keys) {
            rows = rows.children(crate::motion::roster_row(
                &self.stage,
                "sidebar-chats",
                row,
                drawn.remove(&row.key),
            ));
        }
        if more > 0 {
            rows = rows.child(
                div()
                    .id("sidebar-chat-threads-more")
                    .h(px(28.))
                    .w_full()
                    .flex_shrink_0()
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
        if all {
            rows = rows.child(
                div()
                    .id("sidebar-chat-threads-less")
                    .h(px(28.))
                    .w_full()
                    .flex_shrink_0()
                    .pl(px(14.))
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .rounded(px(8.))
                    .text_color(t.secondary)
                    .cursor_default()
                    .child(row_feedback(t, 8.))
                    .child(icon("arrow-down", 16., t.secondary).with_transformation(
                        Transformation::rotate(radians(std::f32::consts::PI)),
                    ))
                    .child(div().flex_1().min_w_0().truncate().child("Show less"))
                    .reach()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.chat_threads_expanded = false;
                        crate::lazy::scroll("sidebar-chats").set_offset(Point::default());
                        cx.notify();
                    })),
            );
        }
        if !pose.at_rest() || (all && !rest.at_rest()) {
            let extra = if all { rest.value.clamp(0., 1.) } else { 0. };
            let natural =
                29. * (short_rows as f32 + (long_rows as f32 - short_rows as f32) * extra);
            rows = rows
                .h(px(natural * pose.value.clamp(0., 1.)))
                .overflow_hidden()
                .opacity(pose.fade);
        }
        (Some(rows), selected, level)
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
            .flex_shrink_0()
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
            return div()
                .relative()
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
                        // The two halves of the Share capsule: dark fills
                        // that wash and squash with the press like every
                        // other button (the fill belongs to the feedback
                        // layer, so it squashes too).
                        let half = |left: bool| {
                            let (near, far) = (14., 4.);
                            let feedback = crate::motion::tactile(neutral_wash(0.16))
                                .base(t.text)
                                .press(neutral_wash(0.14));
                            if left {
                                feedback.corners(near, far, far, near)
                            } else {
                                feedback.corners(far, near, near, far)
                            }
                        };
                        d.child(options.when(self.current_note.and_then(|id| self.workspace.note(id)).is_some_and(|note| !note.enhanced.trim().is_empty()), |options| options.child(
                            div().flex().gap(px(1.))
                                .child(div().id("note-share").h(px(28.)).px(px(11.)).rounded_l(px(14.)).rounded_r(px(4.)).text_color(t.bg).text_size(px(13.)).flex().items_center().gap(px(5.)).cursor_default().child(half(true)).child(icon("lock-solid",13.,t.bg)).child("Share").reach().on_click(cx.listener(|this,_,window,cx|{this.focus.focus(window);this.open_menu(Menu::Share);cx.notify();})))
                                .child(div().id("note-copy-share-link").w(px(30.)).h(px(28.)).rounded_l(px(4.)).rounded_r(px(14.)).flex().items_center().justify_center().cursor_default().child(half(false)).child(icon("link-chain",14.,t.bg)).child(tip("Copy link", t)).reach().on_click(cx.listener(|this,_,_,cx|{if let Some(id)=this.current_note { if crate::server::enabled() { this.server_action(id,crate::server::Action::Share,cx); } else { this.show_toast("Connect to your server to share a note",cx); } }})))
                        )))
                    },
                )
                // A recording running in another note says so here too.
                .children(self.render_session_pill(cx));
        }
        // With the sidebar away the Chat page's bar carries what the sidebar
        // did, as the reference's does: a History menu beside the back
        // pill (the threads are no longer listed at the side), and New chat
        // as a round icon button instead of a labelled pill.
        let sidebar_away = self.sidebar_away();
        let chat_bar = self.screen == Screen::Chat && sidebar_away;
        // Inside a conversation the pill steps back to the Chat page itself.
        // (A conversation opened from a space's page steps back to that space.)
        let in_thread = self.screen == Screen::Chat
            && !self.chat_messages.is_empty()
            && matches!(self.back_target(), Screen::Home | Screen::Chat);
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
                    // Past the traffic lights and the window's two tools,
                    // which stay where they are (see `render_window_tools`),
                    // easing as the sidebar slides away.
                    .ml(px((80. + WINDOW_TOOLS_SPAN) * (1. - self.sidebar_level)))
                    // Every page but Home can step back to where it came from.
                    .when_some(back_label(self.screen, self.back_target()), |d, label| {
                        let label = if in_thread { BackLabel::Icon("chat") } else { label };
                        d.child(back_pill("header-back", label, t).on_click(cx.listener(
                            move |this, _, window, cx| {
                                if in_thread {
                                    this.clear_chat(cx);
                                } else {
                                    this.go_back(cx);
                                }
                                this.focus.focus(window);
                            },
                        )))
                    })
                    .when(chat_bar, |d| {
                        d.child(
                            tool_tip("chat-bar-history", "chat-history", "Recent chats", t)
                                .w_auto()
                                .px(px(8.))
                                .gap(px(6.))
                                .child("History")
                                .child(icon("chevron-down", 12., t.muted))
                                .on_click(cx.listener(|this, _, _, cx| this.toggle_chat_history(cx))),
                        )
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
                        HeaderAction::NewChat if chat_bar => {
                            round_tool("new-chat", "edit", "New chat", t)
                                .size(px(30.))
                                .border_1()
                                .border_color(t.hairline)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.clear_chat(cx);
                                    this.ask.read(cx).focus_handle.focus(window);
                                }))
                        }
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
            .children(self.render_session_pill(cx))
    }

    /// Whether a recording is running somewhere other than the page on
    /// screen, which is when the session pill says so.
    pub(crate) fn session_pill_shown(&self) -> bool {
        self.recording.is_active()
            && self
                .recording
                .note_id
                .is_some_and(|id| self.workspace.note(id).is_some() && self.current_note != Some(id))
    }

    /// The session pill's width while it is in the title bar (what a toast
    /// there stays beside).
    pub(crate) fn session_pill_width(&self) -> Option<f32> {
        if !self.session_pill_shown() {
            return None;
        }
        let note = self.recording.note_id.and_then(|id| self.workspace.note(id))?;
        Some(session_pill_width(if note.title.trim().is_empty() {
            "New note"
        } else {
            &note.title
        }))
    }

    /// The session pill, in the middle of the title bar: it drops out of
    /// the top of the window while a recording runs in a note that is not
    /// the one on screen (every page, and every other note), and folds back
    /// up when the recording stops or its own note is opened. Pressing it
    /// goes to that note; its × stops the recording.
    fn render_session_pill(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let t = self.theme();
        let pose = self.stage.presence(
            ("active-session-pill", 0),
            self.session_pill_shown(),
            crate::motion::GOOEY,
        );
        let note = self
            .recording
            .note_id
            .and_then(|id| self.workspace.note(id))
            .filter(|_| pose.visible)?;
        let id = note.id;
        let title = if note.title.trim().is_empty() {
            "New note".to_owned()
        } else {
            note.title.clone()
        };
        // Dark, as in the reference: a near-black capsule with the title in
        // white, so it reads as the recording at a glance over either
        // theme. Wide enough for the whole title up to 240 pt.
        let width = session_pill_width(&title);
        let (fill, hover, ink) = if self.workspace.settings.dark_mode {
            (rgb(0x41403c), rgb(0x4b4a46), rgb(0xf6f4ee))
        } else {
            (rgb(0x2b2a27), rgb(0x3a3936), rgb(0xfbfaf6))
        };
        // While it is leaving nothing in it can be pressed.
        let live = self.session_pill_shown();
        Some(crate::motion::pop(
            div()
                .id("active-session-pill")
                .absolute()
                .top(px(5.))
                .left(relative(0.5))
                .ml(px(-width / 2. - self.sidebar_inset() / 2.))
                .w(px(width))
                .h(px(34.))
                .pl(px(11.))
                .pr(px(6.))
                .rounded_full()
                .bg(fill)
                .shadow(vec![BoxShadow {
                    color: hsla(0., 0., 0., 0.18),
                    offset: point(px(0.), px(2.)),
                    blur_radius: px(8.),
                    spread_radius: px(0.),
                }])
                .flex()
                .items_center()
                .gap(px(8.))
                .cursor(CursorStyle::Arrow)
                .child(crate::motion::tactile(hover.into()).round())
                .child(crate::recording_view::recording_waveform(
                    self.recording.audio_level(),
                    crate::recording_view::WAVEFORM.into(),
                ))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(14.))
                        .text_color(ink)
                        .child(title),
                )
                .reach()
                .on_click(cx.listener(move |this, _, window, cx| {
                    if live {
                        this.open_note(id, cx);
                        this.focus.focus(window);
                    }
                }))
                .child(
                    div()
                        .id("active-session-stop")
                        .size(px(24.))
                        .rounded_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        // The same eased wash and squash as every other
                        // control, in white on the dark capsule.
                        .child(
                            crate::motion::tactile(hsla(0., 0., 1., 0.12))
                                .press(hsla(0., 0., 1., 0.08))
                                .round(),
                        )
                        .child(icon("close", 13., ink.into()))
                        .child(tip("Stop recording", t).below())
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            if live {
                                this.focus.focus(window);
                                this.stop_recording_preview(cx);
                            }
                        })),
                ),
            crate::motion::Pop::chip((0.5, 0.)),
            pose,
        ))
    }

    /// The window's own two controls, beside the traffic lights: the
    /// sidebar button and search. One pair, drawn by the window over the
    /// sidebar's top strip and the page's header alike, so they never move
    /// and never double while the sidebar slides in or out beneath them;
    /// only the sidebar button's glyph changes. They fade with the pages
    /// that have no sidebar (a note, sign-in).
    pub(crate) fn render_window_tools(
        &self,
        login: bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let t = self.theme();
        let shown = window_tools_shown(
            login || self.screen == Screen::Link,
            self.current_note.is_some(),
            self.chat_maximized,
            self.sidebar_open && !self.sidebar_narrow,
        );
        let pose = self
            .stage
            .presence(("window-tools", 0), shown, crate::motion::SNAPPY);
        if !pose.visible {
            return None;
        }
        // Out, the button offers to hide the sidebar; away, to show it.
        let out = if self.sidebar_narrow {
            self.sidebar_overlay
        } else {
            self.sidebar_open
        };
        // One control whichever it offers: pressing it must not make it a
        // new control under the pointer (its tooltip would come back).
        let (glyph, label) = if out {
            ("sidebar-expanded", "Hide sidebar · ⌘B")
        } else {
            ("sidebar-collapsed", "Show sidebar · ⌘B")
        };
        Some(
            div()
                .absolute()
                // On the traffic lights' centre line (22 pt down).
                .top(px(8.))
                .left(px(88.))
                .flex()
                .items_center()
                .gap(px(WINDOW_TOOLS_GAP))
                .opacity(pose.fade.clamp(0., 1.))
                .child(
                    tool_sized_tip("sidebar-toggle", glyph, WINDOW_TOOL, label, t).on_click(cx.listener(
                        move |this, _, _, cx| {
                            if shown {
                                this.toggle_sidebar();
                                cx.notify();
                            }
                        },
                    )),
                )
                .child(
                    tool_sized("window-search", "search", WINDOW_TOOL, t).on_click(cx.listener(
                        move |this, _, window, cx| {
                            if shown {
                                this.open_search(window, cx)
                            }
                        },
                    )),
                )
                .into_any_element(),
        )
    }

    /// Keep the control that opened what is open lit while it is open (see
    /// [`crate::motion::hold`]). Called once a frame by the window.
    pub(crate) fn hold_open_triggers(&self) {
        use crate::motion::hold;
        match &self.menu {
            Some(Menu::Workspace) => hold("workspace-switcher"),
            Some(Menu::Trash) => hold("footer-trash"),
            Some(Menu::Note) => hold("note-options"),
            Some(Menu::Share) => hold("note-share"),
            Some(Menu::Templates) => hold("note-tab-Enhanced"),
            Some(Menu::NoteDetails | Menu::CalendarEvent) => hold("note-details"),
            Some(Menu::Theme) => hold("setting-theme"),
            Some(Menu::RecordingInput) => hold("transcript-microphone"),
            Some(Menu::RecordingSettings) => hold("transcript-audio-settings"),
            Some(Menu::RecordingLanguage) => hold("transcript-language"),
            // The row a context menu belongs to keeps its wash, and its "…".
            Some(Menu::NoteRow) => {
                if let Some(row) = self.row_menu {
                    hold(("note-row", row.note));
                }
            }
            _ => {}
        }
        if self.chat_history_open {
            hold("chat-bar-history");
            hold("full-chat-history");
        }
    }

    /// How far the title bar's own controls reach in from the window's left
    /// and right edges on the page showing: what a toast in the bar stays
    /// clear of.
    pub(crate) fn header_reach(&self) -> (f32, f32) {
        let note = self
            .current_note
            .and_then(|id| self.workspace.note(id));
        if let Some(note) = note {
            let right = if !note.enhanced.trim().is_empty() {
                8. + 110. + 8. + 28.
            } else if !note.body.trim().is_empty() {
                8. + 28.
            } else {
                0.
            };
            return (88. + 43., right);
        }
        // The traffic lights and the two tools, or the sidebar's edge when
        // that is further in; then the back pill and Chat's History.
        let mut left = (88. + WINDOW_TOOLS_SPAN - 5.).max(self.sidebar_inset());
        if back_label(self.screen, self.back_target()).is_some() {
            left += 8. + 60.;
        }
        if self.screen == Screen::Chat && self.sidebar_away() {
            left += 5. + 96.;
        }
        let right = match header_action(self.screen) {
            Some(_) => 8. + 104.,
            None => 0.,
        };
        (left, right)
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
        overlay(
            self.search_open,
            cx.listener(|this, _, window, cx| {
                this.search_open = false;
                this.focus.focus(window);
                cx.notify();
            }),
        )
        .bg(scrim(Scrim::Palette, pose.fade))
            // The palette drops out of the top of the window like a droplet.
            .child(crate::motion::pop(
                floating(div().id("search-dialog"))
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
                    // The card's bottom edge follows the results as typing
                    // changes how many there are.
                    .child(crate::motion::follow_height(
                        &self.stage,
                        ("palette-results", 0),
                        results,
                    )),
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
        // The layer the menu is drawn on: a press outside the menu closes it
        // and goes no further, and nothing under it answers the pointer.
        let mut overlay = crate::ui::overlay(
            self.menu.is_some(),
            cx.listener(|this, _, w, cx| {
                // The press may be on the button that opened this menu: remember it, so
                // the click it completes closes the menu rather than opening it again.
                if let Some(closed) = this.menu.take() {
                    this.menu_closed_by_press = Some((closed, this.press_seq));
                }
                this.menu_keyboard.selected = None;
                this.focus.focus(w);
                cx.notify();
            }),
        )
        // The pointer has left the menu (its body takes the pointer for
        // itself, so this is only ever outside it): no row is under it, and
        // the highlight eases away rather than staying on the last one.
        .on_mouse_move(cx.listener(|this, _, _, cx| {
            if this.menu.is_some() && this.menu_keyboard.selected.take().is_some() {
                cx.notify();
            }
        }));
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
            // Settings › Members: a row's menu, the invite dialog and the
            // question before someone is removed (`workspace_settings.rs`).
            Menu::Member | Menu::Invite | Menu::RemoveMember => {
                return self.render_organization_menu(menu, content, overlay, pose, window, cx);
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
                // It hangs from the chip that opened it, wherever the title
                // above has put that (a title of several lines moves it).
                let chip = crate::onboarding_view::placed("note-details");
                let (top, left) = hang_from(
                    chip,
                    4.,
                    280.,
                    (130., anchors.details_left),
                    window.viewport_size(),
                );
                return overlay
                    .child(
                        div()
                            .absolute()
                            .top(px(top))
                            .left(px(left))
                            .child(
                                div()
                                    .relative()
                                    .child(pop(
                                        floating(self.render_note_details(cx)),
                                        Pop::menu((0.15, 0.)),
                                        pose,
                                    ))
                                    .when(picker.visible, |d| {
                                        d.child(div().absolute().left(px(8.)).top(px(50.)).child(
                                            pop(
                                                floating(self.render_event_picker(cx)),
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
                        floating(panel),
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
                                    // The person: the personal mark, as on "My notes".
                                    .child(icon("lock-solid", 14., t.text)),
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
                    // An update that is known: a row of the menu's own
                    // kind, a dot of the brand's gold where its icon would
                    // be and the version where a shortcut would be.
                    .when_some(self.updates.menu_row(), |content, (label, version)| {
                        content.child(
                            self.menu_row_led(label, self.updates.badge(8.), cx, |this, _, cx| {
                                this.open_update(cx)
                            })
                            .child(div().flex_1())
                            .child(small(version.to_owned(), t).flex_shrink_0().pr(px(2.))),
                        )
                    })
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
                        .child(shortcut_hint("⌘,", t)),
                    );
                overlay = overlay.child(div().absolute().bottom(px(43.)).left(px(8.)).child(pop(
                    floating(content),
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
                    floating(content),
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
                    floating(content),
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
                            self.danger_row("Move to Trash", "trash", cx, |this, _, cx| {
                                if let Some(id) = this.current_note {
                                    this.trash_note(id, cx);
                                }
                            }),
                        );
                } else {
                    let regenerate_menu = self.menu.clone();
                    let regenerate_epoch = self.menu_epoch;
                    // The template in use is a row like the others, marked
                    // by its check: the highlight is the pointer's (or the
                    // arrow keys'), never a second fill of its own.
                    let (current_index, current_action) =
                        self.menu_keyboard.register(|this, _, cx| {
                            this.active_tab = crate::app::NoteTab::Enhanced;
                            this.menu = None;
                            cx.notify();
                        });
                    let (regenerate_index, regenerate_action) =
                        self.menu_keyboard.register(|this, _, cx| {
                            this.menu = None;
                            if let Some(id) = this.current_note {
                                this.enhance_note_preview(id, cx);
                            }
                        });
                    let current_menu = regenerate_menu.clone();
                    content = content.child(
                        div()
                            .id("enhanced-menu-selected")
                            .h(px(32.))
                            .px(px(8.))
                            .rounded(px(7.))
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_size(px(13.))
                            .cursor_default()
                            .child(crate::motion::tactile(t.border.opacity(0.)).press(t.fill).radius(7.))
                            .when(self.menu_keyboard.selected == Some(current_index), |d| {
                                d.child(self.stage.glide("menu").probe(current_index))
                            })
                            .on_mouse_move(cx.listener(move |this, _, _, cx| {
                                if this.menu_keyboard.selected != Some(current_index) {
                                    this.menu_keyboard.selected = Some(current_index);
                                    cx.notify();
                                }
                            }))
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
                                // The tool has the pointer, not the row
                                // around it: the row's highlight stays, and
                                // the arrow keys can still land on the tool.
                                .when(self.menu_keyboard.selected == Some(regenerate_index), |d| {
                                    d.bg(t.border)
                                })
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
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if this.menu.is_some()
                                    && this.menu == current_menu
                                    && this.menu_epoch == regenerate_epoch
                                {
                                    current_action(this, window, cx);
                                }
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
                            // Under the Enhanced tab, wherever the title
                            // above has put it.
                            let viewport = window.viewport_size();
                            let (top, left) = hang_from(
                                crate::onboarding_view::placed("note-templates"),
                                0.,
                                220.,
                                (130., f32::from(viewport.width) / 2. - 278.),
                                viewport,
                            );
                            d.top(px(top)).left(px(left))
                        })
                        .when(menu == Menu::Note, |d| {
                            let has_share = self
                                .current_note
                                .and_then(|id| self.workspace.note(id))
                                .is_some_and(|note| !note.enhanced.trim().is_empty());
                            d.top(px(40.)).right(px(if has_share { 120. } else { 8. }))
                        })
                        .child(pop(
                            floating(content),
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
                    // Each shortcut as the palette and the menus draw one:
                    // keycaps, and the keys the menu bar shows.
                    let rows: [(&str, &[&[&'static str]]); 8] = [
                        ("New note", &[&["⇧", "⌘", "N"]]),
                        ("Search notes", &[&["⌘", "K"], &["⌘", "F"]]),
                        ("Go home", &[&["⌘", "1"]]),
                        ("Toggle sidebar", &[&["⌘", "B"]]),
                        ("Save notes", &[&["⌘", "S"]]),
                        ("Settings", &[&["⌘", ","]]),
                        ("Dismiss / back", &[&["Esc"]]),
                        ("Undo / redo", &[&["⌘", "Z"], &["⇧", "⌘", "Z"]]),
                    ];
                    for (label, shortcuts) in rows {
                        let mut keys = div().flex().items_center().gap(px(6.));
                        for (index, shortcut) in shortcuts.iter().enumerate() {
                            if index > 0 {
                                keys = keys.child(small("or", t));
                            }
                            keys = keys.child(keycaps(shortcut, t));
                        }
                        content = content.child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .child(label)
                                .child(keys),
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
                        floating(content),
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
        self.menu_item(label, symbol, None, None, cx, action)
    }

    /// A menu row whose 16 px icon slot holds `lead` instead of an icon.
    pub(crate) fn menu_row_led(
        &self,
        label: impl Into<SharedString>,
        lead: impl IntoElement,
        cx: &mut Context<Self>,
        action: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> Stateful<Div> {
        let lead = div()
            .size(px(16.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .child(lead)
            .into_any_element();
        self.menu_item(label, "", None, Some(lead), cx, action)
    }

    /// A destructive menu row: label and icon both carry the danger colour.
    pub(crate) fn danger_row(
        &self,
        label: impl Into<SharedString>,
        symbol: &str,
        cx: &mut Context<Self>,
        action: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> Stateful<Div> {
        let danger = self.theme().danger;
        self.menu_item(label, symbol, Some(danger), None, cx, action)
    }

    fn menu_item(
        &self,
        label: impl Into<SharedString>,
        symbol: &str,
        tint: Option<Hsla>,
        lead: Option<AnyElement>,
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
            .children(lead)
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
            "New note"
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

/// How wide the session pill is for a note called `title`: the whole title
/// up to 240 pt.
fn session_pill_width(title: &str) -> f32 {
    (title.chars().count() as f32 * 7.6 + 78.).clamp(124., 240.)
}

/// Whether the window's own two controls (the sidebar button and search)
/// are there: on every page that can have the sidebar beside it. `bare` is
/// a page that never has one (sign-in, the welcome, a link's page). A note
/// has them only while its chat fills the window with the sidebar out.
pub fn window_tools_shown(bare: bool, note: bool, chat_maximized: bool, sidebar_out: bool) -> bool {
    !bare && (!note || (chat_maximized && sidebar_out))
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
            RowAction::MoveToTrash => "Move to Trash",
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

/// Where a popover `width` wide that hangs from a control goes: its top and
/// left, 6 pt under the control as it was last drawn at `control` and
/// `inset` in from its left edge, kept inside the window. `fallback` is
/// where it goes when the control has not been drawn.
pub fn hang_from(
    control: Option<Bounds<Pixels>>,
    inset: f32,
    width: f32,
    fallback: (f32, f32),
    viewport: Size<Pixels>,
) -> (f32, f32) {
    let (top, left) = match control {
        Some(control) => (
            f32::from(control.bottom()) + 6.,
            f32::from(control.left()) + inset,
        ),
        None => fallback,
    };
    let limit = (f32::from(viewport.width) - width - 12.).max(12.);
    (top, left.clamp(12., limit))
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
        // A pill that is only glyphs says where it goes.
        .child(
            tip(
                match id {
                    "back-to-notes" => "Home · ⌘1",
                    _ => "Back · Esc",
                },
                t,
            )
            .below(),
        )
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
            // For as long as the toast is up the note is one press from
            // being back where it was.
            self.show_toast_with(
                "Note moved to Trash",
                "Undo",
                move |this, window, cx| this.untrash_note(id, leave, window, cx),
                cx,
            );
        }
    }

    /// Undo of [`Cereal::trash_note`]: the note is out of Trash again, shown
    /// to whoever it was shared with, and back on screen if it was the one
    /// on screen (`reopen`). The team space is not told again: showing a
    /// note to the team is something its owner does.
    pub fn untrash_note(&mut self, id: u64, reopen: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(note) = self.workspace.note_mut(id) else {
            return;
        };
        if !note.trashed {
            return;
        }
        note.trashed = false;
        self.share_note_trashed(id, false, cx);
        self.persist(cx);
        if reopen {
            self.open_note(id, cx);
            self.focus.focus(window);
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
            ["Copy link", "Share", "Move to Trash"]
        );
        let destructive: Vec<_> = RowAction::ALL
            .into_iter()
            .filter(|action| action.destructive())
            .collect();
        assert_eq!(destructive, [RowAction::MoveToTrash]);
        assert_eq!(RowAction::ALL.last(), Some(&RowAction::MoveToTrash));
    }

    #[test]
    fn the_windows_two_tools_are_there_on_every_page_that_can_have_the_sidebar() {
        // A page with the sidebar beside it, or hidden: either way they are
        // there, and in the same place, so nothing moves when it is toggled.
        assert!(window_tools_shown(false, false, false, true));
        assert!(window_tools_shown(false, false, false, false));
        // Sign-in, the welcome and a link's page have neither.
        assert!(!window_tools_shown(true, false, false, true));
        // A note has the back pill there instead; its chat opened to the
        // whole window has the sidebar, and so the tools, only while the
        // sidebar is out.
        assert!(!window_tools_shown(false, true, false, true));
        assert!(window_tools_shown(false, true, true, true));
        assert!(!window_tools_shown(false, true, true, false));
    }

    #[test]
    fn a_popover_hangs_from_its_control_wherever_the_page_has_put_it() {
        let viewport = size(px(1240.), px(860.));
        // A one-line title: the chip is at y 96, the card 6 pt under it.
        let chip = Bounds::new(point(px(430.), px(96.)), size(px(160.), px(28.)));
        assert_eq!(hang_from(Some(chip), 4., 280., (130., 443.), viewport), (130., 434.));
        // A title of three lines pushes the chip down: the card follows it
        // instead of opening over the chip (it used to stay at 130).
        let lower = Bounds::new(point(px(430.), px(168.)), size(px(160.), px(28.)));
        assert_eq!(hang_from(Some(lower), 4., 280., (130., 443.), viewport), (202., 434.));
        // Near the window's right edge it slides back in.
        let far = Bounds::new(point(px(1100.), px(96.)), size(px(120.), px(28.)));
        assert_eq!(hang_from(Some(far), 0., 280., (130., 443.), viewport).1, 1240. - 280. - 12.);
        // Not drawn yet: where it used to go.
        assert_eq!(hang_from(None, 4., 280., (130., 443.), viewport), (130., 443.));
        // A window narrower than the popover keeps the margin on the left.
        assert_eq!(hang_from(Some(chip), 0., 280., (130., 443.), size(px(200.), px(400.))).1, 12.);
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
        // The workspace's own pages are a second group, listed only while
        // someone is signed in to a server: its name and its people. Nothing
        // the server has no endpoint for (Spaces, Analytics, Billing).
        let workspace: Vec<_> = SettingsTab::WORKSPACE.iter().map(|tab| tab.label()).collect();
        assert_eq!(workspace, ["General", "Members"]);
        assert_eq!(
            SettingsTab::WORKSPACE.map(|tab| tab.icon()),
            ["building", "users"]
        );
        assert!(SettingsTab::WORKSPACE.iter().all(|tab| tab.is_workspace()));
        assert!(SettingsTab::ALL.iter().all(|tab| !tab.is_workspace()));
        for tab in SettingsTab::WORKSPACE {
            assert!(!SettingsTab::ALL.contains(&tab));
            assert!(!icons.contains(&tab.icon()));
        }
    }
}
