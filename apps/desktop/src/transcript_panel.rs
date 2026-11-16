//! The live transcript panel's own parts: the language and microphone the
//! person chooses in its footer, the text of a line that is still being
//! spoken, the highlights of Find, and the shape that the note's bar becomes
//! when the panel opens. The panel itself is laid out in `note.rs`.
use crate::{
    app::{Cereal, Menu},
    audio::InputDevice,
    motion::{Pose, Spring, Track},
    ui::*,
};
use gpui::{prelude::*, *};
use std::{
    cell::RefCell,
    collections::HashMap,
    ops::Range,
    time::{Duration, Instant},
};

// ------------------------------------------------------------- language

/// A language live transcription can be told to expect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Language {
    /// ISO 639-1, as the server's live hello takes it.
    pub code: &'static str,
    pub name: &'static str,
}

/// No language is named: the provider detects it, utterance by utterance.
pub const AUTO: Language = Language {
    code: "auto",
    name: "Auto-detect",
};

pub const LANGUAGES: [Language; 16] = [
    Language { code: "en", name: "English" },
    Language { code: "es", name: "Spanish" },
    Language { code: "fr", name: "French" },
    Language { code: "de", name: "German" },
    Language { code: "pt", name: "Portuguese" },
    Language { code: "it", name: "Italian" },
    Language { code: "nl", name: "Dutch" },
    Language { code: "hi", name: "Hindi" },
    Language { code: "ja", name: "Japanese" },
    Language { code: "ko", name: "Korean" },
    Language { code: "zh", name: "Chinese" },
    Language { code: "ru", name: "Russian" },
    Language { code: "ar", name: "Arabic" },
    Language { code: "tr", name: "Turkish" },
    Language { code: "pl", name: "Polish" },
    Language { code: "sv", name: "Swedish" },
];

/// What a Mac that has never been asked transcribes in: English when macOS
/// itself is in English (`system` is its first preferred language, "en-US"),
/// and whatever the provider detects otherwise. Detection alone takes a
/// mumble in an English meeting for Japanese or Chinese now and then.
pub fn default_language(system: Option<&str>) -> Language {
    let english = system.is_some_and(|system| {
        let system = system.trim().to_ascii_lowercase();
        system == "en" || system.starts_with("en-") || system.starts_with("en_")
    });
    if english { LANGUAGES[0] } else { AUTO }
}

/// The language in use, from what was saved on this Mac (`None` when the
/// person never chose; a code this build does not know counts as that).
pub fn language_of(saved: Option<&str>, system: Option<&str>) -> Language {
    match saved {
        Some(code) if code == AUTO.code => AUTO,
        Some(code) => LANGUAGES
            .iter()
            .copied()
            .find(|language| language.code == code)
            .unwrap_or_else(|| default_language(system)),
        None => default_language(system),
    }
}

/// What the live hello says of `language`: its code, or nothing for
/// detection.
pub fn hello_language(language: Language) -> Option<String> {
    (language != AUTO).then(|| language.code.to_owned())
}

fn system_language() -> Option<&'static str> {
    static SYSTEM: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    SYSTEM.get_or_init(crate::audio::system_language).as_deref()
}

/// The language live transcription uses on this Mac.
pub fn language() -> Language {
    language_of(
        crate::desktop_preferences::meeting()
            .transcript_language
            .as_deref(),
        system_language(),
    )
}

/// The `language` of the next live stream's hello.
pub fn stream_language() -> Option<String> {
    hello_language(language())
}

/// The microphone a recording uses: the saved one while it is plugged in,
/// otherwise the Mac's default input, otherwise whatever there is.
pub fn chosen_device<'a>(devices: &'a [InputDevice], saved: Option<&str>) -> Option<&'a InputDevice> {
    saved
        .and_then(|uid| devices.iter().find(|device| device.uid == uid))
        .or_else(|| devices.iter().find(|device| device.is_default))
        .or(devices.first())
}

// ---------------------------------------------------------------- state

