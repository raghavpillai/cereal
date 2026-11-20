use crate::{
    app::{Cereal, Screen},
    model::Note,
    ui::*,
};
use chrono::{Datelike, Duration, Local, NaiveDate};
use gpui::{prelude::*, *};

thread_local! {
    /// What the notes list showed of the captions and rows that have left
    /// it, drawn while their space closes.
    static NOTES_LEFT: std::cell::RefCell<crate::lazy::Leavers<Kept>> =
        std::cell::RefCell::new(Default::default());
    /// The note whose row the arrow keys have just moved the focus to: the
    /// list brings it into view when it is next drawn.
    static REVEAL_NOTE: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
}

/// A caption or a row of a notes list that has left it.
#[derive(Clone)]
enum Kept {
    Day(String),
    Row(HomeRow),
}

/// How much of the bottom of Home's list the Ask bar floats over.
const ASK_BAR_REACH: f32 = 110.;

/// Which row the arrow keys move the focus to from the row of note `from`
/// in a list of `ids`: the next or the previous, stopping at the ends.
/// `None` when there is nowhere to go.
pub fn row_step(ids: &[u64], from: u64, back: bool) -> Option<u64> {
    let at = ids.iter().position(|id| *id == from)?;
    let to = if back { at.checked_sub(1)? } else { at + 1 };
    ids.get(to).copied()
}

/// One row of a notes list: everything it draws, and nothing else.
#[derive(Clone, Debug, PartialEq)]
pub struct HomeRow {
    pub id: u64,
    pub title: SharedString,
    pub time: SharedString,
    /// The day it is grouped under, `YYYY-MM-DD`.
    pub date: String,
}

/// One row of the search-results page.
#[derive(Clone, Debug, PartialEq)]
pub struct SearchHit {
    pub row: HomeRow,
    /// The first line that matches, shortened for display.
    pub snippet: String,
}

/// The matching line shown under a search result: the first line of the
/// body, summary or transcript containing `query`, else the title.
pub fn search_snippet(note: &Note, query: &str) -> String {
    /// The most of a line an excerpt shows, an ellipsis included.
    const LIMIT: usize = 140;
    /// How much of the line is kept before the match when the line has to
    /// be entered part of the way along.
    const LEAD: usize = 40;
    let needle = query.to_lowercase();
    let line = note
        .body
        .lines()
        .chain(note.enhanced.lines())
        .chain(note.transcript.lines())
        .find(|line| line.to_lowercase().contains(&needle))
        .unwrap_or(&note.title)
        .trim();
    // The words, not the Markdown that marks them as a heading or a bullet.
    let unmarked = line.trim_start_matches('#');
    let line = if unmarked.len() < line.len() && unmarked.starts_with(' ') {
        unmarked.trim_start()
    } else {
        line.strip_prefix("- ")
            .or_else(|| line.strip_prefix("* "))
            .unwrap_or(line)
    };
    let characters: Vec<char> = line.chars().collect();
    if characters.len() <= LIMIT {
        return line.to_owned();
    }
    // A long line is entered a little before the match, so that what was
    // searched for is in the excerpt, and every cut end says it was cut.
    let lower: Vec<char> = line.to_lowercase().chars().collect();
    let wanted: Vec<char> = needle.chars().collect();
    let found = (lower.len() == characters.len() && !wanted.is_empty())
        .then(|| lower.windows(wanted.len()).position(|part| part == wanted))
        .flatten()
        .unwrap_or(0);
    let start = if found + wanted.len() <= LIMIT - 1 {
        0
    } else {
        (found.saturating_sub(LEAD)).min(characters.len() - (LIMIT - 1))
    };
    let leading = start > 0;
    let room = LIMIT - usize::from(leading);
    let trailing = start + room < characters.len();
    let end = if trailing {
        start + room - 1
    } else {
        characters.len()
    };
    let mut excerpt = String::new();
    if leading {
        excerpt.push('…');
    }
    excerpt.extend(characters[start..end].iter());
    if trailing {
        excerpt.push('…');
    }
    excerpt
}

type WorkspaceRevision = (u64, usize, usize);
type TranscriptKey = (u64, u64, usize, usize, usize);
type Cached<K, V> = std::cell::RefCell<Option<(K, std::rc::Rc<V>)>>;

/// Data derived from the workspace that is expensive to rebuild and only
/// changes when the workspace does. Rendering happens on every animation
/// frame, hover and scroll tick; none of those may clone notes, sort them or
/// re-parse a transcript.
#[derive(Default)]
pub struct Caches {
    home: Cached<(WorkspaceRevision, bool), Vec<HomeRow>>,
    search: Cached<(WorkspaceRevision, String), Vec<u64>>,
    results: Cached<(WorkspaceRevision, String), Vec<SearchHit>>,
    utterances: Cached<TranscriptKey, Vec<crate::note::TranscriptUtterance>>,
}

/// The rows of a notes list, in the order given, from the notes themselves.
pub fn home_rows(notes: &[&Note]) -> Vec<HomeRow> {
    notes
        .iter()
        .map(|note| HomeRow {
            id: note.id,
            title: if note.title.trim().is_empty() {
                "New note".into()
            } else {
                note.title.clone().into()
            },
            time: note.time.clone().into(),
            // The day a row is listed under is never later than tomorrow.
            date: note.sort_day(),
        })
        .collect()
}

