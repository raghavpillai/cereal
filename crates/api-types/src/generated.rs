// Generated from packages/contracts/src Effect Schema definitions. Do not edit by hand.
// Example code that deserializes and serializes the model.
// extern crate serde;
// #[macro_use]
// extern crate serde_derive;
// extern crate serde_json;
//
// use generated_module::NoteAttendeeDTO;
//
// fn main() {
//     let json = r#"{"answer": 42}"#;
//     let model: NoteAttendeeDTO = serde_json::from_str(&json).unwrap();
// }

use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteSummariesPageDTO {
    pub next_cursor: Option<String>,

    pub notes: Vec<NoteSummaryDTO>,

    pub total: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteSummaryDTO {
    pub attendees: NoteAttendeesDTO,

    pub created_at: String,

    pub date: String,

    pub duration: String,

    pub excerpt: String,

    pub folders: Vec<NoteFolderRefDTO>,

    pub generated_revision: Option<i64>,

    pub generation_state: GenerationState,

    pub has_body: bool,

    pub has_summary: bool,

    pub has_transcript: bool,

    pub id: String,

    pub meeting: Option<NoteMeetingTimeDTO>,

    pub organization_id: Option<String>,

    pub recording_state: RecordingState,

    pub speaker_count: i64,

    pub title: String,

    pub transcript_revision: i64,

    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NoteAttendeesDTO {
    pub others: i64,

    pub people: Vec<NoteAttendeeDTO>,

    pub total: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteAttendeeDTO {
    pub initials: String,

    pub name: String,

    pub short_name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NoteFolderRefDTO {
    pub id: String,

    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GenerationState {
    #[serde(rename = "analyzing_transcript")]
    AnalyzingTranscript,

    Failed,

    Idle,

    Ready,

    Unsupported,

    #[serde(rename = "writing_notes")]
    WritingNotes,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteMeetingTimeDTO {
    pub all_day: bool,

    pub end_at: String,

    pub start_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordingState {
    Idle,

    Recording,

    Stopped,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteSearchPageDTO {
    pub next_cursor: Option<String>,

    pub results: Vec<NoteSearchResultDTO>,

    pub terms: Vec<String>,

    pub total: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteSearchResultDTO {
    pub excerpt: Option<NoteSearchExcerptDTO>,

    pub matched_in: Vec<NoteSearchField>,

    pub note: NoteSummaryDTO,

    pub title: Vec<NoteSearchTextPartDTO>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteSearchExcerptDTO {
    pub parts: Vec<NoteSearchTextPartDTO>,

    pub segment_id: Option<String>,

    pub source: NoteSearchField,

    pub start_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NoteSearchTextPartDTO {
    pub hit: bool,

    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoteSearchField {
    Notes,

    People,

    Summary,

    Title,

    Transcript,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCatalogInput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    pub provider: Id,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Id {
    Anthropic,

    Elevenlabs,

    Openai,

    Openrouter,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCatalogDTO {
    pub fetched_at: String,

    pub models: Vec<AiModelDTO>,

    pub notice: Option<String>,

    pub provider: Id,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiModelDTO {
    pub capabilities: Vec<Capability>,

    pub id: String,

    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    Language,

    #[serde(rename = "live_transcription")]
    LiveTranscription,

    Transcription,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatModelsDTO {
    pub models: Vec<LanguageModelSelection>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LanguageModelSelection {
    pub model: String,

    pub provider: LanguageModelSelectionProvider,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LanguageModelSelectionProvider {
    Anthropic,

    Openai,

    Openrouter,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpAuthorizationDTO {
    pub client_id: String,

    pub client_name: String,

    pub global_workspace_switch: bool,

    pub redirect_uri: String,

    pub resource: String,

    pub scopes: Vec<String>,

    pub workspace: Option<McpWorkspaceDTO>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpWorkspaceDTO {
    pub id: String,

    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpConnectionsDTO {
    pub connections: Vec<McpConnectionDTO>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpConnectionDTO {
    pub client_id: String,

    pub client_name: String,

    pub created_at: String,

    pub id: String,

    pub scopes: Vec<String>,

    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadNoteAttachmentInput {
    pub data_base64: String,

    pub mime_type: MimeType,

    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MimeType {
    #[serde(rename = "image/jpeg")]
    ImageJpeg,

    #[serde(rename = "image/png")]
    ImagePng,

    #[serde(rename = "image/webp")]
    ImageWebp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteAttachmentDTO {
    pub created_at: String,

    pub id: String,

    pub mime_type: MimeType,

    pub name: String,

    pub note_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentBriefDTO {
    pub automatically_inferred: bool,

    pub body: String,

    pub source: AgentBriefDtoSource,

    pub updated_at: Option<String>,

    pub workspace_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentBriefDtoSource {
    #[serde(rename = "explicit_user_profile")]
    ExplicitUserProfile,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpdateAgentBriefInput {
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizationsDTO {
    pub active_organization_id: Option<String>,

    pub organizations: Vec<OrganizationDTO>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizationDTO {
    pub created_at: String,

    pub id: String,

    pub name: String,

    pub role: OrganizationDtoRole,

    pub slug: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrganizationDtoRole {
    Admin,

    Member,

    Owner,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrganizationDetailsDTO {
    pub invitations: Vec<OrganizationInvitationDTO>,

    pub members: Vec<OrganizationMemberDTO>,

    pub organization: OrganizationDTO,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizationInvitationDTO {
    pub created_at: String,

    pub email: String,

    pub expires_at: String,

    pub id: String,

    pub inviter_id: String,

    pub organization_id: String,

    pub organization_name: String,

    pub role: OrganizationDtoRole,

    pub status: OrganizationInvitationDtoStatus,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrganizationInvitationDtoStatus {
    Accepted,

    Canceled,

    Expired,

    Pending,

    Rejected,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizationMemberDTO {
    pub created_at: String,

    pub email: String,

    pub id: String,

    pub name: String,

    pub organization_id: String,

    pub role: OrganizationDtoRole,

    pub user_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrganizationInvitationsDTO {
    pub invitations: Vec<OrganizationInvitationDTO>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateOrganizationInput {
    pub name: String,

    pub slug: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpdateOrganizationInput {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetActiveOrganizationInput {
    pub organization_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InviteOrganizationMemberInput {
    pub email: String,

    pub role: InviteOrganizationMemberInputRole,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InviteOrganizationMemberInputRole {
    Admin,

    Member,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpdateOrganizationMemberRoleInput {
    pub role: InviteOrganizationMemberInputRole,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManualParticipantInput {
    pub display_name: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub family_name: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub given_name: Option<String>,

    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleStatusDTO {
    pub configured: bool,

    pub connections: Vec<GoogleConnectionDTO>,

    pub redirect_uri: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleConnectionDTO {
    pub contacts_enabled: bool,

    pub created_at: String,

    pub display_name: Option<String>,

    pub email: String,

    pub id: String,

    pub last_synced_at: Option<String>,

    pub last_sync_error: Option<String>,

    pub status: GoogleConnectionDtoStatus,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoogleConnectionDtoStatus {
    Connected,

    #[serde(rename = "reconnect_required")]
    ReconnectRequired,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GoogleConnectInput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contacts: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleConnectDTO {
    pub authorization_url: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleCalendarDTO {
    pub access_role: String,

    pub color: String,

    pub connection_id: String,

    pub id: String,

    pub name: String,

    pub primary: bool,

    pub selected: bool,

    pub time_zone: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpdateGoogleCalendarInput {
    pub selected: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteContextDTO {
    pub meeting: Option<MeetingContextDTO>,

    pub note_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingContextDTO {
    pub all_day: bool,

    pub calendar_name: String,

    pub end_at: String,

    pub event_id: String,

    pub event_url: Option<String>,

    pub location: Option<String>,

    pub meeting_url: Option<String>,

    pub organizer: Option<MeetingParticipantDTO>,

    pub participants: Vec<MeetingParticipantDTO>,

    pub provider: MeetingContextDtoProvider,

    pub start_at: String,

    pub time_zone: Option<String>,

    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingParticipantDTO {
    pub display_name: Option<String>,

    pub email: Option<String>,

    pub family_name: Option<String>,

    pub given_name: Option<String>,

    pub id: String,

    pub is_organizer: bool,

    pub is_resource: bool,

    pub is_self: bool,

    pub name_source: NameSource,

    pub response_status: ResponseStatus,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NameSource {
    Calendar,

    Contacts,

    Manual,

    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ResponseStatus {
    Accepted,

    Declined,

    #[serde(rename = "needsAction")]
    NeedsAction,

    Tentative,

    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeetingContextDtoProvider {
    Google,

    Ics,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateNoteContextInput {
    pub event_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthSessionDTO {
    pub expires_at: Option<String>,

    pub user: UserDTO,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserDTO {
    pub created_at: String,

    pub email: String,

    pub id: String,

    pub name: String,

    pub role: UserDtoRole,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserDtoRole {
    Admin,

    Owner,

    User,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsersDTO {
    pub users: Vec<UserDTO>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateUserInput {
    pub email: String,

    pub name: String,

    pub password: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpdateUserRoleInput {
    pub role: UpdateUserRoleInputRole,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateUserRoleInputRole {
    Admin,

    User,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarPreferencesDTO {
    pub look_ahead_days: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthMethodsDTO {
    pub google: bool,

    pub instance_name: String,

    pub password: bool,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub setup_required: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppHandoffInput {
    pub challenge: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppHandoffDTO {
    pub code: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppHandoffExchangeInput {
    pub code: String,

    pub verifier: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppHandoffTokenDTO {
    pub token: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdminSettingsDTO {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub apps: Option<AppsSettingsDTO>,

    pub calendar_look_ahead_days: i64,

    pub calendar_refresh_minutes: i64,

    pub chat_models: Vec<LanguageModelSelection>,

    pub eleven_labs_configured: bool,

    pub eleven_labs_source: ElevenLabsSourceEnum,

    pub generation_model: LanguageModelSelection,

    pub instance_logo: Option<String>,

    pub instance_name: String,

    pub language_model: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub live_hold_seconds: Option<i64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub live_max_sessions: Option<i64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub live_transcription_model: Option<LiveTranscriptionModelSelection>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub onboarding: Option<OnboardingDTO>,

    pub open_router_configured: bool,

    pub open_router_source: ElevenLabsSourceEnum,

    pub providers: Vec<ProviderStatusDTO>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub sign_in: Option<SignInSettingsDTO>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage: Option<StorageSettingsDTO>,

    pub transcription_model: TranscriptionModelSelection,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub unused_environment: Option<Vec<String>>,

    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppsSettingsDTO {
    pub ios_app_ids: Vec<String>,

    pub ios_app_store_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ElevenLabsSourceEnum {
    Environment,

    #[serde(rename = "none")]
    SourceNone,

    Stored,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LiveTranscriptionModelSelection {
    pub model: String,

    pub provider: LiveTranscriptionModelSelectionProvider,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveTranscriptionModelSelectionProvider {
    Elevenlabs,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingDTO {
    pub completed_at: Option<String>,

    pub steps: OnboardingStepsDTO,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingStepsDTO {
    pub language: bool,

    pub models: bool,

    pub name: bool,

    pub owner: bool,

    pub sign_in: bool,

    pub speech: bool,

    pub storage: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderStatusDTO {
    pub configured: bool,

    pub id: Id,

    pub source: ElevenLabsSourceEnum,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignInSettingsDTO {
    pub allowed_emails: Vec<String>,

    pub google: bool,

    pub google_client_id: Option<String>,

    pub google_client_secret_set: bool,

    pub google_redirect_uris: Vec<String>,

    pub password: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageSettingsDTO {
    pub access_key_id: Option<String>,

    pub backend: StorageBackend,

    pub bucket: Option<String>,

    pub endpoint: Option<String>,

    pub force_path_style: bool,

    pub prefix: String,

    pub region: Option<String>,

    pub secret_access_key_set: bool,

    pub source: StorageSettingsDtoSource,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageBackend {
    Local,

    S3,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageSettingsDtoSource {
    Default,

    Environment,

    Stored,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranscriptionModelSelection {
    pub model: String,

    pub provider: TranscriptionModelSelectionProvider,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptionModelSelectionProvider {
    Elevenlabs,

    Openai,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAdminSettingsInput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anthropic_api_key: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub apps: Option<UpdateAppsSettingsInput>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub calendar_look_ahead_days: Option<i32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub calendar_refresh_minutes: Option<i32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat_models: Option<Vec<LanguageModelSelection>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub eleven_labs_api_key: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation_model: Option<LanguageModelSelection>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_logo: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_name: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub language_model: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub live_hold_seconds: Option<i32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub live_max_sessions: Option<i32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub live_transcription_model: Option<LiveTranscriptionModelSelection>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_ai_api_key: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_router_api_key: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub sign_in: Option<UpdateSignInSettingsInput>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage: Option<UpdateStorageSettingsInput>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcription_model: Option<TranscriptionModelSelection>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAppsSettingsInput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ios_app_ids: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub ios_app_store_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSignInSettingsInput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_emails: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub google: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub google_client_id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub google_client_secret: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStorageSettingsInput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_key_id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend: Option<StorageBackend>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub bucket: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub force_path_style: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret_access_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarDTO {
    pub color: String,

    pub created_at: String,

    pub enabled: bool,

    pub event_count: i64,

    pub id: String,

    pub last_synced_at: Option<String>,

    pub last_sync_error: Option<String>,

    pub name: String,

    pub provider: CalendarDtoProvider,

    pub source_host: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CalendarDtoProvider {
    Ics,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateCalendarInput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,

    pub name: String,

    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpdateCalendarInput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarEventDTO {
    pub all_day: bool,

    pub calendar_id: String,

    pub calendar_name: String,

    pub color: String,

    pub end_at: String,

    pub end_date: String,

    pub id: String,

    pub location: Option<String>,

    pub meeting_url: Option<String>,

    pub note_id: Option<String>,

    pub start_at: String,

    pub start_date: String,

    pub time_zone: Option<String>,

    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadAudioChunkInput {
    pub audio_base64: String,

    pub channel: AudioChannel,

    pub sequence: i64,

    pub start_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioChannel {
    Mic,

    System,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioChunkDTO {
    pub channel: AudioChannel,

    pub duration_ms: i64,

    pub error: Option<String>,

    pub id: String,

    pub note_id: String,

    pub sequence: i64,

    pub start_ms: i64,

    pub state: AudioChunkState,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioChunkState {
    Failed,

    Processing,

    Queued,

    Ready,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatInput {
    pub history: Vec<ChatMessageDTO>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_sources: Option<bool>,

    pub message: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_zone: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatMessageDTO {
    pub content: String,

    pub role: ChatMessageDtoRole,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatMessageDtoRole {
    Assistant,

    User,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatAnswerDTO {
    pub answer: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatStreamEventDTO {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub citations: Option<Vec<ChatCitationDTO>>,

    pub text: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<ChatToolProgressDTO>,

    #[serde(rename = "type")]
    pub chat_stream_event_dto_type: ChatStreamEventDtoType,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatStreamEventDtoType {
    Citations,

    Delta,

    Done,

    Error,

    Tool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatCitationDTO {
    pub id: String,

    pub note_id: String,

    pub quote: String,

    pub segment_id: Option<String>,

    pub source: ChatCitationDtoSource,

    pub start_ms: Option<i64>,

    pub title: String,

    pub transcript_revision: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatCitationDtoSource {
    Notes,

    Summary,

    Transcript,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatToolProgressDTO {
    pub call_id: String,

    pub label: String,

    pub name: Name,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_count: Option<i64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub results: Option<Vec<ChatToolMeetingDTO>>,

    pub status: ChatToolProgressDtoStatus,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Name {
    #[serde(rename = "get_meeting_transcript")]
    GetMeetingTranscript,

    #[serde(rename = "get_meetings")]
    GetMeetings,

    #[serde(rename = "list_meetings")]
    ListMeetings,

    #[serde(rename = "search_meetings")]
    SearchMeetings,

    #[serde(rename = "search_transcripts")]
    SearchTranscripts,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatToolMeetingDTO {
    pub date: String,

    pub note_id: String,

    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatToolProgressDtoStatus {
    Completed,

    Failed,

    Running,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceRequestDTO {
    pub expected_transcript_revision: i64,

    pub paragraph: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceDTO {
    pub quotes: Vec<EvidenceQuoteDTO>,

    pub summary: String,

    pub transcript_revision: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceQuoteDTO {
    pub segment_id: String,

    pub speaker_label: Option<String>,

    pub start_ms: i64,

    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteDTO {
    pub body: String,

    pub created_at: String,

    pub date: String,

    pub duration: String,

    pub enhanced: String,

    pub generated_revision: Option<i64>,

    pub generation_state: GenerationState,

    pub id: String,

    pub recording_state: RecordingState,

    pub title: String,

    pub transcript_revision: i64,

    pub transcript_segments: Vec<TranscriptSegmentDTO>,

    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptSegmentDTO {
    pub channel: AudioChannel,

    pub end_ms: Option<i64>,

    pub id: String,

    pub is_final: bool,

    pub speaker_id: String,

    pub speaker_label: Option<String>,

    pub start_ms: i64,

    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateNoteInput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub enhanced: Option<String>,

    pub title: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcript_segments: Option<Vec<TranscriptSegmentDTO>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateNoteInput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub enhanced: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcript_segments: Option<Vec<TranscriptSegmentDTO>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecordingCommandDTO {
    pub command: Command,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Command {
    Resume,

    Start,

    Stop,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateNotesCommandDTO {
    pub expected_transcript_revision: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationProgressDTO {
    pub note_id: String,

    pub state: GenerationState,

    pub transcript_revision: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedNoteDTO {
    pub body: String,

    pub date: String,

    pub duration: String,

    pub enhanced: String,

    pub expires_at: Option<String>,

    pub shared_at: String,

    pub share_id: String,

    pub title: String,
}

/// Explicit fields to publish as an immutable snapshot. Transcript is never publishable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateShareInput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,

    pub include_body: bool,

    pub include_enhanced: bool,
}

/// Opaque token is returned once at creation; only its SHA-256 hash is stored.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreatedShareDTO {
    pub path: String,

    pub share: ShareDTO,

    pub token: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareDTO {
    pub created_at: String,

    pub expires_at: Option<String>,

    pub id: String,

    pub note_id: String,

    pub revoked_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HealthDTO {
    pub status: HealthDtoStatus,

    pub version: Version,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthDtoStatus {
    #[serde(rename = "ok")]
    StatusOk,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Version {
    V1,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeletedDTO {
    pub deleted: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApiErrorDTO {
    pub error: ApiErrorCode,

    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiErrorCode {
    Conflict,

    Forbidden,

    #[serde(rename = "invalid_request")]
    InvalidRequest,

    #[serde(rename = "not_found")]
    NotFound,

    Unauthorized,

    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadsDTO {
    pub ios_app_store_url: Option<String>,

    pub mac: Option<MacDownloadDTO>,

    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MacDownloadDTO {
    pub arch: String,

    pub installer: DownloadFileDTO,

    pub minimum_system: String,

    pub update: Option<DownloadFileDTO>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DownloadFileDTO {
    pub file: String,

    pub sha256: String,

    pub size: i64,

    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageTestInput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_key_id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend: Option<StorageBackend>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub bucket: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub force_path_style: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret_access_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StorageTestDTO {
    pub backend: StorageBackend,

    pub message: String,

    pub ok: bool,

    pub step: Option<Step>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    Delete,

    Read,

    Write,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OnboardingStepInput {
    pub state: State,

    pub step: OnboardingStep,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Done,

    Skipped,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OnboardingStep {
    Language,

    Models,

    Name,

    Owner,

    #[serde(rename = "signIn")]
    SignIn,

    Speech,

    Storage,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveHelloMessage {
    pub capture_id: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,

    pub protocol: f64,

    pub streams: Vec<LiveStreamHello>,

    #[serde(rename = "type")]
    pub live_hello_message_type: LiveHelloMessageType,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveHelloMessageType {
    Hello,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveStreamHello {
    pub base_ms: i64,

    pub channel: AudioChannel,

    pub encoding: Encoding,

    pub resume_from_sample: i64,

    pub sample_rate: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Encoding {
    #[serde(rename = "pcm_s16le")]
    PcmS16Le,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveMarkMessage {
    pub at_sample: AtSample,

    pub kind: LiveMarkKind,

    #[serde(rename = "type")]
    pub live_mark_message_type: LiveMarkMessageType,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AtSample {
    pub mic: Option<i64>,

    pub system: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveMarkKind {
    End,

    Pause,

    Resume,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveMarkMessageType {
    Mark,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveReadyMessage {
    pub model: String,

    pub provider: String,

    pub resume_window_ms: i64,

    pub session_id: String,

    pub streams: Vec<LiveReadyStream>,

    pub transcript_revision: i64,

    #[serde(rename = "type")]
    pub live_ready_message_type: LiveReadyMessageType,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveReadyMessageType {
    Ready,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveReadyStream {
    pub accept_from_sample: i64,

    pub channel: AudioChannel,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveAckMessage {
    pub channel: AudioChannel,

    pub through_sample: i64,

    pub transcribed_through_ms: i64,

    #[serde(rename = "type")]
    pub live_ack_message_type: LiveAckMessageType,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveAckMessageType {
    Ack,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveInterimMessage {
    pub channel: AudioChannel,

    pub end_ms: i64,

    pub rev: i64,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub speaker_id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub speaker_label: Option<String>,

    pub start_ms: i64,

    pub text: String,

    #[serde(rename = "type")]
    pub live_interim_message_type: LiveInterimMessageType,

    pub utterance_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveInterimMessageType {
    Interim,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveFinalMessage {
    pub segment: TranscriptSegmentDTO,

    pub transcript_revision: i64,

    #[serde(rename = "type")]
    pub live_final_message_type: LiveFinalMessageType,

    pub utterance_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveFinalMessageType {
    Final,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LiveSpeakersMessage {
    pub merges: Vec<LiveSpeakerMerge>,

    #[serde(rename = "type")]
    pub live_speakers_message_type: LiveSpeakersMessageType,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveSpeakersMessageType {
    Speakers,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LiveSpeakerMerge {
    pub from: String,

    pub to: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveGapMessage {
    pub channel: AudioChannel,

    pub from_ms: i64,

    pub reason: LiveGapReason,

    pub to_ms: i64,

    #[serde(rename = "type")]
    pub live_gap_message_type: LiveGapMessageType,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveGapMessageType {
    Gap,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveGapReason {
    #[serde(rename = "provider_unavailable")]
    ProviderUnavailable,

    #[serde(rename = "resume_window_exceeded")]
    ResumeWindowExceeded,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveStatusMessage {
    pub channel: AudioChannel,

    pub lag_ms: i64,

    pub state: LiveStreamState,

    #[serde(rename = "type")]
    pub live_status_message_type: LiveStatusMessageType,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveStatusMessageType {
    Status,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveStreamState {
    #[serde(rename = "catching_up")]
    CatchingUp,

    Live,

    Paused,

    #[serde(rename = "provider_unavailable")]
    ProviderUnavailable,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveErrorMessage {
    pub code: LiveErrorCode,

    pub message: String,

    pub retryable: bool,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_after_ms: Option<i64>,

    #[serde(rename = "type")]
    pub live_error_message_type: LiveErrorMessageType,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveErrorCode {
    #[serde(rename = "client_too_slow")]
    ClientTooSlow,

    Malformed,

    #[serde(rename = "not_found")]
    NotFound,

    #[serde(rename = "over_limit")]
    OverLimit,

    #[serde(rename = "provider_unavailable")]
    ProviderUnavailable,

    Replaced,

    Unauthenticated,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveErrorMessageType {
    Error,
}
