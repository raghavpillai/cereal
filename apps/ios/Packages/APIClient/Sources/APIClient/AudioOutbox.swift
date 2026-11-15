import Foundation
import CryptoKit
import Contracts

/// Local delivery metadata, never a second copy of the server note or a credential store.
public struct AudioOutboxEntry: Codable, Sendable, Identifiable {
    public let id: String
    public let noteID: String
    public let channel: AudioChannel
    public let sequence: Int
    public let startMs: Int
    public let durationMs: Int
    public let fileName: String
    public let sourceFileName: String
    public var state: DeliveryState

    public enum DeliveryState: String, Codable, Sendable { case pending, acknowledged, ready, failed }
}

/// A single owner on the main actor serializes journal writes and capture callbacks.
/// A queue is bound to one normalized instance, including its path and port.
@MainActor
public final class AudioOutbox {
    public let instance: InstanceURL
    public let principalID: String
    public let directory: URL
    public private(set) var entries: [AudioOutboxEntry]
    /// Completed writes of the delivery journal, for measurement.
    public private(set) var journalWrites = 0
    private let journal: URL

    public init(instance: InstanceURL, principalID: String, root: URL? = nil) throws {
        guard let user = UUID(uuidString: principalID) else { throw APIClientError.invalidIdentifier }
        self.instance = instance
        self.principalID = user.uuidString.lowercased()
        let scope = SHA256.hash(data: Data((instance.description + "|" + self.principalID).utf8)).map { String(format: "%02x", $0) }.joined()
        let base = root ?? FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("server-audio-outbox", isDirectory: true)
        directory = base.appendingPathComponent(scope, isDirectory: true)
        journal = directory.appendingPathComponent("outbox.json")
        try Self.protectDirectory(directory)
        if FileManager.default.fileExists(atPath: journal.path) {
            // Fail closed; corrupt delivery metadata must not silently generate new chunk IDs.
            do { entries = try JSONDecoder().decode([AudioOutboxEntry].self, from: Data(contentsOf: journal)) }
            catch { throw APIClientError.localStorage }
            guard entries.allSatisfy({ UUID(uuidString: $0.id) != nil && UUID(uuidString: $0.noteID) != nil &&
                $0.fileName == "\($0.id).wav" && $0.sequence >= 0 && $0.startMs >= 0 && $0.durationMs > 0 }) else {
                throw APIClientError.localStorage
            }
        } else { entries = [] }
    }

    public func entries(noteID: String) -> [AudioOutboxEntry] { entries.filter { $0.noteID == noteID } }
    public func containsSource(noteID: String, fileName: String) -> Bool {
        entries.contains { $0.noteID == noteID && $0.sourceFileName == fileName }
    }
    public func nextSequence(noteID: String) -> Int { (entries(noteID: noteID).map(\.sequence).max() ?? -1) + 1 }
    public func endMs(noteID: String) -> Int { entries(noteID: noteID).map { $0.startMs + $0.durationMs }.max() ?? 0 }

    /// Copies a native PCM WAV into immutable, two-minute delivery chunks. The source is never deleted.
    /// Parsing and each copy are bounded; a long meeting is not loaded into memory as one request.
    @discardableResult
    public func stage(noteID: String, file: URL, startMs: Int, sequence: Int) throws -> [AudioOutboxEntry] {
        guard UUID(uuidString: noteID) != nil, startMs >= 0, sequence >= 0 else { throw APIClientError.invalidIdentifier }
        let existing = entries(noteID: noteID).filter { $0.sourceFileName == file.lastPathComponent }
        if !existing.isEmpty { return existing }
        let source = try PCMFile(file: file)
        let framesPerChunk = source.sampleRate * 120
        let bytesPerChunk = framesPerChunk * source.blockAlign
        var staged: [AudioOutboxEntry] = []
        var offset = 0
        while offset < source.dataBytes {
            let count = min(bytesPerChunk, source.dataBytes - offset)
            let framesBefore = offset / source.blockAlign
            let frames = count / source.blockAlign
            let part = staged.count
            let id = Self.stableID("\(instance.description)|\(principalID)|\(noteID)|\(file.lastPathComponent)|\(offset)")
            let fileName = "\(id).wav"
            let destination = directory.appendingPathComponent(fileName)
            let header = source.header(dataBytes: count)
            let input = try FileHandle(forReadingFrom: file)
            defer { try? input.close() }
            try input.seek(toOffset: UInt64(source.dataOffset + offset))
            guard let audio = try input.read(upToCount: count), audio.count == count else { throw APIClientError.invalidAudio }
            let data = header + audio
            if FileManager.default.fileExists(atPath: destination.path) {
                // A previous atomic journal failure can leave a completed chunk, but never overwrite it.
                guard try Data(contentsOf: destination) == data else { throw APIClientError.localStorage }
            } else { try Self.protectedWrite(data, to: destination) }
            let ms = Int((Int64(framesBefore) * 1000) / Int64(source.sampleRate))
            let duration = max(1, Int((Int64(frames) * 1000) / Int64(source.sampleRate)))
            staged.append(AudioOutboxEntry(id: id, noteID: noteID, channel: .mic, sequence: sequence + part,
                startMs: startMs + ms, durationMs: duration, fileName: fileName,
                sourceFileName: file.lastPathComponent, state: .pending))
            offset += count
        }
        let updated = entries + staged
        try save(updated)
        entries = updated
        return staged
    }

