/**
 * Evaluation harness entry point. It runs the AI features against fictional meetings through the
 * functions the API runs in production, and writes `results.json` and `report.md` for each run.
 * `--help` lists every option.
 *
 *   bun --env-file=.env apps/api/eval/run.ts [feature ...|all] [options]
 *
 * Provider credentials are read from the process environment by the same code
 * the server uses; this file never prints, logs or stores them.
 */
import {createHash} from "node:crypto"
import {mkdir,readdir,readFile,writeFile} from "node:fs/promises"
import {resolve} from "node:path"
import {validateFixtures} from "./lib/fixtures"
import {stubTransport} from "./lib/instrument"
import {judge,salvageScores,JUDGE_PROMPT_VERSION,DIMENSIONS} from "./lib/judge"
import {agreementOf,FEATURES,passed,renderReport,summarise,median,type Agreement,type ReportMeta} from "./lib/report"
import type {Target} from "./lib/harness"
import type {Scenario} from "./lib/scenario"
import type {Feature,RunResult} from "./lib/types"
import {generationScenarios} from "./scenarios/generation"
import {noteChatScenarios} from "./scenarios/note-chat"
import {workspaceChatScenarios} from "./scenarios/workspace-chat"
import {largeWorkspaceScenarios} from "./workspace/scenarios"
import {liveChatScenarios} from "./scenarios/live-chat"
import {meetingChatScenarios} from "./scenarios/meeting-chat"
import {liveTailScenarios} from "./scenarios/live-tail"
import {liveThreadScenarios} from "./scenarios/live-thread"

export interface Options {rescore?:string|null;features:Feature[];fast:boolean;reps:number;judgeReps:number;model:string;provider:Target["provider"];judgeModel:string|null;stub:boolean;only:string|null;concurrency:number;maxCostUsd:number;out:string|null;label:string|null;agreementSamples:number;list:boolean}
const HELP=`Usage: bun --env-file=.env apps/api/eval/run.ts [generation|note-chat|workspace-chat|live-chat|all ...] [options]

  --fast                 run the fast subset (about 40 scenarios instead of about 150)
  --reps N               repetitions of every scenario (default 1); latency and pass rates are reported over them
  --model P:ID           model under test, e.g. openrouter:openai/gpt-6-sol, anthropic:<id>, openai:<id>
                         (default: openrouter:$OPENROUTER_MODEL, else openrouter:openai/gpt-6-sol)
  --judge-model ID       OpenRouter model for the rubric (default anthropic/claude-sonnet-5.5; pick a different family from --model)
  --no-judge             deterministic checks and performance only
  --judge-reps N         judge only the first N repetitions of each scenario (default 1)
  --agreement N          re-judge N outputs twice more to measure judge noise (default 6; 0 disables)
  --only TEXT            only scenarios whose id contains TEXT
  --concurrency N        scenarios in flight at once (default 3)
  --max-cost USD         stop scheduling new work once estimated spend passes this (default 5)
  --stub                 offline stub provider and judge: verifies the mechanics, measures nothing
  --label NAME           results directory name (default: timestamp)
  --out DIR              results root (default apps/api/eval/results)
  --list                 print the selected scenarios and exit
  --rescore DIR          re-apply the current deterministic checks to the stored outputs of an earlier run
                         (no provider calls; judge scores and timings are kept) and rewrite its report`

