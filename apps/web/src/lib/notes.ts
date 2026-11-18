import type { MeetingParticipantDTO, NoteDTO, TranscriptSegmentDTO } from '@meeting-notes/contracts'

/** Presentation logic for the notes reader. Pure functions: no DOM, no network. */

export type NoteView = 'summary' | 'notes' | 'transcript'

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

export function noteTitle(note: Pick<NoteDTO, 'title'>): string {
  return note.title.trim() || 'Untitled note'
}

// ── The desktop app's own labels ───────────────────────────────────────────
// The apps write dates and times one way, in English; the web writes them the same way.

const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec']
const WEEKDAYS = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat']

/** The day heading of the notes list: "Today", "Yesterday", then "Thu, Nov 12". */
export function listDayLabel(key: string, now: Date): string {
  const day = dayAtNoon(key), offset = dayOffset(key, now)
  if (!day || offset === null) return key
  return offset === 0 ? 'Today' : offset === -1 ? 'Yesterday' : `${WEEKDAYS[day.getDay()]}, ${MONTHS[day.getMonth()]} ${day.getDate()}`
}
/** The date in a note's details chip: "Today", "Yesterday", then "Nov 12". */
export function chipDayLabel(key: string, now: Date): string {
  const day = dayAtNoon(key), offset = dayOffset(key, now)
  if (!day || offset === null) return 'Today'
  return offset === 0 ? 'Today' : offset === -1 ? 'Yesterday' : `${MONTHS[day.getMonth()]} ${day.getDate()}`
}
/** When a note was made, as the list shows it: "7:47 AM". */
export function createdClock(createdAt: string): string {
  const created = new Date(createdAt)
  if (Number.isNaN(created.getTime())) return ''
  const hours = created.getHours()
  return `${hours % 12 || 12}:${String(created.getMinutes()).padStart(2, '0')} ${hours < 12 ? 'AM' : 'PM'}`
}
/** A moment as the apps would write it in a list of links: "Nov 15, 9:41 AM". */
export function stampLabel(value: string): string {
  const at = new Date(value)
  return Number.isNaN(at.getTime()) ? '' : `${MONTHS[at.getMonth()]} ${at.getDate()}, ${createdClock(value)}`
}
/** A clock string as its digits and a trailing AM/PM, which is set as small capitals. */
export function clockParts(time: string): { digits: string; meridiem: string | null } {
  const text = time.trim(), match = /^(.*?)\s*(AM|PM|am|pm)$/.exec(text)
  return match ? { digits: match[1]!, meridiem: match[2]!.toUpperCase() } : { digits: text, meridiem: null }
}
/** The day a note is listed under: its meeting day, or the day it was made if that came first
 * (a note prepared for a meeting still to come is listed on the day it was prepared). */
export function timelineDate(note: Pick<NoteDTO, 'date' | 'createdAt'>): string {
  const created = new Date(note.createdAt), made = Number.isNaN(created.getTime()) ? null : localDateKey(created)
  if (!dayAtNoon(note.date)) return made ?? note.date
  return made !== null && made < note.date ? made : note.date
}
/** The date a note's chip shows: the meeting's own day when a calendar event is attached, else the note's. */
export function noteDay(note: Pick<NoteDTO, 'date'>, meeting: { startAt: string; allDay: boolean } | null | undefined): string {
  if (meeting) {
    const start = new Date(meeting.startAt)
    if (!Number.isNaN(start.getTime())) return meeting.allDay ? meeting.startAt.slice(0, 10) : localDateKey(start)
  }
  return note.date
}

const spoken = (segment: TranscriptSegmentDTO) => segment.text.trim().length > 0
const speakerKey = (segment: TranscriptSegmentDTO) => `${segment.channel}:${segment.speakerId}`

export interface NoteStatus { tone: 'busy' | 'warning' | 'info'; label: string; detail: string }

/** What the reader should know about a finished note whose summary is not simply ready.
 * A note that is still being recorded is not shown on the web at all (see `isFinished`). */
export function noteStatus(note: Pick<NoteDTO, 'generationState' | 'transcriptRevision' | 'generatedRevision' | 'enhanced'>): NoteStatus | null {
  if (note.generationState === 'analyzing_transcript') return { tone: 'busy', label: 'Analyzing transcript', detail: 'Cereal is reading the transcript. The notes will be here shortly.' }
  if (note.generationState === 'writing_notes') return { tone: 'busy', label: 'Writing notes', detail: 'Cereal is writing the notes. They will be here shortly.' }
  if (note.generationState === 'failed') return { tone: 'warning', label: 'Notes failed', detail: 'The notes could not be written. Generate them again in the Cereal app.' }
  if (note.generationState === 'unsupported' && !note.enhanced.trim()) return { tone: 'info', label: 'No notes', detail: 'This instance has no model configured for notes.' }
  if (note.generatedRevision !== null && note.generatedRevision < note.transcriptRevision && note.enhanced.trim()) {
    return { tone: 'info', label: 'Notes out of date', detail: 'The transcript changed after these notes were written. Regenerate them in the Cereal app.' }
  }
  return null
}

/** The web reads finished notes only. A meeting that is being recorded is live in the
 * apps, and a note nothing has been recorded or written into has nothing to read yet. */
export function isFinished(note: { recordingState: NoteDTO['recordingState']; hasSummary: boolean; hasBody: boolean; hasTranscript: boolean }): boolean {
  if (note.recordingState === 'recording') return false
  return note.recordingState === 'stopped' || note.hasSummary || note.hasBody || note.hasTranscript
}
export function noteIsFinished(note: Pick<NoteDTO, 'recordingState' | 'enhanced' | 'body' | 'transcriptSegments'>): boolean {
  return isFinished({ recordingState: note.recordingState, hasSummary: note.enhanced.trim().length > 0, hasBody: note.body.trim().length > 0, hasTranscript: note.transcriptSegments.some(spoken) })
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

export interface NoteFacts { title: string; preview: string; status: NoteStatus | null; hasSummary: boolean; hasBody: boolean; hasTranscript: boolean }
const factsCache = new WeakMap<NoteDTO, NoteFacts>()
/** What the note page shows of a note, apart from anything that needs the transcript read in full. */
export function noteFacts(note: NoteDTO): NoteFacts {
  let facts = factsCache.get(note)
  if (!facts) {
    facts = {
      title: noteTitle(note), preview: notePreview(note), status: noteStatus(note),
      hasSummary: note.enhanced.trim().length > 0, hasBody: note.body.trim().length > 0, hasTranscript: note.transcriptSegments.some(spoken),
    }
    factsCache.set(note, facts)
  }
  return facts
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
