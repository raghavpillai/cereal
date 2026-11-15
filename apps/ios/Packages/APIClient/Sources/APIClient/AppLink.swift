import Foundation

/// Where a Cereal server lives: scheme, host and port, with nothing after them.
/// HTTPS, or HTTP on this device's own loopback addresses for development.
/// The host is kept in its ASCII form, so a look-alike name is shown as what it is.
public struct ServerOrigin: Hashable, Sendable, CustomStringConvertible {
    public let scheme: String
    /// Lower-case ASCII. An IPv6 address is kept without its brackets.
    public let host: String
    /// Nil when it is the scheme's own port.
    public let port: Int?

    /// A bare origin such as `https://notes.example.com` or `http://127.0.0.1:3000`.
    public init?(_ text: String) {
        guard AppLink.isPlain(text), let components = URLComponents(string: text),
              components.path.isEmpty || components.path == "/",
              components.query == nil, components.fragment == nil else { return nil }
        self.init(components: components)
    }

    /// The origin of an instance the user typed, whatever path follows it.
    public init?(_ instance: InstanceURL) {
        guard let components = URLComponents(url: instance.url, resolvingAgainstBaseURL: false) else { return nil }
        self.init(components: components)
    }

    init?(components: URLComponents) {
        guard let scheme = components.scheme?.lowercased(), scheme == "https" || scheme == "http",
              components.user == nil, components.password == nil,
              var host = components.encodedHost?.lowercased(), !host.isEmpty else { return nil }
        if host.hasPrefix("["), host.hasSuffix("]") { host = String(host.dropFirst().dropLast()) }
        let address = host.contains(":")
        if address {
            guard host.utf8.count <= 45, host.utf8.allSatisfy({ Self.isHex($0) || $0 == 58 || $0 == 46 }) else { return nil }
        } else {
            guard host.utf8.count <= 253, host.range(of: Self.namePattern, options: .regularExpression) != nil else { return nil }
        }
        if let port = components.port { guard (1...65_535).contains(port) else { return nil } }
        guard scheme == "https" || ["localhost", "127.0.0.1", "::1"].contains(host) else { return nil }
        self.scheme = scheme
        self.host = host
        self.port = components.port == (scheme == "https" ? 443 : 80) ? nil : components.port
    }

    private static let namePattern = "^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?(\\.[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?)*$"
    private static func isHex(_ byte: UInt8) -> Bool { (48...57).contains(byte) || (97...102).contains(byte) }

    /// Host and, when it is not the usual one, port: what a person recognises a server by.
    public var displayHost: String {
        (host.contains(":") ? "[\(host)]" : host) + (port.map { ":\($0)" } ?? "")
    }

    public var description: String { "\(scheme)://\(displayHost)" }

    /// A route on this server, built from whole segments so a segment cannot change the route.
    func endpoint(_ segments: [String]) -> URL {
        // The origin was assembled from validated parts, so it is always a URL.
        segments.reduce(URL(string: description)!) { $0.appendingPathComponent($1) }
    }

    /// True when `instance` is this server and nothing more: links name a server
    /// by its origin. Scheme, host and port must all be the same after
    /// normalising, so `localhost` and `127.0.0.1` are two different servers.
    public func isInstance(_ instance: InstanceURL) -> Bool {
        instance.url.path.isEmpty && ServerOrigin(instance) == self
    }
}

/// A read-only snapshot someone shared: `<origin>/s/<token>`.
public struct SharedNoteLink: Hashable, Sendable {
    public let origin: ServerOrigin
    public let token: String

    /// What people send each other, and what "Open in browser" opens.
    public var webURL: URL { origin.endpoint(["s", token]) }
}

/// A note in somebody's own account: `<origin>/notes/<id>`.
public struct NoteLink: Hashable, Sendable {
    public let origin: ServerOrigin
    /// A lower-case UUID.
    public let noteID: String

    public var webURL: URL { origin.endpoint(["notes", noteID]) }
}

/// What the browser hands back after a sign-in: `cereal://auth?code=…&state=…&server=…`.
public struct AuthCallback: Equatable, Sendable {
    public let code: String
    public let state: String
    public let origin: ServerOrigin?
}

/// Every URL the app can be opened with, apart from the Live Activity's own
/// `cereal://recording/…`. This is the only place such a URL is read: a web
/// link (a universal link, or one pasted by hand) and its `cereal://` form
/// become the same value, and anything else is no link at all.
public enum AppLink: Equatable, Sendable {
    case shared(SharedNoteLink)
    case note(NoteLink)
    case auth(AuthCallback)

    public static let scheme = "cereal"
    /// Longer than any real link; a longer one is refused unread.
    static let maximumLength = 2048

