/** Chat during a recording as an append-only request: every request repeats the previous one byte for
 * byte and adds one message, whatever happens to the transcript in between; and when a thread cannot
 * be continued, a new one is started that is itself the beginning of an append-only thread. No network. */
import {expect,test} from "bun:test"
import {Effect,Stream} from "effect"
import type {ChatInput,ChatStreamEventDTO,NoteDTO,TranscriptSegmentDTO} from "@meeting-notes/contracts"
import {makeMemoryRepository} from "../repository"
import {makeProcessingService} from "../processing"
import type {ProviderNote,ProviderServices} from "../providers"
import {CitationStreamFilter,validateChatCitations} from "./citations"
import {prepareMeetingChat,chatAnswerMessages,chatAnswerParameters} from "./index"
import {MEETING_CHAT_LIMITS,meetingChatDocument,meetingChatSystemPrompt,type LiveTranscriptState,type MeetingChatNote} from "./meeting-chat"
import {makeLiveConversationStore,type LiveConversationStore} from "./live-conversation"
import {cachedRequest} from "./prompt-cache"
import type {LanguageMessage} from "./shared"

const id=(n:number)=>`00000000-0000-4000-8000-${String(n).padStart(12,"0")}`
/** A segment as live transcription stores it: voices numbered once per channel. */
const seg=(n:number,channel:"mic"|"system",speaker:number,text:string,startMs:number,fields:Partial<TranscriptSegmentDTO>={}):TranscriptSegmentDTO=>({id:id(1000+n),channel,speakerId:`${channel}:n:${speaker}`,speakerLabel:`Speaker ${speaker}`,text,isFinal:true,startMs,endMs:startMs+4000,...fields})
const note=(fields:Partial<NoteDTO>={}):NoteDTO=>({id:id(1),title:"Halden pricing review",body:"",enhanced:"",date:"2026-03-09",duration:"1:10",createdAt:"2026-03-09T10:00:00.000Z",updatedAt:"2026-03-09T11:00:00.000Z",transcriptSegments:[],recordingState:"recording",generationState:"idle",transcriptRevision:1,generatedRevision:null,...fields})
const lines=[
  seg(1,"mic",1,"Thanks for joining. Today I want to walk through the tiered pricing.",0),
  seg(2,"system",1,"Sure. The annual commitment worries me, we only have budget approved through June.",5200),
  seg(3,"system",2,"This is Ines from procurement. Please send a revised quote by Thursday.",19_000),
  seg(4,"mic",1,"Will do. I will send the revised quote with 40 seats by Thursday.",66_000),
  seg(5,"system",1,"Great. And the pilot starts on the ninth of June.",131_000),
  seg(6,"system",2,"Legal needs the data processing agreement two weeks before that.",140_000),
  seg(7,"mic",1,"Understood. I will ask Tomasz to draft it this week.",260_000),
]
const silent:ProviderServices={generate:()=>Effect.succeed(""),chat:()=>Effect.succeed("")}
type Turn=ChatInput["history"][number]
type Tail=LiveTranscriptState["tail"]

/** One meeting and one client. `ask` prepares a request the way `processing.chatStream` does; `answer`
 * is what the client then holds and the server was told. */