/// What the panel remembers between frames.
#[derive(Default)]
pub struct PanelState {
    /// The microphones plugged in, as last listed.
    pub devices: Vec<InputDevice>,
    listed: Option<Instant>,
    /// Listening to the microphone for the sound popover's meter while
    /// nothing is being recorded. Dropped with the popover.
    meter: Option<crate::audio::LevelCheck>,
    meter_polling: bool,
    meter_level: f32,
    /// Where the transcript was when Find opened (its scroll offset, and
    /// whether it was following the newest text), put back when Find closes.
    find_restore: Option<(Point<Pixels>, bool)>,
    /// The microphone and language the running capture started with.
    pub capture: Option<(Option<String>, &'static str)>,
}

/// The eight segments of the sound popover's meter that `level` lights. A
/// microphone that is being heard always lights the first, so the meter
/// reads as alive in a quiet room.
pub fn meter_segments(level: f32, listening: bool) -> usize {
    if !listening {
        return 0;
    }
    let level = if level.is_finite() { level.clamp(0., 1.) } else { 0. };
    // Speech sits low in the range; the curve gives it most of the meter.
    ((level.powf(0.6) * 8.).round() as usize).clamp(1, 8)
}

// ------------------------------------------------------ Find highlights

/// The yellow of a match and the orange of the one Find is on; the text on
/// either is dark in both themes.
pub const MARK: Rgba = Rgba { r: 1., g: 0.878, b: 0.2, a: 1. };
pub const MARK_CURRENT: Rgba = Rgba { r: 1., g: 0.6, b: 0.11, a: 1. };
pub const MARK_INK: Rgba = Rgba { r: 0.16, g: 0.16, b: 0.16, a: 1. };

/// The boxes behind one match: one per line of wrapped text it is on, as
/// `(left, right, top)`. `position` gives where the character at a byte
/// index starts; like the text layout it answers the *end of the line
/// above* for the first character of a wrapped line, which is told apart by
/// the next index being on another line. `left` is where lines start.
/// White space left at the end of a line by a wrap is not marked.
pub fn mark_runs(
    text: &str,
    range: Range<usize>,
    left: f32,
    position: impl Fn(usize) -> Option<(f32, f32)>,
) -> Vec<(f32, f32, f32)> {
    struct Run {
        from: f32,
        to: f32,
        top: f32,
        /// Where trailing white space began, if the run ends with some.
        space: Option<f32>,
    }
    let mut runs: Vec<Run> = Vec::new();
    let Some(matched) = text.get(range.clone()) else {
        return Vec::new();
    };
    for (at, character) in matched.char_indices() {
        let start = range.start + at;
        let end = start + character.len_utf8();
        if character == '\n' {
            continue;
        }
        let (Some(here), Some(next)) = (position(start), position(end)) else {
            continue;
        };
        let (from, to, top) = if (next.1 - here.1).abs() > 0.5 {
            (left, next.0, next.1)
        } else {
            (here.0, next.0, here.1)
        };
        if to <= from {
            continue;
        }
        match runs.last_mut() {
            Some(run) if (run.top - top).abs() <= 0.5 => {
                if character.is_whitespace() {
                    run.space.get_or_insert(from);
                } else {
                    run.space = None;
                }
                run.to = run.to.max(to);
            }
            _ => {
                // The line above ended here: its trailing space is the wrap's.
                if let Some(previous) = runs.last_mut()
                    && let Some(space) = previous.space
                {
                    previous.to = space;
                }
                runs.push(Run {
                    from,
                    to,
                    top,
                    space: character.is_whitespace().then_some(from),
                });
            }
        }
    }
    runs.into_iter()
        .filter(|run| run.to > run.from)
        .map(|run| (run.from, run.to, run.top))
        .collect()
}

/// Transcript text with Find's matches marked: a rounded box behind each,
/// on every line it wraps across, and dark text on it.
pub struct MarkedText {
    text: SharedString,
    /// Byte ranges of the matches, in order, and whether Find is on each.
    marks: Vec<(Range<usize>, bool)>,
    layout: Option<TextLayout>,
}

pub fn marked_text(text: impl Into<SharedString>, marks: Vec<(Range<usize>, bool)>) -> MarkedText {
    MarkedText {
        text: text.into(),
        marks,
        layout: None,
    }
}

impl IntoElement for MarkedText {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for MarkedText {
    type RequestLayoutState = AnyElement;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, AnyElement) {
        let styled = StyledText::new(self.text.clone()).with_highlights(self.marks.iter().map(
            |(range, _)| {
                (
                    range.clone(),
                    HighlightStyle {
                        color: Some(MARK_INK.into()),
                        ..Default::default()
                    },
                )
            },
        ));
        self.layout = Some(styled.layout().clone());
        let mut element = styled.into_any_element();
        (element.request_layout(window, cx), element)
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        element: &mut AnyElement,
        window: &mut Window,
        cx: &mut App,
    ) {
        element.prepaint(window, cx);
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        element: &mut AnyElement,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(layout) = &self.layout {
            let line = f32::from(layout.line_height());
            let left = f32::from(layout.bounds().left());
            let position = |index: usize| {
                layout
                    .position_for_index(index)
                    .map(|at| (f32::from(at.x), f32::from(at.y)))
            };
            for (range, current) in &self.marks {
                let color: Hsla = if *current { MARK_CURRENT } else { MARK }.into();
                for (from, to, top) in mark_runs(&self.text, range.clone(), left, position) {
                    window.paint_quad(
                        fill(
                            Bounds::from_corners(
                                point(px(from - 1.5), px(top + 0.5)),
                                point(px(to + 1.5), px(top + line - 0.5)),
                            ),
                            color,
                        )
                        .corner_radii(px(3.5)),
                    );
                }
            }
        }
        element.paint(window, cx);
    }
}

// ------------------------------------------------- a line being spoken

/// How strongly words that are not final yet are drawn.
pub const INTERIM_INK: f32 = 0.55;
/// A bubble easing to the size of new text: quick and without overshoot,
/// so words never wobble.
const BUBBLE_GROW: Spring = Spring {
    millis: 240,
    damping: 1.,
};
/// The three dots after words still being spoken: 4 pt dots, 8 pt apart.
const DOT_RADIUS: f32 = 2.;
const DOT_SWELL: f32 = 0.5;
const DOT_PITCH: f32 = 8.;
const DOTS_WIDTH: f32 = 2. * DOT_PITCH + 2. * (DOT_RADIUS + DOT_SWELL);
/// One pass of the wave along the dots.
pub const DOTS_PERIOD: Duration = Duration::from_millis(1250);
/// Room kept for the dots at the end of the text: no-break spaces, so they
/// never wrap away from the last word and nothing of them is drawn.
const DOTS_ROOM: &str = "\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}";

/// How far dot `index` of three is into its swell, 0..=1, after `turns` of
/// the wave: each swells in turn for half a period and rests for the other.
pub fn dot_pulse(turns: f32, index: usize) -> f32 {
    let at = (turns - index as f32 * 0.16).rem_euclid(1.);
    if at < 0.5 {
        (std::f32::consts::PI * at / 0.5).sin().powi(2)
    } else {
        0.
    }
}

struct Grow {
    width: Track,
    height: Track,
    seen: Instant,
}

thread_local! {
    /// The size each live bubble is drawn at on its way to its text's size,
    /// by row. A final keeps the track of the line it replaces, so the
    /// bubble settles from where it was.
    static GROW: RefCell<HashMap<u64, Grow>> = RefCell::new(HashMap::new());
}

/// The size to draw bubble `key` at, now that its text measures `target`,
/// and whether it is still moving. `inherits` is the row it took over from;
/// `from_width` is where a bubble that has just appeared starts.
fn grow(key: u64, inherits: Option<u64>, target: Size<Pixels>, from_width: Option<f32>) -> (f32, f32, bool) {
    let now = Instant::now();
    let reduced = crate::motion::reduce_motion();
    let (width, height) = (f32::from(target.width), f32::from(target.height));
    GROW.with(|grow| {
        let mut grow = grow.borrow_mut();
        grow.retain(|_, entry| now.saturating_duration_since(entry.seen) < Duration::from_secs(3));
        let inherited = match inherits {
            Some(from) if !grow.contains_key(&key) => grow.remove(&from),
            _ => None,
        };
        let entry = grow.entry(key).or_insert_with(|| {
            inherited.unwrap_or_else(|| Grow {
                width: Track::rest(from_width.map_or(width, |from| from.min(width)), now),
                height: Track::rest(height, now),
                seen: now,
            })
        });
        entry.seen = now;
        entry.width.spring_to(width, BUBBLE_GROW, now, reduced);
        entry.height.spring_to(height, BUBBLE_GROW, now, reduced);
        let shown = crate::motion::shown_at(now);
        (
            entry.width.value(shown),
            entry.height.value(shown),
            !entry.width.settled(shown) || !entry.height.settled(shown),
        )
    })
}

/// The bubble of a line that is still being spoken, or has just become
/// final. Its fill eases to the size of each new text instead of jumping
/// (anchored on the side the speaker's bubbles sit), the words keep their
/// place while it does, three dots pulse after the last word, and when the
/// final text arrives it takes over in place: the same bubble, the interim
/// words fading out under the final ones as the dots fade away.
pub struct LiveBubble {
    /// The row, and the row it replaced (an interim line, for its final).
    key: u64,
    inherits: Option<u64>,
    text: SharedString,
    /// The interim words the final text replaced.
    ghost: Option<SharedString>,
    own: bool,
    fill: Hsla,
    ink: Hsla,
    /// Corner radii: top left, top right, bottom right, bottom left.
    radii: [f32; 4],
    /// `None` while the line is being spoken, then 0..=1 as it settles.
    settle: Option<f32>,
    /// The line appeared this moment: its bubble grows out of the dots.
    fresh: bool,
    turns: f32,
    padding: (f32, f32),
    size: (f32, f32),
    layout: Option<TextLayout>,
    rect: Bounds<Pixels>,
}

#[allow(clippy::too_many_arguments)]
pub fn live_bubble(
    key: u64,
    inherits: Option<u64>,
    text: impl Into<SharedString>,
    ghost: Option<SharedString>,
    own: bool,
    fill: Hsla,
    ink: Hsla,
    radii: [f32; 4],
    settle: Option<f32>,
    fresh: bool,
) -> LiveBubble {
    LiveBubble {
        key,
        inherits,
        text: text.into(),
        ghost,
        own,
        fill,
        ink,
        radii,
        settle,
        fresh,
        turns: 0.,
        padding: (8., 6.),
        size: (13., 17.875),
        layout: None,
        rect: Bounds::default(),
    }
}

impl LiveBubble {
    /// Where the dots' wave is, in turns.
    pub fn turns(mut self, turns: f32) -> Self {
        self.turns = turns;
        self
    }
    /// Padding (horizontal, vertical), text size and line height.
    pub fn metrics(mut self, padding: (f32, f32), size: (f32, f32)) -> Self {
        self.padding = padding;
        self.size = size;
        self
    }
}

impl IntoElement for LiveBubble {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for LiveBubble {
    type RequestLayoutState = AnyElement;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, AnyElement) {
        let spoken = self.settle.is_none();
        let settle = self.settle.unwrap_or(0.).clamp(0., 1.);
        // The interim words fade out under the final ones only when the two
        // differ; the same words simply firm up.
        let ghost = self.ghost.clone().filter(|ghost| !spoken && *ghost != self.text);
        let ink = if spoken {
            INTERIM_INK
        } else if ghost.is_some() {
            settle
        } else {
            INTERIM_INK + (1. - INTERIM_INK) * settle
        };
        let styled = if spoken {
            StyledText::new(format!("{}{DOTS_ROOM}", self.text))
        } else {
            StyledText::new(self.text.clone())
        };
        self.layout = Some(styled.layout().clone());
        let (pad_x, pad_y) = self.padding;
        let mut element = div()
            .relative()
            .max_w_full()
            .min_w_0()
            .px(px(pad_x))
            .py(px(pad_y))
            .text_size(px(self.size.0))
            .line_height(px(self.size.1))
            .text_color(self.ink.opacity(ink))
            .child(styled)
            .when_some(ghost, |bubble, ghost| {
                bubble.child(
                    div()
                        .absolute()
                        .top(px(pad_y))
                        .left(px(pad_x))
                        .right(px(pad_x))
                        .text_color(self.ink.opacity(INTERIM_INK * (1. - settle)))
                        .child(ghost),
                )
            })
            .into_any_element();
        (element.request_layout(window, cx), element)
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        element: &mut AnyElement,
        window: &mut Window,
        cx: &mut App,
    ) {
        let start = self.fresh.then_some(2. * self.padding.0 + DOTS_WIDTH);
        let (width, height, moving) = grow(self.key, self.inherits, bounds.size, start);
        let left = if self.own {
            bounds.right() - px(width)
        } else {
            bounds.left()
        };
        self.rect = Bounds::new(point(left, bounds.top()), size(px(width), px(height)));
        // The words stand on the bubble's leading edge wherever it is.
        window.with_element_offset(point(left - bounds.left(), px(0.)), |window| {
            element.prepaint(window, cx)
        });
        if moving {
            crate::motion::stats::cause("bubble");
            window.request_animation_frame();
        }
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        element: &mut AnyElement,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let rect = self.rect;
        let [top_left, top_right, bottom_right, bottom_left] = self.radii;
        let limit = rect.size.width.min(rect.size.height) / 2.;
        window.paint_quad(fill(rect, self.fill).corner_radii(Corners {
            top_left: px(top_left).min(limit),
            top_right: px(top_right).min(limit),
            bottom_right: px(bottom_right).min(limit),
            bottom_left: px(bottom_left).min(limit),
        }));
        let Some(layout) = self.layout.clone() else {
            element.paint(window, cx);
            return;
        };
        let spoken = self.settle.is_none();
        let line = layout.line_height();
        // Where the dots go: after the last word, but never past the end of
        // the bubble while it is still growing to fit that word.
        let end = rect.right() - px(self.padding.0 + DOTS_WIDTH);
        let natural = spoken
            .then(|| layout.position_for_index(self.text.len() + '\u{a0}'.len_utf8()))
            .flatten();
        let last_line = rect.bottom() - px(self.padding.1) - line;
        let (dots, pinned) = match natural {
            Some(at) if at.x <= end + px(0.5) => (at, false),
            Some(at) => (point(end, at.y), true),
            None => (point(end, last_line), false),
        };
        let words = if pinned {
            Bounds::from_corners(rect.origin, point(dots.x - px(3.), rect.bottom()))
        } else {
            rect
        };
        window.with_content_mask(Some(ContentMask { bounds: words }), |window| {
            element.paint(window, cx)
        });
        let strength = match self.settle {
            None => 1.,
            Some(settle) => (1. - settle.clamp(0., 1.)).powi(2),
        };
        if strength > 0.02 {
            window.with_content_mask(Some(ContentMask { bounds: rect }), |window| {
                for index in 0..3 {
                    let pulse = dot_pulse(self.turns, index);
                    let radius = px(DOT_RADIUS + DOT_SWELL * pulse);
                    let centre = point(
                        dots.x + px(DOT_RADIUS + DOT_SWELL + DOT_PITCH * index as f32),
                        dots.y + line / 2. + px(0.5 - pulse),
                    );
                    window.paint_quad(
                        fill(
                            Bounds::from_corners(
                                point(centre.x - radius, centre.y - radius),
                                point(centre.x + radius, centre.y + radius),
                            ),
                            self.ink.opacity((0.4 + 0.5 * pulse) * strength),
                        )
                        .corner_radii(radius),
                    );
                }
            });
        }
    }
}

// --------------------------------------------- the bar becoming the panel

/// The bar becoming the panel: brisk, with a slight overshoot of the
/// panel's top edge (closing takes the shorter, monotonic exit).
pub const OPEN: Spring = Spring {
    millis: 400,
    damping: 0.74,
};

/// The note's bar, 56 pt tall, and the gap between its two capsules.
const BAR_HEIGHT: f32 = 56.;
const BAR_GAP: f32 = 8.;
/// The panel's corners: concentric with the footer's 56 pt capsule, 4 pt in.
pub const PANEL_RADIUS: f32 = 32.;
/// The two capsules have become one shape by this much of the way open.
const FUSED_BY: f32 = 0.36;

/// Where the bar stood, in the panel's own frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BarGeometry {
    /// The bar's width, centred on the panel.
    pub width: f32,
    /// How far the bar's bottom edge is above the panel's.
    pub lift: f32,
    /// The record capsule at the bar's leading end.
    pub capsule: f32,
    /// The Ask capsule fills the rest of the bar (it is absent while the
    /// chat panel is up).
    pub ask: bool,
}

