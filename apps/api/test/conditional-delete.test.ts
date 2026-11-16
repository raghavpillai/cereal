import { afterEach, describe, expect, test } from "bun:test"
import type { NoteDTO } from "@meeting-notes/contracts"
import { importBody, postgresEnabled, stack, type Kind, type Stack } from "./import-support"
import { parseNoteStamp } from "../src/note-import"

/** `updatedAt|transcriptRevision|generatedRevision` as the apps build it; the last part is empty when nothing was generated. */
const stamp = (note: Pick<NoteDTO, "updatedAt" | "transcriptRevision" | "generatedRevision">) => `${note.updatedAt}|${note.transcriptRevision}|${note.generatedRevision ?? ""}`
const query = (parameters: { ifStamp?: string }) => "?" + Object.entries(parameters).map(([key, value]) => `${key}=${encodeURIComponent(String(value))}`).join("&")
const changed = { error: "conflict", message: "The note changed since it was read, so it was not deleted" }

test("a stamp is three parts and nothing else", () => {
  expect(parseNoteStamp("2026-11-15T12:00:00.000Z|3|2")).toEqual({ updatedAt: Date.parse("2026-11-15T12:00:00.000Z"), transcriptRevision: 3, generatedRevision: 2 })
  expect(parseNoteStamp("2026-11-15T12:00:00.000Z|0|")!.generatedRevision).toBeNull()
  expect(parseNoteStamp("2026-11-15T12:00:00.000Z|0|null")!.generatedRevision).toBeNull()
  expect(parseNoteStamp("2026-11-15T07:00:00.000-05:00|0|")!.updatedAt).toBe(Date.parse("2026-11-15T12:00:00.000Z"))
  for (const bad of ["", "garbage", "2026-11-15T12:00:00.000Z", "2026-11-15T12:00:00.000Z|1", "2026-11-15T12:00:00.000Z|1|2|3", "2026-11-15|1|2", "2026-11-15T12:00:00.000Z|-1|", "2026-11-15T12:00:00.000Z|1.5|", "2026-11-15T12:00:00.000Z|01|", "2026-11-15T12:00:00.000Z|1|x", "2026-13-45T12:00:00.000Z|1|", " 2026-11-15T12:00:00.000Z|1|", "2026-11-15T12:00:00.000Z|1| "])
    expect(parseNoteStamp(bad), JSON.stringify(bad)).toBeNull()
})

