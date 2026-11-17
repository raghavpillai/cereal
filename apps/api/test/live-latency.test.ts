/**
 * How fast a live session starts and ends, and that speed loses nothing: the first word of a recording and
 * the last word before Stop are both stored, once. Fake recogniser (scripted words), real route, hub and
 * session.
 */
import { afterEach, describe, expect, setDefaultTimeout, test } from "bun:test"
import { Effect } from "effect"
import { encodeFakeSpeech } from "../src/live/providers/fake"
import { makeElevenLabsRealtime } from "../src/live/providers/elevenlabs-realtime"
import { LiveProviderError } from "../src/live/providers/queue"
import { liveFixture, RATE, transcriptText, users, type LiveFixture } from "./live-support"

setDefaultTimeout(30_000)
const open: LiveFixture[] = []
afterEach(async () => { for (const fixture of open.splice(0)) await fixture.dispose() })
async function fixture(options: Parameters<typeof liveFixture>[0] = {}) { const made = await liveFixture(options); open.push(made); return made }
const stream = (resumeFromSample = 0, baseMs = 0) => [{ channel: "mic" as const, sampleRate: RATE, encoding: "pcm_s16le" as const, baseMs, resumeFromSample }]

describe("starting and stopping quickly", () => {
  test("speech from the first sample to the last: the first and last words are stored once, and the end is quick", async () => {
    // Speech starts at the very first sample and runs to the last one: Stop comes mid-sentence's end, with no quiet after it.
    const words = "alpha bravo charlie delta echo foxtrot golf hotel"
    const script = encodeFakeSpeech([{ atMs: 0, text: words }], { gapMs: 40 })
    const f = await fixture({ script, fake: { interimDelayMs: 30, finalDelayMs: 300 } })
    const noteId = await f.recordingNote()
    const client = f.connect(noteId)
    const asked = performance.now()
    const ready = await client.hello(stream())
    const readyMs = performance.now() - asked
    expect(ready).toMatchObject({ type: "ready", streams: [{ channel: "mic", acceptFromSample: 0 }] })
    const end = script.pcm.byteLength / 2
    // In real time, as a capture delivers it.
    await client.audio(script.pcm, 0, end, { channel: "mic", speed: 1 })
    expect(client.of("interim").length).toBeGreaterThan(0)
    const ending = performance.now()
    client.mark("end", { mic: end })
    await client.until(() => client.closed, "close after end")
    const endMs = performance.now() - ending
    expect(client.closed!.code).toBe(1000)
    // The flush is answered after 120 ms (the fake answers a flush at most that late); the session ends with it,
    // not at a later look.
    expect(endMs).toBeLessThan(400)
    expect(readyMs).toBeLessThan(300)
    const stored = await f.note(noteId)
    expect(transcriptText(stored, "mic")).toBe(words)
    expect(client.lastAck("mic")).toBe(end)
  })

  test("Stop and at once Resume: the new capture takes the held provider session, and both are stored once", async () => {
    const first = encodeFakeSpeech([{ atMs: 0, text: "kilo lima mike" }], { durationMs: 1200 })
    const f = await fixture({ script: first, fake: { finalDelayMs: 100 } })
    const noteId = await f.recordingNote()
    const a = f.connect(noteId)
    await a.hello(stream(), "capture-a")
    const aEnd = first.pcm.byteLength / 2
    await a.audio(first.pcm, 0, aEnd, { channel: "mic", speed: 4 })
    a.mark("end", { mic: aEnd })
    await a.until(() => a.closed, "first close")
    const scoped = f.processing.forRepository(f.repository.forUser(users.alice))
    await Effect.runPromise(scoped.recording(noteId, "stop"))
    await Effect.runPromise(scoped.recording(noteId, "resume"))
    const b = f.connect(noteId)
    const asked = performance.now()
    await b.hello(stream(0, 1200), "capture-b")
    const readyMs = performance.now() - asked
    // The same scripted words again, as the second capture: the held provider session hears them.
    await b.audio(first.pcm, 0, aEnd, { channel: "mic", speed: 4 })
    b.mark("end", { mic: aEnd })
    await b.until(() => b.closed, "second close")
    expect(f.fake.control.opened).toBe(1)
    expect(readyMs).toBeLessThan(200)
    expect(transcriptText(await f.note(noteId), "mic")).toBe("kilo lima mike kilo lima mike")
  })

  test("start, stop, start, stop as fast as messages go never wedges a note", async () => {
    const script = encodeFakeSpeech([{ atMs: 0, text: "november oscar" }], { durationMs: 700 })
    const f = await fixture({ script, fake: { finalDelayMs: 50 } })
    const noteId = await f.recordingNote()
    const scoped = f.processing.forRepository(f.repository.forUser(users.alice))
    const end = script.pcm.byteLength / 2
    for (let round = 0; round < 4; round++) {
      if (round) await Effect.runPromise(scoped.recording(noteId, "resume"))
      const client = f.connect(noteId)
      expect(await client.hello(stream(0, round * 700), `capture-${round}`)).toMatchObject({ type: "ready" })
      await client.audio(script.pcm, 0, end, { channel: "mic", speed: 20 })
      client.mark("end", { mic: end })
      // The HTTP Stop at once, racing the end of the socket.
      await Effect.runPromise(scoped.recording(noteId, "stop"))
      await client.until(() => client.closed, `close ${round}`)
    }
    expect(f.hub.active(noteId)).toBe(false)
    expect(transcriptText(await f.note(noteId), "mic")).toBe(Array(4).fill("november oscar").join(" "))
  })
})

describe("a provider at its limit", () => {
  test("an open refused for the account's limit says so, so the hub can let held sessions go at once", async () => {
    // The service refuses before the session starts, and closing the socket reports the close at once (as Bun's does).
    class Refusing {
      onmessage: ((event: { data: string }) => void) | null = null
      onerror: (() => void) | null = null
      onclose: ((event: { code: number }) => void) | null = null
      bufferedAmount = 0
      constructor() { queueMicrotask(() => this.onmessage?.({ data: JSON.stringify({ message_type: "rate_limited" }) })) }
      send() {}
      close() { this.onclose?.({ code: 1000 }) }
    }
    const transcriber = makeElevenLabsRealtime({ apiKey: () => "test-key", model: "scribe_v3_live", webSocket: Refusing as never })
    const refusal = await transcriber.open({ channel: "mic", sampleRate: RATE, signal: new AbortController().signal }).catch(error => error)
    expect(refusal).toBeInstanceOf(LiveProviderError)
    expect(refusal.reason).toBe("over_limit")
  })
})
