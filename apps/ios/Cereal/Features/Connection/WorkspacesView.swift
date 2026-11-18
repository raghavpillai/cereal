import SwiftUI
import APIClient
import Contracts

/// The column of a page pushed inside the account sheet: the Settings cards,
/// 16 pt from the sheet's edges, on the sheet's own material.
private struct SheetPage<Content: View>: View {
    @ViewBuilder var content: Content
    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 0) { content }
                .padding(.horizontal, 16).padding(.top, 4).padding(.bottom, 32)
        }
        .scrollIndicators(.hidden)
        // A navigation stack paints the system's white behind its pages: the warm page goes over it.
        .background(Theme.sheet.ignoresSafeArea())
        .dynamicTypeSize(...DynamicTypeSize.accessibility1)
    }
}

/// A wait that belongs to the whole page (a workspace being switched, a member
/// removed): the Cereal mark in the bar, where the page's title is, so nothing
/// in the page moves or greys for it.
private struct BusyMark: ToolbarContent {
    let busy: Bool
    let label: String
    var body: some ToolbarContent {
        ToolbarItem(placement: .topBarTrailing) {
            CerealLoader(size: 20, color: Theme.textSecondary, running: busy)
                .opacity(busy ? 1 : 0)
                .accessibilityLabel(label).accessibilityHidden(!busy)
        }
        // The mark alone: no glass button around something that is not one.
        .sharedBackgroundVisibility(.hidden)
    }
}

struct WorkspacesView: View {
    @Environment(ServerNotesModel.self) private var server
    @Environment(\.account) private var account
    @Environment(\.motion) private var motion
    @State private var creating = false
    @State private var opened: String?
    /// What was just done, for the hand: a workspace switched to, an invitation accepted or declined.
    @State private var switches = 0
    @State private var joins = 0
    @State private var declines = 0
    private var model: ServerWorkspacesModel { server.workspaces }

    var body: some View {
        SheetPage {
            CardNote("Workspaces manage membership. Your notes and calendars remain private to your account; selecting a workspace does not share them.")
            SettingsSection("Your workspaces") {
                // Nothing is said about having no workspaces until the server has answered.
                Loading(while: !model.loaded && model.error == nil) {
                    VStack(spacing: 0) {
                        ForEach(0..<2, id: \.self) { _ in
                            SettingsRowLabel("Workspace name", systemImage: "person.2", trailing: .none)
                        }
                    }
                    .redacted(reason: .placeholder)
                    .accessibilityElement(children: .ignore).accessibilityLabel("Loading workspaces")
                } then: {
                    VStack(alignment: .leading, spacing: 0) {
                        if model.organizations.isEmpty {
                            Text("You haven’t joined a workspace yet.").uiFont(15).foregroundStyle(Theme.textSecondary)
                                .padding(.horizontal, 18).frame(minHeight: SettingsMetrics.rowHeight)
                        }
                        ForEach(model.organizations, id: \.id) { organization in
                            workspaceRow(organization).transition(motion.rise)
                        }
                        if server.session?.user.role == .owner {
                            SettingsRow("Create workspace", systemImage: "plus", trailing: .none) { creating = true }
                                .accessibilityIdentifier("workspace-create")
                        }
                    }
                }
            }
            if model.loaded, !model.invitations.isEmpty {
                SettingsSection("Invitations for you") {
                    ForEach(model.invitations, id: \.id) { invitation in
                        VStack(alignment: .leading, spacing: 2) {
                            Text(invitation.organizationName).uiFont(SettingsMetrics.label, weight: .medium).foregroundStyle(Theme.textPrimary)
                            Text(invitation.email + Wording.dot + invitation.role.rawValue.capitalized).uiFont(14).foregroundStyle(Theme.textSecondary)
                            Text("Expires \(WorkspaceDates.label(invitation.expiresAt))").uiFont(14).foregroundStyle(Theme.textTertiary)
                            HStack(spacing: 24) {
                                Button("Accept") { Task { await model.acceptInvitation(id: invitation.id); if model.error == nil { joins += 1 } } }
                                    .foregroundStyle(Theme.accentText)
                                    .frame(minWidth: 44, minHeight: 44).contentShape(Rectangle())
                                    .accessibilityIdentifier("workspace-invitation-accept-\(invitation.id)")
                                Button("Decline") { Task { await model.rejectInvitation(id: invitation.id); if model.error == nil { declines += 1 } } }
                                    .foregroundStyle(Theme.danger)
                                    .frame(minWidth: 44, minHeight: 44).contentShape(Rectangle())
                            }
                            .uiFont(16, weight: .medium).buttonStyle(.squash)
                        }
                        .padding(.horizontal, 18).padding(.top, 8)
                        .transition(motion.rise)
                    }
                }
                .transition(motion.rise)
            }
            if let error = model.error {
                CardNote(error, color: Theme.danger).accessibilityIdentifier("workspace-error").transition(motion.rise)
            }
        }
        // A wait blocks touches without greying the page.
        .allowsHitTesting(!model.busy)
        // Workspaces and invitations joining or leaving the lists, and the selection moving.
        .animates(model.organizations.map(\.id), .settle)
        .animates(model.invitations.map(\.id), .settle)
        .animates(model.activeOrganizationID)
        .animates(model.error)
        .animates(model.busy)
        .navigationTitle("Workspaces").navigationBarTitleDisplayMode(.inline)
        .toolbar { BusyMark(busy: model.busy, label: "Updating workspaces") }
        .task(id: server.accountIdentity) { await model.refresh() }
        .sheet(isPresented: $creating) { WorkspaceCreateSheet().glassSheet(background: Theme.sheet) }
        .refreshable { await model.refresh() }
        .navigationDestination(item: $opened) { id in
            // A pushed page takes its environment from the navigation stack, not from this view.
            // Without the account's model handed on here, opening a workspace ended the app.
            WorkspaceDetailView(organizationID: id).environment(server).environment(\.account, account)
        }
        .haptic(.selection, trigger: switches)
        .haptic(.done, trigger: joins)
        .haptic(.destructive, trigger: declines)
        .haptic(.error, trigger: model.error) { old, new in new != nil && new != old }
    }

