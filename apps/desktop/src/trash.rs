//! Trash: the popover the sidebar's trash button opens, and what its rows do.
//!
//! A row restores its note, or deletes it forever after an inline
//! confirmation. A note of an account is deleted on its server first, through
//! the account's serial worker, and only then erased from this Mac; a note
//! of this Mac only, or one its server already deleted, is erased here.
use crate::{
    app::{Cereal, Menu, Screen},
    model::Note,
    motion::{Pop, Pose, pop},
    ui::*,
};
use gpui::{prelude::*, *};
use std::{path::Path, sync::mpsc, time::Duration};

/// What the header says under "Trash". Nothing empties Trash by itself: the
/// app keeps a trashed note until it is deleted forever, and the server has
/// no trash of its own (it deletes a note only when asked).
pub const SUBTITLE: &str = "Notes stay here until you delete them forever";
pub const EMPTY: &str = "Trash is empty";

/// The popover's geometry in points, measured from the reference capture at
/// 2 px per point (`.local/parity/trash`).
pub mod size {
    pub const CARD_WIDTH: f32 = 356.;
    /// The card keeps this height however many notes it lists.
    pub const CARD_HEIGHT: f32 = 391.;
    pub const CARD_RADIUS: f32 = 14.;
    /// From the window's left edge to the card, at least.
    pub const CARD_LEFT: f32 = 160.;
    /// From the trash button's right edge to the card, at least.
    pub const BESIDE_BUTTON: f32 = 7.5;
    /// From the window's bottom edge to the card.
    pub const ABOVE_BOTTOM: f32 = 15.;
    /// The card never rises closer than this to the window's top.
    pub const BELOW_TOP: f32 = 48.;
    pub const HEADER_X: f32 = 16.;
    pub const HEADER_TOP: f32 = 8.75;
    /// To the divider, below the card's 0.5 pt outline.
    pub const HEADER_HEIGHT: f32 = 54.;
    pub const TITLE_SIZE: f32 = 13.;
    pub const TITLE_LINE: f32 = 18.;
    pub const SUBTITLE_SIZE: f32 = 11.;
    pub const SUBTITLE_LINE: f32 = 14.;
    pub const SUBTITLE_GAP: f32 = 2.15;
    /// Between the divider and the first row's box.
    pub const LIST_TOP: f32 = 3.5;
    /// The rows' highlight is inset this far from the card's sides.
    pub const LIST_X: f32 = 6.;
    pub const ROW_X: f32 = 8.;
    pub const ROW_HEIGHT: f32 = 37.;
    pub const ROW_GAP: f32 = 8.;
    pub const TILE: f32 = 24.;
    pub const TILE_RADIUS: f32 = 7.;
    pub const TILE_ICON: f32 = 13.;
    pub const BUTTON: f32 = 28.;
    pub const BUTTON_GAP: f32 = 2.;
    pub const BUTTON_ICON: f32 = 16.;
    /// Where the trash button sits in the sidebar until it has been drawn
    /// (afterwards its drawn bounds are used): the footer's left padding
    /// plus the keyboard button (28) and the 1 pt between them.
    pub const BUTTON_LEFT: f32 = 8. + 28. + 1.;
    /// From the window's bottom edge to the trash button's centre: the
    /// workspace row (43), its hairline (1), the footer's bottom padding (6)
    /// and half the button.
    pub const BUTTON_CENTRE_FROM_BOTTOM: f32 = 43. + 1. + 6. + 14.;
}

/// Scripted runs press rows by these names (the first few rows only).
const RESTORE_PLACES: [&str; 6] = [
    "trash-restore-0",
    "trash-restore-1",
    "trash-restore-2",
    "trash-restore-3",
    "trash-restore-4",
    "trash-restore-5",
];
const DELETE_PLACES: [&str; 6] = [
    "trash-delete-0",
    "trash-delete-1",
    "trash-delete-2",
    "trash-delete-3",
    "trash-delete-4",
    "trash-delete-5",
];
const CONFIRM_PLACES: [&str; 6] = [
    "trash-confirm-0",
    "trash-confirm-1",
    "trash-confirm-2",
    "trash-confirm-3",
    "trash-confirm-4",
    "trash-confirm-5",
];
const CANCEL_PLACES: [&str; 6] = [
    "trash-cancel-0",
    "trash-cancel-1",
    "trash-cancel-2",
    "trash-cancel-3",
    "trash-cancel-4",
    "trash-cancel-5",
];

