//! The window's side of live transcription: starting and ending the stream
//! with the recording, and turning what it reports into transcript state.
//!
//! Final text becomes ordinary transcript segments of the note, in place, and
//! is saved with it on a slow cadence (the server already holds it). Interim
//! text lives only here: it is never written into the note, never persisted,
//! and is at most a handful of lines however long the meeting runs. An
//! interim update therefore costs one redraw of the window and nothing else:
//! no transcript is re-parsed, no note is copied and nothing is saved.
use crate::{
    app::Cereal,
    live::{Channel, Delivery, Event, Interim, Phase, Session, Source, SpeakerMerge, SpeakerRelabel},
    model::{Note, PendingStop},
    note::TranscriptUtterance,
    recording::TranscriptionAttention,
};
use gpui::*;
use meeting_notes_api_types::{AudioChannel, TranscriptSegmentDTO};
use std::{
    cell::RefCell,
    collections::VecDeque,
    rc::Rc,
    time::{Duration, Instant},
};

/// Interim utterances kept at once. There is one per channel, so two; the
/// cap is a backstop, not a working limit.
const MAX_INTERIM: usize = 4;
/// The longest interim text shown; a provider that never commits must not
/// grow a bubble without bound.
const MAX_INTERIM_CHARS: usize = 2_000;
/// How often text that arrived on the stream is written to the workspace
/// file. The server stores every final segment before sending it, so the
/// local copy is a cache and a crash loses nothing.
const SAVE_EVERY: Duration = Duration::from_secs(10);
/// A reconnect shorter than this is not mentioned.
const RECONNECT_GRACE: Duration = Duration::from_millis(1500);
/// A first connection that takes longer than this is mentioned.
const CONNECT_GRACE: Duration = Duration::from_secs(5);
/// A stretch without transcript shorter than this is not shown as missing.
pub const GAP_SHOWN_MS: i64 = 1_000;
/// After a socket becomes ready the server first re-sends the finals it
/// stored while none was attached. Only if the note is still behind the
/// revision it announced once those have had this long to arrive is the note
/// read again (protocol notes, point 15).
const RESYNC_AFTER: Duration = Duration::from_millis(1500);
/// Stage tracks for lines that are arriving or firming up.
const LINE_SLOTS: u64 = 16;
pub const LINE_TRACK: &str = "live-line";

struct Pending {
    note: u64,
    capture_id: String,
    monitor: crate::audio::CaptureMonitor,
    /// The socket is tried once per recording.
    tried: bool,
}

struct Stream {
    /// Tells this stream's events from those of another stream of its note.
    id: u64,
    note: u64,
    capture_id: String,
    session: Session,
    /// What the stream said came of its capture.
    delivered: Option<Delivery>,
}

/// What the transcript views show of the stream, besides its text.
#[derive(Clone, Debug, PartialEq)]
pub struct LiveStatus {
    pub text: SharedString,
    /// The provider is the problem: shown with the recording's attention state.
    pub warning: bool,
}

#[derive(Default)]
pub struct LiveTranscript {
    /// The note whose recording the state below belongs to.
    note: Option<u64>,
    capture: Option<Pending>,
    session: Option<Stream>,
    /// Stopped recordings whose streams are still delivering their last text.
    ending: Vec<Stream>,
    /// Text still being spoken, oldest first.
    interim: Vec<Interim>,
    /// Who last spoke on each channel (microphone, system) as far as this
    /// stream has said so in a final: speaker id and label. An interim line
    /// that names no speaker stands under this one. It is forgotten whenever
    /// the server may have started numbering voices again (a new recording
    /// of the note, a reconnection, a pause), so words spoken after Resume
    /// are never shown under a label an earlier session gave someone else.
    spoke: [Option<(String, Option<String>)>; 2],
    /// Utterances that already have final text: a late interim for one of
    /// them is old news.
    finished: VecDeque<String>,
    phase: Phase,
    behind: u64,
    phase_since: Option<Instant>,
    /// The newest stretch of this recording that will have no transcript
    /// (note-timeline milliseconds): said until another recording starts.
    skipped: Option<(u64, u64)>,
    /// Notes whose pending Stop has been sent and not answered yet.
    pub stopping: std::collections::HashSet<u64>,
    /// The transcript revision the server announced when the socket became
    /// ready, and when to hold the note against it.
    resync: Option<(u64, Instant)>,
    /// What the status line last said, so a timed change redraws once.
    shown: Option<LiveStatus>,
    /// The last thing the status line said, drawn while it fades out.
    said: Option<LiveStatus>,
    streams: u64,
    /// Changes whenever the interim text or the status may have changed.
    revision: u64,
    tail: RefCell<Option<((u64, usize), Rc<Vec<TranscriptUtterance>>)>>,
    watching: bool,
    save_armed: bool,
    /// Interim lines that have just appeared (utterance id, stage slot).
    fresh: Vec<(String, u64)>,
    /// Segments that have just replaced interim text (segment id, stage slot).
    firmed: Vec<(String, u64)>,
    /// For each of those segments, the interim line it replaced: its
    /// utterance and the words it showed, so the final settles out of the
    /// same bubble.
    replaced: Vec<(String, String, String)>,
    slot: u64,
}

impl LiveTranscript {
    /// Whether text for `note` is arriving on a stream right now.
    pub fn streams(&self, note: u64) -> bool {
        self.session.as_ref().is_some_and(|stream| stream.note == note)
            || self.ending.iter().any(|stream| stream.note == note)
    }

    /// Whether a stream of `note` still owes the server part of its capture.
    pub fn delivering(&self, note: u64) -> bool {
        self.session
            .iter()
            .chain(&self.ending)
            .any(|stream| stream.note == note && stream.delivered.is_none())
    }

    /// A stopped recording of `note` is still being delivered.
    pub fn finishing(&self, note: u64) -> bool {
        self.ending
            .iter()
            .any(|stream| stream.note == note && stream.delivered.is_none())
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// For the live probe: cut the stream's connection as a lost network
    /// would. Returns whether there was a stream to cut.
    pub fn sever(&self) -> bool {
        self.session
            .as_ref()
            .map(|stream| stream.session.sever())
            .is_some()
    }

    /// One line for diagnostics: what the stream is doing.
    pub fn describe(&self) -> String {
        format!(
            "stream {:?}{}, {} interim",
            self.phase,
            if self.session.is_some() { "" } else { " (no session)" },
            self.interim.len()
        )
    }

    /// The stream of `note` is up: speech will appear as it is spoken.
    pub fn listening(&self, note: u64) -> bool {
        self.note == Some(note) && matches!(self.phase, Phase::Live | Phase::CatchingUp)
    }

    /// The interim lines of `note` as transcript rows that follow its final
    /// ones. Built once per change, not per frame.
    pub fn tail(&self, note: &Note) -> Rc<Vec<TranscriptUtterance>> {
        thread_local! {
            static NONE: Rc<Vec<TranscriptUtterance>> = Rc::new(Vec::new());
        }
        if self.note != Some(note.id) || self.interim.is_empty() {
            return NONE.with(Rc::clone);
        }
        let key = (self.revision, note.server_transcript_segments.len());
        let spoke = &self.spoke;
        let mut cache = self.tail.borrow_mut();
        if let Some((cached, rows)) = cache.as_ref()
            && *cached == key
        {
            return rows.clone();
        }
        let rows = Rc::new(
            self.interim
                .iter()
                .map(|interim| {
                    interim_row(interim, spoke[usize::from(interim.channel != Channel::Mic)].as_ref())
                })
                .collect::<Vec<_>>(),
        );
        *cache = Some((key, rows.clone()));
        rows
    }

    /// The stage slot of interim line `index` of the tail while it is still
    /// arriving.
    pub fn fresh_slot(&self, index: usize) -> Option<u64> {
        let id = &self.interim.get(index)?.utterance_id;
        self.fresh
            .iter()
            .find(|(fresh, _)| fresh == id)
            .map(|(_, slot)| *slot)
    }

    /// The stage slot of a segment that has just replaced interim text.
    pub fn firmed_slot(&self, segment_id: &str) -> Option<u64> {
        self.firmed
            .iter()
            .find(|(firmed, _)| firmed == segment_id)
            .map(|(_, slot)| *slot)
    }

    /// The interim line that segment `segment_id` has just replaced: its row
    /// (as [`LiveTranscript::tail`] identified it) and the words it showed.
    pub fn replaced(&self, segment_id: &str) -> Option<(u64, &str)> {
        self.replaced
            .iter()
            .find(|(segment, _, _)| segment == segment_id)
            .map(|(_, utterance, text)| (crate::lazy::key(("interim", utterance)), text.as_str()))
    }

    /// Stage slots of the lines that are arriving or firming up.
    pub fn line_slots(&self) -> impl Iterator<Item = u64> + '_ {
        self.fresh.iter().chain(&self.firmed).map(|(_, slot)| *slot)
    }

