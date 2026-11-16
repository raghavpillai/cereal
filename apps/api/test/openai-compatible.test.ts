/** The OpenAI-compatible language provider (Ollama, vLLM, LM Studio, any /v1 API): its address, its model list, the
 * plain chat completions it is sent, the settings that hold it, and chat that still works when it has no tool calling.
 * Also the masked key previews the admin panel shows. No network: every server here is a function. */
import {expect,test} from "bun:test"
import {Effect} from "effect"
import {compatibleBaseUrlProblem,normalizeCompatibleBaseUrl} from "@meeting-notes/contracts/compatible"
import {makeMemoryRepository} from "../src/repository"
import {makeMemoryCalendarStore} from "../src/calendar-store"
import {secretBoxFromKey} from "../src/admin-secrets"
import {keyHint,makeAdministrationService,settingsAwareProviders} from "../src/administration"
import {makeModelCatalog} from "../src/model-catalog"
import {compatibleBody} from "../src/language-transport"
import {LANGUAGE_RESPONSE_LIMIT,makeProviderServices} from "../src/providers"
import {prepareMeetingChat} from "../src/chat-retrieval"
import {cachedRequest} from "../src/ai/prompt-cache"
import {randomBytes} from "node:crypto"
import type {NoteDTO} from "@meeting-notes/contracts"

const run=Effect.runPromise
type Seen={url:string;method:string;headers:Headers;body:any;redirect?:RequestRedirect}
/** A pretend OpenAI-compatible server at `base`, answering /models and /chat/completions as `answer` says. */
function server(answer:(seen:Seen)=>Response|Promise<Response>){
 const seen:Seen[]=[]
 const transport=Object.assign(async(input:any,init?:RequestInit)=>{
  const url=String(input),entry:Seen={url,method:init?.method??"GET",headers:new Headers(init?.headers),body:typeof init?.body==="string"?JSON.parse(init.body):undefined,redirect:init?.redirect}
  seen.push(entry);return answer(entry)
 },{preconnect:()=>{}}) as typeof fetch
 return {seen,transport}
}
const models=(...ids:string[])=>Response.json({object:"list",data:ids.map(id=>({id,object:"model",owned_by:"library"}))})
/** OpenRouter's public model list with one chat model, for tests that switch chat and notes back to it. */
const routerModels=()=>Response.json({data:[{id:"openai/gpt-6-sol",name:"GPT-6 Sol",architecture:{input_modalities:["text"],output_modalities:["text"]}}]})
const completion=(content:string)=>Response.json({id:"chatcmpl-1",object:"chat.completion",choices:[{index:0,message:{role:"assistant",content},finish_reason:"stop"}]})
const sse=(...frames:string[])=>new Response(frames.map(frame=>`data: ${frame}\n\n`).join(""),{headers:{"Content-Type":"text/event-stream"}})
const delta=(text:string,finish:string|null=null)=>JSON.stringify({choices:[{index:0,delta:{content:text},finish_reason:finish}]})

test("the address is checked and tidied the same way everywhere: http or https, nothing guessed",()=>{
 expect(normalizeCompatibleBaseUrl(" http://10.0.0.5:11434/v1/ ")).toBe("http://10.0.0.5:11434/v1")
 expect(normalizeCompatibleBaseUrl("https://api.together.xyz/v1")).toBe("https://api.together.xyz/v1")
 // Without /v1 it is used as given; trailing slashes go, nothing is added.
 expect(normalizeCompatibleBaseUrl("http://localhost:1234///")).toBe("http://localhost:1234")
 expect(normalizeCompatibleBaseUrl("HTTP://Model-Box.lan:8000/openai/v1")).toBe("http://model-box.lan:8000/openai/v1")
 for(const [bad,why] of [["","Enter the server"],["ftp://host/v1","http:// or https://"],["javascript:alert(1)","http:// or https://"],["localhost:11434/v1","http:// or https://"],
  ["http://user:secret@host/v1","user name and password"],["http://host/v1?key=x","after ? or #"],["http://host/v1#x","after ? or #"],["http://host/v1?","after ? or #"],["http://ho st/v1","http:// or https://"],["http://h/"+"a".repeat(2050),"too long"]] as const)
  expect(compatibleBaseUrlProblem(bad)).toContain(why)
})

