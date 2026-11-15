import Foundation
import Testing
@testable import APIClient

private let token = "Zk3vX9aQ2mN7pL5rT8wY1bC4dF6gH0jK-sU_eIoAxyz"
private let noteID = "3a49a4af-c660-42e9-a36a-a806fbb41e0d"

private func shared(_ text: String) -> SharedNoteLink? {
    if case .shared(let link)? = AppLink(text: text) { return link }
    return nil
}

private func note(_ text: String) -> NoteLink? {
    if case .note(let link)? = AppLink(text: text) { return link }
    return nil
}

private func encoded(_ text: String) -> String {
    text.addingPercentEncoding(withAllowedCharacters: .alphanumerics) ?? text
}

@Suite struct AppLinkTests {
    @Test func aSharedWebLinkAndItsAppFormAreTheSameLink() throws {
        let web = try #require(shared("https://notes.example.com/s/\(token)"))
        #expect(web.origin.description == "https://notes.example.com")
        #expect(web.token == token)
        #expect(web.webURL.absoluteString == "https://notes.example.com/s/\(token)")
        #expect(shared("cereal://shared?url=" + encoded("https://notes.example.com/s/\(token)")) == web)
        // What the app writes is what the app reads.
        #expect(AppLink(web.appURL) == .shared(web))
        // A universal link arrives as a URL value.
        #expect(AppLink(try #require(URL(string: "https://notes.example.com/s/\(token)"))) == .shared(web))
    }

    @Test func aNoteWebLinkAndItsAppFormAreTheSameLink() throws {
        let web = try #require(note("https://notes.example.com/notes/\(noteID)"))
        #expect(web.origin.description == "https://notes.example.com")
        #expect(web.noteID == noteID)
        #expect(web.webURL.absoluteString == "https://notes.example.com/notes/\(noteID)")
        #expect(note("cereal://note?server=" + encoded("https://notes.example.com") + "&id=\(noteID)") == web)
        // The identifier is one spelling, however it was written.
        #expect(note("https://notes.example.com/notes/\(noteID.uppercased())") == web)
        #expect(note("cereal://note?id=\(noteID)&server=https://notes.example.com/") == web)
    }

    @Test func theOriginIsNormalisedAndShownAsItsRealName() throws {
        #expect(shared("HTTPS://Notes.Example.COM:443/s/\(token)")?.origin.description == "https://notes.example.com")
        #expect(shared("https://notes.example.com:8443/s/\(token)")?.origin.displayHost == "notes.example.com:8443")
        // A query or fragment picked up along the way is not part of the link.
        #expect(shared("https://notes.example.com/s/\(token)?utm_source=mail#top")?.webURL.absoluteString == "https://notes.example.com/s/\(token)")
        // A look-alike name is kept in the form that shows what it is.
        let lookalike = try #require(URL(string: "https://nоtes.example.com/s/\(token)"))
        if case .shared(let link)? = AppLink(lookalike) {
            #expect(link.origin.host.hasPrefix("xn--"))
            #expect(link.origin.host.utf8.allSatisfy { $0 < 0x80 })
        }
    }

    @Test func plainHTTPIsOnlyForThisDevice() {
        #expect(shared("http://127.0.0.1:3000/s/\(token)")?.origin.description == "http://127.0.0.1:3000")
        #expect(shared("http://localhost:3000/s/\(token)")?.origin.description == "http://localhost:3000")
        #expect(shared("http://[::1]:3000/s/\(token)")?.origin.description == "http://[::1]:3000")
        #expect(note("cereal://note?server=" + encoded("http://127.0.0.1:3000") + "&id=\(noteID)")?.origin.displayHost == "127.0.0.1:3000")
        for refused in ["http://notes.example.com/s/\(token)", "http://127.0.0.1.example.com/s/\(token)",
                        "http://localhost.example.com/s/\(token)", "http://192.168.1.10/s/\(token)", "http://127.0.0.2/s/\(token)"] {
            #expect(AppLink(text: refused) == nil, "\(refused)")
            #expect(AppLink(text: "cereal://shared?url=" + encoded(refused)) == nil, "\(refused)")
        }
        #expect(AppLink(text: "cereal://note?server=" + encoded("http://notes.example.com") + "&id=\(noteID)") == nil)
    }

