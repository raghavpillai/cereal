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
/// What one account needs to get its recordings to its own server: its
/// outbox, its capture ledger and the tasks that upload and watch. Nothing in
/// one is ever read or sent through another account.
@MainActor
final class RecordingDelivery {
    let account: AccountID
    let server: ServerNotesModel
    let outbox: AudioOutbox
    var histories: [String: ServerCaptureHistory] = [:]
    var uploadTask: Task<Void, Never>?
    var uploads: Set<UUID> = []
    var polls: [String: (id: UUID, task: Task<Void, Never>)] = [:]
    var pollBudget: [String: Int] = [:]
    var controlTask: Task<Void, Never>?
    /// The session of the account that this delivery last queued its unsent audio for.
    var generation: Int
    /// True once the account has gone. Work begun before that ends without touching anything.
    private(set) var closed = false

    init(account: AccountID, server: ServerNotesModel, outbox: AudioOutbox) {
        self.account = account; self.server = server; self.outbox = outbox
        generation = server.sessionGeneration
    }

    func close() {
        closed = true
        uploadTask?.cancel(); uploadTask = nil
        polls.values.forEach { $0.task.cancel() }
        polls = [:]; pollBudget = [:]; uploads = []
        controlTask?.cancel(); controlTask = nil
    }

    func saveHistories() throws {
        let url = outbox.directory.appendingPathComponent("captures.json")
        try JSONEncoder().encode(histories).write(to: url, options: [.atomic, .completeFileProtectionUntilFirstUserAuthentication])
        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: url.path)
        PerfProbe.hit("Captures.Save")
    }
}

@MainActor @Observable
final class ServerRecordingController {
    /// Length of a delivery part cut from a continuous capture. Stored with each
    /// capture, so changing it never moves the parts of a capture already begun.
    static let partSeconds = 30

    private(set) var recorder = Recorder(simulateCapture: false)
    /// The note being recorded, or just recorded, with the account it belongs to.
    private(set) var active: NoteRef?
    private(set) var isBusy = false
    private(set) var isSyncing = false
    private(set) var status = ""
    private(set) var error: String?
    private(set) var deliveryRevision = 0
    /// The transcript of the note being recorded, kept current while it is
    /// recorded: stored text from the server, and with a live stream, the text
    /// still being spoken. Nil when no recording has been started here.
    private(set) var transcript: LiveTranscript?
    /// One delivery per account, in place of the single outbox there used to be.
    @ObservationIgnored private var deliveries: [AccountID: RecordingDelivery] = [:]
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
    /// What the Live Activity may say about the account, when there is more than one to tell apart.
    @ObservationIgnored var accountPresentation: (@MainActor (AccountID) -> (label: String, host: String)?)?

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
    var indicator: NoteRef? {
        CaptureNavigation.showsIndicator(activeNoteID: active?.id, state: recorder.state) ? active : nil
    }
    /// Completed writes of the delivery journals, for measurement.
    var outboxJournalWrites: Int { deliveries.values.reduce(0) { $0 + $1.outbox.journalWrites } }

    func isActive(_ ref: NoteRef?) -> Bool { ref != nil && active == ref }
    func isIndicator(_ ref: NoteRef?) -> Bool { ref != nil && indicator == ref }

    func hasPendingAudio(_ ref: NoteRef?) -> Bool {
        _ = deliveryRevision
        guard let ref, let delivery = deliveries[ref.account] else { return false }
        return delivery.outbox.entries(noteID: ref.id).contains { $0.state != .ready }
    }

    /// True for a recording whose note its server says was deleted. Its audio is kept on this device.
    func isDeletedOnServer(_ ref: NoteRef?) -> Bool {
        _ = deliveryRevision
        guard let ref, let delivery = deliveries[ref.account] else { return false }
        return delivery.histories[ref.id]?.serverMissingSince != nil
    }

    /// The growing transcript of the note, when this installation is recording it or just did.
    func transcript(for ref: NoteRef?) -> LiveTranscript? {
        guard let ref, active == ref || transcriptOwner == ref else { return nil }
        return transcript?.noteID == ref.id ? transcript : nil
    }
    @ObservationIgnored private var transcriptOwner: NoteRef?

    /// Why `account` cannot be signed out or removed right now, or nil.
    func removalBlock(for account: AccountID) -> String? {
        active?.account == account && (recorder.isCapturing || isBusy) ? "Stop the recording first." : nil
    }

