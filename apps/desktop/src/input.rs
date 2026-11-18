// Native input plumbing adapted from GPUI's Apache-2.0 licensed input example.
// Copyright Zed Industries and contributors. Multiline layout uses StyledText.
use gpui::{prelude::*, *};
use std::{
    ops::Range,
    time::{Duration, Instant},
};
use unicode_segmentation::UnicodeSegmentation;

fn normalized_input(text: &str, multiline: bool) -> String {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    if multiline {
        text
    } else {
        text.replace('\n', " ")
    }
}

const BULLET_MARKER: &str = "• ";

fn list_indent_len(line: &str) -> usize {
    line.bytes()
        .take_while(|byte| matches!(byte, b' ' | b'\t'))
        .count()
}

/// Convert pasted plain-text list markers only where a new line actually starts.
fn formatted_pasted_lists(text: &str, first_line_is_start: bool) -> String {
    text.split('\n')
        .enumerate()
        .map(|(index, line)| {
            let indent = list_indent_len(line);
            if (index > 0 || first_line_is_start)
                && let Some(item) = line[indent..].strip_prefix("- ")
            {
                format!("{}{BULLET_MARKER}{item}", &line[..indent])
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// What one press of Tab adds before a list item, and what Shift-Tab takes
/// away (a pasted list indented by less, or by a tab, loses what it has).
const LIST_INDENT: &str = "    ";

/// How long a list marker at the start of `item` is, in bytes, with the
/// space after it: a bullet (typed or as it came, "- "), or a number
/// ("12. "), whose value comes back too.
fn list_marker(item: &str) -> Option<(usize, Option<u64>)> {
    if item.starts_with(BULLET_MARKER) {
        return Some((BULLET_MARKER.len(), None));
    }
    if item.starts_with("- ") {
        return Some((2, None));
    }
    let digits = item.bytes().take_while(u8::is_ascii_digit).count();
    if (1..=9).contains(&digits) && item[digits..].starts_with(". ") {
        return Some((digits + 2, item[..digits].parse().ok()));
    }
    None
}

/// Tab and Shift-Tab on list items: every list item on the lines the
/// selection touches moves one level in (or out). Returns the new text and
/// where the selection is in it, or `None` when nothing moved (no list item
/// there, or nothing left to take away), so the key can do what it does
/// elsewhere.
fn shifted_list_lines(
    content: &str,
    selected: Range<usize>,
    outdent: bool,
) -> Option<(String, Range<usize>)> {
    let first = content[..selected.start].rfind('\n').map_or(0, |index| index + 1);
    let last = content[selected.end..]
        .find('\n')
        .map_or(content.len(), |index| selected.end + index);
    let mut out = String::with_capacity(content.len() + LIST_INDENT.len());
    out.push_str(&content[..first]);
    let (mut start, mut end) = (selected.start, selected.end);
    let mut moved = false;
    let mut at = first;
    for line in content[first..last].split('\n') {
        let indent = list_indent_len(line);
        if list_marker(&line[indent..]).is_none() {
            out.push_str(line);
        } else if outdent {
            let cut = if line.starts_with('\t') {
                1
            } else {
                line.bytes().take(LIST_INDENT.len()).take_while(|byte| *byte == b' ').count()
            };
            moved |= cut > 0;
            let shift = |offset: usize| {
                if offset >= at + cut {
                    offset - cut
                } else {
                    offset.min(at)
                }
            };
            // Offsets are in the old text: lines before this one are done.
            if selected.start >= at {
                start = shift(selected.start) + out.len() - at;
            }
            if selected.end >= at {
                end = shift(selected.end) + out.len() - at;
            }
            out.push_str(&line[cut..]);
        } else {
            moved = true;
            if selected.start >= at {
                start = selected.start + LIST_INDENT.len() + out.len() - at;
            }
            if selected.end >= at {
                end = selected.end + LIST_INDENT.len() + out.len() - at;
            }
            out.push_str(LIST_INDENT);
            out.push_str(line);
        }
        at += line.len() + 1;
        if at <= last {
            out.push('\n');
        }
    }
    out.push_str(&content[last..]);
    moved.then_some((out, start..end))
}

/// The lines of a note that are headings ("# ", "## " or "### " at the
/// start of a line): the marker's bytes and the heading's own.
fn heading_spans(text: &str) -> Vec<(Range<usize>, Range<usize>)> {
    let mut spans = Vec::new();
    let mut at = 0;
    for line in text.split('\n') {
        let hashes = line.bytes().take_while(|byte| *byte == b'#').count();
        if (1..=3).contains(&hashes) && line[hashes..].starts_with(' ') && line.len() > hashes + 1 {
            spans.push((at..at + hashes + 1, at + hashes + 1..at + line.len()));
        }
        at += line.len() + 1;
    }
    spans
}

/// A run of typing or of erasing that one undo takes back.
#[derive(Clone, Copy, Debug, PartialEq)]
enum EditKind {
    Type,
    Erase,
}

#[derive(Clone, Copy, Debug)]
struct EditRun {
    kind: EditKind,
    /// Where the edit left the caret.
    caret: usize,
    at: Instant,
    /// The last thing typed was white space: the next word is a new step.
    space: bool,
}

/// A pause this long between two keys ends the run they would have shared.
const UNDO_PAUSE: Duration = Duration::from_millis(1000);

/// What kind of run an edit can belong to: one character typed at the
/// caret, or one erased. Anything else (a paste, a line break, a selection
/// replaced, a list marker formatted) is an undo step of its own.
fn edit_kind(content: &str, range: &Range<usize>, text: &str) -> Option<EditKind> {
    if range.is_empty() && text != "\n" && text.graphemes(true).count() == 1 {
        Some(EditKind::Type)
    } else if text.is_empty()
        && content
            .get(range.clone())
            .is_some_and(|erased| erased != "\n" && erased.graphemes(true).count() == 1)
    {
        Some(EditKind::Erase)
    } else {
        None
    }
}

/// Whether an edit carries on the run before it, and so shares its undo
/// step: the same kind of edit, where the last one left the caret, without
/// a pause. Typing starts a new step with each word (the space belongs to
/// the word before it), so undo steps back word by word.
fn joins_run(
    run: Option<EditRun>,
    kind: Option<EditKind>,
    range: &Range<usize>,
    text: &str,
    now: Instant,
) -> bool {
    let (Some(run), Some(kind)) = (run, kind) else {
        return false;
    };
    if run.kind != kind || now.saturating_duration_since(run.at) > UNDO_PAUSE {
        return false;
    }
    match kind {
        EditKind::Type => {
            run.caret == range.start && !(run.space && !text.chars().all(char::is_whitespace))
        }
        EditKind::Erase => run.caret == range.end || run.caret == range.start,
    }
}

/// The caret is solid for this long after it moved or something was typed,
/// then off and on for as long each.
const CARET_BLINK: Duration = Duration::from_millis(530);

/// Whether a caret that last moved `idle` ago is drawn right now.
fn caret_shown(idle: Duration) -> bool {
    (idle.as_millis() / CARET_BLINK.as_millis()) % 2 == 0
}

/// How long until a caret that last moved `idle` ago next turns on or off.
fn caret_turns_in(idle: Duration) -> Duration {
    let blink = CARET_BLINK.as_millis();
    Duration::from_millis((blink - idle.as_millis() % blink) as u64 + 1)
}

/// Return one replacement operation so formatting shares the native edit's
/// existing undo snapshot and all offsets remain UTF-8 byte boundaries.
fn formatted_list_edit(
    content: &str,
    range: Range<usize>,
    inserted: &str,
) -> (Range<usize>, String) {
    let line_start = content[..range.start]
        .rfind('\n')
        .map_or(0, |index| index + 1);
    let prefix = &content[line_start..range.start];
    let indent = list_indent_len(prefix);
    let item_prefix = &prefix[indent..];

    if range.is_empty() && inserted == " " && item_prefix == "-" {
        return (line_start + indent..range.end, BULLET_MARKER.into());
    }

    if range.is_empty()
        && inserted == "\n"
        && let Some((marker, number)) = list_marker(item_prefix)
    {
        let item = &item_prefix[marker..];
        let suffix = content[range.end..].split('\n').next().unwrap_or("");
        if item.trim().is_empty() && suffix.trim().is_empty() {
            // Return on an empty item ends the list; on a nested one it
            // comes out a level first, as Shift-Tab would take it.
            let cut = if prefix.starts_with('\t') {
                1
            } else {
                prefix.bytes().take(LIST_INDENT.len()).take_while(|byte| *byte == b' ').count()
            };
            if cut > 0 {
                return (line_start..line_start + cut, String::new());
            }
            return (line_start..range.end, String::new());
        }
        return (
            range,
            match number {
                Some(number) => format!("\n{}{}. ", &prefix[..indent], number.saturating_add(1)),
                None => format!("\n{}{BULLET_MARKER}", &prefix[..indent]),
            },
        );
    }

    (
        range,
        formatted_pasted_lists(inserted, prefix.len() == indent),
    )
}

fn byte_offset_from_utf16(text: &str, offset: usize) -> usize {
    let mut units = 0;
    for (i, ch) in text.char_indices() {
        if units + ch.len_utf16() > offset {
            return i;
        }
        units += ch.len_utf16();
    }
    text.len()
}

fn scroll_axis(
    offset: Pixels,
    start: Pixels,
    end: Pixels,
    viewport_start: Pixels,
    viewport_end: Pixels,
    max: Pixels,
) -> Pixels {
    let next = if start < viewport_start {
        offset + viewport_start - start
    } else if end > viewport_end {
        offset + viewport_end - end
    } else {
        offset
    };
    next.clamp(-max, px(0.))
}

actions!(
    cereal_input,
    [
        Backspace,
        Delete,
        Left,
        Right,
        Up,
        Down,
        SelectLeft,
        SelectRight,
        SelectUp,
        SelectDown,
        SelectAll,
        Home,
        End,
        DocumentHome,
        DocumentEnd,
        SelectHome,
        SelectEnd,
        Paste,
        Cut,
        Copy,
        Enter,
        LineBreak,
        Submit,
        Undo,
        Redo,
        Indent,
        Outdent,
        ShowCharacterPalette
    ]
);

#[derive(Clone, Debug)]
pub struct InputChanged {
    pub content: String,
}
#[derive(Clone, Debug)]
pub struct InputSubmitted {
    pub content: String,
}

#[derive(Clone, Debug)]
pub struct InputFocused;

#[derive(Clone)]
struct Snapshot {
    content: SharedString,
    selected: Range<usize>,
    reversed: bool,
}

pub struct TextInput {
    pub focus_handle: FocusHandle,
    content: SharedString,
    placeholder: SharedString,
    multiline: bool,
    secure: bool,
    submit_on_enter: bool,
    wrap: bool,
    list_formatting: bool,
    read_only: bool,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    last_layout: Option<TextLayout>,
    scroll_handle: ScrollHandle,
    autoscroll_requested: bool,
    is_selecting: bool,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    /// The run of typing or erasing the last edit belongs to (see
    /// [`joins_run`]); none after anything else happened.
    run: Option<EditRun>,
    /// When the caret last moved or something was typed: it is solid from
    /// then and blinks after (see [`caret_shown`]).
    caret_since: Instant,
    blink: Option<Task<()>>,
    /// A place in the window the caret is to go to once the text is laid
    /// out (see [`TextInput::place_caret`]).
    pending_caret: Option<Point<Pixels>>,
    focus_subscription: Option<Subscription>,
    blur_subscription: Option<Subscription>,
}
impl EventEmitter<InputChanged> for TextInput {}
impl EventEmitter<InputSubmitted> for TextInput {}
impl EventEmitter<InputFocused> for TextInput {}

impl TextInput {
    pub fn new(placeholder: &str, multiline: bool, cx: &mut Context<Self>) -> Self {
        Self {
            // Every text field is a stop for Tab and Shift-Tab.
            focus_handle: cx.focus_handle().tab_stop(true),
            content: "".into(),
            placeholder: placeholder.to_owned().into(),
            multiline,
            secure: false,
            submit_on_enter: false,
            wrap: multiline,
            list_formatting: false,
            read_only: false,
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            last_layout: None,
            scroll_handle: ScrollHandle::new(),
            autoscroll_requested: false,
            is_selecting: false,
            undo: vec![],
            redo: vec![],
            run: None,
            caret_since: Instant::now(),
            blink: None,
            pending_caret: None,
            focus_subscription: None,
            blur_subscription: None,
        }
    }
    pub fn set_secure(&mut self, cx: &mut Context<Self>) {
        self.secure = true;
        cx.notify();
    }
    pub fn content(&self) -> String {
        self.content.to_string()
    }
    /// Chat keeps pasted/newline text intact while Enter sends and Shift+Enter
    /// inserts a line break. Other native editor fields retain their behavior.
    pub fn set_chat_mode(&mut self, cx: &mut Context<Self>) {
        self.multiline = true;
        self.submit_on_enter = true;
        self.set_wrap(true, cx);
    }
    /// Enable plain-text bullet formatting for future committed multiline
    /// edits. Existing content and non-multiline fields remain unchanged.
    pub fn set_list_formatting(&mut self, enabled: bool) {
        self.list_formatting = enabled;
    }
    /// Show text that is not the user's to change: it can be selected and
    /// copied, and nothing typed, pasted, cut or undone alters it. No caret
    /// is drawn. Only `set_content` changes what it shows.
    pub fn set_read_only(&mut self, read_only: bool) {
        self.read_only = read_only;
    }
    /// Change soft wrapping without changing Enter or pasted-newline behavior.
    /// For example, a title can wrap visually while Enter still submits it.
    pub fn set_wrap(&mut self, wrap: bool, cx: &mut Context<Self>) {
        if self.wrap == wrap {
            return;
        }
        self.wrap = wrap;
        self.last_layout = None;
        self.scroll_handle.set_offset(Point::default());
        self.autoscroll_requested = true;
        cx.notify();
    }
    /// Attach the scroll region containing a wrapped editor. Unwrapped inputs
    /// track their own horizontal viewport automatically.
    pub fn attach_scroll(&mut self, handle: ScrollHandle) {
        self.scroll_handle = handle;
    }
    /// Put the caret where the pointer is, at `position` in the window, as
    /// a click on the text would: for a field that has just taken the place
    /// of the text that was clicked, before it has been laid out. It lands
    /// there on the field's first frame.
    pub fn place_caret(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        self.pending_caret = Some(position);
        cx.notify();
    }
    /// Take the keyboard with the caret before the first character.
    pub fn focus_at_start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
        window.focus(&self.focus_handle);
    }
    pub fn set_content(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.content = text.into();
        // One bullet glyph per note: list items that came with the text are
        // shown as the ones typed here are.
        if self.list_formatting && self.multiline && self.content.contains("- ") {
            self.content = formatted_pasted_lists(&self.content, true).into();
        }
        self.run = None;
        self.pending_caret = None;
        self.selected_range = self.content.len()..self.content.len();
        self.selection_reversed = false;
        self.marked_range = None;
        self.last_layout = None;
        self.scroll_handle.set_offset(Point::default());
        self.autoscroll_requested = false;
        self.undo.clear();
        self.redo.clear();
        cx.notify();
    }
    pub fn set_placeholder(
        &mut self,
        placeholder: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) {
        self.placeholder = placeholder.into();
        cx.notify();
    }
    fn snapshot(&self) -> Snapshot {
        Snapshot {
            content: self.content.clone(),
            selected: self.selected_range.clone(),
            reversed: self.selection_reversed,
        }
    }
    fn restore(&mut self, snap: Snapshot, cx: &mut Context<Self>) {
        self.content = snap.content;
        self.selected_range = snap.selected;
        self.selection_reversed = snap.reversed;
        self.marked_range = None;
        self.last_layout = None;
        self.run = None;
        self.changed(cx);
    }
    fn changed(&mut self, cx: &mut Context<Self>) {
        self.autoscroll_requested = true;
        self.caret_since = Instant::now();
        cx.emit(InputChanged {
            content: self.content(),
        });
        cx.notify();
    }
    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }
    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let offset = self.char_boundary(offset);
        self.selected_range = offset..offset;
        self.selection_reversed = false;
        self.autoscroll_requested = true;
        self.run = None;
        self.caret_since = Instant::now();
        cx.notify();
    }
    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let offset = self.char_boundary(offset);
        let anchor = if self.selection_reversed {
            self.selected_range.end
        } else {
            self.selected_range.start
        };
        self.selected_range = anchor.min(offset)..anchor.max(offset);
        self.selection_reversed = offset < anchor;
        self.autoscroll_requested = true;
        self.run = None;
        self.caret_since = Instant::now();
        cx.notify();
    }
    fn char_boundary(&self, offset: usize) -> usize {
        let mut i = offset.min(self.content.len());
        while !self.content.is_char_boundary(i) {
            i -= 1;
        }
        i
    }
    fn previous_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .rev()
            .find_map(|(i, _)| (i < offset).then_some(i))
            .unwrap_or(0)
    }
    fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .find_map(|(i, _)| (i > offset).then_some(i))
            .unwrap_or(self.content.len())
    }
    fn line_home(&self) -> usize {
        self.content[..self.cursor_offset()]
            .rfind('\n')
            .map(|i| i + 1)
            .unwrap_or(0)
    }
    fn line_end(&self) -> usize {
        let cursor = self.cursor_offset();
        self.content[cursor..]
            .find('\n')
            .map(|i| cursor + i)
            .unwrap_or(self.content.len())
    }
    fn vertical_offset(&self, delta: f32) -> usize {
        let Some(layout) = &self.last_layout else {
            return self.cursor_offset();
        };
        let Some(position) = layout.position_for_index(self.cursor_offset()) else {
            return self.cursor_offset();
        };
        let target = point(
            position.x,
            position.y + layout.line_height() * delta + layout.line_height() * 0.5,
        );
        layout
            .index_for_position(target)
            .unwrap_or_else(|i| i)
            .min(self.content.len())
    }
    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        let i = if self.selected_range.is_empty() {
            self.previous_boundary(self.cursor_offset())
        } else {
            self.selected_range.start
        };
        self.move_to(i, cx);
    }
    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        let i = if self.selected_range.is_empty() {
            self.next_boundary(self.cursor_offset())
        } else {
            self.selected_range.end
        };
        self.move_to(i, cx);
    }
    fn up(&mut self, _: &Up, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.vertical_offset(-1.), cx);
    }
    fn down(&mut self, _: &Down, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.vertical_offset(1.), cx);
    }
    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.cursor_offset()), cx);
    }
    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor_offset()), cx);
    }
    fn select_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.vertical_offset(-1.), cx);
    }
    fn select_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.vertical_offset(1.), cx);
    }
    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
        self.select_to(self.content.len(), cx);
    }
    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.line_home(), cx);
    }
    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.line_end(), cx);
    }
    fn document_home(&mut self, _: &DocumentHome, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }
    fn document_end(&mut self, _: &DocumentEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), cx);
    }
    fn select_home(&mut self, _: &SelectHome, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.line_home(), cx);
    }
    fn select_end(&mut self, _: &SelectEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.line_end(), cx);
    }
    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        if self.selected_range.is_empty() {
            // Selecting the character to erase is not the caret moving.
            let run = self.run;
            self.select_to(self.previous_boundary(self.cursor_offset()), cx);
            self.run = run;
        }
        self.replace_text_in_range(None, "", window, cx);
    }
    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        if self.selected_range.is_empty() {
            let run = self.run;
            self.select_to(self.next_boundary(self.cursor_offset()), cx);
            self.run = run;
        }
        self.replace_text_in_range(None, "", window, cx);
    }
    fn enter(&mut self, _: &Enter, window: &mut Window, cx: &mut Context<Self>) {
        if self.multiline && !self.submit_on_enter {
            self.replace_text_in_range(None, "\n", window, cx);
        } else {
            self.submit(&Submit, window, cx);
        }
    }
    fn line_break(&mut self, _: &LineBreak, window: &mut Window, cx: &mut Context<Self>) {
        if self.multiline {
            self.replace_text_in_range(None, "\n", window, cx);
        } else {
            self.submit(&Submit, window, cx);
        }
    }
    fn submit(&mut self, _: &Submit, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(InputSubmitted {
            content: self.content(),
        });
    }
    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.replace_text_in_range(None, &text, window, cx);
        }
    }
    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.secure && !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
        }
    }
    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            self.copy(&Copy, window, cx);
            self.replace_text_in_range(None, "", window, cx);
        }
    }
    fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        if let Some(snap) = self.undo.pop() {
            self.redo.push(self.snapshot());
            self.restore(snap, cx);
        }
    }
    fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        if let Some(snap) = self.redo.pop() {
            self.undo.push(self.snapshot());
            self.restore(snap, cx);
        }
    }
    /// Tab on a list item moves it a level in, and Shift-Tab a level out.
    /// Anywhere else the key is not this field's: it goes on to whatever it
    /// does there (the next control, or nothing).
    fn shift_list(&mut self, outdent: bool, cx: &mut Context<Self>) {
        let shifted = (self.list_formatting && self.multiline && !self.read_only)
            .then(|| shifted_list_lines(&self.content, self.selected_range.clone(), outdent))
            .flatten();
        let Some((content, selected)) = shifted else {
            cx.propagate();
            return;
        };
        self.undo.push(self.snapshot());
        self.redo.clear();
        self.content = content.into();
        self.selected_range = selected;
        self.marked_range = None;
        self.last_layout = None;
        self.run = None;
        self.changed(cx);
    }
    fn indent(&mut self, _: &Indent, _: &mut Window, cx: &mut Context<Self>) {
        self.shift_list(false, cx);
    }
    fn outdent(&mut self, _: &Outdent, _: &mut Window, cx: &mut Context<Self>) {
        self.shift_list(true, cx);
    }
    fn show_character_palette(
        &mut self,
        _: &ShowCharacterPalette,
        window: &mut Window,
        _: &mut Context<Self>,
    ) {
        window.show_character_palette();
    }
    fn index_for_mouse(&self, position: Point<Pixels>) -> usize {
        if self.content.is_empty() {
            return 0;
        }
        self.last_layout
            .as_ref()
            .map(|layout| {
                layout
                    .index_for_position(position)
                    .unwrap_or_else(|i| i)
                    .min(self.content.len())
            })
            .unwrap_or(0)
    }
    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle);
        cx.emit(InputFocused);
        self.is_selecting = true;
        let index = self.index_for_mouse(event.position);
        if event.click_count >= 3 {
            self.move_to(index, cx);
            let start = self.line_home();
            let end = self.line_end();
            self.move_to(start, cx);
            self.select_to(end, cx);
        } else if event.click_count == 2 {
            let mut found = None;
            for (start, word) in self.content.unicode_word_indices() {
                if index >= start && index <= start + word.len() {
                    found = Some(start..start + word.len());
                    break;
                }
            }
            if let Some(range) = found {
                self.selected_range = range;
                self.selection_reversed = false;
                cx.notify();
            } else {
                self.move_to(index, cx);
            }
        } else if event.modifiers.shift {
            self.select_to(index, cx);
        } else {
            self.move_to(index, cx);
        }
        cx.stop_propagation();
    }
    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }
    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            self.select_to(self.index_for_mouse(event.position), cx);
        }
    }
    fn offset_from_utf16(&self, offset: usize) -> usize {
        byte_offset_from_utf16(&self.content, offset)
    }
    fn offset_to_utf16(&self, offset: usize) -> usize {
        self.content[..self.char_boundary(offset)]
            .encode_utf16()
            .count()
    }
    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }
    fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range.start)..self.offset_from_utf16(range.end)
    }
}

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        actual: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        if self.secure {
            return None;
        }
        let range = self.range_from_utf16(&range);
        actual.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }
    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }
    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked_range.as_ref().map(|r| self.range_to_utf16(r))
    }
    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.marked_range = None;
        cx.notify();
    }
    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.read_only {
            return;
        }
        let range = range
            .as_ref()
            .map(|r| self.range_from_utf16(r))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        let text = normalized_input(new_text, self.multiline);
        let (range, text) = if self.list_formatting && self.multiline && self.marked_range.is_none()
        {
            formatted_list_edit(&self.content, range, &text)
        } else {
            (range, text)
        };
        if range.is_empty() && text.is_empty() {
            return;
        }
        // A run of typing (a word and the space after it) or of erasing is
        // one undo step: only its first edit saves the state before it.
        let now = Instant::now();
        let composing = self.marked_range.is_some();
        let kind = (!composing)
            .then(|| edit_kind(&self.content, &range, &text))
            .flatten();
        let joined = !self.undo.is_empty() && joins_run(self.run, kind, &range, &text, now);
        // The first preedit already saved the state before composition. A final
        // commit must share that undo step, rather than restoring preedit text.
        if !composing && !joined {
            self.undo.push(self.snapshot());
        }
        if self.undo.len() > 200 {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.content = format!(
            "{}{}{}",
            &self.content[..range.start],
            text,
            &self.content[range.end..]
        )
        .into();
        let cursor = range.start + text.len();
        self.selected_range = cursor..cursor;
        self.selection_reversed = false;
        self.marked_range = None;
        self.last_layout = None;
        self.run = kind.map(|kind| EditRun {
            kind,
            caret: cursor,
            at: now,
            space: text.chars().all(char::is_whitespace),
        });
        self.changed(cx);
    }
    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        new_text: &str,
        selection: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.read_only {
            return;
        }
        let range = range
            .as_ref()
            .map(|r| self.range_from_utf16(r))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        let new_text = normalized_input(new_text, self.multiline);
        self.run = None;
        if self.marked_range.is_none() {
            self.undo.push(self.snapshot());
            self.redo.clear();
        }
        self.content = format!(
            "{}{}{}",
            &self.content[..range.start],
            new_text,
            &self.content[range.end..]
        )
        .into();
        self.marked_range =
            (!new_text.is_empty()).then_some(range.start..range.start + new_text.len());
        let insertion = self.offset_to_utf16(range.start);
        self.selected_range = selection
            .map(|r| {
                self.range_from_utf16(
                    &(insertion + r.start.min(new_text.encode_utf16().count())
                        ..insertion + r.end.min(new_text.encode_utf16().count())),
                )
            })
            .unwrap_or(range.start + new_text.len()..range.start + new_text.len());
        self.selection_reversed = false;
        self.last_layout = None;
        self.changed(cx);
    }
    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let range = self.range_from_utf16(&range);
        let layout = self.last_layout.as_ref()?;
        let start = layout.position_for_index(range.start)?;
        let end = layout.position_for_index(range.end).unwrap_or(start);
        let width = if end.y == start.y {
            (end.x - start.x).max(px(1.))
        } else {
            px(1.)
        };
        Some(Bounds::new(start, size(width, layout.line_height())))
    }
    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        Some(self.offset_to_utf16(self.index_for_mouse(point)))
    }
}

