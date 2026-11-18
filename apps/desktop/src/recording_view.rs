use crate::motion::GOOEY;
use crate::{
    app::{Cereal, NoteTab},
    recording::{GenerationPhase, Phase, TranscriptionAttention, elapsed_label},
    ui::*,
};
use gpui::{prelude::*, *};
use std::time::{Duration, Instant};

impl Cereal {
    fn save_recording_preview(&mut self, cx: &mut Context<Self>) {
        let baseline = self.recording_transcript_start.clone();
        if let Some(note) = self
            .recording
            .note_id
            .and_then(|id| self.workspace.note_mut(id))
        {
            if let Some((revision, transcript)) = baseline {
                // Legacy adapters may assign the string directly. Typed adapters
                // should use replace/append_transcript, which already advance it.
                if note.transcript_revision == revision && note.transcript != transcript {
                    note.normalize_generation_revision();
                    note.transcript_revision = note.transcript_revision.saturating_add(1);
                }
            }
            note.duration = elapsed_label(self.recording.elapsed());
        }
        self.persist(cx);
    }

    /// Preserve capture state before assigning a new recording owner. This never
    /// generates notes, and invalidates generation callbacks from the old context.
    pub fn finish_recording_preview(&mut self, cx: &mut Context<Self>) {
        let stopped = self
            .recording
            .note_id
            .filter(|_| crate::server::enabled() && self.recording.is_active());
        if let Some(capture) = self.audio_capture.take() {
            capture.request_stop();
        }
        // The live transcript stream sends what is left; the server is told
        // the recording stopped once it has all of it (at the next launch,
        // if the app is quitting now).
        if stopped.is_none() {
            self.live_end();
        }
        if self.recording.note_id.is_some() {
            self.recording.stop();
            self.save_recording_preview(cx);
        }
        if let Some(id) = stopped {
            self.live_stop(id, crate::model::PendingStop::Stop, cx);
        }
        self.recording.reset();
        self.recording_transcript_start = None;
        self.generation.cancel();
    }

    fn recording_click_allowed(&mut self, stop: bool) -> bool {
        let now = Instant::now();
        if self
            .recording_changed_at
            .is_some_and(|at| now.duration_since(at) < Duration::from_millis(300))
        {
            return false;
        }
        if self.recording_last_click.is_some_and(|(action, at)| {
            action == stop && now.duration_since(at) < Duration::from_secs(2)
        }) {
            return false;
        }
        self.recording_last_click = Some((stop, now));
        true
    }

