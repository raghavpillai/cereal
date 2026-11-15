import Foundation
import Observation
import Contracts

/// The text of one utterance that is still being spoken. It is its own
/// observable object, so that when the provider revises the words several
/// times a second, the one view showing them is the only thing that changes.
/// Interim text lives only here: it is never stored, saved or sent anywhere.
@MainActor @Observable
public final class LiveUtterance: Identifiable {
    public nonisolated let id: String
    public nonisolated let channel: AudioChannel
    public private(set) var text: String
    public private(set) var startMs: Int
    public private(set) var speakerLabel: String?
    @ObservationIgnored fileprivate var revision: Int

    fileprivate init(_ interim: LiveWire.Interim) {
        id = interim.utteranceId
        channel = interim.channel == .mic ? .mic : .system
        text = interim.text
        startMs = interim.startMs
        speakerLabel = interim.speakerLabel
        revision = interim.rev
    }

    fileprivate func revise(_ interim: LiveWire.Interim) {
        guard interim.rev > revision else { return }
        revision = interim.rev
        if text != interim.text { text = interim.text }
        if startMs != interim.startMs { startMs = interim.startMs }
        if speakerLabel != interim.speakerLabel { speakerLabel = interim.speakerLabel }
    }
}

/// One line of a recording note's transcript: stored text, or text still being spoken.
public enum LiveTranscriptLine: Identifiable {
    case stored(TranscriptPresentation.Row, id: String)
    case interim(LiveUtterance)

    /// Stable while an utterance turns from interim into stored text, so the
    /// line settles in place instead of being replaced.
    public nonisolated var id: String {
        switch self {
        case .stored(_, let id): id
        case .interim(let utterance): utterance.id
        }
    }
}

/// The transcript of the note being recorded, as it grows. Stored segments
/// come from the server (the live socket's finals, and note reads for whatever
/// the batch path or another device added); interim utterances come from the
/// live socket only.
@MainActor @Observable
public final class LiveTranscript {
    public nonisolated let noteID: String
    /// Stored segments, in timeline order.
    public private(set) var segments: [TranscriptSegmentDTO] = []
    /// Utterances still being spoken, oldest first.
    public private(set) var utterances: [LiveUtterance] = []
    public private(set) var status: LiveTranscriptStatus = .off
    @ObservationIgnored public private(set) var transcriptRevision = 0
    /// Stored segment → the line it took over from an interim utterance.
    @ObservationIgnored private var lineIDs: [String: String] = [:]
    @ObservationIgnored private var finishedUtterances: Set<String> = []

    public init(noteID: String) { self.noteID = noteID }

    /// Stored lines, then interim ones.
    public var lines: [LiveTranscriptLine] {
        TranscriptPresentation.rows(segments).map { .stored($0, id: lineIDs[$0.segment.id] ?? $0.segment.id) }
            + utterances.map { .interim($0) }
    }

    public func lineID(forSegment id: String) -> String { lineIDs[id] ?? id }

    /// Returns true when the stored transcript on the server has moved in a
    /// way these events do not explain, and the note should be read again.
    @discardableResult
    public func apply(_ event: LiveTranscriptEvent) -> Bool {
        switch event {
        case .ready(let revision, _, _):
            return revision != transcriptRevision
        case .interim(let interim):
            guard !finishedUtterances.contains(interim.utteranceId) else { return false }
            // Empty text withdraws the utterance: the recogniser dropped it, or its session was lost.
            if interim.text.isEmpty {
                if utterances.contains(where: { $0.id == interim.utteranceId }) { utterances.removeAll { $0.id == interim.utteranceId } }
                return false
            }
            if let existing = utterances.first(where: { $0.id == interim.utteranceId }) { existing.revise(interim) }
            else { utterances.append(LiveUtterance(interim)) }
            return false
        case .final(let final):
            finishedUtterances.insert(final.utteranceId)
            if let index = utterances.firstIndex(where: { $0.id == final.utteranceId }) {
                utterances.remove(at: index)
                // The first stored segment of an utterance keeps the interim line's identity.
                if lineIDs[final.segment.id] == nil { lineIDs[final.segment.id] = final.utteranceId }
            }
            let missed = transcriptRevision > 0 && final.transcriptRevision > transcriptRevision + 1
            transcriptRevision = max(transcriptRevision, final.transcriptRevision)
            store([final.segment])
            return missed
        case .speakers(let merges):
            var updated = segments
            var changed = false
            for merge in merges {
                let label = updated.first { $0.speakerId == merge.to }?.speakerLabel ?? Self.speakerLabel(merge.to)
                for index in updated.indices where updated[index].speakerId == merge.from {
                    let old = updated[index]
                    updated[index] = TranscriptSegmentDTO(channel: old.channel, endMs: old.endMs, id: old.id, isFinal: old.isFinal,
                        speakerId: merge.to, speakerLabel: label ?? old.speakerLabel, startMs: old.startMs, text: old.text)
                    changed = true
                }
            }
            if changed { segments = updated }
            return false
        case .gap:
            return false
        case .status(let value):
            if status != value { status = value }
            // Nothing interim survives the end of the stream.
            if case .unavailable = value, !utterances.isEmpty { utterances = [] }
            if value == .ended, !utterances.isEmpty { utterances = [] }
            return false
        }
    }

    /// Takes the stored transcript from a note read. Segments are only ever
    /// added while a meeting is recorded, so a segment this object already
    /// holds and an older read does not is kept.
    public func merge(note: NoteDTO) {
        guard UUID(uuidString: note.id) == UUID(uuidString: noteID) else { return }
        transcriptRevision = max(transcriptRevision, note.transcriptRevision)
        store(note.transcriptSegments.filter(\.isFinal))
    }

    private func store(_ incoming: [TranscriptSegmentDTO]) {
        guard !incoming.isEmpty else { return }
        var byID: [String: Int] = [:]
        for (index, segment) in segments.enumerated() { byID[segment.id] = index }
        var updated = segments
        var changed = false
        for segment in incoming {
            if let index = byID[segment.id] {
                let old = updated[index]
                guard old.text != segment.text || old.speakerId != segment.speakerId || old.speakerLabel != segment.speakerLabel
                        || old.startMs != segment.startMs || old.endMs != segment.endMs else { continue }
                updated[index] = segment
            } else {
                byID[segment.id] = updated.count
                updated.append(segment)
            }
            changed = true
        }
        guard changed else { return }
        // Timeline order; equal start times keep their arrival order.
        segments = updated.enumerated().sorted {
            $0.element.startMs == $1.element.startMs ? $0.offset < $1.offset : $0.element.startMs < $1.element.startMs
        }.map(\.element)
    }

    /// `mic:n:3` is "Speaker 3": the note-scoped speaker numbering of live segments.
    private static func speakerLabel(_ speakerID: String) -> String? {
        let parts = speakerID.split(separator: ":")
        guard parts.count == 3, parts[1] == "n", let number = Int(parts[2]) else { return nil }
        return "Speaker \(number)"
    }
}
