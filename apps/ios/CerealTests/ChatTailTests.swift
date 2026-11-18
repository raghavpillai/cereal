import CoreGraphics
import Testing
@testable import Cereal

/// Staying at the end of a conversation, and when the way back to it is offered.
struct ChatTailTests {
    @Test func aReaderAtTheEndIsStillFollowingWhenTheAnswerGrowsUnderThem() {
        // Text, source chips and the action row make the conversation taller between two
        // scroll events. That is not the reader leaving: nothing changes until they scroll.
        #expect(ChatTail.follows(current: true, readerIsScrolling: false, nearEnd: false))
        #expect(ChatTail.follows(current: false, readerIsScrolling: false, nearEnd: true) == false)
        // The reader's own scrolling decides.
        #expect(ChatTail.follows(current: true, readerIsScrolling: true, nearEnd: false) == false)
        #expect(ChatTail.follows(current: false, readerIsScrolling: true, nearEnd: true))
    }

    @Test func theJumpControlIsOfferedOnlyAfterTheReaderHasScrolledAway() {
        #expect(!ChatTail.showsJumpToLatest(hasMessages: true, followsTail: true))
        #expect(ChatTail.showsJumpToLatest(hasMessages: true, followsTail: false))
        #expect(!ChatTail.showsJumpToLatest(hasMessages: false, followsTail: false))
    }

    @Test func beingWithinAShortDistanceOfTheEndCountsAsTheEnd() {
        // 600 pt of conversation, 100 pt of composer and keyboard under the viewport.
        #expect(ChatTail.isNearEnd(visibleMaxY: 700, bottomInset: 100, contentHeight: 600))
        #expect(ChatTail.isNearEnd(visibleMaxY: 680, bottomInset: 100, contentHeight: 600))  // 20 pt short
        #expect(!ChatTail.isNearEnd(visibleMaxY: 670, bottomInset: 100, contentHeight: 600)) // 30 pt short
    }

    @Test func whileTheJumpControlIsNotOfferedTheEndOfTheAnswerIsClearOfTheComposer() {
        // The last line sits 22 pt of row spacing plus the end space above the composer.
        // A reader counted as at the end is at most `nearEnd` short of it, so the line is still clear.
        let roomUnderLastLine = 22 + ChatTail.endSpace
        #expect(ChatTail.nearEnd < roomUnderLastLine)
        #expect(roomUnderLastLine - ChatTail.nearEnd >= 12)
    }

    @Test func theFadeUnderTheJumpControlCoversItsWholeZone() {
        // 8 pt gap, the 36 pt circle, 8 pt above it: text fades out before any of that.
        #expect(ChatTail.latestZone == 52)
        #expect(ChatColumn.latestGap == 8)
    }

    @Test func aConversationRestingAtItsEndIsClearOfTheFadeAboveTheComposer() {
        // 22 pt between rows plus the end space: the last line sits above the fade, not in it.
        #expect(22 + ChatTail.endSpace >= ChatTail.fade)
    }

    @Test func aShortConversationIsLaidOutAsTallAsItsRoomSoItsEndIsHeldFromTheStart() {
        // 604 pt of scroll view with 152 pt of composer under it leaves 452 pt for the conversation.
        #expect(ChatTail.room(visibleHeight: 604, topInset: 0, bottomInset: 152) == 452)
        // The keyboard takes room; the conversation is never asked to be taller than what is left.
        #expect(ChatTail.room(visibleHeight: 604, topInset: 0, bottomInset: 453) == 151)
        #expect(ChatTail.room(visibleHeight: 100, topInset: 0, bottomInset: 152) == 0)
    }
}
