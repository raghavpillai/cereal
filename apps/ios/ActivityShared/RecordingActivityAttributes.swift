import ActivityKit
import Foundation

/// Shared by the app and widget extension. No audio, transcript, or credentials enter ActivityKit.
struct RecordingActivityAttributes: ActivityAttributes {
    enum Presentation: String, Codable { case preview }
    struct ContentState: Codable, Hashable {
        enum Phase: String, Codable, Hashable { case recording, paused, ended }
        var phase: Phase
        var elapsed: TimeInterval
        var timerAnchor: Date
        var updatedAt: Date

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
    var isPreview: Bool { presentation == .preview }

    var recordingURL: URL {
        URL(string: "cereal://\(isPreview ? "live-activity-preview" : "recording")/\(noteID.uuidString)")!
    }
}
