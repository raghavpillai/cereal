import Foundation
import Network
import APIClient
import Contracts

/// One capture's live stream, seen from the main actor: it starts the client,
/// applies what the server says to the note's growing transcript, and keeps
/// pause, resume and end in the order they happened.
@MainActor
final class LiveCaptureSession {
    let transcript: LiveTranscript
    var onStatus: ((LiveTranscriptStatus) -> Void)?
    private let client: LiveTranscriptClient
    private let readNote: @MainActor () async -> NoteDTO?
    private var pump: Task<Void, Never>?
    private var commands: Task<Void, Never>?
    private var reading: Task<Void, Never>?
    private var readAgain = false
    private var monitor: NWPathMonitor?

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
        enqueue { await $0.start() }
        // Low Data Mode: the stream waits and the durable parts carry the recording.
        let monitor = NWPathMonitor()
        monitor.pathUpdateHandler = { [client] path in
            let constrained = path.isConstrained
            Task { await client.setConstrained(constrained) }
        }
        monitor.start(queue: .global(qos: .utility))
        self.monitor = monitor
    }

    func pause(atSample sample: Int64) { enqueue { await $0.pause(atSample: sample) } }
    func resume() { enqueue { await $0.resume() } }

    /// Sends the rest of the capture and waits, for a bounded time, for the
    /// server's last text. The returned task finishes when the stream has.
    func end(atSample sample: Int64) -> Task<Void, Never> {
        monitor?.cancel(); monitor = nil
        enqueue { await $0.end(atSample: sample) }
        let last = commands
        let pump = pump
        return Task {
            await last?.value
            await pump?.value // every event the server sent has been applied
        }
    }

    func cancel() {
        monitor?.cancel(); monitor = nil
        enqueue { await $0.cancel() }
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
        if case .status(let status) = event { onStatus?(status) }
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
