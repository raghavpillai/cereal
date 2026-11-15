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

@MainActor struct LinkNavigationTests {
    @Test func aSharedNoteOpensInTheAccountOnlyOnItsOwnServer() throws {
        let shared = try sharedLink()
        // Someone else on my server shared it: it is read with my account there.
        #expect(LinkNavigation.destination(for: .shared(shared), instance: try InstanceURL("https://notes.example.com"), signedIn: true) == .sharedOnServer(shared))
        #expect(LinkNavigation.destination(for: .shared(shared), instance: try InstanceURL("https://NOTES.example.com:443/"), signedIn: true) == .sharedOnServer(shared))
        // Signed out, signed in elsewhere, or with only an address remembered: the page that says where the note is.
        let prompt = LinkNavigation.Destination.push(.sharedNote(shared))
        #expect(LinkNavigation.destination(for: .shared(shared), instance: nil, signedIn: false) == prompt)
        #expect(LinkNavigation.destination(for: .shared(shared), instance: try InstanceURL("https://mine.example.com"), signedIn: true) == prompt)
        #expect(LinkNavigation.destination(for: .shared(shared), instance: try InstanceURL("https://notes.example.com"), signedIn: false) == prompt)
        #expect(LinkNavigation.destination(for: .shared(shared), instance: try InstanceURL("https://notes.example.com:8443"), signedIn: true) == prompt)
        // The app form and a universal link are the same link.
        #expect(try link("cereal://shared?url=https%3A%2F%2Fnotes.example.com%2Fs%2F\(token)") == .shared(shared))
    }

    @Test func aSharedNoteIsReadOnlyWithASessionOnItsOwnServer() throws {
        let origin = try sharedLink().origin
        let own = try InstanceURL("https://notes.example.com")
        #expect(LinkNavigation.sharedAccess(to: origin, instance: own, signedIn: true, demo: false) == .read)
        // Another server: nothing is fetched, and signing in is not offered over the account in use.
        #expect(LinkNavigation.sharedAccess(to: origin, instance: try InstanceURL("https://mine.example.com"), signedIn: true, demo: false) == .signIn(offered: false))
        #expect(LinkNavigation.sharedAccess(to: origin, instance: try InstanceURL("https://notes.example.com:8443"), signedIn: true, demo: false) == .signIn(offered: false))
        // Signed out, or in the local demo: nothing is fetched, and signing in is offered.
        #expect(LinkNavigation.sharedAccess(to: origin, instance: nil, signedIn: false, demo: false) == .signIn(offered: true))
        #expect(LinkNavigation.sharedAccess(to: origin, instance: nil, signedIn: false, demo: true) == .signIn(offered: true))
        #expect(LinkNavigation.sharedAccess(to: origin, instance: own, signedIn: false, demo: false) == .signIn(offered: true))
        #expect(LinkNavigation.sharedAccess(to: origin, instance: own, signedIn: true, demo: true) == .signIn(offered: true))
        #expect(LinkNavigation.sharedAccess(to: origin, instance: try InstanceURL("https://mine.example.com"), signedIn: true, demo: true) == .signIn(offered: true))
        // `localhost` and `127.0.0.1` are two servers.
        let local = try sharedLink("127.0.0.1:3000", scheme: "http").origin
        #expect(LinkNavigation.sharedAccess(to: local, instance: try InstanceURL("http://127.0.0.1:3000"), signedIn: true, demo: false) == .read)
        #expect(LinkNavigation.sharedAccess(to: local, instance: try InstanceURL("http://localhost:3000"), signedIn: true, demo: false) == .signIn(offered: false))
    }

