/** A chat about one note answers from that note, and reaches the person's other meetings when a question asks for
 * them: the workspace research runs, its findings go with that one question (without source markers), and a question
 * about the meeting itself makes no search at all. In a recording, the kept thread holds the plain question. */
import {expect,test} from "bun:test"
import {Effect} from "effect"
import type {ChatStreamEventDTO,NoteDTO} from "@meeting-notes/contracts"
import {makeApiHandler} from "../src/api"
import {makeMemoryRepository} from "../src/repository"
import {makeProcessingService} from "../src/processing"
import type {ProviderServices} from "../src/providers"
import {chatAnswerMessages} from "../src/ai"
import {reachesBeyondMeeting} from "../src/ai/meeting-chat"

const token="note-chat-other-meetings-operator-token-at-least-32-chars"
const id=(n:number)=>`00000000-0000-4000-8000-${String(n).padStart(12,"0")}`
const segment=(n:number,text:string,startMs:number)=>({id:id(5000+n),channel:"system" as const,speakerId:"system:n:1",speakerLabel:"Speaker 1",text,isFinal:true,startMs,endMs:startMs+4000})
const note=(n:number,title:string,date:string,body:string,lines:string[],recordingState:NoteDTO["recordingState"]="stopped"):NoteDTO=>({id:id(n),title,body,enhanced:"",date,duration:"10:00",createdAt:`${date}T10:00:00.000Z`,updatedAt:`${date}T11:00:00.000Z`,
  transcriptSegments:lines.map((text,index)=>segment(n*10+index,text,index*8000)),recordingState,generationState:"idle",transcriptRevision:1,generatedRevision:null})
const frames=(text:string)=>text.split("\n\n").filter(frame=>frame.startsWith("data:")).map(frame=>JSON.parse(frame.slice(5)) as ChatStreamEventDTO)

function app(recordingState:NoteDTO["recordingState"]) {
  const sent:string[]=[]
  let planned=0
  const providers:ProviderServices={generate:()=>Effect.succeed(""),chat:()=>Effect.succeed(""),
    async chatToolStep(){return planned++%2===0?[{id:`call-${planned}`,name:"search_meetings",arguments:JSON.stringify({queries:["Quillfeather"]})}]:[]},
    async *chatStream(prepared,input){sent.push(JSON.stringify(chatAnswerMessages(prepared,input)));yield "Answer."}}
  const repository=makeMemoryRepository([
    note(1,"Budget speech","2026-11-16","",["Tariffs are raising prices rather than reducing the deficit."],recordingState),
    note(2,"Quillfeather pitch","2026-11-12","- the game tracks changes in political beliefs",["Quillfeather tracks how political beliefs change over time."]),
  ])
  const api=makeApiHandler(repository,token,{processing:makeProcessingService(repository,{providers})})
  const ask=async(message:string,history:Array<{role:"user"|"assistant";content:string}>=[])=>frames(await (await api.handler(new Request(`http://localhost/api/v1/notes/${id(1)}/chat/stream`,{method:"POST",headers:{Authorization:`Bearer ${token}`,"Content-Type":"application/json"},body:JSON.stringify({message,history,includeSources:true})}))).text())
  /** The question message of a request: the last one. */
  const question=(index:number)=>(JSON.parse(sent[index]!) as Array<{role:string;content:string}>).at(-1)!
  return {api,ask,sent,question,get planned(){return planned}}
}

for(const state of ["stopped","recording"] as const)test(`a note's chat (${state==="recording"?"during the recording":"finished"}) searches other meetings only when asked`,async()=>{
  const f=app(state)
  try{
    const plain=await f.ask("What did I miss?")
    expect(plain.at(-1)!.type).toBe("done")
    expect(f.planned).toBe(0)
    expect(f.question(0).content).not.toContain("<other_meetings>")
    // The instructions say other meetings can be searched, so the model never claims there are none.
    expect(f.sent[0]).toContain("Never say there are no other notes")

    const beyond=await f.ask("Can you look at other notes about Quillfeather?",[{role:"user",content:"What did I miss?"},{role:"assistant",content:"Answer."}])
    expect(beyond.at(-1)!.type).toBe("done")
    expect(f.planned).toBeGreaterThan(0)
    // The search shows as progress, as it does in a chat across notes.
    expect(beyond.some(event=>event.type==="tool"&&event.tool?.name==="search_meetings")).toBe(true)
    const last=f.question(1)
    expect(last.role).toBe("user")
    expect(last.content).toContain("<other_meetings>")
    expect(last.content).toContain("Quillfeather pitch")
    // No source markers reach a chat that shows none.
    expect(f.sent[1]).not.toMatch(/\[S\d+\]/)

    // The next question about the meeting itself carries no search forward.
    await f.ask("And who said that?",[{role:"user",content:"What did I miss?"},{role:"assistant",content:"Answer."},{role:"user",content:"Can you look at other notes about Quillfeather?"},{role:"assistant",content:"Answer."}])
    expect(f.question(2).content).not.toContain("<other_meetings>")
    expect(f.sent[2]).not.toContain("Quillfeather pitch")
  }finally{await f.api.dispose()}
})

test("which questions reach beyond the meeting",()=>{
  const beyond=["Can you look at other notes?","What did we decide in previous meetings?","Search my notes for pricing","What happened last week?","Did Maya mention this in another meeting?","look through my meetings for the launch date","What did we talk about last time?","Across all my meetings, who owns hiring?","Compare with the last sync"]
  const within=["What did I miss","Give me a quote from the transcript","What was said in the last part of this call?","Summarize my notes","List my todos","What's in the notes?","Write follow up email","Who spoke last?","What did they say in the last minute?"]
  for(const message of beyond)expect([message,reachesBeyondMeeting({message,history:[]})]).toEqual([message,true])
  for(const message of within)expect([message,reachesBeyondMeeting({message,history:[]})]).toEqual([message,false])
  // A reply to an offer to look through other notes.
  expect(reachesBeyondMeeting({message:"Find something and do it",history:[{role:"user",content:"Can you look at other notes?"},{role:"assistant",content:"Yes. Which topic or meeting should I look for in your other notes?"}]})).toBe(true)
  expect(reachesBeyondMeeting({message:"Thanks",history:[{role:"assistant",content:"The pilot starts on 9 June."}]})).toBe(false)
})
