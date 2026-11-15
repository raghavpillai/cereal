import Foundation
import Testing
@testable import Cereal

@MainActor
struct RecordingRecoveryTests {
    private func directory() throws -> URL {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("RecordingRecovery-\(UUID())", isDirectory: true)
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        return url
    }

    /// A real readable mono PCM WAV; duration comes from its frames, never elapsed wall time.
    private func writeWAV(_ file: URL, frames: Int = 16_000) throws {
        var data = Data()
        func text(_ value: String) { data.append(contentsOf: value.utf8) }
        func word<T: FixedWidthInteger>(_ value: T) {
            var little = value.littleEndian
            withUnsafeBytes(of: &little) { data.append(contentsOf: $0) }
        }
        text("RIFF"); word(UInt32(36 + frames * 2)); text("WAVEfmt ")
        word(UInt32(16)); word(UInt16(1)); word(UInt16(1)); word(UInt32(16_000))
        word(UInt32(32_000)); word(UInt16(2)); word(UInt16(16)); text("data")
        word(UInt32(frames * 2)); data.append(Data(repeating: 0, count: frames * 2))
        try data.write(to: file)
    }

    private func recordingFile(_ root: URL, id: UUID, name: String = "segment-0.wav") throws -> URL {
        let directory = root.appendingPathComponent(id.uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        return directory.appendingPathComponent(name)
    }

    @Test func journalRecoversFreshUntitledNoteBeforeEmptyFilteringAndOnlyOnce() throws {
        let temporary = try directory()
        defer { try? FileManager.default.removeItem(at: temporary) }
        let notesFile = temporary.appendingPathComponent("notes.json")
        let root = temporary.appendingPathComponent("recordings", isDirectory: true)
        let id = UUID()
        let startedAt = Date(timeIntervalSince1970: 100)
        try JSONEncoder().encode([Note(id: id, title: "", createdAt: startedAt)]).write(to: notesFile)
        let audio = try recordingFile(root, id: id)
        let journal = try CaptureRecovery.begin(noteID: id, index: 0, startedAt: startedAt, audioFile: audio)
        try writeWAV(audio, frames: 24_000)
        let store = NoteStore(fileURL: notesFile, recordingsRoots: [root])
        let restored = try #require(store.note(id))
        #expect(restored.captureSegments.count == 1)
        #expect(restored.captureSegments.first?.duration == 1.5)
        #expect(restored.captureSegments.first?.startedAt == startedAt)
        #expect(restored.transcriptRevision == 1)
        #expect(restored.captureRecoveryHint != nil)
        #expect(!FileManager.default.fileExists(atPath: journal.path))
        #expect(FileManager.default.fileExists(atPath: audio.path))
        let reloaded = NoteStore(fileURL: notesFile, recordingsRoots: [root])
        #expect(reloaded.note(id)?.captureSegments.count == 1)
        #expect(reloaded.note(id)?.transcriptRevision == 1)
    }

    @Test func legacyUUIDFolderRestoresMissingNoteAndKeepsOriginalFile() throws {
        let temporary = try directory()
        defer { try? FileManager.default.removeItem(at: temporary) }
        let notesFile = temporary.appendingPathComponent("notes.json")
        try Data("[]".utf8).write(to: notesFile)
        let root = temporary.appendingPathComponent("Documents-recordings", isDirectory: true)
        let id = UUID()
        let audio = try recordingFile(root, id: id, name: "segment-4.wav")
        try writeWAV(audio, frames: 8_000)
        let original = try Data(contentsOf: audio)
        let store = NoteStore(fileURL: notesFile, recordingsRoots: [root])
        let restored = try #require(store.note(id))
        #expect(restored.captureSegments.first?.index == 4)
        #expect(restored.captureSegments.first?.fileName == "segment-4.wav")
        #expect(restored.captureSegments.first?.duration == 0.5)
        #expect(try Data(contentsOf: audio) == original)
        #expect(NoteStore(fileURL: notesFile, recordingsRoots: [root]).note(id)?.captureSegments.count == 1)
    }

    @Test func unreadableAudioAndJournalRemainAndNoteIsNotDiscarded() throws {
        let temporary = try directory()
        defer { try? FileManager.default.removeItem(at: temporary) }
        let notesFile = temporary.appendingPathComponent("notes.json")
        try Data("[]".utf8).write(to: notesFile)
        let root = temporary.appendingPathComponent("recordings", isDirectory: true)
        let id = UUID()
        let audio = try recordingFile(root, id: id)
        let bytes = Data("unfinished wav bytes".utf8)
        try bytes.write(to: audio)
        let journal = try CaptureRecovery.begin(noteID: id, index: 0, startedAt: .now, audioFile: audio)
        let store = NoteStore(fileURL: notesFile, recordingsRoots: [root])
        #expect(store.note(id)?.captureSegments.isEmpty == true)
        #expect(store.note(id)?.captureRecoveryHint != nil)
        #expect(store.captureRecoveryMessage != nil)
        store.discardIfEmpty(id)
        #expect(store.note(id) != nil)
        #expect(FileManager.default.fileExists(atPath: journal.path))
        #expect(try Data(contentsOf: audio) == bytes)
        #expect(NoteStore(fileURL: notesFile, recordingsRoots: [root]).note(id) != nil)
        // Even a device-position fallback committed at failure cannot acknowledge bad audio.
        let partial = CaptureSegment(index: 0, startedAt: .now, duration: 1, fileName: audio.lastPathComponent)
        #expect(store.finishCapture(id, segment: partial))
        #expect(FileManager.default.fileExists(atPath: journal.path))
        let reloaded = NoteStore(fileURL: notesFile, recordingsRoots: [root])
        #expect(reloaded.note(id)?.captureSegments.count == 1)
        #expect(reloaded.note(id)?.captureRecoveryHint != nil)
        #expect(FileManager.default.fileExists(atPath: journal.path))
    }

    @Test func failedNoteCommitNeverAcknowledgesJournal() throws {
        let temporary = try directory()
        defer { try? FileManager.default.removeItem(at: temporary) }
        let root = temporary.appendingPathComponent("recordings", isDirectory: true)
        let id = UUID()
        let audio = try recordingFile(root, id: id)
        try writeWAV(audio)
        let journal = try CaptureRecovery.begin(noteID: id, index: 0, startedAt: .now, audioFile: audio)
        let notesFile = temporary.appendingPathComponent("notes.json")
        let corrupt = Data("preserve corrupt note store".utf8)
        try corrupt.write(to: notesFile)
        let store = NoteStore(fileURL: notesFile, recordingsRoots: [root])
        let recovered = try #require(store.note(id)?.captureSegments.first)
        #expect(!store.finishCapture(id, segment: recovered))
        #expect(FileManager.default.fileExists(atPath: journal.path))
        #expect(try Data(contentsOf: notesFile) == corrupt)
    }

    @Test func normalStopAcknowledgesOnlyAfterSuccessfulCommit() throws {
        let temporary = try directory()
        defer { try? FileManager.default.removeItem(at: temporary) }
        let notesFile = temporary.appendingPathComponent("notes.json")
        try Data("[]".utf8).write(to: notesFile)
        let root = temporary.appendingPathComponent("recordings", isDirectory: true)
        let store = NoteStore(fileURL: notesFile, recordingsRoots: [root])
        let id = store.createNote().id
        let audio = try recordingFile(root, id: id)
        let journal = try CaptureRecovery.begin(noteID: id, index: 0, startedAt: .now, audioFile: audio)
        try writeWAV(audio)
        let segment = CaptureSegment(index: 0, startedAt: .now, duration: 1, fileName: audio.lastPathComponent)
        #expect(store.finishCapture(id, segment: segment))
        #expect(!FileManager.default.fileExists(atPath: journal.path))
        let reloaded = NoteStore(fileURL: notesFile, recordingsRoots: [root])
        #expect(reloaded.note(id)?.captureSegments.count == 1)
        #expect(reloaded.note(id)?.transcriptRevision == 1)
    }

    @Test func atomicWriteFailureKeepsJournalUntilLaterSuccessfulLoad() throws {
        let temporary = try directory()
        defer { try? FileManager.default.removeItem(at: temporary) }
        let root = temporary.appendingPathComponent("recordings", isDirectory: true)
        let id = UUID()
        let audio = try recordingFile(root, id: id)
        try writeWAV(audio)
        let journal = try CaptureRecovery.begin(noteID: id, index: 0, startedAt: .now, audioFile: audio)
        let absentParent = temporary.appendingPathComponent("not-created", isDirectory: true)
        let notesFile = absentParent.appendingPathComponent("notes.json")
        let store = NoteStore(fileURL: notesFile, recordingsRoots: [root])
        #expect(store.note(id)?.captureSegments.count == 1)
        #expect(store.persistenceError != nil)
        #expect(FileManager.default.fileExists(atPath: journal.path))
        try FileManager.default.createDirectory(at: absentParent, withIntermediateDirectories: true)
        let recovered = NoteStore(fileURL: notesFile, recordingsRoots: [root])
        #expect(recovered.note(id)?.captureSegments.count == 1)
        #expect(!FileManager.default.fileExists(atPath: journal.path))
    }

    @Test func journalNeverReadsAudioThroughASymbolicLink() throws {
        let temporary = try directory()
        defer { try? FileManager.default.removeItem(at: temporary) }
        let notesFile = temporary.appendingPathComponent("notes.json")
        try Data("[]".utf8).write(to: notesFile)
        let root = temporary.appendingPathComponent("recordings", isDirectory: true)
        let id = UUID()
        let audio = try recordingFile(root, id: id)
        let outside = temporary.appendingPathComponent("outside.wav")
        try writeWAV(outside)
        try FileManager.default.createSymbolicLink(at: audio, withDestinationURL: outside)
        let journal = try CaptureRecovery.begin(noteID: id, index: 0, startedAt: .now, audioFile: audio)
        let store = NoteStore(fileURL: notesFile, recordingsRoots: [root])
        #expect(store.note(id)?.captureSegments.isEmpty == true)
        #expect(store.note(id)?.captureRecoveryHint != nil)
        #expect(FileManager.default.fileExists(atPath: journal.path))
        #expect(CaptureRecovery.readableDuration(audio) == nil)
        #expect(CaptureRecovery.readableDuration(outside) == 1)
    }
}
