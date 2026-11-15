import type {Fixture} from "../lib/types"
import {near} from "../lib/transcript"

/** Long quarterly planning meeting (about 61 minutes including a 12-minute break). A decision
 * is reversed late on, and several facts sit in the middle of the meeting.
 * Fictional company (Pinecrest), product (Clinic Desk), vendors and people. */
export const planning:Fixture={
  id:"planning",title:"Clinic Desk Q3 planning",kind:"long planning meeting",daysAgo:20,capture:"desktop",wpm:100,
  people:{Rosalind:"mic",Arjun:"system",Beatriz:"system",Hamid:"system",Signe:"system",Tobias:"system"},
  attendees:[{name:"Rosalind Achterberg",email:"rosalind@pinecrest.example",self:true},{name:"Arjun Nair",email:"arjun@pinecrest.example"},{name:"Beatriz Mota",email:"beatriz@pinecrest.example"},{name:"Hamid Sorour",email:"hamid@pinecrest.example"},{name:"Signe Dahl",email:"signe@pinecrest.example"},{name:"Tobias Krenn",email:"tobias@pinecrest.example"}],
  script:`
Rosalind: Can everyone hear me? Give me a moment, I'm just getting the agenda up.
[+50]
Rosalind: Okay, I think we have everyone except Tobias, who told me he'll join after his budget call. Let's start. We have ninety minutes booked and I'd like to use about seventy. The agenda is, one, a short look back at the second quarter, two, the four candidates for the third quarter, three, the evidence from support, data and design, four, capacity, and then we decide. We'll take a break in the middle. The output of this meeting is a committed list for Q3, a list of what we are explicitly not doing, and owners for the next steps. Arjun, can you start with what we shipped?
Arjun: Sure. Three things were on the Q2 plan. The waitlist shipped on time, May twelfth, and it's been quiet since, which I take as a good sign. SMS reminders shipped three weeks late. The plan said June second, we shipped June twenty third.
Rosalind: Remind everyone why.
Arjun: Carrier registration. The mobile carriers require every sender to register the message templates, and that approval took nineteen days. The code was done on time. We just hadn't put the registration on the plan because none of us knew it existed.
Rosalind: So an external dependency that wasn't on the plan.
Arjun: Yes. And the third thing, the calendar sync rewrite, we cut the scope in half. We did the one way sync and left two way for later. I still think that was the right call.
Rosalind: Agreed. While we're on it, how good were our estimates last quarter overall?
Arjun: Not great. Across the three items we estimated forty weeks of work and spent fifty two. So we were about thirty percent over.
Rosalind: Thirty percent over. Was that one item or all of them?
Arjun: Mostly calendar sync. The waitlist came in almost exactly on estimate. SMS was on estimate for the code and late for the registration. Calendar sync was the one where we found out halfway through that the old data was messier than we thought.
Rosalind: So the lesson there is about work that touches old data.
Arjun: Yes, and I'll come back to that when we talk about multi location, because it has the same shape.
Rosalind: Okay. Signe, what did support see?
Signe: Ticket volume went down. One thousand one hundred eighty tickets in Q2, against one thousand four hundred twenty in Q1. So about seventeen percent fewer. Most of the drop is the waitlist, clinics used to email us to manage waitlists by hand.
Rosalind: That's a real result. Hamid, the health numbers.
Hamid: Net promoter score went from forty one to forty six. Monthly logo churn is two point three percent, it was two point six the quarter before. And we ended the quarter with six hundred twelve active clinics.
Signe: Can I add one thing on the support side, since it affects the quarter ahead? I lost one person in June and I haven't replaced them. We're five people on support now, down from six. Volume fell, so we coped, but if any of these launches goes badly I don't have slack.
Rosalind: Is the replacement approved?
Signe: Approved, and I have two candidates in final interviews next week.
Rosalind: Good. Tell me if that slips, because a launch in late August with a support team of five is a risk I'd want to know about.
Signe: I will.
Rosalind: Six hundred twelve. Okay. The lesson I'm writing down from Q2 is the carrier registration, I want every item this quarter to list its external dependencies before we commit to a date. Now, the candidates. There are four. One, insurance eligibility checks, so the front desk can see whether a patient's cover is active before the appointment. Two, multi location scheduling, for clinic groups that run more than one site. Three, patient self reschedule, a link in the reminder that lets the patient move their own appointment. Four, a revamp of reporting. We cannot do all four. I'd like evidence first and opinions second. Signe.
Signe: From the ticket data, the biggest single driver of tickets is rescheduling. Thirty one percent of all tickets last quarter were a clinic asking how to move, swap or bulk move appointments, or a patient who got a reminder and couldn't change it. Double booking bugs are twelve percent. Reporting complaints are six percent.
Rosalind: Thirty one, twelve, six. And eligibility?
Signe: Almost no tickets, because we don't have the feature, so nobody writes in about it. I hear it on calls instead. I can't give you a number for that.
Rosalind: Fair. Hamid?
Hamid: Three numbers. Eighteen percent of all appointments get rescheduled at least once. The overall no show rate is seven percent. For clinics that have turned on SMS reminders, the no show rate is four and a half percent. So reminders work, and a reminder with a reschedule link should work better, because some of the no shows are people who couldn't be bothered to phone.
Rosalind: Do you have an estimate for what self reschedule does to tickets?
Hamid: An assumption, not a measurement. If patients can move their own appointments, I'd expect roughly forty percent of the reschedule tickets to go away. I want to be clear that forty percent is my guess from looking at the ticket text, it is not a forecast I'd defend to finance.
Rosalind: Understood, we'll label it as an assumption. Beatriz, the research.
Beatriz: We spoke to eleven clinics in May and June. Eight of the eleven asked for self reschedule without being prompted. That's the strongest signal I've seen in two years of doing these interviews. On multi location, only three of the eleven run more than one site.
Arjun: Three of eleven doesn't sound like a priority.
Beatriz: It doesn't, until you look at who the three are. They're our largest customers. And what they need is not really a calendar feature. It's permissions. Today a role belongs to one clinic. A regional manager who covers four sites has four logins. Any multi location work starts with redesigning roles and the data model underneath, and that's invisible to users for a while.
Rosalind: So multi location has a phase one that is mostly plumbing.
Beatriz: Yes. Permissions and data model first, then the cross site calendar on top. I'd propose we never plan it as one item.
Signe: Can I add something here, because it's relevant and it's uncomfortable. Meadowbrook, the clinic group, fourteen locations. They are our largest customer, ninety six thousand in annual revenue. Their renewal is in October. Their operations director told my team two weeks ago that if multi location isn't, her words, visibly underway by the renewal, they will go to tender.
Rosalind: Visibly underway. Not shipped.
Signe: Not shipped. She was specific. She wants to see that it's real, she doesn't expect it finished.
Arjun: That changes how I think about phase one. If it's all plumbing there is nothing to show her.
Beatriz: I can make it visible. If we do the permissions design properly I can put a clickable prototype of the regional manager view in front of her in September. It wouldn't be working software, but it would be our actual design.
Rosalind: Hold that thought, I want to come back to it when we decide. On reporting, does anyone have evidence beyond the six percent?
Hamid: Only that the people who complain about reporting are the same accounts every time. It's about twenty clinics. It's loud, it isn't wide.
Rosalind: Okay. Before the break I'd like ten minutes on what self reschedule actually is, because the estimate depends on it and I've heard three different versions in the corridor. Beatriz, what's the proposal?
Beatriz: Let me share my screen, one second.
[+40]
Beatriz: Okay. The patient gets the normal reminder, and it has a link. The link opens a page with no login. They see their appointment and a list of other free slots. They pick one, they confirm, done. The clinic sees the change on the calendar with a small marker that says the patient moved it.
Arjun: No login worries me. Anyone who gets hold of that message can move someone's appointment.
Beatriz: Agreed, so the link is signed and tied to that one appointment, and before the page shows anything it asks for the patient's date of birth. It's the same check the front desk does on the phone.
Arjun: Signed link plus date of birth. I can live with that. How long does the link stay valid?
Beatriz: Seven days, or until the appointment, whichever is sooner.
Signe: What stops a patient moving an appointment at the last minute? Clinics will hate it if someone moves a nine o'clock at eight fifty.
Beatriz: A cut off. By default patients can reschedule up to twenty four hours before the appointment. Clinics can change that to anything between two hours and seventy two hours.
Signe: Twenty four as the default is right. Most of our clinics charge a late fee inside twenty four hours anyway.
Hamid: And how many times can they move the same appointment? In the data there's a small group of patients who reschedule four or five times.
Beatriz: Twice. After two self reschedules the link says please call the clinic.
Rosalind: And which slots do they see?
Beatriz: Same appointment type, same practitioner, by default. A clinic can switch on any practitioner, but it's off unless they ask.
Arjun: That default matters for the estimate. Same practitioner is simple. Any practitioner means we have to check qualifications for the appointment type, and that's another three weeks.
Rosalind: Then any practitioner is a setting we build only if the nine weeks holds. Now the question I know Signe wants to ask.
Signe: Cancelling. If they can move it, can they cancel it?
Beatriz: Not in my proposal. I left cancel out on purpose.
Signe: Clinics will ask for it on day one.
Beatriz: Some will. But the clinics I interviewed were split. Half of them want cancellations to come by phone, because a phone call is their chance to rebook the patient. A cancel button makes it too easy to disappear.
Hamid: The data supports that a little. When a patient cancels by phone, about sixty percent rebook on the same call.
Rosalind: Then cancel is out for this quarter. Self reschedule only, no self cancel. Signe, I know that's not what you wanted.
Signe: It's fine. I'd rather ship reschedule in August than both in October. I'll write the help article so it explains why.
Rosalind: Thank you. So to summarise the rules. A signed link with a date of birth check, valid seven days. A twenty four hour cut off by default, adjustable from two to seventy two hours. A maximum of two self reschedules per appointment. Same type and same practitioner by default. No cancel.
Arjun: With those rules, nine weeks stands.
Rosalind: Good. And since I said every item lists its external dependencies, what are they for this one?
Arjun: I was hoping you wouldn't ask, because the answer is the same one that bit us last quarter. The reminder text changes. It gets a link and a new sentence. A changed message is a new template, and a new template has to be registered with the carriers again.
Rosalind: The nineteen day thing.
Arjun: The nineteen day thing. It could be faster the second time, but I'm not going to plan on that. If we want to ship on time, the new template has to be submitted in July, well before the code is finished.
Beatriz: That means the wording has to be final before the design is. I can do that, I just need to know the date.
Arjun: If I submit by July fourteenth, nineteen days takes us to the start of August, and that leaves room for one rejection.
Rosalind: Then that's a date. Beatriz, final reminder wording to Arjun by July tenth. Arjun, template submitted to the carriers by July fourteenth.
Beatriz: July tenth, fine.
Arjun: July fourteenth. I'll put it at the top of the staffing plan so nobody forgets it exists this time.
Rosalind: Are there any other outside dependencies for self reschedule?
Arjun: No. Everything else is ours.
Rosalind: Okay. Let's take ten minutes. When we come back, Arjun has capacity, and hopefully we have Tobias.
[+720]
Rosalind: Welcome back. Tobias isn't on yet, let's keep going. Arjun, capacity.
Arjun: Nine engineers. Two of them, Ilse and Renzo, start parental leave on August fourth and are out for the rest of the quarter. After leave, public holidays, and the twenty percent we always reserve for maintenance and support escalations, I have seventy eight engineer weeks for the quarter.
Rosalind: Seventy eight.
Arjun: And I don't want to plan all seventy eight. After what happened with SMS I want a twenty five percent buffer. So I'd plan fifty eight and keep twenty in reserve.
Rosalind: Fifty eight plannable. Now the estimates.
Arjun: Self reschedule, nine engineer weeks. It's small because it reuses the reminder links. Reporting revamp, eighteen. Eligibility, if we do it properly for every clinic, thirty. A pilot version, for a handful of clinics with one insurance vendor, fourteen. Multi location in full, forty four. Phase one only, permissions and data model, twenty two.
Rosalind: Let me do the arithmetic out loud. Self reschedule nine, eligibility pilot fourteen, multi location phase one twenty two. That's forty five. Add reporting at eighteen and we're at sixty three, which is over fifty eight.
Arjun: Right. Three of the four fit, in their smaller versions. All four don't.
Hamid: One constraint from my side before you sequence anything. The data warehouse migration has a freeze from August eleventh to August fifteenth. No schema changes that week, at all. If the permissions data model needs new tables, they land before the eleventh or after the fifteenth.
Arjun: Good to know. That's the same month I lose two people. I'll plan the schema work for July.
Rosalind: Noted, freeze August eleventh to fifteenth. Now eligibility, because it's the one with money attached. Arjun, what do we know about the vendor?
Arjun: We looked at two. Claimwell charges eleven cents per check, with a minimum of two thousand five hundred dollars a month, on a twelve month contract. They have a sandbox and decent documentation. The other one, Verita, is cheaper, eight cents a check and no minimum, but they have no sandbox and their own estimate for integration is six weeks longer.
Rosalind: What volume would a pilot generate?
Hamid: A typical clinic runs about three thousand appointments a month. Five pilot clinics is fifteen thousand checks a month. At eleven cents that's one thousand six hundred fifty dollars.
Arjun: Which is under the minimum. So we'd pay the two thousand five hundred regardless.
Rosalind: So seventy five hundred dollars for the quarter, and a twelve month commitment, which is thirty thousand for the year.
Arjun: Yes.
Rosalind: I don't know if I can sign for that. Tobias isn't here, and I don't know what's left in the vendor budget after the SMS overrun. I don't want to commit engineers to a pilot and then find we can't pay the vendor.
Beatriz: Could we do the design work for eligibility this quarter and build next quarter?
Rosalind: That's where I'm leaning. If we can't confirm the money today, eligibility moves to Q4, and we put the fourteen weeks into reporting instead. Self reschedule nine, multi location phase one twenty two, reporting eighteen, that's forty nine, which fits.
Signe: I'd be sad about that. Eligibility is the thing prospects ask about on every sales call I sit in on.
Rosalind: I know. I'd be sad too. But I won't plan on money I can't see. Let's pencil it that way, eligibility to Q4, reporting in, and I'll confirm with Tobias afterwards.
Arjun: Pencilled.
[+45]
Tobias: Hi, sorry, I'm here. The budget call ran over. Where are we?
Rosalind: Perfect timing. Tobias, we were about to defer the insurance eligibility pilot to Q4 because nobody here knows whether we can afford the vendor. The numbers are two thousand five hundred dollars a month minimum, so seventy five hundred for the quarter, on a twelve month contract. What's left in the budget for new vendors?
Tobias: For Q3 the ceiling for new vendor spend is thirty thousand dollars. Nothing has been drawn against it yet. Seventy five hundred is fine, you didn't need to wait for me for that.
Rosalind: Oh. Good. That's better than I assumed.
Tobias: The part I don't like is the twelve months. I'm not signing a twelve month minimum for something you're calling a pilot. If the pilot fails in October we'd be paying for it until next summer. I'll sign it if there is a ninety day termination clause. Without that, no.
Arjun: They offered us a six month term when I pushed, I think there's room.
Tobias: Then let me do the negotiating. Send me the contact. I want a ninety day exit in writing.
Rosalind: So the money is there, on the condition of a ninety day exit clause. Then I want to reverse what we pencilled ten minutes ago. Eligibility pilot is in for Q3. Reporting goes to Q4. Does anyone object?
Signe: No objection. Relief.
Hamid: No objection. The reporting accounts will complain, I'll warn customer success.
Arjun: No objection, as long as it's a pilot. Five clinics, one vendor.
Rosalind: Five clinics, one vendor, Claimwell, conditional on the exit clause. If Tobias can't get the clause by the end of July, we fall back to Verita or we move it to Q4, and I'll make that call then.
Tobias: That's reasonable.
Rosalind: While we're on the pilot, let's agree how we choose the five clinics and how we judge it, so we're not arguing about it in October. Hamid?
Hamid: Three criteria from the data. One, at least sixty percent of the clinic's appointments are insured, otherwise there's nothing to check. Two, they work with at least two of the five largest insurers, so we test more than one payer. Three, they are not in the group of clinics we're moving during the warehouse migration, because I don't want two changes at once.
Signe: I'd add a fourth, and it's not a data one. I want at least one clinic that complains. Alder Street Dental will tell us everything that's wrong within a day. A pilot of five friendly clinics teaches us nothing.
Rosalind: I like that. One of the five is a known critic. Alder Street if they agree.
Arjun: And how do we know the pilot worked?
Hamid: I'd propose two measures. Ninety percent of checks return an answer in under five seconds, because the front desk won't wait longer than that. And the front desk uses it for at least half of insured appointments by the fourth week. If they don't use it, speed doesn't matter.
Tobias: I'd like a date when we decide whether to continue, since I'm negotiating an exit clause around it.
Rosalind: The pilot goes live September fifteenth. Let's review it on October twentieth, five weeks in. That's inside your ninety days.
Tobias: October twentieth. Good, I'll make sure the notice period lines up with that.
Rosalind: And the dependencies for eligibility, other than the contract?
Arjun: One. We'd be sending patient insurance details to a vendor, so legal has to review their data processing agreement before we send a single real record. Last time a review like that took about two weeks.
Tobias: I'll send the agreement to legal together with the contract, so the two run at the same time. They don't have to be one after the other.
Rosalind: Good. So the contract and the data processing review both sit with Tobias, both by the end of July.
Tobias: Yes.
Beatriz: Can I ask for something while Tobias is here? The permissions redesign is the largest piece of design work we've done, and I'm the only designer on it. I'd like a contract designer for six weeks.
Tobias: What would that cost?
Beatriz: Roughly eighteen thousand dollars.
Tobias: Not this quarter. The thirty thousand is for vendors that the product depends on, and I've just agreed to spend some of it. I can look at it again for Q4.
Beatriz: Then I need to say clearly that September fifth is tight with one designer.
Rosalind: It's noted, and I'd rather hear it now. If the design review on the eighteenth shows it's not achievable, we move the prototype date, we don't cut the review.
Beatriz: Okay. That's fair.
Rosalind: Now multi location. I said I'd come back to it. Given Meadowbrook, I don't want phase one to be a stretch goal. I want it committed, and I want Beatriz's prototype in front of Meadowbrook's operations director in September, before the renewal conversation starts.
Beatriz: I can have the clickable prototype ready by September fifth, if the permissions design review happens in July.
Arjun: Then phase one is committed at twenty two weeks. To be explicit, that is permissions and data model only. No cross site calendar this quarter. I don't want anyone promising Meadowbrook a calendar.
Signe: I'll make sure the account team says exactly that. Design in September, calendar next year.
Rosalind: Not next year. We haven't planned Q4 yet. Say after phase one, and nothing more specific.
Signe: After phase one. Got it.
Hamid: Before you read it back, can I raise the reporting accounts? We're about to tell twenty loud clinics that they wait another quarter.
Rosalind: Go on.
Hamid: Most of what they ask for is not a revamp. They want the appointment data in a spreadsheet every week so they can make their own charts. A scheduled export would quiet most of them. I think it's about two engineer weeks.
Arjun: I don't have two weeks. If it comes out of the reserve it isn't a reserve any more.
Hamid: Then not from your team. My analysts can build it straight from the warehouse. It wouldn't be in the product, it would be a file that lands in their inbox on Monday morning.
Arjun: If it never touches the application code, I have no objection.
Rosalind: Then that's a data team side project, not part of the engineering plan and not counted in the fifty eight. Hamid, can you have the weekly export running by the end of August?
Hamid: End of August, yes, for the twenty accounts that asked.
Rosalind: And it doesn't get announced as a feature. Customer success offers it to those accounts by hand.
Signe: Understood.
Rosalind: One more piece of detail I want on the record for multi location, because twenty two weeks is our biggest item. Arjun, Beatriz, what is actually in phase one?
Beatriz: Three roles instead of one. Clinic staff, clinic manager, and a new regional manager who can see several clinics. And a principle for the migration, which is that nobody gains or loses access on the day we switch. Every existing user keeps exactly what they can see today.
Arjun: Underneath that, a new level in the data model. Today a clinic is the top of the tree. We add an organization above it, and a group like Meadowbrook becomes one organization with fourteen clinics. About nine of the twenty two weeks is migration tooling, moving existing accounts over safely. Meadowbrook alone has two hundred twelve staff accounts.
Rosalind: Nine of twenty two on migration. That's the part I'd have underestimated.
Arjun: Everyone does.
Rosalind: Then let me read the decision back. Committed for Q3. One, patient self reschedule, nine weeks. Two, the insurance eligibility pilot, five clinics, Claimwell, fourteen weeks, conditional on a ninety day exit clause. Three, multi location phase one, permissions and data model, twenty two weeks, plus a design prototype for Meadowbrook by September fifth. That's forty five of fifty eight plannable weeks. Explicitly not doing in Q3. The reporting revamp, which moves to Q4. Full eligibility for all clinics. The cross site calendar. And two way calendar sync stays where it is. Arjun, when can self reschedule ship?
Arjun: If I start it first, while I still have everyone, end of August. Call it August twenty ninth.
Rosalind: August twenty ninth for self reschedule. And the eligibility pilot starts when?
Arjun: Assuming the contract is signed by the end of July, the pilot clinics go live September fifteenth.
Rosalind: Two more things before risks. How we measure the quarter, and how we tell people. Hamid, measures.
Hamid: For self reschedule, I'd call it a success if a quarter of all reschedules are done by patients themselves by the end of September. Twenty five percent. And I'd like to see reschedule tickets down by a third, which is a bit more cautious than my forty percent guess.
Rosalind: Twenty five percent of reschedules by patients, tickets down by a third. And no shows?
Hamid: Under four percent for clinics that have reminders and reschedule both switched on. They're at four and a half today.
Rosalind: Good. Communication. I'll present the plan to the company at the all hands on July ninth. The customer roadmap update goes out on July sixteenth. Signe, can you draft the customer note?
Signe: Yes. What can I say about eligibility?
Rosalind: That we are running a pilot with a small number of clinics. No date for general availability, and the word pilot every time. And I want sales told clearly that eligibility is not something they can sell as available.
Signe: I'll put that in the internal version in bold.
Tobias: Please do. I've seen a pilot become a contractual commitment before.
Rosalind: Good. Risks. I have three and I'd like to hear more. One, the SMS vendor contract renews on September thirtieth.
Tobias: And they've already told us to expect an increase, around fifteen percent. I'll open that negotiation in August. I'd rather not have self reschedule drive our message volume up right before I negotiate, but I accept that I can't stop that.
Hamid: It will go up. Every reschedule sends a confirmation.
Tobias: Then give me a volume forecast and I'll negotiate on a tier instead of a unit price.
Hamid: I can do that.
Rosalind: Two, the compliance audit. It's the week of August eighteenth. Arjun, what does it cost you?
Arjun: Me and one engineer for the whole week. It's in the maintenance reserve already, so it doesn't come out of the fifty eight.
Rosalind: Three, hiring. We have one backend engineer approved.
Arjun: Approved, not hired. Even if we made an offer tomorrow, the earliest start is September. I have not counted that person anywhere in these numbers and I'd ask that nobody else does either.
Rosalind: Agreed. Any other risks?
Beatriz: One. The permissions redesign touches every screen that shows a staff name. If the design review slips past July, September fifth slips with it. I need the review on the calendar this week.
Rosalind: Then let's put it there. Owners and dates. Arjun, a staffing plan for the three items, who is on what, accounting for the leave.
Arjun: By July eleventh.
Rosalind: Beatriz, the permissions design review.
Beatriz: July eighteenth. And the Meadowbrook prototype, September fifth.
Rosalind: Hamid, two things. A baseline for reschedule tickets, so we can tell in October whether your forty percent was right. And the SMS volume forecast for Tobias.
Hamid: Baseline dashboard by July fifteenth. Volume forecast by the end of July, Tobias doesn't need it before August.
Rosalind: Signe, also two. Brief the Meadowbrook account team with the exact wording we agreed, and choose the five pilot clinics for eligibility.
Signe: I'll brief the account team this week. For the five clinics I'll need Hamid to tell me who has the right insurance mix. Let's say the list by July twenty fifth.
Hamid: Fine.
Rosalind: Tobias, the Claimwell contract with the ninety day exit, and the SMS renegotiation.
Tobias: Claimwell by July thirty first. SMS, I'll start in August and it has to be done before September thirtieth.
Rosalind: And mine. I'll write up the Q3 plan, including the not doing list and every external dependency, and send it to all of you by Friday. Our next checkpoint is July twenty second, same group, thirty minutes. Is there anything we haven't said that someone will regret on July twenty second?
Arjun: Only that fifty eight is the plan and seventy eight is not a secret second plan. The reserve is for surprises.
Rosalind: Heard. Thank you, everyone. Good meeting.
`,
  notes:{
    sparse:`Q3 planning\ncandidates: eligibility / multi-location / self-reschedule / reporting\ncapacity 78 wks, plan 58\nMeadowbrook - renewal Oct!!\neligibility - $$? ask Tobias\nmy worry: Arjun's team is stretched in August (leave + audit + freeze)`,
    detailed:`# Q2 retro\n- waitlist on time (May 12)\n- SMS reminders 3 wks late - carrier registration\n- tickets 1,180 vs 1,420\n- NPS 41 -> 46, churn 2.3%\n- lesson: list external dependencies\n\n# Candidates\n- insurance eligibility\n- multi-location\n- patient self-reschedule\n- reporting revamp\n\n# Evidence\n- reschedule = 31% of tickets\n- 8/11 clinics asked for self-reschedule\n- Meadowbrook (14 sites, 96k) wants multi-location "visibly underway" by Oct renewal\n\n# Capacity\n- 78 eng-weeks, plan 58 (25% buffer)\n- estimates: self-resched 9 / elig pilot 14 / ML phase 1 22 / reporting 18\n\n# Decision\n- IN: self-reschedule, eligibility pilot (5 clinics), multi-location phase 1\n- OUT: reporting -> Q4\n\n# Risks\n- SMS contract Sept 30\n- audit wk of Aug 18\n- my worry: August is thin for Arjun (leave + audit + freeze)\n\n# Next\n- plan doc by Fri (me)\n- checkpoint July 22`,
  },
  gold:{
    decisions:[
      {id:"commit-self-reschedule",text:"Patient self-reschedule is committed for Q3 (9 engineer-weeks), targeted to ship August 29.",must:["self.?reschedul","August (?:29|twenty.ninth)|Aug(?:ust)? 29|end of August"]},
      {id:"commit-eligibility",text:"The insurance eligibility pilot is committed for Q3: five clinics, Claimwell, 14 engineer-weeks, conditional on a 90-day exit clause. (An earlier lean to defer it to Q4 was reversed when Tobias confirmed the budget.)",must:["eligibility","five clinics|5 clinics|pilot","ninety.day|90.day"]},
      {id:"commit-multilocation",text:"Multi-location phase one (permissions and data model only, 22 engineer-weeks) is committed, with a design prototype for Meadowbrook by September 5.",must:["multi.?location","permissions|phase (?:one|1)"]},
      {id:"reporting-deferred",text:"The reporting revamp is deferred to Q4.",must:["reporting","Q4|defer|next quarter|fourth quarter"]},
      {id:"no-calendar",text:"No cross-site calendar in Q3; Meadowbrook is to be told it comes after phase one, with nothing more specific.",must:["cross.?site calendar|calendar"]},
      {id:"fallback",text:"If the exit clause is not obtained by the end of July, fall back to Verita or move the pilot to Q4.",must:["Verita|fall ?back"]},
      {id:"no-self-cancel",text:"Self-reschedule ships without self-cancel this quarter.",must:["cancel"]},
      {id:"reschedule-rules",text:"Self-reschedule rules: signed link with date-of-birth check valid seven days; 24-hour cut-off by default (clinics can set 2 to 72 hours); at most two self-reschedules per appointment; same appointment type and practitioner by default.",must:["24.hour|twenty.four","two (?:self.?)?reschedules|twice|maximum of two|max(?:imum)? (?:of )?2"]},
      {id:"pilot-review",text:"The eligibility pilot is reviewed on October 20; one of the five clinics should be a known critic (Alder Street Dental if they agree).",must:["October (?:20|twentieth)|Oct(?:ober)? 20"]},
      {id:"csv-export",text:"A weekly data export for the roughly twenty reporting accounts is a data-team side project by the end of August, outside the engineering plan and not announced as a feature.",must:["export"]},
    ],
    actionItems:[
      {id:"staffing",owner:"Arjun",due:"July 11",text:"Arjun produces a staffing plan for the three items by July 11.",must:["staffing","July (?:11|eleventh)|Jul 11"],ownerMust:near("Arjun","staffing")},
      {id:"design-review",owner:"Beatriz",due:"July 18",text:"Beatriz holds the permissions design review on July 18.",must:["design review","July (?:18|eighteenth)|Jul 18"],ownerMust:near("Beatriz","design review|permissions")},
      {id:"prototype",owner:"Beatriz",due:"September 5",text:"Beatriz delivers the clickable Meadowbrook prototype by September 5.",must:["prototype","September (?:5|fifth)|Sept?\\.? 5"],ownerMust:near("Beatriz","prototype")},
      {id:"baseline",owner:"Hamid",due:"July 15",text:"Hamid builds a baseline dashboard for reschedule tickets by July 15.",must:["baseline","July (?:15|fifteenth)|Jul 15"],ownerMust:near("Hamid","baseline")},
      {id:"sms-forecast",owner:"Hamid",due:"end of July",text:"Hamid gives Tobias an SMS volume forecast by the end of July.",must:["volume forecast|SMS (?:volume|forecast)"],ownerMust:near("Hamid","forecast")},
      {id:"brief-account",owner:"Signe",due:"this week",text:"Signe briefs the Meadowbrook account team with the agreed wording this week.",must:["account team|Meadowbrook","brief"],ownerMust:near("Signe","brief|account team")},
      {id:"pilot-clinics",owner:"Signe",due:"July 25",text:"Signe (with Hamid) chooses the five pilot clinics by July 25.",must:["pilot clinics|five clinics|5 clinics","July (?:25|twenty.fifth)|Jul 25"],ownerMust:near("Signe","clinics")},
      {id:"claimwell",owner:"Tobias",due:"July 31",text:"Tobias negotiates the Claimwell contract with a 90-day exit clause by July 31.",must:["Claimwell","July (?:31|thirty.first)|Jul 31|end of July"],ownerMust:near("Tobias","Claimwell|contract|exit")},
      {id:"sms-renegotiate",owner:"Tobias",due:"before September 30",text:"Tobias renegotiates the SMS vendor contract before it renews on September 30.",must:["SMS","September (?:30|thirtieth)|Sept?\\.? 30"],ownerMust:near("Tobias","SMS")},
      {id:"weekly-export",owner:"Hamid",due:"end of August",text:"Hamid's analysts deliver the weekly export for the twenty reporting accounts by the end of August.",must:["export","August"],ownerMust:near("Hamid","export")},
      {id:"customer-note",owner:"Signe",due:"before July 16",text:"Signe drafts the customer roadmap note (going out July 16), describing eligibility only as a pilot.",must:["customer (?:note|roadmap|update)|roadmap (?:note|update)"],ownerMust:near("Signe","note|roadmap|help article")},
      {id:"all-hands",owner:"Rosalind",due:"July 9",text:"Rosalind presents the Q3 plan at the all-hands on July 9.",must:["all.hands","July (?:9|ninth)|Jul 9"],ownerMust:near("Rosalind","all.hands")},
      {id:"reminder-wording",owner:"Beatriz",due:"July 10",text:"Beatriz gives Arjun the final reminder wording by July 10.",must:["wording","July (?:10|tenth)|Jul 10"],ownerMust:near("Beatriz","wording")},
      {id:"template-registration",owner:"Arjun",due:"July 14",text:"Arjun submits the new SMS reminder template for carrier registration by July 14.",must:["template|carrier","July (?:14|fourteenth)|Jul 14"],ownerMust:near("Arjun","template|carrier|regist")},
      {id:"dpa-review",owner:"Tobias",due:"end of July",text:"Tobias sends Claimwell's data processing agreement to legal alongside the contract.",must:["data processing|legal"],ownerMust:near("Tobias","data processing|legal")},
      {id:"plan-doc",owner:"Rosalind",due:"Friday",text:"Rosalind writes up the Q3 plan, including the not-doing list and external dependencies, by Friday.",must:["plan","Friday"],ownerMust:near("Rosalind","plan|write.?up|Friday")},
    ],
    facts:[
      {id:"sms-late",text:"SMS reminders shipped three weeks late (June 23 instead of June 2) because carrier registration took nineteen days.",must:["three weeks|3 weeks","carrier"]},
      {id:"tickets",text:"Tickets fell to 1,180 in Q2 from 1,420 in Q1 (about 17%).",must:["1,?180","1,?420"]},
      {id:"nps",text:"NPS rose from 41 to 46; monthly logo churn is 2.3% (was 2.6%); 612 active clinics.",must:["46","2\\.3"]},
      {id:"ticket-drivers",text:"Rescheduling drives 31% of tickets; double-booking bugs 12%; reporting 6%.",must:["31 ?%|thirty.one percent|31 percent"]},
      {id:"research",text:"Eight of eleven clinics interviewed asked for self-reschedule unprompted; only three run multiple sites, but they are the largest customers.",must:["eight of (?:the )?eleven|8 of (?:the )?11|8/11"]},
      {id:"meadowbrook",text:"Meadowbrook (14 locations, $96,000 a year, renewal in October) will go to tender unless multi-location is visibly underway by renewal.",must:["Meadowbrook","October|renewal"]},
      {id:"capacity",text:"Capacity is 78 engineer-weeks; with a 25% buffer, 58 are plannable; the committed items total 45.",must:["78|seventy.eight","58|fifty.eight"]},
      {id:"freeze",text:"The data warehouse migration freezes schema changes from August 11 to August 15.",must:["August (?:11|eleventh)|Aug(?:ust)? 11","freeze|schema"]},
      {id:"leave",text:"Two engineers (Ilse and Renzo) start parental leave on August 4.",must:["parental leave|August (?:4|fourth)","two engineers|Ilse|Renzo|2 engineers"]},
      {id:"claimwell-price",text:"Claimwell charges 11 cents per check with a $2,500 monthly minimum on a twelve-month contract; the pilot would cost $7,500 for the quarter.",must:["2,?500|7,?500","Claimwell"]},
      {id:"vendor-budget",text:"The Q3 ceiling for new vendor spend is $30,000, with nothing drawn against it.",must:["30,?000|thirty thousand|30k"]},
      {id:"sms-renewal",text:"The SMS vendor contract renews on September 30 with an expected increase of around 15%.",must:["September (?:30|thirtieth)|Sept?\\.? 30","15 ?%|fifteen percent|15 percent"]},
      {id:"audit",text:"The compliance audit is the week of August 18 and takes Arjun and one engineer for the week.",must:["audit","August (?:18|eighteenth)|Aug(?:ust)? 18"]},
      {id:"pilot-criteria",text:"Pilot clinics need at least 60% insured appointments, at least two of the five largest insurers, and must be outside the warehouse-migration group; success means 90% of checks answered in under five seconds and use on at least half of insured appointments by week four.",must:["60 ?%|sixty percent|60 percent","five seconds|5 seconds"]},
      {id:"migration-share",text:"About nine of the 22 multi-location weeks are migration tooling; Meadowbrook alone has 212 staff accounts.",must:["212|two hundred twelve|nine of (?:the )?(?:22|twenty.two)|9 of (?:the )?22"]},
      {id:"success-measures",text:"Quarter measures: 25% of reschedules done by patients by the end of September, reschedule tickets down by a third, no-shows under 4% for clinics with reminders and reschedule on.",must:["25 ?%|twenty.five percent|a quarter of","under (?:four|4) ?(?:percent|%)|below (?:four|4) ?(?:percent|%)|<\\s?4 ?%"]},
      {id:"estimate-overrun",text:"Q2 work was estimated at 40 engineer-weeks and took 52, about 30% over, mostly from the calendar sync rewrite.",must:["30 ?%|thirty percent|30 percent|52|fifty.two"]},
      {id:"designer-declined",text:"Beatriz asked for a contract designer for six weeks (about $18,000); Tobias declined for this quarter and will revisit for Q4.",must:["contract designer|designer","18,?000|eighteen thousand|not this quarter|declin|Q4"]},
      {id:"support-headcount",text:"Support is down to five people from six; a replacement is approved with two candidates in final interviews.",must:["five (?:people|agents|staff)|down (?:to five|from six)|5 people|replacement"]},
      {id:"deflection-assumption",text:"Hamid's 40% ticket-deflection figure for self-reschedule is an assumption, not a forecast.",must:["40 ?%|forty percent|40 percent","assum|guess|estimate|not a (?:forecast|measurement)"]},
    ],
    notSaid:[
      {id:"designer-approved",claim:"The contract designer was approved.",forbid:["contract designer[^.\\n]{0,40}(?:was|is|has been) (?:approved|hired|funded)","(?:approved|agreed to (?:fund|hire))[^.\\n]{0,30}contract designer"]},
      {id:"self-cancel-in",claim:"Patients will be able to cancel appointments themselves this quarter.",forbid:["(?:self.?cancel|cancel(?:lation)? (?:button|link))[^.\\n]{0,40}(?:is|will be) (?:included|in scope|added|shipped)","patients (?:can|will be able to) cancel"]},
      {id:"eligibility-deferred",claim:"The eligibility pilot was deferred to Q4 (that was pencilled, then reversed).",forbid:["eligibility[^.\\n]{0,60}(?:is|was|has been) (?:deferred|moved|pushed) to (?:Q4|next quarter)(?![^.\\n]{0,160}(?:revers|then|but|until|initially|before|later))","(?:Decision|Decided|Outcome)[^.\\n]{0,40}eligibility[^.\\n]{0,40}Q4"]},
      {id:"reporting-in",claim:"The reporting revamp is in the Q3 commitment.",forbid:["(?:committed|commit|in scope|included)[^.\\n]{0,60}reporting revamp(?![^.\\n]{0,60}(?:not|defer|Q4|out))"]},
      {id:"calendar-promised",claim:"A cross-site calendar was promised for Q3 or next year.",forbid:["calendar[^.\\n]{0,40}(?:in Q3|this quarter|next year)(?![^.\\n]{0,80}(?:no|not|nothing|rather))"]},
      {id:"hire-counted",claim:"The new backend hire is counted in Q3 capacity.",forbid:["(?:new hire|backend engineer|backend hire)[^.\\n]{0,60}(?:adds|increases|brings)[^.\\n]{0,30}capacity"]},
      {id:"twelve-month-signed",claim:"A twelve-month Claimwell contract was signed or approved.",forbid:["(?:signed|approved|agreed to) (?:a |the )?(?:twelve|12).month"]},
      {id:"verita-chosen",claim:"Verita was chosen as the vendor.",forbid:["(?:chose|selected|going with|picked) Verita"]},
      {id:"deflection-fact",claim:"Self-reschedule will cut reschedule tickets by 40% (stated as fact).",forbid:["will (?:cut|reduce|deflect|eliminate)[^.\\n]{0,40}(?:40 ?%|forty percent)(?![^.\\n]{0,80}(?:assum|guess|estimate))"]},
    ],
  },
  questions:[
    {id:"committed",kind:"factual",fast:true,question:"What did we commit to for Q3 and what did we push out?",reference:"Committed: patient self-reschedule (9 weeks, ship August 29); the insurance eligibility pilot (five clinics, Claimwell, 14 weeks, conditional on a 90-day exit clause, live September 15); multi-location phase one (permissions and data model, 22 weeks) plus a Meadowbrook prototype by September 5. Not in Q3: the reporting revamp (Q4), full eligibility for all clinics, the cross-site calendar, two-way calendar sync.",must:["self.?reschedul","eligibility","multi.?location","reporting"],forbid:["eligibility[^.\\n]{0,60}(?:is|was) (?:deferred|moved|pushed) to Q4(?![^.\\n]{0,160}(?:revers|then|but|initially|before|later))"],support:["Then let me read the decision back"]},
    {id:"eligibility-reversal",kind:"synthesis",fast:true,question:"Did we defer the eligibility pilot or not? Walk me through what happened.",reference:"It was pencilled for Q4 while Tobias was absent, because Rosalind did not know whether the vendor budget could cover Claimwell's $2,500 monthly minimum. When Tobias joined he said the Q3 ceiling for new vendor spend is $30,000 with nothing drawn, so $7,500 is fine, but he will only sign with a 90-day termination clause. Rosalind then reversed the pencilled decision: the pilot is in for Q3 and reporting moves to Q4. If the clause is not secured by the end of July the fallback is Verita or Q4.",must:["Tobias","30,?000|thirty thousand|30k|budget","ninety.day|90.day","revers|in for Q3|committed|back in|is in"],support:["Then I want to reverse what we pencilled ten minutes ago"]},
    {id:"freeze-dates",kind:"citation",fast:true,question:"When is the data warehouse freeze and what does it block? Cite the transcript.",reference:"August 11 to August 15: no schema changes at all that week, so new tables for the permissions data model must land before the 11th or after the 15th. Arjun will plan the schema work for July.",must:["August (?:11|eleventh)|Aug(?:ust)? 11","15|fifteenth","schema"],support:["The data warehouse migration has a freeze from August eleventh to August fifteenth"]},
    {id:"meadowbrook",kind:"factual",question:"What exactly did Meadowbrook ask for, and what are we allowed to promise them?",reference:"Their operations director said that unless multi-location is 'visibly underway' by the October renewal they will go to tender; she does not expect it shipped. We can show a clickable design prototype of the regional manager view in September (ready by September 5) and say the cross-site calendar comes after phase one, with nothing more specific. Nobody is to promise a calendar this quarter or next year.",must:["visibly underway|go to tender|tender","prototype","after phase (?:one|1)|not (?:promis|commit)|no (?:cross.?site )?calendar|nothing more specific"],support:["visibly underway by the renewal, they will go to tender","Say after phase one, and nothing more specific"]},
    {id:"capacity-math",kind:"synthesis",question:"How much engineering capacity is left unplanned, and why didn't reporting fit?",reference:"Capacity is 78 engineer-weeks; with a 25% buffer 58 are plannable. The committed items total 45 (9 + 14 + 22), leaving 13 plannable weeks and 20 in reserve. Reporting is 18 weeks, which would take the total to 63, over the 58.",must:["78|seventy.eight","58|fifty.eight","45|forty.five","18|eighteen|63|sixty.three"],support:["That's forty five. Add reporting at eighteen and we're at sixty three, which is over fifty eight"]},
    {id:"reschedule-rules",kind:"citation",fast:true,question:"What rules did we agree for patient self-reschedule? Cite the transcript.",reference:"A signed link tied to one appointment with a date-of-birth check, valid seven days or until the appointment; a 24-hour cut-off by default that clinics can set between 2 and 72 hours; at most two self-reschedules per appointment; only slots of the same appointment type and practitioner by default; no self-cancel this quarter.",must:["date.of.birth|DOB","24|twenty.four","two|twice|\\b2\\b","cancel"],support:["So to summarise the rules"]},
    {id:"pilot-success",kind:"factual",question:"How will we judge whether the eligibility pilot worked, and when do we decide?",reference:"Two measures: 90% of checks return an answer in under five seconds, and the front desk uses it for at least half of insured appointments by the fourth week. The pilot goes live September 15 and is reviewed on October 20, inside the 90-day exit window.",must:["90 ?%|ninety percent|90 percent","five seconds|5 seconds","October (?:20|twentieth)|Oct(?:ober)? 20"],support:["Ninety percent of checks return an answer in under five seconds","Let's review it on October twentieth"]},
    {id:"carrier-dependency",kind:"factual",fast:true,question:"What external dependency does self-reschedule have, and what dates protect against it?",reference:"The reminder text changes, which makes it a new SMS template that must be registered with the carriers again; last time that took nineteen days. Beatriz gives Arjun the final wording by July 10 and Arjun submits the template by July 14, which reaches the start of August with room for one rejection.",must:["carrier|template|regist","July (?:14|fourteenth)|Jul 14","July (?:10|tenth)|Jul 10|nineteen|19"],support:["If I submit by July fourteenth, nineteen days takes us to the start of August"]},
    {id:"q4-commit",kind:"absent",fast:true,question:"What did we commit to delivering in Q4?",reference:"Nothing was committed for Q4; it has not been planned. The reporting revamp moves to Q4 and the eligibility pilot could fall back to Q4, but Rosalind said explicitly that Q4 has not been planned yet.",must:["not (?:been |yet )?(?:planned|committed|decided)|no (?:Q4 )?commitment|hasn't been planned|haven't planned|nothing (?:was|is) committed"],forbid:["committed to (?:deliver|ship)(?:ing)?[^.\\n]{0,60}in Q4"],support:["We haven't planned Q4 yet"]},
    {id:"pricing-change",kind:"absent",question:"What did we decide about raising our own prices for clinics?",reference:"Nothing. Pricing for clinics was not discussed. The only price topics were the Claimwell vendor cost and an expected SMS vendor increase of about 15%.",must:["not (?:discuss|mention|cover|address|decide|come up)|no (?:decision|discussion|mention)|wasn't|was not|didn't|did not|nothing"],forbid:["(?:raise|increase) (?:our |clinic )?prices by"]},
    {id:"follow-who-negotiates",kind:"follow-up",question:"Who is negotiating that, and by when?",history:[{role:"user",content:"What is the condition on the eligibility pilot?"},{role:"assistant",content:"The pilot is committed only if the Claimwell contract includes a 90-day termination clause; Tobias will not sign a twelve-month minimum for a pilot."}],reference:"Tobias, by July 31. Arjun will send him the Claimwell contact; they had already offered a six-month term. If there is no clause by the end of July, Rosalind decides between Verita and moving the pilot to Q4.",must:["Tobias","July (?:31|thirty.first)|Jul 31|end of July"],support:["Claimwell by July thirty first"]},
    {id:"risks-august",kind:"synthesis",question:"Everything that lands on engineering in August: list it with dates.",reference:"August 4: Ilse and Renzo start parental leave. August 11 to 15: data warehouse freeze, no schema changes. Week of August 18: compliance audit takes Arjun and one engineer. August 29: target ship date for self-reschedule. Tobias also opens the SMS renegotiation in August.",must:["August (?:4|fourth)|Aug(?:ust)? 4","August (?:11|eleventh)|Aug(?:ust)? 11","August (?:18|eighteenth)|Aug(?:ust)? 18","August (?:29|twenty.ninth)|Aug(?:ust)? 29"]},
  ],
}
