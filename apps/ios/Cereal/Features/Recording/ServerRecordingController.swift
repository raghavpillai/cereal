import Foundation
import Observation
import APIClient
import Contracts
import os

/// One app-wide owner keeps a real recording alive when its note view is dismissed.
/// Local audio and delivery journals are scoped to the selected instance, outside the demo store.
///
/// A recording is transcribed by its durable parts, always. With the engine
/// capture path the parts are cut from one continuous capture while it grows,
/// and the same capture is also streamed to the server for live text. The live
/// stream is an addition: when it is absent, late or broken, nothing here waits
/// for it and the parts are delivered as they would be without it.
@MainActor @Observable
final class ServerRecordingController {
    /// Length of a delivery part cut from a continuous capture. Stored with each
    /// capture, so changing it never moves the parts of a capture already begun.
    static let partSeconds = 30

    private(set) var recorder = Recorder(simulateCapture: false)
    private(set) var activeNoteID: String?
    private(set) var instanceIdentity: String?
    private(set) var isBusy = false
    private(set) var isSyncing = false
    private(set) var status = ""
    private(set) var error: String?
    private(set) var deliveryRevision = 0
    /// The transcript of the note being recorded, kept current while it is
    /// recorded: stored text from the server, and with a live stream, the text
    /// still being spoken. Nil when no recording has been started here.
    private(set) var transcript: LiveTranscript?
    @ObservationIgnored private var outbox: AudioOutbox?
    @ObservationIgnored private var histories: [String: ServerCaptureHistory] = [:]
    @ObservationIgnored private var server: ServerNotesModel?
    @ObservationIgnored private var uploadTask: Task<Void, Never>?
    @ObservationIgnored private var uploads: Set<UUID> = []
    @ObservationIgnored private var polls: [String: (id: UUID, task: Task<Void, Never>)] = [:]
    @ObservationIgnored private var pollBudget: [String: Int] = [:]
    @ObservationIgnored private var controlTask: Task<Void, Never>?
    @ObservationIgnored private var live: LiveCaptureSession?
    @ObservationIgnored private var liveEnding: Task<Void, Never>?
    @ObservationIgnored private var rotating = false
    @ObservationIgnored private var rotationTask: Task<Void, Never>?
    @ObservationIgnored private var captureStorageFailed = false
    /// How long the last segment rotation kept the main actor, and so an upper
    /// bound on the audio not captured between the two segments. Recorder path only.
    @ObservationIgnored private(set) var lastRotationPause: TimeInterval?
    @ObservationIgnored private let log = Logger(subsystem: AppIdentity.subsystem, category: "recording")
    @ObservationIgnored private let outboxRoot: URL?
    @ObservationIgnored private let makeBackend: (@MainActor () -> any CaptureBackend)?
    @ObservationIgnored private let makeLiveClient: LiveClientFactory

    typealias LiveClientFactory = @MainActor (ServerNotesModel, String, any LiveAudioSource, LiveTranscriptClient.Configuration) throws -> LiveTranscriptClient

    /// The parameters replace storage, audio input and the live socket in tests.
    init(outboxRoot: URL? = nil, makeBackend: (@MainActor () -> any CaptureBackend)? = nil, makeLiveClient: LiveClientFactory? = nil) {
        self.outboxRoot = outboxRoot
        self.makeBackend = makeBackend
        self.makeLiveClient = makeLiveClient ?? { server, noteID, source, configuration in
            try server.liveTranscriptClient(noteID: noteID, source: source, configuration: configuration)
        }
    }

    var isCapturing: Bool { recorder.isCapturing }
    /// The note whose recording the indicator on Home and on the note stands
    /// for, or nil when there is no recording to return to.
    var indicatorNoteID: String? {
        CaptureNavigation.showsIndicator(activeNoteID: activeNoteID, state: recorder.state) ? activeNoteID : nil
    }
    /// Completed writes of the delivery journal, for measurement.
    var outboxJournalWrites: Int { outbox?.journalWrites ?? 0 }

    func hasPendingAudio(noteID: String) -> Bool {
        _ = deliveryRevision
        return outbox?.entries(noteID: noteID).contains { $0.state != .ready } ?? false
    }

