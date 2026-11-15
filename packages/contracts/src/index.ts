import { Schema } from "effect"

export const UUID = Schema.String.check(Schema.isUUID()).annotate({ identifier: "UUID", description: "UUID encoded as a string" })
export const NoteTitle = Schema.String.check(Schema.isMaxLength(500))
export const NoteText = Schema.String.check(Schema.isMaxLength(1_000_000))
export const Timestamp = Schema.String.check(
  Schema.isPattern(/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{1,9})?(?:Z|[+-]\d{2}:\d{2})$/),
  Schema.makeFilter((value) => Number.isFinite(Date.parse(value)) || "Expected an ISO-8601 timestamp"),
).annotate({ identifier: "Timestamp", description: "ISO-8601 timestamp, represented as a string in every client" })
export const NoteDate = Schema.String.check(
  Schema.isPattern(/^\d{4}-\d{2}-\d{2}$/),
  Schema.makeFilter((value) => {
    const date = new Date(`${value}T00:00:00.000Z`)
    return Number.isFinite(date.getTime()) && date.toISOString().slice(0, 10) === value || "Expected a valid calendar date"
  }),
)
const Milliseconds = Schema.Number.check(Schema.isInt32(), Schema.isGreaterThanOrEqualTo(0))

/** Transcript source is explicit. Channel separation is not an assertion of biometric speaker identification. */
export const AudioChannel = Schema.Literals(["mic", "system"]).annotate({ identifier: "AudioChannel" })
export const TranscriptSegmentDTO = Schema.Struct({
  id: UUID,
  channel: AudioChannel,
  speakerId: Schema.String.check(Schema.isMinLength(1), Schema.isMaxLength(100)),
  speakerLabel: Schema.NullOr(Schema.String.check(Schema.isMaxLength(200))),
  text: Schema.String.check(Schema.isMaxLength(20_000)),
  isFinal: Schema.Boolean,
  startMs: Milliseconds,
  endMs: Schema.NullOr(Milliseconds),
}).check(Schema.makeFilter((segment) => segment.endMs === null || segment.endMs >= segment.startMs || "endMs must be at least startMs"))
  .annotate({ identifier: "TranscriptSegmentDTO" })
export type TranscriptSegmentDTO = typeof TranscriptSegmentDTO.Type

export const RecordingState = Schema.Literals(["idle", "recording", "stopped"]).annotate({ identifier: "RecordingState" })
export const GenerationState = Schema.Literals(["idle", "analyzing_transcript", "writing_notes", "ready", "failed", "unsupported"]).annotate({ identifier: "GenerationState" })
const Revision = Schema.Number.check(Schema.isInt32(), Schema.isGreaterThanOrEqualTo(0))
export const RecordingCommandDTO = Schema.Struct({ command: Schema.Literals(["start", "stop", "resume"]) }).annotate({ identifier: "RecordingCommandDTO" })
export const GenerateNotesCommandDTO = Schema.Struct({ expectedTranscriptRevision: Revision }).annotate({ identifier: "GenerateNotesCommandDTO" })
export const GenerationProgressDTO = Schema.Struct({ noteId: UUID, transcriptRevision: Revision, state: GenerationState }).annotate({ identifier: "GenerationProgressDTO" })

export const NoteDTO = Schema.Struct({
  id: UUID,
  title: NoteTitle,
  body: NoteText,
  enhanced: NoteText,
  date: NoteDate,
  duration: Schema.String.check(Schema.isMaxLength(32)),
  createdAt: Timestamp,
  updatedAt: Timestamp,
  transcriptSegments: Schema.Array(TranscriptSegmentDTO),
  recordingState: RecordingState,
  generationState: GenerationState,
  transcriptRevision: Revision,
  generatedRevision: Schema.NullOr(Revision),
}).annotate({ identifier: "NoteDTO" })
export type NoteDTO = typeof NoteDTO.Type

export const CreateNoteInput = Schema.Struct({
  title: NoteTitle,
  body: Schema.optionalKey(NoteText),
  enhanced: Schema.optionalKey(NoteText),
  date: Schema.optionalKey(NoteDate),
  duration: Schema.optionalKey(Schema.String.check(Schema.isMaxLength(32))),
  transcriptSegments: Schema.optionalKey(Schema.Array(TranscriptSegmentDTO).check(Schema.isMaxLength(10_000))),
}).annotate({ identifier: "CreateNoteInput" })
export type CreateNoteInput = typeof CreateNoteInput.Type
export const UpdateNoteInput = Schema.Struct({
  title: Schema.optionalKey(NoteTitle),
  body: Schema.optionalKey(NoteText),
  enhanced: Schema.optionalKey(NoteText),
  date: Schema.optionalKey(NoteDate),
  duration: Schema.optionalKey(Schema.String.check(Schema.isMaxLength(32))),
  transcriptSegments: Schema.optionalKey(Schema.Array(TranscriptSegmentDTO).check(Schema.isMaxLength(10_000))),
}).annotate({ identifier: "UpdateNoteInput" })
export type UpdateNoteInput = typeof UpdateNoteInput.Type

