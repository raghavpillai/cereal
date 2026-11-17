/**
 * One live transcription session: a client socket on one side, one upstream provider session per audio
 * channel on the other.
 *
 * The session takes audio frames addressed by capture sample index, relays them upstream, turns provider
 * events into `interim` and `final` messages, and stores every final as transcript segments together
 * with the coverage it accounts for. It never holds the transcript: stored text is in the repository,
 * and only the unfinished utterance of each channel lives here.
 *
 * It outlives its socket for a short while, so a brief network drop keeps the provider session and its
 * speaker labels. The live transcript is the only transcript a recording gets: audio it does not manage
 * to transcribe (an outage longer than the client's backlog, a provider that stays down) has no text.
 *
 * Speaker labels: who said each word is decided as described in speakers.ts. When the session ends in an
 * orderly way its provider sessions are handed to the hub, which holds them for `holdMs`; the note's
 * next session takes them over with their voices (link.ts).
 */
import { Effect } from "effect"
import type { TranscriptSegmentDTO } from "@meeting-notes/contracts"
import type { LiveAudioFrame, LiveHelloMessage, LiveMarkMessage, LiveServerMessage, LiveStreamState, LiveTranscriber, LiveTranscriberEvent, LiveTranscriberSession } from "@meeting-notes/contracts/live"
import { LiveCloseCode } from "@meeting-notes/contracts/live"
import { liveSpeakerId, liveSpeakerLabel, liveSpeakerNumber, type Interval, type SpeakerTurn } from "./coverage"
import { ProviderLink, type LinkKey } from "./link"
import type { AudioChannelName, LiveRepository, SegmentRelabel } from "./repository"
import { LiveProviderError } from "./providers/queue"
import { Timeline } from "./timeline"
import { normaliseText, normaliseWords, textScript } from "./text"
import { trace } from "./trace"
import { attributeWords, canonicalSpeaker, ME, MIC_OWNER_FLIP_MS, UNKNOWN, type Attribution, type AttributedTurn, type HeardWord } from "./speakers"

export interface LiveSocket {
  /** Bun semantics: a positive number when sent, -1 when queued behind back-pressure, 0 when dropped. */
  send(text: string): number
  close(code: number, reason?: string): void
}
export interface LiveConfig {
  /** A backlog longer than this is skipped by the client instead of replayed. */
  resumeWindowMs: number
  /** How long a session without a socket keeps its provider sessions. */
  detachMs: number
  /** How long a paused channel keeps its provider session. */
  pauseKeepMs: number
  /** Idle time after which the provider is sent a keep-alive. */
  keepAliveMs: number
  /** Chat may ask for a flush at most this often. */
  chatFlushIntervalMs: number
  /** How long chat waits for the flush to produce stored text. */
  chatFlushWaitMs: number
  /** How long Stop waits for the last finals. */
  endWaitMs: number
  /** An utterance this long is finalised at the next pause; at `forceCommitHardMs` regardless. */
  forceCommitMs: number
  forceCommitHardMs: number
  /** Silence this long is confirmed with the provider so coverage moves past it. */
  checkpointMs: number
  /** A channel whose audio stopped arriving this long ago (a stalled network, a sleeping laptop) has what it
   * sent made final, even mid-word. A keep-alive carries no audio (providers/elevenlabs-realtime.ts), and the
   * provider discarded an utterance left uncommitted through a five-minute stall (measured 2026-11-17). */
  stallCommitMs: number
  /** Audio the provider has not read; beyond this the provider session is dropped. */
  upstreamBufferMs: number
  /** Finals queued for a slow or absent client before it is told to re-read the note. */
  maxQueuedFinals: number
  /** How long the provider may stay unavailable before the client is sent back to parts. */
  providerGiveUpMs: number
  /** Delays between attempts to reopen a failed provider session. */
  reopenDelaysMs: ReadonlyArray<number>
  ackIntervalMs: number
  /** `status` is repeated this often while audio flows and nothing changes. */
  statusIntervalMs: number
  tickMs: number
  /** A Stop over HTTP waits until no audio frame has arrived for `stopIdleMs`, at most `stopGraceMs`. */
  stopIdleMs: number
  stopGraceMs: number
  /** How long the hub keeps the provider sessions of a session that ended in an orderly way (Stop, or
   * a connection lost for longer than `detachMs`), so a Resume within it keeps the provider's voices.
   * A held provider session is sent a keep-alive every `keepAliveMs`. 0 turns holding off.
   * Measured on 2026-11-14: the provider bills the audio sent, not the time connected, so a held
   * session costs the keep-alive silence only (0.1 s per 5 s, 2% of an open stream: 120 s held used
   * 1 credit where 60 s of streamed audio used 18). What a hold does use up is one of the account's
   * concurrent streaming sessions per channel. */
  holdMs: number
}
export const DEFAULT_LIVE_CONFIG: LiveConfig = {
  resumeWindowMs: 900_000, detachMs: 20_000, pauseKeepMs: 120_000, keepAliveMs: 5_000, chatFlushIntervalMs: 5_000, chatFlushWaitMs: 400, endWaitMs: 2_000,
  forceCommitMs: 10_000, forceCommitHardMs: 30_000, checkpointMs: 5_000, stallCommitMs: 15_000, upstreamBufferMs: 10_000, maxQueuedFinals: 200, providerGiveUpMs: 60_000,
  reopenDelaysMs: [1_000, 2_000, 4_000, 8_000, 15_000], ackIntervalMs: 1_000, statusIntervalMs: 2_000, tickMs: 500, stopIdleMs: 250, stopGraceMs: 1_500,
  holdMs: 300_000,
}
/** No language is forced: the provider detects it (with English forced, Japanese and short Spanish were dropped,
 * measured 2026-11-16, `.local/live-only/lang/`). Once a recording has this much speech and nearly all of it
 * (`LANGUAGE_LOCK_SHARE`) was detected as one language, a provider session opened for it later (after a drop
 * or a long pause) is told that language, so a short, quiet stretch is not taken for another one. Punctuation
 * the detection gets wrong is put right in text.ts. */
const LANGUAGE_LOCK_MS = 30_000, LANGUAGE_LOCK_SHARE = 0.8
/** After this much speech written in one script, a stretch shorter than `STRAY_LANGUAGE_MS` written in another is
 * dropped. By script, not by the language the provider reports: a mumble came back as "うん。" or "음." labelled
 * with the recording's own language, and was kept; while a short real sentence in another language that shares
 * the script ("Muchas gracias a todos." in an English meeting) was dropped (measured 2026-11-17,
 * `.local/ship/accuracy/`). */
const MAIN_LANGUAGE_MS = 3_000, STRAY_LANGUAGE_MS = 4_000
/** The close that hands a recording back to its client after its provider session was lost: the client
 * reconnects at once and replays its backlog from what is stored (1012, "service restart", which every
 * client already retries). */
export const RECONNECT_CLOSE = 1012
/** Below this amplitude a sample counts as a pause (about -36 dBFS). */
const QUIET_AMPLITUDE = 500
/** Microphone text is kept only where the microphone had sound at speech level (`QUIET_AMPLITUDE`) within this
 * much of its words. A real recogniser hears words ("Yeah.", "Right.") in the low hiss of a quiet room, and they
 * appeared as the note owner speaking while nobody did. The audio itself is sent as it is: replacing the quiet
 * with digital silence made the recogniser drop short words said on their own ("Okay."). System audio is never
 * filtered: what the call plays is there to be transcribed, however quiet. */
const MIC_SOUND_MARGIN_MS = 300
/** How long the record of where the microphone had sound is kept: finals come seconds after their audio. */
const MIC_SOUND_KEEP_MS = 10 * 60_000
/** A flush is only sent after this much quiet, so it does not cut a word in two. */
const PAUSE_MS = 150

/** Where the hub keeps provider sessions between two live sessions of a note. */
export interface LiveHold {
  /** A held provider session opened for exactly `key`, if there is one. It is the caller's from then on. */
  take(key: LinkKey): ProviderLink | undefined
  park(links: ReadonlyArray<ProviderLink>): void
  /** The newest audio a channel sent before its live session ended, for the next one (see `PRIMER_MS`). */
  keepAudio?(channel: AudioChannelName, audio: RecentAudio): void
  recentAudio?(channel: AudioChannelName): RecentAudio | undefined
}
/** PCM16 of a channel, `fromMs` on the note timeline being its first sample. */
export interface RecentAudio { readonly fromMs: number; readonly sampleRate: number; readonly pcm: Uint8Array
  /** A stretch of each voice's own stored speech (`VOICE_SAMPLE_MS`), for the primer. */
  readonly voices?: ReadonlyArray<VoiceSample> }
/** Audio of one voice of the note: its speaker number, and PCM16 from note-timeline `fromMs`. */
export interface VoiceSample { readonly number: number; readonly fromMs: number; readonly pcm: Uint8Array }
/** A new provider session on a channel that already has voices is first played the newest this-much of the
 * audio it already transcribed: its diariser names those voices again, and the stored segments say who they
 * are, so the new ids map onto the note's numbers instead of numbering on. Its text is not stored again. */