    /// The growing transcript of `noteID`, when this installation is recording it or just did.
    func transcript(for noteID: String) -> LiveTranscript? {
        transcript?.noteID == noteID ? transcript : nil
    }

    func attach(to server: ServerNotesModel) async {
        guard instanceIdentity != server.accountIdentity else { return }
        // Native Stop is synchronous and delivers its journal before changing connection ownership.
        recorder.stopSegment()
        rotationTask?.cancel(); rotationTask = nil
        uploadTask?.cancel(); uploadTask = nil
        polls.values.forEach { $0.task.cancel() }
        polls = [:]; pollBudget = [:]; uploads = []
        controlTask?.cancel(); controlTask = nil
        live?.cancel(); live = nil; liveEnding = nil
        transcript = nil
        isSyncing = false
        self.server = server
        activeNoteID = nil
        outbox = nil
        histories = [:]
        instanceIdentity = server.accountIdentity
        error = nil
        guard let instance = server.instance, let principalID = server.principalID, let scope = server.accountIdentity else { return }
        do {
            let queue = try AudioOutbox(instance: instance, principalID: principalID, root: outboxRoot)
            outbox = queue
            let ledger = queue.directory.appendingPathComponent("captures.json")
            if FileManager.default.fileExists(atPath: ledger.path) {
                histories = try JSONDecoder().decode([String: ServerCaptureHistory].self, from: Data(contentsOf: ledger))
            }
            try recoverCaptures()
            deliveryRevision += 1
            // Replay only this installation's recorded Stop intent. Do not stop arbitrary notes.
            for (id, history) in histories where history.needsStop == true {
                do {
                    _ = try await server.recording(noteID: id, command: .stop)
                    guard !Task.isCancelled, scope == instanceIdentity else { return }
                    histories[id]?.needsStop = false
                    try saveHistories()
                } catch {
                    guard !Task.isCancelled, scope == instanceIdentity else { return }
                    self.error = "A saved Stop is waiting for a connection. \(error.localizedDescription)"
                }
            }
            for id in Set(queue.entries.filter { $0.state != .ready }.map(\.noteID)) {
                guard !Task.isCancelled, scope == instanceIdentity else { return }
                await deliver(noteID: id, retryFailures: false)
            }
        } catch { self.error = "Saved recording delivery could not be recovered. Original audio is preserved. \(error.localizedDescription)" }
    }

