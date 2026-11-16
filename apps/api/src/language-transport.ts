import {ProviderFailure,readJsonCapped} from './providers'
import {anthropicUsage} from './ai/prompt-cache'
import {reasoningCapable} from './ai/shared'
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


// ---------------------------------------------------------------- Reasoning (chat answers and research steps)
//
// OpenRouter and OpenAI-compatible servers send a model's reasoning in the chat completions answer by themselves.
// OpenAI and Anthropic have to be asked: OpenAI only gives reasoning summaries through its Responses API, and
// Anthropic only thinks aloud when the request says so. Both are translated here, so the callers keep reading
// chat completions with the reasoning text in `delta.reasoning` (streamed) or `message.reasoning`.

/** A provider that refused a reasoning request (an organisation not allowed reasoning summaries, a model that
 * takes no thinking field) is asked the plain way for the next ten minutes, instead of being refused once per question. */
const refusedReasoning=new Map<string,number>(),REFUSED_MS=600_000
const refusalMark=(provider:string,key:string,model:string)=>`${provider}:${Bun.hash(key)}:${model}`
const refusedBefore=(mark:string)=>{const until=refusedReasoning.get(mark);if(until===undefined)return false;if(until>Date.now())return true;refusedReasoning.delete(mark);return false}
const plainText=(content:unknown):string=>typeof content==='string'?content:Array.isArray(content)?content.filter(part=>object(part)&&typeof part.text==='string').map(part=>part.text).join(''):''

/** A chat completions request as an OpenAI Responses request that asks for a reasoning summary. Nothing is stored
 * at OpenAI, so an earlier tool call is sent as what was called and what came back, without a response item id. */
export function responsesBody(body:Record<string,any>):Record<string,unknown> {
  const input:Array<Record<string,unknown>>=[]
  for(const message of Array.isArray(body.messages)?body.messages:[]){
    if(message.role==='tool'){input.push({type:'function_call_output',call_id:message.tool_call_id,output:plainText(message.content)});continue}
    const assistant=message.role==='assistant'
    // Text parts keep their cache breakpoint (`ai/prompt-cache.ts`); Responses takes it on input text only.
    const content=Array.isArray(message.content)
      ? message.content.filter((part:unknown)=>object(part)&&typeof part.text==='string').map((part:any)=>assistant?{type:'output_text',text:part.text}:{type:'input_text',text:part.text,...(part.prompt_cache_breakpoint?{prompt_cache_breakpoint:part.prompt_cache_breakpoint}:{})})
      : message.content
    if(typeof content==='string'?content:content?.length)input.push({role:message.role,content})
    for(const call of message.tool_calls??[])input.push({type:'function_call',call_id:call.id,name:call.function.name,arguments:call.function.arguments})
  }
  const limit=body.max_completion_tokens??body.max_tokens
  return {model:body.model,input,stream:!!body.stream,store:false,
    reasoning:{summary:'auto',...(body.reasoning_effort?{effort:body.reasoning_effort}:{})},
    ...(limit!==undefined?{max_output_tokens:limit}:{}),
    // Not strict: the tools' optional arguments are optional.
    ...(body.tools?.length?{tools:body.tools.map((tool:any)=>({type:'function',name:tool.function.name,description:tool.function.description,parameters:tool.function.parameters,strict:false})),
      ...(body.tool_choice!==undefined?{tool_choice:body.tool_choice}:{}),...(body.parallel_tool_calls!==undefined?{parallel_tool_calls:body.parallel_tool_calls}:{})}:{}),
    ...(body.response_format?.type==='json_object'?{text:{format:{type:'json_object'}}}:{}),
    ...Object.fromEntries(['prompt_cache_key','prompt_cache_retention','prompt_cache_options'].filter(field=>body[field]!==undefined).map(field=>[field,body[field]])),
  }
}
/** Responses usage in the chat completions shape `readUsage` reads. */
function responsesUsage(raw:unknown):Record<string,unknown>|null {
  if(!object(raw)||typeof raw.input_tokens!=='number')return null
  const details=object(raw.input_tokens_details)?raw.input_tokens_details:{}
  return {prompt_tokens:raw.input_tokens,completion_tokens:typeof raw.output_tokens==='number'?raw.output_tokens:0,
    prompt_tokens_details:{cached_tokens:typeof details.cached_tokens==='number'?details.cached_tokens:0,...(typeof details.cache_write_tokens==='number'?{cache_write_tokens:details.cache_write_tokens}:{})}}
}
/** A finished (not streamed) Responses answer as a chat completion: text, tool calls, and the reasoning summary as `message.reasoning`. */
export function responsesCompletion(raw:unknown):Record<string,unknown> {
  if(!object(raw)||!Array.isArray(raw.output)||raw.status==='failed'||raw.error)throw new ProviderFailure('language','invalid_response')
  const items:any[]=raw.output.filter(object),parts=(item:any,field:string):any[]=>Array.isArray(item[field])?item[field].filter(object):[]
  const content=items.filter(item=>item.type==='message').flatMap(item=>parts(item,'content')).filter(part=>part.type==='output_text'&&typeof part.text==='string').map(part=>part.text).join('')
  const reasoning=items.filter(item=>item.type==='reasoning').flatMap(item=>parts(item,'summary')).map(part=>typeof part.text==='string'?part.text.trim():'').filter(Boolean).join('\n\n')
  const calls=items.filter(item=>item.type==='function_call').map(item=>({id:item.call_id,type:'function',function:{name:item.name,arguments:item.arguments}}))
  const usage=responsesUsage(raw.usage)
  return {choices:[{finish_reason:raw.status==='incomplete'?'length':calls.length?'tool_calls':'stop',message:{content,...(calls.length?{tool_calls:calls}:{}),...(reasoning?{reasoning}:{})}}],...(usage?{usage}:{})}
}
/** A provider's event stream rewritten as chat completion frames. `each` is given every event and what to send for
 * it; it returns true at the event that ends the answer and throws for one that fails it. A stream that stops before
 * its end is an invalid response, as everywhere else. */
