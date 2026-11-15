/**
 * Who is speaking after the provider session of a channel changed.
 *
 * A note's speaker numbers come from the streaming provider's speaker ids, and those mean something
 * only inside one provider session. Whenever a channel that already has numbered voices gets a NEW
 * provider session (Resume after the hold ran out, a connection lost for longer, a long pause, a
 * provider failure), the session decides for every voice it hears there, in this order:
 *
 *  1. Evidence. Up to 18 s of each earlier voice, cut from the note's durable audio parts, and up to
 *     12 s of what the new voice has said are transcribed as ONE file by the batch recogniser with
 *     diarisation (`makeVoiceMatcher`): pieces of the earlier voices in rotation, a piece of the new
 *     voice after each. The new voice IS an earlier voice only if at least 80% of each of the two lies
 *     in the same batch speaker and no other earlier voice touches that speaker. It is a NEW voice if
 *     its batch speaker holds none of the earlier voices and every earlier voice could be placed.
 *     Anything else is "unsure". An earlier voice is given to one new voice only.
 *  2. Channel rule, when there is no evidence (no parts yet, too little speech, a batch provider
 *     without diarisation, "unsure"): if the channel had exactly one voice and this is the first voice
 *     of the new provider session, it is that voice. This is a guess; it is wrong when the person at
 *     the microphone changed and step 1 could not tell.
 *  3. Otherwise the voice gets the next free number. It is asked about again when it has said more,
 *     and a later match renames it through the ordinary relabel path (`mergeSpeakers`, a `speakers`
 *     message). Nothing is ever split again, and two voices of one provider session never share a number.
 *
 * Why the file is laid out as turn-taking (measured on 2026-11-14, scribe_v3 batch, synthetic voices):
 * with the earlier audio simply followed by the new audio, the batch recogniser took a new voice for a
 * similar earlier one when it had not heard the two alternate (two women in 2 of 2 files; a man after
 * 5 s in a live run). With the pieces alternating it placed 15 of 15 files correctly, those pairs
 * included, and the live runs of scripts/live/voices.ts came out right. Replaying earlier audio into
 * the new streaming session instead was worse: the streaming diariser merged most same-gender voices.
 * The provider has no way to enrol a voice: the streaming API takes no reference, and the batch
 * `use_speaker_library` flag refers to a workspace library with no public API to fill it.
 *
 * What this cannot repair: two people the streaming provider itself hears as one voice inside one
 * provider session share that voice's label, here as in a recording that never stopped.
 */
import { readFile } from "node:fs/promises"
import { join } from "node:path"
import { Effect } from "effect"
import type { ProviderServices } from "../providers"
import type { NotesRepositoryService } from "../repository"
import { liveSpeakerId, type Interval } from "./coverage"
import type { AudioChannelName } from "./repository"

export interface VoiceQuestion {
  readonly noteId: string
  readonly channel: AudioChannelName
  /** Earlier voices of the channel, by speaker number, that no new voice has been matched to. */
  readonly candidates: ReadonlyArray<number>
  /** Capture audio of the new provider session: 16-bit mono PCM whose first sample is at note time `startMs`. */
  readonly fresh: { readonly pcm: Uint8Array; readonly sampleRate: number; readonly startMs: number }
  /** Where each new voice speaks, in note time, by provider speaker. */
  readonly voices: Readonly<Record<string, ReadonlyArray<Interval>>>
}
/** An earlier voice's number, "new" (none of the earlier voices), or "unsure". */
export type VoiceVerdict = number | "new" | "unsure"
export interface VoiceMatcher { match(question: VoiceQuestion): Promise<Record<string, VoiceVerdict>> }

export interface VoiceMatcherOptions {
  /** The note's stored segments and durable parts. */
  readonly segments: (noteId: string) => Promise<ReadonlyArray<{ channel: string; speakerId: string; startMs: number; endMs?: number | null }>>
  readonly parts: (noteId: string) => Promise<ReadonlyArray<{ channel: string; startMs: number; durationMs: number; hash: string }>>
  /** Directory of the durable parts (`<dir>/<note>/<hash>.wav`, as the processing service stores them). */
  readonly storageDir: string
  /** Reads one durable part from wherever audio is kept (`storage.ts`). Default: the file in `storageDir`. */
  readonly readPart?: (noteId: string, hash: string) => Promise<Uint8Array | null>
  /** The batch recogniser: speaker turns of one file, speaker in the last `:` field of `speakerId`. */
  readonly transcribe: (input: { audio: Uint8Array; channel: AudioChannelName; chunkId: string; startMs: number; durationMs: number }) => Promise<ReadonlyArray<{ speakerId: string; startMs: number; endMs?: number | null }>>
}

