//! Scripted walk through the app's transitions for motion review, run only
//! with `--motion-tour` (which also selects the offline preview fixtures).
//!
//! It changes view state and dispatches pointer input through the window's own
//! event listeners, so it never moves the real cursor. Each step is logged to
//! stderr with a wall-clock timestamp so screen captures can be lined up with
//! it. Combine with `CEREAL_SLOW_MOTION=8` to inspect transitions frame by
//! frame, or `CEREAL_REDUCE_MOTION=1` to review the reduced-motion fallback.
use crate::{
    app::{Cereal, Menu, NoteTab, Screen},
    motion::slow,
};
use gpui::*;
use std::time::Duration;

/// A step either changes view state or feeds the window pointer input. Input
/// is dispatched outside any view update, exactly as AppKit would deliver it.
enum Action {
    View(Box<dyn Fn(&mut Cereal, &mut Context<Cereal>)>),
    /// A view change that needs the window (typing into an input).
    Window(Box<dyn Fn(&mut Cereal, &mut Window, &mut Context<Cereal>)>),
    Input(Box<dyn Fn(&mut Window, &mut App)>),
}

type Step = (&'static str, u64, Action);

fn step(
    name: &'static str,
    hold: u64,
    action: impl Fn(&mut Cereal, &mut Context<Cereal>) + 'static,
) -> Step {
    (name, hold, Action::View(Box::new(action)))
}

fn input(name: &'static str, hold: u64, action: impl Fn(&mut Window, &mut App) + 'static) -> Step {
    (name, hold, Action::Input(Box::new(action)))
}

fn windowed(
    name: &'static str,
    hold: u64,
    action: impl Fn(&mut Cereal, &mut Window, &mut Context<Cereal>) + 'static,
) -> Step {
    (name, hold, Action::Window(Box::new(action)))
}

fn menu(name: &'static str, hold: u64, menu: Option<Menu>) -> Step {
    step(name, hold, move |view, _| view.menu = menu.clone())
}

fn pointer(window: &mut Window, cx: &mut App, x: f32, y: f32) {
    window.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position: point(px(x), px(y)),
            ..Default::default()
        }),
        cx,
    );
}

fn key(window: &mut Window, cx: &mut App, name: &str) {
    if let Ok(keystroke) = Keystroke::parse(name) {
        window.dispatch_keystroke(keystroke, cx);
    }
}

fn scroll(window: &mut Window, cx: &mut App, x: f32, y: f32, by: f32) {
    window.dispatch_event(
        PlatformInput::ScrollWheel(ScrollWheelEvent {
            position: point(px(x), px(y)),
            delta: ScrollDelta::Pixels(point(px(0.), px(by))),
            ..Default::default()
        }),
        cx,
    );
}

/// Forty wheel events, 50 ms apart: two seconds of steady scrolling, 90 px
/// at a time. A negative `by` moves down the content, a positive one up.
fn scrolling(steps: &mut Vec<Step>, x: f32, y: f32, by: f32) {
    for _ in 0..40 {
        steps.push((
            "b-wheel",
            50,
            Action::Input(Box::new(move |window, cx| scroll(window, cx, x, y, by))),
        ));
    }
}

/// Notes being generated, `p-fix-gen-…` (run with `CEREAL_TOUR=p-fix-gen`,
/// `--dark` for the dark theme): a recording just stopped, offering
/// "Generate notes" and "Resume"; the notes part-way written; the finished
/// notes with nested bullets and the chat link; the same scrolled so the
/// text runs under the bar. The notes are scripted; no server or model is
/// involved.
#[cfg(debug_assertions)]
fn notes_generation_steps() -> Vec<Step> {
    const TITLE: &str = "Tidepool UI critique and design fixes";
    const NOTES: &str = "# Tidepool UI Comparison\n- Benchmarking Tidepool against Lumen across several interaction patterns\n- Overall: Lumen's polish is noticeably ahead in animation, popover behavior and search\n\n# Popover and Animation Issues\n- Click-outside behavior: Lumen closes the popover; ours stays open\n  - Fix dismiss-on-outside-click\n- Open/close animation needs a full rework\n  - Lumen's popover grows out of the event block and shrinks back\n  - Ours pops in; corner radius looks wrong mid-animation\n- Sync shimmer: Lumen's is significantly better\n  - Unclear how it's done, but worth investigating\n\n# Toolbar and Layout Fixes\n- Time zone picker: Lumen places it beside the week header\n  - Week numbers toggle should follow the same pattern\n- Search field sits 2px low and is worse than Lumen's\n  - Needs a fix to match their expanding layout\n- Duration label on event blocks: purpose unclear, needs review\n- Colored event tabs: unclear what they map to\n\n# Elements to Remove\n- Popover minimize button: not needed\n- Help icon: definitely not needed\n- Toolbar hover tooltips: fine as they are\n\n---\n\nChat with meeting transcript: https://notes.example.com/notes/be954608-a942-4d1f-bb83-a6a252a5106e/chat";
    vec![
        step("p-fix-gen-recording", 1500, |view, cx| {
            view.navigate(Screen::Home, cx);
            if let Some(note) = view.workspace.note_mut(1) {
                note.enhanced.clear();
                note.generated_revision = None;
                note.title = String::new();
            }
            view.open_note(1, cx);
            view.active_tab = NoteTab::Notes;
            view.start_recording_preview(1, cx);
        }),
        step("p-fix-gen-stopped", 1500, |view, cx| {
            view.recording_last_click = None;
            view.recording_changed_at = None;
            view.stop_recording_preview(cx);
        }),
        step("p-fix-gen-streaming", 2500, |view, cx| {
            view.start_scripted_generation(1, Some(TITLE), NOTES, Some(620), cx);
        }),
        step("p-fix-gen-finished", 2500, |view, cx| {
            view.generation.cancel();
            view.generation_draft.clear();
            if let Some(note) = view.workspace.note_mut(1) {
                note.title = String::new();
            }
            view.start_scripted_generation(1, Some(TITLE), NOTES, None, cx);
        }),
        step("p-fix-gen-fade", 1500, |view, _| {
            view.note_scroll.set_offset(point(px(0.), px(-260.)));
        }),
    ]
}

#[cfg(not(debug_assertions))]
fn notes_generation_steps() -> Vec<Step> {
    Vec::new()
}

/// The real thing, end to end, `a-e2e-…` (debug builds; run with
/// `CEREAL_AUDIT=1 CEREAL_TOUR=a-e2e` against a throwaway server whose notes
/// are listed in `CEREAL_E2E_IDS`, a JSON file of kind → server note id).
/// It generates notes with the server's real model, opens a bullet's source
/// card, asks about the note and across notes, switches the model and asks
/// again, then regenerates. Each stretch that streams says on stderr when it
/// starts (`tour burst <label>`, for an outside tool to take frames) and how
/// long its first visible text and its end took, as the app saw them.
#[cfg(debug_assertions)]
fn e2e_steps() -> Vec<Step> {
    fn note_of(view: &Cereal, kind: &str) -> Option<u64> {
        let path = std::env::var("CEREAL_E2E_IDS").ok()?;
        let ids: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
        let server = ids.get(kind)?.as_str()?.to_owned();
        view.workspace
            .notes
            .iter()
            .find(|note| !note.trashed && note.server_id.as_deref() == Some(server.as_str()))
            .map(|note| note.id)
    }
    fn open(view: &mut Cereal, kind: &str, cx: &mut Context<Cereal>) -> Option<u64> {
        view.close_chat_surface(cx);
        view.navigate(Screen::Home, cx);
        let id = note_of(view, kind);
        match id {
            Some(id) => view.open_note(id, cx),
            None => eprintln!("tour: e2e: no note {kind:?}"),
        }
        id
    }
    fn stamp() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_millis())
    }
    /// Watch a generation of `id` until it is written out.
    fn watch_generation(label: &'static str, id: u64, cx: &mut Context<Cereal>) {
        eprintln!("tour {} burst {label}", stamp());
        let began = std::time::Instant::now();
        cx.spawn(async move |this, cx| {
            let (mut first, mut shown) = (None, None);
            for _ in 0..12_000 {
                cx.background_executor().timer(Duration::from_millis(10)).await;
                let Ok((received, visible, running, draft)) = this.update(cx, |view, _| {
                    (
                        view.generation_draft.showing(id),
                        !view.generation_draft.visible().is_empty(),
                        view.generation.is_running_for(id),
                        view.generation_draft.note_id.is_some(),
                    )
                }) else {
                    return;
                };
                if received && first.is_none() {
                    first = Some(began.elapsed().as_millis());
                }
                if visible && shown.is_none() {
                    shown = Some(began.elapsed().as_millis());
                }
                if !running && !draft && began.elapsed() > Duration::from_millis(300) {
                    let chars = this
                        .update(cx, |view, _| view.workspace.note(id).map_or(0, |note| note.enhanced.chars().count()))
                        .unwrap_or(0);
                    eprintln!(
                        "tour {} e2e {label}: first text received {first:?} ms, first text shown {shown:?} ms, written out {} ms, {chars} chars",
                        stamp(),
                        began.elapsed().as_millis()
                    );
                    return;
                }
            }
        })
        .detach();
    }
    /// Watch the answer being written to the last question on screen.
    fn watch_answer(label: &'static str, cx: &mut Context<Cereal>) {
        eprintln!("tour {} burst {label}", stamp());
        let began = std::time::Instant::now();
        cx.spawn(async move |this, cx| {
            let (mut thought, mut first) = (None, None);
            for _ in 0..12_000 {
                cx.background_executor().timer(Duration::from_millis(10)).await;
                let Ok((reasoning, text, streaming, chars)) = this.update(cx, |view, _| {
                    let index = view.chat_messages.len().saturating_sub(1);
                    let answer = view.chat_messages.last().map_or("", |(_, answer)| answer.as_str());
                    (
                        view.chat_annotations.get(&index).is_some_and(|notes| {
                            !notes.reasoning.is_empty() || !notes.tools.is_empty()
                        }),
                        !answer.trim().is_empty(),
                        view.chat_stream.is_some(),
                        answer.chars().count(),
                    )
                }) else {
                    return;
                };
                if reasoning && thought.is_none() {
                    thought = Some(began.elapsed().as_millis());
                }
                if text && first.is_none() {
                    first = Some(began.elapsed().as_millis());
                }
                if !streaming && began.elapsed() > Duration::from_millis(300) {
                    eprintln!(
                        "tour {} e2e {label}: first thought/lookup {thought:?} ms, first answer text {first:?} ms, done {} ms, {chars} chars",
                        stamp(),
                        began.elapsed().as_millis()
                    );
                    return;
                }
            }
        })
        .detach();
    }
    fn ask(view: &mut Cereal, label: &'static str, question: &str, cx: &mut Context<Cereal>) {
        view.assistant_open = true;
        view.submit_chat_question(question.into(), cx);
        watch_answer(label, cx);
    }
    /// The first top-level bullet of the open note's generated notes.
    fn first_bullet(view: &Cereal) -> Option<(u64, usize, String)> {
        let id = view.current_note?;
        let note = view.workspace.note(id)?;
        let (row, line) = note
            .enhanced
            .lines()
            .enumerate()
            .find(|(_, line)| line.starts_with("- "))?;
        Some((id, row, crate::summary::paragraph_text(line)))
    }
    fn top(view: &mut Cereal) {
        view.note_scroll.set_offset(point(px(0.), px(0.)));
    }
    let generate = |label: &'static str, kind: &'static str| {
        move |view: &mut Cereal, cx: &mut Context<Cereal>| {
            if let Some(id) = view.current_note.filter(|id| Some(*id) == note_of(view, kind)) {
                view.enhance_note_preview(id, cx);
                watch_generation(label, id, cx);
            }
        }
    };
    let mut steps = vec![
        step("a-e2e-01-stopped", 1500, |view, cx| {
            open(view, "sales-call", cx);
        }),
        step("a-e2e-02-generate-quiet", 30_000, generate("generate", "sales-call")),
        step("a-e2e-03-finished", 900, |view, _| top(view)),
        input("a-e2e-04-fade", 900, |window, cx| scroll(window, cx, 500., 400., -90_000.)),
        // Back at the top first, so the trigger is where it is drawn now.
        step("a-e2e-05-top-quiet", 900, |view, _| top(view)),
        step("a-e2e-05-evidence-quiet", 9000, |view, cx| {
            if let Some((id, row, text)) = first_bullet(view) {
                let trigger = crate::summary::probe::trigger(id, row)
                    .unwrap_or_else(|| Bounds::new(point(px(600.), px(260.)), size(px(24.), px(24.))));
                eprintln!("tour {} burst evidence", stamp());
                view.open_summary(id, row, text, trigger, cx);
            }
        }),
        step("a-e2e-06-evidence", 900, |_, _| {}),
        step("a-e2e-07-note-chat-quiet", 40_000, |view, cx| {
            view.summary.dismiss();
            ask(view, "note-chat", "What pricing did we discuss, and what are the agreed next steps?", cx);
        }),
        step("a-e2e-08-note-chat-answer", 900, |_, _| {}),
        step("a-e2e-09-note-chat-thoughts", 900, |view, _| {
            let index = view.chat_messages.len().saturating_sub(1);
            if let Some(notes) = view.chat_annotations.get_mut(&index) {
                notes.set_thoughts_open(Some(true));
            }
            view.chat_scroll.set_offset(point(px(0.), px(0.)));
        }),
        // Home first: leaving the note saves it, and a question is held
        // back while that request is on its way.
        step("a-e2e-10-home-quiet", 3000, |view, cx| {
            view.close_chat_surface(cx);
            view.navigate(Screen::Home, cx);
            view.clear_chat(cx);
        }),
        step("a-e2e-10-home-chat-quiet", 45_000, |view, cx| {
            eprintln!("tour {} e2e server requests pending {}", stamp(), view.server_pending);
            ask(view, "home-chat", "Across my meetings this week, which follow-ups are still open and who owns each one?", cx);
        }),
        step("a-e2e-11-home-chat-answer", 900, |_, _| {}),
        step("a-e2e-12-home-chat-thoughts", 900, |view, _| {
            let index = view.chat_messages.len().saturating_sub(1);
            if let Some(notes) = view.chat_annotations.get_mut(&index) {
                notes.set_thoughts_open(Some(true));
            }
            view.chat_scroll.set_offset(point(px(0.), px(0.)));
        }),
        step("a-e2e-13-model-picker", 900, |view, cx| {
            let index = view.chat_messages.len().saturating_sub(1);
            if let Some(notes) = view.chat_annotations.get_mut(&index) {
                notes.set_thoughts_open(Some(false));
            }
            view.chat_to_bottom();
            view.toggle_chat_model_picker(cx);
        }),
        input("a-e2e-14-model-down-quiet", 400, |window, cx| {
            press_key(window, cx, "down");
            press_key(window, cx, "down");
        }),
        input("a-e2e-15-model-chosen", 900, |window, cx| press_key(window, cx, "enter")),
        step("a-e2e-16-home-chat-2-quiet", 45_000, |view, cx| {
            eprintln!("tour {} e2e model now {:?}", stamp(), view.selected_chat_model);
            ask(view, "home-chat-2", "Which of those is most urgent, and why?", cx);
        }),
        step("a-e2e-17-home-chat-2-answer", 900, |_, _| {}),
        step("a-e2e-18-standup-quiet", 900, |view, cx| {
            view.close_chat_surface(cx);
            view.clear_chat(cx);
            open(view, "standup", cx);
        }),
        step("a-e2e-19-standup-generate-quiet", 30_000, generate("standup", "standup")),
        step("a-e2e-20-standup", 900, |view, _| top(view)),
        step("a-e2e-21-long-quiet", 1500, |view, cx| {
            open(view, "long-meeting", cx);
        }),
        step("a-e2e-22-long-regenerate-quiet", 80_000, generate("regenerate-long", "long-meeting")),
        step("a-e2e-23-long", 900, |view, _| top(view)),
    ];
    for name in ["a-e2e-24-long-2", "a-e2e-25-long-3", "a-e2e-26-long-4", "a-e2e-27-long-5", "a-e2e-28-long-6"] {
        steps.push(input(name, 600, |window, cx| scroll(window, cx, 500., 400., -560.)));
    }
    for (name, kind) in [
        ("a-e2e-30-one-on-one", "one-on-one"),
        ("a-e2e-31-customer-interview", "customer-interview"),
        ("a-e2e-32-planning", "planning-decisions"),
        ("a-e2e-33-recruiting", "recruiting"),
        ("a-e2e-34-no-content", "no-content"),
        ("a-e2e-35-mixed-language", "mixed-language"),
        ("a-e2e-36-crosstalk", "messy-crosstalk"),
    ] {
        steps.push(step(name, 900, move |view, cx| {
            open(view, kind, cx);
        }));
    }
    steps
}

#[cfg(not(debug_assertions))]
fn e2e_steps() -> Vec<Step> {
    Vec::new()
}

/// The update's film, `f-update-…` (debug builds; record it slowed with
/// `CEREAL_SLOW_MOTION` and speed the frames back up). `f-update-light` and
/// `f-update-dark` play the whole of it over Home: a version is found, its
/// "Update" is pressed, four seconds of a download that comes unevenly, a
/// second and a half of verifying, ready, and "Restart to update" pressed.
/// `f-update-fail-light` and `f-update-fail-dark` fail part of the way and
/// press "Try again". Nothing is asked of any server and nothing is fetched
/// or installed (`Updates::review("film")`); the presses are real presses
/// on the notice's own buttons, wherever they are drawn.
///
/// `f-about-…` is the same update taken in Settings → About, where its
/// version row is the stage and the notice is not shown: "Check for
/// updates" is pressed, the check finds a version a second and a half
/// later, and "Update to 0.2.0" and "Restart to update" are pressed as
/// above, after which the row is put back as it was. `f-about-fail-…`
/// fails part of the way and presses "Try again"; `f-about-uptodate-…`
/// checks and finds nothing newer; `f-about-tight-…` is the whole of it
/// beside the widest sidebar (record it at 900 px wide: the narrowest the
/// row gets).
#[cfg(debug_assertions)]
fn update_film_steps() -> Vec<Step> {
    #[derive(Clone, Copy)]
    enum Mouse {
        Over,
        Down,
        Up,
        Away,
    }
    fn mouse(name: String, hold: u64, what: Mouse) -> Step {
        press(name, hold, what, "update-action")
    }
    fn press(name: String, hold: u64, what: Mouse, action: &'static str) -> Step {
        input(name.leak(), hold, move |window, cx| {
            let Some(at) = crate::onboarding_view::placed(action).map(|bounds| bounds.center()) else {
                eprintln!("tour: the update's action is not on screen");
                return;
            };
            // Where the pointer waits between presses: up and to the left
            // of the notice, over the page.
            let aside = point(at.x - px(150.), at.y - px(96.));
            let event = match what {
                Mouse::Over => PlatformInput::MouseMove(MouseMoveEvent {
                    position: at,
                    ..Default::default()
                }),
                Mouse::Down => PlatformInput::MouseDown(MouseDownEvent {
                    button: MouseButton::Left,
                    position: at,
                    click_count: 1,
                    ..Default::default()
                }),
                Mouse::Up => PlatformInput::MouseUp(MouseUpEvent {
                    button: MouseButton::Left,
                    position: at,
                    click_count: 1,
                    ..Default::default()
                }),
                // Off the window, as far as the app is told: nothing is
                // left lit under it.
                Mouse::Away => PlatformInput::MouseMove(MouseMoveEvent {
                    position: point(px(-40.), px(-40.)),
                    ..Default::default()
                }),
            };
            // The pointer that is drawn (the real one is never moved): its
            // tip a little inside the button.
            crate::update_view::film_pointer(Some(match what {
                Mouse::Away => aside,
                _ => point(at.x + px(6.), at.y + px(3.)),
            }));
            window.dispatch_event(event, cx);
            window.refresh();
        })
    }
    let mut steps = Vec::new();
    for (theme, dark) in [("light", false), ("dark", true)] {
        for (kind, state) in [("", "film"), ("-fail", "film-fail")] {
            let name = |part: &str| format!("f-update{kind}-{theme}-{part}");
            steps.push(step(name("home").leak(), 1400, move |view, cx| {
                view.menu = None;
                view.workspace.settings.force_dark(dark);
                view.navigate(Screen::Home, cx);
                view.updates.review("idle");
            }));
            // Found: the notice comes up, the mark drops in, its flakes hop.
            steps.push(step(name("found").leak(), 1700, move |view, _| view.updates.review(state)));
            // The pointer comes on beside the notice, then goes to "Update".
            steps.push(mouse(name("pointer"), 700, Mouse::Away));
            steps.push(mouse(name("over"), 620, Mouse::Over));
            steps.push(mouse(name("down"), 130, Mouse::Down));
            steps.push(mouse(name("up"), 380, Mouse::Up));
            if state == "film" {
                // The download, the verifying and the pop, then a moment
                // at rest.
                steps.push(mouse(name("away"), 4000 + 1500 + 1900 - 380, Mouse::Away));
                steps.push(mouse(name("restart-over"), 700, Mouse::Over));
                steps.push(mouse(name("restart-down"), 150, Mouse::Down));
                steps.push(mouse(name("restart-up"), 300, Mouse::Up));
                steps.push(mouse(name("restart-away"), 1700, Mouse::Away));
            } else {
                // It fails 2.3 seconds in; the bowl tips and runs out.
                steps.push(mouse(name("away"), 2300 + 2600 - 380, Mouse::Away));
                steps.push(mouse(name("again-over"), 700, Mouse::Over));
                steps.push(mouse(name("again-down"), 150, Mouse::Down));
                steps.push(mouse(name("again-up"), 300, Mouse::Up));
                steps.push(mouse(name("again-away"), 2200, Mouse::Away));
            }
            steps.push(step(name("end").leak(), 500, |view, _| view.updates.review("idle")));
        }
    }
    // The notice and the row are one stage (`f-about-handoff`): a notice
    // that is up over Home leaves when Settings → About comes on, where its
    // row is already the update's card, and is back when the page is left.
    steps.push(step("f-about-handoff-home", 1400, |view, cx| {
        view.menu = None;
        view.workspace.settings.force_dark(false);
        view.updates.review("idle");
        view.navigate(Screen::Home, cx);
    }));
    steps.push(step("f-about-handoff-found", 2000, |view, _| view.updates.review("found")));
    steps.push(step("f-about-handoff-about", 1500, |view, cx| {
        view.open_settings(crate::screens::SettingsTab::About, cx)
    }));
    steps.push(step("f-about-handoff-back", 1500, |view, cx| view.navigate(Screen::Home, cx)));
    steps.push(step("f-about-handoff-end", 500, |view, _| view.updates.review("idle")));
    // Each state of the row held for a while (`f-about-rest`, with
    // `CEREAL_FRAME_STATS`): what waits for the person is drawn once and
    // then costs nothing.
    steps.push(step("f-about-rest-home", 1400, |view, cx| {
        view.menu = None;
        view.workspace.settings.force_dark(false);
        view.updates.review("idle");
        view.open_settings(crate::screens::SettingsTab::About, cx);
    }));
    for (name, state, hold) in [
        ("f-about-rest-idle", "idle", 3000),
        ("f-about-rest-available", "available", 4000),
        ("f-about-rest-ready", "ready", 4000),
        ("f-about-rest-failed", "failed", 4000),
        ("f-about-rest-uptodate", "uptodate", 7000),
        ("f-about-rest-end", "idle", 500),
    ] {
        steps.push(step(name, hold, move |view, _| view.updates.review(state)));
    }
    // Settings → About.
    let check = (crate::update_view::SCRIPTED_CHECK * 1000.) as u64;
    for (theme, dark) in [("light", false), ("dark", true)] {
        for (kind, state) in [
            ("", "film-check"),
            ("-fail", "film-check-fail"),
            ("-uptodate", "film-uptodate"),
            ("-tight", "film-check"),
        ] {
            let name = |part: &str| format!("f-about{kind}-{theme}-{part}");
            let about = |name: String, hold: u64, what: Mouse| {
                press(name, hold, what, crate::update_view::ABOUT_ACTION)
            };
            let tight = kind == "-tight";
            steps.push(step(name("home").leak(), 1400, move |view, cx| {
                view.menu = None;
                view.workspace.settings.force_dark(dark);
                if tight {
                    view.sidebar_width = crate::desktop_preferences::SIDEBAR_MAX;
                }
                view.updates.review("idle");
                view.open_settings(crate::screens::SettingsTab::About, cx);
            }));
            // The row at rest, with the film's answers ready.
            steps.push(step(name("found").leak(), 900, move |view, _| view.updates.review(state)));
            // "Check for updates".
            steps.push(about(name("pointer"), 700, Mouse::Away));
            steps.push(about(name("over"), 620, Mouse::Over));
            steps.push(about(name("down"), 130, Mouse::Down));
            steps.push(about(name("up"), 380, Mouse::Up));
            if state == "film-uptodate" {
                // The check, "You're up to date." for as long as it is
                // said, and the row as it was.
                let said = (crate::update_view::PLEASED_FOR * 1000.) as u64;
                steps.push(about(name("away"), check + said + 1300 - 380, Mouse::Away));
            } else {
                // The check, then the row growing into the update's card
                // and the mark dropping in.
                steps.push(about(name("away"), check + 1900 - 380, Mouse::Away));
                steps.push(about(name("update-over"), 620, Mouse::Over));
                steps.push(about(name("update-down"), 130, Mouse::Down));
                steps.push(about(name("update-up"), 380, Mouse::Up));
                if state == "film-check" {
                    steps.push(about(name("update-away"), 4000 + 1500 + 1900 - 380, Mouse::Away));
                    steps.push(about(name("restart-over"), 700, Mouse::Over));
                    steps.push(about(name("restart-down"), 150, Mouse::Down));
                    steps.push(about(name("restart-up"), 300, Mouse::Up));
                    steps.push(about(name("restart-away"), 1700, Mouse::Away));
                } else {
                    steps.push(about(name("update-away"), 2300 + 2600 - 380, Mouse::Away));
                    steps.push(about(name("again-over"), 700, Mouse::Over));
                    steps.push(about(name("again-down"), 150, Mouse::Down));
                    steps.push(about(name("again-up"), 300, Mouse::Up));
                    // The second try goes through.
                    steps.push(about(name("again-away"), 4000 + 1500 + 1900 - 300, Mouse::Away));
                }
                // Put back: the row eases to a row again.
                steps.push(step(name("rest").leak(), 1300, |view, _| view.updates.review("idle")));
            }
            steps.push(step(name("end").leak(), 500, move |view, _| {
                view.updates.review("idle");
                if tight {
                    view.sidebar_width = crate::desktop_preferences::SIDEBAR_DEFAULT;
                }
            }));
        }
    }
    steps
}

#[cfg(not(debug_assertions))]
fn update_film_steps() -> Vec<Step> {
    Vec::new()
}

/// States to sit in while an outside tool samples the process (`i-…`): each
/// holds still for fourteen seconds and then reports how often it rendered.
fn idle_steps() -> Vec<Step> {
    vec![
        step("i-home", 14_000, |view, cx| view.navigate(Screen::Home, cx)),
        step("i-note", 14_000, |view, cx| view.open_note(1, cx)),
        step("i-paused-setup", 2500, |view, cx| {
            view.open_note(1, cx);
            view.start_recording_preview(1, cx);
        }),
        step("i-paused", 14_000, |view, cx| {
            view.stop_recording_preview(cx)
        }),
        // Run with `--view=pill`: the capsule is up and a preview recording
        // is live, with nothing changing.
        step("i-pill", 14_000, |_, _| {}),
    ]
}

/// Keyboard reach and focus rings, `p-key-…`: Tab from the Home screen, a
/// control pressed with Return, the note's fields, and a control clicked
/// with the pointer (which takes the focus but shows no ring). Each step
/// prints what it found.
fn keyboard_steps() -> Vec<Step> {
    fn tabs(window: &mut Window, cx: &mut App, count: usize, back: bool) {
        for _ in 0..count {
            key(window, cx, if back { "shift-tab" } else { "tab" });
        }
    }
    vec![
        step("p-key-home", 5000, |view, cx| view.navigate(Screen::Home, cx)),
        // The first stop is the first control drawn: the sidebar's toggle.
        input("p-key-first-stop", 5000, |window, cx| tabs(window, cx, 1, false)),
        // A real key press is a key down and a key up; the press is on the up.
        input("p-key-return-presses-it", 5000, |window, cx| {
            let keystroke = Keystroke::parse("enter").expect("enter is a key");
            window.dispatch_event(
                PlatformInput::KeyDown(KeyDownEvent {
                    keystroke: keystroke.clone(),
                    is_held: false,
                }),
                cx,
            );
            window.dispatch_event(PlatformInput::KeyUp(KeyUpEvent { keystroke }), cx);
        }),
        step("p-key-return-check", 600, |view, _| {
            eprintln!(
                "key check: Return on the first stop (the sidebar toggle) {} the sidebar",
                if view.sidebar_open { "did not close" } else { "closed" }
            );
            view.sidebar_open = true;
        }),
        input("p-key-eight-stops-on", 5000, |window, cx| tabs(window, cx, 8, false)),
        input("p-key-two-stops-back", 5000, |window, cx| tabs(window, cx, 2, true)),
        // A pointer press on a control: it takes the focus, without a ring.
        input("p-key-pointer-press", 5000, |window, cx| {
            let at = point(px(134.), px(24.));
            pointer(window, cx, 134., 24.);
            for down in [true, false] {
                let event = if down {
                    PlatformInput::MouseDown(MouseDownEvent {
                        button: MouseButton::Left,
                        position: at,
                        click_count: 1,
                        ..Default::default()
                    })
                } else {
                    PlatformInput::MouseUp(MouseUpEvent {
                        button: MouseButton::Left,
                        position: at,
                        click_count: 1,
                        ..Default::default()
                    })
                };
                window.dispatch_event(event, cx);
            }
        }),
        step("p-key-search-close", 800, |view, _| view.search_open = false),
        // In a note: Tab leaves the title for the next field, the body keeps
        // Tab as a character, and Shift-Tab leaves the body.
        windowed("p-key-note-title", 1500, |view, window, cx| {
            view.open_note(1, cx);
            view.active_tab = NoteTab::Notes;
            view.title_input.read(cx).focus_handle.focus(window);
        }),
        input("p-key-tab-from-title", 800, |window, cx| tabs(window, cx, 1, false)),
        windowed("p-key-title-check", 600, |view, window, cx| {
            let title = view.title_input.read(cx);
            eprintln!(
                "key check: Tab in the title {} it (title holds a tab character: {})",
                if title.focus_handle.is_focused(window) { "stayed in" } else { "left" },
                title.content().contains('\t')
            );
            view.editor.read(cx).focus_handle.focus(window);
        }),
        input("p-key-tab-in-body", 800, |window, cx| tabs(window, cx, 1, false)),
        windowed("p-key-body-check", 600, |view, window, cx| {
            let body = view.editor.read(cx);
            eprintln!(
                "key check: Tab in the note body {} (body holds a tab character: {})",
                if body.focus_handle.is_focused(window) { "stays in the body" } else { "left the body" },
                body.content().contains('\t')
            );
        }),
        input("p-key-shift-tab-from-body", 5000, |window, cx| tabs(window, cx, 1, true)),
        windowed("p-key-body-left-check", 600, |view, window, cx| {
            eprintln!(
                "key check: Shift-Tab in the note body {} it",
                if view.editor.read(cx).focus_handle.is_focused(window) { "stayed in" } else { "left" }
            );
        }),
        // Every stop of the note view, counted by walking them all once.
        windowed("p-key-count", 600, |view, window, cx| {
            let start = window.focused(cx);
            let mut stops = 0;
            for _ in 0..400 {
                window.focus_next();
                stops += 1;
                if window.focused(cx) == start {
                    break;
                }
            }
            eprintln!("key check: the note view has {stops} keyboard stops, and Tab comes back round to where it started");
            let _ = view;
        }),
    ]
}

/// A key press as the keyboard sends it: down, then up.
fn press_key(window: &mut Window, cx: &mut App, name: &str) {
    let keystroke = Keystroke::parse(name).expect("a key name");
    window.dispatch_event(
        PlatformInput::KeyDown(KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
        }),
        cx,
    );
    window.dispatch_event(PlatformInput::KeyUp(KeyUpEvent { keystroke }), cx);
}

