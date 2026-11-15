/**
 * Golden capture of every language-model request the API assembles.
 *
 * Each case drives the real code path (makeProviderServices, prepareMeetingChat,
 * processing.chatStream) with a recording transport and compares the exact wire
 * body with `golden/ai-requests.json`. A prompt, context or parameter change
 * therefore shows up as a diff here before it reaches a provider.
 *
 * Intentional change: `UPDATE_AI_GOLDEN=1 bun test apps/api/test/ai-request-golden.test.ts`
 * and review the resulting diff of the golden file.
 */
import {expect,test} from "bun:test"
import {Effect,Stream} from "effect"
import {readFileSync,writeFileSync,existsSync,mkdirSync} from "node:fs"
import {resolve} from "node:path"
import type {ChatInput,ChatStreamEventDTO,MeetingContextDTO,NoteDTO} from "@meeting-notes/contracts"
import {makeMemoryRepository} from "../src/repository"
import {makeProcessingService} from "../src/processing"
import {makeProviderServices,type ChatToolCall,type ChatToolMessage,type ProviderNote,type ProviderServices} from "../src/providers"
import {MEETING_CHAT_TOOLS,prepareMeetingChat} from "../src/chat-retrieval"
import {makeLiveConversationStore} from "../src/ai/live-conversation"

const goldenPath=resolve(import.meta.dir,"golden/ai-requests.json")
const update=process.env.UPDATE_AI_GOLDEN==="1"
type Capture={url:string;body:string}
const captured:Record<string,unknown>={}
const golden:Record<string,unknown>=existsSync(goldenPath)?JSON.parse(readFileSync(goldenPath,"utf8")):{}
function check(name:string,value:unknown){
  const actual=JSON.parse(JSON.stringify(value))
  captured[name]=actual
  if(update)return
  expect(golden).toHaveProperty([name])
  // Compare strings first so a failure names the first differing byte.
  const a=JSON.stringify(actual),b=JSON.stringify(golden[name])
  if(a!==b){let i=0;while(i<a.length&&a[i]===b[i])i++;throw new Error(`${name}: assembled request differs from golden at byte ${i}: …${a.slice(Math.max(0,i-80),i+120)}… vs …${b.slice(Math.max(0,i-80),i+120)}…`)}
}

const id=(n:number)=>`00000000-0000-4000-8000-${String(n).padStart(12,"0")}`
const clock=()=>new Date("2026-03-10T15:00:00.000Z")
const seg=(n:number,channel:"mic"|"system",chunk:number,speaker:number,text:string,startMs:number,endMs:number|null=startMs+4000)=>({id:id(1000+n),channel,speakerId:`${channel}:${id(900+chunk)}:speaker_${speaker}`,speakerLabel:`Speaker ${speaker}`,text,isFinal:true,startMs,endMs})
const note=(n:number,fields:Partial<NoteDTO>):NoteDTO=>({id:id(n),title:"Untitled",body:"",enhanced:"",date:"2026-03-09",duration:"",createdAt:"2026-03-09T10:00:00.000Z",updatedAt:"2026-03-09T11:00:00.000Z",transcriptSegments:[],recordingState:"stopped",generationState:"idle",transcriptRevision:1,generatedRevision:null,...fields})

const pricing=note(1,{title:"Halden pricing review",duration:"12:34",body:"# Pricing concerns\n- Tiered plan?\n- My impression: Oskar hesitant about annual commit",enhanced:"# Pricing\n- A 14% discount was proposed for annual plans, not approved.\n\n# Next steps\n- Ines sends the revised quote by Thursday.",
  transcriptSegments:[
    seg(1,"mic",1,1,"Thanks for joining, Oskar. Today I want to walk through the tiered pricing and the annual commitment.",0),
    seg(2,"system",2,1,"Sure. Honestly the annual commitment worries me, we only have budget approved through June.",5200),
    seg(3,"mic",1,1,"Understood. We could offer a 14 percent discount on the annual plan, but that is a proposal, I need finance to approve it.",11000),
    seg(4,"system",2,2,"This is Ines from procurement. Please send a revised quote by Thursday and include the 40 seat option.",19000),
    seg(5,"mic",3,1,"Will do. I will send the revised quote with 40 seats by Thursday.",66000,null),
  ]})