    /// The accounts whose capture ledger has `noteID`: where an old link that names no account leads.
    func ledger(of account: AccountID) -> Set<String> {
        Set(deliveries[account]?.histories.keys.map { $0.lowercased() } ?? [])
    }

    // MARK: Accounts

    /// Brings the deliveries in line with the accounts: one for each account
    /// whose identity is known, none for an account that has gone. An account
    /// that has just become Ready again has its unsent audio queued again.
    func sync(with accounts: [ServerNotesModel]) async {
        var present: [AccountID: ServerNotesModel] = [:]
        for notes in accounts { if let id = notes.accountID { present[id] = notes } }
        for (id, delivery) in deliveries where present[id] !== delivery.server {
            // A recording cannot outlive its account. Removal is refused while one runs; this is the backstop.
            if active?.account == id {
                recorder.stopSegment()
                rotationTask?.cancel(); rotationTask = nil
                live?.cancel(); live = nil; liveEnding = nil
                transcript = nil; transcriptOwner = nil
                active = nil
            }
            delivery.close()
            deliveries[id] = nil
            refreshSyncing()
        }
        for notes in accounts {
            // An account has an outbox once its server has said who it is: the directory is named by both.
            guard let id = notes.accountID, let instance = notes.instance, let principalID = notes.principalID else { continue }
            if let delivery = deliveries[id] {
                guard notes.status == .ready, delivery.generation != notes.sessionGeneration else { continue }
                delivery.generation = notes.sessionGeneration
                await resume(delivery)
                continue
            }
            do {
                let queue = try AudioOutbox(instance: instance, principalID: principalID, root: outboxRoot)
                let delivery = RecordingDelivery(account: id, server: notes, outbox: queue)
                let ledger = queue.directory.appendingPathComponent("captures.json")
                if FileManager.default.fileExists(atPath: ledger.path) {
                    delivery.histories = try JSONDecoder().decode([String: ServerCaptureHistory].self, from: Data(contentsOf: ledger))
                }
                deliveries[id] = delivery
                try recoverCaptures(delivery)
                deliveryRevision += 1
                if notes.status == .ready { await resume(delivery) }
            } catch { self.error = "Saved recording delivery could not be recovered. Original audio is preserved. \(error.localizedDescription)" }
        }
    }

    /// The accounts that have a delivery here, for tests.
    var deliveringAccounts: [AccountID] { Array(deliveries.keys) }

    /// Sends what an account still holds here: a Stop it recorded and could not
    /// send, and every part its server does not have. Never another account's.
    private func resume(_ delivery: RecordingDelivery) async {
        // Replay only this installation's recorded Stop intent. Do not stop arbitrary notes.
        for (id, history) in delivery.histories where history.needsStop == true && !isRecording(NoteRef(account: delivery.account, id: id)) {
            guard history.serverMissingSince == nil else { continue }
            do {
                _ = try await delivery.server.recording(noteID: id, command: .stop)
                guard !Task.isCancelled, !delivery.closed else { return }
                delivery.histories[id]?.needsStop = false
                try delivery.saveHistories()
            } catch {
                guard !Task.isCancelled, !delivery.closed else { return }
                if await confirmDeleted(id, delivery) { continue }
                self.error = "A saved Stop is waiting for a connection. \(error.localizedDescription)"
            }
        }
        for id in Set(delivery.outbox.entries.filter { $0.state != .ready }.map(\.noteID)) {
            guard !Task.isCancelled, !delivery.closed else { return }
            if delivery.histories[id]?.serverMissingSince != nil {
                // Asked once per launch or sign-in: a note that is listed again gets its audio after all.
                guard await isBack(id, delivery) else { continue }
            }
            await deliver(NoteRef(account: delivery.account, id: id), retryFailures: false)
        }
    }

