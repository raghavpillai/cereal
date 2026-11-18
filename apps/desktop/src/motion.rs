//! Cereal's motion system: soft matter rather than rigid rectangles.
//!
//! Everything here is built from three ideas:
//!
//! * **Springs** – analytic under-damped springs evaluated over a fixed
//!   duration, ending exactly on their target ([`Spring::at`]).
//! * **Shape morphs** – a surface grows out of its anchor ([`pop`], [`Morph`])
//!   by animating its shell (size, corner radius, shadow) at paint time, while
//!   its content keeps its natural layout. GPUI cannot scale arbitrary
//!   elements, so nothing here ever scales text.
//! * **Stretch and catch-up** – a moving highlight leads with one edge and
//!   lets the other catch up ([`stretch`], [`Glide`]), like a droplet.
//!
//! All tokens live in this file. macOS "Reduce motion" is read in one place
//! ([`reduce_motion`]) and turns every helper into a short opacity-only fade.
use crate::ui::{FONT, Theme, icon};
use gpui::{prelude::*, *};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    hash::{Hash, Hasher},
    rc::Rc,
    time::{Duration, Instant},
};

// ---------------------------------------------------------------------------
// Tokens
// ---------------------------------------------------------------------------

/// A damped spring that settles within a fixed duration.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spring {
    pub millis: u64,
    /// Damping ratio. 1.0 is critically damped; lower values overshoot more.
    pub damping: f32,
}

/// Small controls, hovers and presses: a barely perceptible overshoot.
pub const SNAPPY: Spring = Spring {
    millis: 280,
    damping: 0.82,
};
/// Morphs, menus, capsule state changes and moving highlights: ~5% overshoot.
pub const GOOEY: Spring = Spring {
    millis: 420,
    damping: 0.68,
};
/// The Ask bar swelling into its tray: quicker than [`GOOEY`] and looser, so
/// it overshoots by about a tenth and settles in one soft bounce.
pub const ASK_TRAY: Spring = Spring {
    millis: 340,
    damping: 0.6,
};
/// Large panels, the sidebar and page-level transitions.
pub const SETTLE: Spring = Spring {
    millis: 550,
    damping: 0.80,
};
/// Large reading surfaces (transcript, chat): the same pace as [`GOOEY`] with
/// a quarter of its overshoot, so a 400 px panel settles within a few pixels.
pub const PANEL: Spring = Spring {
    millis: 420,
    damping: 0.80,
};
/// A page arriving: short, no overshoot, and never from fully transparent, so
/// navigation does not dip to the bare background.
pub const PAGE_FADE: Duration = Duration::from_millis(220);
/// Where a page's opacity starts.
pub const PAGE_FROM: f32 = 0.6;
/// Exits take this share of the matching entrance and never bounce.
pub const EXIT_RATIO: f32 = 0.65;
/// The whole transition when macOS "Reduce motion" is on: opacity only.
pub const REDUCED_FADE: Duration = Duration::from_millis(120);
/// Delay between consecutive items of a staggered entrance.
pub const STAGGER_STEP: Duration = Duration::from_millis(22);
/// Items past this index enter together, so long lists never take long.
pub const STAGGER_CAP: usize = 7;
/// How far a staggered item or page travels while it fades in.
pub const RISE_TRAVEL: f32 = 7.;
/// How far a whole page travels while it settles in.
pub const PAGE_TRAVEL: f32 = 4.;
/// Hover washes ease in quickly and out a little slower.
const HOVER_IN: Duration = Duration::from_millis(110);
const HOVER_OUT: Duration = Duration::from_millis(220);
/// A press compresses fast; the release rides [`SNAPPY`] back.
const PRESS_IN: Duration = Duration::from_millis(90);
/// Squash on press, as a share of the control's short side (≈0.96 scale).
const SQUASH: f32 = 0.04;
/// The leading edge of a moving highlight arrives in this share of the time.
const LEAD_SHARE: f32 = 0.6;
/// The trailing edge stays behind for this share before it lets go.
const TRAIL_DELAY: f32 = 0.16;
/// A highlight never stretches past this multiple of its resting length…
const MAX_STRETCH: f32 = 2.4;
/// …and never squashes below this share while it settles.
const MIN_SQUASH: f32 = 0.72;
/// The spring envelope decays to e^-6.9 ≈ 0.1% by the end of its duration.
const SETTLE_DECAY: f32 = 6.9;
/// A frame built now reaches the screen about one display refresh later.
/// Stage tracks, highlights and page entrances are evaluated that far ahead,
/// the way Core Animation evaluates at the frame's display time, so the very
/// first frame after an input already moves instead of repeating the
/// resting pose. Transitions still start where they were and end on target.
pub const FRAME_LEAD: Duration = Duration::from_millis(16);

/// When the frame being built will be on screen: [`FRAME_LEAD`] from `now`.
pub fn shown_at(now: Instant) -> Instant {
    now + FRAME_LEAD
}

/// A GPUI animation of `duration` that starts [`FRAME_LEAD`] in, so its
/// first frame already moves. Returns the animation and the share of the
/// original duration that is skipped; map GPUI's delta with [`led`].
fn led_animation(duration: Duration) -> (Animation, f32) {
    let lead = FRAME_LEAD.min(duration / 2);
    let skipped = lead.as_secs_f32() / duration.as_secs_f32().max(0.001);
    (Animation::new(duration - lead), skipped)
}

/// GPUI's 0..=1 delta for an animation made with [`led_animation`], mapped
/// back onto the full transition.
fn led(skipped: f32, delta: f32) -> f32 {
    skipped + (1. - skipped) * delta
}

/// `CEREAL_SLOW_MOTION=8` stretches every transition eight-fold so it can be
/// inspected frame by frame, like the iOS simulator's slow animations.
fn time_scale() -> f32 {
    thread_local! {
        static SCALE: f32 = std::env::var("CEREAL_SLOW_MOTION")
            .ok()
            .and_then(|value| value.trim().parse::<f32>().ok())
            .filter(|scale| scale.is_finite())
            .map_or(1., |scale| scale.clamp(1., 60.));
    }
    SCALE.with(|scale| *scale)
}

/// Seconds of motion in `elapsed` of the clock: the slow-motion review
/// factor taken back out, for motion that is timed by hand (a pose computed
/// from when it began) rather than by the length of an animation.
pub fn paced(elapsed: Duration) -> f32 {
    elapsed.as_secs_f32() / time_scale()
}

/// A motion duration with the slow-motion review factor applied.
pub fn slow(duration: Duration) -> Duration {
    let scale = time_scale();
    if scale == 1. {
        duration
    } else {
        duration.mul_f32(scale)
    }
}

impl Spring {
    pub fn duration(self) -> Duration {
        slow(Duration::from_millis(self.millis))
    }

    pub fn exit_duration(self) -> Duration {
        slow(Duration::from_millis(
            (self.millis as f32 * EXIT_RATIO) as u64,
        ))
    }

    fn raw(self, t: f32) -> f32 {
        let zeta = self.damping.clamp(0.05, 0.999);
        let omega = SETTLE_DECAY / zeta;
        let damped = omega * (1. - zeta * zeta).sqrt();
        1. - (-zeta * omega * t).exp()
            * ((damped * t).cos() + (zeta * omega / damped) * (damped * t).sin())
    }

    /// Position of a unit step at normalised time `t`. Starts at exactly 0,
    /// overshoots once by [`Spring::overshoot`] and ends at exactly 1.
    ///
    /// The result leaves 0..=1, so it must be applied inside an animator or a
    /// paint callback: GPUI easing functions themselves must stay in range.
    pub fn at(self, t: f32) -> f32 {
        if !t.is_finite() || t <= 0. {
            return 0.;
        }
        if t >= 1. {
            return 1.;
        }
        self.raw(t) / self.raw(1.)
    }

    /// Peak overshoot of the ideal spring, as a share of the distance moved.
    #[cfg(test)]
    pub fn overshoot(self) -> f32 {
        let zeta = self.damping.clamp(0.05, 0.999);
        (-zeta * std::f32::consts::PI / (1. - zeta * zeta).sqrt()).exp()
    }
}

/// Monotonic, critically damped approach used for every exit: fast away,
/// soft landing, no bounce.
pub fn exit_curve(t: f32) -> f32 {
    if !t.is_finite() || t <= 0. {
        return 0.;
    }
    if t >= 1. {
        return 1.;
    }
    let raw = |t: f32| 1. - (-7. * t).exp() * (1. + 7. * t);
    raw(t) / raw(1.)
}

/// CSS `cubic-bezier(.4, 0, .2, 1)`, the reference app's ease-in-out.
pub fn standard_curve(t: f32) -> f32 {
    cubic_bezier(t, 0.4, 0., 0.2, 1.)
}

/// Monotonic ease-out used for opacity-only transitions.
pub fn fade_curve(t: f32) -> f32 {
    if !t.is_finite() || t <= 0. {
        return 0.;
    }
    if t >= 1. {
        return 1.;
    }
    1. - (1. - t) * (1. - t)
}

/// Opacity for a surface at morph level `level`. A function of the level
/// alone, so reversing a transition mid-flight can never flash.
pub fn fade_for(level: f32) -> f32 {
    let x = (level / 0.55).clamp(0., 1.);
    x * x * (3. - 2. * x)
}

/// Delay before item `index` of a staggered list starts to enter.
pub fn stagger_delay(index: usize) -> Duration {
    slow(STAGGER_STEP * index.min(STAGGER_CAP) as u32)
}

fn lerp(from: f32, to: f32, t: f32) -> f32 {
    // Exact at rest, so a settled surface compares equal to its target.
    if t == 1. {
        return to;
    }
    from + (to - from) * t
}

/// Stretch-and-catch-up along one axis. `from` and `to` are `(low, high)`
/// edges; the edge facing the direction of travel runs ahead on a fast spring
/// while the other waits, then whips after it.
pub fn stretch(from: (f32, f32), to: (f32, f32), t: f32) -> (f32, f32) {
    if !t.is_finite() || t <= 0. {
        return from;
    }
    if t >= 1. {
        return to;
    }
    let lead = SNAPPY.at((t / LEAD_SHARE).min(1.));
    let trail = GOOEY.at(((t - TRAIL_DELAY) / (1. - TRAIL_DELAY)).max(0.));
    let travel = (to.0 + to.1) - (from.0 + from.1);
    if travel.abs() < 1. {
        // Nothing to chase on this axis: resize in place.
        let both = GOOEY.at(t);
        return (lerp(from.0, to.0, both), lerp(from.1, to.1, both));
    }
    let forward = travel > 0.;
    let (mut low, mut high) = if forward {
        (lerp(from.0, to.0, trail), lerp(from.1, to.1, lead))
    } else {
        (lerp(from.0, to.0, lead), lerp(from.1, to.1, trail))
    };
    let rest = lerp(from.1 - from.0, to.1 - to.0, t).max(0.);
    let length = (high - low).clamp(rest * MIN_SQUASH, rest * MAX_STRETCH);
    // The leading edge owns the position; the trailing edge absorbs limits.
    if forward {
        low = high - length;
    } else {
        high = low + length;
    }
    (low, high)
}

// ---------------------------------------------------------------------------
// Reduce motion
// ---------------------------------------------------------------------------

/// Whether transitions must collapse to short opacity-only fades.
///
/// Reads the macOS "Reduce motion" accessibility setting, refreshed at most
/// once a second so toggling it applies without a restart.
/// `CEREAL_REDUCE_MOTION=1` (or `0`) overrides the system for verification.
pub fn reduce_motion() -> bool {
    thread_local! {
        static CACHE: Cell<Option<(Instant, bool)>> = const { Cell::new(None) };
        static FORCED: Option<bool> = std::env::var("CEREAL_REDUCE_MOTION")
            .ok()
            .map(|value| !matches!(value.trim(), "" | "0" | "false" | "off"));
    }
    if let Some(forced) = FORCED.with(|forced| *forced) {
        return forced;
    }
    CACHE.with(|cache| {
        let now = Instant::now();
        if let Some((read, value)) = cache.get()
            && now.duration_since(read) < Duration::from_secs(1)
        {
            return value;
        }
        let value = system_reduce_motion();
        cache.set(Some((now, value)));
        value
    })
}

#[cfg(target_os = "macos")]
#[allow(unexpected_cfgs)]
fn system_reduce_motion() -> bool {
    use objc::{
        class, msg_send,
        runtime::{BOOL, NO, Object},
        sel, sel_impl,
    };
    unsafe {
        let workspace: *mut Object = msg_send![class!(NSWorkspace), sharedWorkspace];
        if workspace.is_null() {
            return false;
        }
        let reduce: BOOL = msg_send![workspace, accessibilityDisplayShouldReduceMotion];
        reduce != NO
    }
}

#[cfg(not(target_os = "macos"))]
fn system_reduce_motion() -> bool {
    false
}

// ---------------------------------------------------------------------------
// Tracks: retargetable values owned by a view
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
enum Curve {
    Spring(Spring),
    Exit,
    Fade,
    /// CSS `cubic-bezier(.4, 0, .2, 1)`, the reference app's ease-in-out.
    Standard,
}

/// One animated scalar. Retargeting starts from wherever the value currently
/// is, so an interrupted transition never jumps.
#[derive(Clone, Copy, Debug)]
pub struct Track {
    from: f32,
    to: f32,
    start: Instant,
    duration: Duration,
    curve: Curve,
}

impl Track {
    pub fn rest(value: f32, now: Instant) -> Self {
        Self {
            from: value,
            to: value,
            start: now,
            duration: Duration::ZERO,
            curve: Curve::Fade,
        }
    }

    pub fn target(&self) -> f32 {
        self.to
    }

    fn progress(&self, now: Instant) -> f32 {
        if self.duration.is_zero() {
            return 1.;
        }
        (now.saturating_duration_since(self.start).as_secs_f32() / self.duration.as_secs_f32())
            .min(1.)
    }

    pub fn settled(&self, now: Instant) -> bool {
        self.progress(now) >= 1.
    }

    pub fn value(&self, now: Instant) -> f32 {
        let t = self.progress(now);
        if t >= 1. {
            return self.to;
        }
        let eased = match self.curve {
            Curve::Spring(spring) => spring.at(t),
            Curve::Exit => exit_curve(t),
            Curve::Fade => fade_curve(t),
            Curve::Standard => standard_curve(t),
        };
        lerp(self.from, self.to, eased)
    }

    fn retarget(&mut self, to: f32, now: Instant, duration: Duration, curve: Curve) {
        self.from = self.value(now);
        self.to = to;
        self.start = now;
        self.duration = duration;
        self.curve = curve;
    }

    /// Spring towards `to` with overshoot; snaps when motion is reduced.
    pub fn spring_to(&mut self, to: f32, spring: Spring, now: Instant, reduced: bool) {
        if (self.to - to).abs() <= f32::EPSILON {
            return;
        }
        if reduced {
            *self = Self::rest(to, now);
        } else {
            self.retarget(to, now, spring.duration(), Curve::Spring(spring));
        }
    }

    /// Enter on `spring`, leave on the shorter monotonic exit curve. With
    /// reduced motion both directions become a short linear-ish fade.
    pub fn present(&mut self, open: bool, spring: Spring, now: Instant, reduced: bool) {
        let to = if open { 1. } else { 0. };
        if (self.to - to).abs() <= f32::EPSILON {
            return;
        }
        if reduced {
            self.retarget(to, now, slow(REDUCED_FADE), Curve::Fade);
        } else if open {
            self.retarget(to, now, spring.duration(), Curve::Spring(spring));
        } else {
            self.retarget(to, now, spring.exit_duration(), Curve::Exit);
        }
    }

    /// The pose of a surface whose presence this track drives.
    pub fn pose(&self, now: Instant, reduced: bool) -> Pose {
        let level = self.value(now);
        let settled = self.settled(now);
        Pose {
            value: if reduced { 1. } else { level },
            fade: if reduced {
                level.clamp(0., 1.)
            } else {
                fade_for(level)
            },
            visible: self.to > 0.5 || !settled,
            settled,
        }
    }
}

/// Where a transitioning surface is right now.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    /// Morph level: 0 collapsed, 1 at rest, briefly above 1 on overshoot.
    /// Always 1 when motion is reduced, so nothing moves or resizes.
    pub value: f32,
    /// Opacity in 0..=1.
    pub fade: f32,
    /// False once a closed surface has finished leaving and can unmount.
    pub visible: bool,
    pub settled: bool,
}

impl Pose {
    pub const REST: Pose = Pose {
        value: 1.,
        fade: 1.,
        visible: true,
        settled: true,
    };

    /// The same pose kept fully opaque: for a shape that morphs out of one
    /// that is already on screen, where a fade would read as a flicker.
    pub fn solid(self) -> Self {
        Self { fade: 1., ..self }
    }

    pub fn at_rest(&self) -> bool {
        self.settled && self.value == 1. && self.fade >= 1.
    }
}