    public init?(_ url: URL) { self.init(text: url.absoluteString) }

    /// A link as text, for one typed or pasted. Surrounding white space is ignored.
    public init?(text: String) {
        let text = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard Self.isPlain(text), let components = URLComponents(string: text),
              let scheme = components.scheme?.lowercased() else { return nil }
        switch scheme {
        case Self.scheme:
            guard let link = Self.app(components) else { return nil }
            self = link
        case "https", "http":
            guard let link = Self.web(components) else { return nil }
            self = link
        default: return nil
        }
    }

    /// Printable ASCII only. A backslash is read as a slash by some URL parsers and not by others.
    static func isPlain(_ text: String) -> Bool {
        !text.isEmpty && text.utf8.count <= maximumLength && text.utf8.allSatisfy { $0 > 0x20 && $0 < 0x7f && $0 != 0x5c }
    }

    /// `https://<server>/s/<token>` or `https://<server>/notes/<id>`. The path is
    /// matched whole and unescaped, so an encoded or dotted segment is not a link.
    /// A query or fragment added along the way is dropped.
    private static func web(_ components: URLComponents) -> AppLink? {
        guard let origin = ServerOrigin(components: components) else { return nil }
        let path = components.percentEncodedPath
        guard !path.contains("%") else { return nil }
        let segments = path.split(separator: "/", omittingEmptySubsequences: false).map(String.init)
        guard segments.count == 3, segments[0].isEmpty else { return nil }
        switch segments[1] {
        case "s":
            guard isToken(segments[2]) else { return nil }
            return .shared(SharedNoteLink(origin: origin, token: segments[2]))
        case "notes":
            guard let id = noteID(segments[2]) else { return nil }
            return .note(NoteLink(origin: origin, noteID: id))
        default: return nil
        }
    }

    /// `cereal://shared?url=…`, `cereal://note?server=…&id=…`, `cereal://auth?code=…&state=…&server=…`.
    private static func app(_ components: URLComponents) -> AppLink? {
        guard components.user == nil, components.password == nil, components.port == nil, components.fragment == nil,
              components.path.isEmpty || components.path == "/", let action = components.host?.lowercased() else { return nil }
        let items = components.queryItems ?? []
        /// The one value of a parameter. A repeated parameter is ambiguous and is refused.
        func value(_ name: String) -> String? {
            let found = items.filter { $0.name == name }
            return found.count == 1 ? found[0].value : nil
        }
        func absent(_ name: String) -> Bool { !items.contains { $0.name == name } }
        switch action {
        case "shared":
            // The value is decoded once. One that is still encoded, or that is
            // itself an app link, does not parse as a web link.
            guard let inner = value("url"), isPlain(inner), let web = URLComponents(string: inner),
                  ["https", "http"].contains(web.scheme?.lowercased() ?? ""),
                  case .shared(let link)? = Self.web(web) else { return nil }
            return .shared(link)
        case "note":
            guard let server = value("server"), let origin = ServerOrigin(server),
                  let id = value("id").flatMap(noteID) else { return nil }
            return .note(NoteLink(origin: origin, noteID: id))
        case "auth":
            guard let code = value("code"), isOpaque(code, limit: 1024),
                  let state = value("state"), isOpaque(state, limit: 256) else { return nil }
            if absent("server") { return .auth(AuthCallback(code: code, state: state, origin: nil)) }
            guard let origin = value("server").flatMap(ServerOrigin.init(_:)) else { return nil }
            return .auth(AuthCallback(code: code, state: state, origin: origin))
        default: return nil
        }
    }

    /// The server makes 43 characters of base64url. Other lengths are allowed
    /// for a server of another version, within reason.
    private static func isToken(_ text: String) -> Bool {
        (16...128).contains(text.utf8.count) && text.utf8.allSatisfy { byte in
            (48...57).contains(byte) || (65...90).contains(byte) || (97...122).contains(byte) || byte == 45 || byte == 95
        }
    }

    private static func noteID(_ text: String) -> String? {
        guard text.utf8.count == 36, let id = UUID(uuidString: text) else { return nil }
        return id.uuidString.lowercased()
    }

    private static func isOpaque(_ text: String, limit: Int) -> Bool {
        !text.isEmpty && text.utf8.count <= limit && text.utf8.allSatisfy { $0 > 0x20 && $0 < 0x7f }
    }
}

extension SharedNoteLink {
    /// The link the web page hands to the app.
    public var appURL: URL {
        var components = URLComponents()
        components.scheme = AppLink.scheme
        components.host = "shared"
        components.queryItems = [URLQueryItem(name: "url", value: webURL.absoluteString)]
        return components.url ?? webURL
    }
}
