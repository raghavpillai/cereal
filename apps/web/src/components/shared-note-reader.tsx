import type { SharedNoteDTO } from '@meeting-notes/contracts'
import { useEffect, useId, useState } from 'react'
import { Clock3, Lock } from 'lucide-react'
import { Markdown } from '../lib/markdown'
import { markdownFileName } from '../lib/notes'
import { Segmented } from './kit/segmented'
import { NoteActions, useNoteExport } from './notes/note-actions'

/** The anonymous reader for an immutable shared snapshot. It shares the note
 * page's reading layout and nothing else: no session, no transcript, no storage. */
export function SharedNoteReader({ note }: { note: SharedNoteDTO }) {
  const hasSummary = Boolean(note.enhanced.trim())
  const hasNotes = Boolean(note.body.trim())
  const [view, setView] = useState<'summary' | 'notes'>(hasSummary ? 'summary' : 'notes')
  const panel = useId()
  const content = view === 'summary' ? note.enhanced : note.body
  const title = note.title.trim() || 'Untitled note'
  // A shared link is its own entry in tabs and history.
  useEffect(() => { document.title = `${title} · Cereal` }, [title])
  const actions = useNoteExport(() => `# ${title}\n\n${content}`, markdownFileName(title, 'shared-note'))
  return <article className="note-page is-shared" key={note.shareId}>
    <div className="note-crumbs is-shared"><span className="eyebrow">Shared note</span><span className="read-only"><Lock aria-hidden="true" />Read-only</span></div>
    <header className="note-masthead">
      <p className="eyebrow note-kicker"><time dateTime={note.date}>{dateLabel(note.date)}</time></p>
      <h1 className="note-title">{title}</h1>
      {note.duration && <div className="note-facts"><span><Clock3 aria-hidden="true" />{note.duration}</span></div>}
    </header>
    <div className="note-toolbar">
      {hasSummary && hasNotes
        ? <Segmented label="Note view" controls={panel} value={view} onChange={setView} options={[{ value: 'summary', label: 'Summary' }, { value: 'notes', label: 'Notes' }]} />
        : <span className="eyebrow" style={{ paddingLeft: '0.5rem' }}>{hasNotes && !hasSummary ? 'Notes' : 'Summary'}</span>}
      <NoteActions {...actions} what="displayed note" />
    </div>
    <div className="note-body-wrap">
      <section className="note-body" id={panel} role={hasSummary && hasNotes ? 'tabpanel' : undefined} aria-label={view === 'summary' ? 'Summary' : 'Notes'}>
        <div className="fade-swap" key={view}>{content.trim() ? <Markdown value={content} /> : <div className="note-body-empty"><strong>Nothing to read</strong><span>The sender shared this note without any text.</span></div>}</div>
      </section>
      <p className="note-footnote">Shared {dateLabel(note.sharedAt)}{note.expiresAt && <> · Available until {dateLabel(note.expiresAt)}</>}</p>
      <p className="action-notice" role="status">{actions.notice}</p>
    </div>
  </article>
}
function dateLabel(value: string) {
  const date = new Date(value.length === 10 ? `${value}T12:00:00Z` : value)
  return Number.isNaN(date.getTime()) ? '' : new Intl.DateTimeFormat(undefined, { month: 'long', day: 'numeric', year: 'numeric', ...(value.length === 10 ? { timeZone: 'UTC' } : {}) }).format(date)
}
