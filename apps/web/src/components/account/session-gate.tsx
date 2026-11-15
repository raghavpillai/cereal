import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react'
import type { AuthSessionDTO } from '@meeting-notes/contracts'
import { authClient } from '../../lib/auth-client'
import { AdminClientError, createAdminClient, type AdminClient } from '../../lib/admin-client'
import { Loading, Notice } from '../admin/shared'

type State = { kind: 'loading' } | { kind: 'anonymous' } | { kind: 'error' } | { kind: 'signingOut' }
  | { kind: 'closed'; error: boolean } | { kind: 'ready'; session: AuthSessionDTO; client: AdminClient }

export function SessionGate({ children, fallback, onClient, frame = content => content }: {
  /** Wraps what the gate itself shows (checking, signing out, errors), for a page whose own content is not always inside one frame. */
  frame?: (content: ReactNode) => ReactNode
  children: (session: AuthSessionDTO, client: AdminClient, expired: () => void, signOut: () => Promise<void>) => ReactNode
  /** Shown while the session is being checked, in place of the spinner: usually the skeleton of the page behind the gate. */
  fallback?: ReactNode
  /** Called once with the transport, before the session check answers, for work that can start alongside it. */
  onClient?: (client: AdminClient) => void
}) {
  const [state, setState] = useState<State>({ kind: 'loading' })
  const request = useRef<AbortController | null>(null)
  const transport = useRef<AdminClient | null>(null)
  const signedOut = useRef(false)
  const early = useRef(onClient); early.current = onClient
  const refresh = useCallback(async () => {
    request.current?.abort()
    const controller = new AbortController(); request.current = controller
    let client = transport.current
    if (!client) { client = transport.current = createAdminClient(window.location.origin); early.current?.(client) }
    try {
      const session = await client.getSession(controller.signal)
      // Revalidation runs every minute and on focus. The same person keeps the same
      // state object, so nothing behind the gate re-renders for an unchanged session.
      if (!controller.signal.aborted) setState(old => old.kind === 'ready' && old.client === client && sameUser(old.session, session) ? old : { kind: 'ready', session, client: client! })
    } catch (error) {
      if (!controller.signal.aborted) setState({ kind: error instanceof AdminClientError && error.status === 401 ? 'anonymous' : 'error' })
    }
  }, [])
  const expired = useCallback(() => { request.current?.abort(); setState({ kind: 'anonymous' }) }, [])
  const signOut = useCallback(async () => {
    signedOut.current = true
    request.current?.abort()
    setState({ kind: 'signingOut' })
    try {
      const result = await authClient.signOut()
      setState({ kind: 'closed', error: Boolean(result.error) })
    } catch { setState({ kind: 'closed', error: true }) }
  }, [])
  useEffect(() => {
    void refresh()
    const revalidate = () => { if (!signedOut.current && document.visibilityState === 'visible') void refresh() }
    window.addEventListener('focus', revalidate)
    const timer = window.setInterval(revalidate, 60_000)
    return () => { request.current?.abort(); window.removeEventListener('focus', revalidate); window.clearInterval(timer) }
  }, [refresh])
  if (state.kind === 'loading') return fallback ?? frame(<div className="account-loading"><Loading label="Checking your session…" /></div>)
  if (state.kind === 'signingOut') return frame(<div className="account-loading"><Loading label="Signing out…" /></div>)
  if (state.kind === 'error') return frame(<section className="admin-login"><h1>Unable to check your session.</h1><p>Your private notes and settings are hidden until the connection returns.</p><button type="button" className="button" onClick={() => void refresh()}>Try again</button></section>)
  if (state.kind === 'ready') return <>{children(state.session, state.client, expired, signOut)}</>
  if (state.kind === 'closed' && state.error) return frame(<section className="admin-login"><h1>Your private view is closed.</h1><Notice error>The server could not finish signing out. Retry to end this browser session.</Notice><button type="button" className="button" onClick={() => void signOut()}>Retry sign out</button></section>)
  // Signed out: the sign-in page, which offers every way of signing in this server has turned on, and comes back here.
  return frame(<ToSignIn />)
}

/** The sign-in page's address for someone who was on `location`, so that signing in brings them back to
 * exactly that address: its query is carried whole, because a page reached while connecting an app holds
 * that app's signed request there. */
export function signInPath(location: { pathname: string; search: string; hash: string }): string {
  const here = `${location.pathname}${location.search}${location.hash}`
  return here === '/' || here === '/notes' || location.pathname === '/login' ? '/login' : `/login?return=${encodeURIComponent(here)}`
}

function ToSignIn() {
  useEffect(() => { window.location.replace(signInPath(window.location)) }, [])
  return <div className="account-loading"><Loading label="Opening sign-in…" /></div>
}

function sameUser(a: AuthSessionDTO, b: AuthSessionDTO) {
  return a.user.id === b.user.id && a.user.role === b.user.role && a.user.name === b.user.name && a.user.email === b.user.email
}
