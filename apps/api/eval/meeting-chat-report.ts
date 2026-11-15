/** Before/after tables for single-meeting chat, from stored evaluation runs.
 *
 *   bun --no-env-file apps/api/eval/meeting-chat-report.ts <results dir | label=dir,dir> [...] [--failures tuned|all]
 *
 * Each directory is one column. Rows are scenario groups, reported separately
 * for the shared fixtures, the tuned meeting-chat fixtures (`mc-t-`) and the
 * held-out ones (`mc-h-`). No provider calls. */
import {readFile} from "node:fs/promises"
import {resolve} from "node:path"
import type {RunResult} from "./lib/types"

const hard=(run:RunResult)=>run.checks.filter(check=>!check.soft)
const passed=(run:RunResult)=>run.ok&&hard(run).every(check=>check.pass)
const setOf=(run:RunResult)=>run.fixture.startsWith("mc-t-")?"tuned":run.fixture.startsWith("mc-h-")?"held-out":"shared"
const groupOf=(run:RunResult)=>run.feature==="live-chat"?run.variant.split("/")[0]!+(run.variant.split("/")[1]==="stream"?"":` (${run.variant.split("/")[1]})`):run.variant
const median=(values:number[])=>{if(!values.length)return NaN;const sorted=[...values].sort((a,b)=>a-b),middle=(sorted.length-1)/2;return (sorted[Math.floor(middle)]!+sorted[Math.ceil(middle)]!)/2}
const mean=(values:number[])=>values.length?values.reduce((a,b)=>a+b,0)/values.length:NaN
const judge=(runs:RunResult[],dimension:string)=>mean(runs.flatMap(run=>{const score=run.judge?.scores[dimension]?.score;return typeof score==="number"?[score]:[]}))
const fixed=(value:number,digits=1)=>Number.isFinite(value)?value.toFixed(digits):"–"
const rate=(runs:RunResult[],predicate:(run:RunResult)=>boolean)=>runs.length?`${runs.filter(predicate).length}/${runs.length}`:"–"
const checkRate=(runs:RunResult[],id:string)=>{const checks=runs.flatMap(run=>run.checks.filter(check=>check.id===id));return checks.length?`${checks.filter(check=>check.pass).length}/${checks.length}`:"–"}
const cell=(runs:RunResult[])=>runs.length?`${rate(runs,passed)} · ${fixed(judge(runs,"faithfulness"))}/${fixed(judge(runs,"coverage"))}/${fixed(judge(runs,"usefulness"))}`:"–"
const table=(header:string[],rows:string[][])=>[`| ${header.join(" | ")} |`,`| ${header.map(()=>"---").join(" | ")} |`,...rows.map(row=>`| ${row.join(" | ")} |`)].join("\n")

