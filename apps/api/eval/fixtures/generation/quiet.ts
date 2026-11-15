import type {GenerationFixture} from "../../lib/generation-fixtures"

/** A design critique in which the group compares two concepts, explicitly
 * declines to choose, and nobody takes a task. Good notes record the discussion
 * and the non-decision; they do not invent a decision or a to-do list.
 * Fictional product (Pennywise budgeting app) and people. */
export const quiet:GenerationFixture={
  id:"hard-quiet",split:"tuned",stress:"nothing decided and nothing assigned: no manufactured decisions or action items",
  title:"Onboarding concepts critique",kind:"design critique",daysAgo:4,capture:"ios",wpm:155,
  people:{Rafa:"mic",Lior:"mic",Mei:"mic",Dov:"mic"},
  questions:[],
  expect:{noActionItems:{allowed:["no (?:action items|owners?|tasks?|next steps)","none","nothing (?:was )?(?:assigned|decided)","not? (?:assigned|decided)","revisit|reconvene|wait|after (?:the )?(?:usability|readout|results|sessions)|readout|usability|results|decision deferred|defer"]}},
  script:`
Rafa: So no agenda really, I just wanted the four of us to look at the two onboarding concepts side by side before the usability sessions. Nothing to decide today. Lior, do you want to put A up?
Lior: Yep. So concept A is the guided one. Five screens. You connect a bank account on screen two, pick categories on three, set a monthly target on four, and you land on the dashboard with real numbers.
Mei: Five screens feels long. What's the drop off on the current flow?
Lior: Current flow is seven screens and we lose forty one percent of people before the dashboard.
Mei: Okay so five is already better than seven. I take it back a little.
Dov: The thing I like about A is the dashboard isn't empty when you arrive. That empty dashboard is the saddest screen in the app.
Rafa: Agreed. And B?
Lior: Concept B is the fast one. Three screens. Name, one question about your goal, and you're in. Bank connection is a banner on the dashboard that you can ignore.
Dov: And most people will ignore it.
Lior: Probably, yeah. That's the trade. You get in faster but you might never connect an account.
Mei: The part of B that bothers me is pricing. In A the plan choice is its own screen. In B it's tucked behind the banner, so someone could use the app for a week without ever seeing that it costs six dollars a month.
Rafa: That'll generate support tickets. People hate finding a price late.
Dov: Or it generates conversions because they're already hooked. I honestly don't know which.
Mei: Neither do I. That's kind of my point, we're guessing.
Rafa: We are guessing. Which is why I don't want to pick today.
Lior: Could we do a hybrid? B's three screens but with the bank step from A?
Dov: Then it's four screens and it's just A with a screen removed.
Lior: Ha, fair.
Mei: I'd rather not design a third thing before we've seen anyone use the first two.
Rafa: Yeah. Let's leave the hybrid as a thought, not a plan. Where are we on the sessions?
Lior: Ines's research team already has six usability sessions booked for next week, three on each concept. They post the readout the Monday after.
Rafa: Good. So there's nothing for us to do until then.
Dov: I do think whichever one we pick, the copy on the goal question needs work. Save for something is too vague.
Mei: Agreed, but that's the same in both, so it doesn't help us choose.
Rafa: Right. Okay. So where we are, A is slower but lands you on a full dashboard and shows pricing up front. B is faster but risks nobody connecting a bank and hides the price. No decision, we look at the readout and then choose. Anyone feel differently?
Mei: No, that's it.
Dov: That's where I am.
Lior: Same.
Rafa: Great, thanks. Short one.
`,
  notes:{
    sparse:`A vs B onboarding\nB hides pricing?\nno decision yet`,
    detailed:`# Concept A (guided)\n- 5 screens, bank connect on screen 2\n- dashboard not empty on arrival\n\n# Concept B (fast)\n- 3 screens, bank connect = dismissible banner\n- pricing hidden behind banner - $6/mo seen late\n\n# Where we landed\n- no decision, wait for usability readout\n- hybrid idea floated, not pursuing\n- my hunch: A wins, but I kept quiet`,
  },
  gold:{
    decisions:[
      {id:"no-decision",text:"No concept was chosen; the group will decide after the usability readout.",must:["no decision|not? (?:decided|chosen|picked|choose|pick|selected)|undecided|deferred|defer|wait|after the (?:usability|readout|sessions)|hold off|postpone","readout|usability|sessions|results"]},
    ],
    actionItems:[],
    facts:[
      {id:"concept-a",text:"Concept A is guided: five screens, bank connection on screen two, and a populated dashboard on arrival.",must:["five|\\b5\\b","dashboard"]},
      {id:"concept-b",text:"Concept B is fast: three screens, with bank connection as a dismissible banner on the dashboard.",must:["three|\\b3\\b","banner"]},
      {id:"current-flow",text:"The current flow is seven screens and loses 41% of people before the dashboard.",must:["41|forty.one"]},
      {id:"pricing",text:"In B the $6-a-month price is hidden behind the banner; the group does not know whether that helps or hurts.",must:["pric","6|six"]},
      {id:"sessions",text:"Research (Ines's team) already has six usability sessions booked next week, three per concept; the readout comes the following Monday.",must:["six|\\b6\\b","Monday"]},
      {id:"hybrid",text:"A hybrid (B plus A's bank step) was floated and left as a thought, not a plan.",must:["hybrid"]},
    ],
    notSaid:[
      {id:"chose-a",claim:"The team chose concept A.",forbid:["(?:chose|selected|decided on|agreed on|going with|picked|will (?:ship|build|use)) (?:concept )?A\\b","concept A (?:was|is) (?:chosen|selected|preferred|the winner)","(?:team|group) (?:prefers|preferred|favou?rs|favou?red|leans?|leaned) (?:toward )?(?:concept )?A\\b"]},
      {id:"chose-b",claim:"The team chose concept B.",forbid:["(?:chose|selected|decided on|agreed on|going with|picked|will (?:ship|build|use)) (?:concept )?B\\b","concept B (?:was|is) (?:chosen|selected|preferred|the winner)"]},
      {id:"assigned-task",claim:"Someone at the meeting was given a task.",forbid:["(?:Rafa|Lior|Mei|Dov)(?::| will| to| is going to| agreed to| should)\\s+(?:send|draft|prepare|schedule|write|rewrite|create|share|follow|set up|circulate|compile|revise|update|design|build|mock|book|explore|investigate|review)"]},
      {id:"hybrid-planned",claim:"The hybrid concept will be built or tested.",forbid:["(?:will|to|agreed to|decided to) (?:build|design|test|prototype|explore|pursue|mock up) (?:a |the )?hybrid","hybrid (?:will be|is being|to be) (?:built|designed|tested|prototyped|explored|added)"]},
      {id:"sessions-new",claim:"The usability sessions were scheduled as an action from this meeting.",forbid:["(?:Rafa|Lior|Mei|Dov|team)[^.\\n]{0,20}(?:will|to) (?:schedule|book|set up|organi[sz]e|run)[^.\\n]{0,30}(?:usability|sessions)"]},
      {id:"hunch-shared",claim:"The author's private hunch that A wins was said in the meeting or agreed by the group.",forbid:["(?:Rafa|the group|the team|everyone|attendees) (?:said|felt|agreed|thinks?|believes?|expects?)[^.\\n]{0,30}A (?:will win|wins|is (?:better|stronger|likely))"]},
    ],
  },
}
