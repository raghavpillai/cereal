//! Where sessions are kept: one item per account, whose value binds the token
//! to its address and user. In the installed app that is the macOS Keychain.
//! An instance pointed at its own `CEREAL_DATA_DIR` keeps them as private
//! files in that directory and never opens the Keychain (see
//! [`isolated_directory`]), so a test instance cannot read, replace or delete
//! a session of the installed app.
use std::path::PathBuf;

/// The Keychain service every session item of this app is under.
pub const SERVICE: &str = "dev.cereal.desktop.session";
/// The one item of builds before accounts: `{"url","token"}`.
pub const LEGACY: &str = "current-session";

/// The name of an account's session item.
pub fn account_item(id: &super::AccountId) -> String {
    format!("account:{}", id.as_str())
}

/// The name of the item that lists an account's open moves. Separate from
/// the session, so replacing a token never touches it.
pub fn moves_item(id: &super::AccountId) -> String {
    format!("moves:{}", id.as_str())
}

/// A secret that is never printed.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
    /// The value itself. Only the store and the client's transport read it.
    pub(super) fn expose(&self) -> &str {
        &self.0
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("<redacted>")
    }
}

/// Reading an item failed for a reason other than its absence (the Keychain
/// is locked, a prompt was denied). Never treated as "there is none".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unreadable;

#[derive(Clone)]
pub enum SessionStore {
    Keychain { service: String },
    /// Private files in an isolated data directory.
    Directory(PathBuf),
    #[cfg(test)]
    Memory(std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, Vec<u8>>>>),
}

/// An instance pointed at its own `CEREAL_DATA_DIR` keeps its sessions and
/// its registry in that directory. A debug build does so whenever the
/// directory is set. A release build does so only when
/// `CEREAL_ISOLATED_SESSION=1` asks for it as well; otherwise it always uses
/// the Keychain and the usual place.
pub fn isolated_directory() -> Option<PathBuf> {
    let asked = crate::environment::var("CEREAL_ISOLATED_SESSION").is_ok_and(|value| value == "1");
    if !cfg!(debug_assertions) && !asked {
        return None;
    }
    crate::environment::var_os("CEREAL_DATA_DIR")
        .filter(|directory| !directory.is_empty())
        .map(PathBuf::from)
}

impl SessionStore {
    /// The store this process uses.
    pub fn of_environment() -> Self {
        match isolated_directory() {
            Some(directory) => Self::Directory(directory),
            None => Self::Keychain {
                service: SERVICE.into(),
            },
        }
    }

    #[cfg(test)]
    pub fn memory() -> Self {
        Self::Memory(Default::default())
    }

    /// The file an item is kept in, in an isolated directory. The legacy
    /// item keeps the name builds before accounts gave it.
    fn file(directory: &std::path::Path, name: &str) -> PathBuf {
        if name == LEGACY {
            return directory.join("session.json");
        }
        let safe: String = name
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' })
            .collect();
        directory.join("sessions").join(format!("{safe}.json"))
    }

    /// The item's bytes; `Ok(None)` when there is no such item.
    pub fn read(&self, name: &str) -> Result<Option<Vec<u8>>, Unreadable> {
        match self {
            Self::Directory(directory) => match std::fs::read(Self::file(directory, name)) {
                Ok(bytes) => Ok(Some(bytes)),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(_) => Err(Unreadable),
            },
            Self::Keychain { service } => {
                match security_framework::passwords::get_generic_password(service, name) {
                    Ok(bytes) => Ok(Some(bytes)),
                    // errSecItemNotFound
                    Err(error) if error.code() == -25300 => Ok(None),
                    Err(_) => Err(Unreadable),
                }
            }
            #[cfg(test)]
            Self::Memory(items) => Ok(items.lock().unwrap().get(name).cloned()),
        }
    }

    /// Save the item and read it back: it counts as saved only when what
    /// comes back is what was written.
    pub fn write(&self, name: &str, bytes: &[u8]) -> Result<(), String> {
        match self {
            Self::Directory(directory) => {
                crate::shared::write_private(&Self::file(directory, name), bytes)
                    .map_err(|_| "The session could not be saved".to_string())?
            }
            Self::Keychain { service } => {
                security_framework::passwords::set_generic_password(service, name, bytes)
                    .map_err(|_| "The session could not be saved in Keychain".to_string())?
            }
            #[cfg(test)]
            Self::Memory(items) => {
                items.lock().unwrap().insert(name.to_owned(), bytes.to_vec());
            }
        }
        match self.read(name) {
            Ok(Some(saved)) if saved == bytes => Ok(()),
            _ => Err("The session could not be read back after saving".into()),
        }
    }

    pub fn delete(&self, name: &str) {
        match self {
            Self::Directory(directory) => {
                let _ = std::fs::remove_file(Self::file(directory, name));
            }
            Self::Keychain { service } => {
                let _ = security_framework::passwords::delete_generic_password(service, name);
            }
            #[cfg(test)]
            Self::Memory(items) => {
                items.lock().unwrap().remove(name);
            }
        }
    }
}

/// What a session item holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedSession {
    pub url: String,
    /// Absent in the legacy item, which predates accounts.
    pub user_id: Option<String>,
    pub token: Secret,
}

