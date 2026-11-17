/** Microphone text only where the microphone had sound: a recogniser hears words in the low hiss of a quiet room ("Yeah."), which showed as
 * the note owner speaking while nobody did. Words with no sound at speech level near them are not stored or shown,
 * while speech and the call's audio are transcribed as before. */
import { afterEach, expect, setDefaultTimeout, test } from "bun:test"
import { encodeFakeSpeech } from "../src/live/providers/fake"
import { liveFixture, RATE, transcriptText, type LiveFixture } from "./live-support"

setDefaultTimeout(30_000)
const open: LiveFixture[] = []
afterEach(async () => { for (const fixture of open.splice(0)) await fixture.dispose() })

/** Scripted speech, then after half a second hiss (amplitude 200, under speech level) until the end. */
function speechThenHiss(text: string) {
  const script = encodeFakeSpeech([{ atMs: 0, text }], { durationMs: 4000 })
  const view = new DataView(script.pcm.buffer), count = script.pcm.byteLength >> 1
  const lastWord = script.lines.at(-1)!.lastSample
  for (let sample = lastWord + Math.round(RATE * 0.5); sample < count; sample++) view.setInt16(sample * 2, sample % 2 ? 200 : -200, true)
  return script
}

for (const channel of ["mic", "system"] as const) test(`${channel}: speech is kept; the hiss after it is ${channel === "mic" ? "heard as nothing" : "passed on as it is"}`, async () => {
  const script = speechThenHiss("alpha bravo charlie")
  const fixture = await liveFixture({ script, fake: { noiseWord: "Yeah.", interimDelayMs: 20, finalDelayMs: 40 } })
  open.push(fixture)
  const noteId = await fixture.recordingNote()
  const client = fixture.connect(noteId)
  await client.hello([{ channel, sampleRate: RATE, encoding: "pcm_s16le", baseMs: 0, resumeFromSample: 0 }])
  const end = script.pcm.byteLength / 2
  await client.audio(script.pcm, 0, end, { channel, speed: 8 })
  client.mark("end", { [channel]: end })
  await client.until(() => client.closed, "close after end")
  const text = transcriptText(await fixture.note(noteId), channel)
  expect(text.startsWith("alpha bravo charlie")).toBe(true)
  // Without the gate the fake recogniser, like a real one, hears the hiss as words: the system channel shows it does.
  const drafts = client.of("interim").map(message => (message as { text: string }).text)
  if (channel === "mic") {
    expect(text).toBe("alpha bravo charlie")
    // Not even as a draft: the bubble that showed "Yeah." while nobody spoke.
    expect(drafts.some(draft => draft.includes("Yeah"))).toBe(false)
  }
  else {
    expect(text).toContain("Yeah.")
    expect(drafts.some(draft => draft.includes("Yeah"))).toBe(true)
  }
})
