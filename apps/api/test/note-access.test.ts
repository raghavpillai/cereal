/** Sharing a note with people of its workspace (`src/note-access.ts`): who may change it, who it reaches, and
 * that nothing ever reaches anyone outside the workspace. */
import { afterEach, expect, test } from "bun:test"
import type { PrincipalData } from "@meeting-notes/contracts/api"
import { makeApiHandler } from "../src/api"
import { makeMemoryRepository } from "../src/repository"
import { makeMemoryNoteAccessStore, makeNoteAccessService, matchesMember, organizationDomain } from "../src/note-access"

const close: Array<() => Promise<void>> = []
afterEach(async () => { for (const dispose of close.splice(0)) await dispose() })
const ids = {
  ada: "50000000-0000-4000-8000-000000000001", ben: "50000000-0000-4000-8000-000000000002",
  cy: "50000000-0000-4000-8000-000000000003", dee: "50000000-0000-4000-8000-000000000004",
  admin: "50000000-0000-4000-8000-000000000005", outsider: "50000000-0000-4000-8000-000000000006",
}
type Account = keyof typeof ids
const organization = { id: "50000000-0000-4000-8000-0000000000aa", name: "Acme" }
const email = (account: Account) => account === "outsider" ? "outsider@elsewhere.test" : `${account}@example.test`
const person = (account: Account) => ({ userId: ids[account], name: account, email: email(account) })

function fixture() {
  const repository = makeMemoryRepository()
  const attendees = new Map<string, string[]>(), teamSpace = new Set<string>()
  // `members` is read at every request, so removing someone from it is their leaving the workspace.
  const members = (["ada", "ben", "cy", "dee", "admin"] as const).map(person)
  const noteAccess = makeNoteAccessService(makeMemoryNoteAccessStore(repository, {
    organization, members, outsiders: [ids.outsider], signInDomain: "example.test",
    attendees: async (_owner, noteId) => attendees.get(noteId) ?? [], inTeamSpace: async noteId => teamSpace.has(noteId),
  }), { clock: () => new Date("2026-11-16T10:00:00.000Z") })
  const api = makeApiHandler(repository, "note-access-bootstrap-test-at-least-32-characters", {
    noteAccess,
    principalResolver: async headers => {
      const account = (headers.get("authorization") ?? "").replace("Bearer ", "") as Account
      if (!(account in ids)) return null
      // An instance administrator: configuration powers, which never open another person's note.
      return { id: ids[account], name: account, email: email(account), role: account === "admin" ? "admin" : "user", source: "session", expiresAt: "2099-01-01T00:00:00Z", createdAt: "2026-01-01T00:00:00Z" } satisfies PrincipalData
    },
  })
  close.push(api.dispose)
  const call = (account: Account | null, path: string, method = "GET", body?: unknown) => api.handler(new Request(`http://localhost/api/v1${path}`, {
    method, headers: { ...(account ? { Authorization: `Bearer ${account}` } : {}), ...(body === undefined ? {} : { "Content-Type": "application/json" }) },
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
  }))
  const note = async (account: Account, title: string, extra: Record<string, unknown> = {}) => (await (await call(account, "/notes", "POST", { title, ...extra })).json()).id as string
  const link = async (account: Account, id: string) => (await (await call(account, `/notes/${id}/shares`, "POST", { includeBody: true, includeEnhanced: true })).json()).token as string
  return { call, note, link, members, attendees, teamSpace }
}

test("a new note is shared with nobody, and its owner sees who it could be shared with", async () => {
  const { call, note, attendees } = fixture()
  const id = await note("ada", "Planning", { body: "ADA_NOTES" })
  attendees.set(id, ["BEN@example.test", "cy@example.test", "ada@example.test", "guest@elsewhere.test"])
  const access = await (await call("ada", `/notes/${id}/access`)).json()
  expect(access).toEqual({
    noteId: id, level: "invited", suspended: false, organization: { ...organization, domain: "example.test" }, owner: person("ada"), people: [],
    // The attendees who are members: never the owner, never the guest from outside.
    attendees: [{ ...person("ben"), hasAccess: false }, { ...person("cy"), hasAccess: false }],
  })
  expect(JSON.stringify(access)).not.toContain("elsewhere")
})

