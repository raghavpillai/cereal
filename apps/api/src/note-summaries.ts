/**
 * Notes list and notes search: the projection a list row needs, the page cursors,
 * and the in-memory implementation of both.
 *
 * Search follows the retrieval in `meeting-search.ts`: a
 * literal, case-insensitive substring match over the title, the generated
 * summary, the typed notes and the transcript, read through the caller's scoped
 * repository, with the same field weights as `selectMeetings`. It differs where a
 * search box differs from a chat question: every word typed is kept (no
 * conversational stop words or synonyms), all of them must match, and results
 * are paged. `note-summaries-store.ts` runs the same rules in PostgreSQL; the
 * functions here are the reference and serve the memory repository.
 */
import {Schema} from "effect"
import {forContext} from "./transcript-link"
import {MeetingContextDTO,NoteSearchPageDTO,NoteSummariesPageDTO,NoteSummaryDTO} from "@meeting-notes/contracts"
import type {NoteAttendeesDTO,NoteDTO,NoteListFilter,NoteTrashScope,NoteSearchExcerptDTO,NoteSearchField,NoteSearchResultDTO,NoteSearchTextPartDTO} from "@meeting-notes/contracts"

export const SUMMARY_PAGE_DEFAULT=50,SUMMARY_PAGE_MAX=100,SEARCH_PAGE_DEFAULT=30,SEARCH_PAGE_MAX=50
/** How much of each document is read to write a row's excerpt. */
export const EXCERPT_SOURCE_CHARACTERS=1600
const EXCERPT_CHARACTERS=150,ATTENDEES_SHOWN=4,PARTS_MAXIMUM=64

export class InvalidPageRequest extends Error {}

// ── Cursors ────────────────────────────────────────────────────────────────

export interface ListCursor {date:string;createdAt:string;id:string}
export interface SearchCursor extends ListCursor {score:number}
export interface NoteListInput {limit:number;cursor:ListCursor|null;filter:NoteListFilter;trash:NoteTrashScope}
export interface NoteSearchInput {query:string;terms:ReadonlyArray<string>;limit:number;cursor:SearchCursor|null;filter:NoteListFilter;trash:NoteTrashScope}

const uuid=/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/
const day=/^\d{4}-\d{2}-\d{2}$/
const instant=/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{1,9})?(?:Z|[+-]\d{2}:\d{2})$/
const encode=(value:unknown)=>Buffer.from(JSON.stringify(value)).toString("base64url")
function decode(cursor:string):unknown[] {
  try{const value:unknown=JSON.parse(Buffer.from(cursor,"base64url").toString("utf8"));if(Array.isArray(value))return value}catch{}
  throw new InvalidPageRequest("The page cursor is not valid")
}
const position=(date:unknown,createdAt:unknown,id:unknown):ListCursor=>{
  if(typeof date!=="string"||!day.test(date)||typeof createdAt!=="string"||!instant.test(createdAt)||!Number.isFinite(Date.parse(createdAt))||typeof id!=="string"||!uuid.test(id))throw new InvalidPageRequest("The page cursor is not valid")
  return{date,createdAt,id}
}
/** A cursor is the last row of the page before it. It also carries what it was
 * issued for, so one made for another filter or search is refused, not misread. */
type Listed=Pick<NoteListInput,"filter"|"trash">
/** Reaching into Trash or not is part of what a cursor was issued for. "include" adds nothing, so the cursors of
 * clients that do not say (they list everything) read as they always did. */
