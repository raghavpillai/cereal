import ActivityKit
import Foundation

/// Shared by the app and widget extension. The note's title is the only thing
/// of the note's that enters ActivityKit: no audio, transcript, or credentials.
struct RecordingActivityAttributes: ActivityAttributes {
    enum Presentation: String, Codable { case preview }
    struct ContentState: Codable, Hashable {
        /// - recording, paused: the microphone is in use, or held.
        /// - interrupted: it stopped without being asked to (a call, a lost headset);
        ///   the audio is kept and the person can resume in the app.
        /// - writing: End was chosen and the notes are being written.
        /// - ready: the notes have been written. Only ever set once they have.
        /// - ended: the recording is over and nothing more will be said about it here.
        enum Phase: String, Codable, Hashable { case recording, paused, ended, interrupted, writing, ready }
        /// What the person can do about an ended or interrupted recording, when there is something.
        enum Hint: String, Codable, Hashable {
            /// The audio is kept; the recording resumes from the app.
            case openToResume
            /// The notes will be written by the app (it could not follow them here).
            case notesInApp
            /// Nothing was transcribed, so there are no notes to write.
            case noTranscript
            /// Writing the notes failed; the app can try again.
            case notesFailed
        }
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
        /// Optional, so activities from before hints decode.
        var hint: Hint? = nil

        /// The title as it is shown: trimmed, on one line, and not at all when empty.
        static func shownTitle(_ text: String) -> String? {
            let line = text.split(whereSeparator: \.isNewline).joined(separator: " ").trimmingCharacters(in: .whitespaces)
            return line.isEmpty ? nil : String(line.prefix(80))
        }

        func status(isStale: Bool, isPreview: Bool = false) -> String {
            if isStale {
                switch phase {
                case .writing: return isPreview ? "Open to check preview" : "Open Cereal for your notes"
                default: return isPreview ? "Open to check preview" : "Open to check recording"
                }
            }
            if isPreview {
                switch phase {
                case .recording: return "Live Activity preview"
                case .paused: return "Preview paused"
                case .ended: return "Preview ended"
                case .interrupted: return "Preview interrupted"
                case .writing: return "Preview writing"
                case .ready: return "Preview ready"
                }
            }
            switch phase {
            case .recording: return "Recording audio"
            case .paused: return "Recording paused"
            case .ended: return "Recording ended"
            case .interrupted: return "Recording stopped"
            case .writing: return "Writing notes"
            case .ready: return "Notes ready"
            }
        }

        /// The large line of the activity: "Taking notes...", as in the reference, while recording.
        func headline(isStale: Bool, isPreview: Bool = false) -> String {
            if isStale || isPreview { return status(isStale: isStale, isPreview: isPreview) }
            switch phase {
            case .recording: return "Taking notes..."
            case .paused: return "Paused"
            case .ended: return "Recording ended"
            case .interrupted: return "Recording stopped"
            case .writing: return "Writing notes..."
            case .ready: return "Notes ready"
            }
        }

        /// Said beside the clock instead of the title, when there is something to do or to know.
        var detail: String? {
            switch hint {
            case .openToResume: "Open to resume"
            case .notesInApp: "Open Cereal for your notes"
            case .noTranscript: "Nothing was transcribed"
            case .notesFailed: "Notes couldn't be written"
            case nil: nil
            }
        }

        func symbol(isStale: Bool, isPreview: Bool = false) -> String {
            if isStale { return "exclamationmark.circle" }
            switch phase {
            case .recording: return isPreview ? "play.circle" : "mic.fill"
            case .paused: return "pause.fill"
            case .ended: return "checkmark"
            case .interrupted: return "exclamationmark.triangle.fill"
            case .writing: return "sparkles"
            case .ready: return "checkmark.circle.fill"
            }
        }
        /// The glyph on the corner of the app's mark: what the recording is doing
        /// when it is not simply recording (or simply over).
        func badge(isStale: Bool) -> String? {
            if isStale { return "exclamationmark" }
            switch phase {
            case .recording, .ended: return nil
            case .paused: return "pause.fill"
            case .interrupted: return "exclamationmark"
            case .writing: return "sparkles"
            case .ready: return "checkmark"
            }
        }
        func runsTimer(isStale: Bool) -> Bool { phase == .recording && !isStale }
        /// What follows the clock: what to do, or else the note's title.
        var secondary: String? { detail ?? title }
        /// The microphone is in use or held: End is offered.
        var isLive: Bool { phase == .recording || phase == .paused }
        /// Opening the note is offered as a key of its own.
        var offersOpen: Bool { phase == .ready || phase == .interrupted }

