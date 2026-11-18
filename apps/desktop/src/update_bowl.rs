//! The update's mark: a fresh bowl being poured.
//!
//! The notice of an update leads with the Cereal mark, and the mark tells
//! the update's story instead of a spinner and a bar. A version that is
//! found drops in and its flakes hop. Taking it empties the bowl to an
//! outline; the flakes dive in and the download pours after them: the bowl
//! fills with gold to the share that has arrived, its surface rocking with
//! the flow, a flake falling for every measure of bytes. While the archive
//! is checked the flakes chase the full bowl round (the loader's own loop).
//! When it is ready the bowl pops, the flakes are flung out and settle, and
//! a ring of gold spreads and fades. A failure tips the bowl over and lets
//! it run out.
//!
//! Everything is painted (paths, discs, glyphs), nothing is laid out, and
//! every pose is a pure function of the time since its part began, so it
//! can be tested and slowed (`CEREAL_SLOW_MOTION`). Reduced motion shows
//! each part's still picture: the level is information and stays, the
//! movement goes.
use crate::{
    motion::{
        self, GOOEY, LOADER_BOWL as MIDDLE, LOADER_FLAKES as FLAKES, LOADER_LOOP, LOADER_RING as RING,
        Spring,
    },
    ui::Theme,
};
use gpui::{prelude::*, *};
use std::{
    f32::consts::{PI, TAU},
    sync::OnceLock,
    time::Instant,
};

pub type Spot = (f32, f32);

// ---------------------------------------------------------------------------
// Geometry: the bowl's outline and what of it is under a surface
// ---------------------------------------------------------------------------

/// The bowl's outline in the mark's 24-unit box, read from the icon itself:
/// one closed run of points (the ring, with its bite).
pub fn outline() -> &'static [Spot] {
    static OUTLINE: OnceLock<Vec<Spot>> = OnceLock::new();
    OUTLINE.get_or_init(|| parse_outline(include_str!("../assets/icons/cereal-bowl.svg")))
}

fn parse_outline(svg: &str) -> Vec<Spot> {
    let data = svg
        .split(" d=\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .unwrap_or_default();
    data.trim_start_matches('M')
        .trim_end_matches('Z')
        .split_whitespace()
        .filter_map(|pair| {
            let (x, y) = pair.split_once(',')?;
            Some((x.parse().ok()?, y.parse().ok()?))
        })
        .collect()
}

/// The part of `polygon` on the inner side of the line from `a` to `b`: the
/// side to the right of that direction on screen (below a line drawn left to
/// right). A corner cut off leaves its two crossings joined along the line,
/// which encloses nothing and so fills nothing.
pub fn clip(polygon: &[Spot], a: Spot, b: Spot, kept: &mut Vec<Spot>) {
    kept.clear();
    let side = |p: Spot| (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0);
    let Some(&last) = polygon.last() else {
        return;
    };
    let (mut from, mut from_side) = (last, side(last));
    for &to in polygon {
        let to_side = side(to);
        if (from_side >= 0.) != (to_side >= 0.) {
            let share = from_side / (from_side - to_side);
            kept.push((
                from.0 + (to.0 - from.0) * share,
                from.1 + (to.1 - from.1) * share,
            ));
        }
        if to_side >= 0. {
            kept.push(to);
        }
        (from, from_side) = (to, to_side);
    }
}

/// How many upright strips the liquid is cut into: its surface is a straight
/// line across each, so this many make its curve.
const STRIPS: usize = 10;
/// How long the surface's wave is, in units: one swell across the bowl.
const SWELL: f32 = 2.2 * RING;

/// The height of the surface at `x`, in the mark's units: `level` of the
/// way up the bowl, leaning by `slope` about the bowl's middle and rippling.
pub fn surface(pose: &BowlPose, x: f32) -> f32 {
    let middle = (MIDDLE.0 + pose.shift.0, MIDDLE.1 + pose.shift.1);
    let half = RING * pose.scale.1;
    let from_middle = x - middle.0;
    middle.1 + half - pose.level.clamp(0., 1.) * 2. * half
        + pose.slope * from_middle
        + pose.wave * (pose.phase + from_middle * TAU / SWELL).sin()
}

/// Where a point of the mark at rest is drawn in `pose`: scaled and turned
/// about the middle of the ring, then shifted.
pub fn place(pose: &BowlPose, spot: Spot) -> Spot {
    let (sin, cos) = (TAU * pose.turn).sin_cos();
    let (x, y) = (
        (spot.0 - MIDDLE.0) * pose.scale.0,
        (spot.1 - MIDDLE.1) * pose.scale.1,
    );
    (
        MIDDLE.0 + pose.shift.0 + x * cos - y * sin,
        MIDDLE.1 + pose.shift.1 + x * sin + y * cos,
    )
}

/// The liquid in the bowl, as closed runs of points in the mark's units.
pub fn liquid(pose: &BowlPose) -> Vec<Vec<Spot>> {
    let level = pose.level.clamp(0., 1.);
    if level <= 0.001 {
        return Vec::new();
    }
    let bowl: Vec<Spot> = outline().iter().map(|&spot| place(pose, spot)).collect();
    if level >= 0.999 {
        return vec![bowl];
    }
    let (left, right) = bowl.iter().fold((f32::MAX, f32::MIN), |(low, high), spot| {
        (low.min(spot.0), high.max(spot.0))
    });
    let step = (right - left) / STRIPS as f32;
    let mut body = Vec::new();
    let (mut one, mut two) = (Vec::new(), Vec::new());
    for strip in 0..STRIPS {
        let (from, to) = (left + step * strip as f32, left + step * (strip + 1) as f32);
        let (a, b) = ((from, surface(pose, from)), (to, surface(pose, to)));
        // Right of the strip's left edge, left of its right edge, under the
        // surface.
        clip(&bowl, (from, 1.), (from, 0.), &mut one);
        clip(&one, (to, 0.), (to, 1.), &mut two);
        clip(&two, a, b, &mut one);
        if one.len() >= 3 {
            body.push(one.clone());
        }
    }
    body
}

// ---------------------------------------------------------------------------
// Poses
// ---------------------------------------------------------------------------

/// The parts of the update's story the mark plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Act {
    /// Nothing to tell: the mark as it is drawn everywhere.
    #[default]
    Still,
    /// Asking the server: the loader's loop, in ink.
    Seeking,
    /// A version was found: the mark drops in and its flakes hop.
    Found,
    /// Downloading: the bowl fills.
    Pouring,
    /// Verifying, unpacking, installing: the flakes chase the full bowl.
    Proving,
    /// Ready for the restart: the pop.
    Ready,
    /// It failed: the bowl tips over and runs out.
    Spilled,
    /// Asked, and there is nothing newer: a nod, and the flakes hop.
    Pleased,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Flake {
    pub at: Spot,
    /// Its size, as a share of its size at rest.
    pub grow: f32,
    pub ink: f32,
}

/// A disc on its way somewhere: a flake being poured, a drop spilt, a spark.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mote {
    pub at: Spot,
    pub radius: f32,
    pub ink: f32,
}

impl Mote {
    const NONE: Mote = Mote {
        at: (0., 0.),
        radius: 0.,
        ink: 0.,
    };
}

/// How many motes can be in the air at once.
pub const MOTES: usize = 8;

/// How the mark is drawn right now, in its own 24-unit box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BowlPose {
    /// The bowl's shift, turn (1 is a whole turn) and scale about the
    /// middle of its ring.
    pub shift: Spot,
    pub turn: f32,
    pub scale: Spot,
    /// The whole mark's opacity.
    pub ink: f32,
    /// 1: the bowl in ink, as the mark is drawn everywhere. 0: an empty
    /// bowl, its shape in the faint ink of a track.
    pub solid: f32,
    /// How full the bowl is, 0..=1, and its surface: how it leans (units
    /// down per unit across) and ripples (height in units, and where the
    /// ripple is in its travel, in radians).
    pub level: f32,
    pub slope: f32,
    pub wave: f32,
    pub phase: f32,
    pub flakes: [Flake; 2],
    /// 1: the flakes in gold. 0: in ink.
    pub gilt: f32,
    pub motes: [Mote; MOTES],
    /// The ring that spreads when the update is ready: its radius and
    /// thickness in units, and its ink.
    pub halo: (f32, f32, f32),
    /// How far the sparks have flown, 0..=1; 1 is none.
    pub burst: f32,
    /// The gold lit up, 0..=1.
    pub flash: f32,
    /// The gold gone dull, 0..=1.
    pub dull: f32,
}

impl BowlPose {
    /// The mark as it is drawn everywhere, with its flakes in gold.
    pub const REST: BowlPose = BowlPose {
        shift: (0., 0.),
        turn: 0.,
        scale: (1., 1.),
        ink: 1.,
        solid: 1.,
        level: 0.,
        slope: 0.,
        wave: 0.,
        phase: 0.,
        flakes: [
            Flake {
                at: FLAKES[0].0,
                grow: 1.,
                ink: 1.,
            },
            Flake {
                at: FLAKES[1].0,
                grow: 1.,
                ink: 1.,
            },
        ],
        gilt: 1.,
        motes: [Mote::NONE; MOTES],
        halo: (0., 0., 0.),
        burst: 1.,
        flash: 0.,
        dull: 0.,
    };
}

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    t * t * (3. - 2. * t)
}

fn lerp(from: f32, to: f32, t: f32) -> f32 {
    from + (to - from) * t
}

fn lerp_spot(from: Spot, to: Spot, t: f32) -> Spot {
    (lerp(from.0, to.0, t), lerp(from.1, to.1, t))
}

/// A knock that rings and dies away: nothing at the start, 1 at its first
/// peak, a small dip after it, and still again by [`KICK`] seconds.
pub fn kick(t: f32) -> f32 {
    if t <= 0. || t >= KICK {
        return 0.;
    }
    const PERIOD: f32 = 0.5;
    const DECAY: f32 = 7.;
    let omega = TAU / PERIOD;
    let raw = |t: f32| (-DECAY * t).exp() * (omega * t).sin();
    let peak = (omega / DECAY).atan() / omega;
    raw(t) / raw(peak) * (1. - smooth((t - 0.7 * KICK) / (0.3 * KICK)))
}

