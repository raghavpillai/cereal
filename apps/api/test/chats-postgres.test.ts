import { afterAll, beforeAll, describe, expect, test } from "bun:test"
import { Effect } from "effect"
import type { PrincipalData } from "@meeting-notes/contracts/api"
import { createDatabase } from "@meeting-notes/database"
import { makeApiHandler } from "../src/api"
import { makePrismaRepository } from "../src/prisma-repository"
import { makeProcessingService } from "../src/processing"
import { makeChatsService, makePrismaChatStore } from "../src/chats"

const url = process.env.DATABASE_URL
const enabled = process.env.RUN_PG_TESTS === "1" && !!url && /^\/mn_test_[a-f0-9]+$/.test(new URL(url).pathname)

describe.skipIf(!enabled)("saved chats in PostgreSQL", () => {
  const resources = enabled ? makePrismaRepository(url!) : null
  const db = enabled ? createDatabase(url!) : null
  const people = { alice: crypto.randomUUID(), bob: crypto.randomUUID(), admin: crypto.randomUUID(), gone: crypto.randomUUID() }
  type Person = keyof typeof people
  let now = Date.parse("2026-11-10T10:00:00.000Z")
  let release: (() => void) | null = null
  const chats = db ? makeChatsService(makePrismaChatStore(db), { clock: () => new Date(now), title: async () => "Generated chat title" }) : null
  const processing = resources ? makeProcessingService(resources.repository, { providers: {
    generate: () => Effect.succeed("generated"), chat: () => Effect.succeed("answer"),
    async *chatStream(_note, input, signal) {
      yield `Answer: ${input.message}`
      if (input.message.startsWith("hold")) await new Promise<void>(resolve => { release = resolve; signal.addEventListener("abort", () => resolve(), { once: true }) })
    },
  } }) : null
  const api = resources ? makeApiHandler(resources.repository, "chats-postgres-bootstrap-token-at-least-32-chars", {
    processing: processing!, chats: chats!,
    principalResolver: async headers => {
      const who = (headers.get("authorization") ?? "").replace("Bearer ", "") as Person
      if (!(who in people)) return null
      return { id: people[who], name: who, email: `${who}@example.invalid`, role: who === "admin" ? "admin" : "user", source: "session", expiresAt: "2099-01-01T00:00:00Z", createdAt: "2026-01-01T00:00:00Z" } satisfies PrincipalData
    },
  }) : null
  const call = (who: Person, path: string, method = "GET", body?: unknown) => api!.handler(new Request(`http://localhost/api/v1${path}`, {
    method, headers: { Authorization: `Bearer ${who}`, ...(body === undefined ? {} : { "Content-Type": "application/json" }) }, ...(body === undefined ? {} : { body: JSON.stringify(body) }) }))
  const json = async (who: Person, path: string, method = "GET", body?: unknown) => { const response = await call(who, path, method, body); return { status: response.status, body: await response.json().catch(() => null) as any } }
  const turn = (question: string, answer: string, status = "complete") => ({ question, answer: { content: answer, status } })
  const seed = async (who: Person, body: object, turns: ReadonlyArray<ReturnType<typeof turn>>) => {
    const id = (await json(who, "/chats", "POST", body)).body.id as string
    for (const [index, exchange] of turns.entries()) expect((await json(who, `/chats/${id}/turns`, "POST", { turn: index, ...exchange })).status).toBe(200)
    return id
  }
  beforeAll(async () => {
    await db!.user.createMany({ data: Object.entries(people).map(([name, id]) => ({ id, name, email: `chats-${id}@example.invalid` })) })
  })
  afterAll(async () => {
    await db?.user.deleteMany({ where: { id: { in: Object.values(people) } } })
    await api?.dispose()
    await resources?.close()
    await db?.$disconnect()
  })

  test("threads, messages and their order survive the database, and only their owner reaches them", async () => {
    const note = await (await call("alice", "/notes", "POST", { title: "Alice note in PostgreSQL" })).json() as { id: string }
    const id = await seed("alice", { noteId: note.id }, [turn("PG question", "PG_SECRET_ANSWER"), turn("Second", "Partial", "interrupted")])
    const read = (await json("alice", `/chats/${id}`)).body
    expect(read).toMatchObject({ noteId: note.id, turnCount: 2, createdAt: "2026-11-10T10:00:00.000Z", updatedAt: "2026-11-10T10:00:00.000Z" })
    expect(read.messages.map((m: any) => [m.turn, m.role, m.content, m.status])).toEqual([[0, "user", "PG question", "complete"], [0, "assistant", "PG_SECRET_ANSWER", "complete"], [1, "user", "Second", "complete"], [1, "assistant", "Partial", "interrupted"]])
    for (const who of ["bob", "admin"] as const) {
      for (const [path, method, body] of [[`/chats/${id}`, "GET"], [`/chats/${id}`, "PATCH", { title: "Mine" }], [`/chats/${id}`, "DELETE"], [`/chats/${id}/turns`, "POST", { turn: 2, ...turn("x", "y") }], [`/chats/${id}/stream`, "POST", { turn: 2, message: "x" }]] as const) {
        const answer = await json(who, path, method, body)
        expect({ who, path, method, status: answer.status }).toEqual({ who, path, method, status: 404 })
      }
      expect((await json(who, "/chats")).body.total).toBe(0)
      expect((await json(who, "/chats", "POST", { id })).status).toBe(409)
    }
    expect((await db!.chatMessage.count({ where: { threadId: id } }))).toBe(4)
    await chats!.idle()
    expect((await json("alice", `/chats/${id}`)).body).toMatchObject({ title: "Generated chat title", titleSource: "generated" })
  })

  test("pages are newest first by keyset, stable under equal times, with a total", async () => {
    const made: string[] = []
    for (let i = 0; i < 12; i++) {
      if (i % 3 === 0) now += 1000
      made.push((await json("bob", "/chats", "POST", { title: "Named" })).body.id)
    }
    const seen: string[] = []
    let cursor: string | null = null
    do {
      const page: any = (await json("bob", `/chats?limit=5${cursor ? `&cursor=${cursor}` : ""}`)).body
      expect(page.total).toBe(12)
      seen.push(...page.threads.map((t: { id: string }) => t.id))
      cursor = page.nextCursor
    } while (cursor)
    expect(new Set(seen).size).toBe(12)
    const rows = await db!.chatThread.findMany({ where: { userId: people.bob }, orderBy: [{ updatedAt: "desc" }, { id: "desc" }], select: { id: true } })
    expect(seen).toEqual(rows.map(row => row.id))
  })

  test("two requests for the same exchange: one is stored, the other is refused, never both", async () => {
    const id = (await json("bob", "/chats", "POST", {})).body.id
    const answers = await Promise.all([
      json("bob", `/chats/${id}/turns`, "POST", { turn: 0, ...turn("From the Mac", "Mac answer") }),
      json("bob", `/chats/${id}/turns`, "POST", { turn: 0, ...turn("From the iPhone", "iPhone answer") }),
    ])
    expect(answers.map(a => a.status).sort()).toEqual([200, 409])
    expect(await db!.chatMessage.count({ where: { threadId: id } })).toBe(2)
    // The same request twice at once (a retry racing the original): stored once.
    const again = await Promise.all([1, 2].map(() => json("bob", `/chats/${id}/turns`, "POST", { turn: 1, ...turn("Q2", "A2") })))
    expect(again.map(a => a.status)).toEqual([200, 200])
    expect(await db!.chatMessage.count({ where: { threadId: id } })).toBe(4)
    const twin = crypto.randomUUID()
    const creates = await Promise.all([1, 2, 3].map(() => json("bob", "/chats", "POST", { id: twin })))
    expect(creates.map(c => c.status)).toEqual([200, 200, 200])
    expect(await db!.chatThread.count({ where: { id: twin } })).toBe(1)
  })

  test("a streamed answer is written while it arrives and kept when the client leaves", async () => {
    const id = (await json("alice", "/chats", "POST", {})).body.id
    const done = await (await call("alice", `/chats/${id}/stream`, "POST", { turn: 0, message: "Finish" })).text()
    expect(done).toContain('"type":"done"')
    const response = await call("alice", `/chats/${id}/stream`, "POST", { turn: 1, message: "hold on" })
    const reader = response.body!.getReader()
    expect(new TextDecoder().decode((await reader.read()).value)).toContain("Answer: hold on")
    const during = await db!.chatMessage.findFirst({ where: { threadId: id, position: 3 } })
    expect(during?.status).toBe("streaming")
    await reader.cancel()
    let status = "streaming"
    for (let i = 0; i < 200 && status === "streaming"; i++) { await Bun.sleep(10); status = (await db!.chatMessage.findFirst({ where: { threadId: id, position: 3 } }))!.status }
    release?.()
    const saved = (await json("alice", `/chats/${id}`)).body
    expect(saved.messages.map((m: any) => [m.content, m.status])).toEqual([["Finish", "complete"], ["Answer: Finish", "complete"], ["hold on", "complete"], ["Answer: hold on", "interrupted"]])
  })

  test("a note's threads go with the note, and a person's threads go with the person", async () => {
    const note = await (await call("alice", "/notes", "POST", { title: "Deleted with its chats" })).json() as { id: string }
    const about = await seed("alice", { noteId: note.id }, [turn("About it", "Quote")])
    const across = await seed("alice", {}, [turn("Across", "All")])
    expect((await call("alice", `/notes/${note.id}`, "DELETE")).status).toBe(200)
    expect((await json("alice", `/chats/${about}`)).status).toBe(404)
    expect(await db!.chatMessage.count({ where: { threadId: about } })).toBe(0)
    expect((await json("alice", `/chats/${across}`)).status).toBe(200)
    const theirs = await seed("gone", {}, [turn("Mine", "Mine")])
    await db!.user.delete({ where: { id: people.gone } })
    expect(await db!.chatThread.count({ where: { id: theirs } })).toBe(0)
    expect(await db!.chatMessage.count({ where: { threadId: theirs } })).toBe(0)
  })
})