    pub fn start_recording_preview(&mut self, id: u64, cx: &mut Context<Self>) {
        if self.audio_quitting {
            return;
        }
        if !self.recording_click_allowed(false) {
            return;
        }
        if self.recording.is_active() {
            self.show_toast("A recording is already active in another note", cx);
            return;
        }
        self.recording_from = if self.recording.note_id == Some(id) {
            self.recording.phase
        } else if self
            .workspace
            .note(id)
            .is_some_and(|n| !n.transcript.is_empty())
        {
            Phase::Complete
        } else {
            Phase::Idle
        };
        let resumed = self.recording.note_id == Some(id)
            && matches!(
                self.recording.phase,
                Phase::Held | Phase::Complete | Phase::Failed
            );
        let changed = if resumed {
            self.recording.resume()
        } else {
            self.recording.begin(id)
        };
        if !changed {
            return;
        }
        self.generation.cancel();
        self.regeneration_notice_dismissed = None;
        if let Some(note) = self.workspace.note_mut(id) {
            // Recording again: a Stop still waiting for the last capture to
            // be delivered is no longer wanted (that capture is still
            // delivered, before the new one streams).
            note.pending_stop = None;
            note.normalize_generation_revision();
            self.recording_transcript_start =
                Some((note.transcript_revision, note.transcript.clone()));
        }
        self.recording_changed_at = Some(Instant::now());
        // The one confirmation that it is now listening.
        crate::haptics::play(crate::haptics::Haptic::LevelChange);
        let epoch = self.recording.epoch;
        cx.notify();
        crate::latency::mark("ui_state");
        crate::latency::on_paint("ui_painted");
        if crate::audio::preview_mode() {
            self.apply_capture_choices();
        }
        if !crate::audio::preview_mode() {
            if crate::server::enabled() {
                // The live stream starts with the capture and opens its socket
                // the moment the server answers this (`RecordingSignal`).
                crate::server::recording_signal(id, crate::server::RecordingSignal::Waiting);
                if !self.server_action_sent(
                    id,
                    crate::server::Action::Recording(if resumed { "resume" } else { "start" }),
                    cx,
                ) {
                    crate::server::recording_signal_settled(id);
                }
            }
            if !resumed {
                let seconds: f64 = self
                    .workspace
                    .note(id)
                    .map(|note| {
                        let local: f64 = note
                            .audio_captures
                            .iter()
                            .map(|capture| capture.audio_seconds())
                            .sum();
                        if crate::server::enabled() {
                            crate::server::duration_seconds(&note.duration)
                                .unwrap_or(local)
                                .max(local)
                        } else {
                            local
                        }
                    })
                    .unwrap_or(0.);
                self.recording
                    .restore_elapsed(Duration::try_from_secs_f64(seconds).unwrap_or_default());
            }
            self.start_real_audio(id, epoch, cx);
            crate::latency::mark("handler_done");
            return;
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(450))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.recording.activate(epoch) {
                    cx.notify();
                }
            });
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let Ok(keep) = this.update(cx, |this, cx| {
                    let keep = this.recording.epoch == epoch && this.recording.is_active();
                    if keep {
                        cx.notify();
                    }
                    keep
                }) else {
                    break;
                };
                if !keep {
                    break;
                }
            }
        })
        .detach();
    }

    pub fn stop_recording_preview(&mut self, cx: &mut Context<Self>) {
        if !self.recording_click_allowed(true) {
            return;
        }
        self.recording_from = self.recording.phase;
        if !self.recording.stop() {
            return;
        }
        crate::haptics::play(crate::haptics::Haptic::Generic);
        crate::latency::mark("ui_state");
        crate::latency::on_paint("ui_painted");
        if let Some(capture) = self.audio_capture.take() {
            capture.request_stop();
        }
        if let Some(id) = self.recording.note_id {
            // The person stopped it, and may only be pausing: the note stays
            // as it is, on their own notes, with "Generate notes" above the
            // bar and "Resume" in the capsule. The stream delivers what the
            // server does not have yet and the server is told the recording
            // stopped; the notes are written when they are asked for.
            // Leaving the note, quitting or a capture that fails stop the
            // same way (`finish_recording_preview`, `PendingStop::Stop`):
            // nothing in this app ends a meeting by itself yet, so notes are
            // only ever written when asked for.
            self.live_stop(id, crate::model::PendingStop::Stop, cx);
            self.recording_stopped_by_person(id, cx);
        } else {
            self.live_end();
        }
        self.recording_changed_at = Some(Instant::now());
        self.menu = None;
        self.transcript_open = false;
        self.transcript_find_open = false;
        self.save_recording_preview(cx);
        cx.notify();
        crate::latency::mark("handler_done");
    }

    fn start_real_audio(&mut self, id: u64, epoch: u64, cx: &mut Context<Self>) {
        if let Some(capture) = self.audio_capture.take() {
            capture.request_stop();
        }
        self.audio_error = None;
        let mut options = crate::audio::CaptureOptions::production();
        if self.microphone_muted {
            options.microphone = false;
        }
        // The microphone chosen in the transcript panel; the language goes
        // to the server with the stream.
        self.apply_capture_choices();
        crate::latency::mark("choices_applied");
        // The capture is kept in the directory of the note's account.
        let result = crate::model::audio_directory_of(id)
            .and_then(|directory| crate::audio::Capture::start(id, &directory, options));
        let capture = match result {
            Ok(capture) => capture,
            Err(error) => {
                self.recording.fail(epoch);
                self.audio_error = Some(error.clone());
                self.show_toast(&error, cx);
                return;
            }
        };
        crate::latency::mark("capture_spawned");
        let monitor = capture.monitor();
        // Start and Resume both begin a new capture; the live transcript
        // stream follows it once the server has the note recording.
        self.live_prepare(id, monitor.clone());
        self.audio_pending.push(monitor.clone());
        let connect = crate::server::enabled();
        let initial = monitor.snapshot();
        if let Some(note) = self.workspace.note_mut(id) {
            note.audio_captures.push(initial.clone());
            // Unsent until the server has confirmed its Stop, and owed to
            // the server until its stream has delivered all of it.
            note.unsent_capture = crate::server::enabled();
            if crate::server::enabled() {
                note.live_pending.push(initial.capture_id.clone());
            }
        }
        self.audio_capture = Some(capture);
        // Persist the immutable session directory association before any speech
        // arrives, so shutdown cannot orphan audio from its owner note.
        self.persist(cx);
        if connect {
            self.live_connect(id, cx);
        }
        cx.spawn(async move |this, cx| {
            let mut held = false;
            // Once a frame until the first audio has arrived (so the capsule
            // turns live the frame after), then for the level bars.
            let mut poll = Duration::from_millis(16);
            loop {
                cx.background_executor().timer(poll).await;
                let snapshot = monitor.snapshot();
                let completed = snapshot.completed();
                let Ok(keep) = this.update(cx, |this, cx| {
                    // The capture is local: it carries on whatever happens
                    // to the account's session, for as long as its note is
                    // here (a recording must be stopped before its account
                    // can be signed out of or removed).
                    if this.workspace.note(id).is_none() {
                        return false;
                    }
                    // A recording put on hold pauses its live transcript at
                    // the sample it was held at, and resumes from there.
                    let now_held = this.recording.note_id == Some(id)
                        && this.recording.phase == Phase::Held;
                    if now_held != held {
                        held = now_held;
                        this.live_hold(held);
                    }
                    let current = this.audio_capture.as_ref().is_some_and(|capture| {
                        capture.monitor().snapshot().capture_id == initial.capture_id
                    });
                    let owner = this.recording.note_id == Some(id) && this.recording.epoch == epoch;
                    // What the window shows of this poll: the bars (whole
                    // pixels), the phase and the error. Redraw only if one
                    // of them changed.
                    let shown = |this: &Self| {
                        (
                            waveform_heights(this.recording.audio_level()),
                            crate::pill::voice_bar_targets(this.recording.audio_level()),
                            this.recording.phase,
                            this.audio_error.clone(),
                        )
                    };
                    let before = shown(this);
                    if snapshot.stats.has_audio() || completed {
                        poll = Duration::from_millis(80);
                    }
                    if current && owner && this.recording.is_active() {
                        if snapshot.stats.has_audio() && this.recording.phase == Phase::Starting {
                            crate::latency::mark("ui_live");
                            crate::latency::on_paint("ui_live_painted");
                            this.recording.activate(epoch);
                            this.recording_went_live(id, epoch, snapshot.error_message());
                        }
                        this.recording.set_audio_levels(
                            snapshot.stats.mic.level,
                            snapshot.stats.system.level,
                        );
                        if let Some(error) = snapshot.error_message()
                            && this.audio_error.as_ref() != Some(&error)
                        {
                            this.notify_recording_lost(id, epoch, &error, cx);
                            this.audio_error = Some(error);
                        }
                    }
                    if completed {
                        crate::latency::mark("capture_finished_seen");
                        this.live_capture_finished(&snapshot.capture_id);
                        this.audio_pending
                            .retain(|pending| pending.snapshot().capture_id != snapshot.capture_id);
                        if let Some(note) = this.workspace.note_mut(id) {
                            if let Some(saved) = note
                                .audio_captures
                                .iter_mut()
                                .find(|saved| saved.capture_id == snapshot.capture_id)
                            {
                                *saved = snapshot.clone();
                            }
                            let seconds: f64 = note
                                .audio_captures
                                .iter()
                                .map(|capture| capture.audio_seconds())
                                .sum();
                            note.duration = elapsed_label(
                                Duration::try_from_secs_f64(seconds).unwrap_or_default(),
                            );
                        }
                        if current {
                            this.audio_capture = None;
                            if owner && this.recording.is_active() && this.recording.fail(epoch) {
                                this.notify_recording_stopped(id, epoch, snapshot.error_message().as_deref());
                            }
                        }
                        if (owner
                            || (this.audio_capture.is_none()
                                && this.recording.note_id == Some(id)
                                && this.recording.phase == Phase::Complete))
                            && let Some(error) = snapshot.error_message()
                        {
                            this.audio_error = Some(error);
                        }
                        this.persist(cx);
                        // A finished capture is on disk before anything else.
                        this.flush_edits(cx);
                        return false;
                    }
                    if shown(this) != before {
                        cx.notify();
                    }
                    true
                }) else {
                    break;
                };
                if !keep {
                    break;
                }
            }
        })
        .detach();
    }

    pub fn quit_after_audio_stop(&mut self, cx: &mut Context<Self>) {
        if self.audio_quitting {
            return;
        }
        self.audio_quitting = true;
        let monitors = self.audio_pending.clone();
        self.finish_recording_preview(cx);
        cx.spawn(async move |this, cx| {
            let started = Instant::now();
            while monitors.iter().any(|monitor| !monitor.snapshot().completed())
                && started.elapsed() < Duration::from_secs(10) {
                cx.background_executor().timer(Duration::from_millis(10)).await;
            }
            let _ = this.update(cx, |this, cx| {
                if monitors.iter().any(|monitor| !monitor.snapshot().completed()) {
                    this.audio_quitting = false;
                    this.show_toast("Audio is still finalizing. Wait for the files to finish saving, then quit again.", cx);
                } else { cx.quit(); }
            });
        }).detach();
    }

    /// Explicit generation action. Server work uses the real provider; the local
    /// outline remains available only in the separate local/preview workspace.
    pub fn enhance_note_preview(&mut self, id: u64, cx: &mut Context<Self>) {
        if self.recording.note_id == Some(id) && self.recording.is_active() {
            self.show_toast("Stop the recording before generating notes", cx);
            return;
        }
        if self.generation.is_running_for(id) {
            return;
        }
        if crate::server::enabled() {
            self.active_tab = NoteTab::Enhanced;
            // A capture still being delivered is waited for: the notes are
            // asked for once the server has all of it.
            let owed = self.live.delivering(id)
                || self.workspace.note(id).is_some_and(|note| !note.live_pending.is_empty());
            if owed {
                if let Some(note) = self.workspace.note_mut(id) {
                    note.pending_stop = Some(crate::model::PendingStop::Generate);
                }
                self.live_deliver(id, None, cx);
                cx.notify();
                return;
            }
            self.notifications_first_need(crate::notifications::Kind::Notes, cx);
            self.server_action(id, crate::server::Action::Generate, cx);
            return;
        }
        let Some(note) = self.workspace.note(id) else {
            return;
        };
        let body = note.body.clone();
        let transcript = note.transcript.clone();
        let revision = note.transcript_revision;
        let source = outline_source(&body, &transcript);
        let outline = crate::note::local_outline(&source);
        if outline.is_empty() {
            self.show_toast("Add notes or a transcript before generating", cx);
            return;
        }
        let epoch = self.generation.begin(id, revision);
        self.regeneration_notice_dismissed = None;
        self.active_tab = NoteTab::Enhanced;
        self.summary.dismiss();
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(650))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.generation.writing(epoch) {
                    cx.notify();
                }
            });
            cx.background_executor()
                .timer(Duration::from_millis(850))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.generation.epoch != epoch {
                    return;
                }
                let unchanged = this.workspace.note(id).is_some_and(|note| {
                    note.transcript_revision == revision
                        && note.body == body
                        && note.transcript == transcript
                        && !note.trashed
                });
                if !unchanged {
                    this.generation.cancel();
                    cx.notify();
                    return;
                }
                if !this.generation.complete(epoch, id, revision) {
                    return;
                }
                if let Some(note) = this.workspace.note_mut(id) {
                    note.enhanced = outline;
                    note.generated_revision = Some(revision);
                }
                this.persist(cx);
            });
        })
        .detach();
    }

    /// The generation `epoch` of note `id` has said something (see
    /// [`crate::server::GenerateProgress`]). `submitted_title` is the title
    /// the note had when the notes were asked for.
    pub fn generation_progress(
        &mut self,
        id: u64,
        epoch: u64,
        submitted_title: &str,
        event: crate::server::GenerateProgress,
        cx: &mut Context<Self>,
    ) {
        use crate::server::GenerateProgress;
        if self.generation.epoch != epoch || !self.generation_draft.of(epoch) {
            return;
        }
        match event {
            GenerateProgress::Began => {
                self.generation.writing(epoch);
            }
            // The name arrives before the notes, and is shown at once. Only
            // a note nobody has named takes it (`adopt_server_title`).
            GenerateProgress::Title(title) => {
                let named = self
                    .workspace
                    .note_mut(id)
                    .is_some_and(|note| note.adopt_server_title(submitted_title, &title));
                if named && self.current_note == Some(id) {
                    self.title_input
                        .update(cx, |input, cx| input.set_content(title, cx));
                }
            }
            GenerateProgress::Delta(text) => {
                self.generation.writing(epoch);
                if !self.generation_draft.showing(id) {
                    // The first words: the source card of the old notes,
                    // if one is open, has nothing left to point at.
                    self.summary.dismiss();
                }
                self.generation_draft.push(&text);
            }
            GenerateProgress::Buffered => self.generation_draft.restart(),
        }
        cx.notify();
    }

    /// Write a little more of the draft of generation `epoch` onto the
    /// page, and keep the page on its last line unless the person has
    /// scrolled away. False once there is nothing more to write: the
    /// generation was cancelled, or its saved notes are all shown (the
    /// note's own text takes over from the draft, with the same words in
    /// the same place).
    pub fn generation_draft_step(&mut self, epoch: u64, cx: &mut Context<Self>) -> bool {
        if !self.generation_draft.of(epoch) {
            return false;
        }
        if self.generation.epoch != epoch {
            self.generation_draft.clear();
            cx.notify();
            return false;
        }
        let changed = self.generation_draft.step(Instant::now());
        let on_page = self.current_note == self.generation_draft.note_id
            && self.active_tab == NoteTab::Enhanced;
        if on_page && self.follow_draft() {
            cx.notify();
        }
        if self.generation_draft.complete && self.generation_draft.caught_up() {
            self.generation_draft.clear();
            cx.notify();
            return false;
        }
        if changed {
            cx.notify();
        }
        true
    }

    /// Ease the note's page down to the end of the draft. The person
    /// scrolling anywhere else stops it; scrolling back to the end takes it
    /// up again. True if the page moved.
    fn follow_draft(&mut self) -> bool {
        let offset = -f32::from(self.note_scroll.offset().y);
        let end = f32::from(self.note_scroll.max_offset().height);
        let draft = &mut self.generation_draft;
        if draft
            .scrolled_to
            .is_some_and(|put| (offset - put).abs() > 2.)
        {
            draft.follow = offset >= end - 4.;
        }
        if !draft.follow || end - offset < 0.5 {
            draft.scrolled_to = Some(offset);
            return false;
        }
        // A quarter of the way each frame, never less than a point.
        let next = (offset + ((end - offset) * 0.25).max(1.)).min(end);
        draft.scrolled_to = Some(next);
        self.note_scroll.set_offset(point(px(0.), px(-next)));
        true
    }

    /// For the tour's stills and the smoke test: write `notes` into note
    /// `id` as a streamed generation would, from a script instead of a
    /// server. `hold_at` stops the script after that many characters (a
    /// still of the notes mid-stream); `None` writes them all and saves them.
    #[cfg(debug_assertions)]
    pub fn start_scripted_generation(
        &mut self,
        id: u64,
        title: Option<&str>,
        notes: &str,
        hold_at: Option<usize>,
        cx: &mut Context<Self>,
    ) {
        use crate::server::GenerateProgress;
        let Some(note) = self.workspace.note(id) else {
            return;
        };
        let (revision, submitted_title) = (note.transcript_revision, note.title.clone());
        let epoch = self.generation.begin(id, revision);
        self.generation_draft.begin(id, epoch);
        self.active_tab = NoteTab::Enhanced;
        self.summary.dismiss();
        let (sender, progress) = std::sync::mpsc::channel();
        let (title, notes) = (title.map(str::to_owned), notes.to_owned());
        let script = notes.clone();
        std::thread::spawn(move || {
            let _ = sender.send(GenerateProgress::Began);
            std::thread::sleep(crate::motion::slow(Duration::from_millis(500)));
            if let Some(title) = title {
                let _ = sender.send(GenerateProgress::Title(title));
            }
            let mut sent = 0;
            for word in script.split_inclusive(' ') {
                if hold_at.is_some_and(|limit| sent + word.chars().count() > limit) {
                    // Held: the channel stays open, so the draft stays up.
                    std::thread::sleep(Duration::from_secs(600));
                    return;
                }
                sent += word.chars().count();
                if sender.send(GenerateProgress::Delta(word.into())).is_err() {
                    return;
                }
                std::thread::sleep(crate::motion::slow(Duration::from_millis(14)));
            }
        });
        cx.notify();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;
                let mut ended = false;
                let events: Vec<GenerateProgress> = loop {
                    let mut events = Vec::new();
                    loop {
                        match progress.try_recv() {
                            Ok(event) => events.push(event),
                            Err(std::sync::mpsc::TryRecvError::Empty) => break,
                            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                                ended = true;
                                break;
                            }
                        }
                    }
                    break events;
                };
                let Ok(live) = this.update(cx, |this, cx| {
                    for event in events {
                        this.generation_progress(id, epoch, &submitted_title, event, cx);
                    }
                    if ended && this.generation.epoch == epoch && !this.generation_draft.complete {
                        if let Some(note) = this.workspace.note_mut(id) {
                            note.enhanced = notes.clone();
                            note.generated_revision = Some(revision);
                        }
                        this.generation.writing(epoch);
                        this.generation.complete(epoch, id, revision);
                        this.generation_draft.finish(&notes);
                        this.persist(cx);
                    }
                    this.generation_draft_step(epoch, cx)
                }) else {
                    break;
                };
                if !live {
                    break;
                }
            }
        })
        .detach();
    }

    /// The live account uses the legacy floating `m_n` controls, not the
    /// feature-flagged 32 px `yHr` header capsule. Its layout changes naturally.
    pub fn render_recording_control(&self, id: u64, cx: &mut Context<Self>) -> AnyElement {
        self.recording_control(id, false, cx)
    }

    /// The same control in the transcript panel's footer: the panel is its
    /// surface, so it draws none of its own, whether the panel is open,
    /// opening or closing.
    pub fn render_panel_recording_control(&self, id: u64, cx: &mut Context<Self>) -> AnyElement {
        self.recording_control(id, true, cx)
    }

    /// How wide the note bar's record capsule is while the transcript is
    /// closed: where the bar divides when the panel grows out of it.
    pub fn closed_capsule_width(&self, id: u64, cx: &mut Context<Self>) -> f32 {
        let owned = self.recording.note_id == Some(id);
        let phase = if owned {
            self.recording.phase
        } else if self
            .workspace
            .note(id)
            .is_some_and(|n| !n.transcript.is_empty())
        {
            Phase::Complete
        } else {
            Phase::Idle
        };
        let live = matches!(phase, Phase::Starting | Phase::Live);
        if self.generation.is_running_for(id) {
            return COMPACT_CAPSULE;
        }
        if !live
            && self
                .workspace
                .note(id)
                .is_some_and(|note| !note.enhanced.trim().is_empty())
        {
            return COMPACT_CAPSULE;
        }
        let face = cx.text_system().resolve_font(&font(FONT));
        let text_width = |label: &str| -> f32 {
            label
                .chars()
                .map(|character| {
                    cx.text_system()
                        .advance(face, px(13.), character)
                        .map_or(7., |size| f32::from(size.width))
                })
                .sum()
        };
        let scheduled = if phase == Phase::Idle {
            self.workspace
                .note(id)
                .and_then(|n| n.meeting_context.as_ref())
                .and_then(|meeting| crate::meeting::start_label(meeting, crate::menubar::now()))
        } else {
            None
        };
        if let Some(label) = &scheduled {
            scheduled_capsule_width(text_width(label), false)
        } else if live {
            LIVE_CAPSULE
        } else {
            labelled_capsule_width(text_width(match phase {
                Phase::Idle => "Start now",
                Phase::Held | Phase::Failed => "Retry",
                _ => "Resume",
            }))
        }
    }

    fn recording_control(&self, id: u64, embedded: bool, cx: &mut Context<Self>) -> AnyElement {
        let t = self.theme();
        let owned = self.recording.note_id == Some(id);
        let phase = if owned {
            self.recording.phase
        } else if self
            .workspace
            .note(id)
            .is_some_and(|n| !n.transcript.is_empty())
        {
            Phase::Complete
        } else {
            Phase::Idle
        };
        // A meeting on the calendar that has not been recorded: the control
        // says when it starts (or that it has started) instead of offering a
        // recording, and "Start now" is in the transcript panel it opens.
        let scheduled = if phase == Phase::Idle {
            self.workspace
                .note(id)
                .and_then(|n| n.meeting_context.as_ref())
                .and_then(|meeting| crate::meeting::start_label(meeting, crate::menubar::now()))
        } else {
            None
        };
        let live = matches!(phase, Phase::Starting | Phase::Live);
        let attention = owned
            && live
            && self.recording.transcription_attention == TranscriptionAttention::Warning;
        let processing = self.generation.is_running_for(id);
        // The reference's faint highlight: 6% warm ink, or 4% white in dark.
        let dark = t.bg.l < 0.5;
        let hover: Hsla = if dark {
            rgba(0xffffff0a).into()
        } else {
            rgba(0x625a220f).into()
        };
        let open = self.transcript_open;
        let compact_complete = !open
            && !live
            && self
                .workspace
                .note(id)
                .is_some_and(|note| !note.enhanced.trim().is_empty());
        // A control after the indicator (Stop, Resume, Start now or Retry)
        // joins it: the two share the capsule, each rounded concentrically
        // on its outer side and tightly where they meet.
        let joined = live || (!processing && !compact_complete && (scheduled.is_none() || open));
        let leading_label: SharedString = if open {
            "Hide transcript"
        } else {
            "Show transcript"
        }
        .into();
        let leading = div()
            .id("show-transcript-button")
            .relative()
            .w(px(if joined {
                LEADING_JOINED
            } else {
                LEADING_ALONE
            }))
            .h(px(TILE))
            .flex_shrink_0()
            .map(|d| {
                if joined {
                    d.rounded_l(px(TILE_OUTER_RADIUS))
                        .rounded_r(px(TILE_INNER_RADIUS))
                } else {
                    d.rounded_full()
                }
            })
            .reach()
            .cursor(CursorStyle::Arrow)
            .child(if joined {
                crate::motion::tactile(hover).corners(
                    TILE_OUTER_RADIUS,
                    TILE_INNER_RADIUS,
                    TILE_INNER_RADIUS,
                    TILE_OUTER_RADIUS,
                )
            } else {
                crate::motion::tactile(hover).round()
            })
            .child(
                div()
                    .absolute()
                    .left(px(0.))
                    .top(px(0.))
                    .size_full()
                    .child({
                        // Spinner, waveform, lock and transcript glyph hand over
                        // with a soft rise instead of swapping in place.
                        // It animates only when the phase really changes,
                        // never because this capsule was just mounted.
                        let shown = if attention { Phase::Starting } else { phase };
                        crate::motion::swapped(
                            div().size_full().child(footer_indicator(
                                shown,
                                t,
                                self.recording.audio_level(),
                                joined,
                            )),
                            self.stage.swap(("footer-indicator", id), shown as u64),
                        )
                    }),
            )
            .child(
                icon("footer-chevron", 16., t.muted)
                    .absolute()
                    .left(px(chevron_left(joined)))
                    .top(px((TILE - 16.) / 2.))
                    // One rotation value for the window, shared by the
                    // footer and in-panel capsules: it flips once when the
                    // transcript opens or closes and rests on every mount.
                    // The spring swings past either end of the half turn; a
                    // turn is periodic, so wrap into `percentage`'s range.
                    .with_transformation(Transformation::rotate(percentage(
                        self.stage
                            .aim(
                                ("transcript-chevron", id),
                                if open { 0. } else { 0.5 },
                                GOOEY,
                            )
                            .rem_euclid(1.),
                    ))),
            )
            .map(|d| self.bar_tip(d, TIP_TRANSCRIPT, leading_label.clone(), t, cx))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.transcript_open = !this.transcript_open;
                    this.transcript_epoch = this.transcript_epoch.wrapping_add(1);
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            // Keep a single hit target above the animated indicator and SVG.
            // Those painted children must not make the waveform less clickable
            // than the adjacent chevron.
            .child(
                div()
                    .id("transcript-whole-button-hit-target")
                    .absolute()
                    .inset_0()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.focus.focus(window);
                        this.transcript_open = !this.transcript_open;
                        this.transcript_epoch = this.transcript_epoch.wrapping_add(1);
                        cx.notify();
                    })),
            );
        // Labels as they are drawn: 13 px in the body face.
        let face = cx.text_system().resolve_font(&font(FONT));
        let text_width = |label: &str| -> f32 {
            label
                .chars()
                .map(|character| {
                    cx.text_system()
                        .advance(face, px(13.), character)
                        .map_or(7., |size| f32::from(size.width))
                })
                .sum()
        };
        let trailing_label: SharedString = match phase {
            Phase::Idle => "Start now".into(),
            Phase::Held | Phase::Failed => "Retry".into(),
            _ => "Resume".into(),
        };
        let capsule_target = if processing || compact_complete {
            COMPACT_CAPSULE
        } else if let Some(label) = &scheduled {
            scheduled_capsule_width(text_width(label), open)
        } else if live {
            LIVE_CAPSULE
        } else {
            labelled_capsule_width(text_width(&trailing_label))
        };
        let capsule_width = self
            .stage
            .aim(("recording-capsule", id), capsule_target, GOOEY)
            .max(COMPACT_CAPSULE);
        // The label only has room once the capsule is most of the way open, so
        // it fades in over the second half of the morph instead of being cut.
        let label_fade = if capsule_target > COMPACT_CAPSULE {
            crate::motion::fade_curve(
                ((capsule_width - COMPACT_CAPSULE) / (capsule_target - COMPACT_CAPSULE) - 0.5)
                    / 0.5,
            )
        } else {
            1.
        };
        let mut capsule = div()
            .id("recording-capsule")
            .relative()
            .h(px(CAPSULE_HEIGHT))
            // Compact, expanded and processing are one capsule changing shape:
            // its width rides a spring between states (per note, so opening a
            // different note never animates).
            .w(px(capsule_width))
            .flex_shrink_0()
            .p(px(CAPSULE_INSET))
            .flex()
            .items_center()
            .rounded(px(CAPSULE_HEIGHT / 2.))
            .bg(t.card)
            .when(!embedded && !attention, |s| bar_edge(s, t))
            .when(attention && !embedded, |s| {
                // The warning tint has its own inset edge instead of the ring.
                s.bg(rgba(0xed921233))
                    .shadow(vec![BoxShadow {
                        color: rgba(0x0000000d).into(),
                        offset: point(px(0.), px(4.)),
                        blur_radius: px(6.),
                        spread_radius: px(-2.),
                    }])
                    .child(edge(crate::ui::hairline(0.5), 0., t.warning_edge))
                    .text_color(t.warning_ink)
            })
            // In the panel's footer the panel is the surface, whether it is
            // open, opening or on its way shut.
            .when(embedded, |s| {
                s.bg(rgba(0x00000000))
                    .border_color(rgba(0x00000000))
                    .shadow(Vec::<BoxShadow>::new())
            })
            .child(leading);
        if live {
            capsule = capsule.child(
                div()
                    .id("stop-transcript-button")
                    .relative()
                    .w(px(TRAILING_MIN))
                    .h(px(TILE))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .pr(px(TRAILING_END))
                    .rounded_l(px(TILE_INNER_RADIUS))
                    .rounded_r(px(TILE_OUTER_RADIUS))
                    .child(crate::motion::tactile(hover).corners(
                        TILE_INNER_RADIUS,
                        TILE_OUTER_RADIUS,
                        TILE_OUTER_RADIUS,
                        TILE_INNER_RADIUS,
                    ))
                    .child(icon("footer-stop", 16., t.muted))
                    .map(|d| self.bar_tip(d, TIP_STOP, "Stop recording".into(), t, cx))
                    .reach()
                    .focusable()
                    .cursor(CursorStyle::Arrow)
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            this.stop_recording_preview(cx);
                            cx.stop_propagation();
                        }
                    }))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.focus.focus(window);
                        this.stop_recording_preview(cx);
                    })),
            );
        } else if !processing && !compact_complete {
            if let Some(label) = scheduled.clone() {
                // Not a button of its own: pressing it opens or closes the
                // transcript, like the indicator beside it.
                capsule = capsule.child(
                    div()
                        .id("scheduled-start-label")
                        .h(px(48.))
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .pr(px(SCHEDULED_LABEL_GAP))
                        .text_size(px(13.))
                        .line_height(px(16.))
                        .text_color(t.secondary)
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .cursor(CursorStyle::Arrow)
                        .opacity(label_fade)
                        .child(label)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.focus.focus(window);
                            this.transcript_open = !this.transcript_open;
                            this.transcript_epoch = this.transcript_epoch.wrapping_add(1);
                            cx.notify();
                        })),
                );
            }
            let label = trailing_label.clone();
            // A scheduled meeting offers "Start now" only in the open panel.
            if scheduled.is_none() || open {
            capsule = capsule.child(
                div()
                    .id("start-transcript-button")
                    .relative()
                    .h(px(TILE))
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .pr(px(TRAILING_END))
                    .rounded_l(px(TILE_INNER_RADIUS))
                    .rounded_r(px(TILE_OUTER_RADIUS))
                    .text_size(px(13.))
                    .line_height(px(16.))
                    .text_color(t.accent)
                    // Keep the label on one line while the capsule is mid-morph.
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .cursor(CursorStyle::Arrow)
                    .opacity(if processing { 0.5 } else { label_fade })
                    .when(!processing, |s| {
                        s.focusable()
                            .child(crate::motion::tactile(hover).corners(
                                TILE_INNER_RADIUS,
                                TILE_OUTER_RADIUS,
                                TILE_OUTER_RADIUS,
                                TILE_INNER_RADIUS,
                            ))
                            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                    this.start_recording_preview(id, cx);
                                    cx.stop_propagation();
                                }
                            }))
                            .reach().on_click(cx.listener(move |this, _, window, cx| {
                                this.focus.focus(window);
                                this.start_recording_preview(id, cx);
                            }))
                    })
                    .child(label),
            );
            }
        }
        // The warning badge blobs in and, once transcription recovers, shrinks
        // back into the capsule's corner rather than vanishing.
        let badge = self
            .stage
            .presence(("footer-attention", id), attention, GOOEY);
        if badge.visible {
            capsule = capsule.child(crate::motion::pop(
                div()
                    .id("footer-transcription-attention")
                    .absolute()
                    .top(px(0.))
                    .right(px(0.))
                    .size(px(20.))
                    .rounded_full()
                    .border(px(0.5))
                    .border_color(t.border)
                    .bg(rgb(0xed9212))
                    .flex()
                    .items_center()
                    .justify_center()
                    .font_weight(FontWeight::BOLD)
                    .text_size(px(13.))
                    .line_height(px(16.))
                    .text_color(rgb(0xffffff))
                    .child(crate::motion::tactile(rgb(0xf6a228).into()).round())
                    .child("!")
                    .cursor(CursorStyle::Arrow)
                    .when(attention, |badge| {
                        badge.reach().on_click(cx.listener(|this, _, window, cx| {
                            this.focus.focus(window);
                            this.transcript_open = true;
                            this.transcript_epoch = this.transcript_epoch.wrapping_add(1);
                            cx.notify();
                        }))
                    }),
                crate::motion::Pop::BLOB,
                badge,
            ));
        }
        capsule.into_any_element()
    }

    /// The status chip shown while notes are generated. `pose` is its
    /// presence: it grows out of its leading edge and collapses back into it.
    pub fn render_generation_status(
        &self,
        width: Pixels,
        pose: crate::motion::Pose,
        finishing: bool,
    ) -> AnyElement {
        let t = self.theme();
        let label = match self.generation.phase {
            // The stream is still delivering the end of the recording.
            _ if finishing => "Finishing the transcript…",
            // The note is being saved and its recording stopped.
            GenerationPhase::AnalyzingTranscript => "Reading the transcript",
            // The model has been asked: its first words replace this chip.
            // Also while the finished chip is leaving: its last words stay.
            _ => "Writing notes",
        };
        crate::motion::pop(
            div()
                .w(width.min(px(664.)))
                .ml(px(-24.))
                .flex()
                .items_center()
                .gap(px(10.))
                .h(px(40.))
                .px(px(12.))
                .rounded_full()
                .border_1()
                .border_color(t.border)
                .bg(t.card)
                .text_size(px(13.))
                .child(crate::motion::spinner(t.muted))
                .child(label),
            crate::motion::Pop::chip((0., 0.5)),
            pose,
        )
    }

    pub fn render_generate_notes_cta(
        &self,
        id: u64,
        bottom: Pixels,
        pose: crate::motion::Pose,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = self.theme();
        let server = crate::server::enabled();
        // The button floats above whichever panel is open and rides up with it.
        let bottom = px(self
            .stage
            .aim(("generate-notes-bottom", 0), f32::from(bottom), GOOEY));
        div()
            .absolute()
            .bottom(bottom)
            .left_0()
            .right_0()
            .flex()
            .justify_center()
            .child(crate::motion::pop(
                div()
                    .id("generate-notes")
                    .h(px(36.))
                    .px(px(16.))
                    .rounded_full()
                    .text_color(rgb(0xfcfcf8))
                    .text_size(px(14.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .cursor(CursorStyle::Arrow)
                    // The fill belongs to the feedback layer so the whole
                    // button squashes when pressed.
                    .child(
                        crate::motion::tactile(rgb(0x966d00).into())
                            .base(rgb(0x835e00).into())
                            .press(rgba(0x00000026).into())
                            .round(),
                    )
                    .child(icon("sparkles", 16., rgb(0xfcfcf8).into()))
                    .child("Generate notes")
                    .tooltip(move |_, cx| {
                        cx.new(|_| {
                            ToolTip::new(
                                if server {
                                    "Generate notes from the saved server transcript"
                                } else {
                                    "Create a local outline from the saved notes and transcript"
                                },
                                t,
                            )
                        })
                        .into()
                    })
                    .reach().on_click(cx.listener(move |this, _, window, cx| {
                        this.focus.focus(window);
                        this.enhance_note_preview(id, cx);
                    })),
                crate::motion::Pop::chip((0.5, 1.)),
                pose,
            ))
            .into_any_element()
    }

    pub fn render_regeneration_notice(
        &self,
        id: u64,
        revision: u64,
        bottom: Pixels,
        pose: crate::motion::Pose,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = self.theme();
        // In the place of the call to generate notes (the two are never up
        // together): centred above the footer bar, and above whichever panel
        // is open. In the window's corner it lay across the end of the ask
        // bar for as long as it was up, and under the chat panel's edge.
        let bottom = px(self
            .stage
            .aim(("generate-notes-bottom", 0), f32::from(bottom), GOOEY));
        div()
            .absolute()
            .bottom(bottom)
            .left_0()
            .right_0()
            .flex()
            .justify_center()
            .child(crate::motion::pop(
            div()
                .id("regeneration-notice")
                // Nothing behind the notice is under the pointer.
                .occlude()
                .w(px(392.))
                .max_w(window.viewport_size().width - px(32.))
                .h(px(54.))
                .px(px(12.))
                .rounded(px(12.))
                .border_1()
                .border_color(t.border)
                .bg(t.card)
                .shadow_lg()
                .flex()
                .items_center()
                .gap(px(10.))
                .text_size(px(13.))
                .child(div().flex_1().child("Transcript updated"))
                .child(
                    div()
                        .id("regenerate-notes")
                        .px(px(10.))
                        .h(px(30.))
                        .rounded(px(8.))
                        .bg(t.fill)
                        .flex()
                        .items_center()
                        .cursor(CursorStyle::Arrow)
                        .child(crate::motion::tactile(neutral_wash(0.14)).radius(8.))
                        .child("Regenerate notes")
                        .reach().on_click(cx.listener(move |this, _, window, cx| {
                            this.focus.focus(window);
                            this.enhance_note_preview(id, cx);
                        })),
                )
                .child(
                    tool_tip("dismiss-transcript-update", "close", "Dismiss", t)
                        .size(px(20.))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.regeneration_notice_dismissed = Some((id, revision));
                            cx.notify();
                        })),
                ),
            crate::motion::Pop::chip((0.5, 1.)),
            pose,
            ))
            .into_any_element()
    }
}

