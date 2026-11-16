import Foundation
import Testing
import Contracts
@testable import APIClient

private let token = "Zk3vX9aQ2mN7pL5rT8wY1bC4dF6gH0jK-sU_eIoAxyz"
private let sessionJSON = #"{"user":{"id":"00000000-0000-4000-8000-000000000001","name":"Test Owner","email":"owner@example.test","role":"owner","createdAt":"2026-11-12T00:00:00Z"},"expiresAt":null}"#

private struct Reply: Sendable {
    var status = 200
    var body = Data()
    var failure: URLError.Code?
    var headers: [String: String] = ["Content-Type": "application/json"]

    static func json(_ text: String, status: Int = 200) -> Reply { Reply(status: status, body: Data(text.utf8)) }
}

/// Requests seen and answers given, per test, keyed by a header the test's session adds.
private final class Wire: @unchecked Sendable {
    static let shared = Wire()
    private let lock = NSLock()
    private var handlers: [String: @Sendable (URLRequest) -> Reply] = [:]
    private var seen: [String: [URLRequest]] = [:]

    func register(_ handler: @escaping @Sendable (URLRequest) -> Reply) -> String {
        lock.lock(); defer { lock.unlock() }
        let id = UUID().uuidString
        handlers[id] = handler
        return id
    }

    func reply(_ request: URLRequest) -> Reply {
        let id = request.value(forHTTPHeaderField: "X-Test-Case") ?? ""
        lock.lock()
        seen[id, default: []].append(request)
        let handler = handlers[id]
        lock.unlock()
        return handler?(request) ?? Reply(status: 500)
    }

    func requests(_ id: String) -> [URLRequest] {
        lock.lock(); defer { lock.unlock() }
        return seen[id] ?? []
    }
}

private final class WireProtocol: URLProtocol, @unchecked Sendable {
    private let lock = NSLock()
    private var stopped = false
    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func startLoading() {
        let reply = Wire.shared.reply(request)
        lock.lock(); defer { lock.unlock() }
        guard !stopped else { return }
        if let failure = reply.failure { client?.urlProtocol(self, didFailWithError: URLError(failure)); return }
        let http = HTTPURLResponse(url: request.url!, statusCode: reply.status, httpVersion: "HTTP/1.1", headerFields: reply.headers)!
        client?.urlProtocol(self, didReceive: http, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: reply.body)
        client?.urlProtocolDidFinishLoading(self)
    }
    override func stopLoading() { lock.lock(); defer { lock.unlock() }; stopped = true }
}

private func stubbed(_ handler: @escaping @Sendable (URLRequest) -> Reply) -> (URLSessionConfiguration, String) {
    let configuration = URLSessionConfiguration.ephemeral
    configuration.protocolClasses = [WireProtocol.self]
    let id = Wire.shared.register(handler)
    configuration.httpAdditionalHeaders = ["X-Test-Case": id]
    return (configuration, id)
}

private func body(_ request: URLRequest) -> Data {
    if let data = request.httpBody { return data }
    guard let stream = request.httpBodyStream else { return Data() }
    stream.open(); defer { stream.close() }
    var result = Data()
    var buffer = [UInt8](repeating: 0, count: 1024)
    while true {
        let count = stream.read(&buffer, maxLength: buffer.count)
        if count <= 0 { break }
        result.append(contentsOf: buffer.prefix(count))
    }
    return result
}

private func sharedLink(_ text: String = "https://shared.example.org/s/\(token)") throws -> SharedNoteLink {
    guard case .shared(let link)? = AppLink(text: text) else { throw APIClientError.invalidIdentifier }
    return link
}

private func snapshotJSON(title: String = "Launch plan", enhanced: String = "## Summary\\n- Ship it", body: String = "my own notes") -> String {
    #"{"shareId":"7b0c6a2e-8f0d-4b7e-9a57-2f3f6f2f6c11","title":"\#(title)","body":"\#(body)","enhanced":"\#(enhanced)","date":"2026-11-12","duration":"41:07","sharedAt":"2026-11-13T09:00:00.000Z","expiresAt":null}"#
}

/// A signed-in client for `instance`, answering from `handler`. The session token is the same in every test.
private func signedIn(to instance: String = "https://mine.example.com", _ handler: @escaping @Sendable (URLRequest) -> Reply) throws -> (MeetingNotesAPI, String) {
    let (configuration, id) = stubbed(handler)
    let api = try MeetingNotesAPI(credentials: InstanceCredentials(instance: InstanceURL(instance), bearerToken: "my-session-token"), configuration: configuration)
    return (api, id)
}

private func carriesSession(_ request: URLRequest) -> Bool {
    (request.allHTTPHeaderFields ?? [:]).contains { $0.key.lowercased() == "authorization" || $0.key.lowercased() == "cookie" || $0.value.contains("my-session-token") }
}

