//! Device-only appearance preferences, separate from account/private-note caches.
//! Icon artwork is original Cereal artwork; no installed app assets are read.
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

pub const SOUND_SETTINGS_URL: &str = "x-apple.systempreferences:com.apple.Sound-Settings.extension";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppIcon {
    /// The default icon. It was called Lime while the brand was green, and
    /// a preference saved then still selects it.
    #[default]
    #[serde(alias = "lime")]
    Gold,
    Midnight,
    Paper,
}
impl AppIcon {
    pub const ALL: [Self; 3] = [Self::Gold, Self::Midnight, Self::Paper];
    pub fn name(self) -> &'static str {
        match self {
            Self::Gold => "Gold",
            Self::Midnight => "Midnight",
            Self::Paper => "Paper",
        }
    }
    pub fn asset(self) -> &'static str {
        match self {
            Self::Gold => "app-icons/cereal-gold-thumb.png",
            Self::Midnight => "app-icons/cereal-midnight-thumb.png",
            Self::Paper => "app-icons/cereal-paper-thumb.png",
        }
    }
    fn png(self) -> &'static [u8] {
        match self {
            Self::Gold => include_bytes!("../assets/app-icons/cereal-gold.png"),
            Self::Midnight => include_bytes!("../assets/app-icons/cereal-midnight.png"),
            Self::Paper => include_bytes!("../assets/app-icons/cereal-paper.png"),
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Preferences {
    icon: AppIcon,
}
#[derive(Clone, Debug, Default)]
struct State {
    preferences: Preferences,
    error: Option<String>,
}
static STATE: OnceLock<Mutex<State>> = OnceLock::new();
fn path() -> Result<PathBuf, String> {
    Ok(root()?.join("desktop-preferences.json"))
}
fn root() -> Result<PathBuf, String> {
    let root = crate::environment::var_os("CEREAL_DATA_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .map(|p| PathBuf::from(p).join("Library/Application Support/Cereal"))
        })
        .ok_or("Cannot locate device preferences")?;
    Ok(root)
}
fn read(path: &Path) -> Result<Preferences, String> {
    read_as(path)
}
fn read_as<T: serde::de::DeserializeOwned + Default>(path: &Path) -> Result<T, String> {
    if !path.exists() {
        return Ok(T::default());
    }
    if fs::symlink_metadata(path)
        .map_err(|_| "Cannot read device preferences")?
        .file_type()
        .is_symlink()
    {
        return Err("Device preferences must be a regular file".into());
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|_| "Cannot read device preferences")?
        .take(4097)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read device preferences")?;
    if bytes.len() > 4096 {
        return Err("Device preferences are too large; the original file was preserved".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| {
        "Saved icon preference could not be read; the original file was preserved".into()
    })
}
fn state() -> &'static Mutex<State> {
    STATE.get_or_init(|| {
        Mutex::new(match path().and_then(|p| read(&p)) {
            Ok(preferences) => State {
                preferences,
                error: None,
            },
            Err(error) => State {
                preferences: Preferences::default(),
                error: Some(error),
            },
        })
    })
}
pub fn selected_icon() -> AppIcon {
    state()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .preferences
        .icon
}
pub fn preference_error() -> Option<String> {
    state()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .error
        .clone()
}
pub fn icon_supported() -> bool {
    cfg!(target_os = "macos")
}

