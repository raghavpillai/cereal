use gpui::{prelude::*, *};
use rust_embed::RustEmbed;
use std::borrow::Cow;

pub const FONT: &str = "KMR Melange Grotesk";

/// A quick count of the lines `text` wraps to at `width`, from character
/// advances alone (no shaping). It is what windowed lists guess row heights
/// with: within a line of what layout will say, at a few nanoseconds a
/// character, so an hour of transcript is sized in well under a millisecond.
pub struct LineCounter(LineWrapperHandle);

impl LineCounter {
    /// For text in the app's face at `size`.
    pub fn new(size: f32, cx: &App) -> Self {
        Self(cx.text_system().line_wrapper(font(FONT), px(size)))
    }

    pub fn lines(&mut self, text: &str, width: f32) -> usize {
        text.split('\n')
            .map(|line| {
                1 + self
                    .0
                    .wrap_line(&[LineFragment::text(line)], px(width.max(1.)))
                    .count()
            })
            .sum::<usize>()
            .max(1)
    }
}
pub const DISPLAY: &str = "Quadrant Notepad";

#[derive(RustEmbed)]
#[folder = "assets/"]
pub struct Assets;
impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(Self::get(path).map(|f| f.data))
    }
    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(Self::iter()
            .filter(|p| p.starts_with(path))
            .map(Into::into)
            .collect())
    }
}

/// Colours are the nominal design tokens, not values sampled from a
/// screenshot: a capture is already colour-managed, and the display would apply
/// the same shift a second time (every neutral came out slightly green).
/// Translucent tokens stay translucent so they tint whatever surface they sit
/// on, which is why one `hover` reads correctly on the canvas, the sidebar and
/// white cards alike.
#[derive(Clone, Copy)]
pub struct Theme {
    pub bg: Hsla,
    pub sidebar: Hsla,
    pub card: Hsla,
    pub text: Hsla,
    /// Secondary ink: captions, descriptions, times.
    pub muted: Hsla,
    /// Stronger secondary ink: sidebar labels and their icons.
    pub secondary: Hsla,
    /// Subtle edge of cards and popovers.
    pub border: Hsla,
    /// Hairline: pill outlines, dividers, the sidebar edge.
    pub hairline: Hsla,
    /// Highlight under the pointer and for the selected row (6% ink).
    pub hover: Hsla,
    /// Resting tinted fill: tiles, avatars, soft buttons (12% ink).
    pub fill: Hsla,
    pub accent: Hsla,
    /// Destructive actions.
    pub danger: Hsla,
    /// Warnings (a failed recording, attention needed): ink, tinted fill and
    /// outline that stay readable against each other in both themes.
    pub warning_ink: Hsla,
    pub warning_fill: Hsla,
    pub warning_edge: Hsla,
    /// A quiet informational tile or banner and its outline.
    pub info_fill: Hsla,
    pub info_edge: Hsla,
}
impl Theme {
    pub fn new(dark: bool) -> Self {
        if dark {
            Self {
                bg: rgb(0x292929).into(),
                sidebar: rgb(0x212121).into(),
                card: rgb(0x363635).into(),
                text: rgb(0xfcfcf8).into(),
                muted: rgb(0x9e9e99).into(),
                secondary: rgb(0xacada8).into(),
                border: rgba(0xffffff26).into(),
                hairline: rgba(0xffffff26).into(),
                hover: rgba(0xffffff0f).into(),
                fill: rgba(0xffffff1f).into(),
                accent: rgb(0xe0af2f).into(),
                danger: rgb(0xf0806e).into(),
                warning_ink: rgb(0xf0c9a0).into(),
                warning_fill: rgb(0x493623).into(),
                warning_edge: rgba(0xf0c9a04d).into(),
                info_fill: rgb(0x2c3441).into(),
                info_edge: rgb(0x424955).into(),
            }
        } else {
            Self {
                bg: rgb(0xf7f7f2).into(),
                sidebar: rgb(0xf2f2ec).into(),
                card: rgb(0xffffff).into(),
                text: rgb(0x292929).into(),
                muted: rgb(0x72726e).into(),
                secondary: rgb(0x4e4d4b).into(),
                border: rgb(0xe6e6de).into(),
                hairline: rgba(0x47432a33).into(),
                hover: rgba(0x625a220f).into(),
                fill: rgba(0x56512e1f).into(),
                accent: rgb(0x835e00).into(),
                danger: rgb(0xcf3527).into(),
                warning_ink: rgb(0x80552f).into(),
                warning_fill: rgb(0xffecd8).into(),
                warning_edge: rgb(0xe7d1b7).into(),
                info_fill: rgb(0xe8f2fe).into(),
                info_edge: rgb(0xc8d0d4).into(),
            }
        }
    }
}

