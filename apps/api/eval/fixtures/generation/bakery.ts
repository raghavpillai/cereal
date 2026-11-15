import type {GenerationFixture} from "../../lib/generation-fixtures"
import {near} from "../../lib/transcript"
import {day} from "../../lib/generation-patterns"

/** One two-minute meeting generated at each typed-notes level (none, sparse,
 * detailed), so what the author's notes do to the document can be compared with
 * everything else held equal. Every voice is system audio, as on a machine with
 * no microphone. Fictional co-op, mills and people. */
export const bakery:GenerationFixture={
  id:"hard-bakery",split:"tuned",stress:"one short meeting at the three typed-notes levels: none, sparse and detailed",
  title:"Harbourlight Bakery Co-op supplier review",kind:"quarterly supplier review (about two minutes)",daysAgo:1,capture:"desktop",wpm:150,
  people:{Ottoline:"system",Caspian:"system",Yusuf:"system"},
  questions:[],
  script:`
Ottoline: Welcome to the Harbourlight Bakery Co-op quarterly supplier review. I'm Ottoline Brackenridge, chairing. Caspian Mbeki, our head baker, and Yusuf Delacroix, our treasurer, are here.
Caspian: Caspian here. Sourdough sales are up eighteen percent this quarter. Rye is flat. The cinnamon knots sold out every Saturday.
Yusuf: Yusuf here. On flour. Graystone Mills charges us six hundred and fifty-five pounds a tonne. Windrush Organics has quoted six hundred and twelve pounds a tonne.
Caspian: I baked test loaves with the Windrush flour. The crumb was better, but the dough needed about five percent more water.
Ottoline: Then let's decide. We switch our flour supplier from Graystone Mills to Windrush Organics, starting the first of January.
Yusuf: Agreed. We use about four tonnes a month, so that saves one hundred and seventy-two pounds a month.
Caspian: Second topic. The old deck oven is failing. I want the Vulcan Mark Five. It costs twenty-four thousand five hundred pounds.
Yusuf: We can afford that if we spread it over three years. I'm worried about the installation downtime though.
Caspian: Installation takes two days. I suggest the ninth and tenth of December, which are our quietest days.
Ottoline: Approved. We buy the Vulcan Mark Five and install it on the ninth of December.
Ottoline: Actions. Yusuf will sign the Windrush contract by the twentieth of November. Caspian will adjust the sourdough recipe for the extra water. I will tell Graystone Mills we are leaving.
Yusuf: One open question. We still have not decided whether to raise the price of the cinnamon knots.
Ottoline: Let's park that until January. Thanks everyone.
`,
  notes:{
    sparse:`flour switch\noven\nactions`,
    detailed:`## Oven first\nvulcan mk5 24.5k - 3 yrs\ninstall 9 dec\n## Flour\nwindrush 612 vs graystone 655\nMY TAKE: push Windrush for a 2 yr price lock\n## Money\ncinnamon knots price?? parked`,
  },
  gold:{
    decisions:[
      {id:"flour-switch",text:"Flour supplier switches from Graystone Mills to Windrush Organics from 1 January.",must:["Windrush",`${day("January",1)}|1st Jan|start of (?:the )?year|new year`]},
      {id:"oven",text:"Approved: buy the Vulcan Mark 5 (£24,500 over three years), installed on 9 (and 10) December.",must:["Vulcan",day("December",9)]},
      {id:"knots-parked",text:"The cinnamon knots price question is parked until January.",must:["cinnamon knot","January|parked|deferred"]},
    ],
    actionItems:[
      {id:"contract",owner:"Yusuf",due:"20 November",text:"Yusuf signs the Windrush contract by 20 November.",must:["contract",day("November",20)],ownerMust:near("Yusuf","contract",100)},
      {id:"recipe",owner:"Caspian",text:"Caspian adjusts the sourdough recipe for the extra water.",must:["recipe"],ownerMust:near("Caspian","recipe",100)},
      {id:"tell-graystone",owner:"Ottoline",text:"Ottoline tells Graystone Mills the co-op is leaving.",must:[near("Graystone","tell|notif|inform|let .{0,15}know|leaving|departure|switch",120)],ownerMust:near("Ottoline",near("Graystone","tell|notif|inform|let|leaving|departure",80),140)},
    ],
    facts:[
      {id:"sales",text:"Sourdough sales are up 18% this quarter; rye is flat; cinnamon knots sold out every Saturday.",must:["18 ?%|eighteen percent"]},
      {id:"prices",text:"Graystone charges £655 a tonne; Windrush quoted £612.",must:["655","612"]},
      {id:"saving",text:"At about four tonnes a month the switch saves £172 a month.",must:["172"]},
      {id:"water",text:"Windrush test loaves had a better crumb but the dough needed about 5% more water.",must:["5 ?%|five percent","water"]},
      {id:"oven-cost",text:"The Vulcan Mark 5 costs £24,500, spread over three years; installation takes two days.",must:["24,?500|24\\.5k","3 years|three years|3 yrs|3.year"]},
    ],
    notSaid:[
      {id:"price-lock-agreed",claim:"A two-year price lock with Windrush was discussed or agreed (it is only the author's private note).",forbid:["(?:agreed|decided|secured|negotiated|confirmed|discussed)[^.\\n]{0,30}price lock","price lock (?:was|is) (?:agreed|secured|confirmed|discussed|approved)","Windrush (?:offered|agreed to|will provide)[^.\\n]{0,20}price lock"]},
      {id:"knots-raised",claim:"The cinnamon knots price was raised.",forbid:["(?:raise|raising|increase|increasing)d? the (?:price of (?:the )?)?cinnamon knots?(?: price)? (?:was |is )?(?:agreed|approved|decided)","(?:agreed|decided|approved) to (?:raise|increase)[^.\\n]{0,25}cinnamon"]},
      {id:"graystone-stays",claim:"The co-op stays with Graystone.",forbid:["(?:stay(?:ing)?|remain(?:ing)?|continue) with Graystone"]},
    ],
  },
}
