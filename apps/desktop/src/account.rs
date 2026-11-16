//! The sign-in form. Passwords are used once; the session that comes back
//! is handed to the accounts registry, which keeps it in the macOS Keychain
//! with the address and user it was issued for (`accounts`). Nothing here
//! holds a session or knows which account is which.
use crate::{
    accounts::{Secret, VerifiedAccount, client, validate_url},
    fetch::{Failure, Request},
    handoff::{BrowserSignIn, HandoffError, SignInMethods},
    input::{InputChanged, InputSubmitted, TextInput},
    link::AuthCallback,
    ui::*,
};
use gpui::{prelude::*, *};
use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::Duration,
};

/// Which sign-in is the latest. One that was overtaken (a sign-out, another
/// sign-in) is dropped when it comes back.
static ATTEMPT: AtomicU64 = AtomicU64::new(1);
fn attempt() -> u64 {
    ATTEMPT.load(Ordering::Acquire)
}
pub(crate) fn next_attempt() -> u64 {
    ATTEMPT.fetch_add(1, Ordering::AcqRel) + 1
}

/// Ask the server whose session `token` is: the end of every sign-in.
fn verify(url: String, token: Secret) -> Result<VerifiedAccount, String> {
    match client::session_check(&url, &token) {
        client::SessionCheck::Valid(session) => Ok(VerifiedAccount {
            url,
            token,
            session: *session,
        }),
        client::SessionCheck::Ended => Err("Email, password or session was not accepted".into()),
        client::SessionCheck::Unknown(message) => Err(message),
    }
}
fn sign_in(url: String, email: String, password: String) -> Result<VerifiedAccount, String> {
    let url = validate_url(&url)?;
    let token = client::password_sign_in(&url, &email, &password)?;
    verify(url, token)
}

pub enum AccountEvent {
    Back,
    SignedIn(VerifiedAccount),
    SignOut,
}

/// What the sign-in form knows about the server whose address is in the field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Discovery {
    /// No address yet, or one that is still being typed.
    Idle,
    Checking,
    Ready(SignInMethods),
    Failed(String),
}

/// How long the form waits after the last keystroke before asking a server
/// how it signs in, and how long a sign-in in the browser may take.
const DISCOVERY_PAUSE: Duration = Duration::from_millis(500);
const BROWSER_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// Ask a server how people sign in to it. No credentials are sent.
fn sign_in_methods(url: &str) -> Result<SignInMethods, String> {
    let address = format!("{url}/api/v1/auth/methods");
    let reply = Request {
        method: "GET",
        url: &address,
        body: None,
    }
    .send(65_536)
    .map_err(transport_message)?;
    SignInMethods::read(reply.status, &reply.body)
}

fn transport_message(failure: Failure) -> String {
    match failure {
        Failure::TimedOut => "The server took too long to respond. Try again.",
        Failure::TooLarge => "The server returned an unexpected response.",
        Failure::Unreachable | Failure::Transport => "Could not reach this server",
    }
    .into()
}

/// Trade the code the browser handed back for a session: the same kind of
/// token a password sign-in returns, checked against the server the same way.
fn exchange(attempt: &BrowserSignIn, code: &str) -> Result<VerifiedAccount, String> {
    let (address, body) = attempt.exchange(code);
    let reply = Request {
        method: "POST",
        url: &address,
        body: Some(&body),
    }
    .send(65_536)
    .map_err(transport_message)?;
    let token = BrowserSignIn::read_exchange(reply.status, &reply.body)?;
    verify(attempt.server().to_owned(), Secret::new(token))
}