const RATE = 16000
/** Audio of each earlier voice and of the new voice that goes into one comparison, the length of the
 * pieces they are cut into, and how many earlier voices one comparison takes. */
const REFERENCE_MS = 18_000, NEW_MS = 12_000, OLD_PIECE_MS = 4_500, NEW_PIECE_MS = 2_500, MAX_CANDIDATES = 5
/** Speech the batch result must attribute before a voice counts as placed. */
const MIN_OLD_MS = 2_000, MIN_NEW_MS = 1_500
/** A voice is "in" a batch speaker with this share of its speech; another voice "touches" it with this share. */
const DOMINANT = 0.8, STRAY = 0.2
const ms = (samples: number) => Math.round(samples * 1000 / RATE)

/** Mono PCM (16/24/32-bit integer or 32/64-bit float) WAV to 16-bit samples. Null for anything else. */
function decodeWav(bytes: Uint8Array): { rate: number; samples: Int16Array } | null {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength)
  let format = 0, channels = 0, rate = 0, bits = 0
  for (let offset = 12; offset + 8 <= bytes.byteLength;) {
    const tag = String.fromCharCode(bytes[offset]!, bytes[offset + 1]!, bytes[offset + 2]!, bytes[offset + 3]!), size = view.getUint32(offset + 4, true), body = offset + 8
    if (tag === "fmt " && size >= 16) {
      format = view.getUint16(body, true); channels = view.getUint16(body + 2, true); rate = view.getUint32(body + 4, true); bits = view.getUint16(body + 14, true)
      if (format === 0xfffe && size >= 26) format = view.getUint16(body + 24, true)
    } else if (tag === "data") {
      if (channels !== 1 || !rate || ![16, 24, 32, 64].includes(bits)) return null
      const width = bits / 8, count = Math.floor(Math.min(size, bytes.byteLength - body) / width), samples = new Int16Array(count)
      for (let index = 0; index < count; index++) {
        const at = body + index * width
        samples[index] = format === 3 ? Math.max(-32768, Math.min(32767, Math.round((bits === 64 ? view.getFloat64(at, true) : view.getFloat32(at, true)) * 32767)))
          : bits === 16 ? view.getInt16(at, true) : bits === 24 ? view.getInt16(at + 1, true) : view.getInt16(at + 2, true)
      }
      return { rate, samples }
    }
    offset = body + size + (size & 1)
  }
  return null
}
function resample(samples: Int16Array, from: number): Int16Array {
  if (from === RATE) return samples
  const out = new Int16Array(Math.floor(samples.length * RATE / from))
  for (let index = 0; index < out.length; index++) {
    const position = index * from / RATE, left = Math.floor(position), right = Math.min(samples.length - 1, left + 1)
    out[index] = Math.round(samples[left]! + (samples[right]! - samples[left]!) * (position - left))
  }
  return out
}
function wavFile(samples: Int16Array) {
  const out = new Uint8Array(44 + samples.length * 2), view = new DataView(out.buffer)
  const text = (at: number, value: string) => { for (let index = 0; index < value.length; index++) out[at + index] = value.charCodeAt(index) }
  text(0, "RIFF"); view.setUint32(4, 36 + samples.length * 2, true); text(8, "WAVEfmt "); view.setUint32(16, 16, true); view.setUint16(20, 1, true); view.setUint16(22, 1, true)
  view.setUint32(24, RATE, true); view.setUint32(28, RATE * 2, true); view.setUint16(32, 2, true); view.setUint16(34, 16, true); text(36, "data"); view.setUint32(40, samples.length * 2, true)
  for (let index = 0; index < samples.length; index++) view.setInt16(44 + index * 2, samples[index]!, true)
  return out
}
const overlap = (a: Interval, b: Interval) => Math.max(0, Math.min(a.toMs, b.toMs) - Math.max(a.fromMs, b.fromMs))

/** Decides from the batch speaker turns of the combined file. `spoken` maps each voice ("old:<number>" or
 * "new:<provider speaker>") to where it speaks in the file. Exported for the tests. */
