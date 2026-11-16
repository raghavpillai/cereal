/**
 * Interval arithmetic for the coverage ledger, and segments built from speaker turns. Pure: both
 * repositories use these, so memory and PostgreSQL behave the same.
 *
 * All times are note-timeline milliseconds. Intervals are half-open: [fromMs, toMs).
 */
import type { TranscriptSegmentDTO } from "@meeting-notes/contracts"

export interface Interval { readonly fromMs: number; readonly toMs: number }

/** Sorted, non-overlapping intervals; neighbours closer than `joinWithin` are joined. Empty intervals vanish. */
export function mergeIntervals(intervals: ReadonlyArray<Interval>, joinWithin = 0): Interval[] {
  const sorted = intervals.filter(i => i.toMs > i.fromMs).map(i => ({ fromMs: i.fromMs, toMs: i.toMs })).sort((a, b) => a.fromMs - b.fromMs)
  const out: Array<{ fromMs: number; toMs: number }> = []
  for (const interval of sorted) {
    const last = out.at(-1)
    if (last && interval.fromMs <= last.toMs + joinWithin) last.toMs = Math.max(last.toMs, interval.toMs)
    else out.push(interval)
  }
  return out
}

export interface TimedWord { readonly text: string; readonly startMs: number; readonly endMs: number }
export interface SpeakerTurn { readonly speakerKey: string; readonly words: ReadonlyArray<TimedWord> }

/** Segments from speaker turns, one per turn; a turn longer than `maxCharacters` is cut at a word. Word
 * text carries its own trailing spacing. */
export function segmentsFromTurns(input: {
  channel: "mic" | "system"
  turns: ReadonlyArray<SpeakerTurn>
  speaker: (speakerKey: string) => { speakerId: string; speakerLabel: string | null }
  maxCharacters?: number
}): TranscriptSegmentDTO[] {
  const out: TranscriptSegmentDTO[] = [], limit = input.maxCharacters ?? 18_000
  for (const turn of input.turns) {
    let current: { text: string; startMs: number; endMs: number } | undefined
    const flush = () => {
      if (current?.text.trim()) out.push({ id: crypto.randomUUID(), channel: input.channel, ...input.speaker(turn.speakerKey), text: current.text.trim(), isFinal: true, startMs: Math.max(0, Math.round(current.startMs)), endMs: Math.max(0, Math.round(Math.max(current.startMs, current.endMs))) })
      current = undefined
    }
    for (const word of turn.words) {
      if (current && current.text.length + word.text.length > limit) flush()
      if (!current) current = { text: word.text, startMs: word.startMs, endMs: word.endMs }
      else { current.text += word.text; current.endMs = Math.max(current.endMs, word.endMs) }
    }
    flush()
  }
  return out
}

export const liveSpeakerId = (channel: "mic" | "system", number: number) => `${channel}:n:${number}`
/** The microphone is the person recording (live/speakers.ts): its first voice is "Me", and anyone the
 * diariser confidently hears beside them is a room speaker. System audio numbers the other side. */
export const liveSpeakerLabel = (channel: "mic" | "system", number: number) => channel === "mic" ? number === 1 ? "Me" : `Room speaker ${number - 1}` : `Speaker ${number}`
/** `<channel>:n:<number>`: a voice numbered once for the whole note, unlike the `<channel>:<chunk>:speaker_<n>` of a batch part in an older recording. */
export const isLiveSpeakerId = (speakerId: string) => /^(?:mic|system):n:\d+$/.test(speakerId)
/** The number in a live speaker ID of `channel`, or undefined for any other ID. */
export const liveSpeakerNumber = (channel: "mic" | "system", speakerId: string) => { const match = /^(mic|system):n:(\d+)$/.exec(speakerId); return match && match[1] === channel ? Number(match[2]) : undefined }
