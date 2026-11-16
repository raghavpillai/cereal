//! Folders of this Mac (`folders.json`). A folder can hold notes of any
//! account, so folders are not an account's: they have their own small id
//! space, and each account's folders are folded into them once ("seeding").
//! Local mode ("this Mac only") is not part of this: its folders stay in its
//! own `workspace.json`.
use crate::{
    accounts::{AccountId, Paths},
    model::{Folder, Settings},
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Default, Serialize, Deserialize)]
struct FoldersFile {
    version: u32,
    #[serde(rename = "nextId")]
    next_id: u64,
    folders: Vec<Folder>,
    /// Accounts whose folders have been folded in. Kept here, not in the
    /// account's own file: a key in `workspace.json` would be lost when an
    /// older build rewrites that file.
    #[serde(default)]
    seeded: Vec<String>,
}

/// The device-level files of server mode and what was last written of them.
#[derive(Debug, Clone)]
pub struct Device {
    paths: Paths,
    /// `None` until settings were saved or seeded from an account.
    pub settings: Option<Settings>,
    /// The folders as read at launch. In memory the workspace's own list is
    /// the one that changes.
    pub folders: Vec<Folder>,
    next_id: u64,
    seeded: Vec<String>,
    written_folders: Option<u64>,
    written_settings: Option<u64>,
}

