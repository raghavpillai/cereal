import { describe, expect, test } from 'bun:test'
import { renderToStaticMarkup } from 'react-dom/server'
import type { MeetingParticipantDTO, NoteDTO, TranscriptSegmentDTO } from '@meeting-notes/contracts'
import { Segmented } from '../components/kit/segmented'
import { MeetingCard } from '../components/notes/meeting-card'
import { TranscriptPanel } from '../components/notes/transcript'
import {
  chipDayLabel, clockParts, createNoteSearch, createdClock, dayOffset, findNotes, formatClock, highlight, initials, isFinished, listDayLabel, localDateKey, markdownFileName,
  noteDay, noteFacts, noteIsFinished, noteMarkdown, noteMatch, notePreview, noteStatus, noteTitle, plainText, proseText, searchNotes, searchTerms, stampLabel, timelineDate, transcriptTurns, transcriptUtterances, turnSource,
} from './notes'

const now = new Date(2026, 10, 13, 15, 30) // Friday 13 November 2026, local time
const id = (n: number) => `00000000-0000-4000-8000-${String(n).padStart(12, '0')}`
const chunkA = '11111111-1111-4111-8111-111111111111', chunkB = '22222222-2222-4222-8222-222222222222'

function note(n: number, fields: Partial<NoteDTO> = {}): NoteDTO {
  return {
    id: id(n), title: `Note ${n}`, body: '', enhanced: '', date: '2026-11-13', duration: '',
    createdAt: new Date(2026, 10, 13, 9, n).toISOString(), updatedAt: new Date(2026, 10, 13, 9, n).toISOString(),
    transcriptSegments: [], recordingState: 'idle', generationState: 'idle', transcriptRevision: 0, generatedRevision: null, ...fields,
  }
}
let segmentCount = 0
function segment(channel: 'mic' | 'system', speakerId: string, text: string, startMs: number, fields: Partial<TranscriptSegmentDTO> = {}): TranscriptSegmentDTO {
  return { id: id(1000 + segmentCount++), channel, speakerId, speakerLabel: /speaker_(\d+)$/.test(speakerId) ? `Speaker ${/speaker_(\d+)$/.exec(speakerId)![1]}` : null, text, isFinal: true, startMs, endMs: startMs + 1000, ...fields }
}
function person(n: number, displayName: string | null, fields: Partial<MeetingParticipantDTO> = {}): MeetingParticipantDTO {
  return { id: id(2000 + n), email: `person${n}@example.test`, displayName, givenName: displayName?.split(' ')[0] ?? null, familyName: null, nameSource: 'calendar', isSelf: false, isOrganizer: false, isResource: false, responseStatus: 'accepted', ...fields }
}

