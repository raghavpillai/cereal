import Foundation
import Observation

/// The app needs a Cereal server: every launch shows the server's notes, or
/// the sign-in screen when there is no account yet.
///
/// Debug builds keep the old on-device demo for QA only. `-screen` launches
/// and the Live Activity harness switch to it for that run; nothing switches
/// to it from the interface and it is never remembered. In a release build
/// `isDemo` is always false.
@MainActor @Observable
final class ConnectionMode {
    #if DEBUG
    var isDemo = false {
        didSet { previewLabel = nil }
    }
    #else
    var isDemo: Bool { false }
    #endif
    var previewLabel: String?

    init() {}
}
