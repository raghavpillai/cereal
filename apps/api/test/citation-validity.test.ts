/** The rule that decides whether a chat citation still resolves
 * (`src/ai/citations.ts`), and how a streamed answer ends when one does not
 * (`processing.chatStream`). Each citation is judged on its own passage: speech
 * appended during a meeting and edits elsewhere in the note change nothing;
 * a passage that was edited away or deleted stops resolving; a note the caller
 * cannot read never resolves. Invented meetings only. No network. */
import {expect,test} from "bun:test"
import {Effect,Schema,Stream} from "effect"
import {ChatStreamEventDTO,type ChatCitationDTO,type ChatInput,type NoteDTO,type TranscriptSegmentDTO} from "@meeting-notes/contracts"
import {makeMemoryRepository,type NotesRepositoryService} from "../src/repository"
import {makeProcessingService} from "../src/processing"
import type {ProviderNote,ProviderServices} from "../src/providers"
import {CitationStreamFilter,checkChatCitations,prepareMeetingChat,validateChatCitations} from "../src/chat-retrieval"
import {MAX_ANSWER_CITATIONS} from "../src/ai/citations"

const id=(n:number)=>`00000000-0000-4000-8000-${String(n).padStart(12,"0")}`
const seg=(n:number,text:string,startMs:number,isFinal=true):TranscriptSegmentDTO=>({id:id(1000+n),channel:n%2?"mic":"system",speakerId:`mic:${id(900)}:speaker_1`,speakerLabel:"Speaker 1",text,isFinal,startMs,endMs:startMs+4000})
const spoken=[seg(1,"The berth fee is three hundred a night.",0),seg(2,"We booked the crane for Tuesday.",6000),seg(3,"Ines will confirm the harbour pilot by Friday.",12000)]
const later=[seg(4,"Good, that settles the crane.",70000),seg(5,"Anything else before we close?",125000),seg(6,"No, that is everything.",131000)]
const harbour=(fields:Partial<NoteDTO>={}):NoteDTO=>({id:id(1),title:"Harbour planning",body:"- berth fee is 300\n- ask about the crane",enhanced:"# Decisions\n- The crane is booked for Tuesday.",date:"2026-03-09",duration:"",createdAt:"2026-03-09T10:00:00.000Z",updatedAt:"2026-03-09T10:20:00.000Z",
  transcriptSegments:spoken,recordingState:"recording",generationState:"ready",transcriptRevision:3,generatedRevision:3,...fields})
const ask=(message:string):ChatInput=>({message,history:[],includeSources:true})
const silent:ProviderServices={generate:()=>Effect.succeed(""),chat:()=>Effect.succeed("")}
const signal=()=>new AbortController().signal
const run=<A>(effect:Effect.Effect<A,unknown>)=>Effect.runPromise(effect)
/** The citations the server issues for a note, as the streaming path prepares them. */
const issued=async(repository:NotesRepositoryService,noteId:string)=>(await prepareMeetingChat({noteId,input:ask("What was decided?"),repository,providers:silent,enrich:async source=>source,signal:signal(),emit:()=>{}})).citations
const citing=(citations:ReadonlyArray<ChatCitationDTO>,source:ChatCitationDTO["source"],text:string)=>{
  const found=citations.find(citation=>citation.source===source&&citation.quote.includes(text))
  if(!found)throw new Error(`no ${source} citation quoting "${text}"`)
  return found
}
/** One streamed answer through `processing.chatStream`. `answer` writes the text from the
 * issued citations; `during` runs after the text and before the server re-checks them. */
