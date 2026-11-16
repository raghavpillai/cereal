import { afterEach, expect, test } from "bun:test"
import type { MeetingContextDTO } from "@meeting-notes/contracts"
import type { PrincipalData } from "@meeting-notes/contracts/api"
import { makeApiHandler } from "../src/api"
import { makeMemoryRepository } from "../src/repository"
import { makeMemoryTeamSpaceStore, makeTeamSpaceService, teamSpacePeople } from "../src/team-space"

const close: Array<() => Promise<void>> = []
afterEach(async () => { for (const dispose of close.splice(0)) await dispose() })
const ids = {
  lead: "30000000-0000-4000-8000-000000000001", admin: "30000000-0000-4000-8000-000000000002",
  ada: "30000000-0000-4000-8000-000000000003", ben: "30000000-0000-4000-8000-000000000004",
  outsider: "30000000-0000-4000-8000-000000000005",
}
type Account = keyof typeof ids
const organization = { id: "30000000-0000-4000-8000-0000000000aa", name: "Acme" }
const participant = (name: string, email: string | null, extra: Partial<MeetingContextDTO["participants"][number]> = {}): MeetingContextDTO["participants"][number] =>
  ({ id: crypto.randomUUID(), email, displayName: name, givenName: null, familyName: null, nameSource: "calendar", isSelf: false, isOrganizer: false, isResource: false, responseStatus: "accepted", ...extra })
const meetingWith = (...participants: MeetingContextDTO["participants"]): MeetingContextDTO =>
  ({ eventId: crypto.randomUUID(), provider: "ics", title: "Meeting", calendarName: "Work", startAt: "2026-07-07T17:30:00.000Z", endAt: "2026-07-07T18:00:00.000Z", allDay: false, timeZone: null, location: null, meetingUrl: null, eventUrl: null, organizer: null, participants })

function fixture() {
  const repository = makeMemoryRepository()
  const meetings = new Map<string, MeetingContextDTO>()
  const members = (["lead", "admin", "ada", "ben"] as const).map(name => ({ userId: ids[name], name, email: `${name}@example.test`, role: name === "lead" ? "owner" as const : name === "admin" ? "admin" as const : "member" as const }))
  const teamSpace = makeTeamSpaceService(makeMemoryTeamSpaceStore(repository, { organization, members, meeting: async (_owner, noteId) => meetings.get(noteId) ?? null }))
  const api = makeApiHandler(repository, "team-space-bootstrap-test-at-least-32-characters", {
    teamSpace,
    principalResolver: async headers => {
      const account = (headers.get("authorization") ?? "").replace("Bearer ", "") as Account
      if (!(account in ids)) return null
      return { id: ids[account], name: account, email: `${account}@example.test`, role: "user", source: "session", expiresAt: "2099-01-01T00:00:00Z", createdAt: "2026-01-01T00:00:00Z" } satisfies PrincipalData
    },
  })
  close.push(api.dispose)
  const call = (account: Account | null, path: string, method = "GET", body?: unknown) => api.handler(new Request(`http://localhost/api/v1${path}`, {
    method, headers: { ...(account ? { Authorization: `Bearer ${account}` } : {}), ...(body === undefined ? {} : { "Content-Type": "application/json" }) },
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
  }))
  const note = async (account: Account, title: string, extra: Record<string, unknown> = {}) => (await (await call(account, "/notes", "POST", { title, ...extra })).json()).id as string
  return { call, note, members, meetings }
}

