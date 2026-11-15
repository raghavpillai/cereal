/** Prompt-cache measurement for chat during a recording.
 *
 * Replays one fictional meeting as live transcription would store it (one segment per finished
 * utterance, voices numbered once per channel, the sentence in progress as the live session's
 * unfinished tail) and asks a scripted series of questions in ONE chat thread, the way a client does:
 * each request carries the earlier questions and answers as `history`. Every request goes through
 * `processing.chatStream` to the real provider. For each turn it records the provider's own usage
 * (prompt tokens, tokens read from its prompt cache, tokens written to it, cost), time to first token
 * and total time, and how much of the request is a byte-for-byte prefix of the previous one.
 *
 *   bun --env-file=.env apps/api/eval/live-cache.ts --label live-cache-after
 *   bun --env-file=.env apps/api/eval/live-cache.ts --model openrouter:anthropic/claude-sonnet-5.5
 *
 * Options: --model provider:id (default openrouter:$OPENROUTER_MODEL), --label name, --gap seconds
 * (real time between questions, default 2), --turns N (first N questions), --fresh (every question
 * starts a new thread: no history, as the one-tap recipes do), --long (a 100-minute meeting, eight
 * questions in its last forty minutes, no answer checks), --finished (the meeting has ended: three
 * questions in one thread, then two new threads on the same note).
 * No prompt text lives here; a change in src/ai is measured on the next run. */
import {Effect,Stream} from "effect"
import {mkdirSync,writeFileSync} from "node:fs"
import {resolve} from "node:path"
import type {ChatInput,ChatStreamEventDTO,TranscriptSegmentDTO} from "@meeting-notes/contracts"
import {makeMemoryRepository} from "../src/repository"
import {makeProcessingService} from "../src/processing"
import {makeProviderServices} from "../src/providers"
import type {LiveHub} from "../src/live/hub"
import {compile,fixtureById,fixtureNote,meetingContextFor} from "./lib/fixtures"
import {stableUuid} from "./lib/transcript"
import {partnerDay} from "./fixtures/meeting-chat/long"

interface Step {at:string;question:string;must:string[];forbid?:string[];note:string;
  /** Start a new thread with this question (no history). */
  newThread?:boolean}
