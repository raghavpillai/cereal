/** A note without a title is named when its notes are generated (`src/ai/note-title.ts`). */
import { afterEach, describe, expect, test } from "bun:test"
import { Effect } from "effect"
import { makeApiHandler } from "../src/api"
import { makeMemoryRepository } from "../src/repository"
import { makePrismaRepository } from "../src/prisma-repository"
import { makeProcessingService } from "../src/processing"
import { makeProviderServices, ProviderFailure, type ProviderNote, type ProviderServices } from "../src/providers"
import { cleanNoteTitle, isUntitled, titleMessages, TITLE_INSTRUCTIONS, TITLE_MAX_CHARACTERS, TITLE_PARAMETERS } from "../src/ai/note-title"
import { generationMessages } from "../src/ai/notes-generation"

const token = "title-test-owner-token-with-at-least-32-characters"
const NOTES = "# Flour Supplier Switch\n- Switching from Greystone Mills to Windrush Organics from 1 January\n\n# Next Steps\n- **Send the signed contract to Windrush** (Noemi)"
const segment = (text: string, startMs = 0) => ({ id: crypto.randomUUID(), channel: "mic" as const, speakerId: "mic", speakerLabel: null, text, isFinal: true, startMs, endMs: startMs + 4000 })
const transcript = () => [segment("Let's decide the flour supplier today. Windrush quoted six hundred and twelve pounds a tonne."), segment("Then we switch from the first of January. Noemi, can you send them the signed contract?", 5000)]

const cleanup: Array<() => Promise<unknown>> = []
afterEach(async () => { for (const dispose of cleanup.splice(0).reverse()) await dispose() })

/** The real HTTP layer and processing on the memory repository, with a provider whose answers the test chooses. */
function fixture(title: ProviderServices["title"] | null = () => Effect.succeed("Flour supplier switch to Windrush"), overrides: Partial<ProviderServices> = {}) {
  const repository = makeMemoryRepository()
  const calls = { generate: 0, title: [] as Array<{ note: ProviderNote; notes: string }> }
  const providers: ProviderServices = {
    transcribe: () => Effect.succeed([]),
    generate: () => Effect.sync(() => { calls.generate++; return NOTES }),
    chat: () => Effect.succeed(""),
    ...(title ? { title: (note, notes) => { calls.title.push({ note, notes }); return title(note, notes) } } : {}),
    ...overrides,
  }
  const processing = makeProcessingService(repository, { providers, worker: false })
  const api = makeApiHandler(repository, token, { processing })
  cleanup.push(api.dispose)
  const request = (path: string, method = "GET", body?: unknown) => api.handler(new Request(`http://localhost/api/v1${path}`, {
    method, headers: { Authorization: `Bearer ${token}`, ...(body === undefined ? {} : { "Content-Type": "application/json" }) }, ...(body === undefined ? {} : { body: JSON.stringify(body) }),
  }))
  const create = async (noteTitle: string, extra: Record<string, unknown> = {}) => await (await request("/notes", "POST", { title: noteTitle, transcriptSegments: transcript(), ...extra })).json() as { id: string; title: string; updatedAt: string; transcriptRevision: number }
  const generate = (note: { id: string; transcriptRevision: number }) => request(`/notes/${note.id}/generate`, "POST", { expectedTranscriptRevision: note.transcriptRevision })
  const read = async (id: string) => await (await request(`/notes/${id}`)).json() as { title: string; enhanced: string; generationState: string; generatedRevision: number | null; updatedAt: string; body: string }
  return { repository, providers, processing, calls, request, create, generate, read }
}