fn write(path: &Path, preferences: &Preferences) -> Result<(), String> {
    write_as(path, "desktop-preferences", preferences)
}
/// `stem` names the backup of an unreadable file and the temporary file.
fn write_as<T: Serialize + serde::de::DeserializeOwned + Default>(
    path: &Path,
    stem: &str,
    preferences: &T,
) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or("Cannot locate device preference directory")?;
    fs::create_dir_all(parent).map_err(|_| "Cannot save device preferences")?;
    if path.exists() && read_as::<T>(path).is_err() {
        let metadata =
            fs::symlink_metadata(path).map_err(|_| "Cannot inspect existing preferences")?;
        if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > 4096 {
            return Err(
                "Existing device preferences were preserved; choose a valid data directory".into(),
            );
        }
        let backup = parent.join(format!("{stem}.invalid-{}.json", nonce()));
        fs::copy(path, backup).map_err(|_| "Cannot preserve unreadable preferences")?;
    }
    let bytes = serde_json::to_vec_pretty(preferences)
        .map_err(|_| "Cannot serialize device preferences")?;
    let temporary = parent.join(format!(".{stem}-{}.tmp", nonce()));
    let result = (|| -> Result<(), String> {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&temporary)
            .map_err(|_| "Cannot save device preferences")?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| "Cannot save device preferences")?;
        fs::rename(&temporary, path).map_err(|_| "Cannot save device preferences".to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
fn nonce() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

/// What this Mac shows of meetings: the menu bar item, reminders and the
/// floating indicator. Kept in a file of its own, so an older Cereal reading
/// `desktop-preferences.json` is not handed fields it does not know.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MeetingPreferences {
    /// The next meeting and its countdown beside the mark in the menu bar,
    /// and the events in its menu.
    pub menu_bar_events: bool,
    /// Whether "Coming up" and the menu include events with nobody else
    /// invited and no call link.
    pub events_without_participants: bool,
    /// A notification one minute before a meeting starts.
    pub scheduled_notifications: bool,
    /// A notification when a note's generated notes are ready (or could
    /// not be made) while Cereal is not the app in front.
    pub notes_notifications: bool,
    /// A notification when a recording stops on its own or loses an input.
    pub recording_notifications: bool,
    /// A notification when an account's session ends and it needs signing
    /// in to again.
    pub account_notifications: bool,
    /// The capsule that floats at the screen's edge while recording.
    pub live_indicator: bool,
    /// Calendars left out of "Coming up" and the menu on this Mac, as
    /// `"<AccountId>/<calendarId>"`. (Entries from before accounts are bare
    /// calendar ids; they get the migrated account's id.)
    pub hidden_calendars: Vec<String>,
    /// The scheme of the reminder identifiers registered with macOS (see
    /// `menubar::REMINDER_SCHEME`). 0 before reminders carried their account.
    pub reminder_scheme: u32,
    /// Apple's voice isolation on the microphone of every recording (see
    /// `audio::CaptureOptions`); system audio is never processed.
    pub voice_isolation: bool,
    /// The microphone recordings use, by Core Audio device UID. `None`
    /// follows the Mac's default input (see `audio::set_input_device`).
    pub microphone: Option<String>,
    /// The language live transcription is told to expect: an ISO 639 code,
    /// or `"auto"` to let the provider detect it. `None` until the person
    /// chooses: English on a Mac set to English, detection otherwise (see
    /// `transcript_panel::Language`).
    pub transcript_language: Option<String>,
}
impl Default for MeetingPreferences {
    fn default() -> Self {
        Self {
            menu_bar_events: true,
            events_without_participants: true,
            scheduled_notifications: false,
            notes_notifications: true,
            recording_notifications: true,
            account_notifications: true,
            live_indicator: true,
            hidden_calendars: Vec::new(),
            reminder_scheme: 0,
            voice_isolation: true,
            microphone: None,
            transcript_language: None,
        }
    }
}
/// What this Mac keeps about an account that is erased: the calendars of it
/// hidden here. (Meeting choices and dismissed pairs come with several
/// accounts.)
pub fn forget_account(preferences: &mut MeetingPreferences, account: &crate::accounts::AccountId) {
    let prefix = format!("{}/", account.as_str());
    preferences
        .hidden_calendars
        .retain(|hidden| !hidden.starts_with(&prefix));
}

