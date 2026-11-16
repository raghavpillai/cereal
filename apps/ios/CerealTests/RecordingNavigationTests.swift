import AVFoundation
import Foundation
import Observation
import SwiftUI
import Testing
import UIKit
import APIClient
import Contracts
@testable import Cereal

/// Where the screens around a recording lead, and what never ends one.
struct CaptureNavigationRuleTests {
    private static let account = AccountID(address: "https://example.test", userID: "00000000-0000-4000-8000-000000000001")
    private let note = NoteRef(account: account, id: "3a49a4af-c660-42e9-a36a-a806fbb41e0d")
    private let other = NoteRef(account: account, id: "99ad1171-3771-4ae6-81a1-44f617b663db")
    /// The same note id on another server is another note.
    private let twin = NoteRef(account: AccountID(address: "https://other.example.test", userID: "00000000-0000-4000-8000-000000000001"), id: "3a49a4af-c660-42e9-a36a-a806fbb41e0d")
    private var chat: Route { .serverChat(note.account, noteID: note.id) }

    @Test func askingFromTheRecordingScreenOpensTheNotesChatWithTheNoteBehindIt() {
        // From Home: Back from the chat lands on the note being recorded, where the indicator is.
        #expect(CaptureNavigation.pathForChat(from: [], note: note) == [.serverNote(note), chat])
        // From the note itself: nothing is pushed twice.
        #expect(CaptureNavigation.pathForChat(from: [.serverNote(note)], note: note) == [.serverNote(note), chat])
        // Already in that chat (asked, went back to the recording, asked again).
        #expect(CaptureNavigation.pathForChat(from: [.serverNote(note), chat], note: note) == [.serverNote(note), chat])
        // From somewhere else the user was browsing: their place is kept underneath.
        #expect(CaptureNavigation.pathForChat(from: [.search, .serverNote(other)], note: note) == [.search, .serverNote(other), .serverNote(note), chat])
    }

    @Test func closingTheRecordingScreenReturnsToWhereTheUserWasWhileRecordingAndToTheNoteOnceStopped() {
        // Still recording: closing changes nothing but the screen on top.
        #expect(CaptureNavigation.pathAfterClosing(from: [], note: note, recording: true) == [])
        #expect(CaptureNavigation.pathAfterClosing(from: [.search], note: note, recording: true) == [.search])
        #expect(CaptureNavigation.pathAfterClosing(from: [.serverNote(note)], note: note, recording: true) == [.serverNote(note)])
        // Stopped: the note is where its notes are generated. It is never stacked on itself.
        #expect(CaptureNavigation.pathAfterClosing(from: [], note: note, recording: false) == [.serverNote(note)])
        #expect(CaptureNavigation.pathAfterClosing(from: [.serverNote(note)], note: note, recording: false) == [.serverNote(note)])
    }

    @Test func theNotePageReadsItsNoteAgainWhenTheRecordingScreenOverItCloses() {
        // Notes generated on the recording screen: the page behind it must not keep offering "Generate notes".
        #expect(CaptureNavigation.noteReloads(note: note, captureWas: note, captureIs: nil))
        // Another note's recording screen closing, or this one opening, is not a reason to read again.
        #expect(!CaptureNavigation.noteReloads(note: note, captureWas: other, captureIs: nil))
        #expect(!CaptureNavigation.noteReloads(note: note, captureWas: nil, captureIs: note))
        #expect(!CaptureNavigation.noteReloads(note: note, captureWas: note, captureIs: note))
        // Nor is the recording screen of a note with the same id in another account.
        #expect(!CaptureNavigation.noteReloads(note: note, captureWas: twin, captureIs: nil))
        #expect(!CaptureNavigation.noteReloads(note: nil, captureWas: nil, captureIs: nil))
    }

    @Test func theIndicatorStaysWhileThereIsARecordingToReturnTo() {
        #expect(CaptureNavigation.showsIndicator(activeNoteID: note.id, state: .recording))
        #expect(CaptureNavigation.showsIndicator(activeNoteID: note.id, state: .paused))
        // Ended by a call or a lost input: the way back to the screen that explains it and resumes.
        #expect(CaptureNavigation.showsIndicator(activeNoteID: note.id, state: .failed("Recording was interrupted.")))
        // The user stopped it, or nothing was ever started.
        #expect(!CaptureNavigation.showsIndicator(activeNoteID: note.id, state: .stopped))
        #expect(!CaptureNavigation.showsIndicator(activeNoteID: note.id, state: .idle))
        #expect(!CaptureNavigation.showsIndicator(activeNoteID: nil, state: .recording))
    }

