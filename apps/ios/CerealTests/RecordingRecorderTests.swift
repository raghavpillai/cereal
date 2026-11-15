import AVFoundation
import Foundation
import Testing
@testable import Cereal

@MainActor
@Suite(.serialized)
struct RecordingRecorderTests {
    #if DEBUG && targetEnvironment(simulator)
    @Test func stopIsIdempotentAndResumeKeepsSameSession() async throws {
        let recorder = Recorder(simulateCapture: true)
        let id = UUID()
        recorder.prepare(existing: [CaptureSegment(index: 0, startedAt: .now, duration: 20, fileName: "existing.wav")])
        var finished: [CaptureSegment] = []
        recorder.onSegmentFinished = { segment, _ in finished.append(segment) }
        recorder.startSegment(noteID: id)
        try await Task.sleep(for: .milliseconds(25))
        let first = try #require(recorder.stopSegment())
        #expect(first.segment.index == 1)
        #expect(recorder.stopSegment() == nil)
        #expect(finished.count == 1)
        #expect(recorder.elapsed >= 20)
        recorder.startSegment(noteID: id)
        try await Task.sleep(for: .milliseconds(25))
        recorder.stopSegment()
        #expect(finished.map(\.index) == [1, 2])
        #expect(recorder.levels == Array(repeating: Float(0.12), count: 5))
    }

    @Test func internalChunkBoundaryFinalizesOnceAndKeepsCumulativeCapture() async throws {
        let recorder = Recorder(simulateCapture: true)
        let id = UUID()
        var finished: [CaptureSegment] = []
        recorder.onSegmentFinished = { segment, _ in finished.append(segment) }
        recorder.startSegment(noteID: id)
        try await Task.sleep(for: .milliseconds(25))
        let first = try #require(recorder.stopSegment(keepingLiveActivity: true))
        #expect(recorder.stopSegment(keepingLiveActivity: true) == nil)
        recorder.startSegment(noteID: id)
        try await Task.sleep(for: .milliseconds(25))
        let second = try #require(recorder.stopSegment())
        #expect(finished.map(\.index) == [0, 1])
        #expect(recorder.elapsed >= first.segment.duration + second.segment.duration)
        #expect(recorder.state == .stopped)
    }

    @Test func interruptionFinalizesOnceAndRequiresExplicitResume() async throws {
        let recorder = Recorder(simulateCapture: true)
        var finishes = 0
        recorder.onSegmentFinished = { _, _ in finishes += 1 }
        recorder.startSegment(noteID: UUID())
        try await Task.sleep(for: .milliseconds(25))
        NotificationCenter.default.post(name: AVAudioSession.interruptionNotification, object: nil,
            userInfo: [AVAudioSessionInterruptionTypeKey: AVAudioSession.InterruptionType.began.rawValue])
        #expect(!recorder.isCapturing)
        #expect(finishes == 1)
        #expect(recorder.stopSegment() == nil)
        NotificationCenter.default.post(name: AVAudioSession.interruptionNotification, object: nil,
            userInfo: [AVAudioSessionInterruptionTypeKey: AVAudioSession.InterruptionType.ended.rawValue])
        #expect(!recorder.isCapturing)
    }
    #endif
}
