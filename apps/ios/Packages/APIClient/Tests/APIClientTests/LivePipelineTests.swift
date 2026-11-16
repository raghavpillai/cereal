import Foundation
import Testing
import Contracts
import APIClient

/// Explicit opt-in only. These tests stream caller-selected synthetic fixture files,
/// never the local demo store, microphone history, credentials, or user documents.
/// The fixtures are 16 kHz mono PCM16 WAV files.
@Suite struct LivePipelineTests {
    @Test(.enabled(if: ProcessInfo.processInfo.environment["RUN_LIVE_PROVIDER_TESTS"] == "1"))
    func deployedLiveTranscriptionGenerationChatAndResumeUseActualSwiftTransport() async throws {
        let env = ProcessInfo.processInfo.environment
        let url = try #require(env["CEREAL_QA_INSTANCE_URL"])
        let token = try #require(env["CEREAL_QA_OWNER_TOKEN"])
        let firstFile = try #require(env["CEREAL_QA_AUDIO_PATH"])
        let secondFile = try #require(env["CEREAL_QA_RESUME_AUDIO_PATH"])
        let credentials = try InstanceCredentials(instance: InstanceURL(url), bearerToken: token)
        let api = MeetingNotesAPI(credentials: credentials)
        let note = try await api.createNote(CreateNoteInput(body: "Synthetic Swift API integration fixture", date: nil, duration: nil,
            enhanced: nil, title: "Swift provider pipeline verification", transcriptSegments: nil))
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("cereal-live-pipeline-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        do {
            let started = try await api.recording(noteID: note.id, command: .start)
            #expect(started.recordingState == .recording)
            let firstMs = try await stream(firstFile, api: api, noteID: note.id, capture: root.appendingPathComponent("capture-0.wav"), baseMs: 0)
            // Stop answers once the live session has stored its last words; notes can be written at once.
            let transcribed = try await api.recording(noteID: note.id, command: .stop)
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
            _ = try await stream(secondFile, api: api, noteID: note.id, capture: root.appendingPathComponent("capture-1.wav"), baseMs: firstMs)
            let appended = try await api.recording(noteID: note.id, command: .stop)
            #expect(appended.transcriptRevision > transcribed.transcriptRevision)
            #expect(appended.generatedRevision != appended.transcriptRevision)
            #expect(appended.transcriptSegments.count > transcribed.transcriptSegments.count)
            let regenerated = try await api.generate(noteID: note.id, expectedTranscriptRevision: appended.transcriptRevision)
            #expect(regenerated.generatedRevision == appended.transcriptRevision)
            #expect(!regenerated.enhanced.isEmpty)
            _ = try await api.deleteNote(id: note.id)
        } catch {
            _ = try? await api.deleteNote(id: note.id)
            throw error
        }
    }

    /// Records the fixture into a capture at twice real time while the live
    /// client streams it, then ends the stream. Returns the audio's length in ms.
    private func stream(_ path: String, api: MeetingNotesAPI, noteID: String, capture: URL, baseMs: Int) async throws -> Int {
        let pcm = try Self.pcm16kMono(URL(fileURLWithPath: path))
        let writer = try CaptureFileWriter(url: capture)
        let client = try api.liveTranscriptClient(noteID: noteID, source: CaptureFileReader(url: capture),
            configuration: LiveTranscriptClient.Configuration(captureID: "\(noteID)/\(capture.lastPathComponent)", baseMs: baseMs))
        await client.start()
        let step = 3_200 // 100 ms
        var offset = 0
        while offset < pcm.count {
            let end = min(pcm.count, offset + step * ContinuousCapture.bytesPerFrame)
            try pcm[offset..<end].withUnsafeBytes { try writer.append($0) }
            offset = end
            try await Task.sleep(for: .milliseconds(50))
        }
        try writer.close()
        await client.end(atSample: writer.frames)
        return Int(writer.frames * 1000 / Int64(ContinuousCapture.sampleRate))
    }

    /// The samples of a 16 kHz mono PCM16 WAV file.
    private static func pcm16kMono(_ url: URL) throws -> Data {
        let data = try Data(contentsOf: url)
        func le32(_ at: Int) -> Int { Int(data[at]) | Int(data[at + 1]) << 8 | Int(data[at + 2]) << 16 | Int(data[at + 3]) << 24 }
        func le16(_ at: Int) -> Int { Int(data[at]) | Int(data[at + 1]) << 8 }
        try #require(data.count >= 12 && data.prefix(4) == Data("RIFF".utf8) && data[8..<12] == Data("WAVE".utf8))
        var offset = 12
        var format = false
        while offset + 8 <= data.count {
            let id = String(decoding: data[offset..<(offset + 4)], as: UTF8.self), length = le32(offset + 4)
            if id == "fmt " {
                format = le16(offset + 8) == 1 && le16(offset + 10) == 1 && le32(offset + 12) == 16_000 && le16(offset + 22) == 16
            } else if id == "data" {
                try #require(format, "The fixture must be 16 kHz mono PCM16")
                return Data(data[(offset + 8)..<min(data.count, offset + 8 + length)])
            }
            offset += 8 + length + length % 2
        }
        Issue.record("The fixture has no audio")
        throw CancellationError()
    }
}
