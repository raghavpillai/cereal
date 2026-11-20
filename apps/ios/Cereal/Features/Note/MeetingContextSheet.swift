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

    private var canAdd: Bool { let typed = name.trimmingCharacters(in: .whitespacesAndNewlines); return !typed.isEmpty && name.count <= 200 }

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 18) {
                    // The event this note belongs to. The whole row opens the picker and answers a touch as a row does.
                    RowButton { choosingEvent = true } label: {
                        HStack(alignment: .top) {
                            RoundedRectangle(cornerRadius: 3).fill(Theme.accent).frame(width: 10, height: 10).padding(.top, 6)
                            VStack(alignment: .leading, spacing: 5) {
                                Text(context?.meeting?.title ?? "Select calendar event").uiFont(17, weight: .semibold, relativeTo: .headline).foregroundStyle(Theme.textPrimary)
                                    .multilineTextAlignment(.leading)
                                if let meeting = context?.meeting { Text(MeetingPresentation.time(meeting)).uiFont(13, relativeTo: .footnote).foregroundStyle(Theme.textSecondary) }
                            }
                            Spacer(); Image(systemName: "chevron.down").font(.footnote).foregroundStyle(Theme.textSecondary).padding(.top, 4)
                        }
                        .padding(.vertical, 8).contentShape(Rectangle())
                    }
                    .padding(.vertical, -8)
                    .disabled(busy)
                    .accessibilityIdentifier("meeting-event")
                    if let meeting = context?.meeting {
                        HStack(spacing: 10) {
                            meetingLink("Join", symbol: "video", text: meeting.meetingUrl)
                            meetingLink("Calendar", symbol: "calendar", text: meeting.eventUrl)
                        }
                        VStack(alignment: .leading, spacing: 0) {
                            Text(meeting.calendarName).uiFont(13, relativeTo: .footnote).foregroundStyle(Theme.textTertiary).padding(.bottom, 8)
                            let people = MeetingPresentation.people(meeting)
                            if people.isEmpty { Text("No attendee details available").uiFont(13, relativeTo: .footnote).foregroundStyle(Theme.textSecondary) }
                            ForEach(people, id: \.id) { person in
                                HStack(spacing: 12) {
                                    AvatarView(name: MeetingPresentation.name(person), size: 32)
                                    VStack(alignment: .leading, spacing: 3) {
                                        Text(MeetingPresentation.name(person) + (person.isSelf ? " (me)" : ""))
                                            .uiFont(15).foregroundStyle(Theme.textPrimary)
                                        if let email = person.email, email != MeetingPresentation.name(person) {
                                            Text(email).uiFont(12, relativeTo: .caption).foregroundStyle(Theme.textSecondary)
                                        }
                                    }
                                    Spacer()
                                    if person.isOrganizer { Text("Organizer").uiFont(11, relativeTo: .caption2).foregroundStyle(Theme.textTertiary) }
                                }
                                .padding(.vertical, 7)
                                .transition(motion.rise)
                            }
                        }
                        // Someone who was there and is not on the invitation: fields that look like fields,
                        // and a button that is plainly off until there is a name to add.
                        VStack(alignment: .leading, spacing: 0) {
                            SettingsCard {
                                CardField {
                                    TextField("Name", text: $name, prompt: Text("Add an attendee").foregroundStyle(Theme.textTertiary))
                                        .textContentType(.name).autocorrectionDisabled()
                                        .submitLabel(.done).onSubmit { Task { await addParticipant() } }
                                        .accessibilityLabel("Attendee name").accessibilityIdentifier("meeting-attendee-name")
                                }
                                CardField(divider: false) {
                                    TextField("Email", text: $email, prompt: Text("Email (optional)").foregroundStyle(Theme.textTertiary))
                                        .textContentType(.emailAddress).keyboardType(.emailAddress)
                                        .textInputAutocapitalization(.never).autocorrectionDisabled()
                                        .submitLabel(.done).onSubmit { Task { await addParticipant() } }
                                        .accessibilityLabel("Attendee email").accessibilityIdentifier("meeting-attendee-email")
                                }
                            }
                            if let error { CardNote(error, color: Theme.danger).transition(motion.rise) }
                            PrimaryActionButton(title: "Add attendee", busy: busy) { Task { await addParticipant() } }
                                .disabled(!canAdd)
                                .padding(.top, 12)
                                .accessibilityIdentifier("meeting-attendee-add")
                        }
                        .padding(.top, 4)
                    } else {
                        Text("Choose a calendar event to see its time and attendees.").uiFont(13, relativeTo: .footnote).foregroundStyle(Theme.textSecondary)
                        if let error { Text(error).uiFont(13, relativeTo: .footnote).foregroundStyle(Theme.danger).transition(motion.rise) }
                    }
                }.padding(.horizontal, 20).padding(.top, 12).padding(.bottom, 24)
                    // The event, its links and its attendee list arrive and change with a settle.
                    .animates(context?.meeting?.eventId, .settle)
                    .animates(context?.meeting.map { MeetingPresentation.people($0).map(\.id) })
                    .animates(error)
            }
                .scrollDismissesKeyboard(.interactively)
                .background(Theme.sheet.ignoresSafeArea())
                .dynamicTypeSize(...DynamicTypeSize.accessibility1)
                .navigationTitle("Meeting").navigationBarTitleDisplayMode(.inline)
                .sheetBar()
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
        .glassSheet(background: Theme.sheet)
    }

    @ViewBuilder private func meetingLink(_ label: String, symbol: String, text: String?) -> some View {
        if let url = CalendarSchedule.meetingURL(text) {
            Link(destination: url) { Label(label, systemImage: symbol).frame(maxWidth: .infinity).padding(.vertical, 10).contentShape(Capsule()) }
                .buttonStyle(.squash).uiFont(14).foregroundStyle(Theme.textPrimary)
                .glassSurface(Capsule(), interactive: true)
        } else {
            Label(label, systemImage: symbol).uiFont(14).foregroundStyle(Theme.textTertiary)
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
    @State private var loading = true
    @State private var selecting = false
    @State private var error: String?

    private var matches: [CalendarEventDTO] { events.filter { search.isEmpty || ($0.title + " " + $0.calendarName).localizedCaseInsensitiveContains(search) } }
    var body: some View {
        NavigationStack {
            // Nothing says "no events" until the calendar has answered (`Loading`).
            Loading(while: loading && events.isEmpty, alignment: .top) {
                VStack(alignment: .leading, spacing: 26) {
                    ForEach(0..<4, id: \.self) { index in
                        VStack(alignment: .leading, spacing: 8) {
                            RoundedRectangle(cornerRadius: 5, style: .continuous).fill(Theme.quietFill).frame(width: [210, 160, 240, 180][index], height: 14)
                            RoundedRectangle(cornerRadius: 5, style: .continuous).fill(Theme.quietFill).frame(width: 130, height: 11)
                        }
                    }
                }
                .padding(.horizontal, 20).padding(.top, 20)
                .frame(maxWidth: .infinity, alignment: .leading)
                .accessibilityElement().accessibilityLabel("Loading events")
            } then: {
                List {
                    if let error {
                        VStack(alignment: .leading, spacing: 0) {
                            Text(error).foregroundStyle(Theme.textSecondary)
                            Button(Wording.retry) { Task { await load() } }
                                .fontWeight(.medium).foregroundStyle(Theme.accentText)
                                .frame(minHeight: 44).contentShape(Rectangle()).buttonStyle(.squash)
                        }
                        .uiFont(14)
                        .listRowBackground(Color.clear).listRowSeparator(.hidden)
                    }
                    if selectedEventID != nil {
                        Button { Task { await select(nil) } } label: {
                            Label("Remove calendar event", systemImage: "calendar.badge.minus").uiFont(15).foregroundStyle(Theme.danger)
                                .frame(maxWidth: .infinity, minHeight: 44, alignment: .leading).contentShape(Rectangle())
                        }
                        .buttonStyle(.row).disabled(selecting)
                        .listRowBackground(Color.clear).listRowSeparator(.hidden)
                    }
                    ForEach(matches, id: \.id) { event in
                        Button { Task { await select(event.id) } } label: {
                            HStack(spacing: 12) {
                                RoundedRectangle(cornerRadius: 2).fill(color(event.color)).frame(width: 4)
                                VStack(alignment: .leading, spacing: 4) {
                                    Text(event.title).uiFont(15).foregroundStyle(Theme.textPrimary)
                                    Text(eventDate(event) + Wording.dot + event.calendarName).uiFont(12, relativeTo: .caption).foregroundStyle(Theme.textSecondary)
                                }
                                Spacer()
                                if event.id == selectedEventID { Image(systemName: "checkmark").foregroundStyle(Theme.accentText) }
                            }.fixedSize(horizontal: false, vertical: true).padding(.vertical, 8).contentShape(Rectangle())
                        }.buttonStyle(.row).disabled(selecting)
                            .listRowBackground(Color.clear).listRowSeparator(.hidden)
                            .accessibilityIdentifier("meeting-event-choice")
                    }
                }
                .listStyle(.plain)
                .overlay {
                    if error == nil, matches.isEmpty {
                        ScreenPrompt(symbol: "calendar", title: search.isEmpty ? "No events in the next two months" : "No events match", bottomRoom: 40)
                    }
                }
                .animates(matches.isEmpty)
            }
            .searchable(text: $search, prompt: "Search events")
            .scrollContentBackground(.hidden)
            .background(Theme.sheet.ignoresSafeArea())
            .dynamicTypeSize(...DynamicTypeSize.accessibility1)
            .navigationTitle("Calendar event").navigationBarTitleDisplayMode(.inline)
            .sheetBar()
            .toolbar { ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() }.disabled(selecting) } }
        }.task(id: server.accountIdentity) { await load() }
            .interactiveDismissDisabled(selecting).glassSheet(background: Theme.sheet)
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
