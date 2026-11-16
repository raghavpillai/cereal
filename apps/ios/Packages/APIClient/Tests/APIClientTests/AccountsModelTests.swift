import Foundation
import Testing
import Contracts
@testable import APIClient

// A pretend server per host. Each test makes its own hosts, so tests can run side by side.

private struct Seen: Sendable { let host: String; let path: String; let method: String; let token: String? }

private final class FakeServer: @unchecked Sendable {
    /// `portal`: a page with 403 on every endpoint but `/session`. `proxy`: the same, and a page with 401 on `/session` too.
    /// `refusesNotes`: the API's own JSON 401 on every endpoint but `/session`.
    enum Mode { case up, down, portal, proxy, refusesNotes }
    private let lock = NSLock()
    let host: String
    private var users: [String: (id: String, email: String)] = [:]
    private var refused: Set<String> = []
    private var mode = Mode.up
    private var seen: [Seen] = []
    /// Titles of the notes each user has.
    private var notes: [String: [String]] = [:]
    /// How the server spells a user's id.
    var spelling: @Sendable (String) -> String = { $0 }

    init(_ name: String = "server") {
        host = "\(name)-\(UUID().uuidString.lowercased().prefix(8)).example.test"
        FakeNetwork.shared.add(self)
    }

    var address: String { "https://\(host)" }

    func user(_ token: String, id: String, email: String, notes titles: [String] = []) {
        lock.lock(); defer { lock.unlock() }
        users[token] = (id, email)
        notes[id] = titles
    }
    func refuse(_ token: String) { lock.lock(); defer { lock.unlock() }; refused.insert(token) }
    func set(_ value: Mode) { lock.lock(); defer { lock.unlock() }; mode = value }
    var requests: [Seen] { lock.lock(); defer { lock.unlock() }; return seen }
    func requests(with token: String) -> [Seen] { requests.filter { $0.token == token } }

