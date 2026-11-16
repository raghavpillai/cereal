//! Local-only interaction preview of the installed Cereal sign-in screens.
//! This view never opens OAuth, collects credentials, or creates an account.
use crate::{
    input::{InputChanged, InputSubmitted, TextInput},
    ui::*,
};
use gpui::{prelude::*, *};
use std::{rc::Rc, time::Duration};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoginStage {
    Welcome,
    Waiting { provider: String },
    SsoEmail,
    VerifyEmail { email: String },
    Success,
    Failure { message: String },
}

#[derive(Clone, Debug)]
pub struct LoginCompleted;

pub struct LoginView {
    pub stage: LoginStage,
    pub email: Entity<TextInput>,
    pub code: Entity<TextInput>,
    pub dark_mode: bool,
    focus: FocusHandle,
    generation: u64,
    last_used: Option<String>,
    inline_error: Option<String>,
    focus_pending: bool,
    _subscriptions: Vec<Subscription>,
}
impl EventEmitter<LoginCompleted> for LoginView {}

impl LoginView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let email = cx.new(|cx| TextInput::new("name@company.com", false, cx));
        let code = cx.new(|cx| TextInput::new("Enter code", false, cx));
        let subscriptions = vec![
            cx.subscribe(&email, |this, _, _: &InputChanged, cx| {
                this.inline_error = None;
                cx.notify();
            }),
            cx.subscribe(&email, |this, _, _: &InputSubmitted, cx| {
                if this.stage == LoginStage::SsoEmail
                    && !this.email.read(cx).content().trim().is_empty()
                {
                    this.submit_email(cx);
                }
            }),
            cx.subscribe(&code, |_, _, _: &InputChanged, cx| cx.notify()),
            cx.subscribe(&code, |this, _, _: &InputSubmitted, cx| {
                if matches!(this.stage, LoginStage::VerifyEmail { .. }) {
                    this.verify_preview(cx);
                }
            }),
        ];
        Self {
            stage: LoginStage::Welcome,
            email,
            code,
            dark_mode: false,
            focus: cx.focus_handle(),
            generation: 0,
            last_used: None,
            inline_error: None,
            focus_pending: true,
            _subscriptions: subscriptions,
        }
    }

    pub fn reset(&mut self, cx: &mut Context<Self>) {
        self.email.update(cx, |input, cx| input.set_content("", cx));
        self.code.update(cx, |input, cx| input.set_content("", cx));
        self.change_stage(LoginStage::Welcome, cx);
    }

    fn change_stage(&mut self, stage: LoginStage, cx: &mut Context<Self>) {
        self.stage = stage;
        self.generation = self.generation.wrapping_add(1);
        self.inline_error = None;
        self.focus_pending = true;
        cx.notify();
    }

    pub fn start_provider(&mut self, provider: &str, cx: &mut Context<Self>) {
        self.last_used = Some(provider.to_string());
        self.change_stage(
            LoginStage::Waiting {
                provider: provider.to_string(),
            },
            cx,
        );
    }

    pub fn reveal_sso(&mut self, cx: &mut Context<Self>) {
        self.change_stage(LoginStage::SsoEmail, cx);
        self.focus_pending = false;
        let generation = self.generation;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(150))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.generation == generation && this.stage == LoginStage::SsoEmail {
                    this.focus_pending = true;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub fn submit_email(&mut self, cx: &mut Context<Self>) {
        let email = self.email.read(cx).content().trim().to_lowercase();
        if !valid_preview_email(&email) {
            self.inline_error = Some("Please enter a valid email address".into());
            cx.notify();
            return;
        }
        self.last_used = Some("SSO".into());
        self.change_stage(LoginStage::VerifyEmail { email }, cx);
        self.focus_pending = true;
    }

    pub fn verify_preview(&mut self, cx: &mut Context<Self>) {
        if self.code.read(cx).content().trim().is_empty() {
            return;
        }
        self.complete_preview(cx);
    }

    pub fn complete_preview(&mut self, cx: &mut Context<Self>) {
        self.code.update(cx, |input, cx| input.set_content("", cx));
        self.change_stage(LoginStage::Success, cx);
        let generation = self.generation;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(650))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.generation == generation && this.stage == LoginStage::Success {
                    cx.emit(LoginCompleted);
                }
            });
        })
        .detach();
    }

    pub fn preview_failure(&mut self, cx: &mut Context<Self>) {
        self.change_stage(
            LoginStage::Failure {
                message: "We couldn’t finish signing you in. Please try again.".into(),
            },
            cx,
        );
    }

    fn action_button(
        &self,
        id: &'static str,
        label: &'static str,
        primary: bool,
        enabled: bool,
        cx: &mut Context<Self>,
        activate: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> Stateful<Div> {
        let t = Theme::new(self.dark_mode);
        let activate = Rc::new(activate);
        let click = activate.clone();
        div()
            .id(id)
            .focusable()
            .tab_stop(true)
            .w_full()
            .h(px(40.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .gap(px(9.))
            .px(px(20.))
            .rounded_full()
            .border_1()
            .border_color(t.border)
            .text_size(px(14.))
            .line_height(px(20.))
            .when(primary, |s| s.bg(t.text).text_color(t.bg))
            .when(!primary, |s| s.text_color(t.text))
            .when(!enabled, |s| s.opacity(DISABLED))
            .when(enabled, |s| {
                let s = s.cursor(CursorStyle::Arrow);
                // Eased like every other control: the secondary buttons take
                // the neutral wash, the primary one dims a little.
                let s = if primary {
                    crate::motion::dims(s, crate::motion::reveal_key(id), 0.12)
                } else {
                    s.child(crate::motion::tactile(t.hover).round())
                };
                s.active(move |s| {
                    if primary {
                        s.opacity(0.72)
                    } else {
                        s.bg(t.border)
                    }
                })
            })
            .focus(move |s| s.border_color(t.accent).border_2())
            .when(label == "Sign in with Google", |s| s.child(google_mark()))
            .when(label == "Sign in with Microsoft", |s| {
                s.child(microsoft_mark())
            })
            .child(label)
            .on_click(cx.listener(move |this, _, window, cx| {
                if enabled {
                    click(this, window, cx);
                }
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                if enabled && matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    activate(this, window, cx);
                    cx.stop_propagation();
                }
            }))
    }

    fn provider_button(&self, provider: &'static str, cx: &mut Context<Self>) -> AnyElement {
        let t = Theme::new(self.dark_mode);
        let label = if provider == "Google" {
            "Sign in with Google"
        } else {
            "Sign in with Microsoft"
        };
        let mut result = div()
            .relative()
            .w_full()
            .mb(px(12.))
            .child(self.action_button(
                provider,
                label,
                false,
                true,
                cx,
                move |this, window, cx| {
                    this.focus.focus(window);
                    this.start_provider(provider, cx);
                },
            ));
        if self.last_used.as_deref() == Some(provider) {
            result = result.child(
                div()
                    .absolute()
                    .top(px(-8.))
                    .right(px(-8.))
                    .rounded_full()
                    .px(px(9.))
                    .py(px(4.))
                    .bg(if self.dark_mode {
                        rgb(0xbb8e0b)
                    } else {
                        rgb(0xf8e6c0)
                    })
                    .text_color(t.text)
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .child("Last used")
                    .with_animations(
                        ("login-last-used", self.generation),
                        vec![
                            Animation::new(Duration::from_millis(400)),
                            Animation::new(Duration::from_millis(500)),
                        ],
                        |d, phase, delta| d.opacity(if phase == 0 { 0. } else { delta }),
                    ),
            );
        }
        result.into_any_element()
    }

    fn welcome(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = Theme::new(self.dark_mode);
        let sso = self.stage == LoginStage::SsoEmail;
        let mut controls = div()
            .w_full()
            .flex()
            .flex_col()
            .child(self.provider_button("Google", cx))
            .child(self.provider_button("Microsoft", cx));
        if sso {
            controls = controls.child(
                div()
                    .w_full()
                    .overflow_hidden()
                    .child(
                        div()
                            .mt(px(12.))
                            .mb(px(12.))
                            .w_full()
                            .h(px(40.))
                            .rounded_full()
                            .border_1()
                            .border_color(if self.inline_error.is_some() {
                                rgb(0xb33d34).into()
                            } else {
                                t.border
                            })
                            .px(px(16.))
                            .flex()
                            .items_center()
                            .child(self.email.clone()),
                    )
                    .with_animation(
                        ("login-sso-expand", self.generation),
                        Animation::new(crate::motion::GOOEY.duration()),
                        |d, delta| {
                            let (height, opacity) =
                                sso_expansion(delta, crate::motion::reduce_motion());
                            d.h(px(height)).opacity(opacity)
                        },
                    ),
            );
        }
        if let Some(error) = &self.inline_error {
            controls = controls.child(
                div()
                    .w_full()
                    .text_center()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(rgb(0xb33d34))
                    .mb(px(8.))
                    .child(error.clone()),
            );
        }
        let sso_button = self.action_button(
            "sso-sign-in",
            "Sign in with SSO",
            false,
            true,
            cx,
            |this, window, cx| {
                if this.stage == LoginStage::SsoEmail {
                    this.submit_email(cx);
                } else {
                    this.focus.focus(window);
                    this.reveal_sso(cx);
                }
            },
        );
        controls = controls.child(div().relative().w_full().child(sso_button).when(
            self.last_used.as_deref() == Some("SSO"),
            |s| {
                s.child(
                    div()
                        .absolute()
                        .top(px(-8.))
                        .right(px(-8.))
                        .rounded_full()
                        .px(px(9.))
                        .py(px(4.))
                        .bg(if self.dark_mode {
                            rgb(0xbb8e0b)
                        } else {
                            rgb(0xf8e6c0)
                        })
                        .text_color(t.text)
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .child("Last used"),
                )
            },
        ));
        let width = px(448.);
        div()
            .w(width)
            .max_w_full()
            .flex()
            .flex_col()
            .items_center()
            .child(icon("cereal", 56., t.text))
            .child(
                div()
                    .mt(px(24.))
                    .font_family(DISPLAY)
                    .text_size(px(36.))
                    .line_height(px(40.))
                    .text_center()
                    .child("Welcome to Cereal"),
            )
            .child(
                div()
                    .mt(px(24.))
                    .w_full()
                    .flex()
                    .flex_wrap()
                    .justify_center()
                    .gap(px(4.))
                    .text_size(px(18.))
                    .line_height(px(26.))
                    .text_color(t.muted)
                    .child("Choose your")
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("work account"),
                    )
                    .child("to get started."),
            )
            .child(div().mt(px(40.)).w_full().child(controls))
            .into_any_element()
    }

    fn waiting(&self, provider: &str, cx: &mut Context<Self>) -> AnyElement {
        let t = Theme::new(self.dark_mode);
        div()
            .w(px(448.))
            .max_w_full()
            .flex()
            .flex_col()
            .items_start()
            .gap(px(16.))
            .child(icon("cereal", 48., t.text))
            .child(
                div()
                    .font_family(DISPLAY)
                    .text_size(px(28.))
                    .line_height(px(32.))
                    .child("Sign in to your work calendar..."),
            )
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .text_size(px(18.))
                    .line_height(px(26.))
                    .text_color(t.muted)
                    .child("Cereal needs to see your upcoming meetings to work."),
            )
            .child(
                div()
                    .mt(px(8.))
                    .w_full()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .text_color(t.muted)
                    .child(icon("sun", 18., t.muted).with_animation(
                        ("login-wait-spinner", self.generation),
                        Animation::new(Duration::from_secs(2)).repeat(),
                        |icon, delta| {
                            icon.with_transformation(Transformation::rotate(percentage(delta)))
                        },
                    ))
                    .child(format!("Waiting for {provider}")),
            )
            .child(div().mt(px(16.)).w_full().child(self.action_button(
                "login-complete-preview",
                "Return to Cereal",
                true,
                true,
                cx,
                |this, window, cx| {
                    this.focus.focus(window);
                    this.complete_preview(cx);
                },
            )))
            .child(
                div()
                    .text_size(px(12.))
                    .line_height(px(18.))
                    .text_color(t.muted)
                    .child("No browser or account is opened in this preview."),
            )
            .child(
                div()
                    .id("login-preview-error")
                    .focusable()
            .tab_stop(true)
                    .text_size(px(12.))
                    .underline()
                    .cursor_pointer()
                    .map(|link| {
                        crate::motion::warms(
                            link,
                            crate::motion::reveal_key("login-preview-error"),
                            t.muted,
                            t.text,
                        )
                    })
                    .child("Preview a sign-in error")
                    .focus(move |s| s.text_color(t.text))
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            this.focus.focus(window);
                            this.preview_failure(cx);
                            cx.stop_propagation();
                        }
                    }))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.focus.focus(window);
                        this.preview_failure(cx);
                    })),
            )
            .into_any_element()
    }

    fn verification(&self, email: &str, cx: &mut Context<Self>) -> AnyElement {
        let t = Theme::new(self.dark_mode);
        let enabled = !self.code.read(cx).content().trim().is_empty();
        div()
            .w(px(448.))
            .max_w_full()
            .flex()
            .flex_col()
            .items_start()
            .gap(px(16.))
            .child(icon("cereal", 48., t.text))
            .child(
                div()
                    .font_family(DISPLAY)
                    .text_size(px(30.))
                    .line_height(px(36.))
                    .child("Check your email"),
            )
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .text_size(px(18.))
                    .line_height(px(26.))
                    .text_color(t.muted)
                    .child(format!("We sent a code to {email}.")),
            )
            .child(
                div()
                    .w_full()
                    .mt(px(8.))
                    .p(px(12.))
                    .rounded(px(10.))
                    .border_1()
                    .border_color(t.border)
                    .bg(t.card)
                    .child(self.code.clone()),
            )
            .child(self.action_button(
                "login-verify",
                "Verify",
                true,
                enabled,
                cx,
                |this, window, cx| {
                    this.focus.focus(window);
                    this.verify_preview(cx);
                },
            ))
            .child(small(
                "Preview only: no email was sent. Type any demo text, not a real code.",
                t,
            ))
            .into_any_element()
    }
}

