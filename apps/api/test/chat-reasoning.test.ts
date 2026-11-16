/** The model's own reasoning in chat answers: how each provider's reasoning is read (never written here), the
 * order it is streamed in, how it is stored with an answer, and that the chat model a question names is the one
 * asked. No network: every provider here is a function. */
import {expect,test} from "bun:test"
import {Effect} from "effect"
import type {ChatStreamEventDTO} from "@meeting-notes/contracts"
import {makeApiHandler} from "../src/api"
import {makeMemoryRepository} from "../src/repository"
import {makeMemoryCalendarStore} from "../src/calendar-store"
import {makeAdministrationService,settingsAwareProviders} from "../src/administration"
import {makeProcessingService,MAX_REASONING_CHARACTERS} from "../src/processing"
import {makeChatsService,makeMemoryChatStore} from "../src/chats"
import {makeProviderServices,ProviderFailure,type ChatToolDefinition,type ChatToolMessage,type ProviderServices} from "../src/providers"
import {responsesBody} from "../src/language-transport"

const run=Effect.runPromise
type Seen={url:string;body:any;headers:Headers}
function server(answer:(seen:Seen)=>Response|Promise<Response>){
 const seen:Seen[]=[]
 const transport=Object.assign(async(input:any,init?:RequestInit)=>{const entry:Seen={url:String(input),body:typeof init?.body==="string"?JSON.parse(init.body):undefined,headers:new Headers(init?.headers)};seen.push(entry);return answer(entry)},{preconnect:()=>{}}) as typeof fetch
 return {seen,transport}
}
const sse=(...frames:unknown[])=>new Response(frames.map(frame=>`data: ${typeof frame==="string"?frame:JSON.stringify(frame)}\n\n`).join(""),{headers:{"Content-Type":"text/event-stream"}})
const signal=()=>new AbortController().signal
const stamp="2026-03-09T10:00:00.000Z"
const note={id:"00000000-0000-4000-8000-000000000001",title:"Budget review",body:"We agreed the budget.",enhanced:"",date:"2026-03-09",duration:"",createdAt:stamp,updatedAt:stamp,transcriptSegments:[],recordingState:"stopped" as const,generationState:"idle" as const,transcriptRevision:0,generatedRevision:null}
const question={message:"What was agreed?",history:[]}
const TOOLS:ChatToolDefinition[]=[{type:"function",function:{name:"search",description:"Search",parameters:{type:"object",properties:{query:{type:"string"},limit:{type:"integer"}},required:["query"]}}}]
const LOOP:ChatToolMessage[]=[{role:"system",content:"Find notes"},{role:"user",content:"Question"},{role:"assistant",content:null,tool_calls:[{id:"call_previous",type:"function",function:{name:"search",arguments:'{"query":"budget"}'}}]},{role:"tool",tool_call_id:"call_previous",content:"Result"}]
/** A streamed answer read to its end: the answer text and the reasoning pieces, in the order they were handed over. */
async function answer(providers:ProviderServices){
 const reasoning:string[]=[];let text=""
 for await(const piece of providers.chatStream!(note,question,signal(),part=>reasoning.push(part)))text+=piece
 return {text,reasoning}
}
async function step(providers:ProviderServices){
 const reasoning:string[]=[],calls=await providers.chatToolStep!(LOOP,TOOLS,signal(),undefined,part=>reasoning.push(part))
 return {calls,reasoning}
}

