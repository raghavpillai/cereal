//! Notes other people shared: reading one snapshot, and "Shared with me",
//! the list of the ones opened on this Mac. A shared note is read with an
//! account on its own server, so the session goes to that server and to no
//! other; for a note anywhere else nothing is fetched at all.
use crate::{
    account::Credentials,
    fetch::{Failure, Reply, Request},
    link::{AppLink, SharedNoteLink},
};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// A snapshot larger than this is refused, by its declared length or as it
/// arrives.
pub const MAXIMUM_BYTES: usize = 2 * 1024 * 1024;

/// A shared snapshot as its server sent it. Every field is text someone else
/// wrote: it is shown, never run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SharedNote {
    pub title: String,
    /// The generated notes, as Markdown.
    pub enhanced: String,
    /// The sharer's own notes.
    pub body: String,
    /// The meeting's day, when the server sent one.
    pub date: Option<NaiveDate>,
}

impl SharedNote {
    /// The title as the list and the page show it.
    pub fn display_title(&self) -> String {
        if self.title.trim().is_empty() {
            "Untitled note".into()
        } else {
            self.title.clone()
        }
    }
}

/// As much of a document as is laid out on screen, and whether that is all
/// of it. A snapshot may hold two megabytes of one-character lines; the page
/// shows the beginning and leaves the rest to the browser. A real note is
/// far inside both limits.
pub fn shown(text: &str) -> (&str, bool) {
    const CHARACTERS: usize = 60_000;
    const LINES: usize = 800;
    let mut end = text.len();
    if let Some((index, _)) = text.char_indices().nth(CHARACTERS) {
        end = index;
    }
    if let Some((index, _)) = text[..end].match_indices('\n').nth(LINES - 1) {
        end = index;
    }
    (&text[..end], end == text.len())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SharedNoteError {
    /// The app is not signed in to the note's server, or its session there
    /// is no longer accepted.
    SignInRequired,
    /// The server has no such share: it never existed, was turned off, or
    /// has expired.
    Unavailable,
    TooLarge,
    InvalidResponse,
    Server(u16),
    TimedOut,
    Unreachable,
}

impl SharedNoteError {
    /// Worth asking again. A share that is gone stays gone.
    pub fn is_retryable(self) -> bool {
        !matches!(self, Self::Unavailable | Self::TooLarge | Self::SignInRequired)
    }

    pub fn message(self) -> String {
        match self {
            Self::SignInRequired => "Sign in to this note's server to view it.".into(),
            Self::Unavailable => {
                "This shared note is not available. The link may have expired or been turned off."
                    .into()
            }
            Self::TooLarge => "This shared note is too large to open here.".into(),
            Self::InvalidResponse => "The server did not send a shared note.".into(),
            Self::Server(status) => format!("The server could not send this note (HTTP {status})."),
            Self::TimedOut => "The server took too long to respond. Try again.".into(),
            Self::Unreachable => "The server could not be reached.".into(),
        }
    }
}

/// The session to send with a shared note's request: the one for the server
/// the link is on, and only that one. `None` means nothing may be asked.
pub fn bearer_for<'a>(link: &SharedNoteLink, credentials: Option<&'a Credentials>) -> Option<&'a str> {
    credentials
        .filter(|credentials| !credentials.token.is_empty() && link.origin.is_server(&credentials.url))
        .map(|credentials| credentials.token.as_str())
}

/// Read a shared snapshot with the session of the server it is on. Without
/// such a session no request is made.
pub fn fetch(link: &SharedNoteLink, credentials: Option<&Credentials>) -> Result<SharedNote, SharedNoteError> {
    let Some(bearer) = bearer_for(link, credentials) else {
        return Err(SharedNoteError::SignInRequired);
    };
    let url = link.api_url();
    let reply = Request {
        method: "GET",
        url: &url,
        bearer: Some(bearer),
        body: None,
    }
    .send(MAXIMUM_BYTES)
    .map_err(|failure| match failure {
        Failure::TooLarge => SharedNoteError::TooLarge,
        Failure::TimedOut => SharedNoteError::TimedOut,
        Failure::Unreachable | Failure::Transport => SharedNoteError::Unreachable,
    })?;
    read(&reply)
}

