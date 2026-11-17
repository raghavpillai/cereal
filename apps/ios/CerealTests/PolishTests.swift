import Foundation
import SwiftUI
import Testing
import UIKit
import Contracts
import APIClient
@testable import Cereal

@MainActor private func temporaryStore(draftDelay: Duration = .milliseconds(120)) -> (NoteStore, URL) {
    let dir = FileManager.default.temporaryDirectory.appendingPathComponent("cereal-polish-" + UUID().uuidString, isDirectory: true)
    try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    let url = dir.appendingPathComponent("notes.json")
    return (NoteStore(fileURL: url, recordingsRoots: [dir.appendingPathComponent("recordings", isDirectory: true)], draftDelay: draftDelay), url)
}

private func savedNotes(_ url: URL) throws -> [Note] {
    try JSONDecoder().decode([Note].self, from: Data(contentsOf: url))
}

@MainActor private func waitUntil(timeout: Duration = .seconds(5), _ condition: () -> Bool) async {
    let deadline = ContinuousClock.now + timeout
    while !condition(), ContinuousClock.now < deadline { try? await Task.sleep(for: .milliseconds(20)) }
}

@MainActor struct DraftPersistenceTests {
    @Test func rapidEditsCoalesceToOneWriteWithTheFinalContent() async throws {
        let (store, url) = temporaryStore()
        let note = store.createNote()
        let before = store.diskWriteCount
        for length in 1...25 {
            store.updateDraft(note.id, title: "Standup", userNotes: String(repeating: "x", count: length))
        }
        // Memory is current at once; nothing has been written yet.
        #expect(store.note(note.id)?.userNotes.count == 25)
        #expect(store.diskWriteCount == before)
        #expect(store.hasUnsavedDraft)

        await waitUntil { store.diskWriteCount > before }
        try await Task.sleep(for: .milliseconds(300))
        #expect(store.diskWriteCount == before + 1)
        #expect(!store.hasUnsavedDraft)
        let saved = try savedNotes(url).first { $0.id == note.id }
        #expect(saved?.title == "Standup")
        #expect(saved?.userNotes == String(repeating: "x", count: 25))
        #expect(store.persistenceError == nil)
    }

    @Test func backgroundingWithAPendingEditWritesItImmediately() async throws {
        let (store, url) = temporaryStore(draftDelay: .seconds(30))
        let note = store.createNote()
        let before = store.diskWriteCount
        store.updateDraft(note.id, title: "", userNotes: "Budget approved")
        #expect(try savedNotes(url).first { $0.id == note.id }?.userNotes != "Budget approved")

        NotificationCenter.default.post(name: UIApplication.didEnterBackgroundNotification, object: nil)
        // Synchronous: the file is complete before the app can be suspended.
        #expect(store.diskWriteCount == before + 1)
        #expect(!store.hasUnsavedDraft)
        #expect(try savedNotes(url).first { $0.id == note.id }?.userNotes == "Budget approved")
    }

    @Test func resignActiveAndTerminationAlsoFlush() throws {
        for name in [UIApplication.willResignActiveNotification, UIApplication.willTerminateNotification] {
            let (store, url) = temporaryStore(draftDelay: .seconds(30))
            let note = store.createNote()
            store.updateDraft(note.id, title: "Kickoff", userNotes: "")
            NotificationCenter.default.post(name: name, object: nil)
            #expect(try savedNotes(url).first { $0.id == note.id }?.title == "Kickoff")
        }
    }

    @Test func anImmediateWriteIncludesThePendingDraftAndNothingIsWrittenTwice() async throws {
        let (store, url) = temporaryStore(draftDelay: .milliseconds(150))
        let note = store.createNote()
        store.updateDraft(note.id, title: "Retro", userNotes: "Ship the pill")
        let before = store.diskWriteCount
        // Stopping a recording commits the note durably, draft included.
        #expect(store.finishCapture(note.id, segment: CaptureSegment(index: 0, startedAt: .now, duration: 3, fileName: "segment-0.wav")))
        #expect(store.diskWriteCount == before + 1)
        let saved = try savedNotes(url).first { $0.id == note.id }
        #expect(saved?.userNotes == "Ship the pill")
        #expect(saved?.captureSegments.count == 1)
        try await Task.sleep(for: .milliseconds(500))
        #expect(store.diskWriteCount == before + 1)
    }

