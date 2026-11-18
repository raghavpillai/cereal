import Foundation
import Observation
import UIKit
import APIClient
import Contracts
import os

/// What one account needs for the recordings made to it: its server, its
/// directory on this device and its capture ledger. Nothing in one is ever
/// read or sent through another account.
@MainActor
final class RecordingAccount {
    let account: AccountID
    let server: ServerNotesModel
    let directory: URL
    var histories: [String: ServerCaptureHistory] = [:]
    /// Per note, its Stops, one after another. Each delivers the note's
    /// captures first and answers with the note as the server has it once it
    /// is stopped, or nil when the Stop was not sent or did not reach the server.
    var controlTasks: [String: Task<NoteDTO?, Never>] = [:]
    /// Per note, the Stops scheduled and not finished yet.
    var pendingStops: [String: Int] = [:]
    /// Streams of captures left by an earlier launch, by capture id, while they are delivered.
    var leftoverStreams: [String: LiveCaptureSession] = [:]
    /// The session of the account that this record last sent its saved Stops for.
    var generation: Int
    /// True once the account has gone. Work begun before that ends without touching anything.
    private(set) var closed = false

    init(account: AccountID, server: ServerNotesModel, directory: URL) {
        self.account = account; self.server = server; self.directory = directory
        generation = server.sessionGeneration
    }

    /// Where captures are written while they are recorded and streamed.
    var recordingsRoot: URL { directory.appendingPathComponent("recordings", isDirectory: true) }

    func close() {
        closed = true
        for task in controlTasks.values { task.cancel() }
        controlTasks = [:]
        for stream in leftoverStreams.values { stream.cancel() }
        leftoverStreams = [:]
    }

    /// Where a capture of the note is on this device.
    func captureURL(noteID: String, fileName: String) -> URL? {
        guard let uuid = UUID(uuidString: noteID), fileName == URL(fileURLWithPath: fileName).lastPathComponent else { return nil }
        return recordingsRoot.appendingPathComponent(uuid.uuidString, isDirectory: true).appendingPathComponent(fileName)
    }

    func saveHistories() throws {
        let url = directory.appendingPathComponent("captures.json")
        try JSONEncoder().encode(histories).write(to: url, options: [.atomic, .completeFileProtectionUntilFirstUserAuthentication])
        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: url.path)
        PerfProbe.hit("Captures.Save")
    }
}

/// One app-wide owner keeps a real recording alive when its note view is dismissed.
///
/// A recording is transcribed by its live stream only. The microphone is
/// captured into one continuous file, which is the stream's buffer: after a
/// lost connection the stream reconnects and replays from the file. Nothing is
/// uploaded. When the person stops the recording, the capture stops at once;
/// the stream then hands over what it has not sent yet ("Finishing the
/// transcript…"), for as long as that takes, and only once the server has
/// confirmed all of it is the note stopped on the server and its notes
/// written. The capture file is deleted only after both. A launch that ended
/// before that (the app was killed, or the network never came back) streams
/// the capture again at the next launch, from where the server's transcript
/// ends, and then stops the note and writes its notes.
@MainActor @Observable
final class ServerRecordingController {
    private(set) var recorder: Recorder
    /// The recording's Live Activity, the same one from recording to recording:
    /// each new recorder reports to it, so a new recording replaces what is left
    /// of the last one ("Writing notes...") instead of standing beside it.
    @ObservationIgnored private let liveActivity: RecordingLiveActivityController
    /// The note being recorded, or just recorded, with the account it belongs to.
    private(set) var active: NoteRef?
    private(set) var isBusy = false
    private(set) var error: String?
    /// Notes whose recording has been stopped and whose notes are being written from the transcript.
    private(set) var generatingNotes: Set<NoteRef> = []
    /// Notes whose captures are still being handed to the server after the capture stopped.
    private(set) var finishing: Set<NoteRef> = []
    /// The transcript of the note being recorded, kept current while it is
    /// recorded: stored text from the server, and the text still being spoken.
    /// Nil when no recording has been started here.
    private(set) var transcript: LiveTranscript?
    @ObservationIgnored private var accounts: [AccountID: RecordingAccount] = [:]
    @ObservationIgnored private var live: LiveCaptureSession?
    /// The file the live stream reads.
    @ObservationIgnored private var liveFile: String?
    @ObservationIgnored private let log = Logger(subsystem: AppIdentity.subsystem, category: "recording")
    /// The root of the account directories (named `server-audio-outbox` on disk, after what it once held).
    @ObservationIgnored private let outboxRoot: URL?
    @ObservationIgnored private let makeBackend: @MainActor () -> any ContinuousCaptureBackend
    @ObservationIgnored private let makeLiveClient: LiveClientFactory
    /// What the Live Activity may say about the account, when there is more than one to tell apart.
    @ObservationIgnored var accountPresentation: (@MainActor (AccountID) -> (label: String, host: String)?)?

