import { useEffect, useRef, useState } from 'react'
import type { AuthSessionDTO } from '@meeting-notes/contracts'
import { authClient } from '../../lib/auth-client'
import type { AdminClient } from '../../lib/admin-client'
import { Badge, Notice } from '../admin/shared'
import { Segmented } from '../kit/segmented'
import { AccountMenu } from './account-menu'
import { SessionGate } from './session-gate'
import { GoogleCalendars } from './google-calendars'
import { Workspaces } from './workspaces'
import { accountSection, type AccountSection } from '../../lib/workspace-state'

const SECTIONS: readonly { value: AccountSection; label: string }[] = [
  { value: 'account', label: 'Account' }, { value: 'calendars', label: 'Calendars' }, { value: 'workspaces', label: 'Workspaces' },
]

export function AccountPortal() {
  return <div className="admin-page is-account"><SessionGate>{(session, client, expired, signOut) =>
    <Account key={session.user.id} session={session} client={client} expired={expired} signOut={signOut} />
  }</SessionGate></div>
}

function Account({ session, client, expired, signOut }: { session: AuthSessionDTO; client: AdminClient; expired: () => void; signOut: () => Promise<void> }) {
  const [tab, setTab] = useState<AccountSection>(() => accountSection(typeof window === 'undefined' ? '' : window.location.search))
  // The chosen section lives in the address, so a reload or a shared link opens the same one.
  function choose(next: AccountSection) {
    setTab(next)
    const url = new URL(window.location.href)
    if (next === 'account') url.searchParams.delete('section'); else url.searchParams.set('section', next)
    window.history.replaceState(window.history.state, '', `${url.pathname}${url.search}${url.hash}`)
  }
  const role = session.user.role === 'owner' ? 'Owner' : session.user.role === 'admin' ? 'Administrator' : 'Member'
  return <>
    <AccountMenu session={session} signOut={signOut} />
    <div className="admin-heading"><div><p className="eyebrow">Your account</p><h1>{session.user.name.trim() || 'Your account'}</h1><p>{session.user.email}</p></div><div className="admin-account"><Badge>{role}</Badge></div></div>
    <Segmented className="account-sections" label="Account sections" options={SECTIONS} value={tab} onChange={choose} />
    {tab === 'workspaces' ? <Workspaces session={session} client={client} expired={expired} /> : tab === 'calendars' ? <GoogleCalendars client={client} expired={expired} /> : <PasswordSettings email={session.user.email} expired={expired} signOut={signOut} />}
    <p className="admin-page-footnote">Notes are read in the browser and written in the desktop and iPhone apps. Shared links stay read-only and need no account.</p>
  </>
}

function PasswordSettings({ email, expired, signOut }: { email: string; expired: () => void; signOut: () => Promise<void> }) {
  const [current, setCurrent] = useState(''), [next, setNext] = useState(''), [confirmation, setConfirmation] = useState('')
  const [busy, setBusy] = useState(false), [error, setError] = useState<string | null>(null), [saved, setSaved] = useState(false)
  const request = useRef<AbortController | null>(null)
  useEffect(() => () => request.current?.abort(), [])
  async function changePassword() {
    if (busy || next.length < 12 || next !== confirmation) return
    const controller = new AbortController(); request.current = controller
    const currentPassword = current, newPassword = next
    setCurrent(''); setNext(''); setConfirmation(''); setBusy(true); setError(null); setSaved(false)
    try {
      const result = await authClient.changePassword({ currentPassword, newPassword, revokeOtherSessions: true }, { signal: controller.signal })
      if (controller.signal.aborted) return
      if (result.error?.status === 401) expired()
      else if (result.error) setError('Your password could not be changed. Check the current password and try again.')
      else setSaved(true)
    } catch { if (!controller.signal.aborted) setError('Unable to update your password. Please try again.') }
    finally { if (!controller.signal.aborted) setBusy(false) }
  }
  return <>
    <section className="admin-card account-password"><div className="admin-section-title"><div><h2>Password</h2><p>Changing it signs out your other sessions.</p></div></div>
      <form onSubmit={event => { event.preventDefault(); void changePassword() }}><fieldset className="admin-fieldset" disabled={busy}>
        <input type="email" name="username" value={email} autoComplete="username" readOnly hidden />
        <label>Current password<input type="password" name="currentPassword" value={current} autoComplete="current-password" required maxLength={128} onChange={e => setCurrent(e.target.value)} /></label>
        <label>New password<input type="password" name="newPassword" value={next} autoComplete="new-password" required minLength={12} maxLength={128} onChange={e => setNext(e.target.value)} /><span className="admin-hint">At least 12 characters.</span></label>
        <label>Confirm new password<input type="password" name="confirmPassword" aria-invalid={confirmation && next !== confirmation ? true : undefined} aria-describedby={confirmation && next !== confirmation ? 'password-mismatch' : undefined} value={confirmation} autoComplete="new-password" required minLength={12} maxLength={128} onChange={e => setConfirmation(e.target.value)} /></label>
        {confirmation && next !== confirmation && <p className="admin-inline-error" id="password-mismatch" role="alert">The new passwords must match.</p>}{error && <Notice error>{error}</Notice>}{saved && <Notice>Password changed. Other sessions have been signed out.</Notice>}
        <button type="submit" className="button" disabled={busy}>{busy ? 'Updating…' : 'Change password'}</button>
      </fieldset></form>
    </section>
    <section className="admin-card account-password"><div className="admin-section-title"><div><h2>This browser</h2><p>Signed in as {email}.</p></div><button type="button" className="admin-secondary-button" onClick={() => void signOut()}>Sign out</button></div></section>
  </>
}
