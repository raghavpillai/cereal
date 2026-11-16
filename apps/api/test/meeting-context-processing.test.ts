import {afterEach,expect,test} from "bun:test"
import {Effect} from "effect"
import {randomBytes} from "node:crypto"
import type {PrincipalData} from "@meeting-notes/contracts/api"
import {makeMemoryRepository} from "../src/repository"
import {makeMemoryCalendarStore} from "../src/calendar-store"
import {makeMemoryGoogleStore} from "../src/google-store"
import {makeGoogleService,normalizeGoogleEvent} from "../src/google"
import {makeAdministrationService} from "../src/administration"
import {makeMemoryNoteParticipantsService} from "../src/note-participants"
import {secretBoxFromKey} from "../src/admin-secrets"
import {makeApiHandler} from "../src/api"
import {makeProcessingService} from "../src/processing"
import {makeProviderServices,ProviderFailure,serializeProviderContext,type ProviderNote,type ProviderServices} from "../src/providers"
const cleanups:Array<()=>Promise<void>>=[];afterEach(async()=>{for(const close of cleanups.splice(0))await close()})
const alice:PrincipalData={id:crypto.randomUUID(),name:"Alice",email:"alice@example.invalid",role:"user",source:"session",expiresAt:null,createdAt:"2026-01-01T00:00:00Z"},bob={...alice,id:crypto.randomUUID(),name:"Bob"}
const segment={id:crypto.randomUUID(),channel:"mic" as const,speakerId:"speaker-0",speakerLabel:null,text:"We discussed the launch plan.",isFinal:true,startMs:0,endMs:1000}
async function fixture(){
 const notes=makeMemoryRepository(),calendars=makeMemoryCalendarStore(notes),store=makeMemoryGoogleStore(notes,calendars),box=secretBoxFromKey(randomBytes(32)),google=makeGoogleService(store,box,{baseURL:"http://localhost:3000"}),calls:Array<{kind:string;note:ProviderNote}>=[]
 const providers:ProviderServices={generate:note=>Effect.sync(()=>{calls.push({kind:"generate",note:structuredClone(note)});return "Generated summary"}),chat:(note)=>Effect.sync(()=>{calls.push({kind:"chat",note:structuredClone(note)});return "Answer"}),async *chatStream(note){calls.push({kind:"stream",note:structuredClone(note)});yield "Answer"}}
 const processing=makeProcessingService(notes,{providers}),admin=makeAdministrationService(calendars,box,{clock:()=>new Date("2026-11-12T12:00:00Z"),fetchFeed:async()=>"BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:context-test\r\nDTSTART:20261113T120000Z\r\nDTEND:20261113T130000Z\r\nSUMMARY:Explicit attached meeting\r\nURL:https://meet.google.com/room-secret\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:unrelated-event\r\nDTSTART:20261114T120000Z\r\nDTEND:20261114T130000Z\r\nSUMMARY:UNRELATED CALENDAR SECRET\r\nEND:VEVENT\r\nEND:VCALENDAR"})
 const api=makeApiHandler(notes,"unused-owner-bootstrap-token-at-least-32-characters",{processing,administration:admin,google,noteParticipants:makeMemoryNoteParticipantsService(notes,google),principalResolver:async h=>h.get("authorization")==="Bearer alice"?alice:h.get("authorization")==="Bearer bob"?bob:null});cleanups.push(api.dispose)
 const request=(path:string,method="GET",body?:unknown,user="alice")=>api.handler(new Request("http://localhost"+path,{method,headers:{Authorization:"Bearer "+user,...(body===undefined?{}:{"Content-Type":"application/json"})},...(body===undefined?{}:{body:JSON.stringify(body)})}))
 const source=await Effect.runPromise(admin.createCalendar({name:"Public source",url:"https://calendar.example.com/public.ics"}));const events=await Effect.runPromise(admin.listEvents({},alice.id)),event=events.find(e=>e.title==="Explicit attached meeting")!
 return{notes,calendars,store,google,admin,calls,request,event,source,processing}
}
test("generation, note chat, streamed and workspace chat receive only scoped explicitly attached names, and clear stops metadata",async()=>{
 const f=await fixture(),created=await(await f.request("/api/v1/notes","POST",{title:"New note",transcriptSegments:[segment]})).json(),id=created.id
 const attached=await f.request("/api/v1/notes/"+id+"/context","PUT",{eventId:f.event.id});expect(attached.status).toBe(200);expect((await Effect.runPromise(f.notes.forUser(alice.id).getNote(id)))!.title).toBe("Explicit attached meeting")
 await f.request("/api/v1/notes/"+id+"/context/participants","POST",{id:crypto.randomUUID(),displayName:"Renée van der Meer",givenName:"Renée",familyName:"van der Meer",email:"renee@example.invalid"})
 await f.request("/api/v1/notes/"+id+"/context/participants","POST",{id:crypto.randomUUID(),displayName:"Single Display Name"})
 const foreign=await(await f.request("/api/v1/notes","POST",{title:"Bob PRIVATE NOTE"},"bob")).json();await f.request("/api/v1/notes/"+foreign.id+"/context","PUT",{eventId:f.event.id},"bob");await f.request("/api/v1/notes/"+foreign.id+"/context/participants","POST",{id:crypto.randomUUID(),displayName:"BOB PRIVATE ATTENDEE"},"bob")
 expect((await f.request("/api/v1/notes/"+id+"/generate","POST",{expectedTranscriptRevision:created.transcriptRevision})).status).toBe(200)
 expect((await f.request("/api/v1/notes/"+id+"/chat","POST",{message:"Who is invited?",history:[]})).status).toBe(200)
 await(await f.request("/api/v1/notes/"+id+"/chat/stream","POST",{message:"Names?",history:[]})).text();await(await f.request("/api/v1/chat/stream","POST",{message:"Who is invited?",history:[]})).text()
 expect(f.calls.map(c=>c.kind)).toEqual(["generate","chat","stream","stream"])
 for(const call of f.calls){const serialized=serializeProviderContext(call.note);expect(serialized).toContain("Renée");expect(serialized).not.toContain("BOB PRIVATE");expect(serialized).not.toContain("UNRELATED CALENDAR SECRET");expect(serialized).not.toContain("room-secret")}
 expect(f.calls[0]!.note.meetingContext!.participants[1]).toMatchObject({givenName:null,familyName:null,email:null})
 expect((await f.request("/api/v1/notes/"+id+"/context","PUT",{eventId:null})).status).toBe(200);await f.request("/api/v1/notes/"+id+"/chat","POST",{message:"Who?",history:[]});expect(f.calls.at(-1)!.note.meetingContext).toBeUndefined();expect(serializeProviderContext(f.calls.at(-1)!.note)).not.toContain("renee@example.invalid")
 expect((await f.request("/api/v1/notes/"+id+"/chat","POST",{message:"Private?",history:[]},"bob")).status).toBe(404)
})
test("manual reassociation updates canonical ICS binding, preserves user title and manual names only for the same event, and conflicts never steal notes",async()=>{
 const f=await fixture(),a=await Effect.runPromise(f.notes.forUser(alice.id).createNote({title:"My custom title"})),b=await Effect.runPromise(f.notes.forUser(alice.id).createNote({title:"New note"}));
 expect((await f.request("/api/v1/notes/"+a.id+"/context","PUT",{eventId:f.event.id})).status).toBe(200)
 expect((await Effect.runPromise(f.admin.listEvents({},alice.id))).find(e=>e.id===f.event.id)!.noteId).toBe(a.id)
 const opened=await(await f.request("/api/v1/calendar/events/"+f.event.id+"/note","POST")).json();expect(opened.id).toBe(a.id);expect(opened.title).toBe("My custom title")
 await f.request("/api/v1/notes/"+a.id+"/context/participants","POST",{id:crypto.randomUUID(),displayName:"Manual for first meeting"});await f.request("/api/v1/notes/"+a.id+"/context","PUT",{eventId:f.event.id});expect((await Effect.runPromise(f.google.getNoteContext(alice.id,a.id))).meeting!.participants).toHaveLength(1)
 const conflict=await f.request("/api/v1/notes/"+b.id+"/context","PUT",{eventId:f.event.id});expect(conflict.status).toBe(409);expect((await Effect.runPromise(f.google.getNoteContext(alice.id,b.id))).meeting).toBeNull();expect((await Effect.runPromise(f.notes.forUser(alice.id).getNote(b.id)))!.title).toBe("New note")
 const other=(await Effect.runPromise(f.admin.listEvents({},alice.id))).find(e=>e.id!==f.event.id)!;expect((await f.request("/api/v1/notes/"+a.id+"/context","PUT",{eventId:other.id})).status).toBe(200);expect((await Effect.runPromise(f.google.getNoteContext(alice.id,a.id))).meeting!.participants).toHaveLength(0);const list=await Effect.runPromise(f.admin.listEvents({},alice.id));expect(list.find(e=>e.id===f.event.id)!.noteId).toBeNull();expect(list.find(e=>e.id===other.id)!.noteId).toBe(a.id)
 await f.request("/api/v1/notes/"+a.id+"/context","PUT",{eventId:null});expect((await Effect.runPromise(f.admin.listEvents({},alice.id))).every(e=>e.noteId===null)).toBe(true)
})
test("provider attachment is bounded and an edited snapshot cannot finish an obsolete generation",async()=>{
 const notes=makeMemoryRepository(),scoped=notes.forUser(alice.id),note=await Effect.runPromise(scoped.createNote({title:"Meeting",transcriptSegments:[segment]})),f=await fixture(),base=(await Effect.runPromise(f.admin.eventContext(f.event.id)))!
 let context=base,called=0,release:()=>void=()=>{},started:()=>void=()=>{};const ready=new Promise<void>(r=>{started=r})
 const providers:ProviderServices={chat:()=>Effect.succeed("ok"),generate:()=>Effect.tryPromise({try:async()=>{called++;started();await new Promise<void>(r=>{release=r});return "Obsolete attendee summary"},catch:()=>new ProviderFailure("language")})}
 const processing=makeProcessingService(scoped,{providers,meetingContext:()=>Effect.succeed(context)});cleanups.push(processing.dispose)
 const generating=Effect.runPromise(processing.generate(note.id,note.transcriptRevision));await ready;context={...base,title:"Edited meeting"};release();await expect(generating).rejects.toThrow();expect((await Effect.runPromise(scoped.getNote(note.id)))!.enhanced).toBe("")
 context={...base,title:"x".repeat(100_001)};await expect(Effect.runPromise(processing.chat(note.id,{message:"Who?",history:[]}))).rejects.toThrow();expect(called).toBe(1)
})

