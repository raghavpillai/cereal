//! Accounts. The app holds an ordered list of accounts; an account is one
//! user on one server. Each has its own Keychain item (address, user id and
//! token together), its own directory, its own client, serial worker and
//! cancellation epoch. Nothing is "current": a request can only be made
//! through an account's client, and that client can only reach its own
//! origin.
//!
//! With one account the list happens to hold one entry; there is no separate
//! one-account path.
mod app;
pub mod client;
pub mod handle;
pub mod keychain;
pub mod migration;
pub mod registry;
pub mod saved;
pub mod skeleton;

pub use client::AccountClient;
pub use keychain::Secret;
pub use registry::Registry;

use crate::link::ServerOrigin;
use meeting_notes_api_types::AuthSessionDTO;
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, MutexGuard, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

/// The name of an account everywhere on this device: on the desktop, the
/// name of its directory, `uuid v5(NAMESPACE_URL, "<address>|<user id>")`.
/// Always a hyphenated lower-case UUID, so it is safe as a path component.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AccountId(String);

impl AccountId {
    /// The account of user `user_id` (canonical) at `url` (as stored).
    pub fn of(url: &str, user_id: &str) -> Self {
        Self(
            uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, identity(url, user_id).as_bytes())
                .hyphenated()
                .to_string(),
        )
    }

    /// An entry that has not been verified yet has no user and so no
    /// directory. It is known by a name no directory can have.
    pub fn pending(url: &str) -> Self {
        Self(
            uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_OID, format!("pending|{url}").as_bytes())
                .hyphenated()
                .to_string(),
        )
    }

    /// An id read from a file or a directory name: a hyphenated lower-case
    /// UUID and nothing else. Checked every time one is used to build a path.
    pub fn parse(text: &str) -> Option<Self> {
        let id = uuid::Uuid::parse_str(text).ok()?;
        (id.hyphenated().to_string() == text).then(|| Self(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// How a log line names an account.
    pub fn short(&self) -> &str {
        &self.0[..8]
    }
}

/// `"<address as stored>|<user id>"`. No collision is possible through `|`:
/// the user id is a validated UUID and contains none.
pub fn identity(url: &str, user_id: &str) -> String {
    format!("{url}|{user_id}")
}

/// The server's user id, written the one way: hyphenated, lower case. A
/// server that changes the spelling of the id does not make a second account.
pub fn canonical_user_id(raw: &str) -> Option<String> {
    uuid::Uuid::parse_str(raw.trim())
        .ok()
        .map(|id| id.hyphenated().to_string())
}

/// A server address as a person may type it: HTTPS, or plain HTTP (what
/// that means for the address is said beside the field,
/// `link::address_transport`). The session still goes only to the exact
/// origin it was issued for.
pub fn validate_url(raw: &str) -> Result<String, String> {
    let raw = raw.trim().trim_end_matches('/');
    let (scheme, rest) = raw.split_once("://").ok_or("Enter a valid server URL")?;
    let authority = rest.split('/').next().unwrap_or("");
    if authority.is_empty()
        || !(scheme == "https" || scheme == "http")
        || raw.chars().any(|c| c.is_control() || c.is_whitespace())
        || raw.contains(['@', '?', '#', '"', '\\'])
        || rest.split('/').any(|p| p == "." || p == "..")
    {
        return Err("Enter an address that starts with https:// or http://".into());
    }
    Ok(raw.to_owned())
}

/// A new account's address as it is stored: lower-case scheme and host, the
/// path as typed.
pub fn normalise_address(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.to_owned();
    };
    let (authority, path) = match rest.find('/') {
        Some(at) => rest.split_at(at),
        None => (rest, ""),
    };
    format!(
        "{}://{}{path}",
        scheme.to_ascii_lowercase(),
        authority.to_ascii_lowercase()
    )
}

/// What follows the authority in an address, without a trailing slash.
fn path_prefix(url: &str) -> &str {
    url.split_once("://")
        .map(|(_, rest)| rest)
        .and_then(|rest| rest.find('/').map(|at| &rest[at..]))
        .unwrap_or("")
        .trim_end_matches('/')
}

/// Two addresses name the same server: equal normalised origin and equal
/// path prefix. An address with no origin (a non-ASCII host, which only a
/// migrated account can have) is the same only as itself.
pub fn same_server(a: &str, b: &str) -> bool {
    match (ServerOrigin::of_server(a), ServerOrigin::of_server(b)) {
        (Some(first), Some(second)) => first == second && path_prefix(a) == path_prefix(b),
        _ => a == b,
    }
}

/// What a person recognises a server by: host and, when it is not the usual
/// one, port. The address itself when it has no origin.
pub fn host_of(url: &str) -> String {
    ServerOrigin::of_server(url)
        .map(|origin| origin.display_host())
        .unwrap_or_else(|| {
            url.split_once("://")
                .map_or(url, |(_, rest)| rest)
                .split('/')
                .next()
                .unwrap_or(url)
                .to_owned()
        })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AccountState {
    /// Being verified in this launch. Its notes are not shown yet.
    Checking,
    Ready,
    Unreachable { message: String },
    /// Its own `/session` answered that the session has ended, or no usable
    /// session is saved for it.
    NeedsSignIn,
}

#[derive(Clone, Debug)]
pub struct Account {
    pub id: AccountId,
    /// 1 to 4095, never reused. Matters only in memory (`handle`).
    pub slot: u16,
    /// The address as stored: the base of every request.
    pub url: String,
    /// `None` for an address `ServerOrigin` refuses: it matches no link.
    pub origin: Option<ServerOrigin>,
    /// `None` until the first successful `/session` after a migration.
    pub user_id: Option<String>,
    pub name: String,
    pub email: String,
    pub label: String,
    pub color: u8,
    pub added_at: String,
    pub state: AccountState,
    pub features: Vec<String>,
    pub verified_this_launch: bool,
    epoch: Arc<AtomicU64>,
    suspect: Arc<AtomicBool>,
}

impl Account {
    pub(crate) fn new(id: AccountId, slot: u16, url: String) -> Self {
        Self {
            origin: ServerOrigin::of_server(&url),
            label: host_of(&url),
            id,
            slot,
            url,
            user_id: None,
            name: String::new(),
            email: String::new(),
            color: 0,
            added_at: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
            state: AccountState::Checking,
            features: Vec::new(),
            verified_this_launch: false,
            epoch: Arc::new(AtomicU64::new(1)),
            suspect: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Bumped on sign-out, removal, token replacement and a session that
    /// has ended. Requests check their own account's epoch.
    pub fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::Acquire)
    }
    pub(crate) fn epoch_cell(&self) -> Arc<AtomicU64> {
        self.epoch.clone()
    }
    pub(crate) fn suspect_cell(&self) -> Arc<AtomicBool> {
        self.suspect.clone()
    }
    pub(crate) fn bump_epoch(&self) {
        self.epoch.fetch_add(1, Ordering::AcqRel);
    }

    /// Host and, when it is not the usual one, port.
    pub fn host(&self) -> String {
        host_of(&self.url)
    }

    pub fn is_ready(&self) -> bool {
        self.state == AccountState::Ready
    }

    /// Its notes may be shown: its session was verified in this launch and
    /// has not ended since. (Unreachable after having been Ready counts.)
    pub fn shown(&self) -> bool {
        self.verified_this_launch && self.state != AccountState::NeedsSignIn
    }

    /// Whether this is the account `verified` belongs to.
    pub fn is(&self, url: &str, user_id: &str) -> bool {
        self.user_id.as_deref() == Some(user_id) && same_server(&self.url, url)
    }
}

/// Where the registry and each account's files live.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Paths {
    /// Holds `accounts.json` and `connection.json`.
    pub meta: PathBuf,
    /// Holds `servers/<AccountId>/` and the device-level files.
    pub data: PathBuf,
}

impl Paths {
    pub fn at(root: &Path) -> Self {
        Self {
            meta: root.to_path_buf(),
            data: root.to_path_buf(),
        }
    }

    /// The registry is beside `connection.json`, wherever that is: in an
    /// isolated data directory, or the usual place (a release build with
    /// only `CEREAL_DATA_DIR` keeps both there).
    pub fn of_environment() -> Option<Self> {
        let usual = || {
            std::env::var_os("HOME")
                .map(|home| PathBuf::from(home).join("Library/Application Support/Cereal"))
        };
        let data = crate::environment::var_os("CEREAL_DATA_DIR")
            .filter(|directory| !directory.is_empty())
            .map(PathBuf::from)
            .or_else(usual)?;
        let meta = keychain::isolated_directory().or_else(usual)?;
        Some(Self { meta, data })
    }

    /// Pictures of the people signed in here, kept between launches
    /// (`crate::portrait`).
    pub fn portraits(&self) -> PathBuf {
        self.data.join("portraits")
    }

    pub fn registry(&self) -> PathBuf {
        self.meta.join("accounts.json")
    }
    pub fn connection(&self) -> PathBuf {
        self.meta.join("connection.json")
    }
    pub fn servers(&self) -> PathBuf {
        self.data.join("servers")
    }
    /// An account's directory. The id is a checked UUID by construction.
    pub fn account(&self, id: &AccountId) -> PathBuf {
        self.servers().join(id.as_str())
    }
    pub fn workspace(&self, id: &AccountId) -> PathBuf {
        self.account(id).join("workspace.json")
    }
    pub fn recordings(&self, id: &AccountId) -> PathBuf {
        self.account(id).join("recordings")
    }
    pub fn chats(&self, id: &AccountId) -> PathBuf {
        self.account(id).join("chats.json")
    }
    pub fn settings(&self) -> PathBuf {
        self.data.join("settings.json")
    }
}

/// A session a server has just confirmed: what a sign-in ends with.
#[derive(Clone)]
pub struct VerifiedAccount {
    /// The address the sign-in was made to, validated, as typed.
    pub url: String,
    pub token: Secret,
    pub session: AuthSessionDTO,
}

/// Why an account cannot be signed out of or removed right now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blocked {
    Recording,
    MoveOpen,
}