impl Render for LoginView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = Theme::new(self.dark_mode);
        if self.focus_pending {
            self.focus_pending = false;
            match self.stage {
                LoginStage::SsoEmail => self.email.read(cx).focus_handle.focus(window),
                LoginStage::VerifyEmail { .. } => self.code.read(cx).focus_handle.focus(window),
                _ => self.focus.focus(window),
            }
        }
        let body = match self.stage.clone() {
            LoginStage::Welcome | LoginStage::SsoEmail => self.welcome(cx),
            LoginStage::Waiting { provider } => self.waiting(&provider, cx),
            LoginStage::VerifyEmail { email } => self.verification(&email, cx),
            LoginStage::Success => div()
                .w(px(448.))
                .max_w_full()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(18.))
                .child(icon("check", 42., t.accent))
                .child(heading("You’re all set", t))
                .child(small("Opening your local workspace…", t))
                .into_any_element(),
            LoginStage::Failure { message } => div()
                .w(px(448.))
                .max_w_full()
                .flex()
                .flex_col()
                .gap(px(18.))
                .child(icon("cereal", 48., t.text))
                .child(heading("Let’s try that again", t))
                .child(
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .text_size(px(16.))
                        .line_height(px(24.))
                        .text_color(t.muted)
                        .child(message),
                )
                .child(self.action_button(
                    "login-retry",
                    "Try again",
                    true,
                    true,
                    cx,
                    |this, window, cx| {
                        this.focus.focus(window);
                        this.reset(cx);
                    },
                ))
                .into_any_element(),
        };
        div()
            .id("login-preview")
            .key_context("CerealLogin")
            .track_focus(&self.focus)
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .overflow_hidden()
            .p(px(32.))
            .bg(t.bg)
            .text_color(t.text)
            .font_family(FONT)
            .text_size(px(14.))
            .line_height(px(20.))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                match event.keystroke.key.as_str() {
                    "escape" => {
                        this.reset(cx);
                        this.focus.focus(window);
                        cx.stop_propagation();
                    }
                    "tab" => {
                        if event.keystroke.modifiers.shift {
                            window.focus_prev();
                        } else {
                            window.focus_next();
                        }
                        cx.stop_propagation();
                    }
                    _ => {}
                }
            }))
            .when(!matches!(self.stage, LoginStage::Welcome), |s| {
                s.child(
                    div()
                        .absolute()
                        .top(px(56.))
                        .left(px(24.))
                        .w(px(90.))
                        .child(self.action_button(
                            "login-back",
                            "Back",
                            false,
                            true,
                            cx,
                            |this, window, cx| {
                                this.reset(cx);
                                this.focus.focus(window);
                            },
                        )),
                )
            })
            // Each sign-in stage settles in like any other page of the app.
            .child(crate::motion::page(
                (
                    "login-stage",
                    if matches!(self.stage, LoginStage::Welcome | LoginStage::SsoEmail) {
                        0
                    } else {
                        self.generation
                    },
                ),
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(body),
            ))
            .child(
                div()
                    .absolute()
                    .bottom(px(18.))
                    .left(px(20.))
                    .right(px(20.))
                    .text_center()
                    .text_size(px(11.))
                    .line_height(px(16.))
                    .text_color(t.muted)
                    .child("Sign-in preview"),
            )
    }
}

