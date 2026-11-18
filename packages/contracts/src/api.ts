import { Context, Schema } from "effect"
import {UploadNoteAttachmentInput,NoteAttachmentDTO,AgentBriefDTO,UpdateAgentBriefInput} from "./index"
import {ModelCatalogInput,ModelCatalogDTO,ChatModelsDTO} from "./index"
import {NoteListFilter,NoteTrashScope,NoteSummariesPageDTO,NoteSearchPageDTO} from "./index"
import {AuthMethodsDTO,AppHandoffInput,AppHandoffDTO,AppHandoffExchangeInput,AppHandoffTokenDTO} from "./index"
import {DownloadsDTO} from "./index"
import {ChatThreadsPageDTO,ChatThreadDTO,CreateChatThreadInput,AppendChatTurnInput,UpdateChatThreadInput,ChatThreadStreamInput} from "./chats"
import {ImportNoteInput,ImportedNoteDTO} from "./index"
import {TeamSpaceDTO,TeamSpaceNoteContentDTO,AddTeamSpaceNotesInput} from "./team-space"
import {NoteAccessDTO,AddNoteAccessInput,UpdateNoteAccessInput,NoteAccessCandidatesDTO,SharedWithMeDTO,SharedWithMeNoteDTO} from "./note-access"
import {StorageTestInput,StorageTestDTO} from "./index"
import {OnboardingStepInput,GoogleVerificationInput,GoogleVerificationStartDTO,GoogleVerificationStatusDTO,SwitchToPasswordInput} from "./index"
import { HttpApi, HttpApiEndpoint, HttpApiGroup, HttpApiMiddleware, HttpApiSchema, HttpApiSecurity, OpenApi } from "effect/http-api"
import { OrganizationDTO, OrganizationsDTO, OrganizationMemberDTO, OrganizationInvitationDTO, OrganizationDetailsDTO, OrganizationInvitationsDTO, CreateOrganizationInput, UpdateOrganizationInput, SetActiveOrganizationInput, InviteOrganizationMemberInput, UpdateOrganizationMemberRoleInput, ManualParticipantInput, GoogleStatusDTO, GoogleConnectInput, GoogleConnectDTO, GoogleCalendarsDTO, GoogleCalendarDTO, UpdateGoogleCalendarInput, NoteContextDTO, UpdateNoteContextInput, AuthSessionDTO, UsersDTO, UserDTO, CreateUserInput, UpdateUserRoleInput, CalendarPreferencesDTO, CalendarEventsDTO, CalendarsDTO, CalendarDTO, CreateCalendarInput, UpdateCalendarInput, AdminSettingsDTO, UpdateAdminSettingsInput, Timestamp, ChatStreamEventDTO, EvidenceRequestDTO, EvidenceDTO, EvidenceStreamEventDTO, RecordingCommandDTO, GenerateNotesCommandDTO, GenerationStreamEventDTO, ChatInput, ChatAnswerDTO, CreateNoteInput, CreateShareInput, CreatedShareDTO, DeletedDTO, HealthDTO, NoteDTO, NotesDTO, SharedNoteDTO, ShareDTO, SharesDTO, UUID, UpdateNoteInput } from "./index"

