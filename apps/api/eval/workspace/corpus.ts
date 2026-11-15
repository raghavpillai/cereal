/**
 * A larger fictional workspace for cross-meeting ("workspace") chat: 56 meetings
 * over about four and a half months at an invented company, Brindle Labs, which
 * makes a dispatch-planning product called Quillway. Nothing here is real.
 *
 * It exists to stress retrieval, not generation:
 *   - recurring series with near-identical titles (ten weekly product syncs, six 1:1s,
 *     two "Sales pipeline review"s, two "Untitled meeting"s, two interview debriefs)
 *   - the same people and customers across many meetings (Ilse Brandt, Devraj Menon,
 *     Tamsin), and topics that overlap (pricing, the Export launch, the sync outage, SSO)
 *   - facts that change over time (the Export launch date moves twice; pricing goes from
 *     a proposal to a decision to an amendment)
 *   - meetings with no summary, with only a transcript, with only typed notes
 *   - four long meetings (padded with plausible filler that shares vocabulary with the
 *     needles), and summaries that deliberately leave out details the transcript holds
 *
 * Every date is relative to the day the evaluation runs, and dates spoken in the
 * transcripts are rendered from the same clock, so "last week" and "November 18th"
 * always line up. Use `day(now,daysAgo)` for patterns.
 */
import type {MeetingContextDTO,NoteDTO} from "@meeting-notes/contracts"
import {compileFixture,stableUuid} from "../lib/transcript"
import type {Channel,Fixture} from "../lib/types"

const MONTHS=["January","February","March","April","May","June","July","August","September","October","November","December"]
const WEEKDAYS=["Sunday","Monday","Tuesday","Wednesday","Thursday","Friday","Saturday"]
const ordinal=(n:number)=>`${n}${n%10===1&&n%100!==11?"st":n%10===2&&n%100!==12?"nd":n%10===3&&n%100!==13?"rd":"th"}`
export interface Day {iso:string;
  /** As people say it: "November 18th". */
  spoken:string;weekday:string;
  /** Regular-expression source matching the usual ways an answer writes this date. */
  pattern:string;
  /** As `pattern`, and also the day inside a list under its month: "November 4 and 11". */
  listed:string}
/** A calendar day `daysAgo` days before the run (negative = in the future), in UTC. */
export function day(now:Date,daysAgo:number):Day {
  const date=new Date(Date.UTC(now.getUTCFullYear(),now.getUTCMonth(),now.getUTCDate())-daysAgo*86_400_000)
  const month=MONTHS[date.getUTCMonth()]!,d=date.getUTCDate(),iso=date.toISOString().slice(0,10),short=month.slice(0,3)
  return {iso,spoken:`${month} ${ordinal(d)}`,weekday:WEEKDAYS[date.getUTCDay()]!,
    pattern:`${short}[a-z]*\\.? ${d}(?:st|nd|rd|th)?\\b|\\b${d}(?:st|nd|rd|th)? (?:of )?${short}|${iso}|\\b${date.getUTCMonth()+1}/${d}\\b`,
    listed:`${short}[a-z]*\\.? ${d}(?:st|nd|rd|th)?\\b|\\b${d}(?:st|nd|rd|th)? (?:of )?${short}|${iso}|\\b${date.getUTCMonth()+1}/${d}\\b|${short}[a-z]*\\.?(?: \\d{1,2}(?:st|nd|rd|th)?,?| and)* ${d}(?:st|nd|rd|th)?\\b`}
}

export interface LargeMeeting {
  key:string;title:string;daysAgo:number;capture:"desktop"|"ios"
  people:Record<string,Channel>
  /** `Name: words` per line, as in the other fixtures. Empty for a note with no recording. */
  script:string
  /** Typed notes. */
  notes?:string
  /** The generated document. Absent = never generated. */
  summary?:string
  attendees?:Array<[name:string,email:string]>
  /** Filler turns inserted at `[[pad]]` markers in the script, to make a long meeting. */
  pad?:{turns:number;topics:string[]}
  /** One line a recap of this meeting must contain, for date-scoped scenarios. */
  headline:{text:string;must:string}
}

const FILLER=[
  "Before we go further, can someone recap where we landed on {a} last time? I want to be sure we are not reopening it.",
  "My read is that {a} is fine for now, the real question is how it interacts with {b}.",
  "I looked at the numbers for {a} this morning and nothing jumped out, it is roughly flat week over week.",
  "Can we take {b} offline? I think only two of us need to be in that conversation.",
  "The thing I keep hearing from customers about {a} is that they want fewer surprises, not more features.",
  "Agreed. I would rather be boring and predictable on {a} than clever.",
  "Let me share my screen for a second, this is the dashboard for {b}, you can see the dip on the left.",
  "That dip is the holiday week, I would not read anything into it.",
  "Who is the right person to talk to about {a}? I have asked twice and got two different answers.",
  "Probably whoever owns {b}, they are tangled together in the code anyway.",
  "We said we would revisit {a} once the quarter closes, I do not think anything has changed since.",
  "One risk I want to name on {b}: we have exactly one person who understands it end to end.",
  "That is fair. We should write it down at least, even a rough page would help.",
  "I can draft something on {a}, but it will not be this week.",
  "Is there a customer we can ask about {b}? I would trust that more than our own guesses.",
  "Support has a list, I will pull the five most recent conversations that mention {a}.",
  "Okay. Parking {b} for now, let us keep moving through the agenda.",
  "Sorry, I was on mute. I was saying the timeline for {a} depends on design capacity more than engineering.",
  "Design capacity is tight until the new hire starts, so let us be honest about that.",
  "From a budget point of view {b} is a rounding error, so I would not optimise for cost there.",
  "Understood. Then the constraint is attention, not money.",
  "Do we have a document for {a}? I would like to read it before I form an opinion.",
  "There is a draft, it is about two pages, I will drop the link in the channel after this.",
  "I think we are agreeing loudly. Shall we move on?",
]
/** Deterministic filler dialogue that shares everyday vocabulary with the real content. */
function filler(speakers:ReadonlyArray<string>,topics:ReadonlyArray<string>,turns:number,seed:number):string {
  const lines:string[]=[]
  for(let i=0;i<turns;i++){
    const text=FILLER[(seed*7+i*5)%FILLER.length]!.replace("{a}",topics[(seed+i)%topics.length]!).replace("{b}",topics[(seed+i*3+1)%topics.length]!)
    lines.push(`${speakers[(seed+i)%speakers.length]}: ${text}`)
  }
  return lines.join("\n")
}

const TEAM={Noor:"mic",Jorge:"system",Wren:"system",Mirela:"system"} as const
const email=(name:string,domain="brindlelabs.example")=>`${name.toLowerCase().replace(/[^a-z]+/g,".")}@${domain}`
const INTERNAL:Record<string,string>={Noor:"Noor Haddad",Tamsin:"Tamsin Okoye",Jorge:"Jorge Almeida",Wren:"Wren Castellan",Mirela:"Mirela Stan",Aditi:"Aditi Rao",Lowell:"Lowell Finch",Kenji:"Kenji Watanabe",Odette:"Odette Marsh",Farid:"Farid Nasser"}
const staff=(...names:string[]):Array<[string,string]>=>names.map(name=>[INTERNAL[name]!,email(INTERNAL[name]!)])

