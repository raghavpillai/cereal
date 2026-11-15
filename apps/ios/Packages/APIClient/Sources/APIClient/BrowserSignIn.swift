import CryptoKit
import Foundation
import Security

/// What a server offers on its sign-in page.
public struct SignInMethods: Equatable, Sendable {
    public let password: Bool
    public let google: Bool
    public let instanceName: String?
    /// The server can finish a sign-in in the browser and hand it to this app.
    /// False on a server from before it said which methods it offers.
    public let browser: Bool

    public init(password: Bool, google: Bool, instanceName: String? = nil, browser: Bool = true) {
        self.password = password; self.google = google; self.instanceName = instanceName; self.browser = browser
    }

    /// A server that does not say: email and password, as this app has always asked for.
    public static let passwordOnly = SignInMethods(password: true, google: false, browser: false)

    /// The browser button's label, or nil when there is no such button.
    public var browserLabel: String? {
        guard browser else { return nil }
        return google && !password ? "Continue with Google" : "Continue in browser"
    }
}

extension MeetingNotesAPI {
    /// Asks a server how people sign in to it. No credentials are sent.
    public static func signInMethods(instance: InstanceURL, configuration: URLSessionConfiguration = .ephemeral) async throws -> SignInMethods {
        var request = URLRequest(url: instance.endpoint(["api", "v1", "auth", "methods"]))
        request.httpMethod = "GET"
        let (data, status) = try await anonymous(request, configuration: configuration)
        switch status {
        case 200:
            guard let wire = try? JSONDecoder().decode(MethodsWire.self, from: data) else { throw APIClientError.invalidResponse }
            return SignInMethods(password: wire.password, google: wire.google, instanceName: wire.instanceName.map { String($0.prefix(120)) })
        case 404: return .passwordOnly
        case 503: throw APIClientError.unavailable
        default: throw APIClientError.httpStatus(status)
        }
    }

    private struct MethodsWire: Decodable {
        let password: Bool
        let google: Bool
        let instanceName: String?
    }

    /// One request that carries no session and keeps nothing: no cookies, no cache, no redirects.
    fileprivate static func anonymous(_ request: URLRequest, configuration: URLSessionConfiguration) async throws -> (Data, Int) {
        configuration.httpCookieAcceptPolicy = .never
        configuration.httpShouldSetCookies = false
        configuration.urlCache = nil
        configuration.requestCachePolicy = .reloadIgnoringLocalCacheData
        configuration.timeoutIntervalForRequest = 20
        let session = URLSession(configuration: configuration, delegate: RejectRedirects(), delegateQueue: nil)
        defer { session.invalidateAndCancel() }
        var request = request
        request.httpShouldHandleCookies = false
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        do {
            let (data, response) = try await session.data(for: request)
            guard let response = response as? HTTPURLResponse else { throw APIClientError.invalidResponse }
            return (data, response.statusCode)
        } catch let error as URLError {
            if Task.isCancelled || error.code == .cancelled { throw CancellationError() }
            throw APIClientError.transport(error.code)
        }
    }
}

public enum BrowserSignInError: Error, Equatable, Sendable, LocalizedError {
    /// The user closed the browser. Nothing went wrong and nothing is shown.
    case cancelled
    case browserUnavailable
    case invalidCallback
    /// The answer belongs to another sign-in attempt, or to another server.
    case mismatch
    case rejected
    case unsupported
    /// The server has seen too many sign-in attempts and is refusing them for a while.
    case rateLimited

    public var errorDescription: String? {
        switch self {
        case .cancelled: "Sign-in was cancelled."
        case .browserUnavailable: "The browser could not be opened for sign-in."
        case .invalidCallback: "The browser did not return a sign-in this app can use. Try again."
        case .mismatch: "The sign-in that came back does not match the one this app started. Try again."
        case .rejected: "The server did not accept this sign-in. It may have taken too long. Try again."
        case .unsupported: "This server cannot sign in through the browser. Use your email and password."
        case .rateLimited: "Too many attempts, try again in a minute."
        }
    }
}