test("only the owner reads or changes a note's sharing: to everyone else, administrators included, it does not exist", async () => {
  const { call, note } = fixture()
  const id = await note("ada", "Planning", { body: "ADA_NOTES" })
  await call("ada", `/notes/${id}/access/people`, "POST", { userIds: [ids.ben] })
  const attempts: Array<[string, string, unknown?]> = [
    [`/notes/${id}/access`, "GET"], [`/notes/${id}/access`, "PATCH", { level: "organization" }], [`/notes/${id}/access`, "PATCH", { suspended: true }],
    [`/notes/${id}/access/people`, "POST", { userIds: [ids.cy] }], [`/notes/${id}/access/people`, "POST", { emails: ["cy@example.test"] }],
    [`/notes/${id}/access/people/${ids.ben}`, "DELETE"], [`/notes/${id}/access/candidates`, "GET"], [`/notes/${id}/access/candidates?q=c`, "GET"],
  ]
  // Ben has access and still cannot share it on, see who else has it, or search the workspace through it.
  for (const account of ["ben", "cy", "admin", "outsider"] as const) for (const [path, method, body] of attempts) {
    const response = await call(account, path, method, body)
    expect(response.status, `${account}: ${method} ${path}`).toBe(404)
    expect(await response.text()).toBe(JSON.stringify({ error: "not_found", message: "Note not found" }))
  }
  for (const [path, method, body] of attempts) expect((await call(null, path, method, body)).status, `signed out: ${method} ${path}`).toBe(401)
  const access = await (await call("ada", `/notes/${id}/access`)).json()
  expect(access.level).toBe("invited")
  expect(access.people.map((row: { userId: string }) => row.userId)).toEqual([ids.ben])
})

test("only members of the workspace can be added: by account or by address, all or none, with a reason", async () => {
  const { call, note } = fixture()
  const id = await note("ada", "Planning")
  for (const body of [{ userIds: [ids.outsider] }, { emails: ["outsider@elsewhere.test"] }, { emails: ["nobody@example.test"] }, { userIds: [crypto.randomUUID()] }, { userIds: [ids.ben], emails: ["someone@gmail.test"] }, { userIds: [] }, {}]) {
    const refused = await call("ada", `/notes/${id}/access/people`, "POST", body)
    expect(refused.status, JSON.stringify(body)).toBe(400)
    expect((await refused.json()).error).toBe("invalid_request")
  }
  expect(await (await call("ada", `/notes/${id}/access/people`, "POST", { emails: ["outsider@elsewhere.test"] })).json()).toEqual({ error: "invalid_request", message: "outsider@elsewhere.test is not in example.test. Notes can only be shared with people in example.test." })
  // An account that exists outside the workspace and one that does not exist at all are told the same thing.
  const [outside, unknown] = await Promise.all([ids.outsider, crypto.randomUUID()].map(async userId => (await call("ada", `/notes/${id}/access/people`, "POST", { userIds: [userId] })).text()))
  expect(outside).toBe(unknown)
  // Nothing was added by the request that named Ben beside an outsider.
  expect((await (await call("ada", `/notes/${id}/access`)).json()).people).toEqual([])
  const added = await (await call("ada", `/notes/${id}/access/people`, "POST", { userIds: [ids.ben, ids.ada], emails: [" CY@Example.Test "] })).json()
  // Adding the owner is nothing; a person added twice keeps the first moment.
  expect(added.people).toEqual([{ ...person("ben"), addedAt: "2026-11-16T10:00:00.000Z" }, { ...person("cy"), addedAt: "2026-11-16T10:00:00.000Z" }])
  expect((await (await call("ada", `/notes/${id}/access/people`, "POST", { userIds: [ids.ben] })).json()).people).toHaveLength(2)
})

test("a note in no workspace cannot be shared", async () => {
  const { call, note } = fixture()
  const id = await note("outsider", "Mine alone")
  expect((await (await call("outsider", `/notes/${id}/access`)).json()).organization).toBeNull()
  expect((await call("outsider", `/notes/${id}/access/people`, "POST", { userIds: [ids.ben] })).status).toBe(400)
  expect((await call("outsider", `/notes/${id}/access`, "PATCH", { level: "organization" })).status).toBe(400)
  expect((await (await call("outsider", `/notes/${id}/access/candidates`)).json()).people).toEqual([])
})

