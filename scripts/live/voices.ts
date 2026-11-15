/**
 * Speaker labels across Stop and Resume, measured on the real recogniser (apps/api/src/live/voices.ts
 * states the rule). Each case records a first capture, stops, waits, resumes the same note with a second
 * capture, and reports which label every voice had before and after.
 *
 *   bun --env-file=.env scripts/live/voices.ts <dir>/long [--only <case>] [--work <dir>]
 *
 * <dir>/long is the fixture of scripts/live/make-qa-fixtures.ts (two system voices, one microphone
 * voice). Further voices are rendered with `say` into the work directory (macOS only). The stack runs in
 * this process (scripts/live/stack.ts) with the real streaming and batch recognisers; about $0.03 a case.
 */
import { mkdir, readFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import type { LiveConfig } from "../../apps/api/src/live/session"
import type { QaManifest } from "./make-qa-fixtures"
import { load, Recording, Server, type Channel, type Fixture } from "./qa-lib"
import { startLiveStack } from "./stack"

const RATE = 16000, GAP = Math.round(0.45 * RATE)
const [directory, ...flags] = process.argv.slice(2)
if (!directory) { console.error("usage: bun --env-file=.env scripts/live/voices.ts <dir>/long [--only <case>] [--work <dir>]"); process.exit(2) }
const flag = (name: string) => flags.includes(name) ? flags[flags.indexOf(name) + 1] : undefined
const only = flag("--only"), work = flag("--work") ?? join(tmpdir(), "cereal-voices")

function wavPcm(bytes: Uint8Array) {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength)
  for (let offset = 12; offset + 8 <= bytes.byteLength;) {
    const size = view.getUint32(offset + 4, true)
    if (String.fromCharCode(...bytes.subarray(offset, offset + 4)) === "data") return bytes.subarray(offset + 8, Math.min(bytes.byteLength, offset + 8 + size))
    offset += 8 + size + (size & 1)
  }
  throw new Error("WAV has no data chunk")
}
const extraLines = [
  "I think we should move the review to next week, because the numbers are not ready yet.",
  "Can somebody send me the latest draft of the contract before lunch tomorrow?",
  "The customer asked for a discount, and I told them we would come back with an answer.",
  "We have three candidates for the role, and the second interview is on Thursday morning.",
  "Let us keep the launch date, but cut the scope of the first release a little.",
  "I will write up the notes and share them with everyone after this call.",
]
async function rendered(voice: string, index: number) {
  const aiff = join(work, `${voice}-${index}.aiff`), wav = join(work, `${voice}-${index}.wav`)
  if (!await Bun.file(wav).exists()) for (const command of [["say", "-v", voice, "-o", aiff, extraLines[index]!], ["afconvert", "-f", "WAVE", "-d", `LEI16@${RATE}`, "-c", "1", aiff, wav]]) {
    const child = Bun.spawn(command, { stdout: "ignore", stderr: "pipe" })
    if (await child.exited !== 0) throw new Error(`${command[0]} failed: ${await new Response(child.stderr).text()}`)
  }
  return new Uint8Array(wavPcm(new Uint8Array(await readFile(wav))))
}
await mkdir(work, { recursive: true })
const manifest: QaManifest = JSON.parse(await readFile(join(directory, "manifest.json"), "utf8"))
const source = Object.fromEntries(await Promise.all((["mic", "system"] as const).map(async channel => [channel, wavPcm(new Uint8Array(await readFile(join(directory, manifest.channels[channel]!.file))))]))) as Record<Channel, Uint8Array>
// Every voice's lines, in order. A case takes the next unused line of a voice each time it speaks.
const pools = new Map<string, Uint8Array[]>()
for (const line of manifest.lines.slice(0, 60)) pools.set(line.voice, [...(pools.get(line.voice) ?? []), source[line.channel].slice(line.firstSample * 2, (line.lastSample + 1) * 2)])
for (const voice of ["Moira", "Rishi"]) pools.set(voice, await Promise.all(extraLines.map((_, index) => rendered(voice, index))))