    typealias LiveClientFactory = @MainActor (ServerNotesModel, String, any LiveAudioSource, LiveTranscriptClient.Configuration) throws -> LiveTranscriptClient

    /// The parameters replace storage, audio input and the live socket in tests.
    /// A recording is always one continuous capture, the only kind its live stream can read.
    init(outboxRoot: URL? = nil, makeBackend: (@MainActor () -> any ContinuousCaptureBackend)? = nil, makeLiveClient: LiveClientFactory? = nil,
         liveActivity: RecordingLiveActivityController? = nil) {
        let liveActivity = liveActivity ?? RecordingLiveActivityController()
        self.liveActivity = liveActivity
        recorder = Recorder(simulateCapture: false, activity: liveActivity)
        self.outboxRoot = outboxRoot
        self.makeBackend = makeBackend ?? { CaptureEnginePolicy.makeContinuousBackend() }
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

    func isActive(_ ref: NoteRef?) -> Bool { ref != nil && active == ref }
    func isIndicator(_ ref: NoteRef?) -> Bool { ref != nil && indicator == ref }
    func isGeneratingNotes(_ ref: NoteRef?) -> Bool { ref.map(generatingNotes.contains) ?? false }
    /// The capture has stopped and its audio is still being handed to the server.
    func isFinishingTranscript(_ ref: NoteRef?) -> Bool { ref.map(finishing.contains) ?? false }

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

    /// The notes `account` has recorded to here: where an old link that names no account leads.
    func ledger(of account: AccountID) -> Set<String> {
        Set(accounts[account]?.histories.keys.map { $0.lowercased() } ?? [])
    }

    /// The accounts that have a record here, for tests.
    var knownAccounts: [AccountID] { Array(accounts.keys) }

    static let noTranscript = "No transcript was captured for this recording."

    // MARK: Accounts

    /// Brings the records in line with the accounts: one for each account
    /// whose identity is known, none for an account that has gone. An account
    /// that has just become Ready again has its saved Stops sent.
    func sync(with sessions: [ServerNotesModel]) async {
        var present: [AccountID: ServerNotesModel] = [:]
        for notes in sessions { if let id = notes.accountID { present[id] = notes } }
        for (id, record) in accounts where present[id] !== record.server {
            // A recording cannot outlive its account. Removal is refused while one runs; this is the backstop.
            if active?.account == id {
                // Its activity goes with it: nothing more will be said about that account's note.
                recorder.stopSegment(activity: .discard)
                liveActivity.end(ending: .discard)
                live?.cancel(); live = nil; liveFile = nil
                transcript = nil; transcriptOwner = nil
                active = nil
            }
            generatingNotes = generatingNotes.filter { $0.account != id }
            finishing = finishing.filter { $0.account != id }
            record.close()
            accounts[id] = nil
        }
        for notes in sessions {
            // An account has a directory once its server has said who it is: the directory is named by both.
            guard let id = notes.accountID else { continue }
            if let record = accounts[id] {
                guard notes.status == .ready, record.generation != notes.sessionGeneration else { continue }
                record.generation = notes.sessionGeneration
                await resume(record)
                continue
            }
            do {
                let directory = try AccountStorage(root: outboxRoot).directory(id.raw)
                try Self.prepare(directory)
                let record = RecordingAccount(account: id, server: notes, directory: directory)
                let ledger = directory.appendingPathComponent("captures.json")
                if FileManager.default.fileExists(atPath: ledger.path) {
                    record.histories = try JSONDecoder().decode([String: ServerCaptureHistory].self, from: Data(contentsOf: ledger))
                }
                accounts[id] = record
                placeLeftovers(record)
                if notes.status == .ready { await resume(record) }
            } catch { self.error = "This account's recordings on this iPhone could not be read. \(error.localizedDescription)" }
        }
    }

    /// Finishes what this installation recorded and could not finish: captures
    /// whose audio the server has not confirmed are streamed again, then the
    /// saved Stop is sent, then the notes of a recording the person stopped are
    /// written. Never another account's, and never a note this installation did
    /// not record. Each note runs on its own and is not waited for here: a
    /// stream waits for as long as the network is away.
    private func resume(_ record: RecordingAccount) async {
        for (id, history) in record.histories.sorted(by: { $0.key < $1.key }) {
            let ref = NoteRef(account: record.account, id: id)
            guard !isRecording(ref), record.pendingStops[id, default: 0] == 0,
                  history.needsStop == true || !history.unfinishedCaptures.isEmpty else { continue }
            let stopping = scheduleStop(ref, current: nil, resumed: true)
            guard history.needsNotes == true else { continue }
            Task { [weak self] in
                guard let note = await stopping?.value, let self, !record.closed,
                      record.histories[id]?.needsNotes == true else { return }
                record.histories[id]?.needsNotes = nil
                try? record.saveHistories()
                await writeNotes(ref, from: note, record, followed: false)
            }
        }
    }

    /// Captures on disk that the ledger knows. A capture left by a recording
    /// that ended with the app is placed on the note's timeline by its length
    /// (`append` takes each file once); if its audio is not all on the server
    /// yet, `resume` streams it. Audio no stream can send, from an earlier
    /// version (the delivery files it kept to upload, captures it never marked
    /// for delivery), is left on disk untouched.
    private func placeLeftovers(_ record: RecordingAccount) {
        var changed = false
        for capture in CaptureRecovery.scan(roots: [record.recordingsRoot]) {
            let id = capture.noteID.uuidString.lowercased()
            guard record.histories[id] != nil, let segment = capture.segment else { continue }
            record.histories[id]!.append(segment)
            changed = true
        }
        if changed {
            do { try record.saveHistories() } catch { self.error = error.localizedDescription }
        }
    }

    // MARK: Recording

    /// `justRead`: `note` is the server's answer to a read made a moment ago by whoever asks (the
    /// recording screen reads the note as it opens, then starts), so it is not read a second time.
    func start(note: NoteDTO, in account: AccountID?, justRead: Bool = false) async {
        guard !isBusy, !recorder.isCapturing, let account, let record = accounts[account], !record.closed,
              let uuid = UUID(uuidString: note.id) else { return }
        let server = record.server
        let ref = NoteRef(account: account, id: note.id)
        isBusy = true
        error = nil
        active = ref
        defer { isBusy = false }
        // Only audio from the device's input needs the microphone permission.
        let needsMicrophone = makeBackend().usesMicrophone
        let permitted = await MicPermission.isGranted
        let allowed = permitted || !needsMicrophone ? true : await MicPermission.request()
        guard !Task.isCancelled, !record.closed else { return }
        guard allowed else {
            error = "Allow microphone access in Settings before starting a recording."
            return
        }
        // The audio session comes up while the server is asked, off the main thread: the hardware
        // takes a moment (longer over Bluetooth) that would otherwise be spent after the server's
        // answers, with the screen held. It is let go again if the start gets no further.
        if needsMicrophone { RecordingAudioSession.warm() }
        do {
            // A previous Stop of this note, with the audio it hands over first, must reach the server before Resume.
            let stopping = record.controlTasks[note.id]
            _ = await stopping?.value
            try Task.checkCancellation()
            // A note read a moment ago is the note as it is, unless a Stop of it was still on its way
            // or the person was being asked for the microphone meanwhile: then it is read again.
            let freshNote: NoteDTO
            if justRead, permitted, stopping == nil { freshNote = note } else { freshNote = try await server.note(id: note.id) }
            guard !record.closed else { throw CancellationError() }
            if record.histories[note.id] == nil {
                record.histories[note.id] = ServerCaptureHistory(baseOffsetMs: Self.durationMs(freshNote), segments: [])
            }
            record.histories[note.id]!.prepareForResume(remoteDurationMs: Self.durationMs(freshNote))
            try record.saveHistories()
            let history = record.histories[note.id]!
            _ = try await server.recording(noteID: note.id, command: freshNote.recordingState == .idle ? .start : .resume)
            guard !record.closed else { throw CancellationError() }
            live?.cancel(); live = nil
            let makeBackend = makeBackend
            recorder = Recorder(simulateCapture: false, recordingRoot: record.recordingsRoot, backend: { makeBackend() }, activity: liveActivity)
            let shown = accountPresentation?(account)
            recorder.activityAccount = RecordingActivityAccount(id: account.raw, label: shown?.label, host: shown?.host)
            active = ref
            generatingNotes.remove(ref)
            var elapsedSegments = history.segments
            let baselineMs = max(0, (history.nextStartMs ?? history.baseOffsetMs) - Int(history.segments.reduce(0) { $0 + $1.duration } * 1000))
            if baselineMs > 0 {
                elapsedSegments.insert(CaptureSegment(index: -1, startedAt: .now,
                    duration: Double(baselineMs) / 1000, fileName: "remote-duration"), at: 0)
            }
            recorder.prepare(existing: elapsedSegments)
            recorder.onSegmentFinished = { [weak self] segment, file in self?.finished(segment, file: file, ref: ref) }
            recorder.onPauseChanged = { [weak self] paused in self?.pauseChanged(paused) }
            let stored = LiveTranscript(noteID: note.id)
            stored.merge(note: freshNote)
            transcript = stored
            transcriptOwner = ref
            recorder.startSegment(noteID: uuid, title: freshNote.title)
            followTitle(of: ref)
            // The Live Activity's End stops this recording, as End on the recording screen does.
            RecordingActivityActions.setHandler(owner: Self.activityOwner) { [weak self] id in
                guard let self, let active = self.active, UUID(uuidString: active.id) == id, self.recorder.isCapturing else { return false }
                self.stop()
                return true
            }
            if case .failed(let message) = recorder.state {
                scheduleStop(ref, current: nil)
                error = message
            } else {
                beginLiveCapture(ref, transcript: stored)
            }
        } catch {
            // Everything that throws here comes before the capture: the session warmed for it is not needed.
            if needsMicrophone { RecordingAudioSession.release() }
            if let shown = ErrorText.shown(error) { self.error = shown }
        }
    }

    /// Removes a note that was made only to be recorded and holds nothing: its
    /// recording never started (no microphone, say) and nothing was typed.
    /// True when the note is gone. Anything recorded here, anything on the
    /// server's copy (a title, text, notes, a transcript, a duration) or
    /// anything still in flight keeps it, as does a failure to find out. The
    /// server deletes only if the note is still as it was just read.
    @discardableResult
    func discardIfNothingRecorded(_ ref: NoteRef?, typedTitle: String = "", typedText: String = "") async -> Bool {
        guard let ref, let record = accounts[ref.account], !record.closed, !isBusy, !isRecording(ref),
              Self.isUntitled(typedTitle), typedText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return false }
        // The Stop of the start that failed goes first.
        _ = await record.controlTasks[ref.id]?.value
        guard !record.closed, !isBusy, !isRecording(ref), !generatingNotes.contains(ref), !finishing.contains(ref),
              record.pendingStops[ref.id, default: 0] == 0 else { return false }
        if let history = record.histories[ref.id] {
            guard history.segments.isEmpty, history.deliveries?.isEmpty ?? true, history.needsStop != true, history.needsNotes != true else { return false }
        }
        do {
            let fresh = try await record.server.note(id: ref.id)
            guard Self.holdsNothing(fresh), !record.closed, !isBusy, !isRecording(ref) else { return false }
            try await record.server.delete(id: ref.id, ifStamp: Self.stamp(fresh))
        } catch { return false }
        let wasShown = active == ref
        try? forgetDeletedNote(ref)
        if wasShown { error = nil }
        return true
    }

