//! Real local audio capture. The native bridge owns CoreAudio/AVFoundation;
//! one worker owns its opaque handle, so GPUI never waits for permission or IO.
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, Copy)]
pub struct CaptureOptions {
    pub microphone: bool,
    pub system: bool,
}
impl Default for CaptureOptions {
    fn default() -> Self {
        Self {
            microphone: true,
            system: true,
        }
    }
}
impl CaptureOptions {
    fn flags(self) -> u32 {
        u32::from(self.microphone) | (u32::from(self.system) << 1)
    }
    pub fn production() -> Self {
        let args: Vec<_> = std::env::args().collect();
        if args
            .iter()
            .any(|a| a == "--audio-mic-only" || a == "--mic-only")
        {
            Self {
                microphone: true,
                system: false,
            }
        } else if args
            .iter()
            .any(|a| a == "--audio-system-only" || a == "--system-only")
        {
            Self {
                microphone: false,
                system: true,
            }
        } else {
            Self::default()
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChannelError {
    pub code: String,
    pub message: String,
    pub os_status: i32,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ChannelStats {
    pub state: String,
    pub error: Option<ChannelError>,
    pub level: f32,
    pub frames_received: u64,
    pub frames_written: u64,
    pub dropped_frames: u64,
    pub sample_rate: f64,
    pub channels: u32,
    pub path: Option<String>,
    pub paths: Vec<String>,
    pub last_buffer_uptime: f64,
    pub self_process_excluded: bool,
}
impl ChannelStats {
    fn initial(requested: bool) -> Self {
        Self {
            state: if requested { "starting" } else { "disabled" }.into(),
            ..Default::default()
        }
    }
    pub fn seconds(&self) -> f64 {
        let seconds = self.frames_written as f64 / self.sample_rate;
        if self.sample_rate > 0. && seconds.is_finite() {
            seconds
        } else {
            0.
        }
    }
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct NativeStats {
    pub version: u32,
    pub phase: String,
    pub requested_channels: u32,
    pub mic: ChannelStats,
    pub system: ChannelStats,
}
impl NativeStats {
    pub fn has_audio(&self) -> bool {
        self.mic.frames_received > 0 || self.system.frames_received > 0
    }
    pub fn error_message(&self) -> Option<String> {
        let errors: Vec<_> = [("Microphone", &self.mic), ("System audio", &self.system)]
            .into_iter()
            .filter_map(|(name, channel)| {
                channel
                    .error
                    .as_ref()
                    .map(|error| format!("{name}: {}", error.message))
            })
            .collect();
        (!errors.is_empty()).then(|| errors.join(" · "))
    }
    fn resolved(&self) -> bool {
        [&self.mic, &self.system]
            .iter()
            .all(|c| c.state != "starting")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureManifest {
    pub version: u32,
    pub capture_id: String,
    pub note_id: u64,
    pub directory: PathBuf,
    pub manifest_path: PathBuf,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub stats: NativeStats,
    pub error: Option<String>,
}
impl CaptureManifest {
    pub fn completed(&self) -> bool {
        self.finished_at.is_some()
    }
    pub fn error_message(&self) -> Option<String> {
        self.error.clone().or_else(|| self.stats.error_message())
    }
    pub fn audio_seconds(&self) -> f64 {
        self.stats.mic.seconds().max(self.stats.system.seconds())
    }
}

/// Where the captures of a note are kept under `root`: `note-<file id>`. A
/// handle never names anything on disk.
pub fn capture_directory(root: &Path, note_id: u64) -> PathBuf {
    root.join(format!("note-{}", crate::accounts::handle::file_id(note_id)))
}

#[derive(Clone)]
pub struct CaptureMonitor(Arc<Mutex<CaptureManifest>>);
impl CaptureMonitor {
    pub fn snapshot(&self) -> CaptureManifest {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}
pub struct Capture {
    monitor: CaptureMonitor,
    stop: mpsc::Sender<()>,
}
impl Capture {
    pub fn start(note_id: u64, root: &Path, options: CaptureOptions) -> Result<Self, String> {
        // On disk a note is always named by its file id: the directory
        // `note-<id>` and the manifest never hold a handle.
        let note_id = crate::accounts::handle::file_id(note_id);
        if options.flags() == 0 {
            return Err("Enable microphone or system audio before recording".into());
        }
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos();
        let capture_id = format!("capture-{}-{stamp}", std::process::id());
        let directory = capture_directory(root, note_id).join(&capture_id);
        private_directory(&directory)?;
        let manifest = CaptureManifest {
            version: 1,
            capture_id,
            note_id,
            manifest_path: directory.join("manifest.json"),
            directory,
            started_at: chrono::Utc::now().to_rfc3339(),
            finished_at: None,
            error: None,
            stats: NativeStats {
                version: 1,
                phase: "starting".into(),
                requested_channels: options.flags(),
                mic: ChannelStats::initial(options.microphone),
                system: ChannelStats::initial(options.system),
            },
        };
        let monitor = CaptureMonitor(Arc::new(Mutex::new(manifest)));
        let state = monitor.clone();
        let (stop, receiver) = mpsc::channel();
        let fixture = fixture::source();
        thread::Builder::new()
            .name("local-audio-capture".into())
            .spawn(move || match fixture {
                Some(source) => fixture::run(state, receiver, &source),
                None => run_capture(state, receiver, options),
            })
            .map_err(|e| format!("Cannot start audio worker: {e}"))?;
        Ok(Self { monitor, stop })
    }
    pub fn monitor(&self) -> CaptureMonitor {
        self.monitor.clone()
    }
    pub fn request_stop(&self) {
        let _ = self.stop.send(());
    }
}
impl Drop for Capture {
    fn drop(&mut self) {
        self.request_stop();
    }
}

// Serializes native ownership across quick Stop/New note/Resume handoffs.
static NATIVE_OWNER: Mutex<()> = Mutex::new(());
fn run_capture(state: CaptureMonitor, stop: mpsc::Receiver<()>, options: CaptureOptions) {
    let _owner = NATIVE_OWNER.lock().unwrap_or_else(|e| e.into_inner());
    if stop.try_recv().is_ok() {
        finish(&state, None);
        return;
    }
    if let Err(error) = write_manifest(&state.snapshot()) {
        finish(&state, Some(error));
        return;
    }
    let directory = state.snapshot().directory;
    let mut native = match NativeCapture::start(&directory, options.flags()) {
        Ok(native) => native,
        Err(error) => {
            finish(&state, Some(error));
            return;
        }
    };
    let mut last_checkpoint = Instant::now();
    loop {
        match native.poll(false) {
            Ok(stats) => {
                let failed = stats.phase == "failed";
                state.0.lock().unwrap_or_else(|e| e.into_inner()).stats = stats;
                if failed {
                    break;
                }
            }
            Err(error) => {
                state.0.lock().unwrap_or_else(|e| e.into_inner()).error = Some(error);
                break;
            }
        }
        // Keep paths and received/written counters recoverable while capture is
        // alive. A process exit must not leave a header-only initial manifest
        // beside a real checkpointed WAV that the next launch cannot discover.
        if last_checkpoint.elapsed() >= Duration::from_secs(1) {
            if let Err(error) = write_manifest(&state.snapshot()) {
                state.0.lock().unwrap_or_else(|e| e.into_inner()).error = Some(error);
                break;
            }
            last_checkpoint = Instant::now();
        }
        match stop.recv_timeout(Duration::from_millis(80)) {
            Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
    let error = match native.poll(true) {
        Ok(stats) => {
            state.0.lock().unwrap_or_else(|e| e.into_inner()).stats = stats;
            None
        }
        Err(error) => Some(error),
    };
    drop(native); // drains and releases native resources before publishing final completion
    finish(&state, error);
}
/// A capture fed from a WAV file instead of the audio devices, for running
/// the recording pipeline end to end where there is no microphone (automated
/// sessions). `CEREAL_CAPTURE_FIXTURE=<mono WAV>` plays the file into
/// `mic.wav` in real time, 10 ms at a time, with the header checkpointed
/// after every write exactly as the native writer does, then silence until
/// the capture is stopped. Honoured only together with an isolated
/// `CEREAL_DATA_DIR`, so a real workspace can never record from a file.
mod fixture {
    use super::*;
    use std::os::unix::fs::FileExt;

    pub fn source() -> Option<PathBuf> {
        crate::environment::var_os("CEREAL_DATA_DIR")?;
        crate::environment::var_os("CEREAL_CAPTURE_FIXTURE")
            .filter(|path| !path.is_empty())
            .map(PathBuf::from)
    }

    /// The samples and rate of a mono 16-bit or 32-bit float WAV.
    fn read(path: &Path) -> Result<(u32, Vec<f32>), String> {
        let data = fs::read(path).map_err(|e| format!("Cannot read the capture fixture: {e}"))?;
        if data.len() < 12 || &data[..4] != b"RIFF" || &data[8..12] != b"WAVE" {
            return Err("The capture fixture is not a WAV file".into());
        }
        let word = |at: usize| u32::from_le_bytes(data[at..at + 4].try_into().unwrap());
        let (mut at, mut format) = (12, None);
        while at + 8 <= data.len() {
            let (size, body) = (word(at + 4) as usize, at + 8);
            let end = (body + size).min(data.len());
            match &data[at..at + 4] {
                b"fmt " if size >= 16 && body + 16 <= data.len() => {
                    let short = |at: usize| u16::from_le_bytes([data[at], data[at + 1]]);
                    format = Some((short(body), short(body + 2), word(body + 4), short(body + 14)));
                }
                b"data" => {
                    let samples = match format {
                        Some((1, 1, _, 16)) => data[body..end]
                            .chunks_exact(2)
                            .map(|s| i16::from_le_bytes([s[0], s[1]]) as f32 / 32768.)
                            .collect(),
                        Some((3, 1, _, 32)) => data[body..end]
                            .chunks_exact(4)
                            .map(|s| f32::from_le_bytes(s.try_into().unwrap()))
                            .collect(),
                        _ => return Err("The capture fixture must be mono PCM16 or Float32".into()),
                    };
                    return Ok((format.map_or(0, |format| format.2), samples));
                }
                _ => {}
            }
            at = body + size + size % 2;
        }
        Err("The capture fixture has no audio".into())
    }

    pub fn run(state: CaptureMonitor, stop: mpsc::Receiver<()>, source: &Path) {
        let _owner = NATIVE_OWNER.lock().unwrap_or_else(|e| e.into_inner());
        let outcome = play(&state, &stop, source);
        finish(&state, outcome.err());
    }

    /// One channel of a fixture capture: its samples and the file they are
    /// played into.
    struct Track {
        system: bool,
        samples: Vec<f32>,
        rate: u32,
        file: fs::File,
        name: String,
        frames: u64,
        level: f32,
    }

    fn track(state: &CaptureMonitor, source: &Path, system: bool) -> Result<Track, String> {
        let (rate, samples) = read(source)?;
        if !(8_000..=192_000).contains(&rate) {
            return Err("The capture fixture has an unsupported sample rate".into());
        }
        let path = state
            .snapshot()
            .directory
            .join(if system { "system.wav" } else { "mic.wav" });
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options
            .open(&path)
            .map_err(|e| format!("Cannot create the fixture capture: {e}"))?;
        // The native writer's 56-byte header: fmt, fact, data.
        let mut header = Vec::with_capacity(56);
        header.extend(b"RIFF");
        header.extend(48u32.to_le_bytes());
        header.extend(b"WAVEfmt ");
        header.extend(16u32.to_le_bytes());
        header.extend(3u16.to_le_bytes());
        header.extend(1u16.to_le_bytes());
        header.extend(rate.to_le_bytes());
        header.extend((rate * 4).to_le_bytes());
        header.extend(4u16.to_le_bytes());
        header.extend(32u16.to_le_bytes());
        header.extend(b"fact");
        header.extend(4u32.to_le_bytes());
        header.extend(0u32.to_le_bytes());
        header.extend(b"data");
        header.extend(0u32.to_le_bytes());
        file.write_all_at(&header, 0)
            .map_err(|e| format!("Cannot write the fixture capture: {e}"))?;
        Ok(Track {
            system,
            samples,
            rate,
            file,
            name: path.to_string_lossy().into_owned(),
            frames: 0,
            level: 0.,
        })
    }

    fn play(state: &CaptureMonitor, stop: &mpsc::Receiver<()>, source: &Path) -> Result<(), String> {
        write_manifest(&state.snapshot())?;
        let mut tracks = vec![track(state, source, false)?];
        // `CEREAL_CAPTURE_FIXTURE_SYSTEM=<mono WAV>` plays a second file
        // into `system.wav` alongside, as the other side of the meeting.
        if let Some(system) = crate::environment::var_os("CEREAL_CAPTURE_FIXTURE_SYSTEM")
            .filter(|path| !path.is_empty())
        {
            tracks.push(track(state, Path::new(&system), true)?);
        }
        let both = tracks.len() > 1;
        let io = |e: std::io::Error| format!("Cannot write the fixture capture: {e}");
        let started = Instant::now();
        let mut last_manifest = Instant::now();
        loop {
            let elapsed = started.elapsed().as_millis() as u64;
            for track in &mut tracks {
                // As many 10 ms blocks as real time has got to.
                let block = (track.rate / 100) as usize;
                let due = elapsed * track.rate as u64 / 1000;
                track.level = 0.;
                while track.frames + block as u64 <= due {
                    let at = track.frames as usize;
                    let mut samples_block = vec![0f32; block];
                    if at < track.samples.len() {
                        let end = (at + block).min(track.samples.len());
                        samples_block[..end - at].copy_from_slice(&track.samples[at..end]);
                    }
                    track.level = samples_block.iter().fold(0f32, |peak, s| peak.max(s.abs()));
                    let bytes: Vec<u8> = samples_block.iter().flat_map(|s| s.to_le_bytes()).collect();
                    // Samples first, then the lengths that advertise them.
                    let frames = track.frames + block as u64;
                    track.file.write_all_at(&bytes, 56 + track.frames * 4).map_err(io)?;
                    track.frames = frames;
                    track.file.write_all_at(&(frames as u32 * 4).to_le_bytes(), 52).map_err(io)?;
                    track.file.write_all_at(&(frames as u32).to_le_bytes(), 44).map_err(io)?;
                    track.file.write_all_at(&(48 + frames as u32 * 4).to_le_bytes(), 4).map_err(io)?;
                }
            }
            {
                let mut manifest = state.0.lock().unwrap_or_else(|e| e.into_inner());
                manifest.stats.phase = "recording".into();
                // System audio is part of a fixture capture only when it
                // was given a file of its own.
                manifest.stats.requested_channels = if both { 3 } else { 1 };
                if !both {
                    manifest.stats.system = ChannelStats::initial(false);
                }
                for track in &tracks {
                    let channel = if track.system {
                        &mut manifest.stats.system
                    } else {
                        &mut manifest.stats.mic
                    };
                    channel.state = "recording".into();
                    channel.sample_rate = track.rate as f64;
                    channel.channels = 1;
                    channel.frames_received = track.frames;
                    channel.frames_written = track.frames;
                    channel.level = track.level;
                    channel.path = Some(track.name.clone());
                    channel.paths = vec![track.name.clone()];
                }
            }
            if last_manifest.elapsed() >= Duration::from_secs(1) {
                write_manifest(&state.snapshot())?;
                last_manifest = Instant::now();
            }
            match stop.recv_timeout(Duration::from_millis(10)) {
                Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
        for track in &tracks {
            track.file.sync_all().map_err(io)?;
        }
        let mut manifest = state.0.lock().unwrap_or_else(|e| e.into_inner());
        manifest.stats.phase = "stopped".into();
        manifest.stats.mic.state = "stopped".into();
        manifest.stats.mic.level = 0.;
        if both {
            manifest.stats.system.state = "stopped".into();
            manifest.stats.system.level = 0.;
        }
        Ok(())
    }
}

fn finish(state: &CaptureMonitor, error: Option<String>) {
    let mut manifest = state.snapshot();
    manifest.finished_at = Some(chrono::Utc::now().to_rfc3339());
    if let Some(error) = error {
        manifest.error = Some(error);
    }
    if manifest.stats.phase == "starting" {
        manifest.stats.phase = "stopped".into();
    }
    if let Err(error) = write_manifest(&manifest) {
        manifest.error = Some(error);
    }
    *state.0.lock().unwrap_or_else(|e| e.into_inner()) = manifest;
}
fn private_directory(path: &Path) -> Result<(), String> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(path)
        .map_err(|e| format!("Cannot create audio directory: {e}"))
}
fn write_manifest(manifest: &CaptureManifest) -> Result<(), String> {
    let temporary = manifest.directory.join(".manifest.tmp");
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .map_err(|e| format!("Cannot write audio manifest: {e}"))?;
    let data = serde_json::to_vec_pretty(manifest).map_err(|e| e.to_string())?;
    file.write_all(&data)
        .and_then(|()| file.sync_all())
        .map_err(|e| format!("Cannot save audio manifest: {e}"))?;
    fs::rename(temporary, &manifest.manifest_path)
        .map_err(|e| format!("Cannot finalize audio manifest: {e}"))
}

#[cfg(target_os = "macos")]
struct NativeCapture(*mut std::ffi::c_void);
#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn mn_audio_start(
        directory: *const std::ffi::c_char,
        channels: u32,
        error: *mut std::ffi::c_char,
        capacity: usize,
    ) -> *mut std::ffi::c_void;
    fn mn_audio_poll(
        handle: *mut std::ffi::c_void,
        output: *mut std::ffi::c_char,
        capacity: usize,
    ) -> usize;
    fn mn_audio_stop(
        handle: *mut std::ffi::c_void,
        output: *mut std::ffi::c_char,
        capacity: usize,
    ) -> usize;
    fn mn_audio_destroy(handle: *mut std::ffi::c_void);
}
#[cfg(target_os = "macos")]
impl NativeCapture {
    fn start(directory: &Path, channels: u32) -> Result<Self, String> {
        let directory =
            std::ffi::CString::new(directory.to_str().ok_or("Audio directory is not UTF-8")?)
                .map_err(|e| e.to_string())?;
        let mut error = vec![0u8; 4096];
        let handle = unsafe {
            mn_audio_start(
                directory.as_ptr(),
                channels,
                error.as_mut_ptr().cast(),
                error.len(),
            )
        };
        if handle.is_null() {
            let end = error.iter().position(|b| *b == 0).unwrap_or(error.len());
            let message = String::from_utf8_lossy(&error[..end]).into_owned();
            Err(if message.is_empty() {
                "Native audio capture could not start".into()
            } else {
                message
            })
        } else {
            Ok(Self(handle))
        }
    }
    fn poll(&mut self, stop: bool) -> Result<NativeStats, String> {
        let mut output = vec![0u8; 65536];
        for _ in 0..3 {
            let required = unsafe {
                if stop {
                    mn_audio_stop(self.0, output.as_mut_ptr().cast(), output.len())
                } else {
                    mn_audio_poll(self.0, output.as_mut_ptr().cast(), output.len())
                }
            };
            if required == 0 {
                return Err("Native audio handle is unavailable".into());
            }
            if required > 1_048_576 {
                return Err("Native audio statistics exceed the supported size".into());
            }
            if required > output.len() {
                output.resize(required, 0);
                continue;
            }
            return serde_json::from_slice(&output[..required.saturating_sub(1)])
                .map_err(|e| format!("Invalid native audio statistics: {e}"));
        }
        Err("Native audio statistics changed while reading".into())
    }
}
#[cfg(target_os = "macos")]
impl Drop for NativeCapture {
    fn drop(&mut self) {
        unsafe {
            mn_audio_destroy(self.0);
        }
    }
}
#[cfg(not(target_os = "macos"))]
struct NativeCapture;
#[cfg(not(target_os = "macos"))]
impl NativeCapture {
    fn start(_: &Path, _: u32) -> Result<Self, String> {
        Err("Real audio capture requires macOS".into())
    }
    fn poll(&mut self, _: bool) -> Result<NativeStats, String> {
        Err("Real audio capture requires macOS".into())
    }
}

/// What a listening check hears on one channel.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CheckChannel {
    /// Input level right now, 0..=1.
    pub level: f32,
    /// Audio frames that have arrived since the check began.
    pub frames: u64,
    /// macOS refused the channel (the permission was denied).
    pub refused: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CheckLevels {
    /// Microphone, then system audio.
    pub channels: [CheckChannel; 2],
}

/// Listening without recording: the same native capture a recording uses,
/// started with nothing to write to, so the welcome can show that the
/// microphone and system audio really are heard. Starting it is also how
/// macOS is asked for each permission, exactly as starting a recording is.
/// Dropping it lets go of the devices.
pub struct LevelCheck {
    levels: Arc<Mutex<CheckLevels>>,
    stop: mpsc::Sender<()>,
}

/// Channel flag for the native capture: keep levels, write no files.
const LEVELS_ONLY: u32 = 4;

impl LevelCheck {
    pub fn start(microphone: bool, system: bool) -> Option<Self> {
        let flags = u32::from(microphone) | (u32::from(system) << 1);
        // Never from a test or a review fixture: those open no devices.
        if flags == 0 || preview_mode() {
            return None;
        }
        let levels = Arc::new(Mutex::new(CheckLevels::default()));
        let shared = levels.clone();
        let (stop, receiver) = mpsc::channel();
        thread::Builder::new()
            .name("audio-level-check".into())
            .spawn(move || {
                let _owner = NATIVE_OWNER.lock().unwrap_or_else(|e| e.into_inner());
                if receiver.try_recv().is_ok() {
                    return;
                }
                // The directory is never created: nothing is written.
                let nowhere = std::env::temp_dir().join("cereal-level-check");
                let Ok(mut native) = NativeCapture::start(&nowhere, flags | LEVELS_ONLY) else {
                    return;
                };
                loop {
                    if let Ok(stats) = native.poll(false) {
                        *shared.lock().unwrap_or_else(|e| e.into_inner()) = check_levels(&stats);
                    }
                    match receiver.recv_timeout(Duration::from_millis(40)) {
                        Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }
                }
                let _ = native.poll(true);
            })
            .ok()?;
        Some(Self { levels, stop })
    }

    pub fn levels(&self) -> CheckLevels {
        *self.levels.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Drop for LevelCheck {
    fn drop(&mut self) {
        let _ = self.stop.send(());
    }
}

/// What the native capture's statistics mean for a listening check.
pub fn check_levels(stats: &NativeStats) -> CheckLevels {
    let channel = |stats: &ChannelStats| CheckChannel {
        level: if stats.level.is_finite() {
            stats.level.clamp(0., 1.)
        } else {
            0.
        },
        frames: stats.frames_received,
        refused: stats
            .error
            .as_ref()
            .is_some_and(|error| error.code == "permission_denied"),
    };
    CheckLevels {
        channels: [channel(&stats.mic), channel(&stats.system)],
    }
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn mn_permission_status(channel: u32) -> i32;
}

/// What macOS says about a capture permission, without asking for it:
/// channel 0 is the microphone (an `AVAuthorizationStatus`), channel 1 is
/// system audio (0 granted, 1 denied, 2 not asked). `None` when macOS has no
/// answer to give, and always in tests and review fixtures.
pub fn permission_status(channel: u32) -> Option<i32> {
    if preview_mode() {
        return None;
    }
    #[cfg(target_os = "macos")]
    {
        let status = unsafe { mn_permission_status(channel) };
        (status >= 0).then_some(status)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = channel;
        None
    }
}

pub fn preview_mode() -> bool {
    // Asked dozens of times per render; the arguments never change.
    static PREVIEW: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *PREVIEW.get_or_init(|| {
        std::env::args()
            .any(|a| a.starts_with("--view=") || a == "--smoke-test" || a == "--motion-tour")
            || cfg!(test)
    })
}

#[cfg(target_os = "macos")]
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFRunLoopDefaultMode: *const std::ffi::c_void;
    fn CFRunLoopRunInMode(
        mode: *const std::ffi::c_void,
        seconds: f64,
        return_after_source: u8,
    ) -> i32;
}
fn diagnostic_wait(duration: Duration) {
    let start = Instant::now();
    #[cfg(target_os = "macos")]
    unsafe {
        CFRunLoopRunInMode(kCFRunLoopDefaultMode, duration.as_secs_f64(), 0);
    }
    if let Some(remaining) = duration.checked_sub(start.elapsed()) {
        thread::sleep(remaining);
    }
}

/// Diagnostic path runs before GPUI starts. No synthetic samples or transcript are produced.
pub fn probe_from_args(args: &[String]) -> Option<Result<bool, String>> {
    let index = args
        .iter()
        .position(|a| a == "--audio-probe" || a.starts_with("--audio-probe="))?;
    Some((|| {
        let directory = args[index]
            .strip_prefix("--audio-probe=")
            .map(str::to_owned)
            .or_else(|| {
                args.get(index + 1)
                    .filter(|s| !s.starts_with("--"))
                    .cloned()
            })
            .ok_or(
                "Usage: --audio-probe OUTPUT_DIRECTORY [--seconds 5] [--mic-only|--system-only]",
            )?;
        let seconds = args
            .iter()
            .position(|a| a == "--seconds")
            .and_then(|i| args.get(i + 1))
            .map(|s| s.parse::<f64>())
            .transpose()
            .map_err(|_| "--seconds must be a number")?
            .unwrap_or(5.);
        if !seconds.is_finite() || !(0.1..=3600.).contains(&seconds) {
            return Err("--seconds must be between 0.1 and 3600".into());
        }
        let capture = Capture::start(0, Path::new(&directory), CaptureOptions::production())?;
        let monitor = capture.monitor();
        let timeout = Instant::now();
        loop {
            let snapshot = monitor.snapshot();
            if snapshot.completed()
                || (snapshot.stats.resolved() && snapshot.stats.has_audio())
                || snapshot.stats.phase == "failed"
                // Allow the system tap's three bounded first-frame windows,
                // including format-query and recreation backoff time.
                || timeout.elapsed() > Duration::from_secs(60)
            {
                break;
            }
            diagnostic_wait(Duration::from_millis(50));
        }
        let snapshot = monitor.snapshot();
        if snapshot.stats.has_audio() {
            let recording = Instant::now();
            while recording.elapsed() < Duration::from_secs_f64(seconds) {
                diagnostic_wait(Duration::from_millis(20));
            }
        }
        capture.request_stop();
        let deadline = Instant::now();
        while !monitor.snapshot().completed() && deadline.elapsed() < Duration::from_secs(10) {
            diagnostic_wait(Duration::from_millis(20));
        }
        let snapshot = monitor.snapshot();
        println!(
            "{}",
            serde_json::to_string_pretty(&snapshot).map_err(|e| e.to_string())?
        );
        if !snapshot.completed() {
            eprintln!(
                "Capture did not finalize before the timeout. macOS may still be waiting for System Audio Recording approval; resolve its permission prompt and retry. No successful capture is claimed."
            );
        }
        let success = snapshot.completed()
            && snapshot.error_message().is_none()
            && (snapshot.stats.requested_channels & 1 == 0
                || snapshot.stats.mic.frames_written > 0)
            && (snapshot.stats.requested_channels & 2 == 0
                || snapshot.stats.system.frames_written > 0);
        Ok(success)
    })())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_listening_check_reads_levels_arrivals_and_refusals() {
        let stats: NativeStats = serde_json::from_value(serde_json::json!({"version":1,"phase":"partial","requested_channels":3,"mic":{"state":"recording","level":0.42,"frames_received":4800},"system":{"state":"failed","error":{"code":"permission_denied","message":"Denied","os_status":0}}})).unwrap();
        let levels = check_levels(&stats);
        assert_eq!(levels.channels[0], CheckChannel { level: 0.42, frames: 4800, refused: false });
        assert_eq!(levels.channels[1], CheckChannel { level: 0., frames: 0, refused: true });
        // A device problem is not a refusal, and a wild level is kept in range.
        let stats: NativeStats = serde_json::from_value(serde_json::json!({"mic":{"state":"failed","level":7.0,"error":{"code":"no_input","message":"None","os_status":0}}})).unwrap();
        let levels = check_levels(&stats);
        assert!(!levels.channels[0].refused);
        assert_eq!(levels.channels[0].level, 1.);
        // Tests never open a device or ask macOS anything.
        assert!(LevelCheck::start(true, true).is_none());
        assert_eq!(permission_status(0), None);
    }
    #[test]
    fn live_manifest_checkpoint_preserves_actual_paths_and_counters_before_stop() {
        let directory = std::env::temp_dir().join(format!(
            "meeting-manifest-checkpoint-{}",
            uuid::Uuid::new_v4()
        ));
        private_directory(&directory).unwrap();
        let mut manifest = CaptureManifest {
            version: 1,
            capture_id: "fixture-capture".into(),
            note_id: 9,
            directory: directory.clone(),
            manifest_path: directory.join("manifest.json"),
            started_at: "2026-11-13T05:00:00Z".into(),
            finished_at: None,
            error: None,
            stats: NativeStats::default(),
        };
        write_manifest(&manifest).unwrap();
        manifest.stats.phase = "recording".into();
        manifest.stats.system.state = "recording".into();
        manifest.stats.system.sample_rate = 48000.;
        manifest.stats.system.frames_received = 48000;
        manifest.stats.system.frames_written = 48000;
        manifest.stats.system.path =
            Some(directory.join("system.wav").to_string_lossy().into_owned());
        manifest.stats.system.paths = manifest.stats.system.path.clone().into_iter().collect();
        write_manifest(&manifest).unwrap();
        let saved: CaptureManifest =
            serde_json::from_slice(&fs::read(&manifest.manifest_path).unwrap()).unwrap();
        assert!(saved.finished_at.is_none());
        assert_eq!(saved.stats.system.frames_written, 48000);
        assert_eq!(saved.stats.system.paths.len(), 1);
        assert_eq!(saved.audio_seconds(), 1.);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn quiet_frames_are_real_audio_and_channel_failures_remain_distinct() {
        let stats = NativeStats {
            mic: ChannelStats {
                frames_received: 48_000,
                frames_written: 48_000,
                sample_rate: 48_000.,
                level: 0.,
                ..Default::default()
            },
            system: ChannelStats {
                error: Some(ChannelError {
                    code: "permission".into(),
                    message: "Allow system audio".into(),
                    os_status: -1,
                }),
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(stats.has_audio());
        assert_eq!(stats.mic.seconds(), 1.);
        assert_eq!(stats.mic.level, 0.);
        assert!(stats.error_message().unwrap().starts_with("System audio:"));
    }
    #[test]
    fn distinct_capture_channels_round_trip_without_transcript_fields() {
        let stats: NativeStats = serde_json::from_value(serde_json::json!({"version":1,"phase":"partial","requested_channels":3,"mic":{"state":"recording","path":"mic.wav","frames_written":10},"system":{"state":"failed","error":{"code":"denied","message":"Permission denied","os_status":0}}})).unwrap();
        assert_eq!(stats.mic.path.as_deref(), Some("mic.wav"));
        assert_eq!(stats.system.state, "failed");
        assert!(
            !serde_json::to_string(&stats)
                .unwrap()
                .contains("transcript")
        );
    }
}
