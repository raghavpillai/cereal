import ActivityKit
import Foundation

/// A calendar meeting that is starting, shown on the Lock Screen with a
/// "Take a note" key, as the reference does. The event's title and times are
/// the only things of the calendar's that enter ActivityKit: no attendees,
/// links or descriptions.
struct MeetingActivityAttributes: ActivityAttributes {
    struct ContentState: Codable, Hashable {
        var title: String
        var start: Date
        var end: Date

        /// The title as shown: one line, "Meeting" when the event has none.
        var shownTitle: String {
            let line = title.split(whereSeparator: \.isNewline).joined(separator: " ").trimmingCharacters(in: .whitespaces)
            return line.isEmpty ? "Meeting" : String(line.prefix(80))
        }

        /// "15:30 – 16:30", the 24-hour form Home writes under an event.
        func timeRange(timeZone: TimeZone = .current) -> String {
            let format = DateFormatter()
            format.locale = Locale(identifier: "en_US_POSIX"); format.timeZone = timeZone
            format.dateFormat = "HH:mm"
            return "\(format.string(from: start)) – \(format.string(from: end))"
        }

        func startTime(timeZone: TimeZone = .current) -> String { String(timeRange(timeZone: timeZone).prefix(5)) }
    }

    /// The account the event was read through (its identifier, never an email) and the event's id there.
    var account: String
    var eventID: String
    /// The calendar's colour, "#RRGGBB" or "RRGGBB". The bar beside the title.
    var color: String?

    var key: String { Self.key(account: account, eventID: eventID) }
    static func key(account: String, eventID: String) -> String { "\(account)/\(eventID)" }

    /// `cereal://meeting?account=…&event=…`: opens the event's note and starts recording.
    var takeNoteURL: URL {
        var parts = URLComponents()
        parts.scheme = "cereal"; parts.host = "meeting"
        parts.queryItems = [URLQueryItem(name: "account", value: account), URLQueryItem(name: "event", value: eventID)]
        return parts.url ?? URL(string: "cereal://meeting")!
    }

    /// The account and event a "Take a note" link names, or nil for any other URL.
    static func target(_ url: URL) -> (account: String, eventID: String)? {
        guard url.scheme == "cereal", url.host == "meeting",
              let items = URLComponents(url: url, resolvingAgainstBaseURL: false)?.queryItems,
              let account = items.first(where: { $0.name == "account" })?.value, !account.isEmpty,
              let event = items.first(where: { $0.name == "event" })?.value, !event.isEmpty else { return nil }
        return (account, event)
    }

    /// The red, green and blue of `color`, each 0...1, or nil when it is not six hex digits.
    static func rgb(_ text: String?) -> (red: Double, green: Double, blue: Double)? {
        guard let text else { return nil }
        let hex = text.hasPrefix("#") ? String(text.dropFirst()) : text
        guard hex.count == 6, let value = UInt32(hex, radix: 16) else { return nil }
        return (Double((value >> 16) & 0xFF) / 255, Double((value >> 8) & 0xFF) / 255, Double(value & 0xFF) / 255)
    }
}
