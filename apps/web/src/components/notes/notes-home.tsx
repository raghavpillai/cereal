import { Link } from '@tanstack/react-router'
import { memo, useEffect, useMemo, useRef, useState, useSyncExternalStore, type CSSProperties, type KeyboardEvent } from 'react'
import { flushSync } from 'react-dom'
import type { NoteListFilter, NoteSearchResultDTO, NoteSearchTextPartDTO, NoteSummaryDTO } from '@meeting-notes/contracts'
import { Building2, ChevronDown, ChevronRight, NotebookPen, RefreshCw, Search, SearchX, X } from 'lucide-react'
import { initials } from '../../lib/notes'
import { groupSummariesByDay, shortDay, summaryFacts } from '../../lib/note-list'
import { HeaderActions } from '../kit/header-slot'
import { Menu, MenuItem } from '../kit/menu'
import { Segmented } from '../kit/segmented'
import { Notice } from '../admin/shared'
import { errorText, useNotes, useNotesSearch, type MorphStore } from './notes-context'

const FILTERS: readonly { value: NoteListFilter; label: string }[] = [
  { value: 'all', label: 'All' }, { value: 'summarized', label: 'Summarized' }, { value: 'transcribed', label: 'With transcript' },
]
/** Only the first rows on screen take part in the entrance. */
const STAGGERED = 12

