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
                "palette check: Down then Return opened {:?} (the row under New note is the first folder)",
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
        step("nav-connect", 750, |view, cx| {
            view.navigate(Screen::Connect, cx)
        }),
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
        // The Calendar tab, where the toggle the next steps flip is shown.
        step("settings-open", 900, |view, cx| {
            view.open_settings(crate::screens::SettingsTab::Calendar, cx)
        }),
        // In memory only: the tour never persists a changed preference.
        step("settings-toggle", 700, |view, _| {
            view.workspace.settings.show_upcoming = !view.workspace.settings.show_upcoming
        }),
        step("settings-toggle-back", 700, |view, _| {
            view.workspace.settings.show_upcoming = !view.workspace.settings.show_upcoming
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
        input("p-folders-hover", 900, |window, cx| {
            pointer(window, cx, 96., 259.)
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
        step("p-connect", 900, |view, cx| {
            view.navigate(Screen::Connect, cx)
        }),
        step("p-my-notes", 900, |view, cx| {
            view.navigate(Screen::MyNotes, cx)
        }),
        step("p-folder", 900, |view, cx| {
            view.folders_open = true;
            if let Some(folder) = view.workspace.folders.first().map(|folder| folder.id) {
                view.navigate(Screen::Folder(folder), cx);
            }
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
        menu("p-note-folder-menu", 900, Some(Menu::MoveTo)),
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
        step("p-recipes", 1100, |view, cx| {
            view.chat_recipes_open = true;
            cx.notify();
        }),
        step("p-recipes-close", 500, |view, cx| {
            view.close_chat_recipes(cx)
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
            view.navigate(Screen::Trash, cx)
        }),
        step("p-icons-empty-home", 4000, |view, cx| {
            for note in &mut view.workspace.notes {
                note.trashed = true;
            }
            view.navigate(Screen::Home, cx)
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
        step("b-trash", 1600, |view, cx| view.navigate(Screen::Trash, cx)),
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
            .chain(bench_steps())
            .chain(live_bench_steps())
            .chain(first_frame_steps())
            .chain(keyboard_steps())
            .chain(polish_steps())
            .chain(gap_still_steps())
            .chain(gold_steps())
            .chain(icon_steps())
            .chain(idle_steps());
        settle(cx).await;
        crate::motion::stats::report("launch");
        for (name, hold, action) in all {
            if only.is_empty() && ["p-", "b-", "i-"].iter().any(|kind| name.starts_with(kind)) {
                continue;
            }
            if !only.is_empty() && !only.iter().any(|prefix| name.starts_with(prefix.as_str())) {
                continue;
            }
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_millis());
            eprintln!("tour {stamp} {name}");
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
            if name.starts_with("p-") {
                settle(cx).await;
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
                            "{:?}, note {:?} ({:?}), tab {:?}, transcript open {}, chat open {}, search open {}, menu {:?}, model picker open {}, chat history open {}, recipes open {}",
                            view.screen,
                            view.current_note,
                            note.map(|note| note.title.as_str()),
                            view.active_tab,
                            view.transcript_open,
                            view.assistant_open,
                            view.search_open,
                            view.menu,
                            view.chat_model_picker_open,
                            view.chat_history_open,
                            view.chat_recipes_open
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
            if crate::account::identity().is_some() {
                break;
            }
            cx.background_executor().timer(tick).await;
        }
        if crate::account::identity().is_none() {
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
                    let id = view.workspace.create_note();
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
/// * `local`: the same walk without a server ("Try it on this Mac only").
/// * `leave-at-audio`: walks to the audio step and quits there, to check
///   that the next launch resumes on it.
/// * `report`: says which screen and step the app opened on, then quits.
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
                    press(handle, cx, "welcome-local").await;
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
                    press(handle, cx, "welcome-local").await;
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
            other => say(&format!("unknown tour {other}")),
        }
        finish(cx).await;
    })
    .detach();
}
