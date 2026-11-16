import Foundation
import Observation
import UIKit

private final class LifecycleObservers: @unchecked Sendable {
    var tokens: [NSObjectProtocol] = []
    deinit { tokens.forEach(NotificationCenter.default.removeObserver) }
}

/// The on-device demo's store, for debug QA only (`-screen` launches). Server
/// notes live in ServerNotesModel. A release build has no demo: its store is
/// empty and never reads or writes the disk, so a `notes.json` and recordings
/// left by an earlier version stay exactly as they were.
@MainActor
@Observable
final class NoteStore {
    private(set) var notes: [Note] = []
    var events: [CalendarEvent] = []
    var currentUser = Person(name: "Alex Rivera", email: nil)
    private(set) var persistenceError: String?
    private(set) var captureRecoveryMessage: String?

    private let fileURL: URL
    private let recordingsRoots: [URL]
    @ObservationIgnored private var generationEpoch: [UUID: UInt64] = [:]
    @ObservationIgnored private var persistenceBlocked = false

    // Draft persistence. Every write, immediate or deferred, goes through one
    // serial queue, so a deferred write of an older snapshot can never land
    // after a newer immediate one.
    @ObservationIgnored private let writer = DispatchQueue(label: AppIdentity.subsystem + ".notes-writer", qos: .userInitiated)
    @ObservationIgnored private let draftDelay: Duration
    @ObservationIgnored private var draftTask: Task<Void, Never>?
    @ObservationIgnored private var draftPending = false
    @ObservationIgnored private var writeSequence: UInt64 = 0
    @ObservationIgnored private let lifecycle = LifecycleObservers()
    /// Completed file writes. For tests and the debug performance probe.
    @ObservationIgnored private(set) var diskWriteCount = 0
    /// True while typed text is in memory but not yet in the file.
    var hasUnsavedDraft: Bool { draftPending }

    /// The store this build uses: the demo's file in debug builds, nothing on disk in release.
    static func forThisBuild() -> NoteStore {
        #if DEBUG
        NoteStore()
        #else
        NoteStore(onDisk: false)
        #endif
    }

    init(fileURL: URL? = nil, recordingsRoots: [URL]? = nil, draftDelay: Duration = .milliseconds(400), onDisk: Bool = true) {
        self.draftDelay = draftDelay
        let dir = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        self.fileURL = fileURL ?? dir.appendingPathComponent("notes.json")
        self.recordingsRoots = recordingsRoots ?? (fileURL.map {
            [$0.deletingLastPathComponent().appendingPathComponent("recordings", isDirectory: true)]
        } ?? CaptureRecovery.defaultRoots)
        // Off disk: nothing is read, recovered or written.
        guard onDisk else { persistenceBlocked = true; return }
        Signpost.interval("Launch.StoreLoad") { load() }
        // No edit may be lost: write a pending draft before the app can be suspended or ended.
        for name in [UIApplication.willResignActiveNotification, UIApplication.didEnterBackgroundNotification, UIApplication.willTerminateNotification] {
            lifecycle.tokens.append(NotificationCenter.default.addObserver(forName: name, object: nil, queue: nil) { [weak self] _ in
                MainActor.assumeIsolated { self?.flush() }
            })
        }
    }

    var activeNotes: [Note] { notes.filter { !$0.trashed }.sorted { $0.createdAt > $1.createdAt } }

    /// Notes grouped into the Home sections: "Earlier today", "Yesterday", then "Fri 30 Oct".
    var groupedNotes: [(title: String, notes: [Note])] {
        let cal = Calendar.current
        let groups = Dictionary(grouping: activeNotes) { cal.startOfDay(for: $0.createdAt) }
        return groups.sorted { $0.key > $1.key }.map { day, notes in
            let title: String
            if cal.isDateInToday(day) { title = "Earlier today" }
            else if cal.isDateInYesterday(day) { title = "Yesterday" }
            else { title = day.formatted(.dateTime.weekday(.abbreviated).day().month(.abbreviated)) }
            return (title, notes.sorted { $0.createdAt > $1.createdAt })
        }
    }

    func note(_ id: UUID) -> Note? { notes.first { $0.id == id } }

    @discardableResult
    func createNote() -> Note {
        let note = Note(title: "", createdAt: .now, attendees: [currentUser])
        notes.append(note)
        persist()
        return note
    }