    @Test func anEdgeSwipeOrADragDownFromTheTitleLeavesTheRecordingScreenAndNothingElseDoes() {
        func leaves(_ x: CGFloat, _ y: CGFloat, _ dx: CGFloat, _ dy: CGFloat, rtl: Bool = false, keyboard: Bool = false) -> Bool {
            CaptureLeaveGesture.leaves(start: CGPoint(x: x, y: y), translation: CGSize(width: dx, height: dy), width: 402, rightToLeft: rtl, keyboardUp: keyboard)
        }
        // In from the leading edge, mostly horizontal.
        #expect(leaves(4, 400, 180, 10))
        #expect(!leaves(4, 400, 30, 0))       // too short
        #expect(!leaves(4, 400, 60, 90))      // a vertical drag that began at the edge is scrolling
        #expect(!leaves(120, 400, 200, 0))    // not from the edge: selecting text, moving the caret
        #expect(leaves(398, 400, -180, 0, rtl: true))
        #expect(!leaves(4, 400, 180, 0, rtl: true))
        // Down from the title.
        #expect(leaves(200, 40, 6, 200))
        #expect(!leaves(200, 300, 0, 300))    // in the notes: that is scrolling
        #expect(!leaves(200, 40, 80, 100))    // diagonal
        #expect(!leaves(200, 40, 0, 200, keyboard: true)) // the keyboard goes away first
    }

    @Test func theRecordingScreenGrowsOutOfItsControlOnlyWhereThatControlIs() {
        #expect(CaptureNavigation.zoomsFromSource(path: [], note: note))
        #expect(CaptureNavigation.zoomsFromSource(path: [.serverNote(note)], note: note))
        #expect(!CaptureNavigation.zoomsFromSource(path: [.serverNote(other)], note: note))
        #expect(!CaptureNavigation.zoomsFromSource(path: [.serverNote(note), chat], note: note))
        #expect(!CaptureNavigation.zoomsFromSource(path: [.serverNote(twin)], note: note))
    }
}

/// The recording screen can be shown and taken away; the recording is not its property.
private struct CaptureHost: View {
    let note: NoteRef
    @Bindable var router: Router
    var body: some View {
        ZStack {
            Color.clear
            if router.serverCapture != nil {
                RailMetricsReader { NavigationStack { ServerCaptureView(noteID: note.id).environment(\.account, note.account) } }
            }
        }
    }
}

/// Stands for Home: it shows the indicator and must not follow the recorder's clock.
private struct IndicatorHost: View {
    @Environment(ServerRecordingController.self) private var recording
    @Environment(Router.self) private var router
    var body: some View {
        let _ = PerfProbe.hit("Test.IndicatorHost.body")
        VStack {
            Text(recording.indicator == nil ? "No recording" : "Recording")
            if let ref = recording.indicator {
                RecordingPill(recorder: recording.recorder, onStop: {}, onStart: {},
                              onOpen: { router.serverCapture = ServerCaptureTarget(ref) })
            }
        }
    }
}

@MainActor
@Suite(.serialized)
struct RecordingNavigationTests {
    private func window(_ view: some View) throws -> UIWindow {
        let scene = try #require(UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }.first)
        let window = UIWindow(windowScene: scene)
        window.rootViewController = UIHostingController(rootView: view)
        window.isHidden = false
        return window
    }

