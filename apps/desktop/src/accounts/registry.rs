//! The ordered list of accounts, its file (`accounts.json`, not secret) and
//! the sessions that go with it (one Keychain item per account).
//!
//! The file is written only when there is at least one account: absence
//! means "no accounts". A file that exists but cannot be read is not treated
//! as missing (see [`Registry::load`]).
use super::{
    Account, AccountClient, AccountId, AccountState, Blocked, Paths, VerifiedAccount,
    canonical_user_id, handle,
    keychain::{self, SavedSession, Secret, SessionStore},
    migration::{self, AccountFile},
    normalise_address, same_server, validate_url,
};
use crate::link::ServerOrigin;
use meeting_notes_api_types::AuthSessionDTO;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Colours are an index into a palette of six, repeating from the seventh
/// account. Colour is never the only distinction.
pub const PALETTE: u8 = 6;

#[derive(Debug, Serialize, Deserialize)]
struct RegistryFile {
    version: u32,
    #[serde(rename = "nextSlot")]
    next_slot: u16,
    default: Option<String>,
    chat: Option<String>,
    accounts: Vec<Entry>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Entry {
    id: String,
    slot: u16,
    url: String,
    #[serde(rename = "userId")]
    user_id: Option<String>,
    #[serde(default)]
    name: String,
    #[serde(default)]
    email: String,
    #[serde(default)]
    label: String,
    #[serde(default)]
    color: u8,
    #[serde(default, rename = "addedAt")]
    added_at: String,
}

pub enum RegistryError {
    /// The registry exists and cannot be read. It was copied aside and
    /// rebuilt from the account directories whose Keychain item exists; it
    /// was **not** migrated again from `connection.json`.
    Unreadable {
        rebuilt: Box<Registry>,
        aside: Option<std::path::PathBuf>,
    },
}

/// How a sign-in was taken in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// A new account, or the migrated one now that its user is known.
    Added,
    /// An account that was already there: its token was replaced, its stored
    /// address and directory were not touched.
    TokenReplaced,
}

/// A session to end on its own server, in the background.
pub struct Ended {
    pub client: AccountClient,
}

pub struct Accepted {
    pub id: AccountId,
    pub outcome: Outcome,
    /// Sessions this sign-in replaced.
    pub ended: Vec<Ended>,
    /// Accounts that were signed out of to make room (one account at a
    /// time until several are switched on). Their data stays on this Mac.
    pub departed: Vec<Account>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum AddError {
    Invalid(String),
    Blocked(Blocked),
    /// The session could not be saved, so nothing was added.
    Store(String),
    /// "Sign in again" came back as someone else. Nothing was changed.
    DifferentUser { signed_in_as: String, account: String },
}

impl AddError {
    pub fn message(&self) -> String {
        match self {
            Self::Invalid(message) | Self::Store(message) => message.clone(),
            Self::Blocked(blocked) => blocked.message().into(),
            Self::DifferentUser {
                signed_in_as,
                account,
            } => format!("You signed in as {signed_in_as}; this account is {account}."),
        }
    }
}

pub struct Departed {
    pub account: Account,
    /// To end the session on its own server, in the background.
    pub client: Option<AccountClient>,
}

pub struct Registry {
    accounts: Vec<Account>,
    default: Option<AccountId>,
    chat: Option<AccountId>,
    next_slot: u16,
    paths: Option<Paths>,
    store: SessionStore,
    /// Accounts given for this run only (`CEREAL_SERVER_TOKEN`, the debug
    /// file): nothing is saved and saved accounts are not loaded.
    ephemeral: bool,
    tokens: HashMap<AccountId, Secret>,
    sessions: HashMap<AccountId, AuthSessionDTO>,
    /// `CEREAL_SERVER_URL`: the address this run is for.
    override_url: Option<String>,
    /// Saved accounts on another address than the one this run is for. They
    /// take no part in the run; a sign-in during it replaces them.
    set_aside: Vec<Account>,
    /// The address of the last session, for the sign-in form.
    last_url: Option<String>,
    /// A legacy item whose token is no account's: left by an old build run
    /// by hand. Verified once; it can replace a token, never add an account.
    stray: Option<SavedSession>,
    legacy_present: bool,
    /// Debug: the slot of the account new notes go to instead of the
    /// default. By slot: an id changes once the account's user is known.
    debug_new_note_slot: Option<u16>,
    /// What went wrong reading the registry, to be shown once.
    pub notice: Option<String>,
}

impl Registry {
    pub fn empty(paths: Option<Paths>, store: SessionStore) -> Self {
        Self {
            accounts: Vec::new(),
            default: None,
            chat: None,
            next_slot: 1,
            paths,
            store,
            ephemeral: false,
            tokens: HashMap::new(),
            sessions: HashMap::new(),
            override_url: None,
            set_aside: Vec::new(),
            last_url: None,
            stray: None,
            legacy_present: false,
            debug_new_note_slot: None,
            notice: None,
        }
    }

    /// The registry of this process, from its environment.
    pub fn of_environment() -> Self {
        // Unit tests never read this user's own registry or Keychain.
        #[cfg(test)]
        if true {
            return Self::empty(None, SessionStore::memory());
        }
        let store = SessionStore::of_environment();
        let paths = Paths::of_environment();
        let override_url = crate::environment::var("CEREAL_SERVER_URL")
            .ok()
            .filter(|url| !url.is_empty());
        // One account for this run only; saved accounts are not loaded.
        if let Ok(token) = crate::environment::var("CEREAL_SERVER_TOKEN") {
            let mut registry = Self::empty(paths, store);
            registry.ephemeral = true;
            registry.override_url = override_url.clone();
            if let Some(url) = override_url.as_deref().and_then(|url| validate_url(url).ok()) {
                registry.push_ephemeral(url, Secret::new(token));
            }
            return registry;
        }
        #[cfg(debug_assertions)]
        if let Some(mut registry) = Self::of_debug_file(paths.clone(), store.clone()) {
            registry.override_url = override_url;
            return registry;
        }
        let Some(paths) = paths else {
            return Self::empty(None, store);
        };
        let mut registry = match Self::load(paths, store) {
            Ok(registry) => registry,
            Err(RegistryError::Unreadable { rebuilt, aside }) => {
                let mut registry = *rebuilt;
                registry.notice = Some(match aside {
                    Some(aside) => format!(
                        "The list of accounts could not be read. It was kept as {} and rebuilt.",
                        aside.file_name().unwrap_or_default().to_string_lossy()
                    ),
                    None => "The list of accounts could not be read, and was rebuilt.".into(),
                });
                registry
            }
        };
        if let Some(url) = override_url.as_deref().and_then(|url| validate_url(url).ok()) {
            // An address given for this run must never send a saved session
            // to another server.
            let (kept, aside): (Vec<_>, Vec<_>) = std::mem::take(&mut registry.accounts)
                .into_iter()
                .partition(|account| account.url == url);
            registry.accounts = kept;
            registry.set_aside = aside;
            if registry
                .default
                .as_ref()
                .is_some_and(|id| registry.get(id).is_none())
            {
                registry.default = registry.accounts.first().map(|account| account.id.clone());
            }
        }
        registry.override_url = override_url;
        registry
    }

    fn push_ephemeral(&mut self, url: String, token: Secret) -> AccountId {
        let id = AccountId::pending(&format!("{}#{}", url, self.next_slot));
        let account = Account::new(id.clone(), self.next_slot, url);
        self.next_slot += 1;
        self.tokens.insert(id.clone(), token);
        self.default.get_or_insert(id.clone());
        self.accounts.push(account);
        id
    }

