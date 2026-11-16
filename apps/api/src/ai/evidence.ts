/**
 * "Explain this paragraph" (POST /api/v1/notes/:id/evidence): asks the model
 * which transcript segments support a paragraph of the generated notes. The
 * response is validated against real segment IDs in `providers.ts` and
 * `processing.ts`. Closest in ownership to notes generation.
 */
import type {ProviderNote} from "../providers"
import {baseSystemPrompt,transcriptContext,type LanguageMessage,type LanguageRequestParameters} from "./shared"

/** One request per paragraph a user clicks, usually several on the same note within minutes: the transcript is
 * the part they share, so it is its own message, marked for the prompt cache, and the paragraph follows it. */
export const EVIDENCE_PARAMETERS:LanguageRequestParameters={maxTokens:4096,temperature:0.2,cache:{ttl:"5m",tail:false}}
export const EVIDENCE_INSTRUCTIONS=
  " Explain the selected note paragraph using only the supplied transcript. Return JSON with summary (a short explanation) and segmentIds (up to 8 exact transcript segment IDs supporting it). Choose no IDs and explain the lack of evidence if unsupported. Do not invent quotations or IDs."
export const evidenceMessages=(note:ProviderNote,paragraph:string):LanguageMessage[]=>[
  { role: "system", content: baseSystemPrompt(note.transcriptSegments) + EVIDENCE_INSTRUCTIONS },
  { role: "user", content: JSON.stringify({ transcript: note.transcriptSegments.map(transcriptContext) }), cache: true },
  { role: "user", content: JSON.stringify({ paragraph }) },
]

/** What the model answered, checked as the buffered request checks it (`providers.ts`): a summary of bounded
 * length and at most 8 segment IDs, every one issued in this note's transcript. A streamed answer is read
 * whole once it ends, so what the reader showed is never trusted on its own. A provider that ignores JSON
 * mode may fence the object; only the object is read. Null when the answer is not usable. */
export function readEvidence(raw:string,note:ProviderNote):{summary:string;segmentIds:string[]}|null {
  const start=raw.indexOf("{"),end=raw.lastIndexOf("}")
  if(start<0||end<start)return null
  let value:unknown
  try{value=JSON.parse(raw.slice(start,end+1))}catch{return null}
  if(typeof value!=="object"||value===null||Array.isArray(value))return null
  const {summary,segmentIds}=value as Record<string,unknown>
  if(typeof summary!=="string"||!summary.trim()||summary.length>20_000||!Array.isArray(segmentIds)||segmentIds.length>8)return null
  const issued=new Set(note.transcriptSegments.map(segment=>segment.id))
  if(!segmentIds.every(id=>typeof id==="string"&&issued.has(id)))return null
  return {summary,segmentIds:[...new Set(segmentIds as string[])]}
}

/**
 * The text of the top-level `summary` string of a JSON object that is still being written, as it arrives.
 * `push` takes the next piece of the model's answer and returns the summary characters it completed:
 * escapes are decoded only once whole, and half of a surrogate pair waits for the other half, so every
 * piece returned is valid text. Anything else in the object (the segment IDs, other keys, nesting) is
 * skipped; a second `summary` key is not streamed (the whole answer, read at the end, decides).
 */
export class EvidenceSummaryReader {
  private depth=0
  private inString=false
  private escape:string|null=null
  /** At depth 1: the next string is a key. */
  private expectKey=false
  private key=""
  private lastKey=""
  /** The current string is the summary value. */
  private streaming=false
  private streamed=false
  private pendingHigh=""
  private started=false

  push(piece:string):string {
    let out=""
    const emit=(text:string)=>{
      if(!text)return
      text=this.pendingHigh+text;this.pendingHigh=""
      const last=text.charCodeAt(text.length-1)
      if(last>=0xd800&&last<=0xdbff){this.pendingHigh=text.slice(-1);text=text.slice(0,-1)}
      out+=text
    }
    for(const char of piece){
      if(!this.started){if(char==="{"){this.started=true;this.depth=1;this.expectKey=true}continue}
      if(this.depth===0)continue
      if(this.inString){
        if(this.escape!==null){
          this.escape+=char
          const decoded=this.decode(this.escape)
          if(decoded===undefined)continue
          this.escape=null
          this.take(decoded,emit)
          continue
        }
        if(char==="\\"){this.escape="";continue}
        if(char==="\""){this.endString();continue}
        this.take(char,emit)
        continue
      }
      if(char==="\""){
        this.inString=true
        if(this.depth===1&&this.expectKey)this.key=""
        else if(this.depth===1&&this.lastKey==="summary"&&!this.streamed)this.streaming=true
        continue
      }
      if(char==="{"||char==="["){this.depth++;continue}
      if(char==="}"||char==="]"){this.depth--;continue}
      if(char===","&&this.depth===1){this.expectKey=true;continue}
    }
    return out
  }
  private take(text:string,emit:(text:string)=>void){
    if(this.depth===1&&this.expectKey)this.key+=text
    else if(this.streaming)emit(text)
  }
  private endString(){
    this.inString=false
    if(this.depth!==1)return
    if(this.expectKey){this.lastKey=this.key;this.expectKey=false;return}
    if(this.streaming){this.streaming=false;this.streamed=true;if(this.pendingHigh)this.pendingHigh=""}
  }
  /** The text of an escape after its backslash, or undefined while it is incomplete. */
  private decode(escape:string):string|undefined {
    const simple:Record<string,string>={"\"":"\"","\\":"\\","/":"/",b:"\b",f:"\f",n:"\n",r:"\r",t:"\t"}
    const first=escape[0]!
    if(first!=="u")return simple[first]??""
    if(escape.length<5)return undefined
    const code=Number.parseInt(escape.slice(1,5),16)
    return Number.isNaN(code)?"":String.fromCharCode(code)
  }
}
