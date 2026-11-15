import { describe, expect, test } from 'bun:test'
import type { ShareDTO } from '@meeting-notes/contracts'
import { ShareError, activeShares, createSharesClient, shareChoice, shareIsActive, shareToken, shareURL } from './shares'

const noteId = '01136768-b41d-4ee8-ad19-e73976f18475', token = 'AbCdEfGhIjKlMnOpQrStUvWxYz0123456789-_AbCdE'
const id = (n: number) => `00000000-0000-4000-8000-${String(n).padStart(12, '0')}`
const share = (n: number, fields: Partial<ShareDTO> = {}): ShareDTO => ({ id: id(n), noteId, createdAt: `2026-11-${String(10 + n).padStart(2, '0')}T12:00:00.000Z`, expiresAt: null, revokedAt: null, ...fields })
function transport(handler: (request: Request, init?: RequestInit) => Promise<Response> | Response): typeof fetch {
  return Object.assign(async (...[input, init]: Parameters<typeof fetch>) => handler(new Request(input, init), init), { preconnect: () => {} }) as typeof fetch
}

describe('shared links', () => {
  test('the address is this server and the one path that carries the token', () => {
    expect(shareURL('https://notes.example/', token, `/s/${token}`)).toBe(`https://notes.example/s/${token}`)
    expect(shareURL('http://127.0.0.1:3000', token, `/s/${token}`)).toBe(`http://127.0.0.1:3000/s/${token}`)
    for (const path of ['@other.example/x', '//other.example/x', '/s/other', '/admin', `/s/../${token}`, `/s/${token}/`]) expect(shareURL('https://notes.example', token, path)).toBeNull()
    expect(shareURL('https://notes.example', 'short', '/s/short')).toBeNull()
    expect(shareURL('https://notes.example', '/'.repeat(43), `/s/${'/'.repeat(43)}`)).toBeNull()
    expect(shareToken(`/s/${token}`)).toBe(token)
    expect(shareToken(`/s/${token}/`)).toBe(token)
    for (const path of ['/s/short', `/notes/${token}`, `/s/${token}/more`, '/']) expect(shareToken(path)).toBeNull()
  })
  test('a link holds only what was chosen and the note has; a link with nothing in it is not made', () => {
    expect(shareChoice({ enhanced: true, body: true }, { enhanced: true, body: true })).toEqual({ includeEnhanced: true, includeBody: true })
    expect(shareChoice({ enhanced: true, body: true }, { enhanced: true, body: false })).toEqual({ includeEnhanced: true, includeBody: false })
    expect(shareChoice({ enhanced: true, body: false }, { enhanced: true, body: true })).toEqual({ includeEnhanced: true, includeBody: false })
    expect(shareChoice({ enhanced: true, body: true }, { enhanced: false, body: false })).toBeNull()
    expect(shareChoice({ enhanced: false, body: true }, { enhanced: true, body: false })).toBeNull()
    // There is no way to ask for the transcript.
    expect(Object.keys(shareChoice({ enhanced: true, body: true }, { enhanced: true, body: true })!).sort()).toEqual(['includeBody', 'includeEnhanced'])
  })
  test('links that still open, newest first', () => {
    const now = Date.parse('2026-11-15T12:00:00.000Z')
    const shares = [share(1), share(2, { revokedAt: '2026-11-13T00:00:00.000Z' }), share(3, { expiresAt: '2026-11-14T00:00:00.000Z' }), share(4, { expiresAt: '2026-12-01T00:00:00.000Z' })]
    expect(activeShares(shares, now).map(item => item.id)).toEqual([id(4), id(1)])
    expect(shareIsActive(share(1), now)).toBe(true)
    expect(shareIsActive(share(3, { expiresAt: '2026-11-15T12:00:00.000Z' }), now)).toBe(false)
    expect(activeShares([], now)).toEqual([])
  })
  test('the three calls use the session cookie, never a bearer token, and the canonical routes', async () => {
    const seen: { call: string; body?: unknown }[] = []
    const client = createSharesClient('https://notes.example', transport(async (request, init) => {
      expect(request.headers.get('Authorization')).toBeNull()
      expect(init?.credentials).toBe('same-origin')
      expect(init?.redirect).toBe('error')
      const path = new URL(request.url).pathname
      seen.push({ call: `${request.method} ${path}`, ...(request.body ? { body: await request.json() } : {}) })
      if (request.method === 'POST') return Response.json({ share: share(9), token, path: `/s/${token}`, internal: 'dropped' })
      if (request.method === 'DELETE') return Response.json({ ...share(1), revokedAt: '2026-11-15T12:00:00.000Z' })
      return Response.json([share(1), share(2)])
    }))
    expect((await client.list(noteId)).map(item => item.id)).toEqual([id(1), id(2)])
    expect(await client.create(noteId, { includeEnhanced: true, includeBody: false })).toEqual({ share: share(9), url: `https://notes.example/s/${token}` })
    expect((await client.revoke(id(1))).revokedAt).toBe('2026-11-15T12:00:00.000Z')
    expect(seen).toEqual([
      { call: `GET /api/v1/notes/${noteId}/shares` },
      { call: `POST /api/v1/notes/${noteId}/shares`, body: { includeEnhanced: true, includeBody: false } },
      { call: `DELETE /api/v1/shares/${id(1)}` },
    ])
  })
  test('a link that is not this server\'s own is refused, and failures carry no server text', async () => {
    const elsewhere = createSharesClient('https://notes.example', transport(() => Response.json({ share: share(9), token, path: `/s/${'x'.repeat(43)}` })))
    expect(await elsewhere.create(noteId, { includeEnhanced: true, includeBody: true }).catch(error => error.status)).toBe(422)
    const malformed = createSharesClient('https://notes.example', transport(() => Response.json([{ ...share(1), revokedAt: 5 }])))
    await expect(malformed.list(noteId)).rejects.toBeInstanceOf(ShareError)
    const expired = createSharesClient('https://notes.example', transport(() => Response.json({ error: 'unauthorized', message: 'private upstream detail' }, { status: 401 })))
    const error = await expired.list(noteId).catch(error => error)
    expect(error.status).toBe(401)
    expect(error.message).not.toContain('upstream')
    const gone = createSharesClient('https://notes.example', transport(() => Response.json({ error: 'not_found', message: 'x' }, { status: 404 })))
    expect(await gone.revoke(id(1)).catch(error => error.status)).toBe(404)
  })
})
