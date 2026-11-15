/**
 * Retrieval-stress scenarios for workspace chat over the large fictional
 * workspace in `corpus.ts`. Two sets:
 *
 *   workspace-chat/large/<id>     tuned: prompts and tools may be iterated against these
 *   workspace-chat/heldout/<id>   held out: run only to report, never tuned against
 *
 * Select with `--only large/` or `--only heldout/`. Each case names the meetings
 * that hold the answer (`relevant`), which the judge receives as ground truth.
 *
 * Set EVAL_WORKSPACE_REPOSITORY=postgres (with DATABASE_URL pointing at a
 * disposable `mn_test_*` database) to run the same scenarios through the
 * PostgreSQL search path instead of the in-memory one; see `harness.ts`.
 */
import type {ChatInput} from "@meeting-notes/contracts"
import {chatFormat,citationChecks,grounding,mustAll,mustNot,ABSTAIN} from "../lib/checks"
import {citedClaims,completed,resolver,SPEAKER_NOTE,type Scenario} from "../lib/scenario"
import type {ChatTurn,Check,Pattern} from "../lib/types"
import type {Execution} from "../lib/harness"
import {coverageNote} from "../../src/ai/workspace-tools"
import {day,largeNoteId,largeTruth,largeWorkspace,type Day,type LargeMeeting} from "./corpus"
import {runWorkspaceChat} from "./harness"

type Variant="needle"|"transcript-only"|"superseded"|"date-scoped"|"follow-up"|"since-event"|"disambiguation"|"absent"|"most-recent"|"two-hop"|"aggregate"|"person"|"recent-todos"|"count"
interface Context {d:(daysAgo:number)=>Day;
  /** Meetings whose date falls in the Monday-to-Sunday week `weeksAgo` weeks before this one. */
  week:(weeksAgo:number)=>LargeMeeting[]}
interface Case {
  id:string;set:"large"|"heldout";variant:Variant;fast?:boolean
  question:(context:Context)=>string
  history?:(context:Context)=>ChatTurn[]
  reference:(context:Context)=>string
  must:(context:Context)=>Pattern[]
  forbid?:(context:Context)=>Pattern[]
  /** Keys of the meetings that hold the answer. */
  relevant:(context:Context)=>string[]
}
const weekdayOnly=(meetings:LargeMeeting[],context:Context)=>meetings.filter(meeting=>!["Saturday","Sunday"].includes(context.d(meeting.daysAgo).weekday))
const recap=(meetings:LargeMeeting[],context:Context)=>meetings.map(meeting=>`- ${meeting.title} (${context.d(meeting.daysAgo).spoken}): ${meeting.headline.text}`).join("\n")

