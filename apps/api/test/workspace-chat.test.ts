/** Chat across meetings (`src/ai/workspace-chat.ts`, `workspace-tools.ts`):
 * what the model is shown, what it can reach, and what it can cite. No network. */
import {expect,test} from "bun:test"
import {Effect} from "effect"
import type {ChatInput,ChatStreamEventDTO,NoteDTO} from "@meeting-notes/contracts"
import {makeMemoryRepository} from "../src/repository"
import {prepareMeetingChat,validateChatCitations} from "../src/chat-retrieval"
import type {ChatToolCall,ChatToolMessage,ProviderServices} from "../src/providers"
import {ANSWER_SYSTEM_PROMPT,MEETING_CHAT_TOOLS,RESEARCH_SYSTEM_PROMPT,calendarAnchors,workspaceChatMessages} from "../src/ai/workspace-chat"
import {bestWindow,containsWord,coverageNote,searchWords} from "../src/ai/workspace-tools"

const id=(n:number)=>`00000000-0000-4000-8000-${String(n).padStart(12,"0")}`
const segment=(n:number,text:string,startMs:number)=>({id:id(5000+n),channel:"system" as const,speakerId:`system:${id(900)}:speaker_1`,speakerLabel:"Speaker 1",text,isFinal:true,startMs,endMs:startMs+4000})
const note=(n:number,fields:Partial<NoteDTO>):NoteDTO=>({id:id(n),title:"Untitled",body:"",enhanced:"",date:"2026-03-09",duration:"",createdAt:"2026-03-09T10:00:00.000Z",updatedAt:"2026-03-09T11:00:00.000Z",transcriptSegments:[],recordingState:"stopped",generationState:"idle",transcriptRevision:fields.transcriptSegments?.length?1:0,generatedRevision:null,...fields})
const now=new Date("2026-03-10T15:00:00.000Z")
const base:ProviderServices={generate:()=>Effect.succeed(""),chat:()=>Effect.succeed("")}
const call=(n:number,name:string,args:unknown):ChatToolCall=>({id:`call_${n}`,name,arguments:JSON.stringify(args)})
/** Runs retrieval with a scripted planner and returns everything observable. */
async function research(seed:ReadonlyArray<NoteDTO>,input:ChatInput,script:ReadonlyArray<ReadonlyArray<ChatToolCall>>|null,options:{readCharacterBudget?:number}={}) {
  const events:ChatStreamEventDTO[]=[],rounds:ChatToolMessage[][]=[]
  let round=0
  const providers:ProviderServices={...base,...(script?{chatToolStep:async messages=>{rounds.push(JSON.parse(JSON.stringify(messages)));return script[round++]??[]}}:{})}
  const repository=makeMemoryRepository(seed,()=>now)
  const prepared=await prepareMeetingChat({noteId:null,input,repository,providers,signal:new AbortController().signal,now,enrich:async source=>source,emit:event=>{events.push(event)},...options})
  return {prepared,events,rounds,repository,steps:events.filter(event=>event.tool).map(event=>event.tool!)}
}