/// What the server's answer means.
pub fn read(reply: &Reply) -> Result<SharedNote, SharedNoteError> {
    /// Only the fields this app shows. A server of another version may send
    /// more, or fewer.
    #[derive(Deserialize)]
    struct Wire {
        title: Option<String>,
        body: Option<String>,
        enhanced: Option<String>,
        date: Option<String>,
    }
    match reply.status {
        200 => {}
        401 | 403 => return Err(SharedNoteError::SignInRequired),
        404 | 410 => return Err(SharedNoteError::Unavailable),
        // No redirect is followed, so one arrives here as an answer.
        300..=399 => return Err(SharedNoteError::InvalidResponse),
        status => return Err(SharedNoteError::Server(status)),
    }
    if reply.body.len() > MAXIMUM_BYTES {
        return Err(SharedNoteError::TooLarge);
    }
    let wire: Wire =
        serde_json::from_slice(&reply.body).map_err(|_| SharedNoteError::InvalidResponse)?;
    Ok(SharedNote {
        title: wire.title.unwrap_or_default().chars().take(500).collect(),
        enhanced: wire.enhanced.unwrap_or_default(),
        body: wire.body.unwrap_or_default(),
        // `YYYY-MM-DD`, as a day rather than an instant: it is the same day
        // in every time zone.
        date: wire
            .date
            .filter(|date| date.len() == 10)
            .and_then(|date| NaiveDate::parse_from_str(&date, "%Y-%m-%d").ok()),
    })
}

/// The day a shared note's chip shows, worded as a note's own chip words a
/// date. The year is given when it is not this one; a shared note can be an
/// old one.
pub fn day_label(date: NaiveDate, today: NaiveDate) -> String {
    use chrono::Datelike;
    if date.year() == today.year() || Some(date) == today.pred_opt() {
        crate::note::note_date_label(date, today)
    } else {
        date.format("%b %-d, %Y").to_string()
    }
}

/// A shared note the user has opened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Visit {
    pub link: SharedNoteLink,
    pub title: String,
    pub opened_at: DateTime<Utc>,
}

/// "Shared with me": the shared notes opened on this Mac, newest first.
/// Kept in this app's data directory and nowhere else, and the same whoever
/// is signed in. Opening a note again moves it to the top; the oldest are
/// dropped beyond [`History::LIMIT`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct History {
    visits: Vec<Visit>,
}

#[derive(Serialize, Deserialize)]
struct Saved {
    url: String,
    title: String,
    #[serde(rename = "openedAt")]
    opened_at: DateTime<Utc>,
}

impl History {
    pub const LIMIT: usize = 50;

    pub fn visits(&self) -> &[Visit] {
        &self.visits
    }

    /// What was saved is read as strictly as a link from outside.
    pub fn decode(bytes: &[u8]) -> Self {
        let saved: Vec<Saved> = serde_json::from_slice(bytes).unwrap_or_default();
        let mut visits: Vec<Visit> = Vec::new();
        for entry in saved {
            let Some(AppLink::Shared(link)) = AppLink::parse(&entry.url) else {
                continue;
            };
            if visits.iter().any(|visit| visit.link == link) {
                continue;
            }
            visits.push(Visit {
                link,
                title: entry.title.chars().take(500).collect(),
                opened_at: entry.opened_at,
            });
            if visits.len() == Self::LIMIT {
                break;
            }
        }
        Self { visits }
    }

    pub fn encode(&self) -> Vec<u8> {
        let saved: Vec<Saved> = self
            .visits
            .iter()
            .map(|visit| Saved {
                url: visit.link.web_url(),
                title: visit.title.clone(),
                opened_at: visit.opened_at,
            })
            .collect();
        serde_json::to_vec(&saved).unwrap_or_else(|_| b"[]".to_vec())
    }

    pub fn record(&mut self, link: &SharedNoteLink, title: &str, at: DateTime<Utc>) {
        self.visits.retain(|visit| visit.link != *link);
        self.visits.insert(
            0,
            Visit {
                link: link.clone(),
                title: title.chars().take(500).collect(),
                opened_at: at,
            },
        );
        self.visits.truncate(Self::LIMIT);
    }

    /// Returns whether anything was removed.
    pub fn remove(&mut self, link: &SharedNoteLink) -> bool {
        let before = self.visits.len();
        self.visits.retain(|visit| visit.link != *link);
        self.visits.len() != before
    }

