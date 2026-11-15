import Foundation
import Testing
import Contracts
@testable import APIClient

@Suite struct TranscriptPresentationTests {
    private let firstChunk = "00000000-0000-4000-8000-000000000001"
    private let nextChunk = "00000000-0000-4000-8000-000000000002"
    private func segment(_ speaker: String, label: String? = nil, channel: AudioChannel = .mic, at start: Int = 0, final: Bool = true, text: String = "Fictional speech") -> TranscriptSegmentDTO {
        TranscriptSegmentDTO(channel: channel, endMs: start + 500, id: UUID().uuidString, isFinal: final,
                             speakerId: speaker, speakerLabel: label, startMs: start, text: text)
    }

    @Test func oneMicrophoneCanContainSeveralDistinctDiarizedVoices() {
        let rows = TranscriptPresentation.rows([
            segment("mic:\(firstChunk):speaker_1", label: "Speaker 1"),
            segment("mic:\(firstChunk):speaker_2", label: "Speaker 2", at: 1000),
            segment("mic:\(firstChunk):speaker_3", label: "Speaker 3", at: 2000)
        ])
        #expect(rows.map(\.speaker) == ["Speaker 1", "Speaker 2", "Speaker 3"])
        #expect(rows.allSatisfy { $0.source == "Microphone" && $0.audioPart == 1 })
        #expect(!rows.contains { $0.caption.contains("You") || $0.caption.contains("Me") })
    }

    @Test func missingLabelsKeepDistinctVoicesInsteadOfFlatteningToMicrophoneOwner() {
        let rows = TranscriptPresentation.rows([segment("self"), segment("other", at: 1000), segment("self", at: 2000)])
        #expect(rows.map(\.speaker) == ["Unlabeled speaker 1", "Unlabeled speaker 2", "Unlabeled speaker 1"])
        #expect(rows.allSatisfy { $0.source == "Microphone" && $0.audioPart == nil })
        #expect(!rows.contains { $0.speaker == "You" || $0.speaker == "Microphone" })
    }

    @Test func reusedProviderLabelsInDifferentChunksRemainVisiblyScoped() {
        let rows = TranscriptPresentation.rows([
            segment("mic:\(firstChunk):speaker_1", label: "Speaker 1"),
            segment("mic:\(nextChunk):speaker_1", label: "Speaker 1", at: 120000),
            segment("mic:\(firstChunk):speaker_2", label: "Speaker 2", at: 4000)
        ])
        #expect(rows.map(\.audioPart) == [1, 1, 2])
        #expect(rows[0].caption.contains("Microphone part 1"))
        #expect(rows[2].caption.contains("Microphone part 2"))
        #expect(rows[0].segment.speakerId != rows[2].segment.speakerId)
        #expect(TranscriptPresentation.speakerNotice.contains("does not identify a person"))
    }

    @Test func micAndSystemUseIndependentPartScopesEvenWithSameOpaqueSpeakerID() {
        let rows = TranscriptPresentation.rows([
            segment("same-opaque-id"),
            segment("same-opaque-id", channel: .system),
            segment("system:\(firstChunk):speaker_1", label: "Speaker 1", channel: .system, at: 1000),
            segment("mic:\(nextChunk):speaker_1", label: "Speaker 1", at: 1000)
        ])
        #expect(rows[0].source == "Microphone" && rows[1].source == "System audio")
        #expect(rows[0].caption != rows[1].caption)
        #expect(rows[2].source == "System audio" && rows[2].audioPart == 1)
        #expect(rows[3].source == "Microphone" && rows[3].audioPart == 1)
    }

    @Test func explicitLabelsArePreservedWithoutMatchingToCalendarNamesOrSource() {
        let rows = TranscriptPresentation.rows([
            segment("mic:\(firstChunk):speaker_0", label: "Speaker 0"),
            segment("mic:\(firstChunk):manual", label: "  Zoë 🦊  ", at: 500)
        ])
        #expect(rows.map(\.speaker) == ["Speaker 0", "Zoë 🦊"])
        #expect(rows.map { $0.segment.speakerLabel } == ["Speaker 0", "  Zoë 🦊  "])
    }

    @Test func unknownAndWhitespaceLabelsDoNotBecomeBlankOrNamedPeople() {
        let rows = TranscriptPresentation.rows([
            segment("mic:\(firstChunk):unknown"),
            segment("mic:\(firstChunk):speaker_1", label: " \n ", at: 500),
            segment("unknown", channel: .system, at: 1000),
            segment("", at: 2000)
        ])
        #expect(rows.map(\.speaker) == ["Unknown speaker", "Unlabeled speaker 1", "Unknown speaker", "Unknown speaker"])
    }

    @Test func chronologicalRowsKeepEveryUtteranceAndInterimUncertainty() {
        let late = segment("voice-1", at: 2000, text: "Later")
        let interim = segment("voice-1", at: 0, final: false, text: "Provisional")
        let overlap = segment("voice-2", channel: .system, at: 0, text: "Overlapping")
        let rows = TranscriptPresentation.rows([late, interim, overlap])
        #expect(rows.map { $0.segment.id } == [interim.id, overlap.id, late.id])
        #expect(rows.map { $0.segment.text } == ["Provisional", "Overlapping", "Later"])
        #expect(rows[0].caption.contains("Interim"))
        #expect(!rows[1].caption.contains("Interim"))
    }

    @Test func malformedOrDifferentChannelScopeIsNotPresentedAsConfirmedAudioPart() {
        let rows = TranscriptPresentation.rows([
            segment("mic:not-a-uuid:speaker_1"),
            segment("system:\(firstChunk):speaker_1"),
            segment("mic:\(firstChunk):")
        ])
        #expect(rows.allSatisfy { $0.audioPart == nil })
        #expect(Set(rows.map(\.speaker)).count == 3)
    }
}
