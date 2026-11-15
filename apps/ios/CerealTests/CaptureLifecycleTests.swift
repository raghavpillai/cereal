import Foundation
import Testing
@testable import Cereal

@MainActor
struct CaptureLifecycleTests {
    private func makeStore() -> NoteStore {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("notes-\(UUID()).json")
        return NoteStore(fileURL: url)
    }

    private func segment(_ i: Int, _ d: TimeInterval = 30) -> CaptureSegment {
        CaptureSegment(index: i, startedAt: .now, duration: d, fileName: "segment-\(i).wav")
    }

    @Test func startStopGenerateResumeRegenerate() async {
        let store = makeStore()
        let id = store.createNote().id
        #expect(store.note(id)?.needsGeneration == false)

        // Stop = capture only; notes are not generated yet.
        store.finishCapture(id, segment: segment(0))
        #expect(store.note(id)?.transcriptRevision == 1)
        #expect(store.note(id)?.needsGeneration == true)
        #expect(store.note(id)?.generateActionTitle == "Generate notes")

        await store.generateNotes(id)
        #expect(store.note(id)?.notesGeneratedRevision == 1)
        #expect(store.note(id)?.needsGeneration == false)

        // Resume appends to the same transcript.
        store.finishCapture(id, segment: segment(1))
        let note = store.note(id)!
        #expect(note.captureSegments.map(\.index) == [0, 1])
        #expect(note.needsGeneration == true)
        #expect(note.generateActionTitle == "Regenerate notes")

        await store.generateNotes(id)
        #expect(store.note(id)?.notesGeneratedRevision == 2)
    }

    @Test func regenerateIsNoOpWhenRevisionUnchanged() async {
        let store = makeStore()
        let id = store.createNote().id
        store.finishCapture(id, segment: segment(0))
        await store.generateNotes(id)
        var edited = store.note(id)!
        edited.sections = [NoteSection(heading: "Hand edited", bullets: [])]
        store.update(edited)
        await store.generateNotes(id)
        #expect(store.note(id)?.sections.first?.heading == "Hand edited")
    }

    @Test func recorderResumesTimerAndSegmentIndex() {
        let recorder = Recorder()
        recorder.prepare(existing: [segment(0, 12), segment(1, 8)])
        #expect(recorder.elapsed == 20)
        #expect(recorder.state == .stopped)
    }

    @Test func emptyNotesAreDiscarded() {
        let store = makeStore()
        let id = store.createNote().id
        store.discardIfEmpty(id)
        #expect(store.note(id) == nil)
    }

    @Test func duplicateStopDoesNotAppendTheSameCaptureTwice() {
        let store = makeStore()
        let id = store.createNote().id
        let stopped = segment(0)
        store.finishCapture(id, segment: stopped)
        store.finishCapture(id, segment: stopped)
        #expect(store.note(id)?.captureSegments.count == 1)
        #expect(store.note(id)?.transcriptRevision == 1)
    }

    @Test func generationDoesNotApplyAfterAnotherCaptureStops() async {
        let store = makeStore()
        let id = store.createNote().id
        store.finishCapture(id, segment: segment(0))
        let generator = ControlledGenerator()
        let pending = Task { await store.generateNotes(id, using: generator) }
        await generator.waitUntilStarted()
        store.finishCapture(id, segment: segment(1))
        await generator.complete([NoteSection(heading: "Stale result", bullets: [])])
        await pending.value
        #expect(store.note(id)?.sections.isEmpty == true)
        #expect(store.note(id)?.notesGeneratedRevision == nil)
        #expect(store.note(id)?.transcriptRevision == 2)
    }

    @Test func generationPreservesManualEditsMadeWhileItWasPending() async {
        let store = makeStore()
        let id = store.createNote().id
        store.finishCapture(id, segment: segment(0))
        let generator = ControlledGenerator()
        let pending = Task { await store.generateNotes(id, using: generator) }
        await generator.waitUntilStarted()
        var edited = store.note(id)!
        edited.userNotes = "A correction entered during generation"
        edited.sections = [NoteSection(heading: "Hand edited", bullets: ["Keep this"])]
        store.update(edited)
        await generator.complete([NoteSection(heading: "Stale result", bullets: [])])
        await pending.value
        #expect(store.note(id)?.sections.first?.heading == "Hand edited")
        #expect(store.note(id)?.userNotes == "A correction entered during generation")
        #expect(store.note(id)?.notesGeneratedRevision == nil)
    }

