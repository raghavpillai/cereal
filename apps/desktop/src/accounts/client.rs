//! The only place a session leaves this process. An account's client holds
//! its token privately and builds every address from its own base and a path
//! under `/api/`, so a token can reach its own origin and no other: no
//! function returns the token, and none takes a token and a URL. Every curl
//! invocation here refuses redirects, any protocol but HTTP(S), and URL
//! globbing.
use super::{AccountId, keychain::Secret};
use meeting_notes_api_types::AuthSessionDTO;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::{
    collections::HashMap,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

/// What every transport passes to curl: no redirect is followed, nothing but
/// HTTP(S) is spoken, and `{}`/`[]` in an address are not expanded.
pub const SAFE_TRANSPORT: [&str; 5] = ["--max-redirs", "0", "--proto", "=https,http", "--globoff"];

/// The largest answer the ordinary transport reads.
const READ_LIMIT: usize = 8 * 1024 * 1024;

/// How a request ended when it did not succeed. The status is kept: the
/// rule for a session that has ended, the Trash rule and a move all depend
/// on which answer came back, not on its wording.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RequestError {
    /// No HTTP answer: the transport could not start or exited with a
    /// failure (the exit code is kept for the message), or the answer was
    /// larger than allowed.
    NoAnswer { exit: Option<i32> },
    Status {
        code: u16,
        error: Option<String>,
        message: Option<String>,
    },
    /// A 2xx whose body is not what the contract says.
    Malformed,
    Cancelled,
    /// The request was never made: the path is not one this client sends.
    Refused(&'static str),
}

impl RequestError {
    /// What the person is shown, in the words the app has always used.
    pub fn message(&self) -> String {
        match self {
            Self::Cancelled => "Request cancelled".into(),
            Self::NoAnswer { exit } => format!(
                "Server request failed (transport exit {}). Local audio is preserved; retry when connected.",
                exit.map(|code| code.to_string())
                    .unwrap_or_else(|| "interrupted".into())
            ),
            // Only our bounded public API error message is displayed, never
            // raw provider bodies or arbitrary response text.
            Self::Status { code, message, .. } => message
                .clone()
                .unwrap_or_else(|| format!("Server rejected the request (HTTP {code})")),
            Self::Malformed => "Server response does not match the shared API contract".into(),
            Self::Refused(reason) => (*reason).into(),
        }
    }

    pub fn status(&self) -> Option<u16> {
        match self {
            Self::Status { code, .. } => Some(*code),
            _ => None,
        }
    }

    /// The server answered that there is no such thing.
    pub fn is_not_found(&self) -> bool {
        self.status() == Some(404)
    }
}

/// A path this client may ask its server for: under `/api/`, with no
/// scheme, no authority, no way up, and nothing curl or a proxy could read
/// as more than a path. Checked before any process starts.
pub fn check_path(path: &str) -> Result<(), RequestError> {
    let refused = |reason| Err(RequestError::Refused(reason));
    if !path.starts_with("/api/") {
        return refused("Only the server's API can be asked");
    }
    if path.len() > 4096 {
        return refused("The request address is too long");
    }
    if path.contains("://") || path.contains("//") || path.contains('@') {
        return refused("The request address names another server");
    }
    if path.split(['/', '?', '&', '=']).any(|piece| piece == ".." || piece == ".") {
        return refused("The request address leaves the API");
    }
    if path
        .chars()
        .any(|c| c.is_control() || c.is_whitespace() || matches!(c, '\\' | '{' | '}' | '[' | ']' | '"' | '#'))
    {
        return refused("The request address holds characters an address cannot");
    }
    Ok(())
}

/// A note, event or calendar id on its way into a path: a UUID, written the
/// one way.
pub fn path_id(id: &str) -> Result<String, RequestError> {
    uuid::Uuid::parse_str(id)
        .map(|id| id.hyphenated().to_string())
        .map_err(|_| RequestError::Refused("That is not an id this server gave"))
}

fn quote(text: &str) -> String {
    format!(
        "\"{}\"",
        text.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
    )
}

fn authorization(token: &Secret) -> String {
    format!("Authorization: Bearer {}", token.expose())
}

/// A token as a server issues it: printable ASCII with no white space.
fn printable(token: &str) -> bool {
    !token.is_empty() && token.bytes().all(|b| (0x21..=0x7e).contains(&b))
}

/// The epoch of an account as its client saw it when it was made. A client
/// stops sending the moment its account's epoch moves (sign-out, removal, a
/// replaced token, a session that has ended).
#[derive(Clone)]
struct Watch {
    cell: Arc<AtomicU64>,
    seen: u64,
}

/// One account's way to its own server.
#[derive(Clone)]
pub struct AccountClient {
    /// The account's address: the base of every request.
    pub(crate) url: String,
    token: Secret,
    /// Notes of this account that are known to be on the server, by handle.
    pub(crate) ids: HashMap<u64, String>,
    pub(crate) account: Option<AccountId>,
    pub(crate) slot: Option<u16>,
    watch: Option<Watch>,
    /// Raised when the server answers 401 or 403 to anything: the account's
    /// own `/session` is then asked once, and only its answer changes the
    /// account's state.
    suspect: Option<Arc<AtomicBool>>,
    /// The account's `recordings` directory.
    pub(crate) storage_root: Option<PathBuf>,
}

impl AccountClient {
    pub(super) fn new(
        account: &super::Account,
        token: Secret,
        storage_root: Option<PathBuf>,
    ) -> Result<Self, String> {
        let url = account.url.clone();
        // The session goes to the address it was issued for and nowhere
        // else (it is stored with that address): HTTPS, or plain HTTP for a
        // server on a private network.
        if !(url.starts_with("https://") || url.starts_with("http://"))
            || url.contains(['\n', '\r', '"', '@', '?', '#', '|'])
            || token.expose().len() < 32
            || !printable(token.expose())
        {
            return Err("Configure CEREAL_SERVER_URL (https:// or http://) and CEREAL_SERVER_TOKEN before connecting".into());
        }
        Ok(Self {
            url,
            token,
            ids: HashMap::new(),
            account: Some(account.id.clone()),
            slot: Some(account.slot),
            watch: Some(Watch {
                cell: account.epoch_cell(),
                seen: account.epoch(),
            }),
            suspect: Some(account.suspect_cell()),
            storage_root,
        })
    }

    /// A client for a local fake server, bound to no account.
    #[cfg(test)]
    pub(crate) fn for_test(url: impl Into<String>, token: &str, root: Option<&Path>) -> Self {
        Self {
            url: url.into(),
            token: Secret::new(token),
            ids: HashMap::new(),
            account: None,
            slot: None,
            watch: None,
            suspect: None,
            storage_root: root.map(Path::to_path_buf),
        }
    }

    /// A client for a session a server has just confirmed, bound to no
    /// account yet.
    #[cfg(test)]
    pub(crate) fn of_verified(verified: &super::VerifiedAccount) -> Self {
        Self {
            token: verified.token.clone(),
            ..Self::for_test(verified.url.clone(), "", None)
        }
    }

    #[cfg(test)]
    pub(crate) fn suspected_by(mut self, cell: Arc<AtomicBool>) -> Self {
        self.suspect = Some(cell);
        self
    }

    /// The account's epoch has moved since this client was made.
    pub(crate) fn stale(&self) -> bool {
        self.watch
            .as_ref()
            .is_some_and(|watch| watch.cell.load(Ordering::Acquire) != watch.seen)
    }

    fn stopped(&self, cancel: &Option<Arc<AtomicBool>>) -> bool {
        self.stale()
            || cancel
                .as_ref()
                .is_some_and(|cancel| cancel.load(Ordering::Acquire))
    }

    pub(crate) fn request<T: DeserializeOwned>(
        &self,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> Result<T, String> {
        self.request_with_cancel(method, path, body, None)
    }

    pub(crate) fn request_with_cancel<T: DeserializeOwned>(
        &self,
        method: &str,
        path: &str,
        body: Option<Value>,
        cancel: Option<Arc<AtomicBool>>,
    ) -> Result<T, String> {
        self.send(method, path, body, cancel)
            .map_err(|error| error.message())
    }

    /// One request to this account's server, with how it ended.
    pub(crate) fn send<T: DeserializeOwned>(
        &self,
        method: &str,
        path: &str,
        body: Option<Value>,
        cancel: Option<Arc<AtomicBool>>,
    ) -> Result<T, RequestError> {
        let body = body.map(|body| format!("data-binary = {}\n", quote(&body.to_string())));
        self.exchange(method, path, body, cancel, READ_LIMIT)
    }

    /// A request whose body is read from a private file and whose answer may
    /// be as large as `limit`: a move sends and reads whole notes.
    pub(crate) fn request_large<T: DeserializeOwned>(
        &self,
        method: &str,
        path: &str,
        body_file: &Path,
        limit: usize,
    ) -> Result<T, RequestError> {
        let file = body_file
            .to_str()
            .filter(|name| !name.contains(['"', '\n', '\r']))
            .ok_or(RequestError::Refused("The request body cannot be read"))?;
        self.exchange(
            method,
            path,
            Some(format!("data-binary = {}\n", quote(&format!("@{file}")))),
            None,
            limit,
        )
    }

    fn exchange<T: DeserializeOwned>(
        &self,
        method: &str,
        path: &str,
        body: Option<String>,
        cancel: Option<Arc<AtomicBool>>,
        limit: usize,
    ) -> Result<T, RequestError> {
        check_path(path)?;
        if self.stopped(&cancel) {
            return Err(RequestError::Cancelled);
        }
        // curl reads the token over stdin, never argv or a temporary file.
        // Config escaping is separate from shell escaping; no shell is involved.
        let mut config = format!(
            "url = {}\nrequest = {}\nheader = {}\nheader = \"Content-Type: application/json\"\n",
            quote(&format!("{}{path}", self.url)),
            quote(method),
            quote(&authorization(&self.token))
        );
        if let Some(body) = body {
            config.push_str(&body);
        }
        let no_answer = RequestError::NoAnswer { exit: None };
        let mut child = Command::new("/usr/bin/curl")
            .args(["--disable", "--config", "-", "--silent", "--show-error"])
            .args(SAFE_TRANSPORT)
            .args(["--connect-timeout", "10", "--max-time", "180", "--max-filesize"])
            .arg(limit.to_string())
            .args(["--write-out", "\n%{http_code}"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| no_answer.clone())?;
        let Some(mut input) = child.stdin.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(no_answer);
        };
        if input.write_all(config.as_bytes()).is_err() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(no_answer);
        }
        drop(input);
        let Some(stdout) = child.stdout.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(no_answer);
        };
        // Drain concurrently: waiting for process exit before reading a long
        // transcript response would fill the pipe and stall the child.
        let reader = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            stdout
                .take(limit as u64 + 5)
                .read_to_end(&mut bytes)
                .map(|_| bytes)
        });
        let status = loop {
            if self.stopped(&cancel) {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader.join();
                return Err(RequestError::Cancelled);
            }
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => std::thread::sleep(Duration::from_millis(20)),
                Err(_) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = reader.join();
                    return Err(no_answer);
                }
            }
        };
        let output = reader
            .join()
            .map_err(|_| no_answer.clone())?
            .map_err(|_| no_answer.clone())?;
        if !status.success() || output.len() > limit + 4 {
            return Err(RequestError::NoAnswer {
                exit: status.code(),
            });
        }
        let split = output
            .iter()
            .rposition(|b| *b == b'\n')
            .ok_or(RequestError::Malformed)?;
        let code: u16 = std::str::from_utf8(&output[split + 1..])
            .ok()
            .and_then(|code| code.parse().ok())
            .ok_or(RequestError::Malformed)?;
        if !(200..300).contains(&code) {
            if matches!(code, 401 | 403)
                && let Some(suspect) = &self.suspect
            {
                // Nothing changes here. The account's own `/session` is
                // asked, once, and only that answer counts.
                suspect.store(true, Ordering::Release);
            }
            let error = serde_json::from_slice::<Value>(&output[..split]).ok();
            let text = |key: &str| {
                error
                    .as_ref()
                    .and_then(|e| e.get(key))
                    .and_then(Value::as_str)
                    .filter(|s| s.len() <= 500)
                    .map(str::to_owned)
            };
            return Err(RequestError::Status {
                code,
                error: text("error"),
                message: text("message"),
            });
        }
        if self.stale() {
            return Err(RequestError::Cancelled);
        }
        serde_json::from_slice(&output[..split]).map_err(|_| RequestError::Malformed)
    }

    /// Read something from this account's server with its session, without
    /// decoding it: a note a link named, a shared note.
    pub(crate) fn fetch(&self, path: &str, limit: usize) -> Result<crate::fetch::Reply, crate::fetch::Failure> {
        if check_path(path).is_err() || self.stale() {
            return Err(crate::fetch::Failure::Transport);
        }
        let config = format!(
            "url = {}\nrequest = \"GET\"\nheader = \"Accept: application/json\"\nheader = {}\n",
            quote(&format!("{}{path}", self.url)),
            quote(&authorization(&self.token))
        );
        let reply = crate::fetch::exchange(&config, limit)?;
        if matches!(reply.status, 401 | 403)
            && let Some(suspect) = &self.suspect
        {
            suspect.store(true, Ordering::Release);
        }
        Ok(reply)
    }

    /// Start the chat stream's own transport: independent of the serial
    /// worker, cancellable by killing the child. The token travels through
    /// stdin.
    pub(crate) fn chat_stream(&self, path: &str, payload: &Value) -> Result<Child, String> {
        self.event_stream(path, payload, false)
    }

    /// [`Self::chat_stream`] whose child also writes the response's HTTP
    /// status (`000` when there was no response) as the last thing on its
    /// stderr, so a caller can tell an endpoint the server does not have from
    /// any other failure.
    pub(crate) fn event_stream_with_status(&self, path: &str, payload: &Value) -> Result<Child, String> {
        self.event_stream(path, payload, true)
    }

    fn event_stream(&self, path: &str, payload: &Value, status: bool) -> Result<Child, String> {
        check_path(path).map_err(|error| error.message())?;
        if self.stale() {
            return Err("Request cancelled".into());
        }
        let config = format!(
            "url = {}\nrequest = \"POST\"\nheader = {}\nheader = \"Content-Type: application/json\"\nheader = \"Accept: text/event-stream\"\ndata-binary = {}\n",
            quote(&format!("{}{path}", self.url)),
            quote(&authorization(&self.token)),
            quote(&payload.to_string())
        );
        let mut child = Command::new("/usr/bin/curl")
            .args(["--disable", "--config", "-", "--silent", "--show-error", "--fail", "--no-buffer"])
            .args(SAFE_TRANSPORT)
            .args(["--connect-timeout", "10", "--max-time", "180"])
            .args(if status { &["--write-out", "%{stderr}%{http_code}"][..] } else { &[][..] })
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(if status { Stdio::piped() } else { Stdio::null() })
            .spawn()
            .map_err(|_| "Cannot start chat transport")?;
        let Some(mut input) = child.stdin.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Chat transport input unavailable".into());
        };
        if input.write_all(config.as_bytes()).is_err() {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Cannot send chat request".into());
        }
        Ok(child)
    }

    /// Where the live transcript stream of one of this account's notes
    /// connects, and the session it connects with. `https://host` becomes
    /// `wss://host/api/v1/notes/<id>/live`; loopback development servers
    /// stay unencrypted, as they are for every other request.
    pub(crate) fn live(&self, note_id: &str) -> LiveEndpoint {
        LiveEndpoint {
            url: live_url(&self.url, note_id),
            token: self.token.clone(),
        }
    }

    /// End this session on its server. Best effort, and the last thing this
    /// client does: it is sent even though the account's epoch has moved.
    pub(crate) fn sign_out_on_server(&self) {
        let _ = sign_in_exchange(&self.url, "/api/auth/sign-out", Some(serde_json::json!({})), Some(&self.token));
    }
}

