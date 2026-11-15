//! The welcome on screen: a two-column stage in the main window. The left
//! column has the step's inputs; the right has a little screen that shows
//! what the step sets up and reacts to what happens on the left. The account
//! form's own logic does the signing in; the app's own loaders find the
//! calendar; nothing here is a second implementation of either.
use crate::{
    account::Discovery,
    app::{Cereal, Screen},
    input::TextInput,
    motion::{self, GOOEY, SETTLE, SNAPPY},
    onboarding::*,
    onboarding_scene::{self as scenes, Character, Mini, Scene, TinyEvent, Travel},
    ui::*,
};
use gpui::{prelude::*, *};
use std::{
    cell::RefCell,
    collections::HashMap,
    time::{Duration, Instant},
};

actions!(welcome, [Continue]);

pub fn init(cx: &mut App) {
    // Return continues, from anywhere in the welcome that is not typing.
    cx.bind_keys([KeyBinding::new("enter", Continue, Some("CerealWelcome"))]);
}

/// Where the stage and its two columns are, for a window of a given size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub left: f32,
    pub gap: f32,
    pub right: f32,
    /// Narrow windows get the smaller type and tighter spacing.
    pub compact: bool,
    /// Pixels per unit of the little screen.
    pub k: f32,
}

/// Room kept clear above the stage for the window's traffic lights.
const TOP: f32 = 52.;
/// The line of text under the little screen, and the gap above it.
const CAPTION: f32 = 44.;

pub fn layout(window: Size<Pixels>) -> Layout {
    let (w, h) = (f32::from(window.width), f32::from(window.height));
    let side = if w < 900. { 24. } else { 48. };
    let bottom = if h < 660. { 24. } else { 40. };
    let width = (w - 2. * side).clamp(0., 1040.);
    let height = (h - TOP - bottom).clamp(0., 600.);
    let compact = width < 880.;
    let (left, gap) = if compact { (316., 28.) } else { (392., 56.) };
    let right = (width - left - gap).max(0.);
    let inset = if compact { 24. } else { 40. };
    let k = ((right - 2. * inset) / scenes::SCREEN_BOX.0)
        .min((height - 2. * inset - CAPTION) / scenes::SCREEN_BOX.1)
        .clamp(1., 1.5);
    Layout {
        x: ((w - width) / 2.).round(),
        y: ((h - height) / 2.).max(TOP).round(),
        width,
        height,
        left,
        gap,
        right,
        compact,
        k,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    Address,
    Email,
    Password,
}

/// What wakes the welcome up again when nothing is being pressed.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Wake {
    Never,
    /// Thirty times a second: a meter, a search in progress.
    Tick,
    At(Instant),
}

/// What the last frame knew, to notice what has just happened.
#[derive(Clone, Debug, PartialEq)]
struct Seen {
    signed_in: bool,
    permissions: [Permission; 2],
    calendar: bool,
    error: Option<String>,
}

pub struct Onboarding {
    pub flow: Option<Flow>,
    /// Shown again from Settings: nothing is remembered, and the first
    /// step's Back returns to the app.
    pub replay: bool,
    pub permissions: Permissions,
    pub focus: FocusHandle,
    arrived: Instant,
    direction: f32,
    character: Character,
    travel: Travel,
    link: Link,
    link_changed: Instant,
    seen: Option<Seen>,
    trying: Option<Instant>,
    calendar_found: Option<Instant>,
    calendar_asked: bool,
    calendar_polled: Instant,
    /// For captures: the calendar is not looked for until this is cleared.
    pub calendar_hold: bool,
    permissions_polled: Instant,
    wake: Wake,
    clock: bool,
    focus_field: Option<Field>,
    focus_stage: bool,
}

impl Onboarding {
    pub fn new(flow: Option<Flow>, cx: &mut App) -> Self {
        let now = Instant::now();
        let fresh = flow.is_none_or(|flow| flow.step == Step::Welcome);
        let mut character = if fresh {
            Character::asleep(now)
        } else {
            Character::awake(now)
        };
        // It wakes a moment after the window has appeared.
        character.wake(now + Duration::from_millis(650));
        Self {
            flow,
            replay: false,
            // Only a welcome that is showing asks macOS anything.
            permissions: if flow.is_some() {
                Permissions::detect()
            } else {
                Permissions::idle()
            },
            focus: cx.focus_handle(),
            arrived: now,
            direction: 1.,
            character,
            travel: Travel::at(flow.map_or(0, |flow| flow.index())),
            link: Link::Apart,
            link_changed: now,
            seen: None,
            trying: None,
            calendar_found: None,
            calendar_asked: false,
            calendar_polled: now,
            calendar_hold: false,
            permissions_polled: now,
            wake: Wake::Never,
            clock: false,
            focus_field: None,
            focus_stage: true,
        }
    }

    pub fn step(&self) -> Option<Step> {
        self.flow.map(|flow| flow.step)
    }
}

thread_local! {
    /// Where the welcome's controls and columns were last drawn, by name:
    /// what a scripted run presses, and what its layout is measured from.
    static PLACES: RefCell<HashMap<&'static str, Bounds<Pixels>>> = RefCell::new(HashMap::new());
}

fn place(name: &'static str) -> impl IntoElement {
    canvas(
        move |bounds, _, _| {
            PLACES.with(|places| places.borrow_mut().insert(name, bounds));
        },
        |_, _, _, _| {},
    )
    .absolute()
    .inset_0()
}

/// Where the thing called `name` was last drawn.
pub fn placed(name: &str) -> Option<Bounds<Pixels>> {
    PLACES.with(|places| places.borrow().get(name).copied())
}

