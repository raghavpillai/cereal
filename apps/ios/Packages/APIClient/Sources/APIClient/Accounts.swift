import Foundation
import CryptoKit
import Observation
import Contracts

/// One user on one server. The value is the lower-case hex SHA-256 of
/// `"<address as stored>|<user id>"`, which is also the name of the account's
/// directory on this device. It is checked every time one is made, so a value
/// read from preferences or from a link can never name a path.
public struct AccountID: Hashable, Codable, Sendable, CustomStringConvertible {
    public let raw: String

    public init?(_ raw: String) {
        guard raw.utf8.count == 64, raw.utf8.allSatisfy({ (48...57).contains($0) || (97...102).contains($0) }) else { return nil }
        self.raw = raw
    }

    /// The account of `userID` (already canonical) at `address`, exactly as the address is stored.
    public init(address: String, userID: String) {
        raw = Self.digest(address + "|" + userID)
    }

    /// A migrated session before its server has said whose it is. No directory
    /// and no Keychain item is ever made under this value.
    static func provisional(address: String) -> AccountID {
        AccountID(address: address, userID: "")
    }

    public init(from decoder: Decoder) throws {
        let text = try decoder.singleValueContainer().decode(String.self)
        guard let id = AccountID(text) else {
            throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Not an account id"))
        }
        self = id
    }

    public func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        try container.encode(raw)
    }

    /// What a log line names an account by.
    public var short: String { String(raw.prefix(8)) }
    public var description: String { short }

    /// A server's user id as it enters an identity: a UUID, hyphenated, lower
    /// case. A server that spells the same id another way (no hyphens, braces,
    /// upper case) does not make a second account.
    public static func canonicalUserID(_ text: String) -> String? {
        var hex = text.trimmingCharacters(in: .whitespaces)
        if hex.hasPrefix("{"), hex.hasSuffix("}") { hex = String(hex.dropFirst().dropLast()) }
        if let id = UUID(uuidString: hex) { return id.uuidString.lowercased() }
        guard hex.utf8.count == 32, hex.utf8.allSatisfy({ (48...57).contains($0) || (65...70).contains($0) || (97...102).contains($0) }) else { return nil }
        let c = Array(hex)
        let dashed = String(c[0..<8]) + "-" + String(c[8..<12]) + "-" + String(c[12..<16]) + "-" + String(c[16..<20]) + "-" + String(c[20..<32])
        return UUID(uuidString: dashed)?.uuidString.lowercased()
    }

    private static func digest(_ text: String) -> String {
        SHA256.hash(data: Data(text.utf8)).map { String(format: "%02x", $0) }.joined()
    }
}

/// What the app remembers about an account. Nothing here is secret: the
/// session is in the Keychain, in an item of the account's own.
public struct AccountRecord: Codable, Sendable, Identifiable, Equatable {
    public var id: AccountID
    /// The server's address exactly as it was first stored. Signing in again never changes it.
    public var instanceURL: String
    /// Nil only for a session carried over from an earlier version that its server has not confirmed yet.
    public var userID: String?
    public var name: String
    public var email: String
    public var label: String
    public var colorIndex: Int
    public var features: [String]
    public var addedAt: Date

    enum CodingKeys: String, CodingKey {
        case id, name, email, label, features, addedAt
        case instanceURL = "url", userID = "userId", colorIndex = "color"
    }

    public init(id: AccountID, instanceURL: String, userID: String?, name: String, email: String, label: String,
                colorIndex: Int, features: [String] = [], addedAt: Date = .now) {
        self.id = id; self.instanceURL = instanceURL; self.userID = userID; self.name = name; self.email = email
        self.label = label; self.colorIndex = colorIndex; self.features = features; self.addedAt = addedAt
    }

    public init(from decoder: Decoder) throws {
        let values = try decoder.container(keyedBy: CodingKeys.self)
        id = try values.decode(AccountID.self, forKey: .id)
        instanceURL = try values.decode(String.self, forKey: .instanceURL)
        userID = try values.decodeIfPresent(String.self, forKey: .userID)
        name = try values.decodeIfPresent(String.self, forKey: .name) ?? ""
        email = try values.decodeIfPresent(String.self, forKey: .email) ?? ""
        label = try values.decodeIfPresent(String.self, forKey: .label) ?? ""
        colorIndex = try values.decodeIfPresent(Int.self, forKey: .colorIndex) ?? 0
        features = try values.decodeIfPresent([String].self, forKey: .features) ?? []
        addedAt = try values.decodeIfPresent(Date.self, forKey: .addedAt) ?? .distantPast
    }

