//! First-run welcome: which step is showing, what this Mac has allowed, and
//! whether the welcome has been completed here. Everything in this file is
//! plain state and plain files; `onboarding_view.rs` draws it.
//!
//! The welcome is shown before anything else on a Mac that has never been
//! through it. A marker in the app's data directory records that it was
//! completed, or how far it got, so quitting half way resumes there. An
//! install that already has notes or a saved session is treated as complete:
//! nobody who was using the app before this existed is interrupted by it.
use crate::{account::Discovery, handoff::SignInMethods};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

/// One screen of the welcome, in the order they are shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    Welcome,
    Server,
    Audio,
    /// Only when notes are kept on a server.
    Calendar,
    Pill,
    Done,
}

impl Step {
    /// A step that may be left without doing what it offers.
    pub fn optional(self) -> bool {
        matches!(self, Step::Audio | Step::Calendar)
    }
}

/// Where the welcome is, and whether this Mac keeps its notes to itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Flow {
    pub step: Step,
    /// "Try it on this Mac only" was chosen: there is no server, so there is
    /// no calendar step either.
    pub local: bool,
}

/// What the steps have achieved so far, read fresh on every frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Facts {
    pub signed_in: bool,
    pub microphone: Permission,
    pub calendar: bool,
}

impl Flow {
    pub const START: Flow = Flow {
        step: Step::Welcome,
        local: false,
    };

    /// The steps of this flow, in order.
    pub fn steps(&self) -> Vec<Step> {
        [
            Step::Welcome,
            Step::Server,
            Step::Audio,
            Step::Calendar,
            Step::Pill,
            Step::Done,
        ]
        .into_iter()
        .filter(|step| *step != Step::Calendar || !self.local)
        .collect()
    }

    pub fn index(&self) -> usize {
        self.steps()
            .iter()
            .position(|step| *step == self.step)
            .unwrap_or(0)
    }

    pub fn next(&self) -> Option<Step> {
        self.steps().get(self.index() + 1).copied()
    }

    pub fn previous(&self) -> Option<Step> {
        self.index()
            .checked_sub(1)
            .and_then(|index| self.steps().get(index).copied())
    }

    /// Whether Continue is offered: the step has done what it is for.
    pub fn can_continue(&self, facts: Facts) -> bool {
        match self.step {
            Step::Welcome | Step::Pill | Step::Done => true,
            Step::Server => self.local || facts.signed_in,
            // Recording works without system audio: the microphone is enough.
            Step::Audio => facts.microphone == Permission::Allowed,
            Step::Calendar => facts.calendar,
        }
    }

    /// Whether "Skip for now" is offered.
    pub fn can_skip(&self, facts: Facts) -> bool {
        self.step.optional() && !self.can_continue(facts)
    }

    /// Continue, or skip an optional step. False when the step has not done
    /// what it is for and may not be skipped, or there is no next step.
    pub fn advance(&mut self, facts: Facts) -> bool {
        if !self.can_continue(facts) && !self.step.optional() {
            return false;
        }
        match self.next() {
            Some(next) => {
                self.step = next;
                true
            }
            None => false,
        }
    }

    pub fn back(&mut self) -> bool {
        match self.previous() {
            Some(previous) => {
                self.step = previous;
                true
            }
            None => false,
        }
    }

    /// Keep notes on this Mac only, and move on from the server step.
    pub fn choose_local(&mut self) {
        self.local = true;
        if self.step == Step::Server {
            self.step = Step::Audio;
        }
    }

    /// A session now exists: the flow is a server flow whatever was chosen
    /// before, and has its calendar step.
    pub fn signed_in(&mut self) {
        self.local = false;
    }
}

// ---------------------------------------------------------------------------
// The marker
// ---------------------------------------------------------------------------

/// What the marker file says.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Saved {
    #[serde(default)]
    pub completed: bool,
    #[serde(default = "first_step")]
    pub step: Step,
    #[serde(default)]
    pub local: bool,
}

fn first_step() -> Step {
    Step::Welcome
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Marker {
    Missing,
    Saved(Saved),
}

/// What was already in the data directory before the welcome existed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Existing {
    /// A workspace of local notes, or an account's cached notes.
    pub notes: bool,
    /// A saved session, or a remembered server.
    pub session: bool,
}

/// Whether the welcome is shown at launch, and from where.
///
/// * A marker that says completed: never.
/// * A marker that says how far it got: from there. A step that no longer
///   applies is replaced by the nearest one that does (the calendar without
///   a server; anything after the server step when the session it relied on
///   has gone).
/// * No marker: only on an install with no notes and no session.
pub fn first_run(marker: Marker, existing: Existing) -> Option<Flow> {
    match marker {
        Marker::Saved(saved) if saved.completed => None,
        Marker::Saved(saved) => {
            let mut flow = Flow {
                step: saved.step,
                local: saved.local,
            };
            let past_server = !matches!(flow.step, Step::Welcome | Step::Server);
            if past_server && !flow.local && !existing.session {
                flow.step = Step::Server;
            }
            if flow.step == Step::Calendar && flow.local {
                flow.step = Step::Pill;
            }
            Some(flow)
        }
        Marker::Missing if existing.notes || existing.session => None,
        Marker::Missing => Some(Flow::START),
    }
}

