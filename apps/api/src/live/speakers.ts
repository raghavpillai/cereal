/**
 * Who said the words of a final, before they are numbered and stored.
 *
 * The streaming diariser labels words with `speaker_0`, `speaker_1`, ... that mean something only inside
 * one provider session. Measured on 2026-11-16 with Scribe v3 Live (`.local/live-only/` holds the runs):
 *
 *  - The first utterance of a provider session comes back with no speaker on any word: the diariser has
 *    not formed a voice yet. Later utterances can also carry words without one. Numbering "no speaker"
 *    as a voice of its own turned one person saying "Test. Test, test." into Speaker 1 and Speaker 2.
 *  - On a single voice the diariser sometimes starts a second id for a short stretch ("Test, test,
 *    test" became a third speaker in the screenshot that started this).
 *  - Different people taking real turns (12-17 s each in the four-voice fixture) are separated well,
 *    although two similar voices can share one id.
 *
 * The rules, per channel:
 *
 *  1. Words without a speaker belong to the speaker of the word before them in the same final, else to
 *     the first word after them that has one, else to whoever spoke last in this provider session. Only
 *     at the very start of a provider session is there nobody to give them to; they are then kept under a
 *     placeholder that the first voice the diariser names takes over (it nearly always is that voice).
 *  2. The microphone is the person recording, "Me", and may be shared (an iPhone on a meeting-room table).
 *     A diariser id other than Me's becomes a voice of its own once it has said enough to be trusted: at
 *     least `MIC_VOICE_EVIDENCE_MS` over at least two turns (in any number of finals), or one turn of
 *     `MIC_SECOND_VOICE_MS` and `MIC_SECOND_VOICE_WORDS` words. Until then its words are stored as Me's; when
 *     it is trusted, those words move to it (the session sends `relabels`). A stray id on one person's voice
 *     ("Test, test, test" in the screenshot that started this) never gets that far in our runs.
 *     Me is the voice that has spoken most: when another trusted voice has said clearly more (a quarter
 *     more, and at least `MIC_OWNER_FLIP_MS`), the labels change places, everywhere in the note.
 *  3. System audio keeps the diariser's ids, numbered once for the whole note in the order they first
 *     speak (`planAppend`), so Speaker 2 is the same person from the first minute to the last.
 *
 * Across provider sessions (a Resume after the hold ran out, a long outage) ids start again. The
 * microphone's "Me" carries over by rule 2. On system audio, if the channel had exactly one voice, the
 * first voice of the new provider session is taken to be it; otherwise new voices get new numbers. With
 * no batch recogniser there is no evidence to do better, and a wrong merge is worse than a new number.
 */
import type { SpeakerTurn, TimedWord } from "./coverage"
import type { AudioChannelName } from "./repository"

/** The key of the microphone's own voice, numbered 1 ("Me"). */
export const ME = "me"
/** Words of an unknown speaker at the start of a provider session (rule 1). */
export const UNKNOWN = "_"
export const MIC_SECOND_VOICE_MS = 5_000, MIC_SECOND_VOICE_WORDS = 8, MIC_VOICE_EVIDENCE_MS = 4_000, MIC_OWNER_FLIP_MS = 15_000

/** What one provider session has decided about its speakers so far. */
export interface Attribution {
  /** The provider speaker who spoke last. */
  last?: string
  /** Microphone: the diariser id stored as Me's voice (the key `ME`), once one has spoken. */
  owner?: string
  /** Microphone: ids trusted as voices of their own (rule 2), stored under their own keys. */
  readonly trusted: Set<string>
  /** Microphone: speech heard so far per diariser id, in milliseconds and turns. */
  readonly evidence: Map<string, { ms: number; turns: number }>
  /** The placeholder of rule 1 is waiting for the first named voice. */
  placeholder: boolean
}
export const newAttribution = (): Attribution => ({ trusted: new Set(), evidence: new Map(), placeholder: false })

export interface HeardWord extends TimedWord { readonly speaker?: string }

/** The key a provider speaker is stored under on `channel` (rule 2). */
export function canonicalSpeaker(channel: AudioChannelName, state: Attribution, speaker: string | undefined): string | undefined {
  if (channel !== "mic") return speaker
  return speaker !== undefined && speaker !== state.owner && state.trusted.has(speaker) ? speaker : ME
}
/** A stored turn, with the diariser id it came from (`source`) where its key is not that id. */
export interface AttributedTurn extends SpeakerTurn { readonly source: string }

/**
 * Speaker turns of one final. `speakers` is the provider session's map of keys to note numbers; when the
 * placeholder is taken over (rule 1) the first named voice is given its number there.
 */
export function attributeWords(channel: AudioChannelName, state: Attribution, words: ReadonlyArray<HeardWord>, speakers: Record<string, number>): { turns: AttributedTurn[]; trusted: string[] } {
  // Rule 1: fill in the words without a speaker.
  const firstNamed = words.find(word => word.speaker !== undefined)?.speaker
  let previous: string | undefined
  const filled = words.map(word => {
    const speaker = word.speaker ?? previous ?? firstNamed ?? state.last ?? UNKNOWN
    previous = speaker
    return { word, speaker }
  })
  for (const item of filled) {
    if (item.speaker === UNKNOWN) { state.placeholder = true; continue }
    // The first named voice takes over what the placeholder was given.
    if (state.placeholder && speakers[item.speaker] === undefined && speakers[UNKNOWN] !== undefined && channel !== "mic") speakers[item.speaker] = speakers[UNKNOWN]!
    state.placeholder = false
  }
  const turns: Array<{ speakerKey: string; words: TimedWord[] }> = []
  for (const { word, speaker } of filled) {
    const last = turns.at(-1)
    if (last && last.speakerKey === speaker) last.words.push(word); else turns.push({ speakerKey: speaker, words: [word] })
  }
  const named = filled.filter(item => item.speaker !== UNKNOWN).at(-1)?.speaker
  if (named !== undefined) state.last = named
  if (channel !== "mic") return { turns: turns.map(turn => ({ ...turn, source: turn.speakerKey })), trusted: [] }
  // Rule 2: evidence per diariser id, across finals.
  const trusted: string[] = []
  state.owner ??= filled.find(item => item.speaker !== UNKNOWN)?.speaker
  for (const turn of turns) {
    const key = turn.speakerKey
    if (key === UNKNOWN) continue
    const span = turn.words.at(-1)!.endMs - turn.words[0]!.startMs, seen = state.evidence.get(key) ?? { ms: 0, turns: 0 }
    seen.ms += span; seen.turns++; state.evidence.set(key, seen)
    if (key === state.owner || state.trusted.has(key)) continue
    if (seen.ms >= MIC_VOICE_EVIDENCE_MS && seen.turns >= 2 || span >= MIC_SECOND_VOICE_MS && turn.words.length >= MIC_SECOND_VOICE_WORDS) { state.trusted.add(key); trusted.push(key) }
  }
  // Turns of different diariser ids stay apart, even when both are stored as Me: a later trust can then move one.
  return { turns: turns.map(turn => ({ speakerKey: canonicalSpeaker(channel, state, turn.speakerKey)!, words: turn.words, source: turn.speakerKey })), trusted }
}
