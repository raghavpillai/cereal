import {afterEach,expect,test} from "bun:test"
import {Effect} from "effect"
import {randomBytes} from "node:crypto"
import {mkdtemp,readFile,stat,rm} from "node:fs/promises"
import {tmpdir} from "node:os"
import {join} from "node:path"
import {makeApiHandler} from "../src/api"
import {makeMemoryRepository} from "../src/repository"
import {makeMemoryCalendarStore} from "../src/calendar-store"
import {loadSecretBox,secretBoxFromKey} from "../src/admin-secrets"
import {makeAdministrationService,settingsAwareProviders} from "../src/administration"
import {parseCalendarFeed,normalizedFeedURL,publicAddress,fetchCalendarFeed,MAX_FEED_BYTES} from "../src/calendar-feed"
const clock=()=>new Date("2026-11-12T12:00:00Z"),token="calendar-admin-owner-token-at-least-32-characters",disposals:Array<()=>Promise<void>>=[]
afterEach(async()=>{await Promise.all(disposals.splice(0).map(d=>d()))})
const feed=(body:string)=>"BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Test//Calendar//EN\r\n"+body+"\r\nEND:VCALENDAR"
const event="BEGIN:VEVENT\r\nUID:meeting-1\r\nDTSTART:20261113T150000Z\r\nDTEND:20261113T160000Z\r\nSUMMARY:Product review\r\nLOCATION:https://meet.google.com/abc-defg-hij\r\nEND:VEVENT"
function fixture(fetchFeed:(url:string)=>Promise<string>=async()=>feed(event)){
 const repository=makeMemoryRepository([],clock),store=makeMemoryCalendarStore(repository),box=secretBoxFromKey(randomBytes(32)),admin=makeAdministrationService(store,box,{clock,fetchFeed,environment:{OPENROUTER_API_KEY:"environment-router-key"}}),api=makeApiHandler(repository,token,{clock,administration:admin});disposals.push(api.dispose)
 const request=(path:string,method="GET",body?:unknown,owner=true)=>api.handler(new Request("http://localhost"+path,{method,headers:{...(owner?{Authorization:"Bearer "+token}:{}),...(body===undefined?{}:{"Content-Type":"application/json"})},...(body===undefined?{}:{body:JSON.stringify(body)})}))
 return{repository,store,admin,request}
}
const create=(f:ReturnType<typeof fixture>,name="Personal")=>f.request("/api/v1/calendars","POST",{name,url:"webcal://calendar.example.com/private/secret-subscription?token=secret"}).then(r=>r.json())
test("calendar and settings APIs enforce owner auth, bound fields/ranges and never expose secrets",async()=>{
 const f=fixture();for(const path of["/api/v1/admin/settings","/api/v1/calendars","/api/v1/calendar/events"])expect((await f.request(path,"GET",undefined,false)).status).toBe(401)
 expect((await f.request("/api/v1/admin/settings","PATCH",{calendarRefreshMinutes:0})).status).toBe(400)
 expect((await f.request("/api/v1/calendars","POST",{name:"Private",url:"https://127.0.0.1/private"})).status).toBe(400)
 expect((await f.request("/api/v1/calendar/events?from=2026-01-01T00%3A00%3A00Z&to=2027-01-01T00%3A00%3A00Z")).status).toBe(400)
 expect((await f.request("/api/v1/calendar/events?from=invalid")).status).toBe(400)
 const c=await create(f);expect(c.sourceHost).toBe("calendar.example.com");expect(c.eventCount).toBe(1);expect(JSON.stringify(c)).not.toContain("secret");expect((await f.store.getCalendar(c.id))!.encryptedURL).not.toContain("secret-subscription")
 const updated=await(await f.request("/api/v1/admin/settings","PATCH",{openRouterApiKey:"stored-provider-secret",languageModel:"openai/test-model",calendarLookAheadDays:45})).json();expect(updated.openRouterSource).toBe("stored");expect(JSON.stringify(updated)).not.toContain("stored-provider-secret");expect((await f.store.getSettings())!.openRouterSecret).not.toContain("stored-provider-secret");expect(await f.admin.configuration()).toMatchObject({openRouterKey:"stored-provider-secret",languageModel:"openai/test-model"})
 await f.request("/api/v1/admin/settings","PATCH",{instanceName:"New instance"});expect((await f.admin.configuration()).openRouterKey).toBe("stored-provider-secret")
 await f.request("/api/v1/admin/settings","PATCH",{openRouterApiKey:null});expect((await f.admin.configuration()).openRouterKey).toBe("environment-router-key")
})
test("multiple feeds retain attribution, disable independently and deduplicate concurrent event notes",async()=>{
 const f=fixture(),a=await create(f),b=await create(f,"Work");let events=await(await f.request("/api/v1/calendar/events")).json();expect(events).toHaveLength(2);expect(new Set(events.map((e:any)=>e.calendarName)).size).toBe(2);expect(events[0].meetingUrl).toBe("https://meet.google.com/abc-defg-hij");expect(JSON.stringify(events)).not.toContain("meetingKey")
 const notes=await Promise.all([events[0],events[1],events[0]].map(e=>f.request("/api/v1/calendar/events/"+e.id+"/note","POST").then(r=>r.json())));expect(new Set(notes.map(n=>n.id)).size).toBe(1);expect(notes[0]).toMatchObject({body:"",enhanced:"",recordingState:"idle",transcriptSegments:[]})
 await f.request("/api/v1/calendars/"+a.id,"PATCH",{enabled:false});events=await(await f.request("/api/v1/calendar/events")).json();expect(events).toHaveLength(1);expect(events[0].calendarId).toBe(b.id);expect(events[0].noteId).toBe(notes[0].id)
 await f.request("/api/v1/calendars/"+b.id,"DELETE");expect(await(await f.request("/api/v1/calendar/events")).json()).toEqual([]);expect(await Effect.runPromise(f.repository.getNote(notes[0].id))).not.toBeNull()
})
test("failed sync preserves cached events and reports bounded errors; URL edit/delete reject stale async results",async()=>{
 let fail=false,release:(v:string)=>void=()=>{},pending=false;const f=fixture(async()=>{if(fail)throw new Error("https://calendar.example.com/private?secret=raw");if(pending)return new Promise(r=>{release=r});return feed(event)});const c=await create(f);fail=true
 const failed=await(await f.request("/api/v1/calendars/"+c.id+"/sync","POST")).json();expect(failed.eventCount).toBe(1);expect(failed.lastSyncError).not.toContain("raw");expect(await(await f.request("/api/v1/calendar/events")).json()).toHaveLength(1)
 fail=false;pending=true;const sync=Effect.runPromise(f.admin.syncCalendar(c.id));await new Promise(r=>setTimeout(r,5));await Effect.runPromise(f.admin.updateCalendar(c.id,{url:"https://new.example.com/feed"}));release(feed(event));await sync;expect(await(await f.request("/api/v1/calendar/events")).json()).toEqual([])
 const sync2=Effect.runPromise(f.admin.syncCalendar(c.id));await new Promise(r=>setTimeout(r,5));await Effect.runPromise(f.admin.deleteCalendar(c.id));release(feed(event));await expect(sync2).rejects.toBeDefined();expect(await f.store.listCalendars()).toEqual([])
 const g=fixture(async()=>{throw new Error("unreachable")});const retained=await create(g);expect(retained.id).toBeDefined();expect(retained.lastSyncError).toBeDefined();expect(await g.store.listCalendars()).toHaveLength(1)
})
test("recurrence handles DST, exclusions, moved and cancelled occurrences, all-day exclusive dates and stable IDs",async()=>{
 const source=feed("BEGIN:VEVENT\r\nUID:weekly\r\nDTSTART;TZID=America/New_York:20261025T090000\r\nDTEND;TZID=America/New_York:20261025T100000\r\nRRULE:FREQ=WEEKLY;COUNT=5\r\nEXDATE;TZID=America/New_York:20261108T090000\r\nSUMMARY:Weekly\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:weekly\r\nRECURRENCE-ID;TZID=America/New_York:20261115T090000\r\nDTSTART;TZID=America/New_York:20261115T110000\r\nDTEND;TZID=America/New_York:20261115T120000\r\nSUMMARY:Moved\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:weekly\r\nRECURRENCE-ID;TZID=America/New_York:20261122T090000\r\nDTSTART;TZID=America/New_York:20261122T090000\r\nSTATUS:CANCELLED\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:holiday\r\nDTSTART;VALUE=DATE:20261114\r\nDTEND;VALUE=DATE:20261116\r\nSUMMARY:Holiday\r\nEND:VEVENT")
 const id=crypto.randomUUID(),events=await parseCalendarFeed(id,source,clock());expect(events).toHaveLength(4);expect(events.find(e=>e.startDate==="2026-10-25")!.startAt).toBe("2026-10-25T13:00:00.000Z");expect(events.find(e=>e.startDate==="2026-11-01")!.startAt).toBe("2026-11-01T14:00:00.000Z");expect(events.find(e=>e.title==="Moved")!.startAt).toBe("2026-11-15T16:00:00.000Z");expect(events.find(e=>e.title==="Holiday")).toMatchObject({allDay:true,startDate:"2026-11-14",endDate:"2026-11-16"});expect((await parseCalendarFeed(id,source,clock())).map(e=>e.id)).toEqual(events.map(e=>e.id));expect(events.every(e=>/^[a-f0-9-]{36}$/.test(e.id))).toBe(true)
 await expect(parseCalendarFeed(id,"x".repeat(MAX_FEED_BYTES+1),clock())).rejects.toThrow()
})
test("subscription fetch blocks private literals and mixed public/private DNS before sending a request",async()=>{
 for(const url of["http://example.com/feed","https://localhost/feed","https://user:pass@example.com/feed","https://127.0.0.1/feed","https://169.254.169.254/feed","https://[::1]/feed","https://example.com:8443/feed"])expect(()=>normalizedFeedURL(url)).toThrow()
 for(const address of["10.1.2.3","127.0.0.1","169.254.1.1","100.64.1.2","::1","::ffff:127.0.0.1","fe80::1"])expect(publicAddress(address)).toBe(false)
 expect(publicAddress("8.8.8.8")).toBe(true);expect(publicAddress("2606:4700:4700::1111")).toBe(true)
 await expect(fetchCalendarFeed("https://public.example.com/feed",async()=>[{address:"8.8.8.8",family:4},{address:"127.0.0.1",family:4}])).rejects.toThrow("Private")
})
test("stable file encryption survives restarts, authenticates ciphertext and uses a private file",async()=>{
 const path=await mkdtemp(join(tmpdir(),"calendar-key-"));try{const key=join(path,"state","settings.key"),first=await loadSecretBox(key),cipher=first.seal("provider-secret");expect(cipher).not.toContain("provider-secret");expect((await loadSecretBox(key)).open(cipher)).toBe("provider-secret");expect((await readFile(key)).length).toBe(32);expect((await stat(key)).mode&0o777).toBe(0o600);expect(()=>first.open(cipher.slice(0,-3)+"bad")).toThrow()}finally{await rm(path,{recursive:true,force:true})}
})
test("provider overrides immediately affect requests without adding calendar context",async()=>{
 const f=fixture(),requests:any[]=[];const transport=(async(_url:unknown,init:any)=>{requests.push({headers:init.headers,body:JSON.parse(init.body)});return Response.json({choices:[{message:{content:"Answer"}}]})}) as typeof fetch;const p=settingsAwareProviders(f.admin,transport);const note=await Effect.runPromise(f.repository.createNote({title:"Only note context"}));await Effect.runPromise(p.chat(note,{message:"Question",history:[]}));await Effect.runPromise(f.admin.updateSettings({openRouterApiKey:"new-live-provider-key",languageModel:"vendor/new-model"}));await Effect.runPromise(p.chat(note,{message:"Question",history:[]}));expect(requests[1].headers.Authorization).toBe("Bearer new-live-provider-key");expect(requests[1].body.model).toBe("vendor/new-model");expect(JSON.stringify(requests[1].body)).not.toContain("calendar")
})

test("calendar quotas and pathological recurrences stay bounded",async()=>{
 const f=fixture();for(let i=0;i<20;i++)expect((await f.request("/api/v1/calendars","POST",{name:"Source "+i,url:"https://calendar.example.com/feed"})).status).toBe(200)
 expect((await f.request("/api/v1/calendars","POST",{name:"Excess",url:"https://calendar.example.com/feed"})).status).toBe(400)
 const malicious=feed("BEGIN:VEVENT\r\nUID:too-frequent\r\nDTSTART:20261112T120000Z\r\nDTEND:20261112T120001Z\r\nRRULE:FREQ=SECONDLY;COUNT=99999999\r\nSUMMARY:Bounded recurrence\r\nEND:VEVENT")
 await expect(parseCalendarFeed(crypto.randomUUID(),malicious,clock())).rejects.toThrow()
 const c=await create(f);expect(c.error).toBe("invalid_request")
})
