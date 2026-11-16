import Foundation
import Contracts

/// Who a line of a transcript is shown as belonging to, and when it was said.
///
/// A speaker the server named keeps that name. A voice it did not name is the
/// person's own when it came through the microphone of a recording that also
/// has the other side of a call (a desktop recording: the microphone is the
/// person, the system audio is everyone else). In a recording of a room, where
/// one microphone hears several people, an unnamed voice is "Speaker", with a
/// number when there is more than one of them. A live voice (`mic:n:N`,
/// `system:n:N`) the server has not labelled yet is given the label the server
/// gives it: "Me" for the microphone's first voice, "Room speaker N" for its
/// others, "Speaker N" for the other side of a call. Which audio a line came from,
/// and which part of the recording, is not shown.
public enum TranscriptPresentation {
    public struct Row: Identifiable, Sendable {
        public let segment: TranscriptSegmentDTO
        public let speaker: String
        public var id: String { segment.id }
        /// Speaker and time: "Speaker 3 · 1:10".
        public var caption: String {
            [speaker, TranscriptPresentation.time(segment.startMs), segment.isFinal ? nil : "Interim"].compactMap { $0 }.joined(separator: " · ")
        }
    }

    /// Caption of text that is still being spoken. The speaker is a draft from
    /// the provider and is left out when it has none yet, unless the words
    /// came through a microphone that is the person's own.
    public static func interimCaption(channel: AudioChannel, startMs: Int, speakerLabel: String?, microphoneIsMe: Bool = false) -> String {
        let label = speakerLabel?.trimmingCharacters(in: .whitespacesAndNewlines)
        let speaker = label?.isEmpty == false ? label : channel == .mic && microphoneIsMe ? me : nil
        return [speaker, time(startMs), "Interim"].compactMap { $0 }.joined(separator: " · ")
    }

    /// True for a recording that has the other side of a call beside the
    /// microphone. Its microphone is the person who recorded it.
    public static func microphoneIsMe(_ segments: [TranscriptSegmentDTO]) -> Bool {
        segments.contains { $0.channel == .system }
    }

    static let me = "Me"
    static let speaker = "Speaker"
    /// The label the server gives a live speaker (`<channel>:n:<number>`), for a
    /// line it has not labelled yet: the microphone's first voice is "Me", its
    /// others are "Room speaker 1", "Room speaker 2"…; the other side of a call is
    /// "Speaker N". Nil for any other id.
    static func liveLabel(_ speakerID: String) -> String? {
        let parts = speakerID.split(separator: ":")
        guard parts.count == 3, parts[1] == "n", let number = Int(parts[2]), number >= 1 else { return nil }
        switch parts[0] {
        case "mic": return number == 1 ? me : "Room speaker \(number - 1)"
        case "system": return "\(speaker) \(number)"
        default: return nil
        }
    }

    public static func rows(_ segments: [TranscriptSegmentDTO]) -> [Row] {
        // Keep equal-time sources in their server order and retain every utterance.
        let ordered = segments.enumerated().sorted {
            $0.element.startMs == $1.element.startMs ? $0.offset < $1.offset : $0.element.startMs < $1.element.startMs
        }.map(\.element)
        let microphoneIsMe = microphoneIsMe(segments)

        // The unnamed voices, in the order they first speak. The same voice in a
        // later part of the recording has the same key and so keeps its number.
        var voices: [VoiceKey] = []
        var named: Set<String> = []
        for segment in ordered {
            if let label = label(segment) { named.insert(label); continue }
            guard !(segment.channel == .mic && microphoneIsMe), liveLabel(segment.speakerId) == nil, let key = voice(segment), !voices.contains(key) else { continue }
            voices.append(key)
        }
        // A number is only needed to tell unnamed voices apart, and never one
        // that the server has already given to a speaker it named.
        var numbers: [VoiceKey: Int] = [:]
        if voices.count > 1 {
            var next = 1
            for key in voices {
                while named.contains("\(speaker) \(next)") { next += 1 }
                numbers[key] = next
                next += 1
            }
        }

        return ordered.map { segment in
            let name: String
            if let label = label(segment) { name = label }
            else if segment.channel == .mic && microphoneIsMe { name = me }
            // A live voice the server has not labelled yet gets the label it will be given.
            else if let live = liveLabel(segment.speakerId) { name = live }
            // A voice the provider could not tell apart from the others is nobody in particular.
            else if let key = voice(segment), let number = numbers[key] { name = "\(speaker) \(number)" }
            else { name = speaker }
            return Row(segment: segment, speaker: name)
        }
    }

    /// A row of a transcript as it is shown: a line, or a stretch with no transcript.
    public enum Entry: Identifiable, Sendable {
        case row(Row)
        case gap(TranscriptGap)
        public var id: String {
            switch self {
            case .row(let row): row.id
            case .gap(let gap): gap.id
            }
        }
    }

