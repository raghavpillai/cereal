import Foundation
import Testing
@testable import APIClient

@Suite struct WebAddressesTests {
    private let address = "https://notes.example.com/notes/3a49a4af-c660-42e9-a36a-a806fbb41e0d/chat"

    /// testdata/web-addresses.json: the cases the web reader and the desktop editor run too.
    @Test func theAddressesThatBecomeLinksAreTheSameOnEveryClient() throws {
        var root = URL(fileURLWithPath: #filePath)
        for _ in 0..<7 { root.deleteLastPathComponent() }
        let data = try Data(contentsOf: root.appendingPathComponent("testdata/web-addresses.json"))
        struct Vectors: Decodable { struct Case: Decodable { let text: String; let links: [String] }; let cases: [Case] }
        let vectors = try JSONDecoder().decode(Vectors.self, from: data)
        #expect(vectors.cases.count > 15)
        for item in vectors.cases {
            #expect(WebAddresses.ranges(in: item.text).map { String(item.text[$0.range]) } == item.links, "\(item.text.debugDescription)")
        }
    }

    @Test func theTranscriptChatLineLinksItsAddressAndTheMarkdownAroundItIsRead() throws {
        let line = "**Chat with meeting transcript:** \(address) I CAN TYPE HERE"
        let linked = WebAddresses.markdown(line)
        #expect(String(linked.characters) == "Chat with meeting transcript: \(address) I CAN TYPE HERE")
        let links = linked.runs.compactMap { run in run.link.map { (String(linked[run.range].characters), $0.absoluteString) } }
        #expect(links.map(\.0) == [address])
        #expect(links.map(\.1) == [address])
        // An address with `*` or `_` in it keeps them: it is found before the Markdown is read.
        #expect(WebAddresses.markdown("https://a.example/a*b*c").runs.compactMap(\.link).map(\.absoluteString) == ["https://a.example/a*b*c"])
    }

    @Test func aMarkdownLinkOpensOnlyAnAddressTheRulesAllow() {
        #expect(WebAddresses.markdown("[docs](https://b.example/x)").runs.compactMap(\.link).map(\.absoluteString) == ["https://b.example/x"])
        for unsafe in ["[docs](https://good.example@evil.example/)", "[docs](javascript:alert(1))", "[docs](https://xn--pple-43d.com.evil.example\\@x/)"] {
            #expect(WebAddresses.markdown(unsafe).runs.compactMap(\.link).isEmpty, "\(unsafe)")
        }
        #expect(WebAddresses.markdown("[mail](mailto:a@example.com)").runs.compactMap(\.link).map(\.absoluteString) == ["mailto:a@example.com"])
    }

    @Test func aTranscriptChatLinkOpensItsNote() throws {
        let url = try #require(URL(string: address))
        let link = try #require(AppLink.transcriptChat(url))
        #expect(link.noteID == "3a49a4af-c660-42e9-a36a-a806fbb41e0d")
        #expect(link.transcriptChatURL.absoluteString == address)
        // The same link opened from outside the app (a universal link) opens the note.
        #expect(AppLink(text: address) == .note(link))
        // The note's own page is not its chat, and nothing else after the id is a link.
        #expect(AppLink.transcriptChat(link.webURL) == nil)
        #expect(AppLink(text: address + "/more") == nil)
        #expect(AppLink(text: address.replacingOccurrences(of: "/chat", with: "/edit")) == nil)
    }
}
