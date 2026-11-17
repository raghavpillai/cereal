/**
 * A streaming recogniser that needs no network, for hermetic tests and for client development.
 *
 * Scripted mode (a `vocabulary` is given): the test audio is not sound. A sample whose value is
 * `FAKE_WORD_BASE + k` (k is 1-based) means "word k of the vocabulary is being spoken", and 0 is silence.
 * The base keeps speech well above the level the session treats as a pause. The fake hears exactly
 * the samples it is sent, so a dropped, repeated or skipped stretch of audio shows up in its transcript
 * the way it would with a real recogniser. `encodeFakeSpeech` builds such audio and `decodeFakeSpeech`
 * reads it back (the batch side of the same tests).
 *
 * Energy mode (no vocabulary): any audio works. Every 350 ms of signal above the noise floor becomes
 * the next word of a fixed text. This is what `LIVE_TRANSCRIPTION_PROVIDER=fake` serves to real clients.
 *
 * Either way an utterance ends after `silenceMs` of silence, interim text follows speech by
 * `interimDelayMs` and final text follows the end of the utterance by `finalDelayMs`.
 */
import type { LiveTranscriber, LiveTranscriberEvent, LiveTranscriberSession, LiveWord } from "@meeting-notes/contracts/live"
import { EventQueue } from "./queue"

export interface FakeWord {
  readonly text: string
  readonly speaker?: string
  /** Length of the whole word in samples. A shorter run was cut (by a flush, a skip or the edge of a part) and is transcribed as `text-`. */
  readonly samples?: number
  /** The language the fake reports a final in when most of its words carry this one (as a real provider's detection). */
  readonly language?: string
}
export const FAKE_WORD_BASE = 1000
const wordId = (sample: number, vocabulary: ReadonlyArray<FakeWord>) => sample > FAKE_WORD_BASE && sample - FAKE_WORD_BASE <= vocabulary.length ? sample - FAKE_WORD_BASE : 0
const wordText = (word: FakeWord, heardSamples: number) => word.samples !== undefined && heardSamples < word.samples ? word.text + "-" : word.text
export interface FakeTranscriberOptions {
  vocabulary?: ReadonlyArray<FakeWord>
  interimDelayMs?: number
  finalDelayMs?: number
  silenceMs?: number
  openDelayMs?: number
  /** Seconds of silence a keep-alive adds to the provider clock; 0 imitates an out-of-band keep-alive. */
  keepAliveSeconds?: number
  model?: string
  /** Speakers are reported as `speaker_0`, `speaker_1`, ... in the order each provider session first
   * hears them, as a real provider does: the same person can get another id in the next session. */
  sessionSpeakers?: boolean
  /** A real recogniser hears words in steady low noise ("Yeah.", "Right."): with this set, every 0.6 s of
   * audio that is neither silence (0) nor a scripted word is heard as this word. */
  noiseWord?: string
}
export interface FakeControl {
  /** Upstream sessions opened so far. */
  opened: number
  /** Number of the next `open` calls that fail. */
  failOpens: number
  /** Total seconds of audio received across all sessions, keep-alive silence included. */
  audioSeconds: number
  /** Makes every open session report an error. */
  fail(retryable?: boolean): void
  /** Open sessions stop reading: `bufferedBytes` grows and no events are produced. */
  stalled: boolean
  /** Flushes go unanswered, as the real service sometimes leaves one that closely follows another. */
  ignoreFlushes: boolean
  readonly active: number
}
const FILLER = "this is live transcription from the fake provider so the words you see are placeholders that follow the rhythm of your audio and stop when you stop speaking".split(" ")