const PRIMER_MS = 20_000
/** The primer prefers a sample of each known voice, taken from the end of its newest stored segment: the newest
 * 20 s alone often held no stored voice at all (a provider session lost mid-sentence leaves that sentence
 * unstored, for the replay), and every voice then got a new number; system-audio attribution fell from 83% to
 * 53% over two provider drops (measured 2026-11-17, `.local/ship/accuracy/`). At most `PRIMER_VOICES` voices,
 * the most recently heard, `VOICE_SAMPLE_MS` each, half a second apart, the first of them played again last. */
const VOICE_SAMPLE_MS = 5_000, VOICE_SAMPLE_MIN_MS = 1_200, PRIMER_VOICES = 5, PRIMER_GAP_MS = 500
/** The longest an end waits for the provider to work through what it was sent. */
const SETTLE_MAX_MS = 60_000
interface Upstream {
  readonly id: string
  readonly link: ProviderLink
  readonly session: LiveTranscriberSession
  readonly timeline: Timeline
  readonly abort: AbortController
  /** Provider speaker to note speaker number, for this provider session only. */
  speakers: Record<string, number>
  /** Where the provider's clock stood, in samples, when this capture took the provider session over:
   * 0 unless it was held. Text from before it belongs to the earlier capture and is dropped. */
  readonly origin: number
  /** Voices of the primer audio (`PRIMER_MS`), and how long each new diariser id was heard over each. */
  primer?: { readonly voices: ReadonlyArray<{ fromMs: number; toMs: number; number: number }>; readonly votes: Map<string, Map<number, number>> }
  /** Who is who in this provider session (speakers.ts). */
  readonly attribution: Attribution
  /** Upstream samples already accounted for by finals. */
  finalThrough: number
  /** Provider utterance number to the ID the client knows the utterance by. */
  readonly utterances: Map<number | undefined, string>
  lastSendAt: number
  lastCommitAt: number
  /** Upstream position of the newest flush that was sent. */
  flushedThrough: number
  /** Flushes sent and not yet answered, and who is waiting for all of them to be. */
  pendingFlushes: number
  flushWatchers: Array<() => void>
}
interface Channel {
  readonly name: AudioChannelName
  readonly sampleRate: number
  readonly baseMs: number
  /** One past the last capture sample taken. */
  nextSample: number
  upstream?: Upstream
  opening?: Promise<void>
  interim?: { utteranceId: string; rev: number; text: string; startMs: number; endMs: number; speaker?: number }
  /** Note-timeline end of audio the live path has stored coverage for. */
  finalThroughMs: number
  quietSamples: number
  /** Where the channel had sound at speech level, as [fromMs, toMs] on the note's timeline, oldest first
   * (`MIC_SOUND_MARGIN_MS`). Kept for the microphone only. */
  sound: Array<[number, number]>
  /** Capture sample at which the most recent pause of at least 300 ms ended: nothing before it is waiting to become text. */
  quietThroughSample: number
  paused: boolean
  pausedAt: number
  /** Capture sample since which no provider session has been transcribing. */
  unavailableFrom?: number
  unavailableSince?: number
  reopenAttempt: number
  reopenTimer?: ReturnType<typeof setTimeout>
  ackedAt: number
  ackedSample: number
  statusAt: number
  statusSent?: LiveStreamState
  /** [wall ms, samples] of recent frames, to tell replay from real time. */
  pace: Array<[number, number]>
  /** When audio last arrived for this channel. */
  lastAudioAt: number
  /** The script and end of the channel's newest stored text, for the stray-script rule. */
  lastText?: { script: string; endMs: number }
  pendingInterim: boolean
  /** The newest `PRIMER_MS` of audio received, from capture sample `recentFrom`. */
  recent: Uint8Array[]
  recentFrom: number
  recentSamples: number
  /** System audio: a sample of each voice's speech, by speaker number (`VOICE_SAMPLE_MS`). */
  voices: Map<number, { fromMs: number; pcm: Uint8Array }>
}
export interface LiveTailItem { channel: AudioChannelName; text: string; startMs: number; speakerId?: string }
export interface LiveSnapshot { asOfMs: number; tail: LiveTailItem[]; state: "live" | "catching_up" | "provider_unavailable" }

export class LiveSession {
  readonly sessionId = crypto.randomUUID()
  readonly captureId: string
  private readonly channels = new Map<AudioChannelName, Channel>()
  private socket: LiveSocket | undefined
  private backpressured = false
  private finalsWhileBlocked = 0
  private queuedFinals: LiveServerMessage[] = []
  private queueOverflowed = false
  /** Serialises repository writes so revisions reach the client in order. */
  private writes: Promise<unknown> = Promise.resolve()
  private timer: ReturnType<typeof setInterval> | undefined
  private detachedAt: number | undefined
  private lastChatFlushAt = -Infinity
  /** Milliseconds of speech per detected language, for `lockedLanguage`. */
  private readonly languages = new Map<string, number>()
  /** Milliseconds of speech per script of its text (text.ts `textScript`), for `mainScript`. */
  private readonly scripts = new Map<string, number>()
  /** The microphone's labels by speaker number ("Me", "Room speaker N"), read from the note when a
   * microphone provider session opens and kept here (speakers.ts, rule 2). */
  private readonly micLabels = new Map<number, string>()
  /** Microphone segments stored as Me's although another diariser id said them, by provider session and
   * id: they move to that voice if it is trusted later. */
  private readonly heldAsMe = new Map<string, string[]>()
  private lastFrameAt = -Infinity
  private ending: Promise<void> | undefined
  ended = false

  private constructor(
    readonly noteId: string,
    private readonly hello: LiveHelloMessage,
    private readonly transcriber: LiveTranscriber,
    private readonly store: LiveRepository,
    private readonly config: LiveConfig,
    private readonly onEnded: (session: LiveSession) => void,
    private readonly transcriptRevision: () => Promise<number>,
    private readonly hold: LiveHold | undefined,
  ) { this.captureId = hello.captureId }

  /** Opens the provider sessions and decides where each channel resumes. Throws `LiveProviderError`
   * when no channel could be opened. */
  static async start(input: { noteId: string; hello: LiveHelloMessage; transcriber: LiveTranscriber; store: LiveRepository; config: LiveConfig; onEnded: (session: LiveSession) => void; transcriptRevision: () => Promise<number>; hold?: LiveHold }): Promise<LiveSession> {
    const session = new LiveSession(input.noteId, input.hello, input.transcriber, input.store, input.config, input.onEnded, input.transcriptRevision, input.hold)
    const ledger = await Effect.runPromise(input.store.ledger(input.noteId))
    const now = performance.now()
    for (const stream of input.hello.streams) {
      if (session.channels.has(stream.channel)) throw new LiveSessionRefusal(LiveCloseCode.malformed, "A channel is listed twice")
      // Audio this capture already had accounted for, by either path, is not asked for again. With no
      // record of the capture the client's own resume point stands.
      const known = ledger.filter(row => row.channel === stream.channel && row.toMs > stream.baseMs).map(row => row.toMs)
      const accept = known.length ? Math.floor((Math.max(...known) - stream.baseMs) * stream.sampleRate / 1000) : stream.resumeFromSample
      session.channels.set(stream.channel, { name: stream.channel, sampleRate: stream.sampleRate, baseMs: stream.baseMs, nextSample: accept, finalThroughMs: session.ms(stream, accept), quietSamples: 0, sound: [], quietThroughSample: accept,
        paused: false, pausedAt: 0, reopenAttempt: 0, ackedAt: now, ackedSample: accept, statusAt: 0, pace: [], lastAudioAt: now, pendingInterim: false, recent: [], recentFrom: accept, recentSamples: 0,
        // The voice samples of the note's previous live session carry over, so a voice heard only before a reconnect is still known.
        voices: new Map((input.hold?.recentAudio?.(stream.channel)?.sampleRate === stream.sampleRate ? input.hold.recentAudio(stream.channel)!.voices ?? [] : []).map(sample => [sample.number, { fromMs: sample.fromMs, pcm: sample.pcm }])) })
    }
    const results = await Promise.allSettled([...session.channels.values()].map(channel => session.open(channel)))
    if (results.every(result => result.status === "rejected")) {
      const reason = (results[0] as PromiseRejectedResult).reason
      await session.end()
      throw reason instanceof LiveProviderError ? reason : new LiveProviderError("unavailable", true)
    }
    for (const channel of session.channels.values()) if (!channel.upstream) session.unavailable(channel, true)
    session.timer = setInterval(() => session.tick(), input.config.tickMs)
    return session
  }

  // ------------------------------------------------------------------ socket