test("the coverage note is a function of the steps that ran: nothing when everything was read, one short line when the research was partial",()=>{
  const step=(name:string,doing:string,label:string,resultCount?:number)=>[{name,status:"running",label:doing},{name,status:"completed",label,...(resultCount===undefined?{}:{resultCount})}]
  const failed={name:"get_meetings",status:"failed",label:"Couldn't finish this lookup"}
  expect(coverageNote([])).toBe("")
  // Every lookup ran and read what it found: the answer says nothing about its research, with or without a transcript.
  expect(coverageNote([...step("search_meetings","Searching meetings for “Windrush”","Found 1 meeting",1),...step("get_meetings","Looking deeper into 1 note","Looked deeper into 1 note",1)])).toBe("")
  expect(coverageNote([...step("get_meetings","Looking deeper into 5 notes from 2026-11-02 to 2026-11-08","Looked deeper into 5 notes",5),...step("get_meeting_transcript","Reading transcript for “Coffee chat”","Read transcript for “Coffee chat”",1)])).toBe("")
  expect(coverageNote([...step("search_transcripts","Searching transcripts for “a”","Found 0 transcript passages in 0 meetings",0)])).toBe("")
  // A lookup failed.
  expect(coverageNote([...step("search_meetings","Searching meetings for “a”","Found 3 meetings",3),{name:"get_meetings",status:"running",label:"Looking deeper into 1 note"},failed])).toBe("One lookup failed, so this may be incomplete.")
  expect(coverageNote([...step("search_meetings","Searching meetings for “a”","Found 3 meetings",3),failed,failed])).toBe("2 lookups failed, so this may be incomplete.")
  expect(coverageNote([{name:"search_meetings",status:"running",label:"Searching meetings"},{name:"search_meetings",status:"failed",label:"Couldn't plan the search"}])).toBe("The meeting lookup failed, so nothing was searched or read.")
  // Something found did not fit, or a period holds more meetings than were read.
  expect(coverageNote(step("get_meetings","Looking deeper into 9 notes","Looked deeper into 6 of 9 notes (context limit)",6))).toBe("Some of what was found was too long to read in full.")
  expect(coverageNote(step("search_transcripts","Searching transcripts for “a”","Read 4 of 9 transcript passages in 2 meetings (context limit)",4))).toBe("Some of what was found was too long to read in full.")
  expect(coverageNote(step("get_meetings","Looking deeper into 12 notes from 2026-01-01 to 2026-03-31","Looked deeper into 12 notes (more exist in this period)",12))).toBe("More meetings exist in this period than were read.")
  // Always one line.
  const all=coverageNote([...step("get_meetings","Looking deeper into 12 notes","Looked deeper into 12 notes (more exist in this period)",12),...step("search_transcripts","Searching transcripts for “a”","Read 4 of 9 transcript passages in 2 meetings (context limit)",4),failed])
  expect(all).toBe("One lookup failed, so this may be incomplete. Some of what was found was too long to read in full. More meetings exist in this period than were read.");expect(all).not.toContain("\n")
})

test("search words drop what would match every meeting, and ranking counts only matches at the start of a word",()=>{
  expect(searchWords("What did we decide about the Lisbon office last week?")).toEqual(["lisbon","office"])
  expect(searchWords("List my recent to-dos")).toEqual([])
  expect(containsWord("We are not reopening it","opening")).toBe(false)
  expect(containsWord("Opening the CSV; price-increase","opening")).toBe(true)
  expect(containsWord("the price increases next year","increase")).toBe(true)
  const text=`${"filler words ".repeat(200)}the rollback rehearsal is on March 28 ${"more filler ".repeat(200)}`,span=bestWindow(text,["rollback","rehearsal"],300)
  expect(span.end-span.start).toBeLessThanOrEqual(300)
  expect(text.slice(span.start,span.end)).toContain("rollback rehearsal is on March 28")
})

