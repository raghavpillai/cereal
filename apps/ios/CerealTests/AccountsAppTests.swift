import AVFoundation
import Foundation
import Testing
import APIClient
import Contracts
@testable import Cereal

/// The account rules as the app applies them: a recording that outlives its
/// session, a note deleted on its server, the sheets, and one account looking as it always has.
@MainActor
@Suite(.serialized)
struct AccountLifecycleTests {
    @Test func s3_aSessionThatEndsDuringARecordingKeepsTheCaptureAndUploadsAfterSignIn() async throws {
        UserDefaults.standard.set(2, forKey: "capture.partSeconds")
        defer { UserDefaults.standard.removeObject(forKey: "capture.partSeconds") }
        let harness = try await LiveSessionHarness()
        defer { try? FileManager.default.removeItem(at: harness.root) }
        harness.live.refusal = .refused(status: 503)
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
        try await waitFor("the first part") { harness.instance.allUploads.count == 1 }

        // The server ends the session in the middle of the meeting.
        harness.instance.set(refused: ["unit-test-token"])
        for _ in 0..<4 {
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
        // Nothing more is sent with the refused session.
        let sent = harness.instance.allRequests.count
        try await Task.sleep(for: .milliseconds(300))
        #expect(harness.instance.allRequests.count == sent)
        #expect(harness.instance.allUploads.count == 1)

        // Stop works, and finalises the local files.
        controller.stop()
        #expect(controller.recorder.state == .stopped)
        #expect(controller.recorder.capturedFrames == 8 * 16_000)
        try await waitFor("the remaining parts to be cut") { !controller.isSyncing }
        #expect(controller.hasPendingAudio(ref))
        #expect(controller.removalBlock(for: account) == nil)
        let staged = try AudioOutbox(instance: try #require(server.instance), principalID: StubInstance.principal, root: harness.root.appendingPathComponent("outbox"))
            .entries(noteID: ref.id)
        #expect(staged.map(\.sequence) == [0, 1, 2, 3] && staged.map(\.startMs) == [0, 2_000, 4_000, 6_000])
        #expect(harness.instance.count("POST", suffix: "/recording") == 1) // the Stop has not reached the server

        // Signing in again as the same user: the note's parts are sent with the identities they always had.
        await server.connect(instance: "https://example.test", token: "second-session")
        #expect(server.status == .ready && server.accountID == account)
        await controller.sync(with: [server])
        try await waitFor("every part on the server") { harness.instance.allUploads.count == 4 }
        #expect(harness.instance.allUploads.map(\.chunkID) == staged.map(\.id))
        #expect(harness.instance.allUploads.map(\.startMs) == [0, 2_000, 4_000, 6_000])
        try await waitFor("the saved Stop") { harness.instance.count("POST", suffix: "/recording") == 2 }
        try await waitFor("delivery to settle") { !controller.hasPendingAudio(ref) }
        #expect(Set(harness.instance.allUploads.map(\.chunkID)).count == 4)
    }

    @Test func v4_aRecordingIsMarkedDeletedOnTheServerOnlyWhenTheNoteItselfIsGoneAndNothingIsErased() async throws {
        UserDefaults.standard.set(2, forKey: "capture.partSeconds")
        defer { UserDefaults.standard.removeObject(forKey: "capture.partSeconds") }
        let harness = try await LiveSessionHarness()
        defer { try? FileManager.default.removeItem(at: harness.root) }
        harness.live.refusal = .refused(status: 503)
        let controller = harness.controller
        let ref = try #require(controller.ref(harness.instance.noteID))
        // The server takes no audio for now, so the recording's parts stay on the phone.
        harness.instance.set(upload: 503)
        await controller.start(note: try await harness.note())
        var time = 0.0
        for _ in 0..<3 {
            time = harness.input.feed(format: InjectedAudio.builtIn, from: time, seconds: 1)
            try await Task.sleep(for: .milliseconds(120))
        }
        controller.stop()
        try await waitFor("delivery to give up") { !controller.isSyncing && controller.hasPendingAudio(ref) }
        let outbox = harness.root.appendingPathComponent("outbox")
        func audioFiles() -> [String] { harness.persistedFiles().filter { $0.pathExtension == "wav" }.map(\.lastPathComponent).sorted() }
        let files = audioFiles()
        #expect(files.count == 3) // the capture and its two parts

        // A 404 from another endpoint, a failed upload, a server that is down: none of them says the note is gone.
        harness.instance.set(chunks: 404)
        controller.retry(ref)
        try await waitFor("the retry") { !controller.isSyncing && controller.error != nil }
        #expect(!controller.isDeletedOnServer(ref) && controller.hasPendingAudio(ref))

        // The note itself answers 404: the recording is marked, and everything stays on this iPhone.
        harness.instance.set(note: 404)
        controller.retry(ref)
        try await waitFor("the mark") { controller.isDeletedOnServer(ref) }
        try await waitFor("delivery to settle") { !controller.isSyncing }
        #expect(controller.status == ServerRecordingController.deletedOnServer)
        #expect(controller.hasPendingAudio(ref) && audioFiles() == files)
        let directory = try #require(try FileManager.default.contentsOfDirectory(atPath: outbox.path).first)
        #expect(AccountStorage(root: outbox).summary(directory) == AccountDataSummary(recordings: 1, unsent: 1, deletedOnServer: 1))

        // At the next launch it is asked about once, and not sent again by itself.
        harness.instance.set(upload: .some(nil), chunks: .some(nil))
        let puts = harness.instance.count("PUT", suffix: "")
        let relaunched = ServerRecordingController(outboxRoot: outbox, makeBackend: { EngineCaptureBackend(input: harness.input) })
        await relaunched.sync(with: [harness.server])
        #expect(relaunched.isDeletedOnServer(ref) && relaunched.hasPendingAudio(ref))
        #expect(harness.instance.allUploads.isEmpty && harness.instance.count("PUT", suffix: "") == puts)
        #expect(audioFiles() == files)

        // The note is on the server again (restored there): the mark goes and its audio is delivered.
        harness.instance.set(note: .some(nil))
        let later = ServerRecordingController(outboxRoot: outbox, makeBackend: { EngineCaptureBackend(input: harness.input) })
        await later.sync(with: [harness.server])
        try await waitFor("the audio after all") { harness.instance.allUploads.count == 2 }
        #expect(!later.isDeletedOnServer(ref))
        #expect(AccountStorage(root: outbox).summary(directory).deletedOnServer == 0)
    }

    @Test func p1_twoAccountsEachDeliverThroughTheirOwnServerAndOutbox() async throws {
        UserDefaults.standard.set(2, forKey: "capture.partSeconds")
        defer { UserDefaults.standard.removeObject(forKey: "capture.partSeconds") }
        // Two servers, one recorder. The second is the one recorded to.
        let first = try await LiveSessionHarness(), second = try await LiveSessionHarness(host: "second.example.test", controller: first.controller)
        defer { try? FileManager.default.removeItem(at: first.root); try? FileManager.default.removeItem(at: second.root) }
        let controller = first.controller
        first.live.refusal = .refused(status: 503)
        await controller.sync(with: [first.server, second.server])
        let a = try #require(first.server.accountID), b = try #require(second.server.accountID)
        #expect(a != b && Set(controller.deliveringAccounts) == [a, b])
        let note = NoteRef(account: b, id: second.instance.noteID)

        await controller.start(note: try await second.note(), in: b)
        #expect(controller.recorder.state == .recording && controller.active == note)
        var time = 0.0
        for _ in 0..<5 {
            time = first.input.feed(format: InjectedAudio.builtIn, from: time, seconds: 1)
            try await Task.sleep(for: .milliseconds(120))
        }
        controller.stop()
        try await waitFor("the recording on the second server") { second.instance.allUploads.count == 3 }
        try await waitFor("the Stop on the second server") { second.instance.count("POST", suffix: "/recording") == 2 }
        // The first server saw nothing of it: no start, no audio, no stop.
        #expect(first.instance.allUploads.isEmpty)
        #expect(first.instance.count("POST", suffix: "/recording") == 0 && first.instance.count("PUT", suffix: "") == 0)
        // Its audio is in the second account's directory and nowhere else.
        let root = first.root.appendingPathComponent("outbox")
        #expect(AccountStorage(root: root).summary(b.raw).recordings == 1 && AccountStorage(root: root).summary(a.raw).recordings == 0)
        #expect(controller.ledger(of: b) == [note.id] && controller.ledger(of: a).isEmpty)
        // The same note id in the first account is another note: nothing of the recording shows there.
        let twin = NoteRef(account: a, id: note.id)
        #expect(!controller.isActive(twin) && !controller.hasPendingAudio(twin) && controller.transcript(for: twin) == nil)
        // Only the account recorded to is held by a recording; the other can leave, and its delivery ends with it.
        #expect(controller.removalBlock(for: a) == nil)
        await controller.sync(with: [second.server])
        #expect(controller.deliveringAccounts == [b])
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
