import { memo, startTransition, useEffect, useMemo, useState } from 'react'
import type { TranscriptSegmentDTO } from '@meeting-notes/contracts'
import { formatClock, initials, transcriptTurns, turnSource, type TranscriptTurn } from '../../lib/notes'

/** Turns mounted at once. A long meeting has thousands; the rest follow in a
 * low-priority pass so switching to the transcript never waits on all of them. */
const FIRST_TURNS = 150, MORE_TURNS = 600

/** Speaker turns in the order they were spoken. Labels come from the server's
 * diarization; they name voices within one audio part, not identified people. */
export const Transcript = memo(function Transcript({ segments }: { segments: readonly TranscriptSegmentDTO[] }) {
  const turns = useMemo(() => transcriptTurns(segments), [segments])
  const [mounted, setMounted] = useState(FIRST_TURNS)
  useEffect(() => { if (mounted < turns.length) startTransition(() => setMounted(count => count + MORE_TURNS)) }, [mounted, turns.length])
  const voices = useMemo(() => new Set(turns.map(turn => turn.key)).size, [turns]), parts = turns.some(turn => turn.part !== null)
  return <>
    <p className="transcript-note">{voices} {voices === 1 ? 'voice' : 'voices'} · Speaker labels are automatic{parts ? ' and are matched within each audio part, not across them' : ''}.</p>
    <ol className="transcript">
      {(mounted < turns.length ? turns.slice(0, mounted) : turns).map((turn, index) => <Turn key={`${turn.pieces[0]!.id}-${index}`} turn={turn} />)}
    </ol>
  </>
})

const Turn = memo(function Turn({ turn }: { turn: TranscriptTurn }) {
  const clock = formatClock(turn.startMs), settled = turn.pieces.every(piece => piece.final)
  return <li className="turn">
    <span className="avatar is-small" data-tone={turn.voice % 6} aria-hidden="true">{initials(turn.speaker)}</span>
    <div className="turn-head"><span className="turn-speaker">{turn.speaker}</span><span className="turn-source">{turnSource(turn)}</span><time className="turn-time" aria-label={`at ${clock}`}>{clock}</time></div>
    <p className="turn-text">{settled
      ? turn.pieces.map(piece => piece.text).join(' ')
      : turn.pieces.map((piece, at) => <span key={piece.id} className={piece.final ? undefined : 'is-interim'}>{at > 0 ? ' ' : ''}{piece.text}</span>)}</p>
  </li>
})
