//! What the welcome draws that the rest of the app does not have: the mark as
//! a small character, the step indicator, and the "little screens" that show
//! what each step sets up. Everything is made of the app's own shapes, icons
//! and palette, and moves on the springs in `motion.rs`.
use crate::{
    motion::{self, GOOEY, SNAPPY, Stage},
    onboarding::{Channel, Link, Permission, Step},
    ui::*,
};
use gpui::{prelude::*, *};
use std::{
    f32::consts::{FRAC_PI_2, PI, TAU},
    time::{Duration, Instant},
};

// ---------------------------------------------------------------------------
// Goo: circles of soft matter and the neck that joins them
// ---------------------------------------------------------------------------

type Spot = (f32, f32);

fn dot(window: &mut Window, center: Spot, radius: f32, color: Hsla) {
    if radius <= 0.05 {
        return;
    }
    window.paint_quad(
        fill(
            Bounds::new(
                point(px(center.0 - radius), px(center.1 - radius)),
                size(px(radius * 2.), px(radius * 2.)),
            ),
            color,
        )
        .corner_radii(px(radius)),
    );
}

fn capsule(window: &mut Window, left: f32, right: f32, middle: f32, height: f32, color: Hsla) {
    if right <= left || height <= 0.05 {
        return;
    }
    window.paint_quad(
        fill(
            Bounds::new(
                point(px(left), px(middle - height / 2.)),
                size(px(right - left), px(height)),
            ),
            color,
        )
        .corner_radii(px(height / 2.)),
    );
}

/// The neck of soft matter between two circles: where it leaves each circle
/// and the handles of the two curves that bound it, in the order they are
/// drawn (`from`, its handle, the far handle, `to`; then back). `None` when
/// the circles are too far apart to hold together, or one is inside the
/// other. `reach` is how many times their touching distance they may be
/// pulled apart before the neck snaps; it thins all the way there.
pub fn neck(c1: Spot, r1: f32, c2: Spot, r2: f32, reach: f32) -> Option<[Spot; 8]> {
    let (dx, dy) = (c2.0 - c1.0, c2.1 - c1.1);
    let distance = (dx * dx + dy * dy).sqrt();
    let touching = r1 + r2;
    let limit = touching * reach.max(1.01);
    if r1 <= 0. || r2 <= 0. || distance >= limit || distance <= (r1 - r2).abs() {
        return None;
    }
    // Overlapping circles join where they cross.
    let (u1, u2) = if distance < touching {
        (
            ((r1 * r1 + distance * distance - r2 * r2) / (2. * r1 * distance))
                .clamp(-1., 1.)
                .acos(),
            ((r2 * r2 + distance * distance - r1 * r1) / (2. * r2 * distance))
                .clamp(-1., 1.)
                .acos(),
        )
    } else {
        (0., 0.)
    };
    let closeness = if distance <= touching {
        1.
    } else {
        1. - (distance - touching) / (limit - touching)
    };
    let spread = 0.5 * closeness.powf(0.75);
    let between = dy.atan2(dx);
    let widest = ((r1 - r2) / distance).clamp(-1., 1.).acos();
    let a1 = between + u1 + (widest - u1) * spread;
    let a2 = between - u1 - (widest - u1) * spread;
    let a3 = between + PI - u2 - (PI - u2 - widest) * spread;
    let a4 = between - PI + u2 + (PI - u2 - widest) * spread;
    let at = |center: Spot, angle: f32, radius: f32| {
        (
            center.0 + radius * angle.cos(),
            center.1 + radius * angle.sin(),
        )
    };
    let (p1, p2, p3, p4) = (at(c1, a1, r1), at(c1, a2, r1), at(c2, a3, r2), at(c2, a4, r2));
    let span = ((p1.0 - p3.0).powi(2) + (p1.1 - p3.1).powi(2)).sqrt();
    let handle = (spread * 2.4).min(span / touching) * (distance * 2. / touching).min(1.);
    Some([
        p1,
        at(p1, a1 - FRAC_PI_2, r1 * handle),
        at(p3, a3 + FRAC_PI_2, r2 * handle),
        p3,
        p4,
        at(p4, a4 - FRAC_PI_2, r2 * handle),
        at(p2, a2 + FRAC_PI_2, r1 * handle),
        p2,
    ])
}

/// Two circles of soft matter, joined while they are close enough.
fn goo(window: &mut Window, c1: Spot, r1: f32, c2: Spot, r2: f32, reach: f32, color: Hsla) {
    if let Some(points) = neck(c1, r1, c2, r2, reach) {
        let spot = |(x, y): Spot| point(px(x), px(y));
        let mut path = PathBuilder::fill();
        path.move_to(spot(points[0]));
        path.cubic_bezier_to(spot(points[3]), spot(points[1]), spot(points[2]));
        path.line_to(spot(points[4]));
        path.cubic_bezier_to(spot(points[7]), spot(points[5]), spot(points[6]));
        path.close();
        if let Ok(path) = path.build() {
            window.paint_path(path, color);
        }
    }
    dot(window, c1, r1, color);
    dot(window, c2, r2, color);
}

// ---------------------------------------------------------------------------
// The mark as a character
// ---------------------------------------------------------------------------

/// How the mark holds itself right now.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MarkPose {
    /// 0 asleep (grey, eye shut) to 1 awake (gold).
    pub awake: f32,
    /// 0 shut to 1 open.
    pub eye: f32,
    /// Pixels off the ground, as a share of its size.
    pub lift: f32,
    /// Taller (positive) or flatter (negative), keeping its volume.
    pub stretch: f32,
    /// Radians.
    pub tilt: f32,
    /// 0 swallowed to 1 where the two crumbs rest; briefly above 1.
    pub crumbs: f32,
}

impl MarkPose {
    pub const REST: MarkPose = MarkPose {
        awake: 1.,
        eye: 1.,
        lift: 0.,
        stretch: 0.,
        tilt: 0.,
        crumbs: 1.,
    };
}

const WAKE: Duration = Duration::from_millis(900);
const HOP: Duration = Duration::from_millis(620);
const WOBBLE: Duration = Duration::from_millis(520);
const BLINK: Duration = Duration::from_millis(170);

/// The mark's small life: it wakes once, blinks now and then, hops when
/// something has worked and wobbles when something has not.
pub struct Character {
    woke: Option<Instant>,
    hop: Option<Instant>,
    wobble: Option<Instant>,
    blink: Instant,
    blinks: u32,
}

impl Character {
    /// Asleep until [`Character::wake`].
    pub fn asleep(now: Instant) -> Self {
        Self {
            woke: None,
            hop: None,
            wobble: None,
            blink: now + Duration::from_secs(4),
            blinks: 0,
        }
    }

    /// Already awake: a welcome that is resumed part way.
    pub fn awake(now: Instant) -> Self {
        Self {
            woke: now.checked_sub(WAKE).or(Some(now)),
            ..Self::asleep(now)
        }
    }

    pub fn wake(&mut self, at: Instant) {
        if self.woke.is_none() {
            self.woke = Some(at);
            self.blink = at + WAKE + Duration::from_millis(1800);
        }
    }