pub fn icon(name: &str, size: f32, color: Hsla) -> Svg {
    // Hundreds of icons are built per render; format each path once.
    thread_local! {
        static PATHS: std::cell::RefCell<std::collections::HashMap<String, SharedString>> =
            std::cell::RefCell::new(Default::default());
    }
    let path = PATHS.with(|paths| {
        let mut paths = paths.borrow_mut();
        match paths.get(name) {
            Some(path) => path.clone(),
            None => {
                let path = SharedString::from(format!("icons/{name}.svg"));
                paths.insert(name.to_string(), path.clone());
                path
            }
        }
    });
    svg()
        .path(path)
        .size(px(size))
        .text_color(color)
        .flex_shrink_0()
}

pub fn pill(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    symbol: &str,
    t: Theme,
) -> Stateful<Div> {
    pill_shell(id, t)
        .when(!symbol.is_empty(), |s| s.child(icon(symbol, 16., t.muted)))
        .child(label.into())
}

/// A pill's box and nothing else: its size, outline and type. Nothing is in
/// it and nothing about it answers the pointer or the keyboard.
pub fn pill_frame(id: impl Into<ElementId>, t: Theme) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .gap(px(6.))
        .h(px(28.))
        .px(px(12.))
        .rounded_full()
        .border_1()
        .border_color(t.hairline)
        .text_size(px(13.))
        .text_color(t.text)
        .cursor_default()
}

/// A pill with nothing in it yet: [`pill_frame`] as a control, with its
/// place in the tab order and its wash under the pointer.
pub fn pill_shell(id: impl Into<ElementId>, t: Theme) -> Stateful<Div> {
    pill_frame(id, t)
        .reach()
        // Callers may turn a pill into a dark primary button. A neutral wash
        // reads on every fill, so that pairing survives both pointer states.
        .child(
            crate::motion::tactile(neutral_wash(0.14))
                .press(neutral_wash(0.12))
                .round(),
        )
}

pub fn tool(id: impl Into<ElementId>, symbol: &str, t: Theme) -> Stateful<Div> {
    let id = id.into();
    let label = tool_label(&id, symbol);
    match label {
        Some(label) => tool_tip(id, symbol, label, t),
        None => tool_base(id, symbol, t),
    }
}

/// An icon button with an explicit action label for context-specific controls.
pub fn tool_tip(
    id: impl Into<ElementId>,
    symbol: &str,
    label: impl Into<SharedString>,
    t: Theme,
) -> Stateful<Div> {
    with_tooltip(tool_base(id.into(), symbol, t), label.into(), t)
}

fn tool_base(id: ElementId, symbol: &str, t: Theme) -> Stateful<Div> {
    let glyph = tool_glyph(&id, symbol, t);
    tool_shell(id, t, Some(6.)).child(glyph)
}

/// A [`tool`] with its glyph drawn at `size` instead of the usual 15 pt: the
/// window's own controls beside the traffic lights, which are a size up.
pub fn tool_sized(id: impl Into<ElementId>, symbol: &str, size: f32, t: Theme) -> Stateful<Div> {
    let id = id.into();
    let glyph = tool_glyph_at(&id, symbol, size, t);
    let shell = tool_shell(id.clone(), t, Some(6.)).child(glyph);
    match tool_label(&id, symbol) {
        Some(label) => with_tooltip(shell, label, t),
        None => shell,
    }
}

/// How long a button shows that it did its job.
const CONFIRM_FOR: std::time::Duration = std::time::Duration::from_millis(1200);

thread_local! {
    /// The one button currently confirming an action, and since when.
    static CONFIRMED: std::cell::RefCell<Option<(ElementId, std::time::Instant)>> =
        const { std::cell::RefCell::new(None) };
}

/// Confirm an action on the button that performed it: the tool `id` shows a
/// check in place of its glyph for a moment, then swaps back. Used where the
/// result is otherwise invisible (a copy), instead of a toast in the far
/// corner that reads the same for every copy.
pub fn confirm(id: impl Into<ElementId>, cx: &mut App) {
    CONFIRMED.with(|confirmed| {
        *confirmed.borrow_mut() = Some((id.into(), std::time::Instant::now()));
    });
    cx.refresh_windows();
    cx.spawn(async move |cx| {
        cx.background_executor().timer(CONFIRM_FOR).await;
        let _ = cx.update(|cx| cx.refresh_windows());
    })
    .detach();
}

/// Whether the tool `id` is confirming (`Some(true)`), has just finished and
/// is swapping its own glyph back (`Some(false)`), or neither.
fn confirming(id: &ElementId, now: std::time::Instant) -> Option<bool> {
    CONFIRMED.with(|confirmed| {
        let confirmed = confirmed.borrow();
        let (confirmed_id, since) = confirmed.as_ref()?;
        if confirmed_id != id {
            return None;
        }
        let elapsed = now.saturating_duration_since(*since);
        if elapsed < CONFIRM_FOR {
            Some(true)
        } else if elapsed < CONFIRM_FOR + crate::motion::SNAPPY.duration() {
            Some(false)
        } else {
            None
        }
    })
}

