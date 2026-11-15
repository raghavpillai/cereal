import SwiftUI
import APIClient
import Contracts

struct ServerHomeView: View {
    @Environment(\.scenePhase) private var scenePhase
    @Environment(ServerNotesModel.self) private var server
    @Environment(Router.self) private var router
    @Environment(ConnectionMode.self) private var mode
    @Environment(ServerRecordingController.self) private var recording
    @State private var creating = false
    @State private var requestingMic = false
    @State private var creationError: String?
    @State private var dayOffset = 0
    @State private var openingEventID: String?
    @State private var entrance = EntranceGate()
    @State private var sections: [ServerHomeSection] = []
    @State private var steppedForward = true
    @Environment(\.motion) private var motion
    @Environment(\.restoringConnection) private var restoring
    @Namespace private var glass
    /// True once the indicator, not the + button, is what the recording screen
    /// returns into. It changes hands only after a transition has settled: a
    /// recording started from + begins while its screen is still growing out of +.
    @State private var indicatorIsZoomSource = false

    var body: some View {
        let _ = PerfProbe.hit("ServerHomeView.body")
        // The note being recorded, if any. This screen follows whether there is a
        // recording, never its clock or level: those are read inside the pill.
        let indicator = recording.indicatorNoteID
        RailScaffold {
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 0) {
                    header
                    upcoming
                        .id(dayOffset)
                        .transition(motion.step(forward: steppedForward))
                    HomeDivider()
                    status
                    if let creationError { Text(creationError).font(.footnote).foregroundStyle(Theme.danger).padding(.bottom, 16) }
                    if let error = recording.error, recording.activeNoteID == nil {
                        Text(error).font(.footnote).foregroundStyle(Theme.danger).padding(.bottom, 16)
                    }
                    // Sections are grouped once per change to the notes, not on every body pass.
                    ForEach(sections) { section in
                        HomeSectionHeader(title: section.title)
                            .staggeredEntrance(section.rows.first?.order ?? 0, gate: entrance)
                        ForEach(section.rows) { item in
                            Button { router.path.append(.serverNote(item.id)) } label: {
                                ServerHomeNoteRow(item: item)
                            }
                                .buttonStyle(RowPressStyle(isOrigin: router.origin == .serverNote(item.id)))
                                .transition(motion.fade)
                                .staggeredEntrance(item.order, gate: entrance)
                                // The next page is asked for while its first row is still a screen away.
                                .onAppear { if item.order >= server.list.rows.count - ServerHomeGrouping.rowsBeforeNextPage { Task { await server.loadMore() } } }
                        }
                        Color.clear.frame(height: 22)
                    }
                    if let failure = server.list.moreError {
                        VStack(alignment: .leading, spacing: 0) {
                            Text("More notes could not load. " + failure.localizedDescription).font(.footnote)
                            Button("Retry") { Task { await server.loadMore() } }
                                .frame(minHeight: 44).contentShape(Rectangle())
                        }.foregroundStyle(Theme.textSecondary).transition(motion.fade)
                    }
                }.padding(.leading, Theme.gutter).padding(.trailing, 8).clearsBottomChrome()
                    .animation(motion.snappy, value: creationError)
                    // Loading turning into content, refreshes, and notes created or deleted elsewhere.
                    .animation(motion.settle, value: sections.map(\.id))
                    .animation(motion.snappy, value: statusKind)
            }.scrollIndicators(.hidden).refreshable { await server.refresh(); await refreshCalendars() }
            .onChange(of: server.list.revision, initial: true) { regroup() }
        } standalone: {
            GlassCircleButton(systemImage: "folder") { SidebarModel.shared.open() }
        } rail: {
            RailGroup {
                RailIcon(systemImage: "magnifyingglass") { router.path.append(.search) }
                RailAvatar(name: "Me") { router.showConnection = true }
            }
        } bottom: {
            HomeBottomBar {
                // The recording carries on while the user is here. The indicator is the
                // recording screen's own pill; it buds off the Ask pill and one tap returns.
                if let indicator {
                    RecordingPill(recorder: recording.recorder, onStop: {}, onStart: {},
                                  onOpen: { router.serverCapture = ServerCaptureTarget(id: indicator) },
                                  zoomsToCapture: indicatorIsZoomSource)
                        .glassMorph("recording", in: glass)
                        .accessibilityIdentifier("recording-indicator")
                }
            } ask: {
                AskAnythingPill(prominent: true) { router.path.append(.serverChat(nil)) }.disabled(server.accountIdentity == nil)
            } add: {
                // While a recording runs, + returns to it and the indicator is what the screen grows out of.
                AccentFAB(busy: creating, zoomsToCapture: !indicatorIsZoomSource) { Task { await createMeeting() } }.accessibilityLabel("Create server note").disabled(server.instance == nil || creating)
            }
            .animation(motion.animation(.gooey, entering: indicator != nil), value: indicator != nil)
            .task(id: indicator != nil) {
                try? await Task.sleep(for: .milliseconds(700))
                if !Task.isCancelled { indicatorIsZoomSource = indicator != nil }
            }
        }
        .toolbarVisibility(.visible, for: .navigationBar)
        .task(id: "\(server.accountIdentity ?? "")/\(dayOffset)") { await refreshCalendars() }
        .onChange(of: server.accountIdentity) {
            openingEventID = nil
            creationError = nil; SidebarModel.shared.close()
        }
        .onChange(of: scenePhase) { _, phase in
            // Day titles ("Earlier today") can be stale after time in the background.
            if phase == .active { regroup(); Task { await refreshCalendars() } }
        }
        .sensoryFeedback(.selection, trigger: dayOffset)
        .sheet(isPresented: $requestingMic) {
            MicPermissionSheet(onGranted: { Task { await createMeeting() } })
                .presentationDetents([.height(MicPermissionSheet.detentHeight)]).presentationDragIndicator(.visible).presentationCornerRadius(32)
                .glassSheet()
        }
    }

    private var selectedCalendarDay: Date {
        Calendar.current.date(byAdding: .day, value: dayOffset, to: .now) ?? .now
    }

    private func refreshCalendars() async {
        let start = Calendar.current.startOfDay(for: selectedCalendarDay)
        guard let end = Calendar.current.date(byAdding: .day, value: 1, to: start) else { return }
        await server.refreshCalendars(from: start, to: end)
    }

    /// Same block as the local Home: 4 pt bar, 10 pt gap, the measured events
    /// text. It reserves the empty line's height so the list does not move
    /// when calendars finish loading.
    @ViewBuilder private var upcoming: some View {
        let snapshot = server.calendarState.snapshot
        let enabledIDs = Set(snapshot.calendars.filter(\.enabled).map(\.id))
        let events = CalendarSchedule.events(snapshot.events, on: selectedCalendarDay, enabledCalendarIDs: enabledIDs, knownCalendarIDs: Set(snapshot.calendars.map(\.id)))
        VStack(alignment: .leading, spacing: 10) {
            if case .failed = server.calendarState {
                HStack {
                    Text("Calendar events could not refresh.").font(.footnote).foregroundStyle(Theme.danger)
                    Button("Retry") { Task { await refreshCalendars() } }.font(.footnote)
                        .frame(minHeight: 44).contentShape(Rectangle())
                }
            }
            if snapshot.personalCalendarIssue || snapshot.calendars.contains(where: { $0.enabled && $0.lastSyncError != nil }) {
                Text("A linked calendar needs attention. Its saved events may be out of date.")
                    .font(.footnote).foregroundStyle(Theme.textSecondary)
                // The one case where the user needs the connections page from here.
                if let instance = server.instance {
                    Link("Calendar connections", destination: instance.url.appendingPathComponent("account"))
                        .font(.footnote).foregroundStyle(Theme.textSecondary)
                        .frame(minHeight: 44).contentShape(Rectangle())
                }
            }
            if !events.isEmpty {
                ForEach(events, id: \.id) { event in
                    HStack(alignment: .center, spacing: 8) {
                        Button { Task { await openEvent(event) } } label: {
                            HomeEventsBlock(barColor: calendarColor(event.color)) {
                                Text(event.title).font(.system(size: HomeMetrics.eventsText, weight: .medium)).foregroundStyle(Theme.textPrimary)
                                Text(eventTime(event)).font(.system(size: 14)).foregroundStyle(Theme.textSecondary)
                                Text(event.calendarName).font(.system(size: 12)).foregroundStyle(Theme.textSecondary)
                                if let location = event.location, !location.isEmpty { Text(location).font(.caption).foregroundStyle(Theme.textSecondary) }
                            }.frame(maxWidth: .infinity, alignment: .leading).contentShape(Rectangle())
                        }.buttonStyle(.plain).disabled(openingEventID != nil || creating)
                            .accessibilityLabel("Open note for \(event.title), \(event.calendarName)")
                        if openingEventID == event.id { ProgressView() }
                        if let url = CalendarSchedule.meetingURL(event.meetingUrl) {
                            Link("Join", destination: url).font(.footnote).foregroundStyle(Theme.accent)
                                .frame(minWidth: 44, minHeight: 44).contentShape(Rectangle())
                        }
                    }
                }
            } else if case .loaded = server.calendarState {
                if snapshot.personalCalendarsLinked || snapshot.calendars.contains(where: \.enabled) {
                    HomeEventsEmpty(text: dayOffset == 0 ? "No more events today" : "No events")
                } else { HomeEventsEmpty(text: "Link calendars to see upcoming meetings") }
            } else if case .loading = server.calendarState, snapshot.calendars.isEmpty {
                // Holds the block's place while the first calendars load.
                HomeEventsEmpty(text: "Loading calendars…")
            }
        }
        .frame(minHeight: 36.7, alignment: .topLeading)
        .accessibilityElement(children: .contain).accessibilityLabel("Upcoming calendar events")
    }

    private func calendarColor(_ text: String) -> Color {
        let hex = text.hasPrefix("#") ? String(text.dropFirst()) : text
        return Color(hex: hex.count == 6 ? UInt32(hex, radix: 16) ?? 0x87E5E8 : 0x87E5E8)
    }

    private func eventTime(_ event: CalendarEventDTO) -> String {
        if event.allDay { return "All day" }
        guard let start = CalendarSchedule.timestamp(event.startAt), let end = CalendarSchedule.timestamp(event.endAt) else { return "Time unavailable" }
        return start.formatted(date: .omitted, time: .shortened) + " – " + end.formatted(date: .omitted, time: .shortened)
    }

    private func openEvent(_ event: CalendarEventDTO) async {
        guard openingEventID == nil, !creating else { return }
        let activeAtGesture = CalendarSchedule.isActiveTimedEvent(event)
        openingEventID = event.id; creationError = nil
        defer { openingEventID = nil }
        do {
            let note = try await server.noteForCalendarEvent(id: event.id)
            if activeAtGesture && CalendarSchedule.isActiveTimedEvent(event) {
                if recording.isCapturing, recording.activeNoteID != note.id {
                    creationError = "A recording is already active in another note."
                    router.path.append(.serverNote(note.id))
                } else { router.serverCapture = ServerCaptureTarget(id: note.id) }
            } else { router.path.append(.serverNote(note.id)) }
        } catch is CancellationError { }
        catch { creationError = error.localizedDescription }
    }

    private func createMeeting() async {
        guard !creating else { return }
        if let id = recording.indicatorNoteID {
            router.serverCapture = ServerCaptureTarget(id: id)
            return
        }
        guard await MicPermission.isGranted else { requestingMic = true; return }
        creating = true; creationError = nil
        defer { creating = false }
        do {
            let note = try await server.create(title: "", body: "")
            router.serverCapture = ServerCaptureTarget(id: note.id)
        } catch { creationError = error.localizedDescription }
    }

    private var header: some View {
        HomeHeader(title: dayOffset == 0 ? "Today" : (Calendar.current.date(byAdding: .day, value: dayOffset, to: .now) ?? .now).formatted(.dateTime.weekday(.wide)),
                   canGoBack: dayOffset > 0,
                   back: { steppedForward = false; withAnimation(motion.snappy) { dayOffset = max(0, dayOffset - 1) } },
                   forward: { steppedForward = true; withAnimation(motion.snappy) { dayOffset += 1 } })
    }

    private func regroup() {
        sections = Signpost.interval("Home.Group") { ServerHomeGrouping.sections(server.list.rows) }
    }


    /// Drives the cross-fade between the loading, error, empty and loaded states.
    private var statusKind: Int {
        switch server.state {
        case .disconnected: 0
        case .loading: 1
        case .failed: 2
        case .loaded: server.list.rows.isEmpty ? 3 : 4
        }
    }

    @ViewBuilder private var status: some View {
        switch server.state {
        case .disconnected:
            if restoring {
                // A signed-in launch is restoring its session; do not flash "Connect".
                loadingRows
            } else {
                VStack(alignment: .leading, spacing: 0) {
                    Text("Connect to your instance to read and edit its notes.")
                    Button("Connection settings") { router.showConnection = true }
                        .frame(minHeight: 44).contentShape(Rectangle())
                }.foregroundStyle(Theme.textSecondary)
            }
        case .loading:
            if server.list.rows.isEmpty { loadingRows }
        case .failed(let error, _):
            VStack(alignment: .leading, spacing: 0) {
                Text(error.localizedDescription)
                if !server.list.rows.isEmpty { Text("Showing the last loaded notes.").font(.footnote).padding(.top, 10) }
                Button("Retry") { Task { await server.refresh() } }
                    .frame(minHeight: 44).contentShape(Rectangle())
            }.foregroundStyle(Theme.textSecondary).padding(.bottom, 6)
        case .loaded:
            if server.list.rows.isEmpty { Text("No notes yet. Create your first note with +.").foregroundStyle(Theme.textSecondary) }
        }
    }

    /// Row-shaped placeholders, so the list does not jump when notes arrive.
    private var loadingRows: some View {
        VStack(alignment: .leading, spacing: 0) {
            ForEach(0..<3, id: \.self) { _ in
                NoteListRow(tile: .document, title: "Loading note title", subtitle: "0:00 PM")
            }
        }
        .redacted(reason: .placeholder)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Loading notes")
    }
}

