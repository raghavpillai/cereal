import { useEffect, useState, type KeyboardEvent, type ReactNode } from 'react'
import { copyText } from '../../lib/clipboard'
import { markdownFileName } from '../../lib/notes'
import { Icon } from '../kit/icon'
import { Popover } from '../kit/popover'

/** Which text of a note is on screen: the generated notes or the person's own. */
export type NoteTab = 'enhanced' | 'notes'

/** What the details chip holds: the day, and who was there ("Me" when no calendar event says otherwise). */
export function DetailsChip({ date, people }: { date: string; people: number }) {
  return <><Icon name="calendar-dots" size={14} /><span>{date}</span><Icon name="people" size={14} className="is-people" /><span>{people > 0 ? people : 'Me'}</span></>
}

/** The joined pair above a note: the person's own notes and the generated ones. The one on
 * screen is a raised capsule showing its name; the other is its glyph alone. It is the desktop app's
 * pair as that draws it where there is no template menu (apps/desktop/src/note.rs, `note_tab_widths`). */
export function NoteViews({ shown, show }: { shown: NoteTab; show: (tab: NoteTab) => void }) {
  function keys(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return
    event.preventDefault()
    const next = shown === 'enhanced' ? 'notes' : 'enhanced'
    show(next)
    event.currentTarget.querySelector<HTMLElement>(`[data-tab="${next}"]`)?.focus()
  }
  return <div className="note-views" role="tablist" aria-label="Note view" onKeyDown={keys}>
    <button type="button" role="tab" data-tab="notes" className="note-view" aria-selected={shown === 'notes'} aria-controls="note-notes" tabIndex={shown === 'notes' ? 0 : -1} aria-label="My notes" title="My notes" onClick={() => show('notes')}>
      <Icon name="note-tab-lines" size={18} />{shown === 'notes' && <span>My notes</span>}
    </button>
    <button type="button" role="tab" data-tab="enhanced" className="note-view is-enhanced" aria-selected={shown === 'enhanced'} aria-controls="note-enhanced" tabIndex={shown === 'enhanced' ? 0 : -1} aria-label="Enhanced" title="Enhanced" onClick={() => show('enhanced')}>
      <Icon name="note-tab-sparkle" size={16} />{shown === 'enhanced' && <span>Enhanced</span>}
    </button>
  </div>
}

/** A note's header actions, drawn in place and dimmed while the note is still on its way, so the rest of the
 * header does not slide across when they arrive. Nothing here can be pressed or focused. `open` adds the
 * "Open in Cereal" pill, for the moment before the page that owns the real one is running; `share` is whether
 * the note will have Share (a note in Trash, or one with no text, has none). */
export function PendingNoteActions({ open = false, share = true }: { open?: boolean; share?: boolean }) {
  return <span className="header-pending" aria-hidden="true">
    {open && <span className="app-pill open-in-app"><span className="mark" /><span className="open-in-app-label">Open in Cereal</span></span>}
    <span className="app-tool"><Icon name="more" size={15} /></span>
    {share && <span className="share-split"><span className="share-button"><Icon name="lock-solid" size={13} /><span>Share</span></span><span className="share-link-button"><Icon name="link-chain" size={14} /></span></span>}
  </span>
}

/** The note's own menu: copy what is on screen or save it as Markdown (`markdown` is null when there is no text
 * to take), then the rows the page adds for where the note is kept. Nothing is kept in browser storage. */
export function NoteOptions({ title, markdown, shown, children }: { title: string; markdown: string | null; shown: NoteTab; children?: (close: () => void) => ReactNode }) {
  const [notice, setNotice] = useState('')
  useEffect(() => { if (!notice) return; const timer = window.setTimeout(() => setNotice(''), 1800); return () => window.clearTimeout(timer) }, [notice])
  async function copy() {
    if (markdown === null) return
    try { await copyText(markdown); setNotice('Copied') } catch { setNotice('Couldn’t copy. Select the text and copy it instead.') }
  }
  function save() {
    if (markdown === null) return
    const url = URL.createObjectURL(new Blob([markdown], { type: 'text/markdown;charset=utf-8' }))
    const link = document.createElement('a')
    link.href = url; link.download = markdownFileName(title)
    link.click()
    setTimeout(() => URL.revokeObjectURL(url), 1000)
  }
  return <>
    <Popover label="Note options" className="note-options" buttonClassName="app-tool" buttonLabel="Note options" button={<Icon name="more" size={15} />}>
      {close => <>
        {markdown !== null && <>
          <button type="button" className="app-menu-row" role="menuitem" onClick={() => { close(); void copy() }}><Icon name="copy" size={16} /><span>{shown === 'enhanced' ? 'Copy enhanced notes' : 'Copy notes'}</span></button>
          <button type="button" className="app-menu-row" role="menuitem" onClick={() => { close(); save() }}><Icon name="file" size={16} /><span>Export Markdown</span></button>
        </>}
        {markdown !== null && children && <div className="app-separator is-roomy" role="separator" />}
        {children?.(close)}
      </>}
    </Popover>
    <span className="sr-only" role="status">{notice}</span>
  </>
}
