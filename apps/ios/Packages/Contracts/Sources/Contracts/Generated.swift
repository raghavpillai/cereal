// Generated from packages/contracts/src Effect Schema definitions. Do not edit by hand.
// This file was generated from JSON Schema using quicktype, do not modify it directly.
// To parse the JSON, add this file to your project and do:
//
//   let noteAttendeeDTO = try? JSONDecoder().decode(NoteAttendeeDTO.self, from: jsonData)
//   let noteAttendeesDTO = try? JSONDecoder().decode(NoteAttendeesDTO.self, from: jsonData)
//   let noteMeetingTimeDTO = try? JSONDecoder().decode(NoteMeetingTimeDTO.self, from: jsonData)
//   let noteFolderRefDTO = try? JSONDecoder().decode(NoteFolderRefDTO.self, from: jsonData)
//   let noteSummaryDTO = try? JSONDecoder().decode(NoteSummaryDTO.self, from: jsonData)
//   let noteSummariesPageDTO = try? JSONDecoder().decode(NoteSummariesPageDTO.self, from: jsonData)
//   let noteSearchTextPartDTO = try? JSONDecoder().decode(NoteSearchTextPartDTO.self, from: jsonData)
//   let noteSearchExcerptDTO = try? JSONDecoder().decode(NoteSearchExcerptDTO.self, from: jsonData)
//   let noteSearchResultDTO = try? JSONDecoder().decode(NoteSearchResultDTO.self, from: jsonData)
//   let noteSearchPageDTO = try? JSONDecoder().decode(NoteSearchPageDTO.self, from: jsonData)
//   let languageModelSelection = try? JSONDecoder().decode(LanguageModelSelection.self, from: jsonData)
//   let transcriptionModelSelection = try? JSONDecoder().decode(TranscriptionModelSelection.self, from: jsonData)
//   let liveTranscriptionModelSelection = try? JSONDecoder().decode(LiveTranscriptionModelSelection.self, from: jsonData)
//   let providerStatusDTO = try? JSONDecoder().decode(ProviderStatusDTO.self, from: jsonData)
//   let aIModelDTO = try? JSONDecoder().decode(AIModelDTO.self, from: jsonData)
//   let modelCatalogInput = try? JSONDecoder().decode(ModelCatalogInput.self, from: jsonData)
//   let modelCatalogDTO = try? JSONDecoder().decode(ModelCatalogDTO.self, from: jsonData)
//   let chatModelsDTO = try? JSONDecoder().decode(ChatModelsDTO.self, from: jsonData)
//   let mCPWorkspaceDTO = try? JSONDecoder().decode(MCPWorkspaceDTO.self, from: jsonData)
//   let mCPAuthorizationDTO = try? JSONDecoder().decode(MCPAuthorizationDTO.self, from: jsonData)
//   let mCPConnectionDTO = try? JSONDecoder().decode(MCPConnectionDTO.self, from: jsonData)
//   let mCPConnectionsDTO = try? JSONDecoder().decode(MCPConnectionsDTO.self, from: jsonData)
//   let uploadNoteAttachmentInput = try? JSONDecoder().decode(UploadNoteAttachmentInput.self, from: jsonData)
//   let noteAttachmentDTO = try? JSONDecoder().decode(NoteAttachmentDTO.self, from: jsonData)
//   let agentBriefDTO = try? JSONDecoder().decode(AgentBriefDTO.self, from: jsonData)
//   let updateAgentBriefInput = try? JSONDecoder().decode(UpdateAgentBriefInput.self, from: jsonData)
//   let organizationDTO = try? JSONDecoder().decode(OrganizationDTO.self, from: jsonData)
//   let organizationsDTO = try? JSONDecoder().decode(OrganizationsDTO.self, from: jsonData)
//   let organizationMemberDTO = try? JSONDecoder().decode(OrganizationMemberDTO.self, from: jsonData)
//   let organizationInvitationDTO = try? JSONDecoder().decode(OrganizationInvitationDTO.self, from: jsonData)
//   let organizationDetailsDTO = try? JSONDecoder().decode(OrganizationDetailsDTO.self, from: jsonData)
//   let organizationInvitationsDTO = try? JSONDecoder().decode(OrganizationInvitationsDTO.self, from: jsonData)
//   let createOrganizationInput = try? JSONDecoder().decode(CreateOrganizationInput.self, from: jsonData)
//   let updateOrganizationInput = try? JSONDecoder().decode(UpdateOrganizationInput.self, from: jsonData)
//   let setActiveOrganizationInput = try? JSONDecoder().decode(SetActiveOrganizationInput.self, from: jsonData)
//   let inviteOrganizationMemberInput = try? JSONDecoder().decode(InviteOrganizationMemberInput.self, from: jsonData)
//   let updateOrganizationMemberRoleInput = try? JSONDecoder().decode(UpdateOrganizationMemberRoleInput.self, from: jsonData)
//   let manualParticipantInput = try? JSONDecoder().decode(ManualParticipantInput.self, from: jsonData)
//   let googleConnectionDTO = try? JSONDecoder().decode(GoogleConnectionDTO.self, from: jsonData)
//   let googleStatusDTO = try? JSONDecoder().decode(GoogleStatusDTO.self, from: jsonData)
//   let googleConnectInput = try? JSONDecoder().decode(GoogleConnectInput.self, from: jsonData)
//   let googleConnectDTO = try? JSONDecoder().decode(GoogleConnectDTO.self, from: jsonData)
//   let googleCalendarDTO = try? JSONDecoder().decode(GoogleCalendarDTO.self, from: jsonData)
//   let updateGoogleCalendarInput = try? JSONDecoder().decode(UpdateGoogleCalendarInput.self, from: jsonData)
//   let meetingParticipantDTO = try? JSONDecoder().decode(MeetingParticipantDTO.self, from: jsonData)
//   let meetingContextDTO = try? JSONDecoder().decode(MeetingContextDTO.self, from: jsonData)
//   let noteContextDTO = try? JSONDecoder().decode(NoteContextDTO.self, from: jsonData)
//   let updateNoteContextInput = try? JSONDecoder().decode(UpdateNoteContextInput.self, from: jsonData)
//   let userDTO = try? JSONDecoder().decode(UserDTO.self, from: jsonData)
//   let authSessionDTO = try? JSONDecoder().decode(AuthSessionDTO.self, from: jsonData)
//   let usersDTO = try? JSONDecoder().decode(UsersDTO.self, from: jsonData)
//   let createUserInput = try? JSONDecoder().decode(CreateUserInput.self, from: jsonData)
//   let updateUserRoleInput = try? JSONDecoder().decode(UpdateUserRoleInput.self, from: jsonData)
//   let calendarPreferencesDTO = try? JSONDecoder().decode(CalendarPreferencesDTO.self, from: jsonData)
//   let signInSettingsDTO = try? JSONDecoder().decode(SignInSettingsDTO.self, from: jsonData)
//   let updateSignInSettingsInput = try? JSONDecoder().decode(UpdateSignInSettingsInput.self, from: jsonData)
//   let authMethodsDTO = try? JSONDecoder().decode(AuthMethodsDTO.self, from: jsonData)
//   let appHandoffInput = try? JSONDecoder().decode(AppHandoffInput.self, from: jsonData)
//   let appHandoffDTO = try? JSONDecoder().decode(AppHandoffDTO.self, from: jsonData)
//   let appHandoffExchangeInput = try? JSONDecoder().decode(AppHandoffExchangeInput.self, from: jsonData)
//   let appHandoffTokenDTO = try? JSONDecoder().decode(AppHandoffTokenDTO.self, from: jsonData)
//   let adminSettingsDTO = try? JSONDecoder().decode(AdminSettingsDTO.self, from: jsonData)
//   let updateAdminSettingsInput = try? JSONDecoder().decode(UpdateAdminSettingsInput.self, from: jsonData)
//   let calendarDTO = try? JSONDecoder().decode(CalendarDTO.self, from: jsonData)
//   let createCalendarInput = try? JSONDecoder().decode(CreateCalendarInput.self, from: jsonData)
//   let updateCalendarInput = try? JSONDecoder().decode(UpdateCalendarInput.self, from: jsonData)
//   let calendarEventDTO = try? JSONDecoder().decode(CalendarEventDTO.self, from: jsonData)
//   let uploadAudioChunkInput = try? JSONDecoder().decode(UploadAudioChunkInput.self, from: jsonData)
//   let audioChunkDTO = try? JSONDecoder().decode(AudioChunkDTO.self, from: jsonData)
//   let chatMessageDTO = try? JSONDecoder().decode(ChatMessageDTO.self, from: jsonData)
//   let chatInput = try? JSONDecoder().decode(ChatInput.self, from: jsonData)
//   let chatAnswerDTO = try? JSONDecoder().decode(ChatAnswerDTO.self, from: jsonData)
//   let chatCitationDTO = try? JSONDecoder().decode(ChatCitationDTO.self, from: jsonData)
//   let chatToolMeetingDTO = try? JSONDecoder().decode(ChatToolMeetingDTO.self, from: jsonData)
//   let chatToolProgressDTO = try? JSONDecoder().decode(ChatToolProgressDTO.self, from: jsonData)
//   let chatStreamEventDTO = try? JSONDecoder().decode(ChatStreamEventDTO.self, from: jsonData)
//   let evidenceRequestDTO = try? JSONDecoder().decode(EvidenceRequestDTO.self, from: jsonData)
//   let evidenceQuoteDTO = try? JSONDecoder().decode(EvidenceQuoteDTO.self, from: jsonData)
//   let evidenceDTO = try? JSONDecoder().decode(EvidenceDTO.self, from: jsonData)
//   let transcriptSegmentDTO = try? JSONDecoder().decode(TranscriptSegmentDTO.self, from: jsonData)
//   let noteDTO = try? JSONDecoder().decode(NoteDTO.self, from: jsonData)
//   let createNoteInput = try? JSONDecoder().decode(CreateNoteInput.self, from: jsonData)
//   let updateNoteInput = try? JSONDecoder().decode(UpdateNoteInput.self, from: jsonData)
//   let importedRecordingState = try? JSONDecoder().decode(ImportedRecordingState.self, from: jsonData)
//   let importNoteInput = try? JSONDecoder().decode(ImportNoteInput.self, from: jsonData)
//   let importedSegmentIdDTO = try? JSONDecoder().decode(ImportedSegmentIdDTO.self, from: jsonData)
//   let importedNoteDTO = try? JSONDecoder().decode(ImportedNoteDTO.self, from: jsonData)
//   let recordingCommandDTO = try? JSONDecoder().decode(RecordingCommandDTO.self, from: jsonData)
//   let generateNotesCommandDTO = try? JSONDecoder().decode(GenerateNotesCommandDTO.self, from: jsonData)
//   let generationProgressDTO = try? JSONDecoder().decode(GenerationProgressDTO.self, from: jsonData)
//   let sharedNoteDTO = try? JSONDecoder().decode(SharedNoteDTO.self, from: jsonData)
//   let createShareInput = try? JSONDecoder().decode(CreateShareInput.self, from: jsonData)
//   let shareDTO = try? JSONDecoder().decode(ShareDTO.self, from: jsonData)
//   let createdShareDTO = try? JSONDecoder().decode(CreatedShareDTO.self, from: jsonData)
//   let healthDTO = try? JSONDecoder().decode(HealthDTO.self, from: jsonData)
//   let deletedDTO = try? JSONDecoder().decode(DeletedDTO.self, from: jsonData)
//   let aPIErrorDTO = try? JSONDecoder().decode(APIErrorDTO.self, from: jsonData)
//   let downloadFileDTO = try? JSONDecoder().decode(DownloadFileDTO.self, from: jsonData)
//   let mACDownloadDTO = try? JSONDecoder().decode(MACDownloadDTO.self, from: jsonData)
//   let downloadsDTO = try? JSONDecoder().decode(DownloadsDTO.self, from: jsonData)
//   let storageSettingsDTO = try? JSONDecoder().decode(StorageSettingsDTO.self, from: jsonData)
//   let updateStorageSettingsInput = try? JSONDecoder().decode(UpdateStorageSettingsInput.self, from: jsonData)
//   let storageTestInput = try? JSONDecoder().decode(StorageTestInput.self, from: jsonData)
//   let storageTestDTO = try? JSONDecoder().decode(StorageTestDTO.self, from: jsonData)
//   let appsSettingsDTO = try? JSONDecoder().decode(AppsSettingsDTO.self, from: jsonData)
//   let updateAppsSettingsInput = try? JSONDecoder().decode(UpdateAppsSettingsInput.self, from: jsonData)
//   let onboardingStepsDTO = try? JSONDecoder().decode(OnboardingStepsDTO.self, from: jsonData)
//   let onboardingDTO = try? JSONDecoder().decode(OnboardingDTO.self, from: jsonData)
//   let onboardingStepInput = try? JSONDecoder().decode(OnboardingStepInput.self, from: jsonData)
//   let googleVerificationInput = try? JSONDecoder().decode(GoogleVerificationInput.self, from: jsonData)
//   let googleVerificationStartDTO = try? JSONDecoder().decode(GoogleVerificationStartDTO.self, from: jsonData)
//   let googleVerificationStatusDTO = try? JSONDecoder().decode(GoogleVerificationStatusDTO.self, from: jsonData)
//   let switchToPasswordInput = try? JSONDecoder().decode(SwitchToPasswordInput.self, from: jsonData)
//   let liveHelloMessage = try? JSONDecoder().decode(LiveHelloMessage.self, from: jsonData)
//   let liveMarkMessage = try? JSONDecoder().decode(LiveMarkMessage.self, from: jsonData)
//   let liveReadyMessage = try? JSONDecoder().decode(LiveReadyMessage.self, from: jsonData)
//   let liveAckMessage = try? JSONDecoder().decode(LiveAckMessage.self, from: jsonData)
//   let liveInterimMessage = try? JSONDecoder().decode(LiveInterimMessage.self, from: jsonData)
//   let liveFinalMessage = try? JSONDecoder().decode(LiveFinalMessage.self, from: jsonData)
//   let liveSpeakersMessage = try? JSONDecoder().decode(LiveSpeakersMessage.self, from: jsonData)
//   let liveGapMessage = try? JSONDecoder().decode(LiveGapMessage.self, from: jsonData)
//   let liveStatusMessage = try? JSONDecoder().decode(LiveStatusMessage.self, from: jsonData)
//   let liveErrorMessage = try? JSONDecoder().decode(LiveErrorMessage.self, from: jsonData)

import Foundation

// MARK: - NoteSummariesPageDTO
public struct NoteSummariesPageDTO: Codable, Sendable {
    public let nextCursor: String?
    public let notes: [NoteSummaryDTO]
    public let total: Int

    public enum CodingKeys: String, CodingKey {
        case nextCursor = "nextCursor"
        case notes = "notes"
        case total = "total"
    }

    public init(nextCursor: String?, notes: [NoteSummaryDTO], total: Int) {
        self.nextCursor = nextCursor
        self.notes = notes
        self.total = total
    }
}

// MARK: - NoteSummaryDTO
public struct NoteSummaryDTO: Codable, Sendable {
    public let attendees: NoteAttendeesDTO
    public let createdAt: String
    public let date: String
    public let duration: String
    public let excerpt: String
    public let folders: [NoteFolderRefDTO]
    public let generatedRevision: Int?
    public let generationState: GenerationState
    public let hasBody: Bool
    public let hasSummary: Bool
    public let hasTranscript: Bool
    public let id: String
    public let meeting: NoteMeetingTimeDTO?
    public let organizationId: String?
    public let recordingState: RecordingState
    public let speakerCount: Int
    public let title: String
    public let transcriptRevision: Int
    public let updatedAt: String

    public enum CodingKeys: String, CodingKey {
        case attendees = "attendees"
        case createdAt = "createdAt"
        case date = "date"
        case duration = "duration"
        case excerpt = "excerpt"
        case folders = "folders"
        case generatedRevision = "generatedRevision"
        case generationState = "generationState"
        case hasBody = "hasBody"
        case hasSummary = "hasSummary"
        case hasTranscript = "hasTranscript"
        case id = "id"
        case meeting = "meeting"
        case organizationId = "organizationId"
        case recordingState = "recordingState"
        case speakerCount = "speakerCount"
        case title = "title"
        case transcriptRevision = "transcriptRevision"
        case updatedAt = "updatedAt"
    }

