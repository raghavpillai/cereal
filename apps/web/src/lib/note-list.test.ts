import { describe, expect, test } from 'bun:test'
import type { NoteListFilter, NoteSearchPageDTO, NoteSearchResultDTO, NoteSummaryDTO } from '@meeting-notes/contracts'
import { defaultNoteView, noteFacts } from './notes'
import { createFeed, createNoteSearch, groupSummariesByDay, invitedLine, provisionalNote, searchableTerms, shortDay, summaryFacts, summaryStatus, summaryView, type FeedPage } from './note-list'

const now = new Date(2026, 10, 13, 15, 30) // Friday 13 November 2026, local time
const id = (n: number) => `00000000-0000-4000-8000-${String(n).padStart(12, '0')}`
function row(n: number, fields: Partial<NoteSummaryDTO> = {}): NoteSummaryDTO {
  return {
    id: id(n), title: `Note ${n}`, date: '2026-11-13', duration: '', createdAt: new Date(2026, 10, 13, 9, n).toISOString(), updatedAt: new Date(2026, 10, 13, 9, n).toISOString(),
    recordingState: 'idle', generationState: 'idle', transcriptRevision: 0, generatedRevision: null, hasBody: false, hasSummary: false, hasTranscript: false, speakerCount: 0,
    excerpt: '', meeting: null, attendees: { people: [], others: 0, total: 0 }, organizationId: null, folders: [], ...fields,
  }
}
const result = (n: number): NoteSearchResultDTO => ({ note: row(n), matchedIn: ['title'], title: [{ text: `Note ${n}`, hit: true }], excerpt: null })
/** A request that waits to be answered, so a test decides the order things arrive in. */
function deferred<T>() {
  let resolve!: (value: T) => void, reject!: (error: unknown) => void
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no })
  return { promise, resolve, reject }
}
const settle = () => new Promise<void>(done => setTimeout(done, 0))