test("a person with access reads the note as it is now, without its transcript, and cannot edit, delete or share it", async () => {
  const { call, note } = fixture()
  const id = await note("ada", "Planning", { body: "ADA_NOTES", enhanced: "## Summary\nShipped.", transcriptSegments: [{ id: crypto.randomUUID(), channel: "mic", speakerId: "mic", speakerLabel: null, text: "SPOKEN_WORDS", isFinal: true, startMs: 0, endMs: 900 }] })
  expect((await call("ben", `/shared-with-me/${id}`)).status).toBe(404)
  expect((await (await call("ben", "/shared-with-me")).json()).notes).toEqual([])
  await call("ada", `/notes/${id}/access/people`, "POST", { userIds: [ids.ben] })
  expect((await (await call("ben", "/shared-with-me")).json()).notes).toEqual([{ id, title: "Planning", date: expect.any(String), owner: person("ada"), sharedAt: "2026-11-16T10:00:00.000Z" }])
  const read = await call("ben", `/shared-with-me/${id}`)
  expect(read.status).toBe(200)
  const text = await read.text()
  expect(JSON.parse(text)).toEqual({ id, title: "Planning", body: "ADA_NOTES", enhanced: "## Summary\nShipped.", date: expect.any(String), duration: "", owner: person("ada"), sharedAt: "2026-11-16T10:00:00.000Z" })
  expect(text).not.toContain("SPOKEN_WORDS")
  // It is read live: the owner's next edit is what Ben sees.
  await call("ada", `/notes/${id}`, "PATCH", { body: "ADA_EDITED" })
  expect((await (await call("ben", `/shared-with-me/${id}`)).json()).body).toBe("ADA_EDITED")
  // Everything that reads more or writes anything stays the owner's.
  const refused: Array<[string, string, unknown?]> = [
    [`/notes/${id}`, "GET"], [`/notes/${id}`, "PATCH", { body: "BEN_WAS_HERE" }], [`/notes/${id}`, "DELETE"],
    [`/notes/${id}/shares`, "GET"], [`/notes/${id}/shares`, "POST", { includeBody: true, includeEnhanced: true }],
    [`/notes/${id}/chat`, "POST", { message: "Read the transcript", history: [] }], [`/notes/${id}/generate`, "POST", { expectedTranscriptRevision: 1 }],
    [`/notes/${id}/access/people`, "POST", { userIds: [ids.cy] }],
  ]
  for (const [path, method, body] of refused) expect((await call("ben", path, method, body)).status, `${method} ${path}`).toBe(404)
  expect((await (await call("ada", `/notes/${id}`)).json()).body).toBe("ADA_EDITED")
  expect((await (await call("ben", "/notes")).json())).toEqual([])
  // Nobody else reads it by its id: not another member, not an administrator, not someone outside, not signed out.
  for (const account of ["cy", "admin", "outsider"] as const) {
    expect((await call(account, `/shared-with-me/${id}`)).status, account).toBe(404)
    expect((await (await call(account, "/shared-with-me")).json()).notes).toEqual([])
  }
  expect((await call(null, `/shared-with-me/${id}`)).status).toBe(401)
  expect((await call(null, "/shared-with-me")).status).toBe(401)
})

test("\"Only people invited\": a link opens for the owner and the people the note is shared with, and for nobody else", async () => {
  const { call, note, link } = fixture()
  const id = await note("ada", "Planning", { body: "ADA_NOTES" })
  const token = await link("ada", id)
  await call("ada", `/notes/${id}/access/people`, "POST", { userIds: [ids.ben] })
  for (const account of ["ada", "ben"] as const) expect((await call(account, `/shared/${token}`)).status, account).toBe(200)
  for (const account of ["cy", "dee", "admin", "outsider"] as const) {
    const response = await call(account, `/shared/${token}`)
    expect(response.status, account).toBe(404)
    // The same answer as for a link that never existed.
    expect(await response.text()).toBe(await (await call(account, `/shared/${"a".repeat(43)}`)).text())
  }
  expect((await call(null, `/shared/${token}`)).status).toBe(401)
})

test("\"Anyone at the organization with the link\": every member who holds the link reads it, and still nobody outside", async () => {
  const { call, note, link } = fixture()
  const id = await note("ada", "Planning", { body: "ADA_NOTES" })
  const token = await link("ada", id)
  expect((await (await call("ada", `/notes/${id}/access`, "PATCH", { level: "organization" })).json()).level).toBe("organization")
  for (const account of ["ada", "ben", "cy", "dee", "admin"] as const) expect((await call(account, `/shared/${token}`)).status, account).toBe(200)
  expect((await call("outsider", `/shared/${token}`)).status).toBe(404)
  expect((await call(null, `/shared/${token}`)).status).toBe(401)
  // The level is about links. It lists the note for nobody and opens it by its id for nobody.
  expect((await (await call("cy", "/shared-with-me")).json()).notes).toEqual([])
  expect((await call("cy", `/shared-with-me/${id}`)).status).toBe(404)
  expect((await call("cy", `/notes/${id}`)).status).toBe(404)
  // There is no wider level to ask for.
  for (const level of ["public", "anyone", "link", ""]) expect((await call("ada", `/notes/${id}/access`, "PATCH", { level })).status, level).toBe(400)
  expect((await (await call("ada", `/notes/${id}/access`)).json()).level).toBe("organization")
})