    /// The lines and the missing stretches, in timeline order. A stretch comes
    /// before a line that starts at the same moment.
    public static func entries(_ segments: [TranscriptSegmentDTO], gaps: [TranscriptGap]) -> [Entry] {
        let rows = rows(segments)
        let gaps = TranscriptGap.merged(gaps)
        guard !gaps.isEmpty else { return rows.map { .row($0) } }
        var result: [Entry] = []
        result.reserveCapacity(rows.count + gaps.count)
        var next = 0
        for row in rows {
            while next < gaps.count, gaps[next].fromMs <= row.segment.startMs { result.append(.gap(gaps[next])); next += 1 }
            result.append(.row(row))
        }
        result.append(contentsOf: gaps[next...].map { .gap($0) })
        return result
    }

    public static func entries(_ note: NoteDTO) -> [Entry] {
        entries(note.transcriptSegments, gaps: (note.transcriptGaps ?? []).map { TranscriptGap(channel: $0.channel, fromMs: $0.fromMs, toMs: $0.toMs) })
    }

    /// "12:03–12:10": a stretch of the recording, as transcript times are written.
    /// The stretch is widened to whole seconds, so it never reads shorter than it is.
    public static func span(_ gap: TranscriptGap) -> String {
        "\(time(gap.fromMs / 1000 * 1000))–\(time((gap.toMs + 999) / 1000 * 1000))"
    }
    /// The row in a transcript for a stretch with no text.
    public static func missingText(_ gap: TranscriptGap) -> String { "Transcript missing \(span(gap))" }
    /// What a recording says, for as long as it is shown, about the stretches it could not transcribe. Nil when there are none.
    public static func untranscribedText(_ gaps: [TranscriptGap]) -> String? {
        let gaps = TranscriptGap.merged(gaps)
        guard !gaps.isEmpty else { return nil }
        return "Couldn't transcribe " + gaps.map(span).joined(separator: ", ")
    }

    private static func time(_ milliseconds: Int) -> String {
        Duration.seconds(Double(max(0, milliseconds)) / 1000).formatted(.time(pattern: .minuteSecond))
    }

    private static func label(_ segment: TranscriptSegmentDTO) -> String? {
        guard let label = segment.speakerLabel?.trimmingCharacters(in: .whitespacesAndNewlines), !label.isEmpty else { return nil }
        return label
    }

    private struct VoiceKey: Hashable { let channel: String; let speakerID: String }

    /// The voice a line belongs to, or nil when the provider did not attribute it to one.
    private static func voice(_ segment: TranscriptSegmentDTO) -> VoiceKey? {
        let id = segment.speakerId.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !id.isEmpty, providerSpeaker(segment) != "unknown" else { return nil }
        return VoiceKey(channel: segment.channel.rawValue, speakerID: segment.speakerId)
    }

    /// `<channel>:<audio part>:<speaker>` is how the server names a voice it
    /// heard in one part of the recording; the last piece is the provider's own name for it.
    private static func providerSpeaker(_ segment: TranscriptSegmentDTO) -> String {
        let parts = segment.speakerId.split(separator: ":", maxSplits: 2, omittingEmptySubsequences: false)
        guard parts.count == 3, parts[0] == segment.channel.rawValue, UUID(uuidString: String(parts[1])) != nil, !parts[2].isEmpty else {
            return segment.speakerId
        }
        return String(parts[2])
    }
}

/// A stretch of a recording that has no transcript, on the note timeline.
public struct TranscriptGap: Equatable, Hashable, Sendable, Identifiable {
    public var channel: AudioChannel
    public var fromMs: Int
    public var toMs: Int
    public init(channel: AudioChannel, fromMs: Int, toMs: Int) { self.channel = channel; self.fromMs = fromMs; self.toMs = toMs }
    public var id: String { "gap-\(channel.rawValue)-\(fromMs)" }

    /// In timeline order, with stretches of one channel that overlap or touch made one.
    public static func merged(_ gaps: [TranscriptGap]) -> [TranscriptGap] {
        var result: [TranscriptGap] = []
        for gap in gaps.filter({ $0.toMs > $0.fromMs }).sorted(by: { ($0.fromMs, $0.toMs) < ($1.fromMs, $1.toMs) }) {
            if let index = result.lastIndex(where: { $0.channel == gap.channel }), result[index].toMs >= gap.fromMs {
                result[index].toMs = max(result[index].toMs, gap.toMs)
            } else {
                result.append(gap)
            }
        }
        return result.sorted { ($0.fromMs, $0.toMs) < ($1.fromMs, $1.toMs) }
    }
}
