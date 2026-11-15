/**
 * Regeneration checks for note generation.
 *
 *   # How alike are repeated generations of the same input? (free: reads stored repetitions)
 *   bun --no-env-file apps/api/eval/generation-stability.ts <label> [<label> ...]
 *
 *   # Does regenerating after the typed notes change follow the new notes? (three live requests)
 *   bun --env-file=.env apps/api/eval/generation-stability.ts --notes-change [--model provider:id]
 *
 * "Equivalent, not reshuffled" is measured between repetitions of one scenario:
 * the same sections in the same order, the same action items, the same gold
 * items present, and how much of the wording is shared.
 */
import {readFile} from "node:fs/promises"
import {existsSync} from "node:fs"
import {resolve} from "node:path"
import {Effect} from "effect"
import type {NoteDTO} from "@meeting-notes/contracts"
import {makeMemoryRepository} from "../src/repository"
import {makeProcessingService} from "../src/processing"
import {makeProviderServices} from "../src/providers"
import {fixtureNote} from "./lib/fixtures"
import {GENERATION_FIXTURES} from "./lib/generation-fixtures"
import {isActionHeading,parseNotes,typedNotesKept} from "./lib/generation-checks"
import {meter} from "./lib/instrument"
import type {Fixture,RunResult} from "./lib/types"

const normal=(text:string)=>text.toLowerCase().replace(/[*_`]/g,"").replace(/[^a-z0-9$£€%. ]+/g," ").replace(/\s+/g," ").trim()
const headingsOf=(output:string)=>parseNotes(output).filter(line=>line.kind==="heading").map(line=>normal(line.text))
const actionsOf=(output:string)=>new Set(parseNotes(output).filter(line=>line.kind==="bullet"&&line.depth===0&&isActionHeading(line.heading)).map(line=>normal(line.text)))
const tokensOf=(output:string)=>new Set(normal(output).split(" ").filter(word=>word.length>2))
/** Action items compared by meaning, not by string: two tasks are the same when
 * they share at least half their words and name the same owner. Returns the
 * share of tasks, from both lists, that have a counterpart in the other. */
const ownerOf=(task:string)=>/\(([^()]+)\)\s*$/.exec(task)?.[1]??""
const sameTask=(a:string,b:string)=>ownerOf(a)===ownerOf(b)&&jaccard(new Set(a.replace(/\([^()]+\)\s*$/,"").split(" ").filter(word=>word.length>2)),new Set(b.replace(/\([^()]+\)\s*$/,"").split(" ").filter(word=>word.length>2)))>=0.5
const matched=(a:Set<string>,b:Set<string>)=>{if(!a.size&&!b.size)return 1;const left=[...a].filter(task=>[...b].some(other=>sameTask(task,other))).length,right=[...b].filter(task=>[...a].some(other=>sameTask(task,other))).length;return (left+right)/(a.size+b.size)}
const jaccard=<T>(a:Set<T>,b:Set<T>)=>{if(!a.size&&!b.size)return 1;let shared=0;for(const item of a)if(b.has(item))shared++;return shared/(a.size+b.size-shared)}
const mean=(values:number[])=>values.length?values.reduce((sum,value)=>sum+value,0)/values.length:NaN
const percent=(value:number)=>Number.isFinite(value)?`${Math.round(value*100)}%`:"–"

export function stabilityOf(runs:ReadonlyArray<RunResult>) {
  const byScenario=new Map<string,RunResult[]>()
  for(const run of runs)if(run.feature==="generation"&&run.ok)byScenario.set(run.scenario,[...(byScenario.get(run.scenario)??[]),run])
  const pairs:Array<{scenario:string;sameHeadings:boolean;headingOverlap:number;sameActions:boolean;actionOverlap:number;wording:number;goldAgreement:number;lengthRatio:number}>=[]
  for(const [scenario,reps] of byScenario)for(let i=0;i<reps.length;i++)for(let j=i+1;j<reps.length;j++){
    const a=reps[i]!,b=reps[j]!,ha=headingsOf(a.output),hb=headingsOf(b.output),aa=actionsOf(a.output),ab=actionsOf(b.output)
    const gold=a.checks.filter(check=>/^(?:decision|action|fact):/.test(check.id)),same=gold.filter(check=>b.checks.find(other=>other.id===check.id)?.pass===check.pass).length
    const wa=a.output.split(/\s+/).length,wb=b.output.split(/\s+/).length
    pairs.push({scenario,sameHeadings:ha.join("|")===hb.join("|"),headingOverlap:jaccard(new Set(ha),new Set(hb)),sameActions:aa.size===ab.size&&matched(aa,ab)===1,actionOverlap:matched(aa,ab),wording:jaccard(tokensOf(a.output),tokensOf(b.output)),goldAgreement:gold.length?same/gold.length:1,lengthRatio:Math.min(wa,wb)/Math.max(wa,wb)})
  }
  return {scenarios:[...byScenario.values()].filter(reps=>reps.length>1).length,pairs:pairs.length,
    identicalHeadings:mean(pairs.map(pair=>pair.sameHeadings?1:0)),headingOverlap:mean(pairs.map(pair=>pair.headingOverlap)),
    identicalActions:mean(pairs.map(pair=>pair.sameActions?1:0)),actionOverlap:mean(pairs.map(pair=>pair.actionOverlap)),
    wording:mean(pairs.map(pair=>pair.wording)),goldAgreement:mean(pairs.map(pair=>pair.goldAgreement)),lengthRatio:mean(pairs.map(pair=>pair.lengthRatio)),
    leastStable:[...pairs].sort((x,y)=>x.headingOverlap+x.wording-(y.headingOverlap+y.wording)).slice(0,3).map(pair=>`${pair.scenario} (headings ${percent(pair.headingOverlap)}, wording ${percent(pair.wording)})`)}
}

/** Generate, change the typed notes on the stored note, generate again: through
 * the same processing service and repository calls the API uses. */
async function notesChange(provider:"openrouter"|"openai"|"anthropic",model:string) {
  const fixture=GENERATION_FIXTURES.find(item=>item.id==="hard-bakery") as unknown as Fixture,now=new Date()
  const first=fixtureNote(fixture,{now,level:"detailed"})
  const versions:Array<{label:string;body:string}>=[
    {label:"A: the detailed notes (Oven first / Flour / Money)",body:first.body},
    {label:"B: headings reordered and renamed, one line added, one removed",body:"## Flour\nwindrush 612 vs graystone 655\n## Equipment\nvulcan mk5 24.5k - 3 yrs\nask about trade-in for the old oven??\n## Parked\ncinnamon knots price"},
    {label:"C: notes cleared",body:""},
  ]
  const metered=meter(globalThis.fetch,"generation"),repository=makeMemoryRepository([first])
  const processing=makeProcessingService(repository,{worker:false,providers:makeProviderServices({languageProvider:provider,languageModel:model,toolModel:model,fetch:metered.transport})})
  const outputs:string[]=[]
  try{
    for(const [index,version] of versions.entries()){
      if(index>0)await Effect.runPromise(repository.updateNote(first.id,{body:version.body}))
      const current=await Effect.runPromise(repository.getNote(first.id)) as NoteDTO
      const generated=await Effect.runPromise(processing.generate(first.id,current.transcriptRevision))
      outputs.push(generated.enhanced)
      const kept=typedNotesKept(generated.enhanced,version.body),previous=index>0?headingsOf(outputs[index-1]!):[]
      const stale=index>0?previous.filter(heading=>!isActionHeading(heading)&&headingsOf(generated.enhanced).includes(heading)&&!normal(version.body).includes(heading)):[]
      console.log(`\n## ${version.label}\n\nHeadings: ${headingsOf(generated.enhanced).join(" / ")}\nTyped notes kept: ${kept.map(check=>`${check.id} ${check.pass?"pass":"FAIL"} (${check.detail})`).join("; ")||"no typed notes"}\nHeadings carried over from the previous document that the new notes do not ask for: ${stale.length?stale.join(", "):"none"}\nStored body unchanged by generation: ${generated.body===version.body}\n\n~~~markdown\n${generated.enhanced}\n~~~`)
    }
  }finally{await metered.settle();await processing.dispose()}
  console.log(`\nSpend: $${metered.calls.reduce((sum,call)=>sum+call.costUsd,0).toFixed(3)} for ${metered.calls.length} requests`)
}