/// The app's data directory: the one `CEREAL_DATA_DIR` names, or the usual
/// one under Application Support.
fn data_root() -> Option<PathBuf> {
    match crate::environment::var_os("CEREAL_DATA_DIR").filter(|value| !value.is_empty()) {
        Some(directory) => Some(PathBuf::from(directory)),
        None => Some(
            PathBuf::from(std::env::var_os("HOME")?).join("Library/Application Support/Cereal"),
        ),
    }
}

fn marker_path() -> Option<PathBuf> {
    data_root().map(|root| root.join("onboarding.json"))
}

pub fn read_marker(path: &Path) -> Marker {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Saved>(&bytes).ok())
        .map_or(Marker::Missing, Marker::Saved)
}

pub fn write_marker(path: &Path, saved: Saved) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(&saved).map_err(|error| error.to_string())?;
    crate::shared::write_private(path, &bytes)
}

/// What is already in `root`, apart from the welcome's own marker.
pub fn existing_in(root: &Path, session: bool) -> Existing {
    Existing {
        notes: root.join("workspace.json").exists() || root.join("servers").exists(),
        session: session || root.join("connection.json").exists(),
    }
}

/// `CEREAL_ONBOARDING=0` never shows the welcome and `=1` always starts it
/// from the top, for tools that drive the app.
fn forced() -> Option<bool> {
    crate::environment::var("CEREAL_ONBOARDING")
        .ok()
        .map(|value| !matches!(value.trim(), "" | "0" | "false" | "off" | "skip"))
}

/// The flow to show at launch, if any. Review fixtures and the scripted
/// probes start in the app itself, as they always have.
pub fn at_launch() -> Option<Flow> {
    if crate::audio::preview_mode()
        || ["CEREAL_CAPTURE_PROBE", "CEREAL_LIVE_PROBE"]
            .iter()
            .any(|name| crate::environment::var_os(name).is_some())
    {
        return None;
    }
    match forced() {
        Some(false) => return None,
        Some(true) => return Some(Flow::START),
        None => {}
    }
    let path = marker_path()?;
    let root = path.parent()?;
    let marker = read_marker(&path);
    let existing = existing_in(root, crate::accounts::has_saved_session());
    let flow = first_run(marker, existing);
    if flow.is_none() && marker == Marker::Missing {
        // An install from before the welcome: say so once, so that signing
        // out or clearing notes later does not bring it up.
        let _ = write_marker(
            &path,
            Saved {
                completed: true,
                step: Step::Done,
                local: false,
            },
        );
    }
    flow
}

/// Record how far the welcome has got, or that it is done.
pub fn remember(flow: Flow, completed: bool) {
    if let Some(path) = marker_path() {
        let _ = write_marker(
            &path,
            Saved {
                completed,
                step: flow.step,
                local: flow.local,
            },
        );
    }
}

// ---------------------------------------------------------------------------
// The server step
// ---------------------------------------------------------------------------

/// What the server step offers, worked out from what the account form
/// already knows about the address in its field. The form's own discovery,
/// waiting and sign-in are used as they are; this only says what to draw.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ServerOffer {
    /// No address yet, or one still being typed.
    Address,
    Checking,
    Unreachable(String),
    /// Email and password, and the browser button's label when there is one.
    Password { browser: Option<&'static str> },
    /// The browser is the only way in.
    Browser(&'static str),
    WaitingForBrowser,
    SigningIn,
    SignedIn,
}

pub fn server_offer(
    discovery: &Discovery,
    waiting: bool,
    busy: bool,
    signed_in: bool,
) -> ServerOffer {
    if signed_in {
        return ServerOffer::SignedIn;
    }
    if busy {
        return ServerOffer::SigningIn;
    }
    match discovery {
        Discovery::Idle => ServerOffer::Address,
        Discovery::Checking => ServerOffer::Checking,
        Discovery::Failed(message) => ServerOffer::Unreachable(message.clone()),
        Discovery::Ready(_) if waiting => ServerOffer::WaitingForBrowser,
        Discovery::Ready(methods) => offer_for(methods),
    }
}

fn offer_for(methods: &SignInMethods) -> ServerOffer {
    match (methods.password, methods.browser_label()) {
        (true, browser) => ServerOffer::Password { browser },
        (false, Some(label)) => ServerOffer::Browser(label),
        // A server that offers nothing this app can use.
        (false, None) => ServerOffer::Unreachable(
            "This server does not offer a way to sign in from the app.".into(),
        ),
    }
}

/// How the tiny server and the tiny Mac stand in the little screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Link {
    Apart,
    Reaching,
    Joined,
    Broken,
}

