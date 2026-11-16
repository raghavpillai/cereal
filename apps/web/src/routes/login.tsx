import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react'
import { createFileRoute } from '@tanstack/react-router'
import type { AuthSessionDTO } from '@meeting-notes/contracts'
import { readSession, rememberSetup } from '../lib/session-probe'
import { AppHandoffError, appCallbackURL, appSignInPath, appSignInRequest, authClient, fetchAuthMethods, googleStartURL, requestAppHandoff, safeReturnPath, signInErrorMessage, signInProblem, type AppSignInRequest, type AuthMethods } from '../lib/auth-client'
import { setupLanding } from '../lib/onboarding'
import { Loading, Notice } from '../components/admin/shared'
import { AccountPicture } from '../components/account/account-picture'
import { TransportWarning } from '../components/kit/transport-notice'
import appIcon from '../assets/app-icon.svg?url'
import '../styles/login.css'

export const Route = createFileRoute('/login')({
  ssr: false,
  head: () => ({ meta: [{ title: 'Sign in · Cereal' }] }),
  component: SignInPage,
})

/** Set just before leaving for Google from an app sign-in, so the return trip finishes it without asking again. */
const APP_SIGN_IN_FLAG = 'cereal-app-sign-in'
type Stage =
  | { kind: 'loading' } | { kind: 'unreachable' }
  | { kind: 'form'; methods: AuthMethods; signedOut?: boolean }
  /** Signed in before arriving from an app: the person confirms which account the app gets. */
  | { kind: 'confirm'; methods: AuthMethods; session: AuthSessionDTO }
  | { kind: 'opening'; session: AuthSessionDTO | null }
  | { kind: 'opened'; session: AuthSessionDTO | null }
  | { kind: 'leaving' }