    public var instance: InstanceURL? { try? InstanceURL(instanceURL) }
    public var origin: ServerOrigin? { instance.flatMap(ServerOrigin.init(_:)) }
    /// Host and unusual port, in ASCII: what the person recognises the server by.
    /// An address that has no origin (it never matches a link) is shown whole.
    public var host: String { origin?.displayHost ?? instanceURL }
    /// The identity string that keys what this device keeps for the account (chat history, the outbox).
    public var identity: String? { userID.map { instanceURL + "|" + $0 } }
}

public enum AccountStatus: Sendable, Equatable {
    /// Its saved session has not been confirmed in this launch. No notes are shown.
    case checking
    case ready
    /// Its server has not answered in this launch. No notes are shown; it is asked again.
    case unreachable(APIClientError)
    /// Its own `/session` refused the session. Nothing more is sent until the person signs in again.
    case needsSignIn
}

/// A note, with the account it belongs to. Two servers can hand out the same note id.
public struct NoteRef: Hashable, Codable, Sendable {
    public let account: AccountID
    public let id: String
    public init(account: AccountID, id: String) { self.account = account; self.id = id }
}

public struct EventRef: Hashable, Sendable {
    public let account: AccountID
    public let id: String
    public init(account: AccountID, id: String) { self.account = account; self.id = id }
}

/// One account while the app runs: what is remembered about it, and the one
/// model through which anything is asked of its server.
@MainActor @Observable
public final class AccountSession {
    public internal(set) var record: AccountRecord
    public let notes: ServerNotesModel
    public nonisolated let key = UUID()

    init(record: AccountRecord, notes: ServerNotesModel) {
        self.record = record
        self.notes = notes
    }

    public var id: AccountID { record.id }
    public var status: AccountStatus { notes.status ?? .checking }
    public var isReady: Bool { status == .ready }
    public var host: String { record.host }
    public var origin: ServerOrigin? { record.origin }
}

public enum AccountsError: Error, Equatable, LocalizedError, Sendable {
    /// A recording to the account is running, or something else must finish first.
    case blocked(String)
    /// Signing in again to an account gave a session for somebody else.
    case otherUser(signedInAs: String, expected: String)
    case unknownAccount

    public var errorDescription: String? {
        switch self {
        case .blocked(let reason): reason
        case .otherUser(let actual, let expected): "You signed in as \(actual); this account is \(expected)."
        case .unknownAccount: "This account is no longer on this iPhone."
        }
    }
}

/// The accounts on this device, in the person's order. Nothing is "current":
/// whatever is asked of a server is asked through one account's own model.
@MainActor @Observable
public final class AccountsModel {
    public private(set) var sessions: [AccountSession] = []
    public private(set) var defaultID: AccountID?
    public var chatID: AccountID?
    public var searchScope: Set<AccountID> = []
    /// Accounts that were signed out and still have data on this device.
    public private(set) var signedOut: [SavedAccountData] = []
    /// Directories from before accounts were recorded, which no account here owns.
    public private(set) var olderData: [SavedAccountData] = []
    /// Set when the saved list of accounts could not be read and was rebuilt.
    public private(set) var storageError: String?
    /// True until the saved accounts have been read at launch.
    public private(set) var isRestoring = true
    /// Changes whenever an account is added or removed.
    public private(set) var revision = 0

    /// Says why an account cannot be signed out or erased right now, or nil.
    /// The recorder answers "Stop the recording first" for the account it records to.
    @ObservationIgnored public var removalBlock: (@MainActor (AccountID) -> String?)?
    /// Told after an account has gone, with what identified its data (chat history, link history).
    @ObservationIgnored public var onRemoved: (@MainActor (AccountRecord, _ erased: Bool) -> Void)?

