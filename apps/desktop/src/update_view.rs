//! "Check for updates": the menu item, the row of Settings → About and the
//! notice that follows an update from the question to the restart. The work
//! itself is in `update.rs`.
//!
//! What ends at once (up to date, a failure) is said in a toast. What waits
//! for the person or takes a while (a version is available, downloading,
//! verifying, restart to finish) is a notice in the toast's place and face,
//! which stays until it is answered. The notice leads with the mark, which
//! plays the update's part (`update_bowl.rs`): found, pouring, proving,
//! ready, spilt. Its one action is one capsule from start to end: "Update",
//! then the share that has arrived, then "Restart to update".
//!
//! Settings → About is where an update is taken on purpose, and there its
//! version row is the stage (`update_about.rs`): the same update, told at
//! full size. While that page is the one on screen the notice in the corner
//! is not shown, so there is one stage and never two; both read the same
//! state here, and the notice is back in its corner when the page is left.
//!
//! The app also asks by itself, quietly: once an account is ready, every few
//! hours after that, and when its window comes back after a long gap. A
//! quiet check says nothing unless it finds a version; then the same notice
//! comes up, once for each version in a run.
use crate::{
    app::Cereal,
    motion::{self, GOOEY, SNAPPY},
    ui::*,
    update::{self, Check, Offer, Problem},
    update_bowl::{self, Act, Counter, Pour},
};
use gpui::{prelude::*, *};
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant, SystemTime},
};

/// A quiet check that got its answer is not repeated for this long.
const QUIET_INTERVAL: Duration = Duration::from_secs(4 * 60 * 60);
/// Coming back to the window asks again after this long without a check.
const FOCUS_GAP: Duration = Duration::from_secs(60 * 60);
/// A quiet check that got no answer is tried again after this long.
const QUIET_RETRY: Duration = Duration::from_secs(15 * 60);
/// How often the app looks at whether a quiet check is due.
const QUIET_TICK: Duration = Duration::from_secs(60);

/// What brings up the question of a quiet check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    /// The clock: at launch, then once a minute.
    Tick,
    /// The window became the active one again.
    Focus,
}

/// Whether a quiet check is due. `last` is how long ago the last check of
/// any kind ended and whether it got an answer; `None` before the first
/// one, which is the check at launch.
pub fn check_due(last: Option<(Duration, bool)>, trigger: Trigger) -> bool {
    match last {
        None => true,
        Some((age, false)) => age >= QUIET_RETRY,
        Some((age, true)) => {
            age >= match trigger {
                Trigger::Tick => QUIET_INTERVAL,
                Trigger::Focus => FOCUS_GAP,
            }
        }
    }
}

/// A verified update waiting for the restart that installs it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Staged {
    version: String,
    /// The unpacked, verified app.
    app: PathBuf,
    /// The running app it replaces, and the team that signed both.
    bundle: PathBuf,
    team: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum Phase {
    #[default]
    Idle,
    Checking,
    Available(Offer),
    Downloading(Offer),
    Verifying(Offer),
    Ready(Staged),
    Restarting,
}

/// What the notice says: a title, a line under it, whether work is going
/// on, and the action it offers beside "Later".
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Notice {
    title: String,
    detail: Option<String>,
    busy: bool,
    action: Option<&'static str>,
    /// The part the mark plays.
    act: Act,
    /// The size of the download, while the line under the title counts it.
    counts: Option<u64>,
}

/// How long after the bowl pops the capsule opens into its action, in
/// seconds: the pop's first swing.
const READY_CUE: f32 = 0.2;

/// How long "You're up to date" is said in Settings before its row is as it
/// was, in seconds.
pub const PLEASED_FOR: f32 = 4.;

/// What the update's capsule shows: an action to take, or how much of the
/// download has arrived.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub(crate) enum Face {
    #[default]
    None,
    /// The thing to do next, filled.
    Label(&'static str),
    /// Something that can be asked for, or that is going on: outlined.
    Plain(&'static str),
    Percent,
}

impl Face {
    fn label(self) -> Option<&'static str> {
        match self {
            Face::Label(label) | Face::Plain(label) => Some(label),
            Face::None | Face::Percent => None,
        }
    }

    pub(crate) fn filled(self) -> bool {
        matches!(self, Face::Label(_))
    }
}

/// Where a capsule of the update is drawn: each place has its own tracks,
/// so the two can be on screen in the same frame while one of them leaves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Seat {
    /// The notice in the corner.
    Notice,
    /// The version row of Settings → About.
    About,
}

impl Seat {
    /// The tracks of its width, its fill and what it says.
    fn keys(self) -> [motion::TrackKey; 3] {
        let name = match self {
            Seat::Notice => "update-capsule",
            Seat::About => "about-capsule",
        };
        [(name, 0), (name, 1), (name, 2)]
    }

    /// Its id as a button, its id as a figure, and the name it is placed
    /// under for scripted runs that press it.
    fn names(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Seat::Notice => ("update-action", "update-progress", "update-action"),
            Seat::About => ("settings-check-updates", "settings-update-progress", ABOUT_ACTION),
        }
    }
}

/// The name the row's capsule in Settings → About is placed under.
pub const ABOUT_ACTION: &str = "about-update-action";

// ---------------------------------------------------------------------------
// How fast a download is arriving
// ---------------------------------------------------------------------------

/// How far back the speed of a download is measured over, how far apart its
/// samples are kept, and how much of it must have been seen before a speed
/// is said, in seconds.
const RATE_WINDOW: f32 = 3.;
const RATE_APART: f32 = 0.1;
const RATE_NEEDS: f32 = 1.;
/// The speed that is said is read again this often, so it can be read.
const RATE_SAID_EVERY: f32 = 0.5;

/// How fast a download is arriving, from what has been seen of it: the
/// bytes on disk at moments a known time apart. Nothing is assumed: until
/// enough has been seen, and whenever nothing is arriving, it has no speed
/// to give and the row says none.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Rate {
    /// The seconds into the download and the bytes that had arrived by
    /// then, oldest first.
    samples: VecDeque<(f32, u64)>,
}

impl Rate {
    /// `bytes` had arrived `at` seconds into the download.
    pub fn sample(&mut self, at: f32, bytes: u64) {
        if let Some(&(last_at, last_bytes)) = self.samples.back() {
            if bytes < last_bytes || at < last_at {
                // Another download.
                self.samples.clear();
            } else if at - last_at < RATE_APART {
                return;
            }
        }
        self.samples.push_back((at, bytes));
        // The oldest kept is the last one from before the window.
        while self.samples.len() > 2 && at - self.samples[1].0 >= RATE_WINDOW {
            self.samples.pop_front();
        }
    }

    /// Bytes per second over what has been seen lately; `None` until
    /// [`RATE_NEEDS`] seconds have been seen, and while nothing arrives.
    pub fn speed(&self) -> Option<f64> {
        let (&(from, before), &(to, after)) = (self.samples.front()?, self.samples.back()?);
        let span = to - from;
        if span < RATE_NEEDS || after <= before {
            return None;
        }
        Some((after - before) as f64 / f64::from(span))
    }

    /// The seconds the rest of a download of `total` bytes would take at
    /// that speed.
    pub fn left(&self, total: u64) -> Option<f32> {
        let speed = self.speed()?;
        let arrived = self.samples.back()?.1;
        Some((total.saturating_sub(arrived) as f64 / speed) as f32)
    }
}

/// A speed the way it is said: "9.6 MB/s", "12 MB/s", "640 KB/s".
pub fn speed_text(bytes_per_second: f64) -> String {
    let megabytes = bytes_per_second / 1_000_000.;
    if megabytes >= 9.95 {
        format!("{megabytes:.0} MB/s")
    } else if megabytes >= 0.95 {
        format!("{megabytes:.1} MB/s")
    } else {
        format!("{:.0} KB/s", (bytes_per_second / 1_000.).max(1.))
    }
}

/// The time a download has left the way it is said, and never more exactly
/// than it is known: to the second under ten, to five seconds under a
/// minute, to the minute after. `None` for over an hour, which is no
/// estimate at all.
pub fn left_text(seconds: f32) -> Option<String> {
    if !seconds.is_finite() || seconds < 0. || seconds > 3600. {
        return None;
    }
    Some(if seconds < 9.5 {
        format!("about {} s left", seconds.ceil().max(1.) as u32)
    } else if seconds < 57.5 {
        format!("about {} s left", ((seconds / 5.).round() * 5.) as u32)
    } else {
        format!("about {} min left", (seconds / 60.).round().max(1.) as u32)
    })
}

/// What is said of a download's pace after its count: "9.6 MB/s · about 4 s
/// left". `None` when there is nothing honest to say.
pub fn pace_text(rate: &Rate, total: u64) -> Option<String> {
    let speed = speed_text(rate.speed()?);
    Some(match rate.left(total).and_then(left_text) {
        Some(left) => format!("{speed} · {left}"),
        None => speed,
    })
}

/// What the version row of Settings → About shows.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct AboutRow {
    /// Grown into the update's card: an update is offered, under way,
    /// ready, or has failed.
    pub open: bool,
    pub title: String,
    /// The line under the title, in plain words.
    pub detail: String,
    /// The size of the download, while that line counts it.
    pub counts: Option<u64>,
    /// The download's pace, said after the count when it is known.
    pub pace: Option<String>,
    /// The part the mark plays.
    pub act: Act,
    /// The version this is and the one the update brings.
    pub pair: Option<(String, String)>,
    /// The update is in hand: the pair is only the version it brings.
    pub resolved: bool,
    /// The title is of work going on with nothing to count.
    pub sheen: bool,
    /// A check before the line: asked, and nothing is newer.
    pub checked: bool,
    /// The download's line along the row's bottom edge.
    pub line: bool,
}

/// An update that was under way and did not finish, as the notice says it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Failure {
    title: &'static str,
    detail: String,
    /// The version that was being fetched, to fetch again.
    offer: Option<Offer>,
}

/// A failure in two short lines: what happened, and what that means.
fn failure_lines(problem: &Problem) -> (&'static str, String) {
    match problem {
        Problem::Download => ("Download failed", "Check your connection.".into()),
        Problem::Unverified => ("Update not verified", "Nothing was installed.".into()),
        Problem::Install => ("Update not installed", "Cereal was left as it was.".into()),
        Problem::NeedsSystem(version) => (
            "Update not installed",
            format!("It needs macOS {version} or later."),
        ),
        Problem::MoveToApplications => (
            "Update not installed",
            "Move Cereal to Applications first.".into(),
        ),
        other => ("Update not finished", other.message()),
    }
}

/// A scripted update for the tour's film: nothing is fetched or installed.
#[cfg(debug_assertions)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Script {
    Succeeds,
    Fails,
}

#[cfg(debug_assertions)]
thread_local! {
    /// Where the film's pointer is drawn, in the window: the tour presses
    /// the notice's buttons without moving the real pointer, so a film of
    /// it would show buttons pressing themselves.
    static FILM_POINTER: std::cell::Cell<Option<(f32, f32)>> = const { std::cell::Cell::new(None) };
}

/// How long the film's check takes to get its answer, in seconds.
#[cfg(debug_assertions)]
pub const SCRIPTED_CHECK: f32 = 1.6;

/// Put the film's pointer at a place in the window, or take it away.
#[cfg(debug_assertions)]
pub fn film_pointer(at: Option<Point<Pixels>>) {
    FILM_POINTER.with(|pointer| pointer.set(at.map(|at| (f32::from(at.x), f32::from(at.y)))));
}

/// The film's pointer: the system's arrow, drawn. It glides to wherever
/// the tour last put it.
#[cfg(debug_assertions)]
pub fn film_pointer_mark(stage: &motion::Stage) -> Option<impl IntoElement + use<>> {
    let (x, y) = FILM_POINTER.with(|pointer| pointer.get())?;
    let x = stage.aim(("film-pointer", 0), x, motion::SETTLE);
    let y = stage.aim(("film-pointer", 1), y, motion::SETTLE);
    Some(
        canvas(
            |_, _, _| (),
            move |_, _, window, _| {
                const ARROW: [(f32, f32); 7] = [
                    (0., 0.),
                    (0., 16.2),
                    (4.3, 12.4),
                    (7.1, 18.6),
                    (9.5, 17.5),
                    (6.7, 11.5),
                    (12., 11.5),
                ];
                let points: Vec<_> = ARROW
                    .iter()
                    .map(|(dx, dy)| point(px(x + dx), px(y + dy)))
                    .collect();
                let mut edge = PathBuilder::stroke(px(2.4));
                edge.add_polygon(&points, true);
                if let Ok(edge) = edge.build() {
                    window.paint_path(edge, gpui::white());
                }
                let mut arrow = PathBuilder::fill();
                arrow.add_polygon(&points, true);
                if let Ok(arrow) = arrow.build() {
                    window.paint_path(arrow, gpui::black());
                }
            },
        )
        .absolute()
        .size_0(),
    )
}

