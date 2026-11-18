import { Link } from '@tanstack/react-router'
import { useEffect, useMemo, useRef, useState, useSyncExternalStore } from 'react'
import type { NoteSummaryDTO } from '@meeting-notes/contracts'
import { AdminClientError } from '../../lib/admin-client'
import { chipDayLabel, timelineDate } from '../../lib/notes'
import { summaryFacts } from '../../lib/note-list'
import { Notice } from '../admin/shared'
import { Icon } from '../kit/icon'
import { errorText, useNotes } from './notes-context'
import { NoteRowsSkeleton, useNextPage } from './notes-home'

/** The desktop app's words for Trash (apps/desktop/src/trash.rs). Nothing empties Trash by itself. */
export const SUBTITLE = 'Notes stay here until you delete them forever'
export const EMPTY = 'Trash is empty'

/** Trash: the notes the owner has put away, each with Restore and Delete forever. It is the desktop app's
 * Trash card laid out as a page, in the notes list's column. A row opens its note, which says it is in Trash. */
export function TrashPage() {
  const { expired, notes: store, notify } = useNotes()
  const listed = useSyncExternalStore(store.trash.subscribe, store.trash.get)
  // Trash is read again on every visit: a note may have been put in it, or taken out, in one of the apps.
  useEffect(() => { store.showTrash() }, [store])
  useEffect(() => { document.title = 'Trash · Cereal' }, [])

  const [busy, setBusy] = useState<ReadonlySet<string>>(new Set()), [asking, setAsking] = useState<string | null>(null), [problem, setProblem] = useState<string | null>(null)
  const alive = useRef(true), list = useRef<HTMLUListElement>(null), more = useRef<HTMLDivElement>(null)
  useEffect(() => { alive.current = true; return () => { alive.current = false } }, [])
  /** One note restored or deleted. While it is under way the row's controls are held; a refusal is said above the list. */
  async function change(id: string, action: () => Promise<void>, done: string) {
    if (busy.has(id)) return
    setBusy(old => new Set(old).add(id)); setProblem(null)
    try { await action(); notify(done) } catch (error) {
      if (error instanceof AdminClientError && error.status === 401) expired()
      else if (alive.current) setProblem(errorText(error))
    }
    if (alive.current) { setBusy(old => { const next = new Set(old); next.delete(id); return next }); setAsking(old => old === id ? null : old) }
  }
  /** Letting go of the question puts the keyboard back on the control that asked it. */
  function cancel(id: string) {
    setAsking(null)
    requestAnimationFrame(() => list.current?.querySelector<HTMLElement>(`[data-note="${id}"] [data-delete]`)?.focus())
  }

  const rows = listed.items
  useNextPage(more, { hasMore: listed.hasMore, loading: listed.loading === 'more', error: listed.moreError, load: store.trash.more, received: rows?.length ?? 0 })
  const now = useMemo(() => new Date(), [rows])
  const empty = rows !== null && rows.length === 0 && !listed.hasMore && listed.error === null
  return <section className="notes-home" aria-labelledby="trash-heading"><div className="notes-column">
    <header className="notes-head is-trash">
      <h1 id="trash-heading" className="app-heading">Trash</h1>
      <p className="app-small">{SUBTITLE}</p>
    </header>

    {listed.error !== null && <Notice error>{errorText(listed.error)} <button type="button" className="admin-text-button" onClick={() => void store.trash.load()}>Try again</button></Notice>}
    {problem !== null && <Notice error>{problem}</Notice>}

    <div className="notes-list trash-list">
      {rows === null && listed.error === null && <NoteRowsSkeleton label="Loading Trash…" />}
      {rows !== null && rows.length > 0 && <ul ref={list} aria-labelledby="trash-heading">{rows.map(note => <TrashRow key={note.id} note={note} now={now} busy={busy.has(note.id)} asking={asking === note.id}
        ask={() => setAsking(note.id)} cancel={() => cancel(note.id)}
        restore={() => void change(note.id, () => store.restore(note.id), 'Note restored')}
        confirm={() => void change(note.id, () => store.deleteForever(note.id), 'Note deleted forever')} />)}</ul>}
      {empty && <div className="notes-empty">
        <Icon name="trash" size={32} />
        <h2 className="app-heading">{EMPTY}</h2>
      </div>}
      {listed.loading === 'more' && <NoteRowsSkeleton more />}
      {listed.moreError !== null && rows !== null && rows.length > 0 && <p className="notes-more-error" role="alert">The next notes did not load. <button type="button" className="admin-text-button" onClick={() => void store.trash.more()}>Try again</button></p>}
      {listed.hasMore && <div ref={more} className="notes-more" aria-hidden="true" />}
    </div>
  </div></section>
}