export const Unauthorized = Schema.Struct({ error: Schema.Literal("unauthorized"), message: Schema.String }).pipe(HttpApiSchema.status(401))
export const Forbidden = Schema.Struct({ error: Schema.Literal("forbidden"), message: Schema.String }).pipe(HttpApiSchema.status(403))
export const Conflict = Schema.Struct({ error: Schema.Literal("conflict"), message: Schema.String }).pipe(HttpApiSchema.status(409))
export const NotFound = Schema.Struct({ error: Schema.Literal("not_found"), message: Schema.String }).pipe(HttpApiSchema.status(404))
export const InvalidRequest = Schema.Struct({ error: Schema.Literal("invalid_request"), message: Schema.String }).pipe(HttpApiSchema.status(400))
export const Unavailable = Schema.Struct({ error: Schema.Literal("unavailable"), message: Schema.String }).pipe(HttpApiSchema.status(503))
/** The app sign-in handoff: an unknown, used, expired or mismatched code, and too many attempts. */
export const InvalidCode = Schema.Struct({ error: Schema.Literal("invalid_code"), message: Schema.String }).pipe(HttpApiSchema.status(401))
/** A request body over the limit of its endpoint (24 MB for a note import). */
export const PayloadTooLarge = Schema.Struct({ error: Schema.Literal("payload_too_large"), message: Schema.String }).pipe(HttpApiSchema.status(413))
export const RateLimited = Schema.Struct({ error: Schema.Literal("rate_limited"), message: Schema.String }).pipe(HttpApiSchema.status(429))
export type ApiError = typeof Unauthorized.Type | typeof Forbidden.Type | typeof Conflict.Type | typeof NotFound.Type | typeof InvalidRequest.Type | typeof Unavailable.Type
const errors = [Unauthorized, Forbidden, Conflict, NotFound, InvalidRequest, Unavailable] as const

/** Query strings arrive as text: a page size of 1 to 999 (the server caps it) and an opaque cursor. */
const PageLimit = Schema.String.check(Schema.isPattern(/^[1-9][0-9]{0,2}$/))
/** `updatedAt|transcriptRevision|generatedRevision` of the note as the caller last read it; the last part is empty for a note without generated notes. */
const NoteStamp = Schema.String.check(Schema.isMinLength(1), Schema.isMaxLength(128))
const PageCursor = Schema.String.check(Schema.isMinLength(1), Schema.isMaxLength(512), Schema.isPattern(/^[A-Za-z0-9_-]+$/))

