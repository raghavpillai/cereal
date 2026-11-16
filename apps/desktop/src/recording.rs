//! Capture lifecycle and elapsed time. The audio module supplies real input
//! levels; explicit preview modes never open an audio device or invent speech.
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Idle,
    Starting,
    Live,
    Held,
    Complete,
    Failed,
}

/// Visual transcription attention is independent of the session lifecycle.
/// A warning may remain visible while the local recording preview continues.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TranscriptionAttention {
    #[default]
    None,
    Warning,
}

#[derive(Debug, Default)]
pub struct Recording {
    pub phase: Phase,
    pub transcription_attention: TranscriptionAttention,
    microphone_level: f32,
    system_level: f32,
    /// The level the bars are drawn from: the louder input, held until it
    /// moves by [`LEVEL_STEP`] (see [`Recording::set_audio_levels`]).
    shown_level: f32,
    /// The input level a stretch of steady input started at and when, and
    /// whether the shown level has settled onto it yet.
    steady: Option<(f32, Instant, bool)>,
    pub note_id: Option<u64>,
    /// Captured by delayed transitions. Invalidated by user transitions/reset.
    pub epoch: u64,
    accumulated: Duration,
    live_since: Option<Instant>,
}

/// The smallest change in the input level that the bars follow: about half
/// of the level it takes to move the centre bar by one step.
pub const LEVEL_STEP: f32 = 0.04;
/// How long the input must stay within a step for the shown level to settle
/// onto it, so a slow fade does not leave the bars a step off.
const SETTLE_AFTER: Duration = Duration::from_millis(500);

impl Recording {
    /// Level of actual capture inputs, as the level bars show it. Silence
    /// stays quiet; no demo waveform is inferred from elapsed time.
    /// Capture/server adapters supply these values.
    pub fn audio_level(&self) -> f32 {
        if self.is_active() {
            self.shown_level
        } else {
            0.
        }
    }

    /// The microphone's own level, 0..=1, for the sound popover's meter.
    pub fn microphone_level(&self) -> f32 {
        if self.is_active() {
            self.microphone_level
        } else {
            0.
        }
    }

    pub fn set_audio_levels(&mut self, microphone: f32, system: f32) {
        self.set_audio_levels_at(microphone, system, Instant::now());
    }

    fn set_audio_levels_at(&mut self, microphone: f32, system: f32, now: Instant) {
        let clean = |level: f32| {
            if level.is_finite() {
                level.clamp(0., 1.)
            } else {
                0.
            }
        };
        self.microphone_level = clean(microphone);
        self.system_level = clean(system);
        // Each poll reads the loudness (25 × RMS) of the latest tenth of a
        // second of audio, which is never quite the same twice even for a
        // steady sound. The shown level holds until the input really moves,
        // so a steady sound keeps steady bars (and the window is not redrawn
        // for them), while speech, which swings by far more than a step,
        // moves them on every poll.
        let level = self.microphone_level.max(self.system_level);
        if level == 0. || (level - self.shown_level).abs() >= LEVEL_STEP {
            self.shown_level = level;
        }
        // Once the input has stayed within a step for half a second, the
        // shown level settles onto it, once, if it is a quarter step or more
        // away: a slow fade into room noise then rests on the room's own
        // level rather than up to a step above it.
        match self.steady {
            Some((start, _, _)) if (level - start).abs() < LEVEL_STEP => {}
            _ => self.steady = Some((level, now, false)),
        }
        if let Some((_, since, settled @ false)) = self.steady.as_mut()
            && now.saturating_duration_since(*since) >= SETTLE_AFTER
        {
            *settled = true;
            if (level - self.shown_level).abs() >= LEVEL_STEP / 4. {
                self.shown_level = level;
            }
        }
    }
    pub fn begin(&mut self, note_id: u64) -> bool {
        self.begin_at(note_id)
    }

    fn begin_at(&mut self, note_id: u64) -> bool {
        if !matches!(self.phase, Phase::Idle | Phase::Complete | Phase::Failed) {
            return false;
        }
        self.bump_epoch();
        self.set_audio_levels(0., 0.);
        self.note_id = Some(note_id);
        self.phase = Phase::Starting;
        self.transcription_attention = TranscriptionAttention::None;
        self.accumulated = Duration::ZERO;
        self.live_since = None;
        true
    }

