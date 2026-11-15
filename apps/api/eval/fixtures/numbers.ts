import type {Fixture} from "../lib/types"
import {near} from "../lib/transcript"

/** Quarterly business review where the numbers need care: spoken-number forms,
 * a self-correction, near-miss distractors (last quarter, proposed vs settled). */
export const numbers:Fixture={
  id:"numbers",title:"Northbeam Q3 business review",kind:"numbers that require care",daysAgo:15,capture:"desktop",wpm:140,fast:true,
  people:{Yusuf:"mic",Greta:"system",Anselm:"system",Carys:"system"},
  attendees:[{name:"Yusuf Demir",email:"yusuf@northbeam.example",self:true},{name:"Greta Holm",email:"greta@northbeam.example"},{name:"Anselm Reyes",email:"anselm@northbeam.example"},{name:"Carys Bevan",email:"carys@northbeam.example"}],
  script:`
Yusuf: Let's get into it. Anselm, take us through the quarter, and please go slowly on the numbers because I'm going to repeat them to the board.
Anselm: Sure. Third quarter revenue came in at four point two eight million. Sorry, four point eight two. Four point eight two million dollars, against a plan of five point one.
Yusuf: Four point eight two against five point one. So a miss of two hundred eighty thousand.
Anselm: Two hundred eighty thousand, which is five and a half percent under plan. For reference, the second quarter was four point six one, so we still grew quarter on quarter, about four and a half percent.
Yusuf: And annual recurring revenue?
Anselm: Nineteen point six million at quarter end.
Yusuf: Gross margin?
Anselm: Seventy one point four percent. That's down slightly from seventy two point one last quarter, almost all of it is the new hosting region.
Yusuf: Okay. Carys, retention.
Carys: Net revenue retention is one hundred eight percent. It was one hundred twelve a year ago, so that's the trend I'm worried about. We lost seven customers in the quarter. Five of them were small. Two were not. Halvorsen Dairy was one hundred eighty thousand in annual recurring revenue, and Pike and Rowe was one hundred thirty five thousand.
Yusuf: So those two alone are three hundred fifteen thousand.
Carys: Three hundred fifteen thousand, yes. Out of roughly three hundred sixty thousand of churned recurring revenue in total.
Yusuf: Why did they leave?
Carys: Halvorsen was acquired and the parent company standardised on something else, I don't think we could have saved that. Pike and Rowe was price. They told us in the spring the renewal was too high and we didn't act on it.
Yusuf: That one hurts. Greta, pipeline.
Greta: Total open pipeline is twelve point three million. Weighted, it's three point nine million. And I want to be honest that the twelve point three is inflated. I have forty one opportunities that have sat in the same stage for more than ninety days.
Yusuf: How much are those forty one worth?
Greta: About two point two million unweighted. I think most of it is dead.
Yusuf: Then clean it. I'd rather show the board ten million that is real.
Greta: I'll clean out everything older than ninety days by the end of next week.
Yusuf: Good. Customer acquisition cost payback?
Anselm: Seventeen months. Our target is fourteen.
Yusuf: Seventeen against fourteen. And headcount?
Anselm: One hundred forty eight today. The plan had us at one hundred sixty by year end.
Yusuf: We're not going to one hundred sixty with a revenue miss. Here's what I want to do, and tell me if I'm wrong. We freeze hiring at one hundred fifty two. That covers the four offers already signed. Nothing else until the first quarter.
Carys: I need to push back on one thing. Support is drowning. Response time went from four hours to nine. If we freeze completely I will lose people, and then retention gets worse, not better.
Yusuf: How many do you need?
Carys: Two. Two support roles.
Yusuf: Okay. Freeze at one hundred fifty two, with an exception for two support roles. So one hundred fifty four at most. Anselm, does that work in the model?
Anselm: Two support roles is about one hundred sixty thousand a year fully loaded. Yes, it works.
Yusuf: Then that's decided. Now pricing. We talked last month about an increase on renewals.
Greta: I proposed eight percent.
Yusuf: You did, and I've thought about it. After Pike and Rowe I'm not comfortable with eight. I want six percent, on renewals only, starting January first. Nothing changes mid contract for anybody.
Greta: Six is fine. I'd rather have six that sticks than eight that we discount away.
Yusuf: Which brings me to discounting. What's the cap today?
Greta: Reps can go to twenty five percent on their own.
Yusuf: Too high. I want the cap at twenty percent, and anything above fifteen needs your approval, Greta.
Greta: So fifteen without approval, fifteen to twenty with mine, nothing above twenty.
Yusuf: Exactly that.
Greta: The team will complain, but okay.
Yusuf: Carys, the accounts at risk for next quarter. How many and how much?
Carys: Five accounts, six hundred twenty thousand in recurring revenue between them. I'd like to run a proper save plan on each one, executive sponsor, the works.
Yusuf: Do it. I'll sponsor the largest myself. Which is it?
Carys: Orrin Logistics, two hundred ten thousand.
Yusuf: Put me on Orrin. Anselm, I need a reforecast for the fourth quarter that reflects all of this, the freeze, the six percent, and the pipeline after Greta cleans it.
Anselm: I can have the reforecast by next Wednesday, as long as Greta's clean pipeline numbers reach me by Monday.
Greta: Monday is before the end of next week. I can give you a first pass Monday and the final on Friday.
Anselm: A first pass is enough.
Yusuf: Good. To be clear for the notes, nobody is being let go. This is a hiring freeze, not a reduction. I don't want that rumour starting.
Carys: Thank you for saying that.
Yusuf: Okay. Thanks, everyone.
`,
  notes:{
    sparse:`Q3 rev 4.82 vs 5.1 plan\nNRR 108 (was 112)\nfreeze @152 +2 support\nrenewals +6% Jan 1\ndiscount cap 20`,
    detailed:`# Q3 results\n- revenue $4.82M vs $5.1M plan (-280k)\n- ARR 19.6M, GM 71.4%\n- CAC payback 17 mo (target 14)\n\n# Retention\n- NRR 108% (112% a year ago)\n- 7 logos lost; Halvorsen Dairy 180k, Pike & Rowe 135k\n- Pike & Rowe = price, we were warned in spring\n\n# Pipeline\n- 12.3M total / 3.9M weighted - inflated, 41 stale opps\n\n# Decisions\n- hiring freeze at 152, exception 2 support roles\n- renewals +6% from Jan 1 (Greta proposed 8)\n- discount cap 20%, >15% needs Greta\n\n# Actions\n- Greta clean pipeline\n- Carys save plans x5 (620k) - I sponsor Orrin\n- Anselm Q4 reforecast Wed`,
  },
  gold:{
    decisions:[
      {id:"freeze",text:"Hiring freeze at 152 until the first quarter, with an exception for two support roles (154 at most).",must:["152|one hundred fifty.two","two support|2 support"]},
      {id:"uplift",text:"Renewal price increase of six percent from January 1, on renewals only; nothing changes mid-contract.",must:["six percent|6 ?%|6 percent","January|Jan(?:uary)? 1"]},
      {id:"discount-cap",text:"Discount cap lowered from 25% to 20%; anything above 15% needs Greta's approval.",must:["20 ?%|twenty percent|20 percent","15 ?%|fifteen"]},
      {id:"clean-pipeline",text:"Stale pipeline (older than ninety days) will be cleaned before reporting to the board.",must:["ninety|90","pipeline"]},
    ],
    actionItems:[
      {id:"greta-clean",owner:"Greta",due:"first pass Monday, final by end of next week",text:"Greta removes opportunities older than ninety days from the pipeline: first pass Monday, final by the end of next week.",must:["pipeline","Monday|end of next week|Friday"],ownerMust:near("Greta","pipeline|clean")},
      {id:"anselm-reforecast",owner:"Anselm",due:"next Wednesday",text:"Anselm delivers a fourth-quarter reforecast by next Wednesday.",must:["reforecast|re-forecast|forecast","Wednesday"],ownerMust:near("Anselm","forecast")},
      {id:"carys-save",owner:"Carys",text:"Carys runs save plans on five at-risk accounts worth $620,000 in recurring revenue.",must:["save plan","620|six hundred twenty"],ownerMust:near("Carys","save plan|at.risk")},
      {id:"yusuf-sponsor",owner:"Yusuf",text:"Yusuf is executive sponsor for Orrin Logistics ($210,000).",must:["Orrin"],ownerMust:near("Yusuf","Orrin|sponsor")},
    ],
    facts:[
      {id:"revenue",text:"Third-quarter revenue was $4.82 million against a plan of $5.1 million: a miss of $280,000, 5.5% under plan.",must:["4\\.82","5\\.1"]},
      {id:"miss",text:"The miss was $280,000 (5.5%).",must:["280|5\\.5|five and a half"]},
      {id:"arr",text:"Annual recurring revenue is $19.6 million.",must:["19\\.6"]},
      {id:"margin",text:"Gross margin is 71.4%, down from 72.1%.",must:["71\\.4"]},
      {id:"nrr",text:"Net revenue retention is 108%, down from 112% a year ago.",must:["108","112"]},
      {id:"churn",text:"Seven customers churned, including Halvorsen Dairy ($180,000) and Pike & Rowe ($135,000), together $315,000.",must:["Halvorsen","Pike (?:and|&) Rowe","180","135"]},
      {id:"pipeline",text:"Open pipeline is $12.3 million ($3.9 million weighted); 41 opportunities worth about $2.2 million have been stale for over ninety days.",must:["12\\.3","3\\.9","41|forty.one"]},
      {id:"cac",text:"CAC payback is seventeen months against a target of fourteen.",must:["17|seventeen","14|fourteen"]},
      {id:"headcount",text:"Headcount is 148; the plan was 160 by year end.",must:["148"]},
      {id:"support",text:"Support response time went from four hours to nine.",must:["nine hours|9 hours|four (?:hours )?to nine|4 (?:hours )?to 9"]},
    ],
    notSaid:[
      {id:"wrong-revenue",claim:"Revenue was $4.28 million (a misstatement corrected immediately).",forbid:["(?:revenue|came in)[^.\\n]{0,40}4\\.28(?![^.\\n]{0,60}(?:correct|misspoke|sorry))"]},
      {id:"eight-percent",claim:"The renewal increase is eight percent.",forbid:["(?:increase|uplift|raise)[^.\\n]{0,30}(?:of|by|at|is|to) (?:eight|8) ?(?:percent|%)(?![^.\\n]{0,80}(?:propos|instead|rather|rejected|not ))"]},
      {id:"layoffs",claim:"There will be layoffs or a headcount reduction.",forbid:["(?:layoffs?|redundanc|reduction in force|headcount reduction|let(?:ting)? (?:people|staff|employees) go)(?![^.\\n]{0,60}(?:no|not))"]},
      {id:"mid-contract",claim:"Prices change mid-contract for existing customers.",forbid:["(?:all|existing) (?:customers|contracts)[^.\\n]{0,40}(?:immediately|mid.?contract)(?![^.\\n]{0,40}(?:no|not))"]},
      {id:"q2-as-q3",claim:"Third-quarter revenue was $4.61 million (that was the second quarter).",forbid:["(?:Q3|third.quarter) revenue[^.\\n]{0,30}4\\.61"]},
      {id:"cap-25",claim:"The discount cap is 25%.",forbid:["(?:cap|limit)[^.\\n]{0,20}(?:is|at|remains|stays|of) (?:25|twenty.five) ?(?:%|percent)"]},
    ],
  },
  questions:[
    {id:"revenue",kind:"factual",fast:true,question:"What was Q3 revenue and how far off plan were we?",reference:"$4.82 million against a plan of $5.1 million: a miss of $280,000, or 5.5%. (Anselm first said 4.28 and corrected himself.) It was still up about 4.5% on the second quarter's $4.61 million.",must:["4\\.82","5\\.1","280|5\\.5|five and a half"],forbid:["revenue (?:was|of|came in at) \\$?4\\.28","Q3 revenue[^.\\n]{0,30}4\\.61"],support:["Four point eight two million dollars, against a plan of five point one"]},
    {id:"headcount-math",kind:"synthesis",fast:true,question:"What is the maximum headcount we can reach before Q1, and how do you get there?",reference:"154. Headcount is 148 today; the freeze is at 152, which covers the four signed offers, plus an exception for two support roles.",must:["154|one hundred fifty.four","152","148|four (?:signed )?offers|two support|2 support"],support:["So one hundred fifty four at most"]},
    {id:"discount-rules",kind:"citation",question:"What are the new discount rules? Cite the transcript.",reference:"Reps can discount up to 15% on their own; 15% to 20% needs Greta's approval; nothing above 20%. The previous cap was 25%.",must:["15|fifteen","20|twenty","Greta|approval"],forbid:["(?:cap|limit)[^.\\n]{0,20}(?:is|at|remains|of) (?:25|twenty.five)"],support:["So fifteen without approval, fifteen to twenty with mine, nothing above twenty"]},
    {id:"churn-share",kind:"synthesis",question:"How much of the churned recurring revenue came from the two large accounts?",reference:"$315,000 of roughly $360,000: Halvorsen Dairy at $180,000 and Pike & Rowe at $135,000, which is about 87.5% of the total.",must:["315","360"],support:["Three hundred fifteen thousand, yes. Out of roughly three hundred sixty thousand"]},
    {id:"layoffs",kind:"absent",fast:true,question:"How many people are being laid off?",reference:"None. Yusuf said explicitly that nobody is being let go: it is a hiring freeze, not a reduction.",must:["no(?:ne|body| one| layoffs)|not (?:a reduction|being)|hiring freeze"],forbid:["\\d+ (?:people|employees|roles) (?:are|will be) (?:laid off|let go)"],support:["nobody is being let go"]},
    {id:"q4-guidance",kind:"absent",question:"What revenue number did we forecast for Q4?",reference:"No fourth-quarter number was given. Anselm will produce a reforecast by next Wednesday.",must:["no(?:t| number| figure| forecast)|wasn't|was not|didn't|did not|reforecast"],forbid:["Q4 (?:revenue|forecast)[^.\\n]{0,30}\\$?\\d"],support:["I can have the reforecast by next Wednesday"]},
    {id:"follow-why-six",kind:"follow-up",question:"Why not the higher figure?",history:[{role:"user",content:"What price increase did we settle on?"},{role:"assistant",content:"Six percent, on renewals only, starting January 1. Nothing changes mid-contract."}],reference:"Greta had proposed eight percent, but after losing Pike & Rowe on price Yusuf was not comfortable with eight. Greta agreed she would rather have six that sticks than eight that gets discounted away.",must:["eight|8","Pike"],support:["After Pike and Rowe I'm not comfortable with eight"]},
  ],
}