if(import.meta.main){
  const args=process.argv.slice(2)
  if(args.includes("--notes-change")){
    const target=args[args.indexOf("--model")+1],separator=target?.indexOf(":")??-1
    await notesChange(args.includes("--model")&&separator>0?target!.slice(0,separator) as "openrouter":"openrouter",args.includes("--model")&&separator>0?target!.slice(separator+1):process.env.OPENROUTER_MODEL??"openai/gpt-6-sol")
  }else{
    const root=resolve(import.meta.dir,"results"),rows:string[][]=[]
    for(const label of args){
      const runs:RunResult[]=[]
      for(const part of label.split("+")){const directory=existsSync(part)?resolve(part):resolve(root,part);runs.push(...(JSON.parse(await readFile(resolve(directory,"results.json"),"utf8")) as {runs:RunResult[]}).runs)}
      for(const scope of ["all","shared","tuned","held-out"]){
        const result=stabilityOf(runs.filter(run=>scope==="all"||(run.fixture.startsWith("hard-")?"tuned":run.fixture.startsWith("held-")?"held-out":"shared")===scope))
        if(result.pairs)rows.push([label,scope,String(result.scenarios),String(result.pairs),percent(result.identicalHeadings),percent(result.headingOverlap),percent(result.identicalActions),percent(result.actionOverlap),percent(result.wording),percent(result.goldAgreement),percent(result.lengthRatio),result.leastStable[0]??""])
      }
    }
    const header=["Run","Group","Scenarios","Repetition pairs","Identical section list","Section overlap","Equivalent action list","Action items matched","Shared wording","Same gold items present","Length ratio","Least stable"]
    console.log([`| ${header.join(" | ")} |`,`| ${header.map(()=>"---").join(" | ")} |`,...rows.map(row=>`| ${row.join(" | ")} |`)].join("\n"))
  }
}
