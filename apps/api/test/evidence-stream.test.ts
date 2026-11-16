import { afterEach, expect, test } from "bun:test"
import { Effect } from "effect"
import { makeApiHandler } from "../src/api"
import { makeProcessingService } from "../src/processing"
import { makeMemoryRepository } from "../src/repository"
import { makeProviderServices, ProviderFailure, type ProviderServices } from "../src/providers"
import { EvidenceSummaryReader, readEvidence } from "../src/ai/evidence"

const cleanup: Array<() => Promise<void>> = []
afterEach(async () => { for (const close of cleanup.splice(0)) await close() })
const owner = "evidence-stream-owner-fixture-at-least-32-characters"

async function fixture(extra: Partial<ProviderServices> = {}) {
  const repo = makeMemoryRepository()
  const segments = [
    { id: crypto.randomUUID(), channel: "system" as const, speakerId: "speaker-0", speakerLabel: "Speaker 0", text: "Cedar launches Monday after accessibility review.", startMs: 1000, endMs: 3000, isFinal: true },
    { id: crypto.randomUUID(), channel: "mic" as const, speakerId: "speaker-1", speakerLabel: "Speaker 1", text: "Zoë will confirm the date 👋", startMs: 44_000, endMs: 46_000, isFinal: true },
  ]
  const note = await Effect.runPromise(repo.createNote({ title: "Synthetic evidence fixture", transcriptSegments: segments }))
  const providers: ProviderServices = { generate: () => Effect.succeed("Notes"), chat: () => Effect.succeed("Monday"), ...extra }
  const api = makeApiHandler(repo, owner, { processing: makeProcessingService(repo, { providers }) })
  cleanup.push(api.dispose)
  const call = (payload: unknown, options: { authenticated?: boolean; noteId?: string } = {}) => api.handler(new Request(`http://localhost/api/v1/notes/${options.noteId ?? note.id}/evidence/stream`, {
    method: "POST", headers: { "Content-Type": "application/json", ...(options.authenticated === false ? {} : { Authorization: `Bearer ${owner}` }) }, body: JSON.stringify(payload) }))
  return { repo, note, segments, call }
}
type Event = { type: "delta" | "done" | "error"; text: string; evidence?: { summary: string; quotes: Array<{ segmentId: string; text: string; speakerLabel: string | null; startMs: number }>; transcriptRevision: number } }
const events = (body: string): Event[] => body.split("\n\n").map(frame => frame.split("\n").filter(line => line.startsWith("data:")).map(line => line.slice(5).trimStart()).join("\n")).filter(Boolean).map(data => JSON.parse(data))
const request = { paragraph: "Cedar launches Monday", expectedTranscriptRevision: 1 }

test("the summary streams as deltas in order, then one done with exact quotations", async () => {
  const f = await fixture({ async *evidenceStream(note) {
    const answer = JSON.stringify({ summary: "Cedar \"launches\" Monday —\nZoë 👋 confirms.", segmentIds: [note.transcriptSegments[1]!.id, note.transcriptSegments[0]!.id, note.transcriptSegments[1]!.id] })
    // Split everywhere, including inside escapes and the emoji's surrogate pair.
    for (let i = 0; i < answer.length; i += 3) yield answer.slice(i, i + 3)
  } })
  const response = await f.call(request)
  expect(response.status).toBe(200)
  expect(response.headers.get("content-type")).toContain("text/event-stream")
  const list = events(await response.text())
  const deltas = list.filter(event => event.type === "delta")
  expect(deltas.length).toBeGreaterThan(5)
  expect(deltas.map(event => event.text).join("")).toBe("Cedar \"launches\" Monday —\nZoë 👋 confirms.")
  expect(list.at(-1)!.type).toBe("done")
  expect(list.filter(event => event.type !== "delta").length).toBe(1)
  const evidence = list.at(-1)!.evidence!
  expect(evidence.summary).toBe("Cedar \"launches\" Monday —\nZoë 👋 confirms.")
  // Duplicates are dropped; the model's order is kept.
  expect(evidence.quotes).toEqual([
    { segmentId: f.segments[1]!.id, text: f.segments[1]!.text, speakerLabel: "Speaker 1", startMs: 44_000 },
    { segmentId: f.segments[0]!.id, text: f.segments[0]!.text, speakerLabel: "Speaker 0", startMs: 1000 },
  ])
  expect(evidence.transcriptRevision).toBe(1)
})