function meeting(initial:NoteDTO,store:LiveConversationStore=makeLiveConversationStore()) {
  const repository=makeMemoryRepository([initial]),history:Turn[]=[],rebuilt:Array<{noteId:string;reason:string;historyMessages:number;shownSegments:number;requestCharacters:number}>=[]
  let tail:Tail|null=null
  const self={
    repository,history,store,rebuilt,
    update:(fields:Parameters<typeof repository.updateNote>[1])=>Effect.runPromise(repository.updateNote(initial.id,fields)),
    speaking(text:string|null,channel:"mic"|"system"="system",speakerId="system:n:1"){tail=text===null?null:[{channel,text,startMs:0,speakerId}]},
    async ask(question:string,options:{history?:Turn[];store?:LiveConversationStore;sources?:boolean}={}) {
      const input:ChatInput={message:question,history:options.history??[...history],...(options.sources===false?{}:{includeSources:true})}
      const prepared=await prepareMeetingChat({labelledLines:true,noteId:initial.id,input,repository,providers:silent,signal:new AbortController().signal,emit:()=>{},liveConversations:options.store??store,onLiveThreadRebuilt:event=>{rebuilt.push(event)},
        enrich:async source=>(tail?{...source,live:{asOfMs:0,transcribedThroughMs:0,state:"live",tail}}:source) as ProviderNote})
      const context=(prepared.note as MeetingChatNote).meetingChat!
      return {prepared,input,messages:chatAnswerMessages(prepared.note,input),layout:context.layout,reason:context.reason,
        answer(text:string){prepared.commit?.(text);history.push({role:"user",content:question},{role:"assistant",content:text})}}
    },
  }
  return self
}
const text=(messages:ReadonlyArray<LanguageMessage>)=>messages.map(message=>message.content).join("\n\n")
/** What the app added before the newest question: the message that holds everything new. */
const added=(messages:ReadonlyArray<LanguageMessage>)=>messages.at(-2)!.content
/** The request `next` is `previous`, then the answer to it, then one message with what is new and the new question. */
function expectAppended(previous:ReadonlyArray<LanguageMessage>,next:ReadonlyArray<LanguageMessage>,answer:string) {
  expect(next).toHaveLength(previous.length+3)
  expect(next.slice(0,previous.length)).toEqual([...previous])
  expect(next[previous.length]).toEqual({role:"assistant",content:answer})
  expect(next.slice(-2).map(message=>message.role)).toEqual(["user","user"])
  // The same at the wire, for both kinds of provider: the serialised earlier request is a prefix of the new one.
  for(const [provider,model] of [["openrouter","openai/gpt-6-sol"],["openrouter","anthropic/claude-sonnet-5.5"],["anthropic","claude-sonnet-5-5"]] as const){
    const wire=(messages:ReadonlyArray<LanguageMessage>)=>JSON.stringify(cachedRequest(provider,model,messages,chatAnswerParameters({recordingState:"recording"} as ProviderNote).cache).messages)
    expect(wire(next).startsWith(wire(previous).slice(0,-1))).toBe(true)
  }
}

test("three turns: each request is the previous one byte for byte, the answer, and one message with what is new",async()=>{
  const m=meeting(note({body:"- Oskar hesitant about annual commit",transcriptSegments:lines.slice(0,3)}))
  m.speaking("Will do. I will send the revised","mic","mic:n:1")
  const first=await m.ask("What did Ines ask for?")
  expect(first.layout).toBe("new")
  expect(first.messages.map(message=>message.role)).toEqual(["system","user","user"])
  expect(first.messages[0]!.content).toBe(meetingChatSystemPrompt("live",true))
  // The first message: what does not change, the transcript so far, then everything about this moment, then the question.
  expect(first.messages[1]!.content).toMatch(/^<meeting>\n[\s\S]*<\/meeting>\n\n<transcript>\n\(0:00\)\n\[S1\] Me: [^\n]*\n\[S2\] Them 1: [^\n]*\n\[S3\] Them 2: [^\n]*\n<\/transcript>\n\n<typed_notes>\n\[S4\] - Oskar hesitant about annual commit\n<\/typed_notes>\n\n<unfinished>\nMe: Will do\. I will send the revised …\n<\/unfinished>\n\n<status>\n[\s\S]*<\/status>$/)
  expect(first.messages[2]).toEqual({role:"user",content:"What did Ines ask for?"})
  first.answer("A revised quote by Thursday [[S3]].")

  await m.update({transcriptSegments:lines.slice(0,4)})
  m.speaking(null)
  const second=await m.ask("How many seats?")
  expect(second.layout).toBe("append")
  expectAppended(first.messages,second.messages,"A revised quote by Thursday [[S3]].")
  // Only what was said since: the sentence that was unfinished is now a line with a label, and nothing is repeated.
  expect(added(second.messages)).toMatch(/^<transcript>\n\(1:06\)\n\[S5\] Me: Will do\. I will send the revised quote with 40 seats by Thursday\.\n<\/transcript>\n\n<status>\n[\s\S]*<\/status>$/)
  expect(added(second.messages)).not.toMatch(/<meeting>|<typed_notes>|<unfinished>|Thanks for joining/)
  expect(second.messages.at(-1)).toEqual({role:"user",content:"How many seats?"})
  second.answer("40 seats [[S5]].")

  await m.update({transcriptSegments:lines.slice(0,6),body:"- Oskar hesitant about annual commit\n- pilot 9 June"})
  m.speaking("We should also loop in")
  const third=await m.ask("When does the pilot start?")
  expect(third.layout).toBe("append")
  expectAppended(second.messages,third.messages,"40 seats [[S5]].")
  // Typed notes are sent again because they changed; the line that was already there keeps its label.
  expect(added(third.messages)).toMatch(/^<transcript>\n\(2:11\)\n\[S6\] Them 1: [^\n]*\n\[S7\] Them 2: [^\n]*\n<\/transcript>\n\n<typed_notes>\n\[S4\] - Oskar hesitant about annual commit\n\[S8\] - pilot 9 June\n<\/typed_notes>\n\n<unfinished>\nThem 1: We should also loop in …\n<\/unfinished>\n\n<status>\n/)
  // Every stored line is in the request exactly once.
  for(const line of lines.slice(0,6))expect(text(third.messages).split(line.text).length-1).toBe(1)
  // Nothing in the instructions varies: same bytes on every turn.
  expect(third.messages[0]).toEqual(first.messages[0]!)
})