/// Space between a scheduled meeting's label and what follows it.
const SCHEDULED_LABEL_GAP: f32 = 10.;

// The recording capsule, as the reference lays it out: a 56 pt capsule with
// 4 pt of padding round 48 pt controls. A control's hover shape is concentric
// with the capsule on its outer side (28 − 4 = 24 pt) and 8 pt where it meets
// its neighbour.
const CAPSULE_HEIGHT: f32 = 56.;
const CAPSULE_INSET: f32 = 4.;
const TILE: f32 = CAPSULE_HEIGHT - 2. * CAPSULE_INSET;
const TILE_OUTER_RADIUS: f32 = CAPSULE_HEIGHT / 2. - CAPSULE_INSET;
const TILE_INNER_RADIUS: f32 = 8.;
/// The indicator and chevron with a control after them: 4 pt padding each
/// side, at least 48 pt.
const LEADING_JOINED: f32 = 48.;
/// The indicator and chevron alone: 6 pt padding each side, 49 pt.
const LEADING_ALONE: f32 = 49.;
/// Stop, Resume, Start now or Retry: at least 40 pt, 3 pt short of the end.
const TRAILING_MIN: f32 = 40.;
const TRAILING_END: f32 = 3.;
/// Horizontal padding of a trailing label.
const TRAILING_LABEL_PAD: f32 = 8.;
const LIVE_CAPSULE: f32 = 2. * CAPSULE_INSET + LEADING_JOINED + TRAILING_MIN;
const COMPACT_CAPSULE: f32 = 2. * CAPSULE_INSET + LEADING_ALONE;