    @Test func otherSchemesAreNotLinks() {
        for refused in ["ftp://notes.example.com/s/\(token)", "file:///s/\(token)", "javascript:alert(1)",
                        "data:text/html,hello", "notes.example.com/s/\(token)", "//notes.example.com/s/\(token)", "", "   ",
                        "cereal:shared?url=https://notes.example.com/s/\(token)"] {
            #expect(AppLink(text: refused) == nil, "\(refused)")
        }
        for inner in ["ftp://notes.example.com/s/\(token)", "file:///s/\(token)", "javascript:alert(1)"] {
            #expect(AppLink(text: "cereal://shared?url=" + encoded(inner)) == nil, "\(inner)")
        }
    }

    @Test func unknownAppActionsAndLiveActivityLinksAreLeftAlone() {
        for other in ["cereal://recording/\(noteID)", "cereal://live-activity-preview", "cereal://open?url=https://notes.example.com/s/\(token)",
                      "cereal://settings", "cereal://", "cereal://shared", "cereal://note", "cereal://shared/extra?url=https://notes.example.com/s/\(token)",
                      "cereal://user@shared?url=https://notes.example.com/s/\(token)", "cereal://shared:80?url=https://notes.example.com/s/\(token)"] {
            #expect(AppLink(text: other) == nil, "\(other)")
        }
    }

    @Test func nestedAndRepeatedEncodingsAreRefused() {
        let web = "https://notes.example.com/s/\(token)"
        // Encoded twice: after the one decoding it is still not a URL.
        #expect(AppLink(text: "cereal://shared?url=" + encoded(encoded(web))) == nil)
        // An app link inside an app link.
        #expect(AppLink(text: "cereal://shared?url=" + encoded("cereal://shared?url=" + encoded(web))) == nil)
        // A note link where a shared link belongs.
        #expect(AppLink(text: "cereal://shared?url=" + encoded("https://notes.example.com/notes/\(noteID)")) == nil)
        // Two answers to one question.
        #expect(AppLink(text: "cereal://shared?url=" + encoded(web) + "&url=" + encoded("https://other.example.com/s/\(token)")) == nil)
        #expect(AppLink(text: "cereal://note?server=https://a.example.com&server=https://b.example.com&id=\(noteID)") == nil)
        // An escaped character anywhere in the path.
        #expect(AppLink(text: "https://notes.example.com/s/\(token.dropLast(1))%41") == nil)
        #expect(AppLink(text: "https://notes.example.com/%73/\(token)") == nil)
        #expect(AppLink(text: "https://notes.example.com/s%2F\(token)") == nil)
        // An escaped host.
        #expect(AppLink(text: "https://notes.example%2Ecom/s/\(token)") == nil)
        #expect(AppLink(text: "cereal://note?server=" + encoded(encoded("https://notes.example.com")) + "&id=\(noteID)") == nil)
    }

    @Test func pathTraversalAndOtherRoutesAreRefused() {
        for refused in ["https://notes.example.com/s/../api/v1/notes", "https://notes.example.com/s/\(token)/../../admin",
                        "https://notes.example.com/s/..", "https://notes.example.com/s/%2e%2e/admin", "https://notes.example.com/s/\(token)/",
                        "https://notes.example.com//s/\(token)", "https://notes.example.com/x/s/\(token)", "https://notes.example.com/s/\(token)/more",
                        "https://notes.example.com/api/v1/shared/\(token)", "https://notes.example.com/S/\(token)", "https://notes.example.com/s/",
                        "https://notes.example.com/notes/", "https://notes.example.com/notes/../admin", "https://notes.example.com/notes/\(noteID)/edit",
                        "https://notes.example.com/notes/not-a-note", "https://notes.example.com/notes/\(noteID)0",
                        "https://notes.example.com\\@evil.example/s/\(token)", "https://notes.example.com/s\\\(token)"] {
            #expect(AppLink(text: refused) == nil, "\(refused)")
            #expect(AppLink(text: "cereal://shared?url=" + encoded(refused)) == nil, "\(refused)")
        }
        #expect(AppLink(text: "cereal://note?server=https://notes.example.com&id=../admin") == nil)
        #expect(AppLink(text: "cereal://note?server=https://notes.example.com/base&id=\(noteID)") == nil)
        #expect(AppLink(text: "cereal://note?server=https://notes.example.com?x=1&id=\(noteID)") == nil)
    }