/** Every meeting, newest last within each group. `d(n)` renders the date n days ago. */
export function largeMeetings(now:Date):LargeMeeting[] {
  const d=(daysAgo:number)=>day(now,daysAgo).spoken
  // The Export launch date moves twice: L1 -> L2 -> L3.
  const L1=d(31),L2=d(10),L3=d(-18),GONOGO=d(-11)
  const sync=(n:number,daysAgo:number,script:string,summary:string|undefined,notes:string|undefined,headline:LargeMeeting["headline"],extra:Record<string,Channel>={}):LargeMeeting=>({
    key:`sync-${n}`,title:"Quillway weekly product sync",daysAgo,capture:"desktop",people:{...TEAM,...extra},script,...(summary?{summary}:{}),...(notes?{notes}:{}),attendees:staff("Noor","Jorge","Wren","Mirela"),headline})
  const oneOnOne=(n:number,daysAgo:number,script:string,summary:string|undefined,notes:string|undefined,headline:LargeMeeting["headline"]):LargeMeeting=>({
    key:`1on1-${n}`,title:"Noor / Tamsin 1:1",daysAgo,capture:"ios",people:{Noor:"mic",Tamsin:"mic"},script,...(summary?{summary}:{}),...(notes?{notes}:{}),attendees:staff("Noor","Tamsin"),headline})
  return [
    // ------------------------------------------------------------ weekly sync
    sync(1,87,`
Noor: Okay, first sync with Export on the agenda. Jorge, where are we?
Jorge: Kickoff was Monday. The plan is a beta with five customers, then general launch. The target I gave the team is ${L1}.
Noor: ${L1} for the public launch, and the beta before that?
Jorge: Beta two weeks before. It is tight but doable if the permissions work does not grow.
Wren: On single sign-on, I am still scoping. I will have settings mockups by next sync.
Mirela: Support side, nothing unusual. Twenty one tickets last week, mostly password resets.
Noor: Good. So Export target is ${L1}, Wren owns the single sign-on mockups, and I will line up the five beta customers.
`,`# Export\n- Kickoff done. Beta with five customers, then general launch.\n- Target launch date: ${L1}.\n\n# Single sign-on\n- Still being scoped; Wren to bring settings mockups to the next sync.\n\n# Action items\n- Noor: line up five beta customers for Export.\n- Wren: single sign-on settings mockups.`,
      "export kickoff\n- beta w/ 5 customers\n- SSO mockups next week (Wren)",{text:`Export kicked off with a launch target of ${L1}.`,must:"Export"}),
    sync(2,73,`
Noor: Two things today, mobile offline mode and Export formats.
Jorge: On offline mode, I do not have the people. If we want Export this quarter, offline has to move.
Noor: I agree. Offline mode moves to next quarter. Any objection?
Wren: None from design, the offline flows are not ready anyway.
Jorge: For Export formats, I want to ship CSV and Parquet only. Excel is a rabbit hole, the formatting bugs never end.
Mirela: Customers will ask for Excel.
Noor: They can open the CSV. Decision: CSV and Parquet only at launch, no Excel.
Mirela: Okay, I will write a help article explaining how to open the CSV in Excel.
`,`# Mobile offline mode\n- Moved to next quarter to protect Export capacity.\n\n# Export formats\n- Decision: CSV and Parquet only at launch. No Excel export.\n\n# Action items\n- Mirela: help article on opening the CSV in Excel.`,
      undefined,{text:"Offline mode moved to next quarter; Export will ship CSV and Parquet only.",must:"offline|Parquet|CSV"}),
    sync(3,59,`
Jorge: Bad news on Export. The permissions model is bigger than we thought, row-level access has to be respected in every export.
Noor: How much bigger?
Jorge: Three weeks. I do not think ${L1} is realistic any more. I would say ${L2}.
Noor: Okay. New Export target is ${L2}. I will tell Tamsin and the beta customers.
Wren: For single sign-on, I recommend we support Okta and Microsoft Entra first. Google Workspace can come later, only two prospects have asked.
Noor: Fine. Okta and Entra first, Google Workspace later.
Mirela: Ticket volume is up a little, nineteen about the route editor.
`,`# Export\n- Permissions (row-level access) work is larger than planned.\n- Launch target moved from ${L1} to ${L2}.\n\n# Single sign-on\n- Decision: Okta and Microsoft Entra first; Google Workspace later.\n\n# Action items\n- Noor: tell Tamsin and the beta customers about the new Export date.`,
      "export slips - permissions\nSSO: Okta + Entra first",{text:`Export slipped to ${L2} because of the permissions work; single sign-on will support Okta and Entra first.`,must:"Export|Okta|Entra"}),
    sync(4,45,`
Mirela: I want to flag something. We have had fourteen tickets this week about duplicate stops appearing on routes.
Jorge: That smells like the sync service retrying. If a write times out, the client sends it again and we create the stop twice.
Noor: Is it getting worse?
Mirela: It was three the week before, so yes.
Jorge: I will add idempotency keys to the stop creation endpoint. That is about a week of work.
Noor: Please do. Does it delay Export?
Jorge: A few days at most.
Wren: Nothing from me, I am heads down on the onboarding checklist.
`,`# Sync reliability\n- Fourteen tickets this week about duplicate stops on routes (three the week before).\n- Likely cause: the sync service retrying timed-out writes.\n\n# Action items\n- Jorge: add idempotency keys to the stop creation endpoint (about a week).`,
      undefined,{text:"Fourteen tickets about duplicate stops; Jorge to add idempotency keys.",must:"duplicate stops|idempotency"}),
    sync(5,38,`
Noor: First sync since the outage. Let us keep it short. Jorge?
Jorge: The incident review was on Monday, the short version is expired queue credentials. We are freezing releases for one week while Kenji finishes the postmortem actions.
Noor: A one week release freeze, starting today?
Jorge: Starting today.
Mirela: We have sent the apology email to every affected customer. Eleven have replied asking for a credit so far.
Noor: Route those to Lowell, finance is deciding the credit policy.
Wren: The freeze does not affect design. I will use the week to test the onboarding checklist with three customers.
`,`# After the sync outage\n- One-week release freeze starting today while postmortem actions are finished.\n- Apology email sent to every affected customer; eleven have asked for a credit so far, routed to Lowell.\n\n# Design\n- Wren will test the onboarding checklist with three customers during the freeze.`,
      "post-outage\n- release freeze 1 wk\n- credits -> Lowell",{text:"One-week release freeze after the sync outage; credit requests go to Lowell.",must:"freeze"}),
    sync(6,31,`
Noor: Single sign-on beta. Who goes first?
Jorge: Harrowgate. They have been asking since the discovery call and Devraj will actually test it.
Noor: Agreed, Harrowgate is the first single sign-on beta customer.
Wren: One open question. Nobody owns SCIM provisioning. It is not design, and Jorge's team says it is not in their scope.
Jorge: It is not in the current scope, that is true.
Noor: I do not have an answer today. Let us leave SCIM ownership open and I will raise it with Tamsin.
Mirela: Also, the pricing page experiment starts Thursday, the variant shows annual prices first.
`,`# Single sign-on beta\n- Harrowgate Logistics will be the first beta customer.\n\n# Open questions\n- Who owns SCIM provisioning? Not design, not in engineering's current scope. Noor to raise with Tamsin.\n\n# Other\n- Pricing page experiment (annual prices first) starts Thursday.`,
      undefined,{text:"Harrowgate chosen as the first single sign-on beta customer; SCIM ownership left open.",must:"Harrowgate|SCIM"}),
    sync(7,24,`
Noor: Export. I have news from Pinecrest Health, they will not roll out without an audit log on every export.
Jorge: That is new scope. An audit log of who exported what and when is about two and a half weeks.
Noor: Can we launch without it and add it later?
Jorge: We can, but then Pinecrest does not use it, and healthcare is half the reason we are building this.
Noor: Then we move. What is the date with the audit log included?
Jorge: ${L3}.
Noor: Decision: Export launches ${L3}, with the audit log. ${L2} is off.
Mirela: I will update the beta customers.
Wren: I can design the audit log screen this week, it is a simple table.
`,`# Export\n- Pinecrest Health will not roll out without an audit log on exports.\n- Decision: include the audit log and move the launch from ${L2} to ${L3}.\n\n# Action items\n- Mirela: update the beta customers on the new date.\n- Wren: design the audit log screen this week.`,
      "export -> audit log needed (Pinecrest)\nnew date!",{text:`Export launch moved to ${L3} to include an audit log for Pinecrest Health.`,must:"audit log|Export"}),
    sync(8,17,`
Jorge: Single sign-on status. Okta is ready for beta. Entra slipped two weeks, the group claims mapping was harder than expected.
Noor: Harrowgate is on Entra. So their beta slips two weeks as well?
Jorge: Yes, unfortunately.
Wren: I have the onboarding checklist results. Three customers tested it. Everybody got stuck on team templates.
Noor: What do you recommend?
Wren: Cut team templates from onboarding version one. Nobody needs them on day one.
Noor: Agreed, team templates are cut from onboarding version one.
Mirela: Quiet week in support. Sixteen tickets.
`,`# Single sign-on\n- Okta is ready for beta. Entra slipped two weeks (group claims mapping).\n- Harrowgate is on Entra, so their beta slips too.\n\n# Onboarding\n- Checklist tested with three customers; all got stuck on team templates.\n- Decision: cut team templates from onboarding version one.`,
      undefined,{text:"Entra single sign-on slipped two weeks; team templates cut from onboarding version one.",must:"Entra|team templates"}),
    sync(9,10,`
Noor: Kenji is joining us for five minutes on the outage actions.
Kenji: Thanks. Four of the six postmortem actions are done. The two left are the regional failover test and the alert tuning.
Noor: When?
Kenji: Failover test is booked for ${d(-4)}. Alert tuning I will finish this week.
Jorge: Export docs are ready, by the way. The reference pages and the tutorial.
Mirela: One question I keep getting from sales. Do we charge extra for Parquet?
Noor: I do not know yet. That is an open question for the pricing group, I will take it there.
Wren: Audit log screen is done and handed over.
`,`# Sync outage follow-up\n- Four of six postmortem actions done. Remaining: regional failover test (booked for ${d(-4)}) and alert tuning (this week).\n\n# Export\n- Documentation (reference and tutorial) is ready. Audit log screen designed and handed over.\n\n# Open questions\n- Do we charge extra for Parquet export? Noor to take to the pricing group.`,
      "Kenji: 4/6 outage actions done\nParquet pricing??",{text:"Four of six outage actions are done; whether to charge for Parquet export is an open question.",must:"failover|Parquet|postmortem|outage"},{Kenji:"system"}),
    sync(10,3,`
Noor: Export go or no-go. When do we decide?
Jorge: I have put the go/no-go meeting on ${GONOGO}. If it is a go we launch ${L3} as planned.
Noor: Good. Odette is sending the launch email, Mirela, are the support macros ready?
Mirela: Drafted. I will finish them by Tuesday.
Wren: SCIM again. It still has no owner. Devraj at Harrowgate asked me about it directly.
Noor: I know. It is still unresolved, Tamsin and I have not settled it.
Jorge: The Harrowgate single sign-on beta invite can go out now, Entra passed testing yesterday.
Noor: I will send Harrowgate the single sign-on beta invite by Friday.
`,`# Export\n- Go/no-go meeting on ${GONOGO}; launch stays ${L3} if it is a go.\n- Launch email with Odette; support macros drafted, Mirela to finish by Tuesday.\n\n# Single sign-on\n- Entra passed testing. Noor to send Harrowgate the beta invite by Friday.\n\n# Open questions\n- SCIM provisioning still has no owner.`,
      "go/no-go date set\nsend Harrowgate SSO invite (me, Fri)",{text:`Export go/no-go set for ${GONOGO}; Noor to send Harrowgate the single sign-on beta invite by Friday.`,must:"go/no.go|beta invite|Harrowgate"}),

    // ------------------------------------------------------------------- 1:1s
    oneOnOne(1,89,`
Tamsin: How are you feeling about the role?
Noor: Good, but I want more of the commercial side. I would like to lead the pricing work.
Tamsin: Then join the pricing committee. Lowell chairs it, I will tell him you are coming.
Noor: Thank you. Anything you want from me this month?
Tamsin: Keep Export on the rails. That is the one that matters.
`,`# Growth\n- Noor wants more commercial scope and to lead pricing work.\n- Tamsin will get Noor onto the pricing committee (chaired by Lowell).\n\n# Priorities\n- Keep Export on track.`,undefined,{text:"Noor to join the pricing committee.",must:"pricing committee"}),
    oneOnOne(2,61,`
Tamsin: I am worried about engineering capacity. Jorge has three projects and eight people.
Noor: Export is the one that cannot slip again.
Tamsin: Then I need a one-pager on Export scope by next Wednesday, what is in, what is out, what we cut first.
Noor: I will write it.
Tamsin: And keep it to one page, I am going to forward it to the leadership team.
`,`# Capacity\n- Tamsin is worried about engineering capacity (three projects, eight people).\n\n# Action items\n- Noor: one-page Export scope document by next Wednesday (in, out, what gets cut first).`,"T worried re: eng capacity\n1-pager on Export scope by Weds",{text:"Tamsin asked for a one-page Export scope document.",must:"one.pager|one.page|scope"}),
    oneOnOne(3,47,`
Tamsin: Feedback on the roadmap review. The content was good, it ran forty minutes over.
Noor: I know. People had not read anything beforehand.
Tamsin: Send pre-reads next time, three days ahead. And good news, one product manager hire is approved for the first quarter.
Noor: That is great. Reporting to me?
Tamsin: Reporting to you.
`,`# Feedback\n- The roadmap review ran forty minutes over. Send pre-reads three days ahead next time.\n\n# Headcount\n- One product manager hire approved for the first quarter, reporting to Noor.`,undefined,{text:"One product manager hire approved for the first quarter.",must:"product manager|pre.reads"}),
    oneOnOne(4,33,`
Tamsin: Two things are keeping me up. Harrowgate's renewal, they were hit hard by the outage. And Jorge, he looks burnt out.
Noor: I spoke to Ilse yesterday, she wants a credit. I think we can save it.
Tamsin: Good. On Jorge, take anything off his plate that you can.
Noor: I can run the customer advisory board so he does not have to present.
Tamsin: Yes, please take the customer advisory board.
`,`# Concerns\n- Harrowgate renewal risk after the outage.\n- Jorge looks burnt out.\n\n# Action items\n- Noor takes over the customer advisory board.`,"Harrowgate renewal risk\nJorge burnout\nI take CAB",{text:"Noor takes over the customer advisory board; Tamsin is worried about the Harrowgate renewal.",must:"advisory board|Harrowgate"}),
    // No summary was ever generated for the two most recent 1:1s.
    oneOnOne(5,19,`
Tamsin: The board wants a net revenue retention number in the next deck. Can you get it from Lowell?
Noor: Yes. Anything else on your mind?
Tamsin: Honestly, launching Export without the audit log would have been a mistake, I am glad you moved the date.
Noor: Me too. For the customer advisory board, I am drafting the invite list.
Tamsin: How many?
Noor: Twelve customers. I will have the list to you by ${d(12)}.
Tamsin: Good. Include Pinecrest.
`,undefined,"NRR number for board deck (ask Lowell)\nCAB invite list - 12 customers",{text:"Noor to draft a twelve-customer advisory board invite list.",must:"twelve|12|invite list|retention"}),
    oneOnOne(6,5,`
Tamsin: Let me tell you what I am worried about right now, in order.
Noor: Go ahead.
Tamsin: First, SCIM. Nobody owns it and Harrowgate needs it by the end of the first quarter. Second, Wren is overloaded, she is on two projects and both are yours.
Noor: I can move the onboarding work to the new designer when they start.
Tamsin: Do that. Third, year-end planning, the deadline is closer than people think.
Noor: Understood.
Tamsin: One more thing, and this is good news. Your promotion case goes to calibration in January. I need a wins document from you by ${d(-9)}.
Noor: I will write the wins document by ${d(-9)}.
Tamsin: Keep it factual, numbers where you have them.
`,undefined,undefined,{text:"Tamsin is worried about SCIM ownership, Wren's workload and year-end planning; Noor to write a wins document.",must:"SCIM|wins doc|Wren"}),

    // ------------------------------------------------------- Harrowgate calls
    {key:"harrowgate-discovery",title:"Harrowgate Logistics – discovery",daysAgo:120,capture:"desktop",people:{Noor:"mic",Aditi:"mic",Ilse:"system",Devraj:"system"},attendees:[...staff("Noor","Aditi"),["Ilse Brandt","ilse.brandt@harrowgate.example"],["Devraj Menon","devraj.menon@harrowgate.example"]],script:`
Aditi: Thanks for making time. Ilse, can you describe how dispatch works today?
Ilse: We have thirty eight dispatchers across four depots. When a driver calls in sick, somebody rebuilds the routes by hand in a spreadsheet.
Noor: How much time does that take?
Ilse: I measured it last month. About twenty two hours a week, across the team, just on manual rescheduling.
Noor: Twenty two hours a week, that is a lot.
Devraj: From the IT side I have two questions. Single sign-on with Microsoft Entra, and where the data lives. We need it in the European Union.
Noor: Data is hosted in Frankfurt. Entra single sign-on is on the roadmap, not available yet.
Ilse: On price. If we commit for two years, I would expect fifteen percent off.
Aditi: I cannot agree to fifteen percent today, but I will take it back.
Ilse: Please do. That is the number my finance director will ask about.
`,summary:`# Harrowgate today\n- Thirty-eight dispatchers across four depots; routes are rebuilt by hand in a spreadsheet when a driver is out.\n- Significant time is lost each week to manual rescheduling.\n\n# Requirements\n- Single sign-on with Microsoft Entra (not available yet).\n- Data hosted in the European Union: confirmed, Frankfurt.\n\n# Commercial\n- Ilse asked for a discount in exchange for a two-year commitment. Aditi to take it back.`,
      notes:"Harrowgate - 38 dispatchers, 4 depots\nEntra SSO a must\nwants discount for 2yr",headline:{text:"Harrowgate discovery: 38 dispatchers, need Entra single sign-on and EU hosting.",must:"Harrowgate"}},
    {key:"harrowgate-pricing",title:"Harrowgate – pricing call",daysAgo:75,capture:"desktop",people:{Noor:"mic",Aditi:"mic",Ilse:"system",Devraj:"system"},attendees:[...staff("Noor","Aditi"),["Ilse Brandt","ilse.brandt@harrowgate.example"],["Devraj Menon","devraj.menon@harrowgate.example"]],script:`
Aditi: We can offer ten percent off for a two-year term.
Ilse: I asked for fifteen.
Aditi: I know. Ten is what finance approved.
Ilse: Then I need protection on the renewal. Cap any increase at five percent.
Aditi: A five percent renewal cap. I can do that.
Ilse: Then we have a deal. Ten percent off, two years, renewal increases capped at five percent.
Devraj: Two technical asks before we sign. SCIM provisioning, and audit logs for administrator actions.
Noor: Both are planned. I cannot give you dates today, I will come back to you.
Devraj: Please put it in writing.
`,summary:`# Commercial terms agreed\n- Ten percent discount for a two-year term (Ilse had asked for fifteen).\n- Renewal price increases capped at five percent.\n\n# Technical asks from Devraj\n- SCIM provisioning.\n- Audit logs for administrator actions.\n- Noor to follow up in writing with timing.`,
      headline:{text:"Harrowgate agreed ten percent off for two years with a five percent renewal cap.",must:"Harrowgate|ten percent|10 ?%"}},
    {key:"harrowgate-renewal",title:"Harrowgate renewal check-in",daysAgo:32,capture:"desktop",people:{Noor:"mic",Ilse:"system"},attendees:[...staff("Noor"),["Ilse Brandt","ilse.brandt@harrowgate.example"]],script:`
Ilse: I will be direct. The outage cost us half a day at the Rotterdam depot. Drivers were sitting in the yard.
Noor: I am sorry. That is on us.
Ilse: I want a service credit.
Noor: I can offer one month. I need our finance lead, Lowell, to approve it, so treat it as pending.
Ilse: One month is acceptable if it is confirmed in writing.
Noor: Understood. Anything else?
Ilse: If single sign-on lands, I will add twelve more dispatchers in the new year.
Noor: That is good to hear. I will confirm the credit as soon as I have approval.
`,summary:`# Outage impact\n- The sync outage cost Harrowgate half a day at the Rotterdam depot.\n\n# Service credit\n- Ilse asked for a credit. Noor offered one month, pending approval from Lowell; not yet confirmed.\n\n# Expansion\n- Ilse will add twelve more dispatchers in the new year if single sign-on lands.`,
      notes:"Ilse angry - Rotterdam half day lost\noffered 1 month credit (PENDING Lowell)\n+12 dispatchers if SSO",headline:{text:"Harrowgate asked for a service credit after the outage; one month offered, pending approval.",must:"credit|Harrowgate"}},
    {key:"harrowgate-security",title:"Harrowgate – security review",daysAgo:12,capture:"desktop",people:{Noor:"mic",Jorge:"mic",Devraj:"system",Ilse:"system"},attendees:[...staff("Noor","Jorge"),["Devraj Menon","devraj.menon@harrowgate.example"]],script:`
Devraj: Three items. First, I need your latest penetration test report.
Jorge: The most recent one is from last quarter. I will send it under the non-disclosure agreement.
Devraj: Second, SCIM. We need it by the end of the first quarter or I cannot approve the expansion.
Noor: Noted. I cannot commit to that date today.
Devraj: Third, IP allow-listing for the admin console.
Jorge: That exists already, it is a setting. I will send the instructions.
Devraj: Good. I think Ilse is trying to join, by the way.
Ilse: Sorry, I am joining late, I only have ten minutes. Noor, two things from me. Where is my single sign-on beta invite, and is the credit confirmed?
Noor: Hello Ilse. The invite goes out this week. The credit is approved, one month, I will confirm it in writing by Friday.
Ilse: Thank you. That is all I needed.
`,summary:`# Security items from Devraj\n- Wants the latest penetration test report; Jorge will send last quarter's under the non-disclosure agreement.\n- Needs SCIM by the end of the first quarter to approve the expansion; no date committed.\n- IP allow-listing for the admin console already exists; Jorge to send instructions.\n\n# Action items\n- Jorge: send the penetration test report and the allow-listing instructions.\n- Noor: confirm the one-month service credit in writing by Friday.`,
      headline:{text:"Harrowgate security review: penetration test report, SCIM by end of the first quarter, credit approved.",must:"penetration|pen test|SCIM|Harrowgate"}},

    // --------------------------------------------------------- Pinecrest Health
    {key:"pinecrest-intro",title:"Pinecrest Health intro",daysAgo:66,capture:"desktop",people:{Noor:"mic",Aditi:"mic",Solveig:"system",Thaddeus:"system"},attendees:[...staff("Noor","Aditi"),["Solveig Aune","solveig.aune@pinecrest.example"],["Thaddeus Pike","thaddeus.pike@pinecrest.example"]],script:`
Solveig: We run nine clinics with home-visit nurses. Scheduling the visits is done on whiteboards.
Noor: What would success look like?
Solveig: Fewer missed visits. And for compliance, I need an audit log of anything that leaves the system.
Aditi: We would suggest a pilot. Three clinics, sixty days.
Solveig: Three clinics for sixty days works.
Thaddeus: From procurement, we will need a business associate agreement before any patient data goes in.
Aditi: We have a standard one, I will send it today.
`,summary:`# Pinecrest Health\n- Nine clinics with home-visit nurses; scheduling is done on whiteboards.\n- Goals: fewer missed visits; an audit log for compliance.\n\n# Pilot\n- Agreed: three clinics for sixty days.\n\n# Action items\n- Aditi: send the standard business associate agreement.`,
      headline:{text:"Pinecrest Health agreed a pilot at three clinics for sixty days.",must:"Pinecrest"}},
    {key:"pinecrest-pilot",title:"Pinecrest Health – pilot review",daysAgo:20,capture:"desktop",people:{Noor:"mic",Aditi:"mic",Solveig:"system",Thaddeus:"system"},attendees:[...staff("Noor","Aditi"),["Solveig Aune","solveig.aune@pinecrest.example"],["Thaddeus Pike","thaddeus.pike@pinecrest.example"]],script:`
Solveig: The pilot numbers are good. Missed visits went from eleven percent to seven percent across the three clinics.
Noor: That is better than we hoped.
Solveig: I want to roll out to all nine. But not before Export has the audit log.
Noor: The audit log ships with Export on ${L3}.
Thaddeus: What would nine clinics cost?
Aditi: List price is forty nine dollars per seat per month. You have about one hundred and forty seats.
Thaddeus: I will need that as a formal quote. Our board meets on ${d(-25)} and decides then.
Aditi: You will have the quote this week.
`,summary:`# Pilot results\n- Missed visits fell from eleven percent to seven percent across the three pilot clinics.\n\n# Rollout\n- Solveig wants all nine clinics, but only once Export has the audit log (ships ${L3}).\n- Pinecrest's board decides on ${d(-25)}.\n\n# Commercial\n- Quoted list price of $49 per seat per month for about 140 seats. Aditi to send a formal quote this week.`,
      notes:"pilot: missed visits 11% -> 7%\nwant all 9 clinics after audit log",headline:{text:"Pinecrest pilot cut missed visits from eleven to seven percent; rollout waits on the Export audit log.",must:"Pinecrest|missed visits"}},

    // ------------------------------------------------------------------ pricing
    {key:"pricing-1",title:"Pricing committee",daysAgo:80,capture:"desktop",people:{Noor:"mic",Lowell:"system",Aditi:"system"},attendees:staff("Noor","Lowell","Aditi"),script:`
Lowell: My proposal is simple. A twelve percent increase across the board, effective January first.
Aditi: Twelve percent will hurt. The accounts under fifty seats are the ones who churn when the price moves.
Lowell: They are also the least profitable.
Noor: I do not think we can decide today. I would like to model a few scenarios first.
Lowell: Fine. No decision today. Noor models the scenarios, we meet again in a month.
`,summary:`# Proposal\n- Lowell proposed a twelve percent price increase across the board from January 1.\n- Aditi is concerned about churn in accounts under fifty seats.\n\n# Outcome\n- No decision. Noor to model scenarios before the next meeting.`,
      headline:{text:"Lowell proposed a twelve percent increase; no decision.",must:"twelve percent|12 ?%|pricing"}},
    {key:"pricing-2",title:"Pricing committee – follow-up",daysAgo:52,capture:"desktop",people:{Noor:"mic",Lowell:"system",Aditi:"system"},attendees:staff("Noor","Lowell","Aditi"),script:`
Noor: I modelled three scenarios. Eight percent keeps churn inside what we saw last year. Twelve does not.
Lowell: Eight percent gets us about four hundred and ten thousand dollars of incremental annual recurring revenue. I can live with that.
Aditi: I still object. I want accounts under fifty seats exempt. We have about thirty small accounts I think we lose.
Lowell: Thirty small accounts is not material.
Aditi: It is material to my team's number.
Noor: I hear you. I suggest we decide eight percent today and come back to the small accounts next month.
Lowell: Agreed. Eight percent for existing customers from January first. And the new list price goes from forty four dollars to forty nine dollars per seat.
Aditi: Noted that I disagree on the small accounts.
`,summary:`# Decisions\n- Eight percent price increase for existing customers, effective January 1.\n- New list price: $49 per seat per month (up from $44).\n\n# Expected impact\n- About $410,000 of incremental annual recurring revenue.\n\n# Follow-up\n- Treatment of small accounts to be revisited next month.`,
      notes:"8% it is\nlist 44 -> 49",headline:{text:"Decided an eight percent increase for existing customers and a $49 list price.",must:"eight percent|8 ?%|\\$?49"}},
    {key:"pricing-3",title:"Pricing working session",daysAgo:26,capture:"desktop",people:{Noor:"mic",Lowell:"system",Aditi:"system"},attendees:staff("Noor","Lowell","Aditi"),script:`
Noor: Small accounts. Aditi, your proposal?
Aditi: Grandfather every account under fifty seats for twelve months. They stay on forty four dollars.
Lowell: Twelve months, then they move to the new price?
Aditi: Yes.
Lowell: I can accept that. In exchange I want the annual prepay discount cut from ten percent to eight percent.
Noor: Any objection? Then both are decided. Accounts under fifty seats are grandfathered for twelve months, and the annual prepay discount goes from ten to eight percent.
Aditi: I will draft the customer communication by ${d(-3)}.
Noor: One open item from the product sync, whether Parquet export is a paid add-on. Not today, next session.
`,summary:`# Decisions\n- Accounts under fifty seats are grandfathered at $44 per seat for twelve months.\n- Annual prepay discount reduced from ten percent to eight percent.\n\n# Open\n- Whether Parquet export becomes a paid add-on (next session).\n\n# Action items\n- Aditi: draft the customer communication by ${d(-3)}.`,
      headline:{text:"Small accounts grandfathered for twelve months; annual prepay discount cut to eight percent.",must:"grandfather|prepay"}},

    // ----------------------------------------------------------------- incident
    {key:"incident-review",title:"Incident review: sync outage",daysAgo:41,capture:"desktop",people:{Noor:"mic",Jorge:"system",Kenji:"system",Mirela:"system"},attendees:staff("Noor","Jorge","Kenji","Mirela"),pad:{turns:600,topics:["the queue","alerting","the rotation script","customer comms","credits","the runbook","idempotency","failover"]},script:`
Jorge: Let us walk the timeline. Kenji, you were on call.
Kenji: The first thing I saw was the consumer lag alarm on the eu-west queue, at six fifty two. Nothing else was red yet.
Kenji: I assumed a slow consumer and restarted it. That did nothing.
Jorge: When did we understand it was credentials?
Kenji: About nine fifteen. The rotation script had failed silently the night before, so the queue credentials expired at six forty.
Noor: So the outage ran from six forty to ten o'clock?
Kenji: Six forty to ten, universal time. Three hours and twenty minutes.
[[pad]]
Mirela: Customer impact. Two hundred and twelve customers were affected. Thirty seven of them ended up with duplicate stops on their routes.
Noor: What do we tell them about credits?
Jorge: I suggest one month for any customer who asks. Lowell has to sign off.
Noor: Okay, the policy is one month of credit for customers who ask, subject to Lowell.
[[pad]]
Jorge: Actions. Kenji writes the postmortem and owns the six follow-up actions. I will ship the idempotency keys. Mirela handles customer communication.
Kenji: I will have the postmortem out by Friday.
`,summary:`# What happened\n- The sync service was down for three hours and twenty minutes (06:40 to 10:00 UTC).\n- Cause: queue credentials expired after the rotation script failed silently the night before.\n\n# Impact\n- 212 customers affected; 37 ended up with duplicate stops.\n\n# Decisions\n- Credit policy: one month for any customer who asks, subject to Lowell's sign-off.\n\n# Action items\n- Kenji: postmortem by Friday; owns six follow-up actions.\n- Jorge: ship idempotency keys.\n- Mirela: customer communication.`,
      notes:"outage 3h20\nroot cause: rotation script / expired creds\n212 customers",headline:{text:"Sync outage review: three hours twenty minutes, caused by expired queue credentials.",must:"outage|credentials"}},
    {key:"incident-followup",title:"Incident follow-up",daysAgo:34,capture:"desktop",people:{Noor:"mic",Jorge:"system",Kenji:"system"},attendees:staff("Noor","Jorge","Kenji"),script:`
Kenji: Postmortem is published. Root cause is confirmed, the rotation script swallowed an error.
Jorge: Idempotency keys are in production as of yesterday.
Noor: Good. What is left?
Kenji: The big one is a regional failover test. I want to do it before year end.
Noor: Agreed. We do a regional failover test before the end of the year.
Kenji: I will book a date once the release freeze lifts.
`,summary:`# Status\n- Postmortem published; root cause confirmed (rotation script swallowed an error).\n- Idempotency keys are in production.\n\n# Decision\n- Run a regional failover test before the end of the year. Kenji to book a date after the release freeze.`,
      headline:{text:"Postmortem published; regional failover test to happen before year end.",must:"failover|postmortem"}},

    // ------------------------------------------------------------------- hiring
    {key:"debrief-oluwaseun",title:"Interview debrief – Senior designer (Oluwaseun)",daysAgo:55,capture:"desktop",people:{Noor:"mic",Wren:"system",Jorge:"system"},attendees:staff("Noor","Wren","Jorge"),script:`
Wren: Oluwaseun was strong. The systems thinking in the portfolio review was the best I have seen this year.
Jorge: I liked the way he handled the handoff question. Weaker on research, he admitted he usually leans on a researcher.
Noor: Votes?
Wren: Hire.
Jorge: Lean hire.
Noor: I am a hire. Let us see Marguerite on Wednesday before we decide.
`,summary:`# Oluwaseun Adeyemi\n- Strengths: systems thinking, engineering handoff.\n- Concern: lighter on research.\n- Votes: Wren hire, Jorge lean hire, Noor hire.\n- Decision deferred until Marguerite's interview.`,
      headline:{text:"Debrief on Oluwaseun: strong systems thinking, lighter on research.",must:"Oluwaseun"}},
    {key:"debrief-marguerite",title:"Interview debrief – Senior designer (Marguerite)",daysAgo:53,capture:"desktop",people:{Noor:"mic",Wren:"system",Jorge:"system"},attendees:staff("Noor","Wren","Jorge"),script:`
Wren: Marguerite's research and facilitation are excellent. The workshop exercise was a pleasure.
Jorge: Her portfolio is mostly consumer apps though. Not much business software.
Noor: Between the two?
Wren: Oluwaseun. We need someone who can own the design system on day one.
Jorge: Oluwaseun for me as well.
Noor: Agreed. We make the offer to Oluwaseun, senior level. Let us keep Marguerite warm for the next role, she was very good.
Wren: I will call her myself.
`,summary:`# Marguerite Dubois\n- Strengths: research and facilitation.\n- Concern: portfolio is mostly consumer apps.\n\n# Decision\n- Offer goes to Oluwaseun (senior level), because the team needs someone to own the design system from day one.\n- Keep Marguerite warm for the next role; Wren to call her.`,
      headline:{text:"Decided to offer the senior designer role to Oluwaseun and keep Marguerite warm.",must:"Marguerite|Oluwaseun"}},
    {key:"hiring-plan",title:"Hiring plan Q1",daysAgo:95,capture:"desktop",people:{Noor:"mic",Tamsin:"system",Lowell:"system"},attendees:staff("Noor","Tamsin","Lowell"),script:`
Tamsin: The ask for the first quarter is one product manager, two engineers and one designer.
Lowell: I can fund the engineers and the designer now. The product manager I need to see the revenue plan first.
Tamsin: Fair. We will come back on the product manager.
Noor: I will write the job descriptions for the designer this week.
`,summary:`# First-quarter hiring\n- Requested: one product manager, two engineers, one designer.\n- Funded now: two engineers and one designer. The product manager role waits on the revenue plan.\n\n# Action items\n- Noor: designer job description this week.`,
      headline:{text:"First-quarter hiring: two engineers and a designer funded.",must:"hiring|engineers|designer"}},

    // ------------------------------------------------------------------ offsite
    {key:"offsite-1",title:"Leadership offsite – day 1",daysAgo:68,capture:"ios",people:{Noor:"mic",Tamsin:"mic",Jorge:"mic",Aditi:"mic",Lowell:"mic"},attendees:staff("Noor","Tamsin","Jorge","Aditi","Lowell"),pad:{turns:460,topics:["the retail vertical","logistics","healthcare","the north star","positioning","the sales motion","churn","the roadmap"]},script:`
Tamsin: Day one is strategy. Where do we play?
Aditi: Logistics and healthcare are where we win. Retail has been eighteen months of pilots and no revenue.
[[pad]]
Tamsin: I am going to call it. We stop pursuing the retail vertical. Logistics and healthcare only.
Lowell: Supported. The retail pipeline is under three percent of bookings.
Tamsin: Second decision. Our north star metric is on-time completion rate, the share of planned stops completed inside their window.
Jorge: We can measure that today, it is already in the data.
[[pad]]
Tamsin: Tomorrow is planning. Come with your top three.
`,summary:`# Strategy decisions\n- Stop pursuing the retail vertical; focus on logistics and healthcare.\n- North star metric: on-time completion rate.`,
      headline:{text:"Offsite day 1: stop pursuing retail; north star is on-time completion rate.",must:"retail|north star|on.time"}},
    {key:"offsite-2",title:"Leadership offsite – day 2",daysAgo:67,capture:"ios",people:{Noor:"mic",Tamsin:"mic",Jorge:"mic",Aditi:"mic",Lowell:"mic"},attendees:staff("Noor","Tamsin","Jorge","Aditi","Lowell"),pad:{turns:440,topics:["Export","single sign-on","enterprise deals","capacity","the advisory board","pricing","the calendar","hiring"]},script:`
Tamsin: Planning day. Noor, your top three.
Noor: Export, single sign-on, and a customer advisory board.
Tamsin: Export has to launch this quarter. I do not want to hear a new date again.
Jorge: Understood. The committed date is still ${L1}.
[[pad]]
Aditi: Single sign-on is the top blocker in enterprise deals. Four deals are waiting on it.
Tamsin: Then it is priority two after Export.
[[pad]]
Tamsin: Noor owns the customer advisory board. First session in the new year.
Noor: I will plan it.
`,summary:`# Planning decisions\n- Export must launch this quarter (committed date at the time: ${L1}).\n- Single sign-on is priority two; four enterprise deals are waiting on it.\n- Noor owns the customer advisory board; first session in the new year.`,
      notes:"my top 3: Export, SSO, CAB",headline:{text:"Offsite day 2: Export must launch this quarter; single sign-on is priority two.",must:"Export|single sign.on|SSO"}},

    // --------------------------------------------------------------------- misc
    {key:"board-prep",title:"Board prep",daysAgo:110,capture:"desktop",people:{Noor:"mic",Tamsin:"system",Lowell:"system"},attendees:staff("Noor","Tamsin","Lowell"),script:`
Lowell: The headline numbers. Annual recurring revenue is six point two million dollars. Net revenue retention is one hundred and four percent.
Tamsin: The board will ask why retention is not higher.
Lowell: Small account churn. It is the same story as last quarter.
Noor: I can add a slide on Export as the retention play.
Tamsin: One slide, no more.
`,summary:`# Numbers for the board\n- Annual recurring revenue: $6.2 million.\n- Net revenue retention: 104 percent; held down by small-account churn.\n\n# Action items\n- Noor: one slide on Export as the retention play.`,
      headline:{text:"Board prep: $6.2 million annual recurring revenue, 104 percent net revenue retention.",must:"6\\.2|104|board"}},
    {key:"roadmap-review",title:"Quarterly roadmap review",daysAgo:100,capture:"desktop",people:{Noor:"mic",Tamsin:"system",Jorge:"system",Wren:"system",Aditi:"system"},attendees:staff("Noor","Tamsin","Jorge","Wren","Aditi"),pad:{turns:480,topics:["Export","offline mode","single sign-on","the route editor","onboarding","integrations","mobile","reporting"]},script:`
Noor: Three themes this quarter. Data out, which is Export. Enterprise readiness, which is single sign-on. And first-week success, which is onboarding.
[[pad]]
Aditi: What about the scanner integration? Two logistics prospects asked.
Noor: Not this quarter. We will look at barcode scanners after Export ships.
[[pad]]
Tamsin: I want one explicit trade-off written down. If Export and offline mode collide, Export wins.
Noor: Written down. Export wins over offline mode.
`,summary:`# Themes\n- Data out (Export), enterprise readiness (single sign-on), first-week success (onboarding).\n\n# Trade-offs\n- If Export and offline mode collide, Export wins.\n- Barcode scanner integration: not this quarter; revisit after Export ships.`,
      headline:{text:"Roadmap review: Export, single sign-on and onboarding are the quarter's themes.",must:"roadmap|themes|Export"}},
    {key:"design-critique",title:"Design critique: onboarding",daysAgo:15,capture:"desktop",people:{Noor:"mic",Wren:"system",Mirela:"system"},attendees:staff("Noor","Wren","Mirela"),script:`
Wren: The new onboarding has five steps. Import addresses, add drivers, set depots, plan a first route, invite a teammate.
Mirela: Customers skip the depot step and then routing looks broken.
Wren: I could make depots required.
Noor: Yes. Make the depot step required. And move invite a teammate to the end, as optional.
Wren: Done. I will have the revised flow by Monday.
`,summary:`# Onboarding flow\n- Five steps: import addresses, add drivers, set depots, plan a first route, invite a teammate.\n\n# Decisions\n- The depot step becomes required.\n- "Invite a teammate" moves to the end and becomes optional.\n\n# Action items\n- Wren: revised flow by Monday.`,
      headline:{text:"Onboarding critique: depot step required; invite a teammate made optional.",must:"depot|onboarding"}},
    {key:"vendor-mapleton",title:"Vendor call: Mapleton Analytics",daysAgo:44,capture:"desktop",people:{Noor:"mic",Jorge:"mic",Corinne:"system"},attendees:[...staff("Noor","Jorge"),["Corinne Vale","corinne.vale@mapleton.example"]],script:`
Corinne: Mapleton gives you embedded dashboards with a single script tag. Pricing for your volume is thirty six thousand dollars a year.
Jorge: Where is the data processed?
Corinne: In our Virginia region. We do not have a European region yet.
Noor: And contract length?
Corinne: Three years minimum, that is how we keep the price down.
Noor: Thank you, Corinne. We will discuss and come back to you.
Jorge: Now that she has dropped off. No European data residency is a non-starter for Harrowgate and anyone like them.
Noor: And I am not signing three years for an analytics vendor. We pass on Mapleton.
`,summary:`# Mapleton Analytics\n- Embedded dashboards; quoted $36,000 a year.\n- Data processed in Virginia only; three-year minimum term.\n\n# Decision\n- Pass. No European data residency, and the three-year minimum is too long.`,
      headline:{text:"Passed on Mapleton Analytics: no European data residency and a three-year minimum.",must:"Mapleton"}},
    {key:"legal-dpa",title:"Legal sync: DPA template",daysAgo:28,capture:"desktop",people:{Noor:"mic",Farid:"system"},attendees:staff("Noor","Farid"),script:`
Farid: The data processing agreement template needs two changes. The sub-processor list is out of date, and we should give thirty days notice before adding a new one.
Noor: Thirty days is fine.
Farid: Harrowgate wants a custom clause on breach notification, twenty four hours instead of seventy two.
Noor: Can we do that?
Farid: I would offer forty eight. I will propose it to their counsel.
`,summary:`# Data processing agreement\n- Update the sub-processor list; give thirty days' notice before adding a sub-processor.\n- Harrowgate asked for breach notification within twenty-four hours; Farid will propose forty-eight.`,
      headline:{text:"Legal sync: thirty days' notice for new sub-processors; Harrowgate breach-notification clause.",must:"sub.processor|breach|data processing"}},
    {key:"marketing-export",title:"Marketing launch plan – Export",daysAgo:22,capture:"desktop",people:{Noor:"mic",Odette:"system"},attendees:staff("Noor","Odette"),script:`
Odette: For the Export launch I have three pieces. A launch email, a webinar, and a customer story.
Noor: Who is the customer story?
Odette: I would like Pinecrest Health, if their rollout is approved in time.
Noor: Ask Aditi before you approach them.
Odette: Will do. The webinar is one week after launch. One naming question, do we call it Export or Data Export?
Noor: Export. Keep it short.
`,summary:`# Export launch marketing\n- Launch email, a webinar one week after launch, and a customer story (Pinecrest Health if their rollout is approved; check with Aditi first).\n- Name: "Export", not "Data Export".`,
      headline:{text:"Export launch marketing: email, webinar and a Pinecrest customer story.",must:"webinar|launch email|customer story"}},
    {key:"support-escalations",title:"Support escalation review",daysAgo:8,capture:"desktop",people:{Noor:"mic",Mirela:"system"},attendees:staff("Noor","Mirela"),script:`
Mirela: Three escalations this week. A route that would not save at a customer with nine hundred stops, a billing address bug, and one customer still seeing duplicate stops.
Noor: Still? After the idempotency fix?
Mirela: It was old data from before the fix. I cleaned it up by hand.
Noor: Good. Can we add a macro for the nine hundred stop limit?
Mirela: Yes, I will add a macro explaining the limit of seven hundred and fifty stops per route.
`,summary:`# Escalations this week\n- A route with 900 stops would not save (limit is 750 stops per route).\n- Billing address bug.\n- One customer still saw duplicate stops: old data from before the idempotency fix, cleaned up by hand.\n\n# Action items\n- Mirela: add a support macro explaining the 750-stop limit.`,
      headline:{text:"Support escalations: 750-stop route limit, billing address bug, leftover duplicate stops.",must:"escalation|750|duplicate stops|macro"}},
    // Typed notes only: no recording was made.
    {key:"allhands-prep",title:"All-hands Q&A prep",daysAgo:6,capture:"desktop",people:{Noor:"mic"},script:"",notes:`Likely questions for the all-hands\n- Why did Export move again? -> audit log for healthcare customers, launch ${L3}\n- Are we still doing retail? -> no, decided at the offsite\n- Is there a hiring freeze? -> no. 2 engineers + 1 designer funded\n\nTODO: ask Tamsin whether I can mention the price increase`,
      headline:{text:"Prepared answers for the all-hands questions.",must:"all.hands"}},
    // Transcript only: never summarised, no typed notes.
    {key:"coffee-bastien",title:"Coffee chat with Bastien",daysAgo:2,capture:"ios",people:{Noor:"mic",Bastien:"mic"},script:`
Bastien: So how is life at Brindle?
Noor: Busy. I am up for promotion in January and I am not sure how to make the case.
Bastien: Two pieces of advice. First, ask for the staff title explicitly, do not wait for someone to offer it.
Noor: That feels uncomfortable.
Bastien: It always does. Second, read a book called The Quiet Roadmap. It is about saying no without losing the room.
Noor: The Quiet Roadmap. I will order it.
Bastien: And call me after calibration, I want to hear how it went.
`,headline:{text:"Bastien advised asking for the staff title and recommended a book.",must:"Bastien"}},
    {key:"untitled-scanner",title:"Untitled meeting",daysAgo:9,capture:"desktop",people:{Noor:"mic",Rafferty:"system"},script:`
Rafferty: Thanks for taking the call. I am with Tallis Devices, we make the handheld barcode scanners your logistics customers already use.
Noor: What does an integration look like?
Rafferty: We have a software kit, but I have to be upfront, it supports Android only today.
Noor: No iOS?
Rafferty: Not until late next year. For a pilot we charge twelve hundred dollars per device.
Noor: That is steep for a pilot.
Rafferty: I can send you two sample units next week at no charge so your engineers can try the kit.
Noor: Please do. Send the two sample units and we will take a look.
`,headline:{text:"Call with Tallis Devices about a barcode scanner integration (Android only).",must:"scanner|Tallis"}},
    // Typed notes only, and an unhelpful title.
    {key:"untitled-hallway",title:"Untitled meeting",daysAgo:36,capture:"desktop",people:{Noor:"mic"},script:"",notes:`hallway w/ Lowell + Wren\n- Lowell: credit policy = 1 month max, he signs each one\n- Wren moving to 4 days/week from January\n- need to replan onboarding around Wren's days`,
      headline:{text:"Hallway notes: Lowell signs each credit; Wren moves to four days a week from January.",must:"Wren|credit"}},
    {key:"partner-fennimore",title:"Partner intro: Fennimore Consulting",daysAgo:130,capture:"desktop",people:{Noor:"mic",Aditi:"mic",Alasdair:"system"},attendees:[...staff("Noor","Aditi"),["Alasdair Boyd","alasdair.boyd@fennimore.example"]],script:`
Alasdair: Fennimore implements dispatch software for mid-sized carriers. We would bring you deals and do the rollout.
Aditi: What do you expect in return?
Alasdair: A referral fee. Fifteen percent of first-year revenue.
Aditi: We usually pay ten.
Alasdair: Then we have a gap. Let us both think about it.
Noor: We will come back to you. Nothing is agreed today.
`,summary:`# Fennimore Consulting\n- Implementation partner for mid-sized carriers; would refer deals and run rollouts.\n- They asked for a referral fee of fifteen percent of first-year revenue; we countered with ten. Left open.`,
      headline:{text:"Fennimore Consulting asked for a fifteen percent referral fee; left open.",must:"Fennimore"}},
    {key:"accessibility",title:"Accessibility audit readout",daysAgo:70,capture:"desktop",people:{Noor:"mic",Wren:"system",Jorge:"system"},attendees:staff("Noor","Wren","Jorge"),script:`
Wren: The audit found twenty three issues. Five are critical, mostly keyboard traps in the route editor.
Jorge: The five critical ones we can fix in two sprints.
Noor: Then that is the commitment. Critical accessibility issues fixed within two sprints, the rest go on the backlog.
Wren: I will file them tonight.
`,summary:`# Audit results\n- Twenty-three issues, five critical (mostly keyboard traps in the route editor).\n\n# Decision\n- Fix the five critical issues within two sprints; the rest go on the backlog.`,
      headline:{text:"Accessibility audit: 23 issues, five critical to be fixed within two sprints.",must:"accessibility|critical"}},
    {key:"data-retention",title:"Data retention policy discussion",daysAgo:48,capture:"desktop",people:{Noor:"mic",Farid:"system",Jorge:"system"},attendees:staff("Noor","Farid","Jorge"),script:`
Farid: We need a default. I propose twenty four months for route history.
Jorge: Location pings are the expensive part. Thirteen months is plenty for those.
Noor: So route history twenty four months, location pings thirteen months, and customers can shorten either.
Farid: Agreed. I will write it into the policy.
`,summary:`# Retention defaults\n- Route history: 24 months.\n- Location pings: 13 months.\n- Customers can shorten either.\n\n# Action items\n- Farid: write it into the policy.`,
      headline:{text:"Retention defaults: 24 months for route history, 13 months for location pings.",must:"retention|24 months|13 months"}},
    {key:"cab-planning",title:"Customer advisory board planning",daysAgo:92,capture:"desktop",people:{Noor:"mic",Aditi:"system",Jorge:"system"},attendees:staff("Noor","Aditi","Jorge"),script:`
Aditi: I would keep the first advisory board small. Twelve customers, half logistics, half healthcare.
Jorge: I can present the roadmap, but I would rather not run the session.
Noor: Twelve customers, mixed. We will aim for the new year.
Aditi: I will nominate six from my accounts.
`,summary:`# Customer advisory board\n- First session: twelve customers, half logistics and half healthcare, in the new year.\n\n# Action items\n- Aditi: nominate six customers from her accounts.`,
      headline:{text:"Advisory board planned for twelve customers in the new year.",must:"advisory board"}},
    {key:"yearend-kickoff",title:"Year-end planning kickoff",daysAgo:1,capture:"desktop",people:{Noor:"mic",Tamsin:"system",Lowell:"system",Jorge:"system"},attendees:staff("Noor","Tamsin","Lowell","Jorge"),script:`
Tamsin: Year-end planning. Three deadlines. Headcount requests by ${d(-6)}. Budget drafts by ${d(-13)}. Final plan to the board the week after.
Lowell: I need headcount in my template, not in a slide.
Noor: I will submit my headcount request by ${d(-6)}, in the template.
Jorge: I will need Noor's roadmap draft before I can size engineering.
Noor: You will have the roadmap draft by ${d(-2)}.
Tamsin: Good. Do not be late on these.
`,summary:`# Deadlines\n- Headcount requests by ${d(-6)} (in Lowell's template).\n- Budget drafts by ${d(-13)}.\n- Final plan to the board the week after.\n\n# Action items\n- Noor: headcount request by ${d(-6)}; roadmap draft to Jorge by ${d(-2)}.`,
      headline:{text:"Year-end planning deadlines set; Noor owes a headcount request and a roadmap draft.",must:"headcount|year.end|roadmap draft"}},
    {key:"pipeline-1",title:"Sales pipeline review",daysAgo:54,capture:"desktop",people:{Noor:"mic",Aditi:"system"},attendees:staff("Noor","Aditi"),script:`
Aditi: Pipeline. Harrowgate is closed. Pinecrest is at stage two, they have agreed to a pilot.
Noor: Anything stuck?
Aditi: Four enterprise deals are waiting on single sign-on. The biggest is Calloway Freight at one hundred and eighty thousand dollars a year.
Noor: I will get you a date for single sign-on.
`,summary:`# Pipeline\n- Harrowgate closed. Pinecrest at stage two (pilot agreed).\n- Four enterprise deals blocked on single sign-on; the largest is Calloway Freight ($180,000 a year).`,
      headline:{text:"Pipeline: four enterprise deals blocked on single sign-on, largest is Calloway Freight.",must:"pipeline|Calloway"}},
    {key:"pipeline-2",title:"Sales pipeline review",daysAgo:27,capture:"desktop",people:{Noor:"mic",Aditi:"system"},attendees:staff("Noor","Aditi"),script:`
Aditi: Update. Pinecrest moved to stage three after the pilot results. Calloway Freight is still waiting on single sign-on, and they have started talking to a competitor.
Noor: How long do we have?
Aditi: They want an answer before the end of the quarter.
Noor: Okta is ready now. Are they on Okta?
Aditi: They are. I will offer them the Okta beta this week.
`,summary:`# Pipeline\n- Pinecrest at stage three after the pilot results.\n- Calloway Freight is still waiting on single sign-on and is talking to a competitor; wants an answer before the end of the quarter.\n\n# Action items\n- Aditi: offer Calloway the Okta beta this week.`,
      headline:{text:"Pipeline: Pinecrest at stage three; Calloway Freight at risk, to be offered the Okta beta.",must:"pipeline|Calloway"}},
    {key:"export-beta-feedback",title:"Export beta feedback",daysAgo:13,capture:"desktop",people:{Noor:"mic",Jorge:"system",Mirela:"system"},attendees:staff("Noor","Jorge","Mirela"),script:`
Mirela: Feedback from the five beta customers. Four are happy. The fifth wants scheduled exports, a file every night.
Jorge: Scheduled exports are not in the launch scope.
Noor: Agreed, not for launch. Put scheduled exports on the list for the release after.
Mirela: One bug, timestamps came out in universal time and one customer expected local time.
Jorge: That is a real bug. I will fix the time zone before launch.
`,summary:`# Beta feedback\n- Four of five beta customers are happy.\n- One wants scheduled (nightly) exports: not in launch scope; planned for the following release.\n- Bug: timestamps exported in UTC instead of local time. Jorge to fix before launch.`,
      headline:{text:"Export beta: four of five customers happy; scheduled exports deferred; time-zone bug to fix.",must:"beta|scheduled exports|time.?zone"}},
    {key:"naming-export",title:"Naming brainstorm",daysAgo:63,capture:"desktop",people:{Noor:"mic",Wren:"system",Odette:"system"},attendees:staff("Noor","Wren","Odette"),script:`
Odette: We need a name for the data export feature. Wren, you said you had an idea?
Wren: I had Harbor. Data leaves the harbor.
Odette: Harbor is taken, there is a container registry with that name and legal will not like it.
Wren: Fair. Then Outflow?
Noor: Outflow sounds like plumbing. I vote we just call it Export.
Odette: Plain, but nobody will misunderstand it.
Noor: Export it is.
`,summary:`# Feature name\n- Several names were considered and rejected.\n- Decision: call the feature "Export".`,
      headline:{text:"Naming brainstorm: the feature will simply be called Export.",must:"name|Export"}},
    {key:"sso-kickoff",title:"SSO kickoff",daysAgo:83,capture:"desktop",people:{Noor:"mic",Jorge:"system",Wren:"system"},attendees:staff("Noor","Jorge","Wren"),script:`
Jorge: For single sign-on I propose we buy rather than build. A vendor library for the protocol, our own settings screens.
Noor: Cost?
Jorge: About nine thousand dollars a year for the library.
Noor: Approved. Buy the library, build the settings ourselves.
Wren: I will start on the settings screens next week.
`,summary:`# Single sign-on approach\n- Decision: buy a vendor library for the protocol (about $9,000 a year) and build the settings screens in-house.\n\n# Action items\n- Wren: start the settings screens next week.`,
      headline:{text:"Single sign-on kickoff: buy the protocol library, build settings in-house.",must:"single sign.on|SSO|library"}},
    {key:"budget-checkin",title:"Budget check-in with Lowell",daysAgo:16,capture:"desktop",people:{Noor:"mic",Lowell:"system"},attendees:staff("Noor","Lowell"),script:`
Lowell: You are at ninety one percent of your tooling budget with six weeks to go.
Noor: The research tool renewal is what did it.
Lowell: I can move four thousand dollars from the travel line if you need it.
Noor: Yes please. Move four thousand from travel to tooling.
Lowell: Done. And the Harrowgate credit is approved, one month. Tell Ilse.
Noor: Thank you, I will.
`,summary:`# Budget\n- Tooling budget is 91 percent spent with six weeks to go.\n- Lowell will move $4,000 from the travel line to tooling.\n\n# Harrowgate\n- The one-month service credit is approved.`,
      headline:{text:"Budget check-in: $4,000 moved from travel to tooling; Harrowgate credit approved.",must:"budget|tooling|credit"}},
    {key:"research-readout",title:"Dispatcher research readout",daysAgo:39,capture:"desktop",people:{Noor:"mic",Wren:"system"},attendees:staff("Noor","Wren"),script:`
Wren: I interviewed eight dispatchers. The headline is that they do not trust automatic re-planning. Six of eight override it at least once a day.
Noor: Why?
Wren: They cannot see why the plan changed. They want an explanation next to every change.
Noor: So the opportunity is explanations, not better optimisation.
Wren: Exactly. I will write it up with clips.
`,summary:`# Findings\n- Eight dispatchers interviewed. Six of eight override automatic re-planning at least once a day.\n- They do not trust it because they cannot see why the plan changed.\n\n# Implication\n- Show an explanation next to every change.`,
      headline:{text:"Research: dispatchers override automatic re-planning because they cannot see why it changed.",must:"dispatchers|re.planning|research"}},
    {key:"onboarding-metrics",title:"Onboarding metrics review",daysAgo:29,capture:"desktop",people:{Noor:"mic",Wren:"system",Mirela:"system"},attendees:staff("Noor","Wren","Mirela"),script:`
Mirela: Activation is at forty two percent. That is accounts that plan a first route inside seven days.
Wren: The checklist should move that.
Noor: What is the target?
Wren: Fifty five percent by the end of next quarter.
Noor: Fifty five percent activation by the end of next quarter. Let us review monthly.
`,summary:`# Activation\n- 42 percent of new accounts plan a first route within seven days.\n- Target: 55 percent by the end of next quarter; review monthly.`,
      headline:{text:"Onboarding activation is 42 percent; target 55 percent by end of next quarter.",must:"activation|42|55"}},
  ]
}

