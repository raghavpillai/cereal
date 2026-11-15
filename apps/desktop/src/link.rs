//! Every URL the app can be opened with, and every link a person can paste
//! into it. This is the only place such a URL is read: a web link
//! (`https://server/s/<token>`, `https://server/notes/<id>`) and its
//! `cereal://` form become the same value, and anything else is no link at
//! all. Nothing here touches the network, the window or the account.

/// The URL scheme the system hands to this app.
pub const SCHEME: &str = "cereal";
/// Longer than any real link; a longer one is refused unread.
const MAXIMUM_LENGTH: usize = 2048;

/// Where a Cereal server lives: scheme, host and port, with nothing after
/// them. HTTPS, or HTTP on this Mac's own loopback addresses for
/// development. The host is kept in its ASCII form, so a look-alike name is
/// shown as what it is.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ServerOrigin {
    secure: bool,
    /// Lower-case ASCII. An IPv6 address is kept without its brackets.
    host: String,
    /// `None` when it is the scheme's own port.
    port: Option<u16>,
}

impl ServerOrigin {
    /// A bare origin such as `https://notes.example.com` or
    /// `http://127.0.0.1:3000`.
    pub fn parse(text: &str) -> Option<Self> {
        if !is_plain(text) {
            return None;
        }
        let (origin, rest) = split_origin(text)?;
        (rest.is_empty() || rest == "/").then_some(origin)
    }

    /// The origin of a server address a person typed, whatever path follows it.
    pub fn of_server(url: &str) -> Option<Self> {
        if !is_plain(url) {
            return None;
        }
        let (origin, rest) = split_origin(url)?;
        (rest.is_empty() || rest.starts_with('/')).then_some(origin)
    }

    /// True when `url` (the address the app is connected to) is this server
    /// and nothing more: links name a server by its origin. Scheme, host and
    /// port must all be the same after normalising, so `localhost` and
    /// `127.0.0.1` are two different servers.
    pub fn is_server(&self, url: &str) -> bool {
        Self::parse(url).as_ref() == Some(self)
    }

    /// Host and, when it is not the usual one, port: what a person
    /// recognises a server by.
    pub fn display_host(&self) -> String {
        let host = if self.host.contains(':') {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        };
        match self.port {
            Some(port) => format!("{host}:{port}"),
            None => host,
        }
    }

    /// `scheme://host[:port]`, with no trailing slash.
    pub fn url(&self) -> String {
        format!(
            "{}://{}",
            if self.secure { "https" } else { "http" },
            self.display_host()
        )
    }
}

/// A read-only snapshot someone shared: `<origin>/s/<token>`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SharedNoteLink {
    pub origin: ServerOrigin,
    pub token: String,
}

impl SharedNoteLink {
    /// What people send each other, and what "Open in browser" opens.
    pub fn web_url(&self) -> String {
        format!("{}/s/{}", self.origin.url(), self.token)
    }

    /// Where the snapshot is read from.
    pub fn api_url(&self) -> String {
        format!("{}/api/v1/shared/{}", self.origin.url(), self.token)
    }

    /// The link a web page hands to the app.
    #[cfg(test)]
    pub fn app_url(&self) -> String {
        format!("{SCHEME}://shared?url={}", percent_encode(&self.web_url()))
    }
}

/// A note in somebody's own account: `<origin>/notes/<id>`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct NoteLink {
    pub origin: ServerOrigin,
    /// A lower-case UUID.
    pub note_id: String,
}

impl NoteLink {
    pub fn web_url(&self) -> String {
        format!("{}/notes/{}", self.origin.url(), self.note_id)
    }
}

/// What the browser hands back after a sign-in:
/// `cereal://auth?code=…&state=…&server=…`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthCallback {
    pub code: String,
    pub state: String,
    pub origin: Option<ServerOrigin>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppLink {
    Shared(SharedNoteLink),
    Note(NoteLink),
    Auth(AuthCallback),
}