    /// Forget lines whose animation has finished (`settled` is asked per slot).
    pub fn settle_lines(&mut self, settled: impl Fn(u64) -> bool) {
        self.fresh.retain(|(_, slot)| !settled(*slot));
        self.firmed.retain(|(_, slot)| !settled(*slot));
        self.forget_replaced();
    }

    fn forget_replaced(&mut self) {
        let firmed = &self.firmed;
        self.replaced
            .retain(|(segment, _, _)| firmed.iter().any(|(known, _)| known == segment));
    }

    /// The last status that was shown for `note`, for its exit.
    pub fn last_status(&self, note: u64) -> Option<LiveStatus> {
        self.said.clone().filter(|_| self.note == Some(note))
    }

    /// The quiet line under the transcript of `note`, if there is anything
    /// to say: the stream is behind, reconnecting, skipped a stretch, or the
    /// provider is unavailable. Nothing while everything is simply working,
    /// and nothing at all when live transcription is not in use.
    pub fn status(&self, note: u64) -> Option<LiveStatus> {
        self.status_of(note, Instant::now())
    }

    /// The status line of the recording this state belongs to, as of `now`.
    fn status_at(&self, now: Instant) -> Option<LiveStatus> {
        self.status_of(self.note?, now)
    }

    fn status_of(&self, note: u64, now: Instant) -> Option<LiveStatus> {
        let quiet = |text: String| {
            Some(LiveStatus {
                text: text.into(),
                warning: false,
            })
        };
        let since = |at: Option<Instant>| at.map_or(Duration::MAX, |at| now.duration_since(at));
        let ours = self.note == Some(note);
        let live = match self.phase {
            _ if !ours => None,
            Phase::ProviderUnavailable => Some(LiveStatus {
                text: "Live transcript unavailable · audio is saved on this Mac".into(),
                warning: true,
            }),
            Phase::CatchingUp if self.behind >= 5 => {
                quiet(format!("Transcript about {} s behind", self.behind))
            }
            Phase::CatchingUp => quiet("Catching up…".into()),
            // Audio is not reaching the server: it waits in the capture's
            // files and is sent when the connection is back.
            Phase::Reconnecting if since(self.phase_since) >= RECONNECT_GRACE => {
                quiet("Reconnecting · audio is saved on this Mac".into())
            }
            Phase::Connecting if since(self.phase_since) >= CONNECT_GRACE => {
                quiet("Reconnecting · audio is saved on this Mac".into())
            }
            _ => None,
        };
        live.or_else(|| {
            self.finishing(note)
                .then(|| quiet("Finishing the transcript…".into()))
                .flatten()
        })
        .or_else(|| {
            let (from, to) = self.skipped.filter(|_| ours)?;
            quiet(format!("Couldn't transcribe {}", gap_span(from, to)))
        })
    }

    /// Whether the note has to be read again from the server: it is still
    /// behind the revision a ready socket announced, after the finals the
    /// server re-sends have had time to arrive. Asked once per announcement.
    fn resync_due(&mut self, note: &Note, now: Instant) -> bool {
        match self.resync {
            Some((revision, at)) if now >= at => {
                self.resync = None;
                note.transcript_revision < revision
            }
            _ => false,
        }
    }

    fn next_slot(&mut self) -> u64 {
        self.slot = (self.slot + 1) % LINE_SLOTS;
        // A slot handed out again belongs to its new line only.
        let slot = self.slot;
        self.fresh.retain(|(_, used)| *used != slot);
        self.firmed.retain(|(_, used)| *used != slot);
        self.forget_replaced();
        slot
    }

    /// Start showing the state of a new recording of `note`.
    fn begin(&mut self, note: u64) {
        self.note = Some(note);
        self.interim.clear();
        self.spoke = [None, None];
        self.finished.clear();
        self.fresh.clear();
        self.firmed.clear();
        self.replaced.clear();
        self.phase = Phase::Off;
        self.behind = 0;
        self.phase_since = None;
        self.skipped = None;
        self.resync = None;
        self.shown = None;
        self.said = None;
        self.revision = self.revision.wrapping_add(1);
    }

    /// Take in one event of the stream for `note`. `current` is false for a
    /// stream that is finishing after its recording stopped. Returns what the
    /// caller has to do about it.
    fn apply(&mut self, note: &mut Note, current: bool, event: Event, now: Instant) -> Applied {
        match event {
            Event::Final {
                utterance_id,
                revision,
                segment,
            } => {
                let interim = self
                    .interim
                    .iter()
                    .position(|shown| shown.utterance_id == utterance_id)
                    .map(|at| self.interim.remove(at));
                let replaced = interim.is_some();
                self.fresh.retain(|(id, _)| *id != utterance_id);
                let utterance = utterance_id.clone();
                if !self.finished.contains(&utterance_id) {
                    self.finished.push_back(utterance_id);
                    if self.finished.len() > 32 {
                        self.finished.pop_front();
                    }
                }
                let id = segment.id.clone();
                if current {
                    self.spoke[usize::from(segment.channel != AudioChannel::Mic)] =
                        Some((segment.speaker_id.clone(), segment.speaker_label.clone()));
                }
                let added = add_segment(note, segment, revision);
                if added && replaced && current {
                    let slot = self.next_slot();
                    if let Some(interim) = interim {
                        self.replaced.push((id.clone(), utterance, interim.text));
                    }
                    self.firmed.push((id, slot));
                    return Applied::Final(Some(slot));
                }
                if added || replaced {
                    Applied::Final(None)
                } else {
                    Applied::Nothing
                }
            }
            Event::Speakers(merges) => {
                if merge_speakers(note, &merges) {
                    Applied::Speakers
                } else {
                    Applied::Nothing
                }
            }
            Event::Relabels(relabels) => {
                if relabel_speakers(note, &relabels) {
                    Applied::Speakers
                } else {
                    Applied::Nothing
                }
            }
            Event::Gap {
                channel,
                from_ms,
                to_ms,
            } => {
                // Kept with the note, for the transcript to show where it is
                // missing; and said on the status line of this recording.
                let added = note.add_transcript_gaps([meeting_notes_api_types::TranscriptGapDTO {
                    channel: audio_channel(channel),
                    from_ms: from_ms as i64,
                    to_ms: to_ms as i64,
                }]);
                if self.note == Some(note.id) && to_ms.saturating_sub(from_ms) >= GAP_SHOWN_MS as u64 {
                    let (from, to) = self.skipped.map_or((from_ms, to_ms), |(at, until)| {
                        // The newest stretch, joined to the one before it
                        // when they touch (the server and the stream both
                        // say a skip).
                        if from_ms <= until && to_ms >= at { (at.min(from_ms), until.max(to_ms)) } else { (from_ms, to_ms) }
                    });
                    self.skipped = Some((from, to));
                    return Applied::Gap;
                }
                if added { Applied::Gap } else { Applied::Nothing }
            }
            event => self.apply_passing(current, event, now),
        }
    }