    @Test func flushMakesTheDraftVisibleToAnotherReader() throws {
        let (store, url) = temporaryStore(draftDelay: .seconds(30))
        let note = store.createNote()
        store.updateDraft(note.id, title: "Offsite", userNotes: "Book the venue")
        store.flush()
        store.flush() // Nothing pending: no second write.
        let reader = NoteStore(fileURL: url, recordingsRoots: [url.deletingLastPathComponent().appendingPathComponent("recordings", isDirectory: true)])
        #expect(reader.note(note.id)?.userNotes == "Book the venue")
    }

    @Test func aDeferredWriteNeverOverwritesANewerImmediateOne() async throws {
        let (store, url) = temporaryStore(draftDelay: .milliseconds(1))
        let note = store.createNote()
        for round in 0..<40 {
            store.updateDraft(note.id, title: "", userNotes: "draft \(round)")
            try await Task.sleep(for: .milliseconds(2)) // lets the deferred write start
            store.updateTitle(note.id, title: "final \(round)") // immediate write
            let saved = try savedNotes(url).first { $0.id == note.id }
            #expect(saved?.title == "final \(round)")
            #expect(saved?.userNotes == "draft \(round)")
        }
        try await Task.sleep(for: .milliseconds(200))
        #expect(try savedNotes(url).first { $0.id == note.id }?.title == "final 39")
    }

    @Test func unchangedTextSchedulesNothing() {
        let (store, _) = temporaryStore(draftDelay: .seconds(30))
        let note = store.createNote()
        store.updateDraft(note.id, title: "", userNotes: "")
        #expect(!store.hasUnsavedDraft)
    }
}

@MainActor struct ChatStreamingTests {
    @Test func tokensAreDeliveredOncePerIntervalAndNoneAreLost() async throws {
        var received = ""
        let buffer = ChatDeltaBuffer(interval: .milliseconds(60)) { received += $0 }
        for word in ["The ", "regatta ", "is ", "on ", "the ", "19th."] { buffer.append(word) }
        #expect(received.isEmpty)
        await waitUntil { buffer.deliveries > 0 }
        #expect(buffer.deliveries == 1)
        #expect(received == "The regatta is on the 19th.")
        buffer.append(" Thanks")
        buffer.flushNow()
        #expect(buffer.deliveries == 2)
        #expect(received == "The regatta is on the 19th. Thanks")
        buffer.flushNow()
        #expect(buffer.deliveries == 2)
    }

    @Test func aBurstOfTokensCostsFarFewerUpdatesThanTokens() async throws {
        var updates = 0
        var text = ""
        let buffer = ChatDeltaBuffer(interval: .milliseconds(33)) { text += $0; updates += 1 }
        for index in 0..<200 {
            buffer.append("w\(index) ")
            if index % 20 == 19 { try await Task.sleep(for: .milliseconds(40)) }
        }
        buffer.flushNow()
        #expect(text.split(separator: " ").count == 200)
        #expect(updates <= 12)
    }

    @Test func openingASourceDoesNotCancelButLeavingTheChatDoes() {
        let account = AccountID(address: "https://example.test", userID: "00000000-0000-4000-8000-000000000001")
        let one = NoteRef(account: account, id: "note-1"), two = NoteRef(account: account, id: "note-2")
        let chat = Route.serverChat(account, noteID: "note-1")
        // A note pushed on top of the chat covers it; the chat is still in the path.
        #expect(!ChatStreamPolicy.shouldCancelOnDisappear(path: [.serverNote(one), chat, .serverNote(two)], route: chat))
        #expect(!ChatStreamPolicy.shouldCancelOnDisappear(path: [chat, .serverNoteSource(two, raw: true)], route: chat))
        // Going back removes it.
        #expect(ChatStreamPolicy.shouldCancelOnDisappear(path: [.serverNote(one)], route: chat))
        #expect(ChatStreamPolicy.shouldCancelOnDisappear(path: [], route: chat))
        #expect(ChatStreamPolicy.shouldCancelOnDisappear(path: [.serverChat(account, noteID: "note-2")], route: chat))
    }

