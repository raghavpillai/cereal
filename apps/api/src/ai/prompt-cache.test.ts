/** Prompt-cache fields per provider, and the cached-token count read back from each. No network. */
import {expect,test} from "bun:test"
import type {ChatInput,NoteDTO} from "@meeting-notes/contracts"
import {makeProviderServices,type ProviderNote} from "../providers"
import {assembleMeetingChat} from "./meeting-chat"
import {makeLiveConversationStore} from "./live-conversation"
import {cacheStyle,cachedRequest,readUsage,type LanguageUsageEvent} from "./prompt-cache"
import type {LanguageMessage} from "./shared"

const messages:LanguageMessage[]=[
  {role:"system",content:"Instructions",cache:true},
  {role:"user",content:"Transcript part 1"},
  {role:"user",content:"Transcript part 2",cache:true},
  {role:"user",content:"Rest, state, question 1"},
  {role:"assistant",content:"Answer 1"},
  {role:"user",content:"New lines, state, question 2"},
]

test("which caching a model gets",()=>{
  expect(cacheStyle("openrouter","anthropic/claude-sonnet-5.5")).toBe("anthropic")
  expect(cacheStyle("openrouter","~anthropic/claude-sonnet-latest")).toBe("anthropic")
  expect(cacheStyle("anthropic","claude-sonnet-5-5")).toBe("anthropic")
  for(const model of ["openai/gpt-6-sol","openai/gpt-6.1-sol","openai/gpt-5.6","openai/gpt-5.10-mini","openai/gpt-7"])expect(cacheStyle("openrouter",model)).toBe("openai-breakpoints")
  // Earlier OpenAI models cache prefixes by themselves and reject nothing, so they are left alone; so is everything else.
  for(const model of ["openai/gpt-5.5","openai/gpt-5.1","openai/gpt-4o-mini","google/gemini-4-hydrogen"])expect(cacheStyle("openrouter",model)).toBe("automatic")
  // OpenAI's own chat completions take the same two fields for the same models (model IDs without the vendor prefix).
  for(const model of ["gpt-6-sol","gpt-5.6","gpt-6.1-terra"])expect(cacheStyle("openai",model)).toBe("openai-breakpoints")
  for(const model of ["gpt-5.5","gpt-5.1","gpt-4o-mini","o4-mini"])expect(cacheStyle("openai",model)).toBe("automatic")
})

test("no cache settings, or a provider that needs none: the request is exactly the plain messages",()=>{
  const plain=messages.map(({role,content})=>({role,content}))
  expect(cachedRequest("openrouter","anthropic/claude-sonnet-5.5",messages,undefined)).toEqual({messages:plain,extra:{}})
  // Models outside the two families never receive a cache field, whatever the feature asks for.
  for(const [provider,model] of [["openrouter","google/gemini-4-hydrogen"],["openrouter","openai/gpt-5.5"],["openrouter","openai/gpt-4o-mini"],["openai","gpt-5.1"]] as const)for(const tail of [true,false]){
    const wire=cachedRequest(provider,model,messages,{ttl:"1h",tail})
    expect(wire).toEqual({messages:plain,extra:{}})
    expect(JSON.stringify(wire)).not.toMatch(/cache/)
  }
})

test("GPT-5.6 and later, a conversation: a breakpoint on the marked message only; the instructions and the growing end are cached by the provider itself",()=>{
  for(const [provider,model] of [["openrouter","openai/gpt-6-sol"],["openai","gpt-6-sol"]] as const){
    const wire=cachedRequest(provider,model,messages,{ttl:"1h",tail:true})
    expect(wire.extra).toEqual({})
    expect(wire.messages[2]).toEqual({role:"user",content:[{type:"text",text:"Transcript part 2",prompt_cache_breakpoint:{mode:"explicit"}}]})
    expect(wire.messages.filter((_,index)=>index!==2)).toEqual(messages.filter((_,index)=>index!==2).map(({role,content})=>({role,content})))
  }
})

