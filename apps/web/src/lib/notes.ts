import type { MeetingParticipantDTO, NoteDTO, TranscriptSegmentDTO } from '@meeting-notes/contracts'

/** Presentation logic for the notes reader. Pure functions: no DOM, no network. */

export type NoteView = 'summary' | 'notes' | 'transcript'
export type NoteFilter = 'all' | 'summarized' | 'transcribed'

const formatters = new Map<string, Intl.DateTimeFormat>()
/** Constructing a formatter costs far more than using one, and rows ask for the same few shapes. */
export function dateFormatter(locale: string | undefined, name: string, options: Intl.DateTimeFormatOptions): Intl.DateTimeFormat {
  const key = `${locale ?? ''}|${name}`
  let formatter = formatters.get(key)
  if (!formatter) { formatter = new Intl.DateTimeFormat(locale, options); formatters.set(key, formatter) }
  return formatter
}

export function localDateKey(date: Date): string {
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`
}

/** A note's calendar date is a plain day, so it is read at local noon and never shifts across time zones. */
function dayAtNoon(key: string): Date | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(key)
  if (!match) return null
  const date = new Date(Number(match[1]), Number(match[2]) - 1, Number(match[3]), 12)
  return localDateKey(date) === key ? date : null
}

/** Whole days from `today` to `key`: 0 today, -1 yesterday, 1 tomorrow. */
export function dayOffset(key: string, now: Date): number | null {
  const day = dayAtNoon(key)
  if (!day) return null
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate(), 12)
  return Math.round((day.getTime() - today.getTime()) / 86_400_000)
}

/** "Today", "Yesterday", then "Wed 11 Nov" like the native apps; other years add the year. */
export function dayLabel(key: string, now: Date, locale?: string): string {
  const day = dayAtNoon(key), offset = dayOffset(key, now)
  if (!day || offset === null) return 'Undated'
  if (offset === 0) return 'Today'
  if (offset === -1) return 'Yesterday'
  if (offset === 1) return 'Tomorrow'
  return day.getFullYear() === now.getFullYear()
    ? dateFormatter(locale, 'day', { weekday: 'short', day: 'numeric', month: 'short' }).format(day)
    : dateFormatter(locale, 'day-year', { weekday: 'short', day: 'numeric', month: 'short', year: 'numeric' }).format(day)
}

export function longDayLabel(key: string, locale?: string): string {
  const day = dayAtNoon(key)
  return day ? dateFormatter(locale, 'long-day', { weekday: 'long', day: 'numeric', month: 'long', year: 'numeric' }).format(day) : ''
}

export interface DayGroup { key: string; label: string; notes: NoteDTO[] }

/** Newest day first; within a day, the most recently created note first. */
export function groupNotesByDay(notes: readonly NoteDTO[], now: Date, locale?: string): DayGroup[] {
  const groups = new Map<string, NoteDTO[]>()
  for (const note of notes) {
    const list = groups.get(note.date)
    if (list) list.push(note)
    else groups.set(note.date, [note])
  }
  return [...groups.entries()]
    .sort(([a], [b]) => (a < b ? 1 : a > b ? -1 : 0))
    .map(([key, list]) => ({
      key, label: dayLabel(key, now, locale),
      notes: list.sort((a, b) => Date.parse(b.createdAt) - Date.parse(a.createdAt) || (a.id < b.id ? -1 : 1)),
    }))
}

/** The time a note was started, shown only when that moment falls on the note's own day. */
export function noteTime(note: Pick<NoteDTO, 'createdAt' | 'date'>, locale?: string): string | null {
  const created = new Date(note.createdAt)
  if (Number.isNaN(created.getTime()) || localDateKey(created) !== note.date) return null
  return dateFormatter(locale, 'time', { hour: 'numeric', minute: '2-digit' }).format(created)
}

export function noteTitle(note: Pick<NoteDTO, 'title'>): string {
  return note.title.trim() || 'Untitled note'
}

const spoken = (segment: TranscriptSegmentDTO) => segment.text.trim().length > 0
const speakerKey = (segment: TranscriptSegmentDTO) => `${segment.channel}:${segment.speakerId}`

export function speakerCount(note: Pick<NoteDTO, 'transcriptSegments'>): number {
  return new Set(note.transcriptSegments.filter(spoken).map(speakerKey)).size
}

export interface NoteStatus { tone: 'live' | 'busy' | 'warning' | 'info'; label: string; detail: string }

/** What the reader should know about a note that is not simply ready. */
export function noteStatus(note: Pick<NoteDTO, 'recordingState' | 'generationState' | 'transcriptRevision' | 'generatedRevision' | 'enhanced'>): NoteStatus | null {
  if (note.recordingState === 'recording') return { tone: 'live', label: 'Recording', detail: 'This meeting is being recorded. New transcript appears after you refresh.' }
  if (note.generationState === 'analyzing_transcript') return { tone: 'busy', label: 'Reading transcript', detail: 'Cereal is reading the transcript. The summary will appear here shortly.' }
  if (note.generationState === 'writing_notes') return { tone: 'busy', label: 'Writing notes', detail: 'Cereal is writing the summary. Refresh in a moment to read it.' }
  if (note.generationState === 'failed') return { tone: 'warning', label: 'Summary failed', detail: 'The summary could not be written. Generate it again from the desktop or iPhone app.' }
  if (note.generationState === 'unsupported' && !note.enhanced.trim()) return { tone: 'info', label: 'No summary', detail: 'This instance has no model configured for summaries.' }
  if (note.generatedRevision !== null && note.generatedRevision < note.transcriptRevision && note.enhanced.trim()) {
    return { tone: 'info', label: 'Summary out of date', detail: 'The transcript changed after this summary was written. Regenerate it from the desktop or iPhone app.' }
  }
  return null
}

export function availableViews(note: Pick<NoteDTO, 'enhanced' | 'body' | 'transcriptSegments'>): Record<NoteView, boolean> {
  return { summary: note.enhanced.trim().length > 0, notes: note.body.trim().length > 0, transcript: note.transcriptSegments.some(spoken) }
}

/** Open on the richest thing the note has: summary, then the writer's notes, then the transcript. */
export function defaultNoteView(note: Pick<NoteDTO, 'enhanced' | 'body' | 'transcriptSegments'>): NoteView {
  const views = availableViews(note)
  return views.summary ? 'summary' : views.notes ? 'notes' : views.transcript ? 'transcript' : 'summary'
}

export function matchesFilter(note: NoteDTO, filter: NoteFilter): boolean {
  return filter === 'all' || (filter === 'summarized' ? noteFacts(note).hasSummary : noteFacts(note).hasTranscript)
}

// ── Search ────────────────────────────────────────────────────────────────

export interface TextPart { text: string; hit: boolean }
export interface NoteMatch { note: NoteDTO; title: TextPart[]; snippet: TextPart[] | null; source: 'summary' | 'notes' | 'transcript' | null }

export function searchTerms(query: string): string[] {
  return [...new Set(query.toLocaleLowerCase().split(/\s+/).filter(Boolean))].slice(0, 8)
}

/** Split text around every occurrence of any term, longest terms first so overlaps resolve predictably. */
export function highlight(text: string, terms: readonly string[]): TextPart[] {
  if (!terms.length || !text) return [{ text, hit: false }]
  const lower = text.toLocaleLowerCase(), ordered = [...terms].sort((a, b) => b.length - a.length)
  const parts: TextPart[] = []
  let cursor = 0, plain = 0
  while (cursor < text.length) {
    const term = ordered.find(candidate => lower.startsWith(candidate, cursor))
    if (!term) { cursor++; continue }
    if (cursor > plain) parts.push({ text: text.slice(plain, cursor), hit: false })
    parts.push({ text: text.slice(cursor, cursor + term.length), hit: true })
    cursor += term.length; plain = cursor
  }
  if (plain < text.length) parts.push({ text: text.slice(plain), hit: false })
  return parts
}

/** Markdown read as a sentence: structure characters removed, whitespace collapsed. */
export function plainText(markdown: string): string {
  return markdown
    .replace(/```[^\n]*\n?/g, ' ')
    .replace(/\[([^\]\n]+)\]\([^\s)]+\)/g, '$1')
    .replace(/^\s{0,3}(?:#{1,6}|>|[-*•]|\d+[.)])\s+/gm, '')
    .replace(/[*_`]+/g, '')
    .replace(/\s+/g, ' ')
    .trim()
}

/** Markdown as running prose: headings dropped, and each list item or paragraph ends as a sentence
 * so separate lines never read as one ("…next Tuesday. Pricing holds…"). */
export function proseText(markdown: string): string {
  const lines: string[] = []
  let fenced = false
  for (const raw of markdown.split(/\r\n?|\n/)) {
    if (/^\s*```/.test(raw)) { fenced = !fenced; continue }
    if (fenced || /^\s{0,3}#{1,6}\s/.test(raw) || /^\s*(-{3,}|\*{3,}|_{3,})\s*$/.test(raw)) continue
    const line = plainText(raw)
    if (line) lines.push(/[.!?:;,…]$/.test(line) ? line : `${line}.`)
  }
  return lines.join(' ')
}

function excerpt(text: string, terms: readonly string[], radius = 70): TextPart[] | null {
  const lower = text.toLocaleLowerCase()
  let first = -1
  for (const term of terms) { const at = lower.indexOf(term); if (at >= 0 && (first < 0 || at < first)) first = at }
  if (first < 0) return null
  let start = Math.max(0, first - radius), end = Math.min(text.length, first + radius * 2)
  if (start > 0) { const space = text.indexOf(' ', start); if (space >= 0 && space < first) start = space + 1 }
  if (end < text.length) { const space = text.lastIndexOf(' ', end); if (space > first) end = space }
  return highlight(`${start > 0 ? '… ' : ''}${text.slice(start, end)}${end < text.length ? ' …' : ''}`, terms)
}

// Everything below is derived once per note object and kept for as long as the
// note is. A list render, a keystroke or a scroll then reads cached values
// instead of walking every summary and transcript again.

export interface NoteFacts { title: string; time: string | null; preview: string; status: NoteStatus | null; hasSummary: boolean; hasBody: boolean; hasTranscript: boolean }
const factsCache = new WeakMap<NoteDTO, NoteFacts>()
/** What a list row shows, apart from anything that needs the transcript read in full. */
export function noteFacts(note: NoteDTO): NoteFacts {
  let facts = factsCache.get(note)
  if (!facts) {
    facts = {
      title: noteTitle(note), time: noteTime(note), preview: notePreview(note), status: noteStatus(note),
      hasSummary: note.enhanced.trim().length > 0, hasBody: note.body.trim().length > 0, hasTranscript: note.transcriptSegments.some(spoken),
    }
    factsCache.set(note, facts)
  }
  return facts
}

const speakersCache = new WeakMap<NoteDTO, number>()
/** Distinct voices, counted the first time a row for this note is actually shown. */
export function cachedSpeakerCount(note: NoteDTO): number {
  let count = speakersCache.get(note)
  if (count === undefined) { count = speakerCount(note); speakersCache.set(note, count) }
  return count
}

interface SearchText { head: string; summary: string; body: string; transcript: string | null; transcriptLower: string | null }
const searchCache = new WeakMap<NoteDTO, SearchText>()
function searchText(note: NoteDTO): SearchText {
  let text = searchCache.get(note)
  if (!text) {
    const summary = plainText(note.enhanced), body = plainText(note.body)
    text = { head: [noteTitle(note), summary, body].join('\n').toLocaleLowerCase(), summary, body, transcript: null, transcriptLower: null }
    searchCache.set(note, text)
  }
  return text
}
/** Transcripts are the bulk of a note. They are flattened only when a search term is not already found in the title, summary or notes. */
function transcriptText(note: NoteDTO, text: SearchText): string {
  if (text.transcriptLower === null) {
    const labels = new Set<string>(), said: string[] = []
    for (const segment of note.transcriptSegments) {
      if (segment.speakerLabel) labels.add(segment.speakerLabel)
      if (spoken(segment)) said.push(segment.text.trim())
    }
    text.transcript = said.join(' ')
    text.transcriptLower = `${text.transcript}\n${[...labels].join(' ')}`.toLocaleLowerCase()
  }
  return text.transcriptLower
}

function noteMatches(note: NoteDTO, terms: readonly string[]): boolean {
  const text = searchText(note)
  for (const term of terms) if (!text.head.includes(term) && !transcriptText(note, text).includes(term)) return false
  return true
}

/** The notes that contain every term, in their given order. No snippets: those are made per visible row. */
export function findNotes(notes: readonly NoteDTO[], terms: readonly string[]): NoteDTO[] {
  return terms.length ? notes.filter(note => noteMatches(note, terms)) : [...notes]
}

/** A search that remembers its last answer: extending the query ("ti" to "til")
 * only re-checks the notes that already matched, so typing gets cheaper as it narrows. */
export function createNoteSearch() {
  let last: { notes: readonly NoteDTO[]; query: string; found: NoteDTO[] } | null = null
  return (notes: readonly NoteDTO[], query: string): NoteDTO[] => {
    const normal = query.toLocaleLowerCase().trim().replace(/\s+/g, ' ')
    if (last && last.notes === notes && last.query === normal) return last.found
    const narrowing = last !== null && last.notes === notes && last.query !== '' && normal.startsWith(last.query)
    const found = findNotes(narrowing ? last!.found : notes, searchTerms(normal))
    last = { notes, query: normal, found }
    return found
  }
}

/** How one matching note should be shown: its title with hits marked, and the first place outside the title that explains the match. */
export function noteMatch(note: NoteDTO, terms: readonly string[]): NoteMatch {
  const title = noteTitle(note)
  if (!terms.length) return { note, title: [{ text: title, hit: false }], snippet: null, source: null }
  const text = searchText(note)
  let snippet = excerpt(text.summary, terms), source: NoteMatch['source'] = snippet ? 'summary' : null
  if (!snippet) { snippet = excerpt(text.body, terms); if (snippet) source = 'notes' }
  if (!snippet) { transcriptText(note, text); snippet = excerpt(text.transcript ?? '', terms); if (snippet) source = 'transcript' }
  return { note, title: highlight(title, terms), snippet, source }
}

/** Every term must appear somewhere in the note. The snippet shows where, when it is not the title. */
export function searchNotes(notes: readonly NoteDTO[], query: string): NoteMatch[] {
  const terms = searchTerms(query)
  return findNotes(notes, terms).map(note => noteMatch(note, terms))
}

/** The first lines of a note, for a row that has no search to explain. */
export function notePreview(note: Pick<NoteDTO, 'enhanced' | 'body'>, length = 150): string {
  // Headings name sections; the preview starts with what the note actually says.
  const text = proseText(note.enhanced) || proseText(note.body)
  if (text.length <= length) return text
  const cut = text.lastIndexOf(' ', length)
  return `${text.slice(0, cut > length * 0.6 ? cut : length).replace(/[\s.,;:–-]+$/, '')}…`
}

// ── Transcript ────────────────────────────────────────────────────────────

export interface TranscriptTurn {
  key: string
  /** Position of this voice among the transcript's distinct voices, in first-spoken order. */
  voice: number
  speaker: string
  source: 'Microphone' | 'System audio'
  /** One-based audio part within its source, when the note has several. Voices are matched only within a part. */
  part: number | null
  startMs: number
  pieces: { id: string; text: string; final: boolean }[]
}

const UUID_PATTERN = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i

/** Canonical ids look like `mic:<chunk uuid>:speaker_1`. Anything else carries no part. */
function partScope(segment: TranscriptSegmentDTO): string | null {
  const [source, chunk, speaker, extra] = segment.speakerId.split(':')
  if (extra !== undefined || source !== segment.channel || !chunk || !UUID_PATTERN.test(chunk)) return null
  return speaker === 'unknown' || /^speaker_\d+$/.test(speaker ?? '') ? `${segment.channel}:${chunk.toLowerCase()}` : null
}

/** Consecutive segments from one voice read as one turn. A voice is a speaker id
 * within a source; the same label in another part or source is never merged. */
export function transcriptTurns(segments: readonly TranscriptSegmentDTO[]): TranscriptTurn[] {
  const said = segments.filter(spoken)
  const parts: Record<'mic' | 'system', string[]> = { mic: [], system: [] }
  for (const segment of said) {
    const scope = partScope(segment)
    if (scope && !parts[segment.channel].includes(scope)) parts[segment.channel].push(scope)
  }
  const voices = new Map<string, number>(), turns: TranscriptTurn[] = []
  for (const segment of said) {
    const key = speakerKey(segment)
    if (!voices.has(key)) voices.set(key, voices.size)
    const piece = { id: segment.id, text: segment.text.trim(), final: segment.isFinal }
    const previous = turns[turns.length - 1]
    if (previous?.key === key) { previous.pieces.push(piece); continue }
    const scope = partScope(segment), sourceParts = parts[segment.channel]
    turns.push({
      key, voice: voices.get(key)!,
      speaker: segment.speakerLabel?.trim() || 'Unlabeled speaker',
      source: segment.channel === 'mic' ? 'Microphone' : 'System audio',
      part: scope && sourceParts.length > 1 ? sourceParts.indexOf(scope) + 1 : null,
      startMs: segment.startMs,
      pieces: [piece],
    })
  }
  return turns
}

/** 4:05, 12:30, 1:02:07. */
export function formatClock(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000)), hours = Math.floor(total / 3600)
  const minutes = Math.floor(total / 60) % 60, seconds = String(total % 60).padStart(2, '0')
  return hours ? `${hours}:${String(minutes).padStart(2, '0')}:${seconds}` : `${minutes}:${seconds}`
}

