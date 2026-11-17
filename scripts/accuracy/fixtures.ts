/**
 * Invented meetings with known reference text, for the live transcription accuracy harness
 * (scripts/accuracy/run.ts). macOS only: the voices are `say` voices, rendered at 16 kHz.
 *
 *   bun scripts/accuracy/fixtures.ts [--out .local/ship/accuracy/fixtures] [--only smoke-30s,meeting-5m,...]
 *
 * Two channels, as the desktop app sends them: `mic` is the person recording ("Me", one voice) and
 * `system` is everyone else (several voices, one of them speaking Spanish and one Japanese). Each fixture
 * is a directory with `mic.wav`, `system.wav` (16 kHz PCM16 mono, equal length) and `reference.json`:
 * every utterance with its channel, speaker, language, text and the sample range its speech occupies
 * (leading and trailing silence of the rendering trimmed), word times spread over that range by length.
 *
 * What the meetings contain: turns between four English voices; short interjections on the other channel
 * while someone speaks (cross-channel overlap, as when you say "yeah" over the call); numbers, money,
 * percentages, dates, invented names and engineering jargon; quiet mumbles; pauses of 5 to 120 s; a
 * Spanish and a Japanese passage, and a short Spanish thank-you; background noise on the microphone
 * (pink) and music on system audio at a given signal-to-noise ratio. The audio is deterministic: the
 * same command makes the same files (renders are cached in `<out>/.renders`).
 */
import { mkdir, readFile, writeFile } from "node:fs/promises"
import { createHash } from "node:crypto"
import { join, resolve } from "node:path"

export const RATE = 16000
export type ChannelName = "mic" | "system"
export type Language = "en" | "es" | "ja"
export interface RefWord { text: string; startMs: number; endMs: number }
export interface RefUtterance {
  id: number; channel: ChannelName; speaker: string; voice: string; language: Language
  /** "speech", a short "interjection" over someone else, or a quiet "mumble" (fillers only). */
  kind: "speech" | "interjection" | "mumble"
  text: string; startMs: number; endMs: number; firstSample: number; lastSample: number; words: RefWord[]
}
export interface Reference {
  name: string; sampleRate: number; samples: number; durationMs: number
  channels: Record<ChannelName, string>
  utterances: RefUtterance[]
  /** Stretches where neither channel has speech, longer than 4 s. */
  silences: Array<{ fromMs: number; toMs: number }>
  noise: Array<{ channel: ChannelName; kind: "pink" | "music"; snrDb: number; fromMs: number; toMs: number }>
}

// ------------------------------------------------------------------ the cast

const CAST = {
  me: { channel: "mic" as const, voice: "Samantha", language: "en" as const },
  daniel: { channel: "system" as const, voice: "Daniel", language: "en" as const },
  karen: { channel: "system" as const, voice: "Karen", language: "en" as const },
  rishi: { channel: "system" as const, voice: "Rishi", language: "en" as const },
  paulina: { channel: "system" as const, voice: "Paulina", language: "es" as const },
  kyoko: { channel: "system" as const, voice: "Kyoko", language: "ja" as const },
}
type Who = keyof typeof CAST
const ENGLISH_REMOTE: Who[] = ["daniel", "karen", "rishi"]

// ------------------------------------------------------------------ content

/** A deterministic generator (mulberry32). */
function random(seed: number) {
  let state = seed >>> 0
  const next = () => { state = (state + 0x6d2b79f5) >>> 0; let t = state; t = Math.imul(t ^ (t >>> 15), t | 1); t ^= t + Math.imul(t ^ (t >>> 7), t | 61); return ((t ^ (t >>> 14)) >>> 0) / 4294967296 }
  return { next, pick: <T>(items: ReadonlyArray<T>) => items[Math.floor(next() * items.length)]!, int: (from: number, to: number) => from + Math.floor(next() * (to - from + 1)) }
}
const NAMES = ["Siobhan", "Okonkwo", "Bartholomew", "Nguyen", "Priyanka", "Leopold", "Ximena", "Thaddeus", "Ingrid", "Rafferty", "Marguerite", "Desmond"]
const SYSTEMS = ["the Kubernetes cluster", "the PostgreSQL replica", "the OAuth token service", "the gRPC gateway", "the Terraform modules", "the Kafka consumer group", "the CI pipeline", "the Redis cache", "the GraphQL schema", "the billing webhook"]
const PROJECTS = ["Kestrel", "Halvorsen", "Quillfeather", "Brambling", "Wexcombe", "Petrichor", "Skerryvore", "Altamont", "Thornaby", "Ravensworth"]
const MONTHS = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"]
const DAYS = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday"]
const ordinal = (n: number) => `${n}${n % 100 >= 11 && n % 100 <= 13 ? "th" : ["th", "st", "nd", "rd"][n % 10] ?? "th"}`