/// The capsule of a scheduled meeting: the 48 pt indicator inside 4 pt of
/// padding, the label (`text` pt wide), and in the open panel the 80 pt
/// "Start now" beside it.
fn scheduled_capsule_width(text: f32, open: bool) -> f32 {
    (2. * CAPSULE_INSET + LEADING_JOINED + text + SCHEDULED_LABEL_GAP + if open { 80. } else { 0. })
        .ceil()
}

/// The stopped capsule with a trailing label `text` pt wide (Resume, Start
/// now or Retry).
fn labelled_capsule_width(text: f32) -> f32 {
    (2. * CAPSULE_INSET
        + LEADING_JOINED
        + (text + 2. * TRAILING_LABEL_PAD + TRAILING_END).max(TRAILING_MIN))
    .ceil()
}

/// The left edge of the chevron's 16 pt frame in the indicator control. The
/// reference lays the control out as padding, a 4 pt margin, the indicator
/// in 22 pt with its own margins, an 8 pt gap, and the chevron pulled 10 pt
/// back with 3 pt off its end, centred in its minimum width.
fn chevron_left(joined: bool) -> f32 {
    indicator_slot(joined) + 22. + 8. - 10.
}

/// Where the indicator's 22 pt slot (glyph plus its margins) starts.
fn indicator_slot(joined: bool) -> f32 {
    let padding = if joined { 4. } else { 6. };
    let content = 2. * padding + 4. + 22. + 8. - 10. + 16. - 3.;
    let width = if joined {
        LEADING_JOINED
    } else {
        LEADING_ALONE
    };
    (width - content).max(0.) / 2. + padding + 4.
}

