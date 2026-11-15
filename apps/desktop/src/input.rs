// Native input plumbing adapted from GPUI's Apache-2.0 licensed input example.
// Copyright Zed Industries and contributors. Multiline layout uses StyledText.
use gpui::{prelude::*, *};
use std::ops::Range;
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
        && let Some(item) = item_prefix
            .strip_prefix(BULLET_MARKER)
            .or_else(|| item_prefix.strip_prefix("- "))
    {
        let suffix = content[range.end..].split('\n').next().unwrap_or("");
        if item.trim().is_empty() && suffix.trim().is_empty() {
            return (line_start..range.end, String::new());
        }
        return (range, format!("\n{}{BULLET_MARKER}", &prefix[..indent]));
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
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    last_layout: Option<TextLayout>,
    scroll_handle: ScrollHandle,
    autoscroll_requested: bool,
    is_selecting: bool,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    focus_subscription: Option<Subscription>,
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
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            last_layout: None,
            scroll_handle: ScrollHandle::new(),
            autoscroll_requested: false,
            is_selecting: false,
            undo: vec![],
            redo: vec![],
            focus_subscription: None,
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
    pub fn set_content(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.content = text.into();
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
        self.changed(cx);
    }
    fn changed(&mut self, cx: &mut Context<Self>) {
        self.autoscroll_requested = true;
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
        if self.selected_range.is_empty() {
            self.select_to(self.previous_boundary(self.cursor_offset()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }
    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.next_boundary(self.cursor_offset()), cx);
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
        if let Some(snap) = self.undo.pop() {
            self.redo.push(self.snapshot());
            self.restore(snap, cx);
        }
    }
    fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(snap) = self.redo.pop() {
            self.undo.push(self.snapshot());
            self.restore(snap, cx);
        }
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
        // The first preedit already saved the state before composition. A final
        // commit must share that undo step, rather than restoring preedit text.
        if self.marked_range.is_none() {
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
        let range = range
            .as_ref()
            .map(|r| self.range_from_utf16(r))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        let new_text = normalized_input(new_text, self.multiline);
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
        if !placeholder {
            boundaries.extend([input.selected_range.start, input.selected_range.end]);
            if let Some(marked) = &input.marked_range {
                boundaries.extend([marked.start, marked.end]);
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
        let empty_selection = input.selected_range.is_empty();
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
            self.focus_subscription = Some(cx.on_focus(&focus, window, |_, _, cx| {
                cx.emit(InputFocused);
                cx.notify();
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
        byte_offset_from_utf16, formatted_list_edit, formatted_pasted_lists, normalized_input,
        scroll_axis,
    };
    use gpui::px;

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
        assert_eq!(enter("  • \nnext", "  • ".len()), "\nnext");
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
        KeyBinding::new("ctrl-cmd-space", ShowCharacterPalette, Some("CerealInput")),
    ]);
}
