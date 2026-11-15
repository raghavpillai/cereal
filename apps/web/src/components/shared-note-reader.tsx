import type { SharedNoteDTO } from '@meeting-notes/contracts'
import { useEffect, useState } from 'react'
import { NoteText } from '../lib/markdown'
import { chipDayLabel, stampLabel } from '../lib/notes'
import { sharedAppURL } from '../lib/open-in-app'
import { shareToken } from '../lib/shares'
import { HeaderPageActions } from './kit/header-slot'
import { Icon } from './kit/icon'
import { NoteOptions, NoteViews, type NoteTab } from './notes/note-parts'
import { OpenInApp } from './open-in-app'

/** The reader for an immutable shared snapshot, opened by someone signed in to this
 * server. It is the note page's layout and nothing else: no transcript, no Share, no storage. */
export function SharedNoteReader({ note }: { note: SharedNoteDTO }) {
  const hasEnhanced = Boolean(note.enhanced.trim()), hasNotes = Boolean(note.body.trim())
  const [tab, setTab] = useState<NoteTab>('enhanced')
  const shown: NoteTab = hasEnhanced && (tab === 'enhanced' || !hasNotes) ? 'enhanced' : 'notes'
  const title = note.title.trim() || 'Untitled note'
  // A shared link is its own entry in tabs and history.
  useEffect(() => { document.title = `${title} · Cereal` }, [title])
  const appURL = typeof window === 'undefined' ? null : sharedAppURL(window.location.origin, shareToken(window.location.pathname) ?? '')
  const text = shown === 'enhanced' ? note.enhanced : note.body
  return <>
    <HeaderPageActions>
      <OpenInApp appURL={appURL} />
      {text.trim() && <NoteOptions title={title} markdown={`# ${title}\n\n${text.trim()}\n`} shown={shown} />}
    </HeaderPageActions>
    <article className="note-doc is-shared" key={note.shareId}>
      <h1 className="note-title">{title}</h1>
      <div className="note-meta">
        {hasEnhanced && hasNotes && <NoteViews shown={shown} show={setTab} />}
        <span className="note-chip"><Icon name="calendar-dots" size={14} /><span>{chipDayLabel(note.date, new Date())}</span></span>
        <span className="note-shared-line">Shared note · read-only</span>
      </div>
      <div className="note-content" key={shown} id={`note-${shown}`} role={hasEnhanced && hasNotes ? 'tabpanel' : undefined} aria-label={hasEnhanced && hasNotes ? (shown === 'enhanced' ? 'Enhanced' : 'My notes') : undefined}>
        {!text.trim() ? <p className="note-quiet">The sender shared this note without any text.</p>
          : shown === 'enhanced' ? <NoteText value={note.enhanced} /> : <div className="note-own">{note.body}</div>}
      </div>
      <p className="note-shared-foot">Shared {stampLabel(note.sharedAt)}{note.expiresAt && <> · Available until {stampLabel(note.expiresAt)}</>}</p>
    </article>
  </>
}
