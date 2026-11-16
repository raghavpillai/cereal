/**
 * The source summary, streamed (POST /api/v1/notes/:id/evidence/stream). The same question as the buffered
 * `evidence` route (`processing.ts`) and the same checks, with the summary sent as the model writes it:
 *
 *   delta*  the summary's text, in order (`ai/evidence.ts` reads it out of the JSON answer as it arrives)
 *   done    the whole result, read and validated once the answer has ended: the summary as finally parsed,
 *           and the quotations, each the exact text of a segment the model named and the transcript has
 *   error   instead of done, when anything fails. Nothing streamed before it is to be kept. An answer
 *           the checks refuse says so ("Couldn't match this to the transcript"); it is not a load failure.
 *
 * What is shown while it streams: only the characters of the top-level object's `summary` string, decoded,
 * as soon as the reader is inside it. Prose or a fence before the object, other keys and nested values are
 * never sent. Whether the whole object is well formed and grounded can only be known once it has ended, so
 * holding text back until then would be the buffered route again; instead the client withdraws the
 * streamed text, visibly, when the final check refuses it. A summary longer than the 20,000 characters the
 * result allows is refused as soon as it passes that length, and the model is stopped there.
 *
 * Grounding is unchanged: a segment ID the transcript does not have, more than 8 of them, or a transcript
 * that changed while the model read it ends the stream with an error, never with a result. A provider that
 * cannot stream (no `evidenceStream`, or a stream refused before its first token) is asked the buffered
 * way, and its summary arrives as one delta. Closing the request aborts the provider request.
 */
import {Cause,Effect,Stream} from "effect"
import type {EvidenceDTO,EvidenceRequestDTO,EvidenceStreamEventDTO,NoteDTO} from "@meeting-notes/contracts"
import type {ApiError} from "@meeting-notes/contracts/api"
import {ProviderFailure,type ProviderServices} from "./providers"
import {EvidenceSummaryReader,readEvidence} from "./ai/evidence"

const invalid=(message:string):ApiError=>({error:"invalid_request",message})
const unavailable:ApiError={error:"unavailable",message:"Processing is temporarily unavailable"}
const FAILED="Transcript context could not be loaded. Try again."
const REJECTED="Couldn't match this to the transcript. Try again."
/** The longest summary a result may have (`readEvidence`, `EvidenceDTO`). */
const SUMMARY_LIMIT=20_000
/** The model answered, and the answer is not one that can be shown: malformed, too long, or not grounded. */
class Rejected extends Error {}
const CHANGED="Transcript changed; reopen the explanation"
/** Longest a source summary may take, first token to last. */
const DEADLINE_MS=120_000

export const NO_TRANSCRIPT_SUMMARY="No supporting transcript is available for this note."

export function makeEvidenceStream(dependencies:{
  getNote:(id:string)=>Effect.Effect<NoteDTO,ApiError>
  providers:ProviderServices
  requestSignal?:AbortSignal|undefined
}) {
  const {getNote,providers}=dependencies
  return (id:string,input:EvidenceRequestDTO)=>Effect.gen(function*(){
    const note=yield* getNote(id)
    if(note.transcriptRevision!==input.expectedTranscriptRevision)return yield* Effect.fail(invalid(CHANGED))
    if(!note.transcriptSegments.some(segment=>segment.text.trim())){
      const evidence:EvidenceDTO={summary:NO_TRANSCRIPT_SUMMARY,quotes:[],transcriptRevision:note.transcriptRevision}
      const done:EvidenceStreamEventDTO={type:"done",text:"",evidence}
      return Stream.make(done)
    }
    if(!providers.evidenceStream&&!providers.evidence)return yield* Effect.fail(unavailable)
    if(note.transcriptSegments.reduce((sum,segment)=>sum+segment.text.length,0)>500_000)return yield* Effect.fail(invalid("Transcript is too large for an explanation"))
    const abort=new AbortController()
    const signal=AbortSignal.any([abort.signal,AbortSignal.timeout(DEADLINE_MS),...(dependencies.requestSignal?[dependencies.requestSignal]:[])])
    const readable=new ReadableStream<EvidenceStreamEventDTO>({
      start(output){void (async()=>{
        const emit=(event:EvidenceStreamEventDTO)=>{if(!signal.aborted)output.enqueue(event)}
        try{
          let raw="",streamed=0
          if(providers.evidenceStream){
            const reader=new EvidenceSummaryReader()
            // Stopping the model for an over-long summary is not the client going away.
            const model=new AbortController()
            try{
              for await(const piece of providers.evidenceStream(note,input.paragraph,AbortSignal.any([model.signal,signal]))){
                signal.throwIfAborted()
                raw+=piece
                const text=reader.push(piece)
                if(!text)continue
                streamed+=text.length
                if(streamed>SUMMARY_LIMIT){model.abort();throw new Rejected("Summary too long")}
                emit({type:"delta",text})
              }
            }catch(error){
              // Refused before anything was shown (a provider or server that does not stream JSON): ask once
              // more the buffered way. Once text is on screen, a failure is the answer's failure.
              if(error instanceof Rejected||signal.aborted||streamed||raw||!providers.evidence)throw error
            }
          }
          let result:{summary:string;segmentIds:ReadonlyArray<string>}|null=raw?readEvidence(raw,note):null
          if(!raw){
            if(!providers.evidence)throw new Error("Empty source summary")
            const answered=await Effect.runPromiseExit(providers.evidence(note,input.paragraph),{signal})
            if(answered._tag==="Failure"){
              const failure=Cause.squash(answered.cause)
              throw failure instanceof ProviderFailure&&failure.reason==="invalid_response"?new Rejected("Refused answer"):failure
            }
            result=answered.value
            emit({type:"delta",text:result.summary})
          }
          signal.throwIfAborted()
          if(!result)throw new Rejected("Invalid source summary")
          const latest=await Effect.runPromise(getNote(id),{signal})
          if(latest.transcriptRevision!==note.transcriptRevision){emit({type:"error",text:CHANGED});output.close();return}
          const quotes=result.segmentIds.map(segmentId=>{
            const segment=note.transcriptSegments.find(candidate=>candidate.id===segmentId)
            return segment?{segmentId,text:segment.text,speakerLabel:segment.speakerLabel,startMs:segment.startMs}:undefined
          })
          if(quotes.some(quote=>!quote)||quotes.length>8)throw new Rejected("Unissued segment")
          const evidence:EvidenceDTO={summary:result.summary,quotes:quotes.filter((quote):quote is NonNullable<typeof quote>=>quote!==undefined),transcriptRevision:note.transcriptRevision}
          emit({type:"done",text:"",evidence});output.close()
        }catch(error){
          if(!abort.signal.aborted&&!dependencies.requestSignal?.aborted){output.enqueue({type:"error",text:error instanceof Rejected?REJECTED:FAILED});output.close()}
        }
      })()},
      cancel(){abort.abort()},
    })
    return Stream.fromReadableStream({evaluate:()=>readable,onError:()=>unavailable}).pipe(
      Stream.ensuring(Effect.sync(()=>abort.abort())),
      Stream.catch(()=>Stream.make({type:"error" as const,text:FAILED})),
    )
  })
}
