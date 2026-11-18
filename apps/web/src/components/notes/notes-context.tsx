import { createContext, useCallback, useContext, useEffect, useMemo, useState, useSyncExternalStore, type ReactNode } from 'react'
import type { AuthSessionDTO, MeetingContextDTO, NoteDTO, NoteListFilter, NoteSummariesPageDTO, NoteSummaryDTO } from '@meeting-notes/contracts'
import { AdminClientError, type AdminClient } from '../../lib/admin-client'
import { PAGE, createFeed, createNoteSearch, type Feed, type FeedPage, type NoteSearch } from '../../lib/note-list'

/** What changes rarely: who is signed in and their notes. */
export interface NotesContextValue {
  session: AuthSessionDTO
  client: AdminClient
  expired: () => void
  notes: NotesStore
  meetings: MeetingStore
  morph: MorphStore
  /** Say that something has just been done ("Note moved to trash"), in the corner, for a few seconds. */
  notify: (message: string) => void
}

/** What changes with every keystroke. Kept apart so typing re-renders the list and nothing else. */
export interface NotesSearchValue {
  query: string
  setQuery: (query: string) => void
}

/** The web lists every note of the active workspace; the server's narrower filters are not offered. */
export const LISTED: NoteListFilter = 'all'

const NotesContext = createContext<NotesContextValue | null>(null)
const NotesSearchContext = createContext<NotesSearchValue | null>(null)

export function useNotes(): NotesContextValue {
  const value = useContext(NotesContext)
  if (!value) throw new Error('Notes pages render inside NotesProvider')
  return value
}
export function useNotesSearch(): NotesSearchValue {
  const value = useContext(NotesSearchContext)
  if (!value) throw new Error('Notes pages render inside NotesProvider')
  return value
}

export function errorText(error: unknown): string {
  return error instanceof AdminClientError ? error.message : 'The request could not finish. Please try again.'
}

// ── The list, the search and the notes opened so far ───────────────────────

/** Full notes kept so that going back to one shows it at once. Rows never hold a note's text. */
const NOTES_KEPT = 40

export interface NotesStore {
  /** The rows for a filter, a page at a time. Each filter keeps its rows for the session. */
  list: (filter: NoteListFilter) => Feed<NoteSummaryDTO>
  /** Load a filter's first page if it has never loaded, or again if a refresh has made it out of date. */
  show: (filter: NoteListFilter) => void
  /** Reload the filter on screen from the top and repeat the search; other filters reload when next shown. */
  reload: (filter: NoteListFilter) => Promise<void>
  search: NoteSearch
  /** A note already read in this session. */
  full: (id: string) => NoteDTO | undefined
  /** The row of a note, from whichever list or search has it. */
  summary: (id: string) => NoteSummaryDTO | undefined
  /** Read one note. A request already under way for it is shared, so a press and the click after it ask once.
   * It is not cancelled when one asker leaves: it is small, and its answer is kept for the next visit. */
  read: (id: string) => Promise<NoteDTO>
  /** The notes in Trash, a page at a time. The lists and the search above never hold them. */
  trash: Feed<NoteSummaryDTO>
  /** Read Trash from the top: on every visit, since a note may have been put in it or taken out in one of the apps. */
  showTrash: () => void
  /** Put a note in Trash. It leaves the lists and the search at once, and is in Trash when that is next shown. */
  moveToTrash: (id: string) => Promise<void>
  /** Take a note out of Trash. It leaves Trash at once, and is back among the notes when they are next shown. */
  restore: (id: string) => Promise<void>
  /** Delete a note that is in Trash, for good. */
  deleteForever: (id: string) => Promise<void>
  dispose: () => void
}

const page = (response: NoteSummariesPageDTO): FeedPage<NoteSummaryDTO> => ({ items: response.notes, nextCursor: response.nextCursor, total: response.total })

/** Where a note is listed: among the notes, or in Trash. */
type Place = 'notes' | 'trash'

/** What the store asks of the server: the calls of `AdminClient` it makes, so a test can stand in for them. */
export type NotesTransport = Pick<AdminClient, 'listNoteSummaries' | 'searchNotes' | 'getNote' | 'setNoteTrashed' | 'deleteNote'>

