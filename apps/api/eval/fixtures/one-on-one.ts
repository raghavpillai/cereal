import type {Fixture} from "../lib/types"
import {near} from "../lib/transcript"

/** Manager/report 1:1 recorded on one phone in a room. Fictional people. */
export const oneOnOne:Fixture={
  id:"one-on-one",title:"Ama / Lucas 1:1",kind:"1:1",daysAgo:3,capture:"ios",wpm:145,
  people:{Ama:"mic",Lucas:"mic"},
  script:`
Ama: Okay, recording so I don't have to type. How are you doing, Lucas? Honestly.
Lucas: Honestly, tired. Last week's on call was rough.
Ama: I saw the numbers but tell me how it felt.
Lucas: I was paged nine times. Four of those were between midnight and five in the morning. And three of the nine were the same disk alert on the reporting database that auto resolves in ten minutes.
Ama: So a third of your pages were noise.
Lucas: Yeah. I'm fine being on call, I'm not fine being woken up by an alert everyone knows is broken.
Ama: That's fair. Who owns that alert?
Lucas: Deepak's team, I think. It's been in their backlog since January.
Ama: Okay, I'll take that one. Let me come back to on call in a second. What else is on your list?
Lucas: Two things. The billing migration, and the promotion packet.
Ama: Start with billing.
Lucas: I'd like to lead the design. Not just write a section, actually own the design document and run the reviews. I know the ledger service better than anyone now, and I think it's the kind of scope I need for senior anyway.
Ama: I agree, and I was going to suggest it. Your ledger service design doc was the strongest one the team produced last quarter. People still link to it.
Lucas: Thanks.
Ama: The feedback I have, since we're here, is that it landed a week late, and you didn't tell anyone until the day it was due. The quality was great. The surprise was the problem.
Lucas: Yeah. I kept thinking I could close the gap over the weekend.
Ama: Next time flag it on Wednesday, not Friday. If you lead billing, a late design doc blocks four other people, so I need the early warning more than I need the polish.
Lucas: Understood. That's fair.
Ama: So yes, you lead the billing migration design. And to make room for it, I want to take you off the on call rotation for two sprints, starting next sprint.
Lucas: Really? That would help a lot. Won't that annoy the others?
Ama: Georg already offered to swap in, he wants the extra on call pay before his move. I'll talk to Deepak, he owns the rotation and I want him to hear the alert problem from me at the same time.
Lucas: Okay. Thank you.
Ama: Now the packet.
Lucas: The promotion packet for senior is due March fourteenth. I have nothing written. I don't really know what a good one looks like.
Ama: I'll send you an example, the one Sunita wrote two cycles ago is a good model, I'll ask her if she's okay with me sharing it. You need two peer reviews as well.
Lucas: I was thinking Sunita and Georg.
Ama: Good choices. Ask them this week, they get a lot of requests close to the deadline.
Lucas: I can have an outline of the packet to you by next Friday.
Ama: Outline by next Friday works. Don't write prose yet, just the three or four stories you want to tell and the evidence for each.
Lucas: Got it. One more thing, logistics. I'm taking leave from April sixth to April seventeenth. It's already approved in the system, I just wanted you to know because it's in the middle of the billing work.
Ama: That's fine. Let's plan the design review to finish before April sixth then, so nothing waits on you while you're out.
Lucas: That's tight, but okay.
Ama: If it's too tight, tell me on a Wednesday.
Lucas: Ha. Yes.
Ama: Is there anything else? Anything about the team, or about me?
Lucas: Just that I appreciate being asked how it felt and not just shown the dashboard.
Ama: Good. Okay, I've got three things, you've got three things. Let's stop there.
`,
  notes:{
    sparse:`L tired - on call 9 pages\nwants billing lead -> yes\npacket Mar 14\nmy note: seems relieved more than excited, check energy again in 2 wks`,
    detailed:`# On call\n- 9 pages last week, 4 overnight\n- 3 were the reporting DB disk alert (noise) - Deepak's team\n- off rotation 2 sprints from next sprint, Georg swaps in\n\n# Billing migration\n- Lucas leads design (owns doc + reviews)\n- feedback given: ledger doc great but a week late, no warning. Flag by Wed.\n\n# Promotion\n- packet due Mar 14\n- peer reviews: Sunita, Georg\n- outline to me next Fri\n\n# Leave\n- Apr 6-17\n\n# Mine\n- talk to Deepak\n- send example packet (ask Sunita first)\n- private: he seemed relieved more than excited; check in on energy in two weeks`,
  },
  gold:{
    decisions:[
      {id:"leads-billing",text:"Lucas will lead the billing migration design (own the design document and run the reviews).",must:["billing","lead|own"]},
      {id:"off-oncall",text:"Lucas comes off the on-call rotation for two sprints starting next sprint; Georg swaps in.",must:["on.?call","two sprints|2 sprints"]},
      {id:"review-before-leave",text:"The billing design review should finish before Lucas's leave starts on April 6.",must:["April (?:6|sixth)|before (?:his|the) leave|Apr(?:il)? 6"]},
    ],
    actionItems:[
      {id:"talk-deepak",owner:"Ama",text:"Ama talks to Deepak about the rotation change and the noisy reporting-database disk alert.",must:["Deepak"],ownerMust:near("Ama","Deepak")},
      {id:"example-packet",owner:"Ama",text:"Ama sends an example promotion packet (Sunita's), after asking Sunita's permission.",must:["example","packet"],ownerMust:near("Ama","example|packet")},
      {id:"packet-outline",owner:"Lucas",due:"next Friday",text:"Lucas sends an outline of his promotion packet by next Friday.",must:["outline","Friday"],ownerMust:near("Lucas","outline")},
      {id:"ask-reviewers",owner:"Lucas",due:"this week",text:"Lucas asks Sunita and Georg for peer reviews this week.",must:["Sunita","Georg","review"],ownerMust:near("Lucas","peer review|Sunita|Georg")},
    ],
    facts:[
      {id:"pages",text:"Lucas was paged nine times last week, four of them between midnight and 5 a.m.; three were the same auto-resolving disk alert.",must:["nine|\\b9\\b","four|\\b4\\b"]},
      {id:"packet-due",text:"The senior promotion packet is due March 14.",must:["March (?:14|fourteenth)|Mar 14"]},
      {id:"leave",text:"Lucas is on approved leave April 6 to April 17.",must:["April (?:6|sixth)|Apr(?:il)? 6","17|seventeenth"]},
      {id:"feedback",text:"Feedback: the ledger service design doc was excellent but a week late with no early warning; flag slips by Wednesday.",must:["late","Wednesday|early warning|earlier|flag|surprise"]},
    ],
    notSaid:[
      {id:"resigning",claim:"Lucas is thinking of leaving or has another offer.",forbid:["(?:resign|quit|leaving the (?:company|team)|another offer|external offer|job offer)"]},
      {id:"raise",claim:"A salary figure or raise was discussed.",forbid:["(?:salary|raise|compensation) (?:of|increase)","\\d+ ?% raise"]},
      {id:"promotion-approved",claim:"The promotion was approved or guaranteed.",forbid:["promotion (?:was|is|has been) (?:approved|confirmed|guaranteed)","will be promoted"]},
      {id:"off-oncall-permanent",claim:"Lucas is permanently off on call.",forbid:["permanently","off (?:the )?on.?call (?:rotation )?(?:for good|indefinitely)"]},
    ],
  },
  questions:[
    {id:"pages",kind:"factual",fast:true,question:"How many times was Lucas paged, and how many were overnight?",reference:"Nine times last week; four were between midnight and five in the morning. Three of the nine were the same auto-resolving disk alert on the reporting database.",must:["nine|\\b9\\b","four|\\b4\\b"],support:["I was paged nine times"]},
    {id:"feedback",kind:"citation",question:"What critical feedback did I give Lucas? Quote the source.",reference:"The ledger service design doc landed a week late and he told no one until the due date. The quality was great; the surprise was the problem. He should flag slips on Wednesday, not Friday.",must:["late","Wednesday|surprise|until the day|early warning"],support:["it landed a week late, and you didn't tell anyone until the day it was due"]},
    {id:"my-todos",kind:"synthesis",fast:true,question:"What did I commit to doing after this 1:1?",reference:"Talk to Deepak about the on-call rotation and the noisy disk alert; send Lucas an example promotion packet after asking Sunita; plan the billing design review to finish before April 6.",must:["Deepak","example|packet"]},
    {id:"salary",kind:"absent",fast:true,question:"What raise did we agree on for Lucas?",reference:"None. Compensation was not discussed; the conversation covered the promotion packet due March 14, not a raise.",must:["no(?:t| raise|ne|thing)|didn't|did not|wasn't|was not"],forbid:["\\d+ ?%","agreed (?:on|to) a (?:raise|salary)"]},
    {id:"follow-who-covers",kind:"follow-up",question:"Who covers for him while he's off it?",history:[{role:"user",content:"What changes for Lucas's on-call?"},{role:"assistant",content:"He comes off the on-call rotation for two sprints, starting next sprint, to make room for leading the billing migration design."}],reference:"Georg offered to swap in; he wants the extra on-call pay before his move. Ama will confirm with Deepak, who owns the rotation.",must:["Georg"],support:["Georg already offered to swap in"]},
  ],
}