    public init(attendees: NoteAttendeesDTO, createdAt: String, date: String, duration: String, excerpt: String, folders: [NoteFolderRefDTO], generatedRevision: Int?, generationState: GenerationState, hasBody: Bool, hasSummary: Bool, hasTranscript: Bool, id: String, meeting: NoteMeetingTimeDTO?, organizationId: String?, recordingState: RecordingState, speakerCount: Int, title: String, transcriptRevision: Int, updatedAt: String) {
        self.attendees = attendees
        self.createdAt = createdAt
        self.date = date
        self.duration = duration
        self.excerpt = excerpt
        self.folders = folders
        self.generatedRevision = generatedRevision
        self.generationState = generationState
        self.hasBody = hasBody
        self.hasSummary = hasSummary
        self.hasTranscript = hasTranscript
        self.id = id
        self.meeting = meeting
        self.organizationId = organizationId
        self.recordingState = recordingState
        self.speakerCount = speakerCount
        self.title = title
        self.transcriptRevision = transcriptRevision
        self.updatedAt = updatedAt
    }
}

// MARK: - NoteAttendeesDTO
public struct NoteAttendeesDTO: Codable, Sendable {
    public let others: Int
    public let people: [NoteAttendeeDTO]
    public let total: Int

    public enum CodingKeys: String, CodingKey {
        case others = "others"
        case people = "people"
        case total = "total"
    }

    public init(others: Int, people: [NoteAttendeeDTO], total: Int) {
        self.others = others
        self.people = people
        self.total = total
    }
}

// MARK: - NoteAttendeeDTO
public struct NoteAttendeeDTO: Codable, Sendable {
    public let initials: String
    public let name: String
    public let shortName: String

    public enum CodingKeys: String, CodingKey {
        case initials = "initials"
        case name = "name"
        case shortName = "shortName"
    }

    public init(initials: String, name: String, shortName: String) {
        self.initials = initials
        self.name = name
        self.shortName = shortName
    }
}

// MARK: - NoteFolderRefDTO
public struct NoteFolderRefDTO: Codable, Sendable {
    public let id: String
    public let name: String

    public enum CodingKeys: String, CodingKey {
        case id = "id"
        case name = "name"
    }

    public init(id: String, name: String) {
        self.id = id
        self.name = name
    }
}

public enum GenerationState: String, Codable, Sendable {
    case analyzingTranscript = "analyzing_transcript"
    case failed = "failed"
    case idle = "idle"
    case ready = "ready"
    case unsupported = "unsupported"
    case writingNotes = "writing_notes"
}

// MARK: - NoteMeetingTimeDTO
public struct NoteMeetingTimeDTO: Codable, Sendable {
    public let allDay: Bool
    public let endAt: String
    public let startAt: String

    public enum CodingKeys: String, CodingKey {
        case allDay = "allDay"
        case endAt = "endAt"
        case startAt = "startAt"
    }

    public init(allDay: Bool, endAt: String, startAt: String) {
        self.allDay = allDay
        self.endAt = endAt
        self.startAt = startAt
    }
}

public enum RecordingState: String, Codable, Sendable {
    case idle = "idle"
    case recording = "recording"
    case stopped = "stopped"
}

// MARK: - NoteSearchPageDTO
public struct NoteSearchPageDTO: Codable, Sendable {
    public let nextCursor: String?
    public let results: [NoteSearchResultDTO]
    public let terms: [String]
    public let total: Int

    public enum CodingKeys: String, CodingKey {
        case nextCursor = "nextCursor"
        case results = "results"
        case terms = "terms"
        case total = "total"
    }

    public init(nextCursor: String?, results: [NoteSearchResultDTO], terms: [String], total: Int) {
        self.nextCursor = nextCursor
        self.results = results
        self.terms = terms
        self.total = total
    }
}

// MARK: - NoteSearchResultDTO
public struct NoteSearchResultDTO: Codable, Sendable {
    public let excerpt: NoteSearchExcerptDTO?
    public let matchedIn: [NoteSearchField]
    public let note: NoteSummaryDTO
    public let title: [NoteSearchTextPartDTO]

    public enum CodingKeys: String, CodingKey {
        case excerpt = "excerpt"
        case matchedIn = "matchedIn"
        case note = "note"
        case title = "title"
    }

    public init(excerpt: NoteSearchExcerptDTO?, matchedIn: [NoteSearchField], note: NoteSummaryDTO, title: [NoteSearchTextPartDTO]) {
        self.excerpt = excerpt
        self.matchedIn = matchedIn
        self.note = note
        self.title = title
    }
}

// MARK: - NoteSearchExcerptDTO
public struct NoteSearchExcerptDTO: Codable, Sendable {
    public let parts: [NoteSearchTextPartDTO]
    public let segmentId: String?
    public let source: NoteSearchField
    public let startMs: Int?

    public enum CodingKeys: String, CodingKey {
        case parts = "parts"
        case segmentId = "segmentId"
        case source = "source"
        case startMs = "startMs"
    }

    public init(parts: [NoteSearchTextPartDTO], segmentId: String?, source: NoteSearchField, startMs: Int?) {
        self.parts = parts
        self.segmentId = segmentId
        self.source = source
        self.startMs = startMs
    }
}

// MARK: - NoteSearchTextPartDTO
public struct NoteSearchTextPartDTO: Codable, Sendable {
    public let hit: Bool
    public let text: String

    public enum CodingKeys: String, CodingKey {
        case hit = "hit"
        case text = "text"
    }

    public init(hit: Bool, text: String) {
        self.hit = hit
        self.text = text
    }
}

public enum NoteSearchField: String, Codable, Sendable {
    case notes = "notes"
    case people = "people"
    case summary = "summary"
    case title = "title"
    case transcript = "transcript"
}

// MARK: - ModelCatalogInput
public struct ModelCatalogInput: Codable, Sendable {
    public let apiKey: String?
    public let provider: ID
    public let refresh: Bool?

    public enum CodingKeys: String, CodingKey {
        case apiKey = "apiKey"
        case provider = "provider"
        case refresh = "refresh"
    }

    public init(apiKey: String? = nil, provider: ID, refresh: Bool? = nil) {
        self.apiKey = apiKey
        self.provider = provider
        self.refresh = refresh
    }
}

public enum ID: String, Codable, Sendable {
    case anthropic = "anthropic"
    case elevenlabs = "elevenlabs"
    case openai = "openai"
    case openrouter = "openrouter"
}

// MARK: - ModelCatalogDTO
public struct ModelCatalogDTO: Codable, Sendable {
    public let fetchedAt: String
    public let models: [AIModelDTO]
    public let notice: String?
    public let provider: ID

    public enum CodingKeys: String, CodingKey {
        case fetchedAt = "fetchedAt"
        case models = "models"
        case notice = "notice"
        case provider = "provider"
    }

    public init(fetchedAt: String, models: [AIModelDTO], notice: String?, provider: ID) {
        self.fetchedAt = fetchedAt
        self.models = models
        self.notice = notice
        self.provider = provider
    }
}

// MARK: - AIModelDTO
public struct AIModelDTO: Codable, Sendable {
    public let capabilities: [Capability]
    public let id: String
    public let name: String

    public enum CodingKeys: String, CodingKey {
        case capabilities = "capabilities"
        case id = "id"
        case name = "name"
    }

    public init(capabilities: [Capability], id: String, name: String) {
        self.capabilities = capabilities
        self.id = id
        self.name = name
    }
}

public enum Capability: String, Codable, Sendable {
    case language = "language"
    case liveTranscription = "live_transcription"
    case transcription = "transcription"
}

// MARK: - ChatModelsDTO
public struct ChatModelsDTO: Codable, Sendable {
    public let models: [LanguageModelSelection]

    public enum CodingKeys: String, CodingKey {
        case models = "models"
    }

    public init(models: [LanguageModelSelection]) {
        self.models = models
    }
}

// MARK: - LanguageModelSelection
public struct LanguageModelSelection: Codable, Sendable {
    public let model: String
    public let provider: LanguageModelSelectionProvider

    public enum CodingKeys: String, CodingKey {
        case model = "model"
        case provider = "provider"
    }

    public init(model: String, provider: LanguageModelSelectionProvider) {
        self.model = model
        self.provider = provider
    }
}

public enum LanguageModelSelectionProvider: String, Codable, Sendable {
    case anthropic = "anthropic"
    case openai = "openai"
    case openrouter = "openrouter"
}

// MARK: - MCPAuthorizationDTO
public struct MCPAuthorizationDTO: Codable, Sendable {
    public let clientId: String
    public let clientName: String
    public let globalWorkspaceSwitch: Bool
    public let redirectUri: String
    public let resource: String
    public let scopes: [String]
    public let workspace: MCPWorkspaceDTO?

    public enum CodingKeys: String, CodingKey {
        case clientId = "clientId"
        case clientName = "clientName"
        case globalWorkspaceSwitch = "globalWorkspaceSwitch"
        case redirectUri = "redirectUri"
        case resource = "resource"
        case scopes = "scopes"
        case workspace = "workspace"
    }

    public init(clientId: String, clientName: String, globalWorkspaceSwitch: Bool, redirectUri: String, resource: String, scopes: [String], workspace: MCPWorkspaceDTO?) {
        self.clientId = clientId
        self.clientName = clientName
        self.globalWorkspaceSwitch = globalWorkspaceSwitch
        self.redirectUri = redirectUri
        self.resource = resource
        self.scopes = scopes
        self.workspace = workspace
    }
}

// MARK: - MCPWorkspaceDTO
public struct MCPWorkspaceDTO: Codable, Sendable {
    public let id: String
    public let name: String

    public enum CodingKeys: String, CodingKey {
        case id = "id"
        case name = "name"
    }

    public init(id: String, name: String) {
        self.id = id
        self.name = name
    }
}

// MARK: - MCPConnectionsDTO
public struct MCPConnectionsDTO: Codable, Sendable {
    public let connections: [MCPConnectionDTO]

    public enum CodingKeys: String, CodingKey {
        case connections = "connections"
    }

    public init(connections: [MCPConnectionDTO]) {
        self.connections = connections
    }
}

// MARK: - MCPConnectionDTO
public struct MCPConnectionDTO: Codable, Sendable {
    public let clientId: String
    public let clientName: String
    public let createdAt: String
    public let id: String
    public let scopes: [String]
    public let updatedAt: String

    public enum CodingKeys: String, CodingKey {
        case clientId = "clientId"
        case clientName = "clientName"
        case createdAt = "createdAt"
        case id = "id"
        case scopes = "scopes"
        case updatedAt = "updatedAt"
    }

    public init(clientId: String, clientName: String, createdAt: String, id: String, scopes: [String], updatedAt: String) {
        self.clientId = clientId
        self.clientName = clientName
        self.createdAt = createdAt
        self.id = id
        self.scopes = scopes
        self.updatedAt = updatedAt
    }
}

// MARK: - UploadNoteAttachmentInput
public struct UploadNoteAttachmentInput: Codable, Sendable {
    public let dataBase64: String
    public let mimeType: MIMEType
    public let name: String

    public enum CodingKeys: String, CodingKey {
        case dataBase64 = "dataBase64"
        case mimeType = "mimeType"
        case name = "name"
    }

    public init(dataBase64: String, mimeType: MIMEType, name: String) {
        self.dataBase64 = dataBase64
        self.mimeType = mimeType
        self.name = name
    }
}

public enum MIMEType: String, Codable, Sendable {
    case imageWebp = "image/webp"
    case imagejpeg = "image/jpeg"
    case imagepng = "image/png"
}

// MARK: - NoteAttachmentDTO
public struct NoteAttachmentDTO: Codable, Sendable {
    public let createdAt: String
    public let id: String
    public let mimeType: MIMEType
    public let name: String
    public let noteId: String

    public enum CodingKeys: String, CodingKey {
        case createdAt = "createdAt"
        case id = "id"
        case mimeType = "mimeType"
        case name = "name"
        case noteId = "noteId"
    }

    public init(createdAt: String, id: String, mimeType: MIMEType, name: String, noteId: String) {
        self.createdAt = createdAt
        self.id = id
        self.mimeType = mimeType
        self.name = name
        self.noteId = noteId
    }
}

// MARK: - AgentBriefDTO
public struct AgentBriefDTO: Codable, Sendable {
    public let automaticallyInferred: Bool
    public let body: String
    public let source: AgentBriefDTOSource
    public let updatedAt: String?
    public let workspaceId: String

    public enum CodingKeys: String, CodingKey {
        case automaticallyInferred = "automaticallyInferred"
        case body = "body"
        case source = "source"
        case updatedAt = "updatedAt"
        case workspaceId = "workspaceId"
    }

    public init(automaticallyInferred: Bool, body: String, source: AgentBriefDTOSource, updatedAt: String?, workspaceId: String) {
        self.automaticallyInferred = automaticallyInferred
        self.body = body
        self.source = source
        self.updatedAt = updatedAt
        self.workspaceId = workspaceId
    }
}

public enum AgentBriefDTOSource: String, Codable, Sendable {
    case explicitUserProfile = "explicit_user_profile"
}

// MARK: - UpdateAgentBriefInput
public struct UpdateAgentBriefInput: Codable, Sendable {
    public let body: String

    public enum CodingKeys: String, CodingKey {
        case body = "body"
    }

    public init(body: String) {
        self.body = body
    }
}

// MARK: - OrganizationsDTO
public struct OrganizationsDTO: Codable, Sendable {
    public let activeOrganizationId: String?
    public let organizations: [OrganizationDTO]

    public enum CodingKeys: String, CodingKey {
        case activeOrganizationId = "activeOrganizationId"
        case organizations = "organizations"
    }

    public init(activeOrganizationId: String?, organizations: [OrganizationDTO]) {
        self.activeOrganizationId = activeOrganizationId
        self.organizations = organizations
    }
}

// MARK: - OrganizationDTO
public struct OrganizationDTO: Codable, Sendable {
    public let createdAt: String
    public let id: String
    public let name: String
    public let role: OrganizationDTORole
    public let slug: String

    public enum CodingKeys: String, CodingKey {
        case createdAt = "createdAt"
        case id = "id"
        case name = "name"
        case role = "role"
        case slug = "slug"
    }

    public init(createdAt: String, id: String, name: String, role: OrganizationDTORole, slug: String) {
        self.createdAt = createdAt
        self.id = id
        self.name = name
        self.role = role
        self.slug = slug
    }
}

public enum OrganizationDTORole: String, Codable, Sendable {
    case admin = "admin"
    case member = "member"
    case owner = "owner"
}

// MARK: - OrganizationDetailsDTO
public struct OrganizationDetailsDTO: Codable, Sendable {
    public let invitations: [OrganizationInvitationDTO]
    public let members: [OrganizationMemberDTO]
    public let organization: OrganizationDTO

    public enum CodingKeys: String, CodingKey {
        case invitations = "invitations"
        case members = "members"
        case organization = "organization"
    }

    public init(invitations: [OrganizationInvitationDTO], members: [OrganizationMemberDTO], organization: OrganizationDTO) {
        self.invitations = invitations
        self.members = members
        self.organization = organization
    }
}

// MARK: - OrganizationInvitationDTO
public struct OrganizationInvitationDTO: Codable, Sendable {
    public let createdAt: String
    public let email: String
    public let expiresAt: String
    public let id: String
    public let inviterId: String
    public let organizationId: String
    public let organizationName: String
    public let role: OrganizationDTORole
    public let status: OrganizationInvitationDTOStatus

    public enum CodingKeys: String, CodingKey {
        case createdAt = "createdAt"
        case email = "email"
        case expiresAt = "expiresAt"
        case id = "id"
        case inviterId = "inviterId"
        case organizationId = "organizationId"
        case organizationName = "organizationName"
        case role = "role"
        case status = "status"
    }