/// A pointer click at a point of the window.
fn click(window: &mut Window, cx: &mut App, x: f32, y: f32) {
    let position = point(px(x), px(y));
    pointer(window, cx, x, y);
    window.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            button: MouseButton::Left,
            position,
            click_count: 1,
            ..Default::default()
        }),
        cx,
    );
    window.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            button: MouseButton::Left,
            position,
            click_count: 1,
            ..Default::default()
        }),
        cx,
    );
}

/// The search palette's Return and the new-note shortcuts, `p-palette-…`;
/// the space banner and whether closing it is remembered, `p-banner-…`
/// (run it twice on one `CEREAL_DATA_DIR`); and the scroll thumb,
/// `p-thumb-…` (capture within a second of each step: the thumb leaves).
fn polish_steps() -> Vec<Step> {
    // Whether the note that is open is a new one: untitled and empty.
    fn fresh(view: &Cereal) -> bool {
        view.current_note
            .and_then(|id| view.workspace.note(id))
            .is_some_and(|note| note.title.is_empty() && note.body.is_empty())
    }
    fn home(view: &mut Cereal, cx: &mut Context<Cereal>) {
        view.finish_recording_preview(cx);
        view.recording_last_click = None;
        view.recording_changed_at = None;
        view.navigate(Screen::Home, cx);
    }
    let shortcut = |name: &'static str, keys: &'static str| {
        [
            input(name, 1500, move |window, cx| key(window, cx, keys)),
            step("p-palette-check", 600, move |view, cx| {
                eprintln!(
                    "palette check: {keys} {} a new note",
                    if fresh(view) { "made" } else { "did not make" }
                );
                home(view, cx);
            }),
        ]
    };
    let mut steps = vec![
        step("p-palette-home", 1500, |view, cx| view.navigate(Screen::Home, cx)),
        windowed("p-palette-open", 5000, |view, window, cx| view.open_search(window, cx)),
        input("p-palette-return", 5000, |window, cx| press_key(window, cx, "enter")),
        step("p-palette-check", 600, |view, cx| {
            eprintln!(
                "palette check: Return with nothing typed {} a new note; the palette is {}",
                if fresh(view) { "made" } else { "did not make" },
                if view.search_open { "still open" } else { "closed" }
            );
            home(view, cx);
        }),
        windowed("p-palette-reopen", 1500, |view, window, cx| view.open_search(window, cx)),
        input("p-palette-down", 5000, |window, cx| press_key(window, cx, "down")),
        input("p-palette-down-return", 1500, |window, cx| press_key(window, cx, "enter")),
        step("p-palette-check", 600, |view, cx| {
            eprintln!(
                "palette check: Down then Return opened {:?} (the row under New note is the first meeting)",
                view.screen
            );
            view.search_open = false;
            home(view, cx);
        }),
    ];
    steps.extend(shortcut("p-palette-shift-cmd-n", "cmd-shift-n"));
    steps.extend(shortcut("p-palette-cmd-n", "cmd-n"));
    // Shortcuts still work after the keyboard has left a note whose text
    // had the focus: ⌘1 goes Home, and ⌘N there makes a note.
    steps.extend([
        windowed("p-palette-in-note", 1500, |view, window, cx| {
            view.open_note(1, cx);
            view.editor.read(cx).focus_handle.focus(window);
        }),
        input("p-palette-cmd-1", 1500, |window, cx| key(window, cx, "cmd-1")),
        step("p-palette-check", 600, |view, _| {
            eprintln!("palette check: cmd-1 from a note's text went to {:?}", view.screen);
        }),
    ]);
    steps.extend(shortcut("p-palette-cmd-n-after-leaving", "cmd-n"));
    steps.extend([
        step("p-banner-my-notes", 5000, |view, cx| {
            eprintln!(
                "banner check: at launch the space banner is {}",
                if view.workspace.settings.space_banner_dismissed { "closed" } else { "shown" }
            );
            view.navigate(Screen::MyNotes, cx)
        }),
        // The banner's close button.
        input("p-banner-close", 5000, |window, cx| {
            let x = crate::environment::var("CEREAL_TOUR_BANNER_X").ok().and_then(|x| x.parse().ok());
            let y = crate::environment::var("CEREAL_TOUR_BANNER_Y").ok().and_then(|y| y.parse().ok());
            click(window, cx, x.unwrap_or(1035.), y.unwrap_or(309.));
        }),
        step("p-banner-check", 600, |view, _| {
            eprintln!(
                "banner check: after a click on its close button the banner is {}",
                if view.workspace.settings.space_banner_dismissed { "closed" } else { "shown" }
            );
        }),
        // The audio menu and its caption.
        step("p-caption-audio-menu", 5000, |view, cx| {
            view.navigate(Screen::Home, cx);
            view.open_note(1, cx);
            view.transcript_open = true;
            view.transcript_epoch = view.transcript_epoch.wrapping_add(1);
            view.menu = Some(Menu::RecordingSettings);
        }),
        step("p-caption-close", 800, |view, _| {
            view.menu = None;
            view.transcript_open = false;
            view.transcript_epoch = view.transcript_epoch.wrapping_add(1);
        }),
        step("p-thumb-setup", 2500, |view, cx| {
            hour_long_note(view, cx);
            view.transcript_open = true;
            view.transcript_epoch = view.transcript_epoch.wrapping_add(1);
        }),
        step("p-thumb-transcript-at-rest", 2500, |_, _| {}),
        input("p-thumb-transcript-scrolled", 300, |window, cx| scroll(window, cx, 600., 520., 4000.)),
    ]);
    // Three seconds of slow scrolling, so that a capture taken at any point
    // of it shows the thumb.
    for _ in 0..20 {
        steps.push(input("p-thumb-transcript-wheel", 150, |window, cx| {
            scroll(window, cx, 600., 520., 60.)
        }));
    }
    steps.extend([
        step("p-thumb-transcript-a-moment-later", 4000, |_, _| {}),
        step("p-thumb-close-transcript", 2500, |view, _| {
            view.transcript_open = false;
            view.transcript_epoch = view.transcript_epoch.wrapping_add(1);
            view.active_tab = NoteTab::Enhanced;
        }),
        input("p-thumb-document-scrolled", 300, |window, cx| scroll(window, cx, 600., 420., -6000.)),
    ]);
    for _ in 0..20 {
        steps.push(input("p-thumb-document-wheel", 150, |window, cx| {
            scroll(window, cx, 600., 420., -60.)
        }));
    }
    steps.push(step("p-thumb-document-a-moment-later", 4000, |_, _| {}));
    steps
}

fn steps() -> Vec<Step> {
    // The sidebar search tool: hovered, pressed, then released to open search.
    const SEARCH_TOOL: (f32, f32) = (134., 24.);
    vec![
        step("rest", 500, |_, _| {}),
        step("nav-chat", 750, |view, cx| view.navigate(Screen::Chat, cx)),
        step("nav-my-notes", 750, |view, cx| {
            view.navigate(Screen::MyNotes, cx)
        }),
        step("nav-home", 750, |view, cx| view.navigate(Screen::Home, cx)),
        input("hover-row", 450, |window, cx| {
            pointer(window, cx, 96., 100.)
        }),
        input("hover-tool", 450, |window, cx| {
            pointer(window, cx, SEARCH_TOOL.0, SEARCH_TOOL.1)
        }),
        input("press-tool", 350, |window, cx| {
            window.dispatch_event(
                PlatformInput::MouseDown(MouseDownEvent {
                    button: MouseButton::Left,
                    position: point(px(SEARCH_TOOL.0), px(SEARCH_TOOL.1)),
                    click_count: 1,
                    ..Default::default()
                }),
                cx,
            );
        }),
        input("release-tool-opens-search", 900, |window, cx| {
            window.dispatch_event(
                PlatformInput::MouseUp(MouseUpEvent {
                    button: MouseButton::Left,
                    position: point(px(SEARCH_TOOL.0), px(SEARCH_TOOL.1)),
                    click_count: 1,
                    ..Default::default()
                }),
                cx,
            );
            pointer(window, cx, 600., 700.);
        }),
        // Real key events: the highlight glides to where Return will go.
        input("search-key-down", 320, |window, cx| key(window, cx, "down")),
        input("search-key-down-again", 320, |window, cx| {
            key(window, cx, "down")
        }),
        input("search-key-up", 320, |window, cx| key(window, cx, "up")),
        step("search-close", 600, |view, _| view.search_open = false),
        step("sidebar-close", 850, |view, _| view.sidebar_open = false),
        step("sidebar-open", 850, |view, _| view.sidebar_open = true),
        menu("menu-workspace", 800, Some(Menu::Workspace)),
        step("menu-row-0", 300, |view, _| {
            view.menu_keyboard.selected = Some(0)
        }),
        step("menu-row-1", 300, |view, _| {
            view.menu_keyboard.selected = Some(1)
        }),
        step("menu-row-4", 450, |view, _| {
            view.menu_keyboard.selected = Some(4)
        }),
        menu("menu-close", 550, None),
        menu("menu-reopen", 90, Some(Menu::Workspace)),
        menu("menu-interrupt-close", 70, None),
        menu("menu-interrupt-reopen", 700, Some(Menu::Workspace)),
        menu("menu-close-again", 550, None),
        menu("dialog-shortcuts", 800, Some(Menu::Shortcuts)),
        menu("dialog-close", 550, None),
        step("toast", 900, |view, cx| {
            view.show_toast("Note copied to clipboard", cx)
        }),
        step("toast-replace", 800, |view, cx| {
            view.show_toast("All changes saved", cx)
        }),
        step("toast-dismiss", 550, |view, _| view.toast = None),
        // The Preferences tab, where the toggle the next steps flip is shown.
        step("settings-open", 900, |view, cx| {
            view.open_settings(crate::screens::SettingsTab::Preferences, cx)
        }),
        // In memory only: the tour never persists a changed preference.
        step("settings-toggle", 700, |view, _| {
            view.workspace.settings.force_dark(!view.workspace.settings.dark_mode)
        }),
        step("settings-toggle-back", 700, |view, _| {
            view.workspace.settings.force_dark(!view.workspace.settings.dark_mode)
        }),
        step("note-open", 950, |view, cx| view.open_note(1, cx)),
        step("tab-notes", 750, |view, _| view.active_tab = NoteTab::Notes),
        step("tab-enhanced", 750, |view, _| {
            view.active_tab = NoteTab::Enhanced
        }),
        menu("menu-note", 750, Some(Menu::Note)),
        menu("menu-share-in-place", 750, Some(Menu::Share)),
        menu("menu-share-close", 550, None),
        menu("details", 750, Some(Menu::NoteDetails)),
        menu("details-event-picker", 750, Some(Menu::CalendarEvent)),
        menu("details-picker-folds", 650, Some(Menu::NoteDetails)),
        menu("details-close", 550, None),
        step("transcript-open", 950, |view, _| {
            view.transcript_open = true;
            view.transcript_epoch = view.transcript_epoch.wrapping_add(1);
        }),
        step("transcript-close", 750, |view, _| {
            view.transcript_open = false;
            view.transcript_epoch = view.transcript_epoch.wrapping_add(1);
        }),
        step("chat-open", 850, |view, _| view.assistant_open = true),
        step("chat-answer-grows", 950, |view, cx| {
            view.submit_chat_question("List recent todos".into(), cx)
        }),
        step("chat-close", 750, |view, cx| view.close_chat_surface(cx)),
        step("record-start", 1300, |view, cx| {
            view.start_recording_preview(1, cx)
        }),
        step("level-loud", 350, |view, _| {
            view.recording.set_audio_levels(0.85, 0.2)
        }),
        step("level-quiet", 350, |view, _| {
            view.recording.set_audio_levels(0.08, 0.)
        }),
        step("level-speech", 350, |view, _| {
            view.recording.set_audio_levels(0.45, 0.3)
        }),
        step("level-silent", 450, |view, _| {
            view.recording.set_audio_levels(0., 0.)
        }),
        step("record-stop", 1100, |view, cx| {
            view.stop_recording_preview(cx)
        }),
        // Local generation takes 1.5 s; the chip then leaves and the notes
        // arrive, once.
        step("generate-notes", 2600, |view, cx| {
            view.enhance_note_preview(1, cx)
        }),
        step("home-returns", 1000, |view, cx| {
            view.navigate(Screen::Home, cx)
        }),
    ]
}

/// Still states for layout review, one per step, named `p-…`. They run only
/// when asked for by name (`CEREAL_TOUR=p-`), never as part of the motion tour.
fn still_steps() -> Vec<Step> {
    const ROW: (f32, f32) = (640., 557.);
    let settings = |name: &'static str, tab: crate::screens::SettingsTab| {
        step(name, 900, move |view, cx| view.open_settings(tab, cx))
    };
    vec![
        step("p-home", 900, |view, cx| view.navigate(Screen::Home, cx)),
        input("p-row-hover", 900, |window, cx| {
            pointer(window, cx, ROW.0, ROW.1)
        }),
        input("p-row-hover-settled", 900, |window, cx| {
            pointer(window, cx, ROW.0 + 1., ROW.1)
        }),
        input("p-row-menu", 900, |window, cx| {
            window.dispatch_event(
                PlatformInput::MouseDown(MouseDownEvent {
                    button: MouseButton::Right,
                    position: point(px(ROW.0), px(ROW.1)),
                    click_count: 1,
                    ..Default::default()
                }),
                cx,
            );
        }),
        menu("p-row-menu-close", 500, None),
        input("p-event-hover", 900, |window, cx| {
            pointer(window, cx, 760., 186.)
        }),
        input("p-footer-tip", 900, |window, cx| {
            pointer(window, cx, 22., 775.)
        }),
        input("p-pointer-away", 300, |window, cx| {
            pointer(window, cx, 1100., 420.)
        }),
        menu("p-workspace-menu", 900, Some(Menu::Workspace)),
        menu("p-workspace-menu-close", 500, None),
        step("p-search", 900, |view, _| {
            view.search_open = true;
        }),
        step("p-search-close", 500, |view, _| view.search_open = false),
        step("p-shared", 900, |view, cx| {
            view.navigate(Screen::Shared, cx)
        }),
        step("p-chat", 900, |view, cx| view.navigate(Screen::Chat, cx)),
        step("p-my-notes", 900, |view, cx| {
            view.navigate(Screen::MyNotes, cx)
        }),
        settings(
            "p-settings-preferences",
            crate::screens::SettingsTab::Preferences,
        ),
        settings("p-settings-profile", crate::screens::SettingsTab::Profile),
        settings("p-settings-calendar", crate::screens::SettingsTab::Calendar),
        settings("p-settings-about", crate::screens::SettingsTab::About),
        step("p-note", 1100, |view, cx| view.open_note(1, cx)),
        menu("p-note-menu", 900, Some(Menu::Note)),
        menu("p-note-templates", 900, Some(Menu::Templates)),
        menu("p-note-details", 900, Some(Menu::NoteDetails)),
        menu("p-note-share", 900, Some(Menu::Share)),
        menu("p-note-menu-close", 500, None),
        step("p-note-my-notes-tab", 900, |view, _| {
            view.active_tab = NoteTab::Notes
        }),
        step("p-note-enhanced-tab", 600, |view, _| {
            view.active_tab = NoteTab::Enhanced
        }),
        // The source card of the first bullet in the enhanced notes.
        step("p-summary-card", 1300, |view, cx| {
            view.active_tab = NoteTab::Enhanced;
            let found = view.workspace.note(1).and_then(|note| {
                note.enhanced
                    .lines()
                    .enumerate()
                    .find(|(_, line)| line.trim_start().starts_with("- "))
                    .map(|(row, line)| (row, crate::summary::paragraph_text(line)))
            });
            if let Some((row, paragraph)) = found {
                let trigger = Bounds::new(point(px(880.), px(330.)), size(px(24.), px(24.)));
                view.open_summary(1, row, paragraph, trigger, cx);
            }
        }),
        step("p-summary-card-close", 600, |view, _| {
            view.summary.dismiss()
        }),
        step("p-transcript", 1100, |view, _| {
            view.transcript_open = true;
            view.transcript_epoch = view.transcript_epoch.wrapping_add(1);
        }),
        step("p-transcript-close", 700, |view, _| {
            view.transcript_open = false;
            view.transcript_epoch = view.transcript_epoch.wrapping_add(1);
        }),
        step("p-chat-panel", 1100, |view, _| view.assistant_open = true),
        step("p-chat-panel-answer", 1100, |view, cx| {
            view.submit_chat_question("List recent todos".into(), cx)
        }),
        step("p-chat-thread", 1100, |view, cx| {
            view.close_chat_surface(cx);
            view.navigate(Screen::Chat, cx);
            view.submit_chat_question("List recent todos".into(), cx);
        }),
        // Last: the sign-in screen, which follows the app's theme.
        step("p-login", 1100, |view, cx| view.navigate(Screen::Login, cx)),
    ]
}

/// The screens where the navigation, document, people, sidebar, slider,
/// source and transcript icons are drawn, `p-icons-…`: stills for comparing
/// icon artwork, each held long enough to capture. Run it in the light theme
/// and again with `--dark`.
fn icon_steps() -> Vec<Step> {
    vec![
        step("p-icons-home", 4000, |view, cx| {
            view.sidebar_open = true;
            view.navigate(Screen::Home, cx)
        }),
        // The heading's controls show only under the pointer.
        input("p-icons-home-heading", 4000, |window, cx| {
            pointer(window, cx, 700., 72.)
        }),
        input("p-icons-pointer-away", 4000, |window, cx| {
            pointer(window, cx, 1100., 420.)
        }),
        step("p-icons-sidebar-closed", 4000, |view, _| view.sidebar_open = false),
        step("p-icons-search", 4000, |view, _| {
            view.sidebar_open = true;
            view.search_open = true;
        }),
        step("p-icons-shared", 4000, |view, cx| {
            view.search_open = false;
            view.navigate(Screen::Shared, cx)
        }),
        step("p-icons-chat", 4000, |view, cx| view.navigate(Screen::Chat, cx)),
        // Settings reached from Chat: the header's way back carries the chat
        // icon.
        step("p-icons-back-to-chat", 4000, |view, cx| {
            view.open_settings(crate::screens::SettingsTab::Preferences, cx)
        }),
        // A note: its footer carries the transcript button.
        step("p-icons-note", 4000, |view, cx| {
            view.navigate(Screen::Home, cx);
            view.open_note(1, cx)
        }),
        // The source button of the first bullet in the enhanced notes.
        step("p-icons-summary", 4000, |view, cx| {
            view.active_tab = NoteTab::Enhanced;
            let found = view.workspace.note(1).and_then(|note| {
                note.enhanced
                    .lines()
                    .enumerate()
                    .find(|(_, line)| line.trim_start().starts_with("- "))
                    .map(|(row, line)| (row, crate::summary::paragraph_text(line)))
            });
            if let Some((row, paragraph)) = found {
                let trigger = Bounds::new(point(px(880.), px(330.)), size(px(24.), px(24.)));
                view.open_summary(1, row, paragraph, trigger, cx);
            }
        }),
        // In memory only: the tour's data directory is thrown away.
        step("p-icons-trash", 4000, |view, cx| {
            view.summary.dismiss();
            if let Some(note) = view.workspace.note_mut(2) {
                note.trashed = true;
            }
            view.open_trash(cx)
        }),
        step("p-icons-empty-home", 4000, |view, cx| {
            for note in &mut view.workspace.notes {
                note.trashed = true;
            }
            view.navigate(Screen::Home, cx)
        }),
    ]
}

/// The chat against its reference, `p-ask-…`:
/// Home's composer at rest, focused and let go, a question unfolding the
/// conversation over Home while it thinks and once it has answered, the
/// panel folding back, and the Chat page with its threads. The answer is
/// scripted (`start_scripted_chat`): no server is involved.
#[cfg(debug_assertions)]
fn chat_parity_steps() -> Vec<Step> {
    fn script(think: u64) -> std::sync::mpsc::Receiver<crate::server::ChatStreamEvent> {
        use crate::server::ChatStreamEvent;
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let tool = |status: &str| {
                serde_json::from_value(serde_json::json!({
                    "callId": "t1",
                    "label": "Searching recent meetings",
                    "name": "list_meetings",
                    "status": status,
                    "resultCount": 3,
                }))
                .ok()
            };
            if let Some(tool) = tool("running") {
                let _ = sender.send(ChatStreamEvent::Tool(tool));
            }
            std::thread::sleep(slow(Duration::from_millis(think)));
            if let Some(tool) = tool("completed") {
                let _ = sender.send(ChatStreamEvent::Tool(tool));
            }
            let answer = "Here are your recent action items, organized by meeting:\n\n\
                ## Thursday, Nov 12\n\n### Weekly team standup\n\n\
                - Jules: complete the search review\n\
                - Sam: update the calendar empty state\n\n\
                ### Product & design sync\n\n\
                - Maya: share the updated prototype by Friday\n\
                - Theo: arrange five usability sessions\n\n\
                ## Wednesday, Nov 11\n\n### Customer discovery · Northstar\n\n\
                - Shared project folders would make notes easier to find\n";
            for word in answer.split_inclusive(' ') {
                if sender.send(ChatStreamEvent::Delta(word.into())).is_err() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(8));
            }
            let _ = sender.send(ChatStreamEvent::Done);
        });
        receiver
    }
    fn seed_threads(view: &mut Cereal) {
        let now = chrono::Utc::now();
        let titles = [
            ("Meeting action items extractor", 0),
            ("Untitled thread", 6 * 60),
            ("Untitled conversation", 6 * 60 + 2),
            ("Untitled thread", 6 * 60 + 4),
            ("New conversation", 6 * 60 + 6),
            ("Zach & Agora YC recap", 13 * 60),
            ("Prototype codename ideas", 26 * 60),
            ("Cereal research thread", 50 * 60),
        ];
        view.chat_archives.clear();
        // Nineteen older ones, then the eight above, newest last.
        for index in 0..19 {
            view.chat_archives.push(thread(
                &format!("older-{index}"),
                "Older conversation",
                now - chrono::Duration::days(3 + index),
            ));
        }
        for (index, (title, minutes)) in titles.iter().enumerate().rev() {
            view.chat_archives.push(thread(
                &format!("recent-{index}"),
                title,
                now - chrono::Duration::minutes(*minutes),
            ));
        }
    }
    fn thread(id: &str, title: &str, at: chrono::DateTime<chrono::Utc>) -> crate::chat::ArchivedChat {
        crate::chat::ArchivedChat {
            id: id.into(),
            note_id: 0,
            account: None,
            title: title.into(),
            messages: vec![(title.into(), "Here is what I found in your notes.".into())],
            interrupted: vec![],
            annotations: Default::default(),
            updated_at: Some(at.to_rfc3339()),
        }
    }
    vec![
        windowed("p-ask-home", 900, |view, window, cx| {
            view.navigate(Screen::Home, cx);
            view.clear_chat(cx);
            view.focus.focus(window);
        }),
        windowed("p-ask-focused", 900, |view, window, cx| {
            view.ask.read(cx).focus_handle.focus(window);
            cx.notify();
        }),
        windowed("p-ask-blurred", 900, |view, window, cx| {
            view.focus.focus(window);
            cx.notify();
        }),
        windowed("p-ask-typed-blurred", 900, |view, window, cx| {
            view.ask.update(cx, |input, cx| input.set_content("List recent todos", cx));
            view.focus.focus(window);
            cx.notify();
        }),
        windowed("p-ask-thinking", 900, |view, window, cx| {
            view.ask.read(cx).focus_handle.focus(window);
            view.start_scripted_chat("List recent todos".into(), script(20_000), cx);
        }),
        step("p-ask-wait-answer-quiet", 17_000, |_, _| {}),
        step("p-ask-answer", 900, |view, _| view.chat_scroll.set_offset(Point::default())),
        windowed("p-ask-collapsed", 900, |view, window, cx| {
            view.close_chat_surface(cx);
            view.focus.focus(window);
        }),
        windowed("p-ask-chat-page", 900, |view, window, cx| {
            seed_threads(view);
            view.clear_chat(cx);
            view.navigate(Screen::Chat, cx);
            view.focus.focus(window);
        }),
        windowed("p-ask-chat-page-focused", 900, |view, window, cx| {
            view.ask.read(cx).focus_handle.focus(window);
            cx.notify();
        }),
        step("p-ask-chat-threads-all", 900, |view, cx| {
            view.chat_threads_expanded = true;
            cx.notify();
        }),
        step("p-ask-chat-thread-open", 900, |view, cx| {
            view.chat_threads_expanded = false;
            if let Some(thread) = view.chat_threads().into_iter().find(|thread| thread.id == "recent-0") {
                view.open_chat_thread(&thread.account, &thread.id, true, cx);
            }
        }),
        step("p-ask-home-again", 900, |view, cx| view.navigate(Screen::Home, cx)),
    ]
}
#[cfg(not(debug_assertions))]
fn chat_parity_steps() -> Vec<Step> {
    Vec::new()
}

