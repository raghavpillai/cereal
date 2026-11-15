/** The hand-written skeleton of the two long fixtures: for every agenda item, the
 * lines that had to be spoken word for word, by whom, in order, and what the
 * surrounding conversation was allowed to contain. The gold annotations in
 * `long.ts` and `held-long.ts` quote these lines. The dialogue around them in
 * `*.script.ts` was drafted by a language model (anthropic/claude-sonnet-5.5,
 * one request per agenda item) under the rule that it add no other decision,
 * commitment, owner, deadline, date, price, amount or statistic; a part was
 * accepted only when every line below appeared verbatim and in order.
 * `eval/generation.test.ts` re-checks that against the stored scripts. */
export interface Item {topic:string;who:string[];keys:Array<[string,string]>;notes?:string;words?:[number,number]}
export interface Spec {id:string;premise:string;cast:Record<string,string>;items:Item[]}

export const longA:Spec={id:"hard-long",
  premise:"Tidewater Outfitters, a fictional outdoor clothing retailer with stores and an online shop, holds its annual operating plan review on a video call in January. Corinne chairs. It is a long working session among people who know each other well.",
  cast:{Corinne:"chief operating officer, chairs the meeting",Raj:"head of finance",Mateus:"head of e-commerce and the mobile app",Ottilie:"head of retail stores",Yevgenia:"head of supply chain",Dominic:"head of marketing",Wren:"head of customer support"},
  items:[
    {topic:"Opening and a recap of the fourth quarter results",who:["Corinne","Raj","Mateus","Ottilie"],keys:[
      ["Raj","Fourth quarter revenue closed at twenty three point four million, against a plan of twenty two point one."],
      ["Raj","Gross margin was forty one point two percent, down from forty three last year, and almost all of that is freight."],
      ["Corinne","So we beat the top line and gave it back on freight. That's the theme for today."]],
      notes:"People react to the numbers, ask what drove the online and store split in general terms, and joke a little. No decisions here."},
    {topic:"The revenue target for the coming fiscal year",who:["Raj","Mateus","Ottilie","Corinne","Dominic"],keys:[
      ["Raj","My recommendation is we plan the year at ninety six million."],
      ["Mateus","I think a hundred is reachable if the app launches on time."],
      ["Corinne","We plan at ninety six. A hundred is a stretch goal, it does not go in the budget."]],
      notes:"A real debate between caution and ambition before Corinne settles it. Ottilie sides with Raj. Nobody takes a task."},
    {topic:"Renewing the freight contract with the carrier Halcyon Freight",who:["Yevgenia","Corinne","Raj","Ottilie"],keys:[
      ["Yevgenia","Halcyon offered a two year renewal at four percent below our current rate."],
      ["Corinne","Okay, let's take the two year deal with Halcyon."],
      ["Yevgenia","I'll get the paperwork started this week."]],
      notes:"The group likes the discount and agrees fairly quickly. Raj says he has not read the draft contract yet and will look at it during the break, but does not object. Do not mention fuel surcharges at all in this part."},
    {topic:"The mobile app launch date",who:["Mateus","Corinne","Dominic","Wren"],keys:[
      ["Mateus","The payments certification slipped, so the March tenth launch is not realistic anymore."],
      ["Mateus","I'm proposing April fourteenth for the public launch, with a beta for two thousand loyalty members on March seventeenth."],
      ["Corinne","April fourteenth it is. Mateus, send me the revised launch plan by Friday."],
      ["Mateus","Will do, Friday."]],
      notes:"Dominic worries about the spring campaign timing; Wren asks about support readiness in general terms. No other dates."},
    {topic:"New store openings in Bend and Missoula",who:["Ottilie","Corinne","Raj"],keys:[
      ["Ottilie","Bend is on track to open May second, and the build out is coming in at six hundred forty thousand dollars."],
      ["Ottilie","Missoula is not signed. The landlord wants a ten year lease and we want seven."],
      ["Corinne","I'm not signing ten years. Ottilie, go back with seven years and a break option at year five."],
      ["Ottilie","I'll take that back to the landlord and report at the next review."]],
      notes:"Missoula stays undecided. Raj talks about lease risk in general terms. Nobody commits to an opening date for Missoula."},
    {topic:"The returns policy and return rates",who:["Wren","Raj","Corinne","Mateus"],keys:[
      ["Wren","Our return rate last year was fourteen point eight percent, and footwear alone was twenty two."],
      ["Raj","I'd like to cut the window from sixty days to thirty."],
      ["Wren","Thirty will hurt us with the loyalty members. I could support forty five."],
      ["Corinne","Forty five days, starting February first. That's decided."],
      ["Wren","On refusing worn footwear, I want legal to look at it before we commit. I'll ask them this week."]],
      notes:"Refusing returns of worn footwear is floated by Raj and left open pending legal. It is not decided."},
    {topic:"A tangent about the company all-hands location and parking at the Portland office",who:["Dominic","Wren","Ottilie","Corinne","Mateus"],keys:[
      ["Dominic","Completely off topic, but is the all hands still in the Portland office, because the parking there is a disaster."],
      ["Corinne","Let's not spend review time on parking. Back to the agenda."]],
      notes:"Light, funny, a few anecdotes. Absolutely no decisions, tasks, dates or numbers come out of it.",words:[450,600]},
    {topic:"The marketing budget for the year",who:["Dominic","Raj","Corinne","Mateus"],keys:[
      ["Dominic","I'm asking for five point two million for the year."],
      ["Raj","I can fund four point six."],
      ["Corinne","We'll do four point nine, with three hundred thousand of that held back until the app passes one hundred fifty thousand installs."],
      ["Dominic","I can work with that."],
      ["Dominic","I'll send the channel breakdown at four point nine by next Wednesday."]],
      notes:"Dominic argues for brand spend, Raj for efficiency. No other figures besides the ones given."},
    {topic:"Supplier concentration risk in outerwear",who:["Yevgenia","Raj","Corinne","Ottilie"],keys:[
      ["Yevgenia","Sixty two percent of our outerwear comes from one factory in Da Nang."],
      ["Yevgenia","I want to qualify a second supplier in Porto by September, starting with a sample order of four thousand units."],
      ["Raj","What's the cost premium?"],
      ["Yevgenia","About nine percent per unit on the Porto samples."],
      ["Corinne","Approved. Do the sample order."]],
      notes:"Discussion of why concentration is risky, quality concerns with a new supplier in general terms. No other numbers."},
    {topic:"Customer support response times and hiring",who:["Wren","Corinne","Raj","Mateus"],keys:[
      ["Wren","Average first response time is nineteen hours, and our target is eight."],
      ["Wren","I need six more agents."],
      ["Corinne","Four now. We look at the other two in June, after the app launch."],
      ["Wren","I'll open the four requisitions on Monday."]],
      notes:"Mateus mentions the app could reduce or increase contacts, nobody knows. No other numbers or dates."},
    {topic:"Coming back to the Halcyon freight contract after the break, because Raj read the draft",who:["Raj","Yevgenia","Corinne","Ottilie"],keys:[
      ["Raj","I read the Halcyon draft during the break. The four percent discount comes with an uncapped fuel surcharge."],
      ["Yevgenia","That's new. That wasn't in the summary they sent me."],
      ["Raj","On last year's fuel prices the surcharge would have wiped out the whole discount."],
      ["Corinne","Then I'm reversing what I said earlier. We do not sign two years. One year only, and only if the fuel surcharge is capped."],
      ["Yevgenia","Understood. I'll go back to Halcyon and ask for a one year term with a surcharge cap, and I'll get a comparison quote from Brightline Cargo by the twentieth."]],
      notes:"This overturns the earlier decision to take the two year deal. Yevgenia says she will stop the paperwork. The mood is slightly sheepish. No other numbers."},
    {topic:"The loyalty program and the idea of a paid tier",who:["Mateus","Dominic","Corinne","Wren","Raj"],keys:[
      ["Mateus","We have four hundred ten thousand loyalty members and about a third bought twice or more last year."],
      ["Dominic","I'd like to test a paid tier at forty nine dollars a year."],
      ["Corinne","I'm not deciding a paid tier today. Bring me a test design."],
      ["Mateus","Dominic and I will bring a test design to the February review."]],
      notes:"Views differ on whether members would pay. The paid tier is explicitly not decided. No other prices."},
    {topic:"The inventory system upgrade",who:["Raj","Yevgenia","Corinne","Ottilie"],keys:[
      ["Raj","The upgrade is one point eight million. Sorry, one point three. One point eight was with the warehouse module, which we cut."],
      ["Yevgenia","Go live is planned for July sixth, and we will not cut over in August because of back to school."],
      ["Raj","I'll circulate the revised business case by the end of January."]],
      notes:"Ottilie asks what stores will notice in general terms. After Raj's correction, others refer to the cost only as one point three. No other dates or numbers."},
    {topic:"Wrap-up",who:["Corinne","Raj","Wren","Dominic"],keys:[
      ["Corinne","Next review is Thursday, February nineteenth."],
      ["Corinne","I'll send the decisions list tonight."]],
      notes:"Corinne thanks people and closes. Do NOT recap or restate any of the decisions, numbers or tasks from earlier parts. Keep it brief small talk and goodbyes.",words:[150,250]},
  ]}