        /// How the system ranks this activity against the app's others (an
        /// answer arriving in chat): the recording always comes first.
        var relevance: Double {
            switch phase {
            case .recording, .paused, .interrupted: 100
            case .writing: 75
            case .ready: 50
            case .ended: 10
            }
        }

        /// Until the next heartbeat; past it the activity says to check the app.
        /// Only a running clock and notes being written can go stale: the others
        /// say something that stays true.
        func staleDate(from date: Date) -> Date? {
            switch phase {
            case .recording: date.addingTimeInterval(RecordingActivityTiming.recordingStaleAfter)
            case .writing: date.addingTimeInterval(RecordingActivityTiming.writingStaleAfter)
            case .paused, .interrupted, .ready, .ended: nil
            }
        }
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

/// How long each state of the activity lasts.
enum RecordingActivityTiming {
    /// The recorder's heartbeat is every 30 s while it runs, in the background too.
    static let recordingStaleAfter: TimeInterval = 75
    /// Notes usually take well under a minute; past this the app has lost track.
    static let writingStaleAfter: TimeInterval = 5 * 60
    /// "Notes ready" stays on the Lock Screen this long.
    static let readyLingers: TimeInterval = 3 * 60
    /// An ended recording with something to say ("Open Cereal for your notes").
    static let hintLingers: TimeInterval = 2 * 60
    /// A plain "Recording ended".
    static let endedLingers: TimeInterval = 30
    /// "Notes ready" shows in the Dynamic Island this long before the activity ends.
    static let readyHold: Duration = .seconds(4)
}

/// When an ended activity leaves the Lock Screen. Never longer than a few minutes.
enum RecordingActivityDismissal: Equatable, Sendable {
    case immediate
    case after(Date)

    static func forEnding(_ state: RecordingActivityAttributes.ContentState, discarded: Bool, now: Date) -> RecordingActivityDismissal {
        if discarded { return .immediate }
        switch state.phase {
        case .ready: return .after(now.addingTimeInterval(RecordingActivityTiming.readyLingers))
        case .recording, .paused, .writing:
            // Ending from one of these means the app gave up on it (replaced, or a launch
            // cleaning up): nothing true is left to show.
            return .immediate
        case .ended, .interrupted:
            return .after(now.addingTimeInterval(state.hint == nil ? RecordingActivityTiming.endedLingers : RecordingActivityTiming.hintLingers))
        }
    }

    var policy: ActivityUIDismissalPolicy {
        switch self {
        case .immediate: .immediate
        case .after(let date): .after(date)
        }
    }
}

/// An alert (the activity lights up the screen with a sound) is kept for a
/// recording that stopped on its own and needs the person: never for a pause,
/// a heartbeat, the title, or the notes.
struct RecordingActivityAlert: Equatable, Sendable {
    let title: String
    let body: String

    /// Why a recording stopped without being asked to.
    enum Cause: Equatable, Sendable {
        /// A call or Siri took the audio. The person knows; it is shown, not sounded.
        case interruption
        /// The microphone went away (a headset disconnected) or the audio system failed.
        case failure
    }

    static func forStop(_ cause: Cause, appActive: Bool) -> RecordingActivityAlert? {
        // In the app, the recording screen already says so.
        guard !appActive, cause == .failure else { return nil }
        return RecordingActivityAlert(title: "Recording stopped", body: "Your audio is kept. Open Cereal to resume.")
    }

    var configuration: AlertConfiguration {
        AlertConfiguration(title: LocalizedStringResource(stringLiteral: title), body: LocalizedStringResource(stringLiteral: body), sound: .default)
    }
}
