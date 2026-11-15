/**
 * Generation-only comparison across runs, by scenario group and typed-notes level.
 *
 *   bun --no-env-file apps/api/eval/generation-report.ts <label> [<label> ...] [--rescore] [--examples=id,id]
 *
 * Groups: `shared` (the eight meetings every feature uses), `tuned` (hard cases
 * the generation prompt was tuned against) and `held-out` (hard cases only ever
 * run to report a number). Labels joined with `+` are pooled into one row set,
 * so a "before" can be the stored baseline plus a run of the old prompt on the
 * new fixtures (`notesgen-before-shared+notesgen-before-new`).
 * `--rescore` first re-applies the current checks to each directory (free).
 */
import {readFile} from "node:fs/promises"
import {existsSync} from "node:fs"
import {resolve} from "node:path"
import {rescore} from "./run"
import type {RunResult} from "./lib/types"

const root=resolve(import.meta.dir,"results"),args=process.argv.slice(2)
const flags=new Set(args.filter(arg=>arg.startsWith("--"))),labels=args.filter(arg=>!arg.startsWith("--"))
const directory=(label:string)=>existsSync(label)?resolve(label):resolve(root,label)
const load=async(label:string)=>{
  const runs:RunResult[]=[]
  for(const part of label.split("+")){
    if(flags.has("--rescore"))await rescore(directory(part),()=>{})
    const stored=JSON.parse(await readFile(resolve(directory(part),"results.json"),"utf8")) as {runs:RunResult[]}
    runs.push(...stored.runs.filter(run=>run.feature==="generation"))
  }
  return runs
}
const group=(run:RunResult)=>run.fixture.startsWith("hard-")?"tuned":run.fixture.startsWith("held-")?"held-out":"shared"
const rate=(runs:ReadonlyArray<RunResult>,prefix:string)=>{const checks=runs.flatMap(run=>run.checks.filter(check=>check.id.startsWith(prefix)));return {pass:checks.filter(check=>check.pass).length,total:checks.length}}
const show=(value:{pass:number;total:number})=>value.total?`${Math.round(value.pass/value.total*100)}% (${value.pass}/${value.total})`:"–"
const mean=(values:number[])=>values.length?values.reduce((a,b)=>a+b,0)/values.length:NaN
const median=(values:number[])=>{if(!values.length)return NaN;const sorted=[...values].sort((a,b)=>a-b);return sorted[Math.floor((sorted.length-1)/2)]!}
const fixed=(value:number,digits=1)=>Number.isFinite(value)?value.toFixed(digits):"–"
const judge=(runs:ReadonlyArray<RunResult>,dimension:string)=>mean(runs.flatMap(run=>{const score=run.judge?.scores[dimension]?.score;return typeof score==="number"?[score]:[]}))
const metrics=(run:RunResult)=>{const detail=run.checks.find(check=>check.id==="style:metrics")?.detail;return detail?JSON.parse(detail) as Record<string,number>:null}
const table=(header:string[],rows:string[][])=>[`| ${header.join(" | ")} |`,`| ${header.map(()=>"---").join(" | ")} |`,...rows.map(row=>`| ${row.join(" | ")} |`)].join("\n")
/** Owner outcome per present action item, from the soft owner checks. Tasks
 * that have an identifiable owner and tasks that do not are counted apart. */