    @Test func aNoteOfMyOwnOpensOnlyOnTheServerItIsOn() throws {
        let note = try noteLink()
        let own = try InstanceURL("https://notes.example.com")
        #expect(LinkNavigation.destination(for: .note(note), instance: own, signedIn: true) == .serverNote(noteID))
        // Anywhere else the app says where the note is; it never does nothing.
        let elsewhere = LinkNavigation.Destination.push(.noteElsewhere(note))
        #expect(LinkNavigation.destination(for: .note(note), instance: try InstanceURL("https://other.example.com"), signedIn: true) == elsewhere)
        #expect(LinkNavigation.destination(for: .note(note), instance: nil, signedIn: false) == elsewhere)
        // A server address that is remembered without a session is not a sign-in.
        #expect(LinkNavigation.destination(for: .note(note), instance: own, signedIn: false) == elsewhere)
        #expect(LinkNavigation.destination(for: .note(note), instance: try InstanceURL("https://notes.example.com:8443"), signedIn: true) == elsewhere)

        let local = try noteLink("http://127.0.0.1:3000")
        #expect(LinkNavigation.destination(for: .note(local), instance: try InstanceURL("http://127.0.0.1:3000"), signedIn: true) == .serverNote(noteID))
        #expect(LinkNavigation.destination(for: .note(local), instance: try InstanceURL("http://127.0.0.1:3111"), signedIn: true) == .push(.noteElsewhere(local)))
    }

    @Test func aSignInAnswerOpensNothingByItself() throws {
        let answer = try link("cereal://auth?code=abc&state=xyz&server=https%3A%2F%2Fnotes.example.com")
        #expect(LinkNavigation.destination(for: answer, instance: try InstanceURL("https://notes.example.com"), signedIn: true) == nil)
    }

    @Test func urlsThatAreNotNoteLinksAreLeftToTheirOwnHandlers() {
        // The Live Activity's link and the Debug preview keep their existing handling.
        for text in ["cereal://recording/\(noteID)", "cereal://recording/\(noteID.uppercased())", "cereal://live-activity-preview", "cereal://anything-else"] {
            #expect(URL(string: text).flatMap { AppLink($0) } == nil, "\(text)")
        }
    }

    @Test func aLinkIsPushedOverWhereTheUserIsAndNeverTwice() throws {
        let shared = Route.sharedNote(try sharedLink())
        #expect(LinkNavigation.path([], pushing: shared) == [shared])
        #expect(LinkNavigation.path([.serverNote(noteID), .serverChat(noteID)], pushing: shared) == [.serverNote(noteID), .serverChat(noteID), shared])
        #expect(LinkNavigation.path([.sharedWithMe, shared], pushing: shared) == [.sharedWithMe, shared])
        let other = Route.sharedNote(try sharedLink("other.example.org"))
        #expect(LinkNavigation.path([shared], pushing: other) == [shared, other])
    }

    @Test func anAccountChangeClosesThatAccountsScreensAndKeepsSharedOnes() throws {
        let shared = Route.sharedNote(try sharedLink())
        let elsewhere = Route.noteElsewhere(try noteLink())
        let path: [Route] = [.serverNote(noteID), .sharedWithMe, shared, .serverChat(nil), .search, elsewhere, .note(UUID()), .chat(noteID: nil), .serverNoteSource(noteID, raw: true)]
        #expect(LinkNavigation.pathAfterAccountChange(path, linkOpenedIn: nil, account: "https://a|1") == [.sharedWithMe, shared, elsewhere])
        #expect(LinkNavigation.pathAfterAccountChange(path, linkOpenedIn: nil, account: nil) == [.sharedWithMe, shared, elsewhere])
        #expect(LinkNavigation.pathAfterAccountChange([], linkOpenedIn: nil, account: nil) == [])
    }

    @Test func aNoteALinkJustOpenedIsNotClosedByItsOwnAccountArriving() {
        let path: [Route] = [.serverNote(noteID)]
        // A link at launch: the note is opened as the restored account is announced.
        #expect(LinkNavigation.pathAfterAccountChange(path, linkOpenedIn: .some("https://a|1"), account: "https://a|1") == path)
        // A later change to another account, or a sign-out, closes it as before.
        #expect(LinkNavigation.pathAfterAccountChange(path, linkOpenedIn: .some("https://a|1"), account: "https://b|2") == [])
        #expect(LinkNavigation.pathAfterAccountChange(path, linkOpenedIn: .some("https://a|1"), account: nil) == [])
        #expect(LinkNavigation.pathAfterAccountChange(path, linkOpenedIn: nil, account: "https://a|1") == [])
    }