const reach=(trash:NoteTrashScope)=>trash==="include"?"":`:${trash}`
const listScope=(input:Listed)=>`l:${input.filter}${reach(input.trash)}`
const searchScope=(input:Pick<NoteSearchInput,"terms"|"filter"|"trash">)=>`s:${input.filter}${reach(input.trash)}:${input.terms.join(" ")}`
const fingerprint=(scope:string)=>{let hash=2166136261;for(let i=0;i<scope.length;i++){hash^=scope.charCodeAt(i);hash=Math.imul(hash,16777619)}return(hash>>>0).toString(36)}
export const listCursor=(row:ListCursor,input:Listed)=>encode([fingerprint(listScope(input)),row.date,row.createdAt,row.id])
export const searchCursor=(row:SearchCursor,input:Pick<NoteSearchInput,"terms"|"filter"|"trash">)=>encode([fingerprint(searchScope(input)),row.score,row.date,row.createdAt,row.id])
function parseLimit(text:string|undefined,fallback:number,maximum:number) {
  if(text===undefined)return fallback
  const value=Number(text)
  if(!Number.isInteger(value)||value<1)throw new InvalidPageRequest("The page size is not valid")
  return Math.min(value,maximum)
}
export function parseListInput(query:{limit?:string;cursor?:string;filter?:NoteListFilter;trash?:NoteTrashScope}):NoteListInput {
  const filter=query.filter??"all",trash=query.trash??"include"
  let cursor:ListCursor|null=null
  if(query.cursor!==undefined){
    const value=decode(query.cursor)
    if(value.length!==4||value[0]!==fingerprint(listScope({filter,trash})))throw new InvalidPageRequest("The page cursor belongs to another list")
    cursor=position(value[1],value[2],value[3])
  }
  return{limit:parseLimit(query.limit,SUMMARY_PAGE_DEFAULT,SUMMARY_PAGE_MAX),cursor,filter,trash}
}
export function parseSearchInput(query:{q:string;limit?:string;cursor?:string;filter?:NoteListFilter;trash?:NoteTrashScope}):NoteSearchInput {
  const filter=query.filter??"all",trash=query.trash??"include",terms=noteSearchTerms(query.q)
  let cursor:SearchCursor|null=null
  if(query.cursor!==undefined){
    const value=decode(query.cursor)
    if(value.length!==5||value[0]!==fingerprint(searchScope({terms,filter,trash})))throw new InvalidPageRequest("The page cursor belongs to another search")
    if(typeof value[1]!=="number"||!Number.isInteger(value[1])||value[1]<0||value[1]>1_000_000)throw new InvalidPageRequest("The page cursor is not valid")
    cursor={score:value[1],...position(value[2],value[3],value[4])}
  }
  return{query:phrase(query.q),terms,limit:parseLimit(query.limit,SEARCH_PAGE_DEFAULT,SEARCH_PAGE_MAX),cursor,filter,trash}
}

// ── Row content ────────────────────────────────────────────────────────────

