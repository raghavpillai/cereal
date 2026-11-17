/**
 * Accuracy metrics for a live transcript against a fixture's reference (scripts/accuracy/fixtures.ts).
 * Pure functions: the harness (run.ts) and the API tests (apps/api/test/live-accuracy.test.ts) both use them.
 *
 *  - WER per channel and overall, after normalisation: case, punctuation, accents, numbers and money
 *    written as words, fillers ("uh", "mm-hmm") removed on both sides. Japanese counts characters.
 *  - Attribution: microphone words labelled "Me"; system-audio words under a one-to-one best mapping of
 *    reference speakers to diarised speaker IDs, and how many IDs each person was split into.
 *  - Lost and duplicated runs: four or more consecutive reference words missing, and stretches of text
 *    the transcript holds twice; each reported with its time, and counted near the run's events.
 *  - Drift: segment start against the start of the reference utterance it begins with, over time.
 *  - Stray script: text in a script (CJK, Hangul, Cyrillic, ...) where nothing in that language was said.
 *  - Words in reference silences (hallucinations), punctuation and casing, interim flicker.
 */
import type { ChannelName, Reference, RefUtterance } from "./fixtures"

export interface HypSegment { channel: ChannelName; speakerId: string; speakerLabel: string | null; text: string; startMs: number; endMs: number | null }
export interface InterimMessage { channel: ChannelName; utteranceId: string; rev: number; text: string; startMs: number }
export interface FinalMessage { utteranceId: string; segment: HypSegment }

// ------------------------------------------------------------------ normalisation

const ONES = ["zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven", "twelve", "thirteen", "fourteen", "fifteen", "sixteen", "seventeen", "eighteen", "nineteen"]
const TENS = ["", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety"]
export function numberWords(n: number): string {
  if (n < 20) return ONES[n]!
  if (n < 100) return TENS[Math.floor(n / 10)]! + (n % 10 ? ` ${ONES[n % 10]}` : "")
  if (n < 1000) return `${ONES[Math.floor(n / 100)]} hundred${n % 100 ? ` ${numberWords(n % 100)}` : ""}`
  for (const [size, name] of [[1e9, "billion"], [1e6, "million"], [1e3, "thousand"]] as const) if (n >= size) return `${numberWords(Math.floor(n / size))} ${name}${n % size ? ` ${numberWords(n % size)}` : ""}`
  return String(n)
}
const ORDINAL_WORDS: Record<string, string> = { one: "first", two: "second", three: "third", five: "fifth", eight: "eighth", nine: "ninth", twelve: "twelfth" }
const ordinalWords = (n: number) => { const words = numberWords(n).split(" "), last = words.pop()!; return [...words, ORDINAL_WORDS[last] ?? (last.endsWith("y") ? `${last.slice(0, -1)}ieth` : `${last}th`)].join(" ") }
/** Spoken forms of numbers: a recogniser may write either. Years are read the way they are said. */
function spellNumber(digits: string): string {
  const n = Number(digits)
  if (!Number.isFinite(n)) return digits
  if (digits.length > 1 && digits.startsWith("0")) return [...digits].map(d => ONES[Number(d)]).join(" ")
  return numberWords(n)
}
const FILLERS = new Set(["uh", "um", "umm", "uhm", "hmm", "hm", "mm", "mmm", "mhm", "mmhmm", "uhhuh", "ah", "er", "erm", "eh", "oh"])
const SPELLING: Record<string, string> = { ok: "okay", alright: "all right", percent: "percent", per: "per" }
export const CJK = /[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}\p{Script=Hangul}]/u
const FOREIGN_SCRIPT = /[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}\p{Script=Hangul}\p{Script=Cyrillic}\p{Script=Arabic}\p{Script=Devanagari}\p{Script=Thai}\p{Script=Greek}\p{Script=Hebrew}]/u