/// The welcome's one primary action: a gold pill with dark ink and a small
/// arrow. It is the only place in the flow where the accent leads.
fn primary_button(
    id: &'static str,
    label: impl Into<SharedString>,
    enabled: bool,
    t: Theme,
) -> Stateful<Div> {
    let ink = scenes::on_gold();
    div()
        .id(id)
        .relative()
        .h(px(40.))
        .pl(px(20.))
        .pr(px(16.))
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .gap(px(8.))
        .rounded_full()
        .text_size(px(14.))
        .line_height(px(20.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(if enabled { ink } else { ink.opacity(0.45) })
        .whitespace_nowrap()
        .map(|button| {
            if enabled {
                button.cursor_pointer().reach().child(
                    motion::tactile(neutral_wash(0.16))
                        .press(neutral_wash(0.14))
                        .round()
                        .base(scenes::gold()),
                )
            } else {
                button.child(
                    div()
                        .absolute()
                        .inset_0()
                        .rounded_full()
                        .bg(motion::mix(t.bg, scenes::gold(), 0.38)),
                )
            }
        })
        .child(place(id))
        .child(label.into())
        .child(
            // The app's arrow, pointing on.
            icon("arrow-left", 14., if enabled { ink } else { ink.opacity(0.45) })
                .with_transformation(Transformation::rotate(percentage(0.5))),
        )
}

fn secondary_button(
    id: &'static str,
    label: impl Into<SharedString>,
    symbol: &str,
    t: Theme,
) -> Stateful<Div> {
    div()
        .id(id)
        .relative()
        .h(px(36.))
        .px(px(16.))
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .gap(px(7.))
        .rounded_full()
        .border_1()
        .border_color(t.hairline)
        .text_size(px(14.))
        .line_height(px(20.))
        .text_color(t.text)
        .whitespace_nowrap()
        .cursor_pointer()
        .reach()
        .child(motion::tactile(t.hover).press(t.border.opacity(0.6)).round())
        .child(place(id))
        .when(!symbol.is_empty(), |button| {
            button.child(icon(symbol, 16., t.secondary))
        })
        .child(label.into())
}

fn quiet_button(id: &'static str, label: impl Into<SharedString>, t: Theme) -> Stateful<Div> {
    quiet_button_with(id, label, "", t)
}

fn quiet_button_with(
    id: &'static str,
    label: impl Into<SharedString>,
    symbol: &str,
    t: Theme,
) -> Stateful<Div> {
    div()
        .id(id)
        .relative()
        .h(px(36.))
        .px(px(12.))
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .gap(px(4.))
        .rounded_full()
        .text_size(px(14.))
        .line_height(px(20.))
        .text_color(t.muted)
        .whitespace_nowrap()
        .cursor_pointer()
        .reach()
        .child(motion::tactile(t.hover).press(t.border.opacity(0.6)).round())
        .child(place(id))
        .when(!symbol.is_empty(), |button| {
            button.child(icon(symbol, 14., t.muted))
        })
        .child(label.into())
}

/// A text field of the welcome: 40 px, on the card colour, with the accent
/// for an edge while it is being typed in.
fn field(input: &Entity<TextInput>, focused: bool, t: Theme) -> Div {
    div()
        .w_full()
        .h(px(40.))
        .flex_shrink_0()
        .px(px(12.))
        .flex()
        .items_center()
        .rounded(px(10.))
        .bg(t.card)
        .border_1()
        .border_color(if focused { focus_ring() } else { t.border })
        .text_size(px(14.))
        .line_height(px(20.))
        .child(input.clone())
}

/// A line that says what is going on: a spinner or an icon, then the words.
fn status(lead: AnyElement, text: impl Into<SharedString>, tone: Hsla) -> Div {
    div()
        .w_full()
        .min_h(px(20.))
        .flex()
        .items_center()
        .gap(px(8.))
        .text_size(px(13.))
        .line_height(px(18.))
        .text_color(tone)
        .child(lead)
        .child(div().flex_1().min_w_0().child(text.into()))
}

/// One thing with its own button: a 32 px icon tile, a title over a state,
/// and whatever the caller appends at the trailing edge.
fn choice(symbol: &str, title: &'static str, state: impl Into<SharedString>, tone: Hsla, lit: bool, t: Theme) -> Div {
    div()
        .w_full()
        .min_h(px(56.))
        .px(px(12.))
        .py(px(10.))
        .flex()
        .items_center()
        .gap(px(12.))
        .rounded(px(12.))
        .bg(t.card)
        .border_1()
        .border_color(t.border)
        .child(
            div()
                .size(px(32.))
                .flex_shrink_0()
                .rounded(px(8.))
                .bg(t.hover)
                .flex()
                .items_center()
                .justify_center()
                .child(icon(symbol, 16., if lit { t.accent } else { t.secondary })),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(1.))
                .child(
                    div()
                        .text_size(px(14.))
                        .line_height(px(19.))
                        .text_color(t.text)
                        .whitespace_nowrap()
                        .child(title),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(tone)
                        .truncate()
                        .child(state.into()),
                ),
        )
}

fn hex_color(text: &str, fallback: Hsla) -> Hsla {
    let digits = text.trim().trim_start_matches('#');
    match u32::from_str_radix(digits, 16) {
        Ok(value) if digits.len() == 6 => rgb(value).into(),
        _ => fallback,
    }
}

impl Cereal {
    /// Whether a session exists on the server this app is set up to use.
    fn welcome_session(&self) -> Option<meeting_notes_api_types::AuthSessionDTO> {
        crate::account::session().filter(|_| crate::server::enabled())
    }

    fn welcome_calendar(&self) -> bool {
        self.calendars.loaded
            && (self.calendars.personal_linked || !self.calendars.sources.is_empty())
    }

    pub fn onboarding_facts(&self) -> Facts {
        Facts {
            signed_in: self.welcome_session().is_some(),
            microphone: self.onboarding.permissions.of(Channel::Microphone),
            calendar: self.welcome_calendar(),
        }
    }

    /// What the server step offers right now, from the account form's state.
    pub fn onboarding_offer(&self, cx: &App) -> ServerOffer {
        let account = self.account.read(cx);
        server_offer(
            account.discovery(),
            account.waiting(),
            account.busy(),
            self.welcome_session().is_some(),
        )
    }

    /// Show the welcome from `flow`. `replay` is a second showing, asked for
    /// in Settings: it is not remembered and can be left from its first step.
    pub fn begin_onboarding(&mut self, flow: Flow, replay: bool, cx: &mut Context<Self>) {
        let mut onboarding = Onboarding::new(Some(flow), cx);
        onboarding.replay = replay;
        self.onboarding = onboarding;
        self.menu = None;
        self.search_open = false;
        self.current_note = None;
        self.screen = Screen::Welcome;
        if !replay {
            remember(flow, false);
        }
        cx.notify();
    }

    /// Settings → About → "Show welcome again".
    pub fn show_welcome_again(&mut self, cx: &mut Context<Self>) {
        self.finish_note(cx);
        let local = !crate::server::enabled();
        self.begin_onboarding(
            Flow {
                step: Step::Welcome,
                local,
            },
            true,
            cx,
        );
    }

    /// Everything that follows from the step having changed.
    fn onboarding_entered(&mut self, from: Step, cx: &mut Context<Self>) {
        let Some(flow) = self.onboarding.flow else {
            return;
        };
        let now = Instant::now();
        let before = Flow { step: from, ..flow }.index();
        let onboarding = &mut self.onboarding;
        onboarding.direction = if flow.index() >= before { 1. } else { -1. };
        onboarding.arrived = now;
        onboarding.travel.go(flow.index(), now);
        onboarding.focus_stage = true;
        onboarding.focus_field = None;
        onboarding.trying = None;
        if from == Step::Audio {
            onboarding.permissions.stop();
        }
        match flow.step {
            Step::Server => {
                let signed_in = crate::account::session().is_some() && crate::server::enabled();
                if !signed_in {
                    onboarding.focus_field = Some(Field::Address);
                }
            }
            Step::Audio => {
                onboarding.permissions.follow();
                onboarding.permissions_polled = now;
            }
            Step::Calendar => {
                onboarding.calendar_asked = false;
                onboarding.calendar_polled = now;
                // Already connected: the list is simply there.
                onboarding.calendar_found = None;
                self.load_calendar_events(cx);
            }
            Step::Done => onboarding.character.hop(now + Duration::from_millis(260)),
            Step::Welcome | Step::Pill => {}
        }
        if !self.onboarding.replay {
            remember(flow, false);
        }
        cx.notify();
    }

    fn onboarding_advance(&mut self, cx: &mut Context<Self>) {
        let facts = self.onboarding_facts();
        let Some(flow) = self.onboarding.flow.as_mut() else {
            return;
        };
        let from = flow.step;
        if flow.advance(facts) {
            self.onboarding_entered(from, cx);
        }
    }

    /// Continue: the step's own action. On the server step that is signing
    /// in, by whichever way the server offers; on the last it opens the app.
    pub fn onboarding_continue(&mut self, cx: &mut Context<Self>) {
        let Some(flow) = self.onboarding.flow else {
            return;
        };
        let facts = self.onboarding_facts();
        match flow.step {
            Step::Done => self.finish_onboarding(cx),
            _ if flow.can_continue(facts) => self.onboarding_advance(cx),
            Step::Server => match self.onboarding_offer(cx) {
                ServerOffer::Password { .. } => {
                    self.account.update(cx, |account, cx| account.submit(cx));
                }
                ServerOffer::Browser(_) => {
                    self.account
                        .update(cx, |account, cx| account.continue_in_browser(cx));
                }
                ServerOffer::Address | ServerOffer::Unreachable(_) => {
                    let empty = self.account.read(cx).url.read(cx).content().trim().is_empty();
                    if empty {
                        self.onboarding.character.wobble(Instant::now());
                        self.onboarding.focus_field = Some(Field::Address);
                    } else {
                        self.account.update(cx, |account, cx| account.check_again(cx));
                    }
                }
                _ => {}
            },
            // An optional step is never skipped by accident.
            _ => self.onboarding.character.wobble(Instant::now()),
        }
        cx.notify();
    }

    pub fn onboarding_skip(&mut self, cx: &mut Context<Self>) {
        if self.onboarding.flow.is_some_and(|flow| flow.step.optional()) {
            self.onboarding_advance(cx);
        }
    }

    /// Back, or Escape. Returns whether there was anywhere to go back to.
    pub fn onboarding_back(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(flow) = self.onboarding.flow.as_mut() else {
            return false;
        };
        let from = flow.step;
        if flow.back() {
            // A sign-in that was waiting for the browser is given up.
            if from == Step::Server {
                self.account.update(cx, |account, cx| account.cancel_browser(cx));
            }
            self.onboarding_entered(from, cx);
            return true;
        }
        if self.onboarding.replay {
            self.leave_onboarding(cx);
            return true;
        }
        false
    }

    pub fn onboarding_choose_local(&mut self, cx: &mut Context<Self>) {
        let Some(flow) = self.onboarding.flow.as_mut() else {
            return;
        };
        let from = flow.step;
        flow.choose_local();
        self.account.update(cx, |account, cx| account.cancel_browser(cx));
        self.onboarding_entered(from, cx);
    }

    /// A session was accepted while the welcome is showing: it stays, the
    /// mark is pleased, and the server step moves on by itself.
    pub fn onboarding_signed_in(&mut self, cx: &mut Context<Self>) {
        let Some(flow) = self.onboarding.flow.as_mut() else {
            return;
        };
        flow.signed_in();
        let flow = *flow;
        if !self.onboarding.replay {
            remember(flow, false);
        }
        if flow.step != Step::Server {
            return;
        }
        self.onboarding.focus_stage = true;
        let arrived = self.onboarding.arrived;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(motion::slow(Duration::from_millis(1500)))
                .await;
            let _ = this.update(cx, |this, cx| {
                let still = this.screen == Screen::Welcome
                    && this.onboarding.arrived == arrived
                    && this.onboarding.step() == Some(Step::Server);
                if still {
                    this.onboarding_advance(cx);
                }
            });
        })
        .detach();
    }

    pub fn onboarding_allow(&mut self, channel: Channel, cx: &mut Context<Self>) {
        self.onboarding.permissions.request(channel);
        cx.notify();
    }

    fn onboarding_open_settings(&mut self, channel: Channel, cx: &mut Context<Self>) {
        open_system_settings(channel.settings_url(), cx);
    }

    pub fn onboarding_connect_calendar(&mut self, cx: &mut Context<Self>) {
        if let Some(url) = crate::server::calendar_connections_url() {
            crate::shared_view::open_in_browser(&url, cx);
            self.onboarding.calendar_asked = true;
            self.onboarding.calendar_polled = Instant::now();
        }
        cx.notify();
    }

    pub fn onboarding_try_pill(&mut self, cx: &mut Context<Self>) {
        self.onboarding.trying = Some(Instant::now());
        cx.notify();
    }

    /// "Open Cereal": the welcome is complete, and Home arrives the way any
    /// page does.
    pub fn finish_onboarding(&mut self, cx: &mut Context<Self>) {
        if let Some(flow) = self.onboarding.flow
            && !self.onboarding.replay
        {
            remember(flow, true);
        }
        self.leave_onboarding(cx);
    }

    fn leave_onboarding(&mut self, cx: &mut Context<Self>) {
        self.onboarding.flow = None;
        self.onboarding.permissions.stop();
        self.screen = Screen::Home;
        self.navigate(Screen::Home, cx);
    }

    /// Return in the address field.
    pub fn onboarding_address_entered(&mut self, cx: &mut Context<Self>) {
        if self.screen != Screen::Welcome || self.onboarding.step() != Some(Step::Server) {
            return;
        }
        match self.onboarding_offer(cx) {
            ServerOffer::Password { .. } => self.onboarding.focus_field = Some(Field::Email),
            ServerOffer::Browser(_) => {
                self.account
                    .update(cx, |account, cx| account.continue_in_browser(cx));
            }
            ServerOffer::Address | ServerOffer::Unreachable(_) => {
                self.account.update(cx, |account, cx| account.check_again(cx));
            }
            _ => {}
        }
        cx.notify();
    }

    /// Return in the email field.
    pub fn onboarding_email_entered(&mut self, cx: &mut Context<Self>) {
        if self.screen == Screen::Welcome && self.onboarding.step() == Some(Step::Server) {
            self.onboarding.focus_field = Some(Field::Password);
            cx.notify();
        }
    }

    /// One timer for whatever keeps going with nobody pressing anything: it
    /// sleeps until the next thing is due and ends when nothing is.
    fn onboarding_clock(&mut self, cx: &mut Context<Self>) {
        if self.onboarding.clock || self.onboarding.wake == Wake::Never {
            return;
        }
        self.onboarding.clock = true;
        cx.spawn(async move |this, cx| {
            loop {
                let wake = this
                    .update(cx, |this, _| {
                        if this.screen == Screen::Welcome {
                            this.onboarding.wake
                        } else {
                            Wake::Never
                        }
                    })
                    .unwrap_or(Wake::Never);
                // A far-off deadline is slept towards in short stretches, so
                // that something sooner (a step with a meter) is not kept
                // waiting behind it.
                let (wait, due) = match wake {
                    Wake::Never => break,
                    Wake::Tick => (Duration::from_millis(33), true),
                    Wake::At(at) => {
                        let left = at.saturating_duration_since(Instant::now());
                        if left <= Duration::from_millis(200) {
                            (left.max(Duration::from_millis(8)), true)
                        } else {
                            (Duration::from_millis(200), false)
                        }
                    }
                };
                cx.background_executor().timer(wait).await;
                if !due {
                    continue;
                }
                motion::stats::cause("loop");
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    return;
                }
            }
            let _ = this.update(cx, |this, _| this.onboarding.clock = false);
        })
        .detach();
    }

    pub fn render_onboarding(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(flow) = self.onboarding.flow else {
            return div().into_any_element();
        };
        let t = self.theme();
        let now = Instant::now();
        let reduced = motion::reduce_motion();
        let step = flow.step;
        let l = layout(window.viewport_size());
        let since = now.saturating_duration_since(self.onboarding.arrived).as_secs_f32()
            / motion::slow(Duration::from_secs(1)).as_secs_f32();

        // ---- what is true right now -------------------------------------
        if step == Step::Server {
            self.account.update(cx, |account, cx| account.follow_address(cx));
        }
        if step == Step::Audio
            && now.saturating_duration_since(self.onboarding.permissions_polled)
                >= Duration::from_millis(250)
        {
            self.onboarding.permissions_polled = now;
            self.onboarding.permissions.follow();
        }
        if step == Step::Calendar
            && !self.onboarding.calendar_hold
            && !self.welcome_calendar()
            && !self.calendars.loading
            && now.saturating_duration_since(self.onboarding.calendar_polled)
                >= Duration::from_millis(if self.onboarding.calendar_asked { 2500 } else { 6000 })
        {
            self.onboarding.calendar_polled = now;
            self.load_calendar_events(cx);
        }
        let session = self.welcome_session();
        let facts = self.onboarding_facts();
        let offer = self.onboarding_offer(cx);
        let (address, methods, error, inputs) = {
            let account = self.account.read(cx);
            (
                account.url.read(cx).content(),
                match account.discovery() {
                    Discovery::Ready(methods) => Some(methods.clone()),
                    _ => None,
                },
                account.error().map(str::to_owned),
                (
                    account.url.clone(),
                    account.email.clone(),
                    account.password.clone(),
                ),
            )
        };
        let address = if address.trim().is_empty() {
            crate::account::configured_url()
        } else {
            address
        };
        let instance = instance_label(methods.as_ref(), &address);
        let host = host_of(&address);
        let permissions = self.onboarding.permissions.state;
        let asking = [
            self.onboarding.permissions.waiting(Channel::Microphone),
            self.onboarding.permissions.waiting(Channel::SystemAudio),
        ];
        let levels = [
            self.onboarding.permissions.level(Channel::Microphone),
            self.onboarding.permissions.level(Channel::SystemAudio),
        ];

        // ---- what has just happened --------------------------------------
        let link = link_for(&offer);
        if link != self.onboarding.link {
            if step == Step::Server {
                match link {
                    Link::Joined if self.onboarding.link != Link::Joined => {
                        self.onboarding.character.hop(now)
                    }
                    Link::Broken => self.onboarding.character.wobble(now),
                    _ => {}
                }
            }
            self.onboarding.link = link;
            self.onboarding.link_changed = now;
        }
        let seen = Seen {
            signed_in: facts.signed_in,
            permissions,
            calendar: facts.calendar,
            error: error.clone(),
        };
        if let Some(before) = self.onboarding.seen.as_ref()
            && *before != seen
        {
            let character = &mut self.onboarding.character;
            if seen.signed_in && !before.signed_in {
                character.hop(now);
            }
            for index in 0..2 {
                if seen.permissions[index] != before.permissions[index] && step == Step::Audio {
                    match seen.permissions[index] {
                        Permission::Allowed => character.hop(now),
                        Permission::Denied => character.wobble(now),
                        Permission::NotAsked => {}
                    }
                }
            }
            if seen.calendar && !before.calendar && step == Step::Calendar {
                character.hop(now);
                self.onboarding.calendar_found = Some(now);
            }
            if seen.error.is_some() && seen.error != before.error {
                character.wobble(now);
            }
        }
        self.onboarding.seen = Some(seen);

        // ---- focus --------------------------------------------------------
        if let Some(wanted) = self.onboarding.focus_field.take() {
            let input = match wanted {
                Field::Address => &inputs.0,
                Field::Email => &inputs.1,
                Field::Password => &inputs.2,
            };
            input.read(cx).focus_handle.focus(window);
            self.onboarding.focus_stage = false;
        } else if std::mem::take(&mut self.onboarding.focus_stage) || self.focus.is_focused(window) {
            self.onboarding.focus.focus(window);
        }
        let focused = |input: &Entity<TextInput>| input.read(cx).focus_handle.is_focused(window);
        let typing = [focused(&inputs.0), focused(&inputs.1), focused(&inputs.2)];

        // ---- the character and the indicator ------------------------------
        let (pose, character_moving, next_blink) = self.onboarding.character.pose(now, reduced);
        let merge = self.stage.aim(
            ("welcome-merge", 0),
            if step == Step::Done && since > 0.3 { 1. } else { 0. },
            GOOEY,
        );
        let steps = flow.steps();
        let dots = scenes::indicator(
            &self.stage,
            &steps,
            &self.onboarding.travel,
            merge,
            t,
            now,
            reduced,
        );

        // ---- the left column, step by step ---------------------------------
        let title: &'static str;
        let help: SharedString;
        let mut body: Vec<AnyElement> = Vec::new();
        let mut primary: (SharedString, bool) = ("Continue".into(), flow.can_continue(facts));
        let waiting_line = |text: &'static str| status(motion::spinner(t.muted), text, t.muted).into_any_element();
        let good_line = |text: String| status(icon("check", 16., t.accent).into_any_element(), text, t.text).into_any_element();
        match step {
            Step::Welcome => {
                title = "Welcome to Cereal";
                help = "Cereal listens to your meetings and writes the notes, so you can stay in the conversation.".into();
            }
            Step::Server => {
                title = "Find your server";
                help = "Enter your Cereal server’s address, then sign in. Your notes are kept there.".into();
                if let Some(session) = &session {
                    body.push(
                        choice(
                            "check",
                            "Signed in",
                            format!("{} · {}", session.user.email, instance),
                            t.muted,
                            true,
                            t,
                        )
                        .into_any_element(),
                    );
                } else {
                    body.push(field(&inputs.0, typing[0], t).into_any_element());
                    match &offer {
                        ServerOffer::Address => body.push(
                            small("It looks like https://notes.example.com", t).into_any_element(),
                        ),
                        ServerOffer::Checking => body.push(waiting_line("Looking for your server…")),
                        ServerOffer::Unreachable(message) => {
                            body.push(
                                status(
                                    icon("warning", 16., t.warning_ink).into_any_element(),
                                    message.clone(),
                                    t.warning_ink,
                                )
                                .into_any_element(),
                            );
                            body.push(
                                div()
                                    .flex()
                                    .child(secondary_button("welcome-check-again", "Try again", "refresh", t).on_click(
                                        cx.listener(|this, _, _, cx| {
                                            this.account.update(cx, |account, cx| account.check_again(cx))
                                        }),
                                    ))
                                    .into_any_element(),
                            );
                        }
                        ServerOffer::Password { browser } => {
                            body.push(field(&inputs.1, typing[1], t).into_any_element());
                            body.push(field(&inputs.2, typing[2], t).into_any_element());
                            if flow.local {
                                // Continue goes on without a server here, so
                                // signing in gets a button of its own.
                                body.push(
                                    div()
                                        .flex()
                                        .child(secondary_button("welcome-sign-in", "Sign in", "", t).on_click(
                                            cx.listener(|this, _, _, cx| {
                                                this.account.update(cx, |account, cx| account.submit(cx))
                                            }),
                                        ))
                                        .into_any_element(),
                                );
                            } else {
                                primary = ("Sign in".into(), true);
                            }
                            if let Some(label) = *browser {
                                body.push(browser_button(label, t, cx));
                            }
                        }
                        ServerOffer::Browser(label) => {
                            if flow.local {
                                body.push(browser_button(*label, t, cx));
                            } else {
                                primary = ((*label).into(), true);
                            }
                        }
                        ServerOffer::WaitingForBrowser => {
                            body.push(waiting_line("Waiting for you to sign in in your browser…"));
                            body.push(
                                div()
                                    .flex()
                                    .child(secondary_button("welcome-browser-cancel", "Cancel", "", t).on_click(
                                        cx.listener(|this, _, _, cx| {
                                            this.account.update(cx, |account, cx| account.cancel_browser(cx))
                                        }),
                                    ))
                                    .into_any_element(),
                            );
                        }
                        ServerOffer::SigningIn => {
                            body.push(waiting_line("Signing in…"));
                            primary = ("Signing in…".into(), false);
                        }
                        ServerOffer::SignedIn => {}
                    }
                    if let Some(error) = &error {
                        body.push(
                            status(
                                icon("warning", 16., t.warning_ink).into_any_element(),
                                error.clone(),
                                t.warning_ink,
                            )
                            .into_any_element(),
                        );
                    }
                    if !crate::account::configured() && !flow.local {
                        body.push(
                            div()
                                .flex()
                                .ml(px(-12.))
                                .child(
                                    quiet_button("welcome-local", "Try it on this Mac only", t)
                                        .h(px(30.))
                                        .text_size(px(13.))
                                        .on_click(cx.listener(|this, _, _, cx| this.onboarding_choose_local(cx))),
                                )
                                .into_any_element(),
                        );
                    }
                }
            }
            Step::Audio => {
                title = "Let Cereal listen";
                help = "Cereal records your microphone, and the other side of the call from your Mac’s own sound.".into();
                for (channel, symbol, name) in [
                    (Channel::Microphone, "mic", "Microphone"),
                    (Channel::SystemAudio, "headphones", "System audio"),
                ] {
                    let state = permissions[channel.index()];
                    let waiting = asking[channel.index()];
                    let (label, tone) = match state {
                        _ if waiting => ("Waiting for macOS…", t.muted),
                        Permission::Allowed => (state.label(), t.accent),
                        Permission::Denied => (state.label(), t.danger),
                        Permission::NotAsked => (state.label(), t.muted),
                    };
                    let row = choice(symbol, name, label, tone, state == Permission::Allowed, t);
                    let row = match state {
                        _ if waiting => row.child(motion::spinner(t.muted)),
                        Permission::Allowed => row.child(motion::swap_in(
                            ("welcome-allowed", channel.index()),
                            icon("check", 18., t.accent).mr(px(6.)),
                        )),
                        Permission::NotAsked => row.child(
                            secondary_button(
                                if channel == Channel::Microphone {
                                    "welcome-allow-microphone"
                                } else {
                                    "welcome-allow-system"
                                },
                                "Allow",
                                "",
                                t,
                            )
                            .h(px(32.))
                            .on_click(cx.listener(move |this, _, _, cx| this.onboarding_allow(channel, cx))),
                        ),
                        Permission::Denied => row.child(
                            secondary_button(
                                if channel == Channel::Microphone {
                                    "welcome-settings-microphone"
                                } else {
                                    "welcome-settings-system"
                                },
                                "Open System Settings",
                                "",
                                t,
                            )
                            .h(px(32.))
                            .px(px(if l.compact { 10. } else { 12. }))
                            .text_size(px(if l.compact { 12. } else { 13. }))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.onboarding_open_settings(channel, cx)
                            })),
                        ),
                    };
                    body.push(row.into_any_element());
                }
                let note = match (permissions[0], permissions[1]) {
                    (Permission::Allowed, Permission::Allowed) => None,
                    (Permission::Allowed, _) => Some("Without system audio, only your side of the call is recorded."),
                    (Permission::Denied, _) => Some("Without the microphone Cereal can’t record. Allow it in System Settings, then come back here."),
                    _ => Some("Without the microphone Cereal can’t record. You can allow it later, when you start a note."),
                };
                if let Some(note) = note {
                    body.push(small(note, t).mt(px(2.)).into_any_element());
                }
            }
            Step::Calendar => {
                title = "Bring your calendar";
                help = "Connect Google Calendar and Cereal shows what’s coming up, ready to record.".into();
                if facts.calendar {
                    body.push(good_line("Your calendar is connected".into()));
                    let names: Vec<&str> = self
                        .calendars
                        .sources
                        .iter()
                        .map(|source| source.name.trim())
                        .filter(|name| !name.is_empty())
                        .take(3)
                        .collect();
                    if !names.is_empty() {
                        body.push(small(names.join(", "), t).ml(px(24.)).into_any_element());
                    }
                } else {
                    body.push(
                        div()
                            .flex()
                            .child(
                                secondary_button(
                                    "welcome-connect-calendar",
                                    if self.onboarding.calendar_asked {
                                        "Open the page again"
                                    } else {
                                        "Connect Google Calendar"
                                    },
                                    "calendar",
                                    t,
                                )
                                .on_click(cx.listener(|this, _, _, cx| this.onboarding_connect_calendar(cx))),
                            )
                            .into_any_element(),
                    );
                    if self.onboarding.calendar_asked {
                        body.push(waiting_line("Finish in your browser. Cereal notices when a calendar appears."));
                    } else {
                        body.push(small("It opens your server’s account page in your browser.", t).into_any_element());
                    }
                }
            }
            Step::Pill => {
                title = "Meet the pill";
                help = "While you record, a small pill floats above your other apps and keeps the transcript within reach.".into();
                let keys = |keys: &[&'static str], text: &'static str| {
                    div()
                        .w_full()
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .child(
                            div().w(px(56.)).flex_shrink_0().flex().child(
                                div().flex().gap(px(3.)).children(keys.iter().map(|key| {
                                    div()
                                        .min_w(px(24.))
                                        .h(px(24.))
                                        .px(px(6.))
                                        .rounded(px(6.))
                                        .bg(t.fill)
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .font_family("Inter")
                                        .text_size(px(13.))
                                        .line_height(px(16.))
                                        .text_color(t.secondary)
                                        .child(*key)
                                })),
                            ),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(px(14.))
                                .line_height(px(20.))
                                .text_color(t.text)
                                .child(text),
                        )
                };
                let tile = |symbol: &'static str, text: &'static str| {
                    div()
                        .w_full()
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .child(
                            div().w(px(56.)).flex_shrink_0().flex().child(
                                div()
                                    .size(px(24.))
                                    .rounded(px(6.))
                                    .bg(t.fill)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon(symbol, 13., t.secondary)),
                            ),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(px(14.))
                                .line_height(px(20.))
                                .text_color(t.text)
                                .child(text),
                        )
                };
                body.push(keys(&["⌘", "N"], "Starts a new note and begins recording").into_any_element());
                body.push(tile("cereal", "Click the pill to come back to your note").into_any_element());
                body.push(tile("stop", "Press Stop in the note when you’re done").into_any_element());
                body.push(
                    div()
                        .flex()
                        .mt(px(6.))
                        .child(
                            secondary_button("welcome-try-pill", "Try it", "bolt", t)
                                .on_click(cx.listener(|this, _, _, cx| this.onboarding_try_pill(cx))),
                        )
                        .into_any_element(),
                );
            }
            Step::Done => {
                title = "You’re set";
                help = "Press ⌘N when a meeting starts. Cereal takes it from there.".into();
                primary = ("Open Cereal".into(), true);
                let summary = |done: bool, text: String| {
                    status(
                        icon(if done { "check" } else { "close" }, 16., if done { t.accent } else { t.muted })
                            .into_any_element(),
                        text,
                        if done { t.text } else { t.muted },
                    )
                    .into_any_element()
                };
                body.push(match &session {
                    Some(session) => summary(true, format!("Signed in to {instance} as {}", session.user.email)),
                    None => summary(true, "Notes are kept on this Mac only".into()),
                });
                body.push(match (permissions[0], permissions[1]) {
                    (Permission::Allowed, Permission::Allowed) => summary(true, "Microphone and system audio are allowed".into()),
                    (Permission::Allowed, _) => summary(true, "Microphone is allowed; system audio is not yet".into()),
                    _ => summary(false, "Microphone is not allowed yet".into()),
                });
                if !flow.local {
                    body.push(if facts.calendar {
                        summary(true, "Your calendar is connected".into())
                    } else {
                        summary(false, "No calendar yet. Settings has it when you want it.".into())
                    });
                }
            }
        }

        // ---- the left column -------------------------------------------------
        let compact = l.compact;
        let arriving = |delay: f32| {
            motion::mount_pose(
                SETTLE,
                ((since - delay) / (SETTLE.millis as f32 / 1000.)).clamp(0., 1.),
                reduced,
            )
        };
        let direction = self.onboarding.direction;
        let slide = move |element: Div, pose: motion::Pose| -> AnyElement {
            if pose.at_rest() {
                return element.into_any_element();
            }
            motion::Morph::new(element)
                .opacity(pose.fade)
                .offset(direction * 28. * (1. - pose.value), 0.)
                .into_any_element()
        };
        let eyebrow = match step {
            Step::Welcome => "WELCOME",
            Step::Server | Step::Audio => "SETUP",
            Step::Calendar => "OPTIONAL",
            Step::Pill => "HOW IT WORKS",
            Step::Done => "READY",
        };
        let heading = div()
            .w_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .mb(px(if compact { 6. } else { 8. }))
                    .text_size(px(11.))
                    .line_height(px(16.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(t.muted)
                    .child(eyebrow),
            )
            .child(
                div()
                    .font_family(DISPLAY)
                    .text_size(px(if compact { 28. } else { 34. }))
                    .line_height(px(if compact { 34. } else { 40. }))
                    .text_color(t.text)
                    .child(title),
            )
            .child(
                div()
                    .mt(px(if compact { 6. } else { 10. }))
                    .text_size(px(if compact { 14. } else { 16. }))
                    .line_height(px(if compact { 20. } else { 24. }))
                    .text_color(t.muted)
                    .child(help),
            );
        let fields = div()
            .w_full()
            .mt(px(if compact { 18. } else { 26. }))
            .flex()
            .flex_col()
            .gap(px(if compact { 8. } else { 10. }))
            .children(body);
        let can_go_back = flow.previous().is_some() || self.onboarding.replay;
        let (label, enabled) = primary;
        let nav = div()
            .w_full()
            .h(px(40.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(6.))
            .when(can_go_back, |nav| {
                nav.child(
                    div().ml(px(-12.)).child(
                        quiet_button_with("welcome-back", "Back", "chevron-left", t)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.onboarding_back(cx);
                            })),
                    ),
                )
            })
            .child(div().flex_1())
            .when(flow.can_skip(facts), |nav| {
                nav.child(
                    quiet_button("welcome-skip", "Skip for now", t)
                        .on_click(cx.listener(|this, _, _, cx| this.onboarding_skip(cx))),
                )
            })
            .child(
                primary_button("welcome-continue", label, enabled, t).when(enabled, |button| {
                    button.on_click(cx.listener(|this, _, _, cx| this.onboarding_continue(cx)))
                }),
            );
        let left = div()
            .relative()
            .w(px(l.left))
            .h_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .child(place("welcome-left"))
            // The title and the fields start at the same height on every
            // step and in every state, so nothing jumps as a step fills in;
            // the buttons keep the foot of the column.
            .child(div().h(px(if compact { 34. } else { 118. })).flex_shrink_0())
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .child(slide(heading, arriving(0.)))
                    .child(slide(fields, arriving(0.06))),
            )
            .child(div().flex_1().min_h(px(8.)))
            .child(nav);

        // ---- the little screen ------------------------------------------------
        let mini = Mini {
            k: l.k,
            t,
            dark: self.workspace.settings.dark_mode,
        };
        let events = (step == Step::Calendar && facts.calendar).then(|| {
            self.calendars
                .events
                .iter()
                .take(4)
                .map(|event| TinyEvent {
                    time: if event.all_day {
                        "All day".into()
                    } else {
                        chrono::DateTime::parse_from_rfc3339(&event.start_at)
                            .map(|start| {
                                start
                                    .with_timezone(&chrono::Local)
                                    .format("%-I:%M %p")
                                    .to_string()
                            })
                            .unwrap_or_default()
                    },
                    title: event.title.clone(),
                    calendar: event.calendar_name.clone(),
                    color: hex_color(&event.color, t.accent),
                })
                .collect::<Vec<_>>()
        });
        let seconds = |since: Option<Instant>| {
            since.map_or(60., |since| {
                now.saturating_duration_since(since).as_secs_f32()
                    / motion::slow(Duration::from_secs(1)).as_secs_f32()
            })
        };
        let trying = self.onboarding.trying.map(|at| seconds(Some(at)));
        let scene = Scene {
            mini,
            stage: &self.stage,
            since,
            reduced,
            link,
            link_since: seconds(Some(self.onboarding.link_changed)),
            instance: instance.clone(),
            host: host.clone(),
            person: session.as_ref().map(|session| {
                let name = session.user.name.trim();
                if name.is_empty() {
                    session.user.email.clone()
                } else {
                    name.to_owned()
                }
            }),
            permissions,
            asking,
            levels,
            events,
            calendar_name: self
                .calendars
                .sources
                .first()
                .map(|source| source.name.clone())
                .unwrap_or_else(|| "Google Calendar".into()),
            events_since: seconds(self.onboarding.calendar_found),
            trying,
        };
        let (screen_w, screen_h) = scenes::screen_size(step);
        // The little screen changes shape from step to step on the same
        // spring the app's menus open on, and its content settles in after.
        let width = self.stage.aim(("welcome-screen-w", 0), screen_w, GOOEY) * l.k;
        let height = self.stage.aim(("welcome-screen-h", 0), screen_h, GOOEY) * l.k;
        let settle = motion::mount_pose(SNAPPY, ((since - 0.08) / 0.3).clamp(0., 1.), reduced);
        let perch = (30. * l.k).round();
        let content = scenes::scene(step, &scene);
        let caption: SharedString = match step {
            Step::Welcome => "Your notes, written while you talk".into(),
            Step::Server => match (&offer, link) {
                (ServerOffer::SignedIn, _) => format!("Connected to {instance}").into(),
                (_, Link::Joined) => format!("Found {instance}").into(),
                (_, Link::Reaching) => "Looking for your server".into(),
                (_, Link::Broken) => "That server didn’t answer".into(),
                (_, Link::Apart) => "Waiting for an address".into(),
            },
            Step::Audio => match (permissions[0], permissions[1]) {
                (Permission::Allowed, Permission::Allowed) => "Hearing both sides of the call".into(),
                (Permission::Allowed, _) => "Hearing your microphone".into(),
                (_, Permission::Allowed) => "Hearing your Mac’s sound".into(),
                _ => "Nothing is heard until you allow it".into(),
            },
            Step::Calendar => match &scene.events {
                Some(events) if !events.is_empty() => "What’s coming up, from your calendar".into(),
                Some(_) => "Connected. Nothing coming up yet".into(),
                None => "Your meetings will appear here".into(),
            },
            Step::Pill => match trying {
                Some(at) if at < scenes::TRY_LENGTH => "A preview: nothing is being recorded".into(),
                _ => "The pill, beside whatever you’re in".into(),
            },
            Step::Done => "Ready when you are".into(),
        };
        let caption_pose = {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            caption.hash(&mut hasher);
            self.stage.swap(("welcome-caption", 0), hasher.finish())
        };
        let shadow = if mini.dark { 0.42 } else { 0.1 };
        let right = div()
            .relative()
            .w(px(l.right))
            .h_full()
            .flex_shrink_0()
            .rounded(px(if compact { 20. } else { 24. }))
            .bg(t.sidebar)
            .border_1()
            .border_color(t.border)
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .child(place("welcome-right"))
            .child(
                // The little screen keeps the room of the largest one, so
                // nothing around it moves as it changes shape.
                div()
                    .relative()
                    .w(px(scenes::SCREEN_BOX.0 * l.k))
                    .h(px(scenes::SCREEN_BOX.1 * l.k))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .relative()
                            .w(px(width))
                            .h(px(height))
                            .flex_shrink_0()
                            .rounded(px(14. * l.k))
                            .bg(t.card)
                            .border_1()
                            .border_color(t.border)
                            .shadow(vec![
                                BoxShadow {
                                    color: hsla(0., 0., 0., shadow).into(),
                                    offset: point(px(0.), px(10. * l.k)),
                                    blur_radius: px(28. * l.k),
                                    spread_radius: px(-8. * l.k),
                                },
                                BoxShadow {
                                    color: hsla(0., 0., 0., shadow * 0.6).into(),
                                    offset: point(px(0.), px(1.)),
                                    blur_radius: px(2.),
                                    spread_radius: px(0.),
                                },
                            ])
                            .overflow_hidden()
                            .child(place("welcome-screen"))
                            .child(
                                div()
                                    .absolute()
                                    .top(px(4. * (1. - settle.value)))
                                    .left_0()
                                    .w(px(screen_w * l.k))
                                    .h(px(screen_h * l.k))
                                    .opacity(settle.fade)
                                    .child(content),
                            ),
                    )
                    .child(
                        // The mark, perched on the little screen's top edge.
                        // It rides the edge as the screen changes shape, and
                        // it is where it hops and wobbles.
                        div()
                            .absolute()
                            .left(px((scenes::SCREEN_BOX.0 * l.k + width) / 2.
                                - perch
                                - 30. * l.k))
                            .top(px((scenes::SCREEN_BOX.1 * l.k - height) / 2.
                                - perch * (22.55 / 24.)
                                + 2.))
                            .child(scenes::mark(perch, pose, t, t.sidebar)),
                    ),
            )
            .child(
                div()
                    .mt(px(if compact { 14. } else { 20. }))
                    .h(px(20.))
                    .max_w_full()
                    .px(px(16.))
                    .flex_shrink_0()
                    .child(motion::swapped(
                        div()
                            .text_size(px(13.))
                            .line_height(px(20.))
                            .text_color(t.muted)
                            .whitespace_nowrap()
                            .child(caption),
                        caption_pose,
                    )),
            );

        // ---- what moves next ------------------------------------------------------
        let try_running = trying.is_some_and(|at| at < scenes::TRY_LENGTH + 0.5);
        let frames = character_moving
            || self.onboarding.travel.moving(now)
            || since < 1.
            || (step == Step::Welcome && since < 3.2)
            || try_running
            || (step == Step::Calendar && scene.events_since < 1.2);
        if frames && !reduced {
            self.stage.keep_alive();
        } else if frames {
            // Reduced motion: only the short fades need frames.
            if since < 0.3 || character_moving {
                self.stage.keep_alive();
            }
        }
        let listening = step == Step::Audio
            && (permissions.contains(&Permission::Allowed) || asking.contains(&true));
        let searching = step == Step::Server && link == Link::Reaching && !reduced;
        let mut wake = if listening || searching {
            Wake::Tick
        } else {
            Wake::Never
        };
        let mut due = |at: Instant| match wake {
            Wake::Tick => {}
            Wake::At(earlier) if earlier <= at => {}
            _ => wake = Wake::At(at),
        };
        if let Some(blink) = next_blink {
            due(blink);
        }
        if step == Step::Audio {
            due(now + Duration::from_millis(500));
        }
        if step == Step::Calendar && !facts.calendar {
            due(now + Duration::from_millis(1000));
        }
        self.onboarding.wake = wake;
        self.onboarding_clock(cx);

        div()
            .id("welcome")
            .key_context("CerealWelcome")
            .track_focus(&self.onboarding.focus)
            .on_action(cx.listener(|this, _: &Continue, window, cx| {
                // A focused button is pressed by Return itself.
                if this.onboarding.focus.is_focused(window) {
                    this.onboarding_continue(cx);
                } else {
                    cx.propagate();
                }
            }))
            .size_full()
            .relative()
            .bg(t.bg)
            .text_color(t.text)
            .font_family(FONT)
            .text_size(px(14.))
            .line_height(px(20.))
            .child(
                div()
                    .absolute()
                    .left(px(l.x))
                    .top(px(l.y))
                    .w(px(l.width))
                    .h(px(l.height))
                    .flex()
                    .gap(px(l.gap))
                    .child(left)
                    .child(right),
            )
            .child(
                // The steps, centred at the top of the window, level with
                // its traffic lights.
                div()
                    .absolute()
                    .top(px(15.))
                    .left(px(((f32::from(window.viewport_size().width) - scenes::INDICATOR.0)
                        / 2.)
                        .round()))
                    .child(place("welcome-dots"))
                    .child(dots),
            )
            .into_any_element()
    }
}