/// The share of a scripted download that has arrived `through` of the way
/// through it (0..=1): quick at first, nearly stalled in the middle, then a
/// burst and a slower end, the way a real one goes.
#[cfg(debug_assertions)]
pub fn scripted_share(through: f32) -> f32 {
    const RATES: [f32; 10] = [0.5, 1.5, 1.7, 0.35, 0.08, 0.9, 1.9, 1.6, 0.9, 0.6];
    let whole: f32 = RATES.iter().sum();
    let at = through.clamp(0., 1.) * RATES.len() as f32;
    let step = (at.floor() as usize).min(RATES.len());
    let done: f32 = RATES[..step].iter().sum();
    (done + RATES.get(step).map_or(0., |rate| rate * (at - step as f32))) / whole
}

#[derive(Default)]
pub struct Updates {
    pub phase: Phase,
    /// How the last check or update ended, for the row in Settings.
    outcome: Option<String>,
    /// Bytes of the archive on disk so far.
    received: Arc<AtomicU64>,
    /// "Later" puts the notice away; the row in Settings keeps the offer.
    put_away: bool,
    /// The notice last drawn, kept so it can finish leaving.
    shown: Notice,
    /// Counts the checks and updates started, so the end of one that has
    /// been replaced is dropped.
    turn: u64,
    /// Whether the last check found this to be the latest version; `None`
    /// until one has finished.
    latest: Option<bool>,
    /// When the last check ended, and whether it got an answer. The wall
    /// clock, so that a night asleep counts.
    last_check: Option<(SystemTime, bool)>,
    /// A quiet check is on its way.
    quiet_running: bool,
    /// The version "Later" was said to: in this run its notice comes back
    /// only when asked for.
    declined: Option<String>,
    /// An update that did not finish, until it is answered.
    failure: Option<Failure>,
    /// The mark and what it is playing.
    pour: Pour,
    /// Whether the mark was on screen in the last frame, in the notice or
    /// in Settings; whether the notice was; and whether the notice has just
    /// come on.
    was_on: bool,
    notice_was_on: bool,
    fresh: std::cell::Cell<bool>,
    /// The capsule's face before the one it shows, and the one it shows:
    /// the notice's, and the one of the row in Settings.
    faces: (Face, Face),
    pub(crate) row_faces: (Face, Face),
    /// Settings → About is the page on screen: the update is told in its
    /// version row, and the notice is not shown.
    about: bool,
    /// When a check by hand last found nothing newer: the row in Settings
    /// says so for [`PLEASED_FOR`].
    pleased: Option<Instant>,
    /// When the download under way began, how fast it is arriving, and
    /// what is said of that and since when (seconds into the download).
    download_began: Option<Instant>,
    rate: Rate,
    pace: (f32, Option<String>),
    /// The version being put in place, while the app restarts into it.
    installing: Option<String>,
    #[cfg(debug_assertions)]
    script: Option<Script>,
    /// What the film's check answers: a version, or that there is none.
    #[cfg(debug_assertions)]
    script_check: Option<bool>,
}

/// How wide a pill's label is: 13 px in the app's face.
pub(crate) fn pill_label_width(label: &str, window: &Window) -> f32 {
    text_width(label, 13., FontWeight::NORMAL, window)
}

/// How wide `text` is on one line at `size` px in the app's face.
pub(crate) fn text_width(text: &str, size: f32, weight: FontWeight, window: &Window) -> f32 {
    let run = TextRun {
        len: text.len(),
        font: Font {
            weight,
            ..font(FONT)
        },
        color: gpui::black(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    f32::from(
        window
            .text_system()
            .shape_line(SharedString::from(text.to_owned()), px(size), &[run], None)
            .width,
    )
}

/// Megabytes with one decimal, the way a download is counted.
fn megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_000_000.)
}

/// How much of `total` has arrived, in whole percent.
fn percent(received: u64, total: u64) -> u64 {
    if total == 0 {
        0
    } else {
        received.min(total) * 100 / total
    }
}

/// The megabytes of a download as the notice counts them: in tenths for a
/// small archive, whole for a larger one, so the last digit has about a
/// hundred steps to take either way. Returns how many places are after the
/// point and the whole size in units of the last digit.
pub(crate) fn counted(total: u64) -> (u32, f64) {
    let megabytes = total as f64 / 1_000_000.;
    if megabytes < 10. {
        (1, (megabytes * 10.).round())
    } else {
        (0, megabytes.round())
    }
}

/// "4.2 MB of 9.8 MB (43%)". The total is the one the server advertised.
pub fn progress_text(received: u64, total: u64) -> String {
    let received = received.min(total);
    let percent = percent(received, total);
    format!("{} of {} ({percent}%)", megabytes(received), megabytes(total))
}

impl Updates {
    /// A state of the update to look at, for the scripted tour: nothing is
    /// checked, fetched or installed.
    /// Release builds compile the tour too; there it has nothing to show.
    #[cfg(not(debug_assertions))]
    pub fn review(&mut self, _state: &str) {}

    #[cfg(debug_assertions)]
    pub fn review(&mut self, state: &str) {
        let offer = Offer {
            version: "9.9.9".into(),
            path: String::new(),
            size: 48_200_000,
            sha256: String::new(),
        };
        // What a quiet check at launch finds, and "Later" said to it.
        match state {
            "found" => {
                *self = Self::default();
                self.quiet_result(
                    Ok(Check::Available(Offer {
                        version: "0.2.0".into(),
                        ..offer
                    })),
                    SystemTime::now(),
                );
                return;
            }
            // The film (`f-update-…`): what a quiet check finds, with the
            // notice's buttons playing a scripted update.
            "film" | "film-fail" => {
                *self = Self::default();
                self.script = Some(if state == "film" {
                    Script::Succeeds
                } else {
                    Script::Fails
                });
                self.quiet_result(
                    Ok(Check::Available(Offer {
                        version: "0.2.0".into(),
                        ..offer
                    })),
                    SystemTime::now(),
                );
                return;
            }
            // The film of Settings → About (`f-about-…`): nothing is known
            // yet, and its button plays a scripted check and update.
            "film-check" | "film-check-fail" | "film-uptodate" => {
                *self = Self::default();
                self.script = Some(if state == "film-check-fail" {
                    Script::Fails
                } else {
                    Script::Succeeds
                });
                self.script_check = Some(state != "film-uptodate");
                return;
            }
            "later" => return self.later(),
            _ => {}
        }
        self.put_away = false;
        self.outcome = None;
        self.failure = None;
        self.pleased = None;
        self.script = None;
        self.script_check = None;
        film_pointer(None);
        self.received.store(20_700_000, Ordering::Release);
        self.phase = match state {
            "checking" => Phase::Checking,
            "available" => Phase::Available(offer),
            "downloading" => Phase::Downloading(offer),
            "verifying" => Phase::Verifying(offer),
            "ready" => Phase::Ready(Staged {
                version: offer.version,
                app: PathBuf::new(),
                bundle: PathBuf::new(),
                team: String::new(),
            }),
            "failed" => {
                self.outcome = Some(Problem::Download.message());
                self.fail(&Problem::Download, Some(offer));
                Phase::Idle
            }
            "uptodate" => {
                self.pleased = Some(Instant::now());
                Phase::Idle
            }
            _ => Phase::Idle,
        };
    }

    fn notice(&self) -> Option<Notice> {
        let version = update::current_version();
        Some(match &self.phase {
            Phase::Idle => {
                let failure = self.failure.as_ref()?;
                Notice {
                    title: failure.title.into(),
                    detail: Some(failure.detail.clone()),
                    action: Some("Try again"),
                    act: Act::Spilled,
                    ..Default::default()
                }
            }
            Phase::Checking => Notice {
                title: "Checking for updates…".into(),
                busy: true,
                act: Act::Seeking,
                ..Default::default()
            },
            Phase::Available(offer) => Notice {
                title: format!("Version {} is available", offer.version),
                detail: Some(format!("You have {version}.")),
                action: Some("Update"),
                act: Act::Found,
                ..Default::default()
            },
            Phase::Downloading(offer) => Notice {
                title: format!("Downloading version {}", offer.version),
                detail: Some(progress_text(
                    self.received.load(Ordering::Acquire),
                    offer.size,
                )),
                busy: true,
                act: Act::Pouring,
                counts: Some(offer.size),
                ..Default::default()
            },
            // The same two lines as the parts before and after it, so
            // nothing in the notice changes height on the way.
            Phase::Verifying(offer) => Notice {
                title: "Verifying the update…".into(),
                detail: Some(format!("Version {}", offer.version)),
                busy: true,
                act: Act::Proving,
                ..Default::default()
            },
            Phase::Ready(staged) => Notice {
                title: "Update ready".into(),
                detail: Some(format!("Version {}", staged.version)),
                action: Some("Restart to update"),
                act: Act::Ready,
                ..Default::default()
            },
            Phase::Restarting => Notice {
                title: "Restarting…".into(),
                detail: self.installing.as_ref().map(|version| format!("Version {version}")),
                busy: true,
                act: Act::Proving,
                ..Default::default()
            },
        })
    }

    /// The share of the download that has arrived, 0..=1.
    fn share(&self) -> f32 {
        match &self.phase {
            Phase::Downloading(offer) if offer.size > 0 => {
                let received = self.received.load(Ordering::Acquire).min(offer.size);
                (received as f64 / offer.size as f64) as f32
            }
            Phase::Verifying(_) | Phase::Ready(_) | Phase::Restarting => 1.,
            _ => 0.,
        }
    }

    /// An update that was under way has failed: the notice says so until
    /// it is answered.
    fn fail(&mut self, problem: &Problem, offer: Option<Offer>) {
        let (title, detail) = failure_lines(problem);
        self.failure = Some(Failure {
            title,
            detail,
            offer,
        });
        self.put_away = false;
    }

    /// The capsule's face in this notice.
    fn face(&self, notice: &Notice) -> Face {
        match (notice.action, &self.phase) {
            (Some(label), _) => Face::Label(label),
            (None, Phase::Downloading(_) | Phase::Verifying(_)) => Face::Percent,
            (None, _) => Face::None,
        }
    }

    /// The face of the capsule in the row of Settings: the notice's own
    /// while the row is the update's card, and otherwise what the row's
    /// button has always said.
    fn row_face(&self) -> Face {
        match &self.phase {
            Phase::Idle if self.failure.is_some() => Face::Label("Try again"),
            Phase::Idle => Face::Plain("Check for updates"),
            Phase::Checking => Face::Plain("Checking…"),
            Phase::Available(_) => Face::Label("Update"),
            Phase::Downloading(_) | Phase::Verifying(_) => Face::Percent,
            Phase::Ready(_) => Face::Label("Restart to update"),
            Phase::Restarting => Face::None,
        }
    }

    /// Whether a check by hand has just found nothing newer.
    fn pleased(&self, now: Instant) -> bool {
        self.phase == Phase::Idle
            && self.failure.is_none()
            && self.pleased.is_some_and(|at| {
                motion::paced(now.saturating_duration_since(at)) < PLEASED_FOR
            })
    }

    /// When the row in Settings began to say that nothing is newer, while
    /// it says so.
    pub(crate) fn pleased_since(&self, now: Instant) -> Option<f32> {
        let at = self.pleased.filter(|_| self.pleased(now))?;
        Some(motion::paced(now.saturating_duration_since(at)))
    }

    /// The part the mark plays now, wherever it is shown.
    fn act(&self, now: Instant) -> Act {
        match self.notice().filter(|_| self.open()) {
            Some(notice) => notice.act,
            None if self.pleased(now) => Act::Pleased,
            None => Act::Still,
        }
    }