pub type TrackKey = (&'static str, u64);

/// A view's animated values. Render helpers borrow it immutably, ask for the
/// current value of a track, and the view calls [`Stage::flush`] once per
/// render so frames are only requested while something is still moving.
#[derive(Default)]
pub struct Stage {
    tracks: RefCell<HashMap<TrackKey, Track>>,
    glides: RefCell<HashMap<&'static str, Glide>>,
    live: Cell<bool>,
    /// Set by the first flush: whatever is open on the very first frame is
    /// simply there, rather than animating in at launch.
    primed: Cell<bool>,
    marks: RefCell<HashMap<&'static str, Instant>>,
    counters: RefCell<HashMap<&'static str, usize>>,
    swaps: RefCell<HashMap<TrackKey, (u64, Option<Instant>)>>,
    frame_started: Cell<Option<Instant>>,
    /// The window frame in which each watched thing was last on screen.
    seen: RefCell<HashMap<TrackKey, u64>>,
}

impl Stage {
    /// Presence of a surface that mounts and unmounts. A missing track is a
    /// closed surface, so the first `open` animates in.
    pub fn presence(&self, key: TrackKey, open: bool, spring: Spring) -> Pose {
        let (now, reduced) = (Instant::now(), reduce_motion());
        let mut tracks = self.tracks.borrow_mut();
        let initial = if open && !self.primed.get() { 1. } else { 0. };
        let track = tracks
            .entry(key)
            .or_insert_with(|| Track::rest(initial, now));
        let before = track.to;
        track.present(open, spring, now, reduced);
        stats::traced(key, before, track);
        let pose = track.pose(shown_at(now), reduced);
        if !pose.settled {
            self.live.set(true);
        } else if !open {
            tracks.remove(&key);
        }
        pose
    }

    /// Whether `key` was also rendered in the previous window frame, i.e.
    /// the user has been looking at it without interruption. A change that
    /// happened while it was off screen is old news when it comes back, and
    /// should simply be there rather than be announced with an animation.
    pub fn continuous(&self, key: TrackKey) -> bool {
        let frame = REVEAL_FRAME.with(Cell::get);
        let mut seen = self.seen.borrow_mut();
        seen.retain(|_, last| frame.wrapping_sub(*last) <= 1);
        seen.insert(key, frame)
            .is_some_and(|last| frame.wrapping_sub(last) <= 1)
    }

    /// Drop a track, so the surface is closed without playing its exit.
    pub fn forget(&self, key: TrackKey) {
        self.tracks.borrow_mut().remove(&key);
    }

    /// Put a presence track at rest, open or closed, without playing
    /// anything: for a surface that comes back on screen in a state it
    /// reached while it was away.
    pub fn settle(&self, key: TrackKey, open: bool) {
        let mut tracks = self.tracks.borrow_mut();
        if open {
            tracks.insert(key, Track::rest(1., Instant::now()));
        } else {
            tracks.remove(&key);
        }
    }

    /// A value that springs towards `target`. The first call rests on it.
    pub fn aim(&self, key: TrackKey, target: f32, spring: Spring) -> f32 {
        let (now, reduced) = (Instant::now(), reduce_motion());
        let mut tracks = self.tracks.borrow_mut();
        let track = tracks
            .entry(key)
            .or_insert_with(|| Track::rest(target, now));
        let before = track.to;
        track.spring_to(target, spring, now, reduced);
        stats::traced(key, before, track);
        let shown = shown_at(now);
        if !track.settled(shown) {
            self.live.set(true);
        }
        track.value(shown)
    }

    /// Where a track is heading, if it exists: lets a surface that is leaving
    /// keep the position it had instead of being re-aimed.
    pub fn target(&self, key: TrackKey) -> Option<f32> {
        self.tracks.borrow().get(&key).map(Track::target)
    }

    /// Restart an open surface's entrance from a partially collapsed pose,
    /// e.g. when one menu replaces another in place.
    ///
    /// A surface still on its way in is not pushed forward: it restarts
    /// from wherever it is if that is further collapsed than `from`.
    pub fn kick(&self, key: TrackKey, from: f32, spring: Spring) {
        let (now, reduced) = (Instant::now(), reduce_motion());
        let from = self
            .tracks
            .borrow()
            .get(&key)
            .map_or(from, |track| track.value(shown_at(now)).min(from));
        let mut track = Track::rest(from, now);
        track.present(true, spring, now, reduced);
        stats::traced(key, f32::NAN, &track);
        self.tracks.borrow_mut().insert(key, track);
        self.live.set(true);
    }

    /// Start a staggered entrance for the group called `name`.
    pub fn mark(&self, name: &'static str) {
        self.marks.borrow_mut().insert(name, Instant::now());
        stats::traced((name, u64::MAX), f32::NAN, &Track::rest(0., Instant::now()));
        self.live.set(true);
    }

    /// Pose of item `index` in the group last started with [`Stage::mark`].
    /// Items read it while they render; nothing is wrapped or remounted, and
    /// once the cascade is over every item is back at rest for free.
    pub fn cascade(&self, name: &'static str, index: usize) -> Pose {
        let mut marks = self.marks.borrow_mut();
        let Some(start) = marks.get(name) else {
            return Pose::REST;
        };
        let elapsed = shown_at(Instant::now()).saturating_duration_since(*start);
        let reduced = reduce_motion();
        // Reduced, every item fades at once, so the group is done when one is.
        let length = if reduced {
            slow(REDUCED_FADE)
        } else {
            stagger_delay(STAGGER_CAP) + SNAPPY.duration()
        };
        if elapsed >= length {
            marks.remove(name);
            return Pose::REST;
        }
        self.live.set(true);
        cascade_pose(elapsed, index, reduced)
    }

    /// [`Stage::cascade`] for the next item of `name` in render order.
    pub fn cascade_next(&self, name: &'static str) -> Pose {
        if !self.marks.borrow().contains_key(name) {
            return Pose::REST;
        }
        let index = {
            let mut counters = self.counters.borrow_mut();
            let counter = counters.entry(name).or_default();
            *counter += 1;
            *counter - 1
        };
        self.cascade(name, index)
    }

    /// Pose for a small piece of content identified by `value` under `key`.
    /// It animates only when the value really changes while the view is
    /// alive: the first sighting rests, and remounting the element (another
    /// note, the same control in a different panel) never replays it.
    pub fn swap(&self, key: TrackKey, value: u64) -> Pose {
        let now = Instant::now();
        let mut swaps = self.swaps.borrow_mut();
        let entry = swaps.entry(key).or_insert((value, None));
        if entry.0 != value {
            *entry = (value, Some(now));
        }
        let Some(changed) = entry.1 else {
            return Pose::REST;
        };
        let reduced = reduce_motion();
        let elapsed = shown_at(now).duration_since(changed);
        // Reduced, the swap is a short fade and stops asking for frames
        // when the fade is over rather than when the spring would have been.
        if elapsed
            >= if reduced {
                slow(REDUCED_FADE)
            } else {
                SNAPPY.duration()
            }
        {
            entry.1 = None;
            return Pose::REST;
        }
        self.live.set(true);
        mount_pose(
            SNAPPY,
            elapsed.as_secs_f32() / SNAPPY.duration().as_secs_f32(),
            reduced,
        )
    }

    /// The shared moving highlight called `name`.
    pub fn glide(&self, name: &'static str) -> Glide {
        self.glides.borrow_mut().entry(name).or_default().clone()
    }

    /// Something timed by the view itself is still moving: ask for the next
    /// frame when this one is flushed.
    pub fn keep_alive(&self) {
        self.live.set(true);
    }

    /// Call first thing in the owning view's render.
    pub fn begin(&self) {
        self.frame_started.set(Some(Instant::now()));
        stats::began();
    }

    /// Call once at the end of the owning view's render.
    pub fn flush(&self, window: &mut Window) {
        self.flush_as("main", window);
    }

    /// [`Stage::flush`] for a view other than the main window's.
    pub fn flush_as(&self, view: &'static str, window: &mut Window) {
        self.primed.set(true);
        self.counters.borrow_mut().clear();
        let live = self.live.replace(false);
        stats::rendered(
            view,
            self.frame_started.take().unwrap_or_else(Instant::now),
            live,
        );
        if live {
            stats::cause("stage");
            window.request_animation_frame();
        }
    }
}

/// Render accounting.
///
/// Always on, and nearly free: the time of the last render, which the tour
/// uses to wait for the window to come to rest.
///
/// With `CEREAL_FRAME_STATS=1`: every render of every view is counted, timed
/// and attributed to whatever asked for it (a stage track, a hover wash, the
/// level bars, a glide, or "other": a state change, a GPUI animation, input).
/// [`stats::report`] prints the totals since the previous report, and each
/// stage transition still prints its frame pacing when it settles.
pub mod stats {
    use std::{
        cell::{Cell, RefCell},
        collections::BTreeMap,
        time::{Duration, Instant},
    };

    #[derive(Default)]
    struct Counts {
        renders: BTreeMap<&'static str, u64>,
        causes: BTreeMap<&'static str, u64>,
        pending: Vec<&'static str>,
        render_micros: u64,
        slowest_micros: u64,
        copied: u64,
        writes: u64,
        since: Option<Instant>,
        transition_last: Option<Instant>,
        transition: Vec<f32>,
        /// When the main window last rendered, and the gaps between a frame
        /// and the one before it within one animation.
        frame_last: Option<Instant>,
        intervals: Vec<f32>,
        /// Time spent building each main-window frame, in milliseconds.
        builds: Vec<f32>,
        /// When the main window's frame in progress began to render, and for
        /// each finished frame the time from there to the end of its paint
        /// pass: build, layout, prepaint and paint, everything this process
        /// does for a frame before handing the scene to the GPU.
        frame_began: Option<Instant>,
        paints: Vec<f32>,
        /// The same span as `paints`, in processor time of this thread
        /// rather than time on the clock: what the frame cost, without the
        /// time the thread spent waiting for a core on a busy machine.
        cpu_began: Option<Duration>,
        cpus: Vec<f32>,
        /// Items of windowed columns that were built, out of how many exist.
        listed: (u64, u64),
        /// Guessed row heights that were then measured, and how many of
        /// those guesses were wrong by more than a pixel.
        guessed: (u64, u64),
        /// The step in progress (see [`step_began`]): when its input was
        /// dispatched, and what the main window did after it.
        step: Option<Step>,
    }

    /// What the main window did after one scripted input: when it first
    /// rendered, what that first frame cost, every gap between its frames,
    /// and when the last frame that something moving asked for was drawn.
    #[derive(Default)]
    struct Step {
        at: Option<Instant>,
        first: Option<f32>,
        first_cost: Option<f32>,
        first_motion: Option<f32>,
        last: Option<f32>,
        last_motion: Option<f32>,
        last_frame: Option<Instant>,
        gaps: Vec<f32>,
        renders: u64,
        /// Another window's view (the pill, its popup): its first and last
        /// render after the input, and how many.
        side: Vec<(&'static str, f32, f32, u64)>,
    }

    /// The refresh interval the long-frame count is measured against:
    /// `CEREAL_REFRESH_HZ`, or 60 Hz.
    fn refresh_ms() -> f32 {
        thread_local! {
            static MS: f32 = std::env::var("CEREAL_REFRESH_HZ")
                .ok()
                .and_then(|hz| hz.trim().parse::<f32>().ok())
                .filter(|hz| *hz > 1.)
                .map_or(1000. / 60., |hz| 1000. / hz);
        }
        MS.with(|ms| *ms)
    }

    /// With `CEREAL_MOTION_TRACE=1`: print every stage track that starts
    /// moving, with its target, duration and curve, relative to the step.
    pub fn traced(key: super::TrackKey, before: f32, track: &super::Track) {
        thread_local! {
            static TRACE: bool = std::env::var_os("CEREAL_MOTION_TRACE").is_some();
        }
        if !TRACE.with(|trace| *trace) || before == track.to {
            return;
        }
        let since = COUNTS.with(|counts| {
            counts
                .borrow()
                .step
                .as_ref()
                .and_then(|step| step.at)
                .map_or(-1., |at| at.elapsed().as_secs_f32() * 1000.)
        });
        // How far the first frame that shows the change has already moved.
        let first = track.value(super::shown_at(Instant::now()));
        let share = if (track.to - track.from).abs() > f32::EPSILON {
            (first - track.from) / (track.to - track.from) * 100.
        } else {
            100.
        };
        eprintln!(
            "track {}#{} {:.2} -> {:.2} over {} ms ({:?}) at +{since:.1} ms; first frame {share:.1}% of the way",
            key.0,
            key.1,
            track.from,
            track.to,
            track.duration.as_millis(),
            track.curve,
        );
    }

    /// A scripted input is about to be dispatched: measure what follows it
    /// until the next [`report`].
    pub fn step_began() {
        if enabled() {
            COUNTS.with(|counts| {
                counts.borrow_mut().step = Some(Step {
                    at: Some(Instant::now()),
                    ..Step::default()
                })
            });
        }
    }

    /// Two renders further apart than this are not consecutive frames of one
    /// animation (a 30 Hz loop is 33 ms; a dropped 60 Hz frame is 33 ms too).
    const SAME_ANIMATION: f32 = 100.;

    /// `(median, p95, max)` of `values`, or zeros when there are none.
    pub fn percentiles(values: &mut [f32]) -> (f32, f32, f32) {
        if values.is_empty() {
            return (0., 0., 0.);
        }
        values.sort_by(f32::total_cmp);
        let at = |share: usize| values[(values.len() * share / 100).min(values.len() - 1)];
        (at(50), at(95), values[values.len() - 1])
    }

    thread_local! {
        static ENABLED: bool = std::env::var_os("CEREAL_FRAME_STATS").is_some();
        static LAST_RENDER: Cell<Option<Instant>> = const { Cell::new(None) };
        static COUNTS: RefCell<Counts> = RefCell::new(Counts::default());
    }

    pub fn enabled() -> bool {
        ENABLED.with(|enabled| *enabled)
    }

    /// Something asked for another frame. Attributed to the next render.
    pub fn cause(name: &'static str) {
        if enabled() {
            COUNTS.with(|counts| {
                let mut counts = counts.borrow_mut();
                if !counts.pending.contains(&name) {
                    counts.pending.push(name);
                }
            });
        }
    }

    /// A view finished building its frame. `live` is whether one of its stage
    /// tracks is still moving.
    pub fn rendered(view: &'static str, started: Instant, live: bool) {
        let now = Instant::now();
        LAST_RENDER.with(|last| last.set(Some(now)));
        if !enabled() {
            return;
        }
        COUNTS.with(|counts| {
            let mut counts = counts.borrow_mut();
            *counts.renders.entry(view).or_default() += 1;
            let micros = now.duration_since(started).as_micros() as u64;
            counts.render_micros += micros;
            counts.slowest_micros = counts.slowest_micros.max(micros);
            let mut causes = std::mem::take(&mut counts.pending);
            if causes.is_empty() {
                causes.push("other");
            }
            // A frame continues an animation when something that was moving
            // asked for it. A render that only answers an event (a key, a
            // pointer move, a step of the tour) starts something new, however
            // soon after the last frame it happens to come.
            let continues = causes
                .iter()
                .any(|cause| matches!(*cause, "stage" | "hover" | "glide" | "bars" | "loop"));
            for cause in causes {
                *counts.causes.entry(cause).or_default() += 1;
            }
            if view != "main" {
                if let Some(step) = counts.step.as_mut()
                    && let Some(at) = step.at
                {
                    let since = now.duration_since(at).as_secs_f32() * 1000.;
                    let index = match step.side.iter().position(|side| side.0 == view) {
                        Some(index) => index,
                        None => {
                            step.side.push((view, since, since, 0));
                            step.side.len() - 1
                        }
                    };
                    let side = &mut step.side[index];
                    side.2 = since;
                    side.3 += 1;
                }
                return;
            }
            counts.builds.push(micros as f32 / 1000.);
            counts.frame_began = Some(started);
            if let Some(step) = counts.step.as_mut()
                && let Some(at) = step.at
            {
                let since = now.duration_since(at).as_secs_f32() * 1000.;
                step.renders += 1;
                step.first.get_or_insert(since);
                step.last = Some(since);
                if continues {
                    step.first_motion.get_or_insert(since);
                    step.last_motion = Some(since);
                }
                if let Some(previous) = step.last_frame.replace(now) {
                    let gap = now.duration_since(previous).as_secs_f32() * 1000.;
                    if gap < SAME_ANIMATION {
                        step.gaps.push(gap);
                    }
                }
            }
            if let Some(last) = counts.frame_last.replace(now) {
                let gap = now.duration_since(last).as_secs_f32() * 1000.;
                if continues && gap < SAME_ANIMATION {
                    counts.intervals.push(gap);
                }
            }
            // Frame pacing of one stage transition, as before.
            if let Some(last) = counts.transition_last.take() {
                counts
                    .transition
                    .push(now.duration_since(last).as_secs_f32() * 1000.);
            }
            if live {
                counts.transition_last = Some(now);
            } else if counts.transition.len() > 2 {
                let mut sorted = std::mem::take(&mut counts.transition);
                sorted.sort_by(f32::total_cmp);
                let total: f32 = sorted.iter().sum();
                eprintln!(
                    "motion: {} frames, mean {:.2} ms, p95 {:.2} ms, max {:.2} ms",
                    sorted.len(),
                    total / sorted.len() as f32,
                    sorted[(sorted.len() * 95 / 100).min(sorted.len() - 1)],
                    sorted[sorted.len() - 1],
                );
            } else {
                counts.transition.clear();
            }
        });
    }

    /// Processor time this thread has used so far.
    #[cfg(target_os = "macos")]
    fn thread_cpu() -> Option<Duration> {
        #[repr(C)]
        struct Timespec {
            seconds: i64,
            nanoseconds: i64,
        }
        unsafe extern "C" {
            fn clock_gettime(clock: i32, time: *mut Timespec) -> i32;
        }
        // <time.h>: CLOCK_THREAD_CPUTIME_ID.
        const THREAD_CPU: i32 = 16;
        let mut time = Timespec {
            seconds: 0,
            nanoseconds: 0,
        };
        // SAFETY: `time` is a valid, writable `struct timespec`.
        (unsafe { clock_gettime(THREAD_CPU, &mut time) } == 0)
            .then(|| Duration::new(time.seconds as u64, time.nanoseconds as u32))
    }

    #[cfg(not(target_os = "macos"))]
    fn thread_cpu() -> Option<Duration> {
        None
    }

    /// A view began to render a frame.
    pub fn began() {
        if enabled() {
            COUNTS.with(|counts| counts.borrow_mut().cpu_began = thread_cpu());
        }
    }

    /// The main window reached the end of its paint pass (reported by a
    /// probe that is painted last, and only while stats are on).
    pub fn painted() {
        if !enabled() {
            return;
        }
        COUNTS.with(|counts| {
            let mut counts = counts.borrow_mut();
            if let Some(began) = counts.frame_began.take() {
                let cost = began.elapsed().as_secs_f32() * 1000.;
                counts.paints.push(cost);
                if let Some(step) = counts.step.as_mut()
                    && step.first.is_some()
                {
                    step.first_cost.get_or_insert(cost);
                }
                if let Some((began, now)) = counts.cpu_began.take().zip(thread_cpu()) {
                    counts
                        .cpus
                        .push(now.saturating_sub(began).as_secs_f32() * 1000.);
                }
            }
        });
    }

    /// Note text was copied to build a frame: `bytes` of it. Machine-speed
    /// independent evidence of how much a render duplicates.
    pub fn copied(bytes: usize) {
        if enabled() {
            COUNTS.with(|counts| counts.borrow_mut().copied += bytes as u64);
        }
    }

    /// A windowed column built `built` of its `total` items for a frame.
    pub fn listed(built: usize, total: usize) {
        if enabled() {
            COUNTS.with(|counts| {
                let mut counts = counts.borrow_mut();
                counts.listed.0 += built as u64;
                counts.listed.1 += total as u64;
            });
        }
    }

    /// A row of an estimated column was measured for the first time;
    /// `right` is whether its guessed height was within a pixel.
    pub fn guessed(right: bool) {
        if enabled() {
            COUNTS.with(|counts| {
                let mut counts = counts.borrow_mut();
                counts.guessed.0 += 1;
                counts.guessed.1 += u64::from(!right);
            });
        }
    }

    /// How long ago any view last rendered. A window that has not rendered
    /// for a while has no animation in flight.
    pub fn quiet_for() -> Duration {
        LAST_RENDER
            .with(|last| last.get())
            .map_or(Duration::MAX, |last| last.elapsed())
    }

    /// Print what happened since the previous report, and start over.
    pub fn report(label: &str) {
        if !enabled() {
            return;
        }
        COUNTS.with(|counts| {
            let mut counts = counts.borrow_mut();
            let now = Instant::now();
            let seconds = counts
                .since
                .map_or(0., |since| now.duration_since(since).as_secs_f32());
            let writes = crate::store::writes();
            let renders: u64 = counts.renders.values().sum();
            let list = |map: &BTreeMap<&'static str, u64>| {
                map.iter()
                    .map(|(name, count)| format!("{name} {count}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            let mut intervals = std::mem::take(&mut counts.intervals);
            let mut builds = std::mem::take(&mut counts.builds);
            let (interval, interval_p95, interval_max) = percentiles(&mut intervals);
            let (build, build_p95, _) = percentiles(&mut builds);
            let mut paints = std::mem::take(&mut counts.paints);
            let (paint, paint_p95, paint_max) = percentiles(&mut paints);
            let mut cpus = std::mem::take(&mut counts.cpus);
            let (cpu, cpu_p95, cpu_max) = percentiles(&mut cpus);
            // "frame cpu" is the time on the clock from the start of a
            // frame's render to the end of its paint; "thread cpu" is the
            // processor time the main thread used in that span.
            eprintln!(
                "stats {label}: {renders} renders in {seconds:.1} s ({}){}; causes: {}; render mean {:.2} ms, max {:.2} ms; {} file writes; {} note bytes copied ({}/render); frames {} median {interval:.2} ms, p95 {interval_p95:.2} ms, max {interval_max:.2} ms; build median {build:.2} ms, p95 {build_p95:.2} ms; list items built {} of {}; frame cpu {} median {paint:.2} ms, p95 {paint_p95:.2} ms, max {paint_max:.2} ms; row heights guessed {}, wrong {}; thread cpu {} median {cpu:.2} ms, p95 {cpu_p95:.2} ms, max {cpu_max:.2} ms",
                if renders == 0 {
                    "idle".to_string()
                } else {
                    format!("{:.1}/s", renders as f32 / seconds.max(0.001))
                },
                if counts.renders.len() > 1 {
                    format!(" [{}]", list(&counts.renders))
                } else {
                    String::new()
                },
                if counts.causes.is_empty() {
                    "none".to_string()
                } else {
                    list(&counts.causes)
                },
                counts.render_micros as f32 / 1000. / renders.max(1) as f32,
                counts.slowest_micros as f32 / 1000.,
                writes - counts.writes,
                counts.copied,
                counts.copied / renders.max(1),
                intervals.len(),
                counts.listed.0,
                counts.listed.1,
                paints.len(),
                counts.guessed.0,
                counts.guessed.1,
                cpus.len(),
            );
            if let Some(mut step) = counts.step.take() {
                let budget = refresh_ms();
                let long = step.gaps.iter().filter(|gap| **gap > budget * 1.5).count();
                step.gaps.sort_by(f32::total_cmp);
                let at = |share: usize| {
                    step.gaps
                        .get((step.gaps.len() * share / 100).min(step.gaps.len().saturating_sub(1)))
                        .copied()
                        .unwrap_or(0.)
                };
                let ms = |value: Option<f32>| value.map_or("-".to_string(), |v| format!("{v:.1}"));
                eprintln!(
                    "step {label}: renders {}; first render {} ms (cost {} ms); first motion {} ms; last motion {} ms; last render {} ms; gaps {} median {:.2} p95 {:.2} p99 {:.2} max {:.2} ms; long {} (> {:.1} ms)",
                    step.renders,
                    ms(step.first),
                    ms(step.first_cost),
                    ms(step.first_motion),
                    ms(step.last_motion),
                    ms(step.last),
                    step.gaps.len(),
                    at(50),
                    at(95),
                    at(99),
                    step.gaps.last().copied().unwrap_or(0.),
                    long,
                    budget * 1.5,
                );
                for (view, first, last, renders) in step.side {
                    eprintln!(
                        "step {label} {view}: renders {renders}; first render {first:.1} ms; last render {last:.1} ms"
                    );
                }
            }
            counts.listed = (0, 0);
            counts.guessed = (0, 0);
            counts.copied = 0;
            counts.renders.clear();
            counts.causes.clear();
            counts.render_micros = 0;
            counts.slowest_micros = 0;
            counts.writes = writes;
            counts.since = Some(now);
        });
    }
}

// ---------------------------------------------------------------------------
// Morph: paint-time shell morphing
// ---------------------------------------------------------------------------

/// How a surface grows out of the thing that spawned it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pop {
    /// Unit point of the surface that stays pinned to its origin: `(1., 0.)`
    /// is the top-right corner, `(0.5, 0.5)` the centre.
    pub anchor: (f32, f32),
    /// Shell size at level 0, as a share of the natural size.
    pub scale: (f32, f32),
    /// Distance the content travels away from the anchor while it grows.
    pub travel: f32,
    /// Extra upward travel, for centred dialogs that have no anchor side.
    pub rise: f32,
}

impl Pop {
    /// A menu or popover growing out of a control at `anchor`.
    pub const fn menu(anchor: (f32, f32)) -> Self {
        Self {
            anchor,
            scale: (0.86, 0.7),
            travel: 6.,
            rise: 0.,
        }
    }

    /// A centred dialog.
    pub const DIALOG: Self = Self {
        anchor: (0.5, 0.5),
        scale: (0.94, 0.9),
        travel: 0.,
        rise: 10.,
    };

    /// A badge or dot that swells into place from its own centre.
    pub const BLOB: Self = Self {
        anchor: (0.5, 0.5),
        scale: (0.4, 0.4),
        travel: 0.,
        rise: 0.,
    };

    /// A panel that unfolds upwards out of a `bar`-tall strip at its bottom
    /// edge, the way the chat grows out of the composer.
    #[allow(dead_code)]
    pub fn unfold(bar: f32, height: f32) -> Self {
        Self {
            anchor: (0.5, 1.),
            scale: (1., (bar / height.max(1.)).clamp(0.05, 1.)),
            travel: 0.,
            rise: 0.,
        }
    }

    /// A small status chip or toast.
    pub const fn chip(anchor: (f32, f32)) -> Self {
        Self {
            anchor,
            scale: (0.9, 0.8),
            travel: 8.,
            rise: 0.,
        }
    }
}

// ---------------------------------------------------------------------------
// The Ask bar: one pill that never moves, and what grows around it
// ---------------------------------------------------------------------------

/// The Ask pill's height at rest.
pub const ASK_PILL: f32 = 58.;
/// How far the tray and the chat panel stand out from the pill on its
/// left, right and bottom: the same on all three, so their corners are
/// concentric with the pill's.
pub const ASK_INSET: f32 = 4.;
/// How far the tray rises above the pill, for the recent-chats button.
pub const ASK_HEAD: f32 = 39.;
/// The focus halo's width. It is drawn inside the pill's own outline.
pub const ASK_RING: f32 = 3.;
/// The corner radius of what surrounds the pill: the pill's own plus the
/// inset between them.
pub const ASK_FRAME_RADIUS: f32 = ASK_PILL / 2. + ASK_INSET;

/// Everything about the focused Ask bar that changes while it opens, as a
/// function of its pose alone. The pill is not in here because nothing of
/// it changes: its box, its padding and its text stay where they are, and
/// the tray swells out from behind it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AskShape {
    /// The tray's distance outside the pill on the left, right and bottom:
    /// 0 is the pill's own outline. A little past [`ASK_INSET`] at the top
    /// of the spring's overshoot.
    pub out: f32,
    /// The tray's rise above the pill.
    pub head: f32,
    /// The tray's corner radius, concentric with the pill's. Never past
    /// its resting value, so an overshoot does not blunt the corners.
    pub radius: f32,
    /// The tray's opacity. It is solid while it moves, because it grows
    /// out of a pill of its own colour and a fade would read as a flicker;
    /// with reduced motion nothing moves and this fade is the transition.
    pub opacity: f32,
    /// The share of the tray's shadow that has arrived.
    pub shadow: f32,
    /// The halo's width inside the pill, never past [`ASK_RING`]: it stops
    /// short of the text however far the spring overshoots.
    pub ring: f32,
    /// The gold line's and the halo's opacity.
    pub ring_fade: f32,
    /// The recent-chats button: its opacity, and how far below its place
    /// it still is. It arrives late, once there is a tray to stand on.
    pub history: f32,
    pub history_drop: f32,
}

/// The Ask bar's shape at `pose` (see [`AskShape`]). A pose that is not
/// visible is the bar at rest.
pub fn ask_shape(pose: Pose) -> AskShape {
    let level = if pose.visible && pose.value.is_finite() {
        pose.value.max(0.)
    } else {
        0.
    };
    let within = level.min(1.);
    // Reduced motion holds the level at 1 and carries the change in `fade`.
    let still = pose.value >= 1.;
    let fade = if pose.visible && pose.fade.is_finite() {
        pose.fade.clamp(0., 1.)
    } else {
        0.
    };
    let history = {
        let x = ((within - 0.45) / 0.55).clamp(0., 1.);
        x * x * (3. - 2. * x)
    };
    AskShape {
        out: ASK_INSET * level,
        head: ASK_HEAD * level,
        radius: ASK_PILL / 2. + ASK_INSET * within,
        opacity: if still { fade } else { 1. },
        shadow: if still { fade } else { within },
        ring: ASK_RING * within,
        ring_fade: fade,
        history: history * if still { fade } else { 1. },
        history_drop: 4. * (1. - history),
    }
}

/// How the chat panel grows out of the Ask bar: from the pill's own outline
/// (`foot` 0) or from the open tray's (`foot` 1) to a `panel`-sized frame
/// whose pill stands [`ASK_INSET`] inside it. The pill itself is not part of
/// what morphs; `pill` is its height.
pub fn ask_unfold(foot: f32, pill: f32, panel: (f32, f32)) -> Pop {
    let foot = if foot.is_finite() { foot.clamp(0., 1.) } else { 0. };
    let (width, height) = (panel.0.max(1.), panel.1.max(1.));
    let inside = 2. * ASK_INSET * (1. - foot);
    Pop {
        anchor: (0.5, 1.),
        scale: (
            ((width - inside) / width).clamp(0.05, 1.),
            ((pill + (ASK_INSET + ASK_HEAD) * foot) / height).clamp(0.05, 1.),
        ),
        travel: 0.,
        // Anchored at the panel's bottom edge, the outline is lifted to the
        // pill's, which stands an inset above it.
        rise: -ASK_INSET * (1. - foot),
    }
}

/// How far the gap between a note's record capsule and its Ask pill has
/// closed at panel level `level`: 0 is two capsules, 1 one body. The bridge
/// forms early, while the body is still low, so the two read as drops
/// running together rather than as a panel laid over them.
pub fn bridge_level(level: f32) -> f32 {
    let x = if level.is_finite() { (level / 0.45).clamp(0., 1.) } else { 1. };
    x * x * (3. - 2. * x)
}

/// How far a note's Ask pill has taken in the record capsule beside it at
/// panel level `level`: 0 is two capsules side by side, 1 the pill alone at
/// the full width of the panel (the reference's open chat has no record
/// control). It begins as the two drops run together and is done before the
/// panel lands, so the open panel's composer rests at full width; folding
/// back, the capsule comes out of the pill before the two split.
pub fn capsule_absorb(level: f32) -> f32 {
    let x = if level.is_finite() { ((level - 0.35) / 0.55).clamp(0., 1.) } else { 1. };
    x * x * (3. - 2. * x)
}

/// The open part of the gap at `bridge` (see [`bridge_level`]): how far the
/// gap still reaches in from the row's top and bottom edges, of a row
/// `height` tall. It closes from the middle outwards, like two drops
/// joining at their waists.
pub fn bridge_reach(bridge: f32, height: f32) -> f32 {
    let bridge = if bridge.is_finite() { bridge.clamp(0., 1.) } else { 1. };
    (height / 2. + BRIDGE_GAP) * (1. - bridge)
}

/// The gap between a note's record capsule and its Ask pill.
pub const BRIDGE_GAP: f32 = 8.;

/// The gap between a note's record capsule and its Ask pill while the chat
/// panel grows out of them (or folds back into them): painted in the page's
/// colour over the panel's body, cut to it, and closing from the middle
/// outwards with rounded ends, so the capsules run together as the panel
/// rises and come apart as it lands. `centre` is the gap's centre from the
/// panel's leading edge, `row` the capsules' height, `reached` how much of
/// the panel (from its bottom edge) the body covers.
pub fn ask_bridge(centre: f32, row: f32, reached: f32, level: f32, page: Hsla) -> impl IntoElement {
    let bridge = bridge_level(level);
    let reach = bridge_reach(bridge, row);
    // Apart, the cut takes in the capsules' facing ends (they paint over
    // it), so what is left of the body round them is their own two
    // outlines and no web between them; joined, it is the gap alone.
    let half = BRIDGE_GAP / 2. + (row / 2. + ASK_INSET) * (1. - bridge);
    let past = (ASK_INSET + 2.) * (1. - bridge);
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            if reach <= 0. {
                return;
            }
            let bottom = bounds.bottom() - px(ASK_INSET);
            let top = bottom - px(row);
            let centre = bounds.left() + px(centre);
            let body = Bounds::from_corners(
                point(bounds.left(), (bounds.bottom() - px(reached)).max(bounds.top())),
                bounds.bottom_right(),
            );
            let cut = |from: Pixels, to: Pixels| {
                Bounds::from_corners(point(centre - px(half), from), point(centre + px(half), to))
            };
            window.with_content_mask(Some(ContentMask { bounds: body }), |window| {
                let reach = px(reach);
                for gap in [
                    cut(top - px(past), top + reach),
                    cut(bottom - reach, bottom + px(past)),
                ] {
                    window.paint_quad(fill(gap, page).corner_radii(px(half.min(row / 2.))));
                }
            });
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// Wraps an element and applies offset, opacity and a shell morph at paint
/// time. Layout is delegated untouched, so hitboxes stay live and move with
/// the paint, and nothing around the element reflows.
///
/// While the shell is scaled the wrapper takes over painting the element's
/// background, border and shadow at the morphing size and clips the content
/// to it, so the surface reads as one piece of soft matter growing from its
/// anchor rather than a rectangle fading in.
pub struct Morph<E> {
    element: Option<E>,
    shell: Option<Shell>,
    opacity: f32,
    /// Extra opacity of the content alone, so it arrives behind the shell
    /// instead of riding its moving edge.
    content: f32,
    offset: Point<Pixels>,
    scale: (f32, f32),
    anchor: (f32, f32),
}

/// Content opacity at morph level `level`: nothing until the shell is a third
/// open, complete by four fifths.
pub fn content_fade(level: f32) -> f32 {
    let x = ((level - 0.35) / 0.45).clamp(0., 1.);
    x * x * (3. - 2. * x)
}

/// How far to pull the content mask in from a shell with corner `radius`, so
/// nothing paints outside the curve: the margin of the largest rectangle
/// inside the rounded corners. It eases to nothing as the shell reaches its
/// resting size (`scale` near 1), where the mask is dropped altogether.
pub fn corner_inset(radius: f32, scale: (f32, f32)) -> f32 {
    let away = (1. - scale.0.min(scale.1)).abs();
    radius * (1. - std::f32::consts::FRAC_1_SQRT_2) * (away * 12.).min(1.)
}

struct Shell {
    background: Option<Background>,
    border_color: Option<Hsla>,
    border_widths: Edges<AbsoluteLength>,
    radii: Corners<AbsoluteLength>,
    shadows: Vec<BoxShadow>,
}

impl<E: Styled + IntoElement + 'static> Morph<E> {
    pub fn new(element: E) -> Self {
        Self {
            element: Some(element),
            shell: None,
            opacity: 1.,
            content: 1.,
            offset: point(px(0.), px(0.)),
            scale: (1., 1.),
            anchor: (0.5, 0.5),
        }
    }

    pub fn opacity(mut self, opacity: f32) -> Self {
        self.opacity = opacity.clamp(0., 1.);
        self
    }

    pub fn offset(mut self, x: f32, y: f32) -> Self {
        self.offset = point(px(x), px(y));
        self
    }

    /// Apply a [`Pop`] at the given pose.
    pub fn posed(mut self, pop: Pop, pose: Pose) -> Self {
        let rest = 1. - pose.value;
        let side = ((0.5 - pop.anchor.0) * 2., (0.5 - pop.anchor.1) * 2.);
        self.opacity = pose.fade.clamp(0., 1.);
        self.content = if pose.value < 1. {
            content_fade(pose.value)
        } else {
            1.
        };
        self.anchor = pop.anchor;
        self.scale = (
            lerp(pop.scale.0, 1., pose.value).max(0.),
            lerp(pop.scale.1, 1., pose.value).max(0.),
        );
        self.offset = point(
            px(-side.0 * pop.travel * rest),
            px(-side.1 * pop.travel * rest + pop.rise * rest),
        );
        self
    }

    fn shell_bounds(&self, bounds: Bounds<Pixels>) -> Bounds<Pixels> {
        let scaled = size(
            bounds.size.width * self.scale.0,
            bounds.size.height * self.scale.1,
        );
        Bounds::new(
            point(
                bounds.origin.x + (bounds.size.width - scaled.width) * self.anchor.0,
                bounds.origin.y + (bounds.size.height - scaled.height) * self.anchor.1,
            ),
            scaled,
        )
    }
}

impl<E: Styled + IntoElement + 'static> IntoElement for Morph<E> {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl<E: Styled + IntoElement + 'static> Element for Morph<E> {
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
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut element = self
            .element
            .take()
            .expect("morphed element is laid out once per frame");
        if self.scale != (1., 1.) {
            // Take over the shell so it can be painted at the morphing size.
            let refinement = element.style();
            let mut style = Style::default();
            style.refine(refinement);
            if style.background.is_some()
                || style.border_color.is_some()
                || !style.box_shadow.is_empty()
            {
                refinement.background = None;
                refinement.border_color = None;
                refinement.box_shadow = None;
                self.shell = Some(Shell {
                    background: style.background.map(|Fill::Color(background)| background),
                    border_color: style.border_color,
                    border_widths: style.border_widths,
                    radii: style.corner_radii,
                    shadows: style.box_shadow,
                });
            }
        }
        let content = self.opacity * self.content;
        let mut element = if content < 1. {
            element.opacity(content).into_any_element()
        } else {
            element.into_any_element()
        };
        let layout = element.request_layout(window, cx);
        (layout, element)
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        element: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) {
        window.with_element_offset(self.offset, |window| {
            element.prepaint(window, cx);
        });
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        element: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let Some(shell) = self.shell.as_ref() else {
            element.paint(window, cx);
            return;
        };
        let rect = self.shell_bounds(bounds + self.offset);
        let rem = window.rem_size();
        let radii = shell
            .radii
            .to_pixels(rem)
            .clamp_radii_for_quad_size(rect.size);
        let opacity = self.opacity;
        if !shell.shadows.is_empty() {
            // The shadow trails the fill, so it never shows through a shell
            // that is still translucent.
            let shadows: Vec<_> = shell
                .shadows
                .iter()
                .map(|shadow| BoxShadow {
                    color: shadow.color.opacity(opacity * opacity),
                    ..shadow.clone()
                })
                .collect();
            window.paint_shadows(rect, radii, &shadows);
        }
        if let Some(background) = shell.background {
            window.paint_quad(fill(rect, background.opacity(opacity.sqrt())).corner_radii(radii));
        }
        // GPUI masks are rectangles. Pull the mask inside the corner arcs so
        // content can never paint outside the rounded shell.
        let inset = px(corner_inset(
            f32::from(radii.top_left.max(radii.bottom_right)),
            self.scale,
        ))
        .min(rect.size.width.min(rect.size.height) / 2.);
        let mask = rect.inset(inset);
        window.with_content_mask(Some(ContentMask { bounds: mask }), |window| {
            element.paint(window, cx);
        });
        if let Some(border_color) = shell.border_color {
            window.paint_quad(quad(
                rect,
                radii,
                transparent_black(),
                shell.border_widths.to_pixels(rem),
                border_color.opacity(opacity),
                BorderStyle::default(),
            ));
        }
    }
}

/// Morph a surface out of (or back into) its anchor at the given pose.
/// A surface at rest is returned untouched, so idle UI pays nothing.
pub fn pop<E: Styled + IntoElement + 'static>(element: E, pop: Pop, pose: Pose) -> AnyElement {
    if pose.at_rest() {
        return element.into_any_element();
    }
    Morph::new(element).posed(pop, pose).into_any_element()
}