    /// `CEREAL_DEBUG_ACCOUNTS=<file>`: a JSON list of `{url, token}`, each
    /// verified by `/session` like any account. For this run only.
    /// `CEREAL_DEBUG_NEW_NOTE_ACCOUNT=<n>` (1-based) sends new notes to the
    /// n-th of them instead of the first.
    #[cfg(debug_assertions)]
    fn of_debug_file(paths: Option<Paths>, store: SessionStore) -> Option<Self> {
        let file = crate::environment::var_os("CEREAL_DEBUG_ACCOUNTS")?;
        let entries: Vec<serde_json::Value> = serde_json::from_slice(&std::fs::read(file).ok()?).ok()?;
        let mut registry = Self::empty(paths, store);
        registry.ephemeral = true;
        for entry in entries {
            let (Some(url), Some(token)) = (
                entry.get("url").and_then(|v| v.as_str()).and_then(|url| validate_url(url).ok()),
                entry.get("token").and_then(|v| v.as_str()),
            ) else {
                continue;
            };
            registry.push_ephemeral(url, Secret::new(token));
        }
        let chosen = crate::environment::var("CEREAL_DEBUG_NEW_NOTE_ACCOUNT")
            .ok()
            .and_then(|n| n.parse::<usize>().ok())
            .and_then(|n| registry.accounts.get(n.checked_sub(1)?))
            .map(|account| account.slot);
        // Kept by slot: the id changes once the account's user is known.
        registry.debug_new_note_slot = chosen;
        Some(registry)
    }

    /// Read the registry, migrating from the build before accounts when
    /// that applies. A registry that exists and cannot be read is an error,
    /// not "empty": it is copied aside and rebuilt.
    pub fn load(paths: Paths, store: SessionStore) -> Result<Self, RegistryError> {
        let mut registry = Self::empty(Some(paths.clone()), store.clone());
        registry.last_url = migration::connection_address(&paths);
        registry.legacy_present = migration::legacy_session(&store).is_some();
        let file = match std::fs::read(paths.registry()) {
            Ok(bytes) => match Self::decode(&bytes) {
                Some(file) => Some(file),
                None => return Err(registry.rebuild(Some(bytes))),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(_) => return Err(registry.rebuild(None)),
        };
        match file {
            Some(file) => registry.adopt(file),
            None => {
                // Step 1 of the migration. Offline: the user is not known
                // yet, so the entry has no directory until `/session` says.
                if let Some(url) = migration::first_account(&paths, &store) {
                    let mut account = Account::new(AccountId::pending(&url), 1, url);
                    account.state = AccountState::Checking;
                    registry.default = Some(account.id.clone());
                    registry.accounts.push(account);
                    registry.next_slot = 2;
                    let _ = registry.save();
                }
            }
        }
        registry.load_sessions();
        Ok(registry)
    }

    fn decode(bytes: &[u8]) -> Option<RegistryFile> {
        let file: RegistryFile = serde_json::from_slice(bytes).ok()?;
        if file.version != 1 || file.accounts.is_empty() {
            return None;
        }
        let mut slots = std::collections::HashSet::new();
        for entry in &file.accounts {
            let valid = (1..=handle::MAX_SLOT).contains(&entry.slot)
                && slots.insert(entry.slot)
                && entry.slot < file.next_slot
                && validate_url(&entry.url).as_deref() == Ok(entry.url.as_str())
                && !entry.url.contains('|')
                && match &entry.user_id {
                    // The id is the directory: it must be the one this
                    // address and user give, written the one way.
                    Some(user_id) => {
                        canonical_user_id(user_id).as_deref() == Some(user_id.as_str())
                            && AccountId::parse(&entry.id)
                                == Some(AccountId::of(&entry.url, user_id))
                    }
                    None => true,
                };
            if !valid {
                return None;
            }
        }
        Some(file)
    }

    fn adopt(&mut self, file: RegistryFile) {
        self.next_slot = file.next_slot;
        for entry in file.accounts {
            let id = match &entry.user_id {
                Some(user_id) => AccountId::of(&entry.url, user_id),
                None => AccountId::pending(&entry.url),
            };
            let mut account = Account::new(id, entry.slot, entry.url);
            account.user_id = entry.user_id;
            account.name = entry.name;
            account.email = entry.email;
            if !entry.label.trim().is_empty() {
                account.label = entry.label;
            }
            account.color = entry.color % PALETTE;
            if !entry.added_at.is_empty() {
                account.added_at = entry.added_at;
            }
            self.accounts.push(account);
        }
        let known = |id: &Option<String>| {
            id.as_deref()
                .and_then(AccountId::parse)
                .filter(|id| self.accounts.iter().any(|account| &account.id == id))
        };
        self.chat = known(&file.chat);
        self.default = known(&file.default)
            .or_else(|| self.accounts.first().map(|account| account.id.clone()));
    }

    /// The registry cannot be read: copy it aside, rebuild it from the
    /// account directories whose Keychain item exists, and say so.
    fn rebuild(mut self, bytes: Option<Vec<u8>>) -> RegistryError {
        let Some(paths) = self.paths.clone() else {
            return RegistryError::Unreadable {
                rebuilt: Box::new(self),
                aside: None,
            };
        };
        let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%S%3fZ");
        let aside = paths.meta.join(format!("accounts.unreadable-{stamp}.json"));
        let copied = match bytes {
            Some(bytes) => crate::shared::write_private(&aside, &bytes).is_ok(),
            None => std::fs::rename(paths.registry(), &aside).is_ok(),
        };
        for file in migration::rebuildable(&paths, &self.store) {
            let Some(slot) = self.take_slot() else { break };
            let id = AccountId::of(&file.url, &file.user_id);
            let mut account = Account::new(id, slot, file.url);
            account.user_id = Some(file.user_id);
            account.email = file.email;
            if !file.label.trim().is_empty() {
                account.label = file.label;
            }
            account.color = ((slot - 1) % u16::from(PALETTE)) as u8;
            self.accounts.push(account);
        }
        self.default = self.accounts.first().map(|account| account.id.clone());
        // Written only when there is an account; otherwise the unreadable
        // file is gone (copied aside) and absence means "no accounts".
        if copied || self.accounts.is_empty() {
            let _ = std::fs::remove_file(paths.registry());
        }
        let _ = self.save();
        self.load_sessions();
        RegistryError::Unreadable {
            rebuilt: Box::new(self),
            aside: copied.then_some(aside),
        }
    }

    /// Read each account's session item. An item is used only if its address
    /// and user equal the registry entry's; otherwise the account needs a
    /// sign-in and nothing is sent.
    fn load_sessions(&mut self) {
        let legacy = migration::legacy_session(&self.store);
        for account in &mut self.accounts {
            let token = match &account.user_id {
                Some(user_id) => self
                    .store
                    .read(&keychain::account_item(&account.id))
                    .ok()
                    .flatten()
                    .and_then(|bytes| SavedSession::decode(&bytes))
                    .filter(|saved| {
                        saved.url == account.url && saved.user_id.as_deref() == Some(user_id.as_str())
                    })
                    .map(|saved| saved.token),
                // The migrated account, before its user is known: the token
                // still in the legacy item, if it is for this address.
                None => legacy
                    .as_ref()
                    .filter(|saved| saved.url == account.url)
                    .map(|saved| saved.token.clone()),
            };
            match token.filter(|token| !token.is_empty()) {
                Some(token) => {
                    account.state = AccountState::Checking;
                    self.tokens.insert(account.id.clone(), token);
                }
                None => account.state = AccountState::NeedsSignIn,
            }
        }
        // A legacy item whose token is no account's (an old build run by
        // hand signed in). It is verified once and can only ever replace the
        // token of an account that is already here.
        self.stray = legacy.filter(|saved| {
            self.accounts.iter().any(|account| account.user_id.is_some())
                && !self.tokens.values().any(|token| *token == saved.token)
        });
    }

    fn take_slot(&mut self) -> Option<u16> {
        (self.next_slot <= handle::MAX_SLOT).then(|| {
            self.next_slot += 1;
            self.next_slot - 1
        })
    }

    /// Write the registry: atomically, readable by this user only, and only
    /// when there is at least one account.
    pub fn save(&self) -> Result<(), String> {
        super::touch();
        if self.ephemeral {
            return Ok(());
        }
        let Some(paths) = &self.paths else {
            return Ok(());
        };
        let listed: Vec<&Account> = self.accounts.iter().chain(&self.set_aside).collect();
        if listed.is_empty() {
            return match std::fs::remove_file(paths.registry()) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                    Err("Cannot save the list of accounts".into())
                }
                _ => Ok(()),
            };
        }
        let file = RegistryFile {
            version: 1,
            next_slot: self.next_slot,
            default: self.default.as_ref().map(|id| id.as_str().to_owned()),
            chat: self.chat.as_ref().map(|id| id.as_str().to_owned()),
            accounts: listed
                .into_iter()
                .map(|account| Entry {
                    id: account.id.as_str().to_owned(),
                    slot: account.slot,
                    url: account.url.clone(),
                    user_id: account.user_id.clone(),
                    name: account.name.clone(),
                    email: account.email.clone(),
                    label: account.label.clone(),
                    color: account.color,
                    added_at: account.added_at.clone(),
                })
                .collect(),
        };
        let bytes = serde_json::to_vec_pretty(&file).map_err(|error| error.to_string())?;
        crate::shared::write_private(&paths.registry(), &bytes)
            .map_err(|_| "Cannot save the list of accounts".into())
    }

