/**
 * The read-only retrieval session behind every chat request: owner-scoped tool
 * execution, discovered/read bookkeeping, the character budget, progress events
 * and server-issued citations. Feature modules decide what to do with it:
 * `note-chat.ts` and `live-chat.ts` read the one open note, `workspace-chat.ts`
 * runs the planner loop.
 *
 * Which parts of a note reach the model is a `ContextSelector`. Every feature
 * currently uses `selectBoundedContext`; a feature that needs different context
 * supplies its own selector from its own module instead of editing this one.
 */
import {Effect} from "effect"
import type {ChatCitationDTO,ChatInput,ChatStreamEventDTO,ChatToolProgressDTO,NoteDTO} from "@meeting-notes/contracts"
import type {NotesRepositoryService} from "../repository"
import {meetingSearchTerms,selectMeetings,selectTranscriptPassages,transcriptPassage,type MeetingSearchInput} from "../meeting-search"
import type {ProviderNote,ProviderServices} from "../providers"
import type {LiveConversationStore} from "./live-conversation"
import {forContext} from "../transcript-link"

const object=(value:unknown):value is Record<string,unknown>=>!!value&&typeof value==="object"&&!Array.isArray(value)
const uuid=/^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i
const date=(value:unknown):value is string=>typeof value==="string"&&/^\d{4}-\d{2}-\d{2}$/.test(value)&&Number.isFinite(Date.parse(value))&&new Date(value).toISOString().startsWith(value)
export interface PreparedChat {note:ProviderNote;citations:ReadonlyArray<ChatCitationDTO>;
  /** Size of the request as it will be sent, when the feature knows it; otherwise the note is measured in its JSON form. */
  requestCharacters?:number
  /** Called with the answer once it has been streamed, by a feature that remembers its threads (chat during a recording). */
  commit?:(answer:string)=>void}
export interface MeetingChatOptions {
  noteId:string|null;input:ChatInput;repository:NotesRepositoryService;providers:ProviderServices;
  enrich:(note:NoteDTO)=>Promise<ProviderNote>;signal:AbortSignal;emit:(event:ChatStreamEventDTO)=>void;now?:Date;
  /** A chat about one note with a source label on every line and instructions to cite them. No endpoint sets
   * this: such a chat shows no sources. The form is kept, and tested, because thread layout, label stability
   * and the citation checks it exercises are shared with the form that is sent. */
  labelledLines?:boolean;
  /** Internal resource-limit seam; callers may lower but never raise the cap. */
  readCharacterBudget?:number;
  /** Chat during a recording: where threads are remembered (default: the process-wide store) and whose they are. */
  liveConversations?:LiveConversationStore;conversationScope?:string;
  /** Told when a live chat thread could not be continued and was rebuilt, and why. Default: a server log line. */
  onLiveThreadRebuilt?:(event:{noteId:string;reason:string;historyMessages:number;shownSegments:number;requestCharacters:number})=>void;
}
export type AddCitation=(note:ProviderNote,source:ChatCitationDTO["source"],quote:string,segment?:NoteDTO["transcriptSegments"][number])=>void
/** What a selector may use: the request (question, history) and the session's
 * citation issuer. Only passages passed to `addCitation` become citable. */
export interface ContextSelection {input:ChatInput;addCitation:AddCitation}
/** Reduce a full note to the context the model will see. `around` is set when a
 * read was requested near a transcript timestamp. */
export type ContextSelector=(note:ProviderNote,around:number|undefined,selection:ContextSelection)=>ProviderNote

/** Lexical, bounded selection: up to 12 transcript segments ranked by how many
 * question terms they contain (meeting order breaks ties), each cut to a
 * 2,000-character window, plus 6,000/8,000-character windows of the typed notes
 * and the generated document. Issues citations for the first six segments and
 * the two best paragraphs of each document. */
export function selectBoundedContext(note:ProviderNote,around:number|undefined,{input,addCitation}:ContextSelection):ProviderNote {
  const terms=meetingSearchTerms(input.message),score=(text:string)=>terms.filter(term=>text.toLowerCase().includes(term)).length
  const segments=[...note.transcriptSegments].sort((a,b)=>around===undefined?score(b.text)-score(a.text)||a.startMs-b.startMs:Math.abs(a.startMs-around)-Math.abs(b.startMs-around))
    .slice(0,12).map(segment=>({...segment,text:transcriptPassage(segment.text,terms,2000)})).sort((a,b)=>a.startMs-b.startMs)
  const result={...note,body:transcriptPassage(note.body,terms,6000),enhanced:transcriptPassage(note.enhanced,terms,8000),transcriptSegments:segments,retrievalCoverage:{bodyTruncated:note.body.length>6000,summaryTruncated:note.enhanced.length>8000,transcriptTruncated:note.transcriptSegments.length>segments.length||note.transcriptSegments.some(s=>s.text.length>2000)}}
  for(const segment of segments.slice(0,6))addCitation(note,"transcript",transcriptPassage(segment.text,terms,1200),segment)
  // These are exact source substrings. Source choice is explicit so a summary
  // sentence cannot masquerade as a verbatim transcript quotation.
  for(const [source,text]of[["notes",result.body],["summary",result.enhanced]] as const){
    const passages=text.split(/\n\s*\n/).filter(line=>line.trim()).sort((a,b)=>score(b)-score(a)).slice(0,2)
    for(const passage of passages)addCitation(note,source,transcriptPassage(passage,terms,1200))
  }
  return result
}

