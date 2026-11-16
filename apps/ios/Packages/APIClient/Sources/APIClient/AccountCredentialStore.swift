import Foundation
import Security

/// An account's session as it was saved: the token together with the address
/// and the user it is for. It is used only when both still match the account.
public struct StoredSession: Sendable {
    public let credentials: InstanceCredentials
    public let userID: String

    public init(credentials: InstanceCredentials, userID: String) {
        self.credentials = credentials
        self.userID = userID
    }
}

/// One secret item per account. `load` returns nil when there is none and
/// throws when the store could not be read, which is not the same thing:
/// nothing is discarded because of a failed read.
public protocol AccountCredentialStore: Sendable {
    func load(_ id: AccountID) throws -> StoredSession?
    func save(_ credentials: InstanceCredentials, userID: String, for id: AccountID) throws
    /// Removes the account's session, and its record of open moves with it.
    func clear(_ id: AccountID) throws
}

/// The Keychain item of each account: service `dev.meetingnotes.instance`,
/// account `account:<AccountID>`, on this device only and never synchronised.
/// Its value binds the token to its address and user:
/// `{"url": …, "userId": …, "token": …}`.
public struct KeychainAccountStore: AccountCredentialStore {
    private let service: String

    public init(service: String = "dev.meetingnotes.instance") {
        self.service = service
    }

    private struct Item: Codable {
        let url: String
        let userId: String
        let token: String
    }

    private func query(_ name: String) -> [String: Any] {
        [kSecClass as String: kSecClassGenericPassword,
         kSecAttrService as String: service,
         kSecAttrAccount as String: name,
         kSecAttrSynchronizable as String: false]
    }

    public func load(_ id: AccountID) throws -> StoredSession? {
        var query = query("account:" + id.raw)
        query[kSecReturnData as String] = true
        query[kSecMatchLimit as String] = kSecMatchLimitOne
        var result: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        if status == errSecItemNotFound { return nil }
        guard status == errSecSuccess else { throw KeychainError(status: status) }
        // An item that is there and is not a session of this shape is no session.
        guard let data = result as? Data, let item = try? JSONDecoder().decode(Item.self, from: data),
              let instance = try? InstanceURL(item.url), instance.description == item.url,
              let credentials = try? InstanceCredentials(instance: instance, bearerToken: item.token) else { return nil }
        return StoredSession(credentials: credentials, userID: item.userId)
    }

    public func save(_ credentials: InstanceCredentials, userID: String, for id: AccountID) throws {
        let data = try JSONEncoder().encode(Item(url: credentials.instance.description, userId: userID, token: credentials.bearerToken))
        let query = query("account:" + id.raw)
        let status = SecItemUpdate(query as CFDictionary, [kSecValueData as String: data] as CFDictionary)
        if status == errSecSuccess { return }
        guard status == errSecItemNotFound else { throw KeychainError(status: status) }
        var item = query
        item[kSecValueData as String] = data
        item[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        let added = SecItemAdd(item as CFDictionary, nil)
        guard added == errSecSuccess else { throw KeychainError(status: added) }
    }

    public func clear(_ id: AccountID) throws {
        for name in ["account:" + id.raw, "moves:" + id.raw] {
            let status = SecItemDelete(query(name) as CFDictionary)
            guard status == errSecSuccess || status == errSecItemNotFound else { throw KeychainError(status: status) }
        }
    }
}

/// The file in an account's directory that says whose it is. Not secret, and
/// not trusted: it lets Settings list an account that was signed out and still
/// has data here, and the address in it is shown before anything is sent to it.
public struct AccountFile: Codable, Sendable, Equatable {
    public let id: AccountID
    public let url: String
    public let userId: String?
    public let email: String
    public let label: String
}

/// A directory of account data that no signed-in account owns.
public struct SavedAccountData: Sendable, Identifiable, Equatable {
    /// The directory's name: 64 lower-case hex characters.
    public let directory: String
    /// Nil for data from before accounts were recorded.
    public let account: AccountFile?
    public let bytes: Int64
    public let lastChanged: Date?
    public let summary: AccountDataSummary
    public var id: String { directory }