fn smooth(x: f32) -> f32 {
    let x = x.clamp(0., 1.);
    x * x * (3. - 2. * x)
}

/// How much of the panel's content shows at morph level `level`: nothing
/// until the shape is nearly half open, all of it by nine tenths.
pub fn panel_content_fade(level: f32) -> f32 {
    smooth((level - 0.45) / 0.45)
}

/// How much of the bar's own content (the Ask field) is left at `level`.
pub fn bar_content_fade(level: f32) -> f32 {
    1. - smooth(level / 0.3)
}

/// The shape at morph level `level` (0 the bar, 1 the panel, a little over
/// while the spring overshoots), in a frame whose origin is the panel's top
/// left corner: one or two rounded rectangles as `(left, top, right,
/// bottom, [top left, top right, bottom right, bottom left] radii)`.
pub fn fuse_shapes(panel: (f32, f32), bar: BarGeometry, level: f32) -> Vec<(f32, f32, f32, f32, [f32; 4])> {
    let (width, height) = panel;
    let open = level.clamp(0., 1.);
    let mix = |from: f32, to: f32, t: f32| from + (to - from) * t;
    let bar_left = (width - bar.width) / 2.;
    let bar_right = if bar.ask { bar_left + bar.width } else { bar_left + bar.capsule };
    let left = mix(bar_left, 0., open);
    let right = mix(bar_right, width, open);
    let bottom = mix(height - bar.lift, height, open);
    // Only the height rides the spring's overshoot: the panel's top edge
    // settles softly while its sides and foot stay put.
    let tall = mix(BAR_HEIGHT, height, level).max(BAR_HEIGHT * 0.9);
    let top = bottom - tall;
    let radius = mix(BAR_HEIGHT / 2., PANEL_RADIUS, open).min(tall / 2.);
    let fused = smooth(open / FUSED_BY);
    if !bar.ask || fused >= 1. {
        return vec![(left, top, right, bottom, [radius; 4])];
    }
    // The capsules lean into the gap and their facing ends square off
    // until they are one edge.
    let inner = radius * (1. - fused);
    let seam = left + bar.capsule + BAR_GAP / 2.;
    let gap = BAR_GAP / 2. * (1. - fused);
    vec![
        (left, top, seam - gap, bottom, [radius, inner, inner, radius]),
        (seam + gap, top, right, bottom, [inner, radius, radius, inner]),
    ]
}

