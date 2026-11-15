import {afterEach,describe,expect,test} from "bun:test"
import type {MeetingContextDTO,NoteDTO,NoteSearchPageDTO,NoteSummariesPageDTO} from "@meeting-notes/contracts"
import type {PrincipalData} from "@meeting-notes/contracts/api"
import {makeApiHandler} from "../src/api"
import {makeMemoryRepository} from "../src/repository"
import {attendeeSummary,excerptParts,highlightParts,initials,noteExcerpt,noteSearchTerms,parseListInput,parseSearchInput,selectNoteSearch,selectNoteSummaries,summarizeNote,withMeeting} from "../src/note-summaries"

const token="test-owner-token-with-at-least-32-characters"
const disposals:Array<()=>Promise<void>>=[]
afterEach(async()=>{for(const dispose of disposals.splice(0))await dispose()})
const users={ada:"11111111-1111-4111-8111-111111111111",bob:"22222222-2222-4222-8222-222222222222"}
function fixture() {
  let now=Date.parse("2026-11-01T09:00:00.000Z")
  // Every write gets its own instant, like rows written one after another.
  const repository=makeMemoryRepository([],()=>new Date(now+=1000))
  const principal=(id:string):PrincipalData=>({id,name:id===users.ada?"Ada":"Bob",email:`${id}@example.test`,role:"user",source:"session",expiresAt:null,createdAt:"2026-11-01T00:00:00.000Z"})
  const api=makeApiHandler(repository,token,{principalResolver:async headers=>{const user=headers.get("x-test-user");return user?principal(user):null}})
  disposals.push(api.dispose)
  const request=(user:string|null,path:string,method="GET",body?:unknown)=>api.handler(new Request(`http://localhost${path}`,{method,
    headers:{...(user?{"x-test-user":user,Authorization:`Bearer ${token}`}:{}),...(body===undefined?{}:{"Content-Type":"application/json"})},...(body===undefined?{}:{body:JSON.stringify(body)})}))
  const create=async(user:string,input:Record<string,unknown>)=>{const response=await request(user,"/api/v1/notes","POST",input);expect(response.status).toBe(200);return await response.json() as NoteDTO}
  const list=async(user:string,query="")=>{const response=await request(user,`/api/v1/notes/summaries${query}`);expect(response.status).toBe(200);return await response.json() as NoteSummariesPageDTO}
  const search=async(user:string,query:string)=>{const response=await request(user,`/api/v1/notes/search?${query}`);expect(response.status).toBe(200);return await response.json() as NoteSearchPageDTO}
  return{request,create,list,search}
}
const segment=(text:string,speakerId="self",startMs=0)=>({id:crypto.randomUUID(),channel:"mic",speakerId,speakerLabel:null,text,isFinal:true,startMs,endMs:startMs+1000})
const day=(offset:number)=>new Date(Date.UTC(2026,9,1+offset)).toISOString().slice(0,10)

