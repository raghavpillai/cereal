import Foundation
import Observation
import Contracts

/// Presentation permissions only. Every action is independently authorized by the server.
/// Instance owner/admin/user roles never imply workspace owner/admin/member membership.
public struct WorkspacePermissions: Sendable {
    public let role: String
    public init(role: String) { self.role = role }
    public var canManage: Bool { role == "owner" || role == "admin" }
    public var canAssignAdmin: Bool { role == "owner" }
    public func canManageMember(role target: String) -> Bool {
        target != "owner" && (role == "owner" || role == "admin" && target == "member")
    }
}

/// Workspace selection is metadata, not an account change. It neither replaces
/// private notes nor changes the account identity to which recording is attached.
@MainActor @Observable public final class ServerWorkspacesModel {
    public private(set) var organizations: [OrganizationDTO] = []
    public private(set) var activeOrganizationID: String?
    public private(set) var invitations: [OrganizationInvitationDTO] = []
    public private(set) var details: OrganizationDetailsDTO?
    public private(set) var busy = false
    public private(set) var loaded = false
    public private(set) var error: String?
    @ObservationIgnored private var api: MeetingNotesAPI?
    @ObservationIgnored private var revision: UInt64 = 0

    public init() {}
    func attach(_ api: MeetingNotesAPI?) {
        revision &+= 1; self.api = api
        organizations = []; activeOrganizationID = nil; invitations = []; details = nil
        busy = false; loaded = false; error = nil
    }
    private func message(_ error: Error) -> String {
        switch error as? APIClientError {
        case .unauthorized: "Your session expired or your workspace role does not allow this action."
        case .notFound: "This workspace, member, or invitation is no longer available."
        case .conflict: "The workspace changed. Refresh and try again."
        default: error.localizedDescription
        }
    }
    private func perform(_ operation: (MeetingNotesAPI) async throws -> Void) async {
        guard let api, !busy else { return }
        let current = revision
        busy = true; error = nil
        defer { if revision == current { busy = false } }
        do {
            try await operation(api)
            try Task.checkCancellation()
            guard revision == current else { return }
            async let membership = api.organizations()
            async let incoming = api.organizationInvitations()
            let (list, pending) = try await (membership, incoming)
            try Task.checkCancellation()
            guard revision == current else { return }
            organizations = list.organizations; activeOrganizationID = list.activeOrganizationId
            invitations = pending.invitations; loaded = true
            if let id = details?.organization.id {
                if list.organizations.contains(where: { $0.id == id }) {
                    let updated = try await api.organization(id: id)
                    guard revision == current else { return }
                    details = updated
                } else { details = nil }
            }
        } catch is CancellationError { }
        catch {
            guard revision == current else { return }
            self.error = message(error)
        }
    }
    public func refresh() async { await perform { _ in } }
    public func open(id: String) async {
        guard let api, !busy else { return }
        let current = revision
        busy = true; error = nil; details = nil
        defer { if revision == current { busy = false } }
        do {
            let value = try await api.organization(id: id)
            try Task.checkCancellation()
            guard revision == current else { return }
            details = value
        } catch is CancellationError { }
        catch { if revision == current { self.error = message(error) } }
    }
    public func activate(id: String) async { await perform { _ = try await $0.setActiveOrganization(id: id) } }
    public func create(name: String, slug: String) async {
        await perform { _ = try await $0.createOrganization(CreateOrganizationInput(name: name, slug: slug)) }
    }
    public func rename(id: String, name: String) async {
        await perform { _ = try await $0.updateOrganization(id: id, UpdateOrganizationInput(name: name)) }
    }
    public func invite(id: String, email: String, asAdmin: Bool) async {
        await perform { _ = try await $0.inviteOrganizationMember(id: id, InviteOrganizationMemberInput(email: email, role: asAdmin ? .admin : .member)) }
    }
    public func cancelInvitation(id: String, invitationID: String) async {
        await perform { _ = try await $0.cancelOrganizationInvitation(id: id, invitationID: invitationID) }
    }
    public func changeRole(id: String, memberID: String, asAdmin: Bool) async {
        await perform { _ = try await $0.updateOrganizationMember(id: id, memberID: memberID, UpdateOrganizationMemberRoleInput(role: asAdmin ? .admin : .member)) }
    }
    public func removeMember(id: String, memberID: String) async {
        await perform { _ = try await $0.removeOrganizationMember(id: id, memberID: memberID) }
    }
    public func acceptInvitation(id: String) async { await perform { _ = try await $0.acceptOrganizationInvitation(id: id) } }
    public func rejectInvitation(id: String) async { await perform { _ = try await $0.rejectOrganizationInvitation(id: id) } }
}