describe("a title for a note that has none", () => {
  test("an untitled note gets its title in the same answer as its notes, and every way of reading the note shows it", async () => {
    const f = fixture()
    const note = await f.create("", { body: "windrush 612/t?" })
    expect(note.title).toBe("")
    await Bun.sleep(5)
    const response = await f.generate(note)
    expect(response.status).toBe(200)
    const generated = await response.json()
    expect(generated).toMatchObject({ title: "Flour supplier switch to Windrush", enhanced: NOTES, generationState: "ready", generatedRevision: note.transcriptRevision })
    // The notes are exactly what the notes request returned, and the title request was given them.
    expect(f.calls.generate).toBe(1)
    expect(f.calls.title).toHaveLength(1)
    expect(f.calls.title[0]!.notes).toBe(NOTES)
    expect(f.calls.title[0]!.note.id).toBe(note.id)
    const saved = await f.read(note.id)
    expect(saved).toMatchObject({ title: "Flour supplier switch to Windrush", enhanced: NOTES, body: "windrush 612/t?" })
    expect(Date.parse(saved.updatedAt)).toBeGreaterThan(Date.parse(note.updatedAt))
    const list = await (await f.request("/notes")).json() as Array<{ id: string; title: string }>
    expect(list.find(row => row.id === note.id)!.title).toBe("Flour supplier switch to Windrush")
    const summaries = await (await f.request("/notes/summaries")).json() as { notes: Array<{ id: string; title: string; updatedAt: string }> }
    expect(summaries.notes.find(row => row.id === note.id)).toMatchObject({ title: "Flour supplier switch to Windrush", updatedAt: saved.updatedAt })
    const found = await (await f.request("/notes/search?q=Windrush%20switch")).json() as { results: Array<{ note: { id: string; title: string } }> }
    expect(found.results.map(result => result.note.title)).toContain("Flour supplier switch to Windrush")
  })

  test("the placeholders the apps show count as no title; whitespace too", async () => {
    for (const placeholder of ["Untitled note", "New note", "untitled NOTE", "   ", " New note "]) {
      const f = fixture()
      const note = await f.create(placeholder)
      expect(isUntitled(placeholder)).toBe(true)
      expect((await (await f.generate(note)).json()).title, placeholder).toBe("Flour supplier switch to Windrush")
    }
    for (const real of ["Budget", "Untitled note 2", "Notes", "New note about pricing", "x"]) expect(isUntitled(real), real).toBe(false)
  })

  test("a note that has a title is never retitled, and nothing is asked", async () => {
    const f = fixture()
    const note = await f.create("Supplier call")
    const generated = await (await f.generate(note)).json()
    expect(generated).toMatchObject({ title: "Supplier call", enhanced: NOTES })
    // Regenerating does not ask either.
    expect((await (await f.generate(note)).json()).title).toBe("Supplier call")
    expect(f.calls.generate).toBe(2)
    expect(f.calls.title).toHaveLength(0)
  })

  test("the answer is cleaned before it is saved", async () => {
    for (const [answer, saved] of [
      ["\"Flour supplier switch to Windrush\"", "Flour supplier switch to Windrush"],
      ["Flour supplier switch to Windrush.", "Flour supplier switch to Windrush"],
      ["Title: “Flour supplier switch”.\n\nI chose this because the meeting decided it.", "Flour supplier switch"],
      ["\n\n  **Flour supplier switch**  \n", "Flour supplier switch"],
    ] as const) {
      const f = fixture(() => Effect.succeed(answer))
      const note = await f.create("")
      expect((await (await f.generate(note)).json()).title, answer).toBe(saved)
    }
  })

  test("no usable answer, a failed request or a provider that cannot title: the notes are saved and the note stays untitled", async () => {
    const outcomes: Array<[string, ProviderServices["title"] | null]> = [
      ["empty", () => Effect.succeed("")],
      ["blank lines", () => Effect.succeed(" \n\n ")],
      ["NONE", () => Effect.succeed("NONE")],
      ["generic", () => Effect.succeed("Meeting notes")],
      ["refusal", () => Effect.succeed("I'm sorry, but I can't help with naming this meeting.")],
      ["prose", () => Effect.succeed("This meeting covered the decision to switch the flour supplier from Greystone Mills to Windrush Organics starting in January and the contract paperwork")],
      ["provider failure", () => Effect.fail(new ProviderFailure("language"))],
      ["not configured", () => Effect.fail(new ProviderFailure("language", "not_configured"))],
      ["thrown", () => Effect.sync(() => { throw new Error("boom") })],
      ["rejected", () => Effect.promise(() => Promise.reject(new Error("network")))],
      ["no title support", null],
    ]
    for (const [name, title] of outcomes) {
      const f = fixture(title)
      const note = await f.create("")
      const response = await f.generate(note)
      expect(response.status, name).toBe(200)
      expect(await response.json(), name).toMatchObject({ title: "", enhanced: NOTES, generationState: "ready", generatedRevision: note.transcriptRevision })
      expect(await f.read(note.id), name).toMatchObject({ title: "", enhanced: NOTES, generationState: "ready" })
    }
  })

  test("a note left untitled is titled the next time its notes are generated; then never again", async () => {
    let attempt = 0
    const f = fixture(() => ++attempt === 1 ? Effect.fail(new ProviderFailure("language")) : Effect.succeed(`Flour supplier switch attempt ${attempt}`))
    const note = await f.create("")
    expect((await (await f.generate(note)).json()).title).toBe("")
    expect((await (await f.generate(note)).json()).title).toBe("Flour supplier switch attempt 2")
    expect((await (await f.generate(note)).json()).title).toBe("Flour supplier switch attempt 2")
    expect(attempt).toBe(2)
    expect(f.calls.generate).toBe(3)
  })

  test("failed notes are not titled, and the title is not asked for", async () => {
    const f = fixture(undefined, { generate: () => Effect.fail(new ProviderFailure("language")) })
    const note = await f.create("")
    expect((await f.generate(note)).status).toBe(503)
    expect(await f.read(note.id)).toMatchObject({ title: "", enhanced: "", generationState: "failed" })
    expect(f.calls.title).toHaveLength(0)
  })

  test("a title the person types while the title is being asked for is theirs: the late result is refused whole", async () => {
    let release!: (title: string) => void
    const asked = new Promise<string>(resolve => { release = resolve })
    let started!: () => void
    const waiting = new Promise<void>(resolve => { started = resolve })
    const f = fixture(() => Effect.promise(() => { started(); return asked }))
    const note = await f.create("")
    const pending = f.generate(note)
    await waiting
    expect((await f.request(`/notes/${note.id}`, "PATCH", { title: "Bakery suppliers" })).status).toBe(200)
    release("Flour supplier switch to Windrush")
    // Renaming a note during generation is an edit like any other: the existing guard refuses the late result.
    expect((await pending).status).toBe(400)
    expect(await f.read(note.id)).toMatchObject({ title: "Bakery suppliers", enhanced: "" })
  })

  test("a placeholder sent back by an app that has not read the new title does not erase it; clearing and renaming still work", async () => {
    const f = fixture()
    const note = await f.create("Untitled note")
    await f.generate(note)
    for (const placeholder of ["Untitled note", "New note", " untitled note "]) {
      const kept = await (await f.request(`/notes/${note.id}`, "PATCH", { title: placeholder, body: "typed later" })).json()
      expect(kept, placeholder).toMatchObject({ title: "Flour supplier switch to Windrush", body: "typed later", generationState: "ready" })
    }
    expect((await (await f.request(`/notes/${note.id}`, "PATCH", { title: "Windrush contract" })).json()).title).toBe("Windrush contract")
    expect((await (await f.request(`/notes/${note.id}`, "PATCH", { title: "" })).json()).title).toBe("")
    // Without a real title the placeholder is stored as before.
    expect((await (await f.request(`/notes/${note.id}`, "PATCH", { title: "Untitled note" })).json()).title).toBe("Untitled note")
  })
})