pub fn link_for(offer: &ServerOffer) -> Link {
    match offer {
        ServerOffer::Address => Link::Apart,
        ServerOffer::Checking => Link::Reaching,
        ServerOffer::Unreachable(_) => Link::Broken,
        ServerOffer::Password { .. }
        | ServerOffer::Browser(_)
        | ServerOffer::WaitingForBrowser
        | ServerOffer::SigningIn
        | ServerOffer::SignedIn => Link::Joined,
    }
}

/// The name shown under the tiny server: the instance's own, or its host.
pub fn instance_label(methods: Option<&SignInMethods>, address: &str) -> String {
    if let Some(name) = methods
        .and_then(|methods| methods.instance_name.as_deref())
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        return name.chars().take(40).collect();
    }
    host_of(address)
}

pub fn host_of(address: &str) -> String {
    let rest = address
        .trim()
        .split_once("://")
        .map_or(address.trim(), |(_, rest)| rest);
    rest.split('/').next().unwrap_or("").to_owned()
}

// ---------------------------------------------------------------------------
// Permissions
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Permission {
    NotAsked,
    Allowed,
    Denied,
}

impl Permission {
    pub fn label(self) -> &'static str {
        match self {
            Permission::NotAsked => "Not yet",
            Permission::Allowed => "Allowed",
            Permission::Denied => "Denied",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Channel {
    Microphone,
    SystemAudio,
}

impl Channel {
    pub fn index(self) -> usize {
        match self {
            Channel::Microphone => 0,
            Channel::SystemAudio => 1,
        }
    }

    /// The pane of System Settings where this permission is changed.
    pub fn settings_url(self) -> &'static str {
        match self {
            Channel::Microphone => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone"
            }
            Channel::SystemAudio => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_AudioCapture"
            }
        }
    }
}

/// macOS's answer for the microphone (`AVAuthorizationStatus`): 0 not
/// determined, 1 restricted, 2 denied, 3 authorised. Restricted is a Mac
/// whose policy forbids it, which the person can no more change from a
/// prompt than a denial.
pub fn microphone_permission(status: i32) -> Permission {
    match status {
        3 => Permission::Allowed,
        1 | 2 => Permission::Denied,
        _ => Permission::NotAsked,
    }
}

/// macOS's answer for recording system audio: 0 granted, 1 denied, anything
/// else not asked yet (or not known on this version of macOS).
pub fn system_audio_permission(status: i32) -> Permission {
    match status {
        0 => Permission::Allowed,
        1 => Permission::Denied,
        _ => Permission::NotAsked,
    }
}

/// What a listening check says about a channel macOS cannot be asked about:
/// audio arriving means it was allowed; a refusal means it was denied.
pub fn permission_from_check(arrived: bool, refused: bool, before: Permission) -> Permission {
    if arrived {
        Permission::Allowed
    } else if refused {
        Permission::Denied
    } else {
        before
    }
}

/// A stand-in for macOS, for showing every state of the audio step without
/// raising a real prompt: where each permission starts, and what pressing
/// Allow leads to. Debug builds only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Simulation {
    pub start: [Permission; 2],
    pub answer: [Permission; 2],
}

/// `microphone=not-asked:allowed,system=not-asked:denied`: each channel's
/// state at launch and, after the colon, the answer to being asked. A channel
/// that is left out starts not asked and is allowed when asked.
pub fn parse_simulation(text: &str) -> Option<Simulation> {
    let state = |word: &str| match word.trim() {
        "not-asked" | "not_asked" | "unknown" => Some(Permission::NotAsked),
        "allowed" | "granted" => Some(Permission::Allowed),
        "denied" => Some(Permission::Denied),
        _ => None,
    };
    let mut simulation = Simulation {
        start: [Permission::NotAsked; 2],
        answer: [Permission::Allowed; 2],
    };
    let mut any = false;
    for entry in text.split(',').filter(|entry| !entry.trim().is_empty()) {
        let (name, states) = entry.split_once('=')?;
        let channel = match name.trim() {
            "microphone" | "mic" => 0,
            "system" | "system-audio" => 1,
            _ => return None,
        };
        let (start, answer) = match states.split_once(':') {
            Some((start, answer)) => (state(start)?, state(answer)?),
            None => {
                let start = state(states)?;
                (
                    start,
                    if start == Permission::NotAsked {
                        Permission::Allowed
                    } else {
                        start
                    },
                )
            }
        };
        simulation.start[channel] = start;
        simulation.answer[channel] = answer;
        any = true;
    }
    any.then_some(simulation)
}

/// `CEREAL_SIMULATE_PERMISSIONS`, in debug builds. A release build never
/// reads it: what it shows is always what macOS says.
fn simulation_from_environment() -> Option<Simulation> {
    if !cfg!(debug_assertions) {
        return None;
    }
    crate::environment::var("CEREAL_SIMULATE_PERMISSIONS")
        .ok()
        .and_then(|text| parse_simulation(&text))
}

