// Exercises the actual HTTP server and removes only its own fictional fixture.
// Bun loads the local .env; credentials and capability URLs are never printed.
const base = new URL(process.env.DEPLOYMENT_URL ?? "http://127.0.0.1:3000")
const token = process.env.ADMIN_API_TOKEN
if (!token) throw new Error("Configure ADMIN_API_TOKEN before checking a deployment")
if (base.protocol !== "https:" && !(base.protocol === "http:" && ["127.0.0.1", "localhost", "[::1]"].includes(base.hostname))) {
  throw new Error("Use HTTPS for remote deployment verification")
}
function assert(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message)
}
async function call(path: string, options: { method?: string; body?: unknown; owner?: boolean; status?: number } = {}) {
  const headers = new Headers({ Accept: "application/json" })
  if (options.owner) headers.set("Authorization", `Bearer ${token}`)
  if (options.body !== undefined) headers.set("Content-Type", "application/json")
  const response = await fetch(new URL(path, base), {
    method: options.method ?? "GET", headers,
    body: options.body === undefined ? undefined : JSON.stringify(options.body),
    redirect: "error",
  })
  assert(response.status === (options.status ?? 200), `Unexpected HTTP status for ${options.method ?? "GET"} ${path.includes("/shared/") ? "/api/v1/shared/[redacted]" : path}`)
  return response
}
let noteId: string | undefined
try {
  const health = await call("/api/v1/health")
  assert((await health.json()).status === "ok", "Health response invalid")
  await call("/api/v1/ready")
  await call("/api/v1/notes", { status: 401 })
  await call("/api/v1/notes", { method: "POST", owner: true, body: { title: "Invalid fixture", date: "2026-02-30" }, status: 400 })
  const created = await call("/api/v1/notes", {
    method: "POST", owner: true, body: {
      title: "Deployment verification fixture", body: "Private draft excluded from this share", enhanced: "# Shared fixture\n\n- A fictional verification note.",
      transcriptSegments: [{ id: crypto.randomUUID(), channel: "mic", speakerId: "self", speakerLabel: null, text: "Private transcript excluded from shares", isFinal: true, startMs: 0, endMs: 1000 }],
    },
  })
  const note = await created.json()
  noteId = note.id
  assert(typeof noteId === "string", "Created note has no ID")
  const share = await (await call(`/api/v1/notes/${noteId}/shares`, {
    method: "POST", owner: true, body: { includeBody: false, includeEnhanced: true },
  })).json()
  assert(typeof share.token === "string" && share.token.length === 43, "Capability token invalid")
  const sharedPath = `/api/v1/shared/${encodeURIComponent(share.token)}`
  // Reading a shared link needs an account on the server: anonymous is refused, the signed-in caller reads it.
  await call(sharedPath, { status: 401 })
  const sharedResponse = await call(sharedPath, { owner: true })
  assert(sharedResponse.headers.get("cache-control") === "no-store", "The API must not cache notes")
  const shared = await sharedResponse.json()
  assert(shared.body === "" && shared.enhanced === note.enhanced, "Share selection was not applied")
  assert(!("transcriptSegments" in shared) && !("noteId" in shared), "Share exposed private fields")
  await call(`/api/v1/notes/${noteId}`, { method: "PATCH", owner: true, body: { title: "Changed private title", enhanced: "Changed private draft" } })
  const snapshot = await (await call(sharedPath, { owner: true })).json()
  assert(snapshot.title === note.title && snapshot.enhanced === note.enhanced, "Private edits changed a published snapshot")
  const page = await fetch(new URL(share.path, base), { redirect: "error" })
  assert(page.ok && page.headers.get("content-type")?.includes("text/html"), "Direct share route did not serve the reader")
  assert(page.headers.get("referrer-policy") === "no-referrer", "Reader must not leak capability URLs")
  const shell = await page.text()
  const asset = shell.match(/(?:src|href)="(\/assets\/[^\"]+)"/)
  assert(asset?.[1], "Built reader asset missing")
  const resource = await fetch(new URL(asset[1], base))
  assert(resource.ok, "Built reader asset unavailable")
  await call("/api/no-such-endpoint", { status: 404 })
  await call("/api/v1/shared/invalid", { status: 401 })
  await call("/api/v1/shared/invalid", { owner: true, status: 404 })
  await call(`/api/v1/shares/${share.share.id}`, { method: "DELETE", owner: true })
  await call(sharedPath, { owner: true, status: 404 })
  await call(sharedPath, { status: 401 })
  console.log("Deployment checks passed: health, owner auth, validated CRUD, private share projection, immutable snapshot, reader/assets, revocation, API routing")
} finally {
  if (noteId) await call(`/api/v1/notes/${noteId}`, { method: "DELETE", owner: true })
}