  get attached() { return this.socket !== undefined }
  /** A reconnect of the same capture must describe the same streams. */
  compatible(hello: LiveHelloMessage) {
    return hello.captureId === this.captureId && hello.streams.length === this.channels.size && hello.streams.every(stream => {
      const channel = this.channels.get(stream.channel)
      return channel && channel.sampleRate === stream.sampleRate && channel.baseMs === stream.baseMs
    })
  }
  /** Hands the session to a socket (the first one, or a reconnect) and tells it where to continue. */
  async attach(socket: LiveSocket) {
    const previous = this.socket
    this.socket = socket; this.detachedAt = undefined; this.backpressured = false; this.finalsWhileBlocked = 0
    if (previous && previous !== socket) closeSocket(previous, LiveCloseCode.replaced, "replaced", "Another connection took over this recording", false)
    const revision = await this.transcriptRevision().catch(() => 0)
    trace("srv_ready", this.noteId, this.sessionId)
    this.post({ type: "ready", sessionId: this.sessionId, provider: this.transcriber.provider, model: this.transcriber.model,
      streams: [...this.channels.values()].map(channel => ({ channel: channel.name, acceptFromSample: channel.nextSample })), resumeWindowMs: this.config.resumeWindowMs, transcriptRevision: revision })
    // Finals stored while no socket was attached. If there were too many, the revision in `ready`
    // already tells the client to read the note again.
    const queued = this.queuedFinals; this.queuedFinals = []
    if (!this.queueOverflowed) for (const message of queued) this.post(message)
    this.queueOverflowed = false
    for (const channel of this.channels.values()) { channel.statusSent = undefined; this.status(channel, true) }
  }
  detach(socket: LiveSocket) {
    if (this.socket !== socket) return
    this.socket = undefined; this.detachedAt = performance.now()
  }
  drained(socket: LiveSocket) {
    if (this.socket !== socket) return
    this.backpressured = false; this.finalsWhileBlocked = 0
    for (const channel of this.channels.values()) if (channel.pendingInterim) { channel.pendingInterim = false; this.postInterim(channel) }
  }
  private post(message: LiveServerMessage) {
    const droppable = message.type === "interim" || message.type === "ack" || message.type === "status"
    if (!this.socket) {
      if (message.type === "final" || message.type === "gap" || message.type === "speakers") {
        if (this.queuedFinals.length >= this.config.maxQueuedFinals) { this.queueOverflowed = true; this.queuedFinals = [] } else if (!this.queueOverflowed) this.queuedFinals.push(message)
      }
      return
    }
    // A slow client gets the newest interim once it drains; nothing it needs is dropped.
    if (this.backpressured && droppable) return
    const sent = this.socket.send(JSON.stringify(message))
    if (sent === 0) {
      // The connection is gone but its close has not been reported yet: keep what a reconnect needs.
      this.socket = undefined; this.detachedAt = performance.now()
      this.post(message)
    } else if (sent === -1) {
      this.backpressured = true
      if (!droppable && ++this.finalsWhileBlocked > this.config.maxQueuedFinals) {
        const socket = this.socket
        this.socket = undefined; this.detachedAt = performance.now()
        closeSocket(socket, LiveCloseCode.clientTooSlow, "client_too_slow", "The connection is too slow for live transcription", true)
      }
    }
  }

  // ------------------------------------------------------------------ audio in

  private ms(stream: { baseMs: number; sampleRate: number }, sample: number) { return stream.baseMs + Math.round(sample * 1000 / stream.sampleRate) }
  audio(frame: LiveAudioFrame) {
    const channel = this.channels.get(frame.channel)
    if (!channel || this.ended || this.ending) return
    let { firstSample, pcm } = frame, samples = pcm.byteLength >> 1
    const now = performance.now()
    this.lastFrameAt = now
    // Samples the server already has are dropped; nothing depends on arrival order.
    if (firstSample + samples <= channel.nextSample) { this.acknowledge(channel, now); return }
    if (firstSample < channel.nextSample) { const skip = channel.nextSample - firstSample; pcm = pcm.subarray(skip * 2); firstSample = channel.nextSample; samples -= skip }
    if (channel.paused) { channel.paused = false; if (!channel.upstream && !channel.opening && !channel.reopenTimer) this.reopen(channel, 0) }
    if (firstSample > channel.nextSample) {
      // The client skipped ahead: a declared gap. What was heard before it is finalised first.
      if (channel.upstream && channel.upstream.timeline.upstreamSamples > channel.upstream.finalThrough) void this.commit(channel, channel.upstream, now)
      this.gap(channel.name, this.ms(channel, channel.nextSample), this.ms(channel, firstSample), "resume_window_exceeded")
      channel.nextSample = firstSample
      // Nothing before the skip is waiting to become text (see `lag`).
      channel.quietThroughSample = firstSample
    }
    const view = new DataView(pcm.buffer, pcm.byteOffset, pcm.byteLength)
    let quiet = channel.quietSamples + samples
    for (let index = samples - 1; index >= 0; index--) if (Math.abs(view.getInt16(index * 2, true)) >= QUIET_AMPLITUDE) { quiet = samples - 1 - index; break }
    // Speech resuming after a pause: the silence before it is not speech waiting for text (see `lag`).
    if (quiet < samples && channel.quietSamples >= channel.sampleRate * 0.3) channel.quietThroughSample = firstSample + Math.max(0, samples - quiet - 1)
    channel.quietSamples = quiet
    if (channel.name === "mic" && quiet < samples) this.heardSound(channel, this.ms(channel, firstSample), this.ms(channel, firstSample + samples))
    const upstream = channel.upstream
    if (upstream) {
      upstream.timeline.audio(firstSample, samples)
      trace("srv_first_audio_upstream", this.noteId, this.sessionId)
      upstream.session.send(pcm)
      upstream.lastSendAt = now
    } else channel.unavailableFrom ??= firstSample
    channel.nextSample = firstSample + samples
    if (firstSample !== channel.recentFrom + channel.recentSamples) { channel.recent = []; channel.recentFrom = firstSample; channel.recentSamples = 0 }
    channel.recent.push(pcm.slice()); channel.recentSamples += samples
    while (channel.recent.length > 1 && channel.recentSamples - (channel.recent[0]!.byteLength >> 1) >= PRIMER_MS * channel.sampleRate / 1000) { const dropped = channel.recent.shift()!.byteLength >> 1; channel.recentFrom += dropped; channel.recentSamples -= dropped }
    channel.pace.push([now, samples])
    channel.lastAudioAt = now
    while (channel.pace.length > 1 && now - channel.pace[0]![0] > 2000) channel.pace.shift()
    if (upstream) this.pace(channel, upstream, now)
    this.acknowledge(channel, now)
    this.status(channel)
  }
  /** Decides when to ask the provider to finalise, and notices a provider that stopped reading. */
  private pace(channel: Channel, upstream: Upstream, now: number) {
    if (upstream.session.bufferedBytes > channel.sampleRate * 2 * 1.4 * this.config.upstreamBufferMs / 1000) { this.failed(channel, upstream, { type: "error", retryable: true, reason: "unavailable" }); return }
    // A flush the provider never answers must not block the next one for ever.
    if (upstream.pendingFlushes && now - upstream.lastCommitAt > 3000) this.answered(upstream, true)
    if (upstream.pendingFlushes || now - upstream.lastCommitAt < 2000) return
    const pendingMs = (upstream.timeline.upstreamSamples - upstream.finalThrough) * 1000 / channel.sampleRate, quietMs = channel.quietSamples * 1000 / channel.sampleRate
    // Audio stopped arriving part-way through something (see `stallCommitMs`).
    // Only with a socket attached (a laptop asleep holds its connection): without one, the session ends after
    // `detachMs` and stores what it has, and a reconnect before that resumes the word where it stopped.
    const stalled = pendingMs > 0 && !channel.paused && this.socket !== undefined && now - channel.lastAudioAt >= this.config.stallCommitMs
    if (channel.interim?.text) { if (pendingMs >= this.config.forceCommitHardMs || pendingMs >= this.config.forceCommitMs && quietMs >= PAUSE_MS || stalled) this.commit(channel, upstream, now) }
    // Nothing is being said: have the provider confirm the silence, so coverage (where a reconnect
    // resumes) moves past it.
    else if (pendingMs >= this.config.checkpointMs && quietMs >= 500 || stalled) this.commit(channel, upstream, now)
  }
  private commit(_channel: Channel | undefined, upstream: Upstream, now: number) {
    upstream.lastCommitAt = now
    upstream.flushedThrough = upstream.timeline.upstreamSamples
    upstream.pendingFlushes++
    upstream.session.flush()
    return this.flushed(upstream)
  }
  /** Resolves once every flush sent so far has been answered. */
  private flushed(upstream: Upstream) {
    return upstream.pendingFlushes ? new Promise<void>(resolve => { upstream.flushWatchers.push(resolve) }) : Promise.resolve()
  }
  private answered(upstream: Upstream, all = false) {
    upstream.pendingFlushes = all ? 0 : Math.max(0, upstream.pendingFlushes - 1)
    if (!upstream.pendingFlushes) for (const resolve of upstream.flushWatchers.splice(0)) resolve()
  }
  private acknowledge(channel: Channel, now: number, force = false) {
    if (!force && (now - channel.ackedAt < this.config.ackIntervalMs || channel.ackedSample === channel.nextSample)) return
    channel.ackedAt = now; channel.ackedSample = channel.nextSample
    this.post({ type: "ack", channel: channel.name, throughSample: channel.nextSample, transcribedThroughMs: channel.finalThroughMs })
  }
  private state(channel: Channel): LiveStreamState {
    if (channel.paused) return "paused"
    if (!channel.upstream) return "provider_unavailable"
    const first = channel.pace[0], last = channel.pace.at(-1)
    if (first && last && last[0] - first[0] >= 500) {
      const audioMs = channel.pace.slice(1).reduce((sum, item) => sum + item[1], 0) * 1000 / channel.sampleRate
      if (audioMs > (last[0] - first[0]) * 1.3) return "catching_up"
    }
    return "live"
  }
  private status(channel: Channel, force = false) {
    const state = this.state(channel), now = performance.now()
    if (!force && state === channel.statusSent && now - channel.statusAt < this.config.statusIntervalMs) return
    if (!force && state === channel.statusSent && !channel.pace.length) return
    channel.statusSent = state; channel.statusAt = now
    this.post({ type: "status", channel: channel.name, state, lagMs: this.lag(channel) })
  }
  /** The recogniser's delay: speech the server has received that is in neither interim nor final text
   * yet. Quiet is not speech, whether it trails the last words or precedes the next ones, and an
   * utterance still in progress is not lag (its words are in the interim text), so this stays small
   * during a monologue, a silence, the first words after a silence, and a replay. While no provider
   * session is transcribing, it is all the audio received since it stopped. */
  private lag(channel: Channel) {
    const received = this.ms(channel, channel.nextSample)
    if (!channel.upstream) return Math.max(0, received - this.ms(channel, channel.unavailableFrom ?? channel.nextSample))
    return Math.max(0, received - Math.round(channel.quietSamples * 1000 / channel.sampleRate) - Math.max(channel.finalThroughMs, channel.interim?.endMs ?? 0, this.ms(channel, channel.quietThroughSample)))
  }
  mark(mark: LiveMarkMessage) {
    if (this.ended || this.ending) return
    if (mark.kind === "end") { trace("srv_end_mark", this.noteId, this.sessionId); void this.end(); return }
    const now = performance.now()
    for (const channel of this.channels.values()) {
      if (mark.atSample[channel.name] === undefined && Object.keys(mark.atSample).length) continue
      if (mark.kind === "pause") {
        // The last words become final now; the provider session is kept for a while (see tick).
        channel.paused = true; channel.pausedAt = now; channel.pace = []
        if (channel.upstream && channel.upstream.timeline.upstreamSamples > channel.upstream.finalThrough) void this.commit(channel, channel.upstream, now)
      } else {
        channel.paused = false
        if (!channel.upstream && !channel.opening && !channel.reopenTimer) this.reopen(channel, 0)
      }
      this.status(channel, true)
    }
  }