test("requests are plain OpenAI chat completions at {base}/chat/completions: nothing OpenRouter- or Anthropic-specific, a key only when there is one",async()=>{
 // What cachedRequest would add for providers that take cache fields, laid over the shared request.
 const marked=cachedRequest("anthropic","claude-test",[{role:"system",content:"Rules",cache:true},{role:"user",content:"Question"}],{tail:true,ttl:"5m"})
 const sanitised=compatibleBody({model:"llama3.2:3b",messages:marked.messages,stream:true,stream_options:{include_usage:true},max_completion_tokens:300,reasoning_effort:"none",parallel_tool_calls:false,
  provider:{order:["x"]},transforms:[],prompt_cache_options:{mode:"explicit"},...marked.extra,tools:[{type:"function",function:{name:"search",description:"S",parameters:{type:"object"}}}],tool_choice:"auto"})
 expect(sanitised).toEqual({model:"llama3.2:3b",messages:[{role:"system",content:"Rules"},{role:"user",content:"Question"}],stream:true,max_tokens:300,tools:[{type:"function",function:{name:"search",description:"S",parameters:{type:"object"}}}],tool_choice:"auto"})
 // Tool turns keep their fields.
 expect(compatibleBody({messages:[{role:"assistant",content:null,tool_calls:[{id:"c1",type:"function",function:{name:"search",arguments:"{}"}}],cache:true},{role:"tool",tool_call_id:"c1",content:"R"}]}).messages)
  .toEqual([{role:"assistant",content:null,tool_calls:[{id:"c1",type:"function",function:{name:"search",arguments:"{}"}}]},{role:"tool",tool_call_id:"c1",content:"R"}])

 const repository=makeMemoryRepository(),note=await run(repository.createNote({title:"Weekly sync"}))
 for(const key of ["","local-key"]){
  const s=server(seen=>seen.body?.stream?sse(delta("Hel"),delta("lo","stop"),"[DONE]"):completion("Notes body"))
  const p=makeProviderServices({languageProvider:"openai_compatible",openAiCompatibleBaseUrl:"http://10.0.0.5:11434/v1",openAiCompatibleKey:key,languageModel:"llama3.2:3b",fetch:s.transport,onLanguageUsage:()=>{}})
  expect(await run(p.generate(note))).toBe("Notes body")
  let answer="";for await(const text of p.chatStream!(note,{message:"Question",history:[]},new AbortController().signal))answer+=text
  expect(answer).toBe("Hello")
  for(const request of s.seen){
   expect(request.url).toBe("http://10.0.0.5:11434/v1/chat/completions")
   expect(request.redirect).toBe("error")
   expect(request.headers.get("authorization")).toBe(key?"Bearer local-key":null)
   expect(request.body.model).toBe("llama3.2:3b")
   for(const field of ["stream_options","reasoning_effort","max_completion_tokens","cache_control","prompt_cache_options","provider","parallel_tool_calls"])expect(field in request.body).toBe(false)
   expect(request.body.messages.every((m:any)=>typeof m.content==="string")).toBe(true)
  }
 }
})

test("a streamed answer without usage, or without the closing [DONE] after it finished, is still a whole answer; one cut short is not",async()=>{
 const repository=makeMemoryRepository(),note=await run(repository.createNote({title:"Weekly sync"}))
 const ask=async(response:()=>Response)=>{
  const usage:unknown[]=[],p=makeProviderServices({languageProvider:"openai_compatible",openAiCompatibleBaseUrl:"http://localhost:1234/v1",languageModel:"m",fetch:server(()=>response()).transport,onLanguageUsage:event=>usage.push(event.usage)})
  let text="";for await(const part of p.chatStream!(note,{message:"Q",history:[]},new AbortController().signal))text+=part
  return {text,usage}
 }
 expect(await ask(()=>sse(delta("A"),delta("B","stop")))).toEqual({text:"AB",usage:[null]})
 expect((await ask(()=>sse(delta("A"),delta("B","stop"),"[DONE]"))).text).toBe("AB")
 await expect(ask(()=>sse(delta("A"),delta("B")))).rejects.toMatchObject({reason:"invalid_response"})
})

