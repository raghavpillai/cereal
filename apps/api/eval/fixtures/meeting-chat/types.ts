import {ABSTAIN as SHARED_ABSTAIN} from "../../lib/checks"
import type {ChatTurn,Fixture,NotesLevel,Pattern,Question} from "../../lib/types"

/** The shared pattern plus the other ordinary ways of saying something was not said. */
export const ABSTAIN:Pattern=SHARED_ABSTAIN+"|\\bneither\\b|\\bnever\\b|\\bnot (?:discussed|mentioned|covered|said|stated|raised|addressed|specified|decided|agreed|set|come up)\\b"

/** Fixtures for single-meeting chat (finished-note chat and chat during a
 * recording). They live apart from the shared `FIXTURES` list so they do not
 * become generation or workspace scenarios. `set` separates the meetings the
 * prompts were tuned on from the held-out ones that were only ever scored. */
export type McSet="tuned"|"held"
/** What a question stresses; used as the report group. */
export type McTag="recipe"|"early"|"middle"|"late"|"arithmetic"|"notes-only"|"transcript-only"|"not-said"|"undecided"|"follow-up"|"language"|"citation"|"synthesis"|"speaker"|"stt"|"cross-session"
export interface McQuestion extends Question {tag:McTag}
export type McLiveKind=
  /** "What did I miss?" and its paraphrases: the last beats, not the meeting. */
  |"recency"
  /** A fact stated seconds ago. */
  |"just-said"
  /** The last utterance is still being spoken. */
  |"unfinished"
  /** The topic has not come up yet at this point. */
  |"not-yet"
  /** Never said at any point (asked mid-meeting). */
  |"not-said"
  /** A position that changes later; the answer must be the current one. */
  |"superseded"
  /** The line has not reached the server yet (batch delivery). */
  |"lag"
  /** Depends on earlier turns of the chat thread. */
  |"follow-up"
  /** Something from much earlier, asked late in a live meeting. */
  |"recall"
  |"so-far"|"speaker"|"notes"
export interface McLive {
  id:string;kind:McLiveKind
  /** Verbatim substring of the script line the question follows. */
  after:string
  /** Seconds after that line ends (default 5). */
  delaySeconds?:number
  delivery?:"stream"|"ios"|"desktop"
  /** Ask while the line after `after` is still being spoken: the server holds
   * its first `fraction` of words, stored as a non-final segment unless `final`. */
  partial?:{fraction:number;final?:boolean}
  question:string;history?:ChatTurn[];reference:string
  must:Pattern[]
  /** Only said later; appearing in the answer is a leak. */
  later?:Pattern[]
  /** Wrong for another reason (for example stale content in a catch-up). */
  forbid?:Pattern[]
  /** Graded as a built-in recipe. `true` is "What did I miss": no
   * introduction, one to three bullets, no quotation marks. `"smart"` is
   * "Make me sound smart" (exactly two first-person bullets) and `"questions"`
   * is "Suggest questions" (exactly three questions). */
  recipe?:boolean|"smart"|"questions"
  notes?:NotesLevel
  fast?:boolean
}
export interface McFixture extends Fixture {set:McSet;questions:McQuestion[];live:McLive[]}
