import type { NoteDTO, NoteListFilter, NoteSearchPageDTO, NoteSearchResultDTO, NoteSummaryDTO } from '@meeting-notes/contracts'
import { dateFormatter, dayLabel, dayOffset, noteStatus, noteTime, noteTitle, type NoteStatus, type NoteView } from './notes'

/** The notes list as the server pages it: rows without bodies or transcripts,
 * loaded a page at a time, and a search that runs on the server. No DOM here. */

/** Rows asked for at a time. The first page is what a cold load transfers. */
export const PAGE = 50
/** How long typing must pause before a search is sent. */
export const SEARCH_DELAY = 220

// ── A paged feed ───────────────────────────────────────────────────────────

export interface FeedPage<T> { items: readonly T[]; nextCursor: string | null; total: number }
export interface FeedState<T> {
  /** Null until the first page has arrived. */
  items: readonly T[] | null
  total: number | null
  hasMore: boolean
  /** `first`: nothing to show yet. `refresh`: reloading from the top under the rows already shown. `more`: the next page. */
  loading: 'first' | 'refresh' | 'more' | null
  /** The first page or a refresh failed. Rows already shown stay. */
  error: unknown
  /** The next page failed; `more()` tries it again. */
  moreError: unknown
}
export interface Feed<T> {
  get: () => FeedState<T>
  subscribe: (listener: () => void) => () => void
  /** Load from the top. Whatever was in flight is cancelled and its answer ignored. */
  load: () => Promise<void>
  /** Load the next page, unless one is already on its way or there is none. */
  more: () => Promise<void>
  cancel: () => void
}
type PageLoader<T> = (cursor: string | undefined, signal: AbortSignal) => Promise<FeedPage<T>>

/** `seed` is a first page someone already asked for; it is used once, by the first `load`. */
export function createFeed<T>(loadPage: PageLoader<T>, idOf: (item: T) => string, options: { seed?: Promise<FeedPage<T>>; onError?: (error: unknown) => void } = {}): Feed<T> {
  let state: FeedState<T> = { items: null, total: null, hasMore: false, loading: null, error: null, moreError: null }
  let cursor: string | null = null, controller: AbortController | null = null, seed = options.seed
  const listeners = new Set<() => void>()
  const set = (next: Partial<FeedState<T>>) => { state = { ...state, ...next }; listeners.forEach(listener => listener()) }
  const begin = () => { controller?.abort(); return controller = new AbortController() }
  return {
    get: () => state,
    subscribe(listener) { listeners.add(listener); return () => { listeners.delete(listener) } },
    cancel() { controller?.abort(); controller = null; if (state.loading) set({ loading: null }) },
    async load() {
      const { signal } = begin(), early = seed
      seed = undefined
      set({ loading: state.items ? 'refresh' : 'first', error: null, moreError: null })
      try {
        const page = await (early ? early.catch(() => loadPage(undefined, signal)) : loadPage(undefined, signal))
        if (signal.aborted) return
        cursor = page.nextCursor
        set({ items: page.items, total: page.total, hasMore: page.nextCursor !== null, loading: null })
      } catch (error) {
        if (signal.aborted) return
        set({ loading: null, error })
        options.onError?.(error)
      }
    },
    async more() {
      if (state.loading || !state.items || cursor === null) return
      const { signal } = begin(), from = cursor
      set({ loading: 'more', moreError: null })
      try {
        const page = await loadPage(from, signal)
        if (signal.aborted) return
        // A note whose date was edited between two requests can be handed over twice. It keeps its first place.
        const seen = new Set(state.items.map(idOf))
        cursor = page.nextCursor
        set({ items: [...state.items, ...page.items.filter(item => !seen.has(idOf(item)))], total: page.total, hasMore: page.nextCursor !== null, loading: null })
      } catch (error) {
        if (signal.aborted) return
        set({ loading: null, moreError: error })
        options.onError?.(error)
      }
    },
  }
}

// ── Search ─────────────────────────────────────────────────────────────────

/** The words the server will look for, by its rules: up to eight, and a lone
 * ASCII letter or digit is not one. An empty answer means there is nothing to send. */
export function searchableTerms(query: string): string[] {
  const words = [...query.normalize('NFKC').toLowerCase().matchAll(/[\p{L}\p{N}][\p{L}\p{N}'’._@+-]*/gu)].map(match => match[0].replace(/['’._@+-]+$/, '').slice(0, 80))
  return [...new Set(words.filter(word => word.length > 1 || /[^\x00-\x7f]/.test(word)))].slice(0, 8)
}