const CASES:Case[]=[
  // ------------------------------------------------------------------ tuned
  {id:"harrowgate-discount",set:"large",variant:"needle",relevant:()=>["harrowgate-discovery","harrowgate-pricing"],
    question:()=>"What discount did Harrowgate originally ask for, and what did we end up agreeing?",
    reference:()=>"Ilse Brandt asked for fifteen percent off in exchange for a two-year commitment (discovery call). In the pricing call we agreed ten percent off for a two-year term, with renewal increases capped at five percent.",
    must:()=>["fifteen|15 ?%","ten percent|10 ?%","five percent|5 ?%"]},
  {id:"rescheduling-hours",set:"large",variant:"transcript-only",fast:true,relevant:()=>["harrowgate-discovery"],
    question:()=>"How many hours a week did Ilse say her team spends on manual rescheduling?",
    reference:()=>"About twenty-two hours a week across the team (Harrowgate discovery call). The summary only says significant time is lost; the figure is in the transcript.",
    must:()=>["twenty.two|\\b22\\b"]},
  {id:"export-launch-date",set:"large",variant:"superseded",fast:true,relevant:()=>["sync-7","sync-10","sync-3"],
    question:()=>"When is Export launching?",
    reference:c=>`${c.d(-18).spoken}, with the audit log included. It was moved from ${c.d(10).spoken} in the weekly product sync on ${c.d(24).spoken} because Pinecrest Health will not roll out without an audit log (before that it had slipped from ${c.d(31).spoken}). The go/no-go meeting is on ${c.d(-11).spoken}.`,
    must:c=>[c.d(-18).pattern],
    forbid:c=>[`(?:launch(?:es|ing)?|ships?|go(?:es|ing)? live|is scheduled)(?: is| date is| target is| will be)? (?:on |for )?(?:${c.d(10).pattern}|${c.d(31).pattern})`]},
  {id:"pricing-last-30-days",set:"large",variant:"date-scoped",fast:true,relevant:()=>["pricing-3"],
    question:()=>"What did we decide about pricing in the last 30 days?",
    reference:c=>`In the pricing working session on ${c.d(26).spoken}: accounts under fifty seats are grandfathered at $44 per seat for twelve months, and the annual prepay discount is cut from ten percent to eight percent. Whether Parquet export becomes a paid add-on is still open. (The eight percent increase itself was decided earlier, outside this period.)`,
    must:()=>["grandfather|(?:under|fewer than|below|less than) (?:fifty|50)|small(?:er)? accounts","prepay"],forbid:()=>["(?:decided|agreed)[^.\\n]{0,40}(?:twelve percent|12 ?%) (?:price )?increase"]},
  {id:"last-week",set:"large",variant:"date-scoped",relevant:c=>c.week(1).map(meeting=>meeting.key),
    question:()=>"What happened in my meetings last week?",
    reference:c=>`Last week (${c.d(mondayAgo(c)+7).spoken} to ${c.d(mondayAgo(c)+1).spoken}):\n${recap(c.week(1),c)}`,
    must:c=>weekdayOnly(c.week(1),c).map(meeting=>meeting.headline.must)},
  {id:"week-before",set:"large",variant:"follow-up",fast:true,relevant:c=>c.week(2).map(meeting=>meeting.key),
    question:()=>"And what about the week before?",
    history:c=>[{role:"user",content:"What happened in my meetings last week?"},{role:"assistant",content:`Last week you had ${c.week(1).length} meetings:\n${recap(c.week(1),c)}`}],
    reference:c=>`The week before (${c.d(mondayAgo(c)+14).spoken} to ${c.d(mondayAgo(c)+8).spoken}):\n${recap(c.week(2),c)}`,
    must:c=>weekdayOnly(c.week(2),c).map(meeting=>meeting.headline.must)},
  {id:"export-since-offsite",set:"large",variant:"since-event",relevant:()=>["offsite-2","sync-3","sync-7","sync-10"],
    question:()=>"What has changed on the Export launch date since the leadership offsite?",
    reference:c=>`At the offsite (${c.d(67).spoken}) the committed date was ${c.d(31).spoken}. It then slipped to ${c.d(10).spoken} because the permissions (row-level access) work was larger than planned (${c.d(59).spoken} sync), and moved again to ${c.d(-18).spoken} to include an audit log for Pinecrest Health (${c.d(24).spoken} sync). A go/no-go meeting is set for ${c.d(-11).spoken}.`,
    must:c=>[c.d(10).pattern,c.d(-18).pattern,"audit log","permission"]},
  {id:"sync-on-date",set:"large",variant:"disambiguation",fast:true,relevant:()=>["sync-8"],
    question:c=>`In the weekly product sync on ${c.d(17).spoken}, what did we decide?`,
    reference:()=>"To cut team templates from onboarding version one, after all three customers who tested the checklist got stuck on them. (Status from the same sync: Okta single sign-on is ready for beta; Entra slipped two weeks.)",
    must:()=>["team templates"],forbid:()=>["CSV and Parquet","release freeze"]},
  {id:"lisbon-office",set:"large",variant:"absent",fast:true,relevant:()=>[],
    question:()=>"What did we decide about opening a Lisbon office?",
    reference:()=>"Nothing in the meetings mentions a Lisbon office.",
    must:()=>[ABSTAIN],forbid:()=>["(?:decided|agreed) to (?:open|lease|rent)","Lisbon office (?:will|is going to)"]},
  {id:"bastien-advice",set:"large",variant:"transcript-only",relevant:()=>["coffee-bastien"],
    question:()=>"What book did Bastien recommend, and what else did he advise?",
    reference:()=>"He recommended a book called The Quiet Roadmap, and advised asking for the staff title explicitly rather than waiting to be offered it. He also asked for a call after calibration.",
    must:()=>["Quiet Roadmap","staff title"]},
  {id:"last-spoke-ilse",set:"large",variant:"most-recent",fast:true,relevant:()=>["harrowgate-security"],
    question:()=>"When did I last speak with Ilse Brandt, and what did she want?",
    reference:c=>`On ${c.d(12).spoken}, when she joined the last ten minutes of the Harrowgate security review. She wanted her single sign-on beta invite and confirmation that the service credit was approved; you said the invite goes out that week and that you would confirm the one-month credit in writing by Friday.`,
    must:c=>[c.d(12).pattern,"invite|single sign.on|SSO","credit"],forbid:c=>[`last (?:spoke|talked|met|spoken)[^.\\n]{0,80}(?:${c.d(32).pattern})`]},
  {id:"price-increase-objection",set:"large",variant:"two-hop",fast:true,relevant:()=>["pricing-2"],
    question:()=>"In the meeting where we settled on the 8% price increase, who objected and why?",
    reference:()=>"Aditi objected (pricing committee follow-up). She wanted accounts under fifty seats exempt and expected to lose about thirty small accounts, which mattered to her team's number. The eight percent was decided anyway, with small accounts to be revisited the following month.",
    must:()=>["Aditi","thirty|\\b30\\b|small accounts|under fifty|under 50"]},
  {id:"devraj-asks",set:"large",variant:"person",relevant:()=>["harrowgate-discovery","harrowgate-pricing","harrowgate-security"],
    question:()=>"What has Devraj asked us for across our calls?",
    reference:()=>"Discovery: single sign-on with Microsoft Entra and data hosted in the European Union. Pricing call: SCIM provisioning and audit logs for administrator actions, with timing in writing. Security review: the latest penetration test report, SCIM by the end of the first quarter (or he cannot approve the expansion), and IP allow-listing for the admin console.",
    must:()=>["SCIM","audit log","penetration|pen.?test","allow.?list"]},
  {id:"recent-todos",set:"large",variant:"recent-todos",fast:true,relevant:()=>["yearend-kickoff","sync-10","1on1-6","allhands-prep","harrowgate-security"],
    question:()=>"List my recent to-dos",
    reference:c=>`Most recent first. Year-end planning kickoff (${c.d(1).spoken}): submit the headcount request by ${c.d(-6).spoken} in Lowell's template; roadmap draft to Jorge by ${c.d(-2).spoken}. Weekly product sync (${c.d(3).spoken}): send Harrowgate the single sign-on beta invite by Friday. 1:1 with Tamsin (${c.d(5).spoken}): write the wins document by ${c.d(-9).spoken}; move onboarding work to the new designer. All-hands prep (${c.d(6).spoken}): ask Tamsin whether the price increase can be mentioned. Harrowgate security review (${c.d(12).spoken}): confirm the one-month credit in writing by Friday. It should say which period it covered.`,
    must:()=>["headcount","invite","wins"]},
  {id:"sso-decisions",set:"large",variant:"aggregate",relevant:()=>["sso-kickoff","sync-3","sync-6","offsite-2"],
    question:()=>"What decisions have we made about single sign-on so far?",
    reference:()=>"Buy a vendor library for the protocol (about $9,000 a year) and build the settings screens in-house (SSO kickoff). Support Okta and Microsoft Entra first, Google Workspace later. Single sign-on is priority two after Export (offsite). Harrowgate is the first beta customer.",
    must:()=>["Okta","library","Harrowgate"]},
  {id:"harrowgate-three-year",set:"large",variant:"absent",relevant:()=>["harrowgate-pricing"],
    question:()=>"Did Harrowgate agree to a three-year contract?",
    reference:()=>"No. Harrowgate agreed a two-year term (ten percent off, renewal increases capped at five percent). A three-year minimum came up only with the vendor Mapleton Analytics, which we passed on.",
    must:()=>["two.year|two years|2.year"],forbid:()=>["Harrowgate (?:agreed|signed|committed) to (?:a )?three"]},

  // --------------------------------------------------------------- held out
  {id:"fennimore-fee",set:"heldout",variant:"needle",relevant:()=>["partner-fennimore"],
    question:()=>"What referral fee did Fennimore Consulting ask for, and where did we leave it?",
    reference:()=>"Fifteen percent of first-year revenue. We countered with ten percent and nothing was agreed; both sides were to think about it.",
    must:()=>["fifteen|15 ?%","ten|10 ?%"]},
  {id:"mapleton-pass",set:"heldout",variant:"needle",relevant:()=>["vendor-mapleton"],
    question:()=>"Why did we pass on Mapleton Analytics?",
    reference:()=>"Two reasons: no European data residency (data is processed in Virginia only), which rules it out for customers like Harrowgate, and a three-year minimum contract. They had quoted $36,000 a year.",
    must:()=>["residency|European|\\bEU\\b|Virginia","three.year|3.year|three years"]},
  {id:"pricing-decisions-in-order",set:"heldout",variant:"aggregate",relevant:()=>["pricing-1","pricing-2","pricing-3"],
    question:()=>"List every pricing decision we have made since the first pricing committee meeting, in order.",
    reference:c=>`First pricing committee (${c.d(80).spoken}): no decision; a twelve percent increase was only proposed. Follow-up (${c.d(52).spoken}): eight percent increase for existing customers from January 1; list price from $44 to $49 per seat. Working session (${c.d(26).spoken}): accounts under fifty seats grandfathered at $44 for twelve months; annual prepay discount cut from ten to eight percent.`,
    must:()=>["eight percent|8 ?%","\\$?49","grandfather|(?:under|fewer than|below|less than) (?:fifty|50)|small(?:er)? accounts","prepay"],forbid:()=>["(?:decided|agreed) (?:on |to )?(?:a )?(?:twelve percent|12 ?%)"]},
  {id:"customer-calls-35-days",set:"heldout",variant:"date-scoped",relevant:()=>["harrowgate-renewal","pinecrest-pilot","harrowgate-security"],
    question:()=>"Which customer calls did I have in the last 35 days, and what did each customer want?",
    reference:c=>`Harrowgate renewal check-in (${c.d(32).spoken}): a service credit for the outage; will add twelve dispatchers if single sign-on lands. Pinecrest Health pilot review (${c.d(20).spoken}): roll out to all nine clinics once Export has the audit log; a formal quote before their board meets. Harrowgate security review (${c.d(12).spoken}): penetration test report, SCIM by the end of the first quarter, IP allow-listing; Ilse wanted the beta invite and the credit confirmed.`,
    must:()=>["credit","Pinecrest","penetration|pen.?test|SCIM"],forbid:()=>["Fennimore","Mapleton"]},
  {id:"outage-actions-since-review",set:"heldout",variant:"since-event",relevant:()=>["incident-followup","sync-9","support-escalations"],
    question:()=>"What has happened with the sync outage follow-up actions since the incident review?",
    reference:c=>`Incident follow-up (${c.d(34).spoken}): postmortem published, idempotency keys in production, and a decision to run a regional failover test before year end. Weekly sync (${c.d(10).spoken}): four of six postmortem actions done; the failover test is booked for ${c.d(-4).spoken} and alert tuning was due that week. Support review (${c.d(8).spoken}): one customer still saw duplicate stops, from old data, cleaned up by hand.`,
    must:()=>["(?:four|4) (?:out )?of (?:the )?(?:six|6)|4/6","failover","idempotency"]},
  {id:"designer-choice",set:"heldout",variant:"disambiguation",relevant:()=>["debrief-marguerite","debrief-oluwaseun"],
    question:()=>"Which of the two senior designer candidates did we choose, and why?",
    reference:()=>"Oluwaseun, because the team needs someone who can own the design system from day one; his systems thinking was the strongest. Marguerite was excellent at research and facilitation but her portfolio was mostly consumer apps; the plan is to keep her warm for the next role.",
    must:()=>["Oluwaseun","design system"],forbid:()=>["offer (?:goes|went|was made|is going) to Marguerite"]},
  {id:"calloway-discount",set:"heldout",variant:"absent",relevant:()=>["pipeline-1","pipeline-2"],
    question:()=>"What discount did we agree with Calloway Freight?",
    reference:()=>"None is recorded. Calloway Freight is a prospect (about $180,000 a year) waiting on single sign-on and talking to a competitor; Aditi was to offer them the Okta beta. No discount was discussed.",
    must:()=>[ABSTAIN],forbid:()=>["Calloway[^.\\n]{0,60}(?:agreed|accepted)[^.\\n]{0,30}(?:discount|percent|%)","(?:ten|fifteen|10|15) ?(?:percent|%)[^.\\n]{0,40}Calloway"]},
  {id:"harbor-name",set:"heldout",variant:"transcript-only",relevant:()=>["naming-export"],
    question:()=>"Who suggested calling the export feature Harbor, and why was it rejected?",
    reference:()=>"Wren suggested Harbor. Odette rejected it because the name is taken by a container registry and legal would not like it. The feature is simply called Export.",
    must:()=>["Wren","taken|registry|legal"]},
  {id:"scanner-call",set:"heldout",variant:"transcript-only",relevant:()=>["untitled-scanner"],
    question:()=>"What did the barcode scanner vendor tell me about platform support, and what did they offer?",
    reference:c=>`Tallis Devices (an untitled call on ${c.d(9).spoken}): their software kit supports Android only, with iOS not until late next year. A pilot costs $1,200 per device, and they offered to send two sample units the following week at no charge.`,
    must:()=>["Android","two sample|2 sample|sample units"]},
  {id:"last-duplicate-stops",set:"heldout",variant:"most-recent",relevant:()=>["support-escalations"],
    question:()=>"When was the last time duplicate stops came up, and what was the status?",
    reference:c=>`In the support escalation review on ${c.d(8).spoken}: one customer was still seeing duplicate stops, but it was old data from before the idempotency fix, and Mirela cleaned it up by hand.`,
    must:c=>[c.d(8).pattern,"old data|before the (?:idempotency )?fix|cleaned"]},
  {id:"incident-first-signal",set:"heldout",variant:"two-hop",relevant:()=>["incident-review"],
    question:()=>"In the incident review, what was the first thing the on-call engineer noticed, and at what time?",
    reference:()=>"Kenji, who was on call, first saw the consumer lag alarm on the eu-west queue at 6:52; nothing else was red yet.",
    must:()=>["consumer lag|lag alarm","6:52|six fifty.two|06:52"]},
  {id:"harrowgate-since-then",set:"heldout",variant:"follow-up",relevant:()=>["harrowgate-renewal","harrowgate-security","budget-checkin"],
    question:()=>"Has anything changed for them since then?",
    history:()=>[{role:"user",content:"What did we agree with Harrowgate on price?"},{role:"assistant",content:"In the pricing call, Harrowgate agreed to a ten percent discount for a two-year term, with renewal price increases capped at five percent."}],
    reference:c=>`Yes. The sync outage cost them half a day at the Rotterdam depot and Ilse asked for a service credit (${c.d(32).spoken}); a one-month credit was offered, then approved by Lowell, with written confirmation promised. Ilse will add twelve dispatchers if single sign-on lands; they are the first single sign-on beta customer. Devraj needs SCIM by the end of the first quarter to approve the expansion. The price terms themselves have not changed.`,
    must:()=>["credit","SCIM|single sign.on|SSO|dispatchers"]},
  {id:"tamsin-worries",set:"heldout",variant:"person",relevant:()=>["1on1-6","1on1-5","1on1-4"],
    question:()=>"What is Tamsin worried about lately?",
    reference:c=>`In the most recent 1:1 (${c.d(5).spoken}) she listed, in order: SCIM has no owner and Harrowgate needs it by the end of the first quarter; Wren is overloaded across two projects; and the year-end planning deadline is closer than people think. Earlier: launching Export without the audit log would have been a mistake (${c.d(19).spoken}); the Harrowgate renewal and Jorge's burnout (${c.d(33).spoken}).`,
    must:()=>["SCIM","Wren"]},
  {id:"sync-count",set:"heldout",variant:"count",relevant:()=>["sync-10","sync-9","sync-8","sync-7","sync-6","sync-5","sync-4","sync-3"],
    question:()=>"How many weekly product syncs have I had in the last 60 days, and on which dates?",
    reference:c=>`Eight: ${[59,45,38,31,24,17,10,3].map(n=>c.d(n).spoken).join(", ")}.`,
    must:c=>["\\b8\\b|eight",c.d(3).listed,c.d(59).listed]},
  {id:"open-questions-syncs",set:"heldout",variant:"aggregate",relevant:()=>["sync-10","sync-9","sync-8"],
    question:()=>"What questions are still open from the last three weekly product syncs?",
    reference:()=>"Whether to charge extra for Parquet export (raised in the second most recent sync; Noor is taking it to the pricing group), and who owns SCIM provisioning (still unresolved in the most recent sync). The sync before those recorded no open question.",
    must:()=>["Parquet","SCIM"]},
  {id:"pinecrest-status",set:"heldout",variant:"aggregate",relevant:()=>["pinecrest-intro","pinecrest-pilot","pipeline-2","sync-7"],
    question:()=>"Give me a status summary of Pinecrest Health.",
    reference:c=>`Nine clinics with home-visit nurses. A sixty-day pilot at three clinics cut missed visits from eleven percent to seven percent. They want to roll out to all nine, but only once Export has the audit log (shipping ${c.d(-18).spoken}); their board decides on ${c.d(-25).spoken}. Quoted $49 per seat for about 140 seats, formal quote owed by Aditi. Stage three in the pipeline.`,
    must:()=>["eleven|11 ?%","seven|7 ?%","audit log"]},
]
/** The answer must close with the coverage note the server derived from the
 * lookups that actually ran: recomputed here from the stream's tool events, so
 * a note that claims a search or a read that did not happen fails. */