    /// A capsule's next face, once it is due: the pop comes first, and the
    /// capsule opens into "Restart to update" a beat after the bowl has
    /// popped.
    fn turned(&self, faces: (Face, Face), face: Face, act: Act, now: Instant, reduced: bool) -> (Face, Face) {
        let cued = act != Act::Ready
            || reduced
            || faces.1 != Face::Percent
            || self.pour.since(now) >= READY_CUE;
        if face != faces.1 && cued {
            (faces.1, face)
        } else {
            faces
        }
    }

    /// One frame of the update and its mark, called once for every frame
    /// the window draws. `about` is whether Settings → About is the page on
    /// screen, where the update is told in the version row and the notice
    /// is not shown. Returns whether the mark is still moving, which is
    /// whether another frame is wanted.
    pub fn frame(&mut self, now: Instant, about: bool) -> bool {
        self.about = about;
        let open = self.open();
        let notice = self.notice().filter(|_| open);
        let reduced = motion::reduce_motion();
        let share = self.share();
        let act = self.act(now);
        // The mark is one mark: the notice's, or the row's in Settings.
        let on = open || about;
        if on {
            if !self.was_on {
                // It has just come on: the mark starts its part from the
                // top, and nothing is left over from last time.
                self.pour.arrive(act, share, now);
            }
            self.pour.frame(act, share, now, reduced);
        } else if matches!(self.phase, Phase::Downloading(_)) {
            // Behind a notice that was put away, the download goes on:
            // the dot in the menu follows it.
            self.pour.frame(Act::Pouring, share, now, reduced);
        }
        // The notice's capsule.
        let notice_on = open && !about;
        if let Some(notice) = &notice {
            let face = self.face(notice);
            if notice_on && !self.notice_was_on {
                self.faces = (face, face);
                self.fresh.set(true);
            }
            self.faces = self.turned(self.faces, face, act, now, reduced);
        }
        // The row's: off screen it is simply what it will be when the row
        // is next looked at.
        let face = self.row_face();
        self.row_faces = if about {
            self.turned(self.row_faces, face, act, now, reduced)
        } else {
            (face, face)
        };
        // How fast the download is arriving, from what is on disk.
        match (&self.phase, self.download_began) {
            (Phase::Downloading(offer), Some(began)) => {
                let at = motion::paced(now.saturating_duration_since(began));
                self.rate.sample(at, self.received.load(Ordering::Acquire).min(offer.size));
                if self.pace.1.is_none() || at - self.pace.0 >= RATE_SAID_EVERY || at < self.pace.0 {
                    let said = pace_text(&self.rate, offer.size);
                    if said.is_some() || self.pace.1.is_some() {
                        self.pace = (at, said);
                    }
                }
            }
            _ => {
                if self.download_began.is_some() {
                    self.download_began = None;
                    self.rate = Rate::default();
                    self.pace = (0., None);
                }
            }
        }
        self.was_on = on;
        self.notice_was_on = notice_on;
        on && self.pour.moving()
    }

    /// What the version row of Settings → About shows.
    pub(crate) fn about_row(&self, now: Instant) -> AboutRow {
        let current = update::current_version();
        let notice = self.notice().filter(|_| self.open());
        // A check is told in the row as it is: its mark turns in its tile.
        let Some(notice) = notice.filter(|_| self.phase != Phase::Checking) else {
            let checked = self.pleased(now);
            return AboutRow {
                title: "Version".into(),
                detail: if checked {
                    "You're up to date.".into()
                } else {
                    self.row().0
                },
                act: self.act(now),
                checked,
                ..Default::default()
            };
        };
        let pair = |version: &str| Some((current.to_owned(), version.to_owned()));
        let mut row = AboutRow {
            open: true,
            title: notice.title.clone(),
            detail: notice.detail.clone().unwrap_or_default(),
            act: notice.act,
            ..Default::default()
        };
        match &self.phase {
            Phase::Idle => {
                row.pair = self
                    .failure
                    .as_ref()
                    .and_then(|failure| failure.offer.as_ref())
                    .and_then(|offer| pair(&offer.version));
            }
            Phase::Checking => {}
            Phase::Available(offer) => {
                // One short line at any width the row can have.
                row.detail = format!("You have {current} · {} download", megabytes(offer.size));
                row.pair = pair(&offer.version);
            }
            Phase::Downloading(offer) => {
                row.counts = Some(offer.size);
                row.pace = self.pace.1.clone();
                row.pair = pair(&offer.version);
                row.line = true;
            }
            Phase::Verifying(offer) => {
                row.detail = "Checking the download before it is installed.".into();
                row.pair = pair(&offer.version);
                row.sheen = true;
                row.line = true;
            }
            Phase::Ready(staged) => {
                row.detail = "Restart Cereal to finish installing it.".into();
                row.pair = pair(&staged.version);
                row.resolved = true;
            }
            Phase::Restarting => {
                row.detail = "Putting the new version in place.".into();
                row.pair = self.installing.as_deref().and_then(pair);
                row.resolved = true;
                row.sheen = true;
            }
        }
        row
    }

    /// What a capsule's label says where it is drawn: the row in Settings
    /// names the version its "Update" would bring.
    fn says(&self, seat: Seat, label: &'static str) -> SharedString {
        match (seat, label, self.known()) {
            (Seat::About, "Update", Some(version)) => format!("Update to {version}").into(),
            _ => label.into(),
        }
    }

    /// Whether the row's capsule can be pressed with this face.
    fn pressable(&self, face: Face) -> bool {
        match face {
            Face::Label(_) => true,
            Face::Plain(_) => self.phase == Phase::Idle,
            Face::None | Face::Percent => false,
        }
    }

    /// The mark's pose, and how long it has been playing its part.
    pub(crate) fn mark(&self, now: Instant) -> (update_bowl::BowlPose, f32) {
        (self.pour.pose(), self.pour.since(now))
    }

    /// How much of the download the mark shows, 0..=1, and how fast that
    /// is growing (per second).
    pub(crate) fn level(&self) -> (f32, f32) {
        self.pour.level()
    }

    /// The update's dot, for the account menu and its button: gold while
    /// an update is known, filling while it downloads.
    pub(crate) fn badge(&self, size: f32) -> AnyElement {
        let level = matches!(self.phase, Phase::Downloading(_)).then(|| self.pour.level().0);
        update_bowl::badge(size, level)
    }

    /// Whether the notice is on screen. While it is, what it says is kept,
    /// so it can still be drawn as it leaves.
    pub fn notice_open(&mut self) -> bool {
        // In Settings → About the version row tells it; not twice.
        let open = self.open() && !self.about;
        if let Some(notice) = self.notice().filter(|_| open) {
            self.shown = notice;
        }
        open
    }

    fn open(&self) -> bool {
        match self.phase {
            Phase::Idle => self.failure.is_some(),
            _ => !(self.put_away && self.waits_for_an_answer()),
        }
    }

    fn waits_for_an_answer(&self) -> bool {
        matches!(self.phase, Phase::Available(_) | Phase::Ready(_))
    }

    /// The version an update would bring, from the moment it is found until
    /// the restart that installs it.
    pub(crate) fn known(&self) -> Option<&str> {
        match &self.phase {
            Phase::Available(offer) | Phase::Downloading(offer) | Phase::Verifying(offer) => {
                Some(&offer.version)
            }
            Phase::Ready(staged) => Some(&staged.version),
            Phase::Idle | Phase::Checking | Phase::Restarting => None,
        }
    }

    /// The row of the account menu while an update is known: what it says,
    /// and the version beside it.
    pub(crate) fn menu_row(&self) -> Option<(&'static str, &str)> {
        let label = match &self.phase {
            Phase::Available(_) => "Update available",
            Phase::Downloading(_) | Phase::Verifying(_) => "Downloading update…",
            Phase::Ready(_) => "Restart to update",
            Phase::Idle | Phase::Checking | Phase::Restarting => return None,
        };
        Some((label, self.known()?))
    }

    /// Whether to ask the server quietly now. Never while the person is in
    /// the middle of a check, a download or a recording, and never twice at
    /// once; an offer already found is asked about again, in case a later
    /// version has taken its place.
    fn wants_quiet_check(
        &self,
        now: SystemTime,
        trigger: Trigger,
        ready: bool,
        recording: bool,
    ) -> bool {
        ready
            && !recording
            && !self.quiet_running
            && matches!(self.phase, Phase::Idle | Phase::Available(_))
            && check_due(
                self.last_check.map(|(at, answered)| {
                    // A clock set back counts as a long time ago.
                    (now.duration_since(at).unwrap_or(Duration::MAX), answered)
                }),
                trigger,
            )
    }

    /// A quiet check has ended. Only a version that was not known yet
    /// changes what is on screen: its notice comes up, unless "Later" was
    /// already said to that version in this run. Being up to date and
    /// failing say nothing. Returns whether anything shown has changed.
    fn quiet_result(&mut self, result: Result<Check, Problem>, now: SystemTime) -> bool {
        self.quiet_running = false;
        self.last_check = Some((now, result.is_ok()));
        // The person started something meanwhile: that has the floor.
        if !matches!(self.phase, Phase::Idle | Phase::Available(_)) {
            return false;
        }
        match result {
            Ok(Check::Available(offer)) => {
                let known = self.known() == Some(offer.version.as_str());
                if !known {
                    self.put_away = self.declined.as_deref() == Some(offer.version.as_str());
                    self.outcome = None;
                    self.failure = None;
                }
                self.phase = Phase::Available(offer);
                !known
            }
            Ok(Check::UpToDate) => {
                // What was offered before has been withdrawn.
                let withdrawn = self.phase != Phase::Idle;
                self.phase = Phase::Idle;
                let changed = withdrawn || self.latest != Some(true);
                self.latest = Some(true);
                changed
            }
            Err(_) => false,
        }
    }

    /// "Later": the notice goes, and stays away for this version. Said to
    /// a failure, it is only put away.
    fn later(&mut self) {
        if let Phase::Available(offer) = &self.phase {
            self.declined = Some(offer.version.clone());
        }
        self.failure = None;
        self.put_away = true;
    }

    /// One line for the menu bar's menu: where this version stands.
    pub(crate) fn status(&self) -> &'static str {
        match (&self.phase, self.latest) {
            (Phase::Checking, _) => "Checking for updates…",
            (Phase::Available(_), _) => "Update available",
            (Phase::Downloading(_) | Phase::Verifying(_), _) => "Downloading update…",
            (Phase::Ready(_) | Phase::Restarting, _) => "Update ready to install",
            (Phase::Idle, Some(true)) => "Latest version",
            (Phase::Idle, Some(false)) => "Could not check for updates",
            (Phase::Idle, None) => "Updates not checked yet",
        }
    }

    /// The sentence under "Version" in Settings, and the button beside it:
    /// its label, and whether it can be pressed now.
    pub(crate) fn row(&self) -> (String, String, bool) {
        let notice = self.notice();
        let sentence = match (&self.phase, &notice) {
            (Phase::Idle, _) | (_, None) => self
                .outcome
                .clone()
                .unwrap_or_else(|| "Native GPUI build.".into()),
            // One short line, whatever the width of the window: the row has
            // the version and its button beside it. The megabytes are in the
            // notice, which is up for as long as the download runs.
            (Phase::Downloading(offer), Some(notice)) => format!(
                "{} ({}%)",
                notice.title,
                percent(self.received.load(Ordering::Acquire), offer.size)
            ),
            (_, Some(notice)) => notice.title.clone(),
        };
        let (label, enabled) = match &self.phase {
            Phase::Idle => ("Check for updates", true),
            Phase::Checking => ("Checking…", false),
            Phase::Available(offer) => {
                return (sentence, format!("Update to {}", offer.version), true);
            }
            Phase::Downloading(_) => ("Downloading…", false),
            Phase::Verifying(_) => ("Verifying…", false),
            Phase::Ready(_) => ("Restart", true),
            Phase::Restarting => ("Restarting…", false),
        };
        (sentence, label.into(), enabled)
    }

}

/// What the worker that downloads and verifies an update reports.
enum Progress {
    Verifying,
    Done(Result<Staged, Problem>),
}

