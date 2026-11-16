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
    live::{Channel, Event, Interim, Phase, Session, Source, SpeakerMerge},
    model::Note,
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
/// How long the note about a skipped stretch stays up.
const GAP_NOTICE: Duration = Duration::from_secs(8);
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
    session: Session,
}

/// The uploader of the recording's durable parts (see
/// `server::PartUploads`): it runs for as long as the capture does.
struct Parts {
    note: u64,
    uploads: crate::server::PartUploads,
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
    parts: Option<Parts>,
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
    gap_at: Option<Instant>,
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
    slot: u64,
}

impl LiveTranscript {
    /// Whether text for `note` is arriving on a stream right now.
    pub fn streams(&self, note: u64) -> bool {
        self.session.as_ref().is_some_and(|stream| stream.note == note)
            || self.ending.iter().any(|stream| stream.note == note)
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

    /// Stage slots of the lines that are arriving or firming up.
    pub fn line_slots(&self) -> impl Iterator<Item = u64> + '_ {
        self.fresh.iter().chain(&self.firmed).map(|(_, slot)| *slot)
    }

    /// Forget lines whose animation has finished (`settled` is asked per slot).
    pub fn settle_lines(&mut self, settled: impl Fn(u64) -> bool) {
        self.fresh.retain(|(_, slot)| !settled(*slot));
        self.firmed.retain(|(_, slot)| !settled(*slot));
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
        if self.note != Some(note) {
            return None;
        }
        self.status_at(Instant::now())
    }

