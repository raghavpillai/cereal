import AppIntents
import Foundation

/// End, on the recording's Live Activity (the Dynamic Island and the Lock
/// Screen), as in the reference. A Live Activity intent runs in the app's
/// process, where the recorder lives; the widget extension only draws the button.
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
    static var endHandler: (@MainActor (UUID) -> Void)?

    static func end(noteID: String) {
        guard let id = UUID(uuidString: noteID) else { return }
        endHandler?(id)
    }
}
