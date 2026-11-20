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
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    ops::Range,
    path::Path,
    sync::mpsc,
    time::{Duration, Instant},
};

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
    /// A question's answers: pills of a fixed width each, so that where they
    /// stand is known before they are drawn (`super::answers`).
    pub const ANSWER_HEIGHT: f32 = 26.;
    pub const ANSWER_GAP: f32 = 6.;
    /// From the question's words to its answers, and from the answers to
    /// the row's bottom edge.
    pub const ANSWER_TOP: f32 = 8.;
    pub const ANSWER_BOTTOM: f32 = 9.;
    /// What a row's own content has across: the card less the list's and
    /// the row's insets.
    pub const ROW_CONTENT: f32 = CARD_WIDTH - 2. * LIST_X - 2. * ROW_X;
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
                // What every list calls a note without a title.
                "New note".into()
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
/// What a row asks before its note is deleted forever, under the note's
/// title (which stays where it was, whole).
pub const QUESTION: &str = "Delete this note forever?";

/// One answer to a row's question.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Answer {
    /// The name of its element (with the note's id).
    pub name: &'static str,
    pub label: &'static str,
    pub width: f32,
    /// It deletes the note forever.
    pub destructive: bool,
}

/// The answers a question offers, in the order they are drawn, and whether
/// they stand at the row's trailing edge (else under the start of its words).
///
/// The rule this encodes: **the answer that destroys never appears under
/// the pointer that asked.** "Delete forever?" is asked from the row's trash
/// button, at the row's trailing edge, so the answer drawn there is Cancel;
/// "edited elsewhere" is asked by pressing Delete, so its answers stand at
/// the other end of the row with "Delete anyway" clear of where Delete was.
/// The answers are also on a line of their own under the title, never on
/// the line that was pressed. The second half of the rule is time
/// ([`crate::ui::press_confirms`]).
pub fn answers(ask: Ask) -> (bool, [Answer; 2]) {
    match ask {
        Ask::Delete(_) => (
            true,
            [
                Answer { name: "trash-confirm", label: "Delete", width: 68., destructive: true },
                Answer { name: "trash-cancel", label: "Cancel", width: 68., destructive: false },
            ],
        ),
        Ask::Changed(_) => (
            false,
            [
                Answer { name: "trash-anyway", label: "Delete anyway", width: 116., destructive: true },
                Answer { name: "trash-changed-restore", label: "Restore", width: 76., destructive: false },
            ],
        ),
    }
}

/// Where each answer of `ask` stands across the row's content (0 is its
/// leading edge, [`size::ROW_CONTENT`] its trailing one), in drawing order.
pub fn answer_spans(ask: Ask) -> [(Answer, Range<f32>); 2] {
    use size::*;
    let (trailing, [first, second]) = answers(ask);
    let start = if trailing {
        ROW_CONTENT - first.width - ANSWER_GAP - second.width
    } else {
        // Under the title: past the tile.
        TILE + ROW_GAP
    };
    let then = start + first.width + ANSWER_GAP;
    [(first, start..start + first.width), (second, then..then + second.width)]
}

/// Where across the row's content the control is whose press asks `ask`:
/// the trash button at the row's trailing edge, or the Delete answer of the
/// first question.
pub fn asked_from(ask: Ask) -> Range<f32> {
    match ask {
        Ask::Delete(_) => size::ROW_CONTENT - size::BUTTON..size::ROW_CONTENT,
        Ask::Changed(id) => answer_spans(Ask::Delete(id))
            .into_iter()
            .find(|(answer, _)| answer.destructive)
            .map(|(_, span)| span)
            .unwrap_or(0.0..0.),
    }
}

thread_local! {
    /// The question now showing and when it was asked: an answer that
    /// destroys does not count until the double-click interval has passed.
    static ASKED: Cell<Option<(Ask, Instant)>> = const { Cell::new(None) };
    /// The question each row last asked, drawn while it folds away.
    static LAST_ASK: RefCell<HashMap<u64, Ask>> = RefCell::new(HashMap::new());
    /// The rows that have left the list and are closing up.
    static LEFT: RefCell<crate::lazy::Leavers<TrashRow>> = RefCell::new(Default::default());
}

