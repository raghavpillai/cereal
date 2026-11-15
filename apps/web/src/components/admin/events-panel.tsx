import { useState } from 'react'
import type { CalendarEventDTO } from '@meeting-notes/contracts'
import type { AdminClient } from '../../lib/admin-client'
import { eventDay, formatEventTime, safeMeetingLink } from '../../lib/admin-state'
import { Badge, Empty, Loading, Notice, useAdminTask } from './shared'

export function EventsPanel({ client, events, loading, error, start, days, changeStart, changeDays, refresh, unauthorized }: {
  client: AdminClient; events: readonly CalendarEventDTO[] | null; loading: boolean; error: string | null;
  start: string; days: number; changeStart: (date: string) => void; changeDays: (days: number) => void; refresh: () => Promise<void>; unauthorized: () => void
}) {
  const [notice, setNotice] = useState<string | null>(null)
  const tasks = useAdminTask(unauthorized)
  const grouped = new Map<string, CalendarEventDTO[]>()
  for (const event of events ?? []) { const day = eventDay(event); const list = grouped.get(day) ?? []; list.push(event); grouped.set(day, list) }
  async function prepare(event: CalendarEventDTO) {
    const result = await tasks.run(event.id, signal => client.prepareNote(event.id, signal))
    if (result.ok) { setNotice('Meeting note prepared. Open it in the desktop or iOS app to start recording.'); await refresh() }
  }
  return <section className="admin-card">
    <div className="admin-section-title"><div><h2>Coming up</h2><p>A preview of the enabled calendars shown in your native apps.</p></div><button type="button" className="admin-secondary-button" disabled={loading} onClick={() => void refresh()}>{loading ? 'Refreshing…' : 'Refresh events'}</button></div>
    <div className="admin-event-filters"><label>Starting from<input type="date" value={start} required onChange={e => { if (e.target.value) changeStart(e.target.value) }} /></label><label>Show<select value={days} onChange={e => changeDays(Number(e.target.value))}>{[1, 7, 14, 30, 90].map(value => <option key={value} value={value}>Next {value} {value === 1 ? 'day' : 'days'}</option>)}</select></label></div>
    <p className="admin-hint">Times use your local time zone. All-day events retain their calendar dates.</p>
    {notice && <Notice>{notice}</Notice>}{tasks.error && <Notice error>{tasks.error}</Notice>}
    {error && <Notice error>{error} <button type="button" className="admin-text-button" onClick={() => void refresh()}>Retry</button></Notice>}
    {loading && !events && <Loading label="Loading upcoming events…" />}
    {events?.length === 0 && <Empty title="Nothing coming up in this window">Check the date range, enable a linked calendar, or sync its latest events.</Empty>}
    <div className="admin-events">{[...grouped].sort(([a], [b]) => a.localeCompare(b)).map(([day, items]) => <section className="admin-event-day" key={day}>
      <h3>{new Intl.DateTimeFormat(undefined, { weekday: 'long', month: 'long', day: 'numeric' }).format(new Date(`${day}T12:00:00`))}</h3>
      {items.map(event => {
        const link = safeMeetingLink(event.meetingUrl)
        return <article className="admin-event" key={event.id}>
          <div className="admin-event-marker" style={{ backgroundColor: event.color }} aria-hidden="true" />
          <div className="admin-event-info"><h4>{event.title || 'Untitled event'}</h4><p><span>{formatEventTime(event)}</span><span className="admin-event-calendar">{event.calendarName}</span></p>{event.location && <p className="admin-hint">{event.location}</p>}</div>
          <div className="admin-event-actions">{link && <a href={link} className="admin-secondary-button" target="_blank" rel="noopener noreferrer" referrerPolicy="no-referrer">Join meeting<span className="sr-only">: {event.title}</span> ↗</a>}
            {event.noteId ? <Badge good>Note prepared</Badge> : <button type="button" className="admin-text-button" disabled={tasks.pending.has(event.id)} onClick={() => void prepare(event)}>{tasks.pending.has(event.id) ? 'Preparing…' : 'Prepare note'}<span className="sr-only"> for {event.title}</span></button>}
          </div>
        </article>
      })}
    </section>)}</div>
  </section>
}