test("the research model starts from scope metadata only, and both system prompts are constants a provider can cache",async()=>{
  const seed=[note(1,{title:"Halden pricing review",enhanced:"# Pricing\n- SECRET_SUMMARY_TEXT 14% proposed.",body:"SECRET_TYPED_NOTE",transcriptSegments:[segment(1,"SECRET_TRANSCRIPT_TEXT about pricing",0)]}),note(2,{title:"Design critique",date:"2026-03-08",enhanced:"# Onboarding\n- Empty state."})]
  const first=await research(seed,{message:"List my recent to-dos",history:[],includeSources:true,timeZone:"Europe/Stockholm"},[[]])
  const opening=first.rounds[0]!
  expect(opening[0]).toEqual({role:"system",content:RESEARCH_SYSTEM_PROMPT,cache:true})
  expect(RESEARCH_SYSTEM_PROMPT).not.toMatch(/\d{4}-\d{2}-\d{2}/);expect(ANSWER_SYSTEM_PROMPT).not.toMatch(/\d{4}-\d{2}-\d{2}/)
  // Stable first, volatile last: the list of meetings is its own message, marked for the provider's prompt cache,
  // and the time of day comes after it, with the question.
  const scope=opening.at(-2)!,user=opening.at(-1)!.content!
  expect(scope.cache).toBe(true)
  expect(scope.content).toMatch(/^<workspace>\n[\s\S]*\n<\/workspace>$/)
  expect(scope.content).toContain("m1 | 2026-03-09 Mon | Halden pricing review | has: my notes, summary, transcript")
  expect(scope.content).not.toMatch(/Now:|16:00/)
  expect(user).toMatch(/^<now>\nNow: Tuesday 2026-03-10, 16:00 \(time zone Europe\/Stockholm\)\.\n[\s\S]*\n<\/now>\n\nList my recent to-dos$/)
  expect(user).toContain("last week is 2026-03-02 to 2026-03-08")
  // Titles, dates and what exists; never content, never database IDs.
  for(const secret of ["SECRET_SUMMARY_TEXT","SECRET_TYPED_NOTE","SECRET_TRANSCRIPT_TEXT",id(1),id(2)])expect(JSON.stringify(opening)).not.toContain(secret)
  expect(JSON.stringify(MEETING_CHAT_TOOLS)).not.toMatch(/uuid/i)
  expect(calendarAnchors({today:"2026-03-01",now:new Date("2026-03-01T12:00:00Z"),timeZone:"UTC"})).toContain("this week is Mon 2026-02-23 to Sun 2026-03-01; last week is 2026-02-16 to 2026-02-22; this month began 2026-03-01; last month is 2026-02-01 to 2026-02-28")
  const answer=workspaceChatMessages(first.prepared.note,{message:"List my recent to-dos",history:[{role:"user",content:"Earlier question"},{role:"assistant",content:"Earlier answer. [[S3]]"}]})
  expect(answer[0]).toEqual({role:"system",content:ANSWER_SYSTEM_PROMPT,cache:true})
  // Source markers of earlier answers belong to earlier requests.
  expect(answer[2]!.content).toBe("Earlier answer.")
  expect(answer.at(-1)!.content.endsWith("Question: List my recent to-dos")).toBe(true)
})

test("notes are read before transcripts; a date range reads exactly the meetings in it; a meeting without notes yields its transcript, shown as a step",async()=>{
  const seed=[
    note(1,{title:"Weekly sync",date:"2026-03-02",enhanced:"# Decisions\n- Ship the importer on the 21st.",transcriptSegments:[segment(1,"SPOKEN_ONLY_DETAIL the importer budget is nine thousand",1000)]}),
    note(2,{title:"Weekly sync",date:"2026-03-05",body:"typed: ask about rollout"}),
    note(3,{title:"Hallway chat",date:"2026-03-06",transcriptSegments:[segment(3,"Tomasz owns the rollback rehearsal.",2000)]}),
    note(4,{title:"Weekly sync",date:"2026-02-20",enhanced:"# Decisions\n- OUT_OF_RANGE_DECISION"}),
  ]
  const r=await research(seed,{message:"What happened last week?",history:[],includeSources:true,timeZone:"UTC"},[[call(1,"get_meetings",{from:"2026-03-02",to:"2026-03-08",then:"answer"})]])
  // `then: "answer"` ends research without a further round.
  expect(r.rounds).toHaveLength(1)
  const body=r.prepared.note.body
  expect(body).toContain("Ship the importer on the 21st");expect(body).toContain("typed: ask about rollout");expect(body).toContain("Tomasz owns the rollback rehearsal")
  expect(body).not.toContain("OUT_OF_RANGE_DECISION");expect(body).not.toContain("SPOKEN_ONLY_DETAIL")
  expect(body).toContain("<transcript note=\"not retrieved\"/>")
  expect(r.steps.map(step=>`${step.status}: ${step.label}`)).toEqual([
    "running: Looking deeper into 3 notes from 2026-03-02 to 2026-03-08","completed: Looked deeper into 3 notes",
    "running: Reading transcript for “Hallway chat”","completed: Read transcript for “Hallway chat”"])
  // Everything asked for was read, so the answer is given nothing to say about its research.
  expect(coverageNote(r.steps)).toBe("");expect(body).not.toContain("<coverage_note>")
  // Each passage is written once and carries exactly one source ID.
  expect(body.match(/Ship the importer on the 21st/g)).toHaveLength(1)
  expect(r.prepared.citations.map(citation=>citation.source).sort()).toEqual(["notes","summary","transcript"])
  expect(await validateChatCitations(r.prepared.citations,r.repository,new AbortController().signal)).toHaveLength(3)
})