/** Normalised tokens of one written word (or phrase). */
export function normalise(text: string): string[] {
  let s = text.normalize("NFKC").toLowerCase()
  s = s.replace(/[‘’ʼ]/g, "'").replace(/[–—]/g, " ")
  s = s.replace(/(\d),(?=\d{3}\b)/g, "$1").replace(/\b(?:uh[- ]huh|mm[- ]hmm?|m-hm)\b/g, " ")
  s = s.replace(/\$\s?(\d+(?:\.\d+)?)\s*(million|thousand|billion)?/g, (_, n: string, scale: string | undefined) => `${n}${scale ? ` ${scale}` : ""} dollars`)
  s = s.replace(/€\s?(\d+(?:\.\d+)?)\s*(million|thousand|billion)?/g, (_, n: string, scale: string | undefined) => `${n}${scale ? ` ${scale}` : ""} euros`)
  s = s.replace(/(\d)\s*%/g, "$1 percent")
  s = s.replace(/\b(\d+)(st|nd|rd|th)\b/g, (_, n: string) => ordinalWords(Number(n)))
  s = s.replace(/([a-z])(\d)/g, "$1 $2").replace(/(\d)([a-z])/g, "$1 $2")
  s = s.replace(/(\d+)\.(\d+)/g, (_, a: string, b: string) => `${spellNumber(a)} point ${[...b].map(d => ONES[Number(d)]).join(" ")}`)
  s = s.replace(/\d+/g, digits => ` ${spellNumber(digits)} `)
  // Latin accents only: Japanese voicing marks are part of the letter.
  s = s.normalize("NFD").replace(/[\u0300-\u036f]/g, "").normalize("NFC")
  s = s.replace(/[-_/]/g, " ").replace(/'/g, "")
  const out: string[] = []
  for (const raw of s.split(/\s+/)) {
    // CJK characters are tokens of their own; everything else loses its punctuation.
    const pieces = raw.split(/([\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}\p{Script=Hangul}])/u)
    for (const piece of pieces) {
      const word = piece.replace(/[^\p{L}\p{N}]/gu, "")
      if (!word || FILLERS.has(word)) continue
      for (const part of (SPELLING[word] ?? word).split(" ")) out.push(part)
    }
  }
  return out
}

// ------------------------------------------------------------------ tokens with provenance

interface RefToken { token: string; utterance: RefUtterance; wordIndex: number; atMs: number; raw: string }
interface HypToken { token: string; segment: number; atMs: number; raw: string; rawIndex: number; channel: ChannelName; speakerId: string; label: string | null }

export function referenceTokens(reference: Reference, channel: ChannelName, baseMs = 0): RefToken[] {
  const out: RefToken[] = []
  for (const utterance of reference.utterances.filter(item => item.channel === channel && item.kind !== "mumble").sort((a, b) => a.startMs - b.startMs)) {
    utterance.words.forEach((word, wordIndex) => { for (const token of normalise(word.text)) out.push({ token, utterance, wordIndex, atMs: baseMs + (word.startMs + word.endMs) / 2, raw: word.text }) })
  }
  return out
}
export function hypothesisTokens(segments: ReadonlyArray<HypSegment>, channel: ChannelName): HypToken[] {
  const out: HypToken[] = []
  const ordered = segments.map((segment, index) => ({ segment, index })).filter(item => item.segment.channel === channel).sort((a, b) => a.segment.startMs - b.segment.startMs || a.index - b.index)
  for (const { segment, index } of ordered) {
    const words = segment.text.split(/\s+/).filter(Boolean), weights = words.map(word => word.length + 1), total = weights.reduce((a, b) => a + b, 0) || 1
    const span = Math.max(0, (segment.endMs ?? segment.startMs) - segment.startMs)
    let acc = 0
    words.forEach((word, rawIndex) => {
      const atMs = segment.startMs + span * (acc + weights[rawIndex]! / 2) / total
      acc += weights[rawIndex]!
      for (const token of normalise(word)) out.push({ token, segment: index, atMs, raw: word, rawIndex, channel, speakerId: segment.speakerId, label: segment.speakerLabel })
    })
  }
  return out
}

