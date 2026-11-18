import { Link } from '@tanstack/react-router'
import { memo, useEffect, useMemo, useRef, useState, useSyncExternalStore, type KeyboardEvent, type RefObject } from 'react'
import type { NoteSearchResultDTO, NoteSearchTextPartDTO, NoteSummaryDTO } from '@meeting-notes/contracts'
import { chipDayLabel, clockParts, timelineDate } from '../../lib/notes'
import { finishedResults, finishedSummaries, groupSummariesByDay, summaryFacts } from '../../lib/note-list'
import { HeaderActions } from '../kit/header-slot'
import { Icon } from '../kit/icon'
import { Notice } from '../admin/shared'
import { LISTED, errorText, useNotes, useNotesSearch, type MorphStore } from './notes-context'

/** The notes list, laid out as the desktop app's Home list: a heading, then each
 * day's finished notes. Meetings still being recorded live in the apps. */
export function NotesHome() {
  const { notes: store, morph } = useNotes()
  const { query, setQuery } = useNotesSearch()
  const feed = store.list(LISTED)
  const listed = useSyncExternalStore(feed.subscribe, feed.get)
  const found = useSyncExternalStore(store.search.subscribe, store.search.get)
  const search = useRef<HTMLInputElement>(null), list = useRef<HTMLDivElement>(null), more = useRef<HTMLDivElement>(null)
  // The search field takes the place of its button once it is asked for, and stays while it holds text.
  const [searchOpen, setSearchOpen] = useState(() => query !== '')

  // Rows come a page at a time from the server; the search runs there too, after a pause in typing.
  useEffect(() => { store.show(LISTED) }, [store])
  useEffect(() => { store.search.set(query, LISTED) }, [store, query])

  const searching = found.active
  // A first search leaves the list in place, dimmed, until its results arrive; later searches dim the results they replace.
  const results = useMemo(() => searching && found.results ? finishedResults(found.results) : null, [searching, found.results])
  const rows = useMemo(() => results === null && listed.items ? finishedSummaries(listed.items) : null, [results, listed.items])
  const waiting = rows === null && results === null && (searching ? found.error === null : listed.error === null)
  const hasMore = results !== null ? found.hasMore && !found.pending : !searching && listed.hasMore
  const loadingMore = results !== null ? found.loadingMore : listed.loading === 'more'
  const moreError = results !== null ? found.moreError : listed.moreError
  // Rows the server has handed over, whether or not they are shown: a page of unfinished notes still moves the list on.
  const received = results !== null ? found.results?.length ?? 0 : listed.items?.length ?? 0
  const arrived = listed.items !== null

  const groups = useMemo(() => groupSummariesByDay(rows ?? [], new Date()), [rows])

  const loadMore = results !== null ? store.search.more : feed.more
  useNextPage(more, { hasMore, loading: loadingMore, error: moreError, load: loadMore, received })

  // The field takes focus as it appears, in the same turn as the press that asked for it, so what is typed straight
  // after lands in it. A field that is open because a search is still in it (coming back from a note) does not.
  const asked = useRef(false)
  function openSearch() {
    // Already open: take the keyboard there and select what it holds, now, not a frame later; a selection made
    // after typing has begun would be replaced by the next key.
    if (search.current) { search.current.focus(); search.current.select(); return }
    asked.current = true
    setSearchOpen(true)
  }
  function closeSearch() { setQuery(''); setSearchOpen(false) }

  useEffect(() => {
    const onKey = (event: globalThis.KeyboardEvent) => {
      const target = event.target as HTMLElement | null
      const find = event.key === '/' && !event.metaKey && !event.ctrlKey && !event.altKey, palette = event.key.toLowerCase() === 'k' && (event.metaKey || event.ctrlKey) && !event.altKey
      if (!(find || palette) || (find && target?.closest('input, textarea, select, [contenteditable="true"]'))) return
      event.preventDefault(); openSearch()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])

  // Coming back from a note puts focus on its row, so the keyboard continues where it left off.
  useEffect(() => {
    const id = morph.get()
    if (!id || !arrived || document.activeElement !== document.body) return
    list.current?.querySelector<HTMLElement>(`[data-note="${id}"]`)?.focus({ preventScroll: true })
  }, [arrived, morph])

  const rowElements = () => [...(list.current?.querySelectorAll<HTMLElement>('.note-row') ?? [])]
  function searchKeys(event: KeyboardEvent<HTMLInputElement>) {
    if (event.key === 'Escape') { event.preventDefault(); closeSearch() }
    else if (event.key === 'ArrowDown') { const first = rowElements()[0]; if (first) { event.preventDefault(); first.focus() } }
  }
  function listKeys(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp') return
    const every = rowElements(), at = every.indexOf(document.activeElement as HTMLElement)
    if (at < 0) return
    event.preventDefault()
    if (event.key === 'ArrowUp' && at === 0) { if (searchOpen) search.current?.focus() }
    else every[at + (event.key === 'ArrowDown' ? 1 : -1)]?.focus()
  }
  // A mouse press starts reading the note; the click that follows finds the request already on its way.
  // A finger that lands on a row is as likely to be scrolling, so touch waits for the click.
  const press = useMemo(() => (id: string) => { store.read(id).catch(() => { /* the note page reports it */ }) }, [store])

  const now = new Date()
  const nothingYet = !searching && rows !== null && rows.length === 0 && !listed.hasMore && listed.error === null
  const nothingMatches = searching && results !== null && results.length === 0 && !found.pending && !found.hasMore && found.error === null
  return <section className="notes-home" aria-labelledby="notes-heading"><div className="notes-column">
    <header className="notes-head">
      <h1 id="notes-heading" className="app-heading">My notes</h1>
      {searchOpen
        ? <div className="app-find" role="search">
          <Icon name="search" size={13} />
          <input ref={search} type="search" autoFocus={asked.current} value={query} placeholder="Search" aria-label="Search notes" autoComplete="off" spellCheck={false} enterKeyHint="search" onChange={event => setQuery(event.target.value)} onKeyDown={searchKeys} />
          <button type="button" className="app-tool is-small" aria-label="Close search" title="Close search · Esc" onClick={closeSearch}><Icon name="close" size={15} /></button>
        </div>
        : <button type="button" className="app-tool" aria-label="Search notes" title="Search notes · /" onClick={openSearch}><Icon name="search" size={15} /></button>}
      <Link to="/notes/trash" className="app-tool" aria-label="Trash" title="Trash"><Icon name="trash" size={15} /></Link>
    </header>

    {listed.error !== null && !searching && <Notice error>{errorText(listed.error)} <button type="button" className="admin-text-button" onClick={() => void store.reload(LISTED)}>Try again</button></Notice>}
    {found.error !== null && searching && <Notice error>The search did not finish. {errorText(found.error)} <button type="button" className="admin-text-button" onClick={() => store.search.retry()}>Try again</button></Notice>}

    <div className={`notes-list${searching && found.pending ? ' is-waiting' : ''}`} ref={list} aria-busy={searching && found.pending ? true : undefined} onKeyDown={listKeys}>
      {waiting && <NoteRowsSkeleton />}
      {results !== null && results.length > 0 && <section aria-labelledby="search-results" key={found.query}>
        <h2 className="notes-day is-results" id="search-results">Results for “{found.query.trim()}”</h2>
        <ul>{results.map(result => <NoteRow key={result.note.id} note={result.note} match={result} day={chipDayLabel(timelineDate(result.note), now)} morph={morph} press={press} />)}</ul>
      </section>}
      {groups.map((group, index) => <section key={`${group.key}-${index}`} aria-labelledby={`day-${group.key}-${index}`}>
        <h2 className="notes-day" id={`day-${group.key}-${index}`}>{group.label}</h2>
        <ul>{group.notes.map(note => <NoteRow key={note.id} note={note} match={null} day={null} morph={morph} press={press} />)}</ul>
      </section>)}
      {nothingYet && <div className="notes-empty">
        <Icon name="document" size={32} />
        <h2 className="app-heading">A little space for your thoughts</h2>
        <p className="app-small">Record a meeting in the Cereal app and its notes will be here when it ends.</p>
      </div>}
      {nothingMatches && <p className="notes-none">No matching meetings. Try another search.</p>}
      {loadingMore && <NoteRowsSkeleton more />}
      {moreError !== null && received > 0 && <p className="notes-more-error" role="alert">The next notes did not load. <button type="button" className="admin-text-button" onClick={() => void loadMore()}>Try again</button></p>}
      {hasMore && <div ref={more} className="notes-more" aria-hidden="true" />}
    </div>
  </div></section>
}

/** One row. It is memoised, so a new page or a keystroke re-renders only the rows whose own data changed. */
const NoteRow = memo(function NoteRow({ note, match, day, morph, press }: { note: NoteSummaryDTO; match: NoteSearchResultDTO | null; day: string | null; morph: MorphStore; press: (id: string) => void }) {
  if (import.meta.env.DEV) (globalThis as { __cerealRowRenders?: number }).__cerealRowRenders = ((globalThis as { __cerealRowRenders?: number }).__cerealRowRenders ?? 0) + 1
  const id = note.id, { title, time } = summaryFacts(note)
  return <li>
    <Link to="/notes/$noteId" params={{ noteId: id }} className="note-row" data-note={id} aria-labelledby={`note-${id}-title`}
      onPointerDown={event => { if (event.pointerType === 'mouse' && event.button === 0) press(id) }} onClick={() => morph.set(id)}>
      <span className="note-row-box">
        <span className="note-row-tile" aria-hidden="true"><Icon name="document" size={17} /></span>
        <span className="note-row-main">
          <span className="note-row-title" id={`note-${id}-title`}>{match && match.title.length ? <Parts parts={match.title} /> : title}</span>
          <span className="app-small">Me</span>
        </span>
        {/* Under a day heading a row shows its time; among search results it shows its day. */}
        <span className="note-row-time">{day !== null ? <span className="time-label">{day}</span> : <TimeLabel time={time} />}</span>
      </span>
      {match?.excerpt && <span className="note-row-snippet">{match.excerpt.source === 'transcript' && <span className="sr-only">In the transcript: </span>}{match.excerpt.source === 'people' && <span className="sr-only">Invited: </span>}<Parts parts={match.excerpt.parts} /></span>}
    </Link>
  </li>
})

/** A time with its AM/PM set as small capitals on the digits' baseline. */
export function TimeLabel({ time }: { time: string }) {
  const { digits, meridiem } = clockParts(time)
  return <span className="time-label">{digits}{meridiem && <span className="meridiem">{meridiem}</span>}</span>
}

function Parts({ parts }: { parts: readonly NoteSearchTextPartDTO[] }) {
  return <>{parts.map((part, at) => part.hit ? <mark key={at}>{part.text}</mark> : part.text)}</>
}

/** The next page is asked for as the end of a list comes near: `sentinel` is an element after its last row.
 * Observing again after each page (`received` changes) means a page that did not fill the screen is followed
 * by the next without a scroll. */
export function useNextPage(sentinel: RefObject<HTMLElement | null>, { hasMore, loading, error, load, received }: { hasMore: boolean; loading: boolean; error: unknown; load: () => Promise<void>; received: number }) {
  useEffect(() => {
    const element = sentinel.current
    if (!element || !hasMore || loading || error) return
    if (typeof IntersectionObserver === 'undefined') { void load(); return }
    const observer = new IntersectionObserver(entries => { if (entries.some(entry => entry.isIntersecting)) void load() }, { rootMargin: '900px 0px' })
    observer.observe(element)
    return () => observer.disconnect()
  }, [sentinel, hasMore, loading, error, load, received])
}

/** Row-shaped placeholders: four while the page is first loading, two under a list that is being continued. */
export function NoteRowsSkeleton({ more = false, label = 'Loading your notes…' }: { more?: boolean; label?: string }) {
  return <div className={more ? 'notes-skeleton is-more' : 'notes-skeleton'} aria-busy="true"><span className="sr-only" role="status">{more ? 'Loading more notes…' : label}</span>
    {(more ? [0, 1] : [0, 1, 2, 3]).map(row => <div key={row}><span className="skeleton" /><span className="skeleton" /><span className="skeleton" /></div>)}
  </div>
}

/** The notes page before anything is known: the same heading and rows the real
 * page will have, so a cold load shows one skeleton and then the list. */
export function NotesHomeSkeleton() {
  return <section className="notes-home" aria-busy="true"><div className="notes-column">
    <HeaderActions><span className="site-avatar is-placeholder" aria-hidden="true" /></HeaderActions>
    <header className="notes-head"><h1 className="app-heading">My notes</h1><span className="app-tool" aria-hidden="true"><Icon name="search" size={15} /></span><span className="app-tool" aria-hidden="true"><Icon name="trash" size={15} /></span></header>
    <div className="notes-list"><NoteRowsSkeleton /></div>
  </div></section>
}
