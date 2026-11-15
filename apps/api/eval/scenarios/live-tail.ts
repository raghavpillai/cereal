/** Chat during a recording that is transcribed live.
 *
 * The server holds stored segments for every utterance that has finished, with voices numbered once
 * per channel, and a live session holding the utterance that is still being spoken. The question
 * arrives a fraction of a second after the words it is about, so the answer has to come from the
 * unfinished tail: text that is in the context but must never be cited.
 *
 * Three situations, each anchored to a line of an existing fixture:
 *  - `tail-complete`: the sentence was finished 0.6 s ago and has not been stored yet;
 *  - `tail-fact-spoken`: the sentence is still in progress and already contains the fact;
 *  - `tail-fact-pending`: the sentence is in progress and the fact has not been said. */
import type {ChatInput,TranscriptSegmentDTO} from "@meeting-notes/contracts"
import {compile,fixtureById,fixtureNote} from "../lib/fixtures"
import {runChat} from "../lib/harness"
import {chatFormat,citationChecks,grounding,matches,mustAll,mustNot} from "../lib/checks"
import {clock,findTurn,renderTruth,stableUuid} from "../lib/transcript"
import {citedClaims,completed,resolver,type Scenario} from "../lib/scenario"
import type {Pattern} from "../lib/types"

export interface TailCheckpoint {
  id:string;fixture:string;kind:"tail-complete"|"tail-fact-spoken"|"tail-fact-pending"
  /** Verbatim substring of the script line being spoken (or just finished) when the question is asked. */
  line:string
  /** For a sentence in progress: the last words that have been heard, verbatim. Absent: the whole line. */
  through?:string
  question:string;reference:string
  must:Pattern[]
  /** Not yet said at the moment of the question. */
  later?:Pattern[]
  fast?:boolean
}
const IN_PROGRESS="still (?:being )?(?:said|spoken|speaking|talking|in progress|mid|finishing)|in progress|mid.sentence|hasn.t (?:finished|been said|said|given|stated|named)|has not (?:finished|been said|said|given|stated)|not (?:yet )?(?:been )?(?:said|finished|stated|given|specified)|yet to|so far|unfinished|cut off|cuts off|trail|incomplete|haven.t (?:said|given|stated)|doesn.t (?:say|have)|no (?:date|dates|number) (?:yet|so far)"
export const TAIL_CHECKPOINTS:TailCheckpoint[]=[
  {id:"reconcile-hours",fixture:"discovery",kind:"tail-complete",fast:true,line:"we spend roughly twelve hours a week across the sites just reconciling",
    question:"How much time did they just say they spend reconciling the schedule?",reference:"Roughly twelve hours a week across the three sites.",must:["twelve hours|12 hours"]},
  {id:"revenue",fixture:"numbers",kind:"tail-complete",line:"Four point eight two million dollars, against a plan of five point one.",
    question:"What did he just say Q3 revenue was?",reference:"$4.82 million, against a plan of $5.1 million.",must:["4\\.82|four point eight two"],later:["19\\.6","71\\.4"]},
  {id:"increase-spoken",fixture:"numbers",kind:"tail-fact-spoken",line:"I want six percent, on renewals only, starting January first.",through:"I want six percent, on renewals only,",
    question:"What renewal price increase is he asking for right now?",reference:"Six percent, on renewals only; he is still mid-sentence.",must:["six percent|6 ?%|6 percent"],later:["January"]},
  {id:"freeze-pending",fixture:"planning",kind:"tail-fact-pending",fast:true,line:"The data warehouse migration has a freeze from August eleventh to August fifteenth.",through:"The data warehouse migration has a freeze from",
    question:"What dates is the warehouse freeze?",reference:"He has just started saying it: the migration has a freeze, but the dates have not been spoken yet.",must:[IN_PROGRESS],later:["August (?:11|eleventh)|Aug(?:ust)? 11","fifteenth|August 15"]},
  {id:"freeze-spoken",fixture:"planning",kind:"tail-fact-spoken",line:"The data warehouse migration has a freeze from August eleventh to August fifteenth.",through:"The data warehouse migration has a freeze from August eleventh to",
    question:"When does the warehouse freeze start?",reference:"August 11; the sentence is still in progress and the end date has not been said yet.",must:["August (?:11|eleventh)|Aug(?:ust)? 11"],later:["fifteenth|August 15"]},
]