    public init(createdAt: String, email: String, expiresAt: String, id: String, inviterId: String, organizationId: String, organizationName: String, role: OrganizationDTORole, status: OrganizationInvitationDTOStatus) {
        self.createdAt = createdAt
        self.email = email
        self.expiresAt = expiresAt
        self.id = id
        self.inviterId = inviterId
        self.organizationId = organizationId
        self.organizationName = organizationName
        self.role = role
        self.status = status
    }
}

public enum OrganizationInvitationDTOStatus: String, Codable, Sendable {
    case accepted = "accepted"
    case canceled = "canceled"
    case expired = "expired"
    case pending = "pending"
    case rejected = "rejected"
}

// MARK: - OrganizationMemberDTO
public struct OrganizationMemberDTO: Codable, Sendable {
    public let createdAt: String
    public let email: String
    public let id: String
    public let name: String
    public let organizationId: String
    public let role: OrganizationDTORole
    public let userId: String

    public enum CodingKeys: String, CodingKey {
        case createdAt = "createdAt"
        case email = "email"
        case id = "id"
        case name = "name"
        case organizationId = "organizationId"
        case role = "role"
        case userId = "userId"
    }

    public init(createdAt: String, email: String, id: String, name: String, organizationId: String, role: OrganizationDTORole, userId: String) {
        self.createdAt = createdAt
        self.email = email
        self.id = id
        self.name = name
        self.organizationId = organizationId
        self.role = role
        self.userId = userId
    }
}

// MARK: - OrganizationInvitationsDTO
public struct OrganizationInvitationsDTO: Codable, Sendable {
    public let invitations: [OrganizationInvitationDTO]

    public enum CodingKeys: String, CodingKey {
        case invitations = "invitations"
    }

    public init(invitations: [OrganizationInvitationDTO]) {
        self.invitations = invitations
    }
}

// MARK: - CreateOrganizationInput
public struct CreateOrganizationInput: Codable, Sendable {
    public let name: String
    public let slug: String

    public enum CodingKeys: String, CodingKey {
        case name = "name"
        case slug = "slug"
    }

    public init(name: String, slug: String) {
        self.name = name
        self.slug = slug
    }
}

// MARK: - UpdateOrganizationInput
public struct UpdateOrganizationInput: Codable, Sendable {
    public let name: String

    public enum CodingKeys: String, CodingKey {
        case name = "name"
    }

    public init(name: String) {
        self.name = name
    }
}

// MARK: - SetActiveOrganizationInput
public struct SetActiveOrganizationInput: Codable, Sendable {
    public let organizationId: String

    public enum CodingKeys: String, CodingKey {
        case organizationId = "organizationId"
    }

    public init(organizationId: String) {
        self.organizationId = organizationId
    }
}

// MARK: - InviteOrganizationMemberInput
public struct InviteOrganizationMemberInput: Codable, Sendable {
    public let email: String
    public let role: InviteOrganizationMemberInputRole

    public enum CodingKeys: String, CodingKey {
        case email = "email"
        case role = "role"
    }

    public init(email: String, role: InviteOrganizationMemberInputRole) {
        self.email = email
        self.role = role
    }
}

public enum InviteOrganizationMemberInputRole: String, Codable, Sendable {
    case admin = "admin"
    case member = "member"
}

// MARK: - UpdateOrganizationMemberRoleInput
public struct UpdateOrganizationMemberRoleInput: Codable, Sendable {
    public let role: InviteOrganizationMemberInputRole

    public enum CodingKeys: String, CodingKey {
        case role = "role"
    }

    public init(role: InviteOrganizationMemberInputRole) {
        self.role = role
    }
}

// MARK: - ManualParticipantInput
public struct ManualParticipantInput: Codable, Sendable {
    public let displayName: String
    public let email: String?
    public let familyName: String?
    public let givenName: String?
    public let id: String

    public enum CodingKeys: String, CodingKey {
        case displayName = "displayName"
        case email = "email"
        case familyName = "familyName"
        case givenName = "givenName"
        case id = "id"
    }

    public init(displayName: String, email: String? = nil, familyName: String? = nil, givenName: String? = nil, id: String) {
        self.displayName = displayName
        self.email = email
        self.familyName = familyName
        self.givenName = givenName
        self.id = id
    }
}

// MARK: - GoogleStatusDTO
public struct GoogleStatusDTO: Codable, Sendable {
    public let configured: Bool
    public let connections: [GoogleConnectionDTO]
    public let needsHttps: Bool?
    public let redirectUri: String?

    public enum CodingKeys: String, CodingKey {
        case configured = "configured"
        case connections = "connections"
        case needsHttps = "needsHttps"
        case redirectUri = "redirectUri"
    }

    public init(configured: Bool, connections: [GoogleConnectionDTO], needsHttps: Bool? = nil, redirectUri: String?) {
        self.configured = configured
        self.connections = connections
        self.needsHttps = needsHttps
        self.redirectUri = redirectUri
    }
}

// MARK: - GoogleConnectionDTO
public struct GoogleConnectionDTO: Codable, Sendable {
    public let contactsEnabled: Bool
    public let createdAt: String
    public let displayName: String?
    public let email: String
    public let id: String
    public let lastSyncedAt: String?
    public let lastSyncError: String?
    public let status: GoogleConnectionDTOStatus

    public enum CodingKeys: String, CodingKey {
        case contactsEnabled = "contactsEnabled"
        case createdAt = "createdAt"
        case displayName = "displayName"
        case email = "email"
        case id = "id"
        case lastSyncedAt = "lastSyncedAt"
        case lastSyncError = "lastSyncError"
        case status = "status"
    }

    public init(contactsEnabled: Bool, createdAt: String, displayName: String?, email: String, id: String, lastSyncedAt: String?, lastSyncError: String?, status: GoogleConnectionDTOStatus) {
        self.contactsEnabled = contactsEnabled
        self.createdAt = createdAt
        self.displayName = displayName
        self.email = email
        self.id = id
        self.lastSyncedAt = lastSyncedAt
        self.lastSyncError = lastSyncError
        self.status = status
    }
}

public enum GoogleConnectionDTOStatus: String, Codable, Sendable {
    case connected = "connected"
    case reconnectRequired = "reconnect_required"
}

// MARK: - GoogleConnectInput
public struct GoogleConnectInput: Codable, Sendable {
    public let contacts: Bool?

    public enum CodingKeys: String, CodingKey {
        case contacts = "contacts"
    }

    public init(contacts: Bool? = nil) {
        self.contacts = contacts
    }
}

// MARK: - GoogleConnectDTO
public struct GoogleConnectDTO: Codable, Sendable {
    public let authorizationUrl: String

    public enum CodingKeys: String, CodingKey {
        case authorizationUrl = "authorizationUrl"
    }

    public init(authorizationUrl: String) {
        self.authorizationUrl = authorizationUrl
    }
}

// MARK: - GoogleCalendarDTO
public struct GoogleCalendarDTO: Codable, Sendable {
    public let accessRole: String
    public let color: String
    public let connectionId: String
    public let id: String
    public let name: String
    public let primary: Bool
    public let selected: Bool
    public let timeZone: String

    public enum CodingKeys: String, CodingKey {
        case accessRole = "accessRole"
        case color = "color"
        case connectionId = "connectionId"
        case id = "id"
        case name = "name"
        case primary = "primary"
        case selected = "selected"
        case timeZone = "timeZone"
    }

    public init(accessRole: String, color: String, connectionId: String, id: String, name: String, primary: Bool, selected: Bool, timeZone: String) {
        self.accessRole = accessRole
        self.color = color
        self.connectionId = connectionId
        self.id = id
        self.name = name
        self.primary = primary
        self.selected = selected
        self.timeZone = timeZone
    }
}

// MARK: - UpdateGoogleCalendarInput
public struct UpdateGoogleCalendarInput: Codable, Sendable {
    public let selected: Bool

    public enum CodingKeys: String, CodingKey {
        case selected = "selected"
    }

    public init(selected: Bool) {
        self.selected = selected
    }
}

// MARK: - NoteContextDTO
public struct NoteContextDTO: Codable, Sendable {
    public let meeting: MeetingContextDTO?
    public let noteId: String

    public enum CodingKeys: String, CodingKey {
        case meeting = "meeting"
        case noteId = "noteId"
    }

    public init(meeting: MeetingContextDTO?, noteId: String) {
        self.meeting = meeting
        self.noteId = noteId
    }
}

// MARK: - MeetingContextDTO
public struct MeetingContextDTO: Codable, Sendable {
    public let allDay: Bool
    public let calendarName: String
    public let endAt: String
    public let eventId: String
    public let eventUrl: String?
    public let location: String?
    public let meetingUrl: String?
    public let organizer: MeetingParticipantDTO?
    public let participants: [MeetingParticipantDTO]
    public let provider: MeetingContextDTOProvider
    public let startAt: String
    public let timeZone: String?
    public let title: String

    public enum CodingKeys: String, CodingKey {
        case allDay = "allDay"
        case calendarName = "calendarName"
        case endAt = "endAt"
        case eventId = "eventId"
        case eventUrl = "eventUrl"
        case location = "location"
        case meetingUrl = "meetingUrl"
        case organizer = "organizer"
        case participants = "participants"
        case provider = "provider"
        case startAt = "startAt"
        case timeZone = "timeZone"
        case title = "title"
    }

    public init(allDay: Bool, calendarName: String, endAt: String, eventId: String, eventUrl: String?, location: String?, meetingUrl: String?, organizer: MeetingParticipantDTO?, participants: [MeetingParticipantDTO], provider: MeetingContextDTOProvider, startAt: String, timeZone: String?, title: String) {
        self.allDay = allDay
        self.calendarName = calendarName
        self.endAt = endAt
        self.eventId = eventId
        self.eventUrl = eventUrl
        self.location = location
        self.meetingUrl = meetingUrl
        self.organizer = organizer
        self.participants = participants
        self.provider = provider
        self.startAt = startAt
        self.timeZone = timeZone
        self.title = title
    }
}

// MARK: - MeetingParticipantDTO
public struct MeetingParticipantDTO: Codable, Sendable {
    public let displayName: String?
    public let email: String?
    public let familyName: String?
    public let givenName: String?
    public let id: String
    public let isOrganizer: Bool
    public let isResource: Bool
    public let isSelf: Bool
    public let nameSource: NameSource
    public let responseStatus: ResponseStatus

    public enum CodingKeys: String, CodingKey {
        case displayName = "displayName"
        case email = "email"
        case familyName = "familyName"
        case givenName = "givenName"
        case id = "id"
        case isOrganizer = "isOrganizer"
        case isResource = "isResource"
        case isSelf = "isSelf"
        case nameSource = "nameSource"
        case responseStatus = "responseStatus"
    }

    public init(displayName: String?, email: String?, familyName: String?, givenName: String?, id: String, isOrganizer: Bool, isResource: Bool, isSelf: Bool, nameSource: NameSource, responseStatus: ResponseStatus) {
        self.displayName = displayName
        self.email = email
        self.familyName = familyName
        self.givenName = givenName
        self.id = id
        self.isOrganizer = isOrganizer
        self.isResource = isResource
        self.isSelf = isSelf
        self.nameSource = nameSource
        self.responseStatus = responseStatus
    }
}

public enum NameSource: String, Codable, Sendable {
    case calendar = "calendar"
    case contacts = "contacts"
    case manual = "manual"
    case unknown = "unknown"
}

public enum ResponseStatus: String, Codable, Sendable {
    case accepted = "accepted"
    case declined = "declined"
    case needsAction = "needsAction"
    case tentative = "tentative"
    case unknown = "unknown"
}

public enum MeetingContextDTOProvider: String, Codable, Sendable {
    case google = "google"
    case ics = "ics"
}

// MARK: - UpdateNoteContextInput
public struct UpdateNoteContextInput: Codable, Sendable {
    public let eventId: String?

    public enum CodingKeys: String, CodingKey {
        case eventId = "eventId"
    }

    public init(eventId: String?) {
        self.eventId = eventId
    }
}

// MARK: - AuthSessionDTO
public struct AuthSessionDTO: Codable, Sendable {
    public let expiresAt: String?
    public let user: UserDTO

    public enum CodingKeys: String, CodingKey {
        case expiresAt = "expiresAt"
        case user = "user"
    }

    public init(expiresAt: String?, user: UserDTO) {
        self.expiresAt = expiresAt
        self.user = user
    }
}

// MARK: - UserDTO
public struct UserDTO: Codable, Sendable {
    public let createdAt: String
    public let email: String
    public let id: String
    public let name: String
    public let role: UserDTORole

    public enum CodingKeys: String, CodingKey {
        case createdAt = "createdAt"
        case email = "email"
        case id = "id"
        case name = "name"
        case role = "role"
    }

    public init(createdAt: String, email: String, id: String, name: String, role: UserDTORole) {
        self.createdAt = createdAt
        self.email = email
        self.id = id
        self.name = name
        self.role = role
    }
}

public enum UserDTORole: String, Codable, Sendable {
    case admin = "admin"
    case owner = "owner"
    case user = "user"
}

// MARK: - UsersDTO
public struct UsersDTO: Codable, Sendable {
    public let users: [UserDTO]

    public enum CodingKeys: String, CodingKey {
        case users = "users"
    }

    public init(users: [UserDTO]) {
        self.users = users
    }
}

// MARK: - CreateUserInput
public struct CreateUserInput: Codable, Sendable {
    public let email: String
    public let name: String
    public let password: String

    public enum CodingKeys: String, CodingKey {
        case email = "email"
        case name = "name"
        case password = "password"
    }

    public init(email: String, name: String, password: String) {
        self.email = email
        self.name = name
        self.password = password
    }
}

// MARK: - UpdateUserRoleInput
public struct UpdateUserRoleInput: Codable, Sendable {
    public let role: UpdateUserRoleInputRole

    public enum CodingKeys: String, CodingKey {
        case role = "role"
    }

    public init(role: UpdateUserRoleInputRole) {
        self.role = role
    }
}

public enum UpdateUserRoleInputRole: String, Codable, Sendable {
    case admin = "admin"
    case user = "user"
}

// MARK: - CalendarPreferencesDTO
public struct CalendarPreferencesDTO: Codable, Sendable {
    public let lookAheadDays: Int

    public enum CodingKeys: String, CodingKey {
        case lookAheadDays = "lookAheadDays"
    }

    public init(lookAheadDays: Int) {
        self.lookAheadDays = lookAheadDays
    }
}

// MARK: - AuthMethodsDTO
public struct AuthMethodsDTO: Codable, Sendable {
    public let google: Bool
    public let instanceName: String
    public let password: Bool
    public let setupRequired: Bool?
    public let transport: ServerTransport?

    public enum CodingKeys: String, CodingKey {
        case google = "google"
        case instanceName = "instanceName"
        case password = "password"
        case setupRequired = "setupRequired"
        case transport = "transport"
    }

    public init(google: Bool, instanceName: String, password: Bool, setupRequired: Bool? = nil, transport: ServerTransport? = nil) {
        self.google = google
        self.instanceName = instanceName
        self.password = password
        self.setupRequired = setupRequired
        self.transport = transport
    }
}

public enum ServerTransport: String, Codable, Sendable {
    case publichttp = "public-http"
    case secure = "secure"
    case serverTransportPrivate = "private"
}

// MARK: - AppHandoffInput
public struct AppHandoffInput: Codable, Sendable {
    public let challenge: String

    public enum CodingKeys: String, CodingKey {
        case challenge = "challenge"
    }

    public init(challenge: String) {
        self.challenge = challenge
    }
}

// MARK: - AppHandoffDTO
public struct AppHandoffDTO: Codable, Sendable {
    public let code: String

    public enum CodingKeys: String, CodingKey {
        case code = "code"
    }

    public init(code: String) {
        self.code = code
    }
}

// MARK: - AppHandoffExchangeInput
public struct AppHandoffExchangeInput: Codable, Sendable {
    public let code: String
    public let verifier: String

    public enum CodingKeys: String, CodingKey {
        case code = "code"
        case verifier = "verifier"
    }

    public init(code: String, verifier: String) {
        self.code = code
        self.verifier = verifier
    }
}