    /// Cuts a continuous capture into delivery parts on sample boundaries, while it
    /// is still being recorded. Part `k` holds samples `k × partFrames` up to, but not
    /// including, `(k + 1) × partFrames`: consecutive parts share one boundary, so
    /// their audio joined in order is the capture, with no gap and no overlap.
    ///
    /// A capture that is still growing yields only whole parts; `isFinal` also
    /// yields the shorter remainder. A part's identity is its source name and
    /// byte offset, exactly as for `stage`, so cutting the same capture again
    /// after a relaunch recognises every part already staged or uploaded and
    /// returns only the new ones. `startMs` is the capture's position on the
    /// note timeline; a part starts at that plus its sample offset.
    @discardableResult
    public func stageParts(noteID: String, capture file: URL, availableFrames: Int, partFrames: Int,
                           isFinal: Bool, startMs: Int, sequence: Int) throws -> [AudioOutboxEntry] {
        let frameBytes = ContinuousCapture.bytesPerFrame
        guard UUID(uuidString: noteID) != nil, startMs >= 0, sequence >= 0, availableFrames >= 0,
              partFrames > 0, partFrames * frameBytes + ContinuousCapture.headerBytes <= 20 * 1024 * 1024 else { throw APIClientError.invalidIdentifier }
        guard let inFile = ContinuousCapture.frames(inFile: file) else { throw APIClientError.invalidAudio }
        // Never cut past what is on disk, whatever the caller counted.
        let frames = min(availableFrames, inFile)
        let count = isFinal ? (frames + partFrames - 1) / partFrames : frames / partFrames
        guard count > 0 else { return [] }
        let known = Set(entries(noteID: noteID).map(\.id))
        var staged: [AudioOutboxEntry] = []
        var input: FileHandle?
        defer { try? input?.close() }
        for part in 0..<count {
            let offset = part * partFrames * frameBytes
            let id = Self.stableID("\(instance.description)|\(principalID)|\(noteID)|\(file.lastPathComponent)|\(offset)")
            if known.contains(id) { continue }
            let partLength = min(partFrames, frames - part * partFrames)
            let bytes = partLength * frameBytes
            if input == nil { input = try FileHandle(forReadingFrom: file) }
            try input!.seek(toOffset: UInt64(ContinuousCapture.headerBytes + offset))
            guard let audio = try input!.read(upToCount: bytes), audio.count == bytes else { throw APIClientError.invalidAudio }
            let data = ContinuousCapture.header(dataBytes: bytes) + audio
            let fileName = "\(id).wav"
            let destination = directory.appendingPathComponent(fileName)
            if FileManager.default.fileExists(atPath: destination.path) {
                // A part written before its journal entry was saved. It is kept only if identical.
                guard try Data(contentsOf: destination) == data else { throw APIClientError.localStorage }
            } else { try Self.protectedWrite(data, to: destination) }
            let rate = Int64(ContinuousCapture.sampleRate)
            staged.append(AudioOutboxEntry(id: id, noteID: noteID, channel: .mic, sequence: sequence + staged.count,
                startMs: startMs + Int(Int64(part * partFrames) * 1000 / rate),
                durationMs: max(1, Int(Int64(partLength) * 1000 / rate)), fileName: fileName,
                sourceFileName: file.lastPathComponent, state: .pending))
        }
        guard !staged.isEmpty else { return [] }
        let updated = entries + staged
        try save(updated)
        entries = updated
        return staged
    }

    public func audioData(for entry: AudioOutboxEntry) throws -> Data {
        try Data(contentsOf: audioFile(for: entry))
    }

