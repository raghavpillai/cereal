import { createAuthClient } from 'better-auth/react'
import { oauthProviderClient } from '@better-auth/oauth-provider/client'

/** Better Auth owns the HttpOnly session cookie. No passwords or session tokens
 * are retained in URLs, browser storage or application state after submission. */
export function createSessionAuthClient(baseURL?: string, transport?: typeof fetch) { return createAuthClient({
  baseURL,
  basePath: '/api/auth',
  plugins: [oauthProviderClient()],
  disableDefaultFetchPlugins: true,
  fetchOptions: {
    customFetchImpl: transport,
    credentials: 'same-origin',
    cache: 'no-store',
    redirect: 'error',
    referrerPolicy: 'no-referrer',
  },
}) }
export const authClient = createSessionAuthClient()

export function signInProblem(status?: number): string {
  return status === 429 ? 'Too many attempts. Please wait before trying again.'
    : status && status >= 500 ? 'Sign-in is temporarily unavailable. Please try again.'
    : 'The email or password was not accepted.'
}

/** What this server lets people sign in with. Public: it is read before anyone is signed in. */
export interface AuthMethods { password: boolean; google: boolean; instanceName: string }
/** A server from before sign-in settings answers 404: it only ever had email and password. */
export async function fetchAuthMethods(signal?: AbortSignal, transport: typeof fetch = globalThis.fetch): Promise<AuthMethods> {
  const response = await transport('/api/v1/auth/methods', { signal, credentials: 'omit', cache: 'no-store', redirect: 'error', referrerPolicy: 'no-referrer' })
  if (response.status === 404) return { password: true, google: false, instanceName: 'Cereal' }
  if (!response.ok) throw new Error('Sign-in methods are unavailable')
  const body = await response.json() as Partial<AuthMethods>
  return { password: body.password !== false, google: body.google === true, instanceName: typeof body.instanceName === 'string' && body.instanceName.trim() ? body.instanceName : 'Cereal' }
}

/** The `?error=` codes the server sends back to the sign-in page after a refused Google sign-in. */
export function signInErrorMessage(code: string | null): string | null {
  return code === 'not_allowed' ? 'That Google account is not allowed to sign in here. Ask your administrator to add your address.'
    : code === 'google_unavailable' ? 'Google sign-in is not available on this server.'
    : code === 'cancelled' ? 'Google sign-in was cancelled.'
    : code === 'failed' ? 'Google sign-in did not finish. Please try again.'
    : null
}

/** Where to go after signing in: a path on this origin, never another site and never the API. */
export function safeReturnPath(raw: string | null | undefined, origin: string): string | null {
  // Long enough for the signed query an AI client's connection carries through sign-in (the bound `signedOAuthQuery` uses).
  if (!raw || raw.length > 16_384 || !raw.startsWith('/') || raw.startsWith('//') || /[\\\u0000-\u001f\u007f]/.test(raw)) return null
  try {
    const url = new URL(raw, origin)
    if (url.origin !== origin) return null
    const path = url.pathname + url.search + url.hash
    return path.startsWith('//') || path === '/api' || path.startsWith('/api/') || url.pathname === '/login' ? null : path
  } catch { return null }
}

/** An app asking this browser to sign it in: `/login?app=1&challenge=…&state=…`. */
export interface AppSignInRequest { challenge: string; state: string }
export function appSignInRequest(search: string): AppSignInRequest | null {
  const params = new URLSearchParams(search), challenge = params.get('challenge') ?? '', state = params.get('state') ?? ''
  if (params.get('app') !== '1' || !/^[A-Za-z0-9_-]{43}$/.test(challenge) || !/^[A-Za-z0-9_-]{8,128}$/.test(state)) return null
  return { challenge, state }
}
/** The sign-in page for the same app request, used as the place to come back to after Google. */
export const appSignInPath = (request: AppSignInRequest) => `/login?app=1&challenge=${request.challenge}&state=${request.state}`
/** What the browser hands to the app. The code is single use and worthless without the app's own verifier. */
export function appCallbackURL(code: string, state: string, origin: string): string {
  return `cereal://auth?code=${encodeURIComponent(code)}&state=${encodeURIComponent(state)}&server=${encodeURIComponent(origin)}`
}
export const googleStartURL = (returnPath: string | null) => '/api/v1/auth/google/start' + (returnPath ? '?return=' + encodeURIComponent(returnPath) : '')

/** Asks the server for a code the app can redeem. Sent with the session cookie from this origin only. */
export async function requestAppHandoff(challenge: string, signal?: AbortSignal, transport: typeof fetch = globalThis.fetch): Promise<string> {
  const response = await transport('/api/v1/auth/app-handoff', { method: 'POST', signal, credentials: 'same-origin', cache: 'no-store', redirect: 'error', referrerPolicy: 'no-referrer', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ challenge }) })
  if (!response.ok) throw new AppHandoffError(response.status)
  const body = await response.json() as { code?: unknown }
  if (typeof body.code !== 'string' || !/^[A-Za-z0-9_-]{43}$/.test(body.code)) throw new AppHandoffError(502)
  return body.code
}
export class AppHandoffError extends Error {
  constructor(readonly status: number) {
    super(status === 401 ? 'Your session has ended. Sign in again to continue.'
      : status === 400 ? 'This sign-in request is not valid. Start again from the Cereal app.'
      : 'Cereal could not be opened. Please try again.')
    this.name = 'AppHandoffError'
  }
}