export function coverageCheck(execution:Pick<Execution,"output"|"tools">):Check {
  // Typographic and plain quotation marks are the same note.
  const normal=(text:string)=>text.replace(/\[\[S\d+\]\]/g,"").replace(/[*_`]/g,"").replace(/[\u201c\u201d\u201e]/g,'"').replace(/[\u2018\u2019]/g,"'").replace(/\s+/g," ").trim()
  const expected=normal(coverageNote(execution.tools)),at=execution.output.lastIndexOf("Coverage note")
  if(!expected)return {id:"coverage-note-true",group:"format",pass:at<0,...(at<0?{}:{detail:"a coverage note was written although nothing was looked up"})}
  const actual=at<0?"":normal(execution.output.slice(at)).replace(/^Coverage note:?\s*/,"")
  return {id:"coverage-note-true",group:"format",pass:actual===expected,...(actual===expected?{}:{detail:at<0?"no closing coverage note":`says "${actual.slice(0,160)}" but the lookups were "${expected.slice(0,160)}"`})}
}
/** Days ago of Monday of the current week (UTC). */
const mondayAgo=(context:Context)=>["Monday","Tuesday","Wednesday","Thursday","Friday","Saturday","Sunday"].indexOf(context.d(0).weekday)

export const largeWorkspaceScenarios=(now:Date):Scenario[]=>{
  const workspace=largeWorkspace(now),resolve=resolver(workspace.notes)
  const d=(daysAgo:number)=>day(now,daysAgo)
  const context:Context={d,week:weeksAgo=>{const monday=mondayAgo({d,week:()=>[]})+7*weeksAgo;return workspace.meetings.filter(meeting=>meeting.daysAgo<=monday&&meeting.daysAgo>monday-7).sort((a,b)=>a.daysAgo-b.daysAgo)}}
  const catalogue=workspace.meetings.map(meeting=>`- ${d(meeting.daysAgo).iso} ${meeting.title}${meeting.summary?"":meeting.script.trim()?" (no summary)":" (typed notes only)"}`).sort().reverse().join("\n")
  const everything=[...workspace.notes.map(note=>`${note.title}\n${note.date}\n${note.body}\n${note.enhanced}\n${note.transcriptSegments.map(segment=>segment.text).join("\n")}`),...workspace.meetings.flatMap(meeting=>Object.keys(meeting.people)),JSON.stringify([...workspace.contexts.values()]),...Array.from({length:200},(_,index)=>d(index-30).spoken)].join("\n")
  const ids=new Set<string>()
  return CASES.map((item):Scenario=>{
    if(ids.has(item.id))throw new Error(`Duplicate large-workspace case ${item.id}`)
    ids.add(item.id)
    const question=item.question(context),history=item.history?.(context),reference=item.reference(context),must=item.must(context),forbid=item.forbid?.(context)??[]
    for(const source of [...must,...forbid])new RegExp(source,"i")
    const input:ChatInput={message:question,history:history??[],includeSources:true,timeZone:"UTC"}
    const relevant=item.relevant(context).map(key=>{const meeting=workspace.byKey.get(key);if(!meeting)throw new Error(`${item.id}: unknown meeting ${key}`);return meeting})
    const source=relevant.length?relevant.map(meeting=>`=== ${meeting.title} (${d(meeting.daysAgo).iso}) ===\n${largeTruth(meeting)}`).join("\n\n"):"(no meeting contains the answer)"
    return {
      feature:"workspace-chat",id:`workspace-chat/${item.set}/${item.id}`,fixture:"large-workspace",variant:`${item.set}:${item.variant}`,fast:!!item.fast,
      input:{question,...(history?{history}:{}),reference},
      execute:target=>runWorkspaceChat(target,workspace,input),
      check:execution=>!execution.ok?[completed(execution)]:[
        completed(execution),
        mustAll(execution.output,must,"answer-correct"),
        ...(forbid.length?[mustNot(execution.output,forbid,"answer-not-wrong")]:[]),
        ...citationChecks(execution.output,{citations:execution.citations,resolve,expectedNoteIds:relevant.map(meeting=>largeNoteId(meeting.key)),required:relevant.length>0}),
        ...chatFormat(execution.output,question),
        coverageCheck(execution),
        ...grounding(execution.output,[everything,question,reference,...(history??[]).map(turn=>turn.content)].join("\n")),
      ],
      judge:execution=>({feature:"workspace-chat",
        task:`Answer a question across all of the user's meetings (${item.variant}). Today is ${d(0).iso} (${d(0).weekday}). The user is Noor Haddad. The assistant had search and read tools over these ${workspace.meetings.length} meetings and had to find the right ones itself:\n${catalogue}\n${history?`Earlier turns:\n${history.map(turn=>`${turn.role}: ${turn.content}`).join("\n")}\n`:""}Question: ${question}\nThe source below holds only the meetings that contain the answer (typed notes, generated summary and transcript with real speaker names). Mentioning other meetings is fine if accurate and relevant; you cannot verify those details, so judge faithfulness on what you can check. ${SPEAKER_NOTE}`,
        source,reference,output:execution.output,citations:citedClaims(execution,resolve)}),
    }
  })
}