export function parseOptions(argv:ReadonlyArray<string>,env:NodeJS.ProcessEnv=process.env):Options {
  const options:Options={features:[],fast:false,reps:1,judgeReps:1,model:env.OPENROUTER_MODEL??"openai/gpt-6-sol",provider:"openrouter",judgeModel:"anthropic/claude-sonnet-5.5",stub:false,only:null,concurrency:3,maxCostUsd:5,out:null,label:null,agreementSamples:6,list:false}
  const value=(index:number,flag:string)=>{const next=argv[index+1];if(next===undefined)throw new Error(`${flag} needs a value`);return next}
  const count=(text:string,flag:string,minimum=1)=>{const parsed=Number(text);if(!Number.isInteger(parsed)||parsed<minimum)throw new Error(`${flag} must be an integer of at least ${minimum}`);return parsed}
  for(let i=0;i<argv.length;i++){
    const arg=argv[i]!
    if(arg==="all")options.features=[...FEATURES]
    else if((FEATURES as ReadonlyArray<string>).includes(arg))options.features.push(arg as Feature)
    else if(arg==="--fast")options.fast=true
    else if(arg==="--stub")options.stub=true
    else if(arg==="--no-judge")options.judgeModel=null
    else if(arg==="--list")options.list=true
    else if(arg==="--reps")options.reps=count(value(i++,arg),arg)
    else if(arg==="--judge-reps")options.judgeReps=count(value(i++,arg),arg,0)
    else if(arg==="--agreement")options.agreementSamples=count(value(i++,arg),arg,0)
    else if(arg==="--concurrency")options.concurrency=count(value(i++,arg),arg)
    else if(arg==="--max-cost"){options.maxCostUsd=Number(value(i++,arg));if(!(options.maxCostUsd>0))throw new Error("--max-cost must be positive")}
    else if(arg==="--judge-model")options.judgeModel=value(i++,arg)
    else if(arg==="--only")options.only=value(i++,arg)
    else if(arg==="--rescore")options.rescore=value(i++,arg)
    else if(arg==="--label")options.label=value(i++,arg)
    else if(arg==="--out")options.out=value(i++,arg)
    else if(arg==="--model"){
      const text=value(i++,arg),separator=text.indexOf(":"),provider=text.slice(0,separator)
      if(separator<1||!["openrouter","openai","anthropic"].includes(provider))throw new Error("--model must look like openrouter:<id>, openai:<id> or anthropic:<id>")
      options.provider=provider as Target["provider"];options.model=text.slice(separator+1)
    }else if(arg==="--help"||arg==="-h"){console.log(HELP);process.exit(0)}
    else throw new Error(`Unknown argument ${arg}\n\n${HELP}`)
  }
  if(!options.features.length)options.features=[...FEATURES]
  options.features=FEATURES.filter(feature=>options.features.includes(feature))
  return options
}
export function selectScenarios(options:Pick<Options,"features"|"fast"|"only">,now:Date):Scenario[] {
  const all=[...generationScenarios(now),...noteChatScenarios(now),...workspaceChatScenarios(now),...largeWorkspaceScenarios(now),...liveChatScenarios(now),...liveTailScenarios(now),...liveThreadScenarios(now),...meetingChatScenarios(now)]
  const ids=new Set<string>()
  for(const scenario of all){if(ids.has(scenario.id))throw new Error(`Duplicate scenario id ${scenario.id}`);ids.add(scenario.id)}
  return all.filter(scenario=>options.features.includes(scenario.feature)&&(!options.fast||scenario.fast)&&(!options.only||scenario.id.includes(options.only)))
}
async function pool<T>(items:ReadonlyArray<T>,size:number,work:(item:T)=>Promise<void>) {
  let next=0
  await Promise.all(Array.from({length:Math.min(size,items.length)},async()=>{while(next<items.length)await work(items[next++]!)}))
}
async function sha(paths:ReadonlyArray<string>) {
  const hash=createHash("sha256")
  for(const path of [...paths].sort())hash.update(path).update(await readFile(path))
  return hash.digest("hex")
}
const tree=async(directory:string)=>(await readdir(directory,{recursive:true})).filter(name=>name.endsWith(".ts")).map(name=>resolve(directory,name))
async function git(args:string[]) {
  try{const child=Bun.spawn(["git",...args],{cwd:import.meta.dir,stdout:"pipe",stderr:"ignore"});const text=await new Response(child.stdout).text();await child.exited;return text.trim()}catch{return ""}
}

