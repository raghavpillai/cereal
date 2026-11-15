import { Context, Schema } from "effect"
import {UploadNoteAttachmentInput,NoteAttachmentDTO,AgentBriefDTO,UpdateAgentBriefInput} from "./index"
import {ModelCatalogInput,ModelCatalogDTO,ChatModelsDTO} from "./index"
import {NoteListFilter,NoteSummariesPageDTO,NoteSearchPageDTO} from "./index"
import {AuthMethodsDTO,AppHandoffInput,AppHandoffDTO,AppHandoffExchangeInput,AppHandoffTokenDTO} from "./index"
import {DownloadsDTO} from "./index"
import {StorageTestInput,StorageTestDTO} from "./index"
import { HttpApi, HttpApiEndpoint, HttpApiGroup, HttpApiMiddleware, HttpApiSchema, HttpApiSecurity, OpenApi } from "effect/http-api"
import { OrganizationDTO, OrganizationsDTO, OrganizationMemberDTO, OrganizationInvitationDTO, OrganizationDetailsDTO, OrganizationInvitationsDTO, CreateOrganizationInput, UpdateOrganizationInput, SetActiveOrganizationInput, InviteOrganizationMemberInput, UpdateOrganizationMemberRoleInput, ManualParticipantInput, GoogleStatusDTO, GoogleConnectInput, GoogleConnectDTO, GoogleCalendarsDTO, GoogleCalendarDTO, UpdateGoogleCalendarInput, NoteContextDTO, UpdateNoteContextInput, AuthSessionDTO, UsersDTO, UserDTO, CreateUserInput, UpdateUserRoleInput, CalendarPreferencesDTO, CalendarEventsDTO, CalendarsDTO, CalendarDTO, CreateCalendarInput, UpdateCalendarInput, AdminSettingsDTO, UpdateAdminSettingsInput, Timestamp, ChatStreamEventDTO, EvidenceRequestDTO, EvidenceDTO, AudioChunkDTO, AudioChunksDTO, UploadAudioChunkInput, RecordingCommandDTO, GenerateNotesCommandDTO, ChatInput, ChatAnswerDTO, CreateNoteInput, CreateShareInput, CreatedShareDTO, DeletedDTO, HealthDTO, NoteDTO, NotesDTO, SharedNoteDTO, ShareDTO, SharesDTO, UUID, UpdateNoteInput } from "./index"

export const Unauthorized = Schema.Struct({ error: Schema.Literal("unauthorized"), message: Schema.String }).pipe(HttpApiSchema.status(401))
export const Forbidden = Schema.Struct({ error: Schema.Literal("forbidden"), message: Schema.String }).pipe(HttpApiSchema.status(403))
export const Conflict = Schema.Struct({ error: Schema.Literal("conflict"), message: Schema.String }).pipe(HttpApiSchema.status(409))
export const NotFound = Schema.Struct({ error: Schema.Literal("not_found"), message: Schema.String }).pipe(HttpApiSchema.status(404))
export const InvalidRequest = Schema.Struct({ error: Schema.Literal("invalid_request"), message: Schema.String }).pipe(HttpApiSchema.status(400))
export const Unavailable = Schema.Struct({ error: Schema.Literal("unavailable"), message: Schema.String }).pipe(HttpApiSchema.status(503))
/** The app sign-in handoff: an unknown, used, expired or mismatched code, and too many attempts. */
export const InvalidCode = Schema.Struct({ error: Schema.Literal("invalid_code"), message: Schema.String }).pipe(HttpApiSchema.status(401))
export const RateLimited = Schema.Struct({ error: Schema.Literal("rate_limited"), message: Schema.String }).pipe(HttpApiSchema.status(429))
export type ApiError = typeof Unauthorized.Type | typeof Forbidden.Type | typeof Conflict.Type | typeof NotFound.Type | typeof InvalidRequest.Type | typeof Unavailable.Type
const errors = [Unauthorized, Forbidden, Conflict, NotFound, InvalidRequest, Unavailable] as const

/** Query strings arrive as text: a page size of 1 to 999 (the server caps it) and an opaque cursor. */
const PageLimit = Schema.String.check(Schema.isPattern(/^[1-9][0-9]{0,2}$/))
const PageCursor = Schema.String.check(Schema.isMinLength(1), Schema.isMaxLength(512), Schema.isPattern(/^[A-Za-z0-9_-]+$/))

export interface PrincipalData {id:string;name:string;email:string;role:"owner"|"admin"|"user";source:"bootstrap"|"session";expiresAt:string|null;createdAt:string;sessionId?:string}
export class AuthPrincipal extends Context.Service<AuthPrincipal,PrincipalData>()("MeetingNotesAuthPrincipal") {}