export function judgeVoices(turns: ReadonlyArray<{ speaker: string } & Interval>, spoken: ReadonlyMap<string, ReadonlyArray<Interval>>, candidates: ReadonlyArray<number>, voices: ReadonlyArray<string>): Record<string, VoiceVerdict> {
  const shares = (who: string) => {
    const row = new Map<string, number>()
    for (const interval of spoken.get(who) ?? []) for (const turn of turns) { const common = overlap(interval, turn); if (common > 0) row.set(turn.speaker, (row.get(turn.speaker) ?? 0) + common) }
    const total = [...row.values()].reduce((sum, value) => sum + value, 0)
    return { row, total, top: [...row.entries()].sort((a, b) => b[1] - a[1])[0] }
  }
  const home = (who: string, least: number) => { const { total, top } = shares(who); return top && total >= least && top[1] / total >= DOMINANT ? top[0] : undefined }
  const earlier = new Map(candidates.map(number => [number, { home: home(`old:${number}`, MIN_OLD_MS), ...shares(`old:${number}`) }]))
  const placed: Record<string, VoiceVerdict> = {}
  for (const voice of voices) {
    const speaker = home(`new:${voice}`, MIN_NEW_MS)
    if (!speaker) { placed[voice] = "unsure"; continue }
    const owners = candidates.filter(number => earlier.get(number)!.home === speaker)
    const touched = candidates.some(number => { const one = earlier.get(number)!; return one.home !== speaker && one.total > 0 && (one.row.get(speaker) ?? 0) / one.total >= STRAY })
    placed[voice] = owners.length === 1 && !touched ? owners[0]! : !owners.length && !touched && candidates.every(number => earlier.get(number)!.home !== undefined) ? "new" : "unsure"
  }
  return placed
}

