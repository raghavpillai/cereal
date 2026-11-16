import Foundation
import Testing
import Contracts
@testable import APIClient

/// Creates only fictional typed notes, never records or calls a provider. The
/// harness supplies two temporary regular-user accounts through the environment.
@MainActor @Suite struct LiveAuthTests {
    @Test(.enabled(if: ProcessInfo.processInfo.environment["RUN_NATIVE_AUTH_TESTS"] == "1"))
    func signedNativeSessionsKeepSameServerAccountsAndOutboxesSeparate() async throws {
        let env = ProcessInfo.processInfo.environment
        let base = try #require(env["NATIVE_AUTH_URL"])
        let instance = try InstanceURL(base)
        #expect(["127.0.0.1", "localhost", "::1"].contains(instance.url.host ?? ""))
        let aEmail = try #require(env["NATIVE_AUTH_A_EMAIL"]), aPassword = try #require(env["NATIVE_AUTH_A_PASSWORD"])
        let bEmail = try #require(env["NATIVE_AUTH_B_EMAIL"]), bPassword = try #require(env["NATIVE_AUTH_B_PASSWORD"])
        let domain = "Cereal-native-live-auth-\(UUID().uuidString)"
        let defaults = try #require(UserDefaults(suiteName: domain))
        defer { defaults.removePersistentDomain(forName: domain) }
        let store = MemoryAccountStore()
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(domain)
        defer { try? FileManager.default.removeItem(at: root) }
        let accounts = AccountsModel(defaults: defaults, credentials: store, storage: AccountStorage(root: root), allowsMultiple: true)
        let aAccount = try await accounts.signIn(instance: base, email: aEmail, password: aPassword)
        var model = try #require(accounts.session(aAccount)?.notes)
        let firstIdentity = try #require(model.accountIdentity)
        #expect(model.principalID == env["NATIVE_AUTH_A_ID"])
        #expect(model.session?.user.role == .user)
        let aCredentials = try #require(try store.load(aAccount)).credentials
        let aNote = try await model.create(title: "Native auth QA A", body: "Fictional private canary A")
        let ownAPI = MeetingNotesAPI(credentials: aCredentials)
        let events = try await ownAPI.calendarEvents(from: .now.addingTimeInterval(-86400), to: .now.addingTimeInterval(60 * 86400))
        let event = try #require(events.first)
        let linked = try await model.updateNoteContext(id: aNote.id, eventID: event.id)
        #expect(linked.meeting?.eventId == event.id)
        #expect(try await model.note(id: aNote.id).title == "Native auth QA A")
        #expect(try await model.note(id: aNote.id).recordingState == .idle)
        let manualID = UUID().uuidString.lowercased()
        let manual = ManualParticipantInput(displayName: "Fictional Native QA Attendee", email: "fictional-native@example.invalid", familyName: nil, givenName: nil, id: manualID)
        _ = try await model.addNoteParticipant(id: aNote.id, input: manual)
        let retry = try await model.addNoteParticipant(id: aNote.id, input: manual)
        #expect(retry.meeting?.participants.filter { $0.id == manualID }.count == 1)
        #expect(retry.meeting?.participants.first(where: { $0.id == manualID })?.nameSource == .manual)
        #expect(try await model.note(id: aNote.id).recordingState == .idle)
        let bAccount = try await accounts.signIn(instance: base, email: bEmail, password: bPassword)
        #expect(accounts.sessions.map(\.id) == [aAccount, bAccount])
        model = try #require(accounts.session(bAccount)?.notes)
        #expect(model.accountIdentity != firstIdentity)
        #expect(model.principalID == env["NATIVE_AUTH_B_ID"])
        #expect(!model.state.notes.contains { $0.id == aNote.id })
        await #expect(throws: APIClientError.notFound) { try await model.note(id: aNote.id) }
        await #expect(throws: APIClientError.notFound) { try await model.noteContext(id: aNote.id) }
        let bCredentials = try #require(try store.load(bAccount)).credentials
        let bNote = try await model.create(title: "Native auth QA B", body: "Fictional private canary B")
        let aAPI = MeetingNotesAPI(credentials: aCredentials)
        await #expect(throws: APIClientError.notFound) { try await aAPI.note(id: bNote.id) }
        let bAPI = MeetingNotesAPI(credentials: bCredentials)
        #expect(try await bAPI.calendarPreferences().lookAheadDays > 0)
        #expect(try await aAPI.updateNoteContext(id: aNote.id, eventID: nil).meeting == nil)
        let defaultNote = try await aAPI.createNote(CreateNoteInput(body: "Fictional canonical-title QA", date: nil, duration: nil, enhanced: nil, title: "New note", transcriptSegments: nil))
        _ = try await aAPI.recording(noteID: defaultNote.id, command: .start)
        _ = try await aAPI.recording(noteID: defaultNote.id, command: .stop)
        _ = try await aAPI.updateNoteContext(id: defaultNote.id, eventID: event.id)
        let renamed = try await aAPI.note(id: defaultNote.id)
        #expect(renamed.title == event.title)
        #expect(renamed.recordingState == .stopped)
        #expect(try await aAPI.noteForCalendarEvent(id: event.id).id == defaultNote.id)
        _ = try await aAPI.deleteNote(id: defaultNote.id)
        _ = try await aAPI.deleteNote(id: aNote.id)
        _ = try await bAPI.deleteNote(id: bNote.id)
        try await accounts.signOut(bAccount)
        #expect(model.accountIdentity == nil && model.state.notes.isEmpty)
        // The session is ended on its server in the background.
        for _ in 0..<50 {
            if case .refused = await bAPI.verifySession() { break }
            try await Task.sleep(for: .milliseconds(100))
        }
        await #expect(throws: APIClientError.unauthorized) { try await bAPI.session() }
        try await aAPI.signOut()
    }
}
