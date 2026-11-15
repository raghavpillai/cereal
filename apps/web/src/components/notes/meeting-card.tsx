import { useState } from 'react'
import type { MeetingContextDTO } from '@meeting-notes/contracts'
import { safeMeetingLink } from '../../lib/admin-state'
import { createdClock, listDayLabel, localDateKey, meetingPeople, participantName } from '../../lib/notes'
import { Icon } from '../kit/icon'

const SHOWN = 6

function when(meeting: MeetingContextDTO): string {
  const start = new Date(meeting.startAt), end = new Date(meeting.endAt)
  if (Number.isNaN(start.getTime())) return ''
  const day = listDayLabel(meeting.allDay ? meeting.startAt.slice(0, 10) : localDateKey(start), new Date())
  if (meeting.allDay || Number.isNaN(end.getTime())) return `${day} · All day`
  return `${day} · ${createdClock(meeting.startAt)} – ${createdClock(meeting.endAt)}`
}

/** Saved calendar context for a private note, shown from the note's details chip.
 * Invitees are context, not transcript speakers. */
export function MeetingCard({ meeting }: { meeting: MeetingContextDTO }) {
  const [all, setAll] = useState(false)
  const people = meetingPeople(meeting.participants), visible = all ? people : people.slice(0, SHOWN)
  const eventLink = safeMeetingLink(meeting.eventUrl), meetingLink = safeMeetingLink(meeting.meetingUrl)
  const organizer = meeting.organizer && !people.some(person => person.id === meeting.organizer!.id) ? meeting.organizer : null
  return <section className="meeting-card" aria-label="Meeting details">
    <h2>{meeting.title.trim() || 'Untitled event'}</h2>
    <p className="app-small">{when(meeting)}<br />{meeting.calendarName}{meeting.location?.trim() && <><br />{meeting.location.trim()}</>}{organizer && <><br />Organized by {participantName(organizer)}</>}</p>
    {people.length > 0 && <>
      <div className="app-separator is-roomy" role="separator" />
      <p className="app-small meeting-people-title">Invited · {people.length}</p>
      <ul className="meeting-people">{visible.map(person => {
        const name = participantName(person)
        return <li key={person.id}>
          <span className="share-avatar" aria-hidden="true">{[...name][0]?.toLocaleUpperCase() ?? '?'}</span>
          <span className="meeting-person"><span>{name}{person.isSelf ? ' (you)' : ''}</span>{(person.isOrganizer || (person.displayName && person.email)) && <span className="app-small">{[person.isOrganizer ? 'Organizer' : '', person.displayName ? person.email ?? '' : ''].filter(Boolean).join(' · ')}</span>}</span>
        </li>
      })}</ul>
      {people.length > SHOWN && <button type="button" className="app-menu-row" aria-expanded={all} onClick={() => setAll(value => !value)}><span>{all ? 'Show fewer' : `Show all ${people.length}`}</span></button>}
    </>}
    {(eventLink || meetingLink) && <div className="meeting-links">
      {eventLink && <a href={eventLink} className="app-pill" target="_blank" rel="noopener noreferrer" referrerPolicy="no-referrer"><span>Calendar event</span></a>}
      {meetingLink && <a href={meetingLink} className="app-pill" target="_blank" rel="noopener noreferrer" referrerPolicy="no-referrer"><span>Meeting link</span></a>}
    </div>}
    <p className="app-small meeting-note">Invitees come from the calendar. They are not matched to voices in the transcript.</p>
  </section>
}