/** Public projection: raw transcript, note ID, storage fields, and owner metadata never appear here. */
export const SharedNoteDTO = Schema.Struct({
  shareId: UUID,
  title: NoteTitle,
  body: NoteText,
  enhanced: NoteText,
  date: NoteDate,
  duration: Schema.String.check(Schema.isMaxLength(32)),
  sharedAt: Timestamp,
  expiresAt: Schema.NullOr(Timestamp),
}).annotate({ identifier: "SharedNoteDTO" })
export type SharedNoteDTO = typeof SharedNoteDTO.Type
export const CreateShareInput = Schema.Struct({
  includeBody: Schema.Boolean,
  includeEnhanced: Schema.Boolean,
  expiresAt: Schema.optionalKey(Schema.NullOr(Timestamp)),
}).annotate({ identifier: "CreateShareInput", description: "Explicit fields to publish as an immutable snapshot. Transcript is never publishable." })
export type CreateShareInput = typeof CreateShareInput.Type
export const ShareDTO = Schema.Struct({
  id: UUID,
  noteId: UUID,
  createdAt: Timestamp,
  expiresAt: Schema.NullOr(Timestamp),
  revokedAt: Schema.NullOr(Timestamp),
}).annotate({ identifier: "ShareDTO" })
export type ShareDTO = typeof ShareDTO.Type
export const CreatedShareDTO = Schema.Struct({
  share: ShareDTO,
  token: Schema.String.check(Schema.isPattern(/^[A-Za-z0-9_-]{43}$/)),
  path: Schema.String.check(Schema.isPattern(/^\/s\/[A-Za-z0-9_-]{43}$/)),
}).annotate({ identifier: "CreatedShareDTO", description: "Opaque token is returned once at creation; only its SHA-256 hash is stored." })
export type CreatedShareDTO = typeof CreatedShareDTO.Type
export const ApiErrorCode = Schema.Literals(["unauthorized", "forbidden", "conflict", "not_found", "invalid_request", "unavailable"]).annotate({ identifier: "ApiErrorCode" })
export const ApiErrorDTO = Schema.Struct({ error: ApiErrorCode, message: Schema.String }).annotate({ identifier: "ApiErrorDTO" })
export type ApiErrorDTO = typeof ApiErrorDTO.Type
export const HealthDTO = Schema.Struct({ status: Schema.Literal("ok"), version: Schema.Literal("v1") }).annotate({ identifier: "HealthDTO" })
export const DeletedDTO = Schema.Struct({ deleted: Schema.Boolean }).annotate({ identifier: "DeletedDTO" })
/** One file this server hands out: its name, the path to fetch it from on this server, its size in bytes and its SHA-256 in lower-case hex. */
export const DownloadFileDTO = Schema.Struct({
  file: Schema.String.check(Schema.isPattern(/^[A-Za-z0-9][A-Za-z0-9._+-]{0,199}$/)),
  url: Schema.String.check(Schema.isPattern(/^\/api\/v1\/downloads\/files\/[A-Za-z0-9][A-Za-z0-9._+-]{0,199}$/)),
  size: Schema.Number.check(Schema.isInt(), Schema.isGreaterThanOrEqualTo(0)),
  sha256: Schema.String.check(Schema.isPattern(/^[0-9a-f]{64}$/)),
}).annotate({ identifier: "DownloadFileDTO" })
export type DownloadFileDTO = typeof DownloadFileDTO.Type
/** The Mac app for this server's version: the installer people download, and the archive the app updates itself from (null when only the installer has been published). */
export const MacDownloadDTO = Schema.Struct({
  // "arm64" today. A string, not a closed list, so an app built now can still read the answer when another build exists.
  arch: Schema.String.check(Schema.isPattern(/^[a-z0-9_]{1,20}$/)),
  minimumSystem: Schema.String.check(Schema.isPattern(/^\d{1,3}(\.\d{1,3}){0,2}$/)),
  installer: DownloadFileDTO,
  update: Schema.NullOr(DownloadFileDTO),
}).annotate({ identifier: "MacDownloadDTO" })
export type MacDownloadDTO = typeof MacDownloadDTO.Type
/** The apps for the version this server is running. `mac` is null when no build of that version has been published to this server. Public. */
export const DownloadsDTO = Schema.Struct({
  version: Schema.String.check(Schema.isPattern(/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/), Schema.isMaxLength(40)),
  mac: Schema.NullOr(MacDownloadDTO),
  iosAppStoreUrl: Schema.NullOr(Schema.String),
}).annotate({ identifier: "DownloadsDTO" })
export type DownloadsDTO = typeof DownloadsDTO.Type
export const NotesDTO = Schema.Array(NoteDTO)
export const SharesDTO = Schema.Array(ShareDTO)

/** The batch provider boundary. Streaming transcription has its own boundary and wire protocol in ./live.ts. */
export interface TranscriptionProvider {
  readonly name: string
  transcribe(input: { channel: "mic" | "system"; audio: Uint8Array; sampleRate: number; signal: AbortSignal }): AsyncIterable<TranscriptSegmentDTO>
}

/** Immutable WAV uploads are acknowledged before provider work; retry the same UUID safely. */
export const UploadAudioChunkInput = Schema.Struct({
  channel: AudioChannel,
  sequence: Revision,
  startMs: Milliseconds,
  audioBase64: Schema.String.check(Schema.isMinLength(4), Schema.isMaxLength(27_962_028)),
}).annotate({ identifier: "UploadAudioChunkInput" })
export type UploadAudioChunkInput = typeof UploadAudioChunkInput.Type
export const AudioChunkState = Schema.Literals(["queued", "processing", "ready", "failed"]).annotate({ identifier: "AudioChunkState" })
export const AudioChunkDTO = Schema.Struct({
  id: UUID, noteId: UUID, channel: AudioChannel, sequence: Revision, startMs: Milliseconds, durationMs: Milliseconds,
  state: AudioChunkState,
  error: Schema.NullOr(Schema.String),
}).annotate({ identifier: "AudioChunkDTO" })
export type AudioChunkDTO = typeof AudioChunkDTO.Type
export const AudioChunksDTO = Schema.Array(AudioChunkDTO)
export const ChatMessageDTO = Schema.Struct({
  role: Schema.Literals(["user", "assistant"]), content: Schema.String.check(Schema.isMinLength(1), Schema.isMaxLength(20_000)),
}).annotate({ identifier: "ChatMessageDTO" })
export const ChatInput = Schema.Struct({
  message: Schema.String.check(Schema.isMinLength(1), Schema.isMaxLength(20_000)),
  history: Schema.Array(ChatMessageDTO).check(Schema.isMaxLength(30)),
  includeSources:Schema.optionalKey(Schema.Boolean),
  timeZone:Schema.optionalKey(Schema.String.check(Schema.isMinLength(1),Schema.isMaxLength(100))),
  model:Schema.optionalKey(Schema.String.check(Schema.isMaxLength(200))),
}).annotate({ identifier: "ChatInput" })
export type ChatInput = typeof ChatInput.Type
export const ChatAnswerDTO = Schema.Struct({ answer: NoteText }).annotate({ identifier: "ChatAnswerDTO" })
export type ChatAnswerDTO = typeof ChatAnswerDTO.Type
export const ChatCitationDTO = Schema.Struct({
  id:Schema.String.check(Schema.isPattern(/^S[1-9][0-9]{0,2}$/)),noteId:UUID,title:NoteTitle,
  source:Schema.Literals(["transcript","notes","summary"]),segmentId:Schema.NullOr(UUID),
  startMs:Schema.NullOr(Milliseconds),quote:Schema.String.check(Schema.isMinLength(1),Schema.isMaxLength(20_000)),transcriptRevision:Revision,
}).annotate({identifier:"ChatCitationDTO"})
export type ChatCitationDTO = typeof ChatCitationDTO.Type
export const ChatToolMeetingDTO=Schema.Struct({noteId:UUID,title:NoteTitle,date:NoteDate}).annotate({identifier:"ChatToolMeetingDTO"})
export const ChatToolProgressDTO = Schema.Struct({
  callId:UUID,name:Schema.Literals(["search_meetings","search_transcripts","list_meetings","get_meetings","get_meeting_transcript"]),
  status:Schema.Literals(["running","completed","failed"]),label:Schema.String.check(Schema.isMaxLength(200)),
  resultCount:Schema.optionalKey(Revision),results:Schema.optionalKey(Schema.Array(ChatToolMeetingDTO).check(Schema.isMaxLength(20))),
}).annotate({identifier:"ChatToolProgressDTO"})
export type ChatToolProgressDTO = typeof ChatToolProgressDTO.Type
export const ChatStreamEventDTO = Schema.Struct({
  type: Schema.Literals(["delta", "done", "error","tool","citations"]), text: NoteText,
  tool:Schema.optionalKey(ChatToolProgressDTO),citations:Schema.optionalKey(Schema.Array(ChatCitationDTO).check(Schema.isMaxLength(40))),
}).annotate({ identifier: "ChatStreamEventDTO" })
export type ChatStreamEventDTO = typeof ChatStreamEventDTO.Type
export const EvidenceRequestDTO = Schema.Struct({
  paragraph: Schema.String.check(Schema.isMinLength(1), Schema.isMaxLength(20_000)),
  expectedTranscriptRevision: Revision,
}).annotate({ identifier: "EvidenceRequestDTO" })
export type EvidenceRequestDTO = typeof EvidenceRequestDTO.Type
export const EvidenceQuoteDTO = Schema.Struct({
  segmentId: UUID, text: Schema.String.check(Schema.isMaxLength(20_000)),
  speakerLabel: Schema.NullOr(Schema.String), startMs: Milliseconds,
}).annotate({ identifier: "EvidenceQuoteDTO" })
export const EvidenceDTO = Schema.Struct({
  summary: NoteText, quotes: Schema.Array(EvidenceQuoteDTO).check(Schema.isMaxLength(8)), transcriptRevision: Revision,
}).annotate({ identifier: "EvidenceDTO" })
export type EvidenceDTO = typeof EvidenceDTO.Type

