import Foundation
import SwiftUI
import Testing
@testable import Cereal

@MainActor @Suite struct HapticsTests {
    @Test func theVocabularyMapsOntoSwiftUIFeedback() {
        #expect(HapticFeedback.selection.sensory == .selection)
        #expect(HapticFeedback.success.sensory == .success)
        #expect(HapticFeedback.warning.sensory == .warning)
        #expect(HapticFeedback.error.sensory == .error)
        #expect(HapticFeedback.impact(.light, intensity: 0.8).sensory == .impact(weight: .light, intensity: 0.8))
        #expect(HapticFeedback.impact(.heavy).sensory == .impact(weight: .heavy))
        #expect(HapticFeedback.impact(.soft, intensity: 0.6).sensory == .impact(flexibility: .soft, intensity: 0.6))
        #expect(HapticFeedback.impact(.rigid).sensory == .impact(flexibility: .rigid))
        // The named moments: a destructive confirmation is firmer than a sent message.
        #expect(HapticFeedback.destructive == .impact(.rigid, intensity: 1))
        #expect(HapticFeedback.tap == .impact(.light, intensity: 0.8))
        #expect(HapticFeedback.done == .success)
    }

    @Test func hapticsPlayOnlyInFrontWhenAllowedAndNeverInTests() {
        #expect(HapticGate.allows(enabled: true, appActive: true, testing: false))
        #expect(!HapticGate.allows(enabled: false, appActive: true, testing: false))
        #expect(!HapticGate.allows(enabled: true, appActive: false, testing: false))
        #expect(!HapticGate.allows(enabled: true, appActive: true, testing: true))
        #expect(HapticGate.isTesting)
        #expect(!HapticGate.usesCoreHaptics(supportsHaptics: true, testing: true))
        #expect(!HapticGate.usesCoreHaptics(supportsHaptics: false, testing: false))
        let defaults = UserDefaults(suiteName: "haptics-tests-\(UUID().uuidString)")!
        #expect(HapticGate.isEnabled(defaults))
        defaults.set(false, forKey: HapticGate.storageKey)
        #expect(!HapticGate.isEnabled(defaults))
    }

    @Test func signaturesAreShortWellFormedAndBuild() throws {
        for signature in SignatureHaptic.allCases {
            let pattern = signature.pattern
            #expect(pattern.duration > 0 && pattern.duration < 0.7)
            #expect(pattern.events.map(\.time) == pattern.events.map(\.time).sorted())
            for event in pattern.events {
                #expect((0...1).contains(event.intensity) && (0...1).contains(event.sharpness))
            }
            #expect(pattern.intensityCurve.allSatisfy { (0...1).contains($0.value) })
            _ = try pattern.makePattern()
        }
        // Start: a soft breath (continuous, dull) that lands on a crisp tick.
        let start = SignatureHaptic.recordingStarted.pattern.events
        #expect(start.first?.kind == .continuous && start.last?.kind == .transient)
        #expect(start.last!.sharpness > start.first!.sharpness && start.last!.time >= start.first!.duration)
        // Notes ready: taps that rise.
        let ready = SignatureHaptic.notesReady.pattern.events.map(\.intensity)
        #expect(ready == ready.sorted() && Set(ready).count == ready.count)
        // End: firm, then softer and lower, like something closing.
        let end = SignatureHaptic.recordingEnded.pattern.events
        #expect(end.count == 2 && end[0].intensity > end[1].intensity && end[0].sharpness > end[1].sharpness)
        #expect(SignatureHaptic.notesReady.fallback == .success)
    }

    @Test func eachRecordingChangeHasItsOwnFeel() {
        #expect(RecordingFeedback.output(.started) == .signature(.recordingStarted))
        #expect(RecordingFeedback.output(.ended) == .signature(.recordingEnded))
        #expect(RecordingFeedback.output(.discarded) == .feedback(.destructive))
        #expect(RecordingFeedback.output(.paused) == .feedback(.impact(.soft, intensity: 0.7)))
        #expect(RecordingFeedback.output(.resumed) == .feedback(.impact(.solid, intensity: 0.9)))
        #expect(RecordingFeedback.output(.interrupted) == .feedback(.warning))
        #expect(RecordingFeedback.output(.inputLost) == .feedback(.warning))
        #expect(RecordingFeedback.output(.failed) == .feedback(.error))
        #expect(RecordingHaptic.stopped(by: .interruption) == .interrupted)
        #expect(RecordingHaptic.stopped(by: .inputLost) == .inputLost)
        #expect(RecordingHaptic.stopped(by: .failure) == .failed)
    }

    @Test func theRecorderPlaysOneFeelPerChangeAndNoneForAnInternalBoundary() async throws {
        let recorder = Recorder(simulateCapture: true)
        var felt: [RecordingHaptic] = []
        recorder.playHaptic = { felt.append($0) }
        let id = UUID()
        recorder.startSegment(noteID: id)
        recorder.startSegment(noteID: id) // already recording: nothing
        recorder.pause(); recorder.pause()
        recorder.resumeFromPause(); recorder.resumeFromPause()
        try await Task.sleep(for: .milliseconds(20))
        // A part ends and the next begins at once: the same recording, nothing felt.
        _ = recorder.stopSegment(keepingLiveActivity: true)
        recorder.startSegment(noteID: id)
        try await Task.sleep(for: .milliseconds(20))
        recorder.stopSegment(); recorder.stopSegment()
        #expect(felt == [.started, .paused, .resumed, .ended])
        recorder.startSegment(noteID: id)
        recorder.stopSegment(activity: .discard)
        #expect(felt.suffix(2) == [.started, .discarded])
    }

    @Test func aSidebarDragIsFeltOnlyWhenItSnapsToTheOtherSide() {
        #expect(SidebarModel.snapIsFelt(from: 0, open: true))
        #expect(SidebarModel.snapIsFelt(from: 1, open: false))
        #expect(!SidebarModel.snapIsFelt(from: 0, open: false)) // a peek let go
        #expect(!SidebarModel.snapIsFelt(from: 1, open: true))
    }
}
