//! Live transcription uplink: one WebSocket per recording.
//!
//! A session thread follows the capture's WAV files as the native writer
//! grows them (it checkpoints the file after every 10 ms write, so the file is
//! the buffer), converts them to 16 kHz PCM16 and sends 100–250 ms frames to
//! `GET /api/v1/notes/:id/live`. Interim and final text comes back on the
//! same socket and is handed to the window as [`Event`]s.
//!
//! The wire protocol's schema is `packages/contracts/src/live.ts`.
//!
//! Nothing here touches the capture or the durable upload. The stream can
//! fail in any way at any time and the meeting is still transcribed from the
//! uploaded parts exactly as before; the server skips what the stream covered.
//!
//! * The cursor of each channel is a sample index at 16 kHz on that channel's
//!   capture timeline (its files concatenated in manifest order). At most one
//!   frame of audio is held in memory; when the socket is slow the cursor
//!   simply falls behind the end of the file.
//! * After a drop the session reconnects with back-off and resumes from the
//!   server's `acceptFromSample`. A backlog longer than the server's resume
//!   window is skipped to five seconds before the end of the capture (to
//!   half the window before it, if the window is shorter than ten seconds).
//! * This module has no UI and no global state, so it is tested against a
//!   local WebSocket server (see the tests at the end).
use meeting_notes_api_types::TranscriptSegmentDTO;
use serde::Deserialize;
use std::{
    fs::File,
    io,
    net::{TcpStream, ToSocketAddrs},
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex, OnceLock, mpsc},
    thread,
    time::{Duration, Instant},
};
use tungstenite::{Message, WebSocket, stream::MaybeTlsStream};

// ---------------------------------------------------------------- protocol

pub const PROTOCOL: u8 = 1;
/// Bytes before the samples of a binary audio frame.
pub const FRAME_HEADER: usize = 12;
/// The rate every channel is sent at.
pub const RATE: u32 = 16_000;
/// 100 ms: a frame is not sent until it holds this much, except to flush.
pub const FRAME_MIN: usize = RATE as usize / 10;
/// 250 ms: the most one frame carries.
pub const FRAME_MAX: usize = RATE as usize / 4;
/// How much faster than real time a backlog is replayed.
const CATCH_UP: f64 = 2.;
/// Where a skipped backlog resumes: this long before the end of the capture,
/// or half the resume window before it if that is less.
const SKIP_TO_MS: u64 = 5_000;
const BACK_OFF: [Duration; 5] = [
    Duration::from_millis(500),
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(4),
    Duration::from_secs(8),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    Mic,
    System,
}

impl Channel {
    pub const ALL: [Channel; 2] = [Channel::Mic, Channel::System];
    pub fn code(self) -> u8 {
        self as u8
    }
    pub fn name(self) -> &'static str {
        match self {
            Channel::Mic => "mic",
            Channel::System => "system",
        }
    }
    fn index(self) -> usize {
        self as usize
    }
}

/// One binary audio frame: version, channel, two zero bytes, the index of the
/// first sample as an unsigned 64-bit little-endian integer, then the samples.
pub fn encode_frame(channel: Channel, first_sample: u64, pcm: &[i16]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(FRAME_HEADER + pcm.len() * 2);
    frame.extend([PROTOCOL, channel.code(), 0, 0]);
    frame.extend(first_sample.to_le_bytes());
    for sample in pcm {
        frame.extend(sample.to_le_bytes());
    }
    frame
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamState {
    #[default]
    Live,
    CatchingUp,
    ProviderUnavailable,
    Paused,
    /// A state added after this client was built.
    #[serde(other)]
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadyStream {
    pub channel: Channel,
    pub accept_from_sample: u64,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct SpeakerMerge {
    pub from: String,
    pub to: String,
}

/// Everything the server sends. Unknown message types decode as `Unknown`
/// and are ignored, so additive protocol changes do not break old clients.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ServerMessage {
    Ready {
        streams: Vec<ReadyStream>,
        resume_window_ms: u64,
        transcript_revision: u64,
    },
    Ack {
        channel: Channel,
        /// Exclusive: the next sample the server wants.
        through_sample: u64,
    },
    Interim {
        channel: Channel,
        utterance_id: String,
        rev: u64,
        text: String,
        start_ms: u64,
        #[serde(default)]
        speaker_id: Option<String>,
        #[serde(default)]
        speaker_label: Option<String>,
    },
    Final {
        utterance_id: String,
        transcript_revision: u64,
        segment: TranscriptSegmentDTO,
    },
    Speakers {
        merges: Vec<SpeakerMerge>,
    },
    Gap {
        channel: Channel,
        from_ms: u64,
        to_ms: u64,
    },
    Status {
        channel: Channel,
        state: StreamState,
    },
    Error {
        code: String,
        #[serde(default)]
        retryable: bool,
    },
    #[serde(other)]
    Unknown,
}

/// A text frame from the server, or `None` when it is not a protocol message.
pub fn decode(text: &str) -> Option<ServerMessage> {
    serde_json::from_str(text).ok()
}

// ---------------------------------------------------------------- resampler

/// Phases of the filter bank; the kernel between two phases is interpolated.
const PHASES: usize = 512;
/// Stop-band attenuation the low-pass is designed for, in dB.
const ATTENUATION: f64 = 70.;

/// Mono sample-rate conversion to PCM16 with a Kaiser-windowed sinc low-pass.
///
/// The pass band ends at 7/8 of the lower Nyquist frequency and the stop band
/// starts at that Nyquist frequency, so nothing above 8 kHz folds back when
/// 48 kHz or 44.1 kHz becomes 16 kHz. Output sample `n` sits at input
/// position `n * in_rate / out_rate`, computed in integers: there is no
/// accumulated error, an hour of audio is exactly an hour of output, and a
/// conversion restarted with [`Resampler::seek`] produces the same samples
/// as one that ran from the beginning.
pub struct Resampler {
    /// Input samples per output sample, as a reduced fraction.
    step: (u64, u64),
    /// Kernel half-width in input samples (0 when the rates are equal).
    half: usize,
    /// `PHASES + 1` rows of `2 * half` coefficients.
    bank: Vec<f32>,
    /// Input not yet fully consumed, starting at absolute index `origin`.
    history: Vec<f32>,
    origin: u64,
    /// The next output sample to produce.
    next: u64,
}

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// Zeroth-order modified Bessel function of the first kind.
fn bessel_i0(x: f64) -> f64 {
    let (mut sum, mut term, mut k) = (1., 1., 1.);
    while term > sum * 1e-12 {
        term *= (x / (2. * k)) * (x / (2. * k));
        sum += term;
        k += 1.;
    }
    sum
}

fn pcm16(value: f32) -> i16 {
    if value.is_finite() {
        (value.clamp(-1., 1.) * 32767.).round() as i16
    } else {
        0
    }
}

impl Resampler {
    pub fn new(in_rate: u32, out_rate: u32) -> Self {
        let divisor = gcd(in_rate as u64, out_rate as u64).max(1);
        let step = (in_rate as u64 / divisor, out_rate as u64 / divisor);
        if in_rate == out_rate {
            return Self {
                step,
                half: 0,
                bank: Vec::new(),
                history: Vec::new(),
                origin: 0,
                next: 0,
            };
        }
        // Frequencies in cycles per input sample.
        let nyquist = 0.5 * (out_rate as f64 / in_rate as f64).min(1.);
        let cutoff = nyquist * 15. / 16.;
        let transition = nyquist / 8.;
        let beta = 0.1102 * (ATTENUATION - 8.7);
        let taps = (ATTENUATION - 8.) / (2.285 * std::f64::consts::TAU * transition);
        let half = (taps / 2.).ceil() as usize;
        let width = 2 * half;
        let norm = bessel_i0(beta);
        let mut bank = vec![0f32; (PHASES + 1) * width];
        for phase in 0..=PHASES {
            let fraction = phase as f64 / PHASES as f64;
            let row = &mut bank[phase * width..(phase + 1) * width];
            let mut sum = 0.;
            let mut values = vec![0f64; width];
            for (tap, value) in values.iter_mut().enumerate() {
                // Tap `tap` weighs input `i0 + tap - (half - 1)`.
                let t = fraction - (tap as f64 - (half as f64 - 1.));
                let x = t / half as f64;
                if x.abs() >= 1. {
                    continue;
                }
                let phase_angle = std::f64::consts::TAU * cutoff * t;
                let sinc = if phase_angle.abs() < 1e-9 {
                    1.
                } else {
                    phase_angle.sin() / phase_angle
                };
                let window = bessel_i0(beta * (1. - x * x).sqrt()) / norm;
                *value = 2. * cutoff * sinc * window;
                sum += *value;
            }
            // Unit gain at DC for every phase, so a steady level has no ripple.
            for (coefficient, value) in row.iter_mut().zip(values) {
                *coefficient = (value / sum) as f32;
            }
        }
        Self {
            step,
            half,
            bank,
            history: Vec::new(),
            origin: 0,
            next: 0,
        }
    }

    /// How many output samples `frames` input samples become.
    pub fn output_len(in_rate: u32, out_rate: u32, frames: u64) -> u64 {
        let divisor = gcd(in_rate as u64, out_rate as u64).max(1) as u128;
        let (num, den) = (in_rate as u128 / divisor, out_rate as u128 / divisor);
        (frames as u128 * den).div_ceil(num) as u64
    }

    /// Restart so that the next output is sample `output`. Returns the index
    /// of the first input sample that must then be pushed.
    pub fn seek(&mut self, output: u64) -> u64 {
        self.next = output;
        self.history.clear();
        let position = (output as u128 * self.step.0 as u128 / self.step.1 as u128) as u64;
        self.origin = if self.half == 0 {
            position
        } else {
            (position + 1).saturating_sub(self.half as u64)
        };
        self.origin
    }

    /// The next output sample this will produce.
    #[cfg(test)]
    pub fn position(&self) -> u64 {
        self.next
    }

    /// Feed the next input samples (they continue where the last push, or the
    /// index returned by `seek`, left off) and append whatever output they
    /// complete.
    pub fn push(&mut self, input: &[f32], out: &mut Vec<i16>) {
        self.history.extend_from_slice(input);
        let end = self.origin + self.history.len() as u64;
        self.produce(end, out);
    }

    /// The input ended after `total` samples: produce the tail, treating
    /// everything past the end as silence.
    pub fn finish(&mut self, total: u64, out: &mut Vec<i16>) {
        let limit = Self::limit(self.step, total);
        while self.next < limit {
            let available = self.origin + self.history.len() as u64;
            self.history
                .extend(std::iter::repeat_n(0., self.half.max(1)));
            self.produce_until(available + self.half.max(1) as u64, limit, out);
        }
    }

    fn limit(step: (u64, u64), total: u64) -> u64 {
        (total as u128 * step.1 as u128).div_ceil(step.0 as u128) as u64
    }

    fn produce(&mut self, end: u64, out: &mut Vec<i16>) {
        self.produce_until(end, u64::MAX, out);
    }

    /// Produce outputs while their whole kernel lies before input `end`.
    fn produce_until(&mut self, end: u64, limit: u64, out: &mut Vec<i16>) {
        let (num, den) = (self.step.0 as u128, self.step.1 as u128);
        let width = 2 * self.half;
        loop {
            if self.next >= limit {
                break;
            }
            let exact = self.next as u128 * num;
            let position = (exact / den) as u64;
            let remainder = (exact % den) as u64;
            if self.half == 0 {
                if position >= end {
                    break;
                }
                out.push(pcm16(self.history[(position - self.origin) as usize]));
                self.next += 1;
                continue;
            }
            if position + self.half as u64 >= end {
                break;
            }
            let scaled = remainder as u128 * PHASES as u128;
            let phase = (scaled / den) as usize;
            let blend = (scaled % den) as f32 / den as f32;
            let row = &self.bank[phase * width..(phase + 1) * width];
            let next_row = &self.bank[(phase + 1) * width..(phase + 2) * width];
            // Tap 0 weighs input `position - (half - 1)`, which is before the
            // start of the file for the first few outputs: silence, skipped.
            let first = position as i64 - (self.half as i64 - 1);
            let skip = (self.origin as i64 - first).max(0) as usize;
            let base = (first + skip as i64 - self.origin as i64) as usize;
            let samples = &self.history[base..base + width - skip];
            let sum: f32 = if blend == 0. {
                samples
                    .iter()
                    .zip(&row[skip..])
                    .map(|(sample, weight)| sample * weight)
                    .sum()
            } else {
                samples
                    .iter()
                    .zip(row[skip..].iter().zip(&next_row[skip..]))
                    .map(|(sample, (weight, next))| sample * (weight + (next - weight) * blend))
                    .sum()
            };
            out.push(pcm16(sum));
            self.next += 1;
        }
        // Forget input that no later output can reach.
        let exact = self.next as u128 * num;
        let position = (exact / den) as u64;
        let keep = if self.half == 0 {
            position
        } else {
            (position + 1).saturating_sub(self.half as u64)
        };
        if keep > self.origin {
            let drop = ((keep - self.origin) as usize).min(self.history.len());
            self.history.drain(..drop);
            self.origin += drop as u64;
        }
    }
}

// ---------------------------------------------------------------- following a WAV

/// A mono WAV that may still be growing.
struct Wav {
    file: File,
    rate: u32,
    float: bool,
    data_start: u64,
}

impl Wav {
    /// `Ok(None)` while the header is not on disk yet.
    fn open(path: &Path) -> io::Result<Option<Self>> {
        use std::os::unix::fs::FileExt;
        let file = File::open(path)?;
        let mut header = [0u8; 4096];
        let mut filled = 0;
        while filled < header.len() {
            match file.read_at(&mut header[filled..], filled as u64)? {
                0 => break,
                count => filled += count,
            }
        }
        let header = &header[..filled];
        if header.len() < 12 {
            return Ok(None);
        }
        let invalid = |message: &'static str| io::Error::new(io::ErrorKind::InvalidData, message);
        if &header[..4] != b"RIFF" || &header[8..12] != b"WAVE" {
            return Err(invalid("not a WAV file"));
        }
        let word = |at: usize| u32::from_le_bytes(header[at..at + 4].try_into().unwrap());
        let (mut at, mut format) = (12, None);
        while at + 8 <= header.len() {
            let size = word(at + 4) as usize;
            let body = at + 8;
            match &header[at..at + 4] {
                b"fmt " if body + 16 <= header.len() => {
                    let short = |at: usize| u16::from_le_bytes([header[at], header[at + 1]]);
                    format = Some((short(body), short(body + 2), word(body + 4), short(body + 14)));
                }
                b"data" => {
                    let (code, channels, rate, bits) =
                        format.ok_or_else(|| invalid("WAV data precedes its format"))?;
                    if channels != 1
                        || !(8_000..=192_000).contains(&rate)
                        || !matches!((code, bits), (1, 16) | (3, 32))
                    {
                        return Err(invalid("unsupported WAV encoding"));
                    }
                    return Ok(Some(Self {
                        file,
                        rate,
                        float: code == 3,
                        data_start: body as u64,
                    }));
                }
                _ => {}
            }
            at = body + size + size % 2;
        }
        Ok(None)
    }

    fn width(&self) -> u64 {
        if self.float { 4 } else { 2 }
    }

    /// Frames that are complete on disk now. The writer advertises a length
    /// only after the samples are written, so the smaller of the advertised
    /// length and the file's own size is always real audio.
    fn frames(&self) -> io::Result<u64> {
        use std::os::unix::fs::FileExt;
        let body = self.file.metadata()?.len().saturating_sub(self.data_start);
        let mut word = [0u8; 4];
        self.file.read_exact_at(&mut word, self.data_start - 4)?;
        Ok(body.min(u32::from_le_bytes(word) as u64) / self.width())
    }

    fn read(&self, frame: u64, count: usize, bytes: &mut Vec<u8>, out: &mut Vec<f32>) -> io::Result<()> {
        use std::os::unix::fs::FileExt;
        let width = self.width() as usize;
        bytes.resize(count * width, 0);
        self.file
            .read_exact_at(bytes, self.data_start + frame * width as u64)?;
        out.clear();
        if self.float {
            out.extend(bytes.chunks_exact(4).map(|sample| {
                let value = f32::from_le_bytes(sample.try_into().unwrap());
                if value.is_finite() { value } else { 0. }
            }));
        } else {
            out.extend(
                bytes
                    .chunks_exact(2)
                    .map(|sample| i16::from_le_bytes([sample[0], sample[1]]) as f32 / 32768.),
            );
        }
        Ok(())
    }
}

struct SourceFile {
    path: PathBuf,
    /// First 16 kHz sample of this file on the channel's timeline.
    start: u64,
    /// Samples it contributes, known once it has stopped growing.
    len: Option<u64>,
    /// Its input frames, once it has stopped growing.
    frames: u64,
}

struct Reading {
    file: usize,
    wav: Wav,
    resampler: Resampler,
    /// The next input frame to read.
    frame: u64,
}

/// One channel of one capture as a stream of 16 kHz samples: its files in
/// manifest order, read forward from a cursor.
pub struct Track {
    files: Vec<SourceFile>,
    /// The capture has finished: the last file will not grow.
    done: bool,
    /// Index of `pending[0]`: the next sample to send.
    cursor: u64,
    pending: Vec<i16>,
    reading: Option<Reading>,
    /// The file that is still growing, kept open to ask how long it is.
    tail: Option<(usize, Wav)>,
    bytes: Vec<u8>,
    input: Vec<f32>,
}

impl Default for Track {
    fn default() -> Self {
        Self::new()
    }
}

impl Track {
    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            done: false,
            cursor: 0,
            pending: Vec::new(),
            reading: None,
            tail: None,
            bytes: Vec::new(),
            input: Vec::new(),
        }
    }

    /// Tell the track what the manifest lists now.
    pub fn sync(&mut self, paths: &[String], done: bool) {
        for path in paths.iter().skip(self.files.len()) {
            self.files.push(SourceFile {
                path: PathBuf::from(path),
                start: 0,
                len: None,
                frames: 0,
            });
        }
        self.done |= done;
        // A file is final once a later one exists or the capture has ended.
        let mut start = Some(0);
        let count = self.files.len();
        for (index, file) in self.files.iter_mut().enumerate() {
            let Some(at) = start else { break };
            file.start = at;
            if file.len.is_none() && (index + 1 < count || self.done) {
                // A file that cannot be read contributes nothing; the durable
                // upload reports it.
                let (frames, len) = match Wav::open(&file.path) {
                    Ok(Some(wav)) => {
                        let frames = wav.frames().unwrap_or(0);
                        (frames, Resampler::output_len(wav.rate, RATE, frames))
                    }
                    _ => (0, 0),
                };
                file.frames = frames;
                file.len = Some(len);
            }
            start = file.len.map(|len| at + len);
        }
    }

    pub fn has_files(&self) -> bool {
        !self.files.is_empty()
    }

    /// The next sample that will be sent.
    pub fn cursor(&self) -> u64 {
        self.cursor
    }

    /// How many samples the channel holds so far (exact for finished files,
    /// to within a filter length for the one still growing).
    pub fn end(&mut self) -> u64 {
        let mut end = 0;
        for (index, file) in self.files.iter().enumerate() {
            let Some(len) = file.len else {
                if self.tail.as_ref().is_none_or(|(open, _)| *open != index) {
                    self.tail = Wav::open(&file.path)
                        .ok()
                        .flatten()
                        .map(|wav| (index, wav));
                }
                let live = self.tail.as_ref().and_then(|(_, wav)| {
                    let frames = wav.frames().ok()?;
                    Some(Resampler::output_len(wav.rate, RATE, frames))
                });
                return file.start + live.unwrap_or(0);
            };
            end = file.start + len;
        }
        self.tail = None;
        end
    }

    /// Everything has been read and handed out, and nothing more will come.
    pub fn exhausted(&self) -> bool {
        self.done
            && self.pending.is_empty()
            && self
                .files
                .last()
                .is_none_or(|last| last.len.is_some_and(|len| self.cursor >= last.start + len))
    }

    /// Move the cursor, dropping what was read ahead.
    pub fn seek(&mut self, sample: u64) {
        self.cursor = sample;
        self.pending.clear();
        self.reading = None;
    }

    /// Read ahead until one frame is pending or the files have no more.
    pub fn fill(&mut self) {
        while self.pending.len() < FRAME_MAX {
            let produced = self.cursor + self.pending.len() as u64;
            // The file `produced` falls in: the first that has not ended there.
            let Some(index) = self.files.iter().position(|file| {
                file.start <= produced && file.len.is_none_or(|len| produced < file.start + len)
            }) else {
                break;
            };
            // Files before `index` are final, so its start is known.
            if index > 0 && self.files[index - 1].len.is_none() {
                break;
            }
            if self.reading.as_ref().is_none_or(|reading| reading.file != index) {
                let Ok(Some(wav)) = Wav::open(&self.files[index].path) else {
                    break;
                };
                let mut resampler = Resampler::new(wav.rate, RATE);
                let frame = resampler.seek(produced - self.files[index].start);
                self.reading = Some(Reading {
                    file: index,
                    wav,
                    resampler,
                    frame,
                });
            }
            let file = &self.files[index];
            let reading = self.reading.as_mut().expect("reading was just set");
            let total = match file.len {
                Some(_) => file.frames,
                None => reading.wav.frames().unwrap_or(reading.frame),
            };
            // Enough input for the rest of one frame, and no more.
            let wanted = (FRAME_MAX - self.pending.len()) as u64 * reading.wav.rate as u64
                / RATE as u64
                + 1;
            let count = total.saturating_sub(reading.frame).min(wanted) as usize;
            let before = self.pending.len();
            if count > 0 {
                if reading
                    .wav
                    .read(reading.frame, count, &mut self.bytes, &mut self.input)
                    .is_err()
                {
                    break;
                }
                reading.resampler.push(&self.input, &mut self.pending);
                reading.frame += count as u64;
                continue;
            }
            let Some(len) = file.len else {
                break; // Still growing; nothing new yet.
            };
            // The file is over: its filter tail, then on to the next one.
            reading.resampler.finish(file.frames, &mut self.pending);
            let end = (file.start + len - self.cursor) as usize;
            self.pending.resize(end.max(before), 0);
            self.reading = None;
        }
    }

    /// Up to one frame of pending samples, if at least `min` are there.
    pub fn frame(&self, min: usize) -> Option<&[i16]> {
        (self.pending.len() >= min.max(1)).then(|| &self.pending[..self.pending.len().min(FRAME_MAX)])
    }

    /// `count` pending samples were sent.
    pub fn advance(&mut self, count: usize) {
        self.pending.drain(..count);
        self.cursor += count as u64;
    }
}

