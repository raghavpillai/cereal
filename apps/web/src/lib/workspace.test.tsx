import { describe, expect, test } from 'bun:test'
import { renderToStaticMarkup } from 'react-dom/server'
import type { AuthSessionDTO, OrganizationDTO, OrganizationDetailsDTO, OrganizationInvitationDTO, OrganizationMemberDTO } from '@meeting-notes/contracts'
import { IncomingInvitations, WorkspaceDirectory, WorkspaceMembers, Workspaces } from '../components/account/workspaces'
import { AdminClientError, createAdminClient } from './admin-client'
import { accountSection, canCancelWorkspaceInvitation, canChangeWorkspaceRole, canCreateWorkspace, canManageWorkspace, canRemoveWorkspaceMember, invitationIsPending, workspaceProblem } from './workspace-state'

const orgId = '419d65d1-7a2c-44ca-84cc-ad489522d561', userId = '419d65d1-7a2c-44ca-84cc-ad489522d562', memberId = '419d65d1-7a2c-44ca-84cc-ad489522d563', invitationId = '419d65d1-7a2c-44ca-84cc-ad489522d564'
const createdAt = '2026-11-13T12:00:00Z', expiresAt = '2030-11-13T12:00:00Z'
const organization: OrganizationDTO = { id: orgId, name: 'Cedar', slug: 'cedar', role: 'owner', createdAt }
const member: OrganizationMemberDTO = { id: memberId, organizationId: orgId, userId, name: 'Avery', email: 'avery@example.test', role: 'member', createdAt }
const invitation: OrganizationInvitationDTO = { id: invitationId, organizationId: orgId, organizationName: 'Cedar', email: 'invited@example.test', role: 'member', status: 'pending', inviterId: userId, createdAt, expiresAt }
const details: OrganizationDetailsDTO = { organization, members: [member], invitations: [invitation] }
function fetchStub(handler: (request: Request, init?: RequestInit) => Promise<Response> | Response): typeof fetch {
  return Object.assign(async (...[input, init]: Parameters<typeof fetch>) => handler(new Request(input, init), init), { preconnect: () => {} }) as typeof fetch
}

describe('workspace permissions are separate from instance roles', () => {
  test('only the instance owner creates a workspace; all membership roles can read', () => {
    expect(canCreateWorkspace('owner')).toBe(true)
    expect(canCreateWorkspace('admin')).toBe(false)
    expect(canCreateWorkspace('user')).toBe(false)
    expect(canManageWorkspace('owner')).toBe(true)
    expect(canManageWorkspace('admin')).toBe(true)
    expect(canManageWorkspace('member')).toBe(false)
  })
  test('an organization admin cannot mint admins or remove peers/owners; self-removal is not offered', () => {
    for (const role of ['owner', 'admin', 'member'] as const) {
      const target = { ...member, role }
      expect(canChangeWorkspaceRole('owner', target)).toBe(role !== 'owner')
      expect(canChangeWorkspaceRole('admin', target)).toBe(false)
      expect(canChangeWorkspaceRole('member', target)).toBe(false)
      expect(canRemoveWorkspaceMember('owner', target, 'someone-else')).toBe(role !== 'owner')
      expect(canRemoveWorkspaceMember('admin', target, 'someone-else')).toBe(role === 'member')
      expect(canRemoveWorkspaceMember('member', target, 'someone-else')).toBe(false)
      expect(canRemoveWorkspaceMember('owner', target, userId)).toBe(false)
    }
  })
  test('only live pending invitations expose actions and admins cannot cancel admin invitations', () => {
    const now = Date.parse(createdAt)
    expect(invitationIsPending(invitation, now)).toBe(true)
    expect(invitationIsPending(invitation, Date.parse(expiresAt))).toBe(false)
    for (const status of ['accepted', 'rejected', 'canceled', 'expired'] as const) expect(invitationIsPending({ ...invitation, status }, now)).toBe(false)
    expect(canCancelWorkspaceInvitation('owner', { ...invitation, role: 'admin' }, now)).toBe(true)
    expect(canCancelWorkspaceInvitation('admin', invitation, now)).toBe(true)
    expect(canCancelWorkspaceInvitation('admin', { ...invitation, role: 'admin' }, now)).toBe(false)
    expect(canCancelWorkspaceInvitation('member', invitation, now)).toBe(false)
  })
  test('workspace address validation and account deep links preserve the Google callback', () => {
    expect(workspaceProblem(' Cedar ', 'cedar-2')).toBeNull()
    for (const slug of ['', 'x', 'Cedar', 'cedar--2', '-cedar', 'cedar-', 'a/b', 'a'.repeat(101)]) expect(workspaceProblem('Cedar', slug)).not.toBeNull()
    expect(workspaceProblem('   ')).not.toBeNull()
    expect(accountSection('?section=workspaces')).toBe('workspaces')
    expect(accountSection('?section=workspaces&google=connected')).toBe('calendars')
    expect(accountSection('?section=calendars')).toBe('calendars')
    expect(accountSection('?section=unknown')).toBe('account')
    expect(accountSection('')).toBe('account')
  })
})

