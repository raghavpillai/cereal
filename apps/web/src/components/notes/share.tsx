import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import type { AuthSessionDTO, ShareDTO } from '@meeting-notes/contracts'
import { stampLabel } from '../../lib/notes'
import { ShareError, activeShares, createSharesClient, shareChoice, type SharesClient } from '../../lib/shares'
import { Icon } from '../kit/icon'
import { Popover } from '../kit/popover'

type Busy = 'create' | 'revoke' | null

/** Sharing, as the desktop app's Share card does it: a link is a read-only snapshot
 * of the note's text, never its transcript, for people signed in to this server. Here the reader also chooses which text
 * goes into the link, and sees the links that exist and can revoke each one. */
export function ShareControl({ noteId, session, has, expired, client: given }: {
  noteId: string
  session: AuthSessionDTO
  has: { enhanced: boolean; body: boolean }
  expired: () => void
  /** For tests; the page talks to its own origin. */
  client?: SharesClient
}) {
  const client = useMemo(() => given ?? createSharesClient(window.location.origin), [given])
  const [open, setOpen] = useState(false)
  const [shares, setShares] = useState<readonly ShareDTO[] | null>(null)
  const [include, setInclude] = useState({ enhanced: true, body: true })
  const [busy, setBusy] = useState<Busy>(null), [error, setError] = useState<string | null>(null)
  const [made, setMade] = useState<{ url: string; copied: boolean } | null>(null)
  const [confirmed, setConfirmed] = useState(false)
  const alive = useRef(true), field = useRef<HTMLInputElement>(null)
  useEffect(() => { alive.current = true; return () => { alive.current = false } }, [])
  useEffect(() => { if (!confirmed) return; const timer = window.setTimeout(() => setConfirmed(false), 1200); return () => window.clearTimeout(timer) }, [confirmed])

  const fail = useCallback((problem: unknown) => {
    if (!alive.current) return
    if (problem instanceof ShareError && problem.status === 401) { expired(); return }
    setError(problem instanceof ShareError ? problem.message : 'The link could not be changed. Please try again.')
  }, [expired])
  const reload = useCallback(async () => {
    try { const list = await client.list(noteId); if (alive.current) setShares(list) } catch (problem) { fail(problem) }
  }, [client, noteId, fail])
  // The card always opens on the links as they are now.
  useEffect(() => { if (open) { setError(null); void reload() } }, [open, reload])

  /** Make a link and put it on the clipboard. A browser that refuses the clipboard still gets the link, selected, to copy by hand. */
  async function createLink(wanted: { enhanced: boolean; body: boolean }): Promise<boolean> {
    const choice = shareChoice(has, wanted)
    if (!choice) { setError('Choose what the link should include.'); return false }
    if (busy) return false
    setBusy('create'); setError(null)
    try {
      const { url } = await client.create(noteId, choice)
      let copied = false
      try { await navigator.clipboard.writeText(url); copied = true } catch { /* shown for copying by hand */ }
      if (!alive.current) return copied
      setMade({ url, copied })
      void reload()
      return copied
    } catch (problem) { fail(problem); return false } finally { if (alive.current) setBusy(null) }
  }
  async function revoke(ids: readonly string[]) {
    if (busy || !ids.length) return
    setBusy('revoke'); setError(null)
    try {
      for (const id of ids) await client.revoke(id)
      if (alive.current) setMade(null)
    } catch (problem) { fail(problem) } finally { if (alive.current) { setBusy(null); void reload() } }
  }
  /** The link half of the button: one press makes a link to everything the note has and copies it. */
  async function quickCopy() {
    const copied = await createLink({ enhanced: true, body: true })
    if (!alive.current) return
    if (copied) setConfirmed(true); else setOpen(true)
  }
  useEffect(() => { if (made && !made.copied && open) { field.current?.focus(); field.current?.select() } }, [made, open])

  const { user } = session, owner = user.name.trim() || 'Me', links = shares ? activeShares(shares) : []
  return <div className="share-split">
    <Popover label="Share" menu={false} className="share-card" buttonClassName="share-button" button={<><Icon name="lock-solid" size={13} /><span>Share</span></>} open={open} onOpenChange={setOpen}>
      {() => <>
        <p className="share-label">People with access</p>
        <div className="share-owner">
          <span className="share-avatar" aria-hidden="true">{[...owner][0]?.toLocaleUpperCase() ?? 'M'}</span>
          <span className="share-owner-name"><span>{owner} (you)</span><span className="app-small">{user.email}</span></span>
          <span className="app-small">Owner</span>
        </div>
        <div className="app-separator is-roomy" role="separator" />
        <div className="share-anyone">
          <Icon name="globe" size={15} />
          <span>People with the link</span>
          <button type="button" className="app-pill" disabled={busy !== null || !shareChoice(has, include)} onClick={() => void createLink(include)}><span>{busy === 'create' ? 'Copying…' : 'Copy link'}</span></button>
        </div>
        <p className="share-hint">Anyone signed in to this server who has the link can read it. Copying creates a read-only snapshot. Your transcript stays private.</p>
        <fieldset className="share-include" disabled={busy !== null}>
          <legend>The link includes</legend>
          <label><input type="checkbox" checked={has.enhanced && include.enhanced} disabled={!has.enhanced} onChange={event => setInclude(value => ({ ...value, enhanced: event.target.checked }))} />Generated notes</label>
          <label><input type="checkbox" checked={has.body && include.body} disabled={!has.body} onChange={event => setInclude(value => ({ ...value, body: event.target.checked }))} />My notes</label>
        </fieldset>
        {made && <div className="share-made">
          <input ref={field} type="text" readOnly value={made.url} aria-label="The new link" spellCheck={false} onFocus={event => event.currentTarget.select()} />
          <span role="status">{made.copied ? 'Link copied' : 'Copy this link'}</span>
        </div>}
        {error && <p className="share-error" role="alert">{error}</p>}
        <div className="app-separator" role="separator" />
        {shares === null && !error && <p className="share-links-note" role="status">Loading links…</p>}
        {links.length > 0 && <ul className="share-links" aria-label="Shared links">{links.map(share => <li key={share.id}>
          <Icon name="link-chain" size={14} />
          <span>Created {stampLabel(share.createdAt)}{share.expiresAt && <span className="app-small"> · until {stampLabel(share.expiresAt)}</span>}</span>
          <button type="button" className="share-revoke" disabled={busy !== null} onClick={() => void revoke([share.id])}>Revoke<span className="sr-only"> the link created {stampLabel(share.createdAt)}</span></button>
        </li>)}</ul>}
        {shares !== null && links.length === 0 && <p className="share-links-note">No shared links.</p>}
        <button type="button" className="app-menu-row" disabled={busy !== null || links.length === 0} onClick={() => void revoke(links.map(share => share.id))}><Icon name="lock" size={16} /><span>{busy === 'revoke' ? 'Revoking…' : 'Revoke shared links'}</span></button>
      </>}
    </Popover>
    <button type="button" className="share-link-button" aria-label="Copy a link to this note" title="Copy link" disabled={busy !== null} onClick={() => void quickCopy()}><Icon name={confirmed ? 'check' : 'link-chain'} size={14} /></button>
    <span className="sr-only" role="status">{confirmed ? 'Link copied' : ''}</span>
  </div>
}
