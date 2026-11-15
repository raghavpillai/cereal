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
    pub note_id: Option<u64>,
    /// Captured by delayed transitions. Invalidated by user transitions/reset.
    pub epoch: u64,
    accumulated: Duration,
    live_since: Option<Instant>,
}

impl Recording {
    /// Level of actual capture inputs. Silence stays quiet; no demo waveform is
    /// inferred from elapsed time. Capture/server adapters supply these values.
    pub fn audio_level(&self) -> f32 {
        if self.is_active() {
            self.microphone_level.max(self.system_level)
        } else {
            0.
        }
    }

    pub fn set_audio_levels(&mut self, microphone: f32, system: f32) {
        let clean = |level: f32| {
            if level.is_finite() {
                level.clamp(0., 1.)
            } else {
                0.
            }
        };
        self.microphone_level = clean(microphone);
        self.system_level = clean(system);
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
        Generation, GenerationPhase, Phase, Recording, TranscriptionAttention, elapsed_label,
    };
    use std::time::{Duration, Instant};

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
