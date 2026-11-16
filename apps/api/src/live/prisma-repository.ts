/** PostgreSQL implementation of `LiveRepository`. Every write holds the note's row lock, so writes of the
 * live session and edits of the note never interleave. */
import { Effect } from "effect"
import type { Database } from "@meeting-notes/database"
import type { TranscriptSegmentDTO } from "@meeting-notes/contracts"
import { RepositoryError } from "../repository"
import { liveSpeakerLabel, liveSpeakerNumber } from "./coverage"
import { durationLabel, planAppend, type AudioChannelName, type LiveLedgerRow, type LiveRepository } from "./repository"

type Transaction = Parameters<Parameters<Database["$transaction"]>[0]>[0]
const attempt = <A>(operation: string, task: () => Promise<A>) => Effect.tryPromise({ try: task, catch: () => new RepositoryError(operation) })
const row = (r: { channel: AudioChannelName; fromMs: number; toMs: number }): LiveLedgerRow => ({ channel: r.channel, fromMs: r.fromMs, toMs: r.toMs })
const lock = (tx: Transaction, noteId: string) => tx.$queryRaw`SELECT "id" FROM "Note" WHERE "id" = ${noteId}::uuid FOR UPDATE`

/** The note's displayed length: the end of its stored text or of the audio the live session accounted for. */
export async function refreshDuration(tx: Transaction, noteId: string) {
  const [segments, coverage] = await Promise.all([
    tx.transcriptSegment.aggregate({ where: { noteId }, _max: { endMs: true, startMs: true } }),
    tx.liveCoverage.aggregate({ where: { noteId }, _max: { toMs: true } }),
  ])
  return durationLabel(Math.max(0, segments._max.endMs ?? 0, segments._max.startMs ?? 0, coverage._max.toMs ?? 0))
}

export function makePrismaLiveRepository(db: Database): LiveRepository {
  return {
    ledger: noteId => attempt("liveLedger", async () => (await db.liveCoverage.findMany({ where: { noteId }, orderBy: [{ channel: "asc" }, { fromMs: "asc" }] })).map(row)),
    begin: (noteId, sessionId, channel, atMs) => attempt("liveBegin", () => db.$transaction(async tx => {
      await lock(tx, noteId)
      if (!await tx.note.count({ where: { id: noteId } })) return false
      if (!await tx.liveCoverage.count({ where: { noteId, channel } })) await tx.liveCoverage.create({ data: { noteId, channel, fromMs: atMs, toMs: atMs, sessionId } })
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
        await tx.liveCoverage.deleteMany({ where: { noteId: input.noteId, channel: input.channel } })
        await tx.liveCoverage.createMany({ data: plan.live.map(interval => ({ noteId: input.noteId, channel: input.channel, fromMs: interval.fromMs, toMs: interval.toMs, sessionId: input.sessionId })) })
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
    voiceLabels: (noteId, channel) => attempt("liveVoiceLabels", async () => {
      const rows = await db.transcriptSegment.findMany({ where: { noteId, channel, origin: "live" }, distinct: ["speakerId"], select: { speakerId: true, speakerLabel: true }, orderBy: { startMs: "desc" } })
      const labels: Record<number, string> = {}
      for (const row of rows) { const number = liveSpeakerNumber(channel, row.speakerId); if (number !== undefined && row.speakerLabel) labels[number] = row.speakerLabel }
      return labels
    }),
    recordGap: (noteId, channel, fromMs, toMs, reason) => attempt("liveRecordGap", async () => {
      if (toMs <= fromMs) return false
      return (await db.liveGap.createMany({ data: [{ noteId, channel, fromMs, toMs, reason }] })).count > 0
    }),
    voicesBetween: (noteId, channel, fromMs, toMs) => attempt("liveVoicesBetween", async () => (await db.transcriptSegment.findMany({ where: { noteId, channel, origin: "live", startMs: { lt: toMs }, OR: [{ endMs: { gt: fromMs } }, { endMs: null, startMs: { gte: fromMs } }] }, select: { startMs: true, endMs: true, speakerId: true } }))
      .flatMap(row => { const number = liveSpeakerNumber(channel, row.speakerId); return number === undefined ? [] : [{ fromMs: row.startMs, toMs: row.endMs ?? row.startMs, number }] })),
    segmentsOf: (noteId, channel, number) => attempt("liveSegmentsOf", async () => (await db.transcriptSegment.findMany({ where: { noteId, channel, origin: "live", speakerId: `${channel}:n:${number}` }, select: { id: true } })).map(row => row.id)),
    relabel: (noteId, updates) => attempt("liveRelabel", () => db.$transaction(async tx => {
      await lock(tx, noteId)
      if (!updates.length || !await tx.note.count({ where: { id: noteId } })) return []
      const current = await tx.transcriptSegment.findMany({ where: { noteId, id: { in: updates.map(update => update.segmentId) } }, select: { id: true, speakerId: true, speakerLabel: true } })
      const known = new Map(current.map(row => [row.id, row]))
      const applied = updates.filter(update => { const row = known.get(update.segmentId); return row && (row.speakerId !== update.speakerId || row.speakerLabel !== update.speakerLabel) })
      // One statement per speaker and label, not per segment.
      const groups = new Map<string, { speakerId: string; speakerLabel: string; ids: string[] }>()
      for (const update of applied) { const key = `${update.speakerId}\0${update.speakerLabel}`; const group = groups.get(key) ?? { speakerId: update.speakerId, speakerLabel: update.speakerLabel, ids: [] }; group.ids.push(update.segmentId); groups.set(key, group) }
      for (const group of groups.values()) await tx.transcriptSegment.updateMany({ where: { noteId, id: { in: group.ids } }, data: { speakerId: group.speakerId, speakerLabel: group.speakerLabel } })
      if (applied.length) await tx.note.update({ where: { id: noteId }, data: { transcriptRevision: { increment: 1 } } })
      return applied
    })),
    mergeSpeakers: (noteId, channel, from, to, label) => attempt("liveMergeSpeakers", () => db.$transaction(async tx => {
      await lock(tx, noteId)
      if (from === to || !await tx.note.count({ where: { id: noteId } })) return false
      const target = await tx.noteSpeaker.findUnique({ where: { noteId_channel_number: { noteId, channel, number: to } } })
      if (target) await tx.noteSpeaker.updateMany({ where: { noteId, channel, number: from }, data: { mergedIntoId: target.id } })
      const changed = await tx.transcriptSegment.updateMany({ where: { noteId, channel, speakerId: `${channel}:n:${from}` }, data: { speakerId: `${channel}:n:${to}`, speakerLabel: label ?? liveSpeakerLabel(channel, to) } })
      if (changed.count) await tx.note.update({ where: { id: noteId }, data: { transcriptRevision: { increment: 1 } } })
      return true
    }))
  }
}