    pub fn activate(&mut self, epoch: u64) -> bool {
        self.activate_at(epoch, Instant::now())
    }

    fn activate_at(&mut self, epoch: u64, now: Instant) -> bool {
        if epoch != self.epoch || self.phase != Phase::Starting {
            return false;
        }
        self.phase = Phase::Live;
        self.live_since = Some(now);
        true
    }

    pub fn pause(&mut self) -> bool {
        self.pause_at(Instant::now())
    }

    fn pause_at(&mut self, now: Instant) -> bool {
        if self.phase != Phase::Live {
            return false;
        }
        self.freeze_elapsed(now);
        self.bump_epoch();
        self.phase = Phase::Held;
        true
    }

    /// Resumes this same note. Completed/stopped sessions keep their elapsed
    /// duration, as does a paused session; `begin` starts a fresh duration.
    pub fn resume(&mut self) -> bool {
        if self.note_id.is_none()
            || !matches!(self.phase, Phase::Held | Phase::Complete | Phase::Failed)
        {
            return false;
        }
        self.bump_epoch();
        self.set_audio_levels(0., 0.);
        self.phase = Phase::Starting;
        self.transcription_attention = TranscriptionAttention::None;
        self.live_since = None;
        true
    }

    pub fn stop(&mut self) -> bool {
        self.stop_at(Instant::now())
    }

    fn stop_at(&mut self, now: Instant) -> bool {
        if !matches!(self.phase, Phase::Starting | Phase::Live) {
            return false;
        }
        self.freeze_elapsed(now);
        self.bump_epoch();
        self.phase = Phase::Complete;
        self.set_audio_levels(0., 0.);
        self.transcription_attention = TranscriptionAttention::None;
        true
    }

    pub fn set_transcription_attention(&mut self, attention: TranscriptionAttention) -> bool {
        if !self.is_active() {
            return false;
        }
        self.transcription_attention = attention;
        true
    }

    pub fn fail(&mut self, epoch: u64) -> bool {
        self.fail_at(epoch, Instant::now())
    }

    fn fail_at(&mut self, epoch: u64, now: Instant) -> bool {
        if epoch != self.epoch
            || matches!(self.phase, Phase::Idle | Phase::Complete | Phase::Failed)
        {
            return false;
        }
        self.freeze_elapsed(now);
        self.phase = Phase::Failed;
        true
    }

    pub fn reset(&mut self) {
        self.bump_epoch();
        self.set_audio_levels(0., 0.);
        self.phase = Phase::Idle;
        self.transcription_attention = TranscriptionAttention::None;
        self.note_id = None;
        self.accumulated = Duration::ZERO;
        self.live_since = None;
    }

    pub fn elapsed(&self) -> Duration {
        self.elapsed_at(Instant::now())
    }

    pub fn restore_elapsed(&mut self, elapsed: Duration) {
        if self.phase == Phase::Starting && self.live_since.is_none() {
            self.accumulated = elapsed;
        }
    }

    fn elapsed_at(&self, now: Instant) -> Duration {
        self.accumulated
            + self
                .live_since
                .map_or(Duration::ZERO, |since| now.saturating_duration_since(since))
    }

    /// True only while starting or live. Paused, processing and completed
    /// sessions are not actively recording, even though their owner remains.
    pub fn is_active(&self) -> bool {
        matches!(self.phase, Phase::Starting | Phase::Live)
    }

    fn freeze_elapsed(&mut self, now: Instant) {
        if let Some(since) = self.live_since.take() {
            self.accumulated += now.saturating_duration_since(since);
        }
    }

    fn bump_epoch(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
    }
}

