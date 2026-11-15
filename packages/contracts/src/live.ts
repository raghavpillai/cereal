/**
 * Wire protocol of the live transcription socket, `GET /api/v1/notes/:id/live`.
 *
 * Text frames are JSON and audio frames are binary. The schemas here are what
 * the server validates and emits.
 */
import { Schema } from "effect"
import { AudioChannel, TranscriptSegmentDTO, UUID } from "./index"

export const LIVE_PROTOCOL_VERSION = 1
/** Bytes before the PCM16 little-endian samples in a binary audio frame. */
export const LIVE_FRAME_HEADER_BYTES = 12
/** A frame larger than this (header included) closes the socket with 4400. */
export const LIVE_MAX_FRAME_BYTES = 64 * 1024
/** PCM rates a `hello` may declare. Any other rate is refused with 4400. */
export const LIVE_SAMPLE_RATES = [8000, 16000, 22050, 24000, 44100, 48000] as const

export const LiveCloseCode = {
  malformed: 4400,
  unauthenticated: 4401,
  notFound: 4404,
  clientTooSlow: 4408,
  replaced: 4409,
  overLimit: 4429,
  providerUnavailable: 4503,
} as const
export type LiveCloseCode = typeof LiveCloseCode[keyof typeof LiveCloseCode]

const Milliseconds = Schema.Number.check(Schema.isInt(), Schema.isGreaterThanOrEqualTo(0))
/** Index of a sample within one capture's channel. Unsigned; at most 2^53 - 1 in JSON. */
const SampleIndex = Schema.Number.check(Schema.isInt(), Schema.isGreaterThanOrEqualTo(0))
const Revision = Schema.Number.check(Schema.isInt32(), Schema.isGreaterThanOrEqualTo(0))

// ---------------------------------------------------------------- client to server

export const LiveStreamHello = Schema.Struct({
  channel: AudioChannel,
  /** Rate of the PCM this client sends for the channel. 16000 is preferred; 8000 to 48000 is accepted. */
  sampleRate: Schema.Number.check(Schema.isInt(), Schema.isBetween({ minimum: 8000, maximum: 48000 })),
  encoding: Schema.Literal("pcm_s16le"),
  /** Note-timeline position of sample 0 of this capture. */
  baseMs: Milliseconds,
  /** The last `ack.throughSample` the client saw for this capture and channel, or 0. */
  resumeFromSample: SampleIndex,
}).annotate({ identifier: "LiveStreamHello" })
export type LiveStreamHello = typeof LiveStreamHello.Type

/** First frame on every connection, including reconnects. */
export const LiveHelloMessage = Schema.Struct({
  type: Schema.Literal("hello"),
  protocol: Schema.Literal(LIVE_PROTOCOL_VERSION),
  /** Stable for one capture. The same value on a second connection marks it as a reconnect. */
  captureId: Schema.String.check(Schema.isMinLength(1), Schema.isMaxLength(200)),
  /** ISO 639 code; the provider detects the language when absent. */
  language: Schema.optionalKey(Schema.String.check(Schema.isMinLength(2), Schema.isMaxLength(16))),
  streams: Schema.Array(LiveStreamHello).check(Schema.isMinLength(1), Schema.isMaxLength(2)),
}).annotate({ identifier: "LiveHelloMessage" })
export type LiveHelloMessage = typeof LiveHelloMessage.Type

export const LiveMarkKind = Schema.Literals(["pause", "resume", "end"]).annotate({ identifier: "LiveMarkKind" })
export const LiveMarkMessage = Schema.Struct({
  type: Schema.Literal("mark"),
  kind: LiveMarkKind,
  /** Per channel, the number of samples the capture holds at the mark (one past the last sample). */
  atSample: Schema.Struct({ mic: Schema.optionalKey(SampleIndex), system: Schema.optionalKey(SampleIndex) }),
}).annotate({ identifier: "LiveMarkMessage" })
export type LiveMarkMessage = typeof LiveMarkMessage.Type

export const LiveClientMessage = Schema.Union([LiveHelloMessage, LiveMarkMessage])
export type LiveClientMessage = typeof LiveClientMessage.Type

// ---------------------------------------------------------------- server to client

export const LiveReadyMessage = Schema.Struct({
  type: Schema.Literal("ready"),
  sessionId: UUID,
  provider: Schema.String,
  model: Schema.String,
  /** Per channel, the first sample the server wants. Never earlier than the hello's `resumeFromSample`
   * unless the server lost its state, in which case it is what the server has stored coverage up to. */
  streams: Schema.Array(Schema.Struct({ channel: AudioChannel, acceptFromSample: SampleIndex }).annotate({ identifier: "LiveReadyStream" })),
  /** A backlog longer than this must be skipped by the client rather than replayed. */
  resumeWindowMs: Milliseconds,
  transcriptRevision: Revision,
}).annotate({ identifier: "LiveReadyMessage" })
export type LiveReadyMessage = typeof LiveReadyMessage.Type

