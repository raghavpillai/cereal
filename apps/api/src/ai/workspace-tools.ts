/**
 * Retrieval for chat across meetings: what each planner tool actually does, how
 * its result is written for the model, and how everything that was retrieved
 * becomes the answer's context and its citable sources.
 *
 * `workspace-chat.ts` owns the prompts, the tool descriptions and the loop; this
 * file owns the mechanics. Design points, each of them measured:
 *
 * - Results are plain tagged text, not JSON, and a passage is never sent twice
 *   inside one result.
 * - Meetings are named by short aliases (m1, m2, …). The model never sees a
 *   database ID, so it cannot leak or forge one; an alias resolves only to a
 *   meeting this request discovered through the caller's own scoped repository.
 * - The repository's keyword search is "any term, newest first, at most 20".
 *   That buries an old meeting behind newer ones that share a common word, so
 *   each term is searched separately and the results are fused and ranked here
 *   (rare terms and title/notes matches weigh more), for meetings and for
 *   transcript passages alike.
 * - Notes and summaries are read first and are cheap; a transcript is returned
 *   only when asked for, or when a meeting has nothing else.
 * - Source IDs ([[S1]]) are allocated once, at the end, over what the answer
 *   will actually see, so the budget of 40 goes to the passages that matter and
 *   every meeting that was read stays citable.
 */
import {Effect} from "effect"
import type {ChatCitationDTO,ChatToolProgressDTO,NoteDTO} from "@meeting-notes/contracts"
import {meetingSearchTerms,selectMeetings,selectTranscriptPassages} from "../meeting-search"
import type {ProviderNote} from "../providers"
import type {RetrievalSession} from "./retrieval"
import {forContext} from "../transcript-link"

type Segment=NoteDTO["transcriptSegments"][number]
type ToolName=ChatToolProgressDTO["name"]
export interface ToolOutcome {
  /** What the model reads as the tool result. */
  text:string
  /** The call asked to answer straight after it, and it returned something. */
  final:boolean
  /** One line saying what was done and what came back, as the user saw it. */
  step:string
}
interface Meeting {alias:string;id:string;row:NoteDTO;full?:ProviderNote|null}
interface Span {start:number;end:number}
interface Shown {segment:Segment;span:Span;score:number}
interface Evidence {meeting:Meeting;summary?:Span;notes?:Span;segments:Map<string,Shown>}

