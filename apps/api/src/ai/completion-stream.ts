/**
 * One streamed chat completion at the provider boundary, for a feature that wants the model's text as it
 * is written (the source summary, `evidence.ts`). It sends the same request as the provider's buffered
 * `complete` plus `stream: true`, so a feature that is sometimes streamed and sometimes not shares one
 * cached prefix, and it reads the stream exactly as the chat answer does: OpenAI-style SSE frames (other
 * providers are translated to them by `language-transport.ts`), `[DONE]` required, a cut-off answer
 * refused.
 */
import {ProviderFailure} from "../providers"
import {cachedRequest,readUsage,type LanguageUsage,type LanguageUsageEvent} from "./prompt-cache"
import {completionSettings,type LanguageMessage,type LanguageRequestParameters} from "./shared"

export interface CompletionStreamTarget {
  readonly transport:typeof fetch
  /** False when the provider has no key (or address): the request is never sent. */
  readonly ready:boolean
  /** Request headers besides the content type: the shared request's Authorization, when the provider takes one. */
  readonly headers:Record<string,string>
  readonly provider:Parameters<typeof cachedRequest>[0]
  readonly model:string
  readonly onUsage:(event:LanguageUsageEvent)=>void
}
const object=(v:unknown):v is Record<string,unknown>=>typeof v==="object"&&v!==null&&!Array.isArray(v)
const LIMIT=1_000_000

export async function* streamCompletion(target:CompletionStreamTarget,messages:ReadonlyArray<LanguageMessage>,parameters:LanguageRequestParameters,
  request:{feature:string;json?:boolean;signal:AbortSignal;timeoutMs?:number}):AsyncGenerator<string> {
  if(!target.ready)throw new ProviderFailure("language","not_configured")
  const wire=cachedRequest(target.provider,target.model,messages,parameters.cache)
  const started=performance.now()
  let usage:LanguageUsage|null=null,firstTokenMs:number|null=null
  const response=await target.transport("https://openrouter.ai/api/v1/chat/completions",{
    method:"POST",headers:{...target.headers,"Content-Type":"application/json"},
    body:JSON.stringify({model:target.model,messages:wire.messages,...completionSettings(target.model,parameters.maxTokens,parameters.temperature),
      ...(request.json?{response_format:{type:"json_object"}}:{}),...wire.extra,stream:true}),
    signal:AbortSignal.any([request.signal,AbortSignal.timeout(request.timeoutMs??90_000)]),redirect:"error",
  })
  if(!response.ok||!response.body)throw new ProviderFailure("language")
  const reader=response.body.getReader(),decoder=new TextDecoder()
  let buffer="",characters=0,complete=false
  try{
    while(!complete){
      const next=await reader.read()
      if(next.done)break
      buffer=(buffer+decoder.decode(next.value,{stream:true})).replace(/\r\n/g,"\n")
      if(buffer.length>LIMIT)throw new ProviderFailure("language","invalid_response")
      let end:number
      while((end=buffer.indexOf("\n\n"))>=0){
        const frame=buffer.slice(0,end);buffer=buffer.slice(end+2)
        const data=frame.split("\n").filter(line=>line.startsWith("data:")).map(line=>line.slice(5).trimStart()).join("\n")
        if(!data)continue
        if(data==="[DONE]"){complete=true;break}
        let raw:unknown;try{raw=JSON.parse(data)}catch{throw new ProviderFailure("language","invalid_response")}
        if(!object(raw)||raw.error)throw new ProviderFailure("language")
        usage=readUsage(raw.usage)??usage
        if(!Array.isArray(raw.choices)||raw.choices.length===0)continue // Usage-only final frame.
        const choice=raw.choices[0]
        if(!object(choice)||choice.finish_reason==="length")throw new ProviderFailure("language","invalid_response")
        const text=object(choice.delta)?choice.delta.content:undefined
        if(typeof text==="string"&&text){
          characters+=text.length;if(characters>LIMIT)throw new ProviderFailure("language","invalid_response")
          firstTokenMs??=Math.round(performance.now()-started)
          yield text
        }
      }
    }
    if(!complete||characters===0)throw new ProviderFailure("language","invalid_response")
  }finally{
    await reader.cancel().catch(()=>{})
    if(firstTokenMs!==null)target.onUsage({feature:request.feature,provider:target.provider,model:target.model,usage,firstTokenMs,totalMs:Math.round(performance.now()-started)})
  }
}
