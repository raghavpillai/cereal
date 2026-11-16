import { afterEach, describe, expect, test } from "bun:test"
import { createHash } from "node:crypto"
import { Schema } from "effect"
import { ImportedNoteDTO, NoteDTO, type ImportNoteInput } from "@meeting-notes/contracts"
import { importBody, meeting, participant, postgresEnabled, segment, stack, type Kind, type Stack } from "./import-support"
import { canonicalJson, uuidV5 } from "../src/note-import"

/** RFC 4122 version 5, written out again here so the server's own function is not what checks the server. */
export function expectedSegmentId(noteId: string, requestSegmentId: string): string {
  const hash = createHash("sha1").update(Buffer.from(noteId.toLowerCase().replaceAll("-", ""), "hex")).update(requestSegmentId.toLowerCase()).digest()
  hash[6] = (hash[6]! & 0x0f) | 0x50; hash[8] = (hash[8]! & 0x3f) | 0x80
  const hex = hash.subarray(0, 16).toString("hex")
  return [hex.slice(0, 8), hex.slice(8, 12), hex.slice(12, 16), hex.slice(16, 20), hex.slice(20)].join("-")
}
const conflict = '{"error":"conflict","message":"A note with this id already exists"}'
const feedNow = new Date("2026-11-15T12:00:00Z"), range = "from=2026-11-10T00:00:00Z&to=2026-11-30T00:00:00Z"
const feed = (uid: string, title: string, start = "20261118T150000Z", end = "20261118T160000Z") => `BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nDTSTAMP:20261115T000000Z\r\nDTSTART:${start}\r\nDTEND:${end}\r\nUID:${uid}\r\nSUMMARY:${title}\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n`
/** The fields a move compares (design 5.5), with the segments as a map keyed by id: order is not compared. */
const compared = (note: NoteDTO) => ({
  title: note.title, body: note.body, enhanced: note.enhanced, date: note.date, duration: note.duration, generated: note.generatedRevision !== null, recordingState: note.recordingState,
  segments: Object.fromEntries(note.transcriptSegments.map(({ id, ...rest }) => [id, rest])),
})
const expected = (body: ImportNoteInput) => ({
  title: body.title, body: body.body, enhanced: body.enhanced, date: body.date, duration: body.duration, generated: body.generated, recordingState: body.recordingState,
  segments: Object.fromEntries(body.transcriptSegments.map(({ id, ...rest }) => [expectedSegmentId(body.id, id), rest])),
})