    /// Cancel, offered for the first seconds of a recording made for a new note:
    /// the capture stops and, once its Stop has reached the server, the note is
    /// deleted with what little it caught, so it never appears on Home. A note
    /// in which something was typed is kept, and its notes are written as after
    /// End. True when the note is gone.
    @discardableResult
    func cancel(_ ref: NoteRef?, typedTitle: String = "", typedText: String = "") async -> Bool {
        guard let ref, active == ref, let record = accounts[ref.account], !record.closed else { return false }
        let typed = !Self.isUntitled(typedTitle) || !typedText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        guard !typed else { stop(); return false }
        // The note is about to go: so does its activity, at once.
        let finished = recorder.stopSegment(activity: .discard)
        if finished == nil { scheduleStop(ref, current: endLive(atSample: recorder.capturedFrames)) }
        _ = await record.controlTasks[ref.id]?.value
        guard !record.closed, !isRecording(ref) else { return false }
        do { try await record.server.delete(id: ref.id) } catch { return false }
        try? forgetDeletedNote(ref)
        error = nil
        return true
    }

    private static func isUntitled(_ title: String) -> Bool {
        ["", "new note", "untitled note"].contains(title.trimmingCharacters(in: .whitespacesAndNewlines).lowercased())
    }

    /// True for a note with no title, text, generated notes, transcript or recorded time.
    static func holdsNothing(_ note: NoteDTO) -> Bool {
        func blank(_ text: String) -> Bool { text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }
        return isUntitled(note.title) && blank(note.body) && blank(note.enhanced)
            && note.transcriptRevision == 0 && note.transcriptSegments.isEmpty && (note.transcriptGaps ?? []).isEmpty
            && note.generatedRevision == nil && [.idle, .unsupported, .failed].contains(note.generationState)
            && note.recordingState != .recording && note.importedAt == nil && durationMs(note) == 0
    }