test("removing a person, tightening the level, or leaving the workspace ends access with the next request, for a link already in hand too", async () => {
  const { call, note, link, members } = fixture()
  const id = await note("ada", "Planning", { body: "ADA_NOTES" })
  const token = await link("ada", id)
  await call("ada", `/notes/${id}/access/people`, "POST", { userIds: [ids.ben, ids.dee] })
  await call("ada", `/notes/${id}/access`, "PATCH", { level: "organization" })
  for (const account of ["ben", "cy", "dee"] as const) expect((await call(account, `/shared/${token}`)).status).toBe(200)
  // Tightening: Cy only ever had the link.
  await call("ada", `/notes/${id}/access`, "PATCH", { level: "invited" })
  expect((await call("cy", `/shared/${token}`)).status).toBe(404)
  expect((await call("ben", `/shared/${token}`)).status).toBe(200)
  // Removing Ben.
  const after = await (await call("ada", `/notes/${id}/access/people/${ids.ben}`, "DELETE")).json()
  expect(after.people.map((row: { userId: string }) => row.userId)).toEqual([ids.dee])
  expect((await call("ben", `/shared/${token}`)).status).toBe(404)
  expect((await call("ben", `/shared-with-me/${id}`)).status).toBe(404)
  expect((await (await call("ben", "/shared-with-me")).json()).notes).toEqual([])
  // Removing someone who is not there changes nothing and is not an error.
  expect((await call("ada", `/notes/${id}/access/people/${ids.ben}`, "DELETE")).status).toBe(200)
  // Dee leaves the workspace: her row gives her nothing, and the owner no longer sees her listed.
  expect((await call("dee", `/shared-with-me/${id}`)).status).toBe(200)
  members.splice(members.findIndex(member => member.userId === ids.dee), 1)
  expect((await call("dee", `/shared-with-me/${id}`)).status).toBe(404)
  expect((await call("dee", `/shared/${token}`)).status).toBe(404)
  expect((await (await call("dee", "/shared-with-me")).json()).notes).toEqual([])
  expect((await (await call("ada", `/notes/${id}/access`)).json()).people).toEqual([])
})

test("a note put in Trash is reachable by nobody but its owner until it is taken out; a deleted one is gone", async () => {
  const { call, note, link } = fixture()
  const id = await note("ada", "Planning", { body: "ADA_NOTES" })
  const token = await link("ada", id)
  await call("ada", `/notes/${id}/access/people`, "POST", { userIds: [ids.ben] })
  await call("ada", `/notes/${id}/access`, "PATCH", { level: "organization" })
  expect((await (await call("ada", `/notes/${id}/access`, "PATCH", { suspended: true })).json())).toMatchObject({ suspended: true, level: "organization" })
  for (const account of ["ben", "cy"] as const) expect((await call(account, `/shared/${token}`)).status, account).toBe(404)
  expect((await call("ben", `/shared-with-me/${id}`)).status).toBe(404)
  expect((await (await call("ben", "/shared-with-me")).json()).notes).toEqual([])
  expect((await call("ada", `/shared/${token}`)).status).toBe(200)
  // Restored: everything is as it was.
  await call("ada", `/notes/${id}/access`, "PATCH", { suspended: false })
  expect((await call("ben", `/shared-with-me/${id}`)).status).toBe(200)
  expect((await call("cy", `/shared/${token}`)).status).toBe(200)
  expect((await call("ada", `/notes/${id}`, "DELETE")).status).toBe(200)
  expect((await call("ben", `/shared-with-me/${id}`)).status).toBe(404)
  expect((await call("ben", `/shared/${token}`)).status).toBe(404)
  expect((await (await call("ben", "/shared-with-me")).json()).notes).toEqual([])
  expect((await call("ada", `/notes/${id}/access`)).status).toBe(404)
})