describe('a paged feed', () => {
  function pages(total: number, size: number) {
    const calls: Array<{ cursor: string | undefined; signal: AbortSignal }> = []
    const load = async (cursor: string | undefined, signal: AbortSignal): Promise<FeedPage<NoteSummaryDTO>> => {
      calls.push({ cursor, signal })
      const from = cursor === undefined ? 0 : Number(cursor), items = Array.from({ length: Math.min(size, total - from) }, (_, at) => row(from + at))
      return { items, nextCursor: from + size < total ? String(from + size) : null, total }
    }
    return { load, calls }
  }

  test('loads a page at a time, in order, and stops at the end', async () => {
    const { load, calls } = pages(7, 3), feed = createFeed(load, note => note.id)
    expect(feed.get()).toMatchObject({ items: null, total: null, hasMore: false, loading: null })
    const first = feed.load()
    expect(feed.get().loading).toBe('first')
    await first
    expect(feed.get()).toMatchObject({ total: 7, hasMore: true, loading: null })
    await feed.more(); await feed.more()
    expect(feed.get().items!.map(note => note.id)).toEqual([0, 1, 2, 3, 4, 5, 6].map(id))
    expect(feed.get().hasMore).toBe(false)
    await feed.more()
    expect(calls.map(call => call.cursor)).toEqual([undefined, '3', '6'])
  })

  test('a second request for the next page while one is on its way does nothing', async () => {
    const gate = deferred<FeedPage<NoteSummaryDTO>>()
    let asked = 0
    const feed = createFeed<NoteSummaryDTO>(async cursor => cursor === undefined ? { items: [row(1)], nextCursor: 'next', total: 2 } : (asked++, gate.promise), note => note.id)
    await feed.load()
    const one = feed.more(), two = feed.more()
    expect(feed.get().loading).toBe('more')
    gate.resolve({ items: [row(2)], nextCursor: null, total: 2 })
    await Promise.all([one, two])
    expect(asked).toBe(1)
    expect(feed.get().items!.length).toBe(2)
  })

  test('a row handed over twice keeps its first place', async () => {
    const feed = createFeed<NoteSummaryDTO>(async cursor => cursor === undefined ? { items: [row(1), row(2)], nextCursor: 'b', total: 3 } : { items: [row(2), row(3)], nextCursor: null, total: 3 }, note => note.id)
    await feed.load(); await feed.more()
    expect(feed.get().items!.map(note => note.id)).toEqual([id(1), id(2), id(3)])
  })

  test('reloading cancels the page in flight, ignores its answer, and keeps the rows on screen meanwhile', async () => {
    const slow = deferred<FeedPage<NoteSummaryDTO>>(), signals: AbortSignal[] = []
    let round = 0
    const feed = createFeed<NoteSummaryDTO>(async (cursor, signal) => {
      signals.push(signal)
      if (cursor !== undefined) return slow.promise
      return round++ === 0 ? { items: [row(1)], nextCursor: 'next', total: 5 } : { items: [row(9), row(1)], nextCursor: null, total: 2 }
    }, note => note.id)
    await feed.load()
    const more = feed.more(), reload = feed.load()
    expect(signals[1]!.aborted).toBe(true)
    expect(feed.get()).toMatchObject({ loading: 'refresh', items: [row(1)] })
    await reload
    slow.resolve({ items: [row(2)], nextCursor: null, total: 5 })
    await more
    expect(feed.get()).toMatchObject({ total: 2, hasMore: false, loading: null })
    expect(feed.get().items!.map(note => note.id)).toEqual([id(9), id(1)])
  })

  test('a failed first page and a failed next page are reported apart, and both can be tried again', async () => {
    let fail = true
    const reported: unknown[] = []
    const feed = createFeed<NoteSummaryDTO>(async cursor => {
      if (fail) throw new Error('offline')
      return cursor === undefined ? { items: [row(1)], nextCursor: 'next', total: 2 } : { items: [row(2)], nextCursor: null, total: 2 }
    }, note => note.id, { onError: error => reported.push(error) })
    await feed.load()
    expect(feed.get()).toMatchObject({ items: null, loading: null })
    expect(feed.get().error).toBeInstanceOf(Error)
    fail = false
    await feed.load()
    expect(feed.get()).toMatchObject({ error: null, items: [row(1)] })
    fail = true
    await feed.more()
    expect(feed.get().moreError).toBeInstanceOf(Error)
    expect(feed.get().items!.length).toBe(1)
    fail = false
    await feed.more()
    expect(feed.get()).toMatchObject({ moreError: null, hasMore: false })
    expect(feed.get().items!.length).toBe(2)
    expect(reported.length).toBe(2)
  })

  test('a first page asked for early is used once; if it failed the feed asks again itself', async () => {
    let asked = 0
    const load = async (): Promise<FeedPage<NoteSummaryDTO>> => { asked++; return { items: [row(2)], nextCursor: null, total: 1 } }
    const seeded = createFeed(load, note => note.id, { seed: Promise.resolve({ items: [row(1)], nextCursor: null, total: 1 }) })
    await seeded.load()
    expect(seeded.get().items).toEqual([row(1)])
    expect(asked).toBe(0)
    await seeded.load()
    expect(seeded.get().items).toEqual([row(2)])
    const refused = Promise.reject(new Error('signed out')); refused.catch(() => {})
    const recovered = createFeed(load, note => note.id, { seed: refused })
    await recovered.load()
    expect(recovered.get()).toMatchObject({ error: null, items: [row(2)] })
  })

  test('cancelling stops the request and listeners hear every change', async () => {
    const gate = deferred<FeedPage<NoteSummaryDTO>>(), signals: AbortSignal[] = []
    const feed = createFeed<NoteSummaryDTO>((_, signal) => { signals.push(signal); return gate.promise }, note => note.id)
    let heard = 0
    const stop = feed.subscribe(() => { heard++ })
    const loading = feed.load()
    feed.cancel()
    expect(signals[0]!.aborted).toBe(true)
    expect(feed.get().loading).toBeNull()
    gate.resolve({ items: [row(1)], nextCursor: null, total: 1 })
    await loading
    expect(feed.get().items).toBeNull()
    expect(heard).toBe(2)
    stop()
  })
})