test("the model list comes from {base}/models; every id counts for chat; failures name the server and say what to check",async()=>{
 const s=server(seen=>seen.url.endsWith("/models")?models("llama3.2:3b","qwen2.5:7b","nomic-embed-text"):completion("x"))
 const catalog=makeModelCatalog(s.transport)
 const listed=await catalog("openai_compatible","",false,"http://10.0.0.5:11434/v1")
 expect(listed.models.map(m=>[m.id,m.capabilities])).toEqual([["llama3.2:3b",["language"]],["nomic-embed-text",["language"]],["qwen2.5:7b",["language"]]])
 expect(s.seen.map(r=>[r.url,r.headers.get("authorization"),r.redirect])).toEqual([["http://10.0.0.5:11434/v1/models",null,"error"]])
 // Cached per address and key.
 await catalog("openai_compatible","",false,"http://10.0.0.5:11434/v1");expect(s.seen).toHaveLength(1)
 await catalog("openai_compatible","k",false,"http://10.0.0.5:11434/v1");expect(s.seen.at(-1)!.headers.get("authorization")).toBe("Bearer k")
 // An empty list says the name can be typed.
 expect((await makeModelCatalog(server(()=>models()).transport)("openai_compatible","",false,"http://h/v1")).notice).toContain("Type the model name")

 const admin=(transport:typeof fetch)=>makeAdministrationService(makeMemoryCalendarStore(makeMemoryRepository()),undefined,{fetch:transport,environment:{}})
 const failure=async(answer:()=>Response|Promise<Response>)=>run(admin(server(answer).transport).getModelCatalog({provider:"openai_compatible",baseUrl:"http://10.0.0.5:11434/v1"})).catch(error=>error)
 expect(await failure(()=>{throw new TypeError("connect ECONNREFUSED")})).toEqual({error:"unavailable",message:"Couldn’t reach 10.0.0.5:11434. Check the address and that the server is running."})
 expect((await failure(()=>new Response("nope",{status:401}))).message).toBe("10.0.0.5:11434 refused the request (HTTP 401). Check the API key.")
 expect((await failure(()=>new Response("not found",{status:404}))).message).toContain("answered HTTP 404 for /v1/models")
 expect((await failure(()=>new Response("<html>",{status:200}))).message).toContain("did not answer with a list of models")
 expect(await run(admin(server(()=>models()).transport).getModelCatalog({provider:"openai_compatible",baseUrl:"http://user:pw@h/v1"})).catch(error=>error)).toMatchObject({error:"invalid_request",message:expect.stringContaining("user name and password")})
 expect(await run(admin(server(()=>models()).transport).getModelCatalog({provider:"openai_compatible"})).catch(error=>error)).toMatchObject({error:"invalid_request"})
 // A hosted provider still needs a real key.
 expect(await run(admin(server(()=>models()).transport).getModelCatalog({provider:"openai",apiKey:"short"})).catch(error=>error)).toMatchObject({error:"invalid_request"})
})