export const ownership=(runs:ReadonlyArray<RunResult>)=>{
  const named=runs.flatMap(run=>run.checks.filter(check=>check.id.startsWith("owner-named:")))
  const unowned=named.filter(check=>check.detail?.includes("no identifiable owner")),owned=named.filter(check=>!check.detail?.includes("no identifiable owner"))
  const count=(checks:typeof named,verdict:string)=>checks.filter(check=>check.detail?.startsWith(verdict)).length
  return {owned:owned.length,correct:count(owned,"correct"),missing:count(owned,"missing"),wrong:count(owned,"wrong"),unowned:unowned.length,unownedOk:count(unowned,"unassigned-ok"),unownedWrong:count(unowned,"wrong"),unownedNamed:count(unowned,"unassigned-named")}
}
if(import.meta.main){
  const out:string[]=[]
  const sets=await Promise.all(labels.map(async label=>({label,runs:await load(label)})))
  const percent=(part:number,whole:number)=>whole?`${Math.round(part/whole*100)}%`:"–"
  for(const scope of ["all","shared","tuned","held-out"] as const){
    out.push("",`### ${scope==="all"?"All generation scenarios":scope==="shared"?"Shared meetings (8 fixtures)":scope==="tuned"?"Hard cases, tuned (7 fixtures)":"Hard cases, held out (5 fixtures)"}`,"")
    const rows:string[][]=[]
    for(const {label,runs} of sets)for(const level of ["none","sparse","detailed","all"]){
      const mine=runs.filter(run=>(scope==="all"||group(run)===scope)&&(level==="all"||run.variant===`notes:${level}`))
      if(!mine.length)continue
      const own=ownership(mine),ok=mine.filter(run=>run.ok)
      const style=(id:string)=>show(rate(mine,`style:${id}`))
      rows.push([label,level,String(mine.length),`${ok.length}/${mine.length}`,show(rate(mine,"decision:")),show(rate(mine,"action:")),show(rate(mine,"fact:")),show(rate(mine,"not-said:")),
        own.owned?`${percent(own.correct,own.owned)} / ${percent(own.missing,own.owned)} / ${percent(own.wrong,own.owned)} (n=${own.owned})`:"–",
        own.unowned?`${own.unownedOk}/${own.unowned} (wrongly named ${own.unownedWrong})`:"–",
        show(rate(mine,"typed:no-line-dropped")),show(rate(mine,"typed:headings-all-in-order")),show(rate(mine,"unnoted-fact:")),show(rate(mine,"unnoted-decision:")),
        style("heading-count"),style("fragment-bullets"),style("action-format"),style("length-budget"),
        ["faithfulness","coverage","usefulness","concision","notesFidelity"].map(dimension=>fixed(judge(mine,dimension))).join(" / "),
        String(Math.round(median(ok.flatMap(run=>metrics(run)?.words??[])))),`${fixed(median(ok.map(run=>run.perf.totalMs))/1000)}s (max ${fixed(Math.max(0,...ok.map(run=>run.perf.totalMs))/1000)})`,
        `${Math.round(median(mine.map(run=>run.perf.inputTokens)))} / ${Math.round(median(mine.map(run=>run.perf.outputTokens)))} (max ${Math.max(...mine.map(run=>run.perf.outputTokens))})`,`$${fixed(mean(mine.map(run=>run.perf.costUsd)),4)}`])
    }
    out.push(table(["Run","Typed notes","Runs","Completed","Decisions","Action items","Key facts","Not-said clean","Owner: correct / missing / wrong","Unowned tasks left unowned","No typed line dropped","Typed headings all, in order","Un-noted facts kept (sparse)","Un-noted decisions kept (sparse)","Heading count ok","Fragment bullets","Action format","Within length budget","Judge: faith / cover / useful / concise / notes","Median words","Median time","Tokens in / out","Mean cost"],rows))
  }
  out.push("","### Per fixture (all levels pooled)","")
  const fixtures=[...new Set(sets.flatMap(set=>set.runs.map(run=>run.fixture)))]
  out.push(table(["Fixture",...sets.flatMap(set=>[`${set.label}: gold present`,"owner c/m/w","not-said hits","judge f/c"])],fixtures.map(fixture=>[fixture,...sets.flatMap(set=>{
    const mine=set.runs.filter(run=>run.fixture===fixture);if(!mine.length)return ["–","–","–","–"]
    const gold={pass:["decision:","action:","fact:"].reduce((sum,prefix)=>sum+rate(mine,prefix).pass,0),total:["decision:","action:","fact:"].reduce((sum,prefix)=>sum+rate(mine,prefix).total,0)},own=ownership(mine),bad=rate(mine,"not-said:"),made=rate(mine,"no-manufactured")
    return [show(gold),`${own.correct}/${own.missing}/${own.wrong+own.unownedWrong}`,String(bad.total-bad.pass+made.total-made.pass),`${fixed(judge(mine,"faithfulness"))} / ${fixed(judge(mine,"coverage"))}`]
  })])))
  const examples=args.find(arg=>arg.startsWith("--examples="))?.slice(11).split(",")??[]
  for(const id of examples)for(const {label,runs} of sets){
    const run=runs.find(candidate=>candidate.scenario===id&&candidate.rep===1)
    if(!run)continue
    out.push("",`#### ${id} — ${label}`,"","~~~markdown",run.output,"~~~","",`Not passed: ${run.checks.filter(check=>!check.pass).map(check=>`${check.soft?"(soft) ":""}${check.id}${check.detail?` [${check.detail.slice(0,120)}]`:""}`).join("; ")||"none"}`)
    if(run.judge&&!run.judge.error)out.push(`Judge ${Object.entries(run.judge.scores).map(([dimension,value])=>`${dimension} ${value.score}`).join(", ")}: ${run.judge.overall}`)
  }
  console.log(out.join("\n"))
}