    pub fn hop(&mut self, now: Instant) {
        self.hop = Some(now);
        self.wobble = None;
    }

    pub fn wobble(&mut self, now: Instant) {
        self.wobble = Some(now);
    }

    /// The pose at `now`, whether it is moving, and when it next will.
    pub fn pose(&mut self, now: Instant, reduced: bool) -> (MarkPose, bool, Option<Instant>) {
        let share = |since: Instant, length: Duration| {
            now.saturating_duration_since(since).as_secs_f32() / motion::slow(length).as_secs_f32()
        };
        let mut pose = MarkPose::REST;
        let mut moving = false;
        match self.woke {
            None => {
                pose.awake = 0.;
                pose.eye = 0.;
                pose.crumbs = 0.;
                pose.stretch = -0.08;
                pose.tilt = -0.1;
            }
            Some(woke) if now < woke => {
                pose.awake = 0.;
                pose.eye = 0.;
                pose.crumbs = 0.;
                pose.stretch = -0.08;
                pose.tilt = -0.1;
                moving = true;
            }
            Some(woke) => {
                let t = share(woke, WAKE);
                if t < 1. {
                    moving = true;
                    pose = wake_pose(t);
                }
            }
        }
        if reduced {
            // Colour only: it is awake or it is not.
            let awake = pose.awake;
            pose = MarkPose::REST;
            pose.awake = awake;
            pose.eye = if awake > 0.5 { 1. } else { 0. };
            pose.crumbs = if awake > 0.5 { 1. } else { 0. };
            self.hop = None;
            self.wobble = None;
            return (pose, moving, None);
        }
        if let Some(started) = self.hop {
            let t = share(started, HOP);
            if t < 1. {
                moving = true;
                let (lift, stretch) = hop_pose(t);
                pose.lift = lift;
                pose.stretch += stretch;
                pose.eye = pose.eye.min(1. - 0.6 * (PI * t).sin().max(0.) * f32::from(t < 0.2));
            } else {
                self.hop = None;
            }
        }
        if let Some(started) = self.wobble {
            let t = share(started, WOBBLE);
            if t < 1. {
                moving = true;
                pose.tilt += wobble_tilt(t);
            } else {
                self.wobble = None;
            }
        }
        let mut next = None;
        if self.woke.is_some_and(|woke| share(woke, WAKE) >= 1.) {
            let t = if now >= self.blink {
                share(self.blink, BLINK)
            } else {
                -1.
            };
            if (0. ..1.).contains(&t) {
                moving = true;
                pose.eye = pose.eye.min(1. - (PI * t).sin());
            } else if t >= 1. {
                // Unevenly spaced, so it never reads as a metronome; now
                // and then twice in a row.
                self.blinks = self.blinks.wrapping_add(1);
                let gap = match self.blinks % 5 {
                    0 => 260,
                    1 => 4300,
                    2 => 3100,
                    3 => 5200,
                    _ => 3700,
                };
                self.blink = now + Duration::from_millis(gap);
                next = Some(self.blink);
            } else {
                next = Some(self.blink);
            }
        }
        (pose, moving, next)
    }
}

/// Waking: it straightens up out of its slouch, stretches a little too far,
/// the eye opens and the crumbs pop out of its mouth.
pub fn wake_pose(t: f32) -> MarkPose {
    let t = t.clamp(0., 1.);
    if t >= 1. {
        return MarkPose::REST;
    }
    let up = GOOEY.at((t / 0.55).min(1.));
    let yawn = (PI * ((t - 0.1) / 0.6).clamp(0., 1.)).sin();
    MarkPose {
        awake: motion::fade_curve((t / 0.5).min(1.)),
        eye: motion::fade_curve(((t - 0.3) / 0.25).clamp(0., 1.)),
        lift: 0.,
        stretch: -0.08 * (1. - up) + 0.1 * yawn,
        tilt: -0.1 * (1. - up),
        crumbs: GOOEY.at(((t - 0.45) / 0.55).clamp(0., 1.)),
    }
}

/// A hop: crouch, leave the ground stretched, land flat and settle.
/// Returns `(lift, stretch)`.
pub fn hop_pose(t: f32) -> (f32, f32) {
    let t = t.clamp(0., 1.);
    const CROUCH: f32 = 0.16;
    const LAND: f32 = 0.66;
    if t < CROUCH {
        let down = (PI * 0.5 * t / CROUCH).sin();
        (0., -0.13 * down)
    } else if t < LAND {
        let air = (t - CROUCH) / (LAND - CROUCH);
        let height = (PI * air).sin();
        // Stretched on the way up and again on the way down, round at the top.
        (0.3 * height, 0.12 * (1. - height) - 0.13 * (1. - air).powi(6))
    } else {
        let settle = (t - LAND) / (1. - LAND);
        (0., -0.14 * (1. - GOOEY.at(settle)) * (1. - settle))
    }
}

/// A wobble: a few quick shakes of the head that die away.
pub fn wobble_tilt(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    0.17 * (TAU * 3. * t).sin() * (1. - t).powf(1.6)
}

/// The app's gold, as a fill: the same in both themes (it is the dark
/// theme's accent and the colour of the pill's bars). The light theme's own
/// accent is a darker gold made for text, and stays the colour of text.
pub fn gold() -> Hsla {
    rgb(0xe0af2f).into()
}

/// What is written or drawn on gold: the app's dark ink, in both themes.
pub fn on_gold() -> Hsla {
    rgb(0x292929).into()
}

/// The mark, `size` px square, in the theme's ink, standing on `ground` (the
/// colour behind it, which is what its eyes are cut out of).
pub fn mark(size: f32, pose: MarkPose, t: Theme, ground: Hsla) -> Div {
    let color = motion::mix(motion::mix(ground, t.muted, 0.6), t.text, pose.awake);
    let (sx, sy) = (1. - pose.stretch * 0.7, 1. + pose.stretch);
    // It stretches from its feet, not from its middle.
    let rise = -pose.lift * size - (sy - 1.) * size * 0.43;
    let body = svg()
        .path("icons/cereal-bowl.svg")
        .size(px(size))
        .absolute()
        .top_0()
        .left_0()
        .text_color(color)
        .with_transformation(
            Transformation::scale(gpui::size(sx, sy))
                .with_translation(point(px(0.), px(rise)))
                .with_rotation(radians(pose.tilt)),
        );
    let details = canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let unit = size / 24.;
            let center = (
                f32::from(bounds.origin.x) + size / 2.,
                f32::from(bounds.origin.y) + size / 2.,
            );
            let (sin, cos) = pose.tilt.sin_cos();
            // The same scale, turn and shift the body is drawn with.
            let place = |x: f32, y: f32| {
                let (x, y) = ((x - 12.) * unit * sx, (y - 12.) * unit * sy);
                (
                    center.0 + x * cos - y * sin,
                    center.1 + x * sin + y * cos + rise,
                )
            };
            // Two small eyes, cut out of the body; shut to lines when asleep.
            let radius = 0.95 * unit;
            let open = pose.eye.clamp(0., 1.);
            let height = (2. * radius * open).max(0.9_f32.min(radius));
            let width = 2. * radius * (1. + 0.2 * (1. - open));
            for eye in [place(7.9, 5.6), place(11.3, 4.9)] {
                window.paint_quad(
                    fill(
                        Bounds::new(
                            point(px(eye.0 - width / 2.), px(eye.1 - height / 2.)),
                            gpui::size(px(width), px(height)),
                        ),
                        ground,
                    )
                    .corner_radii(px(height / 2.)),
                );
            }
            // The crumbs: out of its mouth to where they rest.
            let level = pose.crumbs.max(0.);
            if level > 0.01 {
                let mouth = (16.6, 9.4);
                for (rest, radius, lag) in [
                    ((21.835, 2.1485), 0.7, 0.),
                    ((22.1475, 6.5745), 1.1515, 0.12),
                ] {
                    let level = ((level - lag) / (1. - lag)).max(0.);
                    let x = mouth.0 + (rest.0 - mouth.0) * level;
                    let y = mouth.1 + (rest.1 - mouth.1) * level;
                    dot(window, place(x, y), radius * unit * level.min(1.15), color);
                }
            }
        },
    )
    .absolute()
    .inset_0();
    div()
        .relative()
        .size(px(size))
        .flex_shrink_0()
        .child(body)
        .child(details)
}

