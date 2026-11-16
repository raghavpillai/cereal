//! Separate legacy desktop Nub. Shares the local recording owner with the note
//! footer. Production levels come from real audio; explicit previews open no devices.
use crate::{
    app::{Cereal, Screen},
    recording::{Phase, TranscriptionAttention},
    ui::{FONT, icon},
};
use gpui::{prelude::*, *};
use std::{
    cell::RefCell,
    path::PathBuf,
    rc::{Rc, Weak},
    time::Duration,
};

const NUB_SIZE: (f32, f32) = (42., 70.);
const BUBBLE_SIZE: (f32, f32) = (300., 350.);
/// Clear space between a panel's rectangle (`NUB_SIZE`, `BUBBLE_SIZE`: what
/// every position and hover is worked out in) and the shape drawn in it.
const INSET: f32 = 4.;
/// How far each panel's window reaches beyond its rectangle, so the shape's
/// drop shadow has room to fall off instead of being cut at the window edge.
const NUB_SHADOW: f32 = 12.;
const BUBBLE_SHADOW: f32 = 28.;
type SharedHost = Rc<RefCell<Host>>;
type WeakHost = Weak<RefCell<Host>>;

/// Identity of a note's stored transcript: when it changes, the bubble's
/// copy of the rows is rebuilt. Interim text is not part of it.
type StoredKey = (u64, u64, usize, usize);

struct Registry {
    _host: SharedHost,
}
impl Global for Registry {}

struct Host {
    main: WindowHandle<Cereal>,
    model: WeakEntity<Cereal>,
    active: bool,
    force_preview: bool,
    review_expanded: bool,
    nub: Option<WindowHandle<Nub>>,
    bubble: Option<WindowHandle<TranscriptBubble>>,
    position: Option<Bounds<Pixels>>,
    available: Bounds<Pixels>,
    display: Option<DisplayId>,
    hover_nub: bool,
    hover_bubble: bool,
    hover_epoch: u64,
    /// Whether the hover watchdog is running (see `watch_hover`).
    watching: bool,
    /// What each panel last drew, to skip redraws that would change nothing.
    shown_nub: Option<(Phase, TranscriptionAttention, bool, [f32; 3])>,
    shown_bubble: Option<(Phase, TranscriptionAttention, (u64, usize, usize, u64))>,
    /// The theme the panels were last drawn in.
    shown_dark: Option<bool>,
    subscriptions: Vec<Subscription>,
}

/// Install once, after the main root exists. App-global ownership retains only a
/// weak reference to the main model, so a panel cannot keep a closed note alive.
pub fn install(main: WindowHandle<Cereal>, mode: &str, cx: &mut App) {
    let Ok(model) = main.entity(cx) else { return };
    let (mut available, mut display, active) = main
        .update(cx, |_, window, cx| {
            let display = window.display(cx).or_else(|| cx.primary_display());
            (
                display.as_ref().map(|d| d.bounds()).unwrap_or(Bounds::new(
                    point(px(0.), px(0.)),
                    size(px(1440.), px(900.)),
                )),
                display.map(|d| d.id()),
                window.is_window_active(),
            )
        })
        .unwrap();
    // The capsule rests in the part of a screen that is clear of the menu
    // bar and the Dock, on the screen it was left on or the nearest to it.
    let screens = screens(cx);
    if let Some((_, screen)) = resting_place(
        Bounds::new(available.center(), size(px(0.), px(0.))),
        &screens,
    ) {
        available = screen.visible;
    }
    let position = load_position(cx);
    if let Some((_, screen)) = position.and_then(|position| resting_place(position, &screens)) {
        available = screen.visible;
        if let Some(saved_display) = display_of(screen.frame, cx) {
            display = Some(saved_display);
        }
    }
    let host = Rc::new(RefCell::new(Host {
        main,
        model: model.downgrade(),
        active,
        force_preview: matches!(mode, "pill" | "pill-expanded" | "pill-attention"),
        review_expanded: mode == "pill-expanded",
        nub: None,
        bubble: None,
        position,
        available,
        display,
        hover_nub: false,
        hover_bubble: false,
        hover_epoch: 0,
        watching: false,
        shown_nub: None,
        shown_bubble: None,
        shown_dark: None,
        subscriptions: Vec::new(),
    }));
    #[cfg(target_os = "macos")]
    install_tracking(Rc::downgrade(&host), cx);
    let weak = Rc::downgrade(&host);
    let observed = cx.observe(&model, move |_, cx| schedule_sync(&weak, cx));
    host.borrow_mut().subscriptions.push(observed);
    let weak = Rc::downgrade(&host);
    let _ = main.update(cx, |view, window, cx| {
        view._subscriptions
            .push(cx.observe_window_activation(window, move |_, window, cx| {
                if let Some(host) = weak.upgrade() {
                    host.borrow_mut().active = window.is_window_active();
                }
                schedule_sync(&weak, cx);
            }));
    });
    let weak = Rc::downgrade(&host);
    let released = cx.observe_release(&model, move |_, cx| {
        if let Some(host) = weak.upgrade() {
            close_all(&host, cx);
        }
    });
    host.borrow_mut().subscriptions.push(released);
    cx.set_global(Registry {
        _host: host.clone(),
    });
    let _ = main.update(cx, |_, window, cx| {
        window.on_window_should_close(cx, |window, cx| {
            hide_main_window(window, cx);
            false
        });
    });
    schedule_sync(&Rc::downgrade(&host), cx);
    if host.borrow().force_preview {
        let _ = main.update(cx, |_, window, _| window.minimize_window());
    }
}

/// Review fixtures focus their own separate panel when reopened by CUA. Normal
/// launches always follow the main-window reopen behavior instead.
pub fn reopen_preview(cx: &mut App) -> bool {
    let Some(host) = cx.try_global::<Registry>().map(|r| r._host.clone()) else {
        return false;
    };
    if !host.borrow().force_preview {
        return false;
    }
    let (bubble, nub) = {
        let h = host.borrow();
        (h.bubble, h.nub)
    };
    if let Some(panel) = bubble {
        let _ = panel.update(cx, |_, w, _| w.activate_window());
    } else if let Some(panel) = nub {
        let _ = panel.update(cx, |_, w, _| w.activate_window());
    }
    cx.activate(true);
    true
}

/// Restore a minimized main window before GPUI orders it to the front.
pub fn reveal_main_window(window: &Window) {
    // "Quit" takes the app out of the Dock; its window brings it back.
    crate::menubar::show_in_dock();
    native::reveal(window);
}

/// Cereal's normal red-close/Cmd-W path hides its primary window. Keeping the
/// root alive preserves the recording owner and lets the desktop pill reopen it.
pub fn hide_main_window(window: &Window, cx: &mut App) {
    native::hide_main(window);
    if let Some(host) = cx.try_global::<Registry>().map(|r| r._host.clone()) {
        host.borrow_mut().active = false;
        schedule_sync(&Rc::downgrade(&host), cx);
    }
}

/// Something the panels show changed without the main window's view being
/// notified (interim transcript text while its note is not on screen there).
pub fn refresh(cx: &mut App) {
    if let Some(host) = cx.try_global::<Registry>().map(|r| r._host.clone()) {
        schedule_sync(&Rc::downgrade(&host), cx);
    }
}

/// Native smoke validation uses the actual registered window, not a second
/// implementation of the visibility predicate.
pub fn recording_window_size(cx: &mut App) -> Option<Size<Pixels>> {
    let host = cx.try_global::<Registry>()?._host.clone();
    let panel = host.borrow().nub?;
    panel
        .update(cx, |_, window, _| without_margin(window.viewport_size(), NUB_SHADOW))
        .ok()
}
/// A panel's size without the margin its window keeps for the shadow.
fn without_margin(viewport: Size<Pixels>, margin: f32) -> Size<Pixels> {
    size(viewport.width - px(2. * margin), viewport.height - px(2. * margin))
}
pub fn transcript_window_size(cx: &mut App) -> Option<Size<Pixels>> {
    let host = cx.try_global::<Registry>()?._host.clone();
    let panel = host.borrow().bubble?;
    panel
        .update(cx, |_, window, _| without_margin(window.viewport_size(), BUBBLE_SHADOW))
        .ok()
}
pub fn smoke_pointer_move(position: Point<Pixels>, cx: &mut App) {
    smoke_pointer_event(
        false,
        PlatformInput::MouseMove(MouseMoveEvent {
            position,
            ..Default::default()
        }),
        cx,
    );
}
/// Feed the same platform events AppKit tracking areas send to the real panels.
/// Do not borrow the root entity while dispatching: its painted listeners update it.
pub fn smoke_pointer_event(bubble: bool, event: PlatformInput, cx: &mut App) {
    let host = cx.global::<Registry>()._host.clone();
    let panel: AnyWindowHandle = if bubble {
        host.borrow()
            .bubble
            .expect("Transcript bubble exists")
            .into()
    } else {
        host.borrow().nub.expect("Recording Nub exists").into()
    };
    panel
        .update(cx, |_, window, cx| {
            window.dispatch_event(event, cx);
        })
        .expect("Recording panel accepts platform pointer input");
}

#[derive(serde::Serialize, serde::Deserialize)]
struct SavedPosition {
    x: f32,
    y: f32,
}
fn position_path() -> Option<PathBuf> {
    if let Some(directory) = crate::environment::var_os("CEREAL_DATA_DIR").filter(|p| !p.is_empty())
    {
        return Some(PathBuf::from(directory).join("pill-position.json"));
    }
    Some(
        PathBuf::from(std::env::var_os("HOME")?)
            .join("Library/Application Support/Cereal/pill-position.json"),
    )
}
fn load_position(cx: &App) -> Option<Bounds<Pixels>> {
    let saved: SavedPosition =
        serde_json::from_slice(&std::fs::read(position_path()?).ok()?).ok()?;
    if !saved.x.is_finite() || !saved.y.is_finite() {
        return None;
    }
    let bounds = Bounds::new(
        point(px(saved.x), px(saved.y)),
        size(px(NUB_SIZE.0), px(NUB_SIZE.1)),
    );
    // Wherever it was saved (a display that has since gone, a larger one,
    // a different arrangement, a Dock that has moved), it comes back on a
    // screen that exists and clear of the menu bar and the Dock.
    resting_place(bounds, &screens(cx)).map(|(bounds, _)| bounds)
}

/// A screen's geometry: its whole frame, and the part of it a window can rest in without
/// sitting under the menu bar or behind the Dock.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Desk {
    frame: Bounds<Pixels>,
    visible: Bounds<Pixels>,
}

fn screens(cx: &App) -> Vec<Desk> {
    let native: Vec<Desk> = native::screens()
        .into_iter()
        .map(|(frame, visible)| Desk { frame, visible })
        .collect();
    if !native.is_empty() {
        return native;
    }
    cx.displays()
        .into_iter()
        .map(|display| Desk {
            frame: display.bounds(),
            visible: display.bounds(),
        })
        .collect()
}

/// The display whose frame is `frame`.
fn display_of(frame: Bounds<Pixels>, cx: &App) -> Option<DisplayId> {
    cx.displays()
        .into_iter()
        .find(|display| display.bounds().contains(&frame.center()))
        .map(|display| display.id())
}