test("a model typed by hand is accepted after one successful test request, and refused with the server's reason",async()=>{
 let listing:"ok"|"down"|"empty"="down"
 const s=server(seen=>seen.url.endsWith("/models")?(listing==="down"?new Response("",{status:404}):listing==="empty"?models():models("listed-model"))
  :seen.body.model==="real-model"||seen.body.model==="listed-model"?completion("OK"):Response.json({error:{message:"model not found"}},{status:404}))
 const store=makeMemoryCalendarStore(makeMemoryRepository()),admin=makeAdministrationService(store,undefined,{fetch:s.transport,environment:{}})
 const choose=(model:string,extra:object={})=>run(admin.updateSettings({openAiCompatibleBaseUrl:"http://10.0.0.5:11434/v1/",chatModels:[{provider:"openai_compatible",model}],generationModel:{provider:"openai_compatible",model},...extra}))
 await expect(choose("missing-model")).rejects.toMatchObject({error:"invalid_request",message:"10.0.0.5:11434 did not accept the model “missing-model” (HTTP 404). Check the model name."})
 expect(await store.getSettings()).toBeNull()
 const saved=await choose("real-model")
 expect(saved.chatModels).toEqual([{provider:"openai_compatible",model:"real-model"}])
 const probe=s.seen.filter(r=>r.method==="POST").at(-1)!
 expect(probe).toMatchObject({url:"http://10.0.0.5:11434/v1/chat/completions",redirect:"error",body:{model:"real-model",stream:false}})
 // A listed model needs no test request.
 listing="ok";const posts=s.seen.filter(r=>r.method==="POST").length
 await run(admin.updateSettings({chatModels:[{provider:"openai_compatible",model:"listed-model"}]}))
 expect(s.seen.filter(r=>r.method==="POST")).toHaveLength(posts)
 // Without an address there is nothing to check against.
 const empty=makeAdministrationService(makeMemoryCalendarStore(makeMemoryRepository()),undefined,{fetch:s.transport,environment:{}})
 await expect(run(empty.updateSettings({chatModels:[{provider:"openai_compatible",model:"real-model"}]}))).rejects.toMatchObject({message:expect.stringContaining("address")})
 // Unreachable: said so.
 const down=makeAdministrationService(makeMemoryCalendarStore(makeMemoryRepository()),undefined,{fetch:server(()=>{throw new TypeError("offline")}).transport,environment:{}})
 await expect(run(down.updateSettings({openAiCompatibleBaseUrl:"http://10.0.0.9/v1",chatModels:[{provider:"openai_compatible",model:"m"}]}))).rejects.toMatchObject({message:"Couldn’t reach 10.0.0.9. Check the address and that the server is running."})
})

test("settings round-trip: the address is stored tidy, the key sealed and never returned, and notes and chat go to the server",async()=>{
 const key="sk-local-compatible-key-0123456789abcd"
 const s=server(seen=>seen.url==="https://openrouter.ai/api/v1/models"?routerModels():seen.url.endsWith("/models")?models("llama3.2:3b"):seen.body?.stream?sse(delta("Streamed","stop"),"[DONE]"):completion("Generated notes"))
 const repository=makeMemoryRepository(),store=makeMemoryCalendarStore(repository),admin=makeAdministrationService(store,undefined,{fetch:s.transport,environment:{}})
 const saved=await run(admin.updateSettings({openAiCompatibleBaseUrl:" http://10.0.0.5:11434/v1/ ",openAiCompatibleApiKey:key,chatModels:[{provider:"openai_compatible",model:"llama3.2:3b"}],generationModel:{provider:"openai_compatible",model:"llama3.2:3b"}}))
 expect(saved.openAiCompatible).toEqual({baseUrl:"http://10.0.0.5:11434/v1",apiKeySet:true,apiKeyHint:"sk-…abcd"})
 expect(saved.providers.find(p=>p.id==="openai_compatible")).toEqual({id:"openai_compatible",configured:true,source:"stored",hint:"http://10.0.0.5:11434/v1"})
 expect(saved.onboarding!.steps.language).toBe(true)
 expect(JSON.stringify(saved)).not.toContain(key);expect(JSON.stringify(await run(admin.getSettings()))).not.toContain(key)
 const stored=await store.getSettings();expect(stored!.openAiCompatibleBaseUrl).toBe("http://10.0.0.5:11434/v1");expect(JSON.stringify(stored)).not.toContain(key)
 const providers=settingsAwareProviders(admin,s.transport),note=await run(repository.createNote({title:"Weekly sync"}))
 expect(await run(providers.generate(note))).toBe("Generated notes")
 let answer="";for await(const text of providers.chatStream!(note,{message:"Q",history:[]},new AbortController().signal))answer+=text
 expect(answer).toBe("Streamed")
 for(const request of s.seen.filter(r=>r.method==="POST"))expect([request.url,request.headers.get("authorization")]).toEqual(["http://10.0.0.5:11434/v1/chat/completions","Bearer "+key])
 // The key can be removed (a keyless server), and the address too, which disconnects it.
 expect((await run(admin.updateSettings({openAiCompatibleApiKey:null}))).openAiCompatible).toEqual({baseUrl:"http://10.0.0.5:11434/v1",apiKeySet:false,apiKeyHint:null})
 expect((await admin.configuration()).openAiCompatibleKey).toBe("")
 // While chat and notes use it, the address stays: removing it would break the next request.
 await expect(run(admin.updateSettings({openAiCompatibleBaseUrl:null}))).rejects.toEqual({error:"invalid_request",message:"The OpenAI-compatible server is still used for chat and meeting notes. Choose other models there before removing its address."})
 expect((await store.getSettings())!.openAiCompatibleBaseUrl).toBe("http://10.0.0.5:11434/v1")
 const off=await run(admin.updateSettings({openAiCompatibleBaseUrl:null,chatModels:[{provider:"openrouter",model:"openai/gpt-6-sol"}],generationModel:{provider:"openrouter",model:"openai/gpt-6-sol"}}))
 expect(off.providers.find(p=>p.id==="openai_compatible")).toMatchObject({configured:false,source:"none",hint:null})
 await expect(run(admin.updateSettings({openAiCompatibleBaseUrl:"http://h/v1#frag"}))).rejects.toMatchObject({error:"invalid_request"})
})