impl Blocked {
    pub fn message(self) -> &'static str {
        match self {
            Self::Recording => "Stop the recording first",
            Self::MoveOpen => "A note is still being moved. Finish or give up the move first.",
        }
    }
}

// ---------------------------------------------------------------------------
// The process's registry.

static RUNTIME: OnceLock<Mutex<Registry>> = OnceLock::new();
/// Moves whenever the list of accounts, a state, a label or the default
/// changes, so views that draw accounts know to look again.
static REVISION: AtomicU64 = AtomicU64::new(1);
/// The app is set up for a server: it has (or had, in this run) an account,
/// or was given an address for this run.
static SERVER_MODE: AtomicBool = AtomicBool::new(false);
/// What went wrong reading the registry at launch, to be shown once.
static LOAD_NOTICE: Mutex<Option<String>> = Mutex::new(None);

pub(crate) fn touch() {
    REVISION.fetch_add(1, Ordering::AcqRel);
}

pub fn revision() -> u64 {
    REVISION.load(Ordering::Acquire)
}

fn runtime() -> &'static Mutex<Registry> {
    RUNTIME.get_or_init(|| {
        let registry = Registry::of_environment();
        if !registry.accounts().is_empty() || registry.configured_url().is_some_and(|url| !url.is_empty()) {
            SERVER_MODE.store(true, Ordering::Release);
        }
        *LOAD_NOTICE.lock().unwrap_or_else(|poison| poison.into_inner()) = registry.notice.clone();
        Mutex::new(registry)
    })
}