const blank=note(2,{title:"Quick sync",duration:"1:02:03",date:"2026-03-02",transcriptSegments:[seg(6,"mic",4,1,"Let's ship the importer on the 21st. No blockers from my side.",1000)]})
const long=note(3,{title:"Orchard migration planning",duration:"48 min",date:"2026-02-20",body:"Agenda\n\n- cutover\n- rollback\n\n"+"background ".repeat(560)+"\n\nrollback rehearsal owner?",enhanced:"# Cutover\n- Planned for the first weekend of April.\n\n"+"Detail. ".repeat(1010)+"\n\n# Rollback\n- Rehearsal owner is still open.",
  transcriptSegments:Array.from({length:30},(_,i)=>seg(100+i,i%3===0?"system":"mic",10+Math.floor(i/6),1+i%2,
    i===17?"x ".repeat(1050)+"The rollback rehearsal is scheduled for March 28 and Tomasz owns it. "+"y ".repeat(150)
    :i===26?"Final point: the cutover budget is 18,500 euros, and the freeze starts April 3."
    :`Segment ${i} covers routine migration status item number ${i} without any specific commitment.`,i*60000))})
const other=note(4,{title:"Design critique",date:"2026-03-08",body:"- onboarding flow\n- action item: Lena to update the empty state",transcriptSegments:[seg(200,"mic",20,1,"Action item: Lena will update the empty state illustration before Friday.",3000)],enhanced:"# Onboarding\n- Empty state needs a clearer call to action.\n\n# Action items\n- Lena: update the empty state illustration before Friday."})
const meeting:MeetingContextDTO={eventId:id(500),provider:"ics",title:"Halden pricing review",calendarName:"Work",startAt:"2026-03-09T16:00:00Z",endAt:"2026-03-09T16:30:00Z",allDay:false,timeZone:"Europe/Stockholm",location:"Video call",meetingUrl:"https://meet.example.invalid/halden",eventUrl:null,
  organizer:{id:id(501),displayName:"Maren Solberg",givenName:"Maren",familyName:"Solberg",email:"maren@northwind.example",nameSource:"calendar",isSelf:true,isOrganizer:true,isResource:false,responseStatus:"accepted"},
  participants:[{id:id(501),displayName:"Maren Solberg",givenName:"Maren",familyName:"Solberg",email:"maren@northwind.example",nameSource:"calendar",isSelf:true,isOrganizer:true,isResource:false,responseStatus:"accepted"},{id:id(502),displayName:"Oskar Lindqvist",givenName:null,familyName:null,email:"oskar@halden.example",nameSource:"calendar",isSelf:false,isOrganizer:false,isResource:false,responseStatus:"tentative"}]} as MeetingContextDTO
const withMeeting:ProviderNote={...pricing,meetingContext:meeting}

const sse=(frames:ReadonlyArray<string>)=>new Response(frames.map(frame=>`data: ${frame}\n\n`).join(""),{headers:{"Content-Type":"text/event-stream"}})
/** Records the request as it leaves the process (after any provider adaptation)
 * and answers in that provider's own wire format. */
function recorder(evidenceIds:ReadonlyArray<string>=[]) {
  const requests:Capture[]=[]
  const transport=(async(input:Parameters<typeof fetch>[0],init?:RequestInit)=>{
    const url=String(input),body=String(init?.body),parsed=JSON.parse(body)
    requests.push({url,body})
    if(url.includes("anthropic.com")){
      if(parsed.stream)return sse([JSON.stringify({type:"content_block_delta",delta:{type:"text_delta",text:"ok"}}),JSON.stringify({type:"message_stop"})])
      return Response.json({content:[{type:"text",text:"ok"}],stop_reason:"end_turn"})
    }
    if(parsed.stream)return sse([JSON.stringify({choices:[{delta:{content:"ok"}}]}),"[DONE]"])
    if(parsed.tools)return Response.json({choices:[{message:{content:null}}]})
    if(parsed.response_format)return Response.json({choices:[{message:{content:JSON.stringify({summary:"ok",segmentIds:evidenceIds})}}]})
    return Response.json({choices:[{message:{content:"ok"}}]})
  }) as typeof fetch
  return {requests,transport}
}
const drain=async(stream:AsyncIterable<string>)=>{let text="";for await(const delta of stream)text+=delta;return text}
const signal=()=>new AbortController().signal
const history:ChatInput["history"]=[{role:"user",content:"Who joined from procurement?"},{role:"assistant",content:"Ines joined from procurement."}]