    @ObservationIgnored private let defaults: UserDefaults
    @ObservationIgnored private let registryKey: String
    @ObservationIgnored private let credentials: any AccountCredentialStore
    @ObservationIgnored private let legacy: ConnectionSettingsStore?
    @ObservationIgnored public let storage: AccountStorage
    @ObservationIgnored private let makeAPI: @Sendable (InstanceCredentials) -> MeetingNotesAPI
    /// Several accounts at once. Off until the views that need it are all in; a
    /// sign-in to another account then takes the place of the one that is here.
    @ObservationIgnored public let allowsMultiple: Bool
    @ObservationIgnored private var retry: Task<Void, Never>?
    @ObservationIgnored private let retryDelays: [Duration]

    public static let registryKey = "cereal.accounts.v1"

    public init(defaults: UserDefaults = .standard,
                credentials: any AccountCredentialStore = KeychainAccountStore(),
                legacy: ConnectionSettingsStore? = nil,
                storage: AccountStorage = AccountStorage(),
                allowsMultiple: Bool = false,
                registryKey: String = AccountsModel.registryKey,
                retryDelays: [Duration] = [.seconds(5), .seconds(15), .seconds(60), .seconds(300)],
                makeAPI: @escaping @Sendable (InstanceCredentials) -> MeetingNotesAPI = { MeetingNotesAPI(credentials: $0) }) {
        self.defaults = defaults
        self.credentials = credentials
        self.legacy = legacy
        self.storage = storage
        self.allowsMultiple = allowsMultiple
        self.registryKey = registryKey
        self.retryDelays = retryDelays
        self.makeAPI = makeAPI
    }

    // MARK: Reading

    public func session(_ id: AccountID) -> AccountSession? { sessions.first { $0.id == id } }

    /// Accounts on the server a link names, in the person's order. A link names
    /// a server by its origin alone, so an account whose address has a path is not one of them.
    public func sessions(on origin: ServerOrigin) -> [AccountSession] {
        sessions.filter { session in session.record.instance.map(origin.isInstance) ?? false }
    }

    /// Where a new note without a calendar event goes.
    public var defaultSession: AccountSession? {
        if let defaultID, let session = session(defaultID) { return session }
        return sessions.count == 1 ? sessions[0] : nil
    }

    /// Chooses where new notes without a calendar event go.
    public func setDefault(_ id: AccountID) {
        guard session(id) != nil, defaultID != id else { return }
        defaultID = id
        writeRegistry()
    }

    /// Labels and dots appear only when there is more than one account to tell apart.
    public var showsAccountLabels: Bool { sessions.count > 1 }

    /// The label shown for an account: its own, or the host; two accounts on one
    /// host are told apart by the email their server reported.
    public func label(for id: AccountID) -> String {
        guard let record = session(id)?.record else { return "" }
        let twins = sessions.filter { $0.record.label == record.label }.count > 1
        return twins && !record.email.isEmpty ? record.label + " · " + record.email : record.label
    }

    /// What identifies the accounts and their sessions: it changes when one is
    /// added, removed, verified, signed in again or refused.
    public var signature: String {
        sessions.map { "\($0.id.raw)#\($0.notes.sessionGeneration)#\(Self.tag($0.status))" }.joined(separator: ",")
    }

    private static func tag(_ status: AccountStatus) -> String {
        switch status {
        case .checking: "c"
        case .ready: "r"
        case .unreachable: "u"
        case .needsSignIn: "n"
        }
    }

    // MARK: Launch

    /// Reads the saved accounts and confirms each one's session with its own
    /// server. An account's notes are shown only once that has succeeded in
    /// this launch. A session from before accounts were recorded is carried over first.
    public func restoreAll() async {
        defer { isRestoring = false; refreshSavedData() }
        switch readRegistry() {
        case .absent: migrateLegacySession()
        case .unreadable(let data): rebuildRegistry(unreadable: data)
        case .read(let saved):
            sessions = saved.accounts.map { AccountSession(record: $0, notes: ServerNotesModel(makeAPI: makeAPI)) }
            defaultID = saved.default.flatMap { id in sessions.contains { $0.id == id } ? id : nil }
            chatID = saved.chat
        }
        revision += 1
        await withTaskGroup(of: Void.self) { group in
            for session in sessions { group.addTask { await self.verify(session) } }
        }
        await adoptStrayLegacySession()
        scheduleRetry()
    }

    /// Asks again the accounts whose servers have not answered in this launch.
    public func retryUnreachable() async {
        let waiting = sessions.filter { if case .unreachable = $0.status { true } else { false } }
        await withTaskGroup(of: Void.self) { group in
            for session in waiting { group.addTask { await self.verify(session) } }
        }
    }