    /// `scrollTo` puts the marker's anchor point on the scroll view's anchor
    /// point on both axes. The marker spans the message column, which is
    /// off-centre, so the anchor has to cancel that out or a short
    /// conversation ends up scrolled sideways (measured: 4 pt with `.bottom`).
    @Test func scrollingToTheEndLeavesTheColumnWhereItIs() {
        for viewport in [320.0, 358.0, 402.0, 874.0] as [CGFloat] {
            let column = viewport - ChatColumn.leading - ChatColumn.trailing
            func offset(_ anchor: UnitPoint) -> CGFloat { ChatColumn.leading + anchor.x * column - anchor.x * viewport }
            #expect(abs(offset(.chatEnd)) < 0.001)
            #expect(abs(offset(.bottom) - 4) < 0.001)
        }
        #expect(UnitPoint.chatEnd.y == 1)
    }

    @Test func aStreamingReplyParsesOnlyTheBlockThatChanged() {
        ChatMarkdownCache.removeAll()
        let finished = ["### Decisions", "- Regatta on Saturday the 19th", "- Budget is nine hundred for buoys", "", "### Actions", "- Priya books the slipway"]
        let tail = "- Tomas orders twelve new buoys for the outer course".split(separator: " ").map(String.init)
        var builds = 0
        var passes = 0
        // Each published update re-renders the reply: every block asks for its text.
        for count in 1...tail.count {
            let lines = finished + [tail.prefix(count).joined(separator: " ")]
            let blocks = ChatMarkdownCache.blocks(lines.joined(separator: "\n"))
            for block in blocks {
                guard let source = Self.text(of: block) else { continue }
                _ = ChatMarkdownCache.inline(source, context: "turn") { builds += 1; return AttributedString(source) }
            }
            passes += 1
        }
        let textBlocks = finished.filter { !$0.isEmpty }.count
        // Before: every block was parsed on every pass.
        let uncached = passes * (textBlocks + 1)
        #expect(uncached == 60)
        // Now: each finished block once, plus the growing block once per pass.
        #expect(builds == textBlocks + passes)
        #expect(builds == 15)
    }

    @Test func citationsArePartOfTheCacheKey() {
        ChatMarkdownCache.removeAll()
        var builds = 0
        _ = ChatMarkdownCache.inline("See [1]", context: "turn-a|c1") { builds += 1; return AttributedString("a") }
        _ = ChatMarkdownCache.inline("See [1]", context: "turn-a|c1") { builds += 1; return AttributedString("a") }
        _ = ChatMarkdownCache.inline("See [1]", context: "turn-a|") { builds += 1; return AttributedString("b") }
        #expect(builds == 2)
    }

    private static func text(of block: ChatMarkdownBlock) -> String? {
        switch block {
        case .heading(let value), .paragraph(let value), .quote(let value): value
        case .list(_, let value, _), .continuation(_, let value, _): value
        case .code, .space, .table: nil
        }
    }
}

struct ServerHomeGroupingTests {
    private func note(_ id: String, _ title: String, created: String) -> NoteDTO {
        let json = """
        {"id":"\(id)","title":"\(title)","body":"","enhanced":"","createdAt":"\(created)","updatedAt":"\(created)","date":"","duration":"","generatedRevision":null,"generationState":"idle","recordingState":"idle","transcriptRevision":0,"transcriptSegments":[]}
        """
        return try! JSONDecoder().decode(NoteDTO.self, from: Data(json.utf8))
    }

    @Test func notesAreSortedNewestFirstAndGroupedByDayOnce() throws {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = TimeZone(identifier: "UTC")!
        let now = try #require(Date.serverTimestamp("2026-11-13T18:00:00Z"))
        let sections = ServerHomeGrouping.sections([
            note("a", "Morning", created: "2026-11-13T09:00:00Z"),
            note("b", "", created: "2026-11-12T15:30:00.250Z"),
            note("c", "Afternoon", created: "2026-11-13T14:00:00Z"),
            note("d", "Last year", created: "2025-11-13T10:00:00Z"),
        ], now: now, calendar: calendar)
        #expect(sections.map(\.title).prefix(2) == ["Earlier today", "Yesterday"])
        #expect(sections.count == 3)
        #expect(sections[0].rows.map(\.id) == ["c", "a"])
        #expect(sections[1].rows.first?.title == "Untitled note")
        // A note from the same date a year earlier is its own, dated group.
        #expect(sections[2].rows.map(\.id) == ["d"])
        #expect(sections[2].title.contains("2025"))
        #expect(sections.flatMap(\.rows).map(\.order) == [0, 1, 2, 3])
        #expect(Set(sections.map(\.id)).count == sections.count)
    }