/** Clinic Desk Q3 planning: about an hour, with a break from 22:00 to 34:00. */
const STEPS:Step[]=[
  {at:"10:25",question:"What did I miss?",must:["reschedul|ticket|forty percent|40 ?%"],note:"catch-up"},
  {at:"13:12",question:"How many support tickets were there in Q2 compared with Q1?",must:["1,?180|one thousand one hundred eighty","1,?420|one thousand four hundred twenty"],note:"early fact"},
  {at:"16:13",question:"What is the cut off for moving an appointment?",must:["twenty.?four|24"],note:"in the sentence being spoken"},
  {at:"19:40",question:"Can patients cancel through the link?",must:["\\bno\\b|not|out|won't|cannot|can't"],forbid:["patients can cancel through the link\\."],note:"decision just made"},
  {at:"21:30",question:"Who owes what by when for the carrier templates?",must:["July (?:10|tenth)","July (?:14|fourteenth)"],note:"two owners, two dates"},
  {at:"35:12",question:"How many engineer weeks can we plan this quarter?",must:["58|fifty.eight"],note:"after the break"},
  {at:"36:22",question:"When is the data warehouse freeze?",must:["August (?:11|eleventh)|Aug(?:ust)? 11"],note:"both dates are in the sentence being spoken"},
  {at:"39:50",question:"Is the insurance eligibility pilot in the Q3 plan?",must:["Q4|pencil|defer|not"],forbid:["ninety.day|90.day","October (?:20|twentieth)"],note:"pencilled out"},
  {at:"43:12",question:"And now?",must:["Q3|\\bin\\b|yes|revers"],forbid:["October (?:20|twentieth)"],note:"follow-up after the reversal"},
  {at:"46:05",question:"When do we review the pilot?",must:["October (?:20|twentieth)|Oct(?:ober)? 20"],note:"just said"},
  {at:"48:25",question:"Did Tobias agree to pay for a contract designer?",must:["\\bno\\b|not|declin|refus|won't"],note:"declined"},
  {at:"50:30",question:"What did I miss?",must:["report|spreadsheet|export|analyst"],note:"catch-up at the end"},
]
const args=process.argv.slice(2),flag=(name:string)=>{const index=args.indexOf(`--${name}`);return index>=0?args[index+1]:undefined}
const target=flag("model")??`openrouter:${process.env.OPENROUTER_MODEL??"openai/gpt-6-sol"}`,[provider,...modelParts]=target.split(":"),model=modelParts.join(":")
if(provider!=="openrouter"&&provider!=="openai"&&provider!=="anthropic")throw new Error(`Unknown provider in --model ${target}`)
const label=flag("label")??`live-cache-${new Date().toISOString().replace(/[:.]/g,"-")}`,gapMs=Number(flag("gap")??2)*1000,fresh=args.includes("--fresh")
const mode=args.includes("--finished")?"finished":args.includes("--long")?"long":"live"
/** The meeting has ended: follow-ups in one thread, then new threads on the same note. */
const FINISHED_STEPS:Step[]=[
  {at:"-",question:"What was decided about the insurance eligibility pilot?",must:["Q3"],note:"first question on the note"},
  {at:"-",question:"And who negotiates the contract?",must:["Tobias"],note:"follow-up"},
  {at:"-",question:"What dates were set for it?",must:["September (?:15|fifteenth)|October (?:20|twentieth)"],note:"follow-up"},
  {at:"-",question:"How many engineer weeks were plannable?",must:["58|fifty.eight"],note:"new thread, different question",newThread:true},
  {at:"-",question:"Write tldr",must:["."],note:"new thread, a recipe",newThread:true},
]
const LONG_QUESTIONS=["What did I miss?","What did they just say?","Summarise the last five minutes.","What is being discussed right now?","What was the last number mentioned?","What did I miss?","Has anything been decided in the last ten minutes?","What did they just say?"]

const ms=(text:string)=>{const [m,s]=text.split(":").map(Number);return (m!*60+s!)*1000}
const clock=(value:number)=>`${Math.floor(value/60000)}:${String(Math.floor(value/1000)%60).padStart(2,"0")}`