test("an old meeting is found behind many newer ones that share a common word, and a transcript-only fact comes back with its passage",async()=>{
  const seed=[
    ...Array.from({length:30},(_,index)=>note(100+index,{title:`Status meeting ${index}`,date:`2026-02-${String(1+index%28).padStart(2,"0")}`,enhanced:"# Notes\n- General pricing chatter and renewal status, nothing decided."})),
    note(1,{title:"Vendor call",date:"2025-11-03",enhanced:"# Summary\n- Talked through the renewal.",transcriptSegments:[segment(1,"Did you settle the Zephyrine pricing?",1000),segment(2,"Yes. Zephyrine pricing is 412 dollars per seat.",6000)]}),
  ]
  const r=await research(seed,{message:"What is the Zephyrine pricing?",history:[],includeSources:true},[[]])
  const result=r.rounds[0]!.at(-1)!
  expect(result.role).toBe("tool")
  // The rare word outranks thirty newer matches for the common one.
  expect(result.content!.indexOf("Vendor call")).toBeGreaterThan(-1)
  expect(result.content!.indexOf("Vendor call")).toBeLessThan(result.content!.indexOf("Status meeting"))
  expect(result.content).toContain("412 dollars per seat")
  expect(r.steps.filter(step=>step.status==="completed").map(step=>step.label)).toEqual(["Found 6 meetings","Looked deeper into 1 note","Read transcript for “Vendor call”"])
  const spoken=r.prepared.citations.find(citation=>citation.source==="transcript"&&citation.quote.includes("412"))!
  expect(spoken).toMatchObject({noteId:id(1),segmentId:id(5002),startMs:6000})
  // A word that matches nothing is named, so the model does not go looking for it.
  const absent=await research(seed,{message:"What did we decide about Quetzal pricing?",history:[],includeSources:true},[[]])
  expect(absent.rounds[0]!.at(-1)!.content).toContain("contains \"quetzal\"")
})