test("generation requests",async()=>{
  for(const [name,options,source] of [
    ["generation/gpt-6-sol/typed-notes+meeting",{languageModel:"openai/gpt-6-sol"},withMeeting],
    ["generation/gpt-6.1/blank-notes",{languageModel:"openai/gpt-6.1-sol"},blank],
    ["generation/gpt-4o-mini/long",{languageModel:"openai/gpt-4o-mini"},long],
    ["generation/openai-direct/gpt-5.1",{languageProvider:"openai",openAiKey:"fixture-key",languageModel:"gpt-5.1"},pricing],
    ["generation/anthropic-direct",{languageProvider:"anthropic",anthropicKey:"fixture-key",languageModel:"claude-fixture-model"},withMeeting],
  ] as const){
    const r=recorder()
    expect(await Effect.runPromise(makeProviderServices({openRouterKey:"fixture-key",...options,fetch:r.transport}).generate(source))).toBe("ok")
    check(name,r.requests)
  }
})

test("title requests for a note without a title",async()=>{
  // The finished notes and the opening of the transcript, on the notes model; nothing else about the note.
  for(const [name,options] of [
    ["title/gpt-6-sol",{languageModel:"openai/gpt-6-sol"}],
    ["title/gpt-4o-mini",{languageModel:"openai/gpt-4o-mini"}],
    ["title/anthropic-direct",{languageProvider:"anthropic",anthropicKey:"fixture-key",languageModel:"claude-fixture-model"}],
  ] as const){
    const r=recorder()
    expect(await Effect.runPromise(makeProviderServices({openRouterKey:"fixture-key",...options,fetch:r.transport}).title!({...pricing,title:""},pricing.enhanced))).toBe("ok")
    check(name,r.requests)
  }
})

test("note chat, evidence and planner requests at the provider boundary",async()=>{
  const inputs:Record<string,ChatInput>={
    "plain":{message:"What discount was discussed?",history:[]},
    "history":{message:"And what did she ask for?",history},
    "recent-todos":{message:"List my recent to-dos",history:[],timeZone:"Europe/Stockholm"},
    "recent-todos-table":{message:"Show the latest action items as a table",history:[]},
  }
  for(const [name,input] of Object.entries(inputs)){
    const r=recorder(),providers=makeProviderServices({openRouterKey:"fixture-key",languageModel:"openai/gpt-6-sol",fetch:r.transport})
    await Effect.runPromise(providers.chat(withMeeting,input))
    await drain(providers.chatStream!(withMeeting,input,signal()))
    check(`chat/provider/${name}`,r.requests)
  }
  const cited:ProviderNote={...pricing,citations:[{id:"S1",noteId:pricing.id,title:pricing.title,source:"transcript",segmentId:pricing.transcriptSegments[2]!.id,startMs:11000,quote:"We could offer a 14 percent discount",transcriptRevision:1}],retrievalCoverage:{bodyTruncated:false,summaryTruncated:false,transcriptTruncated:true}}
  for(const [name,options] of [["gpt-4o-mini",{languageModel:"openai/gpt-4o-mini"}],["anthropic-direct",{languageProvider:"anthropic",anthropicKey:"fixture-key",languageModel:"claude-fixture-model"}]] as const){
    const r=recorder(),providers=makeProviderServices({openRouterKey:"fixture-key",...options,fetch:r.transport})
    await drain(providers.chatStream!(cited,{message:"What discount was discussed?",history,includeSources:true},signal()))
    check(`chat/provider/issued-sources/${name}`,r.requests)
  }
  const r=recorder([pricing.transcriptSegments[2]!.id]),providers=makeProviderServices({openRouterKey:"fixture-key",languageModel:"openai/gpt-6-sol",fetch:r.transport})
  await Effect.runPromise(providers.evidence!(pricing,"A 14% discount was proposed for annual plans, not approved."))
  check("evidence/gpt-6-sol",r.requests)
  const planner:ChatToolMessage[]=[{role:"system",content:"Planner fixture"},{role:"user",content:"Find the pricing review"},{role:"assistant",content:null,tool_calls:[{id:"call_1",type:"function",function:{name:"search_meetings",arguments:'{"query":"pricing"}'}}]},{role:"tool",tool_call_id:"call_1",content:'[{"noteId":"x"}]'}]
  for(const [name,options] of [["default-tool-model",{}],["tool-model",{toolModel:"openai/gpt-6.1-sol",languageModel:"openai/gpt-6-sol"}],["gpt-4o-mini",{languageModel:"openai/gpt-4o-mini"}],["anthropic-direct",{languageProvider:"anthropic",anthropicKey:"fixture-key",languageModel:"claude-fixture-model"}]] as const){
    const t=recorder()
    await makeProviderServices({openRouterKey:"fixture-key",...options,fetch:t.transport}).chatToolStep!(planner,MEETING_CHAT_TOOLS,signal())
    check(`planner/provider/${name}`,t.requests)
  }
})

