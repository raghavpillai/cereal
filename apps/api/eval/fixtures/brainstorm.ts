import type {Fixture} from "../lib/types"
import {near} from "../lib/transcript"

/** Brainstorm with tangents, recorded on one phone in a room. Fictional product and people. */
export const brainstorm:Fixture={
  id:"brainstorm",title:"Smart recap launch brainstorm",kind:"brainstorm with tangents",daysAgo:12,capture:"ios",wpm:160,
  people:{Fern:"mic",Idris:"mic",Mateo:"mic",Chloe:"mic"},
  script:`
Fern: Okay, phones down, snacks out. The goal for the next little while is ideas for getting people to actually try smart recap in the Loomleaf beta. No bad ideas, we'll cut at the end. I'll keep the list.
Idris: Before we start, is anyone else's kitchen coffee machine doing the thing where it gives you half a cup?
Chloe: Yes! I thought it was just me. It's been like that since Monday.
Mateo: You have to hold the button. It's a feature.
Idris: It is not a feature, Mateo.
Fern: Coffee machine grievances go on a different list. Ideas. Who has one?
Chloe: I'll go. A weekly email. Every Friday you get the five most useful things your recaps caught that week, a decision, a deadline, whatever. I'd call it Friday Five.
Fern: Friday Five. I like that it has a name already.
Mateo: Who writes the email? Us or the model?
Chloe: Generated from their own recaps, so nobody writes it. We design the template once.
Idris: The risk is it becomes one more newsletter people filter.
Chloe: Sure, but it's their own content, not ours. That's the difference.
Fern: Noted, with the risk. Next.
Mateo: Mine is audio. People don't read recaps, but they do listen to things on the way home. So a two minute audio digest of your day's meetings. Working title, Recap Radio.
Idris: Oh, I would use that. I listen to a sci fi podcast on the train anyway. Did anyone watch the finale of that space freighter show, by the way?
Chloe: No spoilers, I'm two episodes behind.
Idris: I'll just say the cat survives.
Chloe: Idris!
Fern: Recap Radio. Mateo, how hard is it to build?
Mateo: Honestly I don't know. Text to speech is easy. Making it sound like something you'd want to listen to is not. I'd want a two week spike before I promise anything.
Fern: Okay, two week spike, written down. Idris, you've been quiet about work things.
Idris: Streaks. Like the language apps. You get a badge for every day you read your recap, and a little flame.
Chloe: Hmm.
Mateo: I hate the flame.
Idris: Everyone hates the flame and everyone keeps the streak.
Fern: My worry is it rewards opening the recap, not using it. And our users are, what, operations managers? I can't picture them wanting a flame.
Chloe: It feels too gamified for this audience. I'd cut it.
Idris: Fine. I'll defend it for exactly ten more seconds. No? Okay, cut it.
Fern: Streak badges, rejected, too gamified. Anything else?
Chloe: Referral credits. Invite a teammate, you both get a month free.
Fern: I like it but I think giving account credit needs legal to look at it. We had that problem with the conference promotion.
Mateo: Yeah, the terms of service thing.
Fern: So that one is parked, not rejected. I'll ask legal.
Idris: Can I do a tangent that is actually relevant? My sourdough starter.
Mateo: How is that relevant?
Idris: It isn't. But I've kept it alive for forty days and I wanted someone to know.
Fern: Congratulations, Idris. Okay, let's talk about what success looks like, because that decides what we pick. The number from leadership is five thousand beta sign ups by the end of May.
Chloe: Five thousand. Where are we now?
Fern: About eleven hundred.
Mateo: So we need roughly four thousand more in, what, ten weeks.
Fern: Yes. And the campaign budget is eight thousand dollars. Total. That includes any tools.
Idris: Eight thousand doesn't buy an agency.
Fern: No, and we're not hiring one. It's us.
Chloe: Then Friday Five is the obvious first move. It costs design time and nothing else.
Mateo: And Recap Radio is the one that could get people talking, if the spike works.
Fern: So the short list is Friday Five now, and Recap Radio as a prototype behind it. Does anyone disagree?
Idris: No. I'd add one thing, we should watch real people use both before we send them to five thousand users. I can book a user testing session, six users, from the beta list.
Fern: Yes, do that. Six users.
Chloe: I can have a mock of the Friday Five email by Tuesday.
Fern: Tuesday, great. Mateo, the spike?
Mateo: I'll scope the Recap Radio prototype this week and start the two week spike on Monday.
Fern: And I'll take the legal question on referral credits. One thing we are not doing today is picking a launch date. I don't want a date until we've seen the user testing.
Chloe: Agreed. No date.
Idris: Can we at least pick a date for fixing the coffee machine?
Fern: I'll put it in the notes, Idris. Thanks everyone.
`,
  notes:{
    sparse:`ideas for smart recap adoption\n- friday five (email)\n- recap radio\n- streaks NO\n- referrals -> legal\n5000 by end of May`,
    detailed:`# Goal\n- 5,000 beta sign-ups by end of May (at ~1,100 now)\n- budget $8k total, no agency\n\n# Ideas\n- Friday Five - weekly email of top 5 recap items (Chloe)\n- Recap Radio - 2 min audio digest (Mateo) - needs spike\n- streak badges - cut\n- referral credits - parked, legal\n\n# Shortlist\n- Friday Five first, Recap Radio prototype second\n\n# Next\n- Chloe mock Tues\n- Mateo spike\n- Idris user testing x6\n- me: legal`,
  },
  gold:{
    decisions:[
      {id:"shortlist",text:"Short list: Friday Five weekly email first, with a Recap Radio audio-digest prototype behind it.",must:["Friday Five","Recap Radio"]},
      {id:"streaks-rejected",text:"Streak badges were rejected as too gamified for the audience.",must:["streak","reject|cut|dropp|too gamified|not pursu"]},
      {id:"referrals-parked",text:"Referral credits are parked until legal reviews them.",must:["referral","legal"]},
      {id:"no-date",text:"No launch date will be set until after user testing.",must:["no (?:launch )?date|not (?:picking|setting|choosing)|launch date[^.\\n]{0,60}(?:not|until|after)|until[^.\\n]{0,40}user testing"]},
    ],
    actionItems:[
      {id:"mock",owner:"Chloe",due:"Tuesday",text:"Chloe produces a mock of the Friday Five email by Tuesday.",must:["mock","Tuesday"],ownerMust:near("Chloe","mock|Friday Five")},
      {id:"spike",owner:"Mateo",due:"starts Monday",text:"Mateo scopes the Recap Radio prototype this week and starts a two-week spike on Monday.",must:["spike|scope","Recap Radio"],ownerMust:near("Mateo","spike|scope|Recap Radio")},
      {id:"legal",owner:"Fern",text:"Fern asks legal about referral credits.",must:["legal"],ownerMust:near("Fern","legal")},
      {id:"user-testing",owner:"Idris",text:"Idris books a user-testing session with six beta users.",must:["user.testing","six|\\b6\\b"],ownerMust:near("Idris","user.testing")},
    ],
    facts:[
      {id:"target",text:"Target: 5,000 beta sign-ups by the end of May; currently about 1,100.",must:["5,?000|five thousand","May"]},
      {id:"current",text:"About 1,100 sign-ups today, so roughly 4,000 more in about ten weeks.",must:["1,?100|eleven hundred"]},
      {id:"budget",text:"Campaign budget is $8,000 in total, including tools; no agency.",must:["8,?000|eight thousand|8k"]},
      {id:"friday-five",text:"Friday Five: a weekly email generated from the user's own recaps listing the five most useful items.",must:["Friday Five","email"]},
    ],
    notSaid:[
      {id:"launch-date",claim:"A launch date was set.",forbid:["launch(?:es|ing)? (?:on|date (?:is|of|set for)) (?:\\d|Jan|Feb|Mar|Apr|May|Jun)","launch date (?:was|is) set"]},
      {id:"agency",claim:"The team will hire an agency.",forbid:["(?:hire|hiring|engage|engaging) an agency(?! is| was|,)"]},
      {id:"streaks-kept",claim:"Streak badges made the short list.",forbid:["short.?list[^.\\n]{0,80}streak"]},
      {id:"tangents",claim:"Tangents (coffee machine, sourdough, TV show) reported as meeting outcomes.",forbid:["sourdough","space freighter|finale"]},
    ],
  },
  questions:[
    {id:"shortlist",kind:"factual",fast:true,question:"Which ideas made the short list and which were dropped?",reference:"Short list: the Friday Five weekly email first, and a Recap Radio audio-digest prototype behind it. Streak badges were rejected as too gamified. Referral credits were parked pending legal review.",must:["Friday Five","Recap Radio","streak","referral"],support:["So the short list is Friday Five now, and Recap Radio as a prototype behind it"]},
    {id:"numbers",kind:"citation",question:"What is the sign-up target, where are we now, and what is the budget? Cite the transcript.",reference:"Five thousand beta sign-ups by the end of May, from about eleven hundred today, with a total campaign budget of eight thousand dollars including tools.",must:["5,?000|five thousand","1,?100|eleven hundred","8,?000|eight thousand|8k"],support:["five thousand beta sign ups by the end of May","About eleven hundred","the campaign budget is eight thousand dollars"]},
    {id:"launch-date",kind:"absent",fast:true,question:"What launch date did we pick?",reference:"None. Fern explicitly said no launch date would be picked until after the user testing.",must:["no(?:t| date| launch date|ne)|didn't|did not|wasn't|was not"],forbid:["launch (?:date )?(?:is|on|will be) (?:\\d|Jan|Feb|Mar|Apr|May|Jun)"],support:["One thing we are not doing today is picking a launch date"]},
    {id:"owners",kind:"synthesis",question:"Who owes what by when?",reference:"Chloe: Friday Five email mock by Tuesday. Mateo: scope the Recap Radio prototype this week and start a two-week spike on Monday. Idris: book a user-testing session with six beta users. Fern: ask legal about referral credits.",must:["Chloe","Mateo","Idris","legal"]},
    {id:"follow-why-cut",kind:"follow-up",question:"Why was that one cut?",history:[{role:"user",content:"What did Idris propose?"},{role:"assistant",content:"Idris proposed streak badges: a badge and a flame for each day a user reads their recap, like language-learning apps."}],reference:"It rewards opening the recap rather than using it, and the team felt it was too gamified for an audience of operations managers.",must:["gamif|opening|operations managers|audience"],support:["It feels too gamified for this audience"]},
  ],
}
