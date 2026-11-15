import Foundation
import Contracts

/// A participant name is supplied calendar/contact/manual data. An email is a
/// fallback label, never a source for guessing a person's name or voice identity.
public enum MeetingPresentation {
    public static func name(_ person: MeetingParticipantDTO) -> String {
        if let name = person.displayName, !name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { return name }
        let name = [person.givenName, person.familyName].compactMap { $0 }.filter { !$0.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }.joined(separator: " ")
        if !name.isEmpty { return name }
        return person.email.flatMap { $0.isEmpty ? nil : $0 } ?? "Unknown participant"
    }

    public static func people(_ meeting: MeetingContextDTO) -> [MeetingParticipantDTO] {
        var people = meeting.participants.filter { !$0.isResource }
        if let organizer = meeting.organizer, !organizer.isResource, !people.contains(where: { person in
            if person.id == organizer.id { return true }
            guard let email = person.email, let other = organizer.email else { return false }
            return email.caseInsensitiveCompare(other) == .orderedSame
        }) { people.insert(organizer, at: 0) }
        return people
    }

    public static func start(_ meeting: MeetingContextDTO, calendar: Calendar = .current) -> Date? {
        if !meeting.allDay { return CalendarSchedule.timestamp(meeting.startAt) }
        return CalendarSchedule.dateOnly(String(meeting.startAt.prefix(10)), calendar: calendar)
    }

    /// The calendar day used for grouping is not a recording timestamp. Event
    /// context owns its displayed start; an ad-hoc note uses canonical creation.
    public static func metadataDate(createdAt: String, fallbackDate: String, meeting: MeetingContextDTO?, calendar: Calendar = .current) -> Date? {
        meeting.flatMap { start($0, calendar: calendar) }
            ?? CalendarSchedule.timestamp(createdAt)
            ?? CalendarSchedule.dateOnly(fallbackDate, calendar: calendar)
    }

    public static func startLabel(_ meeting: MeetingContextDTO, now: Date = .now) -> String? {
        guard let start = self.start(meeting), start > now else { return nil }
        return "Starts " + (meeting.allDay ? start.formatted(.dateTime.month(.abbreviated).day()) : start.formatted(date: .omitted, time: .shortened))
    }

    public static func time(_ meeting: MeetingContextDTO) -> String {
        guard let start = self.start(meeting) else { return "Time unavailable" }
        let day = start.formatted(.dateTime.weekday(.abbreviated).month(.abbreviated).day())
        if meeting.allDay { return day + " · All day" }
        let end = CalendarSchedule.timestamp(meeting.endAt)?.formatted(date: .omitted, time: .shortened) ?? ""
        return day + " · " + start.formatted(date: .omitted, time: .shortened) + " – " + end
    }
    /// Imports keep their historical meeting day; a future calendar association
    /// must not move a newly created note into a future recent-notes group.
    public static func timelineDate(noteDate: String, createdAt: String, calendar: Calendar = .current) -> Date? {
        let day = CalendarSchedule.dateOnly(noteDate, calendar: calendar)
        let created = CalendarSchedule.timestamp(createdAt).map { calendar.startOfDay(for: $0) }
        switch (day, created) {
        case let (day?, created?): return min(day, created)
        case let (day?, nil): return day
        case let (nil, created?): return created
        default: return nil
        }
    }

}
