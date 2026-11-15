import { useCallback, useEffect, useState } from 'react'
import type { GoogleConnectionDTO } from '@meeting-notes/contracts'
import type { AdminClient } from '../../lib/admin-client'
import { googleAuthorizationURL } from '../../lib/google-state'
import { Badge, Empty, Loading, Modal, Notice, timestamp, useAdminTask, useResource } from '../admin/shared'

export function GoogleCalendars({ client, expired }: { client: AdminClient; expired: () => void }) {
  const load = useCallback((signal: AbortSignal) => client.getGoogleStatus(signal), [client])
  const resource = useResource(load, expired), tasks = useAdminTask(expired)
  const [contacts, setContacts] = useState(false), [notice, setNotice] = useState<string | null>(null), [navigationError, setNavigationError] = useState(false)
  useEffect(() => {
    const url = new URL(window.location.href), result = url.searchParams.get('google')
    if (result === 'connected') setNotice('Google account connected. Choose which calendars appear in your upcoming meetings.')
    else if (result === 'failed') setNotice('Google could not be connected. Check the instance configuration, then try again.')
    if (result) { url.searchParams.delete('google'); window.history.replaceState(window.history.state, '', `${url.pathname}${url.search}${url.hash}`) }
  }, [])
  async function connect() {
    setNavigationError(false)
    const result = await tasks.run('google:connect', signal => client.beginGoogleConnect(contacts, signal))
    if (!result.ok) return
    const url = googleAuthorizationURL(result.value.authorizationUrl, window.location.origin)
    if (!url) { setNavigationError(true); return }
    window.location.assign(url)
  }
  return <>
    <section className="admin-card"><div className="admin-section-title"><div><h2>Connected calendars</h2><p>Connect one or more Google accounts, then choose their calendars.</p></div>{resource.data?.configured && <button type="button" className="button" disabled={tasks.pending.has('google:connect') || resource.data.connections.length >= 5} onClick={() => void connect()}>{tasks.pending.has('google:connect') ? 'Connecting…' : 'Connect Google Calendar'}</button>}</div>
      <p className="admin-hint">These connections are private to your account. Administrator access does not share your personal calendars with other people.</p>
      {notice && <Notice>{notice}</Notice>}{tasks.error && <Notice error>{tasks.error}</Notice>}{navigationError && <Notice error>The server returned an invalid connection destination. Contact your instance administrator.</Notice>}
      {resource.error && <Notice error>{resource.error} <button type="button" className="admin-text-button" onClick={() => void resource.reload()}>Retry</button></Notice>}
      {resource.busy && !resource.data && <Loading label="Loading your calendar connections…" />}
      {resource.data?.configured === false && <Empty title="Google Calendar is not set up yet">An administrator of this server needs to add its Google client in Admin first. Connect Google Calendar becomes available after that.</Empty>}
      {resource.data?.configured && <><label className="admin-checkbox google-contact-option"><input type="checkbox" checked={contacts} disabled={tasks.pending.has('google:connect')} onChange={event => setContacts(event.target.checked)} />Also allow read-only contacts to fill missing attendee names.</label><p className="admin-hint">Calendar access is read-only. Contacts are optional, and a missing name stays blank unless an exact email match is available.</p>{resource.data.connections.length === 0 && <Empty title="Bring your meetings with you">Use Connect Google Calendar to authorize this instance. You can connect up to five accounts and select multiple calendars from each.</Empty>}</>}
    </section>
    {resource.data?.connections.map(connection => <Connection key={connection.id} connection={connection} configured={resource.data!.configured} client={client} expired={expired} refresh={resource.reload} reconnect={connect} />)}
  </>
}

