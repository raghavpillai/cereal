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
  dispose: () => void
}

const page = (response: NoteSummariesPageDTO): FeedPage<NoteSummaryDTO> => ({ items: response.notes, nextCursor: response.nextCursor, total: response.total })

function createNotesStore(client: AdminClient, onError: (error: unknown) => void): NotesStore {
  const feeds = new Map<NoteListFilter, Feed<NoteSummaryDTO>>(), stale = new Set<NoteListFilter>()
  const kept = new Map<string, NoteDTO>(), reading = new Map<string, Promise<NoteDTO>>()
  const search = createNoteSearch((input, signal) => client.searchNotes(input, signal), { onError })
  function list(filter: NoteListFilter) {
    let feed = feeds.get(filter)
    if (!feed) {
      const early = filter === 'all' ? earlyNotes.get(client) : undefined
      if (early) earlyNotes.delete(client)
      feed = createFeed<NoteSummaryDTO>((cursor, signal) => client.listNoteSummaries({ limit: PAGE, filter, ...(cursor === undefined ? {} : { cursor }) }, signal).then(page), note => note.id, { ...(early ? { seed: early } : {}), onError })
      feeds.set(filter, feed)
    }
    return feed
  }
  return {
    list, search,
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
      for (const feed of feeds.values()) { const found = feed.get().items?.find(note => note.id === id); if (found) return found }
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
    dispose() { feeds.forEach(feed => feed.cancel()); search.cancel() },
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

const earlyNotes = new WeakMap<AdminClient, Promise<FeedPage<NoteSummaryDTO>>>()
/** Start loading the first page before the session check has answered: both use
 * the same cookie, so a signed-in visit saves a round trip and a signed-out one
 * just gets a refusal that is dropped here. Used once, then forgotten. */
export function preloadNotes(client: AdminClient) {
  const request = client.listNoteSummaries({ limit: PAGE }).then(page)
  request.catch(() => { if (earlyNotes.get(client) === request) earlyNotes.delete(client) })
  earlyNotes.set(client, request)
}

/** Holds one signed-in session's list rows and opened notes for the list and the
 * note page, so opening a note and going back never refetch or flash a loading state. */
export function NotesProvider({ session, client, expired, children }: { session: AuthSessionDTO; client: AdminClient; expired: () => void; children: ReactNode }) {
  const signedOut = useCallback((error: unknown) => { if (error instanceof AdminClientError && error.status === 401) expired() }, [expired])
  const notes = useMemo(() => createNotesStore(client, signedOut), [client, signedOut])
  useEffect(() => () => notes.dispose(), [notes])
  const [query, setQuery] = useState('')
  const meetings = useMemo(createMeetingStore, [client])
  const morph = useMemo(createMorphStore, [])
  const value = useMemo<NotesContextValue>(() => ({ session, client, expired, notes, meetings, morph }), [session, client, expired, notes, meetings, morph])
  const search = useMemo<NotesSearchValue>(() => ({ query, setQuery }), [query])
  return <NotesContext value={value}><NotesSearchContext value={search}>{children}</NotesSearchContext></NotesContext>
}