    /// The events that leave the note alone: interim text, status, gaps.
    /// Kept apart so that taking one in does not even borrow the note (and so
    /// does not mark the workspace as changed five times a second).
    fn apply_passing(&mut self, current: bool, event: Event, now: Instant) -> Applied {
        match event {
            Event::Final { .. }
            | Event::Speakers(_)
            | Event::Relabels(_)
            | Event::Gap { .. }
            | Event::Delivered(_) => Applied::Nothing,
            Event::Status(phase, behind) => {
                if !current || (self.phase, self.behind) == (phase, behind) {
                    return Applied::Nothing;
                }
                if self.phase != phase {
                    self.phase_since = Some(now);
                    // The server lets its recogniser session go after a long
                    // pause or a long loss of signal, and numbers the voices
                    // of the next one afresh.
                    if matches!(phase, Phase::Reconnecting | Phase::Paused | Phase::Connecting) {
                        self.spoke = [None, None];
                    }
                }
                self.phase = phase;
                self.behind = behind;
                Applied::Status
            }
            Event::Revision(revision) => {
                if current {
                    self.resync = Some((revision, now + RESYNC_AFTER));
                }
                Applied::Nothing
            }
            Event::Interim(mut interim) => {
                if !current || self.finished.contains(&interim.utterance_id) {
                    return Applied::Nothing;
                }
                if interim.text.chars().count() > MAX_INTERIM_CHARS {
                    // Keep the newest words.
                    let skip = interim.text.chars().count() - MAX_INTERIM_CHARS;
                    interim.text = format!("…{}", interim.text.chars().skip(skip).collect::<String>());
                }
                let at = self
                    .interim
                    .iter()
                    .position(|shown| shown.utterance_id == interim.utterance_id);
                match at {
                    Some(at) if interim.rev < self.interim[at].rev => return Applied::Nothing,
                    Some(at) if interim.text.trim().is_empty() => {
                        self.interim.remove(at);
                    }
                    Some(at) => {
                        if self.interim[at] == interim {
                            return Applied::Nothing;
                        }
                        self.interim[at] = interim;
                    }
                    None if interim.text.trim().is_empty() => return Applied::Nothing,
                    None => {
                        // The server keeps one unfinished utterance per
                        // channel: a new one means the old one is over, even
                        // if neither its final nor its withdrawal got here.
                        self.interim.retain(|shown| shown.channel != interim.channel);
                        let slot = self.next_slot();
                        self.fresh.push((interim.utterance_id.clone(), slot));
                        // In time order; a new line normally goes last.
                        let at = self
                            .interim
                            .iter()
                            .rposition(|shown| shown.start_ms <= interim.start_ms)
                            .map_or(0, |at| at + 1);
                        self.interim.insert(at, interim);
                        if self.interim.len() > MAX_INTERIM {
                            let dropped = self.interim.remove(0);
                            self.fresh.retain(|(id, _)| *id != dropped.utterance_id);
                        }
                        return Applied::Line(slot);
                    }
                }
                Applied::Interim
            }
            // The owner of the stream lets go of it.
            Event::Ended => Applied::Nothing,
        }
    }
}

/// Who has to draw again after an event.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Redraw {
    Nothing,
    /// Only where the live lines are on screen: the open transcript panel of
    /// that note, and the pill's popup.
    Lines,
    /// The window: the note or the recording's state changed.
    Window,
}

/// What applying an event changed.
#[derive(Debug, PartialEq)]
enum Applied {
    Nothing,
    /// Interim text changed: redraw.
    Interim,
    /// A new interim line appeared, to fade in on the given stage slot.
    Line(u64),
    /// The note gained a segment; the slot animates it firming up.
    Final(Option<u64>),
    /// Stored speakers were relabelled.
    Speakers,
    /// A stretch without transcript was added to the note.
    Gap,
    Status,
}

fn audio_channel(channel: Channel) -> AudioChannel {
    match channel {
        Channel::Mic => AudioChannel::Mic,
        Channel::System => AudioChannel::System,
    }
}

/// An interim line as a transcript row. Its identity is its draft speaker
/// when the provider names one, and otherwise `spoke`: whoever this stream
/// last stored a final for on that channel, so the bubble usually sits
/// exactly where its final text will. With neither it is nobody's yet: it
/// stands on its own, unnamed, rather than under the label of whoever spoke
/// last in an earlier session.
fn interim_row(
    interim: &Interim,
    spoke: Option<&(String, Option<String>)>,
) -> TranscriptUtterance {
    let channel = audio_channel(interim.channel);
    let own = interim.channel == Channel::Mic;
    let (speaker_id, label) = match (&interim.speaker_id, spoke) {
        (Some(id), _) => (id.clone(), interim.speaker_label.clone()),
        (None, Some((id, label))) => (id.clone(), label.clone()),
        // No stored speaker id is ever spelled like this.
        (None, None) => (format!("draft:{}", interim.utterance_id), None),
    };
    TranscriptUtterance {
        id: crate::lazy::key(("interim", &interim.utterance_id)),
        key: format!("{channel:?}:{speaker_id}"),
        speaker: label.filter(|label| !label.trim().is_empty()),
        part: None,
        own,
        source: if own { "Microphone" } else { "System audio" },
        timestamp: Some(format!(
            "{:02}:{:02}",
            interim.start_ms / 60_000,
            interim.start_ms / 1000 % 60
        )),
        text: interim.text.clone(),
        is_final: false,
    }
}

/// The copy-and-search text of one transcript row, as the note keeps it.
fn plain_row(row: &TranscriptUtterance) -> String {
    format!(
        "{}  {}\n{}",
        row.speaker.as_deref().unwrap_or("Unlabeled speaker"),
        row.timestamp.as_deref().unwrap_or("00:00"),
        row.text
    )
}