test("the actual language adapter includes explicit structured attendee provenance without URLs or inferred names",async()=>{
 const f=await fixture(),context=(await Effect.runPromise(f.admin.eventContext(f.event.id)))!,note=await Effect.runPromise(f.notes.forUser(alice.id).createNote({title:"Note"}));let sent:any
 const participant={id:crypto.randomUUID(),displayName:"One Display Name",givenName:null,familyName:null,email:null,nameSource:"calendar" as const,isSelf:false,isOrganizer:false,isResource:false,responseStatus:"unknown" as const}
 const providers=makeProviderServices({openRouterKey:"fixture-private-key",fetch:(async(_url:unknown,init:any)=>{sent=JSON.parse(init.body);return Response.json({choices:[{message:{content:"Generated response"}}]})})as typeof fetch})
 await Effect.runPromise(providers.chat({...note,meetingContext:{...context,participants:[participant]}},{message:"Who was invited?",history:[]}))
 const payload=sent.messages.find((m:any)=>m.role==="user").content;expect(payload).toContain('"givenName":null');expect(payload).toContain('"familyName":null');expect(payload).toContain('"nameSource":"calendar"');expect(payload).not.toContain("room-secret");expect(payload).not.toContain(context.eventId);expect(sent.messages[0].content).toContain("not proof of attendance or transcript speaker identity")
})

