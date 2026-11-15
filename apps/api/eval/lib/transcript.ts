/** Turn a fixture script into transcript segments shaped like the ones the
 * server stores after transcription (see `transcribe` in src/providers.ts). */
import {createHash} from "node:crypto"
import type {TranscriptSegmentDTO} from "@meeting-notes/contracts"
import type {Capture,CompiledFixture,Fixture,Turn} from "./types"

/** Stable UUID so fixture IDs, citations and golden outputs are reproducible. */
export function stableUuid(...parts:ReadonlyArray<string|number>):string {
  const h=createHash("sha256").update(parts.join("\u0000")).digest("hex")
  return `${h.slice(0,8)}-${h.slice(8,12)}-4${h.slice(13,16)}-8${h.slice(17,20)}-${h.slice(20,32)}`
}
const PART_MS:Record<Capture,number>={desktop:60_000,ios:120_000,stream:Number.MAX_SAFE_INTEGER}
export const partMs=(capture:Capture)=>PART_MS[capture]
export const clock=(ms:number)=>`${Math.floor(ms/60000)}:${String(Math.floor(ms/1000)%60).padStart(2,"0")}`

export function parseTurns(fixture:Fixture):Turn[] {
  const turns:Turn[]=[]
  let cursor=0
  for(const raw of fixture.script.split("\n")){
    const line=raw.trim()
    if(!line)continue
    const pause=/^\[\+(\d+)\]$/.exec(line)
    if(pause){cursor+=Number(pause[1])*1000;continue}
    const match=/^(~?)([A-Z][A-Za-z]+):\s+(.+)$/.exec(line)
    if(!match)throw new Error(`${fixture.id}: cannot parse script line: ${line.slice(0,60)}`)
    const [,overlap,speaker,text]=match as unknown as [string,string,string,string]
    const channel=fixture.people[speaker]
    if(!channel)throw new Error(`${fixture.id}: unknown speaker ${speaker}`)
    const words=text.split(/\s+/).length,durationMs=Math.max(600,Math.round(words/fixture.wpm*60_000))
    const previous=turns.at(-1)
    const startMs=overlap&&previous?Math.round(previous.startMs+(previous.endMs-previous.startMs)*0.6):cursor
    const endMs=startMs+durationMs
    turns.push({index:turns.length,speaker,channel,text,startMs,endMs})
    cursor=Math.max(cursor,endMs+350)
  }
  return turns
}

/** Group turns the way batch diarization of each uploaded part does: within one
 * channel and part, consecutive words from the same voice are one segment, and
 * speaker numbers are assigned in first-spoken order per part. */
export function compileFixture(fixture:Fixture,capture:Capture=fixture.capture):CompiledFixture {
  const turns=parseTurns(fixture),size=PART_MS[capture]
  const segments:TranscriptSegmentDTO[]=[],segmentTurns=new Map<string,number[]>()
  const open=new Map<string,{segment:TranscriptSegmentDTO;speaker:string}>(),numbers=new Map<string,Map<string,number>>()
  for(const turn of turns){
    const channel=capture==="desktop"?turn.channel:"mic",part=Math.floor(turn.startMs/size),key=`${channel}:${part}`
    const partId=stableUuid("part",fixture.id,capture,channel,part)
    const speakers=numbers.get(key)??new Map<string,number>()
    numbers.set(key,speakers)
    if(!speakers.has(turn.speaker))speakers.set(turn.speaker,speakers.size+1)
    const current=open.get(key)
    if(capture!=="stream"&&current&&current.speaker===turn.speaker&&current.segment.text.length+turn.text.length<18_000){
      const merged={...current.segment,text:`${current.segment.text} ${turn.text}`,endMs:Math.max(current.segment.endMs??0,turn.endMs)}
      segments[segments.indexOf(current.segment)]=merged
      open.set(key,{segment:merged,speaker:turn.speaker})
      segmentTurns.get(merged.id)!.push(turn.index)
      continue
    }
    const number=speakers.get(turn.speaker)!
    const segment:TranscriptSegmentDTO={id:stableUuid("segment",fixture.id,capture,turn.index),channel,speakerId:`${channel}:${partId}:speaker_${number}`,speakerLabel:`Speaker ${number}`,text:turn.text,isFinal:true,startMs:turn.startMs,endMs:turn.endMs}
    segments.push(segment)
    segmentTurns.set(segment.id,[turn.index])
    // Another voice on the same channel ends the previous speaker's segment.
    open.set(key,{segment,speaker:turn.speaker})
  }
  segments.sort((a,b)=>a.startMs-b.startMs)
  const durationMs=Math.max(0,...turns.map(turn=>turn.endMs))
  return {fixture,capture,turns,segments,segmentTurns,durationMs,duration:clock(durationMs)}
}