impl Cereal {
    /// Rows for the current list, rebuilt only when the workspace changed.
    pub fn list_rows(&self, favorites: bool) -> std::rc::Rc<Vec<HomeRow>> {
        let key = (self.workspace.revision(), favorites);
        let mut cache = self.caches.home.borrow_mut();
        if let Some((cached, rows)) = cache.as_ref()
            && *cached == key
        {
            return rows.clone();
        }
        let rows = std::rc::Rc::new(home_rows(
            &self.workspace.visible_notes(favorites, ""),
        ));
        *cache = Some((key, rows.clone()));
        rows
    }

    /// Ids of the notes matching `query`, best first, searched once per
    /// query and workspace state rather than once per frame.
    pub fn search_ids(&self, query: &str) -> std::rc::Rc<Vec<u64>> {
        let revision = self.workspace.revision();
        let mut cache = self.caches.search.borrow_mut();
        if let Some(((cached, cached_query), ids)) = cache.as_ref()
            && *cached == revision
            && cached_query == query
        {
            return ids.clone();
        }
        let ids = std::rc::Rc::new(
            self.workspace
                .visible_notes(false, query)
                .iter()
                .map(|note| note.id)
                .collect::<Vec<_>>(),
        );
        *cache = Some(((revision, query.to_string()), ids.clone()));
        ids
    }

    /// The search-results page: found, sorted and excerpted once per query
    /// and workspace state. Rendering it copies no note text.
    pub fn search_hits(&self, query: &str) -> std::rc::Rc<Vec<SearchHit>> {
        let key = (self.workspace.revision(), query.to_string());
        let mut cache = self.caches.results.borrow_mut();
        if let Some((cached, hits)) = cache.as_ref()
            && *cached == key
        {
            return hits.clone();
        }
        let notes = self.workspace.visible_notes(false, query);
        let hits = std::rc::Rc::new(
            home_rows(&notes)
                .into_iter()
                .zip(&notes)
                .map(|(row, note)| SearchHit {
                    row,
                    snippet: search_snippet(note, query),
                })
                .collect::<Vec<_>>(),
        );
        *cache = Some((key, hits.clone()));
        hits
    }

    /// A note's transcript as utterances, parsed once per transcript state.
    /// The length is part of the key because legacy adapters may assign the
    /// text without advancing its revision.
    pub fn utterances(&self, note: &Note) -> std::rc::Rc<Vec<crate::note::TranscriptUtterance>> {
        let key = (
            note.id,
            note.transcript_revision,
            note.transcript.len(),
            note.server_transcript_segments.len(),
            note.transcript_gaps.len(),
        );
        let mut cache = self.caches.utterances.borrow_mut();
        if let Some((cached, utterances)) = cache.as_ref()
            && *cached == key
        {
            return utterances.clone();
        }
        let utterances = std::rc::Rc::new(crate::note::transcript_utterances(note));
        *cache = Some((key, utterances.clone()));
        utterances
    }

    /// Parse the transcript again next time: its segments changed in a way
    /// its revision and lengths do not show (speakers were relabelled).
    pub fn forget_utterances(&self) {
        self.caches.utterances.borrow_mut().take();
    }

