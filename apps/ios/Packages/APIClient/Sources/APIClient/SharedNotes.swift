import Foundation
import Observation

/// A shared snapshot as its server sent it. Every field is text someone else
/// wrote: it is shown, never run.
public struct SharedNote: Equatable, Sendable {
    public let title: String
    /// The generated notes, as Markdown.
    public let enhanced: String
    /// The sharer's own notes, as Markdown.
    public let body: String
    /// The meeting's day, when the server sent one.
    public let date: DateComponents?
    public let duration: String?

    public init(title: String, enhanced: String, body: String, date: DateComponents? = nil, duration: String? = nil) {
        self.title = title; self.enhanced = enhanced; self.body = body; self.date = date; self.duration = duration
    }

    /// As much of a document as is laid out on screen. A snapshot may hold two
    /// megabytes of one-character lines, and the note page lays out every line
    /// at once; the page shows the beginning and leaves the rest to the
    /// browser. A real note is far inside both limits.
    public static func shown(_ text: String, characters: Int = 60_000, lines: Int = 800) -> (text: String, isWhole: Bool) {
        var isWhole = true
        var text = Substring(text)
        if text.count > characters { text = text.prefix(characters); isWhole = false }
        var seen = 0
        if let end = text.firstIndex(where: { character in
            guard character.isNewline else { return false }
            seen += 1
            return seen >= lines
        }) { text = text[..<end]; isWhole = false }
        return (String(text), isWhole)
    }
}

public enum SharedNoteError: Error, Equatable, Sendable, LocalizedError {
    /// A shared note is read with an account on its own server. The app is not
    /// signed in to that server, or its session there is no longer accepted.
    case signInRequired
    /// The server has no such share: it never existed, was turned off, or has expired.
    case unavailable
    case tooLarge
    case invalidResponse
    case server(Int)
    case network(URLError.Code)

    /// Worth asking again. A share that is gone stays gone.
    public var isRetryable: Bool { self != .unavailable && self != .tooLarge && self != .signInRequired }

    public var errorDescription: String? {
        switch self {
        case .signInRequired: "Sign in to this note's server to view it."
        case .unavailable: "This shared note is not available. The link may have expired or been turned off."
        case .tooLarge: "This shared note is too large to open here."
        case .invalidResponse: "The server did not send a shared note."
        case .server(let status): "The server could not send this note (HTTP \(status))."
        case .network(let code) where code == .notConnectedToInternet || code == .networkConnectionLost: "You're offline. Try again when the connection returns."
        case .network(let code) where code == .timedOut: "The server took too long to respond. Try again."
        case .network: "The server could not be reached."
        }
    }
}

/// Reads a shared snapshot's answer. Which server is asked, and with whose
/// session, is decided by `MeetingNotesAPI.sharedNote`; nothing here adds a credential.
enum SharedNoteReader {
    /// A snapshot larger than this is refused, by its declared length or as it arrives.
    static let maximumBytes = 2 * 1024 * 1024

    static func read(_ request: URLRequest, session: URLSession) async throws -> SharedNote {
        do {
            let (bytes, response) = try await session.bytes(for: request)
            guard let response = response as? HTTPURLResponse else { throw SharedNoteError.invalidResponse }
            switch response.statusCode {
            case 200: break
            case 401, 403: throw SharedNoteError.signInRequired
            case 404, 410: throw SharedNoteError.unavailable
            // The session's own delegate follows no redirect, so one arrives here as an answer.
            case 300..<400: throw SharedNoteError.invalidResponse
            default: throw SharedNoteError.server(response.statusCode)
            }
            guard response.expectedContentLength <= Int64(maximumBytes) else { throw SharedNoteError.tooLarge }
            var data = Data()
            data.reserveCapacity(Int(max(0, min(response.expectedContentLength, Int64(maximumBytes)))))
            for try await byte in bytes {
                guard data.count < maximumBytes else { throw SharedNoteError.tooLarge }
                data.append(byte)
            }
            guard let wire = try? JSONDecoder().decode(Wire.self, from: data) else { throw SharedNoteError.invalidResponse }
            return wire.note
        } catch let error as URLError {
            if Task.isCancelled || error.code == .cancelled { throw CancellationError() }
            throw SharedNoteError.network(error.code)
        }
    }

