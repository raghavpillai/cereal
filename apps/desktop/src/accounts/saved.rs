//! What is on this Mac of accounts that are not signed in: the directories
//! under `servers/` that no account in the registry owns. Those that
//! describe themselves (`account.json`) are "Signed out, with data on this
//! Mac"; those from before accounts, which do not, are "Older saved data".
//! Everything here is read from plain files, so it is shown, never trusted.
use super::{AccountId, Paths, host_of, migration::AccountFile};
use std::path::{Path, PathBuf};

/// One account directory that no signed-in account owns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Saved {
    pub id: AccountId,
    pub directory: PathBuf,
    /// From `account.json`; `None` for a directory from before accounts.
    pub described: Option<AccountFile>,
    pub bytes: u64,
    /// The newest change to anything in it, seconds since 1970.
    pub changed: Option<u64>,
    pub counts: Counts,
}

impl Saved {
    /// The host its `account.json` names, in full. Shown, not trusted.
    pub fn host(&self) -> Option<String> {
        self.described.as_ref().map(|file| host_of(&file.url))
    }
}

/// What a sign-out leaves and an erase removes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub notes: usize,
    /// Device folders that hold at least one of these notes.
    pub folders: usize,
    pub favorites: usize,
    pub chats: usize,
    pub recordings: usize,
    /// Recordings that exist nowhere else: never confirmed by the server.
    pub never_uploaded: usize,
}

impl Counts {
    /// Count what one account's notes hold. `chats` is counted by the
    /// caller, which knows where conversations are kept.
    pub fn of_notes<'a>(notes: impl Iterator<Item = &'a crate::model::Note>, chats: usize) -> Self {
        let mut counts = Self {
            chats,
            ..Self::default()
        };
        let mut folders = std::collections::HashSet::new();
        for note in notes {
            counts.notes += 1;
            counts.favorites += usize::from(note.favorite);
            folders.extend(note.folder_id);
            counts.recordings += note.audio_captures.len();
            if note.server_id.is_none() || note.unsent_capture {
                counts.never_uploaded += note.audio_captures.len();
            }
        }
        counts.folders = folders.len();
        counts
    }

    /// The same, read from a directory whose account is not signed in.
    pub fn of_directory(directory: &Path) -> Self {
        #[derive(serde::Deserialize, Default)]
        #[serde(default)]
        struct File {
            notes: Vec<crate::model::Note>,
        }
        #[derive(serde::Deserialize, Default)]
        #[serde(default)]
        struct Chats {
            sessions: Vec<serde_json::Value>,
        }
        let read = |name: &str| {
            let path = directory.join(name);
            // A cache this large is counted by its size alone.
            (std::fs::metadata(&path).ok()?.len() <= 256 * 1024 * 1024)
                .then(|| std::fs::read(path).ok())
                .flatten()
        };
        let notes = read("workspace.json")
            .and_then(|bytes| serde_json::from_slice::<File>(&bytes).ok())
            .unwrap_or_default()
            .notes;
        let chats = read("chats.json")
            .and_then(|bytes| serde_json::from_slice::<Chats>(&bytes).ok())
            .map_or(0, |chats| chats.sessions.len());
        Self::of_notes(notes.iter(), chats)
    }

    /// "214 notes and 41 recordings": what stays after a sign-out.
    pub fn stays(&self) -> String {
        format!(
            "{} and {}",
            plural(self.notes, "note"),
            plural(self.recordings, "recording")
        )
    }

    /// "214 notes saved here · 3 folders use them · 12 favourites · 9 chats
    /// · 41 recordings, 3 of which were never uploaded".
    pub fn goes(&self) -> String {
        let mut text = format!(
            "{} saved here · {} · {} · {} · {}",
            plural(self.notes, "note"),
            if self.folders == 1 {
                "1 folder uses them".to_owned()
            } else {
                format!("{} folders use them", self.folders)
            },
            plural(self.favorites, "favourite"),
            plural(self.chats, "chat"),
            plural(self.recordings, "recording"),
        );
        if self.never_uploaded > 0 {
            text.push_str(&format!(
                ", {} of which {} never uploaded",
                self.never_uploaded,
                if self.never_uploaded == 1 { "was" } else { "were" }
            ));
        }
        text
    }

    /// The label of the button that erases.
    pub fn erase_label(&self) -> String {
        match self.never_uploaded {
            0 => "Erase".into(),
            1 => "Erase, including 1 recording that exists nowhere else".into(),
            n => format!("Erase, including {n} recordings that exist nowhere else"),
        }
    }
}