test("the first words leave before the model finishes, and closing the request aborts the provider", async () => {
  let aborted = false
  const f = await fixture({ async *evidenceStream(_note, _paragraph, signal) {
    yield '{"summary":"First words'
    await new Promise<void>(resolve => signal.addEventListener("abort", () => { aborted = true; resolve() }, { once: true }))
  } })
  const response = await f.call(request)
  const reader = response.body!.getReader()
  const first = new TextDecoder().decode((await reader.read()).value)
  expect(first).toContain('"type":"delta"')
  expect(first).toContain("First words")
  expect(first).not.toContain('"type":"done"')
  await reader.cancel()
  for (let i = 0; !aborted && i < 40; i++) await Bun.sleep(5)
  expect(aborted).toBe(true)
})

test("segment IDs are checked against the transcript: an unissued ID or more than 8 end in error, never done", async () => {
  const unissued = await fixture({ async *evidenceStream() { yield JSON.stringify({ summary: "Grounded-looking text", segmentIds: [crypto.randomUUID()] }) } })
  const first = events(await (await unissued.call(request)).text())
  expect(first.map(event => event.type)).toEqual(["delta", "error"])
  expect(first.some(event => event.evidence)).toBe(false)
  // A refused answer says so; it is not a load failure.
  expect(first.at(-1)!.text).toBe("Couldn't match this to the transcript. Try again.")
  const many = await fixture({ async *evidenceStream(note) { yield JSON.stringify({ summary: "Too many", segmentIds: Array(9).fill(note.transcriptSegments[0]!.id) }) } })
  expect(events(await (await many.call(request)).text()).at(-1)!.type).toBe("error")
  const prose = await fixture({ async *evidenceStream() { yield "Not JSON at all" } })
  expect(events(await (await prose.call(request)).text())).toEqual([{ type: "error", text: "Couldn't match this to the transcript. Try again." }])
  // The error says nothing about the provider.
  const failing = await fixture({ async *evidenceStream() { yield '{"summary":"Partial'; throw new Error("secret upstream key fixture") } })
  const failed = await (await failing.call(request)).text()
  expect(failed).toContain('"type":"error"')
  expect(failed).not.toContain("secret upstream")
  expect(failed).not.toContain('"type":"done"')
  expect(events(failed).at(-1)!.text).toBe("Transcript context could not be loaded. Try again.")
})

test("a summary past 20,000 characters is refused as it passes the limit, and the model is stopped there", async () => {
  let aborted = false, sent = 0
  const f = await fixture({ async *evidenceStream(_note, _paragraph, signal) {
    signal.addEventListener("abort", () => { aborted = true }, { once: true })
    yield '{"summary":"'
    // Far more than the limit, if nobody stopped it.
    for (let i = 0; i < 300 && !signal.aborted; i++) { sent += 100; yield "x".repeat(100) }
  } })
  const list = events(await (await f.call(request)).text())
  const shown = list.filter(event => event.type === "delta").map(event => event.text).join("")
  expect(shown.length).toBeLessThanOrEqual(20_000)
  expect(list.at(-1)).toEqual({ type: "error", text: "Couldn't match this to the transcript. Try again." })
  expect(aborted).toBe(true)
  expect(sent).toBeLessThan(21_000)
})

