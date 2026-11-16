import {ProviderFailure} from './providers'
import {anthropicUsage} from './ai/prompt-cache'
import type {LanguageModelSelection} from '@meeting-notes/contracts'
const object=(v:unknown):v is Record<string,any>=>typeof v==='object'&&v!==null&&!Array.isArray(v)
/** Message content as Anthropic text blocks. A string is one block; parts keep their `cache_control`
 * marker (`ai/prompt-cache.ts` puts it on the last part of a message that ends a reusable prefix). */
const textBlocks=(content:unknown):any[]=>Array.isArray(content)
  ? content.filter(part=>object(part)&&typeof part.text==='string'&&part.text).map(part=>({type:'text',text:part.text,...(part.cache_control?{cache_control:part.cache_control}:{})}))
  : content?[{type:'text',text:content}]:[]

/** Adapt the provider wire format once; grounding, output validation and abort
 * handling remain shared by chat, generation, evidence and retrieval tools. */
/** Top-level fields of a chat completion request that every OpenAI-compatible server understands. Anything else
 * (OpenRouter routing, Anthropic or OpenAI cache fields, reasoning settings, `stream_options`) is left out: a
 * strict server answers an unknown field with 400. */
const COMPATIBLE_FIELDS=['model','messages','stream','max_tokens','temperature','response_format','tools','tool_choice'] as const
const MESSAGE_FIELDS=['role','content','tool_calls','tool_call_id'] as const
/** The request as plain OpenAI chat completions: message content as one string (cache markers dropped with the
 * parts that carried them), `max_completion_tokens` as the older `max_tokens`, and only the fields above. */
export function compatibleBody(body:Record<string,any>):Record<string,unknown> {
  const plain:Record<string,any>={...body,...(body.max_tokens===undefined&&body.max_completion_tokens!==undefined?{max_tokens:body.max_completion_tokens}:{})}
  const messages=(Array.isArray(body.messages)?body.messages:[]).map((message:Record<string,any>)=>Object.fromEntries(MESSAGE_FIELDS.filter(field=>message[field]!==undefined).map(field=>[field,
    field!=='content'||!Array.isArray(message.content)?message[field]:message.content.filter((part:unknown)=>object(part)&&typeof part.text==='string').map((part:any)=>part.text).join('')])))
  return Object.fromEntries(COMPATIBLE_FIELDS.filter(field=>plain[field]!==undefined).map(field=>[field,field==='messages'?messages:plain[field]]))
}
/** A streamed answer passed through as it comes. Some servers end the stream after the last chunk without the
 * closing `data: [DONE]`; when a chunk has already said the answer finished, the line is added so the reader
 * knows the answer is whole. A stream that stops before that stays incomplete. */
function closedStream(body:ReadableStream<Uint8Array>):ReadableStream<Uint8Array> {
  const reader=body.getReader(),decoder=new TextDecoder(),encoder=new TextEncoder();let tail='',finished=false,done=false
  return new ReadableStream<Uint8Array>({
    async pull(controller){
      try{
        const next=await reader.read()
        if(next.done){if(finished&&!done)controller.enqueue(encoder.encode('\n\ndata: [DONE]\n\n'));controller.close();return}
        controller.enqueue(next.value)
        tail=(tail+decoder.decode(next.value,{stream:true})).replace(/\r\n/g,'\n')
        let end:number
        while((end=tail.indexOf('\n\n'))>=0){
          const data=tail.slice(0,end).split('\n').filter(line=>line.startsWith('data:')).map(line=>line.slice(5).trimStart()).join('\n');tail=tail.slice(end+2)
          if(data==='[DONE]'){done=true;continue}
          try{const raw=JSON.parse(data);if(Array.isArray(raw?.choices)&&raw.choices.some((choice:any)=>typeof choice?.finish_reason==='string'&&choice.finish_reason))finished=true}catch{/* the reader judges it */}
        }
        if(tail.length>1_000_000)tail=tail.slice(-100_000)
      }catch(error){controller.error(error);await reader.cancel().catch(()=>{})}
    },
    cancel:()=>reader.cancel(),
  })
}

/** Adapt the provider wire format once; grounding, output validation and abort
 * handling remain shared by chat, generation, evidence and retrieval tools. `base` is where an OpenAI-compatible
 * server's API starts (already checked); its key is optional and sent only when there is one. */
