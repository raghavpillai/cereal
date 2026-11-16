import { afterEach, describe, expect, test } from "bun:test"
import { expectedSegmentId, importScenarios } from "./import-scenarios"
import { importBody, postgresEnabled, segment, stack, type Stack } from "./import-support"
import { makePrismaGoogleStore } from "../src/google-store"

// The same scenarios as import.test.ts, on PostgreSQL through Prisma, wired as server.ts wires it. Runs only
// in the throwaway database of scripts/test-postgres-isolated.ts (or CI's), named mn_test_<hex>.
importScenarios("postgres")

describe.skipIf(!postgresEnabled)("note import on PostgreSQL: one transaction", () => {
  const open: Stack[] = []
  afterEach(async () => { for (const s of open.splice(0)) await s.close() })
  const start = async () => { const s = await stack("postgres"); open.push(s); return s }
  const conflict = '{"error":"conflict","message":"A note with this id already exists"}'

  test("what the import stores: the digest, one clock reading, batch segments, the caller's workspace, a context row and no binding", async () => {
    const s = await start(), db = s.db!, body = importBody()
    expect((await s.call("alice", "/notes/import", "POST", body)).status).toBe(200)
    const row = await db.note.findUniqueOrThrow({ where: { id: body.id }, include: { transcriptSegments: true, meetingContext: true, calendarMeeting: true, googleMeeting: true } })
    expect(row.ownerId).toBe(s.ids.alice)
    expect(row.importDigest).toMatch(/^[a-f0-9]{64}$/)
    expect(row.importedAt!.getTime()).toBe(row.updatedAt.getTime())
    expect(row.createdAt.toISOString()).toBe(body.createdAt)
    expect(row.transcriptSegments.map(item => item.origin)).toEqual(["batch", "batch", "batch", "batch"])
    expect(new Set(row.transcriptSegments.map(item => item.id))).toEqual(new Set(body.transcriptSegments.map(item => expectedSegmentId(body.id, item.id))))
    expect(row.meetingContext!.userId).toBe(s.ids.alice)
    expect([row.calendarMeeting, row.googleMeeting]).toEqual([null, null])
    // A note made the ordinary way has neither column.
    const plain = await s.call("alice", "/notes", "POST", { title: "Made here" })
    const ordinary = await db.note.findUniqueOrThrow({ where: { id: plain.json.id } })
    expect([ordinary.importedAt, ordinary.importDigest]).toEqual([null, null])
  })

  test("X1 a detached context saved again stays detached; one that names no event and is not the stored one is refused", async () => {
    const s = await start(), db = s.db!, body = importBody(), store = makePrismaGoogleStore(db)
    expect((await s.call("alice", "/notes/import", "POST", body)).status).toBe(200)
    const saved = (await store.getContext(s.ids.alice, body.id))!.meeting!
    for (const provider of ["google", "ics"] as const) {
      const renamed = await store.putContext(s.ids.alice, body.id, { ...saved, provider, location: `Room for ${provider}` })
      expect(renamed!.meeting).toEqual({ ...saved, provider, location: `Room for ${provider}` })
      expect((await s.call("alice", `/notes/${body.id}/context`)).json.meeting.location).toBe(`Room for ${provider}`)
      expect(await db.calendarMeeting.count({ where: { noteId: body.id } }) + await db.googleMeeting.count({ where: { noteId: body.id } })).toBe(0)
    }
    // Another event id that is no event here: refused, as it always was, and nothing changes.
    expect(await store.putContext(s.ids.alice, body.id, { ...saved, eventId: crypto.randomUUID(), title: "Forged" })).toBeNull()
    expect(await store.putContext(s.ids.alice, body.id, { ...saved, eventId: body.meeting!.eventId })).toBeNull()
    expect(await store.putContext(s.ids.bob, body.id, saved)).toBeNull()
    expect((await s.call("alice", `/notes/${body.id}/context`)).json.meeting.title).toBe("Q4 planning")
    // Clearing the context still works.
    expect((await s.call("alice", `/notes/${body.id}/context`, "PUT", { eventId: null })).json).toEqual({ noteId: body.id, meeting: null })
  })

  test("M9 the same import sent several times at once makes exactly one note, and every answer is the note", async () => {
    const s = await start(), db = s.db!, body = importBody({ transcriptSegments: Array.from({ length: 300 }, (_, index) => segment(index)) })
    const answers = await Promise.all(Array.from({ length: 6 }, () => s.call("alice", "/notes/import", "POST", body)))
    expect(answers.map(answer => answer.status)).toEqual([200, 200, 200, 200, 200, 200])
    expect(new Set(answers.map(answer => JSON.stringify(answer.json.segmentIds))).size).toBe(1)
    expect(new Set(answers.map(answer => answer.json.note.id))).toEqual(new Set([body.id]))
    expect(new Set(answers.map(answer => answer.json.note.updatedAt)).size).toBe(1)
    expect(await db.note.count({ where: { id: body.id } })).toBe(1)
    expect(await db.transcriptSegment.count({ where: { noteId: body.id } })).toBe(300)
    expect(await db.noteMeetingContext.count({ where: { noteId: body.id } })).toBe(1)
    // Two people racing for one id: one of them has the note, the other the one 409.
    const contested = importBody(), mine = { ...contested }, theirs = importBody({ id: contested.id })
    const raced = await Promise.all([s.call("alice", "/notes/import", "POST", mine), s.call("bob", "/notes/import", "POST", theirs)])
    expect(raced.map(answer => answer.status).sort()).toEqual([200, 409])
    expect(raced.find(answer => answer.status === 409)!.text).toBe(conflict)
    expect(await db.note.count({ where: { id: contested.id } })).toBe(1)
  })

  test("an import that fails part-way leaves nothing behind, and the same request then succeeds", async () => {
    const s = await start(), db = s.db!, body = importBody({ transcriptSegments: Array.from({ length: 2500 }, (_, index) => segment(index)) })
    // Make the third batch of segments fail: another note already holds the id the last segment would get.
    const blocker = await s.call("bob", "/notes", "POST", { title: "In the way", transcriptSegments: [{ ...segment(0), id: expectedSegmentId(body.id, body.transcriptSegments[2499]!.id) }] })
    expect(blocker.status).toBe(200)
    const failed = await s.call("alice", "/notes/import", "POST", body)
    // Answered like any id that is taken: it says nothing about whose row was in the way.
    expect([failed.status, failed.text]).toEqual([409, conflict])
    expect(await db.note.count({ where: { id: body.id } })).toBe(0)
    expect(await db.transcriptSegment.count({ where: { noteId: body.id } })).toBe(0)
    expect(await db.noteMeetingContext.count({ where: { noteId: body.id } })).toBe(0)
    expect((await s.call("alice", `/notes/${body.id}`)).status).toBe(404)
    // Bob's note is as it was.
    expect((await s.call("bob", `/notes/${blocker.json.id}`)).json.transcriptSegments).toHaveLength(1)
    expect((await s.call("bob", `/notes/${blocker.json.id}`, "DELETE")).status).toBe(200)
    const retried = await s.call("alice", "/notes/import", "POST", body)
    expect(retried.status).toBe(200)
    expect(await db.transcriptSegment.count({ where: { noteId: body.id } })).toBe(2500)
  })

  test("the import follows the caller's active workspace, as a new note does", async () => {
    const s = await start(), db = s.db!
    const made = await s.call("alice", "/notes", "POST", { title: "Made here" }), body = importBody()
    expect((await s.call("alice", "/notes/import", "POST", body)).status).toBe(200)
    const [ordinary, imported] = await Promise.all([db.note.findUniqueOrThrow({ where: { id: made.json.id } }), db.note.findUniqueOrThrow({ where: { id: body.id } })])
    expect(imported.organizationId).toBe(ordinary.organizationId)
  })
})