    /// The note as it was read, for a delete that must not remove anything newer.
    static func stamp(_ note: NoteDTO) -> String {
        "\(note.updatedAt)|\(note.transcriptRevision)|\(note.generatedRevision.map(String.init) ?? "")"
    }

    /// Keeps the recording's Live Activity named after its note: a title typed
    /// during the meeting, or one the server gave the note, reaches the Lock
    /// Screen without anyone asking for it. Ends with the recording.
    private func followTitle(of ref: NoteRef) {
        guard active == ref, let server = accounts[ref.account]?.server else { return }
        let title = withObservationTracking { server.title(ofNote: ref.id) } onChange: { [weak self] in
            Task { @MainActor in self?.followTitle(of: ref) }
        }
        if let title { recorder.setActivityTitle(title) }
    }

    /// Ends the recording, then writes its notes from the transcript once the
    /// server has stopped it. `beforeNotes` runs first, after the Stop: the
    /// recording screen saves what was typed, which the notes are written with.
    func stop(beforeNotes: (@MainActor () async -> Void)? = nil) {
        // The activity says "Writing notes..." until they are written, and then "Notes ready".
        let finished = recorder.stopSegment(activity: .writingNotes)
        guard let ref = active else { return }
        // A capture that holds nothing still needs a server Stop. A stream still open here belongs
        // to it; after an interruption the stream already ended and there is none.
        if finished == nil {
            scheduleStop(ref, current: endLive(atSample: recorder.capturedFrames))
        }
        writeNotesAfterStop(ref, beforeNotes: beforeNotes)
    }