    public init(directory: String, account: AccountFile?, bytes: Int64, lastChanged: Date?, summary: AccountDataSummary) {
        self.directory = directory; self.account = account; self.bytes = bytes; self.lastChanged = lastChanged; self.summary = summary
    }

    /// The server the data is from, as its file says.
    public var host: String? {
        account.flatMap { (try? InstanceURL($0.url)).flatMap(ServerOrigin.init(_:))?.displayHost ?? $0.url }
    }
}

/// What an account keeps on this device. The iPhone keeps no notes: only the
/// recordings made here, until their audio has reached the server.
public struct AccountDataSummary: Sendable, Equatable {
    /// Notes with a recording made on this device.
    public var recordings = 0
    /// Of those, the ones with audio that never reached the server.
    public var unsent = 0
    /// Of those, the ones whose note the server says was deleted.
    public var deletedOnServer = 0

    public init(recordings: Int = 0, unsent: Int = 0, deletedOnServer: Int = 0) {
        self.recordings = recordings; self.unsent = unsent; self.deletedOnServer = deletedOnServer
    }
}

/// The directories accounts keep their data in: one per account, named by the
/// account's id, under one root that is excluded from backup.
public struct AccountStorage: Sendable {
    public let root: URL

    public init(root: URL? = nil) {
        self.root = root ?? FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("server-audio-outbox", isDirectory: true)
    }

    /// The directory of an account. The name is checked each time: only an
    /// account id, never anything that could be a path, names a directory.
    public func directory(_ name: String) throws -> URL {
        guard AccountID(name) != nil else { throw APIClientError.invalidIdentifier }
        return root.appendingPathComponent(name, isDirectory: true)
    }

    public func writeAccountFile(_ record: AccountRecord) throws {
        guard record.userID != nil else { return }
        let directory = try directory(record.id.raw)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        var excluded = directory
        var values = URLResourceValues()
        values.isExcludedFromBackup = true
        try? excluded.setResourceValues(values)
        let file = AccountFile(id: record.id, url: record.instanceURL, userId: record.userID, email: record.email, label: record.label)
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        let url = directory.appendingPathComponent("account.json")
        try encoder.encode(file).write(to: url, options: .atomic)
        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: url.path)
    }

    /// Every account directory that holds something, whoever owns it.
    public func saved() -> [SavedAccountData] {
        let manager = FileManager.default
        guard let names = try? manager.contentsOfDirectory(atPath: root.path) else { return [] }
        return names.sorted().compactMap { name -> SavedAccountData? in
            guard let directory = try? directory(name),
                  let values = try? directory.resourceValues(forKeys: [.isDirectoryKey, .isSymbolicLinkKey]),
                  values.isDirectory == true, values.isSymbolicLink != true else { return nil }
            let file = (try? Data(contentsOf: directory.appendingPathComponent("account.json")))
                .flatMap { try? JSONDecoder().decode(AccountFile.self, from: $0) }
                // A file that names another directory says nothing about this one.
                .flatMap { $0.id.raw == name ? $0 : nil }
            var bytes: Int64 = 0
            var latest: Date?
            var files = 0
            if let walker = manager.enumerator(at: directory, includingPropertiesForKeys: [.isRegularFileKey, .fileSizeKey, .contentModificationDateKey]) {
                for case let url as URL in walker {
                    guard let item = try? url.resourceValues(forKeys: [.isRegularFileKey, .fileSizeKey, .contentModificationDateKey]),
                          item.isRegularFile == true, url.lastPathComponent != "account.json" else { continue }
                    files += 1
                    bytes += Int64(item.fileSize ?? 0)
                    if let date = item.contentModificationDate, date > latest ?? .distantPast { latest = date }
                }
            }
            // An empty directory holds nothing to keep or to erase, unless it says whose it was.
            guard files > 0 || file != nil else { return nil }
            return SavedAccountData(directory: name, account: file, bytes: bytes, lastChanged: latest, summary: summary(name))
        }
    }