impl AppLink {
    /// A link as text: one the system delivered, or one typed or pasted.
    /// Surrounding white space is ignored.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        if !is_plain(text) {
            return None;
        }
        let scheme = text.split_once(':')?.0.to_ascii_lowercase();
        match scheme.as_str() {
            SCHEME => app(text),
            "https" | "http" => web(text),
            _ => None,
        }
    }

    /// A link to a note, shared or one's own: what a person can paste.
    pub fn parse_note(text: &str) -> Option<Self> {
        Self::parse(text).filter(|link| !matches!(link, AppLink::Auth(_)))
    }

    /// The server a note link points at. A sign-in answer names none here:
    /// it is only ever read by the sign-in that is waiting for it.
    pub fn origin(&self) -> Option<&ServerOrigin> {
        match self {
            AppLink::Shared(link) => Some(&link.origin),
            AppLink::Note(link) => Some(&link.origin),
            AppLink::Auth(_) => None,
        }
    }

    /// The page a browser shows for this link.
    pub fn web_url(&self) -> Option<String> {
        match self {
            AppLink::Shared(link) => Some(link.web_url()),
            AppLink::Note(link) => Some(link.web_url()),
            AppLink::Auth(_) => None,
        }
    }
}

/// What a note's page may do with its link.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    /// The app is signed in to the link's own server: the note is read with
    /// that session, and with no other.
    Read,
    /// Nothing is fetched. `offered` is whether the page offers to sign in.
    SignIn { offered: bool },
}

/// The rule every link follows: a note, shared or one's own, opens only with
/// a session on its own server. `server` is the address the app is connected
/// to (`None` in local mode) and `signed_in` whether it holds a session
/// there. Signing in is offered where it does not replace an account in use:
/// when nobody is signed in, or the app is in local mode.
pub fn access(origin: &ServerOrigin, server: Option<&str>, signed_in: bool) -> Access {
    match server {
        Some(url) if signed_in && origin.is_server(url) => Access::Read,
        Some(_) => Access::SignIn {
            offered: !signed_in,
        },
        None => Access::SignIn { offered: true },
    }
}

/// True while a link has to wait: the app is connected to a server, does not
/// yet know whose session it holds, and is still asking. Once that question
/// is answered either way the link is followed.
pub fn must_wait(server_mode: bool, account_known: bool, checking_session: bool) -> bool {
    server_mode && !account_known && checking_session
}

/// Printable ASCII only. A backslash is read as a slash by some URL parsers
/// and not by others.
fn is_plain(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= MAXIMUM_LENGTH
        && text.bytes().all(|byte| byte > 0x20 && byte < 0x7f && byte != b'\\')
}

/// `scheme://authority` as an origin, and whatever follows the authority.
fn split_origin(text: &str) -> Option<(ServerOrigin, &str)> {
    let (scheme, rest) = text.split_once("://")?;
    let secure = match scheme.to_ascii_lowercase().as_str() {
        "https" => true,
        "http" => false,
        _ => return None,
    };
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, rest) = rest.split_at(end);
    // A user name or password in a link is never a server's address.
    if authority.contains('@') {
        return None;
    }
    let (host, port) = if let Some(inner) = authority.strip_prefix('[') {
        let (address, after) = inner.split_once(']')?;
        let port = if after.is_empty() {
            None
        } else {
            Some(after.strip_prefix(':')?)
        };
        let address = address.to_ascii_lowercase();
        if address.len() > 45
            || !address.contains(':')
            || !address
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() || byte == b':' || byte == b'.')
        {
            return None;
        }
        (address, port)
    } else {
        let (name, port) = match authority.split_once(':') {
            Some((name, port)) => (name, Some(port)),
            None => (authority, None),
        };
        let name = name.to_ascii_lowercase();
        if !is_host_name(&name) {
            return None;
        }
        (name, port)
    };
    let port = match port {
        None => None,
        Some(digits) => {
            if digits.is_empty() || digits.len() > 5 || !digits.bytes().all(|b| b.is_ascii_digit())
            {
                return None;
            }
            let number: u32 = digits.parse().ok()?;
            if !(1..=65_535).contains(&number) {
                return None;
            }
            Some(number as u16)
        }
    };
    if !secure && !matches!(host.as_str(), "localhost" | "127.0.0.1" | "::1") {
        return None;
    }
    let port = port.filter(|port| *port != if secure { 443 } else { 80 });
    Some((ServerOrigin { secure, host, port }, rest))
}