    @Test func timestampsParseWithAndWithoutFractionalSeconds() {
        #expect(Date.serverTimestamp("2026-11-13T09:00:00Z") != nil)
        #expect(Date.serverTimestamp("2026-11-13T09:00:00.123Z") != nil)
        #expect(Date.serverTimestamp("not a date") == nil)
    }
}

@MainActor struct EdgeGestureTests {
    @Test func backWinsOnAPushedScreenAndTheSidebarOnlyOnHome() {
        #expect(EdgeGestureRules.mayPop(stackDepth: 2, inTransition: false, presenting: false))
        #expect(!EdgeGestureRules.mayPop(stackDepth: 1, inTransition: false, presenting: false))
        #expect(EdgeGestureRules.mayOpenSidebar(stackDepth: 1, inTransition: false, presenting: false))
        #expect(!EdgeGestureRules.mayOpenSidebar(stackDepth: 2, inTransition: false, presenting: false))
    }

    @Test func neitherGestureStartsDuringATransitionOrUnderAPresentation() {
        #expect(!EdgeGestureRules.mayPop(stackDepth: 3, inTransition: true, presenting: false))
        #expect(!EdgeGestureRules.mayPop(stackDepth: 3, inTransition: false, presenting: true))
        // Just after a pop commits the stack already has one screen.
        #expect(!EdgeGestureRules.mayOpenSidebar(stackDepth: 1, inTransition: true, presenting: false))
        #expect(!EdgeGestureRules.mayOpenSidebar(stackDepth: 1, inTransition: false, presenting: true))
    }

    @Test func onlyTouchesAtTheLeadingEdgeAndOnlyInwardHorizontalDrags() {
        #expect(EdgeGestureRules.startsAtLeadingEdge(x: 4, width: 402, rightToLeft: false))
        #expect(!EdgeGestureRules.startsAtLeadingEdge(x: 24, width: 402, rightToLeft: false))
        #expect(EdgeGestureRules.startsAtLeadingEdge(x: 398, width: 402, rightToLeft: true))
        #expect(!EdgeGestureRules.startsAtLeadingEdge(x: 4, width: 402, rightToLeft: true))
        #expect(EdgeGestureRules.isInwardHorizontal(dx: 300, dy: 40))
        #expect(!EdgeGestureRules.isInwardHorizontal(dx: 30, dy: 400))
        #expect(!EdgeGestureRules.isInwardHorizontal(dx: -200, dy: 0))
    }

    @Test func anUntestedSystemOrMissingRecogniserFallsBackInsteadOfLosingTheGestures() {
        #expect(EdgeGestureRules.takeover(hasNavigationController: true, hasPopRecogniser: true, osMajorVersion: 26) == .takeover)
        #expect(EdgeGestureRules.takeover(hasNavigationController: true, hasPopRecogniser: true, osMajorVersion: 27) == .takeover)
        for status in [
            EdgeGestureRules.takeover(hasNavigationController: true, hasPopRecogniser: true, osMajorVersion: 28),
            EdgeGestureRules.takeover(hasNavigationController: false, hasPopRecogniser: false, osMajorVersion: 27),
            EdgeGestureRules.takeover(hasNavigationController: true, hasPopRecogniser: false, osMajorVersion: 27),
        ] {
            guard case .fallback = status else { Issue.record("expected the fallback"); continue }
        }
    }

