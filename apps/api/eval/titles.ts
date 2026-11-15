/** Titles for notes that have none.
 *
 * An impromptu note (no calendar event) is untitled until its notes are generated; the same run then asks
 * the notes model what to call the meeting (`src/ai/note-title.ts`). This runs that path, generation and
 * title together through `makeProcessingService(...).generate` as the API does, on untitled copies of the
 * harness meetings or on meetings from a file, and prints each title with its checks
 * (`lib/title-checks.ts`), the time the title request took and what it cost.
 *
 *   bun --env-file=.env apps/api/eval/titles.ts [--model provider:id] [--only id] [--all] [--meetings file.json] [--label name] [--stub]
 *
 * Without --meetings: the three meetings of the fast generation subset (--all: every shared fixture).
 * --meetings: a JSON array of {id, segments, people?}, `segments` in the stored transcript shape.
 * --label writes results.json under eval/results/<label>. --stub answers offline and measures nothing.
 * Provider credentials are read from the environment by the code the server uses; nothing here prints them. */
import {Effect} from "effect"
import {mkdirSync,readFileSync,writeFileSync} from "node:fs"
import {resolve} from "node:path"
import type {NoteDTO} from "@meeting-notes/contracts"
import {makeMemoryRepository} from "../src/repository"
import {makeProcessingService} from "../src/processing"
import {makeProviderServices} from "../src/providers"
import {FIXTURES,fixtureNote} from "./lib/fixtures"
import {meter,stubTransport} from "./lib/instrument"
import {titleChecks} from "./lib/title-checks"
import type {Check} from "./lib/types"

export interface TitleMeeting {id:string;segments:NoteDTO["transcriptSegments"];people?:ReadonlyArray<string>}
export interface TitleResult {id:string;ok:boolean;error?:string;title:string;notes:string;checks:Check[];titleMs:number|null;titleCostUsd:number|null;titleOutputTokens:number|null;totalMs:number}
export interface TitleTarget {provider:"openrouter"|"openai"|"anthropic";model:string;transport:typeof fetch;stub:boolean}

const FAST=["standup","numbers","discovery"]
/** The harness meetings as untitled notes without a calendar event, which is what an impromptu recording is. */
export const fixtureMeetings=(all:boolean,now:Date):TitleMeeting[]=>FIXTURES.filter(fixture=>all||FAST.includes(fixture.id))
  .map(fixture=>({id:fixture.id,segments:fixtureNote(fixture,{now,level:"none"}).transcriptSegments,people:Object.keys(fixture.people)}))

export async function runTitle(target:TitleTarget,meeting:TitleMeeting,now:Date):Promise<TitleResult> {
  const metered=meter(target.transport,"generation"),stamp=now.toISOString()
  const end=Math.max(0,...meeting.segments.map(segment=>segment.endMs??segment.startMs))
  const note:NoteDTO={id:crypto.randomUUID(),title:"",body:"",enhanced:"",date:stamp.slice(0,10),duration:`${Math.floor(end/60000)}:${String(Math.floor(end/1000)%60).padStart(2,"0")}`,createdAt:stamp,updatedAt:stamp,
    transcriptSegments:[...meeting.segments],recordingState:"stopped",generationState:"idle",transcriptRevision:1,generatedRevision:null}
  const providers=makeProviderServices({languageProvider:target.provider,languageModel:target.model,toolModel:target.model,fetch:metered.transport,onLanguageUsage:()=>{},...(target.stub?{openRouterKey:"stub",openAiKey:"stub",anthropicKey:"stub"}:{})})
  const processing=makeProcessingService(makeMemoryRepository([note]),{worker:false,providers})
  const started=performance.now()
  try{
    const generated=await Effect.runPromise(processing.generate(note.id,note.transcriptRevision))
    await metered.settle()
    // The second request of the run is the title request; a run with one request never asked.
    const call=metered.calls[1]
    const checks=titleChecks(generated.title,{source:`${meeting.segments.map(segment=>segment.text).join("\n")}\n${generated.enhanced}`,...(meeting.people?{people:meeting.people}:{})})
    return {id:meeting.id,ok:checks.every(check=>check.pass||check.soft),title:generated.title,notes:generated.enhanced,checks,titleMs:call?.ms??null,titleCostUsd:call?.costUsd??null,titleOutputTokens:call?.outputTokens??null,totalMs:Math.round(performance.now()-started)}
  }catch(error){
    await metered.settle()
    const message=typeof error==="object"&&error!==null&&"message" in error?String((error as {message:unknown}).message):String(error)
    return {id:meeting.id,ok:false,error:message.slice(0,200),title:"",notes:"",checks:[],titleMs:null,titleCostUsd:null,titleOutputTokens:null,totalMs:Math.round(performance.now()-started)}
  }finally{await processing.dispose()}
}

if(import.meta.main){
  const args=process.argv.slice(2),flag=(name:string)=>{const index=args.indexOf(`--${name}`);return index>=0?args[index+1]:undefined},has=(name:string)=>args.includes(`--${name}`)
  const chosen=flag("model")??`openrouter:${process.env.OPENROUTER_MODEL??"openai/gpt-6-sol"}`,[provider,...modelParts]=chosen.split(":"),model=modelParts.join(":")
  if(provider!=="openrouter"&&provider!=="openai"&&provider!=="anthropic")throw new Error(`Unknown provider in --model ${chosen}`)
  const now=new Date(),stub=has("stub"),target:TitleTarget={provider,model,transport:stub?stubTransport:fetch,stub}
  const file=flag("meetings"),only=flag("only")
  const meetings=(file?JSON.parse(readFileSync(resolve(file),"utf8")) as TitleMeeting[]:fixtureMeetings(has("all"),now)).filter(meeting=>!only||meeting.id.includes(only))
  if(!meetings.length)throw new Error("No meetings selected")
  console.log(`Titles for ${meetings.length} untitled meeting${meetings.length===1?"":"s"} on ${provider}:${model}${stub?" (stub: nothing is measured)":""}\n`)
  const results:TitleResult[]=[]
  // One at a time: the instructions of the second and later requests can then come from the provider's cache.
  for(const meeting of meetings){
    const result=await runTitle(target,meeting,now);results.push(result)
    const failed=result.checks.filter(check=>!check.pass)
    console.log(`${result.ok?"ok  ":"FAIL"} ${meeting.id.padEnd(12)} ${result.error?`(${result.error})`:JSON.stringify(result.title)}`+
      (result.error?"":`  [${result.title.length} characters, ${result.title.trim().split(/\s+/).filter(Boolean).length} words; title request ${result.titleMs??"?"} ms, ${result.titleOutputTokens??"?"} output tokens${result.titleCostUsd?`, $${result.titleCostUsd.toFixed(5)}`:""}]`))
    for(const check of failed)console.log(`       ${check.soft?"note":"fails"}: ${check.id}${check.detail?` (${check.detail})`:""}`)
  }
  const cost=results.reduce((sum,result)=>sum+(result.titleCostUsd??0),0)
  console.log(`\n${results.filter(result=>result.ok).length} of ${results.length} titles pass; title requests cost $${cost.toFixed(4)} in all`)
  const label=flag("label")
  if(label){
    const directory=resolve(import.meta.dir,"results",label);mkdirSync(directory,{recursive:true})
    writeFileSync(resolve(directory,"results.json"),JSON.stringify({feature:"note-title",provider,model,stub,at:now.toISOString(),results},null,1)+"\n")
    console.log(`Wrote ${resolve(directory,"results.json")}`)
  }
  if(!stub&&results.some(result=>!result.ok))process.exit(1)
}