describe("the write that saves the notes decides whether the title is saved", () => {
  for (const kind of ["memory", "postgres"] as const) {
    const enabled = kind === "memory" || process.env.RUN_PG_TESTS === "1" && !!process.env.DATABASE_URL
    test.skipIf(!enabled)(`${kind}: saved for an untitled note, kept out when the note has a title by then, never saved with failed notes`, async () => {
      const resources = kind === "postgres" ? makePrismaRepository(process.env.DATABASE_URL!) : null
      const repository = resources?.repository ?? makeMemoryRepository(), ids: string[] = []
      const begin = async (title: string) => {
        const note = await Effect.runPromise(repository.createNote({ title, date: "2026-01-01", transcriptSegments: transcript() })); ids.push(note.id)
        const run = crypto.randomUUID()
        expect(await Effect.runPromise(repository.beginGeneration(note.id, note.transcriptRevision, run))).not.toBeNull()
        return { note, run }
      }
      try {
        // Untitled: notes and title in one write, and the note's modification time moves.
        const untitled = await begin("")
        await Bun.sleep(5)
        const saved = await Effect.runPromise(repository.finishGeneration(untitled.note.id, untitled.note.transcriptRevision, untitled.run, NOTES, "Flour supplier switch"))
        expect(saved).toMatchObject({ title: "Flour supplier switch", enhanced: NOTES, generationState: "ready", generatedRevision: untitled.note.transcriptRevision })
        expect(Date.parse(saved!.updatedAt)).toBeGreaterThan(Date.parse(untitled.note.updatedAt))
        expect((await Effect.runPromise(repository.getNote(untitled.note.id)))!.title).toBe("Flour supplier switch")
        // A placeholder counts as untitled.
        const placeholder = await begin("Untitled note")
        expect((await Effect.runPromise(repository.finishGeneration(placeholder.note.id, placeholder.note.transcriptRevision, placeholder.run, NOTES, "Flour supplier switch")))!.title).toBe("Flour supplier switch")
        // Titled before generation began: the suggestion is ignored.
        const titled = await begin("Supplier call")
        expect(await Effect.runPromise(repository.finishGeneration(titled.note.id, titled.note.transcriptRevision, titled.run, NOTES, "Flour supplier switch"))).toMatchObject({ title: "Supplier call", enhanced: NOTES })
        // Titled while the model was working, by a writer that does not end the generation: a calendar event
        // attached to the note writes its title straight to the row. The notes are saved; the title is theirs.
        // (Only the database has such a writer. In memory every title change goes through updateNote, below.)
        if (resources) {
          const meanwhile = await begin("")
          await resources.database.note.update({ where: { id: meanwhile.note.id }, data: { title: "Windrush quarterly review" } })
          expect(await Effect.runPromise(repository.finishGeneration(meanwhile.note.id, meanwhile.note.transcriptRevision, meanwhile.run, NOTES, "Flour supplier switch"))).toMatchObject({ title: "Windrush quarterly review", enhanced: NOTES, generationState: "ready" })
        }
        // No suggestion, or an empty one: nothing is written to the title.
        const none = await begin("")
        expect((await Effect.runPromise(repository.finishGeneration(none.note.id, none.note.transcriptRevision, none.run, NOTES)))!.title).toBe("")
        const empty = await begin("")
        expect((await Effect.runPromise(repository.finishGeneration(empty.note.id, empty.note.transcriptRevision, empty.run, NOTES, "")))!.title).toBe("")
        // Failed notes carry no title, even if one is offered.
        const failed = await begin("")
        expect(await Effect.runPromise(repository.finishGeneration(failed.note.id, failed.note.transcriptRevision, failed.run, null, "Flour supplier switch"))).toMatchObject({ title: "", enhanced: "", generationState: "failed" })
        // A rename during generation ends the run, as every edit does: the late result is refused, title and all.
        const renamed = await begin("")
        await Effect.runPromise(repository.updateNote(renamed.note.id, { title: "Bakery suppliers" }))
        expect(await Effect.runPromise(repository.finishGeneration(renamed.note.id, renamed.note.transcriptRevision, renamed.run, NOTES, "Flour supplier switch"))).toBeNull()
        expect(await Effect.runPromise(repository.getNote(renamed.note.id))).toMatchObject({ title: "Bakery suppliers", enhanced: "" })
        // A placeholder sent back later does not erase a real title; an empty title clears it.
        expect((await Effect.runPromise(repository.updateNote(untitled.note.id, { title: "Untitled note", body: "later" })))).toMatchObject({ title: "Flour supplier switch", body: "later", generationState: "ready" })
        expect((await Effect.runPromise(repository.updateNote(untitled.note.id, { title: "" })))!.title).toBe("")
        // The scoped (per-account) repository passes the suggestion through.
        const owned = repository.forUser("00000000-0000-4000-8000-000000000001")
        const scoped = await begin("")
        expect((await Effect.runPromise(owned.finishGeneration(scoped.note.id, scoped.note.transcriptRevision, scoped.run, NOTES, "Flour supplier switch")))!.title).toBe("Flour supplier switch")
      } finally { for (const id of ids) await Effect.runPromise(repository.deleteNote(id)); await resources?.close() }
    })
  }
})