/// The transcript panel as one shape with the note's bar: closed it is the
/// bar's two capsules exactly, open it is the panel, and in between the
/// capsules fuse and grow upward (and split back apart on the way down).
/// The panel's content keeps its final layout throughout and fades in once
/// the shape is mostly open; the record controls are drawn at full strength
/// all the way, sliding the couple of points between where they sit in the
/// bar and in the panel's footer; the bar's Ask field fades out as the
/// capsules meet. Also the panel's edge at rest: a hairline and a soft
/// shadow, like the bar's.
pub struct Fuse {
    size: (f32, f32),
    bar: BarGeometry,
    pose: Pose,
    theme: Theme,
    content: Option<Div>,
    /// The record controls, and how far from their place in the panel they
    /// sit in the bar.
    pinned: Option<(Div, Point<Pixels>)>,
    leaving: Option<Div>,
}

pub fn fuse(size: (f32, f32), bar: BarGeometry, pose: Pose, theme: Theme, content: Div) -> Fuse {
    Fuse {
        size,
        bar,
        pose,
        theme,
        content: Some(content),
        pinned: None,
        leaving: None,
    }
}

impl Fuse {
    pub fn pinned(mut self, controls: Div, from: Point<Pixels>) -> Self {
        self.pinned = Some((controls, from));
        self
    }
    pub fn leaving(mut self, content: Div) -> Self {
        self.leaving = Some(content);
        self
    }
    /// Morph level and overall opacity: with reduced motion the shape is
    /// simply the panel, fading.
    fn level(&self) -> (f32, f32) {
        if crate::motion::reduce_motion() {
            (1., self.pose.fade.clamp(0., 1.))
        } else {
            (self.pose.value.max(0.), 1.)
        }
    }
}

impl IntoElement for Fuse {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

pub struct FuseParts {
    content: AnyElement,
    pinned: Option<(AnyElement, Point<Pixels>)>,
    leaving: Option<AnyElement>,
}

impl Element for Fuse {
    type RequestLayoutState = FuseParts;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, FuseParts) {
        let (level, alpha) = self.level();
        let at_rest = level == 1. && alpha >= 1.;
        let shown = |element: Div, opacity: f32| {
            if opacity < 1. {
                element.opacity(opacity).into_any_element()
            } else {
                element.into_any_element()
            }
        };
        let content = self.content.take().expect("a fuse is laid out once");
        let mut content = shown(
            content,
            if at_rest { 1. } else { panel_content_fade(level) * alpha },
        );
        let mut pinned = self
            .pinned
            .take()
            .map(|(controls, from)| (shown(controls, alpha), from));
        let leaving = bar_content_fade(level) * alpha;
        let mut leaving = self
            .leaving
            .take()
            .filter(|_| leaving > 0.01)
            .map(|content| shown(content, leaving));
        let mut ids = vec![content.request_layout(window, cx)];
        if let Some((controls, _)) = &mut pinned {
            ids.push(controls.request_layout(window, cx));
        }
        if let Some(content) = &mut leaving {
            ids.push(content.request_layout(window, cx));
        }
        let mut style = Style::default();
        style.refine(
            &StyleRefinement::default()
                .relative()
                .w(px(self.size.0))
                .h(px(self.size.1))
                .flex_shrink_0(),
        );
        (
            window.request_layout(style, ids, cx),
            FuseParts {
                content,
                pinned,
                leaving,
            },
        )
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        parts: &mut FuseParts,
        window: &mut Window,
        cx: &mut App,
    ) {
        let (level, _) = self.level();
        let closed = 1. - level.clamp(0., 1.);
        parts.content.prepaint(window, cx);
        if let Some((controls, from)) = &mut parts.pinned {
            window.with_element_offset(point(from.x * closed, from.y * closed), |window| {
                controls.prepaint(window, cx)
            });
        }
        if let Some(content) = &mut parts.leaving {
            content.prepaint(window, cx);
        }
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        parts: &mut FuseParts,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let t = self.theme;
        let (level, alpha) = self.level();
        let open = level.clamp(0., 1.);
        let dark = t.bg.l < 0.5;
        let shapes: Vec<(Bounds<Pixels>, Corners<Pixels>)> = fuse_shapes(self.size, self.bar, level)
            .into_iter()
            .map(|(left, top, right, bottom, radii)| {
                (
                    Bounds::from_corners(
                        bounds.origin + point(px(left), px(top)),
                        bounds.origin + point(px(right), px(bottom)),
                    ),
                    Corners {
                        top_left: px(radii[0]),
                        top_right: px(radii[1]),
                        bottom_right: px(radii[2]),
                        bottom_left: px(radii[3]),
                    },
                )
            })
            .collect();
        // The bar's own soft shadow, and under the open panel a wider,
        // fainter one that arrives with its height.
        let shadows = [
            BoxShadow {
                color: Hsla::from(rgba(0x0000000d)).opacity(0.05 * alpha),
                offset: point(px(0.), px(4.)),
                blur_radius: px(6.),
                spread_radius: px(-2.),
            },
            BoxShadow {
                color: Hsla::from(rgba(0x000000ff)).opacity(if dark { 0.22 } else { 0.07 } * open * alpha),
                offset: point(px(0.), px(10.)),
                blur_radius: px(28.),
                spread_radius: px(-10.),
            },
        ];
        for (rect, radii) in &shapes {
            window.paint_shadows(*rect, *radii, &shadows);
        }
        for (rect, radii) in &shapes {
            window.paint_quad(fill(*rect, t.card.opacity(alpha)).corner_radii(*radii));
        }
        let whole = shapes
            .iter()
            .map(|(rect, _)| *rect)
            .reduce(|all, rect| all.union(&rect))
            .unwrap_or(bounds);
        let at_rest = level == 1. && alpha >= 1.;
        if at_rest {
            parts.content.paint(window, cx);
        } else if panel_content_fade(level) > 0.004 {
            // Masks are rectangles: keep the content inside the corners'
            // arcs while the shape is small enough for it to reach them.
            let radius = shapes.first().map_or(0., |(_, radii)| f32::from(radii.top_left));
            let inset = px(radius * (1. - std::f32::consts::FRAC_1_SQRT_2) * (1. - open));
            window.with_content_mask(Some(ContentMask { bounds: whole.inset(inset) }), |window| {
                parts.content.paint(window, cx)
            });
        }
        if let Some(content) = &mut parts.leaving {
            let mask = shapes.last().map_or(whole, |(rect, _)| *rect);
            window.with_content_mask(Some(ContentMask { bounds: mask }), |window| {
                content.paint(window, cx)
            });
        }
        if let Some((controls, _)) = &mut parts.pinned {
            controls.paint(window, cx);
        }
        // The hairline: an outline of its own just outside the surface (just
        // inside it in the dark theme), as on the bar.
        let width = crate::ui::hairline(0.5);
        let out = if dark { 0. } else { width };
        for (rect, radii) in &shapes {
            let grown = |radius: Pixels| if radius > px(0.) { radius + px(out) } else { radius };
            window.paint_quad(quad(
                rect.dilate(px(out)),
                Corners {
                    top_left: grown(radii.top_left),
                    top_right: grown(radii.top_right),
                    bottom_right: grown(radii.bottom_right),
                    bottom_left: grown(radii.bottom_left),
                },
                transparent_black(),
                px(width),
                t.hairline.opacity(t.hairline.a * alpha),
                BorderStyle::default(),
            ));
        }
    }
}

