import type {GenerationFixture} from "../../lib/generation-fixtures"
import {near} from "../../lib/transcript"

/** HELD OUT: never used while tuning the generation prompt.
 * A volunteer committee in one room: the venue is chosen, then changed; the
 * ticket price is floated three ways before it settles; a booking changes hands;
 * a date is corrected; and there is a long tangent about last year's raffle.
 * Fictional town, venues and people. */
export const heldReversal:GenerationFixture={
  id:"held-reversal",split:"held-out",stress:"venue reversed, price settled after three proposals, task handed over, date corrected, tangent",
  title:"Harvest festival committee",kind:"volunteer committee meeting",daysAgo:5,capture:"ios",wpm:155,
  people:{Marguerite:"mic",Gus:"mic",Thandiwe:"mic",Osei:"mic",Pilar:"mic"},
  questions:[],
  script:`
Marguerite: Right, everyone's got tea. Three things tonight, venue, tickets, and the band. Venue first because everything hangs off it. Gus, you went to see Mill Park?
Gus: I did. It's lovely. The bandstand's been repainted and the council will let us have it for nothing as long as we clear up after.
Osei: Free is good. Last year we paid four hundred for the scout hut field.
Marguerite: And it holds enough people?
Gus: They said up to six hundred on the lawn.
Marguerite: Then I don't see why not. Shall we say Mill Park?
Thandiwe: I'm happy with Mill Park.
Pilar: Fine with me.
Marguerite: Mill Park then. Good. Tickets. Last year was eight pounds and we just about broke even.
Osei: I'd go to twelve. Everything's gone up, the marquee quote alone is nine hundred this year.
Pilar: Twelve is a lot for a family of four. That's forty eight pounds before anyone's bought a toffee apple.
Osei: Ten then?
Pilar: Ten for adults I could live with if children are cheaper.
Marguerite: Hold that thought. Before I forget, the raffle. Does anyone know what happened to the hamper that nobody claimed last year?
Gus: It sat in my garage until February. I ate the biscuits. The wine went to the Christmas tombola.
Thandiwe: You ate the biscuits.
Gus: They were going out of date. I maintain it was the responsible thing to do.
Marguerite: I'm not minuting that. Back to tickets.
Thandiwe: Actually, can I go back to the venue? I've been sitting here thinking. Mill Park has no cover at all. If it rains on the day we've got six hundred people and one bandstand.
Gus: We'd have the marquee.
Thandiwe: The marquee holds maybe a hundred and fifty. And the park licence needs a temporary event notice, which is ten working days minimum, and the council's licensing officer is away until the ninth.
Osei: That's cutting it fine.
Thandiwe: Riverside Hall has the hall itself plus the meadow behind it. If it's dry we're outside, if it's wet we move in. And it has its own licence, so no notice needed.
Marguerite: What does Riverside cost?
Thandiwe: Three hundred and fifty for the day, including the meadow.
Pilar: Honestly I'd pay three fifty not to lie awake checking the forecast.
Gus: I'll admit the rain worries me too. I just liked the bandstand.
Marguerite: So are we changing our minds? Riverside Hall instead of Mill Park. Hands up. That's everyone. Riverside Hall it is, not Mill Park. Gus, sorry.
Gus: No, it's the sensible thing.
Marguerite: Then tickets need to cover three hundred and fifty we weren't planning on. Where did we get to?
Pilar: Ten for adults, and I'd say five for children, under fives free.
Osei: I can make the budget work at ten and five if we sell four hundred tickets.
Marguerite: Ten pounds adults, five pounds children, under fives free. Agreed? Good. Not twelve, Osei.
Osei: Not twelve. I heard.
Marguerite: The date. I've been telling people Saturday the seventeenth.
Pilar: The seventeenth is a Friday. Saturday is the eighteenth.
Marguerite: Is it? Oh dear. Saturday the eighteenth, then. I'll have to correct the poster draft.
Thandiwe: I'll ring Riverside Hall tomorrow morning and put a hold on Saturday the eighteenth.
Marguerite: Thank you. Now the band. Gus, you were going to book the Ferrymen.
Gus: I was, but their drummer is my brother in law and we're not really speaking since the biscuits. Family thing. Thandiwe knows the singer.
Thandiwe: I can ask her. I'll book the Ferrymen, leave it with me.
Gus: Thank you. I'll do the marquee instead, I'll get a second quote by next Friday, because nine hundred seems steep.
Marguerite: Lovely. Osei, can you redo the budget with Riverside and the new prices before the next meeting?
Osei: Yes. I'll bring a revised budget to the next meeting.
Marguerite: And Pilar, the posters?
Pilar: I can't send anything to the printer until the venue's confirmed. Once Thandiwe says Riverside's held I'll finish the artwork.
Marguerite: Good. Next meeting is Tuesday the seventh, here, seven o'clock. Thank you all.
`,
  notes:{
    sparse:`venue - Riverside not Mill Park (rain)\ntickets 10/5\nSat 18th!!\nband -> Thandiwe`,
    detailed:`# Venue\n- Mill Park: free, 600 on lawn, no cover, needs temp event notice (10 working days)\n- Riverside Hall: £350 incl meadow, own licence, wet-weather option\n- changed to Riverside\n\n# Tickets\n- £10 adults / £5 children / under 5 free\n- Osei: works if we sell 400\n\n# Date\n- Saturday 18th (NOT 17th - fix poster)\n\n# Jobs\n- Thandiwe: hold Riverside, book Ferrymen\n- Gus: 2nd marquee quote\n- Osei: revised budget\n- Pilar: posters after venue confirmed\n- note to self: Gus a bit deflated about the bandstand, thank him properly`,
  },
  gold:{
    decisions:[
      {id:"venue",text:"Final venue: Riverside Hall (£350 including the meadow), replacing the earlier choice of Mill Park because of rain cover and licensing.",must:["Riverside"]},
      {id:"tickets",text:"Tickets: £10 adults, £5 children, under-fives free (not £12).",must:["10","5\\b|five"]},
      {id:"date",text:"The festival is on Saturday the 18th (the 17th is a Friday).",must:["18th|eighteenth|\\b18\\b"]},
    ],
    actionItems:[
      {id:"hold-venue",owner:"Thandiwe",due:"tomorrow morning",text:"Thandiwe rings Riverside Hall tomorrow morning to hold Saturday the 18th.",must:[near("Riverside","hold|ring|call|book|reserve|phone",120)],ownerMust:near("Thandiwe",near("Riverside","hold|ring|call|book|reserve|phone",80),140)},
      {id:"band",owner:"Thandiwe",text:"Thandiwe books the Ferrymen (taken over from Gus).",must:["Ferrymen|band"],ownerMust:near("Thandiwe","Ferrymen|band",100)},
      {id:"marquee",owner:"Gus",due:"next Friday",text:"Gus gets a second marquee quote by next Friday.",must:["marquee","quote"],ownerMust:near("Gus","marquee",100)},
      {id:"budget",owner:"Osei",due:"next meeting",text:"Osei brings a revised budget (Riverside, new prices) to the next meeting.",must:["budget"],ownerMust:near("Osei","budget",100)},
      {id:"posters",owner:"Pilar",text:"Pilar finishes the poster artwork once the venue hold is confirmed.",must:["poster|artwork"],ownerMust:near("Pilar","poster|artwork",100)},
      {id:"poster-date",owner:"Marguerite",text:"Marguerite corrects the date on the poster draft.",must:[near("poster","date|correct|18th|fix",100)],ownerMust:near("Marguerite","poster",120)},
    ],
    facts:[
      {id:"riverside-cost",text:"Riverside Hall costs £350 for the day including the meadow and has its own licence.",must:["350|three hundred and fifty|three fifty"]},
      {id:"mill-park-problem",text:"Mill Park has no rain cover and needs a temporary event notice (ten working days; the licensing officer is away until the 9th).",must:["rain|cover|wet|weather","notice|licen[cs]"]},
      {id:"break-even",text:"The budget works at £10/£5 if 400 tickets are sold.",must:["400|four hundred"]},
      {id:"marquee-quote",text:"The current marquee quote is £900.",must:["900|nine hundred"]},
      {id:"next-meeting",text:"Next meeting: Tuesday the 7th at seven o'clock.",must:["Tuesday","7th|seventh|\\b7\\b"]},
    ],
    notSaid:[
      {id:"mill-park-final",claim:"The festival will be at Mill Park (reversed).",forbid:["(?:festival|event|venue)[^.\\n:]{0,25}\\b(?:is|will be|at|in|held at)\\b(?! (?:stead|place) of)[^.\\n]{0,10}Mill Park(?! (?:was )?(?:dropped|reversed|rejected|replaced))","(?:chose|selected|confirmed|booked|agreed on) Mill Park","Mill Park (?:was|is) (?:confirmed|booked|the venue)"]},
      {id:"twelve",claim:"Tickets cost £12.",forbid:["(?:tickets?|adults?)[^.\\n]{0,25}(?:£|GBP )?12(?: pounds)?(?!\\w)(?! (?:was|were) (?:rejected|proposed|suggested))"]},
      {id:"seventeenth",claim:"The festival is on Saturday the 17th.",forbid:["Saturday(?: the)? 17(?:th)?","(?:festival|event)[^.\\n]{0,20}(?:on|is)(?: the)? 17th"]},
      {id:"gus-band",claim:"Gus is booking the band.",forbid:["Gus(?::| will| to| is going to)[^.\\n]{0,20}(?:book|contact|ask)[^.\\n]{0,20}(?:Ferrymen|band)"]},
      {id:"biscuits-minuted",claim:"The raffle hamper tangent produced a decision or action.",forbid:["(?:Gus|someone)(?::| will| to)[^.\\n]{0,25}(?:replace|repay|return)[^.\\n]{0,20}(?:hamper|biscuits)"]},
    ],
  },
}