    func update(_ note: Note) {
        guard let i = notes.firstIndex(where: { $0.id == note.id }) else { return }
        var edited = note
        // Capture lifecycle fields are owned by finishCapture/generateNotes;
        // an editor holding an older Note must never rewind those fields.
        edited.captureSegments = notes[i].captureSegments
        edited.transcriptRevision = notes[i].transcriptRevision
        edited.notesGeneratedRevision = notes[i].notesGeneratedRevision
        edited.transcript = notes[i].transcript
        edited.captureRecoveryHint = notes[i].captureRecoveryHint
        notes[i] = edited
        persist()
    }

    func updateTitle(_ id: UUID, title: String) {
        guard let i = notes.firstIndex(where: { $0.id == id }) else { return }
        notes[i].title = title
        persist()
    }

    func updateUserNotes(_ id: UUID, text: String) {
        guard let i = notes.firstIndex(where: { $0.id == id }) else { return }
        notes[i].userNotes = text
        persist()
    }

    /// The typing path. Memory changes at once; the file is written after a
    /// short pause in typing, off the main actor. `flush()` and every other
    /// mutation write immediately and include the draft.
    func updateDraft(_ id: UUID, title: String, userNotes: String) {
        guard let i = notes.firstIndex(where: { $0.id == id }) else { return }
        guard notes[i].title != title || notes[i].userNotes != userNotes else { return }
        if notes[i].title != title { notes[i].title = title }
        if notes[i].userNotes != userNotes { notes[i].userNotes = userNotes }
        draftPending = true
        draftTask?.cancel()
        draftTask = Task { [weak self, draftDelay] in
            do { try await Task.sleep(for: draftDelay) } catch { return }
            self?.writeDraftInBackground()
        }
    }

    /// Writes a pending draft now, on the caller. Safe to call at any time.
    func flush() {
        guard draftPending else { return }
        persist()
    }

    private func writeDraftInBackground() {
        guard draftPending, !persistenceBlocked else { return }
        draftPending = false
        draftTask = nil
        writeSequence &+= 1
        let sequence = writeSequence
        let snapshot = notes
        let url = fileURL
        writer.async { [weak self] in
            let succeeded = Self.write(snapshot, to: url)
            Task { @MainActor [weak self] in
                guard let self else { return }
                diskWriteCount += 1
                // A newer write has already reported its own result.
                guard sequence == writeSequence else { return }
                if succeeded { persistenceError = nil } else {
                    // Keep the edit pending so the next flush tries again.
                    draftPending = true
                    persistenceError = "Local notes couldn’t be saved. Your current edits remain in memory."
                }
            }
        }
    }

    private nonisolated static func write(_ notes: [Note], to url: URL) -> Bool {
        Signpost.interval("Notes.Persist") {
            PerfProbe.hit("NoteStore.diskWrite")
            do {
                try JSONEncoder().encode(notes).write(to: url, options: .atomic)
                return true
            } catch { return false }
        }
    }

    func trash(_ id: UUID) {
        guard let i = notes.firstIndex(where: { $0.id == id }) else { return }
        notes[i].trashed = true
        persist()
    }

    /// Drops empty notes the user abandoned (no title, notes, or capture).
    func discardIfEmpty(_ id: UUID) {
        guard let n = note(id), n.isEmpty else { return }
        notes.removeAll { $0.id == id }
        persist()
    }

    // MARK: Capture lifecycle

    /// Resuming capture invalidates an in-flight preview generation even before
    /// another Stop increments the transcript revision.
    func invalidateGeneration(_ id: UUID) {
        generationEpoch[id] = (generationEpoch[id] ?? 0) &+ 1
    }

    /// A stopped segment appends to the note's one transcript and bumps its revision.
    @discardableResult
    func finishCapture(_ id: UUID, segment: CaptureSegment) -> Bool {
        guard let i = notes.firstIndex(where: { $0.id == id }) else { return false }
        if !notes[i].captureSegments.contains(where: { $0.index == segment.index || $0.fileName == segment.fileName }) {
            notes[i].captureSegments.append(segment)
            notes[i].transcriptRevision += 1
        }
        guard persist() else { return false }
        // A callback alone is not a commit. Only clear metadata once the note is on disk.
        guard notes[i].captureSegments.contains(where: { $0.fileName == segment.fileName }) else { return false }
        CaptureRecovery.acknowledge(noteID: id, fileName: segment.fileName, roots: recordingsRoots)
        return true
    }