@Suite struct SharedNoteClientTests {
    @Test func aNoteSharedOnMyOwnServerIsReadWithMySession() async throws {
        let (api, id) = try signedIn { _ in .json(snapshotJSON()) }
        let note = try await api.sharedNote(try sharedLink("https://mine.example.com/s/\(token)"))
        #expect(note.title == "Launch plan")
        #expect(note.enhanced == "## Summary\n- Ship it")
        #expect(note.body == "my own notes")
        #expect(note.date == DateComponents(year: 2026, month: 11, day: 12))
        #expect(note.duration == "41:07")

        let requests = Wire.shared.requests(id)
        #expect(requests.count == 1)
        let request = try #require(requests.first)
        #expect(request.httpMethod == "GET")
        #expect(request.url?.absoluteString == "https://mine.example.com/api/v1/shared/\(token)")
        #expect(request.value(forHTTPHeaderField: "Authorization") == "Bearer my-session-token")
        #expect(request.value(forHTTPHeaderField: "Accept") == "application/json")
        // The bearer is the whole of the session: no cookie rides along.
        #expect(request.value(forHTTPHeaderField: "Cookie") == nil)
    }

    @Test func theSessionIsNeverSentToAnotherServer() async throws {
        // A cookie for the other server in the shared store, as a browser session there would leave.
        let cookie = try #require(HTTPCookie(properties: [.name: "better-auth.session_token", .value: "my-session-token",
                                                          .domain: "shared.example.org", .path: "/", .secure: true]))
        HTTPCookieStorage.shared.setCookie(cookie)
        defer { HTTPCookieStorage.shared.deleteCookie(cookie) }
        // The stub would answer any host. Nothing may be asked of it at all.
        let (api, id) = try signedIn { _ in .json(snapshotJSON()) }
        for foreign in ["https://shared.example.org/s/\(token)", "https://mine.example.com.evil.example/s/\(token)",
                        "https://evil.example/s/\(token)?host=mine.example.com", "https://sub.mine.example.com/s/\(token)",
                        "https://mine.example.co/s/\(token)", "https://mine.example.com:8443/s/\(token)"] {
            await #expect(throws: SharedNoteError.signInRequired, "\(foreign)") { try await api.sharedNote(try sharedLink(foreign)) }
        }
        // The app form of a link names its server just the same.
        let wrapped = "cereal://shared?url=" + ("https://shared.example.org/s/\(token)".addingPercentEncoding(withAllowedCharacters: .alphanumerics) ?? "")
        await #expect(throws: SharedNoteError.signInRequired) { try await api.sharedNote(try sharedLink(wrapped)) }
        #expect(Wire.shared.requests(id).isEmpty)
        #expect(!SharedNoteError.signInRequired.isRetryable)

        // A request that is made goes to the signed-in server and nowhere else, whatever the answer says.
        let (redirected, redirectID) = try signedIn { _ in Reply(status: 302, headers: ["Location": "https://evil.example/api/v1/shared/\(token)"]) }
        await #expect(throws: SharedNoteError.invalidResponse) { try await redirected.sharedNote(try sharedLink("https://mine.example.com/s/\(token)")) }
        #expect(Wire.shared.requests(redirectID).count == 1)
        #expect(Wire.shared.requests(redirectID).allSatisfy { $0.url?.host == "mine.example.com" })
    }

    @Test func theSameServerIsTheSameSchemeHostAndPortAfterNormalising() async throws {
        // Written differently, the same origin: the session is sent.
        for (instance, link) in [("https://mine.example.com", "https://mine.example.com:443/s/\(token)"),
                                 ("https://mine.example.com:443", "https://mine.example.com/s/\(token)"),
                                 ("https://mine.example.com/", "https://mine.example.com/s/\(token)"),
                                 ("https://MINE.Example.COM", "https://mine.example.com/s/\(token)"),
                                 ("https://mine.example.com", "HTTPS://Mine.Example.Com/s/\(token)"),
                                 ("https://mine.example.com", "https://mine.example.com/s/\(token)?utm_source=mail#top"),
                                 ("http://127.0.0.1:3000", "http://127.0.0.1:3000/s/\(token)"),
                                 ("http://localhost:80", "http://localhost/s/\(token)"),
                                 ("http://[::1]:3000", "http://[::1]:3000/s/\(token)")] {
            let (api, id) = try signedIn(to: instance) { _ in .json(snapshotJSON()) }
            _ = try await api.sharedNote(try sharedLink(link))
            let request = try #require(Wire.shared.requests(id).first, "\(instance) and \(link)")
            #expect(request.value(forHTTPHeaderField: "Authorization") == "Bearer my-session-token", "\(instance) and \(link)")
            // The request is built on the signed-in server, not on the link's spelling of it.
            #expect(request.url?.absoluteString == (try InstanceURL(instance)).description + "/api/v1/shared/\(token)")
        }
        // Another origin, however alike: nothing is sent.
        for (instance, link) in [("http://localhost:3000", "http://127.0.0.1:3000/s/\(token)"),
                                 ("http://127.0.0.1:3000", "http://localhost:3000/s/\(token)"),
                                 ("http://127.0.0.1:3000", "http://[::1]:3000/s/\(token)"),
                                 ("http://127.0.0.1:3000", "http://127.0.0.1:3001/s/\(token)"),
                                 ("http://127.0.0.1:3000", "http://127.0.0.1/s/\(token)"),
                                 ("https://localhost:3000", "http://localhost:3000/s/\(token)"),
                                 ("https://mine.example.com", "https://mine.example.com:8443/s/\(token)"),
                                 ("https://mine.example.com:8443", "https://mine.example.com/s/\(token)"),
                                 ("https://mine.example.com/base", "https://mine.example.com/s/\(token)"),
                                 ("https://example.com", "https://mine.example.com/s/\(token)")] {
            let (api, id) = try signedIn(to: instance) { _ in .json(snapshotJSON()) }
            await #expect(throws: SharedNoteError.signInRequired, "\(instance) and \(link)") { try await api.sharedNote(try sharedLink(link)) }
            #expect(Wire.shared.requests(id).isEmpty, "\(instance) and \(link)")
        }
    }

    @Test func aSessionTheServerNoLongerAcceptsAsksForSignInAgain() async throws {
        for status in [401, 403] {
            let (api, id) = try signedIn { _ in .json(#"{"error":"unauthorized"}"#, status: status) }
            await #expect(throws: SharedNoteError.signInRequired) { try await api.sharedNote(try sharedLink("https://mine.example.com/s/\(token)")) }
            #expect(Wire.shared.requests(id).count == 1)
        }
    }

    @Test func aMissingRevokedOrExpiredShareIsUnavailable() async throws {
        for status in [404, 410] {
            let (api, _) = try signedIn { _ in .json(#"{"error":"not_found","message":"Shared note not found"}"#, status: status) }
            await #expect(throws: SharedNoteError.unavailable) { try await api.sharedNote(try sharedLink("https://mine.example.com/s/\(token)")) }
        }
        #expect(!SharedNoteError.unavailable.isRetryable)
    }

    @Test func aSnapshotOverTwoMegabytesIsRefusedByItsDeclaredLengthAndAsItArrives() async throws {
        let link = try sharedLink("https://mine.example.com/s/\(token)")
        let large = Data(repeating: 0x20, count: SharedNoteReader.maximumBytes + 1)
        let (declared, _) = try signedIn { _ in Reply(body: large, headers: ["Content-Type": "application/json", "Content-Length": String(large.count)]) }
        await #expect(throws: SharedNoteError.tooLarge) { try await declared.sharedNote(link) }
        // A server that does not say how much it will send.
        let (undeclared, _) = try signedIn { _ in Reply(body: large) }
        await #expect(throws: SharedNoteError.tooLarge) { try await undeclared.sharedNote(link) }
        #expect(!SharedNoteError.tooLarge.isRetryable)

        // Exactly at the limit is read.
        let prefix = #"{"title":"Big","enhanced":""#, suffix = #"","body":""}"#
        let filler = String(repeating: "a", count: SharedNoteReader.maximumBytes - prefix.utf8.count - suffix.utf8.count)
        let (atLimit, _) = try signedIn { _ in .json(prefix + filler + suffix) }
        #expect(try await atLimit.sharedNote(link).enhanced.utf8.count == filler.utf8.count)
    }

    @Test func networkFailuresAndServerErrorsAreToldApart() async throws {
        let link = try sharedLink("https://mine.example.com/s/\(token)")
        let (offline, _) = try signedIn { _ in Reply(failure: .notConnectedToInternet) }
        await #expect(throws: SharedNoteError.network(.notConnectedToInternet)) { try await offline.sharedNote(link) }
        #expect(SharedNoteError.network(.timedOut).isRetryable)
        let (broken, _) = try signedIn { _ in .json("{}", status: 502) }
        await #expect(throws: SharedNoteError.server(502)) { try await broken.sharedNote(link) }
        let (page, _) = try signedIn { _ in Reply(body: Data("<!DOCTYPE html><html></html>".utf8), headers: ["Content-Type": "text/html"]) }
        await #expect(throws: SharedNoteError.invalidResponse) { try await page.sharedNote(link) }
    }

    @Test func textFromTheServerIsKeptAsTextAndOddFieldsAreDropped() async throws {
        let link = try sharedLink("https://mine.example.com/s/\(token)")
        let (api, _) = try signedIn { _ in
            .json(#"{"title":"<script>alert(1)</script>","enhanced":"<img src=x onerror=alert(1)> [link](javascript:alert(1))","date":"12/11/2026","duration":"0:00","extra":{"nested":true}}"#)
        }
        let note = try await api.sharedNote(link)
        #expect(note.title == "<script>alert(1)</script>")
        #expect(note.enhanced == "<img src=x onerror=alert(1)> [link](javascript:alert(1))")
        #expect(note.body.isEmpty)
        #expect(note.date == nil)
        #expect(note.duration == nil)

        let long = String(repeating: "t", count: 900)
        let (titled, _) = try signedIn { _ in .json(#"{"title":"\#(long)"}"#) }
        #expect(try await titled.sharedNote(link).title.count == 500)
    }
}

@Suite @MainActor struct SharedNoteModelTests {
    private func model(_ header: String, tokens: MemoryTokens = MemoryTokens()) throws -> ServerNotesModel {
        let name = "shared-note-model-" + UUID().uuidString
        let defaults = try #require(UserDefaults(suiteName: name))
        defaults.removePersistentDomain(forName: name)
        return ServerNotesModel { credentials in
            let configuration = URLSessionConfiguration.ephemeral
            configuration.protocolClasses = [WireProtocol.self]
            configuration.httpAdditionalHeaders = ["X-Test-Case": header]
            return MeetingNotesAPI(credentials: credentials, configuration: configuration)
        }
    }

    private func answers() -> String {
        Wire.shared.register { request in
            let path = request.url?.path ?? ""
            if path.hasSuffix("/api/v1/session") { return .json(sessionJSON) }
            if path.hasSuffix("/notes/summaries") { return .json(#"{"notes":[],"nextCursor":null,"total":0}"#) }
            if path.contains("/api/v1/shared/") { return .json(snapshotJSON()) }
            return Reply(status: 404)
        }
    }

    @Test func signedOutNothingIsAskedOfAnyServer() async throws {
        let id = answers()
        let model = try model(id)
        #expect(!model.isSignedIn(to: try sharedLink("https://mine.example.com/s/\(token)").origin))
        await #expect(throws: SharedNoteError.signInRequired) { try await model.sharedNote(try sharedLink("https://mine.example.com/s/\(token)")) }
        #expect(Wire.shared.requests(id).isEmpty)
    }

    @Test func signedInOnlyTheSignedInServerIsAsked() async throws {
        let id = answers()
        let model = try model(id)
        await model.connect(instance: "https://mine.example.com", token: "my-session-token")
        guard case .loaded = model.state else { Issue.record("not signed in: \(model.state)"); return }
        let before = Wire.shared.requests(id).count

        let mine = try sharedLink("https://mine.example.com/s/\(token)")
        #expect(model.isSignedIn(to: mine.origin))
        #expect(try await model.sharedNote(mine).title == "Launch plan")
        let asked = Wire.shared.requests(id).dropFirst(before)
        #expect(asked.count == 1)
        #expect(asked.first?.url?.absoluteString == "https://mine.example.com/api/v1/shared/\(token)")
        #expect(asked.first?.value(forHTTPHeaderField: "Authorization") == "Bearer my-session-token")

        let other = try sharedLink("https://shared.example.org/s/\(token)")
        #expect(!model.isSignedIn(to: other.origin))
        await #expect(throws: SharedNoteError.signInRequired) { try await model.sharedNote(other) }
        #expect(Wire.shared.requests(id).count == before + 1)
        #expect(Wire.shared.requests(id).allSatisfy { $0.url?.host == "mine.example.com" })
        #expect(!Wire.shared.requests(id).contains { $0.url?.host != "mine.example.com" && carriesSession($0) })
    }
}

@Suite struct SharedNoteDisplayTests {
    @Test func anOrdinaryNoteIsShownWhole() {
        let text = "## Summary\n- one\n- two\n\nA paragraph."
        #expect(SharedNote.shown(text).text == text)
        #expect(SharedNote.shown(text).isWhole)
        #expect(SharedNote.shown("").isWhole)
    }

    @Test func aNoteOfVeryManyLinesOrCharactersIsShownFromItsBeginning() {
        let lines = Array(repeating: "x", count: 10_000).joined(separator: "\n")
        let byLines = SharedNote.shown(lines)
        #expect(!byLines.isWhole)
        #expect(byLines.text.split(separator: "\n", omittingEmptySubsequences: false).count == 800)

        let long = String(repeating: "word ", count: 50_000)
        let byCharacters = SharedNote.shown(long)
        #expect(!byCharacters.isWhole)
        #expect(byCharacters.text.count == 60_000)
        #expect(long.hasPrefix(byCharacters.text))

        // Exactly at either limit nothing is left out.
        #expect(SharedNote.shown(String(repeating: "a", count: 60_000)).isWhole)
        #expect(SharedNote.shown(Array(repeating: "x", count: 800).joined(separator: "\n")).isWhole)
        #expect(SharedNote.shown("a\nb\nc\nd", characters: 100, lines: 2) == ("a\nb", false))
    }
}

@Suite struct BrowserSignInTests {
    private func attempt(_ instance: String = "https://notes.example.com", verifier: String = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk", state: String = "c3RhdGUtc3RhdGUtc3RhdGU") throws -> BrowserSignIn {
        BrowserSignIn(instance: try InstanceURL(instance), verifier: verifier, state: state)
    }

    @Test func theChallengeIsTheS256TransformOfRFC7636() throws {
        // RFC 7636, appendix B.
        #expect(BrowserSignIn.challenge(for: "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk") == "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM")
        #expect(try attempt().challenge == "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM")
    }

    @Test func eachAttemptHasItsOwnVerifierAndStateOfTheRightSize() throws {
        let unreserved = CharacterSet(charactersIn: "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_")
        var verifiers = Set<String>(), states = Set<String>()
        for _ in 0..<50 {
            let attempt = try BrowserSignIn(instance: InstanceURL("https://notes.example.com"))
            // 32 and 16 random bytes as unpadded base64url.
            #expect(attempt.verifier.count == 43)
            #expect(attempt.state.count == 22)
            // The server accepts a challenge of exactly 43 base64url characters and nothing else.
            #expect(attempt.challenge.count == 43)
            #expect(attempt.challenge.unicodeScalars.allSatisfy(unreserved.contains))
            let sent = URLComponents(url: attempt.loginURL, resolvingAgainstBaseURL: false)?.queryItems?.first { $0.name == "challenge" }?.value
            #expect(sent == attempt.challenge)
            #expect(attempt.verifier.unicodeScalars.allSatisfy(unreserved.contains))
            #expect(attempt.state.unicodeScalars.allSatisfy(unreserved.contains))
            #expect(attempt.challenge != attempt.verifier)
            verifiers.insert(attempt.verifier); states.insert(attempt.state)
        }
        #expect(verifiers.count == 50)
        #expect(states.count == 50)
    }

    @Test func theLoginPageIsOpenedWithTheChallengeAndStateButNeverTheVerifier() throws {
        let attempt = try attempt("http://127.0.0.1:3000")
        let components = try #require(URLComponents(url: attempt.loginURL, resolvingAgainstBaseURL: false))
        #expect(components.scheme == "http")
        #expect(components.host == "127.0.0.1")
        #expect(components.port == 3000)
        #expect(components.path == "/login")
        let items = Dictionary(uniqueKeysWithValues: (components.queryItems ?? []).map { ($0.name, $0.value ?? "") })
        #expect(items == ["app": "1", "challenge": "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM", "state": "c3RhdGUtc3RhdGUtc3RhdGU"])
        #expect(!attempt.loginURL.absoluteString.contains(attempt.verifier))
    }

    @Test func theAnswerMustCarryThisAttemptsStateAndServer() throws {
        let attempt = try attempt()
        let good = try #require(URL(string: "cereal://auth?code=one-time-code&state=c3RhdGUtc3RhdGUtc3RhdGU&server=https%3A%2F%2Fnotes.example.com"))
        #expect(try attempt.code(from: good) == "one-time-code")
        // The server may be left out; the code only ever goes to the server the user chose.
        #expect(try attempt.code(from: try #require(URL(string: "cereal://auth?code=one-time-code&state=c3RhdGUtc3RhdGUtc3RhdGU"))) == "one-time-code")

        for mismatched in ["cereal://auth?code=one-time-code&state=someone-elses-state&server=https%3A%2F%2Fnotes.example.com",
                           "cereal://auth?code=one-time-code&state=c3RhdGUtc3RhdGUtc3RhdGUx",
                           "cereal://auth?code=one-time-code&state=C3RHDGUTC3RHDGUTC3RHDGU",
                           "cereal://auth?code=one-time-code&state=c3RhdGUtc3RhdGUtc3RhdGU&server=https%3A%2F%2Fevil.example.com",
                           "cereal://auth?code=one-time-code&state=c3RhdGUtc3RhdGUtc3RhdGU&server=https%3A%2F%2Fnotes.example.com%3A8443"] {
            #expect(throws: BrowserSignInError.mismatch, "\(mismatched)") { try attempt.code(from: try #require(URL(string: mismatched))) }
        }
        for invalid in ["cereal://auth?state=c3RhdGUtc3RhdGUtc3RhdGU", "cereal://auth?code=one-time-code", "cereal://shared?code=one-time-code&state=c3RhdGUtc3RhdGUtc3RhdGU",
                        "https://notes.example.com/auth?code=one-time-code&state=c3RhdGUtc3RhdGUtc3RhdGU", "cereal://auth?error=cancelled"] {
            #expect(throws: BrowserSignInError.invalidCallback, "\(invalid)") { try attempt.code(from: try #require(URL(string: invalid))) }
        }
    }

    @Test func theExchangeSendsTheCodeAndVerifierAndReturnsTheSessionToken() async throws {
        let attempt = try attempt("https://notes.example.com")
        let (configuration, id) = stubbed { _ in .json(#"{"token":"signed-native-session.signature"}"#) }
        let credentials = try await attempt.exchange(code: "one-time-code", configuration: configuration)
        #expect(credentials.bearerToken == "signed-native-session.signature")
        #expect(credentials.instance == attempt.instance)
        #expect(!credentials.description.contains("signed-native-session"))

        let requests = Wire.shared.requests(id)
        #expect(requests.count == 1)
        let request = try #require(requests.first)
        #expect(request.httpMethod == "POST")
        #expect(request.url?.absoluteString == "https://notes.example.com/api/v1/auth/app-handoff/exchange")
        #expect(request.value(forHTTPHeaderField: "Content-Type") == "application/json")
        #expect(request.value(forHTTPHeaderField: "Authorization") == nil)
        #expect(request.value(forHTTPHeaderField: "Cookie") == nil)
        let sent = try #require(try JSONSerialization.jsonObject(with: body(request)) as? [String: String])
        #expect(sent == ["code": "one-time-code", "verifier": "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"])
    }

    @Test func aRefusedOrUnsupportedExchangeSaysWhy() async throws {
        let attempt = try attempt()
        for (status, body) in [(400, #"{"error":"invalid_request"}"#), (401, #"{"error":"invalid_code"}"#)] {
            let (configuration, _) = stubbed { _ in .json(body, status: status) }
            await #expect(throws: BrowserSignInError.rejected) { try await attempt.exchange(code: "used-code", configuration: configuration) }
        }
        let (old, _) = stubbed { _ in Reply(status: 404) }
        await #expect(throws: BrowserSignInError.unsupported) { try await attempt.exchange(code: "code", configuration: old) }
        let (limited, _) = stubbed { _ in .json(#"{"error":"rate_limited"}"#, status: 429) }
        await #expect(throws: BrowserSignInError.rateLimited) { try await attempt.exchange(code: "code", configuration: limited) }
        #expect(BrowserSignInError.rateLimited.localizedDescription == "Too many attempts, try again in a minute.")
        let (empty, _) = stubbed { _ in .json(#"{"token":""}"#) }
        await #expect(throws: APIClientError.invalidResponse) { try await attempt.exchange(code: "code", configuration: empty) }
        let (offline, _) = stubbed { _ in Reply(failure: .timedOut) }
        await #expect(throws: APIClientError.transport(.timedOut)) { try await attempt.exchange(code: "code", configuration: offline) }
        // A redirect from the exchange is an answer, not somewhere to send the verifier next.
        let (redirect, id) = stubbed { _ in Reply(status: 307, headers: ["Location": "https://evil.example.com/collect"]) }
        await #expect(throws: APIClientError.httpStatus(307)) { try await attempt.exchange(code: "code", configuration: redirect) }
        #expect(Wire.shared.requests(id).allSatisfy { $0.url?.host == "notes.example.com" })
    }

    @Test func aServerSaysWhichSignInMethodsItOffers() async throws {
        let instance = try InstanceURL("https://notes.example.com")
        let (both, id) = stubbed { _ in .json(#"{"password":true,"google":true,"instanceName":"Acme notes"}"#) }
        let methods = try await MeetingNotesAPI.signInMethods(instance: instance, configuration: both)
        #expect(methods == SignInMethods(password: true, google: true, instanceName: "Acme notes"))
        #expect(methods.browserLabel == "Continue in browser")
        let request = try #require(Wire.shared.requests(id).first)
        #expect(request.httpMethod == "GET")
        #expect(request.url?.absoluteString == "https://notes.example.com/api/v1/auth/methods")
        #expect(request.value(forHTTPHeaderField: "Authorization") == nil)

        let (google, _) = stubbed { _ in .json(#"{"password":false,"google":true,"instanceName":"Cereal"}"#) }
        let googleOnly = try await MeetingNotesAPI.signInMethods(instance: instance, configuration: google)
        #expect(!googleOnly.password)
        #expect(googleOnly.browserLabel == "Continue with Google")

        let (password, _) = stubbed { _ in .json(#"{"password":true,"google":false,"instanceName":"Cereal"}"#) }
        let passwordOnly = try await MeetingNotesAPI.signInMethods(instance: instance, configuration: password)
        #expect(passwordOnly.password && passwordOnly.browser)
        #expect(passwordOnly.browserLabel == "Continue in browser")
    }

    @Test func anOlderServerWithoutTheMethodsEndpointKeepsThePasswordFormOnly() async throws {
        let instance = try InstanceURL("https://notes.example.com")
        let (old, _) = stubbed { _ in Reply(status: 404, headers: [:]) }
        let methods = try await MeetingNotesAPI.signInMethods(instance: instance, configuration: old)
        #expect(methods == .passwordOnly)
        #expect(methods.password && !methods.google && !methods.browser)
        #expect(methods.browserLabel == nil)

        let (page, _) = stubbed { _ in Reply(body: Data("<html></html>".utf8), headers: ["Content-Type": "text/html"]) }
        await #expect(throws: APIClientError.invalidResponse) { try await MeetingNotesAPI.signInMethods(instance: instance, configuration: page) }
        let (down, _) = stubbed { _ in Reply(failure: .cannotFindHost) }
        await #expect(throws: APIClientError.transport(.cannotFindHost)) { try await MeetingNotesAPI.signInMethods(instance: instance, configuration: down) }
        let (broken, _) = stubbed { _ in .json("{}", status: 500) }
        await #expect(throws: APIClientError.httpStatus(500)) { try await MeetingNotesAPI.signInMethods(instance: instance, configuration: broken) }
    }
}

private let untitledID = "3a49a4af-c660-42e9-a36a-a806fbb41e0d"
private func noteJSON(title: String, updatedAt: String, generated: Bool) -> String {
    #"{"id":"\#(untitledID)","title":"\#(title)","body":"","enhanced":"\#(generated ? "## Summary" : "")","date":"2026-11-12","duration":"1:12","createdAt":"2026-11-12T10:00:00.000Z","updatedAt":"\#(updatedAt)","recordingState":"stopped","generationState":"\#(generated ? "ready" : "idle")","transcriptRevision":2,"generatedRevision":\#(generated ? "2" : "null"),"transcriptSegments":[]}"#
}
private func summaryJSON(title: String, updatedAt: String) -> String {
    #"{"id":"\#(untitledID)","title":"\#(title)","date":"2026-11-12","duration":"1:12","createdAt":"2026-11-12T10:00:00.000Z","updatedAt":"\#(updatedAt)","recordingState":"stopped","generationState":"idle","transcriptRevision":2,"generatedRevision":null,"hasBody":false,"hasSummary":false,"hasTranscript":true,"speakerCount":1,"excerpt":"","meeting":null,"attendees":{"people":[],"others":0,"total":0},"organizationId":null,"folders":[]}"#
}

/// Generating notes for an untitled note also gives it a title. The app must
/// show it everywhere without being asked, and must not put it over a title
/// the person typed meanwhile.
@Suite @MainActor struct GeneratedTitleTests {
    private func model(_ handler: @escaping @Sendable (URLRequest) -> Reply) async throws -> (ServerNotesModel, String) {
        let name = "generated-title-" + UUID().uuidString
        let defaults = try #require(UserDefaults(suiteName: name))
        defaults.removePersistentDomain(forName: name)
        let id = Wire.shared.register { request in
            let path = request.url?.path ?? ""
            if path.hasSuffix("/api/v1/session") { return .json(sessionJSON) }
            if path.hasSuffix("/notes/summaries") {
                return .json(#"{"notes":[\#(summaryJSON(title: "", updatedAt: "2026-11-12T10:01:00.000Z"))],"nextCursor":null,"total":1}"#)
            }
            return handler(request)
        }
        let model = ServerNotesModel { credentials in
            let configuration = URLSessionConfiguration.ephemeral
            configuration.protocolClasses = [WireProtocol.self]
            configuration.httpAdditionalHeaders = ["X-Test-Case": id]
            return MeetingNotesAPI(credentials: credentials, configuration: configuration)
        }
        await model.connect(instance: "https://mine.example.com", token: "my-session-token")
        guard case .loaded = model.state else { throw APIClientError.unauthorized }
        return (model, id)
    }

    @Test func theTitleGeneratedWithTheNotesIsShownEverywhereAtOnce() async throws {
        let (model, id) = try await model { request in
            // Empty before, titled in the note that generation returns.
            request.httpMethod == "POST" && request.url?.path.hasSuffix("/generate") == true
                ? .json(noteJSON(title: "Launch readiness review", updatedAt: "2026-11-12T10:05:00.000Z", generated: true))
                : .json(noteJSON(title: "", updatedAt: "2026-11-12T10:01:00.000Z", generated: false))
        }
        #expect(model.list.rows.map(\.title) == [""])
        let before = model.list.revision
        // What a search made before generation holds for this note.
        let found = ServerNoteRow(id: untitledID, title: "", date: "2026-11-12", createdAt: "2026-11-12T10:00:00.000Z", updatedAt: "2026-11-12T10:01:00.000Z")
        #expect(model.latestTitle(of: found) == "")

        let note = try await model.generate(noteID: untitledID, expectedTranscriptRevision: 2)
        // The note page shows what generate returns; no second read was needed.
        #expect(note.title == "Launch readiness review")
        let reads = Wire.shared.requests(id).filter { $0.httpMethod == "GET" && $0.url?.path.hasSuffix("/notes/\(untitledID)") == true }
        #expect(reads.isEmpty)
        // Home's row and the sidebar read the list; it changed without a refresh.
        #expect(model.list.rows.map(\.title) == ["Launch readiness review"])
        #expect(model.list.revision > before)
        #expect(model.title(ofNote: untitledID) == "Launch readiness review")
        #expect(model.state.notes.first(where: { $0.id == untitledID })?.title == "Launch readiness review")
        // Search results on screen from before take the new title too.
        #expect(model.latestTitle(of: found) == "Launch readiness review")
    }

    @Test func aTitleTypedWhileNotesAreGeneratedIsNotReplacedByTheGeneratedOne() async throws {
        let (model, id) = try await model { request in
            let path = request.url?.path ?? ""
            if request.httpMethod == "POST", path.hasSuffix("/generate") {
                // Slow, and written before the person's title reached the server.
                Thread.sleep(forTimeInterval: 0.5)
                return .json(noteJSON(title: "A generated title", updatedAt: "2026-11-12T10:05:00.000Z", generated: true))
            }
            // The person's title, as saved and as read back: the server keeps theirs.
            return .json(noteJSON(title: "Mine", updatedAt: "2026-11-12T10:05:01.000Z", generated: request.httpMethod == "GET"))
        }
        let generating = Task { try await model.generate(noteID: untitledID, expectedTranscriptRevision: 2) }
        let deadline = ContinuousClock.now + .seconds(5)
        while !Wire.shared.requests(id).contains(where: { $0.url?.path.hasSuffix("/generate") == true }), ContinuousClock.now < deadline {
            try await Task.sleep(for: .milliseconds(10))
        }
        let typed = try await model.update(id: untitledID, UpdateNoteInput(body: nil, date: nil, duration: nil, enhanced: nil, title: "Mine", transcriptSegments: nil))
        #expect(typed.title == "Mine")
        #expect(model.list.rows.map(\.title) == ["Mine"])

        let generated = try await generating.value
        #expect(generated.title == "Mine")
        #expect(generated.generatedRevision == 2)
        #expect(model.list.rows.map(\.title) == ["Mine"])
        #expect(model.title(ofNote: untitledID) == "Mine")
        // The note was read again instead of trusting an answer that predates the title.
        let reads = Wire.shared.requests(id).filter { $0.httpMethod == "GET" && $0.url?.path.hasSuffix("/notes/\(untitledID)") == true }
        #expect(reads.count == 1)
    }

    @Test func savingOnlyTheBodyDoesNotCountAsNamingTheNote() async throws {
        let (model, id) = try await model { request in
            request.httpMethod == "POST" && request.url?.path.hasSuffix("/generate") == true
                ? .json(noteJSON(title: "A generated title", updatedAt: "2026-11-12T10:05:00.000Z", generated: true))
                : .json(noteJSON(title: "", updatedAt: "2026-11-12T10:02:00.000Z", generated: false))
        }
        _ = try await model.update(id: untitledID, UpdateNoteInput(body: "typed notes", date: nil, duration: nil, enhanced: nil, title: nil, transcriptSegments: nil))
        let sent = try #require(Wire.shared.requests(id).last(where: { $0.httpMethod == "PATCH" }))
        let payload = try #require(try JSONSerialization.jsonObject(with: body(sent)) as? [String: Any])
        #expect(payload["title"] == nil || payload["title"] is NSNull)
        #expect(try await model.generate(noteID: untitledID, expectedTranscriptRevision: 2).title == "A generated title")
        #expect(model.list.rows.map(\.title) == ["A generated title"])
    }

    @Test func aSearchResultNewerThanWhatIsHeldKeepsItsOwnTitle() async throws {
        let (model, _) = try await model { _ in Reply(status: 404) }
        // Renamed on another device after this list was loaded; the search is the newer news.
        let found = ServerNoteRow(id: untitledID, title: "Renamed elsewhere", date: "2026-11-12", createdAt: "2026-11-12T10:00:00.000Z", updatedAt: "2026-11-12T11:00:00.000Z")
        #expect(model.latestTitle(of: found) == "Renamed elsewhere")
        let unknown = ServerNoteRow(id: "00000000-0000-4000-8000-00000000000f", title: "Not in the list", date: "2026-11-12", createdAt: "2026-11-12T10:00:00.000Z", updatedAt: "2026-11-12T10:00:00.000Z")
        #expect(model.latestTitle(of: unknown) == "Not in the list")
    }
}

private final class MemoryTokens: BearerTokenStore, @unchecked Sendable {
    private let lock = NSLock()
    private var token: String?
    func load() throws -> String? { lock.lock(); defer { lock.unlock() }; return token }
    func save(_ token: String) throws { lock.lock(); defer { lock.unlock() }; self.token = token }
    func clear() throws { lock.lock(); defer { lock.unlock() }; token = nil }
}

@Suite @MainActor struct BrowserSignInModelTests {
    @Test func aBrowserSignInIsStoredAndUsedExactlyLikeAPasswordSignIn() async throws {
        let name = "browser-sign-in-" + UUID().uuidString
        let defaults = try #require(UserDefaults(suiteName: name))
        defer { defaults.removePersistentDomain(forName: name) }
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(name)
        defer { try? FileManager.default.removeItem(at: root) }
        let store = MemoryAccountStore()
        let (configuration, id) = stubbed { request in
            let path = request.url?.path ?? ""
            if path.hasSuffix("/auth/app-handoff/exchange") { return .json(#"{"token":"browser-session-token"}"#) }
            if path.hasSuffix("/api/v1/session") { return .json(sessionJSON) }
            if path.hasSuffix("/notes/summaries") { return .json(#"{"notes":[],"nextCursor":null,"total":0}"#) }
            return Reply(status: 404)
        }
        let header = id
        let accounts = AccountsModel(defaults: defaults, credentials: store, storage: AccountStorage(root: root)) { credentials in
            let configuration = URLSessionConfiguration.ephemeral
            configuration.protocolClasses = [WireProtocol.self]
            configuration.httpAdditionalHeaders = ["X-Test-Case": header]
            return MeetingNotesAPI(credentials: credentials, configuration: configuration)
        }
        let attempt = BrowserSignIn(instance: try InstanceURL("https://notes.example.com"), verifier: "verifier-verifier-verifier-verifier-verifier", state: "the-state")
        let callback = try #require(URL(string: "cereal://auth?code=one-time-code&state=the-state&server=https%3A%2F%2Fnotes.example.com"))
        let account = try await accounts.signIn(browser: attempt, callback: callback, configuration: configuration)

        let model = try #require(accounts.session(account)?.notes)
        guard case .loaded = model.state else { Issue.record("not signed in: \(model.state)"); return }
        #expect(model.instance?.description == "https://notes.example.com")
        #expect(model.session?.user.email == "owner@example.test")
        // Saved where a password sign-in saves it: the session in the account's own item, the account in preferences.
        #expect(try store.load(account)?.credentials.bearerToken == "browser-session-token")
        #expect(try store.load(account)?.credentials.instance.description == "https://notes.example.com")
        let saved = String(decoding: try #require(defaults.data(forKey: AccountsModel.registryKey)), as: UTF8.self)
        #expect(saved.contains("https:\\/\\/notes.example.com") || saved.contains("https://notes.example.com"))
        #expect(!saved.contains("browser-session-token"))
        let requests = Wire.shared.requests(id)
        #expect(requests.first?.url?.path == "/api/v1/auth/app-handoff/exchange")
        #expect(requests.first?.value(forHTTPHeaderField: "Authorization") == nil)
        #expect(requests.dropFirst().allSatisfy { $0.value(forHTTPHeaderField: "Authorization") == "Bearer browser-session-token" })
        #expect(requests.count == 3)
    }

    @Test func anAnswerForAnotherAttemptChangesNothing() async throws {
        let name = "browser-sign-in-" + UUID().uuidString
        let defaults = try #require(UserDefaults(suiteName: name))
        defer { defaults.removePersistentDomain(forName: name) }
        let store = MemoryAccountStore()
        let (configuration, id) = stubbed { _ in .json(#"{"token":"should-never-be-asked-for"}"#) }
        let accounts = AccountsModel(defaults: defaults, credentials: store,
                                     storage: AccountStorage(root: FileManager.default.temporaryDirectory.appendingPathComponent(name)))
        let attempt = BrowserSignIn(instance: try InstanceURL("https://notes.example.com"), verifier: "verifier-verifier-verifier-verifier-verifier", state: "the-state")
        let forged = try #require(URL(string: "cereal://auth?code=stolen-code&state=another-state&server=https%3A%2F%2Fnotes.example.com"))
        await #expect(throws: BrowserSignInError.mismatch) { try await accounts.signIn(browser: attempt, callback: forged, configuration: configuration) }
        #expect(Wire.shared.requests(id).isEmpty)
        #expect(store.count == 0)
        #expect(accounts.sessions.isEmpty)
        #expect(defaults.object(forKey: AccountsModel.registryKey) == nil)
    }
}