test("workspace chat on a server without tool calling falls back to a direct lookup instead of failing",async()=>{
 const id=(n:number)=>`00000000-0000-4000-8000-${String(n).padStart(12,"0")}`
 const now=new Date("2026-03-10T15:00:00.000Z")
 const seed:NoteDTO[]=[{id:id(1),title:"Windrush budget review",body:"We agreed the Windrush budget.",enhanced:"",date:"2026-03-09",duration:"",createdAt:"2026-03-09T10:00:00.000Z",updatedAt:"2026-03-09T11:00:00.000Z",transcriptSegments:[],recordingState:"stopped",generationState:"idle",transcriptRevision:0,generatedRevision:null}]
 // A server that refuses the tools field outright, and one that ignores it and answers in prose.
 const refusals=[()=>Response.json({error:{message:"tools is not supported"}},{status:400}),()=>completion("I cannot call tools, here is prose.")]
 for(const refusal of refusals){
  const s=server(seen=>seen.url.endsWith("/models")?models("m"):refusal())
  const store=makeMemoryCalendarStore(makeMemoryRepository()),admin=makeAdministrationService(store,undefined,{fetch:s.transport,environment:{}})
  await run(admin.updateSettings({openAiCompatibleBaseUrl:"http://localhost:11434/v1",chatModels:[{provider:"openai_compatible",model:"m"}]}))
  const providers=settingsAwareProviders(admin,s.transport),events:unknown[]=[]
  const prepared=await prepareMeetingChat({noteId:null,input:{message:"What did we decide about Windrush?",history:[]},repository:makeMemoryRepository(seed,()=>now),providers,signal:new AbortController().signal,now,enrich:async source=>source,emit:event=>{events.push(event)}})
  // The planner request went to the server, and the meeting was still found and read.
  expect(s.seen.some(r=>r.method==="POST"&&Array.isArray(r.body.tools))).toBe(true)
  // The meeting was found and read all the same; a refused planner is said to have failed, honestly.
  expect(prepared.note.body).toContain('<meeting title="Windrush budget review"')
  expect(prepared.note.body.includes("One lookup failed")).toBe(refusal===refusals[0])
 }
})