    @Test func signingInToTheServerANoteIsOnOpensThatNote() throws {
        let note = try noteLink()
        let waiting: [Route] = [.sharedWithMe, .noteElsewhere(note)]
        #expect(LinkNavigation.pathAfterSignIn(waiting, instance: try InstanceURL("https://notes.example.com")) == [.serverNote(noteID)])
        // Signing in anywhere else, or with nothing waiting, lands on Home as before.
        #expect(LinkNavigation.pathAfterSignIn(waiting, instance: try InstanceURL("https://other.example.com")) == [])
        #expect(LinkNavigation.pathAfterSignIn([.serverNote(noteID)], instance: try InstanceURL("https://notes.example.com")) == [])
        #expect(LinkNavigation.pathAfterSignIn([], instance: nil) == [])
        #expect(LinkNavigation.pathAfterSignIn(waiting, instance: nil) == [])

        // A shared note that asked for the sign-in stays, and now reads with the new account.
        let shared = Route.sharedNote(try sharedLink())
        #expect(LinkNavigation.pathAfterSignIn([.sharedWithMe, shared], instance: try InstanceURL("https://notes.example.com")) == [.sharedWithMe, shared])
        #expect(LinkNavigation.pathAfterSignIn([.note(UUID()), .search, shared], instance: try InstanceURL("https://notes.example.com")) == [shared])
        #expect(LinkNavigation.pathAfterSignIn([.sharedWithMe, shared], instance: try InstanceURL("https://other.example.com")) == [])
        #expect(LinkNavigation.pathAfterSignIn([shared, .sharedWithMe], instance: try InstanceURL("https://notes.example.com")) == [])
    }

    @Test func theRouterFollowsLinksInBothModes() throws {
        let name = "link-navigation-" + UUID().uuidString
        let defaults = try #require(UserDefaults(suiteName: name))
        defer { defaults.removePersistentDomain(forName: name) }
        let mode = ConnectionMode(defaults: defaults)
        #expect(mode.isDemo)
        let server = ServerNotesModel(settings: ConnectionSettingsStore(defaults: defaults, tokens: NoTokens()))
        let router = Router()
        router.path = [.search]
        router.showConnection = true

        // Signed out, in the local demo: a shared note's page (which asks for a sign-in)
        // opens over the current screen and the mode is untouched.
        let shared = try sharedLink()
        router.open(.shared(shared), server: server, mode: mode)
        #expect(router.path == [.search, .sharedNote(shared)])
        #expect(mode.isDemo)
        #expect(!router.showConnection)

        // A note of someone's account, with nobody signed in, says where it is.
        let note = try noteLink()
        router.open(.note(note), server: server, mode: mode)
        #expect(router.path == [.search, .sharedNote(shared), .noteElsewhere(note)])
        #expect(mode.isDemo)
        #expect(router.linkOpenedIn == nil)

        // A sign-in answer that reaches the router changes nothing.
        router.open(try link("cereal://auth?code=abc&state=xyz"), server: server, mode: mode)
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
        #expect(NoteSearch.server([hit]).map(\.title) == ["Untitled note"])
        #expect(NoteSearch.server([hit], title: { _ in "Launch readiness review" }).map(\.title) == ["Launch readiness review"])
        #expect(NoteSearch.server([hit], title: { _ in "" }).map(\.title) == ["Untitled note"])
    }
}

private struct NoTokens: BearerTokenStore {
    func load() throws -> String? { nil }
    func save(_ token: String) throws {}
    func clear() throws {}
}