/// How long a [`kick`] rings.
pub const KICK: f32 = 0.9;

// Found ----------------------------------------------------------------------

/// The notice starts to open, then the mark is let go.
const FOUND_WAIT: f32 = 0.08;
/// How far above its place the mark is let go, in units, and how long it
/// falls.
const FALL_FROM: f32 = 9.;
const FALL: f32 = 0.24;
/// After landing, when the first flake hops, how long after it the second
/// does, and how long a hop is.
const HOP_AFTER: f32 = 0.16;
const HOP_APART: f32 = 0.13;
const HOP: f32 = 0.36;
/// The whole of the arrival.
pub const FOUND_LENGTH: f32 = FOUND_WAIT + FALL + HOP_AFTER + HOP_APART + HOP + 0.25;

/// A thing let go at 0: how far above its place it is (negative, in units)
/// and how squashed (positive) or stretched (negative) it is, as a share of
/// its height. It falls faster and faster, stretched by its speed, lands
/// flat and wobbles back to round.
pub fn landing(t: f32) -> (f32, f32) {
    const STRETCH: f32 = 0.07;
    if t <= 0. {
        return (-FALL_FROM, 0.);
    }
    if t < FALL {
        let fall = t / FALL;
        return (-FALL_FROM * (1. - fall * fall), -STRETCH * fall);
    }
    let since = t - FALL;
    let turn = TAU * since / 0.36;
    let wobble = (-6.5 * since).exp() * (0.3 * turn.sin() - STRETCH * turn.cos());
    (0., wobble * (1. - smooth((since - 0.45) / 0.25)))
}

fn found(t: f32) -> BowlPose {
    let t = t - FOUND_WAIT;
    if t >= FOUND_LENGTH - FOUND_WAIT {
        return BowlPose::REST;
    }
    let mut pose = BowlPose::REST;
    let (lift, squash) = landing(t);
    pose.ink = motion::fade_curve((t / 0.14).clamp(0., 1.));
    pose.scale = (1. + 0.55 * squash, 1. - squash);
    // It squashes against the ground, not about its middle.
    pose.shift = (0., lift + RING * squash);
    for (index, flake) in pose.flakes.iter_mut().enumerate() {
        // Each flake a beat behind the bowl, so they land after it.
        let (lift, squash) = landing(t - 0.03 * (index + 1) as f32);
        let (x, y) = FLAKES[index].0;
        let hop = motion::bump((t - FALL - HOP_AFTER - HOP_APART * index as f32) / HOP);
        flake.at = (
            MIDDLE.0 + (x - MIDDLE.0) * (1. + 0.55 * squash) + 0.7 * hop,
            MIDDLE.1 + (y - MIDDLE.1) * (1. - squash) + lift + RING * squash - 3.2 * hop,
        );
        flake.grow = 1. + 0.4 * hop;
    }
    pose
}

// Pouring --------------------------------------------------------------------

/// How long a flake takes to dive into the bowl, and how long after the
/// first the second goes.
const DIVE: f32 = 0.44;
const DIVE_APART: f32 = 0.1;
/// When the pour itself starts: the flakes are in.
pub const POUR_AFTER: f32 = 0.04 + DIVE_APART + DIVE * 0.8;
/// Where a poured flake is let go (units above the box's top edge is
/// negative), how fast it leaves and how it speeds up, in units and seconds.
const POUR_FROM: f32 = -4.5;
const POUR_SPEED: f32 = 14.;
const POUR_PULL: f32 = 250.;
/// A flake falls for no longer than this: the bowl's floor is nearer.
const POUR_LIFE: f32 = 0.42;

/// A poured flake let go at `x` (units across) `age` seconds ago, falling
/// towards a surface at `floor`: where it is and how much of it shows. It
/// fades in as it appears and goes under as it reaches the surface.
pub fn falling(x: f32, radius: f32, age: f32, floor: f32) -> Mote {
    if !(0. ..POUR_LIFE).contains(&age) {
        return Mote::NONE;
    }
    let y = POUR_FROM + POUR_SPEED * age + 0.5 * POUR_PULL * age * age;
    let ink = ((y - POUR_FROM) / 2.5).clamp(0., 1.) * ((floor - y) / 1.8).clamp(0., 1.);
    Mote {
        at: (x, y),
        radius,
        ink,
    }
}

fn pouring(t: f32, level: f32, rate: f32) -> BowlPose {
    let mut pose = BowlPose::REST;
    pose.solid = 1. - smooth(t / 0.3);
    for (index, flake) in pose.flakes.iter_mut().enumerate() {
        let share = ((t - 0.04 - DIVE_APART * index as f32) / DIVE).clamp(0., 1.);
        let into = (MIDDLE.0 - 1.6 + 3.2 * index as f32, MIDDLE.1 + 1.5);
        let (x, y) = lerp_spot(FLAKES[index].0, into, share);
        // Tossed up and over the rim.
        flake.at = (x, y - 6.5 * 4. * share * (1. - share));
        flake.grow = 1. + 0.3 * (PI * share).sin();
        flake.ink = 1. - smooth((share - 0.78) / 0.22);
    }
    let level = level.clamp(0., 1.);
    // Flat when empty and when full: the surface has nowhere to go.
    let room = 2. * (level * (1. - level)).sqrt();
    let rate = rate.clamp(0., 1.);
    pose.level = level;
    pose.slope = room * (0.04 + 0.1 * rate) * (TAU * t / 0.92).sin();
    pose.wave = room * (0.12 + 0.3 * rate);
    pose.phase = TAU * t / 0.67;
    pose
}

// Proving and seeking --------------------------------------------------------

/// How long the flakes take to come back out of a full bowl.
const EMERGE: f32 = 0.34;
/// Where they come out: the bite.
const MOUTH: Spot = (16.6, 9.4);

fn looping(t: f32) -> BowlPose {
    let mut pose = BowlPose::REST;
    let turn = motion::cereal_loader_pose((t / LOADER_LOOP.as_secs_f32()).rem_euclid(1.));
    pose.turn = turn.bowl.0;
    pose.scale = (turn.bowl.1, turn.bowl.1);
    for (flake, (at, grow)) in pose.flakes.iter_mut().zip(turn.flakes) {
        flake.at = at;
        flake.grow = grow;
    }
    pose
}

fn proving(t: f32) -> BowlPose {
    let mut pose = if t < EMERGE {
        let mut pose = BowlPose::REST;
        for (index, flake) in pose.flakes.iter_mut().enumerate() {
            let lag = 0.14 * index as f32;
            let out = GOOEY.at(((t / EMERGE - lag) / (1. - lag)).max(0.));
            flake.at = lerp_spot(MOUTH, FLAKES[index].0, out);
            flake.grow = out.clamp(0., 1.15);
            flake.ink = motion::fade_for(out);
        }
        pose
    } else {
        looping(t - EMERGE)
    };
    pose.solid = 0.;
    pose.level = 1.;
    pose
}

fn seeking(t: f32) -> BowlPose {
    let mut pose = looping(t);
    pose.gilt = 0.;
    pose
}

// Ready ----------------------------------------------------------------------

/// The whole of the pop.
pub const READY_LENGTH: f32 = KICK + 0.15;
/// How long the ring takes to spread and the sparks to fly.
const HALO: f32 = 0.62;
const SPARKS: f32 = 0.55;

fn ready(t: f32) -> BowlPose {
    let mut pose = BowlPose::REST;
    pose.solid = 0.;
    pose.level = 1.;
    if t >= READY_LENGTH {
        return pose;
    }
    let pop = 1. + 0.2 * kick(t);
    pose.scale = (pop, pop);
    for (index, flake) in pose.flakes.iter_mut().enumerate() {
        // Flung out along the line from the bowl's middle, a beat apart.
        let fling = kick(t - 0.045 * (index + 1) as f32);
        let (x, y) = FLAKES[index].0;
        let reach = 1. + 0.5 * fling;
        flake.at = (
            MIDDLE.0 + (x - MIDDLE.0) * reach,
            MIDDLE.1 + (y - MIDDLE.1) * reach,
        );
        flake.grow = (1. + 0.6 * fling).max(0.5);
    }
    let spread = (t / HALO).clamp(0., 1.);
    pose.halo = (
        RING * (1.02 + 0.72 * motion::exit_curve(spread)),
        0.2 + 1.5 * (1. - spread),
        0.55 * (1. - spread).powf(1.5),
    );
    pose.burst = ((t - 0.07) / SPARKS).clamp(0., 1.);
    pose.flash = (1. - t / 0.22).clamp(0., 1.);
    pose
}

/// The sparks `burst` of the way through their flight: five, evenly round
/// the bowl, slowing as they go and shrinking to nothing. They are the
/// size of the mark's flakes: anything smaller is lost at one pixel a
/// point.
pub fn sparks(burst: f32) -> [Mote; 5] {
    std::array::from_fn(|index| {
        if burst <= 0. || burst >= 1. {
            return Mote::NONE;
        }
        let angle = TAU * (index as f32 / 5.) + 2.2;
        // From clear of the bowl at its biggest to short of the text.
        let far = RING * (1.42 + 0.33 * motion::exit_curve(burst));
        Mote {
            at: (MIDDLE.0 + far * angle.cos(), MIDDLE.1 + far * angle.sin()),
            radius: 0.95 * (1. - burst * burst) * smooth(burst / 0.12),
            ink: 1. - smooth((burst - 0.6) / 0.4),
        }
    })
}

