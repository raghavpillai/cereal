import {afterEach,expect,test} from "bun:test"
import {Effect} from "effect"
import type {CalendarEventDTO} from "@meeting-notes/contracts"
import type {PrincipalData} from "@meeting-notes/contracts/api"
import type {AuthenticationService} from "../src/auth"
import type {GoogleIntegration} from "../src/google-integration"
import {makeApiHandler} from "../src/api"
import {makeMemoryRepository} from "../src/repository"
import {makeAdministrationService} from "../src/administration"
import {makeMemoryCalendarStore} from "../src/calendar-store"
const disposals:Array<()=>Promise<void>>=[];afterEach(async()=>{await Promise.all(disposals.splice(0).map(d=>d()))})
const alice:PrincipalData={id:"11111111-1111-4111-8111-111111111111",name:"Alice",email:"alice@example.invalid",role:"user",source:"session",expiresAt:"2026-12-01T00:00:00Z",createdAt:"2026-01-01T00:00:00Z",sessionId:"aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"}
const bob:PrincipalData={...alice,id:"22222222-2222-4222-8222-222222222222",name:"Bob",sessionId:"bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb"}
function fixture(){
 const repository=makeMemoryRepository(),calls:Array<{method:string;id:string;value?:unknown}>=[]
 const resolver=async(headers:Headers)=>headers.get("authorization")==="Bearer bob"?bob:headers.get("authorization")==="Bearer alice"||headers.get("cookie")==="session=alice"?alice:null
 const authentication={resolve:resolver,isTrustedOrigin:(origin:string)=>origin==="http://localhost"} as unknown as AuthenticationService
 const administration=makeAdministrationService(makeMemoryCalendarStore(repository),undefined,{clock:()=>new Date("2026-11-12T12:00:00Z"),fetchFeed:async()=>"BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:shared-ics\r\nDTSTART:20261113T160000Z\r\nDTEND:20261113T170000Z\r\nSUMMARY:Shared ICS\r\nEND:VEVENT\r\nEND:VCALENDAR"})
 const event=(id:string):CalendarEventDTO=>({id:id==alice.id?"33333333-3333-4333-8333-333333333333":"44444444-4444-4444-8444-444444444444",calendarId:"55555555-5555-4555-8555-555555555555",calendarName:"Personal Google",color:"#87e5e8",title:id===alice.id?"Alice private calendar":"Bob private calendar",startAt:"2026-11-13T12:00:00Z",endAt:"2026-11-13T13:00:00Z",allDay:false,startDate:"2026-11-13",endDate:"2026-11-13",timeZone:"UTC",location:null,meetingUrl:null,noteId:null})
 const google:GoogleIntegration={
  getStatus:id=>{calls.push({method:"status",id});return Effect.succeed({configured:true,redirectUri:"http://localhost/api/integrations/google/callback",connections:[]})},
  beginConnect:(id,key,contacts)=>{calls.push({method:"connect",id,value:{key,contacts}});return Effect.succeed({authorizationUrl:"https://accounts.google.com/o/oauth2/v2/auth?state=controlled-fixture"})},
  async callback(_request,p){calls.push({method:"callback",id:p.id});throw new Error("Private OAuth response must never be reflected")},
  listCalendars:id=>{calls.push({method:"calendars",id});return Effect.succeed([])},
  selectCalendar:()=>Effect.fail({error:"not_found",message:"Calendar not found"}),
  syncConnection:()=>Effect.succeed({configured:true,redirectUri:null,connections:[]}),disconnect:()=>Effect.succeed({deleted:true}),
  listEvents:(id,query)=>{calls.push({method:"events",id,value:query});return Effect.succeed([event(id)])},
  createEventNote:(id,eventId)=>{calls.push({method:"note",id,value:eventId});return eventId===event(id).id?Effect.mapError(repository.forUser(id).createNote({title:event(id).title}),()=>({error:"unavailable",message:"Fixture unavailable"} as const)):Effect.succeed(null)},
  initializeExternalContext:(_id,noteId,meeting)=>Effect.succeed({noteId,meeting}),
  putExternalContext:(_id,noteId,meeting)=>Effect.succeed({noteId,meeting}),
  getNoteContext:(id,noteId)=>{calls.push({method:"context",id,value:noteId});return Effect.succeed({noteId,meeting:null})},
  setNoteContext:(id,noteId,eventId)=>{calls.push({method:"setContext",id,value:eventId});return Effect.succeed({noteId,meeting:null})},dispose:async()=>{},
 }
 const api=makeApiHandler(repository,"unused-bootstrap-token-with-at-least-32-characters",{authentication,administration,google});disposals.push(api.dispose)
 const request=(path:string,method="GET",body?:unknown,headers:Record<string,string>={Authorization:"Bearer alice"})=>api.handler(new Request("http://localhost"+path,{method,headers:{...headers,...(body===undefined?{}:{"Content-Type":"application/json"})},...(body===undefined?{}:{body:JSON.stringify(body)})}))
 return{calls,repository,administration,request,event}
}
test("personal Google routes pass only the authenticated principal and connect requires a browser session",async()=>{
 const f=fixture();expect((await f.request("/api/v1/integrations/google","GET",undefined,{})).status).toBe(401);expect((await f.request("/api/v1/integrations/google")).status).toBe(200);expect(f.calls[0]).toEqual({method:"status",id:alice.id})
 expect((await f.request("/api/v1/integrations/google/connect","POST",{contacts:true})).status).toBe(403)
 expect((await f.request("/api/v1/integrations/google/connect","POST",{contacts:true},{Cookie:"session=alice"})).status).toBe(403)
 const connected=await f.request("/api/v1/integrations/google/connect","POST",{contacts:true},{Cookie:"session=alice",Origin:"http://localhost"});expect(connected.status).toBe(200);const call=f.calls.find(c=>c.method==="connect")!;expect(call.id).toBe(alice.id);expect((call.value as any).key).toMatch(/^[a-f0-9]{64}$/);expect((call.value as any).key).not.toContain(alice.sessionId!);expect((call.value as any).contacts).toBe(true)
 const noteId=crypto.randomUUID();expect((await f.request("/api/v1/notes/"+noteId+"/context","PUT",{eventId:f.event(bob.id).id},{Authorization:"Bearer bob"})).status).toBe(200);expect(f.calls.at(-1)!.id).toBe(bob.id)
})
test("aggregate upcoming joins shared ICS and only the current user's Google events with normalized bounds",async()=>{
 const f=fixture();await Effect.runPromise(f.administration.createCalendar({name:"Shared",url:"https://calendar.example.com/public.ics"}))
 const events=await(await f.request("/api/v1/calendar/events?from=2026-11-12T00%3A00%3A00Z&to=2026-11-15T00%3A00%3A00Z")).json();expect(events.map((e:any)=>e.title)).toEqual(["Alice private calendar","Shared ICS"]);expect(JSON.stringify(events)).not.toContain("Bob private")
 const eventCall=f.calls.find(c=>c.method==="events")!;expect(eventCall).toMatchObject({id:alice.id,value:{from:"2026-11-12T00:00:00.000Z",to:"2026-11-15T00:00:00.000Z"}})
 const created=await(await f.request("/api/v1/calendar/events/"+f.event(alice.id).id+"/note","POST")).json();expect(created.title).toBe("Alice private calendar");expect(await Effect.runPromise(f.repository.forUser(bob.id).getNote(created.id))).toBeNull()
 const ics=events.find((e:any)=>e.title==="Shared ICS");expect((await f.request("/api/v1/calendar/events/"+ics.id+"/note","POST")).status).toBe(200)
})
test("OAuth callback rejects bearer and missing browser sessions and never reflects private error data",async()=>{
 const f=fixture(),path="/api/integrations/google/callback?code=private-code&state=private-state"
 for(const headers of[{}, {Authorization:"Bearer alice"},{Cookie:"session=alice"}] as Array<Record<string,string>>){const response=await f.request(path,"GET",undefined,headers);expect(response.status).toBe(303);expect(response.headers.get("Location")).toBe("/account?google=failed");expect(response.headers.get("Cache-Control")).toBe("no-store");expect(await response.text()).not.toContain("private")}
 expect(f.calls.filter(c=>c.method==="callback")).toEqual([{method:"callback",id:alice.id}])
})
