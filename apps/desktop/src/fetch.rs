//! One HTTP request that carries only what its caller hands it: no saved
//! session, no cookies, no cache, and no redirect is followed. The requests
//! that go to servers the app may not be signed in to (a shared note, a
//! server's sign-in methods, the exchange that ends a browser sign-in) are
//! made here, so what they send is decided in one place.
use serde_json::Value;
use std::{
    io::{Read, Write},
    process::{Command, Stdio},
};

#[derive(Debug, PartialEq, Eq)]
pub struct Reply {
    pub status: u16,
    pub body: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    /// The request could not be made at all.
    Transport,
    TimedOut,
    Unreachable,
    /// The answer is larger than the caller allows.
    TooLarge,
}

/// What is sent: the address and an optional JSON body, and never a
/// session. A request that carries one is made by the account's own client
/// (`accounts::AccountClient::fetch`), which builds the address from that
/// account's base, so a caller here cannot pair a session with a URL.
/// Kept apart from the sending so it can be checked in tests.
#[derive(Debug, PartialEq, Eq)]
pub struct Request<'a> {
    pub method: &'a str,
    pub url: &'a str,
    pub body: Option<&'a Value>,
}

impl Request<'_> {
    /// curl reads the request over stdin, never argv or a temporary file.
    /// Config escaping is separate from shell escaping; no shell is involved.
    fn config(&self) -> String {
        fn quote(text: &str) -> String {
            format!(
                "\"{}\"",
                text.replace('\\', "\\\\")
                    .replace('"', "\\\"")
                    .replace('\n', "\\n")
                    .replace('\r', "\\r")
            )
        }
        let mut config = format!(
            "url = {}\nrequest = {}\nheader = \"Accept: application/json\"\n",
            quote(self.url),
            quote(self.method)
        );
        if let Some(body) = self.body {
            config.push_str("header = \"Content-Type: application/json\"\n");
            config.push_str(&format!("data-binary = {}\n", quote(&body.to_string())));
        }
        config
    }

    /// Send it, accepting at most `limit` bytes of answer.
    pub fn send(&self, limit: usize) -> Result<Reply, Failure> {
        exchange(&self.config(), limit)
    }
}

/// The bytes at `url`, for something that is drawn rather than read (a
/// person's picture): a plain GET that says nothing about who asks. No
/// session, no cookie, no referrer and no redirect; at most `limit` bytes.
/// The caller decides which addresses may be asked at all.
pub fn bytes(url: &str, limit: usize) -> Result<Reply, Failure> {
    let quoted = url.replace('\\', "\\\\").replace('"', "\\\"");
    if url.chars().any(char::is_control) {
        return Err(Failure::Transport);
    }
    exchange(&format!("url = \"{quoted}\"\nrequest = \"GET\"\n"), limit)
}

/// One curl exchange for a request written as a curl config: no redirect is
/// followed, nothing but HTTP(S) is spoken, and nothing in the address is
/// expanded. The config travels over stdin, never argv or a file.
pub(crate) fn exchange(config: &str, limit: usize) -> Result<Reply, Failure> {
    let mut child = Command::new("/usr/bin/curl")
        .args(["--disable", "--config", "-", "--silent"])
        .args(["--connect-timeout", "10", "--max-time", "25"])
        .args(crate::accounts::client::SAFE_TRANSPORT)
        .arg("--max-filesize")
        .arg(limit.to_string())
        .args(["--write-out", "\n%{http_code}"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| Failure::Transport)?;
    let sent = child
        .stdin
        .take()
        .ok_or(Failure::Transport)
        .and_then(|mut input| {
            input
                .write_all(config.as_bytes())
                .map_err(|_| Failure::Transport)
        });
    let Some(output) = child.stdout.take().filter(|_| sent.is_ok()) else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(Failure::Transport);
    };
    // The answer, a line break and three digits of status. Anything past
    // that is more than was asked for, however the server sent it.
    let room = limit + 4;
    let mut bytes = Vec::new();
    let read = output.take(room as u64 + 1).read_to_end(&mut bytes);
    if read.is_err() || bytes.len() > room {
        let _ = child.kill();
        let _ = child.wait();
        return Err(if read.is_err() {
            Failure::Transport
        } else {
            Failure::TooLarge
        });
    }
    let status = child.wait().map_err(|_| Failure::Transport)?;
    match status.code() {
        Some(0) => {}
        Some(63) => return Err(Failure::TooLarge),
        Some(28) => return Err(Failure::TimedOut),
        _ => return Err(Failure::Unreachable),
    }
    split_reply(bytes).ok_or(Failure::Transport)
}