/// The second flight of sparks, where the mark is big enough to have room
/// for one: seven smaller ones between the first five, let go a beat later
/// and thrown further. `burst` is the first flight's.
pub fn more_sparks(burst: f32) -> [Mote; 7] {
    std::array::from_fn(|index| {
        let burst = (burst - 0.14) / 0.86;
        if burst <= 0. || burst >= 1. {
            return Mote::NONE;
        }
        let angle = TAU * ((index as f32 + 0.5) / 7.) + 0.6;
        // Alternately nearer and further, so they do not read as a ring.
        let reach = if index % 2 == 0 { 0.52 } else { 0.4 };
        let far = RING * (1.5 + reach * motion::exit_curve(burst));
        Mote {
            at: (MIDDLE.0 + far * angle.cos(), MIDDLE.1 + far * angle.sin()),
            radius: 0.5 * (1. - burst * burst) * smooth(burst / 0.12),
            ink: 1. - smooth((burst - 0.5) / 0.5),
        }
    })
}

// Spilled --------------------------------------------------------------------

/// The whole of the spill.
pub const SPILL_LENGTH: f32 = 1.3;
/// How far the bowl tips, in turns: its bite ends up underneath.
const TIP: f32 = 0.11;
const TIP_SPRING: Spring = Spring {
    millis: 0,
    damping: 0.74,
};

fn spilled(t: f32, held: f32) -> BowlPose {
    let mut pose = BowlPose::REST;
    pose.turn = TIP * TIP_SPRING.at(t / 0.95);
    pose.solid = lerp(0., 0.42, smooth(t / 0.7));
    pose.level = held.clamp(0., 1.) * (1. - smooth(t / 0.85));
    pose.dull = smooth(t / 0.6);
    pose.gilt = 1. - pose.dull;
    // The surface leans the way it runs.
    pose.slope = 0.12 * motion::bump(t / 0.85);
    // The flakes go over with the bowl.
    let tipped = pose;
    for (index, flake) in pose.flakes.iter_mut().enumerate() {
        flake.at = place(&tipped, FLAKES[index].0);
    }
    if held > 0.04 {
        // A few drops out of the bite.
        let lip = place(&pose, (19.6, 15.2));
        for (index, mote) in pose.motes.iter_mut().take(4).enumerate() {
            let age = t - 0.16 - 0.13 * index as f32;
            if !(0. ..0.5).contains(&age) {
                continue;
            }
            *mote = Mote {
                at: (
                    lip.0 + 0.4 + (3. + index as f32) * age,
                    lip.1 + 6. * age + 60. * age * age,
                ),
                radius: 0.85 - 0.12 * index as f32,
                ink: (age / 0.06).min(1.) * (1. - smooth(age / 0.5)),
            };
        }
    }
    pose
}

// Pleased --------------------------------------------------------------------

/// The whole of the nod.
pub const PLEASED_LENGTH: f32 = KICK + 0.1;

/// Nothing newer: the bowl swells once, a third as far as the pop, and the
/// flakes hop after it, one and then the other.
fn pleased(t: f32) -> BowlPose {
    let mut pose = BowlPose::REST;
    if t >= PLEASED_LENGTH {
        return pose;
    }
    let nod = 1. + 0.07 * kick(t);
    pose.scale = (nod, nod);
    for (index, flake) in pose.flakes.iter_mut().enumerate() {
        let hop = motion::bump((t - 0.1 - HOP_APART * index as f32) / HOP);
        let (x, y) = FLAKES[index].0;
        flake.at = (x + 0.7 * hop, y - 3.2 * hop);
        flake.grow = 1. + 0.4 * hop;
    }
    pose
}

/// The picture of `act` that reduced motion shows: nothing moves, and the
/// level is the share that has arrived.
pub fn still(act: Act, level: f32) -> BowlPose {
    let mut pose = BowlPose::REST;
    match act {
        Act::Still | Act::Found | Act::Pleased => {}
        Act::Seeking => pose.gilt = 0.,
        Act::Pouring => {
            pose.solid = 0.;
            pose.level = level.clamp(0., 1.);
            for flake in &mut pose.flakes {
                flake.ink = 0.;
            }
        }
        Act::Proving | Act::Ready => {
            pose.solid = 0.;
            pose.level = 1.;
        }
        Act::Spilled => {
            pose.solid = 0.42;
            pose.gilt = 0.;
            pose.dull = 1.;
        }
    }
    pose
}

/// The mark `t` seconds into `act`. `level` is how full the bowl is shown
/// and `rate` how hard it is being poured, both 0..=1; `held` is how full it
/// was when it was spilt.
pub fn pose(act: Act, t: f32, level: f32, rate: f32, held: f32) -> BowlPose {
    let t = t.max(0.);
    match act {
        Act::Still => BowlPose::REST,
        Act::Seeking => seeking(t),
        Act::Found => found(t),
        Act::Pouring => pouring(t, level, rate),
        Act::Proving => {
            let mut pose = proving(t);
            pose.level = level;
            pose
        }
        Act::Ready => ready(t),
        Act::Spilled => spilled(t, held),
        Act::Pleased => pleased(t),
    }
}

/// How long `act` moves for; `None` for the ones that go on until the
/// update moves on.
pub fn length(act: Act) -> Option<f32> {
    match act {
        Act::Still => Some(0.),
        Act::Found => Some(FOUND_LENGTH),
        Act::Ready => Some(READY_LENGTH),
        Act::Spilled => Some(SPILL_LENGTH),
        Act::Pleased => Some(PLEASED_LENGTH),
        Act::Seeking | Act::Pouring | Act::Proving => None,
    }
}

/// How long one part's pose takes to give way to the next's.
const HANDOVER: f32 = 0.22;

/// One part giving way to the next. `was` is the pose the old part ended
/// in and `first` the one the new part starts from; `to` is the new part
/// now, `share` of the way through the handover. What the old pose had over
/// the new part's start is carried along and let go of, so the new part
/// plays in full from wherever the mark was (the bowl by the shorter way
/// round). What belongs to the new part alone (what is in the air, the
/// ring) is its own from the start, and so is the level, unless the new
/// part starts with less in the bowl than there was.
pub fn handover(was: &BowlPose, first: &BowlPose, to: &BowlPose, share: f32) -> BowlPose {
    if share >= 1. {
        return *to;
    }
    let left = 1. - smooth(share);
    let carry = |was: f32, first: f32, to: f32| to + (was - first) * left;
    let carry_spot = |was: Spot, first: Spot, to: Spot| {
        (carry(was.0, first.0, to.0), carry(was.1, first.1, to.1))
    };
    let mut pose = *to;
    pose.shift = carry_spot(was.shift, first.shift, to.shift);
    let turned = was.turn - first.turn;
    pose.turn = to.turn + (turned - turned.round()) * left;
    pose.scale = carry_spot(was.scale, first.scale, to.scale);
    pose.ink = carry(was.ink, first.ink, to.ink).clamp(0., 1.);
    pose.solid = carry(was.solid, first.solid, to.solid).clamp(0., 1.);
    pose.slope = carry(was.slope, first.slope, to.slope);
    pose.wave = carry(was.wave, first.wave, to.wave);
    pose.gilt = carry(was.gilt, first.gilt, to.gilt).clamp(0., 1.);
    pose.dull = carry(was.dull, first.dull, to.dull).clamp(0., 1.);
    for (index, flake) in pose.flakes.iter_mut().enumerate() {
        let (was, first, to) = (was.flakes[index], first.flakes[index], to.flakes[index]);
        // A flake that was not there comes in where the new part has it.
        if was.ink > 0.01 {
            flake.at = carry_spot(was.at, first.at, to.at);
            flake.grow = carry(was.grow, first.grow, to.grow);
        }
        flake.ink = carry(was.ink, first.ink, to.ink).clamp(0., 1.);
    }
    // A bowl that had something in it and is to be empty runs out; it is
    // not empty at once.
    if was.level > first.level {
        pose.level = carry(was.level, first.level, to.level).clamp(0., 1.);
    }
    pose
}

// ---------------------------------------------------------------------------
// The pour: the mark's state from frame to frame
// ---------------------------------------------------------------------------

/// A value that follows a target without ever stepping: a critically damped
/// spring, solved exactly over each interval, so it reads the same at any
/// frame rate.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Follow {
    pub at: f32,
    /// Per second.
    pub speed: f32,
}

impl Follow {
    /// `seconds` later, with `target` where it is now. `pace` is how hard
    /// it is pulled (per second): it is within a few percent of a target
    /// that stands still after `5 / pace` seconds.
    pub fn step(&mut self, target: f32, seconds: f32, pace: f32) {
        if seconds <= 0. || !seconds.is_finite() {
            return;
        }
        let behind = self.at - target;
        let drift = self.speed + pace * behind;
        let decay = (-pace * seconds).exp();
        let at = target + (behind + drift * seconds) * decay;
        let speed = (drift - pace * (behind + drift * seconds)) * decay;
        // Never past what it follows: a bowl is never shown fuller than
        // the download is.
        if (behind <= 0. && at > target) || (behind >= 0. && at < target) {
            *self = Self {
                at: target,
                speed: 0.,
            };
        } else {
            *self = Self { at, speed };
        }
    }

    pub fn settled(&self, target: f32) -> bool {
        (self.at - target).abs() < 0.0005 && self.speed.abs() < 0.002
    }
}

/// How hard the level is pulled to the share that has arrived.
const FOLLOW_PACE: f32 = 10.;
/// The share of a download per second at which the pour is at its hardest.
const FULL_RATE: f32 = 0.25;
/// A flake is poured for every this much of the download, and never sooner
/// than this many seconds after the last.
const POUR_EVERY: f32 = 0.016;
const POUR_APART: f32 = 0.06;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Poured {
    /// Seconds into the pouring.
    at: f32,
    x: f32,
    radius: f32,
}

/// Where the `count`th poured flake falls and how big it is: spread across
/// the bowl in an order that does not repeat within a download.
fn poured(count: u32, at: f32) -> Poured {
    // The golden ratio's steps: each new one lands in the widest gap.
    let across = (count as f32 * 0.618_034).fract();
    let size = (count as f32 * 0.754_877).fract();
    Poured {
        at,
        x: MIDDLE.0 + RING * 0.74 * (2. * across - 1.),
        // Between the mark's own two flakes in size.
        radius: FLAKES[0].1 + (FLAKES[1].1 - FLAKES[0].1) * size,
    }
}

