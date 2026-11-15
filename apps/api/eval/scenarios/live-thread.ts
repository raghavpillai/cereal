/** Chat during a recording as a thread: several questions in one conversation while the meeting goes on.
 *
 * The other live scenarios ask one question each, so they exercise the first request of a thread.
 * These exercise what follows it: every turn after the first is a request that repeats the earlier
 * ones and appends only what has been said since (`src/ai/meeting-chat.ts`, "a meeting in progress"),
 * and the answer has to come from that newest part even when an earlier answer in the thread said
 * something else. Each thread ends with a question whose answer changed, or only came into being,
 * after an earlier turn:
 *
 *  - asked before a fact is said, then again once it has been (`review-date`);
 *  - a proposal, then the sentence that replaces it while it is still being spoken (`increase`);
 *  - nothing proposed yet, then the terms (`pilot-terms`).
 *
 * The `window-` threads ask about a stretch of time: "in the last minute", "in the last 30 seconds", "around the
 * ten-minute mark". Each is asked as the first question of a thread deep into the meeting, where the transcript
 * so far arrives as one long block, and again after an earlier turn. A right answer uses what was said inside the
 * stretch and nothing from well outside it (`only-the-time-asked-about`), which the model can only do from the
 * time markers in the transcript and the time the transcript ends at.
 *
 * The transcript is what live transcription stores (one segment per finished utterance, voices
 * numbered per channel) and the sentence in progress reaches chat as the live session's unfinished
 * tail, as in `live-tail.ts`. The client's side is reproduced too: each request carries the earlier
 * questions and the answers actually given as `history`, which is how the server finds the thread. */
import type {ChatInput,TranscriptSegmentDTO} from "@meeting-notes/contracts"
import {compile,fixtureById,fixtureNote} from "../lib/fixtures"
import {runChat,type Execution} from "../lib/harness"
import {chatFormat,citationChecks,grounding,matches,mustAll,mustNot,ABSTAIN} from "../lib/checks"
import {clock,findTurn,renderTruth,stableUuid} from "../lib/transcript"
import {citedClaims,completed,resolver,type Scenario} from "../lib/scenario"
import type {Check,ChatTurn,Pattern,Turn} from "../lib/types"

export interface ThreadStep {
  /** Verbatim substring of the script line the question follows, or is asked during. */
  line:string
  /** Asked while that line is being spoken: the last words heard, verbatim. Absent: asked `delaySeconds` (5) after it ends. */
  through?:string
  delaySeconds?:number
  question:string;reference:string
  must:Pattern[]
  /** Not yet said at the moment of this question. */
  later?:Pattern[]
  /** Said, but outside the stretch of time the question asks about ("in the last minute", "around the ten-minute mark"). */
  outside?:Pattern[]
}
export interface LiveThread {id:string;fixture:string;fast?:boolean;steps:ThreadStep[]}
const NOT_YET="still (?:being )?(?:said|spoken|speaking|talking|in progress|mid|finishing)|in progress|mid.sentence|not (?:yet )?(?:been )?(?:said|set|given|stated|mentioned|specified|scheduled|decided)|hasn.t|has not|haven.t|have not|no (?:review )?date|yet"
export const LIVE_THREADS:LiveThread[]=[
  {id:"review-date",fixture:"planning",fast:true,steps:[
    {line:"Pencilled.",question:"Is the insurance eligibility pilot in the Q3 plan right now?",reference:"Not as things stand: it has just been pencilled for Q4, with reporting in Q3 instead, because nobody present could confirm the vendor budget.",
      must:["Q4|pencil|defer"],later:["ninety.day|90.day","October (?:20|twentieth)"]},
    {line:"Eligibility pilot is in for Q3. Reporting goes to Q4. Does anyone object?",question:"And now?",reference:"Yes: reversed. The pilot is in for Q3, conditional on a ninety-day exit clause, and reporting moves to Q4.",
      must:["\\bin\\b[^.\\n]{0,40}Q3|is in|yes|back in|revers"],later:["October (?:20|twentieth)","September (?:15|fifteenth)"]},
    {line:"The pilot goes live September fifteenth. Let's review it on October twentieth",through:"The pilot goes live September fifteenth.",question:"When do we review the pilot?",
      reference:"No review date has been said yet. The pilot goes live September 15; she is still speaking.",must:[NOT_YET],later:["October (?:20|twentieth)|Oct(?:ober)? 20"]},
    {line:"The pilot goes live September fifteenth. Let's review it on October twentieth",question:"When do we review the pilot?",
      reference:"October 20, five weeks after it goes live on September 15.",must:["October (?:20|twentieth)|Oct(?:ober)? 20"],later:["Alder Street"]},
  ]},
  {id:"increase",fixture:"numbers",fast:true,steps:[
    {line:"I proposed eight percent.",question:"What renewal price increase are we going with?",reference:"Not decided at this point. Greta proposed eight percent last month.",
      must:["eight|8 ?%|8 percent","propos|not (?:yet )?decided|no decision|hasn't|has not|undecided|yet"],later:["six percent|6 ?%|6 percent","January"]},
    {line:"I want six percent, on renewals only, starting January first.",through:"I want six percent, on renewals only,",question:"And now?",
      reference:"Six percent, on renewals only; he is still mid-sentence.",must:["six percent|6 ?%|6 percent"],later:["January"]},
    {line:"I want six percent, on renewals only, starting January first.",question:"Starting when?",reference:"January first.",must:["January"]},
  ]},
  {id:"pilot-terms",fixture:"discovery",steps:[
    {line:"Those are the two. Everything else is nice to have.",question:"What did I miss?",reference:"Their two hard requirements: an offline mode and direct integration with the Stratus warehouse system; everything else is nice to have.",
      must:["offline","Stratus|warehouse system|integrat"],later:["sixty thousand|60,?000|60k","4,?500"]},
    {line:"Above that it becomes a board conversation and it won't happen this year.",question:"What pilot terms have been proposed so far?",reference:"None yet.",
      must:[ABSTAIN],later:["4,?500|four thousand five hundred","ninety days|90 days|90.day","twenty.five users|25 users"]},
    {line:"I like that. Tacoma pilot, Reno offline test.",question:"And now?",reference:"Ninety days, up to twenty-five users, a flat $4,500 credited against year one, run at Tacoma with a two-day offline test in Reno. Proposed, not formally agreed.",
      must:["4,?500|four thousand five hundred|ninety days|90 days|90.day|twenty.five users|25 users"],later:["nineteenth|19th"]},
  ]},
]

