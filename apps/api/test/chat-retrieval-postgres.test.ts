import {afterAll,beforeAll,describe,expect,test} from "bun:test"
import {Effect} from "effect"
import {makePrismaRepository} from "../src/prisma-repository"
import {prepareMeetingChat,validateChatCitations} from "../src/chat-retrieval"
import type {ProviderServices} from "../src/providers"

const enabled=process.env.RUN_PG_TESTS==="1"&&!!process.env.DATABASE_URL&&/^\/mn_test_[a-f0-9]+$/.test(new URL(process.env.DATABASE_URL).pathname)
describe.skipIf(!enabled)("bounded owner-scoped Prisma meeting retrieval",()=>{
  const resources=enabled?makePrismaRepository(process.env.DATABASE_URL!):null,db=resources?.database
  const alice=crypto.randomUUID(),bob=crypto.randomUUID(),a=resources?.repository.forUser(alice),b=resources?.repository.forUser(bob)
  let privateId="",transcriptId="",segmentId=""
  beforeAll(async()=>{
    for(const [id,name]of[[alice,"Alice"],[bob,"Bob"]])await db!.user.create({data:{id:id!,name:name!,email:`retrieval-${id}@example.invalid`}})
    for(let index=1;index<=25;index++){
      const segment={id:crypto.randomUUID(),speakerId:"mic:fixture:voice1",speakerLabel:null,channel:"mic" as const,text:"Orion launches Friday after review",startMs:1000,endMs:3000,isFinal:true}
      const note=await Effect.runPromise(a!.createNote({title:"Cedar "+index,date:`2026-11-${String(index).padStart(2,"0")}`,enhanced:"# Next Steps\n- Send the checklist",...(index===20?{transcriptSegments:[segment]}:{})}))
      if(index===20){transcriptId=note.id;segmentId=segment.id}
    }
    privateId=(await Effect.runPromise(b!.createNote({title:"Cedar private Bob",body:"FOREIGN_POSTGRES_SENTINEL",date:"2026-11-26",transcriptSegments:[{id:crypto.randomUUID(),speakerId:"private-speaker",speakerLabel:null,channel:"system",text:"Orion FOREIGN_POSTGRES_SENTINEL",startMs:0,endMs:1000,isFinal:true}]}))).id
  })
  afterAll(async()=>{await db?.user.deleteMany({where:{id:{in:[alice,bob]}}});await resources?.close()})
  test("search applies owner/date/take in PostgreSQL and returns metadata without loading transcripts",async()=>{
    const result=await Effect.runPromise(a!.searchNotes!({query:"Cedar",limit:5}))
    expect(result).toHaveLength(5)
    expect(result[0]!.date).toBe("2026-11-25")
    expect(result.some(n=>n.id===privateId)).toBe(false)
    const ranged=await Effect.runPromise(a!.searchNotes!({query:"Cedar",from:"2026-11-20",to:"2026-11-21",limit:20}))
    expect(ranged.map(n=>n.date)).toEqual(["2026-11-21","2026-11-20"])
    const transcript=await Effect.runPromise(a!.searchNotes!({query:"Orion",limit:20}))
    expect(transcript.map(n=>n.id)).toEqual([transcriptId])
    expect(transcript[0]!.transcriptSegments).toEqual([])
    expect((await Effect.runPromise(a!.getNote(transcriptId)))!.transcriptSegments[0]!.id).toBe(segmentId)
    expect((await Effect.runPromise(a!.searchNotes!({query:"recent to-dos",limit:200})))).toHaveLength(20)
  })
  test("forged model read IDs cannot escape scope or supply citation metadata",async()=>{
    let round=0
    const providers:ProviderServices={generate:()=>Effect.succeed(""),chat:()=>Effect.succeed(""),async chatToolStep(){return round++===0?[{id:"forged",name:"get_meetings",arguments:JSON.stringify({noteIds:[privateId]})}]:[]}}
    const prepared=await prepareMeetingChat({noteId:null,input:{message:"Cedar to-dos",history:[],includeSources:true},repository:a!,providers,enrich:async note=>note,signal:new AbortController().signal,emit:()=>{}})
    expect(JSON.stringify(prepared)).not.toContain("FOREIGN_POSTGRES_SENTINEL")
    expect(prepared.citations.some(c=>c.noteId===privateId)).toBe(false)
    expect(await validateChatCitations([{id:"S1",noteId:privateId,title:"Forged",source:"notes",segmentId:null,startMs:null,quote:"FOREIGN_POSTGRES_SENTINEL",transcriptRevision:0}],a!,new AbortController().signal)).toEqual([])
  })
  test("direct transcript search bounds matching passages in PostgreSQL with owner/date/exact-identity filters",async()=>{
    const segments=Array.from({length:25},(_,index)=>({id:crypto.randomUUID(),speakerId:"mic:many:voice1",speakerLabel:null,channel:"mic" as const,text:`Orion passage ${index} confirms the deadline.`,startMs:index*1000,endMs:index*1000+999,isFinal:true}))
    const note=await Effect.runPromise(a!.createNote({title:"Bounded Orion passages",date:"2026-11-27",transcriptSegments:segments}))
    const hits=await Effect.runPromise(a!.searchTranscriptSegments!({query:"Orion",limit:5}))
    expect(hits).toHaveLength(5)
    expect(hits.every(hit=>hit.noteId===note.id)).toBe(true)
    expect(hits.map(hit=>hit.segment.id)).toEqual(segments.slice(0,5).map(segment=>segment.id))
    expect(hits.every(hit=>segments.some(segment=>segment.id===hit.segment.id&&segment.startMs===hit.segment.startMs&&segment.text.includes(hit.segment.text)))).toBe(true)
    const dated=await Effect.runPromise(a!.searchTranscriptSegments!({query:"Orion",from:"2026-11-20",to:"2026-11-20",limit:20}))
    expect(dated.map(hit=>hit.noteId)).toEqual([transcriptId])
    expect(dated[0]!.segment.id).toBe(segmentId)
    expect(await Effect.runPromise(a!.searchTranscriptSegments!({query:"Orion",noteId:privateId,limit:20}))).toEqual([])
    expect(await Effect.runPromise(a!.searchTranscriptSegments!({query:"",limit:20}))).toEqual([])
    expect((await Effect.runPromise(a!.searchTranscriptSegments!({query:"Orion",limit:200})))).toHaveLength(20)
  })
})
