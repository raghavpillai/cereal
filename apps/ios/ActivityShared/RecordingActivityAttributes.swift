import ActivityKit
import Foundation

/// Shared by the app and widget extension. The note's title is the only thing
/// of the note's that enters ActivityKit: no audio, transcript, or credentials.
struct RecordingActivityAttributes: ActivityAttributes {
    enum Presentation: String, Codable { case preview }
    struct ContentState: Codable, Hashable {
        enum Phase: String, Codable, Hashable { case recording, paused, ended }
        var phase: Phase
        var elapsed: TimeInterval
        var timerAnchor: Date
        var updatedAt: Date
        /// The note's title, shown where the activity has room for it. Nil for a
        /// note that has none yet, and in activities from before titles were shown.
        var title: String? = nil
        /// The account the recording goes to, as the person knows it, and its
        /// server's host. Set only when there is more than one account to tell
        /// apart; never an email or anything secret. Optional, so activities
        /// from before accounts decode.
        var accountLabel: String? = nil
        var accountHost: String? = nil

        /// The title as it is shown: trimmed, on one line, and not at all when empty.
        static func shownTitle(_ text: String) -> String? {
            let line = text.split(whereSeparator: \.isNewline).joined(separator: " ").trimmingCharacters(in: .whitespaces)
            return line.isEmpty ? nil : String(line.prefix(80))
        }

        func status(isStale: Bool, isPreview: Bool = false) -> String {
            if isStale { return isPreview ? "Open to check preview" : "Open to check recording" }
            if isPreview {
                switch phase {
                case .recording: return "Live Activity preview"
                case .paused: return "Preview paused"
                case .ended: return "Preview ended"
                }
            }
            switch phase {
            case .recording: return "Recording audio"
            case .paused: return "Recording paused"
            case .ended: return "Recording ended"
            }
        }
        func symbol(isStale: Bool, isPreview: Bool = false) -> String {
            isStale ? "exclamationmark.circle" : phase == .recording ? (isPreview ? "play.circle" : "mic.fill") : phase == .paused ? "pause.fill" : "checkmark"
        }
        func runsTimer(isStale: Bool) -> Bool { phase == .recording && !isStale }
    }

    var noteID: UUID
    // Optional preserves decoding of activities created before preview QA existed.
    var presentation: Presentation? = nil
    /// The account the note belongs to: 64 lower-case hex characters. Nil in
    /// activities from before accounts, and for the local demo.
    var accountID: String? = nil
    var isPreview: Bool { presentation == .preview }

    /// `cereal://recording/<note>?a=<account>`. The link only ever opens a note.
    var recordingURL: URL {
        let base = "cereal://\(isPreview ? "live-activity-preview" : "recording")/\(noteID.uuidString)"
        guard !isPreview, let accountID, accountID.utf8.count == 64,
              accountID.utf8.allSatisfy({ (48...57).contains($0) || (97...102).contains($0) }) else { return URL(string: base)! }
        return URL(string: base + "?a=" + accountID)!
    }
}
