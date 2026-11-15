import AVFoundation
import Foundation
import APIClient

struct CaptureJournal: Codable, Sendable {
    var noteID: UUID
    var index: Int
    var startedAt: Date
    var fileName: String
}

struct RecoverableCapture: Sendable {
    var noteID: UUID
    var index: Int
    var startedAt: Date
    var audioFile: URL
    var journalFile: URL?
    var duration: TimeInterval?

    var segment: CaptureSegment? {
        guard let duration else { return nil }
        return CaptureSegment(index: index, startedAt: startedAt, duration: duration, fileName: audioFile.lastPathComponent)
    }
}

/// Scans only known recording roots and their immediate UUID directories. Never edits audio.
/// Journal acknowledgement belongs to the store, after its atomic note commit succeeds.
nonisolated enum CaptureRecovery {
    static var defaultRoots: [URL] {
        let manager = FileManager.default
        return [manager.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0],
                manager.urls(for: .documentDirectory, in: .userDomainMask)[0]]
            .map { $0.appendingPathComponent("recordings", isDirectory: true) }
    }

    static func journalURL(for audioFile: URL) -> URL { audioFile.appendingPathExtension("capture.json") }

    @discardableResult
    static func begin(noteID: UUID, index: Int, startedAt: Date, audioFile: URL) throws -> URL {
        let journal = CaptureJournal(noteID: noteID, index: index, startedAt: startedAt, fileName: audioFile.lastPathComponent)
        let url = journalURL(for: audioFile)
        let data = try JSONEncoder().encode(journal)
        #if os(iOS)
        try data.write(to: url, options: [.atomic, .completeFileProtectionUntilFirstUserAuthentication])
        #else
        try data.write(to: url, options: .atomic)
        #endif
        return url
    }

    static func readableDuration(_ audioFile: URL) -> TimeInterval? {
        guard let resource = try? audioFile.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey]),
              resource.isRegularFile == true, resource.isSymbolicLink != true else { return nil }
        // A continuous capture is measured by its size. Its header is refreshed about
        // once a second while recording, so after a kill the header can be up to a
        // second short of the audio that is really in the file.
        if ContinuousCapture.isCaptureName(audioFile.lastPathComponent), let frames = ContinuousCapture.frames(inFile: audioFile) {
            let duration = Double(frames) / Double(ContinuousCapture.sampleRate)
            return duration > 0.01 ? duration : nil
        }
        guard let audio = try? AVAudioFile(forReading: audioFile), audio.processingFormat.sampleRate > 0 else { return nil }
        let duration = Double(audio.length) / audio.processingFormat.sampleRate
        return duration.isFinite && duration > 0.01 ? duration : nil
    }

    static func scan(roots: [URL]) -> [RecoverableCapture] {
        let manager = FileManager.default
        var found: [RecoverableCapture] = []
        var visited = Set<String>()
        for root in roots {
            guard let directories = try? manager.contentsOfDirectory(at: root, includingPropertiesForKeys: [.isDirectoryKey, .isSymbolicLinkKey]) else { continue }
            for directory in directories {
                guard let noteID = UUID(uuidString: directory.lastPathComponent),
                      let resource = try? directory.resourceValues(forKeys: [.isDirectoryKey, .isSymbolicLinkKey]),
                      resource.isDirectory == true, resource.isSymbolicLink != true,
                      let files = try? manager.contentsOfDirectory(at: directory, includingPropertiesForKeys: [.isRegularFileKey, .isSymbolicLinkKey, .creationDateKey]) else { continue }
                var journals: [String: (CaptureJournal, URL)] = [:]
                var audioNames = Set<String>()
                for file in files {
                    guard let values = try? file.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey]),
                          values.isRegularFile == true, values.isSymbolicLink != true else { continue }
                    if file.lastPathComponent.hasSuffix(".wav.capture.json"),
                       let data = try? Data(contentsOf: file), let journal = try? JSONDecoder().decode(CaptureJournal.self, from: data),
                       journal.noteID == noteID, journal.index >= 0, journal.index < Int.max,
                       journal.fileName == URL(fileURLWithPath: journal.fileName).lastPathComponent,
                       journal.fileName.lowercased().hasSuffix(".wav") {
                        journals[journal.fileName] = (journal, file)
                        audioNames.insert(journal.fileName)
                    } else if file.pathExtension.lowercased() == "wav" {
                        audioNames.insert(file.lastPathComponent)
                    }
                }
                for name in audioNames.sorted() {
                    let audio = directory.appendingPathComponent(name)
                    guard visited.insert(audio.standardizedFileURL.path).inserted else { continue }
                    let journal = journals[name]
                    guard let index = journal?.0.index ?? legacyIndex(name) else { continue }
                    let created = (try? audio.resourceValues(forKeys: [.creationDateKey]))?.creationDate ?? .now
                    found.append(RecoverableCapture(noteID: noteID, index: index,
                        startedAt: journal?.0.startedAt ?? created, audioFile: audio,
                        journalFile: journal?.1, duration: readableDuration(audio)))
                }
            }
        }
        return found.sorted { $0.startedAt == $1.startedAt ? $0.index < $1.index : $0.startedAt < $1.startedAt }
    }

    /// This removes only known metadata after persistence; audio files are never deleted here.
    static func acknowledge(_ journalFile: URL?) {
        guard let journalFile else { return }
        try? FileManager.default.removeItem(at: journalFile)
    }

    static func acknowledge(noteID: UUID, fileName: String, roots: [URL]) {
        for root in roots {
            let directory = root.appendingPathComponent(noteID.uuidString, isDirectory: true)
            let journalFile = journalURL(for: directory.appendingPathComponent(fileName))
            guard let data = try? Data(contentsOf: journalFile), let journal = try? JSONDecoder().decode(CaptureJournal.self, from: data),
                  journal.noteID == noteID, journal.fileName == fileName,
                  readableDuration(directory.appendingPathComponent(fileName)) != nil else { continue }
            acknowledge(journalFile)
        }
    }

    private static func legacyIndex(_ fileName: String) -> Int? {
        let parts = fileName.split(separator: "-")
        guard parts.count >= 2, parts[0] == "segment" || parts[0] == "capture",
              let number = parts[1].split(separator: ".").first,
              let index = Int(number), index >= 0, index < Int.max else { return nil }
        return index
    }
}
