import SwiftUI
import APIClient
import Contracts

/// Home for the accounts on this device. It is built here, from each account's
/// own list and calendar; with one account it is that account's Home.
struct ServerHomeView: View {
    @Environment(\.scenePhase) private var scenePhase
    @Environment(AccountsModel.self) private var accounts
    @Environment(Router.self) private var router
    @Environment(ConnectionMode.self) private var mode
    @Environment(ServerRecordingController.self) private var recording
    @State private var creating = false
    @State private var requestingMic = false
    @State private var creationError: String?
    @State private var dayOffset = 0
    @State private var openingEvent: EventRef?
    @State private var entrance = EntranceGate()
    @State private var sections: [ServerHomeSection] = []
    @State private var steppedForward = true
    /// The day whose calendars were last read to the end. While that same day
    /// is read again (each return to Home, or the app coming back) its events
    /// stay on screen instead of blinking out until the answer arrives.
    @State private var calendarsReadFor: Int?
    @Environment(\.motion) private var motion
    @Environment(\.restoringConnection) private var restoring
    @Namespace private var glass
    /// True once the indicator, not the + button, is what the recording screen
    /// returns into. It changes hands only after a transition has settled: a
    /// recording started from + begins while its screen is still growing out of +.
    @State private var indicatorIsZoomSource = false

    /// The accounts whose rows are listed. An account that must sign in again is
    /// left out of the list (its notes are not gone; this is only what is shown).
    private var listed: [AccountSession] { accounts.sessions.filter { $0.status != .needsSignIn } }
    /// Rows of every listed account, each with its account. Grouping sorts them by day.
    private var rows: [AccountNoteRow] {
        listed.flatMap { session in session.notes.list.rows.map { AccountNoteRow(account: session.notes.accountID, row: $0) } }
    }
    /// Changes when any account's rows, or the accounts themselves, change.
    private var listSignature: [String] {
        accounts.sessions.map { "\($0.key)/\($0.notes.list.revision)/\($0.status == .needsSignIn)" }
    }

