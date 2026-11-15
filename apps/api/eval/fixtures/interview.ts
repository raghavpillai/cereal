import type {Fixture} from "../lib/types"
import {near} from "../lib/transcript"

/** Candidate interview followed by a short interviewer debrief. Fictional company and people. */
export const interview:Fixture={
  id:"interview",title:"Interview: Elias Varga, senior data engineer",kind:"interview",daysAgo:9,capture:"desktop",wpm:150,
  people:{Noor:"mic",Elias:"system",Petra:"system"},
  script:`
Noor: Hi Elias, thanks for joining. I'm Noor, I lead the data platform team at Quillmark, and Petra is one of our staff engineers. We have about forty minutes. I'll ask about your background, Petra has a design question, and we'll leave time for yours.
Elias: Sounds good. Nice to meet you both.
Petra: Hi Elias.
Noor: Start wherever you like. What have you been doing most recently?
Elias: For the last three years I've been at Tidewater Analytics, six years in data engineering overall. At Tidewater I led the move from nightly batch pipelines to streaming. When I joined, the freshest data an analyst could see was about six hours old. When we finished it was around twelve minutes.
Noor: Six hours to twelve minutes. How big was the team?
Elias: Five engineers, including me. I was the technical lead but not a people manager.
Noor: What was the hardest part?
Elias: Not the streaming itself. It was getting the finance team to trust numbers that changed during the day. We ended up publishing a reconciled daily snapshot alongside the live tables, so they had something stable to close the books on.
Noor: That's a good answer. What would you do differently?
Elias: I would have set up data contracts with the upstream teams much earlier. We broke dashboards four or five times in the first two months because a producer renamed a field and nobody told us.
Noor: Okay. Tell me about being on call for that system.
Elias: That's my weak spot, to be honest. We had an operations team that took first line. I was an escalation contact, but I've never been the incident commander for a serious outage. I've read a lot of postmortems, I haven't run one.
Noor: Thanks for being direct about it. Petra, do you want to go?
Petra: Sure. Elias, here's the scenario. We ingest payment events from about thirty partners. Partners retry, so we get duplicates, sometimes hours later. Downstream, finance needs each payment counted exactly once. How do you design that?
Elias: First I'd ask whether the partners send a stable identifier for each payment.
Petra: Most do. Assume about ninety percent do, and the rest send nothing useful.
Elias: Then for the ninety percent I'd use that as an idempotency key. Write to a keyed store, and make the downstream write an upsert on that key, so a retry overwrites the same row instead of adding one. For the other ten percent I'd build a synthetic key from partner, amount, currency and a time bucket, and accept that it's a heuristic.
Petra: What's the time bucket?
Elias: I'd start at five minutes and measure. It's a trade off, too wide and you merge two real payments, too narrow and you miss the retry.
Petra: Good. Now, you discover a bug in the synthetic key that has been double counting one partner for three weeks. How do you fix history?
Elias: Hmm. I'd, so, I'd replay that partner's raw events through the corrected logic into a new table and then swap. Actually, wait, the swap is the problem, because finance has already closed two of those weeks.
Petra: Right.
Elias: I'm not sure. I think I'd publish corrections as separate adjustment rows rather than rewrite closed periods, but I'd want to talk to finance before choosing. I don't have a confident answer for the closed weeks.
Petra: That's fine, it's a hard one. Thanks.
Noor: Elias, what questions do you have for us?
Elias: How big is the team, and what's the working pattern?
Noor: Seven engineers on data platform today. We're in the office two days a week, Tuesdays and Thursdays, and remote the rest.
Elias: And what would the first six months look like?
Noor: Honestly, a lot of what you just described. We're still mostly nightly batch, and the product team wants same hour data.
Elias: That's appealing. That's the work I like.
Noor: Two practical things. What are you looking for on compensation, and what's your notice period?
Elias: I'm looking for a base between one hundred sixty five and one hundred seventy five thousand. My notice period is four weeks. And I don't need visa sponsorship.
Noor: Thanks. We'll come back to you by the end of the week. Thanks for your time, Elias.
Elias: Thank you both. Bye.
[+20]
Noor: Okay, he's dropped. Petra, first reaction?
Petra: Lean hire. The dedup answer was solid, exactly what I'd want. The backfill answer worries me a little. He got to the right instinct, adjustment rows, but slowly, and he said himself he wasn't confident.
Noor: I'm a hire. The trust story with finance is the thing we're missing here, and he was honest about on call instead of bluffing.
Petra: I can live with that. I'd want the final round to push on the backfill again, properly, with a whiteboard.
Noor: Agreed. Let's advance him to the final round with Oliver. Can you prepare the backfill deep dive for that?
Petra: Yes, I'll write it up before the final.
Noor: And I'll put our feedback in the tracker by tomorrow so Oliver sees it before he schedules. For the record, his range is inside our band, so no issue there.
Petra: Good. Done?
Noor: Done.
`,
  notes:{
    sparse:`Elias V - sr data eng\nTidewater, batch->streaming, 6h -> 12min\nweak: on call\nbackfill Q shaky\ncomp 165-175\ngut: hire, honest`,
    detailed:`# Background\n- 6 yrs data eng, 3 at Tidewater Analytics\n- led batch -> streaming, freshness 6h -> ~12 min, team of 5 (tech lead, not manager)\n- lesson: data contracts earlier\n\n# Strengths\n- finance trust story (reconciled daily snapshot)\n- dedup design: idempotency key + upsert\n\n# Concerns\n- never incident commander\n- backfill of closed periods - not confident\n\n# Logistics\n- comp 165-175k base, 4 wks notice, no visa\n\n# Debrief\n- Petra lean hire / me hire\n- final round w/ Oliver, push on backfill\n- my gut: he'd raise the bar on how we talk to finance`,
  },
  gold:{
    decisions:[
      {id:"advance",text:"Advance Elias to the final round with Oliver.",must:["final round|final interview|advance","Oliver"]},
      {id:"probe-backfill",text:"The final round should probe the backfill question again in depth.",must:["backfill"]},
    ],
    actionItems:[
      {id:"backfill-question",owner:"Petra",due:"before the final",text:"Petra prepares the backfill deep-dive question for the final round.",must:["backfill","deep.?dive|prepare|write"],ownerMust:near("Petra","backfill")},
      {id:"feedback-tracker",owner:"Noor",due:"tomorrow",text:"Noor records the interview feedback in the tracker by tomorrow.",must:["feedback","tracker|tomorrow"],ownerMust:near("Noor","feedback")},
      {id:"reply-candidate",owner:"Noor",due:"end of the week",text:"Quillmark gets back to Elias by the end of the week.",must:["end of (?:the|this) week"],ownerMust:near("Noor|Quillmark|interviewers?|team|we","end of (?:the|this) week")},
    ],
    facts:[
      {id:"freshness",text:"At Tidewater he led batch-to-streaming, cutting data freshness from about six hours to about twelve minutes with a team of five.",must:["six hours|6 hours|6h","twelve minutes|12 min"]},
      {id:"oncall-gap",text:"He has never been incident commander for a serious outage; on call is his self-described weak spot.",must:["incident|on.?call"]},
      {id:"dedup",text:"Dedup design: partner payment ID as idempotency key with upserts; synthetic key (partner, amount, currency, time bucket starting at five minutes) for the ten percent without IDs.",must:["idempoten","synthetic|time bucket|heuristic"]},
      {id:"backfill",text:"On fixing three weeks of double counting with closed finance periods he was unsure, landing on adjustment rows rather than rewriting closed periods.",must:["adjustment|correction","closed|not confident|unsure|hesita"]},
      {id:"comp",text:"Compensation expectation: $165,000 to $175,000 base; four weeks' notice; no visa sponsorship needed.",must:["165","175","four weeks|4 weeks|4.week|four.week"]},
      {id:"verdicts",text:"Petra: lean hire; Noor: hire.",must:["lean hire"]},
    ],
    notSaid:[
      {id:"offer-made",claim:"An offer was made or agreed.",forbid:["offer (?:was|has been) (?:made|extended|accepted)","(?:decided|agreed) to (?:make|extend) (?:him )?an offer"]},
      {id:"competing-offer",claim:"Elias has a competing offer or deadline.",forbid:["competing offer|other offers?|another offer"]},
      {id:"degree",claim:"Elias's degree or university was discussed.",forbid:["PhD|master's degree|university of|graduated from"]},
      {id:"managed-people",claim:"Elias managed people at Tidewater.",forbid:["(?:managed|people manager of|line manager (?:of|for)) (?:a team|five|5)","as a people manager"]},
      {id:"sponsorship-needed",claim:"Elias needs visa sponsorship.",forbid:["(?:needs|requires|will need) (?:visa )?sponsorship"]},
    ],
  },
  questions:[
    {id:"comp",kind:"factual",fast:true,question:"What compensation is he expecting and when could he start?",reference:"A base salary between $165,000 and $175,000, with a four-week notice period. He does not need visa sponsorship. Noor noted the range is inside the band.",must:["165","175","four|\\b4\\b"],support:["I'm looking for a base between one hundred sixty five and one hundred seventy five thousand"]},
    {id:"concerns",kind:"synthesis",question:"Summarise the case for and against hiring Elias.",reference:"For: led a batch-to-streaming migration (six hours to twelve minutes), strong dedup design, built trust with finance, honest about gaps. Against: has never run an incident, and was slow and not confident on backfilling closed finance periods. Petra is a lean hire, Noor a hire; both want the final round to push on backfill.",must:["streaming|six hours|12 min|twelve minutes","incident|on.?call","backfill"]},
    {id:"petra-view",kind:"citation",question:"What exactly was Petra's concern? Cite the transcript.",reference:"The backfill answer: he reached the right instinct (adjustment rows) but slowly, and said himself he was not confident. She still rated him lean hire and wants the final round to push on backfill with a whiteboard.",must:["backfill","confiden|slow"],support:["The backfill answer worries me a little"]},
    {id:"competing",kind:"absent",fast:true,question:"Which other companies is he interviewing with, and does he have a deadline?",reference:"He did not say. Other interviews, competing offers and deadlines were not mentioned.",must:["not (?:mention|say|discuss|state|come up)|no (?:mention|information|indication)|didn't|did not|wasn't|was not|unknown"],forbid:["interviewing with [A-Z]","deadline (?:is|of) "]},
    {id:"follow-next",kind:"follow-up",question:"So what happens next, and who does what?",history:[{role:"user",content:"What did Noor and Petra conclude in the debrief?"},{role:"assistant",content:"Petra was a lean hire and Noor a hire. Both were positive on the dedup design and his honesty; the open concern is the backfill answer."}],reference:"He advances to the final round with Oliver. Petra prepares a backfill deep-dive question before the final; Noor puts the feedback in the tracker by tomorrow; they told Elias he would hear back by the end of the week.",must:["Oliver","Petra","tracker|feedback"],support:["Let's advance him to the final round with Oliver"]},
  ],
}
