import type { OrganizationDTO, OrganizationInvitationDTO, OrganizationMemberDTO, UserDTO } from '@meeting-notes/contracts'

export const workspaceRoleLabel = (role: OrganizationDTO['role']) => role === 'owner' ? 'Workspace owner' : role === 'admin' ? 'Workspace admin' : 'Member'
export const canCreateWorkspace = (role: UserDTO['role']) => role === 'owner'
export const canManageWorkspace = (role: OrganizationDTO['role']) => role === 'owner' || role === 'admin'
export const canChangeWorkspaceRole = (role: OrganizationDTO['role'], target: OrganizationMemberDTO) => role === 'owner' && target.role !== 'owner'
export const canRemoveWorkspaceMember = (role: OrganizationDTO['role'], target: OrganizationMemberDTO, viewerId: string) =>
  target.userId !== viewerId && target.role !== 'owner' && (role === 'owner' || role === 'admin' && target.role === 'member')

export function invitationIsPending(invitation: OrganizationInvitationDTO, now = Date.now()): boolean {
  return invitation.status === 'pending' && Number.isFinite(Date.parse(invitation.expiresAt)) && Date.parse(invitation.expiresAt) > now
}
export function canCancelWorkspaceInvitation(role: OrganizationDTO['role'], invitation: OrganizationInvitationDTO, now = Date.now()): boolean {
  return invitationIsPending(invitation, now) && (role === 'owner' || role === 'admin' && invitation.role === 'member')
}
export function workspaceProblem(name: string, slug?: string): string | null {
  if (!name.trim() || name.trim().length > 100) return 'Use a workspace name between 1 and 100 characters.'
  if (slug !== undefined && !/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(slug.trim())) return 'Use lowercase letters, numbers and single hyphens for the workspace address.'
  if (slug !== undefined && (slug.trim().length < 2 || slug.trim().length > 100)) return 'Use a workspace address between 2 and 100 characters.'
  return null
}

export type AccountSection = 'account' | 'calendars' | 'workspaces'
/** Notes have their own pages; an unknown section opens the account itself. */
export function accountSection(search: string): AccountSection {
  const params = new URLSearchParams(search), section = params.get('section')
  return params.has('google') || section === 'calendars' ? 'calendars' : section === 'workspaces' ? 'workspaces' : 'account'
}
