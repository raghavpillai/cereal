import {FIXTURES} from "../../lib/fixtures"
import type {Channel,Fixture} from "../../lib/types"
import {tidewater} from "./tidewater"
import {kiln} from "./kiln"
import {ABSTAIN,type McFixture} from "./types"

/** Two meetings of well over an hour, to test what happens when the whole
 * transcript is far more than twelve segments: facts planted in the first
 * minutes, the middle and the last minutes, with the eight shared fixtures and
 * one fresh meeting as the sessions in between. The sessions are unrelated to
 * one another on purpose: they are an hour and a half of plausible distraction.
 * Questions depend only on the planted blocks (and one deep fact from a shared
 * session each), so edits to the shared fixtures do not change the answers. */
const session=(id:string):Fixture=>{const found=FIXTURES.find(fixture=>fixture.id===id);if(!found)throw new Error(`Unknown fixture ${id}`);return found}
const body=(fixture:Fixture)=>fixture.script.trim()
const everyone=(hosts:Record<string,Channel>,sessions:ReadonlyArray<Fixture>,rest:Channel):Record<string,Channel>=>({...Object.fromEntries(sessions.flatMap(fixture=>Object.keys(fixture.people)).map(name=>[name,rest])),...hosts})

const TUNED_SESSIONS=["standup","discovery","one-on-one","interview","brainstorm","planning","crosstalk","numbers"].map(session)
/** TUNED. Recorded on a laptop: the host on the microphone, everyone else on system audio. */
export const partnerDay:McFixture={
  set:"tuned",id:"mc-t-partner-day",title:"Harbour Collective partner day",kind:"about two hours, facts planted early, middle and late",daysAgo:3,capture:"desktop",wpm:140,
  people:everyone({Hedda:"mic",Oskar:"system"},[...TUNED_SESSIONS,tidewater],"system"),
  attendees:[{name:"Hedda Lund",email:"hedda@harbourcollective.example",self:true},{name:"Oskar Brandt",email:"oskar@harbourcollective.example"}],
  script:[
`Hedda: Good morning everyone, welcome to the partner day. Some housekeeping before the first session. We have this room until five thirty, and I'm recording the whole day in one go so I can write it up afterwards.
Hedda: Lunch is at twelve forty in the courtyard, not at noon like the invite says, the caterer asked for the extra time.
Oskar: And on money, since people asked. The budget for today is six thousand two hundred euros, and catering is two thousand four hundred fifty of that.
Hedda: Thanks Oskar. Anything else from you?
Oskar: The shuttle to the station leaves at five forty five from the side gate. One shuttle, it won't wait. And I'll circulate the attendee list by Monday.
Hedda: Great. Okay. First session, the ferry team, over to you.`,
    body(TUNED_SESSIONS[0]!),
`Hedda: Thank you. Next session, please, the Kestrel call.`,
    body(TUNED_SESSIONS[1]!),
`Hedda: Thanks. Two short ones before the break.`,
    body(TUNED_SESSIONS[2]!),
    body(TUNED_SESSIONS[3]!),
`Hedda: Before we start the next block, two changes to the afternoon. The workshop moves from the Birch room to the Alder room on the second floor, because the projector in Birch is broken.
Hedda: And the group photo is at three fifteen on the terrace, so please don't disappear after the workshop.
Oskar: Also parking. Bring your ticket to the front desk before four, they stop validating at four.
Hedda: Right. Okay, brainstorm group, go ahead.`,
    body(TUNED_SESSIONS[4]!),
`Hedda: Thank you. Now the long one, the clinic planning session.`,
    body(TUNED_SESSIONS[5]!),
`Hedda: Thanks, that was a lot. Next the outage review, then the numbers.`,
    body(TUNED_SESSIONS[6]!),
    body(TUNED_SESSIONS[7]!),
`Hedda: Last session of the day, the bike launch.`,
    body(tidewater),
`Hedda: Thank you all, that's the end of the sessions. A few closing things. The next partner day is March twelfth, in Ghent.
Oskar: Odile has agreed to book the venue, and she'll do that by January ninth.
Hedda: Feedback forms go out tomorrow. Please send them back within a week, I do read them.
Oskar: And the one decision from the wrap up, we rotate the host each time, so the Ghent one is hosted by Odile's team.
Hedda: That's everything. Safe travels, and thank you.`,
  ].join("\n[+20]\n"),
  notes:{sparse:`Partner day\n- room till 17:30\n- my session notes are in the other doc\n- ask Oskar: who pays for the shuttle?\n- venue contact: Mireille (front desk)`,detailed:`# Partner day\n- room till 17:30\n- my session notes are in the other doc\n- ask Oskar: who pays for the shuttle?\n- venue contact: Mireille (front desk)`},
  gold:{
    decisions:[{id:"host-rotation",text:"The host rotates each time; the Ghent partner day is hosted by Odile's team.",must:["rotat","Odile"]}],
    actionItems:[],
    facts:[
      {id:"lunch",text:"Lunch is at 12:40 in the courtyard, not at noon.",must:["12[:.]40|twelve forty","courtyard"]},
      {id:"budget",text:"The budget for the day is 6,200 euros, of which catering is 2,450.",must:["6,?200|six thousand two hundred","2,?450|two thousand four hundred fifty"]},
      {id:"room-change",text:"The afternoon workshop moved from the Birch room to the Alder room on the second floor because the Birch projector is broken.",must:["Alder","projector"]},
      {id:"next",text:"The next partner day is March 12 in Ghent; Odile books the venue by January 9.",must:["March 12|12 March|March twelfth","Ghent"]},
    ],
    notSaid:[{id:"dinner",claim:"A dinner reservation was announced.",forbid:["dinner (?:is|was|will be) (?:at|booked|reserved)"]}],
  },
  questions:[
    {id:"lunch",tag:"early",kind:"factual",fast:true,question:"What time is lunch, and where?",reference:"12:40 in the courtyard, not noon as the invite said; the caterer asked for extra time.",must:["12[:.]40|twelve forty","courtyard"],support:["Lunch is at twelve forty in the courtyard"]},
    {id:"room-change",tag:"middle",kind:"factual",fast:true,question:"Which room is the afternoon workshop in, and why did it move?",reference:"The Alder room on the second floor. It moved from the Birch room because the projector in Birch is broken.",must:["Alder","projector"],support:["The workshop moves from the Birch room to the Alder room on the second floor"]},
    {id:"next-partner-day",tag:"late",kind:"factual",fast:true,question:"When and where is the next partner day, and who books the venue by when?",reference:"March 12 in Ghent. Odile books the venue by January 9, and her team hosts.",must:["March 12|12 March|March twelfth","Ghent","Odile","January 9|9 January|January ninth"],support:["The next partner day is March twelfth, in Ghent"]},
    {id:"budget-left",tag:"arithmetic",kind:"factual",question:"How much of today's budget is left once catering is paid?",reference:"3,750 euros: the budget is 6,200 and catering is 2,450.",must:["3,?750"],support:["The budget for today is six thousand two hundred euros"]},
    {id:"shuttle-follow-up",tag:"follow-up",kind:"follow-up",fast:true,question:"Is that before or after we lose the room?",
      history:[{role:"user",content:"When does the shuttle leave?"},{role:"assistant",content:"At 5:45 pm from the side gate. There is only one shuttle and it will not wait."}],
      reference:"After. The room is booked until 5:30, and the shuttle leaves fifteen minutes later at 5:45.",must:["after","5[:.]30|five thirty"],support:["We have this room until five thirty"]},
    {id:"dinner",tag:"not-said",kind:"absent",question:"What did Hedda say about the dinner reservation?",reference:"Nothing. No dinner was mentioned; the only meal covered was lunch at 12:40 in the courtyard.",must:[ABSTAIN]},
    {id:"freeze-in-planning",tag:"cross-session",kind:"factual",question:"In the clinic planning session, when is the data warehouse freeze?",reference:"August 11 to August 15, with no schema changes that week.",must:["August (?:11|eleventh)|Aug(?:ust)? 11","15|fifteenth"]},
    {id:"cite-photo",tag:"citation",kind:"citation",fast:true,question:"When is the group photo? Cite it.",reference:"At 3:15 on the terrace, after the workshop.",must:["3[:.]15|three fifteen","terrace"],support:["the group photo is at three fifteen on the terrace"]},
    {id:"venue-contact",tag:"notes-only",kind:"factual",question:"Who is the venue contact?",reference:"Mireille at the front desk, according to your typed notes.",must:["Mireille"]},
  ],
  live:[
    {id:"miss-mid",kind:"recency",recipe:true,fast:true,after:"Also parking. Bring your ticket to the front desk before four, they stop validating at four.",question:"What did I miss?",
      reference:"- The afternoon workshop moves from the Birch room to the Alder room on the second floor because the Birch projector is broken.\n- Group photo at 3:15 on the terrace.\n- Parking tickets must be validated at the front desk before four.",must:["Alder|photo|parking|validat"],later:["Ghent","Odile"],forbid:["courtyard","twelve forty|12[:.]40","shuttle"]},
    {id:"just-said-deep",kind:"just-said",fast:true,after:"I want six percent, on renewals only, starting January first.",question:"What did they just say about the price increase?",
      reference:"Six percent, on renewals only, starting January 1, with nothing changing mid-contract. Eight percent had been proposed before.",must:["six percent|6 ?%|6 percent"],later:["twenty percent|20 ?%","Orrin"]},
    {id:"recall-lunch-deep",kind:"recall",fast:true,after:"I want six percent, on renewals only, starting January first.",question:"What time was lunch again?",
      reference:"12:40 in the courtyard.",must:["12[:.]40|twelve forty"],later:["Ghent"]},
  ],
}

