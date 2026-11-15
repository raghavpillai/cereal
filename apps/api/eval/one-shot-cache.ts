/** What the provider's prompt cache does with requests that are not conversations: note generation and
 * "explain this paragraph" (evidence).
 *
 * Three generations on three different meetings, then three evidence requests on one meeting, one
 * after another, through `makeProviderServices` to the real provider. For each call: the provider's
 * own prompt, cached and cache-written token counts and its cost. A generation shares only its
 * instructions with the next one; evidence requests on one note share the instructions and the
 * transcript. No prompt text lives here.
 *
 *   bun --env-file=.env apps/api/eval/one-shot-cache.ts --label one-shot-after
 *
 * Options: --model provider:id (default openrouter:$OPENROUTER_MODEL), --label name. */
import {Effect} from "effect"
import {mkdirSync,writeFileSync} from "node:fs"
import {resolve} from "node:path"
import {makeProviderServices} from "../src/providers"
import {fixtureById,fixtureNote,meetingContextFor} from "./lib/fixtures"

const args=process.argv.slice(2),flag=(name:string)=>{const index=args.indexOf(`--${name}`);return index>=0?args[index+1]:undefined}
const target=flag("model")??`openrouter:${process.env.OPENROUTER_MODEL??"openai/gpt-6-sol"}`,[provider,...modelParts]=target.split(":"),model=modelParts.join(":")
if(provider!=="openrouter"&&provider!=="openai"&&provider!=="anthropic")throw new Error(`Unknown provider in --model ${target}`)
const label=flag("label")??`one-shot-${new Date().toISOString().replace(/[:.]/g,"-")}`

interface Row {kind:string;what:string;promptTokens:number|null;cachedTokens:number;cacheWriteTokens:number;outputTokens:number|null;costUsd:number|null;ms:number;ok:boolean;fields:string}
const rows:Row[]=[]
let current={kind:"",what:""}
const number=(value:unknown)=>typeof value==="number"&&Number.isFinite(value)?value:null
const metered=(async(input:Parameters<typeof fetch>[0],init?:RequestInit)=>{
  const started=performance.now(),body=JSON.parse(String(init?.body)) as Record<string,any>
  const response=await fetch(input,init),text=await response.text()
  let usage:Record<string,any>={}
  try{usage=JSON.parse(text).usage??{}}catch{/* not JSON */}
  const read=number(usage.prompt_tokens_details?.cached_tokens)??number(usage.cache_read_input_tokens)??0,written=number(usage.prompt_tokens_details?.cache_write_tokens)??number(usage.cache_creation_input_tokens)??0
  rows.push({...current,promptTokens:number(usage.prompt_tokens)??(number(usage.input_tokens)===null?null:number(usage.input_tokens)!+read+written),cachedTokens:read,cacheWriteTokens:written,outputTokens:number(usage.completion_tokens)??number(usage.output_tokens),costUsd:number(usage.cost),
    ms:Math.round(performance.now()-started),ok:response.ok,
    // Which cache fields the request carried, without its content.
    fields:[body.prompt_cache_options?`prompt_cache_options=${JSON.stringify(body.prompt_cache_options)}`:"",body.cache_control?"cache_control (top level)":"",/prompt_cache_breakpoint/.test(String(init?.body))?"prompt_cache_breakpoint":"",/"cache_control"/.test(JSON.stringify(body.messages??"")+JSON.stringify(body.system??""))?"cache_control (block)":""].filter(Boolean).join(", ")||"none"})
  return new Response(text,{status:response.status,headers:response.headers})
}) as typeof fetch
const providers=makeProviderServices({languageProvider:provider,languageModel:model,toolModel:model,fetch:metered})
const now=new Date()

for(const id of ["standup","numbers","crosstalk"]){
  const fixture=fixtureById(id),note={...fixtureNote(fixture,{now,level:"sparse"}),meetingContext:meetingContextFor(fixture,now)}
  current={kind:"generation",what:fixture.title}
  const output=await Effect.runPromise(providers.generate(note)).catch(()=>null)
  console.log(`generation  ${fixture.title}: ${output===null?"FAILED":`${output.length} characters of notes`}  ${JSON.stringify(rows.at(-1))}`)
}
const fixture=fixtureById("discovery"),note=fixtureNote(fixture,{now,level:"none"})
for(const item of [fixture.gold.decisions[0]!,fixture.gold.facts[0]!,fixture.gold.actionItems[0]!]){
  current={kind:"evidence",what:item.text.slice(0,70)}
  const output=await Effect.runPromise(providers.evidence!(note,item.text)).catch(()=>null)
  console.log(`evidence    ${item.text.slice(0,50)}: ${output===null?"FAILED":`${output.segmentIds.length} supporting segments`}  ${JSON.stringify(rows.at(-1))}`)
}
const directory=resolve(import.meta.dir,"results",label)
mkdirSync(directory,{recursive:true})
writeFileSync(resolve(directory,"one-shot-cache.json"),JSON.stringify({target,label,rows},null,1)+"\n")
const table=["| Request | Input | Prompt tokens | Read from cache | Written to cache | Output tokens | Cost | Cache fields sent |","| --- | --- | --- | --- | --- | --- | --- | --- |",
  ...rows.map(row=>`| ${row.kind} | ${row.what} | ${row.promptTokens??"-"} | ${row.cachedTokens} | ${row.cacheWriteTokens} | ${row.outputTokens??"-"} | ${row.costUsd===null?"-":`$${row.costUsd.toFixed(4)}`} | ${row.fields} |`)].join("\n")
writeFileSync(resolve(directory,"one-shot-cache.md"),`# One-shot requests and the prompt cache: ${label}\n\n${target}\n\n${table}\n\nTotal $${rows.reduce((sum,row)=>sum+(row.costUsd??0),0).toFixed(4)}.\n`)
console.log(`\n${table}\n\nTotal $${rows.reduce((sum,row)=>sum+(row.costUsd??0),0).toFixed(4)}\n${resolve(directory,"one-shot-cache.md")}`)
