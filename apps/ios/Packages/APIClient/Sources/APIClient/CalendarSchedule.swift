import Foundation
import Contracts

/// Calendar date-only intervals use the feed's exclusive end date. Timed
/// intervals are compared as instants in the viewer's selected local day.
public enum CalendarSchedule {
    public static func timestamp(_ text: String) -> Date? {
        let parser = ISO8601DateFormatter()
        parser.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        if let date = parser.date(from: text) { return date }
        parser.formatOptions = [.withInternetDateTime]
        return parser.date(from: text)
    }

    public static func dateOnly(_ text: String, calendar: Calendar = .current) -> Date? {
        let parts = text.split(separator: "-").compactMap { Int($0) }
        guard parts.count == 3, text.count == 10 else { return nil }
        let components = DateComponents(year: parts[0], month: parts[1], day: parts[2])
        guard let date = calendar.date(from: components), calendar.component(.year, from: date) == parts[0], calendar.component(.month, from: date) == parts[1], calendar.component(.day, from: date) == parts[2] else { return nil }
        return date
    }

    public static func isActiveTimedEvent(_ event: CalendarEventDTO, now: Date = .now) -> Bool {
        guard !event.allDay, let start = timestamp(event.startAt), let end = timestamp(event.endAt) else { return false }
        return start <= now && now < end
    }

    public static func events(_ events: [CalendarEventDTO], on day: Date, calendar: Calendar = .current, now: Date = .now, enabledCalendarIDs: Set<String>? = nil, knownCalendarIDs: Set<String>? = nil) -> [CalendarEventDTO] {
        let start = calendar.startOfDay(for: day)
        guard let end = calendar.date(byAdding: .day, value: 1, to: start) else { return [] }
        let stamp = DateFormatter()
        stamp.locale = Locale(identifier: "en_US_POSIX")
        stamp.calendar = calendar
        stamp.timeZone = calendar.timeZone
        stamp.dateFormat = "yyyy-MM-dd"
        let localDate = stamp.string(from: start)
        return events.filter { event in
            if let enabledCalendarIDs, knownCalendarIDs?.contains(event.calendarId) ?? true, !enabledCalendarIDs.contains(event.calendarId) { return false }
            if event.allDay { return event.startDate <= localDate && localDate < event.endDate }
            guard let eventStart = timestamp(event.startAt), let eventEnd = timestamp(event.endAt), eventEnd > eventStart else { return false }
            return eventStart < end && eventEnd > start && (!calendar.isDate(day, inSameDayAs: now) || eventEnd > now)
        }.sorted { left, right in
            if left.allDay != right.allDay { return left.allDay }
            let leftStart = timestamp(left.startAt) ?? .distantFuture
            let rightStart = timestamp(right.startAt) ?? .distantFuture
            return leftStart == rightStart ? left.id < right.id : leftStart < rightStart
        }
    }

    public static func meetingURL(_ text: String?) -> URL? {
        guard let text, let url = URLComponents(string: text),
              ["https", "http"].contains(url.scheme?.lowercased() ?? ""),
              url.host?.isEmpty == false, url.user == nil, url.password == nil else { return nil }
        return url.url
    }
}