test("key previews: a known prefix and the last four, less of a short key, nothing of a tiny one, and null for an unreadable secret",async()=>{
 expect(keyHint("sk-or-v1-0123456789abcdefa1b2")).toBe("sk-or-v1-…a1b2")
 expect(keyHint("sk-proj-0123456789abcdefWXYZ")).toBe("sk-proj-…WXYZ")
 expect(keyHint("sk-ant-api03-0123456789-9xYz")).toBe("sk-ant-…9xYz")
 expect(keyHint("GOCSPX-0123456789abcdefa1b2")).toBe("GOCSPX-…a1b2")
 expect(keyHint("sk_0123456789abcdef0123k3Jd")).toBe("sk_…k3Jd")
 expect(keyHint("sk-0123456789abcdefk3Jd")).toBe("sk-…k3Jd")
 expect(keyHint("0123456789abcdefk3Jd")).toBe("…k3Jd")
 expect(keyHint("sk-or-v1-0123456789abcdefa1b2",{prefix:false})).toBe("…a1b2")
 expect(keyHint("short-key-xy")).toBe("…xy")
 expect(keyHint("tiny")).toBe("…")

 const openRouter="sk-or-v1-full-secret-key-0000a1b2",google="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b2",bucket="s3-secret-access-key-value-0000WXYZ"
 const store=makeMemoryCalendarStore(makeMemoryRepository()),box=secretBoxFromKey(randomBytes(32))
 const catalogs=server(seen=>seen.url.includes("openrouter.ai/api/v1/key")?Response.json({data:{}}):Response.json({data:[]}))
 const admin=makeAdministrationService(store,box,{fetch:catalogs.transport,environment:{ANTHROPIC_API_KEY:"sk-ant-environment-only-key-9xYz"}})
 const saved=await run(admin.updateSettings({openRouterApiKey:openRouter,signIn:{googleClientId:"1234567890-fixture.apps.googleusercontent.com",googleClientSecret:google},storage:{bucket:"cereal-files",accessKeyId:"AKIAFIXTURE",secretAccessKey:bucket}}))
 expect(saved.providers.find(p=>p.id==="openrouter")).toMatchObject({source:"stored",hint:"sk-or-v1-…a1b2"})
 expect(saved.providers.find(p=>p.id==="anthropic")).toMatchObject({source:"environment",hint:"sk-ant-…9xYz"})
 expect(saved.providers.find(p=>p.id==="openai")).toMatchObject({source:"none",hint:null})
 expect(saved.signIn!.googleClientSecretHint).toBe("GOCSPX-…a1b2")
 expect(saved.storage!.secretAccessKeyHint).toBe("…WXYZ")
 const json=JSON.stringify(saved);for(const secret of [openRouter,google,bucket,"sk-ant-environment-only-key-9xYz"])expect(json).not.toContain(secret)
 // Read with another settings key (a replaced key), the stored secrets cannot be opened: no hint, no failure.
 const replaced=makeAdministrationService(store,secretBoxFromKey(randomBytes(32)),{fetch:catalogs.transport,environment:{}})
 const unreadable=await run(replaced.getSettings())
 expect(unreadable.providers.find(p=>p.id==="openrouter")).toMatchObject({configured:true,hint:null})
 expect(unreadable.signIn!.googleClientSecretHint).toBeNull()
})