const TEMPLATES: Array<(r: ReturnType<typeof random>) => string> = [
  r => `The ${r.pick(PROJECTS)} migration moved to ${r.pick(MONTHS)} ${ordinal(r.int(1, 28))}, and ${r.pick(NAMES)} owns the follow up.`,
  r => `Latency on ${r.pick(SYSTEMS)} went from ${r.int(120, 480)} milliseconds to ${r.int(40, 110)} after the rollback.`,
  r => `The budget for ${r.pick(PROJECTS)} is ${r.int(2, 9)}.${r.int(1, 9)} million dollars, which is ${r.int(5, 30)} percent over plan.`,
  r => `${r.pick(NAMES)} said ${r.pick(SYSTEMS)} needs ${r.int(3, 40)} more hours, so we revisit it on ${r.pick(DAYS)}.`,
  r => `We had ${r.int(2, 9)} incidents last week, all of them in ${r.pick(SYSTEMS)}, and the error rate peaked at ${r.int(1, 9)}.${r.int(1, 9)} percent.`,
  r => `Can you send me the ${r.pick(PROJECTS)} report before ${r.pick(DAYS)}? I need the numbers for the board.`,
  r => `The contract with ${r.pick(PROJECTS)} renews on ${r.pick(MONTHS)} ${ordinal(r.int(1, 28))}, ${r.int(2026, 2029)}, at ${r.int(40, 900)} thousand dollars a year.`,
  r => `I think we should keep the launch date but cut the scope of ${r.pick(PROJECTS)} a little.`,
  r => `${r.pick(NAMES)} and ${r.pick(NAMES)} will pair on ${r.pick(SYSTEMS)} until the end of the sprint.`,
  r => `The service level agreement promises ${r.pick(["99.9", "99.95", "99.5"])} percent uptime, and we are at ${r.pick(["99.7", "99.92", "98.6"])} this quarter.`,
  r => `Ticket ${r.int(1000, 9999)} is blocked on ${r.pick(SYSTEMS)}. ${r.pick(NAMES)} is looking at it today.`,
  r => `We hired ${r.int(2, 12)} engineers this quarter, and ${r.int(1, 5)} more start in ${r.pick(MONTHS)}.`,
  r => `Let us move the ${r.pick(PROJECTS)} review to next week, because the numbers are not ready yet.`,
  r => `Version ${r.int(2, 9)}.${r.int(0, 9)} of the mobile app ships on ${r.pick(DAYS)} with the new onboarding flow.`,
  r => `The customer asked for a discount of ${r.int(5, 25)} percent, and I told them we would come back with an answer.`,
  r => `Our p99 latency is ${r.int(200, 900)} milliseconds, which is above the target of ${r.int(100, 190)}.`,
]
const INTERJECTIONS = ["Yeah.", "Right.", "Okay.", "Sure.", "Exactly.", "Mm-hmm.", "Uh-huh."]
const MUMBLES = ["Hmm.", "Uh.", "Um, hmm."]
const SPANISH = "Buenos días a todos. El informe trimestral estará listo el jueves, y el presupuesto del proyecto es de doce mil euros. Necesitamos la aprobación del equipo legal antes del viernes."
const SPANISH_SHORT = "Muchas gracias a todos."
const JAPANESE = "来週の会議では、新しいデザインについて話し合いましょう。予算は三百万円で、締め切りは金曜日です。"