    /// The row opens the workspace. Whether it is the one in use, and the way to
    /// make it so, are a separate control at its trailing edge: one tap, one thing.
    private func workspaceRow(_ organization: OrganizationDTO) -> some View {
        let selected = model.activeOrganizationID == organization.id
        return HStack(spacing: 0) {
            RowButton { opened = organization.id } label: {
                HStack(spacing: 14) {
                    SettingsIconTile(systemImage: "person.2")
                    VStack(alignment: .leading, spacing: 2) {
                        Text(organization.name).uiFont(SettingsMetrics.label).foregroundStyle(Theme.textPrimary).lineLimit(1)
                        Text(organization.role.rawValue.capitalized).uiFont(14).foregroundStyle(Theme.textSecondary)
                    }
                    Spacer(minLength: 8)
                }
                .padding(.leading, 11)
                .frame(minHeight: 60).contentShape(Rectangle())
            }
            .accessibilityIdentifier("workspace-details-\(organization.id)")
            .accessibilityHint("Opens the workspace")
            WorkspaceSelection(selected: selected) { Task { await select(organization.id) } }
                .accessibilityIdentifier("workspace-select-\(organization.id)")
            Image(systemName: "chevron.right").font(.system(size: 13, weight: .medium)).foregroundStyle(Theme.textSecondary)
                .padding(.leading, 10).padding(.trailing, 18)
                .accessibilityHidden(true)
        }
    }

    private func select(_ id: String) async {
        await model.activate(id: id)
        if model.error == nil { switches += 1 }
    }
}

/// "In use" for the selected workspace, "Use" for any other. The two trade
/// places in one spot, so the label that leaves is gone before the other arrives.
private struct WorkspaceSelection: View {
    let selected: Bool
    let select: () -> Void

