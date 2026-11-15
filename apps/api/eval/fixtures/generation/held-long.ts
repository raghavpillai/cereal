import type {GenerationFixture} from "../../lib/generation-fixtures"
import {near} from "../../lib/transcript"
import {day} from "../../lib/generation-patterns"
import {HELD_LONG_SCRIPT} from "./held-long.script"

/** HELD OUT: never used while tuning the generation prompt.
 * About 90 minutes and 13,000 words around one table, recorded on a phone:
 * thirteen agenda items, an opening date that is defended early and moved an
 * hour later, a corrected estimate, a pledge that is not accepted, a task
 * nobody takes and a tangent. No calendar event, so names come only from speech.
 * Fictional aquarium (Pelican Point), foundation and people. */
export const heldLong:GenerationFixture={
  id:"held-long",split:"held-out",stress:"13,000 words in one room, 13 agenda items, an early decision reversed late, a corrected estimate, an unowned task",
  title:"Ocean wing steering group",kind:"project steering group (about 90 minutes)",daysAgo:9,capture:"ios",wpm:150,
  people:{Imogen:"mic",Tavish:"mic",Paloma:"mic",Henrik:"mic",Adaeze:"mic",Luca:"mic"},
  questions:[],
  script:HELD_LONG_SCRIPT,
  notes:{
    sparse:`panel 5 wks late\nopening -> Jul 17 (NOT Jun 12)\nHartwell 500k - naming rights -> board\ntickets 38\ntraining plan - no owner`,
    detailed:`# Status\n- 58% complete\n- budget 31.5M, spent 17.9M\n- contingency 1.5M left of 2.4M\n\n# Main tank\n- acrylic panel 5 wks late, arrives Apr 27\n- cure 3 wks, then water, then sharks need 30 days\n- earliest animals on exhibit Jul 6\n- ozone unit failed inspection, replacement 3 wks\n\n# Opening\n- moved to Fri Jul 17, no double shifts\n- announce Mar 9, member preview Jul 14-16\n- lost admissions ~250k\n\n# Money\n- raised 7.35M of 9M\n- Hartwell 500k wants touch pool naming - board Mar 5\n- adult ticket 32 -> 38, kids stay 22\n- cafe ventilation 220k approved, lobby floor 95k no\n\n# People + programs\n- 14 new staff (6 aquarists, 8 guest services)\n- guest services training plan has no owner - raise again\n- school bookings open Aug 3, 120/day\n- thought: Luca took the floor decision hard, check in\n\n# Mine\n- revised milestone list tomorrow`,
  },
  gold:{
    decisions:[
      {id:"opening-july",text:"Final: the opening moves to Friday, July 17, with no double shifts (reversing the earlier call to hold June 12 by paying for double shifts).",must:[day("July",17)]},
      {id:"no-double-shifts",text:"Double shifts are cancelled.",must:["double shift"]},
      {id:"tickets",text:"Adult admission goes from $32 to $38 when the wing opens; children stay at $22 (not $40).",must:["38","22"]},
      {id:"change-orders",text:"The café ventilation change order ($220,000) is approved; the lobby flooring upgrade ($95,000) is rejected.",must:["ventilation","floor"]},
      {id:"naming-board",text:"The Hartwell Foundation's $500,000 pledge with touch-pool naming rights is not accepted in the room; it goes to the board.",must:["Hartwell","board"]},
      {id:"school-bookings",text:"School bookings open August 3, capped at 120 students a day.",must:[day("August",3),"120|one hundred twenty"]},
      {id:"announce",text:"The new date is announced publicly on March 9, not before; member preview days are July 14 to 16.",must:[day("March",9)]},
    ],
    actionItems:[
      {id:"reinspection",owner:"Tavish",due:"Wednesday",text:"Tavish sends Imogen the ozone-unit reinspection date by Wednesday.",must:["reinspection|re.inspection","Wednesday"],ownerMust:near("Tavish","inspection",100)},
      {id:"board-agenda",owner:"Adaeze",due:"March 5",text:"Adaeze puts the Hartwell naming-rights pledge on the March 5 board agenda.",must:["board",day("March",5)],ownerMust:near("Adaeze","board|Hartwell|naming",100)},
      {id:"aquarist-roles",owner:"Paloma",due:"this week",text:"Paloma posts the six aquarist roles this week.",must:["aquarist"],ownerMust:near("Paloma","aquarist|roles",100)},
      {id:"training-plan",owner:"nobody",unassigned:true,text:"Someone has to write the guest-services training plan; nobody took it.",must:["training plan"],ownerMust:""},
      {id:"donors",owner:"Adaeze",due:"Friday",text:"Adaeze calls the top ten donors by Friday.",must:["donor","Friday"],ownerMust:near("Adaeze","donor",100)},
      {id:"cancel-shifts",owner:"Tavish",due:"today",text:"Tavish cancels the double-shift request with the contractor today.",must:[near("double shift","cancel",100)],ownerMust:near("Tavish","double shift|cancel",100)},
      {id:"launch-calendar",owner:"Luca",due:"next week",text:"Luca redoes the launch calendar around July 17 and brings it next week.",must:["launch calendar|calendar"],ownerMust:near("Luca","calendar",100)},
      {id:"grant",owner:"Adaeze",due:"April 1",text:"Adaeze writes the state transport grant application ($60,000), due April 1.",must:["grant",day("April",1)],ownerMust:near("Adaeze","grant",100)},
      {id:"milestones",owner:"Imogen",due:"tomorrow",text:"Imogen sends the revised milestone list tomorrow.",must:["milestone"],ownerMust:near("Imogen","milestone",100)},
    ],
    facts:[
      {id:"progress",text:"The project is 58% complete; total budget $31.5 million, $17.9 million spent.",must:["58|fifty.eight","31\\.5|thirty.one and a half","17\\.9"]},
      {id:"panel",text:"The main acrylic panel is five weeks late: it arrives April 27 instead of March 23.",must:[day("April",27)]},
      {id:"double-shift-cost",text:"Double shifts for six weeks would have cost about $180,000.",must:["180"]},
      {id:"tank",text:"The main tank holds 400,000 gallons; the filtration loop passed its pressure test.",must:["400,?000|400k|four hundred thousand"]},
      {id:"ozone",text:"The ozone unit failed inspection; the replacement is three weeks out.",must:["ozone"]},
      {id:"campaign",text:"The campaign has raised $7.35 million of its $9 million goal.",must:["7\\.35"]},
      {id:"contingency",text:"$1.5 million of the $2.4 million contingency is left.",must:["1\\.5","2\\.4"]},
      {id:"animal-schedule",text:"The panel must cure three weeks before filling, the water must stabilise and the sharks need 30 days in the tank: animals on exhibit July 6 at the earliest.",must:["cure|curing","30 days|thirty days|30.day",day("July",6)]},
      {id:"lost-admissions",text:"The later opening loses about five weeks of summer admissions, roughly $250,000 (not $600,000, which was the whole summer).",must:["250"]},
      {id:"staff",text:"The wing needs 14 new staff: six aquarists and eight in guest services.",must:["14|fourteen"]},
      {id:"waitlist",text:"212 schools are on the waitlist.",must:["212|two hundred twelve"]},
      {id:"next-meeting",text:"The next steering group is on Tuesday the 10th.",must:["Tuesday","10th|tenth|\\b10\\b"]},
    ],
    notSaid:[
      {id:"june-12",claim:"The wing opens on June 12 (reversed).",forbid:["(?:open(?:s|ing)?|hold|keep|stick(?:ing)? (?:to|with))[^.\\n]{0,30}June 12(?!\\d)(?! (?:is|was) (?:not|never|no longer|dropped|abandoned))(?![^.\\n]{0,40}(?:reversed|not possible|impossible|dropped|abandoned|no longer|moved))"]},
      {id:"double-shifts-on",claim:"Double shifts will run or were approved.",forbid:["(?:approved|agreed|will run|will pay for|paying for|run(?:ning)?) (?:the )?double shifts(?! (?:was|were|is) (?:reversed|cancelled|canceled|dropped))"]},
      {id:"six-hundred",claim:"The later opening costs $600,000 in admissions.",forbid:["(?:lose|loss|cost|costs|losing|lost)[^.\\n]{0,40}\\$?600,?000(?! (?:was|is) (?:the whole|for the whole|corrected|wrong))"]},
      {id:"forty",claim:"Adult tickets go to $40.",forbid:["(?:adult|admission|tickets?)[^.\\n]{0,30}(?:to|at|is|be) \\$?40(?!\\d|,)(?! (?:was|were) (?:rejected|proposed|suggested|not))"]},
      {id:"hartwell-accepted",claim:"The Hartwell pledge or naming rights were accepted.",forbid:["(?:accepted|approved|agreed to|secured|confirmed)[^.\\n;]{0,30}(?:Hartwell|naming rights)","Hartwell[^.\\n;]{0,40}(?:was|is|has been) (?:accepted|approved|secured|confirmed)"]},
      {id:"training-owned",claim:"A named person owns the guest-services training plan.",forbid:["(?:Imogen|Tavish|Paloma|Henrik|Adaeze|Luca)(?::| will| to| is going to| agreed to)[^.\\n]{0,20}(?:write|draft|own|create|prepare|develop)[^.\\n]{0,30}training plan"]},
      {id:"floor-approved",claim:"The lobby flooring upgrade was approved.",forbid:["(?:approved|agreed)[^.\\n]{0,25}(?:lobby )?floor(?:ing)?(?! upgrade (?:was )?(?:rejected|declined))","(?:lobby )?flooring upgrade (?:was |is )?approved"]},
      {id:"octopus",claim:"The octopus naming tangent produced a decision or a chosen name.",forbid:["octopus (?:will be|is|was) named","(?:decided|agreed|chose)[^.\\n]{0,30}octopus"]},
    ],
  },
}
