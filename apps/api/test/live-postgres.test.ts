import { afterAll, describe, expect, test } from "bun:test"
import { mkdtemp, rm } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join, resolve } from "node:path"
import { Effect } from "effect"
import { encodeLiveAudioFrame, type LiveServerMessage } from "@meeting-notes/contracts/live"
import { makePrismaCalendarStore } from "../src/calendar-store"
import { encodeFakeSpeech } from "../src/live/providers/fake"
import { makePrismaRepository } from "../src/prisma-repository"
import { BOOTSTRAP_OWNER_ID } from "../src/repository"
import { liveFixture, occurrences, RATE, wav } from "./live-support"

const url = process.env.DATABASE_URL
const enabled = process.env.RUN_PG_TESTS === "1" && Boolean(url)
const samples = (ms: number) => Math.round(ms * RATE / 1000)
const stream = (resumeFromSample = 0) => [{ channel: "system" as const, sampleRate: RATE, encoding: "pcm_s16le" as const, baseMs: 0, resumeFromSample }]
const meeting = () => encodeFakeSpeech([
  { atMs: 200, text: "the kestrel migration moved to march", speaker: "a" },
  { atMs: 3000, text: "halvorsen renewal is four hundred thousand", speaker: "b" },
  { atMs: 6000, text: "ostrava pricing lands on thursday", speaker: "a" },
], { durationMs: 9000 })