test("candidates are members only: attendees first when nothing is typed, then whoever matches the name or address", async () => {
  const { call, note, attendees } = fixture()
  const id = await note("ada", "Planning")
  attendees.set(id, ["dee@example.test", "guest@elsewhere.test", "outsider@elsewhere.test"])
  const names = async (query = "") => (await (await call("ada", `/notes/${id}/access/candidates${query}`)).json()).people.map((row: { name: string; attendee: boolean }) => `${row.name}${row.attendee ? "*" : ""}`)
  // Dee was at the meeting; the owner is never offered to herself; the two outsiders at the meeting are not there.
  expect(await names()).toEqual(["dee*", "ben", "cy", "admin"])
  expect(await names("?q=")).toEqual(["dee*", "ben", "cy", "admin"])
  expect(await names("?q=BE")).toEqual(["ben"])
  expect(await names("?q=cy%40example")).toEqual(["cy"])
  expect(await names("?q=example.test")).toEqual(["dee*", "ben", "cy", "admin"])
  for (const query of ["outsider", "elsewhere", "guest", "outsider%40elsewhere.test", "zzz"]) expect(await names(`?q=${query}`), query).toEqual([])
  // Someone who already has access is not offered again.
  await call("ada", `/notes/${id}/access/people`, "POST", { userIds: [ids.dee] })
  expect(await names()).toEqual(["ben", "cy", "admin"])
  expect((await (await call("ada", `/notes/${id}/access`)).json()).attendees).toEqual([{ ...person("dee"), hasAccess: true }])
  expect((await call("ada", `/notes/${id}/access/candidates?q=${"x".repeat(201)}`)).status).toBe(400)
  expect((await call(null, `/notes/${id}/access/candidates?q=ben`)).status).toBe(401)
})

test("a note in the team space is read by link by every member, whatever its own level: either grant is enough", async () => {
  const { call, note, link, teamSpace } = fixture()
  const id = await note("ada", "Planning")
  const token = await link("ada", id)
  expect((await call("cy", `/shared/${token}`)).status).toBe(404)
  teamSpace.add(id)
  expect((await call("cy", `/shared/${token}`)).status).toBe(200)
  expect((await call("outsider", `/shared/${token}`)).status).toBe(404)
  // Being in the space does not make it "shared with" anyone by name.
  expect((await call("cy", `/shared-with-me/${id}`)).status).toBe(404)
  teamSpace.delete(id)
  expect((await call("cy", `/shared/${token}`)).status).toBe(404)
})

test("without the sharing service a link is its owner's alone", async () => {
  const repository = makeMemoryRepository()
  const api = makeApiHandler(repository, "note-access-bootstrap-test-at-least-32-characters", { principalResolver: async headers => {
    const account = (headers.get("authorization") ?? "").replace("Bearer ", "") as Account
    return account in ids ? { id: ids[account], name: account, email: email(account), role: "user", source: "session", expiresAt: null, createdAt: "2026-01-01T00:00:00Z" } : null
  } })
  close.push(api.dispose)
  const call = (account: Account, path: string, method = "GET", body?: unknown) => api.handler(new Request(`http://localhost/api/v1${path}`, { method, headers: { Authorization: `Bearer ${account}`, ...(body === undefined ? {} : { "Content-Type": "application/json" }) }, ...(body === undefined ? {} : { body: JSON.stringify(body) }) }))
  const id = (await (await call("ada", "/notes", "POST", { title: "Planning" })).json()).id
  const token = (await (await call("ada", `/notes/${id}/shares`, "POST", { includeBody: true, includeEnhanced: true })).json()).token
  expect((await call("ada", `/shared/${token}`)).status).toBe(200)
  expect((await call("ben", `/shared/${token}`)).status).toBe(404)
  expect((await call("ada", `/notes/${id}/access`)).status).toBe(404)
  expect((await (await call("ben", "/shared-with-me")).json()).notes).toEqual([])
})

test("the workspace's domain is the one sign-in is limited to, else the one every member shares, never a public mail service", () => {
  expect(organizationDomain("example.test", [{ email: "a@other.test" }])).toBe("example.test")
  expect(organizationDomain(null, [{ email: "a@Example.test" }, { email: "b@example.test" }])).toBe("example.test")
  expect(organizationDomain(null, [{ email: "a@example.test" }, { email: "b@other.test" }])).toBeNull()
  expect(organizationDomain(null, [{ email: "a@gmail.com" }, { email: "b@gmail.com" }])).toBeNull()
  expect(organizationDomain(null, [])).toBeNull()
  expect(matchesMember({ name: "Ada Lovelace", email: "ada@example.test" }, "love ada")).toBe(true)
  expect(matchesMember({ name: "Ada Lovelace", email: "ada@example.test" }, "ada ben")).toBe(false)
})
