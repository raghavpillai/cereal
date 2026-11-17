/**
 * Regressions found by the live transcription accuracy harness (scripts/accuracy/, results in
 * .local/ship/accuracy/). Each scenario streams a scripted meeting through the real route, hub and session
 * with the fake recogniser, which hears exactly the samples it is sent, and scores the stored transcript with
 * the harness's own metrics: anything lost, repeated or clipped is the pipeline's doing, so the expected word
 * error rate is zero.
 */
import { afterEach, describe, expect, setDefaultTimeout, test } from "bun:test"
import { Effect } from "effect"
import { encodeFakeSpeech, type FakeLine, type FakeScript } from "../src/live/providers/fake"
import type { Reference } from "../../../scripts/accuracy/fixtures"
import { align, evaluate, normalise, type HypSegment } from "../../../scripts/accuracy/metrics"
import { liveFixture, RATE, type LiveClient, type LiveFixture } from "./live-support"

setDefaultTimeout(60_000)
const open: LiveFixture[] = []
afterEach(async () => { for (const fixture of open.splice(0)) await fixture.dispose() })
async function fixture(options: Parameters<typeof liveFixture>[0] = {}) { const made = await liveFixture(options); open.push(made); return made }

const WORD_MS = 240, GAP_MS = 60
/** A scripted meeting and its reference, the same shape the harness scores. */
function meeting(lines: ReadonlyArray<FakeLine & { language?: "en" | "es" | "ja" }>, durationMs: number): { script: FakeScript; reference: Reference } {
  const script = encodeFakeSpeech(lines, { durationMs, wordMs: WORD_MS, gapMs: GAP_MS })
  let index = 0
  const languages = lines.flatMap(line => line.text.split(/\s+/).filter(Boolean).map(() => line.language))
  script.vocabulary.forEach((word, i) => { const language = languages[i]; if (language) (word as { language?: string }).language = language })
  const utterances = script.lines.map((line, id) => {
    const tokens = line.text.split(/\s+/).filter(Boolean), startMs = line.firstSample * 1000 / RATE
    const words = tokens.map((text, k) => ({ text, startMs: startMs + k * (WORD_MS + GAP_MS), endMs: startMs + k * (WORD_MS + GAP_MS) + WORD_MS }))
    index += tokens.length
    return { id, channel: "system" as const, speaker: line.speaker ?? "a", voice: "fake", language: (lines[id]!.language ?? "en") as "en", kind: "speech" as const, text: line.text, startMs, endMs: words.at(-1)!.endMs, firstSample: line.firstSample, lastSample: line.lastSample, words }
  })
  void index
  return { script, reference: { name: "test", sampleRate: RATE, samples: script.pcm.byteLength / 2, durationMs, channels: { mic: "", system: "" }, utterances, silences: [], noise: [] } }
}
const stream = (resumeFromSample = 0) => [{ channel: "system" as const, sampleRate: RATE, encoding: "pcm_s16le" as const, baseMs: 0, resumeFromSample }]
const at = (ms: number) => Math.round(ms * RATE / 1000)
async function finish(f: LiveFixture, client: LiveClient, noteId: string) {
  client.mark("end")
  await client.until(() => client.closed, "close after end")
  await Effect.runPromise(f.repository.forUser("30000000-0000-4000-8000-000000000001").recording(noteId, "stop"))
  return (await f.note(noteId)).transcriptSegments as ReadonlyArray<HypSegment>
}

describe("the harness metrics", () => {
  test("normalisation, alignment and a perfect transcript", () => {
    expect(normalise("The budget is $4.6 million, 29% over plan.").join(" ")).toBe("the budget is four point six million dollars twenty nine percent over plan")
    expect(normalise("March 14th, Mm-hmm, OK").join(" ")).toBe("march fourteenth okay")
    expect(normalise("会議では").join(" ")).toBe("会 議 で は")
    expect(align(["a", "b", "c"], ["a", "x", "c", "d"]).map(op => op.kind)).toEqual(["match", "sub", "match", "ins"])
    const { reference } = meeting([{ atMs: 200, text: "the kestrel migration moved to march", speaker: "a" }, { atMs: 3000, text: "halvorsen renewal is four hundred thousand", speaker: "b" }], 6000)
    const perfect = reference.utterances.map(item => ({ channel: item.channel, speakerId: `system:n:${item.speaker}`, speakerLabel: null, text: item.text, startMs: item.startMs, endMs: item.endMs }))
    const report = evaluate({ reference, segments: perfect })
    expect([report.wer, report.attribution.system, report.drift.medianMs]).toEqual([0, 1, 0])
    // A sentence stored twice, and one lost.
    const doubled = evaluate({ reference, segments: [perfect[0]!, { ...perfect[0]!, startMs: perfect[0]!.endMs }] })
    expect(doubled.duplicatedRuns.length).toBe(1)
    expect(doubled.lostRuns.map(run => run.words)).toEqual([6])
  })
})