    /// Generates enhanced notes for the current transcript revision. No-op if they're current.
    func generateNotes(_ id: UUID, using generator: any NotesGenerator = LocalNotesGenerator()) async {
        guard let snapshot = note(id), !snapshot.trashed, snapshot.needsGeneration else { return }
        let epoch = (generationEpoch[id] ?? 0) &+ 1
        generationEpoch[id] = epoch
        let revision = snapshot.transcriptRevision
        let sections = await generator.generate(for: snapshot)
        guard !Task.isCancelled, generationEpoch[id] == epoch,
              let i = notes.firstIndex(where: { $0.id == id }),
              !notes[i].trashed, notes[i].transcriptRevision == revision,
              notes[i].userNotes == snapshot.userNotes,
              notes[i].sections == snapshot.sections,
              notes[i].title == snapshot.title else { return }
        notes[i].sections = sections
        notes[i].notesGeneratedRevision = revision
        if notes[i].title.isEmpty { notes[i].title = sections.first?.heading ?? "" }
        persist()
    }

    // MARK: Persistence

    private func load() {
        let existed = FileManager.default.fileExists(atPath: fileURL.path)
        if !existed {
            notes = SampleData.notes()
        } else { do {
            notes = try JSONDecoder().decode([Note].self, from: Data(contentsOf: fileURL))
        } catch {
            // Preserve the existing bytes for recovery. Do not replace an
            // unreadable store with examples or a later in-memory edit.
            notes = []
            persistenceBlocked = true
            persistenceError = "Local notes couldn’t be opened. The original file is preserved; new demo edits will stay in memory until it is recovered."
        } }
        // Recovery must run before filtering fresh, still-untitled notes.
        let journalsToAcknowledge = recoverCaptures()
        notes = notes.filter { !$0.isEmpty }
        if !existed || !journalsToAcknowledge.isEmpty || captureRecoveryMessage != nil {
            if persist() { journalsToAcknowledge.forEach(CaptureRecovery.acknowledge) }
        }
    }

    private func recoverCaptures() -> [URL?] {
        var acknowledged: [URL?] = []
        for capture in CaptureRecovery.scan(roots: recordingsRoots) {
            if !notes.contains(where: { $0.id == capture.noteID }) {
                notes.append(Note(id: capture.noteID, title: "", createdAt: capture.startedAt))
            }
            guard let i = notes.firstIndex(where: { $0.id == capture.noteID }) else { continue }
            if notes[i].captureSegments.contains(where: { $0.fileName == capture.audioFile.lastPathComponent }) {
                guard capture.duration != nil else {
                    notes[i].captureRecoveryHint = "An unfinished recording could not be finalized. Any audio files remain on this device; nothing was uploaded or transcribed."
                    continue
                }
                if capture.journalFile != nil { acknowledged.append(capture.journalFile) }
                continue
            }
            guard let segment = capture.segment,
                  !notes[i].captureSegments.contains(where: { $0.index == capture.index }) else {
                notes[i].captureRecoveryHint = "An unfinished recording could not be finalized. Any audio files remain on this device; nothing was uploaded or transcribed."
                continue
            }
            notes[i].captureSegments.append(segment)
            notes[i].captureSegments.sort { $0.index < $1.index }
            notes[i].transcriptRevision += 1
            notes[i].captureRecoveryHint = "Recovered audio from an interrupted recording. Recording is stopped; tap Resume when ready."
            acknowledged.append(capture.journalFile)
        }
        if notes.contains(where: { $0.captureRecoveryHint != nil && !$0.trashed }) {
            captureRecoveryMessage = "An interrupted recording was found. Open its note to review recovery status; recording has not resumed."
        }
        return acknowledged
    }

    /// An immediate, durable write of everything in memory, including any
    /// pending draft. Waits for a deferred write already in progress, so the
    /// file always ends with the newest content.
    @discardableResult
    private func persist() -> Bool {
        guard !persistenceBlocked else { return false }
        let hadDraft = draftPending
        draftTask?.cancel()
        draftTask = nil
        draftPending = false
        writeSequence &+= 1
        let snapshot = notes
        let url = fileURL
        let succeeded = writer.sync { Self.write(snapshot, to: url) }
        diskWriteCount += 1
        if succeeded {
            persistenceError = nil
        } else {
            // The draft is still only in memory; the next flush tries again.
            draftPending = hadDraft
            persistenceError = "Local notes couldn’t be saved. Your current edits remain in memory."
        }
        return succeeded
    }
}

