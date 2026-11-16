import AVFoundation
import Foundation
import Testing
import APIClient
import Contracts
@testable import Cereal

/// The account rules as the app applies them: a recording that outlives its
/// session, a note deleted on its server, leftovers from before, the sheets,
/// and one account looking as it always has.
@MainActor
@Suite(.serialized)
struct AccountLifecycleTests {
    @Test func s3_aSessionThatEndsDuringARecordingKeepsItGoingAndStopsAndWritesItsNotesAfterSignIn() async throws {
        let harness = try await LiveSessionHarness(framesPerUtterance: 10)
        defer { try? FileManager.default.removeItem(at: harness.root) }
        let server = harness.server, controller = harness.controller
        let ref = try #require(controller.ref(harness.instance.noteID))
        let account = try #require(server.accountID)
        await controller.start(note: try await harness.note())
        #expect(controller.recorder.state == .recording)
        // Nothing can sign the account out or remove it while it is being recorded to.
        #expect(controller.removalBlock(for: account) == "Stop the recording first.")

        var time = 0.0
        for _ in 0..<3 {
            time = harness.input.feed(format: InjectedAudio.builtIn, from: time, seconds: 1)
            try await Task.sleep(for: .milliseconds(120))
        }
        try await waitFor("live finals") { controller.transcript(for: ref)?.segments.isEmpty == false }

        // The server ends the session in the middle of the meeting; the next request (Home's refresh) learns of it.
        harness.instance.set(refused: ["unit-test-token"])
        await server.refresh()
        for _ in 0..<2 {
            time = harness.input.feed(format: InjectedAudio.builtIn, from: time, seconds: 1)
            try await Task.sleep(for: .milliseconds(120))
        }
        try await waitFor("the session's end to be noticed") { server.status == .needsSignIn }
        // The capture is local and goes on. The note stays where it is, with its recording.
        #expect(controller.recorder.state == .recording)
        #expect(controller.isActive(ref) && controller.isIndicator(ref))
        #expect(controller.transcript(for: ref) != nil)
        #expect(server.accountID == account && server.instance != nil)
        let framesBefore = controller.recorder.capturedFrames
        time = harness.input.feed(format: InjectedAudio.builtIn, from: time, seconds: 1)
        #expect(controller.recorder.capturedFrames >= framesBefore + 16_000 - 400)

        // Stop works. Its Stop and its notes wait for the account: nothing is sent with the refused session.
        let sent = harness.instance.allRequests.count
        controller.stop()
        #expect(controller.recorder.state == .stopped)
        #expect(controller.recorder.capturedFrames == 6 * 16_000)
        try await waitFor("the Stop to give up") { !controller.isGeneratingNotes(ref) }
        #expect(controller.removalBlock(for: account) == nil)
        #expect(harness.instance.allRequests.count == sent)
        #expect(harness.instance.count("POST", suffix: "/recording") == 1) // the Stop has not reached the server

        // Signing in again as the same user: the saved Stop goes, and the notes are written after it.
        await server.connect(instance: "https://example.test", token: "second-session")
        #expect(server.status == .ready && server.accountID == account)
        await controller.sync(with: [server])
        try await waitFor("the saved Stop and the notes") {
            harness.instance.count("POST", suffix: "/recording") == 2 && harness.instance.generations.count == 1
        }
        #expect(harness.instance.generations == [harness.instance.transcriptRevision])
        #expect(harness.instance.audioRequests == 0)
        // Once is enough: the next launch sends neither again.
        let relaunched = ServerRecordingController(outboxRoot: harness.root.appendingPathComponent("outbox"), makeBackend: { EngineCaptureBackend(input: harness.input) })
        await relaunched.sync(with: [server])
        try await Task.sleep(for: .milliseconds(200))
        #expect(harness.instance.count("POST", suffix: "/recording") == 2 && harness.instance.generations.count == 1)
    }

    @Test func v4_aRecordingWhoseNoteIsGoneFromItsServerIsNotStoppedAgainOrWrittenUp() async throws {
        let harness = try await LiveSessionHarness(framesPerUtterance: 10)
        defer { try? FileManager.default.removeItem(at: harness.root) }
        let controller = harness.controller
        let ref = try #require(controller.ref(harness.instance.noteID))
        await controller.start(note: try await harness.note())
        _ = harness.input.feed(format: InjectedAudio.builtIn, from: 0, seconds: 2)
        try await waitFor("live finals") { controller.transcript(for: ref)?.segments.isEmpty == false }
        // The note is deleted on the server (from another device) during the meeting.
        harness.instance.set(note: 404)
        controller.stop()
        try await waitFor("settled") { !controller.isGeneratingNotes(ref) && harness.instance.count("POST", suffix: "/recording") == 2 }
        #expect(harness.instance.generations.isEmpty)
        // There is nothing to send again at the next launch.
        let relaunched = ServerRecordingController(outboxRoot: harness.root.appendingPathComponent("outbox"), makeBackend: { EngineCaptureBackend(input: harness.input) })
        await relaunched.sync(with: [harness.server])
        try await Task.sleep(for: .milliseconds(200))
        #expect(harness.instance.count("POST", suffix: "/recording") == 2 && harness.instance.generations.isEmpty)
        #expect(harness.instance.audioRequests == 0)
    }

