import { Effect, Result } from 'effect'
import { FetchHttpClient } from 'effect/http'
import { HttpApiClient } from 'effect/http-api'
import { MeetingsApi } from '@meeting-notes/contracts/api'
import type { ShareDTO } from '@meeting-notes/contracts'

/** Shared links of a note: make one, list them, revoke them. The same three calls the
 * desktop app makes. A link publishes a fixed copy of the chosen text; the transcript
 * has no field in the request and cannot be shared. */

export class ShareError extends Error {
  constructor(readonly status: number) {
    super(status === 401 ? 'Your session has ended. Sign in again.'
      : status === 404 ? 'This note is no longer available.'
      : status === 400 ? 'Choose something to share, then try again.'
      : status === 422 ? 'The server returned an invalid sharing link.'
      : 'The link could not be changed. Please try again.')
    this.name = 'ShareError'
  }
}

/** The address people are sent, or null when the server's answer is not the link it
 * should be: a 43-character token and the one path that carries it. */
export function shareURL(origin: string, token: string, path: string): string | null {
  if (!/^[A-Za-z0-9_-]{43}$/.test(token) || path !== `/s/${token}`) return null
  try { return `${new URL(origin).origin}${path}` } catch { return null }
}

/** The token a shared address ends in, for a page that only knows its own address. */
export function shareToken(pathname: string): string | null {
  return /^\/s\/([A-Za-z0-9_-]{43})\/?$/.exec(pathname)?.[1] ?? null
}

export const shareIsActive = (share: Pick<ShareDTO, 'revokedAt' | 'expiresAt'>, now = Date.now()) =>
  share.revokedAt === null && (share.expiresAt === null || Date.parse(share.expiresAt) > now)

/** Links that still open, newest first. */
export function activeShares(shares: readonly ShareDTO[], now = Date.now()): ShareDTO[] {
  return shares.filter(share => shareIsActive(share, now)).sort((a, b) => Date.parse(b.createdAt) - Date.parse(a.createdAt) || (a.id < b.id ? -1 : 1))
}

/** What may be put in a link, given what the note has: a link with nothing in it is never made. */
export function shareChoice(has: { enhanced: boolean; body: boolean }, wanted: { enhanced: boolean; body: boolean }): { includeEnhanced: boolean; includeBody: boolean } | null {
  const includeEnhanced = has.enhanced && wanted.enhanced, includeBody = has.body && wanted.body
  return includeEnhanced || includeBody ? { includeEnhanced, includeBody } : null
}

type Client = HttpApiClient.ForApi<typeof MeetingsApi>

/** Same-origin cookie transport, decoded with the canonical schemas. */
export function createSharesClient(baseUrl: string, transport: typeof fetch = globalThis.fetch) {
  const origin = new URL(baseUrl).origin
  const sameOrigin: typeof fetch = Object.assign(async (...[input, init]: Parameters<typeof fetch>) => {
    const request = new Request(input, init)
    if (new URL(request.url).origin !== origin) throw new ShareError(0)
    request.headers.delete('Authorization')
    return transport(request, { credentials: 'same-origin', cache: 'no-store', redirect: 'error', referrerPolicy: 'no-referrer' })
  }, { preconnect: transport.preconnect })
  async function execute<A, E>(operation: (client: Client) => Effect.Effect<A, E>, signal?: AbortSignal): Promise<A> {
    const program = Effect.gen(function* () {
      const client = yield* HttpApiClient.make(MeetingsApi, { baseUrl: origin })
      return yield* operation(client)
    }).pipe(
      Effect.provide(FetchHttpClient.layer),
      Effect.provideService(FetchHttpClient.Fetch, sameOrigin),
      Effect.provideService(FetchHttpClient.RequestInit, { credentials: 'same-origin', cache: 'no-store', redirect: 'error', referrerPolicy: 'no-referrer' }),
    )
    const result = await Effect.runPromise(Effect.result(program), { signal })
    if (Result.isFailure(result)) {
      if (signal?.aborted) throw new DOMException('Request canceled', 'AbortError')
      const error = result.failure
      const kind = typeof error === 'object' && error !== null && 'error' in error ? error.error : undefined
      throw new ShareError(kind === 'unauthorized' ? 401 : kind === 'not_found' ? 404 : kind === 'invalid_request' ? 400 : 503)
    }
    return result.success
  }
  return {
    list: (noteId: string, signal?: AbortSignal) => execute(c => c.owner.listShares({ params: { id: noteId } }), signal),
    /** The token is returned once, here; the server keeps only its hash. */
    async create(noteId: string, include: { includeEnhanced: boolean; includeBody: boolean }, signal?: AbortSignal): Promise<{ share: ShareDTO; url: string }> {
      const made = await execute(c => c.owner.createShare({ params: { id: noteId }, payload: include }), signal)
      const url = shareURL(origin, made.token, made.path)
      if (!url) throw new ShareError(422)
      return { share: made.share, url }
    },
    revoke: (shareId: string, signal?: AbortSignal) => execute(c => c.owner.revokeShare({ params: { id: shareId } }), signal),
  }
}
export type SharesClient = ReturnType<typeof createSharesClient>