/// A made-up input level, for the simulation only: speech-like bursts for the
/// microphone, a steadier murmur for system audio.
pub fn simulated_level(channel: Channel, seconds: f32) -> f32 {
    let wave = |rate: f32, phase: f32| (seconds * rate + phase).sin() * 0.5 + 0.5;
    let level = match channel {
        Channel::Microphone => {
            let phrase = wave(1.9, 0.3).powf(1.6);
            let syllable = 0.45 + 0.55 * wave(13.7, 1.1);
            let pause = if wave(0.55, 2.0) < 0.22 { 0.08 } else { 1. };
            phrase * syllable * pause
        }
        Channel::SystemAudio => {
            let phrase = wave(1.3, 2.4).powf(1.3);
            let syllable = 0.5 + 0.5 * wave(9.1, 0.4);
            0.75 * phrase * syllable
        }
    };
    level.clamp(0., 1.)
}

/// How long the stand-in for a macOS prompt takes to be answered.
const SIMULATED_ANSWER: Duration = Duration::from_millis(900);

/// What this Mac has allowed, kept current while the audio step is showing.
pub struct Permissions {
    pub state: [Permission; 2],
    /// Asked in this run and not yet answered.
    asked: [Option<Instant>; 2],
    simulation: Option<Simulation>,
    check: Option<crate::audio::LevelCheck>,
    /// Which channels the listening check was started for.
    checking: [bool; 2],
    started: Instant,
    /// For captures: the stand-in does not answer until this is cleared.
    pub hold_answer: bool,
}

impl Permissions {
    /// Nothing known and nothing asked of macOS: for an app that is not
    /// showing the welcome.
    pub fn idle() -> Self {
        Self {
            state: [Permission::NotAsked; 2],
            asked: [None; 2],
            simulation: None,
            check: None,
            checking: [false; 2],
            started: Instant::now(),
            hold_answer: false,
        }
    }

    /// What macOS says now (or its stand-in). Reads only: nothing is asked
    /// for and nothing is listened to.
    pub fn detect() -> Self {
        let simulation = simulation_from_environment();
        let mut permissions = Self {
            state: simulation.map_or([Permission::NotAsked; 2], |simulation| simulation.start),
            simulation,
            ..Self::idle()
        };
        permissions.refresh();
        permissions
    }

    pub fn of(&self, channel: Channel) -> Permission {
        self.state[channel.index()]
    }

    /// Asked, and macOS (or its stand-in) has not answered yet.
    pub fn waiting(&self, channel: Channel) -> bool {
        self.asked[channel.index()].is_some()
    }

    /// Read what macOS says now. Returns whether anything changed.
    pub fn refresh(&mut self) -> bool {
        let before = self.state;
        if let Some(simulation) = self.simulation {
            for index in 0..2 {
                if !self.hold_answer
                    && self.asked[index].is_some_and(|at| at.elapsed() >= SIMULATED_ANSWER)
                {
                    self.asked[index] = None;
                    self.state[index] = simulation.answer[index];
                }
            }
            return self.state != before;
        }
        let levels = self.check.as_ref().map(crate::audio::LevelCheck::levels);
        for channel in [Channel::Microphone, Channel::SystemAudio] {
            let index = channel.index();
            let reported = match (channel, crate::audio::permission_status(index as u32)) {
                (_, None) => None,
                (Channel::Microphone, Some(status)) => Some(microphone_permission(status)),
                (Channel::SystemAudio, Some(status)) => Some(system_audio_permission(status)),
            };
            let channel_levels = levels.as_ref().map(|levels| &levels.channels[index]);
            let arrived = channel_levels.is_some_and(|levels| levels.frames > 0);
            let refused = channel_levels.is_some_and(|levels| levels.refused);
            self.state[index] = match reported {
                // macOS only says "not asked" for system audio when it does
                // not know: what the listening check saw then decides.
                Some(Permission::NotAsked) | None => {
                    permission_from_check(arrived, refused, self.state[index])
                }
                Some(state) => state,
            };
            if self.state[index] != Permission::NotAsked {
                self.asked[index] = None;
            }
        }
        self.state != before
    }

    /// The channels worth listening to: what is allowed, and what has been
    /// asked for (listening is what makes macOS show its prompt).
    fn wanted(&self) -> [bool; 2] {
        [
            self.state[0] == Permission::Allowed || self.asked[0].is_some(),
            self.state[1] == Permission::Allowed || self.asked[1].is_some(),
        ]
    }

    /// While the audio step is showing: read what macOS says, and listen to
    /// exactly what is allowed, so the meters move with the real thing and
    /// whatever is denied or revoked is let go of. Nothing is asked for and
    /// nothing is written. Returns whether a permission changed.
    pub fn follow(&mut self) -> bool {
        let changed = self.refresh();
        if self.simulation.is_none() {
            let wanted = self.wanted();
            if wanted != self.checking {
                self.listen(wanted);
            }
        }
        changed
    }