  // ------------------------------------------------------------------ provider

  /** The audio for a new provider session's primer and the voices in it: a sample of each known voice (this live
   * session's, or the one before it, kept by the hub), else the newest `PRIMER_MS` before where this channel goes
   * on and the stored voices in it. Pieces are played in order, `gapSamples` of silence after each. */
  private async primer(channel: Channel): Promise<{ pieces: Array<{ firstSample: number; pcm: Uint8Array }>; gapSamples: number; voices: Array<{ fromMs: number; toMs: number; number: number }> } | undefined> {
    const rate = channel.sampleRate, kept = this.hold?.recentAudio?.(channel.name)
    const samples = channel.voices.size ? [...channel.voices.entries()].map(([number, sample]) => ({ number, ...sample })) : kept?.sampleRate === rate ? [...kept.voices ?? []] : []
    if (samples.length) {
      const newest = samples.sort((a, b) => b.fromMs - a.fromMs).slice(0, PRIMER_VOICES).sort((a, b) => a.fromMs - b.fromMs)
      // The first sample once more at the end: a new provider session names no speaker in its first utterance (the
      // diariser has not formed a voice yet, speakers.ts), so the first voice played would otherwise get no votes.
      const chosen = newest.length > 1 ? [...newest, newest[0]!] : newest
      return { gapSamples: Math.round(PRIMER_GAP_MS * rate / 1000),
        pieces: chosen.map(sample => ({ firstSample: Math.round((sample.fromMs - channel.baseMs) * rate / 1000), pcm: sample.pcm })),
        voices: chosen.map(sample => ({ fromMs: sample.fromMs, toMs: sample.fromMs + Math.round((sample.pcm.byteLength >> 1) * 1000 / rate), number: sample.number })) }
    }
    const own = channel.recentSamples ? { fromMs: this.ms(channel, channel.recentFrom), sampleRate: rate, pcm: concat(channel.recent) } : kept
    if (!own || own.sampleRate !== rate) return undefined
    const firstSample = Math.round((own.fromMs - channel.baseMs) * rate / 1000)
    // Only audio before where the stream goes on, so the provider's clock and the capture stay in step.
    const usable = Math.min(own.pcm.byteLength >> 1, channel.nextSample - firstSample)
    if (firstSample < 0 || usable < rate * 2) return undefined
    const pcm = own.pcm.subarray(0, usable * 2), fromMs = this.ms(channel, firstSample), toMs = this.ms(channel, firstSample + usable)
    const voices = await Effect.runPromise(this.store.voicesBetween(this.noteId, channel.name, fromMs, toMs)).catch(() => [])
    return voices.length ? { pieces: [{ firstSample, pcm }], gapSamples: 0, voices: [...voices] } : undefined
  }
  /** Keeps the end of each newly stored system-audio segment as its voice's sample, while the audio is still at hand. */
  private sampleVoices(channel: Channel, segments: ReadonlyArray<TranscriptSegmentDTO>) {
    const rate = channel.sampleRate
    for (const segment of segments) {
      const number = liveSpeakerNumber("system", segment.speakerId), endMs = segment.endMs ?? segment.startMs
      if (number === undefined || endMs - segment.startMs < VOICE_SAMPLE_MIN_MS) continue
      const fromMs = Math.max(segment.startMs, endMs - VOICE_SAMPLE_MS)
      const from = Math.round((fromMs - channel.baseMs) * rate / 1000), to = Math.round((endMs - channel.baseMs) * rate / 1000)
      if (from < channel.recentFrom || to > channel.recentFrom + channel.recentSamples || to <= from) continue
      const pcm = new Uint8Array((to - from) * 2)
      let at = channel.recentFrom, written = 0
      for (const piece of channel.recent) {
        const count = piece.byteLength >> 1, start = Math.max(from, at), end = Math.min(to, at + count)
        if (end > start) { pcm.set(piece.subarray((start - at) * 2, (end - at) * 2), (start - from) * 2); written += end - start }
        at += count
      }
      if (written === to - from) channel.voices.set(number, { fromMs, pcm })
    }
  }
  /** The language nearly all of this recording's speech so far was detected as, once there is enough of it. */
  private lockedLanguage(): string | undefined {
    const total = [...this.languages.values()].reduce((sum, ms) => sum + ms, 0)
    const top = [...this.languages.entries()].sort((a, b) => b[1] - a[1])[0]
    return top && total >= LANGUAGE_LOCK_MS && top[1] / total >= LANGUAGE_LOCK_SHARE ? top[0] : undefined
  }

  /** The script this recording's text is plainly written in so far: a few seconds of speech, nearly all of it one script. */
  private mainScript(): string | undefined {
    const total = [...this.scripts.values()].reduce((sum, ms) => sum + ms, 0)
    const top = [...this.scripts.entries()].sort((a, b) => b[1] - a[1])[0]
    return top && total >= MAIN_LANGUAGE_MS && top[1] / total >= LANGUAGE_LOCK_SHARE ? top[0] : undefined
  }

