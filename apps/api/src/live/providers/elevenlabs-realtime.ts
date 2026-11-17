/**
 * ElevenLabs realtime speech-to-text (Scribe v3 Live, Scribe v2 Realtime).
 *
 * Everything here was checked against the real service on 2026-11-14. The points the code depends on:
 *
 *  - `partial_transcript` carries the whole uncommitted utterance. Each update arrives up to three
 *    times within a few milliseconds: bare, then with word times, then with draft speakers.
 *  - A commit produces `committed_transcript`, `committed_transcript_with_timestamps` and, with
 *    diarisation, `diarized_transcript` (same `segment_index`), the last two in either order.
 *  - With the default 1.5 s silence threshold a continuous conversation was only committed by the
 *    36-second safety net, so the threshold is set to 0.4 s: commits then follow each sentence.
 *  - A commit message with empty audio is answered in about 100 ms, also when nothing is pending (an
 *    empty transcript). It is in-band, so it marks exactly how much audio is final. Sent mid-word it
 *    cuts the word in two; the session only flushes in a pause.
 *  - Timestamps count every sample sent, silence included. The service closes a session that receives
 *    no message for 15 seconds. An audio chunk with no samples is a message: it kept a session open through
 *    30 s and 5 min of nothing else, and the sentence the stall cut came back whole (measured 2026-11-17,
 *    `.local/ship/accuracy/`). So a keep-alive adds nothing to the provider's clock.
 *  - Audio sent at four times real time was transcribed normally. A backlog sent in one burst (26 s in about
 *    100 ms) was not: the service answered with partials for its start, then `queue_overflow`, and dropped the
 *    rest. So audio is forwarded at most `PACE` times real time (plus a short burst), however fast it arrives,
 *    and after a `queue_overflow` no commit is trusted to have covered anything past its last word.
 *
 * The key is read on every open and used only in the upgrade request header.
 */
import type { LiveTranscriber, LiveTranscriberEvent, LiveTranscriberSession, LiveWord } from "@meeting-notes/contracts/live"
import { EventQueue, LiveProviderError } from "./queue"

export const ELEVENLABS_LIVE_MODELS = ["scribe_v3_live", "scribe_v2_realtime"] as const
/** How much faster than real time audio is forwarded, and how much may go at once. */
export const PACE = 4, BURST_SECONDS = 1
/** The least uncommitted audio the service accepts a commit for (with less it closes the session). A margin over its 0.3 s. */
export const MIN_COMMIT_SECONDS = 0.35
/** Audio waiting to be forwarded beyond this is a client that does not pace itself: the session is dropped
 * (retryable), and the client replays from what is stored. */
const MAX_QUEUED_SECONDS = 120
const RATES = [8000, 16000, 22050, 24000, 44100, 48000]
const record = (value: unknown): value is Record<string, unknown> => typeof value === "object" && value !== null && !Array.isArray(value)

export interface ElevenLabsRealtimeOptions {
  apiKey: () => Promise<string> | string
  model: string
  /** Silence that ends an utterance. Measured: 0.4 s commits each sentence of a conversation. */
  vadSilenceSeconds?: number
  endpoint?: string
  /** Replaced in tests. Must accept `{ headers }` as Bun's WebSocket does. */
  webSocket?: new (url: string, options: { headers: Record<string, string> }) => WebSocket
  /** Replaced in tests: the clock pacing reads, in milliseconds, and the pace itself. */
  now?: () => number
  pace?: number
}

/** Provider words to ours: spacing is folded into the word before it, audio events are dropped. */
function words(raw: unknown): LiveWord[] {
  const out: LiveWord[] = []
  if (!Array.isArray(raw)) return out
  for (const word of raw) {
    if (!record(word) || typeof word.text !== "string") continue
    if (word.type === "spacing") { if (out.length) out[out.length - 1]!.text += word.text; continue }
    if (word.type !== "word" || typeof word.start !== "number" || typeof word.end !== "number" || !Number.isFinite(word.start) || !Number.isFinite(word.end)) continue
    out.push({ text: word.text, startSec: word.start, endSec: Math.max(word.start, word.end), ...(typeof word.speaker_id === "string" && word.speaker_id ? { speaker: word.speaker_id } : {}) })
  }
  return out
}

