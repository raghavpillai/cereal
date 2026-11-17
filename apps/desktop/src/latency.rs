//! How long each step of starting and stopping a recording takes, for
//! measuring (`CEREAL_LATENCY_TRACE=<file>`, debug builds only; a release
//! build compiles every call here to nothing).
//!
//! A run is one press: [`begin`] starts it (the press of Record or Stop),
//! and each step that follows calls [`mark`] when it happens, from any
//! thread. Only the first mark of a stage in a run is kept, except for the
//! stages given to [`mark_each`]. [`on_paint`] marks a stage at the window's
//! next frame. Each mark is one JSON line in the file:
//! `{"run":3,"kind":"start","stage":"ws_open","ms":41.2,"wall":1763291000123.4}`
//! where `ms` is since the press and `wall` is Unix time in milliseconds (to
//! line the server's own trace up with it).

#[cfg(debug_assertions)]
mod imp {
    use std::{
        collections::HashSet,
        fs::{File, OpenOptions},
        io::Write,
        sync::{Mutex, OnceLock},
        time::{Instant, SystemTime, UNIX_EPOCH},
    };

    pub(super) struct Trace {
        file: Option<File>,
        run: u64,
        kind: String,
        began: Instant,
        seen: HashSet<String>,
        /// Stages to mark at the next frame.
        paint: Vec<String>,
    }

    fn trace() -> Option<&'static Mutex<Trace>> {
        static TRACE: OnceLock<Option<Mutex<Trace>>> = OnceLock::new();
        TRACE
            .get_or_init(|| {
                let path = crate::environment::var_os("CEREAL_LATENCY_TRACE")?;
                crate::environment::var_os("CEREAL_DATA_DIR")?;
                let file = OpenOptions::new().create(true).append(true).open(path).ok();
                Some(Mutex::new(Trace {
                    file,
                    run: 0,
                    kind: String::new(),
                    began: Instant::now(),
                    seen: HashSet::new(),
                    paint: Vec::new(),
                }))
            })
            .as_ref()
    }

    fn lock() -> Option<std::sync::MutexGuard<'static, Trace>> {
        trace().map(|trace| trace.lock().unwrap_or_else(|poison| poison.into_inner()))
    }

    pub fn enabled() -> bool {
        trace().is_some()
    }

    fn write(trace: &mut Trace, stage: &str) {
        let ms = trace.began.elapsed().as_secs_f64() * 1000.;
        let wall = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0., |since| since.as_secs_f64() * 1000.);
        let line = serde_json::json!({
            "run": trace.run, "kind": trace.kind, "stage": stage, "ms": (ms * 10.).round() / 10., "wall": wall,
        });
        if let Some(file) = trace.file.as_mut() {
            let _ = writeln!(file, "{line}");
        }
    }

    pub fn begin(kind: &str) {
        if let Some(mut trace) = lock() {
            trace.run += 1;
            trace.kind = kind.to_owned();
            trace.began = Instant::now();
            trace.seen.clear();
            trace.paint.clear();
            write(&mut trace, "press");
        }
    }

    pub fn mark(stage: &str) {
        if let Some(mut trace) = lock()
            && trace.seen.insert(stage.to_owned())
        {
            write(&mut trace, stage);
        }
    }

    pub fn mark_each(stage: &str) {
        if let Some(mut trace) = lock() {
            trace.seen.insert(stage.to_owned());
            write(&mut trace, stage);
        }
    }

    pub fn on_paint(stage: &str) {
        if let Some(mut trace) = lock()
            && !trace.seen.contains(stage)
            && !trace.paint.iter().any(|known| known == stage)
        {
            trace.paint.push(stage.to_owned());
        }
    }

    pub fn painted() {
        if let Some(mut trace) = lock()
            && !trace.paint.is_empty()
        {
            for stage in std::mem::take(&mut trace.paint) {
                if trace.seen.insert(stage.clone()) {
                    write(&mut trace, &stage);
                }
            }
        }
    }

    /// Whether `stage` has been marked in the current run.
    pub fn seen(stage: &str) -> bool {
        lock().is_some_and(|trace| trace.seen.contains(stage))
    }

    pub(super) fn imp_lock() -> Option<std::sync::MutexGuard<'static, Trace>> {
        lock()
    }

    pub(super) fn trace_line(trace: &mut Trace, line: &serde_json::Value) {
        if let Some(file) = trace.file.as_mut() {
            let _ = writeln!(file, "{line}");
        }
    }
}
#[cfg(debug_assertions)]
use imp::{imp_lock, trace_line};

