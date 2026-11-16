import SwiftUI

struct HomeView: View {
    @Environment(NoteStore.self) private var store
    @Environment(Router.self) private var router
    @State private var dayOffset = 0
    @State private var entrance = EntranceGate()
    @State private var steppedForward = true
    @State private var copyCount = 0
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
        } standalone: {
            GlassCircleButton(systemImage: "sidebar.left") { SidebarModel.shared.open() }
        } rail: {
            RailGroup {
                RailIcon(systemImage: "magnifyingglass") { router.path.append(.search) }
                RailAvatar(name: store.currentUser.name) { router.showConnection = true }
            }
        } bottom: {
            HomeBottomBar {
                AskAnythingPill(prominent: true) { router.path.append(.chat(noteID: nil)) }
            } add: {
                AccentFAB { startNewNote() }
            }
        }
        .toolbarVisibility(.visible, for: .navigationBar)
        .sensoryFeedback(.success, trigger: copyCount)
    }

    private var header: some View {
        HomeHeader(title: dayOffset == 0 ? "Today" : dayTitle, caption: "Local demo", canGoBack: dayOffset > 0,
                   back: { steppedForward = false; withAnimation(motion.snappy) { dayOffset = max(0, dayOffset - 1) } },
                   forward: { steppedForward = true; withAnimation(motion.snappy) { dayOffset += 1 } })
            .sensoryFeedback(.selection, trigger: dayOffset)
    }

    private var dayTitle: String {
        let d = Calendar.current.date(byAdding: .day, value: dayOffset, to: .now)!
        if Calendar.current.isDateInTomorrow(d) { return "Tomorrow" }
        if Calendar.current.isDateInYesterday(d) { return "Yesterday" }
        return d.formatted(.dateTime.weekday(.wide))
    }

    @ViewBuilder private var eventsBlock: some View {
        Group {
            if store.events.isEmpty {
                HomeEventsEmpty(text: dayOffset == 0 ? "No more events today" : "No events")
            } else {
                HomeEventsBlock {
                    ForEach(store.events) { e in
                        Text(e.title).font(.system(size: HomeMetrics.eventsText, weight: .medium)).foregroundStyle(Theme.textPrimary)
                        Text(e.start.formatted(date: .omitted, time: .shortened))
                            .font(.rowSubtitle).foregroundStyle(Theme.textSecondary)
                    }
                }
            }
        }
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
                            }
                        }
                        .staggeredEntrance(order[note.id] ?? 0, gate: entrance)
                        .transition(motion.fade)
                }
                Spacer().frame(height: 22)
            }
        }
    }

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