test("a delayed clear response cannot unlink a newer association after its atomic transaction",async()=>{
 const f=await fixture(),note=await Effect.runPromise(f.notes.forUser(alice.id).createNote({title:"Concurrent selection"})),other=(await Effect.runPromise(f.admin.listEvents({},alice.id))).find(e=>e.id!==f.event.id)!
 await f.request("/api/v1/notes/"+note.id+"/context","PUT",{eventId:f.event.id})
 let entered:()=>void=()=>{},release:()=>void=()=>{};const ready=new Promise<void>(r=>{entered=r}),held=new Promise<void>(r=>{release=r}),original=f.google.setNoteContext
 f.google.setNoteContext=(userId,noteId,eventId)=>Effect.flatMap(original(userId,noteId,eventId),context=>eventId===null?Effect.promise(async()=>{entered();await held;return context}):Effect.succeed(context))
 const clearing=f.request("/api/v1/notes/"+note.id+"/context","PUT",{eventId:null});await ready
 expect((await f.request("/api/v1/notes/"+note.id+"/context","PUT",{eventId:other.id})).status).toBe(200);release();expect((await clearing).status).toBe(200)
 expect((await Effect.runPromise(f.admin.listEvents({},alice.id))).find(e=>e.id===other.id)!.noteId).toBe(note.id)
 expect((await Effect.runPromise(f.google.getNoteContext(alice.id,note.id))).meeting!.eventId).toBe(other.id)
})