// ------------------------------------------------------------------ a script

interface Line { who: Who; text: string; kind?: RefUtterance["kind"]; gainDb?: number; overlap?: boolean }
type Step = { line: Line } | { pause: number }

function meeting(seed: number, turns: number, options: { pauses?: number[]; foreign?: boolean; interjectEvery?: number; maxSentences?: number } = {}): Step[] {
  const r = random(seed), steps: Step[] = []
  const pauses = [...(options.pauses ?? [])]
  const pauseEvery = pauses.length ? Math.max(3, Math.floor(turns / (pauses.length + 1))) : Infinity
  let speaker: Who = "me"
  for (let turn = 0; turn < turns; turn++) {
    const sentences = r.int(1, options.maxSentences ?? 3), text = Array.from({ length: sentences }, () => r.pick(TEMPLATES)(r)).join(" ")
    steps.push({ line: { who: speaker, text } })
    // A short interjection from the other side, while this turn is under way.
    if (options.interjectEvery && turn % options.interjectEvery === 1) {
      const other: Who = CAST[speaker].channel === "mic" ? r.pick(ENGLISH_REMOTE) : "me"
      steps.push({ line: { who: other, text: r.pick(INTERJECTIONS), kind: "interjection", overlap: true } })
    }
    if (turn % 7 === 4) steps.push({ line: { who: "me", text: r.pick(MUMBLES), kind: "mumble", gainDb: -16 } })
    if (options.foreign && turn === Math.floor(turns / 3)) steps.push({ line: { who: "paulina", text: SPANISH } })
    if (options.foreign && turn === Math.floor(turns / 2)) steps.push({ line: { who: "kyoko", text: JAPANESE } })
    if (options.foreign && turn === Math.floor(turns * 3 / 4)) steps.push({ line: { who: "paulina", text: SPANISH_SHORT } })
    if ((turn + 1) % pauseEvery === 0 && pauses.length) steps.push({ pause: pauses.shift()! })
    speaker = speaker === "me" ? r.pick(ENGLISH_REMOTE) : r.next() < 0.6 ? "me" : r.pick(ENGLISH_REMOTE.filter(item => item !== speaker))
  }
  return steps
}

// ------------------------------------------------------------------ rendering

function wavPcm(bytes: Uint8Array): Int16Array {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength)
  for (let offset = 12; offset + 8 <= bytes.byteLength;) {
    const size = view.getUint32(offset + 4, true)
    if (String.fromCharCode(...bytes.subarray(offset, offset + 4)) === "data") {
      const data = bytes.slice(offset + 8, Math.min(bytes.byteLength, offset + 8 + size))
      return new Int16Array(data.buffer, 0, data.byteLength >> 1)
    }
    offset += 8 + size + (size & 1)
  }
  throw new Error("WAV has no data chunk")
}
export function wavBytes(pcm: Int16Array, rate = RATE): Uint8Array {
  const out = new Uint8Array(44 + pcm.byteLength), view = new DataView(out.buffer)
  const ascii = (at: number, text: string) => { for (let i = 0; i < text.length; i++) view.setUint8(at + i, text.charCodeAt(i)) }
  ascii(0, "RIFF"); view.setUint32(4, 36 + pcm.byteLength, true); ascii(8, "WAVE"); ascii(12, "fmt "); view.setUint32(16, 16, true); view.setUint16(20, 1, true); view.setUint16(22, 1, true)
  view.setUint32(24, rate, true); view.setUint32(28, rate * 2, true); view.setUint16(32, 2, true); view.setUint16(34, 16, true); ascii(36, "data"); view.setUint32(40, pcm.byteLength, true)
  out.set(new Uint8Array(pcm.buffer, pcm.byteOffset, pcm.byteLength), 44)
  return out
}
export async function readWav(path: string): Promise<Int16Array> { return wavPcm(new Uint8Array(await readFile(path))) }

