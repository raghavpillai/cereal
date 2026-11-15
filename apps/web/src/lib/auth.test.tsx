import { describe, expect, test } from 'bun:test'
import { renderToStaticMarkup } from 'react-dom/server'
import type { AuthSessionDTO, UserDTO } from '@meeting-notes/contracts'
import { AdministratorsPanel, AdministratorsList, AddAdministrator } from '../components/admin/administrators-panel'
import { UserDirectory } from '../components/admin/users-panel'
import { Login } from '../components/account/session-gate'
import { createAdminClient, AdminClientError } from './admin-client'
import { createSessionAuthClient, signInProblem } from './auth-client'

const owner: UserDTO = { id: '7a9d273c-a15e-458d-8824-2fbe54586002', name: 'Owner', email: 'owner@example.test', role: 'owner', createdAt: '2026-11-12T12:00:00Z' }
const admin: UserDTO = { ...owner, id: '85cc40bb-b204-46b7-9242-244b9abe5780', name: 'Admin', email: 'admin@example.test', role: 'admin' }
const user: UserDTO = { ...owner, id: 'd42fb942-030c-425f-a248-efb38e3bd7d9', name: 'User', email: 'user@example.test', role: 'user' }
function transport(handler: (request: Request, init?: RequestInit) => Response): typeof fetch {
  return Object.assign(async (...[input, init]: Parameters<typeof fetch>) => handler(new Request(input, init), init), { preconnect: () => {} }) as typeof fetch
}

describe('individual accounts and roles', () => {
  test('login uses ordinary account credentials and has no bootstrap-token or public sign-up field', () => {
    const markup = renderToStaticMarkup(<Login connected={async () => {}} />)
    expect(markup).toContain('type="email"')
    expect(markup).toContain('autoComplete="current-password"')
    expect(markup).toContain('Sign in')
    expect(markup).not.toContain('access token')
    expect(markup).not.toContain('Sign up')
  })
  test('owner and admin see the same directory; only the owner sees role changes and owner has no mutation control', () => {
    const users = [owner, admin, user]
    const markup = (viewer: UserDTO) => renderToStaticMarkup(<UserDirectory users={users} viewer={viewer} pending={new Set()} change={() => {}} />)
    const owned = markup(owner), administered = markup(admin)
    for (const person of users) { expect(owned).toContain(person.email); expect(administered).toContain(person.email) }
    expect(owned).toContain('Make admin')
    expect(owned).toContain('Revoke admin')
    expect(owned.match(/<button/g)).toHaveLength(2)
    expect(administered).not.toContain('<button')
    expect(markup(user)).not.toContain('<button')
  })
  test('instance administrator controls are owner-only and never offer mutation of the owner', () => {
    const client = createAdminClient('https://instance.example', transport(() => Response.json({users:[owner,admin,user]})))
    for (const viewer of [admin,user]) expect(renderToStaticMarkup(<AdministratorsPanel viewer={viewer} client={client} expired={() => {}} />)).toBe('')
    expect(renderToStaticMarkup(<AdministratorsPanel viewer={owner} client={client} expired={() => {}} />)).toContain('Add admin')
    const list=renderToStaticMarkup(<AdministratorsList users={[owner,admin,user]} viewer={owner} busy={false} revoke={() => {}} />)
    expect(list).toContain(owner.email);expect(list).toContain(admin.email);expect(list).not.toContain(user.email)
    expect(list.match(/<button/g)).toHaveLength(1)
    expect(list).toContain(`Revoke admin access for ${admin.email}`)
    expect(list).not.toContain(`Revoke admin access for ${owner.email}`)
  })
  test('adding an admin selects an existing eligible user without an account-creation form', () => {
    const render=(users:UserDTO[])=>renderToStaticMarkup(<AddAdministrator users={users} busy={false} error={null} close={()=>{}} promote={async()=>{}} refresh={async()=>{}}/>)
    const markup=render([owner,admin,user])
    expect(markup).toContain('<select');expect(markup).toContain('User to promote')
    expect(markup).toContain(`${user.name} · ${user.email}`)
    expect(markup).not.toContain(owner.email);expect(markup).not.toContain(admin.email)
    expect(markup).toContain('Grant admin');expect(markup).not.toContain('New account')
    expect(markup).not.toContain('type="password"');expect(markup).not.toContain('type="email"')
    expect(render([owner,admin])).toContain('No eligible users')
  })
  test('admin creation uses the authenticated owner endpoint and returns no credentials', async () => {
    const client=createAdminClient('https://instance.example', transport(request => {
      expect(request.url).toBe('https://instance.example/api/v1/admin/administrators')
      expect(request.method).toBe('POST');expect(request.headers.get('Authorization')).toBeNull()
      return Response.json({...admin,password:'never-returned-by-server'})
    }))
    expect(await client.createAdministrator({name:'Admin',email:admin.email,password:'test-admin-password'})).toEqual(admin)
  })
  test('the canonical session decoder accepts only explicit roles and strips unknown auth data', async () => {
    const session: AuthSessionDTO = { user, expiresAt: '2026-11-13T12:00:00Z' }
    const accepted = createAdminClient('https://instance.example', transport(request => {
      expect(request.url).toBe('https://instance.example/api/v1/session')
      return Response.json({ ...session, token: 'must-not-enter-app-state', user: { ...user, passwordHash: 'must-not-enter-app-state' } })
    }))
    expect(await accepted.getSession()).toEqual(session)
    const rejected = createAdminClient('https://instance.example', transport(() => Response.json({ ...session, user: { ...user, role: 'owner,admin' } })))
    await expect(rejected.getSession()).rejects.toBeInstanceOf(AdminClientError)
  })
  test('Better Auth sign-in uses its real cookie endpoint and sign-out without bootstrap headers', async () => {
    const calls: { url: string; method: string; body: unknown; credentials?: RequestCredentials; authorization: string | null }[] = []
    const client = createSessionAuthClient('https://instance.example', transport((request, init) => {
      calls.push({ url: request.url, method: request.method, body: typeof init?.body === 'string' ? JSON.parse(init.body) : null, credentials: init?.credentials, authorization: request.headers.get('authorization') })
      return Response.json(request.url.endsWith('/sign-out') ? { success: true } : { user, token: 'server-owned-test-session', redirect: false })
    }))
    const result = await client.signIn.email({ email: 'user@example.test', password: 'fixture-password-value', rememberMe: false })
    expect(result.error).toBeNull()
    await client.signOut()
    expect(calls[0]).toEqual({ url: 'https://instance.example/api/auth/sign-in/email', method: 'POST', body: { email: 'user@example.test', password: 'fixture-password-value', rememberMe: false }, credentials: 'same-origin', authorization: null })
    expect(calls[1]!.url).toBe('https://instance.example/api/auth/sign-out')
    expect(calls[1]!.method).toBe('POST')
    expect(calls[1]!.authorization).toBeNull()
  })
  test('authentication failures expose bounded messages without raw server data', () => {
    expect(signInProblem(401)).toBe('The email or password was not accepted.')
    expect(signInProblem(429)).toContain('Too many attempts')
    expect(signInProblem(503)).toContain('temporarily unavailable')
  })
})