type Turn = [channel: Channel, voice: string]
interface Span { channel: Channel; voice: string; fromMs: number; toMs: number }
function compose(turns: Turn[], cursor: Map<string, number>): { fixture: Fixture; spans: Span[] } {
  const pieces: Array<{ channel: Channel; pcm: Uint8Array; at: number }> = [], spans: Span[] = []
  let samples = RATE / 2
  for (const [channel, voice] of turns) {
    const pool = pools.get(voice)!, index = cursor.get(voice) ?? 0, pcm = pool[index % pool.length]!
    cursor.set(voice, index + 1)
    pieces.push({ channel, pcm, at: samples })
    spans.push({ channel, voice, fromMs: samples * 1000 / RATE, toMs: (samples + pcm.byteLength / 2) * 1000 / RATE })
    samples += pcm.byteLength / 2 + GAP
  }
  samples += RATE
  const pcm = { mic: new Uint8Array(samples * 2), system: new Uint8Array(samples * 2) }
  for (const piece of pieces) pcm[piece.channel].set(piece.pcm, piece.at * 2)
  return { fixture: { rate: RATE, channels: ["mic", "system"], pcm, totalSamples: samples, lines: [] }, spans }
}

interface Case { name: string; rule: string; config: Partial<LiveConfig>; upload: boolean; waitSeconds: number; before: Turn[]; after: Turn[]; note?: string }
const S: Turn = ["system", "Samantha"], D: Turn = ["system", "Daniel"], K: Turn = ["mic", "Karen"]
const before: Turn[] = [D, S, K, D, S, K, D, S, K]
const cases: Case[] = [
  { name: "hold", rule: "1 (held provider session)", config: {}, upload: true, waitSeconds: 20, before, after: [S, D, K, S, D, K], note: "20 s between Stop and Resume: longer than the provider's 15 s idle close" },
  { name: "channel-rule", rule: "2 (channel rule, nothing to compare with)", config: { holdMs: 0 }, upload: false, waitSeconds: 2, before, after: [S, D, K, S, D, K], note: "no durable parts uploaded, so no evidence" },
  { name: "same-two", rule: "3 (evidence)", config: { holdMs: 0 }, upload: true, waitSeconds: 2, before, after: [S, D, K, S, D, K] },
  { name: "third-joins", rule: "3 (evidence)", config: { holdMs: 0 }, upload: true, waitSeconds: 2, before, after: [["system", "Moira"], S, D, K, ["system", "Moira"], S, D] },
  { name: "third-joins-man", rule: "3 (evidence)", config: { holdMs: 0 }, upload: true, waitSeconds: 2, before, after: [["system", "Rishi"], S, D, K, ["system", "Rishi"], S, D] },
  { name: "mic-changed-man", rule: "3 (evidence) against 2", config: { holdMs: 0 }, upload: true, waitSeconds: 2, before, after: [["mic", "Rishi"], S, D, ["mic", "Rishi"], S, ["mic", "Rishi"]] },
  { name: "mic-changed-woman", rule: "3 (evidence) against 2", config: { holdMs: 0 }, upload: true, waitSeconds: 2, before, after: [["mic", "Moira"], S, D, ["mic", "Moira"], S, ["mic", "Moira"]] },
  { name: "mic-changed-similar", rule: "3 (evidence) against 2", config: { holdMs: 0 }, upload: true, waitSeconds: 2, before, after: [["mic", "Samantha"], D, ["mic", "Samantha"], D, ["mic", "Samantha"]], note: "the pair the batch recogniser merged in the file measurements" },
]