const expand=(meeting:LargeMeeting):string=>{
  if(!meeting.pad)return meeting.script
  const speakers=Object.keys(meeting.people),parts=meeting.script.split("[[pad]]"),each=Math.ceil(meeting.pad.turns/Math.max(1,parts.length-1))
  return parts.map((part,index)=>index<parts.length-1?`${part}\n${filler(speakers,meeting.pad!.topics,each,index*11+meeting.key.length)}\n`:part).join("")
}
export const largeNoteId=(key:string)=>stableUuid("large-workspace","note",key)
/** Real-name transcript for the judge; the model under test never sees names it was not told. */
export function largeTruth(meeting:LargeMeeting):string {
  const body=expand(meeting).split("\n").map(line=>line.trim()).filter(Boolean).join("\n")
  return [meeting.notes?`[typed notes]\n${meeting.notes}`:"",meeting.summary?`[generated summary]\n${meeting.summary}`:"",body?`[transcript]\n${body}`:""].filter(Boolean).join("\n\n")
}
export interface LargeWorkspace {notes:NoteDTO[];contexts:Map<string,MeetingContextDTO>;meetings:LargeMeeting[];byKey:Map<string,LargeMeeting>}
export function largeWorkspace(now:Date):LargeWorkspace {
  const meetings=largeMeetings(now),contexts=new Map<string,MeetingContextDTO>(),keys=new Set<string>()
  const notes=meetings.map((meeting):NoteDTO=>{
    if(keys.has(meeting.key))throw new Error(`Duplicate large-workspace meeting ${meeting.key}`)
    keys.add(meeting.key)
    const id=largeNoteId(meeting.key),date=day(now,meeting.daysAgo).iso,stamp=`${date}T15:00:00.000Z`
    const script=expand(meeting)
    const built=script.trim()?compileFixture({id:`large-${meeting.key}`,title:meeting.title,kind:"large workspace",daysAgo:meeting.daysAgo,capture:meeting.capture,wpm:160,people:meeting.people,script,notes:{sparse:"",detailed:""},gold:{decisions:[],actionItems:[],facts:[],notSaid:[]},questions:[]} satisfies Fixture):null
    if(meeting.attendees){
      const people=meeting.attendees.map(([name,address],index)=>{const [given,...rest]=name.split(" ");return {id:stableUuid("large-attendee",meeting.key,index),displayName:name,givenName:given??null,familyName:rest.join(" ")||null,email:address,nameSource:"calendar",isSelf:index===0,isOrganizer:index===0,isResource:false,responseStatus:"accepted"}})
      contexts.set(id,{eventId:stableUuid("large-event",meeting.key),provider:"ics",title:meeting.title,calendarName:"Work",startAt:`${date}T15:00:00Z`,endAt:`${date}T15:45:00Z`,allDay:false,timeZone:"UTC",location:null,meetingUrl:null,eventUrl:null,organizer:people[0]??null,participants:people} as unknown as MeetingContextDTO)
    }
    return {id,title:meeting.title,body:meeting.notes??"",enhanced:meeting.summary??"",date,duration:built?.duration??"",createdAt:stamp,updatedAt:stamp,transcriptSegments:built?.segments??[],recordingState:built?"stopped":"idle",generationState:meeting.summary?"ready":"idle",transcriptRevision:built?1:0,generatedRevision:meeting.summary?1:null}
  })
  return {notes,contexts,meetings,byKey:new Map(meetings.map(meeting=>[meeting.key,meeting]))}
}
