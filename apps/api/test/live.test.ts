import { afterEach, describe, expect, setDefaultTimeout, test } from "bun:test"
import { Effect } from "effect"
import type { ChatCitationDTO } from "@meeting-notes/contracts"
import { encodeLiveAudioFrame, LiveCloseCode, type LiveServerMessage } from "@meeting-notes/contracts/live"
import { mergeIntervals } from "../src/live/coverage"
import { encodeFakeSpeech, type FakeScript } from "../src/live/providers/fake"
import type { LiveSession } from "../src/live/session"
import { Timeline } from "../src/live/timeline"
import { liveFixture, occurrences, RATE, transcriptText, users, type LiveFixture } from "./live-support"

setDefaultTimeout(30_000)
const open: LiveFixture[] = []
afterEach(async () => { for (const fixture of open.splice(0)) await fixture.dispose() })
async function fixture(options: Parameters<typeof liveFixture>[0] = {}) { const made = await liveFixture(options); open.push(made); return made }
const stream = (resumeFromSample = 0, baseMs = 0) => [{ channel: "system" as const, sampleRate: RATE, encoding: "pcm_s16le" as const, baseMs, resumeFromSample }]
const samples = (ms: number) => Math.round(ms * RATE / 1000)
/** The transcript citations the server issued for one chat request. */
const issued = (request: { note: unknown }) => (request.note as { meetingChat: { citations: ChatCitationDTO[] } }).meetingChat.citations.filter(citation => citation.source === "transcript")
/** Three utterances from two voices, with pauses long enough to end each one. */
const meeting = () => encodeFakeSpeech([
  { atMs: 200, text: "the kestrel migration moved to march", speaker: "a" },
  { atMs: 3000, text: "halvorsen renewal is four hundred thousand", speaker: "b" },
  { atMs: 6000, text: "ostrava pricing lands on thursday", speaker: "a" },
], { durationMs: 9000 })

describe("timeline and coverage arithmetic", () => {
  test("provider time maps back to capture samples across a skip and injected silence", () => {
    const timeline = new Timeline(1000)
    timeline.audio(0, 2000)          // upstream 0..2 s is capture 0..2000
    timeline.silence(2000, 500)      // a keep-alive: upstream 2..2.5 s, the capture stands still
    timeline.audio(2000, 1000)       // upstream 2.5..3.5 s is capture 2000..3000
    timeline.audio(9000, 1000)       // the client skipped to 9000: upstream 3.5..4.5 s
    expect(timeline.captureSample(1.5)).toBe(1500)
    expect(timeline.captureSample(2.2)).toBe(2000)
    expect(timeline.captureSample(3)).toBe(2500)
    expect(timeline.captureSample(4)).toBe(9500)
    // Coverage lists the capture audio behind a span of provider time: neither the silence nor the skipped stretch.
    expect(timeline.captureRanges(1000, 4200)).toEqual([[1000, 3000], [9000, 9700]])
    timeline.prune(3600)
    expect(timeline.captureSample(4)).toBe(9500)
    expect(timeline.anchorCount).toBe(1)
  })
  test("interval arithmetic merges and joins", () => {
    expect(mergeIntervals([{ fromMs: 5, toMs: 9 }, { fromMs: 0, toMs: 5 }, { fromMs: 20, toMs: 20 }])).toEqual([{ fromMs: 0, toMs: 9 }])
  })
})