export interface PrincipalData {id:string;name:string;email:string;image?:string;role:"owner"|"admin"|"user";source:"bootstrap"|"session";expiresAt:string|null;createdAt:string;sessionId?:string}
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
    // How this browser's proving Google sign-in went (claiming with Google, or switching to Google). Answered from
    // the HttpOnly cookie that sign-in set; a browser that did not start one is told "none".
    .add(HttpApiEndpoint.get("getGoogleVerification", "/api/v1/auth/google/verification", { success: GoogleVerificationStatusDTO, error: [Unavailable] }))
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
    // A shared link is read by a signed-in member of the note's workspace who holds it, and only while the note's
    // sharing lets them (src/note-access.ts): its owner, people it is shared with, and, at level "organization",
    // every member. Anyone else is told 404, as for a link that does not exist. Not signed in: 401, before the token is looked at.
    .add(HttpApiEndpoint.get("getSharedNote", "/api/v1/shared/:token", {params:{token:Schema.String},success:SharedNoteDTO,error:errors}))
    .add(HttpApiEndpoint.post("createAppHandoff", "/api/v1/auth/app-handoff", {payload:AppHandoffInput,success:AppHandoffDTO,error:errors}))
    .add(HttpApiEndpoint.get("getAgentBrief", "/api/v1/profile/agent-brief", {success:AgentBriefDTO,error:errors}))
    .add(HttpApiEndpoint.patch("updateAgentBrief", "/api/v1/profile/agent-brief", {payload:UpdateAgentBriefInput,success:AgentBriefDTO,error:errors}))
    .add(HttpApiEndpoint.post("uploadNoteAttachment", "/api/v1/notes/:id/attachments", {params:{id:UUID},payload:UploadNoteAttachmentInput,success:NoteAttachmentDTO,error:errors}))
    .add(HttpApiEndpoint.get("listUsers", "/api/v1/admin/users", {success:UsersDTO,error:errors}))
    .add(HttpApiEndpoint.post("createUser", "/api/v1/admin/users", {payload:CreateUserInput,success:UserDTO,error:errors}))
    .add(HttpApiEndpoint.post("createAdministrator", "/api/v1/admin/administrators", {payload:CreateUserInput,success:UserDTO,error:errors}))
    .add(HttpApiEndpoint.patch("updateUserRole", "/api/v1/admin/users/:id/role", {params:{id:UUID},payload:UpdateUserRoleInput,success:UserDTO,error:errors}))
    // Claims a fresh server: only the bootstrap token (ADMIN_API_TOKEN) is accepted. Attempts are limited per client; 429 carries Retry-After.
    .add(HttpApiEndpoint.post("setupOwner", "/api/v1/admin/owner/setup", {payload:CreateUserInput,success:UserDTO,error:[...errors,RateLimited]}))
    .add(HttpApiEndpoint.get("getCalendarPreferences", "/api/v1/calendar/preferences", {success:CalendarPreferencesDTO,error:errors}))
    .add(HttpApiEndpoint.get("getAdminSettings", "/api/v1/admin/settings", { success: AdminSettingsDTO, error: errors }))
    .add(HttpApiEndpoint.patch("updateAdminSettings", "/api/v1/admin/settings", { payload: UpdateAdminSettingsInput, success: AdminSettingsDTO, error: errors }))
    // Claiming a fresh server with Google: the setup code as bearer, like owner/setup, and the same attempt limit.
    // Nothing is saved yet. The browser is sent to Google; when Google hands back a verified identity the server is
    // claimed for it, Google becomes the one way to sign in, and the account never has a password.
    .add(HttpApiEndpoint.post("setupOwnerWithGoogle", "/api/v1/admin/owner/setup/google", {payload:GoogleVerificationInput,success:GoogleVerificationStartDTO,error:[...errors,RateLimited]}))
    // Changing the one way to sign in, from a browser session of an administrator. To Google: the same proving
    // sign-in, and the Google account is tied to that administrator. To email and password: the administrator sets
    // the password they will use. Either way the new method works for them before the old one stops.
    .add(HttpApiEndpoint.post("switchToGoogle", "/api/v1/admin/sign-in/google", {payload:GoogleVerificationInput,success:GoogleVerificationStartDTO,error:errors}))
    .add(HttpApiEndpoint.post("switchToPassword", "/api/v1/admin/sign-in/password", {payload:SwitchToPasswordInput,success:AdminSettingsDTO,error:errors}))
    // First-run setup. Each answers with the settings as they now stand, so `onboarding` is always current.
    // Recording a step and finishing are for the owner and administrators; finishing is refused (409) until the
    // required steps (owner, language, speech) are done. Starting over is the owner's alone.
    .add(HttpApiEndpoint.post("recordOnboardingStep", "/api/v1/admin/onboarding", { payload: OnboardingStepInput, success: AdminSettingsDTO, error: errors }))
    .add(HttpApiEndpoint.post("completeOnboarding", "/api/v1/admin/onboarding/complete", { success: AdminSettingsDTO, error: errors }))
    .add(HttpApiEndpoint.post("restartOnboarding", "/api/v1/admin/onboarding/restart", { success: AdminSettingsDTO, error: errors }))
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
    // Saved conversations (src/chats.ts): the signed-in person's own, never anyone else's, whatever their role.
    // Another person's thread answers 404, the same as one that does not exist.
    .add(HttpApiEndpoint.get("listChatThreads", "/api/v1/chats", { query: { limit: Schema.optionalKey(PageLimit), cursor: Schema.optionalKey(PageCursor), noteId: Schema.optionalKey(UUID), scope: Schema.optionalKey(Schema.Literals(["all", "workspace"])) }, success: ChatThreadsPageDTO, error: errors }))
    .add(HttpApiEndpoint.post("createChatThread", "/api/v1/chats", { payload: CreateChatThreadInput, success: ChatThreadDTO, error: errors }))
    .add(HttpApiEndpoint.get("getChatThread", "/api/v1/chats/:id", { params: { id: UUID }, success: ChatThreadDTO, error: errors }))
    .add(HttpApiEndpoint.patch("updateChatThread", "/api/v1/chats/:id", { params: { id: UUID }, payload: UpdateChatThreadInput, success: ChatThreadDTO, error: errors }))
    .add(HttpApiEndpoint.delete("deleteChatThread", "/api/v1/chats/:id", { params: { id: UUID }, success: DeletedDTO, error: errors }))
    .add(HttpApiEndpoint.post("appendChatTurn", "/api/v1/chats/:id/turns", { params: { id: UUID }, payload: AppendChatTurnInput, success: ChatThreadDTO, error: errors }))
    .add(HttpApiEndpoint.post("streamChatThread", "/api/v1/chats/:id/stream", { params: { id: UUID }, payload: ChatThreadStreamInput, success: HttpApiSchema.StreamSse({ data: ChatStreamEventDTO, keepAlive: "10 seconds" }), error: errors }))
    .add(HttpApiEndpoint.get("listNotes", "/api/v1/notes", { success: NotesDTO, error: errors }))
    .add(HttpApiEndpoint.get("listNoteSummaries", "/api/v1/notes/summaries", { query: { limit: Schema.optionalKey(PageLimit), cursor: Schema.optionalKey(PageCursor), filter: Schema.optionalKey(NoteListFilter), trash: Schema.optionalKey(NoteTrashScope) }, success: NoteSummariesPageDTO, error: errors }))
    .add(HttpApiEndpoint.get("searchNotes", "/api/v1/notes/search", { query: { q: Schema.String.check(Schema.isMaxLength(200)), limit: Schema.optionalKey(PageLimit), cursor: Schema.optionalKey(PageCursor), filter: Schema.optionalKey(NoteListFilter), trash: Schema.optionalKey(NoteTrashScope) }, success: NoteSearchPageDTO, error: errors }))
    .add(HttpApiEndpoint.post("createNote", "/api/v1/notes", { payload: CreateNoteInput, success: NoteDTO, error: errors }))
    .add(HttpApiEndpoint.get("getNote", "/api/v1/notes/:id", { params: { id: UUID }, success: NoteDTO, error: errors }))
    .add(HttpApiEndpoint.patch("updateNote", "/api/v1/notes/:id", { params: { id: UUID }, payload: UpdateNoteInput, success: NoteDTO, error: errors }))
    // A note moved here from another account, whole. Idempotent by the id the client chose: the same body again
    // answers 200 with the same note; any other use of an id that exists (somebody else's note, another body, a
    // note changed since) answers one and the same 409. At most 50,000 segments and 24 MB (413 beyond).
    .add(HttpApiEndpoint.post("importNote", "/api/v1/notes/import", { payload: ImportNoteInput, success: ImportedNoteDTO, error: [...errors, PayloadTooLarge] }))
    // With `ifStamp` the note is deleted only if it is still what the caller read: 409 when the stamp differs or
    // the note is recording.
    .add(HttpApiEndpoint.delete("deleteNote", "/api/v1/notes/:id", { params: { id: UUID }, query: { ifStamp: Schema.optionalKey(NoteStamp) }, success: DeletedDTO, error: errors }))
    .add(HttpApiEndpoint.get("listShares", "/api/v1/notes/:id/shares", { params: { id: UUID }, success: SharesDTO, error: errors }))
    .add(HttpApiEndpoint.post("createShare", "/api/v1/notes/:id/shares", { params: { id: UUID }, payload: CreateShareInput, success: CreatedShareDTO, error: errors }))
    .add(HttpApiEndpoint.delete("revokeShare", "/api/v1/shares/:id", { params: { id: UUID }, success: ShareDTO, error: errors }))
    // Who a note is shared with (src/note-access.ts). The owner's alone to read and change: anyone else is told
    // 404. People and candidates are members of the note's workspace, never anyone outside it (400 when asked).
    .add(HttpApiEndpoint.get("getNoteAccess", "/api/v1/notes/:id/access", { params: { id: UUID }, success: NoteAccessDTO, error: errors }))
    .add(HttpApiEndpoint.patch("updateNoteAccess", "/api/v1/notes/:id/access", { params: { id: UUID }, payload: UpdateNoteAccessInput, success: NoteAccessDTO, error: errors }))
    .add(HttpApiEndpoint.post("addNoteAccess", "/api/v1/notes/:id/access/people", { params: { id: UUID }, payload: AddNoteAccessInput, success: NoteAccessDTO, error: errors }))
    .add(HttpApiEndpoint.delete("removeNoteAccess", "/api/v1/notes/:id/access/people/:userId", { params: { id: UUID, userId: UUID }, success: NoteAccessDTO, error: errors }))
    .add(HttpApiEndpoint.get("searchNoteAccessCandidates", "/api/v1/notes/:id/access/candidates", { params: { id: UUID }, query: { q: Schema.optionalKey(Schema.String.check(Schema.isMaxLength(200))) }, success: NoteAccessCandidatesDTO, error: errors }))
    // Notes other members shared with the reader by name, and one of them as it is now. Anything else is 404.
    .add(HttpApiEndpoint.get("listSharedWithMe", "/api/v1/shared-with-me", { success: SharedWithMeDTO, error: errors }))
    .add(HttpApiEndpoint.get("getSharedWithMeNote", "/api/v1/shared-with-me/:id", { params: { id: UUID }, success: SharedWithMeNoteDTO, error: errors }))
    // The team space (src/team-space.ts): notes their owners show to the whole workspace. Members only: anyone
    // else is told 404, for the space and for each note. Nothing here changes a note.
    .add(HttpApiEndpoint.get("getTeamSpace", "/api/v1/team-space", { success: TeamSpaceDTO, error: errors }))
    .add(HttpApiEndpoint.post("addTeamSpaceNotes", "/api/v1/team-space/notes", { payload: AddTeamSpaceNotesInput, success: TeamSpaceDTO, error: errors }))
    .add(HttpApiEndpoint.get("getTeamSpaceNote", "/api/v1/team-space/notes/:id", { params: { id: UUID }, success: TeamSpaceNoteContentDTO, error: errors }))
    .add(HttpApiEndpoint.delete("removeTeamSpaceNote", "/api/v1/team-space/notes/:id", { params: { id: UUID }, success: DeletedDTO, error: errors }))
    .add(HttpApiEndpoint.post("recording", "/api/v1/notes/:id/recording", { params: { id: UUID }, payload: RecordingCommandDTO, success: NoteDTO, error: errors }))
    .add(HttpApiEndpoint.post("generateNotes", "/api/v1/notes/:id/generate", { params: { id: UUID }, payload: GenerateNotesCommandDTO, success: NoteDTO, error: errors }))
    .add(HttpApiEndpoint.post("generateNotesStream", "/api/v1/notes/:id/generate/stream", { params: { id: UUID }, payload: GenerateNotesCommandDTO, success: HttpApiSchema.StreamSse({ data: GenerationStreamEventDTO, keepAlive: "10 seconds" }), error: errors }))
    .add(HttpApiEndpoint.post("chat", "/api/v1/notes/:id/chat", { params: { id: UUID }, payload: ChatInput, success: ChatAnswerDTO, error: errors }))
    .add(HttpApiEndpoint.post("chatStream", "/api/v1/notes/:id/chat/stream", { params: { id: UUID }, payload: ChatInput, success: HttpApiSchema.StreamSse({ data: ChatStreamEventDTO, keepAlive: "10 seconds" }), error: errors }))
    .add(HttpApiEndpoint.post("evidence", "/api/v1/notes/:id/evidence", { params: { id: UUID }, payload: EvidenceRequestDTO, success: EvidenceDTO, error: errors }))
    .add(HttpApiEndpoint.post("evidenceStream", "/api/v1/notes/:id/evidence/stream", { params: { id: UUID }, payload: EvidenceRequestDTO, success: HttpApiSchema.StreamSse({ data: EvidenceStreamEventDTO, keepAlive: "10 seconds" }), error: errors }))
    .middleware(OwnerAuth))
  .annotate(OpenApi.Title, "Cereal API")
  .annotate(OpenApi.Version, "1.0.0")
