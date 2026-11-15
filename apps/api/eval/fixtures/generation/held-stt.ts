import type {GenerationFixture} from "../../lib/generation-fixtures"
import {near} from "../../lib/transcript"

/** HELD OUT: never used while tuning the generation prompt.
 * A podcast guest-prep video call as speech-to-text renders it: the guest's
 * name comes out three different ways, a show name is split into words, people
 * restart sentences and talk over each other, and two numbers are only safe
 * after they are spelled out (sixteen, not sixty; the ninth, not the nineteenth).
 * The calendar event carries the correct spellings. Fictional show and people. */
export const heldStt:GenerationFixture={
  id:"held-stt",split:"held-out",stress:"speech-to-text errors in names and numbers, disfluent speech, cross-talk (desktop capture)",
  title:"Guest prep: Cveta Marinova",kind:"podcast guest prep call",daysAgo:1,capture:"desktop",wpm:160,
  people:{Anders:"mic",Cveta:"system",Ruairi:"system"},
  attendees:[{name:"Anders Lindqvist",email:"anders@fieldrecordings.example",self:true},{name:"Cveta Marinova",email:"cveta@marinovalab.example"},{name:"Ruairi Donnelly",email:"ruairi@fieldrecordings.example"}],
  roster:["Sveta","Sweater","Rory","Rury"],
  questions:[],
  script:`
Anders: Hi, hi, can you both hear me? Okay good. So Sveta thank you for for doing this, we're really excited to have you on field recordings.
Cveta: Oh no, thank you. I've, um, I've listened to a few, the one about the the lighthouse keepers was lovely.
Anders: That's kind. So this is just a prep call, nothing is recorded for the show. Rory is our producer, he'll he'll walk you through the logistics and then I want to talk about the the shape of the conversation.
Ruairi: Hi Sweater, sorry, Sveta. So recording is on the ninth.
Cveta: The nineteenth?
Ruairi: Ninth. Zero nine. A week on Thursday. Ten in the morning your time.
Cveta: Ninth, ten a.m., okay. I had the nineteenth in my head from from the first email.
Ruairi: Yeah that was the old date, sorry about that. So it's remote, we send you a microphone, you don't need anything except a quiet room and and headphones.
Cveta: I have headphones. How long is it?
Ruairi: We record for about ninety minutes and the episode comes out at around forty.
Anders: And you get to hear the edit before it goes out. If there's anything you you regret saying, we cut it, no questions.
Cveta: That's reassuring. I do tend to
~Anders: Everyone says that.
Cveta: I tend to go on about the the methodology and then regret it.
Anders: We like methodology. So for the shape, I was thinking three parts. How you got into soundscape ecology, then the the reef project, then what you'd tell someone starting out.
Cveta: That works. On the reef project, I can't talk about the the twenty twenty seven data yet, it's under embargo until the paper is out.
Anders: Understood. So everything up to and including the the twenty twenty six season is fine?
Cveta: Yes. Twenty twenty six and earlier is all published.
Anders: Great. And how many hydrophones was it in the end? I read sixty somewhere.
Cveta: Sixteen. One six. Sixty would have been nice. Sixteen hydrophones across four sites.
Anders: Sixteen across four sites. I'll fix that in my notes, I had sixty.
Ruairi: Can I ask about audio? Do you have any of the reef recordings we could play in the episode?
Cveta: I do. I can send you maybe five or six clips. They're big files though.
Ruairi: I'll send you an upload link today, it takes anything up to two gigabytes.
Cveta: Perfect. I'll upload the clips by Monday then.
Ruairi: And the microphone, I need a shipping address.
Cveta: I'll email you the lab address after this, it's easier than my flat.
Anders: One more thing from me. I'll send you the question outline by Friday so nothing is a surprise. You don't need to prepare answers, it's just so you know where we're going.
Cveta: Thank you, I'd like that.
Ruairi: Oh and Sveta, the release form. Legal sends it, not me, so it'll come from a different address. Just so you don't think it's spam.
Cveta: Okay, I'll look out for it.
Anders: I think that's everything. Thank you so much.
Cveta: Thank you both. See you on the ninth.
`,
  notes:{
    sparse:`record 9th 10am her time\n2027 data embargoed\n16 hydrophones not 60\nshe's nervous about rambling - reassure`,
    detailed:`# Logistics\n- recording 9th, 10am her time, remote\n- ~90 min raw -> ~40 min episode\n- she hears the edit before release\n- mic shipped to her lab\n\n# Shape\n- 1 how she got into soundscape ecology\n- 2 reef project\n- 3 advice for someone starting out\n\n# Constraints\n- 2027 data embargoed until paper is out; 2026 and earlier fine\n- 16 hydrophones / 4 sites (I had 60 - wrong)\n\n# To do\n- me: question outline by Fri\n- Ruairi: upload link today\n- Cveta: clips by Mon, lab address\n- my read: nervous about rambling on methodology - tell her we like it`,
  },
  gold:{
    decisions:[
      {id:"shape",text:"The conversation has three parts: how she got into soundscape ecology, the reef project, advice for someone starting out.",must:["soundscape","reef"]},
    ],
    actionItems:[
      {id:"outline",owner:"Anders",due:"Friday",text:"Anders (the note owner) sends the question outline by Friday.",must:["outline","Friday"],ownerMust:near("Anders","outline",100)},
      {id:"upload-link",owner:"Ruairi",due:"today",text:"Ruairi sends an upload link today (files up to 2 GB).",must:["upload link|link"],ownerMust:near("Ruairi|Rory","link",100)},
      {id:"clips",owner:"Cveta",due:"Monday",text:"Cveta uploads five or six reef clips by Monday.",must:["clip|recording","Monday"],ownerMust:near("Cveta|Sveta","clip|upload",100)},
      {id:"address",owner:"Cveta",text:"Cveta emails her lab address for the microphone shipment.",must:["address"],ownerMust:near("Cveta|Sveta","address",100)},
    ],
    facts:[
      {id:"date",text:"Recording is on the 9th (a week on Thursday) at 10 a.m. her time, remote; the 19th was the old date.",must:["9th|ninth","10"]},
      {id:"length",text:"About 90 minutes are recorded for an episode of about 40; the guest hears the edit before release.",must:["90|ninety","40|forty"]},
      {id:"embargo",text:"2027 reef data is embargoed until the paper is out; 2026 and earlier is published and fine to discuss.",must:["2027|twenty twenty.seven","embargo"]},
      {id:"hydrophones",text:"There were 16 hydrophones across four sites (not 60).",must:["16|sixteen"]},
      {id:"release-form",text:"The release form will come from legal, from a different address.",must:["release form"]},
      {id:"names-spelled",text:"People are named as in the calendar event (Cveta, Ruairi), not as transcribed (Sveta, Rory).",must:["Cveta","Ruairi"]},
      {id:"show-name",text:"The show is Field Recordings.",must:["Field Recordings"]},
    ],
    notSaid:[
      {id:"nineteenth",claim:"Recording is on the 19th.",forbid:["record(?:ing|s)?[^.\\n]{0,25}(?:on|is)(?: the)? 19th","19th at 10"]},
      {id:"sixty",claim:"There were 60 hydrophones.",forbid:["(?:60|sixty) hydrophones(?! (?:was|were|is) (?:wrong|incorrect|a mistake))"]},
      {id:"misheard-names",claim:"Someone called Sveta, Sweater or Rory took part.",forbid:["\\bSveta\\b","\\bSweater\\b","\\bRory\\b"]},
      {id:"embargo-2026",claim:"The 2026 data is embargoed.",forbid:["2026 (?:data|season|results)? ?(?:is|are|remains?) (?:under )?embargo"]},
      {id:"ruairi-release",claim:"Ruairi is sending the release form.",forbid:["Ruairi(?: Donnelly)?(?::| will| to| is going to)[^.\\n]{0,20}send[^.\\n]{0,25}release form"]},
      {id:"prepare-answers",claim:"The guest was asked to prepare answers.",forbid:["Cveta(?: Marinova)?(?::| will| to| should| needs to)[^.\\n]{0,15}prepare (?:her )?answers"]},
    ],
  },
}