describe('days and times, written as the apps write them', () => {
  test('a day is Today, Yesterday, then a short English date, and never shifts with the time zone', () => {
    expect([listDayLabel('2026-11-13', now), listDayLabel('2026-11-12', now), listDayLabel('2026-11-11', now), listDayLabel('2025-12-04', now)]).toEqual(['Today', 'Yesterday', 'Wed, Nov 11', 'Thu, Dec 4'])
    expect([chipDayLabel('2026-11-13', now), chipDayLabel('2026-11-12', now), chipDayLabel('2026-11-11', now), chipDayLabel('2026-03-02', now)]).toEqual(['Today', 'Yesterday', 'Nov 11', 'Mar 2'])
    // Something that is not a day is shown as it came in a list, and as today in a chip, like the app.
    expect(listDayLabel('not-a-date', now)).toBe('not-a-date')
    expect(chipDayLabel('2026-02-30', now)).toBe('Today')
    // Just after midnight and just before it are still the same calendar day.
    expect(listDayLabel('2026-11-13', new Date(2026, 10, 13, 0, 1))).toBe('Today')
    expect(listDayLabel('2026-11-13', new Date(2026, 10, 13, 23, 59))).toBe('Today')
    expect(dayOffset('2026-11-06', now)).toBe(-7)
    // A daylight-saving change between the two days is still one day.
    expect(dayOffset('2026-03-29', new Date(2026, 2, 30, 8))).toBe(-1)
    expect(localDateKey(new Date(2026, 0, 5, 23, 59))).toBe('2026-01-05')
  })
  test('a time is the hour it was made, with its AM or PM set apart', () => {
    expect([createdClock(new Date(2026, 10, 13, 7, 47).toISOString()), createdClock(new Date(2026, 10, 13, 0, 5).toISOString()), createdClock(new Date(2026, 10, 13, 12, 0).toISOString()), createdClock(new Date(2026, 10, 13, 23, 59).toISOString()), createdClock('nonsense')]).toEqual(['7:47 AM', '12:05 AM', '12:00 PM', '11:59 PM', ''])
    expect(clockParts('9:00 AM')).toEqual({ digits: '9:00', meridiem: 'AM' })
    expect(clockParts('10:30 – 11:00 pm')).toEqual({ digits: '10:30 – 11:00', meridiem: 'PM' })
    expect(clockParts(' 14:05 ')).toEqual({ digits: '14:05', meridiem: null })
    expect(clockParts('')).toEqual({ digits: '', meridiem: null })
    expect(stampLabel(new Date(2026, 10, 15, 9, 41).toISOString())).toBe('Nov 15, 9:41 AM')
    expect(stampLabel('nonsense')).toBe('')
  })
  test('a note is listed under its meeting day, or the day it was made if that came first', () => {
    const made = new Date(2026, 10, 13, 14, 5).toISOString()
    expect(timelineDate({ date: '2026-11-13', createdAt: made })).toBe('2026-11-13')
    // An imported meeting keeps its own, earlier day.
    expect(timelineDate({ date: '2026-11-10', createdAt: made })).toBe('2026-11-10')
    // A note prepared for a meeting still to come is listed on the day it was prepared.
    expect(timelineDate({ date: '2026-11-20', createdAt: made })).toBe('2026-11-13')
    expect(timelineDate({ date: 'nope', createdAt: made })).toBe('2026-11-13')
    expect(timelineDate({ date: '2026-11-13', createdAt: 'nonsense' })).toBe('2026-11-13')
    // The chip shows the meeting's own day when a calendar event is attached.
    expect(noteDay({ date: '2026-11-13' }, null)).toBe('2026-11-13')
    expect(noteDay({ date: '2026-11-13' }, { startAt: '2026-11-12T00:00:00.000Z', allDay: true })).toBe('2026-11-12')
    expect(noteDay({ date: '2026-11-13' }, { startAt: new Date(2026, 10, 11, 9).toISOString(), allDay: false })).toBe('2026-11-11')
    expect(noteDay({ date: '2026-11-13' }, { startAt: 'nonsense', allDay: false })).toBe('2026-11-13')
    expect(noteTitle({ title: '   ' })).toBe('Untitled note')
    expect(noteTitle({ title: ' Planning ' })).toBe('Planning')
  })
})

describe('note state', () => {
  const base = { generationState: 'ready', transcriptRevision: 2, generatedRevision: 2, enhanced: 'Summary' } as const
  test('only states the reader should know about produce a status', () => {
    expect(noteStatus(base)).toBeNull()
    expect(noteStatus({ ...base, generationState: 'idle', generatedRevision: null, enhanced: '' })).toBeNull()
    expect(noteStatus({ ...base, generationState: 'analyzing_transcript' })).toMatchObject({ tone: 'busy', label: 'Analyzing transcript' })
    expect(noteStatus({ ...base, generationState: 'writing_notes' })).toMatchObject({ tone: 'busy', label: 'Writing notes' })
    expect(noteStatus({ ...base, generationState: 'failed' })?.tone).toBe('warning')
    expect(noteStatus({ ...base, generationState: 'unsupported', enhanced: '' })?.label).toBe('No notes')
    // An unsupported instance that still holds earlier notes has nothing to report.
    expect(noteStatus({ ...base, generationState: 'unsupported' })).toBeNull()
    expect(noteStatus({ ...base, transcriptRevision: 3 })?.label).toBe('Notes out of date')
    expect(noteStatus({ ...base, transcriptRevision: 3, enhanced: ' ' })).toBeNull()
  })
  test('the web reads finished notes: nothing that is being recorded, and nothing with nothing in it', () => {
    const nothing = { hasSummary: false, hasBody: false, hasTranscript: false }
    // Being recorded is never finished, whatever the note already holds.
    expect(isFinished({ recordingState: 'recording', ...nothing })).toBe(false)
    expect(isFinished({ recordingState: 'recording', hasSummary: true, hasBody: true, hasTranscript: true })).toBe(false)
    // A recording that was stopped is finished, even before anything was written from it.
    expect(isFinished({ recordingState: 'stopped', ...nothing })).toBe(true)
    // Never recorded: finished once there is something to read (an imported meeting, a typed note), not before.
    expect(isFinished({ recordingState: 'idle', ...nothing })).toBe(false)
    expect(isFinished({ recordingState: 'idle', ...nothing, hasTranscript: true })).toBe(true)
    expect(isFinished({ recordingState: 'idle', ...nothing, hasSummary: true })).toBe(true)
    expect(isFinished({ recordingState: 'idle', ...nothing, hasBody: true })).toBe(true)
    const spoken = [segment('mic', `mic:${chunkA}:speaker_1`, 'Hello', 0)]
    expect(noteIsFinished(note(1))).toBe(false)
    expect(noteIsFinished(note(1, { body: '  ', enhanced: '\n', transcriptSegments: [segment('mic', 'x', '   ', 0)] }))).toBe(false)
    expect(noteIsFinished(note(1, { transcriptSegments: spoken }))).toBe(true)
    expect(noteIsFinished(note(1, { recordingState: 'recording', transcriptSegments: spoken, enhanced: 'S' }))).toBe(false)
    expect(noteIsFinished(note(1, { recordingState: 'stopped' }))).toBe(true)
  })
})