test("a note its owner adds is listed for, and readable by, every member of the workspace", async () => {
  const { call, note } = fixture()
  const id = await note("ada", "Customer interview", { body: "ADA_OWN_NOTES", enhanced: "## Summary\nShipped." })
  const hidden = await note("ada", "Private one", { body: "ADA_PRIVATE" })
  expect((await (await call("ben", "/team-space")).json()).notes).toEqual([])
  const added = await call("ada", "/team-space/notes", "POST", { noteIds: [id] })
  expect(added.status).toBe(200)
  for (const account of ["lead", "admin", "ada", "ben"] as const) {
    const page = await (await call(account, "/team-space")).json()
    expect(page.organization).toEqual(organization)
    expect(page.members.map((member: { name: string }) => member.name)).toEqual(["lead", "admin", "ada", "ben"])
    expect(page.notes.map((row: { id: string }) => row.id)).toEqual([id])
    expect(page.notes[0].owner).toEqual({ userId: ids.ada, name: "ada", email: "ada@example.test" })
    expect(page.notes[0].mine).toBe(account === "ada")
    expect(page.notes[0].canRemove).toBe(account !== "ben")
    const read = await call(account, `/team-space/notes/${id}`)
    expect(read.status).toBe(200)
    const content = await read.json()
    expect(content).toMatchObject({ id, title: "Customer interview", body: "ADA_OWN_NOTES", enhanced: "## Summary\nShipped.", mine: account === "ada" })
    // The documents a shared link carries, and nothing else of the note.
    expect(Object.keys(content).sort()).toEqual(["addedAt", "body", "date", "duration", "enhanced", "id", "mine", "owner", "title"])
    // A note that is not in the space stays its owner's alone.
    expect((await call(account, `/team-space/notes/${hidden}`)).status).toBe(404)
  }
})

test("someone who is not a member, or not signed in, sees neither the space nor anything in it", async () => {
  const { call, note } = fixture()
  const id = await note("ada", "Roadmap review", { body: "TEAM_ONLY" })
  expect((await call("ada", "/team-space/notes", "POST", { noteIds: [id] })).status).toBe(200)
  const own = await note("outsider", "Outsider's own")
  for (const [path, method, body] of [["/team-space", "GET"], [`/team-space/notes/${id}`, "GET"], ["/team-space/notes", "POST", { noteIds: [own] }], [`/team-space/notes/${id}`, "DELETE"]] as const) {
    const refused = await call("outsider", path, method, body)
    expect(refused.status, `outsider ${method} ${path}`).toBe(404)
    expect(await refused.text()).not.toContain("TEAM_ONLY")
    expect((await call(null, path, method, body)).status, `signed out ${method} ${path}`).toBe(401)
  }
  // Refused requests changed nothing.
  expect((await (await call("ben", "/team-space")).json()).notes.map((row: { id: string }) => row.id)).toEqual([id])
})

test("a member who leaves the workspace loses the space with their next request", async () => {
  const { call, note, members } = fixture()
  const id = await note("ada", "Planning")
  await call("ada", "/team-space/notes", "POST", { noteIds: [id] })
  expect((await call("ben", `/team-space/notes/${id}`)).status).toBe(200)
  members.splice(members.findIndex(member => member.userId === ids.ben), 1)
  expect((await call("ben", "/team-space")).status).toBe(404)
  expect((await call("ben", `/team-space/notes/${id}`)).status).toBe(404)
})

test("only a note's owner can add it, and one note that is not theirs refuses the whole request", async () => {
  const { call, note } = fixture()
  const adas = await note("ada", "Ada's"), bens = await note("ben", "Ben's")
  for (const account of ["ben", "admin", "lead"] as const) expect((await call(account, "/team-space/notes", "POST", { noteIds: [adas] })).status).toBe(404)
  expect((await call("ben", "/team-space/notes", "POST", { noteIds: [bens, adas] })).status).toBe(404)
  expect((await call("ben", "/team-space/notes", "POST", { noteIds: [bens, crypto.randomUUID()] })).status).toBe(404)
  expect((await (await call("ada", "/team-space")).json()).notes).toEqual([])
  expect((await call("ben", "/team-space/notes", "POST", { noteIds: [] })).status).toBe(400)
  // Adding twice is the same as adding once.
  expect((await call("ben", "/team-space/notes", "POST", { noteIds: [bens, bens] })).status).toBe(200)
  const again = await (await call("ben", "/team-space/notes", "POST", { noteIds: [bens] })).json()
  expect(again.notes.map((row: { id: string }) => row.id)).toEqual([bens])
})

