import { useEffect, useState, type KeyboardEvent } from 'react'
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
 * screen is a raised capsule showing its name; the other is its glyph alone. */
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
      <Icon name="notes-lines" size={15} />{shown === 'notes' && <span>My notes</span>}
    </button>
    <button type="button" role="tab" data-tab="enhanced" className="note-view is-enhanced" aria-selected={shown === 'enhanced'} aria-controls="note-enhanced" tabIndex={shown === 'enhanced' ? 0 : -1} aria-label="Enhanced" title="Enhanced" onClick={() => show('enhanced')}>
      <Icon name={shown === 'enhanced' ? 'sparkles-solid' : 'sparkles'} size={15} />{shown === 'enhanced' && <span>Enhanced</span>}
    </button>
  </div>
}

/** The note's own menu: copy what is on screen, or save it as Markdown. Nothing is kept in browser storage. */
export function NoteOptions({ title, markdown, shown }: { title: string; markdown: string; shown: NoteTab }) {
  const [notice, setNotice] = useState('')
  useEffect(() => { if (!notice) return; const timer = window.setTimeout(() => setNotice(''), 1800); return () => window.clearTimeout(timer) }, [notice])
  async function copy() {
    try { await navigator.clipboard.writeText(markdown); setNotice('Copied') } catch { setNotice('Couldn’t copy. Select the text and copy it instead.') }
  }
  function save() {
    const url = URL.createObjectURL(new Blob([markdown], { type: 'text/markdown;charset=utf-8' }))
    const link = document.createElement('a')
    link.href = url; link.download = markdownFileName(title)
    link.click()
    setTimeout(() => URL.revokeObjectURL(url), 1000)
  }
  return <>
    <Popover label="Note options" className="note-options" buttonClassName="app-tool" buttonLabel="Note options" button={<Icon name="more" size={15} />}>
      {close => <>
        <button type="button" className="app-menu-row" role="menuitem" onClick={() => { close(); void copy() }}><Icon name="copy" size={16} /><span>{shown === 'enhanced' ? 'Copy enhanced notes' : 'Copy notes'}</span></button>
        <button type="button" className="app-menu-row" role="menuitem" onClick={() => { close(); save() }}><Icon name="file" size={16} /><span>Export Markdown</span></button>
      </>}
    </Popover>
    <span className="sr-only" role="status">{notice}</span>
  </>
}