/// The registry, for one short look or change. Never held across a request.
pub fn registry() -> MutexGuard<'static, Registry> {
    runtime().lock().unwrap_or_else(|poison| poison.into_inner())
}

/// The app is set up for a server rather than "this Mac only".
pub fn server_mode() -> bool {
    let _ = runtime();
    SERVER_MODE.load(Ordering::Acquire)
}

pub(crate) fn enter_server_mode() {
    SERVER_MODE.store(true, Ordering::Release);
}

/// What went wrong reading the registry at launch, once.
pub fn take_load_notice() -> Option<String> {
    let _ = runtime();
    LOAD_NOTICE.lock().unwrap_or_else(|poison| poison.into_inner()).take()
}

/// Every account, in the person's account order.
pub fn all() -> Vec<Account> {
    registry().accounts().to_vec()
}

pub fn get(id: &AccountId) -> Option<Account> {
    registry().get(id).cloned()
}

pub fn by_slot(slot: u16) -> Option<Account> {
    registry().by_slot(slot).cloned()
}

/// The account a note in memory belongs to.
pub fn of_note(note_id: u64) -> Option<Account> {
    registry().of_note(note_id).cloned()
}

/// Where a new note with no calendar event goes, and whose server is asked
/// for updates.
pub fn default_account() -> Option<Account> {
    registry().default().cloned()
}

