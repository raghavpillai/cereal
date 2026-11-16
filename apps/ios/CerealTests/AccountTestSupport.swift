import Foundation
import APIClient
import Contracts
@testable import Cereal

/// The one-account form these tests are written in: a model handed a session
/// directly, and a recorder with that one account.
@MainActor extension ServerNotesModel {
    func connect(instance text: String, token: String) async {
        guard let credentials = try? InstanceCredentials(instance: InstanceURL(text), bearerToken: token) else { return }
        _ = await connect(credentials)
    }
}

@MainActor extension ServerRecordingController {
    func attach(to server: ServerNotesModel) async { await sync(with: [server]) }
    private var only: AccountID? { deliveringAccounts.count == 1 ? deliveringAccounts[0] : nil }
    func ref(_ noteID: String) -> NoteRef? { only.map { NoteRef(account: $0, id: noteID) } }
    func start(note: NoteDTO) async { await start(note: note, in: only) }
    func hasPendingAudio(noteID: String) -> Bool { hasPendingAudio(ref(noteID)) }
    var activeNoteID: String? { active?.id }
}
