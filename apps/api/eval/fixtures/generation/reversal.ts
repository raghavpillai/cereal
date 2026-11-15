import type {GenerationFixture} from "../../lib/generation-fixtures"
import {near} from "../../lib/transcript"
import {day} from "../../lib/generation-patterns"

/** Vendor selection in which the first decision is overturned twenty minutes
 * later, an owner is swapped, a deadline moves and a number is corrected.
 * Fictional company (Lantern Foods), vendors (Corvid Packaging, Pallas Pack) and people. */
export const reversal:GenerationFixture={
  id:"hard-reversal",split:"tuned",stress:"a decision reversed late, a reassigned task, a moved deadline, a corrected number, a tangent",
  title:"Pouch vendor decision",kind:"vendor selection meeting",daysAgo:3,capture:"desktop",wpm:150,
  people:{Odette:"mic",Bram:"system",Keiko:"system",Leandro:"system"},
  attendees:[{name:"Odette Marchetti",email:"odette@lanternfoods.example",self:true},{name:"Bram Castellanos",email:"bram@lanternfoods.example"},{name:"Keiko Arai",email:"keiko@lanternfoods.example"},{name:"Leandro Pires",email:"leandro@lanternfoods.example"}],
  questions:[],
  script:`
Odette: Okay, let's get going. One topic today, the compostable pouch vendor for the muesli launch. We have two quotes on the table. Keiko, do you want to run through them?
Keiko: Sure. Corvid Packaging came in at eleven point two cents a unit, minimum order two hundred fifty thousand units. Pallas Pack is twelve point six cents a unit with a minimum of one hundred thousand.
Odette: So Corvid is cheaper by a fair bit.
Keiko: One point four cents a unit cheaper, yes. And their print quality on the samples was better, the green is closer to our brand green.
Bram: I liked the Corvid samples too. The zipper feels sturdier.
Odette: Leandro, from the design side?
Leandro: Corvid, no question. Their matte finish holds the ink better. Pallas looked a little washed out on the first proof.
Odette: Then I think this is easy. Let's go with Corvid. Any objections?
Bram: None from me.
Keiko: Fine by me.
Odette: Great, Corvid it is. Leandro, that means artwork is due to them January sixteenth, right?
Leandro: January sixteenth, yes, that's what their rep said.
Odette: Okay. While I have everyone, completely unrelated, did anybody hear back about the trade show booth? I saw an email about a corner spot.
Bram: Yeah, the organizers offered us the corner booth for an extra eight hundred dollars. I haven't answered. Honestly I'd rather spend that on samples to hand out.
Odette: Let's not solve that today, park it. I just didn't want to forget. Back to pouches.
Bram: Actually, before we close this, I went back through the Corvid quote this morning and there's a problem. Their lead time is fourteen weeks from approved artwork.
Odette: Fourteen. I thought it was eight.
Bram: Eight was the old quote, before they moved production. Fourteen weeks from January sixteenth puts delivery in late April. The retail reset is March second. We'd miss it by almost two months.
Keiko: That's a real problem. If we miss the reset we don't get shelf space until the fall.
Odette: What's the Pallas lead time?
Keiko: Six weeks from approved artwork.
Odette: So with Pallas we could approve artwork later and still make March second.
Keiko: Easily. And because their minimum is lower we don't have to buy two hundred fifty thousand pouches up front.
Leandro: I still don't love their print. But I'd rather have slightly washed out pouches on the shelf than perfect pouches in a warehouse in April.
Odette: Right. Okay, I'm changing my mind. We go with Pallas for the launch order. How many units?
Keiko: I'd order one hundred fifty thousand. That covers the first two retail cycles.
Odette: One hundred fifty thousand from Pallas for launch. And we look at Corvid again for the second order once the timing pressure is off. Does anyone disagree?
Bram: No, that's the right call.
Leandro: Agreed.
Odette: So to be clear for the notes, we are not going with Corvid for launch. What does this do to cost?
Keiko: One point four cents more per unit, times one hundred fifty thousand, so about twenty one thousand dollars more.
Bram: That's not right. One point four cents, not dollars. It's two thousand one hundred dollars.
Keiko: Sorry, yes. Two thousand one hundred dollars more than Corvid would have been for the same quantity. Not twenty one thousand.
Odette: Two thousand one hundred I can live with. Now, somebody needs to get a contract out of Pallas. Leandro, can you call them?
Leandro: I can, but I'm out Thursday and Friday. Keiko, could you take it? You already have the contact.
Keiko: Yes, I'll take it. I'll get the contract draft from Pallas by Wednesday.
Odette: Thanks. And the artwork date moves, I assume.
Leandro: With six weeks lead time we can deliver artwork January twenty third and still be comfortable. So forget January sixteenth, it's the twenty third now.
Odette: Good, that gives you a week back. Bram, the launch plan needs the new dates.
Bram: I'll update the launch plan with the Pallas dates by Monday.
Leandro: And I'll send the artwork files to the Pallas prepress team as soon as Keiko has the contract signed.
Odette: I'll email Corvid today and tell them we're pausing, and that we'd like to talk again about a second order.
Keiko: One open thing. Pallas asked whether we want matte or gloss. Their matte is the one that looked washed out.
Leandro: I don't want to decide that off one proof. They're sending new samples, they arrive next Tuesday.
Odette: Then we decide the finish after the samples. Not today. Anything else?
Bram: That's it from me.
Odette: Thanks everyone.
`,
  notes:{
    sparse:`pouch vendor\nCorvid 11.2c vs Pallas 12.6c\nlead time!!\nfinish - undecided`,
    detailed:`# Quotes\n- Corvid 11.2c/unit, min 250k\n- Pallas 12.6c/unit, min 100k\n- Corvid print + zipper better (everyone agreed)\n\n# Decision\n- Pallas for launch, 150k units\n- Corvid lead time 14 wks -> misses Mar 2 reset\n- revisit Corvid for 2nd order\n\n# Cost\n- +$2,100 vs Corvid\n\n# Open\n- matte vs gloss - wait for samples Tues\n- trade show corner booth +$800 (parked)\n- my worry: Leandro not happy w/ Pallas print, watch the proof\n\n# To do\n- me: email Corvid today\n- Keiko: Pallas contract draft by Wed`,
  },
  gold:{
    decisions:[
      {id:"pallas-launch",text:"Final decision: Pallas Pack supplies the launch order of 150,000 pouches (reversing the earlier choice of Corvid).",must:["Pallas","150,?000|150k|one hundred fifty thousand"]},
      {id:"corvid-later",text:"Corvid is not used for launch; it will be reconsidered for the second order.",must:["Corvid","second order|2nd order|later order|next order|reorder"]},
      {id:"finish-open",text:"Matte versus gloss is not decided; it waits for the new Pallas samples arriving next Tuesday.",must:["matte|gloss|finish","sample"]},
    ],
    actionItems:[
      {id:"contract",owner:"Keiko",due:"Wednesday",text:"Keiko gets the contract draft from Pallas by Wednesday (Leandro was asked first and handed it over).",must:["contract","Wednesday"],ownerMust:near("Keiko","contract",100)},
      {id:"launch-plan",owner:"Bram",due:"Monday",text:"Bram updates the launch plan with the Pallas dates by Monday.",must:["launch plan","Monday"],ownerMust:near("Bram","launch plan",100)},
      {id:"artwork-files",owner:"Leandro",text:"Leandro sends the artwork files to the Pallas prepress team once the contract is signed.",must:["artwork","prepress|Pallas"],ownerMust:near("Leandro","artwork",100)},
      {id:"email-corvid",owner:"Odette",due:"today",text:"Odette emails Corvid today to say the order is paused and that a second order may follow.",must:[near("Corvid","email|tell|notif|inform|let .{0,20}know|paus",120)],ownerMust:near("Odette",near("Corvid","email|tell|notif|inform|let|paus",80),120)},
    ],
    facts:[
      {id:"quotes",text:"Corvid quoted 11.2 cents a unit with a 250,000 minimum; Pallas 12.6 cents with a 100,000 minimum.",must:["11\\.2","12\\.6"]},
      {id:"lead-times",text:"Corvid's lead time is 14 weeks from approved artwork (not 8); Pallas's is 6 weeks.",must:["14 weeks|fourteen weeks|14 wks|14 week","6 weeks|six weeks|6 wks|6 week"]},
      {id:"reset",text:"The retail reset is March 2; with Corvid delivery would slip to late April.",must:[day("March",2)]},
      {id:"cost",text:"Choosing Pallas costs about $2,100 more than Corvid for 150,000 units (1.4 cents a unit).",must:["2,?100"]},
      {id:"artwork-date",text:"Artwork is now due January 23 (moved from January 16).",must:[`${day("January",23)}|the 23rd|twenty.third`]},
      {id:"booth",text:"The trade-show corner booth offer ($800 extra) was parked, not decided.",must:["booth"]},
    ],
    notSaid:[
      {id:"corvid-chosen",claim:"Corvid was chosen for the launch order (that was the first decision, later reversed).",forbid:["(?:go|going|went|proceed(?:ing)?) with Corvid(?! for the second| again| later)","Corvid (?:was|is|has been) (?:selected|chosen|approved|picked)(?! for the second| at first| initially| first| earlier| originally)","(?:selected|chose|chosen|picked|approved) Corvid(?! for the second)"]},
      {id:"artwork-16",claim:"Artwork is due January 16 (superseded by January 23).",forbid:["artwork[^.\\n;]{0,40}(?:due|deadline)[^.\\n;]{0,12}(?:January|Jan) 16(?!\\d)","(?:due|deadline)(?: is| on| by|:| date)*(?: on| is)? (?:January|Jan) 16(?:th)?(?! to| →|->| date superseded)"]},
      {id:"cost-21k",claim:"The extra cost is $21,000 (corrected to $2,100).",forbid:["\\$?21,?000 (?:more|extra|increase|higher)","(?:costs?|increase|premium|difference|more)[^.\\n]{0,30}\\$?21,?000"]},
      {id:"leandro-contract",claim:"Leandro is getting the Pallas contract (he handed it to Keiko).",forbid:["Leandro[^.\\n]{0,40}(?:will|to|:)[^.\\n]{0,30}(?:get|call|obtain|draft|negotiate)[^.\\n]{0,30}contract"]},
      {id:"finish-decided",claim:"The finish was decided (matte or gloss).",forbid:["(?:decided|agreed|chose|selected|going)[^.\\n]{0,25}(?:on|with) (?:the )?(?:matte|gloss)(?! (?:vs|versus|or))"]},
      {id:"booth-decided",claim:"The corner booth was accepted or declined.",forbid:["(?:accepted|declined|took|booked|approved|rejected)[^.\\n]{0,30}(?:corner )?booth"]},
    ],
  },
}