/** Trash before anything is known: its heading and the rows that are coming. */
export function TrashSkeleton() {
  return <section className="notes-home" aria-busy="true"><div className="notes-column">
    <header className="notes-head is-trash"><h1 className="app-heading">Trash</h1><p className="app-small">{SUBTITLE}</p></header>
    <div className="notes-list trash-list"><NoteRowsSkeleton label="Loading Trash…" /></div>
  </div></section>
}

function TrashRow({ note, now, busy, asking, ask, cancel, restore, confirm }: { note: NoteSummaryDTO; now: Date; busy: boolean; asking: boolean; ask: () => void; cancel: () => void; restore: () => void; confirm: () => void }) {
  const { title } = summaryFacts(note), id = note.id
  if (asking) return <li className="trash-row is-asking" data-note={id}>
    <span className="note-row-tile" aria-hidden="true"><Icon name="document" size={17} /></span>
    <DeleteQuestion title={title} busy={busy} cancel={cancel} confirm={confirm} />
  </li>
  return <li className="trash-row" data-note={id}>
    <Link to="/notes/$noteId" params={{ noteId: id }} className="note-row" aria-labelledby={`trash-${id}-title`}>
      <span className="note-row-box">
        <span className="note-row-tile" aria-hidden="true"><Icon name="document" size={17} /></span>
        <span className="note-row-main">
          <span className="note-row-title" id={`trash-${id}-title`}>{title}</span>
          <span className="app-small">{chipDayLabel(timelineDate(note), now)}</span>
        </span>
      </span>
    </Link>
    <span className="trash-actions">
      <button type="button" className="app-tool" aria-label={`Restore ${title}`} title="Restore" disabled={busy} onClick={restore}><Icon name="restore" size={16} /></button>
      <button type="button" className="app-tool" data-delete aria-label={`Delete ${title} forever`} title="Delete forever" disabled={busy} onClick={ask}><Icon name="trash" size={16} /></button>
    </span>
  </li>
}

/** The question before a note is deleted for good, asked in place as the desktop app asks it: the title gives
 * way so that "forever?" always shows. The keyboard starts on Cancel, so Return never deletes; Escape lets go. */
export function DeleteQuestion({ title, busy, cancel, confirm }: { title: string; busy: boolean; cancel: () => void; confirm: () => void }) {
  const first = useRef<HTMLButtonElement>(null)
  useEffect(() => { first.current?.focus({ preventScroll: true }) }, [])
  return <div className="trash-question" role="alertdialog" aria-label={`Delete “${title}” forever?`} aria-describedby="trash-question-detail" onKeyDown={event => { if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); cancel() } }}>
    <p><span className="trash-question-title"><span>Delete “</span><span>{title}</span><span>” forever?</span></span><span className="app-small" id="trash-question-detail">This can’t be undone.</span></p>
    <span className="trash-answers">
      <button ref={first} type="button" className="app-pill" disabled={busy} onClick={cancel}><span>Cancel</span></button>
      <button type="button" className="app-pill is-danger" disabled={busy} onClick={confirm}><span>{busy ? 'Deleting…' : 'Delete'}</span></button>
    </span>
  </div>
}
