import SwiftUI
import APIClient
import Contracts

/// Reusable server-backed calendar context for the document and capture surfaces.
struct MeetingContextSheet: View {
    let noteID: String
    let initial: NoteContextDTO?
    let onUpdate: (NoteContextDTO) -> Void
    @Environment(ServerNotesModel.self) private var server
    @Environment(\.dismiss) private var dismiss
    @State private var context: NoteContextDTO?
    @State private var choosingEvent = false
    @State private var name = ""
    @State private var email = ""
    @State private var manualID = UUID().uuidString.lowercased()
    @State private var busy = false
    @State private var error: String?
    @Environment(\.motion) private var motion

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 18) {
                    Button { choosingEvent = true } label: {
                        HStack(alignment: .top) {
                            RoundedRectangle(cornerRadius: 3).fill(Theme.accent).frame(width: 10, height: 10).padding(.top, 6)
                            VStack(alignment: .leading, spacing: 5) {
                                Text(context?.meeting?.title ?? "Select calendar event").font(.headline).foregroundStyle(Theme.textPrimary)
                                if let meeting = context?.meeting { Text(MeetingPresentation.time(meeting)).font(.footnote).foregroundStyle(Theme.textSecondary) }
                            }
                            Spacer(); Image(systemName: "chevron.down").font(.footnote).foregroundStyle(Theme.textSecondary)
                        }.contentShape(Rectangle())
                    }.buttonStyle(.plain).disabled(busy)
                    if let meeting = context?.meeting {
                        HStack(spacing: 10) {
                            meetingLink("Join", symbol: "video", text: meeting.meetingUrl)
                            meetingLink("Calendar", symbol: "calendar", text: meeting.eventUrl)
                        }
                        Divider().overlay(Theme.divider)
                        VStack(alignment: .leading, spacing: 10) {
                            TextField("Add attendees…", text: $name).textContentType(.name).autocorrectionDisabled()
                                .accessibilityLabel("Attendee name").onSubmit { Task { await addParticipant() } }
                            TextField("Email (optional)", text: $email).textContentType(.emailAddress).keyboardType(.emailAddress)
                                .textInputAutocapitalization(.never).autocorrectionDisabled()
                            Button("Add attendee") { Task { await addParticipant() } }
                                .disabled(busy || name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || name.count > 200)
                        }.font(.system(size: 15)).foregroundStyle(Theme.textSecondary)
                        Divider().overlay(Theme.divider)
                        Text(meeting.calendarName).font(.footnote).foregroundStyle(Theme.textTertiary)
                        let people = MeetingPresentation.people(meeting)
                        if people.isEmpty { Text("No attendee details available").font(.footnote).foregroundStyle(Theme.textSecondary) }
                        ForEach(people, id: \.id) { person in
                            HStack(spacing: 12) {
                                AvatarView(name: MeetingPresentation.name(person), size: 32)
                                VStack(alignment: .leading, spacing: 3) {
                                    Text(MeetingPresentation.name(person) + (person.isSelf ? " (me)" : ""))
                                        .font(.system(size: 15)).foregroundStyle(Theme.textPrimary)
                                    if let email = person.email, email != MeetingPresentation.name(person) {
                                        Text(email).font(.caption).foregroundStyle(Theme.textSecondary)
                                    }
                                }
                                Spacer()
                                if person.isOrganizer { Text("Organizer").font(.caption2).foregroundStyle(Theme.textTertiary) }
                            }.padding(.vertical, 2)
                        }
                    } else if !busy {
                        Text("Choose a calendar event to see its time and attendees.").font(.footnote).foregroundStyle(Theme.textSecondary)
                    }
                    if busy { InlineProgress(text: "Updating meeting…").font(.footnote).transition(motion.fade) }
                    if let error { Text(error).font(.footnote).foregroundStyle(Theme.danger).transition(motion.rise) }
                }.padding(24)
                    // The event, its links and its attendee list arrive and change with a settle.
                    .animation(motion.settle, value: context?.meeting?.eventId)
                    .animation(motion.settle, value: context?.meeting.map { MeetingPresentation.people($0).count })
                    .animation(motion.snappy, value: busy)
                    .animation(motion.snappy, value: error)
            }
                .navigationTitle("Meeting").navigationBarTitleDisplayMode(.inline)
                .toolbar { ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } } }
        }
        .task(id: "\(server.accountIdentity ?? "")/\(noteID)") {
            context = initial
            do { accept(try await server.noteContext(id: noteID)) }
            catch is CancellationError { }
            catch { self.error = error.localizedDescription }
        }
        .sheet(isPresented: $choosingEvent) {
            MeetingEventPicker(noteID: noteID, selectedEventID: context?.meeting?.eventId) { updated in accept(updated) }
        }
        .presentationDetents([.medium, .large]).presentationDragIndicator(.visible)
        .glassSheet()
    }

    @ViewBuilder private func meetingLink(_ label: String, symbol: String, text: String?) -> some View {
        if let url = CalendarSchedule.meetingURL(text) {
            Link(destination: url) { Label(label, systemImage: symbol).frame(maxWidth: .infinity).padding(.vertical, 10) }
                .buttonStyle(.plain).font(.system(size: 14)).foregroundStyle(Theme.textPrimary)
                .glassSurface(Capsule(), interactive: true)
        } else {
            Label(label, systemImage: symbol).font(.system(size: 14)).foregroundStyle(Theme.textTertiary)
                .frame(maxWidth: .infinity).padding(.vertical, 10).overlay(Capsule().stroke(Theme.stroke)).opacity(0.5)
        }
    }

    private func accept(_ context: NoteContextDTO) { self.context = context; onUpdate(context); error = nil }
    private func addParticipant() async {
        guard !busy, context?.meeting != nil else { return }
        let displayName = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !displayName.isEmpty, displayName.count <= 200 else { return }
        busy = true; error = nil
        defer { busy = false }
        do {
            let email = email.trimmingCharacters(in: .whitespacesAndNewlines)
            let input = ManualParticipantInput(displayName: displayName, email: email.isEmpty ? nil : email, familyName: nil, givenName: nil, id: manualID)
            accept(try await server.addNoteParticipant(id: noteID, input: input))
            name = ""; self.email = ""; manualID = UUID().uuidString.lowercased()
            Haptics.shared.play(.done)
        } catch is CancellationError { }
        catch { self.error = error.localizedDescription; Haptics.shared.play(.error) }
    }
}