/// What a Trash row shows of its note.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrashRow {
    pub id: u64,
    pub title: String,
    /// Its own server answered that it no longer has the note.
    pub gone: bool,
}

/// The rows of the popover, in the order the Trash page lists them.
pub fn trash_rows(workspace: &crate::model::Workspace) -> Vec<TrashRow> {
    workspace
        .notes
        .iter()
        .filter(|note| note.trashed && !workspace.is_hidden(note))
        .map(|note| TrashRow {
            id: note.id,
            title: if note.title.trim().is_empty() {
                "Untitled note".into()
            } else {
                note.title.clone()
            },
            gone: note.server_missing_since.is_some(),
        })
        .collect()
}

/// Where deleting a note forever has to happen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeleteTarget {
    /// Only this Mac has it: local mode, or its server already deleted it.
    ThisMac,
    /// Its account's server may have it: it is deleted there first.
    Server,
}

pub fn delete_target(note: &Note, server_mode: bool) -> DeleteTarget {
    if server_mode && note.server_missing_since.is_none() {
        DeleteTarget::Server
    } else {
        DeleteTarget::ThisMac
    }
}

/// What the confirmation says the delete will do.
pub fn confirm_detail(target: DeleteTarget, host: Option<&str>) -> String {
    match (target, host) {
        (DeleteTarget::Server, Some(host)) => {
            format!("It is deleted from {host} too. This can’t be undone.")
        }
        _ => "This can’t be undone.".into(),
    }
}

/// What a Trash row is asking, if anything. While a row asks, the arrow
/// keys and Return stay on it (Return cancels), and Escape lets go of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ask {
    /// "Delete “…” forever?"
    Delete(u64),
    /// The server's copy was edited elsewhere since it was trashed:
    /// Restore, or Delete anyway.
    Changed(u64),
}

impl Ask {
    pub fn note(self) -> u64 {
        match self {
            Self::Delete(id) | Self::Changed(id) => id,
        }
    }
}

pub const CHANGED: &str = "This note was edited elsewhere since it was trashed";

/// Erase what this Mac keeps for a note under its recordings directory: its
/// own capture directory (`note-<id>`) and, for a note of a server, the
/// association sidecar and the identities of its uploaded parts.
///
/// Only that directory is removed. The note's saved capture entries are
/// never followed: one that points anywhere else (another note's capture, a
/// corrupt or foreign manifest) is left alone. A directory that is a
/// symbolic link loses the link, never what it points at.
pub fn erase_files(recordings: &Path, note: &Note) {
    let own = crate::audio::capture_directory(recordings, note.id);
    if own.parent() == Some(recordings) {
        match std::fs::symlink_metadata(&own) {
            Ok(meta) if meta.file_type().is_symlink() || meta.is_file() => {
                let _ = std::fs::remove_file(&own);
            }
            Ok(meta) if meta.is_dir() => {
                // `remove_dir_all` does not follow links inside either.
                let _ = std::fs::remove_dir_all(&own);
            }
            _ => {}
        }
    }
    if let Some(url) = &note.server_url {
        let sidecar = crate::server::association_file(recordings, url, note.id);
        if let (Some(id), Some(directory)) = (&note.server_id, sidecar.parent())
            && uuid::Uuid::parse_str(id).is_ok()
        {
            let _ = std::fs::remove_dir_all(directory.join("uploads").join(id));
        }
        let _ = std::fs::remove_file(sidecar);
    }
}

/// Where following a link to a note leads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Linked {
    /// The note itself.
    Note,
    /// The Trash, where the note can be restored from: a note in the Trash
    /// is never shown as a live one. With its row in the popover, when the
    /// popover lists it (see [`trash_rows`]).
    Trash(Option<usize>),
    /// No such note here.
    Gone,
}