function translatedStream(body:ReadableStream<Uint8Array>,each:(raw:any,send:(frame:unknown)=>void)=>boolean):Response {
  const reader=body.getReader(),decoder=new TextDecoder(),encoder=new TextEncoder();let buffer='',done=false
  return new Response(new ReadableStream<Uint8Array>({
    async pull(controller){
      try{
        let sent=false;const send=(frame:unknown)=>{controller.enqueue(encoder.encode('data: '+JSON.stringify(frame)+'\n\n'));sent=true}
        while(!done&&!sent){
          const next=await reader.read();if(next.done)throw new ProviderFailure('language','invalid_response')
          buffer=(buffer+decoder.decode(next.value,{stream:true})).replace(/\r\n/g,'\n');if(buffer.length>1_000_000)throw new ProviderFailure('language','invalid_response')
          let end:number
          while(!done&&(end=buffer.indexOf('\n\n'))>=0){
            const data=buffer.slice(0,end).split('\n').filter(line=>line.startsWith('data:')).map(line=>line.slice(5).trimStart()).join('\n');buffer=buffer.slice(end+2)
            if(!data||data==='[DONE]')continue
            let raw:unknown;try{raw=JSON.parse(data)}catch{throw new ProviderFailure('language','invalid_response')}
            done=each(raw,send)
          }
        }
        if(done){controller.enqueue(encoder.encode('data: [DONE]\n\n'));controller.close();await reader.cancel().catch(()=>{})}
      }catch(error){controller.error(error);await reader.cancel().catch(()=>{})}
    },
    cancel:()=>reader.cancel(),
  }),{headers:{'Content-Type':'text/event-stream'}})
}
/** A streamed Responses answer as chat completion frames: summary text as `delta.reasoning` (separate parts of a
 * summary a paragraph apart), answer text as `delta.content`, then the usage. An answer cut off at its output
 * budget is refused like `finish_reason: "length"`. */
function responsesStream(body:ReadableStream<Uint8Array>):Response {
  let reasoned=false,gap=false
  return translatedStream(body,(raw,send)=>{
    const type=object(raw)?raw.type:undefined
    if(type==='error'||type==='response.failed')throw new ProviderFailure('language')
    if(type==='response.incomplete')throw new ProviderFailure('language','invalid_response')
    if(type==='response.reasoning_summary_part.added')gap=reasoned
    if(type==='response.reasoning_summary_text.delta'&&typeof raw.delta==='string'&&raw.delta){send({choices:[{delta:{reasoning:(gap?'\n\n':'')+raw.delta}}]});gap=false;reasoned=true}
    if(type==='response.output_text.delta'&&typeof raw.delta==='string'&&raw.delta)send({choices:[{delta:{content:raw.delta}}]})
    if(type!=='response.completed')return false
    const usage=responsesUsage(raw.response?.usage);if(usage)send({choices:[],usage})
    return true
  })
}
/** How an Anthropic model is asked to think where its thinking can be shown: `adaptive` on Claude 4.6 and later
 * (a fixed budget is refused there, and the thinking text has to be asked for as a summary), a small fixed budget
 * on the models before, nothing on a model that does not think. */
