/**
 * Speaker labels across a Stop and a Resume (src/live/link.ts, src/live/voices.ts).
 *
 * The fake recogniser numbers speakers per provider session in the order it first hears them
 * (`sessionSpeakers`), as the real one does, so a new provider session gives the same person another id
 * unless the person happens to speak first. The fake batch recogniser knows who really speaks (the
 * script's speaker names), which is what the comparison of voices relies on.
 */
import { afterEach, describe, expect, setDefaultTimeout, test } from "bun:test"
import { Effect } from "effect"
import { encodeFakeSpeech } from "../src/live/providers/fake"
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { judgeVoices, makeVoiceMatcher } from "../src/live/voices"
import { liveFixture, RATE, users, wav, type LiveClient, type LiveFixture } from "./live-support"

setDefaultTimeout(30_000)
const open: LiveFixture[] = []
afterEach(async () => { for (const fixture of open.splice(0)) await fixture.dispose() })
async function fixture(options: Parameters<typeof liveFixture>[0] = {}) { const made = await liveFixture({ ...options, fake: { sessionSpeakers: true, ...options.fake } }); open.push(made); return made }
const samples = (ms: number) => Math.round(ms * RATE / 1000)
const streams = (channels: ReadonlyArray<"mic" | "system">, baseMs = 0) => channels.map(channel => ({ channel, sampleRate: RATE, encoding: "pcm_s16le" as const, baseMs, resumeFromSample: 0 }))
type Line = { atMs: number; text: string; speaker: string; channel?: "mic" | "system" }
/** One script for the whole meeting; each channel hears only its own lines. */
function meeting(lines: Line[], durationMs: number) {
  const script = encodeFakeSpeech(lines, { durationMs })
  const pcm = (channel: "mic" | "system") => {
    const out = script.pcm.slice()
    for (const [index, line] of [...lines].sort((a, b) => a.atMs - b.atMs).entries()) if ((line.channel ?? "system") !== channel) out.fill(0, script.lines[index]!.firstSample * 2, (script.lines[index]!.lastSample + 1) * 2)
    return out
  }
  return { script, system: pcm("system"), mic: pcm("mic") }
}
/** Eleven words: 3.2 s of speech, enough for a voice to be compared. */
const ten = (name: string) => `${name} one two three four five six seven eight nine ten`
/** Streams note time [fromMs, toMs) of the given channels as one capture that starts at `fromMs`, then ends it with Stop. */
async function capture(f: LiveFixture, noteId: string, audio: ReturnType<typeof meeting>, channels: ReadonlyArray<"mic" | "system">, fromMs: number, toMs: number, captureId: string, options: { uploadParts?: boolean; stop?: boolean } = {}) {
  const client = f.connect(noteId)
  const ready = await client.hello(streams(channels, fromMs), captureId)
  if (ready === "closed") throw new Error(`hello refused: ${JSON.stringify(client.closed)}`)
  await Promise.all(channels.map(channel => client.audio(audio[channel], samples(fromMs), samples(toMs), { channel, speed: 12, offset: -samples(fromMs) })))
  await client.until(() => channels.every(channel => client.lastAck(channel) === samples(toMs - fromMs)), "the server has the audio")
  // The durable parts of the same audio, as a client uploads them while it records.
  if (options.uploadParts) for (const [index, channel] of channels.entries()) await f.uploadPart(noteId, audio[channel].subarray(samples(fromMs) * 2, samples(toMs) * 2), fromMs, Math.round(fromMs / 1000) * 2 + index, channel)
  if (options.stop !== false) {
    client.mark("end", Object.fromEntries(channels.map(channel => [channel, samples(toMs - fromMs)])))
    await Effect.runPromise(f.processing.forRepository(f.repository.forUser(users.alice)).recording(noteId, "stop"))
    await client.until(() => client.closed, "close after Stop")
  }
  return client
}
const resume = (f: LiveFixture, noteId: string) => Effect.runPromise(f.processing.forRepository(f.repository.forUser(users.alice)).recording(noteId, "resume"))
/** "speaker label: first word" of every stored segment of a channel, in time order. */
const labels = async (f: LiveFixture, noteId: string, channel: "mic" | "system" = "system") => (await f.note(noteId)).transcriptSegments.filter(segment => segment.channel === channel).map(segment => `${segment.speakerLabel}: ${segment.text.split(" ")[0]}`)
const merges = (client: LiveClient) => client.of("speakers").flatMap(message => message.merges)