interface Call {promptTokens:number|null;cachedTokens:number|null;cacheWriteTokens:number|null;outputTokens:number|null;costUsd:number|null;ttftMs:number|null;ms:number;upstream:string|null;messages:Array<{role:string;content:unknown}>;status:number}
const calls:Call[]=[]
const number=(value:unknown)=>typeof value==="number"&&Number.isFinite(value)?value:null
/** Records what leaves the process and the usage the provider reports for it (OpenAI-compatible and Anthropic frames). */
const metered=(async(input:Parameters<typeof fetch>[0],init?:RequestInit)=>{
  const started=performance.now(),body=JSON.parse(String(init?.body)) as Record<string,any>
  const call:Call={promptTokens:null,cachedTokens:null,cacheWriteTokens:null,outputTokens:null,costUsd:null,ttftMs:null,ms:0,upstream:null,status:0,
    messages:[...(body.system?[{role:"system",content:body.system}]:[]),...(body.messages??[])]}
  calls.push(call)
  const response=await fetch(input,init)
  call.status=response.status
  if(!response.ok||!response.body||!body.stream){call.ms=Math.round(performance.now()-started);return response}
  const [forCaller,forMeter]=response.body.tee()
  pending.push((async()=>{
    const reader=forMeter.getReader(),decoder=new TextDecoder()
    let buffer=""
    const usage=(raw:Record<string,any>)=>{
      if(raw.prompt_tokens!==undefined){
        call.promptTokens=number(raw.prompt_tokens);call.outputTokens=number(raw.completion_tokens)
        call.cachedTokens=number(raw.prompt_tokens_details?.cached_tokens)??0;call.cacheWriteTokens=number(raw.prompt_tokens_details?.cache_write_tokens)??0
        if(number(raw.cost)!==null)call.costUsd=number(raw.cost)
      }else if(raw.input_tokens!==undefined){
        const read=number(raw.cache_read_input_tokens)??0,write=number(raw.cache_creation_input_tokens)??0
        call.promptTokens=(number(raw.input_tokens)??0)+read+write;call.cachedTokens=read;call.cacheWriteTokens=write
      }
      if(raw.output_tokens!==undefined)call.outputTokens=number(raw.output_tokens)
    }
    for(;;){
      const next=await reader.read()
      if(next.done)break
      buffer=(buffer+decoder.decode(next.value,{stream:true})).replace(/\r\n/g,"\n")
      let end:number
      while((end=buffer.indexOf("\n\n"))>=0){
        const frame=buffer.slice(0,end);buffer=buffer.slice(end+2)
        const data=frame.split("\n").filter(line=>line.startsWith("data:")).map(line=>line.slice(5).trimStart()).join("\n")
        if(!data||data==="[DONE]")continue
        let raw:any;try{raw=JSON.parse(data)}catch{continue}
        const text=raw?.choices?.[0]?.delta?.content??(raw?.type==="content_block_delta"?raw.delta?.text:"")
        if(text&&call.ttftMs===null)call.ttftMs=Math.round(performance.now()-started)
        if(typeof raw?.provider==="string")call.upstream=raw.provider
        if(raw?.usage)usage(raw.usage)
        if(raw?.type==="message_start"&&raw.message?.usage)usage(raw.message.usage)
      }
    }
    call.ms=Math.round(performance.now()-started)
  })())
  return new Response(forCaller,{status:response.status,headers:response.headers})
}) as typeof fetch
const pending:Promise<void>[]=[]

const fixture=mode==="long"?partnerDay:fixtureById("planning"),built=compile(fixture,"stream"),now=new Date()
const stamp=(value:number)=>`${Math.floor(value/60000)}:${String(Math.floor(value/1000)%60).padStart(2,"0")}`
const steps:Step[]=(mode==="finished"?FINISHED_STEPS:mode==="long"?LONG_QUESTIONS.map((question,index)=>({at:stamp(built.durationMs*(0.6+0.37*index/(LONG_QUESTIONS.length-1))),question,must:["."],note:"no check"})):STEPS).slice(0,Number(flag("turns")??99))
const numbers:Record<string,Map<string,number>>={mic:new Map(),system:new Map()}
const voice=(speaker:string,channel:"mic"|"system")=>{const known=numbers[channel]!;if(!known.has(speaker))known.set(speaker,known.size+1);return known.get(speaker)!}
/** What live transcription has stored: one segment per finished utterance, voices numbered per channel in order of first speech. */
const stored:TranscriptSegmentDTO[]=built.turns.map(turn=>{const n=voice(turn.speaker,turn.channel);return {id:stableUuid("live-cache",fixture.id,turn.index),channel:turn.channel,speakerId:`${turn.channel}:n:${n}`,speakerLabel:`Speaker ${n}`,text:turn.text,isFinal:true,startMs:turn.startMs,endMs:turn.endMs}})
const note=fixtureNote(fixture,{now,level:"none",capture:"stream",segments:mode==="finished"?stored:[],recording:mode!=="finished"}),repository=makeMemoryRepository([note])
let atMs=0
const hub:LiveHub={size:1,full:()=>false,connect:async()=>{throw new Error("not used")},active:()=>true,pendingRange:()=>null,flush:async()=>{},end:async()=>{},dispose:async()=>{},
  snapshot:()=>{
    const turn=built.turns.find(candidate=>candidate.startMs<atMs&&candidate.endMs>atMs)
    if(!turn)return {asOfMs:atMs,state:"live" as const,tail:[]}
    // Words heard so far, in proportion to the time the utterance has been going.
    const words=turn.text.split(/\s+/),heard=words.slice(0,Math.max(1,Math.floor(words.length*(atMs-turn.startMs)/(turn.endMs-turn.startMs)))).join(" ")
    return {asOfMs:atMs,state:"live" as const,tail:[{channel:turn.channel,text:heard,startMs:turn.startMs,speakerId:`${turn.channel}:n:${voice(turn.speaker,turn.channel)}`}]}
  }}