/// A tool's glyph, or the check that briefly replaces it after [`confirm`].
/// Each swap rises in on the snappy spring; at rest it is the plain icon.
fn tool_glyph(id: &ElementId, symbol: &str, t: Theme) -> AnyElement {
    tool_glyph_at(id, symbol, 15., t)
}

fn tool_glyph_at(id: &ElementId, symbol: &str, size: f32, t: Theme) -> AnyElement {
    match confirming(id, std::time::Instant::now()) {
        Some(true) => crate::motion::swap_in("tool-confirmed", icon("check", size, t.text)),
        Some(false) => crate::motion::swap_in("tool-restored", icon(symbol, size, t.muted)),
        None => icon(symbol, size, t.muted).into_any_element(),
    }
}

/// A [`tool_tip`] for callers that reshape it into a circle or capsule, so the
/// hover wash and press squash follow the rounded outline.
pub fn round_tool(
    id: impl Into<ElementId>,
    symbol: &str,
    label: impl Into<SharedString>,
    t: Theme,
) -> Stateful<Div> {
    let id = id.into();
    let glyph = tool_glyph(&id, symbol, t);
    with_tooltip(
        tool_shell(id, t, None).rounded_full().child(glyph),
        label.into(),
        t,
    )
}

/// The send button of every composer: a 32 pt disc of the brand's bright
/// gold with a bold near-black arrow, in both themes (the reference's filled
/// send, in our colour; the light theme's deeper accent reads brown at this
/// size). Until there is something to send it is a quiet grey disc.
pub fn send_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    enabled: bool,
    t: Theme,
) -> Stateful<Div> {
    with_tooltip(
        tool_shell(id.into(), t, None)
            .rounded_full()
            .size(px(32.))
            .bg(if enabled { rgb(SEND_GOLD).into() } else { t.hover })
            .when(!enabled, |idle| idle.cursor(CursorStyle::Arrow).opacity(DISABLED))
            .child(icon("send", 17., if enabled { rgb(0x1d1a12).into() } else { t.muted })),
        label.into(),
        t,
    )
}
/// The gold of a send button that can send.
pub const SEND_GOLD: u32 = 0xe0af2f;

fn tool_shell(id: ElementId, t: Theme, radius: Option<f32>) -> Stateful<Div> {
    let feedback = crate::motion::tactile(t.hover).press(t.border.opacity(0.6));
    div()
        .id(id)
        .size(px(28.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(6.))
        .when(radius.is_none(), |round| round.rounded_full())
        .cursor_default()
        .child(match radius {
            Some(radius) => feedback.radius(radius),
            None => feedback.round(),
        })
        .reach()
}

thread_local! {
    /// Focus is being moved with the keyboard, not the pointer.
    static KEYBOARD_FOCUS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The window is in the dark theme (for colours chosen away from a `Theme`).
    static DARK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Device pixels per point of the main window's display.
    static SCALE: std::cell::Cell<f32> = const { std::cell::Cell::new(2.) };
}

/// Record whether focus is moving by keyboard (Tab) or by pointer. Returns
/// whether that changed, in which case the window has to be drawn again.
pub fn set_keyboard_focus(on: bool) -> bool {
    KEYBOARD_FOCUS.with(|flag| flag.replace(on) != on)
}

pub fn set_dark(dark: bool) {
    DARK.with(|flag| flag.set(dark));
}

/// Record the main window's scale, for [`hairline`].
pub fn set_scale(scale: f32) {
    SCALE.with(|cell| cell.set(scale));
}

/// An edge `points` wide as a display can draw it at `scale` device pixels
/// per point: never thinner than one device pixel, the way a browser rounds
/// a sub-pixel border up. Half a point is one pixel on Retina and a full
/// point at 1x, where half a pixel of coverage reads on some sides of a
/// shape and vanishes on others.
pub fn hairline_at(points: f32, scale: f32) -> f32 {
    if scale.is_finite() && scale > 0. {
        points.max(1. / scale)
    } else {
        points
    }
}

/// [`hairline_at`] for the main window.
pub fn hairline(points: f32) -> f32 {
    hairline_at(points, SCALE.with(|cell| cell.get()))
}

/// The edge of a list card (Coming up, Settings): one device pixel of a
/// darker grey, as in the reference, which reads as light as a 1 pt border
/// of `t.border`. Dark mode keeps `t.border`.
pub fn card_edge(t: Theme) -> Hsla {
    if is_dark() { t.border } else { rgb(0xd4d3cb).into() }
}

/// Whether the app is in its dark theme, for a view that is drawn outside
/// the main one and has no settings of its own to ask.
pub fn is_dark() -> bool {
    DARK.with(|flag| flag.get())
}

/// The focus ring's colour: the focus gold at 60%, in its light and dark
/// values.
pub fn focus_ring() -> Hsla {
    if DARK.with(|flag| flag.get()) {
        rgba(0xe0af2f99).into()
    } else {
        rgba(0xa5790099).into()
    }
}

/// The focused Ask bar's edge and the halo round it: the brand gold as a
/// line at 60% inside a hard-edged 3 pt halo of the same gold at 22%, in its
/// light and dark values.
pub fn ask_ring() -> (Hsla, Hsla) {
    let gold: Hsla = if is_dark() {
        rgb(0xe0af2f).into()
    } else {
        rgb(0xa57900).into()
    };
    (gold.opacity(0.6), gold.opacity(0.22))
}

thread_local! {
    /// The focus handle of each reachable control, by its element ID, and
    /// handles made ahead of time for controls that are new this frame
    /// (making one needs the app, which a control being built does not have).
    static FOCUS_HANDLES: std::cell::RefCell<std::collections::HashMap<ElementId, FocusHandle>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
    static SPARE_HANDLES: std::cell::RefCell<Vec<FocusHandle>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Call once per frame, before the window's controls are built.
pub fn provision_focus(cx: &mut App) {
    SPARE_HANDLES.with(|spare| {
        let mut spare = spare.borrow_mut();
        while spare.len() < 96 {
            spare.push(cx.focus_handle().tab_stop(true));
        }
    });
    // Controls come and go (rows of long lists above all): start over now
    // and then rather than keep a handle for every row ever drawn.
    FOCUS_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if handles.len() > 4096 {
            handles.clear();
        }
    });
}