// ---------------------------------------------------------------------------
// The step indicator
// ---------------------------------------------------------------------------

/// Dots are 8 px on a 22 px pitch; the active one is a 20 by 10 px capsule.
pub const DOT: f32 = 8.;
pub const PITCH: f32 = 22.;
pub const BLOB: f32 = 20.;
/// How thick the active blob is at rest.
pub const BLOB_THICK: f32 = 10.;
/// The row's size: six dots' worth, and room for the last step's tick.
pub const INDICATOR: (f32, f32) = (5. * PITCH + BLOB, 16.);
const ALL_STEPS: [Step; 6] = [
    Step::Welcome,
    Step::Server,
    Step::Audio,
    Step::Calendar,
    Step::Pill,
    Step::Done,
];

/// Where the active blob is on its way from one dot to the next.
pub struct Travel {
    from: f32,
    to: f32,
    started: Option<Instant>,
}

impl Travel {
    pub fn at(slot: usize) -> Self {
        Self {
            from: slot as f32,
            to: slot as f32,
            started: None,
        }
    }

    pub fn go(&mut self, slot: usize, now: Instant) {
        let (low, high) = self.edges(now, false);
        self.from = (low + high) / 2. / PITCH;
        self.to = slot as f32;
        self.started = Some(now);
    }

    fn progress(&self, now: Instant) -> f32 {
        self.started.map_or(1., |started| {
            now.saturating_duration_since(started).as_secs_f32()
                / (GOOEY.duration().as_secs_f32() * 1.25)
        })
    }

    pub fn moving(&self, now: Instant) -> bool {
        self.progress(now) < 1.
    }

    /// The blob's left and right edges, in pixels from the first dot's
    /// centre: the edge facing the next dot runs ahead, the other lets go
    /// late and whips after it.
    pub fn edges(&self, now: Instant, reduced: bool) -> (f32, f32) {
        let rest = |slot: f32| (slot * PITCH - BLOB / 2., slot * PITCH + BLOB / 2.);
        if reduced {
            return rest(self.to);
        }
        motion::stretch(rest(self.from), rest(self.to), self.progress(now))
    }
}

/// How thick the blob is at a given length: it thins as it is pulled out and
/// fattens when it is squashed short, like anything that keeps its volume.
pub fn blob_thickness(length: f32) -> f32 {
    let pulled = length / BLOB - 1.;
    if pulled >= 0. {
        BLOB_THICK * (1. - 0.5 * (pulled / 1.4)).clamp(0.4, 1.)
    } else {
        BLOB_THICK * (1. - 0.9 * pulled).min(1.3)
    }
}

/// The row of dots: gold for the steps behind, pale for the steps ahead,
/// and a wider gold blob on the step that is showing. `merge` is how far,
/// 0..=1, they have run together at the end into one gold pill with a tick
/// on it; it may pass 1 a little while the last of them lands.
pub fn indicator(
    stage: &Stage,
    steps: &[Step],
    travel: &Travel,
    merge: f32,
    t: Theme,
    now: Instant,
    reduced: bool,
) -> Div {
    let done = gold();
    let ahead = motion::mix(t.bg, t.text, 0.14);
    let current = steps.len().saturating_sub(1).min(travel.to.round() as usize);
    let middle = (ALL_STEPS.len() as f32 - 1.) / 2. * PITCH;
    // The row is always as wide as six steps; a flow with five is centred
    // in it. A step that is not part of this flow (the calendar without a
    // server) sinks into the dot before it.
    let shift = stage.aim(
        ("welcome-dots-shift", 0),
        (ALL_STEPS.len() - steps.len()) as f32 * PITCH / 2.,
        GOOEY,
    );
    let mut dots = Vec::with_capacity(ALL_STEPS.len());
    let mut slot = 0usize;
    for step in ALL_STEPS {
        let present = steps.contains(&step);
        let home = if present {
            slot as f32
        } else {
            slot.saturating_sub(1) as f32
        };
        let x = stage.aim(("welcome-dot-x", step as u64), home, GOOEY) * PITCH + shift;
        let size = stage.aim(
            ("welcome-dot-size", step as u64),
            if present { 1. } else { 0. },
            GOOEY,
        );
        let passed = stage
            .aim(
                ("welcome-dot-passed", step as u64),
                if present && slot < current { 1. } else { 0. },
                SNAPPY,
            )
            .clamp(0., 1.);
        if present {
            slot += 1;
        }
        let pull = merge.clamp(0., 1.);
        dots.push((
            x + (middle - x) * pull,
            DOT / 2. * size.max(0.),
            motion::mix(motion::mix(ahead, done, passed), done, pull),
        ));
    }
    let (low, high) = travel.edges(now, reduced);
    let forward = travel.to >= travel.from;
    let accent = gold();
    let tick = merge.clamp(0., 1.);
    div()
        .relative()
        .w(px(INDICATOR.0))
        .h(px(INDICATOR.1))
        .flex_shrink_0()
        .child(
            canvas(
                |_, _, _| (),
                move |bounds, _, window, _| {
                    let origin = f32::from(bounds.origin.x) + BLOB / 2.;
                    let line = f32::from(bounds.origin.y) + INDICATOR.1 / 2.;
                    for (x, radius, color) in &dots {
                        dot(window, (origin + x, line), *radius, *color);
                    }
                    // The active blob: thick at its ends, thin between them
                    // while it is pulled, with the end it leaves behind the
                    // smaller of the two.
                    let pull = merge.clamp(0., 1.);
                    let length = high - low;
                    // Taking the other dots in makes it longer and fatter;
                    // only being pulled along thins it.
                    let thick = blob_thickness(length) + (INDICATOR.1 - BLOB_THICK) * merge.max(0.);
                    let pulled = ((length / BLOB - 1.) / 1.4).clamp(0., 1.);
                    let (low, high) = (low + shift, high + shift);
                    let (low, high) = (
                        low + (middle - BLOB / 2. - low) * pull - 7. * merge.max(0.),
                        high + (middle + BLOB / 2. - high) * pull + 7. * merge.max(0.),
                    );
                    let lead = BLOB_THICK / 2. * (1. + 0.1 * pulled);
                    let trail = BLOB_THICK / 2. * (1. - 0.4 * pulled);
                    let (left, right) = if forward { (trail, lead) } else { (lead, trail) };
                    capsule(window, origin + low, origin + high, line, thick, accent);
                    if pulled > 0.01 {
                        goo(
                            window,
                            (origin + low + left, line),
                            left,
                            (origin + high - right, line),
                            right,
                            9.,
                            accent,
                        );
                    }
                },
            )
            .absolute()
            .inset_0(),
        )
        .when(tick > 0.02, |row| {
            // All done: the one blob left wears a tick.
            row.child(
                div()
                    .absolute()
                    .left(px(BLOB / 2. + middle - 6.))
                    .top(px(INDICATOR.1 / 2. - 6.))
                    .opacity(motion::fade_for(tick))
                    .child(icon("check", 12., on_gold())),
            )
        })
}