    fn status_at(&self, now: Instant) -> Option<LiveStatus> {
        let quiet = |text: String| {
            Some(LiveStatus {
                text: text.into(),
                warning: false,
            })
        };
        let since = |at: Option<Instant>| at.map_or(Duration::MAX, |at| now.duration_since(at));
        match self.phase {
            Phase::ProviderUnavailable => Some(LiveStatus {
                text: "Live transcript unavailable · still recording audio".into(),
                warning: true,
            }),
            Phase::CatchingUp if self.behind >= 5 => {
                quiet(format!("Transcript about {} s behind", self.behind))
            }
            Phase::CatchingUp => quiet("Catching up…".into()),
            Phase::Reconnecting if since(self.phase_since) >= RECONNECT_GRACE => {
                quiet("Reconnecting · still recording audio".into())
            }
            _ if since(self.gap_at) < GAP_NOTICE => {
                quiet("Skipped ahead · the gap fills in within a minute".into())
            }
            _ => None,
        }
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
        self.phase = Phase::Off;
        self.behind = 0;
        self.phase_since = None;
        self.gap_at = None;
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
                let replaced = self
                    .interim
                    .iter()
                    .position(|shown| shown.utterance_id == utterance_id)
                    .map(|at| self.interim.remove(at))
                    .is_some();
                self.fresh.retain(|(id, _)| *id != utterance_id);
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
            event => self.apply_passing(current, event, now),
        }
    }

    /// The events that leave the note alone: interim text, status, gaps.
    /// Kept apart so that taking one in does not even borrow the note (and so
    /// does not mark the workspace as changed five times a second).
    fn apply_passing(&mut self, current: bool, event: Event, now: Instant) -> Applied {
        match event {
            Event::Final { .. } | Event::Speakers(_) => Applied::Nothing,
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
            Event::Gap { .. } => {
                if current {
                    self.gap_at = Some(now);
                    Applied::Status
                } else {
                    Applied::Nothing
                }
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

/// Take in the transcript as the server has it, read after it finished a
/// durable part: this is how a stretch the live stream missed is filled in
/// while the meeting is still going. Segments the note lacks are inserted at
/// their place on the timeline; stored text never changes, so what the note
/// already has stays as it is. Returns whether the note changed.
///
/// A read that is at least as new as everything the note has seen is the
/// whole truth, so a segment the note has and the read does not was retired
/// by the server and goes. An older read (a final arrived on the stream
/// after it was taken) can only add.
pub(crate) fn merge_remote(
    note: &mut Note,
    remote: Vec<TranscriptSegmentDTO>,
    revision: u64,
) -> bool {
    use std::collections::HashSet;
    let mut changed = false;
    if revision >= note.transcript_revision {
        let kept: HashSet<&str> = remote.iter().map(|segment| segment.id.as_str()).collect();
        let before = note.server_transcript_segments.len();
        note.server_transcript_segments
            .retain(|segment| kept.contains(segment.id.as_str()));
        changed |= note.server_transcript_segments.len() != before;
    }
    let known: HashSet<String> = note
        .server_transcript_segments
        .iter()
        .map(|segment| segment.id.clone())
        .collect();
    for segment in remote {
        if known.contains(&segment.id) {
            continue;
        }
        let at = note
            .server_transcript_segments
            .iter()
            .rposition(|stored| stored.start_ms <= segment.start_ms)
            .map_or(0, |at| at + 1);
        note.server_transcript_segments.insert(at, segment);
        changed = true;
    }
    if changed {
        note.normalize_generation_revision();
        note.transcript = plain_text(&note.server_transcript_segments);
    }
    note.transcript_revision = note.transcript_revision.max(revision);
    changed
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
            // `<channel>:n:<number>` is "Speaker <number>".
            .unwrap_or_else(|| {
                merge
                    .to
                    .rsplit(':')
                    .next()
                    .filter(|number| !number.is_empty() && number.chars().all(|c| c.is_ascii_digit()))
                    .map(|number| format!("Speaker {number}"))
            });
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

impl Cereal {
    /// A capture has started for note `id`. The stream itself starts when the
    /// server confirms that the note is recording (`live_connect`).
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

    /// The server has note `id` in the recording state: follow its capture.
    /// Tried once per recording; an instance without live transcription
    /// refuses the socket and the recording carries on exactly as before.
    pub fn live_connect(&mut self, id: u64, cx: &mut Context<Self>) {
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
        let uploaded = note.clone();
        // The stream belongs to the note's account: it stops when that
        // account's epoch moves (its session ended or was replaced, or it
        // was signed out of). The capture itself is local and carries on.
        let account = crate::accounts::note_epoch(id);
        let session = Session::start(
            id,
            Source::Monitor(pending.monitor.clone()),
            move || crate::server::live_target(&note, &capture_id),
            move || crate::accounts::note_epoch(id) == account,
        );
        self.live.streams += 1;
        self.live.session = Some(Stream {
            id: self.live.streams,
            note: id,
            session,
        });
        // The capture's durable parts go up as they are completed, whether
        // or not this server transcribes live: they are what fills a stretch
        // the stream missed, and on a server without live transcription they
        // are the transcript.
        self.live.parts = Some(Parts {
            note: id,
            uploads: crate::server::PartUploads::start(
                uploaded,
                pending.capture_id.clone(),
                Source::Monitor(pending.monitor.clone()),
            ),
        });
        self.live_watch(cx);
    }

    /// The recording stopped (or its owner changed): let the stream send the
    /// rest of the capture and collect its last finals, in the background.
    pub fn live_end(&mut self) {
        self.live.capture = None;
        // The part uploader stops where it is; the upload after Stop sends
        // whatever it had not, under the same identities.
        self.live.parts = None;
        if let Some(stream) = self.live.session.take() {
            stream.session.end();
            self.live.ending.push(stream);
        }
        if self.live.phase != Phase::Off || self.live.gap_at.is_some() {
            self.live.phase = Phase::Off;
            self.live.behind = 0;
            self.live.gap_at = None;
            self.live.revision = self.live.revision.wrapping_add(1);
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

    /// Look at the streams twenty times a second while there are any. The
    /// update does nothing, and draws nothing, when nothing has arrived.
    fn live_watch(&mut self, cx: &mut Context<Self>) {
        if std::mem::replace(&mut self.live.watching, true) {
            return;
        }
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(50))
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
        for (stream, note_id, current, event) in events {
            redraw = redraw.max(if event == Event::Ended {
                self.live_ended(stream, note_id)
            } else {
                self.live_event(note_id, current, event, cx)
            });
        }
        // A durable part was transcribed: take in whatever of the server's
        // transcript the note does not have yet.
        let transcribed: Vec<_> = self
            .live
            .parts
            .iter()
            .flat_map(|parts| parts.uploads.events.try_iter().map(|event| (parts.note, event)))
            .collect();
        for (note_id, event) in transcribed {
            let crate::server::PartEvent::Transcript { revision, segments } = event;
            let merged = self
                .workspace
                .note_mut(note_id)
                .is_some_and(|note| merge_remote(note, segments, revision));
            if merged {
                self.forget_utterances();
                self.live_save_later(cx);
                redraw = Redraw::Window;
            }
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
            || self.live.parts.is_some()
            || !self.live.ending.is_empty()
            || self.live.phase == Phase::Reconnecting
            || self.live.gap_at.is_some_and(|at| at.elapsed() < GAP_NOTICE);
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
        let applied = if matches!(event, Event::Final { .. } | Event::Speakers(_)) {
            match self.workspace.note_mut(note_id) {
                Some(note) => self.live.apply(note, current, event, now),
                None => Applied::Nothing,
            }
        } else if self.workspace.note(note_id).is_some() {
            self.live.apply_passing(current, event, now)
        } else {
            Applied::Nothing
        };
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
        // A gap is noted for a few seconds, then the line is empty again.
        live.apply(&mut note, true, Event::Status(Phase::Live, 0), start);
        live.apply(&mut note, true, Event::Gap { channel: Channel::Mic, from_ms: 1, to_ms: 2 }, start);
        assert!(live.status_at(start + Duration::from_secs(1)).unwrap().text.starts_with("Skipped ahead"));
        assert_eq!(live.status_at(start + Duration::from_secs(9)), None);
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

    #[test]
    fn a_durable_part_fills_a_gap_at_its_place_on_the_timeline() {
        let mut note = Note { id: 7, ..Note::default() };
        let mut live = live(&note);
        let now = Instant::now();
        // The stream delivered 0-2 s and 6-8 s; it missed what lies between.
        for (id, start) in [("a", 0), ("b", 1_000), ("e", 6_000), ("f", 7_000)] {
            let stored = segment(id, AudioChannel::Mic, 1, start, id);
            live.apply(&mut note, true, finalise(id, 4, stored), now);
        }
        let ids = |note: &Note| -> Vec<String> {
            note.server_transcript_segments.iter().map(|s| s.id.clone()).collect()
        };
        // The server transcribed the part covering the gap and stored two
        // segments; its transcript is read at revision 6.
        let mut remote = note.server_transcript_segments.clone();
        remote.insert(2, segment("c", AudioChannel::Mic, 1, 2_000, "c"));
        remote.insert(3, segment("d", AudioChannel::Mic, 1, 4_000, "d"));
        assert!(merge_remote(&mut note, remote.clone(), 6));
        assert_eq!(ids(&note), ["a", "b", "c", "d", "e", "f"]);
        assert_eq!(note.transcript_revision, 6);
        assert_eq!(note.transcript, plain_text(&note.server_transcript_segments));
        // The same read again changes nothing: no duplicates.
        assert!(!merge_remote(&mut note, remote.clone(), 6));
        assert_eq!(ids(&note).len(), 6);
        // A final arrives on the stream; an older read that lacks it must
        // not take it away.
        live.apply(&mut note, true, finalise("g", 7, segment("g", AudioChannel::Mic, 1, 8_000, "g")), now);
        assert!(!merge_remote(&mut note, remote.clone(), 6));
        assert_eq!(ids(&note), ["a", "b", "c", "d", "e", "f", "g"]);
        // A read that is newer than everything here is the whole truth: a
        // segment the server retired goes, one it added comes.
        let mut newer: Vec<_> = note.server_transcript_segments.clone();
        newer.retain(|segment| segment.id != "d");
        newer.push(segment("h", AudioChannel::System, 1, 8_500, "h"));
        assert!(merge_remote(&mut note, newer, 9));
        assert_eq!(ids(&note), ["a", "b", "c", "e", "f", "g", "h"]);
        assert_eq!(note.transcript_revision, 9);
        // An empty note takes a read whole (a server without live
        // transcription: the parts are the transcript).
        let mut empty = Note { id: 8, ..Note::default() };
        assert!(merge_remote(&mut empty, remote, 6));
        assert_eq!(ids(&empty), ["a", "b", "c", "d", "e", "f"]);
    }

    /// A local server that is both halves of what a recording talks to: the
    /// live socket (`.../live`) and the HTTP endpoints for durable parts.
    /// It keeps the two honest with each other the way the real server does:
    /// audio the socket turned into text is "covered", and when a part
    /// arrives only what the socket did not cover is transcribed from it.
    mod fake {
        use std::{
            collections::HashMap,
            io::{Read, Write},
            net::{TcpListener, TcpStream},
            sync::{Arc, Mutex},
            time::{Duration, Instant},
        };
        use tungstenite::Message;

        pub const NOTE: &str = "419d65d1-7a2c-44ca-84cc-ad489522d561";
        const RATE: u64 = 16_000;

        #[derive(Clone)]
        pub struct Chunk {
            pub id: String,
            pub sequence: u64,
            pub start_ms: u64,
            pub duration_ms: u64,
            pub ready: bool,
        }

        #[derive(Default)]
        pub struct State {
            pub recording: bool,
            pub revision: i64,
            /// `(id, start ms, end ms, text)`, in the order they were stored.
            pub segments: Vec<(String, u64, u64, String)>,
            pub chunks: Vec<Chunk>,
            /// How many times each part was sent.
            pub puts: HashMap<String, usize>,
            /// Parts refused for clashing with another part's number.
            pub conflicts: usize,
            /// Sample ranges the socket turned into stored text.
            pub covered: Vec<(u64, u64)>,
            /// Where the socket's not yet finalised audio starts.
            pending_from: u64,
            next_sample: u64,
            pub live_over: bool,
            pub sockets: usize,
            pub gaps: Vec<(u64, u64)>,
            outage_until: Option<Instant>,
        }

        impl State {
            fn store(&mut self, start_ms: u64, end_ms: u64, origin: &str) -> serde_json::Value {
                self.revision += 1;
                let id = uuid::Uuid::new_v5(
                    &uuid::Uuid::NAMESPACE_URL,
                    format!("{origin}:{start_ms}:{end_ms}").as_bytes(),
                )
                .to_string();
                let text = format!("{origin} {start_ms}-{end_ms}");
                self.segments.push((id, start_ms, end_ms, text));
                self.segment_json(self.segments.len() - 1)
            }

            fn segment_json(&self, index: usize) -> serde_json::Value {
                let (id, start, end, text) = &self.segments[index];
                serde_json::json!({"id": id, "channel": "mic", "speakerId": "mic:n:1", "speakerLabel": "Speaker 1",
                    "text": text, "isFinal": true, "startMs": start, "endMs": end})
            }

            fn note_json(&self) -> serde_json::Value {
                let mut order: Vec<usize> = (0..self.segments.len()).collect();
                order.sort_by_key(|index| self.segments[*index].1);
                serde_json::json!({
                    "id": NOTE, "title": "Gap", "body": "", "enhanced": "", "date": "2026-11-14", "duration": "0:00",
                    "createdAt": "2026-11-14T10:00:00.000Z", "updatedAt": "2026-11-14T10:00:00.000Z",
                    "generatedRevision": null, "generationState": "idle",
                    "recordingState": if self.recording { "recording" } else { "stopped" },
                    "transcriptRevision": self.revision,
                    "transcriptSegments": order.into_iter().map(|index| self.segment_json(index)).collect::<Vec<_>>(),
                })
            }

            fn chunk_json(chunk: &Chunk) -> serde_json::Value {
                serde_json::json!({"id": chunk.id, "noteId": NOTE, "channel": "mic", "sequence": chunk.sequence,
                    "startMs": chunk.start_ms, "durationMs": chunk.duration_ms,
                    "state": if chunk.ready { "ready" } else { "queued" }, "error": null})
            }

            /// The batch worker: a part is decided once the socket is no
            /// longer working on any of it, and only the audio the socket
            /// did not turn into text is transcribed from it.
            fn settle(&mut self) {
                let working_from = self.pending_from * 1000 / RATE;
                for index in 0..self.chunks.len() {
                    let chunk = self.chunks[index].clone();
                    let end = chunk.start_ms + chunk.duration_ms;
                    if chunk.ready || !(self.live_over || end <= working_from) {
                        continue;
                    }
                    let mut at = chunk.start_ms;
                    let mut covered: Vec<(u64, u64)> = self
                        .covered
                        .iter()
                        .map(|(from, to)| (from * 1000 / RATE, to * 1000 / RATE))
                        .collect();
                    covered.sort();
                    let mut missing = Vec::new();
                    for (from, to) in covered {
                        if to <= at || from >= end {
                            continue;
                        }
                        if from > at {
                            missing.push((at, from));
                        }
                        at = at.max(to);
                    }
                    if at < end {
                        missing.push((at, end));
                    }
                    for (from, to) in missing {
                        self.store(from, to, "batch");
                    }
                    self.chunks[index].ready = true;
                }
            }
        }

        pub struct Server {
            pub url: String,
            pub state: Arc<Mutex<State>>,
        }

        /// `drop_after` samples into the first socket it is cut, and no
        /// socket is accepted for `outage`; `window_ms` is the resume window
        /// every `ready` announces.
        pub fn serve(drop_after: u64, outage: Duration, window_ms: u64) -> Server {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}", listener.local_addr().unwrap());
            let state = Arc::new(Mutex::new(State { recording: true, ..State::default() }));
            let shared = state.clone();
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    let Ok(stream) = stream else { return };
                    let state = shared.clone();
                    std::thread::spawn(move || {
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
                        let mut head = [0u8; 512];
                        let peeked = stream.peek(&mut head).unwrap_or(0);
                        if String::from_utf8_lossy(&head[..peeked]).contains("/live ") {
                            socket(stream, &state, drop_after, outage, window_ms);
                        } else {
                            http(stream, &state);
                        }
                    });
                }
            });
            Server { url, state }
        }

        fn socket(stream: TcpStream, state: &Mutex<State>, drop_after: u64, outage: Duration, window_ms: u64) {
            let first = {
                let mut state = state.lock().unwrap();
                if state.outage_until.is_some_and(|until| Instant::now() < until) {
                    return; // The network is down: the upgrade never completes.
                }
                state.sockets += 1;
                state.sockets == 1
            };
            let Ok(mut socket) = tungstenite::accept(stream) else { return };
            let say = |socket: &mut tungstenite::WebSocket<TcpStream>, value: serde_json::Value| {
                socket.send(Message::text(value.to_string())).is_ok()
            };
            let Ok(Message::Text(_hello)) = socket.read() else { return };
            let accept = {
                let mut state = state.lock().unwrap();
                // Audio taken but not finalised before the drop is lost to
                // the stream; it resumes after it.
                state.pending_from = state.next_sample;
                state.next_sample
            };
            say(&mut socket, serde_json::json!({"type": "ready", "sessionId": "0b1c2d3e-0000-4000-8000-0000000000aa",
                "provider": "fake", "model": "fake-live", "streams": [{"channel": "mic", "acceptFromSample": accept}],
                "resumeWindowMs": window_ms, "transcriptRevision": state.lock().unwrap().revision}));
            loop {
                let Ok(message) = socket.read() else { return };
                match message {
                    Message::Binary(bytes) => {
                        let first_sample = u64::from_le_bytes(bytes[4..12].try_into().unwrap());
                        let count = (bytes.len() as u64 - 12) / 2;
                        let mut replies = Vec::new();
                        let cut = {
                            let mut state = state.lock().unwrap();
                            if first_sample > state.next_sample {
                                // A jump forward is a declared gap.
                                let (from, to) = (state.next_sample * 1000 / RATE, first_sample * 1000 / RATE);
                                state.gaps.push((from, to));
                                replies.push(serde_json::json!({"type": "gap", "channel": "mic", "fromMs": from, "toMs": to, "reason": "resume_window_exceeded"}));
                                state.pending_from = first_sample;
                            }
                            state.next_sample = state.next_sample.max(first_sample + count);
                            // One stored segment per second of audio heard.
                            while state.next_sample - state.pending_from >= RATE {
                                let (from, to) = (state.pending_from, state.pending_from + RATE);
                                state.covered.push((from, to));
                                state.pending_from = to;
                                let segment = state.store(from * 1000 / RATE, to * 1000 / RATE, "live");
                                replies.push(serde_json::json!({"type": "final", "utteranceId": segment["id"], "transcriptRevision": state.revision, "segment": segment}));
                            }
                            replies.push(serde_json::json!({"type": "ack", "channel": "mic", "throughSample": state.next_sample, "transcribedThroughMs": state.pending_from * 1000 / RATE}));
                            state.settle();
                            let cut = first && state.next_sample >= drop_after;
                            if cut {
                                state.outage_until = Some(Instant::now() + outage);
                            }
                            cut
                        };
                        for reply in replies {
                            if !say(&mut socket, reply) {
                                return;
                            }
                        }
                        if cut {
                            return; // Dropped without a close frame.
                        }
                    }
                    Message::Text(text) if text.as_str().contains(r#""kind":"end""#) => {
                        let last = {
                            let mut state = state.lock().unwrap();
                            let (from, to) = (state.pending_from, state.next_sample);
                            let last = (to > from).then(|| {
                                state.covered.push((from, to));
                                state.pending_from = to;
                                let segment = state.store(from * 1000 / RATE, to * 1000 / RATE, "live");
                                serde_json::json!({"type": "final", "utteranceId": segment["id"], "transcriptRevision": state.revision, "segment": segment})
                            });
                            state.live_over = true;
                            state.settle();
                            last
                        };
                        if let Some(last) = last {
                            say(&mut socket, last);
                        }
                        let _ = socket.close(None);
                        while socket.read().is_ok() {}
                        return;
                    }
                    Message::Close(_) => return,
                    _ => {}
                }
            }
        }

        fn http(mut stream: TcpStream, state: &Mutex<State>) {
            let mut head = Vec::new();
            let mut byte = [0u8; 1];
            while !head.ends_with(b"\r\n\r\n") {
                if stream.read_exact(&mut byte).is_err() {
                    return;
                }
                head.push(byte[0]);
            }
            let head = String::from_utf8_lossy(&head).into_owned();
            let length: usize = head
                .lines()
                .find_map(|line| line.to_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse().unwrap()))
                .unwrap_or(0);
            if head.to_lowercase().contains("expect: 100-continue") {
                let _ = stream.write_all(b"HTTP/1.1 100 Continue\r\n\r\n");
            }
            let mut body = vec![0u8; length];
            if stream.read_exact(&mut body).is_err() {
                return;
            }
            let mut request = head.split_whitespace();
            let (method, path) = (request.next().unwrap_or(""), request.next().unwrap_or(""));
            let chunks = format!("/api/v1/notes/{NOTE}/audio/chunks");
            let (status, reply) = {
                let mut state = state.lock().unwrap();
                state.settle();
                match (method, path) {
                    ("GET", path) if path == chunks => (
                        200,
                        serde_json::Value::Array(state.chunks.iter().map(State::chunk_json).collect()),
                    ),
                    ("PUT", path) if path.starts_with(&chunks) => {
                        let id = path.rsplit('/').next().unwrap().to_owned();
                        let part: serde_json::Value = serde_json::from_slice(&body).unwrap();
                        let (sequence, start_ms) = (part["sequence"].as_u64().unwrap(), part["startMs"].as_u64().unwrap());
                        // A PCM16 WAV: 44 bytes of header, then two bytes a sample.
                        let encoded = part["audioBase64"].as_str().unwrap().len() as u64;
                        let duration_ms = (encoded / 4 * 3 - 44) / 2 * 1000 / RATE;
                        *state.puts.entry(id.clone()).or_default() += 1;
                        let same = state.chunks.iter().position(|chunk| chunk.id == id);
                        let clash = state.chunks.iter().any(|chunk| chunk.id != id && chunk.sequence == sequence);
                        match same {
                            Some(index) if state.chunks[index].sequence == sequence && state.chunks[index].start_ms == start_ms => {
                                (200, State::chunk_json(&state.chunks[index]))
                            }
                            _ if same.is_some() || clash => {
                                state.conflicts += 1;
                                (409, serde_json::json!({"error": "conflict", "message": "Chunk ID or sequence already exists with different audio or metadata"}))
                            }
                            _ => {
                                state.chunks.push(Chunk { id, sequence, start_ms, duration_ms, ready: false });
                                state.settle();
                                (200, State::chunk_json(state.chunks.last().unwrap()))
                            }
                        }
                    }
                    ("POST", path) if path.ends_with("/recording") => {
                        // Stop ends the live session: nothing more will come from it.
                        state.recording = false;
                        state.live_over = true;
                        state.settle();
                        (200, state.note_json())
                    }
                    _ => (200, state.note_json()),
                }
            };
            let reply = reply.to_string();
            let _ = write!(
                stream,
                "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                reply.len()
            );
        }
    }

    /// The whole of a recording that loses its socket for longer than the
    /// resume window: the stream skips ahead, the part covering what it
    /// skipped is uploaded while the meeting is still going and fills the gap
    /// in the transcript before Stop, and Stop sends only the parts that had
    /// not gone up yet.
    #[test]
    fn a_dropped_socket_beyond_the_resume_window_is_healed_from_a_part_before_stop() {
        use crate::live::tests::{Fixture, tone};
        use crate::live::{Resolved, Session, Target};
        // One second in, the socket is cut and stays down for 6.5 s; the
        // server will replay at most 2 s.
        let server = fake::serve(16_000, Duration::from_millis(6_500), 2_000);
        let mut fixture = Fixture::new("heal");
        let mut mic = fixture.add(Channel::Mic, "mic.wav", 16_000);
        let root = fixture.directory.join("storage");
        std::fs::create_dir_all(&root).unwrap();
        let capture_id = fixture.manifest.capture_id.clone();
        let local = 0x6a70_0000 + std::process::id() as u64;
        let mut note = Note {
            id: local,
            server_id: Some(fake::NOTE.into()),
            server_url: Some(server.url.clone()),
            audio_captures: vec![fixture.manifest.clone()],
            ..Note::default()
        };
        // Two-second parts, so a test meeting of fourteen seconds has seven.
        crate::server::testing::set_part_seconds(&server.url, &root, &note, &capture_id, 2);
        let target = Target {
            endpoint: crate::accounts::client::LiveEndpoint::for_test(
                crate::server::testing::live_url(&server.url, fake::NOTE),
                "fictional-part-upload-owner-token-0000",
            ),
            capture_id: capture_id.clone(),
            base_ms: 0,
        };
        let session = Session::start(local, fixture.source(), move || Resolved::Ready(target.clone()), || true);
        let parts = crate::server::testing::part_uploads(&server.url, &root, note.clone(), &capture_id, fixture.source());
        let mut live = live(&note);
        let started = Instant::now();
        let audio = tone(16_000, 330., 0, 16_000 * 14);
        let (mut written, mut gap_seen, mut healed_at, mut ended) = (0usize, false, None, false);
        let covered = |note: &Note, from: i64, to: i64| {
            // Every millisecond of [from, to) is inside some stored segment.
            let mut at = from;
            for segment in &note.server_transcript_segments {
                if segment.start_ms <= at && segment.end_ms.unwrap_or(0) > at {
                    at = segment.end_ms.unwrap();
                }
            }
            at >= to
        };
        let pump = |note: &mut Note, live: &mut LiveTranscript, gap_seen: &mut bool, ended: &mut bool| {
            for event in session.events.try_iter() {
                *gap_seen |= matches!(event, Event::Gap { .. });
                *ended |= event == Event::Ended;
                live.apply(note, true, event, Instant::now());
            }
            for event in parts.uploads_events() {
                let crate::server::PartEvent::Transcript { revision, segments } = event;
                merge_remote(note, segments, revision);
            }
        };
        // The meeting: fourteen seconds of audio arriving in real time.
        while written < audio.len() {
            let due = (started.elapsed().as_millis() as usize * 16).min(audio.len());
            if due > written {
                mic.append(&audio[written..due]);
                written = due;
            }
            pump(&mut note, &mut live, &mut gap_seen, &mut ended);
            if healed_at.is_none() && gap_seen {
                let gap = server.state.lock().unwrap().gaps[0];
                if covered(&note, gap.0 as i64, gap.1 as i64) {
                    healed_at = Some(started.elapsed());
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        // What was true while the meeting was still being recorded.
        let (gap, sent_before_stop) = {
            let state = server.state.lock().unwrap();
            assert_eq!(state.gaps.len(), 1, "one gap: {:?}", state.gaps);
            (state.gaps[0], state.chunks.len())
        };
        assert!(gap_seen, "the stream reported the gap");
        // The socket was cut at 1 s (or a frame later) and came back near
        // the end of what had been captured by then (half the 2 s resume
        // window before it), and that was the only skip.
        assert!((1_000..=1_300).contains(&gap.0) && gap.1 >= 3_000, "gap {gap:?}");
        let healed_at = healed_at.expect("the gap was filled from a durable part before Stop");
        assert!(sent_before_stop >= 5, "{sent_before_stop} parts went up during the meeting");
        // The filled-in text sits where it belongs: the transcript is in
        // time order with no segment twice.
        let starts: Vec<i64> = note.server_transcript_segments.iter().map(|s| s.start_ms).collect();
        assert!(starts.windows(2).all(|pair| pair[0] <= pair[1]), "{starts:?}");
        assert!(note.server_transcript_segments.iter().any(|s| s.text.starts_with("batch")));
        // Stop: the capture is finalised, the stream ends, the uploader lets
        // go, and the serial worker's Stop sends what is left.
        fixture.finish();
        note.audio_captures = vec![fixture.manifest.clone()];
        session.end();
        parts.stop();
        let stopped = crate::server::testing::stop_recording(&server.url, &root, &note).expect("Stop");
        let deadline = Instant::now() + Duration::from_secs(20);
        while !ended && Instant::now() < deadline {
            pump(&mut note, &mut live, &mut gap_seen, &mut ended);
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(ended, "the stream ended");
        let state = server.state.lock().unwrap();
        // Seven two-second parts, each sent exactly once: the ones that went
        // up during the meeting were not sent again after Stop.
        assert_eq!(state.chunks.len(), 7, "{:?}", state.chunks.iter().map(|c| (c.start_ms, c.duration_ms)).collect::<Vec<_>>());
        assert!(state.puts.values().all(|count| *count == 1), "{:?}", state.puts);
        assert_eq!(state.conflicts, 0);
        assert!(state.chunks.iter().all(|chunk| chunk.ready));
        let mut sequences: Vec<u64> = state.chunks.iter().map(|chunk| chunk.sequence).collect();
        sequences.sort();
        assert_eq!(sequences, (0..7).collect::<Vec<u64>>());
        let mut positions: Vec<(u64, u64)> = state.chunks.iter().map(|c| (c.start_ms, c.duration_ms)).collect();
        positions.sort();
        assert_eq!(positions, (0..7).map(|part| (part * 2_000, 2_000)).collect::<Vec<_>>());
        // Every millisecond of the meeting was transcribed exactly once:
        // by the stream or from a part, never both and never neither.
        let mut spans: Vec<(u64, u64)> = state.segments.iter().map(|(_, from, to, _)| (*from, *to)).collect();
        spans.sort();
        assert_eq!(spans.first().unwrap().0, 0);
        assert_eq!(spans.last().unwrap().1, 14_000);
        for pair in spans.windows(2) {
            assert_eq!(pair[0].1, pair[1].0, "no hole and no overlap: {spans:?}");
        }
        // What the window holds is what the server holds: the same segments,
        // once each (the note Stop returns is merged like any other read).
        merge_remote(&mut note, stopped.transcript_segments.clone(), stopped.transcript_revision as u64);
        let mut here: Vec<&str> = note.server_transcript_segments.iter().map(|s| s.id.as_str()).collect();
        let mut there: Vec<&str> = state.segments.iter().map(|(id, ..)| id.as_str()).collect();
        assert_eq!(here.len(), there.len());
        here.sort();
        there.sort();
        here.dedup();
        assert_eq!(here, there);
        assert!(live.interim.is_empty());
        eprintln!(
            "gap {} to {} ms filled {:.1} s into a 14 s meeting; {sent_before_stop} of 7 parts sent before Stop",
            gap.0, gap.1, healed_at.as_secs_f32()
        );
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
}