function SignInPage() {
  const [stage, setStage] = useState<Stage>({ kind: 'loading' })
  const [problem, setProblem] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  // Read once: the page never changes its own query string.
  const [query] = useState(() => {
    const search = typeof window === 'undefined' ? '' : window.location.search, params = new URLSearchParams(search)
    return { app: appSignInRequest(search), wantsApp: params.get('app') === '1', error: signInErrorMessage(params.get('error')), returnPath: typeof window === 'undefined' ? null : safeReturnPath(params.get('return'), window.location.origin) }
  })
  const request = useRef<AbortController | null>(null)
  const { app } = query

  /** Asks for a single-use code and hands it to the app. Called again by "Open Cereal": every press gets a fresh code. */
  const openApp = useCallback(async (target: AppSignInRequest, session: AuthSessionDTO | null) => {
    request.current?.abort()
    const controller = new AbortController(); request.current = controller
    setProblem(null); setBusy(true); setStage({ kind: 'opening', session })
    try {
      const code = await requestAppHandoff(target.challenge, controller.signal)
      if (controller.signal.aborted) return
      setStage({ kind: 'opened', session })
      window.location.assign(appCallbackURL(code, target.state, window.location.origin))
    } catch (error) {
      if (controller.signal.aborted) return
      setProblem(error instanceof AppHandoffError ? error.message : 'Cereal could not be opened. Please try again.')
      setStage({ kind: 'opened', session })
    } finally { if (!controller.signal.aborted) setBusy(false) }
  }, [])

  /** On to where the person was going. With nowhere in particular to go, an owner or administrator of a server
   * whose setup is unfinished goes to setup; everyone else goes to their notes. */
  const leave = useCallback(async (session?: AuthSessionDTO | null) => {
    setStage({ kind: 'leaving' })
    let to = query.returnPath
    if (!to) {
      const { createAdminClient } = await import('../lib/admin-client')
      const client = createAdminClient(window.location.origin)
      const known = session ?? await client.getSession().catch(() => null)
      to = known ? await setupLanding(known, () => client.getSettings()) : null
      if (to) rememberSetup(true)
    }
    window.location.replace(to ?? '/notes')
  }, [query.returnPath])

  const load = useCallback(async () => {
    request.current?.abort()
    const controller = new AbortController(); request.current = controller
    setStage({ kind: 'loading' })
    try {
      const [methods, session] = await Promise.all([
        fetchAuthMethods(controller.signal),
        readSession(controller.signal),
      ])
      if (controller.signal.aborted) return
      // Nobody can sign in to a server that has not been claimed: its sign-in page is the first step of setup.
      // …unless they asked for sign-in by name: a server set up before there was a claim step has accounts
      // whose owner never made a password, and those people must still be able to get in.
      if (methods.setupRequired && !session && !new URLSearchParams(window.location.search).has('signin')) { setStage({ kind: 'leaving' }); rememberSetup(true); window.location.replace('/admin'); return }
      if (!session || (query.wantsApp && !app)) {
        // Nothing is in progress any more: a later visit with a session asks before handing it to an app.
        try { sessionStorage.removeItem(APP_SIGN_IN_FLAG) } catch { /* nothing to clear */ }
        setStage({ kind: 'form', methods }); return
      }
      if (!app) { void leave(session); return }
      // Back from Google in the sign-in this page started: that was the confirmation.
      let fresh = false
      try { fresh = sessionStorage.getItem(APP_SIGN_IN_FLAG) === app.state; sessionStorage.removeItem(APP_SIGN_IN_FLAG) } catch { /* then the person confirms */ }
      if (fresh) void openApp(app, session); else setStage({ kind: 'confirm', methods, session })
    } catch { if (!controller.signal.aborted) setStage({ kind: 'unreachable' }) }
  }, [app, leave, openApp, query.wantsApp])
  useEffect(() => { void load(); return () => request.current?.abort() }, [load])

  async function signedIn() {
    if (app) {
      const session = await readSession().catch(() => null)
      await openApp(app, session)
    } else await leave()
  }
  async function useAnotherAccount(methods: AuthMethods) {
    setBusy(true); setProblem(null)
    try { const result = await authClient.signOut(); if (result.error) throw new Error('sign out failed'); setStage({ kind: 'form', methods, signedOut: true }) }
    catch { setProblem('The server could not finish signing out. Please try again.') }
    finally { setBusy(false) }
  }

  if (stage.kind === 'loading' || stage.kind === 'leaving') return <div className="account-loading"><Loading label={stage.kind === 'leaving' ? 'Opening Cereal…' : 'Checking your session…'} /></div>
  if (stage.kind === 'unreachable') return <Shell title="Unable to reach the server." lead="Check your connection and try again."><button type="button" className="button is-large" onClick={() => void load()}>Try again</button></Shell>
  if (query.wantsApp && !app) return <Shell title="This sign-in link is not valid." lead="Go back to the Cereal app and start signing in again." />

  if (stage.kind === 'confirm') return <Shell eyebrow="Cereal app" title="Continue to the Cereal app?" lead="The app on this device will be signed in to your account.">
    <div className="sign-in-account"><span className="sign-in-avatar" aria-hidden="true"><AccountPicture image={stage.session.user.image} name={stage.session.user.name.trim() || stage.session.user.email} /></span><span><strong>{stage.session.user.name}</strong><span>{stage.session.user.email}</span></span></div>
    {problem && <Notice error>{problem}</Notice>}
    <button type="button" className="button is-large" disabled={busy} onClick={() => void openApp(app!, stage.session)}>Continue as {stage.session.user.name.trim() || stage.session.user.email}</button>
    <button type="button" className="sign-in-secondary" disabled={busy} onClick={() => void useAnotherAccount(stage.methods)}>Use a different account</button>
  </Shell>

  if (stage.kind === 'opening' || stage.kind === 'opened') return <Shell eyebrow="Cereal app" title={stage.kind === 'opening' ? 'Opening Cereal…' : 'You can return to Cereal.'} lead={stage.kind === 'opening' ? 'One moment.' : stage.session ? `You are signed in as ${stage.session.user.email}. If the app did not open, use the button below.` : 'If the app did not open, use the button below.'}>
    {problem && <Notice error>{problem}</Notice>}
    {stage.kind === 'opened' && <button type="button" className="button is-large" disabled={busy} onClick={() => void openApp(app!, stage.session)}>Open Cereal</button>}
  </Shell>

  const { methods } = stage
  return <Shell eyebrow={app ? 'Cereal app' : 'Your meeting notes'} title={app ? 'Sign in to continue.' : 'Welcome back.'} lead={app ? `Sign in to ${methods.instanceName} to continue in the Cereal app.` : 'Sign in to read your notes.'}
    footnote="Accounts are provided by your instance administrator." download={!app}>
    <TransportWarning transport={methods.transport} />
    {stage.signedOut && <Notice>You are signed out.</Notice>}
    {query.error && !problem && <Notice error>{query.error}</Notice>}
    {methods.google && <a className="sign-in-google" href={googleStartURL(app ? appSignInPath(app) : query.returnPath)} onClick={() => { if (app) try { sessionStorage.setItem(APP_SIGN_IN_FLAG, app.state) } catch { /* the return trip asks for confirmation instead */ } }}><GoogleMark />Continue with Google</a>}
    {methods.google && methods.password && <div className="sign-in-divider" role="separator"><span>or</span></div>}
    {methods.password && <PasswordForm problem={problem} setProblem={setProblem} done={signedIn} />}
    {!methods.google && !methods.password && <Notice error>Sign-in is not set up on this server. Ask your administrator.</Notice>}
  </Shell>
}

