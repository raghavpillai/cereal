import { describe, expect, test } from 'bun:test'
import { renderToStaticMarkup } from 'react-dom/server'
import { ConnectionInstructions, ConnectionRow } from '../components/mcp/connect'
import { ConsentDetails, InvalidAuthorization, MCPConsent } from '../components/mcp/consent'
import { signInPath } from '../components/account/session-gate'
import { createMCPClient, type MCPAuthorization } from './mcp-client'
import { AdminClientError } from './admin-client'
import { mcpConnectionURL, safeConsentCallback, safeOAuthResume, signedOAuthQuery } from './mcp-state'

const preview: MCPAuthorization = { clientId: 'fictional-client', clientName: 'Cedar Assistant', redirectUri: 'http://localhost:8787/callback?app=cedar', scopes: ['notes:read', 'transcripts:read', 'account:write', 'offline_access'], resource: 'https://instance.example/mcp', workspace: { id: 'workspace-1', name: 'Cedar' }, globalWorkspaceSwitch: true }
const signed = new URLSearchParams([['client_id', 'cedar'], ['scope', 'notes:read'], ['exp', '123'], ['ba_param', 'client_id'], ['ba_param', 'scope'], ['ba_param', 'exp'], ['ba_param', 'ba_param'], ['sig', 'server-signature']]).toString()
const stub = (handler: (request: Request, init?: RequestInit) => Response | Promise<Response>): typeof fetch => Object.assign(async (...[input, init]: Parameters<typeof fetch>) => handler(new Request(input, init), init), { preconnect: () => {} }) as typeof fetch

describe('MCP authorization boundaries', () => {
  test('server address always uses this instance origin and requires HTTPS outside loopback', () => {
    expect(mcpConnectionURL('https://instance.example')).toBe('https://instance.example/api/mcp')
    expect(mcpConnectionURL('http://localhost:3000')).toBe('http://localhost:3000/api/mcp')
    for (const value of ['http://public.example', 'https://user:password@instance.example', 'https://instance.example/path', 'https://instance.example?url=elsewhere', 'javascript:alert(1)']) expect(mcpConnectionURL(value)).toBeNull()
  })
  test('signed query extraction preserves provider signed keys and ignores unsigned identity/returnTo', () => {
    expect(signedOAuthQuery(`?${signed}&client_name=Trusted+App&returnTo=https://other.example`)).toBe(signed)
    expect(signedOAuthQuery('?client_id=cedar&scope=notes:read')).toBeNull()
    expect(signedOAuthQuery(`?${signed}&sig=another`)).toBeNull()
    expect(signedOAuthQuery('?sig=x')).toBeNull()
  })
  test('pre-consent login resumes only our exact same-origin authorization paths', () => {
    expect(safeOAuthResume('/mcp/consent?sig=x', 'https://instance.example')).toBe('https://instance.example/mcp/consent?sig=x')
    expect(safeOAuthResume('/api/auth/oauth2/authorize?client_id=x', 'https://instance.example')).toContain('/api/auth/oauth2/authorize')
    for (const path of ['https://attacker.example/mcp/consent', '//attacker.example/mcp/consent', '/mcp/consent#unexpected', '/mcp/consent/extra', '/admin', 'javascript:alert(1)', '/mcp/consent\\evil', '/mcp/%63onsent']) expect(safeOAuthResume(path, 'https://instance.example')).toBeNull()
  })
  test('explicit consent callback must match registered protocol, host, port, path and fixed query', () => {
    const approved = 'http://localhost:8787/callback?app=cedar&code=one-time&state=csrf'
    expect(safeConsentCallback(approved, preview.redirectUri)).toBe(approved)
    expect(safeConsentCallback('cedar-app://oauth/callback?code=x', 'cedar-app://oauth/callback')).toBe('cedar-app://oauth/callback?code=x')
    expect(safeConsentCallback('http://localhost:8787/callback?app=cedar&error=access_denied', preview.redirectUri)).toContain('access_denied')
    for (const returned of ['http://localhost:8788/callback?app=cedar', 'http://localhost:8787/elsewhere?app=cedar', 'http://localhost:8787/callback?app=changed', 'http://localhost:8787/callback?app=cedar&app=attacker', 'https://attacker.example/callback?app=cedar', 'javascript:alert(1)', 'data:text/html,test', 'file:///callback']) expect(safeConsentCallback(returned, preview.redirectUri)).toBeNull()
  })
})

