import SwiftUI

struct HomeView: View {
    @Environment(NoteStore.self) private var store
    @Environment(Router.self) private var router
    @State private var dayOffset = 0
    @State private var entrance = EntranceGate()
    @State private var steppedForward = true
    @State private var copyCount = 0
    @State private var trashCount = 0
    @Environment(\.motion) private var motion

    var body: some View {
        RailScaffold {
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    header
                    if let error = store.persistenceError {
                        Text(error).font(.footnote).foregroundStyle(Theme.danger).padding(.bottom, 20)
                    }
                    if let recovery = store.captureRecoveryMessage {
                        Label(recovery, systemImage: "waveform")
                            .font(.footnote)
                            .foregroundStyle(Theme.textSecondary)
                            .padding(12)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .background(Theme.surface, in: RoundedRectangle(cornerRadius: 12))
                            .overlay(RoundedRectangle(cornerRadius: 12).stroke(Theme.stroke))
                            .padding(.bottom, 20)
                            .transition(motion.rise)
                    }
                    eventsBlock
                    HomeDivider()
                    noteGroups
                }
                .padding(.leading, Theme.gutter)
                .padding(.trailing, 8)
                .clearsBottomChrome()
                // Notes arriving or leaving the list (a new recording, trash) settle rather than cut.
                .animation(motion.settle, value: store.activeNotes.map(\.id))
                .animation(motion.snappy, value: store.captureRecoveryMessage)
                .animation(motion.snappy, value: store.persistenceError)
            }
            .scrollIndicators(.hidden)
        } leading: {
            // As in the reference: the notes library leads the bar on a phone and heads the
            // rail on a Duo; phone calls, search and the account trail it in one capsule.
            HomeNavigation.Notes()
        } rail: {
            HomeNavigation.Group(name: store.currentUser.name)
        } bottom: {
            HomeBottomBar {
                AskAnythingPill(prominent: true) { router.path.append(.chat(noteID: nil)) }
            } add: {
                AccentFAB { startNewNote() }
            }
        }
        .toolbarVisibility(.visible, for: .navigationBar)
        #if DEBUG
        // QA only: `-qa.dayOffset 1` opens Home on another day, for screenshots of the day pager.
        .onAppear {
            if let offset = Int(UserDefaults.standard.string(forKey: "qa.dayOffset") ?? "") { dayOffset = offset }
            // `-qa.demoEvent 1` puts one meeting on tomorrow's calendar.
            if UserDefaults.standard.bool(forKey: "qa.demoEvent"), store.events.isEmpty,
               let day = Calendar.current.date(byAdding: .day, value: 1, to: .now),
               let start = Calendar.current.date(bySettingHour: 15, minute: 30, second: 0, of: day) {
                store.events = [CalendarEvent(title: "Design review", start: start, end: start.addingTimeInterval(3600))]
            }
        }
        #endif
        .haptic(.done, trigger: copyCount)
        .haptic(.destructive, trigger: trashCount)
    }

    private var header: some View {
        HomeHeader(title: "Today", caption: dayOffset == 0 ? "Local demo" : nil,
                   dayTitle: dayOffset == 0 ? nil : HomeDayTitle.string(Calendar.current.date(byAdding: .day, value: dayOffset, to: .now) ?? .now),
                   canGoBack: dayOffset > 0,
                   back: { steppedForward = false; withAnimation(motion.snappy) { dayOffset = max(0, dayOffset - 1) } },
                   forward: { steppedForward = true; withAnimation(motion.snappy) { dayOffset += 1 } })
            .haptic(.selection, trigger: dayOffset)
    }

    /// The demo's events on the day shown.
    private var dayEvents: [CalendarEvent] {
        let day = Calendar.current.date(byAdding: .day, value: dayOffset, to: .now) ?? .now
        return store.events.filter { Calendar.current.isDate($0.start, inSameDayAs: day) }.sorted { $0.start < $1.start }
    }

    @ViewBuilder private var eventsBlock: some View {
        Group {
            if dayEvents.isEmpty {
                HomeEventsEmpty(text: dayOffset == 0 ? "No more events today" : "No events")
            } else {
                // One block per event, as on the server Home: the title, then "15:30 – 16:30".
                VStack(alignment: .leading, spacing: 10) {
                    ForEach(dayEvents) { e in
                        HomeEventsBlock(barColor: Color(hex: 0x87E5E8)) {
                            Text(e.title).font(.system(size: HomeMetrics.eventsText, weight: .semibold)).foregroundStyle(Theme.textPrimary)
                            Text("\(Self.clock.string(from: e.start)) – \(Self.clock.string(from: e.end))")
                                .font(.system(size: 14)).foregroundStyle(Theme.textSecondary)
                        }
                    }
                }
            }
        }
        .frame(minHeight: dayOffset == 0 ? 0 : HomeMetrics.otherDayEventsHeight, alignment: .topLeading)
        // The day's block arrives from the side the user stepped towards.
        .id(dayOffset)
        .transition(motion.step(forward: steppedForward))
    }

    private var noteGroups: some View {
        let groups = store.groupedNotes
        let order = Dictionary(groups.flatMap(\.notes).enumerated().map { ($0.element.id, $0.offset) }, uniquingKeysWith: { first, _ in first })
        return LazyVStack(alignment: .leading, spacing: 0) {
            ForEach(groups, id: \.title) { group in
                HomeSectionHeader(title: group.title)
                    .staggeredEntrance(group.notes.first.flatMap { order[$0.id] } ?? 0, gate: entrance)
                ForEach(group.notes) { note in
                    Button { router.path.append(.note(note.id)) } label: { NoteRow(note: note) }
                        .buttonStyle(RowPressStyle(isOrigin: router.origin == .note(note.id)))
                        .contextMenu {
                            Button("Copy notes", systemImage: "doc.on.doc") { UIPasteboard.general.string = note.plainText; copyCount += 1 }
                            Button("Move to trash", systemImage: "trash", role: .destructive) {
                                withAnimation(motion.exit(.settle)) { store.trash(note.id) }
                                trashCount += 1
                            }
                        }
                        .staggeredEntrance(order[note.id] ?? 0, gate: entrance)
                        .transition(motion.fade)
                }
                Spacer().frame(height: 22)
            }
        }
    }

    private static let clock: DateFormatter = {
        let format = DateFormatter()
        format.locale = Locale(identifier: "en_US_POSIX"); format.dateFormat = "HH:mm"
        return format
    }()

    private func startNewNote() {
        Task {
            if await MicPermission.isGranted {
                router.newNoteID = store.createNote().id
            } else {
                router.showMicSheet = true
            }
        }
    }
}

struct NoteRow: View {
    let note: Note
    @Environment(NoteStore.self) private var store

    var body: some View {
        NoteListRow(tile: note.tile(currentUser: store.currentUser), title: note.displayTitle, subtitle: note.rowSubtitle)
    }
}

extension Note {
    /// A meeting shows the other party's initial; a note recorded here shows a
    /// phone; anything else is a document.
    func tile(currentUser: Person) -> NoteTile {
        if attendees.count >= 2, let other = attendees.first(where: { $0.name != currentUser.name }) ?? attendees.first {
            return .meeting(with: other.name)
        }
        return hasCapture ? .phone : .document
    }

    var rowSubtitle: String {
        let time = createdAt.formatted(date: .omitted, time: .shortened)
        return attendees.count >= 2 ? "\(time) • \(attendees.count) attendees" : time
    }
}