// ------------------------------------------------------------------ alignment

export type Op = { kind: "match" | "sub"; ref: number; hyp: number } | { kind: "del"; ref: number } | { kind: "ins"; hyp: number }
/** Levenshtein alignment with a backtrace; substitutions cost 1 like insertions and deletions. */
export function align(ref: ReadonlyArray<string>, hyp: ReadonlyArray<string>): Op[] {
  const n = ref.length, m = hyp.length, width = m + 1
  const cost = new Uint32Array((n + 1) * width), back = new Uint8Array((n + 1) * width)
  for (let j = 1; j <= m; j++) { cost[j] = j; back[j] = 2 }
  for (let i = 1; i <= n; i++) {
    cost[i * width] = i; back[i * width] = 1
    const r = ref[i - 1]
    for (let j = 1; j <= m; j++) {
      const diagonal = cost[(i - 1) * width + j - 1]! + (r === hyp[j - 1] ? 0 : 1), up = cost[(i - 1) * width + j]! + 1, left = cost[i * width + j - 1]! + 1
      let best = diagonal, from = 0
      if (up < best) { best = up; from = 1 }
      if (left < best) { best = left; from = 2 }
      cost[i * width + j] = best; back[i * width + j] = from
    }
  }
  const ops: Op[] = []
  for (let i = n, j = m; i > 0 || j > 0;) {
    const from = back[i * width + j]
    if (i > 0 && j > 0 && from === 0) { ops.push({ kind: ref[i - 1] === hyp[j - 1] ? "match" : "sub", ref: i - 1, hyp: j - 1 }); i--; j-- }
    else if (i > 0 && (j === 0 || from === 1)) { ops.push({ kind: "del", ref: i - 1 }); i-- }
    else { ops.push({ kind: "ins", hyp: j - 1 }); j-- }
  }
  return ops.reverse()
}

// ------------------------------------------------------------------ the report

export interface RunEvent { atMs: number; kind: string }
export interface AccuracyReport {
  words: number; wer: number; errors: { sub: number; del: number; ins: number }
  perChannel: Record<ChannelName, { words: number; wer: number; sub: number; del: number; ins: number }>
  perLanguage: Record<string, { words: number; errorRate: number }>
  interjections: { said: number; recalled: number }
  attribution: { micAsMe: number; micWords: number; system: number; systemWords: number; idsPerSpeaker: Record<string, number>; table: Record<string, Record<string, number>> }
  lostRuns: Array<{ channel: ChannelName; atMs: number; words: number; text: string; nearEvent: string | null }>
  duplicatedRuns: Array<{ channel: ChannelName; atMs: number; words: number; text: string; nearEvent: string | null }>
  boundary: { lostWordsNearEvents: number; duplicatedWordsNearEvents: number; lostWordsElsewhere: number; duplicatedWordsElsewhere: number }
  clippedWords: number
  drift: { measured: number; medianMs: number | null; micMedianMs: number | null; systemMedianMs: number | null; firstTenthMs: number | null; lastTenthMs: number | null; slopeMsPerHour: number | null; maxAbsMs: number | null }
  strayScript: { segments: number; characters: number; examples: string[] }
  hallucinatedInSilence: { words: number; examples: string[] }
  punctuation: { sentenceEndRecall: number | null; sentenceEndPrecision: number | null; segmentsCapitalised: number | null; namesCapitalised: number | null }
  flicker: { utterances: number; interims: number; rewrites: number; perFinal: number | null; finalDiffersFromLastInterim: number }
}

const median = (values: number[]) => { if (!values.length) return null; const sorted = [...values].sort((a, b) => a - b); return sorted[Math.floor(sorted.length / 2)]! }