/** The whole streaming path used by POST /notes/:id/chat/stream. */
async function throughProcessing(noteId:string,input:ChatInput,seed:ReadonlyArray<NoteDTO>,meetingFor?:string) {
  const r=recorder(),repository=makeMemoryRepository(seed,clock)
  const processing=makeProcessingService(repository,{worker:false,clock,providers:makeProviderServices({openRouterKey:"fixture-key",languageModel:"openai/gpt-6-sol",fetch:r.transport}),
    ...(meetingFor?{meetingContext:(requested:string)=>Effect.succeed(requested===meetingFor?meeting:null)}:{})})
  const events:ChatStreamEventDTO[]=[]
  await Effect.runPromise(Stream.runForEach(await Effect.runPromise(processing.chatStream(noteId,input)),event=>Effect.sync(()=>{events.push(event)})))
  await processing.dispose()
  return {requests:r.requests,events:events.map(event=>event.tool?{...event,tool:{...event.tool,callId:"<call>"}}:event)}
}
test("note chat through processing: context selection, citations and final request",async()=>{
  check("note-chat/stream/sources",await throughProcessing(pricing.id,{message:"What discount was proposed and who approves it?",history,includeSources:true,timeZone:"Europe/Stockholm"},[pricing,other],pricing.id))
  check("note-chat/stream/legacy-no-sources",await throughProcessing(pricing.id,{message:"Summarise the pricing concerns",history:[]},[pricing]))
  check("note-chat/stream/long-transcript",await throughProcessing(long.id,{message:"Who owns the rollback rehearsal and what is the cutover budget?",history:[],includeSources:true},[long]))
  check("note-chat/stream/no-term-match",await throughProcessing(long.id,{message:"What was just said?",history:[],includeSources:true},[long]))
  check("note-chat/stream/while-recording",await throughProcessing(pricing.id,{message:"What did Ines ask for?",history:[],includeSources:true},[{...pricing,recordingState:"recording"}]))
})

/** Workspace chat with a scripted planner: every planner round, the tool
 * definitions, progress events and the final answer request. */
