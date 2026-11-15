/**
 * Citation integrity shared by every chat feature: markers the model may emit
 * are limited to server-issued IDs, and each used citation is re-read from the
 * repository before the answer completes.
 */
import {Effect} from "effect"
import type {ChatCitationDTO,NoteDTO} from "@meeting-notes/contracts"
import type {NotesRepositoryService} from "../repository"

/** The most distinct markers one answer may carry: `ChatStreamEventDTO.citations`
 * holds at most this many, and an event with more would fail to encode. */
export const MAX_ANSWER_CITATIONS=40

export interface CitationCheck {
  /** Citations whose own passage is still where it was, in the order given. */
  valid:ChatCitationDTO[]
  /** A cited note can no longer be read through this repository at all: it was
   * deleted, moved out of the workspace, or never belonged to the caller. */
  withdrawn:boolean
}

/** Re-read each source through the scoped repository and judge every citation
 * on its own passage. No model-supplied note ID, offset, quotation, or revision
 * is accepted as citation metadata.
 *
 *  - transcript: the cited segment still exists, is final, starts at the cited
 *    time, and its text still contains the quote;
 *  - typed notes and generated notes: no segment or time is attached and the
 *    document still contains the quote.
 *
 * Nothing else about the note is compared. In particular the note-wide
 * transcript revision moving forward (speech appended during a meeting, an
 * unrelated edit) does not invalidate a citation whose passage is untouched.
 * The revision is only checked for being possible: a citation cannot have been
 * issued at a revision the note has not reached, and a transcript passage
 * cannot have been cited at revision 0, which means "no transcript". */
export async function checkChatCitations(citations:ReadonlyArray<ChatCitationDTO>,repository:NotesRepositoryService,signal:AbortSignal):Promise<CitationCheck> {
  const notes=new Map<string,NoteDTO|null>(),valid:ChatCitationDTO[]=[]
  let withdrawn=false
  for(const citation of citations){
    signal.throwIfAborted()
    if(!notes.has(citation.noteId))notes.set(citation.noteId,await Effect.runPromise(repository.getNote(citation.noteId),{signal}))
    const note=notes.get(citation.noteId)
    if(!note){withdrawn=true;continue}
    if(citation.transcriptRevision>note.transcriptRevision)continue
    if(citation.source==="transcript"){
      const segment=citation.segmentId?note.transcriptSegments.find(s=>s.id===citation.segmentId):undefined
      if(citation.transcriptRevision<1||!segment||!segment.isFinal||segment.startMs!==citation.startMs||!segment.text.includes(citation.quote))continue
    }else if(citation.segmentId!==null||citation.startMs!==null||!(citation.source==="notes"?note.body:note.enhanced).includes(citation.quote))continue
    valid.push({...citation,title:note.title})
  }
  return {valid,withdrawn}
}
/** The citations that still resolve; see `checkChatCitations` for the rule. */
export const validateChatCitations=async(citations:ReadonlyArray<ChatCitationDTO>,repository:NotesRepositoryService,signal:AbortSignal):Promise<ChatCitationDTO[]>=>(await checkChatCitations(citations,repository,signal)).valid

/** Hold incomplete citation markers across real deltas; unknown IDs are dropped,
 * never turned into clickable client references. Ordinary text still streams.
 * Once `limit` distinct IDs have been used, any further new ID is dropped the
 * same way, so the citations event of a long answer always fits the contract. */
export class CitationStreamFilter {
  private pending=""
  readonly used=new Set<string>()
  constructor(private readonly allowed:ReadonlySet<string>,private readonly limit=MAX_ANSWER_CITATIONS){}
  push(delta:string,final=false):string {
    this.pending+=delta
    const opening=this.pending.lastIndexOf("[[")
    let cut=this.pending.length
    if(!final&&opening>=0&&this.pending.indexOf("]]",opening)<0&&/^\[\[S?[0-9]{0,6}\]?$/.test(this.pending.slice(opening)))cut=opening
    else if(!final&&this.pending.endsWith("["))cut=this.pending.length-1
    const text=this.pending.slice(0,cut);this.pending=this.pending.slice(cut)
    return text.replace(/\[\[(S[0-9]+)\]\]/g,(_,id:string)=>{if(!this.allowed.has(id)||!this.used.has(id)&&this.used.size>=this.limit)return"";this.used.add(id);return`[[${id}]]`})
  }
}