/// Dot-separated labels of letters, digits and inner hyphens.
fn is_host_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 253
        && name.split('.').all(|label| {
            (1..=63).contains(&label.len())
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        })
}

/// `https://<server>/s/<token>` or `https://<server>/notes/<id>`. The path is
/// matched whole and unescaped, so an encoded or dotted segment is not a
/// link. A query or fragment added along the way is dropped.
fn web(text: &str) -> Option<AppLink> {
    let (origin, rest) = split_origin(text)?;
    let path = &rest[..rest.find(['?', '#']).unwrap_or(rest.len())];
    if path.contains('%') {
        return None;
    }
    let mut segments = path.split('/');
    if segments.next() != Some("") {
        return None;
    }
    let (kind, value) = (segments.next()?, segments.next()?);
    if segments.next().is_some() {
        return None;
    }
    match kind {
        "s" => is_token(value).then(|| {
            AppLink::Shared(SharedNoteLink {
                origin,
                token: value.to_owned(),
            })
        }),
        "notes" => note_id(value).map(|note_id| AppLink::Note(NoteLink { origin, note_id })),
        _ => None,
    }
}

/// `cereal://shared?url=…`, `cereal://note?server=…&id=…`,
/// `cereal://auth?code=…&state=…&server=…`.
fn app(text: &str) -> Option<AppLink> {
    let prefix = text.get(..SCHEME.len() + 3)?;
    if !prefix.eq_ignore_ascii_case("cereal://") || text.contains('#') {
        return None;
    }
    let rest = &text[prefix.len()..];
    let (before, query) = rest.split_once('?').unwrap_or((rest, ""));
    // The action stands where a host would: no user, no port, no path.
    let action = before.strip_suffix('/').unwrap_or(before);
    if action.is_empty() || !action.bytes().all(|byte| byte.is_ascii_alphabetic()) {
        return None;
    }
    let mut items = Vec::new();
    for item in query.split('&').filter(|item| !item.is_empty()) {
        let (name, value) = match item.split_once('=') {
            Some((name, value)) => (name, Some(percent_decode(value)?)),
            None => (item, None),
        };
        items.push((percent_decode(name)?, value));
    }
    // The one value of a parameter. A repeated parameter is ambiguous and is
    // refused.
    let value = |name: &str| -> Option<&str> {
        let mut found = items.iter().filter(|(item, _)| item == name);
        let first = found.next()?;
        if found.next().is_some() {
            return None;
        }
        first.1.as_deref()
    };
    let absent = |name: &str| !items.iter().any(|(item, _)| item == name);
    match action.to_ascii_lowercase().as_str() {
        "shared" => {
            // The value is decoded once. One that is still encoded, or that
            // is itself an app link, does not parse as a web link.
            let inner = value("url")?;
            if !is_plain(inner) {
                return None;
            }
            match web(inner)? {
                link @ AppLink::Shared(_) => Some(link),
                _ => None,
            }
        }
        "note" => {
            let origin = ServerOrigin::parse(value("server")?)?;
            let note_id = note_id(value("id")?)?;
            Some(AppLink::Note(NoteLink { origin, note_id }))
        }
        "auth" => {
            let code = value("code").filter(|code| is_opaque(code, 1024))?;
            let state = value("state").filter(|state| is_opaque(state, 256))?;
            let origin = if absent("server") {
                None
            } else {
                Some(ServerOrigin::parse(value("server")?)?)
            };
            Some(AppLink::Auth(AuthCallback {
                code: code.to_owned(),
                state: state.to_owned(),
                origin,
            }))
        }
        _ => None,
    }
}

