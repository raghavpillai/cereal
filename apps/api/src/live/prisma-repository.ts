/** PostgreSQL implementation of `LiveRepository`. Every write holds the note's row lock, the same lock
 * `finishAudioChunk` takes, so the live session and the batch worker never interleave on one note. */
import { Effect } from "effect"
import type { Database } from "@meeting-notes/database"
import type { TranscriptSegmentDTO } from "@meeting-notes/contracts"
import { RepositoryError } from "../repository"
import { liveSpeakerNumber, subtractIntervals } from "./coverage"
import { durationLabel, planAppend, planChunk, type AudioChannelName, type LiveLedgerRow, type LiveRepository } from "./repository"

type Transaction = Parameters<Parameters<Database["$transaction"]>[0]>[0]
const attempt = <A>(operation: string, task: () => Promise<A>) => Effect.tryPromise({ try: task, catch: () => new RepositoryError(operation) })
const row = (r: { channel: AudioChannelName; fromMs: number; toMs: number; source: string; chunkId: string | null }): LiveLedgerRow => ({ channel: r.channel, fromMs: r.fromMs, toMs: r.toMs, source: r.source === "batch" ? "batch" : "live", chunkId: r.chunkId })
const lock = (tx: Transaction, noteId: string) => tx.$queryRaw`SELECT "id" FROM "Note" WHERE "id" = ${noteId}::uuid FOR UPDATE`

/** Recompute the note's displayed duration from stored text and ready parts. */
export async function refreshDuration(tx: Transaction, noteId: string) {
  const [segments, chunks] = await Promise.all([
    tx.transcriptSegment.aggregate({ where: { noteId }, _max: { endMs: true, startMs: true } }),
    tx.audioChunk.findMany({ where: { noteId, state: "ready" }, select: { startMs: true, durationMs: true } }),
  ])
  const end = Math.max(0, segments._max.endMs ?? 0, segments._max.startMs ?? 0, ...chunks.map(c => c.startMs + c.durationMs))
  return durationLabel(end)
}
/** The live side of finishing a part whose batch result could not be limited to uncovered audio. Returns how many live segments were removed. */
export async function replaceLiveWithin(tx: Transaction, chunk: { id: string; noteId: string; channel: AudioChannelName; startMs: number; durationMs: number }) {
  const part = { fromMs: chunk.startMs, toMs: chunk.startMs + chunk.durationMs }
  const live = await tx.liveCoverage.findMany({ where: { noteId: chunk.noteId, channel: chunk.channel, source: "live" } })
  await tx.liveCoverage.deleteMany({ where: { noteId: chunk.noteId, channel: chunk.channel, OR: [{ source: "live" }, { source: "batch", chunkId: chunk.id }] } })
  await tx.liveCoverage.createMany({ data: [
    ...subtractIntervals(live, [part]).map(interval => ({ noteId: chunk.noteId, channel: chunk.channel, fromMs: interval.fromMs, toMs: interval.toMs, source: "live" })),
    { noteId: chunk.noteId, channel: chunk.channel, fromMs: part.fromMs, toMs: part.toMs, source: "batch", chunkId: chunk.id },
  ] })
  const candidates = await tx.transcriptSegment.findMany({ where: { noteId: chunk.noteId, channel: chunk.channel, origin: "live", startMs: { lt: part.toMs } }, select: { id: true, startMs: true, endMs: true } })
  const doomed = candidates.filter(s => { const mid = (s.startMs + (s.endMs ?? s.startMs)) / 2; return mid >= part.fromMs && mid < part.toMs }).map(s => s.id)
  if (doomed.length) await tx.transcriptSegment.deleteMany({ where: { id: { in: doomed } } })
  return doomed.length
}