describe('search', () => {
  function harness() {
    const requests: Array<{ q: string; filter: NoteListFilter; cursor?: string; signal: AbortSignal; answer: ReturnType<typeof deferred<NoteSearchPageDTO>> }> = []
    let timer: { run: () => void; ms: number } | null = null, cleared = 0
    const search = createNoteSearch((input, signal) => {
      const answer = deferred<NoteSearchPageDTO>()
      requests.push({ ...input, signal, answer })
      return answer.promise
    }, { timers: { set: (run, ms) => (timer = { run, ms }), clear: () => { timer = null; cleared++ } } })
    const pause = () => { const due = timer; timer = null; due?.run() }
    const page = (ids: number[], total = ids.length, nextCursor: string | null = null): NoteSearchPageDTO => ({ results: ids.map(result), total, nextCursor, terms: [] })
    return { search, requests, pause, page, waiting: () => timer, cleared: () => cleared }
  }

  test('typing sends one request, after the pause, for what the field holds then', async () => {
    const { search, requests, pause, page, waiting } = harness()
    for (const text of ['h', 'ha', 'har', 'harr', 'harrow']) search.set(text, 'all')
    expect(requests.length).toBe(0)
    expect(waiting()!.ms).toBe(220)
    expect(search.get()).toMatchObject({ active: true, pending: true, results: null })
    pause()
    expect(requests.map(request => request.q)).toEqual(['harrow'])
    requests[0]!.answer.resolve(page([1, 2], 2))
    await settle()
    expect(search.get()).toMatchObject({ active: true, pending: false, query: 'harrow', total: 2, hasMore: false })
    expect(search.get().results!.map(found => found.note.id)).toEqual([id(1), id(2)])
  })

  test('a newer search cancels the one in flight and a late answer to the old one is dropped', async () => {
    const { search, requests, pause, page } = harness()
    search.set('harrow', 'all'); pause()
    search.set('harrowgate', 'all')
    expect(requests[0]!.signal.aborted).toBe(true)
    pause()
    expect(requests.length).toBe(2)
    requests[1]!.answer.resolve(page([7]))
    await settle()
    requests[0]!.answer.resolve(page([1, 2, 3]))
    await settle()
    expect(search.get()).toMatchObject({ query: 'harrowgate', pending: false })
    expect(search.get().results!.map(found => found.note.id)).toEqual([id(7)])
  })

  test('results stay on screen, marked as waiting, until the next ones arrive', async () => {
    const { search, requests, pause, page } = harness()
    search.set('plan', 'all'); pause(); requests[0]!.answer.resolve(page([1])); await settle()
    search.set('plan review', 'all')
    expect(search.get()).toMatchObject({ pending: true, query: 'plan' })
    expect(search.get().results!.length).toBe(1)
    pause(); requests[1]!.answer.resolve(page([])); await settle()
    expect(search.get()).toMatchObject({ pending: false, query: 'plan review', results: [], total: 0 })
  })

  test('spacing and case do not search again; going back to what is on screen needs no request', async () => {
    const { search, requests, pause, page, waiting } = harness()
    search.set('Plan', 'all'); pause(); requests[0]!.answer.resolve(page([1])); await settle()
    search.set('  plan ', 'all')
    expect(waiting()).toBeNull()
    search.set('plans', 'all')
    expect(waiting()).not.toBeNull()
    search.set('plan', 'all')
    expect(waiting()).toBeNull()
    expect(search.get()).toMatchObject({ pending: false, query: 'Plan' })
    expect(requests.length).toBe(1)
    // The filter is part of the question.
    search.set('plan', 'summarized'); pause()
    expect(requests.map(request => request.filter)).toEqual(['all', 'summarized'])
  })

  test('an empty or unsearchable field cancels everything and shows the list again', async () => {
    const { search, requests, pause, page, waiting } = harness()
    search.set('plan', 'all'); pause(); requests[0]!.answer.resolve(page([1])); await settle()
    search.set('planning', 'all'); pause()
    search.set('p', 'all')
    expect(requests[1]!.signal.aborted).toBe(true)
    expect(search.get()).toEqual({ active: false, query: '', results: null, total: null, hasMore: false, pending: false, loadingMore: false, error: null, moreError: null })
    requests[1]!.answer.resolve(page([5])); await settle()
    expect(search.get().active).toBe(false)
    search.set('pl', 'all'); search.set('', 'all')
    expect(waiting()).toBeNull()
    expect(requests.length).toBe(2)
  })

  test('further pages follow the same query, without duplicates, and stop for a newer search', async () => {
    const { search, requests, pause, page } = harness()
    search.set('plan', 'all'); pause(); requests[0]!.answer.resolve(page([1, 2], 5, 'c1')); await settle()
    expect(search.get().hasMore).toBe(true)
    const more = search.more(), again = search.more()
    expect(search.get().loadingMore).toBe(true)
    expect(requests[1]).toMatchObject({ q: 'plan', cursor: 'c1' })
    requests[1]!.answer.resolve(page([2, 3], 5, 'c2')); await Promise.all([more, again])
    expect(requests.length).toBe(2)
    expect(search.get().results!.map(found => found.note.id)).toEqual([id(1), id(2), id(3)])
    const last = search.more()
    search.set('budget', 'all')
    expect(requests[2]!.signal.aborted).toBe(true)
    expect(search.get()).toMatchObject({ loadingMore: false, pending: true })
    requests[2]!.answer.resolve(page([4])); await last
    expect(search.get().results!.length).toBe(3)
    await search.more()
    expect(requests.length).toBe(3)
  })

  test('a failed search keeps the earlier results and can be tried again at once', async () => {
    const reported: unknown[] = [], requests: Array<ReturnType<typeof deferred<NoteSearchPageDTO>>> = []
    let due: (() => void) | null = null
    const search = createNoteSearch(() => { const answer = deferred<NoteSearchPageDTO>(); requests.push(answer); return answer.promise }, { onError: error => reported.push(error), timers: { set: run => (due = run), clear: () => { due = null } } })
    search.set('plan', 'all'); due!(); requests[0]!.resolve({ results: [result(1)], total: 1, nextCursor: null, terms: ['plan'] }); await settle()
    search.set('plan b2', 'all'); due!(); requests[1]!.reject(new Error('offline')); await settle()
    expect(search.get()).toMatchObject({ pending: false, query: 'plan' })
    expect(search.get().error).toBeInstanceOf(Error)
    expect(search.get().results!.length).toBe(1)
    expect(reported.length).toBe(1)
    search.retry()
    expect(search.get()).toMatchObject({ pending: true, error: null })
    requests[2]!.resolve({ results: [], total: 0, nextCursor: null, terms: ['plan', 'b2'] }); await settle()
    expect(search.get()).toMatchObject({ query: 'plan b2', results: [], error: null })
  })

  test('the words searched for are the server’s', () => {
    expect(searchableTerms("Ada's e-mail ada@example.test v2.1, 日 a")).toEqual(["ada's", 'e-mail', 'ada@example.test', 'v2.1', '日'])
    expect(searchableTerms('  Hiring   PLAN ')).toEqual(['hiring', 'plan'])
    expect(searchableTerms('one two three four five six seven eight nine ten').length).toBe(8)
    for (const nothing of ['', ' ', 'a', '%', '_ -']) expect(searchableTerms(nothing)).toEqual([])
  })
})