impl Focusable for LoginView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

fn google_mark() -> Img {
    // Img preserves the four brand colors; Svg would render an alpha mask.
    img("icons/google.svg").size(px(20.)).flex_shrink_0()
}
fn microsoft_mark() -> Div {
    let square = |color| div().size(px(8.)).bg(rgb(color));
    div()
        .size(px(18.))
        .flex_shrink_0()
        .flex()
        .flex_col()
        .gap(px(2.))
        .child(
            div()
                .flex()
                .gap(px(2.))
                .child(square(0xf25022))
                .child(square(0x7fba00)),
        )
        .child(
            div()
                .flex()
                .gap(px(2.))
                .child(square(0x00a4ef))
                .child(square(0xffb900)),
        )
}

fn valid_preview_email(email: &str) -> bool {
    let Some((local, domain)) = email.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && !domain.is_empty()
        && !domain.contains('@')
        && !email.chars().any(char::is_whitespace)
}

/// Height and opacity of the SSO email field `delta` of the way through its
/// expansion. It unfolds on the gooey spring, opening a touch too far before
/// settling on its 64 px; with reduced motion it only fades in at full height.
fn sso_expansion(delta: f32, reduced: bool) -> (f32, f32) {
    if reduced {
        return (64., crate::motion::fade_curve(delta * 3.5));
    }
    let level = crate::motion::GOOEY.at(delta);
    (64. * level, crate::motion::fade_for(level))
}