    var body: some View {
        let _ = PerfProbe.hit("ServerHomeView.body")
        // The note being recorded, if any. This screen follows whether there is a
        // recording, never its clock or level: those are read inside the pill.
        let indicator = recording.indicator
        let total = sections.reduce(0) { $0 + $1.rows.count }
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
                    if let error = recording.error, recording.active == nil {
                        Text(error).font(.footnote).foregroundStyle(Theme.danger).padding(.bottom, 16)
                    }
                    // Sections are grouped once per change to the notes, not on every body pass.
                    ForEach(sections) { section in
                        HomeSectionHeader(title: section.title)
                            .staggeredEntrance(section.rows.first?.order ?? 0, gate: entrance)
                        ForEach(section.rows, id: \.listID) { item in
                            Button { if let ref = item.ref { router.path.append(.serverNote(ref)) } } label: {
                                ServerHomeNoteRow(item: item)
                            }
                                .buttonStyle(RowPressStyle(isOrigin: item.ref.map { router.origin == .serverNote($0) } ?? false))
                                .transition(motion.fade)
                                .staggeredEntrance(item.order, gate: entrance)
                                // The next page is asked for while its first row is still a screen away.
                                .onAppear { if item.order >= total - ServerHomeGrouping.rowsBeforeNextPage { loadMore() } }
                        }
                        Color.clear.frame(height: 22)
                    }
                    if let failure = listed.lazy.compactMap({ $0.notes.list.moreError }).first {
                        VStack(alignment: .leading, spacing: 0) {
                            Text("More notes could not load. " + failure.localizedDescription).font(.footnote)
                            Button("Retry") { loadMore() }
                                .foregroundStyle(Theme.accentText)
                                .frame(minHeight: 44).contentShape(Rectangle())
                        }.foregroundStyle(Theme.textSecondary).transition(motion.fade)
                    }
                }.padding(.leading, Theme.gutter).padding(.trailing, 8).clearsBottomChrome()
                    .animation(motion.snappy, value: creationError)
                    // Loading turning into content, refreshes, and notes created or deleted elsewhere.
                    .animation(motion.settle, value: sections.map(\.id))
                    .animation(motion.snappy, value: statusKind)
            }.scrollIndicators(.hidden).refreshable { await refresh() }
            .onChange(of: listSignature, initial: true) { regroup() }
        } leading: {
            // As in the reference: the notes library leads the bar on a phone and heads the
            // rail on a Duo; search and the account trail it in one capsule.
            HomeNavigation.Notes()
        } rail: {
            HomeNavigation.Group(name: accounts.defaultSession?.notes.accountIdentity.map { _ in "Me" } ?? "Me")
        } bottom: {
            HomeBottomBar {
                // The recording carries on while the user is here. The indicator is the
                // recording screen's own pill; it buds off the Ask pill and one tap returns.
                if let indicator {
                    // As on the desktop: the note's title, and a close control that stops it.
                    RecordingPill(recorder: recording.recorder, onStop: {}, onStart: {},
                                  onOpen: { router.serverCapture = ServerCaptureTarget(indicator) },
                                  zoomsToCapture: indicatorIsZoomSource,
                                  title: accounts.sessions.first { $0.id == indicator.account }?.notes.title(ofNote: indicator.id) ?? "",
                                  onClose: { recording.stop() })
                        .glassMorph("recording", in: glass)
                        .accessibilityIdentifier("recording-indicator")
                }
            } ask: {
                // Until a chat can choose its account, questions across meetings go to the default account.
                // Chat opens on a new conversation; the one that was open is kept in its history.
                AskAnythingPill(prominent: true) {
                    if let route = ChatNavigation.workspaceChat(accounts) { router.path.append(route) }
                }.disabled(accounts.defaultSession?.notes.accountIdentity == nil)
            } add: {
                // While a recording runs, + returns to it and the indicator is what the screen grows out of.
                AccentFAB(busy: creating, zoomsToCapture: !indicatorIsZoomSource) { Task { await createMeeting() } }.accessibilityLabel("Create server note")
                    .disabled(accounts.defaultSession?.notes.instance == nil || creating)
            }
            .animation(motion.animation(.gooey, entering: indicator != nil), value: indicator != nil)
            .task(id: indicator != nil) {
                try? await Task.sleep(for: .milliseconds(700))
                if !Task.isCancelled { indicatorIsZoomSource = indicator != nil }
            }
        }
        .toolbarVisibility(.visible, for: .navigationBar)
        #if DEBUG
        // QA only: `-qa.dayOffset 1` opens Home on another day, for screenshots of the day pager.
        .onAppear { if let offset = Int(UserDefaults.standard.string(forKey: "qa.dayOffset") ?? "") { dayOffset = offset } }
        #endif
        .task(id: "\(accounts.signature)/\(dayOffset)") { await refreshCalendars() }
        .onChange(of: accounts.sessions.map(\.id)) {
            openingEvent = nil
            creationError = nil; SidebarModel.shared.close()
        }
        .onChange(of: scenePhase) { _, phase in
            // Day titles ("Earlier today") can be stale after time in the background.
            if phase == .active {
                regroup()
                Task { await accounts.retryUnreachable(); await refreshCalendars() }
            }
        }
        .haptic(.selection, trigger: dayOffset)
        // A meeting that could not be opened or made: felt as the error it is.
        .haptic(.error, trigger: creationError) { old, new in new != nil && new != old }
        .sheet(isPresented: $requestingMic) {
            MicPermissionSheet(onGranted: { Task { await createMeeting() } })
                .presentationDetents([.height(MicPermissionSheet.detentHeight)]).presentationDragIndicator(.visible).presentationCornerRadius(32)
                .glassSheet()
        }
    }

    private var selectedCalendarDay: Date {
        Calendar.current.date(byAdding: .day, value: dayOffset, to: .now) ?? .now
    }

    private func refresh() async {
        await accounts.retryUnreachable()
        await withTaskGroup(of: Void.self) { group in
            for session in accounts.sessions where session.isReady { group.addTask { await session.notes.refresh() } }
        }
        await refreshCalendars()
    }

    /// Each account pages its own list.
    private func loadMore() {
        for session in listed where session.notes.list.hasMore { Task { await session.notes.loadMore() } }
    }

    /// Each account is asked for its own calendars, with its own client.
    private func refreshCalendars() async {
        let day = dayOffset
        let start = Calendar.current.startOfDay(for: selectedCalendarDay)
        guard let end = Calendar.current.date(byAdding: .day, value: 1, to: start) else { return }
        await withTaskGroup(of: Void.self) { group in
            for session in accounts.sessions where session.isReady { group.addTask { await session.notes.refreshCalendars(from: start, to: end) } }
        }
        if !Task.isCancelled, day == dayOffset { calendarsReadFor = day }
        if day == 0, UpcomingMeetingNotifications.isEnabled { await scheduleReminders() }
        // Only an answer read for today decides the cards; a failed read leaves them as they are.
        if day == 0, !Task.isCancelled, calendarsLoadedForCards { await syncMeetingActivities() }
    }

    private var calendarsLoadedForCards: Bool {
        // An account that cannot be reached just now keeps its cards: nothing was learned about its day.
        accounts.sessions.allSatisfy { session in
            guard session.isReady, case .loaded = session.notes.calendarState else { return false }
            return true
        }
    }

    /// The Lock Screen's "Take a note" card for each of today's meetings still to come or under way.
    private func syncMeetingActivities() async {
        let today = Date.now
        let events = accounts.sessions.filter(\.isReady).flatMap { session -> [MeetingLiveActivities.Event] in
            guard let account = session.notes.accountID?.raw else { return [] }
            let snapshot = session.notes.calendarState.snapshot
            let enabled = Set(snapshot.calendars.filter(\.enabled).map(\.id))
            return CalendarSchedule.events(snapshot.events, on: today, enabledCalendarIDs: enabled, knownCalendarIDs: Set(snapshot.calendars.map(\.id)))
                .filter { !$0.allDay }
                .compactMap { event in
                    guard let start = CalendarSchedule.timestamp(event.startAt), let end = CalendarSchedule.timestamp(event.endAt) else { return nil }
                    return MeetingLiveActivities.Event(account: account, id: event.id, title: event.title, start: start, end: end, color: event.color)
                }
        }
        await MeetingLiveActivities.sync(events)
    }

    /// Settings › Notifications › Upcoming meetings: a lock-screen reminder as each of today's meetings starts.
    private func scheduleReminders() async {
        let today = Date.now
        let events = accounts.sessions.filter(\.isReady).flatMap { session -> [(id: String, title: String, start: Date)] in
            let snapshot = session.notes.calendarState.snapshot
            let enabled = Set(snapshot.calendars.filter(\.enabled).map(\.id))
            return CalendarSchedule.events(snapshot.events, on: today, enabledCalendarIDs: enabled, knownCalendarIDs: Set(snapshot.calendars.map(\.id)))
                .filter { !$0.allDay }
                .compactMap { event in CalendarSchedule.timestamp(event.startAt).map { (id: "\(session.key)-\(event.id)", title: event.title, start: $0) } }
        }
        await UpcomingMeetingNotifications.schedule(events)
    }

    /// An event with the account it was read through. An event id of one account is never sent to another.
    private struct AccountEvent: Identifiable {
        let session: AccountSession
        let account: AccountID?
        let event: CalendarEventDTO
        var ref: EventRef? { account.map { EventRef(account: $0, id: event.id) } }
        var id: String { "\(session.key)/\(event.id)" }
    }

    /// Same block as the local Home: 4 pt bar, 10 pt gap, the measured events
    /// text. It reserves the empty line's height so the list does not move
    /// when calendars finish loading.
    @ViewBuilder private var upcoming: some View {
        let ready = accounts.sessions.filter(\.isReady)
        let snapshots = ready.map { $0.notes.calendarState.snapshot }
        let readForDay = calendarsReadFor == dayOffset
        let events: [AccountEvent] = !readForDay ? [] : ready.flatMap { session -> [AccountEvent] in
            let snapshot = session.notes.calendarState.snapshot
            let enabledIDs = Set(snapshot.calendars.filter(\.enabled).map(\.id))
            return CalendarSchedule.events(snapshot.events, on: selectedCalendarDay, enabledCalendarIDs: enabledIDs, knownCalendarIDs: Set(snapshot.calendars.map(\.id)))
                .map { AccountEvent(session: session, account: session.notes.accountID, event: $0) }
        }
        // Whether today had any events at all, over or not: the empty line says which.
        let hadEventsToday = dayOffset == 0 && readForDay && ready.contains { session in
            let snapshot = session.notes.calendarState.snapshot
            let enabledIDs = Set(snapshot.calendars.filter(\.enabled).map(\.id))
            return !CalendarSchedule.events(snapshot.events, on: selectedCalendarDay, now: .distantPast, enabledCalendarIDs: enabledIDs, knownCalendarIDs: Set(snapshot.calendars.map(\.id))).isEmpty
        }
        let failed = ready.contains { if case .failed = $0.notes.calendarState { true } else { false } }
        // Only answers read for this day count. Reading it again keeps what it showed;
        // a day just stepped to stays empty until its own answer, which then fades in.
        let loaded = readForDay && !ready.isEmpty && ready.allSatisfy {
            switch $0.notes.calendarState {
            case .loaded, .loading: true
            default: false
            }
        }
        let loading = ready.contains { if case .loading = $0.notes.calendarState { $0.notes.calendarState.snapshot.calendars.isEmpty } else { false } }
        VStack(alignment: .leading, spacing: 10) {
            if failed {
                HStack {
                    Text("Calendar events could not refresh.").font(.footnote).foregroundStyle(Theme.danger)
                    Button("Retry") { Task { await refreshCalendars() } }.font(.footnote)
                        .frame(minHeight: 44).contentShape(Rectangle())
                }
            }
            if let troubled = ready.first(where: { session in
                let snapshot = session.notes.calendarState.snapshot
                return snapshot.personalCalendarIssue || snapshot.calendars.contains(where: { $0.enabled && $0.lastSyncError != nil })
            }) {
                Text("A linked calendar needs attention. Its saved events may be out of date.")
                    .font(.footnote).foregroundStyle(Theme.textSecondary)
                // The one case where the user needs the connections page from here.
                if let instance = troubled.notes.instance {
                    Link("Calendar connections", destination: instance.url.appendingPathComponent("account"))
                        .font(.footnote).foregroundStyle(Theme.textSecondary)
                        .frame(minHeight: 44).contentShape(Rectangle())
                }
            }
            if !events.isEmpty {
                ForEach(events) { item in
                    let event = item.event
                    HStack(alignment: .center, spacing: 8) {
                        Button { Task { await openEvent(item) } } label: {
                            // As in the reference: the title, then "15:30 – 16:30 · 14 attendees".
                            HomeEventsBlock(barColor: calendarColor(event.color)) {
                                Text(event.title).font(.system(size: HomeMetrics.eventsText, weight: .semibold)).foregroundStyle(Theme.textPrimary)
                                    .lineLimit(1)
                                Text(Self.eventSubtitle(event)).font(.system(size: 14)).foregroundStyle(Theme.textSecondary)
                                    .lineLimit(1)
                            }.frame(maxWidth: .infinity, alignment: .leading).contentShape(Rectangle())
                        }.buttonStyle(.plain).disabled(openingEvent != nil || creating)
                            .accessibilityLabel("Open note for \(event.title), \(event.calendarName)")
                        if openingEvent != nil, openingEvent == item.ref { ProgressView() }
                    }
                }
            } else if loaded {
                if snapshots.contains(where: { $0.personalCalendarsLinked || $0.calendars.contains(where: \.enabled) }) {
                    HomeEventsEmpty(text: Self.emptyDay(today: dayOffset == 0, hadEvents: hadEventsToday))
                } else { HomeEventsEmpty(text: "Link calendars to see upcoming meetings") }
            } else if loading {
                // Holds the block's place while the first calendars load.
                HomeEventsEmpty(text: "Loading calendars…")
            }
        }
        .frame(minHeight: dayOffset == 0 ? 36.7 : HomeMetrics.otherDayEventsHeight, alignment: .topLeading)
        // A day's events arrive after its page has stepped in; they fade in rather than appear.
        .animation(motion.snappy, value: UpcomingSignature(events: events.map(\.id), loaded: loaded, loading: loading, failed: failed))
        .accessibilityElement(children: .contain).accessibilityLabel("Upcoming calendar events")
    }

    private struct UpcomingSignature: Equatable {
        let events: [String]
        let loaded: Bool
        let loading: Bool
        let failed: Bool
    }

    private func calendarColor(_ text: String) -> Color {
        let hex = text.hasPrefix("#") ? String(text.dropFirst()) : text
        return Color(hex: hex.count == 6 ? UInt32(hex, radix: 16) ?? 0x87E5E8 : 0x87E5E8)
    }

    /// The line of a day with nothing left on it, in the desktop's words: "No more" once
    /// today's events are over, "No events" when it had none, and for any other day.
    static func emptyDay(today: Bool, hadEvents: Bool) -> String {
        guard today else { return "No events" }
        return hadEvents ? "No more events today" : "No events today"
    }

    /// The line under an event's title, as the reference writes it: a 24-hour
    /// "15:30 – 16:30", then " · 14 attendees" when others are invited.
    static func eventSubtitle(_ event: CalendarEventDTO, timeZone: TimeZone = .current) -> String {
        var time = "Time unavailable"
        if event.allDay { time = "All day" }
        else if let start = CalendarSchedule.timestamp(event.startAt), let end = CalendarSchedule.timestamp(event.endAt) {
            let format = DateFormatter()
            format.locale = Locale(identifier: "en_US_POSIX"); format.timeZone = timeZone
            format.dateFormat = "HH:mm"
            time = "\(format.string(from: start)) – \(format.string(from: end))"
        }
        guard let count = event.attendeeCount, count > 1 else { return time }
        return "\(time) · \(count) attendees"
    }

    /// A note from an event belongs to the event's account, and is asked of that account alone.
    private func openEvent(_ item: AccountEvent) async {
        guard openingEvent == nil, !creating, let eventRef = item.ref else { return }
        let event = item.event
        let activeAtGesture = CalendarSchedule.isActiveTimedEvent(event)
        openingEvent = eventRef; creationError = nil
        MeetingLiveActivities.end(account: eventRef.account.raw, eventID: event.id)
        defer { openingEvent = nil }
        do {
            let created = try await item.session.notes.noteForCalendarEvent(id: event.id)
            let note = NoteRef(account: eventRef.account, id: created.id)
            if activeAtGesture && CalendarSchedule.isActiveTimedEvent(event) {
                if recording.isCapturing, recording.active != note {
                    creationError = "A recording is already active in another note."
                    router.path.append(.serverNote(note))
                } else { router.serverCapture = ServerCaptureTarget(note) }
            } else { router.path.append(.serverNote(note)) }
        } catch is CancellationError { }
        catch { creationError = error.localizedDescription }
    }

    /// A new note with no calendar event goes to the default account, and to no other by itself.
    private func createMeeting() async {
        guard !creating else { return }
        if let ref = recording.indicator {
            router.serverCapture = ServerCaptureTarget(ref)
            return
        }
        guard let session = accounts.defaultSession, let account = session.notes.accountID else { return }
        guard await MicPermission.isGranted else { requestingMic = true; return }
        creating = true; creationError = nil
        defer { creating = false }
        do {
            let note = try await session.notes.create(title: "", body: "")
            router.serverCapture = ServerCaptureTarget(NoteRef(account: account, id: note.id), createdForRecording: true)
        } catch { if let shown = ErrorText.shown(error) { creationError = shown } }
    }

    private var header: some View {
        HomeHeader(title: "Today",
                   dayTitle: dayOffset == 0 ? nil : HomeDayTitle.string(selectedCalendarDay),
                   canGoBack: dayOffset > 0,
                   back: { steppedForward = false; withAnimation(motion.snappy) { dayOffset = max(0, dayOffset - 1) } },
                   forward: { steppedForward = true; withAnimation(motion.snappy) { dayOffset += 1 } })
    }

    private func regroup() {
        sections = Signpost.interval("Home.Group") { ServerHomeGrouping.sections(rows) }
    }

    /// Drives the cross-fade between the loading, error, empty and loaded states.
    private var statusKind: [Int] {
        guard !accounts.sessions.isEmpty else { return [0] }
        return accounts.sessions.map { session in
            switch session.status {
            case .checking: return 1
            case .unreachable: return 2
            case .needsSignIn: return 5
            case .ready:
                switch session.notes.state {
                case .disconnected: return 0
                case .loading: return 1
                case .failed: return 2
                case .loaded: return session.notes.list.rows.isEmpty ? 3 : 4
                }
            }
        }
    }

    /// One block per account that has something to say, in the person's order.
    /// An account that cannot be reached, or must sign in again, never hides another's notes.
    @ViewBuilder private var status: some View {
        if accounts.sessions.isEmpty {
            if restoring {
                // A signed-in launch is restoring its session; do not flash "Connect".
                loadingRows
            } else {
                VStack(alignment: .leading, spacing: 0) {
                    Text("Connect to your instance to read and edit its notes.")
                    Button("Connection settings") { router.showConnection = true }
                        .foregroundStyle(Theme.accentText)
                        .frame(minHeight: 44).contentShape(Rectangle())
                }.foregroundStyle(Theme.textSecondary)
            }
        } else {
            if sections.isEmpty, accounts.sessions.contains(where: { session in
                if session.status == .checking { return true }
                if case .loading = session.notes.state, session.isReady { return true }
                return false
            }) { loadingRows }
            ForEach(accounts.sessions, id: \.key) { session in
                switch session.status {
                case .checking: EmptyView()
                case .unreachable(let error):
                    // With several accounts the line says whose server it is; with one it reads as it always has.
                    notice(accounts.showsAccountLabels ? "\(session.host): \(error.localizedDescription)" : error.localizedDescription,
                           action: "Retry") { Task { await accounts.retryUnreachable() } }
                case .needsSignIn:
                    notice("Sign in to \(session.host) again.", action: "Sign in") {
                        router.signInAddress = session.record.instanceURL
                        router.showConnection = true
                    }.accessibilityIdentifier("home-sign-in-again")
                case .ready:
                    if case .failed(let error, _) = session.notes.state {
                        notice(accounts.showsAccountLabels ? "\(session.host): \(error.localizedDescription)" : error.localizedDescription,
                               detail: session.notes.list.rows.isEmpty ? nil : "Showing the last loaded notes.",
                               action: "Retry") { Task { await session.notes.refresh() } }
                    }
                }
            }
            if sections.isEmpty, accounts.sessions.allSatisfy({ session in
                if case .loaded = session.notes.state, session.isReady { return true }
                return false
            }) { Text("No notes yet. Create your first note with +.").foregroundStyle(Theme.textSecondary) }
        }
    }

    private func notice(_ text: String, detail: String? = nil, action: String, perform: @escaping () -> Void) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(text)
            if let detail { Text(detail).font(.footnote).padding(.top, 10) }
            Button(action, action: perform)
                .foregroundStyle(Theme.accentText)
                .frame(minHeight: 44).contentShape(Rectangle())
        }.foregroundStyle(Theme.textSecondary).padding(.bottom, 6)
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

