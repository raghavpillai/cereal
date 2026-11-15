/**
 * Renders a scripted synthetic meeting with the macOS `say` voices: one 16 kHz PCM16 mono WAV per
 * channel plus a manifest that lists, for every line, its channel, voice, sample range and probe token.
 * No real audio is involved. macOS only (`say`, `afconvert`).
 *
 *   bun scripts/live/make-fixture.ts <output directory> [--lines N] [--no-mic]
 */
import { mkdir, readFile, rm, writeFile } from "node:fs/promises"
import { join, resolve } from "node:path"

export type FixtureLine = {
  channel: "mic" | "system"; voice: string; speaker: string; text: string
  /** An invented name in the line: whether a recogniser gets it right is reported, never required. */
  token: string
  /** A case-insensitive pattern for ordinary words at the end of the line, to time when the whole line has arrived. */
  probe: string
  firstSample: number; lastSample: number
}
export type FixtureManifest = { sampleRate: number; channels: Partial<Record<"mic" | "system", { file: string; samples: number }>>; lines: FixtureLine[] }

const SAMPLE_RATE = 16000
/** Remote speakers share the system channel (two voices, so diarisation has something to separate);
 * the local speaker is alone on the microphone. Every line carries one invented token to probe for. */
const script: Array<{ channel: "mic" | "system"; voice: string; speaker: string; text: string; token: string; probe: string }> = [
  ["system", "Daniel", "Arjun", "Thanks for joining. The first item is the Kestrel migration, which we moved to the fourteenth of March.", "Kestrel", "of March"],
  ["system", "Samantha", "Priya", "Before that, the Halvorsen contract renewal is worth four hundred and twenty thousand dollars a year.", "Halvorsen", "a year"],
  ["mic", "Karen", "Me", "I can take the Halvorsen call, but I need the Ostrava pricing sheet from finance first.", "Ostrava", "finance first"],
  ["system", "Daniel", "Arjun", "Finance said the pricing sheet lands on Thursday. The owner is Marguerite Oyelaran.", "Oyelaran", "owner is"],
  ["system", "Samantha", "Priya", "On reliability, the Quillfeather service had three outages last week, all in the Dublin region.", "Quillfeather", "Dublin region"],
  ["mic", "Karen", "Me", "Were those related to the Brambling cache rollout, or was that a separate change?", "Brambling", "separate change"],
  ["system", "Daniel", "Arjun", "Separate. The cache rollout is paused. The outages came from the Tamsin certificate rotation.", "Tamsin", "certificate rotation"],
  ["system", "Samantha", "Priya", "We agreed to add an alert at ninety seconds of queue delay, and to call it the Wexcombe alarm.", "Wexcombe", "alarm"],
  ["mic", "Karen", "Me", "Fine. For hiring, the Lindqvist offer went out yesterday with a start date in May.", "Lindqvist", "in May"],
  ["system", "Daniel", "Arjun", "Good. We still have two open roles on the Petrichor team, one senior and one staff engineer.", "Petrichor", "staff engineer"],
  ["system", "Samantha", "Priya", "The budget for both is approved. The recruiter is asking for the Corvane interview panel by Monday.", "Corvane", "by Monday"],
  ["mic", "Karen", "Me", "I will send the panel today. Next, the Zennor dashboard still shows last quarter's numbers.", "Zennor", "quarter'?s numbers"],
  ["system", "Daniel", "Arjun", "That is a known bug. The fix is in review under ticket seven four one nine, the Yarrowby patch.", "Yarrowby", "patch\\."],
  ["system", "Samantha", "Priya", "Customers noticed. The Fennimore account opened a support case about it on Tuesday morning.", "Fennimore", "Tuesday morning"],
  ["mic", "Karen", "Me", "Then let us ship the patch before Friday and tell Fennimore directly. Who writes the Dunmore note?", "Dunmore", "who writes"],
  ["system", "Daniel", "Arjun", "I will write it. One more risk: the Skerryvore vendor raised prices by eleven percent.", "Skerryvore", "(?:11|eleven) ?(?:percent|%)"],
  ["system", "Samantha", "Priya", "We can absorb that this year. Next year we should evaluate the Altamont alternative.", "Altamont", "alternative"],
  ["mic", "Karen", "Me", "Agreed. Put the evaluation on the roadmap under the name Project Halcyon Ridge.", "Halcyon", "roadmap under"],
  ["system", "Daniel", "Arjun", "Done. For the offsite, the venue is the Thornaby boathouse and the date is the ninth of June.", "Thornaby", "(?:of June|June (?:9|ninth))"],
  ["system", "Samantha", "Priya", "Thirty two people confirmed. Catering needs the final count by the Ravensworth deadline next Wednesday.", "Ravensworth", "next Wednesday"],
  ["mic", "Karen", "Me", "I will confirm the count. Last topic, the Islington security review found two medium issues.", "Islington", "medium issues"],
  ["system", "Daniel", "Arjun", "Both are fixed in staging. The auditor, Mister Castellane, wants evidence by the end of the month.", "Castellane", "end of the month"],
  ["system", "Samantha", "Priya", "I will collect the evidence. That leaves the Marlowe invoice, which is sixty days overdue.", "Marlowe", "overdue"],
  ["mic", "Karen", "Me", "Escalate it to legal if it is not paid by the twentieth. Anything else before we close?", "twentieth", "before we close"],
  ["system", "Daniel", "Arjun", "Only that the Pemberley release is tagged version five point three and ships on Monday.", "Pemberley", "ships on Monday"],
  ["system", "Samantha", "Priya", "And the retrospective is moved to the Greywether room at two o'clock tomorrow.", "Greywether", "tomorrow"],
  ["mic", "Karen", "Me", "Great. Thanks both. I will send the summary with the Ashgrove action list tonight.", "Ashgrove", "tonight"],
].map(([channel, voice, speaker, text, token, probe]) => ({ channel: channel as "mic" | "system", voice: voice!, speaker: speaker!, text: text!, token: token!, probe: probe! }))