/** Speech of one line, with the rendering's leading and trailing silence trimmed. Cached by voice and text. */
async function render(cache: string, voice: string, text: string): Promise<Int16Array> {
  const key = createHash("sha1").update(`${voice}\n${text}`).digest("hex").slice(0, 16), wav = join(cache, `${key}.wav`)
  if (!await Bun.file(wav).exists()) {
    const child = Bun.spawn(["say", "-v", voice, "-o", wav, "--file-format=WAVE", `--data-format=LEI16@${RATE}`, text], { stdout: "ignore", stderr: "pipe" })
    if (await child.exited !== 0) throw new Error(`say failed for ${voice}: ${await new Response(child.stderr).text()}`)
  }
  const pcm = await readWav(wav)
  let first = 0, last = pcm.length - 1
  while (first < pcm.length && Math.abs(pcm[first]!) < 300) first++
  while (last > first && Math.abs(pcm[last]!) < 300) last--
  return pcm.slice(Math.max(0, first - 80), Math.min(pcm.length, last + 80))
}

/** Words of a reference line: whitespace tokens, or for Japanese, runs of up to three characters
 * (punctuation stays with the run before it). */
export function referenceTokens(text: string, language: Language): string[] {
  if (language !== "ja") return text.split(/\s+/).filter(Boolean)
  const out: string[] = []
  for (const char of text) {
    if (/[、。！？]/.test(char) && out.length) out[out.length - 1] += char
    else if (out.length && [...out.at(-1)!].filter(c => !/[、。！？]/.test(c)).length < 3 && !/[、。！？]$/.test(out.at(-1)!)) out[out.length - 1] += char
    else out.push(char)
  }
  return out
}

/** Pink noise (Paul Kellet's filter), deterministic. */
function pink(samples: number, seed: number) {
  const r = random(seed), out = new Float32Array(samples)
  let b0 = 0, b1 = 0, b2 = 0, b3 = 0, b4 = 0, b5 = 0, b6 = 0
  for (let i = 0; i < samples; i++) {
    const white = r.next() * 2 - 1
    b0 = 0.99886 * b0 + white * 0.0555179; b1 = 0.99332 * b1 + white * 0.0750759; b2 = 0.969 * b2 + white * 0.153852
    b3 = 0.8665 * b3 + white * 0.3104856; b4 = 0.55 * b4 + white * 0.5329522; b5 = -0.7616 * b5 - white * 0.016898
    out[i] = (b0 + b1 + b2 + b3 + b4 + b5 + b6 + white * 0.5362) * 0.11; b6 = white * 0.115926
  }
  return out
}
/** Background music: a slow chord progression with a soft beat. */
function music(samples: number) {
  const out = new Float32Array(samples), chords = [[220, 277.2, 329.6], [196, 246.9, 293.7], [174.6, 220, 261.6], [196, 246.9, 311.1]]
  for (let i = 0; i < samples; i++) {
    const t = i / RATE, chord = chords[Math.floor(t / 2) % chords.length]!, beat = t % 0.6
    let value = 0
    for (const f of chord) value += Math.sin(2 * Math.PI * f * t) + 0.3 * Math.sin(4 * Math.PI * f * t)
    value = value / 4 * (0.6 + 0.4 * Math.exp(-beat * 6))
    out[i] = value
  }
  return out
}
const rms = (pcm: Int16Array, from = 0, to = pcm.length) => { let sum = 0, count = 0; for (let i = from; i < to; i++) { const v = pcm[i]!; if (Math.abs(v) > 300) { sum += v * v; count++ } } return count ? Math.sqrt(sum / count) : 0 }

interface Spec { name: string; seed: number; turns: number; maxSentences?: number; pauses: number[]; foreign: boolean; interjectEvery?: number; noise?: Array<{ snrDb: number; fromFraction: number; toFraction: number }>; leadMs?: number; silenceOnly?: number }