async function workspace(input:ChatInput,script:ReadonlyArray<ReadonlyArray<ChatToolCall>>|null,seed:ReadonlyArray<NoteDTO>) {
  const r=recorder(),real=makeProviderServices({openRouterKey:"fixture-key",languageModel:"openai/gpt-6-sol",fetch:r.transport})
  // Planner messages only ever grow, so the last call's list plus each call's
  // length reproduces every round without storing the shared prefix repeatedly.
  const planner={calls:[] as Array<{messageCount:number;model:string|null}>,tools:null as unknown,toolsStable:true,messages:[] as unknown}
  const events:ChatStreamEventDTO[]=[]
  let round=0
  const providers:ProviderServices={...real,...(script?{chatToolStep:async(messages,tools,_signal,model)=>{
    const snapshot=JSON.parse(JSON.stringify({messages,tools}))
    if(planner.tools===null)planner.tools=snapshot.tools;else if(JSON.stringify(planner.tools)!==JSON.stringify(snapshot.tools))planner.toolsStable=false
    planner.calls.push({messageCount:messages.length,model:model??null});planner.messages=snapshot.messages
    return script[round++]??[]}}:{chatToolStep:undefined})}
  const prepared=await prepareMeetingChat({noteId:null,input,repository:makeMemoryRepository(seed,clock),providers,signal:signal(),now:clock(),
    enrich:async source=>source.id===pricing.id?{...source,meetingContext:meeting}:source,emit:event=>{events.push(event)}})
  await drain(real.chatStream!(prepared.note,input,signal()))
  return {planner,citations:prepared.citations,events:events.map(event=>event.tool?{...event,tool:{...event.tool,callId:"<call>"}}:event),answer:r.requests}
}
test("workspace chat: planner rounds, tool results, fallback and final request",async()=>{
  const call=(n:number,name:string,args:unknown):ChatToolCall=>({id:`call_${n}`,name,arguments:JSON.stringify(args)})
  const seed=[pricing,blank,long,other]
  check("workspace-chat/planned",await workspace({message:"What did we agree about the annual discount and the rollback rehearsal?",history,includeSources:true,timeZone:"Europe/Stockholm",model:"openrouter:openai/gpt-6-sol"},[
    [call(1,"search_meetings",{query:"annual discount",limit:5})],
    [call(2,"search_transcripts",{query:"rollback rehearsal",from:"2026-02-01",to:"2026-03-10"}),call(3,"get_meetings",{noteIds:[pricing.id]})],
    [call(4,"get_meeting_transcript",{noteId:long.id,startMs:1020000})],
  ],seed))
  check("workspace-chat/recent-todos",await workspace({message:"List my recent to-dos",history:[],includeSources:true},[
    [call(1,"list_meetings",{limit:3})],
    [call(2,"get_meetings",{noteIds:[other.id,pricing.id,blank.id]})],
  ],seed))
  check("workspace-chat/invalid-and-foreign-calls",await workspace({message:"Read the importer note",history:[],includeSources:true},[
    [call(1,"get_meetings",{noteIds:[id(77)]}),call(2,"search_meetings",{query:"importer",limit:99})],
    [call(3,"search_meetings",{query:"importer"})],
  ],seed))
  check("workspace-chat/fallback-without-planner",await workspace({message:"When does the importer ship?",history:[]},null,seed))
  check("workspace-chat/fallback-no-match",await workspace({message:"zzzz qqqq",history:[],includeSources:true},null,seed))
})

/** A note transcribed live: voices are numbered once per channel (`<channel>:n:<number>`), not per audio part. */
const liveSeg=(n:number,channel:"mic"|"system",speaker:number,text:string,startMs:number)=>({id:id(2000+n),channel,speakerId:`${channel}:n:${speaker}`,speakerLabel:`Speaker ${speaker}`,text,isFinal:true,startMs,endMs:startMs+4000})
const liveNote=note(5,{title:"Vendor review",duration:"0:31",transcriptSegments:[
  liveSeg(1,"system",1,"The Skerryvore vendor raised prices by eleven percent.",1000),
  liveSeg(2,"system",2,"We can absorb that this year. Next year we should evaluate the Altamont alternative.",7000),
  liveSeg(3,"mic",1,"Agreed. Put the evaluation on the roadmap and I will tell finance by Friday.",15000),
  liveSeg(4,"system",1,"Done. The offsite is at the Thornaby boathouse on the ninth of June.",22000),
]})
test("live-origin transcripts: speaker rule in the base prompt, voice tags, and the unfinished tail in live chat",async()=>{
  const r=recorder([liveNote.transcriptSegments[0]!.id]),providers=makeProviderServices({openRouterKey:"fixture-key",languageModel:"openai/gpt-6-sol",fetch:r.transport})
  await Effect.runPromise(providers.generate(liveNote))
  await Effect.runPromise(providers.evidence!(liveNote,"The vendor raised prices by 11%."))
  await Effect.runPromise(providers.chat(liveNote,{message:"Who raised prices?",history:[]}))
  check("live-origin/generation+evidence+chat",r.requests)
  // Chat during the recording, with a live session holding one unfinished utterance.
  const t=recorder(),repository=makeMemoryRepository([{...liveNote,recordingState:"recording"}],clock)
  const hub={size:1,full:()=>false,connect:async()=>{throw new Error("not used")},active:()=>true,pendingRange:()=>null,flush:async()=>{},end:async()=>{},dispose:async()=>{},
    snapshot:()=>({asOfMs:31_000,state:"live" as const,tail:[{channel:"system" as const,text:"Thirty two people confirmed and catering needs the final count by",startMs:27_500,speakerId:"system:n:2"}]})}
  const processing=makeProcessingService(repository,{worker:false,clock,live:hub,providers:makeProviderServices({openRouterKey:"fixture-key",languageModel:"openai/gpt-6-sol",fetch:t.transport})})
  const events:ChatStreamEventDTO[]=[]
  await Effect.runPromise(Stream.runForEach(await Effect.runPromise(processing.chatStream(liveNote.id,{message:"How many people confirmed?",history:[],includeSources:true})),event=>Effect.sync(()=>{events.push(event)})))
  await processing.dispose()
  check("live-origin/live-chat/stream/unfinished-tail",{requests:t.requests,events:events.map(event=>event.tool?{...event,tool:{...event.tool,callId:"<call>"}}:event)})
})

