/**
 * Persistence for live transcription: stored final segments, note-scoped speakers and the coverage
 * ledger.
 *
 * The ledger records which note-timeline audio the streaming session has accounted for (stored final
 * text for it, or heard nothing in it), so a reconnect resumes where it ended.
 *
 * Only the trusted live session calls these; request-scoped repositories do not expose them.
 */
import { Effect } from "effect"
import type { NoteDTO, TranscriptSegmentDTO } from "@meeting-notes/contracts"
import type { RepositoryError } from "../repository"
import { liveSpeakerId, liveSpeakerLabel, liveSpeakerNumber, mergeIntervals, segmentsFromTurns, type Interval, type SpeakerTurn } from "./coverage"

export type AudioChannelName = "mic" | "system"
export interface LiveLedgerRow { readonly channel: AudioChannelName; readonly fromMs: number; readonly toMs: number }
export interface LiveAppendInput {
  readonly noteId: string
  readonly sessionId: string
  readonly channel: AudioChannelName
  /** Speaker turns of one final, with note-timeline word times. Empty for a silence checkpoint. */
  readonly turns: ReadonlyArray<SpeakerTurn>
  /** Speaker numbers already assigned in this upstream session, by provider speaker key. */
  readonly speakers: Readonly<Record<string, number>>
  /** Note-timeline audio this final accounts for. */
  readonly coverage: ReadonlyArray<Interval>
  /** Labels by speaker number where they differ from `liveSpeakerLabel` (the microphone's, speakers.ts). */
  readonly labels?: Readonly<Record<number, string>>
}
/** One stored segment that now carries another speaker or label. */
export interface SegmentRelabel { readonly segmentId: string; readonly speakerId: string; readonly speakerLabel: string }
export interface LiveAppendResult {
  readonly transcriptRevision: number
  readonly segments: ReadonlyArray<TranscriptSegmentDTO>
  /** `input.speakers` plus any number assigned by this append. */
  readonly speakers: Readonly<Record<string, number>>
}
export interface LiveRepository {
  /** Every ledger row of a note. A note that has never had a live session has none. */
  readonly ledger: (noteId: string) => Effect.Effect<ReadonlyArray<LiveLedgerRow>, RepositoryError>
  /** Marks the note as live-capable from `atMs` on a channel, before any text exists. Idempotent. */
  readonly begin: (noteId: string, sessionId: string, channel: AudioChannelName, atMs: number) => Effect.Effect<boolean, RepositoryError>
  /** Stores one final as transcript segments and extends coverage, in one transaction. Null when the note is gone. */
  readonly append: (input: LiveAppendInput) => Effect.Effect<LiveAppendResult | null, RepositoryError>
  /** The speaker numbers that label stored live segments of a channel, ascending: the voices a new
   * provider session carries over (speakers.ts). */
  readonly voices: (noteId: string, channel: AudioChannelName) => Effect.Effect<ReadonlyArray<number>, RepositoryError>
  /** The label stored live segments of each speaker number of a channel carry. */
  readonly voiceLabels: (noteId: string, channel: AudioChannelName) => Effect.Effect<Readonly<Record<number, string>>, RepositoryError>
  /** Earlier segments of speaker `from` now belong to `to`, labelled `label`. */
  readonly mergeSpeakers: (noteId: string, channel: AudioChannelName, from: number, to: number, label?: string) => Effect.Effect<boolean, RepositoryError>
  /** Records a stretch that was captured but will never have a transcript (`NoteDTO.transcriptGaps`). */
  readonly recordGap: (noteId: string, channel: AudioChannelName, fromMs: number, toMs: number, reason: string) => Effect.Effect<boolean, RepositoryError>
  /** Where each speaker number of a channel speaks between two note times, from the stored live segments. */
  readonly voicesBetween: (noteId: string, channel: AudioChannelName, fromMs: number, toMs: number) => Effect.Effect<ReadonlyArray<{ fromMs: number; toMs: number; number: number }>, RepositoryError>
  /** The ids of the stored live segments of speaker `number` on `channel`. */
  readonly segmentsOf: (noteId: string, channel: AudioChannelName, number: number) => Effect.Effect<ReadonlyArray<string>, RepositoryError>
  /** Gives the segments their new speaker and label (one revision for all of them). The ones applied. */
  readonly relabel: (noteId: string, updates: ReadonlyArray<SegmentRelabel>) => Effect.Effect<ReadonlyArray<SegmentRelabel>, RepositoryError>
}