struct TextElement {
    input: Entity<TextInput>,
    text: Option<StyledText>,
}
impl IntoElement for TextElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = ();
    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let input = self.input.read(cx);
        let placeholder = input.content.is_empty();
        let text = if placeholder {
            if input.placeholder.is_empty() {
                SharedString::from(" ")
            } else {
                input.placeholder.clone()
            }
        } else if input.secure {
            "*".repeat(input.content.len()).into()
        } else {
            input.content.clone()
        };
        let mut boundaries = vec![0, text.len()];
        // In a note's body a line that starts with "# " is a heading: it is
        // drawn heavier, and its marker steps back.
        let headings = if !placeholder && input.list_formatting && input.multiline {
            heading_spans(&text)
        } else {
            Vec::new()
        };
        if !placeholder {
            boundaries.extend([input.selected_range.start, input.selected_range.end]);
            if let Some(marked) = &input.marked_range {
                boundaries.extend([marked.start, marked.end]);
            }
            for (marker, heading) in &headings {
                boundaries.extend([marker.start, marker.end, heading.end]);
            }
        }
        boundaries.sort_unstable();
        boundaries.dedup();
        let style = window.text_style();
        // The input inherits its ink; light ink means a dark surface, and
        // the placeholder and selection have to follow.
        let dark = style.color.l > 0.5;
        let runs = boundaries
            .windows(2)
            .filter(|pair| pair[0] < pair[1])
            .map(|pair| {
                let mut run = style.to_run(pair[1] - pair[0]);
                if placeholder {
                    run.color = rgb(if dark { 0x9e9e99 } else { 0x72726e }).into();
                } else {
                    if let Some((marker, _)) = headings
                        .iter()
                        .find(|(marker, heading)| pair[0] >= marker.start && pair[1] <= heading.end)
                    {
                        run.font.weight = FontWeight::SEMIBOLD;
                        if pair[1] <= marker.end {
                            run.color = rgb(if dark { 0x9e9e99 } else { 0x72726e }).into();
                        }
                    }
                    if pair[0] >= input.selected_range.start
                        && pair[1] <= input.selected_range.end
                        && input.focus_handle.is_focused(window)
                    {
                        run.background_color =
                            Some(rgba(if dark { 0xe0af2f55 } else { 0xd9ceb788 }).into());
                    }
                    if input
                        .marked_range
                        .as_ref()
                        .is_some_and(|r| pair[0] >= r.start && pair[1] <= r.end)
                    {
                        run.underline = Some(UnderlineStyle {
                            color: Some(run.color),
                            thickness: px(1.),
                            wavy: false,
                        });
                    }
                }
                run
            })
            .collect();
        self.text = Some(StyledText::new(text).with_runs(runs));
        self.text
            .as_mut()
            .unwrap()
            .request_layout(id, inspector, window, cx)
    }
    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let text = self.text.as_mut().unwrap();
        text.prepaint(id, inspector, bounds, state, window, cx);
        let layout = text.layout().clone();
        let caret_scroll = self.input.update(cx, |input, _| {
            input.last_layout = Some(layout.clone());
            // A click that put this field here lands its caret now that
            // there is text to land it in.
            if let Some(position) = input.pending_caret.take() {
                let index = input.index_for_mouse(position);
                input.selected_range = index..index;
                input.selection_reversed = false;
                input.caret_since = Instant::now();
            }
            if !std::mem::take(&mut input.autoscroll_requested)
                || !input.focus_handle.is_focused(window)
            {
                return None;
            }
            let position = layout.position_for_index(input.cursor_offset())?;
            let viewport = input.scroll_handle.bounds();
            if viewport.size.width <= px(0.) || viewport.size.height <= px(0.) {
                return None;
            }
            let offset = input.scroll_handle.offset();
            let max = input.scroll_handle.max_offset();
            let caret = Bounds::new(position, size(px(2.), layout.line_height()));
            let next = if input.wrap {
                point(
                    offset.x,
                    scroll_axis(
                        offset.y,
                        caret.top(),
                        caret.bottom(),
                        viewport.top(),
                        viewport.bottom(),
                        max.height,
                    ),
                )
            } else {
                point(
                    scroll_axis(
                        offset.x,
                        caret.left(),
                        caret.right(),
                        viewport.left(),
                        viewport.right(),
                        max.width,
                    ),
                    offset.y,
                )
            };
            (next != offset).then_some((input.scroll_handle.clone(), next))
        });
        if let Some((handle, offset)) = caret_scroll {
            let input = self.input.clone();
            window.on_next_frame(move |_, cx| {
                handle.set_offset(offset);
                input.update(cx, |_, cx| cx.notify());
            });
        }
    }
    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut (),
        prepaint: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let input = self.input.read(cx);
        let focus = input.focus_handle.clone();
        let cursor = input.cursor_offset();
        // Text that cannot be changed has a selection but no caret.
        let empty_selection = input.selected_range.is_empty() && !input.read_only;
        let blink_on = caret_shown(input.caret_since.elapsed());
        // A selected line break has no glyph to carry the highlight: each
        // gets a sliver of it where the line ends, so a selection that runs
        // over empty lines is one continuous shape.
        let dark = window.text_style().color.l > 0.5;
        if focus.is_focused(window)
            && !input.selected_range.is_empty()
            && !input.secure
            && let Some(layout) = self.text.as_ref().map(|text| text.layout().clone())
        {
            let selected = input.selected_range.clone();
            let breaks: Vec<usize> = input.content[selected.clone()]
                .match_indices('\n')
                .map(|(index, _)| selected.start + index)
                .collect();
            let wash: Hsla = rgba(if dark { 0xe0af2f55 } else { 0xd9ceb788 }).into();
            let sliver = layout.line_height() * 0.3;
            for index in breaks {
                if let Some(position) = layout.position_for_index(index) {
                    window.paint_quad(fill(
                        Bounds::new(position, size(sliver, layout.line_height())),
                        wash,
                    ));
                }
            }
        }
        window.handle_input(
            &focus,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        // Div::on_mouse_move only runs while hovered. Selection must continue
        // when a drag moves outside the text input or past the final line.
        let selection_input = self.input.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble && selection_input.read(cx).is_selecting {
                selection_input.update(cx, |input, cx| input.on_mouse_move(event, window, cx));
            }
        });
        let text = self.text.as_mut().unwrap();
        text.paint(id, inspector, bounds, state, prepaint, window, cx);
        if focus.is_focused(window)
            && empty_selection
            && blink_on
            && let Some(position) = text.layout().position_for_index(cursor)
        {
            // A caret that keeps its contrast on the dark canvas too.
            let caret = if window.text_style().color.l > 0.5 {
                rgb(0xe0af2f)
            } else {
                rgb(0x715f3f)
            };
            window.paint_quad(fill(
                Bounds::new(position, size(px(1.5), text.layout().line_height())),
                caret,
            ));
        }
    }
}
impl Render for TextInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.focus_subscription.is_none() {
            let focus = self.focus_handle.clone();
            self.focus_subscription = Some(cx.on_focus(&focus, window, |this, _, cx| {
                this.caret_since = Instant::now();
                cx.emit(InputFocused);
                cx.notify();
            }));
            self.blur_subscription = Some(cx.on_blur(&focus, window, |this, _, cx| {
                this.blink = None;
                cx.notify();
            }));
        }
        // The caret blinks while the field has the keyboard: one redraw each
        // time it turns on or off, and none once the field has lost it.
        let blinking = !self.read_only && self.focus_handle.is_focused(window);
        if !blinking {
            self.blink = None;
        } else if self.blink.is_none() {
            self.blink = Some(cx.spawn(async move |this, cx| {
                loop {
                    let Ok(wait) = this.read_with(cx, |this, _| caret_turns_in(this.caret_since.elapsed()))
                    else {
                        break;
                    };
                    cx.background_executor().timer(wait).await;
                    if this.update(cx, |_, cx| cx.notify()).is_err() {
                        break;
                    }
                }
            }));
        }
        div()
            .id("text-input")
            .w_full()
            .min_w_0()
            .min_h(px(22.))
            .key_context("CerealInput")
            .track_focus(&self.focus_handle)
            .cursor(CursorStyle::IBeam)
            .when(!self.wrap, |d| {
                d.whitespace_nowrap()
                    .overflow_hidden()
                    .overflow_x_scroll()
                    .track_scroll(&self.scroll_handle)
            })
            .when(self.wrap, |d| d.whitespace_normal())
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::up))
            .on_action(cx.listener(Self::down))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_up))
            .on_action(cx.listener(Self::select_down))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::document_home))
            .on_action(cx.listener(Self::document_end))
            .on_action(cx.listener(Self::select_home))
            .on_action(cx.listener(Self::select_end))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::enter))
            .on_action(cx.listener(Self::line_break))
            .on_action(cx.listener(Self::submit))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .on_action(cx.listener(Self::indent))
            .on_action(cx.listener(Self::outdent))
            .on_action(cx.listener(Self::show_character_palette))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .child(TextElement {
                input: cx.entity(),
                text: None,
            })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CARET_BLINK, EditKind, EditRun, UNDO_PAUSE, byte_offset_from_utf16, caret_shown,
        caret_turns_in, edit_kind, formatted_list_edit, formatted_pasted_lists, heading_spans,
        joins_run, normalized_input, scroll_axis, shifted_list_lines,
    };
    use gpui::px;
    use std::time::{Duration, Instant};

    /// Type `text` a key at a time, `gap` apart, and count the undo steps.
    fn undo_steps(text: &str, gap: Duration) -> Vec<String> {
        let mut content = String::new();
        let mut steps: Vec<String> = Vec::new();
        let mut run: Option<EditRun> = None;
        let mut now = Instant::now();
        for character in text.chars() {
            let typed = character.to_string();
            let range = content.len()..content.len();
            let kind = edit_kind(&content, &range, &typed);
            if !joins_run(run, kind, &range, &typed, now) {
                steps.push(content.clone());
            }
            content.push(character);
            run = kind.map(|kind| EditRun {
                kind,
                caret: content.len(),
                at: now,
                space: character.is_whitespace(),
            });
            now += gap;
        }
        steps
    }

    #[test]
    fn typing_is_undone_a_word_at_a_time_not_a_letter() {
        let quick = Duration::from_millis(80);
        // One step per word; the space after a word belongs to it.
        assert_eq!(undo_steps("two", quick), vec![""]);
        assert_eq!(undo_steps("one two three", quick), vec!["", "one ", "one two "]);
        // A pause starts a new step in the middle of a word.
        assert_eq!(undo_steps("ab", UNDO_PAUSE + quick), vec!["", "a"]);
        // A line break is a step of its own, and so is what follows it.
        assert_eq!(undo_steps("a\nb", quick), vec!["", "a", "a\n"]);
    }

    #[test]
    fn a_run_ends_when_the_caret_moves_or_the_edit_changes_kind() {
        let now = Instant::now();
        let run = Some(EditRun { kind: EditKind::Type, caret: 3, at: now, space: false });
        assert!(joins_run(run, Some(EditKind::Type), &(3..3), "x", now));
        // Typed somewhere else.
        assert!(!joins_run(run, Some(EditKind::Type), &(1..1), "x", now));
        // Erasing after typing, and a paste, are their own steps.
        assert!(!joins_run(run, Some(EditKind::Erase), &(2..3), "", now));
        assert!(!joins_run(run, None, &(3..3), "pasted words", now));
        assert!(!joins_run(None, Some(EditKind::Type), &(3..3), "x", now));
        // Backspace after backspace, and forward delete, carry on a run.
        let erasing = Some(EditRun { kind: EditKind::Erase, caret: 3, at: now, space: false });
        assert!(joins_run(erasing, Some(EditKind::Erase), &(2..3), "", now));
        assert!(joins_run(erasing, Some(EditKind::Erase), &(3..4), "", now));
        assert!(!joins_run(erasing, Some(EditKind::Erase), &(7..8), "", now));
        // What counts as one key of a run.
        assert_eq!(edit_kind("abc", &(3..3), "d"), Some(EditKind::Type));
        assert_eq!(edit_kind("abc", &(3..3), "🦊"), Some(EditKind::Type));
        assert_eq!(edit_kind("abc", &(3..3), "\n"), None);
        assert_eq!(edit_kind("abc", &(3..3), "de"), None);
        assert_eq!(edit_kind("abc", &(1..3), "d"), None);
        assert_eq!(edit_kind("abc", &(2..3), ""), Some(EditKind::Erase));
        assert_eq!(edit_kind("abc", &(0..3), ""), None);
    }

    #[test]
    fn the_caret_is_solid_after_a_key_and_then_blinks() {
        assert!(caret_shown(Duration::ZERO));
        assert!(caret_shown(CARET_BLINK - Duration::from_millis(1)));
        assert!(!caret_shown(CARET_BLINK + Duration::from_millis(1)));
        assert!(caret_shown(CARET_BLINK * 2 + Duration::from_millis(1)));
        // The next redraw is asked for just after the caret turns.
        let wait = caret_turns_in(Duration::from_millis(100));
        assert!(wait > CARET_BLINK - Duration::from_millis(100));
        assert!(!caret_shown(Duration::from_millis(100) + wait));
    }

    #[test]
    fn tab_nests_list_items_and_shift_tab_brings_them_back() {
        let content = "• first\n• second\nplain";
        let caret = "• first\n• sec".len();
        let (nested, selected) = shifted_list_lines(content, caret..caret, false).unwrap();
        assert_eq!(nested, "• first\n    • second\nplain");
        assert_eq!(&nested[..selected.start], "• first\n    • sec");
        assert!(selected.is_empty());
        let (back, selected) = shifted_list_lines(&nested, selected, true).unwrap();
        assert_eq!(back, content);
        assert_eq!(selected, caret..caret);
        // Nothing left to take away, and a line that is not a list item:
        // the key is not the editor's.
        assert_eq!(shifted_list_lines(content, caret..caret, true), None);
        let plain = content.len();
        assert_eq!(shifted_list_lines(content, plain..plain, false), None);
        // A selection moves every list item it touches, and stays on the
        // same words; numbered items and pasted indents count too.
        let content = "1. café\n  - 漢字\nplain\n• last";
        let from = "1. ca".len();
        let to = content.len() - 2;
        let (moved, selected) = shifted_list_lines(content, from..to, false).unwrap();
        assert_eq!(moved, "    1. café\n      - 漢字\nplain\n    • last");
        assert_eq!(&moved[selected.clone()], "fé\n      - 漢字\nplain\n    • la");
        let (moved, selected) = shifted_list_lines(&moved, selected, true).unwrap();
        assert_eq!(moved, content);
        assert_eq!(selected, from..to);
        // A caret inside the indent that goes stays at the line's start.
        let (moved, selected) = shifted_list_lines("  • a", 1..1, true).unwrap();
        assert_eq!((moved.as_str(), selected), ("• a", 0..0));
    }

    #[test]
    fn numbered_lists_continue_and_nested_items_step_out() {
        fn enter(content: &str) -> String {
            let cursor = content.len();
            let (range, inserted) = formatted_list_edit(content, cursor..cursor, "\n");
            format!("{}{}{}", &content[..range.start], inserted, &content[range.end..])
        }
        assert_eq!(enter("1. one"), "1. one\n2. ");
        assert_eq!(enter("  9. nine"), "  9. nine\n  10. ");
        // Return on an empty item ends the list.
        assert_eq!(enter("1. one\n2. "), "1. one\n");
        // On an empty nested item it comes out a level and stays an item.
        assert_eq!(enter("• a\n    • "), "• a\n• ");
        // A year at the start of a sentence is not a list.
        assert_eq!(enter("2026 plans"), "2026 plans\n");
        assert_eq!(enter("3.14 is pi"), "3.14 is pi\n");
    }

    #[test]
    fn heading_lines_are_found_by_their_marker() {
        let text = "# Title\nbody # not\n## Second\n#hashtag\n#### too deep\n# ";
        let spans = heading_spans(text);
        let shown: Vec<(&str, &str)> = spans
            .iter()
            .map(|(marker, heading)| (&text[marker.clone()], &text[heading.clone()]))
            .collect();
        assert_eq!(shown, vec![("# ", "Title"), ("## ", "Second")]);
    }

    #[test]
    fn native_utf16_offsets_keep_unicode_boundaries() {
        let text = "a🦊e\u{301}漢";
        let byte_offsets: Vec<_> = (0..=text.encode_utf16().count())
            .map(|offset| byte_offset_from_utf16(text, offset))
            .collect();
        assert_eq!(byte_offsets, vec![0, 1, 1, 5, 6, 8, 11]);
        assert!(
            byte_offsets
                .into_iter()
                .all(|offset| text.is_char_boundary(offset))
        );
        assert_eq!(byte_offset_from_utf16(text, usize::MAX), text.len());
    }

    #[test]
    fn paste_and_ime_share_line_ending_rules() {
        let pasted = "First\r\nSecond\rThird\n🦊";
        assert_eq!(normalized_input(pasted, true), "First\nSecond\nThird\n🦊");
        assert_eq!(normalized_input(pasted, false), "First Second Third 🦊");
    }

    #[test]
    fn pasted_lists_preserve_paragraphs_indentation_and_unicode() {
        let pasted = normalized_input(
            "Review - unchanged\r\n\r\n- café 🦊\r\n  - 漢字\r\n• existing\r\n",
            true,
        );
        assert_eq!(
            formatted_pasted_lists(&pasted, true),
            "Review - unchanged\n\n• café 🦊\n  • 漢字\n• existing\n"
        );
        assert_eq!(
            formatted_pasted_lists("- inline\n- new line", false),
            "- inline\n• new line"
        );
    }

    #[test]
    fn typed_marker_is_one_replacement_at_unicode_safe_offsets() {
        let before = "Résumé 🦊\n  -";
        let cursor = before.len();
        let (range, inserted) = formatted_list_edit(before, cursor..cursor, " ");
        assert_eq!(&before[range.clone()], "-");
        let after = format!(
            "{}{}{}",
            &before[..range.start],
            inserted,
            &before[range.end..]
        );
        assert_eq!(after, "Résumé 🦊\n  • ");
        let caret = range.start + inserted.len();
        assert!(after.is_char_boundary(caret));
        assert_eq!(
            byte_offset_from_utf16(&after, after[..caret].encode_utf16().count()),
            caret
        );
        // Reverting this single replacement restores the pre-space snapshot.
        assert_eq!(format!("{}-", &after[..range.start]), before);
    }

    #[test]
    fn enter_continues_splits_and_finishes_plain_text_lists() {
        fn enter(content: &str, cursor: usize) -> String {
            let (range, inserted) = formatted_list_edit(content, cursor..cursor, "\n");
            format!(
                "{}{}{}",
                &content[..range.start],
                inserted,
                &content[range.end..]
            )
        }
        let content = "• café 🦊";
        assert_eq!(enter(content, content.len()), "• café 🦊\n• ");
        assert_eq!(enter(content, "• café".len()), "• café\n•  🦊");
        assert_eq!(enter("• item\n• ", "• item\n• ".len()), "• item\n");
        // An empty nested item steps out a level before it ends the list.
        assert_eq!(enter("  • \nnext", "  • ".len()), "• \nnext");
        assert_eq!(enter("• \nnext", "• ".len()), "\nnext");
        assert_eq!(enter("- legacy", "- legacy".len()), "- legacy\n• ");
        assert_eq!(enter("Plain 🦊", "Plain 🦊".len()), "Plain 🦊\n");
    }

    #[test]
    fn selected_replacement_does_not_rewrite_unselected_list_prefix() {
        let before = "• café 🦊\nbody";
        let start = before.find("café").unwrap();
        let end = start + "café".len();
        let (range, inserted) = formatted_list_edit(before, start..end, "- tea\n- 漢字");
        assert_eq!(range, start..end);
        let after = format!(
            "{}{}{}",
            &before[..range.start],
            inserted,
            &before[range.end..]
        );
        assert_eq!(after, "• - tea\n• 漢字 🦊\nbody");
        assert!(after.is_char_boundary(range.start + inserted.len()));
    }

    #[test]
    fn caret_scroll_keeps_current_view_when_visible_and_clamps_at_edges() {
        assert_eq!(
            scroll_axis(px(-20.), px(30.), px(32.), px(0.), px(100.), px(200.)),
            px(-20.)
        );
        assert_eq!(
            scroll_axis(px(-20.), px(-5.), px(-3.), px(0.), px(100.), px(200.)),
            px(-15.)
        );
        assert_eq!(
            scroll_axis(px(-20.), px(110.), px(112.), px(0.), px(100.), px(200.)),
            px(-32.)
        );
        assert_eq!(
            scroll_axis(px(-190.), px(110.), px(112.), px(0.), px(100.), px(200.)),
            px(-200.)
        );
        assert_eq!(
            scroll_axis(px(-5.), px(-10.), px(-8.), px(0.), px(100.), px(200.)),
            px(0.)
        );
    }
}
impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, Some("CerealInput")),
        KeyBinding::new("delete", Delete, Some("CerealInput")),
        KeyBinding::new("left", Left, Some("CerealInput")),
        KeyBinding::new("right", Right, Some("CerealInput")),
        KeyBinding::new("up", Up, Some("CerealInput")),
        KeyBinding::new("down", Down, Some("CerealInput")),
        KeyBinding::new("shift-left", SelectLeft, Some("CerealInput")),
        KeyBinding::new("shift-right", SelectRight, Some("CerealInput")),
        KeyBinding::new("shift-up", SelectUp, Some("CerealInput")),
        KeyBinding::new("shift-down", SelectDown, Some("CerealInput")),
        KeyBinding::new("cmd-a", SelectAll, Some("CerealInput")),
        KeyBinding::new("home", Home, Some("CerealInput")),
        KeyBinding::new("end", End, Some("CerealInput")),
        KeyBinding::new("cmd-left", Home, Some("CerealInput")),
        KeyBinding::new("cmd-right", End, Some("CerealInput")),
        KeyBinding::new("cmd-up", DocumentHome, Some("CerealInput")),
        KeyBinding::new("cmd-down", DocumentEnd, Some("CerealInput")),
        KeyBinding::new("shift-cmd-left", SelectHome, Some("CerealInput")),
        KeyBinding::new("shift-cmd-right", SelectEnd, Some("CerealInput")),
        KeyBinding::new("cmd-v", Paste, Some("CerealInput")),
        KeyBinding::new("cmd-c", Copy, Some("CerealInput")),
        KeyBinding::new("cmd-x", Cut, Some("CerealInput")),
        KeyBinding::new("enter", Enter, Some("CerealInput")),
        KeyBinding::new("shift-enter", LineBreak, Some("CerealInput")),
        KeyBinding::new("cmd-enter", Submit, Some("CerealInput")),
        KeyBinding::new("cmd-z", Undo, Some("CerealInput")),
        KeyBinding::new("shift-cmd-z", Redo, Some("CerealInput")),
        KeyBinding::new("tab", Indent, Some("CerealInput")),
        KeyBinding::new("shift-tab", Outdent, Some("CerealInput")),
        KeyBinding::new("ctrl-cmd-space", ShowCharacterPalette, Some("CerealInput")),
    ]);
}