const twoThenTwo = () => meeting([
  { atMs: 200, text: ten("arjun"), speaker: "arjun" }, { atMs: 4000, text: ten("priya"), speaker: "priya" },
  // After the Resume the other person speaks first, so a new provider session calls her speaker_0.
  { atMs: 10_200, text: ten("priya"), speaker: "priya" }, { atMs: 14_000, text: ten("arjun"), speaker: "arjun" },
], 18_000)

describe("rule 1: the provider session is held across Stop and Resume", () => {
  test("a Resume within the hold takes the provider session over: same voices, same labels, right times", async () => {
    const audio = meeting([
      { atMs: 200, text: ten("arjun"), speaker: "arjun" }, { atMs: 4000, text: ten("priya"), speaker: "priya" },
      { atMs: 10_200, text: ten("priya"), speaker: "priya" }, { atMs: 14_000, text: ten("arjun"), speaker: "arjun" }, { atMs: 18_000, text: ten("noemi"), speaker: "noemi" },
    ], 22_000)
    // Keep-alives every 40 ms while held: the provider's clock moves on, and the takeover has to allow for it.
    const f = await fixture({ script: audio.script, config: { keepAliveMs: 40, holdMs: 5000 }, voices: false })
    const noteId = await f.recordingNote()
    await capture(f, noteId, audio, ["system"], 0, 8000, "capture-1")
    expect(f.hub.active(noteId)).toBe(false)
    expect(f.hub.holding).toBe(1)
    expect(f.fake.control.active).toBe(1)
    const heard = f.fake.control.audioSeconds
    await Bun.sleep(300)
    expect(f.fake.control.audioSeconds).toBeGreaterThan(heard + 0.3)
    await resume(f, noteId)
    const second = await capture(f, noteId, audio, ["system"], 10_000, 22_000, "capture-2")
    expect(f.fake.control.opened).toBe(1)
    expect(await labels(f, noteId)).toEqual(["Speaker 1: arjun", "Speaker 2: priya", "Speaker 2: priya", "Speaker 1: arjun", "Speaker 3: noemi"])
    expect(merges(second)).toEqual([])
    const stored = await f.note(noteId)
    // Times are note time although the provider's clock ran on through the hold.
    expect(stored.transcriptSegments.map(segment => Math.round(segment.startMs / 100) * 100)).toEqual([200, 4000, 10_200, 14_000, 18_000])
    for (const segment of second.finals()) expect(stored.transcriptSegments.find(item => item.id === segment.segment.id)).toEqual(segment.segment)
    // The second Stop holds the provider session again.
    expect(f.hub.holding).toBe(1)
  })
  test("the hold ends after holdMs, and at once when the hub is disposed", async () => {
    const audio = twoThenTwo(), f = await fixture({ script: audio.script, config: { holdMs: 200 }, voices: false })
    const noteId = await f.recordingNote()
    await capture(f, noteId, audio, ["system"], 0, 8000, "capture-1")
    expect(f.hub.holding).toBe(1)
    await Bun.sleep(900)
    expect(f.hub.holding).toBe(0)
    expect(f.fake.control.active).toBe(0)
    // With nothing held and nothing to compare with, two voices before means the new session numbers on.
    await resume(f, noteId)
    await capture(f, noteId, audio, ["system"], 10_000, 18_000, "capture-2")
    expect(f.fake.control.opened).toBe(2)
    expect(await labels(f, noteId)).toEqual(["Speaker 1: arjun", "Speaker 2: priya", "Speaker 3: priya", "Speaker 4: arjun"])
  })
  test("a held provider session that died is not taken over, and holds give way to a running recording", async () => {
    const audio = twoThenTwo(), f = await fixture({ script: audio.script, config: { holdMs: 5000 }, maxSessions: 1, voices: false })
    const noteId = await f.recordingNote(), other = await f.recordingNote()
    await capture(f, noteId, audio, ["system"], 0, 8000, "capture-1")
    expect(f.hub.holding).toBe(1)
    // Another note starts recording and the hub is at its limit: the hold is given up for it.
    const busy = f.connect(other)
    expect(await busy.hello(streams(["system"]), "other-capture")).not.toBe("closed")
    expect(f.hub.holding).toBe(0)
    expect(f.fake.control.active).toBe(1)
    busy.mark("end")
    await busy.until(() => busy.closed, "close")
    expect(f.hub.holding).toBe(1)
    // The provider drops the held session: nothing is left to take over.
    f.fake.control.fail()
    await Bun.sleep(700)
    expect(f.hub.holding).toBe(0)
  })
})