/// The pose of something `progress` of the way through arriving on `spring`.
pub fn mount_pose(spring: Spring, progress: f32, reduced: bool) -> Pose {
    if reduced {
        let fade = fade_curve(progress * spring.millis as f32 / REDUCED_FADE.as_millis() as f32);
        return Pose {
            value: 1.,
            fade,
            visible: true,
            settled: progress >= 1.,
        };
    }
    let value = spring.at(progress);
    Pose {
        value,
        fade: fade_for(value),
        visible: true,
        settled: progress >= 1.,
    }
}

/// Pose of item `index`, `elapsed` after its staggered group started.
pub fn cascade_pose(elapsed: Duration, index: usize, reduced: bool) -> Pose {
    let delay = if reduced {
        Duration::ZERO
    } else {
        stagger_delay(index)
    };
    let Some(running) = elapsed.checked_sub(delay) else {
        return Pose {
            value: 0.,
            fade: 0.,
            visible: true,
            settled: false,
        };
    };
    mount_pose(
        SNAPPY,
        running.as_secs_f32() / SNAPPY.duration().as_secs_f32(),
        reduced,
    )
}

/// Apply a staggered item's pose in place: it rises a few pixels as it fades
/// in, without affecting its siblings' layout.
pub fn lift<E: Styled>(element: E, pose: Pose) -> E {
    if pose.at_rest() {
        return element;
    }
    element
        .relative()
        .top(px(RISE_TRAVEL * (1. - pose.value)))
        .opacity(pose.fade)
}

