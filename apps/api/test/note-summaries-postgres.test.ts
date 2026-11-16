/**
 * The notes list and notes search against real PostgreSQL: isolation, paging
 * under writes, agreement with the in-memory reference, and the size and time of
 * a 500-note account with realistic transcripts (printed as a table).
 *
 * Runs only in the disposable `mn_test_<hex>` database made by
 * scripts/test-postgres-isolated.ts.
 */
import {afterAll,beforeAll,describe,expect,test} from "bun:test"
import {Effect} from "effect"
import type {MeetingContextDTO,NoteSearchPageDTO,NoteSummariesPageDTO} from "@meeting-notes/contracts"
import type {PrincipalData} from "@meeting-notes/contracts/api"
import {makeApiHandler} from "../src/api"
import {makePrismaRepository} from "../src/prisma-repository"
import {parseListInput,parseSearchInput,selectNoteSearch,selectNoteSummaries} from "../src/note-summaries"

const url=process.env.DATABASE_URL
const enabled=process.env.RUN_PG_TESTS==="1"&&!!url&&/^\/mn_test_[a-f0-9]+$/.test(new URL(url).pathname)
const NOTES=500,SEGMENTS=60
// Deterministic, so two runs measure the same bytes.
function random(seed:number){return()=>{seed=(seed*1664525+1013904223)>>>0;return seed/4294967296}}
const words="we the team should plan review budget launch customer pricing roadmap hiring design feedback timeline risk owner follow decision quarter release metrics support onboarding contract renewal estimate backlog priority agree next week think about because however actually really going need want make sure before after between update status blocker dependency migration rollout experiment results numbers forecast pipeline".split(" ")
const sentence=(next:()=>number,count:number)=>{const picked=Array.from({length:count},()=>words[Math.floor(next()*words.length)]!);return picked.join(" ").replace(/^./,letter=>letter.toUpperCase())+"."}
const dayOf=(index:number)=>new Date(Date.UTC(2026,0,1)+Math.floor(index/3)*86_400_000)

