//! The desktop half of the live transcription accuracy harness
//! (`scripts/accuracy/run.ts`). Test builds only.
//!
//! `accuracy_drive` runs the real [`crate::live::Session`] against a running
//! server, fed by a capture that this test writes the way the native writer
//! does: WAV files that grow every 10 ms, listed in a `manifest.json` the
//! session follows. The audio comes from a fixture (16 kHz PCM16 WAV per
//! channel). Everything the session reports is written as JSON lines for the
//! harness to score. It is ignored, and does nothing without its
//! configuration, so `cargo test` never runs it:
//!
//! ```text
//! CEREAL_ACCURACY=<config.json> cargo test -p cereal accuracy_drive -- --ignored --exact
//! ```
//!
//! The configuration (written by run.ts) names the live endpoint and token,
//! the fixture files, where to write events, and a control file the harness
//! appends commands to: `end` (Stop: the capture finishes and the session
//! delivers the rest), `pause` / `resume` (the recording is put on hold: the
//! capture stops growing), `sever` (cut the TCP socket), `quit` (drop the
//! session at once, as quitting the app does). The capture's clock advances
//! at most 200 ms per step, so a process stopped with SIGSTOP (a laptop
//! asleep) resumes with no audio for the time it was stopped.
//!
//! Capture files and the manifest live in a fresh temporary directory, never
//! in the app's data folder.
use crate::live::{Event, Resolved, Session, Source, Target};
use serde::Deserialize;
use std::{
    fs::File,
    io::{BufRead, BufReader, Seek, SeekFrom, Write},
    os::unix::fs::FileExt,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Config {
    url: String,
    token: String,
    capture_id: String,
    #[serde(default)]
    base_ms: u64,
    #[serde(default)]
    language: Option<String>,
    events: PathBuf,
    control: PathBuf,
    /// 16 kHz PCM16 mono WAVs; a missing channel is not captured.
    mic: Option<PathBuf>,
    system: Option<PathBuf>,
    /// The capture's own rate: 48 kHz float like the microphone, or 16 kHz
    /// PCM16 so the fake recogniser's coded samples pass unchanged.
    #[serde(default = "default_rate")]
    write_rate: u32,
    #[serde(default)]
    pcm16: bool,
    #[serde(default = "default_speed")]
    speed: f64,
    /// Fixture position the capture starts at, and where it ends (it ends
    /// there as a Stop would, `end` included).
    #[serde(default)]
    from_ms: u64,
    #[serde(default)]
    until_ms: Option<u64>,
    /// System audio starts this much later than the microphone (its first
    /// sample is what was playing then: the tap missed what came before),
    /// and its clock runs this many parts per million fast.
    #[serde(default)]
    system_delay_ms: u64,
    #[serde(default)]
    system_skew_ppm: f64,
    /// Capture already on disk before the session starts (it was offline).
    #[serde(default)]
    prerecorded_ms: u64,
}
fn default_rate() -> u32 {
    48_000
}
fn default_speed() -> f64 {
    1.
}

fn read_fixture(path: &Path) -> Vec<i16> {
    let bytes = std::fs::read(path).expect("fixture WAV");
    let mut at = 12;
    while at + 8 <= bytes.len() {
        let size = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
        if &bytes[at..at + 4] == b"data" {
            let end = (at + 8 + size).min(bytes.len());
            return bytes[at + 8..end].chunks_exact(2).map(|pair| i16::from_le_bytes([pair[0], pair[1]])).collect();
        }
        at += 8 + size + size % 2;
    }
    panic!("{} has no data chunk", path.display());
}

/// A WAV written and checkpointed as the native writer does: samples first,
/// then the lengths in the header.
struct Writer {
    file: File,
    frames: u64,
    pcm16: bool,
}

impl Writer {
    fn create(path: &Path, rate: u32, pcm16: bool) -> Self {
        let (code, bits, width) = if pcm16 { (1u16, 16u16, 2u32) } else { (3, 32, 4) };
        let mut header = Vec::new();
        header.extend(b"RIFF");
        header.extend(48u32.to_le_bytes());
        header.extend(b"WAVEfmt ");
        header.extend(16u32.to_le_bytes());
        header.extend(code.to_le_bytes());
        header.extend(1u16.to_le_bytes());
        header.extend(rate.to_le_bytes());
        header.extend((rate * width).to_le_bytes());
        header.extend((width as u16).to_le_bytes());
        header.extend(bits.to_le_bytes());
        header.extend(b"fact");
        header.extend(4u32.to_le_bytes());
        header.extend(0u32.to_le_bytes());
        header.extend(b"data");
        header.extend(0u32.to_le_bytes());
        let mut file = File::options().write(true).create_new(true).open(path).unwrap();
        file.write_all(&header).unwrap();
        Self { file, frames: 0, pcm16 }
    }
    fn append(&mut self, samples: &[f32]) {
        let width = if self.pcm16 { 2 } else { 4 };
        let bytes: Vec<u8> = if self.pcm16 {
            samples.iter().flat_map(|s| ((s * 32768.).round().clamp(-32768., 32767.) as i16).to_le_bytes()).collect()
        } else {
            samples.iter().flat_map(|s| s.to_le_bytes()).collect()
        };
        self.file.write_all_at(&bytes, 56 + self.frames * width).unwrap();
        self.frames += samples.len() as u64;
        let data = (self.frames * width) as u32;
        self.file.write_all_at(&data.to_le_bytes(), 52).unwrap();
        self.file.write_all_at(&(self.frames as u32).to_le_bytes(), 44).unwrap();
        self.file.write_all_at(&(48 + data).to_le_bytes(), 4).unwrap();
    }
}

/// One channel's audio at the capture's rate, read forward: the fixture
/// resampled linearly from 16 kHz (the session's own resampler then brings
/// it back), its clock off by `skew_ppm`.
struct Feed {
    pcm: Vec<i16>,
    /// Fixture samples per capture sample.
    step: f64,
    position: f64,
    nearest: bool,
}

impl Feed {
    fn take(&mut self, count: usize, out: &mut Vec<f32>) -> bool {
        out.clear();
        for _ in 0..count {
            let index = self.position.floor() as usize;
            if index + 1 >= self.pcm.len() {
                return !out.is_empty();
            }
            let fraction = (self.position - index as f64) as f32;
            let (a, b) = (self.pcm[index] as f32, self.pcm[index + 1] as f32);
            // Coded audio (the fake recogniser's) must keep its exact values: nearest sample.
            out.push(if self.nearest { a } else { a + (b - a) * fraction } / 32768.);
            self.position += self.step;
        }
        true
    }
}

fn manifest(directory: &Path, capture_id: &str) -> crate::audio::CaptureManifest {
    crate::audio::CaptureManifest {
        version: 1,
        capture_id: capture_id.into(),
        note_id: 1,
        manifest_path: directory.join("manifest.json"),
        directory: directory.to_path_buf(),
        started_at: "2026-11-16T10:00:00Z".into(),
        finished_at: None,
        stats: Default::default(),
        error: None,
    }
}

fn save(manifest: &crate::audio::CaptureManifest) {
    let temporary = manifest.directory.join(".manifest.tmp");
    std::fs::write(&temporary, serde_json::to_vec(manifest).unwrap()).unwrap();
    std::fs::rename(temporary, &manifest.manifest_path).unwrap();
}

fn event_json(event: &Event, at: f64) -> serde_json::Value {
    use serde_json::json;
    match event {
        Event::Status(phase, behind) => json!({"t": at, "type": "status", "phase": format!("{phase:?}"), "behind": behind}),
        Event::Revision(revision) => json!({"t": at, "type": "revision", "revision": revision}),
        Event::Interim(interim) => json!({"t": at, "type": "interim", "channel": interim.channel.name(), "utteranceId": interim.utterance_id, "rev": interim.rev,
            "text": interim.text, "startMs": interim.start_ms, "speakerId": interim.speaker_id, "speakerLabel": interim.speaker_label}),
        Event::Final { utterance_id, revision, segment } => json!({"t": at, "type": "final", "utteranceId": utterance_id, "revision": revision, "segment": segment}),
        Event::Speakers(merges) => json!({"t": at, "type": "speakers", "merges": merges.iter().map(|m| json!({"from": m.from, "to": m.to})).collect::<Vec<_>>()}),
        Event::Relabels(relabels) => json!({"t": at, "type": "relabels", "relabels": relabels.iter().map(|r| json!({"segmentId": r.segment_id, "speakerId": r.speaker_id, "speakerLabel": r.speaker_label})).collect::<Vec<_>>()}),
        Event::Gap { channel, from_ms, to_ms } => json!({"t": at, "type": "gap", "channel": channel.name(), "fromMs": from_ms, "toMs": to_ms}),
        Event::Delivered(delivery) => json!({"t": at, "type": "delivered", "delivery": format!("{delivery:?}")}),
        Event::Ended => json!({"t": at, "type": "ended"}),
    }
}

#[test]
#[ignore = "the accuracy harness runs it (scripts/accuracy/run.ts)"]
fn accuracy_drive() {
    let Ok(path) = std::env::var("CEREAL_ACCURACY") else { return };
    let config: Config = serde_json::from_slice(&std::fs::read(&path).unwrap()).expect("configuration");
    let directory = std::env::temp_dir().join(format!("cereal-accuracy-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let mut events = File::create(&config.events).unwrap();
    let started = Instant::now();
    let log = |events: &mut File, value: serde_json::Value| {
        writeln!(events, "{value}").unwrap();
        events.flush().unwrap();
    };
    let rate = config.write_rate;
    let step = 16_000. / rate as f64;
    let from = config.from_ms as f64 * 16.;
    let mut feeds: Vec<(usize, Feed, Option<Writer>, u64)> = Vec::new();
    for (index, source) in [&config.mic, &config.system].into_iter().enumerate() {
        let Some(source) = source else { continue };
        let skew = if index == 1 { 1. + config.system_skew_ppm / 1e6 } else { 1. };
        let delay = if index == 1 { config.system_delay_ms } else { 0 };
        feeds.push((index, Feed { pcm: read_fixture(source), step: step * skew, position: from + delay as f64 * 16., nearest: config.pcm16 }, None, delay));
    }
    let mut manifest = manifest(&directory, &config.capture_id);
    for (index, ..) in &feeds {
        manifest.stats.requested_channels |= 1 << index;
        let stats = if *index == 0 { &mut manifest.stats.mic } else { &mut manifest.stats.system };
        stats.state = "starting".into();
    }
    save(&manifest);
    let until = config.until_ms.map(|ms| ms.saturating_sub(config.from_ms));
    // The capture clock, in capture milliseconds since its start.
    let mut clock = 0f64;
    let mut block = Vec::new();
    let mut write_until = |clock: f64, feeds: &mut Vec<(usize, Feed, Option<Writer>, u64)>, manifest: &mut crate::audio::CaptureManifest| -> bool {
        let mut more = false;
        let mut changed = false;
        for (index, feed, writer, delay) in feeds.iter_mut() {
            let channel_ms = clock - *delay as f64;
            if channel_ms < 0. {
                more = true;
                continue;
            }
            if writer.is_none() {
                let name = if *index == 0 { "mic.wav" } else { "system.wav" };
                let path = directory.join(name);
                *writer = Some(Writer::create(&path, rate, config.pcm16));
                let stats = if *index == 0 { &mut manifest.stats.mic } else { &mut manifest.stats.system };
                stats.state = "recording".into();
                stats.sample_rate = rate as f64;
                stats.paths.push(path.to_string_lossy().into_owned());
                stats.path = stats.paths.last().cloned();
                changed = true;
            }
            let writer = writer.as_mut().unwrap();
            let goal = (channel_ms * rate as f64 / 1000.) as u64;
            if goal > writer.frames {
                let count = (goal - writer.frames) as usize;
                if feed.take(count, &mut block) {
                    writer.append(&block);
                    more |= block.len() == count;
                }
            } else {
                more = true;
            }
        }
        if changed {
            save(manifest);
        }
        more
    };
    if config.prerecorded_ms > 0 {
        clock = config.prerecorded_ms as f64;
        write_until(clock, &mut feeds, &mut manifest);
    }
    let target = Target {
        endpoint: crate::accounts::client::LiveEndpoint::for_test(config.url.clone(), &config.token),
        capture_id: config.capture_id.clone(),
        base_ms: config.base_ms,
        language: config.language.clone(),
    };
    let key = {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        config.capture_id.hash(&mut hasher);
        hasher.finish()
    };
    let mut session = Some(Session::start(key, Source::File(manifest.manifest_path.clone()), move || Resolved::Ready(target.clone()), || true));
    log(&mut events, serde_json::json!({"t": started.elapsed().as_secs_f64() * 1000., "type": "started", "captureMs": clock}));
    let mut control = BufReader::new(File::options().read(true).write(true).create(true).truncate(false).open(&config.control).unwrap());
    control.seek(SeekFrom::End(0)).unwrap();
    let (mut capturing, mut paused, mut last) = (true, false, Instant::now());
    let finish = |manifest: &mut crate::audio::CaptureManifest, session: &Option<Session>| {
        manifest.finished_at = Some("2026-11-16T11:00:00Z".into());
        save(manifest);
        if let Some(session) = session {
            session.end();
        }
    };
    let deadline = Duration::from_secs(4 * 3600);
    loop {
        // Commands from the harness.
        let mut line = String::new();
        while control.read_line(&mut line).map(|count| count > 0).unwrap_or(false) {
            let command = line.trim().to_owned();
            line.clear();
            log(&mut events, serde_json::json!({"t": started.elapsed().as_secs_f64() * 1000., "type": "command", "command": command, "captureMs": clock}));
            match command.as_str() {
                "end" if capturing => {
                    capturing = false;
                    finish(&mut manifest, &session);
                }
                "pause" => {
                    paused = true;
                    if let Some(session) = &session {
                        session.pause();
                    }
                }
                "resume" => {
                    paused = false;
                    last = Instant::now();
                    if let Some(session) = &session {
                        session.resume();
                    }
                }
                "sever" => {
                    if let Some(session) = &session {
                        session.sever();
                    }
                }
                "quit" => {
                    session = None;
                }
                _ => {}
            }
        }
        // The capture grows with the clock, at most 200 ms per step.
        let now = Instant::now();
        let elapsed = now.duration_since(last).as_secs_f64() * 1000.;
        last = now;
        if capturing && !paused {
            clock += (elapsed * config.speed).min(200.);
            let stop = until.is_some_and(|until| clock >= until as f64);
            if stop {
                clock = until.unwrap() as f64;
            }
            let more = write_until(clock, &mut feeds, &mut manifest);
            if stop || !more {
                capturing = false;
                log(&mut events, serde_json::json!({"t": started.elapsed().as_secs_f64() * 1000., "type": "captureEnded", "captureMs": clock}));
                finish(&mut manifest, &session);
            }
        }
        let Some(live) = &session else { break };
        let mut ended = false;
        while let Ok(event) = live.events.try_recv() {
            ended |= event == Event::Ended;
            log(&mut events, event_json(&event, started.elapsed().as_secs_f64() * 1000.));
        }
        if ended || started.elapsed() > deadline {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    drop(session);
    log(&mut events, serde_json::json!({"t": started.elapsed().as_secs_f64() * 1000., "type": "exit"}));
    let _ = std::fs::remove_dir_all(&directory);
}
