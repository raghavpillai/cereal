import ActivityKit
import AppIntents
import Foundation

/// End, on the recording's Live Activity (the Dynamic Island and the Lock
/// Screen), as in the reference. A Live Activity intent runs in the app's
/// process, where the recorder lives; the widget extension only draws the button.
///
/// There is no Pause key beside it. Resuming from the Lock Screen would have to
/// start the microphone from the background, which iOS refuses to an ordinary
/// intent, and a paused app is soon suspended, so the activity could not keep
/// its promise. Pause and resume are on the recording screen.
struct EndRecordingIntent: LiveActivityIntent {
    static let title: LocalizedStringResource = "End recording"
    static let description = IntentDescription("Ends the recording and writes its notes.")
    static let isDiscoverable = false

    @Parameter(title: "Note") var noteID: String

    init() {}
    init(noteID: UUID) { self.noteID = noteID.uuidString }

    func perform() async throws -> some IntentResult {
        let id = noteID
        await RecordingActivityActions.end(noteID: id)
        return .result()
    }
}

/// Where the app's recorder takes the Live Activity's End. Set by whoever owns
/// the recording; empty in the widget extension, which never runs the intent.
@MainActor
enum RecordingActivityActions {
    /// What End on the activity does, and whose it is: the server's recordings or
    /// the local demo's screen. Only the owner that set it can clear it.
    private static var handler: (owner: String, end: @MainActor (UUID) -> Bool)?

    static func setHandler(owner: String, _ end: @escaping @MainActor (UUID) -> Bool) { handler = (owner, end) }
    static func clearHandler(owner: String) { if handler?.owner == owner { handler = nil } }

    /// Who handles End for this note: the owner's handler, or nobody (an activity
    /// left behind by a recording this process does not have, which End just removes).
    enum Route: Equatable { case handler, orphan, invalid }

    static func route(noteID: String, hasHandler: Bool) -> Route {
        guard UUID(uuidString: noteID) != nil else { return .invalid }
        return hasHandler ? .handler : .orphan
    }

    static func end(noteID: String) async {
        switch route(noteID: noteID, hasHandler: handler != nil) {
        case .invalid: return
        case .handler:
            guard let id = UUID(uuidString: noteID), let handler else { return }
            if handler.end(id) { return }
            // The owner is recording something else: this activity is a leftover.
            await endLeftover(id)
        case .orphan:
            guard let id = UUID(uuidString: noteID) else { return }
            await endLeftover(id)
        }
    }

    /// Nothing in this process records the note (the app was relaunched by the
    /// tap itself), so End removes what can no longer be true.
    private static func endLeftover(_ id: UUID) async {
        for activity in Activity<RecordingActivityAttributes>.activities where activity.attributes.noteID == id && !activity.attributes.isPreview {
            var state = activity.content.state
            state.phase = .ended; state.updatedAt = .now
            await activity.end(ActivityContent(state: state, staleDate: nil), dismissalPolicy: .immediate)
        }
    }
}
