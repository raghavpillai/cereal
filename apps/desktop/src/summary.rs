//! Source-summary presentation. Connected notes request actual provider context
//! with server-validated transcript quotations; offline previews select local
//! excerpts and never claim that their loading timer made a cloud request.
use crate::{
    app::{Cereal, NoteTab},
    input::{InputChanged, TextInput},
    model::Note,
    ui::*,
};
use gpui::{prelude::*, *};
use std::{
    cell::Cell,
    collections::HashSet,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TranscriptQuote {
    pub segment_id: Option<String>,
    pub text: String,
    pub speaker: Option<String>,
    pub timestamp: Option<String>,
}

#[derive(Clone)]
pub struct SummaryOpen {
    pub note_id: u64,
    pub row: usize,
    pub paragraph: String,
    pub explanation: String,
    pub quotes: Vec<TranscriptQuote>,
    pub error: Option<String>,
    pub transcript_revision: u64,
    pub ready: bool,
    pub trigger: Bounds<Pixels>,
    pub scroll_offset: Point<Pixels>,
}

#[derive(Default)]
pub struct SummaryState {
    pub open: Option<SummaryOpen>,
    /// The card that was just dismissed, kept only so it can be drawn while
    /// it leaves. Nothing reads it as state.
    closing: Option<SummaryOpen>,
    pub epoch: u64,
    cancel: Option<Arc<AtomicBool>>,
    pub edit: Option<EnhancedEdit>,
    pub transcript_selection: Option<crate::note::TranscriptSelection>,
}

pub struct EnhancedEdit {
    pub note_id: u64,
    pub row: usize,
    source: String,
    pub input: Entity<TextInput>,
    _subscriptions: Vec<Subscription>,
}

/// Replace only the editable text of one Markdown block. Indentation, list or
/// heading marker, untouched lines and the original trailing newline survive.
fn enhanced_edit_parts(source: &str, row: usize) -> Option<(String, String, String)> {
    let mut start = 0;
    let line = source.split_inclusive('\n').nth(row)?;
    for previous in source.split_inclusive('\n').take(row) {
        start += previous.len();
    }
    let line = line.strip_suffix('\n').unwrap_or(line);
    let parsed = note_line(line);
    let text = parsed.text();
    if text.is_empty() {
        return None;
    }
    let offset = text.as_ptr() as usize - line.as_ptr() as usize;
    Some((
        source[..start + offset].to_owned(),
        text.to_owned(),
        source[start + offset + text.len()..].to_owned(),
    ))
}

/// Other blocks retain their original layout while one block is edited. Map
/// their visible row to the current document before switching editors, even if
/// the active block gained or removed newlines. Compute this during rendering
/// so a blur before the click cannot discard the mapping.
fn enhanced_row_after_edit(row: usize, edited_row: usize, source: &str, current: &str) -> usize {
    if row <= edited_row {
        return row;
    }
    let original_breaks = source.bytes().filter(|b| *b == b'\n').count();
    let current_breaks = current.bytes().filter(|b| *b == b'\n').count();
    row.saturating_add(current_breaks)
        .saturating_sub(original_breaks)
}

impl Drop for SummaryState {
    fn drop(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            cancel.store(true, Ordering::Release);
        }
    }
}

impl SummaryState {
    pub fn dismiss(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            cancel.store(true, Ordering::Release);
        }
        self.epoch = self.epoch.wrapping_add(1);
        if let Some(open) = self.open.take() {
            self.closing = Some(open);
        }
        self.transcript_selection = None;
    }

    pub fn finish_loading(&mut self, epoch: u64, note_id: u64, row: usize) -> bool {
        if self.epoch != epoch {
            return false;
        }
        if let Some(open) = self.open.as_mut()
            && open.note_id == note_id
            && open.row == row
            && !open.ready
        {
            open.ready = true;
            return true;
        }
        false
    }

    pub fn finish_evidence(
        &mut self,
        epoch: u64,
        note_id: u64,
        row: usize,
        revision: u64,
        result: Result<(String, Vec<TranscriptQuote>), String>,
    ) -> bool {
        if self.epoch != epoch {
            return false;
        }
        let Some(open) = self.open.as_mut() else {
            return false;
        };
        if open.note_id != note_id
            || open.row != row
            || open.transcript_revision != revision
            || open.ready
        {
            return false;
        }
        match result {
            Ok((explanation, quotes)) => {
                open.explanation = explanation;
                open.quotes = quotes;
            }
            Err(error) => open.error = Some(error),
        }
        open.ready = true;
        self.cancel = None;
        true
    }
}

fn words(text: &str) -> HashSet<String> {
    const STOP: &[&str] = &[
        "that", "this", "with", "from", "they", "their", "have", "will", "were", "been", "about",
        "would", "there", "them", "then", "your", "into", "when", "what", "should", "could",
    ];
    text.split(|c: char| !c.is_alphanumeric())
        .map(str::to_lowercase)
        .filter(|word| word.chars().count() >= 4 && !STOP.contains(&word.as_str()))
        .collect()
}

