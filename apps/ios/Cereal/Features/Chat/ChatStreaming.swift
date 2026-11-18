import Foundation
import SwiftUI

/// Collects streamed text and hands it on at most once per interval, so a
/// reply costs one screen update per frame or so instead of one per token.
/// Nothing is dropped: `flushNow()` delivers whatever is waiting.
@MainActor final class ChatDeltaBuffer {
    private var pending = ""
    private var timer: Task<Void, Never>?
    private let interval: Duration
    private let deliver: (String) -> Void
    /// Number of times text was handed on. For tests and the debug probe.
    private(set) var deliveries = 0

    init(interval: Duration = .milliseconds(33), deliver: @escaping (String) -> Void) {
        self.interval = interval
        self.deliver = deliver
    }

    func append(_ text: String) {
        guard !text.isEmpty else { return }
        pending += text
        guard timer == nil else { return }
        timer = Task { [weak self, interval] in
            do { try await Task.sleep(for: interval) } catch { return }
            self?.timer = nil
            self?.flushNow()
        }
    }

    func flushNow() {
        timer?.cancel()
        timer = nil
        guard !pending.isEmpty else { return }
        let text = pending
        pending = ""
        deliveries += 1
        PerfProbe.hit("Chat.publish")
        Signpost.event("Chat.Delta")
        deliver(text)
    }
}

/// When a chat screen goes off screen. A stream is cancelled only by Stop, by
/// a new question, or by leaving the chat; opening one of the answer's
/// sources covers the chat without removing it and must not cancel.
enum ChatStreamPolicy {
    static func shouldCancelOnDisappear(path: [Route], route: Route) -> Bool {
        !path.contains(route)
    }
}

/// Parsed Markdown for chat replies. While a reply streams only its last
/// block changes, so every earlier block, and every earlier reply, is served
/// from here instead of being parsed again on each update.
enum ChatMarkdownCache {
    private final class Box<T>: NSObject {
        let value: T
        init(_ value: T) { self.value = value }
    }

    nonisolated(unsafe) private static let inlineCache: NSCache<NSString, Box<AttributedString>> = {
        let cache = NSCache<NSString, Box<AttributedString>>()
        cache.countLimit = 2_000
        return cache
    }()
    nonisolated(unsafe) private static let blockCache: NSCache<NSString, Box<[ChatMarkdownBlock]>> = {
        let cache = NSCache<NSString, Box<[ChatMarkdownBlock]>>()
        cache.countLimit = 64
        return cache
    }()

    /// `context` distinguishes turns and citation sets, which change how links resolve.
    static func inline(_ source: String, context: String, build: () -> AttributedString) -> AttributedString {
        let key = (context + "\u{1F}" + source) as NSString
        if let hit = inlineCache.object(forKey: key) { return hit.value }
        PerfProbe.hit("Markdown.attributedBuild")
        let value = build()
        inlineCache.setObject(Box(value), forKey: key)
        return value
    }

    static func blocks(_ text: String) -> [ChatMarkdownBlock] {
        let key = text as NSString
        if let hit = blockCache.object(forKey: key) { return hit.value }
        PerfProbe.hit("Markdown.blockParse")
        let value = ChatMarkdownParser.blocks(text)
        blockCache.setObject(Box(value), forKey: key)
        return value
    }

    static func removeAll() {
        inlineCache.removeAllObjects()
        blockCache.removeAllObjects()
    }
}

/// The message column sits 16 pt from the leading edge of its scroll view and
/// 8 pt from the trailing edge.
enum ChatColumn {
    static let leading: CGFloat = Theme.gutter
    static let trailing: CGFloat = 8
    /// Space between the composer and the jump-to-latest control above it.
    static let latestGap: CGFloat = 8
}

extension UnitPoint {
    /// Anchor for scrolling to the end-of-conversation marker. `scrollTo`
    /// lines the marker up on both axes, and the marker is as wide as the
    /// off-centre column, so `.bottom` (x 0.5) left a conversation shorter than
    /// the screen scrolled 4 pt sideways. This x keeps the horizontal offset at zero.
    static let chatEnd = UnitPoint(x: ChatColumn.leading / (ChatColumn.leading + ChatColumn.trailing), y: 1)
}

/// Staying at the end of a conversation, as rules that can be tested.
///
/// A reader who is at the end stays there while an answer arrives, and ends
/// with all of it above the composer: its text, its source chips and the row
/// of actions that appears when it finishes. A reader who scrolled up to read
/// something is left where they are and offered a way back.
enum ChatTail {
    /// Within this distance of the end counts as being at the end. It is less
    /// than the room under the last line (`endSpace`), so while the control is
    /// not offered the end of the answer is still clear of the composer.
    static let nearEnd: CGFloat = 22
    /// Empty space that ends the conversation, and the row every scroll to the
    /// end aims at. With the 22 pt between rows it is the gap between the last
    /// line and the composer. There is one resting place at the end: where
    /// the reader's own scrolling stops is where a jump or a new answer lands.
    static let endSpace: CGFloat = 13
    /// Room the jump-to-latest control takes above the composer: its gap, its
    /// 36 pt circle and the same gap again above it.
    static let latestZone: CGFloat = ChatColumn.latestGap + 36 + ChatColumn.latestGap
    /// Height over which the conversation fades out above the composer. The last
    /// line rests 35 pt above it (22 between rows plus `endSpace`), clear of the fade.
    static let fade: CGFloat = 20
    /// Height of the fade under the chat title: the conversation's own top padding, so a
    /// conversation resting at its start is not dimmed.
    static let topFade: CGFloat = 8