/// Replace one small piece of content with another (an icon, a status):
/// the newcomer fades in while rising a few pixels on a snappy spring.
pub fn swap_in<E: Styled + IntoElement + 'static>(
    id: impl Into<ElementId>,
    element: E,
) -> AnyElement {
    let reduced = reduce_motion();
    // Reduced, only the short fade runs, and no frames are drawn after it.
    let length = if reduced {
        slow(REDUCED_FADE)
    } else {
        SNAPPY.duration()
    };
    let share = length.as_secs_f32() / SNAPPY.duration().as_secs_f32();
    let (animation, skipped) = led_animation(length);
    Morph::new(element)
        .with_animation(id, animation, move |morph, t| {
            let pose = mount_pose(SNAPPY, led(skipped, t) * share, reduced);
            morph.opacity(pose.fade).offset(0., 4. * (1. - pose.value))
        })
        .into_any_element()
}

/// Opacity and vertical offset of a page `t` of the way through arriving.
pub fn page_pose(t: f32, reduced: bool) -> (f32, f32) {
    let eased = fade_curve(t);
    (
        PAGE_FROM + (1. - PAGE_FROM) * eased,
        if reduced {
            0.
        } else {
            PAGE_TRAVEL * (1. - eased)
        },
    )
}

/// Page-level entrance: the new screen firms up and rises a few pixels. It
/// is the most repeated transition in the app, so it is short and quiet.
pub fn page<E: Styled + IntoElement + 'static>(id: impl Into<ElementId>, element: E) -> AnyElement {
    let reduced = reduce_motion();
    let (animation, skipped) = led_animation(slow(if reduced { REDUCED_FADE } else { PAGE_FADE }));
    Morph::new(element)
        .with_animation(id, animation, move |morph, t| {
            let (opacity, rise) = page_pose(led(skipped, t), reduced);
            morph.opacity(opacity).offset(0., rise)
        })
        .into_any_element()
}

/// Swap content in place (the note's tabs): a brief firming-up with no
/// movement, so text the reader is following does not shift.
pub fn crossfade<E: Styled + IntoElement + 'static>(
    id: impl Into<ElementId>,
    element: E,
) -> AnyElement {
    let (animation, skipped) = led_animation(slow(REDUCED_FADE));
    Morph::new(element)
        .with_animation(id, animation, move |morph, t| {
            morph.opacity(page_pose(led(skipped, t), true).0)
        })
        .into_any_element()
}

/// Apply a [`Stage::swap`] pose: a small piece of content that just replaced
/// another fades in while rising a few pixels.
pub fn swapped<E: Styled + IntoElement + 'static>(element: E, pose: Pose) -> AnyElement {
    if pose.at_rest() {
        return element.into_any_element();
    }
    Morph::new(element)
        .opacity(pose.fade)
        .offset(0., 4. * (1. - pose.value))
        .into_any_element()
}

// ---------------------------------------------------------------------------
// Glide: a highlight that stretches between positions
// ---------------------------------------------------------------------------

#[derive(Default)]
struct GlideState {
    /// Bounds reported by the selected item during this frame's prepaint.
    fresh: Option<(u64, Bounds<Pixels>)>,
    key: Option<u64>,
    from: Bounds<Pixels>,
    to: Bounds<Pixels>,
    start: Option<Instant>,
    shown: Option<Bounds<Pixels>>,
    /// Opacity of the highlight: it eases in where a selection appears and
    /// out where it goes away, like the hover wash, instead of popping.
    alpha: Option<Track>,
    /// How far ahead of `now` frames are evaluated ([`FRAME_LEAD`] when
    /// painting; zero in tests).
    lead: Duration,
}

/// A selection highlight shared by a group of items. The selected item drops
/// a [`Glide::probe`] inside itself; one [`Glide::indicator`] placed before
/// the items paints the highlight and, whenever the selection moves, stretches
/// it across like a droplet before it snaps back to size.
///
/// The probe only measures and the indicator only paints, so moving the
/// highlight never lays anything out.
#[derive(Clone, Default)]
pub struct Glide(Rc<RefCell<GlideState>>);

fn hash_key(key: impl Hash) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut hasher);
    hasher.finish()
}

/// Interpolate a highlight rectangle with stretch-and-catch-up on both axes.
pub fn stretch_bounds(from: Bounds<Pixels>, to: Bounds<Pixels>, t: f32) -> Bounds<Pixels> {
    if !t.is_finite() || t <= 0. {
        return from;
    }
    if t >= 1. {
        return to;
    }
    let axis = |from_low: Pixels, from_size: Pixels, to_low: Pixels, to_size: Pixels| {
        let (from_low, to_low) = (f32::from(from_low), f32::from(to_low));
        stretch(
            (from_low, from_low + f32::from(from_size)),
            (to_low, to_low + f32::from(to_size)),
            t,
        )
    };
    let (left, right) = axis(from.origin.x, from.size.width, to.origin.x, to.size.width);
    let (top, bottom) = axis(from.origin.y, from.size.height, to.origin.y, to.size.height);
    Bounds::new(
        point(px(left), px(top)),
        size(px(right - left), px(bottom - top)),
    )
}

impl Glide {
    /// Put this inside the selected item (which must not clip it away). It
    /// fills the item and reports its bounds; it paints nothing.
    pub fn probe(&self, key: impl Hash) -> Canvas<()> {
        let state = self.0.clone();
        let key = hash_key(key);
        canvas(
            move |bounds, _, _| state.borrow_mut().fresh = Some((key, bounds)),
            |_, _, _, _| {},
        )
        .absolute()
        .inset_0()
    }

    /// Paints the highlight. Place it before the items so it sits under them.
    pub fn indicator(&self, color: Hsla, radius: f32) -> Canvas<()> {
        self.outlined_indicator(color, None, radius)
    }

    /// [`Glide::indicator`] with an optional 1 px outline, for a selected
    /// capsule that needs an edge against a similar surface.
    pub fn outlined_indicator(
        &self,
        color: Hsla,
        outline: Option<Hsla>,
        radius: f32,
    ) -> Canvas<()> {
        let state = self.0.clone();
        canvas(
            |_, _, _| (),
            move |_, _, window, _| {
                let now = Instant::now();
                let painted = {
                    let mut state = state.borrow_mut();
                    state.lead = FRAME_LEAD;
                    state.advance(now, reduce_motion())
                };
                if !state.borrow().settled(now) {
                    stats::cause("glide");
                    window.request_animation_frame();
                }
                let Some((rect, alpha)) = painted else {
                    return;
                };
                let radius = px(radius).min(rect.size.height / 2.);
                let quad = fill(rect, color.opacity(alpha)).corner_radii(radius);
                window.paint_quad(match outline {
                    Some(outline) => quad
                        .border_widths(px(1.))
                        .border_color(outline.opacity(alpha)),
                    None => quad,
                });
            },
        )
        .absolute()
        .size_0()
    }
}

impl GlideState {
    fn settled(&self, now: Instant) -> bool {
        let shown = now + self.lead;
        self.start
            .is_none_or(|start| shown.saturating_duration_since(start) >= GOOEY.duration())
            && self.alpha.is_none_or(|alpha| alpha.settled(shown))
    }

    /// Ease the highlight's opacity towards `to`.
    fn fade_to(&mut self, to: f32, now: Instant) {
        let duration = if to > 0.5 { HOVER_IN } else { HOVER_OUT };
        // No track means at rest: unseen before a fade in, opaque before a
        // fade out.
        let alpha = self
            .alpha
            .get_or_insert(Track::rest(if to > 0.5 { 0. } else { 1. }, now));
        if (alpha.to - to).abs() > f32::EPSILON {
            alpha.retarget(to, now, slow(duration), Curve::Fade);
        }
    }

    /// Consume this frame's probe report and return where to paint, and at
    /// what opacity.
    fn advance(&mut self, now: Instant, reduced: bool) -> Option<(Bounds<Pixels>, f32)> {
        let shown_at = now + self.lead;
        let Some((key, rect)) = self.fresh.take() else {
            // Nothing is selected in this group right now: the highlight
            // fades out where it was, and the position is forgotten.
            self.key = None;
            self.start = None;
            let shown = self.shown?;
            self.fade_to(0., now);
            let alpha = self.alpha.map_or(0., |alpha| alpha.value(shown_at));
            if alpha <= 0. {
                self.shown = None;
                self.alpha = None;
                return None;
            }
            return Some((shown, alpha));
        };
        if self.key != Some(key) {
            match self.shown {
                // The selection moved: start from wherever the highlight is.
                Some(shown) if self.key.is_some() && !reduced => {
                    self.from = shown;
                    self.start = Some(now);
                }
                _ => {
                    self.from = rect;
                    self.start = None;
                }
            }
            if self.key.is_none() {
                // A selection appearing (or coming back while the last one
                // fades out) eases in where it is, without flying in.
                self.fade_to(1., now);
            }
            self.key = Some(key);
            self.to = rect;
        } else {
            // Same item, new bounds (it is resizing, or the layout moved): keep
            // heading for wherever it is now instead of restarting.
            self.to = rect;
        }
        let shown = match self.start {
            Some(start) if shown_at.saturating_duration_since(start) < GOOEY.duration() => {
                stretch_bounds(
                    self.from,
                    self.to,
                    shown_at.saturating_duration_since(start).as_secs_f32()
                        / GOOEY.duration().as_secs_f32(),
                )
            }
            _ => {
                self.start = None;
                self.to
            }
        };
        self.shown = Some(shown);
        let alpha = self.alpha.map_or(1., |alpha| alpha.value(shown_at));
        if self
            .alpha
            .is_some_and(|alpha| alpha.to >= 1. && alpha.settled(shown_at))
        {
            self.alpha = None;
        }
        Some((shown, alpha))
    }
}

// ---------------------------------------------------------------------------
// Tactile: eased hover and squash on press
// ---------------------------------------------------------------------------

/// The pointer feedback of a control, painted as its first child: a hover
/// wash that eases in and out, and a squash on mouse-down that springs back
/// on release. The control's own layout and content never change, so it stays
/// crisp and its hit target stays exactly where it was.
pub struct Tactile {
    hover: Hsla,
    press: Option<Hsla>,
    base: Option<Hsla>,
    radius: Option<f32>,
    /// Separate corner radii (top left, top right, bottom right, bottom
    /// left), for a control joined to a neighbour inside one capsule.
    corners: Option<[f32; 4]>,
    reveals: Option<u64>,
}

thread_local! {
    /// Eased hover levels published by [`Tactile::reveals`], by key, with the
    /// main-window frame that published them.
    static REVEALS: RefCell<HashMap<u64, (f32, u64)>> = RefCell::new(HashMap::new());
    static REVEAL_FRAME: Cell<u64> = const { Cell::new(0) };
}

/// Mark the start of a main-window frame. A level published by a control
/// that has since left the tree (its row was deleted, the page changed while
/// it was hovered) is one frame old by the next build and reads as zero, so
/// a control that comes back under the same key does not flash revealed.
pub fn frame() {
    REVEAL_FRAME.with(|frame| frame.set(frame.get().wrapping_add(1)));
}

fn publish(key: u64, level: f32) -> bool {
    let frame = REVEAL_FRAME.with(Cell::get);
    REVEALS.with(|levels| {
        let mut levels = levels.borrow_mut();
        let before = levels.get(&key).map_or(0., |(level, _)| *level);
        if level <= 0. {
            levels.remove(&key);
        } else {
            levels.insert(key, (level, frame));
        }
        (before - level).abs() > 0.001
    })
}

/// A key naming one hover group (a row, a heading) for [`revealed`].
pub fn reveal_key(group: impl Hash) -> u64 {
    hash_key(group)
}

/// How far the group `key` is hovered, 0..=1, eased. Controls that only show
/// under the pointer take their opacity from this instead of snapping with
/// GPUI's `group_hover`: the group's feedback layer (see [`Tactile::reveals`])
/// already eases its hover wash, and this is the same value, so revealing
/// costs no frames of its own. It trails the pointer by one frame.
pub fn revealed(key: u64) -> f32 {
    let frame = REVEAL_FRAME.with(Cell::get);
    REVEALS.with(|levels| {
        levels
            .borrow()
            .get(&key)
            .filter(|(_, published)| frame.wrapping_sub(*published) <= 1)
            .map_or(0., |(level, _)| *level)
    })
}

/// `from` blended toward `to` by `level`, for text that warms on hover.
pub fn mix(from: Hsla, to: Hsla, level: f32) -> Hsla {
    let level = level.clamp(0., 1.);
    let (from, to) = (Rgba::from(from), Rgba::from(to));
    let lerp = |a: f32, b: f32| a + (b - a) * level;
    Rgba {
        r: lerp(from.r, to.r),
        g: lerp(from.g, to.g),
        b: lerp(from.b, to.b),
        a: lerp(from.a, to.a),
    }
    .into()
}

/// Show `element` only while its group is hovered.
pub fn reveal<E: Styled>(element: E, key: u64) -> E {
    element.opacity(revealed(key))
}

/// Hide `element` while its group is hovered (what a revealed control replaces).
pub fn conceal<E: Styled>(element: E, key: u64) -> E {
    element.opacity(1. - revealed(key))
}

/// A link that fades a little under the pointer: `element` (which needs its
/// own id) loses `dim` of its opacity, eased.
pub fn dims<E: Styled + ParentElement>(element: E, key: u64, dim: f32) -> E {
    element
        .child(hover_probe(key))
        .opacity(1. - dim * revealed(key))
}

/// A link whose text warms from `from` to `to` under the pointer, eased.
pub fn warms<E: Styled + ParentElement>(element: E, key: u64, from: Hsla, to: Hsla) -> E {
    element
        .child(hover_probe(key))
        .text_color(mix(from, to, revealed(key)))
}

/// A feedback layer that paints nothing and only tracks hover for [`revealed`].
pub fn hover_probe(key: u64) -> Tactile {
    tactile(transparent_black()).radius(0.).reveals(key)
}

/// Pointer feedback with a `hover` wash and 6 px corners.
pub fn tactile(hover: Hsla) -> Tactile {
    Tactile {
        hover,
        press: None,
        base: None,
        radius: Some(6.),
        corners: None,
        reveals: None,
    }
}

impl Tactile {
    pub fn radius(mut self, radius: f32) -> Self {
        self.radius = Some(radius);
        self
    }

    /// Fully rounded ends (a capsule or a circle).
    pub fn round(mut self) -> Self {
        self.radius = None;
        self.corners = None;
        self
    }

    /// A radius per corner: top left, top right, bottom right, bottom left.
    pub fn corners(
        mut self,
        top_left: f32,
        top_right: f32,
        bottom_right: f32,
        bottom_left: f32,
    ) -> Self {
        self.corners = Some([top_left, top_right, bottom_right, bottom_left]);
        self
    }

    /// An extra tint while the control is held down.
    pub fn press(mut self, color: Hsla) -> Self {
        self.press = Some(color);
        self
    }

    /// Publish this control's eased hover level under `key` for [`revealed`].
    pub fn reveals(mut self, key: u64) -> Self {
        self.reveals = Some(key);
        self
    }

    /// A resting fill that squashes with the press. Use it instead of a
    /// background on the control itself for filled buttons.
    pub fn base(mut self, color: Hsla) -> Self {
        self.base = Some(color);
        self
    }
}

#[derive(Default)]
struct Pointer {
    pressed: Cell<bool>,
    hovered: Cell<bool>,
}

struct TactileState {
    hover: Track,
    press: Track,
    pointer: Rc<Pointer>,
}

/// Squash distance for a control of the given size.
pub fn squash_inset(size: Size<Pixels>) -> f32 {
    (f32::from(size.width.min(size.height)) * SQUASH).clamp(0.75, 2.)
}

impl IntoElement for Tactile {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Tactile {
    type RequestLayoutState = ();
    type PrepaintState = Hitbox;