test("labels are stable for the whole thread and every citation still resolves",async()=>{
  const m=meeting(note({transcriptSegments:lines.slice(0,3)}))
  const first=await m.ask("What did Ines ask for?");first.answer("A revised quote [[S3]].")
  await m.update({transcriptSegments:lines.slice(0,6)})
  const second=await m.ask("And then?");second.answer("The pilot starts on the ninth of June [[S5]].")
  await m.update({transcriptSegments:lines})
  const third=await m.ask("Who drafts the agreement?")
  // A citation issued in the first turn is the same object of facts in the third.
  for(const citation of first.prepared.citations)expect(third.prepared.citations.find(item=>item.id===citation.id)).toMatchObject({segmentId:citation.segmentId,startMs:citation.startMs,quote:citation.quote})
  expect(third.prepared.citations.map(citation=>citation.id)).toEqual(["S1","S2","S3","S4","S5","S6","S7"])
  expect(third.prepared.citations.at(-1)).toMatchObject({segmentId:lines[6]!.id,quote:lines[6]!.text})
  // The server's own check accepts all of them against the stored note.
  const stored=await Effect.runPromise(m.repository.getNote(id(1)))
  expect(await validateChatCitations(third.prepared.citations,makeMemoryRepository([stored!]),new AbortController().signal)).toHaveLength(7)
  // Each label stands in front of exactly one line of the request.
  for(const citation of third.prepared.citations)expect(text(third.messages).split(`[${citation.id}] `).length-1).toBe(1)
})

test("a client that sends only its newest turns still continues its thread, and the model keeps all of it",async()=>{
  const m=meeting(note({transcriptSegments:lines.slice(0,3)}))
  const first=await m.ask("What did Ines ask for?");first.answer("A revised quote [[S3]].")
  await m.update({transcriptSegments:lines.slice(0,4)})
  const second=await m.ask("How many seats?");second.answer("40 seats [[S4]].")
  await m.update({transcriptSegments:lines.slice(0,6)})
  const third=await m.ask("When does the pilot start?",{history:m.history.slice(-2)})
  expect(third.layout).toBe("append")
  expectAppended(second.messages,third.messages,"40 seats [[S4]].")
  expect(text(third.messages)).toContain("What did Ines ask for?")
})

test("an answer that never completed is dropped from the thread; one the client kept in part is taken on its word",async()=>{
  const m=meeting(note({transcriptSegments:lines.slice(0,3)}))
  const first=await m.ask("What did Ines ask for?");first.answer("A revised quote [[S3]].")
  await m.update({transcriptSegments:lines.slice(0,4)})
  // The second question is asked, the stream breaks, the client discards the exchange.
  const lost=await m.ask("How many seats?")
  expect(lost.layout).toBe("append")
  await m.update({transcriptSegments:lines.slice(0,6)})
  const retry=await m.ask("How many seats, again?")
  expect(retry.layout).toBe("append")
  expectAppended(first.messages,retry.messages,"A revised quote [[S3]].")
  expect(text(retry.messages)).not.toContain("How many seats?\n")
  // What the lost request showed is shown again, once, with the labels it had been given.
  expect(added(retry.messages)).toContain("[S4] Me: Will do.")
  expect(text(retry.messages).split(lines[3]!.text).length-1).toBe(1)
  // This time the client keeps the part of the answer it received, though the server never saw the stream finish.
  await m.update({transcriptSegments:lines})
  const kept=await m.ask("Who drafts it?",{history:[...m.history,{role:"user",content:"How many seats, again?"},{role:"assistant",content:"40 se"}]})
  expect(kept.layout).toBe("append")
  expectAppended(retry.messages,kept.messages,"40 se")
  // A late answer to a turn the thread has moved past changes nothing.
  lost.prepared.commit?.("Forty.")
  const after=await m.ask("Who drafts it?",{history:[...m.history,{role:"user",content:"How many seats, again?"},{role:"assistant",content:"40 se"}]})
  expect(after.messages).toEqual(kept.messages)
})