  private async open(channel: Channel) {
    const abort = new AbortController()
    const opening = (async () => {
      // The key names the client's language only, so a held provider session still fits once a language is locked.
      const key: LinkKey = { provider: this.transcriber.provider, model: this.transcriber.model, channel: channel.name, sampleRate: channel.sampleRate, ...(this.hello.language ? { language: this.hello.language } : {}) }
      const language = this.hello.language ?? this.lockedLanguage()
      // A provider session the hub held since this note's previous live session knows the voices already.
      const held = this.hold?.take(key)
      const link = held ?? new ProviderLink(await this.transcriber.open({ channel: channel.name, sampleRate: channel.sampleRate, ...(language ? { language } : {}), signal: abort.signal }), abort, key)
      trace(held ? "srv_provider_held" : "srv_provider_open", this.noteId, this.sessionId)
      if (this.ended) { await link.close(); return }
      const now = performance.now(), timeline = new Timeline(channel.sampleRate)
      // The provider's clock goes on from where the earlier capture left it; nothing before is this capture's audio.
      if (link.sent) timeline.silence(channel.nextSample, link.sent)
      const earlier = held ? [] : await Effect.runPromise(this.store.voices(this.noteId, channel.name)).catch(() => [] as ReadonlyArray<number>)
      // System audio with voices already: play the primer first, so the new ids can be mapped (`PRIMER_MS`).
      const primer = !held && channel.name === "system" && earlier.length ? await this.primer(channel) : undefined
      const primerAudio = primer ? concat(primer.pieces.flatMap(piece => [piece.pcm, new Uint8Array(primer.gapSamples * 2)])) : undefined
      for (const piece of primer?.pieces ?? []) { timeline.audio(piece.firstSample, piece.pcm.byteLength >> 1); if (primer!.gapSamples) timeline.silence(piece.firstSample + (piece.pcm.byteLength >> 1), primer!.gapSamples) }
      const origin = link.sent + (primerAudio ? primerAudio.byteLength >> 1 : 0)
      const upstream: Upstream = { id: crypto.randomUUID(), link, session: link.session, timeline, abort: link.abort, speakers: { ...link.speakers }, origin, attribution: link.attribution, finalThrough: origin, utterances: new Map(), lastSendAt: now, lastCommitAt: now, flushedThrough: origin, pendingFlushes: 0, flushWatchers: [],
        ...(primer ? { primer: { voices: primer.voices, votes: new Map() } } : {}) }
      if (primerAudio) { link.session.send(primerAudio); link.session.flush(); upstream.pendingFlushes++ }
      if (!held) {
        // A new provider session numbers its voices afresh; the note's earlier voices carry over as speakers.ts describes.
        if (channel.name === "mic" && earlier.length) {
          // Me carries over: the voice the note labels "Me" (the one that has spoken most).
          const labels = await Effect.runPromise(this.store.voiceLabels(this.noteId, "mic")).catch(() => ({} as Record<number, string>))
          for (const [number, label] of Object.entries(labels)) this.micLabels.set(Number(number), label)
          const owner = Number(Object.entries(labels).find(([, label]) => label === "Me")?.[0] ?? earlier[0])
          upstream.speakers = { ...upstream.speakers, [ME]: owner }
        }
        else if (channel.name === "system" && earlier.length === 1 && !primer) { upstream.speakers = { ...upstream.speakers, [UNKNOWN]: earlier[0]! }; upstream.attribution.placeholder = true }
      }
      channel.upstream = upstream
      // Records where this channel's live coverage starts, before any text exists (a reconnect resumes from the ledger).
      await Effect.runPromise(this.store.begin(this.noteId, this.sessionId, channel.name, this.ms(channel, channel.nextSample))).catch(() => false)
      for (const merge of link.own(event => this.event(channel, upstream, event))) await this.merge(channel, upstream, merge)
      // An utterance the earlier capture left unfinished is committed now, and dropped as it arrives (see `origin`).
      if (held) void this.commit(channel, upstream, now)
    })()
    channel.opening = opening
    try { await opening } finally { if (channel.opening === opening) channel.opening = undefined }
  }
  /** One provider event, or `null` when the provider session is over (link.ts calls this in order). */
  private async event(channel: Channel, upstream: Upstream, event: LiveTranscriberEvent | null) {
    if (event === null) { if (channel.upstream === upstream && !this.ended && !this.ending) this.failed(channel, upstream, { type: "error", retryable: true, reason: "unavailable" }); return }
    if (event.type === "error") this.failed(channel, upstream, event)
    else if (event.type === "interim") this.interim(channel, upstream, event)
    else if (this.ended) return
    else if (event.type === "final") await this.final(channel, upstream, event)
    else await this.merge(channel, upstream, event)
  }
  /** A frame of the microphone with sound at speech level, from `fromMs` to `toMs` (`MIC_SOUND_MARGIN_MS`). */
  private heardSound(channel: Channel, fromMs: number, toMs: number) {
    const last = channel.sound.at(-1)
    if (last && fromMs <= last[1] + 20) last[1] = Math.max(last[1], toMs)
    else channel.sound.push([fromMs, toMs])
    while (channel.sound.length > 1 && channel.sound[0]![1] < toMs - MIC_SOUND_KEEP_MS) channel.sound.shift()
  }
  /** Whether text from `fromMs` to `toMs` on this channel can have been said: always on system audio; on the
   * microphone, only with sound at speech level within `MIC_SOUND_MARGIN_MS` of it. */
  private audible(channel: Channel, fromMs: number, toMs: number) {
    if (channel.name !== "mic") return true
    const from = fromMs - MIC_SOUND_MARGIN_MS, to = toMs + MIC_SOUND_MARGIN_MS
    return channel.sound.some(([start, end]) => start <= to && end >= from)
  }
  private interim(channel: Channel, upstream: Upstream, event: Extract<LiveTranscriberEvent, { type: "interim" }>) {
    if (channel.upstream !== upstream || event.endSec * channel.sampleRate <= upstream.origin && event.text.trim()) return
    const text = normaliseText(event.text.trim())
    if (!text && !channel.interim) return
    if (text) trace("srv_first_interim", this.noteId, this.sessionId)
    let utteranceId = upstream.utterances.get(event.utterance)
    if (!utteranceId) { if (!text) return; utteranceId = crypto.randomUUID(); upstream.utterances.set(event.utterance, utteranceId) }
    const previous = channel.interim?.utteranceId === utteranceId ? channel.interim : undefined
    // The draft speaker, under the key its words will be stored under (speakers.ts): "Me" on the microphone.
    const key = canonicalSpeaker(channel.name, upstream.attribution, event.speaker)
    const speaker = key === undefined ? previous?.speaker : upstream.speakers[key] ?? previous?.speaker
    const startMs = this.ms(channel, upstream.timeline.captureSample(event.startSec)), endMs = Math.max(startMs, this.ms(channel, upstream.timeline.captureSample(event.endSec)))
    if (previous && previous.text === text && previous.speaker === speaker) return
    // Words heard in a quiet microphone's hiss are not shown, even as a draft (`MIC_SOUND_MARGIN_MS`).
    if (text && !this.audible(channel, startMs, endMs)) return
    channel.interim = { utteranceId, rev: previous ? previous.rev + 1 : 0, text, startMs, endMs, ...(speaker === undefined ? {} : { speaker }) }
    this.postInterim(channel)
  }
  private postInterim(channel: Channel) {
    const interim = channel.interim
    if (!interim) return
    if (this.backpressured) { channel.pendingInterim = true; return }
    this.post({ type: "interim", channel: channel.name, utteranceId: interim.utteranceId, rev: interim.rev, text: interim.text, startMs: interim.startMs, endMs: interim.endMs,
      ...(interim.speaker === undefined ? {} : { speakerId: liveSpeakerId(channel.name, interim.speaker), speakerLabel: this.label(channel.name, interim.speaker) }) })
    if (!interim.text) channel.interim = undefined
  }
  private async final(channel: Channel, upstream: Upstream, event: Extract<LiveTranscriberEvent, { type: "final" }>) {
    const { timeline } = upstream, rate = channel.sampleRate
    // Never less than the end of the words being stored: text without coverage would be transcribed again.
    const through = Math.max(upstream.finalThrough, Math.min(timeline.upstreamSamples, Math.round(Math.max(event.finalThroughSec, event.words?.at(-1)?.endSec ?? 0) * rate)))
    const coverage: Interval[] = timeline.captureRanges(upstream.finalThrough, through).map(([from, to]) => ({ fromMs: this.ms(channel, from), toMs: this.ms(channel, to) }))
    upstream.finalThrough = through
    const word = (text: string, startSec: number, endSec: number, speaker?: string): HeardWord => { const startMs = this.ms(channel, timeline.captureSample(startSec)); return { text, startMs, endMs: Math.max(startMs, this.ms(channel, timeline.captureSample(endSec))), ...(speaker === undefined ? {} : { speaker }) } }
    const heard: HeardWord[] = []
    if (event.words?.length) for (const item of event.words) {
      if (item.endSec * rate > upstream.origin) { heard.push(word(item.text, item.startSec, item.endSec, item.speaker)); continue }
      // A primer word: who it is, by the stored segment at its time.
      const primer = upstream.primer, at = word(item.text, item.startSec, item.endSec)
      const voice = primer && item.speaker !== undefined ? primer.voices.find(entry => (at.startMs + at.endMs) / 2 >= entry.fromMs && (at.startMs + at.endMs) / 2 < entry.toMs) : undefined
      if (primer && voice) { const tally = primer.votes.get(item.speaker!) ?? new Map<number, number>(); tally.set(voice.number, (tally.get(voice.number) ?? 0) + Math.max(1, at.endMs - at.startMs)); primer.votes.set(item.speaker!, tally) }
    }
    else if (event.text.trim() && event.endSec * rate > upstream.origin) heard.push(word(normaliseText(event.text.trim()), event.startSec, event.endSec))
    this.mapPrimerVoices(upstream)
    // Words heard in a quiet microphone's hiss, with no sound at speech level near their middle, were never said
    // (`MIC_SOUND_MARGIN_MS`). A real word's middle is in its sound; a soft one inside speech is within the margin
    // of the words around it.
    heard.splice(0, heard.length, ...heard.filter(item => { const middle = (item.startMs + item.endMs) / 2; return this.audible(channel, middle, middle) }))
    let spokenMs = heard.length ? heard.at(-1)!.endMs - heard[0]!.startMs : 0
    // A mumble or a filler in a recording that is plainly in one language is sometimes detected as another and
    // written in its script ("uh… hey hey hey" as Chinese, "hmm" as "うん。"). A short stretch in another script is
    // dropped; a long one is kept, and so is a short one in the same script (a Spanish "gracias" in English).
    // Not when a flush cut it (a pause, a stall, Stop: how long it would have been is unknown), nor when it goes
    // on from the channel's previous final in the same script (a passage the provider committed in pieces).
    const script = textScript(heard.map(item => item.text).join("")), main = this.mainScript()
    const continues = heard.length > 0 && channel.lastText?.script === script && heard[0]!.startMs - channel.lastText!.endMs < 2000
    if (script && main && script !== main && spokenMs < STRAY_LANGUAGE_MS && !event.flushed && !continues) { heard.length = 0; spokenMs = 0 }
    if (script && heard.length) channel.lastText = { script, endMs: heard.at(-1)!.endMs }
    if (script && spokenMs > 0) this.scripts.set(script, (this.scripts.get(script) ?? 0) + spokenMs)
    if (event.language && spokenMs > 0) this.languages.set(event.language, (this.languages.get(event.language) ?? 0) + spokenMs)
    heard.splice(0, heard.length, ...normaliseWords(heard))
    const speakers = { ...upstream.speakers }
    const attributed = heard.length ? attributeWords(channel.name, upstream.attribution, heard, speakers) : { turns: [] as AttributedTurn[], trusted: [] as string[] }
    const turns = attributed.turns
    upstream.speakers = speakers
    // Not while the provider is still answering for the primer: its later finals need the primer's anchors to say
    // when each word was spoken (pruned at the first one, only that final's words were ever matched to a voice).
    if (!upstream.primer || event.finalThroughSec * rate >= upstream.origin) timeline.prune(through)
    const utteranceId = upstream.utterances.get(event.utterance) ?? crypto.randomUUID()
    upstream.utterances.delete(event.utterance)
    const stored = await this.write(() => this.append(channel, upstream, turns, coverage))
    if (channel.interim?.utteranceId === utteranceId) channel.interim = undefined
    if (event.flushed !== false) this.answered(upstream)
    if (stored === "gone") { void this.end(LiveCloseCode.notFound, "not_found", "The note is no longer available"); return }
    if (stored === "failed") {
      // Not stored, so not covered: the client is told this stretch has no text.
      for (const interval of coverage) this.gap(channel.name, interval.fromMs, interval.toMs, "provider_unavailable")
      return
    }
    for (const interval of coverage) channel.finalThroughMs = Math.max(channel.finalThroughMs, interval.toMs)
    for (const segment of stored.segments) this.post({ type: "final", utteranceId, transcriptRevision: stored.transcriptRevision, segment })
    this.acknowledge(channel, performance.now(), stored.segments.length > 0)
    if (channel.name === "system") this.sampleVoices(channel, stored.segments)
    if (channel.name === "mic" && stored.segments.length) await this.settleMicVoices(channel, upstream, turns, attributed.trusted, stored.segments)
  }
  /** Gives each diariser id heard over the primer the note's number it was heard over most (at least 60% of
   * its primer speech), one number per id. */
  private mapPrimerVoices(upstream: Upstream) {
    const primer = upstream.primer
    if (!primer?.votes.size) return
    const taken = new Set(Object.values(upstream.speakers))
    const choices = [...primer.votes.entries()].flatMap(([key, tally]) => {
      const total = [...tally.values()].reduce((sum, ms) => sum + ms, 0), [number, ms] = [...tally.entries()].sort((a, b) => b[1] - a[1])[0]!
      return upstream.speakers[key] === undefined && ms / total >= 0.6 ? [{ key, number, ms }] : []
    }).sort((a, b) => b.ms - a.ms)
    const speakers = { ...upstream.speakers }
    for (const choice of choices) if (!taken.has(choice.number)) { speakers[choice.key] = choice.number; taken.add(choice.number) }
    upstream.speakers = speakers
    primer.votes.clear()
  }
  /** Tells the client about a stretch that will have no transcript, and stores it so the note shows it. */
  private gap(channel: AudioChannelName, fromMs: number, toMs: number, reason: "resume_window_exceeded" | "provider_unavailable") {
    this.post({ type: "gap", channel, fromMs, toMs, reason })
    if (toMs > fromMs) void Effect.runPromise(this.store.recordGap(this.noteId, channel, fromMs, toMs, reason)).catch(() => undefined)
  }
  private label(channel: AudioChannelName, number: number) { return channel === "mic" ? this.micLabels.get(number) ?? liveSpeakerLabel("mic", number) : liveSpeakerLabel("system", number) }
  /** Rule 2 of speakers.ts after a microphone final is stored: words of a newly trusted voice move to it, and
   * "Me" moves to the voice that has spoken most. Both change stored segments, and the client is told. */
  private async settleMicVoices(channel: Channel, upstream: Upstream, turns: ReadonlyArray<AttributedTurn>, trusted: ReadonlyArray<string>, segments: ReadonlyArray<TranscriptSegmentDTO>) {
    const state = upstream.attribution, numberOf = (key: string) => upstream.speakers[key]
    // Which turn each stored segment came from: they are made in turn order, each starting at its first word.
    for (const segment of segments) {
      const turn = turns.find(item => item.words.length && Math.max(0, Math.round(item.words[0]!.startMs)) === segment.startMs && numberOf(item.speakerKey) !== undefined && liveSpeakerId("mic", numberOf(item.speakerKey)!) === segment.speakerId)
      if (turn && turn.speakerKey === ME && turn.source !== state.owner && turn.source !== UNKNOWN) {
        const key = `${upstream.id}:${turn.source}`
        this.heldAsMe.set(key, [...(this.heldAsMe.get(key) ?? []), segment.id])
      }
    }
    const before = new Map(this.micLabels)
    // "Me" is the voice that has spoken most, by a clear margin.
    const owner = state.owner && state.evidence.get(state.owner)
    const rival = [...state.trusted].filter(key => key !== state.owner).map(key => ({ key, ms: state.evidence.get(key)?.ms ?? 0 })).sort((a, b) => b.ms - a.ms)[0]
    if (owner && rival && rival.ms >= MIC_OWNER_FLIP_MS && rival.ms > owner.ms * 1.25 && numberOf(rival.key) !== undefined && numberOf(ME) !== undefined) {
      const formerOwner = state.owner!, formerNumber = numberOf(ME)!, newNumber = numberOf(rival.key)!
      upstream.speakers = { ...upstream.speakers, [ME]: newNumber, [formerOwner]: formerNumber }
      state.trusted.add(formerOwner); state.trusted.delete(rival.key); state.owner = rival.key
    }
    const numbers = [...new Set([...this.micLabels.keys(), ...Object.values(upstream.speakers)])].sort((a, b) => a - b), me = numberOf(ME) ?? numbers[0]
    let room = 0
    for (const number of numbers) this.micLabels.set(number, number === me ? "Me" : `Room speaker ${++room}`)
    const updates: SegmentRelabel[] = []
    // Words a newly trusted voice said while it was still taken for Me.
    for (const key of trusted) {
      const number = numberOf(key), held = this.heldAsMe.get(`${upstream.id}:${key}`) ?? []
      this.heldAsMe.delete(`${upstream.id}:${key}`)
      if (number !== undefined) for (const segmentId of held) updates.push({ segmentId, speakerId: liveSpeakerId("mic", number), speakerLabel: this.label("mic", number) })
    }
    // Labels that changed: every stored segment of that voice.
    for (const number of numbers) {
      const was = before.get(number) ?? liveSpeakerLabel("mic", number)
      if (was === this.micLabels.get(number)) continue
      const ids = await Effect.runPromise(this.store.segmentsOf(this.noteId, "mic", number)).catch(() => [] as ReadonlyArray<string>)
      for (const segmentId of ids) if (!updates.some(update => update.segmentId === segmentId)) updates.push({ segmentId, speakerId: liveSpeakerId("mic", number), speakerLabel: this.micLabels.get(number)! })
    }
    if (!updates.length) return
    const applied = await this.write(() => Effect.runPromise(this.store.relabel(this.noteId, updates)).catch(() => [] as ReadonlyArray<SegmentRelabel>))
    if (applied.length) this.post({ type: "speakers", merges: [], relabels: applied.map(update => ({ ...update })) })
  }
  private async append(channel: Channel, upstream: Upstream, turns: ReadonlyArray<SpeakerTurn>, coverage: ReadonlyArray<Interval>) {
    for (let attempt = 0; attempt < 2; attempt++) {
      try {
        const result = await Effect.runPromise(this.store.append({ noteId: this.noteId, sessionId: this.sessionId, channel: channel.name, turns, speakers: upstream.speakers, coverage,
          ...(channel.name === "mic" ? { labels: Object.fromEntries(this.micLabels) } : {}) }))
        if (!result) return "gone" as const
        upstream.speakers = { ...result.speakers }
        return result
      } catch { await new Promise(resolve => setTimeout(resolve, 200)) }
    }
    return "failed" as const
  }
  private async merge(channel: Channel, upstream: Upstream, event: Extract<LiveTranscriberEvent, { type: "speakers_merged" }>) {
    let fromKey = canonicalSpeaker(channel.name, upstream.attribution, event.from)!, toKey = canonicalSpeaker(channel.name, upstream.attribution, event.to)!
    if (fromKey === toKey) return
    // On the microphone a merge with "Me" is always into "Me": the other id was never a second person.
    if (fromKey === ME) [fromKey, toKey] = [toKey, fromKey]
    if (toKey === ME) upstream.attribution.trusted.delete(fromKey)
    const from = upstream.speakers[fromKey], to = upstream.speakers[toKey]
    if (from === undefined) return
    if (to === undefined) { upstream.speakers = { ...upstream.speakers, [toKey]: from }; return }
    await this.relabel(channel, upstream, fromKey, from, to)
  }
  /** Everything stored under speaker number `from` becomes `to`, and the client is told: the one way a
   * stored label ever changes. Chat threads that showed the old label are told through their next
   * message (the `<corrections>` block in ai/meeting-chat.ts). */
  private async relabel(channel: Channel, upstream: Upstream, key: string, from: number, to: number) {
    upstream.speakers = { ...upstream.speakers, [key]: to }
    if (from === to) return
    const merged = await this.write(() => Effect.runPromise(this.store.mergeSpeakers(this.noteId, channel.name, from, to, this.label(channel.name, to))).catch(() => false))
    if (merged) this.post({ type: "speakers", merges: [{ from: liveSpeakerId(channel.name, from), to: liveSpeakerId(channel.name, to) }] })
    const sample = channel.voices.get(from)
    if (sample) { channel.voices.delete(from); if (!channel.voices.has(to)) channel.voices.set(to, sample) }
    if (channel.interim?.speaker === from) channel.interim = { ...channel.interim, speaker: to }
  }
  private write<A>(task: () => Promise<A>): Promise<A> {
    const next = this.writes.then(task, task)
    this.writes = next.catch(() => undefined)
    return next
  }
  /** A provider session was lost. Audio since its last final has no text unless a new provider session
   * opens in time for the client's backlog (the gap is reported to the client). */
  private failed(channel: Channel, upstream: Upstream, event: Extract<LiveTranscriberEvent, { type: "error" }>) {
    if (channel.upstream !== upstream) return
    // The provider's error kind only: enough for an operator, and never any provider text or audio.
    if (!this.ended && !this.ending) console.warn(`Live transcription: the ${channel.name} provider session ended (${event.detail ?? event.reason ?? "unavailable"})`)
    channel.upstream = undefined
    channel.unavailableFrom = upstream.timeline.captureSample(upstream.finalThrough / channel.sampleRate)
    this.answered(upstream, true)
    void upstream.link.close()
    if (channel.interim) { channel.interim = { ...channel.interim, rev: channel.interim.rev + 1, text: "" }; this.postInterim(channel) }
    if (this.ended || this.ending) return
    if (!event.retryable) {
      const over = event.reason === "over_limit"
      void this.end(over ? LiveCloseCode.overLimit : LiveCloseCode.providerUnavailable, over ? "over_limit" : "provider_unavailable", "Live transcription is unavailable; audio recorded from now on will not be transcribed until it is back")
      return
    }
    // The client keeps the audio it sends; the server does not. Reopening a provider session here would
    // leave what is said until then without text, so the client is told to reconnect: its new session
    // resumes from the stored ledger and the client replays the backlog (up to `resumeWindowMs`).
    if (this.socket) { void this.end(RECONNECT_CLOSE, "provider_unavailable", "Live transcription is reconnecting"); return }
    this.unavailable(channel)
  }
  private unavailable(channel: Channel, initial = false) {
    channel.unavailableFrom ??= channel.nextSample
    channel.unavailableSince ??= performance.now()
    const delay = this.config.reopenDelaysMs[Math.min(channel.reopenAttempt, this.config.reopenDelaysMs.length - 1)]!
    if (!initial) this.post({ type: "error", code: "provider_unavailable", message: "Live transcription is temporarily unavailable; the recording continues", retryable: true, retryAfterMs: delay })
    this.status(channel, true)
    if (channel.paused) return
    this.reopen(channel, delay)
  }
  private reopen(channel: Channel, delay: number) {
    clearTimeout(channel.reopenTimer)
    channel.reopenTimer = setTimeout(async () => {
      channel.reopenTimer = undefined
      if (this.ended || this.ending || channel.upstream || channel.paused) return
      try {
        await this.open(channel)
        if (!channel.upstream) return
        if (channel.unavailableFrom !== undefined && channel.unavailableFrom < channel.nextSample) this.gap(channel.name, this.ms(channel, channel.unavailableFrom), this.ms(channel, channel.nextSample), "provider_unavailable")
        channel.unavailableFrom = undefined; channel.unavailableSince = undefined; channel.reopenAttempt = 0
        this.status(channel, true)
      } catch (error) {
        channel.reopenAttempt++
        const fatal = error instanceof LiveProviderError && !error.retryable
        if (fatal || performance.now() - (channel.unavailableSince ?? 0) > this.config.providerGiveUpMs) {
          const over = error instanceof LiveProviderError && error.reason === "over_limit"
          void this.end(over ? LiveCloseCode.overLimit : LiveCloseCode.providerUnavailable, over ? "over_limit" : "provider_unavailable", "Live transcription is unavailable; audio recorded from now on will not be transcribed until it is back")
        } else this.unavailable(channel, true)
      }
    }, delay)
  }
  private tick() {
    if (this.ended || this.ending) return
    const now = performance.now()
    if (this.detachedAt !== undefined && now - this.detachedAt > this.config.detachMs) { void this.end(); return }
    for (const channel of this.channels.values()) {
      const upstream = channel.upstream
      if (upstream && channel.paused && now - channel.pausedAt > this.config.pauseKeepMs) {
        // A long pause: let the provider session go. Resuming opens a new one, with new speaker numbers.
        channel.upstream = undefined
        void this.settle(channel, upstream, 0).then(() => upstream.session.close()).catch(() => undefined)
        continue
      }
      if (upstream?.pendingFlushes && now - upstream.lastCommitAt > 3000) this.answered(upstream, true)
      if (upstream && now - upstream.lastSendAt >= this.config.keepAliveMs) {
        // Silence exists only upstream, so the timeline records that the capture did not advance.
        const seconds = upstream.session.keepAlive()
        if (seconds > 0) upstream.timeline.silence(channel.nextSample, Math.round(seconds * channel.sampleRate))
        upstream.lastSendAt = now
      }
      if (channel.pace.length && now - channel.pace.at(-1)![0] > 2000) channel.pace = []
      // Also when frames have stopped arriving: trailing silence still has to be confirmed.
      if (channel.upstream) this.pace(channel, channel.upstream, now)
      this.acknowledge(channel, now)
      this.status(channel)
    }
  }