    // -- Reading ---------------------------------------------------------

    pub fn accounts(&self) -> &[Account] {
        &self.accounts
    }
    pub fn get(&self, id: &AccountId) -> Option<&Account> {
        self.accounts.iter().find(|account| &account.id == id)
    }
    fn get_mut(&mut self, id: &AccountId) -> Option<&mut Account> {
        self.accounts.iter_mut().find(|account| &account.id == id)
    }
    pub fn by_slot(&self, slot: u16) -> Option<&Account> {
        self.accounts.iter().find(|account| account.slot == slot)
    }
    /// The account a note in memory belongs to: its handle's slot.
    pub fn of_note(&self, note_id: u64) -> Option<&Account> {
        self.by_slot(handle::slot_of(note_id)?)
    }
    /// The accounts on a link's origin with no path in their address, in
    /// account order.
    pub fn on(&self, origin: &ServerOrigin) -> Vec<&Account> {
        self.accounts
            .iter()
            .filter(|account| origin.is_server(&account.url))
            .collect()
    }
    pub fn default(&self) -> Option<&Account> {
        self.default
            .as_ref()
            .and_then(|id| self.get(id))
            .or_else(|| self.accounts.first())
    }
    /// Where a new note with no calendar event goes: the default account
    /// (a debug run may name another).
    pub fn new_note_account(&self) -> Option<&Account> {
        self.debug_new_note_slot
            .and_then(|slot| self.by_slot(slot))
            .or_else(|| self.default())
    }
    pub fn session(&self, id: &AccountId) -> Option<&AuthSessionDTO> {
        self.sessions.get(id)
    }
    pub fn paths(&self) -> Option<&Paths> {
        self.paths.as_ref()
    }
    pub fn store(&self) -> &SessionStore {
        &self.store
    }
    /// The address given for this run, else the default account's, else
    /// the one the last session was for.
    pub fn configured_url(&self) -> Option<String> {
        self.override_url
            .clone()
            .or_else(|| self.default().map(|account| account.url.clone()))
            .or_else(|| self.last_url.clone())
    }
    pub fn has_saved_session(&self) -> bool {
        !self.accounts.is_empty() || self.legacy_present
    }
    pub fn is_ephemeral(&self) -> bool {
        self.ephemeral
    }

    /// An account's directory: only once its user is known.
    pub fn directory(&self, id: &AccountId) -> Option<std::path::PathBuf> {
        let account = self.get(id)?;
        account.user_id.as_ref()?;
        Some(self.paths.as_ref()?.account(&account.id))
    }

    /// The account's client. It can only reach the account's own origin,
    /// and exists only while the account has a session that may be used.
    pub fn client(&self, id: &AccountId) -> Result<AccountClient, String> {
        let account = self.get(id).ok_or("Sign in to your server first")?;
        if account.state == AccountState::NeedsSignIn {
            return Err(format!("Sign in to {} again", account.host()));
        }
        let token = self
            .tokens
            .get(id)
            .cloned()
            .ok_or("Sign in to your server first")?;
        let storage = account
            .user_id
            .as_ref()
            .and_then(|_| self.paths.as_ref())
            .map(|paths| paths.recordings(&account.id));
        AccountClient::new(account, token, storage)
    }

    /// What is needed to ask an account's server whose session it holds.
    pub fn verification(&self, id: &AccountId) -> Option<(String, Secret)> {
        Some((self.get(id)?.url.clone(), self.tokens.get(id)?.clone()))
    }

    // -- Changing --------------------------------------------------------

    pub fn set_state(&mut self, id: &AccountId, state: AccountState) {
        if let Some(account) = self.get_mut(id)
            && account.state != state
        {
            account.state = state;
            super::touch();
        }
    }

    /// The account's own `/session` said the session has ended. Its epoch
    /// moves, so everything on its way to that server stops. Its notes stay
    /// loaded and are written as usual; nothing is deleted.
    pub fn session_ended(&mut self, id: &AccountId) {
        if let Some(account) = self.get_mut(id) {
            account.bump_epoch();
            account.state = AccountState::NeedsSignIn;
            super::touch();
        }
    }

    /// The saved session was refused when it was verified at launch. It is
    /// not tried again; for the migrated account (whose token is still the
    /// legacy item's) that item is discarded.
    pub fn session_refused(&mut self, id: &AccountId) {
        let provisional = self.get(id).is_some_and(|account| account.user_id.is_none());
        self.session_ended(id);
        if provisional {
            self.tokens.remove(id);
            if !self.ephemeral {
                self.store.delete(keychain::LEGACY);
                self.legacy_present = false;
            }
        }
    }

    /// `/session` confirmed an account's saved session at launch (or again
    /// after it could not be reached). For the migrated account this is
    /// step 3: its user becomes known, so its directory does, `account.json`
    /// is written, and the new Keychain item is written and read back.
    /// Returns the account's id, which changes exactly then.
    pub fn verified(
        &mut self,
        id: &AccountId,
        session: AuthSessionDTO,
        features: Vec<String>,
    ) -> Result<AccountId, String> {
        let user_id = canonical_user_id(&session.user.id).ok_or("Invalid account identity")?;
        let index = self
            .accounts
            .iter()
            .position(|account| &account.id == id)
            .ok_or("That account is no longer here")?;
        let known = self.accounts[index].user_id.clone();
        let id = match known {
            Some(known) if known != user_id => {
                // The token is someone else's now. Nothing is sent with it.
                self.session_ended(id);
                return Err("The saved session belongs to another user. Sign in again.".into());
            }
            Some(_) => id.clone(),
            None => {
                let url = self.accounts[index].url.clone();
                let new_id = AccountId::of(&url, &user_id);
                let token = self.tokens.remove(id).ok_or("Sign in to your server first")?;
                self.tokens.insert(new_id.clone(), token.clone());
                if self.default.as_ref() == Some(id) {
                    self.default = Some(new_id.clone());
                }
                if self.chat.as_ref() == Some(id) {
                    self.chat = Some(new_id.clone());
                }
                let account = &mut self.accounts[index];
                account.id = new_id.clone();
                account.user_id = Some(user_id.clone());
                // On disk the entry stays provisional until the new item
                // has been written and read back, so a crash anywhere here
                // repeats this step with the legacy token still in place.
                if !self.ephemeral {
                    self.write_item(&new_id, &url, &user_id, &token)?;
                }
                new_id
            }
        };
        let account = &mut self.accounts[index];
        account.name = session.user.name.clone();
        account.email = session.user.email.clone();
        account.features = features;
        account.state = AccountState::Ready;
        account.verified_this_launch = true;
        self.sessions.insert(id.clone(), session);
        self.write_account_file(&id);
        self.save()?;
        Ok(id)
    }

    fn write_item(&self, id: &AccountId, url: &str, user_id: &str, token: &Secret) -> Result<(), String> {
        self.store.write(
            &keychain::account_item(id),
            &SavedSession {
                url: url.to_owned(),
                user_id: Some(user_id.to_owned()),
                token: token.clone(),
            }
            .encode(),
        )
    }