    func forgetDeletedNote(_ ref: NoteRef?) throws {
        guard let ref, !(active == ref && recorder.isCapturing), let record = accounts[ref.account] else { return }
        record.histories.removeValue(forKey: ref.id)
        try record.saveHistories()
        if let uuid = UUID(uuidString: ref.id) {
            let folder = record.recordingsRoot.appendingPathComponent(uuid.uuidString, isDirectory: true)
            if FileManager.default.fileExists(atPath: folder.path) { try FileManager.default.removeItem(at: folder) }
        }
        generatingNotes.remove(ref)
        finishing.remove(ref)
        if active == ref { active = nil }
        if transcriptOwner == ref { transcript = nil; transcriptOwner = nil }
    }

    /// True while the note is being recorded here.
    private func isRecording(_ ref: NoteRef) -> Bool { active == ref && recorder.isCapturing }

    private func finished(_ segment: CaptureSegment, file: URL, ref: NoteRef) {
        // Every way a capture ends comes through here (Stop, an interruption, a failed route), after
        // its file is closed. `capturedFrames` is then the length of the capture, which is what
        // `mark: end` must carry, however much of it the stream has sent.
        let ending = endLive(atSample: recorder.capturedFrames)
        if let record = accounts[ref.account], record.histories[ref.id] != nil {
            record.histories[ref.id]!.append(segment)
            do { try record.saveHistories() } catch { self.error = error.localizedDescription }
        }
        // The file stays until the server has all of it and has stopped the note. Without a stream
        // (it could not start) the capture is streamed now from its file, as a later launch would.
        scheduleStop(ref, current: ending)
    }

    /// The server's Stop, after every capture of the note recorded here is on
    /// the server: the one that just ended (`current`) and any an earlier
    /// launch left. A Stop ends the live stream for good, so it is never sent
    /// while audio is still only on this iPhone; it is left for the next launch
    /// or sign-in instead. Then the delivered capture files are deleted.
    /// `resumed`: a saved Stop, sent only if it is still needed.
    @discardableResult
    private func scheduleStop(_ ref: NoteRef, current: LiveEnding?, resumed: Bool = false) -> Task<NoteDTO?, Never>? {
        guard let record = accounts[ref.account], !record.closed else { return nil }
        let previous = record.controlTasks[ref.id]
        record.pendingStops[ref.id, default: 0] += 1
        let task = Task { [weak self] () -> NoteDTO? in
            _ = await previous?.value
            defer { record.pendingStops[ref.id] = max(0, record.pendingStops[ref.id, default: 1] - 1) }
            guard let self, !Task.isCancelled, !record.closed else { return nil }
            guard await deliverCaptures(ref, record, current: current) else { return nil }
            guard !Task.isCancelled, !record.closed, record.histories[ref.id] != nil || !resumed else { return nil }
            guard !resumed || record.histories[ref.id]?.needsStop == true else {
                deleteDelivered(ref, record)
                return nil
            }
            do {
                let note = try await record.server.recording(noteID: ref.id, command: .stop)
                guard !record.closed else { return nil }
                record.histories[ref.id]?.needsStop = false
                try? record.saveHistories()
                deleteDelivered(ref, record)
                transcript(for: ref)?.merge(note: note)
                return note
            } catch let failure as APIClientError where failure == .notFound {
                // The note is gone from its server: there is nothing left to stop, write or stream.
                guard !record.closed else { return nil }
                record.histories[ref.id]?.needsStop = false
                record.histories[ref.id]?.needsNotes = nil
                try? record.saveHistories()
                deleteDelivered(ref, record)
                return nil
            } catch {
                if !record.closed {
                    if let shown = ErrorText.shown(error) { self.error = resumed ? "A saved Stop is waiting for a connection. \(shown)" : shown }
                }
                return nil
            }
        }
        record.controlTasks[ref.id] = task
        return task
    }

    /// Hands the note's captures to the server: the one that just ended, then
    /// any an earlier launch left, oldest first. True once none is left whose
    /// audio the server has not confirmed. "Finishing the transcript…" is shown
    /// meanwhile, and the app asks for time to finish if it leaves the screen.
    private func deliverCaptures(_ ref: NoteRef, _ record: RecordingAccount, current: LiveEnding?) async -> Bool {
        let leftovers = record.histories[ref.id]?.unfinishedCaptures.filter { $0 != current?.fileName } ?? []
        guard current != nil || !leftovers.isEmpty else { return true }
        finishing.insert(ref)
        let background = BackgroundTime.begin("finish-transcript")
        defer {
            finishing.remove(ref)
            BackgroundTime.end(background)
        }
        if let current {
            let drain = await current.task.value
            guard settle(ref, record, fileName: current.fileName, drain) else { return false }
        }
        for fileName in leftovers {
            guard !Task.isCancelled, !record.closed, record.histories[ref.id]?.unfinishedCaptures.contains(fileName) == true else { continue }
            let drain = await deliverLeftover(ref, record, fileName: fileName)
            guard settle(ref, record, fileName: fileName, drain) else { return false }
        }
        return record.histories[ref.id]?.unfinishedCaptures.isEmpty ?? false
    }

