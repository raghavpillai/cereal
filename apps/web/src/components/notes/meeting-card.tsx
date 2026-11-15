import { useState } from 'react'
import type { MeetingContextDTO } from '@meeting-notes/contracts'
import { CalendarDays, ExternalLink, MapPin, UserRound, Video } from 'lucide-react'
import { safeMeetingLink } from '../../lib/admin-state'
import { dateFormatter, initials, meetingPeople, participantName } from '../../lib/notes'

const SHOWN = 6

function when(meeting: MeetingContextDTO): string {
  const start = new Date(meeting.startAt), end = new Date(meeting.endAt)
  if (Number.isNaN(start.getTime())) return ''
  const day = dateFormatter(undefined, 'meeting-day', { weekday: 'short', day: 'numeric', month: 'short', year: 'numeric' }).format(start)
  if (meeting.allDay || Number.isNaN(end.getTime())) return `${day} · All day`
  const time = dateFormatter(undefined, 'time', { hour: 'numeric', minute: '2-digit' })
  return `${day} · ${time.format(start)} – ${time.format(end)}`
}

/** Saved calendar context for a private note. Invitees are context, not transcript speakers. */
export function MeetingCard({ meeting }: { meeting: MeetingContextDTO }) {
  const [all, setAll] = useState(false)
  const people = meetingPeople(meeting.participants), visible = all ? people : people.slice(0, SHOWN)
  const eventLink = safeMeetingLink(meeting.eventUrl), meetingLink = safeMeetingLink(meeting.meetingUrl)
  const organizer = meeting.organizer && !people.some(person => person.id === meeting.organizer!.id) ? meeting.organizer : null
  return <section className="card meeting-card" aria-label="Meeting details">
    <div><p className="eyebrow">Meeting</p><h2>{meeting.title.trim() || 'Untitled event'}</h2></div>
    <ul className="meeting-facts">
      <li><CalendarDays aria-hidden="true" /><span>{when(meeting)}<br />{meeting.calendarName}</span></li>
      {meeting.location?.trim() && <li><MapPin aria-hidden="true" /><span>{meeting.location.trim()}</span></li>}
      {organizer && <li><UserRound aria-hidden="true" /><span>Organized by {participantName(organizer)}</span></li>}
    </ul>
    {people.length > 0 && <>
      <p className="meeting-people-title"><span>Invited</span><span>{people.length}</span></p>
      <ul className="meeting-people">{visible.map((person, index) => {
        const name = participantName(person)
        return <li className="meeting-person" key={person.id}>
          <span className="avatar is-small" data-tone={index % 4} aria-hidden="true">{initials(name)}</span>
          <span className="meeting-person-name"><span>{name}{person.isSelf ? ' (you)' : ''}</span>{(person.isOrganizer || (person.displayName && person.email)) && <small>{[person.isOrganizer ? 'Organizer' : '', person.displayName ? person.email ?? '' : ''].filter(Boolean).join(' · ')}</small>}</span>
        </li>
      })}</ul>
      {people.length > SHOWN && <button type="button" className="quiet-button meeting-more" aria-expanded={all} onClick={() => setAll(value => !value)}>{all ? 'Show fewer' : `Show all ${people.length}`}</button>}
    </>}
    {(eventLink || meetingLink) && <div className="meeting-links">
      {eventLink && <a href={eventLink} className="admin-secondary-button" target="_blank" rel="noopener noreferrer" referrerPolicy="no-referrer"><ExternalLink aria-hidden="true" />Calendar event</a>}
      {meetingLink && <a href={meetingLink} className="admin-secondary-button" target="_blank" rel="noopener noreferrer" referrerPolicy="no-referrer"><Video aria-hidden="true" />Meeting link</a>}
    </div>}
    <p className="meeting-note">Invitees come from the calendar. They are not matched to voices in the transcript.</p>
  </section>
}