/// Where the capsule comes to rest when it is let go at `bounds`, or is put
/// back there from a saved position: inside the visible area of the screen
/// its centre is on, or of the nearest screen if it is on none. `None` only
/// when there are no screens.
fn resting_place(bounds: Bounds<Pixels>, screens: &[Desk]) -> Option<(Bounds<Pixels>, Desk)> {
    let center = bounds.center();
    let distance = |screen: &Desk| {
        let frame = screen.frame;
        let x = f32::from(center.x - center.x.max(frame.left()).min(frame.right()));
        let y = f32::from(center.y - center.y.max(frame.top()).min(frame.bottom()));
        x * x + y * y
    };
    let screen = *screens
        .iter()
        .min_by(|a, b| distance(a).total_cmp(&distance(b)))?;
    Some((clamp_nub(bounds, screen.visible), screen))
}
fn save_position_later(weak: &WeakHost, position: Bounds<Pixels>, cx: &mut App) {
    let weak = weak.clone();
    cx.spawn(async move |cx| {
        cx.background_executor()
            .timer(Duration::from_millis(250))
            .await;
        let _ = cx.update(|_| {
            let Some(host) = weak.upgrade() else { return };
            if host.borrow().position != Some(position) {
                return;
            }
            let Some(path) = position_path() else { return };
            let Some(directory) = path.parent() else {
                return;
            };
            let Ok(data) = serde_json::to_vec(&SavedPosition {
                x: f32::from(position.origin.x),
                y: f32::from(position.origin.y),
            }) else {
                return;
            };
            if std::fs::create_dir_all(directory).is_ok() {
                let temp = directory.join(format!(".pill-position-{}.tmp", std::process::id()));
                if std::fs::write(&temp, data).is_ok() {
                    let _ = std::fs::rename(temp, path);
                }
            }
        });
    })
    .detach();
}

fn schedule_sync(weak: &WeakHost, cx: &mut App) {
    let weak = weak.clone();
    cx.defer(move |cx| {
        if let Some(host) = weak.upgrade() {
            sync(&host, cx);
        }
    });
}

/// What the capsule itself shows. Read on every change of the main window,
/// so it copies nothing but four small values.
#[derive(Clone, Copy, PartialEq)]
struct Live {
    phase: Phase,
    attention: TranscriptionAttention,
    note_id: u64,
    audio_level: f32,
    /// Changes whenever the transcript the bubble shows may have changed:
    /// the note's stored text, or the live lines and status after it.
    transcript: (u64, usize, usize, u64),
}

fn live(host: &Host, cx: &App) -> Option<Live> {
    let model = host.model.upgrade()?;
    let model = model.read(cx);
    if model.screen == Screen::Login || !model.recording.is_active() {
        return None;
    }
    let id = model.recording.note_id?;
    let note = model.workspace.note(id)?;
    if note.trashed {
        return None;
    }
    Some(Live {
        phase: model.recording.phase,
        attention: model.recording.transcription_attention,
        note_id: id,
        audio_level: model.recording.audio_level(),
        transcript: (
            note.transcript_revision,
            note.transcript.len(),
            note.server_transcript_segments.len(),
            model.live.revision(),
        ),
    })
}

fn sync(host: &SharedHost, cx: &mut App) {
    let state = live(&host.borrow(), cx);
    let visible = {
        let h = host.borrow();
        // "Live meeting indicator" in Settings turns the capsule off; a
        // review fixture always shows it.
        state.is_some()
            && (h.force_preview
                || (!h.active && crate::desktop_preferences::meeting().live_indicator))
    };
    if !visible {
        close_all(host, cx);
        return;
    }
    if host.borrow().nub.is_none() {
        let (bounds, display, review) = {
            let h = host.borrow();
            (
                h.position
                    .and_then(|position| resting_place(position, &screens(cx)))
                    .map_or_else(|| nub_bounds(h.available), |(position, _)| position),
                h.display,
                h.force_preview,
            )
        };
        let weak = Rc::downgrade(host);
        let frame = bounds.dilate(px(NUB_SHADOW));
        if let Ok(handle) =
            cx.open_window(panel_options(frame, display, review), move |window, cx| {
                // The popup's window keeps a wide margin for its shadow,
                // which lies over the capsule: the capsule stays in front so
                // it takes its own clicks and drags.
                native::configure(window, true);
                native::track(window, NUB_SHADOW);
                window.set_window_title("Nub");
                cx.new(|_| Nub {
                    host: weak,
                    drag: None,
                    stage: Default::default(),
                })
            })
        {
            host.borrow_mut().nub = Some(handle);
        }
    }
    let (nub, bubble, expanded) = {
        let h = host.borrow();
        (h.nub, h.bubble, h.review_expanded)
    };
    // The main window notifies for every keystroke and animation frame. The
    // panels redraw only when what they show has changed.
    let shown = state.map(|live| {
        (
            live.phase,
            live.attention,
            host.borrow().hover_nub,
            voice_bar_targets(live.audio_level),
        )
    });
    let transcript = state.map(|live| (live.phase, live.attention, live.transcript));
    let dark = Some(crate::ui::is_dark());
    let (nub_changed, bubble_changed) = {
        let mut h = host.borrow_mut();
        // Both panels follow the app's theme.
        let themed = std::mem::replace(&mut h.shown_dark, dark) != dark;
        (
            (std::mem::replace(&mut h.shown_nub, shown) != shown) | themed,
            (std::mem::replace(&mut h.shown_bubble, transcript) != transcript) | themed,
        )
    };
    if let Some(w) = nub.filter(|_| nub_changed) {
        let _ = w.update(cx, |_, _, cx| cx.notify());
    }
    if let Some(w) = bubble.filter(|_| bubble_changed) {
        let _ = w.update(cx, |_, _, cx| cx.notify());
    }
    if expanded && bubble.is_none() {
        open_bubble(host, cx);
    }
}

/// Pointer tracking for the panels.
///
/// The panels belong to an app that is usually inactive while they show, so
/// each one carries an AppKit tracking area (`NSTrackingActiveAlways`) whose
/// enter and exit events re-read where the pointer really is. Nothing runs
/// while the pointer is elsewhere: no timer, no wake-ups.
///
/// AppKit does not deliver those events in every case (a cursor warp, the end
/// of a native drag), so while a panel believes it is hovered a 4 Hz check
/// confirms it and stops as soon as the pointer has gone. This needs no
/// Accessibility permission and never generates input events.
#[cfg(target_os = "macos")]
fn install_tracking(weak: WeakHost, cx: &mut App) {
    // The deterministic smoke workflow supplies platform events itself.
    if smoke_test() {
        return;
    }
    let async_cx = cx.to_async();
    native::on_crossing(Box::new(move || {
        let weak = weak.clone();
        // Queued, never run inside the event that delivered it.
        async_cx
            .spawn(async move |cx| {
                let _ = cx.update(|cx| refresh_hover(&weak, cx));
            })
            .detach();
    }));
}

fn smoke_test() -> bool {
    static SMOKE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *SMOKE.get_or_init(|| std::env::args().any(|arg| arg == "--smoke-test"))
}

/// Set each panel's hover from the pointer's actual position.
#[cfg(target_os = "macos")]
fn refresh_hover(weak: &WeakHost, cx: &mut App) {
    let Some(host) = weak.upgrade() else { return };
    let (nub, bubble) = {
        let h = host.borrow();
        (h.nub, h.bubble)
    };
    if let Some(nub) = nub
        && let Ok(Some(inside)) = nub.update(cx, |nub, window, _| {
            nub.drag
                .is_none()
                .then(|| native::pointer_inside(window, NUB_SHADOW))
                .flatten()
        })
    {
        set_hover(weak, false, inside, cx);
    }
    if let Some(bubble) = bubble
        && let Ok(Some(inside)) =
            bubble.update(cx, |_, window, _| native::pointer_inside(window, BUBBLE_SHADOW))
    {
        set_hover(weak, true, inside, cx);
    }
}

/// While a panel is hovered, confirm it four times a second so a missed exit
/// cannot leave the bubble open. Ends itself when nothing is hovered.
#[cfg(target_os = "macos")]
fn watch_hover(weak: &WeakHost, cx: &mut App) {
    let Some(host) = weak.upgrade() else { return };
    if smoke_test() || std::mem::replace(&mut host.borrow_mut().watching, true) {
        return;
    }
    let weak = weak.clone();
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor()
                .timer(Duration::from_millis(250))
                .await;
            let hovered = cx
                .update(|cx| {
                    refresh_hover(&weak, cx);
                    weak.upgrade().is_some_and(|host| {
                        let h = host.borrow();
                        h.nub.is_some() && (h.hover_nub || h.hover_bubble)
                    })
                })
                .unwrap_or(false);
            if !hovered {
                if let Some(host) = weak.upgrade() {
                    host.borrow_mut().watching = false;
                }
                break;
            }
        }
    })
    .detach();
}

#[cfg(not(target_os = "macos"))]
fn watch_hover(_: &WeakHost, _: &mut App) {}

/// A pointer event reached one of the panels. Their windows overlap (each
/// keeps a margin for its shadow), so the event may have gone to the one
/// whose margin is on top while the pointer is over the other's shape: ask
/// AppKit where the pointer really is for both, once the event is done with.
fn pointer_moved(weak: &WeakHost, cx: &mut App) {
    #[cfg(target_os = "macos")]
    if !smoke_test() {
        let weak = weak.clone();
        cx.defer(move |cx| refresh_hover(&weak, cx));
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (weak, cx);
}

fn panel_options(
    bounds: Bounds<Pixels>,
    display_id: Option<DisplayId>,
    focus: bool,
) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: None,
        kind: WindowKind::PopUp,
        focus,
        show: true,
        is_resizable: false,
        is_minimizable: false,
        is_movable: true,
        display_id,
        window_background: WindowBackgroundAppearance::Transparent,
        app_id: Some("dev.cereal.desktop".into()),
        ..Default::default()
    }
}

fn close_bubble(host: &SharedHost, cx: &mut App) {
    let panel = host.borrow_mut().bubble.take();
    let Some(panel) = panel else { return };
    // The bubble folds back toward the capsule, then its window goes.
    let leaving = panel
        .update(cx, |bubble, _, cx| {
            bubble.leaving = true;
            cx.notify();
        })
        .is_ok();
    if !leaving {
        return;
    }
    let exit = if crate::motion::reduce_motion() {
        crate::motion::REDUCED_FADE
    } else {
        crate::motion::GOOEY.exit_duration()
    };
    cx.spawn(async move |cx| {
        cx.background_executor()
            .timer(exit + Duration::from_millis(30))
            .await;
        let _ = panel.update(cx, |_, window, _| window.remove_window());
    })
    .detach();
}
fn close_all(host: &SharedHost, cx: &mut App) {
    let nub = {
        let mut h = host.borrow_mut();
        h.hover_epoch = h.hover_epoch.wrapping_add(1);
        h.hover_nub = false;
        h.hover_bubble = false;
        h.nub.take()
    };
    close_bubble(host, cx);
    if let Some(nub) = nub {
        let _ = nub.update(cx, |_, window, _| window.remove_window());
    }
}

fn set_hover(weak: &WeakHost, bubble: bool, hovered: bool, cx: &mut App) {
    let Some(host) = weak.upgrade() else { return };
    let epoch = {
        let mut h = host.borrow_mut();
        if bubble {
            if h.hover_bubble == hovered {
                return;
            }
            h.hover_bubble = hovered;
        } else {
            if h.hover_nub == hovered {
                return;
            }
            h.hover_nub = hovered;
        }
        h.hover_epoch = h.hover_epoch.wrapping_add(1);
        h.hover_epoch
    };
    // GPUI retains its last hit-test position on MouseExited. Repaint using our
    // native tracking state so the capsule highlight clears on a real exit.
    schedule_sync(weak, cx);
    if hovered {
        watch_hover(weak, cx);
    }
    let weak = weak.clone();
    cx.spawn(async move |cx| {
        cx.background_executor()
            .timer(Duration::from_millis(if hovered { 150 } else { 100 }))
            .await;
        let _ = cx.update(|cx| {
            let Some(host) = weak.upgrade() else { return };
            let (current, wanted) = {
                let h = host.borrow();
                (
                    h.hover_epoch,
                    h.hover_nub || h.hover_bubble || h.review_expanded,
                )
            };
            if current != epoch {
                return;
            }
            if wanted {
                open_bubble(&host, cx);
            } else {
                close_bubble(&host, cx);
            }
        });
    })
    .detach();
}