/// Download the offered archive, check it, unpack it and check the app
/// inside. Nothing of the installed app is touched here.
fn fetch_and_verify(
    server: &str,
    offer: &Offer,
    received: &AtomicU64,
    report: &mpsc::Sender<Progress>,
) -> Result<Staged, Problem> {
    let (bundle, team) = update::readiness()?;
    let staged = (|| {
        let archive = update::download(server, offer, received)?;
        let _ = report.send(Progress::Verifying);
        let app = update::unpack(&archive)?;
        update::verify_bundle(&app, offer, &team)?;
        Ok(Staged {
            version: offer.version.clone(),
            app,
            bundle,
            team,
        })
    })();
    if staged.is_err() {
        // Nothing that was refused stays on disk.
        update::discard_download();
    }
    staged
}

impl Cereal {
    /// The menu item and the button in Settings. With an answer already
    /// waiting, it brings that answer back instead of asking again.
    pub fn check_for_updates(&mut self, cx: &mut Context<Self>) {
        self.updates.put_away = false;
        if self.updates.phase != Phase::Idle {
            cx.notify();
            return;
        }
        #[cfg(debug_assertions)]
        if let Some(finds) = self.updates.script_check {
            return self.play_scripted_check(finds, cx);
        }
        let server = crate::accounts::update_server().filter(|_| crate::server::enabled());
        let Some(server) = server else {
            self.end_update(Err(Problem::NotConnected), cx);
            return;
        };
        self.updates.phase = Phase::Checking;
        self.updates.outcome = None;
        self.updates.failure = None;
        self.updates.pleased = None;
        self.updates.turn = self.updates.turn.wrapping_add(1);
        let turn = self.updates.turn;
        cx.notify();
        let (send, receive) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = send.send(update::check(&server));
        });
        cx.spawn(async move |this, cx| {
            let result = loop {
                match receive.try_recv() {
                    Ok(result) => break result,
                    Err(mpsc::TryRecvError::Disconnected) => break Err(Problem::Unreachable),
                    Err(mpsc::TryRecvError::Empty) => {
                        cx.background_executor()
                            .timer(Duration::from_millis(60))
                            .await
                    }
                }
            };
            let _ = this.update(cx, |this, cx| {
                if this.updates.turn != turn {
                    return;
                }
                this.updates.last_check = Some((SystemTime::now(), result.is_ok()));
                match result {
                    Ok(Check::Available(offer)) => {
                        this.updates.phase = Phase::Available(offer);
                        cx.notify();
                    }
                    Ok(Check::UpToDate) => this.end_update(Ok(()), cx),
                    Err(problem) => this.end_update(Err(problem), cx),
                }
            });
        })
        .detach();
    }

    /// The check nobody asked for: at launch once an account is ready, then
    /// every few hours, and when the window comes back after a long gap.
    /// It shows nothing while it runs and nothing when it finds no update
    /// or cannot reach the server.
    pub fn quiet_update_check(&mut self, trigger: Trigger, cx: &mut Context<Self>) {
        // The default account's server, once its session has been verified
        // and the welcome is over.
        let server = crate::accounts::update_server().filter(|_| {
            crate::server::enabled()
                && crate::accounts::any_shown()
                && self.screen != crate::app::Screen::Welcome
        });
        let recording = self.recording.is_active();
        let now = SystemTime::now();
        let Some(server) = server.filter(|_| {
            self.updates
                .wants_quiet_check(now, trigger, true, recording)
        }) else {
            return;
        };
        self.updates.quiet_running = true;
        let (send, receive) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = send.send(update::check(&server));
        });
        cx.spawn(async move |this, cx| {
            let result = loop {
                match receive.try_recv() {
                    Ok(result) => break result,
                    Err(mpsc::TryRecvError::Disconnected) => break Err(Problem::Unreachable),
                    Err(mpsc::TryRecvError::Empty) => {
                        cx.background_executor()
                            .timer(Duration::from_millis(200))
                            .await
                    }
                }
            };
            let _ = this.update(cx, |this, cx| {
                if this.updates.quiet_result(result, SystemTime::now()) {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// The account menu's row: take the update that was found, or bring
    /// back the notice of the one under way.
    pub fn open_update(&mut self, cx: &mut Context<Self>) {
        self.menu = None;
        self.updates.put_away = false;
        if matches!(self.updates.phase, Phase::Available(_)) {
            self.advance_update(cx);
        }
        cx.notify();
    }

    /// A check or an update is over: say how it ended. A check's answer
    /// is a toast, or, when Settings → About is the page on screen, said in
    /// its version row and nowhere else. An update that was under way and
    /// failed is said where it was being followed: in its notice or its
    /// row, which stays.
    fn end_update(&mut self, result: Result<(), Problem>, cx: &mut Context<Self>) {
        let message = match &result {
            Ok(()) => format!("Cereal is up to date ({})", update::current_version()),
            Err(problem) => problem.message(),
        };
        let under_way = match &self.updates.phase {
            Phase::Downloading(offer) | Phase::Verifying(offer) => Some(Some(offer.clone())),
            Phase::Restarting => Some(None),
            Phase::Idle | Phase::Checking | Phase::Available(_) | Phase::Ready(_) => None,
        };
        self.updates.latest = Some(result.is_ok());
        self.updates.phase = Phase::Idle;
        // Being up to date is said for a while and then the row is as it
        // was; a check that failed stays said until the next one.
        self.updates.outcome = result.is_err().then(|| message.clone());
        match (&result, under_way) {
            (Err(problem), Some(offer)) => {
                self.updates.fail(problem, offer);
                cx.notify();
            }
            (result, _) => {
                if result.is_ok() {
                    self.updates.pleased = Some(Instant::now());
                    // The row relaxes when that has been said long enough.
                    cx.spawn(async move |this, cx| {
                        cx.background_executor()
                            .timer(motion::slow(Duration::from_secs_f32(PLEASED_FOR + 0.05)))
                            .await;
                        let _ = this.update(cx, |_, cx| cx.notify());
                    })
                    .detach();
                }
                if self.updates.about {
                    cx.notify();
                } else {
                    self.show_toast(&message, cx);
                }
            }
        }
    }

    /// The notice's action, and the button in Settings: update when a
    /// version is available, restart when one is ready, fetch again what
    /// failed, otherwise check.
    pub fn advance_update(&mut self, cx: &mut Context<Self>) {
        #[cfg(debug_assertions)]
        if let Some(script) = self.updates.script {
            // With nothing known yet, the film's button checks.
            if self.updates.phase == Phase::Idle && self.updates.failure.is_none() {
                return self.check_for_updates(cx);
            }
            return self.play_scripted_update(script, cx);
        }
        let again = self.updates.failure.as_ref().and_then(|failure| failure.offer.clone());
        match (self.updates.phase.clone(), again) {
            (Phase::Available(offer), _) | (Phase::Idle, Some(offer)) => {
                self.download_update(offer, cx)
            }
            (Phase::Ready(staged), _) => self.install_update(staged, cx),
            _ => self.check_for_updates(cx),
        }
    }

    /// The film's check (`f-about-…`): [`SCRIPTED_CHECK`] seconds of asking
    /// nobody, then a version or the news that there is none.
    #[cfg(debug_assertions)]
    fn play_scripted_check(&mut self, finds: bool, cx: &mut Context<Self>) {
        self.updates.phase = Phase::Checking;
        self.updates.outcome = None;
        self.updates.failure = None;
        self.updates.pleased = None;
        self.updates.turn = self.updates.turn.wrapping_add(1);
        let turn = self.updates.turn;
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(motion::slow(Duration::from_secs_f32(SCRIPTED_CHECK)))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.updates.turn != turn {
                    return;
                }
                this.updates.last_check = Some((SystemTime::now(), true));
                if finds {
                    this.updates.phase = Phase::Available(Offer {
                        version: "0.2.0".into(),
                        path: String::new(),
                        size: 48_200_000,
                        sha256: String::new(),
                    });
                    cx.notify();
                } else {
                    this.end_update(Ok(()), cx);
                }
            });
        })
        .detach();
    }

    /// The film's update (`f-update-…`): the notice's buttons do what they
    /// do, with a download that is only counted. Four seconds of uneven
    /// arrival, a second and a half of verifying, then ready; or a failure
    /// part of the way. "Restart" restarts nothing.
    #[cfg(debug_assertions)]
    fn play_scripted_update(&mut self, script: Script, cx: &mut Context<Self>) {
        const DOWNLOAD: f32 = 4.;
        const FAILS_AT: f32 = 2.3;
        let offer = match (&self.updates.phase, &self.updates.failure) {
            (Phase::Available(offer), _) => offer.clone(),
            (Phase::Idle, Some(Failure { offer: Some(offer), .. })) => offer.clone(),
            (Phase::Ready(staged), _) => {
                self.updates.installing = Some(staged.version.clone());
                self.updates.phase = Phase::Restarting;
                cx.notify();
                return;
            }
            _ => return,
        };
        // The second try works.
        let script = if self.updates.failure.is_some() {
            Script::Succeeds
        } else {
            script
        };
        self.updates.failure = None;
        self.updates.put_away = false;
        self.updates.received = Arc::new(AtomicU64::new(0));
        self.updates.download_began = Some(Instant::now());
        self.updates.rate = Rate::default();
        self.updates.pace = (0., None);
        self.updates.phase = Phase::Downloading(offer.clone());
        self.updates.turn = self.updates.turn.wrapping_add(1);
        let turn = self.updates.turn;
        cx.notify();
        let received = self.updates.received.clone();
        let began = Instant::now();
        cx.spawn(async move |this, cx| {
            let mut ticks = 0u32;
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(8))
                    .await;
                let seconds = motion::paced(began.elapsed());
                let share = scripted_share(seconds / DOWNLOAD);
                received.store((f64::from(share) * offer.size as f64) as u64, Ordering::Release);
                let failed = script == Script::Fails && seconds >= FAILS_AT;
                ticks += 1;
                let current = this.update(cx, |this, cx| {
                    if this.updates.turn != turn {
                        return false;
                    }
                    if failed {
                        this.end_update(Err(Problem::Download), cx);
                    } else if ticks % 12 == 0 {
                        // As the real download does, ten times a second:
                        // all that redraws it when motion is reduced.
                        cx.notify();
                    }
                    true
                });
                if failed || !current.unwrap_or(false) {
                    return;
                }
                if seconds >= DOWNLOAD {
                    break;
                }
            }
            let set = |phase: Phase, cx: &mut AsyncApp| {
                this.update(cx, |this, cx| {
                    if this.updates.turn != turn {
                        return false;
                    }
                    this.updates.phase = phase;
                    cx.notify();
                    true
                })
                .unwrap_or(false)
            };
            if !set(Phase::Verifying(offer.clone()), cx) {
                return;
            }
            cx.background_executor()
                .timer(motion::slow(Duration::from_millis(1500)))
                .await;
            set(
                Phase::Ready(Staged {
                    version: offer.version.clone(),
                    app: PathBuf::new(),
                    bundle: PathBuf::new(),
                    team: String::new(),
                }),
                cx,
            );
        })
        .detach();
    }

    fn download_update(&mut self, offer: Offer, cx: &mut Context<Self>) {
        let Some(server) = crate::accounts::update_server().filter(|_| crate::server::enabled())
        else {
            self.end_update(Err(Problem::NotConnected), cx);
            return;
        };
        self.updates.put_away = false;
        self.updates.failure = None;
        self.updates.received = Arc::new(AtomicU64::new(0));
        self.updates.download_began = Some(Instant::now());
        self.updates.rate = Rate::default();
        self.updates.pace = (0., None);
        self.updates.phase = Phase::Downloading(offer.clone());
        self.updates.turn = self.updates.turn.wrapping_add(1);
        let turn = self.updates.turn;
        cx.notify();
        let received = self.updates.received.clone();
        let (send, receive) = mpsc::channel();
        std::thread::spawn(move || {
            let result = fetch_and_verify(&server, &offer, &received, &send);
            let _ = send.send(Progress::Done(result));
        });
        cx.spawn(async move |this, cx| {
            loop {
                let step = match receive.try_recv() {
                    Ok(step) => Some(step),
                    Err(mpsc::TryRecvError::Disconnected) => {
                        Some(Progress::Done(Err(Problem::Download)))
                    }
                    Err(mpsc::TryRecvError::Empty) => None,
                };
                let finished = matches!(step, Some(Progress::Done(_)));
                let current = this.update(cx, |this, cx| {
                    if this.updates.turn != turn {
                        return false;
                    }
                    match step {
                        Some(Progress::Verifying) => {
                            if let Phase::Downloading(offer) = this.updates.phase.clone() {
                                this.updates.phase = Phase::Verifying(offer);
                            }
                        }
                        Some(Progress::Done(Ok(staged))) => {
                            this.updates.phase = Phase::Ready(staged);
                        }
                        Some(Progress::Done(Err(problem))) => {
                            this.end_update(Err(problem), cx);
                        }
                        // The progress figure follows the bytes on disk.
                        None => {}
                    }
                    cx.notify();
                    true
                });
                if finished || !current.unwrap_or(false) {
                    break;
                }
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
            }
        })
        .detach();
    }

    /// "Restart": put the verified app in the running one's place, then
    /// quit and let the helper start the new one. A swap that fails leaves
    /// the installed app as it was, and the app carries on.
    fn install_update(&mut self, staged: Staged, cx: &mut Context<Self>) {
        if self.recording.is_active() {
            self.show_toast("Stop the recording before restarting", cx);
            return;
        }
        self.updates.put_away = false;
        self.updates.installing = Some(staged.version.clone());
        self.updates.phase = Phase::Restarting;
        self.updates.turn = self.updates.turn.wrapping_add(1);
        let turn = self.updates.turn;
        cx.notify();
        let (send, receive) = mpsc::channel();
        let bundle = staged.bundle.clone();
        std::thread::spawn(move || {
            let _ = send.send(update::install(&staged.app, &staged.bundle, &staged.team));
        });
        cx.spawn(async move |this, cx| {
            let result = loop {
                match receive.try_recv() {
                    Ok(result) => break result,
                    Err(mpsc::TryRecvError::Disconnected) => break Err(Problem::Install),
                    Err(mpsc::TryRecvError::Empty) => {
                        cx.background_executor()
                            .timer(Duration::from_millis(40))
                            .await
                    }
                }
            };
            let _ = this.update(cx, |this, cx| {
                if this.updates.turn != turn {
                    return;
                }
                match result {
                    Ok(()) => {
                        // The new app is in place. If the helper cannot be
                        // started the person opens it themselves.
                        let _ = update::relaunch_after_exit(&bundle);
                        this.quit_after_audio_stop(cx);
                    }
                    Err(problem) => this.end_update(Err(problem), cx),
                }
            });
        })
        .detach();
    }

    /// "Later", in the notice and in the row of Settings.
    pub(crate) fn put_update_away(&mut self, cx: &mut Context<Self>) {
        self.updates.later();
        cx.notify();
    }

    /// The update's capsule: "Update", then the share that has arrived,
    /// then "Restart to update"; in Settings it is also the row's own
    /// button, "Check for updates". One shape throughout: its width springs
    /// to what it holds, its fill comes and goes with whether it is the
    /// thing to do next, and what it says rolls out and in. `None` when
    /// there is nothing for it to say.
    pub(crate) fn render_update_capsule(
        &self,
        seat: Seat,
        fresh: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let [width_key, fill_key, face_key] = seat.keys();
        let (action, figure, placed) = seat.names();
        let t = self.theme();
        let (was, now) = match seat {
            Seat::Notice => self.updates.faces,
            Seat::About => self.updates.row_faces,
        };
        if fresh {
            for key in [width_key, fill_key] {
                self.stage.forget(key);
            }
        }
        let said = |face: Face| face.label().map(|label| self.updates.says(seat, label));
        let (was_said, now_said) = (said(was), said(now));
        // A pill's own width for what it says: its label, 12 px either
        // side and its outline.
        let label_width = |label: &Option<SharedString>| {
            label.as_ref().map_or(0., |label| pill_label_width(label, window))
        };
        let (was_width, now_width) = (label_width(&was_said), label_width(&now_said));
        let rest = match now {
            Face::None => 0.,
            Face::Label(_) | Face::Plain(_) => (now_width + 26.).round(),
            Face::Percent => (pill_label_width("100%", window) + 26.).round(),
        };
        let width = self.stage.aim(width_key, rest, GOOEY).max(0.);
        // Closing, it keeps the look it had; opening from nothing, it has
        // the look it is to have from the start.
        let closing = now == Face::None;
        let filled = if closing { was } else { now }.filled();
        if was == Face::None && !closing {
            self.stage.forget(fill_key);
        }
        let pressable = self.updates.pressable(now);
        let fill = self
            .stage
            .aim(fill_key, if filled { 1. } else { 0. }, SNAPPY)
            .clamp(0., 1.);
        let swap = self.stage.swap(face_key, {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            now.hash(&mut hasher);
            hasher.finish()
        });
        if now == Face::None && width < 1. {
            return None;
        }
        let (level, speed) = self.updates.pour.level();
        let percent = Counter {
            value: f64::from(level) * 100.,
            speed: speed * 100.,
            decimals: 0,
            most: 100.,
        };
        // What it says is painted, not laid out: it rolls by fractions of
        // a pixel, inside a shape that is changing width.
        let rolling = !swap.at_rest();
        let roll = if rolling { swap.value } else { 1. };
        // A label comes in as the capsule comes to fit it, and goes as
        // the capsule closes on it.
        let fits = |face: Face, label: f32| match face {
            Face::Label(_) | Face::Plain(_) => {
                let fit = (width - 26.) / label.max(1.);
                motion::fade_curve(((fit - 0.55) / 0.45).clamp(0., 1.))
            }
            Face::None | Face::Percent => 1.,
        };
        let arrived = if rolling { swap.fade } else { 1. } * fits(now, now_width);
        let (left_behind, risen) = if closing {
            (fits(was, was_width), 0.)
        } else {
            (1. - arrived, roll)
        };
        // A label is in the fill's ink on a fill, in ink when it can be
        // pressed, and quieter when it only says what is going on.
        let (ink, on_fill, quiet) = (t.text, t.bg, t.muted);
        let says = canvas(
            |_, _, _| (),
            move |bounds, _, window, cx| {
                const SIZE: f32 = 13.;
                const TRAVEL: f32 = 10.;
                let line = f32::from(bounds.size.height);
                let (middle, top) = (f32::from(bounds.center().x), f32::from(bounds.origin.y));
                let mut draw = |face: Face,
                                said: &Option<SharedString>,
                                label_width: f32,
                                shift: f32,
                                level: f32,
                                window: &mut Window| {
                    if level <= 0.01 {
                        return;
                    }
                    match (face, said) {
                        (Face::Label(_) | Face::Plain(_), Some(label)) => {
                            let color = match face {
                                Face::Label(_) => on_fill,
                                _ if pressable || face != now => ink,
                                _ => quiet,
                            };
                            update_bowl::paint_counter(
                                None,
                                label,
                                point(px(middle - label_width / 2.), px(top + shift)),
                                (SIZE, line, 0.),
                                color.opacity(level),
                                window,
                                cx,
                            );
                        }
                        (Face::Percent, _) => {
                            let width = update_bowl::counter_width(Some(&percent), "%", SIZE, window);
                            update_bowl::paint_counter(
                                Some(&percent),
                                "%",
                                point(px(middle - width / 2.), px(top + shift)),
                                (SIZE, line, 14.),
                                ink.opacity(level),
                                window,
                                cx,
                            );
                        }
                        _ => {}
                    }
                };
                window.with_content_mask(Some(ContentMask { bounds }), |window| {
                    if rolling || closing {
                        draw(was, &was_said, was_width, -TRAVEL * risen, left_behind, window);
                    }
                    draw(now, &now_said, now_width, TRAVEL * (1. - roll), arrived, window);
                });
            },
        )
        .absolute()
        .inset_0();
        let capsule = if pressable {
            pill_shell(action, t)
                .on_click(cx.listener(|this, _, _, cx| this.advance_update(cx)))
                // Where it is drawn, for scripted runs that press it.
                .child(crate::onboarding_view::place(placed))
        } else {
            pill_frame(figure, t)
        };
        Some(
            capsule
                .relative()
                .flex_shrink_0()
                .px_0()
                .w(px(width))
                .bg(t.text.opacity(fill))
                .when(now == Face::None, |capsule| {
                    capsule.opacity((width / 28.).clamp(0., 1.))
                })
                .child(says)
                .into_any_element(),
        )
    }

    /// "Later" beside a capsule: offered beside an action, it makes room
    /// when there is none, folding away to the capsule's side. `None` once
    /// it has gone.
    pub(crate) fn render_update_later(
        &self,
        key: motion::TrackKey,
        id: &'static str,
        offered: bool,
        fresh: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let t = self.theme();
        if fresh {
            self.stage.forget(key);
        }
        let later = self
            .stage
            .aim(key, if offered { 1. } else { 0. }, GOOEY)
            .clamp(0., 1.);
        if later <= 0.001 {
            return None;
        }
        let later_width = (pill_label_width("Later", window) + 26.).round() + 8.;
        let button = pill(id, "Later", "", t)
            .flex_shrink_0()
            .on_click(cx.listener(|this, _, _, cx| this.put_update_away(cx)));
        if later >= 1. {
            return Some(button.mr(px(8.)).into_any_element());
        }
        // Folding: clipped from its far side, fading.
        Some(
            div()
                .flex_shrink_0()
                .w(px(later_width * later))
                .overflow_hidden()
                .opacity(motion::fade_for(later))
                .flex()
                .justify_end()
                .child(button.mr(px(8.)))
                .into_any_element(),
        )
    }

    /// The film's pointer, over everything in the window (debug builds, and
    /// only while a film has put it somewhere).
    pub fn render_film_pointer(&self) -> Option<AnyElement> {
        #[cfg(debug_assertions)]
        return film_pointer_mark(&self.stage).map(IntoElement::into_any_element);
        #[cfg(not(debug_assertions))]
        None
    }

    /// The notice, in the toast's corner and face. `open` is its presence;
    /// while it leaves, the notice last shown is drawn.
    pub fn render_update_notice(
        &self,
        pose: crate::motion::Pose,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        /// The mark's size: the two lines of text beside it are 34 px.
        const MARK: f32 = 32.;
        let t = self.theme();
        let notice = self.updates.shown.clone();
        let reduced = motion::reduce_motion();
        let fresh = self.updates.fresh.replace(false);
        let hash = |text: &str| {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            text.hash(&mut hasher);
            hasher.finish()
        };
        // "Later" is offered beside an action.
        let later = self.render_update_later(
            ("update-later", 0),
            "update-later",
            self.updates.faces.1.filled(),
            fresh,
            window,
            cx,
        );
        let capsule = self.render_update_capsule(Seat::Notice, fresh, window, cx);
        let title = div()
            .text_size(px(13.))
            .line_height(px(18.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(t.text)
            .whitespace_nowrap()
            .map(|title| {
                if notice.act == Act::Proving && !reduced {
                    title.child(motion::sheen_label(
                        "update-title-sheen",
                        notice.title.clone(),
                        13.,
                        t.muted,
                        t.text,
                        cx,
                    ))
                } else {
                    title.child(notice.title.clone())
                }
            });
        let (level, speed) = self.updates.pour.level();
        let detail = match (notice.counts.filter(|_| !reduced), notice.detail.clone()) {
            // The megabytes, counted: the digits roll.
            (Some(total), _) => {
                let (decimals, most) = counted(total);
                let counter = Counter {
                    value: f64::from(level) * most,
                    speed: speed * most as f32,
                    decimals,
                    most,
                };
                let tail = format!(
                    " of {:.*} MB",
                    decimals as usize,
                    total as f64 / 1_000_000.
                );
                let ink = t.muted;
                Some((
                    hash("counted"),
                    div()
                        .h(px(16.))
                        .w_full()
                        .child(
                            canvas(
                                |_, _, _| (),
                                move |bounds, _, window, cx| {
                                    update_bowl::paint_counter(
                                        Some(&counter),
                                        &tail,
                                        bounds.origin,
                                        (12., 16., 13.),
                                        ink,
                                        window,
                                        cx,
                                    );
                                },
                            )
                            .size_full(),
                        )
                        .into_any_element(),
                ))
            }
            (None, Some(detail)) => Some((
                // A line that counts is one line however it reads: it
                // does not come in again with every figure.
                if notice.counts.is_some() {
                    hash("counted")
                } else {
                    hash(&detail)
                },
                div()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(t.muted)
                    .child(detail)
                    .into_any_element(),
            )),
            (None, None) => None,
        };
        crate::motion::pop(
            div()
                .id("update-notice")
                // Nothing behind the notice is under the pointer.
                .occlude()
                .absolute()
                .bottom(px(16.))
                .right(px(16.))
                .w(px(384.))
                .max_w(window.viewport_size().width - px(32.))
                .min_h(px(NOTICE_HEIGHT))
                .pl(px(16.))
                .pr(px(12.))
                .py(px(10.))
                .rounded(px(12.))
                .bg(t.card)
                .border_1()
                .border_color(t.border)
                .shadow_lg()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .flex()
                .items_center()
                .gap(px(12.))
                .child(update_bowl::bowl(MARK, self.updates.pour.pose(), t))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(motion::swapped(
                            title,
                            self.stage.swap(("update-title", 0), hash(&notice.title)),
                        ))
                        .when_some(detail, |d, (kind, detail)| {
                            d.child(motion::swapped(
                                div().w_full().child(detail),
                                self.stage.swap(("update-detail", 0), kind),
                            ))
                        }),
                )
                .child(
                    div()
                        .flex()
                        .flex_shrink_0()
                        .items_center()
                        .children(later)
                        .children(capsule),
                ),
            crate::motion::Pop::chip((1., 1.)),
            pose,
        )
    }
}