test("OpenRouter: reasoning comes as delta.reasoning in a streamed answer and message.reasoning in a research step, which asks a reasoning model for low effort",async()=>{
 const s=server(seen=>seen.body.stream
  ?sse({choices:[{delta:{reasoning:"**Checking** the"}}]},{choices:[{delta:{reasoning:" notes."}}]},{choices:[{delta:{content:"Agreed"}}]},{choices:[{delta:{content:" in full.",reasoning:""}}]},"[DONE]")
  :Response.json({choices:[{finish_reason:"tool_calls",message:{content:null,reasoning:"  The notes should say.\n",tool_calls:[{id:"call_1",type:"function",function:{name:"search",arguments:'{"query":"budget"}'}}]}}]}))
 const providers=makeProviderServices({openRouterKey:"router-key",languageModel:"openai/gpt-6-sol",fetch:s.transport,onLanguageUsage:()=>{}})
 expect(await answer(providers)).toEqual({text:"Agreed in full.",reasoning:["**Checking** the"," notes."]})
 expect(await step(providers)).toEqual({calls:[{id:"call_1",name:"search",arguments:'{"query":"budget"}'}],reasoning:["The notes should say."]})
 // The answer keeps the effort it had; only the research step is asked to reason, with room to do it.
 expect(s.seen.map(r=>[r.url,r.body.reasoning_effort,r.body.max_completion_tokens])).toEqual([["https://openrouter.ai/api/v1/chat/completions","none",4096],["https://openrouter.ai/api/v1/chat/completions","low",4000]])
 // A model that does not reason is asked exactly as before.
 const plain=server(()=>Response.json({choices:[{message:{content:"READY"}}]}))
 expect(await step(makeProviderServices({openRouterKey:"router-key",languageModel:"vendor/chat-a",fetch:plain.transport}))).toEqual({calls:[],reasoning:[]})
 expect(plain.seen[0]!.body).toMatchObject({max_tokens:1200,temperature:0});expect(plain.seen[0]!.body.reasoning_effort).toBeUndefined()
})

test("an OpenAI-compatible server: reasoning_content is read the same way, and the request gains no field",async()=>{
 const s=server(seen=>seen.body.stream
  ?sse({choices:[{delta:{reasoning_content:"Thinking it"}}]},{choices:[{delta:{reasoning_content:" over."}}]},{choices:[{delta:{content:"Agreed."},finish_reason:"stop"}]})
  :Response.json({choices:[{message:{content:"",reasoning_content:"Search first.",tool_calls:[{id:"c1",type:"function",function:{name:"search",arguments:"{}"}}]}}]}))
 const providers=makeProviderServices({languageProvider:"openai_compatible",openAiCompatibleBaseUrl:"http://localhost:11434/v1",languageModel:"gpt-5-local",fetch:s.transport,onLanguageUsage:()=>{}})
 expect(await answer(providers)).toEqual({text:"Agreed.",reasoning:["Thinking it"," over."]})
 expect((await step(providers)).reasoning).toEqual(["Search first."])
 for(const request of s.seen){
  expect(request.url).toBe("http://localhost:11434/v1/chat/completions")
  expect(Object.keys(request.body).filter(field=>!["model","messages","stream","max_tokens","temperature","response_format","tools","tool_choice"].includes(field))).toEqual([])
 }
})

test("a provider that sends no reasoning gives none: nothing is made up",async()=>{
 const s=server(seen=>seen.body.stream?sse({choices:[{delta:{content:"Agreed."}}]},"[DONE]"):Response.json({choices:[{message:{content:null,tool_calls:[{id:"c1",type:"function",function:{name:"search",arguments:"{}"}}]}}]}))
 const providers=makeProviderServices({openRouterKey:"router-key",languageModel:"vendor/chat-a",fetch:s.transport,onLanguageUsage:()=>{}})
 expect(await answer(providers)).toEqual({text:"Agreed.",reasoning:[]})
 expect((await step(providers)).reasoning).toEqual([])
})

const event=(name:string,data:Record<string,unknown>)=>`event: ${name}\ndata: ${JSON.stringify({type:name,...data})}\n\n`
const responsesAnswer=()=>new Response(
 event("response.created",{response:{id:"resp_1"}})+
 event("response.reasoning_summary_part.added",{summary_index:0})+event("response.reasoning_summary_text.delta",{delta:"**Reading the notes**"})+event("response.reasoning_summary_text.delta",{delta:"\n\nThe budget is there."})+
 event("response.reasoning_summary_part.added",{summary_index:1})+event("response.reasoning_summary_text.delta",{delta:"**Answering**"})+
 event("response.output_text.delta",{delta:"Agreed"})+event("response.output_text.delta",{delta:" in full."})+
 event("response.completed",{response:{status:"completed",usage:{input_tokens:900,input_tokens_details:{cached_tokens:600},output_tokens:40}}}),{headers:{"Content-Type":"text/event-stream"}})