fn open_bubble(host: &SharedHost, cx: &mut App) {
    if host.borrow().bubble.is_some() || live(&host.borrow(), cx).is_none() {
        return;
    }
    let (nub, available, display, review) = {
        let h = host.borrow();
        (h.nub, h.available, h.display, h.review_expanded)
    };
    let Some(nub) = nub else { return };
    let Ok(bounds) = nub.update(cx, |_, window, _| window.bounds()) else {
        return;
    };
    let bounds = bubble_bounds(bounds.inset(px(NUB_SHADOW)), available).dilate(px(BUBBLE_SHADOW));
    let weak = Rc::downgrade(host);
    if let Ok(panel) = cx.open_window(panel_options(bounds, display, review), move |window, cx| {
        native::configure(window, false);
        native::track(window, BUBBLE_SHADOW);
        window.set_window_title("Nub transcript");
        cx.new(|_| {
            // Opened by hand, so the entrance is started by hand.
            let stage = crate::motion::Stage::default();
            stage.kick(("bubble", 0), 0., crate::motion::GOOEY);
            TranscriptBubble {
                host: weak,
                scroll: ScrollHandle::new(),
                stored: None,
                followed: None,
                stage,
                leaving: false,
                copied: 0,
                copied_shown: false,
            }
        })
    }) {
        host.borrow_mut().bubble = Some(panel);
    }
}

/// For the functional log: where the pill is and what it is doing, in one
/// line. Reads state only.
pub fn describe(cx: &mut App) -> String {
    let Some(host) = cx.try_global::<Registry>().map(|r| r._host.clone()) else {
        return "no pill".into();
    };
    let (nub, bubble, active, hover_nub, hover_bubble, saved) = {
        let h = host.borrow();
        (h.nub, h.bubble, h.active, h.hover_nub, h.hover_bubble, h.position)
    };
    let place = |bounds: Bounds<Pixels>| {
        format!("{:.0},{:.0}", f32::from(bounds.origin.x), f32::from(bounds.origin.y))
    };
    let nub = nub.and_then(|nub| {
        nub.update(cx, |nub, window, _| {
            format!(
                "nub at {}{}",
                place(window.bounds().inset(px(NUB_SHADOW))),
                match &nub.drag {
                    Some(drag) if drag.moved => " (being dragged)",
                    Some(_) => " (pressed)",
                    None => "",
                }
            )
        })
        .ok()
    });
    let bubble = bubble
        .and_then(|bubble| {
            bubble
            .update(cx, |_, window, _| {
                format!("popup at {}", place(window.bounds().inset(px(BUBBLE_SHADOW))))
            })
            .ok()
    });
    format!(
        "{}; {}; pointer over nub {hover_nub}, over popup {hover_bubble}; main window active {active}; saved position {}",
        nub.unwrap_or_else(|| "no nub".into()),
        bubble.unwrap_or_else(|| "no popup".into()),
        saved.map_or("none".into(), place)
    )
}

/// For the live probe: keep the pill and its transcript popup on screen
/// beside the main window, as the review fixtures do, so that both can be
/// captured while a real recording runs.
pub fn hold_open(cx: &mut App) {
    let Some(host) = cx.try_global::<Registry>().map(|r| r._host.clone()) else {
        return;
    };
    {
        let mut host = host.borrow_mut();
        host.force_preview = true;
        host.review_expanded = true;
    }
    schedule_sync(&Rc::downgrade(&host), cx);
}

fn reveal_note(weak: &WeakHost, cx: &mut App) {
    let Some(host) = weak.upgrade() else { return };
    let Some(state) = live(&host.borrow(), cx) else {
        return;
    };
    let id = state.note_id;
    let main = host.borrow().main;
    host.borrow_mut().force_preview = false;
    host.borrow_mut().review_expanded = false;
    cx.defer(move |cx| {
        let _ = main.update(cx, |view, window, cx| {
            view.open_note(id, cx);
            reveal_main_window(window);
            window.activate_window();
            cx.activate(true);
        });
    });
}

struct Nub {
    host: WeakHost,
    drag: Option<PillDrag>,
    stage: crate::motion::Stage,
}
/// A press on the capsule that may become a drag.
///
/// The drag is worked out from where the pointer really is on the screen
/// (`grab` is the pointer's offset from the window's corner at the press),
/// never from an event's position inside the window or from the window
/// bounds the UI framework last heard about: both are stale by the time a
/// queued event is handled, and during a fast drag the capsule fell hundreds
/// of pixels behind the pointer and stayed there.
struct PillDrag {
    pressed: (f64, f64),
    grab: (f64, f64),
    /// Where the press was by the framework's account, for the platforms
    /// with no native pointer to ask.
    origin: Point<Pixels>,
    pointer: Point<Pixels>,
    moved: bool,
}
impl Nub {
    fn hover_pointer(
        &self,
        position: Point<Pixels>,
        viewport: Size<Pixels>,
        cx: &mut Context<Self>,
    ) {
        if self.drag.is_none() {
            set_hover(&self.host, false, pointer_inside(position, viewport, NUB_SHADOW), cx);
            pointer_moved(&self.host, cx);
        }
    }
    fn move_pointer(&mut self, event: &MouseMoveEvent, window: &mut Window) {
        let Some(drag) = self.drag.as_mut() else {
            return;
        };
        if event.pressed_button != Some(MouseButton::Left) {
            return;
        }
        if let Some(now) = native::pointer() {
            let (dx, dy) = (now.0 - drag.pressed.0, now.1 - drag.pressed.1);
            drag.moved |= dx * dx + dy * dy > 9.;
            if drag.moved {
                native::set_origin(window, (now.0 - drag.grab.0, now.1 - drag.grab.1));
            }
            return;
        }
        let pointer = window.bounds().origin + event.position;
        drag.moved |= is_drag(drag.pointer, pointer);
        if drag.moved {
            native::move_to(window, drag.origin + (pointer - drag.pointer));
        }
    }
    fn release_pointer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(drag) = self.drag.take() else { return };
        // Let go: the capsule springs back out of its squash.
        cx.notify();
        let Some(host) = self.host.upgrade() else {
            return;
        };
        if drag.moved {
            // Where the pointer is now, in case the last move is still queued.
            if let Some(now) = native::pointer() {
                native::set_origin(window, (now.0 - drag.grab.0, now.1 - drag.grab.1));
            }
            let bounds = native::bounds(window)
                .unwrap_or_else(|| window.bounds())
                .inset(px(NUB_SHADOW));
            // It may be dragged anywhere; it rests clear of the menu bar and
            // the Dock, on the screen it was let go over.
            let rest = resting_place(bounds, &screens(cx));
            let available = rest.map_or(host.borrow().available, |(_, screen)| screen.visible);
            let now = rest.map_or_else(|| clamp_nub(bounds, available), |(now, _)| now);
            if now.origin != bounds.origin {
                native::place(window, now.origin - point(px(NUB_SHADOW), px(NUB_SHADOW)));
            }
            {
                let mut h = host.borrow_mut();
                h.position = Some(now);
                h.available = available;
                if let Some(display) = rest.and_then(|(_, screen)| display_of(screen.frame, cx)) {
                    h.display = Some(display);
                }
            }
            save_position_later(&self.host, now, cx);
        } else {
            reveal_note(&self.host, cx);
        }
    }
}
impl Render for Nub {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use crate::motion::SNAPPY;
        self.stage.begin();
        let rim = nub_rim(window);
        let pointer_events = nub_pointer_events(cx.weak_entity());
        let (phase, attention, hovered, level) = self
            .host
            .upgrade()
            .and_then(|h| {
                let h = h.borrow();
                live(&h, cx).map(|s| (s.phase, s.attention, h.hover_nub, s.audio_level))
            })
            .unwrap_or((Phase::Idle, TranscriptionAttention::None, false, 0.));
        let heights = voice_bar_targets(level);
        let badge = self.stage.presence(
            ("nub-attention", 0),
            attention == TranscriptionAttention::Warning,
            crate::motion::GOOEY,
        );
        // The hover tint eases in, and the capsule squashes while it is held
        // (including for the whole of a drag) and springs back on release.
        let lit = self
            .stage
            .aim(("nub-hover", 0), if hovered { 1. } else { 0. }, SNAPPY)
            .clamp(0., 1.);
        let held = self.stage.aim(
            ("nub-press", 0),
            if self.drag.is_some() { 1. } else { 0. },
            SNAPPY,
        );
        let skin = skin();
        let mark = Hsla::from(skin.ink).opacity(if skin.dark { 0.86 } else { 0.8 });
        let nub = div()
            .relative()
            .size_full()
            .child(pointer_events)
            .p(px(INSET + NUB_SHADOW))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .id("desktop-recording-nub")
                    .relative()
                    .w(px(34. - 1.4 * held))
                    .h(px(62. - 2.5 * held))
                    .rounded_full()
                    .border(px(rim))
                    .border_color(skin.edge)
                    .bg(blend(skin.capsule, skin.capsule_lit, lit))
                    .shadow(lifted(NUB_SHADOW + INSET, skin.dark))
                    .flex()
                    .flex_col()
                    .items_center()
                    // The mark as far from the top as the tallest level bars
                    // come from the bottom, so the pair sits centred whether
                    // the room is quiet or loud.
                    .pt(px(11. - rim))
                    .gap(px(6.))
                    .cursor(CursorStyle::Arrow)
                    .child(icon("cereal", 16., mark).mb(px(2.)))
                    .child(nub_indicator(phase, attention, heights, skin))
                    .when(badge.visible, |d| {
                        d.child(crate::motion::pop(
                            div()
                                .id("nub-transcription-attention")
                                .absolute()
                                .top(px(-4.))
                                .right(px(-4.))
                                .size(px(18.))
                                .rounded_full()
                                .bg(rgb(0xed9212))
                                .flex()
                                .items_center()
                                .justify_center()
                                .font_family(FONT)
                                .font_weight(FontWeight::BOLD)
                                .text_size(px(13.))
                                .line_height(px(18.))
                                .text_color(rgb(0xffffff))
                                .child("!"),
                            crate::motion::Pop::BLOB,
                            badge,
                        ))
                    })
                    .child(
                        div()
                            .id("nub-whole-surface-hit-target")
                            .absolute()
                            .inset_0()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                                    let Some(host) = this.host.upgrade() else {
                                        return;
                                    };
                                    {
                                        let mut h = host.borrow_mut();
                                        h.hover_epoch = h.hover_epoch.wrapping_add(1);
                                        h.hover_nub = false;
                                        h.hover_bubble = false;
                                    }
                                    close_bubble(&host, cx);
                                    // Where on the capsule it was taken hold
                                    // of comes from the press itself: by the
                                    // time the press is handled the pointer
                                    // may already have moved on.
                                    let corner = native::origin(window).unwrap_or_default();
                                    let grab = (
                                        f64::from(f32::from(event.position.x)),
                                        f64::from(f32::from(window.bounds().size.height - event.position.y)),
                                    );
                                    this.drag = Some(PillDrag {
                                        pressed: (corner.0 + grab.0, corner.1 + grab.1),
                                        grab,
                                        origin: window.bounds().origin,
                                        pointer: window.bounds().origin + event.position,
                                        moved: false,
                                    });
                                    cx.notify();
                                }),
                            ),
                    ),
            );
        self.stage.flush_as("nub", window);
        nub
    }
}

