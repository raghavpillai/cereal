import Foundation
import SwiftUI
import Testing
import APIClient
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
        // One separator between two facts on a line, everywhere: the middle dot.
        #expect(NoteSearch.subtitle(date, attendees: 4, calendar: calendar).hasSuffix("17 Nov · 4 attendees"))
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

struct SearchFaceTests {
    @Test func nothingIsSaidAboutASearchBeforeItsAnswer() {
        // Nothing typed, or nothing searchable: the recent searches.
        #expect(SearchFace.showing(query: "", hasHits: false, progress: .idle, settledEmpty: false) == .recent)
        #expect(SearchFace.showing(query: "p", hasHits: false, progress: .idle, settledEmpty: false) == .recent)
        // A first search on its way waits; it does not say "no matching notes".
        #expect(SearchFace.showing(query: "pri", hasHits: false, progress: .searching, settledEmpty: false) == .waiting)
        #expect(SearchFace.showing(query: "pri", hasHits: false, progress: .settled, settledEmpty: false) == .none)
        #expect(SearchFace.showing(query: "pri", hasHits: true, progress: .settled, settledEmpty: false) == .results)
    }

    @Test func noMatchesStaysPutWhileFurtherLettersAreTyped() {
        // The last answer found nothing: the next search leaves the prompt where it is.
        #expect(SearchFace.showing(query: "zzzq", hasHits: false, progress: .searching, settledEmpty: true) == .none)
        // Results stay on screen while a newer search runs, and through a failure.
        #expect(SearchFace.showing(query: "prio", hasHits: true, progress: .searching, settledEmpty: false) == .results)
        #expect(SearchFace.showing(query: "prio", hasHits: true, progress: .failed("x"), settledEmpty: false) == .results)
        #expect(SearchFace.showing(query: "prio", hasHits: false, progress: .failed("x"), settledEmpty: true) == .failed)
    }

    @Test func aResultSaysWhyItWasFoundWhenItsTitleDoesNot() throws {
        let account = AccountID(address: "https://notes.example.com", userID: "u")
        func hit(_ title: String, excerpt: String?, parts: [ServerNoteSearchHit.Part]? = nil) -> SearchHit {
            SearchHit(route: .serverNote(NoteRef(account: account, id: title)), tile: .document, title: title, subtitle: "", excerpt: excerpt, excerptParts: parts)
        }
        // The match is in the title: the highlight there is the reason, and no line is added.
        #expect(NoteSearch.titleShowsMatch("Pricing workshop", query: "pri"))
        #expect(NoteSearch.excerptLine(for: hit("Pricing workshop", excerpt: "Priya to confirm"), query: "pri") == nil)
        // The match is elsewhere: the matching line, with the server's marked words in the primary colour.
        let parts = [ServerNoteSearchHit.Part(text: "Agenda ", isMatch: false), ServerNoteSearchHit.Part(text: "Pri", isMatch: true), ServerNoteSearchHit.Part(text: "ya to confirm", isMatch: false)]
        let line = try #require(NoteSearch.excerptLine(for: hit("Weekly product sync", excerpt: "Agenda Priya to confirm", parts: parts), query: "pri"))
        #expect(String(line.characters) == "Agenda Priya to confirm")
        let marked: [String] = line.runs.filter { $0.foregroundColor != nil }.map { String(line[$0.range].characters) }
        #expect(marked == ["Pri"])
        // A line with nothing marked is still shown; an empty one is not.
        let plain = NoteSearch.excerptLine(for: hit("Board prep", excerpt: "Priya joins"), query: "pri")
        #expect(plain.map { String($0.characters) } == "Priya joins")
        #expect(NoteSearch.excerptLine(for: hit("Board prep", excerpt: ""), query: "pri") == nil)
        #expect(NoteSearch.excerptLine(for: hit("Board prep", excerpt: nil), query: "pri") == nil)
    }
}

struct AccountPagesTests {
    @Test func aWorkspaceNameSuggestsItsSlug() {
        #expect(WorkspaceSlug.suggested(from: "qa space") == "qa-space")
        #expect(WorkspaceSlug.suggested(from: "  Cereal  Team! ") == "cereal-team")
        #expect(WorkspaceSlug.suggested(from: "Café & Crème 2") == "cafe-creme-2")
        #expect(WorkspaceSlug.suggested(from: "") == "")
    }

    @Test func onlyAnAppStorePageIsSharedAsTheApp() {
        #expect(SettingsLinks.appStore("https://apps.apple.com/app/id123")?.host == "apps.apple.com")
        #expect(SettingsLinks.appStore("http://apps.apple.com/app/id123") == nil)
        #expect(SettingsLinks.appStore("https://example.com/app") == nil)
        #expect(SettingsLinks.appStore(nil) == nil)
    }

    @Test func oneNameAndOneWordForEachThing() {
        #expect(Wording.accountSheet == "Account and server")
        #expect(Wording.retry == "Try again")
        #expect(ServerHomeGrouping.subtitle(time: "7:30 PM", attendees: 4) == "7:30 PM · 4 attendees")
        #expect(ServerHomeGrouping.subtitle(time: "7:30 PM", attendees: 1) == "7:30 PM")
        // The server's address is called that in the errors that mention it.
        #expect(APIClientError.transport(.cannotFindHost).localizedDescription.contains("server address"))
        #expect(!APIClientError.invalidInstanceURL.localizedDescription.lowercased().contains("instance"))
        #expect(!APIClientError.unauthorized.localizedDescription.contains("access token"))
    }
}