    func start(note: NoteDTO, in account: AccountID?) async {
        guard !isBusy, !recorder.isCapturing, let account, let delivery = deliveries[account], !delivery.closed,
              let uuid = UUID(uuidString: note.id) else { return }
        let server = delivery.server, outbox = delivery.outbox
        let ref = NoteRef(account: account, id: note.id)
        isBusy = true
        error = nil
        active = ref
        captureStorageFailed = false
        defer { isBusy = false }
        // Only audio from the device's input needs the microphone permission.
        let needsMicrophone = (makeBackend ?? CaptureEnginePolicy.makeBackend)().usesMicrophone
        let permitted = await MicPermission.isGranted
        let allowed = permitted || !needsMicrophone ? true : await MicPermission.request()
        guard !Task.isCancelled, !delivery.closed else { return }
        guard allowed else {
            error = "Allow microphone access in Settings before starting a recording."
            return
        }
        do {
            // A previous Stop must reach the server before Resume, but transcription need not finish.
            await delivery.controlTask?.value
            try Task.checkCancellation()
            let freshNote = try await server.note(id: note.id)
            guard !delivery.closed else { throw CancellationError() }
            let remoteChunks = try await server.audioChunks(noteID: note.id)
            guard !delivery.closed else { throw CancellationError() }
            if delivery.histories[note.id] == nil {
                delivery.histories[note.id] = ServerCaptureHistory(baseOffsetMs: Self.durationMs(freshNote),
                    nextSequence: (remoteChunks.filter { $0.channel == .mic }.map(\.sequence).max() ?? -1) + 1, segments: [])
                try delivery.saveHistories()
            }
            try stageHistory(ref, delivery) // Repair any prior local queue write before a new capture starts.
            delivery.histories[note.id]!.prepareForResume(remoteChunks: remoteChunks,
                remoteDurationMs: Self.durationMs(freshNote), localEndMs: outbox.endMs(noteID: note.id))
            delivery.histories[note.id]!.serverMissingSince = nil
            try delivery.saveHistories()
            let history = delivery.histories[note.id]!
            _ = try await server.recording(noteID: note.id, command: freshNote.recordingState == .idle ? .start : .resume)
            guard !delivery.closed else { throw CancellationError() }
            let root = outbox.directory.appendingPathComponent("recordings", isDirectory: true)
            live?.cancel(); live = nil
            recorder = Recorder(simulateCapture: false, recordingRoot: root, backend: makeBackend)
            let shown = accountPresentation?(account)
            recorder.activityAccount = RecordingActivityAccount(id: account.raw, label: shown?.label, host: shown?.host)
            active = ref
            var elapsedSegments = history.segments
            let baselineMs = max(0, (history.nextStartMs ?? history.baseOffsetMs) - Int(history.segments.reduce(0) { $0 + $1.duration } * 1000))
            if baselineMs > 0 {
                elapsedSegments.insert(CaptureSegment(index: -1, startedAt: .now,
                    duration: Double(baselineMs) / 1000, fileName: "remote-duration"), at: 0)
            }
            recorder.prepare(existing: elapsedSegments)
            recorder.onSegmentFinished = { [weak self] segment, file in self?.finished(segment, file: file, ref: ref) }
            recorder.partSeconds = Self.capturePartSeconds
            recorder.onPartBoundary = { [weak self] frames in self?.stageOpenCapture(ref, frames: frames) }
            recorder.onPauseChanged = { [weak self] paused in self?.pauseChanged(paused) }
            let stored = LiveTranscript(noteID: note.id)
            stored.merge(note: freshNote)
            transcript = stored
            transcriptOwner = ref
            recorder.startSegment(noteID: uuid, title: freshNote.title)
            followTitle(of: ref)
            if case .failed(let message) = recorder.state {
                scheduleDelivery(ref, stopServer: true)
                error = message
            } else {
                status = "Recording on device · Transcript updates after each audio chunk"
                // The one place the two capture paths part ways.
                if recorder.capturesContinuously { beginContinuousCapture(ref, transcript: stored) }
                else { scheduleRotation(ref, uuid: uuid, title: note.title) }
            }
        } catch is CancellationError { }
        catch { self.error = error.localizedDescription }
    }

    /// Keeps the recording's Live Activity named after its note: a title typed
    /// during the meeting, or one the server gave the note, reaches the Lock
    /// Screen without anyone asking for it. Ends with the recording.
    private func followTitle(of ref: NoteRef) {
        guard active == ref, let server = deliveries[ref.account]?.server else { return }
        let title = withObservationTracking { server.title(ofNote: ref.id) } onChange: { [weak self] in
            Task { @MainActor in self?.followTitle(of: ref) }
        }
        if let title { recorder.setActivityTitle(title) }
    }