export function NotesHome() {
  const { notes: store, session, workspaceError, switching, morph } = useNotes()
  const { query, setQuery, filter, setFilter } = useNotesSearch()
  const feed = store.list(filter), everything = store.list('all')
  const listed = useSyncExternalStore(feed.subscribe, feed.get), all = useSyncExternalStore(everything.subscribe, everything.get)
  const found = useSyncExternalStore(store.search.subscribe, store.search.get)
  const search = useRef<HTMLInputElement>(null), list = useRef<HTMLDivElement>(null), more = useRef<HTMLDivElement>(null)

  // Rows come a page at a time from the server; the search runs there too, after a pause in typing.
  useEffect(() => { store.show(filter) }, [store, filter])
  useEffect(() => { store.search.set(query, filter) }, [store, query, filter])

  const searching = found.active
  // A first search leaves the list in place, dimmed, until its results arrive; later searches dim the results they replace.
  const results = searching ? found.results : null
  const rows = results === null ? listed.items : null
  const waiting = rows === null && results === null && (searching ? found.error === null : listed.error === null)
  const total = all.total, shown = searching ? found.total : listed.total
  const hasMore = results !== null ? found.hasMore && !found.pending : !searching && listed.hasMore
  const loadingMore = results !== null ? found.loadingMore : listed.loading === 'more'
  const moreError = results !== null ? found.moreError : listed.moreError
  const count = results?.length ?? rows?.length ?? 0
  const busy = listed.loading !== null || found.pending

  // Rows settle in once, when notes first arrive: not on a revisit, and not for rows a search brings back.
  const [entering, setEntering] = useState(() => listed.items === null)
  const arrived = listed.items !== null
  useEffect(() => {
    if (!entering || !arrived) return
    const timer = window.setTimeout(() => setEntering(false), 900)
    return () => window.clearTimeout(timer)
  }, [entering, arrived])

  const groups = useMemo(() => groupSummariesByDay(rows ?? [], new Date()), [rows])

  // The next page is asked for as the end of the list comes near. Observing again after each
  // page means a page that did not fill the screen is followed by the next without a scroll.
  const loadMore = results !== null ? store.search.more : feed.more
  useEffect(() => {
    const element = more.current
    if (!element || !hasMore || loadingMore || moreError) return
    if (typeof IntersectionObserver === 'undefined') { void loadMore(); return }
    const observer = new IntersectionObserver(entries => { if (entries.some(entry => entry.isIntersecting)) void loadMore() }, { rootMargin: '900px 0px' })
    observer.observe(element)
    return () => observer.disconnect()
  }, [hasMore, loadingMore, moreError, loadMore, count])

  useEffect(() => {
    const onKey = (event: globalThis.KeyboardEvent) => {
      const target = event.target as HTMLElement | null
      if (event.key !== '/' || event.metaKey || event.ctrlKey || event.altKey || target?.closest('input, textarea, select, [contenteditable="true"]')) return
      event.preventDefault(); search.current?.focus(); search.current?.select()
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
    if (event.key === 'Escape') { if (query) { event.preventDefault(); setQuery('') } else event.currentTarget.blur() }
    else if (event.key === 'ArrowDown') { const first = rowElements()[0]; if (first) { event.preventDefault(); first.focus() } }
  }
  function listKeys(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp') return
    const every = rowElements(), at = every.indexOf(document.activeElement as HTMLElement)
    if (at < 0) return
    event.preventDefault()
    if (event.key === 'ArrowUp' && at === 0) search.current?.focus()
    else every[at + (event.key === 'ArrowDown' ? 1 : -1)]?.focus()
  }
  // A mouse press starts reading the note; the click that follows finds the request already on its way.
  // A finger that lands on a row is as likely to be scrolling, so touch waits for the click.
  const press = useMemo(() => (id: string) => { store.read(id).catch(() => { /* the note page reports it */ }) }, [store])

  const narrowed = searching || filter !== 'all', firstName = session.user.name.trim().split(/\s+/)[0], now = new Date()
  const noun = (n: number) => n === 1 ? 'note' : 'notes'
  const resultLine = searching && found.pending ? 'Searching…'
    : waiting || !narrowed || shown === null || (searching && found.error !== null) ? ''
    : shown === 0 ? 'No notes match.'
    : total === null ? `${shown} ${noun(shown)}` : `${shown} of ${total} ${noun(total)}`
  const nothingMatches = searching ? results !== null && results.length === 0 && !found.pending && found.error === null : filter !== 'all' && listed.items?.length === 0 && listed.error === null
  let position = 0
  return <section className="notes-home" aria-labelledby="notes-heading">
    <header className="notes-head">
      <div><h1 id="notes-heading">Notes</h1><p className="notes-sub">{total === null ? `Welcome back${firstName ? `, ${firstName}` : ''}.` : total === 0 ? 'Nothing here yet.' : `${total} ${noun(total)}, newest first.`}</p></div>
      <div className="notes-head-actions"><WorkspaceSwitcher /><button type="button" className={`icon-button${busy && arrived ? ' is-spinning' : ''}`} aria-label="Refresh notes" title="Refresh" disabled={busy} onClick={() => void store.reload(filter)}><RefreshCw aria-hidden="true" /></button></div>
    </header>

    {listed.error !== null && !searching && <Notice error>{errorText(listed.error)} <button type="button" className="admin-text-button" onClick={() => void store.reload(filter)}>Try again</button></Notice>}
    {found.error !== null && searching && <Notice error>The search did not finish. {errorText(found.error)} <button type="button" className="admin-text-button" onClick={() => store.search.retry()}>Try again</button></Notice>}
    {workspaceError && <Notice error>The workspace could not be changed. {workspaceError}</Notice>}

    {(total === null || total > 0) && <>
      <div className="notes-tools">
        <div className="notes-search" role="search">
          <Search aria-hidden="true" />
          <input ref={search} type="search" value={query} placeholder="Search titles, notes and transcripts" aria-label="Search notes" autoComplete="off" spellCheck={false} enterKeyHint="search" onChange={event => setQuery(event.target.value)} onKeyDown={searchKeys} />
          <div className="notes-search-end">{query ? <button type="button" className="icon-button" aria-label="Clear search" onClick={() => { setQuery(''); search.current?.focus() }}><X aria-hidden="true" /></button> : <kbd aria-hidden="true">/</kbd>}</div>
        </div>
        <Segmented label="Filter notes" kind="radio" options={FILTERS} value={filter} onChange={setFilter} />
      </div>
      <p className="notes-result-line" role="status">{resultLine}</p>
    </>}

    {waiting && <NoteRowsSkeleton />}

    {total === 0 && <div className="notes-empty rise-in"><div className="empty-state">
      <div className="empty-tile" aria-hidden="true"><NotebookPen /></div>
      <h2>A fresh page</h2>
      <p>Record a meeting in the desktop or iPhone app and its notes will be waiting here. To read a note someone shared with you, open their link.</p>
    </div></div>}

    {nothingMatches && <div className="notes-empty fade-swap"><div className="empty-state">
      <div className="empty-tile" aria-hidden="true"><SearchX /></div>
      <h2>Nothing matches</h2>
      <p>{searching ? <>No {filter === 'all' ? 'note' : filter === 'summarized' ? 'summarized note' : 'note with a transcript'} mentions “{found.query.trim()}”.</> : <>No notes fit this filter yet.</>}</p>
      <div className="empty-state-actions"><button type="button" className="admin-secondary-button" onClick={() => { setQuery(''); setFilter('all'); search.current?.focus() }}>Show all notes</button></div>
    </div></div>}

    <div className={`notes-list${entering ? ' is-entering' : ''}${switching || (searching && found.pending) ? ' is-switching' : ''}`} ref={list} inert={switching} aria-busy={searching && found.pending ? true : undefined} onKeyDown={listKeys}>
      {results !== null && results.length > 0 && <section className="day-group fade-swap" aria-labelledby="search-results" key={found.query}>
        <h2 className="day-label" id="search-results">Best matches<small>{found.total}</small></h2>
        <ul className="day-notes">{results.map(result => <NoteRow key={result.note.id} note={result.note} match={result} day={shortDay(result.note.date, now)} stagger={undefined} morph={morph} press={press} />)}</ul>
      </section>}
      {groups.map((group, index) => {
        const first = position
        // The last day on screen may continue on the next page, so its count is not known yet.
        const counted = !(listed.hasMore && index === groups.length - 1)
        return <section className="day-group" key={group.key} aria-labelledby={`day-${group.key}`}>
          <h2 className={first < STAGGERED ? 'day-label stagger-item' : 'day-label'} id={`day-${group.key}`} style={first < STAGGERED ? { '--i': first } as CSSProperties : undefined}>{group.label}{counted && <small>{group.notes.length}</small>}</h2>
          <ul className="day-notes">{group.notes.map(note => {
            const at = position++
            return <NoteRow key={note.id} note={note} match={null} day={null} stagger={entering && at < STAGGERED ? at : undefined} morph={morph} press={press} />
          })}</ul>
        </section>
      })}
      {loadingMore && <NoteRowsSkeleton more />}
      {moreError !== null && count > 0 && <p className="notes-more-error" role="alert">The next notes did not load. <button type="button" className="admin-text-button" onClick={() => void loadMore()}>Try again</button></p>}
      {hasMore && <div ref={more} className="notes-more" aria-hidden="true" />}
    </div>
  </section>
}

/** One row. It is memoised and reads its morph state from a store, so a new
 * page or a keystroke re-renders only the rows whose own data changed. */
const NoteRow = memo(function NoteRow({ note, match, day, stagger, morph, press }: { note: NoteSummaryDTO; match: NoteSearchResultDTO | null; day: string | null; stagger: number | undefined; morph: MorphStore; press: (id: string) => void }) {
  if (import.meta.env.DEV) (globalThis as { __cerealRowRenders?: number }).__cerealRowRenders = ((globalThis as { __cerealRowRenders?: number }).__cerealRowRenders ?? 0) + 1
  const id = note.id
  const isMorph = useSyncExternalStore(morph.subscribe, () => morph.get() === id)
  const { title, time, status, details, invited } = summaryFacts(note), people = note.attendees.people
  // Under a day heading a row shows its time; among search results it shows its day.
  const when = day ?? time
  return <li className={stagger === undefined ? undefined : 'stagger-item'} style={stagger === undefined ? undefined : { '--i': stagger } as CSSProperties}>
    <Link to="/notes/$noteId" params={{ noteId: id }} className={`note-row${isMorph ? ' morph-card' : ''}`} data-note={id} aria-labelledby={`note-${id}-title`} aria-describedby={`note-${id}-meta`}
      onPointerDown={event => { if (event.pointerType === 'mouse' && event.button === 0) press(id) }} onClick={() => { if (!isMorph) flushSync(() => morph.set(id)) }}>
      <time className="note-row-time" dateTime={day ? note.date : note.createdAt}>{when ?? ''}</time>
      <span className="note-row-main">
        <span className="note-row-title" id={`note-${id}-title`}>{match && match.title.length ? <Parts parts={match.title} /> : title}</span>
        <span className="note-row-meta" id={`note-${id}-meta`}>
          {when && <span className="phone-time">{when}</span>}
          {details.map(detail => <span key={detail}>{detail}</span>)}
          {invited && <span className="note-row-people"><span className="sr-only">with </span>{invited}</span>}
          {people.length > 0 && <span className="people-stack" aria-hidden="true">{people.slice(0, 3).map((person, at) => <span className="avatar is-small" data-tone={at % 4} key={at}>{person.initials || initials(person.name)}</span>)}</span>}
        </span>
        {match ? match.excerpt && <span className="note-row-snippet">{match.excerpt.source === 'transcript' && <span className="sr-only">In the transcript: </span>}{match.excerpt.source === 'people' && <span className="sr-only">Invited: </span>}<Parts parts={match.excerpt.parts} /></span>
          : note.excerpt ? <span className="note-row-snippet">{note.excerpt}</span> : null}
      </span>
      <span className="note-row-end">
        {status && (status.tone === 'live' || status.tone === 'busy' || status.tone === 'warning') && <span className={`badge${status.tone === 'live' ? ' is-live' : status.tone === 'busy' ? ' is-good' : ''}`}>{status.tone === 'live' && <span className="badge-dot" aria-hidden="true" />}{status.label}</span>}
        <ChevronRight className="note-row-chevron" aria-hidden="true" />
      </span>
    </Link>
  </li>
})

function Parts({ parts }: { parts: readonly NoteSearchTextPartDTO[] }) {
  return <>{parts.map((part, at) => part.hit ? <mark key={at}>{part.text}</mark> : part.text)}</>
}

/** Row-shaped placeholders: four while the page is first loading, two under a list that is being continued. */
function NoteRowsSkeleton({ more = false }: { more?: boolean }) {
  return <div className={more ? 'notes-skeleton is-more' : 'notes-skeleton'} aria-busy="true"><span className="sr-only" role="status">{more ? 'Loading more notes…' : 'Loading your notes…'}</span>
    {(more ? [0, 1] : [0, 1, 2, 3]).map(row => <div key={row}><span className="skeleton" /><span className="skeleton" /><span className="skeleton" /></div>)}
  </div>
}

/** The notes page before anything is known: the same heading, tools and rows the
 * real page will have, so a cold load shows one skeleton and then the list. */
export function NotesHomeSkeleton() {
  return <section className="notes-home" aria-busy="true">
    <HeaderActions><span className="avatar is-placeholder" aria-hidden="true" /></HeaderActions>
    <header className="notes-head"><div><h1>Notes</h1><p className="notes-sub">Welcome back.</p></div><div className="notes-head-actions" aria-hidden="true"><span className="workspace-trigger is-placeholder" /><span className="icon-button" /></div></header>
    <div className="notes-tools" aria-hidden="true">
      <div className="notes-search"><Search aria-hidden="true" /><input type="search" placeholder="Search titles, notes and transcripts" disabled tabIndex={-1} /></div>
      <div className="segmented is-placeholder">{FILTERS.map(option => <span className="segmented-option" key={option.value}>{option.label}</span>)}</div>
    </div>
    <p className="notes-result-line" />
    <NoteRowsSkeleton />
  </section>
}

/** Notes are listed for the active workspace; switching it is a server-side choice shared with the native apps. */
function WorkspaceSwitcher() {
  const { workspaces, switching, switchWorkspace } = useNotes()
  // Holds the trigger's place while workspaces load, so the heading row is laid out once.
  if (!workspaces) return <span className="workspace-trigger is-placeholder" aria-hidden="true" />
  if (workspaces.organizations.length === 0) return null
  const active = workspaces.organizations.find(workspace => workspace.id === workspaces.activeOrganizationId)
  return <Menu label="Workspace" align="end" buttonClassName="workspace-trigger pressable" disabled={switching}
    button={<><span className="avatar is-small" aria-hidden="true">{active ? initials(active.name) : <Building2 size={12} />}</span><span><span className="sr-only">Workspace: </span>{switching ? 'Switching…' : active?.name ?? 'Choose workspace'}</span><ChevronDown aria-hidden="true" /></>}>
    {close => <>
      <div className="menu-label">Show notes from</div>
      {workspaces.organizations.map(workspace => <MenuItem key={workspace.id} checked={workspace.id === workspaces.activeOrganizationId}
        icon={<span className="avatar is-small" aria-hidden="true">{initials(workspace.name)}</span>}
        onSelect={() => { close(); if (workspace.id !== workspaces.activeOrganizationId) void switchWorkspace(workspace.id) }}>{workspace.name}</MenuItem>)}
      <div className="menu-separator" role="separator" />
      <a href="/account?section=workspaces" className="menu-item" role="menuitem" tabIndex={-1}><Building2 aria-hidden="true" /><span>Manage workspaces</span></a>
    </>}
  </Menu>
}
