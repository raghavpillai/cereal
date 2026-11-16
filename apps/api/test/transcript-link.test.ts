/** The "Chat with meeting transcript" line at the foot of generated notes (`src/transcript-link.ts`). */
import { afterEach, describe, expect, test } from "bun:test"
import { Effect } from "effect"
import type { PrincipalData } from "@meeting-notes/contracts/api"
import { makeApiHandler } from "../src/api"
import { BOOTSTRAP_OWNER_ID, makeMemoryRepository, type NotesRepositoryService } from "../src/repository"
import { makePrismaRepository } from "../src/prisma-repository"
import { makeProcessingService } from "../src/processing"
import { makeMemoryNoteAccessStore, makeNoteAccessService } from "../src/note-access"
import type { ProviderServices } from "../src/providers"
import { TRANSCRIPT_LINK_LABEL, rehomedTranscriptLink, transcriptChatURL, transcriptLinkLine, withTranscriptLink, withoutTranscriptLink } from "../src/transcript-link"
import { generationContext } from "../src/ai/notes-generation"

const BASE = "https://notes.example.test"
const segment = (text: string, startMs = 0) => ({ id: crypto.randomUUID(), channel: "mic" as const, speakerId: "mic", speakerLabel: null, text, isFinal: true, startMs, endMs: startMs + 4000 })
const count = (text: string, part: string) => text.split(part).length - 1

describe("the line", () => {
  test("is the note's chat page on the server's public origin", () => {
    const id = "3F0A1B7E-6F0E-4A55-9F7C-0D2C7F1F6A10"
    expect(transcriptChatURL("https://notes.example.test/", id)).toBe("https://notes.example.test/notes/3f0a1b7e-6f0e-4a55-9f7c-0d2c7f1f6a10/chat")
    expect(transcriptChatURL("http://10.30.1.77:3000", id)).toBe("http://10.30.1.77:3000/notes/3f0a1b7e-6f0e-4a55-9f7c-0d2c7f1f6a10/chat")
    expect(transcriptLinkLine(BASE, id)).toBe(`Chat with meeting transcript: ${BASE}/notes/3f0a1b7e-6f0e-4a55-9f7c-0d2c7f1f6a10/chat`)
  })

  test("goes after a rule at the end, once, whatever the model wrote", () => {
    const id = crypto.randomUUID(), line = transcriptLinkLine(BASE, id), url = transcriptChatURL(BASE, id)
    expect(withTranscriptLink({ noteId: id, generated: "# Notes\n- One\n", previous: "", remembered: null, line })).toEqual({ enhanced: `# Notes\n- One\n\n---\n\n${line}`, remembered: line })
    // A copy the model made from the typed notes, in any form, is not doubled.
    for (const copy of [line, `- ${line}`, `**Chat with meeting transcript:** ${url}`, `[Chat with meeting transcript](${url})`, `Chat with meeting transcript: [${url}](${url})`, `Chat with meeting transcript: <${url}>`, `See ${url} for the chat.`])
      expect(withTranscriptLink({ noteId: id, generated: `# Notes\n- One\n\n---\n\n${copy}\n`, previous: "", remembered: null, line }).enhanced, copy).toBe(copy.startsWith("See") ? `# Notes\n- One\n\n---\n\nSee for the chat.\n\n---\n\n${line}` : `# Notes\n- One\n\n---\n\n${line}`)
    // Nothing is added to notes that are empty, or where the server has no link to give.
    expect(withTranscriptLink({ noteId: id, generated: "  \n", previous: "", remembered: null, line })).toEqual({ enhanced: "  \n", remembered: null })
    expect(withTranscriptLink({ noteId: id, generated: "# Notes", previous: "", remembered: null, line: null })).toEqual({ enhanced: "# Notes", remembered: null })
    // Another note's chat link is ordinary text.
    const other = transcriptLinkLine(BASE, crypto.randomUUID())
    expect(withTranscriptLink({ noteId: id, generated: `# Notes\n- ${other}`, previous: "", remembered: null, line }).enhanced).toBe(`# Notes\n- ${other}\n\n---\n\n${line}`)
  })

  test("is replaced while it is unchanged, and stays out once edited or deleted, even when the model copies it", () => {
    const id = "3f0a1b7e-6f0e-4a55-9f7c-0d2c7f1f6a10", old = transcriptLinkLine("http://10.30.1.77:3000", id), line = transcriptLinkLine(BASE, id)
    // Unchanged (the person may have written elsewhere, even after it): the new notes get the current line.
    expect(withTranscriptLink({ noteId: id, generated: "B", previous: `A\n\n---\n\n${old}\nMy own words`, remembered: old, line })).toEqual({ enhanced: `B\n\n---\n\n${line}`, remembered: line })
    // Edited: typed after, or partly deleted. A copy from the typed notes does not bring it back.
    for (const previous of [`A\n\n---\n\n${old} I CAN TYPE HERE`, `A\n\n---\n\nChat with meeting transcript: ORD ELETE`, "A"])
      for (const generated of ["B", `B\n\n---\n\n${old}`, `B\n- **${line}**`])
        expect(withTranscriptLink({ noteId: id, generated, previous, remembered: old, line }), `${previous} / ${generated}`).toEqual({ enhanced: "B", remembered: old })
  })

  test("is read out of everything but the notes on screen", () => {
    const id = crypto.randomUUID(), line = transcriptLinkLine(BASE, id)
    expect(withoutTranscriptLink(id, `# Notes\n- One\n\n---\n\n${line}`)).toBe("# Notes\n- One")
    // Words typed after it stay; text without it is returned as it was.
    expect(withoutTranscriptLink(id, `# Notes\n\n---\n\n${line} I CAN TYPE HERE`)).toBe("# Notes\n\n---\n\nI CAN TYPE HERE")
    expect(withoutTranscriptLink(id, "# Notes\n\n---\n")).toBe("# Notes\n\n---\n")
    // The typed notes the model is given leave it out.
    expect(generationContext({ id, title: "", date: "2026-01-01", duration: "", body: `my notes\n${line}`, enhanced: "", transcriptSegments: [] } as never).userNotes).toBe("my notes")
  })

  test("a note moved here keeps an unedited link, pointed at this server", () => {
    const id = crypto.randomUUID(), line = transcriptLinkLine(BASE, id), old = transcriptLinkLine("https://old-server.example", id)
    expect(rehomedTranscriptLink(id, `A\n\n---\n\n${old}`, line)).toEqual({ enhanced: `A\n\n---\n\n${line}`, remembered: line })
    expect(rehomedTranscriptLink(id, `A\n\n---\n\n${old} edited`, line)).toEqual({ enhanced: `A\n\n---\n\n${old} edited`, remembered: null })
    expect(rehomedTranscriptLink(id, `A\n\n---\n\n${old}`, null)).toEqual({ enhanced: `A\n\n---\n\n${old}`, remembered: null })
  })
})

