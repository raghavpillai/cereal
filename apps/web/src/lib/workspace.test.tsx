import { describe, expect, test } from 'bun:test'
import type { OrganizationDTO, OrganizationDetailsDTO, OrganizationInvitationDTO, OrganizationMemberDTO } from '@meeting-notes/contracts'
import { AdminClientError, createAdminClient } from './admin-client'
// Workspaces are managed in the apps; the web keeps only the transport they share with it.

const orgId = '419d65d1-7a2c-44ca-84cc-ad489522d561', userId = '419d65d1-7a2c-44ca-84cc-ad489522d562', memberId = '419d65d1-7a2c-44ca-84cc-ad489522d563', invitationId = '419d65d1-7a2c-44ca-84cc-ad489522d564'
const createdAt = '2026-11-13T12:00:00Z', expiresAt = '2030-11-13T12:00:00Z'
const organization: OrganizationDTO = { id: orgId, name: 'Cedar', slug: 'cedar', role: 'owner', createdAt }
const member: OrganizationMemberDTO = { id: memberId, organizationId: orgId, userId, name: 'Avery', email: 'avery@example.test', role: 'member', createdAt }
const invitation: OrganizationInvitationDTO = { id: invitationId, organizationId: orgId, organizationName: 'Cedar', email: 'invited@example.test', role: 'member', status: 'pending', inviterId: userId, createdAt, expiresAt }
const details: OrganizationDetailsDTO = { organization, members: [member], invitations: [invitation] }
function fetchStub(handler: (request: Request, init?: RequestInit) => Promise<Response> | Response): typeof fetch {
  return Object.assign(async (...[input, init]: Parameters<typeof fetch>) => handler(new Request(input, init), init), { preconnect: () => {} }) as typeof fetch
}

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