fn nub_pointer_events(entity: WeakEntity<Nub>) -> AnyElement {
    canvas(
        |_, _, _| (),
        move |_, _, window, _| {
            let weak = entity.clone();
            window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                // Accessibility input may deliver a click at a new pointer
                // position without a preceding MouseMove. Treat that position
                // as entry too; only the primary button begins a drag.
                if phase == DispatchPhase::Bubble
                    && event.button != MouseButton::Left
                    && pointer_inside(event.position, window.viewport_size(), NUB_SHADOW)
                {
                    let _ = weak.update(cx, |this, cx| set_hover(&this.host, false, true, cx));
                }
            });
            let weak = entity.clone();
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                if phase == DispatchPhase::Bubble {
                    let _ = weak.update(cx, |this, cx| {
                        this.move_pointer(event, window);
                        this.hover_pointer(event.position, window.viewport_size(), cx);
                    });
                }
            });
            let weak = entity.clone();
            window.on_mouse_event(move |_: &MouseExitEvent, phase, _, cx| {
                if phase == DispatchPhase::Bubble {
                    let _ = weak.update(cx, |this, cx| set_hover(&this.host, false, false, cx));
                }
            });
            let weak = entity.clone();
            window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
                if phase == DispatchPhase::Bubble && event.button == MouseButton::Left {
                    let _ = weak.update(cx, |this, cx| this.release_pointer(window, cx));
                }
            });
        },
    )
    .absolute()
    .size_full()
    .into_any_element()
}

/// The hairline just inside the edge of the capsule and of the popup: half
/// a point, which is one device pixel on Retina. At 1x it is a whole pixel,
/// as a browser would draw it, so it shows on every side.
fn nub_rim(window: &Window) -> f32 {
    crate::ui::hairline_at(0.5, window.scale_factor())
}
/// The popup's corner radius, and that of the transcript rows inside it.
/// The rows sit `ROW_MARGIN` in from the popup's edge, so the two curves
/// share a centre.
const BUBBLE_RADIUS: f32 = 18.;
const ROW_MARGIN: f32 = 8.;
const ROW_RADIUS: f32 = BUBBLE_RADIUS - ROW_MARGIN;
/// Corner of a row where it meets another from the same speaker.
const ROW_JOINED: f32 = 4.;
/// The rows scroll in a viewport this far inside the popup's top and bottom
/// edges: clipping is rectangular, and at `ROW_MARGIN` from the side the
/// popup's corner curve is 3 pt down.
const VIEWPORT_INSET: f32 = 4.;
/// How far the rows take to dissolve under the popup's top and bottom edges.
const FADE: f32 = 28.;
/// The popup's header: its title and the copy button.
const HEADER: f32 = 36.;

/// The panels' colours, in the app's theme. Every fill is opaque: these
/// float over other apps, and anything let through reads as dirt.
#[derive(Clone, Copy)]
struct Skin {
    dark: bool,
    /// The popup's surface, and the capsule's at rest and under the pointer.
    surface: Rgba,
    capsule: Rgba,
    capsule_lit: Rgba,
    /// The hairline inside the edge: lighter than the surface in the dark
    /// theme, darker in the light one.
    edge: Rgba,
    ink: Rgba,
    muted: Rgba,
    warning: Rgba,
    /// Other people's words (left) and the user's own (right, a quiet gold).
    theirs: Rgba,
    own: Rgba,
    own_ink: Rgba,
}

fn skin() -> Skin {
    if crate::ui::is_dark() {
        Skin {
            dark: true,
            surface: rgb(0x212120),
            capsule: rgb(0x262625),
            capsule_lit: rgb(0x3a3a38),
            edge: rgba(0xffffff24),
            ink: rgb(0xf7f7f2),
            muted: rgb(0x9e9e99),
            warning: rgb(0xf0a441),
            theirs: rgb(0x383836),
            own: rgb(0x453c22),
            own_ink: rgb(0xfcf2df),
        }
    } else {
        Skin {
            dark: false,
            surface: rgb(0xfdfdfb),
            capsule: rgb(0xfdfdfb),
            capsule_lit: rgb(0xf0f0ea),
            edge: rgba(0x1f1c0f24),
            ink: rgb(0x292929),
            muted: rgb(0x72726e),
            warning: rgb(0x9a5b00),
            theirs: rgb(0xf0f0ea),
            own: rgb(0xf7e7b9),
            own_ink: rgb(0x363024),
        }
    }
}

/// The shadow a panel floats on: a tight dark line that keeps the edge
/// crisp against any backdrop, a close contact shadow, and a wide soft one.
/// `reach` is the window's margin around the shape; nothing falls outside it.
fn lifted(reach: f32, dark: bool) -> Vec<BoxShadow> {
    let black = |alpha: f32| hsla(0., 0., 0., alpha);
    let strength = if dark { 1. } else { 0.62 };
    // The blur is the Gaussian's deviation: it has faded out by three of them.
    let wide = reach / 4.;
    vec![
        BoxShadow {
            color: black(0.30 * strength),
            offset: point(px(0.), px(wide * 0.75)),
            blur_radius: px(wide),
            spread_radius: px(0.),
        },
        BoxShadow {
            color: black(0.22 * strength),
            offset: point(px(0.), px(1.)),
            blur_radius: px(1.5),
            spread_radius: px(0.),
        },
        BoxShadow {
            color: black(if dark { 0.55 } else { 0.16 }),
            offset: point(px(0.), px(0.)),
            blur_radius: px(0.),
            spread_radius: px(0.5),
        },
    ]
}

fn blend(a: Rgba, b: Rgba, t: f32) -> Rgba {
    Rgba {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: a.a + (b.a - a.a) * t,
    }
}

/// The veils the rows dissolve under at the top and bottom of the popup,
/// each only as strong as there is transcript scrolled past that edge.
/// Painted over the rows, and read from the scroll position at paint time,
/// so it is right on the very frame a jump to the newest line lands.
fn scroll_fades(scroll: ScrollHandle, surface: Rgba, radius: (f32, f32)) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let above = -f32::from(scroll.offset().y);
            let below = f32::from(scroll.max_offset().height) - above;
            let surface = Hsla::from(surface);
            let height = px(FADE.min(f32::from(bounds.size.height) / 2.));
            let solid = VIEWPORT_INSET / FADE;
            for (top, hidden) in [(true, above), (false, below)] {
                let strength = (hidden / 12.).clamp(0., 1.);
                if strength <= 0. {
                    continue;
                }
                let origin = if top {
                    bounds.origin
                } else {
                    point(bounds.origin.x, bounds.bottom() - height)
                };
                let curve = px(if top { radius.0 } else { radius.1 });
                let none = px(0.);
                window.paint_quad(
                    fill(
                        Bounds::new(origin, size(bounds.size.width, height)),
                        linear_gradient(
                            if top { 180. } else { 0. },
                            linear_color_stop(surface.opacity(strength), solid),
                            linear_color_stop(surface.opacity(0.), 1.),
                        ),
                    )
                    .corner_radii(if top {
                        Corners { top_left: curve, top_right: curve, bottom_left: none, bottom_right: none }
                    } else {
                        Corners { top_left: none, top_right: none, bottom_left: curve, bottom_right: curve }
                    }),
                );
            }
        },
    )
    .absolute()
    .inset_0()
}

fn nub_indicator(
    phase: Phase,
    attention: TranscriptionAttention,
    heights: [f32; 3],
    skin: Skin,
) -> AnyElement {
    // Spinner and voice bars hand over with a soft rise rather than a swap.
    if phase == Phase::Starting || attention == TranscriptionAttention::Warning {
        return crate::motion::swap_in(
            "nub-indicator-spinner",
            div()
                .w(px(20.))
                .h(px(20.))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    svg()
                        .path("icons/spinner.svg")
                        .size(px(12.))
                        .text_color(Hsla::from(skin.ink).opacity(0.45))
                        .with_animation(
                            "nub-starting-spinner",
                            Animation::new(Duration::from_secs(1)).repeat(),
                            |el, p| {
                                el.with_transformation(Transformation::rotate(percentage(
                                    crate::motion::fluid_turn(p).clamp(0., 1.),
                                )))
                            },
                        ),
                ),
        );
    }
    crate::motion::swap_in(
        "nub-indicator-voice",
        div()
            .w(px(12.))
            .h(px(20.))
            // 2px bars on a 5px pitch. They ease to each new level and
            // stop redrawing once the input is steady: no perpetual timer.
            .child(crate::motion::bars(
                "nub-voice-waveform",
                heights,
                2.,
                3.,
                20.,
                // The reference green; a shade deeper on the light capsule.
                if skin.dark { crate::recording_view::WAVEFORM.into() } else { rgb(0x5c9900).into() },
            )),
    )
}

/// The installed Nub uses 7px mirrored halves and a 3px quiet floor. Real
/// level changes drive the visualizer; quiet does not invent speech.
pub(crate) fn voice_bar_targets(level: f32) -> [f32; 3] {
    // The capsule's halves are 7 pt: up to 14 pt bars.
    crate::recording_view::level_bars(level, 7.)
}

struct TranscriptBubble {
    host: WeakHost,
    scroll: ScrollHandle,
    /// The recording note's stored transcript as rows, rebuilt only when the
    /// note's transcript changes: not per frame, and not per interim update.
    stored: Option<(StoredKey, Rc<Vec<TranscriptSegment>>)>,
    /// The transcript state the view last followed to the bottom.
    followed: Option<(StoredKey, u64)>,
    stage: crate::motion::Stage,
    /// Set when the bubble has been closed and is animating out; its window
    /// is removed when the exit has played.
    leaving: bool,
    /// Bumped by each press of the copy button; the button says "Copied"
    /// until `COPIED_FOR` after the latest one. This window has no toast.
    copied: u64,
    copied_shown: bool,
}
/// How long the copy button confirms a copy.
const COPIED_FOR: Duration = Duration::from_millis(1600);

/// The transcript as plain text, one paragraph per row, each run of rows
/// from one voice headed by who spoke ("Me" for the microphone).
fn transcript_copy(rows: &[TranscriptSegment]) -> String {
    let mut out = String::new();
    let mut last: Option<&str> = None;
    for row in rows.iter().filter(|row| !row.text.trim().is_empty()) {
        if last != Some(row.speaker.as_str()) {
            let name = row
                .label
                .as_deref()
                .filter(|label| !label.trim().is_empty())
                .unwrap_or(if row.own { "Me" } else { "Them" });
            if !out.is_empty() {
                out.push_str("\n\n");
            }
            out.push_str(name);
            out.push_str(": ");
            last = Some(row.speaker.as_str());
        } else {
            out.push(' ');
        }
        out.push_str(row.text.trim());
    }
    out
}

impl TranscriptBubble {
    fn copy(&mut self, rows: &[TranscriptSegment], cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(transcript_copy(rows)));
        self.copied = self.copied.wrapping_add(1);
        self.copied_shown = true;
        let press = self.copied;
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(COPIED_FOR).await;
            let _ = this.update(cx, |this, cx| {
                if this.copied == press {
                    this.copied_shown = false;
                    cx.notify();
                }
            });
        })
        .detach();
    }
}

