import { afterAll, beforeAll, describe, expect, test } from "bun:test"
import type { PrincipalData } from "@meeting-notes/contracts/api"
import { createDatabase } from "@meeting-notes/database"
import { makeApiHandler } from "../src/api"
import { makePrismaRepository } from "../src/prisma-repository"
import { makePrismaTeamSpaceStore, makeTeamSpaceService } from "../src/team-space"

const url = process.env.DATABASE_URL
const enabled = process.env.RUN_PG_TESTS === "1" && !!url && /^\/mn_test_[a-f0-9]+$/.test(new URL(url).pathname)

/** The same rules as team-space.test.ts, against the real tables: membership, the grant row and its cascades. */
describe.skipIf(!enabled)("the team space in PostgreSQL", () => {
  const resources = enabled ? makePrismaRepository(url!) : null
  const db = enabled ? createDatabase(url!) : null
  const people = { lead: crypto.randomUUID(), admin: crypto.randomUUID(), ada: crypto.randomUUID(), ben: crypto.randomUUID(), outsider: crypto.randomUUID() }
  type Person = keyof typeof people
  const team = crypto.randomUUID(), elsewhere = crypto.randomUUID(), tag = crypto.randomUUID().slice(0, 8)
  const api = resources ? makeApiHandler(resources.repository, "team-space-postgres-bootstrap-token-32-chars", {
    teamSpace: makeTeamSpaceService(makePrismaTeamSpaceStore(db!)),
    principalResolver: async headers => {
      const who = (headers.get("authorization") ?? "").replace("Bearer ", "") as Person
      if (!(who in people)) return null
      return { id: people[who], name: who, email: `${who}-${tag}@example.test`, role: "user", source: "session", expiresAt: "2099-01-01T00:00:00Z", createdAt: "2026-01-01T00:00:00Z" } satisfies PrincipalData
    },
  }) : null
  const call = (who: Person, path: string, method = "GET", body?: unknown) => api!.handler(new Request(`http://localhost/api/v1${path}`, {
    method, headers: { Authorization: `Bearer ${who}`, ...(body === undefined ? {} : { "Content-Type": "application/json" }) }, ...(body === undefined ? {} : { body: JSON.stringify(body) }) }))
  const json = async (who: Person, path: string, method = "GET", body?: unknown) => { const response = await call(who, path, method, body); return { status: response.status, body: await response.json().catch(() => null) as any } }
  const note = async (who: Person, title: string, extra: Record<string, unknown> = {}) => (await json(who, "/notes", "POST", { title, ...extra })).body.id as string
  const ids = (page: { body: { notes: Array<{ id: string }> } }) => page.body.notes.map(row => row.id)

  beforeAll(async () => {
    await db!.organization.create({ data: { id: team, name: "Acme", slug: `acme-${tag}` } })
    await db!.organization.create({ data: { id: elsewhere, name: "Elsewhere", slug: `elsewhere-${tag}` } })
    for (const [name, id] of Object.entries(people)) {
      await db!.user.create({ data: { id, name, email: `${name}-${tag}@example.test`, ...(name === "ada" ? { image: "https://lh3.googleusercontent.com/a/fixture=s96-c" } : {}) } })
      const organizationId = name === "outsider" ? elsewhere : team
      await db!.member.create({ data: { id: crypto.randomUUID(), organizationId, userId: id, role: name === "lead" ? "owner" : name === "admin" ? "admin" : "member" } })
      await db!.userWorkspaceState.create({ data: { userId: id, activeOrganizationId: organizationId } })
    }
  })
  afterAll(async () => {
    await api?.dispose()
    await db?.organization.deleteMany({ where: { id: { in: [team, elsewhere] } } })
    await db?.user.deleteMany({ where: { id: { in: Object.values(people) } } })
    await db?.$disconnect()
    await resources?.close()
  })

  test("members read what an owner adds; people outside the workspace see nothing of it", async () => {
    const id = await note("ada", "Customer interview", { body: "ADA_NOTES", enhanced: "## Summary" }), hidden = await note("ada", "Private")
    expect((await json("ada", "/team-space/notes", "POST", { noteIds: [id] })).status).toBe(200)
    for (const who of ["lead", "admin", "ada", "ben"] as const) {
      const page = await json(who, "/team-space")
      expect(page.body.organization).toEqual({ id: team, name: "Acme" })
      expect(page.body.members.map((member: { name: string }) => member.name).sort()).toEqual(["ada", "admin", "ben", "lead"])
      expect(ids(page)).toEqual([id])
      expect(page.body.notes[0].owner).toEqual({ userId: people.ada, name: "ada", email: `ada-${tag}@example.test`, image: "https://lh3.googleusercontent.com/a/fixture=s96-c" })
      expect(page.body.notes[0]).toMatchObject({ mine: who === "ada", canRemove: who !== "ben" })
      expect((await json(who, `/team-space/notes/${id}`)).body).toMatchObject({ title: "Customer interview", body: "ADA_NOTES", enhanced: "## Summary" })
      expect((await call(who, `/team-space/notes/${hidden}`)).status).toBe(404)
    }
    // The outsider has a space of their own workspace, and this note is not in it.
    expect(ids(await json("outsider", "/team-space"))).toEqual([])
    expect((await call("outsider", `/team-space/notes/${id}`)).status).toBe(404)
    expect((await call("outsider", `/team-space/notes/${id}`, "DELETE")).status).toBe(404)
    expect((await call("outsider", "/team-space/notes", "POST", { noteIds: [id] })).status).toBe(404)
    expect((await db!.teamSpaceNote.findUniqueOrThrow({ where: { noteId: id } })).organizationId).toBe(team)
    await call("ada", `/team-space/notes/${id}`, "DELETE")
  })

  test("someone in no workspace at all is told 404", async () => {
    const membership = await db!.member.findFirstOrThrow({ where: { userId: people.ben } })
    const id = await note("ada", "For the team")
    await call("ada", "/team-space/notes", "POST", { noteIds: [id] })
    expect((await call("ben", `/team-space/notes/${id}`)).status).toBe(200)
    await db!.member.delete({ where: { id: membership.id } })
    expect((await call("ben", "/team-space")).status).toBe(404)
    expect((await call("ben", `/team-space/notes/${id}`)).status).toBe(404)
    await db!.member.create({ data: { id: membership.id, organizationId: team, userId: people.ben, role: "member" } })
    expect((await call("ben", `/team-space/notes/${id}`)).status).toBe(200)
    await call("ada", `/team-space/notes/${id}`, "DELETE")
  })

  test("adding is the owner's alone and all or nothing; removing is the owner's or an administrator's and revokes at once", async () => {
    const adas = await note("ada", "Ada's"), bens = await note("ben", "Ben's")
    for (const who of ["ben", "admin", "lead"] as const) expect((await call(who, "/team-space/notes", "POST", { noteIds: [adas] })).status).toBe(404)
    expect((await call("ben", "/team-space/notes", "POST", { noteIds: [bens, adas] })).status).toBe(404)
    expect(await db!.teamSpaceNote.count({ where: { noteId: { in: [adas, bens] } } })).toBe(0)
    expect(ids(await json("ada", "/team-space/notes", "POST", { noteIds: [adas] }))).toEqual([adas])
    const added = (await db!.teamSpaceNote.findUniqueOrThrow({ where: { noteId: adas } })).addedAt
    await call("ada", "/team-space/notes", "POST", { noteIds: [adas] })
    expect((await db!.teamSpaceNote.findUniqueOrThrow({ where: { noteId: adas } })).addedAt).toEqual(added)
    expect((await call("ben", `/team-space/notes/${adas}`, "DELETE")).status).toBe(403)
    expect((await call("ben", `/team-space/notes/${adas}`)).status).toBe(200)
    expect((await call("admin", `/team-space/notes/${adas}`, "DELETE")).status).toBe(200)
    expect((await call("ben", `/team-space/notes/${adas}`)).status).toBe(404)
    expect((await call("ada", `/notes/${adas}`)).status).toBe(200)
  })

  test("nobody but the owner changes or deletes a note in the space, and deleting it empties its place", async () => {
    const id = await note("ada", "Board prep", { body: "ORIGINAL" })
    await call("ada", "/team-space/notes", "POST", { noteIds: [id] })
    for (const who of ["ben", "admin", "lead"] as const) {
      expect((await call(who, `/notes/${id}`)).status).toBe(404)
      expect((await call(who, `/notes/${id}`, "PATCH", { body: "overwritten" })).status).toBe(404)
      expect((await call(who, `/notes/${id}`, "DELETE")).status).toBe(404)
    }
    expect((await json("ben", `/team-space/notes/${id}`)).body.body).toBe("ORIGINAL")
    expect((await call("ada", `/notes/${id}`, "DELETE")).status).toBe(200)
    expect((await call("ben", `/team-space/notes/${id}`)).status).toBe(404)
    expect(await db!.teamSpaceNote.count({ where: { noteId: id } })).toBe(0)
  })

  test("People lists the attendees of the space's notes, from each note's saved meeting", async () => {
    const id = await note("ada", "Office visit", { date: "2026-07-07" }), solo = await note("ben", "Ben alone", { date: "2026-07-08" })
    const participant = (name: string, email: string) => ({ id: crypto.randomUUID(), email, displayName: name, givenName: null, familyName: null, nameSource: "calendar", isSelf: false, isOrganizer: false, isResource: false, responseStatus: "accepted" })
    const meeting = (...participants: Array<ReturnType<typeof participant>>) => ({ eventId: crypto.randomUUID(), provider: "ics", title: "Meeting", calendarName: "Work", startAt: "2026-07-07T17:30:00.000Z", endAt: "2026-07-07T18:00:00.000Z", allDay: false, timeZone: null, location: null, meetingUrl: null, eventUrl: null, organizer: null, participants })
    await db!.noteMeetingContext.create({ data: { noteId: id, userId: people.ada, meeting: meeting(participant("Kai Example", "kai@example.test"), participant("Noor Sample", "noor@example.test")) } })
    await db!.noteMeetingContext.create({ data: { noteId: solo, userId: people.ben, meeting: meeting(participant("Only Ben Met", "solo@example.test")) } })
    const before = await json("ben", "/team-space")
    expect(before.body.peopleSource).toBe("own")
    expect(before.body.people.map((person: { name: string }) => person.name)).toEqual(["Only Ben Met"])
    expect((await json("lead", "/team-space")).body.people).toEqual([])
    await call("ada", "/team-space/notes", "POST", { noteIds: [id] })
    const page = await json("ben", "/team-space")
    expect(page.body.peopleSource).toBe("team")
    expect(page.body.people.map((person: { name: string; notes: number }) => [person.name, person.notes])).toEqual([["Kai Example", 1], ["Noor Sample", 1]])
    expect(page.body.notes[0].attendees).toMatchObject({ total: 2 })
  })
})
