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