/// What a key pressed in the open Trash popover is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TrashKey {
    /// Dealt with here.
    Handled,
    /// The focused control's own: leave it alone.
    Control,
    /// The menu's usual.
    Menu,
}

/// Where Tab goes next among `count` stops, from the one that has the focus
/// (`current`) or, when none has, from `resume` (the first stop of the
/// highlighted row). It never leaves: the popover keeps the keyboard.
pub fn next_stop(count: usize, current: Option<usize>, resume: Option<usize>, back: bool) -> Option<usize> {
    if count == 0 {
        return None;
    }
    Some(match (current.filter(|index| *index < count), back) {
        (Some(index), false) => (index + 1) % count,
        (Some(index), true) => (index + count - 1) % count,
        (None, false) => resume.filter(|index| *index < count).unwrap_or(0),
        (None, true) => resume
            .filter(|index| *index < count)
            .map_or(count - 1, |index| (index + count - 1) % count),
    })
}

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
        let Some(index) = trash_rows(&self.workspace)
            .iter()
            .position(|row| row.id == ask.note())
        else {
            return;
        };
        self.menu_keyboard.selected = Some(index);
        self.trash_ask = Some(ask);
        // From now, and not before the double-click interval has passed,
        // the answer that destroys counts. It is drawn dimmed until then.
        ASKED.with(|asked| asked.set(Some((ask, Instant::now()))));
        self.stage.forget(("trash-armed", ask.note()));
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(crate::ui::double_click_interval() + Duration::from_millis(8))
                .await;
            let _ = this.update(cx, |_, cx| cx.notify());
        })
        .detach();
        cx.notify();
    }

    /// Ask in the row before deleting its note forever.
    pub fn ask_delete_forever(&mut self, id: u64, cx: &mut Context<Self>) {
        self.ask_in_row(Ask::Delete(id), cx);
    }

    /// Whether the answer that destroys would count if `ask` were answered
    /// with it now, by a press that is the `clicks`-th of its run.
    fn trash_answer_counts(&self, ask: Ask, clicks: usize) -> bool {
        self.trash_ask == Some(ask)
            && ASKED.with(Cell::get).is_some_and(|(asked, at)| {
                asked == ask
                    && crate::ui::press_confirms(at.elapsed(), clicks, crate::ui::double_click_interval())
            })
    }

    /// The Tab stops of the popover, in order, each with its row: a row's
    /// two tools, or, while a row asks, that row's two answers and nothing
    /// else.
    fn trash_stops(&self) -> Vec<(usize, ElementId)> {
        let rows = trash_rows(&self.workspace);
        if let Some(ask) = self.trash_ask {
            let Some(index) = rows.iter().position(|row| row.id == ask.note()) else {
                return Vec::new();
            };
            return answers(ask)
                .1
                .into_iter()
                .map(|answer| (index, ElementId::from((answer.name, ask.note()))))
                .collect();
        }
        rows.iter()
            .enumerate()
            .filter(|(_, row)| !self.deleting_forever.contains(&row.id))
            .flat_map(|(index, row)| {
                [
                    (index, ElementId::from(("trash-restore", row.id))),
                    (index, ElementId::from(("trash-delete", row.id))),
                ]
            })
            .collect()
    }

    /// Whether one of the popover's own controls has the keyboard focus.
    pub(crate) fn trash_focus_inside(&self, window: &Window) -> bool {
        self.menu == Some(Menu::Trash)
            && self.trash_stops().into_iter().any(|(_, id)| {
                crate::ui::reach_handle(id).is_some_and(|handle| handle.is_focused(window))
            })
    }

    /// Tab and Shift-Tab while the popover is open: they go round its own
    /// controls (and the highlight goes with them), never out to the page
    /// behind it. True when the popover is open, so the key is its.
    pub(crate) fn trash_tab(&mut self, back: bool, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.menu != Some(Menu::Trash) {
            return false;
        }
        let stops = self.trash_stops();
        let handles: Vec<Option<FocusHandle>> = stops
            .iter()
            .map(|(_, id)| crate::ui::reach_handle(id.clone()))
            .collect();
        let current = handles
            .iter()
            .position(|handle| handle.as_ref().is_some_and(|handle| handle.is_focused(window)));
        let resume = self
            .menu_keyboard
            .selected
            .and_then(|row| stops.iter().position(|(index, _)| *index == row));
        if let Some(next) = next_stop(stops.len(), current, resume, back)
            && let Some(handle) = &handles[next]
        {
            crate::ui::set_keyboard_focus(true);
            handle.focus(window);
            let row = stops[next].0;
            if self.trash_ask.is_none() {
                self.menu_keyboard.selected = Some(row);
            }
            self.trash_scroll.scroll_to_item(row);
        }
        cx.notify();
        true
    }

    /// The arrows and Return in the open popover. While a row asks, the
    /// arrows stay on it and Return lets go of the question (it never
    /// deletes: the answer that destroys is pressed, or reached with Tab and
    /// pressed with Space). Otherwise Return on a control that has the
    /// keyboard focus presses that control, and the arrows take the
    /// highlight, and the ring with it, from row to row.
    pub(crate) fn trash_key(&mut self, moving: bool, window: &mut Window, cx: &mut Context<Self>) -> TrashKey {
        if self.menu != Some(Menu::Trash) {
            return TrashKey::Menu;
        }
        let inside = self.trash_focus_inside(window);
        if self.trash_ask.is_some() {
            if !moving {
                self.trash_ask = None;
                // The key's release must find nothing to press.
                self.focus.focus(window);
            }
            cx.notify();
            return TrashKey::Handled;
        }
        if inside && moving {
            self.focus.focus(window);
        } else if inside {
            return TrashKey::Control;
        }
        TrashKey::Menu
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
        let (button_left, centre_y) = crate::onboarding_view::placed("footer-trash")
            .map(|button| (f32::from(button.left()), f32::from(button.center().y)))
            .unwrap_or((
                self.sidebar_span * (self.sidebar_level - 1.) + BUTTON_LEFT,
                window_height - BUTTON_CENTRE_FROM_BOTTOM,
            ));
        let left = card_left(button_left, f32::from(viewport.width));
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
        // A row that is restored or deleted folds shut and the rows under
        // it close up; what it showed is kept for as long as that takes.
        let ids: Vec<u64> = rows.iter().map(|row| row.id).collect();
        LEFT.with(|left| {
            left.borrow_mut().update(crate::lazy::key(&ids), || {
                rows.iter().map(|row| (row.id, row.clone())).collect()
            })
        });
        let mut listed: HashMap<u64, TrashRow> = rows.into_iter().map(|row| (row.id, row)).collect();
        let drawn = self.stage.roster("trash", 0, &ids);
        let leaving: Vec<u64> = drawn.iter().filter(|row| !row.present).map(|row| row.key).collect();
        let mut position = 0;
        for row in &drawn {
            let content = match listed.remove(&row.key) {
                Some(listed) if row.present => {
                    position += 1;
                    Some(self.trash_row(Some(position - 1), listed, live.clone(), cx))
                }
                _ => LEFT
                    .with(|left| left.borrow().left(row.key).cloned())
                    .map(|left| self.trash_row(None, left, live.clone(), cx)),
            };
            list = list.children(crate::motion::roster_row(&self.stage, "trash", *row, content));
        }
        LEFT.with(|left| left.borrow_mut().keep(&leaving));
        // "Trash is empty" arrives once the last row has gone.
        let nothing = drawn.is_empty();
        let arrived = self.stage.swap(("trash-empty", 0), u64::from(nothing));
        if nothing {
            list = list.items_center().justify_center().pb(px(HEADER_HEIGHT / 2.)).child(
                crate::motion::swapped(
                    div()
                        .text_size(px(13.))
                        .line_height(px(18.))
                        .text_color(t.muted)
                        .child(EMPTY),
                    arrived,
                ),
            );
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
            .child(scroll_thumb("trash", &self.trash_scroll));
        let card = floating(card);
        div()
            .absolute()
            .left(px(left))
            .bottom(px(ABOVE_BOTTOM))
            .child(pop(card, Pop::menu((0., anchor_y)), pose))
            .into_any_element()
    }

    /// One row of the popover. `position` is its place among the rows, or
    /// `None` for a row that has left the list and is only closing up:
    /// that one answers nothing.
    fn trash_row(
        &self,
        position: Option<usize>,
        row: TrashRow,
        live: impl Fn(&Cereal) -> bool + Clone + 'static,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        use size::*;
        let t = self.theme();
        let id = row.id;
        let inert = position.is_none();
        let asking = self.trash_ask.filter(|ask| ask.note() == id && !inert);
        let busy = self.deleting_forever.contains(&id);
        // The question opens under the title, which stays where it is, and
        // the row's tools make way for it. What it asked is still drawn
        // while it folds away.
        let question_key = ("trash-ask", id);
        let question = self.stage.presence(question_key, asking.is_some(), crate::motion::GOOEY);
        let shown = LAST_ASK.with(|last| {
            let mut last = last.borrow_mut();
            match asking {
                Some(ask) => {
                    last.insert(id, ask);
                    Some(ask)
                }
                None if question.visible => last.get(&id).copied(),
                None => {
                    last.remove(&id);
                    None
                }
            }
        });
        let place = |names: &[&'static str; 6]| position.and_then(|position| names.get(position).copied());
        let tile = div()
            .size(px(TILE))
            .flex_shrink_0()
            .rounded(px(TILE_RADIUS))
            .bg(t.hover)
            .flex()
            .items_center()
            .justify_center()
            .child(icon("document", TILE_ICON, t.muted.opacity(0.6)));
        // The tools answer only while the row is not asking: where they
        // were pressed, nothing is pressable while the question shows.
        let tools_live = !inert && asking.is_none();
        let button = |name: &'static str,
                      symbol: &'static str,
                      label: &'static str,
                      spot: Option<&'static str>| {
            let button = div()
                .id((name, id))
                .relative()
                .size(px(BUTTON))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(6.))
                .cursor_default()
                .child(icon(symbol, BUTTON_ICON, t.muted));
            if !tools_live {
                return button;
            }
            button
                .child(crate::motion::tactile(t.hover).press(t.fill).radius(6.))
                .when_some(spot, |d, spot| d.child(crate::onboarding_view::place(spot)))
                .child(crate::ui::tip(label, t))
                .reach()
        };
        let live_restore = live.clone();
        let live_delete = live.clone();
        let tools = div()
            .flex()
            .flex_shrink_0()
            .items_center()
            .gap(px(BUTTON_GAP))
            .opacity((1. - question.fade.clamp(0., 1.)) * if busy { DISABLED } else { 1. })
            .child(
                button("trash-restore", "restore", "Restore", place(&RESTORE_PLACES)).when(tools_live, |d| {
                    d.on_click(cx.listener(move |this, _, _, cx| {
                        if live_restore(this) && !this.deleting_forever.contains(&id) {
                            this.restore_from_trash(id, cx);
                        }
                    }))
                }),
            )
            .child(
                button("trash-delete", "trash", "Delete forever", place(&DELETE_PLACES)).when(tools_live, |d| {
                    d.on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                        if live_delete(this) && !this.deleting_forever.contains(&id) {
                            this.ask_delete_forever(id, cx);
                            // Asked from the keyboard, the focus goes to
                            // the answer that keeps the note.
                            if event.is_keyboard()
                                && let Some(cancel) = crate::ui::reach_handle(("trash-cancel", id))
                            {
                                cancel.focus(window);
                            }
                        }
                    }))
                }),
            );
        let head = div()
            .h(px(ROW_HEIGHT))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(ROW_GAP))
            .child(tile)
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
            .child(tools);
        let body = shown.map(|ask| {
            let spans = answer_spans(ask);
            debug_assert!(spans.iter().all(|(answer, span)| {
                let from = asked_from(ask);
                !answer.destructive || span.end <= from.start || span.start >= from.end
            }));
            let words = match ask {
                Ask::Delete(_) => {
                    let target = self
                        .workspace
                        .note(id)
                        .map(|note| delete_target(note, crate::server::enabled()))
                        .unwrap_or(DeleteTarget::ThisMac);
                    let host = crate::accounts::of_note(id).map(|account| account.host());
                    div()
                        .flex()
                        .flex_col()
                        .child(div().line_height(px(TITLE_LINE)).child(QUESTION))
                        .child(small(confirm_detail(target, host.as_deref()), t).whitespace_normal())
                }
                Ask::Changed(_) => div().child(small(CHANGED, t).whitespace_normal()),
            };
            let answering = asking == Some(ask);
            // Where `answer_spans` says: the body starts past the tile.
            let mut pills = div()
                .mt(px(ANSWER_TOP))
                .ml(px(spans[0].1.start - (TILE + ROW_GAP)))
                .flex()
                .gap(px(ANSWER_GAP));
            for (answer, _) in spans {
                // The answer that destroys is dimmed until a press on it
                // would count (`ui::press_confirms`).
                let armed = !answer.destructive || self.trash_answer_counts(ask, 1);
                let lit = if answer.destructive && answering {
                    self.stage
                        .aim(("trash-armed", id), if armed { 1. } else { 0. }, crate::motion::SNAPPY)
                        .clamp(0., 1.)
                } else {
                    1.
                };
                let spot = match (answer.destructive, answering) {
                    (true, true) => place(&CONFIRM_PLACES),
                    (false, true) => place(&CANCEL_PLACES),
                    _ => None,
                };
                let live = live.clone();
                let pill = if answering {
                    pill((answer.name, id), answer.label, "", t).on_click(cx.listener(
                        move |this, event: &ClickEvent, window, cx| {
                            if !live(this) {
                                return;
                            }
                            match (ask, answer.destructive) {
                                (_, true) => {
                                    if this.trash_answer_counts(ask, event.click_count()) {
                                        this.delete_forever(id, matches!(ask, Ask::Changed(_)), cx);
                                    }
                                }
                                (Ask::Delete(_), false) => {
                                    this.trash_ask = None;
                                    // Back to the tool that asked, for the keyboard.
                                    if event.is_keyboard()
                                        && let Some(tool) = crate::ui::reach_handle(("trash-delete", id))
                                    {
                                        tool.focus(window);
                                    }
                                    cx.notify();
                                }
                                (Ask::Changed(_), false) => {
                                    this.trash_ask = None;
                                    this.restore_from_trash(id, cx);
                                }
                            }
                        },
                    ))
                } else {
                    // Folding away: the same pill, answering nothing.
                    pill_frame((answer.name, id), t).child(answer.label)
                };
                pills = pills.child(
                    pill.h(px(ANSWER_HEIGHT))
                        .w(px(answer.width))
                        .px(px(0.))
                        .flex_shrink_0()
                        .when(answer.destructive, |d| {
                            d.bg(t.danger)
                                .border_color(t.danger)
                                .text_color(gpui::white())
                                .opacity(DISABLED + (1. - DISABLED) * lit)
                        })
                        .when_some(spot, |d, spot| d.relative().child(crate::onboarding_view::place(spot))),
                );
            }
            div()
                .pl(px(TILE + ROW_GAP))
                .pb(px(ANSWER_BOTTOM))
                .flex()
                .flex_col()
                .child(words)
                .child(pills)
        });
        let mut element = div()
            .id(("trash-row", id))
            .relative()
            .flex_shrink_0()
            .px(px(ROW_X))
            .flex()
            .flex_col()
            .rounded(px(8.));
        if !inert {
            // Return restores the highlighted note; in a row that is asking,
            // it lets go of the question (Return never deletes). While any
            // row asks, the keys stay on it (`trash_key`).
            let (index, _) = self.menu_keyboard.register(move |this, _, cx| {
                if this.trash_ask.is_some() {
                    this.trash_ask = None;
                    cx.notify();
                } else if !this.deleting_forever.contains(&id) {
                    this.restore_from_trash(id, cx);
                }
            });
            element = element
                .when(self.menu_keyboard.selected == Some(index), |d| {
                    d.child(self.stage.glide("menu").probe(index))
                })
                .on_mouse_move(cx.listener(move |this, _, _, cx| {
                    // A row that is asking keeps the highlight.
                    if this.trash_ask.is_none() && this.menu_keyboard.selected != Some(index) {
                        this.menu_keyboard.selected = Some(index);
                        cx.notify();
                    }
                }));
        }
        element
            .child(head)
            .children(body.and_then(|body| {
                crate::motion::fold(&self.stage, question_key, question, body)
            }))
    }
}

