import Foundation
import Testing
import APIClient

/// Uses the app-hosted simulator Keychain and a unique disposable item. The
/// ordinary transport tests intentionally use an in-memory token store; they
/// cannot detect a missing application identifier in the installed signature.
@MainActor
struct KeychainIntegrationTests {
    @Test func signedAppCanSaveRestoreUpdateAndClearKeychainCredentials() throws {
        let domain = "Cereal-Keychain-QA-\(UUID().uuidString)"
        let defaults = try #require(UserDefaults(suiteName: domain))
        let keychain = KeychainTokenStore(service: domain, account: "test-only-owner")
        defer {
            try? keychain.clear()
            defaults.removePersistentDomain(forName: domain)
        }
        let settings = ConnectionSettingsStore(defaults: defaults, tokens: keychain)
        let instance = try InstanceURL("https://keychain-qa.invalid")
        try settings.save(InstanceCredentials(instance: instance, bearerToken: "disposable-test-value-one"))
        let firstValueMatches = try keychain.load() == "disposable-test-value-one"
        #expect(firstValueMatches)
        #expect(try settings.load()?.instance == instance)
        // Blank input follows the exact production reuse path, proving the
        // value can be read back without printing or exposing it in a test.
        _ = try settings.credentials(instance: instance, enteredToken: "")
        try settings.save(InstanceCredentials(instance: instance, bearerToken: "disposable-test-value-two"))
        let updatedValueMatches = try keychain.load() == "disposable-test-value-two"
        #expect(updatedValueMatches)
        _ = try settings.credentials(instance: instance, enteredToken: "")
        #expect(!String(describing: defaults.persistentDomain(forName: domain)).contains("disposable-test-value"))
        try settings.clear()
        #expect(try settings.load() == nil)
        #expect(try keychain.load() == nil)
    }
}
