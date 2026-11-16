//! What is on disk from before accounts, and the plain files that let the
//! registry be rebuilt: `connection.json`, the legacy Keychain item, and the
//! `account.json` each account directory holds.
use super::{
    AccountId, Paths, canonical_user_id,
    keychain::{self, SavedSession, SessionStore},
    validate_url,
};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// `servers/<AccountId>/account.json`: written when the account is added.
/// Not secret, and not trusted: an address read from it is shown, never
/// followed by itself.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountFile {
    pub id: String,
    pub url: String,
    #[serde(rename = "userId")]
    pub user_id: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub label: String,
}

impl AccountFile {
    pub fn read(directory: &Path) -> Option<Self> {
        let path = directory.join("account.json");
        if std::fs::metadata(&path).ok()?.len() > 65_536 {
            return None;
        }
        let file: Self = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
        // The file describes the directory it is in, or it describes nothing.
        let name = directory.file_name()?.to_str()?;
        let user_id = canonical_user_id(&file.user_id)?;
        (file.id == name
            && user_id == file.user_id
            && validate_url(&file.url).as_deref() == Ok(file.url.as_str())
            && !file.url.contains('|')
            && AccountId::of(&file.url, &user_id).as_str() == name)
            .then_some(file)
    }

    pub fn write(&self, directory: &Path) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(self).map_err(|error| error.to_string())?;
        let path = directory.join("account.json");
        if std::fs::read(&path).ok().as_deref() == Some(bytes.as_slice()) {
            return Ok(());
        }
        crate::shared::write_private(&path, &bytes)
    }
}

/// Every account directory under `servers/`, by its checked name.
pub fn account_directories(paths: &Paths) -> Vec<(AccountId, std::path::PathBuf)> {
    let mut found: Vec<_> = std::fs::read_dir(paths.servers())
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter_map(|entry| {
            let id = AccountId::parse(entry.file_name().to_str()?)?;
            Some((id, entry.path()))
        })
        .collect();
    found.sort();
    found
}

/// The address `connection.json` holds, when it is one.
pub fn connection_address(paths: &Paths) -> Option<String> {
    let bytes = std::fs::read(paths.connection()).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let url = value.get("url")?.as_str()?;
    (validate_url(url).as_deref() == Ok(url) && !url.contains('|')).then(|| url.to_owned())
}

/// Keep `connection.json` in step with the default account (milestone 1:
/// installing the previous build by hand still works).
pub fn write_connection(paths: &Paths, url: &str) -> Result<(), String> {
    let bytes = serde_json::to_vec(&serde_json::json!({ "url": url })).unwrap_or_default();
    if std::fs::read(paths.connection()).ok().as_deref() == Some(bytes.as_slice()) {
        return Ok(());
    }
    crate::shared::write_private(&paths.connection(), &bytes)
        .map_err(|_| "Cannot save account preferences".into())
}

/// The legacy item, when it is a session.
pub fn legacy_session(store: &SessionStore) -> Option<SavedSession> {
    let bytes = store.read(keychain::LEGACY).ok()??;
    SavedSession::decode(&bytes).filter(|session| !session.token.is_empty())
}

/// Step 1 of the migration, which works offline. It applies only when there
/// is no registry **and** no account directory describes itself: then, if
/// `connection.json` has a valid address and the legacy item exists with the
/// same address, the one account of the build before is returned (its user
/// not yet known). Otherwise nothing: the app starts as a fresh or
/// signed-out install does.
pub fn first_account(paths: &Paths, store: &SessionStore) -> Option<String> {
    if paths.registry().exists() {
        return None;
    }
    if account_directories(paths)
        .iter()
        .any(|(_, directory)| directory.join("account.json").exists())
    {
        return None;
    }
    let url = connection_address(paths)?;
    let legacy = legacy_session(store)?;
    (legacy.url == url).then_some(url)
}

/// The accounts a lost registry can be rebuilt from: the directories that
/// describe themselves **and** whose Keychain item exists and agrees.
pub fn rebuildable(paths: &Paths, store: &SessionStore) -> Vec<AccountFile> {
    account_directories(paths)
        .into_iter()
        .filter_map(|(id, directory)| {
            let file = AccountFile::read(&directory)?;
            let item = store.read(&keychain::account_item(&id)).ok()??;
            let session = SavedSession::decode(&item)?;
            (session.url == file.url && session.user_id.as_deref() == Some(file.user_id.as_str()))
                .then_some(file)
        })
        .collect()
}