/** Owner-only instance administration. Secret values are accepted only on writes. */
export const UserRole = Schema.Literals(["owner", "admin", "user"])
export const UserDTO = Schema.Struct({id: UUID, name: Schema.String, email: Schema.String, role: UserRole, createdAt: Timestamp}).annotate({identifier:"UserDTO"})
export type UserDTO = typeof UserDTO.Type
export const AuthSessionDTO = Schema.Struct({user:UserDTO, expiresAt:Schema.NullOr(Timestamp)}).annotate({identifier:"AuthSessionDTO"})
export type AuthSessionDTO = typeof AuthSessionDTO.Type
/** Product-owned MCP consent/connection projections. OAuth token and protocol
 * schemas remain owned by Better Auth and the official MCP SDK. */
export const McpWorkspaceDTO=Schema.Struct({id:Schema.String,name:Schema.String}).annotate({identifier:"McpWorkspaceDTO"})
export const McpAuthorizationDTO=Schema.Struct({
  clientId:Schema.String,clientName:Schema.String,redirectUri:Schema.String,scopes:Schema.Array(Schema.String),resource:Schema.String,
  workspace:Schema.NullOr(McpWorkspaceDTO),globalWorkspaceSwitch:Schema.Literal(true),
}).annotate({identifier:"McpAuthorizationDTO"})
export type McpAuthorizationDTO=typeof McpAuthorizationDTO.Type
export const McpConnectionDTO=Schema.Struct({id:Schema.String,clientId:Schema.String,clientName:Schema.String,scopes:Schema.Array(Schema.String),createdAt:Timestamp,updatedAt:Timestamp}).annotate({identifier:"McpConnectionDTO"})
export type McpConnectionDTO=typeof McpConnectionDTO.Type
export const McpConnectionsDTO=Schema.Struct({connections:Schema.Array(McpConnectionDTO)}).annotate({identifier:"McpConnectionsDTO"})
export type McpConnectionsDTO=typeof McpConnectionsDTO.Type
export const NoteAttachmentMime=Schema.Literals(["image/png","image/jpeg","image/webp"])
export const UploadNoteAttachmentInput=Schema.Struct({name:Schema.String.check(Schema.isMinLength(1),Schema.isMaxLength(255)),mimeType:NoteAttachmentMime,dataBase64:Schema.String.check(Schema.isMinLength(4),Schema.isMaxLength(6_990_508),Schema.isPattern(/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/))}).annotate({identifier:"UploadNoteAttachmentInput"})
export const NoteAttachmentDTO=Schema.Struct({id:UUID,noteId:UUID,name:Schema.String,mimeType:NoteAttachmentMime,createdAt:Timestamp}).annotate({identifier:"NoteAttachmentDTO"})
export type NoteAttachmentDTO=typeof NoteAttachmentDTO.Type
export const AgentBriefDTO=Schema.Struct({body:Schema.String,updatedAt:Schema.NullOr(Timestamp),workspaceId:UUID,source:Schema.Literal("explicit_user_profile"),automaticallyInferred:Schema.Literal(false)}).annotate({identifier:"AgentBriefDTO"})
export type AgentBriefDTO=typeof AgentBriefDTO.Type
export const UpdateAgentBriefInput=Schema.Struct({body:Schema.String.check(Schema.isMaxLength(20_000))}).annotate({identifier:"UpdateAgentBriefInput"})
export const UsersDTO = Schema.Struct({users:Schema.Array(UserDTO)}).annotate({identifier:"UsersDTO"})
export const CreateUserInput = Schema.Struct({name:Schema.String.check(Schema.isMinLength(1),Schema.isMaxLength(200)),email:Schema.String.check(Schema.isPattern(/^[^\s@]+@[^\s@]+\.[^\s@]+$/),Schema.isMaxLength(254)),password:Schema.String.check(Schema.isMinLength(12),Schema.isMaxLength(128))}).annotate({identifier:"CreateUserInput"})
export type CreateUserInput = typeof CreateUserInput.Type
export const UpdateUserRoleInput = Schema.Struct({role:Schema.Literals(["admin","user"])}).annotate({identifier:"UpdateUserRoleInput"})
export type UpdateUserRoleInput = typeof UpdateUserRoleInput.Type
/** Workspace membership is independent of instance roles and private note ownership. */
export const OrganizationRole = Schema.Literals(["owner", "admin", "member"])
export const OrganizationDTO = Schema.Struct({id:UUID,name:Schema.String,slug:Schema.String,role:OrganizationRole,createdAt:Timestamp}).annotate({identifier:"OrganizationDTO"})
export type OrganizationDTO = typeof OrganizationDTO.Type
export const OrganizationsDTO = Schema.Struct({organizations:Schema.Array(OrganizationDTO),activeOrganizationId:Schema.NullOr(UUID)}).annotate({identifier:"OrganizationsDTO"})
export type OrganizationsDTO = typeof OrganizationsDTO.Type
export const OrganizationMemberDTO = Schema.Struct({id:UUID,organizationId:UUID,userId:UUID,name:Schema.String,email:Schema.String,role:OrganizationRole,createdAt:Timestamp}).annotate({identifier:"OrganizationMemberDTO"})
export type OrganizationMemberDTO = typeof OrganizationMemberDTO.Type
export const OrganizationInvitationDTO = Schema.Struct({id:UUID,organizationId:UUID,organizationName:Schema.String,email:Schema.String,role:OrganizationRole,status:Schema.Literals(["pending","accepted","rejected","canceled","expired"]),inviterId:UUID,createdAt:Timestamp,expiresAt:Timestamp}).annotate({identifier:"OrganizationInvitationDTO"})
export type OrganizationInvitationDTO = typeof OrganizationInvitationDTO.Type
export const OrganizationDetailsDTO = Schema.Struct({organization:OrganizationDTO,members:Schema.Array(OrganizationMemberDTO),invitations:Schema.Array(OrganizationInvitationDTO)}).annotate({identifier:"OrganizationDetailsDTO"})
export type OrganizationDetailsDTO = typeof OrganizationDetailsDTO.Type
export const OrganizationInvitationsDTO = Schema.Struct({invitations:Schema.Array(OrganizationInvitationDTO)}).annotate({identifier:"OrganizationInvitationsDTO"})
export type OrganizationInvitationsDTO = typeof OrganizationInvitationsDTO.Type
const OrganizationName = Schema.String.check(Schema.isMinLength(1),Schema.isMaxLength(100))
const OrganizationSlug = Schema.String.check(Schema.isMinLength(2),Schema.isMaxLength(100),Schema.isPattern(/^[a-z0-9]+(?:-[a-z0-9]+)*$/))
export const CreateOrganizationInput = Schema.Struct({name:OrganizationName,slug:OrganizationSlug}).annotate({identifier:"CreateOrganizationInput"})
export type CreateOrganizationInput = typeof CreateOrganizationInput.Type
export const UpdateOrganizationInput = Schema.Struct({name:OrganizationName}).annotate({identifier:"UpdateOrganizationInput"})
export type UpdateOrganizationInput = typeof UpdateOrganizationInput.Type
export const SetActiveOrganizationInput = Schema.Struct({organizationId:UUID}).annotate({identifier:"SetActiveOrganizationInput"})
export type SetActiveOrganizationInput = typeof SetActiveOrganizationInput.Type
export const InviteOrganizationMemberInput = Schema.Struct({email:Schema.String.check(Schema.isPattern(/^[^\s@]+@[^\s@]+\.[^\s@]+$/),Schema.isMaxLength(254)),role:Schema.Literals(["admin","member"])}).annotate({identifier:"InviteOrganizationMemberInput"})
export type InviteOrganizationMemberInput = typeof InviteOrganizationMemberInput.Type
export const UpdateOrganizationMemberRoleInput = Schema.Struct({role:Schema.Literals(["admin","member"])}).annotate({identifier:"UpdateOrganizationMemberRoleInput"})
export type UpdateOrganizationMemberRoleInput = typeof UpdateOrganizationMemberRoleInput.Type