export interface SearchState {
  /** True while the field holds something searchable; the list then shows results, not every note. */
  active: boolean
  /** The query the results on screen answer. It trails the field while a newer search is waiting or in flight. */
  query: string
  results: readonly NoteSearchResultDTO[] | null
  total: number | null
  hasMore: boolean
  /** A newer search than the one on screen is waiting out the pause or on its way. */
  pending: boolean
  loadingMore: boolean
  error: unknown
  moreError: unknown
}
export interface NoteSearch {
  get: () => SearchState
  subscribe: (listener: () => void) => () => void
  /** Tell the search what the field and filter now hold. Requests wait for a pause in typing; an answer to anything but the latest is dropped. */
  set: (query: string, filter: NoteListFilter) => void
  more: () => Promise<void>
  /** Ask again now: after an error, or to refresh. */
  retry: () => void
  cancel: () => void
}
type SearchLoader = (input: { q: string; filter: NoteListFilter; cursor?: string }, signal: AbortSignal) => Promise<NoteSearchPageDTO>
interface Timers { set: (run: () => void, ms: number) => unknown; clear: (handle: unknown) => void }
const realTimers: Timers = { set: (run, ms) => setTimeout(run, ms), clear: handle => clearTimeout(handle as ReturnType<typeof setTimeout>) }
const idle: SearchState = { active: false, query: '', results: null, total: null, hasMore: false, pending: false, loadingMore: false, error: null, moreError: null }

export function createNoteSearch(load: SearchLoader, options: { delay?: number; timers?: Timers; onError?: (error: unknown) => void } = {}): NoteSearch {
  const delay = options.delay ?? SEARCH_DELAY, timers = options.timers ?? realTimers
  let state = idle, wanted: { q: string; filter: NoteListFilter; key: string } | null = null
  let timer: unknown = null, controller: AbortController | null = null, cursor: string | null = null, shownKey: string | null = null
  const listeners = new Set<() => void>()
  const set = (next: Partial<SearchState>) => { state = { ...state, ...next }; listeners.forEach(listener => listener()) }
  const stop = () => { if (timer !== null) { timers.clear(timer); timer = null } controller?.abort(); controller = null }
  async function run() {
    const target = wanted
    if (!target) return
    timer = null
    controller?.abort()
    const { signal } = controller = new AbortController()
    set({ pending: true, loadingMore: false, error: null, moreError: null })
    try {
      const page = await load({ q: target.q, filter: target.filter }, signal)
      if (signal.aborted) return
      cursor = page.nextCursor; shownKey = target.key
      set({ query: target.q, results: page.results, total: page.total, hasMore: page.nextCursor !== null, pending: false })
    } catch (error) {
      if (signal.aborted) return
      set({ pending: false, error })
      options.onError?.(error)
    }
  }
  return {
    get: () => state,
    subscribe(listener) { listeners.add(listener); return () => { listeners.delete(listener) } },
    set(query, filter) {
      const terms = searchableTerms(query)
      if (!terms.length) {
        wanted = null; shownKey = null; cursor = null
        stop()
        if (state !== idle) { state = idle; listeners.forEach(listener => listener()) }
        return
      }
      // Spacing and letter case do not make a new search.
      const key = `${filter}\n${terms.join(' ')}`
      if (wanted?.key === key) return
      wanted = { q: query.trim(), filter, key }
      stop()
      if (key === shownKey) { set({ active: true, pending: false, loadingMore: false, error: null }); return }
      set({ active: true, pending: true, loadingMore: false, error: null })
      timer = timers.set(() => void run(), delay)
    },
    retry() { if (wanted) { stop(); void run() } },
    cancel() { stop(); if (state.pending || state.loadingMore) set({ pending: false, loadingMore: false }) },
    async more() {
      const target = wanted
      if (!target || state.pending || state.loadingMore || !state.results || cursor === null || shownKey !== target.key) return
      controller?.abort()
      const { signal } = controller = new AbortController(), from = cursor
      set({ loadingMore: true, moreError: null })
      try {
        const page = await load({ q: target.q, filter: target.filter, cursor: from }, signal)
        if (signal.aborted) return
        const seen = new Set(state.results.map(result => result.note.id))
        cursor = page.nextCursor
        set({ results: [...state.results, ...page.results.filter(result => !seen.has(result.note.id))], total: page.total, hasMore: page.nextCursor !== null, loadingMore: false })
      } catch (error) {
        if (signal.aborted) return
        set({ loadingMore: false, moreError: error })
        options.onError?.(error)
      }
    },
  }
}