fn focus_handle_for(id: Option<ElementId>) -> Option<FocusHandle> {
    let id = id?;
    FOCUS_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some(handle) = handles.get(&id) {
            return Some(handle.clone());
        }
        let handle = SPARE_HANDLES.with(|spare| spare.borrow_mut().pop())?;
        handles.insert(id, handle.clone());
        Some(handle)
    })
}

/// The keyboard focus outline: 1.5 px of the focus colour, 1 px clear of
/// the control.
const RING_WIDTH: f32 = 1.5;
const RING_OFFSET: f32 = 1.;

/// Makes a clickable control reachable with the keyboard: Tab and Shift-Tab
/// stop on it, Return and Space press it (GPUI turns those into its click),
/// and it shows the focus ring while it has the keyboard focus.
///
/// The ring is painted, not laid out: a border-only shape just outside the
/// control, following its corners, that takes no room and moves nothing. It
/// shows only while focus is being moved with the keyboard; a control that
/// was clicked has the focus but no ring. A control that clips its content
/// gets the ring just inside its edge instead. Call it after the control's
/// corner radius is set.
pub trait Reach: Sized {
    fn reach(self) -> Self;
}

impl<E: StatefulInteractiveElement + ParentElement> Reach for E {
    fn reach(mut self) -> Self {
        let interactivity = self.interactivity();
        let Some(handle) = focus_handle_for(interactivity.element_id.clone()) else {
            // More new controls in one frame than handles were made for:
            // still a stop, without the ring until the next frame.
            return self.focusable().tab_stop(true);
        };
        let radii = interactivity.base_style.corner_radii.clone();
        let clips = interactivity.base_style.overflow.y == Some(Overflow::Hidden)
            || interactivity.base_style.overflow.x == Some(Overflow::Hidden);
        let focus = handle.clone();
        let ring = canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                if !KEYBOARD_FOCUS.with(|flag| flag.get()) || !focus.is_focused(window) {
                    return;
                }
                let rem = window.rem_size();
                let limit = bounds.size.width.min(bounds.size.height) / 2.;
                let corner = |radius: Option<AbsoluteLength>| {
                    radius.map_or(px(0.), |radius| radius.to_pixels(rem)).min(limit)
                };
                let grow = if clips { 0. } else { RING_OFFSET + RING_WIDTH };
                let corners = Corners {
                    top_left: corner(radii.top_left) + px(grow),
                    top_right: corner(radii.top_right) + px(grow),
                    bottom_right: corner(radii.bottom_right) + px(grow),
                    bottom_left: corner(radii.bottom_left) + px(grow),
                };
                window.paint_quad(quad(
                    bounds.dilate(px(grow)),
                    corners,
                    gpui::transparent_black(),
                    px(RING_WIDTH),
                    focus_ring(),
                    BorderStyle::Solid,
                ));
            },
        )
        .absolute()
        .inset_0();
        self.track_focus(&handle)
            // A control that is otherwise revealed on hover is shown
            // whenever it has the focus.
            .focus(|style| style.opacity(1.))
            .child(ring)
    }
}

/// The scroll thumb: 6 px wide, 3 px in from the edges of its container.
const THUMB_WIDTH: f32 = 6.;
const THUMB_INSET: f32 = 3.;
const THUMB_SHORTEST: f32 = 28.;
/// How long the thumb stays after the last scroll, and how long it takes to go.
const THUMB_HOLD: std::time::Duration = std::time::Duration::from_millis(900);
const THUMB_FADE: std::time::Duration = std::time::Duration::from_millis(250);