export const CalendarPreferencesDTO = Schema.Struct({lookAheadDays:Schema.Number.check(Schema.isInt32())}).annotate({identifier:"CalendarPreferencesDTO"})

export const AIProvider = Schema.Literals(["openrouter", "openai", "anthropic", "elevenlabs"])
export type AIProvider = typeof AIProvider.Type
export const LanguageProvider = Schema.Literals(["openrouter", "openai", "anthropic"])
const ModelIdentifier = Schema.String.check(Schema.isMinLength(1), Schema.isMaxLength(150), Schema.isPattern(/^[A-Za-z0-9_.:/-]+$/))
const ProviderKey = Schema.String.check(Schema.isMinLength(10), Schema.isMaxLength(1000), Schema.isPattern(/^[^\r\n]+$/))
export const LanguageModelSelection = Schema.Struct({ provider: LanguageProvider, model: ModelIdentifier }).annotate({identifier:"LanguageModelSelection"})
export type LanguageModelSelection = typeof LanguageModelSelection.Type
export const TranscriptionModelSelection = Schema.Struct({ provider: Schema.Literals(["elevenlabs", "openai"]), model: ModelIdentifier }).annotate({identifier:"TranscriptionModelSelection"})
export type TranscriptionModelSelection = typeof TranscriptionModelSelection.Type
export const ProviderStatusDTO = Schema.Struct({id: AIProvider, configured: Schema.Boolean, source: Schema.Literals(["environment", "stored", "none"])}).annotate({identifier:"ProviderStatusDTO"})
/** The streaming speech-to-text model of an instance; no selection means live transcription is off. */
export const LiveTranscriptionModelSelection = Schema.Struct({ provider: Schema.Literals(["elevenlabs"]), model: ModelIdentifier }).annotate({identifier:"LiveTranscriptionModelSelection"})
export type LiveTranscriptionModelSelection = typeof LiveTranscriptionModelSelection.Type
export const AIModelDTO = Schema.Struct({id: ModelIdentifier, name: Schema.String, capabilities: Schema.Array(Schema.Literals(["language", "transcription", "live_transcription"]))}).annotate({identifier:"AIModelDTO"})
export type AIModelDTO = typeof AIModelDTO.Type
export const ModelCatalogInput = Schema.Struct({provider: AIProvider, apiKey: Schema.optionalKey(ProviderKey), refresh: Schema.optionalKey(Schema.Boolean)}).annotate({identifier:"ModelCatalogInput"})
export type ModelCatalogInput = typeof ModelCatalogInput.Type
export const ModelCatalogDTO = Schema.Struct({provider: AIProvider, models: Schema.Array(AIModelDTO), fetchedAt: Timestamp, notice: Schema.NullOr(Schema.String)}).annotate({identifier:"ModelCatalogDTO"})
export type ModelCatalogDTO = typeof ModelCatalogDTO.Type
export const ChatModelsDTO = Schema.Struct({models: Schema.Array(LanguageModelSelection)}).annotate({identifier:"ChatModelsDTO"})
// Raster-only data URLs avoid remote tracking and active SVG content.
const InstanceLogo = Schema.String.check(Schema.isMaxLength(350_000), Schema.isPattern(/^data:image\/(?:png|jpeg|webp);base64,[A-Za-z0-9+/]+={0,2}$/))
// How people sign in to this instance: set on the admin panel and kept in the database.
/** A full address ("someone@example.com") or a whole domain ("@example.com"). The server stores them lower-cased. */
const AllowedEmail = Schema.String.check(Schema.isMaxLength(254), Schema.isPattern(/^[^\s@]*@[^\s@.]+(?:\.[^\s@.]+)+$/))
const GoogleClientId = Schema.String.check(Schema.isMinLength(10), Schema.isMaxLength(255), Schema.isPattern(/^[A-Za-z0-9._-]+$/))
const GoogleClientSecret = Schema.String.check(Schema.isMinLength(8), Schema.isMaxLength(512), Schema.isPattern(/^[^\s]+$/))
export const SignInSettingsDTO = Schema.Struct({
  password: Schema.Boolean, google: Schema.Boolean,
  /** Who may create an account by signing in with Google. An existing account signs in whether or not it is listed. */
  allowedEmails: Schema.Array(Schema.String),
  googleClientId: Schema.NullOr(Schema.String),
  /** The secret itself is never returned. */
  googleClientSecretSet: Schema.Boolean,
  /** The redirect URIs to register in Google Cloud: sign-in first, then the calendar connection. */
  googleRedirectUris: Schema.Array(Schema.String),
}).annotate({ identifier: "SignInSettingsDTO" })
export type SignInSettingsDTO = typeof SignInSettingsDTO.Type
export const UpdateSignInSettingsInput = Schema.Struct({
  password: Schema.optionalKey(Schema.Boolean), google: Schema.optionalKey(Schema.Boolean),
  allowedEmails: Schema.optionalKey(Schema.Array(AllowedEmail).check(Schema.isMaxLength(500))),
  googleClientId: Schema.optionalKey(Schema.NullOr(GoogleClientId)),
  /** Null clears the stored secret. */
  googleClientSecret: Schema.optionalKey(Schema.NullOr(GoogleClientSecret)),
}).annotate({ identifier: "UpdateSignInSettingsInput" })
export type UpdateSignInSettingsInput = typeof UpdateSignInSettingsInput.Type
/** What the sign-in page and the apps offer. Public: it says nothing about accounts. */
export const AuthMethodsDTO = Schema.Struct({
  password: Schema.Boolean, google: Schema.Boolean, instanceName: Schema.String,
  /** True while nobody has claimed this server: no owner account with a password exists yet. The web and the apps
   * then show "Claim this server" in place of sign-in. Always sent by this server; optional so answers from older servers stay valid. */
  setupRequired: Schema.optionalKey(Schema.Boolean),
}).annotate({ identifier: "AuthMethodsDTO" })
export type AuthMethodsDTO = typeof AuthMethodsDTO.Type
/** Browser-to-app sign-in. The app keeps a random verifier and sends base64url(SHA-256(verifier)) as the challenge. */
export const AppHandoffInput = Schema.Struct({ challenge: Schema.String.check(Schema.isMaxLength(128)) }).annotate({ identifier: "AppHandoffInput" })
export type AppHandoffInput = typeof AppHandoffInput.Type
/** Single use; expires after 120 seconds. */
export const AppHandoffDTO = Schema.Struct({ code: Schema.String }).annotate({ identifier: "AppHandoffDTO" })
export type AppHandoffDTO = typeof AppHandoffDTO.Type
export const AppHandoffExchangeInput = Schema.Struct({ code: Schema.String.check(Schema.isMaxLength(256)), verifier: Schema.String.check(Schema.isMaxLength(256)) }).annotate({ identifier: "AppHandoffExchangeInput" })
export type AppHandoffExchangeInput = typeof AppHandoffExchangeInput.Type
/** A bearer session token, the same kind the `set-auth-token` header carries after password sign-in. */
export const AppHandoffTokenDTO = Schema.Struct({ token: Schema.String }).annotate({ identifier: "AppHandoffTokenDTO" })
export type AppHandoffTokenDTO = typeof AppHandoffTokenDTO.Type
/** Where the server keeps recorded audio and the app builds it hands out: its own disk, or an S3-compatible bucket. */
export const StorageBackend = Schema.Literals(["local", "s3"]).annotate({ identifier: "StorageBackend" })
export type StorageBackend = typeof StorageBackend.Type
/** The secret access key is never returned: `secretAccessKeySet` says whether one is stored. `source` says where
 * the settings in force come from: saved on the admin panel, the server environment (a fallback when nothing was
 * saved), or the built-in default (local disk). */