/// The edge of a fully rounded bar surface, as the reference draws it: a
/// half-point hairline (a device pixel at least) just outside it, or just
/// inside it in the dark theme, and the bottom bar's soft shadow. The
/// hairline is an outline of its own, so it takes no room from the
/// surface's content in either theme (a border would), and it is crisp on
/// every side (a spread shadow antialiases unevenly at half a point).
pub fn bar_edge<E: Styled + ParentElement>(element: E, t: Theme) -> E {
    let width = crate::ui::hairline(0.5);
    let out = if t.bg.l < 0.5 { 0. } else { width };
    element
        .shadow(vec![BoxShadow {
            color: rgba(0x0000000d).into(),
            offset: point(px(0.), px(4.)),
            blur_radius: px(6.),
            spread_radius: px(-2.),
        }])
        .child(edge(width, out, t.hairline))
}

/// A `width` outline of a fully rounded surface, `out` points outside its
/// edge (0 for just inside). An overlay, so it takes no room from the
/// surface's content.
fn edge(width: f32, out: f32, color: Hsla) -> Div {
    div()
        .absolute()
        .top(px(-out))
        .left(px(-out))
        .right(px(-out))
        .bottom(px(-out))
        .rounded_full()
        .border(px(width))
        .border_color(color)
}

/// The recording bar's tooltips, as the reference places them: centred above
/// the control, 6 pt clear of it, after the pointer has rested on it for
/// 600 ms. GPUI's own tooltips open at the pointer, below the bar, where they
/// cover the control they explain.
const TIP_DELAY: Duration = Duration::from_millis(600);
const TIP_TRANSCRIPT: u64 = 1;
const TIP_STOP: u64 = 2;
/// The reference's 75 ms ease-out from 95%, without overshoot.
const TIP_ENTRANCE: crate::motion::Spring = crate::motion::Spring {
    millis: 75,
    damping: 1.,
};