    pub fn render_home(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let t = self.theme();
        let home = self.screen == Screen::Home;
        let notes = self.list_rows(self.screen == Screen::Favorites);
        let title = match self.screen {
            Screen::Favorites => "Favorites".to_string(),
            _ => "My notes".into(),
        };
        // The list is a day caption followed by that day's rows, over and
        // over. Only the part near the viewport is built (see `lazy`); a
        // workspace with hundreds of notes costs the same per frame as one
        // with a dozen.
        enum Entry<'a> {
            Day(&'a str),
            Row(&'a HomeRow),
        }
        let mut entries = Vec::with_capacity(notes.len() + 16);
        let mut last_date = "";
        for note in notes.iter() {
            if note.date != last_date {
                last_date = &note.date;
                entries.push(Entry::Day(&note.date));
            }
            entries.push(Entry::Row(note));
        }
        let keys: Vec<u64> = entries
            .iter()
            .map(|entry| match entry {
                Entry::Day(date) => crate::lazy::key(("day", date)),
                Entry::Row(note) => crate::lazy::key(("row", note.id)),
            })
            .collect();
        // A note that leaves the list (to Trash, out of Favorites) folds
        // shut and the rows under it close up, its day's caption with it
        // when it was the day's only note; one that comes back (Undo) opens
        // its place again. What a row showed is kept while it leaves.
        let scope = match self.screen {
            Screen::Favorites => 2,
            Screen::MyNotes => 1,
            _ => 0,
        };
        NOTES_LEFT.with(|left| {
            left.borrow_mut().update(crate::lazy::key((self.workspace.revision(), scope)), || {
                entries
                    .iter()
                    .zip(&keys)
                    .map(|(entry, key)| {
                        (
                            *key,
                            match entry {
                                Entry::Day(date) => Kept::Day((*date).to_owned()),
                                Entry::Row(note) => Kept::Row((*note).clone()),
                            },
                        )
                    })
                    .collect()
            })
        });
        let drawn = crate::lazy::roster(&self.stage, "notes", scope, &keys);
        let drawn_keys: Vec<u64> = drawn.iter().map(|row| row.key).collect();
        // Nothing arriving or leaving: the rows are the entries, one to one.
        let plain = drawn.len() == entries.len();
        let at: std::collections::HashMap<u64, usize> = if plain {
            Default::default()
        } else {
            keys.iter().enumerate().map(|(index, key)| (*key, index)).collect()
        };
        if !plain {
            let leaving: Vec<u64> = drawn.iter().filter(|row| !row.present).map(|row| row.key).collect();
            NOTES_LEFT.with(|left| left.borrow_mut().keep(&leaving));
        }
        // The row the arrow keys moved to is brought into view, clear of
        // the Ask bar.
        if let Some(id) = REVEAL_NOTE.take() {
            let key = crate::lazy::key(("row", id));
            if let Some(index) = drawn_keys.iter().position(|drawn| *drawn == key) {
                crate::lazy::reveal_clear("notes", index, if home { ASK_BAR_REACH } else { 0. });
            }
        }
        // The row that has the keyboard focus carries the list's highlight,
        // which travels from row to row with the arrow keys.
        let glide = self.stage.glide("notes");
        let keyboard = crate::ui::keyboard_focus();
        let draw_caption = |date: &str| {
            caption(date_label(date), t)
                .px(px(8.))
                .pt(px(25.))
                .pb(px(2.))
                .into_any_element()
        };
        // On Home the list sits a pixel inside the card column above it.
        let mut list = div()
            .flex()
            .flex_col()
            .w_full()
            .px(px(if home { 1. } else { 0. }))
            .child(glide.indicator(t.hover, 12.))
            .child(crate::lazy::column(
                "notes",
                &drawn_keys,
                0.,
                0.,
                &self.home_scroll,
                window,
                |index| {
                    let row = drawn[index];
                    let entry = if plain { Some(index) } else { at.get(&row.key).copied() };
                    let content = match entry.filter(|_| row.present).map(|index| &entries[index]) {
                        Some(Entry::Day(date)) => Some(draw_caption(date)),
                        Some(Entry::Row(note)) => {
                            let focused = keyboard
                                && crate::ui::reach_handle(("note-row", note.id))
                                    .is_some_and(|handle| handle.is_focused(window));
                            Some(
                                self.list_row(note.id, note.title.clone(), &note.time, cx)
                                    .when(focused, |row| row.child(glide.probe(note.id)))
                                    .into_any_element(),
                            )
                        }
                        // Gone from the list: what it showed, answering nothing.
                        None => NOTES_LEFT.with(|left| left.borrow().left(row.key).cloned()).map(|kept| match kept {
                            Kept::Day(date) => draw_caption(&date),
                            Kept::Row(note) => self
                                .list_row_frame(
                                    ("note-row-left", note.id),
                                    crate::motion::reveal_key(("note-row-left", note.id)),
                                    note.title.clone(),
                                    "Me",
                                    &note.time,
                                    div().id(("note-row-left-more", note.id)),
                                )
                                .into_any_element(),
                        }),
                    };
                    crate::motion::roster_row(&self.stage, "notes", row, content)
                        .map_or_else(|| div().into_any_element(), IntoElement::into_any_element)
                },
            ));
        // The empty page arrives when the last note has left, in the space
        // that note gave up; shown on arrival, it is simply there.
        list = list.children(crate::motion::appear_on_page(
            &self.stage,
            ("notes-empty", scope),
            notes.is_empty() && drawn.is_empty(),
            div()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .py(px(70.))
                .gap(px(14.))
                .child(icon("document", 32., t.muted))
                .child(heading("A little space for your thoughts", t))
                .child(small("Create a note to get started.", t))
                .child(
                    pill("empty-new", "New note", "plus", t)
                        .on_click(cx.listener(|this, _, w, cx| this.new_note(w, cx))),
                ),
        ));
        // Space pages (My notes, Favorites) are wider and open with
        // a centred hero; Home keeps the narrower column under "Coming up".
        let space = !home;
        // In pixels (see `page_column_width`): the page beside the sidebar,
        // less the scroll bar's gutter.
        let page = f32::from(window.viewport_size().width) - self.sidebar_inset() - 10.;
        let mut column = div()
            .w(px(page.clamp(240., if space { 768. } else { 674. })))
            .px(px(24.))
            .pb(px(130.))
            .flex()
            .flex_col()
            .flex_shrink_0();
        if home {
            column = column.child(self.calendar(cx));
        } else {
            column = column.child(self.space_hero(title, notes.len(), window, cx));
        }
        column = column.child(list);
        let assistant =
            self.stage
                .presence(("assistant", 0), self.assistant_open, crate::motion::PANEL);
        div()
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_hidden()
            .child(
                div()
                    .id("home-scroll")
                    .track_scroll(&self.home_scroll)
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .pr(px(10.))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(column),
            )
            .child(scroll_thumb("home", &self.home_scroll))
            // The chat unfolds out of the ask bar and folds back into it.
            // Space pages ask from their inline composer instead of the bar.
            .when(!assistant.visible && !space, |home| {
                home.child(self.ask_bar(window, cx))
            })
            .when(assistant.visible, |home| {
                // The page ends under the panel as it does under the bar,
                // so nothing of the list shows below the pill while the
                // panel opens or folds, and nothing pops when the bar is back.
                home.child(
                    div()
                        .absolute()
                        .bottom(px(27.))
                        .left_0()
                        .right_0()
                        .h(px(crate::motion::ASK_PILL))
                        .child(bar_scrim(t, 0.)),
                )
                .child(self.render_note_assistant(assistant, window, cx))
                    // Under the panel, centred on the 9 px between it and
                    // the window's edge.
                    .child(
                        div()
                            .absolute()
                            .bottom(px(8.))
                            .left_0()
                            .right_0()
                            .flex()
                            .justify_center()
                            .text_size(px(11.))
                            .line_height(px(12.))
                            .text_color(t.muted)
                            .opacity(assistant.value.clamp(0., 1.) * 0.8)
                            .child("Cereal uses AI and can make mistakes."),
                    )
            })
            .into_any_element()
    }

    pub(crate) fn list_row(
        &self,
        id: u64,
        title: SharedString,
        time: &str,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let hovered = crate::motion::reveal_key(("note-row", id));
        let more = self.note_row_more(id, cx);
        self.note_row_actions(
            self.list_row_frame(("note-row", id), hovered, title, "Me", time, more),
            id,
            cx,
        )
    }

    /// The "…" at the end of a note's row: its menu, under the pointer.
    pub(crate) fn note_row_more(&self, id: u64, cx: &mut Context<Self>) -> Stateful<Div> {
        tool_tip(("note-row-more", id), "more", "More actions", self.theme())
            .size(px(24.))
            .on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                let at = window.mouse_position();
                this.open_row_menu(id, at, cx);
            }))
    }

    /// What a note's row does, wherever it is drawn: a click opens the
    /// note, a secondary click its menu, and the arrow keys move on.
    pub(crate) fn note_row_actions(&self, row: Stateful<Div>, id: u64, cx: &mut Context<Self>) -> Stateful<Div> {
        row.on_click(cx.listener(move |this, _, _, cx| this.open_note(id, cx)))
        // Secondary click opens the row's menu under the pointer.
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.open_row_menu(id, event.position, cx);
            }),
        )
        // With the keyboard focus on a row, Up and Down move it from row to
        // row, as in a menu (Return opens the row, as for any control).
        .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
            let back = match event.keystroke.key.as_str() {
                "up" => true,
                "down" => false,
                _ => return,
            };
            if event.keystroke.modifiers.modified() {
                return;
            }
            cx.stop_propagation();
            this.step_note_row(id, back, window, cx);
        }))
    }

    /// Move the keyboard focus from the row of note `from` to the row before
    /// or after it in the list of notes on screen, and bring it into view.
    fn step_note_row(&mut self, from: u64, back: bool, window: &mut Window, cx: &mut Context<Self>) {
        let ids: Vec<u64> = if self.screen == Screen::SearchResults {
            self.search_hits(&self.search_results_query).iter().map(|hit| hit.row.id).collect()
        } else {
            self.list_rows(self.screen == Screen::Favorites).iter().map(|row| row.id).collect()
        };
        let Some(to) = row_step(&ids, from, back) else {
            return;
        };
        if let Some(handle) = crate::ui::reach_handle(("note-row", to)) {
            crate::ui::set_keyboard_focus(true);
            handle.focus(window);
            REVEAL_NOTE.set(Some(to));
            cx.notify();
        }
    }

    /// The note whose row the arrow keys have just moved the focus to, once:
    /// for the list that draws it to bring it into view.
    pub(crate) fn take_note_to_reveal() -> Option<u64> {
        REVEAL_NOTE.take()
    }

    /// One row of a list of notes, as every such list draws it: a document
    /// tile, a title over a caption, and at the trailing edge a time that
    /// gives way to `action` under the pointer. The caller says what a click
    /// does.
    pub(crate) fn list_row_frame(
        &self,
        id: impl Into<ElementId>,
        hovered: u64,
        title: SharedString,
        subtitle: impl Into<SharedString>,
        time: &str,
        action: Stateful<Div>,
    ) -> Stateful<Div> {
        self.list_row_shell(id, hovered)
            .child(self.list_row_line(hovered, title, subtitle, time, action))
    }

    /// A list row's box and nothing in it: its insets and corners, its
    /// place in the cascade and the tab order, and its wash under the
    /// pointer, which covers everything put in it.
    pub(crate) fn list_row_shell(&self, id: impl Into<ElementId>, hovered: u64) -> Stateful<Div> {
        let t = self.theme();
        div()
            .id(id)
            .relative()
            .w_full()
            // A 44 px row on a 48 px pitch: the fill never touches its
            // neighbours.
            .mb(px(4.))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .px(px(8.))
            .rounded(px(12.))
            .cursor_default()
            // Rows cascade in after navigation and squash under the pointer.
            .map(|row| crate::motion::lift(row, self.stage.cascade_next("page")))
            .child(row_feedback(t, 12.).reveals(hovered))
            .reach()
    }

    /// The 44 px line of a list row: the tile, the title over its caption,
    /// and the time that gives way to `action` under the pointer.
    pub(crate) fn list_row_line(
        &self,
        hovered: u64,
        title: SharedString,
        subtitle: impl Into<SharedString>,
        time: &str,
        action: Stateful<Div>,
    ) -> Div {
        let t = self.theme();
        div()
            .h(px(44.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(16.))
            .child(
                div()
                    .size(px(28.))
                    .flex_shrink_0()
                    .rounded(px(6.))
                    .bg(if self.workspace.settings.dark_mode {
                        rgb(0x515150)
                    } else {
                        rgb(0xd8d9d3)
                    })
                    .border_1()
                    .border_color(if self.workspace.settings.dark_mode {
                        rgba(0xffffff26)
                    } else {
                        rgb(0xcdc9c0)
                    })
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("document", 17., t.text)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(20.))
                            .truncate()
                            .child(title),
                    )
                    .child(small(subtitle, t).truncate()),
            )
            // Under the pointer the time gives way to the row's actions.
            .child(
                div()
                    .relative()
                    .flex_shrink_0()
                    .min_w(px(48.))
                    .h(px(24.))
                    .flex()
                    .items_center()
                    .justify_end()
                    .child(crate::motion::conceal(time_label(time, t), hovered))
                    .child(
                        crate::motion::reveal(div(), hovered)
                            .absolute()
                            .top_0()
                            .right_0()
                            .child(action),
                    ),
            )
    }

    /// The centred opening of a space page: its tile, title and scope, an
    /// optional note about privacy, and a composer for asking about the space.
    fn space_hero(
        &self,
        title: String,
        notes: usize,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let t = self.theme();
        let private = self.screen == Screen::MyNotes;
        let (symbol, subtitle) = match self.screen {
            Screen::Favorites => ("star", "Notes you have marked as favorites."),
            _ => ("lock-solid", "All of your private notes."),
        };
        let plural = |count: usize, word: &str| {
            format!("{count} {word}{}", if count == 1 { "" } else { "s" })
        };
        let meta = div()
            .mt(px(9.))
            .flex()
            .items_center()
            .justify_center()
            .gap(px(6.))
            .text_size(px(13.))
            .line_height(px(18.))
            .text_color(t.muted);
        let meta = if private {
            meta.child(icon("lock-solid", 12., t.muted))
                .child("Your private notes")
                .child("·")
                .child(icon("document", 13., t.muted))
                .child(plural(notes, "note"))
        } else {
            meta.child(icon("document", 13., t.muted))
                .child(plural(notes, "note"))
        };
        let asking = self.assistant_open;
        let focused = self.ask.read(cx).focus_handle.is_focused(window);
        let can_send = !self.ask.read(cx).content().trim().is_empty();
        div()
            .w_full()
            .flex()
            .flex_col()
            .items_center()
            .pt(px(76.))
            .child(
                div()
                    .size(px(28.))
                    .rounded(px(7.))
                    .border_1()
                    .border_color(t.hairline)
                    .bg(t.fill)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon(symbol, 14., t.secondary)),
            )
            .child(
                div()
                    .mt(px(10.))
                    .w_full()
                    .text_center()
                    .font_family(DISPLAY)
                    .text_size(px(35.))
                    .line_height(px(44.))
                    .text_color(t.text)
                    .child(title),
            )
            .child(
                div()
                    .mt(px(1.))
                    .text_size(px(13.))
                    .line_height(px(18.))
                    .text_color(t.muted)
                    .child(subtitle),
            )
            .child(meta)
            .when(private, |hero| {
                let (fill, edge) = (t.info_fill, t.info_edge);
                // Dismissed, the banner fades while the space it took (its
                // own height and the 40 px above it) closes up, so the
                // composer below slides into place instead of jumping.
                hero.children(crate::motion::appear_on_page(
                    &self.stage,
                    ("space-banner", 0),
                    !self.workspace.settings.space_banner_dismissed,
                    div()
                        .mt(px(40.))
                        .w_full()
                        .px(px(16.))
                        .pt(px(14.))
                        .pb(px(17.))
                        .rounded(px(12.))
                        .border_1()
                        .border_color(edge)
                        .bg(fill)
                        .flex()
                        .items_start()
                        .gap(px(12.))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap(px(6.))
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .line_height(px(18.))
                                        .font_weight(FontWeight::MEDIUM)
                                        .child("Your private space"),
                                )
                                .child(
                                    div()
                                        .text_size(px(13.))
                                        .line_height(px(18.))
                                        .text_color(t.secondary)
                                        .child("Your notes live here by default. Nothing gets shared until you choose to share it."),
                                ),
                        )
                        .child(
                            tool_tip("dismiss-space-banner", "close", "Dismiss", t)
                                .size(px(22.))
                                .mt(px(-6.))
                                .mr(px(-6.))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.workspace.settings.space_banner_dismissed = true;
                                    this.persist(cx);
                                    cx.notify();
                                })),
                        ),
                ))
            })
            .child(
                // The composer sits 16 px in from the column.
                div().mt(px(56.)).w_full().px(px(16.)).child(
                div()
                    .w_full()
                    .max_w(px(680.))
                    .h(px(57.))
                    .rounded_full()
                    .bg(t.card)
                    .border_1()
                    .border_color(if focused && !asking { t.accent } else { t.border })
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .pl(px(20.))
                    .pr(px(10.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(14.))
                            .line_height(px(22.))
                            // While the chat panel is open it owns the input.
                            .child(if asking {
                                div()
                                    .text_color(t.muted)
                                    .child("Continue in the chat below")
                                    .into_any_element()
                            } else {
                                self.ask.clone().into_any_element()
                            }),
                    )
                    .child(
                        crate::ui::send_button("space-ask-send", "Send · Enter", can_send && !asking, t)
                            .on_click(cx.listener(|this, _, _, cx| this.submit_chat(cx))),
                    ),
                ),
            )
            .child(div().mt(px(41.)).w_full().px(px(8.)).child(separator(t)))
            .child(
                div().mt(px(55.)).mb(px(9.)).w_full().flex().child(
                    // The one list this space has. Companies and People have
                    // no data behind them here, so they are not offered.
                    div()
                        .h(px(26.))
                        .px(px(11.))
                        .rounded_full()
                        .border_1()
                        .border_color(t.hairline)
                        .bg(t.hover)
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .text_size(px(12.))
                        .text_color(t.text)
                        .child(icon("notes-lines", 13., t.secondary))
                        .child("Notes"),
                ),
            )
    }

    fn calendar(&self, cx: &mut Context<Self>) -> Div {
        // Debug builds: `CEREAL_TOUR_LINKED_CALENDAR` draws the server calendar card from the
        // tour's made-up events, so its layout can be captured without a server.
        if crate::server::enabled()
            || (cfg!(debug_assertions) && crate::environment::var_os("CEREAL_TOUR_LINKED_CALENDAR").is_some())
        {
            return self.connected_calendar(cx);
        }
        let t = self.theme();
        let heading_hover = crate::motion::reveal_key("coming-up-heading");
        let today = Local::now().date_naive() + Duration::days((self.calendar_page * 7) as i64);
        let title = if self.calendar_page == 0 {
            "Coming up".to_string()
        } else {
            format!("{} {}", today.format("%B"), today.day())
        };
        // Paging replaces the days: they arrive from the side paged towards
        // while the title swaps, and "Today" grows in beside the arrows.
        let turn = crate::calendar::page_turn(self.calendar_page);
        let paged = self.stage.swap(("calendar-page", 0), self.calendar_page as u64);
        let mut rows = div().w_full().flex().flex_col();
        // Today and the next two days with events, as the linked card shows.
        for (i, delta) in [0, 4, 5].iter().enumerate() {
            let day = today + Duration::days(*delta);
            let events = [
                ("No events today", ""),
                ("Product & design sync", "10:30 – 11:00 AM"),
                ("Weekly team standup", "9:00 – 9:30 AM"),
                ("Customer discovery", "2:00 – 2:45 PM"),
                ("Engineering planning", "3:30 – 4:15 PM"),
            ];
            let (mut name, time) = events[i];
            if i == 0 && self.calendar_page != 0 {
                name = "No events on this day";
            }
            let row_id = i as u64;
            let mut row = div()
                .relative()
                .flex()
                .items_center()
                .h(px(60.))
                .flex_shrink_0()
                .px(px(16.));
            // Between days, the same dashed hairline as the signed-in card.
            if i > 0 {
                row = row.child(
                    crate::calendar::dashed_rule(t.hairline)
                        .absolute()
                        .top(px(-hairline(0.5)))
                        .left_0()
                        .right_0()
                        .h(px(hairline(0.5))),
                );
            }
            // The event is what is pressed, and what lights: from 8 pt
            // before its colour bar to 8 pt inside the card's edge, never
            // the date beside it (as on the signed-in card). A day with no
            // event has nothing to press.
            let mut cell = div()
                .id(("calendar-event", row_id))
                .relative()
                .flex_1()
                .min_w_0()
                .h(px(44.))
                .ml(px(-8.))
                .mr(px(-8.))
                .px(px(8.))
                .rounded(px(8.))
                .flex()
                .items_center();
            if i > 0 {
                cell = cell
                    .child(crate::motion::tactile(t.hover).radius(8.))
                    .cursor_default()
                    .reach()
                    .on_click(cx.listener(move |this, _, w, cx| {
                        let existing = this
                            .workspace
                            .notes
                            .iter()
                            .find(|n| n.title == name && !n.trashed)
                            .map(|n| n.id);
                        if let Some(id) = existing {
                            this.open_note(id, cx);
                        } else {
                            this.new_note(w, cx);
                            if let Some(n) =
                                this.current_note.and_then(|id| this.workspace.note_mut(id))
                            {
                                n.title = name.into();
                            }
                            this.title_input
                                .update(cx, |input, cx| input.set_content(name, cx));
                            this.persist(cx);
                        }
                    }));
            }
            row = row
                .child(
                    div()
                        .w(px(152.))
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .child(
                            div()
                                .w(px(40.))
                                .font_family(DISPLAY)
                                .text_size(px(27.))
                                .line_height(px(32.))
                                .child(day.day().to_string()),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .text_size(px(11.))
                                .line_height(px(12.))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(4.))
                                        .child(day.format("%B").to_string())
                                        .when(i == 0 && self.calendar_page == 0, |s| {
                                            s.child(
                                                div().size(px(5.)).rounded_full().bg(rgb(0xc84f34)),
                                            )
                                        }),
                                )
                                .child(
                                    div()
                                        .text_color(t.muted)
                                        .child(day.format("%a").to_string()),
                                ),
                        ),
                )
                .child(
                    cell.child(
                        div()
                            .w(px(3.))
                            .h(px(28.))
                            .rounded_full()
                            .flex_shrink_0()
                            .bg(if i == 0 { rgb(0xb5b6ae) } else { rgb(0x87e5e8) }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .pl(px(12.))
                            .flex()
                            .flex_col()
                            .justify_center()
                            .gap(px(2.))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .child(
                                div()
                                    .truncate()
                                    .text_color(if i == 0 { t.muted } else { t.text })
                                    .child(name),
                            )
                            .when(!time.is_empty(), |s| {
                                s.child(time_label(time, t).flex_shrink())
                            }),
                    ),
                );
            rows = rows.child(row);
        }
        let today_pill = self.stage.presence_on_page(
            ("calendar-today", 0),
            self.calendar_page != 0,
            crate::motion::SNAPPY,
        );
        div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .w_full()
            .pt(px(10.))
            .child(
                div()
                    .id("coming-up-heading")
                    .child(crate::motion::hover_probe(heading_hover))
                    .h(px(37.))
                    .px(px(8.))
                    .flex()
                    .items_start()
                    .justify_between()
                    .child(crate::motion::swapped(heading(title, t), paged))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(1.))
                            .when(today_pill.visible, |s| {
                                // On its way out it is only its outline
                                // and its word: nothing under the pointer
                                // that pressed it lights as it goes.
                                let today = if self.calendar_page != 0 {
                                    pill("calendar-today", "Today", "", t).on_click(cx.listener(
                                        |this, _, _, cx| {
                                            this.calendar_page = 0;
                                            cx.notify();
                                        },
                                    ))
                                } else {
                                    pill_frame("calendar-today", t).child("Today")
                                };
                                s.child(crate::motion::pop(
                                    today,
                                    crate::motion::Pop::chip((1., 0.5)),
                                    today_pill,
                                ))
                            })
                            .child(
                                tool_tip(
                                    "calendar-settings",
                                    "sliders-horizontal",
                                    "Calendar settings",
                                    t,
                                )
                                .opacity(crate::motion::revealed(heading_hover))
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.open_settings(
                                            crate::screens::SettingsTab::Calendar,
                                            cx,
                                        )
                                    },
                                )),
                            )
                            .child(
                                tool("calendar-prev", "chevron-left", t)
                                    .size(px(24.))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.calendar_page -= 1;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                tool("calendar-next", "chevron-right", t)
                                    .size(px(24.))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.calendar_page += 1;
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .rounded(px(17.))
                    .bg(t.card)
                    .border_1()
                    .border_color(t.border)
                    .overflow_hidden()
                    .child(crate::motion::paged(rows, paged, turn)),
            )
    }

    /// Home's composer. At rest it is a single "Ask anything" pill; focused
    /// (or holding a draft) it shows the model and send controls, and folds
    /// back when the focus leaves with nothing typed. Focused, a tray swells
    /// out from behind it with the recent-chats button above: the footprint
    /// the conversation then grows from (`render_note_assistant`).
    ///
    /// The pill itself never moves: its box, its padding and its text are
    /// the same at rest, focused and all the way between (`motion::AskShape`
    /// is everything that changes). A press anywhere outside the pill and
    /// its tray gives the focus back to the page, which folds the tray.
    pub fn ask_bar(&self, window: &Window, cx: &mut Context<Self>) -> Div {
        use crate::motion::{ASK_HEAD, ASK_INSET, ASK_TRAY};
        let t = self.theme();
        let focused = self.ask.read(cx).focus_handle.is_focused(window);
        let typed = !self.ask.read(cx).content().trim().is_empty();
        // Back on screen after the panel, another page or a note: the bar is
        // simply in the state it is in. What its tracks were doing when it
        // left is not played now.
        if !self.stage.continuous(("ask-bar", 0)) {
            self.stage.settle(("ask-tray", 0), focused);
            self.stage.settle(("chat-composer-ring", 0), focused);
        }
        self.stage.forget(("assistant-foot", 0));
        let tray = self.stage.presence(("ask-tray", 0), focused, ASK_TRAY);
        let shape = crate::motion::ask_shape(tray);
        div()
            .absolute()
            // The pill is 58 px tall, 675 px wide and 27 px above the
            // window's edge, always. Focused, its 3 px halo is drawn inside
            // its outline and the tray stands 4 px outside it (683 px wide,
            // ending 4 px below) and 39 px above, as the reference measures.
            .bottom(px(27.))
            .left_0()
            .right_0()
            .flex()
            .justify_center()
            .px(px(24.))
            .child(bar_scrim(t, 0.))
            .child(
                div()
                    .relative()
                    .w_full()
                    .max_w(px(675. + 2. * ASK_INSET))
                    .px(px(ASK_INSET))
                    // A press on the pill's own edge or padding is a press
                    // on the field.
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            this.ask.read(cx).focus_handle.focus(window);
                            cx.notify();
                        }),
                    )
                    // The tray's whole footprint, there or not: a press
                    // outside it leaves the bar. The press itself goes on to
                    // whatever it was aimed at. With the model menu open it
                    // only closes the menu (the menu sees to that).
                    .child(
                        div()
                            .absolute()
                            .top(px(-ASK_HEAD))
                            .bottom(px(-ASK_INSET))
                            .left_0()
                            .right_0()
                            .on_mouse_down_out(cx.listener(
                                |this, event: &MouseDownEvent, window, cx| {
                                    if event.button == MouseButton::Left
                                        && !this.chat_popover_open()
                                        && this.ask.read(cx).focus_handle.is_focused(window)
                                    {
                                        this.focus.focus(window);
                                        cx.notify();
                                    }
                                },
                            )),
                    )
                    .when(tray.visible, |bar| {
                        bar.child(
                            div()
                                .absolute()
                                // It grows out of the pill itself: from the
                                // pill's own outline to the full tray.
                                .top(px(-shape.head))
                                .bottom(px(-shape.out))
                                .left(px(ASK_INSET - shape.out))
                                .right(px(ASK_INSET - shape.out))
                                .rounded(px(shape.radius))
                                .border_1()
                                .border_color(t.border.opacity(shape.opacity))
                                .bg(t.card.opacity(shape.opacity))
                                .shadow(vec![BoxShadow {
                                    color: hsla(0., 0., 0.16, 0.06 * shape.shadow),
                                    offset: point(px(0.), px(4.)),
                                    blur_radius: px(16.),
                                    spread_radius: px(0.),
                                }])
                                .when(shape.history > 0., |tray| {
                                    tray.child(
                                        // The glyph is 14 px, centred 28 px
                                        // in and 20 px down.
                                        div()
                                            .absolute()
                                            .left(px(13.))
                                            .top(px(5. + shape.history_drop))
                                            .opacity(shape.history)
                                            .child(
                                                with_tooltip(
                                                    div()
                                                        .id("home-chat-history")
                                                        .size(px(28.))
                                                        .rounded(px(6.))
                                                        .flex()
                                                        .items_center()
                                                        .justify_center()
                                                        .cursor_default()
                                                        .child(
                                                            crate::motion::tactile(t.hover)
                                                                .radius(6.),
                                                        )
                                                        .child(icon("chat-history", 17., t.muted))
                                                        .reach(),
                                                    "Recent chats".into(),
                                                    t,
                                                )
                                                // On the press, not the click:
                                                // the press is also what takes
                                                // the focus from the field.
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    cx.listener(|this, _, _, cx| {
                                                        cx.stop_propagation();
                                                        this.assistant_open = true;
                                                        this.chat_history_open = true;
                                                        cx.notify();
                                                    }),
                                                ),
                                            ),
                                    )
                                }),
                        )
                    })
                    // An answer arriving keeps its stop button in the folded
                    // bar: it says the conversation is still going.
                    .child(self.render_chat_composer_in_tray(
                        focused,
                        !focused && !typed && self.chat_stream.is_none(),
                        cx,
                    )),
            )
    }
}