thread_local! {
    /// When each scroll container was last scrolled by hand, and whether a
    /// timer is already waiting to start that thumb's fade.
    static THUMBS: std::cell::RefCell<std::collections::HashMap<&'static str, (std::time::Instant, bool)>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// Where the thumb of a scroll container sits: its top and height inside a
/// track `track` long, for a viewport `viewport` tall scrolled `offset` down
/// out of `range`. `None` when there is nothing to scroll.
pub fn thumb_span(viewport: f32, range: f32, offset: f32, track: f32) -> Option<(f32, f32)> {
    if range < 1. || viewport <= 0. || track <= THUMB_SHORTEST {
        return None;
    }
    let length = (track * viewport / (viewport + range)).clamp(THUMB_SHORTEST, track);
    let at = (offset / range).clamp(0., 1.);
    Some(((track - length) * at, length))
}

/// A position cue for the scroll container tracked by `scroll`: a thin thumb
/// at its right edge, shown while the user scrolls it and for a moment
/// after, then faded out. It is painted over the container and takes no
/// space in the layout; add it to the container's parent, after it. It is an
/// indicator only: it cannot be dragged.
pub fn scroll_thumb(name: &'static str, scroll: &ScrollHandle) -> impl IntoElement {
    let scroll = scroll.clone();
    canvas(
        |_, _, _| {},
        move |_, _, window, _| {
            let viewport = scroll.bounds();
            window.on_mouse_event(move |event: &ScrollWheelEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble || !viewport.contains(&event.position) {
                    return;
                }
                let arm = THUMBS.with(|thumbs| {
                    let mut thumbs = thumbs.borrow_mut();
                    let armed = thumbs.get(name).is_some_and(|(_, armed)| *armed);
                    thumbs.insert(name, (std::time::Instant::now(), true));
                    !armed
                });
                window.refresh();
                if !arm {
                    return;
                }
                // One timer per thumb: it wakes the window when the thumb is
                // due to fade, so nothing is drawn while it simply stays.
                let handle = window.window_handle();
                cx.spawn(async move |cx| {
                    loop {
                        let left = THUMBS.with(|thumbs| {
                            thumbs
                                .borrow()
                                .get(name)
                                .map_or(std::time::Duration::ZERO, |(at, _)| {
                                    THUMB_HOLD.saturating_sub(at.elapsed())
                                })
                        });
                        if left.is_zero() {
                            break;
                        }
                        cx.background_executor().timer(left).await;
                    }
                    THUMBS.with(|thumbs| {
                        if let Some((_, armed)) = thumbs.borrow_mut().get_mut(name) {
                            *armed = false;
                        }
                    });
                    let _ = handle.update(cx, |_, window, _| window.refresh());
                })
                .detach();
            });
            let Some(since) = THUMBS.with(|thumbs| thumbs.borrow().get(name).map(|(at, _)| at.elapsed()))
            else {
                return;
            };
            let strength = if since <= THUMB_HOLD {
                1.
            } else {
                1. - (since - THUMB_HOLD).as_secs_f32() / THUMB_FADE.as_secs_f32()
            };
            if strength <= 0. {
                return;
            }
            if since > THUMB_HOLD {
                window.request_animation_frame();
            }
            let track = f32::from(viewport.size.height) - 2. * THUMB_INSET;
            let Some((top, length)) = thumb_span(
                f32::from(viewport.size.height),
                f32::from(scroll.max_offset().height),
                -f32::from(scroll.offset().y),
                track,
            ) else {
                return;
            };
            let ink: Hsla = if DARK.with(|flag| flag.get()) {
                rgb(0xfcfcf8).into()
            } else {
                rgb(0x292929).into()
            };
            window.paint_quad(fill(
                Bounds::new(
                    point(
                        viewport.right() - px(THUMB_INSET + THUMB_WIDTH),
                        viewport.top() + px(THUMB_INSET + top),
                    ),
                    size(px(THUMB_WIDTH), px(length)),
                ),
                ink.opacity(0.32 * strength),
            )
            .corner_radii(px(THUMB_WIDTH / 2.)));
        },
    )
    .absolute()
    .size_0()
}

/// Opacity of a control that is present but cannot be used right now.
pub const DISABLED: f32 = 0.45;

/// Corner radius of every floating popover: menus, the details card, the
/// event pickers, the language menu and the model list.
pub const POPOVER_RADIUS: f32 = 12.;

/// The dimming behind an overlay, at full presence; multiply by the
/// overlay's `pose.fade` so it arrives and leaves with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scrim {
    /// The command palette: 10% black.
    Palette,
    /// A dialog that only informs: a click outside dismisses it.
    Dialog,
    /// A form holding typed input: darker, and
    /// a stray click outside does not throw the input away.
    Form,
}

pub fn scrim(kind: Scrim, fade: f32) -> Hsla {
    Hsla::from(match kind {
        Scrim::Palette => rgba(0x0000001a),
        Scrim::Dialog => rgba(0x29292930),
        Scrim::Form => rgba(0x00000099),
    })
    .opacity(fade)
}