enum SampleData {
    static func notes() -> [Note] {
        let cal = Calendar.current
        func at(daysAgo: Int, _ h: Int, _ m: Int) -> Date {
            let day = cal.date(byAdding: .day, value: -daysAgo, to: .now)!
            return cal.date(bySettingHour: h, minute: m, second: 0, of: day)!
        }
        let sam = Person(name: "Sam Okafor"), jo = Person(name: "Jo Lindqvist")
        return [
            Note(
                title: "Lumen pitch: climate data marketplace for insurers",
                createdAt: at(daysAgo: 0, 18, 36),
                attendees: [sam],
                sections: [
                    NoteSection(heading: "Lumen: What They're Building", bullets: [
                        "Data company capturing *how* regional climate risk changes, not just the current snapshot",
                        "Consumer weather app is the vehicle for data collection, not the core product",
                        "Target buyers: insurers, reinsurers, municipal planners, agriculture lenders",
                        "Competitive framing: catastrophe models are annual and coarse; Lumen updates street-level risk weekly",
                        "Key differentiator: sensor network crowdsourced from app users gives ground truth others lack",
                    ]),
                    NoteSection(heading: "Traction and Metrics", bullets: [
                        "40k weekly active app users across three metro areas",
                        "Two paid pilots with regional insurers, $60k combined",
                        "Raising $2.5M seed to expand sensors to five new markets",
                    ]),
                    NoteSection(heading: "Open Questions", bullets: [
                        "How defensible is the sensor network if a large carrier builds its own?",
                        "What does churn look like after the pilots end?",
                    ]),
                ],
                transcript: [
                    TranscriptSegment(speaker: "Sam", startedAt: 0, text: "So the app is really how we get the data."),
                    TranscriptSegment(speaker: "You", startedAt: 6, text: "And who's paying today?"),
                    TranscriptSegment(speaker: "Sam", startedAt: 9, text: "Two regional insurers, both on pilots."),
                ]
            ),
            Note(title: "Interview prep and pitch positioning", createdAt: at(daysAgo: 1, 0, 1), sections: [
                NoteSection(heading: "Positioning", bullets: ["Lead with the wedge customer, not the platform vision", "One sentence on why now"]),
            ]),
            Note(title: "User acquisition and fundraising strategy", createdAt: at(daysAgo: 13, 0, 1), sections: [
                NoteSection(heading: "Channels", bullets: ["Community-led launch first", "Paid only after retention is proven"]),
            ]),
            Note(title: "Security startup ideas and agent tooling", createdAt: at(daysAgo: 14, 1, 53)),
            Note(title: "Model evaluation, safety and inference costs", createdAt: at(daysAgo: 15, 21, 21)),
            Note(title: "Community posting strategy with weekly cadence", createdAt: at(daysAgo: 24, 22, 10)),
            Note(title: "Router model comparison and data pipeline", createdAt: at(daysAgo: 28, 17, 26)),
            Note(title: "Northwind / Acme: Commercial Proposal", createdAt: at(daysAgo: 28, 13, 30), attendees: [sam, jo]),
        ]
    }
}

extension Note {
    var isEmpty: Bool { title.isEmpty && userNotes.isEmpty && sections.isEmpty && transcript.isEmpty && captureSegments.isEmpty && !hasCapture && captureRecoveryHint == nil }
}

/// Local preview generation boundary. A future server provider will consume
/// the explicitly selected transcript revision through its own API contract.
protocol NotesGenerator: Sendable {
    func generate(for note: Note) async -> [NoteSection]
}

/// Offline demo outline from typed notes and recording metadata; no transcript
/// or server AI result is invented.
struct LocalNotesGenerator: NotesGenerator {
    func generate(for note: Note) async -> [NoteSection] {
        try? await Task.sleep(for: .milliseconds(1200))
        let lines = note.userNotes
            .split(whereSeparator: \.isNewline)
            .map { $0.trimmingCharacters(in: .whitespaces).trimmingCharacters(in: CharacterSet(charactersIn: "-•* ")) }
            .filter { !$0.isEmpty }
        let minutes = Int(note.captureSegments.reduce(0) { $0 + $1.duration } / 60)
        var sections: [NoteSection] = []
        if !lines.isEmpty { sections.append(NoteSection(heading: "Key Points", bullets: lines)) }
        sections.append(NoteSection(heading: "Recording", bullets: [
            "\(note.captureSegments.count) capture segment\(note.captureSegments.count == 1 ? "" : "s"), about \(max(1, minutes)) min",
            "Generated from transcript revision \(note.transcriptRevision)",
        ]))
        return sections
    }
}
