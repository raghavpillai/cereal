import { useState } from 'react'
import type { CalendarDTO } from '@meeting-notes/contracts'
import type { AdminClient } from '../../lib/admin-client'
import { calendarDraft, calendarPatch, calendarProblem, type CalendarDraft } from '../../lib/admin-state'
import { Badge, Empty, Loading, Modal, Notice, timestamp, useAdminTask } from './shared'

export function CalendarsPanel({ client, calendars, loading, error, refresh, changed, unauthorized, refreshMinutes }: {
  client: AdminClient; calendars: readonly CalendarDTO[] | null; loading: boolean; error: string | null;
  refresh: () => Promise<void>; changed: () => void; unauthorized: () => void; refreshMinutes: number
}) {
  const [editor, setEditor] = useState<{ calendar?: CalendarDTO } | null>(null)
  const [disconnect, setDisconnect] = useState<CalendarDTO | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const tasks = useAdminTask(unauthorized)
  async function sync(calendar: CalendarDTO) {
    const result = await tasks.run(`calendar:${calendar.id}`, signal => client.syncCalendar(calendar.id, signal))
    if (!result.ok) return
    setNotice(result.value.lastSyncError ? 'Calendar linked, but its events could not be refreshed. Check its subscription or try again.' : 'Calendar events are up to date.')
    await refresh(); changed()
  }
  async function save(draft: CalendarDraft, saved?: CalendarDTO) {
    const result = await tasks.run(saved ? `calendar:${saved.id}` : 'calendar:create', signal => saved
      ? client.updateCalendar(saved.id, calendarPatch(draft, saved), signal)
      : client.createCalendar({ name: draft.name.trim(), url: draft.url.trim(), color: draft.color, enabled: draft.enabled }, signal))
    if (!result.ok) return
    setEditor(null); setNotice(saved ? 'Calendar settings saved.' : result.value.lastSyncError ? 'Calendar linked, but its events could not be refreshed. Check its subscription or try Sync now.' : 'Calendar linked and its upcoming events are ready.')
    await refresh(); changed()
  }
  async function toggle(calendar: CalendarDTO) {
    const result = await tasks.run(`calendar:${calendar.id}`, signal => client.updateCalendar(calendar.id, { enabled: !calendar.enabled }, signal))
    if (result.ok) { setNotice(calendar.enabled ? 'Calendar hidden from upcoming events.' : 'Calendar included in upcoming events.'); await refresh(); changed() }
  }
  async function remove(calendar: CalendarDTO) {
    const result = await tasks.run(`calendar:${calendar.id}`, signal => client.deleteCalendar(calendar.id, signal))
    if (result.ok) { setDisconnect(null); setNotice('Calendar disconnected. Existing meeting notes are kept.'); await refresh(); changed() }
  }
  return <>
    <section className="admin-card">
      <div className="admin-section-title"><div><h2>Linked calendars</h2><p>Combine work, personal and shared calendars without merging their sources.</p></div><button type="button" className="button" onClick={() => { tasks.clearError(); setNotice(null); setEditor({}) }}>＋ Link calendar</button></div>
      {notice && <Notice>{notice}</Notice>}
      {tasks.error && <Notice error>{tasks.error}</Notice>}
      {error && <Notice error>{error} <button type="button" className="admin-text-button" onClick={() => void refresh()}>Retry</button></Notice>}
      {loading && !calendars && <Loading label="Loading calendars…" />}
      {calendars?.length === 0 && <Empty title="Bring your calendars together">Link a read-only iCalendar subscription to show its events in the desktop and iOS apps. You can add more than one.</Empty>}
      <div className="admin-calendar-list">{calendars?.map(calendar => {
        const busy = tasks.pending.has(`calendar:${calendar.id}`)
        return <article className={`admin-calendar${!calendar.enabled ? ' is-paused' : ''}`} key={calendar.id} aria-busy={busy}>
          <div className="admin-calendar-marker" style={{ backgroundColor: calendar.color }} aria-hidden="true" />
          <div className="admin-calendar-details"><div className="admin-calendar-name"><h3>{calendar.name}</h3><Badge good={calendar.enabled}>{calendar.enabled ? 'Included' : 'Hidden'}</Badge></div><p>{calendar.sourceHost} <span aria-hidden="true">·</span> iCalendar subscription</p>
            <div className="admin-calendar-meta"><span>{calendar.eventCount} {calendar.eventCount === 1 ? 'event' : 'events'}</span><span>Last sync: {timestamp(calendar.lastSyncedAt)}</span></div>
            {calendar.lastSyncError && <p className="admin-inline-error">Events could not be refreshed. Check the feed access or retry sync.</p>}
          </div>
          <div className="admin-calendar-actions"><label className="admin-switch"><input type="checkbox" checked={calendar.enabled} disabled={busy} onChange={() => void toggle(calendar)} aria-label={`Include ${calendar.name} in upcoming events`} /><span aria-hidden="true" /></label>
            <button type="button" className="admin-secondary-button" disabled={busy} onClick={() => void sync(calendar)}>{busy ? 'Working…' : 'Sync now'}</button>
            <button type="button" className="admin-text-button" disabled={busy} onClick={() => { tasks.clearError(); setEditor({ calendar }) }}>Edit<span className="sr-only"> {calendar.name}</span></button>
            <button type="button" className="admin-text-button admin-danger-text" disabled={busy} onClick={() => { tasks.clearError(); setDisconnect(calendar) }}>Disconnect<span className="sr-only"> {calendar.name}</span></button>
          </div>
        </article>
      })}</div>
      {calendars && calendars.length > 0 && <p className="admin-hint admin-card-footnote">Enabled sources refresh every {refreshMinutes} minutes. Hiding a source removes it from Upcoming without deleting its connection.</p>}
    </section>
    <section className="admin-card admin-integration-note"><span className="admin-integration-icon" aria-hidden="true">↗</span><div><h3>Use a subscription from your calendar provider</h3><p>Google Calendar, Outlook and Apple Calendar can expose read-only iCalendar links. Account-based Google and Microsoft OAuth connections are not enabled in this instance. Subscription URLs stay private on the server.</p></div></section>
    {editor && <CalendarEditor key={editor.calendar?.id ?? 'new'} calendar={editor.calendar} busy={tasks.pending.has(editor.calendar ? `calendar:${editor.calendar.id}` : 'calendar:create')} error={tasks.error} save={draft => save(draft, editor.calendar)} close={() => setEditor(null)} />}
    {disconnect && <Modal title="Disconnect calendar?" busy={tasks.pending.has(`calendar:${disconnect.id}`)} close={() => setDisconnect(null)}>
      <div className="admin-dialog-body"><p>Remove <strong>{disconnect.name}</strong> and its upcoming events from this instance? Meeting notes you already created will stay.</p>{tasks.error && <Notice error>{tasks.error}</Notice>}</div>
      <div className="admin-dialog-actions"><button type="button" className="admin-secondary-button" disabled={tasks.pending.has(`calendar:${disconnect.id}`)} onClick={() => setDisconnect(null)}>Keep calendar</button><button type="button" className="admin-danger-button" disabled={tasks.pending.has(`calendar:${disconnect.id}`)} onClick={() => void remove(disconnect)}>{tasks.pending.has(`calendar:${disconnect.id}`) ? 'Disconnecting…' : 'Disconnect calendar'}</button></div>
    </Modal>}
  </>
}
function CalendarEditor({ calendar, busy, error, save, close }: { calendar?: CalendarDTO; busy: boolean; error: string | null; save: (draft: CalendarDraft) => Promise<void>; close: () => void }) {
  const [draft, setDraft] = useState(() => calendarDraft(calendar))
  const problem = calendarProblem(draft, Boolean(calendar))
  function change<K extends keyof CalendarDraft>(key: K, value: CalendarDraft[K]) { setDraft(old => ({ ...old, [key]: value })) }
  return <Modal title={calendar ? 'Edit calendar' : 'Link a calendar'} busy={busy} close={close}>
    <form onSubmit={event => { event.preventDefault(); if (!problem) void save(draft) }}>
      <fieldset disabled={busy} className="admin-fieldset admin-dialog-body">
        <label>Calendar name<input name="calendarName" value={draft.name} maxLength={200} required autoFocus onChange={e => change('name', e.target.value)} placeholder="Work calendar" /></label>
        <label>{calendar ? 'Replace subscription URL' : 'Private subscription URL'}<input type="password" name="calendarUrl" value={draft.url} required={!calendar} autoComplete="new-password" maxLength={4096} spellCheck={false} onChange={e => change('url', e.target.value)} placeholder={calendar ? 'Leave blank to keep the current subscription' : 'https://calendar.example.com/feed.ics'} /><span className="admin-hint">Paste an HTTPS or webcal iCalendar link. Treat private subscription links like passwords. The server never returns the saved URL.</span></label>
        <label className="admin-color-label">Calendar color<input type="color" name="calendarColor" value={draft.color} onChange={e => change('color', e.target.value)} /></label>
        <label className="admin-checkbox"><input type="checkbox" checked={draft.enabled} onChange={e => change('enabled', e.target.checked)} />Include this calendar in upcoming events</label>
        {error && <Notice error>{error}</Notice>}
        {problem && <p className="admin-hint">{problem}</p>}
      </fieldset>
      <div className="admin-dialog-actions"><button type="button" className="admin-secondary-button" disabled={busy} onClick={close}>Cancel</button><button type="submit" className="button" disabled={busy || Boolean(problem)}>{busy ? 'Saving…' : calendar ? 'Save calendar' : 'Link calendar'}</button></div>
    </form>
  </Modal>
}
