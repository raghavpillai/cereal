import { afterEach, expect, test } from "bun:test"
import { liveSpeakerLabel } from "../src/live/coverage"
import { encodeFakeSpeech } from "../src/live/providers/fake"
import { attributeWords, canonicalSpeaker, MIC_SECOND_VOICE_MS, ME, newAttribution, UNKNOWN, type HeardWord } from "../src/live/speakers"
import { liveFixture, RATE, type LiveFixture } from "./live-support"

const open: LiveFixture[] = []
afterEach(async () => { for (const fixture of open.splice(0)) await fixture.dispose() })
/** Words of `seconds` each, back to back from `fromMs`. */
const said = (fromMs: number, speaker: string | undefined, text: string, ms = 300): HeardWord[] =>
  text.split(" ").map((word, index) => ({ text: `${word} `, startMs: fromMs + index * ms, endMs: fromMs + (index + 1) * ms, ...(speaker === undefined ? {} : { speaker }) }))
/** Stands in for `planAppend`: numbers every key it has not seen, in order. */
const number = (speakers: Record<string, number>, keys: string[]) => { for (const key of keys) speakers[key] ??= Math.max(0, ...Object.values(speakers)) + 1 }

test("words the diariser gave no speaker are never a voice of their own", () => {
  for (const channel of ["system", "mic"] as const) {
    const state = newAttribution(), speakers: Record<string, number> = {}
    // The first utterance of a provider session comes back without speakers (measured with Scribe v3 Live).
    const first = attributeWords(channel, state, said(0, undefined, "Test."), speakers).turns
    number(speakers, first.map(turn => turn.speakerKey))
    const second = attributeWords(channel, state, [...said(1700, "speaker_0", "Test, test."), ...said(2400, undefined, "test.")], speakers).turns
    number(speakers, second.map(turn => turn.speakerKey))
    const numbers = [...first, ...second].map(turn => speakers[turn.speakerKey])
    expect(new Set(numbers), channel).toEqual(new Set([1]))
    expect(second, channel).toHaveLength(1)
  }
})

test("unknown words take the speaker beside them, or whoever spoke last", () => {
  const state = newAttribution(), speakers: Record<string, number> = { speaker_0: 1, speaker_1: 2 }
  const turns = attributeWords("system", state, [...said(0, undefined, "so"), ...said(300, "speaker_1", "we ship"), ...said(900, undefined, "friday")], speakers).turns
  expect(turns.map(turn => [turn.speakerKey, turn.words.length])).toEqual([["speaker_1", 4]])
  expect(attributeWords("system", state, said(5000, undefined, "right"), speakers).turns.map(turn => turn.speakerKey)).toEqual(["speaker_1"])
})

test("on the microphone a stray diariser id is Me; a second voice is trusted on evidence gathered across finals", () => {
  const state = newAttribution(), speakers: Record<string, number> = {}
  const mine = attributeWords("mic", state, said(0, "speaker_0", "Test test"), speakers).turns
  // A short stretch the diariser gives another id, as in the screenshot: still Me.
  const stray = attributeWords("mic", state, said(1000, "speaker_1", "test test test"), speakers)
  expect([...mine, ...stray.turns].map(turn => turn.speakerKey)).toEqual([ME, ME])
  expect(stray.trusted).toEqual([])
  // Someone else at the same microphone: no single final is long, but together they are (the validator's
  // 8.7 s, 30-word turn arrived as several finals).
  const first = attributeWords("mic", state, said(4000, "speaker_2", "so the budget review moved", 500), speakers)
  expect(first.turns.map(turn => [turn.speakerKey, turn.source])).toEqual([[ME, "speaker_2"]])
  const second = attributeWords("mic", state, said(7000, "speaker_2", "to thursday and the contract", 500), speakers)
  expect(second.trusted).toEqual(["speaker_2"])
  expect(second.turns.map(turn => turn.speakerKey)).toEqual(["speaker_2"])
  // One long turn is enough on its own.
  const other = newAttribution()
  attributeWords("mic", other, said(0, "speaker_0", "hello"), {})
  const long = said(1000, "speaker_1", "so the budget review moved to thursday and the contract goes out tomorrow morning", 400)
  expect(long.at(-1)!.endMs - long[0]!.startMs).toBeGreaterThanOrEqual(MIC_SECOND_VOICE_MS)
  expect(attributeWords("mic", other, long, {}).trusted).toEqual(["speaker_1"])
  expect(canonicalSpeaker("mic", state, "speaker_1")).toBe(ME)
  expect(canonicalSpeaker("mic", state, undefined)).toBe(ME)
  expect(canonicalSpeaker("system", state, undefined)).toBeUndefined()
})