export const LiveAckMessage = Schema.Struct({
  type: Schema.Literal("ack"),
  channel: AudioChannel,
  /** One past the last sample the server has taken; the value to resume from. */
  throughSample: SampleIndex,
  /** Note-timeline end of stored final text on this channel. */
  transcribedThroughMs: Milliseconds,
}).annotate({ identifier: "LiveAckMessage" })
export type LiveAckMessage = typeof LiveAckMessage.Type

/** Text that is still being spoken. It replaces the previous `interim` with the same `utteranceId`
 * (higher `rev` wins) and is discarded when a `final` with that `utteranceId` arrives. Never stored. */
export const LiveInterimMessage = Schema.Struct({
  type: Schema.Literal("interim"),
  channel: AudioChannel,
  utteranceId: UUID,
  rev: Revision,
  text: Schema.String,
  startMs: Milliseconds,
  endMs: Milliseconds,
  speakerId: Schema.optionalKey(Schema.String),
  speakerLabel: Schema.optionalKey(Schema.String),
}).annotate({ identifier: "LiveInterimMessage" })
export type LiveInterimMessage = typeof LiveInterimMessage.Type

/** One stored transcript segment. An utterance with several speaker turns produces several
 * `final` messages with the same `utteranceId`; the first one ends the utterance's interim text. */
export const LiveFinalMessage = Schema.Struct({
  type: Schema.Literal("final"),
  utteranceId: UUID,
  transcriptRevision: Revision,
  segment: TranscriptSegmentDTO,
}).annotate({ identifier: "LiveFinalMessage" })
export type LiveFinalMessage = typeof LiveFinalMessage.Type

export const LiveSpeakersMessage = Schema.Struct({
  type: Schema.Literal("speakers"),
  /** Stored segments whose `speakerId` is `from` now carry `to`. */
  merges: Schema.Array(Schema.Struct({ from: Schema.String, to: Schema.String }).annotate({ identifier: "LiveSpeakerMerge" })),
}).annotate({ identifier: "LiveSpeakersMessage" })
export type LiveSpeakersMessage = typeof LiveSpeakersMessage.Type

export const LiveGapReason = Schema.Literals(["resume_window_exceeded", "provider_unavailable"]).annotate({ identifier: "LiveGapReason" })
/** Audio the live path will not transcribe. The durable part covering it fills it later. */
export const LiveGapMessage = Schema.Struct({
  type: Schema.Literal("gap"),
  channel: AudioChannel,
  fromMs: Milliseconds,
  toMs: Milliseconds,
  reason: LiveGapReason,
}).annotate({ identifier: "LiveGapMessage" })
export type LiveGapMessage = typeof LiveGapMessage.Type

export const LiveStreamState = Schema.Literals(["live", "catching_up", "provider_unavailable", "paused"]).annotate({ identifier: "LiveStreamState" })
export type LiveStreamState = typeof LiveStreamState.Type
export const LiveStatusMessage = Schema.Struct({
  type: Schema.Literal("status"),
  channel: AudioChannel,
  state: LiveStreamState,
  /** The recogniser's delay in milliseconds: speech the server has received that is in neither interim
   * nor final text yet. It does not include an utterance still in progress, trailing silence, or audio the
   * client has not sent yet; a client showing "N s behind" adds its own unsent backlog to it. While the
   * state is `provider_unavailable` it is the audio received since transcription stopped. */
  lagMs: Milliseconds,
}).annotate({ identifier: "LiveStatusMessage" })
export type LiveStatusMessage = typeof LiveStatusMessage.Type

export const LiveErrorCode = Schema.Literals(["malformed", "unauthenticated", "not_found", "client_too_slow", "replaced", "over_limit", "provider_unavailable"]).annotate({ identifier: "LiveErrorCode" })
export const LiveErrorMessage = Schema.Struct({
  type: Schema.Literal("error"),
  code: LiveErrorCode,
  message: Schema.String,
  retryable: Schema.Boolean,
  retryAfterMs: Schema.optionalKey(Milliseconds),
}).annotate({ identifier: "LiveErrorMessage" })
export type LiveErrorMessage = typeof LiveErrorMessage.Type

export const LiveServerMessage = Schema.Union([
  LiveReadyMessage, LiveAckMessage, LiveInterimMessage, LiveFinalMessage,
  LiveSpeakersMessage, LiveGapMessage, LiveStatusMessage, LiveErrorMessage,
])
export type LiveServerMessage = typeof LiveServerMessage.Type

