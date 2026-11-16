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
    collections::{HashMap, HashSet},
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
    /// Text that streamed in and was then refused by the server's final
    /// check: drawn only while it fades out over the error that replaces it.
    pub withdrawn: Option<String>,
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
    /// Finished summaries by note, transcript revision and paragraph, so the
    /// same source opens again at once. A changed transcript is a new key.
    cache: HashMap<(u64, u64, String), (String, Vec<TranscriptQuote>)>,
    /// The open card's content height as last laid out, which its own
    /// height springs towards: text arriving grows the card instead of
    /// jumping it. Zero until the first layout of a newly opened card.
    pub measured: Rc<Cell<f32>>,
}

/// How many finished summaries are kept.
const SUMMARY_CACHE: usize = 64;

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
            // The offline preview's "summary" is the paragraph itself.
            if open.explanation.is_empty() {
                open.explanation = open.paragraph.clone();
            }
            open.ready = true;
            return true;
        }
        false
    }

    /// The open card this request is for, while it is still waiting.
    fn pending(
        &mut self,
        epoch: u64,
        note_id: u64,
        row: usize,
        revision: u64,
    ) -> Option<&mut SummaryOpen> {
        if self.epoch != epoch {
            return None;
        }
        self.open.as_mut().filter(|open| {
            open.note_id == note_id
                && open.row == row
                && open.transcript_revision == revision
                && !open.ready
        })
    }

    /// The next words of the summary being written.
    pub fn stream_evidence(
        &mut self,
        epoch: u64,
        note_id: u64,
        row: usize,
        revision: u64,
        text: &str,
    ) -> bool {
        match self.pending(epoch, note_id, row, revision) {
            Some(open) if !text.is_empty() => {
                open.explanation.push_str(text);
                true
            }
            _ => false,
        }
    }

    pub fn finish_evidence(
        &mut self,
        epoch: u64,
        note_id: u64,
        row: usize,
        revision: u64,
        result: Result<(String, Vec<TranscriptQuote>), String>,
    ) -> bool {
        let Some(open) = self.pending(epoch, note_id, row, revision) else {
            return false;
        };
        match result {
            Ok((explanation, quotes)) => {
                open.explanation = explanation;
                open.quotes = quotes;
                open.error = None;
                let entry = (open.explanation.clone(), open.quotes.clone());
                let key = (note_id, revision, open.paragraph.clone());
                if self.cache.len() >= SUMMARY_CACHE {
                    self.cache.clear();
                }
                self.cache.insert(key, entry);
            }
            Err(error) => {
                // What streamed before the failure is not kept: it is only
                // drawn while it fades out.
                let streamed = std::mem::take(&mut open.explanation);
                open.withdrawn = (!streamed.is_empty()).then_some(streamed);
                open.error = Some(error);
            }
        }
        if let Some(open) = self.open.as_mut() {
            open.ready = true;
        }
        self.cancel = None;
        true
    }

    /// A finished summary of this paragraph at this transcript revision.
    pub fn cached(
        &self,
        note_id: u64,
        revision: u64,
        paragraph: &str,
    ) -> Option<(String, Vec<TranscriptQuote>)> {
        self.cache
            .get(&(note_id, revision, paragraph.to_owned()))
            .cloned()
    }
}

