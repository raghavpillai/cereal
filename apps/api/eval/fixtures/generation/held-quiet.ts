import type {GenerationFixture} from "../../lib/generation-fixtures"

/** HELD OUT: never used while tuning the generation prompt.
 * An informational briefing with questions. Facts are announced; nothing is
 * decided by the people in the room and nobody takes a task. Fictional employer
 * (Calloway Freight), provider and people. */
export const heldQuiet:GenerationFixture={
  id:"held-quiet",split:"held-out",stress:"an informational briefing: facts and answers only, no decisions, no action items",
  title:"Open enrollment briefing",kind:"benefits briefing with Q&A",daysAgo:6,capture:"desktop",wpm:150,
  people:{Bettina:"mic",Marlon:"system",Xiadani:"system",Emeka:"system"},
  questions:[],
  expect:{noActionItems:{allowed:["no (?:action items|owners?|tasks?|next steps)","none","nothing (?:was )?(?:assigned|decided)","not? (?:assigned|decided)","enrol|window|portal|default|roll over|rolls over|carry over|elect|deadline|hotline"]}},
  script:`
Marlon: Thanks for joining, everybody. I'm Marlon from the benefits team. This is the open enrollment briefing, it's the same session we ran on Tuesday, so if you were at that one you can drop. I'll go through what's changing and then take questions.
Marlon: First, dates. Enrollment opens Monday the third and closes Friday the twenty first. That's three weeks. If you do nothing, your current medical and dental elections roll over as they are. The one thing that does not roll over is the flexible spending account. You have to re-elect that every year or it goes to zero.
Bettina: Sorry, can I ask, is that new? The spending account not rolling over?
Marlon: No, that's always been the rule. It just catches people every year, so I say it first.
Marlon: Second, cost. The medical premium goes up four percent on both plans. For the standard plan that's about nine dollars more a paycheck for employee only coverage. Dental and vision are unchanged.
Xiadani: Is the deductible changing?
Marlon: The standard plan deductible stays at fifteen hundred dollars. The high deductible plan goes from three thousand to three thousand two hundred.
Xiadani: Okay, thanks.
Marlon: Third, what's new. The company contribution to the health savings account goes from five hundred to seven hundred and fifty dollars a year, that's only if you're on the high deductible plan. And we're adding a second mental health provider, Brightpath, alongside the existing one. Eight sessions a year covered in full, same as now.
Emeka: Does Brightpath cover dependents?
Marlon: Yes, dependents on your plan are covered, same eight sessions each.
Emeka: Great.
Bettina: What about the commuter benefit? Someone told me that was going away.
Marlon: It's not going away. Nothing about commuter is changing. I've heard that rumor too and I don't know where it started.
Bettina: Good to know.
Xiadani: If I want to switch from standard to high deductible, is there anything I need to do beyond picking it in the portal?
Marlon: Just pick it in the portal. If you want the health savings account you tick that box in the same flow. There's a comparison tool on the first page that does the math with your own numbers.
Emeka: When do the new rates start showing up on paychecks?
Marlon: First paycheck in January.
Bettina: And if someone misses the window?
Marlon: Then your current elections carry over, except the spending account, and you'd need a qualifying life event to change anything before next year. There are no late exceptions, I can't make them even if I wanted to.
Marlon: The slides and the rate sheet are already on the intranet under benefits. The hotline is open eight to six on weekdays during the window. Any other questions?
Xiadani: Not from me.
Emeka: No, that was clear.
Bettina: All good, thank you.
Marlon: Thanks everyone.
`,
  notes:{
    sparse:`enrollment 3rd-21st\nFSA doesn't roll over!!\nHSA employer $ up`,
    detailed:`# Dates\n- opens Mon 3rd, closes Fri 21st\n- do nothing = medical + dental roll over\n- FSA must be re-elected or goes to 0\n\n# Costs\n- medical premium +4% both plans (~$9/paycheck standard, employee only)\n- dental/vision unchanged\n- HDHP deductible 3,000 -> 3,200\n\n# New\n- HSA company contribution 500 -> 750 (HDHP only)\n- Brightpath added, 8 sessions\n\n# For me\n- thinking about switching to HDHP - run the comparison tool first\n- commuter rumor is false`,
  },
  gold:{
    decisions:[],
    actionItems:[],
    facts:[
      {id:"window",text:"Enrollment opens Monday the 3rd and closes Friday the 21st.",must:["3rd|third|\\b3\\b","21st|twenty.first|\\b21\\b"]},
      {id:"fsa",text:"The flexible spending account does not roll over: it must be re-elected or it goes to zero. Medical and dental elections roll over.",must:["flexible spending|FSA|spending account","re.?elect|roll|zero|\\b0\\b"]},
      {id:"premium",text:"Medical premiums rise 4% on both plans, about $9 more per paycheck for employee-only standard coverage; dental and vision are unchanged.",must:["4 ?%|four percent"]},
      {id:"deductible",text:"The standard deductible stays at $1,500; the high-deductible plan goes from $3,000 to $3,200.",must:["3,?200"]},
      {id:"hsa",text:"The company HSA contribution rises from $500 to $750 a year (high-deductible plan only).",must:["750|seven hundred (?:and )?fifty"]},
      {id:"brightpath",text:"Brightpath is added as a second mental-health provider: eight sessions a year covered in full, dependents included.",must:["Brightpath"]},
      {id:"commuter",text:"The commuter benefit is not changing.",must:["commuter"]},
      {id:"rates-start",text:"New rates apply from the first paycheck in January; there are no late exceptions.",must:["January"]},
    ],
    notSaid:[
      {id:"commuter-ending",claim:"The commuter benefit is ending or changing.",forbid:["commuter benefit (?:is|will be) (?:ending|going away|discontinued|removed|cut|reduced)"]},
      {id:"dental-up",claim:"Dental or vision premiums are rising.",forbid:["(?:dental|vision)[^.\\n]{0,20}(?:premiums? )?(?:increase|rise|go(?:es)? up|up) (?:by )?\\d"]},
      {id:"author-switching",claim:"The author decided to switch to the high-deductible plan.",forbid:["(?:Bettina|the author|I) (?:decided|will|plans?|is going) to switch","decided to switch to (?:the )?(?:HDHP|high.deductible)"]},
      {id:"assigned-task",claim:"An attendee was given a task.",forbid:["(?:Bettina|Xiadani|Emeka|Marlon)(?::| will| to| is going to| agreed to)\\s+(?:send|share|circulate|follow up|email|post|confirm|check|look into|investigate|provide|schedule)"]},
      {id:"fsa-rolls",claim:"The spending account rolls over automatically.",forbid:["(?:FSA|flexible spending account|spending account) (?:will |also )?rolls? over(?! unless| only if)"]},
      {id:"late-exceptions",claim:"Late changes or exceptions are possible on request.",forbid:["(?:late )?exceptions? (?:are|can be|may be) (?:available|granted|made|possible)"]},
    ],
  },
}