    func start(note: NoteDTO) async {
        guard !isBusy, !recorder.isCapturing, let server, let outbox,
              instanceIdentity == server.accountIdentity, let uuid = UUID(uuidString: note.id) else { return }
        isBusy = true
        error = nil
        activeNoteID = note.id
        captureStorageFailed = false
        let scope = instanceIdentity
        defer { isBusy = false }
        // Only audio from the device's input needs the microphone permission.
        let needsMicrophone = (makeBackend ?? CaptureEnginePolicy.makeBackend)().usesMicrophone
        let permitted = await MicPermission.isGranted
        let allowed = permitted || !needsMicrophone ? true : await MicPermission.request()
        guard !Task.isCancelled, scope == instanceIdentity, instanceIdentity == server.accountIdentity else { return }
        guard allowed else {
            error = "Allow microphone access in Settings before starting a recording."
            return
        }
        do {
            // A previous Stop must reach the server before Resume, but transcription need not finish.
            await controlTask?.value
            try Task.checkCancellation()
            let freshNote = try await server.note(id: note.id)
            guard scope == instanceIdentity else { throw CancellationError() }
            let remoteChunks = try await server.audioChunks(noteID: note.id)
            guard scope == instanceIdentity else { throw CancellationError() }
            if histories[note.id] == nil {
                histories[note.id] = ServerCaptureHistory(baseOffsetMs: Self.durationMs(freshNote),
                    nextSequence: (remoteChunks.filter { $0.channel == .mic }.map(\.sequence).max() ?? -1) + 1, segments: [])
                try saveHistories()
            }
            try stageHistory(noteID: note.id) // Repair any prior local queue write before a new capture starts.
            histories[note.id]!.prepareForResume(remoteChunks: remoteChunks,
                remoteDurationMs: Self.durationMs(freshNote), localEndMs: outbox.endMs(noteID: note.id))
            try saveHistories()
            let history = histories[note.id]!
            _ = try await server.recording(noteID: note.id, command: freshNote.recordingState == .idle ? .start : .resume)
            guard scope == instanceIdentity, instanceIdentity == server.accountIdentity else { throw CancellationError() }
            let root = outbox.directory.appendingPathComponent("recordings", isDirectory: true)
            live?.cancel(); live = nil
            recorder = Recorder(simulateCapture: false, recordingRoot: root, backend: makeBackend)
            activeNoteID = note.id
            var elapsedSegments = history.segments
            let baselineMs = max(0, (history.nextStartMs ?? history.baseOffsetMs) - Int(history.segments.reduce(0) { $0 + $1.duration } * 1000))
            if baselineMs > 0 {
                elapsedSegments.insert(CaptureSegment(index: -1, startedAt: .now,
                    duration: Double(baselineMs) / 1000, fileName: "remote-duration"), at: 0)
            }
            recorder.prepare(existing: elapsedSegments)
            recorder.onSegmentFinished = { [weak self] segment, file in self?.finished(segment, file: file, noteID: note.id) }
            recorder.partSeconds = Self.capturePartSeconds
            recorder.onPartBoundary = { [weak self] frames in self?.stageOpenCapture(noteID: note.id, frames: frames) }
            recorder.onPauseChanged = { [weak self] paused in self?.pauseChanged(paused) }
            let stored = LiveTranscript(noteID: note.id)
            stored.merge(note: freshNote)
            transcript = stored
            recorder.startSegment(noteID: uuid, title: note.title)
            if case .failed(let message) = recorder.state {
                scheduleDelivery(noteID: note.id, stopServer: true)
                error = message
            } else {
                status = "Recording on device · Transcript updates after each audio chunk"
                // The one place the two capture paths part ways.
                if recorder.capturesContinuously { beginContinuousCapture(noteID: note.id, transcript: stored) }
                else { scheduleRotation(noteID: note.id, uuid: uuid, title: note.title) }
            }
        } catch is CancellationError { }
        catch { self.error = error.localizedDescription }
    }

    func stop() {
        rotationTask?.cancel(); rotationTask = nil
        let finished = recorder.stopSegment()
        // Zero-frame starts still need a server Stop, without creating an upload or transcript.
        if finished == nil, let id = activeNoteID {
            // A stream still open here belongs to a capture that holds nothing, so its length is 0.
            // After an interruption the stream already ended at the capture's length and this is a no-op.
            endLive(atSample: 0)
            scheduleDelivery(noteID: id, stopServer: true)
        }
    }

    func retry(noteID: String) {
        guard !isSyncing else { return }
        error = nil
        scheduleDelivery(noteID: noteID, stopServer: activeNoteID != noteID || !recorder.isCapturing, retryFailures: true)
    }

    func forgetDeletedNote(_ noteID: String) throws {
        guard !(activeNoteID == noteID && recorder.isCapturing), let outbox else { return }
        uploadTask?.cancel(); uploadTask = nil
        polls.removeValue(forKey: noteID)?.task.cancel()
        refreshSyncing()
        try outbox.discard(noteID: noteID)
        histories.removeValue(forKey: noteID)
        try saveHistories()
        if let uuid = UUID(uuidString: noteID) {
            let folder = outbox.directory.appendingPathComponent("recordings/\(uuid.uuidString)", isDirectory: true)
            if FileManager.default.fileExists(atPath: folder.path) { try FileManager.default.removeItem(at: folder) }
        }
        if activeNoteID == noteID { activeNoteID = nil }
        if transcript?.noteID == noteID { transcript = nil }
        deliveryRevision += 1
    }

    // MARK: Delivery

    /// True while the note is being recorded here. Delivery then works quietly:
    /// the recording surface has its own status and must not flicker.
    private func isRecording(_ noteID: String) -> Bool { activeNoteID == noteID && recorder.isCapturing }