/// What a link to note `id` opens.
pub fn linked(workspace: &crate::model::Workspace, id: u64) -> Linked {
    match workspace.note(id) {
        None => Linked::Gone,
        Some(note) if note.trashed => {
            Linked::Trash(trash_rows(workspace).iter().position(|row| row.id == id))
        }
        Some(_) => Linked::Note,
    }
}

impl Cereal {
    /// Follow a link to note `id`: the note, or the Trash with its row in
    /// view when that is where the note is.
    pub fn open_linked_note(&mut self, id: u64, cx: &mut Context<Self>) {
        match linked(&self.workspace, id) {
            Linked::Note => self.open_note(id, cx),
            Linked::Trash(_) => {
                self.open_trash_at(id, cx);
                self.show_toast("This note is in the Trash", cx);
            }
            Linked::Gone => self.show_toast("This note is no longer available.", cx),
        }
    }

    /// [`Cereal::open_trash`], with the row of note `id` highlighted and
    /// scrolled into view.
    pub fn open_trash_at(&mut self, id: u64, cx: &mut Context<Self>) {
        self.open_trash(cx);
        self.trash_reveal = Some(id);
    }

    /// Once the popover's rows are this frame's: highlight the row a link
    /// led to and bring it into view. Each row is one child of the list and
    /// one keyboard entry, in the order of [`trash_rows`].
    pub(crate) fn reveal_trash_row(&mut self) {
        if self.menu != Some(Menu::Trash) {
            return;
        }
        let Some(id) = self.trash_reveal.take() else {
            return;
        };
        if let Linked::Trash(Some(row)) = linked(&self.workspace, id) {
            self.menu_keyboard.selected = Some(row);
            self.trash_scroll.scroll_to_item(row);
        }
    }

    /// The trash button: opens the popover, or closes it when it is open.
    /// A press outside the open popover has already closed it by the time
    /// the button's click arrives; [`Cereal::open_menu`] keeps it closed.
    pub fn toggle_trash(&mut self, cx: &mut Context<Self>) {
        if self.menu == Some(Menu::Trash) {
            self.menu = None;
        } else if self.open_menu(Menu::Trash) {
            self.trash_ask = None;
        }
        cx.notify();
    }

    /// Trash from anywhere else (the command palette, a link to a trashed
    /// note): the popover belongs to the sidebar's footer, so the sidebar is
    /// brought up first, on Home when the page showing has none.
    pub fn open_trash(&mut self, cx: &mut Context<Self>) {
        if !self.trash_button_shown() {
            self.sidebar_open = true;
            if !self.trash_button_shown() {
                self.navigate(Screen::Home, cx);
            }
        }
        self.search_open = false;
        self.menu = Some(Menu::Trash);
        self.trash_ask = None;
        cx.notify();
    }

    /// Whether the sidebar's footer, and so the trash button, is on screen.
    pub(crate) fn trash_button_shown(&self) -> bool {
        let login = matches!(
            self.screen,
            Screen::Login | Screen::Account | Screen::Welcome
        );
        let settings = self.screen == Screen::Settings && self.current_note.is_none();
        self.sidebar_open
            && !login
            && !settings
            && self.screen != Screen::Link
            && (self.current_note.is_none() || self.chat_maximized)
    }

    /// A row asks something: the keyboard highlight goes to it and stays.
    fn ask_in_row(&mut self, ask: Ask, cx: &mut Context<Self>) {
        // Each row registers one keyboard entry, in the order of the rows.
        if let Some(index) = trash_rows(&self.workspace)
            .iter()
            .position(|row| row.id == ask.note())
        {
            self.menu_keyboard.selected = Some(index);
        }
        self.trash_ask = Some(ask);
        cx.notify();
    }

    /// Ask in the row before deleting its note forever.
    pub fn ask_delete_forever(&mut self, id: u64, cx: &mut Context<Self>) {
        self.ask_in_row(Ask::Delete(id), cx);
    }

    /// The keyboard while a row is asking: the arrows stay on it, Return
    /// lets go of the question (it never deletes). True when handled.
    pub(crate) fn trash_key_while_asking(&mut self, moving: bool, cx: &mut Context<Self>) -> bool {
        if self.menu != Some(Menu::Trash) || self.trash_ask.is_none() {
            return false;
        }
        if !moving {
            self.trash_ask = None;
        }
        cx.notify();
        true
    }

