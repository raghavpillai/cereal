import { afterEach, expect, test } from "bun:test"
import { Effect, Stream } from "effect"
import type { PrincipalData } from "@meeting-notes/contracts/api"
import type { ChatInput, ChatStreamEventDTO } from "@meeting-notes/contracts"
import { makeApiHandler } from "../src/api"
import { makeMemoryRepository, BOOTSTRAP_OWNER_ID } from "../src/repository"
import { makeProcessingService } from "../src/processing"
import { chatHistory, makeChatsService, makeMemoryChatStore, questionTitle } from "../src/chats"
import type { ProviderServices } from "../src/providers"

const close: Array<() => Promise<void>> = []
afterEach(async () => { for (const dispose of close.splice(0)) await dispose() })
const ids = { owner: BOOTSTRAP_OWNER_ID, admin: "40000000-0000-4000-8000-000000000002", alice: "40000000-0000-4000-8000-000000000003", bob: "40000000-0000-4000-8000-000000000004" }
type Account = keyof typeof ids

function fixture(options: { chatStream?: ProviderServices["chatStream"]; title?: (question: string, answer: string) => Promise<string | null>; clock?: () => Date } = {}) {
  const repository = makeMemoryRepository()
  const asked: ChatInput[] = []
  const processing = makeProcessingService(repository, { providers: {
    generate: () => Effect.succeed("generated"), chat: () => Effect.succeed("answer"),
    chatStream: options.chatStream ?? async function*(_note, input) { asked.push(input); yield "Answer to "; yield input.message },
  } })
  const chats = makeChatsService(makeMemoryChatStore(repository), { ...(options.title ? { title: options.title } : {}), ...(options.clock ? { clock: options.clock } : {}) })
  const api = makeApiHandler(repository, "chats-bootstrap-test-token-at-least-32-characters", {
    processing, chats,
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
  const json = async (account: Account | null, path: string, method = "GET", body?: unknown) => {
    const response = await call(account, path, method, body)
    return { status: response.status, body: await response.json() as any }
  }
  /** A thread with these exchanges already stored, as the person's own appends store them. */
  const seed = async (account: Account, body: object, turns: ReadonlyArray<ReturnType<typeof turn>>) => {
    const id = (await json(account, "/chats", "POST", body)).body.id as string
    for (const [index, exchange] of turns.entries()) await json(account, `/chats/${id}/turns`, "POST", { turn: index, ...exchange })
    return (await json(account, `/chats/${id}`)).body
  }
  return { call, json, seed, asked, chats, repository }
}
const turn = (question: string, answer: string, status: "complete" | "interrupted" | "failed" = "complete") => ({ question, answer: { content: answer, status } })
const events = (text: string) => text.split("\n\n").filter(frame => frame.startsWith("data:")).map(frame => JSON.parse(frame.slice(5)) as ChatStreamEventDTO)

test("a person creates, reads, renames, lists and deletes their own threads", async () => {
  const f = fixture()
  expect((await f.call(null, "/chats")).status).toBe(401)
  const empty = await f.json("alice", "/chats")
  expect(empty).toEqual({ status: 200, body: { threads: [], nextCursor: null, total: 0 } })
  const created = await f.json("alice", "/chats", "POST", {})
  expect(created.status).toBe(200)
  expect(created.body).toMatchObject({ noteId: null, title: null, titleSource: null, turnCount: 0, messages: [] })
  const id = created.body.id
  expect((await f.json("alice", `/chats/${id}`)).body.id).toBe(id)
  const renamed = await f.json("alice", `/chats/${id}`, "PATCH", { title: "  Launch questions  " })
  expect(renamed.body).toMatchObject({ title: "Launch questions", titleSource: "user" })
  expect((await f.json("alice", `/chats/${id}`, "PATCH", { title: "   " })).status).toBe(400)
  const listed = await f.json("alice", "/chats")
  expect(listed.body.threads.map((t: { id: string }) => t.id)).toEqual([id])
  expect(listed.body.total).toBe(1)
  expect(await f.json("alice", `/chats/${id}`, "DELETE")).toEqual({ status: 200, body: { deleted: true } })
  expect((await f.json("alice", `/chats/${id}`)).status).toBe(404)
  expect((await f.json("alice", `/chats/${id}`, "DELETE")).status).toBe(404)
})

test("creating with an id is idempotent and appended exchanges keep every kind of answer", async () => {
  const f = fixture()
  const id = crypto.randomUUID()
  const first = await f.seed("alice", { id }, [turn("What did we decide?", "Monday."), turn("Who owns it?", "Priya, so far", "interrupted"), turn("And the budget?", "", "failed")])
  expect(first).toMatchObject({ id, turnCount: 3, title: "What did we decide?", titleSource: "question" })
  expect(first.messages.map((m: any) => [m.turn, m.role, m.content, m.status])).toEqual([
    [0, "user", "What did we decide?", "complete"], [0, "assistant", "Monday.", "complete"],
    [1, "user", "Who owns it?", "complete"], [1, "assistant", "Priya, so far", "interrupted"],
    [2, "user", "And the budget?", "complete"], [2, "assistant", "", "failed"],
  ])
  // Sent again (the answer to the first try was lost): the same thread, nothing doubled, nothing changed.
  await f.chats.idle()
  const before = (await f.json("alice", `/chats/${id}`)).body
  const again = await f.json("alice", "/chats", "POST", { id, title: "Something else" })
  expect(again.status).toBe(200)
  expect(again.body).toEqual(before)
  expect((await f.json("alice", "/chats")).body.total).toBe(1)
})

test("nobody else, administrators and the owner included, can read, change or even detect a person's thread", async () => {
  const f = fixture()
  const note = await (await f.call("alice", "/notes", "POST", { title: "Alice private", body: "ALICE_PRIVATE" })).json() as { id: string }
  const thread = await f.seed("alice", { noteId: note.id }, [turn("Secret question", "ALICE_SECRET_ANSWER")])
  for (const account of ["bob", "admin", "owner"] as const) {
    const attempts: Array<[string, string, unknown?]> = [
      [`/chats/${thread.id}`, "GET"], [`/chats/${thread.id}`, "PATCH", { title: "Taken over" }], [`/chats/${thread.id}`, "DELETE"],
      [`/chats/${thread.id}/turns`, "POST", { turn: 1, ...turn("Injected", "Injected") }],
      [`/chats/${thread.id}/turns`, "POST", { turn: 0, replace: true, ...turn("Overwritten", "Overwritten") }],
      [`/chats/${thread.id}/stream`, "POST", { turn: 1, message: "Read Alice's thread" }],
      [`/chats/${crypto.randomUUID()}`, "GET"],
    ]
    for (const [path, method, body] of attempts) {
      const answer = await f.json(account, path, method, body)
      expect({ account, path, method, status: answer.status }).toEqual({ account, path, method, status: 404 })
      expect(JSON.stringify(answer.body)).not.toContain("ALICE")
    }
    expect((await f.json(account, "/chats")).body).toEqual({ threads: [], nextCursor: null, total: 0 })
    expect((await f.json(account, `/chats?noteId=${note.id}`)).body.total).toBe(0)
    // Alice's id cannot be claimed, and the answer says nothing about what is in it.
    const claim = await f.json(account, "/chats", "POST", { id: thread.id, title: "Mine now" })
    expect(claim.status).toBe(409)
    // A thread can only be about a note its owner can read.
    expect((await f.json(account, "/chats", "POST", { noteId: note.id })).status).toBe(404)
  }
  const after = (await f.json("alice", `/chats/${thread.id}`)).body
  expect(after.messages.map((m: any) => m.content)).toEqual(["Secret question", "ALICE_SECRET_ANSWER"])
  expect(after.title).not.toBe("Taken over")
  expect((await f.json("alice", "/chats")).body.total).toBe(1)
})

test("lists are newest first, paged with a cursor, counted for Show N more, and filtered by note or workspace", async () => {
  let now = Date.parse("2026-11-01T12:00:00.000Z")
  const f = fixture({ clock: () => new Date(now) })
  const note = await (await f.call("alice", "/notes", "POST", { title: "Planning" })).json() as { id: string }
  const made: string[] = []
  for (let i = 0; i < 25; i++) {
    now += 1000
    made.push((await f.json("alice", "/chats", "POST", i % 5 === 0 ? { noteId: note.id, title: "Named" } : { title: "Named" })).body.id)
  }
  // Two threads touched in the same millisecond are ordered by id, and paging does not skip or repeat either.
  now += 1000
  const tied = [(await f.json("alice", "/chats", "POST", { title: "Named" })).body.id, (await f.json("alice", "/chats", "POST", { title: "Named" })).body.id]
  // A thread nobody asked anything in or named is not listed.
  await f.json("alice", "/chats", "POST", {})
  made.push(...tied)
  const expected = [...made].reverse()
  expected.splice(0, 2, ...[...tied].sort().reverse())
  const seen: string[] = []
  let cursor: string | null = null, pages = 0
  do {
    const page: any = (await f.json("alice", `/chats?limit=10${cursor ? `&cursor=${cursor}` : ""}`)).body
    expect(page.total).toBe(27)
    expect(page.threads.length).toBeLessThanOrEqual(10)
    seen.push(...page.threads.map((t: { id: string }) => t.id))
    cursor = page.nextCursor; pages++
  } while (cursor)
  expect(pages).toBe(3)
  expect(seen).toEqual(expected)
  const first = (await f.json("alice", "/chats?limit=3")).body
  expect(first.threads.length).toBe(3)
  expect(first.total - first.threads.length).toBe(24)
  const ofNote = (await f.json("alice", `/chats?noteId=${note.id}`)).body
  expect(ofNote.total).toBe(5)
  expect(ofNote.threads.every((t: { noteId: string }) => t.noteId === note.id)).toBe(true)
  expect((await f.json("alice", "/chats?scope=workspace")).body.total).toBe(22)
  expect((await f.json("alice", "/chats?cursor=bm9wZQ")).status).toBe(400)
  expect((await f.json("alice", `/chats?scope=workspace&noteId=${note.id}`)).status).toBe(400)
  // A question asked brings its thread to the top.
  now += 1000
  await f.json("alice", `/chats/${made[0]}/turns`, "POST", { turn: 0, ...turn("Bring me back", "Back") })
  expect((await f.json("alice", "/chats?limit=1")).body.threads[0].id).toBe(made[0])
})

test("an exchange is stored once: a repeat changes nothing, a different one needs replace, a gap is refused", async () => {
  const f = fixture()
  const id = (await f.json("alice", "/chats", "POST", {})).body.id
  const path = `/chats/${id}/turns`
  const first = await f.json("alice", path, "POST", { turn: 0, ...turn("Q1", "A1") })
  expect(first.status).toBe(200)
  expect(first.body.turnCount).toBe(1)
  const repeat = await f.json("alice", path, "POST", { turn: 0, ...turn("Q1", "A1") })
  expect(repeat.status).toBe(200)
  expect(repeat.body.messages).toEqual(first.body.messages)
  expect((await f.json("alice", path, "POST", { turn: 0, ...turn("Q1 edited", "A1") })).status).toBe(409)
  expect((await f.json("alice", path, "POST", { turn: 2, ...turn("Q3", "A3") })).status).toBe(400)
  await f.json("alice", path, "POST", { turn: 1, ...turn("Q2", "A2") })
  await f.json("alice", path, "POST", { turn: 2, ...turn("Q3", "A3 so far", "interrupted") })
  // Editing the second question replaces it and drops what came after.
  const edited = await f.json("alice", path, "POST", { turn: 1, replace: true, ...turn("Q2 edited", "A2 again") })
  expect(edited.body.turnCount).toBe(2)
  expect(edited.body.messages.map((m: any) => m.content)).toEqual(["Q1", "A1", "Q2 edited", "A2 again"])
  expect((await f.call("alice", path, "POST", { turn: 500, ...turn("Too far", "Too far") })).status).toBe(400)
})

test("a streamed answer is saved by the server with its history read from the thread, and the turn rules hold", async () => {
  const f = fixture()
  const id = (await f.seed("alice", {}, [turn("Earlier", "Earlier answer"), turn("Cut off", "Half", "interrupted"), turn("Broken", "", "failed")])).id
  const response = await f.call("alice", `/chats/${id}/stream`, "POST", { turn: 3, message: "Next question", model: "provider/model-a" })
  expect(response.status).toBe(200)
  expect(response.headers.get("content-type")).toContain("text/event-stream")
  const streamed = events(await response.text())
  expect(streamed.map(e => e.type)).toEqual(["delta", "delta", "done"])
  // The model is sent the thread as the server has it, not what a client says it was.
  expect(f.asked.at(-1)?.history).toEqual([{ role: "user", content: "Earlier" }, { role: "assistant", content: "Earlier answer" }, { role: "user", content: "Cut off" }, { role: "assistant", content: "Half" }])
  expect(f.asked.at(-1)?.model).toBe("provider/model-a")
  const saved = (await f.json("alice", `/chats/${id}`)).body
  expect(saved.turnCount).toBe(4)
  expect(saved.messages.slice(-2).map((m: any) => [m.role, m.content, m.status, m.model])).toEqual([["user", "Next question", "complete", "provider/model-a"], ["assistant", "Answer to Next question", "complete", "provider/model-a"]])
  expect(typeof saved.messages.at(-1).elapsedMs).toBe("number")
  // The same exchange asked again from another device, without saying it replaces it.
  expect((await f.call("alice", `/chats/${id}/stream`, "POST", { turn: 3, message: "Next question" })).status).toBe(409)
  expect((await f.call("alice", `/chats/${id}/stream`, "POST", { turn: 9, message: "Gap" })).status).toBe(400)
  // Retrying it replaces it, once.
  await (await f.call("alice", `/chats/${id}/stream`, "POST", { turn: 3, replace: true, message: "Retried question" })).text()
  const retried = (await f.json("alice", `/chats/${id}`)).body
  expect(retried.turnCount).toBe(4)
  expect(retried.messages.filter((m: any) => m.role === "user").map((m: any) => m.content)).toEqual(["Earlier", "Cut off", "Broken", "Retried question"])
  expect((await f.call("alice", `/chats/${id}/stream`, "POST", { turn: 4, message: "x", timeZone: "Not/AZone" })).status).toBe(400)
})

test("an answer the client walks away from is kept, marked interrupted, with what had arrived", async () => {
  let aborted = false
  const f = fixture({ async *chatStream(_note, _input, signal) {
    yield "Part one, "
    await new Promise<void>(resolve => signal.addEventListener("abort", () => { aborted = true; resolve() }, { once: true }))
  } })
  const id = (await f.json("alice", "/chats", "POST", {})).body.id
  const response = await f.call("alice", `/chats/${id}/stream`, "POST", { turn: 0, message: "A long answer, please" })
  const reader = response.body!.getReader()
  expect(new TextDecoder().decode((await reader.read()).value)).toContain("Part one, ")
  // While it streams, another device sees the question and the answer arriving.
  const during = (await f.json("alice", `/chats/${id}`)).body
  expect(during.messages.map((m: any) => [m.role, m.status])).toEqual([["user", "complete"], ["assistant", "streaming"]])
  await reader.cancel()
  let saved: any
  for (let i = 0; i < 100; i++) {
    saved = (await f.json("alice", `/chats/${id}`)).body
    if (saved.messages[1].status !== "streaming") break
    await Bun.sleep(10)
  }
  expect(aborted).toBe(true)
  expect(saved.messages[1]).toMatchObject({ role: "assistant", content: "Part one, ", status: "interrupted" })
  // An interrupted first answer does not name the thread.
  await f.chats.idle()
  expect((await f.json("alice", `/chats/${id}`)).body.title).toBeNull()
})

test("an answer that fails is kept with its error, and a model that cannot start leaves the question with the reason", async () => {
  const failing = fixture({ async *chatStream() { yield "Partial"; throw new Error("provider secret fixture") } })
  const id = (await failing.json("alice", "/chats", "POST", {})).body.id
  const text = await (await failing.call("alice", `/chats/${id}/stream`, "POST", { turn: 0, message: "Question" })).text()
  expect(text).not.toContain("provider secret fixture")
  const saved = (await failing.json("alice", `/chats/${id}`)).body
  expect(saved.messages[1]).toMatchObject({ content: "Partial", status: "failed", error: "Chat could not finish. Please retry." })

  const f = fixture()
  const note = await (await f.call("alice", "/notes", "POST", { title: "Gone soon" })).json() as { id: string }
  const thread = (await f.json("alice", "/chats", "POST", { noteId: note.id })).body.id
  const chats = makeChatsService(makeMemoryChatStore())
  const store = (await Effect.runPromise(chats.create(ids.alice, f.repository.forUser(ids.alice), {}))).id
  const refused = await Effect.runPromise(Effect.exit(chats.stream(ids.alice, store, { turn: 0, message: "Will not start" }, () => Effect.fail({ error: "unavailable", message: "No provider" } as const))))
  expect(refused._tag).toBe("Failure")
  expect((await Effect.runPromise(chats.get(ids.alice, store))).messages.map(m => [m.content, m.status, m.error])).toEqual([["Will not start", "complete", null], ["", "failed", "Chat could not start. Please retry."]])
  expect(thread).toBeTruthy()
})

test("citations, tool progress and the elapsed time are saved with the answer", async () => {
  const chats = makeChatsService(makeMemoryChatStore())
  const notes = makeMemoryRepository().forUser(ids.alice)
  const id = (await Effect.runPromise(chats.create(ids.alice, notes, {}))).id
  const callId = crypto.randomUUID(), noteId = crypto.randomUUID()
  const citation = { id: "S1", noteId, title: "Standup", source: "transcript" as const, segmentId: null, startMs: 1200, quote: "We ship Monday", transcriptRevision: 3 }
  const script: ChatStreamEventDTO[] = [
    { type: "tool", text: "", tool: { callId, name: "search_meetings", status: "running", label: "Searching meetings" } },
    { type: "tool", text: "", tool: { callId, name: "search_meetings", status: "completed", label: "Searched meetings", resultCount: 1 } },
    { type: "delta", text: "We ship Monday [[S1]]." }, { type: "citations", text: "", citations: [citation] }, { type: "done", text: "" },
  ]
  let seen: ChatInput | null = null
  const stream = await Effect.runPromise(chats.stream(ids.alice, id, { turn: 0, message: "When do we ship?", includeSources: true }, (scope, input) => { seen = input; expect(scope).toBeNull(); return Effect.succeed(Stream.fromIterable(script)) }))
  await Effect.runPromise(Stream.runDrain(stream))
  expect(seen!.includeSources).toBe(true)
  const answer = (await Effect.runPromise(chats.get(ids.alice, id))).messages[1]!
  expect(answer).toMatchObject({ content: "We ship Monday [[S1]].", status: "complete", citations: [citation] })
  expect(answer.tools).toEqual([script[1]!.tool!])
  expect(answer.elapsedMs).toBeGreaterThanOrEqual(0)
})

test("the first answered exchange names the thread through the title model, once; a person's name is never replaced", async () => {
  const asked: Array<[string, string]> = []
  const f = fixture({ title: async (question, answer) => { asked.push([question, answer]); return "Regatta date decision" } })
  const id = (await f.json("alice", "/chats", "POST", {})).body.id
  expect((await f.json("alice", `/chats/${id}`)).body.title).toBeNull()
  await (await f.call("alice", `/chats/${id}/stream`, "POST", { turn: 0, message: "When is the regatta?" })).text()
  await f.chats.idle()
  expect((await f.json("alice", `/chats/${id}`)).body).toMatchObject({ title: "Regatta date decision", titleSource: "generated" })
  expect(asked).toEqual([["When is the regatta?", "Answer to When is the regatta?"]])
  await (await f.call("alice", `/chats/${id}/stream`, "POST", { turn: 1, message: "And the venue?" })).text()
  await f.chats.idle()
  expect(asked.length).toBe(1)
  // Renamed by the person, then the first exchange is asked again: their name stays.
  await f.json("alice", `/chats/${id}`, "PATCH", { title: "My regatta" })
  await (await f.call("alice", `/chats/${id}/stream`, "POST", { turn: 0, replace: true, message: "When exactly?" })).text()
  await f.chats.idle()
  expect((await f.json("alice", `/chats/${id}`)).body).toMatchObject({ title: "My regatta", titleSource: "user" })
  // A first exchange stored by append is named the same way.
  const appended = (await f.seed("alice", {}, [turn("Budget for Q3?", "Forty thousand.")])).id
  await f.chats.idle()
  expect((await f.json("alice", `/chats/${appended}`)).body.titleSource).toBe("generated")
})

test("without a usable title from the model a thread is named after its first question", async () => {
  const f = fixture({ title: async () => null })
  const id = (await f.json("alice", "/chats", "POST", {})).body.id
  await f.json("alice", `/chats/${id}/turns`, "POST", { turn: 0, ...turn("   What   are the action items from Tuesday's planning meeting with the design and research teams?  ", "Three.") })
  await f.chats.idle()
  const thread = (await f.json("alice", `/chats/${id}`)).body
  expect(thread.titleSource).toBe("question")
  expect(thread.title).toBe(questionTitle("What are the action items from Tuesday's planning meeting with the design and research teams?"))
  expect([...thread.title].length).toBeLessThanOrEqual(70)
  expect(thread.title.endsWith("…")).toBe(true)
})

test("deleting a note deletes the threads about it and leaves the others", async () => {
  const f = fixture()
  const note = await (await f.call("alice", "/notes", "POST", { title: "To be deleted" })).json() as { id: string }
  const about = (await f.seed("alice", { noteId: note.id }, [turn("About the note", "Quoted transcript")])).id
  const across = (await f.seed("alice", {}, [turn("Across notes", "Answer")])).id
  await (await f.call("alice", `/chats/${about}/stream`, "POST", { turn: 1, message: "Note-scoped follow-up" })).text()
  expect((await f.json("alice", `/chats/${about}`)).body.turnCount).toBe(2)
  expect((await f.call("alice", `/notes/${note.id}`, "DELETE")).status).toBe(200)
  expect((await f.json("alice", `/chats/${about}`)).status).toBe(404)
  expect((await f.json("alice", "/chats")).body.threads.map((t: { id: string }) => t.id)).toEqual([across])
  expect((await f.json("alice", `/chats/${across}`)).status).toBe(200)
})

test("the history sent with a question is the newest fifteen answered exchanges", () => {
  const at = new Date()
  const message = (position: number, content: string) => ({ id: crypto.randomUUID(), position, role: position % 2 ? "assistant" as const : "user" as const, content, status: "complete" as const,
    error: null, model: null, citations: [], tools: [], reasoning: [], elapsedMs: null, createdAt: at, updatedAt: at })
  const messages = Array.from({ length: 40 }, (_, i) => message(i, i === 5 ? "" : `m${i}`))
  const history = chatHistory(messages)
  expect(history.length).toBe(30)
  expect(history[0]).toEqual({ role: "user", content: "m10" })
  expect(history.at(-1)).toEqual({ role: "assistant", content: "m39" })
  expect(chatHistory(messages.slice(0, 8)).map(m => m.content)).toEqual(["m0", "m1", "m2", "m3", "m6", "m7"])
})