    /// The one status line is about the note on screen. While a note is being
    /// recorded it says how that recording is transcribed, and the delivery of
    /// another note's saved audio does not write over it.
    private func showsDeliveryStatus(_ noteID: String) -> Bool {
        !isRecording(noteID) && !(recorder.isCapturing && activeNoteID != noteID)
    }

    private func refreshSyncing() {
        let value = !uploads.isEmpty || !polls.isEmpty
        if isSyncing != value { isSyncing = value }
    }

    /// Uploads every staged part the server does not have yet, in order. It
    /// never waits for transcription: the next part can follow at once.
    private func deliver(noteID: String, retryFailures: Bool) async {
        guard let server, let outbox, instanceIdentity == server.accountIdentity else { return }
        let token = UUID()
        uploads.insert(token)
        refreshSyncing()
        let scope = instanceIdentity
        defer { uploads.remove(token); refreshSyncing() }
        do {
            try stageHistory(noteID: noteID)
            let waiting = outbox.entries(noteID: noteID).filter { $0.state != .ready }
            guard !waiting.isEmpty else { return }
            var chunks = try await server.audioChunks(noteID: noteID)
            for entry in waiting {
                try Task.checkCancellation()
                guard scope == instanceIdentity else { throw CancellationError() }
                let known = chunks.first { $0.id == entry.id }
                let receipt: AudioChunkDTO
                if let known {
                    receipt = known.state == .failed && retryFailures
                        ? try await server.retryAudio(noteID: noteID, chunkID: entry.id) : known
                } else {
                    if showsDeliveryStatus(noteID) { status = "Uploading saved audio…" }
                    receipt = try await server.upload(entry, from: outbox)
                }
                let state = Self.deliveryState(receipt)
                if state != entry.state { try outbox.update(id: entry.id, state: state) }
                chunks.removeAll { $0.id == receipt.id }; chunks.append(receipt)
                deliveryRevision += 1
            }
            follow(noteID: noteID)
        } catch is CancellationError { }
        catch {
            // While recording, the next part boundary tries again; the capture itself is unaffected.
            if !isRecording(noteID) {
                self.error = error.localizedDescription
                if showsDeliveryStatus(noteID) { status = "Audio kept on this device · Retry delivery when connected" }
            } else {
                log.notice("Part upload will be retried: \(error.localizedDescription, privacy: .public)")
            }
        }
    }

    /// Starts, or extends, the one task that watches the server transcribe this note's parts.
    private func follow(noteID: String) {
        pollBudget[noteID] = 120
        guard polls[noteID] == nil else { return }
        let id = UUID()
        let scope = instanceIdentity
        polls[noteID] = (id, Task { [weak self] in
            await self?.poll(noteID: noteID, scope: scope)
            guard let self, polls[noteID]?.id == id else { return }
            polls[noteID] = nil
            refreshSyncing()
        })
        refreshSyncing()
    }