export function createNotesStore(client: NotesTransport, onError: (error: unknown) => void): NotesStore {
  const feeds = new Map<NoteListFilter, Feed<NoteSummaryDTO>>(), stale = new Set<NoteListFilter>()
  const kept = new Map<string, NoteDTO>(), reading = new Map<string, Promise<NoteDTO>>()
  // Where this session has put notes, in order. A page asked for before a move and answered after it would bring
  // the note back to the place it has just left; such a row is dropped as the page arrives. A page asked for
  // after the move is the server's word and is taken as it is.
  const moved = new Map<string, { to: Place | 'gone'; at: number }>()
  let moves = 0
  const move = (id: string, to: Place | 'gone') => { moved.set(id, { to, at: ++moves }) }
  /** Whether a row of a page asked for at `asked` still belongs in `place`. */
  const still = (place: Place, asked: number) => (note: NoteSummaryDTO) => { const last = moved.get(note.id); return !last || last.at <= asked || last.to === place }
  const rows = (place: Place, asked: number) => (response: NoteSummariesPageDTO) => page({ ...response, notes: response.notes.filter(still(place, asked)) })
  const search = createNoteSearch(async (input, signal) => {
    const asked = moves, found = await client.searchNotes({ ...input, trash: 'exclude' }, signal), belongs = still('notes', asked)
    return { ...found, results: found.results.filter(result => belongs(result.note)) }
  }, { onError })
  const trash = createFeed<NoteSummaryDTO>((cursor, signal) => client.listNoteSummaries({ limit: PAGE, trash: 'only', ...(cursor === undefined ? {} : { cursor }) }, signal).then(rows('trash', moves)), note => note.id, { onError })
  function list(filter: NoteListFilter) {
    let feed = feeds.get(filter)
    if (!feed) {
      const early = filter === 'all' ? earlyNotes.get(client) : undefined
      if (early) earlyNotes.delete(client)
      feed = createFeed<NoteSummaryDTO>((cursor, signal) => client.listNoteSummaries({ limit: PAGE, filter, trash: 'exclude', ...(cursor === undefined ? {} : { cursor }) }, signal).then(rows('notes', moves)), note => note.id, { ...(early ? { seed: early.then(rows('notes', 0)) } : {}), onError })
      feeds.set(filter, feed)
    }
    return feed
  }
  /** The lists and the search no longer show what they would be given now: they reload when next shown. */
  const notesChanged = () => { for (const filter of feeds.keys()) stale.add(filter); search.retry() }
  return {
    list, search, trash,
    showTrash() { void trash.load() },
    async moveToTrash(id) {
      await client.setNoteTrashed(id, true)
      move(id, 'trash')
      feeds.forEach(feed => feed.remove(id))
      search.remove(id)
    },
    async restore(id) {
      await client.setNoteTrashed(id, false)
      move(id, 'notes')
      trash.remove(id)
      notesChanged()
    },
    async deleteForever(id) {
      await client.deleteNote(id)
      move(id, 'gone')
      trash.remove(id)
      kept.delete(id)
    },
    show(filter) {
      const feed = list(filter), state = feed.get()
      if (stale.delete(filter) || (!state.items && !state.loading && !state.error)) void feed.load()
    },
    async reload(filter) {
      for (const other of feeds.keys()) if (other !== filter) stale.add(other)
      search.retry()
      await list(filter).load()
    },
    full: id => kept.get(id),
    summary(id) {
      for (const feed of [...feeds.values(), trash]) { const found = feed.get().items?.find(note => note.id === id); if (found) return found }
      return search.get().results?.find(result => result.note.id === id)?.note
    },
    read(id) {
      const under = reading.get(id)
      if (under) return under
      const request = client.getNote(id).then(note => {
        kept.delete(id); kept.set(id, note)
        for (const oldest of kept.keys()) { if (kept.size <= NOTES_KEPT) break; kept.delete(oldest) }
        return note
      }).finally(() => { if (reading.get(id) === request) reading.delete(id) })
      reading.set(id, request)
      return request
    },
    dispose() { feeds.forEach(feed => feed.cancel()); trash.cancel(); search.cancel() },
  }
}