function Connection({ connection, configured, client, expired, refresh, reconnect }: { connection: GoogleConnectionDTO; configured: boolean; client: AdminClient; expired: () => void; refresh: () => Promise<void>; reconnect: () => Promise<void> }) {
  const load = useCallback((signal: AbortSignal) => client.listGoogleCalendars(connection.id, signal), [client, connection.id])
  const resource = useResource(load, expired), tasks = useAdminTask(expired)
  const [confirm, setConfirm] = useState(false), [notice, setNotice] = useState<string | null>(null)
  async function sync() {
    const result = await tasks.run('sync', signal => client.syncGoogleConnection(connection.id, signal))
    if (result.ok) { setNotice('Calendar refresh finished. Check the connection status below.'); await refresh(); await resource.reload() }
  }
  async function select(id: string, enabled: boolean) {
    const result = await tasks.run(id, signal => client.updateGoogleCalendar(id, enabled, signal))
    if (result.ok) { setNotice(enabled ? 'Calendar included in your upcoming meetings.' : 'Calendar hidden from your upcoming meetings.'); await resource.reload(); await refresh() }
  }
  async function disconnect() {
    const result = await tasks.run('disconnect', signal => client.deleteGoogleConnection(connection.id, signal))
    if (result.ok) { setConfirm(false); await refresh() }
  }
  return <section className="admin-card"><div className="admin-section-title"><div><h2>{connection.displayName || connection.email}</h2><p>{connection.email}</p></div><Badge good={configured && connection.status === 'connected'}>{!configured ? 'Server configuration needed' : connection.status === 'connected' ? 'Connected' : 'Reconnect needed'}</Badge></div>
    <p className="admin-hint">Last sync: {timestamp(connection.lastSyncedAt)} · {connection.contactsEnabled ? 'Calendar and contacts' : 'Calendar only'}</p>
    {connection.lastSyncError && <Notice error>Google Calendar could not be refreshed. Your cached events are kept. Retry or reconnect the account.</Notice>}{notice && <Notice>{notice}</Notice>}{tasks.error && <Notice error>{tasks.error}</Notice>}
    <div className="google-connection-actions"><button type="button" className="admin-secondary-button" disabled={!configured || tasks.pending.has('sync')} onClick={() => void sync()}>{tasks.pending.has('sync') ? 'Syncing…' : 'Sync now'}</button>{connection.status === 'reconnect_required' && <button type="button" className="admin-text-button" disabled={!configured} onClick={() => void reconnect()}>Reconnect</button>}<button type="button" className="admin-text-button admin-danger-text" onClick={() => { tasks.clearError(); setConfirm(true) }}>Disconnect</button></div>
    {resource.error && <Notice error>{resource.error} <button type="button" className="admin-text-button" onClick={() => void resource.reload()}>Retry</button></Notice>}{resource.busy && !resource.data && <Loading label="Loading calendars…" />}
    <div className="google-calendar-options">{resource.data?.map(calendar => <label className="google-calendar-option" key={calendar.id}><span className="admin-calendar-marker" style={{ backgroundColor: calendar.color }} aria-hidden="true" /><span><strong>{calendar.name}</strong><span className="admin-hint">{calendar.primary ? 'Primary · ' : ''}{calendar.timeZone}</span></span><input type="checkbox" checked={calendar.selected} disabled={!configured || tasks.pending.has(calendar.id)} onChange={event => void select(calendar.id, event.target.checked)} aria-label={`Include ${calendar.name} in your upcoming meetings`} /></label>)}</div>
    {resource.data?.length === 0 && <Empty title="No readable calendars found">Sync again or check the Google account permissions.</Empty>}
    {confirm && <Modal title="Disconnect Google account?" busy={tasks.pending.has('disconnect')} close={() => setConfirm(false)}><div className="admin-dialog-body"><p>Remove <strong>{connection.email}</strong> and its cached calendar events from this account? Existing meeting notes and their saved context remain yours.</p>{tasks.error && <Notice error>{tasks.error}</Notice>}</div><div className="admin-dialog-actions"><button type="button" className="admin-secondary-button" disabled={tasks.pending.has('disconnect')} onClick={() => setConfirm(false)}>Keep connection</button><button type="button" className="admin-danger-button" disabled={tasks.pending.has('disconnect')} onClick={() => void disconnect()}>{tasks.pending.has('disconnect') ? 'Disconnecting…' : 'Disconnect Google'}</button></div></Modal>}
  </section>
}