static MEETING: OnceLock<Mutex<MeetingPreferences>> = OnceLock::new();
static MEETING_REVISION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
fn meeting_path() -> Result<PathBuf, String> {
    Ok(root()?.join("meeting-preferences.json"))
}
fn meeting_state() -> &'static Mutex<MeetingPreferences> {
    MEETING.get_or_init(|| {
        // Tests start from the defaults, never from the user's own file.
        if cfg!(test) {
            return Mutex::new(MeetingPreferences::default());
        }
        Mutex::new(
            meeting_path()
                .and_then(|path| read_as(&path))
                .unwrap_or_default(),
        )
    })
}
pub fn meeting() -> MeetingPreferences {
    meeting_state()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}
/// Counts the changes made to the meeting preferences since launch, so
/// what is drawn from them knows when to look again.
pub fn meeting_revision() -> u64 {
    MEETING_REVISION.load(std::sync::atomic::Ordering::Acquire)
}
/// Change the meeting preferences and save them. What is in memory changes
/// only if the file was written.
pub fn update_meeting(change: impl FnOnce(&mut MeetingPreferences)) -> Result<(), String> {
    let mut state = meeting_state().lock().unwrap_or_else(|e| e.into_inner());
    let mut next = state.clone();
    change(&mut next);
    if next == *state {
        return Ok(());
    }
    // Tests never write the user's own preferences.
    #[cfg(not(test))]
    meeting_path().and_then(|path| write_as(&path, "meeting-preferences", &next))?;
    *state = next;
    MEETING_REVISION.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
    Ok(())
}

/// The sidebar's narrowest, default and widest widths in points. Dragging its
/// edge moves between the limits; a double-click on the edge returns to the
/// default.
pub const SIDEBAR_MIN: f32 = 168.;
pub const SIDEBAR_DEFAULT: f32 = 192.;
pub const SIDEBAR_MAX: f32 = 320.;
/// The least room the page beside the sidebar keeps: the narrowest window
/// (760 pt) less the default sidebar. A wide sidebar gives way to it in a
/// narrow window, so the page is never crushed.
pub const SIDEBAR_PAGE_MIN: f32 = 568.;
/// Below this window width the sidebar is not beside the page: the sidebar
/// button slides it out over the page instead, as in the reference.
pub const SIDEBAR_OVERLAY_BELOW: f32 = 900.;

/// A width the sidebar may be given: within its limits, and the default for
/// anything that is not a number.
pub fn clamp_sidebar_width(width: f32) -> f32 {
    if width.is_finite() {
        width.clamp(SIDEBAR_MIN, SIDEBAR_MAX).round()
    } else {
        SIDEBAR_DEFAULT
    }
}
/// The width the sidebar is drawn at in a window `viewport` points wide: the
/// chosen width, less whatever the page beside it needs, never under the
/// minimum.
pub fn sidebar_width_in(chosen: f32, viewport: f32) -> f32 {
    let chosen = clamp_sidebar_width(chosen);
    if !viewport.is_finite() {
        return chosen;
    }
    chosen.min((viewport - SIDEBAR_PAGE_MIN).max(SIDEBAR_MIN))
}

