import { afterEach, describe, expect, test } from "bun:test"
import { Effect } from "effect"
import { createSharedNotesClient, SharedNoteClientError } from "@meeting-notes/contracts/client"
import { makeApiHandler } from "../src/api"
import { makeMemoryRepository, RepositoryError } from "../src/repository"

const token = "test-owner-token-with-at-least-32-characters"
const disposals: Array<() => Promise<void>> = []
afterEach(async () => { for (const dispose of disposals.splice(0)) await dispose() })
function fixture(clock?: () => Date) {
  const repository = makeMemoryRepository([], clock)
  const api = makeApiHandler(repository, token, { clock })
  disposals.push(api.dispose)
  const request = (path: string, method = "GET", body?: unknown, owner = true) => api.handler(new Request(`http://localhost${path}`, {
    method,
    headers: { ...(owner ? { Authorization: `Bearer ${token}` } : {}), ...(body === undefined ? {} : { "Content-Type": "application/json" }) },
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
  }))
  return { request, repository }
}
const segment = { id: "30b6f071-202b-4f6c-aafe-d4b7bfb09c53", channel: "mic", speakerId: "self", speakerLabel: "Me", text: "Private spoken words", isFinal: true, startMs: 0, endMs: 1000 }

async function create(request: ReturnType<typeof fixture>["request"], input: Record<string, unknown> = {}) {
  const response = await request("/api/v1/notes", "POST", { title: "Private meeting", body: "Private body", enhanced: "Public summary", transcriptSegments: [segment], ...input })
  expect(response.status).toBe(200)
  return await response.json() as { id: string; transcriptRevision: number; generatedRevision: number | null; generationState: string }
}