    var body: some View {
        Replaced(id: selected, alignment: .trailing) {
            if selected {
                Label("In use", systemImage: "checkmark.circle.fill")
                    .uiFont(14, weight: .medium).foregroundStyle(Theme.accentText)
                    .labelStyle(.titleAndIcon)
                    .frame(minHeight: 44)
                    .accessibilityLabel("Selected workspace")
            } else {
                Button(action: select) {
                    Text("Use").uiFont(14, weight: .medium).foregroundStyle(Theme.textPrimary)
                        .padding(.horizontal, 14).frame(height: 32)
                        .overlay(Capsule().strokeBorder(Theme.ink.opacity(0.14), lineWidth: 1))
                        .frame(minWidth: 44, minHeight: 44).contentShape(Rectangle())
                }
                .buttonStyle(.squash)
                .accessibilityLabel("Switch to workspace")
            }
        }
    }
}

private struct WorkspaceCreateSheet: View {
    @Environment(ServerNotesModel.self) private var server
    @Environment(\.dismiss) private var dismiss
    @Environment(\.motion) private var motion
    @State private var name = ""
    @State private var slug = ""
    /// Until the slug is edited by hand it follows the name.
    @State private var slugEdited = false
    @FocusState private var focus: Field?
    enum Field { case name, slug }
    private var valid: Bool {
        !name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && name.count <= 100 && slug.count >= 2 && slug.count <= 100 && slug.range(of: "^[a-z0-9]+(?:-[a-z0-9]+)*$", options: .regularExpression) != nil
    }

    var body: some View {
        NavigationStack {
            SheetPage {
                SettingsSection("Workspace") {
                    CardField {
                        // A name: capitalised word by word, and never "corrected".
                        TextField("Workspace name", text: $name, prompt: Text("Name").foregroundStyle(Theme.textTertiary))
                            .textInputAutocapitalization(.words).autocorrectionDisabled()
                            .submitLabel(.next).focused($focus, equals: .name).onSubmit { focus = .slug }
                            .accessibilityLabel("Workspace name")
                            .accessibilityIdentifier("workspace-name")
                    }
                    CardField(divider: false) {
                        TextField("URL slug", text: Binding(get: { slug }, set: { slug = $0; slugEdited = true }),
                                  prompt: Text("url-slug").foregroundStyle(Theme.textTertiary))
                            .textInputAutocapitalization(.never).autocorrectionDisabled().keyboardType(.asciiCapable)
                            .submitLabel(.done).focused($focus, equals: .slug).onSubmit { create() }
                            .accessibilityLabel("URL slug")
                            .accessibilityIdentifier("workspace-slug")
                    }
                }
                CardNote("The slug uses lowercase letters, numbers and single hyphens. Creating a workspace does not publish any notes.")
                if let error = server.workspaces.error { CardNote(error, color: Theme.danger).transition(motion.rise) }
                PrimaryActionButton(title: "Create workspace", busy: server.workspaces.busy, action: create)
                    .disabled(!valid || server.session?.user.role != .owner)
                    .padding(.top, 18)
            }
            .animates(server.workspaces.error)
            .allowsHitTesting(!server.workspaces.busy)
            .navigationTitle("Create workspace").navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() }.disabled(server.workspaces.busy) } }
            .interactiveDismissDisabled(server.workspaces.busy)
            .onChange(of: name) { _, name in if !slugEdited { slug = WorkspaceSlug.suggested(from: name) } }
            .onAppear { focus = .name }
        }
    }

    private func create() {
        guard valid, !server.workspaces.busy else { return }
        Task {
            await server.workspaces.create(name: name.trimmingCharacters(in: .whitespacesAndNewlines), slug: slug)
            // The workspace exists, or it was refused.
            Haptics.shared.play(server.workspaces.error == nil ? .done : .error)
            if server.workspaces.error == nil { dismiss() }
        }
    }
}

/// The slug a name suggests: its letters and digits in lower case, joined by single hyphens.
enum WorkspaceSlug {
    static func suggested(from name: String) -> String {
        let folded = name.folding(options: [.diacriticInsensitive, .caseInsensitive], locale: nil).lowercased()
        let words = folded.split(whereSeparator: { !($0.isASCII && ($0.isLetter || $0.isNumber)) })
        return String(words.joined(separator: "-").prefix(100))
    }
}