    @Test func tokensOfUnreasonableLengthOrAlphabetAreRefused() {
        #expect(shared("https://notes.example.com/s/" + String(repeating: "a", count: 16)) != nil)
        #expect(shared("https://notes.example.com/s/" + String(repeating: "a", count: 128)) != nil)
        #expect(AppLink(text: "https://notes.example.com/s/" + String(repeating: "a", count: 15)) == nil)
        #expect(AppLink(text: "https://notes.example.com/s/" + String(repeating: "a", count: 129)) == nil)
        #expect(AppLink(text: "https://notes.example.com/s/" + String(repeating: "a", count: 5000)) == nil)
        #expect(AppLink(text: "https://notes.example.com/s/\(token.dropLast(1))!") == nil)
        #expect(AppLink(text: "https://notes.example.com/s/\(token.dropLast(1)).") == nil)
        #expect(AppLink(text: "https://notes.example.com/s/\(token.dropLast(1))é") == nil)
        // The whole link has a limit too, wherever the length is.
        #expect(AppLink(text: "https://notes.example.com/s/\(token)?x=" + String(repeating: "a", count: 3000)) == nil)
    }

    @Test func hostsThatAreNotPlainNamesAreRefused() {
        for refused in ["https://user:secret@notes.example.com/s/\(token)", "https://notes.example.com@evil.example/s/\(token)",
                        "https:///s/\(token)", "https://notes..example.com/s/\(token)", "https://-notes.example.com/s/\(token)",
                        "https://notes.example.com:0/s/\(token)", "https://notes.example.com:99999/s/\(token)",
                        "https://notes_example.com/s/\(token)", "https://notes.example.com./s/\(token)", "https://notes example.com/s/\(token)",
                        "https://notes.example.com\n/s/\(token)", "https://notes.example.com\t/s/\(token)"] {
            #expect(AppLink(text: refused) == nil, "\(refused)")
        }
        // Surrounding white space is the only white space a pasted link may have.
        #expect(shared("  https://notes.example.com/s/\(token)\n") != nil)
    }

    @Test func aSignInAnswerCarriesItsCodeStateAndServer() throws {
        guard case .auth(let answer)? = AppLink(text: "cereal://auth?code=abc-123_DEF&state=st4te&server=" + encoded("https://notes.example.com")) else {
            Issue.record("not an answer"); return
        }
        #expect(answer.code == "abc-123_DEF")
        #expect(answer.state == "st4te")
        #expect(answer.origin?.description == "https://notes.example.com")
        guard case .auth(let bare)? = AppLink(text: "cereal://auth?code=abc&state=st4te") else { Issue.record("not an answer"); return }
        #expect(bare.origin == nil)
        for refused in ["cereal://auth?code=abc", "cereal://auth?state=st4te", "cereal://auth?code=&state=st4te", "cereal://auth?code=a&code=b&state=st4te",
                        "cereal://auth?code=abc&state=st4te&server=http://notes.example.com", "cereal://auth?code=abc&state=st4te&server=nonsense",
                        "cereal://auth?code=" + String(repeating: "a", count: 1025) + "&state=st4te", "https://notes.example.com/auth?code=abc&state=st4te"] {
            #expect(AppLink(text: refused) == nil, "\(refused)")
        }
    }

    @Test func aNoteLinkIsOnTheSignedInServerOnlyWhenTheOriginIsTheSame() throws {
        let link = try #require(note("https://notes.example.com/notes/\(noteID)"))
        #expect(link.origin.isInstance(try InstanceURL("https://notes.example.com")))
        #expect(link.origin.isInstance(try InstanceURL("https://NOTES.example.com:443/")))
        #expect(!link.origin.isInstance(try InstanceURL("https://other.example.com")))
        #expect(!link.origin.isInstance(try InstanceURL("https://notes.example.com:8443")))
        #expect(!link.origin.isInstance(try InstanceURL("https://notes.example.com/base")))
        let local = try #require(note("http://127.0.0.1:3000/notes/\(noteID)"))
        #expect(local.origin.isInstance(try InstanceURL("http://127.0.0.1:3000")))
        #expect(!local.origin.isInstance(try InstanceURL("http://127.0.0.1:3100")))
        #expect(!local.origin.isInstance(try InstanceURL("http://localhost:3000")))
    }
}

