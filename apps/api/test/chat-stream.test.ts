import { afterEach, expect, test } from "bun:test"
import { Effect } from "effect"
import { makeApiHandler } from "../src/api"
import { makeProcessingService } from "../src/processing"
import { makeMemoryRepository } from "../src/repository"
import { makeProviderServices, type ProviderServices } from "../src/providers"

const cleanup: Array<() => Promise<void>> = []
afterEach(async () => { for (const close of cleanup.splice(0)) await close() })
const owner = "chat-stream-owner-fixture-at-least-32-characters"
async function fixture(extra: Partial<ProviderServices> = {}) {
  const repo = makeMemoryRepository()
  const segment = { id: crypto.randomUUID(), channel: "system" as const, speakerId: "speaker-0", speakerLabel: "Speaker 0", text: "Cedar launches Monday after accessibility review.", startMs: 1000, endMs: 3000, isFinal: true }
  const note = await Effect.runPromise(repo.createNote({ title: "Synthetic chat fixture", transcriptSegments: [segment] }))
  const providers: ProviderServices = { generate: () => Effect.succeed("Notes"), chat: () => Effect.succeed("Monday"), ...extra }
  const api = makeApiHandler(repo, owner, { processing: makeProcessingService(repo, { providers }) })
  cleanup.push(api.dispose)
  const call = (route: string, payload: unknown, authenticated = true) => api.handler(new Request(`http://localhost/api/v1/notes/${note.id}/${route}`, { method: "POST", headers: { "Content-Type": "application/json", ...(authenticated ? { Authorization: `Bearer ${owner}` } : {}) }, body: JSON.stringify(payload) }))
  return { repo, note, segment, call }
}
test("stream and evidence retain owner authentication and validate input", async () => {
  const f = await fixture()
  expect((await f.call("chat/stream", { message: "Hello", history: [] }, false)).status).toBe(401)
  expect((await f.call("evidence", { paragraph: "Monday", expectedTranscriptRevision: 1 }, false)).status).toBe(401)
  expect((await f.call("chat/stream", { message: "", history: [] })).status).toBe(400)
  expect((await f.call("evidence", { paragraph: "Monday", expectedTranscriptRevision: -1 })).status).toBe(400)
  const empty = await Effect.runPromise(f.repo.createNote({ title: "No transcript fixture" }))
  const processing = makeProcessingService(f.repo, {})
  cleanup.push(processing.dispose)
  const explanation = await Effect.runPromise(processing.evidence(empty.id, { paragraph: "Typed notes", expectedTranscriptRevision: 0 }))
  expect(explanation.quotes).toEqual([])
  expect(explanation.summary).toContain("No supporting transcript")
})
test("HTTP sends actual partial text before completion and disconnect aborts upstream", async () => {
  let canceled = false
  const f = await fixture({ async *chatStream(_note, _input, signal) {
    yield "First Zoë 👋"
    await new Promise<void>(resolve => signal.addEventListener("abort", () => { canceled = true; resolve() }, { once: true }))
  } })
  const response = await f.call("chat/stream", { message: "Question", history: [] })
  expect(response.status).toBe(200)
  expect(response.headers.get("content-type")).toContain("text/event-stream")
  expect(response.headers.get("cache-control")).toBe("no-store")
  const reader = response.body!.getReader()
  const first = await reader.read()
  const text = new TextDecoder().decode(first.value)
  expect(text).toContain('"type":"delta"')
  expect(text).toContain("First Zoë 👋")
  expect(text).not.toContain('"type":"done"')
  await reader.cancel()
  for (let i = 0; !canceled && i < 20; i++) await Bun.sleep(5)
  expect(canceled).toBe(true)
})
test("completed HTTP stream emits deltas and explicit done; failure stays bounded", async () => {
  const f = await fixture({ async *chatStream() { yield "One "; yield "two" } })
  const text = await (await f.call("chat/stream", { message: "Question", history: [] })).text()
  // A space at the end of a piece waits for the next one: it may stand before a source marker, which a chat about one note never shows.
  expect(text).toContain('"text":"One"')
  expect(text).toContain('"text":" two"')
  expect(text).toContain('"type":"done"')
  const failing = await fixture({ async *chatStream() { yield "Partial"; throw new Error("secret upstream key fixture") } })
  const failed = await (await failing.call("chat/stream", { message: "Question", history: [] })).text()
  expect(failed).toContain('"type":"error"')
  expect(failed).not.toContain("secret upstream")
  expect(failed).not.toContain('"type":"done"')
})
test("OpenRouter parser preserves split UTF-8, comments, usage frames, and done", async () => {
  let request: Request | undefined
  const bytes = new TextEncoder().encode(': keepalive\r\n\r\ndata: {"choices":[{"delta":{"content":"Zoë 👋"}}]}\r\n\r\ndata: {"choices":[],"usage":{}}\n\ndata: [DONE]\n\n')
  const providers = makeProviderServices({ openRouterKey: "fixture-router-key", fetch: (async (url, init) => {
    request = new Request(url, init)
    let i = 0
    return new Response(new ReadableStream({ pull(c) { if (i < bytes.length) c.enqueue(bytes.slice(i, ++i)); else c.close() } }))
  }) as typeof fetch })
  const f = await fixture()
  const values: string[] = []
  for await (const text of providers.chatStream!(f.note, { message: "Question", history: [] }, new AbortController().signal)) values.push(text)
  expect(values.join("")).toBe("Zoë 👋")
  expect((await request!.json()).stream).toBe(true)
  expect(request!.headers.get("Authorization")).toBe("Bearer fixture-router-key")
})
test("chat receives explicit elapsed units instead of interpreting m:ss as hours", async () => {
  const contexts: string[] = [];
  const providers = makeProviderServices({ openRouterKey: "fixture-router-key", fetch: (async (_url, init) => {
    const body = JSON.parse(String(init?.body));
    contexts.push(body.messages[1].content);
    return Response.json({ choices: [{ message: { content: "Duration verified" } }] });
  }) as typeof fetch });
  const f = await fixture();
  for (const duration of ["1:44", "1:02:03", "12 min"]) {
    await Effect.runPromise(providers.chat({ ...f.note, duration }, { message: "How long?", history: [] }));
  }
  expect(contexts[0]).toContain("104 seconds (elapsed 1:44; minutes:seconds)");
  expect(contexts[1]).toContain("3723 seconds (elapsed 1:02:03; hours:minutes:seconds)");
  expect(contexts[2]).toContain('"duration":"12 min"');
});
test("truncated upstream stream is an error rather than a completed response", async () => {
  const providers = makeProviderServices({ openRouterKey: "fixture-router-key", fetch: (async () => new Response('data: {"choices":[{"delta":{"content":"Partial"}}]}\n\n')) as unknown as typeof fetch })
  const f = await fixture()
  let failed = false
  try { for await (const _text of providers.chatStream!(f.note, { message: "Question", history: [] }, new AbortController().signal)) {} } catch { failed = true }
  expect(failed).toBe(true)
})
test("evidence returns exact source text and guards revision and unknown segment IDs", async () => {
  const f = await fixture({ evidence: note => Effect.succeed({ summary: "The date changed after review.", segmentIds: [note.transcriptSegments[0]!.id] }) })
  const response = await f.call("evidence", { paragraph: "Launch Monday", expectedTranscriptRevision: 1 })
  expect(response.status).toBe(200)
  const evidence = await response.json()
  expect(evidence.quotes[0]).toEqual({ segmentId: f.segment.id, text: f.segment.text, speakerLabel: "Speaker 0", startMs: 1000 })
  expect(evidence.transcriptRevision).toBe(1)
  expect((await f.call("evidence", { paragraph: "Monday", expectedTranscriptRevision: 0 })).status).toBe(400)
  const invalid = await fixture({ evidence: () => Effect.succeed({ summary: "Unsupported", segmentIds: [crypto.randomUUID()] }) })
  expect((await invalid.call("evidence", { paragraph: "Monday", expectedTranscriptRevision: 1 })).status).toBe(503)
})
test("an explanation completing after transcript changes is rejected", async () => {
  let finish!: (value: { summary: string; segmentIds: string[] }) => void
  const deferred = new Promise<{ summary: string; segmentIds: string[] }>(resolve => { finish = resolve })
  const f = await fixture({ evidence: () => Effect.promise(() => deferred) })
  const pending = f.call("evidence", { paragraph: "Monday", expectedTranscriptRevision: 1 })
  await Bun.sleep(5)
  await Effect.runPromise(f.repo.updateNote(f.note.id, { transcriptSegments: [{ ...f.segment, text: "Cedar now launches Tuesday." }] }))
  finish({ summary: "Monday", segmentIds: [f.segment.id] })
  expect((await pending).status).toBe(400)
})
test("workspace chat is owner-only and receives bounded real note context", async () => {
  let context = ""
  const f = await fixture({ async *chatStream(note) { context = note.body; yield "Cedar launches Monday." } })
  const repo = f.repo
  const api = makeApiHandler(repo, owner, { processing: makeProcessingService(repo, { providers: {
    generate: () => Effect.succeed(""), chat: () => Effect.succeed(""),
    async *chatStream(note) { context = note.body; yield "Cedar launches Monday." },
  } }) })
  cleanup.push(api.dispose)
  const make = (auth: boolean) => new Request("http://localhost/api/v1/chat/stream", { method: "POST", headers: { "Content-Type": "application/json", ...(auth ? { Authorization: `Bearer ${owner}` } : {}) }, body: JSON.stringify({ message: "When does Cedar launch?", history: [] }) })
  expect((await api.handler(make(false))).status).toBe(401)
  const text = await (await api.handler(make(true))).text()
  expect(text).toContain("Cedar launches Monday")
  expect(context).toContain(f.segment.text)
  expect(context).toContain(f.note.title)
  expect(context).not.toContain(owner)
})
test("cancelling a pending evidence request aborts the actual upstream fetch", async () => {
  let started!: () => void
  const ready = new Promise<void>(resolve => { started = resolve })
  let aborted = false
  const providers = makeProviderServices({ openRouterKey: "fixture-router-key", fetch: (async (_url, init) => {
    started()
    return await new Promise<Response>((_resolve, reject) => {
      init!.signal!.addEventListener("abort", () => { aborted = true; reject(new Error("Cancelled")) }, { once: true })
    })
  }) as typeof fetch })
  const f = await fixture()
  const cancel = new AbortController()
  const pending = Effect.runPromise(providers.evidence!(f.note, "Monday"), { signal: cancel.signal }).catch(() => undefined)
  await ready
  cancel.abort()
  await pending
  expect(aborted).toBe(true)
})