fn timestamp(value: &str) -> bool {
    let parts: Vec<_> = value.split(':').collect();
    (2..=3).contains(&parts.len())
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum NoteLine<'a> {
    Blank,
    Heading(&'a str),
    Rule,
    List {
        text: &'a str,
        indent: usize,
        ordinal: Option<usize>,
    },
    Paragraph(&'a str),
}

impl<'a> NoteLine<'a> {
    fn text(&self) -> &'a str {
        match self {
            Self::Heading(text) | Self::Paragraph(text) | Self::List { text, .. } => text,
            Self::Blank | Self::Rule => "",
        }
    }
}

/// The wire format is Markdown. Keep nesting and numbering instead of flattening
/// every line before rendering; those structures are visible in Cereal's editor.
/// The text of an enhanced-notes line as its source card identifies it.
pub(crate) fn paragraph_text(line: &str) -> String {
    note_line(line).text().to_owned()
}

fn note_line(line: &str) -> NoteLine<'_> {
    let text = line.trim();
    if text.is_empty() {
        return NoteLine::Blank;
    }
    if matches!(text, "---" | "***" | "___") {
        return NoteLine::Rule;
    }
    let hashes = text.bytes().take_while(|&c| c == b'#').count();
    if (1..=6).contains(&hashes) && text.as_bytes().get(hashes) == Some(&b' ') {
        return NoteLine::Heading(text[hashes..].trim());
    }
    let indent = line
        .chars()
        .take_while(|c| c.is_whitespace())
        .map(|c| if c == '\t' { 4 } else { 1 })
        .sum();
    if let Some(text) = ["- ", "* ", "+ ", "• "]
        .iter()
        .find_map(|prefix| text.strip_prefix(prefix))
    {
        return NoteLine::List {
            text: text.trim(),
            indent,
            ordinal: None,
        };
    }
    if let Some((marker, rest)) = text.split_once(' ')
        && marker.len() >= 2
        && matches!(marker.as_bytes().last(), Some(b'.' | b')'))
        && let Ok(ordinal) = marker[..marker.len() - 1].parse::<usize>()
    {
        return NoteLine::List {
            text: rest.trim(),
            indent,
            ordinal: Some(ordinal),
        };
    }
    NoteLine::Paragraph(text)
}

fn list_indent_unit(text: &str) -> usize {
    text.lines()
        .filter_map(|line| match note_line(line) {
            NoteLine::List { indent, .. } if indent > 0 => Some(indent.min(4)),
            _ => None,
        })
        .min()
        .unwrap_or(2)
        .max(1)
}

fn skeleton_widths(note_id: u64, row: usize, epoch: u64) -> [f32; 3] {
    // Cereal chooses 75–100% widths once when the source card mounts. A stable
    // per-opening seed avoids widths jumping on every native animation frame.
    let mut seed = note_id ^ (row as u64).rotate_left(17) ^ epoch.wrapping_mul(0x9e3779b97f4a7c15);
    std::array::from_fn(|_| {
        seed ^= seed.wrapping_shl(13);
        seed ^= seed.wrapping_shr(7);
        seed ^= seed.wrapping_shl(17);
        0.75 + (seed % 2500) as f32 / 10000.
    })
}

#[cfg(test)]
fn emphasis_ranges(text: &str) -> (String, Vec<(std::ops::Range<usize>, HighlightStyle)>) {
    let mut rendered = String::new();
    let mut highlights = Vec::new();
    let mut cursor = 0;
    while cursor < text.len() {
        let rest = &text[cursor..];
        let delimiter = if rest.starts_with("**") { "**" } else { "*" };
        if rest.starts_with('*')
            && let Some(end) = rest[delimiter.len()..].find(delimiter)
            && end > 0
        {
            let start = rendered.len();
            rendered.push_str(&rest[delimiter.len()..delimiter.len() + end]);
            let style = if delimiter.len() == 2 {
                HighlightStyle {
                    font_weight: Some(FontWeight::BOLD),
                    ..Default::default()
                }
            } else {
                HighlightStyle {
                    font_style: Some(FontStyle::Italic),
                    ..Default::default()
                }
            };
            highlights.push((start..rendered.len(), style));
            cursor += delimiter.len() * 2 + end;
        } else {
            let character = rest.chars().next().unwrap();
            rendered.push(character);
            cursor += character.len_utf8();
        }
    }
    (rendered, highlights)
}

/// Preserve source order among relevant blocks. A block with no lexical match
/// is never presented as supporting evidence for an unrelated summary line.
pub fn matching_quotes(paragraph: &str, transcript: &str) -> Vec<TranscriptQuote> {
    let terms = words(paragraph);
    let mut candidates = Vec::new();
    for (index, block) in transcript.split("\n\n").enumerate() {
        let mut lines = block.trim().lines();
        let first = lines.next().unwrap_or_default();
        let remaining = lines.collect::<Vec<_>>().join("\n");
        let mut header = first.split_whitespace().collect::<Vec<_>>();
        let (speaker, time, text) =
            if !remaining.is_empty() && header.last().is_some_and(|part| timestamp(part)) {
                let time = header.pop().unwrap().to_string();
                let speaker = (!header.is_empty()).then(|| header.join(" "));
                (speaker, Some(time), remaining)
            } else {
                (None, None, block.trim().to_string())
            };
        let score = words(&text).intersection(&terms).count();
        if score > 0 && !text.is_empty() {
            candidates.push((
                score,
                index,
                TranscriptQuote {
                    segment_id: None,
                    text,
                    speaker,
                    timestamp: time,
                },
            ));
        }
    }
    candidates.sort_by_key(|(score, index, _)| (std::cmp::Reverse(*score), *index));
    candidates.truncate(3);
    candidates.sort_by_key(|(_, index, _)| *index);
    candidates.into_iter().map(|(_, _, quote)| quote).collect()
}

impl Cereal {
    pub fn finish_enhanced_edit(&mut self, cx: &mut Context<Self>) {
        let Some(edit) = self.summary.edit.take() else {
            return;
        };
        if crate::server::enabled()
            && self
                .workspace
                .note(edit.note_id)
                .is_some_and(|note| note.enhanced_dirty)
        {
            self.server_action(edit.note_id, crate::server::Action::Save, cx);
        }
        cx.notify();
    }