describe("the socket refuses what it must", () => {
  test("a web page, a cookie, a stranger, another user's note, an idle note and an unconfigured instance", async () => {
    const f = await fixture()
    const noteId = await f.recordingNote("alice")
    const attempt = (headers: Record<string, string>, id = noteId) => fetch(f.url(id, "http"), { headers: { Connection: "Upgrade", Upgrade: "websocket", "Sec-WebSocket-Version": "13", "Sec-WebSocket-Key": "dGhlIHNhbXBsZSBub25jZQ==", ...headers } })
    // A browser page on another site, even with a valid token, and a signed-in browser's cookie alone.
    expect((await attempt({ Authorization: "Bearer alice", Origin: "https://evil.example" })).status).toBe(403)
    expect((await attempt({ Cookie: "better-auth.session_token=alice" })).status).toBe(401)
    expect((await attempt({ Cookie: "better-auth.session_token=alice", Origin: "https://cereal.example.test" })).status).toBe(401)
    expect((await attempt({})).status).toBe(401)
    expect((await attempt({ Authorization: "Bearer mallory" })).status).toBe(401)
    // Ownership: another user's note is indistinguishable from a missing one.
    const foreign = await attempt({ Authorization: "Bearer bob" })
    expect(foreign.status).toBe(404)
    expect(await foreign.json()).toEqual({ error: "not_found", message: "Note not found" })
    expect((await attempt({ Authorization: "Bearer alice" }, crypto.randomUUID())).status).toBe(404)
    const idle = await Effect.runPromise(f.repository.forUser(users.alice).createNote({ title: "Not recording" }))
    expect((await attempt({ Authorization: "Bearer alice" }, idle.id)).status).toBe(409)
    // The same through a WebSocket client: the connection never opens.
    const page = f.connect(noteId, "alice", { Origin: "https://evil.example" })
    await page.until(() => page.closed || page.failed, "refusal")
    expect(page.messages).toEqual([])
    const stranger = f.connect(noteId, "bob")
    await stranger.until(() => stranger.closed || stranger.failed, "refusal")
    expect(stranger.messages).toEqual([])
    expect(f.fake.control.opened).toBe(0)

    const off = await fixture({ transcriber: null })
    const offNote = await off.recordingNote()
    const response = await fetch(off.url(offNote, "http"), { headers: { Authorization: "Bearer alice", Upgrade: "websocket" } })
    expect(response.status).toBe(503)
    expect(await response.json()).toEqual({ error: "unavailable", message: "Live transcription is not configured" })
  })
  test("a trusted origin with a bearer token is accepted; malformed frames close with 4400", async () => {
    const f = await fixture({ script: meeting() })
    const noteId = await f.recordingNote()
    const client = f.connect(noteId, "alice", { Origin: "https://cereal.example.test" })
    expect(await client.hello(stream())).toMatchObject({ type: "ready", provider: "fake", streams: [{ channel: "system", acceptFromSample: 0 }], resumeWindowMs: 900_000, transcriptRevision: 0 })
    client.socket.send(new Uint8Array([9, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 2]))
    await client.until(() => client.closed, "close")
    expect(client.closed!.code).toBe(LiveCloseCode.malformed)
    expect(client.of("error").at(-1)).toMatchObject({ code: "malformed", retryable: false })

    const big = f.connect(await f.recordingNote())
    await big.hello(stream())
    big.socket.send(encodeLiveAudioFrame({ channel: "system", firstSample: 0, pcm: new Uint8Array(70_000) }) as Uint8Array<ArrayBuffer>)
    await big.until(() => big.closed, "close")
    expect(big.closed!.code).toBe(LiveCloseCode.malformed)

    const silent = f.connect(await f.recordingNote())
    await silent.opened()
    await silent.until(() => silent.closed, "hello timeout")
    expect(silent.closed!.code).toBe(LiveCloseCode.malformed)

    const garbage = f.connect(await f.recordingNote())
    await garbage.opened()
    garbage.socket.send(JSON.stringify({ type: "hello", protocol: 2, captureId: "x", streams: [] }))
    await garbage.until(() => garbage.closed, "close")
    expect(garbage.closed!.code).toBe(LiveCloseCode.malformed)
  })
  test("a second publisher is refused, a reconnect of the same capture replaces the first, and the session limit holds", async () => {
    const f = await fixture({ script: meeting(), maxSessions: 1 })
    const noteId = await f.recordingNote()
    const first = f.connect(noteId)
    await first.hello(stream(), "capture-a")
    const second = f.connect(noteId)
    expect(await second.hello(stream(), "capture-b")).toBe("closed")
    expect(second.closed!.code).toBe(LiveCloseCode.replaced)
    expect(second.of("error")[0]).toMatchObject({ code: "replaced" })
    expect(first.closed).toBeUndefined()
    // The limit is answered before the upgrade, so the client stays on parts without opening a socket.
    const other = await f.recordingNote()
    expect((await fetch(f.url(other, "http"), { headers: { Authorization: "Bearer alice", Upgrade: "websocket" } })).status).toBe(429)
    // The same capture connecting again takes over; the old socket is told why.
    const again = f.connect(noteId)
    expect(await again.hello(stream(), "capture-a")).toMatchObject({ type: "ready" })
    await first.until(() => first.closed, "replaced")
    expect(first.closed!.code).toBe(LiveCloseCode.replaced)
    expect(f.fake.control.opened).toBe(1)
  })
  test("a sample rate no recogniser takes is the client's error (4400), and opens no provider session", async () => {
    const f = await fixture({ script: meeting() })
    const client = f.connect(await f.recordingNote())
    expect(await client.hello([{ channel: "system", sampleRate: 12345, encoding: "pcm_s16le", baseMs: 0, resumeFromSample: 0 }])).toBe("closed")
    expect(client.closed!.code).toBe(LiveCloseCode.malformed)
    expect(client.of("error")[0]).toMatchObject({ code: "malformed", message: "Unsupported sample rate", retryable: false })
    expect(f.fake.control.opened).toBe(0)
    expect(f.hub.size).toBe(0)
  })
  test("a session that ends on the server closes the socket with 4401", async () => {
    const f = await fixture({ script: meeting() })
    const client = f.connect(await f.recordingNote())
    await client.hello(stream())
    f.revoked.add("alice")
    await client.until(() => client.closed, "close")
    expect(client.closed!.code).toBe(LiveCloseCode.unauthenticated)
  })
})