describe('what a row shows', () => {
  test('facts come from the row alone and are computed once', () => {
    const plain = row(1, { title: '  ' }), rich = row(2, { duration: ' 31:07 ', speakerCount: 3, hasSummary: true, hasTranscript: true, generationState: 'ready', transcriptRevision: 2, generatedRevision: 1 })
    expect(summaryFacts(plain)).toMatchObject({ title: 'Untitled note', details: ['Empty note'], status: null, invited: '' })
    expect(summaryFacts(row(3, { hasBody: true })).details).toEqual(['Written note'])
    expect(summaryFacts(rich)).toMatchObject({ details: ['31:07', '3 speakers'], status: { label: 'Summary out of date' } })
    expect(summaryFacts(row(4, { speakerCount: 1 })).details).toEqual(['1 speaker'])
    expect(summaryFacts(rich)).toBe(summaryFacts(rich))
    expect(summaryStatus(row(5, { recordingState: 'recording' }))?.tone).toBe('live')
    expect(summaryStatus(row(6, { generationState: 'unsupported' }))?.label).toBe('No summary')
    expect(summaryStatus(row(7, { generationState: 'unsupported', hasSummary: true }))).toBeNull()
  })

  test('who was invited: the first names that fit, then how many more', () => {
    const person = (shortName: string) => ({ name: `${shortName} Surname`, shortName, initials: shortName[0]! })
    expect(invitedLine(row(1))).toBe('')
    expect(invitedLine(row(1, { attendees: { people: [person('Ada')], others: 1, total: 2 } }))).toBe('Ada')
    expect(invitedLine(row(1, { attendees: { people: [person('Ada'), person('Grace')], others: 2, total: 3 } }))).toBe('Ada, Grace')
    expect(invitedLine(row(1, { attendees: { people: [person('Ada'), person('Grace'), person('Alan'), person('Kay')], others: 9, total: 10 } }))).toBe('Ada, Grace +7')
  })

  test('days are runs of neighbouring rows, in the order the server sent them', () => {
    const rows = [row(5, { date: '2026-11-13' }), row(1, { date: '2026-11-13' }), row(9, { date: '2026-11-12' }), row(3, { date: '2025-12-04' })]
    const groups = groupSummariesByDay(rows, now, 'en-GB')
    expect(groups.map(group => group.label)).toEqual(['Today', 'Yesterday', 'Thu, 4 Dec 2025'])
    expect(groups.map(group => group.notes.map(note => note.id))).toEqual([[id(5), id(1)], [id(9)], [id(3)]])
    expect(groupSummariesByDay([], now)).toEqual([])
    expect([shortDay('2026-11-13', now, 'en-GB'), shortDay('2026-11-12', now, 'en-GB'), shortDay('2026-03-02', now, 'en-GB'), shortDay('2025-12-04', now, 'en-GB'), shortDay('nope', now)]).toEqual(['Today', 'Yesterday', '2 Mar', 'Dec 2025', ''])
  })

  test('a note opens from its row on the view it will have, with no text yet', () => {
    expect([summaryView(row(1, { hasSummary: true, hasTranscript: true })), summaryView(row(1, { hasBody: true, hasTranscript: true })), summaryView(row(1, { hasTranscript: true })), summaryView(row(1))]).toEqual(['summary', 'notes', 'transcript', 'summary'])
    const source = row(8, { title: 'Pricing', duration: '12:00', hasSummary: true, speakerCount: 2, recordingState: 'stopped', transcriptRevision: 3, generatedRevision: 3 }), note = provisionalNote(source)
    expect(note).toMatchObject({ id: id(8), title: 'Pricing', duration: '12:00', body: '', enhanced: '', transcriptSegments: [], recordingState: 'stopped', transcriptRevision: 3, generatedRevision: 3 })
    expect(noteFacts(note).title).toBe('Pricing')
    expect(defaultNoteView(note)).toBe('summary')
  })
})