    /// Fails on an OS where UIKit no longer offers the recognisers this app
    /// re-enables, which is the signal to re-test before adding it to `testedMajorVersions`.
    @Test func theExpectedRecognisersExistOnThisSystemAndAreTakenOver() throws {
        let root = UIViewController(), pushed = UIViewController()
        let navigation = UINavigationController(rootViewController: root)
        navigation.pushViewController(pushed, animated: false)
        // Screens hide the back button, which is what disables the system gesture.
        pushed.navigationItem.hidesBackButton = true
        let enabler = SwipeBackController()
        pushed.addChild(enabler)
        pushed.view.addSubview(enabler.view)
        enabler.didMove(toParent: pushed)
        enabler.enable()

        let pop = try #require(navigation.interactivePopGestureRecognizer)
        #expect(pop.delegate is SwipeBackDelegate)
        #expect(pop.isEnabled)
        let content = try #require(navigation.interactiveContentPopGestureRecognizer)
        #expect(content.delegate is SwipeBackDelegate)
        #expect(content.isEnabled)
        #expect(EdgeGestureHealth.shared.status == .takeover)
        // The sidebar's own pan is installed on the same view, once.
        enabler.enable()
        let pans = navigation.view.gestureRecognizers?.filter { $0.delegate is SidebarEdgeGesture } ?? []
        #expect(pans.count == 1)
    }

    @Test func aViewOutsideANavigationControllerReportsTheFallback() {
        let health = EdgeGestureHealth()
        health.report(EdgeGestureRules.takeover(hasNavigationController: false, hasPopRecogniser: false, osMajorVersion: 27))
        #expect(health.usesFallback)
        health.report(.takeover)
        #expect(!health.usesFallback)
    }

    @Test func aCancelledCloseDragLeavesTheDrawerSettledAndItsRowsLive() async throws {
        let model = SidebarModel()
        model.width = 300
        model.open()
        #expect(model.isOpen)
        // A drag that began as a vertical scroll does not jump when it turns sideways.
        model.beginDrag(at: -80)
        model.drag(translation: -80)
        #expect(model.progress == 1)
        model.drag(translation: -140)
        #expect(abs(model.progress - 0.8) < 0.001)
        // The system takes the touch away: no end event arrives.
        model.cancelDrag()
        #expect(!model.isDragging)
        #expect(model.progress == 0 || model.progress == 1)
        // Reopening by any other means never inherits a stuck state.
        model.close()
        model.open()
        #expect(!model.suppressesRowTaps)
    }
}

struct RecordingPolishTests {
    @Test func hapticsFollowRecordingStateChanges() {
        #expect(RecordingFeedback.feedback(from: .idle, to: .recording) != nil)
        #expect(RecordingFeedback.feedback(from: .paused, to: .recording) != nil)
        #expect(RecordingFeedback.feedback(from: .recording, to: .paused) != nil)
        #expect(RecordingFeedback.feedback(from: .recording, to: .stopped) != nil)
        #expect(RecordingFeedback.feedback(from: .recording, to: .failed("x")) != nil)
        #expect(RecordingFeedback.feedback(from: .recording, to: .recording) == nil)
        #expect(RecordingFeedback.feedback(from: .idle, to: .stopped) == nil)
    }
}

/// Counts body evaluations while a simulated recorder runs. Counts, not times:
/// the result does not depend on how fast the machine is.
@MainActor struct RecorderObservationTests {
    private struct ScopedParent: View {
        let recorder: Recorder
        var body: some View {
            let _ = PerfProbe.hit("Test.ScopedParent.body")
            VStack {
                Text(recorder.state == .recording ? "Recording" : "Stopped")
                RecordingStopPill(recorder: recorder) {}
            }
        }
    }

    /// How the server note was written before: the parent reads the clock itself.
    private struct UnscopedParent: View {
        let recorder: Recorder
        var body: some View {
            let _ = PerfProbe.hit("Test.UnscopedParent.body")
            Text(Duration.seconds(recorder.elapsed).formatted(.time(pattern: .minuteSecond)))
        }
    }

    @Test func onlyTheTimerAndMeterFollowTheRecorderClock() async throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("cereal-observe-" + UUID().uuidString, isDirectory: true)
        let recorder = Recorder(simulateCapture: true, recordingRoot: root)
        try #require(recorder.isSimulated)
        let scene = try #require(UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }.first)
        let window = UIWindow(windowScene: scene)
        window.rootViewController = UIHostingController(rootView: VStack {
            ScopedParent(recorder: recorder)
            UnscopedParent(recorder: recorder)
            RecordingPill(recorder: recorder, onStop: {}, onStart: {})
        })
        window.isHidden = false
        defer { window.isHidden = true }