/// Start the quiet checks: one soon after launch (or as soon after as an
/// account is ready), the clock from then on, and the window coming back.
pub fn install(handle: WindowHandle<Cereal>, cx: &mut App) {
    let _ = handle.update(cx, |_, window, cx| {
        cx.observe_window_activation(window, |this, window, cx| {
            if window.is_window_active() {
                this.quiet_update_check(Trigger::Focus, cx);
            }
        })
        .detach();
    });
    cx.spawn(async move |cx| {
        // The first frames and the session's own requests come first.
        let mut wait = Duration::from_secs(3);
        loop {
            cx.background_executor().timer(wait).await;
            wait = QUIET_TICK;
            if handle
                .update(cx, |view, _, cx| view.quiet_update_check(Trigger::Tick, cx))
                .is_err()
            {
                break;
            }
        }
    })
    .detach();
}

/// The notice's height with one or two lines of text, which is the toast's:
/// a toast that appears while the notice is up sits this far above it.
pub const NOTICE_HEIGHT: f32 = 56.;

#[cfg(test)]
mod tests {
    // Named one by one: a glob would bring in gpui's own `test` attribute.
    use super::{
        FOCUS_GAP, Face, PLEASED_FOR, Phase, QUIET_INTERVAL, QUIET_RETRY, RATE_NEEDS, RATE_WINDOW,
        READY_CUE, Rate, Seat, Staged, Trigger, Updates, check_due, counted, failure_lines,
        left_text, pace_text, progress_text, speed_text,
    };
    use crate::{
        motion,
        update::{Check, Offer, Problem},
        update_bowl::{self, Act},
    };
    use std::{
        path::PathBuf,
        sync::atomic::Ordering,
        time::{Duration, Instant, SystemTime},
    };