    @Test func generationCannotModifyATrashedNote() async {
        let store = makeStore()
        let id = store.createNote().id
        store.finishCapture(id, segment: segment(0))
        let generator = ControlledGenerator()
        let pending = Task { await store.generateNotes(id, using: generator) }
        await generator.waitUntilStarted()
        store.trash(id)
        await generator.complete([NoteSection(heading: "Stale result", bullets: [])])
        await pending.value
        #expect(store.note(id)?.trashed == true)
        #expect(store.note(id)?.sections.isEmpty == true)
        #expect(store.note(id)?.notesGeneratedRevision == nil)
    }

    @Test func aNewerGenerationRequestSupersedesAnOlderOne() async {
        let store = makeStore()
        let id = store.createNote().id
        store.finishCapture(id, segment: segment(0))
        let first = ControlledGenerator(), second = ControlledGenerator()
        let old = Task { await store.generateNotes(id, using: first) }
        await first.waitUntilStarted()
        let new = Task { await store.generateNotes(id, using: second) }
        await second.waitUntilStarted()
        await second.complete([NoteSection(heading: "Latest result", bullets: [])])
        await new.value
        await first.complete([NoteSection(heading: "Old result", bullets: [])])
        await old.value
        #expect(store.note(id)?.sections.first?.heading == "Latest result")
        #expect(store.note(id)?.notesGeneratedRevision == 1)
    }

    @Test func legacyUntitledTranscriptSurvivesStoreReload() throws {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("legacy-\(UUID()).json")
        defer { try? FileManager.default.removeItem(at: url) }
        let id = UUID(), transcriptID = UUID()
        let json = """
        [{"id":"\(id)","title":"","createdAt":0,"transcript":[{"id":"\(transcriptID)","speaker":"You","startedAt":0,"text":"An existing thought"}]}]
        """
        try Data(json.utf8).write(to: url)
        let store = NoteStore(fileURL: url)
        #expect(store.note(id)?.transcript.first?.text == "An existing thought")
        #expect(store.note(id)?.sections.isEmpty == true)
        #expect(store.note(id)?.captureSegments.isEmpty == true)
    }

    @Test func staleEditorCannotEraseAStoppedCaptureSegment() {
        let store = makeStore()
        let id = store.createNote().id
        var stale = store.note(id)!
        store.finishCapture(id, segment: segment(0))
        stale.userNotes = "Typed while capture was finishing"
        store.update(stale)
        #expect(store.note(id)?.captureSegments.count == 1)
        #expect(store.note(id)?.transcriptRevision == 1)
        #expect(store.note(id)?.userNotes == "Typed while capture was finishing")
    }

    @Test func cancelledGenerationDoesNotWriteItsResult() async {
        let store = makeStore()
        let id = store.createNote().id
        store.finishCapture(id, segment: segment(0))
        let generator = ControlledGenerator()
        let pending = Task { await store.generateNotes(id, using: generator) }
        await generator.waitUntilStarted()
        pending.cancel()
        await generator.complete([NoteSection(heading: "Cancelled", bullets: [])])
        await pending.value
        #expect(store.note(id)?.sections.isEmpty == true)
        #expect(store.note(id)?.notesGeneratedRevision == nil)
    }

    @Test func resumingCaptureInvalidatesGenerationBeforeTheNextStop() async {
        let store = makeStore()
        let id = store.createNote().id
        store.finishCapture(id, segment: segment(0))
        let generator = ControlledGenerator()
        let pending = Task { await store.generateNotes(id, using: generator) }
        await generator.waitUntilStarted()
        store.invalidateGeneration(id)
        await generator.complete([NoteSection(heading: "Before resume", bullets: [])])
        await pending.value
        #expect(store.note(id)?.transcriptRevision == 1)
        #expect(store.note(id)?.sections.isEmpty == true)
        #expect(store.note(id)?.notesGeneratedRevision == nil)
    }

