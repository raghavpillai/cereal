/**
 * Synthetic audio for the live transcription test campaign (scripts/live/qa.ts).
 * Same manifest format as scripts/live/make-fixture.ts, with every line numbered ("Item 47. ...") so a
 * repeated or missing line can be told apart in a long recording. macOS only (`say`, `afconvert`).
 *
 *   bun scripts/live/make-qa-fixtures.ts <output directory> [--minutes 16] [--only long|chat|spanish]
 *
 * Writes <dir>/long (two channels: two system voices, one microphone voice), <dir>/chat (five minutes,
 * every line about a different topic, for the chat question battery) and <dir>/spanish (one
 * microphone voice speaking Spanish). Silence, quiet and microphone-only variants are derived from
 * these at run time by scripts/live/qa.ts.
 */
import { mkdir, readFile, rm, writeFile } from "node:fs/promises"
import { join, resolve } from "node:path"
import { wavFile, type FixtureLine, type FixtureManifest } from "./make-fixture"

export type QaLine = FixtureLine & { item: number }
export type QaManifest = Omit<FixtureManifest, "lines"> & { lines: QaLine[] }
const RATE = 16000
const ones = ["zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven", "twelve", "thirteen", "fourteen", "fifteen", "sixteen", "seventeen", "eighteen", "nineteen"]
const tens = ["", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety"]
export const numberWords = (n: number): string => n < 20 ? ones[n]! : n < 100 ? tens[Math.floor(n / 10)]! + (n % 10 ? ` ${ones[n % 10]}` : "") : `${ones[Math.floor(n / 100)]} hundred${n % 100 ? ` and ${numberWords(n % 100)}` : ""}`
/** How a recogniser may write "Item forty seven": digits, words, with or without a hyphen. */
export const itemPattern = (n: number) => `\\bitems? (?:number )?(?:${n}|${numberWords(n).replace(/ and /g, " (?:and )?").replace(/ /g, "[ -]")})\\b`

const topics = ["billing export", "onboarding checklist", "search index", "mobile release", "support backlog", "pricing page", "vendor contract", "hiring plan", "security audit", "data migration", "partner webinar", "quarterly forecast", "design review", "incident report", "customer survey", "roadmap draft", "training session", "office move", "analytics dashboard", "renewal pipeline"]
const people = ["Marguerite", "Tobias", "Ingrid", "Farid", "Noemi", "Callum", "Priyanka", "Lorenzo", "Astrid", "Desmond"]
const days = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday"]
const endings = ["before the board meeting", "after the pilot ends", "once legal signs off", "by the end of the sprint", "ahead of the launch", "during the next review", "when the budget is approved", "after the customer call"]
const shapes = [
  (t: string, p: string, d: string, e: string, k: number) => `The ${t} moved to ${d}, and ${p} owns the follow up ${e}.`,
  (t: string, p: string, d: string, e: string, k: number) => `${p} said the ${t} needs ${k} more hours, so we revisit it on ${d} ${e}.`,
  (t: string, p: string, d: string, e: string, k: number) => `We agreed that ${p} will finish the ${t} ${e}, with a check in on ${d}.`,
  (t: string, p: string, d: string, e: string, k: number) => `The budget for the ${t} is ${k} thousand dollars, and ${p} will confirm it on ${d}.`,
  (t: string, p: string, d: string, e: string, k: number) => `There are ${k} open questions about the ${t}, and ${p} answers them ${e}.`,
]
/** The five-minute meeting the chat battery asks about: forty lines, each about its own topic, so every
 * question has one right answer. Two lines carry invented names a recogniser is likely to misspell. */
const chatTopics = [...topics, "payroll switch", "loyalty program", "warehouse lease", "press release", "error budget", "user interviews", "tax filing", "brand guidelines", "holiday rota", "fraud rules",
  "sales playbook", "storage upgrade", "accessibility fixes", "board deck", "referral scheme", "status page", "expense policy", "beta waitlist", "localisation work", "compliance training"]
export interface ChatFact { item: number; channel: "mic" | "system"; voice: string; shape: number | "invoice" | "report"; topic: string; person: string; day: string; ending: string; k: number; text: string }
export function chatFacts(): ChatFact[] {
  return Array.from({ length: 40 }, (_, index) => {
    const who = voices[index % voices.length]!, item = index + 1
    const topic = chatTopics[index]!, person = people[(index * 3) % people.length]!, day = days[(index * 2 + Math.floor(index / 5)) % days.length]!, ending = endings[(index * 5) % endings.length]!, k = 3 + (index * 11) % 37
    if (index === 5) return { item, channel: who.channel, voice: who.voice, shape: "invoice" as const, topic: "Skerryvore invoice", person: "Marguerite Oyelaran", day: "", ending: "", k: 11, text: `Item ${numberWords(item)}. Marguerite Oyelaran from finance approved the Skerryvore invoice for eleven thousand dollars.` }
    if (index === 13) return { item, channel: who.channel, voice: who.voice, shape: "report" as const, topic: "Wexcombe report", person: "Quillfeather Partners", day: "Thursday", ending: "", k: 0, text: `Item ${numberWords(item)}. I will send the Wexcombe report to Quillfeather Partners by Thursday.` }
    const shape = index % shapes.length
    return { item, channel: who.channel, voice: who.voice, shape, topic, person, day, ending, k, text: `Item ${numberWords(item)}. ${shapes[shape]!(topic, person, day, ending, k)}` }
  })
}
const voices = [{ channel: "system", voice: "Daniel", speaker: "Arjun" }, { channel: "system", voice: "Samantha", speaker: "Priya" }, { channel: "mic", voice: "Karen", speaker: "Me" }] as const
const escape = (text: string) => text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")

function wavPcm(bytes: Uint8Array) {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength)
  for (let offset = 12; offset + 8 <= bytes.byteLength;) {
    const size = view.getUint32(offset + 4, true)
    if (String.fromCharCode(...bytes.subarray(offset, offset + 4)) === "data") return bytes.subarray(offset + 8, Math.min(bytes.byteLength, offset + 8 + size))
    offset += 8 + size + (size & 1)
  }
  throw new Error("WAV has no data chunk")
}
async function speak(work: string, name: string, voice: string, text: string) {
  const aiff = join(work, `${name}.aiff`), wav = join(work, `${name}.wav`)
  for (const command of [["say", "-v", voice, "-o", aiff, text], ["afconvert", "-f", "WAVE", "-d", `LEI16@${RATE}`, "-c", "1", aiff, wav]]) {
    const child = Bun.spawn(command, { stdout: "ignore", stderr: "pipe" })
    if (await child.exited !== 0) throw new Error(`${command[0]} failed: ${await new Response(child.stderr).text()}`)
  }
  return new Uint8Array(wavPcm(new Uint8Array(await readFile(wav))))
}
async function render(directory: string, script: Array<{ channel: "mic" | "system"; voice: string; speaker: string; text: string; item: number; probe: string }>, gapMs = 450): Promise<QaManifest> {
  const work = join(directory, ".work")
  await mkdir(work, { recursive: true })
  const gap = Math.round(gapMs * RATE / 1000), used = [...new Set(script.map(line => line.channel))]
  const parts: Record<string, Uint8Array[]> = Object.fromEntries(used.map(channel => [channel, [new Uint8Array(RATE)]]))
  const manifest: QaManifest = { sampleRate: RATE, channels: {}, lines: [] }
  let cursor = RATE / 2
  for (const [index, line] of script.entries()) {
    const pcm = await speak(work, String(index), line.voice, line.text), samples = pcm.byteLength / 2
    for (const channel of used) parts[channel]!.push(channel === line.channel ? pcm : new Uint8Array(pcm.byteLength), new Uint8Array(gap * 2))
    manifest.lines.push({ ...line, token: `Item ${line.item}`, firstSample: cursor, lastSample: cursor + samples - 1 })
    cursor += samples + gap
  }
  for (const channel of used) {
    const pcm = new Uint8Array(cursor * 2); let offset = 0
    for (const part of parts[channel]!) { pcm.set(part, offset); offset += part.byteLength }
    await writeFile(join(directory, `${channel}.wav`), wavFile(pcm, RATE))
    manifest.channels[channel as "mic" | "system"] = { file: `${channel}.wav`, samples: cursor }
  }
  await writeFile(join(directory, "manifest.json"), JSON.stringify(manifest, null, 1) + "\n")
  await rm(work, { recursive: true, force: true })
  return manifest
}