    fn version(version: &str) -> Offer {
        Offer {
            version: version.into(),
            ..offer()
        }
    }

    #[test]
    fn a_quiet_check_is_due_at_launch_then_by_the_clock_and_on_coming_back() {
        let second = Duration::from_secs(1);
        // Never checked: the check at launch, whatever brings it up.
        assert!(check_due(None, Trigger::Tick));
        assert!(check_due(None, Trigger::Focus));
        // Answered: the clock waits the whole interval, the window coming
        // back only the shorter gap.
        assert!(!check_due(Some((QUIET_INTERVAL - second, true)), Trigger::Tick));
        assert!(check_due(Some((QUIET_INTERVAL, true)), Trigger::Tick));
        assert!(!check_due(Some((FOCUS_GAP - second, true)), Trigger::Focus));
        assert!(check_due(Some((FOCUS_GAP, true)), Trigger::Focus));
        assert!(FOCUS_GAP < QUIET_INTERVAL);
        // Unanswered: tried again sooner, but not at once.
        for trigger in [Trigger::Tick, Trigger::Focus] {
            assert!(!check_due(Some((QUIET_RETRY - second, false)), trigger));
            assert!(check_due(Some((QUIET_RETRY, false)), trigger));
        }
    }

    #[test]
    fn a_quiet_check_waits_for_an_account_and_never_interrupts() {
        let now = SystemTime::now();
        let mut updates = Updates::default();
        let wants = |updates: &Updates, ready, recording| {
            updates.wants_quiet_check(now, Trigger::Tick, ready, recording)
        };
        assert!(wants(&updates, true, false));
        // No account yet, or a recording going on: later.
        assert!(!wants(&updates, false, false));
        assert!(!wants(&updates, true, true));
        // One at a time.
        updates.quiet_running = true;
        assert!(!wants(&updates, true, false));
        updates.quiet_running = false;
        // Not over a check, a download or a restart the person started.
        for phase in [
            Phase::Checking,
            Phase::Downloading(offer()),
            Phase::Verifying(offer()),
            Phase::Restarting,
        ] {
            updates.phase = phase;
            assert!(!wants(&updates, true, false));
        }
        // An offer that is waiting is asked about again when due.
        updates.phase = Phase::Available(offer());
        assert!(wants(&updates, true, false));
        // A check that has just ended, by hand or not, is not repeated.
        updates.last_check = Some((now, true));
        assert!(!wants(&updates, true, false));
        assert!(!updates.wants_quiet_check(now + FOCUS_GAP, Trigger::Tick, true, false));
        assert!(updates.wants_quiet_check(now + FOCUS_GAP, Trigger::Focus, true, false));
        assert!(updates.wants_quiet_check(now + QUIET_INTERVAL, Trigger::Tick, true, false));
        // A clock that was set back does not stop the checks.
        assert!(updates.wants_quiet_check(now - QUIET_INTERVAL, Trigger::Tick, true, false));
    }

    #[test]
    fn a_quiet_check_shows_only_a_version_it_finds() {
        let now = SystemTime::now();
        let mut updates = Updates {
            quiet_running: true,
            ..Default::default()
        };
        // Unreachable: nothing on screen, nothing in Settings.
        assert!(!updates.quiet_result(Err(Problem::Unreachable), now));
        assert!(!updates.quiet_running);
        assert_eq!(updates.last_check, Some((now, false)));
        assert!(!updates.open());
        assert_eq!(updates.notice(), None);
        assert_eq!(updates.row(), ("Native GPUI build.".into(), "Check for updates".into(), true));
        assert_eq!(updates.status(), "Updates not checked yet");
        // Up to date: only the menu bar's caption knows.
        updates.quiet_result(Ok(Check::UpToDate), now);
        assert!(!updates.open());
        assert_eq!(updates.row(), ("Native GPUI build.".into(), "Check for updates".into(), true));
        assert_eq!(updates.status(), "Latest version");
        assert_eq!((updates.known(), updates.menu_row()), (None, None));
        // A version: the notice, the menu's row and the button in Settings.
        assert!(updates.quiet_result(Ok(Check::Available(offer())), now));
        assert!(updates.open());
        assert_eq!(updates.notice().unwrap().title, "Version 0.2.0 is available");
        assert_eq!(updates.known(), Some("0.2.0"));
        assert_eq!(updates.menu_row(), Some(("Update available", "0.2.0")));
        assert_eq!(updates.row().1, "Update to 0.2.0");
        assert!(updates.row_face().filled());
    }

    #[test]
    fn later_is_not_asked_again_for_the_same_version_in_a_run() {
        let now = SystemTime::now();
        let mut updates = Updates::default();
        updates.quiet_result(Ok(Check::Available(offer())), now);
        assert!(updates.open());
        updates.later();
        assert!(!updates.open());
        // The offer stays where it is looked for.
        assert_eq!(updates.menu_row(), Some(("Update available", "0.2.0")));
        assert_eq!(updates.row().1, "Update to 0.2.0");
        // The next quiet checks find the same version: no notice.
        for _ in 0..2 {
            assert!(!updates.quiet_result(Ok(Check::Available(offer())), now));
            assert!(!updates.open());
        }
        // Not even after the offer was lost in between (a failed download
        // ends in `Idle`).
        updates.phase = Phase::Idle;
        updates.put_away = false;
        assert!(updates.quiet_result(Ok(Check::Available(offer())), now));
        assert!(!updates.open());
        assert_eq!(updates.known(), Some("0.2.0"));
        // A later version is news.
        assert!(updates.quiet_result(Ok(Check::Available(version("0.3.0"))), now));
        assert!(updates.open());
        assert_eq!(updates.menu_row(), Some(("Update available", "0.3.0")));
    }

    #[test]
    fn a_quiet_answer_leaves_alone_what_the_person_started() {
        let now = SystemTime::now();
        // Still waiting for an answer to the same version: nothing moves.
        let mut updates = Updates::default();
        updates.quiet_result(Ok(Check::Available(offer())), now);
        assert!(!updates.quiet_result(Ok(Check::Available(offer())), now));
        assert!(updates.open());
        // A failure does not take a known offer away.
        assert!(!updates.quiet_result(Err(Problem::Unreachable), now));
        assert_eq!(updates.known(), Some("0.2.0"));
        // The server no longer offers it: it goes.
        assert!(updates.quiet_result(Ok(Check::UpToDate), now));
        assert_eq!((updates.known(), updates.open()), (None, false));
        // A check by hand or a download started while the quiet one was on
        // its way keeps the floor.
        for phase in [Phase::Checking, Phase::Downloading(offer())] {
            let mut updates = Updates {
                phase: phase.clone(),
                quiet_running: true,
                ..Default::default()
            };
            assert!(!updates.quiet_result(Ok(Check::Available(version("0.3.0"))), now));
            assert_eq!(updates.phase, phase);
            assert!(!updates.quiet_running);
        }
    }

    #[test]
    fn the_menu_row_follows_the_update() {
        let mut updates = Updates::default();
        for phase in [Phase::Idle, Phase::Checking, Phase::Restarting] {
            updates.phase = phase;
            assert_eq!((updates.menu_row(), updates.known()), (None, None));
        }
        updates.phase = Phase::Downloading(offer());
        assert_eq!(updates.menu_row(), Some(("Downloading update…", "0.2.0")));
        assert!(!updates.row_face().filled());
        updates.phase = Phase::Verifying(offer());
        assert_eq!(updates.menu_row(), Some(("Downloading update…", "0.2.0")));
        updates.phase = Phase::Ready(Staged {
            version: "0.2.0".into(),
            app: PathBuf::new(),
            bundle: PathBuf::new(),
            team: String::new(),
        });
        assert_eq!(updates.menu_row(), Some(("Restart to update", "0.2.0")));
        assert!(updates.row_face().filled());
    }

    fn offer() -> Offer {
        Offer {
            version: "0.2.0".into(),
            path: "/api/v1/downloads/files/Cereal-0.2.0-mac-arm64.zip".into(),
            size: 9_800_000,
            sha256: "0".repeat(64),
        }
    }

    #[test]
    fn progress_is_counted_against_the_advertised_size() {
        assert_eq!(progress_text(0, 9_800_000), "0.0 MB of 9.8 MB (0%)");
        assert_eq!(progress_text(4_214_000, 9_800_000), "4.2 MB of 9.8 MB (43%)");
        assert_eq!(progress_text(9_800_000, 9_800_000), "9.8 MB of 9.8 MB (100%)");
        // Never more than the whole, and no division by nothing.
        assert_eq!(progress_text(12_000_000, 9_800_000), "9.8 MB of 9.8 MB (100%)");
        assert_eq!(progress_text(5, 0), "0.0 MB of 0.0 MB (0%)");
    }