// ------------------------------------------------------- the panel's parts

/// Width of the Find field and of the popovers that open from the footer.
pub const FIND_WIDTH: f32 = 220.;
const SOUND_WIDTH: f32 = 264.;
const MENU_WIDTH: f32 = 236.;

impl Cereal {
    /// List the microphones again if the list is older than a moment: when
    /// the panel opens, and whenever one of its menus does.
    pub(crate) fn refresh_input_devices(&mut self) {
        let now = Instant::now();
        if self
            .panel
            .listed
            .is_some_and(|at| now.duration_since(at) < Duration::from_secs(2))
        {
            return;
        }
        self.panel.listed = Some(now);
        self.panel.devices = crate::audio::input_devices();
    }

    /// The microphone recordings use right now, by name.
    pub(crate) fn microphone_name(&self) -> SharedString {
        let saved = crate::desktop_preferences::meeting().microphone;
        chosen_device(&self.panel.devices, saved.as_deref())
            .map_or("No microphone".into(), |device| device.name.clone().into())
    }

    /// What the capture layer is told before a capture starts, and what the
    /// running capture is remembered to have started with.
    pub(crate) fn apply_capture_choices(&mut self) {
        let saved = crate::desktop_preferences::meeting().microphone;
        // A microphone that is no longer plugged in gives way to the default.
        let chosen = plugged_in(&crate::audio::input_devices(), saved.as_deref());
        crate::audio::set_input_device(chosen.as_deref());
        self.panel.capture = Some((chosen, language().code));
    }

    /// The level the sound popover's meter shows, 0..=1, and whether the
    /// microphone is being heard at all.
    fn meter_level(&self) -> (f32, bool) {
        if self.recording.is_active() {
            return (self.recording.microphone_level(), !self.microphone_muted);
        }
        (self.panel.meter_level, self.panel.meter.is_some())
    }