    private func poll(noteID: String, scope: String?) async {
        guard let server, let outbox else { return }
        do {
            if showsDeliveryStatus(noteID) { status = "Transcribing and identifying speakers…" }
            while (pollBudget[noteID] ?? 0) > 0 {
                pollBudget[noteID]! -= 1
                try Task.checkCancellation()
                guard scope == instanceIdentity else { throw CancellationError() }
                let chunks = try await server.audioChunks(noteID: noteID)
                guard scope == instanceIdentity else { throw CancellationError() }
                var changed = false
                let local = Dictionary(uniqueKeysWithValues: outbox.entries(noteID: noteID).map { ($0.id, $0.state) })
                for receipt in chunks {
                    guard let current = local[receipt.id] else { continue }
                    let state = Self.deliveryState(receipt)
                    if state != current { try outbox.update(id: receipt.id, state: state); changed = true }
                }
                if changed { deliveryRevision += 1 }
                let working = chunks.contains { $0.state == .queued || $0.state == .processing }
                let failed = chunks.contains { $0.state == .failed }
                var document: NoteDTO?
                // While a live stream is carrying this recording, its finals are the stored text and
                // it asks for a note read itself when the server's revision moves without it. Reading
                // the note here as well would only repeat that, once per part.
                let streamed = isRecording(noteID) && (transcript(for: noteID).map { LiveStatusText.isStreaming($0.status) } ?? false)
                if (changed || failed || !working) && !streamed {
                    // getNote returns the server source of truth; update the list through refresh after completion.
                    // While the note is being recorded this is a private read: the transcript
                    // model keeps the result, and nothing that shows the list has to redraw.
                    document = isRecording(noteID) ? try await server.noteSnapshot(id: noteID) : try await server.note(id: noteID)
                    guard scope == instanceIdentity else { throw CancellationError() }
                    if let document { transcript(for: noteID)?.merge(note: document) }
                }
                if failed {
                    error = "Some audio could not be transcribed. Saved chunks can be retried without recording again."
                    if showsDeliveryStatus(noteID) { status = "Audio kept · Transcription needs retry" }
                    return
                }
                if !working {
                    if !isRecording(noteID) {
                        if showsDeliveryStatus(noteID) { status = document?.transcriptSegments.isEmpty == true ? "Audio saved · No speech detected" : "Transcript updated" }
                        await server.refresh()
                    }
                    return
                }
                try await Task.sleep(for: .seconds(2))
            }
            if showsDeliveryStatus(noteID) { status = "Audio saved on server · Transcription is still processing" }
        } catch is CancellationError { }
        catch {
            if !isRecording(noteID) {
                self.error = error.localizedDescription
                if showsDeliveryStatus(noteID) { status = "Audio kept on this device · Retry delivery when connected" }
            }
        }
    }

    private func finished(_ segment: CaptureSegment, file: URL, noteID: String) {
        // Every way a capture ends comes through here (Stop, an interruption, a failed route), after
        // its file is closed. `capturedFrames` is then the length of the capture, which is what
        // `mark: end` must carry, however much of it the stream has sent.
        endLive(atSample: recorder.capturedFrames)
        guard histories[noteID] != nil else { error = APIClientError.localStorage.localizedDescription; return }
        histories[noteID]!.append(segment)
        if rotating, let gap = CaptureContinuity.gaps(in: histories[noteID]!.segments).last, gap.afterIndex == segment.index - 1 {
            log.notice("Audio between segments \(gap.afterIndex, privacy: .public) and \(segment.index, privacy: .public) was not captured: \(Int(gap.seconds * 1000), privacy: .public) ms")
        }
        do {
            try saveHistories()
            try stageHistory(noteID: noteID)
            CaptureRecovery.acknowledge(CaptureRecovery.journalURL(for: file))
            deliveryRevision += 1
        } catch { self.error = error.localizedDescription; captureStorageFailed = true }
        scheduleDelivery(noteID: noteID, stopServer: !rotating)
    }

    private func scheduleDelivery(noteID: String, stopServer: Bool, retryFailures: Bool = false) {
        let scope = instanceIdentity
        if stopServer {
            let previousControl = controlTask
            let ending = liveEnding
            controlTask = Task { [weak self] in
                await previousControl?.value
                // The live stream hands over its last words before the note stops recording.
                await ending?.value
                guard let self, !Task.isCancelled, scope == instanceIdentity, let server else { return }
                do {
                    _ = try await server.recording(noteID: noteID, command: .stop)
                    guard scope == instanceIdentity else { return }
                    histories[noteID]?.needsStop = false
                    try saveHistories()
                } catch { self.error = error.localizedDescription }
            }
        }
        // Uploads are one ordered chain. Watching transcription is a separate task, so a
        // slow transcription never holds back the next part.
        let previous = uploadTask
        uploadTask = Task { [weak self] in
            await previous?.value
            guard let self, !Task.isCancelled, scope == instanceIdentity, server != nil else { return }
            await deliver(noteID: noteID, retryFailures: retryFailures)
        }
    }