const meeting=meetingContextFor(fixture,now)
const providers=makeProviderServices({languageProvider:provider,languageModel:model,toolModel:model,fetch:metered,...({onLanguageUsage:()=>{}} as object)})
const processing=makeProcessingService(repository,{worker:false,providers,...(mode==="finished"?{}:{live:hub}),meetingContext:()=>Effect.succeed(meeting)})

const flat=(messages:Call["messages"])=>messages.map(message=>`${message.role}\u0001${typeof message.content==="string"?message.content:(message.content as Array<{text?:string}>).map(part=>part.text??"").join("")}`).join("\u0002")
const shared=(a:string,b:string)=>{let i=0;const n=Math.min(a.length,b.length);while(i<n&&a[i]===b[i])i++;return i}
const history:Array<ChatInput["history"][number]>=[],rows:Array<Record<string,unknown>>=[]
let previous:string|null=null
console.log(`${fixture.title}: ${steps.length} questions, ${target}${fresh?", every question a new thread":""}`)
for(const [index,step] of steps.entries()){
  if(mode!=="finished"){
    atMs=ms(step.at)
    await Effect.runPromise(repository.updateNote(note.id,{transcriptSegments:stored.filter(segment=>(segment.endMs??segment.startMs)<=atMs)}))
  }
  if(step.newThread)history.length=0
  const input:ChatInput={message:step.question,history:fresh?[]:history.slice(-30),includeSources:true,timeZone:"UTC"}
  const started=performance.now(),before=calls.length
  let output="",first:number|null=null,error:string|undefined,citations=0
  try{
    const stream=await Effect.runPromise(processing.chatStream(note.id,input))
    await Effect.runPromise(Stream.runForEach(stream,(event:ChatStreamEventDTO)=>Effect.sync(()=>{
      if(event.type==="delta"){if(first===null)first=performance.now()-started;output+=event.text}
      else if(event.type==="citations")citations=event.citations?.length??0
      else if(event.type==="error")error=event.text
    })))
  }catch(failure){error=String(failure)}
  const total=performance.now()-started
  await Promise.allSettled(pending)
  const call=calls[before]
  const text=call?flat(call.messages):"",prefix=previous===null?null:shared(previous,text)
  const normal=output.replace(/\*\*|__/g,"").replace(/\s+/g," ")
  const pass=!error&&step.must.every(source=>new RegExp(source,"i").test(normal))&&!(step.forbid??[]).some(source=>new RegExp(source,"i").test(normal))
  rows.push({turn:index+1,at:step.at,question:step.question,note:step.note,storedSegments:mode==="finished"?stored.length:stored.filter(segment=>(segment.endMs??segment.startMs)<=atMs).length,
    promptTokens:call?.promptTokens??null,cachedTokens:call?.cachedTokens??null,cacheWriteTokens:call?.cacheWriteTokens??null,outputTokens:call?.outputTokens??null,costUsd:call?.costUsd??null,
    ttftMs:first===null?null:Math.round(first),providerTtftMs:call?.ttftMs??null,totalMs:Math.round(total),upstream:call?.upstream??null,
    requestCharacters:text.length,sharedPrefixCharacters:prefix,previousRequestCharacters:previous?.length??null,messages:call?.messages.length??0,citations,pass,...(error?{error}:{}),answer:output})
  const row=rows.at(-1)!
  console.log(`${String(index+1).padStart(2)} ${step.at}  prompt ${row.promptTokens}  cached ${row.cachedTokens}  write ${row.cacheWriteTokens}  first ${row.ttftMs} ms  total ${row.totalMs} ms  prefix ${prefix===null?"-":`${prefix}/${previous!.length}`}  ${pass?"ok":"CHECK"}${error?`  ERROR ${error}`:""}`)
  previous=text
  if(!error&&output){history.push({role:"user",content:step.question},{role:"assistant",content:output.slice(0,20_000)})}
  if(index<steps.length-1&&gapMs>0)await new Promise(done=>setTimeout(done,gapMs))
}
await processing.dispose()