fn same_name(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

impl Device {
    pub fn load(paths: &Paths) -> Self {
        let settings = crate::device_settings::load(paths);
        let read = std::fs::read(paths.folders());
        let file = match &read {
            Ok(bytes) => match serde_json::from_slice::<FoldersFile>(bytes) {
                Ok(file) if file.version == 1 => file,
                // Not a file this build can read: kept aside, never replaced
                // unseen.
                _ => {
                    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%S%3fZ");
                    let _ = crate::shared::write_private(
                        &paths.data.join(format!("folders.unreadable-{stamp}.json")),
                        bytes,
                    );
                    FoldersFile::default()
                }
            },
            Err(_) => FoldersFile::default(),
        };
        let highest = file.folders.iter().map(|folder| folder.id).max().unwrap_or(0);
        let mut device = Self {
            paths: paths.clone(),
            written_settings: settings
                .as_ref()
                .map(|settings| crate::store::fingerprint(&crate::device_settings::encode(settings))),
            settings,
            next_id: file.next_id.max(highest + 1).max(1),
            folders: file.folders,
            seeded: file.seeded,
            written_folders: None,
        };
        if read.is_ok() {
            device.written_folders = Some(crate::store::fingerprint(&device.encode(&device.folders)));
        }
        device
    }

    fn encode(&self, folders: &[Folder]) -> Vec<u8> {
        serde_json::to_vec_pretty(&FoldersFile {
            version: 1,
            next_id: self.next_id,
            folders: folders.to_vec(),
            seeded: self.seeded.clone(),
        })
        .unwrap_or_default()
    }

    fn write_folders(&mut self, folders: &[Folder]) -> Result<(), String> {
        let bytes = self.encode(folders);
        let print = crate::store::fingerprint(&bytes);
        if self.written_folders == Some(print) {
            return Ok(());
        }
        crate::shared::write_private(&self.paths.folders(), &bytes)?;
        self.written_folders = Some(print);
        Ok(())
    }

    /// Fold one account's folders into the device's (`folders`, the
    /// workspace's list), and return the map from the ids in the account's
    /// file to device folder ids. A folder id that maps to nothing names no
    /// folder any more: its note is shown in no folder.
    ///
    /// The first account ever seeded keeps its ids. For later ones, a folder
    /// whose trimmed, case-insensitive name equals a device folder's maps to
    /// it; otherwise it gets a new id. The new folders and the id mark are
    /// written **before** the caller rewrites the account's notes, and the
    /// account is recorded as seeded only after ([`Device::commit`]), so a
    /// crash at any point repeats this and finds the same folders by id or
    /// by name: nothing is duplicated and no membership is lost.
    pub fn seed(
        &mut self,
        account: &AccountId,
        theirs: &[Folder],
        folders: &mut Vec<Folder>,
    ) -> HashMap<u64, u64> {
        let seeded = !self.needs_commit(account);
        let first = self.seeded.is_empty() && !seeded;
        let mut map = HashMap::new();
        let mut added = false;
        for folder in theirs {
            let target = match folders.iter().find(|device| device.id == folder.id) {
                // The same folder: the first account keeps its ids, a
                // seeded account's file holds device ids, and a repeat
                // after a crash finds what it put there.
                Some(device) if same_name(&device.name, &folder.name) => device.id,
                None if first => {
                    folders.push(folder.clone());
                    self.next_id = self.next_id.max(folder.id + 1);
                    added = true;
                    folder.id
                }
                _ => match folders.iter().find(|device| same_name(&device.name, &folder.name)) {
                    Some(device) => device.id,
                    // A folder an older build made in this account's file
                    // after it was seeded, or a new one: a new device folder.
                    None => {
                        let id = self.next_id;
                        self.next_id += 1;
                        folders.push(Folder {
                            id,
                            name: folder.name.clone(),
                        });
                        added = true;
                        id
                    }
                },
            };
            map.insert(folder.id, target);
        }
        if seeded {
            // Its notes already hold device ids.
            for folder in folders.iter() {
                map.entry(folder.id).or_insert(folder.id);
            }
        }
        if added || !seeded {
            // Step 3: the folders and the mark, without the account in
            // `seeded`. (For an account already seeded, its new folders.)
            let _ = self.write_folders(folders);
        }
        map
    }

    /// The account's file does not yet count as holding device folder ids.
    pub fn needs_commit(&self, account: &AccountId) -> bool {
        !self.seeded.iter().any(|id| id == account.as_str())
    }

    /// Step 5: the account's file holds device ids; say so durably.
    pub fn commit(&mut self, account: &AccountId, folders: &[Folder]) {
        if self.needs_commit(account) {
            self.seeded.push(account.as_str().to_owned());
            let _ = self.write_folders(folders);
        }
    }

    /// "New folder": a folder of this Mac.
    pub fn add(&mut self, folders: &mut Vec<Folder>, name: &str) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let name = name.trim();
        folders.push(Folder {
            id,
            name: if name.is_empty() {
                "Untitled folder".into()
            } else {
                name.into()
            },
        });
        id
    }

    /// The device files that differ from what was last written, to be
    /// written with the workspace: `(path, bytes)`.
    pub fn pending(&mut self, folders: &[Folder], settings: &Settings) -> Vec<(std::path::PathBuf, Vec<u8>)> {
        let mut jobs = Vec::new();
        let bytes = self.encode(folders);
        let print = crate::store::fingerprint(&bytes);
        // No folders and no file yet: nothing worth a file.
        let worth = !folders.is_empty() || !self.seeded.is_empty() || self.written_folders.is_some();
        if worth && self.written_folders != Some(print) {
            self.written_folders = Some(print);
            jobs.push((self.paths.folders(), bytes));
        }
        // Settings exist as a device file once something seeded or set them.
        if self.settings.is_some() {
            let bytes = crate::device_settings::encode(settings);
            let print = crate::store::fingerprint(&bytes);
            if self.written_settings != Some(print) {
                self.written_settings = Some(print);
                self.settings = Some(settings.clone());
                jobs.push((self.paths.settings(), bytes));
            }
        }
        jobs
    }

    /// A setting was changed by the person: from now on settings are this
    /// Mac's, whether or not an account ever seeded them.
    pub fn settings_touched(&mut self, settings: &Settings) {
        if self.settings.is_none() {
            self.settings = Some(settings.clone());
        }
    }

    /// A write of a device file failed: try again with the next save.
    pub fn forget(&mut self, path: &std::path::Path) {
        if path == self.paths.folders() {
            self.written_folders = None;
        } else if path == self.paths.settings() {
            self.written_settings = None;
        }
    }
}