pub fn plural(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

/// "1.2 GB", "340 MB", "12 KB".
pub fn size_label(bytes: u64) -> String {
    const KB: f64 = 1000.;
    let bytes = bytes as f64;
    if bytes >= KB * KB * KB {
        format!("{:.1} GB", bytes / (KB * KB * KB))
    } else if bytes >= KB * KB {
        format!("{:.0} MB", bytes / (KB * KB))
    } else {
        format!("{:.0} KB", (bytes / KB).max(1.))
    }
}

/// Bytes in a directory and the newest change to anything in it. Symbolic
/// links are neither followed nor counted.
fn measure(directory: &Path) -> (u64, Option<u64>) {
    let (mut bytes, mut changed) = (0, None);
    let mut pending = vec![directory.to_path_buf()];
    while let Some(next) = pending.pop() {
        for entry in std::fs::read_dir(&next).into_iter().flatten().flatten() {
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                pending.push(entry.path());
                continue;
            }
            bytes += metadata.len();
            let modified = metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|since| since.as_secs());
            changed = changed.max(modified);
        }
    }
    (bytes, changed)
}

/// What Settings lists under the accounts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Listing {
    /// "Signed out, with data on this Mac": host, email, [Sign in] [Erase…].
    pub signed_out: Vec<Saved>,
    /// "Older saved data": directories from before accounts, with their
    /// size and last-changed date and [Erase…] only.
    pub older: Vec<Saved>,
}

/// Every account directory that none of `live` owns.
pub fn listing(paths: &Paths, live: &[AccountId]) -> Listing {
    let mut listing = Listing::default();
    for (id, directory) in super::migration::account_directories(paths) {
        if live.contains(&id) {
            continue;
        }
        let (bytes, changed) = measure(&directory);
        let saved = Saved {
            described: AccountFile::read(&directory),
            counts: Counts::of_directory(&directory),
            id,
            directory,
            bytes,
            changed,
        };
        if saved.described.is_some() {
            listing.signed_out.push(saved);
        } else if bytes > 0 {
            listing.older.push(saved);
        }
    }
    listing
}

/// Erase a directory that no signed-in account owns. Refused for one that
/// an account in the registry owns (that is "Remove account and erase").
pub fn erase(paths: &Paths, id: &AccountId, live: &[AccountId]) -> bool {
    !live.contains(id) && super::registry::erase_directory(&paths.account(id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_read_as_sentences_and_name_what_exists_nowhere_else() {
        let counts = Counts {
            notes: 214,
            folders: 3,
            favorites: 12,
            chats: 9,
            recordings: 41,
            never_uploaded: 3,
        };
        assert_eq!(counts.stays(), "214 notes and 41 recordings");
        assert_eq!(
            counts.goes(),
            "214 notes saved here · 3 folders use them · 12 favourites · 9 chats · 41 recordings, 3 of which were never uploaded"
        );
        assert_eq!(
            counts.erase_label(),
            "Erase, including 3 recordings that exist nowhere else"
        );
        let one = Counts {
            notes: 1,
            folders: 1,
            favorites: 1,
            chats: 1,
            recordings: 1,
            never_uploaded: 1,
        };
        assert_eq!(one.stays(), "1 note and 1 recording");
        assert_eq!(
            one.goes(),
            "1 note saved here · 1 folder uses them · 1 favourite · 1 chat · 1 recording, 1 of which was never uploaded"
        );
        assert_eq!(one.erase_label(), "Erase, including 1 recording that exists nowhere else");
        assert_eq!(Counts::default().erase_label(), "Erase");
        assert_eq!(size_label(1_250_000_000), "1.2 GB");
        assert_eq!(size_label(340_000_000), "340 MB");
        assert_eq!(size_label(12_000), "12 KB");
        assert_eq!(size_label(10), "1 KB");
    }
}
