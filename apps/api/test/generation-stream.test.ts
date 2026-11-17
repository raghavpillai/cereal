import { afterEach, expect, test } from "bun:test"
import { Effect } from "effect"
import { makeApiHandler } from "../src/api"
import { makeProcessingService } from "../src/processing"
import { makeMemoryRepository } from "../src/repository"
import { ProviderFailure, type ProviderServices } from "../src/providers"
import { GeneratedTitleReader, generationSystemPrompt, splitGeneratedTitle } from "../src/ai/notes-generation"

const cleanup: Array<() => Promise<void>> = []
afterEach(async () => { for (const close of cleanup.splice(0)) await close() })
const owner = "generation-stream-owner-fixture-at-least-32-characters"
const ANSWER = "Title: Cedar launch date and review\n\n# Launch Date\n- Cedar launches Monday\n  - After the accessibility review\n\n# Next Steps\n- Zoë: confirm the date"
const DOCUMENT = ANSWER.slice(ANSWER.indexOf("# Launch"))

async function fixture(extra: Partial<ProviderServices> = {}, input: { title?: string; transcript?: boolean } = {}) {
  const repo = makeMemoryRepository()
  const segments = input.transcript === false ? [] : [
    { id: crypto.randomUUID(), channel: "system" as const, speakerId: "system:n:1", speakerLabel: null, text: "Cedar launches Monday after accessibility review.", startMs: 1000, endMs: 3000, isFinal: true },
    { id: crypto.randomUUID(), channel: "mic" as const, speakerId: "mic:n:1", speakerLabel: "Me", text: "Zoë will confirm the date.", startMs: 44_000, endMs: 46_000, isFinal: true },
  ]
  const note = await Effect.runPromise(repo.createNote({ title: input.title ?? "", transcriptSegments: segments }))
  const providers: ProviderServices = { generate: () => Effect.succeed(ANSWER), chat: () => Effect.succeed(""), ...extra }
  const api = makeApiHandler(repo, owner, { processing: makeProcessingService(repo, { providers }) })
  cleanup.push(api.dispose)
  const call = (path: string, payload: unknown) => api.handler(new Request(`http://localhost/api/v1/notes/${note.id}${path}`, {
    method: "POST", headers: { "Content-Type": "application/json", Authorization: `Bearer ${owner}` }, body: JSON.stringify(payload) }))
  const saved = async () => (await Effect.runPromise(repo.getNote(note.id)))!
  return { repo, note, call, saved }
}
type Event = { type: "title" | "delta" | "done" | "error"; text: string; note?: { enhanced: string; title: string; generationState: string; generatedRevision: number | null } }
const events = (body: string): Event[] => body.split("\n\n").map(frame => frame.split("\n").filter(line => line.startsWith("data:")).map(line => line.slice(5).trimStart()).join("\n")).filter(Boolean).map(data => JSON.parse(data))
const request = { expectedTranscriptRevision: 1 }
const pieces = async function* (text: string, size = 5) { for (let i = 0; i < text.length; i += size) yield text.slice(i, i + size) }

test("the title comes first, the notes stream as deltas in order, and done carries the saved note", async () => {
  const f = await fixture({ generateStream: () => pieces(ANSWER) })
  const response = await f.call("/generate/stream", request)
  expect(response.status).toBe(200)
  expect(response.headers.get("content-type")).toContain("text/event-stream")
  const list = events(await response.text())
  expect(list[0]).toEqual({ type: "title", text: "Cedar launch date and review" })
  const deltas = list.filter(event => event.type === "delta")
  expect(deltas.length).toBeGreaterThan(5)
  expect(deltas.map(event => event.text).join("")).toBe(DOCUMENT)
  expect(list.map(event => event.type).filter(type => type !== "delta")).toEqual(["title", "done"])
  const done = list.at(-1)!.note!
  expect(done.enhanced).toBe(DOCUMENT)
  expect(done.title).toBe("Cedar launch date and review")
  expect(done.generationState).toBe("ready")
  expect(done.generatedRevision).toBe(1)
  expect((await f.saved()).enhanced).toBe(DOCUMENT)
})

test("the first words leave before the model finishes; a client that goes away does not stop the generation, which is saved", async () => {
  let release!: () => void, aborted = false
  const held = new Promise<void>(resolve => { release = resolve })
  const f = await fixture({ async *generateStream(_note, signal) {
    signal.addEventListener("abort", () => { aborted = true })
    yield "Title: Cedar launch date and review\n\n# Launch Date\n- Cedar launches"
    await held
    yield " Monday"
  } })
  const response = await f.call("/generate/stream", request)
  const reader = response.body!.getReader()
  let first = ""
  while (!first.includes("Cedar launches")) first += new TextDecoder().decode((await reader.read()).value)
  expect(first).toContain('"type":"title"')
  expect(first).not.toContain('"type":"done"')
  expect((await f.saved()).generationState).toBe("writing_notes")
  await reader.cancel()
  await Bun.sleep(20)
  release()
  for (let i = 0; (await f.saved()).generationState !== "ready" && i < 100; i++) await Bun.sleep(5)
  const saved = await f.saved()
  expect(aborted).toBe(false)
  expect(saved.generationState).toBe("ready")
  expect(saved.enhanced).toBe("# Launch Date\n- Cedar launches Monday")
  expect(saved.title).toBe("Cedar launch date and review")
})

