/** Chat during a recording, as a timeline simulation.
 *
 * A fixture is replayed as it would reach the server under a delivery model
 * (see DELIVERIES in lib/transcript.ts). At a set moment the note holds only
 * what had been delivered by then, is still `recording`, and the question goes
 * through the same streaming endpoint as any note chat.
 *
 * Each checkpoint is anchored to a line of the script (`after`) and asked at
 * one or more delays after that line ends. That answers three things:
 *  - does the answer use what had been said by then (when it was delivered)?
 *  - does it stay clear of anything said later (`later`)?
 *  - how long after a fact is spoken can the assistant actually answer with it? */
import type {ChatInput} from "@meeting-notes/contracts"
import {compile,fixtureById,fixtureNote} from "../lib/fixtures"
import {runChat} from "../lib/harness"
import {chatFormat,citationChecks,grounding,matches,mustAll,mustNot,ABSTAIN} from "../lib/checks"
import {DELIVERIES,clock,deliveredAt,findTurn,renderTruth,visibleAtMs} from "../lib/transcript"
import {citedClaims,completed,resolver,type Scenario} from "../lib/scenario"
import type {Pattern} from "../lib/types"

export interface LiveCheckpoint {
  id:string;fixture:string
  kind:"just-said"|"so-far"|"recency"|"not-yet"|"superseded"
  /** Verbatim substring of the script line the question follows. */
  after:string
  delaysSeconds:number[]
  question:string;reference:string
  /** Correct given everything said up to the anchor line. */
  must:Pattern[]
  /** Only said later in the meeting; appearing in the answer is a leak. */
  later?:Pattern[]
  deliveries?:Array<keyof typeof DELIVERIES>
  /** The question names a stretch of time ("in the last minute"): asked later than this many seconds after the
   * anchor line, that line is outside the stretch and a right answer need not contain it. */
  withinSeconds?:number
  fast?:boolean
}
export const CHECKPOINTS:LiveCheckpoint[]=[
  {id:"reconcile-hours",fixture:"discovery",kind:"just-said",fast:true,after:"we spend roughly twelve hours a week across the sites just reconciling",delaysSeconds:[5,60,150],
    question:"How much time did they just say they spend reconciling the schedule?",reference:"Roughly twelve hours a week across the three sites, reconciling the whiteboard against the spreadsheet.",must:["twelve hours|12 hours"]},
  {id:"pilot-not-yet",fixture:"discovery",kind:"not-yet",after:"Above that it becomes a board conversation and it won't happen this year.",delaysSeconds:[5],
    question:"What pilot terms have been proposed so far?",reference:"None yet. So far the call has covered Kestrel's current process, the failed Dockwise trial, the two hard requirements, timing, and a rough unapproved budget ceiling of about sixty thousand dollars a year.",must:[ABSTAIN],later:["4,?500|four thousand five hundred","ninety days|90 days|90.day","twenty.five users|25 users"]},
  {id:"just-agreed",fixture:"discovery",kind:"recency",fast:true,after:"I like that. Tacoma pilot, Reno offline test.",delaysSeconds:[5,150],withinSeconds:60,
    question:"What was just agreed in the last minute?",reference:"The shape of a pilot: run it at Tacoma, with a two-day offline test in the Reno yard (Joon's suggestion, which Marcus liked). The commercial terms Tessa proposed just before are ninety days, up to twenty-five users, a flat $4,500 credited against year one. Nothing is formally agreed.",must:["Tacoma[^\\n]{0,160}pilot|pilot[^\\n]{0,160}Tacoma","offline test|two.day|2.day"],later:["nineteenth|19th","on.?site"]},
  {id:"so-far",fixture:"discovery",kind:"so-far",after:"Those are the two. Everything else is nice to have.",delaysSeconds:[5,150],
    question:"Summarise the call so far.",reference:"Kestrel runs three warehouses with about 140 dock workers on a whiteboard plus a stale spreadsheet, causing double-booked dock doors and about twelve hours a week of reconciling. A Dockwise trial failed in about three weeks: no offline mode and no direct Stratus WMS integration. Those are their two hard requirements.",must:["offline","Stratus|warehouse system|integrat"],later:["sixty thousand|60,?000|60k","4,?500","Okta|single sign","Renata","nineteenth|19th"]},
  {id:"revenue-correction",fixture:"numbers",kind:"just-said",fast:true,after:"Four point eight two million dollars, against a plan of five point one.",delaysSeconds:[5,150],
    question:"What did he say Q3 revenue was?",reference:"$4.82 million, against a plan of $5.1 million. He first said 4.28 and corrected himself.",must:["4\\.82|four point eight two"],later:["19\\.6","71\\.4"]},
  {id:"increase-proposed",fixture:"numbers",kind:"superseded",after:"I proposed eight percent.",delaysSeconds:[5],deliveries:["stream"],
    question:"What renewal price increase are we going with?",reference:"Not decided yet at this point. Greta proposed eight percent last month; Yusuf has only just raised the topic.",must:["eight|8 ?%|8 percent","propos|not (?:yet )?decided|no decision|hasn't|has not|undecided|yet"],later:["six percent|6 ?%|6 percent","January"]},
  {id:"increase-settled",fixture:"numbers",kind:"superseded",fast:true,after:"I want six percent, on renewals only, starting January first.",delaysSeconds:[5,150],
    question:"What renewal price increase are we going with?",reference:"Six percent, on renewals only, from January 1, with nothing changing mid-contract. Greta had proposed eight; Yusuf was not comfortable with eight after losing Pike & Rowe on price.",must:["six percent|6 ?%|6 percent"],later:["twenty percent|20 ?%|discount cap","Orrin"]},
  {id:"eligibility-pencilled",fixture:"planning",kind:"superseded",fast:true,after:"Pencilled.",delaysSeconds:[5],
    question:"Is the insurance eligibility pilot in the Q3 plan right now?",reference:"Not as things stand: it has just been pencilled for Q4, with reporting taking its place, because nobody present could confirm the vendor budget. Rosalind will confirm with Tobias afterwards.",must:["Q4|pencil|defer"],later:["ninety.day|90.day","thirty thousand|30,?000|30k","October (?:20|twentieth)"]},
  {id:"eligibility-reversed",fixture:"planning",kind:"superseded",fast:true,after:"Eligibility pilot is in for Q3. Reporting goes to Q4. Does anyone object?",delaysSeconds:[5,150],
    question:"Is the insurance eligibility pilot in the Q3 plan right now?",reference:"Yes. It was pencilled for Q4 a few minutes earlier, then reversed once Tobias confirmed the $30,000 vendor ceiling: the pilot is in for Q3, conditional on a 90-day termination clause, and reporting moves to Q4.",must:["\\bin\\b[^.\\n]{0,40}Q3|is in|yes|committed|back in|revers"],later:["October (?:20|twentieth)","Alder Street","September (?:5|fifth)"]},
  {id:"freeze",fixture:"planning",kind:"just-said",after:"The data warehouse migration has a freeze from August eleventh to August fifteenth.",delaysSeconds:[5,150],
    question:"When did Hamid say the warehouse freeze is?",reference:"August 11 to August 15: no schema changes at all that week.",must:["August (?:11|eleventh)|Aug(?:ust)? 11","15|fifteenth"]},
  {id:"catch-up",fixture:"planning",kind:"recency",after:"Right. Three of the four fit, in their smaller versions. All four don't.",delaysSeconds:[5,150],
    question:"I stepped away. What did I miss in the last five minutes?",reference:"Arjun's capacity: nine engineers, two on parental leave from August 4, 78 engineer-weeks, of which he wants to plan 58 with a 25% buffer. Estimates: self-reschedule 9, reporting 18, eligibility 30 or 14 for a pilot, multi-location 44 or 22 for phase one. The three smaller versions total 45 and fit; adding reporting makes 63, which does not.",must:["78|seventy.eight","58|fifty.eight"],later:["Claimwell","Verita","August (?:11|eleventh)"]},
  {id:"decisions-so-far",fixture:"planning",kind:"not-yet",after:"It's loud, it isn't wide.",delaysSeconds:[5],
    question:"What has been decided so far?",reference:"Nothing about the Q3 plan yet. So far: the Q2 look-back, a lesson that every item must list its external dependencies, the four candidates, and evidence from support, data and design. Meadowbrook's renewal risk has been raised.",must:["no(?:thing| decisions?)|not (?:yet )?(?:been )?decided|haven't decided|no final|so far"],later:["Claimwell","fifty.eight|\\b58\\b","August (?:29|twenty.ninth)","no (?:self.?)?cancel"]},
]