test("a transcript that changes while the model reads it ends in error; a stale revision is refused up front", async () => {
  let release!: () => void
  const gate = new Promise<void>(resolve => { release = resolve })
  const f = await fixture({ async *evidenceStream(note) {
    yield '{"summary":"Read from the old transcript",'
    await gate
    yield `"segmentIds":["${note.transcriptSegments[0]!.id}"]}`
  } })
  const pending = f.call(request)
  const response = await pending
  const body = response.text()
  await Bun.sleep(10)
  await Effect.runPromise(f.repo.updateNote(f.note.id, { transcriptSegments: [{ ...f.segments[0]!, text: "Cedar now launches Tuesday." }, f.segments[1]!] }))
  release()
  const list = events(await body)
  expect(list.at(-1)).toEqual({ type: "error", text: "Transcript changed; reopen the explanation" })
  const stale = await f.call({ ...request, expectedTranscriptRevision: 0 })
  expect(stale.status).toBe(400)
})

test("a provider that cannot stream is asked the buffered way, and its summary arrives as one delta", async () => {
  let buffered = 0
  const evidence: ProviderServices["evidence"] = note => { buffered++; return Effect.succeed({ summary: "Buffered summary", segmentIds: [note.transcriptSegments[0]!.id] }) }
  const plain = await fixture({ evidence })
  const list = events(await (await plain.call(request)).text())
  expect(list.map(event => event.type)).toEqual(["delta", "done"])
  expect(list[0]!.text).toBe("Buffered summary")
  expect(list[1]!.evidence!.quotes[0]!.segmentId).toBe(plain.segments[0]!.id)
  // A stream refused before its first token falls back once; one that already showed text does not.
  const refused = await fixture({ evidence, async *evidenceStream() { throw new Error("stream unsupported") } })
  expect(events(await (await refused.call(request)).text()).map(event => event.type)).toEqual(["delta", "done"])
  expect(buffered).toBe(2)
  const broken = await fixture({ evidence, async *evidenceStream() { yield '{"summary":"Half'; throw new Error("cut") } })
  expect(events(await (await broken.call(request)).text()).at(-1)!.type).toBe("error")
  expect(buffered).toBe(2)
  // A buffered answer the checks refuse is reported as refused, not as a load failure.
  const unmatched = await fixture({ evidence: () => Effect.fail(new ProviderFailure("language", "invalid_response")) })
  expect(events(await (await unmatched.call(request)).text())).toEqual([{ type: "error", text: "Couldn't match this to the transcript. Try again." }])
  const down = await fixture({ evidence: () => Effect.fail(new ProviderFailure("language")) })
  expect(events(await (await down.call(request)).text())).toEqual([{ type: "error", text: "Transcript context could not be loaded. Try again." }])
  // Without either, the route is unavailable rather than an empty stream.
  const none = await fixture()
  expect((await none.call(request)).status).toBe(503)
})

test("owner authentication, validation, another note and an empty transcript", async () => {
  let asked = false
  const f = await fixture({ async *evidenceStream() { asked = true; yield "{}" } })
  expect((await f.call(request, { authenticated: false })).status).toBe(401)
  expect((await f.call({ ...request, paragraph: "" })).status).toBe(400)
  expect((await f.call(request, { noteId: crypto.randomUUID() })).status).toBe(404)
  const empty = await Effect.runPromise(f.repo.createNote({ title: "No transcript fixture" }))
  const list = events(await (await f.call({ paragraph: "Typed notes", expectedTranscriptRevision: 0 }, { noteId: empty.id })).text())
  expect(list).toEqual([{ type: "done", text: "", evidence: { summary: "No supporting transcript is available for this note.", quotes: [], transcriptRevision: 0 } }])
  expect(asked).toBe(false)
})

