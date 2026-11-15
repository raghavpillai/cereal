/**
 * Runs a workspace-chat scenario over the large fictional workspace through the
 * same `processing.chatStream(null, …)` the API serves.
 *
 * Default: the in-memory repository, via the shared `runChat`.
 *
 * EVAL_WORKSPACE_REPOSITORY=postgres: the same notes are seeded into PostgreSQL
 * for a throwaway user and the request runs on `makePrismaRepository(...).forUser`,
 * so retrieval goes through the SQL search used in production. It refuses to
 * touch anything but a disposable `mn_test_<hex>` database (the naming the
 * isolated test script uses), so it cannot be pointed at real data by accident.
 */
import {Effect,Stream} from "effect"
import type {ChatInput,ChatStreamEventDTO} from "@meeting-notes/contracts"
import {makeProcessingService} from "../../src/processing"
import {makeProviderServices} from "../../src/providers"
import type {NotesRepositoryService} from "../../src/repository"
import {runChat,type Execution,type Target} from "../lib/harness"
import {meter} from "../lib/instrument"
import type {LargeWorkspace} from "./corpus"

export const postgresEnabled=()=>process.env.EVAL_WORKSPACE_REPOSITORY==="postgres"
const seeded=new Map<string,Promise<NotesRepositoryService>>()
/** Seed once per process and corpus date; returns the owner-scoped repository. */
export function postgresRepository(workspace:LargeWorkspace):Promise<NotesRepositoryService> {
  const key=workspace.notes[0]?.date??"empty"
  if(!seeded.has(key))seeded.set(key,(async()=>{
    const url=process.env.DATABASE_URL
    if(!url||!/^\/mn_test_[a-f0-9]+$/.test(new URL(url).pathname))throw new Error("EVAL_WORKSPACE_REPOSITORY=postgres needs DATABASE_URL to name a disposable mn_test_<hex> database")
    const {makePrismaRepository}=await import("../../src/prisma-repository")
    const resources=makePrismaRepository(url),owner=crypto.randomUUID(),db=resources.database
    await db.user.create({data:{id:owner,name:"Eval workspace owner",email:`eval-workspace-${owner}@example.invalid`}})
    // A second account with look-alike content: nothing of it may ever surface.
    const stranger=crypto.randomUUID()
    await db.user.create({data:{id:stranger,name:"Eval stranger",email:`eval-stranger-${stranger}@example.invalid`}})
    await db.note.create({data:{ownerId:stranger,title:"Harrowgate – pricing call",body:"FOREIGN_EVAL_SENTINEL Harrowgate agreed forty percent off",enhanced:"FOREIGN_EVAL_SENTINEL Export launches never",date:new Date(`${key}T00:00:00.000Z`)}})
    for(const note of workspace.notes){
      // Owner-scoped IDs differ per process so repeated runs never collide.
      await db.note.create({data:{id:note.id,ownerId:owner,title:note.title,body:note.body,enhanced:note.enhanced,date:new Date(`${note.date}T00:00:00.000Z`),duration:note.duration,recordingState:note.recordingState,generationState:note.generationState,transcriptRevision:note.transcriptRevision,generatedRevision:note.generatedRevision,
        transcriptSegments:{create:note.transcriptSegments.map(segment=>({id:segment.id,channel:segment.channel,speakerId:segment.speakerId,speakerLabel:segment.speakerLabel,text:segment.text,isFinal:segment.isFinal,startMs:segment.startMs,endMs:segment.endMs}))}}})
    }
    return resources.repository.forUser(owner)
  })())
  return seeded.get(key)!
}

export async function runWorkspaceChat(target:Target,workspace:LargeWorkspace,input:ChatInput):Promise<Execution> {
  if(!postgresEnabled())return runChat(target,workspace.notes,null,input,workspace.contexts)
  const repository=await postgresRepository(workspace),metered=meter(target.transport,"answer")
  const providers=makeProviderServices({languageProvider:target.provider,languageModel:target.model,toolModel:target.model,fetch:metered.transport,...(target.stub?{openRouterKey:"stub",openAiKey:"stub",anthropicKey:"stub"}:{})})
  const processing=makeProcessingService(repository,{worker:false,providers,meetingContext:id=>Effect.succeed(workspace.contexts.get(id)??null)})
  const started=performance.now(),tools:Execution["tools"]=[]
  let output="",first:number|null=null,error:string|undefined,done=false,citations:Execution["citations"]=[]
  try{
    const stream=await Effect.runPromise(processing.chatStream(null,input))
    await Effect.runPromise(Stream.runForEach(stream,(event:ChatStreamEventDTO)=>Effect.sync(()=>{
      if(event.type==="delta"){if(first===null)first=performance.now()-started;output+=event.text}
      else if(event.type==="tool"&&event.tool)tools.push({name:event.tool.name,status:event.tool.status,label:event.tool.label,...(event.tool.resultCount===undefined?{}:{resultCount:event.tool.resultCount})})
      else if(event.type==="citations")citations=(event.citations??[]).map(({id,source,noteId,segmentId,startMs,quote})=>({id,source,noteId,segmentId,startMs,quote}))
      else if(event.type==="error")error=event.text
      else if(event.type==="done")done=true
    })))
  }catch(failure){error=String(failure instanceof Error?failure.message:failure).slice(0,300)}
  await metered.settle()
  await processing.dispose()
  if(!done&&!error)error="Stream ended without a done event"
  const calls=metered.calls,toolCalls:Record<string,number>={}
  for(const tool of tools)if(tool.status!=="running")toolCalls[tool.name]=(toolCalls[tool.name]??0)+1
  return {ok:done&&!error,...(error?{error}:{}),output,citations,tools,perf:{totalMs:Math.round(performance.now()-started),ttftMs:first===null?null:Math.round(first),preparationMs:metered.answerStartedMs()===null?null:Math.round(metered.answerStartedMs()!),
    inputTokens:calls.reduce((sum,call)=>sum+call.inputTokens,0),cachedTokens:calls.reduce((sum,call)=>sum+call.cachedTokens,0),outputTokens:calls.reduce((sum,call)=>sum+call.outputTokens,0),costUsd:calls.reduce((sum,call)=>sum+call.costUsd,0),estimated:calls.some(call=>call.tokensEstimated||call.costEstimated),calls,toolCalls}}
}