describe("cleaning the model's answer", () => {
  test("quotation marks, labels, Markdown and closing punctuation go; the wording and its capitals stay", () => {
    for (const [raw, title] of [
      ["Flour supplier switch to Windrush", "Flour supplier switch to Windrush"],
      ["\"Flour supplier switch to Windrush\"", "Flour supplier switch to Windrush"],
      ["“Flour supplier switch to Windrush”", "Flour supplier switch to Windrush"],
      ["'Flour supplier switch to Windrush'", "Flour supplier switch to Windrush"],
      ["«Changement de fournisseur de farine»", "Changement de fournisseur de farine"],
      ["`Flour supplier switch`", "Flour supplier switch"],
      ["Flour supplier switch to Windrush.", "Flour supplier switch to Windrush"],
      ["Flour supplier switch to Windrush!", "Flour supplier switch to Windrush"],
      ["Should we switch flour supplier?", "Should we switch flour supplier"],
      ["Flour supplier switch…", "Flour supplier switch"],
      ["\"Flour supplier switch.\"", "Flour supplier switch"],
      ["\"Flour supplier switch\".", "Flour supplier switch"],
      ["Title: Flour supplier switch", "Flour supplier switch"],
      ["Here is the title: \"Flour supplier switch\"", "Flour supplier switch"],
      ["Meeting title: Flour supplier switch", "Flour supplier switch"],
      ["# Flour supplier switch", "Flour supplier switch"],
      ["**Flour supplier switch**", "Flour supplier switch"],
      ["- Flour supplier switch", "Flour supplier switch"],
      ["```\nFlour supplier switch\n```", "Flour supplier switch"],
      ["  Flour   supplier\tswitch  ", "Flour supplier switch"],
      ["Kim's review of Q3 hiring", "Kim's review of Q3 hiring"],
      ["Windrush \"north star\" pricing review", "Windrush north star pricing review"],
      ["Northwind onboarding (pilot of 10)", "Northwind onboarding (pilot of 10)"],
      ["Launch readiness: iPhone review risk", "Launch readiness: iPhone review risk"],
      ["Budget review", "Budget review"],
      ["Wechsel des Mehllieferanten zu Windrush", "Wechsel des Mehllieferanten zu Windrush"],
      ["面粉供应商更换为温德拉什", "面粉供应商更换为温德拉什"],
      ["「小麦粉の仕入先変更」", "小麦粉の仕入先変更"],
      ["Cambio de proveedor de harina.", "Cambio de proveedor de harina"],
    ] as const) expect(cleanNoteTitle(raw), raw).toBe(title)
  })

  test("several lines: the first one with anything on it is the title", () => {
    expect(cleanNoteTitle("Flour supplier switch\nThe team agreed to move to Windrush.")).toBe("Flour supplier switch")
    expect(cleanNoteTitle("\n\n\r\nFlour supplier switch\r\n\r\nAlternative: Windrush contract")).toBe("Flour supplier switch")
    expect(cleanNoteTitle("Flour supplier\u2028switch")).toBe("Flour supplier")
    expect(cleanNoteTitle("Flour\u0000 supplier\u200b switch")).toBe("Flour supplier switch")
  })

  test("over-long answers are cut at a word, to eight words and sixty characters, and never end on a connective", () => {
    const cut = cleanNoteTitle("Flour supplier switch from Greystone Mills to Windrush Organics and the contract")!
    expect(cut).toBe("Flour supplier switch from Greystone Mills to Windrush")
    expect(cut.length).toBeLessThanOrEqual(TITLE_MAX_CHARACTERS)
    expect(cut.split(" ").length).toBeLessThanOrEqual(8)
    // Nine short words: within sixty characters, still cut to eight, and the dangling "and" goes with the ninth.
    expect(cleanNoteTitle("Plan for the Q3 hiring budget review and offsite")).toBe("Plan for the Q3 hiring budget review")
    // Long words: the character limit decides.
    const long = cleanNoteTitle("Internationalisation infrastructure modernisation roadmap prioritisation workshop")!
    expect(long).toBe("Internationalisation infrastructure modernisation roadmap")
    expect(long.length).toBeLessThanOrEqual(TITLE_MAX_CHARACTERS)
    // Exactly sixty characters stays whole; sixty-one loses its last word.
    const sixty = "Aaaaaaaaa bbbbbbbbb ccccccccc ddddddddd eeeeeeeee ffffffffff"
    expect(sixty.length).toBe(60)
    expect(cleanNoteTitle(sixty)).toBe(sixty)
    expect(cleanNoteTitle(sixty + "f")).toBe("Aaaaaaaaa bbbbbbbbb ccccccccc ddddddddd eeeeeeeee")
    // Eight words stay whole; a ninth goes.
    expect(cleanNoteTitle("One two three four five six seven eight")).toBe("One two three four five six seven eight")
    expect(cleanNoteTitle("One two three four five six seven eight nine")).toBe("One two three four five six seven eight")
    expect(cleanNoteTitle("a".repeat(61) + " b")).toBeNull()
    // Far over the limits is prose, not a title.
    expect(cleanNoteTitle("The team met to talk about the flour supplier and agreed to switch to Windrush from January")).toBeNull()
    expect(cleanNoteTitle("x".repeat(3000))).toBeNull()
  })

  test("what is not a title is refused", () => {
    for (const raw of [
      "", "   ", "\n\n", "\"\"", "...", "—", "```\n```", "NONE", "none.", "None", "N/A", "Untitled", "Untitled note", "New note", "Meeting", "Meeting notes", "Meeting Notes.", "Notes", "Discussion", "General discussion",
      "Team meeting", "Call summary", "Catch-up", "Summary", "Title", "Title:", "Budget", "2026",
      "I'm sorry, but I can't help with that.", "I cannot determine a title from these notes.", "Sorry, there is not enough information.", "I am unable to name this meeting",
      "As an AI, I can't name meetings", "Unfortunately the notes are empty", "The notes do not contain enough information", "Here are three options", "Sure! Flour supplier switch", "Not enough information",
      null, undefined, 42, { title: "Flour supplier switch" }, ["Flour supplier switch"],
    ]) expect(cleanNoteTitle(raw), JSON.stringify(raw)).toBeNull()
  })
})