async function stream(repository:NotesRepositoryService,noteId:string,answer:(citations:ReadonlyArray<ChatCitationDTO>)=>string,during:()=>Promise<unknown>=async()=>{}) {
  const providers:ProviderServices={...silent,async *chatStream(note:ProviderNote){
    yield answer((note as ProviderNote&{meetingChat?:{citations:ChatCitationDTO[]}}).meetingChat!.citations)
    await during()
  }}
  const processing=makeProcessingService(repository,{providers}),events:ChatStreamEventDTO[]=[]
  await Effect.runPromise(Stream.runForEach(await Effect.runPromise(processing.chatStream(noteId,ask("What was decided?"))),event=>Effect.sync(()=>{events.push(event)})))
  await processing.dispose()
  // Every event must fit the wire contract the HTTP layer encodes with.
  for(const event of events)Schema.decodeUnknownSync(ChatStreamEventDTO)(event)
  return {events,text:events.filter(event=>event.type==="delta").map(event=>event.text).join(""),citations:events.find(event=>event.type==="citations")?.citations??[],last:events.at(-1)!}
}
const mark=(citations:ReadonlyArray<ChatCitationDTO>,...picked:Array<[ChatCitationDTO["source"],string]>)=>picked.map(([source,text])=>`[[${citing(citations,source,text).id}]]`)

test("transcript appended while an answer streams does not end it, and every citation keeps its passage afterwards",async()=>{
  const repository=makeMemoryRepository([harbour()])
  const result=await stream(repository,id(1),citations=>{
    const [pilot,fee,crane]=mark(citations,["transcript","harbour pilot"],["notes","berth fee"],["summary","crane is booked"])
    return `Ines confirms the pilot by Friday ${pilot}. You noted the fee ${fee}; the notes record the crane ${crane}.`
  },()=>run(repository.updateNote(id(1),{transcriptSegments:[...spoken,later[0]!]})))
  expect((await run(repository.getNote(id(1))))!.transcriptRevision).toBe(4)
  expect(result.events.map(event=>event.type)).toEqual(["tool","tool","delta","citations","done"])
  expect(result.citations.map(citation=>citation.source)).toEqual(["transcript","notes","summary"])
  expect(result.citations[0]).toMatchObject({segmentId:spoken[2]!.id,startMs:12000,quote:spoken[2]!.text})
  // The meeting goes on: two more pieces of speech arrive after the answer.
  await run(repository.updateNote(id(1),{transcriptSegments:[...spoken,...later]}))
  const now=(await run(repository.getNote(id(1))))!
  expect(now.transcriptRevision).toBeGreaterThan(4)
  expect(await validateChatCitations(result.citations,repository,signal())).toEqual([...result.citations])
  const segment=now.transcriptSegments.find(item=>item.id===result.citations[0]!.segmentId)!
  expect(segment.startMs).toBe(result.citations[0]!.startMs!)
  expect(segment.text).toContain(result.citations[0]!.quote)
})

test("a citation whose segment text was edited so the quote is gone is dropped; the others in the same answer survive and the answer completes",async()=>{
  const repository=makeMemoryRepository([harbour()])
  const result=await stream(repository,id(1),citations=>{
    const [fee,crane,pilot]=mark(citations,["transcript","three hundred"],["transcript","booked the crane"],["transcript","harbour pilot"])
    return `Fee ${fee}. Crane ${crane}. Pilot ${pilot}.`
  },()=>run(repository.updateNote(id(1),{transcriptSegments:[spoken[0]!,{...spoken[1]!,text:"We moved the crane to Wednesday."},spoken[2]!]})))
  expect(result.citations.map(citation=>citation.segmentId)).toEqual([spoken[0]!.id,spoken[2]!.id])
  expect(result.last.type).toBe("done")
  expect(result.events.some(event=>event.type==="error")).toBe(false)
})

test("a transcript citation needs its own segment: present, final, at the cited start time, still containing the quote",async()=>{
  const repository=makeMemoryRepository([harbour()]),citations=await issued(repository,id(1)),pilot=citing(citations,"transcript","harbour pilot"),fee=citing(citations,"transcript","three hundred")
  const after=async(transcriptSegments:TranscriptSegmentDTO[])=>{await run(repository.updateNote(id(1),{transcriptSegments}));return validateChatCitations([fee,pilot],repository,signal())}
  expect(await after([...spoken,...later])).toEqual([fee,pilot])
  // Text added around the quote leaves it in place.
  expect(await after([spoken[0]!,spoken[1]!,{...spoken[2]!,text:`Right. ${spoken[2]!.text} Then we are done.`}])).toEqual([fee,pilot])
  expect(await after([spoken[0]!,spoken[1]!,{...spoken[2]!,text:"Ines will confirm the tug by Monday."}])).toEqual([fee])
  expect(await after([spoken[0]!,spoken[1]!,{...spoken[2]!,startMs:13000}])).toEqual([fee])
  expect(await after([spoken[0]!,spoken[1]!,{...spoken[2]!,isFinal:false}])).toEqual([fee])
  // The segment is deleted from a note that still exists.
  expect(await after([spoken[0]!,spoken[1]!])).toEqual([fee])
  // Same words in another segment are not the cited passage.
  expect(await after([spoken[0]!,spoken[1]!,{...spoken[2]!,id:id(1999)}])).toEqual([fee])
  expect(await after(spoken)).toEqual([fee,pilot])
})