/// The popup's copy button: a small capsule at its top-right corner on the
/// popup's own surface. After a press it shows a tick and "Copied", and
/// widens to fit, until `COPIED_FOR` has passed.
fn copy_button(skin: Skin, copied: bool, stage: &crate::motion::Stage) -> Stateful<Div> {
    let shown = stage
        .aim(("nub-copied", 0), if copied { 1. } else { 0. }, crate::motion::SNAPPY)
        .clamp(0., 1.);
    let ink: Hsla = if copied { skin.ink } else { skin.muted }.into();
    div()
        .id("nub-transcript-copy")
        .h(px(24.))
        .min_w(px(24.))
        .px(px(if copied { 8. } else { 0. }))
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .gap(px(4.))
        .bg(skin.surface)
        .border(px(0.5))
        .border_color(skin.edge)
        .cursor(CursorStyle::PointingHand)
        .hover(|d| d.bg(if skin.dark { skin.theirs } else { skin.capsule_lit }))
        .child(icon(if copied { "check" } else { "copy" }, 13., ink))
        .when(copied, |d| {
            d.child(
                div()
                    .text_size(px(11.))
                    .line_height(px(14.))
                    .text_color(ink)
                    .opacity(shown)
                    .child("Copied"),
            )
        })
}

impl Render for TranscriptBubble {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.stage.begin();
        let rim = nub_rim(window);
        let skin = skin();
        let pose = self
            .stage
            .presence(("bubble", 0), !self.leaving, crate::motion::GOOEY);
        let leaving = self.leaving;
        let weak = self.host.clone();
        // The bubble normally opens to the left of the capsule and flips to
        // the right at the screen edge.
        let beside_left = self.host.upgrade().is_none_or(|host| {
            let host = host.borrow();
            let nub = host.position.unwrap_or_else(|| nub_bounds(host.available));
            window.bounds().center().x <= nub.center().x
        });
        let pointer_events = canvas(
            |_, _, _| (),
            move |_, _, window, _| {
                let exit = weak.clone();
                window.on_mouse_event(move |_: &MouseExitEvent, phase, _, cx| {
                    if phase == DispatchPhase::Bubble {
                        set_hover(&exit, true, false, cx);
                    }
                });
                let weak = weak.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                    if phase == DispatchPhase::Bubble {
                        set_hover(
                            &weak,
                            true,
                            pointer_inside(event.position, window.viewport_size(), BUBBLE_SHADOW),
                            cx,
                        );
                        pointer_moved(&weak, cx);
                    }
                });
            },
        )
        .absolute()
        .size_full();
        // What the recording shows now. Nothing is copied out of the note
        // unless its stored transcript changed since the last frame.
        let model = self.host.upgrade().and_then(|host| host.borrow().model.upgrade());
        let mut phase = Phase::Idle;
        let mut attention = TranscriptionAttention::None;
        let mut speaking: Vec<TranscriptSegment> = Vec::new();
        let mut status = None;
        let mut listening = false;
        let mut seen = None;
        if let Some(model) = &model {
            let model = model.read(cx);
            let note = model
                .recording
                .note_id
                .filter(|_| model.screen != Screen::Login && model.recording.is_active())
                .and_then(|id| model.workspace.note(id))
                .filter(|note| !note.trashed);
            if let Some(note) = note {
                phase = model.recording.phase;
                attention = model.recording.transcription_attention;
                let key = (
                    note.id,
                    note.transcript_revision,
                    note.transcript.len(),
                    note.server_transcript_segments.len(),
                );
                if self.stored.as_ref().is_none_or(|(stored, _)| *stored != key) {
                    let rows = bubble_segments(&note.transcript, &note.server_transcript_segments);
                    self.stored = Some((key, Rc::new(rows)));
                }
                speaking = model.live.tail(note).iter().cloned().map(segment_of).collect();
                status = model.live.status(note.id);
                listening = model.live.listening(note.id);
                seen = Some((key, model.live.revision()));
            }
        }
        if seen.is_none() {
            self.stored = None;
        }
        let stored = self
            .stored
            .as_ref()
            .map(|(_, rows)| rows.clone())
            .unwrap_or_default();
        // Follow the live tail until the reader scrolls more than 48px up.
        if seen != self.followed {
            let near_bottom = self.scroll.max_offset().height + self.scroll.offset().y <= px(48.);
            if self.followed.is_none() || near_bottom {
                self.scroll.scroll_to_bottom();
                crate::lazy::to_end("nub-transcript");
            }
            self.followed = seen;
        }
        let count = stored.len() + speaking.len();
        let row = |index: usize| {
            index
                .checked_sub(stored.len())
                .map_or_else(|| stored.get(index), |live| speaking.get(live))
        };
        let several = (
            several_voices(stored.iter().chain(&speaking), true),
            several_voices(stored.iter().chain(&speaking), false),
        );
        let heading = |index: usize| -> Option<&str> {
            let segment = row(index)?;
            let before = index.checked_sub(1).and_then(row);
            speaker_heading(segment, before, if segment.own { several.0 } else { several.1 })
        };
        let mut content = div()
            .id("nub-transcript-content")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .flex()
            .flex_col()
            .py(px(1.));
        if count == 0 {
            content = content
                .items_center()
                .justify_center()
                .gap(px(8.))
                .px(px(16.))
                .text_size(px(13.))
                .text_color(skin.muted);
            content = content.child(if !crate::audio::preview_mode() {
                if phase == Phase::Starting {
                    "Connecting audio inputs…"
                } else if listening {
                    "Listening…"
                } else {
                    "Recording…"
                }
            } else if attention == TranscriptionAttention::Warning {
                "Transcript Paused"
            } else if phase == Phase::Starting {
                "Transcript starting…"
            } else if phase == Phase::Live {
                "Transcript on…"
            } else {
                "Transcript Paused"
            });
            if !crate::audio::preview_mode() {
                if !listening {
                    content = content.child(
                        div()
                            .text_size(px(12.))
                            .child("Stop recording to transcribe this segment"),
                    );
                }
            } else if phase == Phase::Live && attention == TranscriptionAttention::None {
                content =
                    content.child(
                        div()
                            .text_size(px(15.))
                            .text_color(skin.ink)
                            .child("Try saying “Hello Cereal”"),
                    );
            }
        } else {
            // Only the bubbles near the viewport are built (see `lazy`), so
            // an hour of transcript costs the same per frame as a minute.
            let same = |a: usize, b: usize| {
                row(a)
                    .zip(row(b))
                    .is_some_and(|(a, b)| a.speaker == b.speaker)
            };
            let keys: Vec<u64> = (0..count)
                .map(|index| {
                    let segment = row(index).expect("index is within the rows");
                    crate::lazy::key((
                        &segment.text,
                        segment.is_final,
                        index > 0 && same(index - 1, index),
                        same(index, index + 1),
                        heading(index).is_some(),
                    ))
                })
                .collect();
            // The popup's own width: its window is wider by the clear
            // space and the shadow's margin on each side.
            let width = f32::from(window.viewport_size().width) - 2. * (INSET + BUBBLE_SHADOW);
            let ids: Vec<u64> = (0..count)
                .map(|index| row(index).map_or(0, |segment| segment.id))
                .collect();
            // Rows that have not been on screen stand in the scroll range at
            // a guessed height (the numbers the rows below are laid out
            // with), so hovering the pill an hour into a meeting builds one
            // popup's worth of bubbles, not the meeting.
            let mut counter = crate::ui::LineCounter::new(13., cx);
            // The popup's rim, and a row's margins and indent, leave the
            // stack's width.
            let stack_width = width - 2. * rim - 2. * ROW_MARGIN - ROW_INDENT;
            let text_width = stack_width - 2. * ROW_PAD.0;
            let texts = window.text_system().clone();
            content = content.child(crate::lazy::estimated(
                "nub-transcript",
                &ids,
                &keys,
                width,
                0.,
                &self.scroll,
                window,
                |index| {
                    let Some(segment) = row(index) else { return 32. };
                    let lines = if segment.is_final {
                        counter.lines(&segment.text, text_width)
                    } else {
                        counter.lines(&format!("{} •••", segment.text), text_width)
                    };
                    lines as f32 * ROW_LINE
                        + 2. * ROW_PAD.1
                        + 2.
                        + if heading(index).is_some() { SPEAKER_LINE + 2. } else { 0. }
                        + if index > 0 && same(index - 1, index) { 0. } else { GROUP_GAP }
                        + if same(index, index + 1) { 0. } else { GROUP_GAP }
                },
                |i| {
                    let segment = row(i).expect("index is within the rows");
                    let same_before = i > 0 && same(i - 1, i);
                    let same_after = same(i, i + 1);
                    let own = segment.own;
                    let ink: Hsla = if own { skin.own_ink } else { skin.ink }.into();
                    // Words still being heard are quieter, on a fill that
                    // has not quite arrived. Mixed rather than see-through,
                    // so a draft row stays a clean shape.
                    let fill = if own { skin.own } else { skin.theirs };
                    let (ink, fill) = if segment.is_final {
                        (ink, fill)
                    } else {
                        (ink.opacity(0.62), blend(skin.surface, fill, 0.6))
                    };
                    let corner = |joined: bool| px(if joined { ROW_JOINED } else { ROW_RADIUS });
                    let paragraph = div()
                        .max_w_full()
                        .min_w_0()
                        .px(px(ROW_PAD.0))
                        .py(px(ROW_PAD.1))
                        // Rows from one speaker close up along the side they
                        // hang from.
                        .rounded_tl(corner(same_before && !own))
                        .rounded_tr(corner(same_before && own))
                        .rounded_bl(corner(same_after && !own))
                        .rounded_br(corner(same_after && own))
                        .text_size(px(13.))
                        .line_height(px(ROW_LINE))
                        .bg(fill)
                        .text_color(ink)
                        .child(transcript_text(
                            &segment.text,
                            segment.is_final,
                            text_width,
                            ink,
                            &texts,
                        ));
                    let mut stack = div()
                        .w(px(stack_width.max(1.)))
                        .min_w_0()
                        .max_w_full()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .when(own, |d| d.items_end())
                        .when(!own, |d| d.items_start());
                    if let Some(label) = heading(i) {
                        // A name is shown only where diarisation finds
                        // several voices on a side, small and close.
                        stack = stack.child(
                            div()
                                .px(px(ROW_PAD.0))
                                .text_size(px(11.))
                                .line_height(px(SPEAKER_LINE))
                                .text_color(skin.muted)
                                .child(label.to_owned()),
                        );
                    }
                    stack = stack.child(paragraph);
                    div()
                        .id(("nub-transcript", i))
                        .flex()
                        .mx(px(ROW_MARGIN))
                        .py(px(1.))
                        .when(!same_before, |d| d.mt(px(GROUP_GAP)))
                        .when(!same_after, |d| d.mb(px(GROUP_GAP)))
                        .when(own, |d| d.pl(px(ROW_INDENT)).justify_end())
                        .when(!own, |d| d.pr(px(ROW_INDENT)).justify_start())
                        .child(stack)
                        .into_any_element()
                },
            ));
        }
        // The same quiet line as under the note's transcript.
        if let Some(status) = status {
            let ink: Hsla = if status.warning { skin.warning } else { skin.muted }.into();
            // Padding, not margin: see the note's transcript panel. A plain
            // block, so a longer line wraps inside the popup.
            content = content.child(
                div().w_full().flex_shrink_0().px(px(16.)).pt(px(2.)).pb(px(8.)).child(
                    div()
                        .id("nub-live-status")
                        .w_full()
                        .text_size(px(11.))
                        .line_height(px(15.))
                        .text_align(TextAlign::Center)
                        .text_color(ink)
                        .child(status.text),
                ),
            );
        }

        // One shape carries the surface, its hairline and its shadow, so
        // all three grow out of the capsule together and fold back into it.
        let card = div()
            .id("nub-transcript-bubble")
            .size_full()
            .relative()
            .rounded(px(BUBBLE_RADIUS))
            .border(px(rim))
            .border_color(skin.edge)
            .bg(skin.surface)
            .shadow(lifted(BUBBLE_SHADOW + INSET, skin.dark))
            .text_color(skin.ink)
            .font_family(FONT)
            .text_size(px(14.))
            .line_height(px(20.))
            .flex()
            .flex_col()
            .pb(px((VIEWPORT_INSET - rim).max(0.)))
            // A quiet header holds the copy button, so it never sits on a row.
            .child(
                div()
                    .flex_shrink_0()
                    .h(px(HEADER))
                    .pl(px(ROW_MARGIN + ROW_PAD.0 - rim))
                    .pr(px(ROW_MARGIN - rim))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(11.))
                            .line_height(px(14.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(skin.muted)
                            .child("Live transcript"),
                    )
                    .when(count > 0, |header| {
                        // The stored rows are shared, not copied, until a press.
                        let (stored, speaking) = (stored.clone(), speaking.clone());
                        header.child(copy_button(skin, self.copied_shown, &self.stage).on_click(
                            cx.listener(move |this, _, _, cx| {
                                let rows: Vec<_> =
                                    stored.iter().chain(&speaking).cloned().collect();
                                this.copy(&rows, cx);
                            }),
                        ))
                    }),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .child(content)
                    .child(scroll_fades(self.scroll.clone(), skin.surface, (0., BUBBLE_RADIUS - rim))),
            );
        let root = div()
            .relative()
            .size_full()
            // A bubble on its way out no longer keeps itself open.
            .when(!leaving, |root| root.child(pointer_events))
            .p(px(INSET + BUBBLE_SHADOW))
            .child(crate::motion::pop(
                card,
                crate::motion::Pop {
                    // The capsule's side, at the capsule's height.
                    anchor: (if beside_left { 1. } else { 0. }, 0.67),
                    scale: (0.88, 0.8),
                    travel: 6.,
                    rise: 0.,
                },
                pose,
            ));
        self.stage.flush_as("bubble", window);
        root
    }
}

