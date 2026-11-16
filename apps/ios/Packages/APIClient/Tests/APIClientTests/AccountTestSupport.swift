import Foundation
@testable import APIClient

/// The one-connection form many tests are written in: a model that is handed
/// a session directly, without the account list around it.
@MainActor extension ServerNotesModel {
    func connect(instance text: String, token: String) async {
        do { _ = await connect(try InstanceCredentials(instance: InstanceURL(text), bearerToken: token)) }
        catch {
            close()
            markUnreachable(error as? APIClientError ?? .invalidResponse)
        }
    }

    func disconnect() throws { close() }
}
