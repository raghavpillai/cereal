import { describe, expect, test } from 'bun:test'
import { AppHandoffError, appCallbackURL, appSignInPath, appSignInRequest, fetchAuthMethods, googleStartURL, requestAppHandoff, safeReturnPath, signInErrorMessage } from './auth-client'

const origin = 'https://notes.example.test'
// The fetch options are checked as they were passed: Bun's Request does not report the credentials mode it was given.
function transport(handler: (request: Request, init: RequestInit) => Response | Promise<Response>): typeof fetch {
  return Object.assign(async (...[input, init]: Parameters<typeof fetch>) => handler(new Request(new URL(String(input), origin), init), init ?? {}), { preconnect: () => {} }) as typeof fetch
}
const challenge = 'c'.repeat(43), code = 'k'.repeat(43)

describe('sign-in page helpers', () => {
  test('sign-in methods come from the public endpoint without credentials; a server from before them means password only', async () => {
    let credentials = ''
    const methods = await fetchAuthMethods(undefined, transport((request, init) => { credentials = init.credentials ?? ''; expect(new URL(request.url).pathname).toBe('/api/v1/auth/methods'); return Response.json({ password: false, google: true, instanceName: 'Product studio' }) }))
    expect(methods).toEqual({ password: false, google: true, instanceName: 'Product studio' })
    expect(credentials).toBe('omit')
    expect(await fetchAuthMethods(undefined, transport(() => Response.json({ error: 'not_found' }, { status: 404 })))).toEqual({ password: true, google: false, instanceName: 'Cereal' })
    // Anything unexpected in the answer never turns a method on by accident.
    expect(await fetchAuthMethods(undefined, transport(() => Response.json({ google: 'yes', instanceName: '' })))).toEqual({ password: true, google: false, instanceName: 'Cereal' })
    expect(fetchAuthMethods(undefined, transport(() => Response.json({}, { status: 503 })))).rejects.toThrow()
  })

  test('each refusal code has its own sentence, and an unknown code shows nothing', () => {
    for (const known of ['not_allowed', 'google_unavailable', 'cancelled', 'failed']) expect(signInErrorMessage(known)).toBeTruthy()
    expect(signInErrorMessage('not_allowed')).toContain('not allowed')
    expect(new Set(['not_allowed', 'google_unavailable', 'cancelled', 'failed'].map(signInErrorMessage)).size).toBe(4)
    for (const unknown of [null, '', 'FAILED', '<script>alert(1)</script>', 'https://evil.example']) expect(signInErrorMessage(unknown)).toBeNull()
  })

  test('the place to go after signing in is a path on this origin: never another site, the API, or the sign-in page itself', () => {
    for (const path of ['/notes', '/notes/abc?tab=notes#top', '/s/' + 'A'.repeat(43), '/admin', '/%2f%2fevil.example', '/%5Cevil.example']) { expect(safeReturnPath(path, origin)).toBe(path); expect(new URL(safeReturnPath(path, origin)!, origin).origin).toBe(origin) }
    for (const path of [null, undefined, '', 'notes', 'https://evil.example/', '//evil.example', '/\\evil.example', '/.//evil.example', '/notes/..//evil.example', '/%2e%2e//evil.example', '/%2E%2E//evil.example', '/notes/%2e%2e/%2e%2e//evil.example', 'https:evil.example', 'http://notes.example.test@evil.example/', ' /notes', 'javascript:alert(1)', '/a\nb', '/api/auth/sign-out', '/api', '/login', '/login?return=/login', '/' + 'a'.repeat(16_384)]) expect(safeReturnPath(path, origin)).toBeNull()
    // An AI client's connection carries its signed query through sign-in: thousands of characters, with & = % and non-ASCII.
    const signed = '/mcp/consent?client_id=https%3A%2F%2Fclient.example%2Fmeta.json&scope=notes%3Aread%20notes%3Awrite&redirect_uri=https%3A%2F%2Fclient.example%2Fcb%3Fa%3D1%26b%3D2&state=' + 'Zm9v'.repeat(1300) + '&name=Caf%C3%A9%20%E2%98%95&label=Ünïcödé-客户端&sig=' + 'a1B2-_'.repeat(150) + '%3D%3D'
    expect(signed.length).toBeGreaterThan(6000)
    expect(safeReturnPath(signed, origin)).not.toBeNull()
    const kept = new URL(safeReturnPath(signed, origin)!, origin), sent = new URL(signed, origin)
    expect(kept.origin).toBe(origin)
    expect(kept.pathname).toBe('/mcp/consent')
    expect([...kept.searchParams]).toEqual([...sent.searchParams])
    expect(kept.searchParams.get('redirect_uri')).toBe('https://client.example/cb?a=1&b=2')
    expect(kept.searchParams.get('name')).toBe('Café ☕')
    expect(kept.searchParams.get('label')).toBe('Ünïcödé-客户端')
    expect(kept.searchParams.get('state')!.length).toBe(5200)
    // The open-redirect checks are the same at any length.
    for (const path of ['//evil.example/' + signed.slice(1), '/\\evil.example' + signed, '/..//evil.example/' + signed.slice(1), '/api/auth' + signed, '/login' + signed.slice(12), signed + '&pad=' + 'x'.repeat(16_384)]) expect(safeReturnPath(path, origin)).toBeNull()
    expect(googleStartURL('/notes/abc?tab=notes')).toBe('/api/v1/auth/google/start?return=%2Fnotes%2Fabc%3Ftab%3Dnotes')
    expect(googleStartURL(null)).toBe('/api/v1/auth/google/start')
  })

  test('an app sign-in request needs app=1, a 43-character challenge and a state; anything else is not one', () => {
    expect(appSignInRequest(`?app=1&challenge=${challenge}&state=abcdefghijklmnopqrstuv`)).toEqual({ challenge, state: 'abcdefghijklmnopqrstuv' })
    for (const search of ['', '?app=1', `?challenge=${challenge}&state=abcdefghijklmnopqrstuv`, `?app=0&challenge=${challenge}&state=abcdefghijklmnopqrstuv`, `?app=1&challenge=${challenge.slice(1)}&state=abcdefghijklmnopqrstuv`, `?app=1&challenge=${challenge}&state=short`, `?app=1&challenge=${challenge}&state=has%20space%20in%20it`, `?app=1&challenge=${challenge.slice(1)}%2F&state=abcdefghijklmnopqrstuv`]) expect(appSignInRequest(search)).toBeNull()
    // The same request, as a path the Google flow can return to.
    const request = { challenge, state: 'abcdefghijklmnopqrstuv' }
    expect(appSignInRequest(new URL(appSignInPath(request), origin).search)).toEqual(request)
    expect(safeReturnPath(appSignInPath(request), origin)).toBeNull() // only the server-side check allows it, for this one purpose
  })

  test('the app is handed a code, its own state and this server, in the cereal scheme and nothing else', () => {
    const url = new URL(appCallbackURL(code, 'state-_value', origin))
    expect(url.protocol).toBe('cereal:'); expect(url.host).toBe('auth')
    expect(Object.fromEntries(url.searchParams)).toEqual({ code, state: 'state-_value', server: origin })
    expect(appCallbackURL('a&b=c', 's&t', 'https://x.test/?a=1&b=2')).toBe('cereal://auth?code=a%26b%3Dc&state=s%26t&server=https%3A%2F%2Fx.test%2F%3Fa%3D1%26b%3D2')
  })

  test('asking for a code posts the challenge with the session cookie from this origin and trusts only a well-formed answer', async () => {
    let seen: { method: string; credentials: string; body: unknown; type: string | null } | null = null
    const got = await requestAppHandoff(challenge, undefined, transport(async (request, init) => { seen = { method: request.method, credentials: init.credentials ?? '', body: await request.json(), type: request.headers.get('content-type') }; return Response.json({ code }) }))
    expect(got).toBe(code)
    expect(seen!).toEqual({ method: 'POST', credentials: 'same-origin', body: { challenge }, type: 'application/json' })
    for (const [response, status] of [[Response.json({ error: 'unauthorized' }, { status: 401 }), 401], [Response.json({ error: 'invalid_request', message: 'server text that is not shown' }, { status: 400 }), 400], [Response.json({ error: 'forbidden' }, { status: 403 }), 403], [Response.json({ code: 'not a code' }), 502], [Response.json({}), 502]] as const) {
      const error = await requestAppHandoff(challenge, undefined, transport(() => response)).catch(error => error)
      expect(error).toBeInstanceOf(AppHandoffError); expect(error.status).toBe(status)
      expect(error.message).not.toContain('server text')
    }
  })
})