describe("notes list summaries",()=>{
  test("both routes require an account and never collide with the single-note route",async()=>{
    const{request,create}=fixture()
    expect((await request(null,"/api/v1/notes/summaries")).status).toBe(401)
    expect((await request(null,"/api/v1/notes/search?q=plan")).status).toBe(401)
    const note=await create(users.ada,{title:"Quarterly plan"})
    expect((await (await request(users.ada,`/api/v1/notes/${note.id}`)).json()).id).toBe(note.id)
    expect((await request(users.ada,"/api/v1/notes/summaries")).status).toBe(200)
    expect((await request(users.ada,"/api/v1/notes/search?q=plan")).status).toBe(200)
    // The full list keeps its shape for clients that still use it.
    const full=await (await request(users.ada,"/api/v1/notes")).json()
    expect(Array.isArray(full)&&full[0].transcriptSegments).toEqual([])
  })

  test("a row carries what a list shows and neither the body nor the transcript",async()=>{
    const{create,list}=fixture()
    const body="Typed PRIVATE_BODY_SENTINEL ".repeat(200),transcript="Spoken PRIVATE_TRANSCRIPT_SENTINEL ".repeat(200)
    const note=await create(users.ada,{title:"Pricing review",date:"2026-10-20",duration:"31:07",body,enhanced:"# Decisions\n\n- **Pricing** holds until [March](https://example.test/x)\n- Follow up with legal",
      transcriptSegments:[segment(transcript,"mic:a"),segment("Second voice","mic:b",2000),segment("   ","mic:c",3000)]})
    const page=await list(users.ada)
    expect(page.total).toBe(1)
    expect(page.nextCursor).toBeNull()
    const row=page.notes[0]!
    expect(Object.keys(row).sort()).toEqual(["attendees","createdAt","date","duration","excerpt","folders","generatedRevision","generationState","hasBody","hasSummary","hasTranscript","id","meeting","organizationId","recordingState","speakerCount","title","transcriptRevision","updatedAt"])
    expect(row).toMatchObject({id:note.id,title:"Pricing review",date:"2026-10-20",duration:"31:07",hasBody:true,hasSummary:true,hasTranscript:true,speakerCount:2,
      recordingState:"idle",generationState:"idle",transcriptRevision:1,generatedRevision:null,updatedAt:note.updatedAt,createdAt:note.createdAt,
      excerpt:"Pricing holds until March. Follow up with legal.",meeting:null,attendees:{people:[],others:0,total:0},organizationId:null,folders:[]})
    const wire=JSON.stringify(page)
    expect(wire).not.toContain("PRIVATE_BODY_SENTINEL")
    expect(wire).not.toContain("PRIVATE_TRANSCRIPT_SENTINEL")
    expect(wire.length).toBeLessThan(900)
  })

  test("the order is newest meeting first with creation time and ID breaking ties",async()=>{
    const{create,list}=fixture()
    const old=await create(users.ada,{title:"Old",date:"2026-10-01"})
    const first=await create(users.ada,{title:"Same day, first",date:"2026-10-05"})
    const second=await create(users.ada,{title:"Same day, second",date:"2026-10-05"})
    const newest=await create(users.ada,{title:"Newest",date:"2026-10-09"})
    expect((await list(users.ada)).notes.map(note=>note.id)).toEqual([newest.id,second.id,first.id,old.id])
    // Identical dates and instants still have one order.
    const twin=(id:string):NoteDTO=>({...old,id,title:id})
    const ids=["aaaaaaaa-0000-4000-8000-000000000001","aaaaaaaa-0000-4000-8000-000000000003","aaaaaaaa-0000-4000-8000-000000000002"]
    const a=selectNoteSummaries(ids.map(twin),parseListInput({limit:"2"}))
    expect(a.notes.map(note=>note.id)).toEqual([ids[1]!,ids[2]!])
    expect(selectNoteSummaries(ids.map(twin),parseListInput({limit:"2",cursor:a.nextCursor!})).notes.map(note=>note.id)).toEqual([ids[0]!])
  })

  test("paging while notes are being added has no duplicates and no omissions",async()=>{
    const{create,list}=fixture()
    const original:string[]=[]
    for(let i=0;i<23;i++)original.push((await create(users.ada,{title:`Meeting ${i}`,date:day(i%9)})).id)
    const seen:string[]=[],added:{id:string;date:string}[]=[]
    let cursor:string|null=null,pages=0
    do{
      const page:NoteSummariesPageDTO=await list(users.ada,`?limit=5${cursor?`&cursor=${cursor}`:""}`)
      seen.push(...page.notes.map(note=>note.id))
      cursor=page.nextCursor;pages++
      // Between requests: one note newer than everything, one in the middle of
      // the days already listed, one older than everything.
      for(const date of[day(40+pages),day(4),day(-30-pages)])added.push({id:(await create(users.ada,{title:`Added during page ${pages}`,date})).id,date})
    }while(cursor)
    expect(new Set(seen).size).toBe(seen.length)
    for(const id of original)expect(seen).toContain(id)
    // What was added lands either wholly before the reader (not shown this pass) or after it (shown once).
    const final=(await list(users.ada,"?limit=100")).notes.map(note=>note.id)
    expect(final.length).toBe(original.length+added.length)
    expect(seen).toEqual(final.filter(id=>seen.includes(id)))
    for(const note of added.filter(note=>note.date<day(0)))if(added.indexOf(note)<added.length-3)expect(seen).toContain(note.id)
    expect((await list(users.ada)).total).toBe(original.length+added.length)
  })

  test("filters are applied before paging and a cursor cannot cross filters",async()=>{
    const{create,list,request}=fixture()
    for(let i=0;i<6;i++)await create(users.ada,{title:`Note ${i}`,date:day(i),...(i%2?{enhanced:"Summary text"}:{}),...(i%3===0?{transcriptSegments:[segment("Words")]}:{})})
    const summarized=await list(users.ada,"?filter=summarized&limit=2")
    expect(summarized.total).toBe(3)
    expect(summarized.notes.every(note=>note.hasSummary)).toBe(true)
    expect((await list(users.ada,"?filter=transcribed")).notes.map(note=>note.title)).toEqual(["Note 3","Note 0"])
    expect((await list(users.ada,`?filter=summarized&limit=2&cursor=${summarized.nextCursor}`)).notes.map(note=>note.title)).toEqual(["Note 1"])
    for(const query of[`?cursor=${summarized.nextCursor}`,"?cursor=not_a_cursor","?cursor=!!","?limit=0","?limit=abc","?limit=1000","?filter=starred"])
      expect((await request(users.ada,`/api/v1/notes/summaries${query}`)).status).toBe(400)
    expect((await list(users.ada,"?limit=999")).notes.length).toBe(6)
  })

  test("one account never sees another account's notes in the list or in search",async()=>{
    const{create,list,search,request}=fixture()
    const mine=await create(users.ada,{title:"Ada roadmap",body:"SHARED_WORD budget",transcriptSegments:[segment("SHARED_WORD spoken by Ada")]})
    const theirs=await create(users.bob,{title:"Bob roadmap",body:"SHARED_WORD budget",enhanced:"BOB_ONLY summary",transcriptSegments:[segment("SHARED_WORD spoken by Bob")]})
    expect((await list(users.ada)).notes.map(note=>note.id)).toEqual([mine.id])
    expect((await list(users.bob)).notes.map(note=>note.id)).toEqual([theirs.id])
    for(const query of["q=roadmap","q=shared_word","q=spoken","q=budget"]){
      expect((await search(users.ada,query)).results.map(result=>result.note.id)).toEqual([mine.id])
      expect((await search(users.bob,query)).results.map(result=>result.note.id)).toEqual([theirs.id])
    }
    expect((await search(users.ada,"q=bob_only")).total).toBe(0)
    expect(JSON.stringify(await search(users.ada,"q=roadmap"))).not.toContain("Bob")
    // A cursor is a position, not a capability: Bob's cursor in Ada's hands lists only Ada's notes.
    for(let i=0;i<3;i++)await create(users.bob,{title:`Bob extra ${i}`})
    const bobs=await list(users.bob,"?limit=1")
    const crossed=await list(users.ada,`?limit=1&cursor=${bobs.nextCursor}`)
    expect(crossed.notes.every(note=>note.id===mine.id)).toBe(true)
    expect((await request(users.ada,`/api/v1/notes/${theirs.id}`)).status).toBe(404)
  })
})

