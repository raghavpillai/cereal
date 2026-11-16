import { afterEach, expect, test } from "bun:test"
import { Effect } from "effect"
import type { PrincipalData } from "@meeting-notes/contracts/api"
import { makeApiHandler } from "../src/api"
import { makeMemoryRepository, BOOTSTRAP_OWNER_ID } from "../src/repository"
import { makeProcessingService } from "../src/processing"

const close: Array<() => Promise<void>> = []
afterEach(async () => { for (const dispose of close.splice(0)) await dispose() })
const ids = { owner: BOOTSTRAP_OWNER_ID, admin: "20000000-0000-4000-8000-000000000002", alice: "20000000-0000-4000-8000-000000000003", bob: "20000000-0000-4000-8000-000000000004" }
type Account = keyof typeof ids
function fixture() {
  const repository = makeMemoryRepository()
  const contexts: string[] = []
  const processing = makeProcessingService(repository, { providers: {
    generate: () => Effect.succeed("generated"), chat: () => Effect.succeed("answer"),
    async *chatStream(note) { contexts.push(note.body); yield "Account-scoped answer" },
  } })
  const api = makeApiHandler(repository, "multiuser-bootstrap-test-at-least-32-characters", {
    processing,
    principalResolver: async headers => {
      const account = (headers.get("authorization") ?? "").replace("Bearer ", "") as Account
      if (!(account in ids)) return null
      return { id: ids[account], name: account, email: `${account}@example.test`, role: account === "owner" || account === "admin" ? account : "user", source: "session", expiresAt: "2099-01-01T00:00:00Z", createdAt: "2026-01-01T00:00:00Z" } satisfies PrincipalData
    },
  })
  close.push(api.dispose)
  const call = (account: Account | null, path: string, method = "GET", body?: unknown) => api.handler(new Request(`http://localhost/api/v1${path}`, {
    method, headers: { ...(account ? { Authorization: `Bearer ${account}` } : {}), ...(body === undefined ? {} : { "Content-Type": "application/json" }) },
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
  }))
  return { call, contexts }
}

test("HTTP isolation covers list, detail, recording, transcript, chat, evidence, shares and deletion", async () => {
  const { call, contexts } = fixture()
  const note = await (await call("alice", "/notes", "POST", { title: "Alice private", body: "ALICE_PRIVATE_TOKEN" })).json()
  const bobNote = await (await call("bob", "/notes", "POST", { title: "Bob note", body: "BOB_VISIBLE_TOKEN" })).json()
  expect((await (await call("alice", "/notes")).json()).map((n: {id: string}) => n.id)).toEqual([note.id])
  expect((await (await call("bob", "/notes")).json()).map((n: {id: string}) => n.id)).toEqual([bobNote.id])
  // Owner/admin configuration powers do not grant access to another person's notes.
  for (const account of ["owner", "admin", "bob"] as const) {
    const attempts: Array<[string, string, unknown?]> = [
      [`/notes/${note.id}`, "GET"], [`/notes/${note.id}`, "PATCH", { body: "overwritten" }],
      [`/notes/${note.id}/recording`, "POST", { command: "start" }],
      [`/notes/${note.id}/audio/chunks`, "GET"],
      [`/notes/${note.id}/audio/chunks/${crypto.randomUUID()}`, "PUT", { channel: "system", sequence: 0, startMs: 0, audioBase64: "AAAA" }],
      [`/notes/${note.id}/audio/chunks/${crypto.randomUUID()}/retry`, "POST"],
      [`/notes/${note.id}/generate`, "POST", { expectedTranscriptRevision: 0 }],
      [`/notes/${note.id}/chat`, "POST", { message: "Read the private note", history: [] }],
      [`/notes/${note.id}/chat/stream`, "POST", { message: "Read the private note", history: [] }],
      [`/notes/${note.id}/evidence`, "POST", { paragraph: "Private", expectedTranscriptRevision: 0 }],
      [`/notes/${note.id}/evidence/stream`, "POST", { paragraph: "Private", expectedTranscriptRevision: 0 }],
      [`/notes/${note.id}/shares`, "GET"],
      [`/notes/${note.id}/shares`, "POST", { includeBody: true, includeEnhanced: false }],
      [`/notes/${note.id}`, "DELETE"],
    ]
    for (const [path, method, body] of attempts) {
      const response = await call(account, path, method, body)
      expect(response.status, `${account}: ${method} ${path}`).toBe(404)
      expect(await response.text()).not.toContain("ALICE_PRIVATE_TOKEN")
    }
  }
  const stream = await call("bob", "/chat/stream", "POST", { message: "Show notes", history: [] })
  expect(stream.status).toBe(200)
  await stream.text()
  expect(contexts).toHaveLength(1)
  expect(contexts[0]).toContain("BOB_VISIBLE_TOKEN")
  expect(contexts[0]).not.toContain("ALICE_PRIVATE_TOKEN")
  expect((await (await call("alice", `/notes/${note.id}`)).json()).body).toBe("ALICE_PRIVATE_TOKEN")
})

test("a shared link is read by any signed-in account that holds it, never anonymously; another account cannot list or revoke it", async () => {
  const { call } = fixture()
  const note = await (await call("alice", "/notes", "POST", { title: "Shared title", body: "EXPLICIT_SNAPSHOT" })).json()
  const response = await call("alice", `/notes/${note.id}/shares`, "POST", { includeBody: true, includeEnhanced: false })
  expect(response.status).toBe(200)
  const share = await response.json()
  expect((await call("bob", `/shares/${share.share.id}`, "DELETE")).status).toBe(404)
  expect((await call(null, `/shared/${share.token}`)).status).toBe(401)
  // Bob holds the link. He reads the snapshot, though the note itself is not his to open.
  expect((await call("bob", `/notes/${note.id}`)).status).toBe(404)
  for (const reader of ["bob", "alice", "admin"] as const) {
    const read = await call(reader, `/shared/${share.token}`)
    expect(read.status).toBe(200)
    expect((await read.json()).body).toBe("EXPLICIT_SNAPSHOT")
  }
  expect((await call("alice", `/shares/${share.share.id}`, "DELETE")).status).toBe(200)
  expect((await call("bob", `/shared/${share.token}`)).status).toBe(404)
  expect((await call(null, `/shared/${share.token}`)).status).toBe(401)
})

test("configuration gates distinguish anonymous, user, admin and owner on the server", async () => {
  const { call } = fixture()
  for (const account of [null, "alice", "admin", "owner"] as const) {
    const expected = account === null ? 401 : account === "alice" ? 403 : 200
    expect((await call(account, "/admin/settings")).status).toBe(expected)
    expect((await call(account, "/admin/settings", "PATCH", { instanceName: "Configuration permission test" })).status).toBe(expected)
  }
  // Denial occurs before service invocation, even if no account DB is configured.
  expect((await call("admin", `/admin/users/${ids.bob}/role`, "PATCH", { role: "admin" })).status).toBe(403)
  expect((await call("alice", "/calendars", "POST", { name: "Forbidden", url: "https://calendar.example.test/feed.ics" })).status).toBe(403)
  expect((await call("alice", "/session")).status).toBe(200)
  expect((await call("alice", "/calendar/preferences")).status).toBe(200)
})