    /// Delete a trashed note forever (confirmed in its row). `anyway`: the
    /// person was told its server copy changed and chose to delete it.
    pub fn delete_forever(&mut self, id: u64, anyway: bool, cx: &mut Context<Self>) {
        self.trash_ask = None;
        let Some(note) = self.workspace.note(id).cloned() else {
            return;
        };
        if !note.trashed || self.deleting_forever.contains(&id) {
            return;
        }
        match delete_target(&note, crate::server::enabled()) {
            DeleteTarget::ThisMac => self.erase_note(id, cx),
            DeleteTarget::Server => {
                let Some(account) = crate::accounts::of_note(id) else {
                    self.show_toast("This note’s account is not on this Mac", cx);
                    return;
                };
                if !account.shown() {
                    // Nothing reaches a server whose session has ended: the
                    // note stays in Trash until the sign-in.
                    self.show_toast(
                        &format!("Sign in to {} to delete this note there", account.host()),
                        cx,
                    );
                    return;
                }
                let conditional = account.features.iter().any(|feature| feature == "conditional-delete");
                let account_id = account.id.clone();
                let receiver = self
                    .bridges
                    .entry(account_id.clone())
                    .or_insert_with(|| crate::server::Bridge::new(account_id))
                    .delete(note, anyway, conditional);
                self.deleting_forever.insert(id);
                cx.notify();
                cx.spawn(async move |this, cx| {
                    use crate::server::DeleteFailure;
                    let result = loop {
                        match receiver.try_recv() {
                            Ok(result) => break result,
                            Err(mpsc::TryRecvError::Disconnected) => {
                                break Err(DeleteFailure::Failed(
                                    "Server worker stopped. The note is still in Trash.".into(),
                                ));
                            }
                            Err(mpsc::TryRecvError::Empty) => {
                                cx.background_executor()
                                    .timer(Duration::from_millis(100))
                                    .await
                            }
                        }
                    };
                    let _ = this.update(cx, |this, cx| {
                        this.deleting_forever.remove(&id);
                        match result {
                            Ok(()) => this.erase_note(id, cx),
                            Err(DeleteFailure::Changed) if this.menu == Some(Menu::Trash) => {
                                this.ask_in_row(Ask::Changed(id), cx)
                            }
                            Err(DeleteFailure::Changed) => this.show_toast(CHANGED, cx),
                            Err(DeleteFailure::Failed(error)) => this.show_toast(&error, cx),
                        }
                        cx.notify();
                    });
                })
                .detach();
            }
        }
    }

    /// Take a note off this Mac for good: out of the workspace, with its
    /// own captures, its server association files and its chat.
    fn erase_note(&mut self, id: u64, cx: &mut Context<Self>) {
        let recordings = crate::accounts::handle::slot_of(id)
            .and_then(|slot| self.workspace.slots.get(&slot))
            .map(|slot| slot.recordings.clone())
            .or_else(|| crate::model::audio_directory_of(id).ok());
        let Some(note) = self.workspace.remove_note(id) else {
            return;
        };
        if let Some(recordings) = recordings {
            erase_files(&recordings, &note);
        }
        // Its chat goes too. The threads are kept on this Mac; when they
        // move to the server, this is where the note's thread is deleted.
        let threads = self.chat_archives.len();
        self.chat_archives.retain(|chat| chat.note_id != id);
        if self.chat_archives.len() != threads {
            self.persist_chat_archives();
        }
        if self.trash_ask.is_some_and(|ask| ask.note() == id) {
            self.trash_ask = None;
        }
        if self.persist(cx) {
            self.show_toast("Note deleted forever", cx);
        }
        cx.notify();
    }