// ---------------------------------------------------------------- capture source

/// Where a session reads the capture's manifest from.
pub enum Source {
    /// The running capture's own state: what `manifest.json` is written
    /// from, but current to the last 80 ms poll rather than the last second.
    Monitor(crate::audio::CaptureMonitor),
    /// `manifest.json` itself, for a capture this process does not own.
    #[allow(dead_code)]
    File(PathBuf),
}

/// The parts of a capture manifest the stream follows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Capture {
    /// Files per channel, in manifest order.
    pub paths: [Vec<String>; 2],
    /// A requested channel has neither produced a file nor failed yet.
    pub starting: [bool; 2],
    pub done: bool,
}

impl Capture {
    pub fn of(manifest: &crate::audio::CaptureManifest) -> Self {
        let channel = |index: usize, stats: &crate::audio::ChannelStats| {
            let paths = if stats.paths.is_empty() {
                stats.path.clone().into_iter().collect()
            } else {
                stats.paths.clone()
            };
            let requested = manifest.stats.requested_channels & (1 << index) != 0;
            let starting = requested && paths.is_empty() && stats.state == "starting";
            (paths, starting)
        };
        let (mic, mic_starting) = channel(0, &manifest.stats.mic);
        let (system, system_starting) = channel(1, &manifest.stats.system);
        Self {
            paths: [mic, system],
            starting: [mic_starting, system_starting],
            done: manifest.completed(),
        }
    }
}

impl Source {
    /// The capture's manifest as it is now.
    pub fn manifest(&self) -> Option<crate::audio::CaptureManifest> {
        match self {
            Source::Monitor(monitor) => Some(monitor.snapshot()),
            Source::File(path) => std::fs::read(path)
                .ok()
                .and_then(|data| serde_json::from_slice(&data).ok()),
        }
    }

    fn read(&self) -> Option<Capture> {
        self.manifest().map(|manifest| Capture::of(&manifest))
    }
}

// ---------------------------------------------------------------- session

/// Where a capture's stream connects.
#[derive(Clone, Debug)]
pub struct Target {
    /// `ws://` or `wss://` URL of the note's live endpoint.
    pub url: String,
    pub token: String,
    pub capture_id: String,
    /// Note-timeline position of the capture's first sample.
    pub base_ms: u64,
}

/// What looking up the target came to.
pub enum Resolved {
    Ready(Target),
    /// Not yet (the server has not answered, an earlier capture is still
    /// being finalised): ask again after the back-off.
    Later,
    /// This recording will not be streamed.
    Never,
}

/// What the window shows of the stream. `Off` is the silent default: no
/// server, live transcription not configured, or the stream gave up, and the
/// recording is transcribed from its uploaded parts as before.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Off,
    Connecting,
    Live,
    CatchingUp,
    ProviderUnavailable,
    Reconnecting,
    Paused,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Interim {
    pub channel: Channel,
    pub utterance_id: String,
    pub rev: u64,
    pub text: String,
    pub start_ms: u64,
    pub speaker_id: Option<String>,
    pub speaker_label: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// The phase, and for `CatchingUp` how many seconds behind the transcript is.
    Status(Phase, u64),
    /// The server's transcript revision when the socket became ready. If the
    /// note here is at another revision it missed text and should be re-read.
    Revision(u64),
    Interim(Interim),
    Final {
        utterance_id: String,
        revision: u64,
        segment: TranscriptSegmentDTO,
    },
    Speakers(Vec<SpeakerMerge>),
    /// Audio the stream will not transcribe; the uploaded part fills it.
    Gap {
        channel: Channel,
        from_ms: u64,
        to_ms: u64,
    },
    /// The session thread is finished. Always the last event.
    Ended,
}

enum Command {
    Pause,
    Resume,
    End,
    Abort,
    /// Cut the connection as a network failure would (see [`Session::sever`]).
    Sever,
}

/// Whether a stream has handed over to the durable upload: it has sent the
/// last of its capture and `mark: end`, or it has nothing to send.
#[derive(Default)]
struct Handover {
    done: Mutex<bool>,
    changed: Condvar,
    /// The upload has waited as long as it will: the stream is to stop where
    /// it is and say how long the capture really was (see [`wait_for_handover`]).
    cut: std::sync::atomic::AtomicBool,
}

impl Handover {
    fn finish(&self) {
        *self.done.lock().unwrap_or_else(|poison| poison.into_inner()) = true;
        self.changed.notify_all();
    }
    fn cut(&self) -> bool {
        self.cut.load(std::sync::atomic::Ordering::Acquire)
    }
}

/// The streams that are running, by the key they were started with.
static HANDOVERS: Mutex<Vec<(u64, Arc<Handover>)>> = Mutex::new(Vec::new());

/// Called by the upload worker before it tells the server that the recording
/// of `key` has stopped: wait, briefly, until that recording's stream has sent
/// the end of its capture. The server ends the live session when it is told
/// to stop, so without this the last fraction of a second would reach it only
/// in the durable part, and that part would be transcribed a second time.
/// Returns at once when no stream is running; never waits much longer than
/// `limit`. A stream that is still behind when the limit runs out is told to
/// stop there: it sends `mark: end` with the length of the capture, which is
/// more than it has sent, so the server knows the stream ended part-way and
/// leaves the unfinished words to the durable part instead of storing a
/// clipped one. It is given a moment more to get that mark out.
pub fn wait_for_handover(key: u64, limit: Duration) {
    let handover = HANDOVERS
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .iter()
        .rev()
        .find(|(known, _)| *known == key)
        .map(|(_, handover)| handover.clone());
    let Some(handover) = handover else { return };
    let done = handover.done.lock().unwrap_or_else(|poison| poison.into_inner());
    let (done, waited) = handover
        .changed
        .wait_timeout_while(done, limit, |done| !*done)
        .unwrap_or_else(|poison| poison.into_inner());
    if waited.timed_out() {
        handover.cut.store(true, std::sync::atomic::Ordering::Release);
        let _ = handover
            .changed
            .wait_timeout_while(done, Duration::from_millis(500), |done| !*done);
    }
}