    func answer(_ request: URLRequest) -> (status: Int, type: String, body: String)? {
        lock.lock(); defer { lock.unlock() }
        let path = request.url?.path ?? ""
        let token = request.value(forHTTPHeaderField: "Authorization").map { String($0.dropFirst("Bearer ".count)) }
        seen.append(Seen(host: host, path: path, method: request.httpMethod ?? "GET", token: token))
        if mode == .down { return nil }
        if path.hasSuffix("/api/auth/sign-out") {
            if let token { refused.insert(token) }
            return (200, "application/json", "{}")
        }
        let isSession = path.hasSuffix("/api/v1/session")
        if mode == .portal || mode == .proxy, !isSession { return (403, "text/html", "<html>Sign in to the guest network</html>") }
        if mode == .proxy, isSession { return (401, "text/html", "<html>Proxy sign-in</html>") }
        if mode == .refusesNotes, !isSession { return (401, "application/json", #"{"error":"unauthorized","message":"Authentication required"}"#) }
        guard let token, !refused.contains(token), let user = users[token] else {
            return (401, "application/json", #"{"error":"unauthorized","message":"Authentication required"}"#)
        }
        if isSession {
            return (200, "application/json", #"{"user":{"id":"\#(spelling(user.id))","name":"Person","email":"\#(user.email)","role":"owner","createdAt":"2026-11-12T00:00:00Z"},"expiresAt":null}"#)
        }
        if path.hasSuffix("/notes/summaries") {
            let rows = (notes[user.id] ?? []).enumerated().map { index, title in
#"{"id":"\#(UUID().uuidString.lowercased())","title":"\#(title)","date":"2026-11-1\#(index % 9)","duration":"1:12","createdAt":"2026-11-12T10:00:00.000Z","updatedAt":"2026-11-12T10:01:00.123Z","recordingState":"stopped","generationState":"ready","transcriptRevision":2,"generatedRevision":2,"hasBody":true,"hasSummary":true,"hasTranscript":true,"speakerCount":1,"excerpt":"Generated notes.","meeting":null,"attendees":{"people":[],"others":0,"total":0},"organizationId":null,"folders":[]}"#
            }
            return (200, "application/json", #"{"notes":[\#(rows.joined(separator: ","))],"nextCursor":null,"total":\#(rows.count)}"#)
        }
        return (404, "application/json", #"{"error":"not_found"}"#)
    }
}

private final class FakeNetwork: @unchecked Sendable {
    static let shared = FakeNetwork()
    private let lock = NSLock()
    private var servers: [String: FakeServer] = [:]
    func add(_ server: FakeServer) { lock.lock(); defer { lock.unlock() }; servers[server.host] = server }
    func server(_ host: String) -> FakeServer? { lock.lock(); defer { lock.unlock() }; return servers[host] }
}

private final class FakeProtocol: URLProtocol, @unchecked Sendable {
    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func startLoading() {
        guard let host = request.url?.host, let server = FakeNetwork.shared.server(host), let answer = server.answer(request) else {
            client?.urlProtocol(self, didFailWithError: URLError(.cannotConnectToHost)); return
        }
        let http = HTTPURLResponse(url: request.url!, statusCode: answer.status, httpVersion: "HTTP/1.1", headerFields: ["Content-Type": answer.type])!
        client?.urlProtocol(self, didReceive: http, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: Data(answer.body.utf8))
        client?.urlProtocolDidFinishLoading(self)
    }
    override func stopLoading() {}
}

private let fakeAPI: @Sendable (InstanceCredentials) -> MeetingNotesAPI = { credentials in
    let configuration = URLSessionConfiguration.ephemeral
    configuration.protocolClasses = [FakeProtocol.self]
    return MeetingNotesAPI(credentials: credentials, configuration: configuration)
}

private final class LegacyTokens: BearerTokenStore, @unchecked Sendable {
    private let lock = NSLock()
    private var token: String?
    init(_ token: String? = nil) { self.token = token }
    func load() throws -> String? { lock.lock(); defer { lock.unlock() }; return token }
    func save(_ token: String) throws { lock.lock(); defer { lock.unlock() }; self.token = token }
    func clear() throws { lock.lock(); defer { lock.unlock() }; token = nil }
}

private let userA = "00000000-0000-4000-8000-00000000000a"
private let userB = "00000000-0000-4000-8000-00000000000b"

/// A device: its preferences, its Keychain, its files. Making the model again is a relaunch.
@MainActor private final class Device {
    let name = "accounts-" + UUID().uuidString
    let defaults: UserDefaults
    let store = MemoryAccountStore()
    let legacyTokens = LegacyTokens()
    let root: URL
    lazy var legacy = ConnectionSettingsStore(defaults: defaults, tokens: legacyTokens)

    init() {
        defaults = UserDefaults(suiteName: name)!
        root = FileManager.default.temporaryDirectory.appendingPathComponent(name, isDirectory: true)
    }
    deinit {
        UserDefaults().removePersistentDomain(forName: name)
        try? FileManager.default.removeItem(at: root)
    }

    func launch(multiple: Bool = false) -> AccountsModel {
        AccountsModel(defaults: defaults, credentials: store, legacy: legacy, storage: AccountStorage(root: root),
                      allowsMultiple: multiple, retryDelays: [.seconds(3600)], makeAPI: fakeAPI)
    }

    var registry: String? { defaults.data(forKey: AccountsModel.registryKey).map { String(decoding: $0, as: UTF8.self) } }

    /// What the earlier version left: its one session, and recordings under its identity.
    func leaveLegacySession(address: String, token: String) throws {
        try legacyTokens.save(token)
        defaults.set(address, forKey: "meetingNotes.instanceURL")
    }

    @discardableResult
    func leaveRecording(address: String, userID: String, state: AudioOutboxEntry.DeliveryState = .pending) throws -> URL {
        let directory = root.appendingPathComponent(AccountID(address: address, userID: userID).raw, isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let id = UUID().uuidString.lowercased()
        let entry = AudioOutboxEntry(id: id, noteID: UUID().uuidString.lowercased(), channel: .mic, sequence: 0, startMs: 0,
                                     durationMs: 5000, fileName: "\(id).wav", sourceFileName: "capture-0.wav", state: state)
        var entries = (try? JSONDecoder().decode([AudioOutboxEntry].self, from: Data(contentsOf: directory.appendingPathComponent("outbox.json")))) ?? []
        entries.append(entry)
        try JSONEncoder().encode(entries).write(to: directory.appendingPathComponent("outbox.json"))
        try Data("RIFF".utf8).write(to: directory.appendingPathComponent("\(id).wav"))
        return directory
    }
}

private func isUnreachable(_ status: AccountStatus) -> Bool { if case .unreachable = status { true } else { false } }

@MainActor @Suite struct AccountsModelTests {
    // MARK: Welcome and migration

    @Test func w1_freshInstallWritesNoRegistryAndStartsSignedOut() async throws {
        let device = Device()
        let accounts = device.launch()
        await accounts.restoreAll()
        #expect(accounts.sessions.isEmpty && accounts.defaultSession == nil)
        #expect(device.registry == nil)
        #expect(device.store.count == 0)
        #expect(!FileManager.default.fileExists(atPath: device.root.path))
        #expect(accounts.signedOut.isEmpty && accounts.olderData.isEmpty)
    }

    @Test func a1_migrationWorksOfflineThenFinishesOnlineAndKeepsTheEarlierItemInStep() async throws {
        let device = Device(), server = FakeServer()
        server.user("legacy-token", id: userA, email: "a@example.test", notes: ["Planning"])
        try device.leaveLegacySession(address: server.address, token: "legacy-token")
        let recordings = try device.leaveRecording(address: server.address, userID: userA)
        server.set(.down)

        // Offline: an entry from the saved address alone. No notes, nothing secret written, nothing lost.
        let accounts = device.launch()
        await accounts.restoreAll()
        let session = try #require(accounts.sessions.first)
        #expect(accounts.sessions.count == 1 && session.record.userID == nil)
        #expect(session.record.instanceURL == server.address && session.record.label == server.host)
        #expect(isUnreachable(session.status) && session.notes.list.rows.isEmpty)
        #expect(accounts.defaultSession === session)
        #expect(device.store.count == 0)
        #expect(try device.legacy.load()?.bearerToken == "legacy-token")
        #expect(device.registry?.contains("legacy-token") == false)
        // Which directory is the account's is not known yet, so none is offered for erasing.
        #expect(accounts.olderData.isEmpty)

        // Online: the account gets its id (the directory it already has), its own item, read back, and its notes.
        server.set(.up)
        await accounts.retryUnreachable()
        let id = AccountID(address: server.address, userID: userA)
        #expect(session.status == .ready && session.id == id && session.record.userID == userA)
        #expect(session.record.email == "a@example.test")
        #expect(session.notes.list.rows.map(\.title) == ["Planning"])
        let item = try #require(try device.store.load(id))
        #expect(item.credentials.bearerToken == "legacy-token" && item.userID == userA && item.credentials.instance.description == server.address)
        #expect(device.store.ids == [id])
        // This release keeps the earlier item and address, in step.
        #expect(try device.legacy.load()?.bearerToken == "legacy-token")
        #expect(device.defaults.string(forKey: "meetingNotes.instanceURL") == server.address)
        #expect(device.registry?.contains("legacy-token") == false && device.registry?.contains(id.raw) == true)
        // The recordings made before the upgrade are this account's, where they were.
        #expect(try accounts.storage.directory(id.raw).path == recordings.path)
        #expect(accounts.dataSummary(for: id) == AccountDataSummary(recordings: 1, unsent: 1))
        #expect(FileManager.default.fileExists(atPath: recordings.appendingPathComponent("account.json").path))
        #expect(accounts.olderData.isEmpty && accounts.signedOut.isEmpty)
        #expect(accounts.defaultID == id)
    }

    @Test func a2_aCrashAfterAnyMigrationStepEndsInTheSameStateAndATokenIsNeverAbsent() async throws {
        let server = FakeServer()
        server.user("legacy-token", id: userA, email: "a@example.test")
        let id = AccountID(address: server.address, userID: userA)

        /// The state a finished migration leaves, whatever happened on the way.
        func finished(_ device: Device, _ accounts: AccountsModel) throws {
            #expect(accounts.sessions.map(\.id) == [id])
            #expect(accounts.sessions.first?.status == .ready)
            #expect(try device.store.load(id)?.credentials.bearerToken == "legacy-token")
            #expect(try device.legacy.load()?.bearerToken == "legacy-token")
            #expect(device.store.ids == [id])
        }

        // Killed after the provisional entry was written (step 1).
        do {
            let device = Device()
            try device.leaveLegacySession(address: server.address, token: "legacy-token")
            server.set(.down)
            await device.launch().restoreAll()
            #expect(try device.legacy.load() != nil && device.registry != nil)
            server.set(.up)
            let relaunched = device.launch()
            await relaunched.restoreAll()
            try finished(device, relaunched)
        }
        // Killed after the new item was written and before the entry was (step 3, half done).
        do {
            let device = Device()
            try device.leaveLegacySession(address: server.address, token: "legacy-token")
            server.set(.down)
            await device.launch().restoreAll()
            try device.store.save(InstanceCredentials(instance: InstanceURL(server.address), bearerToken: "legacy-token"), userID: userA, for: id)
            server.set(.up)
            let relaunched = device.launch()
            await relaunched.restoreAll()
            try finished(device, relaunched)
        }
        // Killed when the new item could not be written: the session still works from the earlier one, and is finished later.
        do {
            let device = Device()
            try device.leaveLegacySession(address: server.address, token: "legacy-token")
            device.store.failsToSave = true
            let first = device.launch()
            await first.restoreAll()
            #expect(first.sessions.count == 1 && first.sessions.first?.record.userID == nil)
            #expect(try device.legacy.load()?.bearerToken == "legacy-token")
            device.store.failsToSave = false
            let relaunched = device.launch()
            await relaunched.restoreAll()
            try finished(device, relaunched)
        }
        // Finished, then launched again and again: nothing changes.
        do {
            let device = Device()
            try device.leaveLegacySession(address: server.address, token: "legacy-token")
            await device.launch().restoreAll()
            let before = device.registry
            let relaunched = device.launch()
            await relaunched.restoreAll()
            try finished(device, relaunched)
            #expect(device.registry == before)
        }
    }

    @Test func a3_anItemForAnotherAddressOrUserNeedsSignInAndNothingIsSent() async throws {
        for wrong in ["address", "user"] {
            let device = Device(), server = FakeServer(), other = FakeServer("other")
            server.user("token-a", id: userA, email: "a@example.test", notes: ["Private"])
            other.user("token-a", id: userA, email: "a@example.test")
            let accounts = device.launch()
            let id = try await accounts.connect(instance: server.address, token: "token-a")
            // The item now says something the account's entry does not.
            if wrong == "address" {
                try device.store.save(InstanceCredentials(instance: InstanceURL(other.address), bearerToken: "token-a"), userID: userA, for: id)
            } else {
                try device.store.save(InstanceCredentials(instance: InstanceURL(server.address), bearerToken: "token-a"), userID: userB, for: id)
            }
            let before = (server.requests.count, other.requests.count)
            let relaunched = device.launch()
            await relaunched.restoreAll()
            let session = try #require(relaunched.session(id))
            #expect(session.status == .needsSignIn, "wrong \(wrong)")
            #expect(session.notes.list.rows.isEmpty)
            #expect((server.requests.count, other.requests.count) == before, "wrong \(wrong): something was sent")
            await #expect(throws: APIClientError.unauthorized) { try await session.notes.create(title: "x", body: "") }
            #expect((server.requests.count, other.requests.count) == before)
        }
    }

    @Test func a4_anUnreadableRegistryIsKeptAsideAndRebuiltWithoutMigratingAgain() async throws {
        let device = Device(), server = FakeServer(), old = FakeServer("old")
        server.user("token-a", id: userA, email: "a@example.test", notes: ["Kept"])
        old.user("old-token", id: userB, email: "b@example.test")
        let id = try await device.launch().connect(instance: server.address, token: "token-a")
        // What an earlier version would have left for another server must not come back as an account.
        try device.legacyTokens.save("old-token")
        device.defaults.set(old.address, forKey: "meetingNotes.instanceURL")
        device.defaults.set(Data("{not json".utf8), forKey: AccountsModel.registryKey)

        let accounts = device.launch()
        await accounts.restoreAll()
        #expect(accounts.sessions.map(\.id) == [id])
        #expect(accounts.sessions.first?.status == .ready)
        #expect(accounts.storageError != nil)
        let aside = device.defaults.dictionaryRepresentation().keys.filter { $0.hasPrefix("cereal.accounts.unreadable-") }
        #expect(aside.count == 1)
        #expect(old.requests(with: "old-token").allSatisfy { $0.path.hasSuffix("/session") })
    }

    @Test func a5_o2_aStraySessionOfTheEarlierVersionOnlyReplacesAKnownAccountAndNeverAddsOne() async throws {
        let device = Device(), server = FakeServer(), planted = FakeServer("planted")
        server.user("token-1", id: userA, email: "a@example.test")
        server.user("token-2", id: userA, email: "a@example.test")
        planted.user("planted-token", id: userB, email: "b@example.test")
        let id = try await device.launch().connect(instance: server.address, token: "token-1")

        // The earlier version, run by hand, signed the same person in again.
        try device.leaveLegacySession(address: server.address, token: "token-2")
        var accounts = device.launch()
        await accounts.restoreAll()
        #expect(accounts.sessions.map(\.id) == [id] && accounts.sessions.first?.status == .ready)
        #expect(try device.store.load(id)?.credentials.bearerToken == "token-2")

        // Something else put an item there for an account that is not here: it is deleted and adds nothing.
        try device.leaveLegacySession(address: planted.address, token: "planted-token")
        accounts = device.launch()
        await accounts.restoreAll()
        #expect(accounts.sessions.map(\.id) == [id])
        #expect(try device.store.load(id)?.credentials.bearerToken == "token-2")
        #expect(device.store.ids == [id])
        // The earlier item follows the one account again.
        #expect(try device.legacy.load()?.instance.description == server.address)
    }

    @Test func a6_aTokenSavedBeforeItsAddressIsVerifiedAndDiscardedWhenRefused() async throws {
        // The earlier version saved the token first and the address second. Killed
        // in between, it leaves the new server's token beside the old server's address.
        let device = Device(), oldServer = FakeServer("old"), newServer = FakeServer("new")
        oldServer.user("old-token", id: userA, email: "a@example.test", notes: ["Old server note"])
        newServer.user("new-token", id: userB, email: "b@example.test", notes: ["New server note"])
        try device.leaveLegacySession(address: oldServer.address, token: "new-token")

        let accounts = device.launch()
        await accounts.restoreAll()
        // The pair is asked of the server it names, which refuses it: it is discarded, not retried.
        #expect(accounts.sessions.isEmpty)
        #expect(device.registry == nil && device.store.count == 0)
        #expect(try device.legacy.load() == nil)
        #expect(oldServer.requests.map(\.path) == ["/api/v1/session"])
        // The token never went to a server other than the one saved beside it.
        #expect(newServer.requests.isEmpty)

        let relaunched = device.launch()
        await relaunched.restoreAll()
        #expect(relaunched.sessions.isEmpty && oldServer.requests.count == 1)
    }

    // MARK: Identity

    @Test func i1_signingInAgainReplacesTheTokenAndKeepsTheStoredAddressAndDirectory() async throws {
        let device = Device(), server = FakeServer()
        server.user("token-1", id: userA, email: "a@example.test")
        server.user("token-2", id: userA, email: "a@example.test")
        let accounts = device.launch()
        let id = try await accounts.connect(instance: server.address, token: "token-1")
        let directory = try device.leaveRecording(address: server.address, userID: userA)

        // The same server, typed with other capitals and its usual port spelled out.
        let again = try await accounts.connect(instance: "HTTPS://" + server.host.uppercased() + ":443/", token: "token-2")
        #expect(again == id && accounts.sessions.count == 1)
        #expect(accounts.sessions.first?.record.instanceURL == server.address)
        let item = try #require(try device.store.load(id))
        #expect(item.credentials.bearerToken == "token-2" && item.credentials.instance.description == server.address)
        #expect(try accounts.storage.directory(id.raw).path == directory.path)
        #expect(accounts.sessions.first?.status == .ready)
        // The session that was replaced is ended on its server.
        for _ in 0..<100 where !server.requests.contains(where: { $0.path.hasSuffix("/sign-out") }) { try await Task.sleep(for: .milliseconds(10)) }
        #expect(server.requests.filter { $0.path.hasSuffix("/sign-out") }.map(\.token) == ["token-1"])
    }

    @Test func i2_aUserIdSpelledAnotherWayIsTheSameAccount() async throws {
        #expect(AccountID.canonicalUserID("00000000000040008000ABCDEF000001") == "00000000-0000-4000-8000-abcdef000001")
        #expect(AccountID.canonicalUserID("{00000000-0000-4000-8000-ABCDEF000001}") == "00000000-0000-4000-8000-abcdef000001")
        #expect(AccountID.canonicalUserID("not-an-id") == nil && AccountID.canonicalUserID("0000|0000") == nil)

        let device = Device(), server = FakeServer()
        server.user("token-1", id: userA, email: "a@example.test")
        let accounts = device.launch()
        let id = try await accounts.connect(instance: server.address, token: "token-1")
        // A typed pattern on the loop variable crashes the type checker; name the list instead.
        let spellings: [@Sendable (String) -> String] = [{ $0.uppercased() }, { $0.replacingOccurrences(of: "-", with: "") }, { "{" + $0 + "}" }]
        for spelling in spellings {
            server.spelling = spelling
            #expect(try await accounts.connect(instance: server.address, token: "token-1") == id)
            let relaunched = device.launch()
            await relaunched.restoreAll()
            #expect(relaunched.sessions.map(\.id) == [id] && relaunched.sessions.first?.status == .ready)
        }
        #expect(accounts.sessions.count == 1 && device.store.ids == [id])
        #expect(accounts.sessions.first?.notes.principalID == userA)
    }

    @Test func i6_signingInAgainAsSomebodyElseIsRefused() async throws {
        let device = Device(), server = FakeServer()
        server.user("token-a", id: userA, email: "a@example.test")
        server.user("token-b", id: userB, email: "b@example.test")
        let accounts = device.launch(multiple: true)
        let id = try await accounts.connect(instance: server.address, token: "token-a")
        let other = try InstanceCredentials(instance: InstanceURL(server.address), bearerToken: "token-b")
        await #expect(throws: AccountsError.otherUser(signedInAs: "b@example.test", expected: "a@example.test")) {
            try await accounts.signInAgain(id, with: other)
        }
        #expect(try device.store.load(id)?.credentials.bearerToken == "token-a")
        #expect(accounts.sessions.map(\.id) == [id])
        // Adding it as another account is a separate, explicit step.
        let second = try await accounts.add(other)
        #expect(second != id && accounts.sessions.map(\.id) == [id, second])
    }

    @Test func newAddressesWithABarAreRefusedAndTheRegistryHoldsNoSecret() async throws {
        let device = Device(), server = FakeServer()
        server.user("secret-session-token", id: userA, email: "a@example.test")
        let accounts = device.launch()
        await #expect(throws: (any Error).self) { try await accounts.connect(instance: server.address + "/a|b", token: "secret-session-token") }
        #expect(accounts.sessions.isEmpty && device.registry == nil)
        _ = try await accounts.connect(instance: server.address, token: "secret-session-token")
        let everything = device.defaults.persistentDomain(forName: device.name) ?? [:]
        let text = everything.map { key, value in key + ((value as? Data).map { String(decoding: $0, as: UTF8.self) } ?? "\(value)") }.joined()
        #expect(!text.contains("secret-session-token"))
        #expect(AccountID("../../Library") == nil && AccountID(String(repeating: "A", count: 64)) == nil)
    }

    // MARK: Lifecycle

    @Test func s1_onlyTheAPIsOwnRefusalFromSessionEndsTheSession() async throws {
        let device = Device(), server = FakeServer()
        server.user("token-a", id: userA, email: "a@example.test", notes: ["Mine"])
        let accounts = device.launch()
        let id = try await accounts.connect(instance: server.address, token: "token-a")
        let session = try #require(accounts.session(id))
        /// Every refused request is followed by one question to `/session`; wait for its answer to be acted on.
        func settle() async throws {
            let asked = server.requests.count
            for _ in 0..<1000 where server.requests.last?.path != "/api/v1/session" || server.requests.count < asked { try await Task.sleep(for: .milliseconds(5)) }
            for _ in 0..<20 { await Task.yield() }
            try await Task.sleep(for: .milliseconds(100))
        }

        // A captive portal's page, 403 and HTML, on every endpoint but `/session`.
        server.set(.portal)
        await session.notes.refresh()
        try await settle()
        #expect(session.status == .ready)
        #expect(server.requests.last?.path == "/api/v1/session", "a refused request asks /session once: \(server.requests.map(\.path))")
        #expect(session.notes.list.rows.map(\.title) == ["Mine"])

        // A proxy answering `/session` itself with 401 and a page: still not the API's refusal.
        server.set(.proxy)
        await session.notes.refresh()
        try await settle()
        #expect(session.status == .ready)

        // The API's own JSON 401 from another endpoint, while `/session` still accepts: nothing changes.
        server.set(.refusesNotes)
        await session.notes.refresh()
        try await settle()
        #expect(session.status == .ready)
        server.set(.up)
        await session.notes.refresh()
        #expect(session.status == .ready)
        if case .loaded = session.notes.state {} else { Issue.record("the account did not recover: \(session.notes.state)") }

        // `/session` itself refuses, in the API's words: the account needs a sign-in, and what was on screen stays.
        server.refuse("token-a")
        await session.notes.refresh()
        try await settle()
        #expect(session.status == .needsSignIn)
        #expect(session.notes.list.rows.map(\.title) == ["Mine"])
        #expect(session.notes.instance?.description == server.address)
        let sent = server.requests.count
        await session.notes.refresh()
        await #expect(throws: APIClientError.unauthorized) { try await session.notes.create(title: "x", body: "") }
        #expect(server.requests.count == sent, "nothing is sent with a refused session")
        // The account is still here, with its item, until the person signs in again or removes it.
        #expect(accounts.sessions.map(\.id) == [id] && device.store.count == 1)

        // Signing in again as the same user brings it back.
        server.user("token-a2", id: userA, email: "a@example.test", notes: ["Mine"])
        let generation = session.notes.sessionGeneration
        #expect(try await accounts.connect(instance: server.address, token: "token-a2") == id)
        #expect(session.status == .ready && session.notes.sessionGeneration == generation + 1)
    }

    @Test func s1_aSessionRefusedAtLaunchNeedsSignInAndShowsNoNotes() async throws {
        let device = Device(), server = FakeServer()
        server.user("token-a", id: userA, email: "a@example.test", notes: ["Mine"])
        let id = try await device.launch().connect(instance: server.address, token: "token-a")
        server.refuse("token-a")
        let accounts = device.launch()
        await accounts.restoreAll()
        let session = try #require(accounts.session(id))
        #expect(session.status == .needsSignIn && session.notes.list.rows.isEmpty)
        #expect(session.notes.instance?.description == server.address)
    }

    @Test func s4_signOutKeepsTheDirectoryAndSigningInAgainFindsIt() async throws {
        let device = Device(), server = FakeServer()
        server.user("token-1", id: userA, email: "a@example.test")
        server.user("token-2", id: userA, email: "a@example.test")
        let accounts = device.launch()
        let id = try await accounts.connect(instance: server.address, token: "token-1")
        let directory = try device.leaveRecording(address: server.address, userID: userA)
        try device.leaveRecording(address: server.address, userID: userA, state: .ready)
        #expect(accounts.dataSummary(for: id) == AccountDataSummary(recordings: 2, unsent: 1))

        try await accounts.signOut(id)
        #expect(accounts.sessions.isEmpty && device.store.count == 0 && device.registry == nil)
        #expect(try device.legacy.load() == nil)
        #expect(FileManager.default.fileExists(atPath: directory.appendingPathComponent("outbox.json").path))
        // Settings lists it, from the plain file in its directory.
        #expect(accounts.signedOut.map(\.directory) == [id.raw])
        #expect(accounts.signedOut.first?.host == server.host && accounts.signedOut.first?.account?.email == "a@example.test")
        #expect(accounts.signedOut.first?.summary == AccountDataSummary(recordings: 2, unsent: 1))
        for _ in 0..<100 where !server.requests.contains(where: { $0.path.hasSuffix("/sign-out") }) { try await Task.sleep(for: .milliseconds(10)) }
        #expect(server.requests.filter { $0.path.hasSuffix("/sign-out") }.map(\.token) == ["token-1"])

        let again = try await accounts.connect(instance: server.address, token: "token-2")
        #expect(again == id && accounts.signedOut.isEmpty)
        #expect(accounts.dataSummary(for: id) == AccountDataSummary(recordings: 2, unsent: 1))
    }

    @Test func s6_eraseRemovesTheDirectoryAndItemAndAPathLikeNameIsRefused() async throws {
        let device = Device(), server = FakeServer()
        server.user("token-1", id: userA, email: "a@example.test")
        let accounts = device.launch()
        let id = try await accounts.connect(instance: server.address, token: "token-1")
        let directory = try device.leaveRecording(address: server.address, userID: userA)
        let neighbour = try device.leaveRecording(address: "https://elsewhere.example.test", userID: userB)
        var removed: [(AccountRecord, Bool)] = []
        accounts.onRemoved = { removed.append(($0, $1)) }

        try await accounts.erase(id)
        #expect(!FileManager.default.fileExists(atPath: directory.path))
        #expect(FileManager.default.fileExists(atPath: neighbour.path))
        #expect(accounts.sessions.isEmpty && device.store.count == 0 && device.registry == nil)
        #expect(accounts.signedOut.isEmpty)
        #expect(removed.count == 1 && removed.first?.0.id == id && removed.first?.1 == true)

        // Only an account id names a directory.
        let outside = device.root.deletingLastPathComponent().appendingPathComponent("outside-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: outside, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: outside) }
        for name in ["../" + outside.lastPathComponent, "..", ".", "", "/", neighbour.lastPathComponent + "/../" + neighbour.lastPathComponent, neighbour.lastPathComponent.uppercased()] {
            #expect(throws: APIClientError.invalidIdentifier) { try accounts.storage.erase(name) }
        }
        #expect(FileManager.default.fileExists(atPath: outside.path) && FileManager.default.fileExists(atPath: neighbour.path))
    }

    @Test func o1_olderSavedDataListsDirectoriesWithoutAnAccountFileAndCanBeErased() async throws {
        let device = Device(), server = FakeServer()
        server.user("token-1", id: userA, email: "a@example.test")
        let older = try device.leaveRecording(address: "https://gone.example.test", userID: userB)
        try FileManager.default.createDirectory(at: device.root.appendingPathComponent(AccountID(address: "https://empty.example.test", userID: userB).raw), withIntermediateDirectories: true)
        let accounts = device.launch()
        let id = try await accounts.connect(instance: server.address, token: "token-1")
        try device.leaveRecording(address: server.address, userID: userA)

        // The signed-in account's own directory is not "older data"; an empty directory is nothing.
        #expect(accounts.olderData.map(\.directory) == [older.lastPathComponent])
        #expect(accounts.olderData.first?.account == nil && accounts.olderData.first?.host == nil)
        #expect(accounts.olderData.first?.summary == AccountDataSummary(recordings: 1, unsent: 1))
        #expect((accounts.olderData.first?.bytes ?? 0) > 0 && accounts.olderData.first?.lastChanged != nil)
        #expect(accounts.signedOut.isEmpty)

        // A file dropped into a directory that names another account says nothing about it.
        let planted = AccountFile(id: id, url: server.address, userId: userA, email: "a@example.test", label: server.host)
        try JSONEncoder().encode(planted).write(to: older.appendingPathComponent("account.json"))
        accounts.refreshSavedData()
        #expect(accounts.signedOut.isEmpty && accounts.olderData.map(\.directory) == [older.lastPathComponent])

        try accounts.eraseSaved(try #require(accounts.olderData.first))
        #expect(!FileManager.default.fileExists(atPath: older.path) && accounts.olderData.isEmpty)
        #expect(accounts.dataSummary(for: id).recordings == 1)
    }

    @Test func o3_theSignOutSheetsCountsAreTheRecordingsOnThisDevice() async throws {
        let device = Device(), server = FakeServer()
        server.user("token-1", id: userA, email: "a@example.test")
        let accounts = device.launch()
        let id = try await accounts.connect(instance: server.address, token: "token-1")
        #expect(accounts.dataSummary(for: id) == AccountDataSummary())
        let directory = try device.leaveRecording(address: server.address, userID: userA, state: .ready)
        try device.leaveRecording(address: server.address, userID: userA, state: .acknowledged)
        try device.leaveRecording(address: server.address, userID: userA, state: .pending)
        // A capture that was still being recorded when the app was killed: audio that is nowhere else.
        let note = UUID().uuidString
        let folder = directory.appendingPathComponent("recordings/\(note)")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        try Data("RIFF".utf8).write(to: folder.appendingPathComponent("capture-0.wav"))
        try Data("{}".utf8).write(to: folder.appendingPathComponent("capture-0.wav.capture.json"))
        // A recording whose note the server deleted.
        try Data(#"{"\#(note.lowercased())":{"baseOffsetMs":0,"nextSequence":0,"segments":[],"serverMissingSince":0}}"#.utf8)
            .write(to: directory.appendingPathComponent("captures.json"))
        #expect(accounts.dataSummary(for: id) == AccountDataSummary(recordings: 4, unsent: 2, deletedOnServer: 1))
    }

    // MARK: One account, as before

    @Test func withOneAccountASignInAsSomebodyElseTakesItsPlaceAndItsDataStays() async throws {
        let device = Device(), server = FakeServer(), other = FakeServer("other")
        server.user("token-a", id: userA, email: "a@example.test", notes: ["A's note"])
        other.user("token-b", id: userB, email: "b@example.test", notes: ["B's note"])
        let accounts = device.launch()
        let first = try await accounts.connect(instance: server.address, token: "token-a")
        let directory = try device.leaveRecording(address: server.address, userID: userA)

        // A failed sign-in leaves the account that is here exactly as it was.
        await #expect(throws: APIClientError.unauthorized) { try await accounts.connect(instance: other.address, token: "wrong") }
        #expect(accounts.sessions.map(\.id) == [first] && accounts.sessions.first?.status == .ready)

        let second = try await accounts.connect(instance: other.address, token: "token-b")
        #expect(accounts.sessions.map(\.id) == [second] && accounts.defaultID == second)
        #expect(accounts.sessions.first?.notes.list.rows.map(\.title) == ["B's note"])
        #expect(device.store.ids == [second])
        #expect(FileManager.default.fileExists(atPath: directory.path))
        #expect(accounts.signedOut.map(\.directory) == [first.raw])
        #expect(try device.legacy.load()?.instance.description == other.address)
        // B's session never went to A's server, nor A's to B's.
        #expect(server.requests(with: "token-b").isEmpty && other.requests(with: "token-a").isEmpty)
    }

    @Test func anAccountBeingRecordedToCannotBeSignedOutErasedOrReplaced() async throws {
        let device = Device(), server = FakeServer(), other = FakeServer("other")
        server.user("token-a", id: userA, email: "a@example.test")
        other.user("token-b", id: userB, email: "b@example.test")
        let accounts = device.launch()
        let id = try await accounts.connect(instance: server.address, token: "token-a")
        accounts.removalBlock = { $0 == id ? "Stop the recording first." : nil }
        await #expect(throws: AccountsError.blocked("Stop the recording first.")) { try await accounts.signOut(id) }
        await #expect(throws: AccountsError.blocked("Stop the recording first.")) { try await accounts.erase(id) }
        await #expect(throws: AccountsError.blocked("Stop the recording first.")) { try await accounts.connect(instance: other.address, token: "token-b") }
        #expect(accounts.sessions.map(\.id) == [id] && accounts.sessions.first?.status == .ready)
        #expect(device.store.ids == [id])
    }

    // MARK: Two accounts (debug only until they are switched on)

    @Test func p1_twoSessionsFailIndependently() async throws {
        let device = Device(), first = FakeServer("first"), second = FakeServer("second")
        first.user("token-a", id: userA, email: "a@example.test", notes: ["On first"])
        second.user("token-b", id: userB, email: "b@example.test", notes: ["On second"])
        let accounts = device.launch(multiple: true)
        let a = try await accounts.connect(instance: first.address, token: "token-a")
        let b = try await accounts.connect(instance: second.address, token: "token-b")
        #expect(accounts.sessions.map(\.id) == [a, b] && accounts.defaultID == a)
        #expect(accounts.showsAccountLabels)

        // One server does not answer at launch: it shows no notes; the other is untouched.
        first.set(.down)
        var relaunched = device.launch(multiple: true)
        await relaunched.restoreAll()
        #expect(isUnreachable(try #require(relaunched.session(a)).status) && relaunched.session(a)?.notes.list.rows.isEmpty == true)
        #expect(relaunched.session(b)?.status == .ready && relaunched.session(b)?.notes.list.rows.map(\.title) == ["On second"])
        first.set(.up)
        await relaunched.retryUnreachable()
        #expect(relaunched.session(a)?.status == .ready && relaunched.session(a)?.notes.list.rows.map(\.title) == ["On first"])

        // One session is refused: only that account needs a sign-in.
        second.refuse("token-b")
        relaunched = device.launch(multiple: true)
        await relaunched.restoreAll()
        #expect(relaunched.session(a)?.status == .ready && relaunched.session(b)?.status == .needsSignIn)

        // Each session only ever went to its own server.
        #expect(first.requests(with: "token-b").isEmpty && second.requests(with: "token-a").isEmpty)
        #expect(first.requests.allSatisfy { $0.host == first.host })
        #expect(relaunched.sessions(on: try #require(ServerOrigin(first.address))).map(\.id) == [a])

        // Signing out of one leaves the other's session, entry and notes alone.
        try await relaunched.signOut(b)
        #expect(relaunched.sessions.map(\.id) == [a] && device.store.ids == [a])
        #expect(relaunched.session(a)?.status == .ready)
    }

    @Test func p1_eachAccountHasItsOwnOutboxDirectory() async throws {
        let device = Device(), server = FakeServer()
        server.user("token-a", id: userA, email: "a@example.test")
        server.user("token-b", id: userB, email: "b@example.test")
        let accounts = device.launch(multiple: true)
        let a = try await accounts.connect(instance: server.address, token: "token-a")
        let b = try await accounts.connect(instance: server.address, token: "token-b")
        // Two users on one server: two accounts, two directories, two items.
        #expect(a != b && Set(device.store.ids) == [a, b])
        #expect(accounts.label(for: a) == server.host + " · a@example.test")
        let instance = try InstanceURL(server.address)
        let first = try AudioOutbox(instance: instance, principalID: userA, root: device.root)
        let second = try AudioOutbox(instance: instance, principalID: userB, root: device.root)
        #expect(first.directory.path == (try accounts.storage.directory(a.raw)).path)
        #expect(second.directory.path == (try accounts.storage.directory(b.raw)).path)
        #expect(first.directory != second.directory)
        // An outbox is delivered only through its own account's model.
        let entry = AudioOutboxEntry(id: UUID().uuidString, noteID: UUID().uuidString, channel: .mic, sequence: 0, startMs: 0,
                                     durationMs: 1, fileName: "unused.wav", sourceFileName: "capture.wav", state: .pending)
        let notesB = try #require(accounts.session(b)?.notes)
        await #expect(throws: APIClientError.unauthorized) { try await notesB.upload(entry, from: first) }
        try await accounts.erase(a)
        #expect(!FileManager.default.fileExists(atPath: first.directory.path) && FileManager.default.fileExists(atPath: second.directory.path))
    }
}