test("a relabelled voice: earlier messages stay as sent and the newest one says what the label is now",async()=>{
  const m=meeting(note({transcriptSegments:lines.slice(0,4)}))
  const first=await m.ask("What did Ines ask for?");first.answer("A revised quote [[S3]].")
  // Live transcription decides voices 1 and 2 on the call are one person (a speaker merge), then more is said.
  const merged=lines.slice(0,6).map(line=>line.speakerId==="system:n:2"?{...line,speakerId:"system:n:1",speakerLabel:"Speaker 1"}:line)
  await m.update({transcriptSegments:merged})
  const second=await m.ask("Who wants the agreement early?")
  expect(second.layout).toBe("append")
  expectAppended(first.messages,second.messages,"A revised quote [[S3]].")
  const newest=added(second.messages)
  // With one voice left on the call the label is plain `Them`; both old labels are explained in one sentence each.
  expect(newest).toContain("<corrections>\nUpdates to what earlier messages show. What is said here replaces them.\n")
  expect(newest).toContain("- Every line shown earlier as `Them 1` is the voice now labelled `Them`.")
  expect(newest).toContain("- Every line shown earlier as `Them 2` is the voice now labelled `Them`.")
  expect(newest).toMatch(/\[S5\] Them: Great\.[^\n]*\n\[S6\] Them: Legal needs/)
  // Labels and citations are untouched: the passages did not change.
  expect(second.prepared.citations.slice(0,4)).toEqual([...first.prepared.citations])
  second.answer("Legal [[S6]].")
  // Said once: the next turn has nothing to correct.
  await m.update({transcriptSegments:[...merged,{...lines[6]!}]})
  const third=await m.ask("Who drafts it?")
  expectAppended(second.messages,third.messages,"Legal [[S6]].")
  expect(added(third.messages)).not.toContain("<corrections>")
})

test("a replaced stretch (a gap filled from audio parts) and a corrected line: the old lines are withdrawn, the new ones shown",async()=>{
  const m=meeting(note({transcriptSegments:lines.slice(0,4)}))
  const first=await m.ask("What did Ines ask for?");first.answer("A revised quote by Thursday [[S3]].")
  // The durable part covering 0:05–0:25 is transcribed by the batch path: the two live segments in it are deleted
  // and one batch segment takes their place. Separately the wording of the fourth line is corrected in place.
  const batch:TranscriptSegmentDTO={id:id(2000),channel:"system",speakerId:`system:${id(900)}:speaker_1`,speakerLabel:"Speaker 1",text:"Sure. The annual commitment worries me. This is Ines from procurement, please send a revised quote by Tuesday.",isFinal:true,startMs:5000,endMs:24_000}
  const corrected={...lines[3]!,text:"Will do. I will send the revised quote with 14 seats by Thursday."}
  await m.update({transcriptSegments:[lines[0]!,batch,corrected,lines[4]!]})
  const second=await m.ask("By when?")
  expect(second.layout).toBe("append")
  expectAppended(first.messages,second.messages,"A revised quote by Thursday [[S3]].")
  const newest=added(second.messages)
  expect(newest).toContain("- No longer part of the transcript, do not use or cite: S2; S3; S4. Where that speech was transcribed again, the new lines are in <transcript> in this message, at their times.")
  // The replacements, at their own times, with labels that were never used before; then the line that is simply new.
  expect(newest).toMatch(/<transcript>\n\(0:05\)\n\[S5\] Them: Sure\. The annual commitment worries me\. This is Ines from procurement, please send a revised quote by Tuesday\.\n\(1:06\)\n\[S6\] Me: Will do\. I will send the revised quote with 14 seats by Thursday\.\n\(2:11\)\n\[S7\] Them: Great\./)
  // The withdrawn labels can no longer be cited, so a stale marker is removed from the answer instead of failing it.
  expect(second.prepared.citations.map(citation=>citation.id)).toEqual(["S1","S5","S6","S7"])
  const filter=new CitationStreamFilter(new Set(second.prepared.citations.map(citation=>citation.id)))
  expect(filter.push("By Tuesday [[S5]], not Thursday [[S3]].",true)).toBe("By Tuesday [[S5]], not Thursday .")
  // Every citation left resolves against the transcript as it is now.
  const stored=await Effect.runPromise(m.repository.getNote(id(1)))
  expect(await validateChatCitations(second.prepared.citations,makeMemoryRepository([stored!]),new AbortController().signal)).toHaveLength(4)
  second.answer("By Tuesday [[S5]].")
  const third=await m.ask("Anything else?")
  expectAppended(second.messages,third.messages,"By Tuesday [[S5]].")
  expect(added(third.messages)).not.toMatch(/<corrections>|<transcript>/)
  expect(added(third.messages)).toContain("No finished line has been added to the transcript since the previous question.")
})