// Clinic Desk Q3 planning, around 17 to 19 minutes in: the rules of self-rescheduling are settled one after another.
const EARLIER:ThreadStep={line:"Hold that thought, I want to come back to it when we decide.",question:"What does the Meadowbrook contact want to see?",reference:"That multi-location is visibly underway, not that it has shipped.",must:["multi.?location|underway|real|progress"]}
const LAST_MINUTE:ThreadStep={line:"That default matters for the estimate.",delaySeconds:2,question:"What did I miss in the last minute?",
  reference:"A patient can move the same appointment twice, then the link says to call the clinic. They see slots of the same appointment type with the same practitioner by default; any practitioner is a clinic setting and would add three weeks to the estimate.",
  must:["twice|two (?:self.)?reschedul|practitioner|three (?:more |extra |additional )?weeks"],outside:["seven days|7 days","date of birth|signed link"],later:["cancel"]}
const LAST_HALF_MINUTE:ThreadStep={line:"Then cancel is out for this quarter. Self reschedule only, no self cancel.",delaySeconds:1,question:"What was said in the last 30 seconds?",
  reference:"About sixty percent of patients who cancel by phone rebook on the same call, so cancelling is out for this quarter: self-reschedule only, no self-cancel.",
  must:["sixty percent|60 ?%|cancel[^.\\n]{0,80}(?:out|not)|no self.?cancel|(?:reschedule|rescheduling) only|(?:ruled out|out)[^.\\n]{0,40}cancel"],outside:["practitioner","twice","seven days|7 days","date of birth|signed link"],later:["help article","nineteen"]}
const TEN_MINUTE_MARK:ThreadStep={line:"I'd rather ship reschedule in August than both in October.",delaySeconds:3,question:"What happened around the ten-minute mark?",
  reference:"Hamid called his forty percent ticket reduction an assumption, then Beatriz reported the research: eleven clinics interviewed, eight asked for self-reschedule unprompted, only three run more than one site.",
  must:["eleven|\\b11\\b|forty percent|40 ?%|assumption|interview|research"],outside:["date of birth|signed link","net promoter|\\bNPS\\b","1,?180|one thousand one hundred eighty","carrier","no self.?cancel|cancel is out","twenty.four hours?|24.hour|24 hours"],later:["nineteen"]}