/// The thoughts above a chat answer, `p-think-…` (stills) and `m-think-…`
/// (held while they move, for a burst of captures): an answered turn with the
/// model's reasoning and its meeting lookups, closed, open, with one lookup's
/// meetings open; an answer being thought about, its reasoning arriving; the
/// model menu; and an answer's typography across notes and about one note.
/// Run with `CEREAL_TOUR=p-think,m-think` (add `--dark` for the dark theme).
/// Everything is seeded or scripted: no server is involved.
#[cfg(debug_assertions)]
fn thinking_steps() -> Vec<Step> {
    const SEARCH: &str = "61000000-0000-4000-8000-000000000001";
    const READ: &str = "61000000-0000-4000-8000-000000000002";
    const PLAN: &str = "61000000-0000-4000-8000-000000000003";
    fn day(back: i64) -> String {
        (chrono::Local::now().date_naive() - chrono::Duration::days(back)).format("%Y-%m-%d").to_string()
    }
    fn tool(call: &str, name: &str, status: &str, label: &str, meetings: &[(&str, i64)]) -> Option<meeting_notes_api_types::ChatToolProgressDTO> {
        let results: Vec<_> = meetings
            .iter()
            .enumerate()
            .map(|(index, (title, back))| {
                serde_json::json!({ "noteId": format!("62000000-0000-4000-8000-{index:012}"), "title": title, "date": day(*back) })
            })
            .collect();
        let mut value = serde_json::json!({ "callId": call, "name": name, "status": status, "label": label });
        if !results.is_empty() {
            value["resultCount"] = serde_json::json!(results.len());
            value["results"] = serde_json::json!(results);
        }
        serde_json::from_value(value).ok()
    }
    const FOUND: [(&str, i64); 6] = [
        ("Quilfeather outage review", 0),
        ("Brambling cash rollout", 0),
        ("Weekly team standup", 1),
        ("Product & design sync", 3),
        ("Customer discovery · Northstar", 3),
        ("Grayweather retrospective", 400),
    ];
    const ANSWER: &str = "You have two open follow-ups from this week, both from the outage review.\n\n\
        - **Alerting:** add an alert at 90 seconds of queue delay before the next rotation. [[S1]][[S2]]\n\
        - **Rollout:** the Brambling cash rollout stays paused until the certificate rotation is confirmed. [[S3]]\n\
        - **Owners:** Jules takes the alert; Maya confirms the rotation on Thursday. [[S57]][[S58]]\n\n\
        Nothing else was assigned to you in the last seven days.";
    fn follow_log(view: &Cereal) {
        let scroll = &view.chat_scroll;
        eprintln!(
            "follow: {:.0} px from the end of {:.0}, answer {} chars, streaming {}",
            f32::from(scroll.max_offset().height + scroll.offset().y),
            f32::from(scroll.max_offset().height),
            view.chat_messages.last().map_or(0, |(_, answer)| answer.len()),
            view.chat_stream.is_some()
        );
    }
    fn seed(view: &mut Cereal, reasoning: bool) {
        view.chat_messages.clear();
        view.chat_annotations.clear();
        view.chat_messages.push(("What do I still owe people this week?".into(), ANSWER.into()));
        let tools: Vec<_> = [
            tool(SEARCH, "search_meetings", "completed", "Found 6 meetings", &FOUND),
            tool(READ, "get_meetings", "completed", "Looked deeper into 2 notes", &FOUND[..2]),
            tool(PLAN, "search_transcripts", "failed", "Search planning unavailable; using direct meeting lookup", &[]),
        ]
        .into_iter()
        .flatten()
        .collect();
        let citations: Vec<_> = [1, 2, 3, 57, 58]
            .iter()
            .map(|number| {
                serde_json::json!({
                    "id": format!("S{number}"), "noteId": "419d65d1-7a2c-44ca-84cc-ad489522d561", "title": "Quilfeather outage review",
                    "source": "summary", "segmentId": null, "startMs": null,
                    "quote": "Add an alert at 90 seconds of queue delay.", "transcriptRevision": 7
                })
            })
            .collect();
        let thoughts = if reasoning {
            serde_json::json!([
                { "afterTools": 0, "text": "**Searching for this week's commitments**\n\nI need the meetings from the last seven days and what each one left open. I'll search for follow-ups and action items first, then read the notes of the meetings that look relevant, so the list is complete rather than a sample." },
                { "afterTools": 1, "text": "**Reading the two that matter**\n\nThe outage review and the rollout meeting both mention open work. The others are status updates with nothing assigned, so I'll read just these two." },
                { "afterTools": 3, "text": "**Checking owners**\n\nThe notes name an owner for each item. I'll keep the user's own items first and say who has the rest." },
            ])
        } else {
            serde_json::json!([])
        };
        if let Ok(notes) = serde_json::from_value(serde_json::json!({ "tools": tools, "reasoning": thoughts, "citations": citations, "elapsed_ms": 5200 })) {
            view.chat_annotations.insert(0, notes);
        }
        view.assistant_open = true;
    }
    fn live(slow_start: u64) -> std::sync::mpsc::Receiver<crate::server::ChatStreamEvent> {
        use crate::server::ChatStreamEvent;
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let pause = |ms: u64| std::thread::sleep(slow(Duration::from_millis(ms)));
            let think = |text: &str| {
                for word in text.split_inclusive(' ') {
                    if sender.send(ChatStreamEvent::Reasoning(word.into())).is_err() {
                        return;
                    }
                    std::thread::sleep(slow(Duration::from_millis(45)));
                }
            };
            pause(slow_start);
            think("**Searching for this week's commitments**\n\nI need the meetings from the last seven days and what each one left open. I'll search for follow-ups first, then read the notes that look relevant.");
            if let Some(tool) = tool(SEARCH, "search_meetings", "running", "Searching meetings", &[]) {
                let _ = sender.send(ChatStreamEvent::Tool(tool));
            }
            pause(1600);
            if let Some(tool) = tool(SEARCH, "search_meetings", "completed", "Found 6 meetings", &FOUND) {
                let _ = sender.send(ChatStreamEvent::Tool(tool));
            }
            think("**Reading the two that matter**\n\nThe outage review and the rollout meeting both mention open work, so I'll read just these two.");
            if let Some(tool) = tool(READ, "get_meetings", "running", "Reading meeting notes", &[]) {
                let _ = sender.send(ChatStreamEvent::Tool(tool));
            }
            pause(1400);
            if let Some(tool) = tool(READ, "get_meetings", "completed", "Looked deeper into 2 notes", &FOUND[..2]) {
                let _ = sender.send(ChatStreamEvent::Tool(tool));
            }
            pause(500);
            for word in ANSWER.replace("[[S57]][[S58]]", "").split_inclusive(' ') {
                if sender.send(ChatStreamEvent::Delta(word.into())).is_err() {
                    return;
                }
                std::thread::sleep(slow(Duration::from_millis(18)));
            }
            let _ = sender.send(ChatStreamEvent::Done);
        });
        receiver
    }
    fn models(view: &mut Cereal) {
        view.chat_models = serde_json::from_value(serde_json::json!([
            { "provider": "openai", "model": "gpt-6-luna" },
            { "provider": "openrouter", "model": "anthropic/claude-sonnet-5-5" },
            { "provider": "openrouter", "model": "google/gemini-4-carbon" },
            { "provider": "openrouter", "model": "x-ai/grok-4.7" },
            { "provider": "openai", "model": "gpt-6.1-sol" },
            { "provider": "openai_compatible", "model": "llama3.2:3b" },
        ]))
        .unwrap_or_default();
    }
    let notes = |view: &mut Cereal, change: fn(&mut crate::chat::ChatTurnAnnotations)| {
        if let Some(notes) = view.chat_annotations.get_mut(&0) {
            change(notes);
        }
    };
    vec![
        windowed("p-think-collapsed", 900, move |view, window, cx| {
            view.navigate(Screen::Home, cx);
            view.clear_chat(cx);
            models(view);
            seed(view, true);
            view.focus.focus(window);
        }),
        step("m-think-opening", 1400, move |view, _| notes(view, |notes| notes.set_thoughts_open(Some(true)))),
        step("p-think-open", 900, |_, _| {}),
        step("m-think-step-opening", 1400, move |view, _| notes(view, |notes| notes.toggle_step(SEARCH))),
        step("p-think-step-open", 900, |_, _| {}),
        step("m-think-step-closing", 1400, move |view, _| notes(view, |notes| notes.toggle_step(SEARCH))),
        step("m-think-closing", 1400, move |view, _| notes(view, |notes| notes.set_thoughts_open(Some(false)))),
        // A provider that gave no reasoning: the lookups alone.
        step("p-think-steps-only", 900, |view, _| {
            seed(view, false);
            if let Some(notes) = view.chat_annotations.get_mut(&0) {
                notes.set_thoughts_open(Some(true));
            }
        }),
        // Thought about live: waiting, reasoning arriving, a lookup under
        // way, then the answer starting and the thoughts folding away.
        windowed("m-think-live", 16_000, |view, window, cx| {
            view.clear_chat(cx);
            view.ask.read(cx).focus_handle.focus(window);
            view.start_scripted_chat("What do I still owe people this week?".into(), live(1800), cx);
        }),
        step("p-think-live-done", 900, |view, _| view.chat_scroll.set_offset(Point::default())),
        // The model button and its menu.
        step("p-think-model-button", 900, |view, _| seed(view, true)),
        step("p-think-model-menu", 900, |view, cx| {
            view.toggle_chat_model_picker(cx);
        }),
        input("p-think-model-menu-down", 700, |window, cx| {
            press_key(window, cx, "down");
            press_key(window, cx, "down");
        }),
        input("p-think-model-menu-chosen", 900, |window, cx| press_key(window, cx, "enter")),
        // The same answer about one note: no source marks.
        step("p-think-note-chat", 900, |view, cx| {
            view.close_chat_surface(cx);
            view.open_note(1, cx);
            seed(view, true);
        }),
        // A note's chat over a running recording: the record capsule goes
        // into the pill as the panel opens and comes back out as it folds.
        // `CEREAL_TOUR=p-think-absorb,k-absorb`; the `k-` steps are held
        // for a burst of captures while they move.
        step("p-think-absorb-1-bar", 1200, |view, cx| {
            view.close_chat_surface(cx);
            view.open_note(1, cx);
            view.start_recording_preview(1, cx);
        }),
        step("k-absorb-open", 2400, |view, _| seed(view, false)),
        step("p-think-absorb-2-open", 600, |_, _| {}),
        step("k-absorb-close", 2400, |view, cx| view.close_chat_surface(cx)),
        step("p-think-absorb-3-closed", 600, |_, _| {}),
        // Following the end of the chat: a long answer arrives in the note's
        // panel; a reader at the end is kept there, one who scrolls up is
        // left alone, and scrolling back down picks the answer up again.
        // `CEREAL_TOUR=k-follow` logs how far the view is from the end.
        step("k-follow-start", 900, |view, cx| {
            view.close_chat_surface(cx);
            view.open_note(1, cx);
            seed(view, false);
            let (sender, receiver) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                for word in "Here is more detail on each open item, with who owns it and when it is due. "
                    .repeat(40)
                    .split_inclusive(' ')
                {
                    if sender.send(crate::server::ChatStreamEvent::Delta(word.into())).is_err() {
                        return;
                    }
                    std::thread::sleep(slow(Duration::from_millis(8)));
                }
                let _ = sender.send(crate::server::ChatStreamEvent::Done);
            });
            view.start_scripted_chat("Can you provide more details".into(), receiver, cx);
        }),
        windowed("k-follow-tick-00", 250, |view, _, _| follow_log(view)),
        windowed("k-follow-tick-01", 250, |view, _, _| follow_log(view)),
        windowed("k-follow-tick-02", 250, |view, _, _| follow_log(view)),
        windowed("k-follow-tick-03", 250, |view, _, _| follow_log(view)),
        windowed("k-follow-tick-04", 250, |view, _, _| follow_log(view)),
        input("k-follow-wheel-up", 250, |window, cx| scroll(window, cx, 500., 420., 300.)),
        windowed("k-follow-tick-05", 250, |view, _, _| follow_log(view)),
        windowed("k-follow-tick-06", 250, |view, _, _| follow_log(view)),
        windowed("k-follow-tick-07", 250, |view, _, _| follow_log(view)),
        windowed("k-follow-tick-08", 250, |view, _, _| follow_log(view)),
        windowed("k-follow-tick-09", 250, |view, _, _| follow_log(view)),
        input("k-follow-wheel-down", 250, |window, cx| scroll(window, cx, 500., 420., -100000.)),
        windowed("k-follow-tick-10", 250, |view, _, _| follow_log(view)),
        windowed("k-follow-tick-11", 250, |view, _, _| follow_log(view)),
        windowed("k-follow-tick-12", 250, |view, _, _| follow_log(view)),
        windowed("k-follow-tick-13", 250, |view, _, _| follow_log(view)),
        windowed("k-follow-tick-14", 250, |view, _, _| follow_log(view)),
        windowed("k-follow-tick-15", 250, |view, _, _| follow_log(view)),
        // The sidebar's Chat opens a new conversation's splash; the one that
        // was open is kept in Recents. `CEREAL_TOUR=p-chatnav`.
        step("p-chatnav-1-conversation", 900, |view, cx| {
            view.close_chat_surface(cx);
            view.navigate(Screen::Chat, cx);
            seed(view, false);
            view.assistant_open = false;
        }),
        input("p-chatnav-2-pressed", 900, |window, cx| click(window, cx, 60., 128.)),
        // The send button with something to send, in a note's chat and on
        // the Chat page. `CEREAL_TOUR=p-send`.
        step("p-send-1-note", 900, |view, cx| {
            view.close_chat_surface(cx);
            view.open_note(1, cx);
            seed(view, false);
            view.ask.update(cx, |input, cx| input.set_content("What", cx));
        }),
        step("p-send-2-page", 900, |view, cx| {
            view.close_chat_surface(cx);
            view.navigate(Screen::Chat, cx);
            view.clear_chat(cx);
            view.ask.update(cx, |input, cx| input.set_content("What", cx));
        }),
        // A narrow window: no sidebar beside the page; the sidebar button
        // slides it over the dimmed page. `CEREAL_TOUR=p-narrow` with
        // `CEREAL_TOUR_SIZE=800x900`.
        step("p-narrow-1-home", 900, |view, cx| {
            view.close_chat_surface(cx);
            view.navigate(Screen::Home, cx);
        }),
        step("p-narrow-2-overlay", 900, |view, _| view.toggle_sidebar()),
        input("p-narrow-3-pressed-page", 900, |window, cx| click(window, cx, 700., 500.)),
        // A note's transcript panel. `CEREAL_TOUR=p-panel`.
        step("p-panel-transcript", 1500, |view, cx| {
            view.close_chat_surface(cx);
            view.open_note(1, cx);
            view.transcript_open = true;
            view.transcript_epoch = view.transcript_epoch.wrapping_add(1);
        }),
        // The recording's pill in the header while another page is open.
        // `CEREAL_TOUR=p-session`.
        step("p-session-start", 600, |view, cx| {
            view.close_chat_surface(cx);
            view.open_note(1, cx);
            view.start_recording_preview(1, cx);
        }),
        step("p-session-pill", 1500, |view, cx| view.navigate(Screen::Home, cx)),
        step("p-think-end", 300, |view, cx| {
            view.close_chat_surface(cx);
            view.clear_chat(cx);
            view.chat_models.clear();
            view.navigate(Screen::Home, cx);
        }),
    ]
}
#[cfg(not(debug_assertions))]
fn thinking_steps() -> Vec<Step> {
    Vec::new()
}

/// Settings › Workspace against a real workspace, `a-org-…` (debug builds;
/// run with `CEREAL_AUDIT=1 CEREAL_TOUR=a-org`, signed in through
/// `CEREAL_SERVER_URL` and `CEREAL_SERVER_TOKEN` as the owner of a throwaway
/// server on this Mac whose workspace has an admin and then a member, in the
/// order they joined). It makes the member an admin, removes them, invites
/// them back beside an address the server has no account for, cancels the
/// invitation and renames the workspace, all through the page's own menus
/// and dialogs. `Cereal::review_organization_live` refuses any other server.
fn org_e2e_steps() -> Vec<Step> {
    // The "…" of the row whose middle is `y` points down the window.
    fn more(y: f32) -> impl Fn(&mut Window, &mut App) + 'static {
        move |window, cx| {
            let x = f32::from(window.viewport_size().width) - 44.;
            click(window, cx, x, y);
        }
    }
    fn live(act: &'static str) -> impl Fn(&mut Cereal, &mut Window, &mut Context<Cereal>) + 'static {
        move |view, window, cx| view.review_organization_live(act, window, cx)
    }
    // The third member's row, and the first row under the first band.
    const THIRD: f32 = 321.;
    const FIRST: f32 = 233.;
    vec![
        windowed("a-org-open-quiet", 3000, live("members")),
        step("a-org-members", 300, |_, _| {}),
        input("a-org-role-menu-quiet", 900, more(THIRD)),
        input("a-org-role-quiet", 3000, |window, cx| {
            press_key(window, cx, "down");
            press_key(window, cx, "enter");
        }),
        step("a-org-role", 300, |_, _| {}),
        input("a-org-remove-menu-quiet", 900, more(THIRD)),
        input("a-org-remove-ask-quiet", 900, |window, cx| {
            press_key(window, cx, "down");
            press_key(window, cx, "down");
            press_key(window, cx, "enter");
        }),
        step("a-org-remove-ask", 300, |_, _| {}),
        windowed("a-org-remove-quiet", 3000, live("confirm")),
        step("a-org-removed", 300, |_, _| {}),
        windowed(
            "a-org-invite-quiet",
            4000,
            live("invite:mia@acme.example, stranger@nowhere.example"),
        ),
        step("a-org-invite", 300, |_, _| {}),
        windowed("a-org-invite-close-quiet", 900, live("close")),
        step("a-org-invited", 300, |_, _| {}),
        input("a-org-cancel-menu-quiet", 900, more(FIRST)),
        input("a-org-cancel-quiet", 3000, |window, cx| {
            press_key(window, cx, "down");
            press_key(window, cx, "enter");
        }),
        step("a-org-cancelled", 300, |_, _| {}),
        windowed("a-org-general-quiet", 2000, live("general")),
        windowed("a-org-rename-quiet", 3000, live("rename:Acme HQ")),
        step("a-org-renamed", 300, |_, _| {}),
    ]
}

/// Press the "…" of a row of Settings › Members: at `CEREAL_TOUR_ORG_MORE`
/// (`x,y` in window points), else where the third member's is in a window
/// of the default size.
fn org_more(window: &mut Window, cx: &mut App) {
    let (x, y) = std::env::var("CEREAL_TOUR_ORG_MORE")
        .ok()
        .and_then(|at| {
            let (x, y) = at.split_once(',')?;
            Some((x.parse().ok()?, y.parse().ok()?))
        })
        .unwrap_or_else(|| (f32::from(window.viewport_size().width) - 44., 400.));
    click(window, cx, x, y);
}

/// Surfaces reached from somewhere other than their own button, `p-fix-…`:
/// the recent chats from Home's focused composer, Trash from another page
/// and from the command palette, and the line generated notes end with.
/// Stills, run only when asked for (`CEREAL_TOUR=p-fix`).
fn surface_steps() -> Vec<Step> {
    vec![
        windowed("p-fix-ask-focused", 900, |view, window, cx| {
            view.navigate(Screen::Home, cx);
            view.clear_chat(cx);
            view.ask.read(cx).focus_handle.focus(window);
            cx.notify();
        }),
        // The recent-chats button in the tray, 28 pt in and 20 pt down.
        input("p-fix-ask-history", 1200, |window, cx| {
            let size = window.viewport_size();
            let centre = 192. + (f32::from(size.width) - 192.) / 2.;
            click(window, cx, centre - 341.5 + 28., f32::from(size.height) - 124. + 20.);
        }),
        windowed("p-fix-trash", 1200, |view, window, cx| {
            view.close_chat_surface(cx);
            view.focus.focus(window);
            for id in [2, 3] {
                if let Some(note) = view.workspace.note_mut(id) {
                    note.trashed = true;
                }
            }
            view.navigate(Screen::Home, cx);
            view.open_trash(cx);
        }),
        step("p-fix-trash-closed", 600, |view, cx| {
            view.menu = None;
            view.open_settings(crate::screens::SettingsTab::Preferences, cx);
        }),
        // Settings › Appearance: the theme row and the Dock icons, in both
        // themes, then the theme menu open (`CEREAL_TOUR=p-appearance`).
        step("p-appearance-light", 900, |view, cx| {
            view.menu = None;
            view.workspace.settings.force_dark(false);
            view.open_settings(crate::screens::SettingsTab::Preferences, cx);
        }),
        step("p-appearance-dark", 900, |view, _| view.workspace.settings.force_dark(true)),
        step("p-appearance-dark-menu", 900, |view, _| view.menu = Some(Menu::Theme)),
        step("p-appearance-system-menu", 900, |view, _| {
            view.workspace.settings.set_theme(crate::model::ThemeMode::System)
        }),
        step("p-appearance-light-menu", 900, |view, _| {
            view.workspace.settings.force_dark(false)
        }),
        step("p-appearance-end", 300, |view, _| view.menu = None),
        // An update found by the quiet check at launch
        // (`CEREAL_TOUR=p-update`): its notice over Home, then, after
        // "Later", the account menu's row and the dot on its button, and
        // Settings › About, in both themes.
        step("p-update-prompt-light", 900, |view, cx| {
            view.menu = None;
            view.workspace.settings.force_dark(false);
            view.navigate(Screen::Home, cx);
            view.updates.review("found");
        }),
        step("p-update-menu-light", 900, |view, _| {
            view.updates.review("later");
            view.menu = Some(Menu::Workspace);
        }),
        step("p-update-about-light", 900, |view, cx| {
            view.menu = None;
            view.open_settings(crate::screens::SettingsTab::About, cx);
        }),
        step("p-update-prompt-dark", 900, |view, cx| {
            view.workspace.settings.force_dark(true);
            view.navigate(Screen::Home, cx);
            view.updates.review("found");
        }),
        step("p-update-menu-dark", 900, |view, _| {
            view.updates.review("later");
            view.menu = Some(Menu::Workspace);
        }),
        step("p-update-about-dark", 900, |view, cx| {
            view.menu = None;
            view.open_settings(crate::screens::SettingsTab::About, cx);
        }),
        // The same two places while the update downloads: the menu's dot
        // and the button in Settings fill with it.
        step("p-update-busy-menu-light", 900, |view, cx| {
            view.workspace.settings.force_dark(false);
            view.navigate(Screen::Home, cx);
            view.updates.review("downloading");
            view.menu = Some(Menu::Workspace);
        }),
        step("p-update-busy-about-light", 900, |view, cx| {
            view.menu = None;
            view.open_settings(crate::screens::SettingsTab::About, cx);
        }),
        step("p-update-busy-menu-dark", 900, |view, cx| {
            view.workspace.settings.force_dark(true);
            view.navigate(Screen::Home, cx);
            view.menu = Some(Menu::Workspace);
        }),
        step("p-update-busy-about-dark", 900, |view, cx| {
            view.menu = None;
            view.open_settings(crate::screens::SettingsTab::About, cx);
        }),
        step("p-update-end", 300, |view, cx| {
            view.updates.review("idle");
            view.workspace.settings.force_dark(false);
            view.navigate(Screen::Home, cx);
        }),
        // From a page without the sidebar's footer: Home, with the popover.
        step("p-fix-trash-from-settings", 1200, |view, cx| view.open_trash(cx)),
        // A link to a note that is in the Trash, followed from the note
        // that is open: the Trash with its row, not the note.
        step("p-fix-trash-link", 1200, |view, cx| {
            view.menu = None;
            view.open_note(1, cx);
            view.open_linked_note(3, cx);
        }),
        windowed("p-fix-palette-trash", 1200, |view, window, cx| {
            view.menu = None;
            view.open_search(window, cx);
            view.search.update(cx, |input, cx| input.set_content("trash", cx));
        }),
        input("p-fix-palette-trash-down", 600, |window, cx| press_key(window, cx, "down")),
        input("p-fix-palette-trash-return", 1200, |window, cx| press_key(window, cx, "enter")),
        // The team space (`CEREAL_TOUR=p-fix-team`): its notes, its people,
        // the banner closed, the "Add notes" sheet, and an empty space. The
        // people and notes are made up (`team_space::tour_page`).
        step("p-fix-team-notes", 1200, |view, cx| {
            view.menu = None;
            view.workspace.settings.team_banner_dismissed = false;
            view.team.page = Some(crate::team_space::tour_page(false));
            view.team.tab = crate::team_space::Tab::Notes;
            view.team.picker = None;
            view.navigate(Screen::TeamSpace, cx);
        }),
        step("p-fix-team-people", 1200, |view, _| {
            view.team.tab = crate::team_space::Tab::People;
            view.team.scroll.set_offset(point(px(0.), px(-330.)));
        }),
        step("p-fix-team-banner-dismissed", 1500, |view, cx| {
            view.team.tab = crate::team_space::Tab::Notes;
            view.team.scroll.set_offset(Point::default());
            view.dismiss_team_banner(cx);
        }),
        step("p-fix-team-picker", 1200, |view, cx| {
            view.open_team_picker(cx);
            if let Some(picker) = &mut view.team.picker {
                picker.selected = vec![1, 3];
            }
        }),
        step("p-fix-team-empty", 1200, |view, _| {
            view.team.picker = None;
            view.team.page = Some(crate::team_space::tour_page(true));
        }),
        step("p-fix-team-end", 300, |view, cx| {
            view.team.page = None;
            view.navigate(Screen::Home, cx);
        }),
        // Settings › Workspace (`CEREAL_TOUR=p-fix-org`): Members as its
        // owner sees it, a row's menu, the question before removing someone,
        // the invite dialog, a search, then as an admin and a member see it,
        // loading and failing, and General; then the same in the dark theme.
        // The people are made up (`workspace_settings::tour_details`).
        // `CEREAL_TOUR_ORG_MORE=x,y` is where the "…" of a row is pressed.
        step("p-fix-org-owner", 1200, |view, cx| {
            view.menu = None;
            view.workspace.settings.force_dark(false);
            view.review_organization("owner", cx);
            view.open_settings(crate::screens::SettingsTab::Members, cx);
        }),
        input("p-fix-org-menu", 1200, org_more),
        step("p-fix-org-remove", 1200, |view, _| {
            view.org.removing = Some("00000000-0000-4000-8000-000000000103".into());
            view.menu = Some(Menu::RemoveMember);
        }),
        windowed("p-fix-org-invite", 1200, |view, window, cx| {
            view.menu = None;
            view.open_invite(window, cx);
        }),
        step("p-fix-org-invite-error", 1200, |view, cx| {
            view.org.invite_role = meeting_notes_api_types::InviteOrganizationMemberInputRole::Admin;
            view.org.invite.update(cx, |input, cx| input.set_content("lee@acme.example, new@elsewhere.example", cx));
            view.org.invite_problems = vec![
                "new@elsewhere.example is not allowed to sign in to this server. Ask one of its administrators to invite them."
                    .into(),
            ];
        }),
        windowed("p-fix-org-search", 1200, |view, window, cx| {
            view.menu = None;
            view.org.invite.update(cx, |input, cx| input.set_content("", cx));
            view.org.search.update(cx, |input, cx| input.set_content("sa", cx));
            view.org.search.read(cx).focus_handle.focus(window);
        }),
        step("p-fix-org-search-empty", 1200, |view, cx| {
            view.org.search.update(cx, |input, cx| input.set_content("zebra", cx));
        }),
        windowed("p-fix-org-admin", 1200, |view, window, cx| {
            view.focus.focus(window);
            view.review_organization("admin", cx);
        }),
        step("p-fix-org-member", 1200, |view, cx| view.review_organization("member", cx)),
        step("p-fix-org-loading", 1200, |view, cx| view.review_organization("loading", cx)),
        step("p-fix-org-failed", 1200, |view, cx| view.review_organization("failed", cx)),
        step("p-fix-org-general", 1200, |view, cx| {
            view.review_organization("owner", cx);
            view.open_settings(crate::screens::SettingsTab::WorkspaceGeneral, cx);
        }),
        step("p-fix-org-general-member", 1200, |view, cx| view.review_organization("member", cx)),
        step("p-fix-org-dark-general", 1200, |view, cx| {
            view.workspace.settings.force_dark(true);
            view.review_organization("owner", cx);
        }),
        step("p-fix-org-dark-owner", 1200, |view, cx| {
            view.open_settings(crate::screens::SettingsTab::Members, cx);
        }),
        input("p-fix-org-dark-menu", 1200, org_more),
        windowed("p-fix-org-dark-invite", 1200, |view, window, cx| {
            view.menu = None;
            view.open_invite(window, cx);
        }),
        step("p-fix-org-dark-member", 1200, |view, cx| {
            view.menu = None;
            view.review_organization("member", cx);
        }),
        step("p-fix-org-end", 300, |view, cx| {
            view.menu = None;
            view.workspace.settings.force_dark(false);
            view.review_organization("off", cx);
            view.navigate(Screen::Home, cx);
        }),
        // The Share popover (`CEREAL_TOUR=p-fix-share`), from made-up people:
        // at rest, the picker's suggestions, results for a query, after
        // sharing with the attendees, and the link's reach.
        step("p-fix-share", 1200, |view, cx| {
            view.menu = None;
            view.open_note(1, cx);
            let (access, workspace) = crate::share_popover::tour_fixture();
            view.share.preview(access, workspace);
            view.menu = Some(Menu::Share);
        }),
        step("p-fix-share-suggested", 900, |view, cx| view.share_open_picker(cx)),
        step("p-fix-share-results", 900, |view, cx| {
            view.share.field.update(cx, |input, cx| input.set_content("ma", cx));
            view.share_query_changed(cx);
        }),
        input("p-fix-share-results-down", 600, |window, cx| press_key(window, cx, "down")),
        input("p-fix-share-added", 900, |window, cx| press_key(window, cx, "enter")),
        step("p-fix-share-shared", 900, |view, cx| view.share_with_attendees(cx)),
        step("p-fix-share-level", 900, |view, cx| {
            view.share.level_menu = true;
            cx.notify();
        }),
        input("p-fix-share-level-chosen", 900, |window, cx| {
            press_key(window, cx, "down");
            press_key(window, cx, "enter");
        }),
        step("p-fix-share-outside", 900, |view, cx| {
            view.share.field.update(cx, |input, cx| input.set_content("kai@elsewhere.test", cx));
            view.share_query_changed(cx);
        }),
        input("p-fix-share-refused", 900, |window, cx| press_key(window, cx, "enter")),
        step("p-fix-share-end", 300, |view, _| view.menu = None),
        step("p-fix-transcript-link", 1200, |view, cx| {
            view.menu = None;
            for id in [2, 3] {
                if let Some(note) = view.workspace.note_mut(id) {
                    note.trashed = false;
                }
            }
            if let Some(note) = view.workspace.note_mut(1) {
                note.enhanced = "No usable meeting content captured. The transcript contains only a microphone test and no substantive discussion.\n\n---\n\nChat with meeting transcript: https://notes.example.com/t/51207249-97a9-4bd7-b02f-93428ffb367a".into();
            }
            view.open_note(1, cx);
            view.active_tab = NoteTab::Enhanced;
        }),
    ]
}

/// The note page's source card, the ink of edited blocks and the details
/// card under the date chip, `p-polish-…` (run with `CEREAL_TOUR=p-polish`,
/// with `--dark` for the dark theme): the source card waiting and with its
/// summary; a heading edited and a bullet typed into, undone and redone
/// (each says on stderr which rows are the person's); the details card, and
/// the same while a name is typed into "Add attendees".
fn note_polish_steps() -> Vec<Step> {
    const NOTES: &str = "# Interface comparison\n\n- Benchmarking the reference interface against ours across several interaction patterns\n- Overall: its polish is noticeably ahead in animation, pill behavior, and find UX\n\n# Pill and animation issues\n\n- Click-outside behavior: theirs closes the pill; ours stays open\n  - Fix dismiss-on-outside-click\n- Open/close animation needs a full rework\n  - Their pills merge into each other as they open and close\n    - Ours looks visually rough, especially at the edges\n- Buffering animation: theirs is better\n\n# Next steps\n\n- Improve the pill edges\n- Fix the find control's alignment";
    const ROW: usize = 9;
    fn written(view: &Cereal) -> String {
        let Some(note) = view.workspace.note(1) else {
            return "no note".into();
        };
        let mut rows: Vec<usize> = note
            .enhanced_authorship
            .as_ref()
            .map(|authorship| authorship.written(&note.enhanced))
            .unwrap_or_default()
            .into_iter()
            .collect();
        rows.sort();
        format!(
            "rows written by the person {rows:?}, line 3 {:?}, editing {}",
            note.enhanced.lines().nth(3).unwrap_or(""),
            view.summary.edit.is_some()
        )
    }
    vec![
        step("p-polish-note", 900, |view, cx| {
            view.menu = None;
            if let Some(note) = view.workspace.note_mut(1) {
                note.enhanced = NOTES.into();
                note.enhanced_authorship = None;
                note.generated_revision = Some(note.transcript_revision);
            }
            view.open_note(1, cx);
            view.active_tab = NoteTab::Enhanced;
        }),
        // Waiting: the request this would have made is forgotten, so the
        // card stays as it is while it waits.
        step("p-polish-summary-loading", 1200, |view, cx| {
            let paragraph = crate::summary::paragraph_text(NOTES.lines().nth(ROW).unwrap_or(""));
            let trigger = crate::summary::probe::trigger(1, ROW)
                .unwrap_or_else(|| Bounds::new(point(px(880.), px(330.)), size(px(24.), px(24.))));
            view.open_summary(1, ROW, paragraph, trigger, cx);
            view.summary.epoch = view.summary.epoch.wrapping_add(1);
        }),
        step("p-polish-summary-loaded", 1200, |view, _| {
            let quote = |text: &str, time: &str| crate::summary::TranscriptQuote {
                segment_id: None,
                text: text.into(),
                speaker: Some("Me".into()),
                timestamp: Some(time.into()),
            };
            if let Some(open) = view.summary.open.as_mut() {
                open.explanation = "This came up while comparing the two interfaces directly, especially around how the control opens, closes, and dismisses. The broader point was that the other interaction feels much more polished and cohesive.".into();
                open.quotes = vec![
                    quote("Their opening and closing pill is way better.", "01:19"),
                    quote("Like, the pills merge into each other whenever they open and close.", "01:19"),
                    quote("So the open-close thing needs to be way better.", "01:19"),
                ];
                open.ready = true;
            }
            view.stage.mark("source-quotes");
        }),
        step("p-polish-summary-end", 500, |view, _| view.summary.dismiss()),
        // A heading changed earlier, and a bullet being typed into.
        windowed("p-polish-edit-typed", 900, |view, window, cx| {
            if let Some(note) = view.workspace.note_mut(1) {
                let edited = note.enhanced.replacen("# Interface comparison", "# Interface comparisonfff", 1);
                crate::authorship::edited(&mut note.enhanced_authorship, &note.enhanced, &edited);
                note.enhanced = edited;
            }
            view.begin_enhanced_edit(1, 3, window, cx);
            if let Some(edit) = view.summary.edit.as_ref() {
                let input = edit.input.clone();
                input.update(cx, |input, cx| input.replace_text_in_range(None, "fff", window, cx));
            }
            cx.notify();
        }),
        step("p-polish-edit-typed-quiet", 100, |view, _| eprintln!("polish typed: {}", written(view))),
        windowed("p-polish-edit-focus-quiet", 100, |view, window, cx| {
            let focused = view
                .summary
                .edit
                .as_ref()
                .is_some_and(|edit| edit.input.read(cx).focus_handle.is_focused(window));
            eprintln!("polish focus in the block: {focused}, window active: {}", window.is_window_active());
        }),
        input("p-polish-edit-undo", 900, |window, cx| press_key(window, cx, "cmd-z")),
        step("p-polish-edit-undo-quiet", 100, |view, _| eprintln!("polish undone: {}", written(view))),
        input("p-polish-edit-redo", 900, |window, cx| press_key(window, cx, "shift-cmd-z")),
        step("p-polish-edit-redo-quiet", 100, |view, _| eprintln!("polish redone: {}", written(view))),
        step("p-polish-edited", 900, |view, cx| view.finish_enhanced_edit(cx)),
        step("p-polish-details", 900, |view, cx| {
            view.team.page = Some(crate::team_space::tour_page(false));
            view.attendee_input.update(cx, |input, cx| input.set_content("", cx));
            view.menu = Some(Menu::NoteDetails);
        }),
        // A name being typed: who is listed narrows to it, and the people
        // the team space knows who match are offered.
        step("p-polish-details-typing", 900, |view, cx| {
            view.attendee_input.update(cx, |input, cx| input.set_content("or", cx));
        }),
        step("p-polish-end", 300, |view, cx| {
            view.attendee_input.update(cx, |input, cx| input.set_content("", cx));
            view.team.page = None;
            view.menu = None;
        }),
    ]
}