test("GPT-5.6 and later, a one-off: explicit mode, so only what is marked is written to the cache, and with nothing marked nothing is",()=>{
  const oneOff:LanguageMessage[]=[{role:"system",content:"Instructions",cache:true},{role:"user",content:"A transcript nobody sends twice"}]
  const wire=cachedRequest("openrouter","openai/gpt-6-sol",oneOff,{ttl:"5m",tail:false})
  expect(wire.extra).toEqual({prompt_cache_options:{mode:"explicit"}})
  expect(wire.messages).toEqual([{role:"system",content:[{type:"text",text:"Instructions",prompt_cache_breakpoint:{mode:"explicit"}}]},{role:"user",content:"A transcript nobody sends twice"}])
  const unmarked=cachedRequest("openai","gpt-6-sol",oneOff.map(({role,content})=>({role,content})),{ttl:"5m",tail:false})
  expect(unmarked).toEqual({messages:oneOff.map(({role,content})=>({role,content})),extra:{prompt_cache_options:{mode:"explicit"}}})
})

test("instructions split to mark their constant part are still one system message, and one string where no marker is taken",()=>{
  const split:LanguageMessage[]=[{role:"system",content:"Core rules.",cache:true},{role:"system",content:"\n\nFor this meeting: four sections."},{role:"user",content:"Transcript"}]
  const joined=[{role:"system",content:"Core rules.\n\nFor this meeting: four sections."},{role:"user",content:"Transcript"}]
  expect(cachedRequest("openrouter","openai/gpt-4o-mini",split,{ttl:"5m",tail:false})).toEqual({messages:joined,extra:{}})
  expect(cachedRequest("openrouter","openai/gpt-6-sol",split,undefined)).toEqual({messages:joined,extra:{}})
  expect(cachedRequest("openrouter","openai/gpt-6-sol",split,{ttl:"5m",tail:false}).messages[0]).toEqual({role:"system",content:[{type:"text",text:"Core rules.",prompt_cache_breakpoint:{mode:"explicit"}},{type:"text",text:"\n\nFor this meeting: four sections."}]})
  expect(cachedRequest("anthropic","claude-sonnet-5-5",split,{ttl:"5m",tail:false})).toEqual({messages:[{role:"system",content:[{type:"text",text:"Core rules.",cache_control:{type:"ephemeral"}},{type:"text",text:"\n\nFor this meeting: four sections."}]},{role:"user",content:"Transcript"}],extra:{}})
})

test("a tool loop keeps its tool calls and results; only the text of a marked message changes form",()=>{
  const loop=[
    {role:"system",content:"Research rules",cache:true as const},
    {role:"user",content:"<workspace>…</workspace>",cache:true as const},
    {role:"user",content:"<now>…</now>\n\nQuestion"},
    {role:"assistant",content:null,tool_calls:[{id:"call_1",type:"function",function:{name:"search_meetings",arguments:"{}"}}]},
    {role:"tool",tool_call_id:"call_1",content:"results"},
  ]
  const anthropic=cachedRequest("openrouter","anthropic/claude-sonnet-5.5",loop,{ttl:"5m",tail:true}),control={type:"ephemeral"}
  expect(anthropic.extra).toEqual({cache_control:control})
  expect(anthropic.messages).toEqual([
    {role:"system",content:[{type:"text",text:"Research rules",cache_control:control}]},
    {role:"user",content:[{type:"text",text:"<workspace>…</workspace>",cache_control:control}]},
    {role:"user",content:"<now>…</now>\n\nQuestion"},
    {role:"assistant",content:null,tool_calls:loop[3]!.tool_calls},
    {role:"tool",tool_call_id:"call_1",content:"results"},
  ])
  const openai=cachedRequest("openrouter","openai/gpt-6-sol",loop,{ttl:"5m",tail:true})
  expect(openai.extra).toEqual({})
  expect(openai.messages[0]).toEqual({role:"system",content:"Research rules"})
  expect(openai.messages[1]).toEqual({role:"user",content:[{type:"text",text:"<workspace>…</workspace>",prompt_cache_breakpoint:{mode:"explicit"}}]})
  expect(openai.messages.slice(2)).toEqual(anthropic.messages.slice(2))
})