/// The mark's state: which part it is playing, since when, and how full
/// the bowl is shown.
pub struct Pour {
    act: Act,
    began: Instant,
    stepped: Instant,
    /// The pose the part before this one ended in, if it is to give way.
    was: Option<BowlPose>,
    level: Follow,
    /// How full the bowl was when it was spilt.
    held: f32,
    falling: [Option<Poured>; MOTES],
    count: u32,
    /// The level at which the last flake was poured, and when.
    last: (f32, f32),
    shown: BowlPose,
    moving: bool,
}

impl Default for Pour {
    fn default() -> Self {
        let now = Instant::now();
        Self {
            act: Act::Still,
            began: now,
            stepped: now,
            was: None,
            level: Follow::default(),
            held: 0.,
            falling: [None; MOTES],
            count: 0,
            last: (0., 0.),
            shown: BowlPose::REST,
            moving: false,
        }
    }
}

impl Pour {
    /// Start `act` from its own beginning: the mark has just come on
    /// screen. A part that ends in a still picture is shown as that
    /// picture unless it is the arrival itself.
    pub fn arrive(&mut self, act: Act, share: f32, now: Instant) {
        self.act = act;
        self.was = None;
        self.falling = [None; MOTES];
        self.began = match (act, length(act)) {
            (Act::Found, _) | (_, None) => now,
            // Already over.
            (_, Some(length)) => now
                .checked_sub(motion::slow(std::time::Duration::from_secs_f32(length)))
                .unwrap_or(now),
        };
        self.stepped = now;
        self.level = Follow {
            at: Self::aim(act, share, self.level.at),
            speed: 0.,
        };
        self.last = (self.level.at, 0.);
    }

    /// Where the level is heading in `act`.
    fn aim(act: Act, share: f32, at: f32) -> f32 {
        match act {
            Act::Pouring => share.clamp(0., 1.),
            Act::Proving | Act::Ready => 1.,
            // It runs out in the pose, from where it was.
            Act::Spilled => at,
            Act::Still | Act::Seeking | Act::Found | Act::Pleased => 0.,
        }
    }

    /// One frame: `act` is the part the update is at, `share` how much of
    /// the download has arrived. Returns whether the mark is still moving.
    pub fn frame(&mut self, act: Act, share: f32, now: Instant, reduced: bool) -> bool {
        if act != self.act {
            self.was = Some(self.shown);
            if act == Act::Spilled {
                self.held = self.level.at;
            }
            // An empty bowl starts from empty: a new search, a new find,
            // and another try after a spill.
            if matches!(act, Act::Still | Act::Seeking | Act::Found | Act::Pleased)
                || (act == Act::Pouring && self.act == Act::Spilled)
            {
                self.level = Follow::default();
            }
            self.act = act;
            self.began = now;
            self.falling = [None; MOTES];
            self.last = (self.level.at, 0.);
        }
        let t = motion::paced(now.saturating_duration_since(self.began));
        let step = motion::paced(now.saturating_duration_since(self.stepped));
        self.stepped = now;
        let target = Self::aim(act, share, self.level.at);
        if reduced {
            self.level = Follow {
                at: target,
                speed: 0.,
            };
            self.shown = still(act, target);
            self.moving = false;
            return false;
        }
        self.level.step(target, step, FOLLOW_PACE);
        let level = self.level.at;
        let rate = (self.level.speed / FULL_RATE).clamp(0., 1.);
        if act == Act::Pouring
            && t >= POUR_AFTER
            && level < 0.985
            && level - self.last.0 >= POUR_EVERY
            && t - self.last.1 >= POUR_APART
        {
            self.last = (level, t);
            self.count = self.count.wrapping_add(1);
            let slot = self.count as usize % MOTES;
            self.falling[slot] = Some(poured(self.count, t));
        }
        let mut pose = pose(act, t, level, rate, self.held);
        if act == Act::Pouring {
            let surface = pose;
            for (mote, falling) in pose.motes.iter_mut().zip(&mut self.falling) {
                let Some(flake) = *falling else {
                    continue;
                };
                *mote = falling_at(&surface, flake, t);
                if t - flake.at >= POUR_LIFE {
                    *falling = None;
                }
            }
        }
        if let Some(was) = self.was {
            if t < HANDOVER {
                let first = self::pose(act, 0., level, rate, self.held);
                pose = handover(&was, &first, &pose, t / HANDOVER);
            } else {
                self.was = None;
            }
        }
        self.shown = pose;
        self.moving = length(act).is_none_or(|length| t < length.max(HANDOVER))
            || !self.level.settled(target);
        self.moving
    }

    pub fn pose(&self) -> BowlPose {
        self.shown
    }

    pub fn moving(&self) -> bool {
        self.moving
    }

    /// Seconds into the part being played.
    pub fn since(&self, now: Instant) -> f32 {
        motion::paced(now.saturating_duration_since(self.began))
    }

    /// How much of the download the bowl shows, 0..=1, and how fast that is
    /// growing (per second).
    pub fn level(&self) -> (f32, f32) {
        (self.level.at, self.level.speed)
    }

    #[cfg(test)]
    pub fn act(&self) -> Act {
        self.act
    }
}

fn falling_at(pose: &BowlPose, flake: Poured, t: f32) -> Mote {
    // With nothing in the bowl yet, its floor is where a flake goes under.
    let floor = surface(pose, flake.x).min(MIDDLE.1 + RING - 1.2);
    falling(flake.x, flake.radius, t - flake.at, floor)
}

// ---------------------------------------------------------------------------
// The odometer: digits that roll
// ---------------------------------------------------------------------------

/// How long a wheel takes to roll to its next digit, in seconds.
pub const ROLL: f32 = 0.24;
/// The share of its time a digit is shown at rest, however fast the count.
pub const ROLL_REST: f32 = 0.4;

/// Where the wheel of the `place`th digit (0 is the last) of a counter at
/// `value` stands: whole numbers are digits at rest, and the part is how
/// far it has rolled to the next. The last wheel rolls during the last
/// [`ROLL`] seconds before its digit changes, which `speed` (the value's
/// own, per second) tells. A value that changes faster than that rolls
/// faster, and still shows every digit at rest for [`ROLL_REST`] of its
/// time, so the number can be read while it runs. A wheel further up turns only while the one below it
/// comes round from 9 to 0, as on a counter, so a number half way through
/// a carry is never read as one it has not reached. The digit shown is the
/// whole part's last.
pub fn wheel(value: f64, place: u32, speed: f32) -> f64 {
    let size = 10f64.powi(place as i32);
    let turns = value.max(0.) / size;
    let whole = turns.floor();
    let window = f64::from(speed.max(0.) * ROLL).clamp(0.02, 1. - f64::from(ROLL_REST)) / size;
    let part = turns - whole;
    if part <= 1. - window {
        return whole;
    }
    whole + f64::from(smooth(((part - (1. - window)) / window) as f32))
}

/// A wheel's two faces: the digit leaving and the digit arriving (`None`
/// for the blank before a number's first digit), and how far the roll is.
/// `decimals` is how many of the counter's places are after the point.
pub fn faces(position: f64, place: u32, decimals: u32) -> (Option<u8>, Option<u8>, f32) {
    let whole = position.floor();
    let face = |count: f64| (count >= 1. || place <= decimals).then(|| (count % 10.) as u8);
    (face(whole), face(whole + 1.), (position - whole) as f32)
}

/// How a rolling face is drawn `roll` of the way: the leaving face rises
/// out of the wheel's window as the arriving one rises into it, each going
/// faint only near the window's edge. Returns each one's shift (as a share
/// of the window's height; negative is up) and ink.
pub fn rolled(roll: f32) -> ((f32, f32), (f32, f32)) {
    let roll = roll.clamp(0., 1.);
    ((-roll, 1. - roll * roll), (1. - roll, 1. - (1. - roll) * (1. - roll)))
}

/// A number being counted up, for [`paint_counter`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Counter {
    /// In units of its last digit: 437 for 43.7 with one decimal.
    pub value: f64,
    /// Per second, in the same units.
    pub speed: f32,
    pub decimals: u32,
    /// The most it will reach, which is how many wheels it is given.
    pub most: f64,
}

impl Counter {
    fn places(&self) -> u32 {
        let mut places = self.decimals + 1;
        while 10f64.powi(places as i32) <= self.most.max(self.value) {
            places += 1;
        }
        places
    }

    /// The wheels from the first digit to the last: each one's faces and
    /// roll, and how much of a digit's width it takes (a first digit that
    /// is still rolling in takes less).
    pub fn wheels(&self) -> Vec<(Option<u8>, Option<u8>, f32, f32)> {
        (0..self.places())
            .rev()
            .map(|place| {
                let (leaving, arriving, roll) =
                    faces(wheel(self.value, place, self.speed), place, self.decimals);
                let width = match (leaving, arriving) {
                    (Some(_), _) => 1.,
                    (None, Some(_)) => smooth(roll),
                    (None, None) => 0.,
                };
                (leaving, arriving, roll, width)
            })
            .collect()
    }
}