    #[test]
    fn every_phase_has_its_notice_and_its_button() {
        let mut updates = Updates::default();
        assert_eq!(updates.notice(), None);
        assert!(!updates.open());
        assert_eq!(updates.row(), ("Native GPUI build.".into(), "Check for updates".into(), true));
        updates.outcome = Some("Cereal is up to date (0.1.0)".into());
        assert_eq!(updates.row().0, "Cereal is up to date (0.1.0)");

        updates.phase = Phase::Checking;
        let notice = updates.notice().unwrap();
        assert_eq!(notice.title, "Checking for updates…");
        assert!(notice.busy && notice.action.is_none());
        assert_eq!(updates.row(), ("Checking for updates…".into(), "Checking…".into(), false));

        updates.phase = Phase::Available(offer());
        let notice = updates.notice().unwrap();
        assert_eq!(notice.title, "Version 0.2.0 is available");
        assert_eq!(notice.action, Some("Update"));
        assert!(!notice.busy);
        assert_eq!(updates.row().1, "Update to 0.2.0");

        updates.phase = Phase::Downloading(offer());
        updates.received.store(4_214_000, Ordering::Release);
        let notice = updates.notice().unwrap();
        assert_eq!(notice.title, "Downloading version 0.2.0");
        assert_eq!(notice.detail.as_deref(), Some("4.2 MB of 9.8 MB (43%)"));
        assert_eq!(
            updates.row(),
            (
                "Downloading version 0.2.0 (43%)".into(),
                "Downloading…".into(),
                false
            )
        );

        updates.phase = Phase::Verifying(offer());
        let notice = updates.notice().unwrap();
        assert_eq!(notice.title, "Verifying the update…");
        // Two lines, as before and after it: nothing moves up or down.
        assert_eq!(notice.detail.as_deref(), Some("Version 0.2.0"));
        assert_eq!(updates.row().1, "Verifying…");

        updates.phase = Phase::Ready(Staged {
            version: "0.2.0".into(),
            app: PathBuf::from("/tmp/update/unpacked/Cereal.app"),
            bundle: PathBuf::from("/Applications/Cereal.app"),
            team: "LZWN2FF7YB".into(),
        });
        let notice = updates.notice().unwrap();
        assert_eq!(notice.title, "Update ready");
        assert_eq!(notice.detail.as_deref(), Some("Version 0.2.0"));
        assert_eq!(notice.action, Some("Restart to update"));
        assert_eq!(updates.row().1, "Restart");
    }

    fn staged() -> Staged {
        Staged {
            version: "0.2.0".into(),
            app: PathBuf::new(),
            bundle: PathBuf::new(),
            team: String::new(),
        }
    }

    #[test]
    fn the_mark_plays_the_part_the_update_is_at() {
        let mut updates = Updates::default();
        let mut part = |phase: Phase| {
            updates.phase = phase;
            let notice = updates.notice().unwrap();
            (notice.act, updates.face(&notice), notice.counts)
        };
        assert_eq!(part(Phase::Checking), (Act::Seeking, Face::None, None));
        assert_eq!(part(Phase::Available(offer())), (Act::Found, Face::Label("Update"), None));
        // The download is counted against its size, by the bowl and in
        // the capsule.
        assert_eq!(
            part(Phase::Downloading(offer())),
            (Act::Pouring, Face::Percent, Some(9_800_000))
        );
        assert_eq!(part(Phase::Verifying(offer())), (Act::Proving, Face::Percent, None));
        assert_eq!(
            part(Phase::Ready(staged())),
            (Act::Ready, Face::Label("Restart to update"), None)
        );
        assert_eq!(part(Phase::Restarting), (Act::Proving, Face::None, None));
    }

    #[test]
    fn the_share_that_has_arrived_is_what_the_bowl_is_filled_to() {
        let mut updates = Updates::default();
        assert_eq!(updates.share(), 0.);
        updates.phase = Phase::Downloading(offer());
        updates.received.store(2_450_000, Ordering::Release);
        assert_eq!(updates.share(), 0.25);
        // Never more than all of it, and nothing of an archive of no size.
        updates.received.store(20_000_000, Ordering::Release);
        assert_eq!(updates.share(), 1.);
        updates.phase = Phase::Downloading(Offer { size: 0, ..offer() });
        assert_eq!(updates.share(), 0.);
        for phase in [Phase::Verifying(offer()), Phase::Ready(staged()), Phase::Restarting] {
            updates.phase = phase;
            assert_eq!(updates.share(), 1.);
        }
        // Megabytes are counted in tenths up to ten, whole after.
        assert_eq!(counted(9_800_000), (1, 98.));
        assert_eq!(counted(48_200_000), (0, 48.));
        assert_eq!(counted(0), (1, 0.));
    }

    #[test]
    fn the_notice_follows_an_update_frame_by_frame() {
        let start = Instant::now();
        let at = |seconds: f32| start + motion::slow(Duration::from_secs_f32(seconds));
        let mut updates = Updates::default();
        // Nothing to show: nothing moves, and no frames are asked for.
        assert!(!updates.frame(at(0.), false));
        // Found: the notice comes on with its mark arriving, then rests.
        updates.quiet_result(Ok(Check::Available(offer())), SystemTime::now());
        assert!(updates.frame(at(0.1), false));
        assert!(updates.fresh.get());
        assert_eq!(updates.faces, (Face::Label("Update"), Face::Label("Update")));
        assert!(!updates.frame(at(0.2 + update_bowl::FOUND_LENGTH), false));
        // Downloading: the capsule turns to the share, the bowl follows
        // what has arrived and never runs ahead of it.
        updates.phase = Phase::Downloading(offer());
        let mut clock = 2.;
        for frame in 0..120 {
            clock += 1. / 60.;
            updates.received.store(frame * 9_800_000 / 120, Ordering::Release);
            assert!(updates.frame(at(clock), false), "frame {frame}");
            assert!(updates.pour.level().0 <= updates.share() + 1e-5);
        }
        assert_eq!(updates.faces, (Face::Label("Update"), Face::Percent));
        assert!(updates.pour.level().0 > 0.8);
        // Verifying keeps the share up; ready turns the capsule into the
        // restart, a beat after the pop begins.
        updates.phase = Phase::Verifying(offer());
        clock += 1.;
        assert!(updates.frame(at(clock), false));
        assert_eq!(updates.faces.1, Face::Percent);
        updates.phase = Phase::Ready(staged());
        assert!(updates.frame(at(clock + 0.01), false));
        assert_eq!(updates.faces.1, Face::Percent);
        assert!(updates.frame(at(clock + 0.02 + READY_CUE), false));
        assert_eq!(updates.faces, (Face::Percent, Face::Label("Restart to update")));
        // Then it rests: a notice that waits for an answer costs nothing.
        assert!(!updates.frame(at(clock + 0.1 + update_bowl::READY_LENGTH), false));
        // Put away, it is not drawn; back, it is simply there.
        updates.later();
        assert!(!updates.frame(at(clock + 3.), false));
        updates.put_away = false;
        assert!(!updates.frame(at(clock + 4.), false));
        assert_eq!(updates.pour.pose().level, 1.);
    }

    #[test]
    fn a_failure_under_way_is_said_in_the_notice_until_it_is_answered() {
        let mut updates = Updates {
            phase: Phase::Downloading(offer()),
            ..Default::default()
        };
        // What `end_update` does with an update that was under way.
        updates.phase = Phase::Idle;
        updates.fail(&Problem::Download, Some(offer()));
        assert!(updates.open());
        let notice = updates.notice().unwrap();
        assert_eq!(notice.title, "Download failed");
        assert_eq!(notice.detail.as_deref(), Some("Check your connection."));
        assert_eq!((notice.action, notice.act), (Some("Try again"), Act::Spilled));
        assert_eq!(updates.face(&notice), Face::Label("Try again"));
        // Nothing is under way: the menu has no row, Settings can check.
        assert_eq!((updates.known(), updates.menu_row()), (None, None));
        assert_eq!(updates.row().1, "Check for updates");
        // "Later" answers it.
        updates.later();
        assert!(!updates.open());
        assert_eq!(updates.notice(), None);
        // So does the same version being found again.
        updates.put_away = false;
        updates.fail(&Problem::Unverified, None);
        assert_eq!(updates.notice().unwrap().title, "Update not verified");
        assert!(updates.quiet_result(Ok(Check::Available(offer())), SystemTime::now()));
        assert_eq!(updates.failure, None);
        assert_eq!(updates.notice().unwrap().title, "Version 0.2.0 is available");
        // Every problem has two short lines.
        for problem in [
            Problem::Download,
            Problem::Unverified,
            Problem::Install,
            Problem::NeedsSystem("15.0".into()),
            Problem::MoveToApplications,
            Problem::CannotUpdateItself,
        ] {
            let (title, detail) = failure_lines(&problem);
            assert!(!title.is_empty() && title.len() <= 22, "{title}");
            assert!(!detail.is_empty() && detail.len() <= 60, "{detail}");
        }
    }

    #[test]
    fn in_settings_the_version_row_is_the_stage_and_the_notice_is_not_shown() {
        let start = Instant::now();
        let mut updates = Updates::default();
        updates.quiet_result(Ok(Check::Available(offer())), SystemTime::now());
        // Over any other page: the notice, as ever.
        updates.frame(start, false);
        assert!(updates.notice_open());
        assert!(updates.fresh.replace(false));
        // Settings → About comes on screen: the same update, in its row,
        // and no notice. The mark carries on from where it was.
        updates.frame(start, true);
        assert!(!updates.notice_open());
        let row = updates.about_row(start);
        assert!(row.open);
        assert_eq!(row.title, updates.notice().unwrap().title);
        assert_eq!(row.act, Act::Found);
        assert_eq!(updates.pour.act(), Act::Found);
        // What the notice last said is kept, for it to leave with.
        assert_eq!(updates.shown.title, "Version 0.2.0 is available");
        // The page is left: the notice is back, as a notice that has just
        // come on.
        updates.frame(start, false);
        assert!(updates.notice_open());
        assert!(updates.fresh.get());
        // "Later" is the same answer in either place: the row is a row
        // again, and no notice comes back.
        updates.later();
        updates.frame(start, true);
        let row = updates.about_row(start);
        assert!(!row.open);
        assert_eq!((row.title.as_str(), row.detail.as_str()), ("Version", "Version 0.2.0 is available"));
        assert_eq!(updates.row_face(), Face::Label("Update"));
        updates.frame(start, false);
        assert!(!updates.notice_open());
    }

    #[test]
    fn the_row_tells_every_part_of_an_update() {
        let now = Instant::now();
        let current = crate::update::current_version();
        let mut updates = Updates::default();
        // Nothing known: a settings row.
        let row = updates.about_row(now);
        assert!(!row.open && !row.checked && !row.line);
        assert_eq!((row.title.as_str(), row.detail.as_str()), ("Version", "Native GPUI build."));
        assert_eq!((row.act, row.pair.clone()), (Act::Still, None));
        assert_eq!(updates.row_face(), Face::Plain("Check for updates"));
        assert!(updates.pressable(updates.row_face()));
        // Checking: still a row, its mark turning, its button saying so
        // and not to be pressed.
        updates.phase = Phase::Checking;
        let row = updates.about_row(now);
        assert!(!row.open);
        assert_eq!((row.detail.as_str(), row.act), ("Checking for updates…", Act::Seeking));
        assert_eq!(updates.row_face(), Face::Plain("Checking…"));
        assert!(!updates.pressable(updates.row_face()));
        // Found: the card, with both versions and the size of the download.
        updates.phase = Phase::Available(offer());
        let row = updates.about_row(now);
        assert!(row.open && !row.resolved && !row.line && !row.sheen);
        assert_eq!(row.title, "Version 0.2.0 is available");
        assert_eq!(row.detail, format!("You have {current} · 9.8 MB download"));
        assert_eq!(row.pair, Some((current.to_owned(), "0.2.0".to_owned())));
        assert_eq!(row.act, Act::Found);
        // Its capsule is the notice's, and names the version here.
        assert_eq!(updates.row_face(), Face::Label("Update"));
        assert_eq!(updates.says(Seat::About, "Update").as_ref(), "Update to 0.2.0");
        assert_eq!(updates.says(Seat::Notice, "Update").as_ref(), "Update");
        // Downloading: counted, with the line along the bottom edge.
        updates.phase = Phase::Downloading(offer());
        updates.received.store(4_214_000, Ordering::Release);
        let row = updates.about_row(now);
        assert!(row.open && row.line);
        assert_eq!((row.counts, row.act), (Some(9_800_000), Act::Pouring));
        // In plain words too, for when nothing rolls.
        assert_eq!(row.detail, "4.2 MB of 9.8 MB (43%)");
        // No pace is said that has not been measured.
        assert_eq!(row.pace, None);
        assert_eq!(updates.row_face(), Face::Percent);
        // Verifying: nothing to count.
        updates.phase = Phase::Verifying(offer());
        let row = updates.about_row(now);
        assert!(row.open && row.line && row.sheen);
        assert_eq!((row.counts, row.act), (None, Act::Proving));
        assert_eq!(row.title, "Verifying the update…");
        // Ready: the pair is the new version alone.
        updates.phase = Phase::Ready(staged());
        let row = updates.about_row(now);
        assert!(row.open && row.resolved && !row.line && !row.sheen);
        assert_eq!((row.title.as_str(), row.act), ("Update ready", Act::Ready));
        assert_eq!(row.pair, Some((current.to_owned(), "0.2.0".to_owned())));
        assert_eq!(updates.row_face(), Face::Label("Restart to update"));
        assert_eq!(updates.says(Seat::About, "Restart to update").as_ref(), "Restart to update");
        // Restarting: nothing left to press.
        updates.installing = Some("0.2.0".into());
        updates.phase = Phase::Restarting;
        let row = updates.about_row(now);
        assert!(row.open && row.resolved && row.sheen);
        assert_eq!(updates.row_face(), Face::None);
        // A failure: the card stays, with the way to try again, until it
        // is answered.
        updates.phase = Phase::Idle;
        updates.fail(&Problem::Download, Some(offer()));
        let row = updates.about_row(now);
        assert!(row.open);
        assert_eq!((row.title.as_str(), row.detail.as_str()), ("Download failed", "Check your connection."));
        assert_eq!((row.act, row.pair.is_some()), (Act::Spilled, true));
        assert_eq!(updates.row_face(), Face::Label("Try again"));
        updates.later();
        assert!(!updates.about_row(now).open);
        assert_eq!(updates.row_face(), Face::Plain("Check for updates"));
    }

