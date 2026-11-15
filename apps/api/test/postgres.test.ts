import { afterAll, describe, expect, test } from "bun:test"
import { Effect } from "effect"
import { createDatabase } from "@meeting-notes/database"
import { makeApiHandler } from "../src/api"
import { makePrismaRepository } from "../src/prisma-repository"

const url = process.env.DATABASE_URL
const enabled = process.env.RUN_PG_TESTS === "1" && Boolean(url)

describe.skipIf(!enabled)("Prisma/PostgreSQL API integration", () => {
  const resources = enabled ? makePrismaRepository(url!) : null
  const inspection = enabled ? createDatabase(url!) : null
  const ownerToken = "integration-owner-token-at-least-32-characters"
  const api = resources ? makeApiHandler(resources.repository, ownerToken) : null
  const ownedIds: string[] = []
  afterAll(async () => {
    if (resources) for (const id of ownedIds) await Effect.runPromise(resources.repository.deleteNote(id))
    await api?.dispose()
    await resources?.close()
    await inspection?.$disconnect()
  })
  const request = (path: string, method = "GET", body?: unknown, owner = true) => api!.handler(new Request(`http://localhost${path}`, {
    method, headers: { ...(owner ? { Authorization: `Bearer ${ownerToken}` } : {}), ...(body === undefined ? {} : { "Content-Type": "application/json" }) },
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
  }))

  test("persists notes, separate lifecycle state, immutable shares and hashed tokens", async () => {
    const segment = { id: crypto.randomUUID(), channel: "system", speakerId: "remote-1", speakerLabel: null, text: "Private remote transcript", startMs: 10, endMs: null, isFinal: false }
    const response = await request("/api/v1/notes", "POST", { title: "Postgres integration note", body: "Private body", enhanced: "Published text", transcriptSegments: [segment] })
    expect(response.status).toBe(200)
    const note = await response.json()
    ownedIds.push(note.id)
    expect(note.transcriptRevision).toBe(1)
    expect(note.generatedRevision).toBeNull()
    expect(note.generationState).toBe("idle")
    expect((await (await request(`/api/v1/notes/${note.id}`)).json()).transcriptSegments[0].channel).toBe("system")

    const finalized = await (await request(`/api/v1/notes/${note.id}`, "PATCH", { transcriptSegments: [{ ...segment, endMs: 50, isFinal: true }] })).json()
    expect(finalized.transcriptRevision).toBe(1)
    expect(finalized.transcriptSegments[0].isFinal).toBe(true)
    const changed = { ...segment, text: "Updated spoken words", isFinal: true }
    const revisions = await Promise.all([1, 2].map(() => request(`/api/v1/notes/${note.id}`, "PATCH", { transcriptSegments: [changed] }).then((r) => r.json())))
    expect(revisions.map((n) => n.transcriptRevision)).toEqual([2, 2])

    const shared = await (await request(`/api/v1/notes/${note.id}/shares`, "POST", { includeBody: false, includeEnhanced: true })).json()
    expect(shared.token).toHaveLength(43)
    const row = await inspection!.sharedNote.findUniqueOrThrow({ where: { id: shared.share.id } })
    expect(row.tokenHash).toMatch(/^[a-f0-9]{64}$/)
    expect(row.tokenHash).not.toBe(shared.token)
    expect(JSON.stringify(row.snapshot)).not.toContain("Private remote transcript")
    expect(JSON.stringify(row.snapshot)).not.toContain("Updated spoken words")
    expect(JSON.stringify(row.snapshot)).not.toContain("Private body")
    await request(`/api/v1/notes/${note.id}`, "PATCH", { enhanced: "Private later edit" })
    // A share is read by someone signed in; without an account the answer is 401 whether the link is live or not.
    expect((await request(`/api/v1/shared/${shared.token}`, "GET", undefined, false)).status).toBe(401)
    const sharedResponse = await request(`/api/v1/shared/${shared.token}`)
    expect(sharedResponse.status).toBe(200)
    expect((await sharedResponse.json()).enhanced).toBe("Published text")
    expect((await request(`/api/v1/shares/${shared.share.id}`, "DELETE")).status).toBe(200)
    expect((await request(`/api/v1/shared/${shared.token}`)).status).toBe(404)
    expect((await request(`/api/v1/shared/${shared.token}`, "GET", undefined, false)).status).toBe(401)
    expect((await request(`/api/v1/notes/${note.id}`, "DELETE")).status).toBe(200)
    expect(await inspection!.sharedNote.count({ where: { noteId: note.id } })).toBe(0)
  })
})