/** The gaps a note shows: per channel, merged, two seconds or longer, in time order. */
export function visibleGaps(rows: ReadonlyArray<{ channel: AudioChannelName; fromMs: number; toMs: number }>) {
  return (["mic", "system"] as const).flatMap(channel => mergeIntervals(rows.filter(row => row.channel === channel)).filter(gap => gap.toMs - gap.fromMs >= 2000).map(gap => ({ channel, fromMs: gap.fromMs, toMs: gap.toMs })))
    .sort((a, b) => a.fromMs - b.fromMs)
}
export const durationLabel = (endMs: number) => `${Math.floor(endMs / 60000)}:${String(Math.floor(endMs / 1000) % 60).padStart(2, "0")}`

/** Segments and coverage a final contributes. */
export function planAppend(input: LiveAppendInput, rows: ReadonlyArray<LiveLedgerRow>, nextNumber: () => number) {
  const speakers: Record<string, number> = { ...input.speakers }
  const segments = segmentsFromTurns({ channel: input.channel, turns: input.turns, speaker: key => {
    const number = speakers[key] ??= nextNumber()
    return { speakerId: liveSpeakerId(input.channel, number), speakerLabel: input.labels?.[number] ?? liveSpeakerLabel(input.channel, number) }
  } })
  const coverage = mergeIntervals(input.coverage)
  const live = coverage.length ? mergeIntervals([...rows, ...coverage]) : null
  return { segments, speakers, coverage, live }
}