/// What lies behind a bar floating 26 px above the bottom of a page: the page
/// colour from just below the bar's top down to the window edge, easing in
/// over the 54 px above that. Content fades out as it nears the bar and nothing peeks
/// out underneath it. `side` is how far
/// the bar's wrapper is inset from the page on each side. Not interactive.
pub fn bar_scrim(t: Theme, side: f32) -> Div {
    // Text scrolling under the bar dissolves into the page instead of being
    // cut off behind the controls: a long, eased fade (two linear pieces,
    // gentle then steep) from 48 px above the bar to just below its top
    // edge, then the solid page colour down to the window edge, as in the
    // reference.
    div()
        .absolute()
        .top(px(-48.))
        // Past the window's edge: nothing of the page shows below the bar.
        .bottom(px(-48.))
        .left(px(-side))
        .right(px(-side))
        .flex()
        .flex_col()
        .child(div().h(px(40.)).flex_shrink_0().bg(linear_gradient(
            180.,
            linear_color_stop(t.bg.opacity(0.), 0.),
            linear_color_stop(t.bg.opacity(0.6), 1.),
        )))
        .child(div().h(px(14.)).flex_shrink_0().bg(linear_gradient(
            180.,
            linear_color_stop(t.bg.opacity(0.6), 0.),
            linear_color_stop(t.bg, 1.),
        )))
        .child(div().flex_1().bg(t.bg))
}

/// A mid-grey wash: darkens light fills and lightens dark ones.
pub fn neutral_wash(alpha: f32) -> Hsla {
    hsla(0., 0., 0.5, alpha)
}

/// Eased hover and press feedback for a list row with `radius` corners.
pub fn row_feedback(t: Theme, radius: f32) -> crate::motion::Tactile {
    crate::motion::tactile(t.hover).radius(radius)
}

fn tool_label(id: &ElementId, symbol: &str) -> Option<SharedString> {
    let name = match id {
        ElementId::Name(name) | ElementId::NamedInteger(name, _) => name.as_ref(),
        _ => "",
    };
    let label = match name {
        "sidebar-toggle" => "Hide sidebar · ⌘B",
        "open-sidebar" => "Show sidebar · ⌘B",
        "sidebar-search" | "collection-search" => "Search notes · ⌘K",
        "footer-settings" => "Settings",
        "footer-window" => "Workspace menu",
        "footer-invite" => "Invite people",
        "footer-shortcuts" => "Keyboard shortcuts",
        "back-to-notes" => "Back to notes · Esc",
        "close-search" => "Close search · Esc",
        "close-dialog" => "Close dialog · Esc",
        "note-copy" => "Copy note",
        "note-favorite" => "Add or remove favorite",
        "note-more" => "Note actions",
        "note-templates" => "Change template",
        "note-assistant-toggle" => "Open or close chat",
        "close-note-assistant" => "Close chat",
        "send-note-chat" => "Send message · ⌘Enter",
        "clear-chat" => "New chat",
        "calendar-prev" => "Previous week",
        "calendar-next" => "Next week",
        _ => match symbol {
            "search" => "Search notes",
            "copy" => "Copy",
            "more" => "More actions",
            "close" => "Close",
            "settings" | "sliders-horizontal" => "Settings",
            "keyboard" => "Keyboard shortcuts",
            "trash" => "Move to trash",
            "share" => "Share",
            "arrow-up" => "Send message",
            "mic" => "Start recording",
            "pause" => "Pause recording",
            "stop" => "End recording",
            _ => return None,
        },
    };
    Some(label.into())
}

thread_local! {
    static POPOVER_OPEN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Tell tooltips whether a menu or popover is open. While one is, a trigger's
/// tooltip stays hidden instead of sitting on top of the menu it opened.
pub fn set_popover_open(open: bool) {
    POPOVER_OPEN.with(|flag| flag.set(open));
}

/// Whether a menu or popover is open, so a tooltip should stay hidden.
pub fn popover_open() -> bool {
    POPOVER_OPEN.with(|flag| flag.get())
}

/// The tooltip's box: 12 pt text on the inverted ink, 8 × 4 padding, 8 pt
/// corners. Shared by GPUI's pointer tooltips and the bar's own.
pub fn tip_box(label: SharedString, theme: Theme) -> Div {
    div()
        .px(px(8.))
        .py(px(4.))
        .rounded(px(8.))
        .bg(theme.text)
        .text_color(theme.bg)
        .font_family(FONT)
        .text_size(px(12.))
        .line_height(px(16.))
        .shadow_sm()
        .child(label)
}

/// Give a control the app's tooltip, hidden while a popover is open.
pub(crate) fn with_tooltip(
    element: Stateful<Div>,
    label: SharedString,
    theme: Theme,
) -> Stateful<Div> {
    // GPUI 0.2.2 shows these after 500 ms; the delay is the framework's.
    element.tooltip(move |_, cx| {
        cx.new(|_| ToolTip {
            label: label.clone(),
            theme,
            hidden: POPOVER_OPEN.with(|flag| flag.get()),
        })
        .into()
    })
}

pub struct ToolTip {
    label: SharedString,
    theme: Theme,
    hidden: bool,
}

impl ToolTip {
    pub fn new(label: impl Into<SharedString>, theme: Theme) -> Self {
        Self {
            label: label.into(),
            theme,
            hidden: false,
        }
    }
}

impl Render for ToolTip {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        // A tooltip belongs to the pointer. Once the window is no longer the
        // active one the pointer has gone elsewhere and no movement will
        // arrive to take the tooltip down, so it goes now and stays gone.
        if !window.is_window_active() {
            self.hidden = true;
        }
        if self.hidden || POPOVER_OPEN.with(|flag| flag.get()) {
            return div();
        }
        tip_box(self.label.clone(), self.theme)
    }
}