function Shell({ eyebrow, title, lead, footnote, download = false, children }: { eyebrow?: string; title: string; lead?: string; footnote?: string; /** Offer the apps: only where someone is signing in on the web, not on behalf of an app they already have. */ download?: boolean; children?: ReactNode }) {
  return <section className="auth sign-in">
    <div className="card auth-card rise-in">
      <img className="auth-icon" src={appIcon} width="68" height="68" alt="" />
      {eyebrow && <p className="eyebrow">{eyebrow}</p>}<h1>{title}</h1>
      {lead && <p>{lead}</p>}
      <div className="sign-in-body">{children}</div>
    </div>
    {footnote && <p className="auth-footnote">{footnote}</p>}
    {download && <a className="sign-in-download" href="/download">Get the Cereal app</a>}
  </section>
}

function PasswordForm({ problem, setProblem, done }: { problem: string | null; setProblem: (message: string | null) => void; done: () => Promise<void> }) {
  const [email, setEmail] = useState('')
  const [password, setPassword] = useState('')
  const [pending, setPending] = useState(false)
  const request = useRef<AbortController | null>(null)
  useEffect(() => () => request.current?.abort(), [])
  async function submit() {
    if (pending) return
    const controller = new AbortController(); request.current = controller
    setPending(true); setProblem(null)
    // The password leaves component state as soon as it is sent.
    const submitted = password; setPassword('')
    try {
      const result = await authClient.signIn.email({ email: email.trim(), password: submitted, rememberMe: false }, { signal: controller.signal })
      if (controller.signal.aborted) return
      if (result.error) {
        setProblem(result.error.status === 403 ? 'Email and password sign-in is turned off on this server.' : signInProblem(result.error.status))
        requestAnimationFrame(() => document.getElementById('account-password')?.focus())
      } else await done()
    } catch { if (!controller.signal.aborted) setProblem('Unable to reach the server. Please try again.') }
    finally { if (!controller.signal.aborted) setPending(false) }
  }
  return <form onSubmit={event => { event.preventDefault(); void submit() }}>
    <fieldset disabled={pending}>
      <label className="field" htmlFor="account-email">Email address<input id="account-email" name="email" type="email" value={email} autoComplete="username" required autoCapitalize="none" spellCheck={false} onChange={event => setEmail(event.target.value)} /></label>
      <label className="field" htmlFor="account-password">Password<input id="account-password" name="password" type="password" value={password} aria-invalid={problem ? true : undefined} aria-describedby={problem ? 'sign-in-problem' : undefined} autoComplete="current-password" required maxLength={128} onChange={event => setPassword(event.target.value)} /></label>
      {problem && <div id="sign-in-problem"><Notice error>{problem}</Notice></div>}
      <button className="button is-large" type="submit" disabled={pending}>{pending ? 'Signing in…' : 'Sign in'}</button>
    </fieldset>
  </form>
}

/** Google's "G", as its sign-in button guidelines ask for. Drawn inline: nothing is fetched from Google to show the page. */
function GoogleMark() {
  return <svg viewBox="0 0 18 18" width="18" height="18" aria-hidden="true">
    <path fill="#4285F4" d="M17.64 9.2c0-.64-.06-1.25-.16-1.84H9v3.48h4.84a4.14 4.14 0 0 1-1.8 2.72v2.26h2.92c1.7-1.57 2.68-3.88 2.68-6.62Z" />
    <path fill="#34A853" d="M9 18c2.43 0 4.47-.8 5.96-2.18l-2.92-2.26c-.8.54-1.84.86-3.04.86-2.34 0-4.32-1.58-5.03-3.7H.96v2.33A9 9 0 0 0 9 18Z" />
    <path fill="#FBBC05" d="M3.97 10.72A5.4 5.4 0 0 1 3.68 9c0-.6.1-1.18.29-1.72V4.95H.96A9 9 0 0 0 0 9c0 1.45.35 2.83.96 4.05l3.01-2.33Z" />
    <path fill="#EA4335" d="M9 3.58c1.32 0 2.5.45 3.44 1.35l2.58-2.59C13.46.89 11.43 0 9 0A9 9 0 0 0 .96 4.95l3.01 2.33C4.68 5.16 6.66 3.58 9 3.58Z" />
  </svg>
}