test("at the provider: the buffered request plus stream, parsed across split bytes, cut-off answers refused", async () => {
  const bodies: Array<Record<string, unknown>> = []
  const answer = JSON.stringify({ summary: "Zoë 👋", segmentIds: [] })
  const frames = (finish: string | null) => new TextEncoder().encode(
    `: keepalive\r\n\r\ndata: ${JSON.stringify({ choices: [{ delta: { content: answer.slice(0, 7) } }] })}\r\n\r\n` +
    `data: ${JSON.stringify({ choices: [{ delta: { content: answer.slice(7) }, finish_reason: finish }] })}\n\ndata: {"choices":[],"usage":{}}\n\ndata: [DONE]\n\n`)
  let finish: string | null = "stop"
  const usage: string[] = []
  const providers = makeProviderServices({ openRouterKey: "fixture-router-key", onLanguageUsage: event => usage.push(event.feature), fetch: (async (_url, init) => {
    const body = JSON.parse(String(init!.body)); bodies.push(body)
    if (!body.stream) return Response.json({ choices: [{ message: { content: answer } }] })
    const bytes = frames(finish); let i = 0
    return new Response(new ReadableStream({ pull(c) { if (i < bytes.length) c.enqueue(bytes.slice(i, ++i)); else c.close() } }))
  }) as typeof fetch })
  const f = await fixture()
  await Effect.runPromise(providers.evidence!(f.note, "Monday"))
  // The buffered route reads a fenced answer as the streamed one does.
  const fenced = makeProviderServices({ openRouterKey: "fixture-router-key", fetch: (async () => Response.json({ choices: [{ message: { content: "```json\n" + JSON.stringify({ summary: "Fenced", segmentIds: [f.segments[0]!.id] }) + "\n```" } }] })) as unknown as typeof fetch })
  expect(await Effect.runPromise(fenced.evidence!(f.note, "Monday"))).toEqual({ summary: "Fenced", segmentIds: [f.segments[0]!.id] })
  const pieces: string[] = []
  for await (const piece of providers.evidenceStream!(f.note, "Monday", new AbortController().signal)) pieces.push(piece)
  expect(pieces.join("")).toBe(answer)
  const { stream, ...rest } = bodies[1]!
  expect(stream).toBe(true)
  // Same prompt, same parameters, same JSON mode: the cached transcript prefix is shared with the buffered route.
  expect(rest).toEqual(bodies[0]!)
  expect(usage).toEqual(["evidence"])
  finish = "length"
  const cut = (async () => { for await (const _ of providers.evidenceStream!(f.note, "Monday", new AbortController().signal)); })()
  await expect(cut).rejects.toThrow()
  const unconfigured = makeProviderServices({ openRouterKey: "", fetch: (async () => { throw new Error("must not be called") }) as unknown as typeof fetch })
  const refused = (async () => { for await (const _ of unconfigured.evidenceStream!(f.note, "Monday", new AbortController().signal)); })()
  await expect(refused).rejects.toThrow("language not_configured")
})

test("the summary reader returns only whole text from the summary value", () => {
  const read = (pieces: string[]) => { const reader = new EvidenceSummaryReader(); return pieces.map(piece => reader.push(piece)) }
  // An escape and a surrogate pair split across pieces come out whole, in the piece that completes them.
  expect(read(['{"summary":"a\\', 'nb \\ud83d', '\\udc4b c\\u00', 'e9"', ',"segmentIds":["x"]}'])).toEqual(["a", "\nb ", "👋 c", "é", ""])
  // IDs first, a fenced answer, nested values and other keys: only the summary string is read.
  expect(read(['```json\n{"segmentIds":["summary"],"meta":{"summary":"no"},"note":"summary",', '"summary":"yes"}\n```']).join("")).toBe("yes")
  // A second summary key is not streamed.
  expect(read(['{"summary":"one","summary":"two"}']).join("")).toBe("one")
  const note = { transcriptSegments: [{ id: "s1" }] } as never
  expect(readEvidence('```json\n{"summary":"ok","segmentIds":["s1","s1"]}\n```', note)).toEqual({ summary: "ok", segmentIds: ["s1"] })
  expect(readEvidence('{"summary":"ok","segmentIds":["s2"]}', note)).toBeNull()
  expect(readEvidence('{"summary":" ","segmentIds":[]}', note)).toBeNull()
  expect(readEvidence('{"summary":"ok"', note)).toBeNull()
})