/// Where the card's left edge is: clear of the trash button, and no nearer
/// to the sidebar's own left edge than [`size::CARD_LEFT`], so it stands in
/// the same place beside the sidebar whether that shares the window with
/// the page or has slid over it (the button is `button_left` from the
/// window's edge either way, [`size::BUTTON_LEFT`] into the sidebar). It
/// stays inside a window `window` wide.
pub fn card_left(button_left: f32, window: f32) -> f32 {
    use size::*;
    let sidebar = button_left - BUTTON_LEFT;
    (button_left + BUTTON + BESIDE_BUTTON)
        .max(sidebar + CARD_LEFT)
        .min((window - CARD_WIDTH - 8.).max(8.))
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
                TrashRow { id: 2, title: "New note".into(), gone: false },
                TrashRow { id: 3, title: "Gone from the server".into(), gone: true },
            ]
        );
    }

    #[test]
    fn the_answer_that_destroys_is_never_where_the_pointer_that_asked_is() {
        for ask in [Ask::Delete(7), Ask::Changed(7)] {
            let from = asked_from(ask);
            assert!(from.start < from.end, "{ask:?} is asked from somewhere");
            let spans = answer_spans(ask);
            for (answer, span) in &spans {
                // Every answer is inside the row.
                assert!(span.start >= 0. && span.end <= size::ROW_CONTENT, "{ask:?} {answer:?}");
                if answer.destructive {
                    assert!(
                        span.end <= from.start || span.start >= from.end,
                        "{ask:?}: {} stands at {span:?}, under the press at {from:?}",
                        answer.label
                    );
                }
            }
            // One answer destroys, one does not, and they do not overlap.
            assert_eq!(spans.iter().filter(|(answer, _)| answer.destructive).count(), 1);
            assert!(spans[0].1.end <= spans[1].1.start);
        }
        // "Delete forever?" is asked from the trash button at the row's
        // trailing edge: what stands there is Cancel.
        let [(_, delete), (cancel, kept)] = answer_spans(Ask::Delete(7));
        assert_eq!(cancel.label, "Cancel");
        assert_eq!(kept.end, size::ROW_CONTENT);
        let button = asked_from(Ask::Delete(7));
        assert!(kept.start <= button.start && button.end <= kept.end);
        assert!(delete.end < button.start);
    }

    #[test]
    fn tab_goes_round_the_popovers_own_controls() {
        // Two rows, two tools each.
        assert_eq!(next_stop(4, None, None, false), Some(0));
        assert_eq!(next_stop(4, Some(0), None, false), Some(1));
        assert_eq!(next_stop(4, Some(3), None, false), Some(0));
        assert_eq!(next_stop(4, Some(0), None, true), Some(3));
        assert_eq!(next_stop(4, None, None, true), Some(3));
        // With the highlight on the second row, Tab starts at its first tool
        // and Shift-Tab at the tool before it.
        assert_eq!(next_stop(4, None, Some(2), false), Some(2));
        assert_eq!(next_stop(4, None, Some(2), true), Some(1));
        // A row that is asking has its two answers and nothing else.
        assert_eq!(next_stop(2, Some(1), None, false), Some(0));
        // An empty Trash has nowhere to stop, and a stop that has gone is
        // not one to move on from.
        assert_eq!(next_stop(0, None, None, false), None);
        assert_eq!(next_stop(2, Some(5), None, false), Some(0));
    }

    #[test]
    fn the_card_keeps_its_place_beside_the_sidebar_wherever_the_sidebar_is() {
        use size::*;
        // Beside the page: the sidebar starts at the window's edge.
        let beside = card_left(BUTTON_LEFT, 1200.);
        assert_eq!(beside, CARD_LEFT);
        // Slid over a narrow window it starts there too, and so does the card
        // (it was 73 pt, over the sidebar's own rows).
        assert_eq!(card_left(BUTTON_LEFT, 760.), beside);
        // Part of the way in, the card is as far from the sidebar's edge.
        assert_eq!(card_left(BUTTON_LEFT - 60., 760.), beside - 60.);
        // Never past the window's trailing edge.
        assert_eq!(card_left(BUTTON_LEFT + 400., 760.), 760. - CARD_WIDTH - 8.);
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