test("Anthropic: breakpoints on the marked messages (the instructions among them), automatic caching for the end, one lifetime for all",()=>{
  for(const [provider,model] of [["openrouter","anthropic/claude-sonnet-5.5"],["anthropic","claude-sonnet-5-5"]] as const){
    const hour=cachedRequest(provider,model,messages,{ttl:"1h",tail:true}),control={type:"ephemeral",ttl:"1h"}
    expect(hour.extra).toEqual({cache_control:control})
    expect(hour.messages[0]).toEqual({role:"system",content:[{type:"text",text:"Instructions",cache_control:control}]})
    expect(hour.messages[2]).toEqual({role:"user",content:[{type:"text",text:"Transcript part 2",cache_control:control}]})
    expect(hour.messages.filter(message=>typeof message.content!=="string")).toHaveLength(2)
    // The default lifetime carries no `ttl` field.
    expect(cachedRequest(provider,model,messages,{ttl:"5m",tail:true}).extra).toEqual({cache_control:{type:"ephemeral"}})
    // A request whose end is unique gets no automatic breakpoint: only what is marked is written to the cache.
    expect(cachedRequest(provider,model,messages,{ttl:"5m",tail:false}).extra).toEqual({})
  }
  // Never more than four breakpoints: the automatic one, and the three marked messages furthest in.
  const many:LanguageMessage[]=[messages[0]!,...Array.from({length:5},(_,index)=>({role:"user",content:`Part ${index}`,cache:true as const})),{role:"user",content:"Question"}]
  const wire=cachedRequest("anthropic","claude-sonnet-5-5",many,{ttl:"5m",tail:true})
  expect(wire.messages.map(message=>typeof message.content!=="string")).toEqual([false,false,false,true,true,true,false])
  // An unmarked system prompt gets none.
  expect(cachedRequest("anthropic","claude-sonnet-5-5",[{role:"system",content:"Instructions"},{role:"user",content:"Question"}],{ttl:"5m",tail:true}).messages[0]).toEqual({role:"system",content:"Instructions"})
})

test("usage as each provider reports it becomes one shape",()=>{
  expect(readUsage({prompt_tokens:7638,completion_tokens:31,prompt_tokens_details:{cached_tokens:7122,cache_write_tokens:513},cost:0.0028729})).toEqual({promptTokens:7638,cachedTokens:7122,cacheWriteTokens:513,outputTokens:31,costUsd:0.0028729})
  expect(readUsage({prompt_tokens:900,completion_tokens:12})).toEqual({promptTokens:900,cachedTokens:0,cacheWriteTokens:0,outputTokens:12,costUsd:null})
  expect(readUsage(undefined)).toBeNull()
})

const id=(n:number)=>`00000000-0000-4000-8000-${String(n).padStart(12,"0")}`
const recording:NoteDTO={id:id(1),title:"Vendor review",body:"",enhanced:"",date:"2026-03-09",duration:"0:10",createdAt:"2026-03-09T10:00:00.000Z",updatedAt:"2026-03-09T11:00:00.000Z",recordingState:"recording",generationState:"idle",transcriptRevision:1,generatedRevision:null,
  transcriptSegments:[{id:id(10),channel:"system",speakerId:"system:n:1",speakerLabel:"Speaker 1",text:"The vendor raised prices by eleven percent.",isFinal:true,startMs:1000,endMs:5000}]}
const input:ChatInput={message:"By how much?",history:[],includeSources:true}
const prepared=():ProviderNote=>({...recording,meetingChat:assembleMeetingChat(recording,input,{citable:true,conversations:makeLiveConversationStore()})}) as ProviderNote
const sse=(frames:ReadonlyArray<unknown>,done=true)=>new Response(frames.map(frame=>`data: ${JSON.stringify(frame)}\n\n`).join("")+(done?"data: [DONE]\n\n":""),{headers:{"Content-Type":"text/event-stream"}})
async function answer(options:Parameters<typeof makeProviderServices>[0],respond:(body:Record<string,any>)=>Response) {
  const sent:Array<{url:string;body:Record<string,any>}>=[],events:LanguageUsageEvent[]=[]
  const providers=makeProviderServices({...options,onLanguageUsage:event=>{events.push(event)},fetch:(async(url:Parameters<typeof fetch>[0],init?:RequestInit)=>{const body=JSON.parse(String(init?.body));sent.push({url:String(url),body});return respond(body)}) as typeof fetch})
  let text=""
  for await(const delta of providers.chatStream!(prepared(),input,new AbortController().signal))text+=delta
  return {text,sent,events}
}