    /// Records how a capture's stream ended. False when the capture still has
    /// audio to hand over, which a later launch or sign-in tries again.
    private func settle(_ ref: NoteRef, _ record: RecordingAccount, fileName: String?, _ drain: LiveDrain) -> Bool {
        guard !record.closed, record.histories[ref.id] != nil else { return false }
        defer { try? record.saveHistories() }
        guard let fileName else { return drain == .delivered || drain == .refused(.notRecording) }
        let onDisk = record.captureURL(noteID: ref.id, fileName: fileName).map { FileManager.default.fileExists(atPath: $0.path) } ?? false
        switch drain {
        case .delivered:
            record.histories[ref.id]!.markDelivered(fileName, onDisk: onDisk)
            return true
        case .refused(.notRecording):
            // The note was stopped on the server (from elsewhere): no stream can send this capture
            // any more. What it did not hand over was reported as skipped, and the audio stays on
            // this iPhone untouched, like any other capture no stream can send.
            record.histories[ref.id]!.forgetDelivery(fileName)
            return true
        case .refused(.notFound):
            // The note is gone from its server: there is nothing to stream to, stop or write.
            if var history = record.histories[ref.id] {
                history.needsStop = false
                history.needsNotes = nil
                record.histories[ref.id] = history
            }
            for name in record.histories[ref.id]?.deliveries?.keys.sorted() ?? [] {
                record.histories[ref.id]!.forgetDelivery(name)
                if let file = record.captureURL(noteID: ref.id, fileName: name) {
                    try? FileManager.default.removeItem(at: file)
                    CaptureRecovery.acknowledge(CaptureRecovery.journalURL(for: file))
                }
            }
            return false
        case .refused(.unauthorized):
            if !onDisk { record.histories[ref.id]!.forgetDelivery(fileName); return true }
            error = "Sign in again to finish this recording's transcript. Its audio is saved on this iPhone."
            return false
        case .refused, .interrupted:
            // A capture that was never written (it held nothing) has nothing to hand over.
            if !onDisk { record.histories[ref.id]!.forgetDelivery(fileName); return true }
            return false
        }
    }

    /// Streams a capture left by an earlier launch, from where the server's
    /// transcript of it ends, to its end.
    private func deliverLeftover(_ ref: NoteRef, _ record: RecordingAccount, fileName: String) async -> LiveDrain {
        guard let file = record.captureURL(noteID: ref.id, fileName: fileName), FileManager.default.fileExists(atPath: file.path) else {
            record.histories[ref.id]?.forgetDelivery(fileName)
            return .delivered
        }
        // A capture is placed on the note's timeline when it begins. One that is not cannot be streamed.
        guard let baseMs = record.histories[ref.id]?.segmentOffsets?[fileName] else {
            record.histories[ref.id]?.forgetDelivery(fileName)
            return .refused(.notRecording)
        }
        let configuration = LiveTranscriptClient.Configuration(captureID: "\(ref.id)/\(fileName)", baseMs: baseMs)
        guard let source = try? CaptureFileReader(url: file), let client = try? makeLiveClient(record.server, ref.id, source, configuration) else {
            return .interrupted
        }
        let server = record.server, noteID = ref.id
        let session = LiveCaptureSession(client: client, transcript: transcript(for: ref) ?? LiveTranscript(noteID: ref.id)) {
            guard !record.closed else { return nil }
            return try? await server.noteSnapshot(id: noteID)
        }
        record.leftoverStreams[configuration.captureID] = session
        defer { record.leftoverStreams[configuration.captureID] = nil }
        session.start()
        return await session.end(atSample: source.availableFrames()).value
    }

    /// Deletes the capture files the server has all of, once the note is stopped.
    private func deleteDelivered(_ ref: NoteRef, _ record: RecordingAccount) {
        guard let delivered = record.histories[ref.id]?.takeDelivered(), !delivered.isEmpty else { return }
        try? record.saveHistories()
        for fileName in delivered {
            guard let file = record.captureURL(noteID: ref.id, fileName: fileName) else { continue }
            try? FileManager.default.removeItem(at: file)
            CaptureRecovery.acknowledge(CaptureRecovery.journalURL(for: file))
        }
    }

    // MARK: Notes