describe("accuracy scenarios with the fake recogniser", () => {
  // Words of several lines with short pauses, so a cut can land mid-word and mid-sentence.
  const lines = (speakers = ["a", "b"]): FakeLine[] => [
    { atMs: 300, text: "the kestrel migration moved to march fourteenth and siobhan owns the follow up", speaker: speakers[0] },
    { atMs: 5600, text: "latency on the grpc gateway went from three hundred milliseconds to forty", speaker: speakers[1] },
    { atMs: 10_200, text: "we hired four engineers this quarter and two more start in october", speaker: speakers[0] },
  ]

  test("a network stall mid-word that outlasts a keep-alive leaves the word whole: the keep-alive adds no audio", async () => {
    // The provider adapter's keep-alive carries no samples (live-elevenlabs-adapter.test.ts); the fake does the
    // same here. With 100 ms of silence per keep-alive the stalled word came back as "this- this-".
    const { script, reference } = meeting(lines(), 14_000)
    const f = await fixture({ script, fake: { keepAliveSeconds: 0 }, config: { keepAliveMs: 200, detachMs: 10_000 } })
    const noteId = await f.recordingNote()
    const client = f.connect(noteId)
    await client.hello(stream())
    // Mid-word in the second line, then nothing for well over a keep-alive interval, on the same socket.
    const cut = at(5600 + 3 * (WORD_MS + GAP_MS) + WORD_MS / 2)
    await client.audio(script.pcm, 0, cut, { speed: 10 })
    await Bun.sleep(1200)
    expect(f.fake.control.audioSeconds).toBeCloseTo(cut / RATE, 1)
    await client.audio(script.pcm, cut, script.pcm.byteLength / 2, { speed: 10 })
    const segments = await finish(f, client, noteId)
    const report = evaluate({ reference, segments })
    expect(report.clippedWords).toBe(0)
    expect(report.wer).toBe(0)
  })

  test("audio that stops arriving mid-sentence is made final after stallCommitMs, and the rest follows when it resumes", async () => {
    const { script, reference } = meeting(lines(), 14_000)
    const f = await fixture({ script, fake: { keepAliveSeconds: 0 }, config: { keepAliveMs: 200, stallCommitMs: 600 } })
    const noteId = await f.recordingNote()
    const client = f.connect(noteId)
    await client.hello(stream())
    const cut = at(5600 + 6 * (WORD_MS + GAP_MS) - GAP_MS / 2)  // between two words of the second line
    await client.audio(script.pcm, 0, cut, { speed: 10 })
    // Stored without any more audio: the provider is not left holding an uncommitted utterance.
    await client.until(() => client.finals().some(final => final.segment.text.includes("grpc gateway went")), "the stalled sentence stored", 5000)
    await client.audio(script.pcm, cut, script.pcm.byteLength / 2, { speed: 10 })
    const report = evaluate({ reference, segments: await finish(f, client, noteId) })
    expect(report.wer).toBe(0)
  })

  test("in an English meeting a short sentence in Spanish is kept, a mumble written in Japanese is dropped, and a Japanese passage is kept", async () => {
    // Real provider runs: "Muchas gracias a todos." was dropped as stray because it was detected as Spanish, while
    // a "hmm" written "うん。" passed because it was labelled English. The filter now goes by the text's script.
    const { script, reference } = meeting([
      { atMs: 300, text: "the kestrel migration moved to march fourteenth and siobhan owns the follow up", speaker: "a", language: "en" },
      { atMs: 5600, text: "muchas gracias a todos", speaker: "b", language: "es" },
      { atMs: 7600, text: "うん。", speaker: "a", language: "en" },
      { atMs: 9000, text: "来週の 会議で は、新し いデザ インに つい て話 し合い ましょ う。予 算は 三百 万円 です。", speaker: "c", language: "ja" },
      { atMs: 15_000, text: "we hired four engineers this quarter and two more start in october", speaker: "a", language: "en" },
    ], 19_000)
    const f = await fixture({ script, fake: { keepAliveSeconds: 0 } })
    const noteId = await f.recordingNote()
    const client = f.connect(noteId)
    await client.hello(stream())
    await client.audio(script.pcm, 0, script.pcm.byteLength / 2, { speed: 10 })
    const segments = await finish(f, client, noteId)
    const text = segments.map(segment => segment.text).join(" ")
    expect(text).toContain("muchas gracias a todos")
    expect(text).not.toContain("うん")
    expect(text).toContain("来週の")
    // The mumble is the only difference from the reference.
    const report = evaluate({ reference: { ...reference, utterances: reference.utterances.filter(item => item.text !== "うん。") }, segments })
    expect(report.wer).toBe(0)
  })

  test("speakers keep their numbers across provider sessions lost mid-sentence: the primer plays a sample of each known voice", async () => {
    // The newest 20 s alone held no stored voice when the drop came mid-sentence, and every voice was renumbered;
    // and a voice heard only before an earlier drop was renumbered at the next one.
    const { script, reference } = meeting([
      { atMs: 300, text: "the kestrel migration moved to march fourteenth and siobhan owns the follow up", speaker: "a" },
      { atMs: 5600, text: "latency on the grpc gateway went from three hundred milliseconds to forty", speaker: "b" },
      // A long pause: the newest 20 s before each cut holds nothing of b.
      { atMs: 30_200, text: "we hired four engineers this quarter and two more start in october", speaker: "a" },
      { atMs: 34_600, text: "version four point four of the mobile app ships on friday with the new flow", speaker: "a" },
      { atMs: 39_400, text: "can you send me the quillfeather report before monday i need the numbers", speaker: "a" },
      { atMs: 44_200, text: "the budget for brambling is four point six million dollars over plan", speaker: "b" },
    ], 49_000)
    const f = await fixture({ script, fake: { keepAliveSeconds: 0, sessionSpeakers: true } })
    const noteId = await f.recordingNote()
    let client = f.connect(noteId)
    await client.hello(stream())
    let from = 0
    for (const cutAt of [30_200, 39_400]) {
      const cut = at(cutAt + 5 * (WORD_MS + GAP_MS) + WORD_MS / 2)
      await client.audio(script.pcm, from, cut, { speed: 10 })
      await Bun.sleep(1000)  // what was said before the cut is stored
      f.fake.control.fail()
      await client.until(() => client.closed, "1012")
      const acked = client.lastAck()
      client = f.connect(noteId)
      const ready = await client.hello(stream(acked))
      if (ready === "closed") throw new Error("refused")
      from = ready.streams[0]!.acceptFromSample
    }
    await client.audio(script.pcm, from, script.pcm.byteLength / 2, { speed: 10 })
    const report = evaluate({ reference, segments: await finish(f, client, noteId) })
    expect(report.wer).toBe(0)
    expect(report.attribution.idsPerSpeaker).toEqual({ a: 1, b: 1 })
    expect(report.attribution.system).toBe(1)
  })

  for (const kind of ["network drop", "provider drop", "server restart"] as const) {
    test(`${kind} mid-sentence: the client's replay leaves no word lost, repeated or clipped`, async () => {
      const { script, reference } = meeting(lines(), 14_000)
      const f = await fixture({ script, fake: { keepAliveSeconds: 0, sessionSpeakers: true } })
      const noteId = await f.recordingNote()
      let client = f.connect(noteId)
      await client.hello(stream())
      const cut = at(5600 + 4 * (WORD_MS + GAP_MS) + WORD_MS / 3), total = script.pcm.byteLength / 2
      await client.audio(script.pcm, 0, cut, { speed: 10 })
      let server = f
      if (kind === "network drop") { client.drop(); await Bun.sleep(300) }
      else if (kind === "provider drop") { f.fake.control.fail(); await client.until(() => client.closed, "1012") ; expect(client.closed!.code).toBe(1012) }
      else { await f.hub.dispose({ abandon: true }); server = await fixture({ script, repository: f.repository, fake: { keepAliveSeconds: 0, sessionSpeakers: true } }) }
      // The client resumes where the server says, from its own copy of the audio.
      const acked = client.lastAck()
      client = server.connect(noteId)
      const ready = await client.hello(stream(acked))
      if (ready === "closed") throw new Error("refused")
      await client.audio(script.pcm, ready.streams[0]!.acceptFromSample, total, { speed: 10 })
      const report = evaluate({ reference, segments: await finish(server, client, noteId) })
      expect({ wer: report.wer, lost: report.lostRuns, duplicated: report.duplicatedRuns }).toEqual({ wer: 0, lost: [], duplicated: [] })
    })
  }
})