let spend = 0
for (const item of cases) {
  if (only && item.name !== only) continue
  console.log(`\n== ${item.name}: rule ${item.rule}${item.note ? ` (${item.note})` : ""}; load ${load()}`)
  const stack = await startLiveStack({ provider: "elevenlabs", config: item.config }), server = new Server(stack.base, stack.token, null), cursor = new Map<string, number>()
  try {
    const a = compose(item.before, cursor), b = compose(item.after, cursor), title = `Voices ${item.name} (safe to delete)`
    const first = new Recording(server, a.fixture, { title, captureId: "voices-a", partSeconds: 10, upload: item.upload, quiet: true })
    await first.create()
    if (await first.connect() !== "ready") throw new Error(`first capture refused: ${first.refusal}`)
    await first.run()
    const stop = await first.finish({ settleMs: 1500 })
    const held = stack.hub.holding, sessionsBefore = stack.upstream.sessions
    await Bun.sleep(item.waitSeconds * 1000)
    const baseMs = Math.round(a.fixture.totalSamples * 1000 / RATE) + (item.waitSeconds + 3) * 1000
    const second = new Recording(server, b.fixture, { title, captureId: "voices-b", noteId: first.noteId, baseMs, firstSequence: 100, partSeconds: 10, upload: item.upload, quiet: true })
    await second.create()
    if (await second.connect() !== "ready") throw new Error(`second capture refused: ${second.refusal}`)
    await second.run()
    const stop2 = await second.finish({ settleMs: 1500 })
    const note = await second.stored() as unknown as { transcriptSegments: Array<{ channel: Channel; speakerLabel: string | null; startMs: number; endMs: number | null }> }
    // Seconds of each voice under each label, before and after the Resume.
    const table = new Map<string, Map<string, number>>()
    for (const segment of note.transcriptSegments) {
      const after = segment.startMs >= baseMs, spans = after ? b.spans : a.spans, middle = (segment.startMs + (segment.endMs ?? segment.startMs)) / 2 - (after ? baseMs : 0)
      const span = spans.find(candidate => candidate.channel === segment.channel && middle >= candidate.fromMs - 300 && middle <= candidate.toMs + 300)
      const key = `${segment.channel} ${span?.voice ?? "?"} ${after ? "after" : "before"}`, row = table.get(key) ?? new Map<string, number>()
      row.set(segment.speakerLabel ?? "none", (row.get(segment.speakerLabel ?? "none") ?? 0) + ((segment.endMs ?? segment.startMs) - segment.startMs) / 1000); table.set(key, row)
    }
    for (const [key, row] of [...table.entries()].sort()) console.log(`   ${key.padEnd(28)} ${[...row.entries()].sort((x, y) => y[1] - x[1]).map(([label, seconds]) => `${label} ${seconds.toFixed(1)} s`).join(", ")}`)
    const relabels = [...first.of("speakers"), ...second.of("speakers")].flatMap(entry => entry.message.merges.map(merge => `${merge.from} -> ${merge.to}`))
    const delays = second.of("final").map(entry => entry.at)
    console.log(`   held after Stop: ${held} note; provider sessions opened: ${sessionsBefore} before, ${stack.upstream.sessions - sessionsBefore} after; relabels: ${relabels.length ? relabels.join(", ") : "none"}`)
    console.log(`   voice comparisons: ${stack.voiceChecks.calls} (${stack.voiceChecks.seconds.toFixed(0)} s of audio); other batch: ${stack.batch.calls - stack.voiceChecks.calls} calls; stream ${stack.upstream.seconds.toFixed(0)} s; Stop ${stop.stopMs} ms and ${stop2.stopMs} ms; first final after Resume at ${delays[0] === undefined ? "?" : (delays[0] / 1000).toFixed(1)} s`)
    spend += stack.upstream.seconds / 3600 * 0.22 + stack.batch.seconds / 3600 * 0.2
    await first.remove()
  } finally { await stack.stop() }
}
console.log(`\nprovider spend at list price: about $${spend.toFixed(3)}`)
process.exit(0)