describe('MCP cookie transport', () => {
  test('preview, deny, list and revoke use real routes without bearer credentials', async () => {
    const calls: { path: string; method: string; body: unknown }[] = []
    const client = createMCPClient('https://instance.example', stub(async (request, init) => {
      const url = new URL(request.url)
      expect(request.headers.get('authorization')).toBeNull()
      expect(init?.credentials).toBe('same-origin'); expect(init?.redirect).toBe('error'); expect(init?.cache).toBe('no-store')
      calls.push({ path: url.pathname, method: request.method, body: request.method === 'POST' ? await request.json() : null })
      if (url.pathname.endsWith('authorize-info')) { expect(url.searchParams.get('oauth_query')).toBe(signed); return Response.json(preview) }
      if (url.pathname.endsWith('/consent')) return Response.json({ redirect: true, url: 'http://localhost:8787/callback?app=cedar&error=access_denied' })
      if (url.pathname.endsWith('/continue')) return Response.json({ redirect: true, url: `https://instance.example/mcp/consent?${signed}` })
      if (url.pathname.endsWith('/revoke')) return Response.json({ success: true })
      return Response.json({ connections: [] })
    }))
    expect(await client.authorization(signed)).toEqual(preview)
    await client.consent(signed, false)
    expect(await client.connections()).toEqual({ connections: [] })
    expect(await client.revoke('grant/one')).toEqual({ success: true })
    expect(calls[1]).toEqual({ path: '/api/auth/oauth2/consent', method: 'POST', body: { accept: false, oauth_query: signed } })
    expect(calls[3]).toEqual({ path: '/api/auth/mcp/connections/grant%2Fone/revoke', method: 'POST', body: {} })
    // After sign-in on the sign-in page, the request carries on from the connect page with the query it arrived with.
    const next = await client.resume(signed)
    expect(calls[4]).toEqual({ path: '/api/auth/oauth2/continue', method: 'POST', body: { postLogin: true, oauth_query: signed } })
    expect(safeOAuthResume(next.url, 'https://instance.example')).toBe(`https://instance.example/mcp/consent?${signed}`)
  })
  test('expired session and malformed identity never enter authorization UI', async () => {
    const expired = createMCPClient('https://instance.example', stub(() => Response.json({ error: 'private server detail' }, { status: 401 })))
    await expect(expired.connections()).rejects.toMatchObject({ status: 401 })
    const malformed = createMCPClient('https://instance.example', stub(() => Response.json({ ...preview, scopes: 'notes:write' })))
    await expect(malformed.authorization(signed)).rejects.toBeInstanceOf(AdminClientError)
    expect(() => createMCPClient('https://instance.example/untrusted')).toThrow()
  })
  test('revocation errors do not report successful disconnect', async () => {
    const client = createMCPClient('https://instance.example', stub(() => Response.json({}, { status: 503 })))
    await expect(client.revoke('grant-1')).rejects.toMatchObject({ status: 503 })
  })
})

describe('MCP page states', () => {
  test('public instructions contain actual self-hosted address and standard OAuth command', () => {
    const html = renderToStaticMarkup(<ConnectionInstructions url="https://instance.example/api/mcp" />)
    expect(html).toContain('claude mcp add --transport http meeting-notes https://instance.example/api/mcp')
    expect(html).toContain('review the permissions'); expect(html).not.toContain('API key')
  })
  test('signed preview displays explicit read/write permissions and global workspace effect', () => {
    const html = renderToStaticMarkup(<ConsentDetails authorization={preview} email="person@example.test" busy={false} error={null} decide={() => {}} />)
    for (const text of ['Cedar Assistant', 'person@example.test', 'Read notes', 'Read transcripts', 'Switch your active workspace', 'Deny', 'Allow', 'Other members’ private notes stay private', 'across connected apps', 'The app provides its own name']) expect(html).toContain(text)
    expect(html).not.toContain('autoFocus'); expect(html).not.toContain('checked=')
  })
  test('unknown permission disables Allow while Deny remains available', () => {
    const html = renderToStaticMarkup(<ConsentDetails authorization={{ ...preview, scopes: ['unknown:write'] }} email="person@example.test" busy={false} error={null} decide={() => {}} />)
    expect(html).toContain('Unrecognized permission')
    expect(html).toMatch(/<button type="button" class="button" disabled="">Allow<\/button>/)
    expect(html).toMatch(/<button type="button" class="admin-secondary-button">Deny<\/button>/)
  })
  test('expired request offers restart and no permission buttons', () => {
    const html = renderToStaticMarkup(<InvalidAuthorization />)
    expect(html).toContain('Start again'); expect(html).not.toContain('>Allow<'); expect(html).not.toContain('>Deny<')
    expect(renderToStaticMarkup(<MCPConsent />)).toContain('Start again')
  })
  test('logged-out goes to the sign-in page and back; a connected grant offers explicit Disconnect', () => {
    expect(signInPath({ pathname: '/mcp/connect', search: '?sig=a&ba_param=sig', hash: '' })).toBe('/login?return=%2Fmcp%2Fconnect%3Fsig%3Da%26ba_param%3Dsig')
    const html = renderToStaticMarkup(<ConnectionRow connection={{ id: 'grant', clientId: 'client', clientName: '<script>bad</script>', scopes: ['notes:read'], createdAt: '2026-11-13T12:00:00Z', updatedAt: '2026-11-13T12:00:00Z' }} disconnect={() => {}} />)
    expect(html).toContain('&lt;script&gt;bad&lt;/script&gt;'); expect(html).toContain('Disconnect'); expect(html).not.toContain('<script>bad')
  })
})
