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
 * speaker labels. Everything it does not manage to transcribe is simply absent from the coverage ledger
 * and is picked up from the durable audio parts by the batch worker.
 *
 * Speaker labels across provider sessions: when the session ends in an orderly way its provider sessions
 * are handed to the hub, which holds them for `holdMs`; the note's next session takes them over with
 * their voices (link.ts). A channel that has to open a new provider session although the note already
 * has numbered voices there decides who is speaking as described in voices.ts.
 */
import { Effect } from "effect"
import type { LiveAudioFrame, LiveHelloMessage, LiveMarkMessage, LiveServerMessage, LiveStreamState, LiveTranscriber, LiveTranscriberEvent, LiveTranscriberSession } from "@meeting-notes/contracts/live"
import { LiveCloseCode } from "@meeting-notes/contracts/live"
import { liveSpeakerId, liveSpeakerLabel, type Interval, type SpeakerTurn, type TimedWord } from "./coverage"
import { ProviderLink, type LinkKey } from "./link"
import type { AudioChannelName, LiveRepository } from "./repository"
import { LiveProviderError } from "./providers/queue"
import { Timeline } from "./timeline"
import type { VoiceMatcher, VoiceVerdict } from "./voices"

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
  /** How long a final waits for the comparison of a new voice with the note's earlier voices (voices.ts). */
  voiceMatchWaitMs: number
}
export const DEFAULT_LIVE_CONFIG: LiveConfig = {
  resumeWindowMs: 30_000, detachMs: 20_000, pauseKeepMs: 120_000, keepAliveMs: 5_000, chatFlushIntervalMs: 5_000, chatFlushWaitMs: 400, endWaitMs: 2_000,
  forceCommitMs: 20_000, forceCommitHardMs: 30_000, checkpointMs: 5_000, upstreamBufferMs: 10_000, maxQueuedFinals: 200, providerGiveUpMs: 60_000,
  reopenDelaysMs: [1_000, 2_000, 4_000, 8_000, 15_000], ackIntervalMs: 1_000, statusIntervalMs: 2_000, tickMs: 500, stopIdleMs: 250, stopGraceMs: 1_500,
  holdMs: 300_000, voiceMatchWaitMs: 3_000,
}
/** Below this amplitude a sample counts as a pause (about -36 dBFS). */
const QUIET_AMPLITUDE = 500
/** A flush is only sent after this much quiet, so it does not cut a word in two. */
const PAUSE_MS = 150
const UNKNOWN_SPEAKER = "_"
/** Comparing voices (voices.ts): the newest audio of a provider session that is kept for it, the speech
 * a voice needs before it is asked about, and how many comparisons one provider session may ask for. */
const VOICE_WINDOW_MS = 30_000, VOICE_MIN_SPEECH_MS = 3_000, VOICE_MAX_QUESTIONS = 6