/// How this Mac lays the window out. In a file of its own, like the meeting
/// preferences, so `desktop-preferences.json` keeps the fields older
/// versions know.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LayoutPreferences {
    /// The sidebar's width in points, as last dragged.
    pub sidebar_width: f32,
}
impl Default for LayoutPreferences {
    fn default() -> Self {
        Self {
            sidebar_width: SIDEBAR_DEFAULT,
        }
    }
}
impl LayoutPreferences {
    /// What was read from disk, made safe to use: a width outside the limits
    /// (an edited file, limits that have since changed) is brought inside.
    fn sanitized(mut self) -> Self {
        self.sidebar_width = clamp_sidebar_width(self.sidebar_width);
        self
    }
}
static LAYOUT: OnceLock<Mutex<LayoutPreferences>> = OnceLock::new();
fn layout_path() -> Result<PathBuf, String> {
    Ok(root()?.join("layout-preferences.json"))
}
fn read_layout(path: &Path) -> LayoutPreferences {
    read_as::<LayoutPreferences>(path)
        .unwrap_or_default()
        .sanitized()
}
fn write_layout(path: &Path, layout: &LayoutPreferences) -> Result<(), String> {
    write_as(path, "layout-preferences", &layout.clone().sanitized())
}
fn layout_state() -> &'static Mutex<LayoutPreferences> {
    LAYOUT.get_or_init(|| {
        // Tests start from the defaults, never from the user's own file.
        if cfg!(test) {
            return Mutex::new(LayoutPreferences::default());
        }
        Mutex::new(layout_path().map(|path| read_layout(&path)).unwrap_or_default())
    })
}
/// The sidebar's saved width, within its limits.
pub fn sidebar_width() -> f32 {
    layout_state()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .sidebar_width
}
/// Save the sidebar's width (clamped). What is in memory changes even if the
/// file cannot be written: the width in use is the one on screen.
pub fn set_sidebar_width(width: f32) -> Result<(), String> {
    let mut state = layout_state().lock().unwrap_or_else(|e| e.into_inner());
    let width = clamp_sidebar_width(width);
    if state.sidebar_width == width {
        return Ok(());
    }
    state.sidebar_width = width;
    // Tests never write the user's own preferences.
    #[cfg(not(test))]
    layout_path().and_then(|path| write_layout(&path, &state))?;
    Ok(())
}

pub fn apply_saved_icon() -> Result<(), String> {
    apply_icon(selected_icon())
}
pub fn select_icon(icon: AppIcon) -> Result<(), String> {
    let mut state = state().lock().unwrap_or_else(|e| e.into_inner());
    let old = state.preferences.icon;
    apply_icon(icon)?;
    if let Err(error) = path().and_then(|p| write(&p, &Preferences { icon })) {
        let _ = apply_icon(old);
        return Err(error);
    }
    state.preferences.icon = icon;
    state.error = None;
    Ok(())
}