test("OpenAI directly: a reasoning model's answer and research step go through the Responses API, and come back as chat completions with the summary as reasoning",async()=>{
 const usage:unknown[]=[]
 const s=server(seen=>seen.body.stream?responsesAnswer():Response.json({status:"completed",usage:{input_tokens:50,output_tokens:9},output:[
  {type:"reasoning",id:"rs_1",summary:[{type:"summary_text",text:"**Planning**"},{type:"summary_text",text:"Search the budget notes."}]},
  {type:"function_call",id:"fc_1",call_id:"call_9",name:"search",arguments:'{"query":"budget"}'}]}))
 const providers=makeProviderServices({languageProvider:"openai",openAiKey:"sk-responses-stream",languageModel:"gpt-5.2",fetch:s.transport,onLanguageUsage:value=>usage.push(value.usage)})
 // Separate parts of a summary are a paragraph apart.
 expect(await answer(providers)).toEqual({text:"Agreed in full.",reasoning:["**Reading the notes**","\n\nThe budget is there.","\n\n**Answering**"]})
 expect(usage).toEqual([{promptTokens:900,cachedTokens:600,cacheWriteTokens:0,outputTokens:40,costUsd:null}])
 expect(await step(providers)).toEqual({calls:[{id:"call_9",name:"search",arguments:'{"query":"budget"}'}],reasoning:["**Planning**\n\nSearch the budget notes."]})
 const [streamed,planned]=s.seen
 expect(streamed!.url).toBe("https://api.openai.com/v1/responses");expect(streamed!.headers.get("authorization")).toBe("Bearer sk-responses-stream")
 expect(streamed!.body).toMatchObject({model:"gpt-5.2",stream:true,store:false,reasoning:{summary:"auto"},max_output_tokens:4096})
 expect(streamed!.body.input[0].role).toBe("system");expect(streamed!.body.input.at(-1)).toEqual({role:"user",content:"What was agreed?"})
 for(const field of ["messages","max_completion_tokens","stream_options","reasoning_effort"])expect(streamed!.body[field]).toBeUndefined()
 // The research step: low effort, the tools as Responses functions, and the earlier call and its result as items without a response id.
 expect(planned!.url).toBe("https://api.openai.com/v1/responses")
 expect(planned!.body).toMatchObject({stream:false,store:false,reasoning:{summary:"auto",effort:"low"},max_output_tokens:4000,tool_choice:"auto",parallel_tool_calls:false,
  tools:[{type:"function",name:"search",description:"Search",parameters:TOOLS[0]!.function.parameters,strict:false}]})
 expect(planned!.body.input).toEqual([{role:"system",content:"Find notes"},{role:"user",content:"Question"},
  {type:"function_call",call_id:"call_previous",name:"search",arguments:'{"query":"budget"}'},{type:"function_call_output",call_id:"call_previous",output:"Result"}])
})

test("OpenAI Responses request: cache breakpoints stay on input text, JSON mode and the cache fields are carried over",()=>{
 const body=responsesBody({model:"gpt-5.6",stream:true,max_completion_tokens:800,reasoning_effort:"none",response_format:{type:"json_object"},prompt_cache_options:{mode:"explicit"},prompt_cache_key:"thread-1",
  messages:[{role:"system",content:[{type:"text",text:"Rules",prompt_cache_breakpoint:{mode:"explicit"}}]},{role:"user",content:"Q1"},{role:"assistant",content:"A1"},{role:"user",content:"Q2"}]})
 expect(body).toEqual({model:"gpt-5.6",stream:true,store:false,reasoning:{summary:"auto",effort:"none"},max_output_tokens:800,text:{format:{type:"json_object"}},prompt_cache_key:"thread-1",prompt_cache_options:{mode:"explicit"},
  input:[{role:"system",content:[{type:"input_text",text:"Rules",prompt_cache_breakpoint:{mode:"explicit"}}]},{role:"user",content:"Q1"},{role:"assistant",content:"A1"},{role:"user",content:"Q2"}]})
})