#[derive(Clone, Debug, PartialEq)]
struct TranscriptSegment {
    /// Which row this is, however the rows around it change.
    id: u64,
    /// Canonical channel + chunk-scoped speaker identity, never a display name.
    speaker: String,
    label: Option<String>,
    part: Option<usize>,
    own: bool,
    text: String,
    is_final: bool,
}

fn bubble_segments(
    transcript: &str,
    segments: &[meeting_notes_api_types::TranscriptSegmentDTO],
) -> Vec<TranscriptSegment> {
    if segments.is_empty() {
        return transcript_segments(transcript);
    }
    crate::note::server_transcript_utterances(segments)
        .into_iter()
        .map(|segment| TranscriptSegment {
            id: segment.id,
            speaker: segment.key,
            label: segment.speaker,
            part: segment.part,
            own: segment.own,
            text: segment.text,
            is_final: segment.is_final,
        })
        .collect()
}
fn segment_of(row: crate::note::TranscriptUtterance) -> TranscriptSegment {
    TranscriptSegment {
        id: row.id,
        speaker: row.key,
        label: row.speaker,
        part: row.part,
        own: row.own,
        text: row.text,
        is_final: row.is_final,
    }
}
/// Compatibility adapter shared with the full transcript. Anonymous text must
/// not become "Me" merely because it has no source header.
fn transcript_segments(transcript: &str) -> Vec<TranscriptSegment> {
    crate::note::legacy_transcript_utterances(transcript)
        .into_iter()
        .map(|segment| TranscriptSegment {
            id: segment.id,
            speaker: segment.key,
            label: segment.speaker,
            part: segment.part,
            own: segment.own,
            text: segment.text,
            is_final: segment.is_final,
        })
        .collect()
}

/// Whether the microphone (`own`) or the system side of `rows` carries more
/// than one voice: only then are its speakers named.
/// Line height of the name above a group of bubbles, where one is shown.
const SPEAKER_LINE: f32 = 14.;

fn several_voices<'a>(rows: impl Iterator<Item = &'a TranscriptSegment>, own: bool) -> bool {
    // Unnamed words still being spoken are nobody's yet (see the note's
    // transcript): they do not make a side "several voices".
    let mut voices = rows.filter(|row| row.own == own && (row.is_final || row.label.is_some()));
    voices
        .next()
        .is_some_and(|first| voices.any(|other| other.speaker != first.speaker))
}

/// The name shown above `segment`, which follows `before`: only where the
/// voice changes, and only on a side that has several (or for a part-scoped
/// speaker, whose label says which part).
fn speaker_heading<'a>(
    segment: &'a TranscriptSegment,
    before: Option<&TranscriptSegment>,
    several: bool,
) -> Option<&'a str> {
    if before.is_some_and(|before| before.speaker == segment.speaker) {
        return None;
    }
    (several || segment.part.is_some())
        .then_some(segment.label.as_deref())
        .flatten()
        .filter(|label| !label.trim().is_empty())
}

#[cfg(test)]
fn transcript_speaker_heading(segments: &[TranscriptSegment], index: usize) -> Option<&str> {
    let segment = segments.get(index)?;
    let before = index.checked_sub(1).map(|before| &segments[before]);
    speaker_heading(segment, before, several_voices(segments.iter(), segment.own))
}
/// Line height of a transcript row's 13 pt text: a whole number, so every
/// line's baseline lands on a pixel at 1x.
const ROW_LINE: f32 = 18.;
/// A row's padding (across, down), the gap above and below a run of rows
/// from one speaker, and how far a row stops short of the far side.
const ROW_PAD: (f32, f32) = (10., 6.);
const GROUP_GAP: f32 = 4.;
const ROW_INDENT: f32 = 20.;
/// The typing dots after a row still being heard: three 3 pt dots 3 pt
/// apart, 5 pt after the text, as one unbreakable piece that moves to a line
/// of its own when the last line has no room for it.
const DOT: f32 = 3.;
const DOTS_GAP: f32 = 3.;
const DOTS_LEAD: f32 = 5.;
const DOTS_WIDTH: f32 = 3. * DOT + 2. * DOTS_GAP;

