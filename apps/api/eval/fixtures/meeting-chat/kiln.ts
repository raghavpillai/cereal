import {near} from "../../lib/transcript"
import {ABSTAIN,type McFixture} from "./types"

/** HELD OUT: never tuned against. A wholesale negotiation recorded on one phone
 * on the table (a single channel, speaker numbers restarting every two
 * minutes), with transcription errors ("kill and co", "Mary Gold", "sell a
 * don"), a self-corrected quantity, a price that moves twice, a request that is
 * refused, facts that exist only in the typed notes, and no payment terms. */
export const kiln:McFixture={
  set:"held",id:"mc-h-kiln",title:"Kiln & Co / Marigold Cafés wholesale",kind:"negotiation on one phone, with transcription errors",daysAgo:2,capture:"ios",wpm:150,fast:true,
  people:{Bianca:"mic",Linh:"mic",Teodor:"mic"},
  attendees:[{name:"Bianca Ferro",email:"bianca@kilnandco.example",self:true},{name:"Linh Tran",email:"linh@kilnandco.example"},{name:"Teodor Vasile",email:"teodor@marigoldcafes.example"}],
  script:`
Bianca: Thanks for coming out to the studio, Teodor. So, um, this is Linh, she runs production, so anything about timing she's the one to ask.
Teodor: Great. Nice to meet you. So, uh, quick background on us. Mary Gold is fourteen cafes right now, and we're opening three more by September.
Bianca: Fourteen, going to seventeen.
Teodor: Right. And the reason I'm here is our cups. We buy the cheap white ones from a catalogue and, honestly, we break about nine percent of them a month in the dish washers.
Linh: Nine percent a month is a lot. What temperature are you washing at?
Teodor: I have no idea. Hot. The machines are the commercial kind.
Linh: Okay. Ours are stone where, fired high, so they take that fine. We've got them in two restaurants already and they've lost, like, a handful in a year.
Teodor: That's what I heard, that's why I called. So what I'm thinking for a first order is mugs and the small plates. Forty mugs a cafe. No, sorry, sixty, sixty mugs a cafe, because of the outdoor seating in summer. And thirty small plates a cafe.
Bianca: For the fourteen, or the seventeen?
Teodor: For the fourteen. The new ones I'd reorder for when they open.
Bianca: Okay. And colour, did you have something in mind?
Teodor: The green. The pale green one on your website. Everybody on my team keeps sending me that picture.
Linh: That's the sell a don glaze. It's our, kind of our house colour.
Teodor: That one. And I'd want our logo on the bottom. Stamped, not printed.
Linh: We can do a stamp on the base. There's a tooling fee for the stamp, it's one hundred eighty euros, one time.
Teodor: Okay. So let's talk price, because that's where this either works or it doesn't.
Bianca: Sure. So wholesale, the mug is eleven euros and the small plate is nine.
Teodor: Hm. At this volume I was hoping for nine fifty on the mug.
Bianca: Nine fifty I can't do. That's, I mean, that's close to what they cost us in clay and gas and Linh's time.
Teodor: What can you do?
Bianca: Let me come back to that in a second, because it depends on what Linh can make. Linh, capacity?
Linh: So the big kiln holds two hundred twenty pieces a firing, and we fire twice a week. Some of that is spoken for, the restaurants reorder every month or so.
Teodor: And how fast could I get a first batch?
Linh: First delivery is six weeks from deposit. That's the honest number. It's two weeks of throwing, then drying, then two firings, and I don't rush drying because that's how you get cracks.
Teodor: Six weeks. Okay. That's longer than the catalogue, obviously, but fine.
Bianca: And we take a thirty percent deposit when you place the order. The rest on delivery of each batch.
Teodor: Thirty percent. Okay. One thing my operations director will ask. Is the glaze lead free, and can you prove it?
Linh: It is. We had it tested at a lab in February. I can send you the certificate.
Bianca: Linh, can you send that over to him?
Linh: Yes. I'll send it tomorrow.
Teodor: Good. Now, exclusivity. If we put that green in seventeen cafes I don't want to walk into a competitor and see the same cup.
Bianca: I can't give you the glaze. It's on half of what we sell. What I can promise is that the shape with your stamp on it, that we will not sell to any other cafe.
Teodor: So no exclusivity on the colour, but the stamped shape is ours.
Bianca: Exactly.
Teodor: I can probably sell that internally. Okay. Back to price.
Bianca: So here's where I can get to. If you commit to twelve months, meaning you reorder from us for the new cafes and for replacements, I'll do the mug at ten twenty five. The plate stays at nine.
Teodor: Ten twenty five with a twelve month commitment.
Bianca: Yes.
Teodor: And the stamp fee? Can you waive that?
Bianca: If the order is signed by May fifteenth, I'll waive the stamp fee. After that it stands.
Teodor: That's fair. Okay. I think I can work with ten twenty five. I can't sign today, I have to take it to Ilka, she's the operations director.
Bianca: Of course.
Teodor: Delivery. If I sign in, say, the next two weeks, when do the mugs show up?
Linh: I'd split it. First batch of four hundred mugs by June sixteenth. The rest of the mugs and all the plates by July seventh.
Teodor: Four hundred by June sixteenth, rest by July seventh. And if some arrive broken?
Bianca: Anything broken in transit we replace free, as long as you tell us within seven days of delivery.
Teodor: Seven days. Good. Could I get a few to show people? It's much easier if Ilka can hold one.
Linh: I'll courier a sample set of six mugs to your Harbour Street cafe this week.
Teodor: Perfect. Then, uh, next steps on my side, I take this to Ilka and I'll reply by May ninth.
Bianca: And I'll send you the written quote by Wednesday so she has something to look at.
Teodor: Great. Thank you both, this was, this was easier than I expected.
Bianca: Thanks for coming, Teodor. Linh will walk you out past the kiln if you want to see it.
`,
  notes:{
    sparse:`Marigold Cafés - Teodor (buyer)\n- 14 cafés, +3 by Sept\n- he wants 9.50, our floor is 10!!\n- our cost per mug 4.10\n- do NOT give celadon exclusivity\n- ops director = Ilka Brandt`,
    detailed:`# Marigold Cafés wholesale\n\n## Them\n- Teodor (buyer), ops director = Ilka Brandt\n- 14 cafés, +3 by Sept\n- breaking 9%/month of catalogue cups\n\n## Order\n- 60 mugs + 30 plates per café\n- celadon glaze, logo stamp on base\n\n## Price\n- he wants 9.50, our floor is 10!!\n- our cost per mug 4.10\n- do NOT give celadon exclusivity\n\n## Next\n- quote by Wed`,
  },
  gold:{
    decisions:[
      {id:"mug-price",text:"Mugs at 10.25 euros each if Marigold commits to twelve months of reorders; small plates stay at 9 euros. Not signed yet.",must:["10[.,]25|ten twenty.five","twelve.month|12.month|12 months|twelve months"]},
      {id:"no-glaze-exclusivity",text:"No exclusivity on the celadon glaze; the logo-stamped shape will not be sold to any other café.",must:["exclusiv","stamp"]},
      {id:"stamp-fee",text:"The 180 euro stamp tooling fee is waived if the order is signed by May 15.",must:["180|one hundred eighty","May 15|15 May|fifteenth"]},
    ],
    actionItems:[
      {id:"linh-certificate",owner:"Linh",due:"tomorrow",text:"Linh sends the lead-free lab certificate tomorrow.",must:["certificate","tomorrow"],ownerMust:near("Linh","certificate")},
      {id:"linh-samples",owner:"Linh",due:"this week",text:"Linh couriers a sample set of six mugs to the Harbour Street café this week.",must:["sample","Harbour Street|six mugs|6 mugs"],ownerMust:near("Linh","sample")},
      {id:"bianca-quote",owner:"Bianca",due:"Wednesday",text:"Bianca sends the written quote by Wednesday.",must:["quote","Wednesday"],ownerMust:near("Bianca|\\bI\\b|\\byou\\b","quote")},
      {id:"teodor-reply",owner:"Teodor",due:"May 9",text:"Teodor takes the offer to Ilka, the operations director, and replies by May 9.",must:["Ilka|operations director","May 9|9 May|ninth"],ownerMust:near("Teodor","Ilka|repl|May 9|ninth")},
    ],
    facts:[
      {id:"cafes",text:"Marigold has 14 cafés and is opening three more by September.",must:["14|fourteen","three|\\b3\\b|17|seventeen"]},
      {id:"order",text:"First order: 60 mugs and 30 small plates per café, for the 14 existing cafés.",must:["60|sixty","30|thirty"]},
      {id:"list-prices",text:"List wholesale prices: 11 euros per mug, 9 per small plate. Teodor asked for 9.50 per mug.",must:["\\b11\\b|eleven","9[.,]50|nine fifty"]},
      {id:"lead-time",text:"First delivery is six weeks from deposit; the deposit is 30% on order.",must:["six weeks|6 weeks","30 ?%|thirty percent|30 percent"]},
      {id:"delivery",text:"First batch of 400 mugs by June 16; the rest of the mugs and all plates by July 7.",must:["400|four hundred","June 16|16 June|June sixteenth","July 7|7 July|July seventh"]},
      {id:"breakage",text:"Anything broken in transit is replaced free if reported within seven days of delivery.",must:["seven days|7 days"]},
    ],
    notSaid:[
      {id:"glaze-exclusive",claim:"Marigold was given exclusivity on the celadon glaze.",forbid:["(?:agreed|granted|gave|given|gets?|has|have)[^.\\n]{0,30}exclusiv(?:e|ity)[^.\\n]{0,25}(?:glaze|colou?r|green|celadon)"]},
      {id:"nine-fifty",claim:"The mug price was agreed at 9.50.",forbid:["(?:agreed|settled|accepted)[^.\\n]{0,40}(?:9[.,]50|nine fifty)"]},
      {id:"signed",claim:"The order was signed in the meeting.",forbid:["(?:order|deal|contract) (?:was|has been) signed(?! by)","(?:Teodor|Marigold|they) signed (?:the|an?) (?:order|deal|contract)"]},
    ],
  },
  questions:[
    {id:"cafe-count",tag:"early",kind:"factual",fast:true,question:"How many cafés does Marigold have, and how many more are opening?",reference:"Fourteen cafés today, with three more opening by September, which makes seventeen.",must:["14|fourteen","three|\\b3\\b|17|seventeen"],support:["fourteen cafes right now, and we're opening three more by September"]},
    {id:"weekly-capacity",tag:"arithmetic",kind:"factual",fast:true,question:"What is our production capacity per week?",reference:"About 440 pieces a week: the big kiln holds 220 pieces per firing and it is fired twice a week. Some of that is already committed to the restaurants' reorders.",must:["440"],support:["the big kiln holds two hundred twenty pieces a firing, and we fire twice a week"]},
    {id:"settled-price",tag:"late",kind:"factual",fast:true,question:"What mug price did we land on, and under what condition?",reference:"10.25 euros per mug, on condition that Marigold commits to twelve months of reorders for new cafés and replacements. Plates stay at 9 euros. Teodor said he can work with it but has not signed.",must:["10[.,]25|ten twenty.five","twelve.month|12.month|12 months|twelve months|\\byear\\b"],support:["I'll do the mug at ten twenty five"]},
    {id:"order-value",tag:"arithmetic",kind:"factual",question:"How many mugs are in the first order in total, and what is that worth at the price we landed on?",reference:"840 mugs (60 per café for 14 cafés), worth 8,610 euros at 10.25 each. Neither total was said aloud.",must:["840","8,?610"],support:["sixty mugs a cafe"]},
    {id:"floor-and-cost",tag:"notes-only",kind:"factual",fast:true,question:"What is our floor price for a mug, and what does one cost us to make?",reference:"Your notes put the floor at 10 euros and the cost per mug at 4.10. Neither figure was said in the meeting.",must:["\\b10\\b","4[.,]10"]},
    {id:"breakage-rate",tag:"transcript-only",kind:"factual",withSummary:true,fast:true,question:"How many of their current cups do they break?",reference:"About nine percent a month, in the dishwashers.",must:["9 ?%|nine percent|9 percent"],support:["we break about nine percent of them a month"]},
    {id:"payment-terms",tag:"not-said",kind:"absent",fast:true,question:"What payment terms did we agree beyond the deposit, net 30 or net 60?",reference:"Neither was discussed. The only terms mentioned were a 30% deposit on order and the rest on delivery of each batch.",must:[ABSTAIN],forbid:["net (?:30|60|thirty|sixty) (?:was|is|were) (?:agreed|offered|accepted)"]},
    {id:"exclusivity",tag:"undecided",kind:"factual",fast:true,question:"Did we give them exclusivity on the green glaze?",reference:"No. Bianca refused exclusivity on the glaze because it is on half of what the studio sells. She promised only that the shape with Marigold's stamp will not be sold to any other café.",must:["\\bno\\b|not|refus|declin|without|can't|cannot","stamp|shape"]},
    {id:"stamp-follow-up",tag:"follow-up",kind:"follow-up",fast:true,question:"Can we drop it?",
      history:[{role:"user",content:"What does the logo stamp cost?"},{role:"assistant",content:"There is a one-time tooling fee of 180 euros for the stamp on the base."}],
      reference:"Bianca offered to waive the stamp fee if the order is signed by May 15; after that it stands.",must:["May 15|15 May|May fifteenth|the fifteenth"],support:["If the order is signed by May fifteenth, I'll waive the stamp fee"]},
    {id:"reply-follow-up",tag:"follow-up",kind:"follow-up",question:"And when does he get back to us?",
      history:[{role:"user",content:"What does the logo stamp cost?"},{role:"assistant",content:"There is a one-time tooling fee of 180 euros for the stamp on the base."},{role:"user",content:"Can we drop it?"},{role:"assistant",content:"Yes, conditionally: the fee is waived if the order is signed by May 15."}],
      reference:"Teodor said he will take it to Ilka, the operations director, and reply by May 9, which is before the May 15 cut-off for the waiver.",must:["May 9|9 May|May ninth|the ninth"],support:["I take this to Ilka and I'll reply by May ninth"]},
    {id:"german",tag:"language",kind:"factual",fast:true,question:"Wann kommt die erste Lieferung und wie viele Becher sind das?",reference:"Die erste Lieferung mit 400 Bechern kommt bis zum 16. Juni. Der Rest der Becher und alle Teller folgen bis zum 7. Juli.",must:["16\\. ?Juni","400","\\b(?:Lieferung|Becher|bis|kommt|erste)\\b"],support:["First batch of four hundred mugs by June sixteenth"]},
    {id:"cite-deposit",tag:"citation",kind:"citation",fast:true,question:"What deposit do we take? Cite where it was said.",reference:"A 30% deposit when the order is placed, with the rest due on delivery of each batch.",must:["30 ?%|thirty percent|30 percent"],support:["we take a thirty percent deposit when you place the order"]},
    {id:"next-steps",tag:"synthesis",kind:"synthesis",fast:true,question:"What are the next steps, and who owns each?",reference:"Linh: send the lead-free lab certificate tomorrow; courier a sample set of six mugs to the Harbour Street café this week. Bianca: send the written quote by Wednesday. Teodor: take the offer to Ilka and reply by May 9. The stamp fee is waived only if the order is signed by May 15.",must:["Linh","Teodor","Wednesday","May 9|9 May|ninth"]},
    {id:"list-my-todos",tag:"recipe",kind:"synthesis",question:"List my todos",reference:"To do\n- Send Teodor the written quote by Wednesday.\n\nInferred\n- Check that Linh sends the lab certificate tomorrow and couriers the six sample mugs this week.\n- Expect Teodor's reply by May 9; the stamp fee waiver ends if the order is not signed by May 15.",must:["quote","Wednesday"],forbid:["(?:you|I)(?: will|'ll| would| agreed to| committed to)[^.\\n]{0,25}(?:courier|take (?:it|this) to Ilka)"]},
    {id:"write-tldr",tag:"recipe",kind:"synthesis",question:"Write tldr",reference:"Wholesale meeting with Marigold Cafés about mugs and small plates for their 14 cafés.\n- Offered mugs at 10.25 euros with a twelve-month commitment (plates stay at 9); Teodor can work with it but has to take it to his operations director and will reply by May 9.\n- No exclusivity on the glaze, only on the stamped shape; the stamp fee is waived if the order is signed by May 15.\n- First 400 mugs by June 16, the rest by July 7.",must:["10[.,]25|ten twenty.five"]},
    {id:"glaze-name",tag:"stt",kind:"factual",question:"Which glaze do they want?",reference:"The celadon glaze, the pale green house colour. The transcript renders it as \"sell a don\".",must:["celadon"]},
    {id:"who-certificate",tag:"speaker",kind:"factual",question:"Who is sending the lab certificate, and when?",reference:"Linh, tomorrow. Bianca asked her to and she agreed.",must:["Linh","tomorrow"],support:["I'll send it tomorrow"]},
  ],
  live:[
    {id:"miss-early",kind:"recency",recipe:true,fast:true,after:"At this volume I was hoping for nine fifty on the mug.",question:"What did I miss?",
      reference:"- He wants the logo stamped on the base; the stamp has a one-time 180 euro tooling fee.\n- List prices given: 11 euros per mug and 9 per small plate.\n- He asked for 9.50 on the mug at this volume.",must:["9[.,]50|nine fifty","\\b11\\b|eleven|stamp"],later:["10[.,]25|ten twenty.five","six weeks|6 weeks","deposit"]},
    {id:"price-before",kind:"superseded",fast:true,after:"At this volume I was hoping for nine fifty on the mug.",question:"What price are we at for mugs?",
      reference:"Nothing is agreed yet. The list price quoted is 11 euros; Teodor has just asked for 9.50.",must:["\\b11\\b|eleven","9[.,]50|nine fifty"],later:["10[.,]25|ten twenty.five"]},
    {id:"price-after",kind:"superseded",fast:true,after:"I'll do the mug at ten twenty five. The plate stays at nine.",question:"What price are we at for mugs?",
      reference:"10.25 euros, offered just now on condition of a twelve-month commitment. Teodor had asked for 9.50; he has not accepted yet.",must:["10[.,]25|ten twenty.five"],later:["May fifteenth|May 15","June sixteenth|June 16"]},
    {id:"that-price",kind:"just-said",after:"I'll do the mug at ten twenty five. The plate stays at nine.",question:"Sorry, what was that price?",
      reference:"10.25 euros per mug with a twelve-month commitment; the plate stays at 9.",must:["10[.,]25|ten twenty.five"],later:["May fifteenth|May 15"]},
    {id:"that-price-lag",kind:"lag",delivery:"ios",fast:true,after:"I'll do the mug at ten twenty five. The plate stays at nine.",question:"Sorry, what was that price?",
      reference:"The transcript has not caught up to that yet; the latest it has is earlier in the conversation. Ask again in a moment.",must:["transcript|caught up|transcribed|behind|not (?:yet )?(?:reached|arrived|available)|a moment|shortly"],later:["10[.,]25|ten twenty.five"]},
    {id:"delivery-not-yet",kind:"not-yet",fast:true,after:"So no exclusivity on the colour, but the stamped shape is ours.",question:"Have delivery dates been set yet?",
      reference:"Not yet. The only timing so far is that a first delivery takes six weeks from deposit.",must:[ABSTAIN+"|not yet|hasn't|haven't|has not"],later:["June sixteenth|June 16","July seventh|July 7"]},
    {id:"capacity-partial-with-number",kind:"unfinished",fast:true,after:"Let me come back to that in a second, because it depends on what Linh can make. Linh, capacity?",partial:{fraction:0.37},question:"How many pieces fit in the kiln?",
      reference:"Two hundred twenty pieces per firing, going by what is being said right now; the sentence is still in progress.",must:["220|two hundred twenty"],later:["twice a week","every month"]},
    {id:"capacity-partial-before-number",kind:"unfinished",fast:true,after:"Let me come back to that in a second, because it depends on what Linh can make. Linh, capacity?",partial:{fraction:0.17},question:"How many pieces fit in the kiln?",
      reference:"Not said yet. Linh has just started answering and is mid-sentence; the number has not come through.",must:[ABSTAIN+"|still|mid.sentence|unfinished|cut off|in progress|\\byet\\b|hasn't"],later:["220|two hundred twenty","twice a week"]},
    {id:"stamp-follow-up-live",kind:"follow-up",after:"If the order is signed by May fifteenth, I'll waive the stamp fee. After that it stands.",question:"Did she agree to drop it?",
      history:[{role:"user",content:"What's the stamp fee?"},{role:"assistant",content:"180 euros, one time, for the tooling to stamp the logo on the base."}],
      reference:"Conditionally: Bianca will waive the stamp fee if the order is signed by May 15; after that it stands.",must:["May 15|15 May|May fifteenth|the fifteenth"],later:["June sixteenth|June 16","Ilka"]},
    {id:"net-terms-live",kind:"not-said",after:"Thirty percent. Okay. One thing my operations director will ask.",question:"Did they mention net 30 payment terms?",
      reference:"No. So far the only payment terms are a 30% deposit on order and the rest on delivery of each batch.",must:[ABSTAIN+"|\\bno\\b"]},
    {id:"sound-smart",kind:"recency",recipe:"smart",after:"So no exclusivity on the colour, but the stamped shape is ours.",question:"Make me sound smart",
      reference:"- I'd like to make sure the stamped shape is written into the quote as yours alone, so there's no doubt later.\n- Would it help your team if we agreed now how reorders for the three new cafés will be priced?",must:["stamp|shape|exclusiv|colou?r|glaze|caf"],later:["10[.,]25|ten twenty.five","June sixteenth|June 16","May fifteenth|May 15"]},
    {id:"suggest-questions",kind:"recency",recipe:"questions",after:"So no exclusivity on the colour, but the stamped shape is ours.",question:"Suggest questions",
      reference:"- Does the promise on the stamped shape cover the small plates as well as the mugs?\n- What would your operations director need to see to be comfortable with the lead-free certificate?\n- How do you expect to order for the three new cafés?",must:["stamp|shape|exclusiv|colou?r|glaze|caf|certificate"],later:["10[.,]25|ten twenty.five","June sixteenth|June 16","May fifteenth|May 15"]},
    {id:"notes-what-i-typed",kind:"notes",notes:"sparse",after:"I'll do the mug at ten twenty five. The plate stays at nine.",question:"What did I write down as our floor price?",
      reference:"10 euros, in your typed notes.",must:["\\b10\\b"],later:["May fifteenth|May 15"]},
    {id:"notes-transcript-first",kind:"notes",notes:"sparse",fast:true,after:"I'll do the mug at ten twenty five. The plate stays at nine.",question:"What mug price is on the table right now?",
      reference:"10.25 euros, offered on condition of a twelve-month commitment. Teodor had asked for 9.50.",must:["10[.,]25|ten twenty.five"],later:["May fifteenth|May 15"]},
    {id:"miss-late-with-notes",kind:"notes",recipe:true,notes:"sparse",after:"I'll courier a sample set of six mugs to your Harbour Street cafe this week.",question:"What did I miss?",
      reference:"- Breakage in transit is replaced free if reported within seven days.\n- Teodor asked for samples to show his operations director.\n- Linh will courier a sample set of six mugs to the Harbour Street café this week.",must:["sample|six mugs|seven days|7 days|broken|breakage"],later:["May ninth|May 9","Wednesday"],forbid:["4[.,]10","floor","nine percent|9 ?%"]},
    {id:"miss-late",kind:"recency",recipe:true,fast:true,after:"I'll courier a sample set of six mugs to your Harbour Street cafe this week.",question:"What did I miss?",
      reference:"- Breakage in transit is replaced free if reported within seven days.\n- Teodor asked for samples to show his operations director.\n- Linh will courier a sample set of six mugs to the Harbour Street café this week.",must:["sample|six mugs|seven days|7 days|broken|breakage"],later:["May ninth|May 9","Wednesday"],forbid:["nine percent|9 ?%","dish ?washer"]},
    {id:"recall-early-late",kind:"recall",after:"I'll courier a sample set of six mugs to your Harbour Street cafe this week.",question:"How many cafés did he say they have?",
      reference:"Fourteen now, with three more opening by September.",must:["14|fourteen"],later:["May ninth|May 9"]},
  ],
}
