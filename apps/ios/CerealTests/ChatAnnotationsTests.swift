import Foundation
import Testing
import Contracts
@testable import Cereal

@MainActor struct ChatAnnotationsTests {
    private let note = "00000000-0000-4000-8000-000000000001"
    private func citation(_ id: String = "S1", note: String? = nil) -> ChatCitationDTO {
        ChatCitationDTO(id: id, noteId: note ?? self.note, quote: "Release Friday", segmentId: nil, source: .notes, startMs: nil, title: "Actual returned meeting", transcriptRevision: 3)
    }
    private func tool(_ id: String, status: ChatToolProgressDTOStatus) -> ChatToolProgressDTO {
        ChatToolProgressDTO(callId: id, label: status == .running ? "Searching" : "Found meeting", name: .searchMeetings,
                            resultCount: status == .completed ? 1 : nil,
                            results: status == .completed ? [ChatToolMeetingDTO(date: "2026-11-13", noteId: note, title: "Actual returned meeting")] : nil, status: status)
    }
    @Test func toolUpdatesReplaceOnlyTheirCallAndPreserveActualReturnedMeetingMetadata() {
        let call = UUID().uuidString
        var annotations = ChatTurnAnnotations()
        annotations.apply(ChatStreamEventDTO(citations: nil, text: "", tool: tool(call, status: .running), type: .tool))
        let previous = annotations
        annotations.apply(ChatStreamEventDTO(citations: nil, text: "", tool: tool(call, status: .completed), type: .tool))
        #expect(annotations != previous)
        #expect(annotations.tools.count == 1 && annotations.tools[0].status == .completed)
        #expect(annotations.tools[0].results?.first?.date == "2026-11-13")
        #expect(annotations.tools[0].results?.first?.title == "Actual returned meeting")
    }
    @Test func onlyIssuedTurnBoundMarkersBecomeSourceLinksAndCodeRemainsLiteral() throws {
        let turn = UUID(), source = citation()
        let marked = ChatCitationMarkup.markdown("Friday [[S1]], not [[S9]]. `literal [[S1]]`", turnID: turn, citations: [source])
        let url = try #require(ChatCitationMarkup.url(id: "S1", turnID: turn))
        #expect(marked.contains(url.absoluteString))
        #expect(!marked.contains("[[S9]]"))
        #expect(marked.contains("`literal [[S1]]`"))
        #expect(ChatCitationMarkup.resolve(url, turnID: turn, citations: [source])?.id == "S1")
        #expect(ChatCitationMarkup.resolve(url, turnID: UUID(), citations: [source]) == nil)
        #expect(ChatCitationMarkup.resolve(url, turnID: turn, citations: []) == nil)
        #expect(ChatCitationMarkup.markdown("[[S1]]", turnID: nil, citations: []) == "[[S1]]")
        // A note's own chat shows no marks, and leaves no space where they were; neighbours stay apart.
        #expect(ChatCitationMarkup.withoutMarkers("Moved [[S25]][[S26]]. Later [[S3]]") == "Moved. Later")
        let pair = ChatCitationMarkup.markdown("Moved. [[S1]][[S1]]", turnID: turn, citations: [source])
        #expect(pair.hasPrefix("Moved. [\u{2007}1](") && pair.contains(") [\u{2007}1]("))
    }
    @Test func metadataIsPerAssistantTurnAndSurvivesProtectedHistoryWithoutCrossAccountLeak() throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: dir) }
        var first = ChatTurnAnnotations(), second = ChatTurnAnnotations()
        first.apply(ChatStreamEventDTO(citations: [citation()], text: "", tool: nil, type: .citations))
        second.apply(ChatStreamEventDTO(citations: [citation(note: UUID().uuidString)], text: "", tool: nil, type: .citations))
        let messages = [ChatMessage(role: .assistant, text: "First [[S1]]", annotations: first), ChatMessage(role: .assistant, text: "Second [[S1]]", annotations: second)]
        let url = dir.appendingPathComponent("history.json")
        ServerChatSessions(persistenceURL: url).save(messages: messages, history: [], for: "server|account-A|note")
        let sessions = ServerChatSessions(persistenceURL: url)
        let saved = sessions.load(for: "server|account-A|note").messages
        #expect(saved == messages)
        #expect(saved[0].annotations?.citations[0].noteId != saved[1].annotations?.citations[0].noteId)
        #expect(sessions.load(for: "server|account-B|note").messages.isEmpty)
    }
    @Test func interruptedToolOnlyReplyRemainsVisibleAndDoesNotRestartAsRunningAfterRelaunch() throws {
        var annotations = ChatTurnAnnotations()
        annotations.apply(ChatStreamEventDTO(citations: nil, text: "", tool: tool(UUID().uuidString, status: .running), type: .tool))
        let sessions = ServerChatSessions()
        sessions.save(messages: [ChatMessage(role: .user, text: "Find the meeting"), ChatMessage(role: .assistant, text: "", isStreaming: true, annotations: annotations)], history: [], for: "server|A|note")
        let saved = try #require(sessions.load(for: "server|A|note").messages.last)
        #expect(!saved.isStreaming && saved.isInterrupted)
        #expect(saved.text.isEmpty && saved.annotations?.tools.count == 1)
    }
    @Test func oldChatHistoryWithoutAnnotationsStillDecodes() throws {
        let id = UUID()
        let old = "{\"id\":\"\(id)\",\"role\":\"assistant\",\"text\":\"Historical answer\",\"isStreaming\":false,\"isInterrupted\":false}"
        let message = try JSONDecoder().decode(ChatMessage.self, from: Data(old.utf8))
        #expect(message.id == id && message.annotations == nil)
    }
    @Test func boundedToolMetadataDoesNotAccumulateUnboundedLookupRows() {
        var annotations = ChatTurnAnnotations()
        for _ in 0..<25 { annotations.apply(ChatStreamEventDTO(citations: nil, text: "", tool: tool(UUID().uuidString, status: .completed), type: .tool)) }
        #expect(annotations.tools.count == 20)
    }
    @Test func workspaceHistoryIsDistinctFromNoteAndOtherAccountScopes() throws {
        let id = UUID().uuidString
        let workspace = try #require(ChatHistoryScope.key(accountIdentity: "server|A", noteID: nil))
        let note = try #require(ChatHistoryScope.key(accountIdentity: "server|A", noteID: id))
        #expect(workspace != note)
        #expect(workspace != ChatHistoryScope.key(accountIdentity: "server|B", noteID: nil))
        #expect(ChatHistoryScope.key(accountIdentity: nil, noteID: nil) == nil)
        #expect(ChatHistoryScope.key(accountIdentity: "server|A", noteID: "workspace") == nil)
        let sessions = ServerChatSessions()
        sessions.save(messages: [ChatMessage(role: .user, text: "Recent todos")], history: [], for: workspace)
        #expect(sessions.load(for: note).messages.isEmpty)
        #expect(sessions.load(for: workspace).messages.first?.text == "Recent todos")
    }
    @Test func issuedMarkersProduceActualNumberedAttributedLinks() throws {
        let turn = UUID(), source = citation()
        let markdown = ChatCitationMarkup.markdown("Friday [[S1]].", turnID: turn, citations: [source])
        let rendered = try AttributedString(markdown: markdown, options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace))
        #expect(String(rendered.characters) == "Friday \u{2007}1.")
        #expect(rendered.runs.contains { $0.link == ChatCitationMarkup.url(id: "S1", turnID: turn) })
    }

    @Test func authoritativeCitationReplacementAndWithdrawalRemoveOldTurnLinks() throws {
        var annotations = ChatTurnAnnotations()
        let turn = UUID()
        annotations.apply(ChatStreamEventDTO(citations: [citation("S1")], text: "", tool: nil, type: .citations))
        let oldURL = try #require(ChatCitationMarkup.url(id: "S1", turnID: turn))
        annotations.apply(ChatStreamEventDTO(citations: [citation("S2")], text: "", tool: nil, type: .citations))
        #expect(annotations.citations.map(\.id) == ["S2"])
        #expect(ChatCitationMarkup.resolve(oldURL, turnID: turn, citations: annotations.citations) == nil)
        let beforeWithdrawal = annotations
        annotations.apply(ChatStreamEventDTO(citations: [], text: "", tool: nil, type: .citations))
        #expect(annotations != beforeWithdrawal && annotations.citations.isEmpty)
        #expect(ChatCitationMarkup.markdown("Claim [[S2]].", turnID: turn, citations: annotations.citations) == "Claim.")
    }

}