impl Cereal {
    /// Give the control `element` (a positioned tile) the tooltip `label`,
    /// identified by `key` among the bar's controls.
    fn bar_tip(
        &self,
        element: Stateful<Div>,
        key: u64,
        label: SharedString,
        t: Theme,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let shown = !crate::ui::popover_open()
            && self
                .bar_tip
                .is_some_and(|(tip, since)| tip == key && since.elapsed() >= TIP_DELAY);
        let pose = self.stage.presence(("bar-tip", key), shown, TIP_ENTRANCE);
        element
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if *hovered {
                    this.bar_tip = Some((key, Instant::now()));
                    cx.spawn(async move |this, cx| {
                        cx.background_executor().timer(TIP_DELAY).await;
                        let _ = this.update(cx, |_, cx| cx.notify());
                    })
                    .detach();
                } else if this.bar_tip.is_some_and(|(tip, _)| tip == key) {
                    this.bar_tip = None;
                }
                cx.notify();
            }))
            // A press dismisses it until the pointer comes back.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    if this.bar_tip.is_some_and(|(tip, _)| tip == key) {
                        this.bar_tip = None;
                        cx.notify();
                    }
                }),
            )
            .when(pose.visible, |d| {
                d.child(
                    deferred(
                        div()
                            .absolute()
                            .left_0()
                            .right_0()
                            .bottom(relative(1.))
                            .pb(px(6.))
                            .flex()
                            .justify_center()
                            .child(crate::motion::pop(
                                crate::ui::tip_box(label, t).whitespace_nowrap(),
                                crate::motion::Pop {
                                    anchor: (0.5, 1.),
                                    scale: (0.95, 0.95),
                                    travel: 0.,
                                    rise: 0.,
                                },
                                pose,
                            )),
                    )
                    .with_priority(20),
                )
            })
    }
}

