import type {Fixture} from "../lib/types"
import {near} from "../lib/transcript"

/** Short daily stand-up. Fictional team, product and people. */
export const standup:Fixture={
  id:"standup",title:"Ferry app stand-up",kind:"short stand-up",daysAgo:1,capture:"desktop",wpm:155,fast:true,
  people:{Imani:"mic",Dario:"system",Wen:"system",Kofi:"system"},
  script:`
Imani: Morning everyone, let's keep this to five minutes. Dario, do you want to start?
Dario: Sure. Yesterday I finished the payment retry fix, the one where a declined card could get charged twice on a slow connection. The pull request is up and Wen has started reviewing it.
Dario: Today I want to look at the flaky checkout test, because it failed three times on main overnight and I don't think it's related to my change.
Imani: Okay. Anything blocking you?
Dario: No blockers.
Imani: Wen?
Wen: I reviewed about half of Dario's pull request, I'll finish it this morning. My own thing is the address autocomplete, and I'm blocked. The staging certificate expired last night, so every call from the staging app to the address service fails the handshake.
Imani: That's the certificate Kofi's team owns, right?
Wen: Yes. I can't test anything end to end until it's rotated.
Imani: Kofi, can you rotate the staging certificate today?
Kofi: Yes, I saw the alert. I'll rotate it before noon. It's a ten minute job, I just need the approval from security and they're usually quick.
Wen: Great, noon works. I'll write unit tests until then.
Kofi: My update. The release candidate build, two point fourteen, was supposed to be cut on Wednesday. I'd like to move that to Thursday, because the crash reporting upgrade is not merged yet and I don't want to cut a candidate without it.
Imani: Does that put the store submission at risk?
Kofi: No. Submission is still next Monday. We just lose one day of soak time, so four days instead of five.
Imani: Fine, let's do Thursday for the release candidate. I'll let Priyanka know, she's out today and she is expecting Wednesday.
Dario: One thing, Wen, do you have twenty minutes after this to pair on the flaky checkout test? You wrote the original fixture.
Wen: Sure, right after stand-up.
Imani: Good. Last item, the Friday demo. Do we have anything to show?
Dario: The retry fix is not very visual.
Kofi: I'd rather skip a week than demo a settings screen again.
Imani: Let's not decide that now, I'll ask Priyanka when she is back tomorrow. Anything else? Okay, thanks all.
`,
  notes:{
    sparse:`standup\n- retry fix PR up\n- staging cert!! Kofi\n- RC 2.14 -> Thurs`,
    detailed:`# Updates\n- Dario: payment retry fix done, PR in review with Wen\n- Wen: address autocomplete blocked on expired staging cert\n\n# Release\n- RC 2.14 cut moves Wed -> Thu (crash reporting upgrade not merged)\n- store submission still Monday?\n\n# To do\n- tell Priyanka about RC date\n- Friday demo - undecided, ask P`,
  },
  gold:{
    decisions:[
      {id:"rc-thursday",text:"The 2.14 release candidate will be cut on Thursday instead of Wednesday.",must:["Thursday","release candidate|\\bRC\\b|2\\.14"]},
    ],
    actionItems:[
      {id:"rotate-cert",owner:"Kofi",due:"before noon today",text:"Kofi rotates the expired staging certificate before noon.",must:["certificate|cert\\b","rotat"],ownerMust:near("Kofi","cert")},
      {id:"pair-flaky",owner:"Dario and Wen",due:"after stand-up",text:"Dario and Wen pair on the flaky checkout test after stand-up.",must:["flaky|checkout test","pair"],ownerMust:near("Dario|Wen","flaky|checkout test")},
      {id:"tell-priyanka",owner:"Imani",text:"Imani tells Priyanka the release candidate moved to Thursday.",must:["Priyanka"],ownerMust:near("Priyanka","Thursday|release candidate|RC\\b|date|demo")},
      {id:"finish-review",owner:"Wen",due:"this morning",text:"Wen finishes reviewing Dario's payment retry pull request this morning.",must:["review"],ownerMust:near("Wen","review")},
    ],
    facts:[
      {id:"retry-fix",text:"The payment retry fix (double charge on declined card over slow connection) is done and in review.",must:["retry"]},
      {id:"cert-expired",text:"The staging certificate expired, blocking end-to-end testing of address autocomplete.",must:["certificate|cert\\b","expired|block"]},
      {id:"submission-monday",text:"Store submission stays next Monday; soak time drops from five days to four.",must:["Monday"]},
      {id:"crash-reporting",text:"The release candidate moved because the crash reporting upgrade is not merged.",must:["crash reporting"]},
    ],
    notSaid:[
      {id:"demo-cancelled",claim:"The Friday demo was cancelled or skipped (it was left undecided).",forbid:["(?:demo)[^.\\n]{0,40}(?:was|is|will be) (?:cancel|skipp)","(?:decided|agreed) to (?:skip|cancel)[^.\\n]{0,30}demo"]},
      {id:"submission-slips",claim:"The store submission date slipped.",forbid:["submission[^.\\n]{0,40}(?:moved|slipp|delayed|pushed) to","submission (?:on|is) (?:Tuesday|Wednesday|Thursday|Friday)"]},
      {id:"android",claim:"An Android crash or hotfix was discussed.",forbid:["Android","hotfix"]},
    ],
  },
  questions:[
    {id:"when-rc",kind:"factual",fast:true,question:"When is the release candidate being cut now, and why did it move?",reference:"Thursday instead of Wednesday, because the crash reporting upgrade is not merged yet. Store submission stays next Monday, with four days of soak time instead of five.",must:["Thursday","crash reporting"],forbid:["submission[^.\\n]{0,40}(?:moved|slipp|delayed)"],support:["I'd like to move that to Thursday"]},
    {id:"who-blocked",kind:"citation",question:"Who is blocked and on what? Cite the transcript.",reference:"Wen is blocked on address autocomplete because the staging certificate expired; Kofi will rotate it before noon.",must:["certificate|cert\\b","noon|rotat"],support:["The staging certificate expired last night","I'll rotate it before noon"]},
    {id:"demo",kind:"absent",fast:true,question:"What did the team decide about the Friday demo?",reference:"Nothing was decided. Imani will ask Priyanka tomorrow; Kofi would prefer to skip a week.",must:["not decide|no decision|undecided|didn't decide|did not decide|wasn't decided|was not decided|deferred|ask Priyanka|not yet"],forbid:["(?:decided|agreed) to (?:skip|cancel|hold|go ahead)"],support:["Let's not decide that now"]},
    {id:"android",kind:"absent",question:"What was said about the Android crash?",reference:"Nothing. An Android crash was not discussed in this stand-up.",must:["not (?:mention|discuss|cover|address|come up)|no (?:mention|discussion|reference|information)|wasn't|was not|doesn't|does not|didn't|did not|nothing"],forbid:["Android crash (?:was|is) (?:caused|fixed|due)"]},
    {id:"todo-summary",kind:"synthesis",question:"List everyone's commitments for today.",reference:"Kofi: rotate the staging certificate before noon. Wen: finish reviewing Dario's pull request this morning, write unit tests until the certificate is rotated, pair with Dario on the flaky checkout test after stand-up. Dario: investigate the flaky checkout test. Imani: tell Priyanka about the Thursday release candidate.",must:["certificate|cert\\b","flaky|checkout test","review","Priyanka"]},
    {id:"follow-soak",kind:"follow-up",question:"How much soak time does that leave?",history:[{role:"user",content:"When is the release candidate being cut?"},{role:"assistant",content:"The 2.14 release candidate is now being cut on Thursday instead of Wednesday."}],reference:"Four days instead of five; store submission is still next Monday.",must:["four|\\b4\\b"],support:["four days instead of five"]},
  ],
}