pub(crate) fn live_url(base: &str, note_id: &str) -> String {
    format!("ws{}/api/v1/notes/{note_id}/live", base.trim_start_matches("http"))
}

/// The address of a note's live endpoint and the session for it, made
/// together by one client.
#[derive(Clone)]
pub struct LiveEndpoint {
    pub url: String,
    token: Secret,
}

impl std::fmt::Debug for LiveEndpoint {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("LiveEndpoint").field("url", &self.url).finish_non_exhaustive()
    }
}

impl LiveEndpoint {
    /// Put the session on the handshake request.
    pub(crate) fn authorize(
        &self,
        headers: &mut tungstenite::http::HeaderMap,
    ) -> Result<(), ()> {
        let mut bearer =
            tungstenite::http::HeaderValue::from_str(&format!("Bearer {}", self.token.expose()))
                .map_err(|_| ())?;
        bearer.set_sensitive(true);
        headers.insert(tungstenite::http::header::AUTHORIZATION, bearer);
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn for_test(url: impl Into<String>, token: &str) -> Self {
        Self {
            url: url.into(),
            token: Secret::new(token),
        }
    }
}

/// How a sign-in exchange ended when it did not succeed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SignInFailure {
    /// No HTTP answer.
    Unreachable(&'static str),
    /// An answer that is not a success, with its body when it is JSON.
    Status { code: u16, body: Option<Value> },
    Invalid(&'static str),
}

impl SignInFailure {
    pub fn message(&self) -> String {
        match self {
            Self::Unreachable(message) | Self::Invalid(message) => (*message).into(),
            Self::Status { code: 401 | 403, .. } => {
                "Email, password or session was not accepted".into()
            }
            Self::Status { .. } => "The server could not finish signing in".into(),
        }
    }
}

/// The sign-in transport: one request to `url` + `path`, with a session only
/// when one is given, and the `set-auth-token` header of the answer.
pub(crate) fn sign_in_exchange(
    url: &str,
    path: &str,
    body: Option<Value>,
    token: Option<&Secret>,
) -> Result<(Value, Option<String>), SignInFailure> {
    if check_path(path).is_err() {
        return Err(SignInFailure::Invalid("Sign-in request could not be sent"));
    }
    let mut config = format!(
        "url = {}\nrequest = {}\nheader = \"Content-Type: application/json\"\n",
        quote(&format!("{url}{path}")),
        quote(if body.is_some() { "POST" } else { "GET" })
    );
    if let Some(token) = token.filter(|token| !token.is_empty()) {
        config.push_str(&format!("header = {}\n", quote(&authorization(token))));
    }
    if let Some(body) = body {
        config.push_str(&format!("data-binary = {}\n", quote(&body.to_string())));
    }
    let mut child = Command::new("/usr/bin/curl")
        .args(["--disable", "--silent", "--show-error", "--max-time", "25"])
        .args(SAFE_TRANSPORT)
        .args(["--dump-header", "-", "--write-out", "\n%{http_code}", "--config", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| SignInFailure::Unreachable("Sign-in transport could not start"))?;
    child
        .stdin
        .take()
        .ok_or(SignInFailure::Unreachable("Sign-in request could not be sent"))?
        .write_all(config.as_bytes())
        .map_err(|_| SignInFailure::Unreachable("Sign-in request could not be sent"))?;
    let mut bytes = Vec::new();
    child
        .stdout
        .take()
        .ok_or(SignInFailure::Unreachable("Sign-in response could not be read"))?
        .take(1_048_577)
        .read_to_end(&mut bytes)
        .map_err(|_| SignInFailure::Unreachable("Sign-in response could not be read"))?;
    if bytes.len() > 1_048_576 {
        let _ = child.kill();
        let _ = child.wait();
        return Err(SignInFailure::Invalid("Sign-in response was too large"));
    }
    if !child
        .wait()
        .map_err(|_| SignInFailure::Unreachable("Sign-in transport failed"))?
        .success()
    {
        return Err(SignInFailure::Unreachable("Could not reach this server"));
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| SignInFailure::Invalid("Invalid sign-in response"))?;
    let (mut response, status) = text
        .rsplit_once('\n')
        .ok_or(SignInFailure::Invalid("Invalid sign-in response"))?;
    let code: u16 = status
        .parse()
        .map_err(|_| SignInFailure::Invalid("Invalid sign-in response"))?;
    let mut token = None;
    while response.starts_with("HTTP/") {
        let (headers, rest) = response
            .split_once("\r\n\r\n")
            .ok_or(SignInFailure::Invalid("Invalid sign-in headers"))?;
        for line in headers.lines() {
            if let Some((name, value)) = line.split_once(':')
                && name.eq_ignore_ascii_case("set-auth-token")
            {
                token = Some(value.trim().to_owned());
            }
        }
        response = rest;
    }
    if !(200..300).contains(&code) {
        return Err(SignInFailure::Status {
            code,
            body: serde_json::from_str(response).ok(),
        });
    }
    Ok((
        serde_json::from_str(response).map_err(|_| SignInFailure::Invalid("Invalid sign-in response"))?,
        token,
    ))
}

/// What asking a server whose session this is came to.
#[derive(Clone, Debug)]
pub enum SessionCheck {
    Valid(Box<AuthSessionDTO>),
    /// `GET /api/v1/session` itself answered 401 with the API's own JSON
    /// body. Nothing else means the session has ended.
    Ended,
    /// No answer, or an answer that says nothing about the session (an HTML
    /// page from a proxy or a captive portal, a 403, a 5xx).
    Unknown(String),
}

/// Ask `url` whose session `token` is.
pub(crate) fn session_check(url: &str, token: &Secret) -> SessionCheck {
    if !printable(token.expose()) {
        return SessionCheck::Unknown("Invalid session credential".into());
    }
    match sign_in_exchange(url, "/api/v1/session", None, Some(token)) {
        Ok((value, _)) => match serde_json::from_value::<AuthSessionDTO>(value) {
            Ok(session) if uuid::Uuid::parse_str(&session.user.id).is_ok() => {
                SessionCheck::Valid(Box::new(session))
            }
            Ok(_) => SessionCheck::Unknown("Invalid account identity".into()),
            Err(_) => SessionCheck::Unknown("The server session does not match this client".into()),
        },
        Err(SignInFailure::Status {
            code: 401,
            body: Some(body),
        }) if body.get("error").and_then(Value::as_str) == Some("unauthorized") => {
            SessionCheck::Ended
        }
        Err(failure) => SessionCheck::Unknown(failure.message()),
    }
}

/// A password sign-in: the token the server answers with.
pub(crate) fn password_sign_in(url: &str, email: &str, password: &str) -> Result<Secret, String> {
    let (_, token) = sign_in_exchange(
        url,
        "/api/auth/sign-in/email",
        Some(serde_json::json!({"email":email,"password":password,"rememberMe":true})),
        None,
    )
    .map_err(|failure| failure.message())?;
    token
        .filter(|s| s.len() < 8192 && printable(s))
        .map(Secret::new)
        .ok_or_else(|| "The server did not return a signed session".into())
}

/// The features a server says it has (`features` on its health answer). A
/// feature only ever adds a check on the server; none relaxes one made here.
pub(crate) fn server_features(url: &str) -> Vec<String> {
    let address = format!("{url}/api/v1/health");
    crate::fetch::Request {
        method: "GET",
        url: &address,
        body: None,
    }
    .send(65_536)
    .ok()
    .filter(|reply| reply.status == 200)
    .and_then(|reply| serde_json::from_slice::<Value>(&reply.body).ok())
    .and_then(|health| {
        Some(
            health
                .get("features")?
                .as_array()?
                .iter()
                .filter_map(Value::as_str)
                .filter(|feature| feature.len() <= 64)
                .take(32)
                .map(str::to_owned)
                .collect(),
        )
    })
    .unwrap_or_default()
}

#[cfg(test)]
pub(crate) mod testing {
    use std::{
        io::{Read, Write},
        net::{TcpListener, TcpStream},
        sync::{
            Arc, Mutex,
            atomic::{AtomicBool, AtomicUsize, Ordering},
        },
        time::Duration,
    };

    /// One request as a fake server saw it.
    #[derive(Clone, Debug)]
    pub struct Seen {
        pub method: String,
        pub path: String,
        pub headers: String,
        pub body: String,
    }

    impl Seen {
        pub fn bearer(&self) -> Option<String> {
            self.headers.lines().find_map(|line| {
                line.strip_prefix("Authorization: Bearer ")
                    .or_else(|| line.strip_prefix("authorization: Bearer "))
                    .map(|token| token.trim().to_owned())
            })
        }
    }

    /// A small HTTP server on a loopback port: every request is recorded and
    /// answered by `answer` (status, content type, body, extra headers).
    pub struct Fake {
        pub url: String,
        pub seen: Arc<Mutex<Vec<Seen>>>,
        pub connections: Arc<AtomicUsize>,
        stop: Arc<AtomicBool>,
        address: std::net::SocketAddr,
    }

    pub type Answer = (u16, &'static str, String, Vec<String>);

    pub fn json(status: u16, body: impl Into<String>) -> Answer {
        (status, "application/json", body.into(), Vec::new())
    }

    fn read_request(stream: &mut TcpStream) -> Option<Seen> {
        stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
        let mut head = Vec::new();
        while !head.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            stream.read_exact(&mut byte).ok()?;
            head.push(byte[0]);
            if head.len() > 65_536 {
                return None;
            }
        }
        let head = String::from_utf8(head).ok()?;
        let mut lines = head.lines();
        let mut first = lines.next()?.split(' ');
        let (method, path) = (first.next()?.to_owned(), first.next()?.to_owned());
        let length = head
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())?
            })
            .unwrap_or(0);
        let mut body = vec![0; length];
        stream.read_exact(&mut body).ok()?;
        Some(Seen {
            method,
            path,
            headers: head,
            body: String::from_utf8_lossy(&body).into_owned(),
        })
    }