export function createRetrievalSession(options:MeetingChatOptions,select:ContextSelector) {
  const {input,repository,providers,signal}=options,now=options.now??new Date()
  if(input.timeZone) new Intl.DateTimeFormat("en-US",{timeZone:input.timeZone}).format(now)
  const timeZone=input.timeZone??"UTC",today=new Intl.DateTimeFormat("en-CA",{timeZone,year:"numeric",month:"2-digit",day:"2-digit"}).format(now)
  const issued:ChatCitationDTO[]=[],read=new Map<string,ProviderNote>(),discovered=new Set<string>()
  const readBudget=Math.max(0,Math.min(160_000,options.readCharacterBudget??160_000))
  const run=<A>(effect:Effect.Effect<A,unknown>)=>Effect.runPromise(effect,{signal})
  let calls=0,readCharacters=0
  const emit=(event:ChatStreamEventDTO)=>{if(input.includeSources&&!signal.aborted)options.emit(event)}
  const progress=(callId:string,name:ChatToolProgressDTO["name"],status:ChatToolProgressDTO["status"],label:string,notes?:ReadonlyArray<NoteDTO>)=>emit({type:"tool",text:"",tool:{callId,name,status,label,...(notes?{resultCount:notes.length,results:notes.map(note=>({noteId:note.id,title:note.title,date:note.date}))}:{})}})
  function addCitation(note:ProviderNote,source:ChatCitationDTO["source"],quote:string,segment?:NoteDTO["transcriptSegments"][number]) {
    if(!input.includeSources||!quote.trim()||issued.length>=40)return
    if(issued.some(c=>c.noteId===note.id&&c.source===source&&c.segmentId===(segment?.id??null)&&c.quote===quote))return
    issued.push({id:`S${issued.length+1}`,noteId:note.id,title:note.title,source,segmentId:segment?.id??null,startMs:segment?.startMs??null,quote,transcriptRevision:note.transcriptRevision})
  }
  async function readNotes(ids:ReadonlyArray<string>,around?:number) {
    const result:ProviderNote[]=[]
    for(const id of ids){
      signal.throwIfAborted()
      if(options.noteId!==null&&id!==options.noteId)continue
      if(options.noteId===null&&!discovered.has(id))continue
      // Read without the chat link line of the generated notes (transcript-link.ts).
      const stored=await run(repository.getNote(id)),source=stored&&forContext(stored)
      if(!source)continue
      const citationCount=issued.length
      const note=select(await options.enrich(source),around,{input,addCitation})
      const size=JSON.stringify(note).length,old=read.get(id),oldSize=old?JSON.stringify(old).length:0
      if(readCharacters-oldSize+size>readBudget){issued.splice(citationCount);continue}
      read.set(id,note);readCharacters=readCharacters-oldSize+size
      result.push(note)
    }
    return result
  }
  async function execute(name:ChatToolProgressDTO["name"],raw:unknown) {
    signal.throwIfAborted()
    if(++calls>8)throw new Error("Meeting retrieval budget exhausted")
    const callId=crypto.randomUUID(),label=name==="search_meetings"?"Searching meetings":name==="search_transcripts"?"Searching transcript passages":name==="list_meetings"?"Finding recent meetings":name==="get_meetings"?"Reading meeting notes":"Reading transcript passages"
    progress(callId,name,"running",label)
    try {
      if(!object(raw))throw new Error("Invalid tool arguments")
      let notes:ReadonlyArray<NoteDTO>
      if(name==="search_meetings"||name==="list_meetings"||name==="search_transcripts"){
        if(Object.keys(raw).some(key=>!["query","from","to","limit"].includes(key)))throw new Error("Invalid tool arguments")
        if(raw.query!==undefined&&(typeof raw.query!=="string"||raw.query.length>300)||raw.from!==undefined&&!date(raw.from)||raw.to!==undefined&&!date(raw.to)||raw.limit!==undefined&&(!Number.isInteger(raw.limit)||Number(raw.limit)<1||Number(raw.limit)>20))throw new Error("Invalid search arguments")
        const search:MeetingSearchInput={query:name==="list_meetings"?"":String(raw.query??""),limit:Number(raw.limit??12),...(raw.from?{from:String(raw.from)}:{}),...(raw.to?{to:String(raw.to)}:{})}
        if(search.from&&search.to&&search.from>search.to)throw new Error("Invalid date range")
        if(name==="search_transcripts"){
          const query={...search,...(options.noteId?{noteId:options.noteId}:{})}
          const hits=repository.searchTranscriptSegments?await run(repository.searchTranscriptSegments(query)):selectTranscriptPassages(await run(repository.listNotes()),query)
          const touched:ProviderNote[]=[],retained:typeof hits[number][]=[]
          for(const hit of hits){
            const old=read.get(hit.noteId)
            const note:ProviderNote=old??{id:hit.noteId,title:hit.title,date:hit.date,body:"",enhanced:"",transcriptSegments:[],transcriptRevision:hit.transcriptRevision,generatedRevision:null,recordingState:"idle",generationState:"idle",duration:"",createdAt:now.toISOString(),updatedAt:now.toISOString(),retrievalCoverage:{bodyTruncated:true,summaryTruncated:true,transcriptTruncated:true}}
            const segments=note.transcriptSegments.some(segment=>segment.id===hit.segment.id)?note.transcriptSegments:[...note.transcriptSegments,hit.segment]
            const updated={...note,transcriptSegments:segments},oldSize=old?JSON.stringify(old).length:0,size=JSON.stringify(updated).length
            if(readCharacters-oldSize+size>readBudget)continue
            read.set(hit.noteId,updated);readCharacters=readCharacters-oldSize+size;discovered.add(hit.noteId)
            retained.push(hit)
            addCitation(updated,"transcript",transcriptPassage(hit.segment.text,meetingSearchTerms(search.query),1200),hit.segment)
            if(!touched.some(note=>note.id===hit.noteId))touched.push(updated)
          }
          const label=retained.length===hits.length
            ? `Found ${retained.length} transcript passage${retained.length===1?"":"s"} in ${touched.length} meeting${touched.length===1?"":"s"}`
            : `Read ${retained.length} of ${hits.length} transcript passages in ${touched.length} meetings (context limit)`
          emit({type:"tool",text:"",tool:{callId,name,status:"completed",label,resultCount:retained.length,results:touched.map(note=>({noteId:note.id,title:note.title,date:note.date}))}})
          return retained.map(hit=>({noteId:hit.noteId,title:hit.title,date:hit.date,segmentId:hit.segment.id,startMs:hit.segment.startMs,quote:hit.segment.text,transcriptRevision:hit.transcriptRevision,sources:issued.filter(source=>source.noteId===hit.noteId&&source.segmentId===hit.segment.id)}))
        }
        notes=(repository.searchNotes?await run(repository.searchNotes(search)):selectMeetings(await run(repository.listNotes()),search)).map(forContext)
        for(const note of notes)discovered.add(note.id)
        progress(callId,name,"completed",`Found ${notes.length} meeting${notes.length===1?"":"s"}`,notes)
        return notes.map(note=>({noteId:note.id,title:note.title,date:note.date,hasSummary:!!note.enhanced.trim(),hasTranscript:note.transcriptRevision>0}))
      }
      const allowed=name==="get_meetings"?["noteIds"]:["noteId","startMs"]
      if(Object.keys(raw).some(key=>!allowed.includes(key)))throw new Error("Invalid read arguments")
      const ids=name==="get_meetings"?raw.noteIds:[raw.noteId]
      if(!Array.isArray(ids)||!ids.length||ids.length>4||!ids.every(id=>typeof id==="string"&&uuid.test(id)))throw new Error("Invalid note IDs")
      if(raw.startMs!==undefined&&(!Number.isInteger(raw.startMs)||Number(raw.startMs)<0||Number(raw.startMs)>2147483647))throw new Error("Invalid timestamp")
      notes=await readNotes([...new Set(ids as string[])],raw.startMs===undefined?undefined:Number(raw.startMs))
      progress(callId,name,"completed",`Looked deeper into ${notes.length} note${notes.length===1?"":"s"}`,notes)
      return (notes as ReadonlyArray<ProviderNote>).map(note=>({noteId:note.id,title:note.title,date:note.date,body:note.body,summary:note.enhanced,transcript:note.transcriptSegments,retrievalCoverage:note.retrievalCoverage,sources:issued.filter(c=>c.noteId===note.id)}))
    } catch(error){
      progress(callId,name,"failed","Couldn't finish this lookup")
      signal.throwIfAborted()
      return {error:"Meeting lookup could not be completed. Only account-owned discovered meeting IDs are readable."}
    }
  }
  return {input,providers,signal,now,timeZone,today,issued,read,discovered,progress,execute,
    /** The next piece of the model's own reasoning, as the provider gave it. Sent whether or not sources were asked for. */
    reasoning:(text:string)=>{if(text&&!signal.aborted)options.emit({type:"reasoning",text})},
    /** Tool operations executed so far, against the limit of 8 per request. */
    callCount:()=>calls,
    /** The caller's scoped repository, note enrichment, event sink and read
     * budget, for a feature that runs its own retrieval (`workspace-tools.ts`). */
    repository,enrich:options.enrich,emit,readBudget}
}
export type RetrievalSession=ReturnType<typeof createRetrievalSession>