pub fn heading(text: impl Into<SharedString>, t: Theme) -> Div {
    div()
        .font_family(DISPLAY)
        .text_size(px(24.))
        .line_height(px(28.))
        .text_color(t.text)
        .child(text.into())
}

pub fn small(text: impl Into<SharedString>, t: Theme) -> Div {
    div()
        .text_size(px(12.))
        .line_height(px(16.))
        .text_color(t.muted)
        .child(text.into())
}

/// Split a clock string into its digits and a trailing AM/PM marker.
pub fn time_parts(time: &str) -> (&str, Option<&str>) {
    let time = time.trim();
    for suffix in ["AM", "PM", "am", "pm"] {
        if let Some(digits) = time.strip_suffix(suffix) {
            return (digits.trim_end(), Some(suffix));
        }
    }
    (time, None)
}

/// A time in 12 px secondary ink with the AM/PM marker set as small capitals
/// (a separate, smaller uppercase run sharing the digits' baseline).
pub fn time_label(time: &str, t: Theme) -> Div {
    let (digits, meridiem) = time_parts(time);
    div()
        .flex()
        .items_baseline()
        .gap(px(3.))
        .flex_shrink_0()
        .whitespace_nowrap()
        .text_size(px(12.))
        .line_height(px(16.))
        .text_color(t.muted)
        .child(digits.to_string())
        .when_some(meridiem, |label, meridiem| {
            label.child(
                div()
                    .text_size(px(8.5))
                    .line_height(px(16.))
                    .child(meridiem.to_uppercase()),
            )
        })
}

