import Foundation
import APIClient

/// Where a link leads. Pure, so the rules can be tested: a note, shared or
/// one's own, opens only with a session on its own server; on any other
/// server, or signed out, the app says where the note is. No link is ever
/// dropped without a word.
enum LinkNavigation {
    /// What a link needs to know about an account: where it is, and whether its
    /// session has been confirmed. Accounts are given in the person's order.
    struct Account: Equatable {
        let id: AccountID
        let instance: InstanceURL?
        let isReady: Bool
    }

    enum Destination: Equatable {
        /// A page for a note this app cannot read here, placed over wherever the user is.
        case push(Route)
        /// A note of an account on the link's server: its page replaces the stack.
        case serverNote(NoteRef)
        /// A note someone else shared on a server an account is signed in to. It is
        /// read with that account, so the local demo is left for it.
        case sharedOnServer(SharedNoteLink)
    }

    /// The accounts a link may be read with: Ready, on the link's own origin, with no
    /// path in their address, in the person's order. A link goes to nobody else.
    static func candidates(for origin: ServerOrigin, among accounts: [Account]) -> [Account] {
        accounts.filter { account in account.isReady && (account.instance.map(origin.isInstance) ?? false) }
    }

    static func destination(for link: AppLink, accounts: [Account]) -> Destination? {
        switch link {
        case .shared(let shared):
            candidates(for: shared.origin, among: accounts).isEmpty ? .push(.sharedNote(shared)) : .sharedOnServer(shared)
        case .note(let note):
            if let account = candidates(for: note.origin, among: accounts).first { .serverNote(NoteRef(account: account.id, id: note.noteID)) }
            else { .push(.noteElsewhere(note)) }
        // Only the sign-in sheet that is waiting for an answer can use one.
        case .auth: nil
        }
    }

    /// What the shared-note page may do with a link.
    enum SharedAccess: Equatable {
        /// An account is signed in to the link's own server: the note is read with it.
        case read(AccountID)
        /// Nothing is fetched. `offered` is whether the page offers to sign in.
        case signIn(offered: Bool)
    }

    static func sharedAccess(to origin: ServerOrigin, accounts: [Account], demo: Bool) -> SharedAccess {
        if !demo, let account = candidates(for: origin, among: accounts).first { return .read(account.id) }
        return .signIn(offered: offersSignIn(signedIn: !accounts.isEmpty, demo: demo))
    }

    /// Signing in is offered where it does not replace an account in use:
    /// when there is no account, or the local demo is what is on screen. A
    /// link never starts a sign-in to a new address while an account exists.
    static func offersSignIn(signedIn: Bool, demo: Bool) -> Bool { !signedIn || demo }

    /// Opening what is already on top does not stack a second copy of it.
    static func path(_ path: [Route], pushing route: Route) -> [Route] {
        path.last == route ? path : path + [route]
    }

    /// The stack after accounts have gone: their screens go with them. The local
    /// demo's screens, and the pages that decide for themselves what may be
    /// seen (a shared note reads again, or asks for a sign-in), stay.
    static func path(_ path: [Route], removing accounts: Set<AccountID>) -> [Route] {
        path.filter { route in route.account.map { !accounts.contains($0) } ?? true }
    }

    static func path(_ path: [Route], removing account: AccountID) -> [Route] {
        self.path(path, removing: [account])
    }

    /// The stack after signing in from the connection sheet: Home, unless the
    /// user was looking at a note on the server they have just signed in to.
    /// That note is what they signed in for, and it opens.
    static func pathAfterSignIn(_ path: [Route], account: Account?) -> [Route] {
        guard let account, let instance = account.instance else { return [] }
        switch path.last {
        case .noteElsewhere(let note)? where note.origin.isInstance(instance): return [.serverNote(NoteRef(account: account.id, id: note.noteID))]
        case .sharedNote(let shared)? where shared.origin.isInstance(instance): return path.filter(\.belongsToNoAccount)
        default: return []
        }
    }
}

extension Route {
    /// The account whose screen this is. Nil for the local demo's screens and for
    /// the pages that are not part of one account's notes.
    var account: AccountID? {
        switch self {
        case .serverNote(let ref), .serverNoteSource(let ref, _): ref.account
        case .serverChat(let account, _): account
        case .note, .chat, .search, .sharedNote, .sharedWithMe, .noteElsewhere: nil
        }
    }

    /// True for a screen that is not part of one account's notes or of the local
    /// demo: it stays whatever happens to the accounts, and shows what they may see.
    var belongsToNoAccount: Bool {
        switch self {
        case .sharedNote, .sharedWithMe, .noteElsewhere: true
        case .note, .chat, .serverNote, .serverNoteSource, .serverChat, .search: false
        }
    }
}

extension AccountsModel {
    /// The accounts as links see them.
    var linkAccounts: [LinkNavigation.Account] {
        sessions.map { LinkNavigation.Account(id: $0.id, instance: $0.record.instance, isReady: $0.isReady) }
    }
}

extension Router {
    /// Follows a link from outside the app, from "Shared with me" or from a pasted link.
    func open(_ link: AppLink, accounts: AccountsModel, mode: ConnectionMode) {
        guard let destination = LinkNavigation.destination(for: link, accounts: accounts.linkAccounts) else { return }
        showConnection = false
        SidebarModel.shared.close()
        switch destination {
        case .push(let route): path = LinkNavigation.path(path, pushing: route)
        case .serverNote(let ref):
            mode.isDemo = false
            path = [.serverNote(ref)]
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