    func stop() {
        rotationTask?.cancel(); rotationTask = nil
        let finished = recorder.stopSegment()
        // Zero-frame starts still need a server Stop, without creating an upload or transcript.
        if finished == nil, let ref = active {
            // A stream still open here belongs to a capture that holds nothing, so its length is 0.
            // After an interruption the stream already ended at the capture's length and this is a no-op.
            endLive(atSample: 0)
            scheduleDelivery(ref, stopServer: true)
        }
    }

    func retry(_ ref: NoteRef?) {
        guard !isSyncing, let ref, let delivery = deliveries[ref.account] else { return }
        error = nil
        // Asking again is the person's own check of a note the server said was gone.
        delivery.histories[ref.id]?.serverMissingSince = nil
        scheduleDelivery(ref, stopServer: active != ref || !recorder.isCapturing, retryFailures: true)
    }

    func forgetDeletedNote(_ ref: NoteRef?) throws {
        guard let ref, !(active == ref && recorder.isCapturing), let delivery = deliveries[ref.account] else { return }
        let noteID = ref.id
        delivery.uploadTask?.cancel(); delivery.uploadTask = nil
        delivery.polls.removeValue(forKey: noteID)?.task.cancel()
        refreshSyncing()
        try delivery.outbox.discard(noteID: noteID)
        delivery.histories.removeValue(forKey: noteID)
        try delivery.saveHistories()
        if let uuid = UUID(uuidString: noteID) {
            let folder = delivery.outbox.directory.appendingPathComponent("recordings/\(uuid.uuidString)", isDirectory: true)
            if FileManager.default.fileExists(atPath: folder.path) { try FileManager.default.removeItem(at: folder) }
        }
        if active == ref { active = nil }
        if transcriptOwner == ref { transcript = nil; transcriptOwner = nil }
        deliveryRevision += 1
    }

    // MARK: Notes deleted on the server

    /// A recording's note is gone only when the server says so about the note
    /// itself: `GET /notes/{id}` answering 404. Nothing else (a failed upload, a
    /// short list, a server that does not answer) marks it. The audio stays on
    /// this device, marked, and is not sent again by itself.
    private func confirmDeleted(_ noteID: String, _ delivery: RecordingDelivery) async -> Bool {
        do {
            _ = try await delivery.server.noteSnapshot(id: noteID)
            return false
        } catch let failure as APIClientError where failure == .notFound {
            guard !delivery.closed else { return false }
            if delivery.histories[noteID] == nil {
                delivery.histories[noteID] = ServerCaptureHistory(baseOffsetMs: 0, nextSequence: delivery.outbox.nextSequence(noteID: noteID), segments: [])
            }
            delivery.histories[noteID]!.serverMissingSince = .now
            delivery.histories[noteID]!.needsStop = false
            try? delivery.saveHistories()
            deliveryRevision += 1
            return true
        } catch { return false }
    }

    /// A marked note that its server lists again loses the mark.
    private func isBack(_ noteID: String, _ delivery: RecordingDelivery) async -> Bool {
        guard (try? await delivery.server.noteSnapshot(id: noteID)) != nil, !delivery.closed else { return false }
        delivery.histories[noteID]?.serverMissingSince = nil
        try? delivery.saveHistories()
        deliveryRevision += 1
        return true
    }

    static let deletedOnServer = "Deleted on the server · Its audio is kept on this iPhone"

    // MARK: Delivery

    /// True while the note is being recorded here. Delivery then works quietly:
    /// the recording surface has its own status and must not flicker.
    private func isRecording(_ ref: NoteRef) -> Bool { active == ref && recorder.isCapturing }

    /// The one status line is about the note on screen. While a note is being
    /// recorded it says how that recording is transcribed, and the delivery of
    /// another note's saved audio does not write over it.
    private func showsDeliveryStatus(_ ref: NoteRef) -> Bool {
        !isRecording(ref) && !(recorder.isCapturing && active != ref)
    }

    private func refreshSyncing() {
        let value = deliveries.values.contains { !$0.uploads.isEmpty || !$0.polls.isEmpty }
        if isSyncing != value { isSyncing = value }
    }