/// One attempt to sign in through the browser. The app opens `loginURL`; the
/// person signs in on the server's own page, by whichever method it offers;
/// the page hands back a short-lived code, and the code plus the secret that
/// only this attempt holds are exchanged for a session.
public struct BrowserSignIn: Sendable {
    public let instance: InstanceURL
    /// Stays in the app. The server sees only its hash until the exchange.
    let verifier: String
    public let state: String

    public init(instance: InstanceURL) throws {
        self.init(instance: instance, verifier: try Self.random(bytes: 32), state: try Self.random(bytes: 16))
    }

    init(instance: InstanceURL, verifier: String, state: String) {
        self.instance = instance; self.verifier = verifier; self.state = state
    }

    /// `base64url(SHA-256(verifier))`, as in RFC 7636 (S256).
    public var challenge: String { Self.challenge(for: verifier) }

    static func challenge(for verifier: String) -> String {
        base64URL(Data(SHA256.hash(data: Data(verifier.utf8))))
    }

    static func random(bytes count: Int) throws -> String {
        var bytes = [UInt8](repeating: 0, count: count)
        guard SecRandomCopyBytes(kSecRandomDefault, count, &bytes) == errSecSuccess else { throw APIClientError.secureStorage }
        return base64URL(Data(bytes))
    }

    private static func base64URL(_ data: Data) -> String {
        data.base64EncodedString().replacingOccurrences(of: "+", with: "-")
            .replacingOccurrences(of: "/", with: "_").replacingOccurrences(of: "=", with: "")
    }

    /// The server's sign-in page, told that an app is waiting for the result.
    public var loginURL: URL {
        let page = instance.endpoint(["login"])
        var components = URLComponents(url: page, resolvingAgainstBaseURL: false)
        components?.queryItems = [URLQueryItem(name: "app", value: "1"), URLQueryItem(name: "challenge", value: challenge),
                                  URLQueryItem(name: "state", value: state)]
        return components?.url ?? page
    }

    /// The code from `cereal://auth?…`, once the answer is known to be for this attempt.
    public func code(from callback: URL) throws -> String {
        guard case .auth(let answer)? = AppLink(callback) else { throw BrowserSignInError.invalidCallback }
        guard Self.same(answer.state, state) else { throw BrowserSignInError.mismatch }
        // The code is only ever sent to the server the user chose, whatever the answer names.
        if let origin = answer.origin, origin != ServerOrigin(instance) { throw BrowserSignInError.mismatch }
        return answer.code
    }

    /// Compares every byte, so the time taken says nothing about where two values differ.
    private static func same(_ left: String, _ right: String) -> Bool {
        let left = Array(left.utf8), right = Array(right.utf8)
        guard left.count == right.count else { return false }
        return zip(left, right).reduce(UInt8(0)) { $0 | ($1.0 ^ $1.1) } == 0
    }

    /// Trades the code for a session: the same kind of token a password sign-in returns.
    public func exchange(code: String, configuration: URLSessionConfiguration = .ephemeral) async throws -> InstanceCredentials {
        var request = URLRequest(url: instance.endpoint(["api", "v1", "auth", "app-handoff", "exchange"]))
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = try JSONSerialization.data(withJSONObject: ["code": code, "verifier": verifier])
        let (data, status) = try await MeetingNotesAPI.anonymous(request, configuration: configuration)
        switch status {
        case 200..<300:
            guard let token = (try? JSONDecoder().decode(Exchanged.self, from: data))?.token else { throw APIClientError.invalidResponse }
            do { return try InstanceCredentials(instance: instance, bearerToken: token) }
            catch { throw APIClientError.invalidResponse }
        case 400, 401: throw BrowserSignInError.rejected
        case 404: throw BrowserSignInError.unsupported
        case 429: throw BrowserSignInError.rateLimited
        case 503: throw APIClientError.unavailable
        default: throw APIClientError.httpStatus(status)
        }
    }

    private struct Exchanged: Decodable { let token: String }
}