const ids = { alice: BOOTSTRAP_OWNER_ID, bob: "20000000-0000-4000-8000-0000000000b0" }
type Account = keyof typeof ids
const cleanup: Array<() => Promise<unknown>> = []
afterEach(async () => { for (const dispose of cleanup.splice(0).reverse()) await dispose() })

for (const kind of ["memory", "postgres"] as const) {
  const enabled = kind === "memory" || process.env.RUN_PG_TESTS === "1" && !!process.env.DATABASE_URL
  describe.skipIf(!enabled)(`${kind}: generating notes`, () => {
    /** The real HTTP layer and processing, with the server's link on, a model whose answers the test chooses,
     * and two accounts: Alice, who owns the note, and Bob, who does not. */
    function fixture() {
      let base = BASE
      const options = { transcriptLink: (noteId: string) => transcriptLinkLine(base, noteId) }
      const resources = kind === "postgres" ? makePrismaRepository(process.env.DATABASE_URL!, options) : null
      const repository: NotesRepositoryService = resources?.repository ?? makeMemoryRepository([], undefined, options)
      const answers: string[] = [], made: string[] = [], shown: string[] = []
      const providers: ProviderServices = {
        
        generate: () => Effect.sync(() => answers.shift() ?? "# Notes\n- Default"),
        chat: () => Effect.succeed("answer"),
        async *chatStream(note) { shown.push(note.enhanced); yield `Chatting with ${note.transcriptSegments.length} transcript lines` },
      }
      const processing = makeProcessingService(repository, { providers })
      const api = makeApiHandler(repository, "transcript-link-test-token-at-least-32-characters", {
        processing,
        noteAccess: makeNoteAccessService(makeMemoryNoteAccessStore(repository, { organization: { id: "40000000-0000-4000-8000-0000000000aa", name: "Acme" }, members: (Object.keys(ids) as Account[]).map(account => ({ userId: ids[account], name: account, email: `${account}@example.test` })) })),
        principalResolver: async headers => {
          const account = (headers.get("authorization") ?? "").replace("Bearer ", "") as Account
          if (!(account in ids)) return null
          return { id: ids[account], name: account, email: `${account}@example.test`, role: account === "alice" ? "owner" : "user", source: "session", expiresAt: "2099-01-01T00:00:00Z", createdAt: "2026-01-01T00:00:00Z" } satisfies PrincipalData
        },
      })
      cleanup.push(async () => {
        for (const id of made) await Effect.runPromise(repository.deleteNote(id))
        await api.dispose(); await resources?.close()
      })
      const call = (account: Account, path: string, method = "GET", body?: unknown) => api.handler(new Request(`http://localhost/api/v1${path}`, {
        method, headers: { Authorization: `Bearer ${account}`, ...(body === undefined ? {} : { "Content-Type": "application/json" }) }, ...(body === undefined ? {} : { body: JSON.stringify(body) }),
      }))
      const create = async () => {
        const note = await (await call("alice", "/notes", "POST", { title: "Supplier call", body: "", transcriptSegments: [segment("Windrush quoted six hundred and twelve pounds a tonne."), segment("Noemi sends the contract.", 5000)] })).json() as { id: string; transcriptRevision: number }
        made.push(note.id)
        return note
      }
      const generate = async (note: { id: string; transcriptRevision: number }, answer: string) => {
        answers.push(answer)
        const response = await call("alice", `/notes/${note.id}/generate`, "POST", { expectedTranscriptRevision: note.transcriptRevision })
        expect(response.status).toBe(200)
        return (await response.json() as { enhanced: string }).enhanced
      }
      const edit = async (id: string, enhanced: string) => expect((await call("alice", `/notes/${id}`, "PATCH", { enhanced })).status).toBe(200)
      return { call, create, generate, edit, shown, made, moveServer: (to: string) => { base = to }, database: resources?.database }
    }

    test("the line is added once, and a regeneration replaces it while it is unedited", async () => {
      const f = fixture(), note = await f.create(), line = transcriptLinkLine(BASE, note.id)
      const first = await f.generate(note, "# Pricing\n- Windrush at 612/t")
      expect(first).toBe(`# Pricing\n- Windrush at 612/t\n\n---\n\n${line}`)
      if (f.database) expect((await f.database.note.findUniqueOrThrow({ where: { id: note.id } })).transcriptLink).toBe(line)
      // Regenerated with the line as it was, and the person's own words elsewhere: one line, the new notes.
      await f.edit(note.id, `${first}\n\nA thought of my own`)
      const second = await f.generate(note, "# Pricing\n- Windrush at 612/t, from January")
      expect(second).toBe(`# Pricing\n- Windrush at 612/t, from January\n\n---\n\n${line}`)
      // A model that reproduces the line (pasted into the typed notes, say) still leaves one.
      const third = await f.generate(note, `# Pricing\n\n---\n\n${line}`)
      expect(count(third, TRANSCRIPT_LINK_LABEL)).toBe(1)
      expect(third).toBe(`# Pricing\n\n---\n\n${line}`)
      // The server moved: an unedited line follows it.
      f.moveServer("http://10.30.1.77:3000")
      expect(await f.generate(note, "# Pricing")).toBe(`# Pricing\n\n---\n\n${transcriptLinkLine("http://10.30.1.77:3000", note.id)}`)
    })

    test("a line the person edited or deleted is respected: a regeneration does not add it again", async () => {
      const f = fixture()
      for (const change of [(text: string) => `${text} I CAN TYPE HERE`, (text: string) => text.replace(/\n\n---\n\nChat with meeting transcript: .*$/, "\n\n---\n\nORD ELETE"), (text: string) => text.replace(/\n\n---\n\n.*$/, "")]) {
        const note = await f.create()
        const first = await f.generate(note, "# Pricing")
        await f.edit(note.id, change(first))
        expect(await f.generate(note, "# Pricing, again")).toBe("# Pricing, again")
        // And it stays out at every later generation.
        expect(await f.generate(note, "# Pricing, third time")).toBe("# Pricing, third time")
      }
    })

    test("search, list previews and the chat read the notes without the line", async () => {
      const f = fixture(), note = await f.create()
      await f.generate(note, "# Pricing\n- Windrush at 612/t")
      // The line's words and the server's address find nothing; the meeting's own words still do.
      for (const query of ["transcript", "example", "Chat with meeting transcript"]) {
        const found = await (await f.call("alice", `/notes/search?q=${encodeURIComponent(query)}`)).json() as { results: Array<{ note: { id: string } }> }
        expect(found.results.map(result => result.note.id), query).not.toContain(note.id)
      }
      const windrush = await (await f.call("alice", "/notes/search?q=Windrush")).json() as { results: Array<{ note: { id: string }; matchedIn: string[] }> }
      expect(windrush.results.find(result => result.note.id === note.id)?.matchedIn).toContain("summary")
      const list = await (await f.call("alice", "/notes/summaries")).json() as { notes: Array<{ id: string; excerpt: string }> }
      const row = list.notes.find(summary => summary.id === note.id)!
      expect(row.excerpt).toContain("Windrush")
      expect(row.excerpt).not.toContain(TRANSCRIPT_LINK_LABEL)
      // A heading-only note has nothing to preview but the line, so it previews nothing.
      const bare = await f.create()
      await f.generate(bare, "# Pricing")
      const again = await (await f.call("alice", "/notes/summaries")).json() as { notes: Array<{ id: string; excerpt: string }> }
      expect(again.notes.find(summary => summary.id === bare.id)!.excerpt).not.toContain("Chat with")
      // The model answering a question about the note is not shown the line.
      await (await f.call("alice", `/notes/${note.id}/chat/stream`, "POST", { message: "What did Windrush quote?", history: [] })).text()
      expect(f.shown.at(-1)).toBe("# Pricing\n- Windrush at 612/t")
    })

    test("a note moved from another server with its link unedited links here, and is regenerated as unedited", async () => {
      const f = fixture(), id = crypto.randomUUID(), old = transcriptLinkLine("https://old-server.example", id)
      const response = await f.call("alice", "/notes/import", "POST", { id, title: "Moved", body: "", enhanced: `# Moved\n\n---\n\n${old}`, date: "2026-11-16", duration: "0:10", createdAt: "2026-11-16T10:00:00.000Z", generated: true, recordingState: "stopped", transcriptSegments: [segment("Windrush quoted 612 a tonne.")], meeting: null })
      expect(response.status).toBe(200)
      f.made.push(id)
      const moved = await response.json() as { note: { enhanced: string; transcriptRevision: number } }
      expect(moved.note.enhanced).toBe(`# Moved\n\n---\n\n${transcriptLinkLine(BASE, id)}`)
      expect(await f.generate({ id, transcriptRevision: moved.note.transcriptRevision }, "# Again")).toBe(`# Again\n\n---\n\n${transcriptLinkLine(BASE, id)}`)
    })

    test("the link asks for the note's own access: its owner chats with the transcript, another account is told it does not exist", async () => {
      const f = fixture(), note = await f.create()
      const enhanced = await f.generate(note, "# Pricing")
      const url = new URL(enhanced.split(`${TRANSCRIPT_LINK_LABEL} `)[1]!)
      expect(url.pathname).toBe(`/notes/${note.id}/chat`)
      // What the chat page asks the server for: the note, and answers about it.
      expect((await f.call("alice", `/notes/${note.id}`)).status).toBe(200)
      const chat = await f.call("alice", `/notes/${note.id}/chat/stream`, "POST", { message: "What did Windrush quote?", history: [] })
      expect(chat.status).toBe(200)
      expect(await chat.text()).toContain("Chatting with 2 transcript lines")
      for (const [path, method, body] of [[`/notes/${note.id}`, "GET"], [`/notes/${note.id}/chat/stream`, "POST", { message: "What did Windrush quote?", history: [] }], [`/notes/${note.id}/chat`, "POST", { message: "What did Windrush quote?", history: [] }]] as const) {
        const response = await f.call("bob", path, method, body)
        expect(response.status, `${method} ${path}`).toBe(404)
        expect(await response.text()).not.toContain("Windrush")
      }
      // A shared snapshot leaves the line out (Bob could not use it), and sharing never made the transcript Bob's.
      const share = await (await f.call("alice", `/notes/${note.id}/shares`, "POST", { includeBody: false, includeEnhanced: true })).json() as { token: string }
      expect((await f.call("alice", `/notes/${note.id}/access`, "PATCH", { level: "organization" })).status).toBe(200)
      const snapshot = await (await f.call("bob", `/shared/${share.token}`)).json() as { enhanced: string }
      expect(snapshot.enhanced).toBe("# Pricing")
      expect(snapshot.enhanced).not.toContain(note.id)
      expect((await f.call("bob", `/notes/${note.id}/chat/stream`, "POST", { message: "Read the transcript", history: [] })).status).toBe(404)
    })
  })
}

test("a repository without the server's link saves generated notes as written", async () => {
  const repository = makeMemoryRepository()
  const note = await Effect.runPromise(repository.createNote({ title: "", date: "2026-01-01", transcriptSegments: [segment("Hello there.")] }))
  const run = crypto.randomUUID()
  await Effect.runPromise(repository.beginGeneration(note.id, note.transcriptRevision, run))
  expect((await Effect.runPromise(repository.finishGeneration(note.id, note.transcriptRevision, run, "# Notes")))!.enhanced).toBe("# Notes")
})