pub struct AccountView {
    pub url: Entity<TextInput>,
    pub email: Entity<TextInput>,
    pub password: Entity<TextInput>,
    busy: bool,
    error: Option<String>,
    discovery: Discovery,
    /// The address the form last asked about, and which question that was.
    /// Only the answer for the address now in the field is ever shown.
    asked: Option<String>,
    question: u64,
    /// The sign-in that is waiting for the browser to answer, and its turn.
    waiting: Option<(BrowserSignIn, u64)>,
    turn: u64,
    _subscriptions: Vec<Subscription>,
    focus: FocusHandle,
}
impl EventEmitter<AccountEvent> for AccountView {}
impl AccountView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let url = cx.new(|cx| TextInput::new("https://notes.example.com", false, cx));
        url.update(cx, |i, cx| i.set_content(crate::accounts::configured_url(), cx));
        let email = cx.new(|cx| TextInput::new("Email", false, cx));
        let password = cx.new(|cx| TextInput::new("Password", false, cx));
        password.update(cx, |i, cx| i.set_secure(cx));
        let subscriptions = vec![
            cx.subscribe(&password, |this, _, _: &InputSubmitted, cx| this.submit(cx)),
            cx.subscribe(&email, |_, _, _: &InputChanged, cx| cx.notify()),
            cx.subscribe(&password, |_, _, _: &InputChanged, cx| cx.notify()),
            cx.subscribe(&url, |_, _, _: &InputChanged, cx| cx.notify()),
        ];
        Self {
            url,
            email,
            password,
            busy: false,
            error: None,
            discovery: Discovery::Idle,
            asked: None,
            question: 0,
            waiting: None,
            turn: 0,
            _subscriptions: subscriptions,
            focus: cx.focus_handle(),
        }
    }

    /// Start a sign-in to `server`: its address goes in the field and the
    /// form asks it how it signs in.
    pub fn sign_in_to(&mut self, server: String, cx: &mut Context<Self>) {
        self.url.update(cx, |input, cx| input.set_content(server, cx));
        self.error = None;
        self.asked = None;
        cx.notify();
    }

    /// Ask the server whose address is in the field how people sign in to
    /// it, once typing has paused. Each change starts a new question, and an
    /// answer to an earlier one, however late it arrives, is dropped.
    /// A state of the form to look at, for the scripted tour: no server is
    /// asked anything and nobody is signed in or out.
    #[cfg(debug_assertions)]
    pub fn review(&mut self, state: &str, cx: &mut Context<Self>) {
        let (address, methods) = match state {
            "google" => ("https://notes.example.com", (false, true)),
            "both" | "waiting" => ("https://notes.example.com", (true, true)),
            "http-private" => ("http://192.168.1.20:3000", (true, false)),
            "http-public" => ("http://notes.example.com", (true, false)),
            _ => ("https://notes.example.com", (true, false)),
        };
        self.url.update(cx, |input, cx| input.set_content(address, cx));
        self.asked = Some(address.to_owned());
        self.question = self.question.wrapping_add(1);
        self.error = None;
        self.busy = false;
        self.waiting = None;
        let offered = SignInMethods {
            password: methods.0,
            google: methods.1,
            browser: methods.1,
            instance_name: None,
        };
        self.discovery = match state {
            "checking" => Discovery::Checking,
            "unreachable" => Discovery::Failed(
                "Could not reach that server. Check the address and that this Mac can reach it."
                    .into(),
            ),
            _ => Discovery::Ready(offered),
        };
        match state {
            "waiting" => {
                self.turn = self.turn.wrapping_add(1);
                self.waiting = BrowserSignIn::new(address.to_owned())
                    .ok()
                    .map(|attempt| (attempt, self.turn));
            }
            "wrong-password" => {
                self.email
                    .update(cx, |input, cx| input.set_content("sam@cereal.example", cx));
                self.error = Some("That email or password is not right.".into());
            }
            "busy" => self.busy = true,
            _ => {}
        }
        cx.notify();
    }

    pub fn follow_address(&mut self, cx: &mut Context<Self>) {
        let address = self.url.read(cx).content().trim().to_owned();
        if self.asked.as_deref() == Some(address.as_str()) {
            return;
        }
        let first = self.asked.is_none();
        self.ask(address, if first { Duration::ZERO } else { DISCOVERY_PAUSE }, cx);
    }

    fn ask(&mut self, address: String, pause: Duration, cx: &mut Context<Self>) {
        self.asked = Some(address.clone());
        self.question = self.question.wrapping_add(1);
        let asked = self.question;
        // What was offered for the old address is withdrawn at once, and a
        // browser sign-in that was waiting for it is over.
        self.discovery = Discovery::Idle;
        self.waiting = None;
        if address.is_empty() {
            return;
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(pause).await;
            let target = this
                .update(cx, |this, cx| {
                    if this.question != asked {
                        return None;
                    }
                    cx.notify();
                    match validate_url(&address) {
                        Ok(url) => {
                            this.discovery = Discovery::Checking;
                            Some(url)
                        }
                        Err(error) => {
                            this.discovery = Discovery::Failed(error);
                            None
                        }
                    }
                })
                .ok()
                .flatten();
            let Some(url) = target else { return };
            let found = cx
                .background_executor()
                .spawn(async move { sign_in_methods(&url) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.question != asked {
                    return;
                }
                this.discovery = match found {
                    Ok(methods) => Discovery::Ready(methods),
                    Err(error) => Discovery::Failed(error),
                };
                cx.notify();
            });
        })
        .detach();
    }

    /// What the form knows about the server whose address is in the field.
    pub fn discovery(&self) -> &Discovery {
        &self.discovery
    }

    /// A password sign-in or a browser exchange is on its way.
    pub fn busy(&self) -> bool {
        self.busy
    }

    /// A sign-in in the browser has been started and not yet answered.
    pub fn waiting(&self) -> bool {
        self.waiting.is_some()
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Ask the server in the field again, now.
    pub fn check_again(&mut self, cx: &mut Context<Self>) {
        let address = self.url.read(cx).content().trim().to_owned();
        self.error = None;
        self.ask(address, Duration::ZERO, cx);
        cx.notify();
    }

    /// The end of a sign-in, by password or through the browser: the session
    /// is saved and announced, or the reason it failed is shown.
    fn finish(
        &mut self,
        request_attempt: u64,
        result: Result<VerifiedAccount, String>,
        cx: &mut Context<Self>,
    ) {
        self.busy = false;
        if request_attempt != attempt() {
            cx.notify();
            return;
        }
        match result {
            // The registry saves the session (and reads it back) when the
            // app takes the account in; a failure there comes back here.
            Ok(a) => cx.emit(AccountEvent::SignedIn(a)),
            Err(e) => self.error = Some(e),
        }
        cx.notify();
    }

    /// The sign-in was good but could not be taken in (the session could
    /// not be saved, a recording is running): say why, on the form.
    pub fn fail(&mut self, message: String, cx: &mut Context<Self>) {
        self.busy = false;
        self.error = Some(message);
        cx.notify();
    }

    pub fn submit(&mut self, cx: &mut Context<Self>) {
        if self.busy || self.waiting.is_some() {
            return;
        }
        let url = self.url.read(cx).content();
        let email = self.email.read(cx).content().trim().to_owned();
        let password = self.password.read(cx).content();
        if email.is_empty() || password.is_empty() {
            self.error = Some("Enter your email and password".into());
            cx.notify();
            return;
        }
        self.password.update(cx, |i, cx| i.set_content("", cx));
        self.busy = true;
        self.error = None;
        let request_attempt = next_attempt();
        let (send, receive) = mpsc::channel();
        std::thread::spawn(move || {
            let result = sign_in(url, email, password);
            let _ = send.send(result);
        });
        cx.spawn(async move |this, cx| {
            let result = loop {
                match receive.try_recv() {
                    Ok(r) => break r,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        break Err("Sign-in worker stopped".into());
                    }
                    Err(mpsc::TryRecvError::Empty) => {
                        cx.background_executor()
                            .timer(Duration::from_millis(50))
                            .await
                    }
                }
            };
            let _ = this.update(cx, |this, cx| this.finish(request_attempt, result, cx));
        })
        .detach();
        cx.notify();
    }

    /// Sign in on the server's own page, in the default browser, by
    /// whichever method the server offers there. The session that comes back
    /// is kept exactly as one from the password form is.
    pub fn continue_in_browser(&mut self, cx: &mut Context<Self>) {
        if self.busy || self.waiting.is_some() {
            return;
        }
        self.error = None;
        let started = validate_url(&self.url.read(cx).content())
            .and_then(|server| BrowserSignIn::new(server).map_err(|error| error.message().into()));
        let attempt = match started {
            Ok(attempt) => attempt,
            Err(error) => {
                self.error = Some(error);
                cx.notify();
                return;
            }
        };
        crate::shared_view::open_in_browser(&attempt.login_url(), cx);
        self.turn = self.turn.wrapping_add(1);
        let turn = self.turn;
        self.waiting = Some((attempt, turn));
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(BROWSER_TIMEOUT).await;
            let _ = this.update(cx, |this, cx| {
                if this.waiting.as_ref().is_some_and(|(_, waiting)| *waiting == turn) {
                    this.waiting = None;
                    this.error = Some(HandoffError::TimedOut.message().into());
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    pub fn cancel_browser(&mut self, cx: &mut Context<Self>) {
        self.waiting = None;
        cx.notify();
    }

    /// The browser's answer, `cereal://auth?…`. Only the sign-in that is
    /// waiting for one **and whose state matches** can use it. The state is
    /// compared before the waiting attempt is consumed: an answer that is
    /// not for it (a stale link, a link someone else sent) leaves it
    /// waiting, exactly as if nothing had arrived.
    pub fn receive_answer(&mut self, answer: AuthCallback, cx: &mut Context<Self>) {
        let Some((attempt, code)) = take_answered(&mut self.waiting, &answer) else {
            return;
        };
        self.busy = true;
        self.error = None;
        let request_attempt = next_attempt();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { exchange(&attempt, &code) })
                .await;
            let _ = this.update(cx, |this, cx| this.finish(request_attempt, result, cx));
        })
        .detach();
        cx.notify();
    }
}