    /// Ask macOS for `channel`. The app asks the way it always has: by
    /// starting to listen, which is what makes macOS show its prompt.
    pub fn request(&mut self, channel: Channel) {
        let index = channel.index();
        if self.state[index] != Permission::NotAsked || self.asked[index].is_some() {
            return;
        }
        self.asked[index] = Some(Instant::now());
        if self.simulation.is_some() {
            return;
        }
        let wanted = self.wanted();
        self.listen(wanted);
    }

    fn listen(&mut self, channels: [bool; 2]) {
        // The old check lets go of the devices before the new one opens them.
        self.check = None;
        self.checking = channels;
        if channels.contains(&true) {
            self.check = crate::audio::LevelCheck::start(channels[0], channels[1]);
        }
    }

    /// Stop listening: the step was left.
    pub fn stop(&mut self) {
        self.check = None;
        self.checking = [false; 2];
        self.asked = [None; 2];
    }

    /// The input level of `channel` right now, 0..=1. Zero unless allowed.
    pub fn level(&self, channel: Channel) -> f32 {
        let index = channel.index();
        if self.state[index] != Permission::Allowed {
            return 0.;
        }
        if self.simulation.is_some() {
            return simulated_level(channel, self.started.elapsed().as_secs_f32());
        }
        self.check
            .as_ref()
            .map_or(0., |check| check.levels().channels[index].level)
    }