// ── What a row shows ───────────────────────────────────────────────────────

export interface SummaryFacts { title: string; time: string | null; status: NoteStatus | null; details: string[]; invited: string }
const factsCache = new WeakMap<NoteSummaryDTO, SummaryFacts>()
/** Derived once per row object; a re-render of the list reads the cached value. */
export function summaryFacts(note: NoteSummaryDTO): SummaryFacts {
  let facts = factsCache.get(note)
  if (!facts) {
    const details = [note.duration.trim(), note.speakerCount > 0 ? `${note.speakerCount} ${note.speakerCount === 1 ? 'speaker' : 'speakers'}` : ''].filter(Boolean)
    if (!details.length) details.push(note.hasSummary || note.hasBody ? 'Written note' : 'Empty note')
    facts = { title: noteTitle(note), time: noteTime(note), status: summaryStatus(note), details, invited: invitedLine(note) }
    factsCache.set(note, facts)
  }
  return facts
}
/** The status rules read a summary's text only to ask whether there is any. */
export function summaryStatus(note: NoteSummaryDTO): NoteStatus | null {
  return noteStatus({ recordingState: note.recordingState, generationState: note.generationState, transcriptRevision: note.transcriptRevision, generatedRevision: note.generatedRevision, enhanced: note.hasSummary ? 'summary' : '' })
}
/** "Ada, Grace +2": the first names that fit, then how many more were invited. */
export function invitedLine(note: Pick<NoteSummaryDTO, 'attendees'>, shown = 2): string {
  const names = note.attendees.people.slice(0, shown).map(person => person.shortName)
  if (!names.length) return ''
  return note.attendees.others > names.length ? `${names.join(', ')} +${note.attendees.others - names.length}` : names.join(', ')
}

export interface SummaryGroup { key: string; label: string; notes: NoteSummaryDTO[] }
/** Rows arrive newest first, so a day is a run of neighbours. Their order is the server's and is never changed here. */
export function groupSummariesByDay(notes: readonly NoteSummaryDTO[], now: Date, locale?: string): SummaryGroup[] {
  const groups: SummaryGroup[] = []
  for (const note of notes) {
    const last = groups[groups.length - 1]
    if (last?.key === note.date) last.notes.push(note)
    else groups.push({ key: note.date, label: dayLabel(note.date, now, locale), notes: [note] })
  }
  return groups
}
/** A search result is not under a day heading, so it carries its own day, short enough for the time column. */
export function shortDay(key: string, now: Date, locale?: string): string {
  const offset = dayOffset(key, now)
  if (offset === null) return ''
  if (offset === 0) return 'Today'
  if (offset === -1) return 'Yesterday'
  const day = new Date(Number(key.slice(0, 4)), Number(key.slice(5, 7)) - 1, Number(key.slice(8, 10)), 12)
  return day.getFullYear() === now.getFullYear()
    ? dateFormatter(locale, 'short-day', { day: 'numeric', month: 'short' }).format(day)
    : dateFormatter(locale, 'month-year', { month: 'short', year: 'numeric' }).format(day)
}

// ── Opening a note from its row ────────────────────────────────────────────

/** The view a note will open on, known before its text has loaded. */
export function summaryView(note: Pick<NoteSummaryDTO, 'hasSummary' | 'hasBody' | 'hasTranscript'>): NoteView {
  return note.hasSummary ? 'summary' : note.hasBody ? 'notes' : note.hasTranscript ? 'transcript' : 'summary'
}
/** A note with everything the masthead shows and no text yet, so the page can open from its row while the note loads. */
export function provisionalNote(note: NoteSummaryDTO): NoteDTO {
  return {
    id: note.id, title: note.title, body: '', enhanced: '', date: note.date, duration: note.duration, createdAt: note.createdAt, updatedAt: note.updatedAt,
    transcriptSegments: [], recordingState: note.recordingState, generationState: note.generationState, transcriptRevision: note.transcriptRevision, generatedRevision: note.generatedRevision,
  }
}
