import SwiftUI
import APIClient
import Contracts

/// The words for a live stream's state. Nil means nothing needs saying: the
/// stream is working, or it was never available and the recording is simply
/// transcribed from its saved audio as before.
enum LiveStatusText {
    static func line(_ status: LiveTranscriptStatus) -> String? {
        switch status {
        case .off, .connecting, .live, .ended: nil
        case .catchingUp(let lagMs): lagMs >= 10_000 ? "Catching up · about \(lagMs / 1000) s behind" : "Catching up…"
        case .providerUnavailable: "Live transcript paused · audio is still being recorded"
        case .paused: "Paused"
        case .reconnecting: "Reconnecting…"
        case .lowDataMode: "Live transcript is off in Low Data Mode"
        case .unavailable(let reason):
            switch reason {
            case .overLimit, .providerUnavailable: "Live transcript unavailable · text follows from the saved audio"
            case .notConfigured, .unauthorized, .notFound, .notRecording, .replaced, .protocolError: nil
            }
        }
    }

    /// True when text is arriving as it is spoken, or is about to.
    static func isStreaming(_ status: LiveTranscriptStatus) -> Bool {
        switch status {
        case .connecting, .live, .catchingUp, .providerUnavailable, .paused, .reconnecting: true
        case .off, .lowDataMode, .unavailable, .ended: false
        }
    }
}

/// One quiet line for the state of the live stream. It reads the status in its
/// own body, so a change redraws this line and nothing around it.
struct LiveStatusLine: View {
    let live: LiveTranscript
    var font: Font = .footnote
    var color: Color = Theme.textTertiary
    @Environment(\.motion) private var motion

    var body: some View {
        let _ = PerfProbe.hit("LiveStatusLine.body")
        let text = LiveStatusText.line(live.status)
        ZStack {
            if let text {
                Text(text).font(font).foregroundStyle(color).multilineTextAlignment(.center)
                    .transition(motion.fade)
                    .accessibilityIdentifier("live-transcript-status")
            }
        }
        .animation(motion.snappy, value: text)
    }
}

/// The recording note's transcript while it grows: stored text, then the words
/// still being spoken, dimmed until they are final. The view stays at the
/// newest line unless the reader has scrolled up to read something earlier.
struct LiveTranscriptList: View {
    let live: LiveTranscript
    /// False once recording has stopped and only saved audio is still being transcribed.
    var recording = true
    @Environment(\.motion) private var motion
    @State private var followsTail = true
    @State private var readerIsScrolling = false

    var body: some View {
        // This body depends on which lines exist, not on what an interim line
        // says: a revised word redraws that one line (and the follower below).
        let _ = PerfProbe.hit("LiveTranscriptList.body")
        let lines = live.lines
        // With the other side of a call in the note, the microphone is the person recording.
        let microphoneIsMe = TranscriptPresentation.microphoneIsMe(live.segments)
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 20) {
                    if lines.isEmpty { LiveTranscriptPlaceholder(live: live, recording: recording) }
                    ForEach(lines) { line in
                        LiveTranscriptLineView(line: line, microphoneIsMe: microphoneIsMe).id(line.id)
                    }
                    LiveStatusLine(live: live).frame(maxWidth: .infinity, alignment: .leading)
                    // Full width, so scrolling to it never moves the list sideways.
                    Color.clear.frame(maxWidth: .infinity).frame(height: 1).id(Self.bottom)
                }
                .textSelection(.enabled).padding(Theme.gutter).frame(maxWidth: .infinity, alignment: .leading)
            }
            .onScrollPhaseChange { _, phase in
                readerIsScrolling = phase == .tracking || phase == .interacting || phase == .decelerating
            }
            .onScrollGeometryChange(for: Bool.self) { geometry in
                geometry.visibleRect.maxY - geometry.contentInsets.bottom >= geometry.contentSize.height - 48
            } action: { _, nearBottom in
                // Only the reader's own scrolling changes whether the view follows.
                if readerIsScrolling { followsTail = nearBottom }
            }
            .onChange(of: lines.count) {
                if followsTail { withAnimation(motion.snappy) { proxy.scrollTo(Self.bottom, anchor: .bottom) } }
            }
            .background {
                LiveTailFollower(live: live) {
                    if followsTail { proxy.scrollTo(Self.bottom, anchor: .bottom) }
                }
            }
            .overlay(alignment: .bottom) {
                if !followsTail {
                    Button {
                        followsTail = true
                        withAnimation(motion.snappy) { proxy.scrollTo(Self.bottom, anchor: .bottom) }
                    } label: {
                        Image(systemName: "arrow.down").font(.system(size: 15, weight: .semibold))
                            .foregroundStyle(Theme.textPrimary).frame(width: 44, height: 44)
                            .glassSurface(Circle(), interactive: true)
                    }
                    .buttonStyle(.squash)
                    .accessibilityLabel("Scroll to latest text")
                    .accessibilityIdentifier("live-transcript-latest")
                    .padding(.bottom, 16)
                    .transition(motion.pop)
                }
            }
            .animation(motion.animation(.gooey, entering: !followsTail), value: followsTail)
            .onAppear { proxy.scrollTo(Self.bottom, anchor: .bottom) }
        }
        .accessibilityIdentifier("live-transcript")
    }

    private static let bottom = "live-transcript-bottom"
}

