import { memo, startTransition, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import type { TranscriptSegmentDTO } from '@meeting-notes/contracts'
import { Words } from '../../lib/markdown'
import { transcriptUtterances, type Utterance } from '../../lib/notes'
import { Icon } from '../kit/icon'

/** Bubbles mounted at once. A long meeting has thousands; the rest follow in a
 * low-priority pass so opening the transcript never waits on all of them. */
const FIRST_ROWS = 150, MORE_ROWS = 600

/** The transcript, as the desktop app shows it: a card that unfolds from the capsule at
 * the foot of the note, one bubble for each thing said, the reader's own microphone on
 * the right. Controls for recording (resume, audio input, language) are not on the web. */
export const TranscriptPanel = memo(function TranscriptPanel({ segments, close, copy }: { segments: readonly TranscriptSegmentDTO[]; close: () => void; copy: () => Promise<boolean> }) {
  const rows = useMemo(() => transcriptUtterances(segments), [segments])
  const [mounted, setMounted] = useState(FIRST_ROWS), [copied, setCopied] = useState(false)
  const body = useRef<HTMLDivElement>(null), panel = useRef<HTMLElement>(null)
  // The newest rows are mounted first, because the panel opens on the newest text.
  useEffect(() => { if (mounted < rows.length) startTransition(() => setMounted(count => count + MORE_ROWS)) }, [mounted, rows.length])
  const from = Math.max(0, rows.length - mounted)
  // Opening the panel shows the newest text; rows mounted above it afterwards leave the reader where they are.
  const fromEnd = useRef(0)
  useLayoutEffect(() => { const element = body.current; if (element) element.scrollTop = element.scrollHeight - element.clientHeight - fromEnd.current }, [from])
  useEffect(() => { panel.current?.focus({ preventScroll: true }) }, [])
  useEffect(() => { if (!copied) return; const timer = window.setTimeout(() => setCopied(false), 1200); return () => window.clearTimeout(timer) }, [copied])
  return <div className="transcript-dock">
    <section className="transcript-panel" ref={panel} aria-label="Transcript" tabIndex={-1} onKeyDown={event => { if (event.key === 'Escape') { event.stopPropagation(); close() } }}>
      <div className="transcript-toolbar">
        <span className="transcript-toolbar-space" />
        <button type="button" className="app-tool" aria-label="Copy transcript" title="Copy transcript" onClick={() => { void copy().then(setCopied) }}><Icon name={copied ? 'check' : 'copy'} size={15} /></button>
        <button type="button" className="app-tool transcript-collapse" aria-label="Hide transcript" title="Hide transcript" onClick={close}><span aria-hidden="true" /></button>
        <span className="sr-only" role="status">{copied ? 'Transcript copied' : ''}</span>
      </div>
      <div className="transcript-body" ref={body} tabIndex={0} role="region" aria-label="What was said" onScroll={event => { const element = event.currentTarget; fromEnd.current = element.scrollHeight - element.clientHeight - element.scrollTop }}>
        <ol className="transcript-rows">{rows.slice(from).map((row, at) => <Bubble key={row.id} row={row} first={from + at === 0} last={from + at === rows.length - 1} />)}</ol>
      </div>
      <div className="app-separator" />
      <div className="transcript-foot">
        <span className="transcript-capsule is-open"><TranscriptToggle open toggle={close} /></span>
      </div>
    </section>
  </div>
})

const Bubble = memo(function Bubble({ row, first, last }: { row: Utterance; first: boolean; last: boolean }) {
  return <li className="utterance" data-own={row.own || undefined} data-first={first || undefined} data-last={last || undefined} data-joins-before={row.sameBefore || undefined} data-joins-after={row.sameAfter || undefined} title={[row.source, row.speaker].filter(Boolean).join(' · ')}>
    <div className="utterance-stack">
      {row.names && <span className="utterance-speaker">{row.speaker}</span>}
      <p className={row.final ? 'utterance-bubble' : 'utterance-bubble is-interim'}><Words text={row.text} /><time className="utterance-time" aria-label={`at ${row.time}`}>{row.time}</time></p>
    </div>
  </li>
})

/** The capsule's one control: the transcript glyph and a chevron that points the way the card will go. */
export function TranscriptToggle({ open, toggle }: { open: boolean; toggle: () => void }) {
  return <button type="button" className="transcript-toggle" aria-label={open ? 'Hide transcript' : 'Show transcript'} title={open ? 'Hide transcript' : 'Show transcript'} aria-expanded={open} onClick={toggle}>
    <Icon name="footer-transcript" size={32} className="transcript-glyph" />
    <Icon name="chevron-down" size={16} className="transcript-chevron" />
  </button>
}
