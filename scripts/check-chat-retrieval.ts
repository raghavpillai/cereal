// Opt-in deployed-provider test. Creates isolated users and fictional notes only.
import { mkdir, writeFile } from "node:fs/promises";
import { Schema } from "effect";
import { ChatStreamEventDTO } from "@meeting-notes/contracts";

if(process.env.RUN_LIVE_CHAT_TESTS!=="1")throw new Error("Opt in with RUN_LIVE_CHAT_TESTS=1");
const operator=process.env.ADMIN_API_TOKEN;
if(!operator)throw new Error("Configure ADMIN_API_TOKEN privately");
const base=new URL(process.env.DEPLOYMENT_URL??"http://localhost:3000");
if(base.protocol!=="https:"&&!(base.protocol==="http:"&&["localhost","127.0.0.1","[::1]"].includes(base.hostname)))throw new Error("Remote tests require HTTPS");
const out=".local/chat-qa";await mkdir(out,{recursive:true,mode:0o700});
const decode=Schema.decodeUnknownSync(ChatStreamEventDTO);
function assert(v:unknown,message:string):asserts v{if(!v)throw new Error(message)}
async function api(path:string,token:string,body?:unknown,method=body===undefined?"GET":"POST"){
 const r=await fetch(new URL(path,base),{method,headers:{Authorization:`Bearer ${token}`,...(body===undefined?{}:{"Content-Type":"application/json"})},body:body===undefined?undefined:JSON.stringify(body),redirect:"error",signal:AbortSignal.timeout(150_000)});
 assert(r.ok,`Chat verification endpoint returned ${r.status}`);return r;
}
async function user(label:string){
 const email=`chat-${label}-${crypto.randomUUID()}@example.invalid`,password=crypto.randomUUID()+crypto.randomUUID();
 const u=await(await api("/api/v1/admin/users",operator!,{name:`Chat QA ${label}`,email,password})).json();
 const r=await fetch(new URL("/api/auth/sign-in/email",base),{method:"POST",headers:{"Content-Type":"application/json",Origin:base.origin},body:JSON.stringify({email,password}),redirect:"error",signal:AbortSignal.timeout(30_000)});
 assert(r.ok,"Fictional QA sign-in failed");const token=r.headers.get("set-auth-token");assert(token,"Signed bearer missing");
 return {id:u.id,email,password,token};
}
const a=await user("Cedar"),b=await user("Other");
const fixtureDates=[new Date().toISOString().slice(0,10),new Date(Date.now()-86400000).toISOString().slice(0,10)];
const makeSegment=(text:string,startMs=0,speakerLabel:string|null="Alex",speakerNumber=0)=>({id:crypto.randomUUID(),channel:"mic",speakerId:`mic:fictional-part:speaker_${speakerNumber}`,speakerLabel,text,isFinal:true,startMs,endMs:startMs+5000});
const first=await(await api("/api/v1/notes",a.token,{title:"Cedar release review",date:fixtureDates[0],body:"Alex will send the release checklist by Wednesday. Jamie owns the accessibility review due Thursday.",enhanced:"# Release plan\n\n- Launch Monday after accessibility review.\n- Alex: send checklist Wednesday.\n- Jamie: complete accessibility review Thursday.",transcriptSegments:[makeSegment("Alex: I will send the release checklist by Wednesday."),makeSegment("Jamie: I will finish the accessibility review by Thursday. We agreed to release Monday.",6000,"Jamie",1)]})).json();
const second=await(await api("/api/v1/notes",a.token,{title:"Harbor discovery",date:fixtureDates[1],body:"Morgan will send the security questionnaire tomorrow. Priya will arrange a follow-up next Tuesday.",enhanced:"# Security review\n\n- A $12,000 pilot is proposed, not approved.",transcriptSegments:[makeSegment("Morgan will send the security questionnaire tomorrow. Priya will arrange a follow-up next Tuesday.",0,null)]})).json();
const secret=await(await api("/api/v1/notes",b.token,{title:"PRIVATE CANARY ORCHID 7391",body:"Never expose this other account's fictional private note."})).json();
const session={base:base.origin,owner:a,other:b,noteIds:[first.id,second.id],otherNoteId:secret.id};
await writeFile(`${out}/session.private.json`,JSON.stringify(session),{mode:0o600});
const report:Record<string,unknown>={fictionalOnly:true,noteIds:session.noteIds,checks:[]};
async function stream(path:string,message:string){
 const response=await api(path,a.token,{message,history:[],includeSources:true});const raw=await response.text();
 const events=raw.split(/\r?\n\r?\n/).filter(Boolean).map(frame=>frame.split(/\r?\n/).filter(line=>line.startsWith("data:")).map(line=>line.slice(5).trimStart()).join("\n")).filter(Boolean).map(s=>decode(JSON.parse(s)));
 assert(events.at(-1)?.type==="done","Chat stream failed or lacked terminal done");
 assert(!raw.includes(secret.title)&&!raw.includes(secret.id),"Other account source leaked");
 const citations=events.flatMap(e=>e.citations??[]);assert(citations.length>0,"No grounded citations returned");
 assert(citations.every(c=>session.noteIds.includes(c.noteId)),"Citation escaped fixture account");
 for(const citation of citations){
  const source=await(await api(`/api/v1/notes/${citation.noteId}`,a.token)).json();
  assert(source.transcriptRevision===citation.transcriptRevision,"Citation revision drifted");
  if(citation.source==="transcript"){
   const segment=source.transcriptSegments.find((s:{id:string})=>s.id===citation.segmentId);
   assert(segment&&segment.startMs===citation.startMs&&segment.text.includes(citation.quote),"Citation is not an exact timestamped source substring");
  }else assert((citation.source==="notes"?source.body:source.enhanced).includes(citation.quote),"Citation quote was fabricated");
 }
 const answer=events.filter(e=>e.type==="delta").map(e=>e.text).join("");assert(/\[\[S\d+\]\]/.test(answer),"Issued source markers absent");
 return {events,answer,citations};
}
const global=await stream("/api/v1/chat/stream","List my recent todos, organized by meeting. Search my meetings and read their contents. Include citations and say which meetings you checked.");
assert(global.events.some(e=>e.type==="tool"&&e.tool?.status==="completed"),"Actual completed retrieval progress missing");
assert(/checklist|accessibility/i.test(global.answer)&&/questionnaire|follow.up/i.test(global.answer),"Global retrieval missed either fictional meeting");
const scoped=await stream(`/api/v1/notes/${first.id}/chat/stream`,"What exactly did Alex commit to? Cite the transcript quote and keep it concise.");
assert(scoped.citations.some(c=>c.source==="transcript"&&c.segmentId===first.transcriptSegments[0].id),"Exact cited transcript segment missing");
assert(/Wednesday/i.test(scoped.answer),"Scoped answer missed assignment deadline");
// An old meeting with the answer beyond the former first-2k/6k truncation,
// followed by 21 newer notes, prevents a recent-list shortcut from passing.
const tail="ZEPHYR archive decision: Casey will ship the cobalt adapter on December 7. The confirmed budget is $4,200.";
const filler="Fictional discussion of routine testing without decisions or assignments. ".repeat(210);
const old=await(await api("/api/v1/notes",a.token,{title:"ZEPHYR archival review",date:new Date(Date.now()-30*86400000).toISOString().slice(0,10),body:filler+"\n\n"+tail,transcriptSegments:[makeSegment(filler+tail,540000,"Casey")]})).json();
session.noteIds.push(old.id); const fillerIds:string[]=[];
for(let i=0;i<21;i++){
 const extra=await(await api("/api/v1/notes",a.token,{title:`Unrelated fictional chat fixture ${i+1}`,date:fixtureDates[0]})).json();session.noteIds.push(extra.id);fillerIds.push(extra.id);
}
await writeFile(`${out}/session.private.json`,JSON.stringify(session),{mode:0o600});
const archived=await stream("/api/v1/chat/stream","Search transcripts for ZEPHYR. What did Casey commit to shipping, on what date, and with what budget? Cite the exact transcript passage.");
assert(archived.events.some(e=>e.type==="tool"&&e.tool?.name==="search_transcripts"&&e.tool.status==="completed"),"Dedicated transcript search was not exercised");
assert(/cobalt adapter/i.test(archived.answer)&&/December 7/i.test(archived.answer)&&/4,200|4200/.test(archived.answer),"Old long-tail source facts were omitted");
assert(archived.citations.some(c=>c.noteId===old.id&&c.source==="transcript"&&c.quote.includes("cobalt adapter")),"Long-tail exact transcript citation absent");
for(const id of fillerIds)await api(`/api/v1/notes/${id}`,a.token,undefined,"DELETE");
session.noteIds=session.noteIds.filter(id=>!fillerIds.includes(id));
await writeFile(`${out}/session.private.json`,JSON.stringify(session),{mode:0o600});
report.checks=["signed ordinary-user auth","real meeting search/read tools","actual tool progress metadata","two-meeting grounded todos","owner isolation","issued inline markers","exact scoped transcript citation","exact quote/revision/timestamp verification","dedicated transcript search","older-than-top-20 retrieval","late source-window retrieval"];
report.global={answer:global.answer,events:global.events};report.scoped={answer:scoped.answer,events:scoped.events};report.archived={answer:archived.answer,events:archived.events};report.passed=true;
await writeFile(`${out}/result.json`,JSON.stringify(report,null,2),{mode:0o600});
console.log("Live fictional chat checks passed: retrieval, progress, citations, transcript identity, owner isolation");
console.log("Private QA session retained in ignored .local/chat-qa for native UI verification");