#[cfg(target_os = "macos")]
#[allow(unexpected_cfgs)]
fn apply_icon(icon: AppIcon) -> Result<(), String> {
    use cocoa::base::{id, nil};
    use objc::{class, msg_send, sel, sel_impl};
    // Called only from the GPUI main thread during startup or an explicit click.
    unsafe {
        let bytes = icon.png();
        let data: id = msg_send![class!(NSData), dataWithBytes:bytes.as_ptr() length:bytes.len()];
        let image: id = msg_send![class!(NSImage), alloc];
        let image: id = msg_send![image, initWithData:data];
        if image == nil {
            return Err("The selected app icon could not be loaded".into());
        }
        let app: id = msg_send![class!(NSApplication), sharedApplication];
        let _: () = msg_send![app, setApplicationIconImage:image];
        let _: () = msg_send![image, release];
    }
    Ok(())
}
#[cfg(not(target_os = "macos"))]
fn apply_icon(_: AppIcon) -> Result<(), String> {
    Err("Changing the Dock icon is available on macOS".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    /// A directory of this test's own. The clock alone is not unique: tests
    /// run in parallel and macOS timestamps have microsecond granularity, so
    /// two tests could be handed the same name and delete each other's files.
    fn directory() -> PathBuf {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "cereal-icon-test-{}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            nonce()
        ));
        fs::create_dir_all(&p).unwrap();
        p
    }
    #[test]
    fn variants_are_original_pngs_and_preferences_roundtrip() {
        let dir = directory();
        let path = dir.join("prefs.json");
        assert_eq!(read(&path).unwrap().icon, AppIcon::Gold);
        // The default is written as "gold"; a file saved while the icon was
        // called Lime still loads, and selects Gold.
        assert_eq!(serde_json::to_string(&AppIcon::Gold).unwrap(), r#""gold""#);
        fs::write(&path, br#"{"icon":"lime"}"#).unwrap();
        assert_eq!(read(&path).unwrap().icon, AppIcon::Gold);
        assert_eq!(AppIcon::Gold.name(), "Gold");
        fs::remove_file(&path).unwrap();
        for icon in AppIcon::ALL {
            assert!(icon.png().starts_with(b"\x89PNG\r\n\x1a\n"));
            write(&path, &Preferences { icon }).unwrap();
            assert_eq!(read(&path).unwrap().icon, icon);
        }
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn unreadable_metadata_is_preserved_before_explicit_reset() {
        let dir = directory();
        let path = dir.join("prefs.json");
        fs::write(&path, b"broken-json").unwrap();
        assert!(read(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"broken-json");
        write(
            &path,
            &Preferences {
                icon: AppIcon::Paper,
            },
        )
        .unwrap();
        assert!(fs::read_dir(&dir).unwrap().flatten().any(|p| {
            p.file_name()
                .to_string_lossy()
                .starts_with("desktop-preferences.invalid-")
        }));
        assert_eq!(read(&path).unwrap().icon, AppIcon::Paper);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn meeting_preferences_default_to_showing_and_roundtrip_in_their_own_file() {
        let dir = directory();
        let path = dir.join("meeting-preferences.json");
        let defaults: MeetingPreferences = read_as(&path).unwrap();
        assert!(defaults.menu_bar_events && defaults.events_without_participants);
        assert!(defaults.live_indicator && !defaults.scheduled_notifications);
        // Notices that answer something happening on its own are on until
        // turned off; macOS is asked for permission when one is first needed.
        assert!(
            defaults.notes_notifications
                && defaults.recording_notifications
                && defaults.account_notifications
        );
        assert!(defaults.voice_isolation);
        let saved = MeetingPreferences {
            menu_bar_events: false,
            scheduled_notifications: true,
            hidden_calendars: vec!["a".into()],
            ..Default::default()
        };
        write_as(&path, "meeting-preferences", &saved).unwrap();
        assert_eq!(read_as::<MeetingPreferences>(&path).unwrap(), saved);
        // A file from a later version, with a setting this one does not
        // know, still loads.
        fs::write(&path, br#"{"menu_bar_events":false,"later":1}"#).unwrap();
        let older = read_as::<MeetingPreferences>(&path).unwrap();
        assert!(!older.menu_bar_events);
        // A file from before the notices had settings has them on.
        assert!(older.notes_notifications && older.recording_notifications);
        // A file saved before the voice isolation setting turns it on; one
        // with it turned off keeps it off.
        assert!(older.voice_isolation);
        fs::write(&path, br#"{"voice_isolation":false}"#).unwrap();
        assert!(!read_as::<MeetingPreferences>(&path).unwrap().voice_isolation);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn sidebar_width_is_clamped_to_its_limits_and_rounded() {
        assert_eq!(clamp_sidebar_width(SIDEBAR_DEFAULT), SIDEBAR_DEFAULT);
        assert_eq!(clamp_sidebar_width(0.), SIDEBAR_MIN);
        assert_eq!(clamp_sidebar_width(-40.), SIDEBAR_MIN);
        assert_eq!(clamp_sidebar_width(SIDEBAR_MIN - 0.4), SIDEBAR_MIN);
        assert_eq!(clamp_sidebar_width(10_000.), SIDEBAR_MAX);
        assert_eq!(clamp_sidebar_width(f32::INFINITY), SIDEBAR_DEFAULT);
        assert_eq!(clamp_sidebar_width(f32::NAN), SIDEBAR_DEFAULT);
        // Whole points: the edge never rests between two pixels.
        assert_eq!(clamp_sidebar_width(240.4), 240.);
        assert_eq!(clamp_sidebar_width(240.6), 241.);
        assert!(SIDEBAR_MIN < SIDEBAR_DEFAULT && SIDEBAR_DEFAULT < SIDEBAR_MAX);
    }
    #[test]
    fn a_wide_sidebar_gives_way_to_the_page_in_a_narrow_window() {
        // A wide window shows the chosen width.
        assert_eq!(sidebar_width_in(SIDEBAR_MAX, 1440.), SIDEBAR_MAX);
        assert_eq!(sidebar_width_in(SIDEBAR_MIN, 1440.), SIDEBAR_MIN);
        // The narrowest window (760) keeps the page its 568 pt: the widest
        // sidebar is drawn at the default width there, and the default and
        // anything narrower are untouched.
        assert_eq!(sidebar_width_in(SIDEBAR_MAX, 760.), SIDEBAR_DEFAULT);
        assert_eq!(sidebar_width_in(SIDEBAR_DEFAULT, 760.), SIDEBAR_DEFAULT);
        assert_eq!(sidebar_width_in(SIDEBAR_MIN, 760.), SIDEBAR_MIN);
        // In between, the page keeps exactly its minimum.
        assert_eq!(sidebar_width_in(SIDEBAR_MAX, 820.), 820. - SIDEBAR_PAGE_MIN);
        // It is never drawn narrower than its minimum, whatever the window.
        assert_eq!(sidebar_width_in(SIDEBAR_MAX, 300.), SIDEBAR_MIN);
        assert_eq!(sidebar_width_in(SIDEBAR_MAX, f32::NAN), SIDEBAR_MAX);
        for viewport in [760., 800., 900., 1200., 2000.] {
            for chosen in [0., SIDEBAR_MIN, SIDEBAR_DEFAULT, 250., SIDEBAR_MAX, 900.] {
                let width = sidebar_width_in(chosen, viewport);
                assert!((SIDEBAR_MIN..=SIDEBAR_MAX).contains(&width));
                assert!(viewport - width >= SIDEBAR_PAGE_MIN);
            }
        }
    }
    #[test]
    fn layout_preferences_default_roundtrip_and_survive_bad_files() {
        let dir = directory();
        let path = dir.join("layout-preferences.json");
        // Nothing saved: the default width.
        assert_eq!(read_layout(&path).sidebar_width, SIDEBAR_DEFAULT);
        write_layout(&path, &LayoutPreferences { sidebar_width: 264. }).unwrap();
        assert_eq!(read_layout(&path).sidebar_width, 264.);
        // A width outside the limits is saved, and read, inside them.
        write_layout(&path, &LayoutPreferences { sidebar_width: 4000. }).unwrap();
        assert_eq!(read_layout(&path).sidebar_width, SIDEBAR_MAX);
        fs::write(&path, br#"{"sidebar_width":12}"#).unwrap();
        assert_eq!(read_layout(&path).sidebar_width, SIDEBAR_MIN);
        // A file from a later version, with a field this one does not know.
        fs::write(&path, br#"{"sidebar_width":210,"later":true}"#).unwrap();
        assert_eq!(read_layout(&path).sidebar_width, 210.);
        // One without the width, or that cannot be read at all: the default,
        // and the unreadable file is kept aside when the next width is saved.
        fs::write(&path, br#"{}"#).unwrap();
        assert_eq!(read_layout(&path).sidebar_width, SIDEBAR_DEFAULT);
        fs::write(&path, b"not json").unwrap();
        assert_eq!(read_layout(&path).sidebar_width, SIDEBAR_DEFAULT);
        write_layout(&path, &LayoutPreferences { sidebar_width: 200. }).unwrap();
        assert_eq!(read_layout(&path).sidebar_width, 200.);
        assert!(fs::read_dir(&dir).unwrap().flatten().any(|p| {
            p.file_name()
                .to_string_lossy()
                .starts_with("layout-preferences.invalid-")
        }));
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn oversized_preferences_are_not_overwritten() {
        let dir = directory();
        let path = dir.join("prefs.json");
        fs::write(&path, vec![b' '; 4097]).unwrap();
        assert!(write(&path, &Preferences::default()).is_err());
        assert_eq!(fs::metadata(&path).unwrap().len(), 4097);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn the_sound_settings_target_is_the_sound_pane() {
        assert_eq!(
            SOUND_SETTINGS_URL,
            "x-apple.systempreferences:com.apple.Sound-Settings.extension"
        );
    }
}