    /// Only the fields this app shows. A server of another version may send more, or fewer.
    private struct Wire: Decodable {
        let title: String?
        let body: String?
        let enhanced: String?
        let date: String?
        let duration: String?

        var note: SharedNote {
            SharedNote(title: String((title ?? "").prefix(500)), enhanced: enhanced ?? "", body: body ?? "",
                       date: date.flatMap(Self.day), duration: duration.flatMap(Self.length))
        }

        /// `YYYY-MM-DD`, as a day rather than an instant: it is the same day in every time zone.
        private static func day(_ text: String) -> DateComponents? {
            let parts = text.split(separator: "-", omittingEmptySubsequences: false)
            guard text.utf8.count == 10, parts.count == 3, parts[0].utf8.count == 4,
                  let year = Int(parts[0]), let month = Int(parts[1]), let day = Int(parts[2]),
                  (1...12).contains(month), (1...31).contains(day) else { return nil }
            return DateComponents(year: year, month: month, day: day)
        }

        /// `1:12` or `1:02:03`. Anything else, or no time at all, is left out.
        private static func length(_ text: String) -> String? {
            guard (1...12).contains(text.utf8.count), text.utf8.allSatisfy({ (48...57).contains($0) || $0 == 58 }),
                  text.utf8.contains(where: { (49...57).contains($0) }) else { return nil }
            return text
        }
    }
}

/// A shared note the user has opened.
public struct SharedNoteVisit: Equatable, Sendable, Identifiable {
    public let link: SharedNoteLink
    public var title: String
    public var openedAt: Date

    public var id: String { link.webURL.absoluteString }
    public var host: String { link.origin.displayHost }
}

/// "Shared with me": the shared notes opened on this device, newest first.
/// Kept in this app's preferences and nowhere else. Opening a note again moves
/// it to the top; the oldest are dropped beyond `limit`.
@MainActor @Observable
public final class SharedNoteHistory {
    public static let limit = 50
    public private(set) var visits: [SharedNoteVisit] = []
    @ObservationIgnored private let defaults: UserDefaults
    @ObservationIgnored private let key: String

    public init(defaults: UserDefaults = .standard, key: String = "cereal.sharedWithMe") {
        self.defaults = defaults
        self.key = key
        guard let data = defaults.data(forKey: key), let saved = try? JSONDecoder().decode([Saved].self, from: data) else { return }
        // What was saved is read as strictly as a link from outside.
        visits = Array(saved.compactMap { entry -> SharedNoteVisit? in
            guard case .shared(let link)? = AppLink(text: entry.url) else { return nil }
            return SharedNoteVisit(link: link, title: String(entry.title.prefix(500)), openedAt: entry.openedAt)
        }.prefix(Self.limit))
    }

    public func record(_ link: SharedNoteLink, title: String, at date: Date = .now) {
        visits.removeAll { $0.link == link }
        visits.insert(SharedNoteVisit(link: link, title: String(title.prefix(500)), openedAt: date), at: 0)
        if visits.count > Self.limit { visits.removeLast(visits.count - Self.limit) }
        save()
    }

    public func remove(_ id: SharedNoteVisit.ID) {
        visits.removeAll { $0.id == id }
        save()
    }

    /// Forgets the notes opened from one server: for when the last account
    /// that could read them has been removed and its data erased.
    public func removeAll(on origin: ServerOrigin) {
        guard visits.contains(where: { $0.link.origin == origin }) else { return }
        visits.removeAll { $0.link.origin == origin }
        save()
    }

    private func save() {
        let saved = visits.map { Saved(url: $0.id, title: $0.title, openedAt: $0.openedAt) }
        if let data = try? JSONEncoder().encode(saved) { defaults.set(data, forKey: key) }
    }

    private struct Saved: Codable {
        let url: String
        let title: String
        let openedAt: Date
    }
}