    /// The validated location of a staged chunk. Chunks are immutable once
    /// staged, so the bytes can be read away from the main actor.
    public func audioFile(for entry: AudioOutboxEntry) throws -> URL {
        guard entry.fileName == "\(entry.id).wav", entries.contains(where: { $0.id == entry.id }) else { throw APIClientError.invalidAudio }
        let file = directory.appendingPathComponent(entry.fileName)
        let attributes = try file.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey, .fileSizeKey])
        guard attributes.isRegularFile == true, attributes.isSymbolicLink != true,
              let size = attributes.fileSize, size <= 20 * 1024 * 1024 else { throw APIClientError.invalidAudio }
        return file
    }

    public func update(id: String, state: AudioOutboxEntry.DeliveryState) throws {
        guard let index = entries.firstIndex(where: { $0.id == id }) else { throw APIClientError.invalidIdentifier }
        var updated = entries
        updated[index].state = state
        try save(updated)
        entries = updated
    }

    /// Called only after the owner explicitly deletes the server note.
    public func discard(noteID: String) throws {
        let removed = entries(noteID: noteID)
        let remaining = entries.filter { $0.noteID != noteID }
        try save(remaining)
        entries = remaining
        for entry in removed { try FileManager.default.removeItem(at: directory.appendingPathComponent(entry.fileName)) }
    }

    private func save(_ value: [AudioOutboxEntry]) throws {
        do { try Self.protectedWrite(JSONEncoder().encode(value), to: journal) }
        catch { throw APIClientError.localStorage }
        journalWrites += 1
    }

    private static func stableID(_ value: String) -> String {
        var bytes = Array(SHA256.hash(data: Data(value.utf8)).prefix(16))
        bytes[6] = (bytes[6] & 0x0f) | 0x50
        bytes[8] = (bytes[8] & 0x3f) | 0x80
        return UUID(uuid: (bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15])).uuidString.lowercased()
    }

    private static func protectDirectory(_ url: URL) throws {
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        #if os(iOS)
        try FileManager.default.setAttributes([.protectionKey: FileProtectionType.completeUntilFirstUserAuthentication], ofItemAtPath: url.path)
        #endif
        var excluded = url
        var values = URLResourceValues()
        values.isExcludedFromBackup = true
        try excluded.setResourceValues(values)
    }

    private static func protectedWrite(_ data: Data, to url: URL) throws {
        #if os(iOS)
        try data.write(to: url, options: [.atomic, .completeFileProtectionUntilFirstUserAuthentication])
        #else
        try data.write(to: url, options: .atomic)
        #endif
        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: url.path)
    }
}

/// Only the recorder's native linear PCM format is accepted; no compressed or arbitrary file upload.
private struct PCMFile {
    let sampleRate: Int
    let blockAlign: Int
    let dataOffset: Int
    let dataBytes: Int

    init(file: URL) throws {
        let attributes = try file.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey, .fileSizeKey])
        guard attributes.isRegularFile == true, attributes.isSymbolicLink != true,
              let size = attributes.fileSize, size >= 44 else { throw APIClientError.invalidAudio }
        let handle = try FileHandle(forReadingFrom: file)
        defer { try? handle.close() }
        guard let riff = try handle.read(upToCount: 12), riff.count == 12,
              riff.prefix(4) == Data("RIFF".utf8), riff[8..<12] == Data("WAVE".utf8) else { throw APIClientError.invalidAudio }
        var format: (Int, Int)?
        var audio: (Int, Int)?
        var offset = 12
        while offset + 8 <= size {
            try handle.seek(toOffset: UInt64(offset))
            guard let header = try handle.read(upToCount: 8), header.count == 8 else { throw APIClientError.invalidAudio }
            let length = Int(header.le32(4))
            guard length <= size - offset - 8 else { throw APIClientError.invalidAudio }
            if header.prefix(4) == Data("fmt ".utf8) {
                guard length >= 16, let fmt = try handle.read(upToCount: 16), fmt.count == 16,
                      fmt.le16(0) == 1, fmt.le16(2) == 1, fmt.le16(14) == 16,
                      fmt.le16(12) == 2, fmt.le32(4) == 16_000, fmt.le32(8) == 32_000 else { throw APIClientError.invalidAudio }
                format = (16_000, 2)
            } else if header.prefix(4) == Data("data".utf8) { audio = (offset + 8, length) }
            offset += 8 + length + length % 2
        }
        guard let format, let audio, audio.1 > 0, audio.1 % format.1 == 0 else { throw APIClientError.invalidAudio }
        sampleRate = format.0
        blockAlign = format.1
        dataOffset = audio.0
        dataBytes = audio.1
    }

    func header(dataBytes: Int) -> Data {
        var header = Data("RIFF".utf8)
        header.appendLE(UInt32(36 + dataBytes))
        header.append(Data("WAVEfmt ".utf8))
        header.appendLE(UInt32(16))
        header.appendLE(UInt16(1)); header.appendLE(UInt16(1))
        header.appendLE(UInt32(sampleRate)); header.appendLE(UInt32(sampleRate * blockAlign))
        header.appendLE(UInt16(blockAlign)); header.appendLE(UInt16(16))
        header.append(Data("data".utf8)); header.appendLE(UInt32(dataBytes))
        return header
    }
}

private extension Data {
    func le16(_ offset: Int) -> UInt16 { UInt16(self[offset]) | UInt16(self[offset + 1]) << 8 }
    func le32(_ offset: Int) -> UInt32 { UInt32(le16(offset)) | UInt32(le16(offset + 2)) << 16 }
    mutating func appendLE<T: FixedWidthInteger>(_ value: T) {
        var little = value.littleEndian
        Swift.withUnsafeBytes(of: &little) { append(contentsOf: $0) }
    }
}