    /// For captures: put a channel in a given state, as the stand-in would.
    #[cfg(debug_assertions)]
    pub fn simulate(&mut self, channel: Channel, state: Permission) {
        if self.simulation.is_none() {
            self.simulation = Some(Simulation {
                start: self.state,
                answer: [Permission::Allowed; 2],
            });
            self.check = None;
            self.checking = [false; 2];
        }
        self.asked[channel.index()] = None;
        self.state[channel.index()] = state;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOTHING: Facts = Facts {
        signed_in: false,
        microphone: Permission::NotAsked,
        calendar: false,
    };
    const EVERYTHING: Facts = Facts {
        signed_in: true,
        microphone: Permission::Allowed,
        calendar: true,
    };

    fn scratch(name: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "cereal-onboarding-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        directory
    }

    #[test]
    fn a_fresh_install_starts_the_welcome_from_the_top() {
        assert_eq!(first_run(Marker::Missing, Existing::default()), Some(Flow::START));
        let root = scratch("fresh");
        assert_eq!(read_marker(&root.join("onboarding.json")), Marker::Missing);
        assert_eq!(existing_in(&root, false), Existing::default());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn an_install_with_notes_or_a_session_is_never_interrupted() {
        for existing in [
            Existing { notes: true, session: false },
            Existing { notes: false, session: true },
            Existing { notes: true, session: true },
        ] {
            assert_eq!(first_run(Marker::Missing, existing), None);
        }
        // What counts: local notes, an account's cached notes, a remembered
        // server, a saved session.
        let root = scratch("existing");
        std::fs::write(root.join("workspace.json"), b"{}").unwrap();
        assert!(existing_in(&root, false).notes);
        std::fs::remove_file(root.join("workspace.json")).unwrap();
        std::fs::create_dir(root.join("servers")).unwrap();
        assert!(existing_in(&root, false).notes);
        assert!(!existing_in(&root, false).session);
        std::fs::write(root.join("connection.json"), b"{}").unwrap();
        assert!(existing_in(&root, false).session);
        assert!(existing_in(&scratch("session"), true).session);
        // The welcome's own marker is not a sign of earlier use.
        let own = scratch("own");
        write_marker(&own.join("onboarding.json"), Saved { completed: false, step: Step::Welcome, local: false }).unwrap();
        assert_eq!(existing_in(&own, false), Existing::default());
        std::fs::remove_dir_all(root).unwrap();
        std::fs::remove_dir_all(own).unwrap();
    }

    #[test]
    fn a_completed_marker_keeps_the_welcome_away_whatever_else_is_there() {
        let done = Marker::Saved(Saved { completed: true, step: Step::Done, local: false });
        assert_eq!(first_run(done, Existing::default()), None);
        assert_eq!(first_run(done, Existing { notes: true, session: true }), None);
        // It survives being written and read back, readable by this user only.
        let root = scratch("marker");
        let path = root.join("onboarding.json");
        write_marker(&path, Saved { completed: true, step: Step::Done, local: true }).unwrap();
        assert_eq!(
            read_marker(&path),
            Marker::Saved(Saved { completed: true, step: Step::Done, local: true })
        );
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        // A marker that cannot be read is no marker.
        std::fs::write(&path, b"not json").unwrap();
        assert_eq!(read_marker(&path), Marker::Missing);
        // One written by a later version with more in it is still read.
        std::fs::write(&path, br#"{"completed":true,"later":1}"#).unwrap();
        assert!(matches!(read_marker(&path), Marker::Saved(Saved { completed: true, .. })));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn quitting_half_way_resumes_on_the_step_that_was_showing() {
        let root = scratch("resume");
        let path = root.join("onboarding.json");
        let mut flow = Flow::START;
        assert!(flow.advance(NOTHING));
        flow.signed_in();
        assert!(flow.advance(EVERYTHING));
        assert_eq!(flow.step, Step::Audio);
        write_marker(&path, Saved { completed: false, step: flow.step, local: flow.local }).unwrap();
        // The next launch: the session is there, and so are its cached notes.
        let existing = Existing { notes: true, session: true };
        assert_eq!(first_run(read_marker(&path), existing), Some(flow));
        // On this Mac only, there is no session to find and none is needed.
        let local = Saved { completed: false, step: Step::Pill, local: true };
        assert_eq!(
            first_run(Marker::Saved(local), Existing { notes: true, session: false }),
            Some(Flow { step: Step::Pill, local: true })
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_resumed_step_that_no_longer_applies_gives_way_to_one_that_does() {
        // The session the later steps relied on has gone: sign in again.
        let saved = Saved { completed: false, step: Step::Calendar, local: false };
        assert_eq!(
            first_run(Marker::Saved(saved), Existing::default()),
            Some(Flow { step: Step::Server, local: false })
        );
        // There is no calendar without a server.
        let saved = Saved { completed: false, step: Step::Calendar, local: true };
        assert_eq!(
            first_run(Marker::Saved(saved), Existing::default()),
            Some(Flow { step: Step::Pill, local: true })
        );
        // The first two steps need nothing.
        for step in [Step::Welcome, Step::Server] {
            let saved = Saved { completed: false, step, local: false };
            assert_eq!(first_run(Marker::Saved(saved), Existing::default()).unwrap().step, step);
        }
    }

    #[test]
    fn the_steps_come_in_order_and_the_calendar_only_with_a_server() {
        let server = Flow::START;
        assert_eq!(
            server.steps(),
            [Step::Welcome, Step::Server, Step::Audio, Step::Calendar, Step::Pill, Step::Done]
        );
        let local = Flow { step: Step::Welcome, local: true };
        assert_eq!(
            local.steps(),
            [Step::Welcome, Step::Server, Step::Audio, Step::Pill, Step::Done]
        );
        // Walking a server flow with everything granted visits each once.
        let mut flow = Flow::START;
        let mut seen = vec![flow.step];
        while flow.advance(EVERYTHING) {
            seen.push(flow.step);
        }
        assert_eq!(seen, server.steps());
        assert_eq!(flow.index(), 5);
        assert_eq!(flow.next(), None);
        assert!(!flow.advance(EVERYTHING));
    }

    #[test]
    fn the_server_step_is_left_only_signed_in_or_on_this_mac_only() {
        let mut flow = Flow { step: Step::Server, local: false };
        assert!(!flow.can_continue(NOTHING) && !flow.can_skip(NOTHING));
        assert!(!flow.advance(NOTHING));
        assert_eq!(flow.step, Step::Server);
        // Signed in: Continue.
        assert!(flow.can_continue(Facts { signed_in: true, ..NOTHING }));
        // This Mac only: straight on, and the calendar step is gone.
        flow.choose_local();
        assert_eq!(flow, Flow { step: Step::Audio, local: true });
        assert!(!flow.steps().contains(&Step::Calendar));
        // Going back and signing in after all brings the calendar back.
        assert!(flow.back());
        assert_eq!(flow.step, Step::Server);
        assert!(flow.can_continue(NOTHING));
        flow.signed_in();
        assert!(flow.steps().contains(&Step::Calendar));
        assert!(!flow.can_continue(NOTHING));
    }

    #[test]
    fn optional_steps_can_be_skipped_and_only_the_microphone_is_needed() {
        let mut flow = Flow { step: Step::Audio, local: false };
        let signed_in = Facts { signed_in: true, ..NOTHING };
        // Nothing allowed: Skip for now, not Continue.
        assert!(!flow.can_continue(signed_in) && flow.can_skip(signed_in));
        // Denied is not allowed either.
        let denied = Facts { microphone: Permission::Denied, ..signed_in };
        assert!(!flow.can_continue(denied) && flow.can_skip(denied));
        // The microphone alone is enough to continue; nothing left to skip.
        let microphone = Facts { microphone: Permission::Allowed, ..signed_in };
        assert!(flow.can_continue(microphone) && !flow.can_skip(microphone));
        // Skipping moves on all the same.
        assert!(flow.advance(signed_in));
        assert_eq!(flow.step, Step::Calendar);
        assert!(!flow.can_continue(signed_in) && flow.can_skip(signed_in));
        assert!(flow.can_continue(Facts { calendar: true, ..signed_in }));
        assert!(flow.advance(signed_in));
        assert_eq!(flow.step, Step::Pill);
        // The steps that only explain are never skipped, only continued.
        for step in [Step::Welcome, Step::Pill, Step::Done] {
            let flow = Flow { step, local: false };
            assert!(flow.can_continue(NOTHING) && !flow.can_skip(NOTHING));
            assert!(!step.optional());
        }
    }

    #[test]
    fn back_retraces_the_steps_and_stops_at_the_first() {
        let mut flow = Flow { step: Step::Pill, local: false };
        let mut seen = Vec::new();
        while flow.back() {
            seen.push(flow.step);
        }
        assert_eq!(seen, [Step::Calendar, Step::Audio, Step::Server, Step::Welcome]);
        assert_eq!(flow.previous(), None);
        // Without a server, Back from the pill skips the calendar too.
        let mut local = Flow { step: Step::Pill, local: true };
        assert!(local.back());
        assert_eq!(local.step, Step::Audio);
    }

    fn methods(password: bool, google: bool, browser: bool) -> SignInMethods {
        SignInMethods { password, google, browser, instance_name: None }
    }

    #[test]
    fn the_server_step_offers_what_discovery_found() {
        let offer = |discovery: &Discovery| server_offer(discovery, false, false, false);
        assert_eq!(offer(&Discovery::Idle), ServerOffer::Address);
        assert_eq!(offer(&Discovery::Checking), ServerOffer::Checking);
        assert_eq!(
            offer(&Discovery::Failed("Could not reach this server".into())),
            ServerOffer::Unreachable("Could not reach this server".into())
        );
        // Email and password, with the browser as well where it is offered.
        assert_eq!(
            offer(&Discovery::Ready(methods(true, false, true))),
            ServerOffer::Password { browser: Some("Continue in browser") }
        );
        assert_eq!(
            offer(&Discovery::Ready(methods(true, true, true))),
            ServerOffer::Password { browser: Some("Continue in browser") }
        );
        // An older server: the password form alone, as before.
        assert_eq!(
            offer(&Discovery::Ready(SignInMethods::PASSWORD_ONLY)),
            ServerOffer::Password { browser: None }
        );
        // Google is the only method: the button says so.
        assert_eq!(
            offer(&Discovery::Ready(methods(false, true, true))),
            ServerOffer::Browser("Continue with Google")
        );
        assert!(matches!(
            offer(&Discovery::Ready(methods(false, false, false))),
            ServerOffer::Unreachable(_)
        ));
    }

    #[test]
    fn waiting_signing_in_and_signed_in_replace_the_form() {
        let ready = Discovery::Ready(methods(true, false, true));
        assert_eq!(server_offer(&ready, true, false, false), ServerOffer::WaitingForBrowser);
        assert_eq!(server_offer(&ready, false, true, false), ServerOffer::SigningIn);
        // The browser answered and the code is being exchanged.
        assert_eq!(server_offer(&ready, true, true, false), ServerOffer::SigningIn);
        // A session wins over whatever the form was doing.
        for discovery in [Discovery::Idle, Discovery::Checking, ready.clone()] {
            assert_eq!(server_offer(&discovery, true, true, true), ServerOffer::SignedIn);
        }
        // Waiting only means something for a server that has answered.
        assert_eq!(server_offer(&Discovery::Checking, true, false, false), ServerOffer::Checking);
    }

    #[test]
    fn the_little_screen_links_up_only_when_the_server_has_answered() {
        assert_eq!(link_for(&ServerOffer::Address), Link::Apart);
        assert_eq!(link_for(&ServerOffer::Checking), Link::Reaching);
        assert_eq!(link_for(&ServerOffer::Unreachable(String::new())), Link::Broken);
        for joined in [
            ServerOffer::Password { browser: None },
            ServerOffer::Browser("Continue with Google"),
            ServerOffer::WaitingForBrowser,
            ServerOffer::SigningIn,
            ServerOffer::SignedIn,
        ] {
            assert_eq!(link_for(&joined), Link::Joined);
        }
    }

    #[test]
    fn the_instance_is_named_by_the_server_or_by_its_host() {
        let named = SignInMethods { instance_name: Some("  Acme Notes ".into()), ..methods(true, false, true) };
        assert_eq!(instance_label(Some(&named), "https://notes.acme.example"), "Acme Notes");
        let unnamed = methods(true, false, true);
        assert_eq!(instance_label(Some(&unnamed), "https://notes.acme.example/base"), "notes.acme.example");
        assert_eq!(instance_label(None, " http://127.0.0.1:3000 "), "127.0.0.1:3000");
        let blank = SignInMethods { instance_name: Some("  ".into()), ..unnamed };
        assert_eq!(instance_label(Some(&blank), "notes.example"), "notes.example");
        assert_eq!(host_of(""), "");
    }

    #[test]
    fn macos_answers_map_to_the_three_states_shown() {
        // AVAuthorizationStatus.
        assert_eq!(microphone_permission(0), Permission::NotAsked);
        assert_eq!(microphone_permission(1), Permission::Denied);
        assert_eq!(microphone_permission(2), Permission::Denied);
        assert_eq!(microphone_permission(3), Permission::Allowed);
        assert_eq!(microphone_permission(-1), Permission::NotAsked);
        // The system audio preflight.
        assert_eq!(system_audio_permission(0), Permission::Allowed);
        assert_eq!(system_audio_permission(1), Permission::Denied);
        assert_eq!(system_audio_permission(2), Permission::NotAsked);
        assert_eq!(system_audio_permission(-1), Permission::NotAsked);
        assert_eq!(Permission::NotAsked.label(), "Not yet");
        assert_eq!(Permission::Allowed.label(), "Allowed");
        assert_eq!(Permission::Denied.label(), "Denied");
        // Each denial has its own pane of System Settings.
        assert!(Channel::Microphone.settings_url().ends_with("Privacy_Microphone"));
        assert!(Channel::SystemAudio.settings_url().ends_with("Privacy_AudioCapture"));
    }

    #[test]
    fn a_listening_check_settles_what_macos_will_not_say() {
        use Permission::*;
        assert_eq!(permission_from_check(true, false, NotAsked), Allowed);
        assert_eq!(permission_from_check(false, true, NotAsked), Denied);
        // Nothing heard yet and nothing refused: no change.
        for before in [NotAsked, Allowed, Denied] {
            assert_eq!(permission_from_check(false, false, before), before);
        }
        // Audio arriving is proof, whatever was thought before.
        assert_eq!(permission_from_check(true, true, Denied), Allowed);
    }

    #[test]
    fn the_stand_in_for_macos_is_read_from_one_line() {
        use Permission::*;
        assert_eq!(
            parse_simulation("microphone=not-asked:allowed,system=not-asked:denied"),
            Some(Simulation { start: [NotAsked, NotAsked], answer: [Allowed, Denied] })
        );
        // A state alone stays as it is; not asked is allowed when asked.
        assert_eq!(
            parse_simulation("mic=denied"),
            Some(Simulation { start: [Denied, NotAsked], answer: [Denied, Allowed] })
        );
        assert_eq!(
            parse_simulation("system=allowed, mic=not-asked"),
            Some(Simulation { start: [NotAsked, Allowed], answer: [Allowed, Allowed] })
        );
        for nonsense in ["", "microphone", "camera=allowed", "mic=maybe", "mic=allowed:perhaps"] {
            assert_eq!(parse_simulation(nonsense), None, "{nonsense}");
        }
    }

    #[test]
    fn asking_the_stand_in_answers_after_a_moment_and_only_once() {
        use Permission::*;
        let mut permissions = Permissions {
            simulation: parse_simulation("mic=not-asked:allowed,system=not-asked:denied"),
            ..Permissions::idle()
        };
        assert_eq!(permissions.level(Channel::Microphone), 0.);
        permissions.request(Channel::Microphone);
        assert!(permissions.waiting(Channel::Microphone));
        assert!(!permissions.refresh());
        assert_eq!(permissions.of(Channel::Microphone), NotAsked);
        // The answer arrives.
        permissions.asked[0] = Some(Instant::now() - SIMULATED_ANSWER);
        assert!(permissions.refresh());
        assert_eq!(permissions.of(Channel::Microphone), Allowed);
        assert!(!permissions.waiting(Channel::Microphone));
        // Asking again for something already answered does nothing.
        permissions.request(Channel::Microphone);
        assert!(!permissions.waiting(Channel::Microphone));
        // The other channel is denied, and stays silent.
        permissions.request(Channel::SystemAudio);
        permissions.asked[1] = Some(Instant::now() - SIMULATED_ANSWER);
        assert!(permissions.refresh());
        assert_eq!(permissions.of(Channel::SystemAudio), Denied);
        assert_eq!(permissions.level(Channel::SystemAudio), 0.);
        // No listening check is ever started for the stand-in, however
        // often the step follows it.
        permissions.follow();
        assert!(permissions.check.is_none() && permissions.checking == [false; 2]);
        // An app that is not showing the welcome knows nothing and asks nothing.
        let idle = Permissions::idle();
        assert_eq!(idle.state, [NotAsked; 2]);
        assert!(idle.check.is_none() && !idle.waiting(Channel::Microphone));
    }

    #[test]
    fn the_made_up_level_stays_in_range_and_moves() {
        for channel in [Channel::Microphone, Channel::SystemAudio] {
            let levels: Vec<f32> = (0..600).map(|tick| simulated_level(channel, tick as f32 / 30.)).collect();
            assert!(levels.iter().all(|level| (0. ..=1.).contains(level)));
            let (low, high) = levels.iter().fold((1f32, 0f32), |(low, high), level| (low.min(*level), high.max(*level)));
            assert!(low < 0.1 && high > 0.5, "{channel:?}: {low}..{high}");
        }
    }
}