test("a new provider session's first voice takes over a placeholder carried over for the channel's only voice", () => {
  const state = newAttribution(), speakers: Record<string, number> = { [UNKNOWN]: 4 }
  state.placeholder = true
  attributeWords("system", state, said(0, "speaker_0", "still me"), speakers).turns
  expect(speakers.speaker_0).toBe(4)
  // Only the first: a later new voice is numbered afresh.
  attributeWords("system", state, said(2000, "speaker_1", "and me"), speakers).turns
  expect(speakers.speaker_1).toBeUndefined()
})

test("labels: the microphone is Me, other microphone voices are room speakers, system audio is numbered", () => {
  expect(liveSpeakerLabel("mic", 1)).toBe("Me")
  expect(liveSpeakerLabel("mic", 2)).toBe("Room speaker 1")
  expect(liveSpeakerLabel("system", 1)).toBe("Speaker 1")
  expect(liveSpeakerLabel("system", 3)).toBe("Speaker 3")
})

test("end to end: one person, first words unattributed, is one speaker on either channel", async () => {
  // The fake reports no speaker for words of a line without one, as the real diariser does for its first utterance.
  const script = encodeFakeSpeech([{ atMs: 200, text: "test" }, { atMs: 1500, text: "test test", speaker: "a" }, { atMs: 3000, text: "i need you", speaker: "b" }], { durationMs: 5000 })
  for (const channel of ["system", "mic"] as const) {
    const f = await liveFixture({ script }); open.push(f)
    const noteId = await f.recordingNote()
    const client = f.connect(noteId)
    await client.hello([{ channel, sampleRate: RATE, encoding: "pcm_s16le", baseMs: 0, resumeFromSample: 0 }])
    await client.audio(script.pcm, 0, script.pcm.byteLength / 2, { speed: 10, channel })
    await client.until(() => client.finals().length === 3, "three finals")
    client.mark("end")
    await client.until(() => client.closed, "close")
    const labels = (await f.note(noteId)).transcriptSegments.map(segment => segment.speakerLabel)
    // On system audio "b" is a different id, so it is a second speaker; the unattributed first line is not a third.
    expect(labels, channel).toEqual(channel === "mic" ? ["Me", "Me", "Me"] : ["Speaker 1", "Speaker 1", "Speaker 2"])
    expect(client.of("interim").filter(message => message.speakerLabel).every(message => channel === "system" || message.speakerLabel === "Me"), channel).toBe(true)
  }
})