    @Test func malformedSavedNotesAreNotOverwrittenOnLoad() throws {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("recoverable-\(UUID()).json")
        defer { try? FileManager.default.removeItem(at: url) }
        let original = Data("[{\"title\":\"recoverable interrupted save\"".utf8)
        try original.write(to: url)
        let store = NoteStore(fileURL: url)
        let note = store.createNote()
        store.updateUserNotes(note.id, text: "An in-memory edit must not overwrite the recovery file")
        #expect(try Data(contentsOf: url) == original)
    }

    @Test func captureClockExcludesPausesAndTimeBetweenSegments() {
        var clock = RecordingClock()
        clock.prepare(durations: [12, 8])
        clock.start(at: 100)
        #expect(clock.elapsed(at: 105) == 25)
        clock.pause(at: 110)
        #expect(clock.elapsed(at: 1_000) == 30)
        clock.resume(at: 1_000)
        #expect(clock.stop(at: 1_002) == 12)
        #expect(clock.completed == 32)
        #expect(clock.stop(at: 2_000) == 0)
        #expect(clock.completed == 32)
        clock.start(at: 3_000)
        #expect(clock.elapsed(at: 3_003) == 35)
    }

    @Test func captureClockUsesAudioDurationAndRejectsInvalidSavedDurations() {
        var clock = RecordingClock()
        clock.prepare(durations: [.nan, .infinity, -4, 5])
        #expect(clock.completed == 5)
        clock.start(at: 100)
        #expect(clock.stop(at: 105, capturedDuration: 3) == 3)
        #expect(clock.completed == 8)
    }

    @Test func simulatedCaptureRequiresAnExplicitDebugSimulatorFlag() {
        #expect(!RecordingLaunchPolicy.requestsSimulation(arguments: [], debugSimulator: true))
        #expect(!RecordingLaunchPolicy.requestsSimulation(arguments: ["-simulate-capture"], debugSimulator: false))
        #expect(RecordingLaunchPolicy.requestsSimulation(arguments: ["-simulate-capture"], debugSimulator: true))
    }

    #if targetEnvironment(simulator)
    @Test func simulatedRecorderDeliversEachStoppedSegmentOnlyOnce() async throws {
        let recorder = Recorder(simulateCapture: true)
        var finished: [CaptureSegment] = []
        recorder.onSegmentFinished = { segment, _ in finished.append(segment) }
        recorder.prepare(existing: [segment(0, 12)])
        recorder.startSegment(noteID: UUID())
        #expect(recorder.state == .recording)
        try await Task.sleep(for: .milliseconds(40))
        recorder.pause()
        let paused = recorder.elapsed
        try await Task.sleep(for: .milliseconds(40))
        #expect(recorder.elapsed == paused)
        recorder.resumeFromPause()
        try await Task.sleep(for: .milliseconds(40))
        let stopped = recorder.stopSegment()
        #expect(stopped?.segment.index == 1)
        #expect(recorder.elapsed > 12)
        #expect(recorder.stopSegment() == nil)
        #expect(finished.count == 1)
        #expect(finished.first?.duration == stopped?.segment.duration)
    }
    #endif
}

/// Tests control completion order directly instead of depending on wall-clock delays.
private actor ControlledGenerator: NotesGenerator {
    private var result: CheckedContinuation<[NoteSection], Never>?
    private var started = false
    private var startWaiters: [CheckedContinuation<Void, Never>] = []

    func generate(for note: Note) async -> [NoteSection] {
        await withCheckedContinuation { continuation in
            result = continuation
            started = true
            startWaiters.forEach { $0.resume() }
            startWaiters.removeAll()
        }
    }

    func waitUntilStarted() async {
        if started { return }
        await withCheckedContinuation { startWaiters.append($0) }
    }

    func complete(_ sections: [NoteSection]) {
        result?.resume(returning: sections)
        result = nil
    }
}