test("OpenAI directly: a refused Responses request is sent once more as chat completions, and the refusal is remembered; other requests never use Responses",async()=>{
 const completions=(seen:Seen)=>seen.body.stream?sse({choices:[{delta:{content:"Plain answer."}}]},{choices:[],usage:{prompt_tokens:5,completion_tokens:2}},"[DONE]"):Response.json({choices:[{finish_reason:"stop",message:{content:"Generated notes"}}]})
 const s=server(seen=>seen.url.endsWith("/responses")?Response.json({error:{message:"Your organization must be verified to generate reasoning summaries."}},{status:400}):completions(seen))
 const providers=makeProviderServices({languageProvider:"openai",openAiKey:"sk-responses-refused",languageModel:"gpt-5.2",fetch:s.transport,onLanguageUsage:()=>{}})
 expect(await answer(providers)).toEqual({text:"Plain answer.",reasoning:[]})
 expect(s.seen.map(r=>r.url)).toEqual(["https://api.openai.com/v1/responses","https://api.openai.com/v1/chat/completions"])
 // The retry is the request exactly as it was before reasoning was asked for.
 expect(s.seen[1]!.body).toMatchObject({model:"gpt-5.2",stream:true,stream_options:{include_usage:true},max_completion_tokens:4096});expect(s.seen[1]!.body.messages.at(-1)).toEqual({role:"user",content:"What was agreed?"})
 // The next question does not ask in vain again.
 expect((await answer(providers)).text).toBe("Plain answer.");expect(s.seen.map(r=>r.url).slice(2)).toEqual(["https://api.openai.com/v1/chat/completions"])
 // A busy or failing Responses API is also retried the old way, without being remembered.
 let failures=0
 const busy=server(seen=>seen.url.endsWith("/responses")?(failures++,new Response("busy",{status:503})):completions(seen))
 const second=makeProviderServices({languageProvider:"openai",openAiKey:"sk-responses-busy",languageModel:"gpt-5.2",fetch:busy.transport,onLanguageUsage:()=>{}})
 await answer(second);await answer(second);expect(failures).toBe(2)
 // Notes are generated through chat completions, whatever the model; so is chat on a model that does not reason.
 const other=server(completions)
 expect(await run(makeProviderServices({languageProvider:"openai",openAiKey:"sk-responses-other",languageModel:"gpt-5.2",fetch:other.transport}).generate(note))).toBe("Generated notes")
 expect((await answer(makeProviderServices({languageProvider:"openai",openAiKey:"sk-responses-other",languageModel:"gpt-4.1",fetch:other.transport,onLanguageUsage:()=>{}}))).text).toBe("Plain answer.")
 expect(other.seen.map(r=>r.url)).toEqual(["https://api.openai.com/v1/chat/completions","https://api.openai.com/v1/chat/completions"])
})

test("OpenAI Responses: an answer cut off at its budget, a failed one, and a stream that stops early are failures, not answers",async()=>{
 const endings=[event("response.incomplete",{response:{status:"incomplete",incomplete_details:{reason:"max_output_tokens"}}}),event("response.failed",{response:{status:"failed"}}),event("error",{message:"upstream"}),""]
 for(const [index,ending] of endings.entries()){
  const s=server(()=>new Response(event("response.output_text.delta",{delta:"Partial"})+ending))
  const providers=makeProviderServices({languageProvider:"openai",openAiKey:`sk-responses-ending-${index}`,languageModel:"o3",fetch:s.transport,onLanguageUsage:()=>{}})
  await expect(answer(providers)).rejects.toBeInstanceOf(ProviderFailure)
 }
 // A research step cut off before it called anything is a failed step, not a decision to look nothing up.
 const cut=server(()=>Response.json({status:"incomplete",incomplete_details:{reason:"max_output_tokens"},output:[{type:"reasoning",summary:[{type:"summary_text",text:"Still thinking"}]}]}))
 await expect(step(makeProviderServices({languageProvider:"openai",openAiKey:"sk-responses-cut",languageModel:"o3",fetch:cut.transport}))).rejects.toBeInstanceOf(ProviderFailure)
})

const anthropicAnswer=()=>new Response(
 event("message_start",{message:{usage:{input_tokens:10}}})+
 event("content_block_start",{index:0,content_block:{type:"thinking",thinking:""}})+event("content_block_delta",{index:0,delta:{type:"thinking_delta",thinking:"The notes record"}})+event("content_block_delta",{index:0,delta:{type:"thinking_delta",thinking:" the decision."}})+
 event("content_block_delta",{index:0,delta:{type:"signature_delta",signature:"sig"}})+
 event("content_block_start",{index:1,content_block:{type:"thinking",thinking:""}})+event("content_block_delta",{index:1,delta:{type:"thinking_delta",thinking:"Answer briefly."}})+
 event("content_block_start",{index:2,content_block:{type:"text",text:""}})+event("content_block_delta",{index:2,delta:{type:"text_delta",text:"Agreed."}})+
 event("message_delta",{delta:{stop_reason:"end_turn"},usage:{output_tokens:12}})+event("message_stop",{}))