fn shape(text: &str, size: f32, color: Hsla, window: &Window) -> ShapedLine {
    let style = window.text_style();
    let run = TextRun {
        len: text.len(),
        font: style.font(),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    window
        .text_system()
        .shape_line(SharedString::from(text.to_owned()), px(size), &[run], None)
}

/// The width of the widest digit at `size` in the face in use: every wheel
/// is this wide, so a number does not shuffle as its digits change.
fn digit_width(size: f32, window: &Window) -> f32 {
    let style = window.text_style();
    let system = window.text_system();
    let face = system.resolve_font(&style.font());
    ('0'..='9')
        .filter_map(|digit| system.advance(face, px(size), digit).ok())
        .map(|advance| f32::from(advance.width))
        .fold(size * 0.5, f32::max)
}

/// How wide [`paint_counter`] draws `counter` followed by `tail`.
pub fn counter_width(counter: Option<&Counter>, tail: &str, size: f32, window: &Window) -> f32 {
    let tail = f32::from(shape(tail, size, gpui::black(), window).width);
    let Some(counter) = counter else {
        return tail;
    };
    let wheels: f32 = counter.wheels().iter().map(|wheel| wheel.3).sum();
    let point = if counter.decimals > 0 {
        f32::from(shape(".", size, gpui::black(), window).width)
    } else {
        0.
    };
    wheels * digit_width(size, window) + point + tail
}

/// Draw `counter` (if there is one) and `tail` after it on one line whose
/// box starts at `origin`, in `color`. `metrics` is the text's size, the
/// line's height and the height of the window a digit rolls through, in
/// px: the window is in the middle of the line and a digit that is leaving
/// or arriving is cut off at its edges, as on a counter's wheel. It is
/// painted in the face in use, at fractions of a pixel, and nothing of it
/// is laid out.
pub fn paint_counter(
    counter: Option<&Counter>,
    tail: &str,
    origin: Point<Pixels>,
    metrics: (f32, f32, f32),
    color: Hsla,
    window: &mut Window,
    cx: &mut App,
) {
    let (size, line, travel) = metrics;
    let mut x = f32::from(origin.x);
    let top = f32::from(origin.y);
    let wheel_window = Bounds::new(
        point(origin.x - px(1.), px(top + (line - travel) / 2.)),
        gpui::size(px(240.), px(travel)),
    );
    let mut glyph = |text: &str, x: f32, shift: f32, level: f32, cell: Option<f32>, window: &mut Window| {
        if level <= 0.01 || text.is_empty() {
            return 0.;
        }
        let shaped = shape(text, size, color.opacity(color.a * level), window);
        let width = f32::from(shaped.width);
        // A digit sits in the middle of its wheel.
        let left = cell.map_or(x, |cell| x + (cell - width) / 2.);
        let _ = shaped.paint(point(px(left), px(top + shift * travel)), px(line), window, cx);
        width
    };
    if let Some(counter) = counter {
        let digit = digit_width(size, window);
        let wheels = counter.wheels();
        let count = wheels.len() as u32;
        for (index, (leaving, arriving, roll, width)) in wheels.into_iter().enumerate() {
            let place = count - 1 - index as u32;
            let cell = digit * width;
            let ((up, leaving_ink), (down, arriving_ink)) = rolled(roll);
            if roll <= 0. {
                if let Some(face) = leaving {
                    glyph(&face.to_string(), x, 0., 1., Some(cell), window);
                }
            } else {
                window.with_content_mask(Some(ContentMask { bounds: wheel_window }), |window| {
                    if let Some(face) = leaving {
                        glyph(&face.to_string(), x, up, leaving_ink, Some(cell), window);
                    }
                    if let Some(face) = arriving {
                        glyph(&face.to_string(), x, down, arriving_ink, Some(cell), window);
                    }
                });
            }
            x += cell;
            if place == counter.decimals && counter.decimals > 0 {
                x += glyph(".", x, 0., 1., None, window);
            }
        }
    }
    glyph(tail, x, 0., 1., None, window);
}

// ---------------------------------------------------------------------------
// Painting
// ---------------------------------------------------------------------------

/// The app's gold as a fill, the same in both themes.
pub fn gold() -> Hsla {
    crate::onboarding_scene::gold()
}

fn disc(window: &mut Window, center: Spot, radius: f32, color: Hsla) {
    if radius <= 0.05 || color.a <= 0.005 {
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

fn shape_of(window: &mut Window, runs: &[Vec<Spot>], to_px: impl Fn(Spot) -> Spot, color: Hsla) {
    if runs.is_empty() || color.a <= 0.005 {
        return;
    }
    let mut path = PathBuilder::fill();
    for run in runs {
        let points: Vec<_> = run
            .iter()
            .map(|&spot| {
                let (x, y) = to_px(spot);
                point(px(x), px(y))
            })
            .collect();
        path.add_polygon(&points, true);
    }
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

/// The inks a mark is painted in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    /// What it lies on. A path is painted in an opaque colour, mixed
    /// towards this one: GPUI 0.2 applies a path's opacity twice, so a
    /// see-through one comes out far fainter than it was asked for.
    pub ground: Hsla,
    /// The bowl, and the flakes when they are not in gold.
    pub ink: Hsla,
    /// The flakes at rest, the liquid and what is in the air.
    pub gilt: Hsla,
    /// What the gold dulls to when an update fails.
    pub dull: Hsla,
    /// Room for more: a second flight of sparks when the update is ready.
    pub grand: bool,
}

impl Look {
    /// The mark as the notice shows it: ink and gold on a card.
    pub fn of(t: Theme) -> Self {
        Self {
            ground: t.card,
            ink: t.text,
            gilt: gold(),
            dull: t.muted,
            grand: false,
        }
    }

    /// Part of the way from this look to `other`.
    pub fn towards(self, other: Look, share: f32) -> Self {
        // The ends are themselves, exactly: a colour mixed is a colour
        // converted, and comes back a hair off.
        if share <= 0. {
            return self;
        }
        if share >= 1. {
            return other;
        }
        Self {
            ground: motion::mix(self.ground, other.ground, share),
            ink: motion::mix(self.ink, other.ink, share),
            gilt: motion::mix(self.gilt, other.gilt, share),
            dull: motion::mix(self.dull, other.dull, share),
            grand: if share >= 0.5 { other.grand } else { self.grand },
        }
    }

    /// The faint ink of an empty bowl, the track the gold fills: the ink
    /// at this strength on the ground.
    fn track(&self) -> Hsla {
        motion::mix(self.ground, self.ink, 0.14)
    }
}

/// Paint the mark in `pose`, `size` px square with its box's corner at
/// `origin`. It paints outside that box when something is in the air.
pub fn paint_bowl(window: &mut Window, origin: Spot, size: f32, pose: &BowlPose, look: &Look) {
    let unit = size / 24.;
    let (left, top) = origin;
    let to_px = |spot: Spot| (left + spot.0 * unit, top + spot.1 * unit);
    let fade = |color: Hsla| color.opacity(color.a * pose.ink);
    // The same for a path: towards the ground it lies on.
    let onto = |color: Hsla| motion::mix(look.ground, color, pose.ink);
    let lit = motion::mix(look.gilt, gpui::white(), 0.18 * pose.flash);
    let liquid_ink = motion::mix(lit, look.dull, pose.dull);
    // The ring that spreads, under everything.
    let (radius, width, ink) = pose.halo;
    if ink > 0.01 {
        let middle = to_px(place(pose, MIDDLE));
        let radius = radius * unit;
        window.paint_quad(quad(
            Bounds::new(
                point(px(middle.0 - radius), px(middle.1 - radius)),
                gpui::size(px(radius * 2.), px(radius * 2.)),
            ),
            px(radius),
            gpui::transparent_black(),
            px(width * unit),
            look.gilt.opacity(ink),
            BorderStyle::default(),
        ));
    }
    // The bowl: its shape in the track's ink or the mark's, then what is
    // in it.
    let body: Vec<Spot> = outline().iter().map(|&spot| place(pose, spot)).collect();
    let full = pose.level >= 0.999;
    if !full {
        shape_of(
            window,
            &[body],
            to_px,
            onto(motion::mix(look.track(), look.ink, pose.solid)),
        );
    }
    shape_of(window, &liquid(pose), to_px, onto(liquid_ink));
    let flake_ink = motion::mix(motion::mix(look.ink, look.dull, pose.dull), lit, pose.gilt);
    for (index, flake) in pose.flakes.iter().enumerate() {
        disc(
            window,
            to_px(flake.at),
            FLAKES[index].1 * flake.grow * unit,
            fade(flake_ink.opacity(flake.ink.clamp(0., 1.))),
        );
    }
    let more = if look.grand {
        more_sparks(pose.burst)
    } else {
        [Mote::NONE; 7]
    };
    for mote in pose.motes.iter().chain(&sparks(pose.burst)).chain(&more) {
        disc(
            window,
            to_px(mote.at),
            mote.radius * unit,
            fade(liquid_ink.opacity(mote.ink.clamp(0., 1.))),
        );
    }
}

/// The mark in `pose`, `size` px square. It paints outside its box when
/// something is in the air; nothing of it is laid out.
pub fn bowl(size: f32, pose: BowlPose, t: Theme) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            paint_bowl(
                window,
                (f32::from(bounds.origin.x), f32::from(bounds.origin.y)),
                size,
                &pose,
                &Look::of(t),
            );
        },
    )
    .size(px(size))
    .flex_shrink_0()
}

/// How much of a check mark's stroke is drawn `t` seconds after it began:
/// the short arm first, then the long one, slowing into its end.
pub fn check_drawn(t: f32) -> f32 {
    motion::exit_curve((t / CHECK).clamp(0., 1.))
}

/// How long a check mark takes to draw.
pub const CHECK: f32 = 0.34;

/// The check mark's corners in the icons' 24-unit box (`check.svg`), and
/// how far along its stroke the middle one is.
const CHECK_AT: [Spot; 3] = [(5., 12.), (9., 16.), (19., 6.)];

/// The points of a check mark `drawn` of the way along its stroke (0..=1),
/// in the 24-unit box: from its start, round the corner, to wherever the
/// pen is.
pub fn check_points(drawn: f32) -> Vec<Spot> {
    let [a, b, c] = CHECK_AT;
    let (short, long) = ((b.0 - a.0).hypot(b.1 - a.1), (c.0 - b.0).hypot(c.1 - b.1));
    let along = drawn.clamp(0., 1.) * (short + long);
    if along <= 0. {
        return Vec::new();
    }
    if along <= short {
        return vec![a, lerp_spot(a, b, along / short)];
    }
    vec![a, b, lerp_spot(b, c, (along - short) / long)]
}