describe("streaming", () => {
  test("interim text, then final text on the socket; finals are the stored transcript", async () => {
    const script = meeting(), f = await fixture({ script })
    const noteId = await f.recordingNote()
    const client = f.connect(noteId)
    await client.hello(stream())
    await client.audio(script.pcm, 0, samples(2900), { speed: 8 })
    const final = await client.until(() => client.finals()[0], "first final")
    const interims = client.of("interim").filter(message => message.utteranceId === final.utteranceId)
    // Interim text grows word by word under one utterance ID and arrives before the final.
    expect(interims.length).toBeGreaterThanOrEqual(3)
    expect(interims.map(message => message.rev)).toEqual(interims.map((_, index) => index))
    expect(interims.at(-1)!.text).toBe("the kestrel migration moved to march")
    expect(client.messages.indexOf(interims[0]!)).toBeLessThan(client.messages.indexOf(final))
    expect(final.segment).toMatchObject({ channel: "system", text: "the kestrel migration moved to march", isFinal: true, speakerId: "system:n:1", speakerLabel: "Speaker 1", startMs: 200 })
    expect(final.transcriptRevision).toBe(1)
    await client.audio(script.pcm, samples(2900), script.pcm.byteLength / 2, { speed: 8 })
    await client.until(() => client.finals().length === 3, "three finals")
    // The ordinary note read returns exactly what was sent as final, with the same IDs.
    const stored = await f.note(noteId)
    expect(stored.transcriptSegments).toEqual(client.finals().map(message => message.segment))
    expect(stored.transcriptSegments.map(segment => segment.speakerId)).toEqual(["system:n:1", "system:n:2", "system:n:1"])
    expect(stored.transcriptRevision).toBe(3)
    expect(stored.duration).toBe("0:07")
    // Interim text is never stored.
    expect(stored.transcriptSegments.every(segment => segment.isFinal)).toBe(true)
    await client.until(() => client.lastAck() === script.pcm.byteLength / 2, "the last ack")
    expect(client.of("status").some(status => status.state === "catching_up")).toBe(true)
  })
  test("duplicated and re-sent frames change nothing", async () => {
    const script = meeting(), f = await fixture({ script })
    const noteId = await f.recordingNote()
    const client = f.connect(noteId)
    await client.hello(stream())
    await client.audio(script.pcm, 0, samples(2000), { speed: 20 })
    await client.audio(script.pcm, samples(1000), samples(2000), { speed: 20 })   // a replay of audio the server has
    await client.audio(script.pcm, samples(1950), script.pcm.byteLength / 2, { speed: 20 })   // overlapping the high-water mark
    await client.until(() => client.finals().length === 3, "three finals")
    const stored = await f.note(noteId)
    for (const line of script.lines) expect(occurrences(stored, line.text)).toBe(1)
    expect(client.of("gap")).toEqual([])
  })
  test("pause finalises the last words without waiting for silence; end stores everything and closes", async () => {
    const script = encodeFakeSpeech([{ atMs: 100, text: "we agreed on the wexcombe alarm", speaker: "a" }, { atMs: 2600, text: "the fennimore account opened a case", speaker: "b" }], { durationMs: 4400 }), f = await fixture({ script })
    const noteId = await f.recordingNote()
    const client = f.connect(noteId)
    await client.hello(stream())
    // Stop sending right after the last word of the first line: no trailing silence reaches the recogniser.
    const firstEnd = script.lines[0]!.lastSample + 1
    await client.audio(script.pcm, 0, firstEnd, { speed: 10 })
    client.mark("pause", { system: firstEnd })
    const final = await client.until(() => client.finals()[0], "final after pause")
    expect(final.segment.text).toBe("we agreed on the wexcombe alarm")
    expect(client.of("status").at(-1)).toMatchObject({ state: "paused" })
    client.mark("resume", { system: firstEnd })
    const secondEnd = script.lines[1]!.lastSample + 1
    await client.audio(script.pcm, firstEnd, secondEnd, { speed: 10 })
    client.mark("end", { system: secondEnd })
    await client.until(() => client.closed, "close after end")
    expect(client.closed!.code).toBe(1000)
    const stored = await f.note(noteId)
    expect(stored.transcriptSegments.map(segment => segment.text)).toEqual(["we agreed on the wexcombe alarm", "the fennimore account opened a case"])
    // The same provider session throughout, so the second voice is a second speaker, not a renumbered first.
    expect(f.fake.control.opened).toBe(1)
    expect(stored.transcriptSegments.map(segment => segment.speakerLabel)).toEqual(["Speaker 1", "Speaker 2"])
    expect(f.hub.active(noteId)).toBe(false)
  })
  test("mark end followed at once by Stop, and Stop racing the last frames, both keep the last words", async () => {
    const script = encodeFakeSpeech([{ atMs: 100, text: "castellane wants evidence by the end of the month" }], { durationMs: 3200 })
    for (const withMark of [true, false]) {
      const f = await fixture({ script, fake: { finalDelayMs: 200 } })
      const noteId = await f.recordingNote()
      const client = f.connect(noteId)
      await client.hello(stream())
      const end = script.lines[0]!.lastSample + 1
      // No wait at all between the last frame, the mark and the HTTP Stop.
      const sending = client.audio(script.pcm, 0, end, { speed: 10 })
      if (withMark) { await sending; client.mark("end", { system: end }) }
      else await Bun.sleep((end / RATE) * 1000 / 10 - 60)   // Stop is handled while frames are still arriving
      const stopped = await Effect.runPromise(f.processing.forRepository(f.repository.forUser(users.alice)).recording(noteId, "stop"))
      await sending
      expect(stopped.transcriptSegments.map(segment => segment.text), withMark ? "with mark" : "without mark").toEqual(["castellane wants evidence by the end of the month"])
      // Nothing is written after Stop returned: the revision the client generates notes from is final.
      await Bun.sleep(400)
      expect((await f.note(noteId)).transcriptRevision).toBe(stopped.transcriptRevision)
    }
  })
  test("lagMs is the recogniser's delay: small through a monologue, a silence and a replay", async () => {
    // One utterance of 9 s, 8 s of silence, then a second utterance; interim text follows speech by 300 ms.
    const script = encodeFakeSpeech([{ atMs: 200, text: Array.from({ length: 30 }, (_, index) => `word${index}`).join(" ") }, { atMs: 17_200, text: "and then after a long silence somebody speaks again" }], { durationMs: 21_000 })
    const f = await fixture({ script, fake: { interimDelayMs: 300, finalDelayMs: 300, silenceMs: 2000 }, config: { statusIntervalMs: 100 } })
    const client = f.connect(await f.recordingNote())
    await client.hello(stream())
    await client.audio(script.pcm, 0, script.pcm.byteLength / 2, { speed: 1 })
    const statuses = client.of("status").filter(status => status.state === "live")
    expect(statuses.length).toBeGreaterThan(100)
    // Never the length of the utterance in progress (up to 9 s here), nor of the silence after it, nor of
    // that silence at the moment speech starts again (the first words have no interim text yet).
    expect(Math.max(...statuses.map(status => status.lagMs))).toBeLessThan(1500)
    expect(statuses.some(status => status.lagMs > 0)).toBe(true)
    // The same audio again at four times real time. Lag is counted in audio, so the recogniser's 300 ms
    // is 1.2 s of audio here; it still has nothing to do with the 9 s utterance or the backlog.
    const replay = f.connect(await f.recordingNote())
    await replay.hello(stream())
    await replay.audio(script.pcm, 0, script.pcm.byteLength / 2, { speed: 4 })
    await replay.until(() => replay.finals()[0], "final")
    expect(replay.of("status").some(status => status.state === "catching_up")).toBe(true)
    expect(Math.max(...replay.of("status").map(status => status.lagMs))).toBeLessThan(2200)
  }, 30_000)
  test("Stop through the API lets the session store its last words first", async () => {
    const script = encodeFakeSpeech([{ atMs: 100, text: "ship the yarrowby patch before friday" }], { durationMs: 2000 }), f = await fixture({ script, fake: { finalDelayMs: 150 } })
    const noteId = await f.recordingNote()
    const client = f.connect(noteId)
    await client.hello(stream())
    await client.audio(script.pcm, 0, script.lines[0]!.lastSample + 1, { speed: 10 })
    await client.until(() => client.lastAck() === script.lines[0]!.lastSample + 1, "the server has the audio")
    const stopped = await Effect.runPromise(f.processing.forRepository(f.repository.forUser(users.alice)).recording(noteId, "stop"))
    expect(stopped.recordingState).toBe("stopped")
    expect(stopped.transcriptSegments.map(segment => segment.text)).toEqual(["ship the yarrowby patch before friday"])
    await client.until(() => client.closed, "close")
    expect(f.hub.active(noteId)).toBe(false)
  })
})

