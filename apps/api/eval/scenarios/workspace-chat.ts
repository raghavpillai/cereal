/** Chat across the whole fictional workspace: all fixture meetings (with typed
 * notes and a reference summary, as past meetings usually have) plus a few
 * unrelated notes. Dates are relative to the run date. */
import type {ChatInput,MeetingContextDTO,NoteDTO} from "@meeting-notes/contracts"
import {FIXTURES,distractorNotes,fixtureById,fixtureNote,isoDate,meetingContextFor,noteId} from "../lib/fixtures"
import {runChat} from "../lib/harness"
import {chatFormat,citationChecks,grounding,mustAll,mustNot,ABSTAIN} from "../lib/checks"
import {citedClaims,completed,goldText,resolver,truth,SPEAKER_NOTE,type Scenario} from "../lib/scenario"
import type {ChatTurn,Pattern} from "../lib/types"
import {coverageCheck} from "../workspace/scenarios"

interface WorkspaceCase {
  id:string;variant:"find-meeting"|"aggregate"|"date-scoped"|"recent-todos"|"absent"|"follow-up"
  question:(date:(daysAgo:number)=>string)=>string
  history?:ChatTurn[]
  reference:string;must:Pattern[];forbid?:Pattern[]
  /** Fixtures whose meetings hold the answer. */
  relevant:string[];fast?:boolean
}
const CASES:WorkspaceCase[]=[
  {id:"outage-duration",variant:"find-meeting",fast:true,relevant:["crosstalk"],question:()=>"How long did the checkout outage last, and what did we tell customers at first?",
    reference:"Forty minutes (14:10 to 14:50 UTC), from the checkout outage review. Support had first told customers fourteen minutes, which was wrong; Marisol is sending a correction.",must:["forty|\\b40\\b","fourteen|\\b14\\b"],forbid:["(?:lasted|down for|outage (?:of|was)) (?:about |roughly )?(?:fourteen|14) minutes"]},
  {id:"kestrel-budget",variant:"find-meeting",relevant:["discovery"],question:()=>"What budget ceiling did Kestrel Freight mention, and is it approved?",
    reference:"Around $60,000 a year as a ceiling, in the Kestrel Freight discovery call. It is not approved; Marcus guessed what Renata, the finance chief, would sign.",must:["60,?000|sixty thousand|60k","not (?:yet )?approved|unapproved|no approved|isn't approved|hasn't been approved|not been approved"],forbid:["budget (?:is|was|has been) approved"]},
  {id:"no-show-rate",variant:"find-meeting",fast:true,relevant:["planning"],question:()=>"In our Q3 planning discussion, what is the no-show rate for clinics that have SMS reminders switched on, compared with overall?",
    reference:"4.5% for clinics with SMS reminders on, against 7% overall (Hamid, in the Clinic Desk Q3 planning meeting). The target with reminders and self-reschedule is under 4%.",must:["4\\.5|four and a half","\\b7 ?(?:%|percent)|seven percent"]},
  {id:"on-call-swap",variant:"find-meeting",relevant:["one-on-one"],question:()=>"Who is covering on-call for Lucas, and why did they volunteer?",
    reference:"Georg offered to swap in because he wants the extra on-call pay before his move. Lucas is off the rotation for two sprints starting next sprint.",must:["Georg","pay|move"]},
  {id:"undecided",variant:"aggregate",relevant:["standup","crosstalk","brainstorm","planning"],question:()=>"Across my meetings in the last three weeks, what was explicitly left undecided or deferred?",
    reference:"Stand-up: whether to hold the Friday demo (Imani to ask Priyanka). Outage review: customer vouchers (finance decides; Vik asking). Launch brainstorm: no launch date until user testing; referral credits parked pending legal. Q3 planning: reporting revamp deferred to Q4; Q4 not yet planned; Verita-or-Q4 fallback if the exit clause is not secured. Kestrel call: pilot terms proposed but not agreed.",must:["demo","voucher","launch date|referral","reporting"]},
  {id:"money",variant:"aggregate",relevant:["discovery","brainstorm","planning","numbers","crosstalk"],question:()=>"Which of my recent meetings discussed budgets or money, and what were the headline figures?",
    reference:"Kestrel discovery: ~$60k/year ceiling (unapproved), $38 per user per month list, $4,500 proposed pilot. Launch brainstorm: $8,000 campaign budget. Q3 planning: $30,000 new-vendor ceiling, $2,500/month Claimwell minimum ($7,500 for the quarter). Northbeam review: $4.82M revenue vs $5.1M plan, $19.6M ARR. Outage review: about 9,400 euros delayed, not lost.",must:["60,?000|sixty thousand|60k","8,?000|eight thousand|8k","30,?000|thirty thousand|30k","4\\.82"]},
  {id:"tobias",variant:"aggregate",relevant:["planning"],question:()=>"What has Tobias committed to, and by when?",
    reference:"From the Q3 planning meeting: negotiate the Claimwell contract with a 90-day exit clause by July 31; send Claimwell's data processing agreement to legal at the same time; renegotiate the SMS vendor contract, starting in August, before it renews on September 30.",must:["Claimwell","SMS","July (?:31|thirty.first)|Jul 31|end of July"]},
  {id:"last-five-days",variant:"date-scoped",fast:true,relevant:["standup","one-on-one","crosstalk"],question:date=>`Summarise what happened in my meetings between ${date(5)} and ${date(0)}.`,
    reference:"Ferry app stand-up (release candidate moved to Thursday; staging certificate to be rotated), the Ama/Lucas 1:1 (Lucas leads the billing migration design and comes off on-call for two sprints), the checkout outage review (forty-minute outage; canary now mandatory for config pushes), and a lunch-and-learn on spreadsheet shortcuts.",must:["release candidate|stand.?up|certificate","Lucas|billing","outage|canary"],forbid:["Kestrel","Northbeam","Meadowbrook","Elias"]},
  {id:"mid-month",variant:"date-scoped",relevant:["planning","numbers"],question:date=>`Which meetings did I have from ${date(22)} to ${date(14)}, and what was decided in each?`,
    reference:"Clinic Desk Q3 planning (committed self-reschedule, an eligibility pilot with five clinics, multi-location phase one; reporting deferred to Q4), the Northbeam Q3 business review (hiring freeze at 152 with two support roles excepted, 6% renewal increase from January 1, discount cap 20%), and a book club note with no decisions.",must:["self.?reschedul|Clinic Desk|eligibility","freeze|Northbeam|6 ?%|six percent"],forbid:["Kestrel","checkout outage","Elias"]},
  {id:"recent-todos",variant:"recent-todos",fast:true,relevant:["standup","one-on-one","crosstalk"],question:()=>"List my recent to-dos",
    reference:"Grouped by date and meeting, most recent first. Stand-up: Kofi rotates the staging certificate; Dario and Wen pair on the flaky checkout test; Imani tells Priyanka about Thursday. 1:1: Ama talks to Deepak and sends an example packet; Lucas sends a packet outline by next Friday and asks Sunita and Georg for reviews. Outage review: Dunya writes the postmortem by Thursday; Paolo adds the canary step; Marisol sends the correction; Vik asks finance about vouchers. It should state which dates and meetings were covered.",must:["certificate|cert\\b","Deepak|packet","post.?mortem|canary"]},
  {id:"singapore-lease",variant:"absent",fast:true,relevant:[],question:()=>"What did we decide about the Singapore office lease?",
    reference:"Nothing in the meetings covers a Singapore office lease. (An office-move logistics note exists, but it does not mention Singapore or a lease.)",must:[ABSTAIN],forbid:["(?:decided|agreed) to (?:sign|renew|terminate|extend)","lease (?:was|is|will be) (?:signed|renewed|terminated)"]},
  {id:"kestrel-next-steps",variant:"follow-up",relevant:["discovery"],question:()=>"And what are the next steps there, on both sides?",
    history:[{role:"user",content:"Remind me what Kestrel Freight needs from us."},{role:"assistant",content:"Two hard requirements: the product must work offline (their Reno yard has almost no signal) and integrate directly with Stratus WMS. Okta SSO is a nice-to-have."}],
    reference:"Tessa sends the security questionnaire answers and the offline spec by Friday, plus a one-page pilot proposal, and confirms how many on-site onboarding days are possible. Joon sends the Stratus API documentation and sandbox credentials. Marcus confirms a demo with Renata on the 19th.",must:["questionnaire","Joon|API|sandbox","19th|nineteenth|Renata"]},
]