/// Download `url` into the file at `path`, replacing it, with no credentials
/// and no redirect followed. `received` is kept at the number of bytes on
/// disk so a caller can show progress. More than `limit` bytes ends the
/// download, whether or not the server said how much it would send; so does
/// a transfer that stalls for half a minute. Returns the HTTP status.
pub fn download(
    url: &str,
    path: &std::path::Path,
    limit: u64,
    received: &std::sync::atomic::AtomicU64,
) -> Result<u16, Failure> {
    use std::sync::atomic::Ordering;
    let _ = std::fs::remove_file(path);
    let target = path.to_str().ok_or(Failure::Transport)?;
    let quote = |text: &str| {
        format!(
            "\"{}\"",
            text.replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\n', "\\n")
                .replace('\r', "\\r")
        )
    };
    let config = format!("url = {}\noutput = {}\n", quote(url), quote(target));
    let mut child = Command::new("/usr/bin/curl")
        .args([
            "--disable",
            "--config",
            "-",
            "--silent",
            "--connect-timeout",
            "10",
            "--speed-limit",
            "1",
            "--speed-time",
            "30",
            "--max-redirs",
            "0",
            "--proto",
            "=https,http",
            "--globoff",
            "--max-filesize",
            &limit.to_string(),
            "--write-out",
            "%{http_code}",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| Failure::Transport)?;
    let sent = child
        .stdin
        .take()
        .ok_or(Failure::Transport)
        .and_then(|mut input| {
            input
                .write_all(config.as_bytes())
                .map_err(|_| Failure::Transport)
        });
    if sent.is_err() {
        let _ = child.kill();
        let _ = child.wait();
        return Err(Failure::Transport);
    }
    let status = loop {
        let size = std::fs::metadata(path).map(|file| file.len()).unwrap_or(0);
        received.store(size, Ordering::Release);
        if size > limit {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Failure::TooLarge);
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(50)),
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Failure::Transport);
            }
        }
    };
    let size = std::fs::metadata(path).map(|file| file.len()).unwrap_or(0);
    received.store(size, Ordering::Release);
    match status.code() {
        Some(0) => {}
        Some(63) => return Err(Failure::TooLarge),
        Some(28) => return Err(Failure::TimedOut),
        _ => return Err(Failure::Unreachable),
    }
    if size > limit {
        return Err(Failure::TooLarge);
    }
    let mut code = String::new();
    child
        .stdout
        .take()
        .ok_or(Failure::Transport)?
        .take(16)
        .read_to_string(&mut code)
        .map_err(|_| Failure::Transport)?;
    code.trim().parse().map_err(|_| Failure::Transport)
}

/// The body and the status curl wrote after it.
fn split_reply(mut bytes: Vec<u8>) -> Option<Reply> {
    let split = bytes.iter().rposition(|byte| *byte == b'\n')?;
    let status = std::str::from_utf8(&bytes[split + 1..]).ok()?.parse().ok()?;
    bytes.truncate(split);
    Some(Reply {
        status,
        body: bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_made_here_never_carries_a_session() {
        let anonymous = Request {
            method: "GET",
            url: "https://notes.example.com/api/v1/auth/methods",
            body: None,
        }
        .config();
        assert!(!anonymous.to_lowercase().contains("authorization"));
        assert!(!anonymous.to_lowercase().contains("cookie"));
        assert!(anonymous.contains("url = \"https://notes.example.com/api/v1/auth/methods\""));
        let body = serde_json::json!({"code": "c", "verifier": "v"});
        let exchange = Request {
            method: "POST",
            url: "https://notes.example.com/api/v1/auth/app-handoff/exchange",
            body: Some(&body),
        }
        .config();
        assert!(!exchange.to_lowercase().contains("authorization"));
        assert!(exchange.contains("request = \"POST\""));
        assert!(exchange.contains(r#"data-binary = "{\"code\":\"c\",\"verifier\":\"v\"}""#));
    }

    #[test]
    fn config_values_cannot_start_a_new_directive() {
        let config = Request {
            method: "GET",
            url: "https://notes.example.com/\"\nheader = \"X-Injected: 1",
            body: None,
        }
        .config();
        // Every line is still one of the directives this module writes.
        for line in config.lines() {
            assert!(
                ["url = \"", "request = \"", "header = \""]
                    .iter()
                    .any(|start| line.starts_with(start)),
                "{line}"
            );
        }
        assert_eq!(config.lines().count(), 3);
        assert_eq!(config.matches("url = ").count(), 1);
        assert!(config.contains("\\nheader = \\\"X-Injected: 1"));
    }

    #[test]
    fn the_status_is_read_from_the_last_line_whatever_the_body_holds() {
        assert_eq!(
            split_reply(b"{\"title\":\"a\\nb\"}\n200".to_vec()),
            Some(Reply {
                status: 200,
                body: b"{\"title\":\"a\\nb\"}".to_vec()
            })
        );
        assert_eq!(
            split_reply(b"line one\nline two\n404".to_vec()),
            Some(Reply {
                status: 404,
                body: b"line one\nline two".to_vec()
            })
        );
        assert_eq!(
            split_reply(b"\n000".to_vec()),
            Some(Reply {
                status: 0,
                body: Vec::new()
            })
        );
        assert_eq!(split_reply(b"200".to_vec()), None);
        assert_eq!(split_reply(b"body\nabc".to_vec()), None);
        assert_eq!(split_reply(Vec::new()), None);
    }
}