/// Legacy footer visualizer, driven by the recording owner's audio level.
/// Each glyph sits in the indicator control's 22 pt slot with the
/// reference's margins for it, centred vertically in the 48 pt control.
pub fn footer_indicator(phase: Phase, theme: Theme, level: f32, joined: bool) -> AnyElement {
    let slot = indicator_slot(joined);
    let place = |element: Div, margin: f32, height: f32| {
        element
            .absolute()
            .left(px(slot + margin))
            .top(px((TILE - height) / 2.))
            .into_any_element()
    };
    match phase {
        // A 16 pt spinner, 2.5 pt in.
        Phase::Starting => place(div().child(crate::motion::spinner(theme.muted)), 2.5, 16.),
        // Nothing is in progress after a failure: a still glyph, not a spinner
        // that would also keep the window redrawing until the user acts.
        Phase::Failed => place(
            div().child(icon("warning", 18., rgb(0xc08038).into())),
            2.,
            18.,
        ),
        // The 13 pt bars, 4 pt in.
        Phase::Live => place(
            div().child(recording_waveform(level, WAVEFORM.into())),
            4.,
            20.,
        ),
        // The reference's 20 pt policy lock, 2 pt in.
        Phase::Held => place(div().child(icon("lock", 20., theme.muted)), 2., 20.),
        // The 32 pt transcript glyph, pulled 6 pt out of its slot.
        _ => place(
            div().child(icon("footer-transcript", 32., theme.muted)),
            -6.,
            32.,
        ),
    }
}

