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
            show_upcoming: false,
            ..Settings::default()
        };
        crate::shared::write_private(&paths.settings(), &encode(&settings)).unwrap();
        let loaded = load(&paths).unwrap();
        assert!(loaded.dark_mode && !loaded.show_upcoming && loaded.auto_enhance);
        let _ = std::fs::remove_dir_all(root);
    }
}
