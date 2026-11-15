/** Wraps the transport handed to `makeProviderServices`, so every request the
 * real code sends is timed and metered without changing it. Also provides the
 * offline stub transport used by `--stub` and the harness smoke test. */
import {createHash} from "node:crypto"
import type {CallMetric} from "./types"

/** USD per million tokens, from the public OpenRouter model list on 2026-11-13.
 * Used only when the provider response carries no cost of its own. */
export const PRICES:Record<string,{input:number;output:number}>={
  "openai/gpt-6-sol":{input:2,output:10},"openai/gpt-6.1-sol":{input:2,output:10},"openai/gpt-6.1-terra":{input:1,output:5},"openai/gpt-6-luna":{input:0.1,output:0.5},"openai/gpt-4o-mini":{input:0.15,output:0.6},
  "anthropic/claude-sonnet-5.5":{input:2,output:10},"anthropic/claude-sonnet-5":{input:2,output:10},"anthropic/claude-haiku-5.5":{input:0.1,output:0.5},"anthropic/claude-opus-5.5":{input:4,output:20},
  "google/gemini-3.5-flash":{input:1.5,output:9},"google/gemini-4-hydrogen":{input:1.5,output:7.5},
}
const priceFor=(model:string)=>PRICES[model]??PRICES[`openai/${model}`]??PRICES[`anthropic/${model.replace(/-(\d)-(\d)$/,"-$1.$2")}`]
const object=(value:unknown):value is Record<string,any>=>typeof value==="object"&&value!==null&&!Array.isArray(value)
const number=(value:unknown)=>typeof value==="number"&&Number.isFinite(value)?value:undefined

export interface Meter {
  transport:typeof fetch
  calls:CallMetric[]
  /** Label applied to requests that are neither planner (tools) nor streamed. */
  purpose:CallMetric["purpose"]
  /** Resolves when every streamed response has been fully metered. */
  settle:()=>Promise<void>
  /** Milliseconds from meter creation to the first streamed-answer request. */
  answerStartedMs:()=>number|null
}
export function meter(base:typeof fetch,purpose:CallMetric["purpose"]="other"):Meter {
  const calls:CallMetric[]=[],pending:Promise<void>[]=[],created=performance.now()
  let answerStarted:number|null=null
  const self:Meter={calls,purpose,settle:async()=>{await Promise.allSettled(pending)},answerStartedMs:()=>answerStarted,transport:undefined as unknown as typeof fetch}
  self.transport=(async(input:Parameters<typeof fetch>[0],init?:RequestInit)=>{
    const body=typeof init?.body==="string"?init.body:"",started=performance.now()
    let request:Record<string,any>={}
    try{request=JSON.parse(body)}catch{/* not a JSON request */}
    const kind:CallMetric["purpose"]=request.tools?"planner":request.stream?"answer":self.purpose
    if(kind==="answer"&&answerStarted===null)answerStarted=started-created
    const {messages:_m,system:_s,tools:_t,...parameters}=request
    const call:CallMetric={purpose:kind,model:String(request.model??""),ms:0,ttftMs:null,status:0,inputTokens:0,outputTokens:0,tokensEstimated:true,cachedTokens:0,cacheWriteTokens:0,costUsd:0,costEstimated:true,finishReason:null,
      parameters:{...parameters,...(request.tools?{toolCount:request.tools.length}:{})},requestSha256:createHash("sha256").update(body).digest("hex"),requestCharacters:body.length}
    calls.push(call)
    const finish=(outputCharacters:number,usage:Record<string,any>|undefined)=>{
      call.ms=Math.round(performance.now()-started)
      const input=number(usage?.prompt_tokens)??number(usage?.input_tokens),output=number(usage?.completion_tokens)??number(usage?.output_tokens)
      call.tokensEstimated=input===undefined||output===undefined
      // Anthropic counts cached tokens beside `input_tokens`; OpenAI-style usage counts them inside `prompt_tokens`.
      const read=number(usage?.prompt_tokens_details?.cached_tokens)??number(usage?.cache_read_input_tokens)??0,written=number(usage?.prompt_tokens_details?.cache_write_tokens)??number(usage?.cache_creation_input_tokens)??0
      call.cachedTokens=read;call.cacheWriteTokens=written
      call.inputTokens=input===undefined?Math.round(body.length/4):usage?.prompt_tokens===undefined?input+read+written:input;call.outputTokens=output??Math.round(outputCharacters/4)
      const reported=number(usage?.cost),price=priceFor(call.model)
      call.costEstimated=reported===undefined
      call.costUsd=reported??(price?(call.inputTokens*price.input+call.outputTokens*price.output)/1_000_000:0)
    }
    let response:Response
    try{response=await base(input,init)}catch(error){call.ms=Math.round(performance.now()-started);call.finishReason="transport_error";throw error}
    call.status=response.status
    if(!response.ok||!response.body){finish(0,undefined);return response}
    if(!request.stream){
      const text=await response.text()
      let raw:unknown;try{raw=JSON.parse(text)}catch{raw=null}
      const choice=object(raw)&&Array.isArray(raw.choices)&&object(raw.choices[0])?raw.choices[0]:undefined
      call.finishReason=choice?String(choice.finish_reason??""):object(raw)?String(raw.stop_reason??""):null
      const content=choice&&object(choice.message)?String(choice.message.content??"")+JSON.stringify(choice.message.tool_calls??""):object(raw)&&Array.isArray(raw.content)?JSON.stringify(raw.content):""
      finish(content.length,object(raw)&&object(raw.usage)?raw.usage:undefined)
      return new Response(text,{status:response.status,headers:response.headers})
    }
    const [forCaller,forMeter]=response.body.tee()
    pending.push((async()=>{
      const reader=forMeter.getReader(),decoder=new TextDecoder()
      let buffer="",characters=0,usage:Record<string,any>|undefined
      try{
        while(true){
          const next=await reader.read()
          if(next.done)break
          buffer=(buffer+decoder.decode(next.value,{stream:true})).replace(/\r\n/g,"\n")
          let end:number
          while((end=buffer.indexOf("\n\n"))>=0){
            const frame=buffer.slice(0,end);buffer=buffer.slice(end+2)
            const data=frame.split("\n").filter(line=>line.startsWith("data:")).map(line=>line.slice(5).trimStart()).join("\n")
            if(!data||data==="[DONE]")continue
            let raw:unknown;try{raw=JSON.parse(data)}catch{continue}
            if(!object(raw))continue
            // OpenAI-compatible frames, then Anthropic's own event shapes.
            const choice=Array.isArray(raw.choices)&&object(raw.choices[0])?raw.choices[0]:undefined
            const text=choice&&object(choice.delta)&&typeof choice.delta.content==="string"?choice.delta.content:raw.type==="content_block_delta"&&object(raw.delta)&&typeof raw.delta.text==="string"?raw.delta.text:""
            if(text){characters+=text.length;if(call.ttftMs===null)call.ttftMs=Math.round(performance.now()-started)}
            if(choice?.finish_reason)call.finishReason=String(choice.finish_reason)
            if(object(raw.usage))usage={...usage,...raw.usage}
            if(raw.type==="message_start"&&object(raw.message)&&object(raw.message.usage))usage={...usage,...raw.message.usage}
            if(raw.type==="message_delta"&&object(raw.delta)&&raw.delta.stop_reason)call.finishReason=String(raw.delta.stop_reason)
          }
        }
      }catch{/* the caller sees the same failure through its own branch */}
      finish(characters,usage)
    })())
    return new Response(forCaller,{status:response.status,headers:response.headers})
  }) as typeof fetch
  return self
}