describe("rule 2: the channel rule, when there is nothing to compare with", () => {
  test("one voice before and one after is the same speaker; a further voice is a new one", async () => {
    const audio = meeting([
      { atMs: 200, text: ten("me"), speaker: "me", channel: "mic" }, { atMs: 4000, text: ten("arjun"), speaker: "arjun" },
      { atMs: 10_200, text: ten("me"), speaker: "me", channel: "mic" }, { atMs: 14_000, text: ten("guest"), speaker: "guest", channel: "mic" }, { atMs: 18_000, text: ten("arjun"), speaker: "arjun" },
    ], 22_000)
    const f = await fixture({ script: audio.script, config: { holdMs: 0 }, voices: false })
    const noteId = await f.recordingNote()
    await capture(f, noteId, audio, ["mic", "system"], 0, 8000, "capture-1")
    expect(f.hub.holding).toBe(0)
    await resume(f, noteId)
    const second = await capture(f, noteId, audio, ["mic", "system"], 10_000, 22_000, "capture-2")
    expect(f.fake.control.opened).toBe(4)
    expect(await labels(f, noteId, "mic")).toEqual(["Speaker 1: me", "Speaker 1: me", "Speaker 2: guest"])
    expect(await labels(f, noteId, "system")).toEqual(["Speaker 1: arjun", "Speaker 1: arjun"])
    expect(merges(second)).toEqual([])
    expect(f.voiceCalls).toHaveLength(0)
  })
  test("the guess is wrong when the person at the microphone changed and no stored audio can say so", async () => {
    const audio = meeting([{ atMs: 200, text: ten("me"), speaker: "me", channel: "mic" }, { atMs: 10_200, text: ten("guest"), speaker: "guest", channel: "mic" }], 14_000)
    // The matcher is configured, but no durable part of the first capture has been uploaded.
    const f = await fixture({ script: audio.script, config: { holdMs: 0 } })
    const noteId = await f.recordingNote()
    await capture(f, noteId, audio, ["mic"], 0, 8000, "capture-1")
    await resume(f, noteId)
    await capture(f, noteId, audio, ["mic"], 10_000, 14_000, "capture-2")
    // This is what the user sees in that case: one label for two people.
    expect(await labels(f, noteId, "mic")).toEqual(["Speaker 1: me", "Speaker 1: guest"])
  })
})

