import type {GenerationFixture} from "../../lib/generation-fixtures"
import {near} from "../../lib/transcript"

/** Weekly project sync built around who commits to what, and how the stored
 * transcript does or does not support naming them: addressed by name, first
 * person at the note owner's microphone, first person from a remote voice that
 * was named in the same audio part, a volunteer nobody names, a label that means
 * a different person in the next part, an absent invitee, and a task nobody took.
 * Fictional company (Saltmarsh Books) and people. */
export const owners:GenerationFixture={
  id:"hard-owners",split:"tuned",stress:"owner attribution: addressed, first person (microphone and remote), unnamed volunteer, part-local labels, absent invitee, unowned task",
  title:"Website relaunch sync",kind:"weekly project sync",daysAgo:2,capture:"desktop",wpm:150,
  people:{Halima:"mic",Corin:"system",Dessa:"system",Teodor:"system"},
  attendees:[{name:"Halima Yusufzai",email:"halima@saltmarshbooks.example",self:true},{name:"Corin Albright",email:"corin@saltmarshbooks.example"},{name:"Dessa Moreau",email:"dessa@saltmarshbooks.example"},{name:"Teodor Varga",email:"teodor@saltmarshbooks.example"},{name:"Annika Sorensen",email:"annika@saltmarshbooks.example"}],
  roster:["Annika"],
  questions:[],
  script:`
Halima: Morning all. Relaunch is the twenty eighth, so two and a half weeks. Annika can't make it today. Let's go round. Corin, redirects first. Where are we?
Corin: The redirect map covers about six hundred of the nine hundred old URLs. The rest are the old blog tags, and those need a rule, not a line each.
Halima: Corin, can you get the full redirect map done by Thursday?
Corin: Yes, Thursday's fine.
Halima: Good. Dessa, how's the pricing page?
Dessa: Copy is done and approved. What's missing is the screenshots, the ones in there are from the old checkout. I'll get the final screenshots from the product team by Wednesday.
Halima: Great. And I owe everyone the launch announcement. I'll write the draft and have it to you Monday.
Dessa: Monday works, I need it for the newsletter.
[+25]
Halima: Okay. Teodor, the certificate? I got an expiry warning email.
Teodor: Yeah, the current one expires on the sixteenth. I'll renew the SSL certificate before the fifteenth so we're not cutting it close.
Halima: Thank you. Now, who can own the four oh four page? Right now it's the default server error and it looks broken.
Dessa: I'll take that. I have a design for it already, I just need to build it.
Halima: Perfect, thanks Dessa. When?
Dessa: End of next week.
[+30]
Halima: Next one. Analytics. The new templates don't have the tracking tags on them yet, so on launch day we'd be blind.
Corin: Which means the first week of numbers is useless, and that's the week everybody asks about.
Teodor: I can look into the analytics tagging. Give me till the end of next week.
Halima: That would be great. Okay, legal. The new terms page needs a review before we publish.
Corin: That's Annika's, she said last week she'd do the legal review of the terms page.
Halima: Right, it's hers. I'll ping her today so it doesn't slip while she's out.
[+20]
Halima: Last thing. The agency still thinks we launch on the twenty first. Someone needs to tell them it's the twenty eighth now.
Corin: Yeah, they do need to know.
Dessa: They'll find out when the invoice changes.
Halima: Ha. Okay, we'll sort that out. Anything else? No? Thanks all, same time next week.
`,
  notes:{
    sparse:`relaunch 28th\nredirects - Thurs\ncert expires 16th\nagency doesn't know new date!`,
    detailed:`# Status\n- redirects: ~600 of 900 mapped, blog tags need a rule\n- pricing page copy approved, screenshots outdated\n- cert expires 16th\n\n# Gaps\n- 404 page is default server error\n- no analytics tags on new templates\n- terms page needs legal review (Annika)\n- agency still thinks launch = 21st\n\n# Owners\n- Corin: redirect map Thurs\n- me: announcement draft Mon\n- me: ping Annika re legal review\n- feels like nobody wants the agency call - chase next week`,
  },
  gold:{
    decisions:[],
    actionItems:[
      {id:"redirects",owner:"Corin",due:"Thursday",text:"Corin finishes the full redirect map by Thursday (asked by name, accepted).",must:["redirect","Thursday"],ownerMust:near("Corin","redirect",100)},
      {id:"screenshots",owner:"Dessa",due:"Wednesday",text:"Dessa gets the final pricing-page screenshots from the product team by Wednesday (first person, named in the same exchange).",must:["screenshot","Wednesday"],ownerMust:near("Dessa","screenshot",100)},
      {id:"announcement",owner:"Halima",due:"Monday",text:"Halima (the note owner, at the microphone) writes the launch announcement draft by Monday.",must:["announcement","Monday"],ownerMust:near("Halima","announcement",100)},
      {id:"certificate",owner:"Teodor",due:"before the 15th",text:"Teodor renews the SSL certificate before the 15th (it expires on the 16th).",must:["certificate|cert\\b","15th|fifteenth"],ownerMust:near("Teodor","certificate|cert\\b",100)},
      {id:"page-404",owner:"Dessa",due:"end of next week",text:"Dessa builds the 404 page by the end of next week (volunteered; Halima thanks her by name).",must:["404|four oh four"],ownerMust:near("Dessa","404|four oh four",100)},
      {id:"analytics",owner:"Teodor",due:"end of next week",unassigned:true,text:"Someone (in truth Teodor, but nothing in the stored transcript identifies the voice) offered to look into analytics tagging by the end of next week.",must:["analytics|tracking tag|tagging"],ownerMust:""},
      {id:"legal-review",owner:"Annika",text:"Annika (absent) owns the legal review of the new terms page.",must:["legal review|review[^.\\n]{0,40}terms|terms[^.\\n]{0,40}review"],ownerMust:near("Annika","legal|terms|review",100)},
      {id:"ping-annika",owner:"Halima",due:"today",text:"Halima pings Annika today about the legal review.",must:[near("Annika","ping|remind|follow up|message|contact|chase|nudge|check",100)],ownerMust:near("Halima",near("Annika","ping|remind|follow up|message|contact|chase|nudge|check",80),140)},
      {id:"agency",owner:"nobody",unassigned:true,text:"The agency must be told the launch moved from the 21st to the 28th; nobody took it.",must:["agency"],ownerMust:""},
    ],
    facts:[
      {id:"launch",text:"Relaunch is on the 28th, about two and a half weeks away.",must:["28th|twenty.eighth"]},
      {id:"redirect-progress",text:"About 600 of 900 old URLs are mapped; the old blog tags need a rule.",must:["600|six hundred","900|nine hundred"]},
      {id:"cert-expiry",text:"The current certificate expires on the 16th.",must:["16th|sixteenth"]},
      {id:"analytics-gap",text:"The new templates have no tracking tags yet, so launch-week numbers would be missing.",must:["tag"]},
      {id:"agency-date",text:"The agency still believes the launch is on the 21st.",must:["21st|twenty.first"]},
    ],
    notSaid:[
      {id:"agency-owned",claim:"A named person took the job of telling the agency.",forbid:["(?:Halima|Corin|Dessa|Teodor|Annika)(?: Yusufzai| Albright| Moreau| Varga| Sorensen)?(?::| will| to| is going to| agreed to)[^.\\n]{0,25}(?:tell|inform|notify|update|call|email|contact|let)[^.\\n]{0,25}agency"]},
      {id:"annika-attended",claim:"Annika attended or committed to something in this meeting.",forbid:["Annika (?:said|confirmed|agreed|reported|noted|mentioned|committed)(?! last week)"]},
      {id:"launch-21",claim:"The launch is on the 21st.",forbid:["(?<!(?:expects?|thinks?|believes?|assumes?|still has|told)[^.\\n]{0,30})(?:launch|relaunch)(?: date)? (?:is|on|set for|scheduled for)(?: the)? 21st(?!,? not)"]},
    ],
  },
}