/// Target heights of the three footer bars (the reference's visualizer with
/// ten-point halves): quiet input settles to 3 pt dots, speech grows them up
/// to 20 pt, the centre bar twice as readily as the outer two.
pub fn waveform_heights(level: f32) -> [f32; 3] {
    level_bars(level, 10.)
}

/// The reference app's bar mapping for three mirrored bars whose halves are
/// `half` points tall: each bar shows `2 × min(⌊half · level / s⌋ + 1.5,
/// half)` points, where `s` is its sensitivity (2 for the outer bars, 1 for
/// the centre) scaled by a factor between 0.6 and 1. The reference draws that
/// factor at random for every audio sample, which keeps speech lively. Here
/// it is a hash of the level's bits, so the same level always gives the same
/// bars; a steady sound stays still only because the recording model holds
/// its shown level until the input moves by `recording::LEVEL_STEP`.
/// Silence (and anything under about 6% for the centre bar) is three dots.
pub fn level_bars(level: f32, half: f32) -> [f32; 3] {
    let level = if level.is_finite() {
        level.clamp(0., 1.)
    } else {
        0.
    };
    let sample = level.to_bits();
    [2_f32, 1., 2.]
        .into_iter()
        .enumerate()
        .map(|(bar, sensitivity)| {
            // SplitMix32 of the sample and the bar: an even spread in 0..1.
            let mut mix = sample ^ (bar as u32).wrapping_mul(0x9e37_79b9);
            mix = (mix ^ (mix >> 16)).wrapping_mul(0x21f0_aaad);
            mix = (mix ^ (mix >> 15)).wrapping_mul(0x735a_2d97);
            mix ^= mix >> 15;
            let factor = 0.6 + 0.4 * (mix as f32 / u32::MAX as f32);
            2. * ((half * level / (factor * sensitivity)).floor() + 1.5).min(half)
        })
        .collect::<Vec<_>>()
        .try_into()
        .unwrap_or([3.; 3])
}

/// The level bars' green (#74bd00), as the reference draws them in the
/// note's bar and on the floating capsule alike.
pub const WAVEFORM: Rgba = Rgba { r: 116. / 255., g: 189. / 255., b: 0., a: 1. };

/// The live waveform. Each new level eases the bars to their new heights
/// over 300 ms, and they stop redrawing once the input is steady.
pub fn recording_waveform(level: f32, color: Hsla) -> AnyElement {
    crate::motion::bars(
        "recording-waveform",
        waveform_heights(level),
        3.,
        2.,
        20.,
        color,
    )
    .into_any_element()
}

/// The fewest spoken words notes are offered for.
const WORDS_WORTH_NOTES: usize = 12;

/// Whether `transcript` (the note's transcript as it is kept here: blocks
/// of a speaker-and-time line and what was said) holds enough speech to
/// write notes from. A recording of a cough and a "hello" offers nothing to
/// generate, rather than an empty note.
pub fn enough_speech(transcript: &str) -> bool {
    let mut words = 0;
    for block in transcript.split("\n\n") {
        let mut lines = block.lines().peekable();
        if lines.peek().is_some_and(|first| {
            first.split_whitespace().last().is_some_and(|word| {
                word.contains(':') && word.chars().all(|c| c.is_ascii_digit() || c == ':')
            })
        }) {
            lines.next();
        }
        for line in lines {
            words += line
                .split_whitespace()
                .filter(|word| word.chars().any(char::is_alphanumeric))
                .count();
            if words >= WORDS_WORTH_NOTES {
                return true;
            }
        }
    }
    false
}

/// Flatten saved transcript blocks into a plain local-outline input. Speaker/time
/// headers stay in the transcript rather than becoming generated note bullets.
fn outline_source(body: &str, transcript: &str) -> String {
    let spoken = transcript
        .split("\n\n")
        .filter_map(|block| {
            let mut lines = block.lines();
            let first = lines.next()?;
            let looks_like_header = first.split_whitespace().last().is_some_and(|word| {
                word.contains(':') && word.chars().all(|ch| ch.is_ascii_digit() || ch == ':')
            });
            if looks_like_header {
                Some(lines.collect::<Vec<_>>().join("\n"))
            } else {
                Some(block.to_owned())
            }
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    [body.trim(), spoken.trim()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    // `gpui::*` re-exports its own `test` attribute; these are plain unit tests.
    use std::prelude::v1::test;

    #[test]
    fn capsule_controls_follow_the_reference_layout() {
        // 56 pt capsule, 4 pt padding, 48 pt controls, concentric outer corners.
        assert_eq!(TILE, 48.);
        assert_eq!(TILE_OUTER_RADIUS, 24.);
        assert_eq!(LIVE_CAPSULE, 96.);
        assert_eq!(COMPACT_CAPSULE, 57.);
        // Bars 13.5 pt and the chevron's frame 29.5 pt into a joined control;
        // the transcript glyph 3.5 pt in. Alone, 4 pt and 30 pt.
        assert_eq!(indicator_slot(true) + 4., 13.5);
        assert_eq!(chevron_left(true), 29.5);
        assert_eq!(indicator_slot(true) - 6., 3.5);
        assert_eq!(indicator_slot(false) - 6., 4.);
        assert_eq!(chevron_left(false), 30.);
        // Resume: 8 pt either side of the label and 3 pt short of the end.
        assert_eq!(labelled_capsule_width(48.), 4. + 48. + 67. + 4.);
        assert_eq!(labelled_capsule_width(10.), 96.);
    }

    #[test]
    fn notes_are_offered_only_for_a_recording_with_something_said() {
        assert!(!enough_speech(""));
        assert!(!enough_speech("Me 0:01\nHello? Is this on?"));
        // Speaker-and-time lines are not speech.
        assert!(!enough_speech("Me 0:01\nHello.\n\nSpeaker 1 0:04\nHi.\n\nMe 0:09\nOkay, bye."));
        assert!(enough_speech(
            "Me 0:01\nSo the launch moves to Monday.\n\nSpeaker 1 0:04\nRight, after the accessibility review is done."
        ));
        // A transcript without headers counts too.
        assert!(enough_speech("one two three four five six seven eight nine ten eleven twelve"));
    }

    #[test]
    fn footer_bars_are_dots_when_quiet_and_grow_with_the_voice() {
        assert_eq!(waveform_heights(0.), [3.; 3]);
        assert_eq!(waveform_heights(f32::NAN), [3.; 3]);
        // The same level always draws the same bars.
        assert_eq!(waveform_heights(0.4), waveform_heights(0.4));
        let loud = waveform_heights(1.);
        assert_eq!(loud[1], 20.);
        assert!(loud.iter().all(|height| (3. ..=20.).contains(height)));
        let speech = waveform_heights(0.3);
        assert!(speech[1] > speech[0] && speech[1] > speech[2]);
    }
}
