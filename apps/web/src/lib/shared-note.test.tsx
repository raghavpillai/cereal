import { describe, expect, test } from 'bun:test'
import { signInPath } from '../components/account/session-gate'
import { safeReturnPath } from './auth-client'
import { ReaderLoadError, getSharedNote } from './shared-note'

const token = 'AbCdEfGhIjKlMnOpQrStUvWxYz0123456789-_AbCdE'
const snapshot = { shareId: '00000000-0000-4000-8000-000000000001', title: 'Launch', body: 'Mine', enhanced: '## Notes', date: '2026-11-15', duration: '1:54', sharedAt: '2026-11-15T12:00:00.000Z', expiresAt: null }
function transport(handler: (request: Request, init?: RequestInit) => Promise<Response> | Response): typeof fetch {
  return Object.assign(async (...[input, init]: Parameters<typeof fetch>) => handler(new Request(input, init), init), { preconnect: () => {} }) as typeof fetch
}
const read = (handler: Parameters<typeof transport>[0], value = token) => getSharedNote(value, undefined, transport(handler), 'https://notes.example')

describe('a shared link is read by someone signed in to its server', () => {
  test('the request carries the session cookie and the answer decides the page', async () => {
    let seen = ''
    expect(await read((request, init) => { seen = `${request.method} ${request.url} ${init?.credentials}`; return Response.json({ ...snapshot, noteId: 'never kept', transcriptSegments: [{ text: 'private' }] }) })).toEqual({ kind: 'ready', note: snapshot })
    expect(seen).toBe(`GET https://notes.example/api/v1/shared/${token} same-origin`)
    // Not signed in: the page goes to sign-in. Revoked, expired or unknown: unavailable.
    expect(await read(() => Response.json({ error: 'unauthorized', message: 'Sign in' }, { status: 401 }))).toEqual({ kind: 'signed-out' })
    expect(await read(() => Response.json({ error: 'not_found', message: 'Gone' }, { status: 404 }))).toEqual({ kind: 'unavailable' })
    await expect(read(() => new Response('down', { status: 503 }))).rejects.toBeInstanceOf(ReaderLoadError)
    await expect(read(() => { throw new TypeError('offline') })).rejects.toBeInstanceOf(ReaderLoadError)
    await expect(read(() => Response.json({ ...snapshot, enhanced: 5 }))).rejects.toBeInstanceOf(ReaderLoadError)
  })
  test('something that cannot be a token is never sent', async () => {
    let asked = false
    for (const bad of ['short', `${token}x`, '../admin', `${token.slice(0, 42)}/`]) expect(await read(() => { asked = true; return Response.json(snapshot) }, bad)).toEqual({ kind: 'unavailable' })
    expect(asked).toBe(false)
  })
  test('signing in returns to where the visitor was', () => {
    expect(signInPath({ pathname: `/s/${token}`, search: '', hash: '' })).toBe(`/login?return=${encodeURIComponent(`/s/${token}`)}`)
    expect(signInPath({ pathname: '/notes/abc', search: '?x=1', hash: '#top' })).toBe('/login?return=%2Fnotes%2Fabc%3Fx%3D1%23top')
    // The Google Calendar callback's answer comes back with the visitor.
    expect(new URL(signInPath({ pathname: '/account', search: '?google=connected', hash: '' }), 'https://notes.example').searchParams.get('return')).toBe('/account?google=connected')
    // The notes list is where sign-in leads anyway.
    expect(signInPath({ pathname: '/notes', search: '', hash: '' })).toBe('/login')
    expect(signInPath({ pathname: '/', search: '', hash: '' })).toBe('/login')
    expect(signInPath({ pathname: '/login', search: '?return=/notes', hash: '' })).toBe('/login')
  })
  test('an app\'s signed request survives the trip to sign-in and back unchanged', () => {
    // What the authorization endpoint sends to the connect page: every separator and escape a query can hold.
    const names = ['client_id', 'code_challenge', 'code_challenge_method', 'redirect_uri', 'resource', 'response_type', 'scope', 'state', 'exp', 'ba_iat', 'ba_param']
    const signed = '?client_id=cedar%2Bassistant&code_challenge=E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM&code_challenge_method=S256'
      + '&redirect_uri=http%3A%2F%2Flocalhost%3A8787%2Fcallback%3Fapp%3Dcedar%26mode%3Da%253Db&resource=https%3A%2F%2Fnotes.example%2Fapi%2Fmcp&response_type=code'
      + '&scope=notes%3Aread+transcripts%3Aread+offline_access&state=a%3Db%26c%3D%25d+%2B%F0%9F%A6%80&exp=1794760000&ba_iat=1794759400000'
      + names.map(name => `&ba_param=${name}`).join('') + '&sig=Zm9v-YmFy_YmF6LXNpZw%3D%3D'
    expect(signed.length).toBeGreaterThan(600)
    const there = signInPath({ pathname: '/mcp/connect', search: signed, hash: '' })
    // The sign-in page reads `return` from its own address and goes there when sign-in is done.
    const carried = new URL(there, 'https://notes.example').searchParams.get('return')
    expect(carried).toBe(`/mcp/connect${signed}`)
    expect(safeReturnPath(carried, 'https://notes.example')).toBe(`/mcp/connect${signed}`)
    // Back on the connect page, the query is the same bytes, so its signature still stands.
    const back = new URL(safeReturnPath(carried, 'https://notes.example')!, 'https://notes.example')
    expect(back.search).toBe(signed)
    expect([...back.searchParams.entries()]).toEqual([...new URLSearchParams(signed).entries()])
    expect(back.searchParams.get('redirect_uri')).toBe('http://localhost:8787/callback?app=cedar&mode=a%3Db')
    expect(back.searchParams.get('state')).toBe('a=b&c=%d +🦀')
    // The same holds for the consent page.
    expect(safeReturnPath(new URL(signInPath({ pathname: '/mcp/consent', search: signed, hash: '' }), 'https://notes.example').searchParams.get('return'), 'https://notes.example')).toBe(`/mcp/consent${signed}`)
  })
})