export const longB:Spec={id:"held-long",
  premise:"Pelican Point Aquarium, a fictional public aquarium, is building a new ocean wing. The project steering group meets in person around a table in February. Imogen chairs. It is a long working session among colleagues who know each other well, recorded on a phone on the table.",
  cast:{Imogen:"project director, chairs the meeting",Tavish:"construction manager",Paloma:"head of animal care and exhibits",Henrik:"finance director",Adaeze:"head of fundraising",Luca:"head of operations and visitor experience"},
  items:[
    {topic:"Opening and overall project status",who:["Imogen","Tavish","Henrik","Luca"],keys:[
      ["Tavish","We're at fifty eight percent complete overall."],
      ["Henrik","Total budget is thirty one and a half million, and we've spent seventeen point nine so far."]],
      notes:"General status colour: weather, the site, morale. The planned public opening date of June twelfth is mentioned as the current plan. No decisions."},
    {topic:"The main tank acrylic viewing panel is delayed",who:["Tavish","Imogen","Henrik","Paloma"],keys:[
      ["Tavish","The main acrylic panel is five weeks late. It now arrives April twenty seventh instead of March twenty third."],
      ["Tavish","We can still hit June twelfth if we run double shifts for six weeks, and that costs about one hundred eighty thousand dollars."],
      ["Imogen","Then we hold June twelfth and pay for the double shifts."]],
      notes:"Paloma starts to raise a concern about the animals but says she needs to check her schedule first and will come back to it later. Do not discuss curing time or quarantine in this part."},
    {topic:"Life support system commissioning",who:["Paloma","Tavish","Imogen"],keys:[
      ["Paloma","The main tank holds four hundred thousand gallons, and the filtration loop passed its pressure test last week."],
      ["Tavish","The ozone unit failed inspection. The replacement is ordered and it's three weeks out."],
      ["Imogen","Tavish, send me the reinspection date as soon as you have it."],
      ["Tavish","Yes, I'll have it by Wednesday."]],
      notes:"Technical but conversational. No other numbers or dates."},
    {topic:"The capital fundraising campaign and a conditional pledge",who:["Adaeze","Imogen","Henrik","Luca"],keys:[
      ["Adaeze","We've raised seven point three five million against the nine million goal."],
      ["Adaeze","The Hartwell Foundation has offered five hundred thousand dollars, but they want naming rights to the touch pool."],
      ["Imogen","We can't accept naming rights in this room. That goes to the board."],
      ["Adaeze","I'll put it on the agenda for the March fifth board meeting."]],
      notes:"The pledge is not accepted. Opinions differ on naming rights. No other amounts."},
    {topic:"Ticket prices when the new wing opens",who:["Luca","Henrik","Imogen","Adaeze"],keys:[
      ["Luca","I'm proposing adult admission goes from thirty two dollars to thirty eight when the wing opens."],
      ["Henrik","The model works better at forty."],
      ["Imogen","Thirty eight. And children stay at twenty two. I don't want a headline about forty dollar tickets."]],
      notes:"A real back and forth before Imogen settles it. No other prices."},
    {topic:"A tangent about the public contest to name the new octopus",who:["Luca","Paloma","Adaeze","Imogen","Tavish"],keys:[
      ["Luca","Slightly off topic, but the octopus naming contest has over three thousand entries and half of them are jokes."],
      ["Imogen","That's fun, but not for this meeting."]],
      notes:"Light and funny, a few silly name suggestions. Absolutely no decisions, tasks or dates come out of it.",words:[450,600]},
    {topic:"Staffing the new wing",who:["Luca","Paloma","Imogen","Henrik"],keys:[
      ["Luca","We need fourteen new staff for the wing, six aquarists and eight in guest services."],
      ["Paloma","I'll post the six aquarist roles this week."],
      ["Luca","Somebody has to write the guest services training plan, and it isn't in anyone's job right now."],
      ["Imogen","Let's come back to that."]],
      notes:"Nobody volunteers for the training plan and the group never comes back to it; it stays without an owner. No other numbers or dates."},
    {topic:"Construction change orders and the contingency budget",who:["Henrik","Tavish","Imogen","Luca"],keys:[
      ["Henrik","We started with two point four million in contingency and one point five is left."],
      ["Tavish","The café ventilation change order is two hundred twenty thousand."],
      ["Imogen","Approve the ventilation. The lobby flooring upgrade at ninety five thousand is a no."]],
      notes:"Luca argues briefly for the nicer lobby floor and loses. No other amounts."},
    {topic:"Paloma comes back with the animal schedule: curing, water and quarantine",who:["Paloma","Imogen","Tavish","Henrik"],keys:[
      ["Paloma","After the panel goes in it has to cure for three weeks before we can fill the tank."],
      ["Paloma","Then the water has to stabilise, and the sharks need thirty days in the tank before the public sees them."],
      ["Paloma","If the panel arrives April twenty seventh, the earliest I can have animals on exhibit is July sixth. Double shifts don't change that. Fish don't work shifts."]],
      notes:"The room realises the June date does not work. Do not state the new opening date yet; end with people absorbing the problem."},
    {topic:"Reversing the opening date decision",who:["Imogen","Henrik","Adaeze","Tavish","Luca"],keys:[
      ["Imogen","So June twelfth was never possible, with or without double shifts."],
      ["Imogen","I'm reversing the earlier call. No double shifts. We move the opening to Friday, July seventeenth."],
      ["Henrik","That saves the one hundred eighty thousand, but we lose about five weeks of summer admissions, which I'd estimate at six hundred thousand dollars."],
      ["Henrik","Sorry, six hundred thousand was the whole wing for the summer. Five weeks is closer to two hundred fifty thousand."],
      ["Adaeze","I need to tell the donors before they read it somewhere. I'll call the top ten donors by Friday."],
      ["Tavish","I'll cancel the double shift request with the contractor today."]],
      notes:"This overturns the earlier decision to hold June twelfth. After Henrik's correction, others refer only to two hundred fifty thousand. No other numbers or dates."},
    {topic:"Announcing the new date and the member preview",who:["Luca","Adaeze","Imogen"],keys:[
      ["Luca","We announce the new date publicly on March ninth, not before."],
      ["Luca","Member preview days will be July fourteenth through sixteenth."],
      ["Luca","I'll redo the launch calendar around July seventeenth and bring it next week."]],
      notes:"Discussion of how to word the announcement in general terms. No other dates."},
    {topic:"School programs for the new wing",who:["Luca","Paloma","Imogen","Adaeze"],keys:[
      ["Luca","Two hundred twelve schools are on the waitlist for the new wing."],
      ["Imogen","School bookings open August third, capped at one hundred twenty students a day."],
      ["Adaeze","The state transport grant is sixty thousand dollars and the application is due April first. I'll write it."]],
      notes:"Paloma worries about noise near the quarantine area in general terms. No other numbers or dates."},
    {topic:"Wrap-up",who:["Imogen","Tavish","Paloma","Henrik"],keys:[
      ["Imogen","Next steering group is Tuesday the tenth."],
      ["Imogen","I'll send the revised milestone list tomorrow."]],
      notes:"Imogen thanks people and closes. Do NOT recap or restate any of the decisions, numbers or tasks from earlier parts. Keep it brief small talk and goodbyes.",words:[150,250]},
  ]}
