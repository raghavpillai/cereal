import { useCallback, useEffect, useState } from 'react'
import type { GoogleConnectionDTO, GoogleStatusDTO } from '@meeting-notes/contracts'
import type { AdminClient } from '../../lib/admin-client'
import { googleAuthorizationURL } from '../../lib/google-state'
import { Badge, Empty, Loading, Modal, Notice, timestamp, useAdminTask, useResource } from '../admin/shared'

/** What to say after "Sync now". The server answers 200 even when Google refused: the failure is recorded on the
 * connection. So "Synced." is said only when the connection it returns synced cleanly; otherwise nothing, and the
 * card's own error notice says what happened. */
export function syncedNotice(status: GoogleStatusDTO, id: string): string | null {
  const after = status.connections.find(other => other.id === id)
  return after && !after.lastSyncError && after.status === 'connected' ? 'Synced.' : null
}

export function GoogleCalendars({ client, expired }: { client: AdminClient; expired: () => void }) {
  const load = useCallback((signal: AbortSignal) => client.getGoogleStatus(signal), [client])
  const resource = useResource(load, expired), tasks = useAdminTask(expired)
  const [notice, setNotice] = useState<string | null>(null), [navigationError, setNavigationError] = useState(false)
  useEffect(() => {
    const url = new URL(window.location.href), result = url.searchParams.get('google')
    if (result === 'connected') setNotice('Google account connected. Choose which calendars appear in your upcoming meetings.')
    else if (result === 'failed') setNotice('Google could not be connected. Check the instance configuration, then try again.')
    if (result) { url.searchParams.delete('google'); window.history.replaceState(window.history.state, '', `${url.pathname}${url.search}${url.hash}`) }
  }, [])
  async function connect() {
    setNavigationError(false)
    const result = await tasks.run('google:connect', signal => client.beginGoogleConnect(signal))
    if (!result.ok) return
    const url = googleAuthorizationURL(result.value.authorizationUrl, window.location.origin)
    if (!url) { setNavigationError(true); return }
    window.location.assign(url)
  }
  return <>
    <section className="admin-card"><div className="admin-section-title"><div><h2>Connected calendars</h2><p>Only you can see them. Up to five Google accounts.</p></div>{resource.data?.configured && !resource.data.needsHttps && <button type="button" className="button" disabled={tasks.pending.has('google:connect') || resource.data.connections.length >= 5} onClick={() => void connect()}>{tasks.pending.has('google:connect') ? 'Connecting…' : 'Connect Google'}</button>}</div>
      {notice && <Notice>{notice}</Notice>}{tasks.error && <Notice error>{tasks.error}</Notice>}{navigationError && <Notice error>Google could not be opened. Ask your administrator to check this server’s public address.</Notice>}
      {resource.error && <Notice error>{resource.error} <button type="button" className="admin-text-button" onClick={() => void resource.reload()}>Retry</button></Notice>}
      {resource.busy && !resource.data && <Loading label="Loading your calendars…" />}
      {resource.data?.needsHttps && <Empty title="Needs an HTTPS address">Google only connects to HTTPS servers, and this one uses plain HTTP.</Empty>}
      {resource.data?.configured === false && !resource.data.needsHttps && <Empty title="Not set up yet">An administrator needs to add a Google client in Admin first.</Empty>}
      {resource.data?.configured && !resource.data.needsHttps && <><p className="admin-hint google-contact-option">Attendee names are filled in from your Google contacts. Cereal only reads your calendar and contacts. It never changes them.</p>{resource.data.connections.length === 0 && <Empty title="No calendars yet">Connect a Google account, then pick which of its calendars to show.</Empty>}</>}
    </section>
    {resource.data?.connections.map(connection => <Connection key={connection.id} connection={connection} configured={resource.data!.configured} client={client} expired={expired} refresh={resource.reload} reconnect={connect} />)}
  </>
}

