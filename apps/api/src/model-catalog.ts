import {createHash} from 'node:crypto'
import type {AIProvider, AIModelDTO, ModelCatalogDTO} from '@meeting-notes/contracts'

export class CatalogFailure extends Error {
  /** `unreachable`: no answer at all (refused, timed out, no such host). `redirected`: the server answered with a
   * redirect, which is never followed. `status` is the HTTP status of a refusal. */
  constructor(readonly reason:'not_configured'|'unavailable'|'unreachable'|'redirected'|'invalid_response'='unavailable',readonly status?:number){super('The provider model list could not be loaded')}
}
/** Whether a failed fetch was a redirect refused by `redirect:'error'` (a server that did answer), rather than no
 * answer at all. Bun throws `UnexpectedRedirect`; Node's fetch fails with an "unexpected redirect" cause. */
export function isRedirectError(error:unknown):boolean{
  if(!(error instanceof Error))return false
  const cause=(error as {cause?:unknown}).cause,code=(error as {code?:unknown}).code
  return code==='UnexpectedRedirect'||/^UnexpectedRedirect\b|unexpected redirect/i.test(error.message)||cause instanceof Error&&/unexpected redirect/i.test(cause.message)
}
const record=(value:unknown):value is Record<string,unknown>=>typeof value==='object'&&value!==null&&!Array.isArray(value)
const identifier=(value:unknown):value is string=>typeof value==='string'&&/^[A-Za-z0-9_.:/-]{1,150}$/.test(value)
// ElevenLabs' /models endpoint currently lists voice models, not its batch Scribe
// models. Keep the documented batch options separate and disclose that limitation.
// The streaming Scribe models are not in that endpoint either. Both were called successfully on 2026-11-14;
// only v3 Live separates speakers.
const liveScribe:AIModelDTO[]=[{id:'scribe_v3_live',name:'Scribe v3 Live',capabilities:['live_transcription']},{id:'scribe_v2_realtime',name:'Scribe v2 Realtime',capabilities:['live_transcription']}]
// Speech is transcribed live only, so the catalog offers no batch speech model: ElevenLabs' batch Scribe and
// OpenAI's transcription models are left out, and no client can offer them.
export function makeModelCatalog(transport:typeof fetch=globalThis.fetch,clock:()=>Date=()=>new Date()) {
  const cache=new Map<string,{expires:number;value:ModelCatalogDTO}>(),pending=new Map<string,Promise<ModelCatalogDTO>>()
  async function load(provider:AIProvider,key:string,base:string):Promise<ModelCatalogDTO>{
    // An OpenAI-compatible server needs its address; a key only if it asks for one.
    if(provider==='openai_compatible'?!base:!key&&provider!=='openrouter')throw new CatalogFailure('not_configured')
    let url=provider==='openai_compatible'?base+'/models':provider==='openrouter'?'https://openrouter.ai/api/v1/models':provider==='openai'?'https://api.openai.com/v1/models':provider==='anthropic'?'https://api.anthropic.com/v1/models?limit=1000':'https://api.elevenlabs.io/v1/models'
    const headers:Record<string,string>=provider==='elevenlabs'?{'xi-api-key':key}:provider==='anthropic'?{'x-api-key':key,'anthropic-version':'2023-06-01'}:key?{Authorization:'Bearer '+key}:{}
    // OpenRouter's model list is public: it answers the same with a wrong key as with none. Its key endpoint is
    // what tells a real key from a mistyped one, so a key someone has just pasted is checked there first. Only a
    // clear refusal counts; if the check itself cannot be made, the list below decides as before.
    if(provider==='openrouter'&&key){
      const check=await transport('https://openrouter.ai/api/v1/key',{headers,redirect:'error',signal:AbortSignal.timeout(15_000)}).catch(()=>null)
      await check?.body?.cancel().catch(()=>{})
      if(check&&(check.status===401||check.status===403))throw new CatalogFailure()
    }
    const models:AIModelDTO[]=[];let notice:string|null=null
    for(let page=0;page<10;page++){
      const response=await transport(url,{headers,redirect:'error',signal:AbortSignal.timeout(15_000)}).catch(error=>{throw new CatalogFailure(isRedirectError(error)?'redirected':'unreachable')})
      // A transport that hands back the redirect itself (a 3xx) instead of failing on it: still not followed.
      if(response.status>=300&&response.status<400){await response.body?.cancel().catch(()=>{});throw new CatalogFailure('redirected',response.status)}
      if(!response.ok){await response.body?.cancel().catch(()=>{});throw new CatalogFailure('unavailable',response.status)}
      const reader=response.body?.getReader();if(!reader)throw new CatalogFailure('invalid_response')
      let text='',size=0;const decoder=new TextDecoder()
      try{while(true){const next=await reader.read();if(next.done)break;size+=next.value.byteLength;if(size>8_000_000)throw new CatalogFailure('invalid_response');text+=decoder.decode(next.value,{stream:true})}text+=decoder.decode()}finally{await reader.cancel().catch(()=>{})}
      let raw:unknown;try{raw=JSON.parse(text)}catch{throw new CatalogFailure('invalid_response')}
      const rows=provider==='elevenlabs'?raw:record(raw)?raw.data:null
      if(!Array.isArray(rows)||rows.length>10000)throw new CatalogFailure('invalid_response')
      for(const row of rows){
        if(!record(row))continue
        const id=provider==='elevenlabs'?row.model_id:row.id;if(!identifier(id)||row.lifecycle==='retired'||id.endsWith(':batch'))continue
        let capabilities:AIModelDTO['capabilities']=[]
        if(provider==='elevenlabs'){/* Batch speech models: not offered (see above). The live ones are added below. */}
        else if(provider==='openai'){
          if(/^(?:gpt-|o\d|chatgpt-|ft:(?:gpt-|o\d))/.test(id)&&!/(?:audio|realtime|live|image|transcribe|tts|search|instruct|codex)/.test(id))capabilities=['language']
        }else if(provider==='openrouter'){
          const architecture=record(row.architecture)?row.architecture:null
          const input=architecture?.input_modalities,output=architecture?.output_modalities
          if(Array.isArray(input)&&input.includes('text')&&Array.isArray(output)&&output.includes('text'))capabilities=['language']
        }else capabilities=['language'] // Anthropic, and an OpenAI-compatible server: everything it serves is for chat.
        if(capabilities.length)models.push({id,name:typeof row.display_name==='string'?row.display_name:typeof row.name==='string'?row.name:id,capabilities})
      }
      if(provider!=='anthropic'||!record(raw)||raw.has_more!==true)break
      if(page===9||!identifier(raw.last_id))throw new CatalogFailure('invalid_response')
      url='https://api.anthropic.com/v1/models?limit=1000&after_id='+encodeURIComponent(raw.last_id)
    }
    if(provider==='openai_compatible'&&!models.length)notice='This server did not list any models. Type the model name it uses instead.'
    if(provider==='elevenlabs')notice='ElevenLabs does not list its live Scribe models in its model API. These are the supported live models.'
    if(provider==='elevenlabs')for(const model of liveScribe)if(!models.some(m=>m.id===model.id))models.push(model)
    const unique=[...new Map(models.map(m=>[m.id,m])).values()].sort((a,b)=>a.name.localeCompare(b.name))
    return{provider,models:unique,fetchedAt:clock().toISOString(),notice}
  }
  /** `base` is where an OpenAI-compatible server's API starts, already checked (`@meeting-notes/contracts/compatible`). */
  return async(provider:AIProvider,key:string,refresh=false,base=''):Promise<ModelCatalogDTO>=>{
    const hash=createHash('sha256').update(provider+'\0'+key+'\0'+base).digest('hex'),hit=cache.get(hash)
    if(!refresh&&hit&&hit.expires>clock().getTime())return hit.value
    const existing=pending.get(hash);if(existing)return existing
    const task=load(provider,key,base).then(value=>{if(cache.size>=64)cache.delete(cache.keys().next().value!);cache.set(hash,{expires:clock().getTime()+300_000,value});return value}).catch(error=>{throw error instanceof CatalogFailure?error:new CatalogFailure()}).finally(()=>pending.delete(hash))
    pending.set(hash,task);return task
  }
}