/// A running stream. Dropping it abandons the stream at once; [`Session::end`]
/// lets it send the rest of the capture and collect the last finals first.
pub struct Session {
    commands: mpsc::Sender<Command>,
    pub events: mpsc::Receiver<Event>,
}

impl Session {
    /// Start following `source`. `key` names the recording for
    /// [`wait_for_handover`]. `resolve` is called on the session's thread
    /// until it yields a target; `alive` is polled and ends the session when
    /// it turns false (the account changed).
    pub fn start(
        key: u64,
        source: Source,
        resolve: impl FnMut() -> Resolved + Send + 'static,
        alive: impl Fn() -> bool + Send + 'static,
    ) -> Self {
        let (commands, inbox) = mpsc::channel();
        let (outbox, events) = mpsc::sync_channel(2048);
        let handover = Arc::new(Handover::default());
        {
            let mut handovers = HANDOVERS.lock().unwrap_or_else(|poison| poison.into_inner());
            handovers.retain(|(known, _)| *known != key);
            handovers.push((key, handover.clone()));
        }
        let spawned = thread::Builder::new()
            .name("live-transcript".into())
            .spawn({
                let (outbox, handover) = (outbox.clone(), handover.clone());
                move || {
                    let mut worker = Worker::new(
                        source,
                        Box::new(resolve),
                        Box::new(alive),
                        inbox,
                        outbox.clone(),
                        handover.clone(),
                    );
                    worker.run();
                    drop(worker);
                    HANDOVERS
                        .lock()
                        .unwrap_or_else(|poison| poison.into_inner())
                        .retain(|(_, known)| !Arc::ptr_eq(known, &handover));
                    let _ = outbox.send(Event::Ended);
                }
            });
        if spawned.is_err() {
            handover.finish();
            let _ = outbox.send(Event::Ended);
        }
        Self { commands, events }
    }
    /// The recording was put on hold: send what is captured so far, then
    /// `mark: pause`, and send nothing more until [`Session::resume`].
    pub fn pause(&self) {
        let _ = self.commands.send(Command::Pause);
    }
    pub fn resume(&self) {
        let _ = self.commands.send(Command::Resume);
    }
    /// The recording stopped: send the rest of the capture once it is
    /// finalised, then `mark: end`, and wait briefly for the last finals.
    pub fn end(&self) {
        let _ = self.commands.send(Command::End);
    }
    /// For the live probe only: shut the TCP socket under the stream without
    /// a closing handshake, which is what a lost network looks like from
    /// both ends. The session then reconnects as it would after a real drop.
    pub fn sever(&self) {
        let _ = self.commands.send(Command::Sever);
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Abort);
    }
}

type Socket = WebSocket<MaybeTlsStream<TcpStream>>;

struct Connection {
    socket: Socket,
    /// The same TCP socket, for changing its timeouts.
    control: TcpStream,
    /// `ready` has arrived.
    ready: bool,
    /// Since when a write has been waiting for the socket to take it.
    blocked: Option<Instant>,
    /// When the server was last heard from.
    heard: Instant,
    declared: [bool; 2],
}

/// Why a connection attempt or an open connection ended.
#[derive(Debug, PartialEq)]
enum Outcome {
    /// Try again after the back-off.
    Retry,
    /// The note is not recording on the server (yet, or any more).
    NotRecording,
    /// The stream is finished for this recording; `true` when the reason is
    /// the speech provider, which the window says quietly.
    Stop(bool),
    /// The server closed normally after `mark: end`.
    Finished,
}

fn tls() -> Result<Arc<rustls::ClientConfig>, String> {
    static CONFIG: OnceLock<Result<Arc<rustls::ClientConfig>, String>> = OnceLock::new();
    CONFIG
        .get_or_init(|| {
            let mut roots = rustls::RootCertStore::empty();
            let loaded = rustls_native_certs::load_native_certs();
            roots.add_parsable_certificates(loaded.certs);
            if roots.is_empty() {
                return Err("No trusted root certificates are available".into());
            }
            let provider = Arc::new(rustls::crypto::ring::default_provider());
            rustls::ClientConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .map_err(|error| error.to_string())
                .map(|builder| {
                    Arc::new(
                        builder
                            .with_root_certificates(roots)
                            .with_no_client_auth(),
                    )
                })
        })
        .clone()
}

/// How an HTTP refusal of the upgrade is handled (protocol notes, point 1).
fn refusal(status: u16) -> Outcome {
    match status {
        // Not configured, or the instance is at its session limit: stay on
        // durable parts, silently.
        503 | 429 => Outcome::Stop(false),
        401 | 403 => Outcome::Stop(false),
        // The note is not in the recording state, or not there (yet).
        409 | 404 => Outcome::NotRecording,
        _ => Outcome::Retry,
    }
}

/// How a close code is handled (protocol notes, point 1).
fn closed(code: u16) -> Outcome {
    match code {
        1000 => Outcome::Finished,
        4503 => Outcome::Stop(true),
        4400 | 4401 | 4404 | 4409 | 4429 => Outcome::Stop(false),
        // A network drop, 4408, 1001, 1006, 1011, 1012 and anything unknown.
        _ => Outcome::Retry,
    }
}

fn would_block(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut | io::ErrorKind::Interrupted
    )
}

fn connect(target: &Target) -> Result<(Socket, TcpStream), Outcome> {
    use tungstenite::{
        client::IntoClientRequest,
        handshake::HandshakeError,
        http::{HeaderValue, header::AUTHORIZATION},
        protocol::WebSocketConfig,
    };
    let mut request = target
        .url
        .as_str()
        .into_client_request()
        .map_err(|_| Outcome::Stop(false))?;
    let mut bearer = HeaderValue::from_str(&format!("Bearer {}", target.token))
        .map_err(|_| Outcome::Stop(false))?;
    bearer.set_sensitive(true);
    request.headers_mut().insert(AUTHORIZATION, bearer);
    let secure = request.uri().scheme_str() == Some("wss");
    let host = request.uri().host().ok_or(Outcome::Stop(false))?.to_owned();
    let port = request
        .uri()
        .port_u16()
        .unwrap_or(if secure { 443 } else { 80 });
    let host = host.trim_start_matches('[').trim_end_matches(']').to_owned();
    let stream = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|_| Outcome::Retry)?
        .find_map(|address| TcpStream::connect_timeout(&address, Duration::from_secs(5)).ok())
        .ok_or(Outcome::Retry)?;
    let _ = stream.set_nodelay(true);
    // Generous while the handshake runs; shortened once the socket is open.
    let handshake = Some(Duration::from_secs(10));
    stream
        .set_read_timeout(handshake)
        .and_then(|()| stream.set_write_timeout(handshake))
        .map_err(|_| Outcome::Retry)?;
    let control = stream.try_clone().map_err(|_| Outcome::Retry)?;
    let connector = if secure {
        tungstenite::Connector::Rustls(tls().map_err(|_| Outcome::Stop(false))?)
    } else {
        tungstenite::Connector::Plain
    };
    let config = WebSocketConfig::default()
        .max_message_size(Some(1 << 20))
        .max_frame_size(Some(1 << 20));
    match tungstenite::client_tls_with_config(request, stream, Some(config), Some(connector)) {
        Ok((socket, _)) => {
            control
                .set_read_timeout(Some(Duration::from_millis(15)))
                .and_then(|()| control.set_write_timeout(Some(Duration::from_millis(250))))
                .map_err(|_| Outcome::Retry)?;
            Ok((socket, control))
        }
        Err(HandshakeError::Failure(tungstenite::Error::Http(response))) => {
            Err(refusal(response.status().as_u16()))
        }
        Err(_) => Err(Outcome::Retry),
    }
}

struct Worker {
    source: Source,
    resolve: Box<dyn FnMut() -> Resolved + Send>,
    alive: Box<dyn Fn() -> bool + Send>,
    commands: mpsc::Receiver<Command>,
    events: mpsc::SyncSender<Event>,
    handover: Arc<Handover>,
    /// A channel was added and the socket closed to declare it: the server
    /// may still be letting go of the old socket when the new one arrives.
    redeclaring: u32,
    target: Option<Target>,
    capture: Capture,
    tracks: [Track; 2],
    /// The newest `ack.throughSample` per channel: where a reconnect resumes.
    acked: [u64; 2],
    connection: Option<Connection>,
    /// Samples each channel may still send now (see [`CATCH_UP`]).
    budget: [f64; 2],
    budgeted: Instant,
    resume_window_ms: u64,
    attempts: usize,
    next_attempt: Instant,
    /// Refusals because the note was not recording; a few are expected while
    /// the server is still handling Start.
    not_recording: u32,
    was_ready: bool,
    started: Instant,
    paused: bool,
    pausing: bool,
    /// Set by `end`: when to give up waiting.
    ending: Option<Instant>,
    end_sent: Option<Instant>,
    states: [StreamState; 2],
    behind_since: Option<Instant>,
    reported: (Phase, u64),
    provider_gone: bool,
    checked: Instant,
    /// When the manifest was last read.
    synced: Instant,
}

/// What one pass of the session loop decided.
enum Step {
    Continue,
    Exit,
}

impl Worker {
    fn new(
        source: Source,
        resolve: Box<dyn FnMut() -> Resolved + Send>,
        alive: Box<dyn Fn() -> bool + Send>,
        commands: mpsc::Receiver<Command>,
        events: mpsc::SyncSender<Event>,
        handover: Arc<Handover>,
    ) -> Self {
        let now = Instant::now();
        Self {
            source,
            resolve,
            alive,
            commands,
            events,
            handover,
            redeclaring: 0,
            target: None,
            capture: Capture::default(),
            tracks: [Track::new(), Track::new()],
            acked: [0; 2],
            connection: None,
            budget: [0.; 2],
            budgeted: now,
            resume_window_ms: 30_000,
            attempts: 0,
            next_attempt: now,
            not_recording: 0,
            was_ready: false,
            started: now,
            paused: false,
            pausing: false,
            ending: None,
            end_sent: None,
            states: [StreamState::Live; 2],
            behind_since: None,
            reported: (Phase::Off, 0),
            provider_gone: false,
            checked: now,
            synced: now - Duration::from_secs(1),
        }
    }

    fn emit(&self, event: Event) -> bool {
        match event {
            // Interim text is replaceable: never wait for the window.
            Event::Interim(_) => !matches!(
                self.events.try_send(event),
                Err(mpsc::TrySendError::Disconnected(_))
            ),
            event => self.events.send(event).is_ok(),
        }
    }

    fn run(&mut self) {
        loop {
            let now = Instant::now();
            loop {
                match self.commands.try_recv() {
                    Ok(Command::Pause) => self.pausing = !self.paused,
                    Ok(Command::Resume) => self.unpause(),
                    Ok(Command::End) => {
                        self.ending.get_or_insert(now + Duration::from_secs(15));
                        // With no socket up there is nothing to hand over in
                        // the time the upload is prepared to wait.
                        if !self.connection.as_ref().is_some_and(|c| c.ready) || self.paused {
                            self.handover.finish();
                        }
                    }
                    Ok(Command::Sever) => {
                        if let Some(connection) = &self.connection {
                            let _ = connection.control.shutdown(std::net::Shutdown::Both);
                        }
                    }
                    Ok(Command::Abort) | Err(mpsc::TryRecvError::Disconnected) => return,
                    Err(mpsc::TryRecvError::Empty) => break,
                }
            }
            if now.duration_since(self.checked) >= Duration::from_millis(250) {
                self.checked = now;
                if !(self.alive)() {
                    return;
                }
            }
            if now.duration_since(self.synced) >= Duration::from_millis(50)
                && let Some(capture) = self.source.read()
            {
                self.synced = now;
                for channel in Channel::ALL {
                    self.tracks[channel.index()]
                        .sync(&capture.paths[channel.index()], capture.done);
                }
                self.capture = capture;
            }
            if self.ending.is_some_and(|deadline| now >= deadline) {
                // Out of time with the capture not all sent: the same last
                // word as above, if there is a socket to say it on.
                if self.end_sent.is_none()
                    && self.capture.done
                    && let Some(connection) = self.connection.as_mut().filter(|connection| connection.ready)
                {
                    let mark = Self::end_mark(&mut self.tracks, connection.declared);
                    let _ = Self::send(connection, mark);
                }
                return;
            }
            let step = if self.connection.is_some() {
                self.serve(now)
            } else {
                self.dial(now)
            };
            if matches!(step, Step::Exit) {
                return;
            }
            if !self.report(now) {
                return;
            }
        }
    }

    fn wanted(&self) -> [bool; 2] {
        [self.tracks[0].has_files(), self.tracks[1].has_files()]
    }