export const liveTailScenarios=(now:Date):Scenario[]=>TAIL_CHECKPOINTS.map(checkpoint=>{
  const fixture=fixtureById(checkpoint.fixture),built=compile(fixture,"stream"),turn=findTurn(built,checkpoint.line)
  const heard=checkpoint.through?turn.text.slice(0,turn.text.indexOf(checkpoint.through)+checkpoint.through.length):turn.text
  if(checkpoint.through&&!turn.text.includes(checkpoint.through))throw new Error(`${fixture.id}/${checkpoint.id}: "through" is not part of the line`)
  // Asked 0.6 s after the last word heard, before the utterance can have become final.
  const heardEndMs=Math.round(turn.startMs+(turn.endMs-turn.startMs)*heard.length/turn.text.length),atMs=heardEndMs+600
  // What live transcription has stored: one segment per finished utterance, in time order, voices numbered per channel.
  const numbers:Record<string,Map<string,number>>={mic:new Map(),system:new Map()}
  const speakerOf=(speaker:string,channel:"mic"|"system")=>{const known=numbers[channel]!;if(!known.has(speaker))known.set(speaker,known.size+1);return known.get(speaker)!}
  const segments:TranscriptSegmentDTO[]=built.turns.filter(candidate=>candidate.index<turn.index).map(candidate=>{
    const number=speakerOf(candidate.speaker,candidate.channel)
    return {id:stableUuid("live-segment",fixture.id,candidate.index),channel:candidate.channel,speakerId:`${candidate.channel}:n:${number}`,speakerLabel:`Speaker ${number}`,text:candidate.text,isFinal:true,startMs:candidate.startMs,endMs:candidate.endMs}
  })
  const tailSpeaker=speakerOf(turn.speaker,turn.channel)
  const note=fixtureNote(fixture,{now,level:"none",capture:"stream",segments,recording:true}),resolve=resolver([note])
  const hub={size:1,full:()=>false,connect:async()=>{throw new Error("not used")},active:()=>true,pendingRange:()=>null,flush:async()=>{},end:async()=>{},dispose:async()=>{},
    snapshot:()=>({asOfMs:atMs,state:"live" as const,tail:[{channel:turn.channel,text:heard,startMs:turn.startMs,speakerId:`${turn.channel}:n:${tailSpeaker}`}]})}
  const input:ChatInput={message:checkpoint.question,history:[],includeSources:true,timeZone:"UTC"}
  const spoken=built.turns.filter(candidate=>candidate.index<turn.index),unspoken=built.turns.filter(candidate=>candidate.index>turn.index)
  const said=[...spoken.map(candidate=>candidate.text),heard].join("\n"),later=(checkpoint.later??[]).filter(source=>!matches(said,source))
  const tailStart=heard.split(/\s+/).slice(0,5).join(" ")
  return {
    feature:"live-chat",id:`live-chat/${fixture.id}/tail-${checkpoint.id}`,fixture:fixture.id,variant:`${checkpoint.kind}/live/+0.6s`,fast:!!checkpoint.fast,
    input:{question:checkpoint.question,atMs,delivery:"live",reference:checkpoint.reference},
    context:{segmentsAvailable:segments.length,segmentsTotal:built.segments.length,transcriptCharacters:segments.reduce((sum,segment)=>sum+segment.text.length,0)+heard.length,deliveredUntilMs:heardEndMs},
    execute:target=>runChat(target,[note],note.id,input,new Map(),hub),
    check:execution=>!execution.ok?[completed(execution)]:[
      completed(execution),
      mustAll(execution.output,checkpoint.must,"uses-what-was-said"),
      ...(later.length?[mustNot(execution.output,later,"nothing-from-later")]:[]),
      // The tail is context, never a source: no citation may quote it.
      {id:"tail-not-cited",group:"citation" as const,pass:!execution.citations.some(citation=>citation.quote.includes(tailStart)),...(execution.citations.some(citation=>citation.quote.includes(tailStart))?{detail:"a citation quotes the unfinished utterance"}:{})},
      ...citationChecks(execution.output,{citations:execution.citations,resolve,required:false}),
      ...chatFormat(execution.output,checkpoint.question),
      ...grounding(execution.output,[fixture.title,said,checkpoint.question].join("\n")),
    ],
    judge:execution=>({feature:"live-chat",
      task:`The meeting "${fixture.title}" is still being recorded and is transcribed live. The user asks a question ${clock(atMs)} into it, 0.6 seconds after the last words below were spoken. The assistant has every finished sentence as stored transcript, and the sentence ${checkpoint.kind==="tail-complete"?"that has just ended":"that is still being spoken"} as unfinished text that it may use but must not cite. ${checkpoint.kind==="tail-fact-pending"?"The fact asked about has NOT been spoken yet: the right answer says the sentence is still in progress and does not guess.":"The fact asked about has been spoken: the right answer gives it"+(checkpoint.kind==="tail-fact-spoken"?", ideally noting the sentence is still in progress.":".")} Using anything said after that moment would be an error. Judge faithfulness against what had been said by then, and usefulness as an answer for someone in the meeting.\nQuestion: ${checkpoint.question}`,
      source:`--- Said before the question (${clock(atMs)}) ---\n${renderTruth(spoken.slice(-30))}\n\n--- ${checkpoint.kind==="tail-complete"?"Just finished, not yet stored":"Being spoken as the question is asked; only these words had been heard"} ---\n${turn.speaker}: ${heard}${checkpoint.through?" …":""}\n\n--- Not yet said at that moment (must not appear) ---\n${renderTruth(unspoken.slice(0,25))||"(nothing)"}`,
      reference:checkpoint.reference,output:execution.output,citations:citedClaims(execution,resolve)}),
  }
})
