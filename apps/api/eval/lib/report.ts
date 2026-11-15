/** Aggregation and the Markdown report. Pure functions over RunResult[]. */
import type {Check,Feature,RunResult} from "./types"
import {DIMENSIONS} from "./judge"
import {clock} from "./transcript"

export const FEATURES:ReadonlyArray<Feature>=["generation","note-chat","workspace-chat","live-chat"]
export const quantile=(values:ReadonlyArray<number>,q:number)=>{
  if(!values.length)return NaN
  const sorted=[...values].sort((a,b)=>a-b),position=(sorted.length-1)*q,low=Math.floor(position),high=Math.ceil(position)
  return sorted[low]!+(sorted[high]!-sorted[low]!)*(position-low)
}
export const median=(values:ReadonlyArray<number>)=>quantile(values,0.5)
export const mean=(values:ReadonlyArray<number>)=>values.length?values.reduce((sum,value)=>sum+value,0)/values.length:NaN
const fixed=(value:number,digits=1)=>Number.isFinite(value)?value.toFixed(digits):"–"
const seconds=(ms:number)=>Number.isFinite(ms)?`${(ms/1000).toFixed(1)}s`:"–"
const percent=(part:number,whole:number)=>whole?`${Math.round(part/whole*100)}%`:"–"
const spread=(values:ReadonlyArray<number>,format:(value:number)=>string)=>values.length?`${format(median(values))} (${format(quantile(values,0.25))}–${format(quantile(values,0.75))}, max ${format(Math.max(...values))})`:"–"
const hard=(checks:ReadonlyArray<Check>)=>checks.filter(check=>!check.soft)
export const passed=(run:RunResult)=>run.ok&&hard(run.checks).every(check=>check.pass)
const groupRate=(runs:ReadonlyArray<RunResult>,group:Check["group"],predicate:(check:Check)=>boolean=()=>true)=>{
  const checks=runs.flatMap(run=>run.checks.filter(check=>check.group===group&&predicate(check)))
  return {pass:checks.filter(check=>check.pass).length,total:checks.length}
}
const judgeMean=(runs:ReadonlyArray<RunResult>,dimension:string)=>mean(runs.flatMap(run=>{const score=run.judge?.scores[dimension]?.score;return typeof score==="number"?[score]:[]}))
export const judgeOverall=(run:RunResult)=>mean(DIMENSIONS.flatMap(dimension=>{const score=run.judge?.scores[dimension]?.score;return typeof score==="number"?[score]:[]}))
/** Lower is worse: hard failures first, then the judge's faithfulness and mean. */
export const badness=(run:RunResult)=>(run.ok?0:100)+hard(run.checks).filter(check=>!check.pass).length*10+(5-(run.judge?.scores.faithfulness?.score??4))*3+(5-(Number.isFinite(judgeOverall(run))?judgeOverall(run):4))

export interface Agreement {samples:number;judgementsPerSample:number;dimensions:Record<string,{meanAbsoluteDifference:number;exact:number;withinOne:number;pairs:number}>}
export interface Summary {feature:Feature;runs:number;scenarios:number;completed:number;passed:number;required:{pass:number;total:number};hallucination:{pass:number;total:number};format:{pass:number;total:number};
  citation:{present:{pass:number;total:number};resolve:{pass:number;total:number};support:{pass:number;total:number}};judge:Record<string,number>;judged:number;
  totalMs:number[];ttftMs:number[];inputTokens:number[];cachedTokens:number[];outputTokens:number[];costUsd:number[];toolCalls:Record<string,number[]>}