/** Deterministic offline provider. It answers in the OpenAI-compatible wire
 * format so the whole pipeline (planning loop, streaming, citations, scoring,
 * judging, reporting) can be exercised without credentials or spend. */
export const stubTransport:typeof fetch=(async(_input:Parameters<typeof fetch>[0],init?:RequestInit)=>{
  const request=JSON.parse(String(init?.body)) as Record<string,any>
  const usage={prompt_tokens:Math.round(String(init?.body).length/4),completion_tokens:40,cost:0}
  const text=JSON.stringify(request.messages??"")
  if(request.tools){
    const done=(request.messages as Array<Record<string,any>>).filter(message=>message.role==="tool").length
    const ids=[...new Set([...text.matchAll(/\\"noteId\\":\\"([0-9a-f-]{36})\\"/g)].map(match=>match[1]!))].slice(0,3)
    const calls=done===0?[{id:"stub_1",type:"function",function:{name:"list_meetings",arguments:JSON.stringify({limit:6})}}]
      :done===1&&ids.length?[{id:"stub_2",type:"function",function:{name:"get_meetings",arguments:JSON.stringify({noteIds:ids})}}]:undefined
    return Response.json({choices:[{finish_reason:calls?"tool_calls":"stop",message:{content:calls?null:"READY",...(calls?{tool_calls:calls}:{})}}],usage})
  }
  if(/Return only a JSON object with this shape/.test(text)){
    const score=(value:number)=>({score:value,reason:"Stub judgement."})
    return Response.json({choices:[{finish_reason:"stop",message:{content:JSON.stringify({scores:{faithfulness:score(3),coverage:score(3),usefulness:score(3),concision:score(3),notesFidelity:score(3),tone:score(3)},unsupportedClaims:[],citations:[],overall:"Stub judgement; not a quality measurement."})}}],usage})
  }
  const source=/\[\[S1\]\]|\\"id\\":\\"S1\\"/.test(text)&&/issuedSources/.test(text)?" [[S1]]":""
  if(request.stream){
    const frames=["# Stub answer\n\n","- This output comes from the offline stub, not a model."+source,"\n"].map(content=>`data: ${JSON.stringify({choices:[{delta:{content}}]})}\n\n`)
    return new Response(frames.join("")+`data: ${JSON.stringify({choices:[],usage})}\n\ndata: [DONE]\n\n`,{headers:{"Content-Type":"text/event-stream"}})
  }
  if(request.response_format)return Response.json({choices:[{finish_reason:"stop",message:{content:JSON.stringify({summary:"Stub",segmentIds:[]})}}],usage})
  return Response.json({choices:[{finish_reason:"stop",message:{content:"# Stub notes\n\n- This output comes from the offline stub, not a model."}}],usage})
}) as typeof fetch