/** Authenticated session or operator bootstrap credential. */
export class OwnerAuth extends HttpApiMiddleware.Service<OwnerAuth, {provides:AuthPrincipal}>()("MeetingNotesOwnerAuth", {
  error: [Unauthorized,Forbidden,Unavailable],
  security: { bearer: HttpApiSecurity.bearer },
}) {}

export const MeetingsApi = HttpApi.make("MeetingNotesApi")
  .add(HttpApiGroup.make("public")
    .add(HttpApiEndpoint.get("health", "/api/v1/health", { success: HealthDTO }))
    .add(HttpApiEndpoint.get("getAuthMethods", "/api/v1/auth/methods", { success: AuthMethodsDTO, error: [Unavailable] }))
    .add(HttpApiEndpoint.post("exchangeAppHandoff", "/api/v1/auth/app-handoff/exchange", { payload: AppHandoffExchangeInput, success: AppHandoffTokenDTO, error: [InvalidRequest, InvalidCode, RateLimited, Unavailable] }))
    // The apps for the version this server is running, and the files themselves. Both are public.
    .add(HttpApiEndpoint.get("getDownloads", "/api/v1/downloads", { success: DownloadsDTO, error: [Unavailable] }))
    // Answered outside the typed handlers (GET and HEAD, single byte ranges, an ETag, five minutes of caching);
    // listed here so the address and its answers are part of the published description. Only the files named
    // by getDownloads exist: anything else is 404.
    .add(HttpApiEndpoint.get("getDownloadFile", "/api/v1/downloads/files/:file", { params: { file: Schema.String }, success: HttpApiSchema.StreamUint8Array({ contentType: ["application/x-apple-diskimage", "application/zip"] }), error: [NotFound, Unavailable] })))
  .add(HttpApiGroup.make("owner")
    .add(HttpApiEndpoint.get("getGoogleStatus", "/api/v1/integrations/google", {success:GoogleStatusDTO,error:errors}))
    .add(HttpApiEndpoint.post("beginGoogleConnect", "/api/v1/integrations/google/connect", {payload:GoogleConnectInput,success:GoogleConnectDTO,error:errors}))
    .add(HttpApiEndpoint.get("listGoogleCalendars", "/api/v1/integrations/google/connections/:id/calendars", {params:{id:UUID},success:GoogleCalendarsDTO,error:errors}))
    .add(HttpApiEndpoint.patch("updateGoogleCalendar", "/api/v1/integrations/google/calendars/:id", {params:{id:UUID},payload:UpdateGoogleCalendarInput,success:GoogleCalendarDTO,error:errors}))
    .add(HttpApiEndpoint.post("syncGoogleConnection", "/api/v1/integrations/google/connections/:id/sync", {params:{id:UUID},success:GoogleStatusDTO,error:errors}))
    .add(HttpApiEndpoint.delete("deleteGoogleConnection", "/api/v1/integrations/google/connections/:id", {params:{id:UUID},success:DeletedDTO,error:errors}))
    .add(HttpApiEndpoint.post("upsertNoteParticipant", "/api/v1/notes/:id/context/participants", {params:{id:UUID},payload:ManualParticipantInput,success:NoteContextDTO,error:errors}))
    .add(HttpApiEndpoint.get("getNoteContext", "/api/v1/notes/:id/context", {params:{id:UUID},success:NoteContextDTO,error:errors}))
    .add(HttpApiEndpoint.put("updateNoteContext", "/api/v1/notes/:id/context", {params:{id:UUID},payload:UpdateNoteContextInput,success:NoteContextDTO,error:errors}))
    .add(HttpApiEndpoint.get("listOrganizations", "/api/v1/organizations", {success:OrganizationsDTO,error:errors}))
    .add(HttpApiEndpoint.post("createOrganization", "/api/v1/organizations", {payload:CreateOrganizationInput,success:OrganizationDTO,error:errors}))
    .add(HttpApiEndpoint.post("setActiveOrganization", "/api/v1/organizations/active", {payload:SetActiveOrganizationInput,success:OrganizationsDTO,error:errors}))
    .add(HttpApiEndpoint.get("getOrganization", "/api/v1/organizations/:id", {params:{id:UUID},success:OrganizationDetailsDTO,error:errors}))
    .add(HttpApiEndpoint.patch("updateOrganization", "/api/v1/organizations/:id", {params:{id:UUID},payload:UpdateOrganizationInput,success:OrganizationDTO,error:errors}))
    .add(HttpApiEndpoint.post("inviteOrganizationMember", "/api/v1/organizations/:id/invitations", {params:{id:UUID},payload:InviteOrganizationMemberInput,success:OrganizationInvitationDTO,error:errors}))
    .add(HttpApiEndpoint.delete("cancelOrganizationInvitation", "/api/v1/organizations/:id/invitations/:invitationId", {params:{id:UUID,invitationId:UUID},success:DeletedDTO,error:errors}))
    .add(HttpApiEndpoint.patch("updateOrganizationMemberRole", "/api/v1/organizations/:id/members/:memberId", {params:{id:UUID,memberId:UUID},payload:UpdateOrganizationMemberRoleInput,success:OrganizationMemberDTO,error:errors}))
    .add(HttpApiEndpoint.delete("removeOrganizationMember", "/api/v1/organizations/:id/members/:memberId", {params:{id:UUID,memberId:UUID},success:DeletedDTO,error:errors}))
    .add(HttpApiEndpoint.get("listOrganizationInvitations", "/api/v1/organization-invitations", {success:OrganizationInvitationsDTO,error:errors}))
    .add(HttpApiEndpoint.post("acceptOrganizationInvitation", "/api/v1/organization-invitations/:id/accept", {params:{id:UUID},success:OrganizationDTO,error:errors}))
    .add(HttpApiEndpoint.post("rejectOrganizationInvitation", "/api/v1/organization-invitations/:id/reject", {params:{id:UUID},success:DeletedDTO,error:errors}))
    .add(HttpApiEndpoint.get("getSession", "/api/v1/session", {success:AuthSessionDTO,error:errors}))
    // A shared link is read by anyone signed in to this server who holds it; the token is still required. Not signed in: 401, before the token is looked at.
    .add(HttpApiEndpoint.get("getSharedNote", "/api/v1/shared/:token", {params:{token:Schema.String},success:SharedNoteDTO,error:errors}))
    .add(HttpApiEndpoint.post("createAppHandoff", "/api/v1/auth/app-handoff", {payload:AppHandoffInput,success:AppHandoffDTO,error:errors}))
    .add(HttpApiEndpoint.get("getAgentBrief", "/api/v1/profile/agent-brief", {success:AgentBriefDTO,error:errors}))
    .add(HttpApiEndpoint.patch("updateAgentBrief", "/api/v1/profile/agent-brief", {payload:UpdateAgentBriefInput,success:AgentBriefDTO,error:errors}))
    .add(HttpApiEndpoint.post("uploadNoteAttachment", "/api/v1/notes/:id/attachments", {params:{id:UUID},payload:UploadNoteAttachmentInput,success:NoteAttachmentDTO,error:errors}))
    .add(HttpApiEndpoint.get("listUsers", "/api/v1/admin/users", {success:UsersDTO,error:errors}))
    .add(HttpApiEndpoint.post("createUser", "/api/v1/admin/users", {payload:CreateUserInput,success:UserDTO,error:errors}))
    .add(HttpApiEndpoint.post("createAdministrator", "/api/v1/admin/administrators", {payload:CreateUserInput,success:UserDTO,error:errors}))
    .add(HttpApiEndpoint.patch("updateUserRole", "/api/v1/admin/users/:id/role", {params:{id:UUID},payload:UpdateUserRoleInput,success:UserDTO,error:errors}))
    .add(HttpApiEndpoint.post("setupOwner", "/api/v1/admin/owner/setup", {payload:CreateUserInput,success:UserDTO,error:errors}))
    .add(HttpApiEndpoint.get("getCalendarPreferences", "/api/v1/calendar/preferences", {success:CalendarPreferencesDTO,error:errors}))
    .add(HttpApiEndpoint.get("getAdminSettings", "/api/v1/admin/settings", { success: AdminSettingsDTO, error: errors }))
    .add(HttpApiEndpoint.patch("updateAdminSettings", "/api/v1/admin/settings", { payload: UpdateAdminSettingsInput, success: AdminSettingsDTO, error: errors }))
    .add(HttpApiEndpoint.post("getModelCatalog", "/api/v1/admin/models", {payload: ModelCatalogInput, success: ModelCatalogDTO, error: errors}))
    .add(HttpApiEndpoint.post("testStorage", "/api/v1/admin/storage/test", {payload: StorageTestInput, success: StorageTestDTO, error: errors}))
    .add(HttpApiEndpoint.get("getChatModels", "/api/v1/models/chat", {success: ChatModelsDTO, error: errors}))
    .add(HttpApiEndpoint.get("listCalendars", "/api/v1/calendars", { success: CalendarsDTO, error: errors }))
    .add(HttpApiEndpoint.post("createCalendar", "/api/v1/calendars", { payload: CreateCalendarInput, success: CalendarDTO, error: errors }))
    .add(HttpApiEndpoint.patch("updateCalendar", "/api/v1/calendars/:id", { params: { id: UUID }, payload: UpdateCalendarInput, success: CalendarDTO, error: errors }))
    .add(HttpApiEndpoint.delete("deleteCalendar", "/api/v1/calendars/:id", { params: { id: UUID }, success: DeletedDTO, error: errors }))
    .add(HttpApiEndpoint.post("syncCalendar", "/api/v1/calendars/:id/sync", { params: { id: UUID }, success: CalendarDTO, error: errors }))
    .add(HttpApiEndpoint.get("listCalendarEvents", "/api/v1/calendar/events", { query: { from: Schema.optionalKey(Timestamp), to: Schema.optionalKey(Timestamp) }, success: CalendarEventsDTO, error: errors }))
    .add(HttpApiEndpoint.post("createCalendarEventNote", "/api/v1/calendar/events/:id/note", { params: { id: UUID }, success: NoteDTO, error: errors }))
    .add(HttpApiEndpoint.post("chatWorkspaceStream", "/api/v1/chat/stream", { payload: ChatInput, success: HttpApiSchema.StreamSse({ data: ChatStreamEventDTO, keepAlive: "10 seconds" }), error: errors }))
    .add(HttpApiEndpoint.get("listNotes", "/api/v1/notes", { success: NotesDTO, error: errors }))
    .add(HttpApiEndpoint.get("listNoteSummaries", "/api/v1/notes/summaries", { query: { limit: Schema.optionalKey(PageLimit), cursor: Schema.optionalKey(PageCursor), filter: Schema.optionalKey(NoteListFilter) }, success: NoteSummariesPageDTO, error: errors }))
    .add(HttpApiEndpoint.get("searchNotes", "/api/v1/notes/search", { query: { q: Schema.String.check(Schema.isMaxLength(200)), limit: Schema.optionalKey(PageLimit), cursor: Schema.optionalKey(PageCursor), filter: Schema.optionalKey(NoteListFilter) }, success: NoteSearchPageDTO, error: errors }))
    .add(HttpApiEndpoint.post("createNote", "/api/v1/notes", { payload: CreateNoteInput, success: NoteDTO, error: errors }))
    .add(HttpApiEndpoint.get("getNote", "/api/v1/notes/:id", { params: { id: UUID }, success: NoteDTO, error: errors }))
    .add(HttpApiEndpoint.patch("updateNote", "/api/v1/notes/:id", { params: { id: UUID }, payload: UpdateNoteInput, success: NoteDTO, error: errors }))
    .add(HttpApiEndpoint.delete("deleteNote", "/api/v1/notes/:id", { params: { id: UUID }, success: DeletedDTO, error: errors }))
    .add(HttpApiEndpoint.get("listShares", "/api/v1/notes/:id/shares", { params: { id: UUID }, success: SharesDTO, error: errors }))
    .add(HttpApiEndpoint.post("createShare", "/api/v1/notes/:id/shares", { params: { id: UUID }, payload: CreateShareInput, success: CreatedShareDTO, error: errors }))
    .add(HttpApiEndpoint.delete("revokeShare", "/api/v1/shares/:id", { params: { id: UUID }, success: ShareDTO, error: errors }))
    .add(HttpApiEndpoint.post("recording", "/api/v1/notes/:id/recording", { params: { id: UUID }, payload: RecordingCommandDTO, success: NoteDTO, error: errors }))
    .add(HttpApiEndpoint.put("uploadAudioChunk", "/api/v1/notes/:id/audio/chunks/:chunkId", { params: { id: UUID, chunkId: UUID }, payload: UploadAudioChunkInput, success: AudioChunkDTO, error: errors }))
    .add(HttpApiEndpoint.get("listAudioChunks", "/api/v1/notes/:id/audio/chunks", { params: { id: UUID }, success: AudioChunksDTO, error: errors }))
    .add(HttpApiEndpoint.post("retryAudioChunk", "/api/v1/notes/:id/audio/chunks/:chunkId/retry", { params: { id: UUID, chunkId: UUID }, success: AudioChunkDTO, error: errors }))
    .add(HttpApiEndpoint.post("generateNotes", "/api/v1/notes/:id/generate", { params: { id: UUID }, payload: GenerateNotesCommandDTO, success: NoteDTO, error: errors }))
    .add(HttpApiEndpoint.post("chat", "/api/v1/notes/:id/chat", { params: { id: UUID }, payload: ChatInput, success: ChatAnswerDTO, error: errors }))
    .add(HttpApiEndpoint.post("chatStream", "/api/v1/notes/:id/chat/stream", { params: { id: UUID }, payload: ChatInput, success: HttpApiSchema.StreamSse({ data: ChatStreamEventDTO, keepAlive: "10 seconds" }), error: errors }))
    .add(HttpApiEndpoint.post("evidence", "/api/v1/notes/:id/evidence", { params: { id: UUID }, payload: EvidenceRequestDTO, success: EvidenceDTO, error: errors }))
    .middleware(OwnerAuth))
  .annotate(OpenApi.Title, "Cereal API")
  .annotate(OpenApi.Version, "1.0.0")