pub fn date_label(date: &str) -> String {
    let today = Local::now().date_naive();
    match NaiveDate::parse_from_str(date, "%Y-%m-%d") {
        Ok(d) if d == today => "Today".into(),
        Ok(d) if d == today - Duration::days(1) => "Yesterday".into(),
        Ok(d) => d.format("%a, %b %-d").to_string(),
        Err(_) => date.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Workspace;
    // `gpui::*` re-exports its own `test` attribute; these are plain unit tests.
    use std::prelude::v1::test;

    #[test]
    fn list_rows_carry_what_a_row_draws_in_list_order() {
        let workspace = Workspace::demo();
        let notes = workspace.visible_notes(false, "");
        let rows = home_rows(&notes);
        assert_eq!(rows.len(), notes.len());
        for (row, note) in rows.iter().zip(&notes) {
            assert_eq!(row.id, note.id);
            assert_eq!(row.date, note.timeline_date());
            assert_eq!(row.time.as_ref(), note.time.as_str());
        }
        // Newest day first, exactly as the list was sorted.
        assert!(rows.windows(2).all(|pair| pair[0].date >= pair[1].date));
        let untitled = Note::default();
        assert_eq!(home_rows(&[&untitled])[0].title.as_ref(), "New note");
    }

    #[test]
    fn the_arrow_keys_step_through_the_rows_and_stop_at_the_ends() {
        let ids = [7, 3, 9];
        assert_eq!(row_step(&ids, 7, false), Some(3));
        assert_eq!(row_step(&ids, 3, false), Some(9));
        assert_eq!(row_step(&ids, 9, false), None);
        assert_eq!(row_step(&ids, 9, true), Some(3));
        assert_eq!(row_step(&ids, 7, true), None);
        // A row that is no longer in the list has no neighbours.
        assert_eq!(row_step(&ids, 4, false), None);
        assert_eq!(row_step(&[], 4, true), None);
    }

    #[test]
    fn search_snippets_show_the_first_matching_line_or_the_title() {
        let note = Note {
            title: "Planning".into(),
            body: "Agenda\n  Review the Prototype with Maya  \nNext".into(),
            enhanced: "## Summary\nPrototype approved".into(),
            ..Default::default()
        };
        // Case-insensitive, trimmed, and the body wins over the summary.
        assert_eq!(
            search_snippet(&note, "prototype"),
            "Review the Prototype with Maya"
        );
        assert_eq!(search_snippet(&note, "approved"), "Prototype approved");
        assert_eq!(search_snippet(&note, "nothing-here"), "Planning");
        let long = Note {
            body: "x".repeat(400),
            ..Default::default()
        };
        assert_eq!(search_snippet(&long, "x").chars().count(), 140);
        assert!(search_snippet(&long, "x").ends_with('…'));
        // A match far along a long line is still in the excerpt, which says
        // at both ends that it is a part.
        let far = Note {
            body: format!("{} needle {}", "a ".repeat(200), "b ".repeat(200)),
            ..Default::default()
        };
        let excerpt = search_snippet(&far, "needle");
        assert!(excerpt.contains("needle"));
        assert!(excerpt.starts_with('…') && excerpt.ends_with('…'));
        assert_eq!(excerpt.chars().count(), 140);
        // Markdown's own marks are not part of the words.
        let marked = Note {
            enhanced: "## Launch scope\n- Ship the launch on Thursday".into(),
            ..Default::default()
        };
        assert_eq!(search_snippet(&marked, "scope"), "Launch scope");
        assert_eq!(search_snippet(&marked, "thursday"), "Ship the launch on Thursday");
    }

    #[test]
    fn workspace_revision_changes_with_every_way_the_lists_can_change() {
        let mut workspace = Workspace::demo();
        let start = workspace.revision();
        // Reading never invalidates derived lists.
        let first = workspace.notes[0].id;
        let _ = workspace.note(first);
        let _ = workspace.visible_notes(false, "sync");
        assert_eq!(workspace.revision(), start);
        // Any mutable access might have changed a title, date or trash flag.
        workspace.note_mut(first).unwrap().trashed = true;
        let trashed = workspace.revision();
        assert_ne!(trashed, start);
        workspace.create_note();
        let created = workspace.revision();
        assert_ne!(created, trashed);
        workspace.note_mut(first).unwrap().favorite = true;
        assert_ne!(workspace.revision(), created);
        // A replaced workspace (account switch, reload) is a different list.
        assert_ne!(
            Workspace::demo().revision(),
            Workspace::default().revision()
        );
    }
}