fn transcript_text(
    text: &str,
    is_final: bool,
    wrap: f32,
    ink: Hsla,
    texts: &std::sync::Arc<WindowTextSystem>,
) -> AnyElement {
    if is_final {
        return StyledText::new(text.to_owned()).into_any_element();
    }
    // Where the text's last line ends decides where the dots go, as inline
    // content would place them.
    let run = TextRun {
        len: text.len(),
        font: font(FONT),
        color: ink,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let shaped = texts
        .shape_text(
            text.to_owned().into(),
            px(13.),
            &[run],
            Some(px(wrap.max(1.))),
            None,
        )
        .ok();
    let (lines, last, baseline) = shaped
        .as_ref()
        .and_then(|lines| {
            let count: usize = lines
                .iter()
                .map(|line| line.wrap_boundaries().len() + 1)
                .sum();
            let line = lines.last()?;
            let start = line.wrap_boundaries().last().map_or(px(0.), |boundary| {
                line.runs()
                    .get(boundary.run_ix)
                    .and_then(|run| run.glyphs.get(boundary.glyph_ix))
                    .map_or(px(0.), |glyph| glyph.position.x)
            });
            let width = line.unwrapped_layout.width - start;
            let glyphs = line.ascent() + line.descent();
            let baseline = (px(ROW_LINE) - glyphs) / 2. + line.ascent();
            Some((count, f32::from(width), f32::from(baseline)))
        })
        .unwrap_or((1, 0., 13.6));
    let inline = text.is_empty() || last + DOTS_LEAD + DOTS_WIDTH <= wrap;
    let (left, top) = if text.is_empty() {
        (0., 0.)
    } else if inline {
        ((last + DOTS_LEAD).round(), (lines - 1) as f32 * ROW_LINE)
    } else {
        (0., lines as f32 * ROW_LINE)
    };
    // At rest the dots stand on the text's baseline, like the ellipsis they
    // are, on whole pixels; each jumps 2 pt at its peak.
    let dots_top = (top + baseline).round() - DOT;
    let still = crate::motion::reduce_motion();
    let dots = crate::motion::slow_loop(
        "nub-interim-dots",
        Duration::from_millis(1000),
        div()
            .absolute()
            .left(px(left))
            .top(px(dots_top))
            .h(px(DOT))
            .flex()
            .gap(px(DOTS_GAP)),
        move |row, turns| {
            row.children((0..3).map(|index| {
                // With reduced motion the three rest level, at the loop's
                // middle opacity, rather than frozen mid-jump.
                let (opacity, lift) = if still {
                    (0.65, 0.)
                } else {
                    jumping_dot(turns - index as f32 * 0.25)
                };
                div()
                    .size(px(DOT))
                    .mt(px(-lift))
                    .rounded_full()
                    .bg(ink.opacity(opacity))
            }))
        },
    );
    div()
        .relative()
        .min_w_0()
        // A single line grows by the dots; wrapped text already spans the
        // row, and dots that moved down add a line.
        .when(lines == 1 && inline && !text.is_empty(), |d| {
            d.pr(px(DOTS_LEAD + DOTS_WIDTH))
        })
        .when(text.is_empty(), |d| d.w(px(DOTS_WIDTH)).h(px(ROW_LINE)))
        .when(!inline, |d| d.pb(px(ROW_LINE)))
        .child(StyledText::new(text.to_owned()))
        .child(dots)
        .into_any_element()
}

/// One dot of the reference's `oats-jumping-dots`: a one-second loop that
/// fades from 30% to full and rises 2 pt at its middle, eased both ways.
/// `turns` is the loop position; returns the opacity and the rise.
fn jumping_dot(turns: f32) -> (f32, f32) {
    let t = turns.rem_euclid(1.);
    let half = if t < 0.5 { t * 2. } else { (1. - t) * 2. };
    // CSS `ease` on each half of the keyframes.
    let eased = if t < 0.5 {
        crate::motion::cubic_bezier(half, 0.25, 0.1, 0.25, 1.)
    } else {
        1. - crate::motion::cubic_bezier(1. - half, 0.25, 0.1, 0.25, 1.)
    };
    (0.3 + 0.7 * eased, 2. * eased)
}

fn clamp_nub(bounds: Bounds<Pixels>, screen: Bounds<Pixels>) -> Bounds<Pixels> {
    Bounds::new(
        point(
            bounds
                .left()
                .max(screen.left())
                .min((screen.right() - bounds.size.width).max(screen.left())),
            bounds
                .top()
                .max(screen.top())
                .min((screen.bottom() - bounds.size.height).max(screen.top())),
        ),
        bounds.size,
    )
}

fn nub_bounds(available: Bounds<Pixels>) -> Bounds<Pixels> {
    Bounds::new(
        point(
            available.right() - px(NUB_SIZE.0),
            available.top() + px(((f32::from(available.size.height) - 37.) * 0.7).round()),
        ),
        size(px(NUB_SIZE.0), px(NUB_SIZE.1)),
    )
}
fn bubble_bounds(nub: Bounds<Pixels>, available: Bounds<Pixels>) -> Bounds<Pixels> {
    let left = nub.left() - px(BUBBLE_SIZE.0) + px(4.);
    let left = if nub.left() - px(BUBBLE_SIZE.0) < available.left() + px(8.) {
        nub.right() - px(4.)
    } else {
        left
    };
    let max_left = (available.right() - px(BUBBLE_SIZE.0) - px(8.)).max(available.left() + px(8.));
    let top = (nub.top() - px(200.))
        .max(available.top())
        .min((available.bottom() - px(BUBBLE_SIZE.1)).max(available.top()));
    Bounds::new(
        point(left.max(available.left() + px(8.)).min(max_left), top),
        size(px(BUBBLE_SIZE.0), px(BUBBLE_SIZE.1)),
    )
}
/// Whether `point` in a panel's window is over the panel's rectangle, not
/// the `margin` the window keeps around it for the shadow.
fn pointer_inside(point: Point<Pixels>, viewport: Size<Pixels>, margin: f32) -> bool {
    point.x >= px(margin)
        && point.y >= px(margin)
        && point.x < viewport.width - px(margin)
        && point.y < viewport.height - px(margin)
}
fn is_drag(a: Point<Pixels>, b: Point<Pixels>) -> bool {
    let d = b - a;
    f32::from(d.x).powi(2) + f32::from(d.y).powi(2) > 9.
}

#[cfg(target_os = "macos")]
#[allow(unexpected_cfgs)]
mod native {
    use cocoa::base::{NO, YES, id, nil};
    use cocoa::foundation::{NSPoint, NSRect};
    use gpui::Window;
    use objc::{class, msg_send, sel, sel_impl};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    fn native(window: &Window) -> Option<id> {
        let handle = HasWindowHandle::window_handle(window).ok()?;
        let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
            return None;
        };
        let view = handle.ns_view.as_ptr() as id;
        Some(unsafe { msg_send![view, window] })
    }
    /// `front` puts the panel a level above the other one, whichever was
    /// opened last.
    pub fn configure(window: &Window, front: bool) {
        if let Some(window) = native(window) {
            unsafe {
                let _: () = msg_send![window,setHidesOnDeactivate:NO];
                let _: () = msg_send![window,setFloatingPanel:YES];
                // After `setFloatingPanel`, which sets the level itself.
                if front {
                    let level: isize = msg_send![window, level];
                    let _: () = msg_send![window, setLevel: level + 1];
                }
                let _: () = msg_send![window,setHasShadow:NO];
                // A titled window is given a one-pixel highlight along its
                // top edge by the system. On these see-through panels it was
                // a faint line above the capsule, and across the top of the
                // popup's window while the popup itself was folded away.
                // Neither panel has a title bar to lose.
                const TITLED: u64 = 1;
                let mask: u64 = msg_send![window, styleMask];
                if mask & TITLED != 0 {
                    let _: () = msg_send![window, setStyleMask: mask & !TITLED];
                }
            }
        }
    }
    thread_local! {
        static CROSSING: std::cell::RefCell<Option<Box<dyn Fn()>>> =
            const { std::cell::RefCell::new(None) };
    }

    /// Called whenever the pointer enters or leaves a tracked panel.
    pub fn on_crossing(handler: Box<dyn Fn()>) {
        CROSSING.with(|crossing| *crossing.borrow_mut() = Some(handler));
    }

    extern "C" fn crossed(_: &objc::runtime::Object, _: objc::runtime::Sel, _: id) {
        CROSSING.with(|crossing| {
            if let Some(handler) = crossing.borrow().as_ref() {
                handler();
            }
        });
    }

    /// The object AppKit sends the panels' enter and exit events to.
    fn tracker() -> Option<id> {
        thread_local! {
            static TRACKER: std::cell::Cell<Option<id>> = const { std::cell::Cell::new(None) };
        }
        if let Some(tracker) = TRACKER.with(std::cell::Cell::get) {
            return Some(tracker);
        }
        let mut declaration = objc::declare::ClassDecl::new("CerealPillTracker", class!(NSObject))?;
        let handler = crossed as extern "C" fn(&objc::runtime::Object, objc::runtime::Sel, id);
        unsafe {
            declaration.add_method(sel!(mouseEntered:), handler);
            declaration.add_method(sel!(mouseExited:), handler);
            let tracker: id = msg_send![declaration.register(), new];
            TRACKER.with(|cell| cell.set(Some(tracker)));
            Some(tracker)
        }
    }

    /// Give a panel a tracking area covering its rectangle (the window less
    /// the `margin` it keeps for the shadow), live even while the app is
    /// inactive. GPUI's own view is left untouched. The panels never resize,
    /// so the area is set once.
    pub fn track(window: &Window, margin: f32) {
        const ENTERED_AND_EXITED: usize = 0x01;
        const ACTIVE_ALWAYS: usize = 0x80;
        let Ok(handle) = HasWindowHandle::window_handle(window) else {
            return;
        };
        let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
            return;
        };
        let Some(tracker) = tracker() else { return };
        let view = handle.ns_view.as_ptr() as id;
        unsafe {
            let area: id = msg_send![class!(NSTrackingArea), alloc];
            let whole: NSRect = msg_send![view, bounds];
            let margin = f64::from(margin);
            let rect = NSRect::new(
                NSPoint::new(whole.origin.x + margin, whole.origin.y + margin),
                cocoa::foundation::NSSize::new(
                    (whole.size.width - 2. * margin).max(0.),
                    (whole.size.height - 2. * margin).max(0.),
                ),
            );
            let area: id = msg_send![area,
                initWithRect: rect
                options: ENTERED_AND_EXITED | ACTIVE_ALWAYS
                owner: tracker
                userInfo: nil];
            let _: () = msg_send![view, addTrackingArea: area];
        }
    }

    /// Whether the pointer is over the panel's rectangle (the window less
    /// its shadow `margin`) with nothing of another app's in between.
    pub fn pointer_inside(window: &Window, margin: f32) -> Option<bool> {
        let window = native(window)?;
        let margin = f64::from(margin);
        unsafe {
            let visible: cocoa::base::BOOL = msg_send![window, isVisible];
            if visible == NO {
                return Some(false);
            }
            let local: NSPoint = msg_send![window, mouseLocationOutsideOfEventStream];
            let frame: NSRect = msg_send![window, frame];
            if local.x < margin
                || local.y < margin
                || local.x >= frame.size.width - margin
                || local.y >= frame.size.height - margin
            {
                return Some(false);
            }
            let global: NSPoint = msg_send![class!(NSEvent), mouseLocation];
            let top: isize = msg_send![class!(NSWindow), windowNumberAtPoint:global belowWindowWithWindowNumber:0_isize];
            let own: isize = msg_send![window, windowNumber];
            if top == own {
                return Some(true);
            }
            // The other panel's see-through margin may lie over this one.
            let app: id = msg_send![class!(NSApplication), sharedApplication];
            let above: id = msg_send![app, windowWithWindowNumber: top];
            if above == nil {
                return Some(false);
            }
            let panel: cocoa::base::BOOL = msg_send![above, isKindOfClass: class!(NSPanel)];
            if panel == NO {
                return Some(false);
            }
            let floating: cocoa::base::BOOL = msg_send![above, isFloatingPanel];
            Some(floating == YES)
        }
    }
    /// Where the pointer is on the screen, in AppKit's coordinates (points,
    /// origin at the bottom-left of the primary display).
    pub fn pointer() -> Option<(f64, f64)> {
        let global: NSPoint = unsafe { msg_send![class!(NSEvent), mouseLocation] };
        Some((global.x, global.y))
    }
    /// The window's bottom-left corner, in the same coordinates.
    pub fn origin(window: &Window) -> Option<(f64, f64)> {
        let window = native(window)?;
        let frame: NSRect = unsafe { msg_send![window, frame] };
        Some((frame.origin.x, frame.origin.y))
    }
    pub fn set_origin(window: &Window, origin: (f64, f64)) {
        if let Some(window) = native(window) {
            unsafe {
                let _: () = msg_send![window,setFrameOrigin:NSPoint::new(origin.0, origin.1)];
            }
        }
    }
    /// Height of the primary display, which the top-left coordinates of the
    /// UI framework are measured down from.
    fn primary_height() -> Option<f64> {
        unsafe {
            let screens: id = msg_send![class!(NSScreen), screens];
            let count: usize = msg_send![screens, count];
            if count == 0 {
                return None;
            }
            let primary: id = msg_send![screens, objectAtIndex: 0_usize];
            let frame: NSRect = msg_send![primary, frame];
            Some(frame.size.height)
        }
    }
    /// Every screen's frame and visible frame (the part clear of the menu
    /// bar and the Dock), in the UI framework's coordinates.
    pub fn screens() -> Vec<(gpui::Bounds<gpui::Pixels>, gpui::Bounds<gpui::Pixels>)> {
        let Some(height) = primary_height() else {
            return Vec::new();
        };
        let convert = |rect: NSRect| {
            gpui::Bounds::new(
                gpui::point(
                    gpui::px(rect.origin.x as f32),
                    gpui::px((height - rect.origin.y - rect.size.height) as f32),
                ),
                gpui::size(gpui::px(rect.size.width as f32), gpui::px(rect.size.height as f32)),
            )
        };
        unsafe {
            let screens: id = msg_send![class!(NSScreen), screens];
            let count: usize = msg_send![screens, count];
            (0..count)
                .map(|index| {
                    let screen: id = msg_send![screens, objectAtIndex: index];
                    let frame: NSRect = msg_send![screen, frame];
                    let visible: NSRect = msg_send![screen, visibleFrame];
                    (convert(frame), convert(visible))
                })
                .collect()
        }
    }
    /// The window's bounds as the window server has them now, in the UI
    /// framework's coordinates (origin at the top-left of the primary display).
    pub fn bounds(window: &Window) -> Option<gpui::Bounds<gpui::Pixels>> {
        let native = native(window)?;
        let frame: NSRect = unsafe { msg_send![native, frame] };
        let height = primary_height()?;
        Some(gpui::Bounds::new(
            gpui::point(
                gpui::px(frame.origin.x as f32),
                gpui::px((height - frame.origin.y - frame.size.height) as f32),
            ),
            gpui::size(gpui::px(frame.size.width as f32), gpui::px(frame.size.height as f32)),
        ))
    }
    /// Put the window's top-left corner at `origin` (UI framework coordinates).
    pub fn place(window: &Window, origin: gpui::Point<gpui::Pixels>) {
        let (Some(native), Some(height)) = (native(window), primary_height()) else {
            return;
        };
        unsafe {
            let frame: NSRect = msg_send![native, frame];
            let point = NSPoint::new(
                f64::from(f32::from(origin.x)),
                height - f64::from(f32::from(origin.y)) - frame.size.height,
            );
            let _: () = msg_send![native,setFrameOrigin:point];
        }
    }
    pub fn move_to(window: &Window, origin: gpui::Point<gpui::Pixels>) {
        let delta = origin - window.bounds().origin;
        if let Some(window) = native(window) {
            unsafe {
                let frame: NSRect = msg_send![window, frame];
                let point = NSPoint::new(
                    frame.origin.x + f64::from(f32::from(delta.x)),
                    frame.origin.y - f64::from(f32::from(delta.y)),
                );
                let _: () = msg_send![window,setFrameOrigin:point];
            }
        }
    }
    pub fn reveal(window: &Window) {
        if let Some(window) = native(window) {
            unsafe {
                let _: () = msg_send![window, deminiaturize:nil];
            }
        }
    }
    pub fn hide_main(window: &Window) {
        if let Some(window) = native(window) {
            unsafe {
                let _: () = msg_send![window, orderOut:nil];
            }
        }
    }
}
#[cfg(not(target_os = "macos"))]
mod native {
    pub fn configure(_: &gpui::Window, _: bool) {}
    pub fn track(_: &gpui::Window, _: f32) {}
    pub fn move_to(_: &gpui::Window, _: gpui::Point<gpui::Pixels>) {}
    pub fn pointer() -> Option<(f64, f64)> {
        None
    }
    pub fn screens() -> Vec<(gpui::Bounds<gpui::Pixels>, gpui::Bounds<gpui::Pixels>)> {
        Vec::new()
    }
    pub fn origin(_: &gpui::Window) -> Option<(f64, f64)> {
        None
    }
    pub fn set_origin(_: &gpui::Window, _: (f64, f64)) {}
    pub fn bounds(_: &gpui::Window) -> Option<gpui::Bounds<gpui::Pixels>> {
        None
    }
    pub fn place(w: &gpui::Window, origin: gpui::Point<gpui::Pixels>) {
        move_to(w, origin)
    }
    pub fn reveal(_: &gpui::Window) {}
    pub fn hide_main(w: &gpui::Window) {
        w.minimize_window();
    }
}