export const StorageSettingsDTO = Schema.Struct({
  backend: StorageBackend,
  source: Schema.Literals(["stored", "environment", "default"]),
  endpoint: Schema.NullOr(Schema.String),
  region: Schema.NullOr(Schema.String),
  bucket: Schema.NullOr(Schema.String),
  prefix: Schema.String,
  forcePathStyle: Schema.Boolean,
  accessKeyId: Schema.NullOr(Schema.String),
  secretAccessKeySet: Schema.Boolean,
}).annotate({ identifier: "StorageSettingsDTO" })
export type StorageSettingsDTO = typeof StorageSettingsDTO.Type
const StorageText = (max: number) => Schema.String.check(Schema.isMaxLength(max))
/** Every key is optional: only what is sent changes. `endpoint: null` means Amazon S3 itself; `secretAccessKey: null`
 * removes the stored secret. The server checks the values (address, bucket name, prefix) and answers 400 with the reason. */
export const UpdateStorageSettingsInput = Schema.Struct({
  backend: Schema.optionalKey(StorageBackend),
  endpoint: Schema.optionalKey(Schema.NullOr(StorageText(2048))),
  region: Schema.optionalKey(Schema.NullOr(StorageText(40))),
  bucket: Schema.optionalKey(Schema.NullOr(StorageText(63))),
  prefix: Schema.optionalKey(StorageText(200)),
  forcePathStyle: Schema.optionalKey(Schema.Boolean),
  accessKeyId: Schema.optionalKey(Schema.NullOr(StorageText(128))),
  secretAccessKey: Schema.optionalKey(Schema.NullOr(StorageText(512))),
}).annotate({ identifier: "UpdateStorageSettingsInput" })
export type UpdateStorageSettingsInput = typeof UpdateStorageSettingsInput.Type
/** "Test connection": the stored settings with these values laid over them, so a draft can be tried before it is saved. */
export const StorageTestInput = UpdateStorageSettingsInput.annotate({ identifier: "StorageTestInput" })
export type StorageTestInput = typeof StorageTestInput.Type
/** A small file was written, read back and deleted. `step` is the one that failed, null when all three worked. */
export const StorageTestDTO = Schema.Struct({
  ok: Schema.Boolean,
  backend: StorageBackend,
  step: Schema.NullOr(Schema.Literals(["write", "read", "delete"])),
  message: Schema.String,
}).annotate({ identifier: "StorageTestDTO" })
export type StorageTestDTO = typeof StorageTestDTO.Type
/** The iPhone app: which app IDs (team ID, a dot, the bundle ID) may open this server's note links, and the app's
 * numeric App Store ID once it is published (the download page and the Smart App Banner use it). */
export const AppsSettingsDTO = Schema.Struct({ iosAppIds: Schema.Array(Schema.String), iosAppStoreId: Schema.NullOr(Schema.String) }).annotate({ identifier: "AppsSettingsDTO" })
export type AppsSettingsDTO = typeof AppsSettingsDTO.Type
export const UpdateAppsSettingsInput = Schema.Struct({
  iosAppIds: Schema.optionalKey(Schema.Array(Schema.String.check(Schema.isMaxLength(200))).check(Schema.isMaxLength(10))),
  iosAppStoreId: Schema.optionalKey(Schema.NullOr(Schema.String.check(Schema.isMaxLength(20)))),
}).annotate({ identifier: "UpdateAppsSettingsInput" })
export type UpdateAppsSettingsInput = typeof UpdateAppsSettingsInput.Type
/** First-run setup. The steps, in the order the admin panel walks them. `owner`, `language` and `speech` are
 * required before setup can be finished; the others can be skipped. */
export const OnboardingStep = Schema.Literals(["owner", "name", "language", "speech", "models", "signIn", "storage"]).annotate({ identifier: "OnboardingStep" })
export type OnboardingStep = typeof OnboardingStep.Type
/** Which steps are finished, read from the server's real state: an owner account with a password exists; a team
 * name was saved; a key for a chat and notes provider is stored; a key for the speech-to-text provider is stored;
 * models were chosen; and, for sign-in and storage, the step was confirmed or skipped during setup. */
