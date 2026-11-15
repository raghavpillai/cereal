import { Link } from '@tanstack/react-router'
import { useCallback, useEffect, useId, useRef, useState, type RefObject } from 'react'
import type { NoteDTO, NoteSummaryDTO } from '@meeting-notes/contracts'
import { ArrowLeft, AudioLines, ChevronDown, Clock3, Lock, UsersRound } from 'lucide-react'
import { Markdown } from '../../lib/markdown'
import { cachedSpeakerCount, dayLabel, defaultNoteView, initials, longDayLabel, markdownFileName, meetingPeople, noteFacts, noteMarkdown, participantName, type NoteView } from '../../lib/notes'
import { provisionalNote, summaryStatus, summaryView } from '../../lib/note-list'
import { Notice, useResource } from '../admin/shared'
import { ReaderLoading } from '../reader-state'
import { Segmented } from '../kit/segmented'
import { MeetingCard } from './meeting-card'
import { NoteActions, useNoteExport } from './note-actions'
import { useMeeting, useNotes } from './notes-context'
import { Transcript } from './transcript'

const VIEW_NAMES: Record<NoteView, string> = { summary: 'Summary', notes: 'My notes', transcript: 'Transcript' }

export function NotePage({ noteId }: { noteId: string }) {
  const { expired, notes, morph } = useNotes()
  const load = useCallback(() => notes.read(noteId), [notes, noteId])
  const fresh = useResource(load, expired)
  // A note read earlier in this session opens at once and is refreshed in place. One that has only
  // been seen as a row opens with its heading while its text loads.
  const [row] = useState(() => notes.summary(noteId) ?? null)
  const note = fresh.data ?? notes.full(noteId) ?? null
  // Going back should fold this page into its own row, even after a direct visit.
  useEffect(() => { morph.set(noteId) }, [noteId, morph])

  if (!note && !row) return fresh.error
    ? <section className="empty-state" role="alert"><p className="eyebrow">Note</p><h1>This note didn’t open.</h1><p>{fresh.error}</p><div className="empty-state-actions"><button type="button" className="button" onClick={() => void fresh.reload()}>Try again</button><Link to="/notes" className="button is-secondary">All notes</Link></div></section>
    : <ReaderLoading />
  return <NoteReader key={noteId} note={note ?? provisionalNote(row!)} row={note ? null : row} refreshError={fresh.error} retry={fresh.reload} focusTitle={row !== null} />
}