fn plain_text(segments: &[TranscriptSegmentDTO]) -> String {
    crate::note::server_transcript_utterances(segments)
        .iter()
        .map(plain_row)
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Store a final segment in the note, in time order. Stored text never
/// changes, so a segment the note already has is left alone. Returns whether
/// the note changed.
fn add_segment(note: &mut Note, segment: TranscriptSegmentDTO, revision: u64) -> bool {
    let known = note
        .server_transcript_segments
        .iter()
        .rev()
        .any(|stored| stored.id == segment.id);
    if !known {
        let at = note
            .server_transcript_segments
            .iter()
            .rposition(|stored| stored.start_ms <= segment.start_ms)
            .map_or(0, |at| at + 1);
        // Appending is the usual case and costs only the new text. Anything
        // else (a channel finishing an earlier sentence late, a note that so
        // far had only legacy plain text, a part-scoped speaker whose label
        // depends on the other parts) rebuilds it.
        let row = crate::note::server_transcript_utterances(std::slice::from_ref(&segment))
            .into_iter()
            .next();
        let append = at == note.server_transcript_segments.len()
            && (at > 0 || note.transcript.is_empty())
            && row.as_ref().is_none_or(|row| row.part.is_none());
        note.server_transcript_segments.insert(at, segment);
        match row {
            _ if !append => note.transcript = plain_text(&note.server_transcript_segments),
            Some(row) if note.transcript.is_empty() => note.transcript = plain_row(&row),
            Some(row) => {
                note.transcript.push_str("\n\n");
                note.transcript.push_str(&plain_row(&row));
            }
            None => {} // An empty segment adds no text.
        }
    }
    if !known || revision > note.transcript_revision {
        note.normalize_generation_revision();
        note.transcript_revision = note.transcript_revision.max(revision);
    }
    !known
}

/// The label the server gives speaker `id` when no segment says it: on the
/// microphone `mic:n:1` is "Me" and `mic:n:<k>` is "Room speaker <k - 1>"
/// (other people in the room); on system audio `system:n:<n>` is
/// "Speaker <n>".
fn speaker_label_of(id: &str) -> Option<String> {
    let mut parts = id.split(':');
    let (channel, kind, number) = (parts.next()?, parts.next()?, parts.next()?);
    if kind != "n" || parts.next().is_some() {
        return None;
    }
    let number: u32 = number.parse().ok().filter(|_| number.chars().all(|c| c.is_ascii_digit()))?;
    match (channel, number) {
        ("mic", 1) => Some("Me".into()),
        ("mic", k) if k > 1 => Some(format!("Room speaker {}", k - 1)),
        ("system", n) => Some(format!("Speaker {n}")),
        _ => None,
    }
}

/// The provider decided two of its speakers are one: stored segments of
/// `from` now belong to `to`. Text and times are untouched.
fn merge_speakers(note: &mut Note, merges: &[SpeakerMerge]) -> bool {
    let mut changed = false;
    for merge in merges {
        let label = note
            .server_transcript_segments
            .iter()
            .find(|segment| segment.speaker_id == merge.to)
            .map(|segment| segment.speaker_label.clone())
            .unwrap_or_else(|| speaker_label_of(&merge.to));
        for segment in &mut note.server_transcript_segments {
            if segment.speaker_id == merge.from {
                segment.speaker_id = merge.to.clone();
                segment.speaker_label = label.clone();
                changed = true;
            }
        }
    }
    if changed {
        note.transcript = plain_text(&note.server_transcript_segments);
    }
    changed
}

/// A note-timeline position as the transcript shows times: minutes and
/// seconds.
pub(crate) fn clock(ms: u64) -> String {
    format!("{:02}:{:02}", ms / 60_000, ms / 1000 % 60)
}

/// A stretch of the note's timeline, "12:03–12:10".
pub(crate) fn gap_span(from_ms: u64, to_ms: u64) -> String {
    format!("{}–{}", clock(from_ms), clock(to_ms.max(from_ms)))
}

/// The server moved stored segments to another speaker, by segment id
/// (after any merges). Text and times are untouched.
fn relabel_speakers(note: &mut Note, relabels: &[SpeakerRelabel]) -> bool {
    let mut changed = false;
    for relabel in relabels {
        for segment in &mut note.server_transcript_segments {
            if segment.id == relabel.segment_id
                && (segment.speaker_id != relabel.speaker_id
                    || segment.speaker_label.as_deref() != Some(relabel.speaker_label.as_str()))
            {
                segment.speaker_id = relabel.speaker_id.clone();
                segment.speaker_label = Some(relabel.speaker_label.clone());
                changed = true;
            }
        }
    }
    if changed {
        note.transcript = plain_text(&note.server_transcript_segments);
    }
    changed
}

/// The captures of `note` that have not all reached the server, in the
/// order they were recorded.
pub(crate) fn undelivered_captures(note: &Note) -> Vec<crate::audio::CaptureManifest> {
    note.audio_captures
        .iter()
        .filter(|capture| note.live_pending.contains(&capture.capture_id))
        .cloned()
        .collect()
}

/// The Stop that is due for `note` now: one is pending, and every capture
/// has been delivered (`delivering` is whether a stream of it still owes
/// the server audio).
pub(crate) fn stop_due(note: &Note, delivering: bool) -> Option<PendingStop> {
    if delivering || !note.live_pending.is_empty() {
        return None;
    }
    note.pending_stop
}

impl Cereal {
    /// A capture has started for note `id`. Its stream is started at once
    /// (`live_connect`) and opens its socket the moment the server has the
    /// note recording (see `server::RecordingSignal`).
    pub fn live_prepare(&mut self, id: u64, monitor: crate::audio::CaptureMonitor) {
        self.live_end();
        self.live.begin(id);
        self.live.capture = Some(Pending {
            note: id,
            capture_id: monitor.snapshot().capture_id,
            monitor,
            tried: false,
        });
    }

    /// Follow the capture of note `id`, as soon as it has started here.
    /// Started once per recording; while live transcription is unavailable
    /// the stream keeps trying for as long as the recording runs.
    pub fn live_connect(&mut self, id: u64, cx: &mut Context<Self>) {
        crate::latency::mark("live_connect");
        if self.recording.note_id != Some(id) || !self.recording.is_active() {
            return;
        }
        let Some(pending) = self
            .live
            .capture
            .as_mut()
            .filter(|pending| pending.note == id && !pending.tried)
        else {
            return;
        };
        let Some(note) = self.workspace.note(id) else {
            return;
        };
        pending.tried = true;
        // Only what finding the stream's target needs, not the note's text.
        let note = Note {
            id,
            server_id: note.server_id.clone(),
            server_url: note.server_url.clone(),
            audio_captures: note.audio_captures.clone(),
            ..Note::default()
        };
        let capture_id = pending.capture_id.clone();
        let pending_capture = capture_id.clone();
        // The stream belongs to the note's account: it stops when that
        // account's epoch moves (its session ended or was replaced, or it
        // was signed out of). The capture itself is local and carries on.
        let account = crate::accounts::note_epoch(id);
        let session = Session::start(
            id,
            Source::Monitor(pending.monitor.clone()),
            move || crate::server::live_target_started(&note, &capture_id),
            move || crate::accounts::note_epoch(id) == account,
        );
        self.live.streams += 1;
        self.live.session = Some(Stream {
            id: self.live.streams,
            note: id,
            capture_id: pending_capture,
            session,
            delivered: None,
        });
        self.live_watch(cx);
    }

    /// The recording stopped (or its owner changed): let the stream send the
    /// rest of the capture and collect its last finals, in the background.
    pub fn live_end(&mut self) {
        self.live.capture = None;
        if let Some(stream) = self.live.session.take() {
            stream.session.end();
            self.live.ending.push(stream);
        }
        if self.live.phase != Phase::Off {
            self.live.phase = Phase::Off;
            self.live.behind = 0;
            self.live.revision = self.live.revision.wrapping_add(1);
        }
    }

    /// The recording of note `id` stopped here (Stop, a new note, the app
    /// quitting): the capture stops at once, and the server is told so only
    /// once the stream has delivered every captured sample, however long the
    /// backlog (see [`Cereal::live_deliver`]). A capture whose stream never
    /// got going, or gave up, is streamed now from its monitor. Until then the note says
    /// "Finishing the transcript…". `stop` is what is owed then; a pending
    /// Generate is never reduced to a plain Stop.
    pub fn live_stop(&mut self, id: u64, stop: PendingStop, cx: &mut Context<Self>) {
        let current = self
            .live
            .capture
            .as_ref()
            .filter(|pending| pending.note == id)
            .map(|pending| (pending.capture_id.clone(), pending.monitor.clone()));
        self.live_end();
        if !crate::server::enabled() {
            return;
        }
        if let Some(note) = self.workspace.note_mut(id) {
            note.pending_stop = Some(match (note.pending_stop, stop) {
                (Some(PendingStop::Generate), _) | (_, PendingStop::Generate) => PendingStop::Generate,
                _ => PendingStop::Stop,
            });
        }
        self.live_deliver(id, current, cx);
    }

    /// Stream every capture of note `id` that has not all reached the
    /// server and has no stream yet, each from where the server is, then
    /// tell the server the pending Stop once nothing is owed. Used after
    /// Stop (`current` is the capture that has just stopped, if its stream
    /// never got going), and when the note's account is ready, at launch
    /// among others: a quit or a crash before the delivery finished is
    /// finished then, from the files on disk.
    pub fn live_deliver(
        &mut self,
        id: u64,
        current: Option<(String, crate::audio::CaptureMonitor)>,
        cx: &mut Context<Self>,
    ) {
        if !crate::server::enabled() {
            return;
        }
        let Some(note) = self.workspace.note(id) else {
            return;
        };
        let running: Vec<String> = self
            .live
            .session
            .iter()
            .chain(&self.live.ending)
            .filter(|stream| stream.note == id)
            .map(|stream| stream.capture_id.clone())
            .collect();
        let captures: Vec<_> = undelivered_captures(note)
            .into_iter()
            .filter(|capture| !running.contains(&capture.capture_id))
            .collect();
        let target = Note {
            id,
            server_id: note.server_id.clone(),
            server_url: note.server_url.clone(),
            audio_captures: note.audio_captures.clone(),
            ..Note::default()
        };
        let started = !captures.is_empty();
        for capture in captures {
            let capture_id = capture.capture_id.clone();
            let source = match &current {
                // Still being finalised by this launch's writer.
                Some((current, monitor)) if *current == capture_id => Source::Monitor(monitor.clone()),
                _ => Source::Saved(capture),
            };
            let account = crate::accounts::note_epoch(id);
            let target = target.clone();
            let resolved = capture_id.clone();
            let session = Session::start(
                id,
                source,
                move || crate::server::live_target(&target, &resolved),
                move || crate::accounts::note_epoch(id) == account,
            );
            session.end();
            self.live.streams += 1;
            self.live.ending.push(Stream {
                id: self.live.streams,
                note: id,
                capture_id,
                session,
                delivered: None,
            });
        }
        if started {
            self.live_watch(cx);
            cx.notify();
        }
        self.live_stop_if_delivered(id, cx);
    }

    /// Send the pending Stop of note `id` if nothing is owed any more.
    pub fn live_stop_if_delivered(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(stop) = self
            .workspace
            .note(id)
            .and_then(|note| stop_due(note, self.live.delivering(id)))
        else {
            return;
        };
        if self.recording.note_id == Some(id) && self.recording.is_active() {
            return; // Recording again: the Stop is for later.
        }
        if self.live.stopping.contains(&id) {
            return;
        }
        let action = match stop {
            PendingStop::Generate => crate::server::Action::Generate,
            PendingStop::Stop => crate::server::Action::Recording("stop"),
        };
        if self.server_action_sent(id, action, cx) {
            self.live.stopping.insert(id);
        }
    }

    /// Capture `capture_id` has finished on its own (a device failed, the
    /// file limit was reached): its stream ends like one that was stopped.
    pub fn live_capture_finished(&mut self, capture_id: &str) {
        if self
            .live
            .capture
            .as_ref()
            .is_some_and(|pending| pending.capture_id == capture_id)
        {
            self.live_end();
        }
    }

    /// The recording was put on hold or taken off it.
    pub fn live_hold(&mut self, held: bool) {
        if let Some(stream) = &self.live.session {
            if held {
                stream.session.pause();
            } else {
                stream.session.resume();
            }
        }
    }

    /// Look at the streams once a frame (60 times a second) while there are
    /// any, so text is on screen the frame after it arrives. The update does
    /// nothing, and draws nothing, when nothing has arrived.
    fn live_watch(&mut self, cx: &mut Context<Self>) {
        if std::mem::replace(&mut self.live.watching, true) {
            return;
        }
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;
                let Ok(keep) = this.update(cx, |this, cx| this.live_drain(cx)) else {
                    break;
                };
                if !keep {
                    break;
                }
            }
        })
        .detach();
    }

    /// Apply everything the streams have reported. Returns false once there
    /// is nothing left to watch.
    fn live_drain(&mut self, cx: &mut Context<Self>) -> bool {
        let mut events = Vec::new();
        let streams = self
            .live
            .session
            .iter()
            .map(|stream| (stream, true))
            .chain(self.live.ending.iter().map(|stream| (stream, false)));
        for (stream, current) in streams {
            events.extend(
                stream
                    .session
                    .events
                    .try_iter()
                    .map(|event| (stream.id, stream.note, current, event)),
            );
        }
        let mut redraw = Redraw::Nothing;
        let mut settled = Vec::new();
        for (stream, note_id, current, event) in events {
            redraw = redraw.max(match event {
                Event::Ended => {
                    settled.push(note_id);
                    self.live_ended(stream, note_id)
                }
                Event::Delivered(delivery) => {
                    settled.push(note_id);
                    self.live_delivered(stream, note_id, delivery, cx)
                }
                event => self.live_event(note_id, current, event, cx),
            });
        }
        // A Stop that was waiting for these streams goes out now.
        settled.dedup();
        for note in settled {
            self.live_stop_if_delivered(note, cx);
        }
        // Text the stream stored while this client was not listening, and
        // could not re-send: the ordinary save reads the note back.
        let behind = self
            .live
            .session
            .as_ref()
            .map(|stream| stream.note)
            .filter(|note| {
                self.workspace
                    .note(*note)
                    .is_some_and(|note| self.live.resync_due(note, Instant::now()))
            });
        if let Some(note) = behind {
            self.server_action(note, crate::server::Action::Save, cx);
        }
        self.live_redraw(redraw, cx);
        let keep = self.live.session.is_some()
            || !self.live.ending.is_empty()
            || matches!(self.live.phase, Phase::Reconnecting | Phase::Connecting);
        if !keep {
            self.live.watching = false;
        }
        keep
    }

    /// Draw what changed, and no more than that. Interim text arrives several
    /// times a second for the whole meeting; it redraws the window only while
    /// the transcript it belongs to is open there. Otherwise only the pill's
    /// panels are told, and they redraw only if they show it.
    fn live_redraw(&mut self, mut redraw: Redraw, cx: &mut Context<Self>) {
        // What the status line says can change with time alone: a reconnect
        // that has lasted long enough to mention, a notice that has expired.
        let status = self.live.status_at(Instant::now());
        if status != self.live.shown {
            if status.is_some() {
                self.live.said = status.clone();
            }
            self.live.shown = status;
            redraw = Redraw::Window;
        }
        if redraw == Redraw::Nothing {
            return;
        }
        self.live.revision = self.live.revision.wrapping_add(1);
        let on_screen = self.transcript_open && self.current_note == self.live.note;
        if redraw == Redraw::Window || on_screen {
            cx.notify();
        } else {
            crate::pill::refresh(cx);
        }
    }

    /// A stream said what came of its capture: every sample is on the
    /// server, or the server will take no more of it. Either way nothing is
    /// owed for it any more.
    fn live_delivered(&mut self, stream: u64, note_id: u64, delivery: Delivery, cx: &mut Context<Self>) -> Redraw {
        let mut capture = None;
        for open in self.live.session.iter_mut().chain(self.live.ending.iter_mut()) {
            if open.id == stream {
                open.delivered = Some(delivery);
                capture = Some(open.capture_id.clone());
            }
        }
        let Some(capture) = capture else {
            return Redraw::Nothing;
        };
        if let Some(note) = self.workspace.note_mut(note_id) {
            note.live_pending.retain(|pending| *pending != capture);
        }
        self.persist(cx);
        Redraw::Window
    }

    /// A stream's thread has finished: let go of it, and of interim text
    /// that no stream is left to finalise.
    fn live_ended(&mut self, stream: u64, note_id: u64) -> Redraw {
        if self.live.session.as_ref().is_some_and(|open| open.id == stream) {
            self.live.session = None;
        }
        self.live.ending.retain(|open| open.id != stream);
        let orphans = self.live.note == Some(note_id) && !self.live.streams(note_id);
        if orphans && !self.live.interim.is_empty() {
            self.live.interim.clear();
            self.live.fresh.clear();
            return Redraw::Lines;
        }
        Redraw::Nothing
    }

    /// One event of a stream (or of a scripted preview).
    pub(crate) fn live_event(
        &mut self,
        note_id: u64,
        current: bool,
        event: Event,
        cx: &mut Context<Self>,
    ) -> Redraw {
        let now = Instant::now();
        let applied = if matches!(
            event,
            Event::Final { .. } | Event::Speakers(_) | Event::Relabels(_) | Event::Gap { .. }
        ) {
            match self.workspace.note_mut(note_id) {
                Some(note) => self.live.apply(note, current, event, now),
                None => Applied::Nothing,
            }
        } else if self.workspace.note(note_id).is_some() {
            self.live.apply_passing(current, event, now)
        } else {
            Applied::Nothing
        };
        if matches!(applied, Applied::Interim | Applied::Line(_)) {
            crate::latency::mark("interim_applied");
            crate::latency::on_paint("interim_painted");
        }
        if matches!(applied, Applied::Final(_)) {
            crate::latency::mark_each("final_applied");
        }
        match applied {
            Applied::Nothing => Redraw::Nothing,
            Applied::Interim => Redraw::Lines,
            Applied::Line(slot) => {
                self.line_from(slot, 0.);
                Redraw::Lines
            }
            Applied::Final(slot) => {
                if let Some(slot) = slot {
                    self.line_from(slot, 0.);
                }
                self.live_save_later(cx);
                Redraw::Window
            }
            Applied::Speakers => {
                self.forget_utterances();
                self.live_save_later(cx);
                Redraw::Window
            }
            Applied::Gap => {
                self.forget_utterances();
                self.live_save_later(cx);
                Redraw::Window
            }
            Applied::Status => {
                let warning = if self.live.phase == Phase::ProviderUnavailable {
                    TranscriptionAttention::Warning
                } else {
                    TranscriptionAttention::None
                };
                if self.recording.note_id == Some(note_id) {
                    self.recording.set_transcription_attention(warning);
                }
                Redraw::Window
            }
        }
    }

    /// Start a line's transition at `from` (0 is where it begins, 1 where
    /// it rests); the transcript view reads the track as it eases to 1.
    fn line_from(&self, slot: u64, from: f32) {
        let key = (LINE_TRACK, slot);
        self.stage.forget(key);
        self.stage.aim(key, from, crate::motion::SNAPPY);
        self.stage.aim(key, 1., crate::motion::SNAPPY);
    }

    /// Save the text that has arrived, at most once per [`SAVE_EVERY`].
    /// Stopping the recording saves at once, as it always has.
    fn live_save_later(&mut self, cx: &mut Context<Self>) {
        if std::mem::replace(&mut self.live.save_armed, true) {
            return;
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SAVE_EVERY).await;
            let _ = this.update(cx, |this, cx| {
                this.live.save_armed = false;
                this.schedule_save(cx);
            });
        })
        .detach();
    }

    /// Scripted stream events for the tour, the benchmark and screenshots:
    /// the same path the socket's events take, with no socket.
    pub fn live_preview(&mut self, note_id: u64, events: Vec<Event>, cx: &mut Context<Self>) {
        if self.live.note != Some(note_id) {
            self.live.begin(note_id);
        }
        let mut redraw = Redraw::Nothing;
        for event in events {
            redraw = redraw.max(self.live_event(note_id, true, event, cx));
        }
        self.live_redraw(redraw, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // `gpui::*` re-exports its own `test` attribute; these are plain unit tests.
    use std::prelude::v1::test;

    fn segment(id: &str, channel: AudioChannel, speaker: u32, start_ms: i64, text: &str) -> TranscriptSegmentDTO {
        let name = if channel == AudioChannel::Mic { "mic" } else { "system" };
        TranscriptSegmentDTO {
            channel,
            end_ms: Some(start_ms + 1500),
            id: id.into(),
            is_final: true,
            speaker_id: format!("{name}:n:{speaker}"),
            speaker_label: Some(format!("Speaker {speaker}")),
            start_ms,
            text: text.into(),
        }
    }

    fn interim(id: &str, rev: u64, text: &str) -> Event {
        Event::Interim(Interim {
            channel: Channel::System,
            utterance_id: id.into(),
            rev,
            text: text.into(),
            start_ms: 65_000,
            speaker_id: None,
            speaker_label: None,
        })
    }

    fn finalise(utterance: &str, revision: u64, segment: TranscriptSegmentDTO) -> Event {
        Event::Final {
            utterance_id: utterance.into(),
            revision,
            segment,
        }
    }

    fn live(note: &Note) -> LiveTranscript {
        let mut live = LiveTranscript::default();
        live.begin(note.id);
        live
    }

    #[test]
    fn interim_text_is_replaced_in_place_and_never_reaches_the_note() {
        let mut note = Note { id: 7, ..Note::default() };
        let mut live = live(&note);
        let now = Instant::now();
        assert!(matches!(live.apply(&mut note, true, interim("u1", 0, "so if"), now), Applied::Line(..)));
        assert_eq!(live.apply(&mut note, true, interim("u1", 1, "so if we move"), now), Applied::Interim);
        // An older revision arriving late, and a repeat, change nothing.
        assert_eq!(live.apply(&mut note, true, interim("u1", 0, "so"), now), Applied::Nothing);
        assert_eq!(live.apply(&mut note, true, interim("u1", 1, "so if we move"), now), Applied::Nothing);
        live.revision += 1;
        let tail = live.tail(&note);
        assert_eq!(tail.len(), 1);
        assert_eq!((tail[0].text.as_str(), tail[0].is_final, tail[0].own), ("so if we move", false, false));
        assert_eq!(tail[0].timestamp.as_deref(), Some("01:05"));
        // Nothing of it is in the note: not its text, its segments or its revision.
        assert!(note.transcript.is_empty() && note.server_transcript_segments.is_empty());
        assert_eq!(note.transcript_revision, 0);
        assert!(!serde_json::to_string(&note).unwrap().contains("so if"));
        // Another note's transcript shows none of it.
        assert!(live.tail(&Note { id: 8, ..Note::default() }).is_empty());
    }

    #[test]
    fn a_final_replaces_its_interim_and_becomes_a_stored_segment_once() {
        let mut note = Note { id: 7, ..Note::default() };
        let mut live = live(&note);
        let now = Instant::now();
        live.apply(&mut note, true, interim("u1", 0, "so if we move the"), now);
        let stored = segment("s1", AudioChannel::System, 3, 65_000, "So if we move the pilot to May.");
        assert!(matches!(
            live.apply(&mut note, true, finalise("u1", 42, stored.clone()), now),
            Applied::Final(Some(_))
        ));
        assert!(live.interim.is_empty());
        assert_eq!(note.server_transcript_segments, [stored.clone()]);
        assert_eq!(note.transcript, "Speaker 3  01:05\nSo if we move the pilot to May.");
        assert_eq!(note.transcript_revision, 42);
        assert_eq!(live.firmed_slot("s1"), live.firmed.first().map(|(_, slot)| *slot));
        // The final knows the line it took over from, and what that showed.
        assert_eq!(
            live.replaced("s1"),
            Some((crate::lazy::key(("interim", "u1")), "so if we move the"))
        );
        // The same segment again (a resend after a reconnect) adds nothing.
        assert_eq!(live.apply(&mut note, true, finalise("u1", 42, stored), now), Applied::Nothing);
        assert_eq!(note.server_transcript_segments.len(), 1);
        // A late interim for the finished utterance is ignored.
        assert_eq!(live.apply(&mut note, true, interim("u1", 9, "so if we move the pilot"), now), Applied::Nothing);
        // A second turn of the same utterance is one more segment.
        let turn = segment("s2", AudioChannel::System, 4, 67_000, "May works.");
        assert_eq!(live.apply(&mut note, true, finalise("u1", 43, turn), now), Applied::Final(None));
        assert_eq!(
            note.transcript,
            "Speaker 3  01:05\nSo if we move the pilot to May.\n\nSpeaker 4  01:07\nMay works."
        );
        assert_eq!(note.transcript, plain_text(&note.server_transcript_segments));
        assert_eq!(note.transcript_revision, 43);
    }

    #[test]
    fn words_after_resume_never_stand_under_a_label_from_the_session_before() {
        // The first recording of the note stored one remote voice.
        let mut note = Note { id: 7, ..Note::default() };
        let mut live = live(&note);
        let now = Instant::now();
        live.apply(&mut note, true, finalise("u0", 1, segment("s0", AudioChannel::System, 1, 60_000, "See you after lunch.")), now);
        live.apply(&mut note, true, interim("u1", 0, "and one more"), now);
        live.revision += 1;
        // Within a session an unnamed interim line continues whoever spoke last.
        let stored = crate::note::server_transcript_utterances(&note.server_transcript_segments);
        assert_eq!(live.tail(&note)[0].key, stored[0].key);
        // Resume: a new session. Someone else may be talking, and the line
        // names nobody, so it belongs to nobody: not joined to the bubble
        // above and not under its label.
        live.begin(note.id);
        live.apply(&mut note, true, interim("u2", 0, "hello again"), now);
        live.revision += 1;
        let tail = live.tail(&note);
        assert_ne!(tail[0].key, stored[0].key);
        assert_eq!(tail[0].speaker, None);
        assert_eq!(crate::note::transcript_headings(&stored, &tail), [None, None]);
        // The server numbers the new session's voice on from the note's
        // highest, so its stored text is "Speaker 2" and both are named.
        live.apply(&mut note, true, finalise("u2", 2, segment("s2", AudioChannel::System, 2, 120_000, "Hello again.")), now);
        let stored = crate::note::server_transcript_utterances(&note.server_transcript_segments);
        assert_eq!(
            crate::note::transcript_headings(&stored, &[]),
            [Some("Speaker 1".to_string()), Some("Speaker 2".to_string())]
        );
        // From here an unnamed line continues this session's voice,
        live.apply(&mut note, true, interim("u3", 0, "as I said"), now);
        live.revision += 1;
        assert_eq!(live.tail(&note)[0].key, stored[1].key);
        // until a pause or a lost connection, after which the server may
        // number voices afresh.
        for phase in [Phase::Paused, Phase::Reconnecting] {
            live.apply(&mut note, true, Event::Status(Phase::Live, 0), now);
            live.apply(&mut note, true, finalise("u3", 3, segment("s3", AudioChannel::System, 2, 125_000, "As I said.")), now);
            live.apply(&mut note, true, Event::Status(phase, 0), now);
            live.apply(&mut note, true, interim("u4", 0, "right"), now);
            live.revision += 1;
            assert!(live.tail(&note)[0].key.contains("draft:"), "{phase:?}");
            live.apply(&mut note, true, interim("u4", 1, ""), now);
        }
    }

    #[test]
    fn segments_stay_in_time_order_when_channels_finish_out_of_turn() {
        let mut note = Note { id: 7, ..Note::default() };
        let mut live = live(&note);
        let now = Instant::now();
        for (id, channel, start) in [
            ("a", AudioChannel::Mic, 1_000),
            ("c", AudioChannel::Mic, 9_000),
            // The other channel finishes an earlier sentence afterwards.
            ("b", AudioChannel::System, 4_000),
            ("d", AudioChannel::System, 9_000),
        ] {
            let stored = segment(id, channel, 1, start, &format!("text {id}"));
            live.apply(&mut note, true, finalise(id, 1, stored), now);
        }
        let order: Vec<_> = note.server_transcript_segments.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(order, ["a", "b", "c", "d"]);
        assert_eq!(note.transcript, plain_text(&note.server_transcript_segments));
    }

    #[test]
    fn an_interim_line_sits_with_the_speaker_its_final_text_will_have() {
        let mut note = Note { id: 7, ..Note::default() };
        let mut live = live(&note);
        let now = Instant::now();
        live.apply(&mut note, true, finalise("u0", 1, segment("s0", AudioChannel::System, 3, 60_000, "Before.")), now);
        live.apply(&mut note, true, interim("u1", 0, "and then"), now);
        live.revision += 1;
        let finals = crate::note::server_transcript_utterances(&note.server_transcript_segments);
        // No draft speaker: it continues the channel's last stored speaker.
        assert_eq!(live.tail(&note)[0].key, finals[0].key);
        assert_eq!(live.tail(&note)[0].speaker.as_deref(), Some("Speaker 3"));
        // A draft speaker from the provider wins.
        live.apply(
            &mut note,
            true,
            Event::Interim(Interim {
                channel: Channel::System,
                utterance_id: "u1".into(),
                rev: 1,
                text: "and then we".into(),
                start_ms: 65_000,
                speaker_id: Some("system:n:5".into()),
                speaker_label: Some("Speaker 5".into()),
            }),
            now,
        );
        live.revision += 1;
        assert_eq!(live.tail(&note)[0].key, "System:system:n:5");
        assert_eq!(live.tail(&note)[0].speaker.as_deref(), Some("Speaker 5"));
    }

    #[test]
    fn interim_lines_are_bounded_however_long_the_meeting_runs() {
        let mut note = Note { id: 7, ..Note::default() };
        let mut live = live(&note);
        let now = Instant::now();
        // An hour of interim updates at five a second, an utterance every
        // four seconds, none of them ever finalised.
        for update in 0..18_000u64 {
            let utterance = format!("u{}", update / 20);
            let text = "word ".repeat((update % 20 + 1) as usize * 40);
            live.apply(&mut note, true, interim(&utterance, update, &text), now);
            assert!(live.interim.len() <= MAX_INTERIM);
            assert!(live.fresh.len() <= LINE_SLOTS as usize);
            assert!(live.finished.len() <= 32);
        }
        assert!(live.interim.iter().all(|line| line.text.chars().count() <= MAX_INTERIM_CHARS + 1));
        assert!(note.transcript.is_empty() && note.server_transcript_segments.is_empty());
    }

    #[test]
    fn status_is_silent_while_working_and_quiet_about_trouble() {
        let mut note = Note { id: 7, ..Note::default() };
        let mut live = live(&note);
        let start = Instant::now();
        assert_eq!(live.status(7), None);
        for (phase, behind) in [(Phase::Connecting, 0), (Phase::Live, 0), (Phase::Paused, 0)] {
            live.apply(&mut note, true, Event::Status(phase, behind), start);
            assert_eq!(live.status_at(start), None, "{phase:?}");
        }
        live.apply(&mut note, true, Event::Status(Phase::CatchingUp, 0), start);
        assert_eq!(live.status_at(start).unwrap().text, "Catching up…");
        live.apply(&mut note, true, Event::Status(Phase::CatchingUp, 15), start);
        let status = live.status_at(start).unwrap();
        assert_eq!((status.text.as_ref(), status.warning), ("Transcript about 15 s behind", false));
        // A reconnect is mentioned only once it has lasted.
        live.apply(&mut note, true, Event::Status(Phase::Reconnecting, 0), start);
        assert_eq!(live.status_at(start + Duration::from_millis(500)), None);
        assert!(live.status_at(start + Duration::from_secs(2)).unwrap().text.starts_with("Reconnecting"));
        // The provider being unavailable is the one warning.
        assert_eq!(live.apply(&mut note, true, Event::Status(Phase::ProviderUnavailable, 0), start), Applied::Status);
        let status = live.status_at(start).unwrap();
        assert!(status.warning && status.text.starts_with("Live transcript unavailable"));
        // A stretch that could not be transcribed is said, and stays said
        // (see `a_skipped_stretch_is_kept_with_the_note_and_said_until_another_recording`).
        live.apply(&mut note, true, Event::Status(Phase::Live, 0), start);
        live.apply(&mut note, true, Event::Gap { channel: Channel::Mic, from_ms: 1_000, to_ms: 9_000 }, start);
        assert!(live.status_at(start + Duration::from_secs(1)).unwrap().text.starts_with("Couldn't transcribe"));
        assert!(live.status_at(start + Duration::from_secs(9)).is_some());
        // Another note never shows this one's status.
        assert_eq!(live.status(8), None);
    }

    #[test]
    fn a_stream_that_is_finishing_still_delivers_finals_but_no_interim_or_status() {
        let mut note = Note { id: 7, ..Note::default() };
        let mut live = live(&note);
        let now = Instant::now();
        assert_eq!(live.apply(&mut note, false, interim("u1", 0, "late"), now), Applied::Nothing);
        assert_eq!(live.apply(&mut note, false, Event::Status(Phase::CatchingUp, 9), now), Applied::Nothing);
        let stored = segment("s1", AudioChannel::Mic, 1, 1_000, "The last words.");
        assert_eq!(live.apply(&mut note, false, finalise("u1", 5, stored), now), Applied::Final(None));
        assert_eq!(note.server_transcript_segments.len(), 1);
        assert_eq!(live.status(7), None);
    }

    #[test]
    fn merged_speakers_relabel_stored_segments_without_touching_their_text() {
        let mut note = Note { id: 7, ..Note::default() };
        let mut live = live(&note);
        let now = Instant::now();
        for (id, speaker, start) in [("a", 2, 1_000), ("b", 4, 3_000), ("c", 2, 5_000)] {
            live.apply(&mut note, true, finalise(id, 1, segment(id, AudioChannel::System, speaker, start, id)), now);
        }
        let merge = Event::Speakers(vec![SpeakerMerge { from: "system:n:4".into(), to: "system:n:2".into() }]);
        assert_eq!(live.apply(&mut note, true, merge.clone(), now), Applied::Speakers);
        assert!(note.server_transcript_segments.iter().all(|s| s.speaker_id == "system:n:2"));
        assert!(note.server_transcript_segments.iter().all(|s| s.speaker_label.as_deref() == Some("Speaker 2")));
        assert_eq!(note.server_transcript_segments[1].text, "b");
        assert_eq!(note.transcript, plain_text(&note.server_transcript_segments));
        assert_eq!(live.apply(&mut note, true, merge, now), Applied::Nothing);
    }

    /// A voice merged into one no stored segment names yet is labelled as the
    /// server labels it: the microphone's own voice is "Me", never "Speaker 1".
    #[test]
    fn a_voice_merged_into_the_microphone_s_own_is_me() {
        let mut note = Note { id: 7, ..Note::default() };
        let mut live = live(&note);
        let now = Instant::now();
        live.apply(&mut note, true, finalise("a", 1, segment("a", AudioChannel::Mic, 2, 1_000, "a")), now);
        let merge = Event::Speakers(vec![SpeakerMerge { from: "mic:n:2".into(), to: "mic:n:1".into() }]);
        assert_eq!(live.apply(&mut note, true, merge, now), Applied::Speakers);
        assert_eq!(note.server_transcript_segments[0].speaker_id, "mic:n:1");
        assert_eq!(note.server_transcript_segments[0].speaker_label.as_deref(), Some("Me"));
        // Other voices in the room, and on system audio, as the server
        // names them.
        let merge = Event::Speakers(vec![SpeakerMerge { from: "mic:n:1".into(), to: "mic:n:3".into() }]);
        live.apply(&mut note, true, merge, now);
        assert_eq!(note.server_transcript_segments[0].speaker_label.as_deref(), Some("Room speaker 2"));
        assert_eq!(speaker_label_of("system:n:4").as_deref(), Some("Speaker 4"));
        assert_eq!(speaker_label_of("mic:n:0"), None);
        assert_eq!(speaker_label_of("mic:3f1c:speaker_1"), None);
    }

    #[test]
    fn a_reconnect_reads_the_note_again_only_if_resent_finals_leave_it_behind() {
        let mut note = Note { id: 7, transcript_revision: 5, ..Note::default() };
        let mut live = live(&note);
        let now = Instant::now();
        // The socket is ready at revision 7: two finals were stored meanwhile.
        assert_eq!(live.apply(&mut note, true, Event::Revision(7), now), Applied::Nothing);
        // Not judged yet: the server is about to re-send them.
        assert!(!live.resync_due(&note, now + Duration::from_millis(200)));
        live.apply(&mut note, true, finalise("u1", 6, segment("s1", AudioChannel::Mic, 1, 1_000, "One.")), now);
        live.apply(&mut note, true, finalise("u2", 7, segment("s2", AudioChannel::Mic, 1, 3_000, "Two.")), now);
        // They arrived: nothing to read again.
        assert!(!live.resync_due(&note, now + RESYNC_AFTER));
        // Next time one of them was written into a dead connection.
        live.apply(&mut note, true, Event::Revision(9), now);
        live.apply(&mut note, true, finalise("u3", 8, segment("s3", AudioChannel::Mic, 1, 5_000, "Three.")), now);
        assert!(live.resync_due(&note, now + RESYNC_AFTER));
        // Asked once.
        assert!(!live.resync_due(&note, now + RESYNC_AFTER * 2));
        // A stream that is finishing does not ask at all.
        live.apply(&mut note, false, Event::Revision(99), now);
        assert!(!live.resync_due(&note, now + RESYNC_AFTER * 3));
    }

    #[test]
    fn a_skipped_stretch_is_kept_with_the_note_and_said_until_another_recording() {
        use meeting_notes_api_types::TranscriptGapDTO;
        let mut note = Note { id: 7, ..Note::default() };
        let mut live = live(&note);
        let now = Instant::now();
        let gap = |from_ms, to_ms| Event::Gap { channel: Channel::Mic, from_ms, to_ms };
        assert_eq!(live.apply(&mut note, true, gap(723_000, 730_000), now), Applied::Gap);
        // The server says the skip the stream made: one stretch.
        live.apply(&mut note, true, gap(723_000, 730_000), now);
        let stretch = TranscriptGapDTO { channel: AudioChannel::Mic, from_ms: 723_000, to_ms: 730_000 };
        assert_eq!(note.transcript_gaps, [stretch.clone()]);
        // Said for as long as this recording's state is shown, not for 8 s.
        let said = |live: &LiveTranscript, at: Duration| live.status_of(7, now + at).map(|status| status.text.to_string());
        assert_eq!(said(&live, Duration::from_secs(3_600)).as_deref(), Some("Couldn't transcribe 12:03–12:10"));
        // While nothing reaches the server, that is said first, with where
        // the audio is.
        live.apply(&mut note, true, Event::Status(Phase::Reconnecting, 0), now);
        assert_eq!(said(&live, Duration::from_secs(2)).as_deref(), Some("Reconnecting · audio is saved on this Mac"));
        live.apply(&mut note, true, Event::Status(Phase::ProviderUnavailable, 0), now);
        assert_eq!(said(&live, Duration::ZERO).as_deref(), Some("Live transcript unavailable · audio is saved on this Mac"));
        live.apply(&mut note, true, Event::Status(Phase::Live, 0), now);
        assert_eq!(said(&live, Duration::ZERO).as_deref(), Some("Couldn't transcribe 12:03–12:10"));
        // A short stretch is kept but not said; one that touches the last is
        // joined to it.
        live.apply(&mut note, true, gap(731_000, 731_400), now);
        live.apply(&mut note, true, gap(730_000, 735_000), now);
        assert_eq!(said(&live, Duration::ZERO).as_deref(), Some("Couldn't transcribe 12:03–12:15"));
        assert_eq!(note.transcript_gaps, [TranscriptGapDTO { to_ms: 735_000, ..stretch }]);
        // Another recording starts afresh; the note keeps its stretch.
        live.begin(8);
        assert_eq!(said(&live, Duration::ZERO), None);
        assert_eq!(note.transcript_gaps.len(), 1);
    }

    #[test]
    fn relabels_move_stored_segments_to_the_speaker_the_server_names() {
        let mut note = Note { id: 7, ..Note::default() };
        let mut live = live(&note);
        let now = Instant::now();
        live.apply(&mut note, true, finalise("a", 1, segment("a", AudioChannel::Mic, 1, 1_000, "Hello.")), now);
        live.apply(&mut note, true, finalise("b", 2, segment("b", AudioChannel::Mic, 1, 3_000, "Hi.")), now);
        let relabel = Event::Relabels(vec![SpeakerRelabel {
            segment_id: "b".into(),
            speaker_id: "mic:n:2".into(),
            speaker_label: "Room speaker 1".into(),
        }]);
        assert_eq!(live.apply(&mut note, true, relabel.clone(), now), Applied::Speakers);
        assert_eq!(note.server_transcript_segments[0].speaker_id, "mic:n:1");
        assert_eq!(note.server_transcript_segments[1].speaker_id, "mic:n:2");
        assert_eq!(note.server_transcript_segments[1].speaker_label.as_deref(), Some("Room speaker 1"));
        assert!(note.transcript.contains("Room speaker 1  00:03\nHi."), "{}", note.transcript);
        // Said again: nothing changes.
        assert_eq!(live.apply(&mut note, true, relabel, now), Applied::Nothing);
    }

    #[test]
    fn the_stop_waits_for_every_capture_to_be_delivered() {
        let manifest = |id: &str| crate::audio::CaptureManifest {
            version: 1,
            capture_id: id.into(),
            note_id: 7,
            directory: "/x".into(),
            manifest_path: "/x/manifest.json".into(),
            started_at: "2026-11-16T10:00:00Z".into(),
            finished_at: None,
            stats: Default::default(),
            error: None,
        };
        let mut note = Note {
            id: 7,
            audio_captures: vec![manifest("one"), manifest("two"), manifest("three")],
            live_pending: vec!["one".into(), "three".into()],
            pending_stop: Some(PendingStop::Generate),
            ..Note::default()
        };
        let owed: Vec<_> = undelivered_captures(&note).into_iter().map(|capture| capture.capture_id).collect();
        assert_eq!(owed, ["one", "three"]);
        assert_eq!(stop_due(&note, false), None);
        note.live_pending.clear();
        assert_eq!(stop_due(&note, true), None, "a stream still owes the server");
        assert_eq!(stop_due(&note, false), Some(PendingStop::Generate));
        note.pending_stop = None;
        assert_eq!(stop_due(&note, false), None);
        // What a quit leaves on disk is read back as it was.
        note.pending_stop = Some(PendingStop::Stop);
        note.live_pending = vec!["two".into()];
        let saved: serde_json::Value = serde_json::to_value(&note).unwrap();
        assert_eq!((saved["pendingStop"].as_str(), saved["livePending"][0].as_str()), (Some("stop"), Some("two")));
        let read: Note = serde_json::from_value(saved).unwrap();
        assert_eq!((read.pending_stop, read.live_pending), (Some(PendingStop::Stop), vec!["two".to_string()]));
        assert!(note.has_unsent_work());
    }
}
