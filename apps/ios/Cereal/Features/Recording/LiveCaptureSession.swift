import Foundation
import APIClient
import Contracts

/// How a capture's stream ended.
enum LiveDrain: Equatable, Sendable {
    /// The server confirmed every sample of the capture.
    case delivered
    /// The server refused the stream for good before it had all of it.
    case refused(LiveUnavailableReason)
    /// The stream was cancelled (its account went) before it had delivered everything.
    case interrupted
}

/// The stream of a capture that has just ended, and the capture's file name.
struct LiveEnding {
    let fileName: String?
    let task: Task<LiveDrain, Never>
}

/// One capture's live stream, seen from the main actor: it starts the client,
/// applies what the server says to the note's growing transcript, and keeps
/// pause, resume and end in the order they happened.
@MainActor
final class LiveCaptureSession {
    let transcript: LiveTranscript
    private let client: LiveTranscriptClient
    private let readNote: @MainActor () async -> NoteDTO?
    private var pump: Task<Void, Never>?
    private var commands: Task<Void, Never>?
    private var reading: Task<Void, Never>?
    private var readAgain = false

    /// `readNote` returns the note from the server, for stored text the stream did not carry.
    init(client: LiveTranscriptClient, transcript: LiveTranscript, readNote: @escaping @MainActor () async -> NoteDTO?) {
        self.client = client
        self.transcript = transcript
        self.readNote = readNote
    }

    func start() {
        let events = client.events
        // The pump keeps this session alive until the stream has finished, so the
        // server's last text is applied even after the recorder has let go of it.
        pump = Task { for await event in events { self.handle(event) } }
        // The stream is the recording's only transcript, so it runs on any network path, Low Data Mode included.
        enqueue { await $0.start() }
    }

    func pause(atSample sample: Int64) { enqueue { await $0.pause(atSample: sample) } }
    func resume() { enqueue { await $0.resume() } }

    /// Sends the rest of the capture, paced, and waits until the server has
    /// confirmed all of it and sent its last text, for as long as that takes.
    /// The returned task finishes when the stream has, with how it ended.
    func end(atSample sample: Int64) -> Task<LiveDrain, Never> {
        enqueue { await $0.end(atSample: sample) }
        let last = commands
        let pump = pump
        let client = client
        return Task {
            await last?.value
            await pump?.value // every event the server sent has been applied
            if await client.delivered { return .delivered }
            if case .unavailable(let reason) = await client.status { return .refused(reason) }
            return .interrupted
        }
    }

    /// Stops at once, also in the middle of `end`.
    func cancel() {
        Task { [client] in await client.cancel() }
    }

    private func enqueue(_ command: @escaping @Sendable (LiveTranscriptClient) async -> Void) {
        let previous = commands
        commands = Task { [client] in
            await previous?.value
            await command(client)
        }
    }

    private func handle(_ event: LiveTranscriptEvent) {
        if transcript.apply(event) { readStoredTranscript() }
    }

    /// One read at a time; a request that arrives during a read runs once after it.
    private func readStoredTranscript() {
        guard reading == nil else { readAgain = true; return }
        reading = Task { [weak self] in
            guard let self else { return }
            repeat {
                readAgain = false
                if let note = await readNote() { transcript.merge(note: note) }
            } while readAgain && !Task.isCancelled
            reading = nil
        }
    }
}
