import type {NoteDTO} from "@meeting-notes/contracts"

export interface MeetingSearchInput {query:string;from?:string;to?:string;limit:number}
export interface TranscriptSearchInput extends MeetingSearchInput {noteId?:string}
export interface TranscriptSearchHit {noteId:string;title:string;date:string;transcriptRevision:number;segment:NoteDTO["transcriptSegments"][number]}
const ignored=new Set(["what","when","where","which","with","from","have","this","that","there","about","meeting","meetings","please","recent","latest","show","find","search","list","notes","across","were","been","your","their","they","them","does","would","could","should","todo","todos"])
export function meetingSearchTerms(query:string):string[] {
  const terms=[...query.normalize("NFKC").toLowerCase().matchAll(/[\p{L}\p{N}][\p{L}\p{N}'’-]{1,79}/gu)].map(m=>m[0]).filter(term=>!ignored.has(term))
  if (/\b(to-?dos?|action items?|next steps?|follow.?ups?)\b/i.test(query)) terms.push("action item","next step","follow-up","todo","to-do")
  return [...new Set(terms)].slice(0,12)
}
export function selectMeetings(notes:ReadonlyArray<NoteDTO>,input:MeetingSearchInput):NoteDTO[] {
  const terms=meetingSearchTerms(input.query)
  return notes.filter(note=>(!input.from||note.date>=input.from)&&(!input.to||note.date<=input.to))
    .map(note=>{
      const title=note.title.toLowerCase(),summary=note.enhanced.toLowerCase(),body=note.body.toLowerCase(),transcript=note.transcriptSegments.map(s=>s.text).join("\n").toLowerCase()
      return {note,score:terms.reduce((score,term)=>score+(title.includes(term)?4:0)+(summary.includes(term)?2:0)+(body.includes(term)?1:0)+(transcript.includes(term)?1:0),0)}
    }).filter(result=>!terms.length||result.score>0)
    .sort((a,b)=>b.score-a.score||b.note.date.localeCompare(a.note.date)||b.note.updatedAt.localeCompare(a.note.updatedAt))
    .slice(0,Math.min(20,Math.max(1,input.limit))).map(({note})=>note)
}
/** Exact substring, with a little context around a matching term. Avoid cutting
 * UTF-16 surrogate pairs while retaining byte-for-byte source identity. */
export function transcriptPassage(text:string,terms:ReadonlyArray<string>,maximum=1600):string {
  const lower=text.toLowerCase(),positions:number[]=[0],weights=new Map<string,number>()
  for(const term of terms){let from=0,count=0;for(;count<20;count++){const at=lower.indexOf(term,from);if(at<0)break;positions.push(Math.max(0,at-Math.min(240,maximum/4)));from=at+term.length}weights.set(term,term.length/(1+count))}
  let start=positions.map(position=>({position,score:terms.reduce((sum,term)=>sum+(lower.slice(position,position+maximum).includes(term)?weights.get(term)??0:0),0)})).sort((a,b)=>b.score-a.score||a.position-b.position)[0]!.position
  let end=Math.min(text.length,start+maximum)
  if(start>0&&/[\uDC00-\uDFFF]/.test(text[start]!))start--
  if(end<text.length&&/[\uD800-\uDBFF]/.test(text[end-1]!))end--
  return text.slice(start,end)
}
export function selectTranscriptPassages(notes:ReadonlyArray<NoteDTO>,input:TranscriptSearchInput):TranscriptSearchHit[] {
  const terms=meetingSearchTerms(input.query)
  if(!terms.length)return[]
  return notes.filter(note=>(!input.noteId||note.id===input.noteId)&&(!input.from||note.date>=input.from)&&(!input.to||note.date<=input.to))
    .sort((a,b)=>b.date.localeCompare(a.date)||b.updatedAt.localeCompare(a.updatedAt))
    .flatMap(note=>note.transcriptSegments.filter(segment=>terms.some(term=>segment.text.toLowerCase().includes(term)))
      .sort((a,b)=>a.startMs-b.startMs).map(segment=>({noteId:note.id,title:note.title,date:note.date,transcriptRevision:note.transcriptRevision,segment:{...segment,text:transcriptPassage(segment.text,terms)}})))
    .slice(0,Math.min(20,Math.max(1,input.limit)))
}