test("time markers sit at the first line of every half minute of the recording, wherever a block starts",async()=>{
  // Lines every ten seconds for three minutes. One thread asks at 0:55 and again at 2:35; another asks once at 2:35.
  const steady=Array.from({length:18},(_,index)=>seg(300+index,"system",1,`Sentence number ${index}.`,index*10_000))
  const marks=(content:string)=>content.match(/^\(\d+:\d\d\)$/gm)??[]
  const m=meeting(note({transcriptSegments:steady.slice(0,6)}))
  const first=await m.ask("What has been said?")
  expect(marks(first.messages[1]!.content)).toEqual(["(0:00)","(0:30)"])
  first.answer("Six sentences.")
  await m.update({transcriptSegments:steady.slice(0,16)})
  const second=await m.ask("And since?")
  // The block starts where the previous question left off (1:00, which is also a half minute); after that the
  // markers are the half minutes of the recording, not half minutes counted from the question.
  expect(marks(added(second.messages))).toEqual(["(1:00)","(1:30)","(2:00)","(2:30)"])
  expect(added(second.messages)).toContain("(2:30)\n[S16] Them: Sentence number 15.")
  // A thread that starts later puts them in exactly the same places.
  const other=await m.ask("What has been said?",{history:[]})
  expect(marks(other.messages[1]!.content)).toEqual(["(0:00)","(0:30)","(1:00)","(1:30)","(2:00)","(2:30)"])
  // A block that starts between two half minutes begins with its own time, then falls back onto the grid.
  await m.update({transcriptSegments:steady})
  second.answer("Ten more.")
  const third=await m.ask("And now?")
  expect(marks(added(third.messages))).toEqual(["(2:40)"])
  // The status says where "now" is in recording time, which is what a time-window question counts back from.
  expect(added(third.messages)).toContain("The transcript currently ends at line S18, about 2:54 into the recording.")
  expect(m.ask("x").then(asked=>asked.messages[0]!.content)).resolves.toContain("is answered from the lines said in that stretch")
})

test("a line transcribed late is appended with its own time, not inserted into an earlier message",async()=>{
  const m=meeting(note({transcriptSegments:[lines[0]!,lines[4]!]}))
  const first=await m.ask("What has been said?");first.answer("An introduction and the pilot date [[S2]].")
  await m.update({transcriptSegments:[lines[0]!,lines[2]!,lines[4]!,lines[5]!]})
  const second=await m.ask("And now?")
  expectAppended(first.messages,second.messages,"An introduction and the pilot date [[S2]].")
  expect(added(second.messages)).toMatch(/^<transcript>\n\(0:19\)\n\[S3\] Them 2: This is Ines[^\n]*\n\(2:20\)\n\[S4\] Them 2: Legal needs/)
})

test("a stored line that was provisional when shown is shown again, citable, once it is final",async()=>{
  const m=meeting(note({transcriptSegments:[lines[0]!,{...lines[1]!,isFinal:false},lines[2]!]}))
  const first=await m.ask("What worries them?")
  expect(first.messages[1]!.content).toMatch(/\[S1\] Me: [^\n]*\nThem 1: Sure\. The annual commitment worries me[^\n]*\n\[S2\] Them 2: /)
  first.answer("The annual commitment.")
  await m.update({transcriptSegments:lines.slice(0,3)})
  const second=await m.ask("Can you cite that?")
  expectAppended(first.messages,second.messages,"The annual commitment.")
  expect(added(second.messages)).toMatch(/^<transcript>\n\(0:05\)\n\[S3\] Them 1: Sure\. The annual commitment worries me/)
  expect(added(second.messages)).toMatch(/- No longer part of the transcript, do not use or cite: the line at 0:05 that begins "Sure\. The annual commitment worries me[^"]*"\. Where that speech was transcribed again/)
  expect(second.prepared.citations.map(citation=>citation.id)).toEqual(["S1","S2","S3"])
  second.answer("Yes [[S3]].")
  const third=await m.ask("Thanks")
  expectAppended(second.messages,third.messages,"Yes [[S3]].")
  expect(added(third.messages)).not.toMatch(/<transcript>|<corrections>/)
})