// MARK: - AppHandoffTokenDTO
public struct AppHandoffTokenDTO: Codable, Sendable {
    public let token: String

    public enum CodingKeys: String, CodingKey {
        case token = "token"
    }

    public init(token: String) {
        self.token = token
    }
}

// MARK: - AdminSettingsDTO
public struct AdminSettingsDTO: Codable, Sendable {
    public let apps: AppsSettingsDTO?
    public let calendarLookAheadDays: Int
    public let calendarRefreshMinutes: Int
    public let chatModels: [LanguageModelSelection]
    public let elevenLabsConfigured: Bool
    public let elevenLabsSource: ElevenLabsSourceEnum
    public let generationModel: LanguageModelSelection
    public let instanceLogo: String?
    public let instanceName: String
    public let languageModel: String
    public let liveHoldSeconds: Int?
    public let liveMaxSessions: Int?
    public let liveTranscriptionModel: LiveTranscriptionModelSelection?
    public let onboarding: OnboardingDTO?
    public let openRouterConfigured: Bool
    public let openRouterSource: ElevenLabsSourceEnum
    public let providers: [ProviderStatusDTO]
    public let signIn: SignInSettingsDTO?
    public let storage: StorageSettingsDTO?
    public let transcriptionModel: TranscriptionModelSelection
    public let unusedEnvironment: [String]?
    public let updatedAt: String?

    public enum CodingKeys: String, CodingKey {
        case apps = "apps"
        case calendarLookAheadDays = "calendarLookAheadDays"
        case calendarRefreshMinutes = "calendarRefreshMinutes"
        case chatModels = "chatModels"
        case elevenLabsConfigured = "elevenLabsConfigured"
        case elevenLabsSource = "elevenLabsSource"
        case generationModel = "generationModel"
        case instanceLogo = "instanceLogo"
        case instanceName = "instanceName"
        case languageModel = "languageModel"
        case liveHoldSeconds = "liveHoldSeconds"
        case liveMaxSessions = "liveMaxSessions"
        case liveTranscriptionModel = "liveTranscriptionModel"
        case onboarding = "onboarding"
        case openRouterConfigured = "openRouterConfigured"
        case openRouterSource = "openRouterSource"
        case providers = "providers"
        case signIn = "signIn"
        case storage = "storage"
        case transcriptionModel = "transcriptionModel"
        case unusedEnvironment = "unusedEnvironment"
        case updatedAt = "updatedAt"
    }

    public init(apps: AppsSettingsDTO? = nil, calendarLookAheadDays: Int, calendarRefreshMinutes: Int, chatModels: [LanguageModelSelection], elevenLabsConfigured: Bool, elevenLabsSource: ElevenLabsSourceEnum, generationModel: LanguageModelSelection, instanceLogo: String?, instanceName: String, languageModel: String, liveHoldSeconds: Int? = nil, liveMaxSessions: Int? = nil, liveTranscriptionModel: LiveTranscriptionModelSelection? = nil, onboarding: OnboardingDTO? = nil, openRouterConfigured: Bool, openRouterSource: ElevenLabsSourceEnum, providers: [ProviderStatusDTO], signIn: SignInSettingsDTO? = nil, storage: StorageSettingsDTO? = nil, transcriptionModel: TranscriptionModelSelection, unusedEnvironment: [String]? = nil, updatedAt: String?) {
        self.apps = apps
        self.calendarLookAheadDays = calendarLookAheadDays
        self.calendarRefreshMinutes = calendarRefreshMinutes
        self.chatModels = chatModels
        self.elevenLabsConfigured = elevenLabsConfigured
        self.elevenLabsSource = elevenLabsSource
        self.generationModel = generationModel
        self.instanceLogo = instanceLogo
        self.instanceName = instanceName
        self.languageModel = languageModel
        self.liveHoldSeconds = liveHoldSeconds
        self.liveMaxSessions = liveMaxSessions
        self.liveTranscriptionModel = liveTranscriptionModel
        self.onboarding = onboarding
        self.openRouterConfigured = openRouterConfigured
        self.openRouterSource = openRouterSource
        self.providers = providers
        self.signIn = signIn
        self.storage = storage
        self.transcriptionModel = transcriptionModel
        self.unusedEnvironment = unusedEnvironment
        self.updatedAt = updatedAt
    }
}

// MARK: - AppsSettingsDTO
public struct AppsSettingsDTO: Codable, Sendable {
    public let iosAppIds: [String]
    public let iosAppStoreId: String?

    public enum CodingKeys: String, CodingKey {
        case iosAppIds = "iosAppIds"
        case iosAppStoreId = "iosAppStoreId"
    }

    public init(iosAppIds: [String], iosAppStoreId: String?) {
        self.iosAppIds = iosAppIds
        self.iosAppStoreId = iosAppStoreId
    }
}

public enum ElevenLabsSourceEnum: String, Codable, Sendable {
    case environment = "environment"
    case none = "none"
    case stored = "stored"
}

// MARK: - LiveTranscriptionModelSelection
public struct LiveTranscriptionModelSelection: Codable, Sendable {
    public let model: String
    public let provider: LiveTranscriptionModelSelectionProvider

    public enum CodingKeys: String, CodingKey {
        case model = "model"
        case provider = "provider"
    }

    public init(model: String, provider: LiveTranscriptionModelSelectionProvider) {
        self.model = model
        self.provider = provider
    }
}

public enum LiveTranscriptionModelSelectionProvider: String, Codable, Sendable {
    case elevenlabs = "elevenlabs"
}

// MARK: - OnboardingDTO
public struct OnboardingDTO: Codable, Sendable {
    public let completedAt: String?
    public let steps: OnboardingStepsDTO

    public enum CodingKeys: String, CodingKey {
        case completedAt = "completedAt"
        case steps = "steps"
    }

    public init(completedAt: String?, steps: OnboardingStepsDTO) {
        self.completedAt = completedAt
        self.steps = steps
    }
}

// MARK: - OnboardingStepsDTO
public struct OnboardingStepsDTO: Codable, Sendable {
    public let language: Bool
    public let models: Bool
    public let name: Bool
    public let owner: Bool
    public let signIn: Bool
    public let speech: Bool
    public let storage: Bool

    public enum CodingKeys: String, CodingKey {
        case language = "language"
        case models = "models"
        case name = "name"
        case owner = "owner"
        case signIn = "signIn"
        case speech = "speech"
        case storage = "storage"
    }

    public init(language: Bool, models: Bool, name: Bool, owner: Bool, signIn: Bool, speech: Bool, storage: Bool) {
        self.language = language
        self.models = models
        self.name = name
        self.owner = owner
        self.signIn = signIn
        self.speech = speech
        self.storage = storage
    }
}

// MARK: - ProviderStatusDTO
public struct ProviderStatusDTO: Codable, Sendable {
    public let configured: Bool
    public let id: ID
    public let source: ElevenLabsSourceEnum

    public enum CodingKeys: String, CodingKey {
        case configured = "configured"
        case id = "id"
        case source = "source"
    }

    public init(configured: Bool, id: ID, source: ElevenLabsSourceEnum) {
        self.configured = configured
        self.id = id
        self.source = source
    }
}

// MARK: - SignInSettingsDTO
public struct SignInSettingsDTO: Codable, Sendable {
    public let allowedEmails: [String]
    public let google: Bool
    public let googleClientId: String?
    public let googleClientSecretSet: Bool
    public let googleNeedsHttps: Bool?
    public let googleRedirectUris: [String]
    public let password: Bool

    public enum CodingKeys: String, CodingKey {
        case allowedEmails = "allowedEmails"
        case google = "google"
        case googleClientId = "googleClientId"
        case googleClientSecretSet = "googleClientSecretSet"
        case googleNeedsHttps = "googleNeedsHttps"
        case googleRedirectUris = "googleRedirectUris"
        case password = "password"
    }

    public init(allowedEmails: [String], google: Bool, googleClientId: String?, googleClientSecretSet: Bool, googleNeedsHttps: Bool? = nil, googleRedirectUris: [String], password: Bool) {
        self.allowedEmails = allowedEmails
        self.google = google
        self.googleClientId = googleClientId
        self.googleClientSecretSet = googleClientSecretSet
        self.googleNeedsHttps = googleNeedsHttps
        self.googleRedirectUris = googleRedirectUris
        self.password = password
    }
}

// MARK: - StorageSettingsDTO
public struct StorageSettingsDTO: Codable, Sendable {
    public let accessKeyId: String?
    public let backend: StorageBackend
    public let bucket: String?
    public let endpoint: String?
    public let forcePathStyle: Bool
    public let storageSettingsDTOPrefix: String
    public let region: String?
    public let secretAccessKeySet: Bool
    public let source: StorageSettingsDTOSource

    public enum CodingKeys: String, CodingKey {
        case accessKeyId = "accessKeyId"
        case backend = "backend"
        case bucket = "bucket"
        case endpoint = "endpoint"
        case forcePathStyle = "forcePathStyle"
        case storageSettingsDTOPrefix = "prefix"
        case region = "region"
        case secretAccessKeySet = "secretAccessKeySet"
        case source = "source"
    }

    public init(accessKeyId: String?, backend: StorageBackend, bucket: String?, endpoint: String?, forcePathStyle: Bool, storageSettingsDTOPrefix: String, region: String?, secretAccessKeySet: Bool, source: StorageSettingsDTOSource) {
        self.accessKeyId = accessKeyId
        self.backend = backend
        self.bucket = bucket
        self.endpoint = endpoint
        self.forcePathStyle = forcePathStyle
        self.storageSettingsDTOPrefix = storageSettingsDTOPrefix
        self.region = region
        self.secretAccessKeySet = secretAccessKeySet
        self.source = source
    }
}

public enum StorageBackend: String, Codable, Sendable {
    case local = "local"
    case s3 = "s3"
}

public enum StorageSettingsDTOSource: String, Codable, Sendable {
    case environment = "environment"
    case sourceDefault = "default"
    case stored = "stored"
}

// MARK: - TranscriptionModelSelection
public struct TranscriptionModelSelection: Codable, Sendable {
    public let model: String
    public let provider: TranscriptionModelSelectionProvider

    public enum CodingKeys: String, CodingKey {
        case model = "model"
        case provider = "provider"
    }

    public init(model: String, provider: TranscriptionModelSelectionProvider) {
        self.model = model
        self.provider = provider
    }
}

public enum TranscriptionModelSelectionProvider: String, Codable, Sendable {
    case elevenlabs = "elevenlabs"
    case openai = "openai"
}

// MARK: - UpdateAdminSettingsInput
public struct UpdateAdminSettingsInput: Codable, Sendable {
    public let anthropicApiKey: String?
    public let apps: UpdateAppsSettingsInput?
    public let calendarLookAheadDays: Int?
    public let calendarRefreshMinutes: Int?
    public let chatModels: [LanguageModelSelection]?
    public let elevenLabsApiKey: String?
    public let generationModel: LanguageModelSelection?
    public let instanceLogo: String?
    public let instanceName: String?
    public let languageModel: String?
    public let liveHoldSeconds: Int?
    public let liveMaxSessions: Int?
    public let liveTranscriptionModel: LiveTranscriptionModelSelection?
    public let openAiApiKey: String?
    public let openRouterApiKey: String?
    public let signIn: UpdateSignInSettingsInput?
    public let storage: UpdateStorageSettingsInput?
    public let transcriptionModel: TranscriptionModelSelection?

    public enum CodingKeys: String, CodingKey {
        case anthropicApiKey = "anthropicApiKey"
        case apps = "apps"
        case calendarLookAheadDays = "calendarLookAheadDays"
        case calendarRefreshMinutes = "calendarRefreshMinutes"
        case chatModels = "chatModels"
        case elevenLabsApiKey = "elevenLabsApiKey"
        case generationModel = "generationModel"
        case instanceLogo = "instanceLogo"
        case instanceName = "instanceName"
        case languageModel = "languageModel"
        case liveHoldSeconds = "liveHoldSeconds"
        case liveMaxSessions = "liveMaxSessions"
        case liveTranscriptionModel = "liveTranscriptionModel"
        case openAiApiKey = "openAiApiKey"
        case openRouterApiKey = "openRouterApiKey"
        case signIn = "signIn"
        case storage = "storage"
        case transcriptionModel = "transcriptionModel"
    }

    public init(anthropicApiKey: String? = nil, apps: UpdateAppsSettingsInput? = nil, calendarLookAheadDays: Int? = nil, calendarRefreshMinutes: Int? = nil, chatModels: [LanguageModelSelection]? = nil, elevenLabsApiKey: String? = nil, generationModel: LanguageModelSelection? = nil, instanceLogo: String? = nil, instanceName: String? = nil, languageModel: String? = nil, liveHoldSeconds: Int? = nil, liveMaxSessions: Int? = nil, liveTranscriptionModel: LiveTranscriptionModelSelection? = nil, openAiApiKey: String? = nil, openRouterApiKey: String? = nil, signIn: UpdateSignInSettingsInput? = nil, storage: UpdateStorageSettingsInput? = nil, transcriptionModel: TranscriptionModelSelection? = nil) {
        self.anthropicApiKey = anthropicApiKey
        self.apps = apps
        self.calendarLookAheadDays = calendarLookAheadDays
        self.calendarRefreshMinutes = calendarRefreshMinutes
        self.chatModels = chatModels
        self.elevenLabsApiKey = elevenLabsApiKey
        self.generationModel = generationModel
        self.instanceLogo = instanceLogo
        self.instanceName = instanceName
        self.languageModel = languageModel
        self.liveHoldSeconds = liveHoldSeconds
        self.liveMaxSessions = liveMaxSessions
        self.liveTranscriptionModel = liveTranscriptionModel
        self.openAiApiKey = openAiApiKey
        self.openRouterApiKey = openRouterApiKey
        self.signIn = signIn
        self.storage = storage
        self.transcriptionModel = transcriptionModel
    }
}

// MARK: - UpdateAppsSettingsInput
public struct UpdateAppsSettingsInput: Codable, Sendable {
    public let iosAppIds: [String]?
    public let iosAppStoreId: String?

    public enum CodingKeys: String, CodingKey {
        case iosAppIds = "iosAppIds"
        case iosAppStoreId = "iosAppStoreId"
    }

    public init(iosAppIds: [String]? = nil, iosAppStoreId: String? = nil) {
        self.iosAppIds = iosAppIds
        self.iosAppStoreId = iosAppStoreId
    }
}

// MARK: - UpdateSignInSettingsInput
public struct UpdateSignInSettingsInput: Codable, Sendable {
    public let allowedEmails: [String]?
    public let google: Bool?
    public let googleClientId: String?
    public let googleClientSecret: String?
    public let password: Bool?

    public enum CodingKeys: String, CodingKey {
        case allowedEmails = "allowedEmails"
        case google = "google"
        case googleClientId = "googleClientId"
        case googleClientSecret = "googleClientSecret"
        case password = "password"
    }

    public init(allowedEmails: [String]? = nil, google: Bool? = nil, googleClientId: String? = nil, googleClientSecret: String? = nil, password: Bool? = nil) {
        self.allowedEmails = allowedEmails
        self.google = google
        self.googleClientId = googleClientId
        self.googleClientSecret = googleClientSecret
        self.password = password
    }
}

// MARK: - UpdateStorageSettingsInput
public struct UpdateStorageSettingsInput: Codable, Sendable {
    public let accessKeyId: String?
    public let backend: StorageBackend?
    public let bucket: String?
    public let endpoint: String?
    public let forcePathStyle: Bool?
    public let updateStorageSettingsInputPrefix: String?
    public let region: String?
    public let secretAccessKey: String?

    public enum CodingKeys: String, CodingKey {
        case accessKeyId = "accessKeyId"
        case backend = "backend"
        case bucket = "bucket"
        case endpoint = "endpoint"
        case forcePathStyle = "forcePathStyle"
        case updateStorageSettingsInputPrefix = "prefix"
        case region = "region"
        case secretAccessKey = "secretAccessKey"
    }

