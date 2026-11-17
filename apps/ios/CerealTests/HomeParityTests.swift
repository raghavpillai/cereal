import Foundation
import SwiftUI
import Testing
@testable import Cereal

/// Search, recent searches and Settings, as in the reference recordings.
@MainActor struct HomeParityTests {
    @Test func searchMarksEveryMatchedWordInATitleAndNothingElse() {
        let marked = NoteSearch.markedTitle("Lumen pitch: the pitch deck", query: "PITCH")
        #expect(String(marked.characters) == "Lumen pitch: the pitch deck")
        let lit = marked.runs.filter { $0.backgroundColor != nil }.map { String(marked[$0.range].characters) }
        #expect(lit == ["pitch", "pitch"])
        #expect(NoteSearch.markedTitle("Weekly sync", query: "   ").runs.allSatisfy { $0.backgroundColor == nil })
    }

    @Test func searchResultsSayTheirDayAndWhoWasInvited() throws {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = TimeZone(identifier: "UTC")!
        let date = try #require(Date.serverTimestamp("\(calendar.component(.year, from: .now))-11-17T09:00:00Z"))
        #expect(NoteSearch.subtitle(date, attendees: 4, calendar: calendar).hasSuffix("17 Nov  •  4 attendees"))
        #expect(NoteSearch.subtitle(date, attendees: 1, calendar: calendar).hasSuffix("17 Nov"))
    }

    @Test func recentSearchesAreNewestFirstWithoutRepeats() throws {
        let name = "recent-\(UUID().uuidString)"
        let defaults = try #require(UserDefaults(suiteName: name))
        defer { defaults.removePersistentDomain(forName: name) }
        for query in ["notes", "roadmap", " Notes ", "", "a", "b", "c", "d", "e"] { RecentSearchStore.remember(query, in: defaults) }
        let recent = RecentSearchStore.load(defaults)
        #expect(recent.first == "e" && recent.count == RecentSearchStore.limit)
        #expect(recent.filter { $0.lowercased() == "notes" }.count == 1)
    }
}