    @Test func audioLeftByAnEarlierVersionOrByARecordingThatEndedWithTheAppIsLeftUntouchedAtLaunch() async throws {
        let harness = try await LiveSessionHarness()
        defer { try? FileManager.default.removeItem(at: harness.root) }
        let account = try #require(harness.server.accountID)
        let outbox = harness.root.appendingPathComponent("outbox")
        let directory = try AccountStorage(root: outbox).directory(account.raw)
        let noteID = harness.instance.noteID
        // An earlier version's delivery journal and one of its parts, never uploaded.
        let part = UUID().uuidString.lowercased()
        try Data(#"[{"id":"\#(part)","noteID":"\#(noteID)","channel":"mic","sequence":0,"startMs":0,"durationMs":2000,"fileName":"\#(part).wav","sourceFileName":"capture-0.wav","state":"pending"}]"#.utf8)
            .write(to: directory.appendingPathComponent("outbox.json"))
        try Data("RIFF".utf8).write(to: directory.appendingPathComponent("\(part).wav"))
        // A capture of two seconds whose recording ended with the app, and the ledger that placed it.
        let capture = try Recorder.fileURL(noteID: try #require(UUID(uuidString: noteID)), index: 0, continuous: true,
                                           root: directory.appendingPathComponent("recordings"))
        try (ContinuousCapture.header(dataBytes: 64_000) + Data(repeating: 0, count: 64_000)).write(to: capture)
        try CaptureRecovery.begin(noteID: try #require(UUID(uuidString: noteID)), index: 0, startedAt: .now, audioFile: capture)
        try Data(#"{"\#(noteID)":{"baseOffsetMs":5000,"nextSequence":1,"segments":[],"segmentOffsets":{"capture-0.wav":5000},"needsStop":false}}"#.utf8)
            .write(to: directory.appendingPathComponent("captures.json"))
        #expect(AccountStorage(root: outbox).summary(account.raw) == AccountDataSummary(recordings: 1, unsent: 1))
        let before = Set(try FileManager.default.contentsOfDirectory(atPath: directory.path))
        let captureBytes = try Data(contentsOf: capture)

        let relaunched = ServerRecordingController(outboxRoot: outbox, makeBackend: { EngineCaptureBackend(input: harness.input) })
        await relaunched.sync(with: [harness.server])
        // No stream can send any of it now and nothing reads it, but nothing is deleted either: every file is
        // still there as it was, and nothing was uploaded.
        #expect(Set(try FileManager.default.contentsOfDirectory(atPath: directory.path)) == before)
        #expect(FileManager.default.fileExists(atPath: directory.appendingPathComponent("\(part).wav").path))
        #expect(try Data(contentsOf: capture) == captureBytes)
        #expect(harness.instance.audioRequests == 0)
        // The capture still placed the note's next one on its timeline: after its two seconds.
        let ledger = try JSONDecoder().decode([String: ServerCaptureHistory].self, from: Data(contentsOf: directory.appendingPathComponent("captures.json")))
        #expect(ledger[noteID]?.nextStartMs == 7_000)
        #expect(relaunched.ledger(of: account) == [noteID])
    }

    @Test func p1_twoAccountsEachRecordThroughTheirOwnServer() async throws {
        // Two servers, one recorder. The second is the one recorded to.
        let first = try await LiveSessionHarness(), second = try await LiveSessionHarness(framesPerUtterance: 10, host: "second.example.test", controller: first.controller)
        defer { try? FileManager.default.removeItem(at: first.root); try? FileManager.default.removeItem(at: second.root) }
        let controller = first.controller
        await controller.sync(with: [first.server, second.server])
        let a = try #require(first.server.accountID), b = try #require(second.server.accountID)
        #expect(a != b && Set(controller.knownAccounts) == [a, b])
        let note = NoteRef(account: b, id: second.instance.noteID)

        await controller.start(note: try await second.note(), in: b)
        #expect(controller.recorder.state == .recording && controller.active == note)
        var time = 0.0
        for _ in 0..<3 {
            time = first.input.feed(format: InjectedAudio.builtIn, from: time, seconds: 1)
            try await Task.sleep(for: .milliseconds(120))
        }
        controller.stop()
        try await waitFor("the Stop and the notes on the second server") {
            second.instance.count("POST", suffix: "/recording") == 2 && second.instance.generations.count == 1
        }
        // The first server saw nothing of it: no start, no stop, no notes, no stream.
        #expect(first.instance.count("POST", suffix: "/recording") == 0 && first.instance.generations.isEmpty)
        #expect(first.live.counts.connections == 0 && second.live.counts.connections == 1)
        #expect(controller.ledger(of: b) == [note.id] && controller.ledger(of: a).isEmpty)
        // The same note id in the first account is another note: nothing of the recording shows there.
        let twin = NoteRef(account: a, id: note.id)
        #expect(!controller.isActive(twin) && !controller.isGeneratingNotes(twin) && controller.transcript(for: twin) == nil)
        // Only the account recorded to is held by a recording; the other can leave, and its record ends with it.
        #expect(controller.removalBlock(for: a) == nil)
        await controller.sync(with: [second.server])
        #expect(controller.knownAccounts == [b])
    }
}

@MainActor
struct AccountPresentationTests {
    private let host = "notes.acme.com"

    @Test func o3_theSignOutSheetSaysWhatStaysAndTheEraseSheetCountsWhatGoes() {
        #expect(AccountRemovalText.signOutTitle(host: host) == "Sign out of notes.acme.com?")
        #expect(AccountRemovalText.signOutMessage(AccountDataSummary(recordings: 41, unsent: 3), host: host, email: "r@acme.com")
                == "41 recordings stay on this iPhone, and are here again when you sign back in.\n3 of them have audio that was never uploaded.\nSigned in as r@acme.com, as reported by notes.acme.com.")
        #expect(AccountRemovalText.signOutMessage(AccountDataSummary(recordings: 1, unsent: 1), host: host, email: "")
                == "1 recording stays on this iPhone, and is here again when you sign back in.\n1 of them has audio that was never uploaded.")
        #expect(AccountRemovalText.signOutMessage(AccountDataSummary(), host: host, email: "r@acme.com")
                == "Your notes stay on notes.acme.com. No recordings from this account are saved on this iPhone.\nSigned in as r@acme.com, as reported by notes.acme.com.")

        #expect(AccountRemovalText.eraseTitle(host: host) == "Remove notes.acme.com and erase its data from this iPhone?")
        #expect(AccountRemovalText.eraseMessage(AccountDataSummary(recordings: 41, unsent: 3, deletedOnServer: 2), host: host)
                == "41 recordings saved here, 3 of which were never uploaded.\n2 are of notes that were deleted on notes.acme.com.\nChats with this account on this iPhone are erased too. The notes on notes.acme.com are not affected.")
        #expect(AccountRemovalText.eraseMessage(AccountDataSummary(), host: host).hasPrefix("No recordings from this account are saved here."))
        // The button says so when erasing destroys audio that exists nowhere else.
        #expect(AccountRemovalText.eraseButton(AccountDataSummary(recordings: 41)) == "Erase")
        #expect(AccountRemovalText.eraseButton(AccountDataSummary(recordings: 41, unsent: 3)) == "Erase, including 3 recordings that exist nowhere else")
        #expect(AccountRemovalText.eraseButton(AccountDataSummary(recordings: 1, unsent: 1)) == "Erase, including 1 recording that exists nowhere else")
        // The host is in every sheet, whole.
        let long = "notes.acme.com.a-very-long-subdomain.example.io:8443"
        for text in [AccountRemovalText.signOutTitle(host: long), AccountRemovalText.eraseTitle(host: long),
                     AccountRemovalText.signOutMessage(AccountDataSummary(), host: long, email: "r@acme.com"),
                     AccountRemovalText.eraseMessage(AccountDataSummary(recordings: 2), host: long)] { #expect(text.contains(long)) }
    }

