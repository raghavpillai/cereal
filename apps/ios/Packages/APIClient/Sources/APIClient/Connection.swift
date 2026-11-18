import Foundation

/// A server's address: HTTPS, or plain HTTP to this device or to an address
/// on a private network (see `ServerAddress`). Plain HTTP to anything else is refused.
/// Query parameters, fragments, embedded credentials, and traversal are rejected.
public struct InstanceURL: Equatable, Sendable, CustomStringConvertible {
    public let url: URL

    public init(_ text: String) throws {
        let text = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.contains("\\"), var components = URLComponents(string: text),
              let scheme = components.scheme?.lowercased(),
              let host = components.host?.lowercased(), !host.isEmpty,
              components.user == nil, components.password == nil,
              components.query == nil, components.fragment == nil,
              components.port.map({ (1...65_535).contains($0) }) ?? true,
              !(components.path.split(separator: "/").contains { $0 == "." || $0 == ".." }),
              !host.unicodeScalars.contains(where: CharacterSet.whitespacesAndNewlines.contains)
        else { throw APIClientError.invalidInstanceURL }
        guard scheme == "https" || scheme == "http" else { throw APIClientError.insecureInstanceURL }
        guard ServerAddress.kind(scheme: scheme, host: components.encodedHost ?? host) != .publicHTTP else {
            throw APIClientError.insecureInstanceURL
        }
        components.scheme = scheme
        components.host = host
        while components.path.hasSuffix("/") { components.path.removeLast() }
        guard let url = components.url else { throw APIClientError.invalidInstanceURL }
        self.url = url
    }

    public var description: String { url.absoluteString }

    /// Uses route segments, not interpolated paths, so identifiers cannot change the route.
    func endpoint(_ segments: [String]) -> URL {
        segments.reduce(url) { result, segment in result.appendingPathComponent(segment) }
    }
}

/// Credentials are deliberately not Codable. They must not be persisted in preferences.
public struct InstanceCredentials: Sendable, CustomStringConvertible, CustomDebugStringConvertible {
    public let instance: InstanceURL
    let bearerToken: String

    public init(instance: InstanceURL, bearerToken: String) throws {
        let token = bearerToken.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !token.isEmpty, token.utf8.count <= 8192,
              token.unicodeScalars.allSatisfy({ (0x21...0x7e).contains($0.value) }) else {
            throw APIClientError.invalidToken
        }
        self.instance = instance
        self.bearerToken = token
    }

    public var description: String { "InstanceCredentials(instance: \(instance), bearerToken: <redacted>)" }
    public var debugDescription: String { description }
}

public enum APIClientError: Error, LocalizedError, Equatable, Sendable {
    case invalidInstanceURL
    case insecureInstanceURL
    case invalidToken
    case invalidIdentifier
    case secureStorage
    case invalidResponse
    case invalidJSON
    case unauthorized
    case notFound
    case unavailable
    case conflict
    case localStorage
    case httpStatus(Int)
    case transport(URLError.Code)

    public var errorDescription: String? {
        switch self {
        case .invalidInstanceURL: "Enter a server address without a query, fragment, or embedded credentials."
        case .insecureInstanceURL: ServerAddress.needsHTTPS
        case .invalidToken: "Enter a valid access token."
        case .secureStorage: "The access token could not be read or saved securely on this device."
        case .invalidIdentifier: "This note identifier is invalid."
        case .invalidResponse: "The server returned an unexpected response."
        case .invalidJSON: "The server response does not match this app's API version."
        case .unauthorized: "The server did not accept your session. Sign in again."
        case .notFound: "This note is no longer available."
        case .unavailable: "The server is temporarily unavailable. Try again shortly."
        case .conflict: "This note changed. Refresh it before trying again."
        case .localStorage: "The recording's files on this iPhone could not be saved or removed."
        case .httpStatus(let status): "The server could not complete the request (HTTP \(status))."
        case .transport(let code) where code == .notConnectedToInternet || code == .networkConnectionLost:
            "You're offline. Your notes will be available when the connection returns."
        case .transport(let code) where code == .timedOut:
            "The server took too long to respond. Try again."
        case .transport: "The server could not be reached. Check the server address and your connection."
        }
    }
}