/// A note's row. It decides the tile itself, so a recording starting or
/// stopping redraws rows, not the whole Home.
private struct ServerHomeNoteRow: View {
    let item: ServerHomeRow
    @Environment(ServerRecordingController.self) private var recording

    var body: some View {
        if item.isDraft {
            DraftNoteRow(title: item.title, subtitle: item.subtitle)
        } else if let step = recording.isFinishingTranscript(item.ref) ? "Uploading transcript..."
                    : recording.isGeneratingNotes(item.ref) ? "Generating notes..." : nil {
            // While a stopped recording becomes notes, its row says which step it is at.
            DraftNoteRow(title: item.title, subtitle: step, working: true)
        } else {
            // A meeting shows the other party's initial.
            NoteListRow(tile: item.tile,
                        title: item.title, subtitle: item.subtitle)
        }
    }
}

/// A list row with the account it was read through.
struct AccountNoteRow: HomeListNote {
    let account: AccountID?
    let row: ServerNoteRow
    var id: String { row.id }
    var title: String { row.title }
    var date: String { row.date }
    var createdAt: String { row.createdAt }
    var homeAttendee: String? { row.firstAttendee }
    var homeAttendeeTotal: Int { row.attendeeTotal }
    var homeHasRecording: Bool { row.transcriptRevision > 0 }
    var homeAccount: AccountID? { account }
}