test("the saved key goes only to the saved address: a draft address gets a key typed with it, or none",async()=>{
 const saved="sk-saved-compatible-key-0123456789wxyz",typed="typed-key-for-the-new-server"
 const s=server(seen=>seen.url.endsWith("/models")?models("llama3.2:3b"):seen.body?.stream?sse(delta("Hi","stop"),"[DONE]"):completion("OK"))
 const repository=makeMemoryRepository(),store=makeMemoryCalendarStore(repository),admin=makeAdministrationService(store,undefined,{fetch:s.transport,environment:{}})
 await run(admin.updateSettings({openAiCompatibleBaseUrl:"http://10.0.0.5:11434/v1",openAiCompatibleApiKey:saved,chatModels:[{provider:"openai_compatible",model:"llama3.2:3b"}],generationModel:{provider:"openai_compatible",model:"llama3.2:3b"}}))
 s.seen.length=0
 const auth=(host:string)=>s.seen.filter(r=>new URL(r.url).host===host).map(r=>r.headers.get("authorization"))
 // The saved address, as saved or typed a little differently: the saved key.
 for(const baseUrl of [undefined,"http://10.0.0.5:11434/v1"," HTTP://10.0.0.5:11434/v1/ "])await run(admin.getModelCatalog({provider:"openai_compatible",...(baseUrl?{baseUrl}:{}),refresh:true}))
 expect(auth("10.0.0.5:11434")).toEqual(["Bearer "+saved,"Bearer "+saved,"Bearer "+saved])
 // Another address, a typo of the saved one, and a half-typed one (what typing with pauses would send): no key at all.
 for(const baseUrl of ["http://attacker.example/v1","http://10.0.0.5:1143/v1","http://10.0.0.5"])await run(admin.getModelCatalog({provider:"openai_compatible",baseUrl,refresh:true}))
 expect([...auth("attacker.example"),...auth("10.0.0.5:1143"),...auth("10.0.0.5")]).toEqual([null,null,null])
 // A key typed in the same request goes with it.
 await run(admin.getModelCatalog({provider:"openai_compatible",baseUrl:"http://attacker.example/v1",apiKey:typed,refresh:true}))
 expect(auth("attacker.example").at(-1)).toBe("Bearer "+typed)

 // Saving a new address with a model typed by hand: the listing and the test request carry no key, and the old key is
 // dropped, so chat and notes never send it to the new server either.
 const changed=await run(admin.updateSettings({openAiCompatibleBaseUrl:"http://new-box.lan:8000/v1",chatModels:[{provider:"openai_compatible",model:"typed-model"}]}))
 expect(auth("new-box.lan:8000")).toEqual([null,null])
 expect(changed.openAiCompatible).toEqual({baseUrl:"http://new-box.lan:8000/v1",apiKeySet:false,apiKeyHint:null})
 expect((await admin.configuration()).openAiCompatibleKey).toBe("")
 const providers=settingsAwareProviders(admin,s.transport),note=await run(repository.createNote({title:"Weekly sync"}))
 let answer="";for await(const text of providers.chatStream!(note,{message:"Q",history:[]},new AbortController().signal))answer+=text
 expect(answer).toBe("Hi")
 expect(auth("new-box.lan:8000")).toEqual([null,null,null])
 // Across everything above, the saved key went to the saved address and nowhere else.
 for(const request of s.seen)if(request.headers.get("authorization")==="Bearer "+saved)expect(new URL(request.url).host).toBe("10.0.0.5:11434")

 // A new address saved with a key typed for it keeps that key; the same address typed again keeps the saved one.
 await run(admin.updateSettings({openAiCompatibleBaseUrl:"http://other.lan/v1",openAiCompatibleApiKey:typed}))
 expect((await run(admin.updateSettings({openAiCompatibleBaseUrl:"http://other.lan/v1/"}))).openAiCompatible).toMatchObject({apiKeySet:true})
 expect((await admin.configuration()).openAiCompatibleKey).toBe(typed)
})

test("removing the address is refused while chat or notes still use the server, naming which",async()=>{
 const s=server(seen=>seen.url==="https://openrouter.ai/api/v1/models"?routerModels():seen.url.endsWith("/models")?models("m"):completion("OK"))
 const admin=makeAdministrationService(makeMemoryCalendarStore(makeMemoryRepository()),undefined,{fetch:s.transport,environment:{}})
 await run(admin.updateSettings({openAiCompatibleBaseUrl:"http://localhost:11434/v1",chatModels:[{provider:"openrouter",model:"openai/gpt-6-sol"},{provider:"openai_compatible",model:"m"}]}))
 await expect(run(admin.updateSettings({openAiCompatibleBaseUrl:null}))).rejects.toEqual({error:"invalid_request",message:"The OpenAI-compatible server is still used for chat. Choose other models there before removing its address."})
 // Dropping its chat model in the same change is fine.
 expect((await run(admin.updateSettings({openAiCompatibleBaseUrl:null,chatModels:[{provider:"openrouter",model:"openai/gpt-6-sol"}]}))).openAiCompatible).toMatchObject({baseUrl:null})
})