struct WorkspaceDetailView: View {
    let organizationID: String
    @Environment(ServerNotesModel.self) private var server
    @Environment(\.motion) private var motion
    @State private var inviting = false
    @State private var renaming = false
    @State private var name = ""
    @State private var removing: OrganizationMemberDTO?
    @State private var switches = 0
    @State private var saves = 0
    private var model: ServerWorkspacesModel { server.workspaces }
    private var details: OrganizationDetailsDTO? {
        model.details?.organization.id == organizationID ? model.details : nil
    }
    /// The name while the page loads, from the list it was opened from.
    private var listedName: String? { model.organizations.first { $0.id == organizationID }?.name }

    var body: some View {
        SheetPage {
            // Nothing says "unavailable" until the server has said so.
            Loading(while: details == nil && model.busy) {
                VStack(alignment: .leading, spacing: 0) {
                    SettingsSection("Workspace") {
                        SettingsRowLabel("Workspace role", systemImage: "person", trailing: .none)
                        SettingsRowLabel("Workspace slug", systemImage: "link", trailing: .none)
                    }
                    SettingsSection("Members") {
                        ForEach(0..<3, id: \.self) { _ in SettingsRowLabel("Member name here", systemImage: "person", trailing: .none) }
                    }
                }
                .redacted(reason: .placeholder)
                .accessibilityElement(children: .ignore).accessibilityLabel("Loading workspace")
            } then: {
                VStack(alignment: .leading, spacing: 0) {
                    if let details { loaded(details) }
                    else if model.error == nil {
                        ScreenPrompt(symbol: "person.3", title: "Workspace unavailable", detail: "It may have been removed, or you may no longer be a member.",
                                     actions: [.init(title: Wording.retry) { Task { await model.open(id: organizationID) } }], bottomRoom: 0, fills: false)
                            .padding(.top, 60)
                    }
                    if let error = model.error {
                        CardNote(error, color: Theme.danger).accessibilityIdentifier("workspace-detail-error").transition(motion.rise)
                        if details == nil {
                            Button(Wording.retry) { Task { await model.open(id: organizationID) } }
                                .uiFont(15, weight: .medium).foregroundStyle(Theme.accentText)
                                .frame(minHeight: 44).contentShape(Rectangle()).buttonStyle(.squash)
                                .padding(.horizontal, 14)
                        }
                    }
                }
            }
        }
        .allowsHitTesting(!model.busy)
        // Members and invitations joining and leaving, a role changing, the selection moving.
        .animates(details?.members.map(\.id), .settle)
        .animates(details?.invitations.map { "\($0.id)/\($0.status.rawValue)" }, .settle)
        .animates(details?.members.map(\.role.rawValue))
        .animates(model.activeOrganizationID)
        .animates(model.error)
        .navigationTitle(details?.organization.name ?? listedName ?? "Workspace").navigationBarTitleDisplayMode(.inline)
        .toolbar { BusyMark(busy: model.busy && details != nil, label: "Updating workspace") }
        .task(id: organizationID) { await model.open(id: organizationID) }
        // Reads the memberships and this workspace again, keeping what is on screen meanwhile.
        .refreshable { await model.refresh() }
        .sheet(isPresented: $inviting) {
            WorkspaceInviteSheet(organizationID: organizationID, canAssignAdmin: details?.organization.role == .owner).glassSheet(background: Theme.sheet)
        }
        .alert("Rename workspace", isPresented: $renaming) {
            TextField("Name", text: $name).textInputAutocapitalization(.words).autocorrectionDisabled()
            Button("Save") {
                Task {
                    await model.rename(id: organizationID, name: name.trimmingCharacters(in: .whitespacesAndNewlines))
                    if model.error == nil { saves += 1 }
                }
            }
            .disabled(name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || name.count > 100)
            Button("Cancel", role: .cancel) { }
        }
        .haptic(.selection, trigger: switches)
        .haptic(.done, trigger: saves)
        .haptic(.error, trigger: model.error) { old, new in new != nil && new != old }
    }

