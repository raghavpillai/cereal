import { expect, test } from "bun:test"
import type { LiveTranscriberEvent } from "@meeting-notes/contracts/live"
import { BURST_SECONDS, makeElevenLabsRealtime } from "../src/live/providers/elevenlabs-realtime"

const RATE = 16000
/** Stands in for the provider's socket: records what is sent and when, and answers as scripted. */
class FakeSocket {
  static last: FakeSocket | undefined
  readonly sent: Array<{ at: number; message: Record<string, unknown> }> = []
  bufferedAmount = 0
  onmessage: ((event: { data: string }) => void) | null = null
  onerror: (() => void) | null = null
  onclose: ((event: { code: number }) => void) | null = null
  constructor(readonly url: string, _options: unknown) { FakeSocket.last = this; queueMicrotask(() => this.say({ message_type: "session_started" })) }
  say(message: Record<string, unknown>) { this.onmessage?.({ data: JSON.stringify(message) }) }
  send(text: string) { this.sent.push({ at: performance.now(), message: JSON.parse(text) }) }
  close() {}
  audio() { return this.sent.filter(item => item.message.message_type === "input_audio_chunk" && item.message.commit === false) }
}
const open = async (pace: number) => {
  const transcriber = makeElevenLabsRealtime({ apiKey: () => "test-key", model: "scribe_v3_live", pace, webSocket: FakeSocket as never })
  const session = await transcriber.open({ channel: "mic", sampleRate: RATE, signal: new AbortController().signal })
  const events: LiveTranscriberEvent[] = []
  void (async () => { for await (const event of session.events) events.push(event) })()
  return { session, socket: FakeSocket.last!, events }
}
const tone = (samples: number, from: number) => { const pcm = new Uint8Array(samples * 2), view = new DataView(pcm.buffer); for (let i = 0; i < samples; i++) view.setInt16(i * 2, ((from + i) % 2000) - 1000, true); return pcm }

test("a 60 s backlog arriving at once is forwarded whole, in order, and never faster than the pace allows", async () => {
  // The pace is raised for the test (the real one is 4x); the shape of the limit is what is checked.
  const pace = 300, { session, socket } = await open(pace)
  const started = performance.now(), total = 60 * RATE
  for (let at = 0; at < total; at += RATE / 10) session.send(tone(RATE / 10, at))
  session.flush()
  for (const deadline = performance.now() + 5000; socket.audio().reduce((sum, item) => sum + Buffer.from(String(item.message.audio_base_64), "base64").byteLength / 2, 0) < total && performance.now() < deadline;) await Bun.sleep(10)
  let forwarded = 0
  const received: Buffer[] = []
  for (const item of socket.audio()) {
    const bytes = Buffer.from(String(item.message.audio_base_64), "base64")
    forwarded += bytes.byteLength / 2; received.push(bytes)
    // Never ahead of the pace: what has gone by each moment is at most the burst plus pace x the time since.
    expect(forwarded).toBeLessThanOrEqual(RATE * BURST_SECONDS + (item.at - started + 25) / 1000 * RATE * pace)
  }
  expect(forwarded).toBe(total)
  const whole = Buffer.concat(received), expected = Buffer.concat(Array.from({ length: 600 }, (_, index) => Buffer.from(tone(RATE / 10, index * RATE / 10))))
  expect(whole.equals(expected)).toBe(true)
  // The commit goes in band, after all the audio sent before it.
  const commitAt = socket.sent.findIndex(item => item.message.commit === true)
  expect(commitAt).toBe(socket.sent.length - 1)
  await session.close()
})

test("after queue_overflow no commit is trusted past its words, and the session reports a retryable failure", async () => {
  const { session, socket, events } = await open(4)
  session.send(tone(RATE * 2, 0))
  session.flush()
  await Bun.sleep(50)
  // The service drops audio and says so; a commit for the start of it follows.
  socket.say({ message_type: "queue_overflow" })
  socket.say({ message_type: "committed_transcript", text: "hello", segment_index: 0 })
  socket.say({ message_type: "committed_transcript_with_timestamps", text: "hello", words: [{ type: "word", text: "hello", start: 0.1, end: 0.4, speaker_id: "speaker_0" }] })
  await Bun.sleep(50)
  expect(events.find(event => event.type === "error")).toMatchObject({ type: "error", retryable: true, reason: "over_limit" })
  for (const event of events) if (event.type === "final") expect(event.finalThroughSec).toBeLessThanOrEqual(0.4)
  await session.close()
})

test("a keep-alive is an audio chunk with no samples: nothing is spliced into what was being said, and the clock does not move", async () => {
  // A stream that stops mid-word (a network drop) and resumes: 100 ms of keep-alive silence between the two
  // halves of the word made the recogniser write it twice, clipped ("this- this-"), measured 2026-11-17.
  const { session, socket } = await open(4)
  session.send(tone(RATE / 10, 0))
  for (const deadline = performance.now() + 2000; socket.audio().length < 1 && performance.now() < deadline;) await Bun.sleep(5)
  expect(session.keepAlive()).toBe(0)
  session.send(tone(RATE / 10, RATE / 10))
  for (const deadline = performance.now() + 2000; socket.audio().filter(item => item.message.audio_base_64).length < 2 && performance.now() < deadline;) await Bun.sleep(5)
  const chunks = socket.audio().map(item => Buffer.from(String(item.message.audio_base_64), "base64"))
  expect(chunks.map(chunk => chunk.byteLength)).toEqual([RATE / 10 * 2, 0, RATE / 10 * 2])
  expect(Buffer.concat(chunks).equals(Buffer.concat([Buffer.from(tone(RATE / 10, 0)), Buffer.from(tone(RATE / 10, RATE / 10))]))).toBe(true)
  // Audio still waiting for its pace keeps the socket busy: no keep-alive is needed then.
  const slow = await open(0.001)
  slow.session.send(tone(RATE * 3, 0))
  const before = slow.socket.sent.length
  expect(slow.session.keepAlive()).toBe(0)
  expect(slow.socket.sent.length).toBe(before)
  await session.close(); await slow.session.close()
})

test("a flush with less than 0.3 s of audio since the last commit is answered here: the service would close the session", async () => {
  // "commit_throttled: only 0.00s of uncommitted audio": a held provider session was lost on every takeover.
  const { session, socket, events } = await open(1000)
  session.send(tone(RATE * 2, 0))
  session.flush()
  for (const deadline = performance.now() + 2000; !socket.sent.some(item => item.message.commit === true) && performance.now() < deadline;) await Bun.sleep(5)
  const commits = () => socket.sent.filter(item => item.message.commit === true).length
  expect(commits()).toBe(1)
  // Nothing new: answered at once, without a commit.
  session.flush()
  await Bun.sleep(20)
  expect(commits()).toBe(1)
  expect(events.filter(event => event.type === "final" && event.flushed).length).toBe(1)
  // 0.2 s more is still too little; a second more is committed.
  session.send(tone(RATE / 5, 0)); session.flush(); await Bun.sleep(20)
  expect(commits()).toBe(1)
  session.send(tone(RATE, 0)); session.flush()
  for (const deadline = performance.now() + 2000; commits() < 2 && performance.now() < deadline;) await Bun.sleep(5)
  expect(commits()).toBe(2)
  // The service's own commit counts too: right after its transcript, a flush has nothing to commit.
  socket.say({ message_type: "committed_transcript", text: "hello", segment_index: 0 })
  session.flush(); await Bun.sleep(20)
  expect(commits()).toBe(2)
  await session.close()
})