export function makeElevenLabsRealtime(options: ElevenLabsRealtimeOptions): LiveTranscriber {
  const diarize = options.model === "scribe_v3_live", Socket = options.webSocket ?? (WebSocket as unknown as NonNullable<ElevenLabsRealtimeOptions["webSocket"]>)
  return {
    provider: "elevenlabs", model: options.model,
    async open({ sampleRate, language, keyterms, signal }) {
      if (!RATES.includes(sampleRate)) throw new LiveProviderError("unavailable", false)
      const key = await options.apiKey()
      if (!key) throw new LiveProviderError("not_configured", false)
      const query = new URLSearchParams({
        model_id: options.model, audio_format: `pcm_${sampleRate}`, include_timestamps: "true", include_language_detection: "true",
        commit_strategy: diarize ? "turn_prediction" : "vad", vad_silence_threshold_secs: String(options.vadSilenceSeconds ?? 0.4),
        ...(diarize ? { diarize: "true" } : {}), ...(language ? { language_code: language } : {}),
      })
      for (const term of keyterms ?? []) query.append("keyterms", term)
      const socket = new Socket(`${options.endpoint ?? "wss://api.elevenlabs.io/v1/speech-to-text/realtime"}?${query}`, { headers: { "xi-api-key": key } })
      const events = new EventQueue<LiveTranscriberEvent>()
      type Pending = { utterance: number; index: number | null; text: string; timed?: LiveWord[]; diarized?: LiveWord[]; language?: string; at: number }
      const pending: Pending[] = [], manual: number[] = []
      let sent = 0, commits = 0, finalThrough = 0, closed = false, started = false, overflowed = false
      // Where the newest commit stands, in samples sent or queued: the service's own (seen when its transcript arrives) or ours.
      let committedAt = 0
      // Audio (and in-band commits) not forwarded yet, oldest first; `credit` is how many samples may go now.
      const queue: Array<Uint8Array | "commit"> = []
      let queuedSamples = 0, credit = sampleRate * BURST_SECONDS, creditAt = (options.now ?? performance.now.bind(performance))(), pump: ReturnType<typeof setInterval> | undefined
      let last = { text: "", start: 0, end: 0, speaker: undefined as string | undefined }, held: ReturnType<typeof setTimeout> | undefined, sweep: ReturnType<typeof setInterval> | undefined
      const seconds = (samples: number) => samples / sampleRate
      const shut = () => { if (closed) return; closed = true; clearTimeout(held); clearInterval(sweep); clearInterval(pump); queue.length = 0; queuedSamples = 0; events.close(); try { socket.close() } catch { /* already closed */ } }
      // Why the session ended, so an open refused for being over the account's limit says so (the hub then lets
      // held sessions of other notes go and tries again, instead of the client waiting out a back-off).
      let failure: LiveProviderError | undefined
      const fail = (error: LiveProviderError) => { if (!closed) { failure = error; events.push({ type: "error", retryable: error.retryable, reason: error.reason, ...(error.detail ? { detail: error.detail } : {}) }); shut() } }
      const drain = (force = false) => {
        while (pending.length) {
          const head = pending[0]!
          if (!(head.timed && (!diarize || head.diarized)) && !(force || performance.now() - head.at > 1500)) break
          pending.shift()
          const list = head.diarized ?? head.timed ?? [], lastWordEnd = list.at(-1)?.endSec
          // A flush is in-band: everything sent before it is final once it is answered. Its position is
          // matched to this commit only if it lies at or after the commit's last word; an older entry
          // belongs to a flush the service did not act on, and using it would understate coverage.
          while (manual.length && lastWordEnd !== undefined && manual[0]! < lastWordEnd - 0.05) manual.shift()
          const flushedAt = manual.shift()
          // After an overflow the service has dropped audio: a commit only accounts for the words it returned.
          finalThrough = Math.max(finalThrough, lastWordEnd ?? 0, overflowed ? 0 : flushedAt ?? 0)
          events.push({ type: "final", flushed: flushedAt !== undefined, utterance: head.utterance, text: head.text, words: list, startSec: list[0]?.startSec ?? finalThrough, endSec: list.at(-1)?.endSec ?? finalThrough, finalThroughSec: finalThrough, ...(head.language ? { language: head.language } : {}) })
        }
      }
      const interim = (text: string, list: LiveWord[]) => {
        let speaker: string | undefined
        for (const word of list) if (word.speaker) speaker = word.speaker
        const start = list[0]?.startSec ?? last.start, end = list.at(-1)?.endSec ?? Math.max(last.end, start)
        if (text === last.text && speaker === last.speaker) return
        last = { text, start, end, speaker }
        events.push({ type: "interim", utterance: commits, text, startSec: start, endSec: end, ...(speaker ? { speaker } : {}) })
      }
      const handle = (message: Record<string, unknown>) => {
        const type = message.message_type
        if (type === "partial_transcript" && typeof message.text === "string") {
          const text = message.text.trim(), list = words(message.words)
          clearTimeout(held)
          // The bare copy of an update has no times yet; the timed copy follows within milliseconds.
          if (text && !list.length) held = setTimeout(() => { if (!closed) interim(text, [{ text, startSec: last.text ? last.start : seconds(sent), endSec: seconds(sent) }]) }, 120)
          else if (text || last.text) interim(text, list)
        } else if (type === "committed_transcript" && typeof message.text === "string") {
          committedAt = Math.max(committedAt, sent)
          clearTimeout(held)
          pending.push({ utterance: commits++, index: typeof message.segment_index === "number" ? message.segment_index : null, text: message.text.trim(), at: performance.now() })
          last = { text: "", start: 0, end: 0, speaker: undefined }
        } else if (type === "committed_transcript_with_timestamps") {
          const target = pending.find(item => !item.timed)
          if (target) { target.timed = words(message.words); if (typeof message.language_code === "string" && message.language_code) target.language = message.language_code }
          drain()
        } else if (type === "diarized_transcript") {
          const target = pending.find(item => !item.diarized && (item.index === null || item.index === message.segment_index)) ?? pending.find(item => !item.diarized)
          if (target) target.diarized = words(message.words)
          drain()
        } else if (type === "speakers_merged" && typeof message.from_speaker_id === "string" && typeof message.to_speaker_id === "string") {
          events.push({ type: "speakers_merged", from: message.from_speaker_id, to: message.to_speaker_id })
        } else if (type === "commit_throttled") manual.shift()
        else if (type === "auth_error" || type === "unaccepted_terms") fail(new LiveProviderError("not_configured", false, type))
        else if (type === "quota_exceeded") fail(new LiveProviderError("over_limit", false, type))
        else if (type === "queue_overflow") { overflowed = true; manual.length = 0; fail(new LiveProviderError("over_limit", true, type)) }
        else if (type === "rate_limited" || type === "resource_exhausted") fail(new LiveProviderError("over_limit", true, type))
        else if (typeof type === "string" && (type === "error" || type.endsWith("_error") || ["session_time_limit_exceeded", "invalid_request", "chunk_size_exceeded", "insufficient_audio_activity"].includes(type))) fail(new LiveProviderError("unavailable", type !== "invalid_request", type))
      }
      const chunk = (pcm: Uint8Array, commit: boolean) => socket.send(JSON.stringify({ message_type: "input_audio_chunk", audio_base_64: Buffer.from(pcm.buffer, pcm.byteOffset, pcm.byteLength).toString("base64"), commit, sample_rate: sampleRate }))
      /** Forwards queued audio as the pace allows, splitting a piece when only part of it may go. */
      const forward = () => {
        if (closed) return
        const now = (options.now ?? performance.now.bind(performance))()
        credit = Math.min(sampleRate * BURST_SECONDS, credit + (now - creditAt) / 1000 * sampleRate * (options.pace ?? PACE)); creditAt = now
        while (queue.length) {
          const head = queue[0]!
          if (head === "commit") { queue.shift(); manual.push(seconds(sent)); chunk(new Uint8Array(0), true); continue }
          const samples = head.byteLength >> 1
          if (credit < 1) break
          const take = Math.min(samples, Math.floor(credit))
          const piece = take === samples ? head : head.subarray(0, take * 2)
          if (take === samples) queue.shift(); else queue[0] = head.subarray(take * 2)
          chunk(piece, false); sent += take; queuedSamples -= take; credit -= take
        }
      }
      await new Promise<void>((resolve, reject) => {
        const timer = setTimeout(() => reject(new LiveProviderError("unavailable", true)), 8000)
        const abort = () => reject(new LiveProviderError("unavailable", true))
        signal.addEventListener("abort", abort, { once: true })
        socket.onmessage = event => {
          let message: unknown
          try { message = JSON.parse(String(event.data)) } catch { return }
          if (!record(message)) return
          if (!started) {
            if (message.message_type !== "session_started") { handle(message); if (closed) reject(failure ?? new LiveProviderError("unavailable", true)); return }
            started = true; clearTimeout(timer); signal.removeEventListener("abort", abort); resolve(); return
          }
          handle(message)
        }
        // The upgrade failing (bad key, plan without access, network) surfaces here without a body.
        socket.onerror = () => { if (!started) reject(failure ?? new LiveProviderError("unavailable", true)) }
        socket.onclose = event => {
          // (Closing it ourselves after a refusal, `failure` says why.)
          if (!started) reject(failure ?? new LiveProviderError(event.code === 1008 || event.code === 3000 ? "not_configured" : "unavailable", true))
          else if (!closed) { drain(true); fail(new LiveProviderError("unavailable", true, `closed_${event.code}`)) }
        }
      }).catch(error => { shut(); throw error })
      sweep = setInterval(() => drain(), 500)
      pump = setInterval(forward, 20)
      signal.addEventListener("abort", shut, { once: true })
      const session: LiveTranscriberSession = {
        events,
        // Audio waiting for its pace is not a provider that stopped reading: only the socket's own buffer counts.
        get bufferedBytes() { return socket.bufferedAmount },
        send(pcm) {
          if (closed || !pcm.byteLength) return
          queue.push(pcm.slice()); queuedSamples += pcm.byteLength >> 1
          if (queuedSamples > sampleRate * MAX_QUEUED_SECONDS) { fail(new LiveProviderError("over_limit", true, "client_burst")); return }
          forward()
        },
        flush() {
          if (closed) return
          // Nothing has been sent since the session opened: there is nothing to commit, and the service
          // answers a commit without audio with an error. Confirm it locally instead.
          if (!sent && !queue.length) { events.push({ type: "final", flushed: true, utterance: commits, text: "", words: [], startSec: 0, endSec: 0, finalThroughSec: 0 }); return }
          // The service closes the session when asked to commit less than 0.3 s of audio ("commit_throttled", measured
          // 2026-11-17): a held provider session taken over by the note's next live session was lost that way every
          // time. With that little since the last commit there is nothing to finalise: answered here, claiming nothing (as
          // no utterance, so the one in progress keeps its interim).
          const position = sent + queuedSamples
          if (position - committedAt < sampleRate * MIN_COMMIT_SECONDS) { events.push({ type: "final", flushed: true, utterance: -1, text: "", words: [], startSec: finalThrough, endSec: finalThrough, finalThroughSec: finalThrough }); return }
          committedAt = position
          // In band, behind the audio still waiting for its pace.
          queue.push("commit"); forward()
        },
        get queuedSeconds() { return queuedSamples / sampleRate },
        keepAlive() {
          // An audio chunk with no samples: the service counts it as a message, and its clock does not move.
          // Silence would be spliced into whatever was being said when the client's audio stopped (a network
          // drop mid-word came back as "this- this-"). Audio still waiting for its pace keeps the socket busy.
          if (!closed && !queue.length) socket.send(JSON.stringify({ message_type: "input_audio_chunk", audio_base_64: "", commit: false, sample_rate: sampleRate }))
          return 0
        },
        async close() { drain(true); shut() },
      }
      return session
    },
  }
}
