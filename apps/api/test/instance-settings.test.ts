import {expect,test} from 'bun:test'
import {Effect} from 'effect'
import {makeMemoryRepository} from '../src/repository'
import {makeMemoryCalendarStore} from '../src/calendar-store'
import {makeAdministrationService,settingsAwareProviders} from '../src/administration'
import {makeModelCatalog} from '../src/model-catalog'
import {makeApiHandler} from '../src/api'
import {bootstrapPrincipal} from '../src/auth'
import {makeProviderServices} from '../src/providers'

const run=Effect.runPromise
const token='instance-settings-operator-token-at-least-32-characters'
const catalogs:Record<string,unknown>={
 'https://openrouter.ai/api/v1/models':{data:[{id:'vendor/chat-a',name:'Chat A',architecture:{input_modalities:['text'],output_modalities:['text']}},{id:'vendor/chat-b',name:'Chat B',architecture:{input_modalities:['text'],output_modalities:['text']}},{id:'vendor/image',architecture:{input_modalities:['text'],output_modalities:['image']}}]},
 'https://api.openai.com/v1/models':{data:[{id:'gpt-test'},{id:'gpt-transcribe'},{id:'gpt-image-2'},{id:'text-embedding-test'},{id:'gpt-realtime-test'}]},
 'https://api.anthropic.com/v1/models?limit=1000':{data:[{id:'claude-test',display_name:'Claude Test'}],has_more:false},
 'https://api.elevenlabs.io/v1/models':[{model_id:'scribe_v3',name:'Scribe v3'},{model_id:'eleven_multilingual',can_do_text_to_speech:true}],
}
function fixture(){
 const requests:Array<{url:string;init?:RequestInit}>=[]
 const transport=Object.assign(async(input:any,init?:RequestInit)=>{const url=String(input);requests.push({url,init});if(url in catalogs)return Response.json(catalogs[url]);if(url.endsWith('/messages'))return Response.json({content:[{type:'text',text:'Direct Claude answer'}],stop_reason:'end_turn'});return Response.json({choices:[{message:{content:'Answer'}}]})},{preconnect:()=>{}}) as typeof fetch
 const repository=makeMemoryRepository(),store=makeMemoryCalendarStore(repository),admin=makeAdministrationService(store,undefined,{fetch:transport,environment:{OPENROUTER_API_KEY:'environment-router-key'}})
 return{requests,transport,repository,store,admin}
}
test('instance settings encrypt every provider key, preserve partial edits and route tasks independently',async()=>{
 const f=fixture()
 const settings=await run(f.admin.updateSettings({instanceName:'Product studio',instanceLogo:'data:image/png;base64,aGVsbG8=',openAiApiKey:'secret-openai-key',anthropicApiKey:'secret-anthropic-key',elevenLabsApiKey:'secret-eleven-key',chatModels:[{provider:'openrouter',model:'vendor/chat-a'},{provider:'openai',model:'gpt-test'}],generationModel:{provider:'anthropic',model:'claude-test'},transcriptionModel:{provider:'openai',model:'gpt-transcribe'}}))
 expect(settings.providers.filter(p=>p.configured)).toHaveLength(4)
 for(const key of ['secret-openai-key','secret-anthropic-key','secret-eleven-key']){expect(JSON.stringify(settings)).not.toContain(key);expect(JSON.stringify(await f.store.getSettings())).not.toContain(key)}
 await run(f.admin.updateSettings({instanceName:'Another name'}));expect((await f.admin.configuration()).openAiKey).toBe('secret-openai-key')
 const providers=settingsAwareProviders(f.admin,f.transport),note=await run(f.repository.createNote({title:'Meeting'}))
 await run(providers.chat(note,{message:'Question',history:[],model:'openai:gpt-test'}))
 let last=f.requests.at(-1)!;expect(last.url).toBe('https://api.openai.com/v1/chat/completions');expect(JSON.parse(String(last.init?.body)).model).toBe('gpt-test')
 await run(providers.generate(note));last=f.requests.at(-1)!;expect(last.url).toBe('https://api.anthropic.com/v1/messages');expect(JSON.parse(String(last.init?.body)).model).toBe('claude-test')
 await expect(f.admin.configuration('chat','openai:unlisted')).rejects.toThrow('Choose an enabled chat model')
 await run(f.admin.updateSettings({openAiApiKey:null}));expect((await run(f.admin.getSettings())).providers.find(p=>p.id==='openai')?.source).toBe('none')
})
test('catalogs are capability-filtered, key-scoped, cached, refreshable and disclose the Scribe fallback',async()=>{
 const f=fixture(),catalog=makeModelCatalog(f.transport)
 const first=await catalog('openrouter','key-one');expect(first.models.map(m=>m.id)).toEqual(['vendor/chat-a','vendor/chat-b'])
 await catalog('openrouter','key-one');expect(f.requests).toHaveLength(1)
 await catalog('openrouter','key-two');await catalog('openrouter','key-one',true);expect(f.requests).toHaveLength(3)
 const openai=await catalog('openai','key');expect(openai.models).toHaveLength(2);expect(openai.models.find(m=>m.id==='gpt-transcribe')?.capabilities).toEqual(['transcription'])
 const fallback=makeModelCatalog(Object.assign(async()=>Response.json([{model_id:'eleven_voice',can_do_text_to_speech:true}]),{preconnect:()=>{}}) as typeof fetch)
 expect((await fallback('elevenlabs','key')).notice).toContain('does not list Scribe')
 expect((await fallback('elevenlabs','key')).models.some(m=>m.id==='scribe_v3')).toBe(true)
})
test('model discovery and settings writes enforce admin access; authenticated clients only receive enabled chat choices',async()=>{
 const f=fixture(),api=makeApiHandler(f.repository,token,{administration:f.admin,principalResolver:async headers=>headers.has('authorization')?{...bootstrapPrincipal,role:'user'}:null})
 try{
  const request=(path:string,method='GET',body?:unknown,authenticated=true)=>api.handler(new Request('http://localhost'+path,{method,headers:{...(authenticated?{Authorization:'Bearer user'}:{}),...(body?{'Content-Type':'application/json'}:{})},...(body?{body:JSON.stringify(body)}:{})}))
  expect((await request('/api/v1/admin/models','POST',{provider:'openai'})).status).toBe(403)
  expect((await request('/api/v1/admin/settings','PATCH',{instanceName:'Unauthorized'})).status).toBe(403)
  expect((await request('/api/v1/models/chat','GET',undefined,false)).status).toBe(401)
  const choices=await(await request('/api/v1/models/chat')).json();expect(choices.models).toHaveLength(1);expect(JSON.stringify(choices)).not.toContain('key');expect(f.requests).toHaveLength(0)
 }finally{await api.dispose()}
})
test('new model selections reject incompatible catalogs and duplicates without persisting a draft key',async()=>{
 const f=fixture()
 await expect(run(f.admin.updateSettings({openAiApiKey:'draft-key-never-saved',generationModel:{provider:'openai',model:'gpt-image-2'}}))).rejects.toBeDefined()
 expect(await f.store.getSettings()).toBeNull()
 await expect(run(f.admin.updateSettings({chatModels:[{provider:'openrouter',model:'vendor/chat-a'},{provider:'openrouter',model:'vendor/chat-a'}]}))).rejects.toBeDefined()
})
test('Anthropic streaming and tool results use the native Messages wire format',async()=>{
 const requests:any[]=[]
 const transport=Object.assign(async(url:any,init:any)=>{const body=JSON.parse(init.body);requests.push({url,body,headers:init.headers});return body.stream?new Response('event: content_block_delta\ndata: {"type":"content_block_delta","delta":{"type":"text_delta","text":"Hello"}}\n\nevent: message_stop\ndata: {"type":"message_stop"}\n\n'):Response.json({content:[{type:'tool_use',id:'tool-id',name:'search',input:{query:'meeting'}}],stop_reason:'tool_use'})},{preconnect:()=>{}}) as typeof fetch
 const p=makeProviderServices({languageProvider:'anthropic',anthropicKey:'native-claude-key',languageModel:'claude-test',fetch:transport}),f=fixture(),note=await run(f.repository.createNote({title:'Meeting'}))
 let answer='';for await(const text of p.chatStream!(note,{message:'Question',history:[]},new AbortController().signal))answer+=text;expect(answer).toBe('Hello');expect(requests[0].url).toBe('https://api.anthropic.com/v1/messages')
 const tools=await p.chatToolStep!([{role:'system',content:'Find notes'},{role:'user',content:'Question'},{role:'assistant',content:null,tool_calls:[{id:'previous',type:'function',function:{name:'search',arguments:'{}'}}]},{role:'tool',tool_call_id:'previous',content:'Result'}],[{type:'function',function:{name:'search',description:'Search',parameters:{type:'object'}}}],new AbortController().signal)
 expect(tools[0]).toMatchObject({name:'search',arguments:'{"query":"meeting"}'})
 expect(requests[1].body.messages.at(-1).content[0]).toEqual({type:'tool_result',tool_use_id:'previous',content:'Result'})
})
test('OpenAI transcription uses the selected model and retains unknown speaker identity',async()=>{
 let received:any
 const transport=Object.assign(async(url:any,init:any)=>{received={url,model:init.body.get('model')};return Response.json({text:'A new recording'})},{preconnect:()=>{}}) as typeof fetch
 const p=makeProviderServices({openAiKey:'test-openai-key',transcriptionProvider:'openai',transcriptionModel:'gpt-transcribe',fetch:transport})
 const segments=await run(p.transcribe({audio:new Uint8Array([0]),channel:'mic',chunkId:crypto.randomUUID(),startMs:1000,durationMs:2000}))
 expect(received).toEqual({url:'https://api.openai.com/v1/audio/transcriptions',model:'gpt-transcribe'});expect(segments[0]).toMatchObject({text:'A new recording',speakerLabel:'Unknown speaker',startMs:1000,endMs:3000})
})