    @Test func theRecordingSurvivesLeavingItsScreenAndEndsOnlyOnAnExplicitStop() async throws {
        let harness = try await LiveSessionHarness(framesPerUtterance: 10)
        defer { try? FileManager.default.removeItem(at: harness.root) }
        let router = Router()
        let noteID = harness.instance.noteID
        let note = NoteRef(account: try #require(harness.server.accountID), id: noteID)
        // Opening the screen starts the recording, as + on Home does.
        router.serverCapture = ServerCaptureTarget(note)
        let window = try window(CaptureHost(note: note, router: router)
            .environment(harness.server).environment(harness.controller).environment(router))
        defer { window.isHidden = true }
        try await waitFor("recording", timeout: .seconds(10)) { harness.controller.recorder.state == .recording }
        var time = harness.input.feed(format: InjectedAudio.builtIn, from: 0, seconds: 1)
        #expect(harness.controller.indicator?.id == noteID)
        let transcript = try #require(harness.controller.transcript)
        try await waitFor("live text") { !transcript.utterances.isEmpty }

        // The screen goes away: closed, swiped down, or left for the chat. All of them are this.
        router.serverCapture = nil
        try await Task.sleep(for: .milliseconds(400))
        #expect(harness.controller.recorder.state == .recording)
        #expect(harness.controller.indicator?.id == noteID)
        #expect(harness.instance.count("POST", suffix: "/recording") == 1) // only the Start

        // It is still a recording in every sense: audio is captured and streamed, and the
        // live transcript grows while no recording screen exists.
        let framesBefore = harness.controller.recorder.capturedFrames
        let finalsBefore = transcript.segments.count
        for _ in 0..<4 {
            time = harness.input.feed(format: InjectedAudio.builtIn, from: time, seconds: 1)
            try await Task.sleep(for: .milliseconds(120))
        }
        #expect(harness.controller.recorder.capturedFrames >= framesBefore + 4 * 16_000 - 400)
        try await waitFor("finals while away") { transcript.segments.count > finalsBefore }
        #expect(abs(harness.controller.recorder.elapsed - 5) < 0.2)

        // Returning does not restart anything: same capture, same clock.
        let file = harness.controller.recorder.currentFileURL
        router.serverCapture = ServerCaptureTarget(note)
        try await Task.sleep(for: .milliseconds(500))
        #expect(harness.controller.recorder.state == .recording)
        #expect(harness.controller.recorder.currentFileURL == file)
        #expect(harness.instance.count("POST", suffix: "/recording") == 1)
        router.serverCapture = nil
        try await Task.sleep(for: .milliseconds(300))
        #expect(harness.controller.recorder.state == .recording)

        // Stop is the one thing that ends it.
        harness.controller.stop()
        #expect(harness.controller.recorder.state == .stopped)
        #expect(harness.controller.indicator?.id == nil)
        try await waitFor("Stop on the server") { harness.instance.count("POST", suffix: "/recording") == 2 }
        #expect(harness.controller.recorder.capturedFrames == 5 * 16_000)
    }

    @Test func pauseAndAnInterruptionKeepTheIndicatorAndTheWayBack() async throws {
        let harness = try await LiveSessionHarness()
        defer { try? FileManager.default.removeItem(at: harness.root) }
        #expect(harness.controller.indicator?.id == nil)
        await harness.controller.start(note: try await harness.note())
        harness.input.feed(format: InjectedAudio.builtIn, from: 0, seconds: 1)
        #expect(harness.controller.indicator?.id == harness.instance.noteID)
        harness.controller.recorder.pause()
        #expect(harness.controller.indicator?.id == harness.instance.noteID)
        harness.controller.recorder.resumeFromPause()
        // A phone call ends the capture while the user is on another screen.
        NotificationCenter.default.post(name: AVAudioSession.interruptionNotification, object: nil,
            userInfo: [AVAudioSessionInterruptionTypeKey: AVAudioSession.InterruptionType.began.rawValue])
        #expect(!harness.controller.isCapturing)
        // The indicator is still there, so the interruption is not silent and one tap returns.
        #expect(harness.controller.indicator?.id == harness.instance.noteID)
        try await waitFor("Stop on the server") { harness.instance.count("POST", suffix: "/recording") == 2 }
        // Returning to the recording screen is the tap that resumes it.
        await harness.controller.start(note: try await harness.note())
        #expect(harness.controller.recorder.state == .recording)
        harness.controller.stop()
        #expect(harness.controller.indicator?.id == nil)
    }

    @Test func theIndicatorOnAnotherScreenFollowsTheClockInItsLeavesOnly() async throws {
        let harness = try await LiveSessionHarness()
        defer { try? FileManager.default.removeItem(at: harness.root) }
        let router = Router()
        let window = try window(IndicatorHost().environment(harness.controller).environment(router))
        defer { window.isHidden = true }
        await harness.controller.start(note: try await harness.note())
        var time = harness.input.feed(format: InjectedAudio.builtIn, from: 0, seconds: 1)
        try await Task.sleep(for: .milliseconds(400))
        PerfProbe.reset()
        for _ in 0..<10 {
            time = harness.input.feed(format: InjectedAudio.builtIn, from: time, seconds: 1)
            try await Task.sleep(for: .milliseconds(200))
        }
        let host = PerfProbe.count("Test.IndicatorHost.body")
        let pill = PerfProbe.count("RecordingPill.body")
        let timers = PerfProbe.count("RecorderTimer.body")
        // Ten seconds of audio and live text arrived in ten steps: the clock moved in its leaf each time...
        #expect(timers >= 8)
        // ...and neither the screen holding the indicator nor the pill itself was rebuilt.
        #expect(host == 0)
        #expect(pill == 0)
        // Tapping it asks for the recording screen and nothing else.
        #expect(router.serverCapture == nil)
        harness.controller.stop()
    }

    @Test func aPrivateNoteReadChangesNothingTheListScreensObserve() async throws {
        let harness = try await LiveSessionHarness()
        defer { try? FileManager.default.removeItem(at: harness.root) }
        let server = harness.server
        let id = harness.instance.noteID
        final class Flag: @unchecked Sendable { var changed = false }
        func observed(_ read: () async throws -> Void) async rethrows -> Bool {
            let flag = Flag()
            withObservationTracking { _ = server.state.notes.count; _ = server.list.revision } onChange: { flag.changed = true }
            try await read()
            return flag.changed
        }
        let revision = server.list.revision
        // The recorder's read: the note comes back and nothing shared moves.
        let privateRead = try await observed { _ = try await server.noteSnapshot(id: id) }
        #expect(privateRead == false)
        #expect(try await server.noteSnapshot(id: id).id == id)
        #expect(server.list.revision == revision)
        // A screen's read keeps the note for reopening, as before.
        let screenRead = try await observed { _ = try await server.note(id: id) }
        #expect(screenRead == true)
        #expect(server.state.notes.first?.id == id)
    }
}