test("Anthropic: thinking is asked for only in the streamed answer, in the form the model takes, and thinking_delta text comes back as reasoning",async()=>{
 const s=server(seen=>seen.body.stream?anthropicAnswer():Response.json({stop_reason:"tool_use",content:[{type:"thinking",thinking:"Search first.",signature:"sig"},{type:"tool_use",id:"toolu_1",name:"search",input:{query:"budget"}}]}))
 const make=(model:string,key="anthropic-key")=>makeProviderServices({languageProvider:"anthropic",anthropicKey:key,languageModel:model,fetch:s.transport,onLanguageUsage:()=>{}})
 expect(await answer(make("claude-opus-4-8"))).toEqual({text:"Agreed.",reasoning:["The notes record"," the decision.","\n\nAnswer briefly."]})
 // Adaptive thinking with a readable summary, and room for it on top of the answer's own budget.
 expect(s.seen[0]!.body).toMatchObject({model:"claude-opus-4-8",stream:true,thinking:{type:"adaptive",display:"summarized"},max_tokens:4096+8000})
 expect(s.seen[0]!.body.temperature).toBeUndefined()
 // The models before 4.6 take a fixed budget, below max_tokens; a model that is not known to think is asked nothing.
 await answer(make("claude-haiku-4-5"));expect(s.seen[1]!.body).toMatchObject({thinking:{type:"enabled",budget_tokens:2048},max_tokens:4096+2048})
 await answer(make("claude-test"));expect(s.seen[2]!.body.thinking).toBeUndefined();expect(s.seen[2]!.body.max_tokens).toBe(4096)
 // The research loop never asks for thinking (its blocks would have to be handed back); a thinking block that comes anyway is read.
 expect(await step(make("claude-opus-4-8"))).toEqual({calls:[{id:"toolu_1",name:"search",arguments:'{"query":"budget"}'}],reasoning:["Search first."]})
 expect(s.seen[3]!.body.thinking).toBeUndefined();expect(s.seen[3]!.body.max_tokens).toBe(1200)
 // Notes are generated as before.
 const plain=server(()=>Response.json({stop_reason:"end_turn",content:[{type:"text",text:"Generated notes"}]}))
 expect(await run(makeProviderServices({languageProvider:"anthropic",anthropicKey:"anthropic-key",languageModel:"claude-opus-4-8",fetch:plain.transport}).generate(note))).toBe("Generated notes")
 expect(plain.seen[0]!.body.thinking).toBeUndefined()
})

test("Anthropic: a model that refuses the thinking field is asked once more without it, and not asked that way again for a while",async()=>{
 const s=server(seen=>seen.body.thinking?Response.json({type:"error",error:{type:"invalid_request_error",message:"thinking is not supported"}},{status:400})
  :new Response(event("content_block_delta",{delta:{type:"text_delta",text:"Agreed."}})+event("message_stop",{})))
 const providers=makeProviderServices({languageProvider:"anthropic",anthropicKey:"anthropic-key-refused",languageModel:"claude-sonnet-5",fetch:s.transport,onLanguageUsage:()=>{}})
 expect(await answer(providers)).toEqual({text:"Agreed.",reasoning:[]})
 expect(s.seen.map(r=>[!!r.body.thinking,r.body.max_tokens])).toEqual([[true,4096+8000],[false,4096]])
 expect((await answer(providers)).text).toBe("Agreed.");expect(s.seen).toHaveLength(3);expect(s.seen[2]!.body.thinking).toBeUndefined()
})

// ---------------------------------------------------------------- The stream and what is stored

