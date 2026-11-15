import {ProviderFailure,type ProviderServices} from './providers'
import type {TranscriptSegmentDTO} from '@meeting-notes/contracts'
const object=(v:unknown):v is Record<string,unknown>=>typeof v==='object'&&v!==null&&!Array.isArray(v)
export async function transcribeOpenAi(input:Parameters<ProviderServices['transcribe']>[0],model:string,key:string,transport:typeof fetch):Promise<TranscriptSegmentDTO[]>{
  if(!key)throw new ProviderFailure('transcription','not_configured')
  const diarize=/diarize/.test(model),form=new FormData()
  form.set('model',model);form.set('file',new Blob([new Uint8Array(input.audio)],{type:'audio/wav'}),'capture.wav');form.set('response_format',diarize?'diarized_json':model==='whisper-1'?'verbose_json':'json')
  if(diarize)form.set('chunking_strategy','auto')
  const response=await transport('https://api.openai.com/v1/audio/transcriptions',{method:'POST',headers:{Authorization:'Bearer '+key},body:form,signal:AbortSignal.timeout(120_000),redirect:'error'})
  if(!response.ok)throw new ProviderFailure('transcription')
  const raw:unknown=await response.json();if(!object(raw)||typeof raw.text!=='string'||raw.text.length>1_000_000)throw new ProviderFailure('transcription','invalid_response')
  if(!raw.text.trim())return[]
  const segments=Array.isArray(raw.segments)?raw.segments:[{text:raw.text,start:0,end:input.durationMs/1000}]
  if(segments.length>100000)throw new ProviderFailure('transcription','invalid_response')
  const speakers=new Map<string,number>()
  return segments.map((segment):TranscriptSegmentDTO=>{
    if(!object(segment)||typeof segment.text!=='string'||typeof segment.start!=='number'||typeof segment.end!=='number'||!Number.isFinite(segment.start)||!Number.isFinite(segment.end)||segment.start<0||segment.end<segment.start||segment.end*1000>input.durationMs+5000)throw new ProviderFailure('transcription','invalid_response')
    const speaker=typeof segment.speaker==='string'?segment.speaker:null
    if(speaker&&!speakers.has(speaker))speakers.set(speaker,speakers.size+1)
    const number=speaker?speakers.get(speaker):undefined
    return{id:crypto.randomUUID(),channel:input.channel,speakerId:`${input.channel}:${input.chunkId}:${number===undefined?'unknown':`speaker_${number}`}`,speakerLabel:number===undefined?'Unknown speaker':`Speaker ${number}`,text:segment.text.trim(),isFinal:true,startMs:input.startMs+Math.round(segment.start*1000),endMs:input.startMs+Math.round(segment.end*1000)}
  }).filter(segment=>segment.text)
}