    private func scheduleRetry() {
        retry?.cancel()
        guard sessions.contains(where: { if case .unreachable = $0.status { true } else { false } }) else { retry = nil; return }
        retry = Task { [weak self, retryDelays] in
            var attempt = 0
            while !Task.isCancelled {
                try? await Task.sleep(for: retryDelays[min(attempt, retryDelays.count - 1)])
                guard let self, !Task.isCancelled else { return }
                await self.retryUnreachable()
                guard self.sessions.contains(where: { if case .unreachable = $0.status { true } else { false } }) else { return }
                attempt += 1
            }
        }
    }

    private func verify(_ session: AccountSession) async {
        let record = session.record
        let saved: StoredSession?
        do {
            if record.userID == nil {
                // Carried over, not yet confirmed: the session is still where the earlier version kept it.
                guard let legacy, let pair = try legacy.load() else { return removeUnusable(session) }
                saved = StoredSession(credentials: pair, userID: "")
                guard pair.instance.description == record.instanceURL else { return removeUnusable(session) }
            } else {
                saved = try credentials.load(record.id)
            }
        } catch {
            // The Keychain could not be read (before first unlock, for instance). Nothing is discarded.
            session.notes.markUnreachable(.secureStorage)
            return
        }
        // A session is used only for the address and the user it was saved for.
        guard let saved, saved.credentials.instance.description == record.instanceURL,
              record.userID == nil || saved.userID == record.userID else {
            session.notes.markNeedsSignIn(instance: record.instance)
            return
        }
        let outcome = await session.notes.connect(saved.credentials, expecting: record.userID) { [weak self] verified in
            guard let self, record.userID == nil else { return }
            try self.confirmMigrated(session, verified)
        }
        switch outcome {
        case .ready(let verified):
            update(session, from: verified.identity)
        case .needsSignIn where record.userID == nil:
            // The carried-over pair was refused by its own server: it is discarded, not tried again.
            try? legacy?.clear()
            removeUnusable(session)
        case .otherUser:
            session.notes.markNeedsSignIn(instance: record.instance)
        case .needsSignIn, .failed, .superseded: break
        }
    }

    /// A carried-over session that cannot be used leaves no account behind.
    private func removeUnusable(_ session: AccountSession) {
        session.notes.close()
        sessions.removeAll { $0 === session }
        if defaultID == session.id { defaultID = nil }
        writeRegistry()
        revision += 1
    }

    private func update(_ session: AccountSession, from identity: AuthSessionDTO) {
        var record = session.record
        record.name = identity.user.name
        record.email = identity.user.email
        guard record != session.record else { return }
        session.record = record
        writeRegistry()
        try? storage.writeAccountFile(record)
    }

    // MARK: Migration

    /// Phase 1 of carrying over the one session an earlier version kept: an
    /// entry is written from the saved address alone, so it works offline.
    private func migrateLegacySession() {
        guard let legacy, let instance = legacy.savedInstance, (try? legacy.load()) != nil else { return }
        let address = instance.description
        let record = AccountRecord(id: .provisional(address: address), instanceURL: address, userID: nil, name: "", email: "",
                                   label: ServerOrigin(instance)?.displayHost ?? address, colorIndex: 0)
        sessions = [AccountSession(record: record, notes: ServerNotesModel(makeAPI: makeAPI))]
        defaultID = record.id
        writeRegistry()
    }

    /// Phase 3: the server has said whose the carried-over session is. The
    /// account gets its real id (the directory its recordings are already in),
    /// its own Keychain item, read back before anything relies on it, and its
    /// entry. The earlier item is left in place for this release.
    private func confirmMigrated(_ session: AccountSession, _ verified: ServerNotesModel.Verified) throws {
        let address = session.record.instanceURL
        let id = AccountID(address: address, userID: verified.userID)
        try credentials.save(verified.credentials, userID: verified.userID, for: id)
        guard let back = try credentials.load(id), back.userID == verified.userID,
              back.credentials.instance.description == address,
              back.credentials.bearerToken == verified.credentials.bearerToken else { throw APIClientError.secureStorage }
        var record = session.record
        let wasDefault = defaultID == record.id
        record.id = id
        record.userID = verified.userID
        record.name = verified.identity.user.name
        record.email = verified.identity.user.email
        session.record = record
        if wasDefault { defaultID = id }
        try? storage.writeAccountFile(record)
        writeRegistry()
        revision += 1
    }

