//! Settings of this Mac (`settings.json`), beside the accounts' directories.
//! They used to live in the one account's `workspace.json`; that file keeps
//! receiving a copy, so the build before still reads them.
use crate::{accounts::Paths, model::Settings};

/// The saved settings, or `None` when none were saved yet: they are then
/// seeded from the first account's workspace.
pub fn load(paths: &Paths) -> Option<Settings> {
    let bytes = std::fs::read(paths.settings()).ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub fn encode(settings: &Settings) -> Vec<u8> {
    serde_json::to_vec_pretty(settings).unwrap_or_default()
}

/// The device-level settings file of server mode and what was last written
/// of it.
#[derive(Debug, Clone)]
pub struct Device {
    paths: Paths,
    /// `None` until settings were saved or seeded from an account.
    pub settings: Option<Settings>,
    written: Option<u64>,
}

impl Device {
    pub fn load(paths: &Paths) -> Self {
        let settings = load(paths);
        Self {
            paths: paths.clone(),
            written: settings
                .as_ref()
                .map(|settings| crate::store::fingerprint(&encode(settings))),
            settings,
        }
    }

    /// The settings file, when it differs from what was last written, to be
    /// written with the workspace: `(path, bytes)`.
    pub fn pending(&mut self, settings: &Settings) -> Vec<(std::path::PathBuf, Vec<u8>)> {
        let mut jobs = Vec::new();
        // Settings exist as a device file once something seeded or set them.
        if self.settings.is_some() {
            let bytes = encode(settings);
            let print = crate::store::fingerprint(&bytes);
            if self.written != Some(print) {
                self.written = Some(print);
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

    /// A write of the settings file failed: try again with the next save.
    pub fn forget(&mut self, path: &std::path::Path) {
        if path == self.paths.settings() {
            self.written = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_settings_round_trip_and_are_absent_until_saved() {
        let root = std::env::temp_dir().join(format!("cereal-settings-{}", uuid::Uuid::new_v4()));
        let paths = Paths::at(&root);
        assert!(load(&paths).is_none());
        let settings = Settings {
            dark_mode: true,
            auto_enhance: false,
            ..Settings::default()
        };
        crate::shared::write_private(&paths.settings(), &encode(&settings)).unwrap();
        let loaded = load(&paths).unwrap();
        assert!(loaded.dark_mode && !loaded.auto_enhance);
        let _ = std::fs::remove_dir_all(root);
    }
}