/** The part of a recording the server would hold at `atMs` while the meeting is
 * still running, under a client delivery model. */
export interface Delivery {id:string;description:string;capture:Capture;
  /** Seconds between a part closing (or, for `stream`, a turn ending) and its
   * transcript being committed. `null` means nothing arrives before Stop. */
  lagSeconds:number|null}
export const DELIVERIES:Record<string,Delivery>={
  stream:{id:"stream",capture:"stream",lagSeconds:2,description:"Hypothetical live transport: each turn is committed 2 s after it ends. No client does this today."},
  ios:{id:"ios",capture:"ios",lagSeconds:20,description:"iOS today: the recorder rotates every 120 s and uploads the finished part; batch transcription adds roughly 20 s."},
  desktop:{id:"desktop",capture:"desktop",lagSeconds:null,description:"Desktop today: audio is uploaded only after Stop, so nothing from the current recording is on the server."},
}
export function deliveredAt(compiled:CompiledFixture,delivery:Delivery,atMs:number):{segments:TranscriptSegmentDTO[];deliveredUntilMs:number} {
  if(delivery.lagSeconds===null)return {segments:[],deliveredUntilMs:0}
  const visible=atMs-delivery.lagSeconds*1000
  if(delivery.capture==="stream"){
    const segments=compiled.segments.filter(segment=>(segment.endMs??segment.startMs)<=visible)
    return {segments,deliveredUntilMs:Math.max(0,...segments.map(segment=>segment.endMs??segment.startMs))}
  }
  const size=PART_MS[delivery.capture],closed=Math.max(0,Math.floor(visible/size))*size
  return {segments:compiled.segments.filter(segment=>segment.startMs<closed),deliveredUntilMs:closed}
}
/** When a turn first becomes visible to the server under a delivery model. */
export function visibleAtMs(turn:Turn,delivery:Delivery):number|null {
  if(delivery.lagSeconds===null)return null
  if(delivery.capture==="stream")return turn.endMs+delivery.lagSeconds*1000
  const size=PART_MS[delivery.capture]
  return (Math.floor(turn.startMs/size)+1)*size+delivery.lagSeconds*1000
}
export const findTurn=(compiled:CompiledFixture,quote:string):Turn=>{
  const found=compiled.turns.filter(turn=>turn.text.includes(quote))
  if(found.length!==1)throw new Error(`${compiled.fixture.id}: quote must match exactly one turn (${found.length}): ${quote}`)
  return found[0]!
}
export const supportSegments=(compiled:CompiledFixture,quotes:ReadonlyArray<string>=[]):string[]=>{
  const ids=new Set<string>()
  for(const quote of quotes){
    const turn=findTurn(compiled,quote)
    for(const [id,turns] of compiled.segmentTurns)if(turns.includes(turn.index))ids.add(id)
  }
  return [...ids]
}
/** Ground-truth view for people and the judge: real names and times. The model
 * under test never sees this. */
export const renderTruth=(turns:ReadonlyArray<Turn>)=>turns.map(turn=>`[${clock(turn.startMs)}] ${turn.speaker}: ${turn.text}`).join("\n")
export const pattern=(source:string)=>new RegExp(source,"i")
/** Pattern source that matches when both parts occur within `distance` characters on one line. */
export const near=(a:string,b:string,distance=140)=>`(?:${a})[^\\n]{0,${distance}}(?:${b})|(?:${b})[^\\n]{0,${distance}}(?:${a})`