const token="chat-reasoning-operator-token-at-least-32-characters"
const frames=(text:string)=>text.split("\n\n").filter(frame=>frame.startsWith("data:")).map(frame=>JSON.parse(frame.slice(5)) as ChatStreamEventDTO)
function app(providers:ProviderServices){
 const repository=makeMemoryRepository(),processing=makeProcessingService(repository,{providers})
 const api=makeApiHandler(repository,token,{processing,chats:makeChatsService(makeMemoryChatStore(repository))})
 const call=(path:string,method="GET",body?:unknown)=>api.handler(new Request(`http://localhost/api/v1${path}`,{method,headers:{Authorization:`Bearer ${token}`,...(body===undefined?{}:{"Content-Type":"application/json"})},...(body===undefined?{}:{body:JSON.stringify(body)})}))
 const json=async(path:string,method="GET",body?:unknown)=>{const response=await call(path,method,body);return {status:response.status,body:await response.json() as any}}
 return {api,call,json,repository}
}
const base:Pick<ProviderServices,"generate"|"chat">={generate:()=>Effect.succeed(""),chat:()=>Effect.succeed("")}

test("reasoning is streamed in its place among the tool steps and stored as stretches that say where they fall",async()=>{
 let round=0
 const f=app({...base,
  async chatToolStep(_messages,_tools,_signal,_model,onReasoning){
   if(round++===0){onReasoning?.("**Plan**\n\nRead the budget meeting.");return [{id:"call_read",name:"get_meetings",arguments:JSON.stringify({ids:["m1"]})}]}
   onReasoning?.("That is enough.");return []
  },
  async *chatStream(_note,_input,_signal,onReasoning){onReasoning?.(" Now the");onReasoning?.(" answer.");yield "The budget ";yield "was agreed."},
 })
 try{
  await f.json("/notes","POST",{title:"Windrush budget review",body:"We agreed the Windrush budget."})
  const thread=(await f.json("/chats","POST",{})).body.id as string
  const events=frames(await (await f.call(`/chats/${thread}/stream`,"POST",{turn:0,message:"What did we decide about the Windrush budget?",includeSources:true})).text())
  // The search made from the question (which reads its best match), then each round's reasoning before the lookups it asked for, and the answer's reasoning before its text.
  expect(events.map(e=>e.type==="tool"?`tool:${e.tool!.name}:${e.tool!.status}`:e.type==="reasoning"?`reasoning:${e.text}`:e.type)).toEqual([
   "tool:search_meetings:running","tool:search_meetings:completed","tool:get_meetings:running","tool:get_meetings:completed",
   "reasoning:**Plan**\n\nRead the budget meeting.","tool:get_meetings:running","tool:get_meetings:completed",
   "reasoning:That is enough.","reasoning: Now the","reasoning: answer.","delta","delta","citations","done"])
  const stored=(await f.json(`/chats/${thread}`)).body.messages
  expect(stored[0].reasoning).toEqual([])
  expect(stored[1]).toMatchObject({status:"complete",content:"The budget was agreed."})
  expect(stored[1].tools.map((tool:any)=>tool.name)).toEqual(["search_meetings","get_meetings","get_meetings"])
  // Pieces join until a new tool step starts; `afterTools` is how many steps had started when the stretch began.
  expect(stored[1].reasoning).toEqual([{text:"**Plan**\n\nRead the budget meeting.",afterTools:2},{text:"That is enough. Now the answer.",afterTools:3}])
  // The plain stream carries the same events, without storing anything.
  round=0
  const direct=frames(await (await f.call("/chat/stream","POST",{message:"What did we decide about the Windrush budget?",history:[],includeSources:true})).text())
  expect(direct.filter(e=>e.type==="reasoning").map(e=>e.text)).toEqual(["**Plan**\n\nRead the budget meeting.","That is enough."," Now the"," answer."])
 }finally{await f.api.dispose()}
})

test("a provider without reasoning: no reasoning events, and the stored answer has an empty list",async()=>{
 const f=app({...base,async chatToolStep(){return []},async *chatStream(){yield "Nothing recorded."}})
 try{
  const thread=(await f.json("/chats","POST",{})).body.id as string
  const events=frames(await (await f.call(`/chats/${thread}/stream`,"POST",{turn:0,message:"Anything about budgets?",includeSources:true})).text())
  expect(events.some(e=>e.type==="reasoning")).toBe(false);expect(events.at(-1)!.type).toBe("done")
  expect((await f.json(`/chats/${thread}`)).body.messages.map((m:any)=>m.reasoning)).toEqual([[],[]])
 }finally{await f.api.dispose()}
})