    /// Uploads every staged part the server does not have yet, in order. It
    /// never waits for transcription: the next part can follow at once.
    private func deliver(_ ref: NoteRef, retryFailures: Bool) async {
        guard let delivery = deliveries[ref.account], !delivery.closed else { return }
        let server = delivery.server, outbox = delivery.outbox, noteID = ref.id
        let token = UUID()
        delivery.uploads.insert(token)
        refreshSyncing()
        defer { delivery.uploads.remove(token); refreshSyncing() }
        do {
            try stageHistory(ref, delivery)
            let waiting = outbox.entries(noteID: noteID).filter { $0.state != .ready }
            guard !waiting.isEmpty else { return }
            var chunks = try await server.audioChunks(noteID: noteID)
            for entry in waiting {
                try Task.checkCancellation()
                guard !delivery.closed else { throw CancellationError() }
                let known = chunks.first { $0.id == entry.id }
                let receipt: AudioChunkDTO
                if let known {
                    receipt = known.state == .failed && retryFailures
                        ? try await server.retryAudio(noteID: noteID, chunkID: entry.id) : known
                } else {
                    if showsDeliveryStatus(ref) { status = "Uploading saved audio…" }
                    receipt = try await server.upload(entry, from: outbox)
                }
                let state = Self.deliveryState(receipt)
                if state != entry.state { try outbox.update(id: entry.id, state: state) }
                chunks.removeAll { $0.id == receipt.id }; chunks.append(receipt)
                deliveryRevision += 1
            }
            follow(ref)
        } catch is CancellationError { }
        catch {
            // The note itself is asked for before its recording is called deleted.
            if (error as? APIClientError) == .notFound, !isRecording(ref), await confirmDeleted(noteID, delivery) {
                if showsDeliveryStatus(ref) { status = Self.deletedOnServer }
                return
            }
            // While recording, the next part boundary tries again; the capture itself is unaffected.
            if !isRecording(ref) {
                self.error = error.localizedDescription
                if showsDeliveryStatus(ref) { status = "Audio kept on this device · Retry delivery when connected" }
            } else {
                log.notice("Part upload will be retried: \(error.localizedDescription, privacy: .public)")
            }
        }
    }

    /// Starts, or extends, the one task that watches the server transcribe this note's parts.
    private func follow(_ ref: NoteRef) {
        guard let delivery = deliveries[ref.account] else { return }
        let noteID = ref.id
        delivery.pollBudget[noteID] = 120
        guard delivery.polls[noteID] == nil else { return }
        let id = UUID()
        delivery.polls[noteID] = (id, Task { [weak self] in
            await self?.poll(ref, delivery)
            guard let self, delivery.polls[noteID]?.id == id else { return }
            delivery.polls[noteID] = nil
            refreshSyncing()
        })
        refreshSyncing()
    }

    private func poll(_ ref: NoteRef, _ delivery: RecordingDelivery) async {
        let server = delivery.server, outbox = delivery.outbox, noteID = ref.id
        do {
            if showsDeliveryStatus(ref) { status = "Transcribing and identifying speakers…" }
            while (delivery.pollBudget[noteID] ?? 0) > 0 {
                delivery.pollBudget[noteID]! -= 1
                try Task.checkCancellation()
                guard !delivery.closed else { throw CancellationError() }
                let chunks = try await server.audioChunks(noteID: noteID)
                guard !delivery.closed else { throw CancellationError() }
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
                let streamed = isRecording(ref) && (transcript(for: ref).map { LiveStatusText.isStreaming($0.status) } ?? false)
                if (changed || failed || !working) && !streamed {
                    // getNote returns the server source of truth; update the list through refresh after completion.
                    // While the note is being recorded this is a private read: the transcript
                    // model keeps the result, and nothing that shows the list has to redraw.
                    document = isRecording(ref) ? try await server.noteSnapshot(id: noteID) : try await server.note(id: noteID)
                    guard !delivery.closed else { throw CancellationError() }
                    if let document { transcript(for: ref)?.merge(note: document) }
                }
                if failed {
                    error = "Some audio could not be transcribed. Saved chunks can be retried without recording again."
                    if showsDeliveryStatus(ref) { status = "Audio kept · Transcription needs retry" }
                    return
                }
                if !working {
                    if !isRecording(ref) {
                        if showsDeliveryStatus(ref) { status = document?.transcriptSegments.isEmpty == true ? "Audio saved · No speech detected" : "Transcript updated" }
                        await server.refresh()
                    }
                    return
                }
                try await Task.sleep(for: .seconds(2))
            }
            if showsDeliveryStatus(ref) { status = "Audio saved on server · Transcription is still processing" }
        } catch is CancellationError { }
        catch {
            if !isRecording(ref) {
                self.error = error.localizedDescription
                if showsDeliveryStatus(ref) { status = "Audio kept on this device · Retry delivery when connected" }
            }
        }
    }

