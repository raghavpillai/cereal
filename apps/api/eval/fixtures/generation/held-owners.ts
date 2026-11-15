import type {GenerationFixture} from "../../lib/generation-fixtures"
import {near} from "../../lib/transcript"

/** HELD OUT: never used while tuning the generation prompt.
 * A two-person video call with a calendar event attached. Nobody addresses
 * anyone by name after the greeting, so every commitment is first person: the
 * note owner at the microphone, the one remote attendee on system audio. Also a
 * "we should" that nobody takes, and a third party who must sign off but was
 * given no task. Fictional companies and people. */
export const heldOwners:GenerationFixture={
  id:"held-owners",split:"held-out",stress:"first-person commitments on a two-person call; a 'we should' with no owner; a third party who is not an owner",
  title:"Foxglove x Marrow & Pine intro",kind:"partnership intro call",daysAgo:2,capture:"desktop",wpm:150,
  people:{Ileana:"mic",Desmond:"system"},
  attendees:[{name:"Ileana Brandt",email:"ileana@foxgloveanalytics.example",self:true},{name:"Desmond Achebe",email:"desmond@marrowandpine.example"}],
  roster:["Farrukh"],
  questions:[],
  script:`
Ileana: Hi Desmond, thanks for making time.
Desmond: Hi Ileana. Happy to. I've read the one pager, so skip the pitch.
Ileana: Great, that saves ten minutes. So the idea in one line, you have loyalty data from about two hundred and ten garden centres, we have the footfall models, and neither of us can answer the seasonal stocking question alone.
Desmond: That's how I read it. My worry is data sharing. Our loyalty data has postcodes in it, and our legal team gets nervous the moment it leaves the building.
Ileana: Understood. We wouldn't need postcodes. We work at district level, so you could aggregate before anything is shared.
Desmond: District level I can probably get through. How many stores would you need for a first test?
Ileana: Twenty would be enough to see whether the model holds. Thirty would be better.
Desmond: I can do twenty without asking anyone. Thirty I'd have to check.
Ileana: Let's say twenty to start then.
Desmond: Twenty stores, district level, for a first test. Fine. How long?
Ileana: Eight weeks of history is the minimum. Twelve gives us a full season change.
Desmond: Twelve weeks I can pull. It's the same query.
Ileana: Then twelve weeks. On our side, I'll send you our standard data sharing agreement template by Tuesday so your legal team has something concrete to react to.
Desmond: Good. And I'll introduce you to our head of data next week, she's the one who actually runs the extract.
Ileana: Perfect. Is there anyone else who has to say yes on your side?
Desmond: Our chief technology officer, Farrukh, will need to sign off before any data moves. He's supportive in principle, he just wants to see the agreement first.
Ileana: Makes sense. We should probably loop in legal on both sides at some point too.
Desmond: Yeah, at some point. Not before there's a draft to look at.
Ileana: Agreed. What about commercials? I'd suggest we do the test at no cost to either side and talk money only if the model works.
Desmond: I like that. I can't formally agree to it today, but I'd be surprised if anyone objected.
Ileana: That's fine. I'll put it in the summary as a proposal.
Desmond: One question. If this works, who owns the combined model?
Ileana: Honestly we haven't decided how we'd structure that. I don't want to make something up on a call.
Desmond: Fair. Let's leave it open.
Ileana: And timing, I'd like to have the test data by the end of the month if the agreement goes smoothly.
Desmond: End of the month is realistic if legal turns it round in a week. I'll confirm the extract date once I've spoken to our head of data.
Ileana: Great. Thanks Desmond.
Desmond: Thanks. Speak soon.
`,
  notes:{
    sparse:`district level only - no postcodes\n20 stores / 12 wks\nownership of model = open`,
    detailed:`# Fit\n- they have loyalty data (~210 garden centres), we have footfall models\n- Desmond had read the one-pager, no pitch needed\n\n# Data sharing\n- postcodes are the blocker for their legal\n- district-level aggregation OK\n- first test: 20 stores, 12 weeks of history\n\n# Commercials\n- I proposed: no-cost test, talk money if model works (not formally agreed)\n- combined model ownership: open\n\n# Next\n- me: DSA template by Tues\n- Desmond: intro to head of data next week\n- gut feel: he's keen, legal will be the slow part`,
  },
  gold:{
    decisions:[
      {id:"test-scope",text:"The first test will use 20 stores, aggregated to district level (no postcodes), with 12 weeks of history.",must:["20|twenty","12|twelve","district"]},
    ],
    actionItems:[
      {id:"dsa",owner:"Ileana",due:"Tuesday",text:"Ileana (the note owner) sends the standard data-sharing agreement template by Tuesday.",must:["agreement|DSA","Tuesday"],ownerMust:near("Ileana","agreement|DSA|template",100)},
      {id:"intro",owner:"Desmond",due:"next week",text:"Desmond introduces Ileana to Marrow & Pine's head of data next week.",must:["head of data"],ownerMust:near("Desmond","head of data|introduc",100)},
      {id:"extract-date",owner:"Desmond",text:"Desmond confirms the extract date after speaking to the head of data.",must:["extract"],ownerMust:near("Desmond","extract",100)},
    ],
    facts:[
      {id:"stores",text:"Marrow & Pine has loyalty data from about 210 garden centres.",must:["210|two hundred and ten|two hundred ten"]},
      {id:"postcodes",text:"The loyalty data contains postcodes, which worries their legal team; Foxglove only needs district level.",must:["postcode"]},
      {id:"signoff",text:"Their CTO, Farrukh, must sign off before any data moves and wants to see the agreement first.",must:["Farrukh|CTO|chief technology"]},
      {id:"no-cost",text:"A no-cost test was proposed by Ileana; Desmond likes it but could not formally agree.",must:["no.cost|free|at no charge|without charge|no charge|no fee"]},
      {id:"ownership-open",text:"Ownership of a combined model is undecided.",must:["own"]},
      {id:"timing",text:"Target: test data by the end of the month, if legal turns the agreement round in a week.",must:["end of (?:the )?month"]},
    ],
    notSaid:[
      {id:"no-cost-agreed",claim:"The no-cost test was formally agreed.",forbid:["(?:agreed|decided|confirmed)[^.\\n]{0,30}(?:no.cost|free|at no charge)","(?:no.cost|free) (?:test|pilot)[^.\\n]{0,15}(?:was|is) (?:agreed|confirmed|approved)"]},
      {id:"thirty-stores",claim:"The test uses 30 stores.",forbid:["(?:test|pilot|start|begin)[^.\\n]{0,25}(?:with|using|on|of) (?:30|thirty) stores"]},
      {id:"legal-task",claim:"Someone was assigned to loop in legal.",forbid:["(?:Ileana|Desmond)(?: Brandt| Achebe)?(?::| will| to| is going to)[^.\\n]{0,20}(?:loop in|involve|brief|engage|contact) (?:the )?legal"]},
      {id:"farrukh-task",claim:"Farrukh was given or accepted a task in this meeting.",forbid:["Farrukh(?::| will| to| is going to| agreed to)\\s+(?:review|send|sign the|approve the|prepare|check)"]},
      {id:"ownership-decided",claim:"Ownership of the combined model was decided.",forbid:["(?:Foxglove|Marrow & Pine|joint(?:ly)?|both parties) (?:will )?owns? the (?:combined )?model"]},
      {id:"swapped",claim:"The two first-person commitments are swapped (Desmond sends the agreement, Ileana introduces the head of data).",forbid:["Desmond(?: Achebe)?(?::| will| to| is going to)[^.\\n]{0,20}send[^.\\n]{0,40}(?:agreement|template|DSA)","Ileana(?: Brandt)?(?::| will| to| is going to)[^.\\n]{0,20}introduc[^.\\n]{0,40}head of data"]},
    ],
  },
}