    @ViewBuilder private func loaded(_ details: OrganizationDetailsDTO) -> some View {
        let permissions = WorkspacePermissions(role: details.organization.role.rawValue)
        let selected = model.activeOrganizationID == organizationID
        SettingsSection("Workspace") {
            SettingsRowLabel("Your role", systemImage: "person", trailing: .text(details.organization.role.rawValue.capitalized))
                .accessibilityElement(children: .combine)
            SettingsRowLabel("Slug", systemImage: "link", trailing: .text(details.organization.slug))
                .accessibilityElement(children: .combine)
            HStack(spacing: 14) {
                SettingsIconTile(systemImage: "checkmark.circle")
                Text("Selected workspace").uiFont(SettingsMetrics.label).foregroundStyle(Theme.textPrimary)
                Spacer(minLength: 8)
                WorkspaceSelection(selected: selected) {
                    Task { await model.activate(id: organizationID); if model.error == nil { switches += 1 } }
                }
                .accessibilityIdentifier("workspace-detail-select")
            }
            .padding(.leading, 11).padding(.trailing, 18)
            .frame(minHeight: SettingsMetrics.rowHeight)
            if permissions.canManage {
                SettingsRow("Rename workspace", systemImage: "pencil", trailing: .none) { name = details.organization.name; renaming = true }
                    .accessibilityIdentifier("workspace-rename")
            }
        }
        SettingsSection("Members") {
            ForEach(details.members, id: \.id) { member in
                HStack(spacing: 14) {
                    AvatarView(name: member.name, size: SettingsMetrics.iconTile)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(member.name + (member.userId == server.principalID ? " (you)" : ""))
                            .uiFont(SettingsMetrics.label).foregroundStyle(Theme.textPrimary).lineLimit(1)
                        Text(member.email).uiFont(14).foregroundStyle(Theme.textSecondary).lineLimit(1)
                    }
                    Spacer(minLength: 8)
                    if permissions.canManageMember(role: member.role.rawValue) {
                        // The menu opens from the role, which is its label; the row stays.
                        ValueMenu(value: member.role.rawValue.capitalized, size: 15) {
                            if permissions.canAssignAdmin {
                                Button(member.role == .admin ? "Make member" : "Make admin", systemImage: member.role == .admin ? "person" : "person.badge.key") {
                                    Task {
                                        await model.changeRole(id: organizationID, memberID: member.id, asAdmin: member.role != .admin)
                                        if model.error == nil { saves += 1 }
                                    }
                                }
                            }
                            Button("Remove member", systemImage: "person.badge.minus", role: .destructive) { removing = member }
                        }
                        .fixedSize()
                        .accessibilityLabel("Manage \(member.name)")
                        .accessibilityIdentifier("workspace-member-menu-\(member.id)")
                        // Asked where the choice was made: the question grows from this member's
                        // menu, not from the top of the page.
                        .confirmationDialog("Remove \(member.name) from this workspace?",
                                            isPresented: Binding(get: { removing?.id == member.id }, set: { if !$0 { removing = nil } }),
                                            titleVisibility: .visible) {
                            Button("Remove member", role: .destructive) {
                                removing = nil
                                Haptics.shared.play(.destructive)
                                Task { await model.removeMember(id: organizationID, memberID: member.id) }
                            }
                            Button("Cancel", role: .cancel) { removing = nil }
                        }
                    } else {
                        Text(member.role.rawValue.capitalized).uiFont(15).foregroundStyle(Theme.textSecondary)
                    }
                }
                .padding(.leading, 11).padding(.trailing, 18)
                .frame(minHeight: 60)
                .transition(motion.rise)
            }
        }
        if permissions.canManage {
            SettingsSection("Invitations") {
                SettingsRow("Invite member", systemImage: "person.badge.plus", trailing: .none) { inviting = true }
                    .accessibilityIdentifier("workspace-invite")
                // Invitations still waiting for an answer. One that was accepted is a member above.
                ForEach(details.invitations.filter { $0.status == .pending }, id: \.id) { invitation in
                    HStack(spacing: 14) {
                        SettingsIconTile(systemImage: "envelope")
                        VStack(alignment: .leading, spacing: 2) {
                            Text(invitation.email).uiFont(SettingsMetrics.label).foregroundStyle(Theme.textPrimary).lineLimit(1)
                            Text(invitation.role.rawValue.capitalized + Wording.dot + invitation.status.rawValue.capitalized)
                                .uiFont(14).foregroundStyle(Theme.textSecondary)
                        }
                        Spacer(minLength: 8)
                        if permissions.canManageMember(role: invitation.role.rawValue) {
                            Button("Cancel") {
                                Haptics.shared.play(.destructive)
                                Task { await model.cancelInvitation(id: organizationID, invitationID: invitation.id) }
                            }
                            .uiFont(15, weight: .medium).foregroundStyle(Theme.danger)
                            .frame(minWidth: 44, minHeight: 44).contentShape(Rectangle()).buttonStyle(.squash)
                            .accessibilityLabel("Cancel invitation to \(invitation.email)")
                        }
                    }
                    .padding(.leading, 11).padding(.trailing, 18)
                    .frame(minHeight: 60)
                    .transition(motion.rise)
                }
            }
        }
        CardNote("Membership does not grant access to another person’s private notes or calendars.")
    }
}