export const OnboardingStepsDTO = Schema.Struct({
  owner: Schema.Boolean, name: Schema.Boolean, language: Schema.Boolean, speech: Schema.Boolean,
  models: Schema.Boolean, signIn: Schema.Boolean, storage: Schema.Boolean,
}).annotate({ identifier: "OnboardingStepsDTO" })
export type OnboardingStepsDTO = typeof OnboardingStepsDTO.Type
/** `completedAt` is null until setup has been finished; while it is null the admin panel shows only the setup flow. */
export const OnboardingDTO = Schema.Struct({ completedAt: Schema.NullOr(Timestamp), steps: OnboardingStepsDTO }).annotate({ identifier: "OnboardingDTO" })
export type OnboardingDTO = typeof OnboardingDTO.Type
/** Records that a step was confirmed ("done") or passed over ("skipped"). A required step cannot be skipped. */
// Named, so generated code does not get a type called `State` (which collides with SwiftUI's).
export const OnboardingStepState = Schema.Literals(["done", "skipped"]).annotate({ identifier: "OnboardingStepState" })
export const OnboardingStepInput = Schema.Struct({ step: OnboardingStep, state: OnboardingStepState }).annotate({ identifier: "OnboardingStepInput" })
export type OnboardingStepInput = typeof OnboardingStepInput.Type
export const AdminSettingsDTO = Schema.Struct({
  instanceName: Schema.String, languageModel: Schema.String,
  instanceLogo: Schema.NullOr(InstanceLogo),
  providers: Schema.Array(ProviderStatusDTO),
  chatModels: Schema.Array(LanguageModelSelection),
  generationModel: LanguageModelSelection,
  transcriptionModel: TranscriptionModelSelection,
  /** Null when live transcription is off. Always sent by this server; optional so older fixtures stay valid. */
  liveTranscriptionModel: Schema.optionalKey(Schema.NullOr(LiveTranscriptionModelSelection)),
  openRouterConfigured: Schema.Boolean, elevenLabsConfigured: Schema.Boolean,
  openRouterSource: Schema.Literals(["environment", "stored", "none"]),
  elevenLabsSource: Schema.Literals(["environment", "stored", "none"]),
  calendarRefreshMinutes: Schema.Number.check(Schema.isInt32()), calendarLookAheadDays: Schema.Number.check(Schema.isInt32()),
  /** Always sent by this server; optional so responses from older servers stay valid. */
  signIn: Schema.optionalKey(SignInSettingsDTO),
  /** Always sent by this server; optional so older fixtures stay valid. */
  storage: Schema.optionalKey(StorageSettingsDTO),
  /** How many recordings may be transcribed live at once, and how many seconds a stopped recording's provider
   * sessions are kept for a Resume (0: not kept). Always sent by this server. */
  liveMaxSessions: Schema.optionalKey(Schema.Number.check(Schema.isInt32())),
  liveHoldSeconds: Schema.optionalKey(Schema.Number.check(Schema.isInt32())),
  apps: Schema.optionalKey(AppsSettingsDTO),
  /** Variables still set in the server's environment whose settings now live here: they are ignored and can be deleted. */
  unusedEnvironment: Schema.optionalKey(Schema.Array(Schema.String)),
  /** First-run setup: whether it has been finished and which steps are done. Always sent by this server; optional so older fixtures stay valid. */
  onboarding: Schema.optionalKey(OnboardingDTO),
  updatedAt: Schema.NullOr(Timestamp),
}).annotate({ identifier: "AdminSettingsDTO" })
export type AdminSettingsDTO = typeof AdminSettingsDTO.Type
export const UpdateAdminSettingsInput = Schema.Struct({
  instanceName: Schema.optionalKey(Schema.String.check(Schema.isMinLength(1), Schema.isMaxLength(100))),
  instanceLogo: Schema.optionalKey(Schema.NullOr(InstanceLogo)),
  chatModels: Schema.optionalKey(Schema.Array(LanguageModelSelection).check(Schema.isMinLength(1), Schema.isMaxLength(20))),
  generationModel: Schema.optionalKey(LanguageModelSelection),
  transcriptionModel: Schema.optionalKey(TranscriptionModelSelection),
  /** Null turns live transcription off. */
  liveTranscriptionModel: Schema.optionalKey(Schema.NullOr(LiveTranscriptionModelSelection)),
  openAiApiKey: Schema.optionalKey(Schema.NullOr(ProviderKey)),
  anthropicApiKey: Schema.optionalKey(Schema.NullOr(ProviderKey)),
  languageModel: Schema.optionalKey(Schema.String.check(Schema.isMinLength(1), Schema.isMaxLength(150), Schema.isPattern(/^[A-Za-z0-9_.:/-]+$/))),
  openRouterApiKey: Schema.optionalKey(Schema.NullOr(ProviderKey)),
  elevenLabsApiKey: Schema.optionalKey(Schema.NullOr(ProviderKey)),
  calendarRefreshMinutes: Schema.optionalKey(Schema.Number.check(Schema.isInt32(), Schema.isBetween({ minimum: 5, maximum: 1440 }))),
  calendarLookAheadDays: Schema.optionalKey(Schema.Number.check(Schema.isInt32(), Schema.isBetween({ minimum: 1, maximum: 90 }))),
  signIn: Schema.optionalKey(UpdateSignInSettingsInput),
  storage: Schema.optionalKey(UpdateStorageSettingsInput),
  liveMaxSessions: Schema.optionalKey(Schema.Number.check(Schema.isInt32(), Schema.isGreaterThanOrEqualTo(1), Schema.isLessThanOrEqualTo(200))),
  liveHoldSeconds: Schema.optionalKey(Schema.Number.check(Schema.isInt32(), Schema.isGreaterThanOrEqualTo(0), Schema.isLessThanOrEqualTo(3600))),
  apps: Schema.optionalKey(UpdateAppsSettingsInput),
}).annotate({ identifier: "UpdateAdminSettingsInput" })
export type UpdateAdminSettingsInput = typeof UpdateAdminSettingsInput.Type
const CalendarName = Schema.String.check(Schema.isMinLength(1), Schema.isMaxLength(200))
const CalendarColor = Schema.String.check(Schema.isPattern(/^#[A-Fa-f0-9]{6}$/))
const FeedURL = Schema.String.check(Schema.isMinLength(8), Schema.isMaxLength(4096))
export const CalendarDTO = Schema.Struct({
  id: UUID, name: CalendarName, provider: Schema.Literal("ics"), color: CalendarColor, enabled: Schema.Boolean,
  sourceHost: Schema.String, lastSyncedAt: Schema.NullOr(Timestamp), lastSyncError: Schema.NullOr(Schema.String),
  eventCount: Schema.Number.check(Schema.isInt32()), createdAt: Timestamp,
}).annotate({ identifier: "CalendarDTO" })
export type CalendarDTO = typeof CalendarDTO.Type
export const CalendarsDTO = Schema.Array(CalendarDTO)
export const CreateCalendarInput = Schema.Struct({
  name: CalendarName, url: FeedURL, color: Schema.optionalKey(CalendarColor), enabled: Schema.optionalKey(Schema.Boolean),
}).annotate({ identifier: "CreateCalendarInput" })
export type CreateCalendarInput = typeof CreateCalendarInput.Type
export const UpdateCalendarInput = Schema.Struct({
  name: Schema.optionalKey(CalendarName), url: Schema.optionalKey(FeedURL), color: Schema.optionalKey(CalendarColor), enabled: Schema.optionalKey(Schema.Boolean),
}).annotate({ identifier: "UpdateCalendarInput" })
export type UpdateCalendarInput = typeof UpdateCalendarInput.Type
export const CalendarEventDTO = Schema.Struct({
  id: UUID, calendarId: UUID, calendarName: CalendarName, color: CalendarColor, title: NoteTitle,
  startAt: Timestamp, endAt: Timestamp, allDay: Schema.Boolean, startDate: NoteDate, endDate: NoteDate,
  timeZone: Schema.NullOr(Schema.String), location: Schema.NullOr(Schema.String),
  meetingUrl: Schema.NullOr(Schema.String), noteId: Schema.NullOr(UUID),
}).annotate({ identifier: "CalendarEventDTO" })
export type CalendarEventDTO = typeof CalendarEventDTO.Type
export const CalendarEventsDTO = Schema.Array(CalendarEventDTO)

export const GoogleConnectionDTO = Schema.Struct({
  id: UUID, email: Schema.String, displayName: Schema.NullOr(Schema.String),
  status: Schema.Literals(["connected", "reconnect_required"]), contactsEnabled: Schema.Boolean,
  lastSyncedAt: Schema.NullOr(Timestamp), lastSyncError: Schema.NullOr(Schema.String), createdAt: Timestamp,
}).annotate({ identifier: "GoogleConnectionDTO" })
export type GoogleConnectionDTO = typeof GoogleConnectionDTO.Type
export const GoogleStatusDTO = Schema.Struct({ configured: Schema.Boolean, redirectUri: Schema.NullOr(Schema.String), connections: Schema.Array(GoogleConnectionDTO) }).annotate({ identifier: "GoogleStatusDTO" })
export type GoogleStatusDTO = typeof GoogleStatusDTO.Type
export const GoogleConnectInput = Schema.Struct({ contacts: Schema.optionalKey(Schema.Boolean) }).annotate({ identifier: "GoogleConnectInput" })
export const GoogleConnectDTO = Schema.Struct({ authorizationUrl: Schema.String }).annotate({ identifier: "GoogleConnectDTO" })
export const GoogleCalendarDTO = Schema.Struct({
  id: UUID, connectionId: UUID, name: CalendarName, color: CalendarColor,
  timeZone: Schema.String, primary: Schema.Boolean, selected: Schema.Boolean, accessRole: Schema.String,
}).annotate({ identifier: "GoogleCalendarDTO" })
export type GoogleCalendarDTO = typeof GoogleCalendarDTO.Type
export const GoogleCalendarsDTO = Schema.Array(GoogleCalendarDTO)
export const UpdateGoogleCalendarInput = Schema.Struct({ selected: Schema.Boolean }).annotate({ identifier: "UpdateGoogleCalendarInput" })
export const MeetingParticipantDTO = Schema.Struct({
  id: UUID, email: Schema.NullOr(Schema.String), displayName: Schema.NullOr(Schema.String),
  givenName: Schema.NullOr(Schema.String), familyName: Schema.NullOr(Schema.String),
  nameSource: Schema.Literals(["calendar", "contacts", "manual", "unknown"]),
  isSelf: Schema.Boolean, isOrganizer: Schema.Boolean, isResource: Schema.Boolean,
  responseStatus: Schema.Literals(["needsAction", "declined", "tentative", "accepted", "unknown"]),
}).annotate({ identifier: "MeetingParticipantDTO" })
export type MeetingParticipantDTO = typeof MeetingParticipantDTO.Type
export const MeetingContextDTO = Schema.Struct({
  eventId: UUID, provider: Schema.Literals(["google", "ics"]), title: NoteTitle,
  calendarName: CalendarName, startAt: Timestamp, endAt: Timestamp, allDay: Schema.Boolean,
  timeZone: Schema.NullOr(Schema.String), location: Schema.NullOr(Schema.String), meetingUrl: Schema.NullOr(Schema.String), eventUrl: Schema.NullOr(Schema.String),
  organizer: Schema.NullOr(MeetingParticipantDTO), participants: Schema.Array(MeetingParticipantDTO),
}).annotate({ identifier: "MeetingContextDTO" })
export type MeetingContextDTO = typeof MeetingContextDTO.Type
export const NoteContextDTO = Schema.Struct({ noteId: UUID, meeting: Schema.NullOr(MeetingContextDTO) }).annotate({ identifier: "NoteContextDTO" })
export type NoteContextDTO = typeof NoteContextDTO.Type
export const ManualParticipantInput = Schema.Struct({
  id:UUID,
  displayName:Schema.String.check(Schema.isMinLength(1),Schema.isMaxLength(200)),
  email:Schema.optionalKey(Schema.NullOr(Schema.String.check(Schema.isPattern(/^[^\s@]+@[^\s@]+\.[^\s@]+$/),Schema.isMaxLength(254)))),
  givenName:Schema.optionalKey(Schema.NullOr(Schema.String.check(Schema.isMinLength(1),Schema.isMaxLength(200)))),
  familyName:Schema.optionalKey(Schema.NullOr(Schema.String.check(Schema.isMinLength(1),Schema.isMaxLength(200)))),
}).annotate({identifier:"ManualParticipantInput"})
export type ManualParticipantInput = typeof ManualParticipantInput.Type
export const UpdateNoteContextInput = Schema.Struct({ eventId: Schema.NullOr(UUID) }).annotate({ identifier: "UpdateNoteContextInput" })

/** List projections. A row carries what a notes list or a search result shows and
 * never a note body or a transcript; the single-note read stays the source of both. */
export const NoteListFilter = Schema.Literals(["all", "summarized", "transcribed"]).annotate({ identifier: "NoteListFilter" })
export type NoteListFilter = typeof NoteListFilter.Type
export const NoteAttendeeDTO = Schema.Struct({
  name: Schema.String.check(Schema.isMaxLength(254)),
  /** Given name, or the first word of the name: what fits in a row. */
  shortName: Schema.String.check(Schema.isMaxLength(254)),
  /** One or two letters for an avatar or a letter tile. */
  initials: Schema.String.check(Schema.isMaxLength(8)),
}).annotate({ identifier: "NoteAttendeeDTO" })
export type NoteAttendeeDTO = typeof NoteAttendeeDTO.Type
/** Invitees from the saved calendar context, without rooms and equipment. `people`
 * is capped and leaves out the account holder; the counts are not capped. */
export const NoteAttendeesDTO = Schema.Struct({
  people: Schema.Array(NoteAttendeeDTO).check(Schema.isMaxLength(4)),
  others: Revision,
  total: Revision,
}).annotate({ identifier: "NoteAttendeesDTO" })
export type NoteAttendeesDTO = typeof NoteAttendeesDTO.Type
export const NoteMeetingTimeDTO = Schema.Struct({ startAt: Timestamp, endAt: Timestamp, allDay: Schema.Boolean }).annotate({ identifier: "NoteMeetingTimeDTO" })
export const NoteFolderRefDTO = Schema.Struct({ id: UUID, name: Schema.String.check(Schema.isMaxLength(200)) }).annotate({ identifier: "NoteFolderRefDTO" })
export const NoteSummaryDTO = Schema.Struct({
  id: UUID,
  title: NoteTitle,
  date: NoteDate,
  duration: Schema.String.check(Schema.isMaxLength(32)),
  createdAt: Timestamp,
  /** Changes with any edit to the note; with the two revisions it tells a client its cached copy is stale. */
  updatedAt: Timestamp,
  recordingState: RecordingState,
  generationState: GenerationState,
  transcriptRevision: Revision,
  generatedRevision: Schema.NullOr(Revision),
  hasBody: Schema.Boolean,
  hasSummary: Schema.Boolean,
  hasTranscript: Schema.Boolean,
  speakerCount: Revision,
  /** Plain-text opening of the summary, or of the typed notes when there is no summary. */
  excerpt: Schema.String.check(Schema.isMaxLength(400)),
  meeting: Schema.NullOr(NoteMeetingTimeDTO),
  attendees: NoteAttendeesDTO,
  organizationId: Schema.NullOr(UUID),
  folders: Schema.Array(NoteFolderRefDTO).check(Schema.isMaxLength(8)),
}).annotate({ identifier: "NoteSummaryDTO" })
export type NoteSummaryDTO = typeof NoteSummaryDTO.Type
/** Newest meeting first: date, then creation time, then ID, all descending. `nextCursor`
 * is opaque and null on the last page; `total` counts every note the filter matches. */
export const NoteSummariesPageDTO = Schema.Struct({
  notes: Schema.Array(NoteSummaryDTO).check(Schema.isMaxLength(100)),
  nextCursor: Schema.NullOr(Schema.String.check(Schema.isMaxLength(512))),
  total: Revision,
}).annotate({ identifier: "NoteSummariesPageDTO" })
export type NoteSummariesPageDTO = typeof NoteSummariesPageDTO.Type
export const NoteSearchField = Schema.Literals(["title", "summary", "notes", "transcript", "people"]).annotate({ identifier: "NoteSearchField" })
export type NoteSearchField = typeof NoteSearchField.Type
/** Text split around matches, so no client has to agree on string offsets. */
export const NoteSearchTextPartDTO = Schema.Struct({ text: Schema.String.check(Schema.isMaxLength(600)), hit: Schema.Boolean }).annotate({ identifier: "NoteSearchTextPartDTO" })
export type NoteSearchTextPartDTO = typeof NoteSearchTextPartDTO.Type
export const NoteSearchExcerptDTO = Schema.Struct({
  source: NoteSearchField,
  parts: Schema.Array(NoteSearchTextPartDTO).check(Schema.isMaxLength(64)),
  /** Set for a transcript excerpt: the segment it was cut from and where that starts. */
  segmentId: Schema.NullOr(UUID),
  startMs: Schema.NullOr(Milliseconds),
}).annotate({ identifier: "NoteSearchExcerptDTO" })
export type NoteSearchExcerptDTO = typeof NoteSearchExcerptDTO.Type
export const NoteSearchResultDTO = Schema.Struct({
  note: NoteSummaryDTO,
  matchedIn: Schema.Array(NoteSearchField).check(Schema.isMaxLength(5)),
  /** The title with matches marked; empty when the note has no title. */
  title: Schema.Array(NoteSearchTextPartDTO).check(Schema.isMaxLength(64)),
  /** The first place outside the title that explains the match. */
  excerpt: Schema.NullOr(NoteSearchExcerptDTO),
}).annotate({ identifier: "NoteSearchResultDTO" })
export type NoteSearchResultDTO = typeof NoteSearchResultDTO.Type
/** Best match first. `terms` are the words the server searched for; none means the query had nothing searchable. */
export const NoteSearchPageDTO = Schema.Struct({
  results: Schema.Array(NoteSearchResultDTO).check(Schema.isMaxLength(50)),
  nextCursor: Schema.NullOr(Schema.String.check(Schema.isMaxLength(512))),
  total: Revision,
  terms: Schema.Array(Schema.String.check(Schema.isMaxLength(80))).check(Schema.isMaxLength(8)),
}).annotate({ identifier: "NoteSearchPageDTO" })
export type NoteSearchPageDTO = typeof NoteSearchPageDTO.Type

export const WireSchemas = {
  NoteAttendeeDTO, NoteAttendeesDTO, NoteMeetingTimeDTO, NoteFolderRefDTO, NoteSummaryDTO, NoteSummariesPageDTO, NoteSearchTextPartDTO, NoteSearchExcerptDTO, NoteSearchResultDTO, NoteSearchPageDTO,
  LanguageModelSelection, TranscriptionModelSelection, LiveTranscriptionModelSelection, ProviderStatusDTO, AIModelDTO, ModelCatalogInput, ModelCatalogDTO, ChatModelsDTO,
  McpWorkspaceDTO,McpAuthorizationDTO,McpConnectionDTO,McpConnectionsDTO,
  UploadNoteAttachmentInput,NoteAttachmentDTO,AgentBriefDTO,UpdateAgentBriefInput,
  OrganizationDTO, OrganizationsDTO, OrganizationMemberDTO, OrganizationInvitationDTO, OrganizationDetailsDTO, OrganizationInvitationsDTO, CreateOrganizationInput, UpdateOrganizationInput, SetActiveOrganizationInput, InviteOrganizationMemberInput, UpdateOrganizationMemberRoleInput,
  ManualParticipantInput, GoogleConnectionDTO, GoogleStatusDTO, GoogleConnectInput, GoogleConnectDTO, GoogleCalendarDTO, UpdateGoogleCalendarInput, MeetingParticipantDTO, MeetingContextDTO, NoteContextDTO, UpdateNoteContextInput,
  UserDTO, AuthSessionDTO, UsersDTO, CreateUserInput, UpdateUserRoleInput, CalendarPreferencesDTO,
  SignInSettingsDTO, UpdateSignInSettingsInput, AuthMethodsDTO, AppHandoffInput, AppHandoffDTO, AppHandoffExchangeInput, AppHandoffTokenDTO,
  AdminSettingsDTO, UpdateAdminSettingsInput, CalendarDTO, CreateCalendarInput, UpdateCalendarInput, CalendarEventDTO,
  UploadAudioChunkInput, AudioChunkDTO, ChatMessageDTO, ChatInput, ChatAnswerDTO, ChatCitationDTO, ChatToolMeetingDTO, ChatToolProgressDTO, ChatStreamEventDTO,
  EvidenceRequestDTO, EvidenceQuoteDTO, EvidenceDTO,
  TranscriptSegmentDTO, NoteDTO, CreateNoteInput, UpdateNoteInput, RecordingCommandDTO, GenerateNotesCommandDTO, GenerationProgressDTO,
  SharedNoteDTO, CreateShareInput, ShareDTO, CreatedShareDTO, HealthDTO, DeletedDTO, ApiErrorDTO,
  DownloadFileDTO, MacDownloadDTO, DownloadsDTO,
  StorageSettingsDTO, UpdateStorageSettingsInput, StorageTestInput, StorageTestDTO,
  AppsSettingsDTO, UpdateAppsSettingsInput,
  OnboardingStepsDTO, OnboardingDTO, OnboardingStepInput,
} as const

export interface NoteGenerationProvider {
  readonly name: string
  generate(input: { noteId: string; transcriptRevision: number; segments: ReadonlyArray<TranscriptSegmentDTO>; signal: AbortSignal }): Promise<{ enhanced: string }>
}