describe("the title request", () => {
  const note = { id: crypto.randomUUID(), title: "", body: "", enhanced: "", date: "2026-03-09", duration: "0:09", createdAt: "2026-03-09T10:00:00.000Z", updatedAt: "2026-03-09T10:00:00.000Z", transcriptSegments: transcript(), recordingState: "stopped" as const, generationState: "idle" as const, transcriptRevision: 1, generatedRevision: null }

  test("two messages: cacheable instructions, then the finished notes and the opening of the transcript as data", () => {
    const messages = titleMessages(note, NOTES)
    expect(messages).toHaveLength(2)
    expect(messages[0]).toEqual({ role: "system", content: TITLE_INSTRUCTIONS, cache: true })
    expect(messages[1]!.role).toBe("user")
    const data = JSON.parse(messages[1]!.content)
    expect(Object.keys(data)).toEqual(["notes", "transcriptOpening"])
    expect(data.notes).toBe(NOTES)
    expect(data.transcriptOpening).toStartWith("Let's decide the flour supplier today.")
    // No identifiers, speaker labels or typed notes; and a long transcript is not sent again.
    expect(messages[1]!.content).not.toContain(note.id)
    expect(messages[1]!.content).not.toContain(note.transcriptSegments[0]!.id)
    const long = { ...note, transcriptSegments: Array.from({ length: 400 }, (_, index) => segment(`Sentence number ${index} of a long meeting about flour.`, index * 5000)) }
    expect(JSON.parse(titleMessages(long, NOTES)[1]!.content).transcriptOpening.length).toBeLessThanOrEqual(600)
    for (const rule of ["3 to 8 words", "at most 60 characters", "sentence case", "language of the transcript", "untrusted source data", "NONE"]) expect(TITLE_INSTRUCTIONS).toContain(rule)
    expect(TITLE_PARAMETERS).toMatchObject({ temperature: 0, cache: { ttl: "5m", tail: false } })
  })

  test("it goes to the configured notes model through the same transport, and the notes request is what it was", async () => {
    const bodies: any[] = []
    const transport = (async (_url, init) => {
      const body = JSON.parse(init!.body as string); bodies.push(body)
      return Response.json({ choices: [{ message: { content: bodies.length === 1 ? NOTES : "\"Flour supplier switch to Windrush.\"" } }] })
    }) as typeof fetch
    const providers = makeProviderServices({ openRouterKey: "fixture-not-a-real-key", languageModel: "openai/gpt-4o-mini", fetch: transport })
    const repository = makeMemoryRepository(), processing = makeProcessingService(repository, { providers, worker: false })
    cleanup.push(processing.dispose)
    const created = await Effect.runPromise(repository.createNote({ title: "", transcriptSegments: note.transcriptSegments }))
    const generated = await Effect.runPromise(processing.generate(created.id, created.transcriptRevision))
    expect(generated).toMatchObject({ title: "Flour supplier switch to Windrush", enhanced: NOTES })
    expect(bodies).toHaveLength(2)
    // The first request is the notes request, byte for byte what a titled note sends apart from the title field in its context.
    expect(bodies[0].messages).toEqual(generationMessages(created).reduce<Array<{ role: string; content: string }>>((merged, message) => {
      const previous = merged.at(-1)
      if (previous && previous.role === "system" && message.role === "system") previous.content += message.content
      else merged.push({ role: message.role, content: message.content })
      return merged
    }, []))
    expect(bodies[1].model).toBe(bodies[0].model)
    expect(bodies[1].messages[0]).toEqual({ role: "system", content: TITLE_INSTRUCTIONS })
    expect(JSON.parse(bodies[1].messages[1].content).notes).toBe(NOTES)
    expect(bodies[1].max_tokens).toBe(TITLE_PARAMETERS.maxTokens)
    expect(bodies[1].temperature).toBe(0)
    expect(bodies[1].stream).toBeUndefined()
  })

  test("a provider error, a cut-off answer or an empty one leaves the notes saved and the note untitled", async () => {
    for (const answer of [() => new Response("upstream failed", { status: 500 }), () => Response.json({ choices: [{ finish_reason: "length", message: { content: "Flour supplier" } }] }), () => Response.json({ choices: [{ message: { content: "  " } }] }), () => { throw new Error("connection reset") }]) {
      let requests = 0
      const transport = (async () => ++requests === 1 ? Response.json({ choices: [{ message: { content: NOTES } }] }) : answer()) as unknown as typeof fetch
      const repository = makeMemoryRepository(), processing = makeProcessingService(repository, { providers: makeProviderServices({ openRouterKey: "fixture-not-a-real-key", languageModel: "openai/gpt-4o-mini", fetch: transport }), worker: false })
      cleanup.push(processing.dispose)
      const created = await Effect.runPromise(repository.createNote({ title: "", transcriptSegments: note.transcriptSegments }))
      expect(await Effect.runPromise(processing.generate(created.id, created.transcriptRevision))).toMatchObject({ title: "", enhanced: NOTES, generationState: "ready" })
      expect(requests).toBe(2)
    }
  })
})