/// A note's row. It decides the tile itself, so audio being delivered for one
/// note redraws rows, not the whole Home.
private struct ServerHomeNoteRow: View {
    let item: ServerHomeRow
    @Environment(ServerRecordingController.self) private var recording

    var body: some View {
        // Audio still on this phone comes first; otherwise a meeting shows the other party's initial.
        NoteListRow(tile: recording.activeNoteID == item.id || recording.hasPendingAudio(noteID: item.id) ? .phone : item.tile,
                    title: item.title, subtitle: item.subtitle)
    }
}

/// One day of notes on the server Home, with everything a row needs already computed.
struct ServerHomeSection: Identifiable, Equatable {
    let id: Date
    let title: String
    let rows: [ServerHomeRow]
}

struct ServerHomeRow: Identifiable, Equatable {
    let id: String
    let title: String
    let subtitle: String
    /// A letter for a meeting with other people, a document otherwise.
    let tile: NoteTile
    /// Position in the whole list, for the entrance stagger.
    let order: Int
}

/// What Home needs from a note to place and show its row. A list row has it,
/// and so does a whole note, which knows nobody who was invited.
protocol HomeListNote {
    var id: String { get }
    var title: String { get }
    var date: String { get }
    var createdAt: String { get }
    /// The other party of a meeting, when there is one.
    var homeAttendee: String? { get }
}
extension ServerNoteRow: HomeListNote { var homeAttendee: String? { firstAttendee } }
extension NoteDTO: HomeListNote { var homeAttendee: String? { nil } }

