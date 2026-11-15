import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react'
import type { AuthSessionDTO } from '@meeting-notes/contracts'
import { authClient, signInProblem } from '../../lib/auth-client'
import { AdminClientError, createAdminClient, type AdminClient } from '../../lib/admin-client'
import { Loading, Notice } from '../admin/shared'
import { safeOAuthResume, signedOAuthQuery } from '../../lib/mcp-state'
import appIcon from '../../assets/app-icon.svg?url'

type State = { kind: 'loading' } | { kind: 'anonymous' } | { kind: 'error' } | { kind: 'signingOut' }
  | { kind: 'closed'; error: boolean } | { kind: 'ready'; session: AuthSessionDTO; client: AdminClient }

export function SessionGate({ children, fallback, onClient }: {
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
  if (state.kind === 'loading') return fallback ?? <div className="account-loading"><Loading label="Checking your session…" /></div>
  if (state.kind === 'signingOut') return <div className="account-loading"><Loading label="Signing out…" /></div>
  if (state.kind === 'error') return <section className="admin-login"><h1>Unable to check your session.</h1><p>Your private notes and settings are hidden until the connection returns.</p><button type="button" className="button" onClick={() => void refresh()}>Try again</button></section>
  if (state.kind === 'ready') return <>{children(state.session, state.client, expired, signOut)}</>
  if (state.kind === 'closed' && state.error) return <section className="admin-login"><h1>Your private view is closed.</h1><Notice error>The server could not finish signing out. Retry to end this browser session.</Notice><button type="button" className="button" onClick={() => void signOut()}>Retry sign out</button></section>
  return <Login signedOut={state.kind === 'closed'} connected={async () => { signedOut.current = false; await refresh() }} />
}

function sameUser(a: AuthSessionDTO, b: AuthSessionDTO) {
  return a.user.id === b.user.id && a.user.role === b.user.role && a.user.name === b.user.name && a.user.email === b.user.email
}

export function Login({ connected, signedOut = false }: { connected: () => Promise<void>; signedOut?: boolean }) {
  const [email, setEmail] = useState('')
  const [password, setPassword] = useState('')
  const [pending, setPending] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const request = useRef<AbortController | null>(null)
  useEffect(() => () => request.current?.abort(), [])
  async function login() {
    if (pending) return
    const controller = new AbortController(); request.current = controller
    setPending(true); setError(null)
    const submittedPassword = password; setPassword('')
    try {
      const result = await authClient.signIn.email({ email: email.trim(), password: submittedPassword, rememberMe: false }, { signal: controller.signal })
      if (controller.signal.aborted) return
      if (result.error) { setError(signInProblem(result.error.status)); requestAnimationFrame(() => document.getElementById('account-password')?.focus()) }
      else {
        // The provider resumes a signed OAuth request after password sign-in.
        // Only our own authorization pages can be entered before explicit consent.
        const response = result.data as { redirect?: boolean; url?: string } | null
        if (signedOAuthQuery(window.location.search) && response?.redirect) {
          const resume = safeOAuthResume(response.url, window.location.origin)
          if (!resume) { setError('This connection could not be resumed. Start again from your app.'); return }
          window.location.assign(resume)
        } else await connected()
      }
    } catch { if (!controller.signal.aborted) setError('Unable to reach the instance. Please try again.') }
    finally { if (!controller.signal.aborted) setPending(false) }
  }
  return <section className="auth">
    <div className="card auth-card rise-in">
      <img className="auth-icon" src={appIcon} width="68" height="68" alt="" />
      <p className="eyebrow">Your meeting notes</p><h1>Welcome back.</h1>
      <p>Sign in to read your notes and manage your account.</p>
      {signedOut && <Notice>You are signed out.</Notice>}
      <form onSubmit={event => { event.preventDefault(); void login() }}>
        <fieldset disabled={pending}>
          <label className="field" htmlFor="account-email">Email address<input id="account-email" name="email" type="email" value={email} autoComplete="username" required autoCapitalize="none" spellCheck={false} onChange={e => setEmail(e.target.value)} /></label>
          <label className="field" htmlFor="account-password">Password<input id="account-password" name="password" type="password" value={password} aria-invalid={error ? true : undefined} aria-describedby={error ? 'sign-in-problem' : undefined} autoComplete="current-password" required maxLength={128} onChange={e => setPassword(e.target.value)} /></label>
          {error && <div id="sign-in-problem"><Notice error>{error}</Notice></div>}
          {/* Always pressable: the fields are required, so the browser explains what is missing. */}
          <button className="button is-large" type="submit" disabled={pending}>{pending ? 'Signing in…' : 'Sign in'}</button>
        </fieldset>
      </form>
    </div>
    <p className="auth-footnote">Accounts are provided by your instance administrator. Shared links open without signing in.</p>
  </section>
}
