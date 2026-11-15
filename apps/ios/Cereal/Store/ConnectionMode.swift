import Foundation
import Observation

/// Demo notes and server notes have separate stores and navigation. Switching
/// modes never uploads recordings or fictional example notes.
@MainActor @Observable
final class ConnectionMode {
    var isDemo: Bool {
        didSet {
            previewLabel = nil
            defaults.set(!isDemo, forKey: "cereal.serverMode")
        }
    }
    var previewLabel: String?
    @ObservationIgnored private let defaults: UserDefaults

    init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
        self.isDemo = !defaults.bool(forKey: "cereal.serverMode")
    }
}