/// A note with its dates parsed once. (A generic function cannot declare its own type.)
private struct DatedHomeNote<Item: HomeListNote> { let note: Item; let created: Date; let timeline: Date }

enum ServerHomeGrouping {
    /// Rows from the end of the loaded list at which the next page is asked for.
    static let rowsBeforeNextPage = 12

    /// The square at the head of a row: the other party's initial for a meeting, a document otherwise.
    static func tile(for note: some HomeListNote) -> NoteTile {
        note.homeAttendee.map(NoteTile.meeting(with:)) ?? .document
    }

    /// Parses each note's dates once, sorts newest first, and groups by local day.
    static func sections<Item: HomeListNote>(_ notes: [Item], now: Date = .now, calendar: Calendar = .current) -> [ServerHomeSection] {
        typealias Dated = DatedHomeNote<Item>
        var dated: [Dated] = []
        dated.reserveCapacity(notes.count)
        for note in notes {
            let created: Date = Date.serverTimestamp(note.createdAt) ?? .distantPast
            let timeline: Date = MeetingPresentation.timelineDate(noteDate: note.date, createdAt: note.createdAt) ?? created
            dated.append(Dated(note: note, created: created, timeline: timeline))
        }
        dated.sort { (left: Dated, right: Dated) -> Bool in
            if left.timeline == right.timeline { return left.created > right.created }
            return left.timeline > right.timeline
        }

        var order: [Date] = []
        var groups: [Date: [ServerHomeRow]] = [:]
        for (index, item) in dated.enumerated() {
            let day = calendar.startOfDay(for: item.timeline)
            if groups[day] == nil { order.append(day) }
            groups[day, default: []].append(ServerHomeRow(
                id: item.note.id,
                title: item.note.title.isEmpty ? "Untitled note" : item.note.title,
                subtitle: item.created.formatted(date: .omitted, time: .shortened),
                tile: tile(for: item.note),
                order: index))
        }
        return order.map { day in
            ServerHomeSection(id: day, title: title(for: day, now: now, calendar: calendar), rows: groups[day] ?? [])
        }
    }

    static func title(for day: Date, now: Date, calendar: Calendar) -> String {
        if calendar.isDate(day, inSameDayAs: now) { return "Earlier today" }
        if let yesterday = calendar.date(byAdding: .day, value: -1, to: now), calendar.isDate(day, inSameDayAs: yesterday) { return "Yesterday" }
        // Notes from another year say so, and are never merged with this year's.
        if calendar.component(.year, from: day) != calendar.component(.year, from: now) {
            return day.formatted(.dateTime.weekday(.abbreviated).day().month(.abbreviated).year())
        }
        return day.formatted(.dateTime.weekday(.abbreviated).day().month(.abbreviated))
    }
}