    static func isNearEnd(visibleMaxY: CGFloat, bottomInset: CGFloat, contentHeight: CGFloat) -> Bool {
        visibleMaxY - bottomInset >= contentHeight - nearEnd
    }

    /// Only the reader's own scrolling changes whether the conversation follows.
    /// Text arriving, the keyboard and programmatic scrolling leave it as it was.
    static func follows(current: Bool, readerIsScrolling: Bool, nearEnd: Bool) -> Bool {
        readerIsScrolling ? nearEnd : current
    }


    /// Height the conversation has to itself: the scroll view less what its bars cover.
    static func room(visibleHeight: CGFloat, topInset: CGFloat, bottomInset: CGFloat) -> CGFloat {
        max(0, (visibleHeight - topInset - bottomInset).rounded(.down))
    }

    /// The control is offered when the reader has left the end, never because
    /// the conversation grew underneath a reader who is following it.
    static func showsJumpToLatest(hasMessages: Bool, followsTail: Bool) -> Bool {
        hasMessages && !followsTail
    }
}

extension View {
    /// Keeps a conversation at its end while `follows` is true, whatever makes
    /// it taller or its room smaller: streamed text, source chips, the actions
    /// under a finished answer, or the keyboard.
    ///
    /// The scroll view does this itself, by holding its bottom edge still when a
    /// size changes. That needs two things from the conversation. Its height has
    /// to be exact, so it is not a lazy stack (see `ServerChatView`). And it has
    /// to reach the bottom of its room: one shorter than that stayed at its top
    /// when it outgrew the screen (seen on a folded Duo), so `room` is measured
    /// here and the conversation is laid out at least that tall with
    /// `fillsChatRoom`.
    ///
    /// Scrolling from a geometry callback whenever the end moved away was tried
    /// first and must not come back while any row is lazy: each revision of the
    /// list's guessed height looked like the end moving, the scroll that answered
    /// it caused the next revision, and the main thread never came out of layout.
    func followsChatTail(_ follows: Bool, room: Binding<CGFloat>) -> some View {
        defaultScrollAnchor(follows ? .bottom : nil, for: .sizeChanges)
            .onScrollGeometryChange(for: CGFloat.self) { geometry in
                ChatTail.room(visibleHeight: geometry.visibleRect.height, topInset: geometry.contentInsets.top,
                              bottomInset: geometry.contentInsets.bottom)
            } action: { _, height in room.wrappedValue = height }
    }

    /// The conversation's rows, laid out at least as tall as the room above the
    /// composer, with the rows at the top. See `followsChatTail`.
    func fillsChatRoom(_ room: CGFloat) -> some View {
        frame(minHeight: room, alignment: .topLeading)
    }

    /// At the top of the conversation: lines fade out under the title instead of
    /// being cut through the middle of their letters.
    func chatTopFade() -> some View {
        overlay(alignment: .top) {
            LinearGradient(colors: [Theme.bgTop, Theme.bgTop.opacity(0)], startPoint: .top, endPoint: .bottom)
                .frame(height: ChatTail.topFade)
                .allowsHitTesting(false)
        }
    }

    /// Behind the composer: messages fade as they pass under its top edge and
    /// are never visible beside or below it. While the jump-to-latest control
    /// is showing (`raised`), the fade starts above that control instead, so
    /// the control never lies on text.
    func chatBarBackdrop(raised: Bool = false) -> some View {
        background(alignment: .top) {
            if raised {
                VStack(spacing: 0) {
                    LinearGradient(colors: [Theme.bgTop.opacity(0), Theme.bgTop], startPoint: .top, endPoint: .bottom)
                        .frame(height: ChatTail.fade)
                    // Down to where the composer's own backdrop is opaque.
                    Theme.bgTop.frame(height: ChatTail.latestZone + 60)
                }
                .fixedSize(horizontal: false, vertical: true)
                .offset(y: -(ChatTail.latestZone + ChatTail.fade))
                .padding(.horizontal, -40)
                .allowsHitTesting(false)
                .transition(.opacity)
            }
        }
        .background {
            // The conversation fades out over `ChatTail.fade` just above the bar, so no
            // line runs into the composer's edge, and the page is solid from there down
            // to the screen's edge (or the keyboard) beside and below the composer.
            VStack(spacing: 0) {
                LinearGradient(colors: [Theme.bgTop.opacity(0), Theme.bgTop], startPoint: .top, endPoint: .bottom)
                    .frame(height: ChatTail.fade)
                Theme.bgTop
            }
            .padding(.top, -ChatTail.fade)
            .padding(.horizontal, -40)
            .ignoresSafeArea(edges: .bottom)
            .allowsHitTesting(false)
        }
    }
}
