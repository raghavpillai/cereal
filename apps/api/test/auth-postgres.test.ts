import {afterAll,describe,expect,test} from "bun:test"
import {Effect} from "effect"
import {randomBytes} from "node:crypto"
import {makeAuthenticationService,BOOTSTRAP_OWNER_ID} from "../src/auth"
import {makePrismaRepository} from "../src/prisma-repository"
import {makePrismaCalendarStore} from "../src/calendar-store"
import {makeAdministrationService} from "../src/administration"
import {secretBoxFromKey} from "../src/admin-secrets"
import {makeGoogleService} from "../src/google"
import {makePrismaGoogleStore} from "../src/google-store"
import {makeNoteParticipantsService} from "../src/note-participants"
import {DEFAULT_ORGANIZATION_ID} from "../src/organizations"
import {makeApiHandler} from "../src/api"
const enabled=process.env.RUN_AUTH_PG_TESTS==="1"&&!!process.env.DATABASE_URL
/** Run only in a dedicated throwaway database: initializes the bootstrap owner. */
describe.skipIf(!enabled)("real Better Auth / PostgreSQL sessions, roles and private notes",()=>{
 const resources=enabled?makePrismaRepository(process.env.DATABASE_URL!):null,db=resources?.database
 const ownerToken="isolated-auth-bootstrap-token-at-least-32-characters",baseURL="http://localhost:3000"
 const authentication=db?makeAuthenticationService(db,ownerToken,{secret:randomBytes(32).toString("base64url"),baseURL}):null
 const box=secretBoxFromKey(randomBytes(32)),store=db?makePrismaCalendarStore(db,"auth-test"):null
 const administration=store?makeAdministrationService(store,box,{clock:()=>new Date("2026-11-12T00:00:00Z"),fetchFeed:async()=>"BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:auth-test-meeting\r\nDTSTART:20261113T120000Z\r\nDTEND:20261113T130000Z\r\nSUMMARY:Shared calendar private notes\r\nEND:VEVENT\r\nEND:VCALENDAR"}):null
 const google=db?makeGoogleService(makePrismaGoogleStore(db),box,{baseURL}):null
 const api=resources?makeApiHandler(resources.repository,ownerToken,{authentication:authentication!,administration:administration!,google:google!,noteParticipants:makeNoteParticipantsService(db!)}):null
 const password="test-only-password-"+randomBytes(12).toString("hex")
 let aliceMeetingNoteId="",bobMeetingNoteId="",aliceId="",aliceToken="",bobId="",bobToken="",ownerSession="",cookie="",calendarId=""
 const request=(path:string,method="GET",body?:unknown,credential=ownerToken,extra:Record<string,string>={})=>api!.handler(new Request(baseURL+path,{method,headers:{...(credential?{Authorization:"Bearer "+credential}:{}),...(body===undefined?{}:{"Content-Type":"application/json"}),...extra},...(body===undefined?{}:{body:JSON.stringify(body)})}))
 async function login(email:string){const response=await request("/api/auth/sign-in/email","POST",{email,password,rememberMe:true},"",{});const payload=await response.json();expect(response.status).toBe(200);expect(payload.user.id).toBeDefined();const signed=response.headers.get("set-auth-token");expect(signed).toBeTruthy();return{response,payload,token:signed!}}
 afterAll(async()=>{await api?.dispose();await resources?.close()})
 test("owner claim is protected and once only; anonymous signup and role injection are closed",async()=>{
  expect((await request("/api/v1/admin/owner/setup","POST",{name:"Owner",email:"owner@example.test",password},"")).status).toBe(401)
  expect((await request("/api/auth/sign-up/email","POST",{name:"Attacker",email:"attacker@example.test",password,role:"owner"},"")).status).toBe(404)
  const response=await request("/api/v1/admin/owner/setup","POST",{name:"Owner",email:"owner@example.test",password});expect(response.status).toBe(200);expect((await response.json()).id).toBe(BOOTSTRAP_OWNER_ID)
  expect((await request("/api/v1/admin/owner/setup","POST",{name:"Again",email:"again@example.test",password})).status).toBe(409)
  const owner=await login("owner@example.test");ownerSession=owner.token;cookie=owner.response.headers.getSetCookie().map(v=>v.split(";")[0]).join("; ");expect(cookie).toContain("session_token=");expect(owner.response.headers.getSetCookie().join(";")).toContain("HttpOnly");expect(owner.response.headers.getSetCookie().join(";")).toContain("SameSite=Lax")
  for(const [email,name] of[["alice@example.test","Alice"],["bob@example.test","Bob"]]){const created=await request("/api/v1/admin/users","POST",{name,email,password,role:"admin"});expect(created.status).toBe(200);const user=await created.json();expect(user.role).toBe("user");if(name==="Alice")aliceId=user.id;else bobId=user.id}
  aliceToken=(await login("alice@example.test")).token;bobToken=(await login("bob@example.test")).token
  const account=await db!.account.findFirstOrThrow({where:{userId:aliceId}});expect(account.password).not.toBe(password);expect(account.password).not.toContain(password)
 })
 test("cookie session and signed native bearer work; unsafe cookies need trusted origin and conflicting bearer wins",async()=>{
  const session=await request("/api/v1/session","GET",undefined,"",{Cookie:cookie});expect(session.status).toBe(200);expect((await session.json()).user.role).toBe("owner")
  expect((await request("/api/v1/admin/settings","PATCH",{instanceName:"CSRF"},"",{Cookie:cookie,Origin:"https://evil.example"})).status).toBe(403)
  expect((await request("/api/v1/admin/settings","PATCH",{instanceName:"Missing origin"},"",{Cookie:cookie})).status).toBe(403)
  expect((await request("/api/v1/admin/settings","PATCH",{instanceName:"Real cookie update"},"",{Cookie:cookie,Origin:baseURL})).status).toBe(200)
  const mixed=await(await request("/api/v1/session","GET",undefined,aliceToken,{Cookie:cookie})).json();expect(mixed.user.id).toBe(aliceId)
  expect((await request("/api/v1/session","GET",undefined,"invalid-signed-token",{Cookie:cookie})).status).toBe(401)
  expect((await request("/api/auth/sign-in/email","POST",{email:"owner@example.test",password},"",{Origin:"https://evil.example"})).status).toBe(403)
 })
 test("only owner can grant/revoke admin; current role takes effect on the same active session",async()=>{
  const injected=await request("/api/auth/update-user","POST",{name:"Alice",role:"owner"},aliceToken);expect([200,400]).toContain(injected.status);expect((await db!.user.findUniqueOrThrow({where:{id:aliceId}})).role).toBe("user")
  for(const path of["/api/v1/admin/settings","/api/v1/admin/users"])expect((await request(path,"GET",undefined,aliceToken)).status).toBe(403)
  expect((await request("/api/v1/admin/users/"+aliceId+"/role","PATCH",{role:"admin"},bobToken)).status).toBe(403)
  expect((await request("/api/v1/admin/users/"+aliceId+"/role","PATCH",{role:"admin"})).status).toBe(200)
  expect((await request("/api/v1/admin/settings","GET",undefined,aliceToken)).status).toBe(200)
  expect((await request("/api/v1/admin/users","GET",undefined,aliceToken)).status).toBe(200)
  expect((await request("/api/v1/admin/users/"+bobId+"/role","PATCH",{role:"admin"},aliceToken)).status).toBe(403)
  expect((await request("/api/v1/admin/users/"+BOOTSTRAP_OWNER_ID+"/role","PATCH",{role:"user"})).status).toBe(403)
  expect((await request("/api/v1/admin/users/"+bobId+"/role","PATCH",{role:"owner"})).status).toBe(400)
  expect((await request("/api/v1/admin/users/"+aliceId+"/role","PATCH",{role:"user"})).status).toBe(200)
  expect((await request("/api/v1/admin/settings","GET",undefined,aliceToken)).status).toBe(403)
 })
 test("owner can mint an atomic administrator account; admins and users cannot mint, delegate, or reassign ownership",async()=>{
  const payload={name:"New administrator",email:"minted-admin@example.test",password,role:"owner"}
  expect((await request("/api/v1/admin/administrators","POST",payload,"")).status).toBe(401)
  expect((await request("/api/v1/admin/administrators","POST",payload,aliceToken)).status).toBe(403)
  expect((await request("/api/v1/admin/administrators","POST",payload,"",{Cookie:cookie,Origin:"https://evil.example"})).status).toBe(403)
  const response=await request("/api/v1/admin/administrators","POST",payload,ownerSession);expect(response.status).toBe(200)
  const minted=await response.json();expect(minted.role).toBe("admin");expect(JSON.stringify(minted)).not.toContain(password)
  const account=await db!.account.findFirstOrThrow({where:{userId:minted.id,providerId:"credential"}});expect(account.password).not.toBe(password)
  expect(await db!.member.findUnique({where:{organizationId_userId:{organizationId:DEFAULT_ORGANIZATION_ID,userId:minted.id}}})).not.toBeNull()
  const signed=(await login(payload.email)).token
  expect((await request("/api/v1/admin/settings","GET",undefined,signed)).status).toBe(200)
  expect((await request("/api/v1/admin/administrators","POST",{...payload,email:"delegated-admin@example.test"},signed)).status).toBe(403)
  expect((await request("/api/v1/admin/users/"+bobId+"/role","PATCH",{role:"admin"},signed)).status).toBe(403)
  expect((await request("/api/v1/admin/users/"+BOOTSTRAP_OWNER_ID+"/role","PATCH",{role:"user"},ownerSession)).status).toBe(403)
  const count=await db!.user.count();expect((await request("/api/v1/admin/administrators","POST",payload,ownerSession)).status).toBe(400);expect(await db!.user.count()).toBe(count)
  expect((await request("/api/v1/admin/users/"+minted.id+"/role","PATCH",{role:"user"},ownerSession)).status).toBe(200)
  expect((await request("/api/v1/admin/settings","GET",undefined,signed)).status).toBe(403)
  await db!.user.delete({where:{id:minted.id}})
 })
 test("actual sessions isolate note/chat/audio/share APIs and calendar-to-note associations per user",async()=>{
  const created=await request("/api/v1/notes","POST",{title:"Alice private meeting",body:"Alice confidential"},aliceToken);expect(created.status).toBe(200);const note=await created.json()
  expect((await db!.note.findUniqueOrThrow({where:{id:note.id}})).ownerId).toBe(aliceId)
  for(const [path,method,body]of[["/api/v1/notes/"+note.id,"GET",undefined],["/api/v1/notes/"+note.id,"PATCH",{title:"Stolen"}],["/api/v1/notes/"+note.id,"DELETE",undefined],["/api/v1/notes/"+note.id+"/audio/chunks","GET",undefined],["/api/v1/notes/"+note.id+"/shares","GET",undefined],["/api/v1/notes/"+note.id+"/chat","POST",{message:"Read it",history:[]}] ]as const)expect((await request(path,method,body,bobToken)).status).toBe(404)
  expect((await(await request("/api/v1/notes","GET",undefined,bobToken)).json()).some((n:any)=>n.id===note.id)).toBe(false)
  // Admin role does not grant access to another user's content.
  expect((await request("/api/v1/notes/"+note.id,"GET",undefined,ownerSession)).status).toBe(404)
  const share=await(await request("/api/v1/notes/"+note.id+"/shares","POST",{includeBody:true,includeEnhanced:false},aliceToken)).json();expect((await request("/api/v1/shared/"+share.token,"GET",undefined,"")).status).toBe(200)
  const c=await(await request("/api/v1/calendars","POST",{name:"Shared test source",url:"https://calendar.example.test/feed?token=private"})).json();calendarId=c.id;expect(c.lastSyncError).toBeNull();expect(c.eventCount).toBe(1)
  const events=await(await request("/api/v1/calendar/events?from=2026-11-12T00%3A00%3A00Z&to=2026-11-20T00%3A00%3A00Z","GET",undefined,aliceToken)).json();expect(events).toHaveLength(1)
  const [a,b]=await Promise.all([aliceToken,bobToken].map(t=>request("/api/v1/calendar/events/"+events[0].id+"/note","POST",undefined,t).then(r=>r.json())));aliceMeetingNoteId=a.id;bobMeetingNoteId=b.id;expect(a.id).not.toBe(b.id)
  const personal=await(await request("/api/v1/calendar/events?from=2026-11-12T00%3A00%3A00Z&to=2026-11-20T00%3A00%3A00Z","GET",undefined,aliceToken)).json();expect(personal[0].noteId).toBe(a.id)
  const repeats=await Promise.all([1,2,3].map(()=>request("/api/v1/calendar/events/"+events[0].id+"/note","POST",undefined,aliceToken).then(r=>r.json())));expect(repeats.every(n=>n.id===a.id)).toBe(true)
  expect((await request("/api/v1/calendars","GET",undefined,aliceToken)).status).toBe(200);expect((await request("/api/v1/calendars/"+calendarId,"DELETE",undefined,aliceToken)).status).toBe(403)
 })
 test("calendar/settings persistence survives service recreation; failures retain cached events and encryption is not plaintext",async()=>{
  const before=await db!.calendarSubscription.findUniqueOrThrow({where:{id:calendarId}});expect(before.encryptedURL).not.toContain("token=private");expect(box.open(before.encryptedURL)).toContain("token=private")
  const second=makeAdministrationService(makePrismaCalendarStore(db!,"auth-test"),box,{fetchFeed:async()=>{throw new Error("private token URL must not leak")}});try{expect((await Effect.runPromise(second.getSettings())).instanceName).toBe("Real cookie update");const c=await Effect.runPromise(second.syncCalendar(calendarId));expect(c.eventCount).toBe(1);expect(c.lastSyncError).not.toContain("private token");expect(await db!.calendarEvent.count({where:{calendarId}})).toBe(1)}finally{await second.dispose()}
 })
 test("ICS meeting contexts persist, manual attendees are atomic/idempotent/private, and explicit clearing stays cleared",async()=>{
  const context=await(await request("/api/v1/notes/"+aliceMeetingNoteId+"/context","GET",undefined,aliceToken)).json();expect(context.meeting.provider).toBe("ics");expect(context.meeting.participants).toEqual([]);expect(context.meeting.eventUrl).toBeNull()
  const participant={id:crypto.randomUUID(),displayName:"Ada Lovelace",givenName:"Ada",familyName:"Lovelace",email:"ada@example.invalid"},second={id:crypto.randomUUID(),displayName:"Grace Hopper"}
  const path="/api/v1/notes/"+aliceMeetingNoteId+"/context/participants"
  const responses=await Promise.all([participant,second,participant].map(p=>request(path,"POST",p,aliceToken)));expect(responses.every(r=>r.status===200)).toBe(true)
  const saved=await(await request("/api/v1/notes/"+aliceMeetingNoteId+"/context","GET",undefined,aliceToken)).json();expect(saved.meeting.participants).toHaveLength(2);expect(saved.meeting.participants.find((p:any)=>p.id===participant.id)).toMatchObject({...participant,nameSource:"manual",isSelf:false,isOrganizer:false,responseStatus:"unknown"})
  const eventId=context.meeting.eventId;expect((await request("/api/v1/calendar/events/"+eventId+"/note","POST",undefined,aliceToken)).status).toBe(200);expect((await(await request("/api/v1/notes/"+aliceMeetingNoteId+"/context","GET",undefined,aliceToken)).json()).meeting.participants).toHaveLength(2)
  expect((await request(path,"POST",participant,bobToken)).status).toBe(404);expect((await request(path,"POST",participant,ownerSession)).status).toBe(404)
  const empty=await(await request("/api/v1/notes","POST",{title:"No associated meeting"},aliceToken)).json();expect((await request("/api/v1/notes/"+empty.id+"/context/participants","POST",participant,aliceToken)).status).toBe(400)
  expect((await request("/api/v1/notes/"+bobMeetingNoteId+"/context","GET",undefined,aliceToken)).status).toBe(404)
  expect((await request("/api/v1/notes/"+aliceMeetingNoteId+"/context","PUT",{eventId:null},aliceToken)).status).toBe(200);expect((await(await request("/api/v1/notes/"+aliceMeetingNoteId+"/context","GET",undefined,aliceToken)).json()).meeting).toBeNull()
 })
 test("manual ICS association atomically renames only defaults, preserves stopped recording and makes upcoming reuse the note",async()=>{
  const event=await db!.calendarEvent.findFirstOrThrow({where:{calendarId}}),created=await(await request("/api/v1/notes","POST",{title:"  NEW NOTE  "},aliceToken)).json()
  await request("/api/v1/notes/"+created.id+"/recording","POST",{command:"start"},aliceToken);await request("/api/v1/notes/"+created.id+"/recording","POST",{command:"stop"},aliceToken)
  expect((await request("/api/v1/notes/"+created.id+"/context","PUT",{eventId:event.id},aliceToken)).status).toBe(200)
  const updated=await(await request("/api/v1/notes/"+created.id,"GET",undefined,aliceToken)).json();expect(updated.title).toBe(event.title);expect(updated.recordingState).toBe("stopped")
  expect((await(await request("/api/v1/calendar/events/"+event.id+"/note","POST",undefined,aliceToken)).json()).id).toBe(created.id)
  const other=await(await request("/api/v1/notes","POST",{title:"My handwritten title",body:"Do not merge my notes"},aliceToken)).json()
  expect((await request("/api/v1/notes/"+other.id+"/context","PUT",{eventId:event.id},aliceToken)).status).toBe(409);expect((await db!.note.findUniqueOrThrow({where:{id:other.id}})).body).toBe("Do not merge my notes")
  await request("/api/v1/notes/"+created.id+"/context","PUT",{eventId:null},aliceToken);expect((await db!.note.findUniqueOrThrow({where:{id:created.id}})).title).toBe(event.title)
  expect((await request("/api/v1/notes/"+other.id+"/context","PUT",{eventId:event.id},aliceToken)).status).toBe(200);expect((await db!.note.findUniqueOrThrow({where:{id:other.id}})).title).toBe("My handwritten title")
  expect((await(await request("/api/v1/calendar/events/"+event.id+"/note","POST",undefined,aliceToken)).json()).id).toBe(other.id)
 })
 test("password changes verify the current password and revoke other real sessions",async()=>{
  const other=(await login("alice@example.test")).token,newPassword="changed-password-"+randomBytes(12).toString("hex")
  expect((await request("/api/auth/change-password","POST",{currentPassword:"incorrect-password",newPassword,revokeOtherSessions:true},aliceToken)).status).toBe(400)
  expect((await request("/api/auth/change-password","POST",{currentPassword:password,newPassword,revokeOtherSessions:true},aliceToken)).status).toBe(200)
  expect((await request("/api/v1/session","GET",undefined,other)).status).toBe(401)
  const signedIn=await request("/api/auth/sign-in/email","POST",{email:"alice@example.test",password:newPassword},"");expect(signedIn.status).toBe(200);expect(signedIn.headers.get("set-auth-token")).toBeTruthy()
 })
 test("native sign-out revokes the actual bearer session immediately",async()=>{
  expect((await request("/api/v1/session","GET",undefined,bobToken)).status).toBe(200);expect((await request("/api/auth/sign-out","POST",{},bobToken)).status).toBe(200);expect((await request("/api/v1/session","GET",undefined,bobToken)).status).toBe(401)
 })
})