    /// Stages every finished capture of the note that is not fully in the outbox.
    /// A recorder segment is copied in two-minute parts, as it always was. A
    /// continuous capture is cut on its own part length; parts cut while it was
    /// recording are recognised and only the rest is added.
    private func stageHistory(noteID: String) throws {
        guard let outbox, var history = histories[noteID], let uuid = UUID(uuidString: noteID) else { return }
        let folder = outbox.directory.appendingPathComponent("recordings/\(uuid.uuidString)", isDirectory: true)
        var offset = history.baseOffsetMs
        var changed = false
        for segment in history.segments.sorted(by: { $0.index < $1.index }) {
            let capturedOffset = history.segmentOffsets?[segment.fileName] ?? offset
            let file = folder.appendingPathComponent(segment.fileName)
            let sequence = max(history.nextSequence, outbox.nextSequence(noteID: noteID))
            var staged: [AudioOutboxEntry] = []
            if ContinuousCapture.isCaptureName(segment.fileName) {
                if let frames = ContinuousCapture.frames(inFile: file) {
                    staged = try Signpost.interval("Outbox.Stage") {
                        try outbox.stageParts(noteID: noteID, capture: file, availableFrames: frames,
                            partFrames: partSeconds(of: segment.fileName, noteID: noteID) * ContinuousCapture.sampleRate,
                            isFinal: true, startMs: capturedOffset, sequence: sequence)
                    }
                } else if !outbox.containsSource(noteID: noteID, fileName: segment.fileName) { throw APIClientError.invalidAudio }
            } else if !outbox.containsSource(noteID: noteID, fileName: segment.fileName) {
                staged = try Signpost.interval("Outbox.Stage") {
                    try outbox.stage(noteID: noteID, file: file, startMs: capturedOffset, sequence: sequence)
                }
            }
            if let last = staged.map(\.sequence).max() { history.nextSequence = last + 1; changed = true }
            offset = capturedOffset + Int(segment.duration * 1000)
        }
        guard changed else { return }
        histories[noteID] = history
        try saveHistories()
    }

    private func recoverCaptures() throws {
        guard let outbox else { return }
        let root = outbox.directory.appendingPathComponent("recordings", isDirectory: true)
        for capture in CaptureRecovery.scan(roots: [root]) {
            let id = capture.noteID.uuidString.lowercased()
            guard histories[id] != nil else { continue }
            guard let segment = capture.segment else {
                error = "An interrupted recording could not be read completely. Its original audio and recovery journal are still on this device."
                continue
            }
            histories[id]!.append(segment)
            try saveHistories()
            try stageHistory(noteID: id)
            CaptureRecovery.acknowledge(capture.journalFile)
        }
    }