    fn id(&self) -> Option<ElementId> {
        Some("tactile".into())
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
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.refine(&StyleRefinement::default().absolute().inset_0());
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        _: &mut App,
    ) -> Hitbox {
        window.insert_hitbox(bounds, HitboxBehavior::Normal)
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        hitbox: &mut Hitbox,
        window: &mut Window,
        _: &mut App,
    ) {
        let Some(id) = id else { return };
        let (now, reduced) = (Instant::now(), reduce_motion());
        let hovered = hitbox.is_hovered(window);
        let (hover, press, moving, pointer) =
            window.with_element_state::<TactileState, _>(id, |state, _| {
                let mut state = state.unwrap_or_else(|| TactileState {
                    hover: Track::rest(0., now),
                    press: Track::rest(0., now),
                    pointer: Rc::default(),
                });
                state.pointer.hovered.set(hovered);
                let hover = if hovered { 1. } else { 0. };
                if state.hover.target() != hover {
                    let duration = if hovered { HOVER_IN } else { HOVER_OUT };
                    state
                        .hover
                        .retarget(hover, now, slow(duration), Curve::Fade);
                }
                let pressed = hovered && state.pointer.pressed.get();
                let press = if pressed { 1. } else { 0. };
                if state.press.target() != press {
                    if pressed {
                        state
                            .press
                            .retarget(press, now, slow(PRESS_IN), Curve::Fade);
                    } else {
                        state
                            .press
                            .retarget(press, now, SNAPPY.duration(), Curve::Spring(SNAPPY));
                    }
                }
                let moving = !state.hover.settled(now) || !state.press.settled(now);
                (
                    (
                        state.hover.value(now),
                        state.press.value(now),
                        moving,
                        state.pointer.clone(),
                    ),
                    state,
                )
            });
        if moving {
            stats::cause("hover");
            window.request_animation_frame();
        }
        // Readers took last frame's level while this frame was being built;
        // when the level settles, one more frame lets them land on it.
        if let Some(key) = self.reveals
            && publish(key, hover.clamp(0., 1.))
            && !moving
        {
            stats::cause("hover");
            window.request_animation_frame();
        }

        let inset = if reduced {
            0.
        } else {
            press * squash_inset(bounds.size)
        };
        let rect = bounds.inset(px(inset));
        let limit = rect.size.width.min(rect.size.height) / 2.;
        let corner = |radius: f32| px((radius - inset).max(0.)).min(limit);
        let radius: Corners<Pixels> = match self.corners {
            Some([top_left, top_right, bottom_right, bottom_left]) => Corners {
                top_left: corner(top_left),
                top_right: corner(top_right),
                bottom_right: corner(bottom_right),
                bottom_left: corner(bottom_left),
            },
            None => Corners::all(self.radius.map_or(limit, corner)),
        };
        let held = press.clamp(0., 1.);
        if let Some(base) = self.base {
            window.paint_quad(fill(rect, base).corner_radii(radius));
        }
        let wash = hover.max(held);
        if wash > 0.004 {
            window.paint_quad(fill(rect, self.hover.opacity(wash)).corner_radii(radius));
        }
        if let Some(tint) = self.press
            && held > 0.004
        {
            window.paint_quad(fill(rect, tint.opacity(held)).corner_radii(radius));
        }

        let (down, hit) = (pointer.clone(), hitbox.clone());
        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, _| {
            if phase == DispatchPhase::Bubble
                && event.button == MouseButton::Left
                && hit.is_hovered(window)
            {
                down.pressed.set(true);
                stats::cause("pointer");
                window.refresh();
            }
        });
        let up = pointer.clone();
        window.on_mouse_event(move |_: &MouseUpEvent, phase, window, _| {
            if phase == DispatchPhase::Capture && up.pressed.replace(false) {
                stats::cause("pointer");
                window.refresh();
            }
        });
        let hit = hitbox.clone();
        window.on_mouse_event(move |_: &MouseMoveEvent, phase, window, _| {
            if phase == DispatchPhase::Capture && hit.is_hovered(window) != pointer.hovered.get() {
                stats::cause("pointer");
                window.refresh();
            }
        });
    }
}

// ---------------------------------------------------------------------------
// Live levels: eased bars for audio
// ---------------------------------------------------------------------------

struct BarsState {
    bars: Vec<Track>,
    /// When the next animation frame may be asked for.
    next_frame: Instant,
}

/// How long a level bar takes to reach a new height. Like the reference
/// app's visualizer, each new target is a fresh 300 ms ease-in-out from
/// wherever the bar is, with no overshoot.
const BAR_GLIDE: Duration = Duration::from_millis(300);

/// Level bars are three small shapes: 30 frames a second is smooth for them
/// and halves what a meeting-long animation costs.
const BARS_FRAME: Duration = Duration::from_millis(33);
/// [`BARS_FRAME`] is kept by the display's own refreshes, not by a timer:
/// a 33 ms timer started at paint time wakes late (timers are coalesced,
/// more so on a busy machine or in the background) and its frame then waits
/// for the refresh after, so it ran at 20 frames a second at 60 Hz. Instead
/// the view is redrawn on the first refresh at least this long after the
/// last paint (a 33 ms frame less the slack for a frame's own build time):
/// every second refresh at 60 Hz, every third at 120 Hz.
const BARS_WAKE: Duration = BARS_FRAME.saturating_sub(Duration::from_millis(12));

/// Redraw the current view on the first display refresh at or after `due`.
/// The refreshes in between run a callback and draw nothing.
fn redraw_at(window: &Window, due: Instant, cause: &'static str) {
    let view = window.current_view();
    fn wait(window: &Window, view: EntityId, due: Instant, cause: &'static str) {
        window.on_next_frame(move |window, cx| {
            if Instant::now() >= due {
                stats::cause(cause);
                cx.notify(view);
            } else {
                wait(window, view, due, cause);
            }
        });
    }
    wait(window, view, due, cause);
}

/// A row of vertical level bars that ease to each new target height.
/// It paints quads directly and requests frames only until the bars come to
/// rest, so a quiet or stopped recording costs nothing.
pub struct Bars {
    id: ElementId,
    heights: Vec<f32>,
    bar: f32,
    gap: f32,
    height: f32,
    color: Hsla,
}

/// `heights` are the target bar heights in pixels inside a box `height` tall;
/// each bar is `bar` wide with `gap` between bars.
pub fn bars(
    id: impl Into<ElementId>,
    heights: impl IntoIterator<Item = f32>,
    bar: f32,
    gap: f32,
    height: f32,
    color: Hsla,
) -> Bars {
    Bars {
        id: id.into(),
        heights: heights.into_iter().collect(),
        bar,
        gap,
        height,
        color,
    }
}

impl IntoElement for Bars {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Bars {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
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
    ) -> (LayoutId, Self::RequestLayoutState) {
        let count = self.heights.len() as f32;
        let width = count * self.bar + (count - 1.).max(0.) * self.gap;
        let mut style = Style::default();
        style.refine(
            &StyleRefinement::default()
                .w(px(width))
                .h(px(self.height))
                .flex_shrink_0(),
        );
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Window,
        _: &mut App,
    ) {
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        _: &mut App,
    ) {
        let Some(id) = id else { return };
        let (now, reduced) = (Instant::now(), reduce_motion());
        let targets = &self.heights;
        let (heights, moving) = window.with_element_state::<BarsState, _>(id, |state, _| {
            let mut state = state.unwrap_or_else(|| BarsState {
                bars: targets
                    .iter()
                    .map(|height| Track::rest(*height, now))
                    .collect(),
                next_frame: now,
            });
            state.bars.resize(targets.len(), Track::rest(0., now));
            let mut moving = false;
            for (bar, target) in state.bars.iter_mut().zip(targets) {
                if reduced {
                    *bar = Track::rest(*target, now);
                    continue;
                }
                if (bar.target() - target).abs() > f32::EPSILON {
                    bar.retarget(*target, now, slow(BAR_GLIDE), Curve::Standard);
                }
                moving |= !bar.settled(now);
            }
            // One timer per 33 ms however often the view happens to render.
            let moving = moving && {
                let due = now >= state.next_frame;
                if due {
                    state.next_frame = now + BARS_WAKE;
                }
                due
            };
            (
                (
                    state
                        .bars
                        .iter()
                        .map(|bar| bar.value(now))
                        .collect::<Vec<_>>(),
                    moving,
                ),
                state,
            )
        });
        if moving {
            redraw_at(window, now + BARS_WAKE, "bars");
        }
        for (index, height) in heights.into_iter().enumerate() {
            let height = height.clamp(self.bar.min(self.height), self.height);
            let rect = Bounds::new(
                point(
                    bounds.origin.x + px(index as f32 * (self.bar + self.gap)),
                    bounds.origin.y + px((self.height - height) / 2.),
                ),
                size(px(self.bar), px(height)),
            );
            window.paint_quad(fill(rect, self.color).corner_radii(px(self.bar / 2.)));
        }
    }
}

// ---------------------------------------------------------------------------
// Status indicators
// ---------------------------------------------------------------------------

/// One turn of the spinner, eased so it swings rather than ticks. Periodic:
/// it starts and ends each turn at the same speed, so the loop has no seam.
pub fn fluid_turn(progress: f32) -> f32 {
    let progress = progress.clamp(0., 1.);
    // Clamped: rotation percentages must stay inside 0..=1 despite rounding.
    (progress - 0.55 * (std::f32::consts::TAU * progress).sin() / std::f32::consts::TAU)
        .clamp(0., 1.)
}

/// A 16 px, 2 px stroke spinner with a transparent bottom quadrant.
pub fn spinner(color: Hsla) -> AnyElement {
    let reduced = reduce_motion();
    svg()
        .path("icons/spinner.svg")
        .size(px(16.))
        .flex_shrink_0()
        .text_color(color)
        .with_animation(
            "spinner-rotation",
            Animation::new(Duration::from_secs(1)).repeat(),
            move |element, progress| {
                let turn = if reduced {
                    progress
                } else {
                    fluid_turn(progress)
                };
                element.with_transformation(Transformation::rotate(percentage(turn)))
            },
        )
        .into_any_element()
}

/// A short status in 14 px text with the chat's five-second sheen running
/// across it ("Thinking...", "Reading the transcript…"). Reduced motion
/// leaves it still.
pub fn shimmer_label(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    theme: Theme,
    cx: &App,
) -> SlowLoop<StyledText> {
    let label: SharedString = label.into();
    let font_id = cx.text_system().resolve_font(&font(FONT));
    let advances: Vec<_> = label
        .char_indices()
        .map(|(start, character)| {
            let width = cx
                .text_system()
                .advance(font_id, px(14.), character)
                .map(|size| f32::from(size.width))
                .unwrap_or(7.);
            (start..start + character.len_utf8(), width)
        })
        .collect();
    let width = advances
        .iter()
        .map(|(_, width)| *width)
        .sum::<f32>()
        .max(1.);
    let mut x = 0.;
    let glyphs: Vec<_> = advances
        .into_iter()
        .map(|(range, advance)| {
            let center = (x + advance / 2.) / width;
            x += advance;
            (range, center)
        })
        .collect();
    // A five-second sweep: stepped at 30 Hz, not at the display rate.
    slow_loop(
        id,
        Duration::from_secs(5),
        StyledText::new(label),
        move |text, turns| {
            let progress = css_ease_in_out(turns.fract());
            text.with_highlights(
                glyphs
                    .iter()
                    .map(|(range, center)| {
                        let mut color = theme.text;
                        color.a *= shimmer_opacity(*center, progress);
                        (
                            range.clone(),
                            HighlightStyle {
                                color: Some(color),
                                ..Default::default()
                            },
                        )
                    })
                    .collect::<Vec<_>>(),
            )
        },
    )
}

/// The mark while an answer is being thought about: it turns steadily and
/// breathes (a little smaller and dimmer at the end of each breath), so the
/// wait reads as work under way. Reduced motion leaves it still.
pub fn thinking_mark(size: f32, color: Hsla) -> AnyElement {
    let mark = icon("cereal", size, color);
    if reduce_motion() {
        return mark.into_any_element();
    }
    mark.with_animation(
        "thinking-mark",
        Animation::new(slow(Duration::from_millis(2400))).repeat(),
        move |mark, progress| {
            let (turn, scale, ink) = thinking_mark_pose(progress);
            let mut color = color;
            color.a *= ink;
            mark.text_color(color).with_transformation(
                Transformation::rotate(percentage(turn)).with_scaling(size2(scale, scale)),
            )
        },
    )
    .into_any_element()
}

fn size2(width: f32, height: f32) -> Size<f32> {
    Size { width, height }
}

/// Where the thinking mark is `progress` of the way through its loop: its
/// turn (0..=1, one whole turn a loop, swung rather than ticked), its scale
/// and its ink. Two breaths a turn; the loop has no seam.
pub fn thinking_mark_pose(progress: f32) -> (f32, f32, f32) {
    let progress = progress.clamp(0., 1.);
    let breath = 0.5 - 0.5 * (2. * std::f32::consts::TAU * progress).cos();
    (fluid_turn(progress), 1. - 0.12 * breath, 1. - 0.35 * breath)
}

/// One loop of the Cereal loader, in the mark's own 24-unit box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LoaderPose {
    /// The bowl's turn (1 is a whole turn; it overshoots a little past it)
    /// and its scale.
    pub bowl: (f32, f32),
    /// Each flake's centre and scale.
    pub flakes: [((f32, f32), f32); 2],
}

/// The loader's loop. The bowl takes this share of it to spin; the flakes
/// follow, then hop one after the other while the bowl rests.
pub(crate) const LOADER_LOOP: Duration = Duration::from_millis(2200);
const LOADER_SPIN: f32 = 0.42;
const LOADER_LAG: f32 = 0.055;
/// The spin's spring: loose, so the bowl swings past and settles back.
const LOADER_SPRING: Spring = Spring {
    millis: 0,
    damping: 0.56,
};
/// The middle of the bowl's ring, and the flakes' centres and radii at rest.
/// The ring's middle is not the middle of the path's bounds: the bite takes
/// the ring's right-hand edge, so the bounds stop short of it there.
pub(crate) const LOADER_BOWL: (f32, f32) = (10.9925, 12.2575);
pub(crate) const LOADER_FLAKES: [((f32, f32), f32); 2] = [((21.835, 2.149), 0.7), ((22.147, 6.575), 1.152)];
/// The ring's outer radius, and the least dark kept between it and a flake.
pub(crate) const LOADER_RING: f32 = 10.2925;
const LOADER_GAP: f32 = 0.55;

/// 0 → 1 → 0 across `t` in 0..=1, flat at both ends; 0 outside it.
pub(crate) fn bump(t: f32) -> f32 {
    if !(0. ..=1.).contains(&t) {
        return 0.;
    }
    (std::f32::consts::PI * t).sin().powi(2)
}

/// Where the Cereal loader is `progress` of the way through its loop. The
/// bowl spins once on a loose spring, squeezing as it goes; the flakes are
/// flung round after it, each a beat later and pulled in toward the bowl,
/// then hop outward in turn. It starts and ends at rest: no seam.
pub fn cereal_loader_pose(progress: f32) -> LoaderPose {
    let p = progress.clamp(0., 1.);
    let spin = p / LOADER_SPIN;
    let bowl = (LOADER_SPRING.at(spin), 1. - 0.08 * bump(spin));
    let flakes = std::array::from_fn(|index| {
        let ((x, y), radius) = LOADER_FLAKES[index];
        let after = (p - LOADER_LAG * (index + 1) as f32) / LOADER_SPIN;
        let hop = bump((p - 0.66 - 0.11 * index as f32) / 0.2);
        let turn = LOADER_SPRING.at(after);
        let (sin, cos) = (std::f32::consts::TAU * turn).sin_cos();
        // Pulled in most on the far side of the bowl, wherever in time that is.
        let pull = bump(turn.clamp(0., 1.));
        let grow = 1. - 0.3 * pull + 0.35 * hop;
        // But never onto the bowl: a flake the colour of the bowl that
        // touches it reads as a lump on its rim, and one that goes behind it
        // is gone for a few frames.
        let (dx, dy) = (x - LOADER_BOWL.0, y - LOADER_BOWL.1);
        let clear = (LOADER_RING + LOADER_GAP + radius * grow) / dx.hypot(dy);
        let reach = (1. - 0.3 * pull).max(clear.min(1.)) + 0.13 * hop;
        let (dx, dy) = (dx * reach, dy * reach);
        (
            (
                LOADER_BOWL.0 + dx * cos - dy * sin,
                LOADER_BOWL.1 + dx * sin + dy * cos,
            ),
            grow,
        )
    });
    LoaderPose { bowl, flakes }
}

/// How far to shift the bowl's svg, in px, so that it turns and scales about
/// the middle of its own ring. GPUI turns and scales an svg about the middle
/// of its box and then shifts it (`p' = c + shift + R·S·(p − c)`), and the
/// ring's middle is beside the box's, so without this the bowl would swing
/// round the box's middle instead of spinning in place.
fn loader_bowl_shift(turn: f32, scale: f32, unit: f32) -> (f32, f32) {
    let (sin, cos) = (std::f32::consts::TAU * turn).sin_cos();
    let (dx, dy) = ((LOADER_BOWL.0 - 12.) * unit, (LOADER_BOWL.1 - 12.) * unit);
    (
        dx - scale * (dx * cos - dy * sin),
        dy - scale * (dx * sin + dy * cos),
    )
}

/// The Cereal mark as a loader, `size` px square: the bowl spins and the
/// flakes chase it and hop (see [`cereal_loader_pose`]). Reduced motion
/// shows the still mark.
pub fn cereal_loader(id: impl Into<ElementId>, size: f32, color: Hsla) -> AnyElement {
    if reduce_motion() {
        return icon("cereal", size, color).into_any_element();
    }
    let unit = size / 24.;
    div()
        .size(px(size))
        .flex_shrink_0()
        .relative()
        .with_animation(
            id,
            Animation::new(slow(LOADER_LOOP)).repeat(),
            move |frame, progress| {
                let pose = cereal_loader_pose(progress);
                let (turn, scale) = pose.bowl;
                let shift = loader_bowl_shift(turn, scale, unit);
                // The flakes are painted, not laid out: layout snaps to whole
                // pixels, which at this size shows as stepping and as dots
                // that change shape from frame to frame.
                let flakes = canvas(
                    |_, _, _| (),
                    move |bounds, _, window, _| {
                        let (left, top) = (f32::from(bounds.origin.x), f32::from(bounds.origin.y));
                        for (index, ((x, y), grow)) in pose.flakes.into_iter().enumerate() {
                            let radius = LOADER_FLAKES[index].1 * grow * unit;
                            window.paint_quad(
                                fill(
                                    Bounds::new(
                                        point(px(left + x * unit - radius), px(top + y * unit - radius)),
                                        gpui::size(px(2. * radius), px(2. * radius)),
                                    ),
                                    color,
                                )
                                .corner_radii(px(radius)),
                            );
                        }
                    },
                )
                .absolute()
                .inset_0();
                frame
                    .child(
                        svg()
                            .path("icons/cereal-bowl.svg")
                            .size(px(size))
                            .absolute()
                            .top_0()
                            .left_0()
                            .text_color(color)
                            .with_transformation(
                                Transformation::scale(size2(scale, scale))
                                    .with_translation(point(px(shift.0), px(shift.1)))
                                    .with_rotation(radians(std::f32::consts::TAU * turn)),
                            ),
                    )
                    .child(flakes)
            },
        )
        .into_any_element()
}

/// How far through its arrival something is, as its opacity and how far
/// below its place it is drawn. Reduced motion is the whole of
/// [`REDUCED_FADE`], in place.
fn arrive_pose(progress: f32, reduced: bool) -> (f32, f32) {
    if reduced {
        return (fade_curve(progress), 0.);
    }
    let value = GOOEY.at(progress);
    (fade_for(value), 12. * (1. - value))
}