    /// The Trash popover, placed beside the trash button and growing out of
    /// it. Drawn inside the menu overlay, which closes it on an outside press.
    pub(crate) fn render_trash_popover(
        &self,
        pose: Pose,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use size::*;
        let t = self.theme();
        // The card's outline and the header's divider: one device pixel
        // (half a point on a Retina display), never rounded away.
        let hairline = 1. / window.scale_factor().max(1.);
        let viewport = window.viewport_size();
        let height = CARD_HEIGHT
            .min(f32::from(viewport.height) - ABOVE_BOTTOM - BELOW_TOP)
            .max(120.);
        // Beside the trash button as it was last drawn (the footer's own
        // layout decides where that is); the footer's usual place before it
        // has been drawn once.
        let window_height = f32::from(viewport.height);
        let (right, centre_y) = crate::onboarding_view::placed("footer-trash")
            .map(|button| (f32::from(button.right()), f32::from(button.center().y)))
            .unwrap_or((
                192. * (self.sidebar_level - 1.) + BUTTON_LEFT + BUTTON,
                window_height - BUTTON_CENTRE_FROM_BOTTOM,
            ));
        // Clear of the workspace's name in the row under the button, as in
        // the reference (whose trash button is the fifth in its footer).
        let left = (right + BESIDE_BUTTON).max(192. * (self.sidebar_level - 1.) + CARD_LEFT);
        // The card grows out of the point level with the button's centre.
        let top = window_height - ABOVE_BOTTOM - height;
        let anchor_y = ((centre_y - top) / height).clamp(0., 1.);
        let rows = trash_rows(&self.workspace);
        let open_menu = self.visible_menu.clone().or(self.menu.clone());
        let menu_epoch = self.menu_epoch;
        let live = move |this: &Cereal| {
            this.menu.is_some() && this.menu == open_menu && this.menu_epoch == menu_epoch
        };
        let header = div()
            .flex_shrink_0()
            .h(px(HEADER_HEIGHT))
            .px(px(HEADER_X))
            .pt(px(HEADER_TOP))
            .flex()
            .flex_col()
            .gap(px(SUBTITLE_GAP))
            .child(
                div()
                    .text_size(px(TITLE_SIZE))
                    .line_height(px(TITLE_LINE))
                    .text_color(t.text)
                    .child("Trash"),
            )
            .child(
                div()
                    .text_size(px(SUBTITLE_SIZE))
                    .line_height(px(SUBTITLE_LINE))
                    .text_color(t.muted)
                    .truncate()
                    .child(SUBTITLE),
            );
        let mut list = div()
            .id("trash-list")
            .track_scroll(&self.trash_scroll)
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .px(px(LIST_X))
            .pt(px(LIST_TOP))
            .pb(px(LIST_X));
        if rows.is_empty() {
            list = list.items_center().justify_center().pb(px(HEADER_HEIGHT / 2.)).child(
                div()
                    .text_size(px(13.))
                    .line_height(px(18.))
                    .text_color(t.muted)
                    .child(EMPTY),
            );
        }
        for (position, row) in rows.into_iter().enumerate() {
            list = list.child(self.trash_row(position, row, live.clone(), cx));
        }
        let card = div()
            .w(px(CARD_WIDTH))
            .h(px(height))
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded(px(CARD_RADIUS))
            .bg(t.card)
            .border(px(hairline))
            .border_color(t.hairline.opacity(1.5))
            // Fitted to the reference's falloff below, beside and above the
            // card: a soft drop and a tight contact shadow.
            .shadow(vec![
                BoxShadow {
                    color: hsla(0., 0., 0., 0.09),
                    offset: point(px(0.), px(8.)),
                    blur_radius: px(5.),
                    spread_radius: px(0.),
                },
                BoxShadow {
                    color: hsla(0., 0., 0., 0.03),
                    offset: point(px(0.), px(2.)),
                    blur_radius: px(0.5),
                    spread_radius: px(0.),
                },
            ])
            .text_size(px(13.))
            .text_color(t.text)
            .child(crate::onboarding_view::place("trash-popover"))
            .child(header)
            .child(div().flex_shrink_0().h(px(hairline)).bg(t.hairline))
            .child(list.child(self.stage.glide("menu").indicator(t.hover, 8.)))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
        div()
            .absolute()
            .left(px(left))
            .bottom(px(ABOVE_BOTTOM))
            .child(pop(card, Pop::menu((0., anchor_y)), pose))
            .into_any_element()
    }