export function makePrismaLiveRepository(db: Database): LiveRepository {
  return {
    ledger: noteId => attempt("liveLedger", async () => (await db.liveCoverage.findMany({ where: { noteId }, orderBy: [{ channel: "asc" }, { fromMs: "asc" }] })).map(row)),
    begin: (noteId, sessionId, channel, atMs) => attempt("liveBegin", () => db.$transaction(async tx => {
      await lock(tx, noteId)
      if (!await tx.note.count({ where: { id: noteId } })) return false
      if (!await tx.liveCoverage.count({ where: { noteId, channel } })) await tx.liveCoverage.create({ data: { noteId, channel, fromMs: atMs, toMs: atMs, source: "live", sessionId } })
      return true
    })),
    append: input => attempt("liveAppend", () => db.$transaction(async tx => {
      await lock(tx, input.noteId)
      // The note row only: this runs for every utterance, so it never loads the note's segments.
      const note = await tx.note.findUnique({ where: { id: input.noteId }, select: { transcriptRevision: true } })
      if (!note) return null
      const rows = (await tx.liveCoverage.findMany({ where: { noteId: input.noteId, channel: input.channel } })).map(row)
      let highest: number | undefined
      const assigned: number[] = []
      const known = Object.values(input.speakers)
      const numbering = async () => { highest = Math.max(0, ...known, (await tx.noteSpeaker.aggregate({ where: { noteId: input.noteId, channel: input.channel }, _max: { number: true } }))._max.number ?? 0) }
      // Numbers are handed out synchronously while segments are built, so read the high-water mark first.
      if (input.turns.some(turn => input.speakers[turn.speakerKey] === undefined)) await numbering()
      const plan = planAppend(input, rows, () => { highest = (highest ?? 0) + 1; assigned.push(highest); return highest })
      if (assigned.length) await tx.noteSpeaker.createMany({ data: assigned.map(number => ({ noteId: input.noteId, channel: input.channel, number })), skipDuplicates: true })
      if (plan.live) {
        await tx.liveCoverage.deleteMany({ where: { noteId: input.noteId, channel: input.channel, source: "live" } })
        await tx.liveCoverage.createMany({ data: plan.live.map(interval => ({ noteId: input.noteId, channel: input.channel, fromMs: interval.fromMs, toMs: interval.toMs, source: "live", sessionId: input.sessionId })) })
      }
      if (!plan.segments.length) return { transcriptRevision: note.transcriptRevision, segments: [], speakers: plan.speakers }
      await tx.transcriptSegment.createMany({ data: plan.segments.map(segment => ({ ...segment, noteId: input.noteId, origin: "live" })) })
      const updated = await tx.note.update({ where: { id: input.noteId }, data: { transcriptRevision: { increment: 1 }, duration: await refreshDuration(tx, input.noteId) }, select: { transcriptRevision: true } })
      return { transcriptRevision: updated.transcriptRevision, segments: plan.segments as ReadonlyArray<TranscriptSegmentDTO>, speakers: plan.speakers }
    })),
    voices: (noteId, channel) => attempt("liveVoices", async () => {
      const rows = await db.transcriptSegment.findMany({ where: { noteId, channel, origin: "live" }, distinct: ["speakerId"], select: { speakerId: true } })
      return [...new Set(rows.map(item => liveSpeakerNumber(channel, item.speakerId)).filter((number): number is number => number !== undefined))].sort((a, b) => a - b)
    }),
    mergeSpeakers: (noteId, channel, from, to) => attempt("liveMergeSpeakers", () => db.$transaction(async tx => {
      await lock(tx, noteId)
      if (from === to || !await tx.note.count({ where: { id: noteId } })) return false
      const target = await tx.noteSpeaker.findUnique({ where: { noteId_channel_number: { noteId, channel, number: to } } })
      if (target) await tx.noteSpeaker.updateMany({ where: { noteId, channel, number: from }, data: { mergedIntoId: target.id } })
      const changed = await tx.transcriptSegment.updateMany({ where: { noteId, channel, speakerId: `${channel}:n:${from}` }, data: { speakerId: `${channel}:n:${to}`, speakerLabel: `Speaker ${to}` } })
      if (changed.count) await tx.note.update({ where: { id: noteId }, data: { transcriptRevision: { increment: 1 } } })
      return true
    })),
    planAudioChunk: (id, lease) => attempt("planAudioChunk", () => db.$transaction(async tx => {
      const initial = await tx.audioChunk.findUnique({ where: { id } })
      if (!initial) return null
      await lock(tx, initial.noteId)
      const chunk = await tx.audioChunk.findUnique({ where: { id } })
      if (!chunk || chunk.state !== "processing" || chunk.lease !== lease) return null
      if (!await tx.liveCoverage.count({ where: { noteId: chunk.noteId } })) return { kind: "transcribe" as const, keep: null }
      const rows = (await tx.liveCoverage.findMany({ where: { noteId: chunk.noteId, channel: chunk.channel } })).map(row)
      const { claims, ...plan } = planChunk({ fromMs: chunk.startMs, toMs: chunk.startMs + chunk.durationMs }, rows)
      if (plan.kind === "covered") {
        await tx.audioChunk.update({ where: { id }, data: { state: "ready", error: null, lease: null, leaseUntil: null, resolution: "live" } })
        await tx.note.update({ where: { id: chunk.noteId }, data: { duration: await refreshDuration(tx, chunk.noteId) } })
      } else {
        await tx.liveCoverage.deleteMany({ where: { noteId: chunk.noteId, source: "batch", chunkId: id } })
        await tx.liveCoverage.createMany({ data: claims.map(interval => ({ noteId: chunk.noteId, channel: chunk.channel, fromMs: interval.fromMs, toMs: interval.toMs, source: "batch", chunkId: id })) })
      }
      return plan
    })),
    deferAudioChunk: (id, lease, until) => attempt("deferAudioChunk", async () => (await db.audioChunk.updateMany({ where: { id, lease, state: "processing" }, data: { leaseUntil: new Date(until) } })).count > 0),
  }
}