#[cfg(test)]
mod tests {
    use super::{sso_expansion, valid_preview_email};
    #[test]
    fn preview_email_rejects_empty_multiple_at_and_whitespace() {
        for invalid in [
            "",
            "missing-at",
            "@company.com",
            "name@",
            "a@@b.com",
            "a b@company.com",
        ] {
            assert!(!valid_preview_email(invalid));
        }
        assert!(valid_preview_email("maya@example.com"));
    }
    #[test]
    fn sso_expansion_springs_open_with_bounded_overshoot_and_exact_endpoints() {
        assert_eq!(sso_expansion(0., false), (0., 0.));
        assert_eq!(sso_expansion(1., false), (64., 1.));
        let heights: Vec<_> = (0..=200)
            .map(|i| sso_expansion(i as f32 / 200., false).0)
            .collect();
        let peak = heights.iter().copied().fold(0., f32::max);
        // It opens a few percent too far, once, and never collapses again.
        assert!(peak > 64. * 1.03 && peak < 64. * 1.08);
        let peak_at = heights.iter().position(|height| *height == peak).unwrap();
        assert!(heights[..=peak_at].windows(2).all(|w| w[0] <= w[1]));
        assert!(heights[peak_at..].iter().all(|height| *height > 63.));
        // The field is fully opaque well before it has finished settling.
        assert_eq!(sso_expansion(0.5, false).1, 1.);
    }
    #[test]
    fn reduced_motion_sso_expansion_only_fades_at_full_height() {
        let mut previous = 0.;
        for i in 0..=100 {
            let (height, opacity) = sso_expansion(i as f32 / 100., true);
            assert_eq!(height, 64.);
            assert!((0. ..=1.).contains(&opacity) && opacity >= previous);
            previous = opacity;
        }
        assert_eq!(sso_expansion(1., true), (64., 1.));
    }
}