export async function build(spec: Spec, out: string, cache: string): Promise<Reference> {
  const steps: Step[] = spec.silenceOnly ? [{ pause: spec.silenceOnly }] : meeting(spec.seed, spec.turns, { pauses: spec.pauses, foreign: spec.foreign, ...(spec.interjectEvery ? { interjectEvery: spec.interjectEvery } : {}), ...(spec.maxSentences ? { maxSentences: spec.maxSentences } : {}) })
  const placed: Array<{ line: Line; pcm: Int16Array; at: number }> = []
  // The timeline: one cursor, lines one after another with a short gap; an overlap starts inside the line before it.
  let cursor = Math.round((spec.leadMs ?? 800) * RATE / 1000)
  const r = random(spec.seed + 1)
  for (const step of steps) {
    if ("pause" in step) { cursor += step.pause * RATE; continue }
    const who = CAST[step.line.who], pcm = await render(cache, who.voice, step.line.text)
    if (step.line.overlap && placed.length) {
      const previous = placed.at(-1)!, length = previous.pcm.length
      // Inside the previous line, on the other channel, finishing before it does.
      const at = previous.at + Math.round(length * (0.3 + 0.3 * r.next()))
      placed.push({ line: step.line, pcm, at: Math.min(at, previous.at + Math.max(0, length - pcm.length)) })
      continue
    }
    placed.push({ line: step.line, pcm, at: cursor })
    cursor += pcm.length + Math.round(RATE * (0.35 + 0.5 * r.next()))
  }
  const samples = cursor + RATE * 2
  const mix = { mic: new Float64Array(samples), system: new Float64Array(samples) }
  const utterances: RefUtterance[] = []
  for (const [index, item] of placed.entries()) {
    const who = CAST[item.line.who], gain = Math.pow(10, (item.line.gainDb ?? 0) / 20), channel = mix[who.channel]
    for (let i = 0; i < item.pcm.length; i++) channel[item.at + i]! += item.pcm[i]! * gain
    const tokens = referenceTokens(item.line.text, who.language), weights = tokens.map(token => [...token].length + 2), total = weights.reduce((a, b) => a + b, 0)
    const startMs = item.at * 1000 / RATE, endMs = (item.at + item.pcm.length) * 1000 / RATE
    let acc = 0
    const words = tokens.map((text, i) => { const from = startMs + (endMs - startMs) * acc / total; acc += weights[i]!; return { text, startMs: Math.round(from), endMs: Math.round(startMs + (endMs - startMs) * acc / total) } })
    utterances.push({ id: index, channel: who.channel, speaker: item.line.who, voice: who.voice, language: who.language, kind: item.line.kind ?? "speech", text: item.line.text, startMs: Math.round(startMs), endMs: Math.round(endMs), firstSample: item.at, lastSample: item.at + item.pcm.length - 1, words })
  }
  utterances.sort((a, b) => a.startMs - b.startMs)
  const pcm = { mic: Int16Array.from(mix.mic, v => Math.max(-32768, Math.min(32767, Math.round(v)))), system: Int16Array.from(mix.system, v => Math.max(-32768, Math.min(32767, Math.round(v)))) }
  // Noise at a signal-to-noise ratio relative to the channel's speech level (or a quiet floor with no speech).
  const noise: Reference["noise"] = []
  const levels = { mic: rms(pcm.mic) || 3000, system: rms(pcm.system) || 3000 }
  for (const stretch of spec.noise ?? []) {
    const from = Math.floor(samples * stretch.fromFraction), to = Math.floor(samples * stretch.toFraction)
    for (const channel of ["mic", "system"] as const) {
      const kind = channel === "mic" ? "pink" as const : "music" as const
      const source = kind === "pink" ? pink(to - from, spec.seed + from) : music(to - from)
      let power = 0; for (const v of source) power += v * v
      const scale = levels[channel] / Math.pow(10, stretch.snrDb / 20) / Math.sqrt(power / source.length)
      for (let i = 0; i < source.length; i++) pcm[channel][from + i] = Math.max(-32768, Math.min(32767, pcm[channel][from + i]! + Math.round(source[i]! * scale)))
      noise.push({ channel, kind, snrDb: stretch.snrDb, fromMs: Math.round(from * 1000 / RATE), toMs: Math.round(to * 1000 / RATE) })
    }
  }
  const silences: Reference["silences"] = []
  const spoken = [...utterances].sort((a, b) => a.startMs - b.startMs)
  let lastEnd = 0
  for (const utterance of spoken) { if (utterance.startMs - lastEnd > 4000) silences.push({ fromMs: lastEnd, toMs: utterance.startMs }); lastEnd = Math.max(lastEnd, utterance.endMs) }
  if (samples * 1000 / RATE - lastEnd > 4000) silences.push({ fromMs: lastEnd, toMs: Math.round(samples * 1000 / RATE) })
  const directory = join(out, spec.name)
  await mkdir(directory, { recursive: true })
  await writeFile(join(directory, "mic.wav"), wavBytes(pcm.mic))
  await writeFile(join(directory, "system.wav"), wavBytes(pcm.system))
  const reference: Reference = { name: spec.name, sampleRate: RATE, samples, durationMs: Math.round(samples * 1000 / RATE), channels: { mic: "mic.wav", system: "system.wav" }, utterances, silences, noise }
  await writeFile(join(directory, "reference.json"), JSON.stringify(reference, null, 1))
  return reference
}