/// Home's Ask bar under stress, `k-ask-…` (run with `CEREAL_TOUR=k-ask`):
/// the focus transition there and back, then everything that can interrupt
/// it: a press mid-flight, ten quick focus changes, typing while it opens, a
/// tall draft, the window and the sidebar changing under it, the recent
/// chats from the tray, a question handed to the panel, presses outside
/// with a popover open, while an answer streams and on a note, and the dark
/// theme. After each part a `k-ask-…-state` step prints what is focused and
/// open (`askstate …`), so a stuck track or a bar left open shows in the log.
#[cfg(debug_assertions)]
fn ask_bar_steps() -> Vec<Step> {
    fn script(think: u64) -> std::sync::mpsc::Receiver<crate::server::ChatStreamEvent> {
        use crate::server::ChatStreamEvent;
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            std::thread::sleep(slow(Duration::from_millis(think)));
            for word in "Here is what I found in your notes: the launch moved to Thursday, \
                Maya owns the prototype and Theo is arranging five usability sessions. "
                .repeat(3)
                .split_inclusive(' ')
            {
                if sender.send(ChatStreamEvent::Delta(word.into())).is_err() {
                    return;
                }
                std::thread::sleep(slow(Duration::from_millis(12)));
            }
            let _ = sender.send(ChatStreamEvent::Done);
        });
        receiver
    }
    // The pill's text, a point of the page that holds nothing, and the
    // recent-chats button in the tray.
    fn pill(window: &Window) -> (f32, f32) {
        let size = window.viewport_size();
        (192. + (f32::from(size.width) - 192.) / 2. - 120., f32::from(size.height) - 56.)
    }
    fn press_pill(name: &'static str, hold: u64) -> Step {
        input(name, hold, |window, cx| {
            let (x, y) = pill(window);
            click(window, cx, x, y);
        })
    }
    fn press_outside(name: &'static str, hold: u64) -> Step {
        input(name, hold, |window, cx| {
            let size = window.viewport_size();
            click(window, cx, f32::from(size.width) - 30., f32::from(size.height) * 0.45);
        })
    }
    fn state(name: &'static str) -> Step {
        windowed(name, 30, move |view, window, cx| {
            eprintln!(
                "askstate {name} focused={} assistant={} history={} picker={} draft={} stream={} messages={} note={} tray={:?} ring={:?} controls={:?} panel={:?}",
                view.ask.read(cx).focus_handle.is_focused(window),
                view.assistant_open,
                view.chat_history_open,
                view.chat_model_picker_open,
                view.ask.read(cx).content().len(),
                view.chat_stream.is_some(),
                view.chat_messages.len(),
                view.current_note.is_some(),
                view.stage.target(("ask-tray", 0)),
                view.stage.target(("chat-composer-ring", 0)),
                view.stage.target(("chat-composer-controls", 0)),
                view.stage.target(("assistant", 0)),
            );
        })
    }
    fn focus_ask(name: &'static str, hold: u64) -> Step {
        windowed(name, hold, |view, window, cx| {
            view.ask.read(cx).focus_handle.focus(window);
            cx.notify();
        })
    }
    fn focus_page(name: &'static str, hold: u64) -> Step {
        windowed(name, hold, |view, window, cx| {
            view.focus.focus(window);
            cx.notify();
        })
    }
    vec![
        // a: there and back.
        windowed("k-ask-a1-rest", 900, |view, window, cx| {
            view.navigate(Screen::Home, cx);
            view.clear_chat(cx);
            view.focus.focus(window);
        }),
        press_pill("k-ask-a2-open", 1000),
        state("k-ask-a2-state"),
        press_outside("k-ask-a3-outside", 1000),
        state("k-ask-a3-state"),
        focus_page("k-ask-a4-blur", 900),
        focus_ask("k-ask-a5-keyboard", 900),
        focus_page("k-ask-a6-keyboard-back", 900),
        state("k-ask-a6-state"),
        // b: interrupted. A press 90 ms into the opening, a press outside
        // 90 ms into that, then ten quick changes of mind.
        press_pill("k-ask-b1-open", 90),
        press_pill("k-ask-b2-again", 90),
        press_outside("k-ask-b3-away", 60),
        focus_page("k-ask-b3-blur", 60),
        focus_ask("k-ask-b4-r01", 45),
        focus_page("k-ask-b4-r02", 45),
        focus_ask("k-ask-b4-r03", 45),
        focus_page("k-ask-b4-r04", 45),
        focus_ask("k-ask-b4-r05", 45),
        focus_page("k-ask-b4-r06", 45),
        focus_ask("k-ask-b4-r07", 45),
        focus_page("k-ask-b4-r08", 45),
        focus_ask("k-ask-b4-r09", 45),
        focus_page("k-ask-b4-r10", 45),
        focus_ask("k-ask-b4-r11", 45),
        focus_page("k-ask-b4-r12", 45),
        focus_ask("k-ask-b4-r13", 45),
        focus_page("k-ask-b4-r14", 45),
        focus_ask("k-ask-b4-r15", 45),
        focus_page("k-ask-b4-r16", 45),
        focus_ask("k-ask-b4-r17", 45),
        focus_page("k-ask-b4-r18", 45),
        focus_ask("k-ask-b4-r19", 45),
        focus_page("k-ask-b4-r20", 1000),
        state("k-ask-b4-state"),
        // c: typing while it opens, leaving a draft, a tall draft.
        press_pill("k-ask-c1-open", 70),
        step("k-ask-c2-type", 1000, |view, cx| {
            view.ask.update(cx, |input, cx| input.set_content("What changed", cx));
        }),
        press_outside("k-ask-c3-draft-outside", 900),
        focus_page("k-ask-c3-blur", 600),
        state("k-ask-c3-state"),
        press_pill("k-ask-c4-draft-refocus", 900),
        step("k-ask-c5-tall", 900, |view, cx| {
            view.ask.update(cx, |input, cx| {
                input.set_content(
                    "First line of a long pasted draft\nsecond line\nthird line\nfourth line\nfifth",
                    cx,
                )
            });
        }),
        press_outside("k-ask-c6-tall-outside", 900),
        focus_page("k-ask-c6-blur", 600),
        press_pill("k-ask-c7-tall-refocus", 900),
        step("k-ask-c8-emptied", 600, |view, cx| {
            view.ask.update(cx, |input, cx| input.set_content("", cx));
        }),
        press_outside("k-ask-c9-outside", 600),
        focus_page("k-ask-c9-blur", 900),
        state("k-ask-c9-state"),
        // d: the window and the sidebar change under it.
        press_pill("k-ask-d1-open", 80),
        windowed("k-ask-d2-resize", 1000, |_, window, _| {
            window.resize(size(px(820.), px(640.)));
        }),
        focus_page("k-ask-d3-blur", 80),
        windowed("k-ask-d4-resize-back", 1000, |_, window, _| {
            window.resize(size(px(1100.), px(760.)));
        }),
        focus_ask("k-ask-d5-open", 80),
        step("k-ask-d6-sidebar", 1000, |view, _| view.sidebar_open = false),
        focus_page("k-ask-d7-blur", 80),
        step("k-ask-d8-sidebar-back", 1000, |view, _| view.sidebar_open = true),
        windowed("k-ask-d9-narrow", 900, |view, window, cx| {
            window.resize(size(px(620.), px(560.)));
            view.ask.read(cx).focus_handle.focus(window);
        }),
        focus_page("k-ask-d10-narrow-blur", 900),
        windowed("k-ask-d11-wide", 900, |_, window, _| {
            window.resize(size(px(1100.), px(760.)));
        }),
        state("k-ask-d11-state"),
        // e: the recent chats from the tray, then two presses outside: the
        // first closes the list, the second folds the panel.
        press_pill("k-ask-e1-open", 900),
        input("k-ask-e2-history", 1200, |window, cx| {
            let size = window.viewport_size();
            let centre = 192. + (f32::from(size.width) - 192.) / 2.;
            click(window, cx, centre - 341.5 + 28., f32::from(size.height) - 124. + 20.);
        }),
        state("k-ask-e2-state"),
        press_outside("k-ask-e3-outside", 900),
        state("k-ask-e3-state"),
        press_outside("k-ask-e4-outside", 1000),
        focus_page("k-ask-e4-blur", 900),
        state("k-ask-e4-state"),
        // f: the model menu open, then two presses outside.
        press_pill("k-ask-f1-open", 900),
        step("k-ask-f2-models", 800, |view, _| view.chat_model_picker_open = true),
        press_outside("k-ask-f3-outside", 800),
        state("k-ask-f3-state"),
        press_outside("k-ask-f4-outside", 900),
        step("k-ask-f4-close", 300, |view, _| view.chat_model_picker_open = false),
        focus_page("k-ask-f4-blur", 900),
        state("k-ask-f4-state"),
        // g: a question handed to the panel; a press outside while the
        // answer arrives; the pill pressed again to come back to it.
        press_pill("k-ask-g1-open", 900),
        step("k-ask-g2-send", 700, |view, cx| {
            view.start_scripted_chat("What changed this week?".into(), script(1500), cx);
        }),
        state("k-ask-g2-state"),
        press_outside("k-ask-g3-outside-streaming", 1000),
        focus_page("k-ask-g3-blur", 500),
        state("k-ask-g3-state"),
        press_pill("k-ask-g4-continue", 1200),
        state("k-ask-g4-state"),
        step("k-ask-g5-answer", 3000, |_, _| {}),
        press_outside("k-ask-g6-outside", 70),
        press_pill("k-ask-g7-back-mid-fold", 1200),
        state("k-ask-g7-state"),
        step("k-ask-g8-draft", 500, |view, cx| {
            view.ask.update(cx, |input, cx| input.set_content("And next week", cx));
        }),
        press_outside("k-ask-g9-outside-draft", 1000),
        focus_page("k-ask-g9-blur", 700),
        state("k-ask-g9-state"),
        step("k-ask-g10-clear", 900, |view, cx| view.clear_chat(cx)),
        // h: the dark theme, there and back.
        step("k-ask-h1-dark", 900, |view, _| view.workspace.settings.force_dark(true)),
        press_pill("k-ask-h2-open", 1000),
        press_outside("k-ask-h3-outside", 600),
        focus_page("k-ask-h3-blur", 900),
        step("k-ask-h4-light", 900, |view, _| view.workspace.settings.force_dark(false)),
        // i: on a note. The bar focused and a press outside; a question, its
        // panel, and a press outside while it answers.
        step("k-ask-i1-note", 1200, |view, cx| view.open_note(1, cx)),
        focus_ask("k-ask-i2-focus", 900),
        state("k-ask-i2-state"),
        press_outside("k-ask-i3-outside", 900),
        state("k-ask-i3-state"),
        focus_ask("k-ask-i4-focus", 500),
        step("k-ask-i5-send", 900, |view, cx| {
            view.start_scripted_chat("Summarise this note".into(), script(1200), cx);
        }),
        press_outside("k-ask-i6-outside-streaming", 1200),
        state("k-ask-i6-state"),
        focus_ask("k-ask-i7-continue", 1500),
        state("k-ask-i7-state"),
        step("k-ask-i8-history", 900, |view, cx| {
            view.assistant_open = true;
            view.chat_history_open = true;
            cx.notify();
        }),
        press_outside("k-ask-i9-outside", 900),
        state("k-ask-i9-state"),
        press_outside("k-ask-i10-outside", 1200),
        state("k-ask-i10-state"),
        // j: the panel over a note in a narrow window, thinking, then
        // answered: nothing may overlap or be cut.
        windowed("k-ask-j1-narrow-note", 1200, |view, window, cx| {
            view.clear_chat(cx);
            view.open_note(1, cx);
            window.resize(size(px(600.), px(620.)));
            view.ask.read(cx).focus_handle.focus(window);
        }),
        step("k-ask-j2-send", 1500, |view, cx| {
            view.start_scripted_chat("What did we discuss".into(), script(4000), cx);
        }),
        step("k-ask-j3-answer", 5000, |_, _| {}),
        windowed("k-ask-j4-wide", 1200, |_, window, _| {
            window.resize(size(px(1100.), px(760.)));
        }),
        windowed("k-ask-z-home", 900, |view, window, cx| {
            view.clear_chat(cx);
            view.navigate(Screen::Home, cx);
            view.focus.focus(window);
        }),
        state("k-ask-z-state"),
    ]
}
#[cfg(not(debug_assertions))]
fn ask_bar_steps() -> Vec<Step> {
    Vec::new()
}

/// The sidebar's stills, `p-sidebar-…` (run with `CEREAL_TOUR=p-sidebar`):
/// Home, Chat with its conversations listed, one of them open, the pointer
/// on "Show N more" and on the sidebar's edge, the edge dragged, the
/// narrowest and widest widths, a space and Settings; then the same in the
/// dark theme.
fn sidebar_steps() -> Vec<Step> {
    use crate::desktop_preferences::{SIDEBAR_DEFAULT, SIDEBAR_MAX, SIDEBAR_MIN};
    fn seed(view: &mut Cereal) {
        let now = chrono::Utc::now();
        let account = crate::accounts::AccountId::of("http://tour.invalid", "tour");
        let titles = [
            ("Meeting action items extractor", 1),
            ("Recent meeting action items", 3),
            ("Untitled thread", 12 * 60),
            ("Untitled conversation notes", 12 * 60 + 2),
            ("Untitled thread", 12 * 60 + 4),
            ("New conversation", 12 * 60 + 6),
            ("Zach & Agora YC recap", 20 * 60),
            ("Prototype codename ideas", 26 * 60),
        ];
        let thread = |id: String, title: &str, at: chrono::DateTime<chrono::Utc>| {
            crate::chat::ArchivedChat {
                id,
                note_id: 0,
                account: Some(account.clone()),
                title: title.into(),
                messages: vec![(title.into(), "Here is what I found in your notes.".into())],
                interrupted: vec![],
                annotations: Default::default(),
                updated_at: Some(at.to_rfc3339()),
            }
        };
        view.chat_archives.clear();
        for index in 0..20 {
            view.chat_archives.push(thread(
                format!("older-{index}"),
                "Older conversation",
                now - chrono::Duration::days(3 + index),
            ));
        }
        for (index, (title, minutes)) in titles.iter().enumerate() {
            view.chat_archives.push(thread(
                format!("recent-{index}"),
                title,
                now - chrono::Duration::minutes(*minutes),
            ));
        }
    }
    fn open_thread(view: &mut Cereal, cx: &mut Context<Cereal>) {
        view.chat_threads_expanded = false;
        if let Some(thread) = view.chat_threads().into_iter().find(|thread| thread.id == "recent-0") {
            view.open_chat_thread(&thread.account, &thread.id, true, cx);
        }
    }
    /// The pointer on the sidebar's edge, 400 pt down.
    fn on_edge(window: &mut Window, cx: &mut App) {
        if let Some(edge) = crate::onboarding_view::placed("sidebar-edge") {
            pointer(window, cx, f32::from(edge.center().x), 400.);
        }
    }
    fn away(window: &mut Window, cx: &mut App) {
        let size = window.viewport_size();
        pointer(window, cx, f32::from(size.width) - 40., f32::from(size.height) / 2.);
    }
    /// The pointer on "Show 20 more", the ninth row under Chat.
    fn on_more(window: &mut Window, cx: &mut App) {
        pointer(window, cx, 96., 48. + 8. + 29. * 3. + 29. * 8. + 14.);
    }
    vec![
        windowed("p-sidebar-home", 900, |view, window, cx| {
            view.workspace.settings.force_dark(false);
            view.sidebar_open = true;
            view.sidebar_width = SIDEBAR_DEFAULT;
            seed(view);
            view.clear_chat(cx);
            view.navigate(Screen::Home, cx);
            view.focus.focus(window);
        }),
        step("p-sidebar-chat", 900, |view, cx| view.navigate(Screen::Chat, cx)),
        step("p-sidebar-thread", 900, open_thread),
        // The conversations folded away under Chat, and out again.
        step("p-sidebar-fold", 900, |view, _| view.chat_threads_collapsed = true),
        step("p-sidebar-unfold", 900, |view, _| view.chat_threads_collapsed = false),
        input("p-sidebar-more-hover", 900, on_more),
        input("p-sidebar-edge", 900, on_edge),
        // The edge taken hold of and drawn 68 pt to the right: it follows
        // the pointer, and the width is saved when the button comes up.
        input("p-sidebar-edge-held", 600, |window, cx| {
            if let Some(edge) = crate::onboarding_view::placed("sidebar-edge") {
                window.dispatch_event(
                    PlatformInput::MouseDown(MouseDownEvent {
                        button: MouseButton::Left,
                        position: point(edge.center().x, px(400.)),
                        click_count: 1,
                        ..Default::default()
                    }),
                    cx,
                );
            }
        }),
        input("p-sidebar-edge-dragged", 900, |window, cx| {
            window.dispatch_event(
                PlatformInput::MouseMove(MouseMoveEvent {
                    position: point(px(SIDEBAR_DEFAULT + 68.), px(420.)),
                    pressed_button: Some(MouseButton::Left),
                    ..Default::default()
                }),
                cx,
            );
        }),
        input("p-sidebar-edge-released", 900, |window, cx| {
            window.dispatch_event(
                PlatformInput::MouseUp(MouseUpEvent {
                    button: MouseButton::Left,
                    position: point(px(SIDEBAR_DEFAULT + 68.), px(420.)),
                    click_count: 1,
                    ..Default::default()
                }),
                cx,
            );
            eprintln!(
                "sidebar check: the dragged width {} saved ({})",
                if crate::desktop_preferences::sidebar_width() == SIDEBAR_DEFAULT + 68. {
                    "was"
                } else {
                    "was not"
                },
                crate::desktop_preferences::sidebar_width()
            );
        }),
        input("p-sidebar-away-quiet", 300, away),
        step("p-sidebar-min", 900, |view, _| view.sidebar_width = SIDEBAR_MIN),
        step("p-sidebar-max", 900, |view, _| view.sidebar_width = SIDEBAR_MAX),
        step("p-sidebar-space", 900, |view, cx| {
            view.sidebar_width = SIDEBAR_DEFAULT;
            view.navigate(Screen::MyNotes, cx)
        }),
        step("p-sidebar-settings", 900, |view, cx| {
            view.open_settings(crate::screens::SettingsTab::Preferences, cx)
        }),
        step("p-sidebar-settings-max", 900, |view, _| view.sidebar_width = SIDEBAR_MAX),
        step("p-sidebar-dark-home", 900, |view, cx| {
            view.workspace.settings.force_dark(true);
            view.sidebar_width = SIDEBAR_DEFAULT;
            view.clear_chat(cx);
            view.navigate(Screen::Home, cx);
        }),
        step("p-sidebar-dark-chat", 900, |view, cx| view.navigate(Screen::Chat, cx)),
        step("p-sidebar-dark-thread", 900, open_thread),
        input("p-sidebar-dark-more-hover", 900, on_more),
        input("p-sidebar-dark-edge", 900, on_edge),
        input("p-sidebar-dark-away-quiet", 300, away),
        step("p-sidebar-dark-min", 900, |view, _| view.sidebar_width = SIDEBAR_MIN),
        step("p-sidebar-dark-max", 900, |view, _| view.sidebar_width = SIDEBAR_MAX),
        step("p-sidebar-dark-space", 900, |view, cx| {
            view.sidebar_width = SIDEBAR_DEFAULT;
            view.navigate(Screen::MyNotes, cx)
        }),
        step("p-sidebar-dark-settings", 900, |view, cx| {
            view.open_settings(crate::screens::SettingsTab::Preferences, cx)
        }),
        step("p-sidebar-end", 300, |view, cx| {
            view.workspace.settings.force_dark(false);
            view.navigate(Screen::Home, cx);
        }),
    ]
}

/// Render-count scenarios for `CEREAL_FRAME_STATS=1`, named `b-…`. Like the
/// still states they run only when asked for (`CEREAL_TOUR=b-`). Each prints
/// one `stats` line covering itself; the repeated `b-key` and `b-level` steps
/// are folded into the line of the step that follows them.
fn bench_steps() -> Vec<Step> {
    let mut steps = vec![
        step("b-note", 1600, |view, cx| {
            view.open_note(1, cx);
            view.active_tab = NoteTab::Notes;
        }),
        step("b-idle-note", 3000, |_, _| {}),
    ];
    // Twenty keystrokes, 40 ms apart, into the note body.
    for _ in 0..20 {
        steps.push(windowed("b-key", 40, |view, window, cx| {
            view.editor.update(cx, |input, cx| {
                input.replace_text_in_range(None, "x", window, cx)
            });
        }));
    }
    steps.extend([
        step("b-typing-20-keys", 1200, |_, _| {}),
        input("b-hover-in", 700, |window, cx| {
            pointer(window, cx, 520., 112.)
        }),
        input("b-hover-out", 700, |window, cx| {
            pointer(window, cx, 1100., 420.)
        }),
        step("b-record-start", 2500, |view, cx| {
            view.start_recording_preview(1, cx)
        }),
        step("b-recording-settled", 400, |_, _| {}),
    ]);
    // Three seconds of speech: a new level every 80 ms.
    for index in 0..38u32 {
        steps.push((
            "b-level",
            80,
            Action::View(Box::new(move |view, _| {
                let level = 0.25 + 0.6 * ((index as f32 * 1.7).sin() * 0.5 + 0.5);
                view.recording.set_audio_levels(level, level * 0.4);
            })),
        ));
    }
    steps.extend([
        step("b-recording-speech-3s", 200, |_, _| {}),
        step("b-silence-begins", 600, |view, _| {
            view.recording.set_audio_levels(0., 0.)
        }),
        step("b-recording-silence-3s", 3000, |_, _| {}),
        step("b-pause", 1500, |view, cx| view.stop_recording_preview(cx)),
        step("b-paused-3s", 3000, |_, _| {}),
        step("b-home", 1600, |view, cx| view.navigate(Screen::Home, cx)),
        step("b-idle-home-3s", 3000, |_, _| {}),
        step("b-search-results", 1600, |view, cx| {
            view.search
                .update(cx, |input, cx| input.set_content("review", cx));
            view.open_search_results(cx);
        }),
        step("b-trash", 1600, |view, cx| view.open_trash(cx)),
        step("b-note-again", 1600, |view, cx| view.open_note(1, cx)),
        step("b-list", 1600, |view, cx| view.navigate(Screen::Home, cx)),
        input("b-pointer-rest", 500, |window, cx| {
            pointer(window, cx, 700., 4.)
        }),
        // One list row: its wash eases in and its actions are revealed.
        input("b-row-hover-in", 700, |window, cx| {
            pointer(window, cx, 640., 557.)
        }),
        input("b-row-hover-out", 700, |window, cx| {
            pointer(window, cx, 700., 4.)
        }),
    ]);
    // A sweep down the page: 24 px every 50 ms, a dozen rows in 1.2 s. Rows
    // ease out behind the pointer while the next eases in; they share the
    // window's frames, so the cost is bounded by the sweep's duration.
    for index in 0..24u32 {
        steps.push((
            "b-sweep",
            50,
            Action::Input(Box::new(move |window, cx| {
                pointer(window, cx, 640., 150. + 24. * index as f32)
            })),
        ));
    }
    steps.extend([
        step("b-sweep-24-moves-1.2s", 700, |_, _| {}),
        input("b-sweep-out", 700, |window, cx| {
            pointer(window, cx, 700., 4.)
        }),
    ]);
    // Two seconds of wheel scrolling: the Home list, then a transcript.
    scrolling(&mut steps, 640., 520., -90.);
    steps.push(step("b-scroll-home-2s", 600, |_, _| {}));
    steps.push(step("b-transcript", 1800, |view, cx| {
        view.open_note(1, cx);
        view.transcript_open = true;
        view.transcript_epoch = view.transcript_epoch.wrapping_add(1);
    }));
    // The panel opens on its newest text, at the bottom: scroll up from
    // there. (Scrolling down, as this step once did, moves nothing.)
    scrolling(&mut steps, 600., 520., 90.);
    steps.push(step("b-scroll-transcript-2s", 600, |view, _| {
        let scroll = &view.transcript_scroll;
        eprintln!(
            "scroll check: the transcript is {:.0} px above its newest text after 40 wheel events",
            f32::from(scroll.max_offset().height + scroll.offset().y)
        );
    }));
    // Find jumps from the end of the transcript to the first match.
    steps.push(step("b-transcript-find", 1500, |view, cx| {
        view.transcript_find_open = true;
        view.search
            .update(cx, |input, cx| input.set_content("budget", cx));
    }));
    steps.push(step("b-transcript-close", 900, |view, cx| {
        view.transcript_find_open = false;
        view.search
            .update(cx, |input, cx| input.set_content("", cx));
        view.transcript_open = false;
        view.transcript_epoch = view.transcript_epoch.wrapping_add(1);
    }));
    steps
}

/// Scripted speech for the live-transcript scenarios: what the stream would
/// report, with no socket and no audio. Entirely fictional text.
mod speech {
    use crate::live::{Channel, Event, Interim};
    use meeting_notes_api_types::{AudioChannel, TranscriptSegmentDTO};

    const WORDS: &[&str] = &[
        "so", "if", "we", "move", "the", "pilot", "to", "May", "then", "onboarding", "has", "three",
        "steps", "and", "search", "ships", "with", "calendar", "review", "on", "Monday", "after",
        "five", "usability", "sessions", "budget", "stays", "where", "it", "is", "until", "launch",
    ];

    /// Utterance `index` of the meeting: who says it, when, and its words.
    fn line(index: usize) -> (Channel, u32, u64, Vec<&'static str>) {
        // Sentences of 6 to 34 words, so bubbles run from one line to four.
        let length = 6 + (index * 37 % 29);
        let words = (0..length).map(|word| WORDS[(index * 7 + word * 5) % WORDS.len()]).collect();
        // Turns alternate between the channels, so two consecutive
        // utterances can overlap the way two people do: the server keeps one
        // unfinished utterance per channel, never two on one.
        let channel = if index % 2 == 0 { Channel::Mic } else { Channel::System };
        let speaker = if channel == Channel::Mic { 1 } else { 1 + (index % 8 / 4) as u32 };
        (channel, speaker, index as u64 * 2_400, words)
    }

    /// `count` words of running text, different for each `seed`.
    pub fn words(seed: usize, count: usize) -> String {
        let words: Vec<&str> = (0..count)
            .map(|word| WORDS[(seed * 7 + word * 5) % WORDS.len()])
            .collect();
        sentence(&words)
    }

    fn sentence(words: &[&str]) -> String {
        let mut text = words.join(" ");
        if let Some(first) = text.get_mut(..1) {
            first.make_ascii_uppercase();
        }
        text + "."
    }

    fn utterance(index: usize) -> String {
        format!("00000000-0000-4000-8000-{index:012x}")
    }

    /// The stored segment utterance `index` becomes.
    pub fn stored(index: usize) -> Event {
        let (channel, speaker, start_ms, words) = line(index);
        let name = channel.name();
        Event::Final {
            utterance_id: utterance(index),
            revision: index as u64 + 1,
            segment: TranscriptSegmentDTO {
                channel: if channel == Channel::Mic { AudioChannel::Mic } else { AudioChannel::System },
                end_ms: Some(start_ms as i64 + 2_200),
                id: format!("11111111-0000-4000-8000-{index:012x}"),
                is_final: true,
                speaker_id: format!("{name}:n:{speaker}"),
                speaker_label: Some(format!("Speaker {speaker}")),
                start_ms: start_ms as i64,
                text: sentence(&words),
            },
        }
    }

    /// A stretch the stream missed, as the durable part that covered it
    /// fills it in: `count` stored segments starting at `start_ms`, long
    /// before the newest text.
    pub fn filled(start_ms: u64, count: usize) -> Vec<Event> {
        (0..count)
            .map(|index| {
                let Event::Final { utterance_id, revision, mut segment } = stored(index + 3) else {
                    unreachable!("stored makes finals")
                };
                segment.id = format!("22222222-0000-4000-8000-{:012x}", start_ms as usize + index);
                segment.start_ms = (start_ms + index as u64 * 2_400) as i64;
                segment.end_ms = Some(segment.start_ms + 2_200);
                Event::Final { utterance_id, revision: revision + 2_000, segment }
            })
            .collect()
    }

    /// Utterance `index` while it is being spoken: its first `share` of 20
    /// parts, as interim revision `share`.
    pub fn spoken(index: usize, share: usize) -> Event {
        let (channel, speaker, start_ms, words) = line(index);
        let count = (words.len() * share.clamp(1, 20)).div_ceil(20);
        Event::Interim(Interim {
            channel,
            utterance_id: utterance(index),
            rev: share as u64,
            text: words[..count].join(" "),
            start_ms,
            // The provider names a draft speaker only some of the time.
            speaker_id: (index % 2 == 0).then(|| format!("{}:n:{speaker}", channel.name())),
            speaker_label: (index % 2 == 0).then(|| format!("Speaker {speaker}")),
        })
    }

    /// The first `count` utterances, already final: `count` of 1,500 is an
    /// hour of meeting at one utterance every 2.4 s.
    pub fn meeting(count: usize) -> Vec<Event> {
        (0..count).map(stored).collect()
    }

    /// Update `tick` of a live meeting that already holds `stored` final
    /// utterances: five updates a second, a new utterance every four
    /// seconds, and for half of each utterance a second voice on the other
    /// channel talking over it.
    pub fn update(stored_count: usize, tick: usize) -> Vec<Event> {
        let index = stored_count + 2 * (tick / 20);
        let share = tick % 20 + 1;
        let mut events = Vec::new();
        if share == 1 && tick > 0 {
            // The previous utterance and its interjection become final.
            events.push(stored(index - 2));
            events.push(stored(index - 1));
        }
        events.push(spoken(index, share));
        if share > 10 {
            events.push(spoken(index + 1, (share - 10) * 2));
        }
        events
    }
}

thread_local! {
    /// Where the reader had scrolled to before more text arrived.
    static LIVE_READER: std::cell::Cell<f32> = const { std::cell::Cell::new(0.) };
}

/// An hour-long meeting being transcribed live (`b-live-…`): 1,500 stored
/// utterances, the transcript open and following its tail, and interim text
/// arriving five times a second. Then the same with the reader scrolled up.
fn live_bench_steps() -> Vec<Step> {
    const HOUR: usize = 1_500;
    let mut steps = vec![
        step("b-live-hour-setup", 3000, |view, cx| {
            view.open_note(1, cx);
            view.active_tab = NoteTab::Notes;
            if let Some(note) = view.workspace.note_mut(1) {
                note.replace_transcript("");
            }
            view.live_preview(1, speech::meeting(HOUR), cx);
            view.recording_last_click = None;
            view.recording_changed_at = None;
            view.start_recording_preview(1, cx);
            view.transcript_open = true;
            view.transcript_epoch = view.transcript_epoch.wrapping_add(1);
        }),
        step("b-live-hour-idle-3s", 3000, |_, _| {}),
    ];
    let updates = |steps: &mut Vec<Step>, ticks: std::ops::Range<usize>| {
        for tick in ticks {
            steps.push((
                "b-live-update",
                200,
                Action::View(Box::new(move |view, cx| {
                    view.live_preview(1, speech::update(HOUR, tick), cx)
                })),
            ));
        }
    };
    updates(&mut steps, 0..100);
    steps.push(step("b-live-interim-5-per-s-20s", 400, |view, _| {
        let scroll = &view.transcript_scroll;
        eprintln!(
            "live check: after 100 updates the view following the transcript is {:.0} px from its newest text",
            f32::from(scroll.max_offset().height + scroll.offset().y)
        );
    }));
    // The reader scrolls up to re-read; text keeps arriving below.
    for _ in 0..12 {
        steps.push((
            "b-live-wheel",
            50,
            Action::Input(Box::new(|window, cx| scroll(window, cx, 600., 520., 90.))),
        ));
    }
    steps.push(step("b-live-reader-scrolls-up", 400, |view, _| {
        let scroll = &view.transcript_scroll;
        LIVE_READER.with(|reader| reader.set(f32::from(scroll.offset().y)));
        eprintln!(
            "live check: reader scrolled {:.0} px up from the newest text",
            f32::from(scroll.max_offset().height + scroll.offset().y)
        );
    }));
    updates(&mut steps, 100..150);
    steps.push(step("b-live-interim-while-scrolled-up-10s", 400, |view, _| {
        let scroll = &view.transcript_scroll;
        eprintln!(
            "live check: after 50 more updates the reader's position moved {:.0} px (now {:.0} px above the newest text)",
            f32::from(scroll.offset().y) - LIVE_READER.with(|reader| reader.get()),
            f32::from(scroll.max_offset().height + scroll.offset().y)
        );
    }));
    // A stretch the stream missed half an hour ago is filled in from its
    // durable part: eight segments appear far above the reader, who is
    // still scrolled up. What they are reading must not move.
    steps.push(step("b-live-gap-filled-above-reader", 900, |view, cx| {
        let scroll = &view.transcript_scroll;
        LIVE_READER.with(|reader| reader.set(f32::from(scroll.max_offset().height + scroll.offset().y)));
        view.live_preview(1, speech::filled(1_800_000, 8), cx);
    }));
    steps.push(step("b-live-gap-filled-reader-check", 300, |view, _| {
        let scroll = &view.transcript_scroll;
        let now = f32::from(scroll.max_offset().height + scroll.offset().y);
        eprintln!(
            "live check: eight segments filled in above a reader who had scrolled up; the text they were reading moved {:.1} px (it is {:.0} px above the newest text, as before)",
            (now - LIVE_READER.with(|reader| reader.get())).abs(),
            now
        );
    }));
    // And the same for a view that is following the newest text.
    steps.push(step("b-live-follow-again", 900, |view, _| {
        view.transcript_epoch = view.transcript_epoch.wrapping_add(1);
    }));
    steps.push(step("b-live-gap-filled-above-follower", 900, |view, cx| {
        view.live_preview(1, speech::filled(2_400_000, 8), cx);
    }));
    steps.push(step("b-live-gap-filled-follower-check", 300, |view, _| {
        let scroll = &view.transcript_scroll;
        eprintln!(
            "live check: eight more filled in above a view following the transcript; it is {:.1} px from the newest text",
            f32::from(scroll.max_offset().height + scroll.offset().y)
        );
    }));
    steps.push(step("b-live-stop", 1500, |view, cx| {
        view.recording_last_click = None;
        view.recording_changed_at = None;
        view.stop_recording_preview(cx);
    }));
    steps
}

/// Opening long content for the first time (`b-first-…`): the transcript
/// panel on an hour-long meeting, and the Enhanced tab on a 1,500-bullet
/// summary. The first frame used to measure every row; these steps show what
/// it costs now. Self-contained: they install their own content in note 1.
/// Note 1 as an hour-long meeting: 1,500 stored segments and an Enhanced
/// tab of 150 sections of ten bullets, some long enough to wrap.
fn hour_long_note(view: &mut Cereal, cx: &mut Context<Cereal>) {
    view.navigate(Screen::Home, cx);
    view.open_note(1, cx);
    view.active_tab = NoteTab::Notes;
    if let Some(note) = view.workspace.note_mut(1) {
        note.replace_transcript("");
        note.enhanced = (0..150)
            .map(|section| {
                let bullets: Vec<String> = (0..10)
                    .map(|bullet| {
                        let words = 6 + (section * 7 + bullet * 13) % 26;
                        format!("- {}", speech::words(section * 10 + bullet, words))
                    })
                    .collect();
                format!("## Section {}\n\n{}", section + 1, bullets.join("\n"))
            })
            .collect::<Vec<_>>()
            .join("\n\n");
    }
    view.live_preview(1, speech::meeting(1_500), cx);
}

