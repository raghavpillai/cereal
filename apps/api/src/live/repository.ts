/**
 * Persistence for live transcription: stored final segments, note-scoped speakers and the coverage
 * ledger.
 *
 * The ledger is the single record of which note-timeline audio has been accounted for and by whom:
 *   source "live"   the streaming session stored final text for it (or heard nothing in it);
 *   source "batch"  an uploaded part claimed it before asking the batch provider.
 * Each path drops words that fall inside the other's ranges, under the note's row lock, so a piece of
 * audio is transcribed once whichever path gets to it first.
 *
 * Only the trusted session and worker call these; request-scoped repositories do not expose them.
 */
import { Effect } from "effect"
import type { NoteDTO, TranscriptSegmentDTO } from "@meeting-notes/contracts"
import type { RepositoryError, StoredAudioChunk } from "../repository"
import { liveSpeakerId, liveSpeakerLabel, liveSpeakerNumber, mergeIntervals, segmentsFromTurns, subtractIntervals, uncoveredByLive, type Interval, type SpeakerTurn } from "./coverage"

export type AudioChannelName = "mic" | "system"
export type ChunkResolution = "batch" | "live" | "mixed"
export interface LiveLedgerRow { readonly channel: AudioChannelName; readonly fromMs: number; readonly toMs: number; readonly source: "live" | "batch"; readonly chunkId: string | null }
export interface FinishChunkOptions {
  /** How the part was accounted for. Defaults to "batch". */
  readonly resolution?: ChunkResolution
  /** The batch result could not be limited to the audio the live path missed (a provider without word
   * times): remove the live text inside the part and let the batch result stand for all of it. */
  readonly replaceLive?: boolean
}
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
}
export interface LiveAppendResult {
  readonly transcriptRevision: number
  readonly segments: ReadonlyArray<TranscriptSegmentDTO>
  /** `input.speakers` plus any number assigned by this append. */
  readonly speakers: Readonly<Record<string, number>>
}
export type ChunkPlan =
  | { readonly kind: "covered" }
  /** `keep` lists the note-timeline ranges the batch result should contribute; null means the whole part. */
  | { readonly kind: "transcribe"; readonly keep: ReadonlyArray<Interval> | null }
export interface LiveRepository {
  /** Every ledger row of a note. A note that has never had a live session has none. */
  readonly ledger: (noteId: string) => Effect.Effect<ReadonlyArray<LiveLedgerRow>, RepositoryError>
  /** Marks the note as live-capable from `atMs` on a channel, before any text exists. Idempotent. */
  readonly begin: (noteId: string, sessionId: string, channel: AudioChannelName, atMs: number) => Effect.Effect<boolean, RepositoryError>
  /** Stores one final as transcript segments and extends coverage, in one transaction. Null when the note is gone. */
  readonly append: (input: LiveAppendInput) => Effect.Effect<LiveAppendResult | null, RepositoryError>
  /** The speaker numbers that label stored live segments of a channel, ascending: the voices a new
   * provider session has to be told apart from (voices.ts). */
  readonly voices: (noteId: string, channel: AudioChannelName) => Effect.Effect<ReadonlyArray<number>, RepositoryError>
  /** Earlier segments of speaker `from` now belong to `to`. */
  readonly mergeSpeakers: (noteId: string, channel: AudioChannelName, from: number, to: number) => Effect.Effect<boolean, RepositoryError>
  /** Decides, under the note lock, what a claimed part still needs. A fully covered part is marked ready
   * here with no provider call. Null when the lease is no longer held. */
  readonly planAudioChunk: (id: string, lease: string) => Effect.Effect<ChunkPlan | null, RepositoryError>
  /** Keeps a claimed part for a later look without transcribing it. */
  readonly deferAudioChunk: (id: string, lease: string, until: string) => Effect.Effect<boolean, RepositoryError>
}

export const durationLabel = (endMs: number) => `${Math.floor(endMs / 60000)}:${String(Math.floor(endMs / 1000) % 60).padStart(2, "0")}`

/** What a part needs given the ledger rows of its channel. Shared by both repositories. */
export function planChunk(part: Interval, rows: ReadonlyArray<LiveLedgerRow>): ChunkPlan & { claims: Interval[] } {
  const uncovered = uncoveredByLive(part, rows.filter(row => row.source === "live"))
  if (!uncovered.length) return { kind: "covered", claims: [] }
  const whole = uncovered.length === 1 && uncovered[0]!.fromMs === part.fromMs && uncovered[0]!.toMs === part.toMs
  return { kind: "transcribe", keep: whole ? null : uncovered, claims: uncovered }
}
/** Segments and coverage a final contributes once audio already claimed by batch is left out. */
export function planAppend(input: LiveAppendInput, rows: ReadonlyArray<LiveLedgerRow>, nextNumber: () => number) {
  const exclude = rows.filter(row => row.source === "batch"), speakers: Record<string, number> = { ...input.speakers }
  const segments = segmentsFromTurns({ channel: input.channel, turns: input.turns, exclude, speaker: key => {
    const number = speakers[key] ??= nextNumber()
    return { speakerId: liveSpeakerId(input.channel, number), speakerLabel: liveSpeakerLabel(number) }
  } })
  const coverage = subtractIntervals(input.coverage, exclude)
  const live = coverage.length ? mergeIntervals([...rows.filter(row => row.source === "live"), ...coverage]) : null
  return { segments, speakers, coverage, live }
}
const midpoint = (segment: TranscriptSegmentDTO) => (segment.startMs + (segment.endMs ?? segment.startMs)) / 2