export function makeFakeTranscriber(options: FakeTranscriberOptions = {}): LiveTranscriber & { control: FakeControl } {
  const interimDelay = options.interimDelayMs ?? 250, finalDelay = options.finalDelayMs ?? 400, silenceMs = options.silenceMs ?? 400
  const sessions = new Set<{ fail(retryable: boolean): void }>()
  const control: FakeControl = {
    opened: 0, failOpens: 0, audioSeconds: 0, stalled: false, ignoreFlushes: false,
    fail(retryable = true) { for (const session of [...sessions]) session.fail(retryable) },
    get active() { return sessions.size },
  }
  return {
    provider: "fake", model: options.model ?? "fake-live", control,
    async open({ sampleRate, signal }) {
      if (options.openDelayMs) await new Promise(resolve => setTimeout(resolve, options.openDelayMs))
      if (control.failOpens > 0) { control.failOpens--; throw new Error("fake provider unavailable") }
      signal.throwIfAborted()
      control.opened++
      const events = new EventQueue<LiveTranscriberEvent>(), timers = new Set<ReturnType<typeof setTimeout>>()
      const silenceSamples = Math.round(silenceMs * sampleRate / 1000)
      let position = 0, run = { value: 0, start: 0 }, silent = 0, utterance = 0, lastDue = 0, closed = false, buffered = 0, speech = 0, filler = 0
      let noise = 0
      let words: LiveWord[] = [], languages: Array<string | undefined> = []
      const ids = new Map<string, string>()
      const speakerId = (name: string) => options.sessionSpeakers ? ids.get(name) ?? (ids.set(name, `speaker_${ids.size}`), ids.get(name)!) : name
      const later = (event: LiveTranscriberEvent, delay: number) => {
        // Events keep their order even when their delays differ.
        const due = Math.max(lastDue, performance.now() + delay); lastDue = due
        const timer = setTimeout(() => { timers.delete(timer); if (!closed) events.push(event) }, Math.max(0, due - performance.now()))
        timers.add(timer)
      }
      const interim = () => { if (words.length) later({ type: "interim", utterance, text: words.map(w => w.text).join("").trim(), startSec: words[0]!.startSec, endSec: words.at(-1)!.endSec, ...(words.at(-1)!.speaker ? { speaker: words.at(-1)!.speaker } : {}) }, interimDelay) }
      const heard = (word: FakeWord, start: number, end: number) => {
        languages.push(word.language)
        words.push({ text: wordText(word, end - start) + " ", startSec: start / sampleRate, endSec: end / sampleRate, ...(word.speaker ? { speaker: speakerId(word.speaker) } : {}) })
        interim()
      }
      const commit = (through: number, delay: number, flushed = false) => {
        const done = words, spoken = languages; words = []; languages = []
        const counts = new Map<string, number>()
        for (const language of spoken) if (language) counts.set(language, (counts.get(language) ?? 0) + 1)
        const language = [...counts.entries()].sort((a, b) => b[1] - a[1])[0]?.[0]
        later({ type: "final", flushed, utterance, text: done.map(w => w.text).join("").trim(), startSec: done[0]?.startSec ?? through / sampleRate, endSec: done.at(-1)?.endSec ?? through / sampleRate, words: done, finalThroughSec: through / sampleRate, ...(language ? { language } : {}) }, delay)
        utterance++
      }
      const session: LiveTranscriberSession & { fail(retryable: boolean): void } = {
        events,
        get bufferedBytes() { return buffered },
        send(pcm) {
          if (closed) return
          if (control.stalled) { buffered += pcm.byteLength; return }
          const view = new DataView(pcm.buffer, pcm.byteOffset, pcm.byteLength), count = pcm.byteLength >> 1
          control.audioSeconds += count / sampleRate
          for (let index = 0; index < count; index++, position++) {
            const sample = view.getInt16(index * 2, true)
            if (options.vocabulary) {
              const value = wordId(sample, options.vocabulary)
              if (options.noiseWord && !value && sample !== 0) {
                if (++noise === Math.round(sampleRate * 0.6)) { heard({ text: options.noiseWord, speaker: "speaker_0" }, position + 1 - noise, position + 1); noise = 0 }
              } else noise = 0
              if (value !== run.value) {
                if (run.value) heard(options.vocabulary[run.value - 1]!, run.start, position)
                run = { value, start: position }
              }
              silent = value ? 0 : silent + 1
            } else {
              const loud = Math.abs(sample) > 500
              if (loud) { if (!speech || silent > sampleRate / 10) run.start = position; speech++; silent = 0 } else silent++
              if (loud && speech % Math.round(sampleRate * 0.35) === 0) { heard({ text: FILLER[filler++ % FILLER.length]!, speaker: "speaker_0" }, run.start, position + 1); run.start = position + 1 }
            }
            if (silent === silenceSamples && words.length) commit(Math.round(words.at(-1)!.endSec * sampleRate), finalDelay)
          }
        },
        flush() {
          if (closed || control.stalled || control.ignoreFlushes) return
          // A real recogniser asked to finalise in the middle of a word returns a broken word.
          if (options.vocabulary && run.value) { heard(options.vocabulary[run.value - 1]!, run.start, position); run = { value: 0, start: position } }
          commit(position, Math.min(finalDelay, 120), true)
        },
        keepAlive() {
          const seconds = options.keepAliveSeconds ?? 0.1
          if (seconds > 0) session.send(new Uint8Array(Math.round(seconds * sampleRate) * 2))
          return seconds
        },
        async close() {
          if (closed) return
          closed = true; sessions.delete(session)
          for (const timer of timers) clearTimeout(timer)
          events.close()
        },
        fail(retryable) { if (!closed) { events.push({ type: "error", retryable, reason: "unavailable" }); void session.close() } },
      }
      sessions.add(session)
      signal.addEventListener("abort", () => void session.close(), { once: true })
      return session
    },
  }
}