    fn write_account_file(&self, id: &AccountId) {
        if self.ephemeral {
            return;
        }
        let (Some(account), Some(paths)) = (self.get(id), &self.paths) else {
            return;
        };
        let Some(user_id) = &account.user_id else {
            return;
        };
        let _ = AccountFile {
            id: id.as_str().to_owned(),
            url: account.url.clone(),
            user_id: user_id.clone(),
            email: account.email.clone(),
            label: account.label.clone(),
        }
        .write(&paths.account(id));
    }

    /// Milestone 1 keeps the legacy item and `connection.json` in step with
    /// the default account, so the previous build, installed by hand, still
    /// finds its session.
    fn sync_legacy(&mut self) {
        if self.ephemeral {
            return;
        }
        let Some(paths) = self.paths.clone() else {
            return;
        };
        let Some(account) = self.default().filter(|account| account.user_id.is_some()) else {
            return;
        };
        let Some(token) = self.tokens.get(&account.id) else {
            return;
        };
        let wanted = SavedSession {
            url: account.url.clone(),
            user_id: None,
            token: token.clone(),
        };
        let current = migration::legacy_session(&self.store);
        if current.as_ref() != Some(&wanted) {
            let _ = self.store.write(keychain::LEGACY, &wanted.encode());
        }
        let _ = migration::write_connection(&paths, &account.url);
        self.last_url = Some(account.url.clone());
        self.legacy_present = true;
        self.stray = None;
    }

    /// The address a new account is stored under. An earlier cache made
    /// under the address as typed (`https://Notes.Acme.com`), or one whose
    /// own `account.json` names this server and user, is found again by
    /// storing that address; otherwise the address is stored normalised.
    fn stored_address(&self, typed: &str, user_id: &str) -> String {
        let Some(paths) = &self.paths else {
            return normalise_address(typed);
        };
        // Directories from before accounts do not say whose they are: the
        // address as typed, and the address the build before kept in
        // `connection.json`, are the ones that may have named them.
        let earlier = self
            .last_url
            .iter()
            .filter(|url| same_server(url, typed))
            .map(String::as_str);
        for candidate in std::iter::once(typed).chain(earlier) {
            if paths.account(&AccountId::of(candidate, user_id)).is_dir() {
                return candidate.to_owned();
            }
        }
        migration::account_directories(paths)
            .into_iter()
            .filter_map(|(_, directory)| AccountFile::read(&directory))
            .find(|file| file.user_id == user_id && same_server(&file.url, typed))
            .map(|file| file.url)
            .unwrap_or_else(|| normalise_address(typed))
    }

    /// Take in a sign-in. `multiple` is the switch for several accounts;
    /// while it is off, whoever signs in is the account, and an account that
    /// is someone else is signed out of first (its data stays). `busy` says
    /// why an account cannot be signed out of right now, if it cannot.
    pub fn accept(
        &mut self,
        verified: VerifiedAccount,
        multiple: bool,
        busy: &dyn Fn(&Account) -> Option<Blocked>,
    ) -> Result<Accepted, AddError> {
        let url = validate_url(&verified.url).map_err(AddError::Invalid)?;
        if url.contains('|') {
            return Err(AddError::Invalid("Enter a valid server URL".into()));
        }
        let user_id = canonical_user_id(&verified.session.user.id)
            .ok_or_else(|| AddError::Invalid("Invalid account identity".into()))?;
        // The same account, signed in to again.
        if let Some(id) = self
            .accounts
            .iter()
            .find(|account| account.is(&url, &user_id))
            .map(|account| account.id.clone())
        {
            let ended = self.replace_token(&id, verified.token, verified.session)?;
            return Ok(Accepted {
                id,
                outcome: Outcome::TokenReplaced,
                ended: ended.into_iter().collect(),
                departed: Vec::new(),
            });
        }
        // The migrated account, whose user was not known yet.
        if let Some(id) = self
            .accounts
            .iter()
            .find(|account| account.user_id.is_none() && same_server(&account.url, &url))
            .map(|account| account.id.clone())
        {
            self.tokens.insert(id.clone(), verified.token);
            let id = self
                .verified(&id, verified.session, Vec::new())
                .map_err(AddError::Store)?;
            self.sync_legacy();
            return Ok(Accepted {
                id,
                outcome: Outcome::Added,
                ended: Vec::new(),
                departed: Vec::new(),
            });
        }
        // Someone else. Until several accounts are switched on, whoever
        // signs in is the account.
        let mut ended = Vec::new();
        let mut departed = Vec::new();
        if !multiple {
            if let Some(blocked) = self.accounts.iter().find_map(busy) {
                return Err(AddError::Blocked(blocked));
            }
            let others: Vec<AccountId> = self
                .accounts
                .iter()
                .chain(&self.set_aside)
                .map(|account| account.id.clone())
                .collect();
            for other in others {
                if let Ok(Some(gone)) = self.sign_out(&other, None, |_| {}) {
                    ended.extend(gone.client.map(|client| Ended { client }));
                    departed.push(gone.account);
                }
            }
        }
        let stored = self.stored_address(&url, &user_id);
        let id = AccountId::of(&stored, &user_id);
        let slot = self
            .take_slot()
            .ok_or_else(|| AddError::Invalid("No more accounts can be added on this Mac".into()))?;
        // The session is saved (and read back) before the account is
        // listed: an account in the registry always has had its item.
        if !self.ephemeral {
            self.write_item(&id, &stored, &user_id, &verified.token)
                .map_err(AddError::Store)?;
        }
        let mut account = Account::new(id.clone(), slot, stored);
        account.user_id = Some(user_id);
        account.name = verified.session.user.name.clone();
        account.email = verified.session.user.email.clone();
        account.color = (self.accounts.len() % usize::from(PALETTE)) as u8;
        account.state = AccountState::Ready;
        account.verified_this_launch = true;
        self.accounts.push(account);
        self.tokens.insert(id.clone(), verified.token);
        self.sessions.insert(id.clone(), verified.session);
        if self.default().is_none() || self.default.is_none() {
            self.default = Some(id.clone());
        }
        self.write_account_file(&id);
        self.save().map_err(AddError::Store)?;
        self.sync_legacy();
        Ok(Accepted {
            id,
            outcome: Outcome::Added,
            ended,
            departed,
        })
    }

    /// "Sign in again" for one account, with its address fixed. A result
    /// that is a different user is refused and changes nothing.
    pub fn sign_in_again(
        &mut self,
        id: &AccountId,
        verified: VerifiedAccount,
    ) -> Result<Accepted, AddError> {
        let account = self
            .get(id)
            .ok_or_else(|| AddError::Invalid("That account is no longer here".into()))?;
        let user_id = canonical_user_id(&verified.session.user.id)
            .ok_or_else(|| AddError::Invalid("Invalid account identity".into()))?;
        if account.user_id.is_some() && !account.is(&verified.url, &user_id) {
            return Err(AddError::DifferentUser {
                signed_in_as: verified.session.user.email.clone(),
                account: account.email.clone(),
            });
        }
        self.accept(verified, true, &|_| None)
    }

    /// Replace an account's token. The stored address is kept unchanged and
    /// written into the new item, so the account's id and directory do not
    /// change. Returns the old session, to be ended on its server.
    fn replace_token(
        &mut self,
        id: &AccountId,
        token: Secret,
        session: AuthSessionDTO,
    ) -> Result<Option<Ended>, AddError> {
        let account = self
            .get(id)
            .ok_or_else(|| AddError::Invalid("That account is no longer here".into()))?
            .clone();
        let user_id = account
            .user_id
            .clone()
            .ok_or_else(|| AddError::Invalid("That account is not verified yet".into()))?;
        let old = self.tokens.get(id).cloned().filter(|old| *old != token);
        // The client of the old session is made before the epoch moves, so
        // it can still end that session on the server.
        let ended = old.and_then(|old| {
            self.client_with(&account, old).ok().map(|client| Ended { client })
        });
        if !self.ephemeral {
            self.write_item(id, &account.url, &user_id, &token)
                .map_err(AddError::Store)?;
        }
        // Work on its way with the old token stops; it is queued again when
        // the account is Ready.
        account.bump_epoch();
        self.tokens.insert(id.clone(), token);
        let entry = self.get_mut(id).expect("the account was just read");
        entry.name = session.user.name.clone();
        entry.email = session.user.email.clone();
        entry.state = AccountState::Ready;
        entry.verified_this_launch = true;
        self.sessions.insert(id.clone(), session);
        self.write_account_file(id);
        self.save().map_err(AddError::Store)?;
        self.sync_legacy();
        Ok(ended)
    }

