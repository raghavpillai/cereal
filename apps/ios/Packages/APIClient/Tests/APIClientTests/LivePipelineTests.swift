import Foundation
import Testing
import Contracts
import APIClient

/// Explicit opt-in only. These tests upload caller-selected synthetic fixture files,
/// never the local demo store, microphone history, credentials, or user documents.
@Suite struct LivePipelineTests {
    @Test(.enabled(if: ProcessInfo.processInfo.environment["RUN_LIVE_PROVIDER_TESTS"] == "1"))
    func deployedTranscriptionGenerationChatAndResumeUseActualSwiftTransport() async throws {
        let env = ProcessInfo.processInfo.environment
        let url = try #require(env["CEREAL_QA_INSTANCE_URL"])
        let token = try #require(env["CEREAL_QA_OWNER_TOKEN"])
        let firstFile = try #require(env["CEREAL_QA_AUDIO_PATH"])
        let secondFile = try #require(env["CEREAL_QA_RESUME_AUDIO_PATH"])
        let credentials = try InstanceCredentials(instance: InstanceURL(url), bearerToken: token)
        let api = MeetingNotesAPI(credentials: credentials)
        let note = try await api.createNote(CreateNoteInput(body: "Synthetic Swift API integration fixture", date: nil, duration: nil,
            enhanced: nil, title: "Swift provider pipeline verification", transcriptSegments: nil))
        do {
            let started = try await api.recording(noteID: note.id, command: .start)
            #expect(started.recordingState == .recording)
            _ = try await api.recording(noteID: note.id, command: .stop)
            let chunkID = UUID().uuidString.lowercased()
            let data = try Data(contentsOf: URL(fileURLWithPath: firstFile))
            let receipt = try await api.uploadAudio(noteID: note.id, chunkID: chunkID, channel: .mic, sequence: 0, startMs: 0, wav: data)
            #expect(receipt.id == chunkID)
            // The same immutable chunk ID may safely be retried after an ambiguous network result.
            let duplicate = try await api.uploadAudio(noteID: note.id, chunkID: chunkID, channel: .mic, sequence: 0, startMs: 0, wav: data)
            #expect(duplicate.id == receipt.id)
            let transcribed = try await waitForTranscript(api: api, noteID: note.id)
            #expect(!transcribed.transcriptSegments.isEmpty)
            #expect(transcribed.transcriptRevision > 0)
            let generated = try await api.generate(noteID: note.id, expectedTranscriptRevision: transcribed.transcriptRevision)
            #expect(!generated.enhanced.isEmpty)
            #expect(generated.generatedRevision == transcribed.transcriptRevision)
            let answer = try await api.chat(noteID: note.id, message: "What action items were discussed?", history: [])
            #expect(!answer.answer.isEmpty)
            let resume = try await api.recording(noteID: note.id, command: .resume)
            #expect(resume.id == note.id)
            #expect(resume.recordingState == .recording)
            _ = try await api.recording(noteID: note.id, command: .stop)
            let offset = max(1, receipt.startMs + receipt.durationMs)
            _ = try await api.uploadAudio(noteID: note.id, chunkID: UUID().uuidString.lowercased(), channel: .mic,
                sequence: 1, startMs: offset, wav: Data(contentsOf: URL(fileURLWithPath: secondFile)))
            let appended = try await waitForTranscript(api: api, noteID: note.id)
            #expect(appended.id == note.id)
            #expect(appended.transcriptRevision > transcribed.transcriptRevision)
            #expect(appended.generatedRevision != appended.transcriptRevision)
            #expect(appended.transcriptSegments.count > transcribed.transcriptSegments.count)
            let regenerated = try await api.generate(noteID: note.id, expectedTranscriptRevision: appended.transcriptRevision)
            #expect(regenerated.generatedRevision == appended.transcriptRevision)
            #expect(!regenerated.enhanced.isEmpty)
            let chunks = try await api.audioChunks(noteID: note.id)
            #expect(chunks.count == 2)
            #expect(chunks.allSatisfy { $0.state == .ready })
            _ = try await api.deleteNote(id: note.id)
        } catch {
            _ = try? await api.deleteNote(id: note.id)
            throw error
        }
    }

    private func waitForTranscript(api: MeetingNotesAPI, noteID: String) async throws -> NoteDTO {
        for _ in 0..<180 {
            let chunks = try await api.audioChunks(noteID: noteID)
            if chunks.contains(where: { $0.state == .failed }) { throw APIClientError.unavailable }
            if !chunks.isEmpty, chunks.allSatisfy({ $0.state == .ready }) { return try await api.note(id: noteID) }
            try await Task.sleep(for: .seconds(1))
        }
        throw APIClientError.transport(.timedOut)
    }
}
