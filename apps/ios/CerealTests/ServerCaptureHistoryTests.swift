import Foundation
import Testing
import Contracts
@testable import Cereal

struct ServerCaptureHistoryTests {
    @Test func anotherDeviceAdvancesOnlyTheNextCaptureWithoutRewritingAcceptedOffsets() throws {
        let earlier = CaptureSegment(index: 0, startedAt: .now, duration: 2, fileName: "earlier.wav")
        var history = ServerCaptureHistory(baseOffsetMs: 1000, nextSequence: 1, segments: [earlier],
            segmentOffsets: ["earlier.wav": 1000], nextStartMs: 3000, needsStop: false)
        let remote = AudioChunkDTO(channel: .mic, durationMs: 12000, error: nil, id: UUID().uuidString,
            noteId: UUID().uuidString, sequence: 8, startMs: 18000, state: .ready)
        history.prepareForResume(remoteChunks: [remote], remoteDurationMs: 29000, localEndMs: 3000)
        #expect(history.nextSequence == 9)
        #expect(history.nextStartMs == 30000)
        #expect(history.segmentOffsets?[earlier.fileName] == 1000)
        #expect(history.needsStop == true)
        let appended = CaptureSegment(index: 1, startedAt: .now, duration: 1.25, fileName: "resumed.wav")
        history.append(appended)
        #expect(history.segmentOffsets?[appended.fileName] == 30000)
        #expect(history.nextStartMs == 31250)
        history.append(appended)
        #expect(history.segments.count == 2)
        #expect(history.nextStartMs == 31250)
        let restored = try JSONDecoder().decode(ServerCaptureHistory.self, from: JSONEncoder().encode(history))
        #expect(restored.needsStop == true)
        #expect(restored.segmentOffsets == history.segmentOffsets)
    }

    @Test func remoteSystemAudioAdvancesTimelineWithoutConsumingMicrophoneSequence() {
        var history = ServerCaptureHistory(baseOffsetMs: 0, nextSequence: 2, segments: [])
        let remote = AudioChunkDTO(channel: .system, durationMs: 20001, error: nil, id: UUID().uuidString,
            noteId: UUID().uuidString, sequence: 100, startMs: 999, state: .processing)
        history.prepareForResume(remoteChunks: [remote], remoteDurationMs: 20000, localEndMs: 0)
        #expect(history.nextStartMs == 21000)
        #expect(history.nextSequence == 2)
    }

    @Test func olderCaptureLedgerDecodesWithoutInventingStopOrChangingBaseline() throws {
        let old = Data(#"{"baseOffsetMs":1250,"nextSequence":0,"segments":[]}"#.utf8)
        var restored = try JSONDecoder().decode(ServerCaptureHistory.self, from: old)
        #expect(restored.needsStop == nil)
        restored.append(CaptureSegment(index: 0, startedAt: .now, duration: 2, fileName: "first.wav"))
        #expect(restored.segmentOffsets?["first.wav"] == 1250)
        #expect(restored.nextStartMs == 3250)
    }

    @MainActor @Test func serverRecordingFilesStayOutsideTheDemoRecoveryRoot() throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("server-capture-isolation-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: root) }
        let id = UUID()
        let file = try Recorder.fileURL(noteID: id, index: 0, root: root)
        #expect(file.deletingLastPathComponent().deletingLastPathComponent().path == root.path)
        #expect(!CaptureRecovery.defaultRoots.contains(file.deletingLastPathComponent().deletingLastPathComponent()))
        try Data("unfinished source".utf8).write(to: file)
        let next = try Recorder.fileURL(noteID: id, index: 0, root: root)
        #expect(next != file)
        #expect(try Data(contentsOf: file) == Data("unfinished source".utf8))
    }
}