test("at most 40 sources are issued, every meeting that was read keeps one, and every quote is an exact stored passage",async()=>{
  const seed=Array.from({length:36},(_,index)=>note(200+index,{title:`Project review ${index}`,date:`2026-02-${String(1+index%28).padStart(2,"0")}`,
    enhanced:`# Decisions\n- Decision ${index} approved.\n\n# Action items\n- Owner ${index} sends the plan.\n\n# Risks\n- Risk ${index} noted.`,body:`typed note ${index}\n\nsecond paragraph ${index}`,
    transcriptSegments:[segment(index*2,`Spoken detail ${index} about the plan.`,1000)]}))
  const r=await research(seed,{message:"Everything about the project reviews",history:[],includeSources:true},[
    [call(1,"list_meetings",{limit:40})],
    [call(2,"get_meetings",{ids:Array.from({length:12},(_,index)=>`m${index+1}`)})],
    [call(3,"get_meetings",{ids:Array.from({length:12},(_,index)=>`m${index+13}`)})],
    [call(4,"get_meetings",{ids:Array.from({length:12},(_,index)=>`m${index+25}`)})],
  ])
  const citations=r.prepared.citations
  expect(citations.length).toBeLessThanOrEqual(40)
  expect(citations.map(citation=>citation.id)).toEqual(citations.map((_,index)=>`S${index+1}`))
  expect(new Set(citations.map(citation=>citation.noteId)).size).toBe(36)
  expect(await validateChatCitations(citations,r.repository,new AbortController().signal)).toHaveLength(citations.length)
  // Every ID in the context was issued, and every issued ID is in the context once.
  const marked=[...r.prepared.note.body.matchAll(/\[(S\d+)\] /g)].map(match=>match[1]!)
  expect(marked).toEqual(citations.map(citation=>citation.id))
  // A legacy client gets the same content with no IDs at all.
  const legacy=await research(seed.slice(0,3),{message:"Everything about the project reviews",history:[]},[[call(1,"get_meetings",{ids:["m1","m2"]})]])
  expect(legacy.prepared.citations).toEqual([]);expect(legacy.prepared.note.citations).toBeUndefined()
  expect(legacy.prepared.note.body).not.toMatch(/\[S\d+\]/);expect(legacy.prepared.note.body).toContain("approved.")
  expect(legacy.events).toEqual([])
})

test("another account's meetings are unreachable by search, alias, forged ID or citation",async()=>{
  const root=makeMemoryRepository([],()=>now),mine=root.forUser(id(1)),theirs=root.forUser(id(2))
  const own=await Effect.runPromise(mine.createNote({title:"Cedar launch review",date:"2026-03-09",enhanced:"# Next steps\n- Alex sends the Cedar checklist."}))
  const foreign=await Effect.runPromise(theirs.createNote({title:"Cedar launch review",date:"2026-03-09",body:"FOREIGN_SENTINEL Cedar budget",enhanced:"FOREIGN_SENTINEL Cedar",transcriptSegments:[segment(9,"FOREIGN_SENTINEL Cedar spoken",0)]}))
  const events:ChatStreamEventDTO[]=[],seen:string[]=[]
  let round=0
  const script=[[call(1,"search_meetings",{queries:["Cedar","budget","spoken"]})],[call(2,"get_meetings",{ids:[foreign.id,"m2","m1"]})],[call(3,"search_transcripts",{queries:["Cedar"],ids:[foreign.id]})],[call(4,"get_meeting_transcript",{id:foreign.id})],[call(5,"get_meetings",{from:"2026-01-01",to:"2026-12-31"})]]
  const prepared=await prepareMeetingChat({noteId:null,input:{message:"Cedar budget",history:[],includeSources:true},repository:mine,signal:new AbortController().signal,now,enrich:async source=>source,emit:event=>{events.push(event)},
    providers:{...base,chatToolStep:async messages=>{seen.push(JSON.stringify(messages));return script[round++]??[]}}})
  const everything=JSON.stringify([prepared,events,seen])
  expect(everything).not.toContain("FOREIGN_SENTINEL")
  expect(prepared.citations.length).toBeGreaterThan(0)
  expect(prepared.citations.every(citation=>citation.noteId===own.id)).toBe(true)
  expect(events.flatMap(event=>event.tool?.results??[]).every(result=>result.noteId===own.id)).toBe(true)
  // The forged ID is named back to the model as not read; nothing else about it is known.
  expect(seen.at(-1)).toContain("not a meeting from this conversation")
  expect(events.filter(event=>event.tool?.status==="failed").length).toBe(2)
  // Two lookups failed, so the answer is told to say so, in one line; the workspace answer still cites its sources.
  expect(prepared.note.body.match(/<coverage_note>[^\n]*<\/coverage_note>/g)).toEqual(["<coverage_note>2 lookups failed, so this may be incomplete.</coverage_note>"])
  expect(prepared.note.body).toMatch(/\[S1\] /)
})