/** Where the hub keeps provider sessions between two live sessions of a note. */
export interface LiveHold {
  /** A held provider session opened for exactly `key`, if there is one. It is the caller's from then on. */
  take(key: LinkKey): ProviderLink | undefined
  park(links: ReadonlyArray<ProviderLink>): void
}
/** A provider session that is not the one that numbered the channel's earlier voices (see voices.ts). */
interface Recognition {
  /** How many voices the channel had when this provider session opened. */
  readonly prior: number
  /** Earlier voices that no voice of this provider session has been matched to. */
  open: number[]
  /** Provider speakers of this session that have been decided on, in any way. */
  readonly met: Set<string>
  /** Voices numbered without evidence, which may still turn out to be an earlier voice. */
  readonly provisional: Map<string, number>
  /** Where each undecided voice has spoken, in note time. */
  readonly heard: Map<string, Interval[]>
  /** The newest capture audio sent to this provider session; `from` is the capture sample of its first sample. */
  pcm: Uint8Array[]
  from: number
  samples: number
  asked: number
}

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
  recognition?: Recognition
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
  pendingInterim: boolean
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
  /** `atSample` of the client's `mark: end`: how much audio the capture holds, per channel. */
  private endMark: Partial<Record<AudioChannelName, number>> | undefined
  private lastChatFlushAt = -Infinity
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
    private readonly voices: VoiceMatcher | undefined,
  ) { this.captureId = hello.captureId }

  /** Opens the provider sessions and decides where each channel resumes. Throws `LiveProviderError`
   * when no channel could be opened. */
  static async start(input: { noteId: string; hello: LiveHelloMessage; transcriber: LiveTranscriber; store: LiveRepository; config: LiveConfig; onEnded: (session: LiveSession) => void; transcriptRevision: () => Promise<number>; hold?: LiveHold; voices?: VoiceMatcher }): Promise<LiveSession> {
    const session = new LiveSession(input.noteId, input.hello, input.transcriber, input.store, input.config, input.onEnded, input.transcriptRevision, input.hold, input.voices)
    const ledger = await Effect.runPromise(input.store.ledger(input.noteId))
    const now = performance.now()
    for (const stream of input.hello.streams) {
      if (session.channels.has(stream.channel)) throw new LiveSessionRefusal(LiveCloseCode.malformed, "A channel is listed twice")
      // Audio this capture already had accounted for, by either path, is not asked for again. With no
      // record of the capture the client's own resume point stands.
      const known = ledger.filter(row => row.channel === stream.channel && row.toMs > stream.baseMs).map(row => row.toMs)
      const accept = known.length ? Math.floor((Math.max(...known) - stream.baseMs) * stream.sampleRate / 1000) : stream.resumeFromSample
      session.channels.set(stream.channel, { name: stream.channel, sampleRate: stream.sampleRate, baseMs: stream.baseMs, nextSample: accept, finalThroughMs: session.ms(stream, accept), quietSamples: 0, quietThroughSample: accept,
        paused: false, pausedAt: 0, reopenAttempt: 0, ackedAt: now, ackedSample: accept, statusAt: 0, pace: [], pendingInterim: false })
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
      this.post({ type: "gap", channel: channel.name, fromMs: this.ms(channel, channel.nextSample), toMs: this.ms(channel, firstSample), reason: "resume_window_exceeded" })
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
    const upstream = channel.upstream
    if (upstream) {
      upstream.timeline.audio(firstSample, samples)
      upstream.session.send(pcm)
      upstream.lastSendAt = now
      if (upstream.recognition && this.voices) this.remember(channel, upstream.recognition, firstSample, pcm)
    } else channel.unavailableFrom ??= firstSample
    channel.nextSample = firstSample + samples
    channel.pace.push([now, samples])
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
    if (channel.interim?.text) { if (pendingMs >= this.config.forceCommitHardMs || pendingMs >= this.config.forceCommitMs && quietMs >= PAUSE_MS) this.commit(channel, upstream, now) }
    // Nothing is being said: have the provider confirm the silence, so coverage moves past it and a
    // durable part that holds only silence costs no batch call.
    else if (pendingMs >= this.config.checkpointMs && quietMs >= 500) this.commit(channel, upstream, now)
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
    if (mark.kind === "end") { this.endMark = mark.atSample; void this.end(); return }
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

  private async open(channel: Channel) {
    const abort = new AbortController()
    const opening = (async () => {
      const key: LinkKey = { provider: this.transcriber.provider, model: this.transcriber.model, channel: channel.name, sampleRate: channel.sampleRate, ...(this.hello.language ? { language: this.hello.language } : {}) }
      // A provider session the hub held since this note's previous live session knows the voices already.
      const held = this.hold?.take(key)
      const link = held ?? new ProviderLink(await this.transcriber.open({ channel: channel.name, sampleRate: channel.sampleRate, ...(this.hello.language ? { language: this.hello.language } : {}), signal: abort.signal }), abort, key)
      if (this.ended) { await link.close(); return }
      const now = performance.now(), timeline = new Timeline(channel.sampleRate)
      // The provider's clock goes on from where the earlier capture left it; nothing before is this capture's audio.
      if (link.sent) timeline.silence(channel.nextSample, link.sent)
      const upstream: Upstream = { id: crypto.randomUUID(), link, session: link.session, timeline, abort: link.abort, speakers: { ...link.speakers }, origin: link.sent, finalThrough: link.sent, utterances: new Map(), lastSendAt: now, lastCommitAt: now, flushedThrough: link.sent, pendingFlushes: 0, flushWatchers: [] }
      if (!held) {
        // A new provider session numbers voices afresh. If the note already has voices on this channel, find out who is who.
        const earlier = await Effect.runPromise(this.store.voices(this.noteId, channel.name)).catch(() => [] as ReadonlyArray<number>)
        if (earlier.length) upstream.recognition = { prior: earlier.length, open: [...earlier], met: new Set(), provisional: new Map(), heard: new Map(), pcm: [], from: channel.nextSample, samples: 0, asked: 0 }
      }
      channel.upstream = upstream
      // Makes the note "live-capable" in the ledger before any text exists, so the batch worker
      // records what it transcribes from here on.
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
  private interim(channel: Channel, upstream: Upstream, event: Extract<LiveTranscriberEvent, { type: "interim" }>) {
    if (channel.upstream !== upstream || event.endSec * channel.sampleRate <= upstream.origin && event.text.trim()) return
    const text = event.text.trim()
    if (!text && !channel.interim) return
    let utteranceId = upstream.utterances.get(event.utterance)
    if (!utteranceId) { if (!text) return; utteranceId = crypto.randomUUID(); upstream.utterances.set(event.utterance, utteranceId) }
    const previous = channel.interim?.utteranceId === utteranceId ? channel.interim : undefined
    const speaker = event.speaker === undefined ? previous?.speaker : upstream.speakers[event.speaker]
    const startMs = this.ms(channel, upstream.timeline.captureSample(event.startSec)), endMs = Math.max(startMs, this.ms(channel, upstream.timeline.captureSample(event.endSec)))
    if (previous && previous.text === text && previous.speaker === speaker) return
    channel.interim = { utteranceId, rev: previous ? previous.rev + 1 : 0, text, startMs, endMs, ...(speaker === undefined ? {} : { speaker }) }
    this.postInterim(channel)
  }
  private postInterim(channel: Channel) {
    const interim = channel.interim
    if (!interim) return
    if (this.backpressured) { channel.pendingInterim = true; return }
    this.post({ type: "interim", channel: channel.name, utteranceId: interim.utteranceId, rev: interim.rev, text: interim.text, startMs: interim.startMs, endMs: interim.endMs,
      ...(interim.speaker === undefined ? {} : { speakerId: liveSpeakerId(channel.name, interim.speaker), speakerLabel: liveSpeakerLabel(interim.speaker) }) })
    if (!interim.text) channel.interim = undefined
  }
  private async final(channel: Channel, upstream: Upstream, event: Extract<LiveTranscriberEvent, { type: "final" }>) {
    const { timeline } = upstream, rate = channel.sampleRate
    // Never less than the end of the words being stored: text without coverage would be transcribed again.
    const through = Math.max(upstream.finalThrough, Math.min(timeline.upstreamSamples, Math.round(Math.max(event.finalThroughSec, event.words?.at(-1)?.endSec ?? 0) * rate)))
    const coverage: Interval[] = timeline.captureRanges(upstream.finalThrough, through).map(([from, to]) => ({ fromMs: this.ms(channel, from), toMs: this.ms(channel, to) }))
    upstream.finalThrough = through
    const word = (text: string, startSec: number, endSec: number): TimedWord => { const startMs = this.ms(channel, timeline.captureSample(startSec)); return { text, startMs, endMs: Math.max(startMs, this.ms(channel, timeline.captureSample(endSec))) } }
    const turns: Array<{ speakerKey: string; words: TimedWord[] }> = []
    if (event.words?.length) for (const item of event.words) {
      if (item.endSec * rate <= upstream.origin) continue
      const key = item.speaker ?? UNKNOWN_SPEAKER, last = turns.at(-1)
      if (last && last.speakerKey === key) last.words.push(word(item.text, item.startSec, item.endSec)); else turns.push({ speakerKey: key, words: [word(item.text, item.startSec, item.endSec)] })
    } else if (event.text.trim() && event.endSec * rate > upstream.origin) turns.push({ speakerKey: UNKNOWN_SPEAKER, words: [word(event.text.trim(), event.startSec, event.endSec)] })
    timeline.prune(through)
    const utteranceId = upstream.utterances.get(event.utterance) ?? crypto.randomUUID()
    upstream.utterances.delete(event.utterance)
    const watched = await this.identify(channel, upstream, turns)
    const stored = await this.write(() => this.append(channel, upstream, turns, coverage))
    if (stored !== "gone" && stored !== "failed") for (const key of watched) { const number = upstream.speakers[key]; if (number !== undefined) upstream.recognition?.provisional.set(key, number) }
    if (channel.interim?.utteranceId === utteranceId) channel.interim = undefined
    if (event.flushed !== false) this.answered(upstream)
    if (stored === "gone") { void this.end(LiveCloseCode.notFound, "not_found", "The note is no longer available"); return }
    if (stored === "failed") {
      // Not stored, so not covered: the durable part will supply this stretch.
      for (const interval of coverage) this.post({ type: "gap", channel: channel.name, fromMs: interval.fromMs, toMs: interval.toMs, reason: "provider_unavailable" })
      return
    }
    for (const interval of coverage) channel.finalThroughMs = Math.max(channel.finalThroughMs, interval.toMs)
    for (const segment of stored.segments) this.post({ type: "final", utteranceId, transcriptRevision: stored.transcriptRevision, segment })
    this.acknowledge(channel, performance.now(), stored.segments.length > 0)
  }
  private async append(channel: Channel, upstream: Upstream, turns: ReadonlyArray<SpeakerTurn>, coverage: ReadonlyArray<Interval>) {
    for (let attempt = 0; attempt < 2; attempt++) {
      try {
        const result = await Effect.runPromise(this.store.append({ noteId: this.noteId, sessionId: this.sessionId, channel: channel.name, turns, speakers: upstream.speakers, coverage }))
        if (!result) return "gone" as const
        upstream.speakers = { ...result.speakers }
        return result
      } catch { await new Promise(resolve => setTimeout(resolve, 200)) }
    }
    return "failed" as const
  }
  private async merge(channel: Channel, upstream: Upstream, event: Extract<LiveTranscriberEvent, { type: "speakers_merged" }>) {
    const from = upstream.speakers[event.from], to = upstream.speakers[event.to]
    if (from === undefined) return
    if (to === undefined) { upstream.speakers = { ...upstream.speakers, [event.to]: from }; return }
    await this.relabel(channel, upstream, event.from, from, to)
  }
  /** Everything stored under speaker number `from` becomes `to`, and the client is told: the one way a
   * stored label ever changes. Chat threads that showed the old label are told through their next
   * message (the `<corrections>` block in ai/meeting-chat.ts). */
  private async relabel(channel: Channel, upstream: Upstream, key: string, from: number, to: number) {
    upstream.speakers = { ...upstream.speakers, [key]: to }
    upstream.recognition?.provisional.delete(key)
    if (from === to) return
    const merged = await this.write(() => Effect.runPromise(this.store.mergeSpeakers(this.noteId, channel.name, from, to)).catch(() => false))
    if (merged) this.post({ type: "speakers", merges: [{ from: liveSpeakerId(channel.name, from), to: liveSpeakerId(channel.name, to) }] })
    if (channel.interim?.speaker === from) channel.interim = { ...channel.interim, speaker: to }
  }
  /** Keeps the newest `VOICE_WINDOW_MS` of what a new provider session was sent, for the comparison. */
  private remember(channel: Channel, recognition: Recognition, firstSample: number, pcm: Uint8Array) {
    if (firstSample !== recognition.from + recognition.samples) { recognition.pcm = []; recognition.from = firstSample; recognition.samples = 0 }
    recognition.pcm.push(pcm.slice()); recognition.samples += pcm.byteLength >> 1
    const most = VOICE_WINDOW_MS * channel.sampleRate / 1000
    while (recognition.pcm.length > 1 && recognition.samples - (recognition.pcm[0]!.byteLength >> 1) >= most) { const dropped = recognition.pcm.shift()!.byteLength >> 1; recognition.from += dropped; recognition.samples -= dropped }
  }
  /** Decides who the voices of a final are before it is stored, when its provider session is not the one
   * that numbered the channel's earlier voices. The rule is at the top of voices.ts. Returns the voices
   * that are about to be numbered without evidence. */
  private async identify(channel: Channel, upstream: Upstream, turns: ReadonlyArray<SpeakerTurn>): Promise<string[]> {
    const recognition = upstream.recognition
    if (!recognition) return []
    const undecided = (key: string) => upstream.speakers[key] === undefined || recognition.provisional.has(key)
    const keys = [...new Set(turns.map(turn => turn.speakerKey))].filter(undecided)
    if (!keys.length) return []
    for (const turn of turns) if (undecided(turn.speakerKey) && turn.words.length) recognition.heard.set(turn.speakerKey, [...(recognition.heard.get(turn.speakerKey) ?? []), { fromMs: turn.words[0]!.startMs, toMs: turn.words.at(-1)!.endMs }])
    const windowMs = this.ms(channel, recognition.from)
    const spoken = (key: string) => (recognition.heard.get(key) ?? []).reduce((sum, interval) => sum + Math.max(0, interval.toMs - Math.max(interval.fromMs, windowMs)), 0)
    let verdicts: Record<string, VoiceVerdict> = {}
    if (this.voices && recognition.open.length && recognition.asked < VOICE_MAX_QUESTIONS && keys.some(key => spoken(key) >= VOICE_MIN_SPEECH_MS)) {
      recognition.asked++
      const pcm = new Uint8Array(recognition.samples * 2); let offset = 0
      for (const piece of recognition.pcm) { pcm.set(piece, offset); offset += piece.byteLength }
      const asked = keys.filter(key => spoken(key) >= VOICE_MIN_SPEECH_MS)
      // The final waits for the answer, so a voice gets its label once and it does not change afterwards.
      // Also while the session is ending: a voice left numbered on at a Stop would stay split for good.
      verdicts = await Promise.race([
        this.voices.match({ noteId: this.noteId, channel: channel.name, candidates: recognition.open, fresh: { pcm, sampleRate: channel.sampleRate, startMs: windowMs }, voices: Object.fromEntries(asked.map(key => [key, recognition.heard.get(key)!])) }).catch(() => ({})),
        new Promise<Record<string, VoiceVerdict>>(resolve => setTimeout(() => resolve({}), this.config.voiceMatchWaitMs)),
      ])
    }
    const watched: string[] = []
    for (const key of keys) {
      const verdict = verdicts[key] ?? "unsure", provisional = recognition.provisional.get(key), first = !recognition.met.size
      recognition.met.add(key)
      if (typeof verdict === "number" && recognition.open.includes(verdict)) {
        // Evidence: this is an earlier voice.
        recognition.open = recognition.open.filter(number => number !== verdict)
        recognition.heard.delete(key)
        if (provisional === undefined) upstream.speakers = { ...upstream.speakers, [key]: verdict }
        else await this.relabel(channel, upstream, key, provisional, verdict)
      } else if (provisional !== undefined) {
        if (verdict === "new" || recognition.asked >= VOICE_MAX_QUESTIONS) { recognition.provisional.delete(key); recognition.heard.delete(key) }
      } else if (verdict !== "new" && first && recognition.prior === 1 && recognition.open.length === 1) {
        // The channel rule: one voice before, and this is the first voice after.
        upstream.speakers = { ...upstream.speakers, [key]: recognition.open[0]! }
        recognition.open = []; recognition.heard.delete(key)
      } else if (verdict !== "new" && recognition.open.length && this.voices && recognition.asked < VOICE_MAX_QUESTIONS) watched.push(key)
      else recognition.heard.delete(key)
    }
    // Every earlier voice is accounted for, or the comparisons are used up: later voices simply number on.
    if (!recognition.open.length || !this.voices || recognition.asked >= VOICE_MAX_QUESTIONS && !watched.length) upstream.recognition = undefined
    return watched
  }
  private write<A>(task: () => Promise<A>): Promise<A> {
    const next = this.writes.then(task, task)
    this.writes = next.catch(() => undefined)
    return next
  }
  /** A provider session was lost. Audio since its last final is left to the durable parts. */
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
      void this.end(over ? LiveCloseCode.overLimit : LiveCloseCode.providerUnavailable, over ? "over_limit" : "provider_unavailable", "Live transcription is unavailable; the recording continues and will be transcribed from its saved audio")
      return
    }
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
        if (channel.unavailableFrom !== undefined && channel.unavailableFrom < channel.nextSample) this.post({ type: "gap", channel: channel.name, fromMs: this.ms(channel, channel.unavailableFrom), toMs: this.ms(channel, channel.nextSample), reason: "provider_unavailable" })
        channel.unavailableFrom = undefined; channel.unavailableSince = undefined; channel.reopenAttempt = 0
        this.status(channel, true)
      } catch (error) {
        channel.reopenAttempt++
        const fatal = error instanceof LiveProviderError && !error.retryable
        if (fatal || performance.now() - (channel.unavailableSince ?? 0) > this.config.providerGiveUpMs) {
          const over = error instanceof LiveProviderError && error.reason === "over_limit"
          void this.end(over ? LiveCloseCode.overLimit : LiveCloseCode.providerUnavailable, over ? "over_limit" : "provider_unavailable", "Live transcription is unavailable; the recording continues and will be transcribed from its saved audio")
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

  // ------------------------------------------------------------------ what chat and the worker ask

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
  /** Audio a working provider session has not finalised yet: the batch worker waits for it. The range
   * is open-ended, because a durable part can arrive before the stream has reached the same audio (the
   * stream runs a little behind the capture, more after a reconnect), and transcribing it in batch then
   * would pay for audio the stream is about to cover. The worker bounds the wait. */
  pendingRange(name: AudioChannelName): Interval | null {
    const channel = this.channels.get(name), upstream = channel?.upstream
    // Still pending while the session is ending: its last finals are being stored.
    if (!channel || !upstream || this.ended) return null
    return { fromMs: this.ms(channel, upstream.timeline.captureSample(upstream.finalThrough / channel.sampleRate)), toMs: Number.MAX_SAFE_INTEGER }
  }

  // ------------------------------------------------------------------ ending

  /** Makes everything the provider session has heard final, waiting up to `waitMs` for the answers.
   *
   * `abrupt`: the stream stopped although the capture goes on (the connection died, or the client's
   * `mark: end` says the capture holds more than was sent). If someone is speaking at that point the
   * unfinished utterance is not flushed: a flush there stores a clipped word, which the recogniser can
   * mishear ("Item fif-" came back as "Item 55."). The utterance stays out of coverage, and the durable
   * part, which has the audio beyond the cut, supplies it whole. */
  private async settle(channel: Channel, upstream: Upstream, waitMs: number, abrupt = false) {
    const wait = (work: Promise<void>) => waitMs > 0 ? Promise.race([work, new Promise<void>(resolve => setTimeout(resolve, waitMs))]) : Promise.resolve()
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
   * accounted for without asking the provider: otherwise a few hundred milliseconds of trailing silence
   * would send the last durable part to the batch recogniser. Speech the provider still reports for
   * that stretch is stored all the same. */
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
   * not finalised is left to the durable parts. */
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
      // Anything but an orderly end with the client still connected leaves the capture running past the stream.
      const abrupt = code !== 1000 || !this.socket
      const upstreams: Upstream[] = [], settling: Promise<void>[] = []
      for (const channel of this.channels.values()) { clearTimeout(channel.reopenTimer); if (channel.upstream) { upstreams.push(channel.upstream); settling.push(this.settle(channel, channel.upstream, this.config.endWaitMs, abrupt || (this.endMark?.[channel.name] ?? 0) > channel.nextSample)) } }
      await Promise.all(settling)
      await this.settleWrites(this.config.endWaitMs)
      // An orderly end keeps the provider sessions, and with them the provider's voices, for the note's
      // next live session: a Stop is often followed by a Resume. Any other end closes them.
      const kept = code === 1000 && this.hold && this.config.holdMs > 0 ? upstreams.filter(upstream => !upstream.link.lost) : []
      const closing = upstreams.filter(upstream => !kept.includes(upstream))
      await Promise.all(closing.map(upstream => upstream.session.close().catch(() => undefined)))
      await Promise.race([Promise.all(closing.map(upstream => upstream.link.done)), new Promise(resolve => setTimeout(resolve, 1000))])
      await this.settleWrites(1000)
      for (const channel of this.channels.values()) { this.acknowledge(channel, performance.now(), true); channel.upstream = undefined }
      for (const upstream of kept) { upstream.link.release(); upstream.link.speakers = { ...upstream.speakers }; upstream.link.sent = upstream.timeline.upstreamSamples }
      if (kept.length) this.hold!.park(kept.map(upstream => upstream.link))
      this.ended = true
      const socket = this.socket
      this.socket = undefined
      if (socket) closeSocket(socket, code, error, message, error === "provider_unavailable" ? false : undefined)
      this.onEnded(this)
    })()
    return this.ending
  }
}

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
