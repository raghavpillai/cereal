import type {Effect} from "effect"
import type {MeetingContextDTO,CalendarEventDTO,GoogleCalendarDTO,GoogleStatusDTO,NoteContextDTO,NoteDTO} from "@meeting-notes/contracts"
import type {ApiError,PrincipalData} from "@meeting-notes/contracts/api"
/** Request boundary for personal Google data. Instance-shared ICS sources remain
 * in AdministrationService; every Google operation requires an account ID. */
export interface GoogleIntegration {
 getStatus(userId:string):Effect.Effect<GoogleStatusDTO,ApiError>
 beginConnect(userId:string,browserSessionKey:string,contacts?:boolean):Effect.Effect<{authorizationUrl:string},ApiError>
 callback(request:Request,principal:PrincipalData):Promise<Response>
 listCalendars(userId:string,connectionId:string):Effect.Effect<ReadonlyArray<GoogleCalendarDTO>,ApiError>
 selectCalendar(userId:string,calendarId:string,selected:boolean):Effect.Effect<GoogleCalendarDTO,ApiError>
 syncConnection(userId:string,connectionId:string):Effect.Effect<GoogleStatusDTO,ApiError>
 disconnect(userId:string,connectionId:string):Effect.Effect<{deleted:boolean},ApiError>
 listEvents(userId:string,query:{from:string;to:string}):Effect.Effect<ReadonlyArray<CalendarEventDTO>,ApiError>
 createEventNote(userId:string,eventId:string):Effect.Effect<NoteDTO|null,ApiError>
 initializeExternalContext(userId:string,noteId:string,meeting:MeetingContextDTO):Effect.Effect<NoteContextDTO,ApiError>
 putExternalContext(userId:string,noteId:string,meeting:MeetingContextDTO|null):Effect.Effect<NoteContextDTO,ApiError>
 getNoteContext(userId:string,noteId:string):Effect.Effect<NoteContextDTO,ApiError>
 setNoteContext(userId:string,noteId:string,eventId:string|null):Effect.Effect<NoteContextDTO,ApiError>
 dispose():Promise<void>
}
