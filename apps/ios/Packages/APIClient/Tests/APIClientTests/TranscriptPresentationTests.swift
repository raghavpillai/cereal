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

    /// Nothing about where the audio came from, or which part of the recording it was in.
    private func saysNothingOfTheAudio(_ caption: String) -> Bool {
        !["System audio", "Microphone", "part", "Part", "Mic", "System", "Unlabeled", "Unknown"].contains { caption.contains($0) }
    }

    @Test func aLabelIsTheSpeakerAndTheTimeAndNothingElse() {
        let rows = TranscriptPresentation.rows([
            segment("system:\(firstChunk):speaker_3", label: "Speaker 3", channel: .system, at: 70_000),
            segment("mic:\(firstChunk):speaker_1", at: 74_000),
            segment("mic:\(nextChunk):speaker_1", label: "Speaker 1", at: 125_000)
        ])
        #expect(rows.map(\.caption) == ["Speaker 3 · 1:10", "Me · 1:14", "Speaker 1 · 2:05"])
        #expect(rows.allSatisfy { saysNothingOfTheAudio($0.caption) })
    }

    @Test func oneMicrophoneCanContainSeveralDistinctDiarizedVoices() {
        let rows = TranscriptPresentation.rows([
            segment("mic:\(firstChunk):speaker_1", label: "Speaker 1"),
            segment("mic:\(firstChunk):speaker_2", label: "Speaker 2", at: 1000),
            segment("mic:\(firstChunk):speaker_3", label: "Speaker 3", at: 2000)
        ])
        #expect(rows.map(\.speaker) == ["Speaker 1", "Speaker 2", "Speaker 3"])
        #expect(rows.map(\.caption) == ["Speaker 1 · 0:00", "Speaker 2 · 0:01", "Speaker 3 · 0:02"])
        // A room heard by one microphone: nobody in it is taken to be the person recording.
        #expect(!rows.contains { $0.caption.contains("You") || $0.caption.contains("Me") })
    }

    @Test func inARecordingOfARoomUnnamedVoicesAreNumberedSpeakers() {
        let rows = TranscriptPresentation.rows([segment("self"), segment("other", at: 1000), segment("self", at: 2000)])
        #expect(rows.map(\.speaker) == ["Speaker 1", "Speaker 2", "Speaker 1"])
        #expect(rows.map(\.caption) == ["Speaker 1 · 0:00", "Speaker 2 · 0:01", "Speaker 1 · 0:02"])
        #expect(!rows.contains { $0.speaker == "Me" || $0.speaker == "You" })
    }

    @Test func aSingleUnnamedVoiceInARoomIsSpeakerWithoutANumber() {
        let rows = TranscriptPresentation.rows([segment("mic:\(firstChunk):speaker_0"), segment("mic:\(firstChunk):speaker_0", at: 4000)])
        #expect(rows.map(\.speaker) == ["Speaker", "Speaker"])
        #expect(rows.map(\.caption) == ["Speaker · 0:00", "Speaker · 0:04"])
    }

    @Test func theMicrophoneOfARecordingWithSystemAudioIsThePersonRecording() {
        let rows = TranscriptPresentation.rows([
            segment("mic:\(firstChunk):speaker_0"),
            segment("system:\(firstChunk):speaker_1", label: "Speaker 1", channel: .system, at: 1000),
            segment("mic:\(nextChunk):speaker_0", at: 2000),
            segment("system:\(firstChunk):speaker_2", label: "Speaker 2", channel: .system, at: 3000),
            // Not attributed to any voice, but still through the person's own microphone.
            segment("mic:\(nextChunk):unknown", at: 4000),
            segment("", at: 5000)
        ])
        #expect(rows.map(\.speaker) == ["Me", "Speaker 1", "Me", "Speaker 2", "Me", "Me"])
        #expect(TranscriptPresentation.microphoneIsMe(rows.map(\.segment)))
        #expect(!TranscriptPresentation.microphoneIsMe([segment("mic:\(firstChunk):speaker_0")]))
        #expect(!TranscriptPresentation.microphoneIsMe([]))
        // A microphone voice the server did name keeps that name.
        let named = TranscriptPresentation.rows([
            segment("mic:\(firstChunk):speaker_0", label: "Priya"),
            segment("system:\(firstChunk):speaker_1", label: "Speaker 1", channel: .system, at: 1000)
        ])
        #expect(named.map(\.speaker) == ["Priya", "Speaker 1"])
    }

    @Test func unnamedSystemVoicesAreNumberedAndNeverTakeANumberTheServerGave() {
        let rows = TranscriptPresentation.rows([
            segment("mic:\(firstChunk):speaker_0"),
            segment("system:\(firstChunk):speaker_a", channel: .system, at: 1000),
            segment("system:\(firstChunk):speaker_1", label: "Speaker 1", channel: .system, at: 2000),
            segment("system:\(firstChunk):speaker_b", channel: .system, at: 3000),
            segment("system:\(firstChunk):speaker_a", channel: .system, at: 4000)
        ])
        // "Speaker 1" is the server's; the unnamed voices take the next free numbers.
        #expect(rows.map(\.speaker) == ["Me", "Speaker 2", "Speaker 1", "Speaker 3", "Speaker 2"])

        let single = TranscriptPresentation.rows([
            segment("mic:\(firstChunk):speaker_0"),
            segment("system:\(firstChunk):speaker_a", channel: .system, at: 1000)
        ])
        #expect(single.map(\.speaker) == ["Me", "Speaker"])
    }

    @Test func theSameVoiceInALaterPartOfTheRecordingKeepsItsNumber() {
        // The server gives a voice it recognises again the same identifier, whichever part it is heard in.
        let rows = TranscriptPresentation.rows([
            segment("voice-a"), segment("voice-b", at: 1000),
            segment("voice-a", at: 120_000), segment("voice-c", at: 121_000), segment("voice-b", at: 122_000)
        ])
        #expect(rows.map(\.speaker) == ["Speaker 1", "Speaker 2", "Speaker 1", "Speaker 3", "Speaker 2"])
        // A label the server repeats in another part is shown as it is, with nothing about the part.
        let labelled = TranscriptPresentation.rows([
            segment("mic:\(firstChunk):speaker_1", label: "Speaker 1"),
            segment("mic:\(nextChunk):speaker_1", label: "Speaker 1", at: 120_000),
            segment("mic:\(firstChunk):speaker_2", label: "Speaker 2", at: 4000)
        ])
        #expect(labelled.map(\.caption) == ["Speaker 1 · 0:00", "Speaker 2 · 0:04", "Speaker 1 · 2:00"])
    }

    @Test func theSameOpaqueIdentifierOnTheTwoChannelsIsTwoVoices() {
        let rows = TranscriptPresentation.rows([
            segment("same-opaque-id"),
            segment("same-opaque-id", channel: .system),
            segment("other-id", channel: .system, at: 1000)
        ])
        #expect(rows.map(\.speaker) == ["Me", "Speaker 1", "Speaker 2"])
    }

    @Test func explicitLabelsArePreservedWithoutMatchingToCalendarNamesOrSource() {
        let rows = TranscriptPresentation.rows([
            segment("mic:\(firstChunk):speaker_0", label: "Speaker 0"),
            segment("mic:\(firstChunk):manual", label: "  Zoë 🦊  ", at: 500)
        ])
        #expect(rows.map(\.speaker) == ["Speaker 0", "Zoë 🦊"])
        #expect(rows.map { $0.segment.speakerLabel } == ["Speaker 0", "  Zoë 🦊  "])
    }

    @Test func aVoiceNobodyCouldAttributeIsNeverBlankNumberedOrNamed() {
        let rows = TranscriptPresentation.rows([
            segment("mic:\(firstChunk):unknown"),
            segment("mic:\(firstChunk):speaker_1", label: " \n ", at: 500),
            segment("mic:\(firstChunk):speaker_2", at: 700),
            segment("unknown", at: 1000),
            segment("", at: 2000)
        ])
        #expect(rows.map(\.speaker) == ["Speaker", "Speaker 1", "Speaker 2", "Speaker", "Speaker"])
        #expect(rows.allSatisfy { !$0.speaker.isEmpty && saysNothingOfTheAudio($0.caption) })
    }

    @Test func chronologicalRowsKeepEveryUtteranceAndInterimUncertainty() {
        let late = segment("voice-1", at: 2000, text: "Later")
        let interim = segment("voice-1", at: 0, final: false, text: "Provisional")
        let overlap = segment("voice-2", channel: .system, at: 0, text: "Overlapping")
        let rows = TranscriptPresentation.rows([late, interim, overlap])
        #expect(rows.map { $0.segment.id } == [interim.id, overlap.id, late.id])
        #expect(rows.map { $0.segment.text } == ["Provisional", "Overlapping", "Later"])
        #expect(rows[0].caption == "Me · 0:00 · Interim")
        #expect(rows[1].caption == "Speaker · 0:00")
        #expect(!rows[1].caption.contains("Interim"))
    }

    @Test func aRowIsFoundByItsSegmentWhateverItIsCalled() {
        // A citation opens the transcript at a segment's identifier; the label plays no part.
        let cited = segment("system:\(firstChunk):speaker_2", label: "Speaker 2", channel: .system, at: 9000, text: "The cited line")
        let rows = TranscriptPresentation.rows([segment("mic:\(firstChunk):speaker_0"), cited, segment("mic:\(firstChunk):speaker_0", at: 12_000)])
        #expect(rows.map(\.id) == rows.map(\.segment.id))
        #expect(rows.filter { $0.id == cited.id }.map(\.segment.text) == ["The cited line"])
        #expect(Set(rows.map(\.id)).count == 3)
    }

    @Test func wordsStillBeingSpokenHaveASpeakerOnlyWhenThereIsOne() {
        #expect(TranscriptPresentation.interimCaption(channel: .mic, startMs: 72_000, speakerLabel: nil) == "1:12 · Interim")
        #expect(TranscriptPresentation.interimCaption(channel: .system, startMs: 0, speakerLabel: " Speaker 3 ") == "Speaker 3 · 0:00 · Interim")
        #expect(TranscriptPresentation.interimCaption(channel: .mic, startMs: 5000, speakerLabel: "  ", microphoneIsMe: true) == "Me · 0:05 · Interim")
        #expect(TranscriptPresentation.interimCaption(channel: .system, startMs: 5000, speakerLabel: nil, microphoneIsMe: true) == "0:05 · Interim")
        #expect(TranscriptPresentation.interimCaption(channel: .mic, startMs: 5000, speakerLabel: "Speaker 2", microphoneIsMe: true) == "Speaker 2 · 0:05 · Interim")
    }
}
