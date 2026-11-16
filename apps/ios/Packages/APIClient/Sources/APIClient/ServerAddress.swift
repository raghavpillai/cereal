import Foundation

/// How a server address may be reached from an iPhone.
public enum ServerAddressKind: Equatable, Sendable {
    /// HTTPS, or this device talking to itself.
    case secure
    /// Plain HTTP to an address that only exists on a private network: not
    /// encrypted, and acceptable there. iOS allows these without HTTPS.
    case privateNetwork
    /// Plain HTTP to anything else. iOS refuses it, so the app does too, with a reason.
    case publicHTTP
}

/// The one place that decides which server addresses the app will use. Every
/// address goes through it: the connection screen, a link someone sent, the
/// server named in a sign-in answer.
///
/// It only says whether an address is acceptable. Where the session may be
/// sent is a separate and stricter rule (the exact origin the app is signed
/// in to) and is not relaxed by anything here.
public enum ServerAddress {
    /// Shown wherever a public plain-HTTP address is refused.
    public static let needsHTTPS = "iPhone needs HTTPS for this address. Plain HTTP only works on a private network."
    /// Shown under the address field for a private plain-HTTP server.
    public static let notEncrypted = "Not encrypted. Fine on a private network."

    /// `host` is the host as a URL carries it: ASCII, with or without the
    /// brackets of an IPv6 address. Anything that is not plainly a private
    /// address is public; a host written to look like one is not one.
    public static func kind(scheme: String, host: String) -> ServerAddressKind {
        switch scheme.lowercased() {
        case "https": return .secure
        case "http": break
        default: return .publicHTTP
        }
        var host = host.lowercased()
        if host.hasPrefix("["), host.hasSuffix("]") { host = String(host.dropFirst().dropLast()) }
        // Letters, digits, dots, hyphens and (for IPv6) colons. An escaped,
        // zoned or non-ASCII host is not something to reason about.
        guard !host.isEmpty, host.utf8.count <= 253, host.utf8.allSatisfy({ byte in
            (48...57).contains(byte) || (97...122).contains(byte) || byte == 45 || byte == 46 || byte == 58
        }) else { return .publicHTTP }

        if host.contains(":") { return ipv6(host) }
        if let address = ipv4(host) { return ipv4Kind(address) }
        if host == "localhost" { return .secure }

        let labels = host.split(separator: ".", omittingEmptySubsequences: false).map(String.init)
        guard labels.allSatisfy(isLabel) else { return .publicHTTP }
        // A number written some other way (`10.1`, `0x0a000001`, `167772161`,
        // `012.0.0.1`) reaches an address the name does not show.
        guard let last = labels.last, !last.utf8.allSatisfy({ (48...57).contains($0) }),
              !labels.contains(where: { $0.hasPrefix("0x") }) else { return .publicHTTP }
        // A name with no dot is one the local network resolves.
        if labels.count == 1 { return .privateNetwork }
        for suffix in privateSuffixes where labels.count > suffix.count && Array(labels.suffix(suffix.count)) == suffix {
            return .privateNetwork
        }
        return .publicHTTP
    }

    public static func kind(of instance: InstanceURL) -> ServerAddressKind {
        let components = URLComponents(url: instance.url, resolvingAgainstBaseURL: false)
        return kind(scheme: components?.scheme ?? "", host: components?.encodedHost ?? "")
    }

    /// Names that are only ever resolved on a private network.
    private static let privateSuffixes: [[String]] = [["local"], ["internal"], ["lan"], ["home", "arpa"]]

    private static func isLabel(_ label: String) -> Bool {
        (1...63).contains(label.utf8.count) && !label.hasPrefix("-") && !label.hasSuffix("-")
    }

    /// Four decimal numbers from 0 to 255, each written the one ordinary way.
    private static func ipv4(_ host: String) -> [UInt8]? {
        let parts = host.split(separator: ".", omittingEmptySubsequences: false)
        guard parts.count == 4 else { return nil }
        var bytes: [UInt8] = []
        for part in parts {
            guard (1...3).contains(part.utf8.count), part.utf8.allSatisfy({ (48...57).contains($0) }),
                  part == "0" || !part.hasPrefix("0"), let value = UInt8(part) else { return nil }
            bytes.append(value)
        }
        return bytes
    }

    private static func ipv4Kind(_ a: [UInt8]) -> ServerAddressKind {
        switch (a[0], a[1]) {
        case (127, _): .secure                          // loopback
        case (10, _): .privateNetwork                   // RFC 1918
        case (172, 16...31): .privateNetwork            // RFC 1918
        case (192, 168): .privateNetwork                // RFC 1918
        case (100, 64...127): .privateNetwork           // shared address space, 100.64.0.0/10
        case (169, 254): .privateNetwork                // link-local
        default: .publicHTTP
        }
    }

    private static func ipv6(_ host: String) -> ServerAddressKind {
        var address = in6_addr()
        guard inet_pton(AF_INET6, host, &address) == 1 else { return .publicHTTP }
        let bytes = withUnsafeBytes(of: &address) { Array($0) }
        if bytes == [UInt8](repeating: 0, count: 15) + [1] { return .secure }             // ::1
        if bytes[0] & 0xfe == 0xfc { return .privateNetwork }                              // unique local, fc00::/7
        if bytes[0] == 0xfe, bytes[1] & 0xc0 == 0x80 { return .privateNetwork }            // link-local, fe80::/10
        // An IPv4 address carried in an IPv6 one is that IPv4 address.
        if bytes[0..<10].allSatisfy({ $0 == 0 }), bytes[10] == 0xff, bytes[11] == 0xff { return ipv4Kind(Array(bytes[12..<16])) }
        return .publicHTTP
    }
}