describe('search', () => {
  const notes = [
    note(1, { title: 'Orchard planner', enhanced: '## Decisions\n\n- **Offline maps** ship first because growers lose signal.', body: 'ask about tiles' }),
    note(2, { title: 'Vendor call', body: 'Minimum order for label stock' }),
    note(3, { title: 'Cold storage', transcriptSegments: [segment('system', `system:${chunkA}:speaker_1`, 'The second compressor is due for service.', 0)] }),
  ]
  test('every term must match somewhere; the snippet shows the first place outside the title', () => {
    expect(searchNotes(notes, '').map(match => match.note.id)).toEqual([id(1), id(2), id(3)])
    expect(searchNotes(notes, '  ').every(match => match.snippet === null)).toBe(true)
    const maps = searchNotes(notes, 'OFFLINE growers')
    expect(maps.map(match => match.note.id)).toEqual([id(1)])
    expect(maps[0]!.source).toBe('summary')
    expect(maps[0]!.snippet!.filter(part => part.hit).map(part => part.text)).toEqual(['Offline', 'growers'])
    expect(maps[0]!.snippet!.map(part => part.text).join('')).not.toContain('**')
    expect(searchNotes(notes, 'compressor')[0]).toMatchObject({ source: 'transcript' })
    expect(searchNotes(notes, 'label')[0]).toMatchObject({ source: 'notes' })
    const titleOnly = searchNotes(notes, 'vendor')
    expect(titleOnly[0]!.title).toEqual([{ text: 'Vendor', hit: true }, { text: ' call', hit: false }])
    expect(titleOnly[0]!.snippet).toBeNull()
    expect(searchNotes(notes, 'orchard compressor')).toEqual([])
    expect(searchNotes(notes, 'speaker 1').map(match => match.note.id)).toEqual([id(3)])
  })
  test('terms, highlighting and plain text handle repeats, overlaps and markup characters', () => {
    expect(searchTerms('  Tile  tile\tCACHE ')).toEqual(['tile', 'cache'])
    expect(searchTerms(Array.from({ length: 20 }, (_, n) => `t${n}`).join(' '))).toHaveLength(8)
    expect(highlight('Tiles and tile caching', ['tile'])).toEqual([{ text: 'Tile', hit: true }, { text: 's and ', hit: false }, { text: 'tile', hit: true }, { text: ' caching', hit: false }])
    expect(highlight('caching', ['cach', 'caching']).filter(part => part.hit)).toEqual([{ text: 'caching', hit: true }])
    expect(highlight('<b>bold</b>', ['<b>'])[0]).toEqual({ text: '<b>', hit: true })
    expect(highlight('nothing here', [])).toEqual([{ text: 'nothing here', hit: false }])
    expect(plainText('## Plan\n\n1. First [link](https://example.test)\n> quoted `code`\n```js\nrun()\n```')).toBe('Plan First link quoted code run()')
    expect(notePreview({ enhanced: '## Summary\n\nThe vendor can supply rolls.', body: 'ignored' })).toBe('The vendor can supply rolls.')
    expect(notePreview({ enhanced: '', body: 'Call back about the delivery window.' })).toBe('Call back about the delivery window.')
    const long = notePreview({ enhanced: 'word '.repeat(80), body: '' }, 40)
    expect(long.endsWith('…')).toBe(true)
    expect(long.length).toBeLessThanOrEqual(41)
  })
})