/** `row` is set while only the list row is known: the heading is real and the text is still on its way. */
function NoteReader({ note, row, refreshError, retry, focusTitle }: { note: NoteDTO; row: NoteSummaryDTO | null; refreshError: string | null; retry: () => Promise<void>; focusTitle: boolean }) {
  const { client, expired, meetings } = useNotes()
  const [view, setView] = useState<NoteView>(() => row ? summaryView(row) : defaultNoteView(note))
  const [details, setDetails] = useState(false)
  const panel = useId(), detailsId = useId()
  const heading = useRef<HTMLHeadingElement>(null), toolbar = useRef<HTMLDivElement>(null), body = useRef<HTMLElement>(null), marker = useRef<HTMLDivElement>(null)
  const stuck = useStuck(marker)

  // Either way the answer lands in the shared store.
  const loadContext = useCallback(async (signal: AbortSignal) => { const context = await client.getNoteContext(note.id, signal); meetings.set(note.id, context.meeting); return context }, [client, meetings, note.id])
  const context = useResource(loadContext, expired)
  const meeting = useMeeting(note.id) ?? null

  const facts = noteFacts(note), { title, time } = facts
  const status = row ? summaryStatus(row) : facts.status
  const speakers = row ? row.speakerCount : facts.hasTranscript ? cachedSpeakerCount(note) : 0
  const has: Record<NoteView, boolean> = row ? { summary: row.hasSummary, notes: row.hasBody, transcript: row.hasTranscript } : { summary: facts.hasSummary, notes: facts.hasBody, transcript: facts.hasTranscript }
  const actions = useNoteExport(() => noteMarkdown(note, view), markdownFileName(title))
  const content = view === 'summary' ? note.enhanced : view === 'notes' ? note.body : ''
  const people = meeting ? meetingPeople(meeting.participants) : []

  // Each note is its own entry in tabs, history and bookmarks.
  useEffect(() => { document.title = `${title} · Cereal` }, [title])
  // Arriving from the list, focus lands on the title so keyboards and screen readers start at the note.
  useEffect(() => { if (focusTitle && document.activeElement === document.body) heading.current?.focus({ preventScroll: true }) }, [focusTitle])

  function show(next: NoteView) {
    setView(next)
    // Deep in one view, the next would open at an arbitrary offset: start it at its top instead.
    if (toolbar.current && body.current && body.current.getBoundingClientRect().top < toolbar.current.getBoundingClientRect().bottom - 1) body.current.scrollIntoView({ block: 'start' })
  }

  return <article className="note-page">
    <nav className="note-crumbs" aria-label="Back">
      <Link to="/notes" className="quiet-button back-link"><ArrowLeft aria-hidden="true" />Notes</Link>
      <span className="read-only"><Lock aria-hidden="true" />Read-only on the web</span>
    </nav>

    <header className="note-masthead morph-card">
      <p className="eyebrow note-kicker"><time dateTime={note.date} title={longDayLabel(note.date)}>{dayLabel(note.date, new Date())}</time>{time && <><span className="separator" aria-hidden="true">·</span><span>{time}</span></>}</p>
      <h1 className="note-title" ref={heading} tabIndex={-1}>{title}</h1>
      {/* Always present, so details that arrive later join a line that already exists. */}
      <div className="note-facts">
        {note.duration.trim() && <span><Clock3 aria-hidden="true" />{note.duration.trim()}</span>}
        {speakers > 0 && <span><UsersRound aria-hidden="true" />{speakers} {speakers === 1 ? 'speaker' : 'speakers'}</span>}
        {status && <span className={`badge${status.tone === 'live' ? ' is-live' : status.tone === 'busy' ? ' is-good' : status.tone === 'info' ? ' is-info' : ''}`}>{status.tone === 'live' && <span className="badge-dot" aria-hidden="true" />}{status.label}</span>}
        {meeting && <button type="button" className="meeting-toggle quiet-button late-detail" aria-expanded={details} aria-controls={detailsId} onClick={() => setDetails(open => !open)}>
          {people.length > 0 && <span className="people-stack" aria-hidden="true">{people.slice(0, 3).map((person, index) => <span className="avatar is-small" data-tone={index % 4} key={person.id}>{initials(participantName(person))}</span>)}</span>}
          <span>{people.length > 0 ? `${people.length} invited` : 'Meeting details'}</span><ChevronDown aria-hidden="true" />
        </button>}
        {!meeting && context.error && <button type="button" className="quiet-button late-detail" onClick={() => void context.reload()}>Meeting details didn’t load. Retry</button>}
      </div>
    </header>

    {meeting && <aside className="note-aside late-detail" id={detailsId} data-open={details}><MeetingCard meeting={meeting} /></aside>}

    <div className="stuck-marker" ref={marker} aria-hidden="true" />
    <div className="note-toolbar" ref={toolbar} data-stuck={stuck || undefined}>
      <Segmented label="Note view" controls={panel} value={view} onChange={show} options={[
        { value: 'summary', label: VIEW_NAMES.summary }, { value: 'notes', label: VIEW_NAMES.notes },
        { value: 'transcript', label: <><AudioLines size={14} aria-hidden="true" />{VIEW_NAMES.transcript}</> },
      ]} />
      {has[view] && !row && <NoteActions {...actions} what={VIEW_NAMES[view].toLowerCase()} />}
    </div>

    <div className="note-body-wrap">
      <section className="note-body" ref={body} id={panel} role="tabpanel" aria-label={VIEW_NAMES[view]} tabIndex={-1} aria-busy={row && has[view] && !refreshError ? true : undefined}>
        <div className="fade-swap" key={row ? `${view}-loading` : view}>
          {refreshError && !row && <Notice error>This note could not be refreshed, so you may be reading an earlier copy.</Notice>}
          {status && view !== 'notes' && (view === 'summary' || status.tone === 'live') && <p className={`note-status${status.tone === 'warning' ? ' is-warning' : ''}`} role="status">{status.detail}</p>}
          {row && has[view]
            ? refreshError
              ? <div className="note-body-empty" role="alert"><strong>This note’s text didn’t load</strong><span>{refreshError}</span><button type="button" className="admin-secondary-button" onClick={() => void retry()}>Try again</button></div>
              : <div className="note-body-loading"><span className="sr-only" role="status">Loading…</span><div className="skeleton line-skeleton" /><div className="skeleton line-skeleton" /><div className="skeleton line-skeleton short" /></div>
            : view === 'transcript'
            ? has.transcript ? <Transcript segments={note.transcriptSegments} /> : <EmptyView title="No transcript">Nothing was transcribed for this note. Recording happens in the desktop and iPhone apps.</EmptyView>
            : has[view] ? <Markdown value={content} />
            : view === 'summary' ? <EmptyView title="No summary yet" action={has.transcript ? { label: 'Read the transcript', run: () => show('transcript') } : has.notes ? { label: 'Read my notes', run: () => show('notes') } : undefined}>{has.transcript ? 'This note has a transcript but no generated summary. Generate one from the desktop or iPhone app.' : 'A summary is written after a meeting is recorded and transcribed.'}</EmptyView>
            : <EmptyView title="No notes of your own">Anything you type during a meeting in the desktop or iPhone app appears here.</EmptyView>}
        </div>
      </section>
      <p className="action-notice" role="status">{actions.notice}</p>
    </div>
  </article>
}

function EmptyView({ title, children, action }: { title: string; children: string; action?: { label: string; run: () => void } }) {
  return <div className="note-body-empty"><strong>{title}</strong><span>{children}</span>{action && <button type="button" className="admin-secondary-button" onClick={action.run}>{action.label}</button>}</div>
}

/** True while a sticky element is pinned: a one-pixel marker placed just above it has scrolled out of view. */
export function useStuck(marker: RefObject<HTMLElement | null>): boolean {
  const [stuck, setStuck] = useState(false)
  useEffect(() => {
    const element = marker.current
    if (!element || typeof IntersectionObserver === 'undefined') return
    const observer = new IntersectionObserver(([entry]) => setStuck(!entry!.isIntersecting && entry!.boundingClientRect.top < 0))
    observer.observe(element)
    return () => observer.disconnect()
  }, [marker])
  return stuck
}
