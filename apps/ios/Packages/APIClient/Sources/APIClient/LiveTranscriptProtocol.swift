import Foundation
import Contracts

/// Wire protocol of the live transcription socket, `GET /api/v1/notes/:id/live`
/// (the server's schemas are packages/contracts/src/live.ts). Text frames are
/// JSON, audio frames are binary. Decoding ignores message types and enum
/// values this build does not know, so an additive server change is not an error.
public enum LiveWire {
    public static let protocolVersion: UInt8 = 1
    public static let frameHeaderBytes = 12
    public static let maxFrameBytes = 64 * 1024

    public enum Channel: String, Codable, Sendable {
        case mic, system
        var code: UInt8 { self == .mic ? 0 : 1 }
    }

    // MARK: Client to server

    public struct Stream: Codable, Equatable, Sendable {
        public var channel: Channel
        public var sampleRate: Int
        public var encoding = "pcm_s16le"
        /// Note-timeline position of sample 0 of this capture.
        public var baseMs: Int
        /// The last `ack.throughSample` seen for this capture and channel, or 0.
        public var resumeFromSample: Int64
    }

    public struct Hello: Codable, Equatable, Sendable {
        public var type = "hello"
        public var `protocol` = Int(LiveWire.protocolVersion)
        public var captureId: String
        public var language: String?
        public var streams: [Stream]
    }

    public enum MarkKind: String, Codable, Sendable { case pause, resume, end }

    public struct Mark: Codable, Equatable, Sendable {
        public var type = "mark"
        public var kind: MarkKind
        /// Per channel, the number of samples the capture holds at the mark.
        public var atSample: [String: Int64]
    }

    public static func encode<Message: Encodable>(_ message: Message) throws -> String {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        return String(decoding: try encoder.encode(message), as: UTF8.self)
    }

    // MARK: Binary audio frame

    /// Byte 0 protocol version, byte 1 channel, bytes 2–3 zero, bytes 4–11 the
    /// index of the first sample as an unsigned 64-bit little-endian integer,
    /// then PCM16 little-endian samples.
    public static func audioFrame(channel: Channel, firstSample: Int64, pcm: Data) -> Data {
        var frame = Data(capacity: frameHeaderBytes + pcm.count)
        frame.append(protocolVersion)
        frame.append(channel.code)
        frame.append(contentsOf: [0, 0])
        frame.appendLittleEndian(UInt64(max(0, firstSample)))
        frame.append(pcm)
        return frame
    }

    /// The inverse, with the server's validation. Nil for anything that is not a well-formed frame.
    public static func parseAudioFrame(_ frame: Data) -> (channel: Channel, firstSample: Int64, pcm: Data)? {
        let bytes = Data(frame)
        guard bytes.count > frameHeaderBytes, bytes.count <= maxFrameBytes,
              (bytes.count - frameHeaderBytes) % 2 == 0,
              bytes[0] == protocolVersion, bytes[2] == 0, bytes[3] == 0, bytes[1] <= 1 else { return nil }
        var first: UInt64 = 0
        for index in 0..<8 { first |= UInt64(bytes[4 + index]) << UInt64(8 * index) }
        guard first <= UInt64(Int64.max) else { return nil }
        return (bytes[1] == 0 ? .mic : .system, Int64(first), bytes.subdata(in: frameHeaderBytes..<bytes.count))
    }

    // MARK: Server to client

    public struct Ready: Decodable, Equatable, Sendable {
        public struct Stream: Decodable, Equatable, Sendable {
            public var channel: Channel
            public var acceptFromSample: Int64
        }
        public var sessionId: String
        public var provider: String
        public var model: String
        public var streams: [Stream]
        public var resumeWindowMs: Int
        public var transcriptRevision: Int
    }

    public struct Ack: Decodable, Equatable, Sendable {
        public var channel: Channel
        /// One past the last sample the server has taken.
        public var throughSample: Int64
        public var transcribedThroughMs: Int
    }

    public struct Interim: Decodable, Equatable, Sendable {
        public var channel: Channel
        public var utteranceId: String
        public var rev: Int
        public var text: String
        public var startMs: Int
        public var endMs: Int
        public var speakerId: String?
        public var speakerLabel: String?
    }

    public struct Final: Decodable, Sendable {
        public var utteranceId: String
        public var transcriptRevision: Int
        public var segment: TranscriptSegmentDTO
    }

    public struct Speakers: Decodable, Equatable, Sendable {
        public struct Merge: Decodable, Equatable, Sendable {
            public var from: String
            public var to: String
        }
        /// A stored segment that now carries another speaker or label ("Me", "Room speaker N", "Speaker N").
        public struct Relabel: Decodable, Equatable, Sendable {
            public var segmentId: String
            public var speakerId: String
            public var speakerLabel: String
            public init(segmentId: String, speakerId: String, speakerLabel: String) {
                self.segmentId = segmentId; self.speakerId = speakerId; self.speakerLabel = speakerLabel
            }
        }
        public var merges: [Merge]
        /// Applied after `merges`. Absent when there are none.
        public var relabels: [Relabel]?
    }

    public struct Gap: Decodable, Equatable, Sendable {
        public var channel: Channel
        public var fromMs: Int
        public var toMs: Int
        /// `resume_window_exceeded`, `provider_unavailable`, or a later addition.
        public var reason: String
    }

    public enum StreamState: String, Sendable {
        case live, catchingUp = "catching_up", providerUnavailable = "provider_unavailable", paused
    }

    public struct Status: Decodable, Equatable, Sendable {
        public var channel: Channel
        /// Raw value; `streamState` is nil for a state this build does not know.
        public var state: String
        public var lagMs: Int
        public var streamState: StreamState? { StreamState(rawValue: state) }
    }

    public struct ServerError: Decodable, Equatable, Sendable {
        public var code: String
        public var message: String
        public var retryable: Bool
        public var retryAfterMs: Int?
    }

    public enum ServerMessage: Sendable {
        case ready(Ready), ack(Ack), interim(Interim), final(Final), speakers(Speakers)
        case gap(Gap), status(Status), error(ServerError)
        /// A message type added after this build. Ignored.
        case unknown(String)
    }

    private struct Envelope: Decodable { let type: String }

    public static func decode(_ text: String) throws -> ServerMessage {
        let data = Data(text.utf8)
        let decoder = JSONDecoder()
        do {
            switch try decoder.decode(Envelope.self, from: data).type {
            case "ready": return .ready(try decoder.decode(Ready.self, from: data))
            case "ack": return .ack(try decoder.decode(Ack.self, from: data))
            case "interim": return .interim(try decoder.decode(Interim.self, from: data))
            case "final": return .final(try decoder.decode(Final.self, from: data))
            case "speakers": return .speakers(try decoder.decode(Speakers.self, from: data))
            case "gap": return .gap(try decoder.decode(Gap.self, from: data))
            case "status": return .status(try decoder.decode(Status.self, from: data))
            case "error": return .error(try decoder.decode(ServerError.self, from: data))
            case let other: return .unknown(other)
            }
        } catch { throw APIClientError.invalidJSON }
    }

    // MARK: Close codes

    public enum CloseCode {
        public static let malformed = 4400
        public static let unauthenticated = 4401
        public static let notFound = 4404
        public static let clientTooSlow = 4408
        /// The server is restarting this stream, for one when its provider session was lost.
        /// Dial again at once and replay from the `ready` that follows.
        public static let serviceRestart = 1012
        public static let replaced = 4409
        public static let overLimit = 4429
        public static let providerUnavailable = 4503
    }
}