    public init(accessKeyId: String? = nil, backend: StorageBackend? = nil, bucket: String? = nil, endpoint: String? = nil, forcePathStyle: Bool? = nil, updateStorageSettingsInputPrefix: String? = nil, region: String? = nil, secretAccessKey: String? = nil) {
        self.accessKeyId = accessKeyId
        self.backend = backend
        self.bucket = bucket
        self.endpoint = endpoint
        self.forcePathStyle = forcePathStyle
        self.updateStorageSettingsInputPrefix = updateStorageSettingsInputPrefix
        self.region = region
        self.secretAccessKey = secretAccessKey
    }
}

// MARK: - CalendarDTO
public struct CalendarDTO: Codable, Sendable {
    public let color: String
    public let createdAt: String
    public let enabled: Bool
    public let eventCount: Int
    public let id: String
    public let lastSyncedAt: String?
    public let lastSyncError: String?
    public let name: String
    public let provider: CalendarDTOProvider
    public let sourceHost: String

    public enum CodingKeys: String, CodingKey {
        case color = "color"
        case createdAt = "createdAt"
        case enabled = "enabled"
        case eventCount = "eventCount"
        case id = "id"
        case lastSyncedAt = "lastSyncedAt"
        case lastSyncError = "lastSyncError"
        case name = "name"
        case provider = "provider"
        case sourceHost = "sourceHost"
    }

    public init(color: String, createdAt: String, enabled: Bool, eventCount: Int, id: String, lastSyncedAt: String?, lastSyncError: String?, name: String, provider: CalendarDTOProvider, sourceHost: String) {
        self.color = color
        self.createdAt = createdAt
        self.enabled = enabled
        self.eventCount = eventCount
        self.id = id
        self.lastSyncedAt = lastSyncedAt
        self.lastSyncError = lastSyncError
        self.name = name
        self.provider = provider
        self.sourceHost = sourceHost
    }
}

public enum CalendarDTOProvider: String, Codable, Sendable {
    case ics = "ics"
}

// MARK: - CreateCalendarInput
public struct CreateCalendarInput: Codable, Sendable {
    public let color: String?
    public let enabled: Bool?
    public let name: String
    public let url: String

    public enum CodingKeys: String, CodingKey {
        case color = "color"
        case enabled = "enabled"
        case name = "name"
        case url = "url"
    }

    public init(color: String? = nil, enabled: Bool? = nil, name: String, url: String) {
        self.color = color
        self.enabled = enabled
        self.name = name
        self.url = url
    }
}

// MARK: - UpdateCalendarInput
public struct UpdateCalendarInput: Codable, Sendable {
    public let color: String?
    public let enabled: Bool?
    public let name: String?
    public let url: String?

    public enum CodingKeys: String, CodingKey {
        case color = "color"
        case enabled = "enabled"
        case name = "name"
        case url = "url"
    }

    public init(color: String? = nil, enabled: Bool? = nil, name: String? = nil, url: String? = nil) {
        self.color = color
        self.enabled = enabled
        self.name = name
        self.url = url
    }
}

// MARK: - CalendarEventDTO
public struct CalendarEventDTO: Codable, Sendable {
    public let allDay: Bool
    public let attendeeCount: Int?
    public let calendarId: String
    public let calendarName: String
    public let color: String
    public let endAt: String
    public let endDate: String
    public let id: String
    public let location: String?
    public let meetingUid: String?
    public let meetingUrl: String?
    public let noteId: String?
    public let occurrence: String?
    public let startAt: String
    public let startDate: String
    public let timeZone: String?
    public let title: String

    public enum CodingKeys: String, CodingKey {
        case allDay = "allDay"
        case attendeeCount = "attendeeCount"
        case calendarId = "calendarId"
        case calendarName = "calendarName"
        case color = "color"
        case endAt = "endAt"
        case endDate = "endDate"
        case id = "id"
        case location = "location"
        case meetingUid = "meetingUid"
        case meetingUrl = "meetingUrl"
        case noteId = "noteId"
        case occurrence = "occurrence"
        case startAt = "startAt"
        case startDate = "startDate"
        case timeZone = "timeZone"
        case title = "title"
    }

    public init(allDay: Bool, attendeeCount: Int? = nil, calendarId: String, calendarName: String, color: String, endAt: String, endDate: String, id: String, location: String?, meetingUid: String? = nil, meetingUrl: String?, noteId: String?, occurrence: String? = nil, startAt: String, startDate: String, timeZone: String?, title: String) {
        self.allDay = allDay
        self.attendeeCount = attendeeCount
        self.calendarId = calendarId
        self.calendarName = calendarName
        self.color = color
        self.endAt = endAt
        self.endDate = endDate
        self.id = id
        self.location = location
        self.meetingUid = meetingUid
        self.meetingUrl = meetingUrl
        self.noteId = noteId
        self.occurrence = occurrence
        self.startAt = startAt
        self.startDate = startDate
        self.timeZone = timeZone
        self.title = title
    }
}

// MARK: - UploadAudioChunkInput
public struct UploadAudioChunkInput: Codable, Sendable {
    public let audioBase64: String
    public let channel: AudioChannel
    public let sequence: Int
    public let startMs: Int

    public enum CodingKeys: String, CodingKey {
        case audioBase64 = "audioBase64"
        case channel = "channel"
        case sequence = "sequence"
        case startMs = "startMs"
    }

    public init(audioBase64: String, channel: AudioChannel, sequence: Int, startMs: Int) {
        self.audioBase64 = audioBase64
        self.channel = channel
        self.sequence = sequence
        self.startMs = startMs
    }
}

public enum AudioChannel: String, Codable, Sendable {
    case mic = "mic"
    case system = "system"
}

// MARK: - AudioChunkDTO
public struct AudioChunkDTO: Codable, Sendable {
    public let channel: AudioChannel
    public let durationMs: Int
    public let error: String?
    public let id: String
    public let noteId: String
    public let sequence: Int
    public let startMs: Int
    public let state: AudioChunkState

    public enum CodingKeys: String, CodingKey {
        case channel = "channel"
        case durationMs = "durationMs"
        case error = "error"
        case id = "id"
        case noteId = "noteId"
        case sequence = "sequence"
        case startMs = "startMs"
        case state = "state"
    }

    public init(channel: AudioChannel, durationMs: Int, error: String?, id: String, noteId: String, sequence: Int, startMs: Int, state: AudioChunkState) {
        self.channel = channel
        self.durationMs = durationMs
        self.error = error
        self.id = id
        self.noteId = noteId
        self.sequence = sequence
        self.startMs = startMs
        self.state = state
    }
}

public enum AudioChunkState: String, Codable, Sendable {
    case failed = "failed"
    case processing = "processing"
    case queued = "queued"
    case ready = "ready"
}

// MARK: - ChatInput
public struct ChatInput: Codable, Sendable {
    public let history: [ChatMessageDTO]
    public let includeSources: Bool?
    public let message: String
    public let model: String?
    public let timeZone: String?

    public enum CodingKeys: String, CodingKey {
        case history = "history"
        case includeSources = "includeSources"
        case message = "message"
        case model = "model"
        case timeZone = "timeZone"
    }

    public init(history: [ChatMessageDTO], includeSources: Bool? = nil, message: String, model: String? = nil, timeZone: String? = nil) {
        self.history = history
        self.includeSources = includeSources
        self.message = message
        self.model = model
        self.timeZone = timeZone
    }
}

// MARK: - ChatMessageDTO
public struct ChatMessageDTO: Codable, Sendable {
    public let content: String
    public let role: ChatMessageDTORole

    public enum CodingKeys: String, CodingKey {
        case content = "content"
        case role = "role"
    }

    public init(content: String, role: ChatMessageDTORole) {
        self.content = content
        self.role = role
    }
}

public enum ChatMessageDTORole: String, Codable, Sendable {
    case assistant = "assistant"
    case user = "user"
}

// MARK: - ChatAnswerDTO
public struct ChatAnswerDTO: Codable, Sendable {
    public let answer: String

    public enum CodingKeys: String, CodingKey {
        case answer = "answer"
    }

    public init(answer: String) {
        self.answer = answer
    }
}

// MARK: - ChatStreamEventDTO
public struct ChatStreamEventDTO: Codable, Sendable {
    public let citations: [ChatCitationDTO]?
    public let text: String
    public let tool: ChatToolProgressDTO?
    public let type: ChatStreamEventDTOType

    public enum CodingKeys: String, CodingKey {
        case citations = "citations"
        case text = "text"
        case tool = "tool"
        case type = "type"
    }

    public init(citations: [ChatCitationDTO]? = nil, text: String, tool: ChatToolProgressDTO? = nil, type: ChatStreamEventDTOType) {
        self.citations = citations
        self.text = text
        self.tool = tool
        self.type = type
    }
}

// MARK: - ChatCitationDTO
public struct ChatCitationDTO: Codable, Sendable {
    public let id: String
    public let noteId: String
    public let quote: String
    public let segmentId: String?
    public let source: ChatCitationDTOSource
    public let startMs: Int?
    public let title: String
    public let transcriptRevision: Int

    public enum CodingKeys: String, CodingKey {
        case id = "id"
        case noteId = "noteId"
        case quote = "quote"
        case segmentId = "segmentId"
        case source = "source"
        case startMs = "startMs"
        case title = "title"
        case transcriptRevision = "transcriptRevision"
    }

    public init(id: String, noteId: String, quote: String, segmentId: String?, source: ChatCitationDTOSource, startMs: Int?, title: String, transcriptRevision: Int) {
        self.id = id
        self.noteId = noteId
        self.quote = quote
        self.segmentId = segmentId
        self.source = source
        self.startMs = startMs
        self.title = title
        self.transcriptRevision = transcriptRevision
    }
}

public enum ChatCitationDTOSource: String, Codable, Sendable {
    case notes = "notes"
    case summary = "summary"
    case transcript = "transcript"
}

// MARK: - ChatToolProgressDTO
public struct ChatToolProgressDTO: Codable, Sendable {
    public let callId: String
    public let label: String
    public let name: Name
    public let resultCount: Int?
    public let results: [ChatToolMeetingDTO]?
    public let status: ChatToolProgressDTOStatus

    public enum CodingKeys: String, CodingKey {
        case callId = "callId"
        case label = "label"
        case name = "name"
        case resultCount = "resultCount"
        case results = "results"
        case status = "status"
    }

    public init(callId: String, label: String, name: Name, resultCount: Int? = nil, results: [ChatToolMeetingDTO]? = nil, status: ChatToolProgressDTOStatus) {
        self.callId = callId
        self.label = label
        self.name = name
        self.resultCount = resultCount
        self.results = results
        self.status = status
    }
}

public enum Name: String, Codable, Sendable {
    case getMeetingTranscript = "get_meeting_transcript"
    case getMeetings = "get_meetings"
    case listMeetings = "list_meetings"
    case searchMeetings = "search_meetings"
    case searchTranscripts = "search_transcripts"
}

// MARK: - ChatToolMeetingDTO
public struct ChatToolMeetingDTO: Codable, Sendable {
    public let date: String
    public let noteId: String
    public let title: String

    public enum CodingKeys: String, CodingKey {
        case date = "date"
        case noteId = "noteId"
        case title = "title"
    }

    public init(date: String, noteId: String, title: String) {
        self.date = date
        self.noteId = noteId
        self.title = title
    }
}

public enum ChatToolProgressDTOStatus: String, Codable, Sendable {
    case completed = "completed"
    case failed = "failed"
    case running = "running"
}

public enum ChatStreamEventDTOType: String, Codable, Sendable {
    case citations = "citations"
    case delta = "delta"
    case done = "done"
    case error = "error"
    case tool = "tool"
}

// MARK: - EvidenceRequestDTO
public struct EvidenceRequestDTO: Codable, Sendable {
    public let expectedTranscriptRevision: Int
    public let paragraph: String

    public enum CodingKeys: String, CodingKey {
        case expectedTranscriptRevision = "expectedTranscriptRevision"
        case paragraph = "paragraph"
    }

    public init(expectedTranscriptRevision: Int, paragraph: String) {
        self.expectedTranscriptRevision = expectedTranscriptRevision
        self.paragraph = paragraph
    }
}

// MARK: - EvidenceDTO
public struct EvidenceDTO: Codable, Sendable {
    public let quotes: [EvidenceQuoteDTO]
    public let summary: String
    public let transcriptRevision: Int

    public enum CodingKeys: String, CodingKey {
        case quotes = "quotes"
        case summary = "summary"
        case transcriptRevision = "transcriptRevision"
    }

    public init(quotes: [EvidenceQuoteDTO], summary: String, transcriptRevision: Int) {
        self.quotes = quotes
        self.summary = summary
        self.transcriptRevision = transcriptRevision
    }
}

// MARK: - EvidenceQuoteDTO
public struct EvidenceQuoteDTO: Codable, Sendable {
    public let segmentId: String
    public let speakerLabel: String?
    public let startMs: Int
    public let text: String

    public enum CodingKeys: String, CodingKey {
        case segmentId = "segmentId"
        case speakerLabel = "speakerLabel"
        case startMs = "startMs"
        case text = "text"
    }

    public init(segmentId: String, speakerLabel: String?, startMs: Int, text: String) {
        self.segmentId = segmentId
        self.speakerLabel = speakerLabel
        self.startMs = startMs
        self.text = text
    }
}

// MARK: - CreateNoteInput
public struct CreateNoteInput: Codable, Sendable {
    public let body: String?
    public let date: String?
    public let duration: String?
    public let enhanced: String?
    public let title: String
    public let transcriptSegments: [TranscriptSegmentDTO]?

    public enum CodingKeys: String, CodingKey {
        case body = "body"
        case date = "date"
        case duration = "duration"
        case enhanced = "enhanced"
        case title = "title"
        case transcriptSegments = "transcriptSegments"
    }

    public init(body: String? = nil, date: String? = nil, duration: String? = nil, enhanced: String? = nil, title: String, transcriptSegments: [TranscriptSegmentDTO]? = nil) {
        self.body = body
        self.date = date
        self.duration = duration
        self.enhanced = enhanced
        self.title = title
        self.transcriptSegments = transcriptSegments
    }
}

// MARK: - TranscriptSegmentDTO
public struct TranscriptSegmentDTO: Codable, Sendable {
    public let channel: AudioChannel
    public let endMs: Int?
    public let id: String
    public let isFinal: Bool
    public let speakerId: String
    public let speakerLabel: String?
    public let startMs: Int
    public let text: String

    public enum CodingKeys: String, CodingKey {
        case channel = "channel"
        case endMs = "endMs"
        case id = "id"
        case isFinal = "isFinal"
        case speakerId = "speakerId"
        case speakerLabel = "speakerLabel"
        case startMs = "startMs"
        case text = "text"
    }

    public init(channel: AudioChannel, endMs: Int?, id: String, isFinal: Bool, speakerId: String, speakerLabel: String?, startMs: Int, text: String) {
        self.channel = channel
        self.endMs = endMs
        self.id = id
        self.isFinal = isFinal
        self.speakerId = speakerId
        self.speakerLabel = speakerLabel
        self.startMs = startMs
        self.text = text
    }
}

// MARK: - UpdateNoteInput
public struct UpdateNoteInput: Codable, Sendable {
    public let body: String?
    public let date: String?
    public let duration: String?
    public let enhanced: String?
    public let title: String?
    public let transcriptSegments: [TranscriptSegmentDTO]?

    public enum CodingKeys: String, CodingKey {
        case body = "body"
        case date = "date"
        case duration = "duration"
        case enhanced = "enhanced"
        case title = "title"
        case transcriptSegments = "transcriptSegments"
    }

    public init(body: String? = nil, date: String? = nil, duration: String? = nil, enhanced: String? = nil, title: String? = nil, transcriptSegments: [TranscriptSegmentDTO]? = nil) {
        self.body = body
        self.date = date
        self.duration = duration
        self.enhanced = enhanced
        self.title = title
        self.transcriptSegments = transcriptSegments
    }
}