function wavPcm(bytes: Uint8Array) {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength)
  for (let offset = 12; offset + 8 <= bytes.byteLength;) {
    const size = view.getUint32(offset + 4, true)
    if (String.fromCharCode(...bytes.subarray(offset, offset + 4)) === "data") return bytes.subarray(offset + 8, Math.min(bytes.byteLength, offset + 8 + size))
    offset += 8 + size + (size & 1)
  }
  throw new Error("WAV has no data chunk")
}
export function wavFile(pcm: Uint8Array, sampleRate = SAMPLE_RATE) {
  const header = new DataView(new ArrayBuffer(44))
  const ascii = (offset: number, text: string) => { for (let i = 0; i < text.length; i++) header.setUint8(offset + i, text.charCodeAt(i)) }
  ascii(0, "RIFF"); header.setUint32(4, 36 + pcm.byteLength, true); ascii(8, "WAVE"); ascii(12, "fmt ")
  header.setUint32(16, 16, true); header.setUint16(20, 1, true); header.setUint16(22, 1, true); header.setUint32(24, sampleRate, true)
  header.setUint32(28, sampleRate * 2, true); header.setUint16(32, 2, true); header.setUint16(34, 16, true); ascii(36, "data"); header.setUint32(40, pcm.byteLength, true)
  const out = new Uint8Array(44 + pcm.byteLength); out.set(new Uint8Array(header.buffer)); out.set(pcm, 44); return out
}
async function run(command: string[]) {
  const child = Bun.spawn(command, { stdout: "ignore", stderr: "pipe" })
  if (await child.exited !== 0) throw new Error(`${command[0]} failed: ${await new Response(child.stderr).text()}`)
}

export async function makeFixture(directory: string, options: { lines?: number; mic?: boolean; gapMs?: number } = {}): Promise<FixtureManifest> {
  const out = resolve(directory), work = join(out, ".work")
  await mkdir(work, { recursive: true })
  const lines = script.filter(line => options.mic !== false || line.channel === "system").slice(0, options.lines ?? script.length)
  const gap = Math.round((options.gapMs ?? 450) * SAMPLE_RATE / 1000)
  // Both channels share one clock: a line occupies its own channel and is silence on the other.
  const parts: Record<"mic" | "system", Uint8Array[]> = { mic: [], system: [] }
  const manifest: FixtureManifest = { sampleRate: SAMPLE_RATE, channels: {}, lines: [] }
  let cursor = Math.round(SAMPLE_RATE * 0.5)
  for (const channel of ["mic", "system"] as const) parts[channel].push(new Uint8Array(cursor * 2))
  for (const [index, line] of lines.entries()) {
    const aiff = join(work, `${index}.aiff`), wav = join(work, `${index}.wav`)
    await run(["say", "-v", line.voice, "-o", aiff, line.text])
    await run(["afconvert", "-f", "WAVE", "-d", `LEI16@${SAMPLE_RATE}`, "-c", "1", aiff, wav])
    const pcm = new Uint8Array(wavPcm(new Uint8Array(await readFile(wav))))
    const samples = pcm.byteLength / 2
    for (const channel of ["mic", "system"] as const) parts[channel].push(channel === line.channel ? pcm : new Uint8Array(pcm.byteLength), new Uint8Array(gap * 2))
    manifest.lines.push({ ...line, firstSample: cursor, lastSample: cursor + samples - 1 })
    cursor += samples + gap
  }
  for (const channel of ["mic", "system"] as const) {
    if (!lines.some(line => line.channel === channel)) continue
    const pcm = new Uint8Array(cursor * 2); let offset = 0
    for (const part of parts[channel]) { pcm.set(part, offset); offset += part.byteLength }
    await writeFile(join(out, `${channel}.wav`), wavFile(pcm))
    manifest.channels[channel] = { file: `${channel}.wav`, samples: cursor }
  }
  await writeFile(join(out, "manifest.json"), JSON.stringify(manifest, null, 2) + "\n")
  await rm(work, { recursive: true, force: true })
  return manifest
}

if (import.meta.main) {
  const [directory, ...flags] = process.argv.slice(2)
  if (!directory) { console.error("usage: bun scripts/live/make-fixture.ts <output directory> [--lines N] [--no-mic]"); process.exit(2) }
  const count = flags.indexOf("--lines")
  const manifest = await makeFixture(directory, { lines: count >= 0 ? Number(flags[count + 1]) : undefined, mic: !flags.includes("--no-mic") })
  const seconds = Math.max(...Object.values(manifest.channels).map(channel => channel!.samples)) / manifest.sampleRate
  console.log(`${manifest.lines.length} lines, ${seconds.toFixed(1)} s, channels: ${Object.keys(manifest.channels).join(", ")}`)
}