test("typed-notes and generated-notes citations depend only on their own passage",async()=>{
  const repository=makeMemoryRepository([harbour()]),citations=await issued(repository,id(1))
  const typed=citing(citations,"notes","berth fee"),generated=citing(citations,"summary","crane is booked")
  const check=()=>validateChatCitations([typed,generated],repository,signal())
  // Speech arrives, another typed line is edited and a line is added: nothing cited has changed.
  await run(repository.updateNote(id(1),{transcriptSegments:[...spoken,...later],body:"- berth fee is 300\n- crane: ask Ines\n- pilot by Friday"}))
  expect(await check()).toEqual([typed,generated])
  // The cited typed line is rewritten.
  await run(repository.updateNote(id(1),{body:"- berth fee went up\n- crane: ask Ines"}))
  expect(await check()).toEqual([generated])
  // The generated notes are replaced by a version without the cited line.
  await run(repository.updateNote(id(1),{enhanced:"# Decisions\n- The crane moved to Wednesday."}))
  expect(await check()).toEqual([])
  // A notes citation cannot carry transcript fields, and a quote cannot move between documents.
  await run(repository.updateNote(id(1),{body:harbour().body,enhanced:harbour().enhanced}))
  expect(await check()).toEqual([typed,generated])
  for(const forged of [{...typed,segmentId:spoken[0]!.id},{...typed,startMs:0},{...typed,source:"summary" as const},{...generated,source:"notes" as const}])expect(await validateChatCitations([forged],repository,signal())).toEqual([])
})

test("a citation cannot claim a revision the note has not reached, or a transcript passage from before any transcript existed",async()=>{
  const repository=makeMemoryRepository([harbour()]),citations=await issued(repository,id(1)),pilot=citing(citations,"transcript","harbour pilot")
  expect(pilot.transcriptRevision).toBe(3)
  expect(await validateChatCitations([pilot],repository,signal())).toEqual([pilot])
  expect(await validateChatCitations([{...pilot,transcriptRevision:4}],repository,signal())).toEqual([])
  expect(await validateChatCitations([{...pilot,transcriptRevision:0}],repository,signal())).toEqual([])
  // An earlier revision of the same note is the ordinary case after an append.
  expect(await validateChatCitations([{...pilot,transcriptRevision:1}],repository,signal())).toEqual([{...pilot,transcriptRevision:1}])
})

test("deleting the note withdraws every citation, and an answer that used it still ends with the retry error",async()=>{
  const repository=makeMemoryRepository([harbour()]),before=await issued(repository,id(1))
  const result=await stream(repository,id(1),citations=>`Pilot by Friday ${mark(citations,["transcript","harbour pilot"])[0]}. Fee ${mark(citations,["notes","berth fee"])[0]}.`,()=>run(repository.deleteNote(id(1))))
  expect(result.citations).toEqual([])
  expect(result.last).toEqual({type:"error",text:"A source changed while answering. Retry to use the current meeting notes."})
  expect(result.events.some(event=>event.type==="done")).toBe(false)
  expect(await checkChatCitations(before,repository,signal())).toEqual({valid:[],withdrawn:true})
})