/// The waiting sign-in and its code, if `answer` is its answer. The state
/// (and the server, when the answer names one) is compared **before** the
/// waiting attempt is consumed: an answer that is not for it leaves it
/// waiting, and nothing is said.
fn take_answered(
    waiting: &mut Option<(BrowserSignIn, u64)>,
    answer: &AuthCallback,
) -> Option<(BrowserSignIn, String)> {
    let code = waiting.as_ref()?.0.code(answer).ok()?.to_owned();
    waiting.take().map(|(attempt, _)| (attempt, code))
}

impl Render for AccountView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The account page follows the app's theme, like the sign-in screen.
        let t = Theme::new(crate::ui::is_dark());
        self.follow_address(cx);
        let field = |input: &Entity<TextInput>| {
            div()
                .w_full()
                .p(px(12.))
                .rounded(px(12.))
                .bg(t.card)
                .border_1()
                .border_color(t.border)
                .child(input.clone())
        };
        let mut form = div()
            .w(px(400.))
            .max_w_full()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(heading("Your account", t));
        if let Some(session) = crate::accounts::default_session() {
            form = form
                .child(
                    pill("account-back", "Back to notes", "chevron-left", t)
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(AccountEvent::Back))),
                )
                .child(session.user.name)
                .child(small(session.user.email, t))
                .child(
                    pill("account-sign-out", "Sign out", "log-out", t)
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(AccountEvent::SignOut))),
                );
        } else if !crate::accounts::server_mode() {
            // Nothing is connected yet: the notes on this Mac are still there
            // to go back to.
            form = form.child(
                pill("account-back", "Back to notes", "chevron-left", t)
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(AccountEvent::Back))),
            );
        }
        form = form.child(field(&self.url));
        // How what is typed there would travel (see the welcome's server
        // step, which says the same).
        let transport = crate::link::address_transport(&self.url.read(cx).content());
        if let Some((transport, note)) =
            transport.and_then(|transport| Some((transport, transport.note()?)))
        {
            form = form.child(if transport.warns() {
                div()
                    .flex()
                    .items_start()
                    .gap(px(8.))
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(t.warning_ink)
                    .child(icon("warning", 16., t.warning_ink))
                    .child(div().flex_1().min_w_0().child(note))
            } else {
                small(note, t)
            });
        }
        let mut password_offered = true;
        match self.discovery.clone() {
            Discovery::Idle => {}
            Discovery::Checking => {
                form = form.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(crate::motion::spinner(t.muted))
                        .child(small("Checking how this server signs in…", t)),
                );
            }
            Discovery::Failed(message) => {
                form = form.child(small(message, t)).child(
                    pill("account-check-again", "Try again", "refresh", t).on_click(cx.listener(
                        |this, _, _, cx| {
                            let address = this.url.read(cx).content().trim().to_owned();
                            this.ask(address, Duration::ZERO, cx);
                            cx.notify();
                        },
                    )),
                );
            }
            Discovery::Ready(_) if self.waiting.is_some() => {
                password_offered = false;
                form = form
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(crate::motion::spinner(t.muted))
                            .child(small("Waiting for you to sign in in your browser…", t)),
                    )
                    .child(
                        pill("account-browser-cancel", "Cancel", "", t)
                            .on_click(cx.listener(|this, _, _, cx| this.cancel_browser(cx))),
                    );
            }
            Discovery::Ready(methods) => {
                password_offered = methods.password;
                if methods.password {
                    form = form
                        .child(field(&self.email))
                        .child(field(&self.password))
                        .child(
                            pill(
                                "account-sign-in",
                                if self.busy {
                                    "Signing in…"
                                } else {
                                    "Sign in"
                                },
                                "users",
                                t,
                            )
                            .on_click(cx.listener(|this, _, _, cx| this.submit(cx))),
                        );
                }
                if let Some(label) = methods.browser_label() {
                    let google = label == "Continue with Google";
                    // The pill's own shape, with Google's mark in its four
                    // colours where a tinted icon would otherwise be.
                    form = form.child(
                        pill(
                            "account-browser",
                            if self.busy && !methods.password {
                                "Signing in…"
                            } else {
                                label
                            },
                            if google { "" } else { "globe" },
                            t,
                        )
                        .when(google, |pill| {
                            pill.flex_row_reverse().child(
                                img("icons/google.svg").size(px(16.)).flex_shrink_0(),
                            )
                        })
                        .on_click(cx.listener(|this, _, _, cx| this.continue_in_browser(cx))),
                    );
                }
            }
        }
        if let Some(error) = &self.error {
            // What went wrong reads as that, as under the "Paste link" field.
            form = form.child(small(error.clone(), t).text_color(t.danger));
        }
        form = form.child(small(
            if password_offered {
                "Your password is used only to sign in. The session is kept in macOS Keychain."
            } else {
                "Your session is kept in macOS Keychain."
            },
            t,
        ));
        div()
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .p(px(32.))
            .bg(t.bg)
            .text_color(t.text)
            .child(form)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // `gpui::*` re-exports its own `test` attribute; these are plain unit tests.
    use std::prelude::v1::test;

    #[test]
    fn k3_a_wrong_auth_answer_leaves_the_waiting_sign_in_waiting() {
        let attempt = BrowserSignIn::new("https://notes.example.com".into()).unwrap();
        let state = attempt
            .login_url()
            .rsplit_once("state=")
            .unwrap()
            .1
            .to_owned();
        let answer = |state: &str, server: Option<&str>| AuthCallback {
            code: "the-code".into(),
            state: state.into(),
            origin: server.map(|server| crate::link::ServerOrigin::parse(server).unwrap()),
        };
        let mut waiting = Some((attempt, 7));
        // A stale link, a link someone else sent, the right state from
        // another server: none of them is this sign-in's answer, and none
        // of them ends it.
        for wrong in [
            answer("another-state", None),
            answer("", None),
            answer(&format!("{state}x"), None),
            answer(&state, Some("https://evil.example")),
        ] {
            assert!(take_answered(&mut waiting, &wrong).is_none());
            assert!(waiting.is_some(), "the sign-in is still waiting");
        }
        // Its own answer is used, once.
        let (taken, code) = take_answered(&mut waiting, &answer(&state, None)).unwrap();
        assert_eq!(code, "the-code");
        assert_eq!(taken.server(), "https://notes.example.com");
        assert!(waiting.is_none());
        assert!(take_answered(&mut waiting, &answer(&state, None)).is_none());
    }

    #[test]
    #[ignore = "requires RUN_NATIVE_AUTH_TESTS and two synthetic local accounts"]
    fn live_native_signed_sessions_keep_account_notes_separate() {
        use crate::accounts::{AccountClient, client::SessionCheck};
        use serde_json::{Value, json};
        let get = |key: &str| {
            std::env::var(key).expect("Missing synthetic native-auth test configuration")
        };
        assert_eq!(get("RUN_NATIVE_AUTH_TESTS"), "1");
        let url = get("NATIVE_AUTH_URL");
        assert!(
            url.starts_with("http://127.0.0.1:"),
            "Auth probe is loopback-only"
        );
        let a = sign_in(
            url.clone(),
            get("NATIVE_AUTH_A_EMAIL"),
            get("NATIVE_AUTH_A_PASSWORD"),
        )
        .expect("Native A sign-in failed");
        let b = sign_in(
            url.clone(),
            get("NATIVE_AUTH_B_EMAIL"),
            get("NATIVE_AUTH_B_PASSWORD"),
        )
        .expect("Native B sign-in failed");
        assert_eq!(a.session.user.id, get("NATIVE_AUTH_A_ID"));
        assert_eq!(b.session.user.id, get("NATIVE_AUTH_B_ID"));
        assert_ne!(a.session.user.id, b.session.user.id);
        // Two users on one origin: two clients, each with its own session.
        let (first, second) = (AccountClient::of_verified(&a), AccountClient::of_verified(&b));
        let note: Value = first
            .request(
                "POST",
                "/api/v1/notes",
                Some(json!({"title":"Native Rust auth QA","body":"Fictional private Rust canary"})),
            )
            .expect("Native create failed");
        let id = note["id"].as_str().expect("Missing note id");
        let b_notes: Value = second
            .request("GET", "/api/v1/notes", None)
            .expect("Native B list failed");
        assert!(
            !b_notes.as_array().unwrap().iter().any(|n| n["id"] == id),
            "A private note leaked into B's list"
        );
        assert!(
            second
                .request::<Value>("GET", &format!("/api/v1/notes/{id}"), None)
                .is_err(),
            "B could read A's note"
        );
        assert!(
            second
                .request::<Value>("GET", "/api/v1/admin/settings", None)
                .is_err(),
            "A regular user could read admin configuration"
        );
        assert!(
            second
                .request::<Value>("GET", "/api/v1/calendar/preferences", None)
                .is_ok(),
            "A regular user could not read calendar preferences"
        );
        assert!(
            first
                .request::<Value>("DELETE", &format!("/api/v1/notes/{id}"), None)
                .is_ok(),
            "Native cleanup failed"
        );
        for verified in [a, b] {
            AccountClient::of_verified(&verified).sign_out_on_server();
            assert!(
                matches!(
                    client::session_check(&url, &verified.token),
                    SessionCheck::Ended
                ),
                "Revoked native session remained active"
            );
        }
    }
}