const sum=(key:string)=>rows.reduce((total,row)=>total+(Number(row[key])||0),0),median=(values:number[])=>{const sorted=[...values].sort((a,b)=>a-b);return sorted.length?sorted[Math.floor((sorted.length-1)/2)]!:0}
const later=rows.slice(1)
const summary={target,label,mode,fresh,turns:rows.length,promptTokens:sum("promptTokens"),cachedTokens:sum("cachedTokens"),cacheWriteTokens:sum("cacheWriteTokens"),costUsd:Number(sum("costUsd").toFixed(5)),
  cacheHitRateAfterFirstTurn:later.length?Number((later.reduce((total,row)=>total+(Number(row.cachedTokens)||0),0)/Math.max(1,later.reduce((total,row)=>total+(Number(row.promptTokens)||0),0))).toFixed(3)):null,
  medianTtftMs:median(rows.map(row=>Number(row.ttftMs)).filter(Boolean)),medianTotalMs:median(rows.map(row=>Number(row.totalMs))),checksPassed:rows.filter(row=>row.pass).length,loadAverage:Bun.spawnSync(["uptime"]).stdout.toString().trim().replace(/^.*load averages?: /,"")}
const directory=resolve(import.meta.dir,"results",label)
mkdirSync(directory,{recursive:true})
writeFileSync(resolve(directory,"live-cache.json"),JSON.stringify({summary,rows},null,1)+"\n")
const percent=(row:Record<string,unknown>)=>row.promptTokens?`${Math.round(100*Number(row.cachedTokens)/Number(row.promptTokens))}%`:"-"
const table=["| Turn | Asked at | Stored lines | Prompt tokens | Cached | Cache hit | Written to cache | Cost | First token | Total | Byte-identical prefix of previous request | Check |","| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |",
  ...rows.map(row=>`| ${row.turn} | ${row.at} | ${row.storedSegments} | ${row.promptTokens??"-"} | ${row.cachedTokens??"-"} | ${percent(row)} | ${row.cacheWriteTokens??"-"} | ${row.costUsd===null?"-":`$${Number(row.costUsd).toFixed(4)}`} | ${row.ttftMs===null?"-":`${(Number(row.ttftMs)/1000).toFixed(2)} s`} | ${(Number(row.totalMs)/1000).toFixed(2)} s | ${row.sharedPrefixCharacters===null?"-":`${Math.round(100*Number(row.sharedPrefixCharacters)/Number(row.previousRequestCharacters))}%`} | ${row.pass?"ok":"**check**"} |`)].join("\n")
writeFileSync(resolve(directory,"live-cache.md"),`# Live chat prompt cache: ${label}\n\n${target}, ${fixture.title}, ${rows.length} questions${fresh?", every question a new thread":" in one thread"}. Load average while running: ${summary.loadAverage}.\n\n${table}\n\nTotals: ${summary.promptTokens} prompt tokens, ${summary.cachedTokens} read from cache, ${summary.cacheWriteTokens} written; $${summary.costUsd}. Cache hit rate after the first turn: ${summary.cacheHitRateAfterFirstTurn===null?"-":`${Math.round(100*summary.cacheHitRateAfterFirstTurn)}%`}. Median first token ${summary.medianTtftMs} ms, median total ${summary.medianTotalMs} ms. Checks passed: ${summary.checksPassed}/${rows.length}.\n\n## Answers\n\n${rows.map(row=>`**${row.turn}. ${row.question}** (${row.at}; ${row.note})\n\n${row.answer||`_${row.error}_`}`).join("\n\n")}\n`)
console.log(`\n${JSON.stringify(summary)}\n${resolve(directory,"live-cache.md")}`)