        recorder.startSegment(noteID: UUID())
        #expect(recorder.state == .recording)
        try await Task.sleep(for: .milliseconds(400)) // first layout and state change
        PerfProbe.reset()
        try await Task.sleep(for: .seconds(2))
        let scoped = PerfProbe.count("Test.ScopedParent.body")
        let unscoped = PerfProbe.count("Test.UnscopedParent.body")
        let pill = PerfProbe.count("RecordingPill.body")
        let stopPill = PerfProbe.count("RecordingStopPill.body")
        let timers = PerfProbe.count("RecorderTimer.body")
        recorder.stopSegment()

        // The clock ticked: the control case and the leaf timers followed it.
        #expect(unscoped >= 8)
        #expect(timers >= 16)
        // Nothing around the leaves was rebuilt.
        #expect(scoped == 0)
        #expect(pill == 0)
        #expect(stopPill == 0)
    }
}

// MARK: - Server list rows: attendee tiles and search results

@MainActor struct ServerListRowTests {
    private func row(_ id: String, _ title: String, created: String, attendees: [String] = [], total: Int = 0) -> ServerNoteRow {
        ServerNoteRow(id: id, title: title, date: String(created.prefix(10)), createdAt: created, updatedAt: created, attendeeNames: attendees, attendeeTotal: total)
    }
    private func size<Content: View>(_ view: Content, width: CGFloat? = nil) -> CGSize {
        let host = UIHostingController(rootView: view.frame(width: width))
        host.view.frame = CGRect(x: 0, y: 0, width: width ?? 400, height: 400)
        host.view.layoutIfNeeded()
        return host.sizeThatFits(in: CGSize(width: width ?? .greatestFiniteMagnitude, height: .greatestFiniteMagnitude))
    }

    @Test func aMeetingShowsTheOtherPartysInitialAndEverythingElseADocument() throws {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = TimeZone(identifier: "UTC")!
        let now = try #require(Date.serverTimestamp("2026-11-13T18:00:00Z"))
        let sections = ServerHomeGrouping.sections([
            row("a", "With Grace", created: "2026-11-13T09:00:00Z", attendees: ["Grace Hopper", "Alan Turing"], total: 3),
            row("b", "Alone", created: "2026-11-13T08:00:00Z"),
            row("c", "By address", created: "2026-11-13T07:00:00Z", attendees: ["  ", "émile@example.test"], total: 2),
            row("d", "Yesterday", created: "2026-11-12T09:00:00Z", attendees: ["mike"], total: 2),
        ], now: now, calendar: calendar)
        #expect(sections.map(\.title) == ["Earlier today", "Yesterday"])
        #expect(sections[0].rows.map(\.tile) == [.letter("G"), .document, .letter("É")])
        #expect(sections[1].rows.map(\.tile) == [.letter("M")])
        // The time under the title, then, as in the reference, "• 3 attendees" for a meeting with others.
        let times = ["09:00", "08:00", "07:00"].map { (Date.serverTimestamp("2026-11-13T\($0):00Z") ?? .distantPast).formatted(date: .omitted, time: .shortened) }
        #expect(sections[0].rows.map(\.subtitle) == [times[0] + " • 3 attendees", times[1], times[2] + " • 2 attendees"])
        #expect(sections.flatMap(\.rows).map(\.order) == [0, 1, 2, 3])
    }

    @Test func aRowMeasuresTheSameWhateverItsTile() {
        // The row itself is untouched; this pins that a letter tile cannot move it.
        let tiles: [NoteTile] = [.document, .phone, .letter("G"), .letter("W"), .letter("É")]
        let squares = tiles.map { size(NoteTileView(tile: $0)) }
        #expect(squares.allSatisfy { $0 == CGSize(width: 44, height: 44) })
        for width in [320.0, 393.0, 430.0] {
            let rows = tiles.map { size(NoteListRow(tile: $0, title: "Quarterly planning review", subtitle: "9:41 AM"), width: width) }
            #expect(Set(rows.map { "\($0.width)x\($0.height)" }).count == 1)
            #expect(rows[0].height > 44 && rows[0].width == width)
        }
        // A long title is still one line: the row height does not depend on the text either.
        let long = size(NoteListRow(tile: .letter("G"), title: String(repeating: "Long title ", count: 20), subtitle: "9:41 AM"), width: 393)
        #expect(long == size(NoteListRow(tile: .document, title: "Short", subtitle: "9:41 AM"), width: 393))
    }