export const liveChatScenarios=(now:Date):Scenario[]=>{
  const scenarios:Scenario[]=[]
  for(const checkpoint of CHECKPOINTS){
    const fixture=fixtureById(checkpoint.fixture)
    for(const deliveryId of checkpoint.deliveries??["stream","ios"])for(const delay of checkpoint.delaysSeconds){
      const delivery=DELIVERIES[deliveryId]!,built=compile(fixture,delivery.capture),anchor=findTurn(built,checkpoint.after)
      const atMs=anchor.endMs+delay*1000,delivered=deliveredAt(built,delivery,atMs)
      const anchorVisible=visibleAtMs(anchor,delivery),hasAnchor=anchorVisible!==null&&anchorVisible<=atMs
      const note=fixtureNote(fixture,{now,level:"none",capture:delivery.capture,segments:delivered.segments,recording:true}),resolve=resolver([note])
      const input:ChatInput={message:checkpoint.question,history:[],includeSources:true,timeZone:"UTC"}
      const spoken=built.turns.filter(turn=>turn.endMs<=atMs),unspoken=built.turns.filter(turn=>turn.endMs>atMs)
      // A longer delay lets more of the meeting happen; only what is still unsaid
      // at the moment of the question can be a leak.
      const said=spoken.map(turn=>turn.text).join("\n"),later=(checkpoint.later??[]).filter(source=>!matches(said,source))
      scenarios.push({
        feature:"live-chat",id:`live-chat/${fixture.id}/${checkpoint.id}@${deliveryId}+${delay}s`,fixture:fixture.id,variant:`${checkpoint.kind}/${deliveryId}/+${delay}s`,
        fast:!!checkpoint.fast&&(delay===checkpoint.delaysSeconds[0]||delay===150)&&deliveryId!=="desktop",
        input:{question:checkpoint.question,atMs,delivery:deliveryId,reference:checkpoint.reference},
        context:{segmentsAvailable:delivered.segments.length,segmentsTotal:built.segments.length,transcriptCharacters:delivered.segments.reduce((sum,segment)=>sum+segment.text.length,0),deliveredUntilMs:delivered.deliveredUntilMs},
        execute:target=>runChat(target,[note],note.id,input),
        check:execution=>!execution.ok?[completed(execution)]:[
          completed(execution),
          // The fact can only be expected once the server has the line that states it.
          {...mustAll(execution.output,checkpoint.must,"uses-what-was-said"),...(hasAnchor&&!(checkpoint.withinSeconds!==undefined&&delay>checkpoint.withinSeconds)?{}:{soft:true})},
          {id:"anchor-delivered",group:"temporal",soft:true,pass:hasAnchor,detail:hasAnchor?"the line was on the server when the question was asked":`the line reaches the server ${anchorVisible===null?"only after Stop":`${Math.round((anchorVisible-anchor.endMs)/1000)} s after it is spoken`}; asked after ${delay} s`},
          ...(later.length?[mustNot(execution.output,later,"nothing-from-later")]:[]),
          ...(hasAnchor?[]:[{...mustAll(execution.output,[ABSTAIN+"|so far|yet|up to (?:this|that) point|as of"],"admits-gap","temporal"),soft:true}]),
          ...citationChecks(execution.output,{citations:execution.citations,resolve,required:false}),
          ...chatFormat(execution.output,checkpoint.question),
          ...grounding(execution.output,[fixture.title,spoken.map(turn=>turn.text).join("\n"),checkpoint.question].join("\n")),
        ],
        judge:execution=>({feature:"live-chat",
          task:`The meeting "${fixture.title}" is still being recorded. The user asks a question ${clock(atMs)} into it. The assistant can only know what the server had received by then: under the "${deliveryId}" delivery model that is the transcript up to ${clock(delivered.deliveredUntilMs)} (${delivered.segments.length} segments). ${hasAnchor?"The line the question is about had been delivered.":"The line the question is about had NOT been delivered yet, so the assistant could not have known it; the right response says the transcript does not have it yet and offers what it does have."} Using anything said after ${clock(atMs)} would be an error (it cannot happen unless the assistant invents it). Speaker names were not visible to the assistant.\nQuestion: ${checkpoint.question}`,
          source:`--- Said before the question (${clock(atMs)}) ---\n${renderTruth(spoken)||"(nothing yet)"}\n\n--- Not yet said at that moment (must not appear) ---\n${renderTruth(unspoken.slice(0,25))||"(nothing)"}`,
          reference:`If the assistant had everything said so far: ${checkpoint.reference}`,output:execution.output,citations:citedClaims(execution,resolve)}),
      })
    }
  }
  // Desktop today: nothing from the current recording is on the server.
  for(const checkpoint of CHECKPOINTS.filter(item=>item.id==="reconcile-hours"||item.id==="eligibility-reversed")){
    const fixture=fixtureById(checkpoint.fixture),delivery=DELIVERIES.desktop!,built=compile(fixture,"desktop"),anchor=findTurn(built,checkpoint.after),atMs=anchor.endMs+150_000
    const note=fixtureNote(fixture,{now,level:"none",capture:"desktop",segments:[],recording:true}),input:ChatInput={message:checkpoint.question,history:[],includeSources:true,timeZone:"UTC"}
    const spoken=built.turns.filter(turn=>turn.endMs<=atMs)
    scenarios.push({
      feature:"live-chat",id:`live-chat/${fixture.id}/${checkpoint.id}@desktop+150s`,fixture:fixture.id,variant:`${checkpoint.kind}/desktop/+150s`,fast:checkpoint.id==="reconcile-hours",
      input:{question:checkpoint.question,atMs,delivery:"desktop",reference:"The transcript is not available yet: the desktop app uploads audio only after Stop."},
      context:{segmentsAvailable:0,segmentsTotal:built.segments.length,transcriptCharacters:0,deliveredUntilMs:0},
      execute:target=>runChat(target,[note],note.id,input),
      check:execution=>!execution.ok?[completed(execution)]:[
        completed(execution),
        {...mustAll(execution.output,checkpoint.must,"uses-what-was-said"),soft:true},
        {id:"anchor-delivered",group:"temporal",soft:true,pass:false,detail:delivery.description},
        mustAll(execution.output,[ABSTAIN+"|empty"],"admits-gap","temporal"),
        ...chatFormat(execution.output,checkpoint.question),
        ...grounding(execution.output,[fixture.title,checkpoint.question].join("\n")),
      ],
      judge:execution=>({feature:"live-chat",task:`The meeting "${fixture.title}" is still being recorded on the desktop app, which uploads audio only after Stop. The server therefore has no transcript at all for this recording, and no typed notes. The user asks ${clock(atMs)} into the meeting. The only faithful response says there is no transcript yet; anything specific about the meeting is invented. Score coverage 3 if it says so plainly.\nQuestion: ${checkpoint.question}`,
        source:`(The assistant had no transcript.) For your reference only, this had been said:\n${renderTruth(spoken.slice(-12))}`,reference:"There is no transcript for this meeting yet.",output:execution.output}),
    })
  }
  return scenarios
}