    pub fn begin_enhanced_edit(
        &mut self,
        note_id: u64,
        row: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.generation.is_running_for(note_id) {
            return;
        }
        if self
            .summary
            .edit
            .as_ref()
            .is_some_and(|edit| edit.note_id == note_id && edit.row == row)
        {
            return;
        }
        self.finish_enhanced_edit(cx);
        self.summary.dismiss();
        let Some(source) = self
            .workspace
            .note(note_id)
            .map(|note| note.enhanced.clone())
        else {
            return;
        };
        let Some((prefix, text, suffix)) = enhanced_edit_parts(&source, row) else {
            return;
        };
        let input = cx.new(|cx| TextInput::new("", true, cx));
        input.update(cx, |input, cx| input.set_content(text, cx));
        let changed = cx.subscribe(&input, move |this, _, event: &InputChanged, cx| {
            if !this
                .summary
                .edit
                .as_ref()
                .is_some_and(|edit| edit.note_id == note_id && edit.row == row)
            {
                return;
            }
            if let Some(note) = this.workspace.note_mut(note_id) {
                let text = format!("{prefix}{}{suffix}", event.content);
                if note.enhanced != text {
                    note.enhanced = text;
                    note.enhanced_dirty = true;
                    this.persist(cx);
                }
            }
        });
        let focus = input.read(cx).focus_handle.clone();
        let blurred = cx.on_blur(&focus, window, move |this, _, cx| {
            if this
                .summary
                .edit
                .as_ref()
                .is_some_and(|edit| edit.note_id == note_id && edit.row == row)
            {
                this.finish_enhanced_edit(cx);
            }
        });
        self.summary.edit = Some(EnhancedEdit {
            note_id,
            row,
            source,
            input,
            _subscriptions: vec![changed, blurred],
        });
        focus.focus(window);
        cx.notify();
    }