// MARK: - ImportNoteInput
public struct ImportNoteInput: Codable, Sendable {
    public let body: String
    public let createdAt: String
    public let date: String
    public let duration: String
    public let enhanced: String
    public let generated: Bool
    public let id: String
    public let meeting: MeetingContextDTO?
    public let recordingState: ImportedRecordingState
    public let title: String
    public let transcriptSegments: [TranscriptSegmentDTO]

    public enum CodingKeys: String, CodingKey {
        case body = "body"
        case createdAt = "createdAt"
        case date = "date"
        case duration = "duration"
        case enhanced = "enhanced"
        case generated = "generated"
        case id = "id"
        case meeting = "meeting"
        case recordingState = "recordingState"
        case title = "title"
        case transcriptSegments = "transcriptSegments"
    }

    public init(body: String, createdAt: String, date: String, duration: String, enhanced: String, generated: Bool, id: String, meeting: MeetingContextDTO?, recordingState: ImportedRecordingState, title: String, transcriptSegments: [TranscriptSegmentDTO]) {
        self.body = body
        self.createdAt = createdAt
        self.date = date
        self.duration = duration
        self.enhanced = enhanced
        self.generated = generated
        self.id = id
        self.meeting = meeting
        self.recordingState = recordingState
        self.title = title
        self.transcriptSegments = transcriptSegments
    }
}

public enum ImportedRecordingState: String, Codable, Sendable {
    case idle = "idle"
    case stopped = "stopped"
}

// MARK: - ImportedNoteDTO
public struct ImportedNoteDTO: Codable, Sendable {
    public let note: NoteDTO
    public let segmentIds: [ImportedSegmentIdDTO]

    public enum CodingKeys: String, CodingKey {
        case note = "note"
        case segmentIds = "segmentIds"
    }

    public init(note: NoteDTO, segmentIds: [ImportedSegmentIdDTO]) {
        self.note = note
        self.segmentIds = segmentIds
    }
}

// MARK: - NoteDTO
public struct NoteDTO: Codable, Sendable {
    public let body: String
    public let createdAt: String
    public let date: String
    public let duration: String
    public let enhanced: String
    public let generatedRevision: Int?
    public let generationState: GenerationState
    public let id: String
    public let importedAt: String?
    public let recordingState: RecordingState
    public let title: String
    public let transcriptRevision: Int
    public let transcriptSegments: [TranscriptSegmentDTO]
    public let updatedAt: String

    public enum CodingKeys: String, CodingKey {
        case body = "body"
        case createdAt = "createdAt"
        case date = "date"
        case duration = "duration"
        case enhanced = "enhanced"
        case generatedRevision = "generatedRevision"
        case generationState = "generationState"
        case id = "id"
        case importedAt = "importedAt"
        case recordingState = "recordingState"
        case title = "title"
        case transcriptRevision = "transcriptRevision"
        case transcriptSegments = "transcriptSegments"
        case updatedAt = "updatedAt"
    }

    public init(body: String, createdAt: String, date: String, duration: String, enhanced: String, generatedRevision: Int?, generationState: GenerationState, id: String, importedAt: String? = nil, recordingState: RecordingState, title: String, transcriptRevision: Int, transcriptSegments: [TranscriptSegmentDTO], updatedAt: String) {
        self.body = body
        self.createdAt = createdAt
        self.date = date
        self.duration = duration
        self.enhanced = enhanced
        self.generatedRevision = generatedRevision
        self.generationState = generationState
        self.id = id
        self.importedAt = importedAt
        self.recordingState = recordingState
        self.title = title
        self.transcriptRevision = transcriptRevision
        self.transcriptSegments = transcriptSegments
        self.updatedAt = updatedAt
    }
}

// MARK: - ImportedSegmentIdDTO
public struct ImportedSegmentIdDTO: Codable, Sendable {
    public let from: String
    public let to: String

    public enum CodingKeys: String, CodingKey {
        case from = "from"
        case to = "to"
    }

    public init(from: String, to: String) {
        self.from = from
        self.to = to
    }
}

// MARK: - RecordingCommandDTO
public struct RecordingCommandDTO: Codable, Sendable {
    public let command: Command

    public enum CodingKeys: String, CodingKey {
        case command = "command"
    }

    public init(command: Command) {
        self.command = command
    }
}

public enum Command: String, Codable, Sendable {
    case resume = "resume"
    case start = "start"
    case stop = "stop"
}

// MARK: - GenerateNotesCommandDTO
public struct GenerateNotesCommandDTO: Codable, Sendable {
    public let expectedTranscriptRevision: Int

    public enum CodingKeys: String, CodingKey {
        case expectedTranscriptRevision = "expectedTranscriptRevision"
    }

    public init(expectedTranscriptRevision: Int) {
        self.expectedTranscriptRevision = expectedTranscriptRevision
    }
}

// MARK: - GenerationProgressDTO
public struct GenerationProgressDTO: Codable, Sendable {
    public let noteId: String
    public let state: GenerationState
    public let transcriptRevision: Int

    public enum CodingKeys: String, CodingKey {
        case noteId = "noteId"
        case state = "state"
        case transcriptRevision = "transcriptRevision"
    }

    public init(noteId: String, state: GenerationState, transcriptRevision: Int) {
        self.noteId = noteId
        self.state = state
        self.transcriptRevision = transcriptRevision
    }
}

// MARK: - SharedNoteDTO
public struct SharedNoteDTO: Codable, Sendable {
    public let body: String
    public let date: String
    public let duration: String
    public let enhanced: String
    public let expiresAt: String?
    public let sharedAt: String
    public let shareId: String
    public let title: String

    public enum CodingKeys: String, CodingKey {
        case body = "body"
        case date = "date"
        case duration = "duration"
        case enhanced = "enhanced"
        case expiresAt = "expiresAt"
        case sharedAt = "sharedAt"
        case shareId = "shareId"
        case title = "title"
    }

    public init(body: String, date: String, duration: String, enhanced: String, expiresAt: String?, sharedAt: String, shareId: String, title: String) {
        self.body = body
        self.date = date
        self.duration = duration
        self.enhanced = enhanced
        self.expiresAt = expiresAt
        self.sharedAt = sharedAt
        self.shareId = shareId
        self.title = title
    }
}

/// Explicit fields to publish as an immutable snapshot. Transcript is never publishable.
// MARK: - CreateShareInput
public struct CreateShareInput: Codable, Sendable {
    public let expiresAt: String?
    public let includeBody: Bool
    public let includeEnhanced: Bool

    public enum CodingKeys: String, CodingKey {
        case expiresAt = "expiresAt"
        case includeBody = "includeBody"
        case includeEnhanced = "includeEnhanced"
    }

    public init(expiresAt: String? = nil, includeBody: Bool, includeEnhanced: Bool) {
        self.expiresAt = expiresAt
        self.includeBody = includeBody
        self.includeEnhanced = includeEnhanced
    }
}

/// Opaque token is returned once at creation; only its SHA-256 hash is stored.
// MARK: - CreatedShareDTO
public struct CreatedShareDTO: Codable, Sendable {
    public let path: String
    public let share: ShareDTO
    public let token: String

    public enum CodingKeys: String, CodingKey {
        case path = "path"
        case share = "share"
        case token = "token"
    }

    public init(path: String, share: ShareDTO, token: String) {
        self.path = path
        self.share = share
        self.token = token
    }
}

// MARK: - ShareDTO
public struct ShareDTO: Codable, Sendable {
    public let createdAt: String
    public let expiresAt: String?
    public let id: String
    public let noteId: String
    public let revokedAt: String?

    public enum CodingKeys: String, CodingKey {
        case createdAt = "createdAt"
        case expiresAt = "expiresAt"
        case id = "id"
        case noteId = "noteId"
        case revokedAt = "revokedAt"
    }

    public init(createdAt: String, expiresAt: String?, id: String, noteId: String, revokedAt: String?) {
        self.createdAt = createdAt
        self.expiresAt = expiresAt
        self.id = id
        self.noteId = noteId
        self.revokedAt = revokedAt
    }
}

// MARK: - HealthDTO
public struct HealthDTO: Codable, Sendable {
    public let features: [String]?
    public let status: HealthDTOStatus
    public let version: Version

    public enum CodingKeys: String, CodingKey {
        case features = "features"
        case status = "status"
        case version = "version"
    }

    public init(features: [String]? = nil, status: HealthDTOStatus, version: Version) {
        self.features = features
        self.status = status
        self.version = version
    }
}

public enum HealthDTOStatus: String, Codable, Sendable {
    case ok = "ok"
}

public enum Version: String, Codable, Sendable {
    case v1 = "v1"
}

// MARK: - DeletedDTO
public struct DeletedDTO: Codable, Sendable {
    public let deleted: Bool

    public enum CodingKeys: String, CodingKey {
        case deleted = "deleted"
    }

    public init(deleted: Bool) {
        self.deleted = deleted
    }
}

// MARK: - APIErrorDTO
public struct APIErrorDTO: Codable, Sendable {
    public let error: APIErrorCode
    public let message: String

    public enum CodingKeys: String, CodingKey {
        case error = "error"
        case message = "message"
    }

    public init(error: APIErrorCode, message: String) {
        self.error = error
        self.message = message
    }
}

public enum APIErrorCode: String, Codable, Sendable {
    case conflict = "conflict"
    case forbidden = "forbidden"
    case invalidRequest = "invalid_request"
    case notFound = "not_found"
    case unauthorized = "unauthorized"
    case unavailable = "unavailable"
}

// MARK: - DownloadsDTO
public struct DownloadsDTO: Codable, Sendable {
    public let iosAppStoreUrl: String?
    public let mac: MACDownloadDTO?
    public let version: String

    public enum CodingKeys: String, CodingKey {
        case iosAppStoreUrl = "iosAppStoreUrl"
        case mac = "mac"
        case version = "version"
    }

    public init(iosAppStoreUrl: String?, mac: MACDownloadDTO?, version: String) {
        self.iosAppStoreUrl = iosAppStoreUrl
        self.mac = mac
        self.version = version
    }
}

// MARK: - MACDownloadDTO
public struct MACDownloadDTO: Codable, Sendable {
    public let arch: String
    public let installer: DownloadFileDTO
    public let minimumSystem: String
    public let update: DownloadFileDTO?

    public enum CodingKeys: String, CodingKey {
        case arch = "arch"
        case installer = "installer"
        case minimumSystem = "minimumSystem"
        case update = "update"
    }

    public init(arch: String, installer: DownloadFileDTO, minimumSystem: String, update: DownloadFileDTO?) {
        self.arch = arch
        self.installer = installer
        self.minimumSystem = minimumSystem
        self.update = update
    }
}

// MARK: - DownloadFileDTO
public struct DownloadFileDTO: Codable, Sendable {
    public let file: String
    public let sha256: String
    public let size: Int
    public let url: String

    public enum CodingKeys: String, CodingKey {
        case file = "file"
        case sha256 = "sha256"
        case size = "size"
        case url = "url"
    }

    public init(file: String, sha256: String, size: Int, url: String) {
        self.file = file
        self.sha256 = sha256
        self.size = size
        self.url = url
    }
}

// MARK: - StorageTestInput
public struct StorageTestInput: Codable, Sendable {
    public let accessKeyId: String?
    public let backend: StorageBackend?
    public let bucket: String?
    public let endpoint: String?
    public let forcePathStyle: Bool?
    public let storageTestInputPrefix: String?
    public let region: String?
    public let secretAccessKey: String?

    public enum CodingKeys: String, CodingKey {
        case accessKeyId = "accessKeyId"
        case backend = "backend"
        case bucket = "bucket"
        case endpoint = "endpoint"
        case forcePathStyle = "forcePathStyle"
        case storageTestInputPrefix = "prefix"
        case region = "region"
        case secretAccessKey = "secretAccessKey"
    }

    public init(accessKeyId: String? = nil, backend: StorageBackend? = nil, bucket: String? = nil, endpoint: String? = nil, forcePathStyle: Bool? = nil, storageTestInputPrefix: String? = nil, region: String? = nil, secretAccessKey: String? = nil) {
        self.accessKeyId = accessKeyId
        self.backend = backend
        self.bucket = bucket
        self.endpoint = endpoint
        self.forcePathStyle = forcePathStyle
        self.storageTestInputPrefix = storageTestInputPrefix
        self.region = region
        self.secretAccessKey = secretAccessKey
    }
}

// MARK: - StorageTestDTO
public struct StorageTestDTO: Codable, Sendable {
    public let backend: StorageBackend
    public let message: String
    public let ok: Bool
    public let step: Step?

    public enum CodingKeys: String, CodingKey {
        case backend = "backend"
        case message = "message"
        case ok = "ok"
        case step = "step"
    }

    public init(backend: StorageBackend, message: String, ok: Bool, step: Step?) {
        self.backend = backend
        self.message = message
        self.ok = ok
        self.step = step
    }
}

public enum Step: String, Codable, Sendable {
    case delete = "delete"
    case read = "read"
    case write = "write"
}

// MARK: - OnboardingStepInput
public struct OnboardingStepInput: Codable, Sendable {
    public let state: OnboardingStepState
    public let step: OnboardingStep

    public enum CodingKeys: String, CodingKey {
        case state = "state"
        case step = "step"
    }

    public init(state: OnboardingStepState, step: OnboardingStep) {
        self.state = state
        self.step = step
    }
}

public enum OnboardingStepState: String, Codable, Sendable {
    case done = "done"
    case skipped = "skipped"
}

public enum OnboardingStep: String, Codable, Sendable {
    case language = "language"
    case models = "models"
    case name = "name"
    case owner = "owner"
    case signIn = "signIn"
    case speech = "speech"
    case storage = "storage"
}

// MARK: - GoogleVerificationInput
public struct GoogleVerificationInput: Codable, Sendable {
    public let clientId: String
    public let clientSecret: String

    public enum CodingKeys: String, CodingKey {
        case clientId = "clientId"
        case clientSecret = "clientSecret"
    }

    public init(clientId: String, clientSecret: String) {
        self.clientId = clientId
        self.clientSecret = clientSecret
    }
}

// MARK: - GoogleVerificationStartDTO
public struct GoogleVerificationStartDTO: Codable, Sendable {
    public let authorizationUrl: String

    public enum CodingKeys: String, CodingKey {
        case authorizationUrl = "authorizationUrl"
    }

    public init(authorizationUrl: String) {
        self.authorizationUrl = authorizationUrl
    }
}

// MARK: - GoogleVerificationStatusDTO
public struct GoogleVerificationStatusDTO: Codable, Sendable {
    public let reason: String?
    public let state: GoogleVerificationState

    public enum CodingKeys: String, CodingKey {
        case reason = "reason"
        case state = "state"
    }

    public init(reason: String?, state: GoogleVerificationState) {
        self.reason = reason
        self.state = state
    }
}

public enum GoogleVerificationState: String, Codable, Sendable {
    case done = "done"
    case failed = "failed"
    case none = "none"
    case pending = "pending"
}

// MARK: - SwitchToPasswordInput
public struct SwitchToPasswordInput: Codable, Sendable {
    public let password: String

    public enum CodingKeys: String, CodingKey {
        case password = "password"
    }

    public init(password: String) {
        self.password = password
    }
}

// MARK: - LiveHelloMessage
public struct LiveHelloMessage: Codable, Sendable {
    public let captureId: String
    public let language: String?
    public let liveHelloMessageProtocol: Double
    public let streams: [LiveStreamHello]
    public let type: LiveHelloMessageType

    public enum CodingKeys: String, CodingKey {
        case captureId = "captureId"
        case language = "language"
        case liveHelloMessageProtocol = "protocol"
        case streams = "streams"
        case type = "type"
    }

    public init(captureId: String, language: String? = nil, liveHelloMessageProtocol: Double, streams: [LiveStreamHello], type: LiveHelloMessageType) {
        self.captureId = captureId
        self.language = language
        self.liveHelloMessageProtocol = liveHelloMessageProtocol
        self.streams = streams
        self.type = type
    }
}

// MARK: - LiveStreamHello
public struct LiveStreamHello: Codable, Sendable {
    public let baseMs: Int
    public let channel: AudioChannel
    public let encoding: Encoding
    public let resumeFromSample: Int
    public let sampleRate: Int