/// The server's result as the card shows it, refused if it describes another
/// transcript than the one the card was opened on.
fn evidence_result(
    evidence: meeting_notes_api_types::EvidenceDTO,
    revision: u64,
) -> Result<(String, Vec<TranscriptQuote>), String> {
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

/// Control and invisible formatting characters (bidirectional overrides,
/// zero-width spaces and the like, Unicode Cc and Cf): they can make an
/// address show one place and open another.
fn hidden(c: char) -> bool {
    c.is_control()
        || matches!(c as u32,
            0xAD | 0x600..=0x605 | 0x61C | 0x6DD | 0x70F | 0x890..=0x891 | 0x8E2 | 0x180E
            | 0x200B..=0x200F | 0x202A..=0x202E | 0x2060..=0x2064 | 0x2066..=0x206F | 0xFEFF
            | 0xFFF9..=0xFFFB | 0x110BD | 0x110CD | 0x13430..=0x1343F | 0x1BCA0..=0x1BCA3
            | 0x1D173..=0x1D17A | 0xE0001 | 0xE0020..=0xE007F)
}

/// A host that reads as what it is: a plain ASCII name or address, with an
/// optional port. No user name or password, no percent sign, no letters of
/// another script that only look like these.
fn plain_host(authority: &str) -> bool {
    let (host_ok, after) = if let Some(rest) = authority.strip_prefix('[') {
        match rest.split_once(']') {
            Some((address, after)) => (
                !address.is_empty()
                    && address.chars().all(|c| c.is_ascii_hexdigit() || c == ':' || c == '.'),
                after,
            ),
            None => return false,
        }
    } else {
        let (host, after) = authority.find(':').map_or((authority, ""), |at| authority.split_at(at));
        let name = host.strip_suffix('.').unwrap_or(host);
        (
            !name.is_empty()
                && name.split('.').all(|label| {
                    !label.is_empty() && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
                }),
            after,
        )
    };
    host_ok
        && (after.is_empty()
            || after.strip_prefix(':').is_some_and(|port| {
                (1..=5).contains(&port.len()) && port.chars().all(|c| c.is_ascii_digit())
            }))
}

/// The end of an address: sentence punctuation after it is not part of it,
/// nor a closing bracket whose opening partner is outside it, the way
/// GitHub's autolinker reads them.
fn trimmed_address(candidate: &str) -> &str {
    let mut address = candidate;
    while let Some(last) = address.chars().next_back() {
        let pair = match last {
            ')' => Some('('),
            ']' => Some('['),
            '}' => Some('{'),
            _ => None,
        };
        let unbalanced = pair.is_some_and(|open| {
            address.matches(last).count() > address.matches(open).count()
        });
        if ".,:;!?*_~".contains(last) || unbalanced {
            address = &address[..address.len() - last.len_utf8()];
        } else {
            break;
        }
    }
    address
}

/// Web addresses written out in full (`https://…`) in a line of notes as it
/// is written, before any Markdown is read: byte ranges with the address each
/// one opens. The rules, and the cases every client tests, are in
/// testdata/web-addresses.json. Nothing else in a note is a link: a Markdown
/// `[label](…)` stays as typed, so a link always reads as where it goes.
pub(crate) fn web_links(text: &str) -> Vec<(std::ops::Range<usize>, String)> {
    let lower = text.to_ascii_lowercase();
    let mut links = Vec::new();
    let mut from = 0;
    while let Some(found) = lower[from..].find("http") {
        let start = from + found;
        from = start + 4;
        let scheme = if lower[start..].starts_with("https://") {
            8
        } else if lower[start..].starts_with("http://") {
            7
        } else {
            continue;
        };
        // An address starts a word: `xhttps://` is not one.
        if text[..start].chars().next_back().is_some_and(char::is_alphanumeric) {
            continue;
        }
        let rest = &text[start..];
        let length = rest
            .find(|c: char| c.is_whitespace() || matches!(c, '\u{feff}' | '<' | '>' | '"' | '\'' | '`'))
            .unwrap_or(rest.len());
        let address = trimmed_address(&rest[..length]);
        let authority = address.get(scheme..).unwrap_or("").split(['/', '?', '#']).next().unwrap_or("");
        if !plain_host(authority) || address.contains('\\') || address.chars().any(hidden) {
            continue;
        }
        links.push((start..start + address.len(), address.to_string()));
        from = start + address.len();
    }
    links
}

/// A line of notes as it is drawn: emphasis from its Markdown, and each web
/// address underlined (but for the closing "Chat with meeting transcript"
/// line). The layout says, once drawn, which address is where.
struct LinkedLine {
    text: StyledText,
    layout: TextLayout,
    links: Rc<Vec<(std::ops::Range<usize>, String)>>,
}

fn linked_line(source: &str, theme: Theme) -> LinkedLine {
    // The text keeps its colour; the underline is the list markers' grey.
    let underline = HighlightStyle {
        underline: Some(UnderlineStyle {
            thickness: px(1.),
            color: Some(rgb(0x71717a).into()),
            wavy: false,
        }),
        ..Default::default()
    };
    // Addresses are found in the line as written, so a `*`, `_` or `\` inside
    // one stays in it; the Markdown around them is read piece by piece.
    let (mut text, mut highlights, mut links) = (String::new(), Vec::new(), Vec::new());
    let plain = |piece: &str, text: &mut String, highlights: &mut Vec<_>| {
        let (drawn, ranges) = crate::chat::inline(piece, theme);
        let offset = text.len();
        text.push_str(&drawn);
        highlights.extend(ranges.into_iter().map(|(range, style)| ((range.start + offset)..(range.end + offset), style)));
    };
    // The line the notes end with is plain, as in the reference: its
    // address still opens, it is just not drawn as a link.
    let transcript_chat = source.trim_start().starts_with("Chat with meeting transcript:");
    let mut at = 0;
    for (range, address) in web_links(source) {
        plain(&source[at..range.start], &mut text, &mut highlights);
        let offset = text.len();
        text.push_str(&address);
        if !transcript_chat {
            highlights.push((offset..text.len(), underline));
        }
        links.push((offset..text.len(), address));
        at = range.end;
    }
    plain(&source[at..], &mut text, &mut highlights);
    let text = StyledText::new(text).with_highlights(highlights);
    let layout = text.layout().clone();
    LinkedLine {
        text,
        layout,
        links: Rc::new(links),
    }
}

/// The address under `position` in a drawn line, if there is one.
fn link_at(
    layout: &TextLayout,
    links: &[(std::ops::Range<usize>, String)],
    position: Point<Pixels>,
) -> Option<String> {
    let index = layout.index_for_position(position).ok()?;
    links
        .iter()
        .find(|(range, _)| range.contains(&index))
        .map(|(_, address)| address.clone())
}

/// Where each address of a drawn line is, one rectangle per visual line it
/// spans (a long address wraps), over its own characters only.
fn link_bounds(layout: &TextLayout, links: &[(std::ops::Range<usize>, String)]) -> Vec<Bounds<Pixels>> {
    let line_height = layout.line_height();
    let mut rects: Vec<Bounds<Pixels>> = Vec::new();
    for (range, address) in links {
        let mut edges: Vec<usize> = address.char_indices().map(|(at, _)| range.start + at).collect();
        edges.push(range.end);
        let mut current: Option<Bounds<Pixels>> = None;
        // The last character before a wrap has no next position on its line:
        // it is as wide as the one before it.
        let mut step = px(8.);
        for pair in edges.windows(2) {
            let (Some(from), Some(to)) = (
                layout.position_for_index(pair[0]),
                layout.position_for_index(pair[1]),
            ) else {
                continue;
            };
            let same_line = f32::from(to.y - from.y).abs() < 1.;
            if same_line {
                step = to.x - from.x;
            }
            let end = if same_line { to.x } else { from.x + step };
            match current.as_mut() {
                Some(rect) if f32::from(rect.origin.y - from.y).abs() < 1. => {
                    rect.size.width = end.max(rect.right()) - rect.origin.x;
                }
                _ => {
                    rects.extend(current.take());
                    current = Some(Bounds::new(from, size(end - from.x, line_height)));
                }
            }
        }
        rects.extend(current);
    }
    rects
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
        // The same source opened again: what it said last time, at once.
        let cached = remote
            .then(|| self.summary.cached(note_id, revision, &paragraph))
            .flatten();
        // A new card is laid out at its natural size before its height
        // springs to anything.
        self.summary.measured.set(0.);
        self.stage.forget(("summary-height", 0));
        let ready = cached.is_some();
        let (explanation, quotes) = cached.unwrap_or((String::new(), quotes));
        self.summary.open = Some(SummaryOpen {
            note_id,
            row,
            explanation,
            paragraph: paragraph.clone(),
            quotes,
            error: None,
            withdrawn: None,
            transcript_revision: revision,
            ready,
            trigger,
            scroll_offset: self.note_scroll.offset(),
        });
        let epoch = self.summary.epoch;
        cx.notify();
        if ready {
            return;
        }
        if remote {
            let Some(note) = note else {
                self.summary.finish_evidence(
                    epoch,
                    note_id,
                    row,
                    revision,
                    Err("This note is no longer available.".into()),
                );
                return;
            };
            let cancel = Arc::new(AtomicBool::new(false));
            self.summary.cancel = Some(cancel.clone());
            let receiver = crate::server::stream_evidence(note, paragraph, cancel);
            cx.spawn(async move |this, cx| {
                loop {
                    // Everything that has arrived since the last look, applied
                    // in one update: a burst of tokens is one render.
                    let mut events = Vec::new();
                    let mut closed = false;
                    loop {
                        match receiver.try_recv() {
                            Ok(event) => events.push(event),
                            Err(mpsc::TryRecvError::Empty) => break,
                            Err(mpsc::TryRecvError::Disconnected) => {
                                closed = true;
                                break;
                            }
                        }
                    }
                    let finished = this
                        .update(cx, |this, cx| {
                            if this.summary.epoch != epoch {
                                return true;
                            }
                            // The card cannot be on screen any more (another note or
                            // tab, a changed transcript): it closes, and so does the request.
                            if this.current_note != Some(note_id)
                                || this.active_tab != NoteTab::Enhanced
                                || this
                                    .workspace
                                    .note(note_id)
                                    .is_some_and(|note| note.transcript_revision != revision)
                            {
                                this.summary.dismiss();
                                cx.notify();
                                return true;
                            }
                            let mut changed = false;
                            let mut finished = false;
                            for event in events {
                                use crate::server::EvidenceStreamEvent as Event;
                                let result = match event {
                                    Event::Delta(text) => {
                                        changed |= this
                                            .summary
                                            .stream_evidence(epoch, note_id, row, revision, &text);
                                        continue;
                                    }
                                    Event::Done(evidence) => evidence_result(evidence, revision),
                                    Event::Error(error) => Err(error),
                                };
                                let quoted = result.as_ref().is_ok_and(|(_, quotes)| !quotes.is_empty());
                                if this.summary.finish_evidence(epoch, note_id, row, revision, result) {
                                    changed = true;
                                    // The quotations rise in once their segments are known.
                                    if quoted {
                                        this.stage.mark("source-quotes");
                                    }
                                    // Refused text fades out from where it is.
                                    if this.summary.open.as_ref().is_some_and(|open| open.withdrawn.is_some()) {
                                        this.stage.forget(("summary-withdrawn", 0));
                                        this.stage.aim(("summary-withdrawn", 0), 1., crate::motion::SNAPPY);
                                    }
                                }
                                finished = true;
                                break;
                            }
                            if !finished && closed {
                                finished = this.summary.finish_evidence(
                                    epoch,
                                    note_id,
                                    row,
                                    revision,
                                    Err("Could not load transcript context. Try again.".into()),
                                );
                                changed |= finished;
                                finished = true;
                            }
                            if changed {
                                cx.notify();
                            }
                            finished
                        })
                        .unwrap_or(true);
                    if finished {
                        return;
                    }
                    cx.background_executor()
                        .timer(Duration::from_millis(16))
                        .await;
                }
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
                    this.stage.mark("source-quotes");
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
            // A web address in the line (the chat link at the foot of generated
            // notes, say) opens when clicked, under the arrow; anywhere else in
            // the line a click edits it, as before.
            let drawn = editing.is_none().then(|| linked_line(&text, t));
            let links = drawn.as_ref().map(|line| (line.layout.clone(), line.links.clone()));
            let linked = links.as_ref().is_some_and(|(_, links)| !links.is_empty());
            paragraph = paragraph.child(
                div()
                    .id(("enhanced-text", row))
                    .flex_1()
                    .min_w_0()
                    .child(if let Some(edit) = editing {
                        edit.input.clone().into_any_element()
                    } else {
                        drawn.map(|line| line.text).unwrap().into_any_element()
                    })
                    .when_some(links.clone().filter(|_| linked), |text, (layout, links)| {
                        text.child(
                            canvas(
                                move |_, window, _| {
                                    link_bounds(&layout, &links)
                                        .into_iter()
                                        .map(|rect| window.insert_hitbox(rect, HitboxBehavior::Normal))
                                        .collect::<Vec<_>>()
                                },
                                |_, hitboxes, window, _| {
                                    for hitbox in &hitboxes {
                                        window.set_cursor_style(CursorStyle::Arrow, hitbox);
                                    }
                                },
                            )
                            .absolute()
                            .size_0(),
                        )
                    })
                    .when(!read_only || linked, |text| {
                        text.when(!read_only, |text| text.cursor(CursorStyle::IBeam))
                            .on_click(cx.listener({
                                let note_id = note.id;
                                move |this, event: &ClickEvent, window, cx| {
                                    if let Some((layout, links)) = links.as_ref()
                                        && !links.is_empty()
                                        && let Some(address) = link_at(layout, links, event.position())
                                    {
                                        crate::shared_view::open_in_browser(&address, cx);
                                        return;
                                    }
                                    if !read_only {
                                        this.begin_enhanced_edit(note_id, editable_row, window, cx)
                                    }
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
                        .cursor_default()
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
                                move |bounds, _, _| {
                                    for_canvas.set(Some(bounds));
                                    #[cfg(debug_assertions)]
                                    probe::placed(note_id, row, bounds);
                                },
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
        if !open.ready && open.explanation.is_empty() {
            // Until the first words arrive: one line, as tall as a line of
            // the summary that replaces it, so the card does not jump.
            body = body.child(
                div()
                    .h(px(22.))
                    .flex()
                    .items_center()
                    .text_color(muted)
                    .child(crate::motion::shimmer_label(
                        SharedString::from(format!("source-reading-{}", self.summary.epoch)),
                        "Reading the transcript…",
                        t,
                        cx,
                    )),
            );
        } else if let Some(error) = &open.error {
            let note_id = open.note_id;
            let row = open.row;
            let paragraph = open.paragraph.clone();
            let trigger = open.trigger;
            // Text the final check refused leaves deliberately: it fades out
            // in place while the error fades in under it and the card's
            // height follows the error. Reduced motion swaps them at once.
            let withdrawn = open.withdrawn.as_ref().map(|text| {
                let level = self
                    .stage
                    .aim(("summary-withdrawn", 0), 0., crate::motion::SNAPPY)
                    .clamp(0., 1.);
                (text, level)
            });
            let shown = withdrawn.map_or(1., |(_, level)| 1. - level);
            body = body
                .child(
                    div()
                        .relative()
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(20.))
                                .text_color(muted)
                                .opacity(shown)
                                .child(error.clone()),
                        )
                        .when_some(withdrawn.filter(|(_, level)| *level > 0.01), |block, (text, level)| {
                            block.child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .left_0()
                                    .right_0()
                                    .opacity(level)
                                    .child(crate::chat::inline_text(text, t)),
                            )
                        }),
                )
                .child(
                    div()
                        .id("retry-source-summary")
                        .opacity(shown)
                        .px(px(10.))
                        .py(px(6.))
                        .rounded(px(6.))
                        .text_size(px(13.))
                        .cursor_default()
                        .child(crate::motion::tactile(skeleton).radius(6.))
                        .child("Try again")
                        .reach().on_click(cx.listener(move |this, _, _, cx| {
                            this.summary.dismiss();
                            this.open_summary(note_id, row, paragraph.clone(), trigger, cx);
                        })),
                );
        } else {
            body = body.child(crate::chat::inline_text(
                if open.ready && open.explanation.trim().is_empty() {
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
                // Each quotation rises in on its turn once the segments
                // are known; reduced motion fades them in together.
                let pose = self.stage.cascade("source-quotes", index);
                body = body.child(crate::motion::lift(
                    div()
                        .id(SharedString::from(format!("source-quote-{index}")))
                        .flex()
                        .gap(px(12.))
                        .py(px(4.))
                        .rounded(px(6.))
                        .bg(skeleton)
                        .cursor_default()
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
                    pose,
                ));
            }
        }
        if open.ready && self.recording.note_id == Some(open.note_id) && self.recording.is_active()
        {
            body = body.child(div().pt(px(8.)).border_t_1().border_color(border).text_size(px(12.)).line_height(px(16.)).text_color(muted).child("Transcription is still active. These notes were generated at a previous point when transcription was paused."));
        }
        // The card's height follows its content on a spring: the summary
        // arriving line by line and the quotations after it grow the card
        // rather than jump it. Its first layout is at the natural size, which
        // is then measured; reduced motion follows at once.
        let max_height = px(448.).min(window.viewport_size().height - px(32.));
        let measured = self.summary.measured.get();
        let height = (measured > 0.).then(|| {
            // The content and the card's 1 px border above and below it.
            let target = (measured + 2.).min(f32::from(max_height));
            if live {
                self.stage
                    .aim(("summary-height", 0), target, crate::motion::SNAPPY)
            } else {
                // Leaving: keep the height it had.
                self.stage.target(("summary-height", 0)).unwrap_or(target)
            }
        });
        let measure = {
            let measured = self.summary.measured.clone();
            canvas(
                move |bounds, window, _| {
                    let height = f32::from(bounds.size.height);
                    if (measured.get() - height).abs() > 0.5 {
                        measured.set(height);
                        window.request_animation_frame();
                    }
                },
                |_, _, _, _| {},
            )
            .absolute()
            .inset_0()
        };
        let body = body.relative().flex_shrink_0().when(live, |body| body.child(measure));
        let card = div()
            .id("transcript-summary-popover")
            .w(width)
            .when_some(height, |card, height| card.h(px(height)))
            .max_h(max_height)
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

/// `CEREAL_WELCOME_TOUR=source` (debug builds, isolated data): after the
/// tour has signed in, open a note's source card the way a person does and
/// say on stderr what it shows as the summary streams in, so captures of the
/// window can be lined up with it. Pointer input goes through the window's
/// own listeners; the real cursor is never moved.
#[cfg(debug_assertions)]
pub mod probe {
    use crate::app::{Cereal, NoteTab};
    use gpui::*;
    use std::{cell::RefCell, collections::HashMap, time::Duration};

    thread_local! {
        static TRIGGERS: RefCell<HashMap<(u64, usize), Bounds<Pixels>>> = RefCell::new(HashMap::new());
    }

    /// Where the source trigger of `row` was last laid out.
    pub fn placed(note_id: u64, row: usize, bounds: Bounds<Pixels>) {
        TRIGGERS.with(|triggers| triggers.borrow_mut().insert((note_id, row), bounds));
    }

    fn say(what: &str) {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_millis());
        eprintln!("welcome {stamp} {what}");
    }

    async fn pause(cx: &mut AsyncApp, millis: u64) {
        cx.background_executor()
            .timer(Duration::from_millis(millis))
            .await;
    }

    fn send(handle: WindowHandle<Cereal>, cx: &mut AsyncApp, event: PlatformInput) {
        let _ = cx.update_window(handle.into(), |_, window, cx| window.dispatch_event(event, cx));
    }

    /// Hover the paragraph, then press its magnifier.
    async fn click(handle: WindowHandle<Cereal>, cx: &mut AsyncApp, at: Point<Pixels>) {
        let row = point(at.x - px(200.), at.y);
        send(handle, cx, PlatformInput::MouseMove(MouseMoveEvent { position: row, ..Default::default() }));
        pause(cx, 350).await;
        send(handle, cx, PlatformInput::MouseMove(MouseMoveEvent { position: at, ..Default::default() }));
        pause(cx, 250).await;
        let button = MouseButton::Left;
        send(handle, cx, PlatformInput::MouseDown(MouseDownEvent { button, position: at, click_count: 1, ..Default::default() }));
        pause(cx, 90).await;
        send(handle, cx, PlatformInput::MouseUp(MouseUpEvent { button, position: at, click_count: 1, ..Default::default() }));
    }

    /// The card as it is now: its state, how much summary it shows, its
    /// quotations and the height it is laid out at.
    fn card(view: &Cereal) -> String {
        match &view.summary.open {
            None => "closed".into(),
            Some(open) => format!(
                "row {} ready {} chars {} quotes {} error {:?} content_h {:.1} card_h {:.1}",
                open.row,
                open.ready,
                open.explanation.chars().count(),
                open.quotes.len(),
                open.error,
                view.summary.measured.get(),
                view.stage.target(("summary-height", 0)).unwrap_or(0.)
            ),
        }
    }

    /// Follow the open card until it is finished or `seconds` pass, saying
    /// every change of what it shows.
    async fn follow(handle: WindowHandle<Cereal>, cx: &mut AsyncApp, label: &str, seconds: u64) -> bool {
        let mut last = String::new();
        for _ in 0..seconds * 100 {
            let (now, ready) = handle
                .update(cx, |view, _, _| (card(view), view.summary.open.as_ref().is_some_and(|open| open.ready)))
                .unwrap_or_default();
            if now != last {
                say(&format!("source {label}: {now}"));
                last = now;
            }
            if ready {
                return true;
            }
            pause(cx, 10).await;
        }
        false
    }

    pub async fn run(handle: WindowHandle<Cereal>, cx: &mut AsyncApp) {
        // The server's notes, read after signing in.
        let mut target = None;
        for _ in 0..600 {
            target = handle
                .update(cx, |view, _, _| {
                    view.workspace.notes.iter().find(|note| {
                        note.server_id.is_some() && !note.enhanced.trim().is_empty() && !note.transcript.is_empty()
                    }).map(|note| note.id)
                })
                .ok()
                .flatten();
            if target.is_some() {
                break;
            }
            pause(cx, 50).await;
        }
        let Some(note_id) = target else {
            say("source: no server note with enhanced notes and a transcript");
            return;
        };
        let _ = handle.update(cx, |view, _, cx| {
            view.open_note(note_id, cx);
            view.active_tab = NoteTab::Enhanced;
            cx.notify();
        });
        pause(cx, 2000).await;
        let rows: Vec<usize> = TRIGGERS.with(|triggers| {
            let mut rows: Vec<_> = triggers.borrow().keys().filter(|(note, _)| *note == note_id).map(|(_, row)| *row).collect();
            rows.sort();
            rows
        });
        say(&format!("source: note {note_id} rows with a trigger {rows:?}"));
        let at = |row: usize| TRIGGERS.with(|triggers| triggers.borrow().get(&(note_id, row)).map(|bounds| bounds.center()));
        // The second card is one the first does not cover: under another heading.
        let (Some(&first), Some(&second)) = (rows.first(), rows.get(3).or(rows.last())) else {
            say("source: fewer than two paragraphs with a trigger");
            return;
        };
        say("shot source-0-before");
        pause(cx, 800).await;
        // 1. Open, and watch it stream.
        click(handle, cx, at(first).unwrap()).await;
        say("source opened first");
        let finished = follow(handle, cx, "first", 40).await;
        say(&format!("source first finished: {finished}"));
        pause(cx, 1500).await;
        say("shot source-1-done");
        pause(cx, 800).await;
        // 2. Close it and open it again: from the cache, at once.
        click(handle, cx, at(first).unwrap()).await;
        pause(cx, 700).await;
        say(&format!("source after toggling closed: {}", handle.update(cx, |view, _, _| card(view)).unwrap_or_default()));
        click(handle, cx, at(first).unwrap()).await;
        say(&format!("source reopened: {}", handle.update(cx, |view, _, _| card(view)).unwrap_or_default()));
        pause(cx, 900).await;
        say("shot source-2-reopened");
        pause(cx, 800).await;
        // 3. Closed again, then another paragraph, closed while it streams.
        click(handle, cx, at(first).unwrap()).await;
        pause(cx, 700).await;
        click(handle, cx, at(second).unwrap()).await;
        say("source opened second");
        for _ in 0..1000 {
            let chars = handle
                .update(cx, |view, _, _| view.summary.open.as_ref().map_or(0, |open| open.explanation.len()))
                .unwrap_or(0);
            if chars > 0 {
                break;
            }
            pause(cx, 10).await;
        }
        pause(cx, 300).await;
        say(&format!("source second before closing: {}", handle.update(cx, |view, _, _| card(view)).unwrap_or_default()));
        // Its magnifier again closes it.
        click(handle, cx, at(second).unwrap()).await;
        say(&format!("source second closed mid-stream: {}", handle.update(cx, |view, _, _| card(view)).unwrap_or_default()));
        pause(cx, 1500).await;
        let cached = handle
            .update(cx, |view, _, _| {
                let note = view.workspace.note(note_id);
                note.and_then(|note| {
                    let line = note.enhanced.lines().nth(second)?;
                    view.summary.cached(note_id, note.transcript_revision, &super::paragraph_text(line))
                })
                .is_some()
            })
            .unwrap_or(false);
        say(&format!("source second cached after closing mid-stream: {cached}"));
    }
}

#[cfg(test)]
mod tests {
    use super::{
        NoteLine, SummaryOpen, SummaryState, emphasis_ranges, list_indent_unit, matching_quotes,
        note_line, web_links,
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
    fn the_addresses_that_become_links_are_the_same_on_every_client() {
        // testdata/web-addresses.json: the cases the web reader and the iPhone run too.
        let vectors: serde_json::Value =
            serde_json::from_str(include_str!("../../../testdata/web-addresses.json")).unwrap();
        let cases = vectors["cases"].as_array().unwrap();
        assert!(cases.len() > 15);
        for case in cases {
            let text = case["text"].as_str().unwrap();
            let expected: Vec<&str> = case["links"].as_array().unwrap().iter().map(|link| link.as_str().unwrap()).collect();
            let found: Vec<String> = web_links(text).into_iter().map(|(range, address)| {
                assert_eq!(&text[range], address);
                address
            }).collect();
            assert_eq!(found, expected, "{text:?}");
        }
    }

    #[test]
    fn web_addresses_written_out_in_full_are_the_only_links() {
        let address = "https://notes.example.test/notes/51207249-97a9-4bd7-b02f-93428ffb367a/chat";
        let line = format!("Chat with meeting transcript: {address} I CAN TYPE HERE");
        let links = web_links(&line);
        assert_eq!(links.len(), 1);
        assert_eq!(&line[links[0].0.clone()], address);
        assert_eq!(links[0].1, address);
        // Sentence punctuation is not part of an address; brackets and quotes end it.
        let links = web_links("See (http://10.30.1.77:3000/notes/x), then \"https://a.example/b\".");
        assert_eq!(
            links.iter().map(|(_, address)| address.as_str()).collect::<Vec<_>>(),
            ["http://10.30.1.77:3000/notes/x", "https://a.example/b"]
        );
        // Not addresses: no host, another scheme, or the middle of a word.
        for text in ["http:// nothing", "ftp://a.example", "xhttps://a.example", "Chat with meeting transcript: ORD ELETE", ""] {
            assert!(web_links(text).is_empty(), "{text}");
        }
        // Byte ranges stay on character boundaries after non-ASCII text.
        let line = "Résumé → https://a.example/é";
        let links = web_links(line);
        assert_eq!(&line[links[0].0.clone()], "https://a.example/é");
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
            withdrawn: None,
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

    fn waiting(note_id: u64, row: usize, paragraph: &str, revision: u64) -> SummaryOpen {
        SummaryOpen {
            note_id,
            row,
            paragraph: paragraph.into(),
            explanation: String::new(),
            quotes: vec![],
            error: None,
            withdrawn: None,
            transcript_revision: revision,
            ready: false,
            trigger: Bounds::default(),
            scroll_offset: Point::default(),
        }
    }

    #[test]
    fn streamed_words_accumulate_then_the_final_result_replaces_them_and_is_cached() {
        let mut state = SummaryState::default();
        state.open = Some(waiting(4, 2, "Ship Monday", 7));
        let epoch = state.epoch;
        assert!(state.stream_evidence(epoch, 4, 2, 7, "The launch "));
        assert!(state.stream_evidence(epoch, 4, 2, 7, "moved."));
        // Another card, revision or request never receives them.
        assert!(!state.stream_evidence(epoch, 4, 3, 7, "x"));
        assert!(!state.stream_evidence(epoch, 4, 2, 8, "x"));
        assert!(!state.stream_evidence(epoch.wrapping_add(1), 4, 2, 7, "x"));
        let open = state.open.as_ref().unwrap();
        assert_eq!(open.explanation, "The launch moved.");
        assert!(!open.ready);
        assert!(state.cached(4, 7, "Ship Monday").is_none());
        let quote = super::TranscriptQuote {
            segment_id: Some("00000000-0000-4000-8000-000000000001".into()),
            text: "We moved it.".into(),
            speaker: Some("Speaker 1".into()),
            timestamp: Some("00:44".into()),
        };
        // The server's final reading wins over what streamed.
        assert!(state.finish_evidence(epoch, 4, 2, 7, Ok(("The launch moved to Monday.".into(), vec![quote.clone()]))));
        let open = state.open.as_ref().unwrap();
        assert!(open.ready);
        assert_eq!(open.explanation, "The launch moved to Monday.");
        assert_eq!(open.quotes, vec![quote.clone()]);
        // Late words cannot change a finished card.
        assert!(!state.stream_evidence(epoch, 4, 2, 7, " more"));
        assert_eq!(
            state.cached(4, 7, "Ship Monday"),
            Some(("The launch moved to Monday.".into(), vec![quote]))
        );
        // A changed transcript or another paragraph is not the same source.
        assert!(state.cached(4, 8, "Ship Monday").is_none());
        assert!(state.cached(4, 7, "Ship Tuesday").is_none());
        // The cache outlives the card.
        state.dismiss();
        assert!(state.cached(4, 7, "Ship Monday").is_some());
    }

    #[test]
    fn a_failure_mid_stream_drops_the_streamed_words_and_is_not_cached() {
        let mut state = SummaryState::default();
        state.open = Some(waiting(1, 0, "Decision", 1));
        let epoch = state.epoch;
        assert!(state.stream_evidence(epoch, 1, 0, 1, "Half a sente"));
        assert!(state.finish_evidence(epoch, 1, 0, 1, Err("Try again".into())));
        let open = state.open.as_ref().unwrap();
        assert!(open.ready && open.explanation.is_empty() && open.quotes.is_empty());
        assert_eq!(open.error.as_deref(), Some("Try again"));
        // Kept only to be drawn fading out over the error.
        assert_eq!(open.withdrawn.as_deref(), Some("Half a sente"));
        assert!(state.cached(1, 1, "Decision").is_none());
        // A failure before any words has nothing to withdraw.
        state.open = Some(waiting(1, 0, "Decision", 1));
        assert!(state.finish_evidence(state.epoch, 1, 0, 1, Err("Try again".into())));
        assert!(state.open.as_ref().unwrap().withdrawn.is_none());
    }

    #[test]
    fn closing_mid_stream_cancels_the_request() {
        let mut state = SummaryState::default();
        state.open = Some(waiting(1, 0, "Decision", 1));
        let cancel = Arc::new(AtomicBool::new(false));
        state.cancel = Some(cancel.clone());
        assert!(state.stream_evidence(state.epoch, 1, 0, 1, "Some"));
        state.dismiss();
        assert!(cancel.load(Ordering::Acquire));
        assert!(!state.stream_evidence(state.epoch, 1, 0, 1, " more"));
    }

    #[test]
    fn the_final_result_is_refused_for_another_transcript_revision() {
        let evidence = meeting_notes_api_types::EvidenceDTO {
            summary: "Moved".into(),
            quotes: vec![meeting_notes_api_types::EvidenceQuoteDTO {
                segment_id: "00000000-0000-4000-8000-000000000001".into(),
                text: "We moved it.".into(),
                speaker_label: Some("Speaker 4".into()),
                start_ms: 47_000,
            }],
            transcript_revision: 3,
        };
        let (summary, quotes) = super::evidence_result(evidence.clone(), 3).unwrap();
        assert_eq!(summary, "Moved");
        assert_eq!(quotes[0].timestamp.as_deref(), Some("00:47"));
        assert_eq!(quotes[0].speaker.as_deref(), Some("Speaker 4"));
        assert!(super::evidence_result(evidence, 4).is_err());
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
            withdrawn: None,
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
            withdrawn: None,
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
            withdrawn: None,
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
            withdrawn: None,
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
