/** Preserve only the provider's signed authorization parameters. The server,
 * never the browser, verifies their signature and resolves client identity. */
export function signedOAuthQuery(search: string): string | null {
  if (search.length > 16_384) return null
  const params = new URLSearchParams(search)
  const signedNames = params.getAll('ba_param')
  if (params.getAll('sig').length !== 1 || !params.get('sig') || !signedNames.length) return null
  const signed = new URLSearchParams()
  for (const [key, value] of params) {
    if (key === 'sig' || key === 'ba_param' || signedNames.includes(key)) signed.append(key, value)
  }
  return signed.toString()
}

/** Browser sign-in can resume our own authorization UI, never a query-supplied
 * callback. The final registered OAuth callback is handled after consent. */
export function safeOAuthResume(value: unknown, origin: string): string | null {
  if (typeof value !== 'string' || value.length > 16_384 || /[\\\u0000-\u0020]/.test(value)) return null
  try {
    const url = new URL(value, origin)
    if (url.origin !== origin || url.username || url.password || url.hash) return null
    if (!['/mcp/consent', '/mcp/connect', '/api/auth/oauth2/authorize'].includes(url.pathname)) return null
    return url.href
  } catch { return null }
}

/** The address an AI client is given: every programmatic endpoint of an instance lives under `/api`. */
export function mcpConnectionURL(origin: string): string | null {
  try {
    const url = new URL(origin)
    const loopback = ['localhost', '127.0.0.1', '[::1]'].includes(url.hostname)
    if ((url.protocol !== 'https:' && !(url.protocol === 'http:' && loopback)) || url.username || url.password || url.pathname !== '/' || url.search || url.hash) return null
    return `${url.origin}/api/mcp`
  } catch { return null }
}

export const mcpScopeDescriptions: Record<string, { label: string; detail: string; write?: boolean }> = {
  'notes:read': { label: 'Read notes', detail: 'Find and read your meeting notes and summaries.' },
  'transcripts:read': { label: 'Read transcripts', detail: 'Read the transcripts attached to your meetings.' },
  'attachments:read': { label: 'Read attachments', detail: 'Read attachments available to your account.' },
  'profile:read': { label: 'Read your profile', detail: 'See your name and profile information.' },
  'account:read': { label: 'Read account and workspaces', detail: 'See your account and the workspaces you belong to.' },
  'notes:write': { label: 'Change notes', detail: 'Create and update your meeting notes.', write: true },
  'folders:write': { label: 'Manage folders', detail: 'Create folders and organize your notes.', write: true },
  'account:write': { label: 'Switch your active workspace', detail: 'Change the active workspace for your account, including other connected clients.', write: true },
  offline_access: { label: 'Stay connected', detail: 'Keep access until you disconnect this app.' },
}

export function knownMCPScope(scope: string): boolean { return Object.hasOwn(mcpScopeDescriptions, scope) }

/** Only a callback returned by the consent endpoint after an explicit decision
 * may leave this instance, and it must match the server-validated registration. */
export function safeConsentCallback(value: string, registered: string): string | null {
  if ([value, registered].some(part => part.length > 16_384 || /[\\\u0000-\u0020]/.test(part))) return null
  try {
    const target = new URL(value), expected = new URL(registered)
    if (['javascript:', 'data:', 'file:', 'blob:', 'about:', 'vbscript:'].includes(target.protocol)) return null
    if (target.username || target.password || target.hash) return null
    if (target.protocol !== expected.protocol || target.hostname !== expected.hostname || target.port !== expected.port || target.pathname !== expected.pathname) return null
    for (const key of new Set(expected.searchParams.keys())) {
      const original = expected.searchParams.getAll(key), returned = target.searchParams.getAll(key)
      if (original.length !== returned.length || original.some((item, index) => item !== returned[index])) return null
    }
    return target.href
  } catch { return null }
}
