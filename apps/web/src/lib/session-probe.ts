import type { AuthSessionDTO } from '@meeting-notes/contracts'

/** Who is signed in to this browser, or null when nobody is. A signed-out visitor is answered by one plain
 * request; the typed client, which is most of what a signed-in page downloads, is fetched only for someone who
 * is signed in, and it then reads and checks the session as it always has. Other failures are the client's to report. */
export async function readSession(signal?: AbortSignal): Promise<AuthSessionDTO | null> {
  const response = await fetch('/api/v1/session', { signal, credentials: 'same-origin', cache: 'no-store', redirect: 'error', referrerPolicy: 'no-referrer', headers: { Accept: 'application/json' } })
  if (response.status === 401) return null
  const { createAdminClient } = await import('./admin-client')
  return createAdminClient(window.location.origin).getSession(signal)
}

/** Set while this browser is in first-run setup at /admin, and read before first paint (the root route),
 * so that arriving at setup, or reloading it, never shows the site header for a moment first.
 * It is the only thing kept, and only for the tab's session. */
const SETUP_FLAG = 'cereal-setup'
export function rememberSetup(showing: boolean) {
  try { if (showing) sessionStorage.setItem(SETUP_FLAG, '1'); else sessionStorage.removeItem(SETUP_FLAG) } catch { /* the header shows for a moment, as before */ }
  if (!showing && typeof document !== 'undefined') delete document.documentElement.dataset.setup
}