export function languageTransport(transport:typeof fetch,provider:LanguageModelSelection['provider'],key:string,base=''):typeof fetch {
  if(provider==='openrouter')return transport
  return Object.assign(async(input:Parameters<typeof fetch>[0],init?:RequestInit)=>{
    if(String(input)!=='https://openrouter.ai/api/v1/chat/completions')return transport(input,init)
    const body=JSON.parse(String(init?.body))
    if(provider==='openai_compatible'){
      if(!base)throw new ProviderFailure('language','not_configured')
      const response=await transport(base+'/chat/completions',{...init,redirect:'error',headers:{...(key?{Authorization:'Bearer '+key}:{}),'Content-Type':'application/json'},body:JSON.stringify(compatibleBody(body))})
      return body.stream&&response.ok&&response.body?new Response(closedStream(response.body),{status:response.status,headers:response.headers}):response
    }
    // A streamed chat completion reports usage (and so cached tokens) only when asked.
    if(provider==='openai')return transport('https://api.openai.com/v1/chat/completions',{...init,headers:{Authorization:'Bearer '+key,'Content-Type':'application/json'},body:JSON.stringify(body.stream?{...body,stream_options:{include_usage:true}}:body)})
    const messages:Array<{role:string;content:any[]}>=[],system:any[]=[]
    for(const message of body.messages){
      if(message.role==='system'){system.push(...(Array.isArray(message.content)?textBlocks(message.content):[{type:'text',text:message.content??''}]));continue}
      const role=message.role==='assistant'?'assistant':'user',content:any[]=[]
      if(message.role==='tool')content.push({type:'tool_result',tool_use_id:message.tool_call_id,content:message.content??''})
      else {
        content.push(...textBlocks(message.content))
        for(const call of message.tool_calls??[])content.push({type:'tool_use',id:call.id,name:call.function.name,input:JSON.parse(call.function.arguments)})
      }
      if(!content.length)continue
      if(messages.at(-1)?.role===role)messages.at(-1)!.content.push(...content)
      else messages.push({role,content})
    }
    // Without a cache marker the system prompt stays the plain string it has always been.
    const systemField=system.some(block=>block.cache_control)?system:system.map(block=>block.text).join('\n\n')
    const response=await transport('https://api.anthropic.com/v1/messages',{...init,headers:{'x-api-key':key,'anthropic-version':'2023-06-01','Content-Type':'application/json'},body:JSON.stringify({model:body.model,max_tokens:body.max_completion_tokens??body.max_tokens??4096,system:systemField,messages,stream:body.stream??false,...(body.cache_control?{cache_control:body.cache_control}:{}),...(body.tools?.length?{tools:body.tools.map((t:any)=>({name:t.function.name,description:t.function.description,input_schema:t.function.parameters})),tool_choice:{type:'auto',disable_parallel_tool_use:true}}:{})})})
    if(!response.ok)return response
    if(!body.stream){
      const raw:unknown=await response.json();if(!object(raw)||!Array.isArray(raw.content))throw new ProviderFailure('language','invalid_response')
      const usage=anthropicUsage(raw.usage)
      return Response.json({choices:[{finish_reason:raw.stop_reason==='max_tokens'?'length':'stop',message:{content:raw.content.filter((b:any)=>b.type==='text').map((b:any)=>b.text).join(''),tool_calls:raw.content.filter((b:any)=>b.type==='tool_use').map((b:any)=>({id:b.id,type:'function',function:{name:b.name,arguments:JSON.stringify(b.input)}}))}}],...(usage?{usage}:{})})
    }
    if(!response.body)throw new ProviderFailure('language','invalid_response')
    const reader=response.body.getReader(),decoder=new TextDecoder(),encoder=new TextEncoder();let buffer='',done=false,usage:Record<string,unknown>={}
    const stream=new ReadableStream<Uint8Array>({
      async pull(controller){try{while(!done){const next=await reader.read();if(next.done){if(!done)throw new ProviderFailure('language','invalid_response');break}buffer=(buffer+decoder.decode(next.value,{stream:true})).replace(/\r\n/g,'\n');if(buffer.length>1_000_000)throw new ProviderFailure('language','invalid_response');let end:number;let emitted=false
        while((end=buffer.indexOf('\n\n'))>=0){const frame=buffer.slice(0,end);buffer=buffer.slice(end+2);const data=frame.split('\n').filter(l=>l.startsWith('data:')).map(l=>l.slice(5).trimStart()).join('\n');if(!data)continue;const raw=JSON.parse(data)
          if(raw.type==='error'||raw.type==='message_delta'&&raw.delta?.stop_reason==='max_tokens')throw new ProviderFailure('language')
          // Input and cache counts arrive with the first event, the output count with the last.
          if(raw.type==='message_start'&&object(raw.message?.usage))usage={...usage,...raw.message.usage}
          if(raw.type==='message_delta'&&object(raw.usage))usage={...usage,...raw.usage}
          if(raw.type==='content_block_delta'&&raw.delta?.type==='text_delta'){controller.enqueue(encoder.encode('data: '+JSON.stringify({choices:[{delta:{content:raw.delta.text}}]})+'\n\n'));emitted=true}
          if(raw.type==='message_stop'){done=true;const reported=anthropicUsage(usage);if(reported)controller.enqueue(encoder.encode('data: '+JSON.stringify({choices:[],usage:reported})+'\n\n'));controller.enqueue(encoder.encode('data: [DONE]\n\n'));controller.close();await reader.cancel();return}
        }if(emitted)return
      }}catch(error){controller.error(error);await reader.cancel().catch(()=>{})}},
      cancel:()=>reader.cancel(),
    })
    return new Response(stream,{headers:{'Content-Type':'text/event-stream'}})
  },{preconnect:transport.preconnect}) as typeof fetch
}
