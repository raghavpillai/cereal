/** The only cross-origin navigation permitted by Connect Google. Authorization
 * comes from the signed-in backend and must return to this instance's callback. */
export function googleAuthorizationURL(value: string, currentOrigin: string): string | null {
  try {
    const url = new URL(value)
    if (url.origin !== 'https://accounts.google.com' || url.pathname !== '/o/oauth2/v2/auth' || url.username || url.password || url.hash) return null
    if (url.searchParams.get('redirect_uri') !== `${new URL(currentOrigin).origin}/api/integrations/google/callback`) return null
    if (url.searchParams.get('response_type') !== 'code' || url.searchParams.get('code_challenge_method') !== 'S256') return null
    if (!/^[A-Za-z0-9_-]{43}$/.test(url.searchParams.get('state') ?? '') || !/^[A-Za-z0-9_-]{43}$/.test(url.searchParams.get('code_challenge') ?? '')) return null
    return url.href
  } catch { return null }
}