/** Roots for the Swift and Rust generators, one per message so each becomes a plain struct. */
export const LiveWireSchemas = {
  LiveHelloMessage, LiveMarkMessage, LiveReadyMessage, LiveAckMessage, LiveInterimMessage, LiveFinalMessage,
  LiveSpeakersMessage, LiveGapMessage, LiveStatusMessage, LiveErrorMessage,
} as const

// ---------------------------------------------------------------- binary audio frame

export const liveChannelCode = { mic: 0, system: 1 } as const
export type LiveAudioFrame = { channel: "mic" | "system"; firstSample: number; pcm: Uint8Array }

/** Header: byte 0 protocol version, byte 1 channel (0 microphone, 1 system), bytes 2-3 zero,
 * bytes 4-11 index of the first sample as an unsigned 64-bit little-endian integer. */
export function encodeLiveAudioFrame(frame: LiveAudioFrame): Uint8Array {
  const bytes = new Uint8Array(LIVE_FRAME_HEADER_BYTES + frame.pcm.byteLength)
  const view = new DataView(bytes.buffer)
  view.setUint8(0, LIVE_PROTOCOL_VERSION)
  view.setUint8(1, liveChannelCode[frame.channel])
  view.setBigUint64(4, BigInt(frame.firstSample), true)
  bytes.set(frame.pcm, LIVE_FRAME_HEADER_BYTES)
  return bytes
}

/** Returns undefined for anything that is not a well-formed frame; the server answers that with 4400. */
export function decodeLiveAudioFrame(bytes: Uint8Array): LiveAudioFrame | undefined {
  if (bytes.byteLength <= LIVE_FRAME_HEADER_BYTES || bytes.byteLength > LIVE_MAX_FRAME_BYTES) return undefined
  if ((bytes.byteLength - LIVE_FRAME_HEADER_BYTES) % 2 !== 0) return undefined
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength)
  if (view.getUint8(0) !== LIVE_PROTOCOL_VERSION || view.getUint16(2, true) !== 0) return undefined
  const code = view.getUint8(1)
  if (code !== 0 && code !== 1) return undefined
  const first = view.getBigUint64(4, true)
  if (first > BigInt(Number.MAX_SAFE_INTEGER)) return undefined
  return { channel: code === 0 ? "mic" : "system", firstSample: Number(first), pcm: bytes.subarray(LIVE_FRAME_HEADER_BYTES) }
}

// ---------------------------------------------------------------- server-side provider boundary

/** One word of a final transcript, in seconds of audio since the upstream session opened.
 * `text` carries the spacing that follows the word, so concatenating words gives the transcript. */
export type LiveWord = { text: string; startSec: number; endSec: number; speaker?: string }
/** `utterance` is the provider's own count of utterances in this upstream session: every interim
 * revision of an utterance and its final carry the same number. */
export type LiveTranscriberEvent =
  | { type: "interim"; text: string; startSec: number; endSec: number; speaker?: string; utterance?: number }
  /** `finalThroughSec`: all audio before this point is accounted for by this and earlier finals.
   * `text` is empty when the provider confirmed a stretch of silence. */
  | { type: "final"; text: string; startSec: number; endSec: number; words?: ReadonlyArray<LiveWord>; finalThroughSec: number; utterance?: number
      /** Whether this final answers a `flush()`: true or false when the adapter knows, absent when it cannot tell. */
      flushed?: boolean }
  | { type: "speakers_merged"; from: string; to: string }
  /** `detail` names the provider's error kind for the server log; it never carries provider text. */
  | { type: "error"; retryable: boolean; reason?: "unavailable" | "over_limit" | "not_configured"; detail?: string }

/** A streaming speech-to-text session for one audio channel. Provider keys never leave the server. */
export interface LiveTranscriberSession {
  /** PCM16 little-endian mono at the rate the session was opened with, in upstream-session order. */
  send(pcm: Uint8Array): void
  /** Ask the provider to finalise what it has heard. Answered by a `final`, possibly an empty one. */
  flush(): void
  /** Keeps an idle session open. Returns the seconds of silence this added to the provider's clock
   * (0 for a provider with an out-of-band keep-alive), so the caller can keep its timeline right. */
  keepAlive(): number
  close(): Promise<void>
  /** Bytes accepted by `send` that have not reached the provider yet. */
  readonly bufferedBytes: number
  readonly events: AsyncIterable<LiveTranscriberEvent>
}
export interface LiveTranscriber {
  readonly provider: string
  readonly model: string
  open(options: { channel: "mic" | "system"; sampleRate: number; language?: string; keyterms?: ReadonlyArray<string>; signal: AbortSignal }): Promise<LiveTranscriberSession>
}