describe("typed owner API and opaque shared snapshots", () => {
  test("health is anonymous; every owner operation requires a bearer credential", async () => {
    const { request } = fixture()
    expect((await request("/api/v1/health", "GET", undefined, false)).status).toBe(200)
    for (const [path, method, body] of [
      ["/api/v1/notes", "GET", undefined], ["/api/v1/notes", "POST", { title: "X" }],
      ["/api/v1/notes/30b6f071-202b-4f6c-aafe-d4b7bfb09c53", "GET", undefined],
    ] as const) expect((await request(path, method, body, false)).status).toBe(401)
  })

  test("rejects invalid payloads and keeps generation separate from transcript revision", async () => {
    const { request } = fixture()
    expect((await request("/api/v1/notes", "POST", { title: "x".repeat(501) })).status).toBe(400)
    expect((await request("/api/v1/notes", "POST", { title: "X", date: "2026-02-30" })).status).toBe(400)
    expect((await request("/api/v1/notes", "POST", { title: "X", transcriptSegments: [{ ...segment, endMs: -1 }] })).status).toBe(400)
    const note = await create(request)
    expect(note.transcriptRevision).toBe(1)
    expect(note.generatedRevision).toBeNull()
    expect(note.generationState).toBe("idle")
    const unchanged = await (await request(`/api/v1/notes/${note.id}`, "PATCH", { transcriptSegments: [segment] })).json()
    expect(unchanged.transcriptRevision).toBe(1)
    const changed = await (await request(`/api/v1/notes/${note.id}`, "PATCH", { transcriptSegments: [{ ...segment, text: "Additional transcript" }] })).json()
    expect(changed.transcriptRevision).toBe(2)
    expect(changed.generationState).toBe("idle")
    expect(changed.generatedRevision).toBeNull()
  })

  test("shared response is an explicit immutable projection with no transcript or other notes", async () => {
    const { request } = fixture()
    const note = await create(request)
    await create(request, { title: "Other private meeting", body: "Other private note content" })
    const created = await request(`/api/v1/notes/${note.id}/shares`, "POST", { includeBody: false, includeEnhanced: true, expiresAt: null })
    expect(created.status).toBe(200)
    const share = await created.json()
    expect(share.token).toMatch(/^[A-Za-z0-9_-]{43}$/)
    expect(share.path).toBe(`/s/${share.token}`)
    // Reading a share needs an account: without one the answer is 401, whether or not the token exists.
    const anonymous = await request(`/api/v1/shared/${share.token}`, "GET", undefined, false)
    expect(anonymous.status).toBe(401)
    expect((await anonymous.json()).error).toBe("unauthorized")
    expect(JSON.stringify(await (await request(`/api/v1/shared/${"a".repeat(43)}`, "GET", undefined, false)).json())).toBe(JSON.stringify({ error: "unauthorized", message: "Authentication required" }))
    expect((await request("/api/v1/shared/not-a-token", "GET", undefined, false)).status).toBe(401)
    const response = await request(`/api/v1/shared/${share.token}`)
    expect(response.status).toBe(200)
    expect(response.headers.get("Cache-Control")).toBe("no-store")
    expect(response.headers.get("Referrer-Policy")).toBe("no-referrer")
    const projection = await response.json()
    expect(Object.keys(projection).sort()).toEqual(["shareId", "title", "body", "enhanced", "date", "duration", "sharedAt", "expiresAt"].sort())
    expect(projection.body).toBe("")
    expect(projection.enhanced).toBe("Public summary")
    expect(JSON.stringify(projection)).not.toContain("Private spoken words")
    expect(JSON.stringify(projection)).not.toContain("Other private")
    await request(`/api/v1/notes/${note.id}`, "PATCH", { enhanced: "Later private edits" })
    expect((await (await request(`/api/v1/shared/${share.token}`)).json()).enhanced).toBe("Public summary")
    const metadata = await (await request(`/api/v1/notes/${note.id}/shares`)).json()
    expect(JSON.stringify(metadata)).not.toContain(share.token)
    expect(JSON.stringify(metadata)).not.toContain("tokenHash")
  })

  test("revoked, expired and unknown tokens all return the same not-found response", async () => {
    let now = new Date("2026-11-12T12:00:00.000Z")
    const { request } = fixture(() => now)
    const note = await create(request)
    const share = await (await request(`/api/v1/notes/${note.id}/shares`, "POST", { includeBody: true, includeEnhanced: false, expiresAt: "2026-11-12T13:00:00.000Z" })).json()
    now = new Date("2026-11-12T14:00:00.000Z")
    const expired = await request(`/api/v1/shared/${share.token}`)
    const unknown = await request(`/api/v1/shared/${"a".repeat(43)}`)
    expect(expired.status).toBe(404)
    expect(await expired.json()).toEqual(await unknown.json())
    now = new Date("2026-11-12T12:00:00.000Z")
    expect((await request(`/api/v1/shares/${share.share.id}`, "DELETE")).status).toBe(200)
    const revoked = await request(`/api/v1/shared/${share.token}`)
    expect(revoked.status).toBe(404)
    expect(await revoked.json()).toEqual({ error: "not_found", message: "Shared note not found" })
    expect((await request("/api/v1/shared/not-a-token")).status).toBe(404)
    // Someone without an account cannot tell a revoked, expired or unknown link from a live one.
    for (const path of [`/api/v1/shared/${share.token}`, `/api/v1/shared/${"a".repeat(43)}`, "/api/v1/shared/not-a-token"]) expect((await request(path, "GET", undefined, false)).status).toBe(401)
  })

  test("deleting a note removes its shares and does not affect another note", async () => {
    const { request } = fixture()
    const note = await create(request)
    const other = await create(request, { title: "Keep me" })
    const share = await (await request(`/api/v1/notes/${note.id}/shares`, "POST", { includeBody: false, includeEnhanced: true, expiresAt: null })).json()
    expect((await request(`/api/v1/notes/${note.id}`, "DELETE")).status).toBe(200)
    expect((await request(`/api/v1/notes/${note.id}`)).status).toBe(404)
    expect((await request(`/api/v1/notes/${other.id}`)).status).toBe(200)
    expect((await request(`/api/v1/shared/${share.token}`)).status).toBe(404)
  })

  test("typed shared-note client sends the browser session to its own origin only and reports a missing session as 401", async () => {
    const { request, repository } = fixture()
    const note = await create(request)
    const share = await (await request(`/api/v1/notes/${note.id}/shares`, "POST", { includeBody: false, includeEnhanced: true })).json()
    const api = makeApiHandler(repository, token)
    disposals.push(api.dispose)
    let observed: RequestInit | undefined
    // The browser attaches the session cookie; here the stand-in for it is the operator's credential.
    let signedIn = true
    const fetcher = (async (input: RequestInfo | URL, init?: RequestInit) => {
      observed = init
      const sent = new Request(input, init)
      if (signedIn) sent.headers.set("Authorization", `Bearer ${token}`)
      return api.handler(sent)
    }) as typeof fetch
    const client = createSharedNotesClient("http://localhost", fetcher)
    expect((await client.getSharedNote(share.token)).enhanced).toBe("Public summary")
    expect(observed?.credentials).toBe("same-origin")
    expect(observed?.redirect).toBe("error")
    expect(observed?.cache).toBe("no-store")
    expect(observed?.referrerPolicy).toBe("no-referrer")
    expect(new Headers(observed?.headers).has("Authorization")).toBe(false)
    try {
      await client.getSharedNote("a".repeat(43))
      throw new Error("Expected not found")
    } catch (error) {
      expect(error).toBeInstanceOf(SharedNoteClientError)
      expect((error as SharedNoteClientError).status).toBe(404)
    }
    signedIn = false
    const refused = await client.getSharedNote(share.token).catch(error => error)
    expect(refused).toBeInstanceOf(SharedNoteClientError)
    expect((refused as SharedNoteClientError).status).toBe(401)
  })

  test("a path under /api that is not an endpoint answers 404 in JSON, signed in or not", async () => {
    const { request } = fixture()
    for (const path of ["/api", "/api/", "/api/zzz", "/api/v1", "/api/v1/nope", "/api/v2/notes", "/api/mcp/extra", "/api/v1/notes/not-a-uuid/extra/deeper"]) for (const authorized of [true, false]) {
      const response = await request(path, "GET", undefined, authorized)
      expect(response.status).toBe(404)
      expect(response.headers.get("Content-Type")).toContain("application/json")
      expect((await response.json()).error).toBe("not_found")
      expect(response.headers.get("Cache-Control")).toBe("no-store")
    }
  })

  test("repository failures map to a bounded service error", async () => {
    const memory = makeMemoryRepository()
    const repository = { ...memory, forUser: (id: string) => ({ ...memory.forUser(id), listNotes: () => Effect.fail(new RepositoryError("database-password-private")) }) }
    const api = makeApiHandler(repository, token)
    disposals.push(api.dispose)
    const response = await api.handler(new Request("http://localhost/api/v1/notes", { headers: { Authorization: `Bearer ${token}` } }))
    expect(response.status).toBe(503)
    const body = await response.text()
    expect(body).toContain("temporarily unavailable")
    expect(body).not.toContain("password")
    expect(body).not.toContain("stack")
  })
})