    fn client_with(&self, account: &Account, token: Secret) -> Result<AccountClient, String> {
        // Bound to no epoch: its one job is to end a session.
        AccountClient::new(account, token, None)
    }

    /// Sign out of an account: the session ends; everything on this device
    /// stays. In order: the epoch moves; `cleanup` withdraws its reminders,
    /// closes its screens, drops its notes from memory and marks its slot
    /// dead; its Keychain item is deleted (and the legacy item, if it holds
    /// the same token); it leaves the registry. The caller then ends the
    /// session on its own server, in the background.
    pub fn sign_out(
        &mut self,
        id: &AccountId,
        blocked: Option<Blocked>,
        cleanup: impl FnOnce(&Account),
    ) -> Result<Option<Departed>, Blocked> {
        if let Some(blocked) = blocked {
            return Err(blocked);
        }
        let listed = self.accounts.iter().position(|account| &account.id == id);
        let aside = self.set_aside.iter().position(|account| &account.id == id);
        let account = match (listed, aside) {
            (Some(index), _) => self.accounts[index].clone(),
            (None, Some(index)) => self.set_aside[index].clone(),
            // Not here: nothing to do, and nothing to refuse.
            (None, None) => return Ok(None),
        };
        let token = self.tokens.get(id).cloned();
        let client = token
            .clone()
            .and_then(|token| self.client_with(&account, token).ok());
        account.bump_epoch();
        cleanup(&account);
        if !self.ephemeral {
            self.store.delete(&keychain::account_item(id));
            // The legacy item goes with the account it is in step with.
            let legacy = migration::legacy_session(&self.store);
            if legacy.is_some_and(|legacy| {
                Some(&legacy.token) == token.as_ref()
                    || (token.is_none() && legacy.url == account.url)
            }) {
                self.store.delete(keychain::LEGACY);
                self.legacy_present = false;
            }
        }
        self.tokens.remove(id);
        self.sessions.remove(id);
        self.accounts.retain(|account| &account.id != id);
        self.set_aside.retain(|account| &account.id != id);
        if self.default.as_ref() == Some(id) {
            // With exactly one remaining, it becomes the default.
            self.default = self.accounts.first().map(|account| account.id.clone());
        }
        if self.chat.as_ref() == Some(id) {
            self.chat = None;
        }
        if !self.ephemeral {
            if self.accounts.is_empty()
                && self.set_aside.is_empty()
                && let Some(paths) = &self.paths
            {
                // As signing out always did.
                let _ = std::fs::remove_file(paths.connection());
            }
            let _ = self.save();
            self.sync_legacy();
        } else {
            super::touch();
        }
        Ok(Some(Departed { account, client }))
    }

    /// Remove an account and erase its data from this Mac: sign out, then
    /// delete its directory. The notes on its server are not affected.
    pub fn erase(
        &mut self,
        id: &AccountId,
        blocked: Option<Blocked>,
        cleanup: impl FnOnce(&Account),
    ) -> Result<Option<Departed>, Blocked> {
        let directory = self.directory(id);
        let departed = self.sign_out(id, blocked, cleanup)?;
        if let Some(directory) = directory {
            erase_directory(&directory);
        }
        Ok(departed)
    }

    /// The legacy item an old build left, to be verified once (step 5).
    pub fn take_stray(&mut self) -> Option<(String, Secret)> {
        self.stray.take().map(|saved| (saved.url, saved.token))
    }

    /// What verifying the stray legacy item came to. A session for an
    /// account that is already here replaces that account's token; anything
    /// else is ignored and deleted. It never adds an account: any process of
    /// the same user can create an item with that name.
    pub fn settle_stray(
        &mut self,
        url: &str,
        token: Secret,
        session: Option<AuthSessionDTO>,
    ) -> Option<Accepted> {
        let known = session.and_then(|session| {
            let user_id = canonical_user_id(&session.user.id)?;
            let id = self
                .accounts
                .iter()
                .find(|account| account.is(url, &user_id))?
                .id
                .clone();
            Some((id, session))
        });
        let accepted = known.and_then(|(id, session)| {
            let ended = self.replace_token(&id, token, session).ok()?;
            Some(Accepted {
                id,
                outcome: Outcome::TokenReplaced,
                ended: ended.into_iter().collect(),
                departed: Vec::new(),
            })
        });
        if accepted.is_none() && !self.ephemeral {
            self.store.delete(keychain::LEGACY);
            self.legacy_present = false;
            // Back in step with the default account.
            self.sync_legacy();
        }
        accepted
    }
}