/// Note generation is independent of capture. Stopping a capture never starts it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GenerationPhase {
    #[default]
    Idle,
    AnalyzingTranscript,
    WritingNotes,
    Ready,
}
#[derive(Debug, Default)]
pub struct Generation {
    pub phase: GenerationPhase,
    pub note_id: Option<u64>,
    pub transcript_revision: u64,
    pub epoch: u64,
}
impl Generation {
    pub fn begin(&mut self, note_id: u64, transcript_revision: u64) -> u64 {
        self.epoch = self.epoch.wrapping_add(1);
        self.note_id = Some(note_id);
        self.transcript_revision = transcript_revision;
        self.phase = GenerationPhase::AnalyzingTranscript;
        self.epoch
    }
    pub fn writing(&mut self, epoch: u64) -> bool {
        if self.epoch != epoch || self.phase != GenerationPhase::AnalyzingTranscript {
            return false;
        }
        self.phase = GenerationPhase::WritingNotes;
        true
    }
    pub fn complete(&mut self, epoch: u64, note_id: u64, revision: u64) -> bool {
        if self.epoch != epoch
            || self.note_id != Some(note_id)
            || self.transcript_revision != revision
            || self.phase != GenerationPhase::WritingNotes
        {
            return false;
        }
        self.phase = GenerationPhase::Ready;
        true
    }
    pub fn is_running_for(&self, note_id: u64) -> bool {
        self.note_id == Some(note_id)
            && matches!(
                self.phase,
                GenerationPhase::AnalyzingTranscript | GenerationPhase::WritingNotes
            )
    }
    pub fn cancel(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        self.phase = GenerationPhase::Idle;
        self.note_id = None;
    }
}

/// The notes of a generation as the server writes them, and how much of
/// them is on the page. Text arrives in bursts; it is shown at a steady
/// pace that speeds up with the backlog, so the notes write themselves out
/// evenly instead of jumping forward a clause at a time.
#[derive(Debug, Default)]
pub struct Draft {
    /// The note being written, and the generation it belongs to.
    pub note_id: Option<u64>,
    pub epoch: u64,
    /// Everything received so far.
    text: String,
    /// How many bytes of it are shown (always at a character boundary).
    shown: usize,
    /// Fractions of a character carried between steps.
    carry: f32,
    stepped: Option<Instant>,
    /// Characters a second: set when text arrives, so that what is waiting
    /// then is shown within `DRAFT_CATCH_UP`, and back to `DRAFT_PACE` once
    /// it is.
    pace: f32,
    /// The server has finished: `text` is the saved notes.
    pub complete: bool,
    /// The page follows the text as it grows, until the person scrolls
    /// away from it. `scrolled_to` is where this last put the page.
    pub follow: bool,
    pub scrolled_to: Option<f32>,
}

/// Characters a second when nothing is waiting.
const DRAFT_PACE: f32 = 90.;
/// A backlog is shown within about this long, however much arrived at once.
const DRAFT_CATCH_UP: f32 = 0.45;

