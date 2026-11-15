import type {GenerationFixture} from "../../lib/generation-fixtures"
import {near} from "../../lib/transcript"
import {day} from "../../lib/generation-patterns"
import {LONG_SCRIPT} from "./long.script"

/** About 100 minutes and 14,700 words on a video call: fourteen agenda items,
 * eight decisions (one of them reversed an hour later), eleven action items,
 * a corrected figure and a tangent. Stresses the output cap, what a long note
 * drops, and whether the final state of a reversed decision is reported.
 * Fictional retailer (Tidewater Outfitters), carriers and people. */
export const long:GenerationFixture={
  id:"hard-long",split:"tuned",stress:"14,700 words, 14 agenda items, dense decisions and numbers, a decision reversed an hour later, a corrected figure",
  title:"FY27 operating plan review",kind:"annual operating plan review (about 100 minutes)",daysAgo:7,capture:"desktop",wpm:150,
  people:{Corinne:"mic",Raj:"system",Mateus:"system",Ottilie:"system",Yevgenia:"system",Dominic:"system",Wren:"system"},
  attendees:[{name:"Corinne Abara",email:"corinne@tidewater.example",self:true},{name:"Raj Venkataraman",email:"raj@tidewater.example"},{name:"Mateus Oliveira",email:"mateus@tidewater.example"},{name:"Ottilie Brandvold",email:"ottilie@tidewater.example"},{name:"Yevgenia Sokolova",email:"yevgenia@tidewater.example"},{name:"Dominic Achterberg",email:"dominic@tidewater.example"},{name:"Wren Castillo",email:"wren@tidewater.example"}],
  questions:[],
  script:LONG_SCRIPT,
  notes:{
    sparse:`Q4 beat, margin down - freight\nplan = 96 not 100\napp slips to Apr 14\nMissoula lease - not 10 yrs\nHalcyon - check the draft!!\nmktg 4.9`,
    detailed:`# Q4\n- rev 23.4 vs plan 22.1\n- GM 41.2% (43 last yr) - freight\n\n# FY27 plan\n- plan at 96M, 100 = stretch only\n- mktg 4.9M, 300k held back until 150k installs\n\n# App\n- launch moves Mar 10 -> Apr 14 (payments cert)\n- beta Mar 17, 2,000 loyalty members\n\n# Stores\n- Bend opens May 2, build-out 640k\n- Missoula: landlord wants 10 yrs, we want 7 + break at yr 5\n\n# Supply chain\n- Halcyon: NOT 2 yrs. 1 yr only if fuel surcharge capped\n- 62% outerwear from one factory (Da Nang) -> Porto sample order approved\n- inventory system 1.3M, go-live Jul 6\n\n# Customers\n- returns window 60 -> 45 days from Feb 1\n- support: 4 hires now, 2 more reviewed in June\n- paid loyalty tier: not decided, want a test design\n- my read: Raj will keep pushing 30 days, Wren won't budge below 45\n\n# Mine\n- send decisions list tonight`,
  },
  gold:{
    decisions:[
      {id:"plan-96",text:"The year is planned at $96 million; $100 million is a stretch goal that does not go in the budget.",must:["96","100|hundred","stretch"]},
      {id:"halcyon-final",text:"Final: no two-year Halcyon deal. One year only, and only if the fuel surcharge is capped (reversing the earlier agreement to take the two-year renewal).",must:["Halcyon","one.year|1.year|one year|12.month","cap"]},
      {id:"app-date",text:"The app launches publicly on April 14 (moved from March 10), with a beta for 2,000 loyalty members on March 17.",must:[day("April",14),day("March",17)]},
      {id:"missoula",text:"Missoula: Corinne will not sign a ten-year lease; the counter is seven years with a break option at year five. Not signed.",must:["Missoula","seven|7","break"]},
      {id:"returns-45",text:"The returns window becomes 45 days from February 1 (was 60; 30 was proposed).",must:["45|forty.five",day("February",1)]},
      {id:"marketing",text:"Marketing budget is $4.9 million, with $300,000 held back until the app passes 150,000 installs.",must:["4\\.9","300,?000|300k|three hundred thousand","150,?000|150k|one hundred fifty thousand"]},
      {id:"porto",text:"A second outerwear supplier in Porto is approved for qualification by September, starting with a 4,000-unit sample order.",must:["Porto","4,?000|four thousand"]},
      {id:"support-hires",text:"Support gets four hires now; the other two are reviewed in June after the app launch.",must:["four|\\b4\\b","June"]},
      {id:"paid-tier",text:"A paid loyalty tier is not decided; Corinne wants a test design first.",must:["paid tier|paid loyalty|paid membership","test"]},
    ],
    actionItems:[
      {id:"launch-plan",owner:"Mateus",due:"Friday",text:"Mateus sends Corinne the revised app launch plan by Friday.",must:["launch plan","Friday"],ownerMust:near("Mateus","launch plan",100)},
      {id:"missoula-landlord",owner:"Ottilie",due:"next review",text:"Ottilie takes the seven-year/break-option counter to the Missoula landlord and reports at the next review.",must:["landlord"],ownerMust:near("Ottilie","landlord|Missoula|lease",100)},
      {id:"footwear-legal",owner:"Wren",due:"this week",text:"Wren asks legal this week about refusing returns of worn footwear.",must:["legal","footwear"],ownerMust:near("Wren","legal",100)},
      {id:"channel-breakdown",owner:"Dominic",due:"next Wednesday",text:"Dominic sends the channel breakdown at $4.9 million by next Wednesday.",must:["channel breakdown|channel plan|breakdown","Wednesday"],ownerMust:near("Dominic","breakdown|channel",100)},
      {id:"porto-sample",owner:"Yevgenia",text:"Yevgenia places the Porto sample order.",must:["sample order|sample"],ownerMust:near("Yevgenia","sample|Porto",100)},
      {id:"requisitions",owner:"Wren",due:"Monday",text:"Wren opens the four support requisitions on Monday.",must:["requisition|open(?:s|ing)? (?:the )?(?:four|4) (?:roles|positions|reqs)","Monday"],ownerMust:near("Wren","requisition|roles|positions|reqs",100)},
      {id:"halcyon-renegotiate",owner:"Yevgenia",text:"Yevgenia goes back to Halcyon for a one-year term with a surcharge cap.",must:[near("Halcyon","cap|one.year|1.year|one year|renegotiat|go(?:es)? back",120)],ownerMust:near("Yevgenia","Halcyon",120)},
      {id:"brightline",owner:"Yevgenia",due:"the 20th",text:"Yevgenia gets a comparison quote from Brightline Cargo by the 20th.",must:["Brightline","20th|twentieth"],ownerMust:near("Yevgenia","Brightline",120)},
      {id:"test-design",owner:"Mateus and Dominic",due:"February review",text:"Mateus and Dominic bring a paid-tier test design to the February review.",must:["test design","February"],ownerMust:near("Mateus|Dominic","test design",100)},
      {id:"business-case",owner:"Raj",due:"end of January",text:"Raj circulates the revised inventory-system business case by the end of January.",must:["business case","January"],ownerMust:near("Raj","business case",100)},
      {id:"decisions-list",owner:"Corinne",due:"tonight",text:"Corinne (the note owner) sends the decisions list tonight.",must:["decisions list|list of decisions"],ownerMust:near("Corinne","decisions",100)},
    ],
    facts:[
      {id:"q4-revenue",text:"Q4 revenue was $23.4 million against a plan of $22.1 million.",must:["23\\.4","22\\.1"]},
      {id:"margin",text:"Gross margin was 41.2%, down from 43% last year, almost entirely because of freight.",must:["41\\.2","freight"]},
      {id:"halcyon-offer",text:"Halcyon offered a two-year renewal at 4% below the current rate; the draft carries an uncapped fuel surcharge that would have wiped out the discount at last year's fuel prices.",must:["4 ?%|four percent","surcharge"]},
      {id:"cert",text:"The app slipped because the payments certification slipped.",must:["certification"]},
      {id:"bend",text:"The Bend store is on track to open May 2; the build-out is coming in at $640,000.",must:[day("May",2),"640"]},
      {id:"return-rate",text:"Last year's return rate was 14.8%; footwear alone was 22%.",must:["14\\.8","22"]},
      {id:"marketing-ask",text:"Dominic asked for $5.2 million; Raj offered $4.6 million.",must:["5\\.2","4\\.6"]},
      {id:"concentration",text:"62% of outerwear comes from one factory in Da Nang; Porto samples cost about 9% more per unit.",must:["62","Da Nang","9 ?%|nine percent"]},
      {id:"response-time",text:"Average first response time is 19 hours against a target of 8; Wren asked for six agents.",must:["19|nineteen","8|eight"]},
      {id:"loyalty",text:"There are 410,000 loyalty members, about a third of whom bought twice or more last year; the tier floated was $49 a year.",must:["410","49"]},
      {id:"inventory",text:"The inventory system upgrade costs $1.3 million (not $1.8 million, which included a warehouse module that was cut); go-live is July 6, with no cutover in August.",must:["1\\.3",day("July",6)]},
      {id:"next-review",text:"The next review is Thursday, February 19.",must:[day("February",19)]},
    ],
    notSaid:[
      {id:"two-year-final",claim:"Tidewater will sign (or agreed to) the two-year Halcyon renewal (that was reversed).",forbid:["(?:agreed|decided|approved|will|to) (?:to )?(?:take|sign|accept|renew)[^.\\n]{0,40}(?:two.year|2.year|two year)(?! (?:deal|renewal|term)? ?(?:was|is) (?:reversed|dropped|off|rejected))","(?:two.year|2.year|two year) (?:deal|renewal|term|contract) (?:was |is )?(?:approved|agreed|accepted|signed|confirmed)(?! (?:earlier|initially|at first|then reversed))"]},
      {id:"paperwork",claim:"Yevgenia is starting the two-year paperwork this week (superseded).",forbid:["Yevgenia(?: Sokolova)?(?::| will| to| is going to)[^.\\n]{0,20}(?:start|begin|get)[^.\\n]{0,25}paperwork"]},
      {id:"march-10",claim:"The app launches on March 10.",forbid:["(?:app|launch)[^.\\n]{0,25}(?:on|is|set for|scheduled for) March 10(?!\\d)(?! (?:is|was) (?:not|no longer))"]},
      {id:"plan-100",claim:"The plan (budget) is $100 million.",forbid:["(?:plan|budget)(?:ned|s)? (?:the year )?(?:at|is|of) \\$?100(?: million|M)"]},
      {id:"thirty-days",claim:"The returns window is 30 days.",forbid:["(?:returns?|window)[^.\\n]{0,30}(?:cut|reduced|shortened|set|changed|moves?|goes)[^.\\n]{0,15}to (?:30|thirty) days","30.day (?:returns? )?window (?:was |is )?(?:approved|agreed|decided)"]},
      {id:"worn-footwear",claim:"Refusing worn-footwear returns was decided.",forbid:["(?:decided|agreed|approved)[^.\\n]{0,30}(?:refus|reject|no longer accept)[^.\\n]{0,25}(?:worn )?footwear","worn footwear (?:will|is|are) no longer (?:be )?(?:returnable|accepted)"]},
      {id:"marketing-52",claim:"The marketing budget is $5.2 million or $4.6 million.",forbid:["marketing budget (?:is|was|of|at|set at|approved at) \\$?(?:5\\.2|4\\.6)"]},
      {id:"missoula-signed",claim:"The Missoula lease is signed or an opening date is set.",forbid:["Missoula[^.\\n]{0,25}(?:lease )?(?:is|was|has been) signed","Missoula (?:opens|will open|opening) (?:on|in) "]},
      {id:"six-hires",claim:"Six support hires were approved.",forbid:["(?:approved|agreed|decided)[^.\\n]{0,20}(?:six|6) (?:hires|agents|new agents|requisitions|roles)"]},
      {id:"paid-tier-decided",claim:"A paid loyalty tier at $49 was approved or will launch.",forbid:["paid (?:loyalty )?tier (?:was |is )?(?:approved|agreed|decided|launch)","(?:launch|introduc)(?:e|ing) (?:a |the )?paid (?:loyalty )?tier"]},
      {id:"upgrade-18",claim:"The inventory upgrade costs $1.8 million.",forbid:["(?:upgrade|system|inventory)[^.\\n]{0,30}(?:costs?|is|at|budget(?:ed)?|priced)[^.\\n]{0,12}\\$?1\\.8(?: million|M)(?! (?:included|with|was))"]},
      {id:"parking",claim:"The all-hands or parking tangent produced a decision.",forbid:["(?:decided|agreed)[^.\\n]{0,30}(?:all.hands|parking)"]},
    ],
  },
}