/// Something arriving with a little weight: it fades in while it drops into
/// place on [`GOOEY`], `index` beats after the first of its group.
pub fn arrive<E: Styled + IntoElement + 'static>(
    id: impl Into<ElementId>,
    index: usize,
    element: E,
) -> AnyElement {
    let reduced = reduce_motion();
    let wait = if reduced { 0. } else { 0.045 * index as f32 };
    // In unscaled seconds; `slow` stretches the whole of it.
    let length = if reduced { REDUCED_FADE } else { Duration::from_millis(GOOEY.millis) }.as_secs_f32();
    let total = slow(Duration::from_secs_f32(wait + length));
    Morph::new(element)
        .with_animation(id, Animation::new(total), move |morph, t| {
            let (fade, rise) = arrive_pose(((t * (wait + length) - wait) / length).clamp(0., 1.), reduced);
            morph.opacity(fade).offset(0., rise)
        })
        .into_any_element()
}

/// How much of the highlight a glyph centred at `glyph_center` (0..=1 along
/// the label) carries when the band is `progress` of the way through a sweep:
/// 1 under the middle of the band, falling smoothly to 0 at its edges. The
/// band is `SHEEN_BAND` of the label wide, starts wholly before the label and
/// ends wholly past it, so the loop has no seam.
pub fn sheen_level(glyph_center: f32, progress: f32) -> f32 {
    let half = SHEEN_BAND / 2.;
    let centre = -half + (1. + SHEEN_BAND) * progress.clamp(0., 1.);
    let distance = ((glyph_center - centre) / half).abs();
    if distance >= 1. {
        return 0.;
    }
    // Smoothstep down from the centre.
    let t = 1. - distance;
    t * t * (3. - 2. * t)
}

/// Width of the moving highlight as a share of the label's width.
const SHEEN_BAND: f32 = 0.9;
/// One sweep of the highlight, and the rest before the next.
const SHEEN_SWEEP: Duration = Duration::from_millis(1500);
const SHEEN_REST: Duration = Duration::from_millis(350);

/// A status label in `base` ink with a band of `peak` ink sweeping across its
/// glyphs from the leading end: "Thinking…" and the live name of what is
/// being done. `size` is the text size it is drawn at. Reduced motion leaves
/// it still, in `base`.
pub fn sheen_label(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    size: f32,
    base: Hsla,
    peak: Hsla,
    cx: &App,
) -> SlowLoop<StyledText> {
    let label: SharedString = label.into();
    let font_id = cx.text_system().resolve_font(&font(FONT));
    let mut x = 0.;
    let mut glyphs: Vec<(std::ops::Range<usize>, f32)> = label
        .char_indices()
        .map(|(start, character)| {
            let advance = cx
                .text_system()
                .advance(font_id, px(size), character)
                .map(|size| f32::from(size.width))
                .unwrap_or(size / 2.);
            let centre = x + advance / 2.;
            x += advance;
            (start..start + character.len_utf8(), centre)
        })
        .collect();
    let width = x.max(1.);
    for (_, centre) in &mut glyphs {
        *centre /= width;
    }
    let period = SHEEN_SWEEP + SHEEN_REST;
    let sweep = SHEEN_SWEEP.as_secs_f32() / period.as_secs_f32();
    slow_loop(id, period, StyledText::new(label), move |text, turns| {
        let progress = (turns.fract() / sweep).min(1.);
        text.with_highlights(
            glyphs
                .iter()
                .map(|(range, centre)| {
                    (
                        range.clone(),
                        HighlightStyle {
                            color: Some(mix(base, peak, sheen_level(*centre, progress))),
                            ..Default::default()
                        },
                    )
                })
                .collect::<Vec<_>>(),
        )
    })
}

// ---------------------------------------------------------------------------
// Folds: a disclosure's body, growing and shrinking with its content
// ---------------------------------------------------------------------------

thread_local! {
    /// The natural height of each fold's content, as last laid out.
    static FOLDS: RefCell<HashMap<TrackKey, f32>> = RefCell::new(HashMap::new());
}

/// How far a fold's content travels as it opens (it settles downwards into
/// place) and closes.
const FOLD_TRAVEL: f32 = 6.;

/// The height a fold `level` open is drawn at when its content is `natural`
/// tall. Never above its content: a spring's overshoot would open a gap under
/// it and close it again.
pub fn fold_height(natural: f32, level: f32) -> f32 {
    (natural * level.clamp(0., 1.)).max(0.)
}

/// The body of a disclosure (an accordion): `content` clipped to the share of
/// its own height that `pose` has reached, fading and settling into place, so
/// whatever is below it slides instead of jumping. `pose` is the presence of
/// `key` on the view's stage ([`Stage::presence`]); `None` once it has
/// closed. The content is laid out at its natural height throughout and
/// measured each frame, so it may change (text arriving, a fold inside it
/// opening) while this one moves. At rest it is not clipped or sized at all.
pub fn fold(stage: &Stage, key: TrackKey, pose: Pose, content: impl IntoElement) -> Option<Div> {
    if !pose.visible {
        FOLDS.with(|folds| folds.borrow_mut().remove(&key));
        return None;
    }
    let probe = canvas(
        move |bounds, _, _| {
            FOLDS.with(|folds| folds.borrow_mut().insert(key, f32::from(bounds.size.height)));
        },
        |_, _, _, _| {},
    )
    .absolute()
    .inset_0();
    let inner = div()
        .relative()
        .w_full()
        .min_w_0()
        .flex_shrink_0()
        .flex()
        .flex_col()
        .child(content)
        .child(probe);
    let shell = div().w_full().min_w_0().flex().flex_col();
    if pose.at_rest() {
        return Some(shell.child(inner));
    }
    let level = pose.value.clamp(0., 1.);
    Some(match FOLDS.with(|folds| folds.borrow().get(&key).copied()) {
        Some(natural) => shell
            .h(px(fold_height(natural, level)))
            .overflow_hidden()
            .opacity(pose.fade)
            .child(inner.mt(px(-FOLD_TRAVEL * (1. - level)))),
        // Not laid out yet: this frame measures it, the next shows it.
        None => {
            stage.keep_alive();
            shell.h(px(0.)).overflow_hidden().opacity(0.).child(inner)
        }
    })
}

// ---------------------------------------------------------------------------
// Slow loops
// ---------------------------------------------------------------------------

thread_local! {
    /// When each view's next slow-loop frame is due: however many loops a
    /// view shows, they share one timer per [`BARS_FRAME`].
    static LOOP_FRAMES: RefCell<HashMap<EntityId, Instant>> = RefCell::new(HashMap::new());
}

/// How many whole and part turns of a `period` loop have passed after
/// `elapsed`. Pure, so the loop's timing can be tested.
pub fn loop_turns(elapsed: Duration, period: Duration) -> f32 {
    elapsed.as_secs_f32() / period.as_secs_f32().max(0.001)
}

/// A slow, indefinite effect (a shimmer, a skeleton's breathing, waiting
/// dots). These last for seconds or minutes, and every frame of them
/// re-renders the window, so they are stepped by a 33 ms timer instead of at
/// the display's rate: at 120 Hz that is a quarter of the renders for a
/// difference nobody can see in an effect this slow. The spinner, which does
/// read as stepping at 30 Hz, stays on display frames.
///
/// `animator` receives the turns completed since the element mounted (use
/// `fract()` for the position within a turn). With reduced motion it is
/// called once with 0 and nothing is scheduled.
pub fn slow_loop<E: IntoElement + 'static>(
    id: impl Into<ElementId>,
    period: Duration,
    element: E,
    animator: impl Fn(E, f32) -> E + 'static,
) -> SlowLoop<E> {
    SlowLoop {
        id: id.into(),
        period,
        element: Some(element),
        animator: Box::new(animator),
    }
}

pub struct SlowLoop<E> {
    id: ElementId,
    period: Duration,
    element: Option<E>,
    animator: Box<dyn Fn(E, f32) -> E>,
}

impl<E: IntoElement + 'static> IntoElement for SlowLoop<E> {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl<E: IntoElement + 'static> Element for SlowLoop<E> {
    type RequestLayoutState = AnyElement;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, AnyElement) {
        let now = Instant::now();
        let reduced = reduce_motion();
        let turns = match id {
            Some(id) if !reduced => {
                let started = window.with_element_state::<Instant, _>(id, |started, _| {
                    let started = started.unwrap_or(now);
                    (started, started)
                });
                loop_turns(now.saturating_duration_since(started), slow(self.period))
            }
            _ => 0.,
        };
        if !reduced {
            let view = window.current_view();
            let due = LOOP_FRAMES.with(|frames| {
                let mut frames = frames.borrow_mut();
                frames.retain(|_, next| *next + Duration::from_secs(1) > now);
                let due = frames.get(&view).is_none_or(|next| now >= *next);
                if due {
                    frames.insert(view, now + BARS_WAKE);
                }
                due
            });
            if due {
                redraw_at(window, now + BARS_WAKE, "loop");
            }
        }
        let mut element = self
            .element
            .take()
            .map(|element| (self.animator)(element, turns).into_any_element())
            .unwrap_or_else(|| div().into_any_element());
        let layout = element.request_layout(window, cx);
        (layout, element)
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
        element.paint(window, cx);
    }
}

fn css_ease_in_out(t: f32) -> f32 {
    cubic_bezier(t, 0.42, 0., 0.58, 1.)
}

fn shimmer_opacity(glyph_center: f32, progress: f32) -> f32 {
    // A 250%-wide repeating gradient travelling 250% → -250%, with 20% ink at
    // either edge and full ink in its centre.
    let gradient_position = ((glyph_center + 3.75 - 7.5 * progress) / 2.5).rem_euclid(1.);
    0.2 + 0.8 * (1. - (2. * gradient_position - 1.).abs())
}