    /// The notes are written once the Stop has reached the server, from the
    /// transcript it answered with. A Stop that did not reach it is sent again
    /// when the account is next ready, and the notes are written after it.
    private func writeNotesAfterStop(_ ref: NoteRef, beforeNotes: (@MainActor () async -> Void)?) {
        // A second Stop while the first is still on its way writes nothing more.
        guard let record = accounts[ref.account], !record.closed, !generatingNotes.contains(ref) else { return }
        if record.histories[ref.id] != nil {
            record.histories[ref.id]!.needsNotes = true
            try? record.saveHistories()
        }
        generatingNotes.insert(ref)
        let stopping = record.controlTasks[ref.id]
        Task { [weak self] in
            let stopped = await stopping?.value ?? nil
            await beforeNotes?()
            guard let self else { return }
            guard let stopped, !record.closed else {
                generatingNotes.remove(ref)
                // The Stop is kept and sent again; the notes are written after it, in the app.
                notesFinished(ref, .deferred)
                return
            }
            record.histories[ref.id]?.needsNotes = nil
            try? record.saveHistories()
            await writeNotes(ref, from: stopped, record, followed: true)
        }
    }

    /// Writes the note's notes from its transcript. Nothing is asked of the
    /// server when there is no transcript (the note then says so) or when its
    /// notes already cover this transcript.
    /// `followed`: the notes of a Stop made in this launch, which its Live
    /// Activity is waiting on and which are felt when they are ready.
    private func writeNotes(_ ref: NoteRef, from note: NoteDTO, _ record: RecordingAccount, followed: Bool) async {
        generatingNotes.insert(ref)
        defer { generatingNotes.remove(ref) }
        guard Self.hasTranscript(note) else { notesFinished(ref, .noTranscript); return }
        // Notes that already cover this transcript are ready as they are.
        guard note.generatedRevision != note.transcriptRevision else { notesFinished(ref, .ready); return }
        do {
            let written = try await record.server.generate(noteID: ref.id, expectedTranscriptRevision: note.transcriptRevision)
            notesFinished(ref, .ready)
            if followed, !record.closed { Haptics.shared.play(.notesReady) }
            // Asked for from the note's "Notify me"; sent only while the app is in the background.
            await NotesReadyNotifications.notesReady(title: written.title, noteID: ref.id)
        } catch is CancellationError {
            notesFinished(ref, .deferred)
        } catch let failure as APIClientError where failure == .httpStatus(400) {
            // The server found nothing to write from.
            notesFinished(ref, .noTranscript)
            if !record.closed { self.error = Self.noTranscript }
        } catch {
            notesFinished(ref, .failed)
            if !record.closed { self.error = "Notes could not be generated. \(error.localizedDescription)" }
        }
    }

    /// Tells the recording's Live Activity how its notes turned out. It listens
    /// only while it says "Writing notes..." for this note.
    private func notesFinished(_ ref: NoteRef, _ outcome: RecordingNotesOutcome) {
        guard let id = UUID(uuidString: ref.id) else { return }
        liveActivity.notesFinished(noteID: id, outcome: outcome)
    }

    /// Whose End the Live Activity's End is (see `RecordingActivityActions`).
    static let activityOwner = "server-recording"

    static func hasTranscript(_ note: NoteDTO) -> Bool {
        note.transcriptRevision > 0 && note.transcriptSegments.contains { $0.isFinal && !$0.text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }
    }

    /// True for a recorded note that ended with nothing transcribed.
    static func endedWithoutTranscript(_ note: NoteDTO) -> Bool {
        note.recordingState == .stopped && !hasTranscript(note)
    }

    // MARK: Live stream

    /// Records where the capture sits on the note timeline, then starts the live stream from the same file.
    private func beginLiveCapture(_ ref: NoteRef, transcript: LiveTranscript) {
        guard let file = recorder.currentFileURL, let record = accounts[ref.account], record.histories[ref.id] != nil else { return }
        // From here until the server has all of it and has stopped the note, the capture is kept and is
        // streamed again by a later launch if this one ends first.
        let baseMs = record.histories[ref.id]!.begin(fileName: file.lastPathComponent)
        do { try record.saveHistories() } catch { self.error = error.localizedDescription }
        let server = record.server, noteID = ref.id
        let configuration = LiveTranscriptClient.Configuration(captureID: "\(noteID)/\(file.lastPathComponent)", baseMs: baseMs)
        guard let source = try? CaptureFileReader(url: file), let client = try? makeLiveClient(server, noteID, source, configuration) else {
            error = "The live transcript could not start, so this recording has no transcript. Stop and start again to retry."
            return
        }
        let session = LiveCaptureSession(client: client, transcript: transcript) {
            guard !record.closed else { return nil }
            return try? await server.noteSnapshot(id: noteID)
        }
        live = session
        liveFile = file.lastPathComponent
        session.start()
    }

    private func pauseChanged(_ paused: Bool) {
        if paused { live?.pause(atSample: recorder.capturedFrames) } else { live?.resume() }
    }

    /// Finishes the live stream: it hands over the rest of the capture, for as
    /// long as that takes. The server's Stop waits for this.
    private func endLive(atSample sample: Int64) -> LiveEnding? {
        guard let session = live else { return nil }
        live = nil
        let fileName = liveFile
        liveFile = nil
        return LiveEnding(fileName: fileName, task: session.end(atSample: sample))
    }

    // MARK: Helpers