test("when the thread cannot be found (the server restarted): transcript first, then the client's turns, then the question; and the next turn appends to that",async()=>{
  const m=meeting(note({transcriptSegments:lines.slice(0,3)}))
  const first=await m.ask("What did Ines ask for?");first.answer("A revised quote [[S3]].")
  await m.update({transcriptSegments:lines.slice(0,4)})
  const restarted=makeLiveConversationStore()
  const second=await m.ask("How many seats?",{store:restarted})
  expect(second.layout).toBe("rebase")
  expect(second.reason).toBe("not found: no thread with this history is remembered here (the server restarted, or the history was changed)")
  expect(second.messages.map(message=>message.role)).toEqual(["system","user","user","assistant","user","user"])
  expect(second.messages[1]!.content).toMatch(/^<meeting>\n[\s\S]*<\/meeting>\n\n<transcript>\n\(0:00\)\n\[S1\] [^\n]*\n\[S2\] [^\n]*\n\[S3\] [^\n]*\n\(1:06\)\n\[S4\] Me: Will do\.[^\n]*\n<\/transcript>$/)
  // The client's turns as it sent them, without markers from a numbering this thread does not have.
  expect(second.messages.slice(2,4)).toEqual([{role:"user",content:"What did Ines ask for?"},{role:"assistant",content:"A revised quote."}])
  expect(second.messages[4]!.content).toMatch(/^<typed_notes>\n\(the note owner has typed nothing\)\n<\/typed_notes>\n\n<status>\n[\s\S]*<\/status>$/)
  expect(second.messages[5]).toEqual({role:"user",content:"How many seats?"})
  second.answer("40 seats [[S4]].")
  // From here on it is an append-only thread again, in the new store.
  await m.update({transcriptSegments:lines.slice(0,6)})
  m.store=restarted
  const third=await m.ask("When does the pilot start?",{store:restarted})
  expect(third.layout).toBe("append")
  expectAppended(second.messages,third.messages,"40 seats [[S4]].")
  // An edited history matches nothing either, and is answered the same way.
  const edited=await m.ask("When does the pilot start?",{store:restarted,history:[{role:"user",content:"What did Ines ask for?"},{role:"assistant",content:"Something else entirely."}]})
  expect(edited.layout).toBe("rebase")
  expect(edited.messages.at(-1)).toEqual({role:"user",content:"When does the pilot start?"})
})

test("a new thread never matches an old one: no history means a fresh request",async()=>{
  const m=meeting(note({transcriptSegments:lines.slice(0,3)}))
  const first=await m.ask("What did I miss?");first.answer("Ines asked for a revised quote [[S3]].")
  await m.update({transcriptSegments:lines.slice(0,4)})
  const again=await m.ask("What did I miss?",{history:[]})
  expect(again.layout).toBe("new")
  expect(again.messages.map(message=>message.role)).toEqual(["system","user","user"])
  expect(again.messages[1]!.content).toContain("[S4] Me: Will do.")
})