// ---------------------------------------------------------------------------
// Little screens
// ---------------------------------------------------------------------------

/// A little screen is drawn in its own units, `k` px each, so the same
/// drawing fits the smallest window and fills a large one.
#[derive(Clone, Copy)]
pub struct Mini {
    pub k: f32,
    pub t: Theme,
    pub dark: bool,
}

impl Mini {
    pub fn u(&self, units: f32) -> Pixels {
        px(units * self.k)
    }

    fn at(&self, x: f32, y: f32, w: f32, h: f32) -> Div {
        div()
            .absolute()
            .left(self.u(x))
            .top(self.u(y))
            .w(self.u(w))
            .h(self.u(h))
    }

    /// A rounded stroke standing in for a line of text.
    fn line(&self, x: f32, y: f32, w: f32, h: f32, color: Hsla) -> Div {
        self.at(x, y, w.max(0.), h).rounded(self.u(h / 2.)).bg(color)
    }

    fn label(&self, x: f32, y: f32, w: f32, size: f32, color: Hsla) -> Div {
        div()
            .absolute()
            .left(self.u(x))
            .top(self.u(y))
            .w(self.u(w))
            .text_size(self.u(size))
            .line_height(self.u(size * 1.35))
            .text_color(color)
            .whitespace_nowrap()
            .overflow_hidden()
            .text_ellipsis()
    }

    /// Something at a fixed place that rises into it as it arrives. (The
    /// rise is applied inside the placed box: `motion::lift` moves an
    /// element relative to where layout put it, which a placed box is not.)
    fn risen(&self, x: f32, y: f32, w: f32, h: f32, pose: motion::Pose, content: Div) -> Div {
        self.at(x, y, w, h)
            .child(motion::lift(content.relative().size_full(), pose))
    }

    /// Ink at `strength` on the little screen's own surface, opaque.
    fn ink(&self, strength: f32) -> Hsla {
        motion::mix(self.t.card, self.t.text, strength)
    }
}

/// The size of each step's little screen, in its units.
pub fn screen_size(step: Step) -> (f32, f32) {
    match step {
        Step::Welcome | Step::Done => (300., 212.),
        Step::Server => (300., 176.),
        Step::Audio => (300., 196.),
        Step::Calendar => (244., 236.),
        Step::Pill => (300., 200.),
    }
}

/// The largest little screen, which the column must have room for.
pub const SCREEN_BOX: (f32, f32) = (300., 236.);

/// An event as the tiny calendar shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct TinyEvent {
    pub time: String,
    pub title: String,
    pub calendar: String,
    pub color: Hsla,
}

/// Everything a little screen reacts to.
pub struct Scene<'a> {
    pub mini: Mini,
    pub stage: &'a Stage,
    /// Seconds since this step arrived.
    pub since: f32,
    pub reduced: bool,
    pub link: Link,
    /// Seconds since `link` last changed.
    pub link_since: f32,
    pub instance: String,
    pub host: String,
    pub person: Option<String>,
    pub permissions: [Permission; 2],
    pub asking: [bool; 2],
    pub levels: [f32; 2],
    /// `None` until a calendar is connected, then what is coming up.
    pub events: Option<Vec<TinyEvent>>,
    pub calendar_name: String,
    /// Seconds since a calendar appeared.
    pub events_since: f32,
    /// Seconds since "Try it" was pressed.
    pub trying: Option<f32>,
}

pub fn scene(step: Step, s: &Scene) -> AnyElement {
    match step {
        Step::Welcome => welcome_scene(s, false),
        Step::Server => server_scene(s),
        Step::Audio => audio_scene(s),
        Step::Calendar => calendar_scene(s),
        Step::Pill => pill_scene(s),
        Step::Done => welcome_scene(s, true),
    }
}

/// The pill's bars for a level, `count` of them, in units: tall in the
/// middle and low at the ends, each with a little life of its own.
pub fn meter_heights(level: f32, count: usize, floor: f32, ceiling: f32, seconds: f32) -> Vec<f32> {
    let level = if level.is_finite() {
        level.clamp(0., 1.)
    } else {
        0.
    };
    let middle = (count as f32 - 1.) / 2.;
    (0..count)
        .map(|index| {
            if level < 0.015 {
                return floor;
            }
            let away = (index as f32 - middle) / (middle.max(1.) * 0.62);
            let shape = (-away * away).exp();
            let life = 0.72 + 0.28 * (seconds * (6.5 + index as f32 * 1.7) + index as f32 * 2.1).sin();
            (floor + (ceiling - floor) * (level.sqrt() * shape * life).clamp(0., 1.)).max(floor)
        })
        .collect()
}

/// The tiny Cereal window's chrome: three dots and a sidebar.
fn tiny_window(m: Mini, height: f32) -> Div {
    let t = m.t;
    let mut window = div()
        .relative()
        .size_full()
        .child(
            m.at(0., 0., 70., height)
                .rounded_tl(m.u(13.))
                .rounded_bl(m.u(13.))
                .bg(t.sidebar)
                .border_r_1()
                .border_color(t.border),
        );
    for index in 0..3 {
        window = window.child(
            m.at(10. + 8. * index as f32, 9., 5., 5.)
                .rounded_full()
                .bg(m.ink(0.2)),
        );
    }
    window = window.child(m.at(6., 27., 58., 13.).rounded(m.u(4.)).bg(t.hover));
    for (index, width) in [34., 42., 30., 38.].into_iter().enumerate() {
        window = window.child(m.line(11., 31. + 15. * index as f32, width, 5., m.ink(0.2)));
    }
    window
}

