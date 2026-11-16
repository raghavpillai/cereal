import Foundation
import Testing
@testable import APIClient

private let token = "Zk3vX9aQ2mN7pL5rT8wY1bC4dF6gH0jK-sU_eIoAxyz"

private func kind(_ host: String, scheme: String = "http") -> ServerAddressKind { ServerAddress.kind(scheme: scheme, host: host) }

private func sharedLink(_ text: String) throws -> SharedNoteLink {
    guard case .shared(let link)? = AppLink(text: text) else { throw APIClientError.invalidIdentifier }
    return link
}

private struct Reply: Sendable { var status = 200; var body = Data() }

private final class Seen: @unchecked Sendable {
    static let shared = Seen()
    private let lock = NSLock()
    private var requests: [String: [URLRequest]] = [:]
    func add(_ request: URLRequest) {
        lock.lock(); defer { lock.unlock() }
        requests[request.value(forHTTPHeaderField: "X-Test-Case") ?? "", default: []].append(request)
    }
    func all(_ id: String) -> [URLRequest] { lock.lock(); defer { lock.unlock() }; return requests[id] ?? [] }
}

/// Answers every request with a shared snapshot, and remembers what was asked.
private final class SnapshotProtocol: URLProtocol, @unchecked Sendable {
    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func startLoading() {
        Seen.shared.add(request)
        let http = HTTPURLResponse(url: request.url!, statusCode: 200, httpVersion: "HTTP/1.1", headerFields: ["Content-Type": "application/json"])!
        client?.urlProtocol(self, didReceive: http, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: Data(#"{"title":"Shared","enhanced":"text","body":""}"#.utf8))
        client?.urlProtocolDidFinishLoading(self)
    }
    override func stopLoading() {}
}

private func signedIn(to instance: String) throws -> (MeetingNotesAPI, String) {
    let id = UUID().uuidString
    let configuration = URLSessionConfiguration.ephemeral
    configuration.protocolClasses = [SnapshotProtocol.self]
    configuration.httpAdditionalHeaders = ["X-Test-Case": id]
    return (try MeetingNotesAPI(credentials: InstanceCredentials(instance: InstanceURL(instance), bearerToken: "my-session-token"), configuration: configuration), id)
}

@Suite struct ServerAddressTests {
    @Test func httpsIsSecureWhateverTheHost() {
        for host in ["notes.example.com", "10.30.1.77", "cereal.local", "8.8.8.8", "[2001:db8::1]", "nas"] {
            #expect(kind(host, scheme: "https") == .secure, "\(host)")
            #expect(kind(host, scheme: "HTTPS") == .secure, "\(host)")
        }
    }

    @Test func thisDeviceOverPlainHTTPIsSecure() {
        for host in ["localhost", "LOCALHOST", "127.0.0.1", "127.0.0.2", "127.255.255.254", "::1", "[::1]", "[0:0:0:0:0:0:0:1]"] {
            #expect(kind(host) == .secure, "\(host)")
        }
    }

    @Test func privateNetworkAddressesOverPlainHTTPArePrivate() {
        for host in [// RFC 1918
                     "10.0.0.1", "10.30.1.77", "10.255.255.255", "172.16.0.1", "172.31.255.255", "192.168.0.1", "192.168.255.254",
                     // Shared address space (100.64.0.0/10) and link-local
                     "100.64.0.1", "100.127.255.254", "169.254.1.1",
                     // IPv6 unique local and link-local, and an IPv4 address carried in IPv6
                     "fd12:3456:789a::1", "[fd00::1]", "fc00::1", "fe80::1", "[fe80::abcd:1]", "febf::1", "::ffff:10.0.0.1", "[::ffff:192.168.1.1]",
                     // Names that only a private network resolves
                     "cereal.local", "Mac-Studio.local", "nas.lan", "notes.internal", "box.home.arpa", "a.b.internal",
                     // A name with no dot
                     "nas", "cereal", "mac-studio", "xn--tst-bma"] {
            #expect(kind(host) == .privateNetwork, "\(host)")
        }
    }

    @Test func everythingElseOverPlainHTTPIsPublic() {
        for host in ["notes.example.com", "example.com", "8.8.8.8", "1.1.1.1", "11.0.0.1", "9.255.255.255",
                     // Just outside each private range
                     "172.15.255.255", "172.32.0.1", "192.167.1.1", "192.169.1.1", "100.63.255.255", "100.128.0.1", "169.253.1.1", "169.255.1.1", "0.0.0.0", "255.255.255.255",
                     "2001:db8::1", "[2606:4700::1111]", "::", "fec0::1", "ff02::1", "::ffff:8.8.8.8", "fbff::1", "fe00::1"] {
            #expect(kind(host) == .publicHTTP, "\(host)")
        }
    }

    @Test func aHostDressedAsAPrivateOneIsPublic() {
        for host in [// A private address or name as a label of a public name
                     "10.30.1.77.example.com", "192.168.1.1.evil.example", "localhost.example.com", "127.0.0.1.example.com",
                     "cereal.local.example.com", "internal.example.com", "lan.example.com", "home.arpa.example.com", "nas.local.com",
                     // A number written some other way reaches an address the text does not show
                     "167772161", "3232235777", "0x0a000001", "0x0a.0.0.1", "10.1", "10.0.1", "192.168.1", "012.0.0.1", "010.030.001.077",
                     "192.168.1.256", "192.168.1.1.1", "1.2.3.4.5", "10.0.0.-1", "10..0.1",
                     // Not a host at all
                     "", ".", ".local", "local.", "a..local", "-nas", "nas-", "nas.-lan", "10.30.1.77.", "cereal.local.",
                     "nas%2elocal", "exa%6dple", "nas local", "nas\tlocal", "nas/..", "nas@evil.example", "nás", "nas\u{202e}",
                     "fe80::1%en0", "[fe80::1%25en0]", "fd00::1::2", "fd00:zz::1", "[fd00::1", ":::", "1::2::3",
                     String(repeating: "a", count: 64), String(repeating: "a.", count: 130) + "local"] {
            #expect(kind(host) == .publicHTTP, "\(host.debugDescription)")
        }
        // The top-level names themselves are single labels, which a private network resolves.
        #expect(kind("local") == .privateNetwork)
        #expect(kind("internal") == .privateNetwork)
        // Another scheme is never acceptable, whatever the host.
        for scheme in ["ftp", "ws", "file", "", "httpx", "javascript"] { #expect(kind("10.30.1.77", scheme: scheme) == .publicHTTP, "\(scheme)") }
    }

    @Test func aServerAddressIsAcceptedWhenItIsSecureOrPrivateAndRefusedWithAReasonOtherwise() throws {
        for accepted in ["https://notes.example.com", "http://127.0.0.1:3000", "http://localhost:3000", "http://[::1]:3000",
                         "http://10.30.1.77:3000", "http://192.168.1.10", "http://172.20.0.5:8080/", "http://100.100.1.1:3000",
                         "http://cereal.local:3000", "http://nas:3000", "http://notes.internal", "http://[fd12:3456::1]:3000", " http://10.30.1.77:3000 "] {
            let instance = try InstanceURL(accepted)
            #expect(ServerAddress.kind(of: instance) != .publicHTTP, "\(accepted)")
            #expect(ServerOrigin(instance) != nil, "\(accepted)")
        }
        #expect(ServerAddress.kind(of: try InstanceURL("http://10.30.1.77:3000")) == .privateNetwork)
        #expect(ServerAddress.kind(of: try InstanceURL("http://cereal.local")) == .privateNetwork)
        #expect(ServerAddress.kind(of: try InstanceURL("http://127.0.0.1:3000")) == .secure)
        #expect(ServerAddress.kind(of: try InstanceURL("https://10.30.1.77")) == .secure)

        for refused in ["http://notes.example.com", "http://8.8.8.8:3000", "http://10.30.1.77.example.com", "http://167772161",
                        "http://0x0a.0.0.1", "http://10.1:3000", "http://[2001:db8::1]:3000", "http://cereal.local.example.com", "ftp://10.30.1.77"] {
            #expect(throws: APIClientError.insecureInstanceURL, "\(refused)") { try InstanceURL(refused) }
            #expect(ServerOrigin(refused) == nil, "\(refused)")
        }
        // Credentials in the address are refused before the host is looked at.
        #expect(throws: APIClientError.invalidInstanceURL) { try InstanceURL("http://10.30.1.77@evil.example") }
        #expect(APIClientError.insecureInstanceURL.localizedDescription == "iPhone needs HTTPS for this address. Plain HTTP only works on a private network.")
        #expect(ServerAddress.notEncrypted == "Not encrypted. Fine on a private network.")
    }

    @Test func theSessionStillGoesOnlyToTheExactOriginSignedInTo() async throws {
        // Signed in to a private plain-HTTP server: a note shared there is read with the session.
        let (api, id) = try signedIn(to: "http://10.30.1.77:3000")
        _ = try await api.sharedNote(try sharedLink("http://10.30.1.77:3000/s/\(token)"))
        let sent = try #require(Seen.shared.all(id).first)
        #expect(sent.url?.absoluteString == "http://10.30.1.77:3000/api/v1/shared/\(token)")
        #expect(sent.value(forHTTPHeaderField: "Authorization") == "Bearer my-session-token")

        // Another scheme, host or port is another server, private or not: nothing is sent at all.
        for other in ["https://10.30.1.77:3000/s/\(token)", "http://10.30.1.77/s/\(token)", "http://10.30.1.77:3001/s/\(token)",
                      "http://10.30.1.78:3000/s/\(token)", "http://10.30.1.7:3000/s/\(token)", "http://cereal.local:3000/s/\(token)",
                      "http://127.0.0.1:3000/s/\(token)", "http://localhost:3000/s/\(token)", "http://[::ffff:10.30.1.77]:3000/s/\(token)",
                      "https://notes.example.com/s/\(token)"] {
            let (api, id) = try signedIn(to: "http://10.30.1.77:3000")
            await #expect(throws: SharedNoteError.signInRequired, "\(other)") { try await api.sharedNote(try sharedLink(other)) }
            #expect(Seen.shared.all(id).isEmpty, "\(other)")
        }
        // Signed in over HTTPS, the same host over plain HTTP is not that server either.
        let (secure, secureID) = try signedIn(to: "https://cereal.local")
        await #expect(throws: SharedNoteError.signInRequired) { try await secure.sharedNote(try sharedLink("http://cereal.local/s/\(token)")) }
        #expect(Seen.shared.all(secureID).isEmpty)
        // Two names for one machine are two servers.
        let (named, namedID) = try signedIn(to: "http://cereal.local:3000")
        await #expect(throws: SharedNoteError.signInRequired) { try await named.sharedNote(try sharedLink("http://10.30.1.77:3000/s/\(token)")) }
        #expect(Seen.shared.all(namedID).isEmpty)
    }

    @Test func theBrowserSignInWorksForAPrivateAddressAndChecksTheServerThatComesBack() throws {
        let attempt = BrowserSignIn(instance: try InstanceURL("http://10.30.1.77:3000"), verifier: "verifier-verifier-verifier-verifier-verifier", state: "the-state")
        #expect(attempt.loginURL.absoluteString.hasPrefix("http://10.30.1.77:3000/login?app=1&challenge="))
        let good = try #require(URL(string: "cereal://auth?code=one-time&state=the-state&server=http%3A%2F%2F10.30.1.77%3A3000"))
        #expect(try attempt.code(from: good) == "one-time")
        for mismatched in ["cereal://auth?code=one-time&state=the-state&server=http%3A%2F%2F10.30.1.78%3A3000",
                           "cereal://auth?code=one-time&state=the-state&server=https%3A%2F%2F10.30.1.77%3A3000",
                           "cereal://auth?code=one-time&state=the-state&server=http%3A%2F%2F10.30.1.77",
                           "cereal://auth?code=one-time&state=the-state&server=http%3A%2F%2F127.0.0.1%3A3000"] {
            #expect(throws: BrowserSignInError.mismatch, "\(mismatched)") { try attempt.code(from: try #require(URL(string: mismatched))) }
        }
        // A public plain-HTTP server in the answer is not an answer.
        let publicServer = try #require(URL(string: "cereal://auth?code=one-time&state=the-state&server=http%3A%2F%2Fevil.example"))
        #expect(throws: BrowserSignInError.invalidCallback) { try attempt.code(from: publicServer) }
    }
}