describe.skipIf(!enabled)("notes list and search on PostgreSQL",()=>{
  const resources=enabled?makePrismaRepository(url!):null,db=resources?.database
  const ada=crypto.randomUUID(),bob=crypto.randomUUID(),north=crypto.randomUUID(),south=crypto.randomUUID()
  const token="integration-owner-token-at-least-32-characters"
  const principal=(id:string):PrincipalData=>({id,name:"Test",email:`${id}@example.invalid`,role:"user",source:"session",expiresAt:null,createdAt:"2026-11-01T00:00:00.000Z"})
  const api=resources?makeApiHandler(resources.repository,token,{principalResolver:async headers=>{const user=headers.get("x-test-user");return user?principal(user):null}}):null
  const get=async(user:string,path:string)=>{
    const started=performance.now(),response=await api!.handler(new Request(`http://localhost${path}`,{headers:{"x-test-user":user,Authorization:`Bearer ${token}`}}))
    const bytes=new Uint8Array(await response.arrayBuffer())
    return{status:response.status,bytes:bytes.byteLength,ms:performance.now()-started,json:()=>JSON.parse(new TextDecoder().decode(bytes))}
  }
  const noteIds:string[]=[]
  let transcriptCharacters=0,needle={title:"",transcript:"",typed:""}
  const person=(displayName:string|null,email:string|null,extra:Partial<MeetingContextDTO["participants"][number]>={}):MeetingContextDTO["participants"][number]=>({id:crypto.randomUUID(),email,displayName,givenName:null,familyName:null,nameSource:"calendar",isSelf:false,isOrganizer:false,isResource:false,responseStatus:"accepted",...extra})
  const meeting=(participants:MeetingContextDTO["participants"]):MeetingContextDTO=>({eventId:crypto.randomUUID(),provider:"google",title:"Review",calendarName:"Work",startAt:"2026-03-02T15:00:00.000Z",endAt:"2026-03-02T15:30:00.000Z",allDay:false,timeZone:"UTC",location:null,meetingUrl:null,eventUrl:null,organizer:null,participants})

  beforeAll(async()=>{
    if(!db)return
    for(const [id,name]of[[ada,"Ada"],[bob,"Bob"]] as const)await db.user.create({data:{id,name,email:`list-${id}@example.invalid`}})
    for(const [id,name]of[[north,"North"],[south,"South"]] as const)await db.organization.create({data:{id,name,slug:`list-${id}`}})
    const next=random(20261114),notes=[],segments=[]
    for(let index=0;index<NOTES;index++){
      const id=crypto.randomUUID(),created=new Date(dayOf(index).getTime()+9*3_600_000+(index%3)*3_600_000)
      noteIds.push(id)
      const bullets=Array.from({length:8},()=>`- ${sentence(next,14)}`).join("\n")
      notes.push({id,ownerId:ada,organizationId:index%2?north:south,title:`${sentence(next,4).slice(0,-1)} ${index}`,body:index%4===0?"":`${sentence(next,20)}\n${sentence(next,18)}`,
        enhanced:index%5===0?"":`## Decisions\n${bullets}\n\n## Next steps\n${Array.from({length:5},()=>`- ${sentence(next,12)}`).join("\n")}`,
        date:dayOf(index),duration:`${20+index%40}:00`,createdAt:created,updatedAt:created,recordingState:"stopped",generationState:index%5===0?"idle":"ready",transcriptRevision:index%7===0?0:1,generatedRevision:index%5===0?null:index%7===0?0:1})
      if(index%7===0)continue
      for(let turn=0;turn<SEGMENTS;turn++){
        const text=sentence(next,22+Math.floor(next()*16))
        transcriptCharacters+=text.length
        segments.push({id:crypto.randomUUID(),noteId:id,channel:turn%2?"system" as const:"mic" as const,speakerId:`${turn%2?"system":"mic"}:speaker_${turn%3}`,speakerLabel:turn%2?`Speaker ${turn%3+1}`:null,text,isFinal:true,startMs:turn*18_000,endMs:turn*18_000+16_000})
      }
    }
    // Needles: one word that exists only in a title, one only in a transcript, one only in typed notes.
    needle={title:"quillfeather",transcript:"zephyrine",typed:"marlowesque"}
    notes[412]!.title=`Quillfeather account review 412`
    notes[77]!.body=`Ask about the Marlowesque clause before signing.`
    const spoken=segments.find(segment=>segment.noteId===noteIds[201]&&segment.startMs===18_000*30)!
    spoken.text=`${spoken.text} I think the Zephyrine integration slips to next quarter.`
    for(let at=0;at<notes.length;at+=100)await db.note.createMany({data:notes.slice(at,at+100)})
    for(let at=0;at<segments.length;at+=2000)await db.transcriptSegment.createMany({data:segments.slice(at,at+2000)})
    await db.note.create({data:{ownerId:bob,organizationId:north,title:"Quillfeather Zephyrine Marlowesque (Bob)",body:"marlowesque",enhanced:"quillfeather",date:dayOf(900),transcriptSegments:{create:[{channel:"mic",speakerId:"self",text:"zephyrine",isFinal:true,startMs:0}]}}})
  },120_000)
  afterAll(async()=>{
    if(db){await db.user.deleteMany({where:{id:{in:[ada,bob]}}});await db.organization.deleteMany({where:{id:{in:[north,south]}}})}
    await api?.dispose();await resources?.close()
  })
  const scoped=(user:string,organization?:string)=>resources!.repository.forUser(user,organization)
  const list=(user:string,query:Parameters<typeof parseListInput>[0]={},organization?:string)=>Effect.runPromise(scoped(user,organization).listNoteSummaries!(parseListInput(query)))
  const search=(user:string,query:Parameters<typeof parseSearchInput>[0],organization?:string)=>Effect.runPromise(scoped(user,organization).searchNoteSummaries!(parseSearchInput(query)))

  test("500 notes: payload size and time, old list against the paged summaries and search",async()=>{
    const median=async(path:string,runs=5)=>{const samples=[];let last=await get(ada,path);for(let i=0;i<runs;i++){last=await get(ada,path);samples.push(last.ms)}samples.sort((a,b)=>a-b);return{...last,ms:samples[Math.floor(samples.length/2)]!}}
    const full=await median("/api/v1/notes",3),first=await median("/api/v1/notes/summaries?limit=50"),largest=await median("/api/v1/notes/summaries?limit=100")
    expect(full.status).toBe(200);expect(first.status).toBe(200)
    expect((full.json() as unknown[]).length).toBe(NOTES)
    let everything=0,pages=0,cursor:string|null=null,walked=0
    const walk=performance.now()
    do{const page=await get(ada,`/api/v1/notes/summaries?limit=50${cursor?`&cursor=${cursor}`:""}`);const body=page.json() as NoteSummariesPageDTO;everything+=page.bytes;pages++;walked+=body.notes.length;cursor=body.nextCursor}while(cursor)
    const walkMs=performance.now()-walk
    expect(walked).toBe(NOTES)
    const queries:Array<[string,string]>=[["title-only word",needle.title],["transcript-only word",needle.transcript],["typed-notes word",needle.typed],["common word (most notes)","roadmap"],["two words","pricing roadmap"],["three words","customer renewal forecast"],["common word then a transcript-only word",`roadmap ${needle.transcript}`],["no match","xylophonic"],["three words, none present","xylophonic quixotry zugzwang"]]
    const rows:Array<[string,number,number,string]>=[
      ["GET /notes (old full list)",full.bytes,full.ms,`${NOTES} notes`],
      ["GET /notes/summaries?limit=50 (first page)",first.bytes,first.ms,"50 rows"],
      ["GET /notes/summaries?limit=100",largest.bytes,largest.ms,"100 rows"],
      [`all ${pages} summary pages, sequential`,everything,walkMs,`${NOTES} rows`],
    ]
    for(const [label,q]of queries){const result=await median(`/api/v1/notes/search?q=${encodeURIComponent(q)}`);expect(result.status).toBe(200);const body=result.json() as NoteSearchPageDTO;rows.push([`search: ${label} “${q}”`,result.bytes,result.ms,`${body.total} matches, ${body.results.length} returned`])}
    console.log([`NOTE LIST BENCHMARK: ${NOTES} notes, ${SEGMENTS} transcript turns in 6 of every 7, ${(transcriptCharacters/1e6).toFixed(1)} MB of transcript text`,
      ...rows.map(([label,bytes,ms,detail])=>`${label.padEnd(52)} ${String(bytes).padStart(10)} bytes ${ms.toFixed(1).padStart(8)} ms  ${detail}`),
      `first page is ${(full.bytes/first.bytes).toFixed(0)}x smaller than the old list; every row of the account is ${(full.bytes/everything).toFixed(0)}x smaller`].join("\n"))
    expect(first.bytes).toBeLessThan(full.bytes/50)
    expect(everything).toBeLessThan(full.bytes/10)
    expect(first.bytes/50).toBeLessThan(1200)
  },240_000)

  test("rows match the in-memory reference: order, flags, counts and excerpts",async()=>{
    const notes=await Effect.runPromise(scoped(ada).listNotes())
    for(const query of[{},{filter:"summarized" as const},{filter:"transcribed" as const},{limit:"37"}]){
      const expected=selectNoteSummaries(notes,parseListInput(query)),actual=await list(ada,query)
      expect(actual.total).toBe(expected.total)
      expect(actual.notes.map(({organizationId,...note})=>note)).toEqual(expected.notes.map(({organizationId,...note})=>note))
    }
    const first=(await list(ada,{limit:"1"})).notes[0]!
    expect(first).toMatchObject({hasTranscript:true,speakerCount:6,organizationId:north})
    expect(first.excerpt.length).toBeGreaterThan(40)
  },60_000)

  test("search agrees with the in-memory reference across pages, and ranks sanely",async()=>{
    const notes=await Effect.runPromise(scoped(ada).listNotes())
    const shape=(page:NoteSearchPageDTO)=>({total:page.total,terms:page.terms,results:page.results.map(result=>({id:result.note.id,matchedIn:result.matchedIn,title:result.title,excerpt:result.excerpt}))})
    for(const q of[needle.title,needle.transcript,needle.typed,"roadmap","pricing roadmap","customer renewal forecast","decisions","Speaker","xylophonic","100% _"]){
      for(const filter of["all","summarized"] as const){
        const collected:Array<NoteSearchPageDTO["results"][number]>=[]
        let cursor:string|undefined,expectedCursor:string|undefined,rounds=0
        do{
          const actual=await search(ada,{q,filter,limit:"40",...(cursor?{cursor}:{})}),expected=selectNoteSearch(notes,parseSearchInput({q,filter,limit:"40",...(expectedCursor?{cursor:expectedCursor}:{})}))
          expect(shape(actual)).toEqual(shape(expected))
          expect(actual.nextCursor).toBe(expected.nextCursor)
          collected.push(...actual.results)
          cursor=actual.nextCursor??undefined;expectedCursor=expected.nextCursor??undefined
        }while(cursor&&++rounds<3)
        expect(new Set(collected.map(result=>result.note.id)).size).toBe(collected.length)
      }
    }
    expect((await search(ada,{q:needle.title})).results.map(result=>result.note.id)).toEqual([noteIds[412]!])
    const heard=(await search(ada,{q:needle.transcript})).results
    expect(heard.map(result=>result.note.id)).toEqual([noteIds[201]!])
    expect(heard[0]).toMatchObject({matchedIn:["transcript"],excerpt:{source:"transcript",startMs:18_000*30}})
    expect(heard[0]!.excerpt!.parts.some(part=>part.hit&&part.text==="Zephyrine")).toBe(true)
    expect((await search(ada,{q:needle.typed})).results[0]).toMatchObject({note:{id:noteIds[77]!},matchedIn:["notes"],excerpt:{source:"notes"}})
    // A word in the title outranks the same word only spoken; more fields outrank fewer.
    const ranked=(await search(ada,{q:"roadmap",limit:"50"})).results
    const rank=(result:NoteSearchPageDTO["results"][number])=>result.matchedIn.reduce((sum,field)=>sum+({title:4,summary:2,notes:1,transcript:1,people:2})[field],0)
    expect(ranked.map(rank)).toEqual([...ranked.map(rank)].sort((a,b)=>b-a))
    expect(ranked[0]!.matchedIn).toContain("title")
    for(let i=1;i<ranked.length;i++)if(rank(ranked[i-1]!)===rank(ranked[i]!))expect(ranked[i-1]!.note.date>=ranked[i]!.note.date).toBe(true)
  },120_000)

  test("an account and a workspace are hard boundaries for the list and for search",async()=>{
    const bobs=await list(bob)
    expect(bobs.total).toBe(1)
    expect(bobs.notes[0]!.title).toContain("(Bob)")
    for(const q of[needle.title,needle.transcript,needle.typed]){
      const mine=await search(ada,{q}),theirs=await search(bob,{q})
      expect(mine.total).toBe(1)
      expect(mine.results.every(result=>noteIds.includes(result.note.id))).toBe(true)
      expect(theirs.total).toBe(1)
      expect(theirs.results[0]!.note.title).toContain("(Bob)")
    }
    expect((await search(bob,{q:"roadmap"})).total).toBe(0)
    // A cursor from Ada's list is only a position in Bob's.
    const adas=await list(ada,{limit:"1"})
    expect((await list(bob,{cursor:adas.nextCursor!})).notes.every(note=>note.title.includes("(Bob)"))).toBe(true)
    // Workspace scope, as the MCP and workspace-chat repositories use it.
    const inNorth=await list(ada,{limit:"100"},north),inSouth=await list(ada,{limit:"100"},south)
    expect(inNorth.total+inSouth.total).toBe(NOTES)
    expect(inNorth.notes.every(note=>note.organizationId===north)).toBe(true)
    expect(inSouth.notes.every(note=>note.organizationId===south)).toBe(true)
    expect((await search(ada,{q:needle.title},north)).total).toBe(0)
    expect((await search(ada,{q:needle.title},south)).results.map(result=>result.note.id)).toEqual([noteIds[412]!])
    expect((await list(ada,{},crypto.randomUUID())).total).toBe(0)
    // Over HTTP too, with the same session resolution the routes use.
    const page=(await get(bob,"/api/v1/notes/summaries")).json() as NoteSummariesPageDTO
    expect(page.notes.map(note=>note.title)).toEqual(["Quillfeather Zephyrine Marlowesque (Bob)"])
    expect(((await get(bob,"/api/v1/notes/search?q=quillfeather")).json() as NoteSearchPageDTO).results.map(result=>result.note.id)).toEqual([page.notes[0]!.id])
    expect((await get(bob,"/api/v1/notes/summaries?cursor=garbage")).status).toBe(400)
  },60_000)

  test("paging while notes are written has no duplicates and no omissions",async()=>{
    const carol=crypto.randomUUID()
    await db!.user.create({data:{id:carol,name:"Carol",email:`list-${carol}@example.invalid`}})
    try{
      const repository=scoped(carol),original:string[]=[]
      for(let i=0;i<40;i++)original.push((await Effect.runPromise(repository.createNote({title:`Carol ${i}`,date:dayOf(i*3%33).toISOString().slice(0,10)}))).id)
      const seen:string[]=[],older:string[]=[]
      let cursor:string|undefined,round=0
      do{
        // Writers run while the page is being read, not only between pages.
        const [page,newer,past]=await Promise.all([list(carol,{limit:"7",...(cursor?{cursor}:{})}),
          Effect.runPromise(repository.createNote({title:`Newer ${round}`,date:"2027-06-01"})),Effect.runPromise(repository.createNote({title:`Older ${round}`,date:new Date(Date.UTC(2020,0,1)-round*86_400_000).toISOString().slice(0,10)}))])
        await Effect.runPromise(repository.updateNote(original[round%original.length]!,{body:`edited in round ${round}`}))
        seen.push(...page.notes.map(note=>note.id));cursor=page.nextCursor??undefined;round++
        if(cursor)older.push(past.id)
        expect(newer.id).toBeTruthy()
      }while(cursor)
      expect(new Set(seen).size).toBe(seen.length)
      for(const id of original)expect(seen).toContain(id)
      // Each of these is older than everything before it, so it is always ahead of the reader and must be picked up.
      for(const id of older)expect(seen).toContain(id)
      const all=(await list(carol,{limit:"100"})).notes.map(note=>note.id)
      expect(seen).toEqual(all.filter(id=>seen.includes(id)))
    }finally{await db!.user.deleteMany({where:{id:carol}})}
  },60_000)

  test("paging is exact when the database session is not in UTC",async()=>{
    // Prisma writes and reads instants without a zone, so a cursor has to be bound the same way.
    // On a UTC server a zoned cursor happens to agree; in any other session it loses a row at each page boundary.
    const zoned=makePrismaRepository(`${url}${url!.includes("?")?"&":"?"}options=${encodeURIComponent("-c timezone=America/New_York")}`),dana=crypto.randomUUID()
    await db!.user.create({data:{id:dana,name:"Dana",email:`list-${dana}@example.invalid`}})
    try{
      expect((await zoned.database.$queryRawUnsafe<Array<{TimeZone:string}>>("SHOW timezone"))[0]!.TimeZone).toBe("America/New_York")
      const writer=zoned.repository.forUser(dana),ids:string[]=[]
      // Several notes a day, so every page boundary falls inside a day and is decided by the instant.
      for(let i=0;i<23;i++)ids.push((await Effect.runPromise(writer.createNote({title:`Roadmap ${i}`,date:`2026-05-0${1+i%2}`}))).id)
      for(const repository of[writer,scoped(dana)]){
        const seen:string[]=[],found:string[]=[]
        let cursor:string|undefined,searchCursor:string|undefined
        do{const page=await Effect.runPromise(repository.listNoteSummaries!(parseListInput({limit:"4",...(cursor?{cursor}:{})})));seen.push(...page.notes.map(note=>note.id));cursor=page.nextCursor??undefined}while(cursor)
        do{const page=await Effect.runPromise(repository.searchNoteSummaries!(parseSearchInput({q:"roadmap",limit:"4",...(searchCursor?{cursor:searchCursor}:{})})));found.push(...page.results.map(result=>result.note.id));searchCursor=page.nextCursor??undefined}while(searchCursor)
        expect(seen.length).toBe(ids.length)
        expect(new Set(seen)).toEqual(new Set(ids))
        expect(found).toEqual(seen)
      }
    }finally{await db!.user.deleteMany({where:{id:dana}});await zoned.close()}
  },60_000)

  test("attendees and meeting time come from saved context, scoped to the owner",async()=>{
    const [withPeople,broken,plain]=[noteIds[NOTES-1]!,noteIds[NOTES-2]!,noteIds[NOTES-3]!]
    const context=meeting([person("Me",`list-${ada}@example.invalid`,{isSelf:true}),person("Grace Hopper","grace@navy.example",{givenName:"Grace"}),person(null,"alan@bletchley.example"),person("Boardroom",null,{isResource:true}),
      ...Array.from({length:5},(_,i)=>person(`Guest ${i+1}`,null))])
    await db!.noteMeetingContext.create({data:{noteId:withPeople,userId:ada,meeting:JSON.parse(JSON.stringify(context))}})
    await db!.noteMeetingContext.create({data:{noteId:broken,userId:ada,meeting:{participants:"not a list"}}})
    // A context row that names another user is not this owner's context.
    await db!.noteMeetingContext.create({data:{noteId:plain,userId:bob,meeting:JSON.parse(JSON.stringify(context))}})
    const rows=new Map((await list(ada,{limit:"3"})).notes.map(note=>[note.id,note]))
    expect(rows.get(withPeople)).toMatchObject({meeting:{startAt:"2026-03-02T15:00:00.000Z",endAt:"2026-03-02T15:30:00.000Z",allDay:false},
      attendees:{total:8,others:7,people:[{name:"Grace Hopper",shortName:"Grace",initials:"GH"},{name:"alan@bletchley.example",shortName:"alan",initials:"AE"},{name:"Guest 1",shortName:"Guest",initials:"G1"},{name:"Guest 2",shortName:"Guest",initials:"G2"}]}})
    expect(rows.get(broken)).toMatchObject({meeting:null,attendees:{people:[],others:0,total:0}})
    expect(rows.get(plain)).toMatchObject({meeting:null,attendees:{total:0}})
    const byName=await search(ada,{q:"hopper"})
    expect(byName.results.map(result=>result.note.id)).toEqual([withPeople])
    expect(byName.results[0]).toMatchObject({matchedIn:["people"],excerpt:{source:"people",parts:[{text:"Grace ",hit:false},{text:"Hopper",hit:true}]}})
    expect((await search(ada,{q:"bletchley.example"})).total).toBe(1)
    expect((await search(ada,{q:"boardroom"})).total).toBe(0)
    expect((await search(bob,{q:"hopper"})).total).toBe(0)
  },60_000)
})