    public enum CodingKeys: String, CodingKey {
        case baseMs = "baseMs"
        case channel = "channel"
        case encoding = "encoding"
        case resumeFromSample = "resumeFromSample"
        case sampleRate = "sampleRate"
    }

    public init(baseMs: Int, channel: AudioChannel, encoding: Encoding, resumeFromSample: Int, sampleRate: Int) {
        self.baseMs = baseMs
        self.channel = channel
        self.encoding = encoding
        self.resumeFromSample = resumeFromSample
        self.sampleRate = sampleRate
    }
}

public enum Encoding: String, Codable, Sendable {
    case pcmS16le = "pcm_s16le"
}

public enum LiveHelloMessageType: String, Codable, Sendable {
    case hello = "hello"
}

// MARK: - LiveMarkMessage
public struct LiveMarkMessage: Codable, Sendable {
    public let atSample: AtSample
    public let kind: LiveMarkKind
    public let type: LiveMarkMessageType

    public enum CodingKeys: String, CodingKey {
        case atSample = "atSample"
        case kind = "kind"
        case type = "type"
    }

    public init(atSample: AtSample, kind: LiveMarkKind, type: LiveMarkMessageType) {
        self.atSample = atSample
        self.kind = kind
        self.type = type
    }
}

// MARK: - AtSample
public struct AtSample: Codable, Sendable {
    public let mic: Int?
    public let system: Int?

    public enum CodingKeys: String, CodingKey {
        case mic = "mic"
        case system = "system"
    }

    public init(mic: Int?, system: Int?) {
        self.mic = mic
        self.system = system
    }
}

public enum LiveMarkKind: String, Codable, Sendable {
    case end = "end"
    case pause = "pause"
    case resume = "resume"
}

public enum LiveMarkMessageType: String, Codable, Sendable {
    case mark = "mark"
}

// MARK: - LiveReadyMessage
public struct LiveReadyMessage: Codable, Sendable {
    public let model: String
    public let provider: String
    public let resumeWindowMs: Int
    public let sessionId: String
    public let streams: [LiveReadyStream]
    public let transcriptRevision: Int
    public let type: LiveReadyMessageType

    public enum CodingKeys: String, CodingKey {
        case model = "model"
        case provider = "provider"
        case resumeWindowMs = "resumeWindowMs"
        case sessionId = "sessionId"
        case streams = "streams"
        case transcriptRevision = "transcriptRevision"
        case type = "type"
    }

    public init(model: String, provider: String, resumeWindowMs: Int, sessionId: String, streams: [LiveReadyStream], transcriptRevision: Int, type: LiveReadyMessageType) {
        self.model = model
        self.provider = provider
        self.resumeWindowMs = resumeWindowMs
        self.sessionId = sessionId
        self.streams = streams
        self.transcriptRevision = transcriptRevision
        self.type = type
    }
}

// MARK: - LiveReadyStream
public struct LiveReadyStream: Codable, Sendable {
    public let acceptFromSample: Int
    public let channel: AudioChannel

    public enum CodingKeys: String, CodingKey {
        case acceptFromSample = "acceptFromSample"
        case channel = "channel"
    }

    public init(acceptFromSample: Int, channel: AudioChannel) {
        self.acceptFromSample = acceptFromSample
        self.channel = channel
    }
}

public enum LiveReadyMessageType: String, Codable, Sendable {
    case ready = "ready"
}

// MARK: - LiveAckMessage
public struct LiveAckMessage: Codable, Sendable {
    public let channel: AudioChannel
    public let throughSample: Int
    public let transcribedThroughMs: Int
    public let type: LiveAckMessageType

    public enum CodingKeys: String, CodingKey {
        case channel = "channel"
        case throughSample = "throughSample"
        case transcribedThroughMs = "transcribedThroughMs"
        case type = "type"
    }

    public init(channel: AudioChannel, throughSample: Int, transcribedThroughMs: Int, type: LiveAckMessageType) {
        self.channel = channel
        self.throughSample = throughSample
        self.transcribedThroughMs = transcribedThroughMs
        self.type = type
    }
}

public enum LiveAckMessageType: String, Codable, Sendable {
    case ack = "ack"
}

// MARK: - LiveInterimMessage
public struct LiveInterimMessage: Codable, Sendable {
    public let channel: AudioChannel
    public let endMs: Int
    public let rev: Int
    public let speakerId: String?
    public let speakerLabel: String?
    public let startMs: Int
    public let text: String
    public let type: LiveInterimMessageType
    public let utteranceId: String

    public enum CodingKeys: String, CodingKey {
        case channel = "channel"
        case endMs = "endMs"
        case rev = "rev"
        case speakerId = "speakerId"
        case speakerLabel = "speakerLabel"
        case startMs = "startMs"
        case text = "text"
        case type = "type"
        case utteranceId = "utteranceId"
    }

    public init(channel: AudioChannel, endMs: Int, rev: Int, speakerId: String? = nil, speakerLabel: String? = nil, startMs: Int, text: String, type: LiveInterimMessageType, utteranceId: String) {
        self.channel = channel
        self.endMs = endMs
        self.rev = rev
        self.speakerId = speakerId
        self.speakerLabel = speakerLabel
        self.startMs = startMs
        self.text = text
        self.type = type
        self.utteranceId = utteranceId
    }
}

public enum LiveInterimMessageType: String, Codable, Sendable {
    case interim = "interim"
}

// MARK: - LiveFinalMessage
public struct LiveFinalMessage: Codable, Sendable {
    public let segment: TranscriptSegmentDTO
    public let transcriptRevision: Int
    public let type: LiveFinalMessageType
    public let utteranceId: String

    public enum CodingKeys: String, CodingKey {
        case segment = "segment"
        case transcriptRevision = "transcriptRevision"
        case type = "type"
        case utteranceId = "utteranceId"
    }

    public init(segment: TranscriptSegmentDTO, transcriptRevision: Int, type: LiveFinalMessageType, utteranceId: String) {
        self.segment = segment
        self.transcriptRevision = transcriptRevision
        self.type = type
        self.utteranceId = utteranceId
    }
}

public enum LiveFinalMessageType: String, Codable, Sendable {
    case typeFinal = "final"
}

// MARK: - LiveSpeakersMessage
public struct LiveSpeakersMessage: Codable, Sendable {
    public let merges: [LiveSpeakerMerge]
    public let type: LiveSpeakersMessageType

    public enum CodingKeys: String, CodingKey {
        case merges = "merges"
        case type = "type"
    }

    public init(merges: [LiveSpeakerMerge], type: LiveSpeakersMessageType) {
        self.merges = merges
        self.type = type
    }
}

// MARK: - LiveSpeakerMerge
public struct LiveSpeakerMerge: Codable, Sendable {
    public let from: String
    public let to: String

    public enum CodingKeys: String, CodingKey {
        case from = "from"
        case to = "to"
    }

    public init(from: String, to: String) {
        self.from = from
        self.to = to
    }
}

public enum LiveSpeakersMessageType: String, Codable, Sendable {
    case speakers = "speakers"
}

// MARK: - LiveGapMessage
public struct LiveGapMessage: Codable, Sendable {
    public let channel: AudioChannel
    public let fromMs: Int
    public let reason: LiveGapReason
    public let toMs: Int
    public let type: LiveGapMessageType

    public enum CodingKeys: String, CodingKey {
        case channel = "channel"
        case fromMs = "fromMs"
        case reason = "reason"
        case toMs = "toMs"
        case type = "type"
    }

    public init(channel: AudioChannel, fromMs: Int, reason: LiveGapReason, toMs: Int, type: LiveGapMessageType) {
        self.channel = channel
        self.fromMs = fromMs
        self.reason = reason
        self.toMs = toMs
        self.type = type
    }
}

public enum LiveGapReason: String, Codable, Sendable {
    case providerUnavailable = "provider_unavailable"
    case resumeWindowExceeded = "resume_window_exceeded"
}

public enum LiveGapMessageType: String, Codable, Sendable {
    case gap = "gap"
}

// MARK: - LiveStatusMessage
public struct LiveStatusMessage: Codable, Sendable {
    public let channel: AudioChannel
    public let lagMs: Int
    public let state: LiveStreamState
    public let type: LiveStatusMessageType

    public enum CodingKeys: String, CodingKey {
        case channel = "channel"
        case lagMs = "lagMs"
        case state = "state"
        case type = "type"
    }

    public init(channel: AudioChannel, lagMs: Int, state: LiveStreamState, type: LiveStatusMessageType) {
        self.channel = channel
        self.lagMs = lagMs
        self.state = state
        self.type = type
    }
}

public enum LiveStreamState: String, Codable, Sendable {
    case catchingUp = "catching_up"
    case live = "live"
    case paused = "paused"
    case providerUnavailable = "provider_unavailable"
}

public enum LiveStatusMessageType: String, Codable, Sendable {
    case status = "status"
}

// MARK: - LiveErrorMessage
public struct LiveErrorMessage: Codable, Sendable {
    public let code: LiveErrorCode
    public let message: String
    public let retryable: Bool
    public let retryAfterMs: Int?
    public let type: LiveErrorMessageType

    public enum CodingKeys: String, CodingKey {
        case code = "code"
        case message = "message"
        case retryable = "retryable"
        case retryAfterMs = "retryAfterMs"
        case type = "type"
    }

    public init(code: LiveErrorCode, message: String, retryable: Bool, retryAfterMs: Int? = nil, type: LiveErrorMessageType) {
        self.code = code
        self.message = message
        self.retryable = retryable
        self.retryAfterMs = retryAfterMs
        self.type = type
    }
}

public enum LiveErrorCode: String, Codable, Sendable {
    case clientTooSlow = "client_too_slow"
    case malformed = "malformed"
    case notFound = "not_found"
    case overLimit = "over_limit"
    case providerUnavailable = "provider_unavailable"
    case replaced = "replaced"
    case unauthenticated = "unauthenticated"
}

public enum LiveErrorMessageType: String, Codable, Sendable {
    case error = "error"
}


extension NoteSummaryDTO {
    private enum ContractWireKeys: String, CodingKey {
        case id
        case title
        case date
        case duration
        case createdAt
        case updatedAt
        case recordingState
        case generationState
        case transcriptRevision
        case generatedRevision
        case hasBody
        case hasSummary
        case hasTranscript
        case speakerCount
        case excerpt
        case meeting
        case attendees
        case organizationId
        case folders
    }
    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: ContractWireKeys.self)
        try container.encode(id, forKey: .id)
        try container.encode(title, forKey: .title)
        try container.encode(date, forKey: .date)
        try container.encode(duration, forKey: .duration)
        try container.encode(createdAt, forKey: .createdAt)
        try container.encode(updatedAt, forKey: .updatedAt)
        try container.encode(recordingState, forKey: .recordingState)
        try container.encode(generationState, forKey: .generationState)
        try container.encode(transcriptRevision, forKey: .transcriptRevision)
        try container.encode(generatedRevision, forKey: .generatedRevision)
        try container.encode(hasBody, forKey: .hasBody)
        try container.encode(hasSummary, forKey: .hasSummary)
        try container.encode(hasTranscript, forKey: .hasTranscript)
        try container.encode(speakerCount, forKey: .speakerCount)
        try container.encode(excerpt, forKey: .excerpt)
        try container.encode(meeting, forKey: .meeting)
        try container.encode(attendees, forKey: .attendees)
        try container.encode(organizationId, forKey: .organizationId)
        try container.encode(folders, forKey: .folders)
    }
}


extension NoteSummariesPageDTO {
    private enum ContractWireKeys: String, CodingKey {
        case notes
        case nextCursor
        case total
    }
    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: ContractWireKeys.self)
        try container.encode(notes, forKey: .notes)
        try container.encode(nextCursor, forKey: .nextCursor)
        try container.encode(total, forKey: .total)
    }
}


extension NoteSearchExcerptDTO {
    private enum ContractWireKeys: String, CodingKey {
        case source
        case parts
        case segmentId
        case startMs
    }
    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: ContractWireKeys.self)
        try container.encode(source, forKey: .source)
        try container.encode(parts, forKey: .parts)
        try container.encode(segmentId, forKey: .segmentId)
        try container.encode(startMs, forKey: .startMs)
    }
}


extension NoteSearchResultDTO {
    private enum ContractWireKeys: String, CodingKey {
        case note
        case matchedIn
        case title
        case excerpt
    }
    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: ContractWireKeys.self)
        try container.encode(note, forKey: .note)
        try container.encode(matchedIn, forKey: .matchedIn)
        try container.encode(title, forKey: .title)
        try container.encode(excerpt, forKey: .excerpt)
    }
}


extension NoteSearchPageDTO {
    private enum ContractWireKeys: String, CodingKey {
        case results
        case nextCursor
        case total
        case terms
    }
    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: ContractWireKeys.self)
        try container.encode(results, forKey: .results)
        try container.encode(nextCursor, forKey: .nextCursor)
        try container.encode(total, forKey: .total)
        try container.encode(terms, forKey: .terms)
    }
}


extension ModelCatalogDTO {
    private enum ContractWireKeys: String, CodingKey {
        case provider
        case models
        case fetchedAt
        case notice
    }
    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: ContractWireKeys.self)
        try container.encode(provider, forKey: .provider)
        try container.encode(models, forKey: .models)
        try container.encode(fetchedAt, forKey: .fetchedAt)
        try container.encode(notice, forKey: .notice)
    }
}


extension MCPAuthorizationDTO {
    private enum ContractWireKeys: String, CodingKey {
        case clientId
        case clientName
        case redirectUri
        case scopes
        case resource
        case workspace
        case globalWorkspaceSwitch
    }
    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: ContractWireKeys.self)
        try container.encode(clientId, forKey: .clientId)
        try container.encode(clientName, forKey: .clientName)
        try container.encode(redirectUri, forKey: .redirectUri)
        try container.encode(scopes, forKey: .scopes)
        try container.encode(resource, forKey: .resource)
        try container.encode(workspace, forKey: .workspace)
        try container.encode(globalWorkspaceSwitch, forKey: .globalWorkspaceSwitch)
    }
}


extension AgentBriefDTO {
    private enum ContractWireKeys: String, CodingKey {
        case body
        case updatedAt
        case workspaceId
        case source
        case automaticallyInferred
    }
    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: ContractWireKeys.self)
        try container.encode(body, forKey: .body)
        try container.encode(updatedAt, forKey: .updatedAt)
        try container.encode(workspaceId, forKey: .workspaceId)
        try container.encode(source, forKey: .source)
        try container.encode(automaticallyInferred, forKey: .automaticallyInferred)
    }
}


extension OrganizationsDTO {
    private enum ContractWireKeys: String, CodingKey {
        case organizations
        case activeOrganizationId
    }
    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: ContractWireKeys.self)
        try container.encode(organizations, forKey: .organizations)
        try container.encode(activeOrganizationId, forKey: .activeOrganizationId)
    }
}


extension GoogleConnectionDTO {
    private enum ContractWireKeys: String, CodingKey {
        case id
        case email
        case displayName
        case status
        case contactsEnabled
        case lastSyncedAt
        case lastSyncError
        case createdAt
    }
    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: ContractWireKeys.self)
        try container.encode(id, forKey: .id)
        try container.encode(email, forKey: .email)
        try container.encode(displayName, forKey: .displayName)
        try container.encode(status, forKey: .status)
        try container.encode(contactsEnabled, forKey: .contactsEnabled)
        try container.encode(lastSyncedAt, forKey: .lastSyncedAt)
        try container.encode(lastSyncError, forKey: .lastSyncError)
        try container.encode(createdAt, forKey: .createdAt)
    }
}


extension GoogleStatusDTO {
    private enum ContractWireKeys: String, CodingKey {
        case configured
        case redirectUri
        case connections
        case needsHttps
    }
    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: ContractWireKeys.self)
        try container.encode(configured, forKey: .configured)
        try container.encode(redirectUri, forKey: .redirectUri)
        try container.encode(connections, forKey: .connections)
        try container.encodeIfPresent(needsHttps, forKey: .needsHttps)
    }
}