  // ------------------------------------------------------------------ what chat asks

  /** Asks the provider to finalise what has been heard, where that does not cut a word, and waits
   * briefly for the stored text. Limited to one per `chatFlushIntervalMs`. */
  async flushForChat() {
    const now = performance.now()
    if (this.ended || this.ending || now - this.lastChatFlushAt < this.config.chatFlushIntervalMs) { await this.settleWrites(this.config.chatFlushWaitMs); return }
    const waits: Promise<void>[] = []
    for (const channel of this.channels.values()) {
      const upstream = channel.upstream
      if (!upstream || !channel.interim?.text || upstream.pendingFlushes) continue
      if (channel.quietSamples * 1000 / channel.sampleRate < PAUSE_MS && now - upstream.lastSendAt < 1000) continue
      waits.push(this.commit(channel, upstream, now))
    }
    if (waits.length) this.lastChatFlushAt = now
    await Promise.race([Promise.all(waits).then(() => this.writes), new Promise(resolve => setTimeout(resolve, this.config.chatFlushWaitMs))])
  }
  private settleWrites(limitMs: number) { return Promise.race([this.writes, new Promise(resolve => setTimeout(resolve, limitMs))]) }
  /** The unfinished utterance of each channel and how current the server's audio is. */
  snapshot(): LiveSnapshot {
    const channels = [...this.channels.values()], states = channels.map(channel => this.state(channel))
    return {
      asOfMs: Math.max(0, ...channels.map(channel => this.ms(channel, channel.nextSample))),
      tail: channels.flatMap(channel => channel.interim?.text ? [{ channel: channel.name, text: channel.interim.text, startMs: channel.interim.startMs, ...(channel.interim.speaker === undefined ? {} : { speakerId: liveSpeakerId(channel.name, channel.interim.speaker) }) }] : []),
      state: states.every(state => state === "provider_unavailable") ? "provider_unavailable" : states.includes("catching_up") ? "catching_up" : "live",
    }
  }