test("stored reasoning is bounded: 20,000 characters a stretch, and the stream stops forwarding at what an answer can hold",async()=>{
 const piece="r".repeat(150_000)
 const f=app({...base,async *chatStream(_note,_input,_signal,onReasoning){for(let i=0;i<7;i++)onReasoning?.(piece);yield "Done."}})
 try{
  const created=await f.json("/notes","POST",{title:"Budget"}),thread=(await f.json("/chats","POST",{noteId:created.body.id})).body.id as string
  const events=frames(await (await f.call(`/chats/${thread}/stream`,"POST",{turn:0,message:"Q"})).text())
  expect(events.filter(e=>e.type==="reasoning").reduce((sum,e)=>sum+e.text.length,0)).toBe(MAX_REASONING_CHARACTERS)
  const stored=(await f.json(`/chats/${thread}`)).body.messages[1]
  expect(stored.reasoning).toEqual([{text:"r".repeat(20_000),afterTools:0}])
 }finally{await f.api.dispose()}
})

test("a finished exchange can be stored with its reasoning, which is checked like the rest",async()=>{
 const f=app({...base})
 try{
  const thread=(await f.json("/chats","POST",{})).body.id as string
  const reasoning=[{text:"**Plan**\n\nSearch.",afterTools:0},{text:"Answer from the notes.",afterTools:2}]
  const stored=await f.json(`/chats/${thread}/turns`,"POST",{turn:0,question:"Q",answer:{content:"A",status:"complete",reasoning}})
  expect(stored.status).toBe(200);expect(stored.body.messages[1].reasoning).toEqual(reasoning)
  expect((await f.json(`/chats/${thread}`)).body.messages[1].reasoning).toEqual(reasoning)
  expect((await f.call(`/chats/${thread}/turns`,"POST",{turn:1,question:"Q",answer:{content:"A",status:"complete",reasoning:[{text:"",afterTools:0}]}})).status).toBe(400)
  expect((await f.call(`/chats/${thread}/turns`,"POST",{turn:1,question:"Q",answer:{content:"A",status:"complete",reasoning:Array.from({length:41},()=>({text:"x",afterTools:0}))}})).status).toBe(400)
 }finally{await f.api.dispose()}
})

// ---------------------------------------------------------------- Which chat model answers

const routerCatalog={data:["vendor/chat-a","vendor/chat-b","vendor/chat-c"].map(id=>({id,name:id,architecture:{input_modalities:["text"],output_modalities:["text"]}}))}
async function modelApp(){
 const s=server(seen=>seen.url==="https://openrouter.ai/api/v1/models"?Response.json(routerCatalog)
  :seen.body?.stream?sse({choices:[{delta:{content:`Answer from ${seen.body.model}`}}]},"[DONE]")
  :seen.body?.tools?Response.json({choices:[{message:{content:"READY"}}]}):Response.json({choices:[{message:{content:"Title"}}]}))
 const repository=makeMemoryRepository(),store=makeMemoryCalendarStore(repository),admin=makeAdministrationService(store,undefined,{fetch:s.transport,environment:{OPENROUTER_API_KEY:"environment-router-key"}})
 await run(admin.updateSettings({chatModels:[{provider:"openrouter",model:"vendor/chat-b"},{provider:"openrouter",model:"vendor/chat-a"}],generationModel:{provider:"openrouter",model:"vendor/chat-c"}}))
 const providers=settingsAwareProviders(admin,s.transport),processing=makeProcessingService(repository,{providers})
 const api=makeApiHandler(repository,token,{processing,administration:admin,chats:makeChatsService(makeMemoryChatStore(repository))})
 const call=(path:string,method="GET",body?:unknown)=>api.handler(new Request(`http://localhost/api/v1${path}`,{method,headers:{Authorization:`Bearer ${token}`,...(body===undefined?{}:{"Content-Type":"application/json"})},...(body===undefined?{}:{body:JSON.stringify(body)})}))
 const json=async(path:string,method="GET",body?:unknown)=>{const response=await call(path,method,body);return {status:response.status,body:await response.json() as any}}
 /** The chat requests sent since the last look: [research step or answer, model]. */
 let looked=0
 const asked=()=>{const requests=s.seen.slice(looked).filter(r=>r.url.endsWith("/chat/completions")&&(r.body.stream||r.body.tools)).map(r=>[r.body.stream?"answer":"research",r.body.model]);looked=s.seen.length;return requests}
 return {api,call,json,asked,seen:s.seen}
}