describe('workspace presentation', () => {
  const noop = () => {}
  test('selected and active are different states, with safe text and disabled mutations while loading', () => {
    const markup = renderToStaticMarkup(<WorkspaceDirectory organizations={[{ ...organization, name: '<script>not html</script>' }]} activeId={null} selectedId={orgId} busy open={noop} activate={noop} />)
    expect(markup).toContain('is-selected')
    expect(markup).toContain('aria-expanded="true"')
    expect(markup).toContain('&lt;script&gt;')
    expect(markup).toContain('Make active')
    expect(markup).toContain('disabled=""')
    const active = renderToStaticMarkup(<WorkspaceDirectory organizations={[organization]} activeId={orgId} selectedId={null} busy={false} open={noop} activate={noop} />)
    expect(active).toContain('Active workspace')
    expect(active).not.toContain('Make active')
  })
  test('member view has no management controls, admin can remove only ordinary members', () => {
    const members: OrganizationMemberDTO[] = [member, { ...member, id: 'owner', role: 'owner' }, { ...member, id: 'admin', role: 'admin' }]
    const view = (role: OrganizationDTO['role']) => renderToStaticMarkup(<WorkspaceMembers details={{ ...details, organization: { ...organization, role }, members }} viewerId="other-user" busy={false} change={noop} />)
    expect(view('member')).not.toContain('<button')
    const admin = view('admin')
    expect((admin.match(/<button/g) ?? []).length).toBe(1)
    expect(admin).not.toContain('Make workspace admin')
    expect(admin).not.toContain('Revoke workspace admin')
    const owner = view('owner')
    expect(owner).toContain('Make workspace admin')
    expect(owner).toContain('Revoke workspace admin')
    expect((owner.match(/<button/g) ?? []).length).toBe(4)
  })
  test('expired invitations remain informative with no accept or decline action', () => {
    const markup = renderToStaticMarkup(<IncomingInvitations invitations={[{ ...invitation, expiresAt: createdAt }]} busy={false} respond={noop} />)
    expect(markup).toContain('Expired')
    expect(markup).not.toContain('<button')
    const pending = renderToStaticMarkup(<IncomingInvitations invitations={[invitation]} busy respond={noop} />)
    expect(pending).toContain('Accept')
    expect(pending).toContain('Decline')
    expect(pending).toContain('disabled=""')
  })
  test('the initial shell exposes no membership data and creation follows the instance role', () => {
    const session = { user: { id: userId, email: member.email, name: member.name, role: 'user', createdAt }, expiresAt } as AuthSessionDTO
    const props = { client: createAdminClient('https://instance.example'), expired: noop }
    const user = renderToStaticMarkup(<Workspaces {...props} session={session} />)
    expect(user).toContain('Loading your workspaces')
    expect(user).not.toContain('Cedar')
    expect(user).not.toContain('Create workspace')
    expect(renderToStaticMarkup(<Workspaces {...props} session={{ ...session, user: { ...session.user, role: 'owner' } }} />)).toContain('Create workspace')
  })
})