describe('transcript presentation', () => {
  const segments = [
    segment('mic', `mic:${chunkA}:speaker_1`, 'Thanks for joining.', 0),
    segment('mic', `mic:${chunkA}:speaker_1`, 'Let us begin.', 4000),
    segment('system', `system:${chunkA}:speaker_1`, 'Agreed.', 9000),
    segment('system', `system:${chunkA}:speaker_2`, '   ', 12_000),
    segment('system', `system:${chunkA}:speaker_2`, 'I can do that.', 13_000, { isFinal: false }),
    segment('system', `system:${chunkB}:speaker_1`, 'One more thing.', 3_725_000),
    segment('mic', 'legacy-voice', 'Older recording.', 3_730_000, { speakerLabel: '  ' }),
  ]
  test('copied text: consecutive segments from one voice merge; equal labels in another source or part never do', () => {
    const turns = transcriptTurns(segments)
    expect(turns.map(turn => [turn.speaker, turn.source, turn.part, turn.pieces.length])).toEqual([
      ['Speaker 1', 'Microphone', null, 2],
      ['Speaker 1', 'System audio', 1, 1],
      ['Speaker 2', 'System audio', 1, 1],
      ['Speaker 1', 'System audio', 2, 1],
      ['Unlabeled speaker', 'Microphone', null, 1],
    ])
    expect(new Set(turns.map(turn => turn.voice)).size).toBe(5)
    expect(turns[2]!.pieces[0]!.final).toBe(false)
    expect(turnSource(turns[3]!)).toBe('System audio · Part 2')
    expect(turnSource(turns[0]!)).toBe('Microphone')
    expect(transcriptTurns([])).toEqual([])
  })
  test('a speaker id that names another source or is not canonical carries no part', () => {
    const odd = [segment('mic', `system:${chunkA}:speaker_1`, 'a', 0), segment('mic', `mic:${chunkB}:speaker_1`, 'b', 1), segment('mic', `mic:${chunkA}:speaker_x`, 'c', 2), segment('mic', `mic:${chunkA}:speaker_1:extra`, 'd', 3)]
    expect(transcriptTurns(odd).map(turn => turn.part)).toEqual([null, null, null, null])
  })
  test('the transcript panel: one bubble for each thing said, mine on the right, named where a source has several voices', () => {
    expect([0, 999, 65_000, 3_599_000, 3_725_000, -5].map(formatClock)).toEqual(['0:00', '0:00', '1:05', '59:59', '1:02:05', '0:00'])
    const rows = transcriptUtterances(segments)
    // The blank segment is not a bubble; consecutive segments of one voice stay separate bubbles.
    expect(rows.map(row => row.text)).toEqual(['Thanks for joining.', 'Let us begin.', 'Agreed.', 'I can do that.', 'One more thing.', 'Older recording.'])
    expect(rows.map(row => row.own)).toEqual([true, true, false, false, false, true])
    expect(rows.map(row => [row.sameBefore, row.sameAfter])).toEqual([[false, true], [true, false], [false, false], [false, false], [false, false], [false, false]])
    // A speaker id that carries an audio part is named with its source and part; a legacy id is not named.
    expect(rows.map(row => row.speaker)).toEqual(['Speaker 1 · Mic · Part 1', 'Speaker 1 · Mic · Part 1', 'Speaker 1 · System · Part 1', 'Speaker 2 · System · Part 1', 'Speaker 1 · System · Part 2', null])
    expect(rows.map(row => row.names)).toEqual([true, false, true, true, true, false])
    expect(rows.map(row => row.time)).toEqual(['00:00', '00:04', '00:09', '00:13', '62:05', '62:10'])
    expect(rows[3]!.final).toBe(false)
    expect(rows[5]!.source).toBe('Microphone')
    // One voice on each side and no audio parts: nobody needs naming.
    const plain = transcriptUtterances([segment('mic', 'mic', 'Mine.', 0), segment('system', 'them', 'Theirs.', 1000, { speakerLabel: 'Speaker 1' }), segment('mic', 'mic', 'Mine again.', 2000)])
    expect(plain.map(row => row.names)).toEqual([false, false, false])
    // A second voice on the other side names both of them, and still not me.
    const two = transcriptUtterances([segment('mic', 'mic', 'Mine.', 0), segment('system', 'a', 'One.', 1000, { speakerLabel: 'Speaker 1' }), segment('system', 'b', 'Two.', 2000, { speakerLabel: 'Speaker 2' })])
    expect(two.map(row => [row.names, row.speaker])).toEqual([[false, null], [true, 'Speaker 1'], [true, 'Speaker 2']])
    expect(transcriptUtterances([])).toEqual([])

    const html = renderToStaticMarkup(<TranscriptPanel segments={[...segments, segment('system', `system:${chunkB}:speaker_2`, '<script>alert(1)</script>', 3_740_000)]} close={() => {}} copy={async () => true} />)
    expect((html.match(/<li class="utterance"/g) ?? []).length).toBe(7)
    expect((html.match(/data-own="true"/g) ?? []).length).toBe(3)
    expect(html).toContain('Speaker 1 · System · Part 2')
    expect(html).toContain('is-interim')
    expect(html).toContain('&lt;script&gt;')
    expect(html).not.toContain('<script>')
    expect(html).toContain('aria-label="Hide transcript"')
    // Recording controls belong to the apps.
    expect(html).not.toContain('Resume')
  })
  test('copied Markdown follows the view on screen', () => {
    const item = note(1, { title: ' Launch ', enhanced: 'Summary text', body: 'Own text', transcriptSegments: segments.slice(0, 3) })
    expect(noteMarkdown(item, 'summary')).toBe('# Launch\n\nSummary text\n')
    expect(noteMarkdown(item, 'notes')).toBe('# Launch\n\nOwn text\n')
    expect(noteMarkdown(item, 'transcript')).toBe('# Launch\n\n**Speaker 1** (Microphone, 0:00)\nThanks for joining. Let us begin.\n\n**Speaker 1** (System audio, 0:09)\nAgreed.\n')
    expect(markdownFileName('Q3: plan / review?')).toBe('Q3 plan  review.md')
    expect(markdownFileName('///', 'shared-note')).toBe('shared-note.md')
  })
})