test("a redirect is said to be one, and not followed, for the model list and for a model typed by hand",async()=>{
 const target:string[]=[]
 const elsewhere=Bun.serve({port:0,fetch:request=>{target.push(request.url);return models("x")}})
 const redirecting=Bun.serve({port:0,fetch:request=>Response.redirect(`http://127.0.0.1:${elsewhere.port}${new URL(request.url).pathname}`,302)})
 try{
  const host=`127.0.0.1:${redirecting.port}`,admin=makeAdministrationService(makeMemoryCalendarStore(makeMemoryRepository()),undefined,{environment:{}})
  const expected=`${host} answered with a redirect, and Cereal doesn’t follow redirects. Enter the address it redirects to, where its API starts.`
  expect(await run(admin.getModelCatalog({provider:"openai_compatible",baseUrl:`http://${host}/v1`})).catch(error=>error)).toEqual({error:"unavailable",message:expected})
  await expect(run(admin.updateSettings({openAiCompatibleBaseUrl:`http://${host}/v1`,chatModels:[{provider:"openai_compatible",model:"m"}]}))).rejects.toEqual({error:"invalid_request",message:expected})
  expect(target).toEqual([])
  // A transport that hands back the 3xx itself is treated the same.
  const handing=makeAdministrationService(makeMemoryCalendarStore(makeMemoryRepository()),undefined,{fetch:server(()=>new Response(null,{status:307,headers:{location:"http://elsewhere/v1/models"}})).transport,environment:{}})
  expect((await run(handing.getModelCatalog({provider:"openai_compatible",baseUrl:"http://10.0.0.5/v1"})).catch(error=>error)).message).toContain("answered with a redirect")
  // No answer at all is still "couldn't reach".
  const closed=Bun.serve({port:0,fetch:()=>new Response()}),port=closed.port;closed.stop(true)
  expect((await run(admin.getModelCatalog({provider:"openai_compatible",baseUrl:`http://127.0.0.1:${port}/v1`})).catch(error=>error)).message).toContain("Couldn’t reach")
 }finally{redirecting.stop(true);elsewhere.stop(true)}
})

test("a non-streamed answer is read up to the same 1 MB as a streamed one; a longer one is refused, not buffered",async()=>{
 const repository=makeMemoryRepository(),note=await run(repository.createNote({title:"Weekly sync"}))
 const huge=()=>completion("x".repeat(LANGUAGE_RESPONSE_LIMIT+10))
 // Without a declared length, as a stream: read until the limit, then refused.
 const chunked=()=>{const body=JSON.stringify({choices:[{message:{content:"y".repeat(LANGUAGE_RESPONSE_LIMIT)},finish_reason:"stop"}]});let sent=0
  return new Response(new ReadableStream({pull(c){if(sent>=body.length){c.close();return}c.enqueue(new TextEncoder().encode(body.slice(sent,sent+65536)));sent+=65536}}))}
 for(const answer of [huge,chunked]){
  const p=makeProviderServices({languageProvider:"openai_compatible",openAiCompatibleBaseUrl:"http://h/v1",languageModel:"m",fetch:server(()=>answer()).transport})
  await expect(run(p.generate(note))).rejects.toMatchObject({reason:"invalid_response"})
  await expect(p.chatToolStep!([{role:"user",content:"Q"}],[{type:"function",function:{name:"search",description:"S",parameters:{type:"object"}}}],new AbortController().signal)).rejects.toMatchObject({reason:"invalid_response"})
 }
 // A normal answer still reads.
 expect(await run(makeProviderServices({languageProvider:"openai_compatible",openAiCompatibleBaseUrl:"http://h/v1",languageModel:"m",fetch:server(()=>completion("fine")).transport}).generate(note))).toBe("fine")
})