fn first_frame_steps() -> Vec<Step> {
    let mut steps = vec![
        step("b-first-setup", 1500, hour_long_note),
        step("b-first-open-hour-transcript", 1500, |view, _| {
            view.transcript_open = true;
            view.transcript_epoch = view.transcript_epoch.wrapping_add(1);
        }),
    ];
    for _ in 0..20 {
        steps.push((
            "b-first-wheel",
            50,
            Action::Input(Box::new(|window, cx| scroll(window, cx, 600., 520., 240.))),
        ));
    }
    steps.extend([
        step("b-first-scroll-up-hour-transcript", 600, |_, _| {}),
        step("b-first-close-transcript", 900, |view, _| {
            view.transcript_open = false;
            view.transcript_epoch = view.transcript_epoch.wrapping_add(1);
        }),
        step("b-first-open-1500-bullets", 1500, |view, _| {
            view.active_tab = NoteTab::Enhanced;
        }),
    ]);
    for _ in 0..20 {
        steps.push((
            "b-first-wheel",
            50,
            Action::Input(Box::new(|window, cx| scroll(window, cx, 600., 420., -240.))),
        ));
    }
    steps.extend([
        step("b-first-scroll-1500-bullets", 600, |_, _| {}),
        step("b-first-back-to-my-notes", 900, |view, _| {
            view.active_tab = NoteTab::Notes;
        }),
    ]);
    steps
}

/// The transcript panel while recording, `p-txp-…` (run with
/// `CEREAL_TOUR=p-txp,f-txp`): lines being spoken, Find empty and with
/// matches, the sound popover, the microphone and language menus; then
/// `f-txp-…`, the panel folding into the bar and growing back out of it,
/// and toggled mid-flight, for a frame-by-frame capture.
fn transcript_panel_steps() -> Vec<Step> {
    fn setup(view: &mut Cereal, cx: &mut Context<Cereal>) {
        view.navigate(Screen::Home, cx);
        view.open_note(1, cx);
        view.active_tab = NoteTab::Notes;
        if let Some(note) = view.workspace.note_mut(1) {
            note.replace_transcript("");
        }
        view.live_preview(1, speech::meeting(7), cx);
        view.recording_last_click = None;
        view.recording_changed_at = None;
        view.start_recording_preview(1, cx);
        view.transcript_open = true;
        view.transcript_epoch = view.transcript_epoch.wrapping_add(1);
        view.live_preview(1, vec![speech::spoken(7, 12), speech::spoken(8, 9)], cx);
    }
    fn find(view: &mut Cereal, window: &mut Window, cx: &mut Context<Cereal>, query: &str) {
        if !view.transcript_find_open {
            view.transcript_find_begin(window, cx);
        }
        view.search.update(cx, |input, cx| input.set_content(query.to_owned(), cx));
        cx.notify();
    }
    fn toggle(view: &mut Cereal, cx: &mut Context<Cereal>, open: bool) {
        view.menu = None;
        view.transcript_open = open;
        view.transcript_epoch = view.transcript_epoch.wrapping_add(1);
        cx.notify();
    }
    vec![
        step("p-txp-recording", 3000, setup),
        step("p-txp-growing", 3000, |view, cx| {
            view.live_preview(1, vec![speech::spoken(7, 16), speech::spoken(8, 13)], cx);
        }),
        windowed("p-txp-find", 2000, |view, window, cx| find(view, window, cx, "")),
        windowed("p-txp-find-query", 2000, |view, window, cx| find(view, window, cx, "the")),
        step("p-txp-find-next", 2000, |view, cx| {
            view.transcript_find_index += 3;
            cx.notify();
        }),
        windowed("p-txp-find-closed", 2000, |view, window, cx| view.transcript_find_end(window, cx)),
        step("p-txp-sound", 2000, |view, cx| {
            view.refresh_input_devices();
            view.menu = Some(Menu::RecordingSettings);
            cx.notify();
        }),
        menu("p-txp-microphone", 2000, Some(Menu::RecordingInput)),
        menu("p-txp-language", 2000, Some(Menu::RecordingLanguage)),
        step("p-txp-final", 2000, |view, cx| {
            view.menu = None;
            view.live_preview(1, vec![speech::stored(7)], cx);
        }),
        step("f-txp-close", 5000, |view, cx| toggle(view, cx, false)),
        step("f-txp-open", 5000, |view, cx| toggle(view, cx, true)),
        // Interrupted both ways: closed during the open, opened during the close.
        step("f-txp-flip", 250, |view, cx| toggle(view, cx, false)),
        step("f-txp-flop", 250, |view, cx| toggle(view, cx, true)),
        step("f-txp-flip2", 150, |view, cx| toggle(view, cx, false)),
        step("p-txp-after-flips", 3000, |view, cx| toggle(view, cx, true)),
        step("p-txp-end", 1200, |view, cx| {
            view.recording_last_click = None;
            view.recording_changed_at = None;
            view.stop_recording_preview(cx);
        }),
    ]
}

/// Still states of the live transcript for review, `p-live-…` (run with
/// `CEREAL_TOUR=p-live`; add `--view=pill-expanded` for the pill's popup).
fn live_still_steps() -> Vec<Step> {
    use crate::live::{Event, Phase};
    vec![
        step("p-live-interim", 5000, |view, cx| {
            view.navigate(Screen::Home, cx);
            view.open_note(1, cx);
            view.active_tab = NoteTab::Notes;
            if let Some(note) = view.workspace.note_mut(1) {
                note.replace_transcript("");
            }
            view.live_preview(1, speech::meeting(7), cx);
            view.recording_last_click = None;
            view.recording_changed_at = None;
            view.start_recording_preview(1, cx);
            view.transcript_open = true;
            view.transcript_epoch = view.transcript_epoch.wrapping_add(1);
            // Two voices mid-sentence: one on each channel.
            view.live_preview(1, vec![speech::spoken(7, 12), speech::spoken(8, 9)], cx);
        }),
        step("p-live-final", 5000, |view, cx| {
            // The system voice finishes; the microphone is still talking.
            view.live_preview(1, vec![speech::stored(7), speech::spoken(8, 14)], cx);
        }),
        step("p-live-catching-up", 5000, |view, cx| {
            view.live_preview(1, vec![Event::Status(Phase::CatchingUp, 15)], cx);
        }),
        step("p-live-provider-unavailable", 5000, |view, cx| {
            view.live_preview(1, vec![Event::Status(Phase::ProviderUnavailable, 0)], cx);
        }),
        step("p-live-recovered", 5000, |view, cx| {
            view.live_preview(
                1,
                vec![Event::Status(Phase::Live, 0), speech::stored(8), speech::spoken(9, 6)],
                cx,
            );
        }),
        step("p-live-stop", 1200, |view, cx| {
            view.recording_last_click = None;
            view.recording_changed_at = None;
            view.stop_recording_preview(cx);
        }),
    ]
}

/// The screens the colour scheme is reviewed on, `p-gold-…`: stills, each
/// held long enough to capture. Run it in the light theme and again with
/// `--dark`; add `--view=pill-expanded` to `p-gold-recording` for the pill.
fn gold_steps() -> Vec<Step> {
    use crate::live::{Event, Phase};
    // A chat turn in the open note: a question, its answer and the answer's
    // citation of a transcript passage, as the server sends them.
    fn turn(view: &mut Cereal, question: &str, answer: &str) {
        let index = view.chat_messages.len();
        view.chat_messages.push((question.into(), answer.into()));
        let citation = serde_json::json!({
            "id": "S1", "noteId": "419d65d1-7a2c-44ca-84cc-ad489522d561", "title": "Product & design sync",
            "source": "transcript", "segmentId": "419d65d1-7a2c-44ca-84cc-ad489522d562", "startMs": 18000,
            "quote": "Keep onboarding to three steps.", "transcriptRevision": 7
        });
        // One tool call, so the turn has its "Thought for 3s" line.
        let tool = serde_json::json!({
            "callId": "419d65d1-7a2c-44ca-84cc-ad489522d563", "name": "search_meetings", "status": "completed",
            "label": "Found 1 meeting", "resultCount": 1,
            "results": [{ "noteId": "419d65d1-7a2c-44ca-84cc-ad489522d561", "title": "Product & design sync", "date": "2026-11-12" }]
        });
        if let Ok(notes) = serde_json::from_value(serde_json::json!({ "tools": [tool], "citations": [citation], "elapsed_ms": 3000 })) {
            view.chat_annotations.insert(index, notes);
        }
    }
    let mut steps = vec![
        step("p-gold-home", 5000, |view, cx| view.navigate(Screen::Home, cx)),
        step("p-gold-enhanced", 5000, |view, cx| {
            view.open_note(1, cx);
            view.active_tab = NoteTab::Enhanced;
        }),
        step("p-gold-recording", 5000, |view, cx| {
            view.active_tab = NoteTab::Notes;
            if let Some(note) = view.workspace.note_mut(1) {
                note.replace_transcript("");
            }
            view.live_preview(1, speech::meeting(7), cx);
            view.recording_last_click = None;
            view.recording_changed_at = None;
            view.start_recording_preview(1, cx);
            view.transcript_open = true;
            view.transcript_epoch = view.transcript_epoch.wrapping_add(1);
            view.live_preview(1, vec![speech::spoken(7, 12), speech::spoken(8, 9)], cx);
        }),
        // The warning line under the transcript beside the gold recording bars.
        step("p-gold-recording-warning", 5000, |view, cx| {
            view.live_preview(1, vec![Event::Status(Phase::ProviderUnavailable, 0)], cx);
        }),
        step("p-gold-paused", 5000, |view, cx| {
            view.live_preview(1, vec![Event::Status(Phase::Live, 0)], cx);
            view.recording_last_click = None;
            view.recording_changed_at = None;
            view.stop_recording_preview(cx);
        }),
        // A first answer, short enough that almost nothing is hidden: the
        // jump-to-latest button must stay away (it used to sit on the chips).
        step("p-gold-chat", 5000, |view, _| {
            view.transcript_open = false;
            view.transcript_epoch = view.transcript_epoch.wrapping_add(1);
            view.chat_messages.clear();
            view.chat_annotations.clear();
            turn(
                view,
                "What was just said?",
                "The Quilfeather outages were separate from the paused Brambling cash rollout; they came from the TAMSIN certificate rotation. [[S1]] The speaker is now saying they agreed to add an alert at 90 seconds of queue delay, but the sentence is still in progress.",
            );
            view.assistant_open = true;
        }),
        step("p-gold-chat-check", 600, |view, _| {
            let scroll = &view.chat_scroll;
            eprintln!(
                "gold check: after a first answer {:.0} px of the conversation is hidden below; the jump-to-latest button is {}",
                f32::from(scroll.max_offset().height + scroll.offset().y),
                if view.chat_can_scroll_down() { "shown" } else { "not shown" }
            );
        }),
        // Citation marks past the twentieth, side by side.
        step("p-marks", 5000, |view, _| {
            view.chat_messages.clear();
            view.chat_annotations.clear();
            view.chat_messages.push((
                "What was decided?".into(),
                "The retrospective moved to the Grayweather room. [[S19]][[S20]][[S21]][[S22]] The release ships Monday. [[S35]][[S36]][[S50]] You will send the summary tonight. [[S51]][[S52]]".into(),
            ));
            let citations: Vec<_> = [19, 20, 21, 22, 35, 36, 50, 51, 52]
                .iter()
                .map(|number| {
                    serde_json::json!({
                        "id": format!("S{number}"), "noteId": "419d65d1-7a2c-44ca-84cc-ad489522d561", "title": "Product & design sync",
                        "source": "transcript", "segmentId": "419d65d1-7a2c-44ca-84cc-ad489522d562", "startMs": 18000,
                        "quote": "Keep onboarding to three steps.", "transcriptRevision": 7
                    })
                })
                .collect();
            if let Ok(notes) = serde_json::from_value(serde_json::json!({ "citations": citations })) {
                view.chat_annotations.insert(0, notes);
            }
            view.assistant_open = true;
        }),
        // A longer conversation, scrolled up: now the button belongs.
        step("p-gold-chat-long", 1500, |view, _| {
            for index in 0..4 {
                turn(
                    view,
                    "And before that?",
                    &format!("{} [[S1]]\n\n{}", speech::words(index, 40), speech::words(index + 9, 55)),
                );
            }
        }),
        input("p-gold-chat-scrolled-up", 5000, |window, cx| scroll(window, cx, 600., 560., 320.)),
        step("p-gold-chat-check", 600, |view, cx| {
            let scroll = &view.chat_scroll;
            eprintln!(
                "gold check: scrolled up, {:.0} px is hidden below; the jump-to-latest button is {}",
                f32::from(scroll.max_offset().height + scroll.offset().y),
                if view.chat_can_scroll_down() { "shown" } else { "not shown" }
            );
            view.close_chat_surface(cx);
        }),
        step("p-gold-settings", 2500, |view, cx| view.navigate(Screen::Settings, cx)),
    ];
    // Down to the Dock icon picker.
    steps.push(input("p-gold-settings-icons", 5000, |window, cx| scroll(window, cx, 700., 500., -420.)));
    steps.push(step("p-gold-login", 5000, |view, cx| view.navigate(Screen::Login, cx)));
    steps.push(step("p-gold-end", 600, |view, cx| view.navigate(Screen::Home, cx)));
    steps
}

/// The recording capsule in the note's bottom bar, `p-bar-…`: at rest and
/// with each of its controls hovered, while recording and once stopped, for
/// measuring its insets. The pointer rests low in each control so its
/// tooltip opens below the bar. `CEREAL_TOUR_BAR=<x>,<y>` is the capsule's
/// top-left corner in window points (the default suits
/// `CEREAL_TOUR_SIZE=1200x800`).
fn bar_steps() -> Vec<Step> {
    fn at(window: &mut Window, cx: &mut App, x: f32, y: f32) {
        let (left, top) = std::env::var("CEREAL_TOUR_BAR")
            .ok()
            .and_then(|corner| {
                let (x, y) = corner.split_once(',')?;
                Some((x.trim().parse::<f32>().ok()?, y.trim().parse::<f32>().ok()?))
            })
            .unwrap_or((264., 716.));
        pointer(window, cx, left + x, top + y);
    }
    thread_local! {
        /// Note 1's written notes, put back after the steps that change them.
        static NOTES: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
    }
    fn restore(view: &mut Cereal) {
        if let Some(notes) = NOTES.with(|notes| notes.borrow_mut().take())
            && let Some(note) = view.workspace.note_mut(1)
        {
            note.enhanced = notes;
        }
    }
    vec![
        step("p-bar-live", 3000, |view, cx| {
            view.navigate(Screen::Home, cx);
            view.open_note(1, cx);
            view.active_tab = NoteTab::Notes;
            view.recording_last_click = None;
            view.recording_changed_at = None;
            view.start_recording_preview(1, cx);
            view.transcript_open = false;
            view.transcript_epoch = view.transcript_epoch.wrapping_add(1);
            // Speech, as in the reference capture: bars of 13, 20 and 13 pt.
            view.recording.set_audio_levels(0.82, 0.);
        }),
        input("p-bar-live-indicator", 3000, |window, cx| {
            at(window, cx, 29., 50.)
        }),
        input("p-bar-live-stop", 3000, |window, cx| {
            at(window, cx, 73., 50.)
        }),
        input("p-bar-stopping", 600, |window, cx| {
            at(window, cx, -200., -200.)
        }),
        step("p-bar-paused", 3000, |view, cx| {
            // Without written notes the stopped capsule offers "Resume".
            if let Some(note) = view.workspace.note_mut(1) {
                let notes = std::mem::take(&mut note.enhanced);
                NOTES.with(|saved| {
                    saved.borrow_mut().get_or_insert(notes);
                });
            }
            view.recording_last_click = None;
            view.recording_changed_at = None;
            view.stop_recording_preview(cx);
        }),
        input("p-bar-paused-indicator", 3000, |window, cx| {
            at(window, cx, 29., 50.)
        }),
        input("p-bar-paused-resume", 3000, |window, cx| {
            at(window, cx, 86., 50.)
        }),
        input("p-bar-written", 600, |window, cx| {
            at(window, cx, -200., -200.)
        }),
        // Once notes are written the stopped capsule is the indicator alone.
        step("p-bar-done", 3000, |view, _| {
            restore(view);
            if let Some(note) = view.workspace.note_mut(1)
                && note.enhanced.trim().is_empty()
            {
                note.enhanced = "Launch checklist".into();
            }
        }),
        input("p-bar-done-indicator", 3000, |window, cx| {
            at(window, cx, 29., 50.)
        }),
        // Takes the pointer off the bar for the tours that follow.
        input("p-bar-leave-quiet", 300, |window, cx| {
            at(window, cx, -200., -200.)
        }),
    ]
}

/// A stretch the stream skipped and its durable part then filled in,
/// `p-gap-…`: stills for review. The reader has scrolled up and the filled
/// text lands above what they are reading, which must not move.
fn gap_still_steps() -> Vec<Step> {
    use crate::live::{Channel, Event};
    vec![
        step("p-gap-skipped", 5000, |view, cx| {
            view.navigate(Screen::Home, cx);
            view.open_note(1, cx);
            view.active_tab = NoteTab::Notes;
            if let Some(note) = view.workspace.note_mut(1) {
                note.replace_transcript("");
            }
            // Four minutes of meeting; the stream lost 0:48 to 1:12.
            let heard = speech::meeting(100)
                .into_iter()
                .enumerate()
                .filter(|(index, _)| !(20..30).contains(index))
                .map(|(_, event)| event);
            view.live_preview(1, heard.collect(), cx);
            view.recording_last_click = None;
            view.recording_changed_at = None;
            view.start_recording_preview(1, cx);
            view.transcript_open = true;
            view.transcript_epoch = view.transcript_epoch.wrapping_add(1);
            view.live_preview(
                1,
                vec![Event::Gap { channel: Channel::System, from_ms: 48_000, to_ms: 72_000 }],
                cx,
            );
        }),
        input("p-gap-reader-scrolls-up", 5000, |window, cx| scroll(window, cx, 600., 520., 900.)),
        step("p-gap-reader-before", 5000, |view, _| {
            let scroll = &view.transcript_scroll;
            LIVE_READER.with(|reader| reader.set(f32::from(scroll.max_offset().height + scroll.offset().y)));
        }),
        // The part that covered the lost stretch is transcribed: its ten
        // segments arrive, all of them above the reader.
        step("p-gap-filled-above-reader", 5000, |view, cx| {
            view.live_preview(1, speech::filled(48_000, 10), cx);
        }),
        step("p-gap-check", 600, |view, _| {
            let scroll = &view.transcript_scroll;
            let now = f32::from(scroll.max_offset().height + scroll.offset().y);
            eprintln!(
                "gap check: ten segments filled in above the reader; what they were reading moved {:.1} px on screen (it is {:.0} px above the newest text)",
                (now - LIVE_READER.with(|reader| reader.get())).abs(),
                now
            );
        }),
        // Scrolled to where the stretch was: the filled text is in its
        // place on the timeline.
        input("p-gap-filled-in-place", 5000, |window, cx| scroll(window, cx, 600., 520., 6000.)),
        step("p-gap-stop", 1200, |view, cx| {
            view.recording_last_click = None;
            view.recording_changed_at = None;
            view.stop_recording_preview(cx);
        }),
    ]
}

/// The audit's own stills, `a-…`: states the other sets do not reach. Run
/// without `--motion-tour` (`CEREAL_AUDIT=1`) they act on the notes of the
/// server the data directory is signed in to, found by title.
fn audit_steps() -> Vec<Step> {
    /// The note whose title starts with `prefix`.
    fn titled(view: &Cereal, prefix: &str) -> Option<u64> {
        view.workspace
            .notes
            .iter()
            .find(|note| {
                !note.trashed
                    && if prefix.is_empty() {
                        note.title.trim().is_empty()
                    } else {
                        note.title.starts_with(prefix)
                    }
            })
            .map(|note| note.id)
    }
    fn open(name: &'static str, prefix: &'static str) -> Step {
        step(name, 900, move |view, cx| {
            view.navigate(Screen::Home, cx);
            match titled(view, prefix) {
                Some(id) => view.open_note(id, cx),
                None => eprintln!("tour: no note titled {prefix:?}"),
            }
        })
    }
    fn wheel(name: &'static str, x: f32, y: f32, by: f32) -> Step {
        input(name, 600, move |window, cx| scroll(window, cx, x, y, by))
    }
    /// Follow the `index`th link of `CEREAL_AUDIT_LINKS`.
    fn follow(view: &mut Cereal, index: usize, cx: &mut Context<Cereal>) {
        let links = std::env::var("CEREAL_AUDIT_LINKS").unwrap_or_default();
        match links
            .split_whitespace()
            .nth(index)
            .and_then(crate::link::AppLink::parse)
        {
            Some(link) => view.open_link(link, cx),
            None => eprintln!("tour: no link {index}"),
        }
    }
    /// Put the open note on the calendar, starting `minutes` from now.
    fn meeting(view: &mut Cereal, minutes: i64) {
        let at = |minutes: i64| {
            (crate::menubar::now() + chrono::Duration::minutes(minutes))
                .with_timezone(&chrono::Utc)
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
        };
        let person = |name: &str, email: &str, organizer: bool| {
            serde_json::json!({"id":uuid::Uuid::new_v4().to_string(),"email":email,"displayName":name,"givenName":null,"familyName":null,"nameSource":"unknown","isSelf":false,"isOrganizer":organizer,"isResource":false,"responseStatus":"accepted"})
        };
        let context = serde_json::from_value(serde_json::json!({"eventId":"00000000-0000-4000-8000-000000000002","provider":"google","title":"Northwind onboarding call","calendarName":"Work","startAt":at(minutes),"endAt":at(minutes + 45),"allDay":false,"timeZone":null,"location":"Meeting room with a very long name on the fourth floor, gjpqy","meetingUrl":"https://meet.example.com/abc-defg-hij","eventUrl":null,"organizer":person("Dana Whitfield-Montgomery","dana@northwind.example",true),"participants":[person("Priya Raman","priya@northwind.example",false),person("A Very Long Participant Name That Should Truncate Somewhere Sensible","long.participant.name@northwind.example",false),person("عمر","omar@northwind.example",false)]}));
        match context {
            Ok(context) => {
                if let Some(note) = view.current_note.and_then(|id| view.workspace.note_mut(id)) {
                    note.meeting_context = Some(context);
                }
            }
            Err(error) => eprintln!("tour: meeting context: {error}"),
        }
    }
    fn update(name: &'static str, state: &'static str) -> Step {
        step(name, 900, move |view, cx| {
            view.open_settings(crate::screens::SettingsTab::About, cx);
            view.updates.review(state);
        })
    }
    const ANSWER: &str = "Here is the plan, with gjpqy descenders.\n\n## Owners\n\n| Owner | Task | Due | Notes |\n| --- | --- | --- | --- |\n| Sam | Missing key banner | Wed | Shows on the note and in the admin panel; must not block recording |\n| Priya | iPhone upload with privacy manifest and export compliance flag set | Wed noon | — |\n| Dana | Announcement | Tue | aVeryLongUnbrokenCellValueThatShouldNotPushTheTableOutOfTheColumn_0123456789 |\n\n```rust\nfn main() {\n    let very_long_line_of_code_that_goes_on_and_on = some_function_with_a_long_name(argument_one, argument_two, argument_three, argument_four);\n    println!(\"{very_long_line_of_code_that_goes_on_and_on}\");\n}\n```\n\n1. First step with `inline code gjpqy`\n2. Second step, **bold** and *italic*\n   - Nested point that is long enough to wrap onto a second line in the compact assistant panel at any width\n\nhttps://example.com/aVeryLongPathSegmentThatNeverBreaksBecauseItHasNoSpacesOrHyphensAtAll_0123456789_abcdefghijklmnopqrstuvwxyz_ABCDEFGHIJKLMNOPQRSTUVWXYZ\n\nمرحبا بالعالم 🚀 final line gjpqy.";
    let mut steps = vec![
        step("a-home", 900, |view, cx| view.navigate(Screen::Home, cx)),
        wheel("a-home-list", 700., 500., -420.),
        input("a-home-row-hover", 600, |window, cx| pointer(window, cx, 700., 300.)),
        input("a-home-row-menu", 600, |window, cx| {
            window.dispatch_event(
                PlatformInput::MouseDown(MouseDownEvent {
                    button: MouseButton::Right,
                    position: point(px(700.), px(300.)),
                    click_count: 1,
                    ..Default::default()
                }),
                cx,
            );
        }),
        menu("a-home-row-menu-close-quiet", 300, None),
        wheel("a-home-list-2", 700., 500., -700.),
        wheel("a-home-list-deep", 700., 500., -9000.),
        wheel("a-home-list-end", 700., 500., -900_000.),
        step("a-home-sidebar-closed", 900, |view, cx| {
            view.navigate(Screen::Home, cx);
            view.sidebar_open = false;
        }),
        step("a-search-open", 900, |view, _| {
            view.sidebar_open = true;
            view.search_open = true;
        }),
        windowed("a-search-query", 900, |view, _, cx| {
            view.search
                .update(cx, |input, cx| input.set_content("gjpqy", cx));
            cx.notify();
        }),
        windowed("a-search-no-match", 900, |view, _, cx| {
            view.search
                .update(cx, |input, cx| input.set_content("zzzzqqqq nothing matches this", cx));
            cx.notify();
        }),
        windowed("a-search-results", 900, |view, _, cx| {
            view.search
                .update(cx, |input, cx| input.set_content("launch", cx));
            view.open_search_results(cx);
            view.search_open = false;
        }),
        wheel("a-search-results-end", 700., 400., -90_000.),
        step("a-my-notes", 900, |view, cx| view.navigate(Screen::MyNotes, cx)),
        step("a-favorites", 900, |view, cx| view.navigate(Screen::Favorites, cx)),
        step("a-trash", 900, |view, cx| view.open_trash(cx)),
        step("a-shared", 900, |view, cx| view.navigate(Screen::Shared, cx)),
        windowed("a-shared-paste-invalid", 900, |view, _, cx| {
            view.links
                .paste
                .update(cx, |input, cx| input.set_content("not a link at all gjpqy", cx));
            view.open_pasted_link(cx);
        }),
        step("a-account", 900, |view, cx| view.navigate(Screen::Account, cx)),
        open("a-note-long-title", "Quarterly planning, budgeting"),
        menu("a-note-long-title-menu", 900, Some(Menu::Note)),
        menu("a-note-long-title-share", 900, Some(Menu::Share)),
        menu("a-note-long-title-details", 900, Some(Menu::NoteDetails)),
        menu("a-note-long-title-templates", 900, Some(Menu::Templates)),
        menu("a-note-menus-close-quiet", 300, None),
        step("a-note-long-title-own", 900, |view, _| view.active_tab = NoteTab::Notes),
        step("a-note-long-title-transcript", 900, |view, _| {
            view.transcript_open = true;
            view.transcript_epoch = view.transcript_epoch.wrapping_add(1);
        }),
        open("a-note-unbroken-title", "AVeryLongUnbrokenTitle"),
        open("a-note-empty", ""),
        open("a-note-rtl", "مراجعة"),
        open("a-note-emoji", "🚀🚀 Launch"),
        open("a-note-hostile", "Hostile content"),
    ];
    for name in [
        "a-note-hostile-2",
        "a-note-hostile-3",
        "a-note-hostile-4",
        "a-note-hostile-5",
        "a-note-hostile-6",
    ] {
        steps.push(wheel(name, 600., 400., -560.));
    }
    steps.push(step("a-note-hostile-own", 900, |view, _| {
        view.active_tab = NoteTab::Notes
    }));
    for name in ["a-note-hostile-own-2", "a-note-hostile-own-3"] {
        steps.push(wheel(name, 600., 400., -560.));
    }
    steps.extend([
        step("a-note-hostile-transcript", 900, |view, _| {
            view.active_tab = NoteTab::Enhanced;
            view.transcript_open = true;
            view.transcript_epoch = view.transcript_epoch.wrapping_add(1);
        }),
        step("a-note-hostile-chat", 900, |view, cx| {
            view.transcript_open = false;
            view.transcript_epoch = view.transcript_epoch.wrapping_add(1);
            view.chat_messages.push((
                "Summarise the launch plan in a table, with the code we discussed, and keep going for a while so that the question itself wraps onto several lines gjpqy".into(),
                ANSWER.into(),
            ));
            view.assistant_open = true;
            cx.notify();
        }),
        wheel("a-note-hostile-chat-2", 600., 600., -300.),
        wheel("a-note-hostile-chat-3", 600., 600., -300.),
        step("a-note-hostile-chat-max", 900, |view, _| view.chat_maximized = true),
        wheel("a-note-hostile-chat-max-2", 600., 500., -500.),
        step("a-chat-full", 900, |view, cx| {
            view.chat_maximized = false;
            view.close_chat_surface(cx);
            view.navigate(Screen::Chat, cx);
            view.chat_messages.clear();
            view.chat_messages.push((
                "Summarise the launch plan in a table, with the code we discussed, and keep going for a while so that the question itself wraps onto several lines gjpqy".into(),
                ANSWER.into(),
            ));
            view.chat_messages
                .push(("And then?".into(), ANSWER.repeat(2)));
            cx.notify();
        }),
        wheel("a-chat-full-2", 700., 400., -500.),
        wheel("a-chat-full-3", 700., 400., -500.),
        wheel("a-chat-full-end", 700., 400., -90_000.),
        step("a-chat-full-end-check-quiet", 600, |view, _| {
            eprintln!(
                "tour: chat scroll offset {:?} of max {:?}, viewport {:?}",
                view.chat_scroll.offset().y,
                view.chat_scroll.max_offset().height,
                view.chat_scroll.bounds().size.height
            );
        }),
        wheel("a-chat-full-end-again", 700., 400., -90_000.),
        step("a-chat-full-end-check-2-quiet", 600, |view, _| {
            eprintln!(
                "tour: chat scroll offset {:?} of max {:?}; column {:?}; turns: {}",
                view.chat_scroll.offset().y,
                view.chat_scroll.max_offset().height,
                view.chat_scroll.bounds_for_item(0).map(|bounds| bounds.size),
                crate::lazy::describe("chat-turns")
            );
        }),
        step("a-chat-queued", 900, |view, cx| {
            view.chat_to_bottom();
            view.chat_queue = vec![
                "A queued question gjpqy".into(),
                "A second queued question that is long enough to need truncating or wrapping in the queue above the composer, gjpqy".into(),
            ];
            cx.notify();
        }),
        step("a-chat-history", 900, |view, cx| {
            view.chat_queue.clear();
            view.chat_history_open = true;
            cx.notify();
        }),
        step("a-chat-empty", 900, |view, cx| {
            view.chat_history_open = false;
            view.clear_chat(cx);
        }),
        open("a-transcript-long-note", "Two thousand line"),
        wheel("a-transcript-long-note-2", 600., 400., -3000.),
        step("a-transcript-long", 1200, |view, _| {
            view.transcript_open = true;
            view.transcript_epoch = view.transcript_epoch.wrapping_add(1);
        }),
        wheel("a-transcript-long-up", 400., 420., 2000.),
        step("a-transcript-find", 900, |view, _| view.transcript_find_open = true),
        step("a-settings-preferences", 900, |view, cx| {
            view.transcript_open = false;
            view.open_settings(crate::screens::SettingsTab::Preferences, cx)
        }),
        wheel("a-settings-preferences-end", 700., 400., -9000.),
        step("a-settings-profile", 900, |view, cx| {
            view.open_settings(crate::screens::SettingsTab::Profile, cx)
        }),
        step("a-settings-calendar", 900, |view, cx| {
            view.open_settings(crate::screens::SettingsTab::Calendar, cx)
        }),
        wheel("a-settings-calendar-end", 700., 400., -9000.),
        step("a-settings-notifications", 900, |view, cx| {
            view.open_settings(crate::screens::SettingsTab::Notifications, cx)
        }),
        step("a-settings-about", 900, |view, cx| {
            view.open_settings(crate::screens::SettingsTab::About, cx)
        }),
        update("a-update-checking", "checking"),
        update("a-update-available", "available"),
        update("a-update-downloading", "downloading"),
        update("a-update-verifying", "verifying"),
        update("a-update-ready", "ready"),
        update("a-update-failed", "failed"),
        step("a-update-over-note", 900, |view, cx| {
            view.updates.review("available");
            view.navigate(Screen::Home, cx);
            if let Some(id) = titled(view, "Quarterly planning, budgeting") {
                view.open_note(id, cx);
            }
        }),
        // The notice over Home's list: the pointer rests on the notice,
        // where a row's "More actions" button lies underneath it.
        step("a-update-over-home-quiet", 900, |view, cx| {
            view.navigate(Screen::Home, cx);
        }),
        input("a-update-over-home-scrolled-quiet", 700, |window, cx| {
            scroll(window, cx, 700., 400., -400.)
        }),
        input("a-update-over-home-pointer-quiet", 900, |window, cx| {
            let size = window.viewport_size();
            pointer(
                window,
                cx,
                f32::from(size.width) - 216.,
                f32::from(size.height) - 46.,
            )
        }),
        input("a-update-over-home-hover", 1200, |window, cx| {
            let size = window.viewport_size();
            pointer(
                window,
                cx,
                f32::from(size.width) - 215.,
                f32::from(size.height) - 46.,
            )
        }),
        // The sign-in form in each of its states (nothing is asked of any
        // server; the session is left alone).
        step("a-account-password", 900, |view, cx| {
            view.updates.review("idle");
            view.navigate(Screen::Account, cx);
            view.account.update(cx, |account, cx| account.review("password", cx));
        }),
        step("a-account-google", 900, |view, cx| {
            view.account.update(cx, |account, cx| account.review("google", cx));
        }),
        step("a-account-both", 900, |view, cx| {
            view.account.update(cx, |account, cx| account.review("both", cx));
        }),
        step("a-account-waiting", 900, |view, cx| {
            view.account.update(cx, |account, cx| account.review("waiting", cx));
        }),
        step("a-account-checking", 900, |view, cx| {
            view.account.update(cx, |account, cx| account.review("checking", cx));
        }),
        step("a-account-unreachable", 900, |view, cx| {
            view.account.update(cx, |account, cx| account.review("unreachable", cx));
        }),
        step("a-account-wrong-password", 900, |view, cx| {
            view.account.update(cx, |account, cx| account.review("wrong-password", cx));
        }),
        step("a-account-http-private", 900, |view, cx| {
            view.account.update(cx, |account, cx| account.review("http-private", cx));
        }),
        step("a-account-http-public", 900, |view, cx| {
            view.account.update(cx, |account, cx| account.review("http-public", cx));
        }),
        // Links (`CEREAL_AUDIT_LINKS`, separated by spaces): a shared note,
        // a second one, one that has been revoked, a shared note on another
        // server and a note of one's own on another server.
        step("a-link-shared", 2500, |view, cx| {
            view.navigate(Screen::Home, cx);
            follow(view, 0, cx);
        }),
        wheel("a-link-shared-2", 600., 400., -560.),
        wheel("a-link-shared-end", 600., 400., -90_000.),
        step("a-link-shared-second", 2500, |view, cx| follow(view, 1, cx)),
        step("a-link-unavailable", 2500, |view, cx| follow(view, 2, cx)),
        step("a-link-other-server", 1500, |view, cx| follow(view, 3, cx)),
        step("a-link-note-other-server", 1500, |view, cx| follow(view, 4, cx)),
        step("a-shared-entries", 900, |view, cx| view.navigate(Screen::Shared, cx)),
        input("a-shared-entries-hover", 900, |window, cx| {
            let width = f32::from(window.viewport_size().width);
            pointer(window, cx, (width + 192.) / 2., 188.)
        }),
        // A meeting on the calendar that has not been recorded: the control
        // says when it starts, then that it has started.
        open("a-event-note-quiet", "🚀🚀 Launch"),
        step("a-event-note-starts", 900, |view, _| meeting(view, 25)),
        step("a-event-note-starts-open", 900, |view, _| {
            view.transcript_open = true;
            view.transcript_epoch = view.transcript_epoch.wrapping_add(1);
        }),
        step("a-event-note-started", 900, |view, _| {
            view.transcript_open = false;
            view.transcript_epoch = view.transcript_epoch.wrapping_add(1);
            meeting(view, -5)
        }),
        step("a-event-note-details", 900, |view, _| {
            view.menu = Some(Menu::NoteDetails);
        }),
        // A tooltip that is up when the app stops being the active one.
        open("a-tooltip-setup-quiet", "Two thousand line"),
        step("a-tooltip-panel-quiet", 1200, |view, _| {
            view.menu = None;
            view.transcript_open = true;
            view.transcript_epoch = view.transcript_epoch.wrapping_add(1);
        }),
        input("a-tooltip-hover-quiet", 1200, |window, cx| {
            let size = window.viewport_size();
            pointer(window, cx, f32::from(size.width) / 2. - 150., f32::from(size.height) - 160.)
        }),
        input("a-tooltip-shown", 600, |window, cx| {
            let size = window.viewport_size();
            pointer(window, cx, f32::from(size.width) / 2. - 149., f32::from(size.height) - 160.)
        }),
        step("a-tooltip-app-inactive", 1500, |_, _| {
            let _ = std::process::Command::new("/usr/bin/osascript")
                .args(["-e", "tell application \"Finder\" to activate"])
                .status();
        }),
        step("a-tooltip-app-active-again", 900, |_, cx| cx.activate(true)),
        // The app menu's commands with the main window closed.
        windowed("a-window-closed-quiet", 800, |view, window, cx| {
            view.transcript_open = false;
            view.navigate(Screen::Home, cx);
            crate::pill::hide_main_window(window, cx);
        }),
        input("a-window-closed-new-note", 1500, |_, cx| {
            let available = cx.is_action_available(&crate::app::NewNote);
            eprintln!("tour: window closed, New Note available: {available}");
            // As the menu sends it: from outside any window's own update.
            cx.defer(|cx| cx.dispatch_action(&crate::app::Search));
        }),
        step("a-window-closed-result", 900, |view, _| {
            eprintln!("tour: after Search Notes with the window closed, search open: {}", view.search_open);
            view.search_open = false;
        }),
        step("a-toast", 600, |view, cx| {
            view.updates.review("idle");
            view.navigate(Screen::Home, cx);
            view.show_toast("Note copied to clipboard", cx)
        }),
        step("a-toast-long", 600, |view, cx| {
            view.show_toast("The server could not be reached at http://127.0.0.1:3000 — check that it is running and that this Mac can reach it, gjpqy", cx)
        }),
    ]);
    steps
}

