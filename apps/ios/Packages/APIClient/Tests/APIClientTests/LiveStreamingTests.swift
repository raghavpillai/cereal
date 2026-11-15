import Foundation
import Testing
import Contracts
import APIClient

/// Explicit opt-in against the actual deployed SSE/provider path. All context
/// is fictional typed text; no microphone history or private note is uploaded.
@Suite struct LiveStreamingTests {
    @Test(.enabled(if: ProcessInfo.processInfo.environment["RUN_LIVE_STREAM_TESTS"] == "1"))
    func realDeltasAndCancellationUseTheSwiftStreamingTransport() async throws {
        let env = ProcessInfo.processInfo.environment
        let url = try #require(env["CEREAL_QA_INSTANCE_URL"])
        let token = try #require(env["CEREAL_QA_OWNER_TOKEN"])
        let api = try MeetingNotesAPI(credentials: InstanceCredentials(instance: InstanceURL(url), bearerToken: token))
        let note = try await api.createNote(CreateNoteInput(body: "Synthetic test: Cedar launches on Monday. Alex owns mobile recording. Jamie owns the transcription pipeline. The budget remains three thousand dollars.", date: nil, duration: nil, enhanced: nil, title: "Swift SSE verification fixture", transcriptSegments: nil))
        do {
            var deltas: [String] = []
            for try await text in try api.chatStream(noteID: note.id, message: "What day does Cedar launch, and who owns mobile recording?", history: []) { deltas.append(text) }
            #expect(deltas.count > 1)
            #expect(deltas.joined().localizedCaseInsensitiveContains("monday"))
            #expect(deltas.joined().localizedCaseInsensitiveContains("alex"))

            let started = AsyncStream<Void>.makeStream()
            let stream = try api.chatStream(noteID: note.id, message: "Write 25 numbered review questions about the supplied launch plan, responsibilities and budget, using only the given facts.", history: [])
            let consumer = Task {
                defer { started.continuation.finish() }
                var signaled = false
                for try await _ in stream {
                    if !signaled { started.continuation.yield(()); started.continuation.finish(); signaled = true }
                }
            }
            var ready = started.stream.makeAsyncIterator()
            let firstDelta = await ready.next()
            try #require(firstDelta != nil)
            let cancellation = ContinuousClock.now
            consumer.cancel()
            _ = await consumer.result
            #expect(cancellation.duration(to: .now) < .seconds(2))
            #expect(consumer.isCancelled)
            _ = try await api.deleteNote(id: note.id)
            print("Swift live SSE checks passed: multiple actual deltas, grounded answer, consumer cancellation under two seconds")
        } catch {
            _ = try? await api.deleteNote(id: note.id)
            throw error
        }
    }
}