// ── Meeting context, one subscription per note ─────────────────────────────

/** Saved meeting context per note: undefined until known, null when the note has none.
 * The note page fills it in; list rows take who was invited from their own summary. */
export interface MeetingStore {
  get: (noteId: string) => MeetingContextDTO | null | undefined
  set: (noteId: string, meeting: MeetingContextDTO | null) => void
  subscribe: (noteId: string, listener: () => void) => () => void
}

function createMeetingStore(): MeetingStore {
  const known = new Map<string, MeetingContextDTO | null>(), listeners = new Map<string, Set<() => void>>()
  return {
    get: id => known.get(id),
    set(id, meeting) {
      if (known.has(id) && known.get(id) === meeting) return
      known.set(id, meeting)
      listeners.get(id)?.forEach(listener => listener())
    },
    subscribe(id, listener) {
      let group = listeners.get(id)
      if (!group) listeners.set(id, group = new Set())
      group.add(listener)
      return () => { group.delete(listener); if (!group.size) listeners.delete(id) }
    },
  }
}

export function useMeeting(noteId: string): MeetingContextDTO | null | undefined {
  const { meetings } = useNotes()
  return useSyncExternalStore(useCallback(listener => meetings.subscribe(noteId, listener), [meetings, noteId]), () => meetings.get(noteId))
}

// ── The note last opened, so that going back returns focus to its row ──────

export interface MorphStore { get: () => string | null; set: (id: string | null) => void }
function createMorphStore(): MorphStore {
  let current: string | null = null
  return { get: () => current, set(id) { current = id } }
}

// ── A list request started alongside the session check ─────────────────────

const earlyNotes = new WeakMap<object, Promise<NoteSummariesPageDTO>>()
/** Start loading the first page before the session check has answered: both use
 * the same cookie, so a signed-in visit saves a round trip and a signed-out one
 * just gets a refusal that is dropped here. Used once, then forgotten. */
export function preloadNotes(client: AdminClient) {
  const request = client.listNoteSummaries({ limit: PAGE, trash: 'exclude' })
  request.catch(() => { if (earlyNotes.get(client) === request) earlyNotes.delete(client) })
  earlyNotes.set(client, request)
}

/** How long a notice stays: the desktop app's four seconds. */
const TOAST_MS = 4000

/** Holds one signed-in session's list rows and opened notes for the list and the
 * note page, so opening a note and going back never refetch or flash a loading state. */
export function NotesProvider({ session, client, expired, children }: { session: AuthSessionDTO; client: AdminClient; expired: () => void; children: ReactNode }) {
  const signedOut = useCallback((error: unknown) => { if (error instanceof AdminClientError && error.status === 401) expired() }, [expired])
  const notes = useMemo(() => createNotesStore(client, signedOut), [client, signedOut])
  useEffect(() => () => notes.dispose(), [notes])
  const [query, setQuery] = useState('')
  const meetings = useMemo(createMeetingStore, [client])
  const morph = useMemo(createMorphStore, [])
  // One notice at a time; a second one replaces the first and starts its own four seconds, as in the desktop app.
  const [toast, setToast] = useState<{ message: string; key: number } | null>(null)
  const notify = useCallback((message: string) => setToast(old => ({ message, key: (old?.key ?? 0) + 1 })), [])
  useEffect(() => { if (!toast) return; const timer = window.setTimeout(() => setToast(null), TOAST_MS); return () => window.clearTimeout(timer) }, [toast])
  const value = useMemo<NotesContextValue>(() => ({ session, client, expired, notes, meetings, morph, notify }), [session, client, expired, notes, meetings, morph, notify])
  const search = useMemo<NotesSearchValue>(() => ({ query, setQuery }), [query])
  return <NotesContext value={value}><NotesSearchContext value={search}>
    {children}
    <div className="app-toast-place" role="status" aria-live="polite">{toast && <div className="app-toast" key={toast.key}>{toast.message}</div>}</div>
  </NotesSearchContext></NotesContext>
}
