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
        if crate::server::enabled()
            && self.recording.is_active()
            && let Some(id) = self.recording.note_id
        {
            self.server_action(id, crate::server::Action::Recording("stop"), cx);
        }
        if let Some(capture) = self.audio_capture.take() {
            capture.request_stop();
        }
        // The live transcript stream sends what is left and marks the end.
        self.live_end();
        if self.recording.note_id.is_some() {
            self.recording.stop();
            self.save_recording_preview(cx);
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
            note.normalize_generation_revision();
            self.recording_transcript_start =
                Some((note.transcript_revision, note.transcript.clone()));
        }
        self.recording_changed_at = Some(Instant::now());
        let epoch = self.recording.epoch;
        cx.notify();
        if !crate::audio::preview_mode() {
            if crate::server::enabled() {
                self.server_action(
                    id,
                    crate::server::Action::Recording(if resumed { "resume" } else { "start" }),
                    cx,
                );
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
        if let Some(capture) = self.audio_capture.take() {
            capture.request_stop();
        }
        // The live transcript stream sends what is left and marks the end;
        // the durable upload below is unchanged.
        self.live_end();
        if let Some(id) = self.recording.note_id {
            self.server_action(id, crate::server::Action::Recording("stop"), cx);
        }
        self.recording_changed_at = Some(Instant::now());
        self.menu = None;
        self.transcript_open = false;
        self.transcript_find_open = false;
        self.save_recording_preview(cx);
        cx.notify();
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
        let result = crate::model::audio_directory()
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
        let monitor = capture.monitor();
        // Start and Resume both begin a new capture; the live transcript
        // stream follows it once the server has the note recording.
        self.live_prepare(id, monitor.clone());
        self.audio_pending.push(monitor.clone());
        let initial = monitor.snapshot();
        if let Some(note) = self.workspace.note_mut(id) {
            note.audio_captures.push(initial.clone());
        }
        self.audio_capture = Some(capture);
        // Persist the immutable session directory association before any speech
        // arrives, so shutdown cannot orphan audio from its owner note.
        self.persist(cx);
        let account_epoch = crate::account::epoch();
        cx.spawn(async move |this, cx| {
            let mut held = false;
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(80))
                    .await;
                let snapshot = monitor.snapshot();
                let completed = snapshot.completed();
                let Ok(keep) = this.update(cx, |this, cx| {
                    if crate::account::epoch() != account_epoch {
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
                    if current && owner && this.recording.is_active() {
                        if snapshot.stats.has_audio() && this.recording.phase == Phase::Starting {
                            this.recording.activate(epoch);
                        }
                        this.recording.set_audio_levels(
                            snapshot.stats.mic.level,
                            snapshot.stats.system.level,
                        );
                        if let Some(error) = snapshot.error_message()
                            && this.audio_error.as_ref() != Some(&error)
                        {
                            this.audio_error = Some(error);
                        }
                    }
                    if completed {
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
                            if owner && this.recording.is_active() {
                                this.recording.fail(epoch);
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
            if self.server_pending > 0 {
                self.show_toast(
                    "Wait for audio transcription to finish before generating notes",
                    cx,
                );
                return;
            }
            self.active_tab = NoteTab::Enhanced;
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

    /// The live account uses the legacy floating `m_n` controls, not the
    /// feature-flagged 32 px `yHr` header capsule. Its layout changes naturally.
    pub fn render_recording_control(&self, id: u64, cx: &mut Context<Self>) -> AnyElement {
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
        let hover: Hsla = if t.bg.l < 0.5 {
            rgba(0xffffff0f).into()
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
        let leading_label: SharedString = if open {
            "Hide transcript"
        } else {
            "Show transcript"
        }
        .into();
        let leading = div()
            .id("show-transcript-button")
            .relative()
            .w(px(48.))
            .h(px(48.))
            .flex_shrink_0()
            .rounded(px(10.))
            .reach()
            .cursor(CursorStyle::Arrow)
            .child(crate::motion::tactile(hover).radius(10.))
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
                            )),
                            self.stage.swap(("footer-indicator", id), shown as u64),
                        )
                    }),
            )
            .child(
                icon("chevron-down", 16., t.muted)
                    .absolute()
                    .left(px(28.))
                    .top(px(16.))
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
            .tooltip(move |_, cx| cx.new(|_| ToolTip::new(leading_label.clone(), t)).into())
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
        let capsule_target = if processing || compact_complete {
            58.
        } else if let Some(label) = &scheduled {
            // The label as it is drawn: 13 px in the body face.
            let face = cx.text_system().resolve_font(&font(FONT));
            let text: f32 = label
                .chars()
                .map(|character| {
                    cx.text_system()
                        .advance(face, px(13.), character)
                        .map_or(7., |size| f32::from(size.width))
                })
                .sum();
            scheduled_capsule_width(text, open)
        } else {
            capsule_width(phase)
        };
        let capsule_width = self
            .stage
            .aim(("recording-capsule", id), capsule_target, GOOEY)
            .max(58.);
        // The label only has room once the capsule is most of the way open, so
        // it fades in over the second half of the morph instead of being cut.
        let label_fade = if capsule_target > 58. {
            crate::motion::fade_curve(((capsule_width - 58.) / (capsule_target - 58.) - 0.5) / 0.5)
        } else {
            1.
        };
        let mut capsule = div()
            .id("recording-capsule")
            .relative()
            .h(px(58.))
            // Compact, expanded and processing are one capsule changing shape:
            // its width rides a spring between states (per note, so opening a
            // different note never animates).
            .w(px(capsule_width))
            .flex_shrink_0()
            .p(px(4.))
            .flex()
            .items_center()
            .rounded(px(29.))
            .border_1()
            .border_color(t.border)
            .bg(t.card)
            .shadow(vec![BoxShadow {
                color: rgba(0x0000000d).into(),
                offset: point(px(0.), px(4.)),
                blur_radius: px(6.),
                spread_radius: px(-2.),
            }])
            .when(attention && !open, |s| {
                s.bg(rgba(0xed921233))
                    .border(px(0.5))
                    .border_color(t.warning_edge)
                    .text_color(t.warning_ink)
            })
            .when(open, |s| {
                s.bg(rgba(0x00000000))
                    .border_color(rgba(0x00000000))
                    .shadow(Vec::<BoxShadow>::new())
            })
            .child(leading);
        if live {
            capsule = capsule.child(
                tool_tip("stop-transcript-button", "footer-stop", "Stop recording", t)
                    .w(px(40.))
                    .h(px(48.))
                    .flex_shrink_0()
                    .rounded(px(10.))
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
            let label: SharedString = match phase {
                Phase::Idle => "Start now".into(),
                Phase::Held | Phase::Failed => "Retry".into(),
                _ => "Resume".into(),
            };
            // A scheduled meeting offers "Start now" only in the open panel.
            if scheduled.is_none() || open {
            capsule = capsule.child(
                div()
                    .id("start-transcript-button")
                    .h(px(48.))
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .pr(px(3.))
                    .rounded(px(10.))
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
                            .child(crate::motion::tactile(hover).radius(10.))
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
    pub fn render_generation_status(&self, width: Pixels, pose: crate::motion::Pose) -> AnyElement {
        let t = self.theme();
        let label = match self.generation.phase {
            GenerationPhase::AnalyzingTranscript => "Analyzing transcript",
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

    pub fn transcription_language_settings(&self, cx: &mut Context<Self>) -> Div {
        let t = self.theme();
        let connected = crate::server::enabled();
        div()
            .w(px(220.))
            .p(px(6.))
            .rounded(px(POPOVER_RADIUS))
            .border_1()
            .border_color(t.border)
            .bg(t.card)
            .shadow_lg()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(
                div()
                    .id("english-transcription-language")
                    .h(px(32.))
                    .px(px(8.))
                    .rounded(px(6.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_size(px(13.))
                    .child(if connected {
                        crate::desktop_preferences::PROCESSING_LANGUAGE
                    } else {
                        "Transcription unavailable"
                    })
                    .when(connected, |d| {
                        d.bg(t.hover)
                            .child(icon("check", 14., t.accent))
                            .cursor_pointer()
                            .reach().on_click(cx.listener(|this, _, _, cx| {
                                this.menu = None;
                                cx.notify();
                            }))
                    }),
            )
            .child(
                div()
                    .px(px(8.))
                    .pb(px(6.))
                    .text_size(px(12.))
                    .line_height(px(17.))
                    .text_color(t.muted)
                    .child(if connected {
                        "Your server transcribes and writes notes in English by default."
                    } else {
                        "Connect to your server to transcribe recordings."
                    }),
            )
    }

    pub fn recording_settings(&self, cx: &mut Context<Self>) -> Div {
        let t = self.theme();
        div()
            .w(px(272.))
            .p(px(8.))
            .flex()
            .flex_col()
            .gap(px(4.))
            .rounded(px(12.))
            .bg(t.card)
            .border_1()
            .border_color(t.border)
            .shadow_lg()
            .child(
                div()
                    .px(px(8.))
                    .py(px(7.))
                    .text_size(px(12.))
                    .text_color(t.muted)
                    .child("Audio input"),
            )
            .child(
                div()
                    .id("microphone-preview-toggle")
                    .px(px(8.))
                    .py(px(8.))
                    .rounded(px(6.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .cursor_pointer()
                    .child(row_feedback(t, 6.))
                    .child(icon("mic", 16., t.muted))
                    .child(if self.microphone_muted {
                        "Microphone off for next segment"
                    } else {
                        "Microphone on for next segment"
                    })
                    .reach().on_click(cx.listener(|this, _, _, cx| {
                        if this.recording.is_active() && !crate::audio::preview_mode() {
                            this.show_toast("Stop recording before changing audio inputs", cx);
                            return;
                        }
                        this.microphone_muted = !this.microphone_muted;
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .px(px(8.))
                    .py(px(6.))
                    .text_size(px(11.))
                    .line_height(px(16.))
                    .text_color(t.muted)
                    .child(if crate::audio::preview_mode() {
                        "Preview · audio capture is off"
                    } else {
                        "Changes apply when you start or resume recording."
                    }),
            )
            .child(separator(t))
            .child(
                div()
                    .id("sound-settings")
                    .px(px(8.))
                    .py(px(9.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .child(row_feedback(t, 6.))
                    .child("Sound settings")
                    .reach().on_click(cx.listener(|this, _, _, cx| {
                        this.menu = None;
                        cx.open_url(crate::desktop_preferences::SOUND_SETTINGS_URL);
                        cx.notify();
                    })),
            )
    }
}

/// Space between a scheduled meeting's label and what follows it.
const SCHEDULED_LABEL_GAP: f32 = 10.;

/// The capsule of a scheduled meeting: the 48 px indicator inside 4 px of
/// padding, the label (`text` px wide), and in the open panel the 80 px
/// "Start now" beside it.
fn scheduled_capsule_width(text: f32, open: bool) -> f32 {
    (56. + text + SCHEDULED_LABEL_GAP + if open { 80. } else { 0. }).ceil()
}

fn capsule_width(phase: Phase) -> f32 {
    match phase {
        Phase::Starting | Phase::Live => 98.,
        Phase::Idle => 137.,
        Phase::Held | Phase::Failed => 112.,
        _ => 124.,
    }
}

/// Legacy footer visualizer, driven by the recording owner's audio level.
pub fn footer_indicator(phase: Phase, theme: Theme, level: f32) -> AnyElement {
    match phase {
        Phase::Starting => div()
            .absolute()
            .left(px(12.))
            .top(px(16.))
            .child(crate::motion::spinner(theme.muted))
            .into_any_element(),
        // Nothing is in progress after a failure: a still glyph, not a spinner
        // that would also keep the window redrawing until the user acts.
        Phase::Failed => icon("warning", 18., rgb(0xc08038).into())
            .absolute()
            .left(px(11.))
            .top(px(15.))
            .into_any_element(),
        Phase::Live => div()
            .absolute()
            .left(px(14.))
            .top(px(14.))
            .child(recording_waveform(level, rgb(0xce9d00).into()))
            .into_any_element(),
        Phase::Held => div()
            .absolute()
            .left(px(12.))
            .top(px(14.))
            .child(icon("lock", 20., theme.muted))
            .into_any_element(),
        _ => icon("footer-transcript", 32., theme.muted)
            .absolute()
            .left(px(3.))
            .top(px(8.))
            .into_any_element(),
    }
}

/// Target heights of the three source-width bars: quiet input settles to
/// dots, speech grows vertically.
pub fn waveform_heights(level: f32) -> [f32; 3] {
    let level = if level.is_finite() {
        level.clamp(0., 1.)
    } else {
        0.
    };
    // Whole pixels: a level that moves a bar by less than a pixel is not a
    // change anyone can see, and must not wake the animation.
    [0.72_f32, 1., 0.72].map(|gain| {
        if level < 0.015 {
            2.
        } else {
            (level.sqrt() * 20. * gain).clamp(3., 20.).round()
        }
    })
}

/// The live waveform. Bars follow level changes on springs rather than
/// stepping between samples, and stop redrawing once the input is steady.
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
