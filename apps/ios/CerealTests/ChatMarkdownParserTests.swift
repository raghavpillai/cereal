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
}
