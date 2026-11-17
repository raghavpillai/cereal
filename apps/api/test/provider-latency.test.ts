import {afterEach,expect,test} from 'bun:test'
import {Effect} from 'effect'
import {makeMemoryRepository} from '../src/repository'
import {makeMemoryCalendarStore} from '../src/calendar-store'
import {makeAdministrationService,settingsAwareProviders} from '../src/administration'
import {makeProcessingService} from '../src/processing'
import {makeApiHandler} from '../src/api'

const cleanup:Array<()=>Promise<void>>=[]
afterEach(async()=>{for(const close of cleanup.splice(0))await close()})
const owner='settings-generation-stream-owner-at-least-32-characters'

// The server builds its providers with settingsAwareProviders. Without `generateStream` there, generation fell back to
// the buffered request and the whole document reached the client as one delta, together with the finished note.
test('the server providers stream generated notes as the model writes them',async()=>{
 const bodies:any[]=[]
 const PIECES=['# Launch Plan\n','- Ship on Monday\n','  - After the accessibility review\n','- Budget: $3,000']
 const sse=new Response(PIECES.map(text=>`data: ${JSON.stringify({choices:[{delta:{content:text}}]})}\n\n`).join('')+'data: [DONE]\n\n',{headers:{'Content-Type':'text/event-stream'}})
 const transport=Object.assign(async(_input:any,init?:RequestInit)=>{const body=JSON.parse(String(init?.body));bodies.push(body)
  return body.stream?sse:Response.json({choices:[{message:{content:PIECES.join('')}}]})},{preconnect:()=>{}}) as typeof fetch
 const repo=makeMemoryRepository()
 const admin=makeAdministrationService(makeMemoryCalendarStore(repo),undefined,{fetch:transport,environment:{OPENROUTER_API_KEY:'environment-router-key'}})
 const providers=settingsAwareProviders(admin,transport)
 expect(typeof providers.generateStream).toBe('function')
 const note=await Effect.runPromise(repo.createNote({title:'Launch review',transcriptSegments:[{id:crypto.randomUUID(),channel:'mic',speakerId:'mic:n:1',speakerLabel:'Me',text:'We ship Monday after the accessibility review; the budget is three thousand dollars.',startMs:0,endMs:4000,isFinal:true}]}))
 const api=makeApiHandler(repo,owner,{processing:makeProcessingService(repo,{providers})});cleanup.push(api.dispose)
 const response=await api.handler(new Request(`http://localhost/api/v1/notes/${note.id}/generate/stream`,{method:'POST',headers:{'Content-Type':'application/json',Authorization:`Bearer ${owner}`},body:JSON.stringify({expectedTranscriptRevision:note.transcriptRevision})}))
 expect(response.status).toBe(200)
 const events=(await response.text()).split('\n\n').map(frame=>frame.split('\n').filter(line=>line.startsWith('data:')).map(line=>line.slice(5).trimStart()).join('\n')).filter(Boolean).map(data=>JSON.parse(data))
 // One delta per piece the model sent, not one delta holding everything.
 expect(events.filter(event=>event.type==='delta').map(event=>event.text)).toEqual(PIECES)
 expect(events.at(-1).type).toBe('done')
 expect(events.at(-1).note.enhanced).toContain('- Budget: $3,000')
 // Exactly one provider request, and it was the streamed one.
 expect(bodies.map(body=>body.stream===true)).toEqual([true])
})

// Claude and Gemini on OpenRouter think before answering unless asked not to; seconds before the first word.
test('OpenRouter requests to models that think by default ask for no reasoning, or the minimum where it is mandatory',async()=>{
 const {languageTransport}=await import('../src/language-transport')
 const sent:any[]=[]
 const raw=Object.assign(async(_input:any,init?:RequestInit)=>{const body=JSON.parse(String(init?.body));sent.push(body)
  if(body.model==='google/gemini-test'&&body.reasoning?.enabled===false)return Response.json({error:{message:'Reasoning is mandatory for this endpoint and cannot be disabled.',code:400}},{status:400})
  return Response.json({choices:[{message:{content:'ok'}}]})},{preconnect:()=>{}}) as typeof fetch
 const transport=languageTransport(raw,'openrouter','router-key')
 const ask=(body:Record<string,unknown>)=>transport('https://openrouter.ai/api/v1/chat/completions',{method:'POST',body:JSON.stringify({messages:[],...body})})
 expect((await ask({model:'anthropic/claude-haiku-test'})).status).toBe(200)
 expect(sent.at(-1).reasoning).toEqual({enabled:false})
 expect((await ask({model:'google/gemini-test'})).status).toBe(200)
 expect(sent.slice(-2).map(body=>body.reasoning)).toEqual([{enabled:false},{effort:'minimal'}])
 // Remembered: the next request goes straight to the minimum.
 sent.length=0;await ask({model:'google/gemini-test'});expect(sent.map(body=>body.reasoning)).toEqual([{effort:'minimal'}])
 // Requests that choose their own reasoning, and other families, are sent unchanged.
 sent.length=0;await ask({model:'anthropic/claude-haiku-test',reasoning:{effort:'low'}});await ask({model:'openai/gpt-6-sol',reasoning_effort:'none'});await ask({model:'openai/gpt-4o-mini'})
 expect(sent.map(body=>body.reasoning??null)).toEqual([{effort:'low'},null,null])
})