/** In-memory twin of the PostgreSQL implementation, sharing the planners above. */
export function makeMemoryLiveRepository(state: { notes: Map<string, NoteDTO>; chunks: Map<string, StoredAudioChunk>; clock: () => Date }) {
  const ledgers = new Map<string, LiveLedgerRow[]>(), numbers = new Map<string, number>(), liveSegments = new Set<string>()
  const rowsOf = (noteId: string, channel?: AudioChannelName) => (ledgers.get(noteId) ?? []).filter(row => !channel || row.channel === channel)
  const setChannelRows = (noteId: string, channel: AudioChannelName, source: "live" | "batch", rows: LiveLedgerRow[], chunkId?: string) =>
    ledgers.set(noteId, [...(ledgers.get(noteId) ?? []).filter(row => !(row.channel === channel && row.source === source && (chunkId === undefined || row.chunkId === chunkId))), ...rows])
  const endOf = (noteId: string, segments: ReadonlyArray<TranscriptSegmentDTO>, alsoReady?: string) => Math.max(0, ...segments.map(s => s.endMs ?? s.startMs),
    ...[...state.chunks.values()].filter(c => c.noteId === noteId && (c.state === "ready" || c.id === alsoReady)).map(c => c.startMs + c.durationMs))
  const live: LiveRepository = {
    ledger: noteId => Effect.sync(() => structuredClone(rowsOf(noteId))),
    begin: (noteId, _sessionId, channel, atMs) => Effect.sync(() => {
      if (!state.notes.has(noteId)) return false
      if (!rowsOf(noteId, channel).length) ledgers.set(noteId, [...(ledgers.get(noteId) ?? []), { channel, fromMs: atMs, toMs: atMs, source: "live", chunkId: null }])
      return true
    }),
    append: input => Effect.sync(() => {
      const note = state.notes.get(input.noteId)
      if (!note) return null
      const key = `${input.noteId}:${input.channel}`
      const plan = planAppend(input, rowsOf(input.noteId, input.channel), () => { const next = (numbers.get(key) ?? 0) + 1; numbers.set(key, next); return next })
      if (plan.live) setChannelRows(input.noteId, input.channel, "live", plan.live.map(interval => ({ ...interval, channel: input.channel, source: "live" as const, chunkId: null })))
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
    mergeSpeakers: (noteId, channel, from, to) => Effect.sync(() => {
      const note = state.notes.get(noteId)
      if (!note || from === to) return false
      const old = liveSpeakerId(channel, from)
      if (!note.transcriptSegments.some(segment => segment.speakerId === old)) return true
      state.notes.set(noteId, { ...note, transcriptRevision: note.transcriptRevision + 1, updatedAt: state.clock().toISOString(),
        transcriptSegments: note.transcriptSegments.map(segment => segment.speakerId === old ? { ...segment, speakerId: liveSpeakerId(channel, to), speakerLabel: liveSpeakerLabel(to) } : segment) })
      return true
    }),
    planAudioChunk: (id, lease) => Effect.sync(() => {
      const chunk = state.chunks.get(id), note = chunk && state.notes.get(chunk.noteId)
      if (!chunk || !note || chunk.state !== "processing" || chunk.lease !== lease) return null
      if (!rowsOf(chunk.noteId).length) return { kind: "transcribe", keep: null }
      const part = { fromMs: chunk.startMs, toMs: chunk.startMs + chunk.durationMs }
      const { claims, ...plan } = planChunk(part, rowsOf(chunk.noteId, chunk.channel))
      if (plan.kind === "covered") {
        state.chunks.set(id, { ...chunk, state: "ready", error: null, lease: null, leaseUntil: null, resolution: "live" })
        state.notes.set(note.id, { ...note, duration: durationLabel(endOf(note.id, note.transcriptSegments)), updatedAt: state.clock().toISOString() })
      } else setChannelRows(chunk.noteId, chunk.channel, "batch", claims.map(interval => ({ ...interval, channel: chunk.channel, source: "batch" as const, chunkId: id })), id)
      return plan
    }),
    deferAudioChunk: (id, lease, until) => Effect.sync(() => {
      const chunk = state.chunks.get(id)
      if (!chunk || chunk.state !== "processing" || chunk.lease !== lease) return false
      state.chunks.set(id, { ...chunk, leaseUntil: until }); return true
    }),
  }
  return {
    live,
    /** The live side of finishing a part: called by the memory repository's `finishAudioChunk`. */
    replaceLiveWithin(chunk: StoredAudioChunk, segments: ReadonlyArray<TranscriptSegmentDTO>) {
      const part = { fromMs: chunk.startMs, toMs: chunk.startMs + chunk.durationMs }
      setChannelRows(chunk.noteId, chunk.channel, "live", subtractIntervals(rowsOf(chunk.noteId, chunk.channel).filter(row => row.source === "live"), [part]).map(interval => ({ ...interval, channel: chunk.channel, source: "live" as const, chunkId: null })))
      setChannelRows(chunk.noteId, chunk.channel, "batch", [{ ...part, channel: chunk.channel, source: "batch", chunkId: chunk.id }], chunk.id)
      const kept = segments.filter(segment => !(liveSegments.has(segment.id) && segment.channel === chunk.channel && midpoint(segment) >= part.fromMs && midpoint(segment) < part.toMs))
      return { kept, removed: segments.length - kept.length }
    },
    forget(noteId: string) { ledgers.delete(noteId); for (const channel of ["mic", "system"]) numbers.delete(`${noteId}:${channel}`) },
    isLive: (segmentId: string) => liveSegments.has(segmentId),
  }
}