test("a long transcript is cut at fixed points, so a later thread on the same meeting repeats the earlier one's messages",async()=>{
  const long=Array.from({length:60},(_,index)=>seg(100+index,index%2?"system":"mic",1,`Point ${index}: `+"the supplier confirmed the delivery window and nobody objected to it. ".repeat(6),index*20_000))
  const m=meeting(note({transcriptSegments:long.slice(0,40)}))
  const first=await m.ask("What did I miss?")
  const base=first.messages.slice(1,-2)
  // Several complete pieces, each a message of its own; the last complete one carries the cache breakpoint.
  expect(base.length).toBeGreaterThanOrEqual(2)
  expect(base.every(message=>message.role==="user"&&/^(?:<meeting>\n[\s\S]*<\/meeting>\n\n)?<transcript>\n\(\d+:\d\d\)\n[\s\S]*\n<\/transcript>$/.test(message.content))).toBe(true)
  expect(base.map(message=>!!message.cache)).toEqual([...base.slice(0,-1).map(()=>false),true])
  expect(added(first.messages)).toMatch(/^<transcript>\n[\s\S]*<\/transcript>\n\n<typed_notes>/)
  first.answer("Delivery windows were confirmed [[S1]].")
  // Twenty more utterances, then a new thread (a one-tap recipe sends no history).
  await m.update({transcriptSegments:long})
  const second=await m.ask("What did I miss?",{history:[]})
  expect(second.layout).toBe("new")
  expect(second.messages.length).toBeGreaterThan(first.messages.length)
  // Everything up to the earlier breakpoint is identical, text and labels, so the provider serves it from cache.
  expect(second.messages.slice(0,base.length+1).map(message=>message.content)).toEqual(first.messages.slice(0,base.length+1).map(message=>message.content))
  // And the thread itself still only appends.
  const next=await m.ask("And since then?")
  expectAppended(first.messages,next.messages,"Delivery windows were confirmed [[S1]].")
  expect(added(next.messages)).toContain("Point 59:")
  // The single-document view used by tools and tests shows one transcript.
  const document=meetingChatDocument(next.messages)
  expect(document.match(/<transcript>/g)).toHaveLength(1)
  for(const line of long)expect(document.split(line.text.slice(0,12)).length-1).toBe(1)
  expect(document).toContain("<status>")
})

test("when most of what was shown has changed, or the thread outgrows its budget, it is started again with the client's turns kept",async()=>{
  const many=Array.from({length:80},(_,index)=>seg(200+index,"system",1,`Routine item ${index} was read out and accepted.`,index*5000))
  const m=meeting(note({transcriptSegments:many}))
  const first=await m.ask("What has been decided?");first.answer("Routine items were accepted [[S1]].")
  // A whole-meeting re-transcription: every stored segment is replaced.
  await m.update({transcriptSegments:many.map((line,index)=>({...line,id:id(5000+index),text:line.text.replace("accepted","approved")}))})
  const second=await m.ask("And now?")
  expect(second.layout).toBe("rebase")
  expect(second.reason).toBe("too many corrections: 80 lines already shown have changed (limit 40)")
  expect(second.messages.at(-4)).toEqual({role:"user",content:"What has been decided?"})
  expect(second.messages.at(-3)).toEqual({role:"assistant",content:"Routine items were accepted."})
  expect(second.messages[1]!.content).toContain("Routine item 0 was read out and approved.")
  expect(second.messages[1]!.content).not.toContain("accepted")
  expect(second.prepared.citations.every(citation=>citation.quote.includes("approved"))).toBe(true)
  second.answer("They were approved [[S2]].")
  // Budget: a thread that would pass the request limit is started again rather than refused.
  const heavy=Array.from({length:100},(_,index)=>seg(9000+index,"system",1,`Long reading ${index}. `+"x ".repeat(2250),1_000_000+index*30_000))
  const stored=(await Effect.runPromise(m.repository.getNote(id(1))))!.transcriptSegments
  await m.update({transcriptSegments:[...stored,...heavy.slice(0,60)]})
  const third=await m.ask("Still going?")
  expect(third.layout).toBe("append")
  third.answer("Yes.")
  await m.update({transcriptSegments:[...stored,...heavy]})
  const fourth=await m.ask("And still?")
  expect(fourth.layout).toBe("rebase")
  expect(fourth.reason).toBe("over budget: the request would pass 420000 characters")
  expect(text(fourth.messages).length).toBeLessThan(MEETING_CHAT_LIMITS.liveCharacters)
  expect(fourth.prepared.requestCharacters).toBe(fourth.messages.reduce((sum,message)=>sum+message.content.length,0))
})