    private func saveHistories() throws {
        guard let outbox else { throw APIClientError.localStorage }
        let url = outbox.directory.appendingPathComponent("captures.json")
        try JSONEncoder().encode(histories).write(to: url, options: [.atomic, .completeFileProtectionUntilFirstUserAuthentication])
        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: url.path)
        PerfProbe.hit("Captures.Save")
    }

    // MARK: Continuous capture (engine path)

    static var capturePartSeconds: Int {
        #if DEBUG
        // QA: `-capture.partSeconds 10` crosses part boundaries sooner.
        let override = UserDefaults.standard.integer(forKey: "capture.partSeconds")
        if (2...120).contains(override) { return override }
        #endif
        return partSeconds
    }

    /// The part length a continuous capture was begun with. If the ledger lost it
    /// (a kill between starting the capture and saving the ledger), it is read
    /// back from the parts already cut, so later parts keep the same boundaries.
    private func partSeconds(of fileName: String, noteID: String) -> Int {
        if let stored = histories[noteID]?.partSeconds?[fileName], stored > 0 { return stored }
        let cut = outbox?.entries(noteID: noteID).filter { $0.sourceFileName == fileName }.map(\.durationMs).max() ?? 0
        return cut >= 1000 && cut % 1000 == 0 ? cut / 1000 : Self.capturePartSeconds
    }

    /// Records where the capture sits on the note timeline before any part of
    /// it is cut, then starts the live stream from the same file.
    private func beginContinuousCapture(noteID: String, transcript: LiveTranscript) {
        guard let file = recorder.currentFileURL, histories[noteID] != nil else { return }
        let baseMs = histories[noteID]!.begin(fileName: file.lastPathComponent, partSeconds: Self.capturePartSeconds)
        do { try saveHistories() } catch { self.error = error.localizedDescription }
        startLive(noteID: noteID, file: file, baseMs: baseMs, transcript: transcript)
    }

    /// The capture grew past a part boundary: cut the part and send it.
    private func stageOpenCapture(noteID: String, frames: Int64) {
        guard isRecording(noteID), let outbox, let history = histories[noteID], let file = recorder.currentFileURL,
              let baseMs = history.segmentOffsets?[file.lastPathComponent] else { return }
        do {
            let staged = try Signpost.interval("Outbox.Stage") {
                try outbox.stageParts(noteID: noteID, capture: file, availableFrames: Int(frames),
                    partFrames: partSeconds(of: file.lastPathComponent, noteID: noteID) * ContinuousCapture.sampleRate,
                    isFinal: false, startMs: baseMs, sequence: max(history.nextSequence, outbox.nextSequence(noteID: noteID)))
            }
            guard !staged.isEmpty else { return }
            deliveryRevision += 1
            scheduleDelivery(noteID: noteID, stopServer: false)
        } catch {
            // The capture file is intact and is cut again at the next boundary and at Stop.
            self.error = error.localizedDescription
        }
    }

    private func startLive(noteID: String, file: URL, baseMs: Int, transcript: LiveTranscript) {
        guard let server, let source = try? CaptureFileReader(url: file) else { return }
        let configuration = LiveTranscriptClient.Configuration(captureID: "\(noteID)/\(file.lastPathComponent)", baseMs: baseMs)
        guard let client = try? makeLiveClient(server, noteID, source, configuration) else { return }
        let scope = instanceIdentity
        let session = LiveCaptureSession(client: client, transcript: transcript) { [weak self] in
            guard let self, scope == instanceIdentity, let server = self.server else { return nil }
            return try? await server.noteSnapshot(id: noteID)
        }
        session.onStatus = { [weak self] value in
            guard let self, isRecording(noteID) else { return }
            // One line under the note says how the transcript is being made.
            switch value {
            case .live: status = "Recording on device · Live transcript"
            case .unavailable: status = "Recording on device · Transcript updates after each audio chunk"
            default: break
            }
        }
        live = session
        session.start()
    }

    private func pauseChanged(_ paused: Bool) {
        if paused { live?.pause(atSample: recorder.capturedFrames) } else { live?.resume() }
    }

    /// Finishes the live stream. The server's Stop waits for this, bounded by the stream's own limit.
    private func endLive(atSample sample: Int64) {
        guard let session = live else { return }
        live = nil
        liveEnding = session.end(atSample: sample)
    }

    // MARK: Recorder path (legacy, behind CaptureEnginePolicy)

    private func scheduleRotation(noteID: String, uuid: UUID, title: String) {
        rotationTask?.cancel()
        rotationTask = Task { [weak self] in
            while !Task.isCancelled {
                do { try await Task.sleep(for: .seconds(120)) } catch { return }
                guard let self, activeNoteID == noteID, recorder.state == .recording else { return }
                // Measurement only: the order of operations below is unchanged.
                let interval = Signpost.ui.beginInterval("Capture.Rotate")
                let began = ProcessInfo.processInfo.systemUptime
                rotating = true
                recorder.stopSegment(keepingLiveActivity: true)
                rotating = false
                guard !captureStorageFailed else {
                    Signpost.ui.endInterval("Capture.Rotate", interval)
                    recorder.stopSegment() // End the retained indicator when rotation cannot continue.
                    scheduleDelivery(noteID: noteID, stopServer: true)
                    return
                }
                recorder.startSegment(noteID: uuid, title: title)
                Signpost.ui.endInterval("Capture.Rotate", interval)
                noteRotationPause(ProcessInfo.processInfo.systemUptime - began)
                if case .failed(let message) = recorder.state {
                    error = message
                    scheduleDelivery(noteID: noteID, stopServer: true)
                    return
                }
            }
        }
    }

    private func noteRotationPause(_ seconds: TimeInterval) {
        lastRotationPause = seconds
        log.notice("Segment rotation held capture for \(Int(seconds * 1000), privacy: .public) ms")
    }

    // MARK: Helpers

    private static func deliveryState(_ receipt: AudioChunkDTO) -> AudioOutboxEntry.DeliveryState {
        switch receipt.state {
        case .ready: .ready
        case .failed: .failed
        case .queued, .processing: .acknowledged
        }
    }

    private static func durationMs(_ note: NoteDTO) -> Int {
        let parts = note.duration.split(separator: ":").compactMap { Double($0) }
        let duration: Double = parts.count == 2 ? parts[0] * 60 + parts[1] : parts.count == 3 ? parts[0] * 3600 + parts[1] * 60 + parts[2] : 0
        return max(Int(duration * 1000), note.transcriptSegments.map { $0.endMs ?? $0.startMs }.max() ?? 0)
    }
}