/** Markdown read as a sentence: structure characters removed, whitespace collapsed. */
export function plainText(markdown:string):string {
  return markdown.replace(/```[^\n]*\n?/g," ").replace(/\[([^\]\n]+)\]\([^\s)]+\)/g,"$1").replace(/^\s{0,3}(?:#{1,6}|>|[-*•]|\d+[.)])\s+/gm,"").replace(/[*_`]+/g,"").replace(/\s+/g," ").trim()
}
/** Markdown as running prose: headings dropped, each list item or paragraph ending as a sentence. */
function proseText(markdown:string):string {
  const lines:string[]=[]
  let fenced=false
  for(const raw of markdown.split(/\r\n?|\n/)){
    if(/^\s*```/.test(raw)){fenced=!fenced;continue}
    if(fenced||/^\s{0,3}#{1,6}\s/.test(raw)||/^\s*(-{3,}|\*{3,}|_{3,})\s*$/.test(raw))continue
    const line=plainText(raw)
    if(line)lines.push(/[.!?:;,…]$/.test(line)?line:`${line}.`)
  }
  return lines.join(" ")
}
const wholeCharacters=(text:string,end:number)=>end<text.length&&end>0&&/[\uD800-\uDBFF]/.test(text[end-1]!)?end-1:end
/** The first lines of a note. `complete` says whether the text given is the whole
 * document; a cut-off last line is dropped rather than shown as a sentence. */
export function noteExcerpt(enhanced:string,body:string,complete=true):string {
  const whole=(text:string)=>complete||text.length<EXCERPT_SOURCE_CHARACTERS?text:text.slice(0,Math.max(text.lastIndexOf("\n"),0)||text.length)
  const text=proseText(whole(enhanced))||proseText(whole(body))
  if(text.length<=EXCERPT_CHARACTERS)return text
  const cut=text.lastIndexOf(" ",EXCERPT_CHARACTERS)
  return`${text.slice(0,wholeCharacters(text,cut>EXCERPT_CHARACTERS*0.6?cut:EXCERPT_CHARACTERS)).replace(/[\s.,;:–-]+$/,"")}…`
}

export function initials(name:string):string {
  const words=name.trim().split(/[\s._@-]+/).filter(Boolean)
  const first=words[0]??"",last=words.length>1?words[words.length-1]!:""
  const letters=[...first].slice(0,1).join("")+(last?[...last].slice(0,/^\d+$/.test(last)?2:1).join(""):"")
  return letters.toLocaleUpperCase()||"?"
}
type Participant=MeetingContextDTO["participants"][number]
const participantName=(person:Pick<Participant,"displayName"|"email">)=>person.displayName?.trim()||person.email?.trim()||"Unnamed participant"
const clip=(text:string,maximum:number)=>text.length<=maximum?text:text.slice(0,wholeCharacters(text,maximum))
/** Stored context is JSON written by this server; anything that no longer decodes is treated as no meeting. */
export function decodeMeeting(value:unknown):MeetingContextDTO|null {
  if(value===null||value===undefined)return null
  const result=Schema.decodeUnknownOption(MeetingContextDTO)(value)
  return result._tag==="Some"?result.value:null
}
export function attendeeSummary(meeting:MeetingContextDTO|null):NoteAttendeesDTO {
  const invited=(meeting?.participants??[]).filter(person=>!person.isResource),others=invited.filter(person=>!person.isSelf)
  return{people:others.slice(0,ATTENDEES_SHOWN).map(person=>{
    const name=participantName(person)
    return{name:clip(name,254),shortName:clip(person.givenName?.trim()||name.split(/[\s@]/)[0]||name,254),initials:clip(initials(name),8)}
  }),others:others.length,total:invited.length}
}
/** Names and addresses a search can match, in invitation order. */
export const attendeeNames=(meeting:MeetingContextDTO|null)=>(meeting?.participants??[]).filter(person=>!person.isResource).flatMap(person=>[person.displayName?.trim()??"",person.email?.trim()??""]).filter(Boolean)

export interface NoteSummarySource {
  id:string;title:string;date:string;duration:string;createdAt:string;updatedAt:string;recordingState:string;generationState:string
  transcriptRevision:number;generatedRevision:number|null;hasBody:boolean;hasSummary:boolean;speakerCount:number
  /** The opening of each document, or all of it when `complete`. */
  enhanced:string;body:string;complete:boolean
  meeting:MeetingContextDTO|null;organizationId:string|null;trashed:boolean
}
const decodeSummary=Schema.decodeUnknownSync(NoteSummaryDTO)
export function noteSummary(source:NoteSummarySource):NoteSummaryDTO {
  const meeting=source.meeting
  return decodeSummary({
    id:source.id,title:source.title,date:source.date,duration:source.duration,createdAt:source.createdAt,updatedAt:source.updatedAt,
    recordingState:source.recordingState,generationState:source.generationState,transcriptRevision:source.transcriptRevision,generatedRevision:source.generatedRevision,
    hasBody:source.hasBody,hasSummary:source.hasSummary,hasTranscript:source.speakerCount>0,speakerCount:source.speakerCount,
    excerpt:noteExcerpt(source.enhanced,source.body,source.complete),
    meeting:meeting?{startAt:meeting.startAt,endAt:meeting.endAt,allDay:meeting.allDay}:null,
    attendees:attendeeSummary(meeting),organizationId:source.organizationId,trashed:source.trashed,
  })
}
/** What a repository without its own calendar data cannot know about a note. */
export interface NoteExtras {meeting?:MeetingContextDTO|null;organizationId?:string|null;trashed?:boolean}
const spoken=(segment:NoteDTO["transcriptSegments"][number])=>segment.text.trim().length>0
export function summarizeNote(note:NoteDTO,extras:NoteExtras={}):NoteSummaryDTO {
  // The preview is read without the chat link line (transcript-link.ts).
  return noteSummary({...forContext(note),hasBody:note.body.trim().length>0,hasSummary:note.enhanced.trim().length>0,
    speakerCount:new Set(note.transcriptSegments.filter(spoken).map(segment=>`${segment.channel}:${segment.speakerId}`)).size,
    complete:true,meeting:extras.meeting??null,organizationId:extras.organizationId??null,trashed:extras.trashed??false})
}
/** Calendar context is saved separately from the note. This puts it on a summary that was made without it. */
export const withMeeting=(summary:NoteSummaryDTO,meeting:MeetingContextDTO|null):NoteSummaryDTO=>({...summary,meeting:meeting?{startAt:meeting.startAt,endAt:meeting.endAt,allDay:meeting.allDay}:null,attendees:attendeeSummary(meeting)})

// ── Ordering and the in-memory list ────────────────────────────────────────

/** Newest meeting first. Creation time and then the ID break ties, so the order is total and a cursor is exact. */
export function newestFirst(a:ListCursor,b:ListCursor):number {
  return a.date!==b.date?(a.date<b.date?1:-1):a.createdAt!==b.createdAt?(a.createdAt<b.createdAt?1:-1):a.id===b.id?0:a.id<b.id?1:-1
}
const matchesFilter=(note:NoteDTO,input:Listed,extras:(id:string)=>NoteExtras)=>
  (input.trash==="include"||(extras(note.id).trashed??false)===(input.trash==="only"))
  &&(input.filter==="all"||(input.filter==="summarized"?note.enhanced.trim().length>0:note.transcriptSegments.some(spoken)))
const decodeSummaries=Schema.decodeUnknownSync(NoteSummariesPageDTO),decodeSearch=Schema.decodeUnknownSync(NoteSearchPageDTO)
export function selectNoteSummaries(notes:ReadonlyArray<NoteDTO>,input:NoteListInput,extras:(id:string)=>NoteExtras=()=>({})):NoteSummariesPageDTO {
  const matching=notes.filter(note=>matchesFilter(note,input,extras)).sort(newestFirst)
  const after=input.cursor?matching.filter(note=>newestFirst(input.cursor!,note)<0):matching
  const page=after.slice(0,input.limit),last=page[page.length-1]
  return decodeSummaries({notes:page.map(note=>summarizeNote(note,extras(note.id))),nextCursor:after.length>input.limit&&last?listCursor(last,input):null,total:matching.length})
}

// ── Search ─────────────────────────────────────────────────────────────────

/** Same field weights as `selectMeetings`; attendee names weigh like the summary. */
export const FIELD_WEIGHT:Readonly<Record<NoteSearchField,number>>={title:4,summary:2,notes:1,transcript:1,people:2}
/** Added once when the words appear together, in order, in the title or in the written note. */
export const PHRASE_WEIGHT={title:6,document:2} as const
export const SEARCH_FIELDS:ReadonlyArray<NoteSearchField>=["title","summary","notes","transcript","people"]
const phrase=(query:string)=>query.normalize("NFKC").toLowerCase().trim().replace(/\s+/g," ")
/** Up to eight distinct words, lowercased. A lone ASCII letter or digit matches nearly
 * everything and is dropped; a single character of another script is a word. */
export function noteSearchTerms(query:string):string[] {
  const words=[...phrase(query).matchAll(/[\p{L}\p{N}][\p{L}\p{N}'’._@+-]*/gu)].map(match=>match[0].replace(/['’._@+-]+$/,"").slice(0,80))
  return[...new Set(words.filter(word=>word.length>1||/[^\x00-\x7f]/.test(word)))].slice(0,8)
}
/** Text split around every occurrence of any term, longest term first. */
export function highlightParts(text:string,terms:ReadonlyArray<string>):NoteSearchTextPartDTO[] {
  if(!text)return[]
  const lower=text.toLowerCase(),ordered=[...terms].sort((a,b)=>b.length-a.length)
  // A character whose lowercase form has another length would shift every offset after it.
  if(!ordered.length||lower.length!==text.length)return[{text:clip(text,600),hit:false}]
  const parts:NoteSearchTextPartDTO[]=[]
  let cursor=0,plain=0
  while(cursor<text.length){
    const term=ordered.find(candidate=>lower.startsWith(candidate,cursor))
    if(!term){cursor++;continue}
    if(parts.length>=PARTS_MAXIMUM-2)break
    if(cursor>plain)parts.push({text:text.slice(plain,cursor),hit:false})
    parts.push({text:text.slice(cursor,cursor+term.length),hit:true})
    cursor+=term.length;plain=cursor
  }
  if(plain<text.length)parts.push({text:text.slice(plain),hit:false})
  return parts.map(part=>({...part,text:clip(part.text,600)}))
}
/** About two lines of `text` around the first match, cut at word boundaries. */
export function excerptParts(text:string,terms:ReadonlyArray<string>,radius=70):NoteSearchTextPartDTO[]|null {
  const lower=text.toLowerCase()
  if(lower.length!==text.length)return null
  let first=-1
  for(const term of terms){const at=lower.indexOf(term);if(at>=0&&(first<0||at<first))first=at}
  if(first<0)return null
  let start=Math.max(0,first-radius),end=Math.min(text.length,first+radius*2)
  if(start>0){const space=text.indexOf(" ",start);if(space>=0&&space<first)start=space+1;else if(/[\uDC00-\uDFFF]/.test(text[start]!))start++}
  if(end<text.length){const space=text.lastIndexOf(" ",end);end=space>first?space:wholeCharacters(text,end)}
  return highlightParts(`${start>0?"… ":""}${text.slice(start,end)}${end<text.length?" …":""}`,terms)
}
export interface NoteSearchSources {title:string;enhanced:string;body:string;people:ReadonlyArray<string>;
  /** The earliest transcript segment containing a term, when the transcript matched. */
  segment:{id:string;text:string;startMs:number}|null}
/** How a match is shown: the title with hits marked, and the first place outside the title that explains it. */
export function searchResult(note:NoteSummaryDTO,matchedIn:ReadonlyArray<NoteSearchField>,sources:NoteSearchSources,terms:ReadonlyArray<string>):NoteSearchResultDTO {
  let excerpt:NoteSearchExcerptDTO|null=null
  const written=(source:"summary"|"notes",text:string)=>{const parts=matchedIn.includes(source)?excerptParts(plainText(text),terms):null;return parts?{source,parts,segmentId:null,startMs:null}:null}
  excerpt=written("summary",sources.enhanced)??written("notes",sources.body)
  if(!excerpt&&sources.segment){const parts=excerptParts(sources.segment.text.replace(/\s+/g," ").trim(),terms);if(parts)excerpt={source:"transcript",parts,segmentId:sources.segment.id,startMs:sources.segment.startMs}}
  if(!excerpt&&matchedIn.includes("people")){
    const named=sources.people.filter(name=>terms.some(term=>name.toLowerCase().includes(term))).slice(0,3)
    if(named.length)excerpt={source:"people",parts:highlightParts(clip(named.join(", "),400),terms),segmentId:null,startMs:null}
  }
  return{note,matchedIn:SEARCH_FIELDS.filter(field=>matchedIn.includes(field)),title:highlightParts(note.title,terms),excerpt}
}
/** Which fields each term is in. A note matches when every term is in at least one. */
export function scoreNote(fields:Record<NoteSearchField,string>,input:Pick<NoteSearchInput,"query"|"terms">):{score:number;matchedIn:NoteSearchField[]}|null {
  const matched=new Set<NoteSearchField>()
  let score=0
  for(const term of input.terms){
    let found=false
    for(const field of SEARCH_FIELDS)if(fields[field].includes(term)){found=true;matched.add(field);score+=FIELD_WEIGHT[field]}
    if(!found)return null
  }
  if(input.terms.length>1){
    if(fields.title.includes(input.query))score+=PHRASE_WEIGHT.title
    if(fields.summary.includes(input.query)||fields.notes.includes(input.query))score+=PHRASE_WEIGHT.document
  }
  return{score,matchedIn:[...matched]}
}
export function selectNoteSearch(notes:ReadonlyArray<NoteDTO>,input:NoteSearchInput,extras:(id:string)=>NoteExtras=()=>({})):NoteSearchPageDTO {
  if(!input.terms.length)return{results:[],nextCursor:null,total:0,terms:[]}
  // Matched, and quoted, without the chat link line (transcript-link.ts).
  const scored=notes.filter(note=>matchesFilter(note,input,extras)).map(forContext).flatMap(note=>{
    const meeting=extras(note.id).meeting??null,people=attendeeNames(meeting)
    const match=scoreNote({title:note.title.toLowerCase(),summary:note.enhanced.toLowerCase(),notes:note.body.toLowerCase(),transcript:note.transcriptSegments.map(segment=>segment.text.toLowerCase()).join("\n"),people:people.join("\n").toLowerCase()},input)
    return match?[{note,people,...match,date:note.date,createdAt:note.createdAt,id:note.id}]:[]
  }).sort((a,b)=>b.score-a.score||newestFirst(a,b))
  const cursor=input.cursor
  const after=cursor?scored.filter(row=>row.score<cursor.score||row.score===cursor.score&&newestFirst(cursor,row)<0):scored
  const page=after.slice(0,input.limit),last=page[page.length-1]
  return decodeSearch({
    results:page.map(row=>{
      const segment=row.matchedIn.includes("transcript")?[...row.note.transcriptSegments].sort((a,b)=>a.startMs-b.startMs||a.id.localeCompare(b.id)).find(candidate=>input.terms.some(term=>candidate.text.toLowerCase().includes(term)))??null:null
      return searchResult(summarizeNote(row.note,extras(row.note.id)),row.matchedIn,{title:row.note.title,enhanced:row.note.enhanced,body:row.note.body,people:row.people,segment},input.terms)
    }),
    nextCursor:after.length>input.limit&&last?searchCursor(last,input):null,total:scored.length,terms:[...input.terms],
  })
}
