import type {GenerationFixture} from "../../lib/generation-fixtures"
import {near} from "../../lib/transcript"

/** A forty-second call. The right notes are three or four lines, not a
 * structured document padded with sections. Fictional people and supplier. */
export const short:GenerationFixture={
  id:"hard-short",split:"tuned",stress:"a very short meeting: the notes must stay tiny",
  title:"Quick call with Hollis",kind:"short phone call",daysAgo:1,capture:"desktop",wpm:160,
  people:{Zainab:"mic",Hollis:"system"},
  questions:[],
  script:`
Zainab: Hey Hollis, quick one. Is the Friday delivery still on?
Hollis: It is, but it's the afternoon slot now, between two and four. The morning truck is full.
Zainab: Afternoon's fine. Same forty crates?
Hollis: Thirty eight. Two of the oak ones failed inspection, they'll come the week after.
Zainab: Okay, thirty eight Friday, two the week after. Can you send me the updated delivery note so the warehouse has it?
Hollis: I'll email it over within the hour.
Zainab: Thanks. That's all I needed.
Hollis: Cheers.
`,
  notes:{
    sparse:`fri delivery pm`,
    detailed:`# Delivery\n- Friday, afternoon 2-4 (morning truck full)\n- 38 crates not 40\n\n# Follow-up\n- Hollis sending updated delivery note\n- tell warehouse`,
  },
  gold:{
    decisions:[],
    actionItems:[
      {id:"delivery-note",owner:"Hollis",due:"within the hour",text:"Hollis emails the updated delivery note within the hour.",must:["delivery note"],ownerMust:near("Hollis","delivery note",100)},
    ],
    facts:[
      {id:"slot",text:"Friday's delivery moved to the afternoon slot, between two and four.",must:["Friday","afternoon|2.{0,6}4|two and four"]},
      {id:"crates",text:"38 crates arrive Friday instead of 40; two oak crates failed inspection and follow the week after.",must:["38|thirty.eight"]},
    ],
    notSaid:[
      {id:"forty",claim:"Forty crates arrive on Friday.",forbid:["(?:40|forty) crates (?:will |are |to )?(?:arriv|deliver|com)","deliver(?:y|ing) (?:of )?(?:40|forty) crates"]},
      {id:"morning",claim:"The delivery is in the morning.",forbid:["(?:delivery|arriv\\w+|slot)[^.\\n]{0,25}(?:is|in|on) (?:the )?(?:Friday )?morning(?! (?:truck|slot) (?:is|was) full)"]},
    ],
  },
}