test("OpenRouter: the cached-token count of a streamed answer is reported with the request's layout",async()=>{
  const {text,sent,events}=await answer({openRouterKey:"fixture-key",languageModel:"openai/gpt-6-sol"},()=>sse([
    {choices:[{delta:{content:"Eleven "}}]},{choices:[{delta:{content:"percent."}}]},
    {choices:[],usage:{prompt_tokens:4000,completion_tokens:5,prompt_tokens_details:{cached_tokens:3600,cache_write_tokens:380},cost:0.0021}}]))
  expect(text).toBe("Eleven percent.")
  expect(sent[0]!.url).toBe("https://openrouter.ai/api/v1/chat/completions")
  expect(events).toHaveLength(1)
  expect(events[0]).toMatchObject({feature:"live-chat",layout:"new",provider:"openrouter",model:"openai/gpt-6-sol",usage:{promptTokens:4000,cachedTokens:3600,cacheWriteTokens:380,outputTokens:5,costUsd:0.0021}})
  expect(events[0]!.firstTokenMs).toBeGreaterThanOrEqual(0)
})

test("Anthropic directly: cache markers reach the Messages API, and its usage comes back as cached and written tokens",async()=>{
  const {text,sent,events}=await answer({languageProvider:"anthropic",anthropicKey:"fixture-key",languageModel:"claude-sonnet-5-5"},()=>sse([
    {type:"message_start",message:{usage:{input_tokens:40,cache_read_input_tokens:3600,cache_creation_input_tokens:380,output_tokens:1}}},
    {type:"content_block_delta",delta:{type:"text_delta",text:"Eleven percent."}},
    {type:"message_delta",delta:{stop_reason:"end_turn"},usage:{output_tokens:6}},
    {type:"message_stop"}],false))
  expect(text).toBe("Eleven percent.")
  const {url,body}=sent[0]!,control={type:"ephemeral",ttl:"1h"}
  expect(url).toBe("https://api.anthropic.com/v1/messages")
  expect(body.cache_control).toEqual(control)
  expect(body.system).toHaveLength(1)
  expect(body.system[0].cache_control).toEqual(control)
  expect(body.messages).toHaveLength(1)
  expect(body.messages[0].content[0].text).toContain("<transcript>")
  expect(events[0]).toMatchObject({provider:"anthropic",usage:{promptTokens:4020,cachedTokens:3600,cacheWriteTokens:380,outputTokens:6}})
})

// A model that does not reason: its chat answer is a chat completion (a reasoning model's goes through the Responses API, `test/chat-reasoning.test.ts`).
test("OpenAI directly: a streamed answer asks for usage; a short thread has nothing to mark, so it carries no cache field",async()=>{
  const {sent,events}=await answer({languageProvider:"openai",openAiKey:"fixture-key",languageModel:"gpt-4.1"},()=>sse([
    {choices:[{delta:{content:"Eleven percent."}}]},{choices:[],usage:{prompt_tokens:4000,completion_tokens:5,prompt_tokens_details:{cached_tokens:3584}}}]))
  expect(sent[0]!.url).toBe("https://api.openai.com/v1/chat/completions")
  expect(sent[0]!.body.stream_options).toEqual({include_usage:true})
  expect(JSON.stringify(sent[0]!.body)).not.toMatch(/cache_control|prompt_cache/)
  expect(events[0]).toMatchObject({provider:"openai",usage:{promptTokens:4000,cachedTokens:3584,cacheWriteTokens:0}})
})
