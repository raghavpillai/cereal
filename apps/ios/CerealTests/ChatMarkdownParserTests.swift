import Testing
@testable import Cereal

struct ChatMarkdownParserTests {
    @Test func preservesNestedActionsNumbersAndUnicode() {
        #expect(ChatMarkdownParser.blocks("### Decisions\n- Ship Monday\n  * Review café\n1. Ask Alex\n  2) Test 🦊") == [
            .heading("Decisions"), .list(marker: "•", text: "Ship Monday", depth: 0),
            .list(marker: "•", text: "Review café", depth: 1), .list(marker: "1.", text: "Ask Alex", depth: 0),
            .list(marker: "2)", text: "Test 🦊", depth: 1)
        ])
    }
    @Test func anUnfinishedStreamKeepsCodeLiteralInsteadOfFormattingItsMarkers() {
        #expect(ChatMarkdownParser.blocks("```swift\n# literal\n- source") == [.code("# literal\n- source")])
        #expect(ChatMarkdownParser.blocks("> exact quote\n\nPlain text") == [.quote("exact quote"), .space, .paragraph("Plain text")])
    }
    @Test func aPipeTableIsOneBlockWithItsHeaderAndRows() {
        #expect(ChatMarkdownParser.blocks("Before\n| Item | State |\n|---|:--:|\n| Staging | **Pending** |\n| Owner | Jürgen |\nAfter") == [
            .paragraph("Before"),
            .table(header: ["Item", "State"], rows: [["Staging", "**Pending**"], ["Owner", "Jürgen"]]),
            .paragraph("After")
        ])
        // While it streams, the first row is shown as a row until the rule under it arrives.
        #expect(ChatMarkdownParser.blocks("| Item | State |") == [.table(header: [], rows: [["Item", "State"]])])
        // A table inside a code fence stays literal.
        #expect(ChatMarkdownParser.blocks("```\n| a | b |\n```") == [.code("| a | b |")])
    }
    @Test func anIndentedLineUnderAnItemContinuesIt() {
        // A next step with its deadline beneath, as generated notes write them.
        #expect(ChatMarkdownParser.blocks("# Next Steps\n- Send the spec (Owen)\n  Due Friday, so engineering can scope it.\n1. Book training (Priya)\n   Aim for the 14th.\n  - Night shift\n    Both leads asked.") == [
            .heading("Next Steps"),
            .list(marker: "•", text: "Send the spec (Owen)", depth: 0),
            .continuation(marker: "•", text: "Due Friday, so engineering can scope it.", depth: 0),
            .list(marker: "1.", text: "Book training (Priya)", depth: 0),
            .continuation(marker: "1.", text: "Aim for the 14th.", depth: 0),
            .list(marker: "•", text: "Night shift", depth: 1),
            .continuation(marker: "•", text: "Both leads asked.", depth: 1)
        ])
        // Indented text after a gap, or under no item, stays a paragraph; a note that is one plain sentence is that sentence.
        #expect(ChatMarkdownParser.blocks("- Item\n\n  Later") == [.list(marker: "•", text: "Item", depth: 0), .space, .paragraph("  Later")])
        #expect(ChatMarkdownParser.blocks("Looks like this was just a microphone test, nothing to summarize yet.") == [.paragraph("Looks like this was just a microphone test, nothing to summarize yet.")])
    }
}