const args=process.argv.slice(2),failuresAt=args.indexOf("--failures"),failures=failuresAt>=0?args[failuresAt+1]:null
const directories=args.filter((arg,index)=>!arg.startsWith("--")&&(failuresAt<0||index!==failuresAt+1))
if(!directories.length){console.error("Give at least one results directory");process.exit(1)}
/** `label=dirA,dirB` merges several runs into one column. */
const columns=await Promise.all(directories.map(async argument=>{
  const [name,list]=argument.includes("=")?argument.split("=") as [string,string]:[null,argument],parts=list.split(",")
  const stored=await Promise.all(parts.map(async directory=>JSON.parse(await readFile(resolve(directory,"results.json"),"utf8")) as {meta:{model:string;spendUsd:{total:number}};runs:RunResult[]}))
  return {label:name??parts[0]!.replace(/\/$/,"").split("/").at(-1)!,runs:stored.flatMap(item=>item.runs).filter(run=>run.feature==="note-chat"||run.feature==="live-chat"),meta:{model:stored[0]!.meta.model,spendUsd:{total:stored.reduce((sum,item)=>sum+item.meta.spendUsd.total,0)}}}
}))
const out:string[]=[`Columns: ${columns.map(column=>`\`${column.label}\` (${column.meta.model}, $${column.meta.spendUsd.total.toFixed(2)})`).join("; ")}`,"","Cells: runs passing every hard check · judge faithfulness/coverage/usefulness (1–5 means).",""]
for(const feature of ["live-chat","note-chat"] as const)for(const set of ["tuned","held-out","shared"]){
  const groups=[...new Set(columns.flatMap(column=>column.runs.filter(run=>run.feature===feature&&setOf(run)===set).map(groupOf)))]
  if(!groups.length)continue
  const select=(column:typeof columns[number],group:string|null)=>column.runs.filter(run=>run.feature===feature&&setOf(run)===set&&(group===null||groupOf(run)===group))
  out.push(`### ${feature}, ${set}`,"",table(["Group",...columns.map(column=>column.label)],[...groups.map(group=>[group,...columns.map(column=>cell(select(column,group)))]),["**all**",...columns.map(column=>cell(select(column,null)))]]),"")
  out.push(table(["Measure",...columns.map(column=>column.label)],[
    ["answer correct (required checks)",...columns.map(column=>{const checks=select(column,null).flatMap(run=>run.checks.filter(check=>check.group==="required"&&!check.soft));return checks.length?`${checks.filter(check=>check.pass).length}/${checks.length}`:"–"})],
    ["planted false or later facts absent",...columns.map(column=>{const checks=select(column,null).flatMap(run=>run.checks.filter(check=>check.group==="hallucination"));return checks.length?`${checks.filter(check=>check.pass).length}/${checks.length}`:"–"})],
    ["substantive answers with a citation (soft)",...columns.map(column=>checkRate(select(column,null),"substantive-answer-cited"))],
    ["citations resolve to stored text",...columns.map(column=>checkRate(select(column,null),"citations-resolve"))],
    ["cited the passage that states the answer (soft)",...columns.map(column=>checkRate(select(column,null),"cites-gold-passage"))],
    ["judge: cited passage supports its sentence",...columns.map(column=>{const verdicts=select(column,null).flatMap(run=>run.judge?.citations??[]);return verdicts.length?`${verdicts.filter(verdict=>verdict.supported).length}/${verdicts.length}`:"–"})],
    ...(feature==="live-chat"?[["recipe format (3 hard checks)",...columns.map(column=>{const checks=select(column,null).flatMap(run=>run.checks.filter(check=>check.id.startsWith("recipe-")&&!check.soft));return checks.length?`${checks.filter(check=>check.pass).length}/${checks.length}`:"–"})],
      ["live answers within the length guide (soft)",...columns.map(column=>checkRate(select(column,null),"live-answer-brief"))]]:[]),
    ["judge concision",...columns.map(column=>fixed(judge(select(column,null),"concision")))],
    ["time to first token, median",...columns.map(column=>`${fixed(median(select(column,null).flatMap(run=>run.perf.ttftMs===null?[]:[run.perf.ttftMs]))/1000,2)} s`)],
    ["total time, median",...columns.map(column=>`${fixed(median(select(column,null).filter(run=>run.ok).map(run=>run.perf.totalMs))/1000,2)} s`)],
    ["input tokens, median (max)",...columns.map(column=>{const values=select(column,null).map(run=>run.perf.inputTokens);return values.length?`${Math.round(median(values))} (${Math.max(...values)})`:"–"})],
    ["output tokens, median",...columns.map(column=>fixed(median(select(column,null).map(run=>run.perf.outputTokens)),0))],
    ["cost per answer, median (mean)",...columns.map(column=>{const values=select(column,null).map(run=>run.perf.costUsd);return `$${fixed(median(values),4)} ($${fixed(mean(values),4)})`})],
  ]),"")
}
if(failures){
  const last=columns.at(-1)!
  out.push(`### Failures in \`${last.label}\`${failures==="tuned"?" (tuned and shared sets only)":""}`,"")
  for(const run of last.runs.filter(run=>!passed(run)&&(failures!=="tuned"||setOf(run)!=="held-out")))
    out.push(`- **${run.scenario}** #${run.rep} [${run.variant}] — ${run.ok?hard(run).filter(check=>!check.pass).map(check=>`${check.id}${check.detail?` (${check.detail})`:""}`).join("; "):run.error}\n  - Q: ${run.input.question}\n  - A: ${run.output.replace(/\n+/g," ⏎ ").slice(0,700)}`)
}
console.log(out.join("\n"))