    pub fn open_summary(
        &mut self,
        note_id: u64,
        row: usize,
        paragraph: String,
        trigger: Bounds<Pixels>,
        cx: &mut Context<Self>,
    ) {
        if self
            .summary
            .open
            .as_ref()
            .is_some_and(|open| open.note_id == note_id && open.row == row)
        {
            self.summary.dismiss();
            cx.notify();
            return;
        }
        // One card replacing another restarts from part-way, like a menu
        // swapped in place; the outgoing card is not drawn again.
        let replacing = self.summary.open.is_some();
        self.summary.dismiss();
        self.summary.closing = None;
        self.stage.kick(
            ("summary-card", 0),
            if replacing { 0.6 } else { 0. },
            crate::motion::GOOEY,
        );
        let note = self.workspace.note(note_id).cloned();
        let remote = crate::server::enabled();
        let revision = note
            .as_ref()
            .map(|note| note.transcript_revision)
            .unwrap_or(0);
        let quotes = if remote {
            Vec::new()
        } else {
            note.as_ref()
                .map(|note| matching_quotes(&paragraph, &note.transcript))
                .unwrap_or_default()
        };
        self.summary.open = Some(SummaryOpen {
            note_id,
            row,
            explanation: if remote {
                String::new()
            } else {
                paragraph.clone()
            },
            paragraph: paragraph.clone(),
            quotes,
            error: None,
            transcript_revision: revision,
            ready: false,
            trigger,
            scroll_offset: self.note_scroll.offset(),
        });
        let epoch = self.summary.epoch;
        cx.notify();
        if remote {
            let cancel = Arc::new(AtomicBool::new(false));
            self.summary.cancel = Some(cancel.clone());
            let (sender, receiver) = mpsc::channel();
            std::thread::spawn(move || {
                let result = note
                    .ok_or_else(|| "This note is no longer available.".to_string())
                    .and_then(|note| crate::server::load_evidence(&note, &paragraph, cancel))
                    .and_then(|evidence| {
                        if evidence.transcript_revision != revision as i64 {
                            return Err("The transcript changed. Open the source again.".into());
                        }
                        let quotes = evidence
                            .quotes
                            .into_iter()
                            .map(|quote| TranscriptQuote {
                                segment_id: Some(quote.segment_id),
                                text: quote.text,
                                speaker: quote.speaker_label,
                                timestamp: Some(format!(
                                    "{:02}:{:02}",
                                    quote.start_ms.max(0) / 60000,
                                    quote.start_ms.max(0) / 1000 % 60
                                )),
                            })
                            .collect();
                        Ok((evidence.summary, quotes))
                    });
                let _ = sender.send(result);
            });
            cx.spawn(async move |this, cx| {
                let result = loop {
                    match receiver.try_recv() {
                        Ok(result) => break result,
                        Err(mpsc::TryRecvError::Disconnected) => {
                            break Err("Could not load transcript context. Try again.".into());
                        }
                        Err(mpsc::TryRecvError::Empty) => {
                            cx.background_executor()
                                .timer(Duration::from_millis(50))
                                .await;
                            let current = this
                                .update(cx, |this, _| this.summary.epoch == epoch)
                                .unwrap_or(false);
                            if !current {
                                return;
                            }
                        }
                    }
                };
                let _ = this.update(cx, |this, cx| {
                    if this.summary.epoch != epoch {
                        return;
                    }
                    if this.current_note == Some(note_id) && this.active_tab == NoteTab::Enhanced {
                        if this
                            .workspace
                            .note(note_id)
                            .is_some_and(|note| note.transcript_revision != revision)
                        {
                            this.summary.dismiss();
                        } else if this
                            .summary
                            .finish_evidence(epoch, note_id, row, revision, result)
                        {
                            cx.notify();
                        }
                    }
                });
            })
            .detach();
            return;
        }
        // Local preview only: a fixed delay stands in for the wait a
        // connected instance has for its streamed response.
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(850))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.current_note == Some(note_id)
                    && this.active_tab == NoteTab::Enhanced
                    && this.summary.finish_loading(epoch, note_id, row)
                {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub fn render_enhanced_content(
        &self,
        note: &Note,
        t: Theme,
        width: Pixels,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        self.render_note_markdown(note, false, t, width, window, cx)
    }

    /// The generated notes of `note`, laid out block by block. `read_only`
    /// is for a note that is not the user's to change (a shared snapshot):
    /// the same blocks, with nothing that edits them.
    pub fn render_note_markdown(
        &self,
        note: &Note,
        read_only: bool,
        t: Theme,
        width: Pixels,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let dark = t.bg.l < 0.5;
        let highlight: Hsla = if dark {
            rgb(0x363635).into()
        } else {
            rgb(0xf0ece4).into()
        };
        let result = div()
            .w(width)
            .min_w_0()
            .whitespace_normal()
            .flex()
            .flex_col()
            .text_size(px(16.))
            .line_height(px(24.))
            .text_color(if dark { rgb(0xacada8).into() } else { t.muted });
        let note_id = note.id;
        let edit = self
            .summary
            .edit
            .as_ref()
            .filter(|edit| edit.note_id == note.id);
        let source = edit
            .map(|edit| edit.source.as_str())
            .unwrap_or(&note.enhanced);
        let indent_unit = list_indent_unit(source);
        // After a generation the user watched, the notes arrive block by
        // block (the note view sets the mark once; it expires by itself and
        // every block is back to being a plain child).
        let arrive = |block: AnyElement| {
            let pose = self.stage.cascade_next("enhanced");
            if pose.at_rest() {
                block
            } else {
                crate::motion::lift(div().w_full().child(block), pose).into_any_element()
            }
        };
        // First what each line is: its kind, its place in a list, its number.
        // That is cheap and needs every line. Elements are then built only
        // for the blocks near the viewport (see `lazy`), so a summary of
        // 1,500 bullets opens and scrolls like one of fifteen.
        struct Block<'a> {
            row: usize,
            line: &'a str,
            parsed: NoteLine<'a>,
            list: Option<(usize, Option<usize>)>,
        }
        let has_transcript = !note.transcript.trim().is_empty();
        let eligible = |block: &Block| {
            let text = block.parsed.text();
            edit.is_none_or(|edit| edit.row != block.row)
                && has_transcript
                && text.chars().any(char::is_alphanumeric)
                && text.chars().count() > 1
        };
        let mut blocks: Vec<Block> = Vec::new();
        let mut ordinals: [Option<usize>; 9] = [None; 9];
        let mut after_heading = false;
        for (row, line) in source.lines().enumerate() {
            let parsed = note_line(line);
            if parsed == NoteLine::Blank {
                // The blank line Markdown puts after a heading is not a gap of
                // its own: the first block starts one line step below.
                if !after_heading {
                    blocks.push(Block { row, line, parsed, list: None });
                }
                continue;
            }
            after_heading = matches!(parsed, NoteLine::Heading(_));
            let list = if let NoteLine::List {
                indent, ordinal, ..
            } = parsed
            {
                let depth = (indent / indent_unit).min(8);
                let number = ordinal.map(|number| {
                    let next = ordinals[depth]
                        .map(|value| value.saturating_add(1))
                        .unwrap_or(number);
                    ordinals[depth] = Some(next);
                    next
                });
                if ordinal.is_none() {
                    ordinals[depth] = None;
                }
                for old in ordinals.iter_mut().skip(depth + 1) {
                    *old = None;
                }
                Some((depth, number))
            } else {
                ordinals.fill(None);
                None
            };
            blocks.push(Block { row, line, parsed, list });
        }
        // Leading whitespace of a continuation line, in list levels.
        let levels = |line: &str| {
            let indent = line
                .chars()
                .take_while(|c| c.is_whitespace())
                .map(|c| if c == '\t' { 4 } else { 1 })
                .sum::<usize>();
            (indent, (indent / indent_unit).min(8))
        };
        // The row being edited is as tall as what has been typed into it.
        let typed = edit.map(|edit| crate::lazy::key(edit.input.read(cx).content()));
        let ids: Vec<u64> = blocks.iter().map(|block| block.row as u64).collect();
        let keys: Vec<u64> = blocks
            .iter()
            .map(|block| {
                let editing = edit.is_some_and(|edit| edit.row == block.row).then_some(typed);
                match &block.parsed {
                    NoteLine::Blank => crate::lazy::key("space"),
                    NoteLine::Rule => crate::lazy::key("rule"),
                    NoteLine::Heading(title) => {
                        crate::lazy::key(("heading", title, block.row == 0, editing))
                    }
                    parsed => crate::lazy::key((
                        "text",
                        parsed.text(),
                        block.list.map(|(depth, _)| depth),
                        levels(block.line),
                        eligible(block),
                        editing,
                    )),
                }
            })
            .collect();
        let mut counter = LineCounter::new(16., cx);
        let full = f32::from(width);
        let estimate = |index: usize| {
            let block = &blocks[index];
            match &block.parsed {
                NoteLine::Blank => 8.,
                NoteLine::Rule => 41.,
                NoteLine::Heading(title) => {
                    counter.lines(title, full) as f32 * 24. + if block.row == 0 { 0. } else { 8. }
                }
                parsed => {
                    let source_trigger = if eligible(block) { 28. } else { 0. };
                    let (inset, lift) = match block.list {
                        Some((depth, _)) => (16. * (depth + 1) as f32, 0.),
                        None => {
                            let (indent, level) = levels(block.line);
                            (level as f32 * 16., if indent > 0 { -4. } else { 0. })
                        }
                    };
                    let text_width = full - inset - 2. - source_trigger;
                    counter.lines(parsed.text(), text_width) as f32 * 24. + lift
                }
            }
        };
        let build = |index: usize| -> AnyElement {
            let block = &blocks[index];
            let (row, line) = (block.row, block.line);
            let parsed = block.parsed.clone();
            let list = block.list;
            let editing = edit.filter(|edit| edit.row == row);
            let editable_row = edit
                .map(|edit| enhanced_row_after_edit(row, edit.row, &edit.source, &note.enhanced))
                .unwrap_or(row);
            if parsed == NoteLine::Blank {
                return div().h(px(8.)).into_any_element();
            }
            if parsed == NoteLine::Rule {
                return arrive(
                    div()
                        .w_full()
                        .h(px(1.))
                        .my(px(20.))
                        .bg(t.border)
                        .into_any_element(),
                );
            }
            if let NoteLine::Heading(title) = parsed {
                return arrive(
                    div()
                        .id(("enhanced-heading", row))
                        .relative()
                        .mt(px(if row == 0 { 0. } else { 8. }))
                        .font_family(FONT)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_size(px(16.))
                        .line_height(px(24.))
                        .child(
                            div()
                                .absolute()
                                .left(px(-16.))
                                .font_weight(FontWeight::NORMAL)
                                .text_color(t.muted)
                                .opacity(0.6)
                                .child("#"),
                        )
                        .child(if let Some(edit) = editing {
                            edit.input.clone().into_any_element()
                        } else {
                            crate::chat::inline_text(title, t).into_any_element()
                        })
                        .when(!read_only, |heading| {
                            heading.on_click(cx.listener(move |this, _, window, cx| {
                                this.begin_enhanced_edit(note_id, editable_row, window, cx)
                            }))
                        })
                        .into_any_element(),
                );
            }
            let text = parsed.text().to_string();
            let eligible = eligible(block);
            let selected = self
                .summary
                .open
                .as_ref()
                .is_some_and(|open| open.note_id == note.id && open.row == row);
            let group: SharedString = format!("source-paragraph-{}-{row}", note.id).into();
            let paragraph_hover = crate::motion::reveal_key(&group);
            let mut paragraph = div()
                .id(group)
                .relative()
                .flex()
                .items_start()
                .gap(px(4.))
                .flex_1()
                .min_w_0()
                .ml(px(-2.))
                .mr(px(2.))
                .pl(px(2.))
                .rounded(px(4.));
            if eligible {
                paragraph = paragraph
                    .pr(px(0.))
                    .child(
                        crate::motion::tactile(highlight)
                            .radius(4.)
                            .reveals(paragraph_hover),
                    )
                    .when(selected, |div| div.bg(highlight));
            }
            paragraph = paragraph.child(
                div()
                    .id(("enhanced-text", row))
                    .flex_1()
                    .min_w_0()
                    .child(if let Some(edit) = editing {
                        edit.input.clone().into_any_element()
                    } else {
                        crate::chat::inline_text(&text, t).into_any_element()
                    })
                    .when(!read_only, |text| {
                        text.cursor(CursorStyle::IBeam).on_click(cx.listener({
                            let note_id = note.id;
                            move |this, _, window, cx| {
                                this.begin_enhanced_edit(note_id, editable_row, window, cx)
                            }
                        }))
                    }),
            );
            if eligible {
                let bounds: Rc<Cell<Option<Bounds<Pixels>>>> = Rc::new(Cell::new(None));
                let for_canvas = bounds.clone();
                let for_key = bounds.clone();
                let key_text = text.clone();
                let note_id = note.id;
                paragraph = paragraph.child(
                    div()
                        .id(SharedString::from(format!(
                            "source-trigger-{note_id}-{row}"
                        )))
                        .relative()
                        .size(px(24.))
                        .flex_shrink_0()
                        .rounded(px(4.))
                        .focusable()
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .opacity(if selected {
                            1.
                        } else {
                            crate::motion::revealed(paragraph_hover)
                        })
                        .reach()
                        .focus(move |style| style.opacity(1.).bg(t.hover))
                        .child(crate::motion::tactile(t.hover).radius(4.))
                        .child(icon("source-zoom-in", 16., t.muted))
                        .child(
                            canvas(
                                move |bounds, _, _| for_canvas.set(Some(bounds)),
                                |_, _, _, _| {},
                            )
                            .absolute()
                            .size_full(),
                        )
                        .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                let trigger = for_key.get().unwrap_or_else(|| {
                                    Bounds::new(window.mouse_position(), size(px(24.), px(24.)))
                                });
                                this.open_summary(note_id, row, key_text.clone(), trigger, cx);
                                cx.stop_propagation();
                            }
                        }))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.focus.focus(window);
                            let trigger = bounds.get().unwrap_or_else(|| {
                                Bounds::new(window.mouse_position(), size(px(24.), px(24.)))
                            });
                            this.open_summary(note_id, row, text.clone(), trigger, cx);
                        })),
                );
            }
            arrive(if let Some((depth, number)) = list {
                let inset = 16. * (depth + 1) as f32;
                let marker: Hsla = rgb(0x71717a).into();
                div()
                    .relative()
                    .w_full()
                    .flex()
                    .items_start()
                    .pl(px(inset))
                    .child(if let Some(number) = number {
                        div()
                            .absolute()
                            .left(px(inset - 26.))
                            .w(px(20.))
                            .text_right()
                            .text_color(marker)
                            .child(format!("{number}."))
                    } else {
                        div()
                            .absolute()
                            .left(px(inset - 15.))
                            .top(px(9.5))
                            .size(px(5.))
                            .when(depth != 1, |d| d.bg(marker))
                            .when(depth < 2, |d| d.rounded_full())
                            .when(depth == 1, |d| d.border_1().border_color(marker))
                    })
                    .child(paragraph)
                    .into_any_element()
            } else {
                // Markdown continuation lines stay aligned with their parent
                // bullet text rather than jumping back to the heading gutter.
                let (indent, level) = levels(line);
                paragraph
                    .ml(px(-2. + level as f32 * 16.))
                    .when(indent > 0, |d| d.mt(px(-4.)))
                    .into_any_element()
            })
        };
        // The same 4 px between blocks as before, now between the column's
        // rows. The note's own scroll container is the one the column windows
        // against.
        let result = result.child(crate::lazy::estimated(
            "enhanced",
            &ids,
            &keys,
            full,
            4.,
            &self.note_scroll,
            window,
            estimate,
            build,
        ));
        result
    }

    pub fn render_summary_popover(
        &mut self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let matches_context = |open: &SummaryOpen| {
            self.current_note == Some(open.note_id)
                && self.active_tab == NoteTab::Enhanced
                && self
                    .workspace
                    .note(open.note_id)
                    .and_then(|note| note.enhanced.lines().nth(open.row))
                    .is_some_and(|line| note_line(line).text() == open.paragraph)
        };
        let open_stale = self
            .summary
            .open
            .as_ref()
            .is_some_and(|open| !matches_context(open));
        let closing_stale = self
            .summary
            .closing
            .as_ref()
            .is_some_and(|open| !matches_context(open));
        if open_stale {
            self.summary.dismiss();
        }
        // A card whose paragraph has gone leaves with it, without an exit.
        if open_stale || closing_stale {
            self.summary.closing = None;
        }
        let live = self.summary.open.is_some();
        let pose = self
            .stage
            .presence(("summary-card", 0), live, crate::motion::GOOEY);
        if !pose.visible {
            self.summary.closing = None;
            return None;
        }
        let open = self
            .summary
            .open
            .as_ref()
            .or(self.summary.closing.as_ref())?;
        let t = self.theme();
        let dark = t.bg.l < 0.5;
        let card: Hsla = if dark {
            rgb(0x363635).into()
        } else {
            rgb(0xf7f8f3).into()
        };
        let border: Hsla = if dark {
            rgba(0xffffff26).into()
        } else {
            rgb(0xdedfd7).into()
        };
        let foreground: Hsla = if dark { rgb(0xfcfcf8).into() } else { t.text };
        let muted: Hsla = if dark { rgb(0xacada8).into() } else { t.muted };
        let skeleton: Hsla = if dark {
            rgb(0x3c3c3b).into()
        } else {
            rgb(0xe9ebe3).into()
        };
        let rule: Hsla = if dark {
            rgb(0x645539).into()
        } else {
            rgb(0xd6c49e).into()
        };
        let width = px(if window.viewport_size().width >= px(1024.) {
            512.
        } else if window.viewport_size().width >= px(640.) {
            384.
        } else {
            320.
        })
        .min(window.viewport_size().width - px(24.));
        let offset = self.note_scroll.offset() - open.scroll_offset;
        let trigger = Bounds::new(open.trigger.origin + offset, open.trigger.size);
        let anchor = trigger.bottom_right() + point(px(0.), px(5.));
        if trigger.bottom() < px(40.) || trigger.top() > window.viewport_size().height {
            return None;
        }
        let mut body = div()
            .w_full()
            .whitespace_normal()
            .flex()
            .flex_col()
            .gap(px(12.))
            .p(px(16.))
            .text_size(px(14.))
            .line_height(px(22.))
            .text_color(foreground)
            .child(
                div()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(muted)
                    .child("TRANSCRIPT SUMMARY"),
            );
        if !open.ready {
            let mut lines = div().w_full().flex().flex_col().gap(px(8.));
            for (index, portion) in skeleton_widths(open.note_id, open.row, self.summary.epoch)
                .into_iter()
                .enumerate()
            {
                // Each line breathes on a two-second turn, starting 200 ms
                // after the one above. Slow, so stepped at 30 Hz.
                let delay = index as f32 * 0.1;
                lines = lines.child(crate::motion::slow_loop(
                    SharedString::from(format!("source-skeleton-{}-{index}", self.summary.epoch)),
                    Duration::from_secs(2),
                    div()
                        .h(px(16.))
                        .w((width - px(32.)) * portion)
                        .rounded(px(4.))
                        .bg(skeleton),
                    move |div, turns| {
                        let turn = turns - delay;
                        if turn < 0. {
                            return div.opacity(1.);
                        }
                        div.opacity(0.75 + 0.25 * (turn * std::f32::consts::TAU).cos())
                    },
                ));
            }
            body = body.child(lines);
        } else if let Some(error) = &open.error {
            let note_id = open.note_id;
            let row = open.row;
            let paragraph = open.paragraph.clone();
            let trigger = open.trigger;
            body = body
                .child(
                    div()
                        .text_size(px(14.))
                        .line_height(px(20.))
                        .text_color(muted)
                        .child(error.clone()),
                )
                .child(
                    div()
                        .id("retry-source-summary")
                        .px(px(10.))
                        .py(px(6.))
                        .rounded(px(6.))
                        .text_size(px(13.))
                        .cursor_pointer()
                        .child(crate::motion::tactile(skeleton).radius(6.))
                        .child("Try again")
                        .reach().on_click(cx.listener(move |this, _, _, cx| {
                            this.summary.dismiss();
                            this.open_summary(note_id, row, paragraph.clone(), trigger, cx);
                        })),
                );
        } else {
            body = body.child(crate::chat::inline_text(
                if open.explanation.trim().is_empty() {
                    "No relevant context found"
                } else {
                    &open.explanation
                },
                t,
            ));
            for (index, quote) in open.quotes.iter().enumerate() {
                let quote_text = quote.text.clone();
                let segment_id = quote.segment_id.clone();
                let note_id = open.note_id;
                let revision = open.transcript_revision;
                let attribution = [quote.speaker.clone(), quote.timestamp.clone()]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(" • ");
                body = body.child(
                    div()
                        .id(SharedString::from(format!("source-quote-{index}")))
                        .flex()
                        .gap(px(12.))
                        .py(px(4.))
                        .rounded(px(6.))
                        .bg(skeleton)
                        .cursor_pointer()
                        // The row jumps to its place in the transcript: it
                        // answers the pointer like any other row.
                        .child(crate::motion::tactile(t.hover).radius(6.))
                        .child(div().w(px(4.)).flex_shrink_0().rounded_full().bg(rule))
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .flex()
                                .flex_col()
                                .text_size(px(14.))
                                .line_height(px(20.))
                                .child(quote.text.clone())
                                .when(!attribution.is_empty(), |container| {
                                    container.child(
                                        div()
                                            .mt(px(4.))
                                            .text_size(px(12.))
                                            .line_height(px(16.))
                                            .text_color(muted)
                                            .child(attribution),
                                    )
                                }),
                        )
                        .reach().on_click(cx.listener(move |this, _, window, cx| {
                            this.jump_to_transcript_quote(
                                note_id,
                                segment_id.as_deref(),
                                &quote_text,
                                Some(revision),
                                window,
                                cx,
                            );
                        })),
                );
            }
        }
        if open.ready && self.recording.note_id == Some(open.note_id) && self.recording.is_active()
        {
            body = body.child(div().pt(px(8.)).border_t_1().border_color(border).text_size(px(12.)).line_height(px(16.)).text_color(muted).child("Transcription is still active. These notes were generated at a previous point when transcription was paused."));
        }
        let card = div()
            .id("transcript-summary-popover")
            .w(width)
            .min_h(px(192.))
            .max_h(px(448.).min(window.viewport_size().height - px(32.)))
            .overflow_y_scroll()
            .overflow_x_hidden()
            .rounded(px(12.))
            .bg(card)
            .border_1()
            .border_color(border)
            .shadow_xl()
            .child(body)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .when(live, |card| {
                card.on_mouse_down_out(cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    if !trigger.contains(&event.position) {
                        this.summary.dismiss();
                        cx.notify();
                    }
                }))
            });
        Some(
            deferred(
                anchored()
                    .anchor(Corner::TopRight)
                    .position(anchor)
                    .snap_to_window_with_margin(px(12.))
                    // The source card grows out of the paragraph's trigger at
                    // its top-right corner, and folds back into it.
                    .child(crate::motion::pop(
                        card,
                        crate::motion::Pop::menu((1., 0.)),
                        pose,
                    )),
            )
            .with_priority(10)
            .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{
        NoteLine, SummaryOpen, SummaryState, emphasis_ranges, list_indent_unit, matching_quotes,
        note_line,
    };
    use gpui::{Bounds, FontStyle, FontWeight, Point};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    #[test]
    fn switching_paragraphs_after_multiline_edits_targets_the_visible_block() {
        let source = "# T\n- Alpha\n- Beta\n- Gamma";
        let (prefix, _, suffix) = super::enhanced_edit_parts(source, 1).unwrap();
        let updated = format!("{prefix}Alpha\ncontinued\nthird line{suffix}");
        let beta_row = super::enhanced_row_after_edit(2, 1, source, &updated);
        assert_eq!(beta_row, 4);
        let (before, beta, after) = super::enhanced_edit_parts(&updated, beta_row).unwrap();
        assert_eq!(beta, "Beta");
        assert_eq!(
            format!("{before}Updated Beta{after}"),
            "# T\n- Alpha\ncontinued\nthird line\n- Updated Beta\n- Gamma"
        );
        assert_eq!(super::enhanced_row_after_edit(0, 1, source, &updated), 0);
        let collapsed = "# T\n- Alpha\n- Beta\n- Gamma";
        assert_eq!(super::enhanced_row_after_edit(4, 1, &updated, collapsed), 2);
    }

    #[test]
    fn paragraph_edit_preserves_markdown_structure_unicode_and_untouched_content() {
        let source =
            "# Project Cedar\n\n- Ship Friday\n  - Alex owns **mobile** 🦊\n\nLast paragraph.\n";
        let (prefix, text, suffix) = super::enhanced_edit_parts(source, 3).unwrap();
        assert_eq!(text, "Alex owns **mobile** 🦊");
        let updated = format!("{prefix}Zoë owns **mobile**\nand final testing 🦊{suffix}");
        assert_eq!(
            updated,
            "# Project Cedar\n\n- Ship Friday\n  - Zoë owns **mobile**\nand final testing 🦊\n\nLast paragraph.\n"
        );
        let (prefix, text, suffix) = super::enhanced_edit_parts("# #\n- -\n", 0).unwrap();
        assert_eq!(
            (prefix, text, suffix),
            ("# ".into(), "#".into(), "\n- -\n".into())
        );
        assert!(super::enhanced_edit_parts(source, 1).is_none());
        assert!(super::enhanced_edit_parts(source, 999).is_none());
    }

    #[test]
    fn emphasis_ranges_keep_unicode_offsets_and_unmatched_markers() {
        let (text, highlights) =
            emphasis_ranges("Zoë captures *how* 🌱 **people** think; *unfinished");
        assert_eq!(text, "Zoë captures how 🌱 people think; *unfinished");
        assert_eq!(&text[highlights[0].0.clone()], "how");
        assert_eq!(highlights[0].1.font_style, Some(FontStyle::Italic));
        assert_eq!(&text[highlights[1].0.clone()], "people");
        assert_eq!(highlights[1].1.font_weight, Some(FontWeight::BOLD));
    }

    #[test]
    fn quotes_preserve_exact_source_and_only_claim_real_attribution() {
        let transcript = "Zoë 李  00:18\nThe onboarding prototype is ready.\n\nUnrelated remark without a speaker.";
        let quotes = matching_quotes("Share the onboarding prototype", transcript);
        assert_eq!(quotes.len(), 1);
        assert_eq!(quotes[0].text, "The onboarding prototype is ready.");
        assert_eq!(quotes[0].speaker.as_deref(), Some("Zoë 李"));
        assert_eq!(quotes[0].timestamp.as_deref(), Some("00:18"));
        let plain = matching_quotes("Unrelated remark", transcript);
        assert_eq!(plain[0].speaker, None);
        assert_eq!(plain[0].timestamp, None);
    }

    #[test]
    fn unrelated_or_empty_transcripts_do_not_invent_evidence() {
        assert!(
            matching_quotes("Review the budget", "Maya  01:20\nThe calendar is ready.").is_empty()
        );
        assert!(matching_quotes("Review the budget", "").is_empty());
        assert!(matching_quotes("the and with", "Everything is ready.").is_empty());
    }

    #[test]
    fn markdown_note_rows_preserve_hierarchy_numbering_and_source_text() {
        assert_eq!(note_line("## Decisions"), NoteLine::Heading("Decisions"));
        assert_eq!(
            note_line("#cedar release"),
            NoteLine::Paragraph("#cedar release")
        );
        assert_eq!(
            note_line("  - **Zoë:** ship Monday"),
            NoteLine::List {
                text: "**Zoë:** ship Monday",
                indent: 2,
                ordinal: None,
            }
        );
        assert_eq!(
            note_line("    3. Review the launch"),
            NoteLine::List {
                text: "Review the launch",
                indent: 4,
                ordinal: Some(3),
            }
        );
        assert_eq!(list_indent_unit("- Owner\n  - Action\n    - Detail"), 2);
        assert_eq!(
            list_indent_unit("- Owner\n    - Action\n        - Detail"),
            4
        );
        assert_eq!(note_line("---"), NoteLine::Rule);
    }

    #[test]
    fn real_evidence_completion_is_scoped_to_note_row_revision_and_request_epoch() {
        let mut state = SummaryState::default();
        state.open = Some(SummaryOpen {
            note_id: 4,
            row: 2,
            paragraph: "Ship Monday".into(),
            explanation: String::new(),
            quotes: vec![],
            error: None,
            transcript_revision: 7,
            ready: false,
            trigger: Bounds::default(),
            scroll_offset: Point::default(),
        });
        let result = || Ok(("The launch changed after review.".into(), vec![]));
        assert!(!state.finish_evidence(state.epoch, 5, 2, 7, result()));
        assert!(!state.finish_evidence(state.epoch, 4, 3, 7, result()));
        assert!(!state.finish_evidence(state.epoch, 4, 2, 8, result()));
        assert!(!state.finish_evidence(state.epoch.wrapping_add(1), 4, 2, 7, result()));
        assert!(!state.open.as_ref().unwrap().ready);
        assert!(state.finish_evidence(state.epoch, 4, 2, 7, result()));
        assert_eq!(
            state.open.as_ref().unwrap().explanation,
            "The launch changed after review."
        );
        assert!(state.open.as_ref().unwrap().ready);
        // A duplicate/late completion cannot replace already-visible content.
        assert!(!state.finish_evidence(state.epoch, 4, 2, 7, Err("late error".into())));
        state.dismiss();
        assert!(!state.finish_evidence(state.epoch, 4, 2, 7, result()));
    }

    #[test]
    fn a_source_service_error_finishes_loading_without_inventing_quotes() {
        let mut state = SummaryState::default();
        state.open = Some(SummaryOpen {
            note_id: 1,
            row: 0,
            paragraph: "Decision".into(),
            explanation: String::new(),
            quotes: vec![],
            error: None,
            transcript_revision: 1,
            ready: false,
            trigger: Bounds::default(),
            scroll_offset: Point::default(),
        });
        assert!(state.finish_evidence(state.epoch, 1, 0, 1, Err("Try again".into())));
        let open = state.open.as_ref().unwrap();
        assert!(open.ready);
        assert!(open.quotes.is_empty());
        assert!(open.explanation.is_empty());
        assert_eq!(open.error.as_deref(), Some("Try again"));
    }

    #[test]
    fn a_dismissed_card_is_kept_only_to_be_drawn_while_it_leaves() {
        let mut state = SummaryState::default();
        state.open = Some(SummaryOpen {
            note_id: 1,
            row: 3,
            paragraph: "Decision".into(),
            explanation: String::new(),
            quotes: vec![],
            error: None,
            transcript_revision: 1,
            ready: true,
            trigger: Bounds::default(),
            scroll_offset: Point::default(),
        });
        state.dismiss();
        // Closed as far as every reader of `open` is concerned.
        assert!(state.open.is_none());
        assert_eq!(state.closing.as_ref().map(|open| open.row), Some(3));
        // A second dismissal (a click outside the leaving card) changes nothing.
        state.dismiss();
        assert_eq!(state.closing.as_ref().map(|open| open.row), Some(3));
    }

    #[test]
    fn closing_or_dropping_source_context_cancels_its_pending_transport() {
        let cancel = Arc::new(AtomicBool::new(false));
        let mut state = SummaryState::default();
        state.cancel = Some(cancel.clone());
        state.dismiss();
        assert!(cancel.load(Ordering::Acquire));
        let cancel = Arc::new(AtomicBool::new(false));
        state.cancel = Some(cancel.clone());
        drop(state);
        assert!(cancel.load(Ordering::Acquire));
    }

    #[test]
    fn dismissed_and_replaced_loading_callbacks_cannot_reopen_a_card() {
        let mut state = SummaryState::default();
        state.open = Some(SummaryOpen {
            note_id: 1,
            row: 3,
            paragraph: String::new(),
            explanation: String::new(),
            quotes: vec![],
            error: None,
            transcript_revision: 0,
            ready: false,
            trigger: Bounds::default(),
            scroll_offset: Point::default(),
        });
        let epoch = state.epoch;
        assert!(!state.finish_loading(epoch, 2, 3));
        state.dismiss();
        assert!(!state.finish_loading(epoch, 1, 3));
        assert!(state.open.is_none());
        state.open = Some(SummaryOpen {
            note_id: 1,
            row: 5,
            paragraph: String::new(),
            explanation: String::new(),
            quotes: vec![],
            error: None,
            transcript_revision: 0,
            ready: false,
            trigger: Bounds::default(),
            scroll_offset: Point::default(),
        });
        assert!(!state.finish_loading(epoch, 1, 3));
        assert!(!state.open.as_ref().unwrap().ready);
        assert!(state.finish_loading(state.epoch, 1, 5));
    }
}