/// Follows the growth of interim text so the list can stay at the bottom. An
/// invisible leaf: it is what re-evaluates when words arrive, not the list.
private struct LiveTailFollower: View {
    let live: LiveTranscript
    let grew: () -> Void

    var body: some View {
        let _ = PerfProbe.hit("LiveTailFollower.body")
        Color.clear.frame(width: 0, height: 0)
            .onChange(of: live.utterances.reduce(0) { $0 + $1.text.count }) { grew() }
            .accessibilityHidden(true)
    }
}

/// Shown before the first words of a recording.
private struct LiveTranscriptPlaceholder: View {
    let live: LiveTranscript
    let recording: Bool

    var body: some View {
        Text(text).foregroundStyle(Theme.textSecondary)
    }

    private var text: String {
        guard recording else { return "No transcript is saved for this note." }
        return LiveStatusText.isStreaming(live.status) ? "Listening…" : "Text appears here after each part of the recording is transcribed."
    }
}

/// One transcript line. A stored line is plain values. An interim line reads
/// its utterance's text here, so this is the only body that runs when the
/// provider revises the words.
struct LiveTranscriptLineView: View {
    let line: LiveTranscriptLine
    var microphoneIsMe = false
    @Environment(\.motion) private var motion
    @State private var arrived = false

    var body: some View {
        let _ = PerfProbe.hit("LiveTranscriptLine.body")
        let content = content
        VStack(alignment: .leading, spacing: 6) {
            Text(content.caption).font(.caption).foregroundStyle(Theme.textSecondary)
            // Words replace words with no animation of their own. The one
            // change that is animated is a line brightening as it becomes final.
            Text(content.text).readingFont(.body).contentTransition(.identity)
                .opacity(content.isFinal ? 1 : 0.6)
                .animation(motion.snappy, value: content.isFinal)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        // A line that starts as interim text fades in once. Stored lines, and
        // lines scrolled back into view, are simply there.
        .opacity(arrived || content.isFinal ? 1 : 0)
        .onAppear {
            guard !arrived else { return }
            if content.isFinal { arrived = true } else { withAnimation(motion.snappy) { arrived = true } }
        }
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier(content.isFinal ? "transcript-segment-\(content.segmentID)" : "transcript-interim")
    }

    private var content: (caption: String, text: String, isFinal: Bool, segmentID: String) {
        switch line {
        case .stored(let row, _):
            (row.caption, row.segment.text, row.segment.isFinal, row.segment.id)
        case .interim(let utterance):
            (TranscriptPresentation.interimCaption(channel: utterance.channel, startMs: utterance.startMs, speakerLabel: utterance.speakerLabel,
                                                   microphoneIsMe: microphoneIsMe),
             utterance.text, false, utterance.id)
        }
    }
}
