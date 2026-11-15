import Foundation
import Security

public protocol BearerTokenStore: Sendable {
    func load() throws -> String?
    func save(_ token: String) throws
    func clear() throws
}

/// A single-instance token, local to this device and excluded from iCloud Keychain.
public struct KeychainTokenStore: BearerTokenStore {
    private let service: String
    private let account: String

    public init(service: String = "dev.meetingnotes.instance", account: String = "owner-token") {
        self.service = service
        self.account = account
    }

    private var query: [String: Any] {
        [kSecClass as String: kSecClassGenericPassword,
         kSecAttrService as String: service,
         kSecAttrAccount as String: account,
         kSecAttrSynchronizable as String: false]
    }

    public func load() throws -> String? {
        var query = query
        query[kSecReturnData as String] = true
        query[kSecMatchLimit as String] = kSecMatchLimitOne
        var result: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        if status == errSecItemNotFound { return nil }
        guard status == errSecSuccess else { throw KeychainError(status: status) }
        guard let data = result as? Data, let value = String(data: data, encoding: .utf8) else {
            throw KeychainError(status: errSecDecode)
        }
        return value
    }

    public func save(_ token: String) throws {
        let data = Data(token.utf8)
        let status = SecItemUpdate(query as CFDictionary, [kSecValueData as String: data] as CFDictionary)
        if status == errSecSuccess { return }
        guard status == errSecItemNotFound else { throw KeychainError(status: status) }
        var item = query
        item[kSecValueData as String] = data
        item[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        let added = SecItemAdd(item as CFDictionary, nil)
        guard added == errSecSuccess else { throw KeychainError(status: added) }
    }

    public func clear() throws {
        let status = SecItemDelete(query as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else { throw KeychainError(status: status) }
    }
}

public struct KeychainError: Error, LocalizedError, Sendable {
    public let status: OSStatus
    public var errorDescription: String? {
        "The access token could not be read or saved securely on this device."
    }
}

/// Preferences contain only the non-secret instance URL; tokens remain in Keychain.
/// Keep this store on the main actor when integrating it into a SwiftUI app model.
@MainActor
public final class ConnectionSettingsStore {
    private let defaults: UserDefaults
    private let tokens: any BearerTokenStore
    private let urlKey: String

    public init(
        defaults: UserDefaults = .standard,
        tokens: any BearerTokenStore = KeychainTokenStore(),
        urlKey: String = "meetingNotes.instanceURL"
    ) {
        self.defaults = defaults
        self.tokens = tokens
        self.urlKey = urlKey
    }

    public var savedInstance: InstanceURL? {
        guard let text = defaults.string(forKey: urlKey) else { return nil }
        return try? InstanceURL(text)
    }

    public func load() throws -> InstanceCredentials? {
        guard let instance = savedInstance, let token = try tokens.load() else { return nil }
        return try InstanceCredentials(instance: instance, bearerToken: token)
    }

    /// Validate a replacement token before saving it. A changed server must never
    /// silently inherit the old server's credential when a settings field is blank.
    public func credentials(instance: InstanceURL, enteredToken: String) throws -> InstanceCredentials {
        let token: String
        if enteredToken.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            guard savedInstance == instance, let saved = try tokens.load() else {
                throw APIClientError.invalidToken
            }
            token = saved
        } else { token = enteredToken }
        return try InstanceCredentials(instance: instance, bearerToken: token)
    }

    public func save(_ credentials: InstanceCredentials) throws {
        try tokens.save(credentials.bearerToken)
        defaults.set(credentials.instance.url.absoluteString, forKey: urlKey)
    }

    public func clear() throws {
        // Remove the restore pointer even if a locked Keychain refuses deletion.
        // A stale orphaned item must not silently reconnect on the next launch.
        defaults.removeObject(forKey: urlKey)
        try tokens.clear()
    }
}