describe.skipIf(!enabled)("live transcription on PostgreSQL", () => {
  const resources = enabled ? makePrismaRepository(url!) : null
  const cleanup: Array<() => Promise<unknown>> = []
  afterAll(async () => {
    for (const task of cleanup.reverse()) await task().catch(() => undefined)
    await resources?.close()
  })
  async function fixture(options: Parameters<typeof liveFixture>[0] = {}) {
    const made = await liveFixture({ ...options, repository: resources!.repository, accounts: { alice: BOOTSTRAP_OWNER_ID } })
    cleanup.push(made.dispose)
    const recordingNote = async () => { const id = await made.recordingNote(); cleanup.push(() => Effect.runPromise(resources!.repository.deleteNote(id))); return id }
    return { ...made, recordingNote }
  }

  test("finals are stored as live segments with note-scoped speakers and coverage", async () => {
    const script = meeting(), f = await fixture({ script }), db = resources!.database
    const noteId = await f.recordingNote()
    const client = f.connect(noteId)
    await client.hello(stream())
    await client.audio(script.pcm, 0, script.pcm.byteLength / 2, { speed: 10 })
    await client.until(() => client.finals().length === 3, "three finals")
    const rows = await db.transcriptSegment.findMany({ where: { noteId }, orderBy: { startMs: "asc" } })
    expect(rows.map(row => [row.origin, row.speakerId, row.text])).toEqual([
      ["live", "system:n:1", "the kestrel migration moved to march"],
      ["live", "system:n:2", "halvorsen renewal is four hundred thousand"],
      ["live", "system:n:1", "ostrava pricing lands on thursday"],
    ])
    expect((await db.noteSpeaker.findMany({ where: { noteId }, orderBy: { number: "asc" } })).map(row => [row.channel, row.number])).toEqual([["system", 1], ["system", 2]])
    // The ordinary read returns the same segments the socket delivered.
    expect((await f.note(noteId)).transcriptSegments).toEqual(client.finals().map(message => message.segment))
    expect((await f.note(noteId)).transcriptRevision).toBe(3)
    client.mark("end")
    await client.until(() => client.closed, "close")
    const coverage = await db.liveCoverage.findMany({ where: { noteId } })
    expect(coverage.map(row => [row.fromMs, row.toMs])).toEqual([[0, 9000]])
    // Notes can be generated as soon as the recording stops.
    await Effect.runPromise(resources!.repository.recording(noteId, "stop"))
    expect(await Effect.runPromise(resources!.repository.beginGeneration(noteId, 3, crypto.randomUUID()))).not.toBeNull()
    // Deleting the note removes the ledger and the speakers with it.
    await Effect.runPromise(resources!.repository.deleteNote(noteId))
    expect(await db.liveCoverage.count({ where: { noteId } })).toBe(0)
    expect(await db.noteSpeaker.count({ where: { noteId } })).toBe(0)
  })

  test("a skipped backlog is declared as a gap and has no text; the ledger holds only what the stream covered", async () => {
    const script = encodeFakeSpeech([
      { atMs: 200, text: "the kestrel migration moved to march", speaker: "a" },
      { atMs: 3000, text: "halvorsen renewal is four hundred thousand", speaker: "b" },
      { atMs: 6000, text: "ostrava pricing lands on thursday", speaker: "a" },
      { atMs: 9000, text: "quillfeather had three outages in dublin", speaker: "b" },
      { atMs: 12000, text: "castellane wants evidence this month", speaker: "a" },
    ], { durationMs: 15_000 })
    const f = await fixture({ script, config: { detachMs: 150, resumeWindowMs: 2000, endWaitMs: 300 } }), db = resources!.database
    const noteId = await f.recordingNote()
    const first = f.connect(noteId)
    await first.hello(stream())
    await first.audio(script.pcm, 0, samples(2800), { speed: 10 })
    await first.until(() => first.finals()[0], "first final")
    first.drop()
    await first.until(() => !f.hub.active(noteId), "session end", 3000)
    const second = f.connect(noteId)
    const ready = await second.hello(stream(first.lastAck()))
    if (ready === "closed") throw new Error("reconnect refused")
    // Stored coverage decides where the stream resumes, not the lost session.
    expect(ready.streams[0]!.acceptFromSample).toBe(samples(2800))
    await second.audio(script.pcm, samples(11_500), script.pcm.byteLength / 2, { speed: 10 })
    expect(await second.until(() => second.of("gap")[0], "gap")).toMatchObject({ fromMs: 2800, toMs: 11_500, reason: "resume_window_exceeded" })
    await second.until(() => second.finals().some(message => message.segment.text === "castellane wants evidence this month"), "final after the gap")
    second.mark("end")
    await second.until(() => second.closed, "close")
    const after = await f.note(noteId)
    expect(after.transcriptSegments.map(segment => segment.text)).toEqual(["the kestrel migration moved to march", "castellane wants evidence this month"])
    expect((await db.transcriptSegment.findMany({ where: { noteId }, orderBy: { startMs: "asc" } })).map(row => row.origin)).toEqual(["live", "live"])
    // The skipped stretch is stored, and the note shows it as missing.
    expect((await db.liveGap.findMany({ where: { noteId } })).map(row => [row.fromMs, row.toMs, row.reason])).toEqual([[2800, 11_500, "resume_window_exceeded"]])
    expect(after.transcriptGaps).toEqual([{ channel: "system", fromMs: 2800, toMs: 11_500 }])
    expect((await db.liveCoverage.findMany({ where: { noteId }, orderBy: { fromMs: "asc" } })).map(row => [row.fromMs, row.toMs])).toEqual([[0, 2800], [11_500, 15_000]])
    for (const message of [...first.finals(), ...second.finals()]) expect(after.transcriptSegments.find(segment => segment.id === message.segment.id)).toEqual(message.segment)
  })

  test("speakers the recogniser merges are relabelled in storage", async () => {
    const script = meeting(), f = await fixture({ script, config: { endWaitMs: 200 } }), db = resources!.database
    const noteId = await f.recordingNote()
    const client = f.connect(noteId)
    await client.hello(stream())
    await client.audio(script.pcm, 0, samples(5900), { speed: 10 })
    await client.until(() => client.finals().length === 2, "two finals")
    // The recogniser decides two of its speakers were one voice.
    expect(await Effect.runPromise(resources!.repository.live!.mergeSpeakers(noteId, "system", 2, 1))).toBe(true)
    expect((await f.note(noteId)).transcriptSegments.map(segment => [segment.speakerId, segment.speakerLabel])).toEqual([["system:n:1", "Speaker 1"], ["system:n:1", "Speaker 1"]])
    const merged = await db.noteSpeaker.findMany({ where: { noteId }, orderBy: { number: "asc" } })
    expect(merged[1]!.mergedIntoId).toBe(merged[0]!.id)
    client.mark("end")
    await client.until(() => client.closed, "close")
  })

  test("Stop and Resume on a new provider session: a channel's only voice keeps its number, and the microphone stays Me", async () => {
    const eleven = (name: string) => `${name} one two three four five six seven eight nine ten`
    const script = encodeFakeSpeech([
      { atMs: 200, text: eleven("arjun"), speaker: "arjun" },
      { atMs: 10_200, text: eleven("arjun"), speaker: "arjun" }, { atMs: 14_000, text: eleven("noemi"), speaker: "noemi" },
    ], { durationMs: 22_000 })
    // No hold, so the Resume opens a new provider session, which numbers speakers in the order it hears them.
    const f = await fixture({ script, fake: { sessionSpeakers: true }, config: { holdMs: 0 } }), live = resources!.repository.live!
    const noteId = await f.recordingNote(), owner = f.processing.forRepository(resources!.repository.forUser(BOOTSTRAP_OWNER_ID))
    const both = (baseMs: number) => (["system", "mic"] as const).map(channel => ({ channel, sampleRate: RATE, encoding: "pcm_s16le" as const, baseMs, resumeFromSample: 0 }))
    const first = f.connect(noteId)
    await first.hello(both(0), "capture-1")
    await Promise.all((["system", "mic"] as const).map(channel => first.audio(script.pcm, 0, samples(8000), { speed: 12, channel })))
    await first.until(() => first.finals().length === 2, "two finals")
    first.mark("end", { system: samples(8000), mic: samples(8000) })
    await Effect.runPromise(owner.recording(noteId, "stop"))
    await first.until(() => first.closed, "close")
    expect(await Effect.runPromise(live.voices(noteId, "system"))).toEqual([1])
    expect(await Effect.runPromise(live.voices(noteId, "mic"))).toEqual([1])
    await Effect.runPromise(owner.recording(noteId, "resume"))
    const second = f.connect(noteId)
    await second.hello(both(10_000), "capture-2")
    await Promise.all((["system", "mic"] as const).map(channel => second.audio(script.pcm, samples(10_000), samples(22_000), { speed: 12, offset: -samples(10_000), channel })))
    await second.until(() => second.finals().length === 4, "four finals")
    second.mark("end", { system: samples(12_000), mic: samples(12_000) })
    await second.until(() => second.closed, "close")
    const stored = await f.note(noteId)
    const of = (channel: "mic" | "system") => stored.transcriptSegments.filter(segment => segment.channel === channel).map(segment => `${segment.speakerLabel}: ${segment.text.split(" ")[0]}`)
    expect(of("system")).toEqual(["Speaker 1: arjun", "Speaker 1: arjun", "Speaker 2: noemi"])
    // A second person at the microphone for one sentence is not a confident second voice: still Me.
    expect(stored.transcriptSegments.filter(segment => segment.channel === "mic").every(segment => segment.speakerLabel === "Me")).toBe(true)
    expect(second.of("speakers")).toEqual([])
  })
  test("the live model setting is stored and cleared", async () => {
    const store = makePrismaCalendarStore(resources!.database)
    const before = (await store.getSettings())?.liveTranscriptionModel
    try {
      expect((await store.saveSettings({ liveTranscriptionModel: { provider: "elevenlabs", model: "scribe_v3_live" } })).liveTranscriptionModel).toEqual({ provider: "elevenlabs", model: "scribe_v3_live" })
      expect((await store.getSettings())!.liveTranscriptionModel).toEqual({ provider: "elevenlabs", model: "scribe_v3_live" })
      // An unrelated save leaves it alone.
      expect((await store.saveSettings({ calendarLookAheadDays: 30 })).liveTranscriptionModel).toEqual({ provider: "elevenlabs", model: "scribe_v3_live" })
    } finally { if (before) await store.saveSettings({ liveTranscriptionModel: before }) }
  })

  test("the real server entry point serves the live socket on PostgreSQL", async () => {
    // src/server.ts as deployed, with the built-in fake recogniser: upgrade route, bearer check, session, stored segments.
    const state = await mkdtemp(join(tmpdir(), "live-server-")), token = "live-postgres-test-token-0123456789abcdef"
    const port = 20000 + Math.floor(Math.random() * 20000)
    const server = Bun.spawn(["bun", resolve(import.meta.dir, "../src/server.ts")], { stdout: "pipe", stderr: "pipe", env: { ...process.env, PORT: String(port), HOST: "127.0.0.1", ADMIN_API_TOKEN: token,
      AUDIO_STORAGE_DIR: join(state, "audio"), SETTINGS_KEY_FILE: join(state, "state/settings.key"), PUBLIC_BASE_URL: `http://localhost:${port}`, LIVE_TRANSCRIPTION_PROVIDER: "fake", ELEVENLABS_API_KEY: "", OPENROUTER_API_KEY: "" } })
    // SIGKILL: a graceful stop waits for this test's own keep-alive connections.
    cleanup.push(async () => { server.kill("SIGKILL"); await server.exited; await rm(state, { recursive: true, force: true }) })
    const base = `http://127.0.0.1:${port}`, headers = { Authorization: `Bearer ${token}`, "Content-Type": "application/json" }
    for (let attempt = 0; ; attempt++) {
      if ((await fetch(`${base}/api/v1/ready`).catch(() => null))?.ok) break
      if (attempt > 100 || server.exitCode !== null) throw new Error(`server did not start: ${(await new Response(server.stderr).text()).slice(0, 500)}`)
      await Bun.sleep(100)
    }
    const api = (path: string, method: string, body?: unknown) => fetch(`${base}/api/v1${path}`, { method, headers, ...(body === undefined ? {} : { body: JSON.stringify(body) }) })
    const note = await (await api("/notes", "POST", { title: "Live on PostgreSQL" })).json() as { id: string }
    cleanup.push(() => api(`/notes/${note.id}`, "DELETE"))
    // Refused before recording starts, without a token, and from a web page.
    expect((await fetch(`${base}/api/v1/notes/${note.id}/live`, { headers: { Authorization: `Bearer ${token}`, Upgrade: "websocket" } })).status).toBe(409)
    expect((await api(`/notes/${note.id}/recording`, "POST", { command: "start" })).status).toBe(200)
    expect((await fetch(`${base}/api/v1/notes/${note.id}/live`, { headers: { Upgrade: "websocket" } })).status).toBe(401)
    expect((await fetch(`${base}/api/v1/notes/${note.id}/live`, { headers: { Authorization: `Bearer ${token}`, Origin: "https://evil.example", Upgrade: "websocket" } })).status).toBe(403)
    const messages: LiveServerMessage[] = []
    const socket = new WebSocket(`ws://127.0.0.1:${port}/api/v1/notes/${note.id}/live`, { headers: { Authorization: `Bearer ${token}` } } as never)
    let closed: number | undefined
    socket.onmessage = event => { messages.push(JSON.parse(String(event.data))) }
    socket.onclose = event => { closed = event.code }
    const until = async (probe: () => unknown, label: string) => { for (let i = 0; !probe(); i++) { if (i > 200) throw new Error(`timed out: ${label}; ${JSON.stringify(messages.map(m => m.type))} closed ${closed}`); await Bun.sleep(50) } }
    await until(() => socket.readyState === WebSocket.OPEN, "open")
    socket.send(JSON.stringify({ type: "hello", protocol: 1, captureId: "pg", streams: stream() }))
    await until(() => messages.some(message => message.type === "ready"), "ready")
    expect(messages.find(message => message.type === "ready")).toMatchObject({ provider: "fake" })
    // Three seconds of a loud tone, then silence: the fake recogniser turns signal into words.
    const pcm = new Uint8Array(samples(5000) * 2), view = new DataView(pcm.buffer)
    for (let index = 0; index < samples(3000); index++) view.setInt16(index * 2, Math.round(8000 * Math.sin(index / 6)), true)
    for (let sample = 0; sample < samples(5000); sample += samples(100)) { socket.send(encodeLiveAudioFrame({ channel: "system", firstSample: sample, pcm: pcm.subarray(sample * 2, (sample + samples(100)) * 2) }) as Uint8Array<ArrayBuffer>); await Bun.sleep(25) }
    await until(() => messages.some(message => message.type === "interim") && messages.some(message => message.type === "final"), "interim and final")
    // Audio parts are not accepted at all: the live transcript is the only one.
    expect((await api(`/notes/${note.id}/audio/chunks/${crypto.randomUUID()}`, "PUT", { channel: "system", sequence: 0, startMs: 0, audioBase64: Buffer.from(wav(pcm)).toString("base64") })).status).toBe(404)
    socket.send(JSON.stringify({ type: "mark", kind: "end", atSample: { system: samples(5000) } }))
    const stopped = await (await api(`/notes/${note.id}/recording`, "POST", { command: "stop" })).json() as { recordingState: string; transcriptSegments: Array<{ speakerId: string; text: string; isFinal: boolean }> }
    expect(stopped.recordingState).toBe("stopped")
    expect(stopped.transcriptSegments.length).toBeGreaterThan(0)
    expect(stopped.transcriptSegments.every(segment => segment.speakerId === "system:n:1" && segment.isFinal && segment.text.length > 0)).toBe(true)
    await until(() => closed !== undefined, "socket closed")
    expect(closed).toBe(1000)
  }, 30_000)
})