    #[test]
    fn up_to_date_is_said_in_the_row_for_a_while_and_then_it_relaxes() {
        let start = Instant::now();
        let at = |seconds: f32| start + motion::slow(Duration::from_secs_f32(seconds));
        let mut updates = Updates {
            phase: Phase::Checking,
            ..Default::default()
        };
        updates.frame(at(0.), true);
        assert_eq!(updates.pour.act(), Act::Seeking);
        // What `end_update` does with a check that found nothing newer.
        updates.phase = Phase::Idle;
        updates.pleased = Some(at(1.));
        assert!(updates.frame(at(1.02), true));
        assert_eq!(updates.pour.act(), Act::Pleased);
        let row = updates.about_row(at(1.02));
        assert!(row.checked && !row.open);
        assert_eq!((row.detail.as_str(), row.act), ("You're up to date.", Act::Pleased));
        // The button can be pressed again at once.
        assert_eq!(updates.row_faces, (Face::Plain("Checking…"), Face::Plain("Check for updates")));
        assert!((updates.pleased_since(at(2.)).unwrap() - 1.).abs() < 1e-3);
        // The nod ends long before the words do, and costs nothing then.
        assert!(!updates.frame(at(1.2 + update_bowl::PLEASED_LENGTH), true));
        assert!(updates.about_row(at(0.9 + PLEASED_FOR)).checked);
        // Then the row is as it was.
        let row = updates.about_row(at(1.1 + PLEASED_FOR));
        assert!(!row.checked);
        assert_eq!((row.detail.as_str(), row.act), ("Native GPUI build.", Act::Still));
        assert_eq!(updates.pleased_since(at(1.1 + PLEASED_FOR)), None);
        // Over another page nothing of it is drawn, and no frame is asked.
        updates.pleased = Some(at(10.));
        assert!(!updates.frame(at(10.1), false));
        assert!(!updates.notice_open());
    }

    #[test]
    fn the_rows_capsule_follows_the_update_while_the_row_is_on_screen() {
        let start = Instant::now();
        let at = |seconds: f32| start + motion::slow(Duration::from_secs_f32(seconds));
        let mut updates = Updates::default();
        // Off screen it is simply what it will be.
        updates.phase = Phase::Available(offer());
        updates.frame(at(0.), false);
        assert_eq!(updates.row_faces, (Face::Label("Update"), Face::Label("Update")));
        // On screen each face gives way to the next.
        updates.frame(at(0.1), true);
        updates.phase = Phase::Downloading(offer());
        updates.download_began = Some(at(0.2));
        assert!(updates.frame(at(0.2), true));
        assert_eq!(updates.row_faces, (Face::Label("Update"), Face::Percent));
        // The mark is the row's while the notice is not shown: it pours.
        assert_eq!(updates.pour.act(), Act::Pouring);
        assert!(!updates.notice_open());
        updates.phase = Phase::Verifying(offer());
        updates.frame(at(3.), true);
        // Ready: the pop first, then the capsule, as in the notice.
        updates.phase = Phase::Ready(staged());
        updates.frame(at(3.01), true);
        assert_eq!(updates.row_faces.1, Face::Percent);
        updates.frame(at(3.02 + READY_CUE), true);
        assert_eq!(updates.row_faces, (Face::Percent, Face::Label("Restart to update")));
        // At rest it asks for no more frames.
        assert!(!updates.frame(at(3.2 + update_bowl::READY_LENGTH), true));
        // Put off, the row is a row with the mark at rest in its tile.
        updates.later();
        updates.frame(at(6.), true);
        assert!(!updates.about_row(at(6.)).open);
        assert_eq!(updates.pour.act(), Act::Still);
    }

    #[test]
    fn a_downloads_speed_is_measured_and_never_made_up() {
        let mut rate = Rate::default();
        assert_eq!((rate.speed(), rate.left(48_000_000)), (None, None));
        // Two megabytes a second, sampled thirty times a second.
        let mut tick = |rate: &mut Rate, frame: u32, per_second: f64| {
            let at = frame as f32 / 30.;
            rate.sample(at, (f64::from(at) * per_second) as u64);
            at
        };
        for frame in 0..=300 {
            let at = tick(&mut rate, frame, 2_000_000.);
            match rate.speed() {
                // Nothing is said of less than a second of it.
                None => assert!(at < RATE_NEEDS + 0.11, "no speed at {at}"),
                Some(speed) => {
                    assert!(at >= RATE_NEEDS);
                    assert!((speed - 2_000_000.).abs() < 20_000., "{speed}");
                }
            }
        }
        // Ten seconds in, twenty megabytes of forty-eight: fourteen to go.
        assert!((rate.left(48_000_000).unwrap() - 14.).abs() < 0.2);
        // Only what was seen lately counts, and little of it is kept.
        assert!(rate.samples.len() <= ((RATE_WINDOW + 0.3) * 10.) as usize, "{}", rate.samples.len());
        assert!(10. - rate.samples.front().unwrap().0 <= RATE_WINDOW + 0.2);
        // It stalls: once the window holds nothing arriving, no speed and
        // no time left are said.
        let held = rate.samples.back().unwrap().1;
        for frame in 301..=420 {
            rate.sample(frame as f32 / 30., held);
        }
        assert_eq!((rate.speed(), rate.left(48_000_000)), (None, None));
        assert_eq!(pace_text(&rate, 48_000_000), None);
        // Another download starts from nothing.
        rate.sample(0., 0);
        assert_eq!(rate.samples.len(), 1);
        rate.sample(2., 19_200_000);
        assert_eq!(pace_text(&rate, 48_000_000).as_deref(), Some("9.6 MB/s · about 3 s left"));
        // All of it has arrived: a speed, and next to no time.
        rate.sample(4., 48_000_000);
        assert_eq!(rate.left(48_000_000), Some(0.));
    }

    #[test]
    fn speeds_and_times_are_said_no_more_exactly_than_they_are_known() {
        assert_eq!(speed_text(9_640_000.), "9.6 MB/s");
        assert_eq!(speed_text(12_300_000.), "12 MB/s");
        assert_eq!(speed_text(9_960_000.), "10 MB/s");
        assert_eq!(speed_text(1_000_000.), "1.0 MB/s");
        assert_eq!(speed_text(640_000.), "640 KB/s");
        assert_eq!(speed_text(200.), "1 KB/s");
        assert_eq!(left_text(0.).as_deref(), Some("about 1 s left"));
        assert_eq!(left_text(3.2).as_deref(), Some("about 4 s left"));
        assert_eq!(left_text(9.4).as_deref(), Some("about 10 s left"));
        assert_eq!(left_text(22.).as_deref(), Some("about 20 s left"));
        assert_eq!(left_text(57.).as_deref(), Some("about 55 s left"));
        assert_eq!(left_text(58.).as_deref(), Some("about 1 min left"));
        assert_eq!(left_text(400.).as_deref(), Some("about 7 min left"));
        // Hours away is not an estimate, and neither is nonsense.
        for none in [3601., f32::INFINITY, f32::NAN, -1.] {
            assert_eq!(left_text(none), None);
        }
    }

    #[test]
    fn the_pace_that_is_said_is_read_from_the_bytes_on_disk() {
        let start = Instant::now();
        let at = |seconds: f32| start + motion::slow(Duration::from_secs_f32(seconds));
        let mut updates = Updates {
            phase: Phase::Downloading(offer()),
            download_began: Some(start),
            ..Default::default()
        };
        // A megabyte a second, sixty frames a second.
        for frame in 0..=180u64 {
            updates.received.store(frame * 1_000_000 / 60, Ordering::Release);
            updates.frame(at(frame as f32 / 60.), true);
            let said = updates.about_row(at(frame as f32 / 60.)).pace;
            if frame < 60 {
                assert_eq!(said, None, "frame {frame}");
            }
        }
        // 3 of 9.8 MB at 1 MB/s; said half a second ago at the latest.
        let said = updates.about_row(at(3.)).pace.unwrap();
        assert!(said.starts_with("1.0 MB/s · about "), "{said}");
        assert!(said.ends_with("7 s left") || said.ends_with("8 s left"), "{said}");
        // The download ends: nothing of it is left over for the next.
        updates.phase = Phase::Verifying(offer());
        updates.frame(at(3.1), true);
        assert_eq!(updates.about_row(at(3.1)).pace, None);
        assert_eq!((updates.rate.clone(), updates.download_began), (Rate::default(), None));
    }

    // The film is in debug builds only.
    #[cfg(debug_assertions)]
    #[test]
    fn the_films_check_is_armed_with_its_answer() {
        let mut updates = Updates::default();
        updates.review("film-check");
        assert_eq!(updates.script_check, Some(true));
        assert_eq!(updates.phase, Phase::Idle);
        assert_eq!(updates.row_face(), Face::Plain("Check for updates"));
        updates.review("film-uptodate");
        assert_eq!(updates.script_check, Some(false));
        // Any other state takes the script away.
        updates.review("idle");
        assert_eq!((updates.script, updates.script_check), (None, None));
    }

    // The film is in debug builds only.
    #[cfg(debug_assertions)]
    #[test]
    fn the_films_download_arrives_unevenly_and_all_of_it() {
        use super::scripted_share;
        assert_eq!(scripted_share(0.), 0.);
        assert!((scripted_share(1.) - 1.).abs() < 1e-6);
        assert!((scripted_share(7.) - 1.).abs() < 1e-6);
        let (mut before, mut slowest, mut fastest) = (0., f32::MAX, 0f32);
        for tick in 1..=100 {
            let now = scripted_share(tick as f32 / 100.);
            assert!(now >= before);
            slowest = slowest.min(now - before);
            fastest = fastest.max(now - before);
            before = now;
        }
        // It nearly stalls, and it bursts: twenty times the pace.
        assert!(fastest > 20. * slowest, "{slowest} to {fastest}");
    }

    #[test]
    fn later_puts_away_only_a_notice_that_waits_for_an_answer() {
        let mut updates = Updates {
            phase: Phase::Available(offer()),
            ..Default::default()
        };
        assert!(updates.open());
        updates.put_away = true;
        assert!(!updates.open());
        // The offer is still there for the button in Settings.
        assert_eq!(updates.row().1, "Update to 0.2.0");
        // Work that is going on is always shown.
        updates.phase = Phase::Downloading(offer());
        assert!(updates.open());
        updates.phase = Phase::Idle;
        assert!(!updates.open());
    }
}