describe("notes search",()=>{
  test("ranks a title match above the summary, typed notes and transcript, newest first within a rank",async()=>{
    const{create,search}=fixture()
    const transcript=await create(users.ada,{title:"Weekly sync",date:day(9),transcriptSegments:[segment("We should revisit the Harrowgate renewal next week")]})
    const typed=await create(users.ada,{title:"Scratch",date:day(8),body:"Call Harrowgate about seats"})
    const summary=await create(users.ada,{title:"Customer review",date:day(1),enhanced:"## Accounts\n- Harrowgate wants a discount"})
    const title=await create(users.ada,{title:"Harrowgate pricing call",date:day(0)})
    const both=await create(users.ada,{title:"Harrowgate kickoff",date:day(2),enhanced:"Harrowgate onboarding plan",transcriptSegments:[segment("Welcome, Harrowgate team")]})
    await create(users.ada,{title:"Unrelated",date:day(9),body:"Nothing to see"})
    const page=await search(users.ada,"q=harrowgate")
    expect(page.terms).toEqual(["harrowgate"])
    expect(page.total).toBe(5)
    expect(page.results.map(result=>result.note.id)).toEqual([both.id,title.id,summary.id,transcript.id,typed.id])
    expect(page.results.map(result=>result.matchedIn)).toEqual([["title","summary","transcript"],["title"],["summary"],["transcript"],["notes"]])
    // Equal rank: the transcript-only and typed-notes matches both score 1, and the newer meeting leads.
    expect(page.results[1]!.title).toEqual([{text:"Harrowgate",hit:true},{text:" pricing call",hit:false}])
    expect(page.results[1]!.excerpt).toBeNull()
    expect(page.results[2]!.excerpt).toMatchObject({source:"summary",segmentId:null,startMs:null,parts:[{text:"Accounts ",hit:false},{text:"Harrowgate",hit:true},{text:" wants a discount",hit:false}]})
    expect(page.results[3]!.excerpt).toMatchObject({source:"transcript",segmentId:transcript.transcriptSegments[0]!.id,startMs:0})
    expect(page.results[3]!.excerpt!.parts.filter(part=>part.hit).map(part=>part.text)).toEqual(["Harrowgate"])
    expect(page.results[4]!.excerpt!.source).toBe("notes")
  })

  test("every word must match somewhere; words found together in the title lead",async()=>{
    const{create,search}=fixture()
    const apart=await create(users.ada,{title:"Budget",date:day(5),body:"review of the hiring plan"})
    const together=await create(users.ada,{title:"Hiring plan review",date:day(1)})
    const scattered=await create(users.ada,{title:"Plan for hiring",date:day(3)})
    await create(users.ada,{title:"Hiring only",date:day(6)})
    const page=await search(users.ada,`q=${encodeURIComponent("  Hiring   PLAN ")}`)
    expect(page.terms).toEqual(["hiring","plan"])
    expect(page.results.map(result=>result.note.id)).toEqual([together.id,scattered.id,apart.id])
    expect((await search(users.ada,"q=hiring+plan+zebra")).total).toBe(0)
  })

  test("search pages are stable, filtered, and a cursor belongs to its query",async()=>{
    const{create,search,request}=fixture()
    const ids:string[]=[]
    for(let i=0;i<11;i++)ids.push((await create(users.ada,{title:i%2?`Roadmap ${i}`:`Notes ${i}`,date:day(i),...(i%2?{}:{body:"roadmap"}),...(i<4?{enhanced:"Written"}:{})})).id)
    const seen:string[]=[]
    let cursor:string|null=null
    do{
      const page:NoteSearchPageDTO=await search(users.ada,`q=roadmap&limit=4${cursor?`&cursor=${cursor}`:""}`)
      expect(page.total).toBe(11)
      seen.push(...page.results.map(result=>result.note.id));cursor=page.nextCursor
      if(cursor)expect((await request(users.ada,`/api/v1/notes/search?q=notes&cursor=${cursor}`)).status).toBe(400)
    }while(cursor)
    const titled=ids.filter((_,i)=>i%2).reverse(),typed=ids.filter((_,i)=>i%2===0).reverse()
    expect(seen).toEqual([...titled,...typed])
    expect((await search(users.ada,"q=roadmap&filter=summarized")).results.map(result=>result.note.id)).toEqual([ids[3]!,ids[1]!,ids[2]!,ids[0]!])
    expect((await request(users.ada,"/api/v1/notes/search")).status).toBe(400)
    expect((await request(users.ada,`/api/v1/notes/search?q=${"x".repeat(201)}`)).status).toBe(400)
  })

  test("queries are literal text: wildcards, quotes and one-letter words do nothing special",async()=>{
    const{create,search}=fixture()
    const percent=await create(users.ada,{title:"Discount of 100% agreed"})
    await create(users.ada,{title:"Discount of 1000 agreed"})
    const under=await create(users.ada,{title:"snake_case rename"})
    await create(users.ada,{title:"snakeXcase rename"})
    expect((await search(users.ada,`q=${encodeURIComponent("snake_case")}`)).results.map(result=>result.note.id)).toEqual([under.id])
    expect((await search(users.ada,`q=${encodeURIComponent("100%")}`)).results.map(result=>result.note.id)).toContain(percent.id)
    expect((await search(users.ada,`q=${encodeURIComponent("'; DROP TABLE \"Note\"; --")}`)).terms).toEqual(["drop","table","note"])
    for(const empty of["q=","q=a","q=%20","q=%25"])expect(await search(users.ada,empty)).toEqual({results:[],nextCursor:null,total:0,terms:[]})
    expect(noteSearchTerms("Ada's e-mail ada@example.test v2.1, 日 a")).toEqual(["ada's","e-mail","ada@example.test","v2.1","日"])
    expect(noteSearchTerms("one two three four five six seven eight nine ten").length).toBe(8)
  })
})