describe("a stream that ends in the middle of a word", () => {
  const twoLines = () => encodeFakeSpeech([{ atMs: 200, text: "the kestrel migration moved to march", speaker: "a" }, { atMs: 3000, text: "halvorsen renewal is four hundred thousand", speaker: "b" }], { durationMs: 7000 })
  // 700 ms into the second line: inside its third word.
  const cut = (script: FakeScript) => script.lines[1]!.firstSample + samples(700)
  test("a client killed mid-sentence: everything it sent is on the server once the session times out, and a return replays only the rest", async () => {
    const script = twoLines(), f = await fixture({ script, config: { detachMs: 150, endWaitMs: 200 } })
    const noteId = await f.recordingNote()
    const client = f.connect(noteId)
    await client.hello(stream())
    await client.audio(script.pcm, 0, cut(script), { speed: 10 })
    await client.until(() => client.finals()[0] && client.of("interim").some(message => message.text.startsWith("halvorsen")), "first final and the start of the second line")
    // The process dies: no mark, no close handshake, and it may never come back.
    client.drop()
    await client.until(() => !f.hub.active(noteId), "session end")
    // Nothing it sent is lost: the sentence it was in the middle of is stored up to the cut.
    expect((await f.note(noteId)).transcriptSegments.map(segment => segment.text)).toEqual(["the kestrel migration moved to march", "halvorsen renewal is-"])
    // It comes back after all and replays from where the server's coverage ends: the rest of the sentence, once.
    const again = f.connect(noteId)
    const ready = await again.hello(stream(cut(script)))
    if (ready === "closed") throw new Error("refused")
    const accept = ready.streams[0]!.acceptFromSample
    expect(accept).toBeGreaterThanOrEqual(cut(script) - samples(50))
    await again.audio(script.pcm, accept, script.pcm.byteLength / 2, { speed: 10 })
    again.mark("end", { system: script.pcm.byteLength / 2 })
    await again.until(() => again.closed, "close")
    const text = transcriptText(await f.note(noteId))
    expect(text.split("the kestrel migration moved to march").length - 1).toBe(1)
    expect(text.split("four hundred thousand").length - 1).toBe(1)
  })
  test("Stop while the client still holds unsent audio: what was heard is stored, clipped word and all, rather than lost", async () => {
    const script = twoLines(), f = await fixture({ script, config: { endWaitMs: 200 } })
    const noteId = await f.recordingNote()
    const client = f.connect(noteId)
    await client.hello(stream())
    await client.audio(script.pcm, 0, cut(script), { speed: 10 })
    await client.until(() => client.finals()[0] && client.of("interim").some(message => message.text.startsWith("halvorsen")), "first final and the start of the second line")
    client.mark("end", { system: script.pcm.byteLength / 2 })
    await client.until(() => client.closed, "close")
    expect(client.closed!.code).toBe(1000)
    expect((await f.note(noteId)).transcriptSegments.map(segment => segment.text)).toEqual(["the kestrel migration moved to march", "halvorsen renewal is-"])
  })
  test("the capture itself ends there: what was heard is stored, clipped word and all", async () => {
    const script = twoLines(), f = await fixture({ script, config: { endWaitMs: 200 } })
    const noteId = await f.recordingNote()
    const client = f.connect(noteId)
    await client.hello(stream())
    await client.audio(script.pcm, 0, cut(script), { speed: 10 })
    await client.until(() => client.finals()[0], "first final")
    client.mark("end", { system: cut(script) })
    await client.until(() => client.closed, "close")
    expect((await f.note(noteId)).transcriptSegments.map(segment => segment.text)).toEqual(["the kestrel migration moved to march", "halvorsen renewal is-"])
  })
})

