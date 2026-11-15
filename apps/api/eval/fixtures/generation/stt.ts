import type {GenerationFixture} from "../../lib/generation-fixtures"
import {near} from "../../lib/transcript"

/** A renewal check-in recorded on one phone, written the way speech-to-text
 * renders it: names misheard and inconsistent ("Neve", "Wakeen"), a product name
 * split into ordinary words, fillers and restarts, people talking over each
 * other, and numbers that are only safe once the speakers spell them out
 * (fifty, not fifteen; the thirtieth, not the thirteenth). The attendee list
 * carries the correct spellings. Fictional companies and people. */
export const stt:GenerationFixture={
  id:"hard-stt",split:"tuned",stress:"speech-to-text errors in names and numbers, disfluent speech, cross-talk",
  title:"Ostrava Dental renewal check-in",kind:"customer renewal call",daysAgo:1,capture:"ios",wpm:160,
  people:{Niamh:"mic",Joaquin:"mic",Ngozi:"mic"},
  attendees:[{name:"Niamh Gallagher",email:"niamh@tallyline.example",self:true},{name:"Joaquin Herrera",email:"joaquin@ostravadental.example"},{name:"Ngozi Adeyemi",email:"ngozi@ostravadental.example"}],
  roster:["Neve","Wakeen","Wakine","Goazy","Gozi"],
  questions:[],
  script:`
Niamh: Okay so um thanks for coming in, both of you. I know it's, it's a busy week. So the the renewal is coming up and I wanted to, you know, make sure we're we're aligned before the paperwork goes out.
Joaquin: Yeah no, good timing. So Neve, the short version is we want to renew but we need more seats. We opened the, um, the two new clinics in the spring.
Niamh: Right, right. So how many seats are we talking, because you're on thirty five today.
Joaquin: Fifteen. Sorry, fifty. Five zero. Fifty seats total, so fifteen more than today. I always say that wrong.
Niamh: Five zero, fifty total. Got it. And that's for tally line scheduler, same product, not the the analytics add on?
Joaquin: Same product. Tally line scheduler only. We looked at the analytics thing and it's, I mean it's nice but
~Ngozi: It's not in the budget.
Joaquin: it's not in the budget this year, yeah. What Goazy said.
Niamh: No problem. So at fifty seats you move into the next pricing tier, which is um, that's thirty six dollars a seat, so that's eighteen hundred a month.
Ngozi: Eighteen thousand?
Niamh: No no, eighteen hundred. One thousand eight hundred dollars a month. Sorry, I I said that fast.
Ngozi: Okay good, because eighteen thousand I'd have to, I'd have to lie down.
Niamh: Ha. No. Eighteen hundred a month, on an annual term. And the current term ends on the thirtieth.
Joaquin: The thirteenth?
Niamh: Thirtieth. Three zero. End of the month.
Joaquin: Okay, thirtieth. That's, yeah that's more time than I thought.
Niamh: So what I'll do is I'll send you a revised quote for fifty seats tomorrow. And then Wakeen, can you get the signed order form back to me before the thirtieth?
Joaquin: Yes. I'll sign it as soon as, um, as soon as finance clears it, but before the thirtieth for sure.
Niamh: Perfect. Now the other thing you mentioned on email was was onboarding for the new clinics.
Ngozi: Yeah so that's me. The two new sites have, I think it's eleven front desk staff between them and none of them have used it. Last time we did the the remote training and honestly it it didn't stick.
Niamh: We can do on site. It's a one day session per clinic, it's four hundred dollars a session.
Ngozi: So eight hundred for both.
Niamh: Eight hundred for both, yes.
Ngozi: I don't know if that comes out of my training budget or or the software line. I'd need to check.
Niamh: And Goazy, you'll check the training budget and let me know?
Ngozi: Yes. I'll check whether the training budget covers it and I'll email you by Friday.
Niamh: Great. And if it does I'll I'll get dates from our trainer. I'm not going to book anything until I hear from you.
Joaquin: One thing, um, the the reminder texts. We had that week in, was it March, where they went out twice.
Niamh: The duplicate reminders, yeah. That was fixed in the March release, we haven't seen it since. Have you?
Joaquin: No. No it's been fine. I just, Dr. Novak asks me every time so I wanted to say I asked.
Niamh: Totally fair. You can tell Dr. Novak it's it's been clean since March.
Joaquin: Will do.
Niamh: Okay. I think that's everything. Thanks both.
`,
  notes:{
    sparse:`renewal - 50 seats\n$1,800/mo\nonsite training?\nJoaquin seemed relieved about the date`,
    detailed:`# Renewal\n- want to renew, 2 new clinics opened in spring\n- 35 -> 50 seats, Scheduler only (no Analytics - not in budget)\n- $36/seat = $1,800/mo, annual\n- term ends 30th\n\n# Onboarding new clinics\n- 11 front desk staff, remote training didn't stick\n- on-site: 1 day per clinic, $400 each = $800\n- Ngozi unsure which budget\n\n# Other\n- duplicate reminder texts - fixed in March, fine since\n\n# Next\n- me: revised quote tomorrow\n- Joaquin: signed order form before 30th\n- Ngozi: confirm training budget by Fri`,
  },
  gold:{
    decisions:[
      {id:"renew-50",text:"Ostrava intends to renew Tallyline Scheduler at 50 seats (up from 35); the Analytics add-on is out because it is not in this year's budget.",must:["50|fifty","35|thirty.five|15 more|fifteen more|15 additional"]},
    ],
    actionItems:[
      {id:"quote",owner:"Niamh",due:"tomorrow",text:"Niamh sends a revised quote for 50 seats tomorrow.",must:["quote","tomorrow"],ownerMust:near("Niamh|Neve","quote",100)},
      {id:"order-form",owner:"Joaquin",due:"before the 30th",text:"Joaquin returns the signed order form before the 30th, once finance clears it.",must:["order form","30th|thirtieth"],ownerMust:near("Joaquin|Wakeen","order form",100)},
      {id:"training-budget",owner:"Ngozi",due:"Friday",text:"Ngozi checks whether the training budget covers on-site onboarding and emails Niamh by Friday.",must:["training budget|budget[^.\\n]{0,40}(?:training|on.?site|onboarding)","Friday"],ownerMust:near("Ngozi|Goazy|Gozi","budget",100)},
    ],
    facts:[
      {id:"price",text:"At 50 seats the price is $36 a seat, $1,800 a month, on an annual term.",must:["1,?800|eighteen hundred"]},
      {id:"term-end",text:"The current term ends on the 30th.",must:["30th|thirtieth"]},
      {id:"onsite",text:"On-site training is one day per clinic at $400 a session, $800 for both new clinics; 11 front-desk staff are untrained.",must:["400|800"]},
      {id:"reminders",text:"The duplicate reminder texts were fixed in the March release and have not recurred.",must:["reminder","March"]},
      {id:"names-spelled",text:"People are named as in the attendee list (Niamh, Joaquin, Ngozi), not as transcribed (Neve, Wakeen, Goazy).",must:["Joaquin","Ngozi"]},
      {id:"product-name",text:"The product is Tallyline Scheduler.",must:["Tally ?line"]},
    ],
    notSaid:[
      {id:"fifteen-seats",claim:"The renewal is for 15 seats.",forbid:["(?:renew(?:al|ing)?|total|plan|quote)[^.\\n]{0,30}(?:for|at|of|to) (?:15|fifteen) seats","(?:15|fifteen) seats total"]},
      {id:"eighteen-thousand",claim:"The price is $18,000 a month.",forbid:["\\$?18,000 (?:a|per|/) ?mo"]},
      {id:"thirteenth",claim:"The term ends on the 13th.",forbid:["(?:ends?|expires?|before|by)(?: on)?(?: the)? 13th"]},
      {id:"misheard-names",claim:"Someone called Neve, Wakeen or Goazy took part.",forbid:["\\bNeve\\b","\\bWakeen\\b","\\bGoazy\\b"]},
      {id:"training-booked",claim:"On-site training was booked or agreed.",forbid:["on.?site (?:training|onboarding|sessions?) (?:was|were|is|are|has been) (?:booked|agreed|confirmed|scheduled|approved)","(?:booked|scheduled|confirmed) (?:the |two )?on.?site"]},
      {id:"analytics-added",claim:"The Analytics add-on is part of the renewal.",forbid:["(?:add(?:ing|ed)?|includ(?:e|es|ing)|with) (?:the )?analytics add.?on(?! (?:is|was) not| excluded)"]},
    ],
  },
}
