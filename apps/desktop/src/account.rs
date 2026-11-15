//! Native Better Auth sessions. Passwords are used once, signed session bearers
//! remain in macOS Keychain, and private caches are keyed by stable user UUID.
use crate::{
    app::{Cereal, Screen},
    fetch::{Failure, Request},
    handoff::{BrowserSignIn, HandoffError, SignInMethods},
    input::{InputChanged, InputSubmitted, TextInput},
    link::AuthCallback,
    ui::*,
};
use gpui::{prelude::*, *};
use meeting_notes_api_types::AuthSessionDTO;
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    process::{Command, Stdio},
    sync::{Mutex, OnceLock, mpsc},
    time::Duration,
};

#[derive(Clone)]
pub struct Credentials {
    pub url: String,
    pub token: String,
}
#[derive(Clone)]
pub struct VerifiedAccount {
    pub credentials: Credentials,
    pub session: AuthSessionDTO,
}
struct Runtime {
    configured: bool,
    url: String,
    credentials: Option<Credentials>,
    session: Option<AuthSessionDTO>,
    epoch: u64,
    attempt: u64,
}
static RUNTIME: OnceLock<Mutex<Runtime>> = OnceLock::new();
const SERVICE: &str = "dev.cereal.desktop.session";
const ACCOUNT: &str = "current-session";
/// An instance pointed at its own `CEREAL_DATA_DIR` keeps its session and its
/// server address in that directory, and never opens the Keychain: a test
/// instance cannot read, replace or delete the session of the installed app.
/// A debug build does so whenever the directory is set. A release build does
/// so only when `CEREAL_ISOLATED_SESSION=1` asks for it as well; otherwise it
/// always uses the Keychain.
fn isolated_directory() -> Option<std::path::PathBuf> {
    let asked = crate::environment::var("CEREAL_ISOLATED_SESSION").is_ok_and(|value| value == "1");
    if !cfg!(debug_assertions) && !asked {
        return None;
    }
    crate::environment::var_os("CEREAL_DATA_DIR")
        .filter(|directory| !directory.is_empty())
        .map(std::path::PathBuf::from)
}
fn metadata_path() -> Option<std::path::PathBuf> {
    if let Some(directory) = isolated_directory() {
        return Some(directory.join("connection.json"));
    }
    std::env::var_os("HOME").map(|p| {
        std::path::PathBuf::from(p).join("Library/Application Support/Cereal/connection.json")
    })
}
fn read_saved_session() -> Option<Vec<u8>> {
    match isolated_directory() {
        Some(directory) => std::fs::read(directory.join("session.json")).ok(),
        None => security_framework::passwords::get_generic_password(SERVICE, ACCOUNT).ok(),
    }
}
fn write_saved_session(bytes: &[u8]) -> Result<(), String> {
    match isolated_directory() {
        Some(directory) => crate::shared::write_private(&directory.join("session.json"), bytes)
            .map_err(|_| "The session could not be saved".into()),
        None => security_framework::passwords::set_generic_password(SERVICE, ACCOUNT, bytes)
            .map_err(|_| "The session could not be saved in Keychain".into()),
    }
}
fn delete_saved_session() {
    match isolated_directory() {
        Some(directory) => {
            let _ = std::fs::remove_file(directory.join("session.json"));
        }
        None => {
            let _ = security_framework::passwords::delete_generic_password(SERVICE, ACCOUNT);
        }
    }
}
fn runtime() -> &'static Mutex<Runtime> {
    RUNTIME.get_or_init(|| {
        let saved = metadata_path()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .and_then(|v| v.get("url").and_then(Value::as_str).map(str::to_owned));
        let url = crate::environment::var("CEREAL_SERVER_URL")
            .ok()
            .or(saved)
            .unwrap_or_default();
        let normalized = validate_url(&url).ok();
        // The origin travels atomically with the token inside Keychain. An env
        // URL override must never send a saved session to another server.
        let saved_credentials =
            read_saved_session()
                .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
                .and_then(|v| {
                    let saved_url = v.get("url")?.as_str()?;
                    let token = v.get("token")?.as_str()?;
                    (normalized.as_deref() == Some(saved_url)).then(|| Credentials {
                        url: saved_url.to_owned(),
                        token: token.to_owned(),
                    })
                });
        let credentials = if let Ok(token) = crate::environment::var("CEREAL_SERVER_TOKEN") {
            normalized.map(|url| Credentials { url, token })
        } else {
            saved_credentials
        };
        Mutex::new(Runtime {
            configured: !url.is_empty(),
            url,
            credentials,
            session: None,
            epoch: 1,
            attempt: 1,
        })
    })
}
pub fn configured() -> bool {
    runtime().lock().unwrap().configured
}
pub fn epoch() -> u64 {
    runtime().lock().unwrap().epoch
}
fn attempt() -> u64 {
    runtime().lock().unwrap().attempt
}
fn next_attempt() -> u64 {
    let mut state = runtime().lock().unwrap();
    state.attempt = state.attempt.wrapping_add(1);
    state.attempt
}
pub fn credentials() -> Result<Credentials, String> {
    runtime()
        .lock()
        .unwrap()
        .credentials
        .clone()
        .ok_or_else(|| "Sign in to your server first".into())
}
pub fn session() -> Option<AuthSessionDTO> {
    runtime().lock().unwrap().session.clone()
}
pub fn identity() -> Option<String> {
    let state = runtime().lock().unwrap();
    state
        .session
        .as_ref()
        .map(|s| format!("{}|{}", state.url, s.user.id.to_lowercase()))
}
/// A session is saved for the server this app is set up to use (or one was
/// given for this run), whether or not it has been checked yet.
pub fn has_saved_session() -> bool {
    runtime().lock().unwrap().credentials.is_some()
}
pub fn configured_url() -> String {
    runtime().lock().unwrap().url.clone()
}
/// The server this app is set up to use, as a checked origin, whether or not
/// anyone is signed in to it.
pub fn server_origin() -> Option<String> {
    validate_url(&configured_url()).ok()
}
fn validate_url(raw: &str) -> Result<String, String> {
    let raw = raw.trim().trim_end_matches('/');
    let (scheme, rest) = raw.split_once("://").ok_or("Enter a valid server URL")?;
    let authority = rest.split('/').next().unwrap_or("");
    let host = authority.split(':').next().unwrap_or("");
    let loopback = host == "localhost" || host == "127.0.0.1" || authority.starts_with("[::1]:");
    if authority.is_empty()
        || !((scheme == "https") || (scheme == "http" && loopback))
        || raw.chars().any(|c| c.is_control() || c.is_whitespace())
        || raw.contains(['@', '?', '#', '"', '\\'])
        || rest.split('/').any(|p| p == "." || p == "..")
    {
        return Err("Use HTTPS, or HTTP on localhost for development".into());
    }
    Ok(raw.to_owned())
}
fn http(
    credentials: &Credentials,
    path: &str,
    body: Option<Value>,
) -> Result<(Value, Option<String>), String> {
    let quote = |s: &str| {
        format!(
            "\"{}\"",
            s.replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\n', "\\n")
                .replace('\r', "\\r")
        )
    };
    let mut config = format!(
        "url = {}\nrequest = {}\nheader = \"Content-Type: application/json\"\n",
        quote(&format!("{}{path}", credentials.url)),
        quote(if body.is_some() { "POST" } else { "GET" })
    );
    if !credentials.token.is_empty() {
        config.push_str(&format!(
            "header = {}\n",
            quote(&format!("Authorization: Bearer {}", credentials.token))
        ));
    }
    if let Some(body) = body {
        config.push_str(&format!("data-binary = {}\n", quote(&body.to_string())));
    }
    let mut child = Command::new("/usr/bin/curl")
        .args([
            "--disable",
            "--silent",
            "--show-error",
            "--max-time",
            "25",
            "--max-redirs",
            "0",
            "--dump-header",
            "-",
            "--write-out",
            "\n%{http_code}",
            "--config",
            "-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "Sign-in transport could not start")?;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(config.as_bytes())
        .map_err(|_| "Sign-in request could not be sent")?;
    let mut bytes = Vec::new();
    child
        .stdout
        .take()
        .unwrap()
        .take(1_048_577)
        .read_to_end(&mut bytes)
        .map_err(|_| "Sign-in response could not be read")?;
    if bytes.len() > 1_048_576 {
        let _ = child.kill();
        let _ = child.wait();
        return Err("Sign-in response was too large".into());
    }
    if !child
        .wait()
        .map_err(|_| "Sign-in transport failed")?
        .success()
    {
        return Err("Could not reach this server".into());
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| "Invalid sign-in response")?;
    let (mut response, status) = text.rsplit_once('\n').ok_or("Invalid sign-in response")?;
    if !status.starts_with('2') {
        return Err(if status == "401" || status == "403" {
            "Email, password or session was not accepted"
        } else {
            "The server could not finish signing in"
        }
        .into());
    }
    let mut token = None;
    while response.starts_with("HTTP/") {
        let (headers, rest) = response
            .split_once("\r\n\r\n")
            .ok_or("Invalid sign-in headers")?;
        for line in headers.lines() {
            if let Some((name, value)) = line.split_once(':') {
                if name.eq_ignore_ascii_case("set-auth-token") {
                    token = Some(value.trim().to_owned());
                }
            }
        }
        response = rest;
    }
    Ok((
        serde_json::from_str(response).map_err(|_| "Invalid sign-in response")?,
        token,
    ))
}
fn verify(credentials: Credentials) -> Result<VerifiedAccount, String> {
    if credentials.token.is_empty()
        || !credentials
            .token
            .bytes()
            .all(|b| (0x21..=0x7e).contains(&b))
    {
        return Err("Invalid session credential".into());
    }
    let (value, _) = http(&credentials, "/api/v1/session", None)?;
    let session: AuthSessionDTO = serde_json::from_value(value)
        .map_err(|_| "The server session does not match this client")?;
    uuid::Uuid::parse_str(&session.user.id).map_err(|_| "Invalid account identity")?;
    Ok(VerifiedAccount {
        credentials,
        session,
    })
}
fn sign_in(url: String, email: String, password: String) -> Result<VerifiedAccount, String> {
    let mut credentials = Credentials {
        url: validate_url(&url)?,
        token: String::new(),
    };
    let (_, token) = http(
        &credentials,
        "/api/auth/sign-in/email",
        Some(json!({"email":email,"password":password,"rememberMe":true})),
    )?;
    credentials.token = token
        .filter(|s| {
            !s.is_empty() && s.len() < 8192 && s.bytes().all(|b| (0x21..=0x7e).contains(&b))
        })
        .ok_or("The server did not return a signed session")?;
    verify(credentials)
}
fn persist(account: &VerifiedAccount) -> Result<(), String> {
    write_saved_session(
        serde_json::to_vec(
            &json!({"url":account.credentials.url,"token":account.credentials.token}),
        )
        .unwrap()
        .as_slice(),
    )?;
    let path = metadata_path().ok_or("Cannot find account preferences")?;
    std::fs::create_dir_all(path.parent().unwrap())
        .map_err(|_| "Cannot save account preferences")?;
    let bytes = serde_json::to_vec(&json!({"url":account.credentials.url})).unwrap();
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .map_err(|_| "Cannot save account preferences")?;
    file.write_all(&bytes)
        .map_err(|_| "Cannot save account preferences".to_string())
}
fn activate(account: VerifiedAccount) {
    let mut state = runtime().lock().unwrap();
    state.epoch = state.epoch.wrapping_add(1);
    state.attempt = state.attempt.wrapping_add(1);
    state.configured = true;
    state.url = account.credentials.url.clone();
    state.credentials = Some(account.credentials);
    state.session = Some(account.session);
}
fn clear() {
    let mut state = runtime().lock().unwrap();
    state.epoch = state.epoch.wrapping_add(1);
    state.attempt = state.attempt.wrapping_add(1);
    state.credentials = None;
    state.session = None;
    drop(state);
    delete_saved_session();
    if let Some(path) = metadata_path() {
        let _ = std::fs::remove_file(path);
    }
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
        bearer: None,
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
        bearer: None,
        body: Some(&body),
    }
    .send(65_536)
    .map_err(transport_message)?;
    let token = BrowserSignIn::read_exchange(reply.status, &reply.body)?;
    verify(Credentials {
        url: attempt.server().to_owned(),
        token,
    })
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
        url.update(cx, |i, cx| i.set_content(configured_url(), cx));
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
            Ok(a) => match persist(&a) {
                Ok(()) => cx.emit(AccountEvent::SignedIn(a)),
                Err(error) => self.error = Some(error),
            },
            Err(e) => self.error = Some(e),
        }
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
    /// waiting for one can use it; with nothing waiting it is ignored.
    pub fn receive_answer(&mut self, answer: AuthCallback, cx: &mut Context<Self>) {
        let Some((attempt, _)) = self.waiting.take() else {
            return;
        };
        let code = match attempt.code(&answer) {
            Ok(code) => code.to_owned(),
            Err(error) => {
                self.error = Some(error.message().into());
                cx.notify();
                return;
            }
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
impl Render for AccountView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = Theme::new(true);
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
        if let Some(session) = session() {
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
        } else if !configured() {
            // Nothing is connected yet: the notes on this Mac are still there
            // to go back to.
            form = form.child(
                pill("account-back", "Back to notes", "chevron-left", t)
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(AccountEvent::Back))),
            );
        }
        form = form.child(field(&self.url));
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
            form = form.child(small(error.clone(), t));
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
impl Cereal {
    pub fn reset_account_state(&mut self, cx: &mut Context<Self>) {
        self.stop_all_chat_streams(cx);
        self.save_chat_history();
        self.finish_recording_preview(cx);
        // The outgoing account's edits are written under its own path before
        // anything is cleared or the identity changes.
        self.flush_all_blocking(cx);
        self.summary.dismiss();
        self.generation.cancel();
        self.server_bridge = None;
        self.server_pending = 0;
        self.server_status = None;
        self.workspace = crate::model::Workspace::default();
        self.current_note = None;
        self.chat_models.clear();
        self.selected_chat_model = None;
        self.chat_model_picker_open = false;
        self.chat_messages.clear();
        self.chat_annotations.clear();
        self.chat_archives.clear();
        self.chat_recipes.clear();
        self.chat_recipe_editor = None;
        self.chat_recipes_open = false;
        self.chat_history_open = false;
        self.audio_pending.clear();
        self.audio_error = None;
        self.assistant_open = false;
        self.transcript_open = false;
        self.calendars = Default::default();
        self.meeting = Default::default();
        self.title_input.update(cx, |i, cx| i.set_content("", cx));
        self.editor.update(cx, |i, cx| i.set_content("", cx));
        self.ask.update(cx, |i, cx| i.set_content("", cx));
        self.search.update(cx, |i, cx| i.set_content("", cx));
        self.search_results_query.clear();
        self.folder_input.update(cx, |i, cx| i.set_content("", cx));
        self.attendee_input
            .update(cx, |i, cx| i.set_content("", cx));
        self.search_open = false;
        self.menu = None;
        self.visible_menu = None;
        self.toast = None;
        self.chat_editing = None;
        self.chat_interrupted.clear();
        self.chat_thread_id = uuid::Uuid::new_v4().to_string();
        self.note_body_dirty = false;
        self.transcript_last_text.clear();
        self.transcript_last_query.clear();
        self.transcript_epoch = self.transcript_epoch.wrapping_add(1);
        self.regeneration_notice_dismissed = None;
    }
    pub fn accept_account(&mut self, account: VerifiedAccount, cx: &mut Context<Self>) {
        self.reset_account_state(cx);
        activate(account);
        self.workspace = crate::model::Workspace::load();
        if std::env::args().any(|argument| argument == "--dark") {
            self.workspace.settings.dark_mode = true;
        }
        self.chat_archives = crate::chat::load_history();
        self.chat_recipes = crate::chat::load_recipes();
        // A sign-in made in the welcome carries on there: its next step.
        if self.screen == Screen::Welcome && self.onboarding.flow.is_some() {
            self.onboarding_signed_in(cx);
        } else {
            self.screen = Screen::Home;
        }
        self.connect_server(cx);
        // A link that was waiting for this session, or the note this
        // sign-in was started for.
        self.resume_links(cx);
        cx.notify();
    }
    pub fn sign_out_account(&mut self, cx: &mut Context<Self>) {
        let previous = credentials().ok();
        self.reset_account_state(cx);
        clear();
        self.screen = Screen::Account;
        if let Some(previous) = previous {
            std::thread::spawn(move || {
                let _ = http(&previous, "/api/auth/sign-out", Some(json!({})));
            });
        }
        cx.notify();
    }
    pub fn restore_account(&mut self, cx: &mut Context<Self>) {
        if self.server_pending > 0 {
            return;
        }
        self.server_pending = 1;
        // The welcome has its own server step and stays where it is.
        let welcoming = self.screen == Screen::Welcome;
        if !welcoming {
            self.screen = Screen::Account;
        }
        let started = epoch();
        let request_attempt = attempt();
        let (send, receive) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = send.send(credentials().and_then(verify));
        });
        cx.spawn(async move |this, cx| {
            let result = loop {
                match receive.try_recv() {
                    Ok(r) => break r,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        break Err("Session verification stopped".into());
                    }
                    Err(mpsc::TryRecvError::Empty) => {
                        cx.background_executor()
                            .timer(Duration::from_millis(50))
                            .await
                    }
                }
            };
            let _ = this.update(cx, |this, cx| {
                if epoch() != started || attempt() != request_attempt {
                    return;
                }
                this.server_pending = 0;
                match result {
                    Ok(a) => this.accept_account(a, cx),
                    Err(e) => {
                        this.server_status = Some(e.clone());
                        if !welcoming {
                            this.show_toast(&e, cx);
                        }
                        // Nobody is signed in: a link that was waiting to
                        // find that out says where its note is.
                        this.resume_links(cx);
                    }
                }
            });
        })
        .detach();
    }
}
#[cfg(test)]
mod tests {
    use super::validate_url;
    #[test]
    #[ignore = "requires RUN_NATIVE_AUTH_TESTS and two synthetic local accounts"]
    fn live_native_signed_sessions_keep_account_notes_separate() {
        use super::{Credentials, http, sign_in};
        use serde_json::json;
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
        let (note, _) = http(
            &a.credentials,
            "/api/v1/notes",
            Some(json!({"title":"Native Rust auth QA","body":"Fictional private Rust canary"})),
        )
        .expect("Native create failed");
        let id = note["id"].as_str().expect("Missing note id");
        let (b_notes, _) =
            http(&b.credentials, "/api/v1/notes", None).expect("Native B list failed");
        assert!(
            !b_notes.as_array().unwrap().iter().any(|n| n["id"] == id),
            "A private note leaked into B's list"
        );
        assert!(
            http(&b.credentials, &format!("/api/v1/notes/{id}"), None).is_err(),
            "B could read A's note"
        );
        assert!(
            http(&b.credentials, "/api/v1/admin/settings", None).is_err(),
            "A regular user could read admin configuration"
        );
        assert!(
            http(&b.credentials, "/api/v1/calendar/preferences", None).is_ok(),
            "A regular user could not read calendar preferences"
        );
        // DELETE uses the canonical native HTTP transport instead of the auth helper.
        let client = crate::server::test_client_for_auth_probe(
            a.credentials.url.clone(),
            a.credentials.token.clone(),
        );
        assert!(
            client(&format!("/api/v1/notes/{id}")).is_ok(),
            "Native cleanup failed"
        );
        for credentials in [a.credentials, b.credentials] {
            http(&credentials, "/api/auth/sign-out", Some(json!({})))
                .expect("Native sign-out failed");
            assert!(
                http(
                    &Credentials {
                        url: url.clone(),
                        token: credentials.token
                    },
                    "/api/v1/session",
                    None
                )
                .is_err(),
                "Revoked native session remained active"
            );
        }
    }

    #[test]
    fn login_only_sends_credentials_to_https_or_explicit_loopback() {
        assert!(validate_url("https://notes.example/base").is_ok());
        assert!(validate_url("http://127.0.0.1:3000").is_ok());
        for value in [
            "http://example.com",
            "https://secret@host",
            "https://host/?token=x",
            "https://host/../api",
            "https://host\nattack",
        ] {
            assert!(validate_url(value).is_err());
        }
    }
}