    impl Fake {
        pub fn start(answer: impl Fn(&Seen) -> Answer + Send + Sync + 'static) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let seen = Arc::new(Mutex::new(Vec::new()));
            let connections = Arc::new(AtomicUsize::new(0));
            let stop = Arc::new(AtomicBool::new(false));
            let (log, count, stopping) = (seen.clone(), connections.clone(), stop.clone());
            let answer = Arc::new(answer);
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    if stopping.load(Ordering::Acquire) {
                        break;
                    }
                    let Ok(mut stream) = stream else { continue };
                    count.fetch_add(1, Ordering::AcqRel);
                    let (log, answer) = (log.clone(), answer.clone());
                    std::thread::spawn(move || {
                        let Some(request) = read_request(&mut stream) else {
                            return;
                        };
                        let (status, kind, body, extra) = answer(&request);
                        log.lock().unwrap().push(request);
                        let mut head = format!(
                            "HTTP/1.1 {status} Answer\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n",
                            body.len()
                        );
                        for line in extra {
                            head.push_str(&line);
                            head.push_str("\r\n");
                        }
                        let _ = write!(stream, "{head}\r\n{body}");
                    });
                }
            });
            Self {
                url: format!("http://{address}"),
                seen,
                connections,
                stop,
                address,
            }
        }

        pub fn requests(&self) -> Vec<Seen> {
            self.seen.lock().unwrap().clone()
        }

        pub fn paths(&self) -> Vec<String> {
            self.requests()
                .into_iter()
                .map(|seen| format!("{} {}", seen.method, seen.path))
                .collect()
        }
    }

    impl Drop for Fake {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Release);
            let _ = TcpStream::connect(self.address);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{testing::*, *};

    const TOKEN: &str = "fictional-account-client-token-0000000";

    #[test]
    fn t1_paths_that_could_reach_another_server_are_refused_before_a_process_starts() {
        let server = Fake::start(|_| json(200, "{}"));
        let client = AccountClient::for_test(&server.url, TOKEN, None);
        for path in [
            "//evil.example/api/v1/notes",
            "/api/v1//evil.example",
            "/api/v1/notes@evil.example",
            "/api/v1/../admin",
            "/api/v1/notes/..",
            "/api/v1\\notes",
            "/api/v1/notes/{a,b}",
            "/api/v1/notes/[1-3]",
            "https://evil.example/api/v1/notes",
            "/api/v1/notes?next=https://evil.example",
            "/api/v1/notes\nheader = \"X-Injected: 1\"",
            "/api/v1/notes\r\n",
            "/api/v1/notes x",
            "/admin",
            "api/v1/notes",
            "",
        ] {
            assert!(matches!(check_path(path), Err(RequestError::Refused(_))), "{path:?}");
            assert!(
                matches!(
                    client.send::<Value>("GET", path, None, None),
                    Err(RequestError::Refused(_))
                ),
                "{path:?}"
            );
            assert!(client.fetch(path, 1024).is_err(), "{path:?}");
            assert!(client.chat_stream(path, &serde_json::json!({})).is_err(), "{path:?}");
            assert!(sign_in_exchange(&server.url, path, None, Some(&Secret::new(TOKEN))).is_err());
        }
        // Nothing was started: the server never saw a connection.
        assert_eq!(server.connections.load(Ordering::Acquire), 0);
        // What the app really asks for passes.
        for path in [
            "/api/v1/notes",
            "/api/v1/notes/summaries?limit=100&cursor=abc-DEF_1",
            "/api/v1/calendar/events?from=2026-11-12T05:00:00Z&to=2026-12-12T05:00:00Z",
            "/api/v1/notes/3a49a4af-c660-42e9-a36a-a806fbb41e0d/audio/chunks/x",
            "/api/auth/sign-out",
        ] {
            assert_eq!(check_path(path), Ok(()), "{path}");
        }
        // Ids are checked as UUIDs in one place, and written the one way.
        assert_eq!(
            path_id("3A49A4AF-C660-42E9-A36A-A806FBB41E0D").unwrap(),
            "3a49a4af-c660-42e9-a36a-a806fbb41e0d"
        );
        assert!(path_id("../admin").is_err());
        assert!(path_id("3a49a4af/../x").is_err());
    }

    #[test]
    fn t2_a_redirect_to_a_second_origin_is_never_followed_on_any_transport() {
        // The second origin: it must never see a connection, let alone a
        // session.
        let second = Fake::start(|_| json(200, "{\"stolen\":true}"));
        let target = second.url.clone();
        let first = Fake::start(move |request| {
            (
                302,
                "text/html",
                "<html>moved</html>".into(),
                vec![format!("Location: {target}{}", request.path)],
            )
        });
        let client = AccountClient::for_test(&first.url, TOKEN, None);
        // Main transport.
        let answer = client.send::<Value>("GET", "/api/v1/notes", None, None);
        assert!(
            matches!(answer, Err(RequestError::Status { code: 302, .. })),
            "{answer:?}"
        );
        let posted = client.send::<Value>("POST", "/api/v1/notes", Some(serde_json::json!({"title":"t"})), None);
        assert!(posted.is_err());
        // The authenticated fetch (a note or shared note a link named).
        let fetched = client.fetch("/api/v1/shared/token", 4096).unwrap();
        assert_eq!(fetched.status, 302);
        // The chat stream, which has its own curl invocation.
        let mut child = client
            .chat_stream("/api/v1/chat/stream", &serde_json::json!({"message":"hi"}))
            .unwrap();
        let mut streamed = String::new();
        child.stdout.take().unwrap().read_to_string(&mut streamed).unwrap();
        let _ = child.wait();
        // What came back is the first origin's own answer, never the
        // second's (the reader then refuses it as no chat stream at all).
        assert!(!streamed.contains("stolen"));
        assert!(streamed.is_empty() || streamed.contains("moved"));
        // Sign-in and the session check.
        assert!(sign_in_exchange(&first.url, "/api/v1/session", None, Some(&Secret::new(TOKEN))).is_err());
        assert!(matches!(
            session_check(&first.url, &Secret::new(TOKEN)),
            SessionCheck::Unknown(_)
        ));
        assert!(password_sign_in(&first.url, "a@b.example", "password").is_err());
        // The unauthenticated fetch and the download.
        let address = format!("{}/api/v1/auth/methods", first.url);
        let reply = crate::fetch::Request {
            method: "GET",
            url: &address,
            body: None,
        }
        .send(4096)
        .unwrap();
        assert_eq!(reply.status, 302);
        // The first origin saw every one of those, with the session where
        // one was sent; the second saw nothing at all.
        assert!(first.requests().len() >= 7);
        assert!(first.requests().iter().any(|seen| seen.bearer().as_deref() == Some(TOKEN)));
        assert_eq!(second.connections.load(Ordering::Acquire), 0);
        assert!(second.requests().is_empty());
    }

    #[test]
    fn every_curl_invocation_refuses_redirects_other_protocols_and_globbing() {
        assert_eq!(
            SAFE_TRANSPORT,
            ["--max-redirs", "0", "--proto", "=https,http", "--globoff"]
        );
        // A brace or bracket that did reach curl would be sent as written,
        // not expanded into several requests.
        let server = Fake::start(|_| json(200, "{}"));
        let address = format!("{}/api/v1/auth/methods?x={{a,b}}", server.url);
        let _ = crate::fetch::Request {
            method: "GET",
            url: &address,
            body: None,
        }
        .send(4096);
        assert_eq!(server.requests().len(), 1);
    }

    #[test]
    fn s1_only_a_json_401_from_session_itself_means_the_session_has_ended() {
        let token = Secret::new(TOKEN);
        // A 403 HTML page from a proxy or captive portal.
        let portal = Fake::start(|_| (403, "text/html", "<html>Sign in to the network</html>".into(), vec![]));
        assert!(matches!(session_check(&portal.url, &token), SessionCheck::Unknown(_)));
        // A 401 that is not the API's own answer.
        let html = Fake::start(|_| (401, "text/html", "<html>401</html>".into(), vec![]));
        assert!(matches!(session_check(&html.url, &token), SessionCheck::Unknown(_)));
        let other = Fake::start(|_| json(401, r#"{"error":"something_else"}"#));
        assert!(matches!(session_check(&other.url, &token), SessionCheck::Unknown(_)));
        let down = Fake::start(|_| json(503, r#"{"error":"unavailable"}"#));
        assert!(matches!(session_check(&down.url, &token), SessionCheck::Unknown(_)));
        // The API's own 401 from /session.
        let ended = Fake::start(|_| json(401, r#"{"error":"unauthorized","message":"Authentication required"}"#));
        assert!(matches!(session_check(&ended.url, &token), SessionCheck::Ended));
        assert_eq!(ended.paths(), ["GET /api/v1/session"]);
        // A 401 from another endpoint changes nothing by itself: it only
        // asks for the /session check.
        let suspect = Arc::new(AtomicBool::new(false));
        let notes = Fake::start(|_| json(401, r#"{"error":"unauthorized","message":"Authentication required"}"#));
        let client = AccountClient::for_test(&notes.url, TOKEN, None).suspected_by(suspect.clone());
        let answer = client.send::<Value>("GET", "/api/v1/notes", None, None);
        assert_eq!(answer.unwrap_err().status(), Some(401));
        assert!(suspect.load(Ordering::Acquire));
        assert!(!client.stale(), "the client itself is not cancelled by a 401");
    }

    #[test]
    fn a_request_keeps_its_status_and_only_the_bounded_api_message() {
        let server = Fake::start(|request| match request.path.as_str() {
            "/api/v1/notes/missing" => json(404, r#"{"error":"not_found","message":"Note not found"}"#),
            "/api/v1/html" => (502, "text/html", "<html>Bad gateway</html>".into(), vec![]),
            "/api/v1/garbage" => json(200, "not json"),
            _ => json(200, r#"{"ok":true}"#),
        });
        let client = AccountClient::for_test(&server.url, TOKEN, None);
        let missing = client.send::<Value>("GET", "/api/v1/notes/missing", None, None).unwrap_err();
        assert!(missing.is_not_found());
        assert_eq!(missing.message(), "Note not found");
        let html = client.send::<Value>("GET", "/api/v1/html", None, None).unwrap_err();
        assert_eq!(html.message(), "Server rejected the request (HTTP 502)");
        assert_eq!(
            client.send::<Value>("GET", "/api/v1/garbage", None, None),
            Err(RequestError::Malformed)
        );
        let ok: Value = client.request("GET", "/api/v1/ok", None).unwrap();
        assert_eq!(ok["ok"], true);
        // A large body travels from a private file.
        let directory = std::env::temp_dir().join(format!("cereal-large-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let file = directory.join("body.json");
        std::fs::write(&file, br#"{"title":"from a file"}"#).unwrap();
        let _: Value = client
            .request_large("POST", "/api/v1/notes/import", &file, 32 * 1024 * 1024)
            .unwrap();
        let sent = server.requests().into_iter().last().unwrap();
        assert_eq!(sent.body, r#"{"title":"from a file"}"#);
        assert_eq!(sent.bearer().as_deref(), Some(TOKEN));
        let _ = std::fs::remove_dir_all(directory);
    }
}
