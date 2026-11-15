import type {Fixture} from "../lib/types"
import {near} from "../lib/transcript"

/** Customer discovery call. Fictional vendor (Brightwell), prospect (Kestrel Freight) and people. */
export const discovery:Fixture={
  id:"discovery",title:"Kestrel Freight discovery call",kind:"customer discovery call",daysAgo:6,capture:"desktop",wpm:150,fast:true,
  people:{Tessa:"mic",Marcus:"system",Joon:"system"},
  attendees:[{name:"Tessa Lindgren",email:"tessa@brightwell.example",self:true},{name:"Marcus Oyelaran",email:"marcus@kestrelfreight.example"},{name:"Joon Park",email:"joon@kestrelfreight.example"}],
  script:`
Tessa: Hi Marcus, hi Joon, thanks for making the time. I've got forty five minutes blocked but we can stop whenever. Would it help if I start with a quick overview, or should we jump straight into how you run the docks today?
Marcus: Let's start with us, I think. You'll get more out of it.
Tessa: Perfect. So tell me how dock scheduling works at Kestrel right now.
Marcus: Okay. We run three warehouses, Tacoma, Reno and Boise. About a hundred and forty dock workers across the three. Scheduling is a whiteboard at each site plus one shared spreadsheet that the shift leads update, in theory, at the end of each shift.
Tessa: In theory.
Marcus: In theory. In practice the spreadsheet is a day behind, and the carriers call the site directly, so we double book dock doors. Last month Tacoma had two trucks assigned to door six at the same time on eleven separate occasions.
Tessa: What does a double booking cost you?
Marcus: Detention fees if the truck waits more than two hours, and honestly the bigger cost is my shift leads. I had my analyst add it up, we spend roughly twelve hours a week across the sites just reconciling the whiteboard against the spreadsheet.
Tessa: Twelve hours a week, got it. Have you tried software for this before?
Marcus: We ran a trial of Dockwise last year. It died in about three weeks.
Tessa: What killed it?
Marcus: No offline mode. The Reno yard has basically no signal past door ten. The guys would walk out there, the app would spin, and they went back to the whiteboard. After that nobody trusted it.
Joon: And from my side, it couldn't talk to our warehouse system. We're on Stratus WMS, and Dockwise wanted us to export a file every night. That's not an integration, that's a chore.
Tessa: That's really useful. So if I play it back, the two things that have to be true are, it works with no signal, and it talks to Stratus directly.
Marcus: Those are the two. Everything else is nice to have.
Tessa: Joon, what about sign on? A lot of IT teams ask us for single sign on first.
Joon: We use Okta. I'd like it, but I'm not going to block a pilot on it. If it's on your roadmap for the full rollout that's fine.
Tessa: It's available today on the business plan, so that shouldn't be an issue. On offline, the mobile app queues changes on the device and syncs when it sees a connection again. I can send you the technical spec for how conflicts are resolved, because that's usually the next question.
Joon: Yes, please. That is exactly my next question. And I'll need your security questionnaire answers before anything touches production data.
Tessa: I'll send both. I can have the questionnaire and the offline spec to you by Friday.
Joon: Friday's fine. I can send you the Stratus API documentation, we have a sandbox tenant you can point at.
Tessa: That would be great. Marcus, can I ask about timing and budget, so I don't waste your time with the wrong proposal?
Marcus: Sure. Timing, I want a decision by the end of the first quarter. Our peak starts in May and I'm not changing how the docks run during peak.
Tessa: Understood. And budget?
Marcus: I don't have an approved number. If I had to guess what Renata would sign, that's our finance chief, it's somewhere up to sixty thousand a year. Above that it becomes a board conversation and it won't happen this year.
Tessa: So around sixty thousand annually as a ceiling, not approved yet, and Renata signs.
Marcus: Correct. And she'll want to see it working at one site before she signs anything for three.
Tessa: That lines up with how we usually start. Our list price is thirty eight dollars per user per month. For a hundred and forty users that's a bit over sixty three thousand a year at list, so we'd need to look at volume pricing to fit under your ceiling, and I think we can. What I'd suggest is a paid pilot at one site first. Ninety days, up to twenty five users, for a flat four thousand five hundred dollars, credited against the first year if you go ahead.
Marcus: Which site?
Tessa: You tell me. Where is the pain worst?
Marcus: Tacoma. It has the most doors and the most double bookings. Reno has the signal problem though.
Joon: I'd pilot in Tacoma and run a two day offline test in Reno. If it survives the Reno yard I'm convinced.
Marcus: I like that. Tacoma pilot, Reno offline test.
Tessa: Great. I'll write it up that way. To be clear, I'm proposing those pilot terms, you're not agreeing to them on this call.
Marcus: Right, I can't agree to anything without Renata. But I think it's reasonable.
Tessa: Is there anything that worries you about this, other than the two requirements?
Marcus: Change management. My shift leads have used a whiteboard for fifteen years. If the training is a two hour webinar they will ignore it. I need someone on site for the first week.
Tessa: We do on site onboarding for pilots. I'd want to confirm how many days with our customer success lead before I promise a full week.
Marcus: Okay. That matters to me more than the price, honestly.
Tessa: Noted. What does the next step look like on your side?
Marcus: I'd like Renata to see a demo. She's free on the nineteenth, I think, in the afternoon. Let me confirm with her and come back to you.
Tessa: The nineteenth works for me. I'll hold the afternoon and wait for your confirmation. Before then I'll send the questionnaire, the offline spec, and a one page pilot proposal.
Joon: And I'll send the API docs and sandbox credentials this week.
Marcus: One more thing. Are you working with anyone else in freight? Renata will ask for references.
Tessa: We have two logistics customers I can connect you with once we're a bit further along. I'd rather ask them first than give you names on a call.
Marcus: Fair enough.
Tessa: Thank you both, this was really helpful. Talk soon.
`,
  notes:{
    sparse:`Kestrel - Marcus (ops), Joon (IT)\n3 warehouses\noffline!!\nStratus WMS\nbudget ~60k?? not approved\nimpression: Marcus cares more about onsite training than price`,
    detailed:`# Current process\n- 3 warehouses: Tacoma, Reno, Boise - ~140 dock workers\n- whiteboard + shared spreadsheet, a day behind\n- double bookings (Tacoma door 6)\n\n# Requirements\n- MUST: offline mode (Reno yard no signal)\n- MUST: direct Stratus WMS integration\n- Okta SSO - nice to have\n\n# Commercial\n- decision by end of Q1, peak starts May\n- ceiling ~60k/yr, Renata (finance) signs - NOT approved\n- I proposed pilot: 90 days, 25 users, $4,500\n\n# Concerns\n- change mgmt / on-site training (matters more than price - my read)\n\n# Next steps\n- me: questionnaire + offline spec by Fri, pilot one-pager\n- Joon: API docs + sandbox\n- demo w/ Renata 19th? Marcus to confirm`,
  },
  gold:{
    decisions:[
      {id:"pilot-shape",text:"Shared direction (not a commitment): pilot at Tacoma with a two-day offline test in Reno.",must:["Tacoma","Reno"]},
    ],
    actionItems:[
      {id:"questionnaire",owner:"Tessa",due:"Friday",text:"Tessa sends the security questionnaire answers and the offline/conflict-resolution spec by Friday.",must:["questionnaire","Friday"],ownerMust:near("Tessa","questionnaire|spec")},
      {id:"pilot-proposal",owner:"Tessa",text:"Tessa sends a one-page pilot proposal.",must:["proposal|one.page|write.?up"],ownerMust:near("Tessa","proposal|one.page")},
      {id:"api-docs",owner:"Joon",due:"this week",text:"Joon sends the Stratus API documentation and sandbox credentials.",must:["API doc|sandbox"],ownerMust:near("Joon","API|sandbox")},
      {id:"confirm-demo",owner:"Marcus",text:"Marcus confirms with Renata whether she can join a demo on the 19th.",must:["19th|nineteenth","Renata"],ownerMust:near("Marcus","confirm|Renata|19th|nineteenth")},
      {id:"onsite-days",owner:"Tessa",text:"Tessa confirms with customer success how many on-site onboarding days can be offered.",must:["on.?site"],ownerMust:near("Tessa","on.?site|customer success|onboarding")},
    ],
    facts:[
      {id:"sites",text:"Kestrel runs three warehouses (Tacoma, Reno, Boise) with about 140 dock workers.",must:["three|\\b3\\b","140|hundred and forty"]},
      {id:"reconcile",text:"About twelve hours a week are spent reconciling the whiteboard and the spreadsheet.",must:["twelve hours|12 hours"]},
      {id:"dockwise",text:"A Dockwise trial last year failed in about three weeks: no offline mode and no direct Stratus WMS integration.",must:["Dockwise","offline"]},
      {id:"requirements",text:"Two hard requirements: works offline, integrates directly with Stratus WMS. Okta SSO is nice to have, not a blocker.",must:["offline","Stratus"]},
      {id:"budget",text:"Budget ceiling around $60,000 a year, not approved; Renata (finance chief) must sign.",must:["60,?000|sixty thousand|60k","not (?:yet )?approved|unapproved|no approved|hasn't been approved|ceiling|up to|guess|estimate|sign"]},
      {id:"timing",text:"Decision wanted by the end of the first quarter because peak starts in May.",must:["first quarter|Q1","May|peak"]},
      {id:"pricing",text:"List price $38 per user per month; proposed paid pilot: 90 days, up to 25 users, $4,500 flat, credited against year one.",must:["38","4,?500"]},
      {id:"change-mgmt",text:"Marcus's main worry is change management; he wants someone on site for the first week.",must:["on.?site|change management|training"]},
    ],
    notSaid:[
      {id:"pilot-agreed",claim:"Kestrel agreed to or signed the pilot (it was only proposed).",forbid:["(?:Kestrel|Marcus|they) (?:agreed|committed|signed|approved)[^.\\n]{0,40}pilot","pilot (?:was|is|has been) (?:agreed|approved|signed|confirmed)"]},
      {id:"sso-blocker",claim:"SSO is a blocker or hard requirement.",forbid:["(?:SSO|single sign.on|Okta)[^.\\n]{0,30}(?:is|as) (?:a )?(?:blocker|must|required|hard requirement)"]},
      {id:"budget-approved",claim:"The budget is approved.",forbid:["budget (?:is|was|has been) approved","approved budget of"]},
      {id:"full-week",claim:"Tessa promised a full week on site.",forbid:["(?:Tessa|Brightwell|we) (?:promised|committed to|guaranteed)[^.\\n]{0,40}(?:week|on.?site)"]},
      {id:"references-named",claim:"Named reference customers were given.",forbid:["reference customers? (?:are|include|such as)"]},
    ],
  },
  questions:[
    {id:"why-dockwise-failed",kind:"factual",fast:true,question:"Why did their Dockwise trial fail?",reference:"No offline mode (the Reno yard has almost no signal past door ten, so workers went back to the whiteboard) and no real integration with Stratus WMS (it needed a nightly file export). It ended after about three weeks.",must:["offline|signal","Stratus|integrat|export"],support:["No offline mode. The Reno yard has basically no signal past door ten","it couldn't talk to our warehouse system"]},
    {id:"budget",kind:"citation",fast:true,question:"What is their budget and who approves it? Cite where it was said.",reference:"No approved number. Marcus guesses Renata, the finance chief, would sign up to about sixty thousand dollars a year; above that it becomes a board conversation.",must:["60,?000|sixty thousand|60k","Renata"],forbid:["budget (?:is|was|has been) approved"],support:["it's somewhere up to sixty thousand a year"]},
    {id:"pilot-terms",kind:"factual",question:"What pilot terms did I propose?",reference:"A paid pilot at one site: ninety days, up to twenty five users, a flat $4,500 credited against the first year. Proposed only; Marcus cannot agree without Renata. Direction was a Tacoma pilot plus a two-day offline test in Reno.",must:["ninety|90","25|twenty.five","4,?500"],forbid:["pilot (?:was|is|has been) (?:agreed|approved|signed)"],support:["Ninety days, up to twenty five users, for a flat four thousand five hundred dollars"]},
    {id:"contract-length",kind:"absent",fast:true,question:"What contract length did they commit to?",reference:"None. No contract length or commitment was discussed; the pilot terms were only proposed and need Renata's sign-off.",must:["no(?:t| contract| commitment|ne|thing)|didn't|did not|wasn't|was not"],forbid:["(?:three|two|one|3|2|1).year (?:contract|term|commitment)","committed to a"]},
    {id:"risks",kind:"synthesis",question:"What could kill this deal? Rank the risks.",reference:"Offline mode failing in the Reno yard; no direct Stratus WMS integration; shift-lead adoption without on-site onboarding (Marcus says this matters more than price); budget unapproved with a ceiling near sixty thousand while list price for 140 users is about sixty-three thousand; decision must land before the May peak; Renata needs to see it working at one site and will want references.",must:["offline","Stratus|integrat","on.?site|training|change management|adoption","60|sixty|budget|Renata"]},
    {id:"follow-who",kind:"follow-up",question:"And who on their side owns that integration?",history:[{role:"user",content:"What are their hard requirements?"},{role:"assistant",content:"Two: it must work offline, and it must integrate directly with their warehouse system, Stratus WMS."}],reference:"Joon, their IT lead. He will send the Stratus API documentation and sandbox credentials this week.",must:["Joon"],support:["I can send you the Stratus API documentation"]},
  ],
}