    fn trash_row(
        &self,
        position: usize,
        row: TrashRow,
        live: impl Fn(&Cereal) -> bool + Clone + 'static,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        use size::*;
        let t = self.theme();
        let id = row.id;
        let asking = self.trash_ask.filter(|ask| ask.note() == id);
        let busy = self.deleting_forever.contains(&id);
        // Return restores the highlighted note; in a row that is asking, it
        // lets go of the question (Return never deletes). While any row asks,
        // the keys stay on it (`trash_key_while_asking`).
        let (index, _) = self.menu_keyboard.register(move |this, _, cx| {
            if this.trash_ask.is_some() {
                this.trash_ask = None;
                cx.notify();
            } else if !this.deleting_forever.contains(&id) {
                this.restore_from_trash(id, cx);
            }
        });
        let place = |names: &[&'static str; 6]| names.get(position).copied();
        let tile = div()
            .size(px(TILE))
            .flex_shrink_0()
            .rounded(px(TILE_RADIUS))
            .bg(t.hover)
            .flex()
            .items_center()
            .justify_center()
            .child(icon("document", TILE_ICON, t.muted.opacity(0.6)));
        let button = |name: &'static str,
                      symbol: &'static str,
                      label: &'static str,
                      spot: Option<&'static str>| {
            div()
                .id((name, id))
                .relative()
                .size(px(BUTTON))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(6.))
                .cursor_default()
                .child(crate::motion::tactile(t.hover).press(t.fill).radius(6.))
                .child(icon(symbol, BUTTON_ICON, t.muted))
                .when_some(spot, |d, spot| d.child(crate::onboarding_view::place(spot)))
                // The app's tooltips stay hidden while a popover is open;
                // these belong to one, so they are shown.
                .tooltip(move |_, cx| cx.new(|_| ToolTip::new(label, t)).into())
        };
        let action = |id: (&'static str, u64), label: &'static str, danger: bool, spot: Option<&'static str>| {
            pill(id, label, "", t)
                .h(px(BUTTON - 2.))
                .flex_shrink_0()
                .when(danger, |d| d.bg(t.danger).border_color(t.danger).text_color(gpui::white()))
                .when_some(spot, |d, spot| d.relative().child(crate::onboarding_view::place(spot)))
        };
        let mut element = div()
            .id(("trash-row", id))
            .relative()
            .flex_shrink_0()
            .min_h(px(ROW_HEIGHT))
            .px(px(ROW_X))
            .flex()
            .items_center()
            .gap(px(ROW_GAP))
            .rounded(px(8.))
            .when(self.menu_keyboard.selected == Some(index), |d| {
                d.child(self.stage.glide("menu").probe(index))
            })
            .on_mouse_move(cx.listener(move |this, _, _, cx| {
                // A row that is asking keeps the highlight.
                if this.trash_ask.is_none() && this.menu_keyboard.selected != Some(index) {
                    this.menu_keyboard.selected = Some(index);
                    cx.notify();
                }
            }))
            .child(tile);
        match asking {
            Some(Ask::Delete(_)) => {
                let target = self
                    .workspace
                    .note(id)
                    .map(|note| delete_target(note, crate::server::enabled()))
                    .unwrap_or(DeleteTarget::ThisMac);
                let host = crate::accounts::of_note(id).map(|account| account.host());
                let live_cancel = live.clone();
                let live_confirm = live;
                element = element
                    .py(px(6.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            // The title gives way; "forever?" always shows.
                            .child(
                                div()
                                    .flex()
                                    .min_w_0()
                                    .child(div().flex_shrink_0().child("Delete “"))
                                    .child(div().min_w_0().truncate().child(row.title))
                                    .child(div().flex_shrink_0().child("” forever?")),
                            )
                            .child(small(confirm_detail(target, host.as_deref()), t).whitespace_normal()),
                    )
                    .child(action(("trash-cancel", id), "Cancel", false, place(&CANCEL_PLACES)).on_click(
                        cx.listener(move |this, _, _, cx| {
                            if live_cancel(this) {
                                this.trash_ask = None;
                                cx.notify();
                            }
                        }),
                    ))
                    .child(action(("trash-confirm", id), "Delete", true, place(&CONFIRM_PLACES)).on_click(
                        cx.listener(move |this, _, _, cx| {
                            if live_confirm(this) {
                                this.delete_forever(id, false, cx);
                            }
                        }),
                    ));
            }
            Some(Ask::Changed(_)) => {
                let live_restore = live.clone();
                let live_anyway = live;
                element = element
                    .py(px(6.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(div().truncate().child(row.title))
                            .child(small(CHANGED, t).whitespace_normal()),
                    )
                    .child(action(("trash-changed-restore", id), "Restore", false, place(&CANCEL_PLACES)).on_click(
                        cx.listener(move |this, _, _, cx| {
                            if live_restore(this) {
                                this.trash_ask = None;
                                this.restore_from_trash(id, cx);
                            }
                        }),
                    ))
                    .child(
                        action(("trash-anyway", id), "Delete anyway", true, place(&CONFIRM_PLACES)).on_click(
                            cx.listener(move |this, _, _, cx| {
                                if live_anyway(this) {
                                    this.delete_forever(id, true, cx);
                                }
                            }),
                        ),
                    );
            }
            None => {
                let live_restore = live.clone();
                let live_delete = live;
                element = element
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(div().min_w_0().truncate().child(row.title))
                            // As on the Trash page: its own server no longer has it.
                            .when(row.gone, |d| {
                                d.child(
                                    div()
                                        .flex_shrink_0()
                                        .text_size(px(SUBTITLE_SIZE))
                                        .text_color(t.muted)
                                        .child(crate::screens::DELETED_ON_SERVER),
                                )
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_shrink_0()
                            .items_center()
                            .gap(px(BUTTON_GAP))
                            .when(busy, |d| d.opacity(0.45))
                            .child(
                                button("trash-restore", "restore", "Restore", place(&RESTORE_PLACES))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if live_restore(this) && !this.deleting_forever.contains(&id) {
                                            this.restore_from_trash(id, cx);
                                        }
                                    })),
                            )
                            .child(
                                button("trash-delete", "trash", "Delete forever", place(&DELETE_PLACES))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if live_delete(this) && !this.deleting_forever.contains(&id) {
                                            this.ask_delete_forever(id, cx);
                                        }
                                    })),
                            ),
                    );
            }
        }
        element
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Workspace;
    // `gpui::*` re-exports its own `test` attribute; these are plain unit tests.
    use std::prelude::v1::test;

    fn note(id: u64, title: &str, trashed: bool) -> Note {
        Note {
            id,
            title: title.into(),
            trashed,
            ..Default::default()
        }
    }

    #[test]
    fn the_popover_lists_trashed_notes_only_and_names_untitled_ones() {
        let mut gone = note(3, "Gone from the server", true);
        gone.server_missing_since = Some("2026-11-16T10:00:00Z".into());
        let mut workspace = Workspace::default();
        workspace.notes = vec![note(1, "Kept", false), note(2, "  ", true), gone];
        assert_eq!(
            trash_rows(&workspace),
            [
                TrashRow { id: 2, title: "Untitled note".into(), gone: false },
                TrashRow { id: 3, title: "Gone from the server".into(), gone: true },
            ]
        );
    }

    #[test]
    fn a_link_to_a_trashed_note_leads_to_its_row_in_the_trash_never_to_the_note() {
        let mut workspace = Workspace::default();
        workspace.notes = vec![
            note(1, "Live", false),
            note(2, "Trashed first", true),
            note(3, "Live too", false),
            note(4, "Trashed second", true),
        ];
        assert_eq!(linked(&workspace, 1), Linked::Note);
        assert_eq!(linked(&workspace, 3), Linked::Note);
        // The row is the note's place among the popover's rows, not among
        // all notes.
        assert_eq!(linked(&workspace, 2), Linked::Trash(Some(0)));
        assert_eq!(linked(&workspace, 4), Linked::Trash(Some(1)));
        assert_eq!(trash_rows(&workspace)[1].id, 4);
        assert_eq!(linked(&workspace, 9), Linked::Gone);
        // Restored, the same link opens the note again; trashed, it stops.
        workspace.note_mut(4).unwrap().trashed = false;
        assert_eq!(linked(&workspace, 4), Linked::Note);
        workspace.note_mut(1).unwrap().trashed = true;
        assert_eq!(linked(&workspace, 1), Linked::Trash(Some(0)));
        assert_eq!(linked(&workspace, 2), Linked::Trash(Some(1)));
    }

    #[test]
    fn a_note_goes_to_its_server_unless_the_server_already_deleted_it() {
        let mut on_server = note(1, "On the server", true);
        on_server.server_id = Some("3a49a4af-c660-42e9-a36a-a806fbb41e99".into());
        assert_eq!(delete_target(&on_server, true), DeleteTarget::Server);
        // Never sent yet: the worker still looks for a sidecar first.
        assert_eq!(delete_target(&note(2, "Unsent", true), true), DeleteTarget::Server);
        let mut gone = on_server.clone();
        gone.server_missing_since = Some("now".into());
        assert_eq!(delete_target(&gone, true), DeleteTarget::ThisMac);
        assert_eq!(delete_target(&on_server, false), DeleteTarget::ThisMac);
        assert_eq!(
            confirm_detail(DeleteTarget::Server, Some("notes.example.com")),
            "It is deleted from notes.example.com too. This can’t be undone."
        );
        assert_eq!(confirm_detail(DeleteTarget::ThisMac, Some("notes.example.com")), "This can’t be undone.");
    }

    #[test]
    fn a_note_deleted_forever_leaves_its_files_and_its_id_for_good() {
        let root = std::env::temp_dir().join(format!("cereal-trash-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let url = "https://notes.example.com";
        let server_id = "3a49a4af-c660-42e9-a36a-a806fbb41e99";
        let mut gone = note(7, "Erased", true);
        gone.server_url = Some(url.into());
        gone.server_id = Some(server_id.into());
        let captures = crate::audio::capture_directory(&root, 7).join("capture-1");
        std::fs::create_dir_all(&captures).unwrap();
        std::fs::write(captures.join("mic.caf"), b"audio").unwrap();
        let sidecar = crate::server::association_file(&root, url, 7);
        std::fs::create_dir_all(sidecar.parent().unwrap().join("uploads").join(server_id)).unwrap();
        std::fs::write(&sidecar, server_id).unwrap();
        // Another note's files stay, even when this note's saved capture
        // entry points at them (a corrupt or foreign manifest).
        let other = crate::audio::capture_directory(&root, 8).join("capture-b");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("mic.caf"), b"another note's audio").unwrap();
        gone.audio_captures = vec![crate::audio::CaptureManifest {
            version: 1,
            capture_id: "capture-b".into(),
            note_id: 7,
            directory: other.clone(),
            manifest_path: other.join("manifest.json"),
            started_at: String::new(),
            finished_at: None,
            stats: Default::default(),
            error: None,
        }];
        erase_files(&root, &gone);
        assert!(!crate::audio::capture_directory(&root, 7).exists());
        assert!(!sidecar.exists());
        assert!(!sidecar.parent().unwrap().join("uploads").join(server_id).exists());
        assert!(other.join("mic.caf").exists(), "another note's recording is never followed");
        // A note directory that is a link loses the link, not what it names.
        #[cfg(unix)]
        {
            let linked = note(9, "Linked", true);
            std::os::unix::fs::symlink(crate::audio::capture_directory(&root, 8), crate::audio::capture_directory(&root, 9)).unwrap();
            erase_files(&root, &linked);
            assert!(!crate::audio::capture_directory(&root, 9).exists());
            assert!(other.join("mic.caf").exists());
        }
        let _ = std::fs::remove_dir_all(&root);

        // Local mode: the highest id deleted forever is never given again.
        let mut workspace = Workspace::default();
        workspace.notes = vec![note(1, "Kept", false), gone];
        assert_eq!(workspace.remove_note(7).map(|note| note.id), Some(7));
        assert!(workspace.note(7).is_none());
        assert_eq!(workspace.create_note(), 8);
        let written: serde_json::Value = serde_json::from_slice(&workspace.encode().unwrap()).unwrap();
        assert_eq!(written["nextId"], 8);
    }
}