describe('people and controls', () => {
  test('initials', () => {
    expect(['Juniper Vale', 'Speaker 12', 'wren@example.test', 'Åsa', '', 'Unlabeled speaker'].map(initials)).toEqual(['JV', 'S12', 'WT', 'Å', '?', 'US'])
  })
  test('the meeting card lists invitees, omits rooms, and links only to safe addresses', () => {
    const organizer = person(1, 'Juniper Vale', { isSelf: true, isOrganizer: true })
    const html = renderToStaticMarkup(<MeetingCard meeting={{
      eventId: id(1), provider: 'google', title: 'Launch <order>', calendarName: 'Field team', startAt: '2026-11-13T16:00:00Z', endAt: '2026-11-13T16:45:00Z', allDay: false, timeZone: null,
      location: 'Packing shed', meetingUrl: 'javascript:alert(1)', eventUrl: 'https://calendar.example.test/e/1', organizer, participants: [organizer, person(2, 'Wren Okafor'), person(4, 'Room 2', { isResource: true })],
    }} />)
    expect(html).toContain('Launch &lt;order&gt;')
    expect(html).toContain('Juniper Vale (you)')
    expect(html).toContain('Wren Okafor')
    expect(html).not.toContain('Room 2')
    expect(html).toContain('href="https://calendar.example.test/e/1"')
    expect(html).not.toContain('javascript:')
    expect(html).toContain('not matched to voices')
  })
  test('the segmented control is a tablist or a radio group with one tab stop, before any measurement', () => {
    const options = [{ value: 'summary', label: 'Summary' }, { value: 'transcript', label: 'Transcript', count: 4 }] as const
    const tabs = renderToStaticMarkup(<Segmented label="Note view" options={options} value="transcript" onChange={() => {}} controls="panel" />)
    expect(tabs).toContain('role="tablist"')
    expect(tabs).toContain('aria-label="Note view"')
    expect(tabs).toMatch(/data-selected="" tabindex="0" role="tab" aria-selected="true" aria-controls="panel">Transcript/)
    expect(tabs).toMatch(/tabindex="-1" role="tab" aria-selected="false" aria-controls="panel">Summary/)
    expect(tabs).toContain('<span class="segmented-count">4</span>')
    const radio = renderToStaticMarkup(<Segmented kind="radio" label="Filter notes" options={options} value="summary" onChange={() => {}} />)
    expect(radio).toContain('role="radiogroup"')
    expect(radio).toMatch(/tabindex="0" role="radio" aria-checked="true">Summary/)
    expect(radio).toMatch(/tabindex="-1" role="radio" aria-checked="false">Transcript/)
    // A filter is not navigation: no landmark, no current-page marker.
    expect(radio).not.toContain('navigation')
    expect(radio).not.toContain('aria-current')
  })
})