test("removing a note from the space revokes access at once; the owner and administrators may remove, other members may not", async () => {
  const { call, note } = fixture()
  const first = await note("ada", "First"), second = await note("ada", "Second"), third = await note("ada", "Third")
  await call("ada", "/team-space/notes", "POST", { noteIds: [first, second, third] })
  expect((await call("ben", `/team-space/notes/${first}`)).status).toBe(200)
  // Another member cannot take it out.
  const refused = await call("ben", `/team-space/notes/${first}`, "DELETE")
  expect(refused.status).toBe(403)
  expect((await call("ben", `/team-space/notes/${first}`)).status).toBe(200)
  // Its owner can.
  expect(await (await call("ada", `/team-space/notes/${first}`, "DELETE")).json()).toEqual({ deleted: true })
  expect((await call("ben", `/team-space/notes/${first}`)).status).toBe(404)
  expect((await call("lead", `/team-space/notes/${first}`)).status).toBe(404)
  expect((await call("ada", `/team-space/notes/${first}`, "DELETE")).status).toBe(404)
  // So can an administrator and the owner of the workspace.
  expect((await call("admin", `/team-space/notes/${second}`, "DELETE")).status).toBe(200)
  expect((await call("lead", `/team-space/notes/${third}`, "DELETE")).status).toBe(200)
  expect((await (await call("ben", "/team-space")).json()).notes).toEqual([])
  // Out of the space, each note is still its owner's, untouched.
  for (const id of [first, second, third]) expect((await call("ada", `/notes/${id}`)).status).toBe(200)
})

test("a member can never edit, delete, record on, share or chat with someone else's note in the space", async () => {
  const { call, note } = fixture()
  const id = await note("ada", "Board prep", { body: "ORIGINAL" })
  await call("ada", "/team-space/notes", "POST", { noteIds: [id] })
  for (const account of ["ben", "admin", "lead"] as const) {
    const attempts: Array<[string, string, unknown?]> = [
      [`/notes/${id}`, "GET"], [`/notes/${id}`, "PATCH", { title: "Taken over", body: "overwritten" }], [`/notes/${id}`, "DELETE"],
      [`/notes/${id}/recording`, "POST", { command: "start" }], [`/notes/${id}/generate`, "POST", { expectedTranscriptRevision: 0 }],
      [`/notes/${id}/shares`, "POST", { includeBody: true, includeEnhanced: true }], [`/notes/${id}/chat`, "POST", { message: "Read it", history: [] }],
      [`/notes/${id}/context`, "PUT", { eventId: null }],
    ]
    for (const [path, method, body] of attempts) expect((await call(account, path, method, body)).status, `${account}: ${method} ${path}`).not.toBe(200)
    // The space's own address takes no writes to a note at all.
    for (const method of ["PATCH", "PUT", "POST"]) expect([404, 405]).toContain((await call(account, `/team-space/notes/${id}`, method, { title: "Taken over" })).status)
  }
  expect(await (await call("ada", `/notes/${id}`)).json()).toMatchObject({ title: "Board prep", body: "ORIGINAL" })
  expect(await (await call("ben", `/team-space/notes/${id}`)).json()).toMatchObject({ title: "Board prep", body: "ORIGINAL" })
  // The owner's edits are what members read next.
  expect((await call("ada", `/notes/${id}`, "PATCH", { body: "REVISED" })).status).toBe(200)
  expect((await (await call("ben", `/team-space/notes/${id}`)).json()).body).toBe("REVISED")
})

