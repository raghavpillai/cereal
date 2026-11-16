import Foundation
import Testing
import APIClient
@testable import Cereal

private let token = "Zk3vX9aQ2mN7pL5rT8wY1bC4dF6gH0jK-sU_eIoAxyz"
private let noteID = "3a49a4af-c660-42e9-a36a-a806fbb41e0d"

private func link(_ text: String) throws -> AppLink { try #require(AppLink(text: text)) }

private func sharedLink(_ host: String = "notes.example.com", scheme: String = "https") throws -> SharedNoteLink {
    guard case .shared(let shared) = try link("\(scheme)://\(host)/s/\(token)") else { throw APIClientError.invalidIdentifier }
    return shared
}

private func noteLink(_ origin: String = "https://notes.example.com") throws -> NoteLink {
    guard case .note(let note) = try link("\(origin)/notes/\(noteID)") else { throw APIClientError.invalidIdentifier }
    return note
}

private let accountA = AccountID(address: "https://notes.example.com", userID: "00000000-0000-4000-8000-00000000000a")
private let accountB = AccountID(address: "https://mine.example.com", userID: "00000000-0000-4000-8000-00000000000b")
private let ownNote = NoteRef(account: accountA, id: noteID)

/// An account as a link sees it: signed in and confirmed, unless `ready` is false.
private func account(_ address: String, id: AccountID = accountA, ready: Bool = true) throws -> LinkNavigation.Account {
    LinkNavigation.Account(id: id, instance: try InstanceURL(address), isReady: ready)
}

@MainActor struct LinkNavigationTests {
    @Test func aSharedNoteOpensInTheAccountOnlyOnItsOwnServer() throws {
        let shared = try sharedLink()
        // Someone else on my server shared it: it is read with my account there.
        #expect(LinkNavigation.destination(for: .shared(shared), accounts: [try account("https://notes.example.com")]) == .sharedOnServer(shared))
        #expect(LinkNavigation.destination(for: .shared(shared), accounts: [try account("https://NOTES.example.com:443/")]) == .sharedOnServer(shared))
        // Signed out, signed in elsewhere, or with an account whose session is not confirmed: the page that says where the note is.
        let prompt = LinkNavigation.Destination.push(.sharedNote(shared))
        #expect(LinkNavigation.destination(for: .shared(shared), accounts: []) == prompt)
        #expect(LinkNavigation.destination(for: .shared(shared), accounts: [try account("https://mine.example.com")]) == prompt)
        #expect(LinkNavigation.destination(for: .shared(shared), accounts: [try account("https://notes.example.com", ready: false)]) == prompt)
        #expect(LinkNavigation.destination(for: .shared(shared), accounts: [try account("https://notes.example.com:8443")]) == prompt)
        // An account whose address has a path is not "the server" a link names.
        #expect(LinkNavigation.destination(for: .shared(shared), accounts: [try account("https://notes.example.com/team")]) == prompt)
        // The app form and a universal link are the same link.
        #expect(try link("cereal://shared?url=https%3A%2F%2Fnotes.example.com%2Fs%2F\(token)") == .shared(shared))
    }

    @Test func aSharedNoteIsReadOnlyWithASessionOnItsOwnServer() throws {
        let origin = try sharedLink().origin
        let own = try account("https://notes.example.com")
        #expect(LinkNavigation.sharedAccess(to: origin, accounts: [own], demo: false) == .read(accountA))
        // Another server: nothing is fetched, and signing in is not offered over the account in use.
        #expect(LinkNavigation.sharedAccess(to: origin, accounts: [try account("https://mine.example.com")], demo: false) == .signIn(offered: false))
        #expect(LinkNavigation.sharedAccess(to: origin, accounts: [try account("https://notes.example.com:8443")], demo: false) == .signIn(offered: false))
        // Signed out, or in the local demo: nothing is fetched, and signing in is offered.
        #expect(LinkNavigation.sharedAccess(to: origin, accounts: [], demo: false) == .signIn(offered: true))
        #expect(LinkNavigation.sharedAccess(to: origin, accounts: [], demo: true) == .signIn(offered: true))
        #expect(LinkNavigation.sharedAccess(to: origin, accounts: [own], demo: true) == .signIn(offered: true))
        #expect(LinkNavigation.sharedAccess(to: origin, accounts: [try account("https://mine.example.com")], demo: true) == .signIn(offered: true))
        // An account that must sign in again reads nothing, and a link does not start that sign-in.
        #expect(LinkNavigation.sharedAccess(to: origin, accounts: [try account("https://notes.example.com", ready: false)], demo: false) == .signIn(offered: false))
        // `localhost` and `127.0.0.1` are two servers.
        let local = try sharedLink("127.0.0.1:3000", scheme: "http").origin
        #expect(LinkNavigation.sharedAccess(to: local, accounts: [try account("http://127.0.0.1:3000")], demo: false) == .read(accountA))
        #expect(LinkNavigation.sharedAccess(to: local, accounts: [try account("http://localhost:3000")], demo: false) == .signIn(offered: false))
    }

    @Test func aNoteOfMyOwnOpensOnlyOnTheServerItIsOn() throws {
        let note = try noteLink()
        let own = try account("https://notes.example.com")
        #expect(LinkNavigation.destination(for: .note(note), accounts: [own]) == .serverNote(ownNote))
        // Anywhere else the app says where the note is; it never does nothing.
        let elsewhere = LinkNavigation.Destination.push(.noteElsewhere(note))
        #expect(LinkNavigation.destination(for: .note(note), accounts: [try account("https://other.example.com")]) == elsewhere)
        #expect(LinkNavigation.destination(for: .note(note), accounts: []) == elsewhere)
        // An account that is remembered without a confirmed session is not a sign-in.
        #expect(LinkNavigation.destination(for: .note(note), accounts: [try account("https://notes.example.com", ready: false)]) == elsewhere)
        #expect(LinkNavigation.destination(for: .note(note), accounts: [try account("https://notes.example.com:8443")]) == elsewhere)

        let local = try noteLink("http://127.0.0.1:3000")
        #expect(LinkNavigation.destination(for: .note(local), accounts: [try account("http://127.0.0.1:3000")]) == .serverNote(ownNote))
        #expect(LinkNavigation.destination(for: .note(local), accounts: [try account("http://127.0.0.1:3111")]) == .push(.noteElsewhere(local)))
    }

    @Test func aLinkGoesOnlyToAccountsOnItsOwnOriginInThePersonsOrder() throws {
        let note = try noteLink(), shared = try sharedLink()
        let elsewhere = try account("https://mine.example.com", id: accountB)
        let own = try account("https://notes.example.com")
        // The account on another server is never a candidate, wherever it is in the order.
        for accounts in [[elsewhere, own], [own, elsewhere]] {
            #expect(LinkNavigation.candidates(for: note.origin, among: accounts).map(\.id) == [accountA])
            #expect(LinkNavigation.destination(for: .note(note), accounts: accounts) == .serverNote(ownNote))
            #expect(LinkNavigation.sharedAccess(to: shared.origin, accounts: accounts, demo: false) == .read(accountA))
        }
        // Two users on the link's server: the first in the person's order.
        let second = AccountID(address: "https://notes.example.com", userID: "00000000-0000-4000-8000-00000000000c")
        let twin = try account("https://notes.example.com", id: second)
        #expect(LinkNavigation.destination(for: .note(note), accounts: [twin, own]) == .serverNote(NoteRef(account: second, id: noteID)))
        #expect(LinkNavigation.candidates(for: note.origin, among: [twin, elsewhere, own]).map(\.id) == [second, accountA])
    }

    @Test func aSignInAnswerOpensNothingByItself() throws {
        let answer = try link("cereal://auth?code=abc&state=xyz&server=https%3A%2F%2Fnotes.example.com")
        #expect(LinkNavigation.destination(for: answer, accounts: [try account("https://notes.example.com")]) == nil)
    }

    @Test func urlsThatAreNotNoteLinksAreLeftToTheirOwnHandlers() {
        // The Live Activity's link and the Debug preview keep their existing handling.
        for text in ["cereal://recording/\(noteID)", "cereal://recording/\(noteID.uppercased())", "cereal://recording/\(noteID)?a=\(accountA.raw)", "cereal://live-activity-preview", "cereal://anything-else"] {
            #expect(URL(string: text).flatMap { AppLink($0) } == nil, "\(text)")
        }
    }

    @Test func aLinkIsPushedOverWhereTheUserIsAndNeverTwice() throws {
        let shared = Route.sharedNote(try sharedLink())
        #expect(LinkNavigation.path([], pushing: shared) == [shared])
        #expect(LinkNavigation.path([.serverNote(ownNote), .serverChat(accountA, noteID: noteID)], pushing: shared) == [.serverNote(ownNote), .serverChat(accountA, noteID: noteID), shared])
        #expect(LinkNavigation.path([.sharedWithMe, shared], pushing: shared) == [.sharedWithMe, shared])
        let other = Route.sharedNote(try sharedLink("other.example.org"))
        #expect(LinkNavigation.path([shared], pushing: other) == [shared, other])
    }

    @Test func removingAnAccountClosesItsScreensAndNobodyElses() throws {
        let shared = Route.sharedNote(try sharedLink())
        let elsewhere = Route.noteElsewhere(try noteLink())
        let local = Route.note(UUID())
        let theirs = NoteRef(account: accountB, id: noteID)
        let path: [Route] = [.serverNote(ownNote), .sharedWithMe, shared, .serverChat(accountA, noteID: nil), .search, elsewhere, local,
                             .chat(noteID: nil), .serverNoteSource(ownNote, raw: true), .serverNote(theirs), .serverChat(accountB, noteID: noteID)]
        // The same note id in another account is another note: it stays.
        #expect(LinkNavigation.path(path, removing: accountA) == [.sharedWithMe, shared, .search, elsewhere, local, .chat(noteID: nil), .serverNote(theirs), .serverChat(accountB, noteID: noteID)])
        #expect(LinkNavigation.path(path, removing: [accountA, accountB]) == [.sharedWithMe, shared, .search, elsewhere, local, .chat(noteID: nil)])
        #expect(LinkNavigation.path(path, removing: []) == path)
        #expect(LinkNavigation.path([], removing: accountA) == [])
        // An account arriving (a sign-in, or the saved session at launch) closes nothing: a note a link has just opened stays open.
        #expect(Route.serverNote(ownNote).account == accountA && Route.search.account == nil && shared.account == nil)
    }

    @Test func signingInToTheServerANoteIsOnOpensThatNote() throws {
        let note = try noteLink()
        let waiting: [Route] = [.sharedWithMe, .noteElsewhere(note)]
        let own = try account("https://notes.example.com"), other = try account("https://other.example.com", id: accountB)
        #expect(LinkNavigation.pathAfterSignIn(waiting, account: own) == [.serverNote(ownNote)])
        // Signing in anywhere else, or with nothing waiting, lands on Home as before.
        #expect(LinkNavigation.pathAfterSignIn(waiting, account: other) == [])
        #expect(LinkNavigation.pathAfterSignIn([.serverNote(ownNote)], account: own) == [])
        #expect(LinkNavigation.pathAfterSignIn([], account: nil) == [])
        #expect(LinkNavigation.pathAfterSignIn(waiting, account: nil) == [])

        // A shared note that asked for the sign-in stays, and now reads with the new account.
        let shared = Route.sharedNote(try sharedLink())
        #expect(LinkNavigation.pathAfterSignIn([.sharedWithMe, shared], account: own) == [.sharedWithMe, shared])
        #expect(LinkNavigation.pathAfterSignIn([.note(UUID()), .search, shared], account: own) == [shared])
        #expect(LinkNavigation.pathAfterSignIn([.sharedWithMe, shared], account: other) == [])
        #expect(LinkNavigation.pathAfterSignIn([shared, .sharedWithMe], account: own) == [])
    }

    @Test func everyLaunchStartsOnTheServerAndForgetsTheDebugDemo() {
        // A new install, and one that an earlier version left in the demo, both open on the server.
        let standard = UserDefaults.standard, saved = standard.object(forKey: "cereal.serverMode")
        defer { standard.set(saved, forKey: "cereal.serverMode") }
        standard.removeObject(forKey: "cereal.serverMode")
        #expect(!ConnectionMode().isDemo)
        // What an earlier version wrote for "Use local demo".
        standard.set(false, forKey: "cereal.serverMode")
        #expect(!ConnectionMode().isDemo)
        // The debug demo is for the run that asked for it; nothing remembers it.
        let qa = ConnectionMode()
        qa.isDemo = true
        qa.previewLabel = "QA preview"
        qa.isDemo = false
        #expect(qa.previewLabel == nil)
        #expect(!ConnectionMode().isDemo)
    }

    @Test func theRouterFollowsLinksInBothModes() throws {
        let name = "link-navigation-" + UUID().uuidString
        let defaults = try #require(UserDefaults(suiteName: name))
        defer { defaults.removePersistentDomain(forName: name) }
        // The debug QA demo, which a `-screen` launch opens. Release builds have no demo.
        let mode = ConnectionMode()
        #expect(!mode.isDemo)
        mode.isDemo = true
        let accounts = AccountsModel(defaults: defaults, credentials: MemoryAccountStore(),
                                     storage: AccountStorage(root: FileManager.default.temporaryDirectory.appendingPathComponent(name)))
        let router = Router()
        router.path = [.search]
        router.showConnection = true

        // Signed out, in the debug demo: a shared note's page (which asks for a sign-in)
        // opens over the current screen and the mode is untouched.
        let shared = try sharedLink()
        router.open(.shared(shared), accounts: accounts, mode: mode)
        #expect(router.path == [.search, .sharedNote(shared)])
        #expect(mode.isDemo)
        #expect(!router.showConnection)

        // A note of someone's account, with nobody signed in, says where it is.
        let note = try noteLink()
        router.open(.note(note), accounts: accounts, mode: mode)
        #expect(router.path == [.search, .sharedNote(shared), .noteElsewhere(note)])
        #expect(mode.isDemo)

        // A sign-in answer that reaches the router changes nothing.
        router.open(try link("cereal://auth?code=abc&state=xyz"), accounts: accounts, mode: mode)
        #expect(router.path.count == 3)

        // "Sign in" on that page opens the connection sheet with the note's server filled in.
        #expect(router.signInAddress == nil)
        router.signIn(to: shared.origin)
        #expect(router.showConnection)
        #expect(router.signInAddress == "https://notes.example.com")
        #expect(try InstanceURL(try #require(router.signInAddress)).description == "https://notes.example.com")
    }

    @Test func signingInIsOfferedOnlyWhereItReplacesNoAccountInUse() {
        #expect(LinkNavigation.offersSignIn(signedIn: false, demo: false))
        #expect(LinkNavigation.offersSignIn(signedIn: false, demo: true))
        #expect(LinkNavigation.offersSignIn(signedIn: true, demo: true))
        #expect(!LinkNavigation.offersSignIn(signedIn: true, demo: false))
    }

    @Test func theSignInFormOffersWhatTheServerOffers() {
        #expect(SignInMethods(password: true, google: false).browserLabel == "Continue in browser")
        #expect(SignInMethods(password: true, google: true).browserLabel == "Continue in browser")
        #expect(SignInMethods(password: false, google: true).browserLabel == "Continue with Google")
        // An older server: the password form alone, as before.
        #expect(SignInMethods.passwordOnly.password)
        #expect(SignInMethods.passwordOnly.browserLabel == nil)
    }
}

/// Answers for the sign-in form's question, each released when the test says so.
private actor MethodAnswers {
    private var waiting: [String: CheckedContinuation<Result<SignInMethods, APIClientError>, Never>] = [:]
    private(set) var asked: [String] = []

    func answer(for instance: InstanceURL) async throws -> SignInMethods {
        asked.append(instance.description)
        return try await withCheckedContinuation { waiting[instance.description] = $0 }.get()
    }

    func release(_ address: String, _ result: Result<SignInMethods, APIClientError>) {
        waiting.removeValue(forKey: address)?.resume(returning: result)
    }
}

@MainActor struct SignInDiscoveryTests {
    private func waitUntil(_ condition: () async -> Bool) async {
        let deadline = ContinuousClock.now + .seconds(5)
        while ContinuousClock.now < deadline {
            if await condition() { return }
            try? await Task.sleep(for: .milliseconds(10))
        }
    }

    @Test func onlyTheAnswerForTheAddressInTheFieldIsShown() async {
        let answers = MethodAnswers()
        let model = SignInDiscoveryModel(pause: .milliseconds(20)) { try await answers.answer(for: $0) }
        // Typing pauses on an address that is not finished; the server there is asked.
        model.check("http://127.0.0.1:339")
        #expect(model.state == .idle)
        await waitUntil { await answers.asked == ["http://127.0.0.1:339"] }
        #expect(model.state == .checking)
        // The last digit is typed while that question is still out.
        model.check("http://127.0.0.1:3392")
        #expect(model.state == .idle)
        await waitUntil { await answers.asked.count == 2 }
        // The old question fails late. It must not replace the new one's state.
        await answers.release("http://127.0.0.1:339", .failure(.transport(.cannotConnectToHost)))
        try? await Task.sleep(for: .milliseconds(50))
        #expect(model.state == .checking)
        await answers.release("http://127.0.0.1:3392", .success(SignInMethods(password: false, google: true)))
        await waitUntil { model.state == .ready(SignInMethods(password: false, google: true)) }
        #expect(model.state == .ready(SignInMethods(password: false, google: true)))
        #expect(await answers.asked == ["http://127.0.0.1:339", "http://127.0.0.1:3392"])
    }

    @Test func typingWithoutAPauseAsksOnlyAboutTheFinishedAddress() async {
        let answers = MethodAnswers()
        let model = SignInDiscoveryModel(pause: .milliseconds(60)) { try await answers.answer(for: $0) }
        for text in ["h", "https://", "https://notes", "https://notes.example.com"] { model.check(text) }
        await waitUntil { await !answers.asked.isEmpty }
        await answers.release("https://notes.example.com", .success(.passwordOnly))
        await waitUntil { model.state == .ready(.passwordOnly) }
        #expect(await answers.asked == ["https://notes.example.com"])
        // The same address again, or with a stray space, is not a new question.
        model.check(" https://notes.example.com ")
        #expect(model.state == .ready(.passwordOnly))
    }

    @Test func aFailureIsShownAndCanBeTriedAgain() async {
        let answers = MethodAnswers()
        let model = SignInDiscoveryModel(pause: .milliseconds(10)) { try await answers.answer(for: $0) }
        model.check("https://notes.example.com")
        await waitUntil { await answers.asked.count == 1 }
        await answers.release("https://notes.example.com", .failure(.transport(.cannotFindHost)))
        await waitUntil { model.state != .checking }
        #expect(model.state == .failed(APIClientError.transport(.cannotFindHost).localizedDescription))
        model.check("https://notes.example.com", again: true)
        await waitUntil { await answers.asked.count == 2 }
        await answers.release("https://notes.example.com", .success(SignInMethods(password: true, google: true)))
        await waitUntil { model.state == .ready(SignInMethods(password: true, google: true)) }
        #expect(model.state == .ready(SignInMethods(password: true, google: true)))
    }

    @Test func anAddressThatCannotBeAServerSaysWhyWithoutAsking() async {
        let answers = MethodAnswers()
        let model = SignInDiscoveryModel(pause: .milliseconds(10)) { try await answers.answer(for: $0) }
        model.check("http://notes.example.com")
        await waitUntil { model.state != .idle }
        #expect(model.state == .failed(APIClientError.insecureInstanceURL.localizedDescription))
        model.check("notes.example.com")
        await waitUntil { model.state != .idle }
        #expect(model.state == .failed(APIClientError.invalidInstanceURL.localizedDescription))
        #expect(await answers.asked.isEmpty)
        // Clearing the field clears the message.
        model.check("")
        #expect(model.state == .idle)
    }
}

@MainActor struct CaptureTitleTests {
    @Test func theRecordingScreenSendsATitleOnlyWhenThePersonTypedOne() {
        // Untouched, and empty: the body is saved and the title is left to the server.
        let untouched = ServerCaptureView.edit(title: "", serverTitle: "", text: "notes so far")
        #expect(untouched.title == nil && untouched.body == "notes so far")
        // Untouched after the server named the note: still not sent back.
        #expect(ServerCaptureView.edit(title: "Launch review", serverTitle: "Launch review", text: "x").title == nil)
        // Typed: theirs is sent, including clearing a title on purpose.
        #expect(ServerCaptureView.edit(title: "Mine", serverTitle: "", text: "x").title == "Mine")
        #expect(ServerCaptureView.edit(title: "", serverTitle: "Launch review", text: "x").title == "")
    }

    @Test func searchResultsShowTheNewestTitle() {
        let row = ServerNoteRow(id: noteID, title: "", date: "2026-11-12", createdAt: "2026-11-12T10:00:00.000Z", updatedAt: "2026-11-12T10:01:00.000Z")
        let hit = ServerNoteSearchHit(row: row, excerpt: nil)
        #expect(NoteSearch.server([hit], account: accountA).map(\.title) == ["Untitled note"])
        #expect(NoteSearch.server([hit], account: accountA, title: { _ in "Launch readiness review" }).map(\.title) == ["Launch readiness review"])
        #expect(NoteSearch.server([hit], account: accountA, title: { _ in "" }).map(\.title) == ["Untitled note"])
        // A result opens in the account it was found in.
        #expect(NoteSearch.server([hit], account: accountA).map(\.route) == [.serverNote(ownNote)])
    }
}