test("an answer left with no passage it cited still ends with the retry error; one with any passage left completes",async()=>{
  const gone=makeMemoryRepository([harbour()])
  const none=await stream(gone,id(1),citations=>`Crane ${mark(citations,["transcript","booked the crane"])[0]}.`,()=>run(gone.updateNote(id(1),{transcriptSegments:[spoken[0]!,{...spoken[1]!,text:"We moved the crane to Wednesday."},spoken[2]!]})))
  expect(none.citations).toEqual([])
  expect(none.last.type).toBe("error")
  // No markers at all is an ordinary answer.
  const plain=makeMemoryRepository([harbour()])
  const unmarked=await stream(plain,id(1),()=>"Nothing about that has come up so far.",()=>run(plain.updateNote(id(1),{transcriptSegments:[...spoken,later[0]!]})))
  expect(unmarked.citations).toEqual([])
  expect(unmarked.last.type).toBe("done")
})

test("another account's note never validates, whatever the passage says, and appended speech does not change that",async()=>{
  const root=makeMemoryRepository(),alice=root.forUser(crypto.randomUUID()),bob=root.forUser(crypto.randomUUID())
  const mine=await run(alice.createNote({title:"Alice harbour",date:"2026-03-09",body:"- berth fee is 300",transcriptSegments:spoken}))
  const citations=await issued(alice,mine.id)
  expect(citations.length).toBeGreaterThan(0)
  expect(await validateChatCitations(citations,alice,signal())).toEqual([...citations])
  expect(await checkChatCitations(citations,bob,signal())).toEqual({valid:[],withdrawn:true})
  await run(alice.updateNote(mine.id,{transcriptSegments:[...spoken,...later]}))
  expect(await validateChatCitations(citations,alice,signal())).toEqual([...citations])
  expect(await checkChatCitations(citations,bob,signal())).toEqual({valid:[],withdrawn:true})
  // Bob holds a note with the very same words: Alice's citation does not attach to it, and his ID in her citation reads nothing.
  const theirs=await run(bob.createNote({title:"Bob harbour",date:"2026-03-09",body:"- berth fee is 300",transcriptSegments:spoken.map((segment,index)=>({...segment,id:id(3000+index)}))}))
  expect(await validateChatCitations(citations.map(citation=>({...citation,noteId:theirs.id})),alice,signal())).toEqual([])
  expect(await validateChatCitations(citations.map(citation=>({...citation,noteId:theirs.id})),bob,signal())).toEqual(citations.filter(citation=>citation.source==="notes").map(citation=>({...citation,noteId:theirs.id,title:"Bob harbour"})))
})

test("an answer cannot carry more distinct markers than the citations event holds",async()=>{
  const allowed=new Set(Array.from({length:60},(_,index)=>`S${index+1}`)),filter=new CitationStreamFilter(allowed)
  const text=Array.from({length:60},(_,index)=>`point ${index+1} [[S${index+1}]]`).join(" ")+" and again [[S1]] [[S60]]"
  const kept=filter.push(text.slice(0,200))+filter.push(text.slice(200))+filter.push("",true)
  expect(filter.used.size).toBe(MAX_ANSWER_CITATIONS)
  expect([...filter.used]).toEqual(Array.from({length:40},(_,index)=>`S${index+1}`))
  // A marker already in use may repeat; a new one past the limit is removed like an unknown one.
  expect(kept).toContain("point 40 [[S40]]");expect(kept).toContain("point 41  point 42");expect(kept.endsWith("and again [[S1]] ")).toBe(true)
  // End to end: 45 short passages, all cited in one answer.
  const many=Array.from({length:45},(_,index)=>seg(100+index,`Item ${index+1} costs ${index+11} euros.`,index*5000))
  const repository=makeMemoryRepository([harbour({body:"",enhanced:"",transcriptSegments:many,generationState:"idle",generatedRevision:null})])
  const result=await stream(repository,id(1),citations=>citations.map(citation=>`${citation.quote} [[${citation.id}]]`).join(" "))
  expect(result.citations).toHaveLength(40)
  expect(result.citations.map(citation=>citation.segmentId)).toEqual(many.slice(0,40).map(segment=>segment.id))
  expect(result.text).toContain("[[S40]]");expect(result.text).not.toContain("[[S41]]")
  expect(result.last.type).toBe("done")
})