/// Delete an account's directory. Its name was checked as an account id
/// when the path was built; anything that is not directly under `servers`
/// is refused all the same.
pub fn erase_directory(directory: &std::path::Path) -> bool {
    let named = directory
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(AccountId::parse)
        .is_some();
    let under_servers = directory
        .parent()
        .and_then(|parent| parent.file_name())
        .is_some_and(|name| name == "servers");
    if !named || !under_servers || directory.is_symlink() {
        return false;
    }
    std::fs::remove_dir_all(directory).is_ok() || !directory.exists()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::accounts::{AccountState, Paths, keychain::SavedSession};

    pub const USER: &str = "0b7e1c9e-5a44-4c1d-9d2e-3f6a7b8c9d01";
    pub const OTHER: &str = "0b7e1c9e-5a44-4c1d-9d2e-3f6a7b8c9d02";
    pub const TOKEN: &str = "fictional-session-token-0000000000000001";
    pub const TOKEN_2: &str = "fictional-session-token-0000000000000002";

    pub fn root(name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("cereal-accounts-{name}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    pub fn session(user: &str, email: &str) -> AuthSessionDTO {
        serde_json::from_value(serde_json::json!({
            "expiresAt": null,
            "user": {"createdAt": "2026-11-01T00:00:00Z", "email": email, "id": user, "name": "Sam Carter", "role": "owner"}
        }))
        .unwrap()
    }

    pub fn verified(url: &str, user: &str, token: &str) -> VerifiedAccount {
        VerifiedAccount {
            url: url.into(),
            token: Secret::new(token),
            session: session(user, "sam@acme.example"),
        }
    }

    /// The two files the build before accounts keeps for one session.
    fn legacy(paths: &Paths, store: &SessionStore, url: &str, token: &str) {
        crate::shared::write_private(&paths.connection(), format!(r#"{{"url":"{url}"}}"#).as_bytes()).unwrap();
        store
            .write(
                keychain::LEGACY,
                &SavedSession { url: url.into(), user_id: None, token: Secret::new(token) }.encode(),
            )
            .unwrap();
    }

    fn item(store: &SessionStore, id: &AccountId) -> Option<SavedSession> {
        SavedSession::decode(&store.read(&keychain::account_item(id)).unwrap()?)
    }

    fn token_of(registry: &Registry, id: &AccountId) -> Option<String> {
        registry.tokens.get(id).map(|token| token.expose().to_owned())
    }

    const URL: &str = "https://notes.acme.example";

    #[test]
    fn w1_a_fresh_install_writes_no_registry_and_the_welcome_starts() {
        let root = root("w1");
        let paths = Paths::at(&root);
        let registry = Registry::load(paths.clone(), SessionStore::memory()).ok().unwrap();
        assert!(registry.accounts().is_empty());
        assert!(!registry.has_saved_session());
        assert!(!paths.registry().exists(), "no registry is written for zero accounts");
        let existing = crate::onboarding::existing_in(&root, registry.has_saved_session());
        assert_eq!(
            crate::onboarding::first_run(crate::onboarding::Marker::Missing, existing),
            Some(crate::onboarding::Flow::START)
        );
        // An account that leaves again leaves no registry behind either.
        let store = SessionStore::memory();
        let mut registry = Registry::load(paths.clone(), store).ok().unwrap();
        let accepted = registry.accept(verified(URL, USER, TOKEN), false, &|_| None).unwrap();
        assert!(paths.registry().exists());
        registry.sign_out(&accepted.id, None, |_| {}).unwrap();
        assert!(!paths.registry().exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a1_migration_offline_then_online_keeps_the_legacy_session_in_step() {
        let root = root("a1");
        let paths = Paths::at(&root);
        let store = SessionStore::memory();
        legacy(&paths, &store, URL, TOKEN);
        // Offline: a provisional entry, the legacy token, no user, no
        // directory, and the registry written.
        let mut registry = Registry::load(paths.clone(), store.clone()).ok().unwrap();
        assert_eq!(registry.accounts().len(), 1);
        let pending = registry.accounts()[0].clone();
        assert_eq!((pending.slot, pending.url.as_str(), pending.user_id.as_ref()), (1, URL, None));
        assert_eq!(pending.state, AccountState::Checking);
        assert_eq!(token_of(&registry, &pending.id).as_deref(), Some(TOKEN));
        assert!(registry.directory(&pending.id).is_none());
        let file: serde_json::Value = serde_json::from_slice(&std::fs::read(paths.registry()).unwrap()).unwrap();
        assert_eq!(file["accounts"][0]["userId"], serde_json::Value::Null);
        assert_eq!(file["nextSlot"], 2);
        // Online: the first /session fills the user. The id is today's
        // directory name; the new item is written and read back.
        let id = registry.verified(&pending.id, session(USER, "sam@acme.example"), vec![]).unwrap();
        assert_eq!(id, AccountId::of(URL, USER));
        assert_eq!(
            paths.account(&id),
            crate::model::account_cache_path(&root, &format!("{URL}|{USER}")).parent().unwrap()
        );
        let saved = item(&store, &id).unwrap();
        assert_eq!((saved.url.as_str(), saved.user_id.as_deref(), saved.token.expose()), (URL, Some(USER), TOKEN));
        assert!(paths.account(&id).join("account.json").exists());
        let file: serde_json::Value = serde_json::from_slice(&std::fs::read(paths.registry()).unwrap()).unwrap();
        assert_eq!(file["accounts"][0]["userId"], USER);
        assert_eq!(file["default"], id.as_str());
        // Milestone 1: the legacy item and connection.json stay, in step.
        assert_eq!(migration::legacy_session(&store).unwrap().token.expose(), TOKEN);
        assert_eq!(migration::connection_address(&paths).as_deref(), Some(URL));
        // A token replacement updates both.
        registry.accept(verified(URL, USER, TOKEN_2), false, &|_| None).unwrap();
        assert_eq!(item(&store, &id).unwrap().token.expose(), TOKEN_2);
        assert_eq!(migration::legacy_session(&store).unwrap().token.expose(), TOKEN_2);
        // The next launch reads the registry, not the migration.
        let again = Registry::load(paths.clone(), store.clone()).ok().unwrap();
        assert_eq!(again.accounts().len(), 1);
        assert_eq!(again.accounts()[0].id, id);
        assert_eq!(token_of(&again, &id).as_deref(), Some(TOKEN_2));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a2_a_crash_after_any_migration_step_ends_in_the_same_state_with_a_token() {
        // Each case is the disk as a crash would leave it; the next launch
        // must end where an uninterrupted migration does.
        let finish = |paths: &Paths, store: &SessionStore| {
            let mut registry = Registry::load(paths.clone(), store.clone()).ok().unwrap();
            assert_eq!(registry.accounts().len(), 1, "one account, never two");
            let account = registry.accounts()[0].clone();
            assert_ne!(account.state, AccountState::NeedsSignIn, "a token is never absent");
            assert!(token_of(&registry, &account.id).is_some());
            let id = registry.verified(&account.id, session(USER, "sam@acme.example"), vec![]).unwrap();
            assert_eq!(id, AccountId::of(URL, USER));
            assert_eq!(item(store, &id).unwrap().token.expose(), TOKEN);
            assert_eq!(migration::legacy_session(store).unwrap().token.expose(), TOKEN);
            let file: serde_json::Value = serde_json::from_slice(&std::fs::read(paths.registry()).unwrap()).unwrap();
            assert_eq!((file["accounts"].as_array().unwrap().len(), &file["accounts"][0]["userId"]), (1, &serde_json::json!(USER)));
        };
        for step in 0..4 {
            let root = root("a2");
            let paths = Paths::at(&root);
            let store = SessionStore::memory();
            legacy(&paths, &store, URL, TOKEN);
            let mut registry = Registry::load(paths.clone(), store.clone()).ok().unwrap();
            let pending = registry.accounts()[0].id.clone();
            let id = AccountId::of(URL, USER);
            match step {
                // After step 1 only: the provisional registry.
                0 => {}
                // The new item written, the registry still provisional.
                1 => store
                    .write(
                        &keychain::account_item(&id),
                        &SavedSession { url: URL.into(), user_id: Some(USER.into()), token: Secret::new(TOKEN) }.encode(),
                    )
                    .unwrap(),
                // Everything but account.json.
                2 => {
                    registry.verified(&pending, session(USER, "sam@acme.example"), vec![]).unwrap();
                    std::fs::remove_file(paths.account(&id).join("account.json")).unwrap();
                }
                // Done, and run again.
                _ => {
                    registry.verified(&pending, session(USER, "sam@acme.example"), vec![]).unwrap();
                }
            }
            drop(registry);
            finish(&paths, &store);
            let _ = std::fs::remove_dir_all(root);
        }
    }

    #[test]
    fn a3_an_item_whose_address_or_user_differs_from_the_registry_is_never_used() {
        for (url, user) in [("https://elsewhere.example", USER), (URL, OTHER)] {
            let root = root("a3");
            let paths = Paths::at(&root);
            let store = SessionStore::memory();
            let mut registry = Registry::load(paths.clone(), store.clone()).ok().unwrap();
            let id = registry.accept(verified(URL, USER, TOKEN), false, &|_| None).unwrap().id;
            drop(registry);
            store
                .write(
                    &keychain::account_item(&id),
                    &SavedSession { url: url.into(), user_id: Some(user.into()), token: Secret::new(TOKEN_2) }.encode(),
                )
                .unwrap();
            let registry = Registry::load(paths.clone(), store.clone()).ok().unwrap();
            let account = registry.get(&id).unwrap();
            assert_eq!(account.state, AccountState::NeedsSignIn);
            // Nothing can be sent: there is no client.
            assert!(registry.client(&id).is_err());
            assert!(registry.verification(&id).is_none());
            let _ = std::fs::remove_dir_all(root);
        }
        // The legacy item for another address does not become the
        // migrated account's token either.
        let root = root("a3-legacy");
        let paths = Paths::at(&root);
        let store = SessionStore::memory();
        legacy(&paths, &store, "https://elsewhere.example", TOKEN);
        crate::shared::write_private(&paths.connection(), format!(r#"{{"url":"{URL}"}}"#).as_bytes()).unwrap();
        let registry = Registry::load(paths.clone(), store).ok().unwrap();
        assert!(registry.accounts().is_empty(), "nothing is migrated from a mismatched pair");
        assert!(!paths.registry().exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a4_an_unreadable_registry_is_kept_aside_and_rebuilt_never_migrated_again() {
        let root = root("a4");
        let paths = Paths::at(&root);
        let store = SessionStore::memory();
        let mut registry = Registry::load(paths.clone(), store.clone()).ok().unwrap();
        let id = registry.accept(verified(URL, USER, TOKEN), false, &|_| None).unwrap().id;
        drop(registry);
        // A legacy pair for another server, which a migration would take.
        legacy(&paths, &store, "https://legacy.example", TOKEN_2);
        // A directory that describes itself but has no Keychain item.
        let orphan = AccountId::of(URL, OTHER);
        AccountFile { id: orphan.as_str().into(), url: URL.into(), user_id: OTHER.into(), email: String::new(), label: String::new() }
            .write(&paths.account(&orphan))
            .unwrap();
        std::fs::write(paths.registry(), b"{ not the registry").unwrap();
        let rebuilt = match Registry::load(paths.clone(), store.clone()) {
            Err(RegistryError::Unreadable { rebuilt, aside }) => {
                let aside = aside.expect("copied aside");
                assert_eq!(std::fs::read(&aside).unwrap(), b"{ not the registry");
                assert!(aside.file_name().unwrap().to_string_lossy().starts_with("accounts.unreadable-"));
                rebuilt
            }
            Ok(_) => panic!("an unreadable registry is an error, not empty"),
        };
        let ids: Vec<_> = rebuilt.accounts().iter().map(|account| account.id.clone()).collect();
        assert_eq!(ids, [id.clone()], "only the account whose item exists; no legacy migration");
        assert_eq!(token_of(&rebuilt, &id).as_deref(), Some(TOKEN));
        // The rebuilt registry is what is on disk now.
        let again = Registry::load(paths.clone(), store).ok().unwrap();
        assert_eq!(again.accounts().len(), 1);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a5_a_stray_legacy_session_for_a_known_account_replaces_its_token() {
        let root = root("a5");
        let paths = Paths::at(&root);
        let store = SessionStore::memory();
        let mut registry = Registry::load(paths.clone(), store.clone()).ok().unwrap();
        let id = registry.accept(verified(URL, USER, TOKEN), false, &|_| None).unwrap().id;
        drop(registry);
        // An old build, run by hand, signed in again: a new legacy token.
        legacy(&paths, &store, URL, TOKEN_2);
        let mut registry = Registry::load(paths.clone(), store.clone()).ok().unwrap();
        let (url, token) = registry.take_stray().expect("verified once");
        assert_eq!((url.as_str(), token.expose()), (URL, TOKEN_2));
        let accepted = registry.settle_stray(&url, token, Some(session(USER, "sam@acme.example"))).unwrap();
        assert_eq!(accepted.id, id);
        assert_eq!(accepted.outcome, Outcome::TokenReplaced);
        assert_eq!(item(&store, &id).unwrap().token.expose(), TOKEN_2);
        // The old session is ended on its server.
        assert_eq!(accepted.ended.len(), 1);
        assert_eq!(registry.accounts().len(), 1);
        assert_eq!(migration::legacy_session(&store).unwrap().token.expose(), TOKEN_2);
        // Settled: it is not looked at again.
        assert!(Registry::load(paths.clone(), store).ok().unwrap().take_stray().is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn o2_a_planted_legacy_session_for_an_unknown_account_is_deleted_and_adds_nothing() {
        let root = root("o2");
        let paths = Paths::at(&root);
        let store = SessionStore::memory();
        let mut registry = Registry::load(paths.clone(), store.clone()).ok().unwrap();
        let id = registry.accept(verified(URL, USER, TOKEN), false, &|_| None).unwrap().id;
        drop(registry);
        let planted = "planted-by-another-process-000000000000";
        for (url, session) in [
            ("https://attacker.example", Some(session(OTHER, "mallory@attacker.example"))),
            (URL, Some(session(OTHER, "mallory@acme.example"))),
            (URL, None),
        ] {
            legacy(&paths, &store, url, planted);
            let mut registry = Registry::load(paths.clone(), store.clone()).ok().unwrap();
            let (stray_url, token) = registry.take_stray().unwrap();
            assert!(registry.settle_stray(&stray_url, token, session).is_none());
            assert_eq!(registry.accounts().len(), 1, "it never adds an account");
            assert_eq!(item(&store, &id).unwrap().token.expose(), TOKEN);
            // Gone; the legacy item is back in step with the real account.
            let legacy_now = migration::legacy_session(&store).unwrap();
            assert_ne!(legacy_now.token.expose(), planted);
            assert_eq!(legacy_now.token.expose(), TOKEN);
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn i1_signing_in_again_with_another_case_keeps_the_stored_address_and_directory() {
        let root = root("i1");
        let paths = Paths::at(&root);
        let store = SessionStore::memory();
        let mut registry = Registry::load(paths.clone(), store.clone()).ok().unwrap();
        let first = registry.accept(verified(URL, USER, TOKEN), false, &|_| None).unwrap();
        let again = registry
            .accept(verified("https://NOTES.Acme.example/", &USER.to_uppercase(), TOKEN_2), false, &|_| None)
            .unwrap();
        assert_eq!(again.id, first.id);
        assert_eq!(again.outcome, Outcome::TokenReplaced);
        assert_eq!(registry.accounts().len(), 1);
        assert_eq!(registry.accounts()[0].url, URL, "the stored address is kept");
        let saved = item(&store, &first.id).unwrap();
        assert_eq!((saved.url.as_str(), saved.token.expose()), (URL, TOKEN_2));
        assert_eq!(again.ended.len(), 1, "the old session is signed out on its server");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn i3_a_directory_made_under_the_address_as_typed_before_is_found_again() {
        let typed = "https://Notes.Acme.example";
        // As the build before: the address typed, in connection.json, and
        // a cache under it with unsent audio, but no account.json.
        let root = root("i3");
        let paths = Paths::at(&root);
        let old = AccountId::of(typed, USER);
        std::fs::create_dir_all(paths.recordings(&old).join("note-3")).unwrap();
        crate::shared::write_private(&paths.connection(), format!(r#"{{"url":"{typed}"}}"#).as_bytes()).unwrap();
        let mut registry = Registry::load(paths.clone(), SessionStore::memory()).ok().unwrap();
        // Signed in as the lower-case address (the session had ended).
        let accepted = registry.accept(verified(URL, USER, TOKEN), false, &|_| None).unwrap();
        assert_eq!(accepted.id, old, "the earlier cache is this account's directory");
        assert_eq!(registry.accounts()[0].url, typed);
        // The same, typed that way this time.
        let root2 = self::root("i3-typed");
        let paths2 = Paths::at(&root2);
        std::fs::create_dir_all(paths2.account(&old)).unwrap();
        let mut registry = Registry::load(paths2.clone(), SessionStore::memory()).ok().unwrap();
        assert_eq!(registry.accept(verified(typed, USER, TOKEN), false, &|_| None).unwrap().id, old);
        // With no earlier cache, a new address is stored normalised.
        let root3 = self::root("i3-new");
        let mut registry = Registry::load(Paths::at(&root3), SessionStore::memory()).ok().unwrap();
        let fresh = registry.accept(verified(typed, USER, TOKEN), false, &|_| None).unwrap();
        assert_eq!(registry.get(&fresh.id).unwrap().url, URL);
        for root in [root, root2, root3] {
            let _ = std::fs::remove_dir_all(root);
        }
    }

    #[test]
    fn i4_a_migrated_address_with_a_non_ascii_host_gets_a_client_and_matches_no_link() {
        let url = "https://nötes.acme.example";
        let root = root("i4");
        let paths = Paths::at(&root);
        let store = SessionStore::memory();
        legacy(&paths, &store, url, TOKEN);
        let mut registry = Registry::load(paths.clone(), store).ok().unwrap();
        let pending = registry.accounts()[0].clone();
        assert_eq!(pending.url, url, "kept byte for byte");
        assert!(pending.origin.is_none());
        let id = registry.verified(&pending.id, session(USER, "sam@acme.example"), vec![]).unwrap();
        assert!(registry.client(&id).is_ok(), "it still gets a client");
        for link in ["https://xn--ntes-moa.acme.example", "https://notes.acme.example"] {
            let origin = crate::link::ServerOrigin::parse(link).unwrap();
            assert!(registry.on(&origin).is_empty(), "{link}");
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn i6_signing_in_again_as_someone_else_is_refused_and_can_be_added_instead() {
        let root = root("i6");
        let paths = Paths::at(&root);
        let store = SessionStore::memory();
        let mut registry = Registry::load(paths.clone(), store.clone()).ok().unwrap();
        let id = registry.accept(verified(URL, USER, TOKEN), false, &|_| None).unwrap().id;
        registry.session_ended(&id);
        let mut ops = verified(URL, OTHER, TOKEN_2);
        ops.session = session(OTHER, "ops@acme.example");
        let refused = registry.sign_in_again(&id, ops.clone()).err().unwrap();
        assert_eq!(
            refused.message(),
            "You signed in as ops@acme.example; this account is sam@acme.example."
        );
        // Nothing changed.
        assert_eq!(registry.accounts().len(), 1);
        assert_eq!(registry.get(&id).unwrap().state, AccountState::NeedsSignIn);
        assert_eq!(item(&store, &id).unwrap().token.expose(), TOKEN);
        // "Add ops@acme.example as another account".
        let added = registry.accept(ops, true, &|_| None).unwrap();
        assert_ne!(added.id, id);
        assert_eq!(registry.accounts().len(), 2);
        assert_eq!(registry.get(&added.id).unwrap().slot, 2);
        assert_eq!(registry.default().unwrap().id, id, "the first added stays the default");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn i7_a_replaced_token_cancels_requests_on_their_way_and_ends_the_old_session() {
        use crate::accounts::client::testing::{Fake, json};
        let server = Fake::start(|request| match request.path.as_str() {
            "/api/auth/sign-out" => json(200, "{}"),
            _ => json(200, r#"{"ok":true}"#),
        });
        let root = root("i7");
        let mut registry = Registry::load(Paths::at(&root), SessionStore::memory()).ok().unwrap();
        let id = registry.accept(verified(&server.url, USER, TOKEN), false, &|_| None).unwrap().id;
        let before = registry.client(&id).unwrap();
        let replaced = registry.accept(verified(&server.url, USER, TOKEN_2), false, &|_| None).unwrap();
        // An upload made with the old token stops; it is queued again when
        // the account is Ready (`Cereal::interrupted`).
        assert!(before.stale());
        assert_eq!(
            before.send::<serde_json::Value>("GET", "/api/v1/notes", None, None),
            Err(crate::accounts::client::RequestError::Cancelled)
        );
        // A new client uses the new session.
        let after = registry.client(&id).unwrap();
        let _: serde_json::Value = after.request("GET", "/api/v1/notes", None).unwrap();
        // The old session is ended on its own server.
        for ended in replaced.ended {
            ended.client.sign_out_on_server();
        }
        let seen = server.requests();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0].bearer().as_deref(), Some(TOKEN_2));
        assert_eq!((seen[1].method.as_str(), seen[1].path.as_str()), ("POST", "/api/auth/sign-out"));
        assert_eq!(seen[1].bearer().as_deref(), Some(TOKEN));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn s4_signing_out_keeps_the_directory_and_signing_in_again_finds_it() {
        let root = root("s4");
        let paths = Paths::at(&root);
        let store = SessionStore::memory();
        let mut registry = Registry::load(paths.clone(), store.clone()).ok().unwrap();
        let id = registry.accept(verified(URL, USER, TOKEN), false, &|_| None).unwrap().id;
        // Something on this Mac for the account.
        let directory = paths.account(&id);
        std::fs::create_dir_all(paths.recordings(&id).join("note-1")).unwrap();
        std::fs::write(paths.workspace(&id), br#"{"notes":[{"id":1,"title":"Kept","favorite":true,"folder_id":4}],"folders":[{"id":4,"name":"Clients"}]}"#).unwrap();
        std::fs::write(paths.chats(&id), br#"{"version":1,"sessions":[],"recipes":[]}"#).unwrap();
        let mut cleaned = None;
        let departed = registry
            .sign_out(&id, None, |account| cleaned = Some(account.epoch()))
            .unwrap()
            .unwrap();
        // The epoch had moved before the window let go of the account.
        assert_eq!(cleaned, Some(2));
        assert!(departed.client.is_some(), "the session is ended on its server");
        assert!(registry.accounts().is_empty());
        assert!(store.read(&keychain::account_item(&id)).unwrap().is_none());
        assert!(migration::legacy_session(&store).is_none());
        assert!(!paths.registry().exists());
        assert!(!paths.connection().exists());
        // Everything on this Mac stays.
        assert!(paths.recordings(&id).join("note-1").is_dir());
        assert!(paths.workspace(&id).exists() && paths.chats(&id).exists());
        assert!(directory.join("account.json").exists());
        // Signing in again as the same user: the same directory.
        let back = registry.accept(verified(URL, USER, TOKEN_2), false, &|_| None).unwrap();
        assert_eq!(back.id, id);
        assert_ne!(registry.get(&id).unwrap().slot, 1, "a slot is never reused");
        let mut workspace = crate::model::Workspace::for_accounts(&paths);
        let account = registry.get(&id).unwrap().clone();
        workspace.load_account(account.slot, &id, &paths).unwrap();
        let note = workspace.notes_of(account.slot).next().unwrap();
        assert_eq!((note.title.as_str(), note.favorite), ("Kept", true));
        assert_eq!(
            workspace.folders.iter().find(|folder| Some(folder.id) == note.folder_id).map(|f| f.name.as_str()),
            Some("Clients")
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn s6_erase_removes_the_directory_and_refuses_a_path_like_id() {
        let root = root("s6");
        let paths = Paths::at(&root);
        let store = SessionStore::memory();
        let mut registry = Registry::load(paths.clone(), store.clone()).ok().unwrap();
        let id = registry.accept(verified(URL, USER, TOKEN), false, &|_| None).unwrap().id;
        std::fs::create_dir_all(paths.recordings(&id).join("note-1")).unwrap();
        // A sibling that must survive.
        let other = paths.servers().join("not-an-account");
        std::fs::create_dir_all(&other).unwrap();
        registry.erase(&id, None, |_| {}).unwrap();
        assert!(!paths.account(&id).exists());
        assert!(other.is_dir());
        assert!(store.read(&keychain::account_item(&id)).unwrap().is_none());
        // Only an account directory, by its checked name, directly under
        // `servers`, and not through a link.
        for refused in [
            paths.servers().join(".."),
            paths.servers().join("not-an-account"),
            paths.data.clone(),
            root.join("elsewhere").join(id.as_str()),
        ] {
            assert!(!erase_directory(&refused), "{refused:?}");
        }
        assert!(AccountId::parse("../../etc").is_none());
        assert!(root.exists() && other.exists());
        let linked = paths.servers().join(AccountId::of(URL, OTHER).as_str());
        std::os::unix::fs::symlink(&other, &linked).unwrap();
        assert!(!erase_directory(&linked), "a symbolic link is not followed");
        assert!(other.exists());
        // An account's hidden calendars go with it; another's stay.
        let mut preferences = crate::desktop_preferences::MeetingPreferences {
            hidden_calendars: vec![format!("{}/work", id.as_str()), format!("{}/home", AccountId::of(URL, OTHER).as_str())],
            ..Default::default()
        };
        crate::desktop_preferences::forget_account(&mut preferences, &id);
        assert_eq!(preferences.hidden_calendars.len(), 1);
        assert!(!preferences.hidden_calendars[0].starts_with(id.as_str()));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn s8_an_account_whose_server_does_not_answer_shows_nothing_and_another_works() {
        // A port with nothing listening.
        let closed = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            format!("http://{}", listener.local_addr().unwrap())
        };
        assert!(matches!(
            crate::accounts::client::session_check(&closed, &Secret::new(TOKEN)),
            crate::accounts::client::SessionCheck::Unknown(_)
        ));
        let root = root("s8");
        let paths = Paths::at(&root);
        let store = SessionStore::memory();
        let mut registry = Registry::load(paths.clone(), store.clone()).ok().unwrap();
        let down = registry.accept(verified(&closed, USER, TOKEN), false, &|_| None).unwrap().id;
        let up = registry.accept(verified(URL, OTHER, TOKEN_2), true, &|_| None).unwrap().id;
        drop(registry);
        // The next launch: both Checking; one answers, one does not.
        let mut registry = Registry::load(paths.clone(), store).ok().unwrap();
        registry.set_state(&down, AccountState::Unreachable { message: "Could not reach this server".into() });
        registry.verified(&up, session(OTHER, "sam@acme.example"), vec![]).unwrap();
        assert!(!registry.get(&down).unwrap().shown(), "no notes for the account that did not answer");
        assert!(registry.get(&up).unwrap().shown());
        assert!(registry.client(&up).is_ok());
        let _ = std::fs::remove_dir_all(root);
    }
}