    /// Not connected: wait for the capture and the back-off, then connect.
    fn dial(&mut self, now: Instant) -> Step {
        let wait = |until: Instant| {
            thread::sleep(
                until
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_millis(20)),
            );
            Step::Continue
        };
        if now < self.next_attempt {
            return wait(self.next_attempt);
        }
        let wanted = self.wanted();
        // Give a channel that is still starting a moment to produce its file,
        // so the first hello usually declares both.
        let settling = self.capture.starting.contains(&true)
            && now.duration_since(self.started) < Duration::from_millis(2500);
        if wanted == [false, false] || (settling && !self.capture.done) {
            if self.capture.done && wanted == [false, false] {
                return Step::Exit; // Nothing was captured.
            }
            return wait(now + Duration::from_millis(20));
        }
        if self.target.is_none() {
            match (self.resolve)() {
                Resolved::Ready(target) => self.target = Some(target),
                Resolved::Later => return self.back_off(),
                Resolved::Never => return Step::Exit,
            }
        }
        let target = self.target.clone().expect("target was just resolved");
        match connect(&target) {
            Ok((socket, control)) => {
                let mut connection = Connection {
                    socket,
                    control,
                    ready: false,
                    blocked: None,
                    heard: Instant::now(),
                    declared: wanted,
                };
                let streams: Vec<_> = Channel::ALL
                    .into_iter()
                    .filter(|channel| wanted[channel.index()])
                    .map(|channel| {
                        serde_json::json!({
                            "channel": channel.name(),
                            "sampleRate": RATE,
                            "encoding": "pcm_s16le",
                            "baseMs": target.base_ms,
                            "resumeFromSample": self.acked[channel.index()],
                        })
                    })
                    .collect();
                let hello = serde_json::json!({
                    "type": "hello",
                    "protocol": PROTOCOL,
                    "captureId": target.capture_id,
                    "streams": streams,
                });
                if Self::send(&mut connection, Message::text(hello.to_string())).is_err() {
                    return self.back_off();
                }
                self.connection = Some(connection);
                Step::Continue
            }
            Err(outcome) => self.settle(outcome),
        }
    }

    fn back_off(&mut self) -> Step {
        let delay = BACK_OFF[self.attempts.min(BACK_OFF.len() - 1)];
        self.attempts += 1;
        self.next_attempt = Instant::now() + delay;
        Step::Continue
    }

    /// Act on how a connection or an attempt ended.
    fn settle(&mut self, outcome: Outcome) -> Step {
        self.connection = None;
        if self.ending.is_some() {
            self.handover.finish();
        }
        match outcome {
            Outcome::Finished => Step::Exit,
            Outcome::Stop(provider) => {
                self.provider_gone = provider;
                Step::Exit
            }
            // Once the recording is ending nothing is gained by reconnecting
            // to a note the server has already stopped.
            Outcome::NotRecording if self.ending.is_some() || self.not_recording >= 8 => Step::Exit,
            Outcome::NotRecording => {
                self.not_recording += 1;
                self.back_off()
            }
            Outcome::Retry if self.end_sent.is_some() => Step::Exit,
            Outcome::Retry => self.back_off(),
        }
    }

    /// Queue a message. `Ok(false)` means the socket could not take all of it
    /// yet; it is kept and goes out with a later flush.
    fn send(connection: &mut Connection, message: Message) -> Result<bool, ()> {
        match connection.socket.send(message) {
            Ok(()) => Ok(true),
            Err(tungstenite::Error::Io(error)) if would_block(&error) => {
                connection.blocked.get_or_insert_with(Instant::now);
                Ok(false)
            }
            Err(_) => Err(()),
        }
    }

    fn unpause(&mut self) {
        self.pausing = false;
        if !std::mem::take(&mut self.paused) {
            return;
        }
        if let Some(connection) = self.connection.as_mut().filter(|connection| connection.ready) {
            let mark = Self::mark("resume", &self.tracks, connection.declared);
            let _ = Self::send(connection, mark);
        }
    }

    /// A mark at the sample each streaming channel has been sent up to.
    /// (A pause or a resume is marked once everything captured has gone out,
    /// so that is also how much the capture holds.)
    fn mark(kind: &str, tracks: &[Track; 2], declared: [bool; 2]) -> Message {
        let mut at = serde_json::Map::new();
        for channel in Channel::ALL {
            if declared[channel.index()] {
                at.insert(
                    channel.name().into(),
                    tracks[channel.index()].cursor().into(),
                );
            }
        }
        Message::text(serde_json::json!({"type": "mark", "kind": kind, "atSample": at}).to_string())
    }

    /// The end of the recording: how many samples the capture holds for each
    /// streaming channel, whatever part of them has been sent. When that is
    /// more than was sent (the stream was behind and has been told to stop,
    /// or its time ran out) the server knows the stream ended part-way.
    fn end_mark(tracks: &mut [Track; 2], declared: [bool; 2]) -> Message {
        let mut at = serde_json::Map::new();
        for channel in Channel::ALL {
            if declared[channel.index()] {
                let track = &mut tracks[channel.index()];
                at.insert(channel.name().into(), track.end().max(track.cursor()).into());
            }
        }
        Message::text(serde_json::json!({"type": "mark", "kind": "end", "atSample": at}).to_string())
    }

    /// Connected: read what the server sent, then send what is due.
    fn serve(&mut self, now: Instant) -> Step {
        // A channel produced its first file after the hello (system audio
        // that started late): declare it with a fresh hello.
        let declared = self.connection.as_ref().map(|c| c.declared);
        if declared.is_some_and(|declared| declared != self.wanted()) {
            if let Some(mut connection) = self.connection.take() {
                let _ = connection.socket.close(None);
                let _ = connection.socket.flush();
            }
            self.redeclaring = 1;
            self.next_attempt = now;
            return Step::Continue;
        }
        // Reading waits up to the socket's 15 ms timeout: that is the pace of
        // this loop while there is nothing to do.
        for _ in 0..64 {
            let Some(connection) = self.connection.as_mut() else {
                break;
            };
            match connection.socket.read() {
                Ok(Message::Text(text)) => {
                    connection.heard = Instant::now();
                    let Some(message) = decode(text.as_str()) else {
                        continue;
                    };
                    if !self.receive(message) {
                        return Step::Exit;
                    }
                }
                Ok(Message::Close(frame)) => {
                    let code = frame.map_or(1005, |frame| u16::from(frame.code));
                    let _ = connection.socket.flush(); // our half of the close
                    let outcome = if self.end_sent.is_some() {
                        Outcome::Finished
                    } else if code == 4409 && !connection.ready && (1..=3).contains(&self.redeclaring) {
                        // "Another publisher" is our own previous socket,
                        // which the server has not finished closing.
                        self.redeclaring += 1;
                        Outcome::Retry
                    } else {
                        closed(code)
                    };
                    return self.settle(outcome);
                }
                Ok(_) => connection.heard = Instant::now(),
                Err(tungstenite::Error::Io(error)) if would_block(&error) => break,
                Err(_) => {
                    let outcome = if self.end_sent.is_some() {
                        Outcome::Finished
                    } else {
                        Outcome::Retry
                    };
                    return self.settle(outcome);
                }
            }
        }
        let Some(connection) = self.connection.as_mut() else {
            return Step::Continue;
        };
        // Pongs and anything a slow socket left behind.
        match connection.socket.flush() {
            Ok(()) => connection.blocked = None,
            Err(tungstenite::Error::Io(error)) if would_block(&error) => {
                connection.blocked.get_or_insert(now);
            }
            Err(_) => return self.settle(Outcome::Retry),
        }
        let stalled = connection
            .blocked
            .is_some_and(|since| now.duration_since(since) > Duration::from_secs(10));
        // Audio is going out and nothing has come back: the socket is dead
        // even though the system has not said so.
        let silent = connection.ready
            && !self.paused
            && self.end_sent.is_none()
            && now.duration_since(connection.heard) > Duration::from_secs(20);
        if stalled || silent || (!connection.ready && now.duration_since(connection.heard) > Duration::from_secs(10)) {
            return self.settle(Outcome::Retry);
        }
        // The upload will wait no longer for a stream that is behind: say how
        // long the capture is and send no more of it. (Once the capture is
        // final, so that the length is.)
        if self.end_sent.is_none()
            && self.ending.is_some()
            && self.handover.cut()
            && self.capture.done
            && connection.ready
            && !self.paused
        {
            let mark = Self::end_mark(&mut self.tracks, connection.declared);
            let sent = Self::send(connection, mark);
            self.handover.finish();
            if sent.is_err() {
                return self.settle(Outcome::Retry);
            }
            self.end_sent = Some(now);
            return Step::Continue;
        }
        if let Some(sent) = self.end_sent {
            // Waiting for the last finals and the server's close.
            return if now.duration_since(sent) > Duration::from_secs(4) {
                Step::Exit
            } else {
                Step::Continue
            };
        }
        if !connection.ready || connection.blocked.is_some() {
            return Step::Continue;
        }
        if self.paused {
            return Step::Continue;
        }
        self.pump(now)
    }

    /// Send the audio that is due on each channel.
    fn pump(&mut self, now: Instant) -> Step {
        let elapsed = now.duration_since(self.budgeted).as_secs_f64();
        self.budgeted = now;
        // Everything captured is to go out, short frames included: the
        // capture has ended, or a pause is waiting for the last samples.
        let flushing = self.pausing || (self.ending.is_some() && self.capture.done);
        let mut drained = true;
        for channel in Channel::ALL {
            let index = channel.index();
            let Some(connection) = self.connection.as_mut() else {
                return Step::Continue;
            };
            if !connection.declared[index] {
                continue;
            }
            self.budget[index] =
                (self.budget[index] + elapsed * CATCH_UP * RATE as f64).min(RATE as f64 / 2.);
            let track = &mut self.tracks[index];
            // More than the resume window behind: skip to near the end. The
            // server sees the index jump and declares the gap; the durable
            // part covers it.
            let end = track.end();
            let behind_ms = end.saturating_sub(track.cursor()) * 1000 / RATE as u64;
            if behind_ms > self.resume_window_ms {
                // It resumes well inside the window, and only ever forwards.
                // Resuming further back than the window allows would leave
                // the stream "behind" for good: it would skip again on every
                // pass, pinned that far behind the capture and dropping a
                // few milliseconds whenever this thread was kept waiting.
                let back = SKIP_TO_MS.min(self.resume_window_ms / 2);
                let to = end.saturating_sub(back * RATE as u64 / 1000);
                if to > track.cursor() {
                    track.seek(to);
                }
            }
            loop {
                track.fill();
                let Some(samples) = track.frame(if flushing { 1 } else { FRAME_MIN }) else {
                    break;
                };
                if self.budget[index] < samples.len() as f64 {
                    drained = false;
                    break;
                }
                let count = samples.len();
                let frame = encode_frame(channel, track.cursor(), samples);
                let sent = Self::send(connection, Message::binary(frame));
                if sent.is_err() {
                    return self.settle(Outcome::Retry);
                }
                track.advance(count);
                self.budget[index] -= count as f64;
                if sent == Ok(false) {
                    return Step::Continue; // The socket is full; flush first.
                }
            }
            drained &= track.frame(1).is_none();
        }
        let Some(connection) = self.connection.as_mut() else {
            return Step::Continue;
        };
        if self.pausing && drained {
            self.pausing = false;
            self.paused = true;
            let mark = Self::mark("pause", &self.tracks, connection.declared);
            if Self::send(connection, mark).is_err() {
                return self.settle(Outcome::Retry);
            }
        } else if self.ending.is_some()
            && self.capture.done
            && Channel::ALL
                .into_iter()
                .all(|channel| !connection.declared[channel.index()] || self.tracks[channel.index()].exhausted())
        {
            let mark = Self::end_mark(&mut self.tracks, connection.declared);
            let sent = Self::send(connection, mark);
            self.handover.finish();
            if sent.is_err() {
                return self.settle(Outcome::Retry);
            }
            self.end_sent = Some(now);
        }
        Step::Continue
    }

    /// Handle one server message. Returns false when the window is gone.
    fn receive(&mut self, message: ServerMessage) -> bool {
        match message {
            ServerMessage::Ready {
                streams,
                resume_window_ms,
                transcript_revision,
            } => {
                self.resume_window_ms = resume_window_ms;
                self.attempts = 0;
                self.not_recording = 0;
                self.redeclaring = 0;
                self.was_ready = true;
                self.states = [StreamState::Live; 2];
                for stream in streams {
                    let index = stream.channel.index();
                    self.acked[index] = stream.accept_from_sample;
                    self.tracks[index].seek(stream.accept_from_sample);
                }
                self.budget = [0.; 2];
                self.budgeted = Instant::now();
                if let Some(connection) = self.connection.as_mut() {
                    connection.ready = true;
                    if self.paused {
                        let mark = Self::mark("pause", &self.tracks, connection.declared);
                        let _ = Self::send(connection, mark);
                    }
                }
                self.emit(Event::Revision(transcript_revision))
            }
            ServerMessage::Ack {
                channel,
                through_sample,
            } => {
                let acked = &mut self.acked[channel.index()];
                *acked = (*acked).max(through_sample);
                true
            }
            ServerMessage::Interim {
                channel,
                utterance_id,
                rev,
                text,
                start_ms,
                speaker_id,
                speaker_label,
            } => self.emit(Event::Interim(Interim {
                channel,
                utterance_id,
                rev,
                text,
                start_ms,
                speaker_id,
                speaker_label,
            })),
            ServerMessage::Final {
                utterance_id,
                transcript_revision,
                segment,
            } => self.emit(Event::Final {
                utterance_id,
                revision: transcript_revision,
                segment,
            }),
            ServerMessage::Speakers { merges } => self.emit(Event::Speakers(merges)),
            ServerMessage::Gap {
                channel,
                from_ms,
                to_ms,
            } => self.emit(Event::Gap {
                channel,
                from_ms,
                to_ms,
            }),
            ServerMessage::Status { channel, state } => {
                self.states[channel.index()] = state;
                true
            }
            ServerMessage::Error { code, retryable } => {
                // The close that follows decides what happens; this only
                // records that the provider is the reason.
                if code == "provider_unavailable" && !retryable {
                    self.provider_gone = true;
                }
                true
            }
            ServerMessage::Unknown => true,
        }
    }

    /// Tell the window when what it shows of the stream has changed.
    fn report(&mut self, now: Instant) -> bool {
        let declared = self
            .connection
            .as_ref()
            .filter(|connection| connection.ready)
            .map(|connection| connection.declared);
        let (phase, behind) = match declared {
            None if self.was_ready => (Phase::Reconnecting, 0),
            None => (Phase::Connecting, 0),
            Some(_) if self.paused => (Phase::Paused, 0),
            Some(declared) => {
                let channels = || Channel::ALL.into_iter().filter(|c| declared[c.index()]);
                let mut unsent = 0;
                for channel in channels() {
                    let track = &mut self.tracks[channel.index()];
                    let samples = track.end().saturating_sub(track.cursor());
                    unsent = unsent.max(samples * 1000 / RATE as u64);
                }
                // The server says so while it is being sent a backlog. How far
                // behind is counted here, in captured audio not yet sent: the
                // server's own figure also counts the sentence being spoken.
                let replaying =
                    channels().any(|c| self.states[c.index()] == StreamState::CatchingUp);
                let behind_ms = unsent;
                // A second or two of catching up at the start of a recording
                // or after a reconnect is not worth a word.
                let behind = (unsent > 3_000 || replaying)
                    && *self.behind_since.get_or_insert(now) + Duration::from_secs(2) <= now;
                if !(unsent > 3_000 || replaying) {
                    self.behind_since = None;
                }
                if channels().any(|c| self.states[c.index()] == StreamState::ProviderUnavailable) {
                    (Phase::ProviderUnavailable, 0)
                } else if behind {
                    // Whole seconds below ten, then steps of five: the number
                    // on screen should not tick.
                    let seconds = (behind_ms + 500) / 1000;
                    let shown = if seconds > 10 {
                        seconds.div_ceil(5) * 5
                    } else {
                        seconds
                    };
                    (Phase::CatchingUp, shown)
                } else {
                    (Phase::Live, 0)
                }
            }
        };
        if (phase, behind) == self.reported {
            return true;
        }
        self.reported = (phase, behind);
        self.emit(Event::Status(phase, behind))
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.handover.finish();
        if let Some(mut connection) = self.connection.take() {
            let _ = connection.control.set_write_timeout(Some(Duration::from_millis(200)));
            let _ = connection.socket.close(None);
            let _ = connection.socket.flush();
        }
        // The last word on the stream: off, or the provider being the reason
        // it stopped.
        let last = if self.provider_gone {
            Phase::ProviderUnavailable
        } else {
            Phase::Off
        };
        if self.reported != (last, 0) {
            let _ = self.events.send(Event::Status(last, 0));
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::{
        io::Write,
        net::TcpListener,
        os::unix::fs::FileExt,
        sync::atomic::{AtomicUsize, Ordering},
    };

    // ------------------------------------------------------------ wire format

    #[test]
    fn audio_frame_header_is_version_channel_reserved_then_little_endian_index() {
        let frame = encode_frame(Channel::System, 0x0102_0304_0506_0708, &[1, -2, i16::MIN]);
        assert_eq!(
            frame,
            [
                1, 1, 0, 0, // protocol, channel, reserved
                8, 7, 6, 5, 4, 3, 2, 1, // first sample, u64 little-endian
                1, 0, 0xfe, 0xff, 0x00, 0x80, // PCM16 little-endian
            ]
        );
        assert_eq!(encode_frame(Channel::Mic, 0, &[])[..2], [1, 0]);
        assert_eq!(encode_frame(Channel::Mic, 7, &[0; 1600]).len(), FRAME_HEADER + 3200);
        // 100 to 250 ms at 16 kHz, comfortably under the server's 64 KiB limit.
        assert_eq!((FRAME_MIN, FRAME_MAX), (1_600, 4_000));
        assert!(FRAME_HEADER + FRAME_MAX * 2 < 64 * 1024);
    }

    #[test]
    fn server_messages_decode_as_the_contract_spells_them() {
        assert_eq!(
            decode(r#"{"type":"ready","sessionId":"0b1c2d3e-0000-4000-8000-000000000001","provider":"elevenlabs","model":"scribe_v3_live","streams":[{"channel":"mic","acceptFromSample":0},{"channel":"system","acceptFromSample":48000}],"resumeWindowMs":30000,"transcriptRevision":41}"#),
            Some(ServerMessage::Ready {
                streams: vec![
                    ReadyStream { channel: Channel::Mic, accept_from_sample: 0 },
                    ReadyStream { channel: Channel::System, accept_from_sample: 48_000 },
                ],
                resume_window_ms: 30_000,
                transcript_revision: 41,
            })
        );
        assert_eq!(
            decode(r#"{"type":"ack","channel":"mic","throughSample":1280000,"transcribedThroughMs":832400}"#),
            Some(ServerMessage::Ack { channel: Channel::Mic, through_sample: 1_280_000 })
        );
        assert_eq!(
            decode(r#"{"type":"interim","channel":"system","utteranceId":"u-1","rev":7,"text":"so if we move the pilot to","startMs":833100,"endMs":835000,"speakerId":"system:n:3","speakerLabel":"Speaker 3"}"#),
            Some(ServerMessage::Interim {
                channel: Channel::System,
                utterance_id: "u-1".into(),
                rev: 7,
                text: "so if we move the pilot to".into(),
                start_ms: 833_100,
                speaker_id: Some("system:n:3".into()),
                speaker_label: Some("Speaker 3".into()),
            })
        );
        // The speaker fields are optional.
        assert!(matches!(
            decode(r#"{"type":"interim","channel":"mic","utteranceId":"u","rev":0,"text":"","startMs":0,"endMs":0}"#),
            Some(ServerMessage::Interim { speaker_id: None, speaker_label: None, .. })
        ));
        let Some(ServerMessage::Final { utterance_id, transcript_revision, segment }) = decode(
            r#"{"type":"final","utteranceId":"u-1","transcriptRevision":42,"segment":{"id":"419d65d1-7a2c-44ca-84cc-ad489522d562","channel":"system","speakerId":"system:n:3","speakerLabel":"Speaker 3","text":"So if we move the pilot to May.","isFinal":true,"startMs":833100,"endMs":836200}}"#,
        ) else {
            panic!("a final decodes");
        };
        assert_eq!((utterance_id.as_str(), transcript_revision), ("u-1", 42));
        assert_eq!(segment.text, "So if we move the pilot to May.");
        assert_eq!((segment.start_ms, segment.end_ms, segment.is_final), (833_100, Some(836_200), true));
        assert_eq!(
            decode(r#"{"type":"speakers","merges":[{"from":"system:n:4","to":"system:n:2"}]}"#),
            Some(ServerMessage::Speakers {
                merges: vec![SpeakerMerge { from: "system:n:4".into(), to: "system:n:2".into() }]
            })
        );
        assert_eq!(
            decode(r#"{"type":"gap","channel":"mic","fromMs":840000,"toMs":871000,"reason":"resume_window_exceeded"}"#),
            Some(ServerMessage::Gap { channel: Channel::Mic, from_ms: 840_000, to_ms: 871_000 })
        );
        // A reason added later is still a gap.
        assert!(matches!(
            decode(r#"{"type":"gap","channel":"mic","fromMs":1,"toMs":2,"reason":"something_new"}"#),
            Some(ServerMessage::Gap { .. })
        ));
        for (state, expected) in [
            ("live", StreamState::Live),
            ("catching_up", StreamState::CatchingUp),
            ("provider_unavailable", StreamState::ProviderUnavailable),
            ("paused", StreamState::Paused),
            ("a_later_state", StreamState::Unknown),
        ] {
            assert_eq!(
                decode(&format!(r#"{{"type":"status","channel":"mic","state":"{state}","lagMs":1800}}"#)),
                Some(ServerMessage::Status { channel: Channel::Mic, state: expected })
            );
        }
        assert_eq!(
            decode(r#"{"type":"error","code":"provider_unavailable","message":"…","retryable":true,"retryAfterMs":5000}"#),
            Some(ServerMessage::Error { code: "provider_unavailable".into(), retryable: true })
        );
        // Additive protocol changes are ignored, not fatal; junk is not a message.
        assert_eq!(decode(r#"{"type":"a_later_message","x":1}"#), Some(ServerMessage::Unknown));
        assert_eq!(decode(r#"{"type":"ack","channel":"front"}"#), None);
        assert_eq!(decode("not json"), None);
    }

    #[test]
    fn refusals_and_close_codes_follow_the_protocol_notes() {
        // Not configured, at the limit, or not allowed: stay on parts, no retry.
        for status in [503, 429, 401, 403] {
            assert_eq!(refusal(status), Outcome::Stop(false), "HTTP {status}");
        }
        for status in [409, 404] {
            assert_eq!(refusal(status), Outcome::NotRecording, "HTTP {status}");
        }
        assert_eq!(refusal(502), Outcome::Retry);
        for code in [4400, 4401, 4404, 4409, 4429] {
            assert_eq!(closed(code), Outcome::Stop(false), "close {code}");
        }
        assert_eq!(closed(4503), Outcome::Stop(true));
        for code in [4408, 1001, 1006, 1011, 1012, 1005] {
            assert_eq!(closed(code), Outcome::Retry, "close {code}");
        }
        assert_eq!(closed(1000), Outcome::Finished);
        assert_eq!(BACK_OFF.map(|delay| delay.as_millis()), [500, 1000, 2000, 4000, 8000]);
    }

    // ------------------------------------------------------------ resampler

    pub(crate) fn tone(rate: u32, hz: f64, first: u64, count: usize) -> Vec<f32> {
        (0..count)
            .map(|i| {
                let t = (first + i as u64) as f64 / rate as f64;
                (0.5 * (std::f64::consts::TAU * hz * t).sin()) as f32
            })
            .collect()
    }

    fn convert(rate: u32, input: &[f32]) -> Vec<i16> {
        let mut resampler = Resampler::new(rate, RATE);
        let mut out = Vec::new();
        resampler.push(input, &mut out);
        resampler.finish(input.len() as u64, &mut out);
        out
    }

    /// Level of a 0.5-amplitude tone after conversion, in dB relative to the
    /// input, measured over the middle of one second.
    fn gain_db(rate: u32, hz: f64) -> f64 {
        let out = convert(rate, &tone(rate, hz, 0, rate as usize));
        let middle = &out[2_000..14_000];
        let power = middle.iter().map(|s| (*s as f64 / 32767.).powi(2)).sum::<f64>() / middle.len() as f64;
        10. * (power.max(1e-12) / 0.125).log10()
    }

    #[test]
    fn speech_band_passes_and_everything_above_eight_kilohertz_is_removed() {
        for rate in [48_000, 44_100] {
            for hz in [200., 1_000., 3_400., 6_000.] {
                let gain = gain_db(rate, hz);
                assert!(gain.abs() < 0.1, "{hz} Hz at {rate}: {gain:.3} dB");
            }
            let edge = gain_db(rate, 7_000.);
            assert!((-0.5..0.1).contains(&edge), "7 kHz at {rate}: {edge:.3} dB");
            // These would fold back to 7.5, 7, 4 and 1 kHz without the filter.
            for hz in [8_500., 9_000., 12_000., 15_000., 20_000.] {
                let gain = gain_db(rate, hz);
                assert!(gain < -60., "{hz} Hz at {rate}: {gain:.1} dB");
            }
        }
    }

    #[test]
    fn a_tone_keeps_its_frequency_and_phase() {
        for rate in [48_000u32, 44_100, 32_000, 96_000, 8_000, 16_000] {
            let out = convert(rate, &tone(rate, 1_000., 0, rate as usize));
            assert_eq!(out.len(), 16_000, "{rate}");
            for (n, sample) in out.iter().enumerate().take(15_000).skip(1_000) {
                let expected = 0.5 * (std::f64::consts::TAU * 1_000. * n as f64 / 16_000.).sin() * 32767.;
                assert!((*sample as f64 - expected).abs() < 40., "{rate} Hz, sample {n}: {sample} vs {expected:.0}");
            }
        }
    }

    #[test]
    fn output_length_is_exact_for_every_rate_and_an_hour_has_no_drift() {
        assert_eq!(Resampler::output_len(48_000, RATE, 3 * 3_600 * 48_000), 3 * 3_600 * 16_000);
        assert_eq!(Resampler::output_len(44_100, RATE, 3_600 * 44_100), 3_600 * 16_000);
        assert_eq!(Resampler::output_len(44_100, RATE, 441), 160);
        assert_eq!(Resampler::output_len(44_100, RATE, 442), 161);
        assert_eq!(Resampler::output_len(48_000, RATE, 1), 1);
        assert_eq!(Resampler::output_len(48_000, RATE, 0), 0);
        assert_eq!(Resampler::output_len(8_000, RATE, 8_000), 16_000);
        // The last 100 ms of an hour-long capture, converted on its own from
        // its position in the file, lands exactly where it would have after
        // an hour of streaming: same count, and the tone is still in phase
        // (one sample of drift would be 22 degrees at 1 kHz).
        for rate in [48_000u32, 44_100] {
            let total = 3_600 * rate as u64;
            let outputs = 3_600 * RATE as u64;
            let mut resampler = Resampler::new(rate, RATE);
            let from = resampler.seek(outputs - 1_600);
            let mut out = Vec::new();
            resampler.push(&tone(rate, 1_000., from, (total - from) as usize), &mut out);
            resampler.finish(total, &mut out);
            assert_eq!(out.len(), 1_600, "{rate}");
            assert_eq!(resampler.position(), outputs);
            for (i, sample) in out.iter().enumerate().take(1_500) {
                let n = outputs - 1_600 + i as u64;
                let expected = 0.5 * (std::f64::consts::TAU * 1_000. * (n % 16_000) as f64 / 16_000.).sin() * 32767.;
                assert!((*sample as f64 - expected).abs() < 40., "{rate} Hz, sample {n}: {sample} vs {expected:.0}");
            }
        }
    }

    #[test]
    fn streaming_in_pieces_and_restarting_mid_file_give_identical_samples() {
        for rate in [48_000u32, 44_100, 22_050] {
            let input: Vec<f32> = tone(rate, 440., 0, rate as usize * 2)
                .iter()
                .zip(tone(rate, 2_300., 0, rate as usize * 2))
                .map(|(a, b)| a * 0.6 + b * 0.3)
                .collect();
            let whole = convert(rate, &input);
            assert_eq!(whole.len() as u64, Resampler::output_len(rate, RATE, input.len() as u64));
            // Ragged pieces, as the file grows.
            let mut resampler = Resampler::new(rate, RATE);
            let (mut out, mut at, mut size) = (Vec::new(), 0, 1);
            while at < input.len() {
                let end = (at + size).min(input.len());
                resampler.push(&input[at..end], &mut out);
                at = end;
                size = size * 7 % 1_013 + 1;
            }
            resampler.finish(input.len() as u64, &mut out);
            assert_eq!(out, whole, "{rate} in pieces");
            // A reconnect: start again at an arbitrary output sample.
            for start in [1u64, 159, 160, 4_001, 20_000] {
                let mut resampler = Resampler::new(rate, RATE);
                let from = resampler.seek(start) as usize;
                let mut out = Vec::new();
                resampler.push(&input[from..], &mut out);
                resampler.finish(input.len() as u64, &mut out);
                assert_eq!(out, whole[start as usize..], "{rate} from {start}");
            }
        }
    }

    // ------------------------------------------------------------ following files

    fn scratch(name: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!("cereal-live-{name}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        directory
    }

    /// A WAV laid out and checkpointed exactly as the native writer does it:
    /// 56-byte header with a `fact` chunk, samples written first, lengths after.
    pub(crate) struct Growing {
        file: File,
        frames: u64,
    }

    impl Growing {
        fn create(path: &Path, rate: u32) -> Self {
            let mut header = Vec::new();
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
            assert_eq!(header.len(), 56);
            let mut file = File::options().write(true).create_new(true).open(path).unwrap();
            file.write_all(&header).unwrap();
            Self { file, frames: 0 }
        }
        fn write_samples(&mut self, samples: &[f32]) {
            let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
            self.file.write_all_at(&bytes, 56 + self.frames * 4).unwrap();
            self.frames += samples.len() as u64;
        }
        fn checkpoint(&mut self) {
            self.file.write_all_at(&(self.frames as u32 * 4).to_le_bytes(), 52).unwrap();
            self.file.write_all_at(&(self.frames as u32).to_le_bytes(), 44).unwrap();
            self.file.write_all_at(&(48 + self.frames as u32 * 4).to_le_bytes(), 4).unwrap();
        }
        pub(crate) fn append(&mut self, samples: &[f32]) {
            self.write_samples(samples);
            self.checkpoint();
        }
    }

    fn drain(track: &mut Track, out: &mut Vec<i16>) {
        loop {
            track.fill();
            let Some(frame) = track.frame(1) else { break };
            assert!(frame.len() <= FRAME_MAX);
            let count = frame.len();
            out.extend_from_slice(frame);
            track.advance(count);
        }
    }

    #[test]
    fn a_growing_file_is_followed_to_its_end_without_reading_unadvertised_samples() {
        let directory = scratch("follow");
        let path = directory.join("mic.wav");
        let paths = vec![path.to_string_lossy().into_owned()];
        let input = tone(48_000, 700., 0, 48_000 * 3);
        let mut track = Track::new();
        let mut out = Vec::new();
        // Nothing yet: not even a file.
        track.sync(&[], false);
        drain(&mut track, &mut out);
        assert!(out.is_empty() && !track.has_files());
        let mut file = Growing::create(&path, 48_000);
        track.sync(&paths, false);
        drain(&mut track, &mut out);
        assert!(out.is_empty());
        // 10 ms writes, read after every few of them.
        for (index, block) in input.chunks(480).enumerate() {
            if index == 100 {
                // Samples on disk that the header does not advertise yet (the
                // writer is between its two writes) are not read.
                drain(&mut track, &mut out);
                let before = out.len();
                file.write_samples(block);
                drain(&mut track, &mut out);
                assert_eq!(out.len(), before);
                file.checkpoint();
            } else {
                file.append(block);
            }
            if index % 7 == 0 {
                drain(&mut track, &mut out);
            }
            assert!(!track.exhausted());
        }
        drain(&mut track, &mut out);
        // Up to the filter's reach of the end while the file may still grow.
        assert!(out.len() > 48_000 - 60 && out.len() < 48_000);
        assert!((track.end() as i64 - 48_000).abs() <= 1);
        track.sync(&paths, true);
        drain(&mut track, &mut out);
        assert!(track.exhausted());
        assert_eq!(track.cursor(), 48_000);
        assert_eq!(track.end(), 48_000);
        assert_eq!(out, convert(48_000, &input));
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn retry_files_continue_the_channel_and_the_cursor_can_return_to_any_sample() {
        let directory = scratch("retry");
        let first = directory.join("system.wav");
        let second = directory.join("system-retry-1.wav");
        let names = |paths: &[&PathBuf]| -> Vec<String> {
            paths.iter().map(|path| path.to_string_lossy().into_owned()).collect()
        };
        // The tap came back at another rate after the retry.
        let one = tone(48_000, 500., 0, 48_000 + 77);
        let two = tone(44_100, 900., 0, 44_100 * 2 + 5);
        let mut expected = convert(48_000, &one);
        let boundary = expected.len() as u64;
        assert_eq!(boundary, Resampler::output_len(48_000, RATE, one.len() as u64));
        expected.extend(convert(44_100, &two));
        let mut track = Track::new();
        let mut out = Vec::new();
        let mut file = Growing::create(&first, 48_000);
        file.append(&one[..30_000]);
        track.sync(&names(&[&first]), false);
        drain(&mut track, &mut out);
        file.append(&one[30_000..]);
        // The retry file appears: the first is final from now on.
        let mut retry = Growing::create(&second, 44_100);
        retry.append(&two[..10_000]);
        track.sync(&names(&[&first, &second]), false);
        drain(&mut track, &mut out);
        assert!(out.len() as u64 > boundary);
        retry.append(&two[10_000..]);
        track.sync(&names(&[&first, &second]), true);
        drain(&mut track, &mut out);
        assert!(track.exhausted());
        assert_eq!(out.len(), expected.len());
        assert_eq!(out, expected);
        // Resume inside the retired file, on the boundary, and in the second.
        for sample in [0, 123, boundary - 1, boundary, boundary + 4_000, expected.len() as u64 - 10] {
            track.seek(sample);
            assert!(!track.exhausted());
            let mut again = Vec::new();
            drain(&mut track, &mut again);
            assert_eq!(again, expected[sample as usize..], "from {sample}");
            assert_eq!(track.cursor(), expected.len() as u64);
        }
        // Past the end there is nothing, and that is the end.
        track.seek(expected.len() as u64);
        assert!(track.exhausted());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn manifest_channels_keep_their_order_and_report_what_is_still_starting() {
        let mut manifest = crate::audio::CaptureManifest {
            version: 1,
            capture_id: "capture-1".into(),
            note_id: 1,
            directory: PathBuf::from("/x"),
            manifest_path: PathBuf::from("/x/manifest.json"),
            started_at: "2026-11-14T10:00:00Z".into(),
            finished_at: None,
            stats: Default::default(),
            error: None,
        };
        manifest.stats.requested_channels = 3;
        manifest.stats.mic.path = Some("/x/mic.wav".into());
        manifest.stats.system.state = "starting".into();
        let capture = Capture::of(&manifest);
        assert_eq!(capture.paths, [vec!["/x/mic.wav".to_string()], vec![]]);
        assert_eq!((capture.starting, capture.done), ([false, true], false));
        manifest.stats.system.state = "recording".into();
        manifest.stats.system.path = Some("/x/system-retry-1.wav".into());
        manifest.stats.system.paths = vec!["/x/system.wav".into(), "/x/system-retry-1.wav".into()];
        manifest.finished_at = Some("2026-11-14T10:30:00Z".into());
        let capture = Capture::of(&manifest);
        assert_eq!(capture.paths[1], ["/x/system.wav", "/x/system-retry-1.wav"]);
        assert_eq!((capture.starting, capture.done), ([false, false], true));
        // A channel that was not asked for, or that failed, is not waited for.
        manifest.stats.requested_channels = 1;
        manifest.stats.system = Default::default();
        manifest.stats.system.state = "starting".into();
        assert_eq!(Capture::of(&manifest).starting, [false, false]);
    }

    // ------------------------------------------------------------ against a socket

    /// A capture directory with a manifest on disk, written the way the app
    /// writes it, for sessions that follow `manifest.json`.
    pub(crate) struct Fixture {
        pub(crate) directory: PathBuf,
        pub(crate) manifest: crate::audio::CaptureManifest,
    }

    impl Fixture {
        pub(crate) fn new(name: &str) -> Self {
            let directory = scratch(name);
            let manifest = crate::audio::CaptureManifest {
                version: 1,
                capture_id: format!("capture-{name}"),
                note_id: 1,
                manifest_path: directory.join("manifest.json"),
                directory: directory.clone(),
                started_at: "2026-11-14T10:00:00Z".into(),
                finished_at: None,
                stats: Default::default(),
                error: None,
            };
            let fixture = Self { directory, manifest };
            fixture.save();
            fixture
        }
        fn save(&self) {
            let temporary = self.directory.join(".manifest.tmp");
            std::fs::write(&temporary, serde_json::to_vec(&self.manifest).unwrap()).unwrap();
            std::fs::rename(temporary, &self.manifest.manifest_path).unwrap();
        }
        pub(crate) fn add(&mut self, channel: Channel, name: &str, rate: u32) -> Growing {
            let path = self.directory.join(name);
            let file = Growing::create(&path, rate);
            let stats = match channel {
                Channel::Mic => &mut self.manifest.stats.mic,
                Channel::System => &mut self.manifest.stats.system,
            };
            stats.state = "recording".into();
            stats.sample_rate = rate as f64;
            stats.paths.push(path.to_string_lossy().into_owned());
            stats.path = stats.paths.last().cloned();
            self.manifest.stats.requested_channels |= 1 << channel.index();
            self.save();
            file
        }
        pub(crate) fn finish(&mut self) {
            self.manifest.finished_at = Some("2026-11-14T10:30:00Z".into());
            self.save();
        }
        pub(crate) fn source(&self) -> Source {
            Source::File(self.manifest.manifest_path.clone())
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.directory);
        }
    }

    type ServerSocket = WebSocket<TcpStream>;

    /// What the fake server saw, for the test body to assert on.
    #[derive(Debug)]
    enum Seen {
        Hello(serde_json::Value),
        Headers { authorization: Option<String>, origin: bool, path: String },
        Frame { channel: u8, first: u64, samples: Vec<i16>, at: Instant },
        Mark(serde_json::Value),
        Closed,
    }

    struct Server {
        url: String,
        seen: mpsc::Receiver<Seen>,
        connections: Arc<AtomicUsize>,
    }

    /// A local WebSocket server. `handler` runs once per connection (given
    /// its index) on the server's thread; when it returns the connection is
    /// dropped.
    fn server(mut handler: impl FnMut(usize, TcpStream, &mpsc::Sender<Seen>) + Send + 'static) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("ws://{}/api/v1/notes/0b1c2d3e-0000-4000-8000-000000000001/live", listener.local_addr().unwrap());
        let (sender, seen) = mpsc::channel();
        let connections = Arc::new(AtomicUsize::new(0));
        let count = connections.clone();
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { return };
                let _ = stream.set_read_timeout(Some(Duration::from_secs(8)));
                let index = count.fetch_add(1, Ordering::SeqCst);
                handler(index, stream, &sender);
                if sender.send(Seen::Closed).is_err() {
                    return;
                }
            }
        });
        Server { url, seen, connections }
    }

    fn accept(stream: TcpStream, seen: &mpsc::Sender<Seen>) -> ServerSocket {
        use tungstenite::handshake::server::{Request, Response};
        let report = seen.clone();
        let socket = tungstenite::accept_hdr(stream, move |request: &Request, response: Response| {
            let _ = report.send(Seen::Headers {
                authorization: request
                    .headers()
                    .get("authorization")
                    .and_then(|value| value.to_str().ok())
                    .map(str::to_owned),
                origin: request.headers().contains_key("origin"),
                path: request.uri().path().to_owned(),
            });
            Ok(response)
        })
        .expect("the client completes the upgrade");
        socket
    }

    fn refuse(stream: TcpStream, status: u16) {
        use tungstenite::handshake::server::{ErrorResponse, Request, Response};
        let _ = tungstenite::accept_hdr(stream, move |_: &Request, _: Response| {
            Err::<Response, ErrorResponse>(
                tungstenite::http::Response::builder()
                    .status(status)
                    .body(Some(r#"{"error":"unavailable","message":"Live transcription is not configured"}"#.into()))
                    .unwrap(),
            )
        });
    }

    fn say(socket: &mut ServerSocket, value: serde_json::Value) {
        socket.send(Message::text(value.to_string())).expect("the client is listening");
    }

    fn ready(socket: &mut ServerSocket, accept: &[(&str, u64)], window: u64, revision: u64) {
        let streams: Vec<_> = accept
            .iter()
            .map(|(channel, from)| serde_json::json!({"channel": channel, "acceptFromSample": from}))
            .collect();
        say(socket, serde_json::json!({
            "type": "ready", "sessionId": "0b1c2d3e-0000-4000-8000-0000000000aa", "provider": "fake",
            "model": "fake-live", "streams": streams, "resumeWindowMs": window, "transcriptRevision": revision,
        }));
    }

    /// Read until the client's next message, reporting it. `None` when the
    /// client has gone or says nothing for the socket's timeout.
    fn hear(socket: &mut ServerSocket, seen: &mpsc::Sender<Seen>) -> Option<Message> {
        loop {
            let message = socket.read().ok()?;
            match &message {
                Message::Text(text) => {
                    let value: serde_json::Value = serde_json::from_str(text.as_str()).expect("the client sends JSON");
                    let report = if value["type"] == "hello" { Seen::Hello(value) } else { Seen::Mark(value) };
                    let _ = seen.send(report);
                }
                Message::Binary(bytes) => {
                    assert!(bytes.len() > FRAME_HEADER && (bytes.len() - FRAME_HEADER) % 2 == 0);
                    assert_eq!((bytes[0], bytes[2], bytes[3]), (1, 0, 0), "version and reserved bytes");
                    let _ = seen.send(Seen::Frame {
                        channel: bytes[1],
                        first: u64::from_le_bytes(bytes[4..12].try_into().unwrap()),
                        samples: bytes[FRAME_HEADER..]
                            .chunks_exact(2)
                            .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
                            .collect(),
                        at: Instant::now(),
                    });
                }
                Message::Close(_) => return None,
                _ => continue,
            }
            return Some(message);
        }
    }

    fn target(server: &Server, fixture: &Fixture) -> Target {
        Target {
            url: server.url.clone(),
            token: "t".repeat(40),
            capture_id: fixture.manifest.capture_id.clone(),
            base_ms: 754_000,
        }
    }

    /// A handover key of the fixture's own, as a note's ID is in the app.
    fn key(fixture: &Fixture) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        fixture.directory.hash(&mut hasher);
        hasher.finish()
    }

    fn session(server: &Server, fixture: &Fixture) -> Session {
        let target = target(server, fixture);
        Session::start(key(fixture), fixture.source(), move || Resolved::Ready(target.clone()), || true)
    }

    /// Everything the session reports until it ends.
    fn events(session: &Session) -> Vec<Event> {
        let mut events = Vec::new();
        loop {
            match session.events.recv_timeout(Duration::from_secs(20)) {
                Ok(Event::Ended) => return events,
                Ok(event) => events.push(event),
                Err(error) => panic!("the session did not end: {error}; so far {events:?}"),
            }
        }
    }

    fn until(seen: &mpsc::Receiver<Seen>, mut found: impl FnMut(&Seen) -> bool) -> Vec<Seen> {
        let mut all = Vec::new();
        loop {
            let next = seen
                .recv_timeout(Duration::from_secs(20))
                .unwrap_or_else(|error| panic!("the server did not see it: {error}; so far {all:?}"));
            let done = found(&next);
            all.push(next);
            if done {
                return all;
            }
        }
    }

    fn frames(seen: &[Seen], channel: u8) -> Vec<(u64, &[i16])> {
        seen.iter()
            .filter_map(|seen| match seen {
                Seen::Frame { channel: c, first, samples, .. } if *c == channel => Some((*first, samples.as_slice())),
                _ => None,
            })
            .collect()
    }

    /// The samples of consecutive frames, checking that each starts where the
    /// one before it ended.
    fn contiguous(frames: &[(u64, &[i16])]) -> Vec<i16> {
        let mut next = frames.first().map_or(0, |frame| frame.0);
        let mut all = Vec::new();
        for (first, samples) in frames {
            assert_eq!(*first, next, "frames are contiguous");
            next += samples.len() as u64;
            all.extend_from_slice(samples);
        }
        all
    }

    #[test]
    fn a_capture_is_streamed_as_it_grows_and_text_comes_back() {
        let mut fixture = Fixture::new("stream");
        let mut mic = fixture.add(Channel::Mic, "mic.wav", 48_000);
        let mut system = fixture.add(Channel::System, "system.wav", 44_100);
        let server = server(|_, stream, seen| {
            let mut socket = accept(stream, seen);
            hear(&mut socket, seen).expect("hello");
            ready(&mut socket, &[("mic", 0), ("system", 0)], 30_000, 41);
            let mut taken = [0u64; 2];
            let mut spoke = false;
            while let Some(message) = hear(&mut socket, seen) {
                match message {
                    Message::Binary(bytes) => {
                        let channel = bytes[1] as usize;
                        taken[channel] += (bytes.len() - FRAME_HEADER) as u64 / 2;
                        let name = ["mic", "system"][channel];
                        say(&mut socket, serde_json::json!({
                            "type": "ack", "channel": name,
                            "throughSample": taken[channel], "transcribedThroughMs": 754_000,
                        }));
                        if !spoke && taken[0] >= 8_000 {
                            spoke = true;
                            for (rev, text) in ["so if", "so if we move"].iter().enumerate() {
                                say(&mut socket, serde_json::json!({
                                    "type": "interim", "channel": "mic", "utteranceId": "u-1", "rev": rev,
                                    "text": text, "startMs": 754_100, "endMs": 754_900,
                                }));
                            }
                            say(&mut socket, serde_json::json!({
                                "type": "final", "utteranceId": "u-1", "transcriptRevision": 42,
                                "segment": {"id": "419d65d1-7a2c-44ca-84cc-ad489522d562", "channel": "mic",
                                    "speakerId": "mic:n:1", "speakerLabel": "Speaker 1", "text": "So if we move the pilot.",
                                    "isFinal": true, "startMs": 754_100, "endMs": 755_000},
                            }));
                        }
                    }
                    Message::Text(text) if text.as_str().contains(r#""kind":"end""#) => {
                        let _ = socket.close(None);
                        while socket.read().is_ok() {}
                        return;
                    }
                    _ => {}
                }
            }
        });
        let session = session(&server, &fixture);
        // 1.5 s of audio arriving in real time, 10 ms at a time.
        let spoken = tone(48_000, 440., 0, 72_000);
        let played = tone(44_100, 880., 0, 66_150);
        for block in 0..150 {
            mic.append(&spoken[block * 480..(block + 1) * 480]);
            system.append(&played[block * 441..(block + 1) * 441]);
            thread::sleep(Duration::from_millis(10));
        }
        fixture.finish();
        session.end();
        // What the upload does before it tells the server to stop: wait for
        // the stream to hand over. That takes as long as sending the last
        // fraction of a second, not the limit.
        let asked = Instant::now();
        wait_for_handover(key(&fixture), Duration::from_secs(10));
        assert!(asked.elapsed() < Duration::from_secs(3), "{:?}", asked.elapsed());
        let handed_over = until(&server.seen, |seen| matches!(seen, Seen::Mark(_)));
        let events = events(&session);
        let mut seen = handed_over;
        seen.extend(until(&server.seen, |seen| matches!(seen, Seen::Closed)));
        // With the stream gone there is nothing to wait for.
        let asked = Instant::now();
        wait_for_handover(key(&fixture), Duration::from_secs(10));
        assert!(asked.elapsed() < Duration::from_millis(100));
        // One connection, authenticated with the bearer token and no Origin.
        assert_eq!(server.connections.load(Ordering::SeqCst), 1);
        let Seen::Headers { authorization, origin, path } = &seen[0] else { panic!("{:?}", seen[0]) };
        assert_eq!(authorization.as_deref(), Some(format!("Bearer {}", "t".repeat(40)).as_str()));
        assert!(!origin);
        assert!(path.ends_with("/live"));
        let Seen::Hello(hello) = &seen[1] else { panic!("hello is the first frame: {:?}", seen[1]) };
        assert_eq!(
            *hello,
            serde_json::json!({
                "type": "hello", "protocol": 1, "captureId": "capture-stream",
                "streams": [
                    {"channel": "mic", "sampleRate": 16_000, "encoding": "pcm_s16le", "baseMs": 754_000, "resumeFromSample": 0},
                    {"channel": "system", "sampleRate": 16_000, "encoding": "pcm_s16le", "baseMs": 754_000, "resumeFromSample": 0},
                ],
            })
        );
        // Every sample of both channels arrived once, in order, exactly as a
        // conversion of the finished files gives them.
        for (channel, rate, input) in [(0, 48_000, &spoken), (1, 44_100, &played)] {
            let frames = frames(&seen, channel);
            assert_eq!(frames[0].0, 0);
            let all = contiguous(&frames);
            assert_eq!(all.len(), 24_000);
            assert_eq!(all, convert(rate, input), "channel {channel}");
            // 100 to 250 ms a frame, except the flush at the end.
            for (_, samples) in &frames[..frames.len() - 1] {
                assert!((FRAME_MIN..=FRAME_MAX).contains(&samples.len()), "{} samples", samples.len());
            }
            assert!(frames.last().unwrap().1.len() <= FRAME_MAX);
        }
        // Audio went out while the capture was still running, not at the end.
        let first_frame = seen.iter().find_map(|seen| match seen {
            Seen::Frame { at, .. } => Some(*at),
            _ => None,
        });
        let marks: Vec<_> = seen.iter().filter_map(|seen| match seen { Seen::Mark(mark) => Some(mark), _ => None }).collect();
        assert_eq!(marks, [&serde_json::json!({"type": "mark", "kind": "end", "atSample": {"mic": 24_000, "system": 24_000}})]);
        assert!(first_frame.is_some());
        // The window hears the revision, the interim text, then the final.
        assert!(events.contains(&Event::Revision(41)));
        let texts: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                Event::Interim(interim) => Some((interim.rev, interim.text.as_str())),
                _ => None,
            })
            .collect();
        assert_eq!(texts, [(0, "so if"), (1, "so if we move")]);
        let finals: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                Event::Final { utterance_id, revision, segment } => Some((utterance_id.as_str(), *revision, segment.text.as_str())),
                _ => None,
            })
            .collect();
        assert_eq!(finals, [("u-1", 42, "So if we move the pilot.")]);
        assert!(events.contains(&Event::Status(Phase::Live, 0)));
        assert_eq!(events.last(), Some(&Event::Status(Phase::Off, 0)));
    }

    #[test]
    fn a_dropped_socket_resumes_from_the_last_acknowledged_sample() {
        let mut fixture = Fixture::new("resume");
        let mut mic = fixture.add(Channel::Mic, "mic.wav", 48_000);
        let spoken = tone(48_000, 330., 0, 48_000 * 4);
        mic.append(&spoken[..48_000]);
        let server = server(|index, stream, seen| {
            let mut socket = accept(stream, seen);
            hear(&mut socket, seen).expect("hello");
            if index == 0 {
                ready(&mut socket, &[("mic", 0)], 30_000, 0);
                // Take four frames, acknowledge only the first two, and vanish.
                let mut taken = 0u64;
                for frame in 0..4 {
                    let Some(Message::Binary(bytes)) = hear(&mut socket, seen) else { panic!("audio") };
                    if frame < 2 {
                        taken += (bytes.len() - FRAME_HEADER) as u64 / 2;
                        say(&mut socket, serde_json::json!({"type": "ack", "channel": "mic", "throughSample": taken, "transcribedThroughMs": 0}));
                    }
                }
                thread::sleep(Duration::from_millis(100));
                return; // dropped without a close frame
            }
            // The server kept a little more than it had acknowledged.
            let resume = seen_resume(&mut socket);
            ready(&mut socket, &[("mic", resume)], 30_000, 0);
            while let Some(message) = hear(&mut socket, seen) {
                if matches!(&message, Message::Text(text) if text.as_str().contains(r#""kind":"end""#)) {
                    let _ = socket.close(None);
                    while socket.read().is_ok() {}
                    return;
                }
            }
        });
        // The second connection accepts from 800 samples after the last ack.
        fn seen_resume(_: &mut ServerSocket) -> u64 {
            RESUME.load(Ordering::SeqCst) as u64 + 800
        }
        static RESUME: AtomicUsize = AtomicUsize::new(0);
        let session = session(&server, &fixture);
        let first = until(&server.seen, |seen| matches!(seen, Seen::Closed));
        let sent = frames(&first, 0);
        assert_eq!(sent.len(), 4);
        let acked = (sent[0].1.len() + sent[1].1.len()) as u64;
        RESUME.store(acked as usize, Ordering::SeqCst);
        // The capture carries on while the socket is down.
        mic.append(&spoken[48_000..]);
        fixture.finish();
        session.end();
        let second = until(&server.seen, |seen| matches!(seen, Seen::Closed));
        let events = events(&session);
        assert_eq!(server.connections.load(Ordering::SeqCst), 2);
        let hello = second.iter().find_map(|seen| match seen { Seen::Hello(hello) => Some(hello), _ => None }).unwrap();
        assert_eq!(hello["streams"][0]["resumeFromSample"], acked, "resumes from the last ack");
        assert_eq!(hello["captureId"], "capture-resume");
        // It continues from where the server said, to the end, with the same
        // samples a single pass produces.
        let resumed = frames(&second, 0);
        assert_eq!(resumed[0].0, acked + 800);
        let all = contiguous(&resumed);
        let whole = convert(48_000, &spoken);
        assert_eq!(all, whole[(acked + 800) as usize..]);
        // Catching up is paced: 3 s of backlog is not sent in one burst.
        let (start, end) = (second.iter().find_map(frame_time).unwrap(), second.iter().rev().find_map(frame_time).unwrap());
        assert!(end.duration_since(start) > Duration::from_millis(900), "{:?}", end.duration_since(start));
        assert!(events.contains(&Event::Status(Phase::Reconnecting, 0)));
        fn frame_time(seen: &Seen) -> Option<Instant> {
            match seen {
                Seen::Frame { at, .. } => Some(*at),
                _ => None,
            }
        }
    }

    #[test]
    fn a_backlog_longer_than_the_resume_window_is_skipped_to_five_seconds_before_the_end() {
        let mut fixture = Fixture::new("skip");
        let mut mic = fixture.add(Channel::Mic, "mic.wav", 48_000);
        // Twelve seconds were captured before the socket came up.
        mic.append(&tone(48_000, 330., 0, 48_000 * 12));
        let server = server(|_, stream, seen| {
            let mut socket = accept(stream, seen);
            hear(&mut socket, seen).expect("hello");
            ready(&mut socket, &[("mic", 0)], 10_000, 0);
            let started = Instant::now();
            while started.elapsed() < Duration::from_millis(1_500) {
                if hear(&mut socket, seen).is_none() {
                    return;
                }
            }
        });
        let session = session(&server, &fixture);
        let seen = until(&server.seen, |seen| matches!(seen, Seen::Closed));
        drop(session);
        let sent = frames(&seen, 0);
        // 12 s minus 5 s, to within the filter's reach of the growing end.
        assert!((sent[0].0 as i64 - 7 * 16_000).abs() < 100, "first frame at {}", sent[0].0);
        contiguous(&sent);
        // Never faster than twice real time: 1.5 s of wall clock carries at
        // most 3 s of audio plus the half second the pacing lets through at
        // once. (How close it gets to that depends on how busy the machine
        // running the test is, so only the ceiling is asserted tightly.)
        let total: usize = sent.iter().map(|frame| frame.1.len()).sum();
        assert!(total >= 8_000 && total < 16_000 * 4, "{total} samples in 1.5 s");
    }

    #[test]
    fn a_short_resume_window_is_skipped_into_once_and_not_again() {
        let mut fixture = Fixture::new("skip-short");
        let mut mic = fixture.add(Channel::Mic, "mic.wav", 16_000);
        mic.append(&tone(16_000, 330., 0, 16_000 * 12));
        let server = server(|_, stream, seen| {
            let mut socket = accept(stream, seen);
            hear(&mut socket, seen).expect("hello");
            // A window shorter than the usual five seconds back.
            ready(&mut socket, &[("mic", 0)], 2_000, 0);
            let started = Instant::now();
            while started.elapsed() < Duration::from_millis(1_200) {
                if hear(&mut socket, seen).is_none() {
                    return;
                }
            }
        });
        let session = session(&server, &fixture);
        let seen = until(&server.seen, |seen| matches!(seen, Seen::Closed));
        drop(session);
        let sent = frames(&seen, 0);
        // Half the window before the end: inside it, so the backlog is
        // skipped once. (Five seconds back would still be "more than the
        // window behind" and the stream would skip again on every pass.)
        assert!((sent[0].0 as i64 - 11 * 16_000).abs() < 100, "first frame at {}", sent[0].0);
        contiguous(&sent);
        let total: usize = sent.iter().map(|frame| frame.1.len()).sum();
        assert!((14_000..=16_000).contains(&total), "the last second, once: {total} samples");
    }

    #[test]
    fn pause_resume_and_end_are_marked_at_the_sample_they_happen() {
        let mut fixture = Fixture::new("marks");
        let mut mic = fixture.add(Channel::Mic, "mic.wav", 16_000);
        let spoken = tone(16_000, 330., 0, 16_000 * 2);
        mic.append(&spoken[..10_000]);
        let server = server(|_, stream, seen| {
            let mut socket = accept(stream, seen);
            hear(&mut socket, seen).expect("hello");
            ready(&mut socket, &[("mic", 0)], 30_000, 0);
            while let Some(message) = hear(&mut socket, seen) {
                if matches!(&message, Message::Text(text) if text.as_str().contains(r#""kind":"end""#)) {
                    let _ = socket.close(None);
                    while socket.read().is_ok() {}
                    return;
                }
            }
        });
        let session = session(&server, &fixture);
        session.pause();
        let paused = until(&server.seen, |seen| matches!(seen, Seen::Mark(_)));
        // Everything captured so far went out first, short last frame included.
        assert_eq!(contiguous(&frames(&paused, 0)).len(), 10_000);
        let Some(Seen::Mark(mark)) = paused.last() else { panic!() };
        assert_eq!(*mark, serde_json::json!({"type": "mark", "kind": "pause", "atSample": {"mic": 10_000}}));
        // Audio written while on hold is not sent.
        mic.append(&spoken[10_000..20_000]);
        assert!(server.seen.recv_timeout(Duration::from_millis(400)).is_err());
        session.resume();
        mic.append(&spoken[20_000..]);
        fixture.finish();
        session.end();
        let rest = until(&server.seen, |seen| matches!(seen, Seen::Closed));
        let marks: Vec<_> = rest.iter().filter_map(|seen| match seen { Seen::Mark(mark) => Some(mark.clone()), _ => None }).collect();
        assert_eq!(
            marks,
            [
                serde_json::json!({"type": "mark", "kind": "resume", "atSample": {"mic": 10_000}}),
                serde_json::json!({"type": "mark", "kind": "end", "atSample": {"mic": 32_000}}),
            ]
        );
        let resumed = frames(&rest, 0);
        assert_eq!(resumed[0].0, 10_000, "continues from the same sample");
        // 16 kHz PCM is passed through sample for sample.
        let all = contiguous(&resumed);
        let expected: Vec<i16> = spoken[10_000..].iter().map(|s| pcm16(*s)).collect();
        assert_eq!(all, expected);
        let events = events(&session);
        assert!(events.contains(&Event::Status(Phase::Paused, 0)));
    }

    #[test]
    fn a_stream_that_is_behind_at_stop_says_how_long_the_capture_was() {
        let mut fixture = Fixture::new("behind");
        let mut mic = fixture.add(Channel::Mic, "mic.wav", 16_000);
        // Eight seconds were captured before the socket came up; at twice
        // real time the stream needs four seconds to send them.
        mic.append(&tone(16_000, 330., 0, 16_000 * 8));
        let server = server(|_, stream, seen| {
            let mut socket = accept(stream, seen);
            hear(&mut socket, seen).expect("hello");
            ready(&mut socket, &[("mic", 0)], 30_000, 0);
            while let Some(message) = hear(&mut socket, seen) {
                if matches!(&message, Message::Text(text) if text.as_str().contains(r#""kind":"end""#)) {
                    let _ = socket.close(None);
                    while socket.read().is_ok() {}
                    return;
                }
            }
        });
        let session = session(&server, &fixture);
        let mut seen = until(&server.seen, |seen| matches!(seen, Seen::Frame { .. }));
        // Stop. The upload waits a moment for the stream and then goes ahead.
        fixture.finish();
        session.end();
        let waited = Instant::now();
        wait_for_handover(key(&fixture), Duration::from_millis(300));
        assert!(waited.elapsed() < Duration::from_millis(1_500), "the wait is bounded");
        seen.extend(until(&server.seen, |seen| matches!(seen, Seen::Closed)));
        // The mark says how much was captured, which is more than was sent,
        // and nothing more of the capture follows it.
        let marks: Vec<_> = seen.iter().filter_map(|seen| match seen { Seen::Mark(mark) => Some(mark.clone()), _ => None }).collect();
        assert_eq!(marks, [serde_json::json!({"type": "mark", "kind": "end", "atSample": {"mic": 128_000}})]);
        let sent: usize = frames(&seen, 0).iter().map(|frame| frame.1.len()).sum();
        assert!(sent > 0 && sent < 128_000, "{sent} of 128000 samples had been sent");
        let marked = seen.iter().position(|seen| matches!(seen, Seen::Mark(_))).unwrap();
        assert!(!seen[marked..].iter().any(|seen| matches!(seen, Seen::Frame { .. })));
        drop(session);
    }

    #[test]
    fn an_instance_without_live_transcription_is_tried_once_and_stays_silent() {
        let mut fixture = Fixture::new("off");
        let mut mic = fixture.add(Channel::Mic, "mic.wav", 48_000);
        mic.append(&tone(48_000, 330., 0, 48_000));
        let server = server(|_, stream, _| refuse(stream, 503));
        let session = session(&server, &fixture);
        let events = events(&session);
        // No text, no warning, no status: the window never hears of it.
        assert_eq!(events, []);
        thread::sleep(Duration::from_millis(700));
        assert_eq!(server.connections.load(Ordering::SeqCst), 1, "503 is not retried");
    }

    #[test]
    fn a_note_that_is_not_recording_yet_is_retried_until_the_server_has_started_it() {
        let mut fixture = Fixture::new("early");
        let mut mic = fixture.add(Channel::Mic, "mic.wav", 48_000);
        mic.append(&tone(48_000, 330., 0, 24_000));
        fixture.finish();
        let server = server(|index, stream, seen| {
            if index == 0 {
                return refuse(stream, 409);
            }
            let mut socket = accept(stream, seen);
            hear(&mut socket, seen).expect("hello");
            ready(&mut socket, &[("mic", 0)], 30_000, 0);
            while let Some(message) = hear(&mut socket, seen) {
                if matches!(&message, Message::Text(text) if text.as_str().contains(r#""kind":"end""#)) {
                    let _ = socket.close(None);
                    while socket.read().is_ok() {}
                    return;
                }
            }
        });
        // The target is not known on the first ask either.
        let (target, mut asked) = (target(&server, &fixture), 0);
        let session = Session::start(
            key(&fixture),
            fixture.source(),
            move || {
                asked += 1;
                if asked == 1 { Resolved::Later } else { Resolved::Ready(target.clone()) }
            },
            || true,
        );
        // Ending is asked for only once the stream is up, as Stop would be.
        let mut before = Vec::new();
        loop {
            let event = session.events.recv_timeout(Duration::from_secs(20)).unwrap();
            let live = event == Event::Status(Phase::Live, 0);
            before.push(event);
            if live {
                break;
            }
        }
        session.end();
        let events = events(&session);
        let seen = until(&server.seen, |seen| matches!(seen, Seen::Mark(_)));
        assert_eq!(server.connections.load(Ordering::SeqCst), 2);
        assert_eq!(contiguous(&frames(&seen, 0)).len(), 8_000);
        assert_eq!(events.last(), Some(&Event::Status(Phase::Off, 0)));
    }

    #[test]
    fn a_provider_outage_is_reported_and_the_stream_does_not_retry() {
        let mut fixture = Fixture::new("provider");
        let mut mic = fixture.add(Channel::Mic, "mic.wav", 48_000);
        mic.append(&tone(48_000, 330., 0, 48_000));
        let server = server(|_, stream, seen| {
            use tungstenite::protocol::{CloseFrame, frame::coding::CloseCode};
            let mut socket = accept(stream, seen);
            hear(&mut socket, seen).expect("hello");
            ready(&mut socket, &[("mic", 0)], 30_000, 0);
            hear(&mut socket, seen).expect("audio");
            // The provider stops answering: first a status while the server
            // retries it, then the server gives up.
            say(&mut socket, serde_json::json!({"type": "status", "channel": "mic", "state": "provider_unavailable", "lagMs": 4_000}));
            thread::sleep(Duration::from_millis(300));
            say(&mut socket, serde_json::json!({"type": "gap", "channel": "mic", "fromMs": 754_000, "toMs": 755_000, "reason": "provider_unavailable"}));
            say(&mut socket, serde_json::json!({"type": "error", "code": "provider_unavailable", "message": "The speech provider is unavailable", "retryable": false}));
            let _ = socket.close(Some(CloseFrame { code: CloseCode::from(4503), reason: "provider unavailable".into() }));
            while socket.read().is_ok() {}
        });
        let session = session(&server, &fixture);
        let events = events(&session);
        assert!(events.contains(&Event::Status(Phase::ProviderUnavailable, 0)));
        assert!(events.contains(&Event::Gap { channel: Channel::Mic, from_ms: 754_000, to_ms: 755_000 }));
        // The last thing said about the stream is why it stopped, and it
        // stays on durable parts.
        let last = events.iter().rfind(|event| matches!(event, Event::Status(..)));
        assert_eq!(last, Some(&Event::Status(Phase::ProviderUnavailable, 0)));
        thread::sleep(Duration::from_millis(700));
        assert_eq!(server.connections.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_channel_that_starts_late_is_declared_with_a_new_hello_that_resumes_the_other() {
        let mut fixture = Fixture::new("late");
        let mut mic = fixture.add(Channel::Mic, "mic.wav", 48_000);
        mic.append(&tone(48_000, 330., 0, 48_000));
        let server = server(|index, stream, seen| {
            use tungstenite::protocol::{CloseFrame, frame::coding::CloseCode};
            let mut socket = accept(stream, seen);
            let Some(Message::Text(hello)) = hear(&mut socket, seen) else { panic!("hello") };
            if index == 1 {
                // The server has not let go of the first socket yet: to it
                // the new hello is another publisher.
                say(&mut socket, serde_json::json!({"type": "error", "code": "replaced", "message": "Another device is streaming this recording", "retryable": false}));
                let _ = socket.close(Some(CloseFrame { code: CloseCode::from(4409), reason: "replaced".into() }));
                while socket.read().is_ok() {}
                return;
            }
            let hello: serde_json::Value = serde_json::from_str(hello.as_str()).unwrap();
            let accept: Vec<(&str, u64)> = hello["streams"]
                .as_array()
                .unwrap()
                .iter()
                .map(|stream| (if stream["channel"] == "mic" { "mic" } else { "system" }, stream["resumeFromSample"].as_u64().unwrap()))
                .collect();
            ready(&mut socket, &accept, 30_000, 0);
            let mut taken = accept.iter().find(|(channel, _)| *channel == "mic").unwrap().1;
            while let Some(message) = hear(&mut socket, seen) {
                match message {
                    Message::Binary(bytes) if bytes[1] == 0 => {
                        taken += (bytes.len() - FRAME_HEADER) as u64 / 2;
                        say(&mut socket, serde_json::json!({"type": "ack", "channel": "mic", "throughSample": taken, "transcribedThroughMs": 0}));
                    }
                    Message::Text(text) if index == 2 && text.as_str().contains(r#""kind":"end""#) => {
                        let _ = socket.close(None);
                        while socket.read().is_ok() {}
                        return;
                    }
                    _ => {}
                }
            }
        });
        let session = session(&server, &fixture);
        // The microphone alone at first, long enough for acknowledgements
        // to have come back.
        let mut heard = 0;
        let first = until(&server.seen, |seen| {
            heard += usize::from(matches!(seen, Seen::Frame { .. }));
            heard == 4
        });
        let Some(Seen::Hello(hello)) = first.iter().find(|seen| matches!(seen, Seen::Hello(_))) else { panic!() };
        assert_eq!(hello["streams"].as_array().unwrap().len(), 1);
        thread::sleep(Duration::from_millis(500));
        // System audio comes up after its retry.
        let mut system = fixture.add(Channel::System, "system-retry-1.wav", 48_000);
        system.append(&tone(48_000, 660., 0, 24_000));
        fixture.finish();
        session.end();
        let rest = until(&server.seen, |seen| matches!(seen, Seen::Mark(_)));
        // Refused once as "another publisher" (its own old socket), it asks again.
        assert_eq!(server.connections.load(Ordering::SeqCst), 3);
        let Some(Seen::Hello(hello)) = rest.iter().rfind(|seen| matches!(seen, Seen::Hello(_))) else { panic!("{rest:?}") };
        let streams = hello["streams"].as_array().unwrap();
        assert_eq!(streams.len(), 2);
        assert_eq!(streams[1]["channel"], "system");
        assert_eq!(streams[1]["resumeFromSample"], 0);
        // The microphone carries on from what was acknowledged.
        let resumed = streams[0]["resumeFromSample"].as_u64().unwrap();
        assert!(resumed >= 1_600, "resumed from {resumed}");
        let hello_at = rest.iter().rposition(|seen| matches!(seen, Seen::Hello(_))).unwrap();
        assert_eq!(frames(&rest[hello_at..], 0)[0].0, resumed);
        assert_eq!(contiguous(&frames(&rest[hello_at..], 1)).len(), 8_000);
        let Some(Seen::Mark(mark)) = rest.last() else { panic!() };
        assert_eq!(mark["atSample"], serde_json::json!({"mic": 16_000, "system": 8_000}));
        events(&session);
    }
}
