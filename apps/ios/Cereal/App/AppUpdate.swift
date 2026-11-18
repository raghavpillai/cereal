import Foundation

/// Whether the server this app is signed in to is a later version than the app.
/// The server and its apps carry one product version, so a later server means a
/// later app exists; an app left behind may not understand everything it is sent.
enum AppUpdate {
    /// What the launch prompt offers: the version, and the App Store page when the server names one.
    struct Offer: Equatable, Identifiable {
        let version: String
        let store: URL?
        var id: String { version }
    }

    /// This app's version, from its bundle.
    static var current: String { Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "" }

    /// The numbers of a dotted version, without a pre-release or build suffix.
    static func numbers(_ version: String) -> [Int]? {
        guard version.first?.isNumber == true, let core = version.split(whereSeparator: { $0 == "-" || $0 == "+" }).first else { return nil }
        let parts = core.split(separator: ".", omittingEmptySubsequences: false).map { Int($0) }
        guard !parts.isEmpty, parts.count <= 4, parts.allSatisfy({ ($0 ?? -1) >= 0 }) else { return nil }
        return parts.compactMap { $0 }
    }

    /// True when `candidate` is strictly later than `current`, number by number. False when
    /// either is not a dotted version: the app never claims an update it cannot be sure of.
    static func newer(_ candidate: String, than current: String) -> Bool {
        guard var left = numbers(candidate), var right = numbers(current) else { return false }
        let count = max(left.count, right.count)
        left += Array(repeating: 0, count: count - left.count)
        right += Array(repeating: 0, count: count - right.count)
        for (a, b) in zip(left, right) where a != b { return a > b }
        return false
    }

    /// The offer a server's answer makes to an app at `current`, when it makes one. Only an
    /// https App Store address is ever opened.
    static func offer(serverVersion: String, storeURL: String?, current: String = current) -> Offer? {
        guard newer(serverVersion, than: current) else { return nil }
        let store = storeURL.flatMap(URL.init(string:)).flatMap { $0.scheme == "https" && $0.host == "apps.apple.com" ? $0 : nil }
        return Offer(version: serverVersion, store: store)
    }

    static let title = "Update available"
    static func message(_ offer: Offer) -> String {
        "Cereal \(offer.version) is available. Some functionality may be degraded if you don't update."
    }
}