export async function runEvaluation(options:Options,log:(line:string)=>void=console.log):Promise<{directory:string|null;runs:RunResult[];meta:ReportMeta;report:string}> {
  const now=new Date(),startedAt=now.toISOString(),fixtures=validateFixtures()
  const scenarios=selectScenarios(options,now)
  if(options.list){for(const scenario of scenarios)log(`${scenario.id}  [${scenario.variant}]${scenario.fast?" fast":""}`);log(`${scenarios.length} scenarios`);return {directory:null,runs:[],meta:{} as ReportMeta,report:""}}
  if(!scenarios.length)throw new Error("No scenarios match the selection")
  const keyName=options.provider==="openai"?"OPENAI_API_KEY":options.provider==="anthropic"?"ANTHROPIC_API_KEY":"OPENROUTER_API_KEY"
  if(!options.stub&&!process.env[keyName])throw new Error(`${keyName} is not set in the environment. Run with --env-file pointing at the server's private .env, or use --stub to verify the mechanics without a provider.`)
  const judgeKey=process.env.OPENROUTER_API_KEY??""
  if(!options.stub&&options.judgeModel&&!judgeKey){log("OPENROUTER_API_KEY is not set; the rubric judge is disabled for this run.");options.judgeModel=null}
  const target:Target={provider:options.provider,model:options.model,transport:options.stub?stubTransport:globalThis.fetch,stub:options.stub}
  const judgeTransport=options.stub?stubTransport:globalThis.fetch
  const runs:RunResult[]=[],spend={underTest:0,judge:0}
  let partial:string|null=null,finished=0
  const jobs=Array.from({length:options.reps},(_,rep)=>scenarios.map(scenario=>({scenario,rep:rep+1}))).flat()
  log(`${scenarios.length} scenarios × ${options.reps} = ${jobs.length} runs against ${options.stub?"the offline stub":`${options.provider}:${options.model}`}${options.judgeModel?`, judged by ${options.stub?"the stub":options.judgeModel}`:", no judge"}; budget $${options.maxCostUsd}`)
  const judged=new Map<RunResult,ReturnType<Scenario["judge"]>>()
  await pool(jobs,options.concurrency,async({scenario,rep})=>{
    if(spend.underTest+spend.judge>=options.maxCostUsd){partial??=`stopped at the $${options.maxCostUsd} budget after ${finished} of ${jobs.length} runs`;return}
    let execution
    try{execution=await scenario.execute(target)}catch(error){execution={ok:false,error:`harness error: ${String(error).slice(0,200)}`,output:"",citations:[],tools:[],perf:{totalMs:0,ttftMs:null,preparationMs:null,inputTokens:0,cachedTokens:0,outputTokens:0,costUsd:0,estimated:true,calls:[],toolCalls:{}}}}
    spend.underTest+=execution.perf.costUsd
    const run:RunResult={feature:scenario.feature,scenario:scenario.id,fixture:scenario.fixture,variant:scenario.variant,rep,input:scenario.input,ok:execution.ok,...(execution.error?{error:execution.error}:{}),output:execution.output,
      ...(execution.citations.length?{citations:execution.citations}:{}),...(execution.tools.length?{tools:execution.tools}:{}),checks:scenario.check(execution),perf:execution.perf,...(scenario.context?{context:scenario.context}:{})}
    if(options.judgeModel&&rep<=options.judgeReps&&execution.ok&&execution.output.trim()){
      const input=scenario.judge(execution),result=await judge(input,{model:options.judgeModel,transport:judgeTransport,key:judgeKey})
      run.judge={...result.judgement,usage:{inputTokens:result.calls.reduce((sum,call)=>sum+call.inputTokens,0),outputTokens:result.calls.reduce((sum,call)=>sum+call.outputTokens,0),costUsd:result.calls.reduce((sum,call)=>sum+call.costUsd,0),ms:result.calls.reduce((sum,call)=>sum+call.ms,0),attempts:result.calls.length}};spend.judge+=result.calls.reduce((sum,call)=>sum+call.costUsd,0);judged.set(run,input)
    }
    runs.push(run);finished++
    log(`[${finished}/${jobs.length}] ${passed(run)?"pass":run.ok?"FAIL":"ERROR"} ${scenario.id}${options.reps>1?` #${rep}`:""} ${(run.perf.totalMs/1000).toFixed(1)}s $${(spend.underTest+spend.judge).toFixed(2)}${run.ok?"":` — ${run.error}`}`)
  })
  // Judge noise: identical input, judged twice more.
  let agreement:Agreement|null=null
  if(options.judgeModel&&options.agreementSamples>0&&judged.size){
    const candidates=[...judged.keys()].filter(run=>!run.judge?.error).sort((a,b)=>a.scenario.localeCompare(b.scenario)),step=Math.max(1,Math.floor(candidates.length/options.agreementSamples))
    const sample=candidates.filter((_,index)=>index%step===0).slice(0,options.agreementSamples),scores:Array<Array<Record<string,number|null>>>=[]
    await pool(sample,options.concurrency,async run=>{
      const own=[Object.fromEntries(DIMENSIONS.map(dimension=>[dimension,run.judge!.scores[dimension]?.score??null]))]
      for(let repeat=0;repeat<2;repeat++){
        if(spend.underTest+spend.judge>=options.maxCostUsd)break
        const result=await judge(judged.get(run)!,{model:options.judgeModel!,transport:judgeTransport,key:judgeKey})
        spend.judge+=result.calls.reduce((sum,call)=>sum+call.costUsd,0)
        if(!result.judgement.error)own.push(Object.fromEntries(DIMENSIONS.map(dimension=>[dimension,result.judgement.scores[dimension]?.score??null])))
      }
      if(own.length>1)scores.push(own)
    })
    agreement=agreementOf(scores)
  }
  runs.sort((a,b)=>FEATURES.indexOf(a.feature)-FEATURES.indexOf(b.feature)||a.scenario.localeCompare(b.scenario)||a.rep-b.rep)
  const observed=runs.flatMap(run=>run.perf.calls).find(call=>call.purpose!=="planner")?.parameters??{}
  const source=resolve(import.meta.dir,"../src/ai"),here=resolve(import.meta.dir)
  const meta:ReportMeta={startedAt,finishedAt:new Date().toISOString(),model:options.model,provider:options.provider,judgeModel:options.judgeModel,judgePromptVersion:JUDGE_PROMPT_VERSION,reps:options.reps,fast:options.fast,stub:options.stub,only:options.only,features:options.features,
    observedParameters:observed,promptsSha256:await sha(await tree(source)),fixturesSha256:await sha([...await tree(resolve(here,"fixtures")),...await tree(resolve(here,"scenarios"))]),gitCommit:(await git(["rev-parse","--short","HEAD"]))||"unknown",gitDirty:(await git(["status","--porcelain","--","../src"])).length>0,
    concurrency:options.concurrency,spendUsd:{underTest:spend.underTest,judge:spend.judge,total:spend.underTest+spend.judge},partial,fixtures}
  const report=renderReport(meta,runs,agreement)
  const directory=resolve(options.out??resolve(here,"results"),options.label??startedAt.replace(/[:.]/g,"-"))
  await mkdir(directory,{recursive:true})
  // Per-call request hashes identify the exact request; bodies are not stored.
  const compact=runs.map(run=>({...run,perf:{...run.perf,calls:run.perf.calls.map(({parameters:_parameters,...call})=>call)}}))
  await writeFile(resolve(directory,"results.json"),JSON.stringify({meta,agreement,runs:compact}))
  await writeFile(resolve(directory,"report.md"),report)
  log("")
  for(const feature of FEATURES){
    const summary=summarise(runs,feature)
    if(!summary.runs)continue
    log(`${feature.padEnd(15)} ${String(summary.passed).padStart(3)}/${summary.runs} pass  required ${summary.required.pass}/${summary.required.total}  hallucination-clean ${summary.hallucination.pass}/${summary.hallucination.total}  faithfulness ${Number.isFinite(summary.judge.faithfulness!)?summary.judge.faithfulness!.toFixed(2):"–"}  median ${(median(summary.totalMs)/1000).toFixed(1)}s  $${summary.costUsd.reduce((a,b)=>a+b,0).toFixed(2)}`)
  }
  log(`Spend: $${meta.spendUsd.total.toFixed(2)} ($${spend.underTest.toFixed(2)} under test, $${spend.judge.toFixed(2)} judge)${partial?` — PARTIAL: ${partial}`:""}`)
  log(`Report: ${resolve(directory,"report.md")}`)
  return {directory,runs,meta,report}
}

/** Checks are pure functions of a stored output, so a scoring fix or a new
 * check can be applied to an existing run for free. Dates are rebuilt from the
 * run's own start time so date-relative scenarios line up. */
export async function rescore(directory:string,log:(line:string)=>void=console.log) {
  const path=resolve(directory,"results.json"),stored=JSON.parse(await readFile(path,"utf8")) as {meta:ReportMeta;agreement:Agreement|null;runs:RunResult[]}
  const scenarios=new Map(selectScenarios({features:[...FEATURES],fast:false,only:null},new Date(stored.meta.startedAt)).map(scenario=>[scenario.id,scenario]))
  // Runs of scenarios that have since been removed are dropped, not kept with stale checks.
  const before=stored.runs.length
  stored.runs=stored.runs.filter(run=>scenarios.has(run.scenario))
  let changed=0,salvaged=0
  const missing=before-stored.runs.length
  for(const run of stored.runs){
    const scenario=scenarios.get(run.scenario)!
    if(run.judge?.error&&run.judge.raw){
      const scores=salvageScores(run.judge.raw)
      if(scores){const {error:_error,...rest}=run.judge;run.judge={...rest,scores,overall:"(scores recovered from malformed judge JSON)"};salvaged++}
    }
    const before=JSON.stringify(run.checks)
    run.checks=scenario.check({ok:run.ok,...(run.error?{error:run.error}:{}),output:run.output,citations:run.citations??[],tools:run.tools??[],perf:run.perf})
    if(JSON.stringify(run.checks)!==before)changed++
  }
  await writeFile(path,JSON.stringify(stored))
  await writeFile(resolve(directory,"report.md"),renderReport(stored.meta,stored.runs,stored.agreement))
  log(`Rescored ${stored.runs.length} runs (${changed} changed${missing?`, ${missing} dropped because their scenario no longer exists`:""}${salvaged?`, ${salvaged} judge verdicts recovered`:""}); report rewritten at ${resolve(directory,"report.md")}`)
}

if(import.meta.main){
  try{const options=parseOptions(process.argv.slice(2));if(options.rescore)await rescore(options.rescore);else await runEvaluation(options)}
  catch(error){console.error(error instanceof Error?error.message:String(error));process.exit(1)}
}