export const SPECS: Spec[] = [
  { name: "smoke-30s", seed: 30, turns: 5, maxSentences: 1, pauses: [], foreign: false, interjectEvery: 2 },
  { name: "meeting-5m", seed: 5, turns: 24, maxSentences: 2, pauses: [6, 15, 30], foreign: true, interjectEvery: 3 },
  { name: "meeting-5m-snr20", seed: 5, turns: 24, maxSentences: 2, pauses: [6, 15, 30], foreign: true, interjectEvery: 3, noise: [{ snrDb: 20, fromFraction: 0, toFraction: 1 }] },
  { name: "meeting-5m-snr10", seed: 5, turns: 24, maxSentences: 2, pauses: [6, 15, 30], foreign: true, interjectEvery: 3, noise: [{ snrDb: 10, fromFraction: 0, toFraction: 1 }] },
  { name: "meeting-5m-snr5", seed: 5, turns: 24, maxSentences: 2, pauses: [6, 15, 30], foreign: true, interjectEvery: 3, noise: [{ snrDb: 5, fromFraction: 0, toFraction: 1 }] },
  // An hour: pauses from 5 s to two minutes, noise that changes every ten minutes.
  { name: "meeting-60m", seed: 60, turns: 380, maxSentences: 2, pauses: [5, 8, 12, 20, 30, 45, 60, 90, 120, 10, 25], foreign: true, interjectEvery: 4,
    noise: [{ snrDb: 20, fromFraction: 1 / 6, toFraction: 2 / 6 }, { snrDb: 10, fromFraction: 3 / 6, toFraction: 4 / 6 }, { snrDb: 5, fromFraction: 4 / 6, toFraction: 5 / 6 }, { snrDb: 15, fromFraction: 5 / 6, toFraction: 1 }] },
  // Nothing but a noise floor on both channels for three minutes.
  { name: "silence-3m", seed: 3, turns: 0, pauses: [], foreign: false, silenceOnly: 180, noise: [{ snrDb: 40, fromFraction: 0, toFraction: 1 }] },
]

if (import.meta.main) {
  const args = process.argv.slice(2), value = (name: string) => { const index = args.indexOf(`--${name}`); return index >= 0 ? args[index + 1] : undefined }
  const out = resolve(value("out") ?? ".local/ship/accuracy/fixtures"), only = value("only")?.split(",")
  const cache = join(out, ".renders")
  await mkdir(cache, { recursive: true })
  for (const spec of SPECS) {
    if (only && !only.includes(spec.name)) continue
    const started = performance.now(), reference = await build(spec, out, cache)
    const words = reference.utterances.reduce((sum, utterance) => sum + utterance.words.length, 0)
    console.log(`${spec.name}: ${(reference.durationMs / 1000).toFixed(0)} s, ${reference.utterances.length} utterances, ${words} words, ${reference.silences.length} silences, rendered in ${((performance.now() - started) / 1000).toFixed(1)} s`)
  }
}
