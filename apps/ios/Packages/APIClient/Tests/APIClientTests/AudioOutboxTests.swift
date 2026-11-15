import Foundation
import Testing
import Contracts
@testable import APIClient

@MainActor @Suite struct AudioOutboxTests {
    @Test func sameServerDifferentAccountsUseDifferentDurableAudioNamespaces() throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        let instance = try InstanceURL("https://example.test")
        let first = try AudioOutbox(instance: instance, principalID: "00000000-0000-4000-8000-000000000001", root: root)
        let second = try AudioOutbox(instance: instance, principalID: "00000000-0000-4000-8000-000000000002", root: root)
        #expect(first.directory != second.directory)
        #expect(second.entries.isEmpty)
        #expect(throws: APIClientError.invalidIdentifier) { try AudioOutbox(instance: instance, principalID: "not-a-user", root: root) }
    }

    @Test func splitsLongCapturePreservesSourceAndResumesSameChunkIDsAfterRelaunch() throws {
        let root = temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let source = root.appendingPathComponent("native-segment.wav")
        let audio = makePCM(seconds: 121)
        try audio.write(to: source)
        let instance = try InstanceURL("https://first.example/base")
        let queue = try AudioOutbox(instance: instance, principalID: "00000000-0000-4000-8000-000000000001", root: root.appendingPathComponent("outboxes"))
        let noteID = UUID().uuidString.lowercased()
        let staged = try queue.stage(noteID: noteID, file: source, startMs: 5000, sequence: 7)
        #expect(staged.count == 2)
        #expect(staged.map(\.startMs) == [5000, 125000])
        #expect(staged.map(\.durationMs) == [120000, 1000])
        #expect(staged.map(\.sequence) == [7, 8])
        #expect(staged.allSatisfy { $0.channel == .mic && $0.state == .pending })
        #expect(try Data(contentsOf: source) == audio)
        #expect(try queue.audioData(for: staged[0]).count == 44 + 120 * 32_000)
        try queue.update(id: staged[0].id, state: .ready)
        let restored = try AudioOutbox(instance: instance, principalID: "00000000-0000-4000-8000-000000000001", root: root.appendingPathComponent("outboxes"))
        let retried = try restored.stage(noteID: noteID, file: source, startMs: 5000, sequence: 999)
        #expect(retried.map(\.id) == staged.map(\.id))
        #expect(retried.first?.state == .ready)
        #expect(restored.entries.count == 2)
        #expect(restored.endMs(noteID: noteID) == 126000)
        #expect(restored.nextSequence(noteID: noteID) == 9)
        let permissions = try FileManager.default.attributesOfItem(atPath: restored.directory.appendingPathComponent(staged[0].fileName).path)[.posixPermissions] as? NSNumber
        #expect(permissions?.intValue == 0o600)
    }

    @Test func separateInstancesCannotSeeOrReadEachOthersQueue() throws {
        let root = temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let source = root.appendingPathComponent("segment.wav")
        try makePCM(seconds: 1).write(to: source)
        let first = try AudioOutbox(instance: InstanceURL("https://example.test/first"), principalID: "00000000-0000-4000-8000-000000000001", root: root)
        let second = try AudioOutbox(instance: InstanceURL("https://example.test/second"), principalID: "00000000-0000-4000-8000-000000000001", root: root)
        let entry = try #require(first.stage(noteID: UUID().uuidString, file: source, startMs: 0, sequence: 0).first)
        #expect(second.entries.isEmpty)
        #expect(first.directory != second.directory)
        #expect(throws: APIClientError.invalidAudio) { try second.audioData(for: entry) }
    }

    @Test func malformedAudioAndCorruptJournalFailClosedWithoutReplacingOriginal() throws {
        let root = temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let instance = try InstanceURL("https://example.test")
        let queue = try AudioOutbox(instance: instance, principalID: "00000000-0000-4000-8000-000000000001", root: root)
        let source = root.appendingPathComponent("broken.wav")
        let bad = Data("not a wave".utf8)
        try bad.write(to: source)
        #expect(throws: APIClientError.invalidAudio) { try queue.stage(noteID: UUID().uuidString, file: source, startMs: 0, sequence: 0) }
        #expect(queue.entries.isEmpty)
        #expect(try Data(contentsOf: source) == bad)
        try Data("corrupt metadata".utf8).write(to: queue.directory.appendingPathComponent("outbox.json"))
        #expect(throws: APIClientError.localStorage) { try AudioOutbox(instance: instance, principalID: "00000000-0000-4000-8000-000000000001", root: root) }
        #expect(try String(contentsOf: queue.directory.appendingPathComponent("outbox.json"), encoding: .utf8) == "corrupt metadata")
    }

    @Test func truncatedWaveDoesNotBecomeAcknowledgedUpload() throws {
        let root = temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let queue = try AudioOutbox(instance: InstanceURL("https://example.test"), principalID: "00000000-0000-4000-8000-000000000001", root: root)
        let source = root.appendingPathComponent("truncated.wav")
        try makePCM(seconds: 1).prefix(100).write(to: source)
        #expect(throws: APIClientError.invalidAudio) { try queue.stage(noteID: UUID().uuidString, file: source, startMs: 0, sequence: 0) }
        #expect(queue.entries.isEmpty)
    }
}

private func temporaryDirectory() -> URL {
    let root = FileManager.default.temporaryDirectory.appendingPathComponent("cereal-outbox-test-\(UUID().uuidString)")
    try! FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
    return root
}

private func makePCM(seconds: Int) -> Data {
    let bytes = seconds * 32_000
    var data = Data("RIFF".utf8)
    func append<T: FixedWidthInteger>(_ number: T) {
        var little = number.littleEndian
        withUnsafeBytes(of: &little) { data.append(contentsOf: $0) }
    }
    append(UInt32(36 + bytes)); data.append(Data("WAVEfmt ".utf8)); append(UInt32(16))
    append(UInt16(1)); append(UInt16(1)); append(UInt32(16_000)); append(UInt32(32_000))
    append(UInt16(2)); append(UInt16(16)); data.append(Data("data".utf8)); append(UInt32(bytes))
    data.append(Data(repeating: 1, count: bytes))
    return data
}