/// Capture offsets are immutable once staged. A different device may advance the note
/// while this installation is stopped; only the next local segment moves forward.
struct ServerCaptureHistory: Codable {
    var baseOffsetMs: Int
    var nextSequence: Int
    var segments: [CaptureSegment]
    var segmentOffsets: [String: Int]?
    var nextStartMs: Int?
    var needsStop: Bool?
    /// Part length, in seconds, of each continuous capture, fixed when it begins.
    var partSeconds: [String: Int]?

    /// A continuous capture is about to be recorded into a new file. Its place
    /// on the note timeline is fixed now, because parts are cut from it and its
    /// audio is streamed while it is still being recorded. Returns that place.
    mutating func begin(fileName: String, partSeconds seconds: Int) -> Int {
        let offset = nextStartMs ?? (baseOffsetMs + Int(segments.reduce(0) { $0 + $1.duration } * 1000))
        if segmentOffsets == nil { segmentOffsets = [:] }
        segmentOffsets![fileName] = offset
        if partSeconds == nil { partSeconds = [:] }
        partSeconds![fileName] = seconds
        return offset
    }

    mutating func prepareForResume(remoteChunks: [AudioChunkDTO], remoteDurationMs: Int, localEndMs: Int) {
        nextSequence = max(nextSequence, (remoteChunks.filter { $0.channel == .mic }.map(\.sequence).max() ?? -1) + 1)
        nextStartMs = max(localEndMs, remoteDurationMs, remoteChunks.map { $0.startMs + $0.durationMs }.max() ?? 0)
        needsStop = true
    }

    mutating func append(_ segment: CaptureSegment) {
        guard !segments.contains(where: { $0.fileName == segment.fileName }) else { return }
        // A continuous capture was placed when it began; anything else is placed now.
        let offset = (partSeconds?[segment.fileName] != nil ? segmentOffsets?[segment.fileName] : nil)
            ?? nextStartMs ?? (baseOffsetMs + Int(segments.reduce(0) { $0 + $1.duration } * 1000))
        if segmentOffsets == nil { segmentOffsets = [:] }
        segmentOffsets![segment.fileName] = offset
        nextStartMs = offset + Int(segment.duration * 1000)
        segments.append(segment)
    }
}

/// Detects audio that was not captured between consecutive segments of one
/// recording. Each segment records when its recorder started and how much
/// audio its file holds, so any time between the end of one file and the
/// start of the next is audio that no file contains.
enum CaptureContinuity {
    struct Gap: Equatable {
        let afterIndex: Int
        let seconds: TimeInterval
    }

    /// Gaps longer than `tolerance` between segments that follow each other
    /// within `sameSession` seconds (a rotation, not a later Resume).
    static func gaps(in segments: [CaptureSegment], tolerance: TimeInterval = 0.02, sameSession: TimeInterval = 5) -> [Gap] {
        let ordered = segments.sorted { $0.index < $1.index }
        var result: [Gap] = []
        for (previous, next) in zip(ordered, ordered.dropFirst()) {
            let end = previous.startedAt.addingTimeInterval(previous.duration)
            let missing = next.startedAt.timeIntervalSince(end)
            if missing > tolerance, missing < sameSession { result.append(Gap(afterIndex: previous.index, seconds: missing)) }
        }
        return result
    }

    static func lostSeconds(in segments: [CaptureSegment]) -> TimeInterval {
        gaps(in: segments).reduce(0) { $0 + $1.seconds }
    }
}