fn welcome_scene(s: &Scene, done: bool) -> AnyElement {
    let m = s.mini;
    let t = m.t;
    let (_, height) = screen_size(Step::Welcome);
    let mut window = tiny_window(m, height);
    // A note being written while the meeting is listened to.
    window = window
        .child(m.line(88., 30., 104., 8., m.ink(0.72)))
        .child(m.line(88., 46., 58., 5., m.ink(0.2)));
    let widths = [176., 150., 184., 126., 96.];
    let mut writing = false;
    for (index, width) in widths.into_iter().enumerate() {
        let written = if done || s.reduced {
            1.
        } else {
            SNAPPY.at(((s.since - 0.7 - 0.36 * index as f32) / 0.4).clamp(0., 1.))
        };
        let y = 66. + 14. * index as f32;
        let bullet = (1..4).contains(&index);
        let x = if bullet { 98. } else { 88. };
        if bullet && written > 0. {
            window = window.child(m.at(88., y, 5., 5.).rounded_full().bg(m.ink(0.3)));
        }
        window = window.child(m.line(x, y, (width - (x - 88.)) * written, 5., m.ink(0.2)));
        if written > 0. && written < 1. {
            writing = true;
            window = window.child(
                m.at(x + (width - (x - 88.)) * written + 2., y - 2., 1.5, 9.)
                    .rounded_full()
                    .bg(gold()),
            );
        }
    }
    // The recording control at the foot of the note, as in the app.
    let listening = !done && !s.reduced && (0.3..2.7).contains(&s.since);
    let level = if listening || writing {
        crate::onboarding::simulated_level(Channel::Microphone, s.since + 0.4).max(0.25)
    } else {
        0.
    };
    let heights: Vec<f32> = crate::recording_view::waveform_heights(level)
        .into_iter()
        .map(|height| height * 0.55 * m.k)
        .collect();
    window = window.child(
        m.at(137., 172., 96., 24.)
            .rounded_full()
            .bg(t.card)
            .border_1()
            .border_color(t.hairline)
            .shadow_sm()
            .flex()
            .items_center()
            .justify_center()
            .gap(m.u(7.))
            .child(motion::bars(
                "welcome-tiny-waveform",
                heights,
                2. * m.k,
                2. * m.k,
                12. * m.k,
                gold(),
            ))
            .child(div().w(m.u(24.)).h(m.u(5.)).rounded_full().bg(m.ink(0.24)))
            .child(div().size(m.u(7.)).rounded(m.u(2.)).bg(m.ink(0.5))),
    );
    if done {
        // Ready: a gold tick lands on the note.
        let pose = motion::mount_pose(GOOEY, ((s.since - 0.25) / 0.42).clamp(0., 1.), s.reduced);
        if pose.fade > 0. {
            let badge = div()
                .absolute()
                .left(m.u(254.))
                .top(m.u(20.))
                .size(m.u(28.))
                .rounded_full()
                .bg(gold())
                .flex()
                .items_center()
                .justify_center()
                .child(icon("check", 15. * m.k, on_gold()));
            window = window.child(motion::pop(badge, motion::Pop::BLOB, pose));
        }
    }
    window.into_any_element()
}

