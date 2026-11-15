import {createHash} from 'node:crypto'
import type {AIProvider, AIModelDTO, ModelCatalogDTO} from '@meeting-notes/contracts'

export class CatalogFailure extends Error {
  constructor(readonly reason:'not_configured'|'unavailable'|'invalid_response'='unavailable'){super('The provider model list could not be loaded')}
}
const record=(value:unknown):value is Record<string,unknown>=>typeof value==='object'&&value!==null&&!Array.isArray(value)
const identifier=(value:unknown):value is string=>typeof value==='string'&&/^[A-Za-z0-9_.:/-]{1,150}$/.test(value)
// ElevenLabs' /models endpoint currently lists voice models, not its batch Scribe
// models. Keep the documented batch options separate and disclose that limitation.
// The streaming Scribe models are not in that endpoint either. Both were called successfully on 2026-11-14;
// only v3 Live separates speakers.
const liveScribe:AIModelDTO[]=[{id:'scribe_v3_live',name:'Scribe v3 Live',capabilities:['live_transcription']},{id:'scribe_v2_realtime',name:'Scribe v2 Realtime',capabilities:['live_transcription']}]
const batchScribe:AIModelDTO[]=[{id:'scribe_v3',name:'Scribe v3',capabilities:['transcription']},{id:'scribe_v2',name:'Scribe v2',capabilities:['transcription']},{id:'scribe_v1',name:'Scribe v1',capabilities:['transcription']}]
export function makeModelCatalog(transport:typeof fetch=globalThis.fetch,clock:()=>Date=()=>new Date()) {
  const cache=new Map<string,{expires:number;value:ModelCatalogDTO}>(),pending=new Map<string,Promise<ModelCatalogDTO>>()
  async function load(provider:AIProvider,key:string):Promise<ModelCatalogDTO>{
    if(!key&&provider!=='openrouter')throw new CatalogFailure('not_configured')
    let url=provider==='openrouter'?'https://openrouter.ai/api/v1/models':provider==='openai'?'https://api.openai.com/v1/models':provider==='anthropic'?'https://api.anthropic.com/v1/models?limit=1000':'https://api.elevenlabs.io/v1/models'
    const headers:Record<string,string>=provider==='elevenlabs'?{'xi-api-key':key}:provider==='anthropic'?{'x-api-key':key,'anthropic-version':'2023-06-01'}:key?{Authorization:'Bearer '+key}:{}
    const models:AIModelDTO[]=[];let notice:string|null=null
    for(let page=0;page<10;page++){
      const response=await transport(url,{headers,redirect:'error',signal:AbortSignal.timeout(15_000)})
      if(!response.ok)throw new CatalogFailure()
      const reader=response.body?.getReader();if(!reader)throw new CatalogFailure('invalid_response')
      let text='',size=0;const decoder=new TextDecoder()
      try{while(true){const next=await reader.read();if(next.done)break;size+=next.value.byteLength;if(size>8_000_000)throw new CatalogFailure('invalid_response');text+=decoder.decode(next.value,{stream:true})}text+=decoder.decode()}finally{await reader.cancel().catch(()=>{})}
      const raw:unknown=JSON.parse(text),rows=provider==='elevenlabs'?raw:record(raw)?raw.data:null
      if(!Array.isArray(rows)||rows.length>10000)throw new CatalogFailure('invalid_response')
      for(const row of rows){
        if(!record(row))continue
        const id=provider==='elevenlabs'?row.model_id:row.id;if(!identifier(id)||row.lifecycle==='retired'||id.endsWith(':batch'))continue
        let capabilities:AIModelDTO['capabilities']=[]
        if(provider==='elevenlabs'){if(row.can_do_speech_to_text===true||/^scribe_v\d+$/.test(id))capabilities=['transcription']}
        else if(provider==='openai'){
          if(/^(?:gpt-.*transcribe(?:-diarize)?(?:-\d{4}-\d{2}-\d{2})?|whisper-1)$/.test(id))capabilities=['transcription']
          else if(/^(?:gpt-|o\d|chatgpt-|ft:(?:gpt-|o\d))/.test(id)&&!/(?:audio|realtime|live|image|transcribe|tts|search|instruct|codex)/.test(id))capabilities=['language']
        }else if(provider==='openrouter'){
          const architecture=record(row.architecture)?row.architecture:null
          const input=architecture?.input_modalities,output=architecture?.output_modalities
          if(Array.isArray(input)&&input.includes('text')&&Array.isArray(output)&&output.includes('text'))capabilities=['language']
        }else capabilities=['language']
        if(capabilities.length)models.push({id,name:typeof row.display_name==='string'?row.display_name:typeof row.name==='string'?row.name:id,capabilities})
      }
      if(provider!=='anthropic'||!record(raw)||raw.has_more!==true)break
      if(page===9||!identifier(raw.last_id))throw new CatalogFailure('invalid_response')
      url='https://api.anthropic.com/v1/models?limit=1000&after_id='+encodeURIComponent(raw.last_id)
    }
    if(provider==='elevenlabs'&&!models.length){models.push(...batchScribe);notice='ElevenLabs does not list Scribe in its model API. These are the supported Scribe models.'}
    if(provider==='elevenlabs')for(const model of liveScribe)if(!models.some(m=>m.id===model.id))models.push(model)
    const unique=[...new Map(models.map(m=>[m.id,m])).values()].sort((a,b)=>a.name.localeCompare(b.name))
    return{provider,models:unique,fetchedAt:clock().toISOString(),notice}
  }
  return async(provider:AIProvider,key:string,refresh=false):Promise<ModelCatalogDTO>=>{
    const hash=createHash('sha256').update(provider+'\0'+key).digest('hex'),hit=cache.get(hash)
    if(!refresh&&hit&&hit.expires>clock().getTime())return hit.value
    const existing=pending.get(hash);if(existing)return existing
    const task=load(provider,key).then(value=>{if(cache.size>=64)cache.delete(cache.keys().next().value!);cache.set(hash,{expires:clock().getTime()+300_000,value});return value}).catch(error=>{throw error instanceof CatalogFailure?error:new CatalogFailure()}).finally(()=>pending.delete(hash))
    pending.set(hash,task);return task
  }
}