/** A chat thread on a meeting in progress, turn by turn: the transcript grows, a sentence is in progress at the first
 * question, and each request must be the previous one plus the answer plus one message. `only` keeps just the last
 * request of a provider variant, which is where its cache fields show. */
async function liveThread(options:Parameters<typeof makeProviderServices>[0],only?:"last") {
  const r=recorder(),lines=liveNote.transcriptSegments
  const repository=makeMemoryRepository([{...liveNote,recordingState:"recording",body:"- Skerryvore +11%",transcriptSegments:lines.slice(0,2)}],clock)
  let tail:Array<{channel:"mic"|"system";text:string;startMs:number;speakerId:string}>=[{channel:"mic",text:"Agreed. Put the evaluation on",startMs:15_000,speakerId:"mic:n:1"}]
  const hub={size:1,full:()=>false,connect:async()=>{throw new Error("not used")},active:()=>true,pendingRange:()=>null,flush:async()=>{},end:async()=>{},dispose:async()=>{},snapshot:()=>({asOfMs:17_000,state:"live" as const,tail})}
  const processing=makeProcessingService(repository,{worker:false,clock,live:hub,liveConversations:makeLiveConversationStore(),providers:makeProviderServices({openRouterKey:"fixture-key",...options,fetch:r.transport})})
  const thread:Array<ChatInput["history"][number]>=[]
  for(const [question,shown,speaking,notes] of [["What happened to prices?",2,true,"- Skerryvore +11%"],["Who tells finance?",3,false,"- Skerryvore +11%"],["Where is the offsite?",4,false,"- Skerryvore +11%\n- offsite 9 June"]] as const){
    if(!speaking)tail=[]
    await Effect.runPromise(repository.updateNote(liveNote.id,{transcriptSegments:lines.slice(0,shown),body:notes}))
    let answer=""
    await Effect.runPromise(Stream.runForEach(await Effect.runPromise(processing.chatStream(liveNote.id,{message:question,history:[...thread],includeSources:true,timeZone:"Europe/Stockholm"})),event=>Effect.sync(()=>{if(event.type==="delta")answer+=event.text})))
    thread.push({role:"user",content:question},{role:"assistant",content:answer})
  }
  await processing.dispose()
  return only?r.requests.slice(-1):r.requests
}
test("live chat thread: three turns at the wire, and the cache fields each provider gets",async()=>{
  check("live-chat/thread/openrouter-gpt-6-sol",await liveThread({languageModel:"openai/gpt-6-sol"}))
  check("live-chat/thread/openrouter-anthropic/turn-3",await liveThread({languageModel:"anthropic/claude-sonnet-5.5"},"last"))
  check("live-chat/thread/anthropic-direct/turn-3",await liveThread({languageProvider:"anthropic",anthropicKey:"fixture-key",languageModel:"claude-fixture-model"},"last"))
  check("live-chat/thread/openai-direct/turn-3",await liveThread({languageProvider:"openai",openAiKey:"fixture-key",languageModel:"gpt-6-sol"},"last"))
  // A finished note on an Anthropic model: breakpoints on the instructions and on the record, automatic caching for the thread.
  const r=recorder(),repository=makeMemoryRepository([pricing],clock)
  const processing=makeProcessingService(repository,{worker:false,clock,providers:makeProviderServices({openRouterKey:"fixture-key",languageModel:"anthropic/claude-sonnet-5.5",fetch:r.transport})})
  await Effect.runPromise(Stream.runDrain(await Effect.runPromise(processing.chatStream(pricing.id,{message:"And what did she ask for?",history,includeSources:true,timeZone:"Europe/Stockholm"}))))
  await processing.dispose()
  check("note-chat/stream/openrouter-anthropic/follow-up",r.requests)
})

test("golden file is current",()=>{
  if(update){mkdirSync(resolve(import.meta.dir,"golden"),{recursive:true});writeFileSync(goldenPath,JSON.stringify(captured,null,1)+"\n");return}
  expect(Object.keys(captured).sort()).toEqual(Object.keys(golden).sort())
})