describe('canonical workspace HTTP client', () => {
  test('all workspace operations use Effect schemas, cookie transport and canonical facade paths', async () => {
    const received: { url: string; method: string; body?: unknown }[] = []
    const organizations = { organizations: [organization], activeOrganizationId: orgId }
    const client = createAdminClient('https://instance.example', fetchStub(async (request, init) => {
      expect(request.headers.get('Authorization')).toBeNull()
      expect(init?.credentials).toBe('same-origin')
      expect(init?.cache).toBe('no-store')
      expect(init?.redirect).toBe('error')
      const path = new URL(request.url).pathname
      received.push({ url: path, method: request.method, ...(request.body ? { body: await request.json() } : {}) })
      const value = request.method === 'DELETE' || path.endsWith('/reject') ? { deleted: true }
        : path.endsWith('/organization-invitations') ? { invitations: [invitation] }
        : path.endsWith('/accept') || request.method === 'PATCH' && path === `/api/v1/organizations/${orgId}` || request.method === 'POST' && path.endsWith('/organizations') ? organization
        : path.endsWith(`/members/${memberId}`) ? member
        : path.endsWith('/invitations') ? invitation
        : path === `/api/v1/organizations/${orgId}` ? details : organizations
      return Response.json({ ...value, privateInternalField: 'must-be-stripped' })
    }))
    expect(await client.listOrganizations()).toEqual(organizations)
    expect(await client.createOrganization({ name: 'Cedar', slug: 'cedar' })).toEqual(organization)
    expect(await client.getOrganization(orgId)).toEqual(details)
    await client.updateOrganization(orgId, { name: 'Renamed' })
    await client.setActiveOrganization(orgId)
    await client.inviteOrganizationMember(orgId, { email: member.email, role: 'member' })
    await client.cancelOrganizationInvitation(orgId, invitationId)
    await client.updateOrganizationMemberRole(orgId, memberId, { role: 'admin' })
    await client.removeOrganizationMember(orgId, memberId)
    await client.listOrganizationInvitations()
    await client.acceptOrganizationInvitation(invitationId)
    await client.rejectOrganizationInvitation(invitationId)
    expect(received.map(request => `${request.method} ${request.url}`)).toEqual([
      'GET /api/v1/organizations', 'POST /api/v1/organizations', `GET /api/v1/organizations/${orgId}`, `PATCH /api/v1/organizations/${orgId}`,
      'POST /api/v1/organizations/active', `POST /api/v1/organizations/${orgId}/invitations`, `DELETE /api/v1/organizations/${orgId}/invitations/${invitationId}`,
      `PATCH /api/v1/organizations/${orgId}/members/${memberId}`, `DELETE /api/v1/organizations/${orgId}/members/${memberId}`, 'GET /api/v1/organization-invitations',
      `POST /api/v1/organization-invitations/${invitationId}/accept`, `POST /api/v1/organization-invitations/${invitationId}/reject`,
    ])
    expect(received[4]?.body).toEqual({ organizationId: orgId })
    expect(received[7]?.body).toEqual({ role: 'admin' })
  })
  test('malformed membership responses and authorization failures cannot expose upstream text', async () => {
    const malformed = createAdminClient('https://instance.example', fetchStub(() => Response.json({ ...details, members: [{ ...member, role: 'instance-admin' }] })))
    await expect(malformed.getOrganization(orgId)).rejects.toBeInstanceOf(AdminClientError)
    const forbidden = createAdminClient('https://instance.example', fetchStub(() => Response.json({ error: 'forbidden', message: 'private upstream credential' }, { status: 403 })))
    const error = await forbidden.getOrganization(orgId).catch(error => error)
    expect(error.status).toBe(403)
    expect(error.message).not.toContain('upstream')
  })
  test('workspace request cancellation reaches the underlying fetch', async () => {
    const controller = new AbortController()
    let reached!: () => void, aborted = false
    const started = new Promise<void>(resolve => { reached = resolve })
    const client = createAdminClient('https://instance.example', fetchStub(request => new Promise((_resolve, reject) => {
      request.signal.addEventListener('abort', () => { aborted = true; reject(new DOMException('Canceled', 'AbortError')) }, { once: true }); reached()
    })))
    const response = client.getOrganization(orgId, controller.signal).catch(() => null)
    await started; controller.abort(); await response
    expect(aborted).toBe(true)
  })
})