#[cfg(debug_assertions)]
#[allow(unused_imports)]
pub use imp::{begin, enabled, mark, mark_each, on_paint, painted, seen};

#[cfg(not(debug_assertions))]
mod off {
    #[inline(always)]
    pub fn enabled() -> bool {
        false
    }
    #[inline(always)]
    #[allow(dead_code)]
    pub fn begin(_: &str) {}
    #[inline(always)]
    pub fn mark(_: &str) {}
    #[inline(always)]
    pub fn mark_each(_: &str) {}
    #[inline(always)]
    pub fn on_paint(_: &str) {}
    #[inline(always)]
    pub fn painted() {}
}
#[cfg(not(debug_assertions))]
#[allow(unused_imports)]
pub use off::*;

/// `CEREAL_LATENCY_BENCH=<rounds>` (debug builds, with an isolated
/// `CEREAL_DATA_DIR`, a server configured and `CEREAL_CAPTURE_FIXTURE`):
/// presses Record and Stop through the app's own path, round after round,
/// while [`mark`] writes where the time goes. One round:
///
/// 1. `start`: a new note is recorded (the first round after launch is the
///    cold one); it records `CEREAL_LATENCY_BENCH_SPEAK` seconds (4).
/// 2. `stop`, then waits until "Generate notes" is offered.
/// 3. `resume`: the same note again, as soon as that is offered; 3 s.
/// 4. `stop_resumed`, and at once
/// 5. `start_after_stop`: another new note, without waiting for the first
///    to finish; 3 s.
/// 6. `stop_after_start`, then waits until everything has settled.
///
/// After each round a `check` line says where each session's stored text
/// begins and ends against what the capture held. The app quits at the end.
#[cfg(debug_assertions)]
pub fn bench(handle: gpui::WindowHandle<crate::app::Cereal>, rounds: usize, cx: &mut gpui::App) {
    use std::time::{Duration, Instant};
    let speak = crate::environment::var("CEREAL_LATENCY_BENCH_SPEAK")
        .ok()
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(4.);
    cx.spawn(async move |cx| {
        let tick = Duration::from_millis(5);
        let quit = |cx: &mut gpui::AsyncApp, reason: &str| {
            eprintln!("latency bench: {reason}");
            let _ = cx.update(|cx| cx.quit());
        };
        if !crate::server::enabled() {
            return quit(cx, "refused: no server is configured");
        }
        for _ in 0..400 {
            if crate::accounts::any_shown() {
                break;
            }
            cx.background_executor().timer(Duration::from_millis(25)).await;
        }
        if !crate::accounts::any_shown() {
            return quit(cx, "refused: the server did not accept the configured token");
        }
        // Wait for `done` (checked every 5 ms), at most `limit`.
        macro_rules! wait_for {
            ($limit:expr, $done:expr) => {{
                let began = Instant::now();
                loop {
                    let done = handle.update(cx, |view, _, _| $done(view)).unwrap_or(true);
                    if done || began.elapsed() >= $limit {
                        break done;
                    }
                    cx.background_executor().timer(tick).await;
                }
            }};
        }
        let new_note = |view: &mut crate::app::Cereal, cx: &mut gpui::Context<crate::app::Cereal>| {
            let id = view.create_note_without_event()?;
            view.open_note(id, cx);
            view.transcript_open = true;
            view.transcript_epoch = view.transcript_epoch.wrapping_add(1);
            view.persist(cx);
            Some(id)
        };
        let press = |view: &mut crate::app::Cereal, kind: &str, id: Option<u64>, cx: &mut gpui::Context<crate::app::Cereal>| {
            // The double-click guard is the person's, not the system's.
            view.recording_last_click = None;
            view.recording_changed_at = None;
            begin(kind);
            match id {
                Some(id) => view.start_recording_preview(id, cx),
                None => view.stop_recording_preview(cx),
            }
        };
        let interim = |_: &mut crate::app::Cereal| seen("interim_applied");
        let settled = |id: u64| {
            move |view: &mut crate::app::Cereal| {
                // What the note page offers "Generate notes" on (note.rs),
                // for when the window is off screen and draws nothing.
                // (Less the "enough words" rule, which is about what was
                // said, not about time: the fake recogniser says few words.)
                let offered = !view.recording.is_active()
                    && view.server_pending == 0
                    && view.workspace.note(id).is_some_and(|note| {
                        !note.transcript.trim().is_empty() && note.enhanced.trim().is_empty()
                    });
                if offered {
                    mark("generate_available");
                }
                seen("generate_available") && !view.live.streams(id) && view.server_pending == 0
            }
        };
        let recorded = |view: &mut crate::app::Cereal| {
            view.audio_capture
                .as_ref()
                .map_or(0., |capture| capture.monitor().snapshot().audio_seconds())
        };
        let check = |handle: gpui::WindowHandle<crate::app::Cereal>, cx: &mut gpui::AsyncApp, round: usize, id: u64, label: &str| {
            let _ = handle.update(cx, |view, _, _| {
                let Some(note) = view.workspace.note(id) else { return };
                let captures: Vec<f64> = note.audio_captures.iter().map(|c| c.audio_seconds()).collect();
                let segments: Vec<_> = note
                    .server_transcript_segments
                    .iter()
                    .map(|s| serde_json::json!({"start": s.start_ms, "end": s.end_ms, "text": s.text}))
                    .collect();
                let line = serde_json::json!({"check": label, "round": round, "captures": captures, "segments": segments, "gaps": note.transcript_gaps.len()});
                if let Some(mut trace) = imp_lock() {
                    trace_line(&mut trace, &line);
                }
            });
        };
        for round in 0..rounds {
            // 1. Record a new note.
            let Ok(Some(a)) = handle.update(cx, |view, _, cx| {
                let id = new_note(view, cx)?;
                press(view, "start", Some(id), cx);
                Some(id)
            }) else {
                return quit(cx, "could not start a note");
            };
            let began = Instant::now();
            wait_for!(Duration::from_secs(15), interim);
            wait_for!(Duration::from_secs_f64(speak + 5.), |view: &mut crate::app::Cereal| {
                began.elapsed().as_secs_f64() >= speak && recorded(view) >= speak - 0.5
            });
            // 2. Stop, and wait until the notes can be asked for.
            let _ = handle.update(cx, |view, _, cx| press(view, "stop", None, cx));
            wait_for!(Duration::from_secs(30), settled(a));
            // 3. Resume the same note.
            let _ = handle.update(cx, |view, _, cx| press(view, "resume", Some(a), cx));
            let began = Instant::now();
            wait_for!(Duration::from_secs(15), interim);
            wait_for!(Duration::from_secs(8), |_: &mut crate::app::Cereal| began.elapsed().as_secs_f64() >= 3.);
            // 4. Stop, and 5. at once record another new note.
            let _ = handle.update(cx, |view, _, cx| press(view, "stop_resumed", None, cx));
            cx.background_executor().timer(Duration::from_millis(20)).await;
            let Ok(Some(b)) = handle.update(cx, |view, _, cx| {
                let id = new_note(view, cx)?;
                press(view, "start_after_stop", Some(id), cx);
                Some(id)
            }) else {
                return quit(cx, "could not start the second note");
            };
            let began = Instant::now();
            wait_for!(Duration::from_secs(15), interim);
            wait_for!(Duration::from_secs(8), |_: &mut crate::app::Cereal| began.elapsed().as_secs_f64() >= 3.);
            // 6. Stop, and let everything settle.
            let _ = handle.update(cx, |view, _, cx| press(view, "stop_after_start", None, cx));
            wait_for!(Duration::from_secs(30), settled(b));
            wait_for!(Duration::from_secs(30), |view: &mut crate::app::Cereal| {
                !view.live.streams(a) && view.server_pending == 0
            });
            cx.background_executor().timer(Duration::from_millis(300)).await;
            check(handle, cx, round, a, "a");
            check(handle, cx, round, b, "b");
            eprintln!("latency bench: round {} of {rounds} done", round + 1);
        }
        quit(cx, "done");
    })
    .detach();
}
