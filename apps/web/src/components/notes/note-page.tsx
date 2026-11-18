import { Link, useNavigate } from '@tanstack/react-router'
import { useCallback, useEffect, useRef, useState } from 'react'
import type { NoteDTO, NoteSummaryDTO } from '@meeting-notes/contracts'
import { NoteText } from '../../lib/markdown'
import { chipDayLabel, isFinished, meetingPeople, noteDay, noteFacts, noteIsFinished, noteMarkdown, type NoteStatus } from '../../lib/notes'
import { provisionalNote, summaryStatus } from '../../lib/note-list'
import { AdminClientError } from '../../lib/admin-client'
import { noteAppURL } from '../../lib/open-in-app'
import { useResource } from '../admin/shared'
import { HeaderPageActions } from '../kit/header-slot'
import { Icon } from '../kit/icon'
import { Popover } from '../kit/popover'
import { DetailsChip, NoteOptions, NoteViews, PendingNoteActions, type NoteTab } from './note-parts'
import { OpenInApp } from '../open-in-app'
import { ReaderLoading } from '../reader-state'
import { MeetingCard } from './meeting-card'
import { errorText, useMeeting, useNotes } from './notes-context'
import { ShareControl } from './share'
import { DeleteQuestion, SUBTITLE } from './trash'

export function NotePage({ noteId }: { noteId: string }) {
  const { expired, notes, morph } = useNotes()
  const load = useCallback(() => notes.read(noteId), [notes, noteId])
  const fresh = useResource(load, expired)
  // A note read earlier in this session opens at once and is refreshed in place. One that has only
  // been seen as a row opens with its heading while its text loads.
  const [row] = useState(() => notes.summary(noteId) ?? null)
  const note = fresh.data ?? notes.full(noteId) ?? null
  // Going back returns focus to this note's row, even after a direct visit.
  useEffect(() => { morph.set(noteId) }, [noteId, morph])
  const appURL = typeof window === 'undefined' ? null : noteAppURL(window.location.origin, noteId)

  if (!note && !row) return fresh.error
    ? <section className="reader-state" role="alert"><h1 className="app-heading">This note didn’t open.</h1><p>{fresh.error}</p><div className="reader-state-actions"><button type="button" className="app-pill" onClick={() => void fresh.reload()}><span>Try again</span></button><Link to="/notes" className="app-pill"><span>All notes</span></Link></div></section>
    : <><HeaderPageActions><OpenInApp appURL={appURL} /><PendingNoteActions /></HeaderPageActions><ReaderLoading /></>
  // The web reads finished notes. A meeting that is still being recorded is live in the apps.
  if (note ? !noteIsFinished(note) : !isFinished(row!)) return <section className="reader-state">
    <HeaderPageActions><OpenInApp appURL={appURL} /></HeaderPageActions>
    <h1 className="app-heading">This meeting is still in progress.</h1>
    <p>Open it in the Cereal app to follow along. Its notes will be here once the meeting has ended.</p>
    <div className="reader-state-actions">{appURL && <a className="app-pill is-primary" href={appURL}><span>Open in Cereal</span></a>}<Link to="/notes" className="app-pill"><span>All notes</span></Link></div>
  </section>
  return <NoteReader key={noteId} note={note ?? provisionalNote(row!)} row={note ? null : row} appURL={appURL} refreshError={fresh.error} retry={fresh.reload} focusTitle={row !== null} />
}