    private func finished(_ segment: CaptureSegment, file: URL, ref: NoteRef) {
        // Every way a capture ends comes through here (Stop, an interruption, a failed route), after
        // its file is closed. `capturedFrames` is then the length of the capture, which is what
        // `mark: end` must carry, however much of it the stream has sent.
        endLive(atSample: recorder.capturedFrames)
        guard let delivery = deliveries[ref.account], delivery.histories[ref.id] != nil else { error = APIClientError.localStorage.localizedDescription; return }
        delivery.histories[ref.id]!.append(segment)
        if rotating, let gap = CaptureContinuity.gaps(in: delivery.histories[ref.id]!.segments).last, gap.afterIndex == segment.index - 1 {
            log.notice("Audio between segments \(gap.afterIndex, privacy: .public) and \(segment.index, privacy: .public) was not captured: \(Int(gap.seconds * 1000), privacy: .public) ms")
        }
        do {
            try delivery.saveHistories()
            try stageHistory(ref, delivery)
            CaptureRecovery.acknowledge(CaptureRecovery.journalURL(for: file))
            deliveryRevision += 1
        } catch { self.error = error.localizedDescription; captureStorageFailed = true }
        scheduleDelivery(ref, stopServer: !rotating)
    }

    private func scheduleDelivery(_ ref: NoteRef, stopServer: Bool, retryFailures: Bool = false) {
        guard let delivery = deliveries[ref.account] else { return }
        let noteID = ref.id
        if stopServer {
            let previousControl = delivery.controlTask
            let ending = liveEnding
            delivery.controlTask = Task { [weak self] in
                await previousControl?.value
                // The live stream hands over its last words before the note stops recording.
                await ending?.value
                guard let self, !Task.isCancelled, !delivery.closed else { return }
                do {
                    _ = try await delivery.server.recording(noteID: noteID, command: .stop)
                    guard !delivery.closed else { return }
                    delivery.histories[noteID]?.needsStop = false
                    try delivery.saveHistories()
                } catch { self.error = error.localizedDescription }
            }
        }
        // Uploads are one ordered chain per account. Watching transcription is a separate
        // task, so a slow transcription never holds back the next part.
        let previous = delivery.uploadTask
        delivery.uploadTask = Task { [weak self] in
            await previous?.value
            guard let self, !Task.isCancelled, !delivery.closed else { return }
            await deliver(ref, retryFailures: retryFailures)
        }
    }

    /// Stages every finished capture of the note that is not fully in the outbox.
    /// A recorder segment is copied in two-minute parts, as it always was. A
    /// continuous capture is cut on its own part length; parts cut while it was
    /// recording are recognised and only the rest is added.
    private func stageHistory(_ ref: NoteRef, _ delivery: RecordingDelivery) throws {
        let outbox = delivery.outbox, noteID = ref.id
        guard var history = delivery.histories[noteID], let uuid = UUID(uuidString: noteID) else { return }
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
                            partFrames: partSeconds(of: segment.fileName, ref: ref) * ContinuousCapture.sampleRate,
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
        delivery.histories[noteID] = history
        try delivery.saveHistories()
    }