for (const kind of ["memory", "postgres"] as Kind[]) {
  describe.skipIf(kind === "postgres" && !postgresEnabled)(`conditional delete over HTTP (${kind})`, () => {
    const open: Stack[] = []
    afterEach(async () => { for (const s of open.splice(0)) await s.close() })
    const start = async (options?: Parameters<typeof stack>[1]) => { const s = await stack(kind, options); open.push(s); return s }
    const create = async (s: Stack, account: "alice" | "bob" = "alice") => (await s.call(account, "/notes", "POST", { title: "To move", body: "Typed notes" })).json as NoteDTO
    const read = async (s: Stack, id: string) => (await s.call("alice", `/notes/${id}`)).json as NoteDTO
    const exists = async (s: Stack, id: string) => (await s.call("alice", `/notes/${id}`)).status === 200

    test("matching stamp deletes", async () => {
      const s = await start(), note = await create(s)
      await s.record("alice", note.id, 0); await s.record("alice", note.id, 1)
      const current = await read(s, note.id)
      const deleted = await s.call("alice", `/notes/${note.id}${query({ ifStamp: stamp(current) })}`, "DELETE")
      expect([deleted.status, deleted.json]).toEqual([200, { deleted: true }])
      expect(await exists(s, note.id)).toBe(false)
      // Gone is gone: the same request again is a 404, which a move treats as done.
      expect((await s.call("alice", `/notes/${note.id}${query({ ifStamp: stamp(current) })}`, "DELETE")).status).toBe(404)
    })

    test("a note edited between read and delete answers 409 and keeps its transcript", async () => {
      const s = await start(), note = await create(s)
      await s.record("alice", note.id)
      const seen = await read(s, note.id)
      await Bun.sleep(5)
      expect((await s.call("alice", `/notes/${note.id}`, "PATCH", { body: "Typed on another device during the window" })).status).toBe(200)
      const refused = await s.call("alice", `/notes/${note.id}${query({ ifStamp: stamp(seen) })}`, "DELETE")
      expect([refused.status, refused.json]).toEqual([409, changed])
      const kept = await read(s, note.id)
      expect(kept.body).toBe("Typed on another device during the window")
      expect(kept.transcriptSegments).toHaveLength(1)
      // Every kind of change moves the stamp: the title, the transcript, generated notes.
      for (const change of [() => s.call("alice", `/notes/${note.id}`, "PATCH", { title: "Renamed" }), () => s.record("alice", note.id, 1), () => s.call("alice", `/notes/${note.id}/generate`, "POST", { expectedTranscriptRevision: 2 })]) {
        const before = await read(s, note.id)
        await Bun.sleep(5)
        await change()
        expect(stamp(await read(s, note.id))).not.toBe(stamp(before))
        expect((await s.call("alice", `/notes/${note.id}${query({ ifStamp: stamp(before) })}`, "DELETE")).status).toBe(409)
      }
      // With what is there now, it goes.
      const now = await read(s, note.id)
      expect(now.generatedRevision).toBe(2)
      expect((await s.call("alice", `/notes/${note.id}${query({ ifStamp: stamp(now) })}`, "DELETE")).status).toBe(200)
    })

    test("a recording note answers 409", async () => {
      const s = await start(), note = await create(s)
      expect((await s.call("alice", `/notes/${note.id}/recording`, "POST", { command: "start" })).status).toBe(200)
      const recording = await read(s, note.id)
      // The stamp is the current one and the count is right: it is refused because it is recording.
      for (const parameters of [{ ifStamp: stamp(recording) }])
        expect((await s.call("alice", `/notes/${note.id}${query(parameters)}`, "DELETE")).json).toEqual(changed)
      expect(await exists(s, note.id)).toBe(true)
      expect((await s.call("alice", `/notes/${note.id}/recording`, "POST", { command: "stop" })).status).toBe(200)
      expect((await s.call("alice", `/notes/${note.id}${query({ ifStamp: stamp(await read(s, note.id)) })}`, "DELETE")).status).toBe(200)
      // An imported note that was resumed here is a recording note like any other.
      const moved = importBody()
      expect((await s.call("alice", "/notes/import", "POST", moved)).status).toBe(200)
      expect((await s.call("alice", `/notes/${moved.id}/recording`, "POST", { command: "resume" })).status).toBe(200)
      expect((await s.call("alice", `/notes/${moved.id}${query({ ifStamp: stamp(await read(s, moved.id)) })}`, "DELETE")).status).toBe(409)
    })

    test("a precondition that cannot be read is a 400, never ignored", async () => {
      const s = await start(), note = await create(s)
      for (const bad of ["?ifStamp=garbage", "?ifStamp=", `?ifStamp=${encodeURIComponent(note.updatedAt)}`, `?ifStamp=${encodeURIComponent(note.updatedAt + "|0")}`, `?ifStamp=${encodeURIComponent(note.updatedAt + "|0||")}`, `?ifStamp=${"9".repeat(200)}`]) {
        expect((await s.call("alice", `/notes/${note.id}${bad}`, "DELETE")).status, bad).toBe(400)
        expect(await exists(s, note.id)).toBe(true)
      }
      // Both spellings of "nothing generated" are read; a number where nothing was generated is a mismatch.
      expect((await s.call("alice", `/notes/${note.id}${query({ ifStamp: `${note.updatedAt}|0|0` })}`, "DELETE")).status).toBe(409)
      expect((await s.call("alice", `/notes/${note.id}${query({ ifStamp: `${note.updatedAt}|1|` })}`, "DELETE")).status).toBe(409)
      const other = await create(s)
      expect((await s.call("alice", `/notes/${other.id}${query({ ifStamp: `${other.updatedAt}|0|null` })}`, "DELETE")).status).toBe(200)
      expect((await s.call("alice", `/notes/${note.id}${query({ ifStamp: `${note.updatedAt}|0|` })}`, "DELETE")).status).toBe(200)
    })

    test("somebody else's note, and a note that is not there, are 404 whatever the precondition", async () => {
      const s = await start(), note = await create(s)
      for (const parameters of [{ ifStamp: stamp(note) }, { ifStamp: "2020-01-01T00:00:00.000Z|0|" }]) {
        expect((await s.call("bob", `/notes/${note.id}${query(parameters)}`, "DELETE")).status).toBe(404)
        expect((await s.call("alice", `/notes/${crypto.randomUUID()}${query(parameters)}`, "DELETE")).status).toBe(404)
      }
      expect((await s.call(null, `/notes/${note.id}${query({ ifStamp: stamp(note) })}`, "DELETE")).status).toBe(401)
      expect(await exists(s, note.id)).toBe(true)
    })

    test("without the parameters a delete is what it was: unconditional", async () => {
      const s = await start(), note = await create(s)
      expect((await s.call("alice", `/notes/${note.id}/recording`, "POST", { command: "start" })).status).toBe(200)
      await s.record("alice", note.id, 0)
      const deleted = await s.call("alice", `/notes/${note.id}`, "DELETE")
      expect([deleted.status, deleted.json]).toEqual([200, { deleted: true }])
      expect((await s.call("alice", `/notes/${note.id}`, "DELETE")).status).toBe(404)
    })

    test("M6 an edit racing the delete: the edit is kept or it is refused, never accepted and then deleted", async () => {
      const s = await start()
      for (let round = 0; round < (kind === "postgres" ? 12 : 4); round++) {
        const note = await create(s)
        const [edit, removal] = await Promise.all([
          s.call("alice", `/notes/${note.id}`, "PATCH", { body: `Edit ${round}` }),
          s.call("alice", `/notes/${note.id}${query({ ifStamp: stamp(note) })}`, "DELETE"),
        ])
        const survived = await exists(s, note.id)
        if (removal.status === 200) { expect(edit.status).toBe(404); expect(survived).toBe(false) }
        else { expect([removal.status, edit.status]).toEqual([409, 200]); expect((await read(s, note.id)).body).toBe(`Edit ${round}`) }
      }
    })
  })
}