    pub fn load() -> Self {
        history_path()
            .and_then(|path| std::fs::read(path).ok())
            .map(|bytes| Self::decode(&bytes))
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        let path = history_path().ok_or("Cannot find the app's data directory")?;
        write_private(&path, &self.encode())
    }
}

/// The app's data directory: the one `CEREAL_DATA_DIR` names, or the usual
/// one under Application Support.
fn history_path() -> Option<PathBuf> {
    let root = match crate::environment::var_os("CEREAL_DATA_DIR").filter(|v| !v.is_empty()) {
        Some(directory) => PathBuf::from(directory),
        None => PathBuf::from(std::env::var_os("HOME")?).join("Library/Application Support/Cereal"),
    };
    Some(root.join("shared-with-me.json"))
}

/// Replace `path` with `bytes`, readable by this user only.
pub fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::{io::Write, os::unix::fs::OpenOptionsExt};
    let failed = |_| "Cannot save to the app's data directory".to_string();
    let directory = path.parent().ok_or("Cannot find the app's data directory")?;
    std::fs::create_dir_all(directory).map_err(failed)?;
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    let written = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&temporary)
        .and_then(|mut file| file.write_all(bytes))
        .and_then(|()| std::fs::rename(&temporary, path));
    if written.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    written.map_err(failed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    const TOKEN: &str = "Zk3vX0b1aQ9mT7cR2dS5eU8fW4gY6hJ0iK1lL3nM5oP";

    fn link(text: &str) -> SharedNoteLink {
        match AppLink::parse(text) {
            Some(AppLink::Shared(link)) => link,
            other => panic!("not a shared link: {text} ({other:?})"),
        }
    }

    fn credentials(url: &str) -> Credentials {
        Credentials {
            url: url.into(),
            token: "session-token-for-the-signed-in-server".into(),
        }
    }

    fn reply(status: u16, body: &str) -> Reply {
        Reply {
            status,
            body: body.as_bytes().to_vec(),
        }
    }

    #[test]
    fn the_session_goes_to_the_server_the_app_is_signed_in_to_and_no_other() {
        let on_server = link(&format!("http://127.0.0.1:3000/s/{TOKEN}"));
        let signed_in = credentials("http://127.0.0.1:3000");
        assert_eq!(
            bearer_for(&on_server, Some(&signed_in)),
            Some("session-token-for-the-signed-in-server")
        );
        for elsewhere in [
            format!("http://localhost:3000/s/{TOKEN}"),
            format!("http://127.0.0.1:3001/s/{TOKEN}"),
            format!("https://127.0.0.1:3000/s/{TOKEN}"),
            format!("https://notes.example.com/s/{TOKEN}"),
            format!("https://127.0.0.1.evil.example/s/{TOKEN}"),
        ] {
            let elsewhere = link(&elsewhere);
            assert_eq!(bearer_for(&elsewhere, Some(&signed_in)), None, "{elsewhere:?}");
            // And so nothing is asked of that server: no request is made.
            assert_eq!(
                fetch(&elsewhere, Some(&signed_in)),
                Err(SharedNoteError::SignInRequired)
            );
        }
        // Signed out, or in local mode.
        assert_eq!(bearer_for(&on_server, None), None);
        assert_eq!(fetch(&on_server, None), Err(SharedNoteError::SignInRequired));
        let empty = Credentials {
            url: "http://127.0.0.1:3000".into(),
            token: String::new(),
        };
        assert_eq!(bearer_for(&on_server, Some(&empty)), None);
        // A server kept under a path is not the origin a link names.
        assert_eq!(
            bearer_for(&on_server, Some(&credentials("http://127.0.0.1:3000/base"))),
            None
        );
    }

    #[test]
    fn a_snapshot_is_read_leniently_and_never_trusted() {
        let note = read(&reply(
            200,
            r##"{"shareId":"x","title":"Weekly sync","body":"- mine","enhanced":"# Summary\n- one","date":"2026-11-12","duration":"42:10","sharedAt":"2026-11-12T10:00:00Z","expiresAt":null,"extra":{"a":1}}"##,
        ))
        .unwrap();
        assert_eq!(note.title, "Weekly sync");
        assert_eq!(note.enhanced, "# Summary\n- one");
        assert_eq!(note.body, "- mine");
        assert_eq!(note.date, NaiveDate::from_ymd_opt(2026, 11, 12));
        // Fields a server of another version leaves out are simply empty.
        let bare = read(&reply(200, "{}")).unwrap();
        assert_eq!(bare.display_title(), "Untitled note");
        assert_eq!((bare.enhanced.as_str(), bare.body.as_str(), bare.date), ("", "", None));
        for date in ["2026-13-01", "2026-11-12T10:00:00Z", "12/11/2026", "", "2026-1-2"] {
            let note = read(&reply(200, &format!(r#"{{"title":"t","date":"{date}"}}"#))).unwrap();
            assert_eq!(note.date, None, "{date}");
        }
        let long = read(&reply(200, &format!(r#"{{"title":"{}"}}"#, "é".repeat(900)))).unwrap();
        assert_eq!(long.title.chars().count(), 500);
        // HTML and scripts are just text.
        let markup = read(&reply(200, r#"{"title":"<script>alert(1)</script>","enhanced":"<img src=x onerror=alert(1)>"}"#)).unwrap();
        assert_eq!(markup.title, "<script>alert(1)</script>");
        for body in ["", "<html>", "[]", "null", r#"{"title":5}"#, r#"{"title":"t""#] {
            assert_eq!(read(&reply(200, body)), Err(SharedNoteError::InvalidResponse), "{body}");
        }
    }

    #[test]
    fn each_answer_from_the_server_has_its_own_meaning() {
        assert_eq!(read(&reply(404, r#"{"error":"not_found"}"#)), Err(SharedNoteError::Unavailable));
        assert_eq!(read(&reply(410, "")), Err(SharedNoteError::Unavailable));
        assert_eq!(read(&reply(401, r#"{"error":"unauthorized"}"#)), Err(SharedNoteError::SignInRequired));
        assert_eq!(read(&reply(403, "")), Err(SharedNoteError::SignInRequired));
        assert_eq!(read(&reply(302, "")), Err(SharedNoteError::InvalidResponse));
        assert_eq!(read(&reply(500, "boom")), Err(SharedNoteError::Server(500)));
        assert_eq!(read(&reply(0, "")), Err(SharedNoteError::Server(0)));
        let oversized = Reply {
            status: 200,
            body: vec![b' '; MAXIMUM_BYTES + 1],
        };
        assert_eq!(read(&oversized), Err(SharedNoteError::TooLarge));
        assert!(!SharedNoteError::Unavailable.is_retryable());
        assert!(!SharedNoteError::TooLarge.is_retryable());
        assert!(!SharedNoteError::SignInRequired.is_retryable());
        assert!(SharedNoteError::Unreachable.is_retryable());
        assert!(SharedNoteError::Server(502).is_retryable());
    }

    #[test]
    fn a_very_long_document_is_shown_from_its_beginning() {
        assert_eq!(shown("short\nnote"), ("short\nnote", true));
        assert_eq!(shown(""), ("", true));
        let lines = (1..=5000).map(|i| format!("- line {i}")).collect::<Vec<_>>().join("\n");
        let (text, whole) = shown(&lines);
        assert!(!whole);
        assert_eq!(text.lines().count(), 800);
        assert!(text.ends_with("- line 800"));
        let exact = (1..=800).map(|i| format!("- line {i}")).collect::<Vec<_>>().join("\n");
        assert_eq!(shown(&exact), (exact.as_str(), true));
        // Characters, not bytes: a cut never lands inside one.
        let wide = "é".repeat(70_000);
        let (text, whole) = shown(&wide);
        assert!(!whole);
        assert_eq!(text.chars().count(), 60_000);
    }

    #[test]
    fn a_shared_notes_day_names_its_year_when_it_is_not_this_one() {
        let day = |y, m, d| NaiveDate::from_ymd_opt(y, m, d).unwrap();
        let today = day(2026, 11, 15);
        assert_eq!(day_label(today, today), "Today");
        assert_eq!(day_label(day(2026, 11, 14), today), "Yesterday");
        assert_eq!(day_label(day(2026, 3, 2), today), "Mar 2");
        assert_eq!(day_label(day(2025, 3, 2), today), "Mar 2, 2025");
        assert_eq!(day_label(day(2025, 12, 31), day(2026, 1, 1)), "Yesterday");
    }

    #[test]
    fn history_keeps_the_newest_fifty_and_moves_a_reopened_note_to_the_top() {
        let at = |minute: u32| Utc.with_ymd_and_hms(2026, 11, 15, 9, minute % 60, 0).unwrap();
        let nth = |index: usize| link(&format!("https://notes.example.com/s/{index:0>20}"));
        let mut history = History::default();
        for index in 0..60 {
            history.record(&nth(index), &format!("Note {index}"), at(index as u32));
        }
        assert_eq!(history.visits().len(), History::LIMIT);
        assert_eq!(history.visits()[0].title, "Note 59");
        assert_eq!(history.visits()[49].title, "Note 10");
        // Opening one again moves it up instead of listing it twice.
        history.record(&nth(20), "Note 20, renamed", at(59));
        assert_eq!(history.visits().len(), History::LIMIT);
        assert_eq!(history.visits()[0].title, "Note 20, renamed");
        assert_eq!(history.visits().iter().filter(|visit| visit.link == nth(20)).count(), 1);
        assert!(history.remove(&nth(20)));
        assert!(!history.remove(&nth(20)));
        assert_eq!(history.visits().len(), History::LIMIT - 1);
        assert_eq!(History::decode(&history.encode()), history);
    }

    #[test]
    fn saved_history_is_read_as_strictly_as_a_link_from_outside() {
        let saved = format!(
            r#"[
              {{"url":"https://notes.example.com/s/{TOKEN}","title":"Kept","openedAt":"2026-11-15T09:00:00Z"}},
              {{"url":"https://notes.example.com/s/{TOKEN}","title":"The same link again","openedAt":"2026-11-14T09:00:00Z"}},
              {{"url":"http://evil.example/s/{TOKEN}","title":"Plain HTTP","openedAt":"2026-11-15T09:00:00Z"}},
              {{"url":"https://notes.example.com/notes/01136768-b41d-4ee8-ad19-e73976f18475","title":"Not a share","openedAt":"2026-11-15T09:00:00Z"}},
              {{"url":"cereal://auth?code=a&state=b","title":"Not a share","openedAt":"2026-11-15T09:00:00Z"}},
              {{"url":"javascript:alert(1)","title":"Not a link","openedAt":"2026-11-15T09:00:00Z"}}
            ]"#
        );
        let history = History::decode(saved.as_bytes());
        assert_eq!(history.visits().len(), 1);
        assert_eq!(history.visits()[0].title, "Kept");
        assert_eq!(history.visits()[0].link.origin.display_host(), "notes.example.com");
        // A damaged file is an empty list, not a crash.
        for damaged in ["", "{", "{}", "null", r#"[{"url":5}]"#] {
            assert!(History::decode(damaged.as_bytes()).visits().is_empty(), "{damaged}");
        }
        let many: Vec<String> = (0..80)
            .map(|index| format!(r#"{{"url":"https://notes.example.com/s/{index:0>20}","title":"{}","openedAt":"2026-11-15T09:00:00Z"}}"#, "t".repeat(900)))
            .collect();
        let history = History::decode(format!("[{}]", many.join(",")).as_bytes());
        assert_eq!(history.visits().len(), History::LIMIT);
        assert_eq!(history.visits()[0].title.chars().count(), 500);
    }

    #[test]
    fn history_is_written_whole_and_private() {
        use std::os::unix::fs::PermissionsExt;
        let directory = std::env::temp_dir().join(format!("cereal-shared-test-{}", uuid::Uuid::new_v4()));
        let path = directory.join("nested").join("shared-with-me.json");
        let mut history = History::default();
        history.record(
            &link(&format!("https://notes.example.com/s/{TOKEN}")),
            "Weekly sync",
            Utc.with_ymd_and_hms(2026, 11, 15, 9, 0, 0).unwrap(),
        );
        write_private(&path, &history.encode()).unwrap();
        write_private(&path, &history.encode()).unwrap();
        assert_eq!(History::decode(&std::fs::read(&path).unwrap()), history);
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        // No temporary file is left beside it.
        assert_eq!(std::fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