/// A CSS `cubic-bezier(x1, y1, x2, y2)` timing function at time `t`.
pub fn cubic_bezier(t: f32, x1: f64, y1: f64, x2: f64, y2: f64) -> f32 {
    if !t.is_finite() || t <= 0. {
        return 0.;
    }
    if t >= 1. {
        return 1.;
    }
    let coordinate = |u: f64, first: f64, second: f64| {
        let inverse = 1. - u;
        3. * inverse * inverse * u * first + 3. * inverse * u * u * second + u * u * u
    };
    // CSS easing takes time on the x axis, not the Bezier's parameter u. The
    // curve has monotonic x, so bounded bisection is stable even close to its
    // almost-flat endpoints.
    let (mut low, mut high) = (0., 1.);
    for _ in 0..26 {
        let middle = (low + high) * 0.5;
        if coordinate(middle, x1, x2) < f64::from(t) {
            low = middle;
        } else {
            high = middle;
        }
    }
    coordinate((low + high) * 0.5, y1, y2) as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    // `gpui::*` re-exports its own `test` attribute; these are plain unit tests.
    use std::prelude::v1::test;

    #[test]
    fn the_sheen_is_a_band_that_enters_crosses_and_leaves_the_label() {
        // Before it enters and after it leaves, nothing is lit: no seam.
        for centre in [0., 0.3, 0.7, 1.] {
            assert_eq!(sheen_level(centre, 0.), 0.);
            assert_eq!(sheen_level(centre, 1.), 0.);
        }
        // Mid-sweep the middle is brightest and both ends are dimmer, so part
        // of the word is lit rather than all of it.
        let (start, middle, end) = (sheen_level(0., 0.5), sheen_level(0.5, 0.5), sheen_level(1., 0.5));
        assert!((middle - 1.).abs() < 1e-5 && start < 0.2 && end < 0.2, "{start} {middle} {end}");
        // It travels from the leading end to the trailing end.
        assert!(sheen_level(0.1, 0.3) > sheen_level(0.9, 0.3));
        assert!(sheen_level(0.9, 0.7) > sheen_level(0.1, 0.7));
        for step in 0..=100 {
            let level = sheen_level(0.4, step as f32 / 100.);
            assert!((0. ..=1.).contains(&level));
        }
    }

    #[test]
    fn the_thinking_mark_loops_without_a_seam() {
        let (turn_start, scale_start, ink_start) = thinking_mark_pose(0.);
        let (turn_end, scale_end, ink_end) = thinking_mark_pose(1.);
        assert_eq!((turn_start, turn_end), (0., 1.));
        assert!((scale_start - scale_end).abs() < 1e-5 && (ink_start - ink_end).abs() < 1e-5);
        let mut last = 0.;
        for step in 0..=120 {
            let (turn, scale, ink) = thinking_mark_pose(step as f32 / 120.);
            assert!(turn + 1e-6 >= last, "it never turns back");
            assert!((0.85..=1.).contains(&scale) && (0.6..=1.).contains(&ink));
            last = turn;
        }
    }

    #[test]
    fn a_fold_is_never_taller_than_its_content_or_below_nothing() {
        assert_eq!(fold_height(120., 0.), 0.);
        assert_eq!(fold_height(120., 0.5), 60.);
        assert_eq!(fold_height(120., 1.), 120.);
        // A spring's overshoot does not open a gap under the content.
        assert_eq!(fold_height(120., 1.08), 120.);
        assert_eq!(fold_height(120., -0.2), 0.);
        // A closed fold whose presence has ended is not built at all.
        let stage = Stage::default();
        let closed = Pose { value: 0., fade: 0., visible: false, settled: true };
        assert!(fold(&stage, ("test-fold", 1), closed, gpui::div()).is_none());
        assert!(fold(&stage, ("test-fold", 1), Pose::REST, gpui::div()).is_some());
    }

    const TOKENS: [Spring; 3] = [SNAPPY, GOOEY, SETTLE];

    #[test]
    fn springs_start_at_zero_and_settle_on_exactly_one() {
        for spring in TOKENS {
            assert_eq!(spring.at(0.), 0.);
            assert_eq!(spring.at(1.), 1.);
            assert_eq!(spring.at(-0.5), 0.);
            assert_eq!(spring.at(1.5), 1.);
            assert_eq!(spring.at(f32::NAN), 0.);
            // No visible snap when the fixed duration ends.
            assert!((spring.at(0.999) - 1.).abs() < 0.002);
            // Starts from rest rather than jumping.
            assert!(spring.at(0.001) < 0.01);
        }
    }

    fn ask_pose(value: f32) -> Pose {
        Pose {
            value,
            fade: fade_for(value),
            visible: true,
            settled: false,
        }
    }

    #[test]
    fn the_open_note_chat_has_taken_in_the_record_capsule() {
        assert_eq!(capsule_absorb(0.), 0.);
        assert_eq!(capsule_absorb(0.3), 0.);
        assert_eq!(capsule_absorb(0.9), 1.);
        assert_eq!(capsule_absorb(1.08), 1.);
        assert_eq!(capsule_absorb(f32::NAN), 1.);
        let mut last = 0.;
        for step in 0..=100 {
            let now = capsule_absorb(step as f32 / 100.);
            assert!(now >= last);
            last = now;
        }
    }

    #[test]
    fn the_ask_tray_starts_as_the_pills_own_outline_and_ends_as_the_tray() {
        // Closed, and on the first frame of opening, the tray is exactly
        // the pill: nothing can flash into view around it.
        for rest in [
            ask_shape(Pose { visible: false, ..ask_pose(0.7) }),
            ask_shape(ask_pose(0.)),
            ask_shape(ask_pose(f32::NAN)),
            ask_shape(ask_pose(-0.2)),
        ] {
            assert_eq!((rest.out, rest.head), (0., 0.));
            assert_eq!(rest.radius, ASK_PILL / 2.);
            assert_eq!((rest.ring, rest.ring_fade, rest.shadow, rest.history), (0., 0., 0., 0.));
        }
        let open = ask_shape(Pose::REST);
        assert_eq!((open.out, open.head), (ASK_INSET, ASK_HEAD));
        assert_eq!(open.radius, ASK_FRAME_RADIUS);
        assert_eq!((open.ring, open.ring_fade, open.opacity, open.shadow), (ASK_RING, 1., 1., 1.));
        assert_eq!((open.history, open.history_drop), (1., 0.));
        // The frame's corners are concentric with the pill's.
        assert_eq!(ASK_FRAME_RADIUS - ASK_INSET, ASK_PILL / 2.);
    }

    #[test]
    fn the_ask_tray_overshoots_outwards_only() {
        assert!(
            (0.06..=0.12).contains(&ASK_TRAY.overshoot()),
            "{}",
            ASK_TRAY.overshoot()
        );
        let mut peak = 0f32;
        let mut last = ask_shape(ask_pose(0.));
        for tick in 0..=1000 {
            let level = ASK_TRAY.at(tick as f32 / 1000.);
            let shape = ask_shape(ask_pose(level));
            peak = peak.max(shape.head);
            // The tray may swell past its size; what is drawn inside the
            // pill, and the corner radius, never pass theirs, so the halo
            // stays clear of the text and the corners stay round.
            assert!((0. ..=ASK_RING).contains(&shape.ring));
            assert!((ASK_PILL / 2. ..=ASK_FRAME_RADIUS).contains(&shape.radius));
            assert!((0. ..=1.).contains(&shape.ring_fade));
            assert!((0. ..=1.).contains(&shape.history));
            // Solid throughout: it grows out of a pill of its own colour.
            assert_eq!(shape.opacity, 1.);
            // No jump between consecutive samples (a 340 ms spring sampled
            // every 0.34 ms).
            assert!((shape.head - last.head).abs() < 0.5, "{tick}");
            assert!((shape.ring - last.ring).abs() < 0.05, "{tick}");
            assert!((shape.history - last.history).abs() < 0.03, "{tick}");
            last = shape;
        }
        assert!(peak > ASK_HEAD * 1.05 && peak < ASK_HEAD * 1.13, "{peak}");
        assert_eq!(last, ask_shape(Pose::REST).clone());
        // The recent-chats button arrives late and is there at the end.
        assert_eq!(ask_shape(ask_pose(0.45)).history, 0.);
        assert!(ask_shape(ask_pose(0.8)).history > 0.5);
    }

    #[test]
    fn with_reduced_motion_the_ask_tray_fades_at_full_size() {
        let mut track = Track::rest(0., Instant::now());
        let start = Instant::now();
        track.present(true, ASK_TRAY, start, true);
        let half = ask_shape(track.pose(start + REDUCED_FADE / 2, true));
        assert_eq!((half.out, half.head, half.ring), (ASK_INSET, ASK_HEAD, ASK_RING));
        assert_eq!(half.radius, ASK_FRAME_RADIUS);
        assert!(half.opacity > 0. && half.opacity < 1.);
        assert_eq!(half.opacity, half.ring_fade);
        assert_eq!(half.history_drop, 0.);
        let done = ask_shape(track.pose(start + REDUCED_FADE, true));
        assert_eq!(done, ask_shape(Pose::REST));
        // And back: closed means gone, not left part-open.
        track.present(false, ASK_TRAY, start + REDUCED_FADE, true);
        let gone = track.pose(start + REDUCED_FADE * 2, true);
        assert!(!gone.visible);
        assert_eq!(ask_shape(gone).opacity, 0.);
    }

    #[test]
    fn an_interrupted_ask_tray_carries_on_from_where_it_was() {
        let start = Instant::now();
        let mut track = Track::rest(0., start);
        let mut now = start;
        let mut last = ask_shape(track.pose(now, false)).head;
        // Ten changes of mind 45 ms apart, sampled every millisecond.
        for flip in 0..10 {
            track.present(flip % 2 == 0, ASK_TRAY, now, false);
            for _ in 0..45 {
                now += Duration::from_millis(1);
                let head = ask_shape(track.pose(now, false)).head;
                assert!((head - last).abs() < 1.6, "jumped {last} -> {head}");
                last = head;
            }
        }
        // Left alone it closes completely and its track can be dropped.
        now += ASK_TRAY.duration();
        let pose = track.pose(now, false);
        assert!(pose.settled && !pose.visible);
        assert_eq!(ask_shape(pose), ask_shape(ask_pose(0.)));
    }

    #[test]
    fn a_notes_two_capsules_join_from_the_middle_and_part_the_same_way() {
        // Apart at rest: the gap reaches past the row's middle from both
        // edges, so nothing joins them.
        assert_eq!(bridge_level(0.), 0.);
        assert!(bridge_reach(0., 56.) > 28.);
        // Joined well before the panel is open, and for good once it is.
        assert_eq!(bridge_level(0.45), 1.);
        assert_eq!(bridge_reach(bridge_level(1.08), 56.), 0.);
        assert_eq!(bridge_reach(bridge_level(1.), 56.), 0.);
        // Closing evenly, never jumping, and the same way back.
        let mut last = bridge_reach(0., 56.);
        for tick in 1..=200 {
            let reach = bridge_reach(bridge_level(tick as f32 / 200.), 56.);
            assert!(reach <= last && last - reach < 1.5, "{tick}: {last} -> {reach}");
            last = reach;
        }
        assert_eq!(bridge_reach(bridge_level(f32::NAN), 56.), 0.);
    }

    #[test]
    fn the_chat_panel_grows_from_the_pill_or_from_the_tray() {
        let panel = (683., 467.);
        // From the bar at rest: the frame starts as the pill's outline,
        // an inset in from the panel on each side and above its bottom edge.
        let pill = ask_unfold(0., ASK_PILL, panel);
        assert!((pill.scale.0 * panel.0 - (panel.0 - 2. * ASK_INSET)).abs() < 1e-3);
        assert!((pill.scale.1 * panel.1 - ASK_PILL).abs() < 1e-3);
        assert_eq!((pill.anchor, pill.rise, pill.travel), ((0.5, 1.), -ASK_INSET, 0.));
        // From the focused bar: the frame starts as the tray, which already
        // has the panel's width and bottom edge.
        let tray = ask_unfold(1., ASK_PILL, panel);
        assert_eq!((tray.scale.0, tray.rise), (1., 0.));
        assert!((tray.scale.1 * panel.1 - (ASK_PILL + ASK_INSET + ASK_HEAD)).abs() < 1e-3);
        let open = ask_shape(Pose::REST);
        assert_eq!(tray.scale.1 * panel.1, ASK_PILL + open.out + open.head);
        // Out-of-range feet and degenerate panels stay drawable.
        assert_eq!(ask_unfold(7., ASK_PILL, panel), tray);
        assert_eq!(ask_unfold(f32::NAN, ASK_PILL, panel), pill);
        let tiny = ask_unfold(1., ASK_PILL, (0., 0.));
        assert!(tiny.scale.0 > 0. && tiny.scale.1 <= 1.);
    }

    #[test]
    fn springs_overshoot_once_within_their_token_bounds() {
        for (spring, least, most) in [
            (SNAPPY, 0.004, 0.02),
            (GOOEY, 0.04, 0.08),
            (SETTLE, 0.008, 0.025),
        ] {
            let samples: Vec<f32> = (0..=2000)
                .map(|tick| spring.at(tick as f32 / 2000.))
                .collect();
            let peak = samples.iter().copied().fold(0., f32::max);
            assert!(
                (1. + least..=1. + most).contains(&peak),
                "{spring:?} peaked at {peak}"
            );
            assert!((peak - 1. - spring.overshoot()).abs() < 0.003);
            let peak_at = samples.iter().position(|value| *value == peak).unwrap();
            // Monotonic rise to the single peak…
            for pair in samples[..=peak_at].windows(2) {
                assert!(pair[1] + 1e-6 >= pair[0]);
            }
            // …then a soft settle that never dips visibly below the target.
            for value in &samples[peak_at..] {
                assert!(*value > 0.99 && *value <= peak);
            }
        }
        // The tokens are ordered from tight to loose.
        assert!(SNAPPY.overshoot() < SETTLE.overshoot());
        assert!(SETTLE.overshoot() < GOOEY.overshoot());
    }

    #[test]
    fn exits_are_shorter_monotonic_and_never_bounce() {
        for curve in [exit_curve as fn(f32) -> f32, fade_curve, fade_for] {
            assert_eq!(curve(0.), 0.);
            assert_eq!(curve(1.), 1.);
            assert_eq!(curve(-1.), 0.);
            assert_eq!(curve(2.), 1.);
            let mut previous = 0.;
            for tick in 0..=1000 {
                let value = curve(tick as f32 / 1000.);
                assert!((0. ..=1.).contains(&value));
                assert!(value >= previous);
                previous = value;
            }
        }
        for spring in TOKENS {
            let ratio = spring.exit_duration().as_secs_f32() / spring.duration().as_secs_f32();
            assert!((0.6..=0.7).contains(&ratio));
        }
        // Most of an exit is over early: nothing lingers on screen.
        assert!(exit_curve(0.5) > 0.85);
    }

    #[test]
    fn stagger_is_a_few_milliseconds_per_item_and_capped() {
        assert_eq!(stagger_delay(0), Duration::ZERO);
        assert!(stagger_delay(1) <= Duration::from_millis(30));
        assert!(stagger_delay(3) > stagger_delay(2));
        assert_eq!(stagger_delay(STAGGER_CAP), stagger_delay(500));
        assert!(stagger_delay(500) + SNAPPY.duration() < Duration::from_millis(450));
    }

    #[test]
    fn pages_arrive_briefly_without_overshoot_and_never_from_transparent() {
        assert!(PAGE_FADE <= Duration::from_millis(250));
        let mut previous = (0., f32::MAX);
        for tick in 0..=200 {
            let (opacity, rise) = page_pose(tick as f32 / 200., false);
            // Navigation never dips to the bare background…
            assert!((PAGE_FROM..=1.).contains(&opacity));
            // …and the page only ever moves towards its place.
            assert!(opacity >= previous.0 && rise <= previous.1);
            assert!((0. ..=PAGE_TRAVEL).contains(&rise));
            previous = (opacity, rise);
        }
        assert_eq!(page_pose(0., false), (PAGE_FROM, PAGE_TRAVEL));
        assert_eq!(page_pose(1., false), (1., 0.));
        assert!(PAGE_TRAVEL <= 4.);
        // Reduced motion and in-place swaps do not move at all.
        for tick in 0..=20 {
            assert_eq!(page_pose(tick as f32 / 20., true).1, 0.);
        }
    }

    #[test]
    fn large_panels_overshoot_a_quarter_as_much_as_menus() {
        assert_eq!(PANEL.millis, GOOEY.millis);
        assert!(PANEL.overshoot() < GOOEY.overshoot() / 3.);
        // A transcript unfolding from its 59 px bar to 465 px passes its
        // resting edge by a few pixels, not twenty.
        let travel = 465. - 59.;
        assert!(travel * PANEL.overshoot() < 7.);
        assert!(travel * GOOEY.overshoot() > 20.);
    }

    #[test]
    fn morph_content_stays_inside_the_arc_and_arrives_behind_the_shell() {
        // Nothing while the shell is a sliver, everything well before rest.
        assert_eq!(content_fade(0.), 0.);
        assert_eq!(content_fade(0.35), 0.);
        assert!(content_fade(0.8) > 0.999);
        assert_eq!(content_fade(1.05), 1.);
        let mut previous = 0.;
        for tick in 0..=100 {
            let value = content_fade(tick as f32 / 100.);
            assert!((0. ..=1.).contains(&value) && value >= previous);
            previous = value;
        }
        // Collapsed, a 32 px corner needs about 9 px to clear its arc.
        let collapsed = corner_inset(32., (1., 0.13));
        assert!((9. ..9.6).contains(&collapsed));
        // The inset is gone at rest, so nothing pops when the mask is dropped.
        assert_eq!(corner_inset(32., (1., 1.)), 0.);
        assert!(corner_inset(32., (1., 0.99)) < 1.2);
        // A shell overshooting past its size is larger than its content, so
        // nothing needs pulling in; nor does a square shell.
        assert_eq!(corner_inset(32., (1., 1.04)), 0.);
        assert_eq!(corner_inset(0., (0.5, 0.5)), 0.);
    }

    #[test]
    fn cascade_items_wait_their_turn_then_rise_and_rest() {
        // Before its turn an item is invisible and tucked below its slot.
        let waiting = cascade_pose(Duration::from_millis(10), 3, false);
        assert_eq!((waiting.value, waiting.fade), (0., 0.));
        // The first item is already moving.
        let first = cascade_pose(Duration::from_millis(10), 0, false);
        assert!(first.value > 0. && !first.settled);
        // Every item, however deep in the list, is at rest when the cascade ends.
        let end = stagger_delay(STAGGER_CAP) + SNAPPY.duration();
        for index in [0, 1, 7, 400] {
            assert_eq!(cascade_pose(end, index, false), Pose::REST);
        }
        // Reduced motion: no stagger and no travel, just the short fade.
        let reduced = cascade_pose(Duration::from_millis(10), 400, true);
        assert_eq!(reduced.value, 1.);
        assert!(reduced.fade > 0. && reduced.fade < 1.);
        assert_eq!(cascade_pose(REDUCED_FADE, 400, true).fade, 1.);
    }

    #[test]
    fn stretch_leads_with_the_travelling_edge_and_lands_exactly() {
        let (from, to) = ((10., 38.), (126., 154.));
        assert_eq!(stretch(from, to, 0.), from);
        assert_eq!(stretch(from, to, 1.), to);
        assert_eq!(stretch(to, from, 1.), from);
        let mut longest: f32 = 0.;
        for tick in 0..=1000 {
            let t = tick as f32 / 1000.;
            let (low, high) = stretch(from, to, t);
            let length = high - low;
            assert!(length >= 28. * MIN_SQUASH - 0.001, "{t}: {length}");
            assert!(length <= 28. * MAX_STRETCH + 0.001, "{t}: {length}");
            // The droplet never leaves the corridor by more than a nudge.
            assert!(low >= 10. - 0.001 && high <= 154. + 28. * 0.1);
            longest = longest.max(length);
            // Mirrored travel is a mirror image.
            let (mirror_low, mirror_high) = stretch((-38., -10.), (-154., -126.), t);
            assert!((mirror_low + high).abs() < 0.01 && (mirror_high + low).abs() < 0.01);
        }
        // It visibly stretches on the way…
        assert!(longest > 28. * 1.6);
        // …with the leading edge ahead while the trailing edge still waits.
        let (low, high) = stretch(from, to, TRAIL_DELAY);
        assert!((high - low - 28. * MAX_STRETCH).abs() < 0.001);
        assert!(high > 80.);
    }

    #[test]
    fn stretch_without_travel_only_resizes_in_place() {
        let (from, to) = ((20., 120.), (10., 130.));
        for tick in 0..=100 {
            let (low, high) = stretch(from, to, tick as f32 / 100.);
            assert!(((low + high) / 2. - 70.).abs() < 0.01);
            assert!(high - low >= 100. - 0.001 && high - low <= 120. * (1. + GOOEY.overshoot()));
        }
    }

    #[test]
    fn retargeting_a_track_mid_flight_never_jumps() {
        let start = Instant::now();
        let mut track = Track::rest(0., start);
        track.present(true, GOOEY, start, false);
        let interrupted = start + Duration::from_millis(60);
        let before = track.value(interrupted);
        assert!(before > 0.05 && before < 1.);
        track.present(false, GOOEY, interrupted, false);
        assert!((track.value(interrupted) - before).abs() < 1e-6);
        // The exit is monotonic from where it was interrupted and unmounts.
        let mut previous = before;
        for millis in 1..=GOOEY.exit_duration().as_millis() as u64 {
            let value = track.value(interrupted + Duration::from_millis(millis));
            assert!(value <= previous + 1e-6 && value >= 0.);
            previous = value;
        }
        let done = interrupted + GOOEY.exit_duration();
        assert_eq!(track.value(done), 0.);
        assert!(!track.pose(done, false).visible);
        // Reopening half-way through the exit continues from there too.
        let reopened = interrupted + Duration::from_millis(40);
        let partial = track.value(reopened);
        track.present(true, GOOEY, reopened, false);
        assert!((track.value(reopened) - partial).abs() < 1e-6);
        assert_eq!(track.value(reopened + GOOEY.duration()), 1.);
    }

    #[test]
    fn presence_pose_fades_without_flashing_and_overshoots_only_while_entering() {
        let start = Instant::now();
        let mut track = Track::rest(0., start);
        track.present(true, GOOEY, start, false);
        let mut peak: f32 = 0.;
        let mut previous_fade = 0.;
        for millis in 0..=GOOEY.millis {
            let pose = track.pose(start + Duration::from_millis(millis), false);
            assert!(pose.visible);
            assert!((0. ..=1.).contains(&pose.fade));
            assert!(pose.fade + 1e-6 >= previous_fade);
            previous_fade = pose.fade;
            peak = peak.max(pose.value);
        }
        assert!(peak > 1.03 && peak < 1.08);
        assert_eq!(track.pose(start + GOOEY.duration(), false), Pose::REST);
        // Fully opaque long before the shape has finished settling.
        assert_eq!(
            track.pose(start + Duration::from_millis(120), false).fade,
            1.
        );
    }

    #[test]
    fn reduced_motion_is_a_short_fade_with_no_overshoot_stretch_or_scale() {
        let start = Instant::now();
        let mut track = Track::rest(0., start);
        track.present(true, SETTLE, start, true);
        let mut previous = 0.;
        for millis in 0..=REDUCED_FADE.as_millis() as u64 {
            let pose = track.pose(start + Duration::from_millis(millis), true);
            // The morph level stays at rest: nothing scales, slides or stretches.
            assert_eq!(pose.value, 1.);
            assert!((0. ..=1.).contains(&pose.fade) && pose.fade >= previous);
            previous = pose.fade;
        }
        assert!(track.settled(start + REDUCED_FADE));
        assert_eq!(track.pose(start + REDUCED_FADE, true), Pose::REST);
        assert!(REDUCED_FADE < SNAPPY.exit_duration());

        // Leaving is the same short fade and then unmounts.
        track.present(false, SETTLE, start + REDUCED_FADE, true);
        let gone = track.pose(start + REDUCED_FADE * 2, true);
        assert_eq!((gone.value, gone.fade, gone.visible), (1., 0., false));

        // Plain values snap instead of springing.
        let mut width = Track::rest(58., start);
        width.spring_to(137., GOOEY, start, true);
        assert_eq!(width.value(start), 137.);
        assert!(width.settled(start));

        // Mounted entrances are opacity-only as well.
        for tick in 0..=100 {
            let pose = mount_pose(GOOEY, tick as f32 / 100., true);
            assert_eq!(pose.value, 1.);
            assert!((0. ..=1.).contains(&pose.fade));
        }
        assert_eq!(mount_pose(GOOEY, 1., true).fade, 1.);
        assert_eq!(mount_pose(GOOEY, 0.5, true).fade, 1.);

        // A moving highlight snaps to its new item; appearing, it only fades.
        let mut glide = GlideState::default();
        let row = |y: f32| Bounds::new(point(px(8.), px(y)), size(px(176.), px(28.)));
        glide.fresh = Some((1, row(60.)));
        assert_eq!(glide.advance(start, true), Some((row(60.), 0.)));
        let shown = start + HOVER_IN;
        glide.fresh = Some((1, row(60.)));
        assert_eq!(glide.advance(shown, true), Some((row(60.), 1.)));
        glide.fresh = Some((2, row(147.)));
        assert_eq!(glide.advance(shown, true), Some((row(147.), 1.)));
        assert!(glide.settled(shown));
    }

    #[test]
    fn values_spring_between_states_and_first_use_rests_on_target() {
        let start = Instant::now();
        let mut width = Track::rest(58., start);
        assert_eq!(width.value(start), 58.);
        assert!(width.settled(start));
        width.spring_to(137., GOOEY, start, false);
        let peak = (0..=GOOEY.millis)
            .map(|millis| width.value(start + Duration::from_millis(millis)))
            .fold(0., f32::max);
        assert!(peak > 137. && peak < 137. + 79. * 0.08);
        assert_eq!(width.value(start + GOOEY.duration()), 137.);
    }

    #[test]
    fn pop_grows_from_its_anchor_and_rests_untouched() {
        let menu = Pop::menu((1., 0.));
        let bounds = Bounds::new(point(px(100.), px(40.)), size(px(200.), px(300.)));
        let collapsed = Morph::new(div()).posed(
            menu,
            Pose {
                value: 0.,
                fade: 0.,
                visible: true,
                settled: false,
            },
        );
        let shell = collapsed.shell_bounds(bounds);
        // Pinned to the top-right corner it grows out of.
        assert_eq!(shell.top(), bounds.top());
        assert!((shell.right() - bounds.right()).abs() < px(0.001));
        assert!(shell.size.width < bounds.size.width && shell.size.height < bounds.size.height);
        // Content starts tucked towards the anchor.
        assert!(collapsed.offset.x > px(0.) && collapsed.offset.y < px(0.));

        let rest = Morph::new(div()).posed(menu, Pose::REST);
        assert_eq!(rest.shell_bounds(bounds), bounds);
        assert_eq!(rest.offset, point(px(0.), px(0.)));
        assert_eq!(rest.scale, (1., 1.));
        assert!(Pose::REST.at_rest());
    }

    #[test]
    fn highlight_glides_from_where_it_is_and_chases_a_moving_target() {
        let start = Instant::now();
        let row = |x: f32, y: f32| Bounds::new(point(px(x), px(y)), size(px(176.), px(28.)));
        let rect = |painted: Option<(Bounds<Pixels>, f32)>| painted.map(|(rect, _)| rect);
        let mut glide = GlideState::default();
        // First appearance has nothing to glide from: it eases in in place.
        glide.fresh = Some((1, row(8., 60.)));
        assert_eq!(glide.advance(start, false), Some((row(8., 60.), 0.)));
        assert!(!glide.settled(start));
        let faded_in = start + HOVER_IN;
        glide.fresh = Some((1, row(8., 60.)));
        assert_eq!(glide.advance(faded_in, false), Some((row(8., 60.), 1.)));
        assert!(glide.settled(faded_in));
        let start = faded_in;

        glide.fresh = Some((2, row(8., 147.)));
        assert_eq!(glide.advance(start, false), Some((row(8., 60.), 1.)));
        assert!(!glide.settled(start));
        let middle = start + Duration::from_millis(70);
        glide.fresh = Some((2, row(8., 147.)));
        let stretched = rect(glide.advance(middle, false)).unwrap();
        assert!(stretched.size.height > px(40.));
        assert_eq!(stretched.size.width, px(176.));
        assert!(stretched.top() >= px(60.) && stretched.bottom() <= px(180.));

        // The target moves mid-flight (its tab is still widening): the droplet
        // keeps heading for wherever the item is now, without restarting.
        glide.fresh = Some((2, row(-40., 147.)));
        let chasing = rect(glide.advance(middle, false)).unwrap();
        assert!(chasing.left() < px(8.) && chasing.left() >= px(-40.));
        assert_eq!(chasing.size.height, stretched.size.height);

        // Retargeting starts from the stretched shape, not from an endpoint.
        glide.fresh = Some((1, row(-40., 60.)));
        assert_eq!(rect(glide.advance(middle, false)), Some(chasing));

        let done = middle + GOOEY.duration();
        glide.fresh = Some((1, row(-40., 60.)));
        assert_eq!(glide.advance(done, false), Some((row(-40., 60.), 1.)));
        assert!(glide.settled(done));

        // Losing the selection fades the highlight out where it was, and
        // forgets the position instead of flying in later.
        let (fading, alpha) = glide.advance(done, false).unwrap();
        assert_eq!(fading, row(-40., 60.));
        assert_eq!(alpha, 1.);
        assert!(!glide.settled(done));
        let half = done + HOVER_OUT / 2;
        let (_, alpha) = glide.advance(half, false).unwrap();
        assert!(alpha > 0. && alpha < 1.);
        // A new selection while it fades eases in from that opacity, in place.
        glide.fresh = Some((3, row(8., 200.)));
        let (back, from) = glide.advance(half, false).unwrap();
        assert_eq!(back, row(8., 200.));
        assert!((from - alpha).abs() < 1e-6);
        glide.fresh = Some((3, row(8., 200.)));
        assert_eq!(
            glide.advance(half + HOVER_IN, false),
            Some((row(8., 200.), 1.))
        );
        let gone = half + HOVER_IN + HOVER_OUT;
        glide.advance(half + HOVER_IN, false);
        assert_eq!(glide.advance(gone, false), None);
        assert!(glide.settled(gone));
        glide.fresh = Some((4, row(8., 240.)));
        assert_eq!(rect(glide.advance(gone, false)), Some(row(8., 240.)));
    }

    #[test]
    fn frames_are_evaluated_one_refresh_ahead_so_the_first_one_moves() {
        // A highlight move: with the lead the first frame has already left.
        let row = |y: f32| Bounds::new(point(px(8.), px(y)), size(px(176.), px(28.)));
        let start = Instant::now();
        let mut glide = GlideState {
            lead: FRAME_LEAD,
            ..GlideState::default()
        };
        glide.fresh = Some((1, row(60.)));
        glide.advance(start, false);
        let later = start + Duration::from_secs(1);
        glide.fresh = Some((1, row(60.)));
        glide.advance(later, false);
        glide.fresh = Some((2, row(147.)));
        let (first, _) = glide.advance(later, false).unwrap();
        assert!(first.bottom() > row(60.).bottom());
        assert!(first.top() <= row(147.).top());
        // It still lands exactly, one lead before the token ends.
        glide.fresh = Some((2, row(147.)));
        let end = later + GOOEY.duration() - FRAME_LEAD;
        assert_eq!(glide.advance(end, false), Some((row(147.), 1.)));
        assert!(glide.settled(end));

        // GPUI animations skip the same share, and map onto the whole curve.
        let (animation, skipped) = led_animation(slow(PAGE_FADE));
        assert_eq!(animation.duration, slow(PAGE_FADE) - FRAME_LEAD);
        assert!(
            (led(skipped, 0.) - FRAME_LEAD.as_secs_f32() / PAGE_FADE.as_secs_f32()).abs() < 1e-6
        );
        assert_eq!(led(skipped, 1.), 1.);
        assert!(page_pose(led(skipped, 0.), false).0 > PAGE_FROM);
    }

    #[test]
    fn revealed_controls_follow_their_groups_published_hover_level() {
        let key = reveal_key(("row", 7u64));
        assert_ne!(key, reveal_key(("row", 8u64)));
        // Nothing published: hidden, and what it replaces is fully shown.
        assert_eq!(revealed(key), 0.);
        // A changed level reports itself so the control gets a frame to land.
        assert!(publish(key, 0.4));
        assert!(!publish(key, 0.4));
        assert_eq!(revealed(key), 0.4);
        // Another row is unaffected: a sweep never lights up its neighbours.
        assert_eq!(revealed(reveal_key(("row", 8u64))), 0.);
        // The next frame still reads it (it was published during the last
        // paint); a frame later, with nothing republishing, it has lapsed.
        frame();
        assert_eq!(revealed(key), 0.4);
        frame();
        assert_eq!(revealed(key), 0.);
        assert!(publish(key, 1.));
        assert!(publish(key, 0.));
        assert_eq!(revealed(key), 0.);
    }

    #[test]
    fn only_something_watched_without_a_break_counts_as_continuous() {
        let stage = Stage::default();
        let key = ("note-view", 3);
        // First sight is not a continuation of anything.
        assert!(!stage.continuous(key));
        // Same frame and the next one are.
        assert!(stage.continuous(key));
        frame();
        assert!(stage.continuous(key));
        // Two frames in which it was not rendered: the user looked away.
        frame();
        frame();
        assert!(!stage.continuous(key));
        // A dropped track is closed at once, with no exit to play.
        stage.presence(("chip", 0), true, GOOEY);
        assert_eq!(stage.target(("chip", 0)), Some(1.));
        stage.forget(("chip", 0));
        assert_eq!(stage.target(("chip", 0)), None);
    }

    #[test]
    fn slow_loops_count_turns_from_when_they_mounted() {
        let period = Duration::from_secs(2);
        assert_eq!(loop_turns(Duration::ZERO, period), 0.);
        assert!((loop_turns(Duration::from_millis(500), period) - 0.25).abs() < 1e-6);
        // Turns keep counting, so a staggered effect can tell its first turn
        // from later ones; the position within a turn is the fraction.
        let turns = loop_turns(Duration::from_millis(4500), period);
        assert!((turns - 2.25).abs() < 1e-6 && (turns.fract() - 0.25).abs() < 1e-6);
        // One step every 33 ms is at most 31 renders a second.
        assert!(Duration::from_secs(1).as_millis() / BARS_FRAME.as_millis() <= 31);
    }

    #[test]
    fn frame_percentiles_are_taken_from_the_sorted_intervals() {
        let mut none: [f32; 0] = [];
        assert_eq!(stats::percentiles(&mut none), (0., 0., 0.));
        // Nineteen frames on time and one that took two refreshes.
        let mut frames = [16.7_f32; 20];
        frames[7] = 33.4;
        let (median, p95, max) = stats::percentiles(&mut frames);
        assert_eq!(median, 16.7);
        assert_eq!(p95, 33.4);
        assert_eq!(max, 33.4);
    }

    #[test]
    fn hover_text_blends_between_its_two_colours() {
        let (from, to) = (Hsla::from(rgb(0x000000)), Hsla::from(rgb(0xffffff)));
        let half = Rgba::from(mix(from, to, 0.5));
        assert!((half.r - 0.5).abs() < 0.01 && (half.a - 1.).abs() < 0.01);
        assert_eq!(Rgba::from(mix(from, to, 0.)), Rgba::from(from));
        assert_eq!(Rgba::from(mix(from, to, 7.)), Rgba::from(to));
    }

    #[test]
    fn press_squash_is_a_subtle_inset_for_every_control_size() {
        for (width, height) in [
            (20., 20.),
            (28., 28.),
            (176., 28.),
            (240., 36.),
            (137., 58.),
        ] {
            let inset = squash_inset(size(px(width), px(height)));
            let short: f32 = width.min(height);
            // Roughly a 0.9–0.98 scale of the short side, never sub-pixel.
            assert!((0.75..=2.).contains(&inset));
            assert!(1. - 2. * inset / short >= 0.9);
        }
    }

    #[test]
    fn level_bars_glide_like_the_reference_ease_in_out() {
        // cubic-bezier(.4, 0, .2, 1): starts and ends gently, no overshoot.
        assert_eq!(standard_curve(0.), 0.);
        assert_eq!(standard_curve(1.), 1.);
        assert!(standard_curve(0.1) < 0.1);
        assert!(standard_curve(0.5) > 0.5);
        let mut previous = 0.;
        for step in 1..=100 {
            let value = standard_curve(step as f32 / 100.);
            assert!(value >= previous && value <= 1.);
            previous = value;
        }
        // A bar retargeted mid-glide starts from where it is.
        let now = Instant::now();
        let mut bar = Track::rest(3., now);
        bar.retarget(13., now, BAR_GLIDE, Curve::Standard);
        let halfway = bar.value(now + BAR_GLIDE / 2);
        assert!(halfway > 3. && halfway < 13.);
        bar.retarget(3., now + BAR_GLIDE / 2, BAR_GLIDE, Curve::Standard);
        assert!((bar.value(now + BAR_GLIDE / 2) - halfway).abs() < 1e-4);
        assert!(bar.settled(now + BAR_GLIDE * 2));
    }

    #[test]
    fn cereal_loader_starts_and_ends_at_rest_and_stays_in_its_box() {
        for end in [0., 1.] {
            let pose = cereal_loader_pose(end);
            assert!((pose.bowl.0 - end).abs() < 1e-5 && (pose.bowl.1 - 1.).abs() < 1e-5);
            for (index, ((x, y), grow)) in pose.flakes.into_iter().enumerate() {
                let ((rx, ry), _) = LOADER_FLAKES[index];
                assert!((x - rx).abs() < 1e-3 && (y - ry).abs() < 1e-3 && (grow - 1.).abs() < 1e-5);
            }
        }
        let mut swung_past = false;
        for tick in 0..=1000 {
            let pose = cereal_loader_pose(tick as f32 / 1000.);
            swung_past |= pose.bowl.0 > 1.01;
            for (index, ((x, y), grow)) in pose.flakes.into_iter().enumerate() {
                let radius = LOADER_FLAKES[index].1 * grow;
                assert!(x - radius > -4. && x + radius < 28., "x {x}");
                assert!(y - radius > -4. && y + radius < 28., "y {y}");
            }
        }
        assert!(swung_past);
    }

    #[test]
    fn arriving_starts_unseen_below_and_ends_in_place_and_reduced_only_fades() {
        assert_eq!(arrive_pose(0., false), (0., 12.));
        assert_eq!(arrive_pose(1., false), (1., 0.));
        let mut last = 0.;
        for tick in 0..=100 {
            let (fade, rise) = arrive_pose(tick as f32 / 100., false);
            assert!(fade >= last && (0. ..=1.).contains(&fade));
            // It settles from below and swings past by no more than a pixel or so.
            assert!(rise <= 12. && rise > -1.5, "{rise}");
            last = fade;
        }
        // Reduced motion: nothing moves, and the fade lasts the whole of its
        // time rather than being over in the first third of it.
        assert_eq!(arrive_pose(0., true), (0., 0.));
        assert_eq!(arrive_pose(1., true), (1., 0.));
        let (half, rise) = arrive_pose(0.3, true);
        assert!(half < 0.6 && rise == 0., "{half}");
    }

    /// Every `x,y` pair of an icon's path.
    fn path_points(svg: &str) -> Vec<(f32, f32)> {
        let path = svg.split(" d=\"").nth(1).unwrap().split('"').next().unwrap();
        path.split(|c: char| c == ' ' || c == 'M' || c == 'Z')
            .filter_map(|pair| {
                let (x, y) = pair.split_once(',')?;
                Some((x.parse().ok()?, y.parse().ok()?))
            })
            .collect()
    }

    #[test]
    fn cereal_loader_turns_the_bowl_about_the_middle_of_its_ring() {
        // The ring's outer edge is one circle about `LOADER_BOWL`: its
        // leftmost, topmost and lowest points are the same distance from it
        // (the bite takes the right-hand edge, which is why the middle of the
        // path's bounds is not the middle of the ring).
        let bowl = path_points(include_str!("../assets/icons/cereal-bowl.svg"));
        assert!(bowl.len() > 100);
        let far = |pick: fn(&(f32, f32)) -> f32| {
            let (x, y) = bowl
                .iter()
                .copied()
                .min_by(|a, b| pick(a).total_cmp(&pick(b)))
                .unwrap();
            ((x - LOADER_BOWL.0).powi(2) + (y - LOADER_BOWL.1).powi(2)).sqrt()
        };
        let (left, top, bottom) = (far(|p| p.0), far(|p| p.1), far(|p| -p.1));
        assert!((left - top).abs() < 0.02 && (left - bottom).abs() < 0.02, "{left} {top} {bottom}");
        assert!((left - LOADER_RING).abs() < 0.02, "{left}");
        // And no point of the bowl is further from it than that edge.
        for (x, y) in &bowl {
            let away = ((x - LOADER_BOWL.0).powi(2) + (y - LOADER_BOWL.1).powi(2)).sqrt();
            assert!(away < left + 0.02, "{x},{y} is {away} away");
        }
        // GPUI draws an svg at `c + shift + R·S·(p − c)`, `c` the middle of
        // its box. With the loader's shift the ring's middle stays where it
        // is at every turn and scale, so the bowl spins without swinging.
        let unit = 56. / 24.;
        let (cx, cy) = (LOADER_BOWL.0 * unit, LOADER_BOWL.1 * unit);
        for tick in 0..=200 {
            let (turn, scale) = cereal_loader_pose(tick as f32 / 200.).bowl;
            let shift = loader_bowl_shift(turn, scale, unit);
            let (sin, cos) = (std::f32::consts::TAU * turn).sin_cos();
            let (dx, dy) = (scale * (cx - 12. * unit), scale * (cy - 12. * unit));
            let x = 12. * unit + shift.0 + dx * cos - dy * sin;
            let y = 12. * unit + shift.1 + dx * sin + dy * cos;
            assert!((x - cx).abs() < 1e-3 && (y - cy).abs() < 1e-3, "{x},{y}");
        }
    }

    #[test]
    fn cereal_loader_rests_as_the_mark_and_moves_without_a_jump() {
        // At rest the flakes are the mark's own: the same middles and radii
        // as the two dots of `cereal.svg`, which is the bowl plus them.
        let mark = path_points(include_str!("../assets/icons/cereal.svg"));
        let bowl = path_points(include_str!("../assets/icons/cereal-bowl.svg"));
        let dots: Vec<_> = mark.iter().filter(|point| !bowl.contains(point)).copied().collect();
        for ((x, y), radius) in LOADER_FLAKES {
            let on_edge = dots
                .iter()
                .filter(|(px, py)| (((px - x).powi(2) + (py - y).powi(2)).sqrt() - radius).abs() < 0.01)
                .count();
            assert!(on_edge >= 20, "a dot of the mark at {x},{y}: {on_edge}");
        }
        // No frame moves anything far: at 120 Hz over the 2.2 s loop a flake
        // never travels more than a fifth of the mark between two frames,
        // the bowl never turns more than a twelfth, and the wrap is still.
        let frames = 264;
        let at = |frame: usize| cereal_loader_pose((frame % frames) as f32 / frames as f32);
        for frame in 0..frames {
            let (a, b) = (at(frame), at(frame + 1));
            let wrapped = if frame + 1 == frames { 1. } else { 0. };
            assert!((b.bowl.0 + wrapped - a.bowl.0).abs() < 1. / 12., "bowl at {frame}");
            assert!((b.bowl.1 - a.bowl.1).abs() < 0.01);
            for index in 0..2 {
                let ((ax, ay), ag) = a.flakes[index];
                let ((bx, by), bg) = b.flakes[index];
                assert!(((bx - ax).powi(2) + (by - ay).powi(2)).sqrt() < 24. / 5., "flake at {frame}");
                assert!((bg - ag).abs() < 0.08);
            }
        }
        // The flakes go round the bowl, never onto it.
        for tick in 0..=2000 {
            let pose = cereal_loader_pose(tick as f32 / 2000.);
            for (index, ((x, y), grow)) in pose.flakes.into_iter().enumerate() {
                let away = (x - LOADER_BOWL.0).hypot(y - LOADER_BOWL.1);
                let gap = away - LOADER_FLAKES[index].1 * grow - LOADER_RING;
                assert!(gap > 0.5, "flake {index} is {gap} from the bowl at {tick}");
            }
        }
        // Nor onto each other, as the first passes the second on its way.
        for tick in 0..=2000 {
            let [((ax, ay), ag), ((bx, by), bg)] = cereal_loader_pose(tick as f32 / 2000.).flakes;
            let gap = (ax - bx).hypot(ay - by) - LOADER_FLAKES[0].1 * ag - LOADER_FLAKES[1].1 * bg;
            assert!(gap > 0.25, "the flakes are {gap} apart at {tick}");
        }
        let (last, first) = (at(frames - 1), at(0));
        assert!((last.bowl.0 - 1.).abs() < 1e-3 && first.bowl.0 == 0.);
    }

    #[test]
    fn spinner_turn_is_seamless_and_never_runs_backwards() {
        assert_eq!(fluid_turn(0.), 0.);
        assert!((fluid_turn(1.) - 1.).abs() < 1e-6);
        let mut previous = 0.;
        for tick in 1..=1000 {
            let value = fluid_turn(tick as f32 / 1000.);
            assert!(value > previous);
            previous = value;
        }
        // Same speed entering and leaving a turn, so the loop has no seam.
        let entering = fluid_turn(0.001) - fluid_turn(0.);
        let leaving = fluid_turn(1.) - fluid_turn(0.999);
        assert!((entering - leaving).abs() < 1e-5);
    }

    #[test]
    fn thinking_sheen_preserves_readability_and_loops_without_a_brightness_jump() {
        for glyph in 0..=100 {
            let x = glyph as f32 / 100.;
            assert!((shimmer_opacity(x, 0.) - shimmer_opacity(x, 1.)).abs() < 0.00001);
            for tick in 0..=100 {
                let alpha = shimmer_opacity(x, tick as f32 / 100.);
                assert!((0.19999..=1.00001).contains(&alpha));
            }
        }
        assert!(shimmer_opacity(0., 0.) > 0.99);
        assert!(shimmer_opacity(0., 0.5) < 0.21);
        assert!((css_ease_in_out(0.5) - 0.5).abs() < 0.00001);
        // GPUI easing functions must stay inside 0..=1 and be monotonic.
        let mut previous = 0.;
        for tick in 0..=1000 {
            let value = css_ease_in_out(tick as f32 / 1000.);
            assert!((0. ..=1.).contains(&value) && value >= previous);
            previous = value;
        }
    }
}