@Suite @MainActor struct SharedNoteHistoryTests {
    private func defaults() -> UserDefaults {
        let name = "shared-history-" + UUID().uuidString
        let defaults = UserDefaults(suiteName: name)!
        defaults.removePersistentDomain(forName: name)
        return defaults
    }

    private func link(_ index: Int, host: String = "notes.example.com") throws -> SharedNoteLink {
        try #require(shared("https://\(host)/s/" + String(format: "%043d", index)))
    }

    @Test func openedNotesAreKeptNewestFirstAndSurviveARelaunch() throws {
        let defaults = defaults()
        let history = SharedNoteHistory(defaults: defaults)
        #expect(history.visits.isEmpty)
        let first = Date(timeIntervalSince1970: 1_800_000_000)
        history.record(try link(1), title: "Launch plan", at: first)
        history.record(try link(2, host: "other.example.org:8443"), title: "Budget", at: first.addingTimeInterval(60))
        #expect(history.visits.map(\.title) == ["Budget", "Launch plan"])
        #expect(history.visits.map(\.host) == ["other.example.org:8443", "notes.example.com"])

        let relaunched = SharedNoteHistory(defaults: defaults)
        #expect(relaunched.visits == history.visits)
        #expect(relaunched.visits.last?.openedAt == first)
        #expect(relaunched.visits.last?.id == "https://notes.example.com/s/" + String(format: "%043d", 1))
    }

    @Test func openingANoteAgainMovesItToTheTopWithItsNewTitle() throws {
        let history = SharedNoteHistory(defaults: defaults())
        history.record(try link(1), title: "Old title", at: Date(timeIntervalSince1970: 10))
        history.record(try link(2), title: "Other", at: Date(timeIntervalSince1970: 20))
        history.record(try link(1), title: "New title", at: Date(timeIntervalSince1970: 30))
        #expect(history.visits.map(\.title) == ["New title", "Other"])
        #expect(history.visits.first?.openedAt == Date(timeIntervalSince1970: 30))
    }

    @Test func aNoteCanBeRemovedAndStaysRemoved() throws {
        let defaults = defaults()
        let history = SharedNoteHistory(defaults: defaults)
        history.record(try link(1), title: "One")
        history.record(try link(2), title: "Two")
        history.remove(try link(1).webURL.absoluteString)
        #expect(history.visits.map(\.title) == ["Two"])
        #expect(SharedNoteHistory(defaults: defaults).visits.map(\.title) == ["Two"])
        history.remove("not there")
        #expect(history.visits.count == 1)
    }

    @Test func onlyTheFiftyMostRecentAreKept() throws {
        let defaults = defaults()
        let history = SharedNoteHistory(defaults: defaults)
        for index in 1...60 { history.record(try link(index), title: "Note \(index)", at: Date(timeIntervalSince1970: Double(index))) }
        #expect(history.visits.count == SharedNoteHistory.limit)
        #expect(history.visits.first?.title == "Note 60")
        #expect(history.visits.last?.title == "Note 11")
        #expect(SharedNoteHistory(defaults: defaults).visits.count == 50)
    }

    @Test func savedEntriesAreReadAsStrictlyAsLinksFromOutside() throws {
        let defaults = defaults()
        let good = "https://notes.example.com/s/" + String(format: "%043d", 7)
        let entries: [[String: Any]] = [
            ["url": "http://evil.example/s/" + String(format: "%043d", 1), "title": "Plain HTTP", "openedAt": 1.0],
            ["url": "cereal://recording/abc", "title": "Not a share", "openedAt": 2.0],
            ["url": good, "title": String(repeating: "t", count: 900), "openedAt": 3.0]
        ]
        defaults.set(try JSONSerialization.data(withJSONObject: entries), forKey: "cereal.sharedWithMe")
        let history = SharedNoteHistory(defaults: defaults)
        #expect(history.visits.map(\.id) == [good])
        #expect(history.visits.first?.title.count == 500)

        defaults.set(Data("not json".utf8), forKey: "cereal.sharedWithMe")
        #expect(SharedNoteHistory(defaults: defaults).visits.isEmpty)
    }
}
