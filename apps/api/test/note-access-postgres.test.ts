import { afterAll, beforeAll, describe, expect, test } from "bun:test"
import type { PrincipalData } from "@meeting-notes/contracts/api"
import { createDatabase } from "@meeting-notes/database"
import { makeApiHandler } from "../src/api"
import { makePrismaRepository } from "../src/prisma-repository"
import { makeNoteAccessService, makePrismaNoteAccessStore } from "../src/note-access"

const url = process.env.DATABASE_URL
const enabled = process.env.RUN_PG_TESTS === "1" && !!url && /^\/mn_test_[a-f0-9]+$/.test(new URL(url).pathname)

/** The same rules as note-access.test.ts, against the real tables: membership, the rows and their cascades. */
describe.skipIf(!enabled)("sharing a note with people of its workspace in PostgreSQL", () => {
  const resources = enabled ? makePrismaRepository(url!) : null
  const db = enabled ? createDatabase(url!) : null
  const people = { ada: crypto.randomUUID(), ben: crypto.randomUUID(), cy: crypto.randomUUID(), outsider: crypto.randomUUID() }
  type Person = keyof typeof people
  const team = crypto.randomUUID(), elsewhere = crypto.randomUUID(), tag = crypto.randomUUID().slice(0, 8)
  const mail = (who: Person) => who === "outsider" ? `outsider-${tag}@elsewhere.test` : `${who}-${tag}@example.test`
  const api = resources ? makeApiHandler(resources.repository, "note-access-postgres-bootstrap-token-32-chars", {
    noteAccess: makeNoteAccessService(makePrismaNoteAccessStore(db!, { signInDomains: async () => ["@example.test", "someone@elsewhere.test"] })),
    principalResolver: async headers => {
      const who = (headers.get("authorization") ?? "").replace("Bearer ", "") as Person
      if (!(who in people)) return null
      return { id: people[who], name: who, email: mail(who), role: "user", source: "session", expiresAt: "2099-01-01T00:00:00Z", createdAt: "2026-01-01T00:00:00Z" } satisfies PrincipalData
    },
  }) : null
  const call = (who: Person, path: string, method = "GET", body?: unknown) => api!.handler(new Request(`http://localhost/api/v1${path}`, {
    method, headers: { Authorization: `Bearer ${who}`, ...(body === undefined ? {} : { "Content-Type": "application/json" }) }, ...(body === undefined ? {} : { body: JSON.stringify(body) }) }))
  const json = async (who: Person, path: string, method = "GET", body?: unknown) => { const response = await call(who, path, method, body); return { status: response.status, body: await response.json().catch(() => null) as any } }
  const note = async (who: Person, title: string, extra: Record<string, unknown> = {}) => (await json(who, "/notes", "POST", { title, ...extra })).body.id as string
  const link = async (who: Person, id: string) => (await json(who, `/notes/${id}/shares`, "POST", { includeBody: true, includeEnhanced: true })).body.token as string

  beforeAll(async () => {
    await db!.organization.create({ data: { id: team, name: "Acme", slug: `acme-${tag}` } })
    await db!.organization.create({ data: { id: elsewhere, name: "Elsewhere", slug: `elsewhere-${tag}` } })
    for (const [name, id] of Object.entries(people)) {
      await db!.user.create({ data: { id, name, email: mail(name as Person), ...(name === "ben" ? { image: "https://lh3.googleusercontent.com/a/fixture=s96-c" } : {}) } })
      const organizationId = name === "outsider" ? elsewhere : team
      await db!.member.create({ data: { id: crypto.randomUUID(), organizationId, userId: id, role: "member" } })
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

  test("the owner shares with members only, and the people it is shared with read it", async () => {
    const id = await note("ada", "Planning", { body: "ADA_NOTES", enhanced: "## Summary" })
    const participant = (name: string, email: string) => ({ id: crypto.randomUUID(), email, displayName: name, givenName: null, familyName: null, nameSource: "calendar", isSelf: false, isOrganizer: false, isResource: false, responseStatus: "accepted" })
    await db!.noteMeetingContext.create({ data: { noteId: id, userId: people.ada, meeting: { eventId: crypto.randomUUID(), provider: "ics", title: "Meeting", calendarName: "Work", startAt: "2026-07-07T17:30:00.000Z", endAt: "2026-07-07T18:00:00.000Z", allDay: false, timeZone: null, location: null, meetingUrl: null, eventUrl: null, organizer: null, participants: [participant("Ben", mail("ben").toUpperCase()), participant("Outsider", mail("outsider")), participant("Guest", "guest@elsewhere.test")] } } })
    const stamp = (await db!.note.findUniqueOrThrow({ where: { id } })).updatedAt
    const access = await json("ada", `/notes/${id}/access`)
    expect(access.body).toMatchObject({ level: "invited", suspended: false, organization: { id: team, name: "Acme", domain: "example.test" }, owner: { userId: people.ada }, people: [] })
    expect(access.body.attendees).toEqual([{ userId: people.ben, name: "ben", email: mail("ben"), image: "https://lh3.googleusercontent.com/a/fixture=s96-c", hasAccess: false }])
    expect((await json("ada", `/notes/${id}/access/candidates`)).body.people.map((row: { name: string; attendee: boolean }) => [row.name, row.attendee])).toEqual([["ben", true], ["cy", false]])
    expect((await json("ada", `/notes/${id}/access/candidates?q=outsider`)).body.people).toEqual([])
    for (const body of [{ userIds: [people.outsider] }, { emails: [mail("outsider")] }, { userIds: [people.ben, people.outsider] }]) expect((await call("ada", `/notes/${id}/access/people`, "POST", body)).status).toBe(400)
    expect(await db!.noteAccess.count({ where: { noteId: id } })).toBe(0)
    expect((await json("ada", `/notes/${id}/access/people`, "POST", { userIds: [people.ben] })).body.people.map((row: { userId: string }) => row.userId)).toEqual([people.ben])
    expect((await json("ben", "/shared-with-me")).body.notes.map((row: { id: string }) => row.id)).toEqual([id])
    expect((await json("ben", `/shared-with-me/${id}`)).body).toMatchObject({ title: "Planning", body: "ADA_NOTES", enhanced: "## Summary", owner: { userId: people.ada } })
    for (const who of ["cy", "outsider"] as const) expect((await call(who, `/shared-with-me/${id}`)).status).toBe(404)
    for (const who of ["ben", "cy", "outsider"] as const) for (const [path, method, body] of [[`/notes/${id}`, "GET"], [`/notes/${id}`, "PATCH", { body: "x" }], [`/notes/${id}`, "DELETE"], [`/notes/${id}/access`, "GET"], [`/notes/${id}/access`, "PATCH", { level: "organization" }], [`/notes/${id}/access/people`, "POST", { userIds: [people.cy] }]] as const)
      expect((await call(who, path, method, body)).status, `${who}: ${method} ${path}`).toBe(404)
    // Sharing never touches the note itself.
    expect((await db!.note.findUniqueOrThrow({ where: { id } })).updatedAt).toEqual(stamp)
  })

  test("the level decides who a link reaches; removing, tightening, trashing, leaving and deleting each end access at once", async () => {
    const id = await note("ada", "Roadmap", { body: "ADA_NOTES" }), token = await link("ada", id)
    for (const who of ["ben", "cy", "outsider"] as const) expect((await call(who, `/shared/${token}`)).status).toBe(404)
    await call("ada", `/notes/${id}/access/people`, "POST", { emails: [mail("ben")] })
    expect((await call("ben", `/shared/${token}`)).status).toBe(200)
    expect((await call("cy", `/shared/${token}`)).status).toBe(404)
    expect((await json("ada", `/notes/${id}/access`, "PATCH", { level: "organization" })).body.level).toBe("organization")
    expect((await call("cy", `/shared/${token}`)).status).toBe(200)
    expect((await call("outsider", `/shared/${token}`)).status).toBe(404)
    expect((await call("ada", `/notes/${id}/access`, "PATCH", { level: "public" })).status).toBe(400)
    // Trash.
    await call("ada", `/notes/${id}/access`, "PATCH", { suspended: true })
    for (const who of ["ben", "cy"] as const) expect((await call(who, `/shared/${token}`)).status).toBe(404)
    expect((await call("ben", `/shared-with-me/${id}`)).status).toBe(404)
    expect((await json("ben", "/shared-with-me")).body.notes.map((row: { id: string }) => row.id)).not.toContain(id)
    expect((await json("ada", `/notes/${id}/access`, "PATCH", { suspended: false })).body).toMatchObject({ suspended: false, level: "organization" })
    // Tightening, then removing.
    await call("ada", `/notes/${id}/access`, "PATCH", { level: "invited" })
    expect((await call("cy", `/shared/${token}`)).status).toBe(404)
    expect((await call("ben", `/shared/${token}`)).status).toBe(200)
    await call("ada", `/notes/${id}/access/people/${people.ben}`, "DELETE")
    expect((await call("ben", `/shared/${token}`)).status).toBe(404)
    expect((await call("ben", `/shared-with-me/${id}`)).status).toBe(404)
    // Leaving the workspace: the row stays and gives nothing.
    await call("ada", `/notes/${id}/access/people`, "POST", { userIds: [people.cy] })
    expect((await call("cy", `/shared-with-me/${id}`)).status).toBe(200)
    const membership = await db!.member.findFirstOrThrow({ where: { userId: people.cy } })
    await db!.member.delete({ where: { id: membership.id } })
    expect((await call("cy", `/shared-with-me/${id}`)).status).toBe(404)
    expect((await call("cy", `/shared/${token}`)).status).toBe(404)
    expect((await json("ada", `/notes/${id}/access`)).body.people).toEqual([])
    await db!.member.create({ data: { id: membership.id, organizationId: team, userId: people.cy, role: "member" } })
    expect((await call("cy", `/shared-with-me/${id}`)).status).toBe(200)
    // The team space is a second way in for members, and only for them.
    await db!.noteAccess.deleteMany({ where: { noteId: id } })
    expect((await call("cy", `/shared/${token}`)).status).toBe(404)
    await db!.teamSpaceNote.create({ data: { noteId: id, organizationId: team, addedById: people.ada } })
    expect((await call("cy", `/shared/${token}`)).status).toBe(200)
    expect((await call("outsider", `/shared/${token}`)).status).toBe(404)
    // Deleting the note takes every row with it.
    await call("ada", `/notes/${id}/access/people`, "POST", { userIds: [people.cy] })
    expect((await call("ada", `/notes/${id}`, "DELETE")).status).toBe(200)
    expect(await db!.noteAccess.count({ where: { noteId: id } })).toBe(0)
    expect(await db!.noteSharing.count({ where: { noteId: id } })).toBe(0)
    expect((await call("cy", `/shared-with-me/${id}`)).status).toBe(404)
  })
})