impl Draft {
    pub fn begin(&mut self, note_id: u64, epoch: u64) {
        *self = Self {
            note_id: Some(note_id),
            epoch,
            follow: true,
            ..Self::default()
        };
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Whether this is the draft of generation `epoch`.
    pub fn of(&self, epoch: u64) -> bool {
        self.note_id.is_some() && self.epoch == epoch
    }

    /// The draft of `note_id`, once it has text to show.
    pub fn showing(&self, note_id: u64) -> bool {
        self.note_id == Some(note_id) && !self.text.is_empty()
    }

    pub fn push(&mut self, piece: &str) {
        self.text.push_str(piece);
        self.hurry();
    }

    fn hurry(&mut self) {
        let waiting = self.text[self.shown..].chars().count() as f32;
        self.pace = self.pace.max(DRAFT_PACE).max(waiting / DRAFT_CATCH_UP);
    }

    /// What was streamed is dropped (the server answers without a stream).
    pub fn restart(&mut self) {
        self.text.clear();
        self.shown = 0;
        self.carry = 0.;
    }

    /// The server has saved `saved`. What is on the page stays there and the
    /// rest (the closing link line, say) is written out after it; notes that
    /// do not continue what was shown replace it at once.
    pub fn finish(&mut self, saved: &str) {
        self.complete = true;
        if saved.starts_with(&self.text[..self.shown]) {
            self.text = saved.to_owned();
        } else {
            self.text = saved.to_owned();
            self.shown = self.text.len();
        }
        self.hurry();
    }

    /// Everything received is on the page.
    pub fn caught_up(&self) -> bool {
        self.shown >= self.text.len()
    }

    /// Show what the time since the last step allows. True if more is on
    /// the page than before.
    pub fn step(&mut self, now: Instant) -> bool {
        let elapsed = self
            .stepped
            .replace(now)
            .map_or(1. / 60., |before| now.saturating_duration_since(before).as_secs_f32())
            .min(0.1);
        if self.caught_up() {
            self.carry = 0.;
            self.pace = DRAFT_PACE;
            return false;
        }
        self.carry += self.pace.max(DRAFT_PACE) * elapsed;
        let count = self.carry.floor() as usize;
        if count == 0 {
            return false;
        }
        self.carry -= count as f32;
        let rest = &self.text[self.shown..];
        self.shown += rest
            .char_indices()
            .nth(count)
            .map_or(rest.len(), |(at, _)| at);
        true
    }

    /// The Markdown on the page. A last line that is so far only its
    /// marker ("-", "#", an indent) waits for its first word, so a bullet
    /// never flashes as a dash.
    pub fn visible(&self) -> &str {
        let shown = &self.text[..self.shown];
        let line = shown.rfind('\n').map_or(0, |at| at + 1);
        if shown[line..]
            .chars()
            .all(|c| c.is_whitespace() || matches!(c, '-' | '#' | '*' | '+'))
        {
            shown[..line].trim_end()
        } else {
            shown
        }
    }
}

pub fn elapsed_label(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs();
    if seconds >= 3600 {
        format!(
            "{}:{:02}:{:02}",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60
        )
    } else {
        format!("{}:{:02}", seconds / 60, seconds % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Draft, Generation, GenerationPhase, Phase, Recording, TranscriptionAttention,
        elapsed_label,
    };
    use std::time::{Duration, Instant};

    #[test]
    fn a_steady_level_is_held_and_a_real_change_shows() {
        let mut session = Recording::default();
        assert!(session.begin(1));
        session.set_audio_levels(0.6, 0.);
        assert_eq!(session.audio_level(), 0.6);
        // A steady sound's peak wobbles a little from poll to poll.
        for wobble in [0.603, 0.598, 0.61, 0.592, 0.6005] {
            session.set_audio_levels(wobble, 0.);
            assert_eq!(session.audio_level(), 0.6);
        }
        // Speech moves by far more than a step, and is followed at once.
        session.set_audio_levels(0.3, 0.);
        assert_eq!(session.audio_level(), 0.3);
        session.set_audio_levels(0.1, 0.45);
        assert_eq!(session.audio_level(), 0.45);
        // Silence is never held above zero.
        session.set_audio_levels(0., 0.);
        assert_eq!(session.audio_level(), 0.);
    }

    #[test]
    fn a_slow_fade_settles_onto_the_room_and_then_holds() {
        let mut session = Recording::default();
        assert!(session.begin(1));
        let start = Instant::now();
        let poll = Duration::from_millis(80);
        // Fading out in small steps, each under LEVEL_STEP: the shown level
        // lags up to a step behind.
        let mut at = start;
        for level in [0.2, 0.17, 0.14, 0.11, 0.08, 0.06, 0.05] {
            session.set_audio_levels_at(level, 0., at);
            at += poll;
        }
        assert!(session.audio_level() > 0.05);
        // The room stays at 0.05 with a little wobble: after half a second
        // the bars rest on it, and then stay put.
        for step in 0..12 {
            let wobble = if step % 2 == 0 { 0.052 } else { 0.048 };
            session.set_audio_levels_at(wobble, 0., at);
            at += poll;
        }
        let settled = session.audio_level();
        assert!((settled - 0.05).abs() < 0.005, "{settled}");
        for step in 0..12 {
            session.set_audio_levels_at(if step % 2 == 0 { 0.049 } else { 0.051 }, 0., at);
            at += poll;
            assert_eq!(session.audio_level(), settled);
        }
    }

    #[test]
    fn complete_workflow_keeps_owner_and_rejects_invalid_transitions() {
        let mut session = Recording::default();
        assert!(!session.stop());
        assert!(!session.pause());
        assert!(!session.resume());
        assert!(session.begin(42));
        let starting_epoch = session.epoch;
        assert!(!session.begin(9));
        assert!(session.activate(starting_epoch));
        assert!(session.is_active());
        assert!(!session.activate(starting_epoch));
        assert!(session.stop());
        let ending_epoch = session.epoch;
        assert!(!session.is_active());
        assert_eq!(session.note_id, Some(42));
        assert_eq!(session.phase, Phase::Complete);
        assert!(!session.fail(ending_epoch));
        assert!(session.resume());
        assert_eq!(session.note_id, Some(42));
    }

    #[test]
    fn elapsed_excludes_startup_pauses_and_processing() {
        let now = Instant::now();
        let mut session = Recording::default();
        assert!(session.begin(1));
        assert_eq!(
            session.elapsed_at(now + Duration::from_secs(5)),
            Duration::ZERO
        );
        assert!(session.activate_at(session.epoch, now));
        assert!(session.pause_at(now + Duration::from_secs(9)));
        assert_eq!(
            session.elapsed_at(now + Duration::from_secs(99)),
            Duration::from_secs(9)
        );
        assert!(!session.stop_at(now + Duration::from_secs(99)));
        assert!(session.resume());
        assert!(session.activate_at(session.epoch, now + Duration::from_secs(100)));
        assert!(session.stop_at(now + Duration::from_secs(107)));
        assert_eq!(
            session.elapsed_at(now + Duration::from_secs(200)),
            Duration::from_secs(16)
        );
        assert_eq!(
            session.elapsed_at(now + Duration::from_secs(300)),
            Duration::from_secs(16)
        );
    }

    #[test]
    fn stale_callbacks_cannot_resurrect_stopped_or_replaced_sessions() {
        let mut session = Recording::default();
        assert!(session.begin(1));
        let old_start = session.epoch;
        assert!(session.stop());
        assert!(!session.activate(old_start));
        let old_end = session.epoch;
        session.reset();
        assert!(session.begin(2));
        assert!(!session.activate(old_start));
        assert!(!session.fail(old_end));
        assert_eq!(session.note_id, Some(2));
        assert!(session.activate(session.epoch));
        assert_eq!(session.phase, Phase::Live);
    }

    #[test]
    fn failure_freezes_clock_and_retry_uses_a_new_epoch() {
        let now = Instant::now();
        let mut session = Recording::default();
        assert!(session.begin(3));
        assert!(session.activate_at(session.epoch, now));
        let previous = session.epoch;
        assert!(session.fail_at(previous, now + Duration::from_secs(12)));
        assert_eq!(
            session.elapsed_at(now + Duration::from_secs(60)),
            Duration::from_secs(12)
        );
        assert!(session.resume());
        assert_ne!(session.epoch, previous);
        assert!(!session.fail(previous));
        assert!(session.activate_at(session.epoch, now + Duration::from_secs(70)));
        assert_eq!(
            session.elapsed_at(now + Duration::from_secs(73)),
            Duration::from_secs(15)
        );
    }

    #[test]
    fn elapsed_labels_cover_minutes_and_hours() {
        assert_eq!(elapsed_label(Duration::from_secs(0)), "0:00");
        assert_eq!(elapsed_label(Duration::from_secs(65)), "1:05");
        assert_eq!(elapsed_label(Duration::from_secs(3661)), "1:01:01");
    }

    #[test]
    fn transcription_attention_preserves_startup_and_live_session() {
        let mut session = Recording::default();
        assert!(!session.set_transcription_attention(TranscriptionAttention::Warning));
        assert!(session.begin(7));
        let epoch = session.epoch;
        assert!(session.set_transcription_attention(TranscriptionAttention::Warning));
        assert!(session.is_active());
        assert_eq!(session.epoch, epoch);
        assert!(session.activate(epoch));
        assert_eq!(
            session.transcription_attention,
            TranscriptionAttention::Warning
        );
        assert_eq!(session.phase, Phase::Live);
        assert_eq!(session.note_id, Some(7));
        assert!(session.stop());
        assert_eq!(
            session.transcription_attention,
            TranscriptionAttention::None
        );
        assert!(!session.set_transcription_attention(TranscriptionAttention::Warning));
    }

    #[test]
    fn resume_and_reset_clear_transcription_attention() {
        let mut session = Recording::default();
        assert!(session.begin(7));
        assert!(session.activate(session.epoch));
        assert!(session.set_transcription_attention(TranscriptionAttention::Warning));
        assert!(session.pause());
        assert!(session.resume());
        assert_eq!(
            session.transcription_attention,
            TranscriptionAttention::None
        );
        assert!(session.set_transcription_attention(TranscriptionAttention::Warning));
        session.reset();
        assert_eq!(
            session.transcription_attention,
            TranscriptionAttention::None
        );
        assert!(session.begin(8));
        assert_eq!(
            session.transcription_attention,
            TranscriptionAttention::None
        );
    }
    #[test]
    fn stopping_capture_does_not_begin_generation() {
        let mut recording = Recording::default();
        let generation = Generation::default();
        assert!(recording.begin(7));
        assert!(recording.activate(recording.epoch));
        assert!(recording.stop());
        assert_eq!(recording.phase, Phase::Complete);
        assert_eq!(generation.phase, GenerationPhase::Idle);
        assert!(recording.resume());
        assert_eq!(recording.note_id, Some(7));
    }

    #[test]
    fn generation_requires_explicit_stages_and_matching_revision() {
        let mut generation = Generation::default();
        let epoch = generation.begin(7, 3);
        assert!(!generation.complete(epoch, 7, 3));
        assert!(generation.writing(epoch));
        assert!(!generation.complete(epoch, 7, 4));
        assert!(!generation.complete(epoch, 8, 3));
        assert!(generation.complete(epoch, 7, 3));
        assert_eq!(generation.phase, GenerationPhase::Ready);
    }

    #[test]
    fn a_draft_writes_itself_out_evenly_and_never_shows_a_bare_marker() {
        let mut draft = Draft::default();
        assert!(!draft.showing(7));
        draft.begin(7, 3);
        assert!(draft.of(3) && !draft.of(4) && draft.follow);
        draft.push("# Launch\n- Décision: lundi\n  - ");
        assert!(draft.showing(7) && !draft.showing(8));
        let start = Instant::now();
        assert!(draft.step(start));
        let mut seen = vec![draft.visible().to_owned()];
        for frame in 1..200 {
            draft.step(start + Duration::from_millis(16 * frame));
            let now = draft.visible().to_owned();
            // Only ever grows, always whole characters, never a bare marker.
            assert!(now.starts_with(seen.last().unwrap().as_str()));
            let last = now.lines().last().unwrap_or("");
            assert!(last.is_empty() || last.chars().any(char::is_alphanumeric), "{now:?}");
            seen.push(now);
        }
        assert!(draft.caught_up());
        // The dangling "  - " waits for its first word.
        assert_eq!(draft.visible(), "# Launch\n- Décision: lundi");
        // It took more than a frame or two: the text was paced.
        assert!(seen.iter().filter(|text| text.len() < 10).count() > 2);
        draft.push("Après la revue");
        for frame in 200..300 {
            draft.step(start + Duration::from_millis(16 * frame));
        }
        assert_eq!(draft.visible(), "# Launch\n- Décision: lundi\n  - Après la revue");
    }

    #[test]
    fn a_large_backlog_is_shown_within_half_a_second() {
        let mut draft = Draft::default();
        draft.begin(1, 1);
        draft.push(&"word ".repeat(2000));
        let start = Instant::now();
        for frame in 0..40 {
            draft.step(start + Duration::from_millis(16 * frame));
        }
        assert!(draft.caught_up());
    }

    #[test]
    fn the_saved_notes_continue_the_draft_or_replace_it() {
        let mut draft = Draft::default();
        draft.begin(1, 1);
        draft.push("# A\n- one");
        let start = Instant::now();
        for frame in 0..100 {
            draft.step(start + Duration::from_millis(16 * frame));
        }
        // The saved notes add the link line: what is shown stays, the rest follows.
        draft.finish("# A\n- one\n\n---\n\nChat with meeting transcript: https://notes.example/notes/1/chat");
        assert!(draft.complete && !draft.caught_up());
        assert_eq!(draft.visible(), "# A\n- one");
        // Notes that are not a continuation are simply the notes.
        draft.finish("# B\n- two");
        assert!(draft.caught_up());
        assert_eq!(draft.visible(), "# B\n- two");
        // A server that does not stream: what was shown is dropped.
        draft.restart();
        assert!(!draft.showing(1) && draft.visible().is_empty());
    }

    #[test]
    fn resume_navigation_or_replacement_can_cancel_pending_generation() {
        let mut generation = Generation::default();
        let old_epoch = generation.begin(7, 3);
        generation.cancel();
        assert!(!generation.writing(old_epoch));
        let current_epoch = generation.begin(8, 1);
        assert!(generation.writing(current_epoch));
        assert!(!generation.complete(old_epoch, 7, 3));
        generation.cancel();
        assert!(!generation.complete(current_epoch, 8, 1));
    }
}