/** Characters of tool results one request may place in front of the model. */
export const MAX_RESULT_CHARACTERS=120_000
export const MAX_TOOL_CALLS=8
const MAX_CITATIONS=40,REPOSITORY_PAGE=20
const UUID=/^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i
const WEEKDAYS=["Sun","Mon","Tue","Wed","Thu","Fri","Sat"]
// Words that say nothing about which meeting is meant. The shared tokenizer
// already drops question words; these are the ones it keeps.
const STOP=new Set("the and for are was were did does has had have not but our out who why how its it's i'm we've you your yours his her him she he they them their there here this that these those then than with without from into onto over under about after before during since until while also just only very more most some such any all each every both either neither will would could should can may might must shall being been be am is do done doing say said says tell told ask asked asks want wanted wants need needed needs get got give gave make made take took go went come came know knew think thought mention mentioned discuss discussed discussion talk talked talking speak spoke spoken decide decided decision decisions agree agreed happen happened happening thing things stuff anything something everything nothing one two three first second last latest recent recently time times day days week weeks month months year years today yesterday tomorrow ago me my mine us we im ive on in at to of or as an by if so up no yes ok okay what when where which whom whose across again ever never still yet already now new old us end ended ending up down originally eventually finally actually really".split(" "))
const isDate=(value:unknown):value is string=>typeof value==="string"&&/^\d{4}-\d{2}-\d{2}$/.test(value)&&Number.isFinite(Date.parse(value))&&new Date(value).toISOString().startsWith(value)
const clock=(ms:number)=>{const total=Math.max(0,Math.floor(ms/1000)),hours=Math.floor(total/3600),minutes=Math.floor(total%3600/60),seconds=String(total%60).padStart(2,"0");return hours?`${hours}:${String(minutes).padStart(2,"0")}:${seconds}`:`${minutes}:${seconds}`}
export const dayLabel=(date:string)=>`${date} ${WEEKDAYS[new Date(`${date}T00:00:00Z`).getUTCDay()]??""}`.trim()
const attribute=(value:string)=>value.replace(/["<>\n\r]+/g," ").replace(/\s+/g," ").trim()
const plural=(count:number,noun:string)=>`${count} ${noun}${count===1?"":"s"}`
/** Distinctive lower-case search words of a query, in the tokenization the
 * repositories use, minus words that would match every meeting. */
export function searchWords(query:string):string[] {
  const words=distinctiveWords(query).map(stem).filter(term=>!STOP.has(term))
  return [...new Set(words)].filter(term=>{const again=meetingSearchTerms(term);return again.length===1&&again[0]===term})
}
/** The same words as the user wrote them, for showing what is being searched. */
export const distinctiveWords=(query:string):string[]=>meetingSearchTerms(query).filter(term=>!term.includes(" ")&&!STOP.has(term)&&!STOP.has(stem(term)))
/** Matching is by word prefix, so a longer word is searched by its stem:
 * "objected" finds "object" and "objection", "pricing" finds "price". Only
 * plain inflections are removed, and never down to fewer than four letters. */
function stem(term:string):string {
  if(term.length<6||/[\d'’-]/.test(term))return term
  const cut=term.replace(/(?:ing|ed|es|s)$/,"")
  return cut.length>=4?cut:term
}
const wordPatterns=new Map<string,RegExp>()
/** Whether a word, or a word beginning with the term, occurs in the text. The
 * repositories match substrings ("opening" inside "reopening"); ranking counts
 * only matches at the start of a word. */
export function containsWord(text:string,term:string):boolean {
  let pattern=wordPatterns.get(term)
  if(!pattern){pattern=new RegExp(`(?<![\\p{L}\\p{N}])${term.replace(/[.*+?^${}()|[\]\\]/g,"\\$&")}`,"iu");if(wordPatterns.size<5000)wordPatterns.set(term,pattern)}
  return pattern.test(text)
}
/** The window of `text` of at most `maximum` characters that holds the most
 * distinct terms, snapped to whitespace so it reads as an excerpt and remains
 * an exact substring. */
export function bestWindow(text:string,terms:ReadonlyArray<string>,maximum:number):Span {
  if(text.length<=maximum)return trimSpan(text,{start:0,end:text.length})
  const lower=text.toLowerCase(),starts=[0]
  for(const term of terms){let from=0;for(let count=0;count<12;count++){const at=lower.indexOf(term,from);if(at<0)break;starts.push(Math.max(0,at-Math.floor(maximum/3)));from=at+term.length}}
  let best=0,bestScore=-1
  for(const start of starts){
    const slice=lower.slice(start,start+maximum),score=terms.reduce((sum,term)=>sum+(slice.includes(term)?1+Math.min(3,slice.split(term).length-1)*0.1:0),0)
    if(score>bestScore||score===bestScore&&start<best){best=start;bestScore=score}
  }
  let start=best,end=Math.min(text.length,best+maximum)
  if(start>0){const space=text.slice(start,start+80).search(/\s/);if(space>=0)start+=space+1}
  if(end<text.length){const space=text.slice(end-80,end).search(/\s\S*$/);if(space>=0)end=end-80+space}
  return trimSpan(text,{start,end:Math.max(start,end)})
}
function trimSpan(text:string,span:Span):Span {
  let {start,end}=span
  while(start<end&&/\s/.test(text[start]!))start++
  while(end>start&&/\s/.test(text[end-1]!))end--
  return {start,end}
}
const union=(a:Span|undefined,b:Span):Span=>a?{start:Math.min(a.start,b.start),end:Math.max(a.end,b.end)}:b
/** Paragraph blocks of a span of a document (a heading stays with the lines under it). */
function blocks(text:string,span:Span):Span[] {
  const result:Span[]=[],body=text.slice(span.start,span.end),separator=/\n[ \t]*\n+/g
  let from=0,match:RegExpExecArray|null
  const push=(a:number,b:number)=>{const trimmed=trimSpan(text,{start:span.start+a,end:span.start+b});if(trimmed.end>trimmed.start)result.push(trimmed)}
  while((match=separator.exec(body))){push(from,match.index);from=match.index+match[0].length}
  push(from,body.length)
  // A heading on its own paragraph belongs with what follows it.
  for(let index=result.length-2;index>=0;index--)if(/^#{1,6}\s[^\n]*$/.test(text.slice(result[index]!.start,result[index]!.end))){result[index]={start:result[index]!.start,end:result[index+1]!.end};result.splice(index+1,1)}
  return result
}

/** A progress step as the client saw it: the fields of a `tool` stream event. */
export interface CoverageStep {name:string;status:string;label:string;resultCount?:number}
/**
 * The closing "Coverage note" of an answer: what was searched and read, and
 * whether transcripts were consulted. It is a pure function of the progress
 * steps that were emitted, so it cannot claim a lookup that did not run; the
 * evaluation recomputes it from the stream's `tool` events and compares.
 */
export function coverageNote(steps:ReadonlyArray<CoverageStep>):string {
  const done:Array<{name:string;doing:string;label:string;count:number}>=[]
  let doing="",failed=0
  for(const step of steps){
    if(step.status==="running")doing=step.label
    else if(step.status==="completed"){done.push({name:step.name,doing,label:step.label,count:step.resultCount??0});doing=""}
    else if(step.status==="failed"){failed++;doing=""}
  }
  if(!done.length)return failed?"The meeting lookup failed, so nothing was searched or read.":""
  const lower=(text:string)=>text.charAt(0).toLowerCase()+text.slice(1),when=(text:string)=>/ (?:from|on|since|up to) \d{4}-\d{2}-\d{2}.*$/.exec(text)?.[0]??""
  const phrase=(step:typeof done[number])=>
    step.name==="search_meetings"?`${lower(step.doing).replace(/^searching meetings/,"searched your meetings")} (${step.count} found)`
    :step.name==="list_meetings"?`listed ${plural(step.count,"meeting")}${when(step.doing)}`
    :step.name==="get_meetings"?`${lower(step.label)}${when(step.doing)}`
    :step.name==="search_transcripts"?`${lower(step.doing).replace(/^searching/,"searched")} (${lower(step.label).replace(/^found /,"")})`
    :lower(step.label)
  const heard=done.some(step=>(step.name==="search_transcripts"||step.name==="get_meeting_transcript")&&step.count>0)
  const sum=(name:string)=>done.filter(step=>step.name===name).reduce((total,step)=>total+step.count,0),times=(name:string)=>done.filter(step=>step.name===name).length
  const parts=done.length<=3?done.map(phrase):[
    times("search_meetings")?`searched your meetings ${times("search_meetings")===1?"once":`${times("search_meetings")} times`}`:"",
    times("list_meetings")?`listed ${plural(sum("list_meetings"),"meeting")}`:"",
    times("get_meetings")?`looked deeper into ${plural(sum("get_meetings"),"note")}`:"",
    times("search_transcripts")?`searched transcripts ${times("search_transcripts")===1?"once":`${times("search_transcripts")} times`} (${plural(sum("search_transcripts"),"passage")})`:"",
    times("get_meeting_transcript")?`read ${plural(sum("get_meeting_transcript"),"transcript")}`:"",
  ].filter(Boolean)
  const periods=done.length<=3?[]:[...new Set(done.filter(step=>step.name==="get_meetings"||step.name==="list_meetings").map(step=>when(step.doing).trim()).filter(Boolean))]
  const text=parts.join("; ")+(periods.length?` (${periods.join("; ")})`:"")
  return `${text.charAt(0).toUpperCase()}${text.slice(1)}.${heard?"":" Transcripts were not consulted."}${failed?` ${failed===1?"One lookup":`${failed} lookups`} failed.`:""}`
}

export function createWorkspaceResearch(session:RetrievalSession) {
  const {input,signal,repository,enrich,emit,today}=session
  const run=<A>(effect:Effect.Effect<A,unknown>)=>Effect.runPromise(effect,{signal})
  const budget=Math.max(0,Math.min(session.readBudget,MAX_RESULT_CHARACTERS))
  const meetings=new Map<string,Meeting>(),aliases=new Map<string,Meeting>(),evidence=new Map<string,Evidence>(),listed:Meeting[]=[]
  /** Every search word used in this request, to rank which transcript lines get source IDs. */
  const focus=new Set<string>(searchWords(input.message))
  const notes:string[]=[],shownSteps:CoverageStep[]=[]
  // Sources another module issued before this research began keep their IDs.
  const issuedBefore=session.issued.length
  let spent=0,calls=0

  // ------------------------------------------------------------ repository
  const searchRows=async(query:string,from?:string,to?:string):Promise<ReadonlyArray<NoteDTO>>=>{
    const search={query,limit:REPOSITORY_PAGE,...(from?{from}:{}),...(to?{to}:{})}
    // Notes are read without the chat link line of their generated notes (transcript-link.ts).
    return (repository.searchNotes?await run(repository.searchNotes(search)):selectMeetings(await run(repository.listNotes()),search)).map(forContext)
  }
  const searchSegments=async(query:string,options:{noteId?:string;from?:string;to?:string})=>{
    const search={query,limit:REPOSITORY_PAGE,...(options.noteId?{noteId:options.noteId}:{}),...(options.from?{from:options.from}:{}),...(options.to?{to:options.to}:{})}
    return repository.searchTranscriptSegments?run(repository.searchTranscriptSegments(search)):selectTranscriptPassages(await run(repository.listNotes()),search)
  }
  /** Meetings by date, newest first. The repository returns at most 20 a call,
   * so later pages are requested by moving the end date back. */
  async function listRange(from:string|undefined,to:string|undefined,maximum:number):Promise<{rows:NoteDTO[];more:boolean}> {
    const rows:NoteDTO[]=[],seen=new Set<string>()
    let end=to
    for(let page=0;page<8&&rows.length<maximum;page++){
      signal.throwIfAborted()
      const batch=await searchRows("",from,end),fresh=batch.filter(row=>!seen.has(row.id))
      for(const row of fresh){seen.add(row.id);rows.push(row)}
      if(batch.length<REPOSITORY_PAGE)return {rows:rows.slice(0,maximum),more:rows.length>maximum}
      const oldest=batch.reduce((date,row)=>row.date<date?row.date:date,batch[0]!.date)
      // No progress on this end date: every remaining meeting shares one day.
      const next=fresh.length?oldest:new Date(Date.parse(`${oldest}T00:00:00Z`)-86_400_000).toISOString().slice(0,10)
      if(from&&next<from)return {rows:rows.slice(0,maximum),more:rows.length>maximum}
      end=next
    }
    return {rows:rows.slice(0,maximum),more:true}
  }
  function register(row:NoteDTO):Meeting {
    const known=meetings.get(row.id)
    if(known){if(row.transcriptSegments.length||!known.row.transcriptSegments.length)known.row=row;return known}
    const meeting:Meeting={alias:`m${meetings.size+1}`,id:row.id,row}
    meetings.set(row.id,meeting);aliases.set(meeting.alias,meeting)
    return meeting
  }
  const fetched=new Map<string,Promise<NoteDTO|null>>()
  const fetchNote=(id:string)=>{if(!fetched.has(id))fetched.set(id,run(repository.getNote(id)).then(note=>note&&forContext(note)));return fetched.get(id)!}
  async function load(meeting:Meeting):Promise<ProviderNote|null> {
    if(meeting.full!==undefined)return meeting.full
    signal.throwIfAborted()
    const source=await fetchNote(meeting.id)
    meeting.full=source?await enrich(source):null
    if(meeting.full)meeting.row=meeting.full
    return meeting.full
  }
  /** Only meetings this request has discovered can be named, by alias or by ID. */
  const resolve=(value:unknown):Meeting|undefined=>typeof value!=="string"?undefined:aliases.get(value.trim().toLowerCase())??(UUID.test(value.trim())?meetings.get(value.trim().toLowerCase()):undefined)

  // ---------------------------------------------------------------- output
  const remaining=()=>budget-spent
  const people=(note:ProviderNote|NoteDTO)=>{
    const participants=(note as ProviderNote).meetingContext?.participants??[]
    const names=participants.filter(person=>!person.isResource).map(person=>`${person.displayName??person.email??"Unknown"}${person.isSelf?" (you)":""}`)
    return names.length>8?`${names.slice(0,8).join(", ")} and ${names.length-8} more`:names.join(", ")
  }
  const has=(row:NoteDTO)=>[row.body.trim()?"my notes":"",row.enhanced.trim()?"summary":"",row.transcriptRevision>0||row.transcriptSegments.length?`transcript${row.duration?` (${row.duration})`:""}`:""].filter(Boolean).join(", ")||"empty"
  const rowLine=(meeting:Meeting,extra="")=>`${meeting.alias} | ${dayLabel(meeting.row.date)} | ${meeting.row.title||"Untitled"} | has: ${has(meeting.row)}${extra}`
  const open=(meeting:Meeting,note:ProviderNote|NoteDTO,extra="")=>{const who=people(note);return `<meeting id="${meeting.alias}" title="${attribute(meeting.row.title||"Untitled")}" date="${dayLabel(meeting.row.date)}"${who?` attendees="${attribute(who)}"`:""}${extra}>`}
  const speaker=(segment:Segment)=>`${segment.speakerLabel??"Speaker"} (${segment.channel==="mic"?"mic":"system"})`
  const line=(segment:Segment,span:Span)=>`[${clock(segment.startMs)}] ${speaker(segment)}: ${span.start>0?"… ":""}${segment.text.slice(span.start,span.end)}${span.end<segment.text.length?" …":""}`
  const record=(meeting:Meeting)=>{let entry=evidence.get(meeting.id);if(!entry){entry={meeting,segments:new Map()};evidence.set(meeting.id,entry)}return entry}
  const show=(meeting:Meeting,segment:Segment,span:Span,score:number)=>{
    const entry=record(meeting),known=entry.segments.get(segment.id)
    entry.segments.set(segment.id,{segment,span:union(known?.span,span),score:Math.max(score,known?.score??0)})
  }
  /** Evidence is what was handed to the model. A block that turns out not to
   * fit the budget is not handed over, so what it recorded is undone. */
  const snapshot=(meeting:Meeting)=>{
    const entry=evidence.get(meeting.id),saved=entry?{summary:entry.summary,notes:entry.notes,segments:new Map(entry.segments)}:null
    return ()=>{
      const current=evidence.get(meeting.id)
      if(!saved)evidence.delete(meeting.id)
      else if(current){current.summary=saved.summary;current.notes=saved.notes;current.segments=saved.segments}
    }
  }
  function progress(callId:string,name:ToolName,status:ChatToolProgressDTO["status"],label:string,found?:ReadonlyArray<Meeting>,count?:number) {
    shownSteps.push({name,status,label:label.slice(0,200),...(found?{resultCount:count??found.length}:{})})
    emit({type:"tool",text:"",tool:{callId,name,status,label:label.slice(0,200),...(found?{resultCount:count??found.length,results:found.slice(0,20).map(meeting=>({noteId:meeting.id,title:meeting.row.title,date:meeting.row.date}))}:{})}})
  }
  /** Notes and summary of one meeting, each cut to a limit; a meeting with
   * neither gets the start of its transcript instead. */
  function renderDocuments(meeting:Meeting,note:ProviderNote|NoteDTO,limits:{summary:number;notes:number;transcript:number},terms:ReadonlyArray<string>=[]):{text:string;transcript:boolean} {
    const parts:string[]=[],entry=record(meeting)
    let transcript=false
    const section=(tag:"summary"|"my_notes",text:string,limit:number)=>{
      if(!text.trim())return
      const span=text.length>limit?bestWindow(text,terms,limit):trimSpan(text,{start:0,end:text.length})
      if(tag==="summary")entry.summary=union(entry.summary,span);else entry.notes=union(entry.notes,span)
      parts.push(`<${tag}${span.end-span.start<text.trim().length?` note="excerpt; ${text.length-(span.end-span.start)} more characters not shown"`:""}>\n${text.slice(span.start,span.end)}\n</${tag}>`)
    }
    section("summary",note.enhanced,limits.summary);section("my_notes",note.body,limits.notes)
    const segments=note.transcriptSegments
    if(!parts.length&&segments.length&&limits.transcript>0){
      const lines:string[]=[];let size=0,shown=0
      transcript=true
      for(const segment of segments){
        const span=trimSpan(segment.text,{start:0,end:Math.min(segment.text.length,2000)})
        if(size+span.end-span.start>limits.transcript&&shown)break
        show(meeting,segment,span,0);lines.push(line(segment,span));size+=span.end-span.start;shown++
      }
      parts.push(`<transcript note="This meeting has no notes or summary. ${shown===segments.length?"Complete transcript":`First ${shown} of ${segments.length} transcript turns; use get_meeting_transcript for the rest`}.">\n${lines.join("\n")}\n</transcript>`)
    }else if(segments.length||note.transcriptRevision>0)parts.push(`<transcript note="not shown${note.duration?`; ${note.duration} long`:""}. Use search_transcripts or get_meeting_transcript with this id for what was said."/>`)
    if(!parts.length)parts.push("(this meeting has no notes, summary or transcript)")
    return {text:parts.join("\n"),transcript}
  }

  // ----------------------------------------------------------------- tools
  /** Ranked keyword search over titles, typed notes, summaries and transcripts. */
  async function searchMeetings(queries:string[],from:string|undefined,to:string|undefined,limit:number,inline:number,inlineCoverage=0):Promise<{text:string;found:Meeting[];read:Meeting[];heard:Meeting[]}> {
    const perQuery=queries.map(query=>({query,terms:searchWords(query)})).filter(item=>item.terms.length)
    const terms=[...new Set(perQuery.flatMap(item=>item.terms))].slice(0,12)
    for(const term of terms)focus.add(term)
    if(!terms.length)return {text:`No distinctive search words in ${queries.map(query=>`"${query}"`).join(", ")}. Search for names, companies, products or topics; to browse by date use list_meetings or get_meetings with from/to.`,found:[],read:[],heard:[]}
    const results=await Promise.all(terms.map(term=>searchRows(term,from,to)))
    const weight=new Map(terms.map((term,index)=>[term,results[index]!.length>=REPOSITORY_PAGE?0.5:Math.log(1+REPOSITORY_PAGE/Math.max(1,results[index]!.length))]))
    const candidates=new Map<string,{row:NoteDTO;returned:Set<string>}>()
    results.forEach((rows,index)=>{for(const row of rows){const entry=candidates.get(row.id)??{row,returned:new Set<string>()};entry.returned.add(terms[index]!);candidates.set(row.id,entry)}})
    // A meeting returned only because of its transcript is checked against the
    // transcript itself (search rows do not carry it): the repository matches
    // substrings, and a hit inside another word is not a match. Bounded.
    const spokenOnly=[...candidates.values()].filter(({row})=>!row.transcriptSegments.length&&!terms.some(term=>containsWord(row.title,term)||containsWord(row.enhanced,term)||containsWord(row.body,term)))
      .sort((a,b)=>b.returned.size-a.returned.size||b.row.date.localeCompare(a.row.date)).slice(0,10)
    const transcripts=new Map<string,ReadonlyArray<Segment>>()
    await Promise.all(spokenOnly.map(async({row})=>{const note=await fetchNote(row.id);if(note)transcripts.set(row.id,note.transcriptSegments)}))
    const scored=[...candidates.values()].map(({row,returned})=>{
      const title=row.title.toLowerCase(),summary=row.enhanced.toLowerCase(),body=row.body.toLowerCase(),fields=new Set<string>(),matched=new Map<string,number>()
      const segments=row.transcriptSegments.length?row.transcriptSegments:transcripts.get(row.id)
      for(const term of terms){
        let factor=0
        if(containsWord(row.title,term)){factor+=3;fields.add("title")}
        if(containsWord(row.enhanced,term)){factor=Math.max(factor,2)+(factor?0.5:0);fields.add("summary")}
        if(containsWord(row.body,term)){factor=Math.max(factor,2)+(factor?0.5:0);fields.add("my notes")}
        // Returned for this word without it being in the title or notes: it was spoken.
        const spoken=segments?segments.some(segment=>containsWord(segment.text,term)):returned.has(term)&&!factor
        if(spoken){fields.add("transcript");if(!factor)factor=1}
        if(factor)matched.set(term,factor*weight.get(term)!)
      }
      const each=perQuery.map(item=>{
        const hit=item.terms.filter(term=>matched.has(term)),base=hit.reduce((sum,term)=>sum+matched.get(term)!,0)
        const phrase=item.terms.length>1&&[title,summary,body].some(text=>text.includes(item.query.trim().toLowerCase()))
        return base*(hit.length===item.terms.length?1.5:hit.length/item.terms.length)*(phrase?1.3:1)
      }).sort((a,b)=>b-a)
      return {row,fields,matched:[...matched.keys()],coverage:0,score:(each[0]??0)+0.25*each.slice(1).reduce((sum,value)=>sum+value,0)}
    }).filter(item=>item.score>0).sort((a,b)=>b.score-a.score||b.row.date.localeCompare(a.row.date)||b.row.updatedAt.localeCompare(a.row.updatedAt))
    // How much of a query a meeting covers, by weight. A word that occurs nowhere
    // counts against every meeting, but for less than a rare word that does occur:
    // one stray word in a question must not hide a strong match, while a question
    // whose distinctive words are all missing ("Lisbon office") covers nothing.
    const present=new Set(scored.flatMap(item=>item.matched))
    for(const item of scored)item.coverage=Math.max(0,...perQuery.map(query=>{
      const live=query.terms.filter(term=>present.has(term)),total=live.reduce((sum,term)=>sum+weight.get(term)!,0)+1.5*(query.terms.length-live.length)
      return total?live.filter(term=>item.matched.includes(term)).reduce((sum,term)=>sum+weight.get(term)!,0)/total:0
    }))
    const top=scored.slice(0,limit),found=top.map(item=>register(item.row)),read:Meeting[]=[],heard:Meeting[]=[]
    const period=from||to?` between ${from??"the beginning"} and ${to??"today"}`:""
    if(!top.length)return {text:`No meetings match ${queries.map(query=>`"${query}"`).join(", ")}${period}. Matching is literal: try other words, names or spellings, or drop the date range. If nothing relevant exists, say so rather than stretching.`,found:[],read,heard}
    // A word that occurs nowhere is the strongest sign that the thing asked about is not in the meetings.
    const absent=terms.filter(term=>!scored.some(item=>item.matched.includes(term)))
    const out:string[]=[`${plural(scored.length,"meeting")} matched${scored.length>top.length?`; the ${top.length} most relevant are shown`:""}, most relevant first (not in date order).${absent.length?` No meeting${period} contains ${absent.map(term=>`"${term}"`).join(" or ")}, in notes or transcripts: the matches below are for the other words only.`:""}`]
    const best=top[0]!.score
    for(const [index,item] of top.entries()){
      const meeting=found[index]!,where=[...item.fields].join(", "),onlySpoken=item.fields.size===1&&item.fields.has("transcript")
      const full=index<inline&&item.score>=best*0.5&&item.coverage>=inlineCoverage
      let block:string,documents=false,spoken=false
      const undo=snapshot(meeting)
      if(full){
        const note=await enrich(item.row),body:string[]=[]
        if(item.row.enhanced.trim()||item.row.body.trim()){body.push(renderDocuments(meeting,note,{summary:2600,notes:1400,transcript:0},item.matched).text.replace(/\n?<transcript note="not shown[^>]*\/>/,""));documents=true}
        // Search rows from SQL carry no transcript, so whether the words were also
        // spoken is read from the transcript itself for the few best matches. Both
        // repository paths then show the same passages.
        const turns=item.row.transcriptSegments.length?item.row.transcriptSegments:item.row.transcriptRevision>0?(await fetchNote(item.row.id))?.transcriptSegments??[]:[]
        if(item.matched.some(term=>turns.some(turn=>containsWord(turn.text,term)))){
          item.fields.add("transcript")
          const passages=await passagesIn(meeting,item.matched,3)
          if(passages){body.push(`<transcript_matches>\n${passages}\n</transcript_matches>`);spoken=true}
        }
        block=`${open(meeting,note,` matched="${[...item.fields].join(", ")}"`)}\n${body.join("\n")||"(no notes or summary)"}\n</meeting>`
      }else{
        const inSummary=item.matched.some(term=>containsWord(item.row.enhanced,term))
        const source=inSummary?item.row.enhanced:item.matched.some(term=>containsWord(item.row.body,term))?item.row.body:""
        const window=source?bestWindow(source,item.matched,220):null
        // Whole lines, so an excerpt never ends mid-fact ("- 212").
        const span=window?trimSpan(source,{start:source.lastIndexOf("\n",window.start)+1,end:source.indexOf("\n",window.end)<0?source.length:source.indexOf("\n",window.end)}):null
        // An excerpt the model has seen is evidence it may rely on: the answer must see it too.
        if(span){const entry=record(meeting);if(inSummary)entry.summary=union(entry.summary,span);else entry.notes=union(entry.notes,span)}
        block=rowLine(meeting,` | matched in: ${where}`)+(span?`\n   “${source.slice(span.start,span.end).replace(/\s+/g," ")}”`:onlySpoken?"\n   (the words were spoken; they are not in the notes)":"")
      }
      if(block.length>remaining()){undo();out.push(`(context limit reached; ${top.length-index} more matches not shown)`);break}
      spent+=block.length;out.push(block)
      if(!full&&!listed.includes(meeting))listed.push(meeting)
      if(documents)read.push(meeting)
      if(spoken)heard.push(meeting)
    }
    return {text:out.join("\n"),found,read,heard}
  }
  /** Best transcript passages of one meeting for some words, as transcript lines. */
  async function passagesIn(meeting:Meeting,terms:ReadonlyArray<string>,maximum:number):Promise<string> {
    const note=await load(meeting)
    if(!note)return ""
    const ranked=rankSegments(note.transcriptSegments,[{query:terms.join(" "),terms:[...terms]}],new Map(terms.map(term=>[term,1]))).slice(0,maximum)
    const lines=ranked.sort((a,b)=>a.segment.startMs-b.segment.startMs).map(hit=>{const span=bestWindow(hit.segment.text,terms,600);show(meeting,hit.segment,span,hit.score);return line(hit.segment,span)})
    return lines.join("\n")
  }
  function rankSegments(segments:ReadonlyArray<Segment>,perQuery:ReadonlyArray<{query:string;terms:string[]}>,weight:ReadonlyMap<string,number>) {
    return segments.map(segment=>{
      const lower=segment.text.toLowerCase()
      const each=perQuery.map(item=>{
        const hit=item.terms.filter(term=>containsWord(segment.text,term)),base=hit.reduce((sum,term)=>sum+(weight.get(term)??1),0)
        return base*(hit.length===item.terms.length?1.5:hit.length/item.terms.length)*(item.terms.length>1&&lower.includes(item.query.trim().toLowerCase())?1.3:1)
      }).sort((a,b)=>b-a)
      return {segment,score:(each[0]??0)+0.25*each.slice(1).reduce((sum,value)=>sum+value,0)}
    }).filter(item=>item.score>0).sort((a,b)=>b.score-a.score||a.segment.startMs-b.segment.startMs)
  }
  /** Passage search: find the meetings whose transcripts contain the words,
   * then rank every turn of the best few and return passages with the turns
   * around them, because an answer is often in the turn after the question. */
  async function searchTranscripts(queries:string[],scope:Meeting[]|null,from:string|undefined,to:string|undefined,limit:number):Promise<{text:string;found:Meeting[];passages:number;available:number}> {
    const perQuery=queries.map(query=>({query,terms:searchWords(query)})).filter(item=>item.terms.length)
    const terms=[...new Set(perQuery.flatMap(item=>item.terms))].slice(0,12)
    for(const term of terms)focus.add(term)
    if(!terms.length)return {text:"No distinctive search words. Search transcripts for the words someone would have said: names, numbers, product or topic words.",found:[],passages:0,available:0}
    const weight=new Map<string,number>()
    let candidates:Meeting[]
    if(scope){candidates=scope;for(const term of terms)weight.set(term,1)}
    else{
      const results=await Promise.all(terms.map(term=>searchSegments(term,{...(from?{from}:{}),...(to?{to}:{})})))
      const byMeeting=new Map<string,{row:NoteDTO;score:number}>()
      results.forEach((hits,index)=>{
        const ids=new Set(hits.map(hit=>hit.noteId)),value=hits.length>=REPOSITORY_PAGE&&ids.size>=6?0.5:Math.log(1+REPOSITORY_PAGE/Math.max(1,ids.size))
        weight.set(terms[index]!,value)
        for(const hit of hits){
          if(byMeeting.get(hit.noteId)?.row&&!ids.delete(hit.noteId))continue
          const known=meetings.get(hit.noteId)?.row
          const row=known??{id:hit.noteId,title:hit.title,date:hit.date,body:"",enhanced:"",duration:"",createdAt:"",updatedAt:"",transcriptSegments:[],recordingState:"stopped",generationState:"idle",transcriptRevision:hit.transcriptRevision,generatedRevision:null} as NoteDTO
          const entry=byMeeting.get(hit.noteId)??{row,score:0};entry.score+=value;byMeeting.set(hit.noteId,entry);ids.delete(hit.noteId)
        }
      })
      candidates=[...byMeeting.values()].sort((a,b)=>b.score-a.score||b.row.date.localeCompare(a.row.date)).slice(0,6).map(item=>register(item.row))
    }
    const hits:Array<{meeting:Meeting;note:ProviderNote;index:number;score:number}>=[]
    for(const meeting of candidates){
      const note=await load(meeting)
      if(!note)continue
      const position=new Map(note.transcriptSegments.map((segment,index)=>[segment.id,index]))
      const perMeeting=scope?limit:Math.max(2,Math.ceil(limit/2))
      for(const hit of rankSegments(note.transcriptSegments,perQuery,weight).slice(0,perMeeting))hits.push({meeting,note,index:position.get(hit.segment.id)!,score:hit.score})
    }
    hits.sort((a,b)=>b.score-a.score||b.meeting.row.date.localeCompare(a.meeting.row.date)||a.index-b.index)
    const chosen=hits.slice(0,limit),found:Meeting[]=[],out:string[]=[]
    let shown=0
    for(const meeting of candidates){
      const mine=chosen.filter(hit=>hit.meeting===meeting).sort((a,b)=>a.index-b.index)
      if(!mine.length)continue
      const note=mine[0]!.note,lines:string[]=[],done=new Set<number>(),undo=snapshot(meeting)
      let previous=-2,size=0
      for(const hit of mine){
        // The turn before and after give the question and the reply.
        for(const index of [hit.index-1,hit.index,hit.index+1]){
          const segment=note.transcriptSegments[index]
          if(!segment||done.has(index))continue
          done.add(index)
          const isHit=index===hit.index||mine.some(other=>other.index===index)
          const span=isHit?bestWindow(segment.text,terms,700):index<hit.index?trimSpan(segment.text,{start:Math.max(0,segment.text.length-280),end:segment.text.length}):trimSpan(segment.text,{start:0,end:Math.min(segment.text.length,280)})
          const text=`${index>previous+1&&lines.length?"[…]\n":""}${isHit?"> ":"  "}${line(segment,span)}`
          size+=text.length;lines.push(text);previous=index
          show(meeting,segment,span,isHit?hit.score:0)
        }
        shown++
      }
      const block=`${open(meeting,note)}\n${lines.join("\n")}\n</meeting>`
      if(block.length>remaining()){undo();shown-=mine.length;continue}
      spent+=block.length;out.push(block);found.push(meeting)
    }
    const where=scope?` in ${scope.map(meeting=>meeting.alias).join(", ")}`:from||to?` between ${from??"the beginning"} and ${to??"today"}`:""
    if(!hits.length)return {text:`No transcript passage contains ${terms.map(term=>`"${term}"`).join(" or ")}${where}. Matching is literal, so try other words or spellings; an empty result does not prove the topic never came up.`,found:[],passages:0,available:0}
    return {text:`${plural(shown,"passage")} in ${plural(found.length,"meeting")}${where}, best matches marked with ">" and shown with the turns around them:\n${out.join("\n")}${shown<chosen.length?"\n(context limit reached; some passages are not shown)":""}`,found,passages:shown,available:chosen.length}
  }
  /** Notes and summaries of several meetings. */
  async function readMeetings(chosen:Meeting[],more:string):Promise<{text:string;found:Meeting[];heard:Meeting[]}> {
    const out:string[]=[],found:Meeting[]=[],heard:Meeting[]=[]
    // Shrink the per-meeting allowance as more are read at once.
    const limits=chosen.length<=4?{summary:8000,notes:5000,transcript:9000}:chosen.length<=8?{summary:5000,notes:3000,transcript:5000}:{summary:3500,notes:2000,transcript:3000}
    for(const [index,meeting] of chosen.entries()){
      const note=await load(meeting)
      if(!note)continue
      // Already in front of the model in full: point back at it instead of repeating it.
      const seen=evidence.get(meeting.id),whole=(text:string,span?:Span)=>!text.trim()||!!span&&span.end-span.start>=text.trim().length
      if(seen&&(seen.summary||seen.notes)&&whole(note.enhanced,seen.summary)&&whole(note.body,seen.notes)){
        const block=`<meeting id="${meeting.alias}" title="${attribute(meeting.row.title||"Untitled")}" date="${dayLabel(meeting.row.date)}">(notes and summary already shown in full above; nothing new)</meeting>`
        spent+=block.length;out.push(block);continue
      }
      const undo=snapshot(meeting)
      const documents=renderDocuments(meeting,note,limits,[...focus]),block=`${open(meeting,note)}\n${documents.text}\n</meeting>`
      if(block.length>remaining()){
        undo()
        out.push(`(context limit reached; ${chosen.length-index} meetings not read: ${chosen.slice(index).map(item=>item.alias).join(", ")})`);break
      }
      spent+=block.length;out.push(block);found.push(meeting)
      if(documents.transcript)heard.push(meeting)
    }
    return {text:`${out.join("\n")}${more}`||"None of those meetings could be read.",found,heard}
  }
  /** One meeting's transcript: whole when short, else the parts asked for. */
  async function readTranscript(meeting:Meeting,query:string|undefined,around:number|undefined):Promise<{text:string;found:Meeting[]}> {
    const note=await load(meeting)
    if(!note)return {text:"That meeting could not be read.",found:[]}
    const segments=note.transcriptSegments,total=segments.reduce((sum,segment)=>sum+segment.text.length,0)
    if(!segments.length)return {text:`${meeting.alias} has no transcript.${note.enhanced.trim()||note.body.trim()?" It has notes: read them with get_meetings.":""}`,found:[]}
    const terms=query?searchWords(query):[]
    for(const term of terms)focus.add(term)
    const allowance=Math.min(remaining(),total<=16_000?16_000:10_000)
    let picked:Array<{index:number;score:number}>
    let description:string
    if(total<=allowance){picked=segments.map((_,index)=>({index,score:0}));description="complete transcript"}
    else if(terms.length){
      const position=new Map(segments.map((segment,index)=>[segment.id,index])),ranked=rankSegments(segments,[{query:query!,terms}],new Map(terms.map(term=>[term,1])))
      const set=new Map<number,number>();let size=0
      for(const hit of ranked){
        const index=position.get(hit.segment.id)!
        for(const near of [index-1,index,index+1]){const segment=segments[near];if(!segment||set.has(near))continue;if(size+Math.min(segment.text.length,2000)>allowance)break;set.set(near,near===index?hit.score:0);size+=Math.min(segment.text.length,2000)}
        if(size>allowance*0.9)break
      }
      picked=[...set.entries()].map(([index,score])=>({index,score})).sort((a,b)=>a.index-b.index)
      description=picked.length?`the ${picked.length} of ${segments.length} turns most relevant to "${query}", in order ([…] marks skipped stretches)`:`no turn contains ${terms.map(term=>`"${term}"`).join(" or ")}; try other words, or \`around\` a time`
    }else{
      const start=around===undefined?0:Math.max(0,segments.findIndex(segment=>(segment.endMs??segment.startMs)>=around-90_000))
      picked=[];let size=0
      for(let index=start<0?0:start;index<segments.length;index++){const length=Math.min(segments[index]!.text.length,2000);if(size+length>allowance&&picked.length)break;picked.push({index,score:0});size+=length}
      const last=picked.at(-1)
      description=`turns ${picked[0]?clock(segments[picked[0].index]!.startMs):"0:00"} to ${last?clock(segments[last.index]!.startMs):"0:00"} of a ${note.duration||clock(segments.at(-1)!.endMs??segments.at(-1)!.startMs)} transcript; pass \`query\` for the relevant stretches or \`around\` for another time`
    }
    const lines:string[]=[],undo=snapshot(meeting);let previous=-1
    for(const {index,score} of picked){
      const segment=segments[index]!,span=trimSpan(segment.text,segment.text.length>2000?bestWindow(segment.text,terms,2000):{start:0,end:segment.text.length})
      lines.push(`${index>previous+1&&lines.length?"[…]\n":""}${line(segment,span)}`);previous=index
      show(meeting,segment,span,score)
    }
    const block=`${open(meeting,note,` transcript="${attribute(description)}"`)}\n${lines.join("\n")}\n</meeting>`
    if(block.length>remaining()){undo();return {text:"(context limit reached; the transcript could not be added)",found:[]}}
    spent+=block.length
    return {text:block,found:picked.length?[meeting]:[]}
  }

  // -------------------------------------------------------------- dispatch
  const strings=(value:unknown,maximum:number,length:number):string[]|null=>{
    const list=typeof value==="string"?[value]:Array.isArray(value)?value:null
    if(!list||!list.length||list.length>maximum||!list.every(item=>typeof item==="string"&&item.trim().length>0&&item.length<=length))return null
    return [...new Set((list as string[]).map(item=>item.trim()))]
  }
  const integer=(value:unknown,fallback:number,minimum:number,maximum:number)=>value===undefined?fallback:Number.isInteger(value)?Math.min(maximum,Math.max(minimum,Number(value))):null
  const quoted=(queries:ReadonlyArray<string>)=>queries.map(query=>`“${query}”`).join(", ")
  const titled=(meeting:Meeting)=>`“${(meeting.row.title||"Untitled").slice(0,80)}”`
  const between=(from?:string,to?:string)=>from&&to?(from===to?` on ${from}`:` from ${from} to ${to}`):from?` since ${from}`:to?` up to ${to}`:""
  /** Execute one tool call. Never throws for bad arguments: the model gets a
   * message it can act on, the user sees an honest failed step. */
  async function call(name:ToolName,raw:unknown,options:{inline?:number;inlineCoverage?:number}={}):Promise<ToolOutcome> {
    signal.throwIfAborted()
    const callId=crypto.randomUUID()
    let doing=""
    const fail=(label:string,text:string):ToolOutcome=>{progress(callId,name,"failed",label);return {text,final:false,step:`${doing||name}: failed (${text.slice(0,120)})`}}
    const start=(label:string)=>{doing=label;progress(callId,name,"running",label)}
    const done=(label:string,found:ReadonlyArray<Meeting>,text:string,final=false,count?:number):ToolOutcome=>{progress(callId,name,"completed",label,found,count);return {text,final,step:`${doing} → ${label}`}}
    /** Content handed over as part of another call is still a step the user sees. */
    const also=(kind:ToolName,running:string,completed:string,found:ReadonlyArray<Meeting>)=>{const id=crypto.randomUUID();progress(id,kind,"running",running);progress(id,kind,"completed",completed,found)}
    const transcriptsRead=(heard:ReadonlyArray<Meeting>)=>{
      const unique=[...new Set(heard)]
      if(unique.length===1)also("get_meeting_transcript",`Reading transcript for ${titled(unique[0]!)}`,`Read transcript for ${titled(unique[0]!)}`,unique)
      else if(unique.length)also("get_meeting_transcript",`Reading ${unique.length} transcripts`,`Read ${unique.length} transcripts`,unique)
    }
    if(++calls>MAX_TOOL_CALLS)return fail("Lookup limit reached","No more lookups are available in this request. Reply READY.")
    if(!raw||typeof raw!=="object"||Array.isArray(raw))return fail("Could not complete meeting lookup","Invalid arguments: pass a JSON object.")
    const args=raw as Record<string,unknown>
    const from=args.from,to=args.to
    if(from!==undefined&&!isDate(from)||to!==undefined&&!isDate(to)||isDate(from)&&isDate(to)&&from>to)return fail("Could not complete meeting lookup","Invalid date range: from and to are YYYY-MM-DD calendar dates, from on or before to.")
    const range=[from as string|undefined,to as string|undefined] as const
    const final=args.then==="answer"
    try{
      if(name==="search_meetings"){
        const queries=strings(args.queries??args.query,6,300),limit=integer(args.limit,8,1,20)
        if(!queries||limit===null)return fail("Could not complete meeting lookup","Invalid arguments: `queries` is a list of 1 to 6 short strings; `limit` is 1 to 20.")
        start(`Searching meetings for ${quoted(queries)}${between(...range)}`)
        const result=await searchMeetings(queries,range[0],range[1],limit,options.inline??3,options.inlineCoverage??0)
        const outcome=done(`Found ${plural(result.found.length,"meeting")}`,result.found,result.text)
        if(result.read.length)also("get_meetings",`Looking deeper into ${plural(result.read.length,"note")}`,`Looked deeper into ${plural(result.read.length,"note")}`,result.read)
        transcriptsRead(result.heard)
        return outcome
      }
      if(name==="list_meetings"){
        const limit=integer(args.limit,30,1,60)
        if(limit===null)return fail("Could not complete meeting lookup","Invalid arguments: `limit` is 1 to 60.")
        start(range[0]||range[1]?`Listing meetings${between(...range)}`:"Finding recent meetings")
        const {rows,more}=await listRange(range[0],range[1],limit),found=rows.map(register),lines:string[]=[]
        for(const meeting of found){const text=rowLine(meeting);if(text.length>remaining())break;spent+=text.length;lines.push(text);if(!listed.includes(meeting))listed.push(meeting)}
        if(!found.length)return done("Found 0 meetings",found,`No meetings${between(...range)||" at all"}.`)
        return done(`Found ${plural(found.length,"meeting")}`,found,`${plural(lines.length,"meeting")}${between(...range)}, newest first${more?` (more exist${range[0]?"":" earlier"}; narrow the range or set \`to\` to ${rows.at(-1)!.date} to continue)`:range[0]||range[1]?" (that is all of them in this range)":""}:\n${lines.join("\n")}`)
      }
      if(name==="search_transcripts"){
        const queries=strings(args.queries??args.query,6,300),limit=integer(args.limit,8,1,16)
        const ids=args.ids??args.noteIds??args.meeting_ids
        if(!queries||limit===null||ids!==undefined&&(!Array.isArray(ids)||ids.length>8))return fail("Could not complete meeting lookup","Invalid arguments: `queries` is a list of 1 to 6 short strings; `ids` is a list of up to 8 meeting ids; `limit` is 1 to 16.")
        const scope=Array.isArray(ids)&&ids.length?(ids.map(resolve).filter(Boolean) as Meeting[]):null
        if(scope&&!scope.length)return fail("Could not complete meeting lookup","None of those ids is a meeting from this conversation. Use ids exactly as returned by search or list results (for example m3).")
        start(`Searching ${scope?(scope.length===1?`transcript of ${titled(scope[0]!)}`:`${scope.length} transcripts`):"transcripts"} for ${quoted(queries)}${scope?"":between(...range)}`)
        const result=await searchTranscripts(queries,scope,range[0],range[1],limit)
        const label=result.passages===result.available?`Found ${plural(result.passages,"transcript passage")} in ${plural(result.found.length,"meeting")}`:`Read ${result.passages} of ${result.available} transcript passages in ${plural(result.found.length,"meeting")} (context limit)`
        return done(label,result.found,result.text,final&&result.passages>0,result.passages)
      }
      if(name==="get_meetings"){
        const ids=args.ids??args.noteIds??args.meeting_ids
        let chosen:Meeting[],more=""
        const unknown:string[]=[]
        if(ids!==undefined){
          if(!Array.isArray(ids)||!ids.length||ids.length>12||!ids.every(id=>typeof id==="string"))return fail("Could not complete meeting lookup","Invalid arguments: `ids` is a list of 1 to 12 meeting ids, or pass from/to to read by date.")
          chosen=[...new Set(ids.map(id=>{const meeting=resolve(id);if(!meeting)unknown.push(String(id).slice(0,40));return meeting}).filter(Boolean) as Meeting[])]
          if(!chosen.length)return fail("Could not complete meeting lookup","None of those ids is a meeting from this conversation. Use ids exactly as returned by search or list results (for example m3); you cannot read a meeting that has not been found first.")
        }else if(range[0]||range[1]){
          const listing=await listRange(range[0],range[1],12)
          chosen=listing.rows.map(register)
          if(listing.more)more=`\n(More meetings exist${between(...range)} than the 12 newest read here. To continue, call again with \`to\` set to ${listing.rows.at(-1)!.date}.)`
          if(!chosen.length){start(`Looking for meetings${between(...range)}`);return done("Found 0 meetings",[],`No meetings${between(...range)}.`)}
        }else return fail("Could not complete meeting lookup","Invalid arguments: pass `ids`, or `from`/`to` to read every meeting in a date range.")
        start(`Looking deeper into ${plural(chosen.length,"note")}${ids===undefined?between(...range):""}`)
        const result=await readMeetings(chosen,more)
        const outcome=done(`Looked deeper into ${plural(result.found.length,"note")}${more?" (more exist in this period)":""}`,result.found,result.text+(unknown.length?`\n(Not meetings from this conversation, so not read: ${unknown.join(", ")})`:""),final&&result.found.length>0)
        transcriptsRead(result.heard)
        return outcome
      }
      const id=args.id??args.noteId??args.meeting_id,meeting=resolve(id)
      if(!meeting)return fail("Could not complete meeting lookup","That id is not a meeting from this conversation. Use an id exactly as returned by search or list results (for example m3).")
      const query=args.query===undefined?undefined:typeof args.query==="string"&&args.query.length<=300?args.query:null
      const around=args.around!==undefined?minutes(args.around):args.startMs!==undefined?(Number.isInteger(args.startMs)&&Number(args.startMs)>=0?Number(args.startMs):null):undefined
      if(query===null||around===null)return fail("Could not complete meeting lookup","Invalid arguments: `query` is a short string; `around` is a time such as \"12:30\".")
      start(`Reading transcript for ${titled(meeting)}`)
      const result=await readTranscript(meeting,query,around)
      return done(result.found.length?`Read transcript for ${titled(meeting)}`:`No transcript read for ${titled(meeting)}`,result.found,result.text,final&&result.found.length>0)
    }catch(error){
      signal.throwIfAborted()
      return fail("Could not complete meeting lookup","The lookup failed. Try once more or reply READY.")
    }
  }
  const minutes=(value:unknown):number|null=>{
    if(typeof value==="number"&&Number.isFinite(value)&&value>=0)return Math.round(value*60_000)
    const match=typeof value==="string"?/^(?:(\d{1,2}):)?(\d{1,3}):([0-5]\d)$/.exec(value.trim()):null
    return match?(Number(match[1]??0)*3600+Number(match[2])*60+Number(match[3]))*1000:null
  }

  /** The newest meetings, as rows, for the scope the planner starts from. Reading
   * the list is part of assembling the request, not a step the user needs to see. */
  async function recent(maximum:number):Promise<{lines:string[];complete:boolean}> {
    const {rows,more}=await listRange(undefined,undefined,maximum)
    return {lines:rows.map(register).map(meeting=>rowLine(meeting)),complete:!more}
  }

  // --------------------------------------------------------- final context
  /** Everything retrieved, written once for the answer, with a source ID in
   * front of each citable passage. Fills the session's issued citations. */
  function context():string {
    const issue=!!input.includeSources
    interface Unit {meeting:Meeting;source:ChatCitationDTO["source"];span:Span;segment?:Segment;score:number;cited:boolean}
    const entries=[...evidence.values()].filter(entry=>entry.summary||entry.notes||entry.segments.size).sort((a,b)=>b.meeting.row.date.localeCompare(a.meeting.row.date)||a.meeting.alias.localeCompare(b.meeting.alias))
    const words=[...focus]
    const byMeeting=new Map<Evidence,Unit[]>()
    for(const entry of entries){
      const note=entry.meeting.full??entry.meeting.row,units:Unit[]=[]
      if(entry.summary)for(const span of blocks(note.enhanced,entry.summary))units.push({meeting:entry.meeting,source:"summary",span,score:0,cited:true})
      if(entry.notes)for(const span of blocks(note.body,entry.notes))units.push({meeting:entry.meeting,source:"notes",span,score:0,cited:true})
      for(const shown of [...entry.segments.values()].sort((a,b)=>a.segment.startMs-b.segment.startMs)){
        const lower=shown.segment.text.slice(shown.span.start,shown.span.end).toLowerCase()
        units.push({meeting:entry.meeting,source:"transcript",span:shown.span,segment:shown.segment,score:shown.score+words.filter(word=>lower.includes(word)).length*0.2,cited:true})
      }
      byMeeting.set(entry,units)
    }
    const all=()=>[...byMeeting.values()].flat(),cited=()=>all().filter(unit=>unit.cited).length,allowance=Math.max(0,MAX_CITATIONS-issuedBefore)
    // Fit the budget: coarsen documents before giving up transcript lines, and
    // never leave a meeting that was read without any citable source.
    const mergeDocuments=(floor:number)=>{
      let target:{units:Unit[];index:number;size:number}|null=null
      for(const units of byMeeting.values())for(const source of ["summary","notes"] as const){
        const mine=units.filter(unit=>unit.source===source)
        if(mine.length<=floor)continue
        for(let index=0;index<mine.length-1;index++){const size=mine[index+1]!.span.end-mine[index]!.span.start;if(!target||mine.length*10_000-size>target.size)target={units,index:units.indexOf(mine[index]!),size:mine.length*10_000-size}}
      }
      if(!target)return false
      const first=target.units[target.index]!,second=target.units[target.index+1]!
      first.span={start:first.span.start,end:second.span.end};target.units.splice(target.index+1,1)
      return true
    }
    const dropTranscript=(keep:number)=>{
      const lines=all().filter(unit=>unit.source==="transcript"&&unit.cited)
      if(lines.length<=keep)return false
      // Keep a meeting's only source even when it scores lowest.
      const candidates=lines.filter(unit=>all().filter(other=>other.meeting===unit.meeting&&other.cited).length>1).sort((a,b)=>a.score-b.score||(b.segment!.startMs-a.segment!.startMs))
      if(!candidates.length)return false
      candidates[0]!.cited=false
      return true
    }
    if(issue){
      while(cited()>allowance&&mergeDocuments(3));
      while(cited()>allowance&&dropTranscript(12));
      while(cited()>allowance&&mergeDocuments(1));
      while(cited()>allowance&&dropTranscript(0));
      // Still too many: a meeting with a summary keeps that and gives up the ID
      // on its typed notes, oldest meetings first.
      for(const units of [...byMeeting.values()].reverse()){
        if(cited()<=allowance)break
        if(units.some(unit=>unit.source==="summary"&&unit.cited))for(const unit of units)if(unit.source==="notes")unit.cited=false
      }
      // More meetings than sources: the oldest lose theirs.
      const rest=all().filter(unit=>unit.cited)
      for(let index=rest.length-1;index>=0&&cited()>allowance;index--)rest[index]!.cited=false
    }
    session.issued.length=issuedBefore
    const cite=(unit:Unit):string=>{
      if(!issue||!unit.cited||session.issued.length>=MAX_CITATIONS)return ""
      const note=unit.meeting.full??unit.meeting.row,source=unit.source==="transcript"?unit.segment!.text:unit.source==="notes"?note.body:note.enhanced
      const quote=source.slice(unit.span.start,unit.span.end)
      if(!quote.trim())return ""
      const id=`S${session.issued.length+1}`
      session.issued.push({id,noteId:unit.meeting.id,title:note.title,source:unit.source,segmentId:unit.segment?.id??null,startMs:unit.segment?.startMs??null,quote,transcriptRevision:note.transcriptRevision})
      return `[${id}] `
    }
    const out:string[]=[]
    for(const entry of entries){
      const note=entry.meeting.full??entry.meeting.row,units=byMeeting.get(entry)!,parts:string[]=[]
      for(const [source,tag] of [["summary","summary"],["notes","my_notes"]] as const){
        const mine=units.filter(unit=>unit.source===source)
        if(!mine.length)continue
        const whole=source==="summary"?note.enhanced:note.body,shown=source==="summary"?entry.summary!:entry.notes!
        parts.push(`<${tag}${shown.end-shown.start<whole.trim().length?" note=\"excerpt\"":""}>\n${mine.map(unit=>`${cite(unit)}${whole.slice(unit.span.start,unit.span.end)}`).join("\n\n")}\n</${tag}>`)
      }
      const lines=units.filter(unit=>unit.source==="transcript")
      if(lines.length){
        const total=note.transcriptSegments.length
        parts.push(`<transcript${total&&lines.length<total?` note="${lines.length} of ${total} turns"`:""}>\n${lines.map(unit=>`${cite(unit)}${line(unit.segment!,unit.span)}`).join("\n")}\n</transcript>`)
      }else if(note.transcriptRevision>0||note.transcriptSegments.length)parts.push("<transcript note=\"not retrieved\"/>")
      const who=people(note)
      out.push(`<meeting title="${attribute(note.title||"Untitled")}" date="${dayLabel(note.date)}"${who?` attendees="${attribute(who)}"`:""}>\n${parts.join("\n")}\n</meeting>`)
    }
    // Meetings that were found but not read: enough to say they exist and when.
    const others=listed.filter(meeting=>!evidence.has(meeting.id)||!entries.some(entry=>entry.meeting===meeting)).sort((a,b)=>b.row.date.localeCompare(a.row.date))
    if(others.length){
      const rows=others.slice(0,60).map(meeting=>{
        const row=meeting.row,text=row.enhanced.trim()?row.enhanced:row.body
        let marker=""
        if(issue&&session.issued.length<MAX_CITATIONS&&text.trim()){
          const first=blocks(text,trimSpan(text,{start:0,end:Math.min(text.length,400)}))[0]
          if(first){const id=`S${session.issued.length+1}`;session.issued.push({id,noteId:row.id,title:row.title,source:row.enhanced.trim()?"summary":"notes",segmentId:null,startMs:null,quote:text.slice(first.start,first.end),transcriptRevision:row.transcriptRevision});marker=`[${id}] `}
        }
        return `${marker}${dayLabel(row.date)} | ${row.title||"Untitled"} | has: ${has(row)}`
      })
      out.push(`<meetings_found note="found by search or listing; their content was not read">\n${rows.join("\n")}\n</meetings_found>`)
    }
    if(!out.length)out.push("(nothing was retrieved: no meeting matched)")
    const coverage=coverageNote(shownSteps)
    return [...notes.map(note=>`<retrieval_note>${note}</retrieval_note>`),...out,...(coverage?[`<coverage_note>${coverage}</coverage_note>`]:[])].join("\n")
  }

  return {call,recent,context,today,
    /** A step of the request itself, such as the planner being unavailable. */
    announce:(name:ToolName,status:ChatToolProgressDTO["status"],label:string)=>progress(crypto.randomUUID(),name,status,label),
    /** A fact about the retrieval itself that the answer should know (for example a range that was cut off). */
    remark:(note:string)=>{notes.push(note)},
    callCount:()=>calls,
    /** Meetings whose content was retrieved. */
    readCount:()=>[...evidence.values()].filter(entry=>entry.summary||entry.notes||entry.segments.size).length}
}
export type WorkspaceResearch=ReturnType<typeof createWorkspaceResearch>