describe("rule 3: a new provider session's voices are compared with the earlier ones", () => {
  test("two voices before and the same two after keep their labels, whoever speaks first", async () => {
    const audio = twoThenTwo(), f = await fixture({ script: audio.script, config: { holdMs: 0 } })
    const noteId = await f.recordingNote()
    await capture(f, noteId, audio, ["system"], 0, 8000, "capture-1", { uploadParts: true })
    await resume(f, noteId)
    const second = await capture(f, noteId, audio, ["system"], 10_000, 18_000, "capture-2")
    expect(f.fake.control.opened).toBe(2)
    expect(await labels(f, noteId)).toEqual(["Speaker 1: arjun", "Speaker 2: priya", "Speaker 2: priya", "Speaker 1: arjun"])
    // Decided before the text was stored: no label was shown and then changed.
    expect(merges(second)).toEqual([])
    expect(second.finals().map(message => message.segment.speakerLabel)).toEqual(["Speaker 2", "Speaker 1"])
    expect(f.voiceCalls.length).toBeGreaterThanOrEqual(1)
    // Once both earlier voices are accounted for nothing more is asked.
    expect(f.voiceCalls.length).toBeLessThanOrEqual(2)
  })
  test("a third person who joins after the Resume gets a new label, also when they speak first", async () => {
    const audio = meeting([
      { atMs: 200, text: ten("arjun"), speaker: "arjun" }, { atMs: 4000, text: ten("priya"), speaker: "priya" },
      { atMs: 10_200, text: ten("noemi"), speaker: "noemi" }, { atMs: 14_000, text: ten("priya"), speaker: "priya" }, { atMs: 18_000, text: ten("arjun"), speaker: "arjun" },
    ], 22_000)
    const f = await fixture({ script: audio.script, config: { holdMs: 0 } })
    const noteId = await f.recordingNote()
    await capture(f, noteId, audio, ["system"], 0, 8000, "capture-1", { uploadParts: true })
    await resume(f, noteId)
    const second = await capture(f, noteId, audio, ["system"], 10_000, 22_000, "capture-2")
    expect(await labels(f, noteId)).toEqual(["Speaker 1: arjun", "Speaker 2: priya", "Speaker 3: noemi", "Speaker 2: priya", "Speaker 1: arjun"])
    expect(merges(second)).toEqual([])
  })
  test("the microphone speaker changed: the stored audio shows it, and the new person gets a new label", async () => {
    const audio = meeting([
      { atMs: 200, text: ten("me"), speaker: "me", channel: "mic" },
      { atMs: 10_200, text: ten("guest"), speaker: "guest", channel: "mic" }, { atMs: 14_000, text: ten("me"), speaker: "me", channel: "mic" },
    ], 18_000)
    const f = await fixture({ script: audio.script, config: { holdMs: 0 } })
    const noteId = await f.recordingNote()
    await capture(f, noteId, audio, ["mic"], 0, 8000, "capture-1", { uploadParts: true })
    await resume(f, noteId)
    const second = await capture(f, noteId, audio, ["mic"], 10_000, 18_000, "capture-2")
    expect(await labels(f, noteId, "mic")).toEqual(["Speaker 1: me", "Speaker 2: guest", "Speaker 1: me"])
    expect(merges(second)).toEqual([])
  })
  test("a voice that has said too little is numbered on, then renamed through a speakers message once it is recognised", async () => {
    const audio = meeting([
      { atMs: 200, text: ten("arjun"), speaker: "arjun" }, { atMs: 4000, text: ten("priya"), speaker: "priya" },
      { atMs: 10_200, text: "priya agrees", speaker: "priya" }, { atMs: 12_000, text: ten("priya"), speaker: "priya" }, { atMs: 16_000, text: ten("arjun"), speaker: "arjun" },
    ], 20_000)
    const f = await fixture({ script: audio.script, config: { holdMs: 0 } })
    const noteId = await f.recordingNote()
    await capture(f, noteId, audio, ["system"], 0, 8000, "capture-1", { uploadParts: true })
    await resume(f, noteId)
    const second = await capture(f, noteId, audio, ["system"], 10_000, 20_000, "capture-2")
    // Two words are not enough to compare: shown as a new voice first, never under somebody else's label.
    expect(second.finals()[0]!.segment).toMatchObject({ text: "priya agrees", speakerLabel: "Speaker 3" })
    expect(merges(second)).toEqual([{ from: "system:n:3", to: "system:n:2" }])
    expect(await labels(f, noteId)).toEqual(["Speaker 1: arjun", "Speaker 2: priya", "Speaker 2: priya", "Speaker 2: priya", "Speaker 1: arjun"])
    // The number given up is not handed to anybody else.
    expect(second.finals().map(message => message.segment.speakerLabel)).toEqual(["Speaker 3", "Speaker 2", "Speaker 1"])
    // The voices a later provider session would be compared with: the merged number is gone.
    expect(await Effect.runPromise(f.repository.live!.voices(noteId, "system"))).toEqual([1, 2])
  })
})

