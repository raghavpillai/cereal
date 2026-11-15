import {Effect,Schema} from "effect"
import type {Database} from "@meeting-notes/database"
import {MeetingContextDTO,type ManualParticipantInput,type MeetingParticipantDTO,type NoteContextDTO} from "@meeting-notes/contracts"
import type {ApiError} from "@meeting-notes/contracts/api"
import type {GoogleIntegration} from "./google-integration"
import type {NotesRepositoryService} from "./repository"
class ParticipantFailure extends Error{constructor(readonly payload:ApiError){super(payload.message)}}
const invalid=(message:string)=>new ParticipantFailure({error:"invalid_request",message})
const missing=()=>new ParticipantFailure({error:"not_found",message:"Note not found"})
const boundary=<A>(task:()=>Promise<A>)=>Effect.tryPromise({try:task,catch:(e):ApiError=>e instanceof ParticipantFailure?e.payload:{error:"unavailable",message:"Meeting attendees could not be saved"}})
function updatedContext(context:MeetingContextDTO,input:ManualParticipantInput):MeetingContextDTO {
 const displayName=input.displayName.trim();if(!displayName)throw invalid("Enter an attendee name")
 const existing=context.participants.find(p=>p.id===input.id)
 if(existing&&existing.nameSource!=="manual")throw invalid("Calendar participants cannot be overwritten by a manual attendee")
 const person:MeetingParticipantDTO={id:input.id,displayName,email:input.email?.trim()??null,givenName:input.givenName?.trim()??null,familyName:input.familyName?.trim()??null,nameSource:"manual",isSelf:false,isOrganizer:false,isResource:false,responseStatus:"unknown"}
 const participants=existing?context.participants.map(p=>p.id===input.id?person:p):[...context.participants,person]
 if(participants.length>500)throw invalid("A meeting supports at most 500 attendees")
 return{...context,participants}
}
export interface NoteParticipantsService {upsert(userId:string,noteId:string,input:ManualParticipantInput):Effect.Effect<NoteContextDTO,ApiError>}
/** A stable client-generated participant UUID makes uncertain POST retries
 * idempotent. Only explicit manual names are stored; no voice identity is inferred. */
export function makeNoteParticipantsService(db:Database):NoteParticipantsService {
 return{upsert:(userId,noteId,input)=>boundary(()=>db.$transaction(async tx=>{
  const owned=await tx.$queryRawUnsafe<Array<{id:string}>>('SELECT "id" FROM "Note" WHERE "id"=$1::uuid AND "ownerId"=$2::uuid FOR UPDATE',noteId,userId)
  if(!owned.length)throw missing()
  const row=await tx.noteMeetingContext.findFirst({where:{noteId,userId}})
  if(!row)throw invalid("Associate a meeting before adding attendees")
  const meeting=updatedContext(Schema.decodeUnknownSync(MeetingContextDTO)(row.meeting),input)
  await tx.noteMeetingContext.update({where:{noteId},data:{meeting:JSON.parse(JSON.stringify(meeting))}})
  return{noteId,meeting}
 }))}
}
export function makeMemoryNoteParticipantsService(notes:NotesRepositoryService,google:GoogleIntegration):NoteParticipantsService {
 let queue=Promise.resolve()
 return{upsert:(userId,noteId,input)=>boundary(()=>{const operation=queue.then(async()=>{if(!await Effect.runPromise(notes.forUser(userId).getNote(noteId)))throw missing();const context=await Effect.runPromise(google.getNoteContext(userId,noteId));if(!context.meeting)throw invalid("Associate a meeting before adding attendees");return Effect.runPromise(google.putExternalContext(userId,noteId,updatedContext(context.meeting,input)))});queue=operation.then(()=>{},()=>{});return operation})}
}
