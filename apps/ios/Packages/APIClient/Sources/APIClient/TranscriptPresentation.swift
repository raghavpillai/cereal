import Foundation
import Contracts

/// Diarization identifies voices within an audio part, not calendar people.
/// Never infer the owner's identity from a microphone source or join batch-local
/// IDs across parts merely because the provider reused a display label.
public enum TranscriptPresentation {
    public struct Row: Identifiable, Sendable {
        public let segment: TranscriptSegmentDTO
        public let speaker: String
        public let source: String
        public let audioPart: Int?
        public var id: String { segment.id }
        public var caption: String {
            let source = audioPart.map { "\(self.source) part \($0)" } ?? self.source
            let time = Duration.seconds(Double(max(0, segment.startMs)) / 1000).formatted(.time(pattern: .minuteSecond))
            return [speaker, source, time, segment.isFinal ? nil : "Interim"].compactMap { $0 }.joined(separator: " · ")
        }
    }

    /// Caption of text that is still being spoken. The speaker is a draft from
    /// the provider and is left out when it has none yet.
    public static func interimCaption(channel: AudioChannel, startMs: Int, speakerLabel: String?) -> String {
        let speaker = speakerLabel?.trimmingCharacters(in: .whitespacesAndNewlines)
        let time = Duration.seconds(Double(max(0, startMs)) / 1000).formatted(.time(pattern: .minuteSecond))
        return [speaker?.isEmpty == false ? speaker : nil, channel == .mic ? "Microphone" : "System audio", time, "Interim"]
            .compactMap { $0 }.joined(separator: " · ")
    }

    public static let speakerNotice = "Speaker labels can change between audio parts. Audio source does not identify a person."

    public static func rows(_ segments: [TranscriptSegmentDTO]) -> [Row] {
        // Keep equal-time sources in their server order and retain every utterance.
        let ordered = segments.enumerated().sorted {
            $0.element.startMs == $1.element.startMs ? $0.offset < $1.offset : $0.element.startMs < $1.element.startMs
        }
        var partNumbers: [String: Int] = [:]
        var partCounts: [String: Int] = [:]
        var unlabeledNumbers: [SpeakerKey: Int] = [:]
        var unlabeledCounts: [String: Int] = [:]
        return ordered.map { _, segment in
            let channel = segment.channel.rawValue
            let scope = chunkScope(segment)
            var part: Int?
            if let scope {
                let key = channel + ":" + scope.chunk
                if partNumbers[key] == nil {
                    partCounts[channel, default: 0] += 1
                    partNumbers[key] = partCounts[channel]
                }
                part = partNumbers[key]
            }
            let scopeKey = channel + ":" + (scope?.chunk ?? "legacy")
            let label = segment.speakerLabel?.trimmingCharacters(in: .whitespacesAndNewlines)
            let speaker: String
            if let label, !label.isEmpty {
                speaker = label
            } else if segment.speakerId.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || (scope?.providerSpeaker ?? segment.speakerId) == "unknown" {
                speaker = "Unknown speaker"
            } else {
                let key = SpeakerKey(channel: channel, speakerID: segment.speakerId)
                if unlabeledNumbers[key] == nil {
                    unlabeledCounts[scopeKey, default: 0] += 1
                    unlabeledNumbers[key] = unlabeledCounts[scopeKey]
                }
                speaker = "Unlabeled speaker \(unlabeledNumbers[key]!)"
            }
            return Row(segment: segment, speaker: speaker,
                       source: segment.channel == .mic ? "Microphone" : "System audio", audioPart: part)
        }
    }

    private struct SpeakerKey: Hashable { let channel: String; let speakerID: String }
    private static func chunkScope(_ segment: TranscriptSegmentDTO) -> (chunk: String, providerSpeaker: String)? {
        let parts = segment.speakerId.split(separator: ":", maxSplits: 2, omittingEmptySubsequences: false)
        guard parts.count == 3, parts[0] == segment.channel.rawValue,
              let id = UUID(uuidString: String(parts[1])), !parts[2].isEmpty else { return nil }
        return (id.uuidString.lowercased(), String(parts[2]))
    }
}