extension MeetingParticipantDTO {
    private enum ContractWireKeys: String, CodingKey {
        case id
        case email
        case displayName
        case givenName
        case familyName
        case nameSource
        case isSelf
        case isOrganizer
        case isResource
        case responseStatus
    }
    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: ContractWireKeys.self)
        try container.encode(id, forKey: .id)
        try container.encode(email, forKey: .email)
        try container.encode(displayName, forKey: .displayName)
        try container.encode(givenName, forKey: .givenName)
        try container.encode(familyName, forKey: .familyName)
        try container.encode(nameSource, forKey: .nameSource)
        try container.encode(isSelf, forKey: .isSelf)
        try container.encode(isOrganizer, forKey: .isOrganizer)
        try container.encode(isResource, forKey: .isResource)
        try container.encode(responseStatus, forKey: .responseStatus)
    }
}


extension MeetingContextDTO {
    private enum ContractWireKeys: String, CodingKey {
        case eventId
        case provider
        case title
        case calendarName
        case startAt
        case endAt
        case allDay
        case timeZone
        case location
        case meetingUrl
        case eventUrl
        case organizer
        case participants
    }
    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: ContractWireKeys.self)
        try container.encode(eventId, forKey: .eventId)
        try container.encode(provider, forKey: .provider)
        try container.encode(title, forKey: .title)
        try container.encode(calendarName, forKey: .calendarName)
        try container.encode(startAt, forKey: .startAt)
        try container.encode(endAt, forKey: .endAt)
        try container.encode(allDay, forKey: .allDay)
        try container.encode(timeZone, forKey: .timeZone)
        try container.encode(location, forKey: .location)
        try container.encode(meetingUrl, forKey: .meetingUrl)
        try container.encode(eventUrl, forKey: .eventUrl)
        try container.encode(organizer, forKey: .organizer)
        try container.encode(participants, forKey: .participants)
    }
}


extension NoteContextDTO {
    private enum ContractWireKeys: String, CodingKey {
        case noteId
        case meeting
    }
    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: ContractWireKeys.self)
        try container.encode(noteId, forKey: .noteId)
        try container.encode(meeting, forKey: .meeting)
    }
}


extension UpdateNoteContextInput {
    private enum ContractWireKeys: String, CodingKey {
        case eventId
    }
    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: ContractWireKeys.self)
        try container.encode(eventId, forKey: .eventId)
    }
}


extension AuthSessionDTO {
    private enum ContractWireKeys: String, CodingKey {
        case user
        case expiresAt
    }
    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: ContractWireKeys.self)
        try container.encode(user, forKey: .user)
        try container.encode(expiresAt, forKey: .expiresAt)
    }
}


extension SignInSettingsDTO {
    private enum ContractWireKeys: String, CodingKey {
        case password
        case google
        case allowedEmails
        case googleClientId
        case googleClientSecretSet
        case googleRedirectUris
        case googleNeedsHttps
    }
    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: ContractWireKeys.self)
        try container.encode(password, forKey: .password)
        try container.encode(google, forKey: .google)
        try container.encode(allowedEmails, forKey: .allowedEmails)
        try container.encode(googleClientId, forKey: .googleClientId)
        try container.encode(googleClientSecretSet, forKey: .googleClientSecretSet)
        try container.encode(googleRedirectUris, forKey: .googleRedirectUris)
        try container.encodeIfPresent(googleNeedsHttps, forKey: .googleNeedsHttps)
    }
}


extension AdminSettingsDTO {
    private enum ContractWireKeys: String, CodingKey {
        case instanceName
        case languageModel
        case instanceLogo
        case providers
        case chatModels
        case generationModel
        case transcriptionModel
        case liveTranscriptionModel
        case openRouterConfigured
        case elevenLabsConfigured
        case openRouterSource
        case elevenLabsSource
        case calendarRefreshMinutes
        case calendarLookAheadDays
        case signIn
        case storage
        case liveMaxSessions
        case liveHoldSeconds
        case apps
        case unusedEnvironment
        case onboarding
        case updatedAt
    }
    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: ContractWireKeys.self)
        try container.encode(instanceName, forKey: .instanceName)
        try container.encode(languageModel, forKey: .languageModel)
        try container.encode(instanceLogo, forKey: .instanceLogo)
        try container.encode(providers, forKey: .providers)
        try container.encode(chatModels, forKey: .chatModels)
        try container.encode(generationModel, forKey: .generationModel)
        try container.encode(transcriptionModel, forKey: .transcriptionModel)
        try container.encodeIfPresent(liveTranscriptionModel, forKey: .liveTranscriptionModel)
        try container.encode(openRouterConfigured, forKey: .openRouterConfigured)
        try container.encode(elevenLabsConfigured, forKey: .elevenLabsConfigured)
        try container.encode(openRouterSource, forKey: .openRouterSource)
        try container.encode(elevenLabsSource, forKey: .elevenLabsSource)
        try container.encode(calendarRefreshMinutes, forKey: .calendarRefreshMinutes)
        try container.encode(calendarLookAheadDays, forKey: .calendarLookAheadDays)
        try container.encodeIfPresent(signIn, forKey: .signIn)
        try container.encodeIfPresent(storage, forKey: .storage)
        try container.encodeIfPresent(liveMaxSessions, forKey: .liveMaxSessions)
        try container.encodeIfPresent(liveHoldSeconds, forKey: .liveHoldSeconds)
        try container.encodeIfPresent(apps, forKey: .apps)
        try container.encodeIfPresent(unusedEnvironment, forKey: .unusedEnvironment)
        try container.encodeIfPresent(onboarding, forKey: .onboarding)
        try container.encode(updatedAt, forKey: .updatedAt)
    }
}


extension CalendarDTO {
    private enum ContractWireKeys: String, CodingKey {
        case id
        case name
        case provider
        case color
        case enabled
        case sourceHost
        case lastSyncedAt
        case lastSyncError
        case eventCount
        case createdAt
    }
    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: ContractWireKeys.self)
        try container.encode(id, forKey: .id)
        try container.encode(name, forKey: .name)
        try container.encode(provider, forKey: .provider)
        try container.encode(color, forKey: .color)
        try container.encode(enabled, forKey: .enabled)
        try container.encode(sourceHost, forKey: .sourceHost)
        try container.encode(lastSyncedAt, forKey: .lastSyncedAt)
        try container.encode(lastSyncError, forKey: .lastSyncError)
        try container.encode(eventCount, forKey: .eventCount)
        try container.encode(createdAt, forKey: .createdAt)
    }
}


extension CalendarEventDTO {
    private enum ContractWireKeys: String, CodingKey {
        case id
        case calendarId
        case calendarName
        case color
        case title
        case startAt
        case endAt
        case allDay
        case startDate
        case endDate
        case timeZone
        case location
        case meetingUrl
        case noteId
        case meetingUid
        case occurrence
        case attendeeCount
    }
    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: ContractWireKeys.self)
        try container.encode(id, forKey: .id)
        try container.encode(calendarId, forKey: .calendarId)
        try container.encode(calendarName, forKey: .calendarName)
        try container.encode(color, forKey: .color)
        try container.encode(title, forKey: .title)
        try container.encode(startAt, forKey: .startAt)
        try container.encode(endAt, forKey: .endAt)
        try container.encode(allDay, forKey: .allDay)
        try container.encode(startDate, forKey: .startDate)
        try container.encode(endDate, forKey: .endDate)
        try container.encode(timeZone, forKey: .timeZone)
        try container.encode(location, forKey: .location)
        try container.encode(meetingUrl, forKey: .meetingUrl)
        try container.encode(noteId, forKey: .noteId)
        try container.encodeIfPresent(meetingUid, forKey: .meetingUid)
        try container.encodeIfPresent(occurrence, forKey: .occurrence)
        try container.encodeIfPresent(attendeeCount, forKey: .attendeeCount)
    }
}


extension AudioChunkDTO {
    private enum ContractWireKeys: String, CodingKey {
        case id
        case noteId
        case channel
        case sequence
        case startMs
        case durationMs
        case state
        case error
    }
    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: ContractWireKeys.self)
        try container.encode(id, forKey: .id)
        try container.encode(noteId, forKey: .noteId)
        try container.encode(channel, forKey: .channel)
        try container.encode(sequence, forKey: .sequence)
        try container.encode(startMs, forKey: .startMs)
        try container.encode(durationMs, forKey: .durationMs)
        try container.encode(state, forKey: .state)
        try container.encode(error, forKey: .error)
    }
}


extension ChatCitationDTO {
    private enum ContractWireKeys: String, CodingKey {
        case id
        case noteId
        case title
        case source
        case segmentId
        case startMs
        case quote
        case transcriptRevision
    }
    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: ContractWireKeys.self)
        try container.encode(id, forKey: .id)
        try container.encode(noteId, forKey: .noteId)
        try container.encode(title, forKey: .title)
        try container.encode(source, forKey: .source)
        try container.encode(segmentId, forKey: .segmentId)
        try container.encode(startMs, forKey: .startMs)
        try container.encode(quote, forKey: .quote)
        try container.encode(transcriptRevision, forKey: .transcriptRevision)
    }
}


extension EvidenceQuoteDTO {
    private enum ContractWireKeys: String, CodingKey {
        case segmentId
        case text
        case speakerLabel
        case startMs
    }
    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: ContractWireKeys.self)
        try container.encode(segmentId, forKey: .segmentId)
        try container.encode(text, forKey: .text)
        try container.encode(speakerLabel, forKey: .speakerLabel)
        try container.encode(startMs, forKey: .startMs)
    }
}


extension TranscriptSegmentDTO {
    private enum ContractWireKeys: String, CodingKey {
        case id
        case channel
        case speakerId
        case speakerLabel
        case text
        case isFinal
        case startMs
        case endMs
    }
    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: ContractWireKeys.self)
        try container.encode(id, forKey: .id)
        try container.encode(channel, forKey: .channel)
        try container.encode(speakerId, forKey: .speakerId)
        try container.encode(speakerLabel, forKey: .speakerLabel)
        try container.encode(text, forKey: .text)
        try container.encode(isFinal, forKey: .isFinal)
        try container.encode(startMs, forKey: .startMs)
        try container.encode(endMs, forKey: .endMs)
    }
}


extension NoteDTO {
    private enum ContractWireKeys: String, CodingKey {
        case id
        case title
        case body
        case enhanced
        case date
        case duration
        case createdAt
        case updatedAt
        case transcriptSegments
        case recordingState
        case generationState
        case transcriptRevision
        case generatedRevision
        case importedAt
    }
    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: ContractWireKeys.self)
        try container.encode(id, forKey: .id)
        try container.encode(title, forKey: .title)
        try container.encode(body, forKey: .body)
        try container.encode(enhanced, forKey: .enhanced)
        try container.encode(date, forKey: .date)
        try container.encode(duration, forKey: .duration)
        try container.encode(createdAt, forKey: .createdAt)
        try container.encode(updatedAt, forKey: .updatedAt)
        try container.encode(transcriptSegments, forKey: .transcriptSegments)
        try container.encode(recordingState, forKey: .recordingState)
        try container.encode(generationState, forKey: .generationState)
        try container.encode(transcriptRevision, forKey: .transcriptRevision)
        try container.encode(generatedRevision, forKey: .generatedRevision)
        try container.encodeIfPresent(importedAt, forKey: .importedAt)
    }
}


extension ImportNoteInput {
    private enum ContractWireKeys: String, CodingKey {
        case id
        case title
        case body
        case enhanced
        case date
        case duration
        case createdAt
        case generated
        case recordingState
        case transcriptSegments
        case meeting
    }
    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: ContractWireKeys.self)
        try container.encode(id, forKey: .id)
        try container.encode(title, forKey: .title)
        try container.encode(body, forKey: .body)
        try container.encode(enhanced, forKey: .enhanced)
        try container.encode(date, forKey: .date)
        try container.encode(duration, forKey: .duration)
        try container.encode(createdAt, forKey: .createdAt)
        try container.encode(generated, forKey: .generated)
        try container.encode(recordingState, forKey: .recordingState)
        try container.encode(transcriptSegments, forKey: .transcriptSegments)
        try container.encode(meeting, forKey: .meeting)
    }
}


extension SharedNoteDTO {
    private enum ContractWireKeys: String, CodingKey {
        case shareId
        case title
        case body
        case enhanced
        case date
        case duration
        case sharedAt
        case expiresAt
    }
    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: ContractWireKeys.self)
        try container.encode(shareId, forKey: .shareId)
        try container.encode(title, forKey: .title)
        try container.encode(body, forKey: .body)
        try container.encode(enhanced, forKey: .enhanced)
        try container.encode(date, forKey: .date)
        try container.encode(duration, forKey: .duration)
        try container.encode(sharedAt, forKey: .sharedAt)
        try container.encode(expiresAt, forKey: .expiresAt)
    }
}


extension ShareDTO {
    private enum ContractWireKeys: String, CodingKey {
        case id
        case noteId
        case createdAt
        case expiresAt
        case revokedAt
    }
    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: ContractWireKeys.self)
        try container.encode(id, forKey: .id)
        try container.encode(noteId, forKey: .noteId)
        try container.encode(createdAt, forKey: .createdAt)
        try container.encode(expiresAt, forKey: .expiresAt)
        try container.encode(revokedAt, forKey: .revokedAt)
    }
}


extension MACDownloadDTO {
    private enum ContractWireKeys: String, CodingKey {
        case arch
        case minimumSystem
        case installer
        case update
    }
    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: ContractWireKeys.self)
        try container.encode(arch, forKey: .arch)
        try container.encode(minimumSystem, forKey: .minimumSystem)
        try container.encode(installer, forKey: .installer)
        try container.encode(update, forKey: .update)
    }
}


extension DownloadsDTO {
    private enum ContractWireKeys: String, CodingKey {
        case version
        case mac
        case iosAppStoreUrl
    }
    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: ContractWireKeys.self)
        try container.encode(version, forKey: .version)
        try container.encode(mac, forKey: .mac)
        try container.encode(iosAppStoreUrl, forKey: .iosAppStoreUrl)
    }
}


extension StorageSettingsDTO {
    private enum ContractWireKeys: String, CodingKey {
        case backend
        case source
        case endpoint
        case region
        case bucket
        case prefix
        case forcePathStyle
        case accessKeyId
        case secretAccessKeySet
    }
    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: ContractWireKeys.self)
        try container.encode(backend, forKey: .backend)
        try container.encode(source, forKey: .source)
        try container.encode(endpoint, forKey: .endpoint)
        try container.encode(region, forKey: .region)
        try container.encode(bucket, forKey: .bucket)
        try container.encode(storageSettingsDTOPrefix, forKey: .prefix)
        try container.encode(forcePathStyle, forKey: .forcePathStyle)
        try container.encode(accessKeyId, forKey: .accessKeyId)
        try container.encode(secretAccessKeySet, forKey: .secretAccessKeySet)
    }
}


extension StorageTestDTO {
    private enum ContractWireKeys: String, CodingKey {
        case ok
        case backend
        case step
        case message
    }
    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: ContractWireKeys.self)
        try container.encode(ok, forKey: .ok)
        try container.encode(backend, forKey: .backend)
        try container.encode(step, forKey: .step)
        try container.encode(message, forKey: .message)
    }
}


extension AppsSettingsDTO {
    private enum ContractWireKeys: String, CodingKey {
        case iosAppIds
        case iosAppStoreId
    }
    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: ContractWireKeys.self)
        try container.encode(iosAppIds, forKey: .iosAppIds)
        try container.encode(iosAppStoreId, forKey: .iosAppStoreId)
    }
}


extension OnboardingDTO {
    private enum ContractWireKeys: String, CodingKey {
        case completedAt
        case steps
    }
    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: ContractWireKeys.self)
        try container.encode(completedAt, forKey: .completedAt)
        try container.encode(steps, forKey: .steps)
    }
}


extension GoogleVerificationStatusDTO {
    private enum ContractWireKeys: String, CodingKey {
        case state
        case reason
    }
    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: ContractWireKeys.self)
        try container.encode(state, forKey: .state)
        try container.encode(reason, forKey: .reason)
    }
}