impl SavedSession {
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
        Some(Self {
            url: value.get("url")?.as_str()?.to_owned(),
            user_id: value
                .get("userId")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned),
            token: Secret::new(value.get("token")?.as_str()?),
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut value = serde_json::json!({"url": self.url, "token": self.token.expose()});
        if let Some(user_id) = &self.user_id {
            value["userId"] = serde_json::json!(user_id);
        }
        serde_json::to_vec(&value).unwrap_or_default()
    }
}

/// `CEREAL_KEYCHAIN_PROBE=1`: what milestone P0 asks of a signed build
/// installed over the shipped app. Reads the legacy item, then writes, reads
/// back and deletes `account:probe` and `moves:probe` in the same service.
/// Prints only what happened, never a value. The service is the app's own
/// unless `CEREAL_KEYCHAIN_PROBE_SERVICE` names another (a scratch name, so
/// a development build can exercise the same calls without touching the
/// installed app's items).
pub fn probe() -> bool {
    let service = crate::environment::var("CEREAL_KEYCHAIN_PROBE_SERVICE")
        .ok()
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| SERVICE.to_owned());
    let store = SessionStore::Keychain {
        service: service.clone(),
    };
    // `CEREAL_KEYCHAIN_PROBE_SEED=1`, in a scratch service only: first put a
    // legacy item there as the build before would, so one process exercises
    // reading it too. Never in the app's own service.
    let seed = crate::environment::var("CEREAL_KEYCHAIN_PROBE_SEED").is_ok_and(|value| value == "1");
    if seed && service == SERVICE {
        println!("keychain probe: refused: seeding is for a scratch service only");
        return false;
    }
    if seed {
        let fake = SavedSession {
            url: "https://probe.invalid".into(),
            user_id: None,
            token: Secret::new(format!("probe-{}", uuid::Uuid::new_v4().simple())),
        };
        let seeded = store.write(LEGACY, &fake.encode()).is_ok();
        println!("keychain probe: seeded a legacy item: {}", if seeded { "ok" } else { "FAILED" });
    }
    let started = std::time::Instant::now();
    let legacy = match store.read(LEGACY) {
        Ok(Some(bytes)) => match SavedSession::decode(&bytes) {
            Some(session) if !session.token.is_empty() => "read ok",
            _ => "read ok, but not a session",
        },
        Ok(None) => "no legacy item",
        Err(Unreadable) => "READ FAILED",
    };
    let mut written = true;
    for name in ["account:probe", "moves:probe"] {
        let value = format!("{{\"probe\":\"{}\"}}", uuid::Uuid::new_v4());
        let saved = store.write(name, value.as_bytes()).is_ok();
        store.delete(name);
        let gone = matches!(store.read(name), Ok(None));
        println!(
            "keychain probe: {name}: write and read back {}, delete {}",
            if saved { "ok" } else { "FAILED" },
            if gone { "ok" } else { "FAILED" }
        );
        written &= saved && gone;
    }
    // A Keychain prompt blocks the call that raised it until it is answered:
    // calls that return at once cannot have shown one.
    let seconds = started.elapsed().as_secs_f32();
    if seed {
        store.delete(LEGACY);
        println!(
            "keychain probe: seeded item removed: {}",
            if matches!(store.read(LEGACY), Ok(None)) { "ok" } else { "FAILED" }
        );
    }
    println!("keychain probe: service {service}");
    println!("keychain probe: legacy item: {legacy}");
    println!(
        "keychain probe: write {}",
        if written { "ok" } else { "FAILED" }
    );
    println!(
        "keychain probe: took {seconds:.2}s; prompt seen: {}",
        if seconds < 1.5 && legacy != "READ FAILED" && written {
            "no"
        } else {
            "POSSIBLY (slow or failed calls; look at the screen)"
        }
    );
    legacy != "READ FAILED" && written
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_item_binds_the_token_to_its_address_and_user_and_never_prints_it() {
        let session = SavedSession {
            url: "https://notes.acme.com".into(),
            user_id: Some("0b7e1c9e-0000-4000-8000-000000000001".into()),
            token: Secret::new("a-token-that-must-not-be-printed"),
        };
        let decoded = SavedSession::decode(&session.encode()).unwrap();
        assert_eq!(decoded, session);
        assert!(!format!("{session:?}").contains("must-not-be-printed"));
        // The legacy item has no user.
        let legacy = SavedSession::decode(br#"{"url":"https://a.example","token":"t"}"#).unwrap();
        assert_eq!(legacy.user_id, None);
        assert!(SavedSession::decode(b"{}").is_none());
    }

    #[test]
    fn an_isolated_store_keeps_each_item_in_its_own_private_file() {
        let directory = std::env::temp_dir().join(format!(
            "cereal-session-store-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let store = SessionStore::Directory(directory.clone());
        assert_eq!(store.read(LEGACY), Ok(None));
        store.write(LEGACY, b"legacy").unwrap();
        store.write("account:3f0c", b"new").unwrap();
        store.write("moves:3f0c", b"[]").unwrap();
        assert_eq!(std::fs::read(directory.join("session.json")).unwrap(), b"legacy");
        assert_eq!(store.read("account:3f0c"), Ok(Some(b"new".to_vec())));
        assert_eq!(store.read("moves:3f0c"), Ok(Some(b"[]".to_vec())));
        store.delete("account:3f0c");
        assert_eq!(store.read("account:3f0c"), Ok(None));
        // Replacing the session leaves the moves item alone.
        assert_eq!(store.read("moves:3f0c"), Ok(Some(b"[]".to_vec())));
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(directory.join("session.json"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        let _ = std::fs::remove_dir_all(directory);
    }
}