function Connection({ connection, configured, client, expired, refresh, reconnect }: { connection: GoogleConnectionDTO; configured: boolean; client: AdminClient; expired: () => void; refresh: () => Promise<void>; reconnect: () => Promise<void> }) {
  const load = useCallback((signal: AbortSignal) => client.listGoogleCalendars(connection.id, signal), [client, connection.id])
  const resource = useResource(load, expired), tasks = useAdminTask(expired)
  const [confirm, setConfirm] = useState(false), [notice, setNotice] = useState<string | null>(null)
  async function sync() {
    setNotice(null)
    const result = await tasks.run('sync', signal => client.syncGoogleConnection(connection.id, signal))
    if (!result.ok) return
    setNotice(syncedNotice(result.value, connection.id))
    await refresh(); await resource.reload()
  }
  async function select(id: string, enabled: boolean) {
    const result = await tasks.run(id, signal => client.updateGoogleCalendar(id, enabled, signal))
    if (result.ok) { setNotice(enabled ? 'Calendar shown in your meetings.' : 'Calendar hidden from your meetings.'); await resource.reload(); await refresh() }
  }
  async function disconnect() {
    const result = await tasks.run('disconnect', signal => client.deleteGoogleConnection(connection.id, signal))
    if (result.ok) { setConfirm(false); await refresh() }
  }
  return <section className="admin-card"><div className="admin-section-title"><div><h2>{connection.displayName || connection.email}</h2><p>{connection.email}</p></div><Badge good={configured && connection.status === 'connected'}>{!configured ? 'Not set up' : connection.status === 'connected' ? 'Connected' : 'Reconnect needed'}</Badge></div>
    <p className="admin-hint">Last sync: {timestamp(connection.lastSyncedAt)} · {connection.contactsEnabled ? 'Calendar and contacts' : 'Calendar only'}</p>
    {connection.lastSyncError && <Notice error>Couldn’t sync with Google. Your saved events are kept. Try again or reconnect.</Notice>}{notice && <Notice>{notice}</Notice>}{tasks.error && <Notice error>{tasks.error}</Notice>}
    <div className="google-connection-actions"><button type="button" className="admin-secondary-button" disabled={!configured || tasks.pending.has('sync')} onClick={() => void sync()}>{tasks.pending.has('sync') ? 'Syncing…' : 'Sync now'}</button>{connection.status === 'reconnect_required' && <button type="button" className="admin-text-button" disabled={!configured} onClick={() => void reconnect()}>Reconnect</button>}<button type="button" className="admin-text-button admin-danger-text" onClick={() => { tasks.clearError(); setConfirm(true) }}>Disconnect</button></div>
    {resource.error && <Notice error>{resource.error} <button type="button" className="admin-text-button" onClick={() => void resource.reload()}>Retry</button></Notice>}{resource.busy && !resource.data && <Loading label="Loading calendars…" />}
    <div className="google-calendar-options">{resource.data?.map(calendar => <label className="google-calendar-option" key={calendar.id}><span className="admin-calendar-marker" style={{ backgroundColor: calendar.color }} aria-hidden="true" /><span><strong>{calendar.name}</strong><span className="admin-hint">{calendar.primary ? 'Primary · ' : ''}{calendar.timeZone}</span></span><input type="checkbox" checked={calendar.selected} disabled={!configured || tasks.pending.has(calendar.id)} onChange={event => void select(calendar.id, event.target.checked)} aria-label={`Include ${calendar.name} in your upcoming meetings`} /></label>)}</div>
    {resource.data?.length === 0 && <Empty title="No calendars found">Sync again, or check this account’s permissions in Google.</Empty>}
    {confirm && <Modal title="Disconnect Google account?" busy={tasks.pending.has('disconnect')} close={() => setConfirm(false)}><div className="admin-dialog-body"><p>Remove <strong>{connection.email}</strong> and its saved events? Your meeting notes are kept.</p>{tasks.error && <Notice error>{tasks.error}</Notice>}</div><div className="admin-dialog-actions"><button type="button" className="admin-secondary-button" disabled={tasks.pending.has('disconnect')} onClick={() => setConfirm(false)}>Keep connection</button><button type="button" className="admin-danger-button" disabled={tasks.pending.has('disconnect')} onClick={() => void disconnect()}>{tasks.pending.has('disconnect') ? 'Disconnecting…' : 'Disconnect'}</button></div></Modal>}
  </section>
}