describe('work done once per note', () => {
  const spokenSegments = [segment('mic', `mic:${chunkA}:speaker_1`, 'The compressor is due for service.', 0), segment('system', `system:${chunkA}:speaker_1`, 'Booked for the ninth.', 5000)]
  test('a note\'s facts are computed once and reused for the same note object', () => {
    const item = note(1, { title: ' Cold storage ', enhanced: '## Summary\n\nMain room is at eighty percent.', transcriptSegments: spokenSegments })
    const facts = noteFacts(item)
    expect(facts).toEqual({ title: 'Cold storage', preview: 'Main room is at eighty percent.', status: null, hasSummary: true, hasBody: false, hasTranscript: true })
    expect(noteFacts(item)).toBe(facts)
    // A refreshed copy of the note is a different object and is read again.
    const refreshed = { ...item, enhanced: '', transcriptSegments: [] }
    expect(noteFacts(refreshed)).toMatchObject({ hasSummary: false, hasTranscript: false, preview: '' })
    expect(noteFacts(item)).toBe(facts)
  })
  test('a transcript is only flattened when the smaller fields do not already explain the match', () => {
    let reads = 0
    const counted = (segments: TranscriptSegmentDTO[]) => new Proxy(segments, { get(target, key, receiver) { if (key === Symbol.iterator || key === 'filter' || key === 'map') reads++; return Reflect.get(target, key, receiver) } })
    const item = note(1, { title: 'Cold storage', enhanced: 'Main room is at eighty percent.', transcriptSegments: counted(spokenSegments) })
    expect(findNotes([item], ['storage', 'eighty'])).toEqual([item])
    expect(noteMatch(item, ['eighty']).source).toBe('summary')
    expect(reads).toBe(0)
    expect(findNotes([item], ['compressor'])).toEqual([item])
    const after = reads
    expect(after).toBeGreaterThan(0)
    expect(findNotes([item], ['ninth'])).toEqual([item])
    expect(noteMatch(item, ['ninth'])).toMatchObject({ source: 'transcript' })
    expect(reads).toBe(after)
    expect(findNotes([item], ['speaker'])).toEqual([item])
    expect(findNotes([item], ['absent'])).toEqual([])
    expect(findNotes([item], [])).toEqual([item])
  })
  test('extending a query only re-checks what already matched, and anything else starts over', () => {
    const notes = [note(1, { title: 'Tile caching' }), note(2, { title: 'Tiller service' }), note(3, { title: 'Vendor call', body: 'tile samples' })]
    const search = createNoteSearch(), ids = (found: readonly NoteDTO[]) => found.map(item => item.id)
    expect(ids(search(notes, ''))).toEqual([id(1), id(2), id(3)])
    expect(ids(search(notes, 'ti'))).toEqual([id(1), id(2), id(3)])
    const til = search(notes, 'til')
    expect(ids(til)).toEqual([id(1), id(2), id(3)])
    expect(search(notes, ' TIL ')).toBe(til)
    expect(ids(search(notes, 'tile'))).toEqual([id(1), id(3)])
    expect(ids(search(notes, 'tile c'))).toEqual([id(1), id(3)])
    expect(ids(search(notes, 'tile cac'))).toEqual([id(1)])
    // Deleting a character widens the search again.
    expect(ids(search(notes, 'til'))).toEqual([id(1), id(2), id(3)])
    // A different word, or a different list, never reuses the narrowed set.
    expect(ids(search(notes, 'vendor'))).toEqual([id(3)])
    expect(ids(search([notes[1]!], 'vendor'))).toEqual([])
    expect(ids(search(notes, 'vendor tiller'))).toEqual([])
    // Every answer agrees with a search from scratch.
    for (const query of ['t', 'ti', 'til', 'tile', 'tile s', 'service', '']) expect(ids(search(notes, query))).toEqual(searchNotes(notes, query).map(match => match.note.id))
  })
  test('previews read as sentences', () => {
    expect(proseText('## Summary\n\nThe vendor can supply rolls\n\n- Sample roll arrives next Tuesday\n- Pricing holds until the end of the quarter.\n\n```js\nrun()\n```\n---')).toBe('The vendor can supply rolls. Sample roll arrives next Tuesday. Pricing holds until the end of the quarter.')
    expect(notePreview({ enhanced: 'Schedules change on Friday. ' + 'word '.repeat(60), body: '' }, 27)).toBe('Schedules change on Friday…')
  })
})