if (import.meta.main) {
  const [directory, ...flags] = process.argv.slice(2)
  if (!directory) { console.error("usage: bun scripts/live/make-qa-fixtures.ts <output directory> [--minutes 16] [--only long|chat|spanish]"); process.exit(2) }
  const only = flags.includes("--only") ? flags[flags.indexOf("--only") + 1] : undefined, wanted = (name: string) => !only || only === name
  const minutes = Number(flags[flags.indexOf("--minutes") + 1] ?? 16) || 16
  // About 6.3 seconds per line including the pause.
  const count = Math.ceil(minutes * 60 / 6.3)
  const long = Array.from({ length: count }, (_, index) => {
    const item = index + 1, who = voices[index % voices.length]!
    const body = shapes[index % shapes.length]!(topics[(index * 7) % topics.length]!, people[(index * 3) % people.length]!, days[(index * 2 + Math.floor(index / 5)) % days.length]!, endings[(index * 5) % endings.length]!, 3 + (index * 11) % 37)
    // The timing probe is the last three words of the line: ordinary words a recogniser writes as spoken.
    return { ...who, item, text: `Item ${numberWords(item)}. ${body}`, probe: escape(body.replace(/\.$/, "").split(" ").slice(-3).join(" ")).replace(/ /g, "\\s+") }
  })
  if (wanted("long")) { const made = await render(resolve(directory, "long"), long); console.log(`long: ${made.lines.length} lines, ${(made.channels.system!.samples / RATE / 60).toFixed(1)} minutes`) }
  const chat = chatFacts().map(fact => ({ channel: fact.channel, voice: fact.voice, speaker: fact.channel === "mic" ? "Me" : fact.voice === "Daniel" ? "Arjun" : "Priya", item: fact.item, text: fact.text, probe: escape(fact.text.replace(/\.$/, "").split(" ").slice(-3).join(" ")).replace(/ /g, "\\s+") }))
  if (wanted("chat")) { const made = await render(resolve(directory, "chat"), chat); console.log(`chat: ${made.lines.length} lines, ${(made.channels.system!.samples / RATE / 60).toFixed(1)} minutes`) }
  const spanish = [
    "Buenos días a todos. Hoy revisamos el presupuesto del proyecto y las fechas de entrega.",
    "El equipo de ventas necesita el informe antes del viernes por la tarde.",
    "También tenemos que contratar a dos ingenieros para la oficina de Madrid.",
    "La próxima reunión será el martes a las diez de la mañana. Muchas gracias.",
  ].map((text, index) => ({ channel: "mic" as const, voice: "Mónica", speaker: "Me", item: index + 1, text, probe: escape(text.replace(/\.$/, "").split(" ").slice(-2).join(" ")) }))
  if (wanted("spanish")) { const es = await render(resolve(directory, "spanish"), spanish, 600); console.log(`spanish: ${es.lines.length} lines, ${(es.channels.mic!.samples / RATE).toFixed(1)} s`) }
}