export const WINDOW_THREADS:LiveThread[]=[
  {id:"window-last-minute-first",fixture:"planning",steps:[LAST_MINUTE]},
  {id:"window-last-minute-later",fixture:"planning",steps:[EARLIER,LAST_MINUTE]},
  {id:"window-last-30s-first",fixture:"planning",steps:[LAST_HALF_MINUTE]},
  {id:"window-last-30s-later",fixture:"planning",steps:[{...LAST_MINUTE,question:"How many times can a patient move the same appointment?",reference:"Twice.",must:["twice|two"],outside:[]},LAST_HALF_MINUTE]},
  {id:"window-ten-minute-mark-first",fixture:"planning",steps:[TEN_MINUTE_MARK]},
  {id:"window-ten-minute-mark-later",fixture:"planning",steps:[EARLIER,TEN_MINUTE_MARK]},
]

/** What the checks need from the turns before the last one. */
const earlier=new WeakMap<Execution,Array<{output:string;ok:boolean}>>()
export const liveThreadScenarios=(now:Date):Scenario[]=>[...LIVE_THREADS,...WINDOW_THREADS].map(thread=>{
  const fixture=fixtureById(thread.fixture),built=compile(fixture,"stream")
  const numbers:Record<string,Map<string,number>>={mic:new Map(),system:new Map()}
  const voice=(turn:Turn)=>{const known=numbers[turn.channel]!;if(!known.has(turn.speaker))known.set(turn.speaker,known.size+1);return known.get(turn.speaker)!}
  const segment=(turn:Turn):TranscriptSegmentDTO=>{const number=voice(turn);return {id:stableUuid("live-thread",fixture.id,turn.index),channel:turn.channel,speakerId:`${turn.channel}:n:${number}`,speakerLabel:`Speaker ${number}`,text:turn.text,isFinal:true,startMs:turn.startMs,endMs:turn.endMs}}
  const all=built.turns.map(segment)
  const moments=thread.steps.map((step,index)=>{
    const anchor=findTurn(built,step.line)
    if(step.through&&!anchor.text.includes(step.through))throw new Error(`${fixture.id}/${thread.id}: "through" is not part of the line`)
    const heard=step.through?anchor.text.slice(0,anchor.text.indexOf(step.through)+step.through.length):null
    const atMs=heard?Math.round(anchor.startMs+(anchor.endMs-anchor.startMs)*heard.length/anchor.text.length)+600:anchor.endMs+(step.delaySeconds??5)*1000
    // Someone else may be mid-sentence when a question follows a line: that is the tail then.
    const speaking=heard?anchor:built.turns.find(turn=>turn.startMs<atMs&&turn.endMs>atMs)
    const tailText=heard??(speaking?speaking.text.split(/\s+/).slice(0,Math.max(1,Math.floor(speaking.text.split(/\s+/).length*(atMs-speaking.startMs)/(speaking.endMs-speaking.startMs)))).join(" "):"")
    const spoken=built.turns.filter(turn=>turn.endMs<=atMs&&turn!==speaking),said=[...spoken.map(turn=>turn.text),tailText].join("\n")
    return {step,index,atMs,speaking,tailText,spoken,said,later:(step.later??[]).filter(source=>!matches(said,source)),segments:all.filter((_,turnIndex)=>spoken.includes(built.turns[turnIndex]!))}
  })
  for(const [index,moment] of moments.entries())if(index&&moment.atMs<=moments[index-1]!.atMs)throw new Error(`${fixture.id}/${thread.id}: steps must be in the order they happen`)
  const last=moments.at(-1)!,final=fixtureNote(fixture,{now,level:"none",capture:"stream",segments:last.segments,recording:true}),resolve=resolver([final])
  return {
    feature:"live-chat",id:`live-chat/${fixture.id}/thread-${thread.id}`,fixture:fixture.id,variant:`thread/${thread.steps.length} turns/live`,fast:!!thread.fast,
    input:{question:last.step.question,atMs:last.atMs,delivery:"live",reference:last.step.reference},
    context:{segmentsAvailable:last.segments.length,segmentsTotal:built.segments.length,transcriptCharacters:last.segments.reduce((sum,item)=>sum+item.text.length,0),deliveredUntilMs:last.atMs},
    async execute(target){
      const history:ChatTurn[]=[],before:Array<{output:string;ok:boolean}>=[]
      let execution:Execution|undefined,cost=0
      for(const moment of moments){
        const note=fixtureNote(fixture,{now,level:"none",capture:"stream",segments:moment.segments,recording:true})
        const hub={size:1,full:()=>false,connect:async()=>{throw new Error("not used")},active:()=>true,pendingRange:()=>null,flush:async()=>{},end:async()=>{},dispose:async()=>{},
          snapshot:()=>({asOfMs:moment.atMs,state:"live" as const,tail:moment.speaking&&moment.tailText?[{channel:moment.speaking.channel,text:moment.tailText,startMs:moment.speaking.startMs,speakerId:`${moment.speaking.channel}:n:${voice(moment.speaking)}`}]:[]})}
        const input:ChatInput={message:moment.step.question,history:[...history],includeSources:true,timeZone:"UTC"}
        execution=await runChat(target,[note],note.id,input,new Map(),hub)
        cost+=execution.perf.costUsd
        if(moment!==last){
          before.push({output:execution.output,ok:execution.ok})
          if(!execution.ok)break
          // What a client holds after an answer, and sends back with the next question.
          history.push({role:"user",content:moment.step.question},{role:"assistant",content:execution.output})
        }
      }
      const result={...execution!,perf:{...execution!.perf,costUsd:cost}}
      earlier.set(result,before)
      return result
    },
    check:execution=>{
      const before=earlier.get(execution)??[],checks:Check[]=[completed(execution)]
      for(const [index,turn] of before.entries()){
        const moment=moments[index]!
        checks.push({...mustAll(turn.output,moment.step.must,`turn-${index+1}-uses-what-was-said`),...(turn.ok?{}:{pass:false,detail:"the turn did not complete"})})
        if(moment.later.length)checks.push(mustNot(turn.output,moment.later,`turn-${index+1}-nothing-from-later`))
        if(moment.step.outside?.length)checks.push(mustNot(turn.output,moment.step.outside,`turn-${index+1}-only-the-time-asked-about`))
      }
      if(!execution.ok||before.length<moments.length-1)return checks
      const answer=execution.perf.calls.filter(call=>call.purpose==="answer").at(-1)
      return [...checks,
        mustAll(execution.output,last.step.must,"uses-what-was-said"),
        ...(last.later.length?[mustNot(execution.output,last.later,"nothing-from-later")]:[]),
        ...(last.step.outside?.length?[mustNot(execution.output,last.step.outside,"only-the-time-asked-about")]:[]),
        // The last request repeats the ones before it, so the provider should have served most of it from its cache.
        {id:"thread-served-from-cache",group:"temporal" as const,soft:true,pass:!!answer&&answer.cachedTokens>=answer.inputTokens*0.5,detail:answer?`${answer.cachedTokens} of ${answer.inputTokens} input tokens were read from the provider's cache`:"no answer request"},
        ...citationChecks(execution.output,{citations:execution.citations,resolve,required:false}),
        ...chatFormat(execution.output,last.step.question),
        ...grounding(execution.output,[fixture.title,last.said,...thread.steps.map(step=>step.question)].join("\n")),
      ]
    },
    judge:execution=>({feature:"live-chat",
      task:`The meeting "${fixture.title}" is still being recorded and is transcribed live. This is the last question of a chat thread of ${moments.length} questions asked during the meeting. Earlier in the thread the user asked: ${moments.slice(0,-1).map(moment=>`"${moment.step.question}" at ${clock(moment.atMs)}`).join("; ")}; the assistant answered each from what had been said by then. The last question is asked ${clock(last.atMs)} into the meeting. The right answer uses what has been said since the earlier answers; repeating an earlier answer that is now out of date is an error, and so is using anything said after ${clock(last.atMs)}. Judge faithfulness against what had been said by then, and usefulness as an answer for someone in the meeting.\nQuestion: ${last.step.question}`,
      source:`--- Said before the question (${clock(last.atMs)}) ---\n${renderTruth(last.spoken.slice(-40))}${last.tailText?`\n\n--- Being spoken as the question is asked; only these words had been heard ---\n${last.speaking!.speaker}: ${last.tailText} …`:""}\n\n--- Not yet said at that moment (must not appear) ---\n${renderTruth(built.turns.filter(turn=>turn.endMs>last.atMs&&turn!==last.speaking).slice(0,25))||"(nothing)"}`,
      reference:last.step.reference,output:execution.output,citations:citedClaims(execution,resolve)}),
  }
})
