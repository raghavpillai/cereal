import {describe,expect,test} from "bun:test"
import {Effect} from "effect"
import type {UpdateNoteInput} from "@meeting-notes/contracts"
import {makeMemoryRepository} from "../src/repository"
import {makePrismaRepository} from "../src/prisma-repository"
import {makeProcessingService} from "../src/processing"
import {makeProviderServices,serializeGenerationContext,serializeProviderContext} from "../src/providers"

const segment={id:crypto.randomUUID(),channel:"mic" as const,speakerId:"fixture",speakerLabel:"Mira",text:"Mira will ship the prototype on Friday.",isFinal:true,startMs:0,endMs:1000}
const input={title:"Prototype review",body:"# Risks\n- Check launch confidence\n- My observation: Mira seemed hesitant",enhanced:"OLD_UNSUPPORTED_SUMMARY_CLAIM",transcriptSegments:[segment]}
const mutations:UpdateNoteInput[]=[{body:"# Risks\n- New private observation"},{enhanced:"User's edited generated document"},{title:"Renamed review"},{date:"2026-11-13"},{duration:"2:30"},{transcriptSegments:[{...segment,text:"Corrected transcript"}]}]

describe("generation snapshot protects both original and enhanced notes",()=>{
 for(const kind of ["memory","postgres"] as const){
  const enabled=kind==="memory"||process.env.RUN_PG_TESTS==="1"&&!!process.env.DATABASE_URL
  test.skipIf(!enabled)(`${kind}: late completion cannot replace any changed source or document`,async()=>{
   const resources=kind==="postgres"?makePrismaRepository(process.env.DATABASE_URL!):null
   const repository=resources?.repository??makeMemoryRepository(),ids:string[]=[]
   try{
    for(const patch of mutations){
     const note=await Effect.runPromise(repository.createNote({...input,date:"2026-01-01",transcriptSegments:[{...segment,id:crypto.randomUUID()}]}));ids.push(note.id)
     const run=crypto.randomUUID()
     expect(await Effect.runPromise(repository.beginGeneration(note.id,note.transcriptRevision,run))).not.toBeNull()
     const update=patch.transcriptSegments?{...patch,transcriptSegments:patch.transcriptSegments.map(s=>({...s,id:note.transcriptSegments[0]!.id}))}:patch
     const edited=await Effect.runPromise(repository.updateNote(note.id,update))
     expect(edited!.generationState).toBe("idle")
     expect(await Effect.runPromise(repository.finishGeneration(note.id,note.transcriptRevision,run,"STALE_PROVIDER_OUTPUT"))).toBeNull()
     const saved=await Effect.runPromise(repository.getNote(note.id))
     expect(saved!.body).toBe(patch.body??input.body)
     expect(saved!.enhanced).toBe(patch.enhanced??input.enhanced)
    }
    const note=await Effect.runPromise(repository.createNote(input));ids.push(note.id);const run=crypto.randomUUID()
    await Effect.runPromise(repository.beginGeneration(note.id,note.transcriptRevision,run))
    await Effect.runPromise(repository.updateNote(note.id,{body:note.body,enhanced:note.enhanced,title:note.title}))
    expect((await Effect.runPromise(repository.finishGeneration(note.id,note.transcriptRevision,run,"Fresh output")))!.enhanced).toBe("Fresh output")
   }finally{for(const id of ids)await Effect.runPromise(repository.deleteNote(id));await resources?.close()}
  })
 }
})

test("enhancement source contains original notes and transcript, while chat can also read the current enhanced document",async()=>{
 const repo=makeMemoryRepository(),note=await Effect.runPromise(repo.createNote(input))
 const generation=JSON.parse(serializeGenerationContext(note)),chat=JSON.parse(serializeProviderContext(note))
 expect(generation.userNotes).toBe(input.body)
 expect(generation.transcript[0].text).toBe(segment.text)
 expect(generation).not.toHaveProperty("enhanced")
 expect(JSON.stringify(generation)).not.toContain(input.enhanced)
 expect(chat.enhanced).toBe(input.enhanced)
 let requestBody:any
 const providers=makeProviderServices({openRouterKey:"fixture-not-a-real-key",fetch:(async(_input,init)=>{
  requestBody=JSON.parse(init!.body as string)
  return Response.json({choices:[{message:{content:"# Risks\n- Mira will ship Friday."}}]})
 }) as typeof fetch})
 await Effect.runPromise(providers.generate(note))
 expect(JSON.parse(requestBody.messages[1].content)).toEqual(generation)
 // On GPT-5.6 and later the instructions are sent as text parts, the first with a prompt-cache breakpoint.
 expect(requestBody.messages[0].content.map((part:{text:string})=>part.text).join("")).toContain("author's original notes")
 expect((await Effect.runPromise(repo.getNote(note.id)))!.body).toBe(input.body)
})

test("regeneration uses newly edited original notes without replacing them or the transcript",async()=>{
 const repo=makeMemoryRepository(),note=await Effect.runPromise(repo.createNote(input)),seen:string[]=[]
 const providers={generate:(source:typeof note)=>Effect.sync(()=>{seen.push(source.body);return "# Current risks\n- Supported transcript detail."}),chat:()=>Effect.succeed("")}
 const processing=makeProcessingService(repo,{providers})
 await Effect.runPromise(processing.generate(note.id,note.transcriptRevision))
 const revised="# My decision\n- Hold launch until reviewed"
 await Effect.runPromise(repo.updateNote(note.id,{body:revised}))
 await Effect.runPromise(processing.generate(note.id,note.transcriptRevision))
 const saved=await Effect.runPromise(repo.getNote(note.id))
 expect(seen).toEqual([input.body,revised])
 expect(saved!.body).toBe(revised)
 expect(saved!.transcriptSegments).toEqual([segment])
 expect(saved!.enhanced).toBe("# Current risks\n- Supported transcript detail.")
 expect(saved!.transcriptRevision).toBe(note.transcriptRevision)
})