fn browser_button(label: &'static str, t: Theme, cx: &mut Context<Cereal>) -> AnyElement {
    let google = label == "Continue with Google";
    div()
        .flex()
        .child(
            secondary_button("welcome-browser", label, if google { "" } else { "globe" }, t)
                .when(google, |button| {
                    button
                        .flex_row_reverse()
                        .child(img("icons/google.svg").size(px(16.)).flex_shrink_0())
                })
                .on_click(cx.listener(|this, _, _, cx| {
                    this.account
                        .update(cx, |account, cx| account.continue_in_browser(cx))
                })),
        )
        .into_any_element()
}

/// Open a pane of System Settings. A debug build given `CEREAL_BROWSER_LOG`
/// writes the address to that file instead, as `open_in_browser` does.
fn open_system_settings(url: &str, cx: &mut App) {
    #[cfg(debug_assertions)]
    if let Some(path) = crate::environment::var_os("CEREAL_BROWSER_LOG").filter(|p| !p.is_empty()) {
        use std::io::Write;
        if let Ok(mut log) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(log, "{url}");
        }
        return;
    }
    cx.open_url(url);
}

#[cfg(test)]
mod tests {
    use super::*;
    // `gpui::*` re-exports its own `test` attribute; these are plain unit tests.
    use std::prelude::v1::test;