    @Test func o1_savedDataIsDescribedByItsRecordingsSizeAndDate() {
        let date = Date(timeIntervalSince1970: 1_800_000_000)
        let item = SavedAccountData(directory: String(repeating: "a", count: 64), account: nil, bytes: 14_500_000, lastChanged: date,
                                    summary: AccountDataSummary(recordings: 3, unsent: 1))
        let line = AccountRemovalText.saved(item)
        #expect(line.hasPrefix("3 recordings · 1 never uploaded · "))
        #expect(line.contains(ByteCountFormatter.string(fromByteCount: 14_500_000, countStyle: .file)))
        #expect(line.hasSuffix("last changed " + date.formatted(date: .abbreviated, time: .omitted)))
        #expect(item.host == nil)
    }

    @Test func v7_withOneAccountHomeIsThatAccountsListWithNothingAboutAccountsShown() throws {
        let account = AccountID(address: "https://example.test", userID: "00000000-0000-4000-8000-000000000001")
        func row(_ id: String, _ title: String, _ created: String, attendee: String? = nil) -> ServerNoteRow {
            ServerNoteRow(id: id, title: title, date: String(created.prefix(10)), createdAt: created, updatedAt: created,
                          attendeeNames: attendee.map { [$0] } ?? [], attendeeTotal: attendee == nil ? 0 : 2)
        }
        let rows = [row("11111111-1111-4111-8111-111111111111", "Weekly planning", "2026-11-15T08:15:00.000Z"),
                    row("22222222-2222-4222-8222-222222222222", "", "2026-11-15T07:47:00.000Z", attendee: "Grace Hopper"),
                    row("33333333-3333-4333-8333-333333333333", "Q1 planning", "2026-11-12T07:47:00.000Z")]
        let now = try #require(Date.serverTimestamp("2026-11-15T20:00:00.000Z"))
        var calendar = Calendar(identifier: .gregorian); calendar.timeZone = TimeZone(identifier: "UTC")!
        // What Home showed before accounts, from the one list...
        let before = ServerHomeGrouping.sections(rows, now: now, calendar: calendar)
        // ...and what it shows now, from the list of the one account.
        let after = ServerHomeGrouping.sections(rows.map { AccountNoteRow(account: account, row: $0) }, now: now, calendar: calendar)
        #expect(after.map(\.title) == before.map(\.title) && after.map(\.id) == before.map(\.id))
        let shown = after.flatMap(\.rows), was = before.flatMap(\.rows)
        #expect(shown.map(\.id) == was.map(\.id) && shown.map(\.title) == was.map(\.title) && shown.map(\.subtitle) == was.map(\.subtitle))
        #expect(shown.map(\.tile) == was.map(\.tile) && shown.map(\.order) == was.map(\.order))
        // Each row now knows its account, which is what opening it uses; nothing about it is drawn.
        #expect(shown.map(\.ref) == rows.map { NoteRef(account: account, id: $0.id) })
        #expect(was.allSatisfy { $0.ref == nil })

        // Labels, dots and hosts appear only with two or more accounts; the Live Activity is told nothing to show.
        let name = "v7-" + UUID().uuidString
        let defaults = try #require(UserDefaults(suiteName: name))
        defer { defaults.removePersistentDomain(forName: name) }
        let accounts = AccountsModel(defaults: defaults, credentials: MemoryAccountStore(),
                                     storage: AccountStorage(root: FileManager.default.temporaryDirectory.appendingPathComponent(name)))
        accounts.showQAState(.loaded([]))
        #expect(accounts.sessions.count == 1 && !accounts.showsAccountLabels)
        #expect(!AppAccounts.allowsMultiple)
        // The same note id in two accounts is two rows, each its own.
        let other = AccountID(address: "https://other.example.test", userID: "00000000-0000-4000-8000-000000000001")
        let both = ServerHomeGrouping.sections([AccountNoteRow(account: account, row: rows[0]), AccountNoteRow(account: other, row: rows[0])], now: now, calendar: calendar)
        #expect(Set(both.flatMap(\.rows).map(\.listID)).count == 2)
    }