describe("a client that reads too slowly", () => {
  /** A socket whose send reports back-pressure on demand, as Bun's does when the peer stops reading. */
  function slowSocket() {
    const sent: LiveServerMessage[] = [], state = { blocked: false, closed: undefined as number | undefined, sent }
    return { state, socket: { send(text: string) { sent.push(JSON.parse(text)); return state.blocked ? -1 : text.length }, close(code: number) { state.closed = code } } }
  }
  const lines = Array.from({ length: 8 }, (_, index) => ({ atMs: 200 + index * 2000, text: `line number ${index} is here`, speaker: "a" }))
  const feed = async (session: LiveSession, script: FakeScript, fromMs: number, toMs: number) => {
    for (let sample = samples(fromMs); sample < samples(toMs); sample += 1600) { session.audio({ channel: "system", firstSample: sample, pcm: script.pcm.subarray(sample * 2, (sample + 1600) * 2) }); await Bun.sleep(2) }
    await Bun.sleep(250)
  }
  test("interim text and acks are held back, finals still go out, and the newest interim follows once it drains", async () => {
    const script = encodeFakeSpeech(lines, { durationMs: 17_000 }), f = await fixture({ script })
    const noteId = await f.recordingNote(), { socket, state } = slowSocket()
    const session = await f.hub.connect(noteId, { type: "hello", protocol: 1, captureId: "slow", streams: stream() }, socket, f.fake)
    await feed(session, script, 0, 2000)
    expect(state.sent.some(message => message.type === "interim")).toBe(true)
    // The peer stops reading.
    state.blocked = true
    const before = state.sent.length
    await feed(session, script, 2000, 9000)
    const during = state.sent.slice(before), afterFirst = during.slice(1)
    // The message that met the back-pressure was still handed to the transport; after it only finals are.
    expect(afterFirst.filter(message => message.type === "final").map(message => (message as { segment: { text: string } }).segment.text)).toEqual(["line number 0 is here", "line number 1 is here", "line number 2 is here", "line number 3 is here"])
    expect(afterFirst.filter(message => message.type !== "final").map(message => message.type)).toEqual([])
    expect(state.closed).toBeUndefined()
    // Mid-way through the next line the peer reads again: it gets the current interim at once.
    await feed(session, script, 9000, 9100)
    state.blocked = false
    const mark = state.sent.length
    session.drained(socket)
    expect(state.sent.slice(mark)[0]).toMatchObject({ type: "interim", text: "line number 4" })
    await feed(session, script, 9100, 17_000)
    expect((await f.note(noteId)).transcriptSegments.map(segment => segment.text)).toEqual(lines.map(line => line.text))
    expect(state.sent.filter(message => message.type === "final")).toHaveLength(8)
  })
  test("too many finals behind a client that never drains close it with 4408; the transcript is complete and a reconnect resynchronises", async () => {
    const script = encodeFakeSpeech(lines, { durationMs: 17_000 }), f = await fixture({ script, config: { maxQueuedFinals: 3 } })
    const noteId = await f.recordingNote(), { socket, state } = slowSocket()
    const session = await f.hub.connect(noteId, { type: "hello", protocol: 1, captureId: "slow", streams: stream() }, socket, f.fake)
    state.blocked = true
    await feed(session, script, 0, 17_000)
    expect(state.closed).toBe(LiveCloseCode.clientTooSlow)
    expect(state.sent.at(-1)).toMatchObject({ type: "error", code: "client_too_slow", retryable: true })
    // Three finals were handed over, the fourth closed the connection. Nothing was lost on the server.
    expect(state.sent.filter(message => message.type === "final")).toHaveLength(4)
    expect((await f.note(noteId)).transcriptSegments.map(segment => segment.text)).toEqual(lines.map(line => line.text))
    // The session is still there. A reconnect is told the revision, which is ahead of what the client saw.
    const again = slowSocket()
    await f.hub.connect(noteId, { type: "hello", protocol: 1, captureId: "slow", streams: stream() }, again.socket, f.fake)
    expect(again.state.sent[0]).toMatchObject({ type: "ready", transcriptRevision: 8, streams: [{ channel: "system", acceptFromSample: samples(17_000) }] })
    expect(f.fake.control.opened).toBe(1)
  })
})