    /// A session left where the earlier version keeps its own, by that version
    /// run again by hand. It can only replace the session of an account that is
    /// already here; it never adds one.
    private func adoptStrayLegacySession() async {
        guard let legacy, !sessions.isEmpty, !sessions.contains(where: { $0.record.userID == nil }),
              let stray = try? legacy.load() else { return }
        let known = sessions.contains { session in
            (try? credentials.load(session.id))?.credentials.bearerToken == stray.bearerToken
        }
        guard !known else { return }
        switch await makeAPI(stray).verifySession() {
        case .valid(let identity):
            if let userID = AccountID.canonicalUserID(identity.user.id),
               let session = sessions.first(where: { Self.same($0.record, instance: stray.instance, userID: userID) }),
               let stored = try? InstanceCredentials(instance: InstanceURL(session.record.instanceURL), bearerToken: stray.bearerToken) {
                if (try? credentials.save(stored, userID: userID, for: session.id)) != nil {
                    _ = await session.notes.connect(stored, expecting: userID)
                }
            } else { try? legacy.clear() }
        case .refused: try? legacy.clear()
        case .noAnswer: break
        }
        syncLegacy()
    }

    /// For this release the earlier version's item follows the one account, so
    /// that version still starts signed in if it is installed again by hand.
    private func syncLegacy() {
        guard let legacy, !allowsMultiple else { return }
        guard let session = sessions.first, sessions.count == 1 else {
            if sessions.isEmpty { try? legacy.clear() }
            return
        }
        guard session.record.userID != nil, let saved = try? credentials.load(session.id) else { return }
        try? legacy.save(saved.credentials)
    }

    // MARK: Signing in

    /// Signs in with a session that is already in hand. An account that is
    /// already here has its session replaced and keeps its stored address, so
    /// its directory does not move. While several accounts are switched off, a
    /// different account takes the place of the one that is here, as before.
    @discardableResult
    public func add(_ entered: InstanceCredentials) async throws -> AccountID {
        // A bar separates the address from the user in an identity. A user id never has one; a new address may not either.
        guard !entered.instance.description.contains("|"), !entered.instance.url.path.contains("|") else { throw APIClientError.invalidInstanceURL }
        let verified: ServerNotesModel.Verified
        do { verified = try await ServerNotesModel.verify(entered, makeAPI: makeAPI) }
        catch let failure as ServerNotesModel.VerifyFailure {
            switch failure {
            case .refused: throw APIClientError.unauthorized
            case .failed(let error): throw error
            }
        }
        if let existing = sessions.first(where: { Self.same($0.record, instance: entered.instance, userID: verified.userID) }) {
            return try await replaceSession(of: existing, with: verified)
        }
        let address = entered.instance.description
        let id = AccountID(address: address, userID: verified.userID)
        if !allowsMultiple {
            for other in sessions { if let reason = removalBlock?(other.id) { verified.api.cancelRequests(); throw AccountsError.blocked(reason) } }
        }
        do { try credentials.save(verified.credentials, userID: verified.userID, for: id) }
        catch { verified.api.cancelRequests(); throw APIClientError.secureStorage }
        if !allowsMultiple {
            // The account that is here makes way. Its data stays, as a sign-out leaves it.
            for other in sessions { remove(other, erasing: false) }
        }
        let record = AccountRecord(id: id, instanceURL: address, userID: verified.userID, name: verified.identity.user.name,
                                   email: verified.identity.user.email, label: ServerOrigin(entered.instance)?.displayHost ?? address,
                                   colorIndex: sessions.count % 6)
        let added = AccountSession(record: record, notes: ServerNotesModel(makeAPI: makeAPI))
        sessions.append(added)
        if sessions.count == 1 { defaultID = id }
        try? storage.writeAccountFile(record)
        writeRegistry()
        syncLegacy()
        added.notes.adopt(verified)
        revision += 1
        refreshSavedData()
        return id
    }