/// One day of notes on the server Home, with everything a row needs already computed.
struct ServerHomeSection: Identifiable, Equatable {
    let id: Date
    let title: String
    let rows: [ServerHomeRow]
}

struct ServerHomeRow: Identifiable, Equatable {
    let id: String
    /// The account the note belongs to. Nil only where there is none (previews of a list).
    var account: AccountID? = nil
    /// The note with its account: two servers can hand out the same note id.
    var ref: NoteRef? { account.map { NoteRef(account: $0, id: id) } }
    var listID: String { (account?.raw ?? "") + "/" + id }
    let title: String
    let subtitle: String
    /// A letter for a meeting with other people, a document otherwise.
    let tile: NoteTile
    /// Position in the whole list, for the entrance stagger.
    let order: Int
    /// A note made ahead of a meeting that has not happened yet. The reference
    /// lists it under the meeting's day as an outlined "Draft:" card.
    var isDraft = false
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
    /// Everyone invited, the account holder included.
    var homeAttendeeTotal: Int { get }
    /// False until something was recorded into the note.
    var homeHasRecording: Bool { get }
    /// The account the note was read through.
    var homeAccount: AccountID? { get }
}
extension HomeListNote {
    var homeAccount: AccountID? { nil }
    var homeAttendeeTotal: Int { 0 }
    var homeHasRecording: Bool { true }
}
extension ServerNoteRow: HomeListNote {
    var homeAttendee: String? { firstAttendee }
    var homeAttendeeTotal: Int { attendeeTotal }
    var homeHasRecording: Bool { transcriptRevision > 0 }
}
extension NoteDTO: HomeListNote { var homeAttendee: String? { nil } }

