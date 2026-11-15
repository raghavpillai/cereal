import {expect, test} from "bun:test"
import {Effect} from "effect"
import {makeProviderServices,serializeProviderContext} from "../src/providers"
import {makeMemoryRepository} from "../src/repository"

const word=(text:string,speaker_id:string|null,start:number,type="word",end=start+0.3)=>({text,speaker_id,start,end,type})
const audio=new Uint8Array([0,1]) // Transport is injected; no provider call or audio upload.

for(const channel of ["mic","system"] as const){
  test(`${channel} preserves three speakers and returning turns without treating spacing as another person`,async()=>{
    let request:Request|undefined
    const providers=makeProviderServices({elevenLabsKey:"test-not-a-key",fetch:(async(input,init)=>{
      request=new Request(input,init)
      return Response.json({words:[
        word("Hello","speaker_0",0),word(" ",null,0.3,"spacing"),word("team.","speaker_0",0.4),
        word("I own design.","speaker_1",1),word(" ","unrelated-spacing-id",1.3,"spacing"),
        word("I own testing.","speaker_2",2),word("Thank you.","speaker_0",3),
      ]})
    }) as typeof fetch})
    const chunkId=crypto.randomUUID()
    const turns=await Effect.runPromise(providers.transcribe({audio,channel,chunkId,startMs:120000,durationMs:5000}))
    expect(turns.map(t=>t.speakerLabel)).toEqual(["Speaker 1","Speaker 2","Speaker 3","Speaker 1"])
    expect(turns.map(t=>t.text)).toEqual(["Hello team.","I own design.","I own testing.","Thank you."])
    expect(new Set(turns.map(t=>t.speakerId)).size).toBe(3)
    expect(turns[0]!.speakerId).toBe(turns[3]!.speakerId)
    expect(turns.every(t=>t.channel===channel && t.speakerId.startsWith(`${channel}:${chunkId}:`) && t.isFinal)).toBe(true)
    expect(turns[0]!.startMs).toBe(120000)
    expect(turns[3]!.endMs).toBe(123300)
    const form=await request!.formData()
    expect(form.get("diarize")).toBe("true")
    expect(form.get("use_multi_channel")).toBe("false")
    expect(form.get("timestamps_granularity")).toBe("word")
    expect(form.get("num_speakers")).toBeNull() // Never cap a microphone or remote group at two.
  })
}

test("opaque provider identities remain distinct and unknown speech stays unknown",async()=>{
  const prefix="provider-"+"x".repeat(150)
  const providers=makeProviderServices({elevenLabsKey:"test-not-a-key",fetch:(async()=>Response.json({words:[
    word("First.",prefix+"a",0),word("Second.",prefix+"b",1),word("Unattributed.",null,2),
    word("Still unknown.","",3),word("First again.",prefix+"a",4),
  ]})) as unknown as typeof fetch})
  const turns=await Effect.runPromise(providers.transcribe({audio,channel:"mic",chunkId:crypto.randomUUID(),startMs:0,durationMs:6000}))
  expect(turns.map(t=>t.speakerLabel)).toEqual(["Speaker 1","Speaker 2","Unknown speaker","Speaker 1"])
  expect(new Set(turns.map(t=>t.speakerId)).size).toBe(3)
  expect(turns.every(t=>t.speakerId.length<=100 && !t.speakerId.includes("provider-") && t.speakerLabel!=="You" && t.speakerLabel!=="Me")).toBe(true)
})

test("speaker identities never imply a cross-chunk or cross-source voice match",async()=>{
  const providers=makeProviderServices({elevenLabsKey:"test-not-a-key",fetch:(async()=>Response.json({words:[word("A turn.","speaker_0",0)]})) as unknown as typeof fetch})
  const first=crypto.randomUUID(),next=crypto.randomUUID()
  const run=(channel:"mic"|"system",chunkId:string)=>Effect.runPromise(providers.transcribe({audio,channel,chunkId,startMs:0,durationMs:1000}))
  const turns=await Promise.all([run("mic",first),run("system",first),run("mic",next)])
  expect(new Set(turns.map(t=>t[0]!.speakerId)).size).toBe(3)
  expect(turns.every(t=>t[0]!.speakerLabel==="Speaker 1")).toBe(true)
})

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
