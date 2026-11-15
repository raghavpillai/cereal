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

    @Test func theComposerBackdropFadesOverTheSameDistanceWithOrWithoutAKeyboard() {
        // Bar 118 pt plus the 18 pt above it; home indicator 34 pt: opaque 71.4 pt down, as before.
        let resting = ChatTail.backdropRamp(height: 136, bottomInset: 34) * (136 + 34)
        #expect(abs(resting - 0.42 * 170) < 0.01)
        // With a 336 pt keyboard the backdrop is much taller, and the fade is still 71.4 pt,
        // not 42% of the whole (198 pt, which is below the composer).
        let typing = ChatTail.backdropRamp(height: 136, bottomInset: 336) * (136 + 336)
        #expect(abs(typing - resting) < 0.01)
        #expect(ChatTail.backdropRamp(height: 0, bottomInset: 0) == 0.42)
    }

    @Test func aShortConversationIsLaidOutAsTallAsItsRoomSoItsEndIsHeldFromTheStart() {
        // 604 pt of scroll view with 152 pt of composer under it leaves 452 pt for the conversation.
        #expect(ChatTail.room(visibleHeight: 604, topInset: 0, bottomInset: 152) == 452)
        // The keyboard takes room; the conversation is never asked to be taller than what is left.
        #expect(ChatTail.room(visibleHeight: 604, topInset: 0, bottomInset: 453) == 151)
        #expect(ChatTail.room(visibleHeight: 100, topInset: 0, bottomInset: 152) == 0)
    }
}