const HELD_SESSIONS=["numbers","crosstalk","brainstorm","interview","planning","one-on-one","discovery","standup"].map(session)
/** HELD OUT. Recorded on one phone at the front of the room (a single channel). */
export const springSummit:McFixture={
  set:"held",id:"mc-h-summit",title:"Spring summit, all sessions",kind:"about two hours on one phone, facts planted early, middle and late",daysAgo:4,capture:"ios",wpm:140,
  people:everyone({Ludo:"mic",Sanne:"mic"},[...HELD_SESSIONS,kiln],"mic"),
  attendees:[{name:"Ludo Peeters",email:"ludo@springsummit.example",self:true},{name:"Sanne Vos",email:"sanne@springsummit.example"}],
  script:[
`Ludo: Welcome to the spring summit. I'll keep this short. Forty six people registered and thirty nine of you are in the room, which is a record for us.
Ludo: Sessions run thirty five minutes each. The coffee break is at ten fifty, not ten thirty, the programme on the door is wrong.
Sanne: Two admin things from me. Travel expenses, please submit them by the twenty fourth, and the cap is one hundred eighty euros per person.
Sanne: And this recording gets deleted after fourteen days, it's only for the write up.
Ludo: Okay. First up, the business review.`,
    body(HELD_SESSIONS[0]!),
`Ludo: Thank you. Next, the outage review and then the launch brainstorm.`,
    body(HELD_SESSIONS[1]!),
    body(HELD_SESSIONS[2]!),
`Sanne: Sorry to interrupt the flow. Change of plan for this afternoon. The panel is cancelled, because Marta is ill.
Sanne: Instead we'll do lightning talks. Six speakers, five minutes each.
Ludo: And a warning, the fire alarm test is at two o'clock. It's only a test, please ignore it.
Sanne: Okay, carry on, the interview debrief.`,
    body(HELD_SESSIONS[3]!),
`Ludo: Thanks. The planning session is next and it's the long one.`,
    body(HELD_SESSIONS[4]!),
`Ludo: Thank you. Three shorter ones now.`,
    body(HELD_SESSIONS[5]!),
    body(HELD_SESSIONS[6]!),
    body(HELD_SESSIONS[7]!),
`Ludo: And the last one, the ceramics wholesale conversation.`,
    body(kiln),
`Ludo: That's a wrap. The autumn summit is on October eighth, in Utrecht.
Sanne: Wim will send the survey on Friday, and we'd like responses by the thirtieth.
Ludo: Slides go into the shared folder called Summit Spring by tomorrow noon. Sanne is uploading them.
Sanne: I am. Thanks everyone, get home safe.`,
  ].join("\n[+20]\n"),
  notes:{sparse:`Spring summit\n- 46 registered\n- remind Wim about the survey\n- AV supplier: Brightline, invoice no. 2217`,detailed:`# Spring summit\n- 46 registered\n- remind Wim about the survey\n- AV supplier: Brightline, invoice no. 2217`},
  gold:{
    decisions:[{id:"panel",text:"The afternoon panel is cancelled because Marta is ill; lightning talks replace it (six speakers, five minutes each).",must:["Marta","lightning"]}],
    actionItems:[],
    facts:[
      {id:"break",text:"The coffee break is at 10:50, not 10:30; sessions run 35 minutes each.",must:["10[:.]50|ten fifty","35|thirty.five"]},
      {id:"expenses",text:"Travel expenses are due by the 24th and capped at 180 euros per person.",must:["24","180|one hundred eighty"]},
      {id:"next",text:"The autumn summit is on October 8 in Utrecht; Wim sends the survey on Friday, with responses due by the 30th.",must:["October 8|8 October|October eighth","Utrecht"]},
    ],
    notSaid:[{id:"hotel",claim:"A hotel block was arranged.",forbid:["hotel (?:block|rooms?) (?:is|was|are|were|has been) (?:booked|reserved|arranged|held)"]}],
  },
  questions:[
    {id:"coffee-break",tag:"early",kind:"factual",fast:true,question:"When is the coffee break, and how long is each session?",reference:"The coffee break is at 10:50, not 10:30 as the programme on the door says. Sessions run 35 minutes each.",must:["10[:.]50|ten fifty","35|thirty.five"],support:["The coffee break is at ten fifty, not ten thirty"]},
    {id:"panel",tag:"middle",kind:"factual",fast:true,question:"Why was the panel cancelled, and what replaces it?",reference:"Marta is ill, so the panel is cancelled. Lightning talks replace it: six speakers, five minutes each.",must:["Marta","lightning"],support:["The panel is cancelled, because Marta is ill"]},
    {id:"autumn",tag:"late",kind:"factual",fast:true,question:"When and where is the autumn summit, and who sends the survey?",reference:"October 8 in Utrecht. Wim sends the survey on Friday, with responses wanted by the 30th.",must:["October 8|8 October|October eighth","Utrecht","Wim"],support:["The autumn summit is on October eighth, in Utrecht"]},
    {id:"no-shows",tag:"arithmetic",kind:"factual",question:"How many people registered but did not show up?",reference:"Seven: 46 registered and 39 were in the room.",must:["\\b7\\b|seven"],support:["Forty six people registered and thirty nine of you are in the room"]},
    {id:"expenses-follow-up",tag:"follow-up",kind:"follow-up",fast:true,question:"And when do I have to submit by?",
      history:[{role:"user",content:"What's the cap on travel expenses?"},{role:"assistant",content:"180 euros per person."}],
      reference:"By the 24th.",must:["24|twenty.fourth"],support:["please submit them by the twenty fourth"]},
    {id:"hotel",tag:"not-said",kind:"absent",question:"What did they say about the hotel block?",reference:"Nothing. No hotel block was mentioned.",must:[ABSTAIN]},
    {id:"freeze-in-review",tag:"cross-session",kind:"factual",question:"In the business review session, what headcount did they freeze hiring at?",reference:"152, with an exception for two support roles, so 154 at most.",must:["152|one hundred fifty.two"]},
    {id:"cite-recording",tag:"citation",kind:"citation",fast:true,question:"How long is this recording kept? Cite it.",reference:"It is deleted after fourteen days; it is only for the write-up.",must:["14|fourteen"],support:["this recording gets deleted after fourteen days"]},
    {id:"av-supplier",tag:"notes-only",kind:"factual",question:"Who is the AV supplier and what is the invoice number?",reference:"Brightline, invoice number 2217, according to your typed notes.",must:["Brightline","2217"]},
  ],
  live:[
    {id:"miss-mid",kind:"recency",recipe:true,fast:true,after:"It's only a test, please ignore it.",question:"What did I miss?",
      reference:"- The afternoon panel is cancelled because Marta is ill.\n- Lightning talks replace it: six speakers, five minutes each.\n- There is a fire alarm test at two o'clock; ignore it.",must:["Marta|panel|lightning|fire alarm"],later:["Utrecht","Wim"],forbid:["ten fifty|10[:.]50","expenses"]},
    {id:"just-said-deep",kind:"just-said",fast:true,after:"She's free on the nineteenth, I think, in the afternoon.",question:"What date did they just suggest for the demo?",
      reference:"The nineteenth, in the afternoon, when Renata is thought to be free; Marcus still has to confirm with her.",must:["nineteenth|19th|\\b19\\b"],later:["Utrecht"]},
    {id:"recall-break-deep",kind:"recall",fast:true,after:"She's free on the nineteenth, I think, in the afternoon.",question:"When was the coffee break again?",
      reference:"10:50, not 10:30.",must:["10[:.]50|ten fifty"],later:["Utrecht"]},
  ],
}
