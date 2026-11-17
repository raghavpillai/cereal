import SwiftUI
import APIClient
import Contracts

struct WorkspacesView: View {
    @Environment(ServerNotesModel.self) private var server
    @Environment(\.motion) private var motion
    @State private var creating = false
    private var model: ServerWorkspacesModel { server.workspaces }

    var body: some View {
        List {
            Section {
                Text("Workspaces manage membership. Your notes and calendars remain private to your account; selecting a workspace does not share them.")
                    .font(.footnote).foregroundStyle(Theme.textSecondary)
                if let role = server.session?.user.role {
                    LabeledContent("Instance role", value: role.rawValue.capitalized)
                }
            }
            Section("Your workspaces") {
                if model.loaded && model.organizations.isEmpty { Text("You haven’t joined a workspace yet.").foregroundStyle(Theme.textSecondary) }
                ForEach(model.organizations, id: \.id) { organization in
                    VStack(alignment: .leading, spacing: 10) {
                        NavigationLink {
                            WorkspaceDetailView(organizationID: organization.id)
                        } label: {
                            VStack(alignment: .leading, spacing: 3) {
                                Text(organization.name)
                                Text("Workspace \(organization.role.rawValue.capitalized)").font(.caption).foregroundStyle(Theme.textSecondary)
                            }
                        }.accessibilityIdentifier("workspace-details-\(organization.id)")
                        if model.activeOrganizationID == organization.id {
                            Label("Selected workspace", systemImage: "checkmark.circle.fill")
                                .font(.caption).foregroundStyle(Theme.accentText)
                        } else {
                            Button("Switch to workspace") { Task { await model.activate(id: organization.id) } }
                                .font(.subheadline).accessibilityIdentifier("workspace-select-\(organization.id)")
                        }
                    }.padding(.vertical, 4)
                }
                if server.session?.user.role == .owner {
                    Button("Create workspace", systemImage: "plus") { creating = true }
                        .accessibilityIdentifier("workspace-create")
                }
            }
            Section("Invitations for you") {
                if model.loaded && model.invitations.isEmpty { Text("No pending invitations.").foregroundStyle(Theme.textSecondary) }
                ForEach(model.invitations, id: \.id) { invitation in
                    VStack(alignment: .leading, spacing: 8) {
                        Text(invitation.organizationName).font(.headline)
                        Text("\(invitation.email) · Workspace \(invitation.role.rawValue.capitalized)").font(.caption)
                        Text("Expires \(WorkspaceDates.label(invitation.expiresAt))").font(.caption).foregroundStyle(Theme.textSecondary)
                        HStack {
                            Button("Accept") { Task { await model.acceptInvitation(id: invitation.id) } }
                                .accessibilityIdentifier("workspace-invitation-accept-\(invitation.id)")
                            Spacer()
                            Button("Decline", role: .destructive) { Task { await model.rejectInvitation(id: invitation.id) } }
                        }.buttonStyle(.borderless)
                    }.padding(.vertical, 4)
                }
            }
            if let error = model.error { Section { Text(error).foregroundStyle(Theme.danger).accessibilityIdentifier("workspace-error") } }
        }
        .disabled(model.busy)
        .overlay { if model.busy { ProgressView().accessibilityLabel("Updating workspaces").transition(motion.fade) } }
        // Workspaces and invitations joining or leaving the lists, and the selection moving.
        .animation(motion.settle, value: model.organizations.map(\.id))
        .animation(motion.settle, value: model.invitations.map(\.id))
        .animation(motion.snappy, value: model.activeOrganizationID)
        .animation(motion.snappy, value: model.busy)
        .animation(motion.snappy, value: model.error)
        .navigationTitle("Workspaces").navigationBarTitleDisplayMode(.inline)
        .toolbar { ToolbarItem(placement: .topBarTrailing) { Button("Refresh", systemImage: "arrow.clockwise") { Task { await model.refresh() } }.disabled(model.busy) } }
        .task(id: server.accountIdentity) { await model.refresh() }
        .sheet(isPresented: $creating) { WorkspaceCreateSheet() }
        .refreshable { await model.refresh() }
    }
}