/** The server tests of design 5.9, each through HTTP, against the memory adapters or PostgreSQL. */
export function importScenarios(kind: Kind) {
  describe.skipIf(kind === "postgres" && !postgresEnabled)(`note import over HTTP (${kind})`, () => {
    const open: Stack[] = []
    afterEach(async () => { for (const s of open.splice(0)) await s.close() })
    const start = async (options?: Parameters<typeof stack>[1]) => { const s = await stack(kind, options); open.push(s); return s }

    test("uuid v5 is the RFC 4122 one", () => {
      expect(uuidV5("6ba7b810-9dad-11d1-80b4-00c04fd430c8", "www.example.com")).toBe("2ed6657d-e927-568b-95e1-2665a8aea6a2")
      expect(expectedSegmentId("6ba7b810-9dad-11d1-80b4-00c04fd430c8", "www.example.com")).toBe("2ed6657d-e927-568b-95e1-2665a8aea6a2")
      expect(canonicalJson({ b: 1, a: [{ d: null, c: "x" }] })).toBe('{"a":[{"c":"x","d":null}],"b":1}')
    })

    test("M1 import then read: every field equal; ids are uuid v5; importedAt set", async () => {
      const s = await start(), body = importBody()
      const before = Date.now(), imported = await s.call("alice", "/notes/import", "POST", body)
      expect(imported.status).toBe(200)
      const answer = Schema.decodeUnknownSync(ImportedNoteDTO)(imported.json)
      expect(answer.note.id).toBe(body.id)
      expect(answer.segmentIds).toEqual(body.transcriptSegments.map(item => ({ from: item.id, to: expectedSegmentId(body.id, item.id) })))
      const read = await s.call("alice", `/notes/${body.id}`)
      expect(read.status).toBe(200)
      const note = Schema.decodeUnknownSync(NoteDTO)(read.json)
      expect(read.json).toEqual(imported.json.note)
      expect(compared(note)).toEqual(expected(body))
      expect(note.createdAt).toBe(body.createdAt)
      // Marked as imported, with the time of the import; the same clock reading as `updatedAt`.
      expect(note.importedAt).toBe(note.updatedAt)
      expect(Date.parse(note.importedAt!)).toBeGreaterThanOrEqual(before - 1000)
      expect([note.transcriptRevision, note.generatedRevision, note.generationState]).toEqual([1, 1, "ready"])
      // The meeting details and attendees arrive exactly, under an event id that is not the request's.
      const context = await s.call("alice", `/notes/${body.id}/context`)
      expect(context.status).toBe(200)
      expect({ ...context.json.meeting, eventId: body.meeting!.eventId }).toEqual(body.meeting!)
      expect(context.json.meeting.eventId).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/)
      expect(context.json.meeting.eventId).not.toBe(body.meeting!.eventId)
      // An app built before `importedAt` existed reads the same note.
      const { importedAt: _importedAt, ...olderFields } = NoteDTO.fields
      expect(Schema.decodeUnknownSync(Schema.Struct(olderFields))(read.json).id).toBe(body.id)
      // Nobody else can see any of it.
      for (const path of [`/notes/${body.id}`, `/notes/${body.id}/context`]) expect((await s.call("bob", path)).status).toBe(404)
      // A note made here the ordinary way says nothing about an import.
      const plain = await s.call("alice", "/notes", "POST", { title: "Made here" })
      expect("importedAt" in plain.json).toBe(false)
    })

    test("ids sent in upper case (as Swift writes them) give the same note and the same map", async () => {
      const s = await start(), lower = importBody()
      const upper = { ...lower, id: lower.id.toUpperCase(), transcriptSegments: lower.transcriptSegments.map(item => ({ ...item, id: item.id.toUpperCase() })) }
      const imported = await s.call("alice", "/notes/import", "POST", upper)
      expect(imported.status).toBe(200)
      expect(imported.json.note.id).toBe(lower.id)
      expect(imported.json.segmentIds).toEqual(upper.transcriptSegments.map(item => ({ from: item.id, to: expectedSegmentId(lower.id, item.id.toLowerCase()) })))
      expect(compared(imported.json.note)).toEqual(expected(lower))
      // The same segment twice, once in each case, is still a duplicate.
      const twice = importBody({ transcriptSegments: [lower.transcriptSegments[0]!, { ...lower.transcriptSegments[0]!, id: lower.transcriptSegments[0]!.id.toUpperCase() }] })
      expect((await s.call("alice", "/notes/import", "POST", twice)).status).toBe(400)
    })

    test("X1 cross-server shape: a context whose eventId does not exist here imports, reads back with attendees, and the list row shows them", async () => {
      const s = await start(), body = importBody({ title: "Moved from another server" })
      expect((await s.call("alice", "/notes/import", "POST", body)).status).toBe(200)
      const context = await s.call("alice", `/notes/${body.id}/context`)
      expect(context.json.meeting.participants.map((person: { displayName: string }) => person.displayName)).toEqual(["Raghav", "Ada Lovelace", "Grace Hopper", "Room 4"])
      expect(context.json.meeting.organizer.email).toBe("raghav@acme.test")
      expect([context.json.meeting.startAt, context.json.meeting.endAt, context.json.meeting.title]).toEqual(["2026-11-12T15:00:00.000Z", "2026-11-12T16:00:00.000Z", "Q4 planning"])
      const list = await s.call("alice", "/notes/summaries")
      expect(list.status).toBe(200)
      const row = list.json.notes.find((note: { id: string }) => note.id === body.id)
      // Three invited people (the room is not one); two of them are not the account holder.
      expect(row.attendees.total).toBe(3)
      expect(row.attendees.others).toBe(2)
      expect(row.attendees.people.map((person: { name: string }) => person.name)).toEqual(["Ada Lovelace", "Grace Hopper"])
      expect(row.meeting).toEqual({ startAt: "2026-11-12T15:00:00.000Z", endAt: "2026-11-12T16:00:00.000Z", allDay: false })
      // No calendar event of this server reports the note.
      const events = await s.call("alice", `/calendar/events?${range}`)
      expect(events.json.some((event: { noteId: string | null }) => event.noteId === body.id)).toBe(false)
    })

    test("X1 same-server shape: a context whose eventId exists here creates no CalendarMeeting row, raises no conflict when the user already has a note for that meeting, and the calendar event still reports its own note", async () => {
      const s = await start(), [event] = await s.calendar(feed(`same-${crypto.randomUUID()}@example.test`, "Team planning"), feedNow)
      // Alice already has a note for the meeting on this server.
      const own = await s.call("alice", `/calendar/events/${event!.id}/note`, "POST")
      expect(own.status).toBe(200)
      const reports = async () => (await s.call("alice", `/calendar/events?${range}`)).json.find((listed: { id: string }) => listed.id === event!.id).noteId
      expect(await reports()).toBe(own.json.id)
      // The imported context names that very event (a move between two accounts of one server, or a forged request).
      const body = importBody({ meeting: meeting({ eventId: event!.id, provider: "ics", title: "Team planning" }) })
      const imported = await s.call("alice", "/notes/import", "POST", body)
      expect(imported.status).toBe(200)
      // Nothing was bound and nothing was taken: the event still reports the note it had.
      expect(await reports()).toBe(own.json.id)
      const context = await s.call("alice", `/notes/${body.id}/context`)
      expect(context.json.meeting.eventId).not.toBe(event!.id)
      expect(context.json.meeting.participants).toHaveLength(4)
      if (s.db) {
        expect(await s.db.calendarMeeting.count({ where: { noteId: body.id } })).toBe(0)
        expect(await s.db.googleMeeting.count({ where: { noteId: body.id } })).toBe(0)
        expect(await s.db.calendarMeeting.count({ where: { userId: s.ids.alice } })).toBe(1)
      }
      // Bob, who has no note for the meeting, does not acquire a binding by importing either.
      const bobs = importBody({ meeting: meeting({ eventId: event!.id, provider: "ics" }) })
      expect((await s.call("bob", "/notes/import", "POST", bobs)).status).toBe(200)
      expect((await s.call("bob", `/calendar/events?${range}`)).json.find((listed: { id: string }) => listed.id === event!.id).noteId).toBeNull()
      // The note Alice had is untouched, with its own context.
      expect((await s.call("alice", `/notes/${own.json.id}/context`)).json.meeting.eventId).toBe(event!.id)
    })

    test("X1 an attendee can be added to an imported note; a real event can be attached afterwards", async () => {
      const s = await start(), body = importBody(), [event] = await s.calendar(feed(`attach-${crypto.randomUUID()}@example.test`, "Q4 planning"), feedNow)
      expect((await s.call("alice", "/notes/import", "POST", body)).status).toBe(200)
      const detached = (await s.call("alice", `/notes/${body.id}/context`)).json.meeting.eventId
      const added = await s.call("alice", `/notes/${body.id}/context/participants`, "POST", { id: crypto.randomUUID(), displayName: "Katherine Johnson", email: "katherine@acme.test" })
      expect(added.status).toBe(200)
      expect(added.json.meeting.participants.map((person: { displayName: string }) => person.displayName)).toEqual(["Raghav", "Ada Lovelace", "Grace Hopper", "Room 4", "Katherine Johnson"])
      // Still detached: the same event id, bound to nothing.
      expect(added.json.meeting.eventId).toBe(detached)
      expect((await s.call("alice", `/notes/${body.id}/context`)).json).toEqual(added.json)
      expect((await s.call("alice", `/calendar/events?${range}`)).json.some((listed: { noteId: string | null }) => listed.noteId === body.id)).toBe(false)
      expect((await s.call("alice", "/notes/summaries")).json.notes.find((note: { id: string }) => note.id === body.id).attendees.total).toBe(4)
      // Somebody else cannot add to it.
      expect((await s.call("bob", `/notes/${body.id}/context/participants`, "POST", { id: crypto.randomUUID(), displayName: "Mallory" })).status).toBe(404)
      // The person ticked "attach": a real event of this server replaces the detached context and binds.
      const attached = await s.call("alice", `/notes/${body.id}/context`, "PUT", { eventId: event!.id })
      expect(attached.status).toBe(200)
      expect(attached.json.meeting.eventId).toBe(event!.id)
      expect((await s.call("alice", `/calendar/events?${range}`)).json.find((listed: { id: string }) => listed.id === event!.id).noteId).toBe(body.id)
      // The detached event id names nothing: it cannot be "attached".
      const other = importBody()
      expect((await s.call("alice", "/notes/import", "POST", other)).status).toBe(200)
      const stray = (await s.call("alice", `/notes/${other.id}/context`)).json.meeting.eventId
      expect((await s.call("alice", `/notes/${other.id}/context`, "PUT", { eventId: stray })).status).toBe(404)
      expect((await s.call("alice", `/notes/${other.id}/context`)).json.meeting.eventId).toBe(stray)
    })

    test("X2 replay with the same body: 200, same note, same map, nothing written; after an edit: 409; after deletion: creates; another body: 409; another user's id: the same 409 body", async () => {
      const s = await start(), body = importBody()
      const first = await s.send("alice", "/notes/import", "POST", body), firstText = await first.text()
      expect(first.status).toBe(200)
      const context = (await s.call("alice", `/notes/${body.id}/context`)).text
      const counts = async () => s.db ? [await s.db.note.count({ where: { ownerId: s.ids.alice } }), await s.db.transcriptSegment.count({ where: { noteId: body.id } }), await s.db.noteMeetingContext.count({ where: { noteId: body.id } })] : [(await s.call("alice", "/notes")).json.length]
      const written = await counts()
      // The same request again (the first answer was lost): the same bytes back, and nothing new.
      for (let attempt = 0; attempt < 3; attempt++) {
        const again = await s.send("alice", "/notes/import", "POST", body)
        expect(again.status).toBe(200)
        expect(await again.text()).toBe(firstText)
      }
      // The same request with its keys in another order and other white space is the same request.
      const reordered = JSON.stringify(Object.fromEntries(Object.entries(body).reverse()), null, 2)
      expect(reordered).not.toBe(JSON.stringify(body))
      expect((await s.call("alice", "/notes/import", "POST", reordered)).text).toBe(firstText)
      expect(await counts()).toEqual(written)
      expect((await s.call("alice", `/notes/${body.id}/context`)).text).toBe(context)
      expect((await s.call("alice", `/notes/${body.id}`)).json.updatedAt).toBe(JSON.parse(firstText).note.updatedAt)

      // Another body with the same id, however small the difference.
      for (const changed of [{ ...body, title: "Q4 planning!" }, { ...body, generated: false }, { ...body, transcriptSegments: body.transcriptSegments.slice(1) }, { ...body, meeting: null }, { ...body, meeting: { ...body.meeting!, title: "Other" } }, { ...body, createdAt: "2026-11-12T15:01:02.346Z" }]) {
        const refused = await s.call("alice", "/notes/import", "POST", changed)
        expect([refused.status, refused.text]).toEqual([409, conflict])
      }
      // Somebody else's id: the identical answer, for the same body and for any other.
      for (const attempt of [body, importBody({ id: body.id })]) {
        const refused = await s.call("bob", "/notes/import", "POST", attempt)
        expect([refused.status, refused.text]).toEqual([409, conflict])
      }
      // An id nobody has is simply a new note for Bob.
      expect((await s.call("bob", "/notes/import", "POST", importBody())).status).toBe(200)
      expect((await s.call("alice", `/notes/${body.id}`)).text).toBe(JSON.stringify(JSON.parse(firstText).note))

      // Edited since the import: the replay is refused, and the edit stays.
      await Bun.sleep(5)
      const edited = await s.call("alice", `/notes/${body.id}`, "PATCH", { body: "Edited after the move" })
      expect(edited.status).toBe(200)
      const afterEdit = await s.call("alice", "/notes/import", "POST", body)
      expect([afterEdit.status, afterEdit.text]).toEqual([409, conflict])
      expect((await s.call("alice", `/notes/${body.id}`)).json.body).toBe("Edited after the move")

      // Deleted: the same request creates it again, as it did the first time.
      expect((await s.call("alice", `/notes/${body.id}`, "DELETE")).status).toBe(200)
      const recreated = await s.call("alice", "/notes/import", "POST", body)
      expect(recreated.status).toBe(200)
      expect(recreated.json.segmentIds).toEqual(JSON.parse(firstText).segmentIds)
      expect(compared(recreated.json.note)).toEqual(expected(body))
      expect((await s.call("alice", `/notes/${body.id}/context`)).json.meeting.participants).toHaveLength(4)
    })

    test("X2 a replay is refused once the note is recording again, was stopped, or its notes were generated again", async () => {
      const s = await start()
      const resumed = importBody()
      expect((await s.call("alice", "/notes/import", "POST", resumed)).status).toBe(200)
      await Bun.sleep(5)
      expect((await s.call("alice", `/notes/${resumed.id}/recording`, "POST", { command: "resume" })).status).toBe(200)
      expect((await s.call("alice", "/notes/import", "POST", resumed)).text).toBe(conflict)
      expect((await s.call("alice", `/notes/${resumed.id}/recording`, "POST", { command: "stop" })).status).toBe(200)
      expect((await s.call("alice", "/notes/import", "POST", resumed)).text).toBe(conflict)
      const regenerated = importBody()
      expect((await s.call("alice", "/notes/import", "POST", regenerated)).status).toBe(200)
      await Bun.sleep(5)
      expect((await s.call("alice", `/notes/${regenerated.id}/generate`, "POST", { expectedTranscriptRevision: 1 })).status).toBe(200)
      expect((await s.call("alice", "/notes/import", "POST", regenerated)).text).toBe(conflict)
    })

    test("X2 other people's segment ids are neither consulted nor revealed: a request that names them imports, and theirs are untouched", async () => {
      const s = await start()
      const theirs = await s.call("bob", "/notes", "POST", { title: "Bob's", transcriptSegments: [segment(0, "Bob said this."), segment(1, "And this.")] })
      expect(theirs.status).toBe(200)
      const taken = theirs.json.transcriptSegments.map((item: { id: string }) => item.id)
      const body = importBody({ transcriptSegments: [{ ...segment(0, "Alice's line."), id: taken[0] }, { ...segment(1, "Another."), id: taken[1] }] })
      const imported = await s.call("alice", "/notes/import", "POST", body)
      expect(imported.status).toBe(200)
      expect(imported.json.segmentIds.map((entry: { to: string }) => entry.to)).toEqual(taken.map((id: string) => expectedSegmentId(body.id, id)))
      expect(imported.json.note.transcriptSegments.map((item: { id: string }) => item.id).some((id: string) => taken.includes(id))).toBe(false)
      expect((await s.call("bob", `/notes/${theirs.json.id}`)).json.transcriptSegments).toEqual(theirs.json.transcriptSegments)
      // The same source segments under another note id are other ids again: two imports never meet.
      const second = importBody({ transcriptSegments: body.transcriptSegments })
      const again = await s.call("alice", "/notes/import", "POST", second)
      expect(again.status).toBe(200)
      expect(again.json.segmentIds.map((entry: { to: string }) => entry.to)).not.toEqual(imported.json.segmentIds.map((entry: { to: string }) => entry.to))
    })

    test("X3 createdAt in 1970 is clamped to 2000; four minutes ahead is kept; an hour ahead becomes now + 5 minutes", async () => {
      const now = new Date("2026-11-15T12:00:00.000Z"), s = await start({ clock: () => now })
      const created = async (createdAt: string) => {
        const body = importBody({ createdAt }), imported = await s.call("alice", "/notes/import", "POST", body)
        expect(imported.status).toBe(200)
        expect((await s.call("alice", `/notes/${body.id}`)).json.createdAt).toBe(imported.json.note.createdAt)
        return imported.json.note.createdAt
      }
      expect(await created("1970-01-01T00:00:00.000Z")).toBe("2000-01-01T00:00:00.000Z")
      expect(await created("1999-12-31T23:59:59.999Z")).toBe("2000-01-01T00:00:00.000Z")
      expect(await created("2000-01-01T00:00:00.000Z")).toBe("2000-01-01T00:00:00.000Z")
      expect(await created("2026-11-15T12:04:00.000Z")).toBe("2026-11-15T12:04:00.000Z")
      expect(await created("2026-11-15T12:05:00.000Z")).toBe("2026-11-15T12:05:00.000Z")
      expect(await created("2026-11-15T12:05:00.001Z")).toBe("2026-11-15T12:05:00.000Z")
      expect(await created("2026-11-15T13:00:00.000Z")).toBe("2026-11-15T12:05:00.000Z")
      expect(await created("9999-12-31T23:59:59.999Z")).toBe("2026-11-15T12:05:00.000Z")
      // An ordinary past time is carried exactly; another offset or precision is the same instant, written the server's way.
      expect(await created("2021-03-04T05:06:07.089Z")).toBe("2021-03-04T05:06:07.089Z")
      expect(await created("2021-03-04T07:06:07.089123+02:00")).toBe("2021-03-04T05:06:07.089Z")
      // What a clamped note says about itself: imported now, so a backdated note is recognisable.
      const body = importBody({ createdAt: "1970-01-01T00:00:00.000Z" }), imported = await s.call("alice", "/notes/import", "POST", body)
      expect(imported.json.note.importedAt).toBe(now.toISOString())
      // Never refused for its time: only a value that is not a timestamp at all is a 400.
      expect((await s.call("alice", "/notes/import", "POST", { ...importBody(), createdAt: "yesterday" })).status).toBe(400)
    })

    test("generated: true reads as generated; recordingState stopped allows resume and refuses start", async () => {
      const s = await start()
      const stopped = importBody({ generated: true, recordingState: "stopped" })
      expect((await s.call("alice", "/notes/import", "POST", stopped)).status).toBe(200)
      const note = (await s.call("alice", `/notes/${stopped.id}`)).json
      expect([note.generatedRevision, note.transcriptRevision, note.generationState, note.recordingState]).toEqual([1, 1, "ready", "stopped"])
      const row = (await s.call("alice", "/notes/summaries")).json.notes.find((listed: { id: string }) => listed.id === stopped.id)
      expect([row.generatedRevision, row.hasSummary, row.hasTranscript, row.recordingState]).toEqual([1, true, true, "stopped"])
      expect((await s.call("alice", `/notes/${stopped.id}/recording`, "POST", { command: "start" })).status).not.toBe(200)
      expect((await s.call("alice", `/notes/${stopped.id}`)).json.recordingState).toBe("stopped")
      const resumed = await s.call("alice", `/notes/${stopped.id}/recording`, "POST", { command: "resume" })
      expect([resumed.status, resumed.json.recordingState]).toEqual([200, "recording"])
      // Not generated, never recorded: idle, and a recording can start.
      const idle = importBody({ generated: false, recordingState: "idle", enhanced: "" })
      expect((await s.call("alice", "/notes/import", "POST", idle)).status).toBe(200)
      const fresh = (await s.call("alice", `/notes/${idle.id}`)).json
      expect([fresh.generatedRevision, fresh.generationState, fresh.recordingState]).toEqual([null, "idle", "idle"])
      expect((await s.call("alice", `/notes/${idle.id}/recording`, "POST", { command: "resume" })).status).not.toBe(200)
      expect((await s.call("alice", `/notes/${idle.id}/recording`, "POST", { command: "start" })).json.recordingState).toBe("recording")
      // No transcript at all: revision 0, and "generated" is recorded against it.
      const empty = importBody({ transcriptSegments: [], meeting: null })
      const imported = await s.call("alice", "/notes/import", "POST", empty)
      expect([imported.json.note.transcriptRevision, imported.json.note.generatedRevision, imported.json.segmentIds]).toEqual([0, 0, []])
      expect((await s.call("alice", `/notes/${empty.id}/context`)).json).toEqual({ noteId: empty.id, meeting: null })
    })

    test("what the schema refuses is a 400 and creates nothing", async () => {
      const s = await start(), good = importBody()
      const bad: Array<[string, unknown]> = [
        ["a recording state", { ...good, recordingState: "recording" }],
        ["no id", (({ id: _id, ...rest }) => rest)(good)],
        ["an id that is not a UUID", { ...good, id: "note-1" }],
        ["no createdAt", (({ createdAt: _createdAt, ...rest }) => rest)(good)],
        ["no meeting key", (({ meeting: _meeting, ...rest }) => rest)(good)],
        ["duplicate segment ids", { ...good, transcriptSegments: [good.transcriptSegments[0], good.transcriptSegments[0]] }],
        ["a segment that ends before it starts", { ...good, transcriptSegments: [{ ...good.transcriptSegments[0], startMs: 10, endMs: 5 }] }],
        ["501 attendees", { ...good, meeting: meeting({ participants: Array.from({ length: 501 }, (_, index) => participant(`Person ${index}`, null)) }) }],
        ["a provider nobody knows", { ...good, meeting: { ...good.meeting, provider: "outlook" } }],
        ["a title of 501 characters", { ...good, title: "t".repeat(501) }],
        ["a body that is not an object", [good]],
      ]
      for (const [what, body] of bad) expect((await s.call("alice", "/notes/import", "POST", body)).status, what).toBe(400)
      expect((await s.call("alice", "/notes/import", "POST", "{not json")).status).toBe(400)
      expect((await s.call("alice", `/notes/${good.id}`)).status).toBe(404)
      // 500 attendees are allowed.
      const full = importBody({ meeting: meeting({ participants: Array.from({ length: 500 }, (_, index) => participant(`Person ${index}`, `p${index}@acme.test`)) }) })
      expect((await s.call("alice", "/notes/import", "POST", full)).status).toBe(200)
      expect((await s.call("alice", `/notes/${full.id}/context`)).json.meeting.participants).toHaveLength(500)
      // Not signed in: 401 before anything is read.
      expect((await s.call(null, "/notes/import", "POST", good)).status).toBe(401)
      expect((await s.call("alice", `/notes/${good.id}`)).status).toBe(404)
    })

    test("50,000 segments at about 24 MB are accepted within the timeout; 50,001 are refused; over 24 MB is a 413", async () => {
      const s = await start()
      // 50,000 segments and a little under 24 MB of JSON: the largest note the apps will move (design 5.4).
      const body = importBody({ transcriptSegments: Array.from({ length: 50_000 }, (_, index) => segment(index, "x".repeat(331))) })
      const json = JSON.stringify(body)
      expect(json.length).toBeGreaterThan(23.9 * 1024 * 1024)
      expect(json.length).toBeLessThanOrEqual(24 * 1024 * 1024)
      const began = performance.now(), imported = await s.call("alice", "/notes/import", "POST", json), took = performance.now() - began
      expect(imported.status).toBe(200)
      expect(took).toBeLessThan(60_000)
      expect(imported.json.segmentIds).toHaveLength(50_000)
      expect(imported.json.segmentIds[49_999]).toEqual({ from: body.transcriptSegments[49_999]!.id, to: expectedSegmentId(body.id, body.transcriptSegments[49_999]!.id) })
      const read = await s.call("alice", `/notes/${body.id}`)
      expect(read.status).toBe(200)
      expect(read.json.transcriptSegments).toHaveLength(50_000)
      expect(compared(read.json)).toEqual(expected(body))
      // The same request again finds the note and answers with the same map.
      const again = await s.call("alice", "/notes/import", "POST", json)
      expect(again.status).toBe(200)
      expect(again.json.segmentIds).toEqual(imported.json.segmentIds)
      if (s.db) expect(await s.db.transcriptSegment.count({ where: { noteId: body.id } })).toBe(50_000)

      // One segment more is refused by the schema, whatever its size.
      const tooMany = importBody({ transcriptSegments: Array.from({ length: 50_001 }, (_, index) => segment(index, "x")) })
      expect((await s.call("alice", "/notes/import", "POST", tooMany)).status).toBe(400)
      expect((await s.call("alice", `/notes/${tooMany.id}`)).status).toBe(404)

      // Over 24 MB: refused in JSON before it is parsed, by the declared length and by what actually arrives.
      const large = importBody({ body: "", transcriptSegments: Array.from({ length: 50_000 }, (_, index) => segment(index, "x".repeat(340))) }), oversized = JSON.stringify(large)
      expect(oversized.length).toBeGreaterThan(24 * 1024 * 1024)
      const undeclared = await s.call("alice", "/notes/import", "POST", oversized)
      expect([undeclared.status, undeclared.json]).toEqual([413, { error: "payload_too_large", message: "A note import is limited to 24 MB" }])
      // A request that declares its length, as every real client does, is answered from the header alone.
      const byLength = await s.sendRaw(new Request("http://localhost/api/v1/notes/import", { method: "POST", headers: { Authorization: "Bearer alice", "Content-Type": "application/json", "Content-Length": String(24 * 1024 * 1024 + 1) }, body: JSON.stringify(importBody()) }))
      expect(byLength.status).toBe(413)
      const atLimit = importBody()
      const allowed = await s.sendRaw(new Request("http://localhost/api/v1/notes/import", { method: "POST", headers: { Authorization: "Bearer alice", "Content-Type": "application/json", "Content-Length": String(24 * 1024 * 1024) }, body: JSON.stringify(atLimit) }))
      expect(allowed.status).toBe(200)
      expect((await s.call("alice", `/notes/${large.id}`)).status).toBe(404)
    }, 180_000)
  })
}