#[cfg(test)]
mod tests {
    use super::{
        bubble_bounds, bubble_segments, clamp_nub, is_drag, nub_bounds, transcript_segments,
        transcript_speaker_heading, voice_bar_targets,
    };
    use gpui::{Bounds, point, px, size};
    #[test]
    fn default_nub_tracks_right_edge_and_seventy_percent_height() {
        let screen = Bounds::new(point(px(1440.), px(0.)), size(px(1920.), px(1080.)));
        let nub = nub_bounds(screen);
        assert_eq!(nub.origin, point(px(3318.), px(730.)));
        assert_eq!(nub.size, size(px(42.), px(70.)));
    }
    #[test]
    fn transcript_bubble_flips_at_left_edge_and_clamps_vertically() {
        let screen = Bounds::new(point(px(0.), px(0.)), size(px(1440.), px(900.)));
        let left = bubble_bounds(
            Bounds::new(point(px(0.), px(100.)), size(px(42.), px(70.))),
            screen,
        );
        assert_eq!(left.origin, point(px(38.), px(0.)));
        let bottom = bubble_bounds(
            Bounds::new(point(px(1398.), px(850.)), size(px(42.), px(70.))),
            screen,
        );
        assert_eq!(bottom.origin, point(px(1102.), px(550.)));
    }
    #[test]
    fn drag_threshold_preserves_three_pixel_clicks() {
        let a = point(px(0.), px(0.));
        assert!(!is_drag(a, point(px(3.), px(0.))));
        assert!(is_drag(a, point(px(3.), px(1.))));
    }
    #[test]
    fn popup_prefers_left_but_flips_when_full_panel_would_cross_margin() {
        let screen = Bounds::new(point(px(0.), px(0.)), size(px(1440.), px(900.)));
        let nub = |x| Bounds::new(point(px(x), px(400.)), size(px(42.), px(70.)));
        assert_eq!(bubble_bounds(nub(307.), screen).left(), px(345.));
        assert_eq!(bubble_bounds(nub(308.), screen).left(), px(12.));
    }
    #[test]
    fn dragging_cannot_leave_pill_beyond_display_edges() {
        let screen = Bounds::new(point(px(-1440.), px(90.)), size(px(1440.), px(900.)));
        let result = clamp_nub(
            Bounds::new(point(px(-1500.), px(1000.)), size(px(42.), px(70.))),
            screen,
        );
        assert_eq!(result.origin, point(px(-1440.), px(920.)));
    }
    #[test]
    fn the_capsule_rests_clear_of_the_menu_bar_and_the_dock_on_the_nearest_screen() {
        use super::{Desk, resting_place};
        let nub = |x: f32, y: f32| Bounds::new(point(px(x), px(y)), size(px(42.), px(70.)));
        let rect = |x: f32, y: f32, w: f32, h: f32| Bounds::new(point(px(x), px(y)), size(px(w), px(h)));
        // A 1920 by 1080 display with a 30 px menu bar and a 90 px Dock.
        let main = Desk { frame: rect(0., 0., 1920., 1080.), visible: rect(0., 30., 1920., 960.) };
        let rest = |x: f32, y: f32, screens: &[Desk]| resting_place(nub(x, y), screens).unwrap().0.origin;
        // Let go at the bottom edge: it stops above the Dock, not behind it.
        assert_eq!(rest(1878., 1010., &[main]), point(px(1878.), px(920.)));
        // At the top: below the menu bar. In the middle: where it was put.
        assert_eq!(rest(500., 0., &[main]), point(px(500.), px(30.)));
        assert_eq!(rest(700., 400., &[main]), point(px(700.), px(400.)));
        // Past the corner of the screen: back inside both ways.
        assert_eq!(rest(2500., -300., &[main]), point(px(1878.), px(30.)));
        // A Dock on the left edge instead.
        let left_dock = Desk { frame: rect(0., 0., 1920., 1080.), visible: rect(80., 30., 1840., 1050.) };
        assert_eq!(rest(10., 500., &[left_dock]), point(px(80.), px(500.)));
        // A position saved on a larger display that is no longer there comes
        // back on the display that is, clear of the Dock.
        assert_eq!(rest(3200., 1900., &[main]), point(px(1878.), px(920.)));
        // Two displays, the second to the left with no menu bar or Dock: a
        // capsule on it stays on it; one saved in the gap above the smaller
        // display goes to the nearer of the two.
        let second = Desk { frame: rect(-1440., 90., 1440., 900.), visible: rect(-1440., 90., 1440., 900.) };
        let both = [main, second];
        assert_eq!(rest(-1500., 1000., &both), point(px(-1440.), px(920.)));
        assert_eq!(resting_place(nub(-700., 500.), &both).unwrap().1, second);
        assert_eq!(rest(-300., 20., &both), point(px(-300.), px(90.)));
        assert_eq!(resting_place(nub(40., 5.), &both).unwrap().1, main);
        // No screens at all: nothing to rest on.
        assert!(resting_place(nub(0., 0.), &[]).is_none());
    }
    #[test]
    fn transcript_adapter_keeps_speaker_channels_and_partial_text_without_headers() {
        let groups = transcript_segments(
            "Me  00:12\nHello there\n\nSomeone else  00:14\nResponse\n\nMe  00:18\nStill speaking [interim]",
        );
        assert_eq!(groups.len(), 3);
        assert!(groups[0].own);
        assert!(!groups[1].own);
        assert_eq!(groups[1].text, "Response");
        assert!(!groups[2].is_final);
        assert_eq!(groups[2].text, "Still speaking");
    }
    #[test]
    fn floating_transcript_preserves_server_source_and_distinct_mic_speakers() {
        use meeting_notes_api_types::{AudioChannel, TranscriptSegmentDTO};
        let segment = |channel, speaker: &str, label: &str, text: &str| TranscriptSegmentDTO {
            id: speaker.into(),
            channel,
            speaker_id: speaker.into(),
            speaker_label: Some(label.into()),
            text: text.into(),
            is_final: true,
            start_ms: 1000,
            end_ms: Some(2000),
        };
        let source = vec![
            segment(
                AudioChannel::Mic,
                "mic-first",
                "Speaker 0",
                "First microphone voice.",
            ),
            segment(
                AudioChannel::Mic,
                "mic-second",
                "Speaker 1",
                "Second microphone voice.",
            ),
            segment(
                AudioChannel::System,
                "system-first",
                "Me",
                "Actual system source.",
            ),
        ];
        let groups = bubble_segments("Me  00:01\nStale flattened text", &source);
        assert_eq!(groups.len(), 3);
        assert!(groups[0].own && groups[1].own);
        assert_ne!(groups[0].speaker, groups[1].speaker);
        assert!(!groups[2].own);
        assert_eq!(groups[2].text, "Actual system source.");
        assert!(groups.iter().all(|group| group.speaker != "Me"));
        assert_eq!(transcript_speaker_heading(&groups, 0), Some("Speaker 0"));
        assert_eq!(transcript_speaker_heading(&groups, 1), Some("Speaker 1"));
        assert_eq!(transcript_speaker_heading(&groups, 2), None);
    }
    #[test]
    fn anonymous_legacy_transcript_never_becomes_me() {
        let groups = bubble_segments(
            "A voice without a source header.\n\nStill speaking [interim]",
            &[],
        );
        assert_eq!(groups.len(), 2);
        assert!(
            groups
                .iter()
                .all(|group| !group.own && group.label.is_none())
        );
        assert!(groups.iter().all(|group| group.speaker.is_empty()));
        assert_eq!(groups[1].text, "Still speaking");
        assert!(!groups[1].is_final);
    }
    #[test]
    fn system_speakers_and_chunk_scoped_ids_keep_separate_group_headings() {
        use meeting_notes_api_types::{AudioChannel, TranscriptSegmentDTO};
        let segment = |channel, id: &str, label: &str| TranscriptSegmentDTO {
            id: format!("segment-{id}"),
            channel,
            speaker_id: id.into(),
            speaker_label: Some(label.into()),
            text: "Actual transcript text".into(),
            is_final: true,
            start_ms: 0,
            end_ms: Some(1000),
        };
        let groups = bubble_segments(
            "",
            &[
                segment(AudioChannel::Mic, "chunk-a:speaker_0", "Speaker 1"),
                segment(AudioChannel::Mic, "chunk-a:speaker_0", "Speaker 1"),
                segment(AudioChannel::Mic, "chunk-b:speaker_0", "Speaker 1"),
                segment(AudioChannel::System, "chunk-a:speaker_0", "Speaker 1"),
                segment(AudioChannel::System, "chunk-a:speaker_1", "Speaker 2"),
            ],
        );
        assert_ne!(
            groups[0].speaker, groups[2].speaker,
            "Matching provider labels in different chunks must not merge identities"
        );
        assert_ne!(
            groups[0].speaker, groups[3].speaker,
            "A channel is provenance, not a shared speaker identity"
        );
        assert_eq!(transcript_speaker_heading(&groups, 0), Some("Speaker 1"));
        assert_eq!(transcript_speaker_heading(&groups, 1), None);
        assert_eq!(transcript_speaker_heading(&groups, 2), Some("Speaker 1"));
        assert_eq!(transcript_speaker_heading(&groups, 3), Some("Speaker 1"));
        assert_eq!(transcript_speaker_heading(&groups, 4), Some("Speaker 2"));
        assert!(!groups[3].own && !groups[4].own);
        let mut unknown = segment(
            AudioChannel::System,
            "system:419d65d1-7a2c-44ca-84cc-ad489522d561:unknown",
            "",
        );
        unknown.speaker_label = None;
        let canonical = bubble_segments(
            "",
            &[
                segment(
                    AudioChannel::Mic,
                    "mic:419d65d1-7a2c-44ca-84cc-ad489522d561:speaker_0",
                    "Speaker 1",
                ),
                unknown,
            ],
        );
        assert_eq!(
            transcript_speaker_heading(&canonical, 0),
            Some("Speaker 1 · Mic · Part 1")
        );
        assert_eq!(
            transcript_speaker_heading(&canonical, 1),
            Some("Unlabeled speaker · System · Part 1")
        );
    }
    #[test]
    fn quiet_audio_is_three_static_dots_and_voice_changes_bar_height() {
        assert_eq!(voice_bar_targets(0.), [3.; 3]);
        assert_eq!(voice_bar_targets(f32::NAN), [3.; 3]);
        let active = voice_bar_targets(0.5);
        assert!(active[1] > active[0]);
        assert!(active.iter().all(|height| *height <= 14. && *height > 3.));
    }
}
