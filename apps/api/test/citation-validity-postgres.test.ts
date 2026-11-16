/** The citation rule of `src/ai/citations.ts` against the real scoped Prisma
 * repository: a citation survives transcript being appended to its note, stops
 * resolving when its own passage is edited or its note is deleted, and never
 * resolves for another account or another workspace. Disposable rows only. */
import {afterAll,beforeAll,describe,expect,test} from "bun:test"
import {Effect} from "effect"
import {makePrismaRepository} from "../src/prisma-repository"
import {checkChatCitations,prepareMeetingChat,validateChatCitations} from "../src/chat-retrieval"
import type {ProviderServices} from "../src/providers"

const enabled=process.env.RUN_PG_TESTS==="1"&&!!process.env.DATABASE_URL&&/^\/mn_test_[a-f0-9]+$/.test(new URL(process.env.DATABASE_URL).pathname)
describe.skipIf(!enabled)("citation validity through the scoped Prisma repository",()=>{
  const resources=enabled?makePrismaRepository(process.env.DATABASE_URL!):null,db=resources?.database
  const alice=crypto.randomUUID(),bob=crypto.randomUUID(),one=crypto.randomUUID(),two=crypto.randomUUID()
  const inOne=resources?.repository.forUser(alice,one),inTwo=resources?.repository.forUser(alice,two),bobInOne=resources?.repository.forUser(bob,one)
  const silent:ProviderServices={generate:()=>Effect.succeed(""),chat:()=>Effect.succeed("")}
  const signal=()=>new AbortController().signal
  const segment=(text:string,startMs:number)=>({id:crypto.randomUUID(),channel:"mic" as const,speakerId:"mic:fixture:speaker_1",speakerLabel:"Speaker 1",text,isFinal:true,startMs,endMs:startMs+4000})
  const spoken=[segment("The berth fee is three hundred a night.",0),segment("We booked the crane for Tuesday.",6000),segment("Ines will confirm the harbour pilot by Friday.",12000)]
  const make=async()=>(await db!.note.create({data:{ownerId:alice,organizationId:one,title:"Harbour planning",body:"- berth fee is 300\n- ask about the crane",enhanced:"# Decisions\n- The crane is booked for Tuesday.",date:new Date("2026-03-09T00:00:00Z"),recordingState:"recording",transcriptRevision:1,transcriptSegments:{create:spoken}}})).id
  const issued=async(noteId:string)=>(await prepareMeetingChat({noteId,input:{message:"What was decided?",history:[],includeSources:true},repository:inOne!,providers:silent,enrich:async note=>note,signal:signal(),emit:()=>{}})).citations
  beforeAll(async()=>{
    for(const [id,name]of[[alice,"Alice"],[bob,"Bob"]])await db!.user.create({data:{id:id!,name:name!,email:`citation-${id}@example.invalid`}})
    await db!.organization.createMany({data:[{id:one,name:"One",slug:`one-${one}`},{id:two,name:"Two",slug:`two-${two}`}]})
    await db!.member.createMany({data:[{id:crypto.randomUUID(),userId:alice,organizationId:one,role:"member"},{id:crypto.randomUUID(),userId:alice,organizationId:two,role:"member"},{id:crypto.randomUUID(),userId:bob,organizationId:one,role:"member"}]})
  })
  afterAll(async()=>{await db?.user.deleteMany({where:{id:{in:[alice,bob]}}});await db?.organization.deleteMany({where:{id:{in:[one,two]}}});await resources?.close()})

  test("citations survive appended transcript, lose only an edited passage, and are withdrawn with the note",async()=>{
    const noteId=await make(),citations=await issued(noteId)
    expect(citations.map(citation=>citation.source)).toEqual(["transcript","transcript","transcript","notes","notes","summary"])
    expect(await validateChatCitations(citations,inOne!,signal())).toEqual([...citations])
    // Two more pieces of speech arrive.
    await Effect.runPromise(inOne!.updateNote(noteId,{transcriptSegments:[...spoken,segment("Good, that settles the crane.",70000),segment("Anything else before we close?",125000)]}))
    const grown=(await Effect.runPromise(inOne!.getNote(noteId)))!
    expect(grown.transcriptRevision).toBe(2)
    expect(await validateChatCitations(citations,inOne!,signal())).toEqual([...citations])
    for(const citation of citations.filter(item=>item.source==="transcript")){
      const stored=grown.transcriptSegments.find(item=>item.id===citation.segmentId)!
      expect(stored.startMs).toBe(citation.startMs!);expect(stored.text).toContain(citation.quote)
    }
    // One cited segment is rewritten, and an uncited typed line is edited.
    await Effect.runPromise(inOne!.updateNote(noteId,{body:"- berth fee is 300\n- crane: ask Ines",transcriptSegments:grown.transcriptSegments.map(item=>item.id===spoken[1]!.id?{...item,text:"We moved the crane to Wednesday."}:item)}))
    const kept=await validateChatCitations(citations,inOne!,signal())
    expect(kept.map(citation=>citation.id)).toEqual(["S1","S3","S4","S6"])
    expect(await Effect.runPromise(inOne!.deleteNote(noteId))).toBe(true)
    expect(await checkChatCitations(citations,inOne!,signal())).toEqual({valid:[],withdrawn:true})
  })

  test("the same citations never resolve for another account or from another workspace, before or after an append",async()=>{
    const noteId=await make(),citations=await issued(noteId)
    for(const round of [0,1]){
      if(round)await Effect.runPromise(inOne!.updateNote(noteId,{transcriptSegments:[...spoken,segment("Good, that settles the crane.",70000)]}))
      expect(await validateChatCitations(citations,inOne!,signal())).toEqual([...citations])
      expect(await checkChatCitations(citations,inTwo!,signal())).toEqual({valid:[],withdrawn:true})
      expect(await checkChatCitations(citations,bobInOne!,signal())).toEqual({valid:[],withdrawn:true})
    }
    // Moving the note to the other workspace withdraws it from the first.
    await db!.note.update({where:{id:noteId},data:{organizationId:two}})
    expect(await checkChatCitations(citations,inOne!,signal())).toEqual({valid:[],withdrawn:true})
    expect(await validateChatCitations(citations,inTwo!,signal())).toEqual([...citations])
  })
})