/// A note with its dates parsed once. (A generic function cannot declare its own type.)
private struct DatedHomeNote<Item: HomeListNote> { let note: Item; let created: Date; let timeline: Date; var draft = false }

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
        let today = calendar.startOfDay(for: now)
        for note in notes {
            let created: Date = Date.serverTimestamp(note.createdAt) ?? .distantPast
            // A note dated after today with nothing recorded yet was made ahead of its meeting:
            // as in the reference, it waits on that day as a draft. Anything recorded keeps the
            // day it was made (`MeetingPresentation.timelineDate`).
            if !note.homeHasRecording, let day = CalendarSchedule.dateOnly(note.date, calendar: calendar), day > today {
                dated.append(Dated(note: note, created: created, timeline: day, draft: true))
                continue
            }
            let timeline: Date = MeetingPresentation.timelineDate(noteDate: note.date, createdAt: note.createdAt, calendar: calendar) ?? created
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
                account: item.note.homeAccount,
                title: item.note.title.isEmpty ? "Untitled note" : item.note.title,
                subtitle: subtitle(time: item.draft ? nil : item.created.formatted(date: .omitted, time: .shortened),
                                   attendees: item.note.homeAttendeeTotal),
                tile: tile(for: item.note),
                order: index,
                isDraft: item.draft))
        }
        return order.map { day in
            ServerHomeSection(id: day, title: title(for: day, now: now, calendar: calendar), rows: groups[day] ?? [])
        }
    }

    /// "7:30 PM • 4 attendees" for a meeting with others, the time alone otherwise.
    static func subtitle(time: String?, attendees: Int) -> String {
        let people = attendees >= 2 ? "\(attendees) attendees" : nil
        return [time, people].compactMap { $0 }.joined(separator: " • ")
    }

    /// "Earlier today", "Yesterday", then the reference's "Fri 13 Nov".
    static func title(for day: Date, now: Date, calendar: Calendar) -> String {
        if calendar.isDate(day, inSameDayAs: now) { return "Earlier today" }
        if let yesterday = calendar.date(byAdding: .day, value: -1, to: now), calendar.isDate(day, inSameDayAs: yesterday) { return "Yesterday" }
        let format = DateFormatter()
        format.calendar = calendar; format.timeZone = calendar.timeZone
        // Notes from another year say so, and are never merged with this year's.
        format.dateFormat = calendar.component(.year, from: day) != calendar.component(.year, from: now) ? "EEE d MMM yyyy" : "EEE d MMM"
        return format.string(from: day)
    }
}