test("a provider that cannot stream, or refuses before its first word, is asked the buffered way: one delta, then done", async () => {
  for (const extra of [{}, { async *generateStream() { throw new ProviderFailure("language") } }] as Partial<ProviderServices>[]) {
    const f = await fixture(extra)
    const list = events(await (await f.call("/generate/stream", request)).text())
    expect(list.map(event => event.type)).toEqual(["title", "delta", "done"])
    expect(list[1]!.text).toBe(DOCUMENT)
    expect((await f.saved()).enhanced).toBe(DOCUMENT)
  }
})

test("a failure once words were sent ends in error: nothing is saved and the note can be generated again", async () => {
  const f = await fixture({ async *generateStream() { yield "# Launch Date\n- Cedar"; throw new ProviderFailure("language") } })
  const list = events(await (await f.call("/generate/stream", request)).text())
  expect(list.map(event => event.type)).toEqual(["delta", "error"])
  expect(list.at(-1)!.text).toBe("Notes could not be generated. Try again.")
  const saved = await f.saved()
  expect(saved.enhanced).toBe("")
  expect(saved.generationState).toBe("failed")
  expect(saved.generatedRevision).toBeNull()
  expect((await f.call("/generate", request)).status).toBe(200)
})

test("what a buffered generation refuses is refused the same way, before any event", async () => {
  const empty = await fixture({}, { transcript: false })
  const refused = await empty.call("/generate/stream", { expectedTranscriptRevision: 0 })
  expect(refused.status).toBe(400)
  expect((await refused.json()).message).toContain("no transcript")
  const stale = await fixture()
  expect((await stale.call("/generate/stream", { expectedTranscriptRevision: 7 })).status).toBe(400)
  expect((await stale.saved()).generationState).toBe("idle")
})

test("a note that has a title keeps it: no title is asked for, and one the model writes anyway is dropped", async () => {
  const f = await fixture({ generateStream: () => pieces(ANSWER) }, { title: "Cedar weekly" })
  expect(generationSystemPrompt({ ...f.note })).not.toContain("# The title")
  expect(generationSystemPrompt({ ...f.note, title: "" })).toContain('write one line that starts with "Title: "')
  const list = events(await (await f.call("/generate/stream", request)).text())
  expect(list.some(event => event.type === "title")).toBe(false)
  expect(list.at(-1)!.note!.title).toBe("Cedar weekly")
  expect(list.at(-1)!.note!.enhanced).toBe(DOCUMENT)
})

test("the buffered route names an untitled note from the same answer, and never saves the title line as notes", async () => {
  let asked = 0
  const f = await fixture({ title: () => { asked++; return Effect.succeed("Another title entirely") } })
  const generated = await (await f.call("/generate", request)).json()
  expect(generated.enhanced).toBe(DOCUMENT)
  expect(generated.title).toBe("Cedar launch date and review")
  expect(asked).toBe(0)
  // An answer without the line is named by the separate request, as before.
  const plain = await fixture({ generate: () => Effect.succeed(DOCUMENT), title: () => { asked++; return Effect.succeed("Cedar launch plan") } })
  const named = await (await plain.call("/generate", request)).json()
  expect(named.enhanced).toBe(DOCUMENT)
  expect(named.title).toBe("Cedar launch plan")
  expect(asked).toBe(1)
})

test("the title line is read off the front of an answer however it is cut, and only there", () => {
  for (const size of [1, 2, 3, 7, 1000]) {
    const reader = new GeneratedTitleReader()
    let document = ""
    for (let i = 0; i < ANSWER.length; i += size) document += reader.push(ANSWER.slice(i, i + size))
    document += reader.push("", true)
    expect(reader.title).toBe("Cedar launch date and review")
    expect(document).toBe(DOCUMENT)
  }
  expect(splitGeneratedTitle(DOCUMENT)).toEqual({ title: null, document: DOCUMENT })
  expect(splitGeneratedTitle("\n title: NONE\n\n# A\n- b")).toEqual({ title: "NONE", document: "# A\n- b" })
  expect(splitGeneratedTitle("Title: Only a title")).toEqual({ title: "Only a title", document: "" })
  // A "Title:" further down is the document's own text.
  expect(splitGeneratedTitle("# Naming\n- Title: undecided")).toEqual({ title: null, document: "# Naming\n- Title: undecided" })
  expect(splitGeneratedTitle("Tidy notes\n- a")).toEqual({ title: null, document: "Tidy notes\n- a" })
})

test("the format the model is asked for: nested bullets, action items as task then owner with the deadline under it, no placeholders, no padding section", () => {
  const prompt = generationSystemPrompt({ id: crypto.randomUUID(), title: "", body: "", enhanced: "", date: "2026-11-16", duration: "3:00", createdAt: "", updatedAt: "", recordingState: "stopped", generationState: "idle", transcriptRevision: 1, generatedRevision: null, transcriptSegments: [] } as never)
  expect(prompt).toContain('Sub-bullets (two spaces, then "- ")')
  expect(prompt).toContain("If nobody took on a task, there is no such section")
  expect(prompt).toContain('Never a placeholder such as "(no owner)"')
  expect(prompt).toContain("- Send the yard system integration spec (Owen)\n  Due Friday,")
  expect(prompt).not.toContain("**")
})