struct MeetingEventPicker: View {
    let noteID: String
    let selectedEventID: String?
    let onUpdate: (NoteContextDTO) -> Void
    @Environment(ServerNotesModel.self) private var server
    @Environment(\.dismiss) private var dismiss
    @State private var events: [CalendarEventDTO] = []
    @State private var search = ""
    @State private var loading = false
    @State private var selecting = false
    @State private var error: String?

    private var matches: [CalendarEventDTO] { events.filter { search.isEmpty || ($0.title + " " + $0.calendarName).localizedCaseInsensitiveContains(search) } }
    var body: some View {
        NavigationStack {
            List {
                if loading { InlineProgress(text: "Loading events…").listRowBackground(Theme.bgTop) }
                if let error {
                    VStack(alignment: .leading, spacing: 8) { Text(error); Button("Retry") { Task { await load() } } }.font(.footnote)
                        .listRowBackground(Theme.bgTop)
                }
                if selectedEventID != nil {
                    Button("Remove calendar event", role: .destructive) { Task { await select(nil) } }.disabled(selecting)
                        .listRowBackground(Theme.bgTop)
                }
                ForEach(matches, id: \.id) { event in
                    Button { Task { await select(event.id) } } label: {
                        HStack(spacing: 12) {
                            RoundedRectangle(cornerRadius: 2).fill(color(event.color)).frame(width: 4)
                            VStack(alignment: .leading, spacing: 4) {
                                Text(event.title).font(.system(size: 15)).foregroundStyle(Theme.textPrimary)
                                Text(eventDate(event) + " · " + event.calendarName).font(.caption).foregroundStyle(Theme.textSecondary)
                            }
                            Spacer()
                            if event.id == selectedEventID { Image(systemName: "checkmark").foregroundStyle(Theme.accent) }
                        }.fixedSize(horizontal: false, vertical: true).padding(.vertical, 4)
                    }.buttonStyle(.plain).disabled(selecting).listRowBackground(Theme.bgTop)
                }
                if !loading, error == nil, matches.isEmpty { Text("No events found in this date range").foregroundStyle(Theme.textSecondary).listRowBackground(Theme.bgTop) }
            }
            .searchable(text: $search, prompt: "Change calendar event…")
            .scrollContentBackground(.hidden)
            .navigationTitle("Calendar event").navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() }.disabled(selecting) } }
        }.task(id: server.accountIdentity) { await load() }
            .interactiveDismissDisabled(selecting).glassSheet()
    }
    private func load() async {
        loading = true; error = nil
        defer { loading = false }
        do { events = try await server.searchCalendarEvents(from: .now.addingTimeInterval(-7 * 86400), to: .now.addingTimeInterval(60 * 86400)) }
        catch is CancellationError { }
        catch { self.error = error.localizedDescription }
    }
    private func select(_ eventID: String?) async {
        guard !selecting else { return }
        selecting = true; error = nil
        defer { selecting = false }
        do {
            let context = try await server.updateNoteContext(id: noteID, eventID: eventID)
            // An event chosen for the note (or removed from it): a selection, made.
            Haptics.shared.play(eventID == nil ? .destructive : .selection)
            onUpdate(context); dismiss()
        }
        catch is CancellationError { }
        catch { self.error = error.localizedDescription; Haptics.shared.play(.error) }
    }
    private func eventDate(_ event: CalendarEventDTO) -> String {
        guard let date = event.allDay ? CalendarSchedule.dateOnly(event.startDate) : CalendarSchedule.timestamp(event.startAt) else { return "Time unavailable" }
        return date.formatted(.dateTime.weekday(.abbreviated).month(.abbreviated).day()) + (event.allDay ? " · All day" : " · " + date.formatted(date: .omitted, time: .shortened))
    }
    private func color(_ value: String) -> Color { let hex = value.hasPrefix("#") ? String(value.dropFirst()) : value; return Color(hex: hex.count == 6 ? UInt32(hex, radix: 16) ?? 0x87E5E8 : 0x87E5E8) }
}
