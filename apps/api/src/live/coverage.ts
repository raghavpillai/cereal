/**
 * Interval arithmetic for the coverage ledger and the
 * word filters that keep each piece of audio transcribed exactly once. Pure: both repositories and the
 * worker use these, so memory and PostgreSQL behave the same.
 *
 * All times are note-timeline milliseconds. Intervals are half-open: [fromMs, toMs).
 */
import type { TranscriptSegmentDTO } from "@meeting-notes/contracts"

export interface Interval { readonly fromMs: number; readonly toMs: number }
/** Slack when comparing a durable part with coverage: provider word times and part cuts are not sample-exact. */
export const COVERAGE_TOLERANCE_MS = 250

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
/** The parts of `base` that no hole touches. */
export function subtractIntervals(base: ReadonlyArray<Interval>, holes: ReadonlyArray<Interval>): Interval[] {
  let remaining = mergeIntervals(base)
  for (const hole of mergeIntervals(holes)) {
    remaining = remaining.flatMap(piece => {
      if (hole.toMs <= piece.fromMs || hole.fromMs >= piece.toMs) return [piece]
      return [{ fromMs: piece.fromMs, toMs: hole.fromMs }, { fromMs: hole.toMs, toMs: piece.toMs }].filter(p => p.toMs > p.fromMs)
    })
  }
  return remaining
}
export const withinIntervals = (intervals: ReadonlyArray<Interval>, ms: number) => intervals.some(i => ms >= i.fromMs && ms < i.toMs)
export const overlapMs = (a: Interval, b: Interval) => Math.max(0, Math.min(a.toMs, b.toMs) - Math.max(a.fromMs, b.fromMs))

/** What of a durable part the live path has not accounted for, ignoring slivers under the tolerance.
 * `[]` means fully covered; a single interval equal to the part means not covered at all. */
export function uncoveredByLive(part: Interval, live: ReadonlyArray<Interval>, tolerance = COVERAGE_TOLERANCE_MS): Interval[] {
  const uncovered = subtractIntervals([part], mergeIntervals(live, tolerance)).filter(piece => piece.toMs - piece.fromMs > tolerance)
  if (uncovered.length === 1 && uncovered[0]!.fromMs - part.fromMs <= tolerance && part.toMs - uncovered[0]!.toMs <= tolerance) return [{ fromMs: part.fromMs, toMs: part.toMs }]
  return uncovered
}

export interface TimedWord { readonly text: string; readonly startMs: number; readonly endMs: number }
export interface SpeakerTurn { readonly speakerKey: string; readonly words: ReadonlyArray<TimedWord> }
export const wordMidpoint = (word: TimedWord) => (word.startMs + word.endMs) / 2

/** Segments from speaker turns, leaving out every word whose midpoint falls inside `exclude` (audio the
 * other path has claimed). A turn is cut where words were left out, so text from the two paths never
 * interleaves inside one segment. Word text carries its own trailing spacing. */
export function segmentsFromTurns(input: {
  channel: "mic" | "system"
  turns: ReadonlyArray<SpeakerTurn>
  exclude: ReadonlyArray<Interval>
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
      if (withinIntervals(input.exclude, wordMidpoint(word))) { flush(); continue }
      if (current && current.text.length + word.text.length > limit) flush()
      if (!current) current = { text: word.text, startMs: word.startMs, endMs: word.endMs }
      else { current.text += word.text; current.endMs = Math.max(current.endMs, word.endMs) }
    }
    flush()
  }
  return out
}

export const liveSpeakerId = (channel: "mic" | "system", number: number) => `${channel}:n:${number}`
export const liveSpeakerLabel = (number: number) => `Speaker ${number}`
/** `<channel>:n:<number>`: a voice numbered once for the whole note, unlike a batch part's `<channel>:<chunk>:speaker_<n>`. */
export const isLiveSpeakerId = (speakerId: string) => /^(?:mic|system):n:\d+$/.test(speakerId)
/** The number in a live speaker ID of `channel`, or undefined for any other ID. */
export const liveSpeakerNumber = (channel: "mic" | "system", speakerId: string) => { const match = /^(mic|system):n:(\d+)$/.exec(speakerId); return match && match[1] === channel ? Number(match[2]) : undefined }