test("the chat models are offered in the administrator's order, the default first",async()=>{
 const f=await modelApp()
 try{
  expect((await f.json("/models/chat")).body).toEqual({models:[{provider:"openrouter",model:"vendor/chat-b"},{provider:"openrouter",model:"vendor/chat-a"}]})
 }finally{await f.api.dispose()}
})

test("the model a question names is the one asked, for the research step and for the answer; without one, the first enabled model",async()=>{
 const f=await modelApp()
 try{
  f.asked()
  const ask=async(path:string,body:Record<string,unknown>)=>{const events=frames(await (await f.call(path,"POST",body)).text());expect(events.at(-1)!.type).toBe("done");return events.filter(e=>e.type==="delta").map(e=>e.text).join("")}
  // Across all notes, without a thread.
  expect(await ask("/chat/stream",{message:"Anything about budgets?",history:[],model:"openrouter:vendor/chat-a"})).toBe("Answer from vendor/chat-a")
  expect(f.asked()).toEqual([["research","vendor/chat-a"],["answer","vendor/chat-a"]])
  expect(await ask("/chat/stream",{message:"Anything about budgets?",history:[]})).toBe("Answer from vendor/chat-b")
  expect(f.asked()).toEqual([["research","vendor/chat-b"],["answer","vendor/chat-b"]])
  // In a saved thread: the same, and the answer records the model it was asked of.
  const thread=(await f.json("/chats","POST",{})).body.id as string
  expect(await ask(`/chats/${thread}/stream`,{turn:0,message:"Anything about budgets?",model:"openrouter:vendor/chat-a"})).toBe("Answer from vendor/chat-a")
  expect(f.asked()).toEqual([["research","vendor/chat-a"],["answer","vendor/chat-a"]])
  expect(await ask(`/chats/${thread}/stream`,{turn:1,message:"And about hiring?"})).toBe("Answer from vendor/chat-b")
  expect(f.asked()).toEqual([["research","vendor/chat-b"],["answer","vendor/chat-b"]])
  expect((await f.json(`/chats/${thread}`)).body.messages.map((m:any)=>m.model)).toEqual(["openrouter:vendor/chat-a","openrouter:vendor/chat-a",null,null])
  // About one note: there is no research step, and the answer is asked of the named model.
  const created=await f.json("/notes","POST",{title:"Budget review",body:"We agreed the budget."})
  expect(await ask(`/notes/${created.body.id}/chat/stream`,{message:"What was agreed?",history:[],model:"openrouter:vendor/chat-a"})).toBe("Answer from vendor/chat-a")
  expect(f.asked()).toEqual([["answer","vendor/chat-a"]])
 }finally{await f.api.dispose()}
})

test("a model that is not enabled is refused with 400 before anything is asked or stored",async()=>{
 const f=await modelApp()
 try{
  f.asked()
  const refused={status:400,body:{error:"invalid_request",message:"Choose an enabled chat model"}}
  const thread=(await f.json("/chats","POST",{})).body.id as string
  // Unknown, known to the provider but not enabled (the notes model), and enabled under another provider's name.
  for(const model of ["openrouter:vendor/unknown","openrouter:vendor/chat-c","openai:vendor/chat-a","vendor/chat-a"]){
   expect(await f.json(`/chats/${thread}/stream`,"POST",{turn:0,message:"Anything about budgets?",model})).toMatchObject(refused)
   expect(await f.json("/chat/stream","POST",{message:"Anything about budgets?",history:[],model})).toMatchObject(refused)
  }
  expect(f.asked()).toEqual([])
  // The refused question took no place in the thread: nothing is stored, and the same exchange can be asked properly.
  expect((await f.json(`/chats/${thread}`)).body).toMatchObject({turnCount:0,messages:[]})
  const events=frames(await (await f.call(`/chats/${thread}/stream`,"POST",{turn:0,message:"Anything about budgets?",model:"openrouter:vendor/chat-a"})).text())
  expect(events.at(-1)!.type).toBe("done");expect((await f.json(`/chats/${thread}`)).body.turnCount).toBe(1)
 }finally{await f.api.dispose()}
})