describe("reconnection", () => {
  test("a short drop resumes on the same provider session with no lost or duplicated text", async () => {
    const script = meeting(), f = await fixture({ script, config: { detachMs: 2000 } })
    const noteId = await f.recordingNote()
    const first = f.connect(noteId)
    await first.hello(stream())
    // The connection is lost in the middle of the second sentence.
    const cut = script.lines[1]!.firstSample + samples(700)
    await first.audio(script.pcm, 0, cut, { speed: 10 })
    await first.until(() => first.lastAck() > 0, "an ack")
    const acked = first.lastAck()
    first.drop()
    await Bun.sleep(150)
    const second = f.connect(noteId)
    const ready = await second.hello(stream(acked))
    if (ready === "closed") throw new Error("reconnect refused")
    // The server's high-water mark can be later than the last ack the client saw.
    const accept = ready.streams[0]!.acceptFromSample
    expect(accept).toBeGreaterThanOrEqual(acked)
    expect(accept).toBe(cut)
    await second.audio(script.pcm, accept, script.pcm.byteLength / 2, { speed: 10 })
    await second.until(() => f.note(noteId).then(note => note.transcriptSegments.length === 3 || undefined) as never, "stored").catch(() => undefined)
    await Bun.sleep(300)
    const stored = await f.note(noteId)
    expect(stored.transcriptSegments.map(segment => segment.text)).toEqual(script.lines.map(line => line.text))
    expect(stored.transcriptSegments.map(segment => segment.speakerLabel)).toEqual(["Speaker 1", "Speaker 2", "Speaker 1"])
    expect(f.fake.control.opened).toBe(1)
    expect([...first.of("gap"), ...second.of("gap")]).toEqual([])
    // Finals stored while no socket was attached are delivered after `ready`. One written into a
    // connection that was already dead cannot be; `ready.transcriptRevision` then tells the client
    // it is behind and must read the note.
    const delivered = new Set([...first.finals(), ...second.finals()].map(message => message.segment.id))
    expect(stored.transcriptSegments.every(segment => delivered.has(segment.id)) || ready.transcriptRevision > first.finals().length).toBe(true)
  })
  test("a backlog beyond the resume window is skipped and declared as a gap; the skipped stretch has no text", async () => {
    const script = encodeFakeSpeech([
      { atMs: 200, text: "the kestrel migration moved to march", speaker: "a" },
      { atMs: 3000, text: "halvorsen renewal is four hundred thousand", speaker: "b" },
      { atMs: 6000, text: "ostrava pricing lands on thursday", speaker: "a" },
      { atMs: 9000, text: "quillfeather had three outages in dublin", speaker: "b" },
      { atMs: 12000, text: "castellane wants evidence this month", speaker: "a" },
    ], { durationMs: 15_000 })
    const f = await fixture({ script, config: { detachMs: 150, resumeWindowMs: 2000, endWaitMs: 300 } })
    const noteId = await f.recordingNote()
    const first = f.connect(noteId)
    await first.hello(stream())
    await first.audio(script.pcm, 0, samples(2800), { speed: 10 })
    await first.until(() => first.finals()[0], "first final")
    first.drop()
    // Longer than the server keeps a detached session: the session ends, and its provider session is held for a while.
    await first.until(() => !f.hub.active(noteId), "session end", 3000)
    const second = f.connect(noteId)
    const ready = await second.hello(stream(first.lastAck()))
    if (ready === "closed") throw new Error("reconnect refused")
    expect(ready.resumeWindowMs).toBe(2000)
    // The capture now holds 11.5 s; the backlog is far beyond the window, so the client jumps near the end.
    const jump = samples(11_500)
    await second.audio(script.pcm, jump, script.pcm.byteLength / 2, { speed: 10 })
    const gap = await second.until(() => second.of("gap")[0], "gap")
    expect(gap).toMatchObject({ channel: "system", reason: "resume_window_exceeded", toMs: 11_500 })
    expect(gap.fromMs).toBeLessThanOrEqual(2800)
    // The skipped stretch is not "speech waiting for text": lag does not jump by the size of the gap.
    expect(Math.max(...second.of("status").map(status => status.lagMs))).toBeLessThan(3000)
    await second.until(() => second.finals().some(message => message.segment.text === "castellane wants evidence this month"), "final after the gap")
    // The reconnect took the held provider session over, so the voice it already knew keeps its label.
    expect(f.fake.control.opened).toBe(1)
    const before = await f.note(noteId)
    expect(before.transcriptSegments.map(segment => segment.text)).toEqual(["the kestrel migration moved to march", "castellane wants evidence this month"])
    expect(before.transcriptSegments.map(segment => segment.speakerLabel)).toEqual(["Speaker 1", "Speaker 1"])
    second.mark("end", { system: script.pcm.byteLength / 2 })
    await second.until(() => second.closed, "close")

    // Nothing transcribes the skipped stretch later: the live transcript is the only one.
    expect((await f.note(noteId)).transcriptSegments.map(segment => segment.text)).toEqual(["the kestrel migration moved to march", "castellane wants evidence this month"])
    // The note says what is missing, so the transcript shows it.
    const gaps = (await f.note(noteId)).transcriptGaps ?? []
    expect(gaps).toHaveLength(1)
    expect(gaps[0]).toMatchObject({ channel: "system", toMs: 11_500 })
    expect(gaps[0]!.fromMs).toBeLessThanOrEqual(2800)
  })
  test("after the server loses its sessions the client resumes from stored coverage", async () => {
    const script = meeting(), f = await fixture({ script })
    const noteId = await f.recordingNote()
    const first = f.connect(noteId)
    await first.hello(stream())
    await first.audio(script.pcm, 0, samples(4200), { speed: 10 })
    await first.until(() => first.finals()[0], "first final")
    // A restart: no session survives and nothing is flushed; stored segments and the ledger remain.
    await f.hub.dispose({ abandon: true })
    await first.until(() => first.closed, "close")
    const second = f.connect(noteId)
    const ready = await second.hello(stream(first.lastAck()))
    if (ready === "closed") throw new Error("reconnect refused")
    // Audio after the last stored final was acknowledged but never finalised: it is asked for again.
    const accept = ready.streams[0]!.acceptFromSample
    expect(accept).toBeLessThan(first.lastAck())
    expect(accept).toBeGreaterThanOrEqual(script.lines[0]!.lastSample)
    expect(ready.transcriptRevision).toBe(1)
    await second.audio(script.pcm, accept, script.pcm.byteLength / 2, { speed: 10 })
    await second.until(() => second.finals().length === 2, "the rest")
    const stored = await f.note(noteId)
    expect(stored.transcriptSegments.map(segment => segment.text)).toEqual(script.lines.map(line => line.text))
  })
})

describe("the provider", () => {
  test("a lost provider session hands the recording back to the client, whose replay leaves nothing out", async () => {
    const script = meeting(), f = await fixture({ script, config: { endWaitMs: 300 } })
    const noteId = await f.recordingNote()
    const client = f.connect(noteId)
    await client.hello(stream())
    await client.audio(script.pcm, 0, samples(4500), { speed: 10 })
    await client.until(() => client.finals()[0], "first final")
    f.fake.control.fail()
    // Told to reconnect (1012, which clients retry), not left with a stretch that would have no text.
    await client.until(() => client.closed, "close")
    expect(client.closed!.code).toBe(1012)
    expect(client.of("error").at(-1)).toMatchObject({ code: "provider_unavailable", retryable: true })
    const again = f.connect(noteId)
    const ready = await again.hello(stream(samples(4500)))
    if (ready === "closed") throw new Error("reconnect refused")
    const accept = ready.streams[0]!.acceptFromSample
    expect(accept).toBeLessThanOrEqual(script.lines[1]!.firstSample)
    await again.audio(script.pcm, accept, script.pcm.byteLength / 2, { speed: 10 })
    again.mark("end")
    await again.until(() => again.closed, "close")
    const stored = await f.note(noteId)
    for (const line of script.lines) expect(occurrences(stored, line.text), line.text).toBe(1)
    expect(stored.transcriptSegments.map(segment => segment.text)).toEqual(script.lines.map(line => line.text))
  })
  test("a provider that cannot be reached at hello is refused as unavailable, and nothing is left open", async () => {
    const f = await fixture({ script: meeting() })
    f.fake.control.failOpens = 1
    const client = f.connect(await f.recordingNote())
    expect(await client.hello(stream())).toBe("closed")
    expect(client.closed!.code).toBe(LiveCloseCode.providerUnavailable)
    expect(client.of("error")[0]).toMatchObject({ code: "provider_unavailable" })
    expect(f.hub.size).toBe(0)
  })
  test("a provider that stops reading is dropped once ten seconds of audio are waiting, and the client is told to reconnect", async () => {
    const script = encodeFakeSpeech([{ atMs: 100, text: "one two three" }], { durationMs: 16_000 }), f = await fixture({ script, config: { reopenDelaysMs: [5000] } })
    const client = f.connect(await f.recordingNote())
    await client.hello(stream())
    f.fake.control.stalled = true
    await client.audio(script.pcm, 0, script.pcm.byteLength / 2, { speed: 60 })
    // The client still has that audio; its reconnect replays it to a provider session that reads.
    await client.until(() => client.closed, "close")
    expect(client.closed!.code).toBe(1012)
  })

})