describe("the decision from the batch speaker turns", () => {
  const spoken = (entries: Record<string, Array<[number, number]>>) => new Map(Object.entries(entries).map(([who, list]) => [who, list.map(([fromMs, toMs]) => ({ fromMs, toMs }))]))
  const turns = (list: Array<[string, number, number]>) => list.map(([speaker, fromMs, toMs]) => ({ speaker, fromMs, toMs }))
  test("a match needs one earlier voice in the batch speaker and no other earlier voice near it", () => {
    const where = spoken({ "old:1": [[0, 10_000]], "old:2": [[11_000, 21_000]], "new:x": [[22_000, 26_000]], "new:y": [[27_000, 31_000]] })
    expect(judgeVoices(turns([["A", 0, 10_000], ["B", 11_000, 21_000], ["B", 22_000, 26_000], ["C", 27_000, 31_000]]), where, [1, 2], ["x", "y"])).toEqual({ x: 2, y: "new" })
    // Both earlier voices in one batch speaker: the recogniser cannot tell them apart, so it proves nothing.
    expect(judgeVoices(turns([["A", 0, 21_000], ["A", 22_000, 26_000], ["C", 27_000, 31_000]]), where, [1, 2], ["x", "y"])).toEqual({ x: "unsure", y: "new" })
    // A quarter of voice 1 is in B as well: B is not cleanly voice 2.
    expect(judgeVoices(turns([["A", 0, 7500], ["B", 7500, 10_000], ["B", 11_000, 21_000], ["B", 22_000, 26_000]]), where, [1, 2], ["x"])).toEqual({ x: "unsure" })
    // The new voice is spread over two batch speakers.
    expect(judgeVoices(turns([["A", 0, 10_000], ["B", 11_000, 21_000], ["B", 22_000, 24_000], ["A", 24_000, 26_000]]), where, [1, 2], ["x"])).toEqual({ x: "unsure" })
  })
  test("\"new\" needs every earlier voice to be placed", () => {
    const where = spoken({ "old:1": [[0, 10_000]], "old:2": [[11_000, 12_000]], "new:x": [[22_000, 26_000]], "new:y": [[27_000, 31_000]] })
    // Voice 2 has only a second of stored audio: the stranger could be voice 2.
    expect(judgeVoices(turns([["A", 0, 10_000], ["B", 11_000, 12_000], ["C", 22_000, 26_000]]), where, [1, 2], ["x"])).toEqual({ x: "unsure" })
    // Too little of the new voice was attributed.
    expect(judgeVoices(turns([["A", 0, 10_000], ["A", 22_000, 23_000]]), where, [1], ["x"])).toEqual({ x: "unsure" })
  })
  test("the comparison reads the stored part at its own sample rate, and never gives one earlier voice to two new voices", async () => {
    const dir = await mkdtemp(join(tmpdir(), "voices-test-"))
    try {
      await mkdir(join(dir, "note"))
      await writeFile(join(dir, "note", "part.wav"), wav(new Uint8Array(48_000 * 2 * 10), 48_000))
      const files: number[] = []
      // A batch recogniser that hears one speaker in everything.
      const matcher = makeVoiceMatcher({ storageDir: dir, segments: async () => [{ channel: "system", speakerId: "system:n:1", startMs: 0, endMs: 9000 }], parts: async () => [{ channel: "system", startMs: 0, durationMs: 10_000, hash: "part" }],
        transcribe: async input => { files.push(input.durationMs); return [{ speakerId: "system:voices:speaker_1", startMs: 0, endMs: input.durationMs }] } })
      const fresh = { pcm: new Uint8Array(RATE * 2 * 10), sampleRate: RATE, startMs: 20_000 }, x = [{ fromMs: 20_000, toMs: 24_000 }], y = [{ fromMs: 25_000, toMs: 29_000 }]
      expect(await matcher.match({ noteId: "note", channel: "system", candidates: [1], fresh, voices: { x } })).toEqual({ x: 1 })
      // 9 s of the earlier voice and 4 s of the new one, in pieces with pauses between them.
      expect(files[0]).toBeGreaterThan(13_000)
      expect(files[0]).toBeLessThan(16_000)
      // The streaming recogniser hears two voices, the batch one says both are voice 1: one of them is wrong, so neither is merged.
      expect(await matcher.match({ noteId: "note", channel: "system", candidates: [1], fresh, voices: { x, y } })).toEqual({ x: "unsure", y: "unsure" })
      // No stored audio for the earlier voice: no answer at all, and no call.
      const calls = files.length
      expect(await matcher.match({ noteId: "other", channel: "system", candidates: [1], fresh, voices: { x } })).toEqual({})
      expect(files).toHaveLength(calls)
    } finally { await rm(dir, { recursive: true, force: true }) }
  })
})
