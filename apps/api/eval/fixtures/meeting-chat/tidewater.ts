import {near} from "../../lib/transcript"
import {ABSTAIN,type McFixture} from "./types"

/** TUNED. A launch-readiness call written the way speech-to-text returns it:
 * fillers, restarts, misheard proper nouns ("Lyden", "vol taro", "nord lease"),
 * a self-corrected number, a decision that changes, a proposal that is left
 * open, facts that exist only in the typed notes, and nothing about helmets. */
export const tidewater:McFixture={
  set:"tuned",id:"mc-t-tidewater",title:"Tidewater Bikes: Leiden launch readiness",kind:"launch readiness with transcription errors",daysAgo:1,capture:"desktop",wpm:150,fast:true,
  people:{Ottilie:"mic",Ravi:"system",Calla:"system",Jonas:"system"},
  attendees:[{name:"Ottilie Brandsma",email:"ottilie@tidewater.example",self:true},{name:"Ravi Menon",email:"ravi@tidewater.example"},{name:"Calla Whitlock",email:"calla@tidewater.example"},{name:"Jonas Aalto",email:"jonas@tidewater.example"}],
  script:`
Ottilie: Okay, um, I think that's everyone who's coming. So this is the, the launch readiness check for Lyden. I want to go through fleet, the app, insurance, pricing, marketing, and then anything else. Ravi, you want to start with the bikes?
Ravi: Yeah. So, uh, we ordered three hundred and forty bikes in total. Three hundred of those land at the depot on April twenty eighth. The other forty are, they're stuck with the frame supplier, they come May twelfth.
Ottilie: So three hundred first and then forty two weeks later.
Ravi: Right. And that's kind of why I wanted to talk about the date, because, um, there's a second problem which is the batteries.
Ottilie: Go on.
Ravi: So vol taro, the battery people, they emailed Monday. They're recalling sixty packs from our batch. It's a connector thing, it's not a fire thing, but we can't put those on the street.
Calla: Sixty out of how many?
Ravi: Sixty out of the three forty. Replacements take three weeks from when they ship, and they said they ship this Friday.
Ottilie: Okay so, three weeks from Friday. That's, what, that takes us past May sixth.
Ravi: It does. May sixth we'd be launching with, like, two hundred and forty good bikes. I don't love it.
Ottilie: No. Me neither. Um. What if we move the launch two weeks. May twentieth.
Calla: I can live with the twentieth. I haven't sent anything public with the sixth on it yet, it's only on the wait list page and I can change that today.
Ottilie: Then let's do that. Launch moves from May sixth to May twentieth. Ravi, you'll chase vol taro so the replacements actually ship Friday?
Ravi: Yeah I'll chase them. I'll, I'll get a tracking number out of them.
Ottilie: Good. Um, app. Where are we.
Ravi: App is mostly fine. The one thing that's not fine is unlock on android fourteen. About one in five unlocks just fails, the bike doesn't hear the phone. I phone is fine.
Ottilie: One in five is a lot.
Ravi: It is a lot. We know what it is, it's the blue tooth permission changing in the background. Sunil has a fix in review. I'll have it shipped by April thirtieth.
Ottilie: April thirtieth, okay. And the stations, how many do we actually have?
Ravi: Fourteen hubs planned. Nine are confirmed by the city. The other five are waiting on permits and the permit decision is supposed to come May eighth.
Ottilie: So worst case we launch with nine hubs.
Ravi: Worst case nine, yeah. Which is okay for a soft start, it's not okay for the full wait list.
Ottilie: And repairs, did we sign with the, the shop?
Ravi: Dekker, yeah. Forty five an hour, and they promise a forty eight hour turnaround. I haven't signed, I have the contract, I'll sign it this week.
Ottilie: Okay. And the deposit flow, did we land on a number?
Calla: We said fifty. No sorry, seventy five. Fifty was the old number before we looked at what the locks cost. Seventy five euros, refundable.
Ottilie: Seventy five refundable. Fine.
Jonas: Sorry, sorry I'm late. I was on with nord lease, actually, so it's good timing.
Ottilie: Perfect. Jonas, go, insurance.
Jonas: So. They will cover the fleet. But, and it's a big but, they want a theft tracker on every single bike. Not just the, not a sample. All of them.
Ravi: We budgeted trackers for like a third of them.
Jonas: I know. The tracker they'll accept is eleven fifty per bike per month. Eleven euros fifty.
Ottilie: Per bike per month. On three hundred forty bikes.
Jonas: Yeah. I haven't done the, I'll put it in the model. And they want an answer by Friday or the quote lapses and we start over.
Ottilie: Okay. Can you confirm with them by Friday then? I don't think we have a choice, we can't launch uninsured.
Jonas: I'll confirm by Friday. I'm going to try to get the first month waived but I wouldn't count on it.
Ottilie: Okay. Pricing. Calla.
Calla: So the standard subscription is thirty nine a month. And then the bundle with theft cover and priority repairs is fifty four.
Jonas: And those are final?
Calla: Those are final, they're on the page. What's not final is the student thing. I'd like to do fifteen percent off for students, because, I mean, it's lie den, half the city is students.
Jonas: Fifteen percent on thirty nine is, um, that's under what we need per bike once you add the tracker. I'm not saying no. I'm saying I want to see it in the model first.
Ottilie: So let's not decide the student discount today. Jonas runs the numbers, we come back to it next week.
Calla: Fine. But I want it noted that I think we'll regret launching without it.
Ottilie: Noted. Um, support hours, did that get settled, because I keep getting asked.
Calla: Yes. Weekdays eight to eight. Weekends ten to four. Chat only, no phone line for now.
Ottilie: Eight to eight, ten to four on weekends, chat only. Okay. Marketing.
Calla: So the budget you approved is eighteen thousand for launch. Of that, six thousand five hundred goes to campus events, that's the two university fairs and the bike breakfast thing. The rest is paid social and the posters.
Ottilie: And the wait list?
Calla: Wait list is at four hundred and twelve as of this morning.
Ottilie: That's more than we have bikes for on day one, honestly.
Calla: Which is my actual question. Do we open to everybody on the twentieth, or do we do a soft launch.
Ravi: I'd vote soft. If android unlock has any more surprises I'd rather find out with a hundred people than four hundred.
Ottilie: Yeah. Okay. Soft launch. First one hundred and twenty people off the wait list get invited on the twentieth, everybody else two weeks after if nothing's on fire.
Calla: First one twenty. Okay. I'll draft the invite email and send it to you by Thursday so you can look at it.
Ottilie: Thursday's great. Um, what else. Oh, the depot.
Jonas: Yeah the depot lease. The landlord came back. He wants two thousand one hundred fifty a month, up from nineteen hundred.
Ottilie: Twenty one fifty. Hm. And we need to tell him by when?
Jonas: I told him we'd answer by the third. May third.
Ottilie: Okay. I don't want to decide that on this call, I want to see if the Zoeterwoude unit is still free first. I'll call them tomorrow.
Ravi: The Zoeterwoude one doesn't have charging though. Just so you know.
Ottilie: Right. Yeah. That's a real problem. Okay, I'll still call.
Calla: One thing, the launch day photo shoot, I need two bikes that are, like, clean, on the nineteenth.
Ravi: I'll set two aside.
Ottilie: Great. Okay, I think that's it. Thanks all. Ravi, send me the vol taro tracking number when you have it.
Ravi: Will do.
`,
  notes:{
    sparse:`Leiden launch readiness\n- fleet 340 (300 + 40 late)\n- Voltaro recall!!\n- Nordlys = insurer, tracker on ALL bikes\n- student discount?? -> Jonas model\n- depot: landlord is Verbeek, counter at 2000?\n- CAC target 42 (board pre-read)`,
    detailed:`# Leiden launch readiness\n\n## Fleet\n- 340 ordered: 300 on Apr 28, 40 on May 12\n- Voltaro recall: 60 packs, replacements 3 wks\n\n## Insurance\n- Nordlys wants a tracker on ALL bikes, 11.50/bike/month\n- Jonas confirms by Fri\n\n## Pricing\n- 39 standard / 54 bundle\n- student discount?? -> Jonas model, next week\n\n## Launch\n- moved May 6 -> May 20\n- soft launch, first 120\n\n## Depot\n- landlord is Verbeek, wants 2150, counter at 2000?\n- CAC target 42 (board pre-read)`,
  },
  gold:{
    decisions:[
      {id:"date",text:"The Leiden launch moves from May 6 to May 20 because of the battery recall.",must:["May 20|20 May|twentieth"]},
      {id:"soft-launch",text:"Soft launch: the first 120 people on the wait list are invited on May 20, everyone else two weeks later if nothing goes wrong.",must:["soft launch","120|one hundred (?:and )?twenty"]},
      {id:"deposit",text:"The deposit is 75 euros, refundable.",must:["75|seventy.five"]},
    ],
    actionItems:[
      {id:"ravi-voltaro",owner:"Ravi",due:"Friday",text:"Ravi chases Voltaro so the replacement battery packs ship on Friday and gets a tracking number.",must:["Voltaro|vol taro|batter","tracking|ship|chase"],ownerMust:near("Ravi","Voltaro|vol taro|batter|tracking")},
      {id:"ravi-android",owner:"Ravi",due:"April 30",text:"Ravi ships the fix for unlock failures on Android 14 by April 30.",must:["android","April 30|30 April|thirtieth"],ownerMust:near("Ravi","android|unlock")},
      {id:"jonas-insurer",owner:"Jonas",due:"Friday",text:"Jonas confirms the insurance with Nordlys by Friday.",must:["Nordlys|nord lease|insur","Friday"],ownerMust:near("Jonas","Nordlys|nord lease|insur")},
      {id:"jonas-student",owner:"Jonas",due:"next week",text:"Jonas models the proposed 15% student discount for next week.",must:["student"],ownerMust:near("Jonas","student|model|numbers")},
      {id:"calla-email",owner:"Calla",due:"Thursday",text:"Calla drafts the invite email and sends it to Ottilie by Thursday.",must:["email","Thursday"],ownerMust:near("Calla","email|invite")},
      {id:"ottilie-unit",owner:"Ottilie",due:"tomorrow",text:"Ottilie calls about the Zoeterwoude unit tomorrow before answering the depot landlord.",must:["Zoeterwoude"],ownerMust:near("Ottilie|\\bI\\b|\\byou\\b","Zoeterwoude")},
    ],
    facts:[
      {id:"fleet",text:"340 bikes ordered: 300 arrive April 28, the other 40 on May 12.",must:["340|three hundred (?:and )?forty","300|three hundred"]},
      {id:"recall",text:"Voltaro is recalling 60 battery packs; replacements take three weeks from shipping.",must:["60|sixty","three weeks|3 weeks"]},
      {id:"tracker",text:"The insurer Nordlys requires a theft tracker on every bike, at 11.50 euros per bike per month.",must:["tracker","11[.,]50|eleven (?:euros? )?fifty"]},
      {id:"prices",text:"Subscription is 39 euros a month, or 54 for the bundle with theft cover and priority repairs.",must:["39|thirty.nine","54|fifty.four"]},
      {id:"budget",text:"Launch marketing budget is 18,000 euros, of which 6,500 is for campus events.",must:["18,?000|eighteen thousand","6,?500|six thousand five hundred"]},
      {id:"lease",text:"The depot landlord wants 2,150 euros a month, up from 1,900; an answer is due by May 3.",must:["2,?150|twenty.one fifty|two thousand one hundred fifty","May 3|3 May|May third|the third"]},
    ],
    notSaid:[
      {id:"student-agreed",claim:"A student discount was agreed.",forbid:["(?:agreed|decided|approved|confirmed)[^.\\n]{0,40}(?:15|fifteen) ?(?:%|percent)[^.\\n]{0,30}student","student discount (?:of (?:15|fifteen) ?(?:%|percent) )?(?:was|is|has been) (?:agreed|approved|decided|confirmed)"]},
      {id:"lease-accepted",claim:"The new depot rent was accepted.",forbid:["(?:accepted|agreed to|signed)[^.\\n]{0,40}(?:2,?150|lease|rent)"]},
      {id:"helmets",claim:"Anything was said about helmets.",forbid:["helmets? (?:are|is|will be|were) (?:required|included|provided|mandatory|optional)"]},
    ],
  },
  questions:[
    {id:"first-delivery",tag:"early",kind:"factual",fast:true,question:"How many bikes arrive in the first delivery, and when?",reference:"300 of the 340 bikes arrive at the depot on April 28. The remaining 40 arrive on May 12.",must:["300|three hundred","April 28|28 April|April twenty.eighth"],support:["Three hundred of those land at the depot on April twenty eighth"]},
    {id:"insurer-requirement",tag:"middle",kind:"factual",question:"What does the insurer require, and what does it cost per bike?",reference:"The insurer requires a theft tracker on every bike, not a sample. The tracker they accept costs 11.50 euros per bike per month.",must:["tracker","11[.,]50|eleven (?:euros? )?fifty"],support:["The tracker they'll accept is eleven fifty per bike per month"]},
    {id:"lease-terms",tag:"late",kind:"factual",fast:true,question:"What does the depot landlord want per month, and by when do we have to answer?",reference:"2,150 euros a month, up from 1,900. Jonas told him we would answer by May 3.",must:["2,?150|twenty.one fifty|two thousand one hundred fifty","May 3|3 May|May third|the third"],support:["He wants two thousand one hundred fifty a month"]},
    {id:"tracker-total",tag:"arithmetic",kind:"factual",fast:true,question:"What will the trackers cost us per month across the whole fleet?",reference:"About 3,910 euros a month: 340 bikes at 11.50 euros each. Nobody stated the total in the meeting; Jonas said he would put it in the model.",must:["3,?910"],support:["The tracker they'll accept is eleven fifty per bike per month"]},
    {id:"budget-left",tag:"arithmetic",kind:"factual",question:"How much of the launch marketing budget is left after the campus events?",reference:"11,500 euros: the budget is 18,000 and 6,500 goes to campus events. The rest is for paid social and posters.",must:["11,?500"],support:["six thousand five hundred goes to campus events"]},
    {id:"landlord-name",tag:"notes-only",kind:"factual",fast:true,question:"Who is the depot landlord?",reference:"Verbeek, according to your typed notes. In the conversation he was only called the landlord.",must:["Verbeek"]},
    {id:"cac-target",tag:"notes-only",kind:"factual",question:"What is our CAC target?",reference:"42, from your typed notes (the board pre-read). It was not discussed in the meeting.",must:["\\b42\\b"]},
    {id:"support-hours",tag:"transcript-only",kind:"factual",question:"What are the support hours?",reference:"Weekdays 8 to 8 and weekends 10 to 4, chat only, with no phone line for now.",must:["8[^\\n]{0,40}(?:8|20)|eight to eight","10[^\\n]{0,40}(?:4|16)|ten to four"],support:["Weekdays eight to eight. Weekends ten to four."]},
    {id:"android-failure-rate",tag:"transcript-only",kind:"factual",withSummary:true,fast:true,question:"How often does unlock fail on Android, and what is causing it?",reference:"About one in five unlocks fails on Android 14, because the Bluetooth permission changes in the background. iPhone is fine. A fix is in review and Ravi will ship it by April 30.",must:["one in five|1 in 5|20 ?%|a fifth","bluetooth|blue tooth|permission"],support:["About one in five unlocks just fails"]},
    {id:"helmets",tag:"not-said",kind:"absent",fast:true,question:"What did they decide about helmets?",reference:"Nothing. Helmets did not come up in this meeting.",must:[ABSTAIN]},
    {id:"student-discount",tag:"undecided",kind:"factual",fast:true,question:"What student discount was agreed?",reference:"None was agreed. Calla proposed 15% off for students; Jonas wants to see it in the model first, and Ottilie deferred the decision to next week.",must:["not|no\\b|none|defer|wasn't|hasn't|didn't|undecided|open","15|fifteen"]},
    {id:"battery-follow-up",tag:"follow-up",kind:"follow-up",fast:true,question:"How long will that take to sort out, and who is chasing it?",
      history:[{role:"user",content:"What's the problem with the batteries?"},{role:"assistant",content:"The battery supplier is recalling 60 packs from the batch because of a connector fault, so those bikes cannot go on the street."}],
      reference:"Replacements take three weeks from when they ship, and the supplier said they ship this Friday. Ravi is chasing Voltaro and will get a tracking number.",must:["three weeks|3 weeks","Ravi"],support:["Replacements take three weeks from when they ship"]},
    {id:"date-follow-up",tag:"follow-up",kind:"follow-up",question:"Does that change the date?",
      history:[{role:"user",content:"What's the problem with the batteries?"},{role:"assistant",content:"The battery supplier is recalling 60 packs from the batch because of a connector fault, so those bikes cannot go on the street."},{role:"user",content:"How long will that take to sort out, and who is chasing it?"},{role:"assistant",content:"Replacements take three weeks from when they ship, which should be this Friday. Ravi is chasing the supplier for a tracking number."}],
      reference:"Yes. The launch moves from May 6 to May 20, two weeks later, because on May 6 there would only have been about 240 usable bikes.",must:["May 20|20 May|twentieth"],support:["Launch moves from May sixth to May twentieth"]},
    {id:"spanish",tag:"language",kind:"factual",fast:true,question:"¿Cuándo es el lanzamiento ahora y cuántas personas entran el primer día?",reference:"El lanzamiento es ahora el 20 de mayo (antes era el 6 de mayo). Será un lanzamiento gradual: el primer día se invita a las primeras 120 personas de la lista de espera.",must:["20 de mayo","120","\\b(?:lanzamiento|personas|primer|lista)\\b"],support:["First one hundred and twenty people off the wait list"]},
    {id:"cite-date-change",tag:"citation",kind:"citation",fast:true,question:"Where exactly did the launch date change? Cite the transcript.",reference:"Ottilie proposed moving the launch two weeks to May 20, Calla said she could live with it, and Ottilie confirmed that the launch moves from May 6 to May 20.",must:["May 20|20 May|twentieth"],support:["Launch moves from May sixth to May twentieth"]},
    {id:"action-items",tag:"synthesis",kind:"synthesis",fast:true,question:"List every action item with its owner and due date.",reference:"Ravi: chase Voltaro so replacements ship Friday and get a tracking number; ship the Android 14 unlock fix by April 30; sign the Dekker repair contract this week; set two clean bikes aside for the photo shoot on the 19th. Jonas: confirm with Nordlys by Friday; model the student discount for next week. Calla: change the date on the wait list page today; send Ottilie the invite email draft by Thursday. Ottilie: call about the Zoeterwoude unit tomorrow.",must:["Ravi","Jonas","Calla","April 30|30 April|thirtieth","Thursday","Friday"]},
    {id:"my-commitments",tag:"speaker",kind:"factual",question:"What did I personally commit to doing?",reference:"You said you would call about the Zoeterwoude unit tomorrow, to see whether it is still free before answering the depot landlord. You will also look at Calla's invite email draft when she sends it on Thursday.",must:["Zoeterwoude"],forbid:["(?:you|I)(?: will|'ll| would| agreed to| committed to)[^.\\n]{0,25}(?:chase|ship the|draft the|confirm with)"]},
    {id:"list-my-todos",tag:"recipe",kind:"synthesis",question:"List my todos",reference:"To do\n- Call about the Zoeterwoude unit tomorrow, before answering the depot landlord (the answer is due by May 3).\n- Review Calla's invite email draft when it arrives on Thursday.\n\nInferred\n- Decide on the depot lease once you know whether the Zoeterwoude unit is free.\n- Come back to the student discount next week, after Jonas has modelled it.",must:["Zoeterwoude"],forbid:["(?:you|I)(?: will|'ll| would| agreed to| committed to)[^.\\n]{0,25}(?:chase|ship the|draft the|confirm with)"]},
    {id:"write-tldr",tag:"recipe",kind:"synthesis",question:"Write tldr",reference:"Launch readiness check for the Leiden bike subscription.\n- The launch moves from May 6 to May 20 because of a battery recall, and starts as a soft launch for the first 120 people on the wait list.\n- The insurer requires a tracker on every bike at 11.50 euros per bike per month; Jonas confirms by Friday.\n- Still open: the student discount and the depot lease.",must:["May 20|20 May|twentieth"]},
    {id:"supplier-name",tag:"stt",kind:"factual",fast:true,question:"Which battery supplier is doing the recall, and which insurer was Jonas talking to?",reference:"The battery supplier is Voltaro and the insurer is Nordlys. The transcript renders them as \"vol taro\" and \"nord lease\"; the spellings come from your typed notes.",must:["Voltaro","Nordlys"]},
  ],
  live:[
    {id:"miss-early",kind:"recency",recipe:true,fast:true,after:"May sixth we'd be launching with, like, two hundred and forty good bikes. I don't love it.",question:"What did I miss?",
      reference:"- The battery supplier is recalling 60 packs; replacements take three weeks from shipping, which is due Friday.\n- That runs past May 6: on that date there would be only about 240 usable bikes, and Ravi is not comfortable with it.",must:["batter|recall|pack","240|two hundred (?:and )?forty|May (?:6|sixth)|three weeks|3 weeks"],later:["May 20|twentieth","nord|insur|tracker"]},
    {id:"launch-before-move",kind:"superseded",after:"Okay so, three weeks from Friday. That's, what, that takes us past May sixth.",question:"When are we launching?",
      reference:"The date on the table is still May 6, but it is in doubt: the replacement batteries arrive three weeks after Friday, which is past May 6. No new date has been set yet.",must:["May (?:6|sixth)|\\bsixth\\b"],later:["May 20|twentieth"]},
    {id:"launch-after-move",kind:"superseded",fast:true,after:"Launch moves from May sixth to May twentieth. Ravi, you'll chase vol taro so the replacements actually ship Friday?",question:"When are we launching?",
      reference:"May 20. It was just moved from May 6 because of the battery recall.",must:["May 20|20 May|twentieth"],later:["soft launch","nord|insur"]},
    {id:"who-insurer",kind:"speaker",after:"Not just the, not a sample. All of them.",question:"Who is talking about the insurer right now?",
      reference:"Jonas. He joined late, said he had just been on a call with the insurer, and Ottilie asked him by name to cover insurance.",must:["Jonas"],later:["eleven fifty|11[.,]50","Friday"]},
    {id:"that-number",kind:"just-said",fast:true,after:"The tracker they'll accept is eleven fifty per bike per month. Eleven euros fifty.",question:"What was that number?",
      reference:"11.50 euros per bike per month, for the theft tracker the insurer will accept.",must:["11[.,]50|eleven (?:euros? )?fifty"],later:["thirty.nine|\\b39\\b","waived"]},
    {id:"that-number-lag",kind:"lag",delivery:"ios",fast:true,after:"The tracker they'll accept is eleven fifty per bike per month. Eleven euros fifty.",question:"What was that number?",
      reference:"The transcript has not caught up to that yet. The latest it has is the deposit discussion (75 euros, refundable); ask again in a moment.",must:["transcript|caught up|transcribed|behind|not (?:yet )?(?:reached|arrived|available)|a moment|shortly"],later:["11[.,]50|eleven (?:euros? )?fifty","tracker"]},
    {id:"just-said-mid",kind:"recency",fast:true,after:"I'm going to try to get the first month waived but I wouldn't count on it.",question:"What did they just say?",
      reference:"Jonas will confirm with the insurer by Friday, and will try to get the first month waived but does not expect to. The insurer wants a tracker on every bike at 11.50 euros per bike per month and needs an answer by Friday.",must:["Friday","waive|first month|tracker"],later:["thirty.nine|\\b39\\b","student"],forbid:["android","permit"]},
    {id:"marketing-not-yet",kind:"not-yet",fast:true,after:"I'm going to try to get the first month waived but I wouldn't count on it.",question:"Has the marketing budget come up yet?",
      reference:"Not yet. So far the meeting has covered the fleet, the battery recall and the new launch date, the app, stations, repairs, the deposit and insurance.",must:[ABSTAIN+"|not yet|hasn't|has not"],later:["eighteen thousand|18,?000","six thousand five hundred|6,?500","campus"]},
    {id:"so-far",kind:"so-far",after:"I'm going to try to get the first month waived but I wouldn't count on it.",question:"Summarise where we are so far.",
      reference:"Fleet: 340 bikes, 300 on April 28 and 40 on May 12. The battery supplier is recalling 60 packs, so the launch moved from May 6 to May 20; Ravi is chasing the replacements. Android 14 unlock fails about one time in five; fix by April 30. Nine of 14 hubs are confirmed, the rest depend on a permit decision on May 8. Deposit is 75 euros. The insurer wants a tracker on every bike at 11.50 per bike per month; Jonas confirms by Friday.",must:["May 20|20 May|twentieth","tracker","recall|batter"],later:["soft launch","eighteen thousand|18,?000","student"]},
    {id:"tracker-follow-up",kind:"follow-up",fast:true,after:"I'm going to try to get the first month waived but I wouldn't count on it.",question:"And are we going ahead with that?",
      history:[{role:"user",content:"What's the tracker going to cost us?"},{role:"assistant",content:"The tracker the insurer accepts is 11.50 euros per bike per month, on all 340 bikes."}],
      reference:"Yes, in effect: Ottilie said there is no choice because the fleet cannot launch uninsured, and Jonas will confirm with the insurer by Friday. He will try to get the first month waived.",must:["Friday"],later:["student","thirty.nine|\\b39\\b"]},
    {id:"helmets-live",kind:"not-said",after:"Eight to eight, ten to four on weekends, chat only. Okay. Marketing.",question:"Did anyone mention helmets?",
      reference:"No, helmets have not come up so far.",must:[ABSTAIN+"|\\bno\\b"]},
    {id:"lease-partial-with-number",kind:"unfinished",fast:true,after:"Thursday's great. Um, what else. Oh, the depot.",partial:{fraction:0.8},question:"How much does the landlord want?",
      reference:"Two thousand one hundred fifty a month, going by what is being said right now; the sentence is still in progress.",must:["2,?150|two thousand one hundred fifty|twenty.one fifty"],later:["nineteen hundred|1,?900","May third|May 3"]},
    {id:"lease-partial-before-number",kind:"unfinished",fast:true,after:"Thursday's great. Um, what else. Oh, the depot.",partial:{fraction:0.48},question:"How much does the landlord want?",
      reference:"Not said yet. Jonas has just started on the depot lease and is mid-sentence (\"He wants…\"); the figure has not come through.",must:[ABSTAIN+"|still|mid.sentence|unfinished|cut off|in progress|\\byet\\b|hasn't"],later:["2,?150|two thousand one hundred|twenty.one fifty","nineteen hundred|1,?900"]},
    {id:"landlord-from-notes",kind:"notes",notes:"sparse",after:"I told him we'd answer by the third. May third.",question:"Who's the landlord again?",
      reference:"Verbeek, per your notes. He wants 2,150 a month (up from 1,900) and an answer by May 3.",must:["Verbeek"],later:["Zoeterwoude"]},
    {id:"miss-late",kind:"recency",recipe:true,fast:true,after:"Right. Yeah. That's a real problem. Okay, I'll still call.",question:"What did I miss?",
      reference:"- The depot landlord wants 2,150 a month, up from 1,900, with an answer due by May 3.\n- Ravi pointed out the Zoeterwoude unit has no charging.",must:["2,?150|twenty.one fifty|two thousand one hundred fifty|Zoeterwoude|charging|lease|depot"],later:["photo shoot","nineteenth"],forbid:["recall","android","student"]},
    {id:"catch-up-late",kind:"recency",after:"Right. Yeah. That's a real problem. Okay, I'll still call.",question:"I had to step out for a couple of minutes. Catch me up.",
      reference:"The depot lease: the landlord wants 2,150 a month, up from 1,900, and Jonas told him you would answer by May 3. You said you would call about the Zoeterwoude unit tomorrow before deciding; Ravi noted that unit has no charging. Just before that, Calla agreed to send you the invite email draft by Thursday.",must:["2,?150|twenty.one fifty|two thousand one hundred fifty","Zoeterwoude"],later:["photo shoot"],forbid:["recall","android"]},
    {id:"sound-smart",kind:"recency",recipe:"smart",after:"I'm going to try to get the first month waived but I wouldn't count on it.",question:"Make me sound smart",
      reference:"- I'd like to understand what the tracker requirement does to our cost per bike before we lock the subscription price.\n- If we can't get the first month waived, is there anything else we can ask the insurer for in return for putting trackers on the whole fleet?",must:["tracker|insur|waive|quote|Friday"],later:["student","thirty.nine|\\b39\\b","soft launch"]},
    {id:"suggest-questions",kind:"recency",recipe:"questions",after:"I'm going to try to get the first month waived but I wouldn't count on it.",question:"Suggest questions",
      reference:"- What does the tracker add to our monthly cost across all 340 bikes?\n- Is there a cheaper tracker the insurer would accept?\n- What happens to the launch date if the quote lapses on Friday?",must:["tracker|insur|waive|quote|Friday"],later:["student","thirty.nine|\\b39\\b","soft launch"]},
    {id:"notes-what-i-typed",kind:"notes",notes:"sparse",after:"I told him we'd answer by the third. May third.",question:"What have I typed about the depot so far?",
      reference:"Your notes say the landlord is Verbeek, with a possible counter at 2,000.",must:["Verbeek","2,?000"],later:["Zoeterwoude"]},
    {id:"notes-transcript-first",kind:"notes",notes:"sparse",fast:true,after:"I told him we'd answer by the third. May third.",question:"How much does the landlord want?",
      reference:"2,150 euros a month, up from 1,900. (The 2,000 in your notes is your own idea for a counter-offer, not what he asked for.)",must:["2,?150|twenty.one fifty|two thousand one hundred fifty"],later:["Zoeterwoude"],forbid:["(?:wants?|asking(?: for)?|asked for) (?:€ ?|EUR ?)?2,?000\\b"]},
    {id:"miss-late-with-notes",kind:"notes",recipe:true,notes:"sparse",after:"Right. Yeah. That's a real problem. Okay, I'll still call.",question:"What did I miss?",
      reference:"- The depot landlord wants 2,150 a month, up from 1,900, with an answer due by May 3.\n- Ravi pointed out the Zoeterwoude unit has no charging.",must:["2,?150|twenty.one fifty|two thousand one hundred fifty|Zoeterwoude|charging|lease|depot"],later:["photo shoot","nineteenth"],forbid:["Verbeek","CAC","recall","android"]},
    {id:"miss-nothing-delivered",kind:"lag",delivery:"desktop",delaySeconds:150,after:"Launch moves from May sixth to May twentieth. Ravi, you'll chase vol taro so the replacements actually ship Friday?",question:"What did I miss?",
      reference:"There is no transcript for this meeting yet, so there is nothing to catch up from. It will be available once the recording has been transcribed.",must:[ABSTAIN,"transcript|transcribed"],later:["May 20|twentieth","recall"]},
  ],
}