    /// Signing in again to `id`, with the address fixed. A session for anybody else is refused.
    @discardableResult
    public func signInAgain(_ id: AccountID, with entered: InstanceCredentials) async throws -> AccountID {
        guard let session = session(id) else { throw AccountsError.unknownAccount }
        let verified: ServerNotesModel.Verified
        do { verified = try await ServerNotesModel.verify(entered, makeAPI: makeAPI) }
        catch let failure as ServerNotesModel.VerifyFailure {
            switch failure {
            case .refused: throw APIClientError.unauthorized
            case .failed(let error): throw error
            }
        }
        guard Self.same(session.record, instance: entered.instance, userID: verified.userID) else {
            verified.api.cancelRequests()
            throw AccountsError.otherUser(signedInAs: verified.identity.user.email, expected: session.record.email)
        }
        return try await replaceSession(of: session, with: verified)
    }

    private func replaceSession(of session: AccountSession, with verified: ServerNotesModel.Verified) async throws -> AccountID {
        // The address the account was first stored under goes into the new item.
        guard let instance = session.record.instance,
              let stored = try? InstanceCredentials(instance: instance, bearerToken: verified.credentials.bearerToken) else {
            throw APIClientError.invalidInstanceURL
        }
        let previous = session.notes.connection
        do { try credentials.save(stored, userID: verified.userID, for: session.id) }
        catch { verified.api.cancelRequests(); throw APIClientError.secureStorage }
        let kept = ServerNotesModel.Verified(api: makeAPI(stored), credentials: stored, identity: verified.identity,
                                             userID: verified.userID, first: verified)
        verified.api.cancelRequests()
        session.notes.adopt(kept)
        update(session, from: verified.identity)
        syncLegacy()
        // The session that was replaced is ended on its own server, when it was a different one.
        if let previous, previous.bearerToken != stored.bearerToken {
            Task { try? await previous.signOut() }
        }
        return session.id
    }

    public func signIn(instance text: String, email: String, password: String) async throws -> AccountID {
        try await add(MeetingNotesAPI.signIn(instance: InstanceURL(text), email: email, password: password))
    }

    /// Finishes a sign-in that was made in the browser.
    public func signIn(browser attempt: BrowserSignIn, callback: URL, configuration: URLSessionConfiguration = .ephemeral) async throws -> AccountID {
        let code = try attempt.code(from: callback)
        return try await add(attempt.exchange(code: code, configuration: configuration))
    }

    public func connect(instance text: String, token: String) async throws -> AccountID {
        try await add(InstanceCredentials(instance: InstanceURL(text), bearerToken: token))
    }

    static func same(_ record: AccountRecord, instance: InstanceURL, userID: String) -> Bool {
        guard record.userID == userID else { return false }
        if record.instanceURL == instance.description { return true }
        guard let stored = record.instance, let left = ServerOrigin(stored), let right = ServerOrigin(instance) else { return false }
        return left == right && stored.url.path == instance.url.path
    }

    // MARK: Leaving

    /// Ends the session. Everything the account has on this device stays, and
    /// is there again when the same person signs back in.
    public func signOut(_ id: AccountID) async throws {
        guard let session = session(id) else { throw AccountsError.unknownAccount }
        if let reason = removalBlock?(id) { throw AccountsError.blocked(reason) }
        remove(session, erasing: false)
        refreshSavedData()
    }

    /// Removes the account and erases what it has on this device. The notes on its server are not touched.
    public func erase(_ id: AccountID) async throws {
        guard let session = session(id) else { throw AccountsError.unknownAccount }
        if let reason = removalBlock?(id) { throw AccountsError.blocked(reason) }
        remove(session, erasing: true)
        refreshSavedData()
    }

    private func remove(_ session: AccountSession, erasing: Bool) {
        let record = session.record
        // The order matters: nothing of the account is asked or shown once this begins.
        let previous = session.notes.close()
        sessions.removeAll { $0 === session }
        revision += 1
        try? credentials.clear(record.id)
        if defaultID == record.id { defaultID = sessions.count == 1 ? sessions[0].id : nil }
        if chatID == record.id { chatID = nil }
        searchScope.remove(record.id)
        writeRegistry()
        if let legacy, let pair = try? legacy.load(), pair.bearerToken == previous?.bearerToken { try? legacy.clear() }
        syncLegacy()
        if erasing, record.userID != nil { try? storage.erase(record.id.raw) }
        onRemoved?(record, erasing)
        if let previous { Task { try? await previous.signOut() } }
    }

    // MARK: Data left on this device