    @Test func serverResultsKeepTheirOrderAndMarkOnlyTheMatchedWords() {
        let hits = [
            ServerNoteSearchHit(row: row("11111111-1111-4111-8111-111111111111", "Roadmap review", created: "2026-11-13T09:00:00Z", attendees: ["Grace Hopper"], total: 2), excerpt: nil),
            ServerNoteSearchHit(row: row("22222222-2222-4222-8222-222222222222", "", created: "2026-11-12T15:30:00Z"),
                                excerpt: [.init(text: "… slips the ", isMatch: false), .init(text: "roadmap", isMatch: true), .init(text: " to next quarter", isMatch: false)]),
            ServerNoteSearchHit(row: row("33333333-3333-4333-8333-333333333333", "Recorded here", created: "2026-11-11T10:00:00Z", attendees: ["Alan Turing"], total: 2), excerpt: nil),
        ]
        let account = AccountID(address: "https://example.test", userID: "00000000-0000-4000-8000-000000000001")
        let rows = NoteSearch.server(hits, account: account) { $0.account == account && $0.id.hasPrefix("3333") }
        #expect(rows.map(\.route) == hits.map { Route.serverNote(NoteRef(account: account, id: $0.id)) })
        #expect(rows.map(\.tile) == [.letter("G"), .document, .phone])
        #expect(rows.map(\.title) == ["Roadmap review", "Untitled note", "Recorded here"])
        #expect(rows[0].excerpt == nil && rows[1].excerpt == "… slips the roadmap to next quarter")
        let marked = NoteSearch.marked(hits[1].excerpt ?? [])
        #expect(String(marked.characters) == "… slips the roadmap to next quarter")
        let coloured = marked.runs.filter { $0.foregroundColor != nil }.map { String(marked[$0.range].characters) }
        #expect(coloured == ["roadmap"])
        #expect(NoteSearch.server([], account: account).isEmpty)
    }
}

/// Release builds have no demo. Their store leaves what an earlier version saved exactly as it was.
@MainActor struct OffDiskStoreTests {
    @Test func aStoreOffDiskNeitherReadsNorWritesTheDemoFiles() throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("cereal-off-disk-" + UUID().uuidString, isDirectory: true)
        let recordings = dir.appendingPathComponent("recordings", isDirectory: true)
        let interrupted = recordings.appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: interrupted, withIntermediateDirectories: true)
        try Data("RIFF".utf8).write(to: interrupted.appendingPathComponent("segment-0.wav"))
        let file = dir.appendingPathComponent("notes.json")
        let saved = try JSONEncoder().encode([Note(title: "Made in the old demo", createdAt: .now, userNotes: "kept as it was")])
        try saved.write(to: file)

        let store = NoteStore(fileURL: file, recordingsRoots: [recordings], onDisk: false)
        #expect(store.notes.isEmpty)
        #expect(store.captureRecoveryMessage == nil)
        store.createNote()
        store.flush()
        #expect(try Data(contentsOf: file) == saved)
        #expect(try FileManager.default.contentsOfDirectory(atPath: interrupted.path) == ["segment-0.wav"])
        // A store on disk, as debug builds use for the QA demo, reads the same file.
        #expect(NoteStore(fileURL: file, recordingsRoots: [recordings]).notes.contains { $0.title == "Made in the old demo" })
    }

    @Test func aFirstDebugLaunchStillSeedsTheQADemo() throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("cereal-qa-demo-" + UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let store = NoteStore(fileURL: dir.appendingPathComponent("notes.json"), recordingsRoots: [dir])
        #expect(store.activeNotes.contains { !$0.sections.isEmpty })
    }
}