/** `row` is set while only the list row is known: the heading is real and the text is still on its way. */
function NoteReader({ note, row, appURL, refreshError, retry, focusTitle }: { note: NoteDTO; row: NoteSummaryDTO | null; appURL: string | null; refreshError: string | null; retry: () => Promise<void>; focusTitle: boolean }) {
  const { client, expired, meetings, notes, notify, session } = useNotes()
  const navigate = useNavigate()
  const facts = noteFacts(note), { title } = facts
  const has = row ? { enhanced: row.hasSummary, body: row.hasBody, transcript: row.hasTranscript } : { enhanced: facts.hasSummary, body: facts.hasBody, transcript: facts.hasTranscript }
  const status = row ? summaryStatus(row) : facts.status
  // A note with generated notes opens on them, as it does in the app.
  const [tab, setTab] = useState<NoteTab>('enhanced')
  const shown: NoteTab = has.enhanced && tab === 'enhanced' ? 'enhanced' : 'notes'
  const heading = useRef<HTMLHeadingElement>(null)
  // Whether the note is in Trash. Its row says so at once; a note opened by its address asks.
  const [trashed, setTrashed] = useState<boolean | null>(() => { const known = row ?? notes.summary(note.id); return known ? known.trashed === true : null })
  const loadAccess = useCallback(async (signal: AbortSignal) => { const access = await client.getNoteAccess(note.id, signal); setTrashed(access.suspended); return access }, [client, note.id])
  useResource(loadAccess, expired)
  const [busy, setBusy] = useState(false), [asking, setAsking] = useState(false), [problem, setProblem] = useState<string | null>(null)
  const alive = useRef(true)
  useEffect(() => { alive.current = true; return () => { alive.current = false } }, [])
  /** One change to where the note is kept. A refusal is said beside the note; a session that has ended goes to sign-in. */
  async function change(action: () => Promise<void>, done: () => void) {
    if (busy) return
    setBusy(true); setProblem(null)
    try { await action() } catch (error) {
      if (error instanceof AdminClientError && error.status === 401) expired()
      else if (alive.current) setProblem(errorText(error))
      if (alive.current) setBusy(false)
      return
    }
    if (alive.current) setBusy(false)
    done()
  }
  const moveToTrash = () => change(() => notes.moveToTrash(note.id), () => { notify('Note moved to trash'); void navigate({ to: '/notes' }) })
  const restore = () => change(() => notes.restore(note.id), () => { if (alive.current) setTrashed(false); notify('Note restored') })
  const deleteForever = () => change(() => notes.deleteForever(note.id), () => { notify('Note deleted forever'); void navigate({ to: '/notes/trash', replace: true }) })

  // Either way the answer lands in the shared store.
  const loadContext = useCallback(async (signal: AbortSignal) => { const context = await client.getNoteContext(note.id, signal); meetings.set(note.id, context.meeting); return context }, [client, meetings, note.id])
  useResource(loadContext, expired)
  const meeting = useMeeting(note.id) ?? null
  const people = meeting ? meetingPeople(meeting.participants).length : 0
  const date = chipDayLabel(noteDay(note, meeting), new Date())

  // Each note is its own entry in tabs, history and bookmarks.
  useEffect(() => { document.title = `${title} · Cereal` }, [title])
  // Arriving from the list, focus lands on the title so keyboards and screen readers start at the note.
  useEffect(() => { if (focusTitle && document.activeElement === document.body) heading.current?.focus({ preventScroll: true }) }, [focusTitle])

  const loading = row !== null && (shown === 'enhanced' ? has.enhanced : has.body)

  return <>
    <HeaderPageActions>
      <OpenInApp appURL={appURL} />
      {/* Until the note and where it is kept are both known, its actions stand in their places, dimmed. A note in Trash is shared with nobody, so it has no Share. */}
      {row || trashed === null ? <PendingNoteActions share={trashed !== true && (has.enhanced || has.body)} />
        : <>
          <NoteOptions title={title} markdown={has.enhanced || has.body ? noteMarkdown(note, shown === 'enhanced' ? 'summary' : 'notes') : null} shown={shown}>
            {close => trashed
              ? <>
                <button type="button" className="app-menu-row" role="menuitem" disabled={busy} onClick={() => { close(); void restore() }}><Icon name="restore" size={16} /><span>Restore</span></button>
                <button type="button" className="app-menu-row is-danger" role="menuitem" disabled={busy} onClick={() => { close(); setAsking(true) }}><Icon name="trash" size={16} /><span>Delete forever</span></button>
              </>
              : <button type="button" className="app-menu-row is-danger" role="menuitem" disabled={busy} onClick={() => { close(); void moveToTrash() }}><Icon name="trash" size={16} /><span>Move to trash</span></button>}
          </NoteOptions>
          {!trashed && (has.enhanced || has.body) && <ShareControl noteId={note.id} session={session} has={has} expired={expired} />}
        </>}
    </HeaderPageActions>
    <article className={has.enhanced ? 'note-doc' : 'note-doc is-plain'}>
      {trashed && <div className="note-trashed">
        {asking
          ? <DeleteQuestion title={title} busy={busy} cancel={() => setAsking(false)} confirm={() => void deleteForever()} />
          : <>
            <Icon name="trash" size={16} />
            <p role="status"><span>This note is in the Trash</span><span className="app-small">{SUBTITLE}</span></p>
            <span className="trash-answers">
              <button type="button" className="app-pill" disabled={busy} onClick={() => void restore()}><span>Restore</span></button>
              <button type="button" className="app-pill" disabled={busy} onClick={() => setAsking(true)}><span>Delete forever</span></button>
            </span>
          </>}
      </div>}
      {problem && <p className="note-quiet is-warning" role="alert">{problem}</p>}
      <h1 className="note-title" ref={heading} tabIndex={-1}>{title}</h1>
      <div className="note-meta">
        {has.enhanced && <NoteViews shown={shown} show={setTab} />}
        {meeting
          ? <Popover label="Meeting details" menu={false} className="note-details-card" buttonClassName="note-chip is-button" button={<DetailsChip date={date} people={people} />}>{() => <MeetingCard meeting={meeting} />}</Popover>
          : <span className="note-chip"><DetailsChip date={date} people={people} /></span>}
      </div>
      <div className="note-content" key={loading ? `${shown}-loading` : shown} id={`note-${shown}`} role={has.enhanced ? 'tabpanel' : undefined} aria-label={has.enhanced ? (shown === 'enhanced' ? 'Enhanced' : 'My notes') : undefined} aria-busy={loading && !refreshError ? true : undefined}>
        {refreshError && !row && <p className="note-quiet" role="alert">This note could not be refreshed, so you may be reading an earlier copy.</p>}
        {loading
          ? refreshError
            ? <div className="note-quiet" role="alert"><p>This note’s text didn’t load. {refreshError}</p><button type="button" className="app-pill" onClick={() => void retry()}><span>Try again</span></button></div>
            : <div className="note-loading"><span className="sr-only" role="status">Loading…</span><div className="skeleton" /><div className="skeleton" /><div className="skeleton is-short" /></div>
          : shown === 'enhanced'
          ? <><StatusLine status={status} /><NoteText value={note.enhanced} sourced={has.transcript} /></>
          : has.body ? <div className="note-own">{note.body}</div>
          : <><StatusLine status={has.enhanced ? null : status} /><p className="note-quiet">{has.enhanced ? 'You wrote no notes of your own in this meeting.' : has.transcript ? 'No notes were written for this meeting.' : 'This note is empty.'}</p></>}
      </div>
    </article>
  </>
}

/** A summary that is on its way, failed or out of date: one quiet line above the text. */
function StatusLine({ status }: { status: NoteStatus | null }) {
  return status ? <p className={status.tone === 'warning' ? 'note-quiet is-warning' : 'note-quiet'} role="status">{status.detail}</p> : null
}
