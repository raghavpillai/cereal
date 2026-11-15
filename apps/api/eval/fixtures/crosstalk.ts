import type {Fixture} from "../lib/types"
import {near} from "../lib/transcript"

/** Incident review with interruptions, overlap, false starts and a corrected
 * number. Recorded on one phone in a room. Fictional people and system. */
export const crosstalk:Fixture={
  id:"crosstalk",title:"Checkout outage review",kind:"cross-talk and disfluent speech",daysAgo:4,capture:"ios",wpm:170,
  people:{Vik:"mic",Marisol:"mic",Dunya:"mic",Paolo:"mic"},
  script:`
Vik: Okay so, um, the checkout thing on Tuesday. Let's, let's just get the timeline straight before we, before anyone starts fixing things.
Paolo: Right, so it started at, uh,
~Dunya: Fourteen ten.
Paolo: fourteen ten UTC, yeah. That's when the first alert, um, the error rate alert fired.
Marisol: And how long were we down? Because support was telling people fourteen minutes and I,
~Paolo: No no no, not fourteen.
~Dunya: Forty.
Paolo: Forty. Four zero. It was forty minutes. Fourteen ten is the start time, I think that's where the fourteen came from.
Marisol: Oh. Oh no. Okay. So we told customers fourteen minutes.
Vik: We'll, yeah, we'll come back to that. So forty minutes, fourteen ten to fourteen fifty. What, um, what actually broke?
Dunya: So the, the feature flag config. Someone pushed a,
~Paolo: I pushed it. It was me.
Dunya: Paolo pushed a config change, and it had an expired, um, an expiry date on the checkout flag that was already in the past, so the flag just,
~Paolo: Switched off. For everyone.
Dunya: switched off for everyone, yeah, the moment it loaded.
Vik: And that went straight to, to all of production?
Paolo: Straight to all of it. Config pushes don't go through the canary. Code does, config doesn't. Which is, I mean, that's the actual problem.
Marisol: Sorry, can I, how many orders? I need a number for the,
~Dunya: Three twelve.
Marisol: Three hundred and twelve?
Dunya: Three hundred twelve failed orders. I pulled it this morning.
Vik: And the, the money? Finance will ask.
Dunya: About nine thousand four hundred euros. But, and this is important, that's delayed, not lost. Um, most of those people,
~Paolo: They came back.
Dunya: most of them came back and ordered within the hour. I don't have the exact, the exact recovered figure yet.
Vik: Okay so nine thousand four hundred delayed, not lost, recovered number to be confirmed. Got it. How did we, how did we fix it?
Paolo: Rolled back the config. That's it. Took two minutes once we, once we knew what it was. The thirty eight minutes before that was us looking at the payment provider because,
~Marisol: Because everyone always blames the payment provider.
Paolo: because everyone, yes.
Vik: Right. So, um, what do we change? I'll, I think it's obvious but say it.
Paolo: Config pushes go through the canary. Same as code. Five percent of traffic for ten minutes before it goes wide.
Dunya: And validation. The, the push should reject an expiry date in the past. That's a, that's a ten line check.
Vik: Yes to both. So, decision, canary is mandatory for config pushes from now on. No exceptions, not even, not even for flags.
Paolo: Agreed. I can add the canary step this sprint.
Vik: And the validation?
Paolo: Same change, I'll do them together.
Marisol: Can we, sorry, can we go back to the customers? We've got three hundred twelve people who had a failed order and we told some of them the wrong, the wrong duration. I'd like to send them a voucher. Like ten euros.
Vik: I, hmm. I don't want to decide vouchers in this room.
~Marisol: It's three thousand euros, it's nothing.
Vik: It's three thousand one hundred and twenty, and it's not our budget. I'm not saying no. I'm saying finance decides. I'll ask them today.
Marisol: Okay. But I can at least send the correction? That it was forty minutes, not fourteen?
Vik: Yes. Please. Send the correction to everyone who got the first message.
Marisol: I'll do that today.
Dunya: I'll write the postmortem. I can have it, um, Thursday.
Vik: Thursday's good. Blameless, please, I don't want Paolo's name in it,
~Paolo: I don't mind.
Vik: I mind. It's a process gap, it's not a Paolo gap.
Dunya: Blameless. Thursday.
Vik: Okay. Anything else? No? Thank you. That was, um, that was a good one actually.
`,
  notes:{
    sparse:`checkout outage tues\n40 min NOT 14\nflag config, no canary\n312 orders\nvouchers?? finance`,
    detailed:`# Timeline\n- Tue, 14:10-14:50 UTC = 40 min (support said 14 - wrong)\n\n# Cause\n- config push w/ expired flag date, no canary for config\n\n# Impact\n- 312 failed orders, ~EUR 9,400 delayed (not lost)\n\n# Fix / changes\n- rollback\n- canary mandatory for config\n- validate expiry dates\n\n# Open\n- vouchers - finance decides\n\n# Actions\n- Dunya postmortem Thu (blameless)\n- Paolo canary + validation this sprint\n- Marisol correction to customers\n- me: ask finance`,
  },
  gold:{
    decisions:[
      {id:"canary",text:"Canary is mandatory for config pushes from now on, with no exceptions.",must:["canary","config"]},
      {id:"validation",text:"Config pushes will reject expiry dates in the past.",must:["validat|reject","expir|past"]},
      {id:"blameless",text:"The postmortem will be blameless and will not name Paolo.",must:["blameless"]},
    ],
    actionItems:[
      {id:"postmortem",owner:"Dunya",due:"Thursday",text:"Dunya writes the blameless postmortem by Thursday.",must:["post.?mortem","Thursday"],ownerMust:near("Dunya","post.?mortem")},
      {id:"canary-step",owner:"Paolo",due:"this sprint",text:"Paolo adds the canary step and expiry-date validation for config pushes this sprint.",must:["canary","sprint|validat"],ownerMust:near("Paolo","canary|validat")},
      {id:"correction",owner:"Marisol",due:"today",text:"Marisol sends a correction (forty minutes, not fourteen) to everyone who got the first message.",must:["correction|correct"],ownerMust:near("Marisol","correct")},
      {id:"ask-finance",owner:"Vik",due:"today",text:"Vik asks finance today whether to send vouchers.",must:["finance","voucher"],ownerMust:near("Vik","finance")},
    ],
    facts:[
      {id:"duration",text:"The outage lasted forty minutes, from 14:10 to 14:50 UTC (not fourteen minutes as support said).",must:["forty|\\b40\\b"]},
      {id:"start",text:"It started at 14:10 UTC.",must:["14:10|fourteen ten|1410"]},
      {id:"cause",text:"A config push carried a checkout feature-flag expiry date already in the past; config pushes bypass the canary.",must:["config","expir|flag"]},
      {id:"orders",text:"312 orders failed.",must:["312|three hundred (?:and )?twelve"]},
      {id:"revenue",text:"About 9,400 euros of revenue was delayed, not lost; the recovered figure is not yet confirmed.",must:["9,?400|nine thousand four hundred","delayed|not lost|recover"]},
      {id:"fix",text:"Fixed by rolling back the config; 38 of the 40 minutes were spent investigating the payment provider.",must:["roll(?:ed|ing)?(?: it| the config| the change)? ?back|rollback|revert"]},
    ],
    notSaid:[
      {id:"fourteen-minutes",claim:"The outage lasted fourteen minutes.",forbid:["(?:lasted|down for|outage of|duration (?:of|was)) (?:about |roughly )?(?:fourteen|14) minutes"]},
      {id:"vouchers-approved",claim:"Vouchers were approved or will be sent.",forbid:["vouchers? (?:was|were|will be|has been|have been) (?:approved|sent|issued)","(?:decided|agreed) to (?:send|issue|offer) (?:a |ten.euro )?vouchers?"]},
      {id:"revenue-lost",claim:"9,400 euros was lost.",forbid:["(?:lost|loss of) (?:about |roughly |approximately )?(?:€|EUR )?9,?400","9,?400 (?:euros? )?(?:in |of )?lost"]},
      {id:"payment-provider",claim:"The payment provider caused the outage.",forbid:["(?:caused by|due to|because of) (?:the |an? )?(?:payment provider|payment processor)","payment provider (?:outage|failure|was at fault)"]},
      {id:"blame-paolo",claim:"Paolo is blamed as the root cause.",forbid:["Paolo(?:'s)? (?:mistake|fault|error)","root cause[^.\\n]{0,40}Paolo"]},
    ],
  },
  questions:[
    {id:"duration",kind:"factual",fast:true,question:"How long was the outage?",reference:"Forty minutes, from 14:10 to 14:50 UTC. Support had told customers fourteen minutes, which was wrong; the fourteen came from the start time.",must:["forty|\\b40\\b"],forbid:["(?:lasted|down for|outage (?:of|was)) (?:about |roughly )?(?:fourteen|14) minutes"],support:["It was forty minutes"]},
    {id:"impact",kind:"citation",fast:true,question:"What was the customer and revenue impact? Cite the transcript.",reference:"312 failed orders and about 9,400 euros of delayed, not lost, revenue; most customers came back and ordered within the hour, and the recovered figure is not yet confirmed.",must:["312|three hundred (?:and )?twelve","9,?400|nine thousand four hundred","delayed|not lost|came back|recover"],forbid:["(?:lost|loss of) (?:about |roughly )?(?:€|EUR )?9,?400"],support:["Three hundred twelve failed orders","About nine thousand four hundred euros"]},
    {id:"vouchers",kind:"absent",fast:true,question:"How much are we giving customers in vouchers?",reference:"Nothing has been decided. Marisol suggested ten euros each (3,120 euros for 312 customers), but Vik said finance decides and he will ask them today.",must:["not (?:been )?decided|no decision|undecided|finance|hasn't been|has not been|not yet|pending"],forbid:["vouchers? (?:was|were|will be|has been|have been) (?:approved|sent|issued)"],support:["I don't want to decide vouchers in this room"]},
    {id:"what-changes",kind:"synthesis",question:"What are we changing so this cannot happen again, and who is doing it?",reference:"Config pushes must go through the canary like code (five percent of traffic for ten minutes), with no exceptions, and pushes must reject expiry dates in the past. Paolo will make both changes this sprint. Dunya writes a blameless postmortem by Thursday.",must:["canary","validat|reject|expir","Paolo"]},
    {id:"follow-why-wrong",kind:"follow-up",question:"Where did the wrong number come from?",history:[{role:"user",content:"What did support tell customers about the outage?"},{role:"assistant",content:"Support told customers the outage lasted fourteen minutes, which was wrong: it lasted forty minutes."}],reference:"Probably from the start time, 14:10 UTC: Paolo thinks that is where the fourteen came from.",must:["14:10|fourteen ten|start time"],support:["Fourteen ten is the start time, I think that's where the fourteen came from"]},
  ],
}