    fn window(width: f32, height: f32) -> Size<Pixels> {
        size(px(width), px(height))
    }

    #[test]
    fn the_two_columns_fit_the_smallest_window_with_room_to_spare() {
        // The window's minimum size.
        let l = layout(window(760., 580.));
        assert!(l.compact);
        assert_eq!(l.left + l.gap + l.right, l.width);
        assert_eq!((l.x, l.width), (24., 712.));
        // Clear of the traffic lights, and inside the window.
        assert!(l.y >= TOP && l.y + l.height <= 580. - 24.);
        // The largest little screen and its caption fit the right column
        // with at least 24 px around them.
        assert!(l.k >= 1.);
        assert!(scenes::SCREEN_BOX.0 * l.k <= l.right - 48. + 0.01);
        assert!(scenes::SCREEN_BOX.1 * l.k + CAPTION <= l.height - 48. + 0.01);
        // The left column is wide enough for its widest row of buttons.
        assert!(l.left >= 316.);
    }

    #[test]
    fn a_large_window_gets_a_centred_stage_that_stops_growing() {
        let l = layout(window(1200., 840.));
        assert!(!l.compact);
        assert_eq!((l.width, l.height), (1040., 600.));
        assert_eq!((l.x, l.y), (80., 120.));
        assert_eq!((l.left, l.gap, l.right), (392., 56., 592.));
        assert_eq!(l.k, 1.5);
        // Larger still: the same stage, still centred.
        let huge = layout(window(2400., 1400.));
        assert_eq!((huge.width, huge.height, huge.k), (1040., 600., 1.5));
        assert_eq!(huge.x, 680.);
        // Between the two the little screen scales smoothly with the room.
        let mut previous = 0.;
        for width in (760..=1300).step_by(20) {
            let l = layout(window(width as f32, 700.));
            assert!(l.k >= 1. && l.k <= 1.5);
            assert!(scenes::SCREEN_BOX.0 * l.k <= l.right - 48. + 0.01, "{width}");
            if !l.compact && previous > 0. {
                assert!(l.k + 0.001 >= previous);
            }
            previous = if l.compact { 0. } else { l.k };
        }
    }

    #[test]
    fn calendar_colours_are_read_or_fall_back() {
        let fallback: Hsla = rgb(0x835e00).into();
        assert_eq!(hex_color("#87e5e8", fallback), Hsla::from(rgb(0x87e5e8)));
        assert_eq!(hex_color("87e5e8", fallback), Hsla::from(rgb(0x87e5e8)));
        for bad in ["", "#fff", "blue", "#12345g"] {
            assert_eq!(hex_color(bad, fallback), fallback);
        }
    }
}