const anthropicThinking=(model:string):'adaptive'|'budget'|null=>
  /^claude-(?:(?:fable|mythos)-|(?:opus|sonnet|haiku)-(?:[5-9]|4-[6-9])(?:-|$))/.test(model)?'adaptive'
  :/^claude-(?:(?:opus|sonnet|haiku)-4-5|opus-4-1|(?:opus|sonnet)-4-2025|3-7-sonnet)/.test(model)?'budget':null
/** Thinking is written before the answer and counts against `max_tokens`, so the answer's budget is raised by this
 * much when thinking is asked for (an answer cut off at its budget is discarded as a failure). */
const ADAPTIVE_THINKING_ROOM=8000,THINKING_BUDGET=2048

/** Adapt the provider wire format once; grounding, output validation and abort
 * handling remain shared by chat, generation, evidence and retrieval tools. `base` is where an OpenAI-compatible
 * server's API starts (already checked); its key is optional and sent only when there is one. `reasoning` marks
 * the requests whose reasoning is shown to the person asking (the chat answer and its research steps): only
 * those are sent in the form that makes OpenAI and Anthropic return it, and every other request is sent as before. */
export function languageTransport(transport:typeof fetch,provider:LanguageModelSelection['provider'],key:string,base='',options:{reasoning?:boolean}={}):typeof fetch {
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
    if(provider==='openai'){
      const headers={Authorization:'Bearer '+key,'Content-Type':'application/json'}
      const completions=()=>transport('https://api.openai.com/v1/chat/completions',{...init,headers,body:JSON.stringify(body.stream?{...body,stream_options:{include_usage:true}}:body)})
      // Chat completions never returns reasoning; the Responses API returns a summary of it.
      const mark=refusalMark(provider,key,String(body.model))
      if(!options.reasoning||!reasoningCapable(String(body.model))||refusedBefore(mark))return completions()
      const response=await transport('https://api.openai.com/v1/responses',{...init,headers,body:JSON.stringify(responsesBody(body))})
      if(!response.ok){
        // Chat must not depend on this: whatever the refusal, the same request is sent once the way it always was.
        await response.body?.cancel().catch(()=>{})
        if([400,403,404].includes(response.status))refusedReasoning.set(mark,Date.now()+REFUSED_MS)
        return completions()
      }
      if(!body.stream)return Response.json(responsesCompletion(await readJsonCapped(response)))
      if(!response.body)throw new ProviderFailure('language','invalid_response')
      return responsesStream(response.body)
    }
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
    // Thinking is asked for only where it can be shown and nothing has to be handed back: the streamed chat answer,
    // which calls no tools. In the research loop every thinking block would have to be returned with its tool call.
    const mark=refusalMark(provider,key,String(body.model)),outputLimit=body.max_completion_tokens??body.max_tokens??4096
    const thinking=options.reasoning&&body.stream&&!body.tools?.length&&!refusedBefore(mark)?anthropicThinking(String(body.model)):null
    const send=(think:typeof thinking)=>transport('https://api.anthropic.com/v1/messages',{...init,headers:{'x-api-key':key,'anthropic-version':'2023-06-01','Content-Type':'application/json'},body:JSON.stringify({model:body.model,max_tokens:outputLimit+(think==='adaptive'?ADAPTIVE_THINKING_ROOM:think==='budget'?THINKING_BUDGET:0),
      ...(think==='adaptive'?{thinking:{type:'adaptive',display:'summarized'}}:think==='budget'?{thinking:{type:'enabled',budget_tokens:THINKING_BUDGET}}:{}),
      system:systemField,messages,stream:body.stream??false,...(body.cache_control?{cache_control:body.cache_control}:{}),...(body.tools?.length?{tools:body.tools.map((t:any)=>({name:t.function.name,description:t.function.description,input_schema:t.function.parameters})),tool_choice:{type:'auto',disable_parallel_tool_use:true}}:{})})})
    let response=await send(thinking)
    // A model that takes no such field says 400: the answer is then asked for once more, exactly as it was before.
    if(thinking&&response.status===400){await response.body?.cancel().catch(()=>{});refusedReasoning.set(mark,Date.now()+REFUSED_MS);response=await send(null)}
    if(!response.ok)return response
    if(!body.stream){
      const raw:unknown=await response.json();if(!object(raw)||!Array.isArray(raw.content))throw new ProviderFailure('language','invalid_response')
      const usage=anthropicUsage(raw.usage)
      const thought=raw.content.filter((b:any)=>b.type==='thinking'&&typeof b.thinking==='string'&&b.thinking.trim()).map((b:any)=>b.thinking.trim()).join('\n\n')
      return Response.json({choices:[{finish_reason:raw.stop_reason==='max_tokens'?'length':'stop',message:{...(thought?{reasoning:thought}:{}),content:raw.content.filter((b:any)=>b.type==='text').map((b:any)=>b.text).join(''),tool_calls:raw.content.filter((b:any)=>b.type==='tool_use').map((b:any)=>({id:b.id,type:'function',function:{name:b.name,arguments:JSON.stringify(b.input)}}))}}],...(usage?{usage}:{})})
    }
    if(!response.body)throw new ProviderFailure('language','invalid_response')
    const reader=response.body.getReader(),decoder=new TextDecoder(),encoder=new TextEncoder();let buffer='',done=false,usage:Record<string,unknown>={},thought=false,gap=false
    const stream=new ReadableStream<Uint8Array>({
      async pull(controller){try{while(!done){const next=await reader.read();if(next.done){if(!done)throw new ProviderFailure('language','invalid_response');break}buffer=(buffer+decoder.decode(next.value,{stream:true})).replace(/\r\n/g,'\n');if(buffer.length>1_000_000)throw new ProviderFailure('language','invalid_response');let end:number;let emitted=false
        while((end=buffer.indexOf('\n\n'))>=0){const frame=buffer.slice(0,end);buffer=buffer.slice(end+2);const data=frame.split('\n').filter(l=>l.startsWith('data:')).map(l=>l.slice(5).trimStart()).join('\n');if(!data)continue;const raw=JSON.parse(data)
          if(raw.type==='error'||raw.type==='message_delta'&&raw.delta?.stop_reason==='max_tokens')throw new ProviderFailure('language')
          // Input and cache counts arrive with the first event, the output count with the last.
          if(raw.type==='message_start'&&object(raw.message?.usage))usage={...usage,...raw.message.usage}
          if(raw.type==='message_delta'&&object(raw.usage))usage={...usage,...raw.usage}
          // Thinking text as `delta.reasoning`; a second thinking block starts a new paragraph.
          if(raw.type==='content_block_start'&&raw.content_block?.type==='thinking')gap=thought
          if(raw.type==='content_block_delta'&&raw.delta?.type==='thinking_delta'&&typeof raw.delta.thinking==='string'&&raw.delta.thinking){controller.enqueue(encoder.encode('data: '+JSON.stringify({choices:[{delta:{reasoning:(gap?'\n\n':'')+raw.delta.thinking}}]})+'\n\n'));emitted=true;gap=false;thought=true}
          if(raw.type==='content_block_delta'&&raw.delta?.type==='text_delta'){controller.enqueue(encoder.encode('data: '+JSON.stringify({choices:[{delta:{content:raw.delta.text}}]})+'\n\n'));emitted=true}
          if(raw.type==='message_stop'){done=true;const reported=anthropicUsage(usage);if(reported)controller.enqueue(encoder.encode('data: '+JSON.stringify({choices:[],usage:reported})+'\n\n'));controller.enqueue(encoder.encode('data: [DONE]\n\n'));controller.close();await reader.cancel();return}
        }if(emitted)return
      }}catch(error){controller.error(error);await reader.cancel().catch(()=>{})}},
      cancel:()=>reader.cancel(),
    })
    return new Response(stream,{headers:{'Content-Type':'text/event-stream'}})
  },{preconnect:transport.preconnect}) as typeof fetch
}