export function turnSource(turn: Pick<TranscriptTurn, 'source' | 'part'>): string {
  return turn.part ? `${turn.source} · Part ${turn.part}` : turn.source
}

// ── People ────────────────────────────────────────────────────────────────

export function initials(name: string): string {
  const words = name.trim().split(/[\s._@-]+/).filter(Boolean)
  const first = words[0] ?? '', last = words.length > 1 ? words[words.length - 1]! : ''
  const number = /^\d+$/.test(last)
  const letters = [...first].slice(0, 1).join('') + (last ? [...last].slice(0, number ? 2 : 1).join('') : '')
  return letters.toLocaleUpperCase() || '?'
}

export function participantName(person: Pick<MeetingParticipantDTO, 'displayName' | 'email'>): string {
  return person.displayName?.trim() || person.email?.trim() || 'Unnamed participant'
}

/** People who were invited, without rooms and equipment. */
export function meetingPeople(participants: readonly MeetingParticipantDTO[]): MeetingParticipantDTO[] {
  return participants.filter(person => !person.isResource)
}

/** "Ada, Grace +2": the first names that fit, then a count. */
export function peopleSummary(participants: readonly MeetingParticipantDTO[], shown = 2): string {
  const people = meetingPeople(participants).filter(person => !person.isSelf)
  const names = people.slice(0, shown).map(person => (person.givenName?.trim() || participantName(person).split(/[\s@]/)[0] || participantName(person)))
  if (!names.length) return ''
  return people.length > shown ? `${names.join(', ')} +${people.length - shown}` : names.join(', ')
}

// ── Export ────────────────────────────────────────────────────────────────

export function transcriptMarkdown(segments: readonly TranscriptSegmentDTO[]): string {
  return transcriptTurns(segments)
    .map(turn => `**${turn.speaker}** (${turnSource(turn)}, ${formatClock(turn.startMs)})\n${turn.pieces.map(piece => piece.text).join(' ')}`)
    .join('\n\n')
}

export function noteMarkdown(note: Pick<NoteDTO, 'title' | 'enhanced' | 'body' | 'transcriptSegments'>, view: NoteView): string {
  const content = view === 'summary' ? note.enhanced : view === 'notes' ? note.body : transcriptMarkdown(note.transcriptSegments)
  return `# ${noteTitle(note)}\n\n${content.trim()}\n`
}

export function markdownFileName(title: string, fallback = 'note'): string {
  return `${title.replace(/[^\p{L}\p{N} _.-]/gu, '').trim().slice(0, 80) || fallback}.md`
}