test("deleting a note takes it out of the space", async () => {
  const { call, note } = fixture()
  const id = await note("ada", "Short lived"), kept = await note("ada", "Kept")
  await call("ada", "/team-space/notes", "POST", { noteIds: [id, kept] })
  expect((await call("ada", `/notes/${id}`, "DELETE")).status).toBe(200)
  expect((await call("ben", `/team-space/notes/${id}`)).status).toBe(404)
  expect((await (await call("ben", "/team-space")).json()).notes.map((row: { id: string }) => row.id)).toEqual([kept])
})

test("notes are newest meeting first with their attendees, and People lists everyone those meetings were with", async () => {
  const { call, note, meetings } = fixture()
  const older = await note("ada", "Vendor call", { date: "2026-06-29" }), newer = await note("ben", "Office visit", { date: "2026-07-07" })
  const personal = await note("ada", "One to one", { date: "2026-07-09" })
  meetings.set(older, meetingWith(participant("Kai Example", "kai@example.test"), participant("Ada", "ada@example.test", { isSelf: true })))
  meetings.set(newer, meetingWith(participant("Kai Example", "KAI@example.test"), participant("Noor Sample", "noor@example.test"), participant("Room 4", "room4@example.test", { isResource: true }), participant("Walk-in guest", null)))
  meetings.set(personal, meetingWith(participant("Only Ada Met", "solo@example.test")))
  // Nothing in the space yet: each reader is shown the people on their own notes, and nobody else's.
  const empty = await (await call("ada", "/team-space")).json()
  expect(empty.peopleSource).toBe("own")
  expect(empty.people.map((person: { name: string }) => person.name)).toEqual(["Only Ada Met", "Ada", "Kai Example"])
  expect((await (await call("lead", "/team-space")).json()).people).toEqual([])
  await call("ada", "/team-space/notes", "POST", { noteIds: [older] })
  await call("ben", "/team-space/notes", "POST", { noteIds: [newer] })
  const page = await (await call("lead", "/team-space")).json()
  expect(page.notes.map((row: { title: string }) => row.title)).toEqual(["Office visit", "Vendor call"])
  expect(page.notes[0].attendees).toMatchObject({ total: 3, others: 3 })
  expect(page.notes[0].meeting).toEqual({ startAt: "2026-07-07T17:30:00.000Z", endAt: "2026-07-07T18:00:00.000Z", allDay: false })
  expect(page.peopleSource).toBe("team")
  expect(page.people.map((person: { name: string; email: string | null; notes: number; lastNoteDate: string }) => [person.name, person.email, person.notes, person.lastNoteDate])).toEqual([
    ["Kai Example", "kai@example.test", 2, "2026-07-07"], ["Noor Sample", "noor@example.test", 1, "2026-07-07"], ["Walk-in guest", null, 1, "2026-07-07"], ["Ada", "ada@example.test", 1, "2026-06-29"],
  ])
  // Someone met only on a note outside the space is not in the team's list.
  expect(JSON.stringify(page.people)).not.toContain("solo@example.test")
})

test("people are told apart by address, and an address alone gains a name when one meeting gives it", () => {
  const people = teamSpacePeople([
    { noteId: "a", date: "2026-01-02", meeting: meetingWith(participant("", "sam@example.test", { displayName: null })) },
    { noteId: "b", date: "2026-01-05", meeting: meetingWith(participant("Sam Rivera", "Sam@Example.test")) },
    { noteId: "c", date: "2026-01-01", meeting: null },
  ])
  expect(people).toEqual([{ name: "Sam Rivera", email: "sam@example.test", initials: "SR", notes: 2, lastNoteDate: "2026-01-05", noteIds: ["a", "b"] }])
})

test("a server without a team space answers 404 rather than failing", async () => {
  const api = makeApiHandler(makeMemoryRepository(), "team-space-bootstrap-test-at-least-32-characters")
  close.push(api.dispose)
  const response = await api.handler(new Request("http://localhost/api/v1/team-space", { headers: { Authorization: "Bearer team-space-bootstrap-test-at-least-32-characters" } }))
  expect(response.status).toBe(404)
})
