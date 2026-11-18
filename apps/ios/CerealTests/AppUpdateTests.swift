import Foundation
import Testing
@testable import Cereal

struct AppUpdateTests {
    @Test func onlyAStrictlyLaterServerIsAnUpdate() {
        #expect(AppUpdate.newer("0.2.0", than: "0.1.0"))
        #expect(AppUpdate.newer("0.1.1", than: "0.1"))
        #expect(AppUpdate.newer("1.0.0-beta.2", than: "0.9.9"))
        #expect(AppUpdate.newer("0.10.0", than: "0.9.0"))
        #expect(!AppUpdate.newer("0.1.0", than: "0.1.0"))
        #expect(!AppUpdate.newer("0.0.1", than: "0.1.0"))
        #expect(!AppUpdate.newer("0.1.0+build5", than: "0.1.0"))
    }

    @Test func whatIsNotAVersionIsNeverAnUpdate() {
        #expect(!AppUpdate.newer("latest", than: "0.1.0"))
        #expect(!AppUpdate.newer("1..2", than: "0.1.0"))
        #expect(!AppUpdate.newer("2.0.0", than: ""))
        #expect(!AppUpdate.newer("-1.0.0", than: "0.1.0"))
    }

    @Test func theOfferOpensOnlyTheAppStore() throws {
        let offer = try #require(AppUpdate.offer(serverVersion: "0.2.0", storeURL: "https://apps.apple.com/app/id123", current: "0.1.0"))
        #expect(offer.store?.absoluteString == "https://apps.apple.com/app/id123")
        #expect(AppUpdate.message(offer) == "Cereal 0.2.0 is available. Some functionality may be degraded if you don't update.")
        #expect(AppUpdate.offer(serverVersion: "0.2.0", storeURL: "https://example.com/app", current: "0.1.0")?.store == nil)
        #expect(AppUpdate.offer(serverVersion: "0.2.0", storeURL: nil, current: "0.1.0")?.store == nil)
        #expect(AppUpdate.offer(serverVersion: "0.1.0", storeURL: nil, current: "0.1.0") == nil)
    }
}