/// The session the server confirmed for an account in this launch.
pub fn session_of(id: &AccountId) -> Option<AuthSessionDTO> {
    registry().session(id).cloned()
}

/// The default account's confirmed session: who the sidebar and Settings
/// name. `None` until it has been verified in this launch.
pub fn default_session() -> Option<AuthSessionDTO> {
    let registry = registry();
    let account = registry.default()?;
    account
        .shown()
        .then(|| registry.session(&account.id).cloned())
        .flatten()
}

pub fn client(id: &AccountId) -> Result<AccountClient, String> {
    registry().client(id)
}

/// The client of the account a note belongs to.
pub fn client_of_note(note_id: u64) -> Result<AccountClient, String> {
    let registry = registry();
    let account = registry
        .of_note(note_id)
        .ok_or("This note does not belong to an account on this Mac")?;
    registry.client(&account.id)
}

/// The default account's client: for what has no note or event to belong to.
pub fn default_client() -> Result<AccountClient, String> {
    let registry = registry();
    let account = registry.default().ok_or("Sign in to your server first")?;
    registry.client(&account.id)
}

pub fn epoch_of(id: &AccountId) -> Option<u64> {
    registry().get(id).map(Account::epoch)
}

/// The epoch of the account a note belongs to: what a request about that
/// note checks. `None` when the note has no account here (any more).
pub fn note_epoch(note_id: u64) -> Option<u64> {
    registry().of_note(note_id).map(Account::epoch)
}

/// The confirmed session of the account a note belongs to: who "me" is on
/// that note.
pub fn session_of_note(note_id: u64) -> Option<AuthSessionDTO> {
    let registry = registry();
    let account = registry.of_note(note_id)?;
    registry.session(&account.id).cloned()
}

/// Some account's session has been verified in this launch and has not
/// ended: there are notes to show.
pub fn any_shown() -> bool {
    registry().accounts().iter().any(Account::shown)
}

/// Some account is still being verified.
pub fn any_checking() -> bool {
    registry()
        .accounts()
        .iter()
        .any(|account| account.state == AccountState::Checking)
}

/// The address the sign-in form starts with: the one given for this run,
/// else the default account's, else the one the last session was for.
pub fn configured_url() -> String {
    registry().configured_url().unwrap_or_default()
}

/// The one server asked for updates: the default account's, as a checked
/// address, whether or not its session has been verified. Other accounts'
/// servers are never asked.
pub fn update_server() -> Option<String> {
    validate_url(&configured_url()).ok()
}

/// A session is saved (an account in the registry, or the legacy item),
/// whether or not it has been checked yet.
pub fn has_saved_session() -> bool {
    registry().has_saved_session()
}

/// Labels and dots appear only with two or more accounts.
pub fn several() -> bool {
    registry().accounts().len() > 1
}

/// `accounts.multiple`: the switch that gates the ways to get a second
/// account. Off until milestone 4; a debug build may turn it on.
pub fn multiple() -> bool {
    cfg!(debug_assertions)
        && crate::environment::var("CEREAL_ACCOUNTS_MULTIPLE").is_ok_and(|value| value == "1")
}

