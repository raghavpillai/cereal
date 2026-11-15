import {afterAll,beforeAll,describe,expect,test} from "bun:test"
import {createDatabase} from "@meeting-notes/database"
import {createMcpStore,McpStoreError,resolveNoteOrganization} from "../src/mcp-store"

const enabled=process.env.RUN_PG_TESTS==="1"&&!!process.env.DATABASE_URL&&/^\/mn_test_[a-f0-9]+$/.test(new URL(process.env.DATABASE_URL).pathname)
describe.skipIf(!enabled)("private MCP domain in PostgreSQL",()=>{
 const db=enabled?createDatabase(process.env.DATABASE_URL!):null,store=db?createMcpStore(db):null
 const alice=crypto.randomUUID(),bob=crypto.randomUUID(),one=crypto.randomUUID(),two=crypto.randomUUID()
 const scope={userId:alice,workspaceId:one},other={userId:bob,workspaceId:one}
 let own="",foreign="",elsewhere="",transcript="",longText="",longSegment="",folder="",image=""
 beforeAll(async()=>{
  await db!.user.createMany({data:[{id:alice,name:"Alice MCP",email:`mcp-${alice}@example.invalid`},{id:bob,name:"Bob MCP",email:`mcp-${bob}@example.invalid`}]})
  await db!.organization.createMany({data:[{id:one,name:"One",slug:`one-${one}`},{id:two,name:"Two",slug:`two-${two}`}]})
  await db!.member.createMany({data:[{id:crypto.randomUUID(),userId:alice,organizationId:one,role:"member"},{id:crypto.randomUUID(),userId:alice,organizationId:two,role:"member"},{id:crypto.randomUUID(),userId:bob,organizationId:one,role:"member"}]})
  await db!.userWorkspaceState.create({data:{userId:alice,activeOrganizationId:one}})
  const make=(ownerId:string,organizationId:string,title:string,body:string)=>db!.note.create({data:{ownerId,organizationId,title,body,date:new Date("2026-11-13T00:00:00Z")}})
  own=(await make(alice,one,"Cedar release","Original private notes 🦊")).id
  foreign=(await make(bob,one,"FOREIGN_OWNER_CANARY","DO_NOT_RETURN_OTHER_OWNER")).id
  elsewhere=(await make(alice,two,"OTHER_WORKSPACE_CANARY","Private second workspace")).id
  longText="Café 🦊 ".repeat(130)+"FINAL EXACT END";longSegment=crypto.randomUUID()
  transcript=(await db!.note.create({data:{ownerId:alice,organizationId:one,title:"Long transcript",body:"",date:new Date("2026-11-12T00:00:00Z"),transcriptRevision:1,transcriptSegments:{create:[{id:longSegment,channel:"mic",speakerId:`mic:${crypto.randomUUID()}:speaker_0`,speakerLabel:"Speaker 1",text:longText,startMs:1000,isFinal:true},{id:crypto.randomUUID(),channel:"system",speakerId:`system:${crypto.randomUUID()}:speaker_0`,speakerLabel:"Speaker 1",text:"transcriptoraclecanary remote answer.",startMs:2000,isFinal:true}]}}})).id
  await db!.noteMeetingContext.create({data:{noteId:own,userId:alice,meeting:{eventId:crypto.randomUUID(),provider:"ics",title:"Explicit meeting",calendarName:"Explicit",allDay:false,startAt:"2026-11-13T12:00:00Z",endAt:"2026-11-13T13:00:00Z",timeZone:"UTC",location:null,eventUrl:null,meetingUrl:null,organizer:null,participants:[{id:crypto.randomUUID(),displayName:"Avery Chen",givenName:"Avery",familyName:"Chen",email:"avery@acme.example",nameSource:"manual",isSelf:false,isOrganizer:false,isResource:false,responseStatus:"accepted"}]}}})
 })
 afterAll(async()=>{await db?.user.deleteMany({where:{id:{in:[alice,bob]}}});await db?.organization.deleteMany({where:{id:{in:[one,two]}}});await db?.$disconnect()})
 test("all meeting reads and transcript search preserve owner and workspace boundaries",async()=>{
  const meetings=await store!.listMeetings(scope,{limit:100})
  expect(meetings.meetings.map(note=>note.id)).toContain(own)
  expect(JSON.stringify(meetings)).not.toContain("FOREIGN_OWNER_CANARY")
  expect(JSON.stringify(meetings)).not.toContain("OTHER_WORKSPACE_CANARY")
  await expect(store!.getMeetings(scope,[foreign])).rejects.toBeInstanceOf(McpStoreError)
  await expect(store!.getMeetings(scope,[elsewhere])).rejects.toBeInstanceOf(McpStoreError)
  expect((await store!.listMeetings(scope,{query:"transcriptoraclecanary",limit:20})).meetings).toEqual([])
  expect((await store!.listMeetings(scope,{query:"transcriptoraclecanary",limit:20,includeTranscriptMatch:true})).meetings.map(note=>note.id)).toEqual([transcript])
  const hits=await store!.searchTranscripts(scope,{query:"transcriptoraclecanary",limit:20})
  expect(hits.passages[0]?.noteId).toBe(transcript)
  expect(hits.passages[0]?.quote).toContain("transcriptoraclecanary")
  expect(hits.passages[0]?.channel).toBe("system")
  expect((await store!.searchTranscripts(other,{query:"transcriptoraclecanary",limit:20})).passages).toEqual([])
 })
 test("literal attendee filters use explicit associated names/emails and cannot alter SQL",async()=>{
  const actual=await store!.listMeetings(scope,{attendee:"Avery",limit:20})
  expect(actual.meetings.map(note=>note.id)).toEqual([own])
  expect(actual.meetings[0]?.meeting?.attendees[0]).toMatchObject({name:"Avery Chen",givenName:"Avery",familyName:"Chen",email:"avery@acme.example",nameSource:"manual"})
  expect((await store!.listMeetings(scope,{company:"acme.example",limit:20})).meetings.map(note=>note.id)).toEqual([own])
  expect((await store!.listMeetings(scope,{attendee:"%' OR true--",limit:20})).meetings).toEqual([])
 })
 test("full transcripts paginate without losing Unicode or conflating sources; changed cursors fail",async()=>{
  let cursor:string|undefined,combined="",pages=0
  do{const page=await store!.transcript(scope,transcript,{limit:1,maxCharacters:256,...(cursor?{cursor}:{})});for(const segment of page.segments)if(segment.id===longSegment)combined+=segment.text;cursor=page.nextCursor??undefined;pages++;expect(pages).toBeLessThan(20)}while(cursor)
  expect(combined).toBe(longText)
  const page=await store!.transcript(scope,transcript,{limit:1,maxCharacters:256})
  await db!.note.update({where:{id:transcript},data:{title:"Long transcript changed"}})
  await expect(store!.transcript(scope,transcript,{limit:1,maxCharacters:256,cursor:page.nextCursor!})).rejects.toBeInstanceOf(McpStoreError)
 })
 test("private folders are persistent and never share membership across owners or workspaces",async()=>{
  const created=await store!.createFolder(scope,"Hiring","Explicit folder description");folder=created.id
  await store!.updateFolder(scope,folder,{name:"Hiring 2027"})
  await store!.folderNote(scope,folder,own,true);await store!.folderNote(scope,folder,own,true)
  expect((await store!.listFolders(scope)).folders.find(row=>row.id===folder)?.noteCount).toBe(1)
  await expect(store!.folderNote(scope,folder,foreign,true)).rejects.toBeInstanceOf(McpStoreError)
  await expect(store!.folderNote(scope,folder,elsewhere,true)).rejects.toBeInstanceOf(McpStoreError)
  await expect(store!.updateFolder(other,folder,{name:"Not ours"})).rejects.toBeInstanceOf(McpStoreError)
  expect((await store!.listMeetings(scope,{folderId:folder,limit:20})).meetings.map(note=>note.id)).toEqual([own])
  await store!.folderNote(scope,folder,own,false)
  expect((await store!.listFolders(scope)).folders.find(row=>row.id===folder)?.noteCount).toBe(0)
 })
 test("simultaneous private-note appends preserve every write and UTF16 limits",async()=>{
  await Promise.all(Array.from({length:8},(_,index)=>store!.updatePrivateNotes(scope,own,`Append ${index} 🦊`,"append")))
  const note=await db!.note.findUniqueOrThrow({where:{id:own}})
  expect(note.body.startsWith("Original private notes 🦊")).toBe(true)
  for(let index=0;index<8;index++)expect(note.body.split(`Append ${index} 🦊`).length).toBe(2)
  await store!.updatePrivateNotes(scope,own,"Prep questions","prepend")
  expect((await db!.note.findUniqueOrThrow({where:{id:own}})).body.startsWith("Prep questions\n\nOriginal")).toBe(true)
  await expect(store!.updatePrivateNotes(scope,foreign,"Do not write","append")).rejects.toBeInstanceOf(McpStoreError)
  const huge=await db!.note.create({data:{ownerId:alice,organizationId:one,title:"UTF16",body:"🦊".repeat(499999),date:new Date("2026-11-13")}})
  await expect(store!.updatePrivateNotes(scope,huge.id,"x","append")).rejects.toBeInstanceOf(McpStoreError)
  expect((await db!.note.findUniqueOrThrow({where:{id:huge.id}})).body).toBe(huge.body)
 })
 test("attachments return actual private bytes and agent briefs remain explicit owner input",async()=>{
  const png=Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+kV1sAAAAASUVORK5CYII=","base64")
  image=(await store!.storeAttachment(scope,own,"Actual fixture.png","image/png",png)).id
  expect(Buffer.from((await store!.getAttachment(scope,image)).data)).toEqual(png)
  await expect(store!.getAttachment(other,image)).rejects.toBeInstanceOf(McpStoreError)
  await expect(store!.storeAttachment(scope,foreign,"No.png","image/png",png)).rejects.toBeInstanceOf(McpStoreError)
  await expect(store!.storeAttachment(scope,own,"Fake.png","image/png",new Uint8Array([1,2,3]))).rejects.toBeInstanceOf(McpStoreError)
  expect((await store!.agentBrief(scope)).body).toBe("")
  await store!.updateAgentBrief(scope,"My role is engineering. These facts were entered by me.")
  expect((await store!.agentBrief(scope)).automaticallyInferred).toBe(false)
  expect((await store!.agentBrief(other)).body).toBe("")
 })
 test("global workspace switches validate membership and update sessions without granting peer notes",async()=>{
  const session=await db!.session.create({data:{id:crypto.randomUUID(),userId:alice,token:crypto.randomUUID(),expiresAt:new Date(Date.now()+3600000)}})
  await store!.switchWorkspace(alice,two)
  expect(await resolveNoteOrganization(db!,alice)).toBe(two)
  expect((await db!.session.findUniqueOrThrow({where:{id:session.id}})).activeOrganizationId).toBe(two)
  await expect(store!.switchWorkspace(bob,two)).rejects.toBeInstanceOf(McpStoreError)
  expect((await store!.listMeetings({userId:alice,workspaceId:two},{limit:20})).meetings.map(note=>note.id)).toEqual([elsewhere])
 })
})
