import Foundation

/// The names this app is known by outside itself. The bundle identifiers are
/// set in `project.yml`: `dev.cereal.mobile`, its recording extension
/// `dev.cereal.mobile.recording` and its tests `dev.cereal.mobile.tests`.
enum AppIdentity {
    /// Subsystem of every log and signpost, and the prefix of queue labels.
    static let subsystem = "dev.cereal.mobile"
    /// The one URL scheme the app answers to: the Live Activity's link, links to
    /// notes and shared notes, and the answer to a sign-in made in the browser.
    static let urlScheme = "cereal"
}