private struct WorkspaceInviteSheet: View {
    let organizationID: String
    let canAssignAdmin: Bool
    @Environment(ServerNotesModel.self) private var server
    @Environment(\.dismiss) private var dismiss
    @Environment(\.motion) private var motion
    @State private var email = ""
    @State private var asAdmin = false
    @FocusState private var focused: Bool
    private var valid: Bool { email.count <= 254 && email.range(of: "^[^\\s@]+@[^\\s@]+\\.[^\\s@]+$", options: .regularExpression) != nil }

    var body: some View {
        NavigationStack {
            SheetPage {
                SettingsSection("Invite by email") {
                    CardField(divider: canAssignAdmin) {
                        TextField("Email", text: $email, prompt: Text("name@example.com").foregroundStyle(Theme.textTertiary))
                            .accessibilityLabel("Email")
                            .keyboardType(.emailAddress).textContentType(.emailAddress)
                            .textInputAutocapitalization(.never).autocorrectionDisabled()
                            .submitLabel(.send).focused($focused).onSubmit { invite() }
                            .accessibilityIdentifier("workspace-invite-email")
                    }
                    if canAssignAdmin {
                        SettingsToggleRow("Workspace admin", systemImage: "person.badge.key", isOn: $asAdmin)
                    }
                }
                CardNote("No email is sent. The person signs in to this server and accepts the invitation in Workspaces. This does not share existing notes.")
                if let error = server.workspaces.error { CardNote(error, color: Theme.danger).transition(motion.rise) }
                PrimaryActionButton(title: "Invite", busy: server.workspaces.busy, action: invite)
                    .disabled(!valid)
                    .padding(.top, 18)
            }
            .animates(server.workspaces.error)
            .allowsHitTesting(!server.workspaces.busy)
            .navigationTitle("Invite member").navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() }.disabled(server.workspaces.busy) } }
            .interactiveDismissDisabled(server.workspaces.busy)
            .haptic(.selection, trigger: asAdmin)
            .onAppear { focused = true }
        }
    }

    private func invite() {
        guard valid, !server.workspaces.busy else { return }
        Task {
            await server.workspaces.invite(id: organizationID, email: email.trimmingCharacters(in: .whitespacesAndNewlines), asAdmin: canAssignAdmin && asAdmin)
            // The invitation is made, or it was refused.
            Haptics.shared.play(server.workspaces.error == nil ? .done : .error)
            if server.workspaces.error == nil { dismiss() }
        }
    }
}

private enum WorkspaceDates {
    static func label(_ value: String) -> String {
        CalendarSchedule.timestamp(value)?.formatted(date: .abbreviated, time: .shortened) ?? value
    }
}
