import {expect, test} from "bun:test"
import {Effect} from "effect"
import {makeProviderServices,serializeProviderContext} from "../src/providers"
import {makeMemoryRepository} from "../src/repository"

const word=(text:string,speaker_id:string|null,start:number,type="word",end=start+0.3)=>({text,speaker_id,start,end,type})
const audio=new Uint8Array([0,1]) // Transport is injected; no provider call or audio upload.

test("summary, chat and evidence preserve speaker identity when chunk labels repeat",async()=>{
  const repository=makeMemoryRepository()
  const note=await Effect.runPromise(repository.createNote({title:"Synthetic speaker scope",transcriptSegments:[
    {id:crypto.randomUUID(),channel:"mic",speakerId:`mic:${crypto.randomUUID()}:speaker_1`,speakerLabel:"Speaker 1",text:"First part.",isFinal:true,startMs:0,endMs:500},
    {id:crypto.randomUUID(),channel:"mic",speakerId:`mic:${crypto.randomUUID()}:speaker_1`,speakerLabel:"Speaker 1",text:"Another part.",isFinal:true,startMs:120000,endMs:120500},
  ]}))
  const context=JSON.parse(serializeProviderContext(note))
  expect(context.transcript.map((t:any)=>t.speakerId)).toEqual(note.transcriptSegments.map(t=>t.speakerId))
  expect(context.transcript.every((t:any)=>t.channel==="mic" && t.speaker==="Speaker 1")).toBe(true)
  const requests:any[]=[]
  const providers=makeProviderServices({openRouterKey:"test-not-a-key",fetch:(async(_,init)=>{
    requests.push(JSON.parse(String(init?.body)))
    return Response.json({choices:[{message:{content:requests.length===3?JSON.stringify({summary:"Supported",segmentIds:[note.transcriptSegments[0]!.id]}):"Supported"}}]})
  }) as typeof fetch})
  await Effect.runPromise(providers.generate(note))
  await Effect.runPromise(providers.chat(note,{message:"Who spoke?",history:[]}))
  await Effect.runPromise(providers.evidence!(note,"First part"))
  // A message that carries a prompt-cache breakpoint is sent as text parts (generation's instructions, evidence's transcript).
  const text=(content:string|Array<{text:string}>)=>typeof content==="string"?content:content.map(part=>part.text).join("")
  for(const request of requests){
    expect(text(request.messages[0].content)).toContain("Repeated labels on different speakerIds")
    const supplied=JSON.parse(text(request.messages[1].content).replace(/^Meeting context: /,""))
    expect(supplied.transcript.map((t:any)=>t.speakerId)).toEqual(note.transcriptSegments.map(t=>t.speakerId))
  }
})
