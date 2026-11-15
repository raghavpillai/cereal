import type {ChatInput,TranscriptSegmentDTO} from "@meeting-notes/contracts"

export type ChatTurn=ChatInput["history"][number]

export type Channel="mic"|"system"
export type NotesLevel="none"|"sparse"|"detailed"
export type Feature="generation"|"note-chat"|"workspace-chat"|"live-chat"
/** JavaScript regular-expression source, matched case-insensitively. */
export type Pattern=string

/** How the audio reached the server, which decides the shape of the transcript.
 * - desktop: microphone and system audio are separate channels, uploaded as
 *   60-second parts after Stop. Speaker numbers restart in every part, and all
 *   of one speaker's words between two other speakers on the same channel in a
 *   part form one segment (so a lone microphone speaker is one segment/minute).
 * - ios: one microphone channel that hears everyone, in 120-second parts.
 * - stream: one segment per turn with stable speaker numbers; what a true live
 *   transport could deliver. No client produces this today. */
export type Capture="desktop"|"ios"|"stream"

export interface GoldItem {id:string;text:string;must:Pattern[]}
export interface GoldAction extends GoldItem {owner:string;due?:string;
  /** Matches only when the owner is attached to the task. */
  ownerMust:Pattern}
/** Something that was explicitly not said or not decided; a hallucination probe. */
export interface NotSaid {id:string;claim:string;forbid:Pattern[]}
export interface Gold {decisions:GoldItem[];actionItems:GoldAction[];facts:GoldItem[];notSaid:NotSaid[]}

export type QuestionKind="factual"|"synthesis"|"absent"|"follow-up"|"citation"
export interface Question {
  id:string;kind:QuestionKind;question:string
  /** Earlier turns, fixed so follow-up scenarios are reproducible. */
  history?:ChatTurn[]
  reference:string
  must:Pattern[];forbid?:Pattern[]
  /** Verbatim transcript substrings that support the reference answer. */
  support?:string[]
  /** Include the reference summary as the note's generated document. */
  withSummary?:boolean
  fast?:boolean
}
export interface Fixture {
  id:string;title:string;kind:string
  /** Meeting date relative to the day the evaluation runs (workspace chat). */
  daysAgo:number
  capture:Exclude<Capture,"stream">
  /** Words per minute used to time turns. */
  wpm:number
  /** Speaker name -> channel. With `ios` capture every speaker is on the microphone. */
  people:Record<string,Channel>
  /** Optional attached calendar meeting (invitees are context, not attendance). */
  attendees?:Array<{name:string;email:string;self?:boolean}>
  /** One turn per line: `Name: words`. `~Name:` starts while the previous turn
   * is still going. `[+90]` inserts a 90-second pause. */
  script:string
  notes:{sparse:string;detailed:string}
  gold:Gold
  questions:Question[]
  fast?:boolean
}
export interface Turn {index:number;speaker:string;channel:Channel;text:string;startMs:number;endMs:number}
export interface CompiledFixture {fixture:Fixture;capture:Capture;turns:Turn[];segments:TranscriptSegmentDTO[];
  /** Segment ID -> indexes of the turns merged into it. */
  segmentTurns:Map<string,number[]>;durationMs:number;duration:string}

export interface CallMetric {purpose:"generation"|"answer"|"planner"|"judge"|"other";model:string;ms:number;ttftMs:number|null;status:number;
  inputTokens:number;outputTokens:number;tokensEstimated:boolean;
  /** Of `inputTokens`: read from the provider's prompt cache, and written to it, as the provider reports them (0 when it does not). */
  cachedTokens:number;cacheWriteTokens:number
  costUsd:number;costEstimated:boolean;finishReason:string|null;parameters:Record<string,unknown>;requestSha256:string;requestCharacters:number}
export interface Check {id:string;group:"required"|"hallucination"|"grounding"|"citation"|"format"|"temporal"|"owner"|"notes-fidelity";pass:boolean;
  /** Soft checks are reported but do not fail a run. */
  soft?:boolean;detail?:string}
export interface JudgeScore {score:number|null;reason:string}
export interface Judgement {model:string;promptVersion:string;scores:Record<string,JudgeScore>;unsupportedClaims:string[];citations?:Array<{marker:string;supported:boolean;reason:string}>;overall:string;raw?:string;error?:string;usage?:{inputTokens:number;outputTokens:number;costUsd:number;ms:number;attempts:number}}
export interface RunResult {
  feature:Feature;scenario:string;fixture:string;variant:string;rep:number
  input:{question?:string;history?:ChatTurn[];notesLevel?:NotesLevel;atMs?:number;delivery?:string;reference?:string}
  ok:boolean;error?:string;output:string
  citations?:Array<{id:string;source:string;noteId:string;segmentId:string|null;startMs:number|null;quote:string}>
  tools?:Array<{name:string;status:string;label:string;resultCount?:number}>
  checks:Check[];judge?:Judgement
  perf:{totalMs:number;ttftMs:number|null;preparationMs:number|null;inputTokens:number;cachedTokens:number;outputTokens:number;costUsd:number;estimated:boolean;calls:CallMetric[];toolCalls:Record<string,number>}
  context?:{segmentsAvailable:number;segmentsTotal:number;transcriptCharacters:number;deliveredUntilMs?:number}
}