export function summarise(runs:ReadonlyArray<RunResult>,feature:Feature):Summary {
  const mine=runs.filter(run=>run.feature===feature),tools=new Set(mine.flatMap(run=>Object.keys(run.perf.toolCalls)))
  return {feature,runs:mine.length,scenarios:new Set(mine.map(run=>run.scenario)).size,completed:mine.filter(run=>run.ok).length,passed:mine.filter(passed).length,
    required:groupRate(mine,"required",check=>!check.soft),hallucination:groupRate(mine,"hallucination"),format:groupRate(mine,"format"),
    citation:{present:groupRate(mine,"citation",check=>check.id==="citation-present"),resolve:groupRate(mine,"citation",check=>check.id==="citations-resolve"),support:groupRate(mine,"citation",check=>check.id==="citations-support-claim")},
    judge:Object.fromEntries(DIMENSIONS.map(dimension=>[dimension,judgeMean(mine,dimension)])),judged:mine.filter(run=>run.judge&&!run.judge.error).length,
    totalMs:mine.filter(run=>run.ok).map(run=>run.perf.totalMs),ttftMs:mine.flatMap(run=>run.ok&&run.perf.ttftMs!==null?[run.perf.ttftMs]:[]),
    inputTokens:mine.map(run=>run.perf.inputTokens),cachedTokens:mine.map(run=>run.perf.cachedTokens??0),outputTokens:mine.map(run=>run.perf.outputTokens),costUsd:mine.map(run=>run.perf.costUsd),
    toolCalls:Object.fromEntries([...tools].map(tool=>[tool,mine.map(run=>run.perf.toolCalls[tool]??0)]))}
}
const table=(header:ReadonlyArray<string>,rows:ReadonlyArray<ReadonlyArray<string>>)=>[`| ${header.join(" | ")} |`,`| ${header.map(()=>"---").join(" | ")} |`,...rows.map(row=>`| ${row.map(cell=>cell.replace(/\|/g,"\\|").replace(/\n/g," ")).join(" | ")} |`)].join("\n")
const scores=(run:RunResult)=>run.judge&&!run.judge.error?DIMENSIONS.map(dimension=>run.judge!.scores[dimension]?.score??"–").join("/"):"–"
const quote=(text:string)=>text.split("\n").map(line=>`> ${line}`).join("\n")

export interface ReportMeta {startedAt:string;finishedAt:string;model:string;provider:string;judgeModel:string|null;judgePromptVersion:string;reps:number;fast:boolean;stub:boolean;only:string|null;features:Feature[];
  observedParameters:Record<string,unknown>;promptsSha256:string;fixturesSha256:string;gitCommit:string;gitDirty:boolean;concurrency:number;spendUsd:{underTest:number;judge:number;total:number};partial:string|null;
  fixtures:Array<{id:string;kind:string;capture:string;turns:number;segments:number;words:number;duration:string}>}