/** The matcher over a notes repository, the directory of its durable parts and the batch recogniser the worker uses. */
export const makeNoteVoiceMatcher = (repository: Pick<NotesRepositoryService, "getNote" | "listAudioChunks">, transcribe: ProviderServices["transcribe"], storageDir: string, readPart?: (noteId: string, hash: string) => Promise<Uint8Array | null>) => makeVoiceMatcher({
  storageDir, ...(readPart ? { readPart } : {}), segments: async noteId => (await Effect.runPromise(repository.getNote(noteId)))?.transcriptSegments ?? [], parts: noteId => Effect.runPromise(repository.listAudioChunks(noteId)), transcribe: input => Effect.runPromise(transcribe(input)),
})
export function makeVoiceMatcher(options: VoiceMatcherOptions): VoiceMatcher {
  return {
    async match(question) {
      const candidates = question.candidates.slice(0, MAX_CANDIDATES), voices = Object.keys(question.voices)
      if (!candidates.length || !voices.length) return {}
      const [segments, parts] = await Promise.all([options.segments(question.noteId), options.parts(question.noteId)])
      const mine = parts.filter(part => part.channel === question.channel).sort((a, b) => a.startMs - b.startMs), decoded = new Map<string, { rate: number; samples: Int16Array } | null>()
      const audio = async (hash: string) => {
        if (!decoded.has(hash)) decoded.set(hash, await (options.readPart ? options.readPart(question.noteId, hash) : readFile(join(options.storageDir, question.noteId, `${hash}.wav`)).then(bytes => new Uint8Array(bytes))).then(bytes => bytes ? decodeWav(bytes) : null, () => null))
        return decoded.get(hash)!
      }
      /** The stored audio of a stretch of note time, or null when a part of it is not on disk. */
      const cut = async (range: Interval): Promise<Int16Array | null> => {
        const pieces: Int16Array[] = []
        for (let at = range.fromMs; at < range.toMs;) {
          const part = mine.find(item => item.startMs <= at && at < item.startMs + item.durationMs), wave = part && await audio(part.hash)
          if (!part || !wave) return null
          const end = Math.min(range.toMs, part.startMs + part.durationMs)
          pieces.push(resample(wave.samples.subarray(Math.round((at - part.startMs) * wave.rate / 1000), Math.round((end - part.startMs) * wave.rate / 1000)), wave.rate))
          at = end
        }
        const out = new Int16Array(pieces.reduce((sum, piece) => sum + piece.length, 0)); let offset = 0
        for (const piece of pieces) { out.set(piece, offset); offset += piece.length }
        return out
      }
      /** A stretch of speech as pieces of at most `pieceMs`; a short rest is left on the piece before it. */
      const pieces = (range: Interval, pieceMs: number) => { const out: Interval[] = []; for (let at = range.fromMs; at < range.toMs;) { const end = range.toMs - (at + pieceMs) < pieceMs / 2 ? range.toMs : at + pieceMs; out.push({ fromMs: at, toMs: end }); at = end } return out }
      // Each earlier voice: its longest turns first (they give the recogniser the most to go on), as pieces.
      const earlier: Array<{ number: number; pieces: Int16Array[] }> = []
      for (const number of candidates) {
        const turns = segments.filter(segment => segment.channel === question.channel && segment.speakerId === liveSpeakerId(question.channel, number) && (segment.endMs ?? segment.startMs) - segment.startMs >= 1000 && (segment.endMs ?? 0) <= question.fresh.startMs)
          .map(segment => ({ fromMs: segment.startMs, toMs: segment.endMs! })).sort((a, b) => (b.toMs - b.fromMs) - (a.toMs - a.fromMs))
        const held: Int16Array[] = []
        let taken = 0
        for (const turn of turns) for (const piece of pieces({ fromMs: turn.fromMs, toMs: Math.min(turn.toMs, turn.fromMs + REFERENCE_MS - taken) }, OLD_PIECE_MS)) {
          if (taken >= REFERENCE_MS) break
          const samples = await cut(piece)
          if (samples?.length) { held.push(samples); taken += ms(samples.length) }
        }
        earlier.push({ number, pieces: held })
      }
      // None of the earlier voices has stored audio: there is nothing to compare with.
      if (!earlier.some(voice => voice.pieces.length)) return {}
      const fresh = question.fresh, view = new DataView(fresh.pcm.buffer, fresh.pcm.byteOffset, fresh.pcm.byteLength), raw = new Int16Array(fresh.pcm.byteLength >> 1)
      for (let index = 0; index < raw.length; index++) raw[index] = view.getInt16(index * 2, true)
      const freshMs = raw.length * 1000 / fresh.sampleRate
      const placed: Record<string, VoiceVerdict> = {}
      // One file per new voice: the earlier voices' pieces in rotation with a piece of the new voice after
      // each, so the recogniser hears the new voice take turns with every voice it might be.
      await Promise.all(voices.map(async voice => {
        let budget = NEW_MS
        const own = [...question.voices[voice]!].reverse().flatMap(interval => {
          const range = { fromMs: Math.max(0, interval.fromMs - fresh.startMs), toMs: Math.min(freshMs, interval.toMs - fresh.startMs) }
          if (range.toMs - range.fromMs < 500 || budget <= 0) return []
          const kept = { fromMs: Math.max(range.fromMs, range.toMs - budget), toMs: range.toMs }
          budget -= kept.toMs - kept.fromMs
          return pieces(kept, NEW_PIECE_MS)
        }).map(range => resample(raw.subarray(Math.round(range.fromMs * fresh.sampleRate / 1000), Math.round(range.toMs * fresh.sampleRate / 1000)), fresh.sampleRate))
        const file: Int16Array[] = [], spoken = new Map<string, Interval[]>(), next = earlier.map(() => 0)
        let length = 0, used = 0
        const add = (samples: Int16Array, who: string) => { spoken.set(who, [...(spoken.get(who) ?? []), { fromMs: ms(length), toMs: ms(length + samples.length) }]); file.push(samples, new Int16Array(RATE * 0.35)); length += samples.length + RATE * 0.35 }
        for (let turn = 0; earlier.some((one, index) => next[index]! < one.pieces.length); turn++) {
          const index = turn % earlier.length, one = earlier[index]!
          if (next[index]! >= one.pieces.length) continue
          add(one.pieces[next[index]!++]!, `old:${one.number}`)
          if (used < own.length && turn >= earlier.length - 1) add(own[used++]!, `new:${voice}`)
        }
        while (used < own.length) add(own[used++]!, `new:${voice}`)
        const combined = new Int16Array(length); let offset = 0
        for (const piece of file) { combined.set(piece, offset); offset += piece.length }
        const turns = await options.transcribe({ audio: wavFile(combined), channel: question.channel, chunkId: "voices", startMs: 0, durationMs: ms(length) })
        placed[voice] = judgeVoices(turns.map(turn => ({ speaker: turn.speakerId.slice(turn.speakerId.lastIndexOf(":") + 1), fromMs: turn.startMs, toMs: turn.endMs ?? turn.startMs })).filter(turn => turn.speaker !== "unknown" && turn.toMs > turn.fromMs), spoken, candidates, [voice])[voice] ?? "unsure"
      }))
      // Two voices the streaming provider keeps apart cannot both be the same earlier voice: one of the two recognisers is wrong.
      for (const voice of voices) if (typeof placed[voice] === "number" && voices.some(other => other !== voice && placed[other] === placed[voice])) for (const other of voices.filter(item => placed[item] === placed[voice])) placed[other] = "unsure"
      return placed
    },
  }
}