/// Paint a check mark `size` px square with its box's corner at `origin`,
/// `drawn` of the way along its stroke, in `color` (opaque: see [`Look`]).
pub fn paint_check(window: &mut Window, origin: Spot, size: f32, drawn: f32, color: Hsla) {
    let unit = size / 24.;
    let points: Vec<_> = check_points(drawn)
        .into_iter()
        .map(|(x, y)| point(px(origin.0 + x * unit), px(origin.1 + y * unit)))
        .collect();
    let Some((first, rest)) = points.split_first() else {
        return;
    };
    // The icons' own weight: a stroke of 1.5 in 24, with round ends and a
    // round corner (a disc on each, the stroke itself being cut square).
    let weight = (1.5 * unit).max(1.2);
    let mut stroke = PathBuilder::stroke(px(weight));
    stroke.move_to(*first);
    for point in rest {
        stroke.line_to(*point);
    }
    if let Ok(stroke) = stroke.build() {
        window.paint_path(stroke, color);
    }
    for point in &points {
        disc(window, (f32::from(point.x), f32::from(point.y)), weight / 2., color);
    }
}

/// The update's state in a dot: gold when an update is known, and while it
/// downloads a faint disc of that gold filling from the bottom to the
/// `level` that has arrived (`None` for the plain dot).
pub fn badge(size: f32, level: Option<f32>) -> AnyElement {
    let Some(level) = level.filter(|level| *level < 0.999) else {
        return crate::ui::update_dot(size).into_any_element();
    };
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let radius = size / 2.;
            let middle = (
                f32::from(bounds.origin.x) + radius,
                f32::from(bounds.origin.y) + radius,
            );
            disc(window, middle, radius, gold().opacity(0.3));
            let round: Vec<Spot> = (0..32)
                .map(|step| {
                    let angle = TAU * step as f32 / 32.;
                    (middle.0 + radius * angle.cos(), middle.1 + radius * angle.sin())
                })
                .collect();
            let surface = middle.1 + radius - 2. * radius * level.clamp(0., 1.);
            let mut filled = Vec::new();
            clip(
                &round,
                (middle.0 - radius, surface),
                (middle.0 + radius, surface),
                &mut filled,
            );
            if filled.len() >= 3 {
                shape_of(window, &[filled], |spot| spot, gold());
            }
        },
    )
    .size(px(size))
    .flex_shrink_0()
    .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    // `gpui::*` re-exports its own `test` attribute; these are plain unit tests.
    use std::prelude::v1::test;
    use std::time::Duration;

    fn area(polygon: &[Spot]) -> f32 {
        let mut twice = 0.;
        for (index, a) in polygon.iter().enumerate() {
            let b = polygon[(index + 1) % polygon.len()];
            twice += a.0 * b.1 - b.0 * a.1;
        }
        twice.abs() / 2.
    }

    fn filled(pose: &BowlPose) -> f32 {
        liquid(pose).iter().map(|run| area(run)).sum()
    }

    #[test]
    fn the_outline_is_the_icons_ring() {
        let outline = outline();
        assert!(outline.len() > 200, "{} points", outline.len());
        for &(x, y) in outline {
            let from_middle = (x - MIDDLE.0).hypot(y - MIDDLE.1);
            assert!(from_middle <= RING + 0.01, "({x}, {y}) is outside the ring");
            // The hole in the middle is no smaller than this.
            assert!(from_middle >= 4., "({x}, {y}) is inside the hole");
        }
        let left = outline.iter().map(|spot| spot.0).fold(f32::MAX, f32::min);
        assert!((left - (MIDDLE.0 - RING)).abs() < 0.01);
    }

    #[test]
    fn a_cut_keeps_the_side_under_the_line() {
        let square = [(0., 0.), (4., 0.), (4., 4.), (0., 4.)];
        let mut kept = Vec::new();
        // Under a level line a quarter of the way down.
        clip(&square, (0., 1.), (4., 1.), &mut kept);
        assert!((area(&kept) - 12.).abs() < 1e-4);
        assert!(kept.iter().all(|spot| spot.1 >= 1. - 1e-5));
        // Under a leaning one: half of it.
        clip(&square, (0., 0.), (4., 4.), &mut kept);
        assert!((area(&kept) - 8.).abs() < 1e-4);
        // A line above all of it keeps all of it, one below it nothing.
        clip(&square, (0., -1.), (4., -1.), &mut kept);
        assert!((area(&kept) - 16.).abs() < 1e-4);
        clip(&square, (0., 5.), (4., 5.), &mut kept);
        assert!(kept.is_empty());
    }

    #[test]
    fn the_bowl_fills_from_the_bottom_and_more_for_every_bit_more() {
        let whole = area(outline());
        let at = |level: f32| {
            let mut pose = BowlPose::REST;
            pose.level = level;
            pose
        };
        assert_eq!(filled(&at(0.)), 0.);
        assert!((filled(&at(1.)) - whole).abs() < 1e-3);
        let mut before = 0.;
        for step in 1..=100 {
            let pose = at(step as f32 / 100.);
            let now = filled(&pose);
            assert!(now >= before - 1e-3, "less at {step}% than before");
            assert!(now <= whole + 1e-2);
            before = now;
            // Nothing of it is above its surface.
            for &(x, y) in liquid(&pose).iter().flatten() {
                assert!(y >= surface(&pose, x) - 1e-3, "{step}%: ({x}, {y})");
            }
        }
        // Half way up is a little more than half of it: most of the bite is
        // above the middle.
        let half = filled(&at(0.5)) / whole;
        assert!((0.5..0.68).contains(&half), "{half}");
    }

    #[test]
    fn the_surface_rocks_but_holds_what_it_holds() {
        // Leaning and rippling move the liquid about; they do not make
        // much more or less of it.
        let mut pose = BowlPose::REST;
        pose.level = 0.4;
        let flat = filled(&pose);
        for tick in 0..40 {
            let rocked = pouring(POUR_AFTER + tick as f32 / 40., 0.4, 1.);
            assert!((filled(&rocked) / flat - 1.).abs() < 0.12, "tick {tick}");
        }
        // Empty and full, it is flat: there is nothing to rock.
        for level in [0., 1.] {
            let pose = pouring(2.3, level, 1.);
            assert_eq!((pose.slope, pose.wave), (0., 0.));
        }
    }

    #[test]
    fn the_level_follows_without_steps_and_never_runs_ahead() {
        // A download that arrives in lumps, ten times a second.
        let mut follow = Follow::default();
        let (mut before, mut largest) = (0., 0f32);
        for frame in 0..(3 * 120) {
            let seconds = frame as f32 / 120.;
            let target = ((seconds * 10.).floor() / 10. * 0.4).min(1.);
            follow.step(target, 1. / 120., FOLLOW_PACE);
            assert!(follow.at <= target + 1e-6, "ahead at {seconds}");
            assert!(follow.at >= before - 1e-6, "back at {seconds}");
            largest = largest.max(follow.at - before);
            before = follow.at;
        }
        // The lumps are 4% each; no frame moves a sixth of one.
        assert!(largest < 0.0066, "{largest}");
        assert!((follow.at - 1.).abs() < 0.01);
        // The same at any frame rate: two short steps are one long one.
        let (mut fine, mut coarse) = (Follow::default(), Follow::default());
        for _ in 0..30 {
            fine.step(0.7, 1. / 120., FOLLOW_PACE);
            fine.step(0.7, 1. / 120., FOLLOW_PACE);
            coarse.step(0.7, 1. / 60., FOLLOW_PACE);
        }
        assert!((fine.at - coarse.at).abs() < 1e-4);
        assert!((fine.speed - coarse.speed).abs() < 1e-3);
        // A frame that took no time changes nothing.
        let held = fine;
        fine.step(1., 0., FOLLOW_PACE);
        assert_eq!(fine, held);
    }

    #[test]
    fn the_arrival_starts_out_of_sight_and_ends_as_the_mark() {
        let start = found(0.);
        assert_eq!(start.ink, 0.);
        assert!((start.shift.1 + FALL_FROM).abs() < 1e-4);
        assert_eq!(found(FOUND_LENGTH), BowlPose::REST);
        // No jump from one frame to the next, at 120 Hz.
        let mut before = start;
        let mut lowest: f32 = 0.;
        let mut hopped = [0f32; 2];
        for frame in 1..=(FOUND_LENGTH * 120.) as usize + 2 {
            let pose = found(frame as f32 / 120.);
            assert!((pose.shift.1 - before.shift.1).abs() < 0.8, "frame {frame}");
            assert!((pose.scale.1 - before.scale.1).abs() < 0.05, "frame {frame}");
            for index in 0..2 {
                let moved = (pose.flakes[index].at.1 - before.flakes[index].at.1).abs();
                assert!(moved < 1., "flake {index} at frame {frame}");
                hopped[index] = hopped[index].max(FLAKES[index].0.1 - pose.flakes[index].at.1);
            }
            // It lands on the ground and squashes against it, never
            // through it.
            let bottom = MIDDLE.1 + pose.shift.1 + RING * pose.scale.1;
            assert!(bottom <= MIDDLE.1 + RING + 1e-3, "frame {frame}: {bottom}");
            lowest = lowest.max(1. - pose.scale.1);
            before = pose;
        }
        assert!(lowest > 0.08, "it lands flat: {lowest}");
        assert!(hopped[0] > 2.5 && hopped[1] > 2.5, "the flakes hop: {hopped:?}");
    }

    #[test]
    fn the_pop_rings_once_and_comes_to_rest() {
        assert_eq!(kick(0.), 0.);
        assert_eq!(kick(KICK), 0.);
        let (mut most, mut least) = (0f32, 0f32);
        let mut before = 0.;
        for frame in 0..=(KICK * 240.) as usize {
            let now = kick(frame as f32 / 240.);
            most = most.max(now);
            least = least.min(now);
            assert!((now - before).abs() < 0.2, "frame {frame}");
            before = now;
        }
        assert!((most - 1.).abs() < 0.01, "{most}");
        // It swings back past rest, a little: an overshoot, not a shake.
        assert!((-0.3..-0.05).contains(&least), "{least}");
        let rest = ready(READY_LENGTH);
        assert_eq!(rest.scale, (1., 1.));
        assert_eq!((rest.level, rest.halo.2, rest.burst, rest.flash), (1., 0., 1., 0.));
        assert_eq!(rest.flakes, BowlPose::REST.flakes);
        // At its peak the bowl is bigger and the flakes are further out.
        let peak = (0..200)
            .map(|tick| ready(tick as f32 / 1000.))
            .max_by(|a, b| a.scale.0.total_cmp(&b.scale.0))
            .unwrap();
        assert!(peak.scale.0 > 1.15);
        let flung = ready(0.16);
        for index in 0..2 {
            assert!(flung.flakes[index].at.0 > FLAKES[index].0.0 + 1.);
        }
        // The ring spreads and fades; the sparks shrink to nothing.
        assert!(ready(0.3).halo.0 > ready(0.05).halo.0);
        assert!(ready(0.3).halo.2 < ready(0.05).halo.2);
        assert!(sparks(0.99).iter().all(|spark| spark.radius < 0.1));
        assert!(sparks(1.).iter().all(|spark| spark.ink == 0.));
    }

    #[test]
    fn a_spill_tips_gently_and_runs_dry() {
        let mut before = spilled(0., 0.6);
        assert_eq!(before.turn, 0.);
        assert!((before.level - 0.6).abs() < 1e-6);
        let mut furthest: f32 = 0.;
        for frame in 1..=(SPILL_LENGTH * 120.) as usize {
            let pose = spilled(frame as f32 / 120., 0.6);
            // No shake: it goes over once and at most settles back a little.
            assert!(pose.turn >= before.turn - 0.002, "frame {frame}");
            assert!(pose.level <= before.level + 1e-6);
            furthest = furthest.max(pose.turn);
            before = pose;
        }
        assert!(furthest <= TIP * 1.08, "{furthest}");
        let end = spilled(SPILL_LENGTH, 0.6);
        assert!((end.turn - TIP).abs() < 1e-4);
        assert_eq!((end.level, end.dull, end.gilt), (0., 1., 0.));
        assert!(end.motes.iter().all(|mote| mote.ink <= 0.01));
        // An empty bowl has nothing to drip.
        assert!(spilled(0.4, 0.).motes.iter().all(|mote| mote.ink == 0.));
        assert!(spilled(0.4, 0.6).motes.iter().any(|mote| mote.ink > 0.3));
    }

    #[test]
    fn one_part_gives_way_to_the_next_without_a_jump() {
        // Mid-spin into the pop: the bowl goes the short way round.
        let was = proving(EMERGE + 0.45);
        let first = ready(0.);
        let start = handover(&was, &first, &first, 0.);
        assert!((start.turn - (was.turn - was.turn.round())).abs() < 1e-5);
        assert!((start.flakes[0].at.0 - was.flakes[0].at.0).abs() < 1e-4);
        assert!((start.flakes[0].at.1 - was.flakes[0].at.1).abs() < 1e-4);
        assert_eq!(handover(&was, &first, &first, 1.), first);
        let mut before = start;
        let mut biggest: f32 = 0.;
        for step in 1..=26 {
            let pose = handover(&was, &first, &ready(step as f32 / 120.), step as f32 / 26.);
            assert!((pose.turn - before.turn).abs() < 0.06, "step {step}");
            biggest = biggest.max(pose.scale.0);
            before = pose;
        }
        // The new part plays in full while the old one is let go of.
        assert!(biggest > 1.15, "{biggest}");
        // A flake that was under comes up where the new part has it.
        let mut under = BowlPose::REST;
        under.flakes[0].ink = 0.;
        under.flakes[0].at = (0., 0.);
        let up = handover(&under, &BowlPose::REST, &BowlPose::REST, 0.3);
        assert_eq!(up.flakes[0].at, FLAKES[0].0);
    }

    #[test]
    fn the_pour_plays_the_updates_parts_in_order() {
        let start = Instant::now();
        let at = |seconds: f32| start + motion::slow(Duration::from_secs_f32(seconds));
        let mut pour = Pour::default();
        assert_eq!((pour.act(), pour.pose()), (Act::Still, BowlPose::REST));
        // Found: it arrives, then rests.
        pour.arrive(Act::Found, 0., at(0.));
        assert!(pour.frame(Act::Found, 0., at(0.), false));
        assert_eq!(pour.pose().ink, 0.);
        assert!(!pour.frame(Act::Found, 0., at(FOUND_LENGTH + 0.1), false));
        assert_eq!(pour.pose(), BowlPose::REST);
        // Downloading: the level follows what arrives, frame by frame.
        let mut clock = FOUND_LENGTH + 0.2;
        let mut before = 0.;
        let mut poured = 0;
        for frame in 0..(4 * 60) {
            let share = (frame as f32 / 200.).min(1.);
            clock += 1. / 60.;
            assert!(pour.frame(Act::Pouring, share, at(clock), false));
            let (level, speed) = pour.level();
            assert!(level <= share + 1e-5 && level >= before - 1e-5, "frame {frame}");
            assert!(speed >= 0.);
            assert_eq!(pour.pose().level, level);
            poured += pour.pose().motes.iter().filter(|mote| mote.ink > 0.).count();
            before = level;
        }
        assert!(before > 0.99, "{before}");
        assert!(poured > 30, "flakes fall while it pours: {poured}");
        // The flakes went into the bowl.
        assert!(pour.pose().flakes.iter().all(|flake| flake.ink == 0.));
        // Verifying: full, and going round for as long as it takes.
        for _ in 0..90 {
            clock += 1. / 60.;
            assert!(pour.frame(Act::Proving, 1., at(clock), false));
        }
        assert!(pour.pose().level > 0.999);
        assert!(pour.pose().flakes.iter().all(|flake| flake.ink > 0.99));
        // Ready: the pop, then the full gold mark at rest.
        clock += 1. / 60.;
        assert!(pour.frame(Act::Ready, 1., at(clock), false));
        let mut biggest: f32 = 0.;
        for _ in 0..(READY_LENGTH * 60.) as usize + 20 {
            clock += 1. / 60.;
            pour.frame(Act::Ready, 1., at(clock), false);
            biggest = biggest.max(pour.pose().scale.0);
        }
        assert!(biggest > 1.15);
        assert!(!pour.moving());
        assert_eq!(pour.pose(), ready(READY_LENGTH));
    }

    #[test]
    fn a_failure_spills_from_where_the_bowl_was() {
        let start = Instant::now();
        let at = |seconds: f32| start + motion::slow(Duration::from_secs_f32(seconds));
        let mut pour = Pour::default();
        pour.arrive(Act::Pouring, 0.5, at(0.));
        assert_eq!(pour.level().0, 0.5);
        pour.frame(Act::Pouring, 0.5, at(0.1), false);
        pour.frame(Act::Spilled, 0.5, at(0.2), false);
        assert!((pour.pose().level - 0.5).abs() < 0.01);
        assert!(pour.moving());
        assert!(!pour.frame(Act::Spilled, 0.5, at(0.3 + SPILL_LENGTH), false));
        let end = pour.pose();
        assert_eq!((end.level, end.dull), (0., 1.));
        assert!((end.turn - TIP).abs() < 1e-4);
        // Trying again fills it from empty, and stands it back up.
        let mut clock = 0.4 + SPILL_LENGTH;
        pour.frame(Act::Pouring, 0., at(clock), false);
        assert_eq!(pour.level(), (0., 0.));
        for frame in 0..60 {
            clock += 1. / 60.;
            pour.frame(Act::Pouring, frame as f32 / 100., at(clock), false);
            assert!(pour.level().0 <= frame as f32 / 100. + 1e-5);
        }
        assert!(pour.pose().turn.abs() < 1e-4);
        assert!(pour.level().0 > 0.3);
    }

    #[test]
    fn reduced_motion_shows_each_part_as_a_still_picture() {
        let start = Instant::now();
        let mut pour = Pour::default();
        pour.arrive(Act::Found, 0., start);
        for (act, share) in [
            (Act::Found, 0.),
            (Act::Pouring, 0.25),
            (Act::Pouring, 0.5),
            (Act::Proving, 1.),
            (Act::Ready, 1.),
            (Act::Spilled, 1.),
            (Act::Seeking, 0.),
        ] {
            for frame in 0..3 {
                let now = start + Duration::from_millis(40 * frame);
                assert!(!pour.frame(act, share, now, true), "{act:?} moves");
                let pose = pour.pose();
                // Nothing is displaced, turned, stretched or in the air.
                assert_eq!((pose.shift, pose.turn, pose.scale), ((0., 0.), 0., (1., 1.)));
                assert_eq!((pose.slope, pose.wave, pose.halo.2, pose.burst), (0., 0., 0., 1.));
                assert!(pose.motes.iter().all(|mote| mote.ink == 0.));
                for (index, flake) in pose.flakes.iter().enumerate() {
                    assert_eq!((flake.at, flake.grow), (FLAKES[index].0, 1.));
                }
                assert_eq!(pose, still(act, share));
            }
        }
        // The level is the share that has arrived, at once.
        pour.frame(Act::Pouring, 0.37, start, true);
        assert_eq!(pour.pose().level, 0.37);
        assert_eq!(still(Act::Ready, 0.).level, 1.);
        assert_eq!(still(Act::Spilled, 0.8).level, 0.);
    }

    #[test]
    fn a_full_bowl_that_is_to_be_empty_runs_out() {
        // Ready, then put back as the mark: the gold runs out over the
        // handover; it is not gone in one frame.
        let was = pose(Act::Ready, READY_LENGTH, 1., 0., 0.);
        let (first, to) = (BowlPose::REST, BowlPose::REST);
        let mut before = was.level;
        for step in 0..=20 {
            let now = handover(&was, &first, &to, step as f32 / 20.).level;
            assert!(now <= before && before - now < 0.2, "step {step}: {before} to {now}");
            before = now;
        }
        assert_eq!(before, 0.);
        // A part that starts with what was there keeps its own level.
        let pouring = pose(Act::Pouring, 2., 0.4, 0.5, 0.);
        let proving = pose(Act::Proving, 0., 0.4, 0., 0.);
        assert_eq!(handover(&pouring, &proving, &proving, 0.3).level, 0.4);
        // And one that starts fuller is as full as it says at once.
        let found = pose(Act::Found, FOUND_LENGTH, 0., 0., 0.);
        let ready = pose(Act::Ready, 0., 1., 0., 0.);
        assert_eq!(handover(&found, &ready, &ready, 0.1).level, 1.);
    }

    #[test]
    fn the_nod_is_small_and_ends_as_the_mark() {
        assert_eq!(pose(Act::Pleased, 0., 0., 0., 0.), BowlPose::REST);
        assert_eq!(pose(Act::Pleased, PLEASED_LENGTH, 0., 0., 0.), BowlPose::REST);
        assert_eq!(length(Act::Pleased), Some(PLEASED_LENGTH));
        assert_eq!(still(Act::Pleased, 0.3), BowlPose::REST);
        let (mut biggest, mut highest, mut before) = (1f32, f32::MAX, BowlPose::REST);
        for step in 0..=200 {
            let now = pose(Act::Pleased, PLEASED_LENGTH * step as f32 / 200., 0., 0., 0.);
            // Nothing is poured, lit or thrown: it is only a nod.
            assert_eq!((now.level, now.halo.2, now.burst, now.solid), (0., 0., 1., 1.));
            biggest = biggest.max(now.scale.0);
            highest = highest.min(now.flakes[0].at.1).min(now.flakes[1].at.1);
            // No jump from one frame to the next.
            assert!((now.scale.0 - before.scale.0).abs() < 0.02, "step {step}");
            for (flake, was) in now.flakes.iter().zip(&before.flakes) {
                assert!((flake.at.1 - was.at.1).abs() < 0.3, "step {step}");
            }
            before = now;
        }
        // A third of the pop, and the flakes hop as they do when found.
        assert!((1.05..1.08).contains(&biggest), "{biggest}");
        assert!(highest < FLAKES[0].0.1 - 2.5, "{highest}");
    }

    #[test]
    fn the_second_flight_of_sparks_leaves_later_flies_further_and_is_gone() {
        assert!(more_sparks(0.).iter().chain(&more_sparks(0.1)).all(|spark| spark.radius == 0.));
        assert!(more_sparks(1.).iter().all(|spark| spark.radius == 0.));
        let (first, second) = (sparks(0.6), more_sparks(0.6));
        let reach = |mote: &Mote| (mote.at.0 - MIDDLE.0).hypot(mote.at.1 - MIDDLE.1);
        let furthest = |motes: &[Mote]| motes.iter().map(reach).fold(0f32, f32::max);
        assert!(furthest(&second) > furthest(&first));
        // Smaller than the first, and clear of the bowl at its biggest.
        for spark in &second {
            assert!(spark.radius > 0. && spark.radius < first[0].radius);
            assert!(reach(spark) - spark.radius > RING * 1.2);
        }
    }

    #[test]
    fn a_check_is_drawn_from_its_short_arm_round_the_corner() {
        assert!(check_points(0.).is_empty());
        // Part of the short arm.
        let part = check_points(0.1);
        assert_eq!((part.len(), part[0]), (2, (5., 12.)));
        assert!(part[1].0 > 5. && part[1].0 < 9. && part[1].1 > 12. && part[1].1 < 16.);
        // Round the corner, on the way up.
        let more = check_points(0.6);
        assert_eq!((more.len(), more[1]), (3, (9., 16.)));
        assert!(more[2].1 < 16. && more[2].1 > 6.);
        // All of it is the icon's check.
        assert_eq!(check_points(1.), vec![(5., 12.), (9., 16.), (19., 6.)]);
        assert_eq!(check_points(7.), check_points(1.));
        // Its pen starts at once and slows into the end.
        assert_eq!((check_drawn(-1.), check_drawn(0.), check_drawn(CHECK), check_drawn(9.)), (0., 0., 1., 1.));
        assert!(check_drawn(CHECK / 2.) > 0.5);
    }

    #[test]
    fn a_look_gives_way_to_another_by_shares() {
        let t = crate::ui::Theme::new(false);
        let rest = Look {
            ink: t.accent,
            gilt: t.accent,
            ..Look::of(t)
        };
        let grand = Look {
            grand: true,
            ..Look::of(t)
        };
        assert_eq!(rest.towards(grand, 0.), rest);
        assert_eq!(rest.towards(grand, 1.), grand);
        assert_eq!(rest.towards(grand, -3.), rest);
        let half = rest.towards(grand, 0.5);
        assert!(half.grand);
        assert_ne!(half.ink, rest.ink);
        assert_ne!(half.ink, grand.ink);
    }

    #[test]
    fn a_poured_flake_falls_into_the_bowl_and_goes_under() {
        assert_eq!(falling(8., 0.7, -0.1, 20.), Mote::NONE);
        assert_eq!(falling(8., 0.7, POUR_LIFE, 20.), Mote::NONE);
        let mut before = falling(8., 0.7, 0., 20.);
        assert_eq!(before.ink, 0.);
        for tick in 1..40 {
            let now = falling(8., 0.7, tick as f32 * POUR_LIFE / 40., 20.);
            assert!(now.at.1 > before.at.1);
            before = now;
        }
        // Under by the time it is at the surface, wherever that is.
        for floor in [4., 12., 20.] {
            for tick in 0..200 {
                let flake = falling(8., 0.7, tick as f32 * POUR_LIFE / 200., floor);
                if flake.at.1 >= floor {
                    assert_eq!(flake.ink, 0., "floor {floor}");
                }
            }
        }
        // Spread across the bowl, inside its rim.
        let places: Vec<f32> = (1..=40).map(|count| poured(count, 0.).x).collect();
        assert!(places.iter().all(|x| (x - MIDDLE.0).abs() < RING * 0.75));
        assert!(places.iter().any(|x| *x < MIDDLE.0 - 4.) && places.iter().any(|x| *x > MIDDLE.0 + 4.));
    }

    #[test]
    fn digits_roll_to_the_next_and_carry() {
        // At rest on whole numbers.
        assert_eq!(wheel(43., 0, 5.), 43.);
        assert_eq!(wheel(43., 1, 5.), 4.);
        // Slow: a digit stays put until just before it changes, then rolls.
        let slow = 1.;
        assert_eq!(wheel(43.5, 0, slow), 43.);
        let late = wheel(43.9, 0, slow);
        assert!(late > 43.1 && late < 44., "{late}");
        // The tens wait for the units to come round, and roll with them.
        assert_eq!(wheel(48.9, 1, slow), 4.);
        let carrying = wheel(49.9, 1, slow);
        assert!(carrying > 4.1 && carrying < 5., "{carrying}");
        // Fast, a digit still rests for part of its time.
        assert_eq!(wheel(43.3, 0, 120.), 43.);
        assert!(wheel(43.8, 0, 120.) > 43.2);
        // At any speed the wheel never goes back or jumps.
        for speed in [0.5, 4., 25., 120.] {
            for place in 0..3 {
                let mut before = 0.;
                let step = f64::from(speed) / 120.;
                let mut value = 0.;
                while value < 130. {
                    let now = wheel(value, place, speed);
                    // The steepest part of a roll, over the window it has.
                    let window = f64::from(speed * ROLL).clamp(0.02, 1. - f64::from(ROLL_REST));
                    let most = 1.5 * step / window * 1.05;
                    assert!(now >= before - 1e-9, "{place} at {value}");
                    assert!(now - before <= most + 1e-9, "{place} at {value}: {}", now - before);
                    before = now;
                    value += step;
                }
            }
        }
        // The faces: no zero in front of a number, a zero before the point.
        assert_eq!(faces(4.0, 1, 0), (Some(4), Some(5), 0.));
        assert_eq!(faces(0.5, 1, 0), (None, Some(1), 0.5));
        assert_eq!(faces(0.5, 1, 1), (Some(0), Some(1), 0.5));
        assert_eq!(faces(9.25, 0, 0), (Some(9), Some(0), 0.25));
        // What is drawn: one face out as the other comes in.
        assert_eq!(rolled(0.), ((0., 1.), (1., 0.)));
        assert_eq!(rolled(1.), ((-1., 0.), (0., 1.)));
        // Half way, both are in the window and both can be read.
        let ((up, leaving), (down, arriving)) = rolled(0.5);
        assert_eq!((up, down), (-0.5, 0.5));
        assert!(leaving > 0.7 && arriving > 0.7);
    }

    #[test]
    fn a_counter_has_a_wheel_for_every_digit_it_will_reach() {
        let counter = |value: f64, most: f64, decimals: u32| Counter {
            value,
            speed: 3.,
            decimals,
            most,
        };
        // 7 of 48: the tens wheel is there, blank, and takes no room.
        let wheels = counter(7., 48., 0).wheels();
        assert_eq!(wheels.len(), 2);
        assert_eq!(wheels[0], (None, Some(1), 0., 0.));
        assert_eq!(wheels[1].0, Some(7));
        // 100%.
        let wheels = counter(100., 100., 0).wheels();
        assert_eq!(wheels.iter().map(|wheel| wheel.0).collect::<Vec<_>>(), [Some(1), Some(0), Some(0)]);
        assert!(wheels.iter().all(|wheel| wheel.2 == 0. && wheel.3 == 1.));
        // 0.4 with a decimal: the zero before the point is shown.
        let wheels = counter(4., 98., 1).wheels();
        assert_eq!(wheels.iter().map(|wheel| wheel.0).collect::<Vec<_>>(), [Some(0), Some(4)]);
        // A first digit rolling in widens the number little by little.
        let mut before = 1.;
        for tick in 0..=100 {
            let width: f32 = counter(9. + f64::from(tick) / 100., 48., 0)
                .wheels()
                .iter()
                .map(|wheel| wheel.3)
                .sum();
            assert!(width >= before - 1e-6 && width - before < 0.2, "tick {tick}");
            before = width;
        }
        assert_eq!(before, 2.);
    }
}