describe("coverage and notes", () => {
  test("Stop leaves the live transcript complete, and notes can be generated from it at once", async () => {
    const script = meeting(), f = await fixture({ script })
    const noteId = await f.recordingNote()
    const client = f.connect(noteId)
    await client.hello(stream())
    await client.audio(script.pcm, 0, script.pcm.byteLength / 2, { speed: 10 })
    await client.until(() => client.finals().length === 3, "three finals")
    const stopped = await Effect.runPromise(f.processing.forRepository(f.repository.forUser(users.alice)).recording(noteId, "stop"))
    expect(stopped.transcriptSegments.map(segment => segment.text)).toEqual(script.lines.map(line => line.text))
    expect(await Effect.runPromise(f.repository.beginGeneration(noteId, stopped.transcriptRevision, crypto.randomUUID()))).not.toBeNull()
  })
  test("silence is confirmed with the provider, so coverage (where a reconnect resumes) moves through it", async () => {
    const script = encodeFakeSpeech([{ atMs: 200, text: "only this was said" }], { durationMs: 9000 }), f = await fixture({ script, config: { checkpointMs: 1000 } })
    const noteId = await f.recordingNote()
    const client = f.connect(noteId)
    await client.hello(stream())
    await client.audio(script.pcm, 0, script.pcm.byteLength / 2, { speed: 4 })
    await client.until(() => client.of("ack").some(ack => ack.transcribedThroughMs >= 7000), "coverage moving through silence")
    const ledger = await Effect.runPromise(f.repository.live!.ledger(noteId))
    expect(Math.max(...ledger.map(row => row.toMs))).toBeGreaterThanOrEqual(7000)
  })
})