/** In-memory twin of the PostgreSQL implementation, sharing the planner above. */
export function makeMemoryLiveRepository(state: { notes: Map<string, NoteDTO>; clock: () => Date }) {
  const ledgers = new Map<string, LiveLedgerRow[]>(), numbers = new Map<string, number>(), liveSegments = new Set<string>(), gaps = new Map<string, LiveLedgerRow[]>()
  const rowsOf = (noteId: string, channel?: AudioChannelName) => (ledgers.get(noteId) ?? []).filter(row => !channel || row.channel === channel)
  const setChannelRows = (noteId: string, channel: AudioChannelName, rows: LiveLedgerRow[]) =>
    ledgers.set(noteId, [...(ledgers.get(noteId) ?? []).filter(row => row.channel !== channel), ...rows])
  /** The note's displayed length: the end of its text or of the audio the live session accounted for. */
  const endOf = (noteId: string, segments: ReadonlyArray<TranscriptSegmentDTO>) => Math.max(0, ...segments.map(s => s.endMs ?? s.startMs), ...rowsOf(noteId).map(row => row.toMs))
  const live: LiveRepository = {
    ledger: noteId => Effect.sync(() => structuredClone(rowsOf(noteId))),
    begin: (noteId, _sessionId, channel, atMs) => Effect.sync(() => {
      if (!state.notes.has(noteId)) return false
      if (!rowsOf(noteId, channel).length) ledgers.set(noteId, [...(ledgers.get(noteId) ?? []), { channel, fromMs: atMs, toMs: atMs }])
      return true
    }),
    append: input => Effect.sync(() => {
      const note = state.notes.get(input.noteId)
      if (!note) return null
      const key = `${input.noteId}:${input.channel}`
      // Never below a number this provider session already holds (a voice carried over, speakers.ts).
      const plan = planAppend(input, rowsOf(input.noteId, input.channel), () => { const next = Math.max(numbers.get(key) ?? 0, ...Object.values(input.speakers)) + 1; numbers.set(key, next); return next })
      if (plan.live) setChannelRows(input.noteId, input.channel, plan.live.map(interval => ({ ...interval, channel: input.channel })))
      for (const segment of plan.segments) liveSegments.add(segment.id)
      const merged = [...note.transcriptSegments, ...plan.segments].sort((a, b) => a.startMs - b.startMs)
      const next: NoteDTO = { ...note, transcriptSegments: merged, transcriptRevision: note.transcriptRevision + (plan.segments.length ? 1 : 0),
        ...(plan.segments.length ? { duration: durationLabel(endOf(note.id, merged)), updatedAt: state.clock().toISOString() } : {}) }
      state.notes.set(note.id, next)
      return { transcriptRevision: next.transcriptRevision, segments: structuredClone(plan.segments), speakers: plan.speakers }
    }),
    voices: (noteId, channel) => Effect.sync(() => {
      const numbers = (state.notes.get(noteId)?.transcriptSegments ?? []).filter(segment => segment.channel === channel && liveSegments.has(segment.id)).map(segment => liveSpeakerNumber(channel, segment.speakerId))
      return [...new Set(numbers.filter((number): number is number => number !== undefined))].sort((a, b) => a - b)
    }),
    voiceLabels: (noteId, channel) => Effect.sync(() => {
      const labels: Record<number, string> = {}
      for (const segment of state.notes.get(noteId)?.transcriptSegments ?? []) {
        const number = segment.channel === channel && liveSegments.has(segment.id) ? liveSpeakerNumber(channel, segment.speakerId) : undefined
        if (number !== undefined && segment.speakerLabel) labels[number] = segment.speakerLabel
      }
      return labels
    }),
    mergeSpeakers: (noteId, channel, from, to, label) => Effect.sync(() => {
      const note = state.notes.get(noteId)
      if (!note || from === to) return false
      const old = liveSpeakerId(channel, from)
      if (!note.transcriptSegments.some(segment => segment.speakerId === old)) return true
      state.notes.set(noteId, { ...note, transcriptRevision: note.transcriptRevision + 1, updatedAt: state.clock().toISOString(),
        transcriptSegments: note.transcriptSegments.map(segment => segment.speakerId === old ? { ...segment, speakerId: liveSpeakerId(channel, to), speakerLabel: label ?? liveSpeakerLabel(channel, to) } : segment) })
      return true
    }),
    recordGap: (noteId, channel, fromMs, toMs) => Effect.sync(() => {
      const note = state.notes.get(noteId)
      if (!note || toMs <= fromMs) return false
      const rows = [...(gaps.get(noteId) ?? []), { channel, fromMs, toMs }]
      gaps.set(noteId, rows)
      const visible = visibleGaps(rows)
      state.notes.set(noteId, { ...note, ...(visible.length ? { transcriptGaps: visible } : {}) })
      return true
    }),
    voicesBetween: (noteId, channel, fromMs, toMs) => Effect.sync(() => (state.notes.get(noteId)?.transcriptSegments ?? [])
      .filter(segment => segment.channel === channel && liveSegments.has(segment.id) && (segment.endMs ?? segment.startMs) > fromMs && segment.startMs < toMs)
      .flatMap(segment => { const number = liveSpeakerNumber(channel, segment.speakerId); return number === undefined ? [] : [{ fromMs: segment.startMs, toMs: segment.endMs ?? segment.startMs, number }] })),
    segmentsOf: (noteId, channel, number) => Effect.sync(() => (state.notes.get(noteId)?.transcriptSegments ?? []).filter(segment => liveSegments.has(segment.id) && segment.speakerId === liveSpeakerId(channel, number)).map(segment => segment.id)),
    relabel: (noteId, updates) => Effect.sync(() => {
      const note = state.notes.get(noteId)
      if (!note) return []
      const byId = new Map(updates.map(update => [update.segmentId, update]))
      const applied = note.transcriptSegments.filter(segment => { const update = byId.get(segment.id); return update && (update.speakerId !== segment.speakerId || update.speakerLabel !== segment.speakerLabel) }).map(segment => byId.get(segment.id)!)
      if (!applied.length) return []
      state.notes.set(noteId, { ...note, transcriptRevision: note.transcriptRevision + 1, updatedAt: state.clock().toISOString(),
        transcriptSegments: note.transcriptSegments.map(segment => { const update = byId.get(segment.id); return update ? { ...segment, speakerId: update.speakerId, speakerLabel: update.speakerLabel } : segment }) })
      return applied
    }),
  }
  return {
    live,
    forget(noteId: string) { ledgers.delete(noteId); gaps.delete(noteId); for (const channel of ["mic", "system"]) numbers.delete(`${noteId}:${channel}`) },
    isLive: (segmentId: string) => liveSegments.has(segmentId),
  }
}
