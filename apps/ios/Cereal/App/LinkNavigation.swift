import Foundation
import APIClient

/// Where a link leads. Pure, so the rules can be tested: a note, shared or
/// one's own, opens only with a session on its own server; on any other
/// server, or signed out, the app says where the note is. No link is ever
/// dropped without a word.
enum LinkNavigation {
    enum Destination: Equatable {
        /// A page for a note this app cannot read here, placed over wherever the user is.
        case push(Route)
        /// A note of the signed-in account: its page replaces the stack.
        case serverNote(String)
        /// A note someone else shared on the signed-in server. It is read with
        /// that account, so the local demo is left for it.
        case sharedOnServer(SharedNoteLink)
    }

    static func destination(for link: AppLink, instance: InstanceURL?, signedIn: Bool) -> Destination? {
        switch link {
        case .shared(let shared):
            if signedIn, let instance, shared.origin.isInstance(instance) { .sharedOnServer(shared) }
            else { .push(.sharedNote(shared)) }
        case .note(let note):
            if signedIn, let instance, note.origin.isInstance(instance) { .serverNote(note.noteID) }
            else { .push(.noteElsewhere(note)) }
        // Only the sign-in sheet that is waiting for an answer can use one.
        case .auth: nil
        }
    }

    /// What the shared-note page may do with a link.
    enum SharedAccess: Equatable {
        /// The app is signed in to the link's own server and showing that account.
        case read
        /// Nothing is fetched. `offered` is whether the page offers to sign in.
        case signIn(offered: Bool)
    }

    static func sharedAccess(to origin: ServerOrigin, instance: InstanceURL?, signedIn: Bool, demo: Bool) -> SharedAccess {
        if signedIn, !demo, let instance, origin.isInstance(instance) { return .read }
        return .signIn(offered: offersSignIn(signedIn: signedIn, demo: demo))
    }

    /// Signing in is offered where it does not replace an account in use:
    /// when nobody is signed in, or the local demo is what is on screen.
    static func offersSignIn(signedIn: Bool, demo: Bool) -> Bool { !signedIn || demo }

    /// Opening what is already on top does not stack a second copy of it.
    static func path(_ path: [Route], pushing route: Route) -> [Route] {
        path.last == route ? path : path + [route]
    }

    /// The stack after the account changed. Screens of the previous account go.
    /// The pages that decide for themselves what the account may see stay (a
    /// shared note reads again, or asks for a sign-in), and so does a note
    /// that a link has just opened in the account now announced.
    static func pathAfterAccountChange(_ path: [Route], linkOpenedIn: String??, account: String?) -> [Route] {
        if let linkOpenedIn, linkOpenedIn == account { return path }
        return path.filter(\.belongsToNoAccount)
    }

    /// The stack after signing in from the connection sheet: Home, unless the
    /// user was looking at a note on the server they have just signed in to.
    /// That note is what they signed in for, and it opens.
    static func pathAfterSignIn(_ path: [Route], instance: InstanceURL?) -> [Route] {
        guard let instance else { return [] }
        switch path.last {
        case .noteElsewhere(let note)? where note.origin.isInstance(instance): return [.serverNote(note.noteID)]
        case .sharedNote(let shared)? where shared.origin.isInstance(instance): return path.filter(\.belongsToNoAccount)
        default: return []
        }
    }
}

extension Route {
    /// True for a screen that is not part of one account's notes or of the local
    /// demo: it stays when the account changes, and shows what the new one may see.
    var belongsToNoAccount: Bool {
        switch self {
        case .sharedNote, .sharedWithMe, .noteElsewhere: true
        case .note, .chat, .serverNote, .serverNoteSource, .serverChat, .search: false
        }
    }
}

extension Router {
    /// Follows a link from outside the app, from "Shared with me" or from a pasted link.
    func open(_ link: AppLink, server: ServerNotesModel, mode: ConnectionMode) {
        guard let destination = LinkNavigation.destination(for: link, instance: server.instance, signedIn: server.session != nil) else { return }
        showConnection = false
        SidebarModel.shared.close()
        switch destination {
        case .push(let route): path = LinkNavigation.path(path, pushing: route)
        case .serverNote(let id):
            mode.isDemo = false
            path = [.serverNote(id)]
            linkOpenedIn = .some(server.accountIdentity)
        case .sharedOnServer(let shared):
            // Over the account's own screens; the local demo's screens are left behind with it.
            path = mode.isDemo ? [.sharedNote(shared)] : LinkNavigation.path(path, pushing: .sharedNote(shared))
            mode.isDemo = false
        }
    }

    /// Opens the connection sheet to sign in to `origin`, with its address filled in.
    func signIn(to origin: ServerOrigin) {
        signInAddress = origin.description
        showConnection = true
    }
}