export interface FakeLine { readonly atMs: number; readonly text: string; readonly speaker?: string }
export interface FakeScript {
  readonly vocabulary: FakeWord[]
  /** PCM16 little-endian mono of the whole script, `durationMs` long. */
  readonly pcm: Uint8Array
  readonly sampleRate: number
  readonly lines: ReadonlyArray<FakeLine & { firstSample: number; lastSample: number }>
}
/** Audio for scripted mode. Every word is its own vocabulary entry, so a repeated word is still told apart. */
export function encodeFakeSpeech(lines: ReadonlyArray<FakeLine>, options: { sampleRate?: number; durationMs?: number; wordMs?: number; gapMs?: number } = {}): FakeScript {
  const sampleRate = options.sampleRate ?? 16000, wordSamples = Math.round((options.wordMs ?? 240) * sampleRate / 1000), gapSamples = Math.round((options.gapMs ?? 60) * sampleRate / 1000)
  const vocabulary: FakeWord[] = [], placed: Array<{ id: number; start: number }> = [], out: FakeScript["lines"][number][] = []
  let end = 0
  for (const line of [...lines].sort((a, b) => a.atMs - b.atMs)) {
    let cursor = Math.round(line.atMs * sampleRate / 1000)
    if (cursor < end) throw new Error(`Fake line at ${line.atMs} ms overlaps the previous line`)
    const first = cursor
    for (const text of line.text.split(/\s+/).filter(Boolean)) {
      vocabulary.push({ text, samples: wordSamples, ...(line.speaker ? { speaker: line.speaker } : {}) })
      placed.push({ id: vocabulary.length, start: cursor })
      cursor += wordSamples + gapSamples
    }
    end = cursor - gapSamples
    out.push({ ...line, firstSample: first, lastSample: end - 1 })
  }
  if (vocabulary.length > 32767 - FAKE_WORD_BASE) throw new Error("Fake script is too long")
  const total = Math.max(end, Math.round((options.durationMs ?? 0) * sampleRate / 1000)), pcm = new Uint8Array(total * 2), view = new DataView(pcm.buffer)
  for (const word of placed) for (let sample = word.start; sample < word.start + wordSamples; sample++) view.setInt16(sample * 2, FAKE_WORD_BASE + word.id, true)
  return { vocabulary, pcm, sampleRate, lines: out }
}
/** The words in a piece of scripted audio, with times in seconds from its first sample. A word cut by
 * either end of the audio is still reported, as a batch recogniser would report a clipped word. */
export function decodeFakeSpeech(pcm: Uint8Array, sampleRate: number, vocabulary: ReadonlyArray<FakeWord>): LiveWord[] {
  const view = new DataView(pcm.buffer, pcm.byteOffset, pcm.byteLength), count = pcm.byteLength >> 1, words: LiveWord[] = []
  let value = 0, start = 0
  const close = (end: number) => { if (value) { const word = vocabulary[value - 1]!; words.push({ text: wordText(word, end - start) + " ", startSec: start / sampleRate, endSec: end / sampleRate, ...(word.speaker ? { speaker: word.speaker } : {}) }) } }
  for (let index = 0; index < count; index++) {
    const next = wordId(view.getInt16(index * 2, true), vocabulary)
    if (next !== value) { close(index); value = next; start = index }
  }
  close(count)
  return words
}