    /// Counts the recordings in an account's directory from the delivery
    /// journal an earlier version kept, its capture ledger and the capture
    /// folders themselves. This version uploads no audio, so the journal is
    /// only ever one left behind.
    public func summary(_ name: String) -> AccountDataSummary {
        guard let directory = try? directory(name) else { return AccountDataSummary() }
        let manager = FileManager.default
        var notes = Set<String>(), unsent = Set<String>(), deleted = Set<String>()
        if let data = try? Data(contentsOf: directory.appendingPathComponent("outbox.json")),
           let entries = try? JSONDecoder().decode([LegacyDelivery].self, from: data) {
            for entry in entries {
                let note = entry.noteID.lowercased()
                notes.insert(note)
                if entry.state == "pending" { unsent.insert(note) }
            }
        }
        if let data = try? Data(contentsOf: directory.appendingPathComponent("captures.json")),
           let ledger = try? JSONDecoder().decode([String: Ledger].self, from: data) {
            for (note, entry) in ledger where entry.serverMissingSince != nil { deleted.insert(note.lowercased()) }
        }
        let recordings = directory.appendingPathComponent("recordings", isDirectory: true)
        for folder in (try? manager.contentsOfDirectory(atPath: recordings.path)) ?? [] {
            guard UUID(uuidString: folder) != nil,
                  let files = try? manager.contentsOfDirectory(atPath: recordings.appendingPathComponent(folder).path),
                  files.contains(where: { $0.hasSuffix(".wav") }) else { continue }
            let note = folder.lowercased()
            notes.insert(note)
            // A capture whose journal is still beside it was never handed to the outbox whole.
            if files.contains(where: { $0.hasSuffix(".capture.json") }) { unsent.insert(note) }
        }
        return AccountDataSummary(recordings: notes.count, unsent: unsent.count, deletedOnServer: deleted.intersection(notes).count)
    }

    private struct Ledger: Decodable { let serverMissingSince: Date? }
    private struct LegacyDelivery: Decodable { let noteID: String; let state: String }

    /// Deletes an account's directory and everything in it. Only a directory
    /// directly under the root, named by an account id, that is a real directory.
    public func erase(_ name: String) throws {
        let directory = try directory(name)
        let values = try? directory.resourceValues(forKeys: [.isDirectoryKey, .isSymbolicLinkKey])
        guard let values else { return }
        guard values.isDirectory == true, values.isSymbolicLink != true,
              directory.deletingLastPathComponent().standardizedFileURL.path == root.standardizedFileURL.path else {
            throw APIClientError.invalidIdentifier
        }
        do { try FileManager.default.removeItem(at: directory) }
        catch { throw APIClientError.localStorage }
    }
}

/// Sessions held in memory, for tests and for the debug run with several accounts.
public final class MemoryAccountStore: AccountCredentialStore, @unchecked Sendable {
    private let lock = NSLock()
    private var items: [AccountID: StoredSession] = [:]
    /// When set, every read fails as a locked Keychain does.
    public var unreadable = false
    public var failsToSave = false

    public init() {}

    public func load(_ id: AccountID) throws -> StoredSession? {
        lock.lock(); defer { lock.unlock() }
        if unreadable { throw KeychainError(status: errSecInteractionNotAllowed) }
        return items[id]
    }

    public func save(_ credentials: InstanceCredentials, userID: String, for id: AccountID) throws {
        lock.lock(); defer { lock.unlock() }
        if failsToSave { throw KeychainError(status: errSecInteractionNotAllowed) }
        items[id] = StoredSession(credentials: credentials, userID: userID)
    }

    public func clear(_ id: AccountID) throws {
        lock.lock(); defer { lock.unlock() }
        items[id] = nil
    }

    public var count: Int { lock.lock(); defer { lock.unlock() }; return items.count }
    public var ids: [AccountID] { lock.lock(); defer { lock.unlock() }; return Array(items.keys) }
}