private struct WorkspaceCreateSheet: View {
    @Environment(ServerNotesModel.self) private var server
    @Environment(\.dismiss) private var dismiss
    @State private var name = ""
    @State private var slug = ""
    private var valid: Bool {
        !name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && name.count <= 100 && slug.count >= 2 && slug.count <= 100 && slug.range(of: "^[a-z0-9]+(?:-[a-z0-9]+)*$", options: .regularExpression) != nil
    }
    var body: some View {
        NavigationStack {
            Form {
                TextField("Workspace name", text: $name).accessibilityIdentifier("workspace-name")
                TextField("URL slug", text: $slug).textInputAutocapitalization(.never).autocorrectionDisabled()
                    .accessibilityIdentifier("workspace-slug")
                Text("Use lowercase letters, numbers, and single hyphens for the slug. Creating a workspace does not publish any notes.").font(.footnote)
                if let error = server.workspaces.error { Text(error).foregroundStyle(Theme.danger) }
            }.disabled(server.workspaces.busy)
            .navigationTitle("Create workspace").navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() }.disabled(server.workspaces.busy) }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Create") {
                        Task {
                            await server.workspaces.create(name: name.trimmingCharacters(in: .whitespacesAndNewlines), slug: slug)
                            if server.workspaces.error == nil { dismiss() }
                        }
                    }.disabled(!valid || server.workspaces.busy || server.session?.user.role != .owner)
                }
            }.interactiveDismissDisabled(server.workspaces.busy)
        }
    }
}

private struct WorkspaceDetailView: View {
    let organizationID: String
    @Environment(ServerNotesModel.self) private var server
    @State private var inviting = false
    @State private var renaming = false
    @State private var name = ""
    @State private var removing: OrganizationMemberDTO?
    private var model: ServerWorkspacesModel { server.workspaces }
    private var details: OrganizationDetailsDTO? {
        model.details?.organization.id == organizationID ? model.details : nil
    }
    var body: some View {
        List {
            if let details {
                let permissions = WorkspacePermissions(role: details.organization.role.rawValue)
                Section {
                    LabeledContent("Workspace role", value: details.organization.role.rawValue.capitalized)
                    LabeledContent("Slug", value: details.organization.slug)
                    if model.activeOrganizationID == organizationID { Label("Selected workspace", systemImage: "checkmark.circle.fill").foregroundStyle(Theme.accent) }
                    else { Button("Switch to workspace") { Task { await model.activate(id: organizationID) } } }
                    if permissions.canManage {
                        Button("Rename workspace") { name = details.organization.name; renaming = true }
                    }
                }
                Section("Members") {
                    ForEach(details.members, id: \.id) { member in
                        HStack {
                            VStack(alignment: .leading, spacing: 4) {
                                Text(member.name + (member.userId == server.principalID ? " (you)" : ""))
                                Text(member.email).font(.caption).foregroundStyle(Theme.textSecondary)
                                Text(member.role.rawValue.capitalized).font(.caption)
                            }
                            Spacer()
                            if permissions.canManageMember(role: member.role.rawValue) {
                                Menu {
                                    if permissions.canAssignAdmin {
                                        Button(member.role == .admin ? "Make member" : "Make admin") {
                                            Task { await model.changeRole(id: organizationID, memberID: member.id, asAdmin: member.role != .admin) }
                                        }
                                    }
                                    Button("Remove member", role: .destructive) { removing = member }
                                } label: { Image(systemName: "ellipsis.circle").frame(minWidth: 44, minHeight: 44) }
                                    .accessibilityLabel("Manage \(member.name)")
                            }
                        }
                    }
                }
                if permissions.canManage {
                    Section("Invitations") {
                        Button("Invite member", systemImage: "person.badge.plus") { inviting = true }
                        ForEach(details.invitations, id: \.id) { invitation in
                            VStack(alignment: .leading, spacing: 4) {
                                Text(invitation.email)
                                Text("\(invitation.role.rawValue.capitalized) · \(invitation.status.rawValue.capitalized)").font(.caption).foregroundStyle(Theme.textSecondary)
                                if invitation.status == .pending && permissions.canManageMember(role: invitation.role.rawValue) {
                                    Button("Cancel invitation", role: .destructive) { Task { await model.cancelInvitation(id: organizationID, invitationID: invitation.id) } }
                                        .buttonStyle(.borderless)
                                }
                            }
                        }
                    }
                }
                Section { Text("Membership does not grant access to another person’s private notes or calendars.").font(.footnote).foregroundStyle(Theme.textSecondary) }
            } else if !model.busy && model.error == nil {
                ContentUnavailableView("Workspace unavailable", systemImage: "person.3", description: Text("Refresh your memberships to continue."))
            }
            if let error = model.error { Section { Text(error).foregroundStyle(Theme.danger) } }
        }
        .disabled(model.busy)
        .overlay { if model.busy { ProgressView() } }
        .navigationTitle(details?.organization.name ?? "Workspace").navigationBarTitleDisplayMode(.inline)
        .task(id: organizationID) { await model.open(id: organizationID) }
        .sheet(isPresented: $inviting) { WorkspaceInviteSheet(organizationID: organizationID, canAssignAdmin: details?.organization.role == .owner) }
        .alert("Rename workspace", isPresented: $renaming) {
            TextField("Name", text: $name)
            Button("Save") { Task { await model.rename(id: organizationID, name: name.trimmingCharacters(in: .whitespacesAndNewlines)) } }
                .disabled(name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || name.count > 100)
            Button("Cancel", role: .cancel) { }
        }
        .confirmationDialog("Remove \(removing?.name ?? "member") from this workspace?", isPresented: Binding(get: { removing != nil }, set: { if !$0 { removing = nil } }), titleVisibility: .visible) {
            if let member = removing {
                Button("Remove member", role: .destructive) {
                    removing = nil
                    Haptics.shared.play(.destructive)
                    Task { await model.removeMember(id: organizationID, memberID: member.id) }
                }
            }
            Button("Cancel", role: .cancel) { removing = nil }
        }
    }
}