/// Accounts whose server answered 401 or 403 to something since the last
/// look. Each is asked for its own `/session` once.
pub fn take_suspects() -> Vec<AccountId> {
    registry()
        .accounts()
        .iter()
        .filter(|account| account.suspect.swap(false, Ordering::AcqRel))
        .filter(|account| account.state == AccountState::Ready)
        .map(|account| account.id.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every source file of the crate, with its test modules cut off: what
    /// ships.
    fn shipped_sources() -> Vec<(String, String)> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut pending = vec![root.clone()];
        let mut files = Vec::new();
        while let Some(directory) = pending.pop() {
            for entry in std::fs::read_dir(directory).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    pending.push(path);
                } else if path.extension().is_some_and(|extension| extension == "rs") {
                    let text = std::fs::read_to_string(&path).unwrap();
                    let cut = ["\n#[cfg(test)]\nmod ", "\n#[cfg(test)]\npub(crate) mod "]
                        .iter()
                        .filter_map(|marker| text.find(marker))
                        .min()
                        .unwrap_or(text.len());
                    let name = path.strip_prefix(&root).unwrap().to_string_lossy().into_owned();
                    files.push((name, text[..cut].to_owned()));
                }
            }
        }
        assert!(files.len() > 30);
        files
    }

    #[test]
    fn t8_no_authorization_is_built_outside_the_account_client() {
        for (name, text) in shipped_sources() {
            if name == "accounts/client.rs" {
                continue;
            }
            for needle in ["Authorization: ", "AUTHORIZATION", "Bearer "] {
                assert!(!text.contains(needle), "{name} builds a session header ({needle})");
            }
        }
        // And the one request type others may use cannot carry one.
        let fetch = shipped_sources().into_iter().find(|(name, _)| name == "fetch.rs").unwrap().1;
        assert!(!fetch.contains("bearer"));
    }

    #[test]
    fn t7_nothing_the_app_draws_is_fetched_from_a_server() {
        // A note's text is drawn as text: no image element is ever given an
        // address, no network image source or HTTP client is used, and the
        // asset source serves only what is embedded in the app. The one
        // exception is the signed-in person's own picture, which
        // `portrait.rs` fetches itself from Google's picture host, decodes,
        // and hands over as pixels (its own tests say which addresses).
        for (name, text) in shipped_sources() {
            for needle in ["ImageSource::Uri", "Resource::Uri", "http_client", "reqwest", "img(format!", "img(url", "img(link"] {
                assert!(!text.contains(needle), "{name} uses {needle}");
            }
            for (at, _) in text.match_indices("img(") {
                let argument = &text[at + 4..(at + 64).min(text.len())];
                assert!(
                    argument.starts_with("\"icons/")
                        || argument.starts_with("SharedString::from(choice.asset())")
                        || (name == "portrait.rs" && argument.starts_with("picture)")),
                    "{name}: img({argument}"
                );
            }
        }
        // Nothing but that module asks for bytes to draw.
        for (name, text) in shipped_sources() {
            assert!(name == "portrait.rs" || name == "fetch.rs" || !text.contains("fetch::bytes("), "{name} fetches something to draw");
        }
        let ui = shipped_sources().into_iter().find(|(name, _)| name == "ui.rs").unwrap().1;
        assert!(ui.contains("Ok(Self::get(path).map(|f| f.data))"), "assets are the embedded ones");
        // A note whose text holds remote image markdown is the same text.
        let note = crate::model::Note {
            enhanced: "![tracking pixel](https://evil.example/pixel.png)".into(),
            ..Default::default()
        };
        assert_eq!(note.frame_copy().enhanced, note.enhanced);
    }

    #[test]
    fn i2_a_user_id_in_any_spelling_is_one_account() {
        let canonical = "0b7e1c9e-5a44-4c1d-9d2e-3f6a7b8c9d01";
        for spelling in [
            "0b7e1c9e-5a44-4c1d-9d2e-3f6a7b8c9d01",
            "0B7E1C9E-5A44-4C1D-9D2E-3F6A7B8C9D01",
            "0b7e1c9e5a444c1d9d2e3f6a7b8c9d01",
            "{0b7e1c9e-5a44-4c1d-9d2e-3f6a7b8c9d01}",
            "{0B7E1C9E-5A44-4C1D-9D2E-3F6A7B8C9D01}",
            " 0b7e1c9e-5a44-4c1d-9d2e-3f6a7b8c9d01 ",
        ] {
            assert_eq!(canonical_user_id(spelling).as_deref(), Some(canonical), "{spelling}");
            assert_eq!(
                AccountId::of("https://notes.acme.com", &canonical_user_id(spelling).unwrap()),
                AccountId::of("https://notes.acme.com", canonical)
            );
        }
        assert_eq!(canonical_user_id("not-an-id"), None);
        assert_eq!(canonical_user_id("a|b"), None);
        // The directory is today's: uuid v5 of "<address>|<user id>".
        let id = AccountId::of("https://notes.acme.com", canonical);
        assert_eq!(
            crate::model::account_cache_path(
                Path::new("/root"),
                &format!("https://notes.acme.com|{canonical}")
            ),
            Path::new("/root/servers").join(id.as_str()).join("workspace.json")
        );
    }

    #[test]
    fn an_account_id_is_a_hyphenated_lower_case_uuid_and_nothing_else() {
        let id = AccountId::of("https://notes.acme.com", "0b7e1c9e-5a44-4c1d-9d2e-3f6a7b8c9d01");
        assert_eq!(AccountId::parse(id.as_str()), Some(id.clone()));
        for text in [
            "",
            "..",
            "../x",
            "a/b",
            "3F0C1C9E-5A44-4C1D-9D2E-3F6A7B8C9D01",
            "3f0c1c9e5a444c1d9d2e3f6a7b8c9d01",
            "{3f0c1c9e-5a44-4c1d-9d2e-3f6a7b8c9d01}",
            "3f0c1c9e-5a44-4c1d-9d2e-3f6a7b8c9d01/..",
        ] {
            assert_eq!(AccountId::parse(text), None, "{text:?}");
        }
        assert_eq!(id.short().len(), 8);
    }

    #[test]
    fn addresses_are_compared_by_origin_and_path_and_stored_normalised() {
        assert!(same_server("https://Notes.Acme.com", "https://notes.acme.com/"));
        assert!(same_server("https://notes.acme.com:443", "https://notes.acme.com"));
        assert!(!same_server("https://notes.acme.com", "http://notes.acme.com"));
        assert!(!same_server("https://notes.acme.com/a", "https://notes.acme.com/b"));
        assert!(same_server("https://notes.acme.com/base/", "https://NOTES.acme.com/base"));
        // No origin: the same only as itself.
        assert!(same_server("https://nötes.example", "https://nötes.example"));
        assert!(!same_server("https://nötes.example", "https://notes.example"));
        assert_eq!(normalise_address("HTTPS://Notes.Acme.com/Base"), "https://notes.acme.com/Base");
        assert_eq!(host_of("http://127.0.0.1:3000"), "127.0.0.1:3000");
        assert_eq!(host_of("https://nötes.example/x"), "nötes.example");
    }

    #[test]
    fn a_server_address_is_https_or_http_and_nothing_dressed_up() {
        assert!(validate_url("https://notes.example/base").is_ok());
        assert!(validate_url("http://127.0.0.1:3000").is_ok());
        // A server on a private network, over plain HTTP.
        assert_eq!(validate_url("http://10.30.1.77:3000/").unwrap(), "http://10.30.1.77:3000");
        assert!(validate_url("http://cereal.local:3000").is_ok());
        assert!(validate_url("http://example.com").is_ok());
        for value in [
            "ftp://10.30.1.77",
            "//10.30.1.77:3000",
            "10.30.1.77:3000",
            "http://",
            "http://secret@10.30.1.77",
            "http://10.30.1.77/?token=x",
            "http://10.30.1.77/#x",
            "http://10.30.1.77/../api",
            "http://10.30.1.77\\evil",
            "http://10.30.1.77 :3000",
            "https://secret@host",
            "https://host/?token=x",
            "https://host/../api",
            "https://host\nattack",
        ] {
            assert!(validate_url(value).is_err());
        }
    }
}