    /// Listen to the microphone for the meter while the sound popover is up
    /// and nothing is recording (a recording has its own levels); let go of
    /// it otherwise. Called every frame the note is drawn.
    pub(crate) fn tend_meter(&mut self, cx: &mut Context<Self>) {
        let wanted = self.menu == Some(Menu::RecordingSettings)
            && !self.recording.is_active()
            && !self.microphone_muted;
        if !wanted {
            self.panel.meter = None;
            self.panel.meter_level = 0.;
            return;
        }
        if self.panel.meter.is_none() {
            let saved = crate::desktop_preferences::meeting().microphone;
            crate::audio::set_input_device(plugged_in(&self.panel.devices, saved.as_deref()).as_deref());
            self.panel.meter = crate::audio::LevelCheck::start(true, false);
        }
        if self.panel.meter.is_none() || std::mem::replace(&mut self.panel.meter_polling, true) {
            return;
        }
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(80))
                    .await;
                let Ok(keep) = this.update(cx, |this, cx| {
                    let Some(meter) = &this.panel.meter else {
                        return false;
                    };
                    let level = meter.levels().channels[0].level;
                    if meter_segments(level, true) != meter_segments(this.panel.meter_level, true) {
                        cx.notify();
                    }
                    this.panel.meter_level = level;
                    true
                }) else {
                    break;
                };
                if !keep {
                    break;
                }
            }
            let _ = this.update(cx, |this, _| this.panel.meter_polling = false);
        })
        .detach();
    }

    // ------------------------------------------------------------- Find

    pub(crate) fn transcript_find_begin(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let following = self.transcript_scroll.max_offset().height + self.transcript_scroll.offset().y
            <= px(48.);
        self.panel.find_restore = Some((self.transcript_scroll.offset(), following));
        self.transcript_find_open = true;
        self.summary.transcript_selection = None;
        self.transcript_find_index = 0;
        self.search.update(cx, |input, cx| {
            input.set_content("", cx);
            input.set_placeholder("Find", cx);
        });
        self.search.read(cx).focus_handle.focus(window);
        cx.notify();
    }

    /// Close Find and put the transcript back where it was.
    pub(crate) fn transcript_find_end(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.transcript_find_open {
            return;
        }
        self.transcript_find_open = false;
        self.transcript_find_index = 0;
        self.search.update(cx, |input, cx| input.set_content("", cx));
        // The query is empty again: nothing to reveal on the next frame.
        self.transcript_last_query.clear();
        match self.panel.find_restore.take() {
            Some((_, true)) | None => {
                self.transcript_scroll.scroll_to_bottom();
                crate::lazy::to_end("transcript");
            }
            Some((offset, false)) => self.transcript_scroll.set_offset(offset),
        }
        self.focus.focus(window);
        cx.notify();
    }

    /// The Find field: the query, then the count, the two steps, a divider
    /// and the close button, all inside one rounded field.
    pub(crate) fn transcript_find_field(
        &self,
        query: &str,
        index: usize,
        positions: Vec<usize>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let t = self.theme();
        let count = positions.len();
        let step = move |this: &mut Self, previous: bool, cx: &mut Context<Self>| {
            if let Some(next) = crate::note::transcript_find_step(this.transcript_find_index, count, previous) {
                this.transcript_find_index = next;
                crate::lazy::reveal("transcript", positions[next]);
            }
            cx.notify();
        };
        let mut field = div()
            .id("transcript-find-capsule")
            .w(px(FIND_WIDTH))
            .h(px(30.))
            .pl(px(12.))
            .pr(px(5.))
            .rounded_full()
            .bg(t.hover)
            .border(px(crate::ui::hairline(0.5)))
            .border_color(t.hairline)
            .flex()
            .items_center()
            // Return steps to the next match and Shift-Return to the one
            // before, ahead of the field's own handling of either.
            .capture_action(cx.listener({
                let step = step.clone();
                move |this, _: &crate::input::Enter, _, cx| {
                    step(this, false, cx);
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener({
                let step = step.clone();
                move |this, _: &crate::input::LineBreak, _, cx| {
                    step(this, true, cx);
                    cx.stop_propagation();
                }
            }))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .text_size(px(13.))
                    // The input's own box is 22 pt tall: a line of the same
                    // height puts the text on the field's centre line.
                    .line_height(px(22.))
                    .child(self.search.clone()),
            );
        if !query.is_empty() {
            field = field.child(
                div()
                    .flex_shrink_0()
                    .pl(px(6.))
                    .pr(px(4.))
                    .text_size(px(11.))
                    .line_height(px(16.))
                    .text_color(t.muted)
                    .whitespace_nowrap()
                    .child(if count == 0 {
                        "0/0".to_string()
                    } else {
                        format!("{}/{count}", index + 1)
                    }),
            );
            for previous in [true, false] {
                let tip = if previous { "Previous match" } else { "Next match" };
                let step = step.clone();
                field = field.child(
                    div()
                        .id(if previous {
                            "transcript-find-previous"
                        } else {
                            "transcript-find-next"
                        })
                        .size(px(22.))
                        .flex_shrink_0()
                        .rounded(px(6.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_default()
                        .when(count == 0, |button| button.opacity(DISABLED))
                        .child(crate::motion::tactile(t.hover).radius(6.))
                        .child(icon("footer-chevron", 16., t.secondary).with_transformation(
                            Transformation::rotate(radians(if previous {
                                std::f32::consts::PI
                            } else {
                                0.
                            })),
                        ))
                        .tooltip(move |_, cx| cx.new(|_| ToolTip::new(tip, t)).into())
                        .reach()
                        .on_click(cx.listener(move |this, _, _, cx| step(this, previous, cx))),
                );
            }
            field = field.child(div().mx(px(4.)).w(px(1.)).h(px(12.)).flex_shrink_0().bg(t.hairline));
        }
        field.child(
            tool_tip("transcript-find-close", "close", "Close Find", t)
                .size(px(22.))
                .flex_shrink_0()
                .on_click(cx.listener(|this, _, window, cx| this.transcript_find_end(window, cx))),
        )
    }

    // ---------------------------------------------------------- footer

    /// The right of the panel's footer: the microphone in use, the sound
    /// settings and the language of the transcript.
    pub(crate) fn transcript_footer_tools(&self, cx: &mut Context<Self>) -> Div {
        let t = self.theme();
        let choice = |id: &'static str,
                      glyph: Option<&'static str>,
                      label: SharedString,
                      menu: Menu,
                      cx: &mut Context<Self>| {
            div()
                .id(id)
                .h(px(28.))
                .min_w_0()
                .pl(px(8.))
                .pr(px(4.))
                .rounded(px(6.))
                .flex()
                .items_center()
                .gap(px(2.))
                .text_size(px(13.))
                .line_height(px(18.))
                .text_color(t.secondary)
                .cursor_default()
                .child(crate::motion::tactile(t.hover).radius(6.))
                .when_some(glyph, |control, glyph| {
                    control.child(div().pr(px(5.)).child(icon(glyph, 15., t.muted)))
                })
                .child(div().min_w_0().truncate().child(label))
                .child(icon("footer-chevron", 16., t.muted))
                .reach()
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.focus.focus(window);
                    this.refresh_input_devices();
                    this.open_menu(menu.clone());
                    cx.notify();
                }))
        };
        let language = language();
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .items_center()
            .justify_end()
            .gap(px(6.))
            .child(
                choice(
                    "transcript-microphone",
                    None,
                    self.microphone_name(),
                    Menu::RecordingInput,
                    cx,
                )
                .flex_shrink()
                .tooltip(move |_, cx| cx.new(|_| ToolTip::new("Microphone", t)).into()),
            )
            .child(
                tool_tip("transcript-audio-settings", "sliders-vertical", "Sound settings", t)
                    .flex_shrink_0()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.focus.focus(window);
                        this.refresh_input_devices();
                        this.open_menu(Menu::RecordingSettings);
                        cx.notify();
                    })),
            )
            .child(
                choice(
                    "transcript-language",
                    Some("translate"),
                    if language == AUTO { "Auto" } else { language.name }.into(),
                    Menu::RecordingLanguage,
                    cx,
                )
                .flex_shrink_0()
                .tooltip(move |_, cx| cx.new(|_| ToolTip::new("Transcript language", t)).into()),
            )
    }

    /// The popover of one of the footer's three controls, and where it
    /// stands: its width and how far its trailing edge is from the panel's.
    pub(crate) fn recording_popover(&self, menu: &Menu, cx: &mut Context<Self>) -> (Div, f32, f32) {
        match menu {
            Menu::RecordingInput => (self.microphone_menu(cx), MENU_WIDTH, 96.),
            Menu::RecordingLanguage => (self.language_menu(cx), 188., 8.),
            _ => (self.sound_settings(cx), SOUND_WIDTH, 60.),
        }
    }

    fn popover_shell(&self, width: f32) -> Div {
        let t = self.theme();
        div()
            .w(px(width))
            .flex()
            .flex_col()
            .rounded(px(POPOVER_RADIUS))
            .bg(t.card)
            .border_1()
            .border_color(t.border)
            .shadow_lg()
    }

    /// "Sound settings": the input in use and a live meter of it.
    fn sound_settings(&self, cx: &mut Context<Self>) -> Div {
        let t = self.theme();
        let (level, listening) = self.meter_level();
        let lit = meter_segments(level, listening);
        let green: Hsla = crate::recording_view::WAVEFORM.into();
        let gold: Hsla = rgb(0xe0af2f).into();
        self.popover_shell(SOUND_WIDTH)
            .p(px(8.))
            .gap(px(6.))
            .child(
                div()
                    .h(px(28.))
                    .pl(px(6.))
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(13.))
                            .line_height(px(18.))
                            .font_weight(FontWeight::MEDIUM)
                            .child("Sound settings"),
                    )
                    .child(
                        tool_tip("sound-settings", "open-external", "Open Sound settings", t)
                            .size(px(24.))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.menu = None;
                                cx.open_url(crate::desktop_preferences::SOUND_SETTINGS_URL);
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .px(px(10.))
                    .pt(px(9.))
                    .pb(px(11.))
                    .rounded(px(8.))
                    .bg(t.hover)
                    .flex()
                    .flex_col()
                    .gap(px(9.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .text_size(px(13.))
                            .line_height(px(18.))
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(t.secondary)
                                    .child("Audio input"),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_color(t.muted)
                                    .child(if self.microphone_muted {
                                        "Microphone off".into()
                                    } else {
                                        self.microphone_name()
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .id("sound-meter")
                            .h(px(4.))
                            .flex()
                            .gap(px(3.))
                            .children((0..8).map(|segment| {
                                div().flex_1().h_full().rounded_full().bg(if segment < lit {
                                    if segment < 5 { green } else { gold }
                                } else {
                                    t.fill
                                })
                            })),
                    ),
            )
    }

    /// One row of a footer menu: a label and a check when it is the choice.
    fn choice_row(
        &self,
        label: SharedString,
        chosen: bool,
        cx: &mut Context<Self>,
        action: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> Stateful<Div> {
        let t = self.theme();
        self.menu_row(label, "", cx, move |this, _, cx| {
            this.menu = None;
            action(this, cx);
            cx.notify();
        })
        .px(px(8.))
        .child(div().flex_1().min_w_0())
        .when(chosen, |row| row.child(icon("check", 14., t.secondary)))
    }

    /// Said under a menu whose choice the running recording cannot take up.
    fn next_start_note(&self) -> Div {
        let t = self.theme();
        div()
            .px(px(8.))
            .pt(px(6.))
            .pb(px(4.))
            .text_size(px(11.))
            .line_height(px(15.))
            .text_color(t.muted)
            .child("Applies when you next start recording")
    }

    fn microphone_menu(&self, cx: &mut Context<Self>) -> Div {
        let t = self.theme();
        let saved = crate::desktop_preferences::meeting().microphone;
        let chosen = chosen_device(&self.panel.devices, saved.as_deref()).map(|device| device.uid.clone());
        let mut menu = self
            .popover_shell(MENU_WIDTH)
            .p(px(4.))
            .child(self.stage.glide("menu").indicator(t.hover, 6.));
        if self.panel.devices.is_empty() {
            menu = menu.child(
                div()
                    .h(px(28.))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .text_size(px(13.))
                    .text_color(t.muted)
                    .child("No microphone found"),
            );
        }
        for device in &self.panel.devices {
            let uid = device.uid.clone();
            menu = menu.child(self.choice_row(
                device.name.clone().into(),
                chosen.as_ref() == Some(&device.uid),
                cx,
                move |this, cx| {
                    let uid = uid.clone();
                    if let Err(error) =
                        crate::desktop_preferences::update_meeting(|meeting| meeting.microphone = Some(uid))
                    {
                        this.show_toast(&error, cx);
                    }
                },
            ));
        }
        // A capture keeps the device it started with.
        let now = plugged_in(&self.panel.devices, saved.as_deref());
        let running = self.recording.is_active()
            && self
                .panel
                .capture
                .as_ref()
                .is_some_and(|(started, _)| *started != now);
        menu.when(running, |menu| menu.child(self.next_start_note()))
    }

    fn language_menu(&self, cx: &mut Context<Self>) -> Div {
        let t = self.theme();
        let current = language();
        let mut rows = div().flex().flex_col();
        for choice in std::iter::once(AUTO).chain(LANGUAGES) {
            rows = rows.child(self.choice_row(choice.name.into(), choice == current, cx, move |this, cx| {
                if let Err(error) = crate::desktop_preferences::update_meeting(|meeting| {
                    meeting.transcript_language = Some(choice.code.to_owned())
                }) {
                    this.show_toast(&error, cx);
                }
            }));
        }
        let running = self.recording.is_active()
            && self
                .panel
                .capture
                .as_ref()
                .is_some_and(|(_, code)| *code != current.code);
        self.popover_shell(188.)
            .p(px(4.))
            .child(self.stage.glide("menu").indicator(t.hover, 6.))
            .child(
                div()
                    .id("transcript-language-rows")
                    .max_h(px(336.))
                    .overflow_y_scroll()
                    .child(rows),
            )
            .when(running, |menu| menu.child(self.next_start_note()))
    }
}

/// The saved microphone if it is plugged in, as the capture layer is told
/// it: `None` follows the Mac's default input.
pub fn plugged_in(devices: &[InputDevice], saved: Option<&str>) -> Option<String> {
    saved
        .filter(|uid| devices.iter().any(|device| device.uid == *uid))
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    // `gpui::*` re-exports its own `test` attribute; these are plain unit tests.
    use std::prelude::v1::test;

    #[test]
    fn english_is_the_default_only_on_a_mac_set_to_english() {
        assert_eq!(default_language(Some("en-US")).code, "en");
        assert_eq!(default_language(Some("en")).code, "en");
        assert_eq!(default_language(Some("EN_GB")).code, "en");
        // "en" must be the language, not the start of another one's code.
        assert_eq!(default_language(Some("enm")), AUTO);
        assert_eq!(default_language(Some("ja-JP")), AUTO);
        assert_eq!(default_language(None), AUTO);
    }

    #[test]
    fn the_saved_language_wins_and_an_unknown_one_falls_back() {
        assert_eq!(language_of(Some("ja"), Some("en-US")).name, "Japanese");
        // Choosing detection is a choice: it is not replaced by the default.
        assert_eq!(language_of(Some("auto"), Some("en-US")), AUTO);
        assert_eq!(language_of(None, Some("en-US")).code, "en");
        assert_eq!(language_of(None, Some("fr-FR")), AUTO);
        assert_eq!(language_of(Some("xx"), Some("en-GB")).code, "en");
        assert_eq!(language_of(Some("xx"), Some("de-DE")), AUTO);
        // Every language offered has a distinct two-letter code.
        let mut codes: Vec<_> = LANGUAGES.iter().map(|language| language.code).collect();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), LANGUAGES.len());
        assert!(LANGUAGES.iter().all(|language| language.code.len() == 2));
    }

    #[test]
    fn the_hello_carries_a_code_and_never_the_word_auto() {
        assert_eq!(hello_language(AUTO), None);
        assert_eq!(hello_language(LANGUAGES[0]).as_deref(), Some("en"));
        assert_eq!(hello_language(language_of(Some("ko"), None)).as_deref(), Some("ko"));
    }

    #[test]
    fn the_language_preference_is_kept_with_the_meeting_preferences() {
        // Tests start from the defaults and never write the person's file.
        let before = crate::desktop_preferences::meeting();
        assert_eq!(before.transcript_language, None);
        let saved = serde_json::to_string(&crate::desktop_preferences::MeetingPreferences {
            transcript_language: Some("es".into()),
            microphone: Some("BuiltInMicrophoneDevice".into()),
            ..before.clone()
        })
        .unwrap();
        let read: crate::desktop_preferences::MeetingPreferences = serde_json::from_str(&saved).unwrap();
        assert_eq!(read.transcript_language.as_deref(), Some("es"));
        assert_eq!(read.microphone.as_deref(), Some("BuiltInMicrophoneDevice"));
        // A file from before the choice existed reads as "never chosen".
        let old: crate::desktop_preferences::MeetingPreferences =
            serde_json::from_str(r#"{"voice_isolation":false}"#).unwrap();
        assert_eq!((old.transcript_language, old.microphone), (None, None));
        assert!(!old.voice_isolation);
    }

    fn device(uid: &str, is_default: bool) -> InputDevice {
        InputDevice {
            uid: uid.into(),
            name: format!("{uid} microphone"),
            is_default,
        }
    }

    #[test]
    fn the_saved_microphone_is_used_while_it_is_plugged_in() {
        let devices = [device("built-in", true), device("usb", false)];
        assert_eq!(chosen_device(&devices, Some("usb")).unwrap().uid, "usb");
        assert_eq!(chosen_device(&devices, None).unwrap().uid, "built-in");
        // Unplugged: back to the Mac's default, not to nothing.
        assert_eq!(chosen_device(&devices, Some("headset")).unwrap().uid, "built-in");
        assert_eq!(chosen_device(&[device("usb", false)], None).unwrap().uid, "usb");
        assert!(chosen_device(&[], Some("usb")).is_none());
        // What the capture layer is told: the saved device, or to follow the default.
        assert_eq!(plugged_in(&devices, Some("usb")).as_deref(), Some("usb"));
        assert_eq!(plugged_in(&devices, Some("headset")), None);
        assert_eq!(plugged_in(&devices, None), None);
    }

    #[test]
    fn input_devices_are_read_from_the_native_list() {
        let devices = crate::audio::parse_input_devices(
            r#"{"default":"b","devices":[{"uid":"a","name":"USB Mic"},{"uid":"b","name":" MacBook Air Microphone "},{"uid":"","name":"x"},{"uid":"c","name":" "}]}"#,
        );
        assert_eq!(devices.len(), 2);
        assert_eq!((devices[0].name.as_str(), devices[0].is_default), ("USB Mic", false));
        assert_eq!((devices[1].name.as_str(), devices[1].is_default), ("MacBook Air Microphone", true));
        assert!(crate::audio::parse_input_devices("not json").is_empty());
        // Tests and review fixtures never ask Core Audio.
        assert!(crate::audio::input_devices().iter().any(|device| device.is_default));
    }

    #[test]
    fn the_meter_lights_more_segments_as_the_level_rises() {
        assert_eq!(meter_segments(0.5, false), 0);
        assert_eq!(meter_segments(0., true), 1);
        assert_eq!(meter_segments(f32::NAN, true), 1);
        assert_eq!(meter_segments(1., true), 8);
        assert_eq!(meter_segments(7., true), 8);
        let lit: Vec<_> = (0..=10).map(|step| meter_segments(step as f32 / 10., true)).collect();
        assert!(lit.windows(2).all(|pair| pair[0] <= pair[1]));
        assert!(meter_segments(0.3, true) >= 3);
    }

    /// Text laid out 10 pt a character and 20 pt a line, wrapped before the
    /// byte indices in `wraps`, answering like the text layout: the first
    /// character of a wrapped line reports the end of the line above.
    fn laid_out(wraps: &'static [usize]) -> impl Fn(usize) -> Option<(f32, f32)> {
        move |index| {
            let line = wraps.iter().filter(|wrap| **wrap < index).count();
            let start = if line == 0 { 0 } else { wraps[line - 1] };
            Some((100. + (index - start) as f32 * 10., 50. + line as f32 * 20.))
        }
    }

    #[test]
    fn a_match_is_marked_on_every_line_it_wraps_across() {
        let text = "review the budget before launch";
        // One line: a single box exactly over the word.
        assert_eq!(mark_runs(text, 11..17, 100., laid_out(&[])), [(210., 270., 50.)]);
        // The match starts a wrapped line: it is on that line, from its start.
        assert_eq!(mark_runs(text, 11..17, 100., laid_out(&[11])), [(100., 160., 70.)]);
        // "the budget" split by the wrap: two boxes, and the space the wrap
        // left at the end of the first line is not marked.
        assert_eq!(
            mark_runs(text, 7..17, 100., laid_out(&[11])),
            [(170., 200., 50.), (100., 160., 70.)]
        );
        // A space inside one line is part of the match.
        assert_eq!(mark_runs(text, 7..17, 100., laid_out(&[])), [(170., 270., 50.)]);
        // Three lines.
        assert_eq!(
            mark_runs(text, 0..text.len(), 100., laid_out(&[11, 18])).len(),
            3
        );
        // A range that is not on character boundaries marks nothing.
        assert!(mark_runs("né", 1..2, 0., laid_out(&[])).is_empty());
        assert!(mark_runs(text, 40..50, 0., laid_out(&[])).is_empty());
    }

    #[test]
    fn the_dots_swell_one_after_another_and_rest_in_between() {
        // At the start of a turn only the first dot is rising.
        assert!(dot_pulse(0.1, 0) > 0.3);
        assert_eq!(dot_pulse(0.1, 2), 0.);
        // Each peaks in turn, a sixth of a period apart.
        for index in 0..3 {
            let peak = 0.25 + index as f32 * 0.16;
            assert!((dot_pulse(peak, index) - 1.).abs() < 1e-4);
            assert!(dot_pulse(peak + 0.5, index) == 0.);
        }
        // The wave is continuous across turns: no jump where one ends.
        for index in 0..3 {
            assert!((dot_pulse(0.9999, index) - dot_pulse(1.0001, index)).abs() < 0.01);
            assert!((0. ..=1.).contains(&dot_pulse(3.37, index)));
        }
    }

    const BAR: BarGeometry = BarGeometry {
        width: 672.,
        lift: 6.,
        capsule: 96.,
        ask: true,
    };

    #[test]
    fn closed_the_shape_is_the_bar_and_open_it_is_the_panel() {
        // Closed: the record capsule and the Ask capsule, 8 pt apart, 56 pt
        // tall with round ends, 6 pt above the panel's foot.
        let closed = fuse_shapes((682., 465.), BAR, 0.);
        assert_eq!(
            closed,
            [
                (5., 403., 101., 459., [28.; 4]),
                (109., 403., 677., 459., [28.; 4]),
            ]
        );
        // Open: the panel exactly.
        assert_eq!(fuse_shapes((682., 465.), BAR, 1.), [(0., 0., 682., 465., [PANEL_RADIUS; 4])]);
        // On the way the capsules close the gap and square off where they
        // meet, then are one shape for the rest of the way.
        let meeting = fuse_shapes((682., 465.), BAR, 0.12);
        assert_eq!(meeting.len(), 2);
        let gap = meeting[1].0 - meeting[0].2;
        assert!(gap > 0. && gap < 8., "{gap}");
        assert!(meeting[0].4[1] < 28. && meeting[0].4[0] >= 28.);
        assert_eq!(fuse_shapes((682., 465.), BAR, FUSED_BY).len(), 1);
        // The spring's overshoot lifts the top edge only.
        let over = fuse_shapes((682., 465.), BAR, 1.04)[0];
        assert!(over.1 < 0. && (over.0, over.2, over.3) == (0., 682., 465.));
        // The shape only ever grows on the way open.
        let mut last = 0.;
        for step in 0..=50 {
            let shapes = fuse_shapes((682., 465.), BAR, step as f32 / 50.);
            let area: f32 = shapes.iter().map(|shape| (shape.2 - shape.0) * (shape.3 - shape.1)).sum();
            assert!(area >= last, "step {step}");
            last = area;
        }
        // With the chat panel up there is no Ask capsule to merge with.
        let alone = fuse_shapes((682., 465.), BarGeometry { ask: false, ..BAR }, 0.);
        assert_eq!(alone, [(5., 403., 101., 459., [28.; 4])]);
    }

    #[test]
    fn the_panel_content_arrives_after_the_shape_and_the_bar_content_leaves_first() {
        assert_eq!(panel_content_fade(0.), 0.);
        assert_eq!(panel_content_fade(0.45), 0.);
        assert!(panel_content_fade(0.7) > 0.4);
        assert_eq!(panel_content_fade(0.9), 1.);
        assert_eq!(panel_content_fade(1.05), 1.);
        assert_eq!(bar_content_fade(0.), 1.);
        assert_eq!(bar_content_fade(0.3), 0.);
        // The two are never on screen together.
        for step in 0..=20 {
            let level = step as f32 / 20.;
            assert!(panel_content_fade(level) == 0. || bar_content_fade(level) == 0.);
        }
    }
}