/// Render accounting on the signed-in account's own content, `c-…` (run
/// with `CEREAL_AUDIT=1` and `CEREAL_TOUR=c-`): two seconds of steady
/// scrolling through Home with every note the account has, a very long
/// note, a very long transcript and a long conversation; a long note opened
/// and closed twenty times, with the process's resident size before and
/// after.
fn audit_bench_steps() -> Vec<Step> {
    fn titled(view: &Cereal, prefix: &str) -> Option<u64> {
        view.workspace
            .notes
            .iter()
            .find(|note| !note.trashed && note.title.starts_with(prefix))
            .map(|note| note.id)
    }
    fn wheels(steps: &mut Vec<Step>, x: f32, y: f32, by: f32) {
        for _ in 0..40 {
            steps.push((
                "c-wheel",
                50,
                Action::Input(Box::new(move |window, cx| scroll(window, cx, x, y, by))),
            ));
        }
    }
    fn resident() -> String {
        std::process::Command::new("/bin/ps")
            .args(["-o", "rss=", "-p", &std::process::id().to_string()])
            .output()
            .ok()
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
            .unwrap_or_default()
    }
    const LONG: &str = "Two thousand line";
    let mut steps = vec![step("c-home", 1500, |view, cx| view.navigate(Screen::Home, cx))];
    wheels(&mut steps, 700., 500., -90.);
    steps.push(step("c-scroll-home-2s", 600, |_, _| {}));
    wheels(&mut steps, 700., 500., 90.);
    steps.push(step("c-scroll-home-back-2s", 600, |_, _| {}));
    steps.push(step("c-long-note", 2000, |view, cx| {
        if let Some(id) = titled(view, LONG) {
            view.open_note(id, cx);
        }
    }));
    wheels(&mut steps, 600., 400., -90.);
    steps.push(step("c-scroll-long-note-2s", 600, |_, _| {}));
    steps.push(step("c-transcript", 2000, |view, _| {
        view.transcript_open = true;
        view.transcript_epoch = view.transcript_epoch.wrapping_add(1);
    }));
    wheels(&mut steps, 600., 500., 90.);
    steps.push(step("c-scroll-transcript-up-2s", 600, |_, _| {}));
    steps.push(step("c-chat", 2000, |view, cx| {
        view.transcript_open = false;
        view.navigate(Screen::Chat, cx);
        view.chat_messages.clear();
        for turn in 0..12 {
            view.chat_messages.push((
                format!("Question {turn}: what did we decide about the launch?"),
                "Here is the plan.\n\n## Owners\n\n| Owner | Task | Due |\n| --- | --- | --- |\n| Sam | Missing key banner | Wed |\n| Priya | iPhone upload | Wed noon |\n\n```rust\nfn main() {\n    println!(\"launch\");\n}\n```\n\n1. First step\n2. Second step\n   - Nested point that is long enough to wrap onto a second line in the column at any width of the window\n\nThe launch stays on Thursday. Desktop and web are the commitment and the phone follows if review is slow.".into(),
            ));
        }
        cx.notify();
    }));
    wheels(&mut steps, 700., 400., 90.);
    steps.push(step("c-scroll-chat-up-2s", 600, |_, _| {}));
    steps.push(step("c-memory-before", 1500, |view, cx| {
        view.clear_chat(cx);
        view.navigate(Screen::Home, cx);
        eprintln!("tour: resident before {} KB", resident());
    }));
    for _ in 0..20 {
        steps.push(step("c-open", 700, |view, cx| {
            if let Some(id) = titled(view, LONG) {
                view.open_note(id, cx);
                view.transcript_open = true;
                view.transcript_epoch = view.transcript_epoch.wrapping_add(1);
            }
        }));
        steps.push(step("c-open", 500, |view, cx| view.navigate(Screen::Home, cx)));
    }
    steps.push(step("c-memory-after-20-opens", 3000, |_, _| {
        eprintln!("tour: resident after {} KB", resident());
    }));
    // Twenty more: what the first twenty added was either kept for good
    // (caches that fill once) or is added again each time (a leak).
    for _ in 0..20 {
        steps.push(step("c-open", 700, |view, cx| {
            if let Some(id) = titled(view, LONG) {
                view.open_note(id, cx);
                view.transcript_open = true;
                view.transcript_epoch = view.transcript_epoch.wrapping_add(1);
            }
        }));
        steps.push(step("c-open", 500, |view, cx| view.navigate(Screen::Home, cx)));
    }
    steps.push(step("c-memory-after-40-opens", 3000, |_, _| {
        eprintln!("tour: resident after forty {} KB", resident());
    }));
    steps
}

/// The pill's popup opening under the pointer and folding away again,
/// `d-pill-…` (run with `--view=pill`): the same events AppKit's tracking
/// areas send, so the real pointer is not used.
fn pill_steps() -> Vec<Step> {
    vec![
        step("d-pill-rest", 1500, |_, _| {}),
        input("d-pill-hover", 1400, |_, cx| {
            cx.defer(|cx| crate::pill::smoke_pointer_move(point(px(21.), px(34.)), cx));
        }),
        input("d-pill-leave", 1400, |_, cx| {
            cx.defer(|cx| {
                crate::pill::smoke_pointer_event(
                    false,
                    PlatformInput::MouseExited(MouseExitEvent::default()),
                    cx,
                )
            });
        }),
        step("d-pill-closed", 1200, |_, _| {}),
        input("d-pill-hover-again", 1400, |_, cx| {
            cx.defer(|cx| crate::pill::smoke_pointer_move(point(px(21.), px(34.)), cx));
        }),
        step("d-pill-loud", 900, |view, _| view.recording.set_audio_levels(0.85, 0.2)),
        step("d-pill-quiet", 900, |view, _| view.recording.set_audio_levels(0.0, 0.0)),
        step("d-pill-attention", 1200, |view, _| {
            view.recording
                .set_transcription_attention(crate::recording::TranscriptionAttention::Warning);
        }),
        input("d-pill-leave-again", 1400, |_, cx| {
            cx.defer(|cx| {
                crate::pill::smoke_pointer_event(
                    false,
                    PlatformInput::MouseExited(MouseExitEvent::default()),
                    cx,
                )
            });
        }),
        step("d-pill-end", 1200, |_, _| {}),
    ]
}

/// Two minutes with the main window closed, `e-…`, for an outside tool to
/// sample the process (run with `CEREAL_AUDIT=1` for the signed-in app).
fn closed_window_steps() -> Vec<Step> {
    let mut steps = Vec::new();
    steps.push(windowed("e-window-closed-setup", 5000, |view, window, cx| {
        view.navigate(Screen::Home, cx);
        crate::pill::hide_main_window(window, cx);
        eprintln!("tour: window closed");
    }));
    steps.push(step("e-window-closed-120s", 122_000, |_, _| {}));
    steps
}

/// The transcript panel's stills, `p-tx-…` (run with `CEREAL_TOUR=p-tx`):
/// a meeting with several voices on each channel, the panel at rest, a
/// bubble under the pointer, and a passage opened from a source citation,
/// then the same in the dark theme. `CEREAL_TOUR_HOVER=x,y` says where the
/// pointer rests for `p-tx-hover`, `CEREAL_TOUR_CITE=<row>` which row the
/// citation opens. Entirely fictional text.
fn transcript_steps() -> Vec<Step> {
    use meeting_notes_api_types::{AudioChannel, TranscriptSegmentDTO};
    const LINES: &[(bool, u32, &str)] = &[
        (false, 2, "Okay, I think everyone is here, so let's get going."),
        (false, 2, "First thing is the onboarding numbers from last week."),
        (true, 1, "Sure. Completion went from sixty-one to seventy-four percent."),
        (true, 1, "Most of that is the shorter calendar step."),
        (false, 3, "Is that across both platforms?"),
        (true, 1, "Desktop only for now. Mobile ships on Thursday."),
        (false, 2, "Got it."),
        (false, 2, "And the drop-off at the permissions screen, did that move at all?"),
        (true, 1, "A little. We still lose about one in eight people there, mostly when the microphone prompt and the screen recording prompt arrive back to back."),
        (false, 3, "Could we ask for the screen recording later, when someone first shares a screen?"),
        (true, 1, "That's the plan."),
        (true, 1, "Theo has a prototype that defers it until the first meeting."),
        (true, 4, "Yes, I can share it after this call."),
        (true, 4, "It needs one more pass on the copy."),
        (false, 2, "Great. Let's keep onboarding to three steps and review the prototype on Monday."),
        (true, 1, "Works for me."),
        (false, 3, "One more thing on search: the calendar results are still slow."),
        (false, 3, "It's around two seconds on a cold start."),
        (true, 1, "We're caching the event list now, so it should be under half a second next week."),
        (false, 2, "Perfect."),
        (false, 2, "Anything else before we wrap up?"),
        (true, 1, "Just the budget review, but that can wait until the launch."),
        (false, 3, "Agreed."),
        (false, 2, "Okay, thanks everyone. Talk Monday."),
    ];
    /// The passage the source citation opens.
    const CITED: usize = 14;
    fn segment(index: usize) -> TranscriptSegmentDTO {
        let (own, speaker, text) = LINES[index];
        let name = if own { "mic" } else { "system" };
        let start_ms = 4_000 + index as i64 * 3_700;
        TranscriptSegmentDTO {
            channel: if own { AudioChannel::Mic } else { AudioChannel::System },
            end_ms: Some(start_ms + 3_200),
            id: format!("33333333-0000-4000-8000-{index:012x}"),
            is_final: true,
            speaker_id: format!("{name}:n:{speaker}"),
            speaker_label: Some(format!("Speaker {speaker}")),
            start_ms,
            text: text.into(),
        }
    }
    fn open(view: &mut Cereal, cx: &mut Context<Cereal>, dark: bool) {
        view.workspace.settings.force_dark(dark);
        view.navigate(Screen::Home, cx);
        view.open_note(1, cx);
        view.active_tab = NoteTab::Enhanced;
        if let Some(note) = view.workspace.note_mut(1) {
            note.replace_transcript("");
            note.server_transcript_segments = (0..LINES.len()).map(segment).collect();
            note.transcript_revision = note.transcript_revision.saturating_add(1);
            // The notes were written from this transcript: no notice.
            note.generated_revision = Some(note.transcript_revision);
        }
        view.summary.transcript_selection = None;
        view.transcript_open = true;
        view.transcript_epoch = view.transcript_epoch.wrapping_add(1);
    }
    fn hover(window: &mut Window, cx: &mut App) {
        let (x, y) = std::env::var("CEREAL_TOUR_HOVER")
            .ok()
            .and_then(|at| {
                let (x, y) = at.split_once(',')?;
                Some((x.parse().ok()?, y.parse().ok()?))
            })
            .unwrap_or((860., 700.));
        pointer(window, cx, x, y);
    }
    fn cite(view: &mut Cereal, window: &mut Window, cx: &mut Context<Cereal>) {
        // `CEREAL_TOUR_CITE=<row>` cites another row (12: a gold bubble with
        // another gold one right under it).
        let cited = std::env::var("CEREAL_TOUR_CITE")
            .ok()
            .and_then(|row| row.parse::<usize>().ok())
            .filter(|row| *row < LINES.len())
            .unwrap_or(CITED);
        let segment = segment(cited);
        view.jump_to_transcript_quote(1, Some(&segment.id), &segment.text, None, window, cx);
    }
    let mut steps = Vec::new();
    for dark in [false, true] {
        let names = if dark {
            ["p-tx-dark-normal", "p-tx-dark-hover", "p-tx-dark-away", "p-tx-dark-highlight"]
        } else {
            ["p-tx-normal", "p-tx-hover", "p-tx-away", "p-tx-highlight"]
        };
        steps.push(step(names[0], 3000, move |view, cx| open(view, cx, dark)));
        steps.push(input(names[1], 3000, hover));
        steps.push(input(names[2], 1500, |window, cx| pointer(window, cx, 20., 300.)));
        steps.push(windowed(names[3], 3000, cite));
    }
    steps.push(step("p-tx-end", 600, |view, cx| {
        view.workspace.settings.force_dark(false);
        view.transcript_open = false;
        view.navigate(Screen::Home, cx);
    }));
    steps
}

/// Wait until the window has stopped rendering, so a still state is captured
/// at rest and never mid-transition. Gives up after a few seconds: a state
/// with a deliberate loop (a spinner) never goes quiet.
async fn settle(cx: &mut AsyncApp) {
    for _ in 0..80 {
        cx.background_executor()
            .timer(Duration::from_millis(50))
            .await;
        if crate::motion::stats::quiet_for() >= Duration::from_millis(300) {
            return;
        }
    }
}