export function workspaceCorpus(now:Date):{notes:NoteDTO[];meetings:Map<string,MeetingContextDTO>} {
  const notes=[...FIXTURES.map(fixture=>fixtureNote(fixture,{now,level:"sparse",summary:true})),...distractorNotes(now)]
  const meetings=new Map<string,MeetingContextDTO>()
  for(const fixture of FIXTURES){const meeting=meetingContextFor(fixture,now);if(meeting)meetings.set(noteId(fixture),meeting)}
  return {notes,meetings}
}
export const workspaceChatScenarios=(now:Date):Scenario[]=>{
  const {notes,meetings}=workspaceCorpus(now),resolve=resolver(notes),date=(daysAgo:number)=>isoDate(now,daysAgo)
  const catalogue=notes.map(note=>`- ${note.date} ${note.title}`).sort().reverse().join("\n")
  return CASES.map((item):Scenario=>{
    const question=item.question(date),input:ChatInput={message:question,history:item.history??[],includeSources:true,timeZone:"UTC"}
    const relevant=item.relevant.map(fixtureById)
    const source=relevant.length?relevant.map(fixture=>`=== ${fixture.title} (${date(fixture.daysAgo)}) ===\n${truth(fixture)}`).join("\n\n"):"(no meeting contains the answer)"
    return {
      feature:"workspace-chat",id:`workspace-chat/${item.id}`,fixture:item.relevant.join("+")||"none",variant:item.variant,fast:!!item.fast,
      input:{question,...(item.history?{history:item.history}:{}),reference:item.reference},
      execute:target=>runChat(target,notes,null,input,meetings),
      check:execution=>!execution.ok?[completed(execution)]:[
        completed(execution),
        mustAll(execution.output,item.must,"answer-correct"),
        ...(item.forbid?.length?[mustNot(execution.output,item.forbid,"answer-not-wrong")]:[]),
        ...citationChecks(execution.output,{citations:execution.citations,resolve,expectedNoteIds:relevant.map(noteId),required:relevant.length>0}),
        ...chatFormat(execution.output,question),
        coverageCheck(execution),
        // Grounded against every meeting in the workspace: a number or name
        // from any of them is legitimate here.
        ...grounding(execution.output,[...notes.map(note=>`${note.title}\n${note.date}\n${note.body}\n${note.enhanced}\n${note.transcriptSegments.map(segment=>segment.text).join("\n")}`),question,...(item.history??[]).map(turn=>turn.content),FIXTURES.map(fixture=>Object.keys(fixture.people).join(" ")).join(" "),JSON.stringify([...meetings.values()])].join("\n")),
      ],
      judge:execution=>({feature:"workspace-chat",
        task:`Answer a question across all of the user's meetings (${item.variant}). Today is ${date(0)}. The assistant had search and read tools over these meetings and had to find the right ones itself:\n${catalogue}\n${item.history?`Earlier turns:\n${item.history.map(turn=>`${turn.role}: ${turn.content}`).join("\n")}\n`:""}Question: ${question}\nThe source below is the full transcript of only the meetings that hold the answer. Mentioning other meetings is fine if accurate and relevant; you cannot verify those details, so judge faithfulness on what you can check. ${SPEAKER_NOTE}`,
        source,gold:relevant.map(fixture=>`${fixture.title}\n${goldText(fixture)}`).join("\n\n")||undefined,reference:item.reference,output:execution.output,citations:citedClaims(execution,resolve)}),
    }
  })
}