test("end to end: two people on one microphone; the one who speaks most ends up as Me, the other as a room speaker", async () => {
  const words = (name: string, count: number) => Array.from({ length: count }, (_, index) => `${name}${index}`).join(" ")
  const lines = [
    { atMs: 200, text: words("ada", 8), speaker: "ada" },
    { atMs: 3_500, text: words("ben", 15), speaker: "ben" }, { atMs: 9_000, text: words("ben", 15), speaker: "ben" },
    { atMs: 14_500, text: words("ben", 15), speaker: "ben" }, { atMs: 20_000, text: words("ben", 15), speaker: "ben" },
    { atMs: 25_500, text: words("ada", 6), speaker: "ada" },
  ]
  const script = encodeFakeSpeech(lines, { durationMs: 29_000 })
  const f = await liveFixture({ script, fake: { sessionSpeakers: true } }); open.push(f)
  const noteId = await f.recordingNote()
  const client = f.connect(noteId)
  await client.hello([{ channel: "mic", sampleRate: RATE, encoding: "pcm_s16le", baseMs: 0, resumeFromSample: 0 }])
  await client.audio(script.pcm, 0, script.pcm.byteLength / 2, { speed: 20, channel: "mic" })
  await client.until(() => client.finals().length >= 6, "six finals")
  client.mark("end")
  await client.until(() => client.closed, "close")
  const stored = (await f.note(noteId)).transcriptSegments
  const labelOf = (name: string) => [...new Set(stored.filter(segment => segment.text.startsWith(name)).map(segment => segment.speakerLabel))]
  expect(labelOf("ben")).toEqual(["Me"])
  expect(labelOf("ada")).toEqual(["Room speaker 1"])
  // The client was told about every stored segment that changed.
  const relabelled = new Set(client.of("speakers").flatMap(message => (message.relabels ?? []).map(item => item.segmentId)))
  for (const segment of client.finals().map(message => message.segment)) {
    const now = stored.find(item => item.id === segment.id)!
    if (now.speakerLabel !== segment.speakerLabel || now.speakerId !== segment.speakerId) expect(relabelled.has(segment.id)).toBe(true)
  }
})

test("system audio: after a provider drop the new session's voices keep the note's numbers (primer), instead of numbering on", async () => {
  const words = (name: string, count: number) => Array.from({ length: count }, (_, index) => `${name}${index}`).join(" ")
  const lines = [
    { atMs: 200, text: words("ana", 10), speaker: "ana" }, { atMs: 3_800, text: words("bo", 10), speaker: "bo" }, { atMs: 7_400, text: words("cy", 10), speaker: "cy" },
    { atMs: 11_000, text: words("bo", 8), speaker: "bo" }, { atMs: 14_000, text: words("ana", 8), speaker: "ana" },
    // After the drop: the same three people, in another order.
    { atMs: 18_000, text: words("cy", 8), speaker: "cy" }, { atMs: 21_000, text: words("ana", 8), speaker: "ana" }, { atMs: 24_000, text: words("bo", 8), speaker: "bo" },
  ]
  const script = encodeFakeSpeech(lines, { durationMs: 27_500 }), dropAt = Math.round(17_200 * RATE / 1000)
  const f = await liveFixture({ script, fake: { sessionSpeakers: true } }); open.push(f)
  const noteId = await f.recordingNote()
  const stream = [{ channel: "system" as const, sampleRate: RATE, encoding: "pcm_s16le" as const, baseMs: 0, resumeFromSample: 0 }]
  const first = f.connect(noteId)
  await first.hello(stream)
  await first.audio(script.pcm, 0, dropAt, { speed: 20 })
  await first.until(() => first.finals().length >= 5, "five finals")
  f.fake.control.fail()
  await first.until(() => first.closed, "1012")
  const second = f.connect(noteId)
  const ready = await second.hello(stream.map(item => ({ ...item, resumeFromSample: dropAt })))
  if (ready === "closed") throw new Error("refused")
  await second.audio(script.pcm, ready.streams[0]!.acceptFromSample, script.pcm.byteLength / 2, { speed: 20 })
  second.mark("end")
  await second.until(() => second.closed, "close")
  const stored = (await f.note(noteId)).transcriptSegments
  const labels = (name: string) => [...new Set(stored.filter(segment => segment.text.startsWith(name)).map(segment => segment.speakerLabel))]
  expect(labels("ana")).toEqual(["Speaker 1"])
  expect(labels("bo")).toEqual(["Speaker 2"])
  expect(labels("cy")).toEqual(["Speaker 3"])
  expect(f.fake.control.opened).toBe(2)
})
