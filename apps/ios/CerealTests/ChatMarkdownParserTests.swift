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
}