pub fn run(handle: WindowHandle<Cereal>, cx: &mut App) {
    cx.spawn(async move |cx| {
        // Let the first frames, fonts and fixtures settle before starting.
        cx.background_executor()
            .timer(Duration::from_millis(1500))
            .await;
        // `CEREAL_TOUR=menu,toast` runs only the steps whose names start with
        // one of the given prefixes.
        let only: Vec<String> = std::env::var("CEREAL_TOUR")
            .map(|value| {
                value
                    .split(',')
                    .filter(|prefix| !prefix.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        let all = steps()
            .into_iter()
            .chain(still_steps())
            .chain(live_still_steps())
            .chain(transcript_panel_steps())
            .chain(bench_steps())
            .chain(live_bench_steps())
            .chain(first_frame_steps())
            .chain(keyboard_steps())
            .chain(polish_steps())
            .chain(gap_still_steps())
            .chain(bar_steps())
            .chain(gold_steps())
            .chain(icon_steps())
            .chain(chat_parity_steps())
            .chain(thinking_steps())
            .chain(surface_steps())
            .chain(note_polish_steps())
            .chain(ask_bar_steps())
            .chain(sidebar_steps())
            .chain(idle_steps())
            .chain(update_film_steps())
            .chain(notes_generation_steps())
            .chain(e2e_steps())
            .chain(org_e2e_steps());
        // `CEREAL_TOUR_SIZE=760x580` resizes the window first, and
        // `CEREAL_TOUR_SHOTS=<directory>` holds each still (`p-…`, `a-…`)
        // until `<directory>/<name>.png` exists: an outside tool takes the
        // picture when it sees `tour … shot <name>` on stderr.
        if let Some((width, height)) = std::env::var("CEREAL_TOUR_SIZE").ok().and_then(|size| {
            let (width, height) = size.split_once('x')?;
            Some((width.parse::<f32>().ok()?, height.parse::<f32>().ok()?))
        }) {
            let _ = handle.update(cx, |_, window, _| window.resize(size(px(width), px(height))));
        }
        let shots = std::env::var_os("CEREAL_TOUR_SHOTS").map(std::path::PathBuf::from);
        // Without `--motion-tour` the tour runs against the server the data
        // directory is signed in to: wait for its notes before starting.
        if crate::server::enabled() {
            for _ in 0..600 {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                let ready = handle
                    .update(cx, |view, _, _| {
                        crate::accounts::any_shown()
                            && view.server_pending == 0
                            && !view.workspace.notes.is_empty()
                    })
                    .unwrap_or(true);
                if ready {
                    break;
                }
            }
        }
        settle(cx).await;
        crate::motion::stats::report("launch");
        for (name, hold, action) in all
            .chain(audit_steps())
            .chain(audit_bench_steps())
            .chain(pill_steps())
            .chain(closed_window_steps())
            .chain(transcript_steps())
        {
            if only.is_empty()
                && ["p-", "b-", "i-", "a-", "c-", "d-", "e-", "k-", "m-", "f-"]
                    .iter()
                    .any(|kind| name.starts_with(kind))
            {
                continue;
            }
            if !only.is_empty() && !only.iter().any(|prefix| name.starts_with(prefix.as_str())) {
                continue;
            }
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_millis());
            eprintln!("tour {stamp} {name}");
            crate::motion::stats::step_began();
            let ran = match action {
                Action::View(action) => handle.update(cx, |view, _, cx| {
                    action(view, cx);
                    cx.notify();
                }),
                Action::Window(action) => handle.update(cx, |view, window, cx| {
                    action(view, window, cx);
                }),
                Action::Input(action) => {
                    cx.update_window(handle.into(), |_, window, cx| action(window, cx))
                }
            };
            if ran.is_err() {
                return;
            }
            // Still states are held only once the window has come to rest.
            if name.starts_with("p-") || name.starts_with("a-") {
                settle(cx).await;
                if let Some(directory) = shots.as_ref().filter(|_| !name.ends_with("-quiet")) {
                    eprintln!("tour {stamp} shot {name}");
                    let picture = directory.join(format!("{name}.png"));
                    for _ in 0..600 {
                        if picture.exists() {
                            break;
                        }
                        cx.background_executor()
                            .timer(Duration::from_millis(50))
                            .await;
                    }
                    cx.background_executor()
                        .timer(Duration::from_millis(150))
                        .await;
                    crate::motion::stats::report(name);
                    continue;
                }
            }
            cx.background_executor()
                .timer(slow(Duration::from_millis(hold)))
                .await;
            if !matches!(
                name,
                "b-key"
                    | "b-level"
                    | "b-sweep"
                    | "b-wheel"
                    | "b-live-update"
                    | "b-live-wheel"
                    | "b-first-wheel"
                    | "c-wheel"
                    | "c-open"
            ) {
                crate::motion::stats::report(name);
            }
        }
        eprintln!("tour done");
        let _ = cx.update(|cx| cx.quit());
    })
    .detach();
}

/// `CEREAL_CAPTURE_PROBE=<seconds>`, without `--motion-tour` so the capture
/// is real: record into a new note in the isolated data directory for that
/// long, printing the render accounting once a second for the main window
/// and the pill, then stop and quit. It refuses to run when a server is
/// configured, so nothing can leave the machine.
pub fn capture_probe(handle: WindowHandle<Cereal>, seconds: u64, cx: &mut App) {
    cx.spawn(async move |cx| {
        let second = Duration::from_secs(1);
        cx.background_executor()
            .timer(Duration::from_millis(1500))
            .await;
        if crate::server::enabled() {
            eprintln!("probe refused: a server is configured; this only runs in local mode");
            let _ = cx.update(|cx| cx.quit());
            return;
        }
        crate::motion::stats::report("probe-launch");
        let started = handle.update(cx, |view, window, cx| {
            view.new_note(window, cx);
            let id = view.current_note;
            if let Some(id) = id {
                view.start_recording_preview(id, cx);
            }
            id
        });
        let Ok(Some(_)) = started else {
            eprintln!("probe: could not start a note");
            let _ = cx.update(|cx| cx.quit());
            return;
        };
        for index in 0..seconds {
            cx.background_executor().timer(second).await;
            let state = handle.update(cx, |view, _, _| {
                format!(
                    "phase {:?}, level {:.3}, error {:?}",
                    view.recording.phase,
                    view.recording.audio_level(),
                    view.audio_error
                )
            });
            eprintln!("probe second {}: {}", index + 1, state.unwrap_or_default());
            crate::motion::stats::report("probe-recording");
        }
        let _ = handle.update(cx, |view, _, cx| view.stop_recording_preview(cx));
        cx.background_executor().timer(second * 2).await;
        crate::motion::stats::report("probe-stopped");
        let _ = handle.update(cx, |view, _, cx| {
            view.flush_all_blocking(cx);
        });
        eprintln!("probe done");
        let _ = cx.update(|cx| cx.quit());
    })
    .detach();
}

/// `CEREAL_FUNCTIONAL_LOG=1` (with an isolated `CEREAL_DATA_DIR`): the app is
/// used as usual, by a person or by a tool that posts real pointer and
/// keyboard events, and this prints one line whenever something it shows
/// changes: `fx <milliseconds since 1970> <what>: <value>`. It reads state
/// and never changes any, so what a functional test measures is the app.
pub fn functional_log(handle: WindowHandle<Cereal>, cx: &mut App) {
    cx.spawn(async move |cx| {
        let mut last: std::collections::BTreeMap<&'static str, String> = Default::default();
        let mut known: std::collections::HashMap<String, String> = Default::default();
        // Per chat turn: the length of its answer and whether it has finished.
        let mut turns: Vec<(usize, bool)> = Vec::new();
        let mut last_thread = String::new();
        let mut level = (0f32, 0f32, std::time::Instant::now());
        loop {
            cx.background_executor().timer(Duration::from_millis(25)).await;
            let stamp = || {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |since| since.as_millis())
            };
            let pill = cx.update(crate::pill::describe).unwrap_or_default();
            let Ok((values, events)) = handle.update(cx, |view, _, cx| {
                let mut events = Vec::new();
                let note = view.current_note.and_then(|id| view.workspace.note(id));
                let recorded = view.recording.note_id.and_then(|id| view.workspace.note(id));
                let mut values: Vec<(&'static str, String)> = vec![
                    (
                        "screen",
                        format!(
                            "{:?}, note {:?} ({:?}), tab {:?}, transcript open {}, chat open {}, search open {}, menu {:?}, model picker open {}, chat history open {}",
                            view.screen,
                            view.current_note,
                            note.map(|note| note.title.as_str()),
                            view.active_tab,
                            view.transcript_open,
                            view.assistant_open,
                            view.search_open,
                            view.menu,
                            view.chat_model_picker_open,
                            view.chat_history_open
                        ),
                    ),
                    (
                        "recording",
                        format!(
                            "{:?} for note {:?}, clock {}, attention {:?}",
                            view.recording.phase,
                            view.recording.note_id,
                            crate::recording::elapsed_label(view.recording.elapsed()),
                            view.recording.transcription_attention
                        ),
                    ),
                    (
                        "live",
                        format!(
                            "{}, status {:?}",
                            view.live.describe(),
                            view.recording.note_id.and_then(|id| view.live.status(id)).map(|status| status.text)
                        ),
                    ),
                    (
                        "speaking",
                        recorded
                            .map(|note| {
                                view.live
                                    .tail(note)
                                    .iter()
                                    .map(|row| format!("{} [{}]", row.text, row.key))
                                    .collect::<Vec<_>>()
                                    .join(" | ")
                            })
                            .unwrap_or_default(),
                    ),
                    (
                        "highlight",
                        view.summary
                            .transcript_selection
                            .as_ref()
                            .map_or("none".into(), |selection| format!("{:?}", selection.text())),
                    ),
                    ("model", format!("{:?} of {} models", view.selected_chat_model, view.chat_models.len())),
                    (
                        "generation",
                        format!(
                            "{:?}; server pending {}, status {:?}; enhanced {} characters",
                            view.generation.phase,
                            view.server_pending,
                            view.server_status,
                            note.map_or(0, |note| note.enhanced.len())
                        ),
                    ),
                    ("toast", format!("{:?}", view.toast)),
                    (
                        "chat",
                        format!(
                            "conversation {} with {} turns; answer arriving on screen {}; queued {:?}; arriving off screen {:?}; composer {:?}",
                            view.chat_thread_id.chars().take(8).collect::<String>(),
                            view.chat_messages.len(),
                            view.chat_stream.is_some(),
                            view.chat_queue,
                            {
                                let mut parked: Vec<String> = view
                                    .chat_parked
                                    .iter()
                                    .map(|(thread, parked)| {
                                        let length = view
                                            .chat_archives
                                            .iter()
                                            .find(|chat| &chat.id == thread)
                                            .and_then(|chat| chat.messages.get(parked.active.index))
                                            .map_or(0, |(_, answer)| answer.len());
                                        format!(
                                            "{} turn {} has {length} characters, {} queued",
                                            thread.chars().take(8).collect::<String>(),
                                            parked.active.index,
                                            parked.queue.len()
                                        )
                                    })
                                    .collect();
                                parked.sort();
                                parked
                            },
                            view.ask.read(cx).content()
                        ),
                    ),
                ];
                // The level the bars are drawn from: its range over each half second.
                let now = view.recording.audio_level();
                level.0 = level.0.min(now);
                level.1 = level.1.max(now);
                if level.2.elapsed() >= Duration::from_millis(500) {
                    values.push(("level", format!("{:.2} to {:.2}", level.0, level.1)));
                    level = (now, now, std::time::Instant::now());
                }
                for segment in recorded.iter().flat_map(|note| &note.server_transcript_segments) {
                    match known.get(&segment.id) {
                        None => events.push(format!(
                            "stored {:?} {} at {} ms: {}",
                            segment.channel, segment.speaker_id, segment.start_ms, segment.text
                        )),
                        Some(speaker) if *speaker != segment.speaker_id => events.push(format!(
                            "relabelled the segment at {} ms: {speaker} is now {}",
                            segment.start_ms, segment.speaker_id
                        )),
                        Some(_) => continue,
                    }
                    known.insert(segment.id.clone(), segment.speaker_id.clone());
                }
                // Chat: a question appears, its answer's first text, its end.
                if last_thread != view.chat_thread_id {
                    last_thread = view.chat_thread_id.clone();
                    turns.clear();
                    events.push("chat: another conversation is on screen".into());
                }
                if view.chat_messages.len() < turns.len() {
                    turns.clear();
                    events.push("chat cleared".into());
                }
                for (index, (question, answer)) in view.chat_messages.iter().enumerate() {
                    if index >= turns.len() {
                        turns.push((0, false));
                        events.push(format!("chat turn {index} asked: {question}"));
                    }
                    let active = view.chat_stream.is_some() && index + 1 == view.chat_messages.len();
                    let (length, finished) = turns[index];
                    if length == 0 && !answer.is_empty() {
                        events.push(format!("chat turn {index} first text ({} characters)", answer.len()));
                    }
                    if !finished && !active && (!answer.is_empty() || view.chat_annotations.get(&index).is_some_and(|notes| notes.error.is_some())) {
                        let notes = view.chat_annotations.get(&index);
                        events.push(format!(
                            "chat turn {index} finished (error {:?}, interrupted {}): {}",
                            notes.and_then(|notes| notes.error.clone()),
                            view.chat_interrupted.contains(&index),
                            answer.replace('\n', " ⏎ ")
                        ));
                        for citation in notes.iter().flat_map(|notes| &notes.citations) {
                            events.push(format!(
                                "chat turn {index} citation {} {:?}: {:?}",
                                citation.id, citation.source, citation.quote
                            ));
                        }
                        turns[index].1 = true;
                    }
                    turns[index].0 = answer.len();
                }
                (values, events)
            }) else {
                return;
            };
            for event in events {
                eprintln!("fx {} {event}", stamp());
            }
            for (key, value) in values.into_iter().chain([("pill", pill)]) {
                if last.get(key) != Some(&value) {
                    eprintln!("fx {} {key}: {value}", stamp());
                    last.insert(key, value);
                }
            }
        }
    })
    .detach();
}

/// `CEREAL_LIVE_PROBE=<seconds>`, with a server configured, an isolated
/// `CEREAL_DATA_DIR` and `CEREAL_CAPTURE_FIXTURE=<wav>`: record a new note
/// from the fixture for that long through the app's real path (capture
/// files, the live transcript stream, the transcript views), printing twice
/// a second what the stream and the transcript hold; then stop, wait for the
/// durable upload, print the stored transcript and quit. This is how live
/// transcription is exercised end to end where there is no microphone.
/// `CEREAL_LIVE_PROBE_RESUME=<seconds>` then resumes the same note for that
/// long and stops again, to show what a second session of one note stores
/// (speaker ids and labels) and which names the transcript puts above it.
pub fn live_probe(handle: WindowHandle<Cereal>, seconds: u64, cx: &mut App) {
    let resume = crate::environment::var("CEREAL_LIVE_PROBE_RESUME")
        .ok()
        .and_then(|seconds| seconds.parse::<u64>().ok())
        .filter(|seconds| *seconds > 0);
    cx.spawn(async move |cx| {
        let tick = Duration::from_millis(500);
        let quit = |cx: &mut AsyncApp, reason: &str| {
            eprintln!("live probe: {reason}");
            let _ = cx.update(|cx| cx.quit());
        };
        if !crate::server::enabled() {
            return quit(cx, "refused: no server is configured");
        }
        // Signing in with the configured token takes a request or two.
        for _ in 0..40 {
            if crate::accounts::any_shown() {
                break;
            }
            cx.background_executor().timer(tick).await;
        }
        if !crate::accounts::any_shown() {
            return quit(cx, "refused: the server did not accept the configured token");
        }
        // What the probe is asked to do besides recording:
        //   CEREAL_LIVE_PROBE_TITLE     the new note's title
        //   CEREAL_LIVE_PROBE_PILL      keep the pill and its popup on screen
        //   CEREAL_LIVE_PROBE_ASK       a question to ask in the note's chat,
        //   CEREAL_LIVE_PROBE_ASK_AT    this many seconds into the recording;
        //                               the first transcript citation of the
        //                               answer is then opened
        //   CEREAL_LIVE_PROBE_SEVER_AT  cut the stream's socket at this time
        let setting = |name: &str| crate::environment::var(name).ok().filter(|value| !value.is_empty());
        let title = setting("CEREAL_LIVE_PROBE_TITLE");
        let ask = setting("CEREAL_LIVE_PROBE_ASK");
        let ask_at = setting("CEREAL_LIVE_PROBE_ASK_AT")
            .and_then(|at| at.parse::<f32>().ok())
            .unwrap_or(40.);
        let sever_at = setting("CEREAL_LIVE_PROBE_SEVER_AT").and_then(|at| at.parse::<f32>().ok());
        let began = std::time::Instant::now();
        let started = handle.update(cx, |view, window, cx| {
            match &title {
                // The steps of `new_note`, with the title in place before
                // the server first hears of the note.
                Some(title) => {
                    let Some(id) = view.create_note_without_event() else {
                        return None;
                    };
                    view.workspace.note_mut(id).unwrap().title = title.clone();
                    view.open_note(id, cx);
                    view.active_tab = NoteTab::Notes;
                    view.title_input
                        .update(cx, { let title = title.clone(); move |input, cx| input.set_content(title, cx) });
                    view.persist(cx);
                    view.recording_last_click = None;
                    view.recording_changed_at = None;
                    view.start_recording_preview(id, cx);
                    view.editor.read(cx).focus_handle.focus(window);
                }
                None => view.new_note(window, cx),
            }
            view.transcript_open = true;
            view.transcript_epoch = view.transcript_epoch.wrapping_add(1);
            view.current_note
        });
        let Ok(Some(id)) = started else {
            return quit(cx, "could not start a note");
        };
        // Sampled ten times a second; a line is printed when something
        // changed. `captured` is how much audio the capture file held at
        // that moment, which is what delays are measured against: a word
        // that ends at 3.0 s of the fixture and is on screen when the file
        // holds 4.2 s took 1.2 s to get there.
        let sample = Duration::from_millis(50);
        if setting("CEREAL_LIVE_PROBE_PILL").is_some() {
            let _ = cx.update(crate::pill::hold_open);
        }
        // Every stored segment seen so far, and the speaker it was under.
        let mut known: std::collections::HashMap<String, String> = Default::default();
        // The chat question: not asked yet, asked (turn index), answered
        // (when), citation handled.
        #[derive(Clone, Copy, PartialEq)]
        enum Asking {
            Waiting,
            Asked(usize),
            Answered(usize, std::time::Instant),
            Done,
        }
        let mut asking = if ask.is_some() { Asking::Waiting } else { Asking::Done };
        let (mut severed, mut highlighted) = (sever_at.is_none(), false);
        for (round, seconds) in std::iter::once(seconds).chain(resume).enumerate() {
        let round_began = std::time::Instant::now();
        if round > 0 {
            eprintln!("live probe: resuming the same note for {seconds} s");
            let _ = handle.update(cx, |view, _, cx| {
                view.recording_last_click = None;
                view.recording_changed_at = None;
                view.start_recording_preview(id, cx);
                view.transcript_open = true;
                view.transcript_epoch = view.transcript_epoch.wrapping_add(1);
            });
        }
        let (mut first_interim, mut first_final) = (None, None);
        // What an earlier round stored is not this round's first text.
        let already = handle
            .update(cx, |view, _, _| {
                view.workspace
                    .note(id)
                    .map_or(0, |note| note.server_transcript_segments.len())
            })
            .unwrap_or(0);
        let mut last = String::new();
        // By the clock, not by a count of samples: on a busy machine a
        // sample takes longer than its 50 ms.
        while round_began.elapsed() < Duration::from_secs(seconds) {
            cx.background_executor().timer(sample).await;
            let Ok((line, captured, events)) = handle.update(cx, |view, window, cx| {
                let captured = view
                    .audio_capture
                    .as_ref()
                    .map_or(0., |capture| capture.monitor().snapshot().audio_seconds());
                let stored = view
                    .workspace
                    .note(id)
                    .map(|note| note.server_transcript_segments.clone())
                    .unwrap_or_default();
                let mut events = Vec::new();
                // Segments that are new, that changed speaker (a merge), or
                // that are gone (retired by a later read of the note).
                for segment in &stored {
                    match known.get(&segment.id) {
                        None => {
                            events.push(format!(
                                "stored {:?} {} {:?} at {} ms: {}",
                                segment.channel,
                                segment.speaker_id,
                                segment.speaker_label,
                                segment.start_ms,
                                segment.text
                            ));
                        }
                        Some(speaker) if *speaker != segment.speaker_id => events.push(format!(
                            "relabelled the segment at {} ms: {speaker} is now {} {:?}",
                            segment.start_ms, segment.speaker_id, segment.speaker_label
                        )),
                        Some(_) => continue,
                    }
                    known.insert(segment.id.clone(), segment.speaker_id.clone());
                }
                if known.len() > stored.len() {
                    known.retain(|id, _| {
                        let kept = stored.iter().any(|segment| segment.id == *id);
                        if !kept {
                            events.push(format!("a stored segment was retired ({id})"));
                        }
                        kept
                    });
                }
                let elapsed = round_began.elapsed().as_secs_f32();
                if round == 0 && !severed && sever_at.is_some_and(|at| elapsed >= at) {
                    severed = true;
                    events.push(format!(
                        "cut the stream's socket (a stream was up: {})",
                        view.live.sever()
                    ));
                }
                if round == 0 {
                    match asking {
                        Asking::Waiting if elapsed >= ask_at => {
                            let question = ask.clone().unwrap_or_default();
                            view.ask
                                .update(cx, { let question = question.clone(); move |input, cx| input.set_content(question, cx) });
                            view.submit_chat(cx);
                            let turn = view.chat_messages.len().saturating_sub(1);
                            events.push(format!("asked the note's chat {question:?} (turn {turn})"));
                            asking = Asking::Asked(turn);
                        }
                        Asking::Asked(turn) if view.chat_stream.is_none() => {
                            let answer = view
                                .chat_messages
                                .get(turn)
                                .map(|(_, answer)| answer.clone())
                                .unwrap_or_default();
                            let notes = view.chat_annotations.get(&turn);
                            events.push(format!(
                                "chat answered (error {:?}): {answer}",
                                notes.and_then(|notes| notes.error.clone())
                            ));
                            for citation in notes.iter().flat_map(|notes| &notes.citations) {
                                events.push(format!(
                                    "citation {} {:?} segment {:?}: {:?}",
                                    citation.id, citation.source, citation.segment_id, citation.quote
                                ));
                            }
                            asking = Asking::Answered(turn, std::time::Instant::now());
                        }
                        // The answer is left on screen for a few seconds,
                        // then its first transcript citation is opened: the
                        // same call a click on the citation makes.
                        Asking::Answered(turn, at) if at.elapsed() >= Duration::from_secs(4) => {
                            let citation = view.chat_annotations.get(&turn).and_then(|notes| {
                                notes
                                    .citations
                                    .iter()
                                    .find(|citation| {
                                        citation.source
                                            == meeting_notes_api_types::ChatCitationDtoSource::Transcript
                                    })
                                    .cloned()
                            });
                            match citation {
                                Some(citation) => {
                                    view.open_chat_citation(&citation, window, cx);
                                    events.push(format!(
                                        "opened citation {}: transcript open {}, a passage selected {}",
                                        citation.id,
                                        view.transcript_open,
                                        view.summary.transcript_selection.is_some()
                                    ));
                                    highlighted = view.summary.transcript_selection.is_some();
                                }
                                None => {
                                    events.push("the answer cites no transcript passage".into());
                                    view.assistant_open = false;
                                    view.transcript_open = true;
                                    view.transcript_epoch = view.transcript_epoch.wrapping_add(1);
                                }
                            }
                            asking = Asking::Done;
                        }
                        _ => {}
                    }
                }
                if asking == Asking::Done
                    && highlighted != view.summary.transcript_selection.is_some()
                {
                    highlighted = !highlighted;
                    events.push(format!(
                        "the citation's highlight is {} ({} stored segments)",
                        if highlighted { "back" } else { "gone" },
                        stored.len()
                    ));
                }
                let speaking = view.workspace.note(id).map(|note| view.live.tail(note));
                let speaking: Vec<String> = speaking
                    .iter()
                    .flat_map(|rows| {
                        rows.iter().map(|row| {
                            format!("{} [{} / {:?}]", row.text, row.key, row.speaker)
                        })
                    })
                    .collect();
                if !speaking.is_empty() {
                    first_interim.get_or_insert(captured);
                }
                if stored.len() > already {
                    first_final.get_or_insert(captured);
                }
                let line = format!(
                    "recording {:?}, {}, status {:?}, {} stored, speaking {:?}",
                    view.recording.phase,
                    view.live.describe(),
                    view.live.status(id).map(|status| status.text),
                    stored.len(),
                    speaking,
                );
                (line, captured, events)
            }) else {
                return;
            };
            for event in events {
                eprintln!(
                    "live probe {:6.2} s, captured {captured:6.2} s: {event}",
                    began.elapsed().as_secs_f32()
                );
            }
            if line != last {
                eprintln!(
                    "live probe {:6.2} s, captured {captured:6.2} s: {line}",
                    began.elapsed().as_secs_f32()
                );
                last = line;
            }
        }
        eprintln!(
            "live probe: first interim text on screen when the capture held {:?} s of audio, first stored segment at {:?} s",
            first_interim, first_final
        );
        if round == 0 && ask.is_some() {
            let _ = handle.update(cx, |view, _, _| {
                eprintln!(
                    "live probe: at Stop the citation's passage is {} and the transcript is {}",
                    if view.summary.transcript_selection.is_some() { "still highlighted" } else { "not highlighted" },
                    if view.transcript_open { "open" } else { "closed" }
                );
            });
        }
        let _ = handle.update(cx, |view, _, cx| {
            view.recording_last_click = None;
            view.recording_changed_at = None;
            crate::server::stop_trace("Stop is pressed");
            view.stop_recording_preview(cx);
        });
        let stopped = std::time::Instant::now();
        // The durable upload and whatever the server still has to transcribe.
        for _ in 0..360 {
            cx.background_executor().timer(tick).await;
            let pending = handle
                .update(cx, |view, _, _| view.server_pending > 0 || view.live.streams(id))
                .unwrap_or(false);
            if !pending && stopped.elapsed() > Duration::from_secs(2) {
                break;
            }
        }
        let _ = handle.update(cx, |view, _, cx| {
            eprintln!(
                "live probe: upload finished {:.1} s after Stop; server status {:?}",
                stopped.elapsed().as_secs_f32(),
                view.server_status
            );
            if let Some(note) = view.workspace.note(id) {
                eprintln!(
                    "live probe: note {:?} revision {}, {} stored segments:",
                    note.server_id,
                    note.transcript_revision,
                    note.server_transcript_segments.len()
                );
                let rows = crate::note::server_transcript_utterances(
                    &note.server_transcript_segments,
                );
                let headings = crate::note::transcript_headings(&rows, &[]);
                for (segment, heading) in note
                    .server_transcript_segments
                    .iter()
                    .filter(|segment| !segment.text.trim().is_empty())
                    .zip(&headings)
                {
                    eprintln!(
                        "live probe:   {:>7} ms {:?} id {:?} label {:?}, heading shown {:?}: {}",
                        segment.start_ms,
                        segment.channel,
                        segment.speaker_id,
                        segment.speaker_label,
                        heading,
                        segment.text
                    );
                }
            }
            view.flush_all_blocking(cx);
        });
        }
        quit(cx, "done");
    })
    .detach();
}

/// `CEREAL_WELCOME_TOUR=<name>` (debug builds, with an isolated
/// `CEREAL_DATA_DIR`): the first-run welcome walks itself, for captures. It
/// presses the welcome's own buttons through the window's event listeners
/// and types into its fields, so the real pointer and keyboard are never
/// touched; nothing outside the welcome is driven.
///
/// * `flow`: Welcome to Home against `CEREAL_WELCOME_SERVER`, signing in
///   with the `email` and `password` of the JSON file `CEREAL_WELCOME_LOGIN`
///   names (never printed), at the pace of a person.
/// * `stills`: every state of every step, each held until
///   `CEREAL_WELCOME_SHOTS/<name>.png` exists (an outside tool takes the
///   picture when it sees `welcome … shot <name>` on stderr).
/// * `local`: the same walk without a server. The app no longer offers one;
///   the walk passes the server step through a debug-only hook.
/// * `leave-at-audio`: walks to the audio step and quits there, to check
///   that the next launch resumes on it.
/// * `report`: says which screen and step the app opened on, then quits.
/// * `http`: the address line for a loopback, a private and a public
///   plain-HTTP address, and a sign-in over `CEREAL_WELCOME_SERVER`.
/// * `signed-in`: signs in and shows the server step at rest afterwards.
/// * `audio-asking`: the audio step alone, held between pressing Allow
///   and the answer.
/// * `audio-real`: the audio step alone, with macOS's own answers (run it
///   without `CEREAL_SIMULATE_PERMISSIONS`); it presses nothing.
/// * `again`: from Home, Settings → About → "Show welcome again".
///
/// Permissions are never asked of macOS here: run it with
/// `CEREAL_SIMULATE_PERMISSIONS`. `CEREAL_WELCOME_SIZE=760x580` resizes the
/// window first. `CEREAL_WELCOME_CALENDAR=1` stands a made-up calendar in
/// for one connected in the browser, which a capture cannot do.
#[cfg(debug_assertions)]
pub fn welcome(handle: WindowHandle<Cereal>, name: String, cx: &mut App) {
    use crate::onboarding::{Channel, Permission, ServerOffer, Step};
    use crate::onboarding_view::placed;

    fn say(what: &str) {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_millis());
        eprintln!("welcome {stamp} {what}");
    }
    async fn pause(cx: &mut AsyncApp, millis: u64) {
        cx.background_executor()
            .timer(slow(Duration::from_millis(millis)))
            .await;
    }
    /// Move over the control called `name`, press it and let go.
    async fn press(handle: WindowHandle<Cereal>, cx: &mut AsyncApp, name: &'static str) -> bool {
        let Some(at) = placed(name).map(|bounds| bounds.center()) else {
            say(&format!("missing {name}"));
            return false;
        };
        let send = |cx: &mut AsyncApp, event: PlatformInput| {
            let _ = cx.update_window(handle.into(), |_, window, cx| {
                window.dispatch_event(event, cx);
            });
        };
        send(cx, PlatformInput::MouseMove(MouseMoveEvent { position: at, ..Default::default() }));
        pause(cx, 260).await;
        send(
            cx,
            PlatformInput::MouseDown(MouseDownEvent {
                button: MouseButton::Left,
                position: at,
                click_count: 1,
                ..Default::default()
            }),
        );
        pause(cx, 130).await;
        send(
            cx,
            PlatformInput::MouseUp(MouseUpEvent {
                button: MouseButton::Left,
                position: at,
                click_count: 1,
                ..Default::default()
            }),
        );
        say(&format!("pressed {name}"));
        pause(cx, 120).await;
        // The pointer goes away again, so no hover is left behind.
        send(
            cx,
            PlatformInput::MouseMove(MouseMoveEvent {
                position: point(px(-40.), px(-40.)),
                ..Default::default()
            }),
        );
        true
    }
    /// Type `text` into one of the account form's fields (0 address,
    /// 1 email, 2 password), a character at a time.
    async fn type_into(handle: WindowHandle<Cereal>, cx: &mut AsyncApp, field: usize, text: &str, pace: u64) {
        let mut typed = String::new();
        for character in text.chars() {
            typed.push(character);
            let so_far = typed.clone();
            let _ = handle.update(cx, |view, window, cx| {
                let input = {
                    let account = view.account.read(cx);
                    [account.url.clone(), account.email.clone(), account.password.clone()][field].clone()
                };
                input.read(cx).focus_handle.focus(window);
                input.update(cx, |input, cx| input.set_content(so_far, cx));
                cx.notify();
            });
            pause(cx, pace).await;
        }
    }
    /// Wait until `ready` says so, or `seconds` have passed.
    async fn until(
        handle: WindowHandle<Cereal>,
        cx: &mut AsyncApp,
        seconds: u64,
        ready: impl Fn(&mut Cereal, &mut Context<Cereal>) -> bool,
    ) -> bool {
        for _ in 0..seconds * 20 {
            if handle.update(cx, |view, _, cx| ready(view, cx)).unwrap_or(false) {
                return true;
            }
            cx.background_executor().timer(Duration::from_millis(50)).await;
        }
        false
    }
    /// Hold still for a picture called `name`, and say where things are.
    async fn shot(cx: &mut AsyncApp, name: &str) {
        pause(cx, 700).await;
        let rect = |name: &str| {
            placed(name).map_or("none".to_string(), |b| {
                format!(
                    "{:.1},{:.1} {:.1}x{:.1}",
                    f32::from(b.origin.x),
                    f32::from(b.origin.y),
                    f32::from(b.size.width),
                    f32::from(b.size.height)
                )
            })
        };
        say(&format!(
            "layout {name}: left {}; right {}; screen {}; continue {}",
            rect("welcome-left"),
            rect("welcome-right"),
            rect("welcome-screen"),
            rect("welcome-continue")
        ));
        say(&format!("shot {name}"));
        let Some(directory) = crate::environment::var_os("CEREAL_WELCOME_SHOTS") else {
            pause(cx, 900).await;
            return;
        };
        let picture = std::path::PathBuf::from(directory).join(format!("{name}.png"));
        for _ in 0..400 {
            if picture.exists() {
                break;
            }
            cx.background_executor().timer(Duration::from_millis(50)).await;
        }
        cx.background_executor().timer(Duration::from_millis(150)).await;
    }
    fn made_up_calendar(view: &mut Cereal) {
        let at = |hours: i64| (chrono::Local::now() + chrono::Duration::hours(hours)).to_rfc3339();
        let event = |id: &str, title: &str, color: &str, from: i64| {
            serde_json::json!({"id":id,"calendarId":"work","calendarName":"Work","color":color,"title":title,"startAt":at(from),"endAt":at(from + 1),"allDay":false,"startDate":"2026-11-26","endDate":"2026-11-26","timeZone":null,"location":null,"meetingUrl":null,"noteId":null})
        };
        view.calendars.sources = serde_json::from_value(serde_json::json!([{"id":"work","name":"Work","provider":"google","color":"#e0af2f","enabled":true,"sourceHost":"calendar.google.com","lastSyncedAt":null,"lastSyncError":null,"eventCount":3,"createdAt":"2026-11-12T00:00:00Z"}])).unwrap_or_default();
        view.calendars.events = serde_json::from_value(serde_json::json!([
            event("a", "Design review", "#e0af2f", 1),
            event("b", "Weekly team standup", "#87b6e8", 3),
            event("c", "Catch-up with Maya", "#9bd18a", 5),
            event("d", "Planning", "#e89b87", 24),
        ]))
        .unwrap_or_default();
        view.calendars.personal_linked = true;
        view.calendars.loaded = true;
        view.calendars.loading = false;
    }
    /// The account already has a calendar, so the step would open on it.
    /// Put it out of sight, and keep the step from looking, until the
    /// capture has shown what comes before.
    fn hide_calendar(view: &mut Cereal) {
        view.onboarding.calendar_hold = true;
        view.calendars.sources.clear();
        view.calendars.events.clear();
        view.calendars.personal_linked = false;
        view.calendars.loaded = true;
    }

    cx.spawn(async move |cx| {
        let setting = |name: &str| crate::environment::var(name).ok().filter(|value| !value.is_empty());
        let server = setting("CEREAL_WELCOME_SERVER").unwrap_or_else(|| "http://127.0.0.1:3000".into());
        let login: Option<(String, String)> = setting("CEREAL_WELCOME_LOGIN")
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .and_then(|value| Some((value.get("email")?.as_str()?.to_owned(), value.get("password")?.as_str()?.to_owned())));
        let made_up = setting("CEREAL_WELCOME_CALENDAR").is_some();
        if let Some((width, height)) = setting("CEREAL_WELCOME_SIZE").and_then(|size| {
            let (width, height) = size.split_once('x')?;
            Some((width.parse::<f32>().ok()?, height.parse::<f32>().ok()?))
        }) {
            let _ = handle.update(cx, |_, window, _| window.resize(size(px(width), px(height))));
        }
        let step = |wanted: Step| move |view: &mut Cereal, _: &mut Context<Cereal>| view.onboarding.step() == Some(wanted);
        let offered = |view: &mut Cereal, cx: &mut Context<Cereal>| matches!(view.onboarding_offer(cx), ServerOffer::Password { .. });
        let where_now = |view: &mut Cereal, _: &mut Window, _: &mut Context<Cereal>| format!("screen {:?}, step {:?}", view.screen, view.onboarding.step());
        let finish = async |cx: &mut AsyncApp| {
            crate::motion::stats::report("welcome");
            say("done");
            let _ = cx.update(|cx| cx.quit());
        };
        pause(cx, 900).await;
        say(&format!("opened on {}", handle.update(cx, where_now).unwrap_or_default()));
        crate::motion::stats::report("launch");
        // The walks that start from Home: an install without a server now
        // opens on the server step, so the harness closes the welcome.
        if matches!(name.as_str(), "again" | "menu-toggle") {
            let _ = handle.update(cx, |view, _, cx| {
                if view.screen == Screen::Welcome {
                    view.finish_onboarding(cx);
                }
            });
            pause(cx, 900).await;
        }
        match name.as_str() {
            "report" => {
                // What macOS says about each permission, read without asking.
                say(&format!(
                    "macOS says: microphone {:?}, system audio {:?}",
                    crate::audio::permission_status(0),
                    crate::audio::permission_status(1)
                ));
                pause(cx, 2500).await;
                shot(cx, "report").await;
            }
            "leave-at-audio" | "flow" | "local" => {
                let local = name == "local";
                // `CEREAL_WELCOME_RESTART=<ms>`: a recorder can only start once
                // the window exists, so the welcome is begun again from the
                // top when it has had time to, exactly as it begins at launch.
                if let Some(wait) = setting("CEREAL_WELCOME_RESTART") {
                    pause(cx, wait.parse::<u64>().unwrap_or(2600)).await;
                    let _ = handle.update(cx, |view, _, cx| {
                        view.begin_onboarding(crate::onboarding::Flow::START, false, cx)
                    });
                    say("restart");
                }
                pause(cx, 3600).await;
                crate::motion::stats::report("welcome");
                press(handle, cx, "welcome-continue").await;
                until(handle, cx, 5, step(Step::Server)).await;
                pause(cx, 1300).await;
                crate::motion::stats::report("to-server");
                if local {
                    // Tab walks the step's controls in the order they read.
                    for stop in 1..=3 {
                        let _ = cx.update_window(handle.into(), |_, window, cx| key(window, cx, "tab"));
                        pause(cx, 500).await;
                        shot(cx, ["", "tab-1", "tab-2", "tab-3"][stop]).await;
                    }
                    // Nothing on the step offers this any more: the walk
                    // without a server is the harness's own.
                    let _ = handle.update(cx, |view, _, cx| view.onboarding_choose_local(cx));
                } else {
                    type_into(handle, cx, 0, &server, 45).await;
                    let found = until(handle, cx, 30, offered).await;
                    say(&format!("server answered: {found}"));
                    pause(cx, 1500).await;
                    crate::motion::stats::report("discovery");
                    if let Some((email, password)) = &login {
                        type_into(handle, cx, 1, email, 40).await;
                        pause(cx, 300).await;
                        type_into(handle, cx, 2, password, 40).await;
                        pause(cx, 500).await;
                        press(handle, cx, "welcome-continue").await;
                    }
                    let signed_in = until(handle, cx, 40, |view, _| view.onboarding_facts().signed_in).await;
                    say(&format!("signed in: {signed_in}"));
                    if !signed_in {
                        // Nothing after this would be what it claims to be.
                        say(&format!(
                            "stopping: the server did not sign in ({:?})",
                            handle.update(cx, |view, _, cx| view.account.read(cx).error().map(str::to_owned)).ok().flatten()
                        ));
                        finish(cx).await;
                        return;
                    }
                }
                until(handle, cx, 10, step(Step::Audio)).await;
                say(&format!("now on {}", handle.update(cx, where_now).unwrap_or_default()));
                pause(cx, 1500).await;
                crate::motion::stats::report("to-audio");
                if name == "leave-at-audio" {
                    finish(cx).await;
                    return;
                }
                press(handle, cx, "welcome-allow-microphone").await;
                until(handle, cx, 5, |view, _| view.onboarding_facts().microphone == Permission::Allowed).await;
                pause(cx, 2600).await;
                crate::motion::stats::report("microphone");
                if local {
                    // On this Mac only, with the microphone alone.
                    shot(cx, "local-audio").await;
                    press(handle, cx, "welcome-continue").await;
                } else {
                    press(handle, cx, "welcome-allow-system").await;
                    pause(cx, 3400).await;
                    crate::motion::stats::report("system-audio");
                    press(handle, cx, "welcome-continue").await;
                    let _ = handle.update(cx, |view, _, _| hide_calendar(view));
                    until(handle, cx, 5, step(Step::Calendar)).await;
                    until(handle, cx, 15, |view, _| !view.calendars.loading).await;
                    let _ = handle.update(cx, |view, _, cx| {
                        hide_calendar(view);
                        cx.notify();
                    });
                    pause(cx, 2000).await;
                    press(handle, cx, "welcome-connect-calendar").await;
                    pause(cx, 2400).await;
                    // Now the step may look: what it finds is the account's
                    // own calendar, read from the server.
                    let _ = handle.update(cx, |view, _, cx| {
                        view.onboarding.calendar_hold = false;
                        cx.notify();
                    });
                    let mut found = until(handle, cx, 12, |view, _| view.onboarding_facts().calendar).await;
                    say(&format!("calendar found on the server: {found}"));
                    if !found && made_up {
                        let _ = handle.update(cx, |view, _, cx| {
                            made_up_calendar(view);
                            cx.notify();
                        });
                        say("calendar: made up for the capture");
                        found = true;
                    }
                    pause(cx, 3000).await;
                    crate::motion::stats::report("calendar");
                    press(handle, cx, if found { "welcome-continue" } else { "welcome-skip" }).await;
                }
                until(handle, cx, 5, step(Step::Pill)).await;
                pause(cx, 1900).await;
                crate::motion::stats::report("to-pill");
                press(handle, cx, "welcome-try-pill").await;
                pause(cx, 5200).await;
                crate::motion::stats::report("try-pill");
                press(handle, cx, "welcome-continue").await;
                until(handle, cx, 5, step(Step::Done)).await;
                pause(cx, 2800).await;
                crate::motion::stats::report("done");
                press(handle, cx, "welcome-continue").await;
                until(handle, cx, 5, |view, _| view.screen == Screen::Home).await;
                pause(cx, 3200).await;
                say(&format!("ended on {}", handle.update(cx, where_now).unwrap_or_default()));
                crate::motion::stats::report("home");
                if local {
                    shot(cx, "local-home").await;
                }
            }
            "stills" => {
                let permission = |channel: Channel, state: Permission| {
                    move |view: &mut Cereal, _: &mut Window, cx: &mut Context<Cereal>| {
                        view.onboarding.permissions.simulate(channel, state);
                        cx.notify();
                    }
                };
                pause(cx, 3400).await;
                shot(cx, "1-welcome").await;
                press(handle, cx, "welcome-continue").await;
                until(handle, cx, 5, step(Step::Server)).await;
                pause(cx, 900).await;
                shot(cx, "2-server-address").await;
                // An address nothing answers at: looking, then not found.
                type_into(handle, cx, 0, "http://127.0.0.1:9", 12).await;
                until(handle, cx, 20, |view, cx| matches!(view.onboarding_offer(cx), ServerOffer::Unreachable(_))).await;
                pause(cx, 900).await;
                shot(cx, "2-server-unreachable").await;
                type_into(handle, cx, 0, &server, 12).await;
                until(handle, cx, 4, |view, cx| view.onboarding_offer(cx) == ServerOffer::Checking).await;
                say("shot 2-server-looking");
                until(handle, cx, 30, offered).await;
                pause(cx, 1200).await;
                shot(cx, "2-server-found").await;
                if let Some((email, _)) = &login {
                    // One wrong password, to show how a refusal reads.
                    type_into(handle, cx, 1, email, 8).await;
                    type_into(handle, cx, 2, "not-the-password", 8).await;
                    pause(cx, 300).await;
                    shot(cx, "2-server-typed").await;
                    press(handle, cx, "welcome-continue").await;
                    until(handle, cx, 30, |view, cx| view.account.read(cx).error().is_some()).await;
                    pause(cx, 900).await;
                    shot(cx, "2-server-refused").await;
                }
                if press(handle, cx, "welcome-browser").await {
                    until(handle, cx, 5, |view, cx| view.onboarding_offer(cx) == ServerOffer::WaitingForBrowser).await;
                    pause(cx, 600).await;
                    shot(cx, "2-server-browser-waiting").await;
                    press(handle, cx, "welcome-browser-cancel").await;
                    pause(cx, 600).await;
                }
                if let Some((email, password)) = &login {
                    type_into(handle, cx, 1, email, 8).await;
                    type_into(handle, cx, 2, password, 8).await;
                    press(handle, cx, "welcome-continue").await;
                    let signed_in = until(handle, cx, 40, |view, _| view.onboarding_facts().signed_in).await;
                    say(&format!("signed in: {signed_in}"));
                    if !signed_in {
                        // Nothing after this would be what it claims to be.
                        say(&format!(
                            "stopping: the server did not sign in ({:?})",
                            handle.update(cx, |view, _, cx| view.account.read(cx).error().map(str::to_owned)).ok().flatten()
                        ));
                        finish(cx).await;
                        return;
                    }
                    // The step moves on by itself a moment after signing
                    // in; Back shows it at rest, signed in.
                    until(handle, cx, 10, step(Step::Audio)).await;
                    pause(cx, 900).await;
                    let _ = cx.update_window(handle.into(), |_, window, cx| key(window, cx, "escape"));
                    until(handle, cx, 5, step(Step::Server)).await;
                    pause(cx, 900).await;
                    shot(cx, "2-server-signed-in").await;
                    press(handle, cx, "welcome-continue").await;
                } else {
                    // Nothing on the step offers this any more: the walk
                    // without a server is the harness's own.
                    let _ = handle.update(cx, |view, _, cx| view.onboarding_choose_local(cx));
                }
                until(handle, cx, 10, step(Step::Audio)).await;
                pause(cx, 1200).await;
                shot(cx, "3-audio-not-yet").await;
                press(handle, cx, "welcome-allow-microphone").await;
                until(handle, cx, 5, |view, _| view.onboarding_facts().microphone == Permission::Allowed).await;
                pause(cx, 1500).await;
                shot(cx, "3-audio-microphone").await;
                let _ = handle.update(cx, permission(Channel::SystemAudio, Permission::Allowed));
                pause(cx, 1500).await;
                shot(cx, "3-audio-both").await;
                let _ = handle.update(cx, permission(Channel::SystemAudio, Permission::Denied));
                pause(cx, 1200).await;
                shot(cx, "3-audio-system-denied").await;
                let _ = handle.update(cx, permission(Channel::Microphone, Permission::Denied));
                pause(cx, 1200).await;
                shot(cx, "3-audio-denied").await;
                let _ = handle.update(cx, permission(Channel::Microphone, Permission::Allowed));
                let _ = handle.update(cx, permission(Channel::SystemAudio, Permission::Allowed));
                pause(cx, 900).await;
                press(handle, cx, "welcome-continue").await;
                let _ = handle.update(cx, |view, _, _| hide_calendar(view));
                let calendar = until(handle, cx, 5, step(Step::Calendar)).await;
                if calendar {
                    until(handle, cx, 15, |view, _| !view.calendars.loading).await;
                    let _ = handle.update(cx, |view, _, cx| {
                        hide_calendar(view);
                        cx.notify();
                    });
                    pause(cx, 1500).await;
                    shot(cx, "4-calendar-none").await;
                    press(handle, cx, "welcome-connect-calendar").await;
                    pause(cx, 900).await;
                    shot(cx, "4-calendar-waiting").await;
                    let _ = handle.update(cx, |view, _, cx| {
                        view.onboarding.calendar_hold = false;
                        cx.notify();
                    });
                    let mut found = until(handle, cx, 12, |view, _| view.onboarding_facts().calendar).await;
                    say(&format!("calendar found on the server: {found}"));
                    if !found && made_up {
                        let _ = handle.update(cx, |view, _, cx| {
                            made_up_calendar(view);
                            cx.notify();
                        });
                        say("calendar: made up for the capture");
                        found = true;
                    }
                    pause(cx, 1800).await;
                    shot(cx, if found { "4-calendar-connected" } else { "4-calendar-still-none" }).await;
                    press(handle, cx, if found { "welcome-continue" } else { "welcome-skip" }).await;
                }
                until(handle, cx, 5, step(Step::Pill)).await;
                pause(cx, 1500).await;
                shot(cx, "5-pill").await;
                press(handle, cx, "welcome-try-pill").await;
                pause(cx, 1300).await;
                say("shot 5-pill-trying");
                pause(cx, 4200).await;
                press(handle, cx, "welcome-continue").await;
                until(handle, cx, 5, step(Step::Done)).await;
                pause(cx, 1800).await;
                shot(cx, "6-done").await;
                // Back and forward with the keyboard: Escape, then Return.
                let _ = cx.update_window(handle.into(), |_, window, cx| key(window, cx, "escape"));
                pause(cx, 900).await;
                say(&format!("after Escape: {}", handle.update(cx, where_now).unwrap_or_default()));
                let _ = cx.update_window(handle.into(), |_, window, cx| key(window, cx, "enter"));
                pause(cx, 1500).await;
                say(&format!("after Return: {}", handle.update(cx, where_now).unwrap_or_default()));
                let _ = cx.update_window(handle.into(), |_, window, cx| key(window, cx, "enter"));
                until(handle, cx, 5, |view, _| view.screen == Screen::Home).await;
                pause(cx, 2500).await;
                say(&format!("ended on {}", handle.update(cx, where_now).unwrap_or_default()));
                shot(cx, "7-home").await;
            }
            "source" => {
                // Signed in, then the source card of a server note's enhanced
                // notes, streamed, reopened and closed mid-stream
                // (`summary::probe`).
                pause(cx, 1200).await;
                press(handle, cx, "welcome-continue").await;
                until(handle, cx, 5, step(Step::Server)).await;
                type_into(handle, cx, 0, &server, 4).await;
                until(handle, cx, 30, offered).await;
                if let Some((email, password)) = &login {
                    type_into(handle, cx, 1, email, 4).await;
                    type_into(handle, cx, 2, password, 4).await;
                    press(handle, cx, "welcome-continue").await;
                }
                let signed_in = until(handle, cx, 40, |view, _| view.onboarding_facts().signed_in).await;
                say(&format!("signed in: {signed_in}"));
                if signed_in {
                    until(handle, cx, 10, step(Step::Audio)).await;
                    let _ = handle.update(cx, |view, _, cx| view.finish_onboarding(cx));
                    pause(cx, 1500).await;
                    crate::summary::probe::run(handle, cx).await;
                }
            }
            "signed-in" => {
                // The server step at rest after signing in, and nothing else.
                pause(cx, 1200).await;
                press(handle, cx, "welcome-continue").await;
                until(handle, cx, 5, step(Step::Server)).await;
                type_into(handle, cx, 0, &server, 12).await;
                until(handle, cx, 30, offered).await;
                if let Some((email, password)) = &login {
                    type_into(handle, cx, 1, email, 8).await;
                    type_into(handle, cx, 2, password, 8).await;
                    press(handle, cx, "welcome-continue").await;
                }
                let signed_in = until(handle, cx, 40, |view, _| view.onboarding_facts().signed_in).await;
                say(&format!("signed in: {signed_in}"));
                if signed_in {
                    until(handle, cx, 10, step(Step::Audio)).await;
                    pause(cx, 900).await;
                    let _ = cx.update_window(handle.into(), |_, window, cx| key(window, cx, "escape"));
                    until(handle, cx, 5, step(Step::Server)).await;
                    pause(cx, 900).await;
                    shot(cx, "2-server-signed-in").await;
                }
            }
            "http" => {
                // A server reached over plain HTTP at an address that is not
                // this Mac's loopback (`CEREAL_WELCOME_SERVER`): what the
                // address line says for each kind of address, then a real
                // sign-in there and what the session can read.
                pause(cx, 1200).await;
                press(handle, cx, "welcome-continue").await;
                until(handle, cx, 5, step(Step::Server)).await;
                let kind = |view: &mut Cereal, _: &mut Window, cx: &mut Context<Cereal>| {
                    let typed = view.account.read(cx).url.read(cx).content();
                    format!("{typed} is {:?}", crate::link::address_transport(&typed))
                };
                type_into(handle, cx, 0, "http://127.0.0.1:3000", 10).await;
                until(handle, cx, 30, offered).await;
                say(&handle.update(cx, kind).unwrap_or_default());
                pause(cx, 900).await;
                shot(cx, "1-secure-loopback").await;
                type_into(handle, cx, 0, "http://notes.cereal.invalid", 10).await;
                until(handle, cx, 30, |view, cx| matches!(view.onboarding_offer(cx), ServerOffer::Unreachable(_))).await;
                say(&handle.update(cx, kind).unwrap_or_default());
                pause(cx, 900).await;
                shot(cx, "3-public-http").await;
                type_into(handle, cx, 0, &server, 10).await;
                let found = until(handle, cx, 30, offered).await;
                say(&handle.update(cx, kind).unwrap_or_default());
                say(&format!("discovery over {server}: {found}"));
                pause(cx, 900).await;
                shot(cx, "2-private").await;
                if let Some((email, password)) = &login {
                    type_into(handle, cx, 1, email, 8).await;
                    type_into(handle, cx, 2, password, 8).await;
                    press(handle, cx, "welcome-continue").await;
                }
                let signed_in = until(handle, cx, 40, |view, _| view.onboarding_facts().signed_in).await;
                say(&format!(
                    "password sign-in over {server}: {signed_in} ({:?})",
                    handle.update(cx, |view, _, cx| view.account.read(cx).error().map(str::to_owned)).ok().flatten()
                ));
                if signed_in {
                    until(handle, cx, 10, step(Step::Audio)).await;
                    pause(cx, 800).await;
                    // What the session reads from that server.
                    until(handle, cx, 30, |view, _| view.server_pending == 0 && !view.calendars.loading).await;
                    say(&handle
                        .update(cx, |view, _, _| {
                            format!(
                                "session at {}: {} notes read, server status {:?}, calendars loaded {} ({} sources, error {:?})",
                                crate::accounts::configured_url(),
                                view.workspace.notes.iter().filter(|note| note.server_id.is_some()).count(),
                                view.server_status,
                                view.calendars.loaded,
                                view.calendars.sources.len(),
                                view.calendars.error
                            )
                        })
                        .unwrap_or_default());
                    let _ = handle.update(cx, |view, _, cx| view.finish_onboarding(cx));
                    pause(cx, 3500).await;
                    shot(cx, "4-home-over-http").await;
                    let _ = handle.update(cx, |view, _, cx| view.check_for_updates(cx));
                    pause(cx, 6000).await;
                    say(&format!(
                        "check for updates: {:?}",
                        handle
                            .update(cx, |view, _, _| format!("{} ({})", view.updates.row().0, view.updates.status()))
                            .ok()
                    ));
                    // The account screen says the same about the address.
                    let _ = handle.update(cx, |view, _, cx| view.navigate(Screen::Account, cx));
                    pause(cx, 1500).await;
                    shot(cx, "5-account-private").await;
                    let _ = handle.update(cx, |view, _, cx| {
                        let input = view.account.read(cx).url.clone();
                        input.update(cx, |input, cx| input.set_content("http://notes.cereal.invalid", cx));
                        cx.notify();
                    });
                    pause(cx, 2500).await;
                    shot(cx, "6-account-public-http").await;
                }
            }
            "audio-asking" => {
                // Straight to the audio step, to hold the moment between
                // pressing Allow and the answer.
                let _ = handle.update(cx, |view, _, cx| {
                    view.begin_onboarding(crate::onboarding::Flow { step: Step::Audio, local: true }, true, cx);
                    view.onboarding.permissions.hold_answer = true;
                });
                pause(cx, 1500).await;
                press(handle, cx, "welcome-allow-microphone").await;
                pause(cx, 600).await;
                shot(cx, "3-audio-asking").await;
                let _ = handle.update(cx, |view, _, cx| {
                    view.onboarding.permissions.hold_answer = false;
                    cx.notify();
                });
                pause(cx, 1800).await;
            }
            "audio-real" => {
                // Without the stand-in: what macOS itself says, and what is
                // heard on whatever is already allowed. Nothing is asked for.
                let _ = handle.update(cx, |view, _, cx| {
                    view.begin_onboarding(crate::onboarding::Flow { step: Step::Audio, local: true }, true, cx);
                });
                for _ in 0..8 {
                    pause(cx, 1000).await;
                    let heard = handle
                        .update(cx, |view, _, _| {
                            let permissions = &view.onboarding.permissions;
                            format!(
                                "microphone {:?} level {:.3}; system audio {:?} level {:.3}",
                                permissions.of(Channel::Microphone),
                                permissions.level(Channel::Microphone),
                                permissions.of(Channel::SystemAudio),
                                permissions.level(Channel::SystemAudio)
                            )
                        })
                        .unwrap_or_default();
                    say(&format!("heard: {heard}"));
                }
                shot(cx, "3-audio-real").await;
            }
            "again" => {
                pause(cx, 1500).await;
                let _ = handle.update(cx, |view, _, cx| {
                    view.settings_tab = crate::screens::SettingsTab::About;
                    view.navigate(Screen::Settings, cx);
                });
                pause(cx, 1200).await;
                shot(cx, "8-settings-about").await;
                let _ = handle.update(cx, |view, _, cx| view.show_welcome_again(cx));
                pause(cx, 3200).await;
                say(&format!("after Show welcome again: {}", handle.update(cx, where_now).unwrap_or_default()));
                shot(cx, "8-welcome-again").await;
                let _ = cx.update_window(handle.into(), |_, window, cx| key(window, cx, "escape"));
                pause(cx, 1500).await;
                say(&format!("after Escape: {}", handle.update(cx, where_now).unwrap_or_default()));
            }
            // The server calendar card with days that hold one and two events, for
            // measuring its spacing (`CEREAL_TOUR_LINKED_CALENDAR=1`).
            "calendar-card" => {
                let _ = handle.update(cx, |view, _, cx| view.navigate(Screen::Home, cx));
                pause(cx, 1200).await;
                let _ = handle.update(cx, |view, _, cx| {
                    let at = |days: i64, hour: u32, minute: u32| {
                        let date = chrono::Local::now().date_naive() + chrono::Duration::days(days);
                        date.and_hms_opt(hour, minute, 0).unwrap().and_local_timezone(chrono::Local).unwrap().to_rfc3339()
                    };
                    // The reference's days: two events today, then single
                    // events tomorrow, the day after and a week out.
                    let event = |id: &str, title: &str, days: i64, from: (u32, u32), to: (u32, u32), call: bool| {
                        serde_json::json!({"id":id,"calendarId":"work","calendarName":"Work","color":"#87e5e8","title":title,"startAt":at(days, from.0, from.1),"endAt":at(days, to.0, to.1),"allDay":false,"startDate":"2026-11-26","endDate":"2026-11-26","timeZone":null,"location":null,"meetingUrl":if call { serde_json::json!("https://meet.example.com/abc") } else { serde_json::Value::Null },"noteId":null})
                    };
                    view.calendars.sources = serde_json::from_value(serde_json::json!([{"id":"work","name":"Work","provider":"google","color":"#87e5e8","enabled":true,"sourceHost":"calendar.google.com","lastSyncedAt":null,"lastSyncError":null,"eventCount":5,"createdAt":"2026-11-12T00:00:00Z"}])).unwrap_or_default();
                    view.calendars.events = serde_json::from_value(serde_json::json!([
                        event("a", "Maya Chen - Schedule a Time to Meet", 0, (17, 0), (17, 30), true),
                        event("b", "Northwind Labs/H5 Data Centers Intro", 0, (19, 30), (20, 0), true),
                        event("c", "R&D Weekly", 1, (14, 0), (15, 0), false),
                        event("d", "Nir <-> Maya", 2, (15, 0), (15, 30), false),
                        event("e", "gh.example.dev cert expires", 7, (7, 30), (11, 45), false),
                    ])).unwrap_or_default();
                    // What a server sends; without one it is 0 and no day shows.
                    view.calendars.look_ahead_days = 30;
                    view.calendars.personal_linked = true;
                    view.calendars.loaded = true;
                    view.calendars.loading = false;
                    view.calendars.error = None;
                    cx.notify();
                });
                pause(cx, 1500).await;
                shot(cx, "calendar-card").await;
                // `CEREAL_TOUR_HOVER="x,y x,y"`: one picture with the pointer
                // at each point, in window coordinates.
                let points: Vec<(f32, f32)> = crate::environment::var("CEREAL_TOUR_HOVER")
                    .unwrap_or_default()
                    .split_whitespace()
                    .filter_map(|pair| {
                        let (x, y) = pair.split_once(',')?;
                        Some((x.parse().ok()?, y.parse().ok()?))
                    })
                    .collect();
                for (index, (x, y)) in points.into_iter().enumerate() {
                    let _ = handle.update(cx, |_, window, cx| pointer(window, cx, x, y));
                    pause(cx, 900).await;
                    shot(cx, &format!("calendar-card-hover-{index}")).await;
                }
            }
            // Settings › Calendar with made-up calendars, for the visible
            // calendars card.
            "visible-calendars" => {
                let _ = handle.update(cx, |view, _, cx| {
                    let calendar = |id: &str, name: &str, color: &str, primary: bool| {
                        serde_json::json!({"accessRole":"owner","color":color,"connectionId":"c","id":id,"name":name,"primary":primary,"selected":primary,"timeZone":"America/New_York"})
                    };
                    view.calendars.personal = serde_json::from_value(serde_json::json!([
                        calendar("p2", "Northwind Labs - Visiting IAD", "#ac7262", false),
                        calendar("p3", "Northwind Labs IAD OoO", "#cfb8c0", false),
                        calendar("p1", "maya@example.com", "#9fe1e8", true),
                        calendar("p4", "omer@example.com", "#c2c2c2", false),
                        calendar("p5", "Office Visits", "#fbe983", false),
                        calendar("p6", "Travel Dates", "#c2c2c2", false),
                    ])).unwrap_or_default();
                    view.calendars.personal_linked = true;
                    view.calendars.loaded = true;
                    view.open_settings(crate::screens::SettingsTab::Calendar, cx);
                    cx.notify();
                });
                pause(cx, 1500).await;
                shot(cx, "visible-calendars").await;
            }
            "server-required" => {
                // What an install without a server opens on, and (with
                // `CEREAL_WELCOME_LOGIN`) signing in there and opening Home.
                pause(cx, 2000).await;
                shot(cx, "1-opened").await;
                if handle.update(cx, |view, _, _| view.onboarding.step() == Some(Step::Welcome)).unwrap_or(false) {
                    press(handle, cx, "welcome-continue").await;
                    until(handle, cx, 5, step(Step::Server)).await;
                    pause(cx, 1500).await;
                    shot(cx, "2-server").await;
                }
                // Continue without an address, a few times: nothing moves on.
                for _ in 0..3 {
                    press(handle, cx, "welcome-continue").await;
                    pause(cx, 400).await;
                }
                say(&format!("after Continue with nothing typed: {}", handle.update(cx, where_now).unwrap_or_default()));
                say(&format!(
                    "the step offers this Mac only: {}",
                    placed("welcome-local").is_some()
                ));
                if let Some((email, password)) = &login {
                    type_into(handle, cx, 0, &server, 10).await;
                    let found = until(handle, cx, 30, offered).await;
                    say(&format!("server answered: {found}"));
                    pause(cx, 800).await;
                    shot(cx, "3-server-found").await;
                    type_into(handle, cx, 1, email, 10).await;
                    type_into(handle, cx, 2, password, 10).await;
                    press(handle, cx, "welcome-continue").await;
                    let signed_in = until(handle, cx, 40, |view, _| view.onboarding_facts().signed_in).await;
                    say(&format!("signed in: {signed_in}"));
                }
                if handle.update(cx, |view, _, _| view.onboarding_facts().signed_in).unwrap_or(false) {
                    pause(cx, 1500).await;
                    let _ = handle.update(cx, |view, _, cx| {
                        if view.screen == Screen::Welcome {
                            view.finish_onboarding(cx);
                        }
                    });
                    pause(cx, 2500).await;
                    shot(cx, "4-home").await;
                }
            }
            // The workspace switcher pressed three times with real pointer events:
            // open, closed, open. Then the cases where the button must not be a
            // toggle: reopening soon after an outside press closed the menu, a slow
            // press, a keyboard-style open after an outside press, and a press whose
            // release never arrived. The harness closes the welcome first.
            "menu-toggle" => {
                pause(cx, 1500).await;
                for round in 1..=3 {
                    press(handle, cx, "workspace-switcher").await;
                    pause(cx, 900).await;
                    let open = handle.update(cx, |view, _, _| view.menu.is_some()).unwrap_or(false);
                    say(&format!("after press {round}: menu {}", if open { "open" } else { "closed" }));
                }
                async fn press_at(handle: WindowHandle<Cereal>, cx: &mut AsyncApp, at: Point<Pixels>, hold: u64, release: bool) {
                    let send = |cx: &mut AsyncApp, event: PlatformInput| {
                        let _ = cx.update_window(handle.into(), |_, window, cx| window.dispatch_event(event, cx));
                    };
                    send(cx, PlatformInput::MouseMove(MouseMoveEvent { position: at, ..Default::default() }));
                    pause(cx, 260).await;
                    send(cx, PlatformInput::MouseDown(MouseDownEvent { button: MouseButton::Left, position: at, click_count: 1, ..Default::default() }));
                    pause(cx, hold).await;
                    if release {
                        send(cx, PlatformInput::MouseUp(MouseUpEvent { button: MouseButton::Left, position: at, click_count: 1, ..Default::default() }));
                    }
                    pause(cx, 120).await;
                    // Like `press`: the pointer leaves again, so no hover is left behind.
                    send(cx, PlatformInput::MouseMove(MouseMoveEvent { position: point(px(-40.), px(-40.)), ..Default::default() }));
                    pause(cx, 40).await;
                }
                let Some(switcher) = placed("workspace-switcher").map(|bounds| bounds.center()) else {
                    say("missing workspace-switcher");
                    finish(cx).await;
                    return;
                };
                let open = |handle: WindowHandle<Cereal>, cx: &mut AsyncApp| {
                    let menu = handle.update(cx, |view, _, _| view.menu.clone()).ok().flatten();
                    let record = handle.update(cx, |view, _, _| format!("press {} record {:?} on {:?} visible {:?} exiting {}", view.press_seq, view.menu_closed_by_press, view.screen, view.visible_menu, view.menu_exiting)).unwrap_or_default();
                    format!("{} [{record}]", if menu == Some(Menu::Workspace) { "open" } else if menu.is_none() { "closed" } else { "other menu" })
                };
                // Open (press 3 left it open), then press empty space.
                let size = handle.update(cx, |_, window, _| window.viewport_size()).unwrap_or(size(px(1200.), px(800.)));
                // Near the top right: nothing there takes a press. (The middle of an empty Home does: a
                // press there, in this harness, left the switcher not answering clicks for the rest of the run.)
                let empty = point(size.width - px(200.), px(60.));
                say(&format!("switcher at {switcher:?}, empty space at {empty:?} in {size:?}"));
                press_at(handle, cx, empty, 60, true).await;
                say(&format!("C1 outside press: menu {}", open(handle, cx)));
                press_at(handle, cx, switcher, 60, true).await;
                pause(cx, 600).await;
                say(&format!("C2 switcher pressed about 450 ms after an outside press: menu {} (want open)", open(handle, cx)));
                press_at(handle, cx, switcher, 700, true).await;
                pause(cx, 600).await;
                say(&format!("E switcher held 700 ms while open: menu {} (want closed)", open(handle, cx)));
                press_at(handle, cx, switcher, 60, true).await;
                pause(cx, 600).await;
                say(&format!("E2 switcher again: menu {} (want open)", open(handle, cx)));
                press_at(handle, cx, empty, 60, true).await;
                let opened = handle.update(cx, |view, _, cx| { let opened = view.open_menu(Menu::Workspace); cx.notify(); opened }).unwrap_or(false);
                pause(cx, 600).await;
                say(&format!("G keyboard-style open right after an outside press: opened={opened} menu {} (want open)", open(handle, cx)));
                // A press whose release never reached the window (let go outside it), then a click.
                press_at(handle, cx, empty, 60, false).await;
                say(&format!("H1 outside press, no release: menu {}", open(handle, cx)));
                press_at(handle, cx, switcher, 60, true).await;
                pause(cx, 600).await;
                say(&format!("H2 switcher: menu {} (want open)", open(handle, cx)));
                press_at(handle, cx, switcher, 60, true).await;
                pause(cx, 600).await;
                say(&format!("H3 switcher again: menu {} (want closed)", open(handle, cx)));
            }
            // The sidebar's Trash popover, driven with real pointer events:
            // the button as a toggle, an outside press, Escape, the arrow
            // keys, Restore, Delete forever (cancelled, then confirmed) and
            // the empty state. Run on a data folder whose welcome is done
            // and whose workspace has trashed notes.
            "trash" => {
                async fn press_at(handle: WindowHandle<Cereal>, cx: &mut AsyncApp, at: Point<Pixels>) {
                    let send = |cx: &mut AsyncApp, event: PlatformInput| {
                        let _ = cx.update_window(handle.into(), |_, window, cx| window.dispatch_event(event, cx));
                    };
                    send(cx, PlatformInput::MouseMove(MouseMoveEvent { position: at, ..Default::default() }));
                    pause(cx, 200).await;
                    send(cx, PlatformInput::MouseDown(MouseDownEvent { button: MouseButton::Left, position: at, click_count: 1, ..Default::default() }));
                    pause(cx, 120).await;
                    send(cx, PlatformInput::MouseUp(MouseUpEvent { button: MouseButton::Left, position: at, click_count: 1, ..Default::default() }));
                    pause(cx, 120).await;
                }
                let state = |view: &mut Cereal, _: &mut Window, _: &mut Context<Cereal>| {
                    let trashed: Vec<String> = view
                        .workspace
                        .notes
                        .iter()
                        .filter(|note| note.trashed)
                        .map(|note| format!("{}:{}", note.id, note.title))
                        .collect();
                    format!(
                        "screen {:?}, note {:?}, menu {:?}, confirm {:?}, selected {:?}, notes {}, trashed [{}]",
                        view.screen,
                        view.current_note,
                        view.menu,
                        view.trash_ask,
                        view.menu_keyboard.selected,
                        view.workspace.notes.len(),
                        trashed.join(", ")
                    )
                };
                let report = |handle: WindowHandle<Cereal>, cx: &mut AsyncApp, what: &str| {
                    say(&format!("trash {what}: {}", handle.update(cx, state).unwrap_or_default()));
                };
                let rect = |name: &str| {
                    placed(name).map_or("none".to_string(), |b| {
                        format!(
                            "{:.1},{:.1} {:.1}x{:.1}",
                            f32::from(b.origin.x),
                            f32::from(b.origin.y),
                            f32::from(b.size.width),
                            f32::from(b.size.height)
                        )
                    })
                };
                let _ = handle.update(cx, |view, _, cx| {
                    if view.screen == Screen::Welcome {
                        view.finish_onboarding(cx);
                    }
                    view.navigate(Screen::Home, cx);
                });
                pause(cx, 1500).await;
                report(handle, cx, "start");
                shot(cx, "0-closed").await;
                // Hover: the tooltip.
                if let Some(at) = placed("footer-trash").map(|b| b.center()) {
                    let _ = cx.update_window(handle.into(), |_, window, cx| pointer(window, cx, f32::from(at.x), f32::from(at.y)));
                    pause(cx, 1200).await;
                    shot(cx, "0-tooltip").await;
                }
                press(handle, cx, "footer-trash").await;
                pause(cx, 900).await;
                report(handle, cx, "opened by the button");
                say(&format!(
                    "layout: button {}; popover {}; restore {}; delete {}",
                    rect("footer-trash"),
                    rect("trash-popover"),
                    rect("trash-restore-0"),
                    rect("trash-delete-0")
                ));
                shot(cx, "1-open").await;
                press(handle, cx, "footer-trash").await;
                pause(cx, 900).await;
                report(handle, cx, "button pressed again");
                shot(cx, "2-closed-by-button").await;
                press(handle, cx, "footer-trash").await;
                pause(cx, 700).await;
                report(handle, cx, "opened again");
                // An empty part of the page, so the press reaches nothing else.
                press_at(handle, cx, point(px(880.), px(430.))).await;
                pause(cx, 700).await;
                report(handle, cx, "pressed outside");
                press(handle, cx, "footer-trash").await;
                pause(cx, 700).await;
                for name in ["down", "down", "down", "up"] {
                    let _ = cx.update_window(handle.into(), |_, window, cx| key(window, cx, name));
                    pause(cx, 250).await;
                }
                pause(cx, 500).await;
                report(handle, cx, "after down, down, down, up");
                shot(cx, "3-keyboard").await;
                let _ = cx.update_window(handle.into(), |_, window, cx| key(window, cx, "escape"));
                pause(cx, 800).await;
                report(handle, cx, "after Escape");
                press(handle, cx, "footer-trash").await;
                pause(cx, 800).await;
                press(handle, cx, "trash-restore-0").await;
                pause(cx, 900).await;
                report(handle, cx, "after Restore on the first row");
                shot(cx, "4-restored").await;
                press(handle, cx, "trash-delete-0").await;
                pause(cx, 700).await;
                report(handle, cx, "after Delete forever on the first row");
                shot(cx, "5-confirm").await;
                // While the row asks, the arrows stay on it and Return lets go.
                for name in ["down", "down"] {
                    let _ = cx.update_window(handle.into(), |_, window, cx| key(window, cx, name));
                    pause(cx, 250).await;
                }
                report(handle, cx, "after down, down while asking");
                let _ = cx.update_window(handle.into(), |_, window, cx| key(window, cx, "enter"));
                pause(cx, 500).await;
                report(handle, cx, "after Return while asking");
                press(handle, cx, "trash-delete-0").await;
                pause(cx, 500).await;
                let _ = cx.update_window(handle.into(), |_, window, cx| key(window, cx, "escape"));
                pause(cx, 500).await;
                report(handle, cx, "after Escape in the question");
                press(handle, cx, "trash-delete-0").await;
                pause(cx, 500).await;
                press(handle, cx, "trash-cancel-0").await;
                pause(cx, 500).await;
                report(handle, cx, "after Cancel");
                press(handle, cx, "trash-delete-0").await;
                pause(cx, 500).await;
                press(handle, cx, "trash-confirm-0").await;
                pause(cx, 900).await;
                report(handle, cx, "after Delete confirmed");
                shot(cx, "6-deleted").await;
                // What the row says when the server's copy was edited
                // elsewhere (the server's 409, shown here without a server),
                // then "Delete anyway".
                let _ = handle.update(cx, |view, _, cx| {
                    if let Some(row) = crate::trash::trash_rows(&view.workspace).first() {
                        view.trash_ask = Some(crate::trash::Ask::Changed(row.id));
                        cx.notify();
                    }
                });
                pause(cx, 500).await;
                report(handle, cx, "shown as edited elsewhere");
                shot(cx, "6b-changed").await;
                press(handle, cx, "trash-confirm-0").await;
                pause(cx, 900).await;
                report(handle, cx, "after Delete anyway");
                for _ in 0..8 {
                    if placed("trash-delete-0").is_none()
                        || handle.update(cx, |view, _, _| crate::trash::trash_rows(&view.workspace).is_empty()).unwrap_or(true)
                    {
                        break;
                    }
                    press(handle, cx, "trash-delete-0").await;
                    pause(cx, 400).await;
                    press(handle, cx, "trash-confirm-0").await;
                    pause(cx, 700).await;
                }
                pause(cx, 600).await;
                report(handle, cx, "after deleting the rest");
                shot(cx, "7-empty").await;
                press(handle, cx, "footer-trash").await;
                pause(cx, 900).await;
                report(handle, cx, "closed");
            }
            // The popover growing out of the trash button and folding back
            // into it, a frame at a time: run with `CEREAL_SLOW_MOTION=8`,
            // and with `CEREAL_REDUCE_MOTION=1` for the plain fade.
            "trash-motion" => {
                async fn frame(cx: &mut AsyncApp, name: &str) {
                    say(&format!("shot {name}"));
                    if let Some(directory) = crate::environment::var_os("CEREAL_WELCOME_SHOTS") {
                        let picture = std::path::PathBuf::from(directory).join(format!("{name}.png"));
                        for _ in 0..200 {
                            if picture.exists() {
                                break;
                            }
                            cx.background_executor().timer(Duration::from_millis(20)).await;
                        }
                    }
                }
                let _ = handle.update(cx, |view, _, cx| {
                    if view.screen == Screen::Welcome {
                        view.finish_onboarding(cx);
                    }
                    view.navigate(Screen::Home, cx);
                });
                pause(cx, 1500).await;
                let _ = handle.update(cx, |view, _, cx| view.toggle_trash(cx));
                for index in 0..6 {
                    frame(cx, &format!("m-open-{index}")).await;
                    cx.background_executor().timer(Duration::from_millis(250)).await;
                }
                pause(cx, 800).await;
                let _ = handle.update(cx, |view, _, cx| view.toggle_trash(cx));
                for index in 0..4 {
                    frame(cx, &format!("m-close-{index}")).await;
                    cx.background_executor().timer(Duration::from_millis(250)).await;
                }
            }
            other => say(&format!("unknown tour {other}")),
        }
        finish(cx).await;
    })
    .detach();
}
