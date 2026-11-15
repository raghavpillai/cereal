/** Runs each feature through the functions the API itself calls:
 * `makeProcessingService(...).generate` and `.chatStream`, on the in-memory
 * repository, with `makeProviderServices` building and sending the requests.
 * Nothing here knows the prompts. */
import {Effect,Stream} from "effect"
import type {ChatInput,ChatStreamEventDTO,MeetingContextDTO,NoteDTO} from "@meeting-notes/contracts"
import {makeMemoryRepository} from "../../src/repository"
import {makeProcessingService} from "../../src/processing"
import {makeProviderServices} from "../../src/providers"
import type {LiveHub} from "../../src/live/hub"
import {meter} from "./instrument"
import type {CallMetric,RunResult} from "./types"

export interface Target {provider:"openrouter"|"openai"|"anthropic";model:string;transport:typeof fetch;stub:boolean}
export interface Execution {ok:boolean;error?:string;output:string;citations:NonNullable<RunResult["citations"]>;tools:NonNullable<RunResult["tools"]>;perf:RunResult["perf"]}

function providersFor(target:Target,purpose:CallMetric["purpose"]) {
  const metered=meter(target.transport,purpose)
  // Production resolves one model per request and uses it for planning too
  // (`AdministrationService.configuration`); mirror that here.
  const providers=makeProviderServices({languageProvider:target.provider,languageModel:target.model,toolModel:target.model,fetch:metered.transport,onLanguageUsage:()=>{},
    ...(target.stub?{openRouterKey:"stub",openAiKey:"stub",anthropicKey:"stub"}:{})})
  return {providers,metered}
}
function perf(calls:CallMetric[],totalMs:number,ttftMs:number|null,preparationMs:number|null,tools:Execution["tools"]):RunResult["perf"] {
  const toolCalls:Record<string,number>={}
  for(const tool of tools)if(tool.status!=="running")toolCalls[tool.name]=(toolCalls[tool.name]??0)+1
  return {totalMs:Math.round(totalMs),ttftMs:ttftMs===null?null:Math.round(ttftMs),preparationMs:preparationMs===null?null:Math.round(preparationMs),
    inputTokens:calls.reduce((sum,call)=>sum+call.inputTokens,0),cachedTokens:calls.reduce((sum,call)=>sum+call.cachedTokens,0),outputTokens:calls.reduce((sum,call)=>sum+call.outputTokens,0),costUsd:calls.reduce((sum,call)=>sum+call.costUsd,0),
    estimated:calls.some(call=>call.tokensEstimated||call.costEstimated),calls,toolCalls}
}
const failureText=(error:unknown,calls:CallMetric[])=>{
  const api=typeof error==="object"&&error!==null&&"message" in error?String((error as {message:unknown}).message):String(error)
  const last=calls.at(-1)
  return `${api}${last?` (provider status ${last.status}${last.finishReason?`, finish ${last.finishReason}`:""})`:""}`.slice(0,300)
}

/** POST /api/v1/notes/:id/generate. */
export async function runGeneration(target:Target,note:NoteDTO,meeting:MeetingContextDTO|null):Promise<Execution> {
  const {providers,metered}=providersFor(target,"generation"),repository=makeMemoryRepository([note])
  const processing=makeProcessingService(repository,{worker:false,providers,...(meeting?{meetingContext:()=>Effect.succeed(meeting)}:{})})
  const started=performance.now()
  try{
    const generated=await Effect.runPromise(processing.generate(note.id,note.transcriptRevision))
    await metered.settle()
    return {ok:true,output:generated.enhanced,citations:[],tools:[],perf:perf(metered.calls,performance.now()-started,null,null,[])}
  }catch(error){
    await metered.settle()
    return {ok:false,error:failureText(error,metered.calls),output:"",citations:[],tools:[],perf:perf(metered.calls,performance.now()-started,null,null,[])}
  }finally{await processing.dispose()}
}

/** POST /api/v1/notes/:id/chat/stream (noteId set) or /api/v1/chat/stream (null). */
/** `live`: the live transcription sessions of the server, when the scenario has one (its unfinished utterances reach chat through it). */
export async function runChat(target:Target,notes:ReadonlyArray<NoteDTO>,noteId:string|null,input:ChatInput,meetings:ReadonlyMap<string,MeetingContextDTO>=new Map(),live?:LiveHub):Promise<Execution> {
  const {providers,metered}=providersFor(target,"answer"),repository=makeMemoryRepository(notes)
  const processing=makeProcessingService(repository,{worker:false,providers,meetingContext:id=>Effect.succeed(meetings.get(id)??null),...(live?{live}:{})})
  const started=performance.now(),tools:Execution["tools"]=[]
  let output="",first:number|null=null,error:string|undefined,done=false,citations:Execution["citations"]=[]
  try{
    const stream=await Effect.runPromise(processing.chatStream(noteId,input))
    await Effect.runPromise(Stream.runForEach(stream,(event:ChatStreamEventDTO)=>Effect.sync(()=>{
      if(event.type==="delta"){if(first===null)first=performance.now()-started;output+=event.text}
      else if(event.type==="tool"&&event.tool)tools.push({name:event.tool.name,status:event.tool.status,label:event.tool.label,...(event.tool.resultCount===undefined?{}:{resultCount:event.tool.resultCount})})
      else if(event.type==="citations")citations=(event.citations??[]).map(({id,source,noteId,segmentId,startMs,quote})=>({id,source,noteId,segmentId,startMs,quote}))
      else if(event.type==="error")error=event.text
      else if(event.type==="done")done=true
    })))
  }catch(failure){error=failureText(failure,metered.calls)}
  await metered.settle()
  await processing.dispose()
  if(!done&&!error)error="Stream ended without a done event"
  if(error&&metered.calls.at(-1))error=`${error} (provider status ${metered.calls.at(-1)!.status}${metered.calls.at(-1)!.finishReason?`, finish ${metered.calls.at(-1)!.finishReason}`:""})`
  return {ok:done&&!error,...(error?{error}:{}),output,citations,tools,perf:perf(metered.calls,performance.now()-started,first,metered.answerStartedMs(),tools)}
}