test("every rebuilt thread is reported with its reason: not found, evicted, over budget, too many corrections",async()=>{
  // A store that keeps two threads per note and forgets after an hour, with a clock the test moves.
  let now=0
  const store=makeLiveConversationStore({conversations:3,perNote:2,characters:1_000_000,idleMs:3_600_000,resumableTurns:2},()=>now)
  const m=meeting(note({transcriptSegments:lines.slice(0,3)}),store)
  const thread=async(question:string,answer:string)=>{const asked=await m.ask(question,{history:[]});asked.prepared.commit?.(answer);return [{role:"user" as const,content:question},{role:"assistant" as const,content:answer}]}
  const first=await thread("What did Ines ask for?","A revised quote [[S3]]."),second=await thread("Who is worried?","The first caller [[S2]].")
  // New threads and continued threads are not reported.
  expect((await m.ask("And by when?",{history:first})).layout).toBe("append")
  expect(m.rebuilt).toEqual([])
  // A third thread on the note pushes out the one used longest ago.
  await thread("What is this meeting about?","Tiered pricing [[S1]].")
  expect((await m.ask("Why?",{history:second})).layout).toBe("rebase")
  expect(m.rebuilt.at(-1)).toMatchObject({noteId:id(1),reason:"evicted: more than 2 threads on this note",historyMessages:2,shownSegments:3})
  expect(m.rebuilt.at(-1)!.requestCharacters).toBeGreaterThan(10_000)
  // Idle for longer than the store keeps a thread.
  const idle=await thread("How many voices are there?","Three.")
  now+=2*3_600_000
  expect((await m.ask("Which ones?",{history:idle})).reason).toBe("evicted: idle for more than 1 hours")
  // A history nobody here has seen, and one that does not end with an answer.
  expect((await m.ask("And?",{history:[{role:"user",content:"Never asked"},{role:"assistant",content:"Never answered"}]})).reason).toBe("not found: no thread with this history is remembered here (the server restarted, or the history was changed)")
  expect((await m.ask("And?",{history:[{role:"user",content:"Dangling question"}]})).reason).toBe("not found: the history does not end with a question and its answer")
  // Continued from a turn the store no longer keeps the state for (two turns here).
  const long=makeLiveConversationStore({conversations:3,perNote:2,characters:1_000_000,idleMs:3_600_000,resumableTurns:2}),n=meeting(note({transcriptSegments:lines.slice(0,3)}),long)
  for(const [question,answer] of [["One?","First."],["Two?","Second."],["Three?","Third."],["Four?","Fourth."]] as const)(await n.ask(question)).answer(answer)
  expect((await n.ask("Again?",{history:n.history.slice(0,2)})).reason).toBe("not found: continued from a turn older than the last 2")
  expect(n.rebuilt).toHaveLength(1)
  expect(m.rebuilt.map(event=>event.reason.split(":")[0])).toEqual(["evicted","evicted","not found","not found"])
})

test("without sources there are no labels, and the thread still only appends",async()=>{
  const m=meeting(note({transcriptSegments:lines.slice(0,3)}))
  const first=await m.ask("Summarise so far",{sources:false})
  expect(first.messages[0]!.content).toContain("Do not add source markers")
  expect(first.messages[1]!.content).not.toMatch(/\[S\d+\]/)
  first.answer("Pricing and a quote.")
  await m.update({transcriptSegments:lines.slice(0,4)})
  const second=await m.ask("And now?",{sources:false})
  expectAppended(first.messages,second.messages,"Pricing and a quote.")
  expect(second.prepared.citations).toEqual([])
  // Asking for sources is a different thread: its request has labels from the start.
  const cited=await m.ask("And now?")
  expect(cited.layout).toBe("rebase")
})

test("through the endpoint: the answer the client received is what the next request repeats",async()=>{
  const stored=note({transcriptSegments:lines.slice(0,3),transcriptRevision:3}),repository=makeMemoryRepository([stored]),requests:LanguageMessage[][]=[]
  const answers=["Ines asked for a revised quote [[S3]] [[S99]].","Forty [[S4]]."]
  const providers:ProviderServices={...silent,async *chatStream(prepared,input){requests.push(chatAnswerMessages(prepared,input));yield answers[requests.length-1]!}}
  const processing=makeProcessingService(repository,{labelledNoteChat:true,providers,liveConversations:makeLiveConversationStore()})
  const run=async(input:ChatInput)=>{const events:ChatStreamEventDTO[]=[];await Effect.runPromise(Stream.runForEach(await Effect.runPromise(processing.chatStream(stored.id,input)),event=>Effect.sync(()=>{events.push(event)})));return events.filter(event=>event.type==="delta").map(event=>event.text).join("")}
  const first=await run({message:"What did Ines ask for?",history:[],includeSources:true})
  // A marker the server did not issue never reaches the client, and so is not in the thread either.
  expect(first).toBe("Ines asked for a revised quote [[S3]] .")
  await Effect.runPromise(repository.updateNote(stored.id,{transcriptSegments:lines.slice(0,4)}))
  await run({message:"How many seats?",history:[{role:"user",content:"What did Ines ask for?"},{role:"assistant",content:first}],includeSources:true})
  await processing.dispose()
  expectAppended(requests[0]!,requests[1]!,first)
})