  // ------------------------------------------------------------------ ending

  /** Makes everything the provider session has heard final, waiting up to `waitMs` for the answers.
   *
   * `abrupt`: the server told the client to reconnect, and its replay follows at once. If someone is
   * speaking at that point the unfinished utterance is not flushed: a flush there stores a clipped word,
   * which the recogniser can mishear ("Item fif-" came back as "Item 55."). The utterance stays out of
   * coverage, so the replay transcribes it whole. Any other end flushes: the live transcript is the only
   * one, and a clipped last word is better than a lost last sentence. */
  private async settle(channel: Channel, upstream: Upstream, waitMs: number, abrupt = false) {
    // The provider may still be working through audio held back for its pace (a backlog sent at the end): keep
    // waiting while its finals keep coming, up to `SETTLE_MAX_MS`, and stop `waitMs` after the last progress.
    const wait = async (work: Promise<void>) => {
      if (waitMs <= 0) return
      let done = false, progress = upstream.finalThrough, idleSince = performance.now()
      const finished = work.then(() => { done = true })
      for (const deadline = performance.now() + SETTLE_MAX_MS; !done && performance.now() < deadline;) {
        // The answer ends the wait the moment it is stored, not at the next look.
        await Promise.race([finished, new Promise(resolve => setTimeout(resolve, 50))])
        if (done) return
        if (upstream.finalThrough > progress) { progress = upstream.finalThrough; idleSince = performance.now() }
        else if (performance.now() - idleSince > waitMs + Math.ceil((upstream.session.queuedSeconds ?? 0) * 1000 / 3)) return
      }
    }
    // A flush already in flight only covers what had been sent when it left; audio after it needs its own.
    if (upstream.pendingFlushes) await wait(this.flushed(upstream))
    if (upstream.timeline.upstreamSamples <= upstream.finalThrough) return
    if (await this.coverQuietTail(channel, upstream)) return
    if (abrupt && channel.quietSamples * 1000 / channel.sampleRate < PAUSE_MS) return
    await wait(this.commit(channel, upstream, performance.now()))
    // The service does not always answer a flush that follows another by a fraction of a second.
    await this.coverQuietTail(channel, upstream)
  }
  /** A short stretch at the end in which the server itself heard nothing above the noise floor is
   * accounted for without asking the provider, which saves a flush round trip at Stop. Speech the
   * provider still reports for that stretch is stored all the same. */
  private async coverQuietTail(channel: Channel, upstream: Upstream) {
    const { timeline } = upstream, rest = timeline.upstreamSamples - upstream.finalThrough
    if (rest <= 0) return true
    if (rest > channel.sampleRate || channel.quietSamples < rest || channel.interim?.text) return false
    const coverage = timeline.captureRanges(upstream.finalThrough, timeline.upstreamSamples).map(([from, to]) => ({ fromMs: this.ms(channel, from), toMs: this.ms(channel, to) }))
    upstream.finalThrough = timeline.upstreamSamples
    const stored = await this.write(() => this.append(channel, upstream, [], coverage))
    if (stored !== "gone" && stored !== "failed") for (const interval of coverage) channel.finalThroughMs = Math.max(channel.finalThroughMs, interval.toMs)
    return true
  }
  /** Ends the session for a Stop that arrived over HTTP. The last audio frames, and the client's
   * `mark: end`, travel on the socket and can still be in flight when the Stop request is handled. So
   * with a socket attached this waits until the mark has arrived, or until no frame has arrived for
   * `stopIdleMs` counted from this call, and never longer than `stopGraceMs`. A client that sent
   * `mark: end` and saw the socket close pays no wait at all. */
  async endAfterAudio() {
    const called = performance.now(), deadline = called + this.config.stopGraceMs
    while (this.socket && !this.ending && !this.ended && performance.now() < deadline && performance.now() - Math.max(called, this.lastFrameAt) < this.config.stopIdleMs) await new Promise(resolve => setTimeout(resolve, 20))
    await this.end()
  }
  /** Drops the session as a crash would: nothing is flushed and nothing more is stored. What it had
   * not finalised has no text unless the client replays it to a new session. */
  abandon() {
    if (this.ended) return
    this.ended = true
    clearInterval(this.timer)
    for (const channel of this.channels.values()) {
      clearTimeout(channel.reopenTimer)
      const upstream = channel.upstream
      channel.upstream = undefined
      if (upstream) void upstream.link.close()
    }
    const socket = this.socket
    this.socket = undefined
    try { socket?.close(1011, "") } catch { /* already gone */ }
    this.onEnded(this)
  }
  /** Flushes, waits for the last finals, closes the provider sessions and the socket. Idempotent. */
  end(code: number = 1000, error?: Extract<LiveServerMessage, { type: "error" }>["code"], message?: string): Promise<void> {
    this.ending ??= (async () => {
      clearInterval(this.timer)
      // Told to reconnect: an utterance cut here is left for the client's replay, which follows at once (see
      // `settle`). A client that went away (crashed, or offline for longer than `detachMs`) may never come back,
      // so what it sent is made final, a clipped last word included: a returning client replays from there.
      const reconnect = code === RECONNECT_CLOSE, abrupt = reconnect
      const upstreams: Upstream[] = [], settling: Promise<void>[] = []
      for (const channel of this.channels.values()) { clearTimeout(channel.reopenTimer); if (channel.upstream) { upstreams.push(channel.upstream); settling.push(this.settle(channel, channel.upstream, this.config.endWaitMs, abrupt)) } }
      await Promise.all(settling)
      trace("srv_end_settled", this.noteId, this.sessionId)
      await this.settleWrites(this.config.endWaitMs)
      // An orderly end keeps the provider sessions, and with them the provider's voices, for the note's
      // next live session: a Stop is often followed by a Resume. So does a reconnect the server asked for,
      // for the channels whose provider session is still fine. Any other end closes them.
      const kept = (code === 1000 || reconnect) && this.hold && this.config.holdMs > 0 ? upstreams.filter(upstream => !upstream.link.lost) : []
      const closing = upstreams.filter(upstream => !kept.includes(upstream))
      await Promise.all(closing.map(upstream => upstream.session.close().catch(() => undefined)))
      await Promise.race([Promise.all(closing.map(upstream => upstream.link.done)), new Promise(resolve => setTimeout(resolve, 1000))])
      await this.settleWrites(1000)
      for (const channel of this.channels.values()) { this.acknowledge(channel, performance.now(), true); channel.upstream = undefined }
      // The newest audio of each channel, for the primer of the note's next live session.
      for (const channel of this.channels.values()) if (channel.recentSamples) this.hold?.keepAudio?.(channel.name, { fromMs: this.ms(channel, channel.recentFrom), sampleRate: channel.sampleRate, pcm: concat(channel.recent), voices: [...channel.voices.entries()].map(([number, sample]) => ({ number, ...sample })) })
      for (const upstream of kept) { upstream.link.release(); upstream.link.speakers = { ...upstream.speakers }; upstream.link.sent = upstream.timeline.upstreamSamples }
      if (kept.length) this.hold!.park(kept.map(upstream => upstream.link))
      this.ended = true
      trace("srv_end_closed", this.noteId, this.sessionId)
      const socket = this.socket
      this.socket = undefined
      if (socket) closeSocket(socket, code, error, message, reconnect ? true : error === "provider_unavailable" ? false : undefined)
      this.onEnded(this)
    })()
    return this.ending
  }
}