fn server_scene(s: &Scene) -> AnyElement {
    let m = s.mini;
    let t = m.t;
    let joined = s.link == Link::Joined;
    // How far the two arms of the link have reached toward each other, and
    // how far they have run together where they meet: they reach first,
    // then join on the spring the app's menus open on.
    let reach = s
        .stage
        .aim(("welcome-link", 0), if joined { 1. } else { 0. }, motion::SETTLE)
        .clamp(0., 1.);
    let met = s
        .stage
        .aim(("welcome-link-met", 0), if joined && reach > 0.78 { 1. } else { 0. }, GOOEY);
    let lamp = match s.link {
        Link::Joined => gold(),
        Link::Broken => t.danger,
        _ => m.ink(0.24),
    };
    let named = s.stage.presence(("welcome-instance", 0), joined && !s.instance.is_empty(), SNAPPY);
    let greeted = s.stage.presence(("welcome-person", 0), s.person.is_some(), SNAPPY);
    let mut frame = div().relative().size_full();
    // The server: a small rack with a lamp.
    let mut server = m
        .at(34., 36., 60., 76.)
        .rounded(m.u(10.))
        .bg(t.bg)
        .border_1()
        .border_color(t.border);
    for (index, color) in [lamp, m.ink(0.2), m.ink(0.2)].into_iter().enumerate() {
        let y = 14. + 18. * index as f32;
        server = server
            .child(m.line(10., y, 26., 5., m.ink(0.2)))
            .child(m.at(44., y, 5., 5.).rounded_full().bg(color));
    }
    frame = frame.child(server);
    // This Mac: a screen with the mark on it, on its stand.
    frame = frame
        .child(
            m.at(206., 46., 70., 50.)
                .rounded(m.u(7.))
                .bg(t.bg)
                .border_1()
                .border_color(t.border)
                .flex()
                .items_center()
                .justify_center()
                .child(icon(
                    "cereal",
                    20. * m.k,
                    motion::mix(m.ink(0.36), gold(), reach.clamp(0., 1.)),
                )),
        )
        .child(m.at(235., 96., 12., 7.).bg(m.ink(0.16)))
        .child(m.line(222., 102., 38., 4., m.ink(0.24)));
    // The link between them.
    let (k, link, link_since, reduced) = (m.k, s.link, s.link_since, s.reduced);
    let (gold, quiet, broken) = (
        gold(),
        m.ink(0.26),
        motion::mix(t.card, t.danger, 0.7),
    );
    frame = frame.child(
        canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                let x = |units: f32| f32::from(bounds.origin.x) + units * k;
                let line = f32::from(bounds.origin.y) + 71. * k;
                let (from, to) = (x(99.), x(201.));
                let middle = (from + to) / 2.;
                let radius = 3.2 * k;
                if reach > 0.004 {
                    // Each arm grows out of its own end, a round head on a
                    // thinner stem; where the heads meet they run together
                    // and the link fills out to one even bar.
                    let gap = (middle - from - radius) * (1. - reach);
                    let (left, right) = (middle - radius * 0.5 - gap, middle + radius * 0.5 + gap);
                    let stem = radius * (1.1 + 0.6 * met.clamp(0., 1.2));
                    let head = radius * (1. + 0.3 * (1. - met).clamp(0., 1.) * reach);
                    if met < 0.98 {
                        capsule(window, from - radius, left, line, stem, gold);
                        capsule(window, right, to + radius, line, stem, gold);
                        goo(window, (left, line), head, (right, line), head, 3.2, gold);
                    } else {
                        capsule(window, from - radius, to + radius, line, stem, gold);
                    }
                } else {
                    let color = if link == Link::Broken { broken } else { quiet };
                    dot(window, (from, line), radius, color);
                    dot(window, (to, line), radius, color);
                    if link == Link::Reaching && !reduced {
                        // Looking: a drop goes out from this Mac and back.
                        let swing = 0.5 - 0.5 * (link_since * 3.6).cos();
                        let drop = (to - (to - from) * swing, line);
                        let pulse = radius * (0.85 + 0.2 * (link_since * 7.2).sin());
                        dot(window, (to, line), radius, gold);
                        goo(window, (to, line), radius, drop, pulse, 2.6, gold);
                        goo(window, drop, pulse, (from, line), radius, 2.6, gold);
                    }
                }
            },
        )
        .absolute()
        .inset_0(),
    );
    // Who is at each end.
    if named.visible {
        frame = frame.child(m.risen(
            4.,
            120.,
            120.,
            30.,
            named,
            div()
                .flex()
                .flex_col()
                .items_center()
                .child(
                    div()
                        .max_w_full()
                        .truncate()
                        .text_size(m.u(10.5))
                        .line_height(m.u(15.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(t.text)
                        .child(s.instance.clone()),
                )
                .when(s.host != s.instance, |label| {
                    label.child(
                        div()
                            .max_w_full()
                            .truncate()
                            .text_size(m.u(8.5))
                            .line_height(m.u(12.))
                            .text_color(t.muted)
                            .child(s.host.clone()),
                    )
                }),
        ));
    } else {
        frame = frame.child(m.line(46., 126., 36., 5., m.ink(0.14)));
    }
    let person = s.person.clone().unwrap_or_default();
    frame = frame.child(
        div()
            .absolute()
            .left(m.u(181.))
            .top(m.u(120.))
            .w(m.u(120.))
            .flex()
            .flex_col()
            .items_center()
            .child(
                div()
                    .max_w_full()
                    .truncate()
                    .text_size(m.u(10.5))
                    .line_height(m.u(15.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(if greeted.visible { t.text } else { t.muted })
                    .child(if greeted.visible {
                        person
                    } else {
                        "This Mac".to_string()
                    }),
            )
            .when(greeted.visible, |label| {
                label.child(
                    div()
                        .text_size(m.u(8.5))
                        .line_height(m.u(12.))
                        .text_color(t.muted)
                        .opacity(greeted.fade)
                        .child("Signed in"),
                )
            }),
    );
    frame.into_any_element()
}

/// The strip across the top of a tiny desktop.
fn tiny_menu_bar(m: Mini, width: f32) -> Div {
    let t = m.t;
    let mut bar = m
        .at(0., 0., width, 18.)
        .rounded_tl(m.u(13.))
        .rounded_tr(m.u(13.))
        .bg(t.sidebar)
        .border_b_1()
        .border_color(t.border)
        .child(
            div()
                .absolute()
                .left(m.u(9.))
                .top(m.u(4.5))
                .child(icon("cereal", 9. * m.k, m.ink(0.5))),
        );
    for (x, w) in [(25., 22.), (52., 15.), (72., 19.), (width - 62., 12.), (width - 44., 12.), (width - 26., 17.)] {
        bar = bar.child(m.line(x, 7., w, 4., m.ink(0.2)));
    }
    bar
}

fn audio_scene(s: &Scene) -> AnyElement {
    let m = s.mini;
    let t = m.t;
    let (width, _) = screen_size(Step::Audio);
    let mut frame = div().relative().size_full().child(tiny_menu_bar(m, width));
    for (index, (channel, symbol, name)) in [
        (Channel::Microphone, "mic", "Microphone"),
        (Channel::SystemAudio, "headphones", "System audio"),
    ]
    .into_iter()
    .enumerate()
    {
        let state = s.permissions[index];
        let (status, tone) = match state {
            Permission::Allowed => ("Listening", t.accent),
            Permission::Denied => ("Denied", t.danger),
            Permission::NotAsked if s.asking[index] => ("Waiting for macOS", t.muted),
            Permission::NotAsked => ("Not yet", t.muted),
        };
        // The level meter: the pill's bars, more of them.
        let lit = s
            .stage
            .aim(("welcome-meter", index as u64), if state == Permission::Allowed { 1. } else { 0. }, SNAPPY)
            .clamp(0., 1.);
        let resting = if state == Permission::Denied {
            motion::mix(t.bg, t.danger, 0.5)
        } else {
            motion::mix(t.bg, t.text, 0.2)
        };
        let seconds = s.since + index as f32 * 3.1;
        let heights: Vec<f32> = meter_heights(s.levels[index], 13, 3., 34., seconds)
            .into_iter()
            .map(|height| height * m.k)
            .collect();
        let status_pose = s.stage.swap(("welcome-meter-status", index as u64), state as u64 * 2 + u64::from(s.asking[index]));
        frame = frame.child(
            m.at(16., 32. + 78. * index as f32, 268., 66.)
                .rounded(m.u(10.))
                .bg(t.bg)
                .border_1()
                .border_color(t.border)
                .child(
                    m.at(12., 19., 28., 28.)
                        .rounded(m.u(8.))
                        .bg(t.hover)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon(symbol, 14. * m.k, motion::mix(t.secondary, t.accent, lit))),
                )
                .child(m.label(50., 17., 110., 10.5, t.text).child(name))
                .child(
                    div()
                        .absolute()
                        .left(m.u(50.))
                        .top(m.u(34.))
                        .child(motion::swapped(
                            div()
                                .text_size(m.u(8.5))
                                .line_height(m.u(12.))
                                .text_color(tone)
                                .whitespace_nowrap()
                                .child(status),
                            status_pose,
                        )),
                )
                .child(
                    div().absolute().left(m.u(181.)).top(m.u(16.)).child(motion::bars(
                        ("welcome-meter-bars", channel.index()),
                        heights,
                        3. * m.k,
                        3. * m.k,
                        34. * m.k,
                        motion::mix(resting, gold(), lit),
                    )),
                ),
        );
    }
    frame.into_any_element()
}

fn calendar_scene(s: &Scene) -> AnyElement {
    let m = s.mini;
    let t = m.t;
    let mut frame = div()
        .relative()
        .size_full()
        .child(
            m.label(16., 13., 150., 14., t.text)
                .font_family(DISPLAY)
                .child("Coming up"),
        )
        .child(
            div()
                .absolute()
                .left(m.u(214.))
                .top(m.u(16.))
                .child(icon("calendar-dots", 13. * m.k, if s.events.is_some() { t.accent } else { t.muted })),
        );
    let events = s.events.clone().unwrap_or_default();
    let connected = s.events.is_some();
    for index in 0..4 {
        let y = 44. + 46. * index as f32;
        let arrival = if connected {
            motion::mount_pose(
                GOOEY,
                ((s.events_since - 0.1 * index as f32) / 0.42).clamp(0., 1.),
                s.reduced,
            )
        } else {
            motion::Pose::REST
        };
        let event = events.get(index);
        let mut row = div().rounded(m.u(8.));
        let placed = |row: Div| m.risen(12., y, 220., 40., arrival, row);
        match event {
            Some(event) => {
                row = row
                    .bg(t.bg)
                    .child(m.label(9., 13., 50., 8.5, t.muted).child(event.time.clone()))
                    .child(
                        m.at(62., 20. - 13. * arrival.value.clamp(0., 1.2), 3., 26. * arrival.value.clamp(0., 1.2))
                            .rounded_full()
                            .bg(event.color),
                    )
                    .child(m.label(73., 6., 138., 10.5, t.text).child(event.title.clone()))
                    .child(m.label(73., 21., 138., 8.5, t.muted).child(event.calendar.clone()));
                frame = frame.child(placed(row));
            }
            None if connected && index == 0 && events.is_empty() => {
                row = row
                    .bg(t.bg)
                    .child(m.at(12., 7., 3., 26.).rounded_full().bg(gold()))
                    .child(m.label(23., 6., 188., 10.5, t.text).child("Nothing coming up yet"))
                    .child(m.label(23., 21., 188., 8.5, t.muted).child(s.calendar_name.clone()));
                frame = frame.child(placed(row));
            }
            None if connected => {}
            None => {
                // Not connected: where the meetings will be.
                let tone = m.ink(0.13);
                row = row
                    .child(m.line(9., 17., 30., 5., tone))
                    .child(m.at(62., 7., 3., 26.).rounded_full().bg(tone))
                    .child(m.line(73., 11., 116. - 16. * index as f32, 6., tone))
                    .child(m.line(73., 24., 58., 4., tone));
                frame = frame.child(placed(row));
            }
        }
    }
    frame.into_any_element()
}

/// How long "Try it" plays for, and when the popup folds away again.
pub const TRY_LENGTH: f32 = 4.4;
const TRY_OPEN: (f32, f32) = (0.22, 3.5);

fn pill_scene(s: &Scene) -> AnyElement {
    let m = s.mini;
    let t = m.t;
    let (width, _) = screen_size(Step::Pill);
    let mut frame = div().relative().size_full().child(tiny_menu_bar(m, width));
    // A call going on in some other app.
    let mut call = m
        .at(20., 36., 180., 126.)
        .rounded(m.u(9.))
        .bg(t.bg)
        .border_1()
        .border_color(t.border);
    for index in 0..3 {
        call = call.child(m.at(8. + 7. * index as f32, 7., 4., 4.).rounded_full().bg(m.ink(0.2)));
    }
    for (index, (x, y)) in [(8., 18.), (92., 18.), (8., 70.), (92., 70.)].into_iter().enumerate() {
        call = call.child(
            m.at(x, y, 80., 48.)
                .rounded(m.u(6.))
                .bg(t.hover)
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .size(m.u(16.))
                        .rounded_full()
                        .bg(m.ink(if index == 1 { 0.3 } else { 0.18 })),
                ),
        );
    }
    frame = frame.child(call);
    let trying = s.trying.filter(|at| *at < TRY_LENGTH);
    let open = trying.is_some_and(|at| (TRY_OPEN.0..TRY_OPEN.1).contains(&at));
    // The popup grows out of the side of the pill, exactly as the real one.
    let bubble = s.stage.presence(("welcome-bubble", 0), open, GOOEY);
    if bubble.visible {
        let at = trying.unwrap_or(TRY_LENGTH);
        let row = |after: f32| motion::mount_pose(SNAPPY, ((at - after) / 0.28).clamp(0., 1.), s.reduced);
        let said = |x: f32, y: f32, w: f32, h: f32, own: bool, pose: motion::Pose| {
            m.risen(
                x,
                y,
                w,
                h,
                pose,
                div()
                    .rounded(m.u(5.))
                    .bg(if own { rgb(0x363635) } else { rgb(0x4e4d4b) })
                    .child(m.line(6., 6., w - 12. - 10., 3.5, rgba(0xfcfcf8b0).into()))
                    .when(h > 18., |said| {
                        said.child(m.line(6., 13.5, (w - 12.) * 0.6, 3.5, rgba(0xfcfcf8b0).into()))
                    }),
            )
        };
        let panel = m
            .at(128., 46., 118., 104.)
            .rounded(m.u(9.))
            .border_1()
            .border_color(rgba(0x72726ef2))
            .bg(rgba(0x262626f2))
            .shadow(vec![BoxShadow {
                color: rgba(0x00000066).into(),
                offset: point(px(0.), m.u(2.)),
                blur_radius: m.u(5.),
                spread_radius: px(0.),
            }])
            .overflow_hidden()
            .when(at > 0.6, |panel| panel.child(said(8., 10., 66., 16., false, row(0.6))))
            .when(at > 1.15, |panel| panel.child(said(48., 31., 62., 16., true, row(1.15))))
            .when(at > 1.8, |panel| panel.child(said(8., 52., 84., 24., false, row(1.8))))
            .when(at > 2.5, |panel| panel.child(said(8., 81., 40., 14., false, row(2.5)).opacity(0.5)));
        frame = frame.child(motion::pop(
            panel,
            motion::Pop {
                anchor: (1., 0.67),
                scale: (0.9, 0.86),
                travel: 6.,
                rise: 0.,
            },
            bubble,
        ));
    }
    // The pill itself: it slides in from the edge of the screen, squashes
    // when pressed, and its bars follow the voice.
    let arrived = motion::mount_pose(GOOEY, ((s.since - 0.3) / 0.42).clamp(0., 1.), s.reduced);
    let held = s
        .stage
        .aim(("welcome-pill-press", 0), if trying.is_some_and(|at| at < 0.14) { 1. } else { 0. }, SNAPPY);
    let level = match trying {
        Some(at) if at < TRY_OPEN.1 + 0.3 => {
            crate::onboarding::simulated_level(Channel::Microphone, at + 1.3).max(0.12)
        }
        _ => 0.,
    };
    let heights: Vec<f32> = crate::pill::voice_bar_targets(level)
        .into_iter()
        .map(|height| height * 0.6 * m.k)
        .collect();
    frame = frame.child(
        div()
            .absolute()
            .left(m.u(256. + 0.4 * held + 44. * (1. - arrived.value)))
            .top(m.u(64. + 0.8 * held))
            .w(m.u(21. - 0.8 * held))
            .h(m.u(38. - 1.6 * held))
            .opacity(arrived.fade)
            .rounded_full()
            .border_1()
            .border_color(rgba(0x72726ef2))
            .bg(rgba(0x262626f2))
            .shadow(vec![BoxShadow {
                color: rgba(0x00000080).into(),
                offset: point(px(0.), m.u(1.5)),
                blur_radius: m.u(3.),
                spread_radius: px(0.),
            }])
            .flex()
            .flex_col()
            .items_center()
            .pt(m.u(6.))
            .gap(m.u(4.))
            .child(icon("cereal", 10. * m.k, rgba(0xffffffcc).into()))
            .child(motion::bars(
                "welcome-tiny-pill",
                heights,
                1.3 * m.k,
                1.8 * m.k,
                12. * m.k,
                rgb(0xce9d00).into(),
            )),
    );
    frame.into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    // `gpui::*` re-exports its own `test` attribute; these are plain unit tests.
    use std::prelude::v1::test;

    #[test]
    fn a_neck_joins_circles_that_are_close_and_snaps_when_they_part() {
        // Touching: a neck as wide as the circles allow.
        let near = neck((0., 0.), 4., (8., 0.), 4., 3.).unwrap();
        // Symmetric about the line between the centres.
        assert!((near[0].1 + near[7].1).abs() < 0.001 && (near[3].1 + near[4].1).abs() < 0.001);
        assert!((near[0].0 - near[7].0).abs() < 0.001);
        let width = |points: &[Spot; 8]| (points[0].1 - points[7].1).abs();
        // It thins steadily as the circles are pulled apart…
        let mut previous = width(&near);
        for distance in [10., 14., 18., 22.] {
            let points = neck((0., 0.), 4., (distance, 0.), 4., 3.).unwrap();
            assert!(width(&points) < previous, "{distance}");
            previous = width(&points);
        }
        // …and is gone at three times their touching distance, and before
        // that never leaves the circles it joins.
        assert!(neck((0., 0.), 4., (24., 0.), 4., 3.).is_none());
        assert!(previous < 2.);
        for point in [near[0], near[7]] {
            assert!(((point.0.powi(2) + point.1.powi(2)).sqrt() - 4.).abs() < 0.001);
        }
        // One circle inside the other, or no circle at all: nothing to join.
        assert!(neck((0., 0.), 6., (1., 0.), 2., 3.).is_none());
        assert!(neck((0., 0.), 0., (5., 0.), 2., 3.).is_none());
    }

    #[test]
    fn the_active_dot_stretches_toward_the_next_and_lands_on_it() {
        let start = Instant::now();
        let mut travel = Travel::at(1);
        assert_eq!(travel.edges(start, false), (PITCH - BLOB / 2., PITCH + BLOB / 2.));
        assert!(!travel.moving(start));
        travel.go(2, start);
        let length = GOOEY.duration().mul_f32(1.25);
        let mut longest: f32 = 0.;
        for tick in 0..=100 {
            let now = start + length.mul_f32(tick as f32 / 100.);
            let (low, high) = travel.edges(now, false);
            assert!(high > low);
            // It never leaves the row between the two dots by more than a nudge.
            assert!(low >= PITCH - BLOB / 2. - 0.01 && high <= 2. * PITCH + BLOB / 2. + 3.);
            longest = longest.max(high - low);
        }
        assert!(longest > BLOB * 1.5, "{longest}");
        assert_eq!(travel.edges(start + length, false), (2. * PITCH - BLOB / 2., 2. * PITCH + BLOB / 2.));
        assert!(!travel.moving(start + length));
        // Reduced motion: it is simply on the new dot.
        assert_eq!(travel.edges(start, true), (2. * PITCH - BLOB / 2., 2. * PITCH + BLOB / 2.));
        // Turned back half way, it sets off from where it had got to.
        let half = start + length.mul_f32(0.3);
        let (low, high) = travel.edges(half, false);
        travel.go(1, half);
        let (again_low, again_high) = travel.edges(half, false);
        assert!(((low + high) - (again_low + again_high)).abs() < 0.01);
    }

    #[test]
    fn the_blob_keeps_its_volume_as_it_is_pulled_and_squashed() {
        assert_eq!(blob_thickness(BLOB), BLOB_THICK);
        assert!(blob_thickness(BLOB * 1.7) < BLOB_THICK * 0.8);
        assert!(blob_thickness(BLOB * 2.4) >= BLOB_THICK * 0.4);
        assert!(
            blob_thickness(BLOB * 0.8) > BLOB_THICK
                && blob_thickness(BLOB * 0.2) <= BLOB_THICK * 1.3
        );
        // The resting blob and the last step's pill both fit the row.
        assert!(BLOB_THICK <= INDICATOR.1 && DOT < BLOB_THICK);
        assert_eq!(INDICATOR.0, 5. * PITCH + BLOB);
        let mut previous = f32::MAX;
        for tick in 4..=48 {
            let thickness = blob_thickness(BLOB * tick as f32 / 20.);
            assert!(thickness <= previous + 0.001);
            previous = thickness;
        }
    }

    #[test]
    fn the_mark_wakes_once_then_blinks_hops_and_wobbles_back_to_rest() {
        let start = Instant::now();
        let mut character = Character::asleep(start);
        let (asleep, moving, _) = character.pose(start, false);
        assert_eq!((asleep.awake, asleep.eye, asleep.crumbs), (0., 0., 0.));
        assert!(!moving);
        character.wake(start);
        let (waking, moving, _) = character.pose(start + WAKE / 2, false);
        assert!(moving && waking.awake > 0.9 && waking.eye > 0.);
        let rest = start + WAKE + Duration::from_millis(1);
        let (awake, moving, next) = character.pose(rest, false);
        assert_eq!(awake, MarkPose::REST);
        assert!(!moving);
        // The next blink is known, so nothing has to poll for it.
        let blink = next.unwrap();
        assert!(blink > rest);
        let (blinking, moving, _) = character.pose(blink + BLINK / 2, false);
        assert!(moving && blinking.eye < 0.1);
        let (after, _, next) = character.pose(blink + BLINK, false);
        assert_eq!(after.eye, 1.);
        assert!(next.unwrap() > blink + BLINK);
        // A hop leaves the ground and comes back to it exactly.
        character.hop(rest);
        let (up, moving, _) = character.pose(rest + HOP.mul_f32(0.4), false);
        assert!(moving && up.lift > 0.2);
        let (down, moving, _) = character.pose(rest + HOP, false);
        assert_eq!((down.lift, down.stretch), (0., 0.));
        assert!(!moving || down.eye < 1.);
        // A wobble shakes and stops.
        character.wobble(rest);
        let (shaking, _, _) = character.pose(rest + WOBBLE.mul_f32(0.09), false);
        assert!(shaking.tilt.abs() > 0.05);
        assert_eq!(character.pose(rest + WOBBLE, false).0.tilt, 0.);
        // Reduced motion: awake or not, and nothing else.
        let mut still = Character::asleep(start);
        still.wake(start);
        still.hop(start);
        let (pose, _, _) = still.pose(start + WAKE / 2, true);
        assert_eq!((pose.lift, pose.stretch, pose.tilt), (0., 0., 0.));
        assert_eq!(Character::awake(rest).pose(rest, false).0, MarkPose::REST);
    }

    #[test]
    fn character_curves_start_and_end_at_rest() {
        assert_eq!(hop_pose(0.), (0., 0.));
        let (lift, stretch) = hop_pose(1.);
        assert!(lift == 0. && stretch.abs() < 0.001);
        assert_eq!(wobble_tilt(0.), 0.);
        assert!(wobble_tilt(1.).abs() < 0.001);
        assert_eq!(wake_pose(1.), MarkPose::REST);
        let asleep = wake_pose(0.);
        assert_eq!((asleep.awake, asleep.eye, asleep.crumbs), (0., 0., 0.));
        // The hop is highest in the air and never goes below the ground.
        let peak = (0..=100).map(|tick| hop_pose(tick as f32 / 100.).0).fold(0., f32::max);
        assert!((0.28..=0.31).contains(&peak));
        assert!((0..=100).all(|tick| hop_pose(tick as f32 / 100.).0 >= 0.));
    }

    #[test]
    fn a_meter_is_tallest_in_the_middle_and_flat_when_quiet() {
        let quiet = meter_heights(0., 13, 3., 34., 1.);
        assert!(quiet.iter().all(|height| *height == 3.));
        let loud = meter_heights(0.8, 13, 3., 34., 1.);
        assert_eq!(loud.len(), 13);
        assert!(loud.iter().all(|height| (3. ..=34.).contains(height)));
        assert!(loud[6] > loud[0] * 2. && loud[6] > loud[12] * 2.);
        // It moves with time, and nonsense is treated as silence.
        assert_ne!(loud, meter_heights(0.8, 13, 3., 34., 1.2));
        assert_eq!(meter_heights(f32::NAN, 5, 3., 34., 0.), vec![3.; 5]);
    }

    #[test]
    fn every_little_screen_fits_the_box_the_column_keeps_for_it() {
        for step in ALL_STEPS {
            let (width, height) = screen_size(step);
            assert!(width <= SCREEN_BOX.0 && height <= SCREEN_BOX.1, "{step:?}");
        }
    }
}