describe("row content",()=>{
  const person=(input:Partial<MeetingContextDTO["participants"][number]>&{id:string}):MeetingContextDTO["participants"][number]=>({email:null,displayName:null,givenName:null,familyName:null,nameSource:"calendar",isSelf:false,isOrganizer:false,isResource:false,responseStatus:"accepted",...input})
  const meeting=(participants:MeetingContextDTO["participants"]):MeetingContextDTO=>({eventId:crypto.randomUUID(),provider:"google",title:"Review",calendarName:"Work",startAt:"2026-10-20T15:00:00.000Z",endAt:"2026-10-20T15:30:00.000Z",allDay:false,timeZone:null,location:null,meetingUrl:null,eventUrl:null,organizer:null,participants})
  const note:NoteDTO={id:crypto.randomUUID(),title:"Review",body:"",enhanced:"",date:"2026-10-20",duration:"",createdAt:"2026-10-20T15:00:00.000Z",updatedAt:"2026-10-20T15:00:00.000Z",transcriptSegments:[],recordingState:"idle",generationState:"idle",transcriptRevision:0,generatedRevision:null}

  test("attendees leave out rooms and the account holder, are capped, and keep the full counts",()=>{
    const people=[person({id:crypto.randomUUID(),displayName:"Me Myself",isSelf:true}),person({id:crypto.randomUUID(),displayName:"Room 4",isResource:true}),
      person({id:crypto.randomUUID(),displayName:" Grace Hopper ",givenName:"Grace"}),person({id:crypto.randomUUID(),email:"alan.turing@example.test"}),person({id:crypto.randomUUID()}),
      ...Array.from({length:6},(_,i)=>person({id:crypto.randomUUID(),displayName:`Guest ${i+1}`}))]
    const summary=attendeeSummary(meeting(people))
    expect(summary).toEqual({total:10,others:9,people:[{name:"Grace Hopper",shortName:"Grace",initials:"GH"},{name:"alan.turing@example.test",shortName:"alan.turing",initials:"AT"},
      {name:"Unnamed participant",shortName:"Unnamed",initials:"UP"},{name:"Guest 1",shortName:"Guest",initials:"G1"}]})
    expect(attendeeSummary(null)).toEqual({people:[],others:0,total:0})
    const row=withMeeting(summarizeNote(note),meeting(people))
    expect(row.meeting).toEqual({startAt:"2026-10-20T15:00:00.000Z",endAt:"2026-10-20T15:30:00.000Z",allDay:false})
    expect(row.attendees.total).toBe(10)
    expect([initials("ada"),initials("Ada Lovelace King"),initials("  "),initials("Guest 12")]).toEqual(["A","AK","?","G12"])
  })

  test("attendee names are searchable and explain the match",()=>{
    const context=meeting([person({id:crypto.randomUUID(),displayName:"Grace Hopper",email:"grace@navy.example"}),person({id:crypto.randomUUID(),displayName:"Hopper Room",isResource:true})])
    const page=selectNoteSearch([note],parseSearchInput({q:"hopper"}),()=>({meeting:context}))
    expect(page.results[0]).toMatchObject({matchedIn:["people"],excerpt:{source:"people",parts:[{text:"Grace ",hit:false},{text:"Hopper",hit:true}]}})
    expect(selectNoteSearch([note],parseSearchInput({q:"navy.example"}),()=>({meeting:context})).total).toBe(1)
    expect(selectNoteSearch([note],parseSearchInput({q:"room"}),()=>({meeting:context})).total).toBe(0)
  })

  test("excerpts drop headings, end list items as sentences and never split a character",()=>{
    expect(noteExcerpt("# Title\n\nFirst point\n- second point\n```\ncode\n```\n","ignored")).toBe("First point. second point.")
    expect(noteExcerpt("","Typed **notes** only")).toBe("Typed notes only.")
    const long=noteExcerpt("word ".repeat(100),"")
    expect(long.length).toBeLessThanOrEqual(151)
    expect(long.endsWith("…")).toBe(true)
    expect(/[\uD800-\uDBFF](?![\uDC00-\uDFFF])/.test(noteExcerpt("😀".repeat(200),""))).toBe(false)
    // A document cut off by the list query does not show its broken last line.
    expect(noteExcerpt(`Whole line\n${"cut".repeat(600)}`.slice(0,1600),"",false)).toBe("Whole line.")
    expect(highlightParts("Plan the PLANNING",["plan","planning"])).toEqual([{text:"Plan",hit:true},{text:" the ",hit:false},{text:"PLANNING",hit:true}])
    const parts=excerptParts(`${"before ".repeat(40)}needle ${"after ".repeat(80)}`,["needle"])!
    expect(parts.map(part=>part.text).join("").length).toBeLessThan(230)
    expect(parts[0]!.text.startsWith("… ")).toBe(true)
    expect(parts[parts.length-1]!.text.endsWith(" …")).toBe(true)
    expect(excerptParts("nothing here",["needle"])).toBeNull()
  })
})