describe("chat during a recording", () => {
  test("stored text is citable, the unfinished utterance is context only, and the prompt says how current it is", async () => {
    const script = encodeFakeSpeech([
      { atMs: 200, text: "the kestrel migration moved to march", speaker: "a" },
      { atMs: 3000, text: "that works for finance", speaker: "b" },
      { atMs: 5000, text: "the skerryvore vendor raised prices by eleven percent and next year we should", speaker: "b" },
    ], { durationMs: 10_000 })
    const f = await fixture({ script })
    const noteId = await f.recordingNote()
    const client = f.connect(noteId)
    await client.hello(stream())
    // Stop in the middle of the third sentence: its words exist only as interim text.
    const cut = script.lines[2]!.firstSample + samples(2500)
    await client.audio(script.pcm, 0, samples(2900), { speed: 10 })
    await client.audio(script.pcm, samples(2900), cut, { speed: 1 })
    await client.until(() => client.of("interim").some(message => message.text.includes("eleven percent")), "interim with the fact")
    const { events, request } = await f.chat(noteId, "How much did the vendor raise prices?")
    // The unfinished line carries the label its voice already has in the stored transcript.
    expect(request.context).toContain("<unfinished>\nThem 2: the skerryvore vendor raised prices by eleven percent")
    expect(request.context).toMatch(/\[S1\] Them 1: the kestrel migration moved to march\n\[S2\] Them 2: that works for finance/)
    expect(request.context).toContain("Transcription is streaming")
    expect(request.context).toContain("cannot be cited")
    // The base sentence about labels being local to an audio part does not apply to a live transcript.
    expect(request.context).toContain("Voice numbers are consistent through the transcript: the same number is the same voice.")
    expect(request.context).not.toContain("restart at every time marker")
    // How current the transcript is: where the server's audio stands, beyond the last stored line.
    expect(request.context).toMatch(/The transcript currently ends at line S2, about 0:0[34] into the recording/)
    expect(request.context).toMatch(/The server has heard the meeting up to about 0:07\./)
    // Only the stored line has a citation; nothing points at the unfinished one.
    const citations = events.find(event => event.type === "citations")
    expect(citations).toBeDefined()
    const note = await f.note(noteId)
    expect(note.transcriptSegments).toHaveLength(2)
    expect(issued(request).map(citation => citation.quote)).toEqual(["the kestrel migration moved to march", "that works for finance"])
    expect((request.note as { live?: { tail: unknown[]; state: string; asOfMs: number; transcribedThroughMs: number } }).live).toMatchObject({ state: "live", transcribedThroughMs: note.transcriptSegments[1]!.endMs!, tail: [{ channel: "system", speakerId: "system:n:2" }] })
  }, 20_000)
  test("a question in a pause makes the last sentence citable; one asked mid-word leaves the word whole", async () => {
    const script = encodeFakeSpeech([{ atMs: 200, text: "the lindqvist offer went out yesterday", speaker: "a" }, { atMs: 4000, text: "petrichor has two open roles", speaker: "a" }], { durationMs: 8000 })
    // A long silence threshold, so only a flush can end the first sentence early.
    const f = await fixture({ script, fake: { silenceMs: 3000 }, config: { chatFlushIntervalMs: 0 } })
    const noteId = await f.recordingNote()
    const client = f.connect(noteId)
    await client.hello(stream())
    await client.audio(script.pcm, 0, script.lines[0]!.lastSample + 1 + samples(300), { speed: 10 })
    await client.until(() => client.of("interim").some(message => message.text.endsWith("yesterday")), "interim")
    expect((await f.note(noteId)).transcriptSegments).toHaveLength(0)
    const first = await f.chat(noteId, "When did the offer go out?")
    expect(first.request.context).toMatch(/\[S1\] .*the lindqvist offer went out yesterday/)
    expect(first.request.context).not.toContain("<unfinished>")
    // Asked in the middle of the word "has": a flush now would store a broken word, so none is sent.
    await client.audio(script.pcm, script.lines[0]!.lastSample + 1 + samples(300), script.lines[1]!.firstSample + samples(400), { speed: 10 })
    await client.until(() => client.of("interim").some(message => message.text === "petrichor"), "interim")
    const second = await f.chat(noteId, "And the open roles?")
    expect(second.request.context).toContain("<unfinished>\nThem: petrichor …")
    expect((await f.note(noteId)).transcriptSegments.map(segment => segment.text)).toEqual(["the lindqvist offer went out yesterday"])
    await client.audio(script.pcm, script.lines[1]!.firstSample + samples(400), script.lines[1]!.lastSample + 1, { speed: 10 })
    client.mark("end")
    await client.until(() => client.closed, "close")
    expect((await f.note(noteId)).transcriptSegments.map(segment => segment.text)).toEqual(["the lindqvist offer went out yesterday", "petrichor has two open roles"])
  })
  test("a spoken fact is in the chat context within three seconds at the median", async () => {
    // Slower than the provider measured for real: interim text 600 ms after speech, final text 1200 ms after the pause.
    const lines = ["kestrel moved to march fourteenth", "halvorsen is worth four hundred thousand", "ostrava pricing lands thursday", "quillfeather had three outages", "wexcombe alarm at ninety seconds", "lindqvist starts in may", "petrichor has two open roles"]
    const script = encodeFakeSpeech(lines.map((text, index) => ({ atMs: 300 + index * 2600, text, speaker: index % 2 ? "b" : "a" })), { durationMs: 300 + lines.length * 2600 })
    const f = await fixture({ script, fake: { interimDelayMs: 600, finalDelayMs: 1200 } })
    const noteId = await f.recordingNote()
    const client = f.connect(noteId)
    await client.hello(stream())
    // A timing test means nothing on a machine that cannot keep time: watch how late a 20 ms timer fires.
    const lateness: number[] = []
    let expected = performance.now() + 20
    const watch = setInterval(() => { lateness.push(performance.now() - expected); expected = performance.now() + 20 }, 20)
    const started = performance.now()
    const streaming = client.audio(script.pcm, 0, script.pcm.byteLength / 2, { speed: 1 })
    const answerable: number[] = [], citable: number[] = [], interim: number[] = []
    await Promise.all(script.lines.map(async line => {
      const token = line.text.split(" ")[0]!, spokenAt = started + (line.lastSample + 1) * 1000 / RATE
      await Bun.sleep(Math.max(0, spokenAt - performance.now()))
      let seen = false, cited = false
      while (!cited && performance.now() - spokenAt < 20_000) {
        const { request } = await f.chat(noteId, `What about ${token}?`)
        const now = performance.now() - spokenAt
        // Answerable: the whole sentence is in the context, stored or unfinished. Citable: it has a source label.
        if (!seen && request.context.includes(line.text)) { seen = true; answerable.push(now) }
        if (new RegExp(`\\[S\\d+\\] [^\\n]*${line.text}`).test(request.context)) { cited = true; citable.push(now) }
        await Bun.sleep(100)
      }
      const first = client.of("interim").find(message => message.text.includes(line.text))
      if (first) interim.push(0)
    }))
    await streaming
    clearInterval(watch)
    const starved = [...lateness].sort((a, b) => a - b)[Math.floor(lateness.length * 0.95)]! > 150
    if (starved) { console.log("live chat latency: not asserted, the event loop was starved (a 20 ms timer ran more than 150 ms late at its 95th percentile)"); return }
    const median = (values: number[]) => [...values].sort((a, b) => a - b)[Math.floor(values.length / 2)]!
    const p95 = (values: number[]) => [...values].sort((a, b) => a - b)[Math.min(values.length - 1, Math.ceil(values.length * 0.95) - 1)]!
    console.log(`live chat latency (fake provider, interim +600 ms, final +1200 ms): answerable median ${Math.round(median(answerable))} ms, p95 ${Math.round(p95(answerable))} ms; citable median ${Math.round(median(citable))} ms, p95 ${Math.round(p95(citable))} ms`)
    expect(answerable).toHaveLength(lines.length)
    expect(citable).toHaveLength(lines.length)
    expect(median(answerable)).toBeLessThanOrEqual(3000)
    expect(p95(answerable)).toBeLessThanOrEqual(6000)
    expect(median(citable)).toBeLessThanOrEqual(6000)
    expect(interim).toHaveLength(lines.length)
  }, 90_000)
  test("a citation issued during the recording still validates after more speech and after Stop", async () => {
    const script = meeting(), f = await fixture({ script })
    const noteId = await f.recordingNote()
    const client = f.connect(noteId)
    await client.hello(stream())
    await client.audio(script.pcm, 0, samples(2900), { speed: 10 })
    await client.until(() => client.finals()[0], "first final")
    const { request } = await f.chat(noteId, "When is the migration?")
    const cited = issued(request)
    expect(cited).toHaveLength(1)
    await client.audio(script.pcm, samples(2900), script.pcm.byteLength / 2, { speed: 10 })
    await client.until(() => client.finals().length === 3, "three finals")
    client.mark("end")
    await client.until(() => client.closed, "close")
    const { checkChatCitations } = await import("../src/ai/citations")
    const check = await checkChatCitations(cited, f.repository.forUser(users.alice), new AbortController().signal)
    expect(check.valid).toHaveLength(1)
    expect(check.withdrawn).toBe(false)
  })
})