export function evaluate(input: { reference: Reference; segments: ReadonlyArray<HypSegment>; interims?: ReadonlyArray<InterimMessage>; finals?: ReadonlyArray<FinalMessage>; events?: ReadonlyArray<RunEvent>;
  /** Note-timeline position of the fixture's first sample. */ baseMs?: number;
  /** Only the reference up to here was captured (a stop before the end). */ untilMs?: number; eventWindowMs?: number }): AccuracyReport {
  const { reference, segments } = input, baseMs = input.baseMs ?? 0, window = input.eventWindowMs ?? 4000
  const events = input.events ?? []
  const near = (atMs: number) => events.find(event => Math.abs(event.atMs - atMs) <= window)?.kind ?? null
  // A stop part-way through: the words said before it (a word cut by it counts when half of it was said).
  const until = input.untilMs
  const trimmed = until === undefined ? reference.utterances : reference.utterances.filter(utterance => utterance.startMs < until).map(utterance => ({ ...utterance, words: utterance.words.filter(word => (word.startMs + word.endMs) / 2 < until) }))
  const captured = (utterance: RefUtterance) => utterance.words.length > 0
  const totals = { sub: 0, del: 0, ins: 0, words: 0 }
  const perChannel = {} as AccuracyReport["perChannel"], perLanguage: Record<string, { words: number; errors: number }> = {}
  const lostRuns: AccuracyReport["lostRuns"] = [], duplicatedRuns: AccuracyReport["duplicatedRuns"] = []
  const table: Record<string, Record<string, number>> = {}
  let micAsMe = 0, micWords = 0, interjectionsSaid = 0, interjectionsRecalled = 0
  let sentenceEnds = 0, sentenceEndsKept = 0, hypSentenceEnds = 0, hypSentenceEndsRight = 0, names = 0, namesKept = 0
  const offsets: Array<{ atMs: number; offsetMs: number; channel: ChannelName }> = []
  for (const channel of ["mic", "system"] as const) {
    const ref = referenceTokens({ ...reference, utterances: trimmed.filter(captured) }, channel, baseMs), hyp = hypothesisTokens(segments, channel)
    const ops = align(ref.map(item => item.token), hyp.map(item => item.token))
    const counts = { sub: 0, del: 0, ins: 0 }
    for (const op of ops) if (op.kind !== "match") counts[op.kind]++
    perChannel[channel] = { words: ref.length, wer: ref.length ? (counts.sub + counts.del + counts.ins) / ref.length : counts.ins ? 1 : 0, ...counts }
    totals.sub += counts.sub; totals.del += counts.del; totals.ins += counts.ins; totals.words += ref.length
    // Per language: errors charged to the reference token they replace or miss; an insertion to the one before it.
    let lastRef = -1
    for (const op of ops) {
      if (op.kind !== "ins") lastRef = op.ref
      const item = op.kind === "ins" ? ref[Math.max(0, lastRef)] : ref[op.ref]
      if (!item) continue
      const language = item.utterance.language, entry = perLanguage[language] ??= { words: 0, errors: 0 }
      if (op.kind !== "ins") entry.words++
      if (op.kind !== "match") entry.errors++
    }
    // Interjections said and recalled.
    const byUtterance = new Map<RefUtterance, { total: number; matched: number }>()
    for (const op of ops) if (op.kind !== "ins") { const item = ref[op.ref]!, entry = byUtterance.get(item.utterance) ?? { total: 0, matched: 0 }; entry.total++; if (op.kind === "match") entry.matched++; byUtterance.set(item.utterance, entry) }
    for (const [utterance, entry] of byUtterance) if (utterance.kind === "interjection") { interjectionsSaid++; if (entry.matched === entry.total) interjectionsRecalled++ }
    // Attribution, punctuation and drift on matched words.
    for (const op of ops) {
      if (op.kind !== "match") continue
      const r = ref[op.ref]!, h = hyp[op.hyp]!
      if (channel === "mic") { micWords++; if (h.label === "Me") micAsMe++ }
      else { const row = table[r.utterance.speaker] ??= {}; row[h.speakerId] = (row[h.speakerId] ?? 0) + 1 }
      if (r.utterance.language === "en") {
        if (/[.?!]$/.test(r.raw) && r.utterance.kind === "speech") { sentenceEnds++; if (/[.?!]$/.test(h.raw)) sentenceEndsKept++ }
        if (/^[A-Z][a-z]/.test(r.raw) && r.wordIndex > 0 && !/[.?!]$/.test(r.utterance.words[r.wordIndex - 1]!.text) && !/^(I|I'm|I'll)$/.test(r.raw)) { names++; if (/^[A-Z]/.test(h.raw)) namesKept++ }
      }
    }
    for (const op of ops) if (op.kind === "match" || op.kind === "sub") { const h = hyp[op.hyp]!, r = ref[op.ref]!; if (/[.?!]$/.test(h.raw) && h.rawIndex >= 0) { hypSentenceEnds++; if (/[.?!]$/.test(r.raw)) hypSentenceEndsRight++ } }
    // Drift: a segment whose first token is the first word of a reference utterance.
    const firstOfSegment = new Set<number>()
    hyp.forEach((item, index) => { if (index === 0 || hyp[index - 1]!.segment !== item.segment) firstOfSegment.add(index) })
    for (const op of ops) {
      if (op.kind !== "match" || !firstOfSegment.has(op.hyp)) continue
      const r = ref[op.ref]!
      if (r.wordIndex !== 0 || normalise(r.raw)[0] !== r.token) continue
      const segment = segments[hyp[op.hyp]!.segment]!
      offsets.push({ atMs: baseMs + r.utterance.startMs, offsetMs: segment.startMs - (baseMs + r.utterance.startMs), channel })
    }
    // Duplicated text: a stretch of four words the transcript holds more often within 30 s than the
    // reference does; every copy past the reference's count is a duplicate.
    const GRAM = 4, NEAR = 30_000, gram = (tokens: ReadonlyArray<{ token: string }>, at: number) => tokens.slice(at, at + GRAM).map(item => item.token).join(" ")
    const refGrams = new Map<string, number[]>()
    for (let i = 0; i + GRAM <= ref.length; i++) { const key = gram(ref, i); refGrams.set(key, [...(refGrams.get(key) ?? []), ref[i]!.atMs]) }
    const hypGrams = new Map<string, number[]>(), duplicate = new Uint8Array(hyp.length)
    // The reference word each transcript word stands against: a phrase the recogniser always mishears the same way
    // ("p99" as "page 99") is allowed as often as the reference says the phrase it stands for.
    const alignedRef = new Int32Array(hyp.length).fill(-1)
    for (const op of ops) if (op.kind === "match" || op.kind === "sub") alignedRef[op.hyp] = op.ref
    const nearRef = (key: string, atMs: number) => (refGrams.get(key) ?? []).filter(refAt => Math.abs(refAt - atMs) <= NEAR).length
    for (let i = 0; i + GRAM <= hyp.length; i++) {
      const key = gram(hyp, i), atMs = hyp[i]!.atMs, earlier = hypGrams.get(key) ?? []
      const refs = Array.from({ length: GRAM }, (_, k) => alignedRef[i + k]!), consecutive = refs.every((r, k) => r >= 0 && (k === 0 || r === refs[k - 1]! + 1))
      const allowed = Math.max(nearRef(key, atMs), consecutive ? nearRef(refs.map(r => ref[r]!.token).join(" "), atMs) : 0)
      if (earlier.filter(before => atMs - before <= NEAR).length >= Math.max(1, allowed)) for (let k = i; k < i + GRAM; k++) duplicate[k] = 1
      hypGrams.set(key, [...earlier, atMs])
    }
    for (let i = 0; i < hyp.length;) {
      if (!duplicate[i]) { i++; continue }
      let end = i
      while (end < hyp.length && duplicate[end]) end++
      duplicatedRuns.push({ channel, atMs: hyp[i]!.atMs, words: end - i, text: hyp.slice(i, end).map(item => item.token).join(" ").slice(0, 160), nearEvent: near(hyp[i]!.atMs) })
      i = end
    }
    // Lost runs: four or more consecutive reference tokens missing (deleted, or standing against duplicated text).
    const missing = (op: Op) => op.kind === "del" || op.kind === "sub" && duplicate[op.hyp] === 1
    for (let index = 0; index < ops.length;) {
      if (!missing(ops[index]!)) { index++; continue }
      let end = index
      const run: number[] = []
      while (end < ops.length && (missing(ops[end]!) || ops[end]!.kind === "ins")) { const op = ops[end]!; if (op.kind !== "ins") run.push(op.ref); end++ }
      if (run.length >= 4) { const first = ref[run[0]!]!; lostRuns.push({ channel, atMs: first.atMs, words: run.length, text: run.map(at => ref[at]!.token).join(" ").slice(0, 160), nearEvent: near(first.atMs) }) }
      index = end
    }
  }
  // System attribution: best one-to-one mapping, greedy on the largest counts.
  const pairs = Object.entries(table).flatMap(([speaker, row]) => Object.entries(row).map(([id, count]) => ({ speaker, id, count }))).sort((a, b) => b.count - a.count)
  const usedSpeakers = new Set<string>(), usedIds = new Set<string>()
  let systemCorrect = 0
  for (const pair of pairs) if (!usedSpeakers.has(pair.speaker) && !usedIds.has(pair.id)) { usedSpeakers.add(pair.speaker); usedIds.add(pair.id); systemCorrect += pair.count }
  const systemWords = pairs.reduce((sum, pair) => sum + pair.count, 0)
  const idsPerSpeaker = Object.fromEntries(Object.entries(table).map(([speaker, row]) => [speaker, Object.values(row).filter(count => count >= 3).length]))
  // Drift.
  offsets.sort((a, b) => a.atMs - b.atMs)
  const tenth = Math.max(1, Math.floor(offsets.length / 10))
  let slope: number | null = null
  if (offsets.length >= 10) {
    const mx = offsets.reduce((s, o) => s + o.atMs, 0) / offsets.length, my = offsets.reduce((s, o) => s + o.offsetMs, 0) / offsets.length
    const sxx = offsets.reduce((s, o) => s + (o.atMs - mx) ** 2, 0), sxy = offsets.reduce((s, o) => s + (o.atMs - mx) * (o.offsetMs - my), 0)
    slope = sxx > 0 ? sxy / sxx * 3_600_000 : null
  }
  // Stray script: foreign-script text on a channel where no Japanese was said within three seconds.
  const japanese = reference.utterances.filter(item => item.language === "ja")
  const stray = segments.filter(segment => FOREIGN_SCRIPT.test(segment.text) && !japanese.some(item => item.channel === segment.channel && segment.startMs <= baseMs + item.endMs + 3000 && (segment.endMs ?? segment.startMs) >= baseMs + item.startMs - 3000))
  // Words in silence: hypothesis tokens more than a second inside a reference silence.
  const quiet = (atMs: number) => reference.silences.some(silence => atMs > baseMs + silence.fromMs + 1000 && atMs < baseMs + silence.toMs - 1000)
  const hallucinated = (["mic", "system"] as const).flatMap(channel => hypothesisTokens(segments, channel)).filter(item => quiet(item.atMs))
  // Flicker.
  const interims = input.interims ?? [], finals = input.finals ?? []
  const byId = new Map<string, InterimMessage[]>()
  for (const message of interims) if (message.text) byId.set(message.utteranceId, [...(byId.get(message.utteranceId) ?? []), message])
  let rewrites = 0, finalDiffers = 0
  for (const list of byId.values()) for (let i = 1; i < list.length; i++) { const before = normalise(list[i - 1]!.text), after = normalise(list[i]!.text); if (before.some((token, k) => after[k] !== token)) rewrites++ }
  const finalText = new Map<string, string>()
  for (const final of finals) finalText.set(final.utteranceId, `${finalText.get(final.utteranceId) ?? ""} ${final.segment.text}`)
  for (const [id, list] of byId) { const text = finalText.get(id); if (text !== undefined && normalise(list.at(-1)!.text).join(" ") !== normalise(text).join(" ")) finalDiffers++ }
  const errors = { sub: totals.sub, del: totals.del, ins: totals.ins }
  const nearLost = lostRuns.filter(run => run.nearEvent).reduce((s, r) => s + r.words, 0), nearDup = duplicatedRuns.filter(run => run.nearEvent).reduce((s, r) => s + r.words, 0)
  const startsCapital = segments.filter(segment => /^\P{L}*\p{L}/u.test(segment.text) && !CJK.test(segment.text))
  return {
    words: totals.words, wer: totals.words ? (totals.sub + totals.del + totals.ins) / totals.words : 0, errors, perChannel,
    perLanguage: Object.fromEntries(Object.entries(perLanguage).map(([language, entry]) => [language, { words: entry.words, errorRate: entry.words ? entry.errors / entry.words : 0 }])),
    interjections: { said: interjectionsSaid, recalled: interjectionsRecalled },
    attribution: { micAsMe: micWords ? micAsMe / micWords : 1, micWords, system: systemWords ? systemCorrect / systemWords : 1, systemWords, idsPerSpeaker, table },
    lostRuns, duplicatedRuns,
    boundary: { lostWordsNearEvents: nearLost, duplicatedWordsNearEvents: nearDup, lostWordsElsewhere: lostRuns.reduce((s, r) => s + r.words, 0) - nearLost, duplicatedWordsElsewhere: duplicatedRuns.reduce((s, r) => s + r.words, 0) - nearDup },
    clippedWords: segments.reduce((sum, segment) => sum + segment.text.split(/\s+/).filter(word => /\p{L}-$/u.test(word)).length, 0),
    drift: { measured: offsets.length, medianMs: median(offsets.map(o => o.offsetMs)), micMedianMs: median(offsets.filter(o => o.channel === "mic").map(o => o.offsetMs)), systemMedianMs: median(offsets.filter(o => o.channel === "system").map(o => o.offsetMs)), firstTenthMs: median(offsets.slice(0, tenth).map(o => o.offsetMs)), lastTenthMs: median(offsets.slice(-tenth).map(o => o.offsetMs)), slopeMsPerHour: slope === null ? null : Math.round(slope), maxAbsMs: offsets.length ? Math.max(...offsets.map(o => Math.abs(o.offsetMs))) : null },
    strayScript: { segments: stray.length, characters: stray.reduce((sum, segment) => sum + [...segment.text].filter(char => FOREIGN_SCRIPT.test(char)).length, 0), examples: stray.slice(0, 5).map(segment => `${segment.channel}@${(segment.startMs / 1000).toFixed(1)}s: ${segment.text.slice(0, 60)}`) },
    hallucinatedInSilence: { words: hallucinated.length, examples: hallucinated.slice(0, 5).map(item => `${item.channel}@${(item.atMs / 1000).toFixed(1)}s: ${item.raw}`) },
    punctuation: { sentenceEndRecall: sentenceEnds ? sentenceEndsKept / sentenceEnds : null, sentenceEndPrecision: hypSentenceEnds ? hypSentenceEndsRight / hypSentenceEnds : null, segmentsCapitalised: startsCapital.length ? startsCapital.filter(segment => /^\P{L}*\p{Lu}/u.test(segment.text)).length / startsCapital.length : null, namesCapitalised: names ? namesKept / names : null },
    flicker: { utterances: byId.size, interims: interims.filter(message => message.text).length, rewrites, perFinal: byId.size ? interims.filter(message => message.text).length / byId.size : null, finalDiffersFromLastInterim: finalDiffers },
  }
}