    public func refreshSavedData() {
        let live = Set(sessions.map(\.id.raw))
        // A carried-over account that is not confirmed yet owns one of the
        // directories below, and which one is not known: none is offered for erasing.
        let unconfirmed = sessions.contains { $0.record.userID == nil }
        let saved = unconfirmed ? [] : storage.saved().filter { !live.contains($0.directory) }
        signedOut = saved.filter { $0.account != nil }
        olderData = saved.filter { $0.account == nil }
    }

    /// What signing out would leave here, and erasing would remove.
    public func dataSummary(for id: AccountID) -> AccountDataSummary {
        guard session(id)?.record.userID != nil else { return AccountDataSummary() }
        return storage.summary(id.raw)
    }

    /// Erases a directory no account here owns: a signed-out account's, or older saved data.
    public func eraseSaved(_ item: SavedAccountData) throws {
        guard !sessions.contains(where: { $0.id.raw == item.directory }) else { throw AccountsError.blocked("Sign out of this account first.") }
        try storage.erase(item.directory)
        if let account = item.account {
            try? credentials.clear(account.id)
            onRemoved?(AccountRecord(id: account.id, instanceURL: account.url, userID: account.userId, name: "", email: account.email,
                                     label: account.label, colorIndex: 0), true)
        }
        refreshSavedData()
    }

    // MARK: The registry

    private struct Registry: Codable {
        var version = 1
        var `default`: AccountID?
        var chat: AccountID?
        var accounts: [AccountRecord]
    }

    private enum RegistryRead { case absent, read(Registry), unreadable(Data) }

    private func readRegistry() -> RegistryRead {
        guard let data = defaults.data(forKey: registryKey) else {
            // Anything else under the key is not a list this app wrote.
            return defaults.object(forKey: registryKey) == nil ? .absent : .unreadable(Data())
        }
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .iso8601
        guard let saved = try? decoder.decode(Registry.self, from: data), saved.version == 1,
              Set(saved.accounts.map(\.id)).count == saved.accounts.count else { return .unreadable(data) }
        return saved.accounts.isEmpty ? .absent : .read(saved)
    }

    /// Absence means "no accounts": nothing is written for none.
    private func writeRegistry() {
        guard !sessions.isEmpty else { defaults.removeObject(forKey: registryKey); return }
        let encoder = JSONEncoder()
        encoder.dateEncodingStrategy = .iso8601
        encoder.outputFormatting = [.sortedKeys]
        let value = Registry(default: defaultID, chat: chatID, accounts: sessions.map(\.record))
        if let data = try? encoder.encode(value) { defaults.set(data, forKey: registryKey) }
    }

    /// A list that is there and cannot be read is not "no accounts". It is kept
    /// aside, and the accounts are found again from the files in their own
    /// directories, for those whose Keychain item still exists. The session of
    /// an earlier version is not carried over a second time.
    private func rebuildRegistry(unreadable data: Data) {
        let stamp = Int(Date.now.timeIntervalSince1970)
        defaults.set(data, forKey: "cereal.accounts.unreadable-\(stamp)")
        storageError = "The list of accounts on this iPhone could not be read. It was rebuilt from the accounts that are still signed in."
        var found: [AccountRecord] = []
        for item in storage.saved() {
            guard let file = item.account, let userID = file.userId, AccountID(address: file.url, userID: userID) == file.id,
                  file.id.raw == item.directory, (try? credentials.load(file.id)) != nil else { continue }
            found.append(AccountRecord(id: file.id, instanceURL: file.url, userID: userID, name: "", email: file.email,
                                       label: file.label, colorIndex: found.count % 6, addedAt: item.lastChanged ?? .now))
        }
        sessions = found.map { AccountSession(record: $0, notes: ServerNotesModel(makeAPI: makeAPI)) }
        defaultID = sessions.first?.id
        writeRegistry()
    }

    #if DEBUG
    /// Static QA states: one account that asks nothing of any server.
    public func showQAState(_ state: ServerNotesState) {
        let record = AccountRecord(id: AccountID(address: "qa", userID: "preview"), instanceURL: "qa", userID: nil,
                                   name: "", email: "", label: "QA preview", colorIndex: 0)
        let session = AccountSession(record: record, notes: ServerNotesModel(makeAPI: makeAPI))
        session.notes.showQAState(state)
        sessions = [session]
        defaultID = record.id
        isRestoring = false
        revision += 1
    }
    #endif
}