    private func recoverCaptures(_ delivery: RecordingDelivery) throws {
        let root = delivery.outbox.directory.appendingPathComponent("recordings", isDirectory: true)
        for capture in CaptureRecovery.scan(roots: [root]) {
            let id = capture.noteID.uuidString.lowercased()
            guard delivery.histories[id] != nil else { continue }
            guard let segment = capture.segment else {
                error = "An interrupted recording could not be read completely. Its original audio and recovery journal are still on this device."
                continue
            }
            delivery.histories[id]!.append(segment)
            try delivery.saveHistories()
            try stageHistory(NoteRef(account: delivery.account, id: id), delivery)
            CaptureRecovery.acknowledge(capture.journalFile)
        }
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
    private func partSeconds(of fileName: String, ref: NoteRef) -> Int {
        let delivery = deliveries[ref.account]
        if let stored = delivery?.histories[ref.id]?.partSeconds?[fileName], stored > 0 { return stored }
        let cut = delivery?.outbox.entries(noteID: ref.id).filter { $0.sourceFileName == fileName }.map(\.durationMs).max() ?? 0
        return cut >= 1000 && cut % 1000 == 0 ? cut / 1000 : Self.capturePartSeconds
    }

    /// Records where the capture sits on the note timeline before any part of
    /// it is cut, then starts the live stream from the same file.
    private func beginContinuousCapture(_ ref: NoteRef, transcript: LiveTranscript) {
        guard let file = recorder.currentFileURL, let delivery = deliveries[ref.account], delivery.histories[ref.id] != nil else { return }
        let baseMs = delivery.histories[ref.id]!.begin(fileName: file.lastPathComponent, partSeconds: Self.capturePartSeconds)
        do { try delivery.saveHistories() } catch { self.error = error.localizedDescription }
        startLive(ref, delivery, file: file, baseMs: baseMs, transcript: transcript)
    }

    /// The capture grew past a part boundary: cut the part and send it.
    private func stageOpenCapture(_ ref: NoteRef, frames: Int64) {
        let noteID = ref.id
        guard isRecording(ref), let delivery = deliveries[ref.account], let history = delivery.histories[noteID], let file = recorder.currentFileURL,
              let baseMs = history.segmentOffsets?[file.lastPathComponent] else { return }
        do {
            let staged = try Signpost.interval("Outbox.Stage") {
                try delivery.outbox.stageParts(noteID: noteID, capture: file, availableFrames: Int(frames),
                    partFrames: partSeconds(of: file.lastPathComponent, ref: ref) * ContinuousCapture.sampleRate,
                    isFinal: false, startMs: baseMs, sequence: max(history.nextSequence, delivery.outbox.nextSequence(noteID: noteID)))
            }
            guard !staged.isEmpty else { return }
            deliveryRevision += 1
            scheduleDelivery(ref, stopServer: false)
        } catch {
            // The capture file is intact and is cut again at the next boundary and at Stop.
            self.error = error.localizedDescription
        }
    }

    private func startLive(_ ref: NoteRef, _ delivery: RecordingDelivery, file: URL, baseMs: Int, transcript: LiveTranscript) {
        let server = delivery.server, noteID = ref.id
        guard let source = try? CaptureFileReader(url: file) else { return }
        let configuration = LiveTranscriptClient.Configuration(captureID: "\(noteID)/\(file.lastPathComponent)", baseMs: baseMs)
        guard let client = try? makeLiveClient(server, noteID, source, configuration) else { return }
        let session = LiveCaptureSession(client: client, transcript: transcript) {
            guard !delivery.closed else { return nil }
            return try? await server.noteSnapshot(id: noteID)
        }
        session.onStatus = { [weak self] value in
            guard let self, isRecording(ref) else { return }
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

    private func scheduleRotation(_ ref: NoteRef, uuid: UUID, title: String) {
        rotationTask?.cancel()
        rotationTask = Task { [weak self] in
            while !Task.isCancelled {
                do { try await Task.sleep(for: .seconds(120)) } catch { return }
                guard let self, active == ref, recorder.state == .recording else { return }
                // Measurement only: the order of operations below is unchanged.
                let interval = Signpost.ui.beginInterval("Capture.Rotate")
                let began = ProcessInfo.processInfo.systemUptime
                rotating = true
                recorder.stopSegment(keepingLiveActivity: true)
                rotating = false
                guard !captureStorageFailed else {
                    Signpost.ui.endInterval("Capture.Rotate", interval)
                    recorder.stopSegment() // End the retained indicator when rotation cannot continue.
                    scheduleDelivery(ref, stopServer: true)
                    return
                }
                // The next part of the same recording: its Live Activity keeps the title it has.
                recorder.startSegment(noteID: uuid, title: "")
                Signpost.ui.endInterval("Capture.Rotate", interval)
                noteRotationPause(ProcessInfo.processInfo.systemUptime - began)
                if case .failed(let message) = recorder.state {
                    error = message
                    scheduleDelivery(ref, stopServer: true)
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
    /// Set when the server answered 404 for the note itself. The audio is kept;
    /// it is cleared when the note is listed again or recorded to again.
    var serverMissingSince: Date?
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