/// Shortcut keycaps shown at the trailing edge of a menu or palette row.
pub fn keycaps(keys: &[&'static str], t: Theme) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(2.))
        .children(keys.iter().map(|key| {
            div()
                .min_w(px(16.))
                .h(px(16.))
                .px(px(3.))
                .rounded(px(4.))
                .bg(t.fill)
                .flex()
                .items_center()
                .justify_center()
                .font_family("Inter")
                .text_size(px(11.))
                .line_height(px(14.))
                .text_color(t.muted)
                .child(*key)
        }))
}

/// A shortcut at the end of a menu row, as macOS menus write it: plain
/// muted glyphs, no keycaps (a keycap's fill is the row's own highlight, so
/// it disappears under the pointer).
pub fn shortcut_hint(keys: &'static str, t: Theme) -> Div {
    div()
        .flex_shrink_0()
        .pr(px(2.))
        .font_family("Inter")
        .text_size(px(12.))
        .line_height(px(16.))
        .text_color(t.muted)
        .child(keys)
}

/// The mark of an update that is known: a dot of the brand's bright gold,
/// in both themes (the light theme's deeper accent reads brown this small).
pub fn update_dot(size: f32) -> Div {
    div()
        .size(px(size))
        .flex_shrink_0()
        .rounded_full()
        .bg(rgb(SEND_GOLD))
}

/// A 12 px medium-weight secondary caption: date groups, section labels.
pub fn caption(text: impl Into<SharedString>, t: Theme) -> Div {
    small(text, t).font_weight(FontWeight::MEDIUM)
}

pub fn separator(t: Theme) -> Div {
    div().h(px(1.)).w_full().bg(t.border).flex_shrink_0()
}

pub fn init(cx: &mut App) {
    let fonts = Assets::iter()
        .filter(|p| p.starts_with("fonts/") && p.ends_with(".ttf"))
        .filter_map(|p| Assets::get(&p).map(|f| f.data))
        .collect();
    if let Err(error) = cx.text_system().add_fonts(fonts) {
        eprintln!("Font loading: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // `gpui::*` re-exports its own `test` attribute; these are plain unit tests.
    use std::prelude::v1::test;

    #[test]
    fn hairlines_are_never_thinner_than_a_device_pixel() {
        assert_eq!(hairline_at(0.5, 2.), 0.5);
        assert_eq!(hairline_at(0.5, 1.), 1.);
        assert_eq!(hairline_at(0.75, 2.), 0.75);
        assert_eq!(hairline_at(0.5, 3.), 0.5);
        assert_eq!(hairline_at(0.5, 0.), 0.5);
    }

    #[test]
    fn the_scroll_thumb_spans_the_track_in_proportion_and_never_leaves_it() {
        // Nothing to scroll, or no room to draw in: no thumb.
        assert_eq!(thumb_span(400., 0., 0., 394.), None);
        assert_eq!(thumb_span(400., 800., 0., 20.), None);
        // A third of the content is in view: the thumb is a third of the track.
        let (top, length) = thumb_span(400., 800., 0., 394.).unwrap();
        assert_eq!(top, 0.);
        assert!((length - 394. / 3.).abs() < 0.01);
        // Half way down it is centred; at the end it touches the bottom.
        let (top, _) = thumb_span(400., 800., 400., 394.).unwrap();
        assert!((top - (394. - length) / 2.).abs() < 0.01);
        let (top, _) = thumb_span(400., 800., 800., 394.).unwrap();
        assert!((top + length - 394.).abs() < 0.01);
        // An hour of transcript: the thumb keeps a length that can be seen,
        // and an offset past either end (a bounce) does not push it out.
        let (top, length) = thumb_span(400., 400_000., 500_000., 394.).unwrap();
        assert_eq!(length, THUMB_SHORTEST);
        assert_eq!(top + length, 394.);
        assert_eq!(thumb_span(400., 400_000., -50., 394.).unwrap().0, 0.);
    }

    #[test]
    fn a_button_confirms_briefly_then_swaps_its_own_glyph_back() {
        use std::time::{Duration, Instant};
        let id: ElementId = ("copy-chat-response", 2usize).into();
        let other: ElementId = "transcript-copy".into();
        let start = Instant::now();
        assert_eq!(confirming(&id, start), None);
        CONFIRMED.with(|confirmed| *confirmed.borrow_mut() = Some((id.clone(), start)));
        // Only the button that did the work shows the check.
        assert_eq!(confirming(&id, start), Some(true));
        assert_eq!(confirming(&other, start), None);
        assert_eq!(
            confirming(&id, start + CONFIRM_FOR - Duration::from_millis(1)),
            Some(true)
        );
        // Then its glyph swaps back in, and after that it is a plain icon.
        assert_eq!(confirming(&id, start + CONFIRM_FOR), Some(false));
        assert_eq!(
            confirming(&id, start + CONFIRM_FOR + Duration::from_secs(1)),
            None
        );
        CONFIRMED.with(|confirmed| *confirmed.borrow_mut() = None);
    }

    #[test]
    fn times_split_into_digits_and_a_small_caps_marker() {
        assert_eq!(time_parts("9:00 AM"), ("9:00", Some("AM")));
        assert_eq!(
            time_parts("10:30 – 11:00 pm"),
            ("10:30 – 11:00", Some("pm"))
        );
        assert_eq!(time_parts(" 14:05 "), ("14:05", None));
        assert_eq!(time_parts(""), ("", None));
    }

    #[test]
    fn theme_uses_nominal_tokens_and_keeps_tints_translucent() {
        let hex = |color: Hsla| {
            let Rgba { r, g, b, .. } = color.into();
            let channel = |value: f32| (value * 255.).round() as u32;
            (channel(r) << 16) | (channel(g) << 8) | channel(b)
        };
        let light = Theme::new(false);
        assert_eq!(hex(light.bg), 0xf7f7f2);
        assert_eq!(hex(light.sidebar), 0xf2f2ec);
        assert_eq!(hex(light.muted), 0x72726e);
        assert_eq!(hex(light.secondary), 0x4e4d4b);
        assert_eq!(hex(light.danger), 0xcf3527);
        // Hover and fill tint whatever they sit on; hover is the lighter one.
        assert!(light.hover.a < light.fill.a && light.fill.a < 0.2);
        assert!(light.hairline.a < 1. && light.border.a == 1.);
        let dark = Theme::new(true);
        assert_eq!(hex(dark.bg), 0x292929);
        assert_eq!(hex(dark.secondary), 0xacada8);
        assert!(dark.hover.a < dark.fill.a);
        // Warning ink must be readable on its own fill in both themes
        // (the dark banner used to be about 1.8:1).
        let luminance = |color: Hsla| {
            let Rgba { r, g, b, .. } = color.into();
            let linear = |c: f32| {
                if c <= 0.03928 {
                    c / 12.92
                } else {
                    ((c + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
        };
        for theme in [light, dark] {
            let (ink, fill) = (luminance(theme.warning_ink), luminance(theme.warning_fill));
            let contrast = (ink.max(fill) + 0.05) / (ink.min(fill) + 0.05);
            assert!(contrast >= 4.5, "warning contrast {contrast}");
        }
        // Secondary ink is always stronger than muted ink against the canvas.
        assert!(light.secondary.l < light.muted.l && dark.secondary.l > dark.muted.l);
    }
}