export function renderReport(meta:ReportMeta,runs:ReadonlyArray<RunResult>,agreement:Agreement|null):string {
  const out:string[]=[]
  const features=FEATURES.filter(feature=>runs.some(run=>run.feature===feature)),summaries=features.map(feature=>summarise(runs,feature))
  out.push(`# Evaluation report${meta.stub?" (STUB PROVIDER — mechanics only, not a quality measurement)":""}`,"",
    `- Run: ${meta.startedAt} → ${meta.finishedAt}${meta.partial?` — **partial: ${meta.partial}**`:""}`,
    `- Model under test: \`${meta.provider}:${meta.model}\`; request parameters observed on the wire: \`${JSON.stringify(meta.observedParameters)}\``,
    `- Judge: ${meta.judgeModel?`\`${meta.judgeModel}\`, rubric version ${meta.judgePromptVersion}, temperature 0`:"not run"}`,
    `- Scope: ${meta.features.join(", ")}${meta.fast?" (fast subset)":""}${meta.only?`, only \`${meta.only}\``:""}; ${meta.reps} repetition${meta.reps===1?"":"s"}; concurrency ${meta.concurrency}`,
    `- Code: commit \`${meta.gitCommit}\`${meta.gitDirty?" + uncommitted changes":""}; prompts (src/ai) sha256 \`${meta.promptsSha256.slice(0,12)}\`; fixtures sha256 \`${meta.fixturesSha256.slice(0,12)}\``,
    `- Spend: $${meta.spendUsd.underTest.toFixed(2)} model under test + $${meta.spendUsd.judge.toFixed(2)} judge = **$${meta.spendUsd.total.toFixed(2)}**`,"")
  out.push("## Summary","",table(["Feature","Runs","Completed","All hard checks pass","Required facts","Hallucination probes clean","Format","Judge: faith / cover / useful / concise / notes / tone"],
    summaries.map(s=>[s.feature,String(s.runs),percent(s.completed,s.runs),percent(s.passed,s.runs),`${percent(s.required.pass,s.required.total)} (${s.required.pass}/${s.required.total})`,`${percent(s.hallucination.pass,s.hallucination.total)} (${s.hallucination.pass}/${s.hallucination.total})`,percent(s.format.pass,s.format.total),s.judged?DIMENSIONS.map(d=>fixed(s.judge[d]!)).join(" / ")+` (n=${s.judged})`:"–"])),"",
    "Required facts: share of gold items (or required answer content) present. Hallucination probes: share of planted not-said claims and wrong-answer patterns that stayed absent. Judge scores are 1–5 means.","")
  out.push("## Latency, tokens and cost","",table(["Feature","Total time: median (p25–p75, max)","Time to first token","Input tokens / run","Read from prompt cache","Output tokens / run","Cost / run (median)","Cost (all runs)"],
    summaries.map(s=>[s.feature,spread(s.totalMs,seconds),s.ttftMs.length?spread(s.ttftMs,seconds):"not streamed",spread(s.inputTokens,v=>String(Math.round(v))),percent(s.cachedTokens.reduce((a,b)=>a+b,0),s.inputTokens.reduce((a,b)=>a+b,0)),spread(s.outputTokens,v=>String(Math.round(v))),`$${fixed(median(s.costUsd),4)}`,`$${fixed(s.costUsd.reduce((a,b)=>a+b,0),2)}`])),"",
    "Time to first token is measured from the start of the request, so for chat it includes retrieval and planning. Generation is not streamed by the API. Read from prompt cache: share of all input tokens the provider reported as served from its cache; scenarios are mostly single questions on different meetings, so this is low by construction and only the thread scenarios (`live-chat/*/thread-*`) and `eval/live-cache.ts` measure a conversation.","")
  if(agreement&&agreement.samples)out.push("## Judge noise","",`${agreement.samples} outputs were each judged ${agreement.judgementsPerSample} times with identical input.`,"",
    table(["Dimension","Mean absolute difference","Identical score","Within one point","Pairs"],Object.entries(agreement.dimensions).map(([dimension,value])=>[dimension,fixed(value.meanAbsoluteDifference,2),percent(value.exact,value.pairs),percent(value.withinOne,value.pairs),String(value.pairs)])),"",
    "Differences between configurations smaller than the mean absolute difference are within judge noise.","")
  for(const feature of features){
    const mine=runs.filter(run=>run.feature===feature),summary=summaries.find(s=>s.feature===feature)!
    out.push(`## ${feature}`,"")
    const variants=[...new Set(mine.map(run=>feature==="live-chat"?run.variant.split("/")[0]!+" / "+run.variant.split("/")[1]!:run.variant))]
    out.push(table(["Variant","Runs","All hard checks pass","Required","Hallucination clean","Judge faith / cover / useful","Median total"],variants.map(variant=>{
      const group=mine.filter(run=>(feature==="live-chat"?run.variant.split("/")[0]!+" / "+run.variant.split("/")[1]!:run.variant)===variant),req=groupRate(group,"required",check=>!check.soft),h=groupRate(group,"hallucination")
      return [variant,String(group.length),percent(group.filter(passed).length,group.length),req.total?`${percent(req.pass,req.total)} (${req.pass}/${req.total})`:"–",h.total?percent(h.pass,h.total):"–",["faithfulness","coverage","usefulness"].map(d=>fixed(judgeMean(group,d))).join(" / "),seconds(median(group.filter(run=>run.ok).map(run=>run.perf.totalMs)))]
    })),"")
    if(feature==="generation"){
      const rate=(group:ReadonlyArray<RunResult>,prefix:string)=>{const checks=group.flatMap(run=>run.checks.filter(check=>check.id.startsWith(prefix)));return checks.length?`${percent(checks.filter(c=>c.pass).length,checks.length)} (${checks.filter(c=>c.pass).length}/${checks.length})`:"–"}
      out.push("Coverage of gold annotations by typed-notes level:","",table(["Typed notes","Decisions","Action items","Owner attached","Key facts","Not-said probes clean","Typed lines kept","Judge notes fidelity"],["notes:none","notes:sparse","notes:detailed"].map(variant=>{
        const group=mine.filter(run=>run.variant===variant)
        return [variant.slice(6),rate(group,"decision:"),rate(group,"action:"),rate(group,"owner:"),rate(group,"fact:"),rate(group,"not-said:"),rate(group,"typed-lines-kept"),fixed(judgeMean(group,"notesFidelity"))]
      })),"")
    }
    if(feature==="workspace-chat"&&Object.keys(summary.toolCalls).length)out.push("Retrieval operations per request (planner tool calls plus fallback reads):","",table(["Tool","Median","Max","Share of runs using it"],Object.entries(summary.toolCalls).map(([tool,counts])=>[tool,fixed(median(counts),0),String(Math.max(...counts)),percent(counts.filter(count=>count>0).length,counts.length)])),"")
    if(feature==="live-chat"){
      const points=[...new Set(mine.map(run=>run.scenario.split("@")[0]!))]
      out.push("Answerability over time. Each cell: was the answer correct at that delay after the line was spoken (✓/✗), and had the line reached the server (`on server` / `not yet`).","",
        table(["Checkpoint","Delivery","Asked after","Line on server?","Answered with the fact","Leak-free","Segments on server"],points.flatMap(point=>mine.filter(run=>run.scenario.startsWith(point+"@")&&run.rep===1).map(run=>{
          const used=run.checks.find(check=>check.id==="uses-what-was-said"),delivered=run.checks.find(check=>check.id==="anchor-delivered"),leak=run.checks.find(check=>check.id==="nothing-from-later")
          const reps=mine.filter(other=>other.scenario===run.scenario),hits=reps.filter(other=>other.checks.find(check=>check.id==="uses-what-was-said")?.pass).length
          return [point.replace("live-chat/",""),run.input.delivery??"",run.variant.split("/")[2]??"",delivered?.pass?"on server":"not yet",reps.length>1?`${hits}/${reps.length}`:used?.pass?"✓":"✗",leak?(leak.pass?"✓":"✗ "+(leak.detail??"")):"–",`${run.context?.segmentsAvailable??"?"}/${run.context?.segmentsTotal??"?"}`]
        }))),"")
    }
    out.push("### Scenarios","",table(["Scenario","Variant","Pass","Failed hard checks","Soft flags","Judge "+DIMENSIONS.map(d=>d[0]).join("/"),"Total","TTFT","Tokens in/out","Cost"],
      [...new Set(mine.map(run=>run.scenario))].map(id=>{
        const group=mine.filter(run=>run.scenario===id),first=group[0]!,failures=new Map<string,number>(),soft=new Map<string,number>()
        for(const run of group){for(const check of hard(run.checks))if(!check.pass)failures.set(check.id,(failures.get(check.id)??0)+1);for(const check of run.checks)if(check.soft&&!check.pass&&check.id!=="anchor-delivered")soft.set(check.id,(soft.get(check.id)??0)+1)}
        const list=(map:Map<string,number>)=>[...map].map(([check,count])=>group.length>1?`${check} ×${count}`:check).join(", ")||"–"
        const judged=group.find(run=>run.judge&&!run.judge.error)
        return [id.replace(`${feature}/`,""),first.variant,`${group.filter(passed).length}/${group.length}`,list(failures),list(soft),judged?scores(judged):"–",seconds(median(group.map(run=>run.perf.totalMs))),group.some(run=>run.perf.ttftMs!==null)?seconds(median(group.flatMap(run=>run.perf.ttftMs===null?[]:[run.perf.ttftMs]))):"–",`${Math.round(median(group.map(run=>run.perf.inputTokens)))}/${Math.round(median(group.map(run=>run.perf.outputTokens)))}`,`$${fixed(median(group.map(run=>run.perf.costUsd)),4)}`]
      })),"")
    const worst=[...mine].sort((a,b)=>badness(b)-badness(a)).filter((run,index,all)=>all.findIndex(other=>other.scenario===run.scenario)===index).slice(0,feature==="generation"?3:4).filter(run=>badness(run)>1.5)
    if(worst.length)out.push("### Worst examples","")
    for(const run of worst){
      out.push(`#### ${run.scenario} (repetition ${run.rep})`,"")
      if(run.input.question)out.push(`**Question:** ${run.input.question}`,"")
      if(run.input.history?.length)out.push(`**Earlier turns:** ${run.input.history.map(turn=>`_${turn.role}:_ ${turn.content}`).join(" ")}`,"")
      if(run.input.atMs!==undefined)out.push(`**Asked at** ${clock(run.input.atMs)} into the recording, delivery \`${run.input.delivery}\`; server held ${run.context?.segmentsAvailable}/${run.context?.segmentsTotal} segments (up to ${clock(run.context?.deliveredUntilMs??0)}).`,"")
      if(run.input.reference)out.push(`**Reference answer:** ${run.input.reference}`,"")
      if(!run.ok)out.push(`**Failed:** ${run.error}`,"")
      const failures=run.checks.filter(check=>!check.pass&&check.id!=="anchor-delivered")
      if(failures.length)out.push("**Checks not passed:**","",...failures.map(check=>`- ${check.soft?"(soft) ":""}\`${check.id}\`${check.detail?`: ${check.detail}`:""}`),"")
      if(run.tools?.length)out.push(`**Retrieval:** ${run.tools.filter(tool=>tool.status!=="running").map(tool=>`${tool.name} → ${tool.label}`).join("; ")}`,"")
      if(run.judge&&!run.judge.error)out.push(`**Judge (${scores(run)}):** ${run.judge.overall}`,"",...DIMENSIONS.filter(d=>(run.judge!.scores[d]?.score??5)<=3).map(d=>`- ${d} ${run.judge!.scores[d]!.score}: ${run.judge!.scores[d]!.reason}`),...(run.judge.unsupportedClaims.length?["- unsupported claims: "+run.judge.unsupportedClaims.map(claim=>`“${claim}”`).join("; ")]:[]),"")
      out.push("**Output:**","",quote(run.output||"(empty)"),"")
    }
  }
  out.push("## Fixtures","",table(["Fixture","Kind","Capture","Duration","Words","Turns","Stored segments"],meta.fixtures.map(f=>[f.id,f.kind,f.capture,f.duration,String(f.words),String(f.turns),String(f.segments)])),"")
  return out.join("\n")
}
export function agreementOf(samples:ReadonlyArray<ReadonlyArray<Record<string,number|null>>>):Agreement {
  const dimensions:Agreement["dimensions"]={}
  for(const dimension of DIMENSIONS){
    let total=0,exact=0,within=0,pairs=0
    for(const sample of samples)for(let i=0;i<sample.length;i++)for(let j=i+1;j<sample.length;j++){
      const a=sample[i]![dimension],b=sample[j]![dimension]
      if(typeof a!=="number"||typeof b!=="number")continue
      pairs++;total+=Math.abs(a-b);if(a===b)exact++;if(Math.abs(a-b)<=1)within++
    }
    if(pairs)dimensions[dimension]={meanAbsoluteDifference:total/pairs,exact,withinOne:within,pairs}
  }
  return {samples:samples.length,judgementsPerSample:samples[0]?.length??0,dimensions}
}
