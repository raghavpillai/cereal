// Explicit, synthetic-audio-only integration check through the deployed API.
import {mkdir,writeFile} from "node:fs/promises"
import {resolve} from "node:path"
import {Schema} from "effect"
import {NoteDTO,AudioChunkDTO} from "@meeting-notes/contracts"

if(process.env.RUN_LIVE_DIARIZATION_TESTS!=="1")throw new Error("Opt in with RUN_LIVE_DIARIZATION_TESTS=1; synthetic fixtures only")
const token=process.env.ADMIN_API_TOKEN, multi=process.env.DIARIZATION_AUDIO, single=process.env.DIARIZATION_SINGLE_AUDIO
if(!token||!multi||!single)throw new Error("Provide API credentials and explicit synthetic three-voice/single-voice WAV paths")
const base=new URL(process.env.DEPLOYMENT_URL??"http://localhost:3000")
if(base.protocol!=="https:"&&!(base.protocol==="http:"&&["localhost","127.0.0.1","[::1]"].includes(base.hostname)))throw new Error("Remote tests require HTTPS")
const note=Schema.decodeUnknownSync(NoteDTO),chunk=Schema.decodeUnknownSync(AudioChunkDTO)
const created:string[]=[],evidence:Record<string,unknown>={syntheticAudioOnly:true,throughDeployedIngestion:true,cases:[]}
const output=resolve(process.env.DIARIZATION_EVIDENCE_DIR??".local/diarization-qa")
async function call(path:string,method="GET",body?:unknown){
 const r=await fetch(new URL(path,base),{method,headers:{Authorization:`Bearer ${token}`,...(body===undefined?{}:{"Content-Type":"application/json"})},...(body===undefined?{}:{body:JSON.stringify(body)}),signal:AbortSignal.timeout(180000),redirect:"error"})
 if(!r.ok)throw new Error(`Diarization test ${method} endpoint returned ${r.status}`)
 return r
}
async function capture(title:string,uploads:ReadonlyArray<{channel:"mic"|"system";file:string}>){
 const n=note(await(await call("/api/v1/notes","POST",{title})).json());created.push(n.id)
 const path=`/api/v1/notes/${n.id}`
 await call(path+"/recording","POST",{command:"start"});await call(path+"/recording","POST",{command:"stop"})
 const ids:string[]=[]
 for(const upload of uploads){
  const id=crypto.randomUUID();ids.push(id)
  const data=Buffer.from(await Bun.file(upload.file).arrayBuffer()).toString("base64")
  chunk(await(await call(path+"/audio/chunks/"+id,"PUT",{channel:upload.channel,sequence:0,startMs:0,audioBase64:data})).json())
 }
 const until=Date.now()+240000
 while(true){
  const rows=(await(await call(path+"/audio/chunks")).json() as unknown[]).map(chunk).filter(c=>ids.includes(c.id))
  if(rows.some(r=>r.state==="failed"))throw new Error("Real transcription failed; provider response withheld")
  if(rows.length===ids.length&&rows.every(r=>r.state==="ready"))break
  if(Date.now()>until)throw new Error("Real transcription timed out")
  await Bun.sleep(750)
 }
 const result=note(await(await call(path)).json())
 await mkdir(output,{recursive:true,mode:0o700})
 await writeFile(resolve(output,n.id+".synthetic-transcript.json"),JSON.stringify(result,null,2),{mode:0o600})
 return result
}
function assert(value:unknown,message:string):asserts value{if(!value)throw new Error(message)}
function verify(n:NoteDTO,channel:"mic"|"system",expected:number,returning:boolean){
 const turns=n.transcriptSegments.filter(s=>s.channel===channel)
 const voices=new Set(turns.map(s=>s.speakerId))
 assert(voices.size===expected,`${channel}: expected ${expected} synthetic voices, received ${voices.size}`)
 assert(turns.every(s=>s.isFinal&&s.speakerId.startsWith(channel+":")&&s.speakerLabel!=="You"&&s.speakerLabel!=="Me"&&s.endMs!==null&&s.endMs>=s.startMs),"Speaker source, labels or timestamps were lost")
 if(returning){
  const groups=new Map<string,string>();for(const t of turns)groups.set(t.speakerId,(groups.get(t.speakerId)??"")+" "+t.text.toLowerCase())
  const matching=(a:RegExp,b:RegExp)=>[...groups].find(([,text])=>a.test(text)&&b.test(text))?.[0]
  const ids=[matching(/review starts/,/budget remains/),matching(/finish the prototype/,/before lunch/),matching(/test the dashboard/,/collect the test results/)]
  assert(ids.every(Boolean)&&new Set(ids).size===3,`${channel}: returning voices did not retain the expected distinct attribution`)
 }
 return {channel,speakerCount:voices.size,turnCount:turns.length,returningVoicesChecked:returning}
}
try{
 const mic=await capture("Synthetic three-speaker microphone verification",[{channel:"mic",file:multi}])
 const first=verify(mic,"mic",3,true);console.log("Single microphone: three voices and returning turns verified")
 const desktop=await capture("Synthetic desktop microphone and remote speakers",[{channel:"mic",file:single},{channel:"system",file:multi}])
 const local=verify(desktop,"mic",1,false),remote=verify(desktop,"system",3,true)
 assert(new Set(desktop.transcriptSegments.map(s=>s.speakerId)).size===4,"Desktop source identities collided")
 evidence.cases=[{noteId:mic.id,checks:[first]},{noteId:desktop.id,checks:[local,remote]}]
 evidence.passed=true
 console.log("Desktop: one microphone voice plus three remote voices verified independently")
}finally{
 await mkdir(output,{recursive:true,mode:0o700})
 evidence.noteIds=created
 await writeFile(resolve(output,"result.json"),JSON.stringify(evidence,null,2),{mode:0o600})
 if(process.env.KEEP_DIARIZATION_FIXTURES!=="1")for(const id of created)await call(`/api/v1/notes/${id}`,"DELETE")
}