private struct WorkspaceInviteSheet: View {
    let organizationID: String
    let canAssignAdmin: Bool
    @Environment(ServerNotesModel.self) private var server
    @Environment(\.dismiss) private var dismiss
    @State private var email = ""
    @State private var asAdmin = false
    private var valid: Bool { email.count <= 254 && email.range(of: "^[^\\s@]+@[^\\s@]+\\.[^\\s@]+$", options: .regularExpression) != nil }
    var body: some View {
        NavigationStack {
            Form {
                TextField("Email", text: $email).keyboardType(.emailAddress).textContentType(.emailAddress)
                    .textInputAutocapitalization(.never).autocorrectionDisabled().accessibilityIdentifier("workspace-invite-email")
                if canAssignAdmin { Toggle("Workspace admin", isOn: $asAdmin) }
                Text("The recipient signs in to this server and accepts the invitation in Workspaces. This does not share existing notes.").font(.footnote)
                if let error = server.workspaces.error { Text(error).foregroundStyle(Theme.danger) }
            }.disabled(server.workspaces.busy)
            .navigationTitle("Invite member").navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() }.disabled(server.workspaces.busy) }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Invite") {
                        Task {
                            await server.workspaces.invite(id: organizationID, email: email.trimmingCharacters(in: .whitespacesAndNewlines), asAdmin: canAssignAdmin && asAdmin)
                            // The invitation is sent, or it was refused.
                            Haptics.shared.play(server.workspaces.error == nil ? .done : .error)
                            if server.workspaces.error == nil { dismiss() }
                        }
                    }.disabled(!valid || server.workspaces.busy)
                }
            }.interactiveDismissDisabled(server.workspaces.busy)
        }
    }
}

private enum WorkspaceDates {
    static func label(_ value: String) -> String {
        CalendarSchedule.timestamp(value)?.formatted(date: .abbreviated, time: .shortened) ?? value
    }
}