    private static func prepare(_ directory: URL) throws {
        let manager = FileManager.default
        try manager.createDirectory(at: directory, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        try manager.setAttributes([.protectionKey: FileProtectionType.completeUntilFirstUserAuthentication], ofItemAtPath: directory.path)
        var excluded = directory
        var values = URLResourceValues()
        values.isExcludedFromBackup = true
        try excluded.setResourceValues(values)
    }

    private static func durationMs(_ note: NoteDTO) -> Int {
        let parts = note.duration.split(separator: ":").compactMap { Double($0) }
        let duration: Double = parts.count == 2 ? parts[0] * 60 + parts[1] : parts.count == 3 ? parts[0] * 3600 + parts[1] * 60 + parts[2] : 0
        return max(Int(duration * 1000), note.transcriptSegments.map { $0.endMs ?? $0.startMs }.max() ?? 0)
    }
}

/// Where each capture of a note sits on the note's timeline. Offsets are fixed
/// when a capture begins. A different device may advance the note while this
/// installation is stopped; only the next local capture moves forward.
struct ServerCaptureHistory: Codable {
    var baseOffsetMs: Int
    var segments: [CaptureSegment]
    var segmentOffsets: [String: Int]?
    var nextStartMs: Int?
    /// Set while the server may still be recording the note because of this
    /// installation: sent again after a launch or sign-in until it arrives.
    var needsStop: Bool?
    /// Set when the person stopped the recording: its notes are written from
    /// the transcript once the Stop has reached the server.
    var needsNotes: Bool?
    /// Captures recorded here that are still on this device, by file name:
    /// `streaming` until the server has confirmed all of the audio, then
    /// `delivered` until the note's Stop has reached the server, when the file
    /// is deleted. Absent in a ledger of an earlier version, whose captures are
    /// never streamed again.
    var deliveries: [String: CaptureDelivery]?

    enum CaptureDelivery: String, Codable { case streaming, delivered }

    /// Captures whose audio the server has not confirmed yet, in timeline order.
    var unfinishedCaptures: [String] {
        (deliveries ?? [:]).filter { $0.value == .streaming }.keys
            .sorted { (segmentOffsets?[$0] ?? 0, $0) < (segmentOffsets?[$1] ?? 0, $1) }
    }

    mutating func markDelivered(_ fileName: String, onDisk: Bool) {
        if onDisk { deliveries?[fileName] = .delivered } else { forgetDelivery(fileName) }
    }

    mutating func forgetDelivery(_ fileName: String) {
        deliveries?[fileName] = nil
        if deliveries?.isEmpty == true { deliveries = nil }
    }

    /// The delivered captures, which the caller deletes; they are no longer listed.
    mutating func takeDelivered() -> [String] {
        let delivered = (deliveries ?? [:]).filter { $0.value == .delivered }.keys.sorted()
        for fileName in delivered { forgetDelivery(fileName) }
        return delivered
    }

    /// A capture is about to be recorded into a new file. Its place on the
    /// note timeline is fixed now, because its audio is streamed while it is
    /// still being recorded. Returns that place.
    mutating func begin(fileName: String) -> Int {
        let offset = nextStartMs ?? endOfSegments
        if segmentOffsets == nil { segmentOffsets = [:] }
        segmentOffsets![fileName] = offset
        if deliveries == nil { deliveries = [:] }
        deliveries![fileName] = .streaming
        return offset
    }

    mutating func prepareForResume(remoteDurationMs: Int) {
        nextStartMs = max(nextStartMs ?? endOfSegments, remoteDurationMs)
        needsStop = true
    }

    mutating func append(_ segment: CaptureSegment) {
        guard !segments.contains(where: { $0.fileName == segment.fileName }) else { return }
        // A capture was placed when it began; one from before that was recorded is placed now.
        let offset = segmentOffsets?[segment.fileName] ?? nextStartMs ?? endOfSegments
        if segmentOffsets == nil { segmentOffsets = [:] }
        segmentOffsets![segment.fileName] = offset
        nextStartMs = offset + Int(segment.duration * 1000)
        segments.append(segment)
    }

    private var endOfSegments: Int { baseOffsetMs + Int(segments.reduce(0) { $0 + $1.duration } * 1000) }
}

/// A little more running time when the app leaves the screen while a
/// transcript is being finished. iOS grants about half a minute; a stream that
/// needs longer is suspended with the app and goes on when it is back, or is
/// streamed again at the next launch.
@MainActor
enum BackgroundTime {
    static func begin(_ name: String) -> UIBackgroundTaskIdentifier {
        var identifier = UIBackgroundTaskIdentifier.invalid
        identifier = UIApplication.shared.beginBackgroundTask(withName: name) {
            UIApplication.shared.endBackgroundTask(identifier)
        }
        return identifier
    }

    static func end(_ identifier: UIBackgroundTaskIdentifier) {
        if identifier != .invalid { UIApplication.shared.endBackgroundTask(identifier) }
    }
}
