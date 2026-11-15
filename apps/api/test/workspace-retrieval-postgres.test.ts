/**
 * Workspace-chat retrieval on the PostgreSQL search path, compared with the
 * in-memory path the evaluation uses by default. No model: the planner is
 * scripted, so this measures retrieval alone.
 *
 * The large fictional workspace (apps/api/eval/workspace/corpus.ts, 56 meetings)
 * is seeded for a throwaway user in the isolated test database, next to a
 * second account holding look-alike content. Each probe runs the same request
 * through both repositories; the meeting that holds the answer must come back
 * from both, and every difference between the two is printed as a table.
 */
import {beforeAll,describe,expect,test} from "bun:test"
import type {ChatInput,ChatStreamEventDTO} from "@meeting-notes/contracts"
import {Effect} from "effect"
import {makeMemoryRepository,type NotesRepositoryService} from "../src/repository"
import {prepareMeetingChat} from "../src/chat-retrieval"
import {selectMeetings} from "../src/meeting-search"
import type {ChatToolCall,ProviderServices} from "../src/providers"
import {day,largeWorkspace} from "../eval/workspace/corpus"
import {postgresRepository} from "../eval/workspace/harness"

const enabled=process.env.RUN_PG_TESTS==="1"&&!!process.env.DATABASE_URL&&/^\/mn_test_[a-f0-9]+$/.test(new URL(process.env.DATABASE_URL).pathname)
const now=new Date(),workspace=largeWorkspace(now),d=(daysAgo:number)=>day(now,daysAgo).iso
const base:ProviderServices={transcribe:()=>Effect.succeed([]),generate:()=>Effect.succeed(""),chat:()=>Effect.succeed("")}
const call=(name:string,args:unknown):ChatToolCall=>({id:`call_${name}`,name,arguments:JSON.stringify(args)})
interface Probe {name:string;message:string;
  /** Scripted planner calls. Empty: only the automatic first search runs. */
  calls?:ChatToolCall[];
  /** Text the retrieved context must contain: the needle. */
  needle:string;
  /** Text it must not contain. */
  absent?:string}
const PROBES:Probe[]=[
  {name:"old meeting, rare name",message:"What did Harrowgate originally ask for as a discount?",needle:"Ilse had asked for fifteen"},
  {name:"fact only in a transcript",message:"How many hours a week does Ilse's team spend on manual rescheduling?",needle:"twenty two hours a week"},
  {name:"common word in more than 20 meetings",message:"When is Export launching?",needle:"move the launch from"},
  {name:"meeting with only a transcript",message:"Which book did Bastien recommend?",needle:"The Quiet Roadmap"},
  // The newest meeting has the name only in speech; it must at least be listed for the planner to follow up.
  {name:"name only spoken, newest of several",message:"When did I last speak with Ilse?",needle:"Harrowgate – security review"},
  {name:"decision meeting among similar titles",message:"Who objected to the eight percent price increase?",needle:"Eight percent price increase for existing customers"},
  {name:"oldest quarter of the workspace",message:"What is our net revenue retention?",needle:"Net revenue retention: 104 percent"},
  {name:"number in a summary",message:"What is the stop limit per route?",needle:"750 stops per route"},
  {name:"nothing relevant",message:"What did we decide about a Lisbon office lease?",needle:"No meetings match",absent:"<summary>"},
  {name:"passage search",message:"and what did the on-call engineer see first?",calls:[call("search_transcripts",{queries:["consumer lag","eu-west"]})],needle:"consumer lag alarm on the eu-west queue, at six fifty two"},
  {name:"passage search inside one long meeting",message:"and the credit policy?",calls:[call("search_meetings",{queries:["incident review"]}),call("search_transcripts",{queries:["credit","policy"],ids:["m21"]})],needle:"one month of credit for customers who ask"},
  {name:"date range read",message:"and the week before?",calls:[call("get_meetings",{from:d(18),to:d(12)})],needle:"cut team templates from onboarding version one"},
  {name:"listing past the repository page of 20",message:"and how many meetings in all?",calls:[call("list_meetings",{limit:60})],needle:"Partner intro: Fennimore Consulting"},
]
async function run(repository:NotesRepositoryService,probe:Probe) {
  const events:ChatStreamEventDTO[]=[],results:string[]=[]
  let round=0
  // A follow-up phrasing ("and …") with history keeps the automatic search out of scripted probes.
  const input:ChatInput={message:probe.message,history:probe.calls?[{role:"user",content:"Earlier question"},{role:"assistant",content:"Earlier answer"}]:[],includeSources:true,timeZone:"UTC"}
  const prepared=await prepareMeetingChat({noteId:null,input,repository,signal:new AbortController().signal,now,enrich:async note=>({...note,meetingContext:workspace.contexts.get(note.id)??null}),emit:event=>{events.push(event)},
    providers:{...base,chatToolStep:async messages=>{const last=messages.at(-1)!;if(last.role==="tool")results.push(last.content!);const next=probe.calls?.[round++];return next?[next]:[]}}})
  const order=results.flatMap(text=>[...text.matchAll(/^(?:m\d+ \| (\S+) \S+ \| ([^|]+) \||<meeting id="m\d+" title="([^"]*)" date="(\S+))/gm)].map(match=>match[1]?`${match[1]} ${match[2]!.trim()}`:`${match[4]} ${match[3]}`))
  return {prepared,events,results,order,steps:events.filter(event=>event.tool&&event.tool.status!=="running").map(event=>`${event.tool!.name}:${event.tool!.label}`)}
}