/// The server makes 43 characters of base64url. Other lengths are allowed
/// for a server of another version, within reason.
fn is_token(text: &str) -> bool {
    (16..=128).contains(&text.len())
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

/// A UUID in its usual hyphenated form, lower-cased.
fn note_id(text: &str) -> Option<String> {
    if text.len() != 36 {
        return None;
    }
    let groups: Vec<&str> = text.split('-').collect();
    let lengths = [8, 4, 4, 4, 12];
    if groups.len() != lengths.len()
        || !groups
            .iter()
            .zip(lengths)
            .all(|(group, length)| {
                group.len() == length && group.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
    {
        return None;
    }
    Some(text.to_ascii_lowercase())
}

fn is_opaque(text: &str, limit: usize) -> bool {
    !text.is_empty() && text.len() <= limit && text.bytes().all(|byte| byte > 0x20 && byte < 0x7f)
}

/// One round of percent-decoding. A stray `%` or a value that is not text
/// makes the whole link unreadable.
fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let pair = bytes.get(index + 1..index + 3)?;
            let digit = |byte: u8| (byte as char).to_digit(16);
            decoded.push((digit(pair[0])? * 16 + digit(pair[1])?) as u8);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

#[cfg(test)]
fn percent_encode(text: &str) -> String {
    let mut encoded = String::with_capacity(text.len() * 3);
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "Zk3vX0b1aQ9mT7cR2dS5eU8fW4gY6hJ0iK1lL3nM5oP";
    const NOTE: &str = "01136768-b41d-4ee8-ad19-e73976f18475";

    fn shared(text: &str) -> Option<SharedNoteLink> {
        match AppLink::parse(text) {
            Some(AppLink::Shared(link)) => Some(link),
            _ => None,
        }
    }

    fn note(text: &str) -> Option<NoteLink> {
        match AppLink::parse(text) {
            Some(AppLink::Note(link)) => Some(link),
            _ => None,
        }
    }

    fn app_shared(inner: &str) -> String {
        format!("cereal://shared?url={}", percent_encode(inner))
    }

    #[test]
    fn a_web_share_link_and_its_app_form_are_the_same_link() {
        let web = format!("https://notes.example.com/s/{TOKEN}");
        let link = shared(&web).expect("web link");
        assert_eq!(link.web_url(), web);
        assert_eq!(link.api_url(), format!("https://notes.example.com/api/v1/shared/{TOKEN}"));
        assert_eq!(link.origin.display_host(), "notes.example.com");
        assert_eq!(shared(&link.app_url()), Some(link.clone()));
        assert_eq!(shared(&app_shared(&web)), Some(link.clone()));
        // What a person pastes: white space around it, a query the mail
        // client added, another capitalisation of the scheme and host.
        assert_eq!(shared(&format!("  {web}\n")), Some(link.clone()));
        assert_eq!(shared(&format!("{web}?utm_source=mail#top")), Some(link.clone()));
        assert_eq!(shared(&format!("HTTPS://Notes.Example.COM:443/s/{TOKEN}")), Some(link.clone()));
        assert_eq!(shared(&format!("CEREAL://SHARED/?url={}", percent_encode(&web))), Some(link));
    }

    #[test]
    fn a_note_link_names_its_server_and_a_lower_case_id() {
        let link = note(&format!("cereal://note?server=https%3A%2F%2Fnotes.example.com&id={NOTE}"))
            .expect("app link");
        assert_eq!(link.note_id, NOTE);
        assert_eq!(link.web_url(), format!("https://notes.example.com/notes/{NOTE}"));
        assert_eq!(note(&format!("https://notes.example.com/notes/{NOTE}")), Some(link.clone()));
        assert_eq!(
            note(&format!(
                "cereal://note?id={}&server=https://notes.example.com/",
                NOTE.to_uppercase()
            )),
            Some(link)
        );
        let local = note(&format!("cereal://note?server=http%3A%2F%2F127.0.0.1%3A3000&id={NOTE}"))
            .expect("loopback");
        assert_eq!(local.origin.url(), "http://127.0.0.1:3000");
        assert_eq!(local.origin.display_host(), "127.0.0.1:3000");
    }

    #[test]
    fn a_sign_in_answer_carries_its_code_state_and_optional_server() {
        let with = AppLink::parse("cereal://auth?code=abc-123&state=xyz&server=http%3A%2F%2F127.0.0.1%3A3391");
        assert_eq!(
            with,
            Some(AppLink::Auth(AuthCallback {
                code: "abc-123".into(),
                state: "xyz".into(),
                origin: ServerOrigin::parse("http://127.0.0.1:3391"),
            }))
        );
        assert!(matches!(
            AppLink::parse("cereal://auth?code=abc&state=xyz"),
            Some(AppLink::Auth(AuthCallback { origin: None, .. }))
        ));
        // A sign-in answer is not something a person pastes to open a note.
        assert_eq!(AppLink::parse_note("cereal://auth?code=abc&state=xyz"), None);
        for refused in [
            "cereal://auth?state=xyz",
            "cereal://auth?code=abc",
            "cereal://auth?code=&state=xyz",
            "cereal://auth?code=abc&code=def&state=xyz",
            "cereal://auth?code=abc&state=xyz&server=",
            "cereal://auth?code=abc&state=xyz&server=http%3A%2F%2Fevil.example",
            "cereal://auth?code=abc&state=xyz&server=https%3A%2F%2Fa.example&server=https%3A%2F%2Fb.example",
            "cereal://auth?code=a%20b&state=xyz",
            "cereal://auth?code=abc&state=x%0Ay",
        ] {
            assert_eq!(AppLink::parse(refused), None, "{refused}");
        }
        let long = "a".repeat(1025);
        assert_eq!(AppLink::parse(&format!("cereal://auth?code={long}&state=xyz")), None);
        let long = "a".repeat(257);
        assert_eq!(AppLink::parse(&format!("cereal://auth?code=abc&state={long}")), None);
    }

    #[test]
    fn other_schemes_and_unknown_actions_are_not_links() {
        let web = format!("https://notes.example.com/s/{TOKEN}");
        for refused in [
            String::new(),
            " ".into(),
            "cereal".into(),
            "cereal:".into(),
            "cereal://".into(),
            "cereal://shared".into(),
            "cereal://open?url=x".into(),
            format!("cereal:shared?url={}", percent_encode(&web)),
            format!("cereal:/shared?url={}", percent_encode(&web)),
            format!("cereals://shared?url={}", percent_encode(&web)),
            format!("cereal://user@shared?url={}", percent_encode(&web)),
            format!("cereal://shared:80?url={}", percent_encode(&web)),
            format!("cereal://shared/extra?url={}", percent_encode(&web)),
            format!("cereal://shared?url={}#fragment", percent_encode(&web)),
            format!("cereal://shared?URL={}", percent_encode(&web)),
            format!("ftp://notes.example.com/s/{TOKEN}"),
            format!("file:///s/{TOKEN}"),
            format!("javascript:alert('{TOKEN}')"),
            format!("data:text/html,https://notes.example.com/s/{TOKEN}"),
            format!("notes.example.com/s/{TOKEN}"),
            format!("//notes.example.com/s/{TOKEN}"),
        ] {
            assert_eq!(AppLink::parse(&refused), None, "{refused}");
        }
    }

    #[test]
    fn plain_http_is_only_for_this_macs_own_loopback() {
        for refused in [
            format!("http://notes.example.com/s/{TOKEN}"),
            format!("http://192.168.1.10/s/{TOKEN}"),
            format!("http://127.0.0.2/s/{TOKEN}"),
            format!("http://localhost.example.com/s/{TOKEN}"),
            format!("http://127.0.0.1.example.com/s/{TOKEN}"),
            format!("http://127.0.0.1@evil.example/s/{TOKEN}"),
            format!("http://evil.example#@127.0.0.1/s/{TOKEN}"),
            format!("http://evil.example?@127.0.0.1/s/{TOKEN}"),
        ] {
            assert_eq!(AppLink::parse(&refused), None, "{refused}");
            assert_eq!(AppLink::parse(&app_shared(&refused)), None, "{refused}");
        }
        for accepted in [
            format!("http://localhost:3000/s/{TOKEN}"),
            format!("http://127.0.0.1:3000/s/{TOKEN}"),
            format!("http://[::1]:3000/s/{TOKEN}"),
        ] {
            assert!(shared(&accepted).is_some(), "{accepted}");
            assert!(shared(&app_shared(&accepted)).is_some(), "{accepted}");
        }
        assert_eq!(
            shared(&format!("http://[::1]:3000/s/{TOKEN}")).unwrap().origin.display_host(),
            "[::1]:3000"
        );
    }

    #[test]
    fn nested_and_broken_percent_encoding_is_refused() {
        let web = format!("https://notes.example.com/s/{TOKEN}");
        let once = percent_encode(&web);
        let twice = percent_encode(&once);
        assert!(shared(&format!("cereal://shared?url={once}")).is_some());
        for refused in [
            // Encoded twice: one round of decoding leaves an encoded URL.
            format!("cereal://shared?url={twice}"),
            // An app link inside an app link.
            format!("cereal://shared?url={}", percent_encode(&format!("cereal://shared?url={once}"))),
            // An encoded slash, dot or token character in the web path.
            format!("https://notes.example.com/s%2F{TOKEN}"),
            format!("https://notes.example.com/%73/{TOKEN}"),
            format!("https://notes.example.com/s/{}%41", &TOKEN[..42]),
            app_shared(&format!("https://notes.example.com/s/{}%2541", &TOKEN[..40])),
            // Broken escapes, and bytes that are not text.
            "cereal://shared?url=%".into(),
            "cereal://shared?url=%4".into(),
            "cereal://shared?url=%zz".into(),
            "cereal://shared?url=%ff%fe".into(),
            format!("cereal://shared?url={once}%00"),
            format!("cereal://shared?url={once}%0d%0aHost:%20evil.example"),
            // The same parameter twice.
            format!("cereal://shared?url={once}&url={once}"),
            "cereal://shared?url".into(),
        ] {
            assert_eq!(AppLink::parse(&refused), None, "{refused}");
        }
    }

    #[test]
    fn tokens_have_a_sane_length_and_alphabet() {
        let link = |token: &str| format!("https://notes.example.com/s/{token}");
        assert!(shared(&link(&"a".repeat(16))).is_some());
        assert!(shared(&link(&"a".repeat(128))).is_some());
        for refused in [
            "a".repeat(15),
            "a".repeat(129),
            "a".repeat(4000),
            format!("{}.", &TOKEN[..42]),
            format!("{}~", &TOKEN[..42]),
            format!("{}+", &TOKEN[..42]),
            format!("{} {}", &TOKEN[..20], &TOKEN[20..]),
            format!("{}é", &TOKEN[..42]),
        ] {
            assert_eq!(AppLink::parse(&link(&refused)), None, "{refused}");
        }
        // A link longer than any real one is refused before it is read.
        let padded = format!("{}?{}", link(TOKEN), "x".repeat(MAXIMUM_LENGTH));
        assert_eq!(AppLink::parse(&padded), None);
    }

    #[test]
    fn paths_are_matched_whole_so_traversal_goes_nowhere() {
        for refused in [
            format!("https://notes.example.com/s/../s/{TOKEN}"),
            format!("https://notes.example.com/s/./{TOKEN}"),
            format!("https://notes.example.com//s/{TOKEN}"),
            format!("https://notes.example.com/s//{TOKEN}"),
            format!("https://notes.example.com/s/{TOKEN}/"),
            format!("https://notes.example.com/s/{TOKEN}/.."),
            format!("https://notes.example.com/S/{TOKEN}"),
            format!("https://notes.example.com/api/v1/shared/{TOKEN}"),
            format!("https://notes.example.com/x/../s/{TOKEN}"),
            format!("https://notes.example.com\\s\\{TOKEN}"),
            format!("https://notes.example.com/s\\{TOKEN}"),
            "https://notes.example.com/s/..".into(),
            "https://notes.example.com/s/................".into(),
            format!("https://notes.example.com/notes/../s/{TOKEN}"),
            format!("https://notes.example.com/notes/{NOTE}/../../admin"),
        ] {
            assert_eq!(AppLink::parse(&refused), None, "{refused}");
            assert_eq!(AppLink::parse(&app_shared(&refused)), None, "{refused}");
        }
    }

    #[test]
    fn note_ids_must_be_hyphenated_uuids() {
        let link = |id: &str| format!("cereal://note?server=https%3A%2F%2Fnotes.example.com&id={id}");
        assert!(note(&link(NOTE)).is_some());
        for refused in [
            "",
            "1",
            "not-a-uuid",
            "01136768b41d4ee8ad19e73976f18475",
            "{01136768-b41d-4ee8-ad19-e73976f18475}",
            "urn:uuid:01136768-b41d-4ee8-ad19-e73976f18475",
            "01136768-b41d-4ee8-ad19-e73976f1847",
            "01136768-b41d-4ee8-ad19-e73976f184755",
            "01136768-b41d-4ee8-ad19-e73976f1847g",
            "01136768-b41d-4ee8-ad19e-73976f18475",
            "01136768_b41d_4ee8_ad19_e73976f18475",
            "../../../../../../etc/passwd-aaaaaaa",
            "01136768-b41d-4ee8-ad19-e73976f18475%2F..",
        ] {
            assert_eq!(AppLink::parse(&link(refused)), None, "{refused}");
            assert_eq!(
                AppLink::parse(&format!("https://notes.example.com/notes/{refused}")),
                None,
                "{refused}"
            );
        }
        // The server must be a bare origin: no path, query, user or other scheme.
        for server in [
            "https%3A%2F%2Fnotes.example.com%2Fbase",
            "https%3A%2F%2Fnotes.example.com%3Fq%3D1",
            "https%3A%2F%2Fuser%3Apass%40notes.example.com",
            "http%3A%2F%2Fnotes.example.com",
            "ftp%3A%2F%2Fnotes.example.com",
            "notes.example.com",
            "",
        ] {
            let text = format!("cereal://note?server={server}&id={NOTE}");
            assert_eq!(AppLink::parse(&text), None, "{server}");
        }
        assert_eq!(AppLink::parse(&format!("cereal://note?id={NOTE}")), None);
    }

    #[test]
    fn hosts_are_plain_names_or_addresses_and_nothing_else() {
        for refused in [
            "https://",
            "https:///s",
            "https://.",
            "https://-a.example",
            "https://a-.example",
            "https://a..example",
            "https://example.com.",
            "https://exa_mple.com",
            "https://exa%6dple.com",
            "https://example.com:",
            "https://example.com:0",
            "https://example.com:65536",
            "https://example.com:99999999999",
            "https://example.com:80a",
            "https://example.com:+80",
            "https://[::1",
            "https://[]",
            "https://[::1]x",
            "https://[example.com]",
            "https://[::g]",
            "https://a:b:c",
            "https://user@example.com",
            "https://user:pass@example.com",
            "https://exämple.com",
            "https://example.com\u{0}",
        ] {
            assert_eq!(ServerOrigin::parse(refused), None, "{refused}");
        }
        let long_label = format!("https://{}.example", "a".repeat(64));
        assert_eq!(ServerOrigin::parse(&long_label), None);
        let long_name = format!("https://{}example", "a.".repeat(130));
        assert_eq!(ServerOrigin::parse(&long_name), None);
        // The ASCII form of an international name is kept as it is.
        assert_eq!(
            ServerOrigin::parse("https://xn--exmple-cua.com").unwrap().display_host(),
            "xn--exmple-cua.com"
        );
        assert_eq!(ServerOrigin::parse("https://Example.com/").unwrap().url(), "https://example.com");
        assert_eq!(ServerOrigin::parse("https://example.com:8443").unwrap().url(), "https://example.com:8443");
        assert_eq!(ServerOrigin::parse("http://localhost:80").unwrap().url(), "http://localhost");
        assert_eq!(ServerOrigin::parse("https://example.com/base"), None);
        assert_eq!(
            ServerOrigin::of_server("https://example.com/base").unwrap().url(),
            "https://example.com"
        );
        assert_eq!(ServerOrigin::of_server("https://example.com?x=1"), None);
    }

    #[test]
    fn a_server_is_matched_by_its_whole_normalised_origin() {
        let origin = |text: &str| ServerOrigin::parse(text).unwrap();
        let local = origin("http://127.0.0.1:3000");
        assert!(local.is_server("http://127.0.0.1:3000"));
        assert!(local.is_server("http://127.0.0.1:3000/"));
        assert!(local.is_server("HTTP://127.0.0.1:3000"));
        // The same machine under another name is another origin.
        assert!(!local.is_server("http://localhost:3000"));
        assert!(!local.is_server("http://127.0.0.1:3001"));
        assert!(!local.is_server("http://127.0.0.1"));
        assert!(!local.is_server("https://127.0.0.1:3000"));
        assert!(!local.is_server(""));
        let hosted = origin("https://notes.example.com");
        assert!(hosted.is_server("https://notes.example.com:443"));
        assert!(hosted.is_server("https://NOTES.example.com"));
        assert!(!hosted.is_server("https://notes.example.com:8443"));
        assert!(!hosted.is_server("https://notes.example.com.evil.example"));
        assert!(!hosted.is_server("https://evil.example/notes.example.com"));
        assert!(!hosted.is_server("https://notes.example.com@evil.example"));
        // A server kept under a path is not named by a link's origin alone.
        assert!(!hosted.is_server("https://notes.example.com/base"));
    }

    #[test]
    fn a_link_waits_only_while_the_saved_session_is_being_checked() {
        // Launching into a server with a saved session: held until it loads.
        assert!(must_wait(true, false, true));
        // Signed in, signed out for good, and local mode: followed at once.
        assert!(!must_wait(true, true, true));
        assert!(!must_wait(true, true, false));
        assert!(!must_wait(true, false, false));
        assert!(!must_wait(false, false, true));
        assert!(!must_wait(false, false, false));
    }

    #[test]
    fn a_note_is_read_only_with_a_session_on_its_own_server() {
        let origin = ServerOrigin::parse("https://notes.example.com").unwrap();
        assert_eq!(access(&origin, Some("https://notes.example.com"), true), Access::Read);
        // Signed in somewhere else: nothing is offered that would replace
        // the account in use.
        assert_eq!(
            access(&origin, Some("https://other.example.com"), true),
            Access::SignIn { offered: false }
        );
        // Connected but signed out, and local mode: signing in is offered.
        assert_eq!(
            access(&origin, Some("https://notes.example.com"), false),
            Access::SignIn { offered: true }
        );
        assert_eq!(
            access(&origin, Some("https://other.example.com"), false),
            Access::SignIn { offered: true }
        );
        assert_eq!(access(&origin, None, false), Access::SignIn { offered: true });
        assert_eq!(access(&origin, None, true), Access::SignIn { offered: true });
    }
}