const concat = (pieces: ReadonlyArray<Uint8Array>) => { const out = new Uint8Array(pieces.reduce((sum, piece) => sum + piece.byteLength, 0)); let offset = 0; for (const piece of pieces) { out.set(piece, offset); offset += piece.byteLength } return out }

/** A refusal decided after the upgrade: an `error` message, then the close code. */
// (LiveSession.abandon, above, is the only way a session ends without storing its last words.)
export class LiveSessionRefusal extends Error {
  constructor(readonly code: number, message: string) { super(message) }
}
const errorCodes: Record<number, Extract<LiveServerMessage, { type: "error" }>["code"]> = {
  [LiveCloseCode.malformed]: "malformed", [LiveCloseCode.unauthenticated]: "unauthenticated", [LiveCloseCode.notFound]: "not_found", [LiveCloseCode.clientTooSlow]: "client_too_slow",
  [LiveCloseCode.replaced]: "replaced", [LiveCloseCode.overLimit]: "over_limit", [LiveCloseCode.providerUnavailable]: "provider_unavailable",
}
/** Every 4xxx close is preceded by an `error` message with the same meaning. */
export function closeSocket(socket: LiveSocket, code: number, error?: Extract<LiveServerMessage, { type: "error" }>["code"], message?: string, retryable?: boolean) {
  const kind = error ?? errorCodes[code]
  try {
    if (kind) socket.send(JSON.stringify({ type: "error", code: kind, message: message ?? kind, retryable: retryable ?? (code === LiveCloseCode.clientTooSlow) } satisfies LiveServerMessage))
    socket.close(code, kind ?? "")
  } catch { /* the socket is already gone */ }
}