    @Test func answersStillArrivingEndOnlyForAnAccountThatHasGone() async throws {
        let store = ServerChatSessions()
        func session(_ account: String) -> ServerChatSession {
            let key = ChatHistoryScope.key(accountIdentity: account, noteID: nil)!
            return store.session(for: key, accountIdentity: account, noteID: nil,
                                 events: { _, _, _, _ in AsyncThrowingStream { _ in } }, currentAccount: { account })
        }
        let a = session("https://a|1"), b = session("https://b|2")
        #expect(a.ask("first", model: nil) && b.ask("second", model: nil))
        #expect(store.answering.count == 2)
        // One account leaves; the other's answer carries on.
        store.accountsChanged(to: ["https://b|2"])
        #expect(store.answering == [ChatHistoryScope.key(accountIdentity: "https://b|2", noteID: nil)!])
        #expect(!a.busy && b.busy)
        // Erasing an account removes its saved conversations and nobody else's.
        store.save(messages: [ChatMessage(role: .user, text: "kept")], history: [], for: ChatHistoryScope.key(accountIdentity: "https://b|2", noteID: nil)!)
        store.save(messages: [ChatMessage(role: .user, text: "gone")], history: [], for: ChatHistoryScope.key(accountIdentity: "https://a|1", noteID: nil)!)
        store.erase(accountIdentity: "https://a|1")
        #expect(store.load(for: ChatHistoryScope.key(accountIdentity: "https://a|1", noteID: nil)!).messages.isEmpty)
        #expect(store.load(for: ChatHistoryScope.key(accountIdentity: "https://b|2", noteID: nil)!).messages.map(\.text) == ["kept"])
        store.accountsChanged(to: [])
    }
}