const memory=makeMemoryRepository(workspace.notes)
test("in-memory retrieval over the large workspace finds every probe's needle",async()=>{
  for(const probe of PROBES){
    const result=await run(memory,probe),text=JSON.stringify([result.prepared,result.results])
    expect(text.includes(probe.needle),`${probe.name}: needle missing`).toBe(true)
    if(probe.absent)expect(text.includes(probe.absent),`${probe.name}: unexpected content`).toBe(false)
  }
},60_000)
test("search rows without transcripts, as SQL returns them, lead to the same answer context",async()=>{
  // PostgreSQL search returns note metadata only. The same probes, with the
  // transcript stripped from every search row, must retrieve exactly what the
  // in-memory rows (which carry it) retrieve.
  const stripped:NotesRepositoryService={...memory,searchNotes:input=>Effect.map(memory.listNotes(),notes=>selectMeetings(notes,input).map(row=>({...row,transcriptSegments:[]})))}
  for(const probe of PROBES){
    const a=await run(memory,probe),b=await run(stripped,probe)
    expect(b.prepared.note.body,probe.name).toBe(a.prepared.note.body)
    expect(b.steps,probe.name).toEqual(a.steps)
  }
},60_000)
describe.skipIf(!enabled)("workspace retrieval on PostgreSQL against the in-memory search",()=>{
  let postgres:NotesRepositoryService
  beforeAll(async()=>{postgres=await postgresRepository(workspace)},120_000)
  test("every probe finds its needle on both paths; differences are reported",async()=>{
    const rows:string[]=[]
    let identical=0
    for(const probe of PROBES){
      const a=await run(memory,probe),b=await run(postgres,probe)
      for(const [path,result] of [["memory",a],["postgres",b]] as const){
        const text=JSON.stringify([result.prepared,result.results])
        expect(text.includes(probe.needle),`${probe.name}: needle missing on ${path}`).toBe(true)
        if(probe.absent)expect(text.includes(probe.absent),`${probe.name}: unexpected content on ${path}`).toBe(false)
        // The second account's look-alike note never surfaces, in results, events or citations.
        expect(JSON.stringify([result.prepared,result.events])).not.toContain("FOREIGN_EVAL_SENTINEL")
        expect(result.prepared.citations.length).toBeLessThanOrEqual(40)
      }
      const same=a.prepared.note.body===b.prepared.note.body
      if(same)identical++
      const firstDifference=a.order.findIndex((item,index)=>item!==b.order[index])
      rows.push(`| ${probe.name} | ${same?"identical":"differs"} | ${a.order.length} / ${b.order.length} | ${same?"–":firstDifference<0?(a.order.length===b.order.length?"same meetings, same order; passages differ":"same prefix, different length"):`rank ${firstDifference+1}: ${a.order[firstDifference]??"(none)"} vs ${b.order[firstDifference]??"(none)"}`} | ${a.prepared.citations.length} / ${b.prepared.citations.length} | ${a.steps.join("; ")===b.steps.join("; ")?"same":"differ"} |`)
    }
    console.log(`\nWorkspace retrieval, in-memory vs PostgreSQL (${PROBES.length} probes, ${workspace.notes.length} meetings): ${identical} identical answer contexts\n| Probe | Answer context | Meetings shown (memory / postgres) | First difference in order | Sources (memory / postgres) | Steps |\n| --- | --- | --- | --- | --- | --- |\n${rows.join("\n")}\n`)
  },120_000)
})
