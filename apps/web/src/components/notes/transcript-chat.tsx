import { Link } from '@tanstack/react-router'
import { useCallback, useEffect, useRef, useState, type FormEvent, type KeyboardEvent } from 'react'
import type { ChatCitationDTO } from '@meeting-notes/contracts'
import { NoteText } from '../../lib/markdown'
import { noteTitle } from '../../lib/notes'
import { ChatError, askTranscript, clock, numberedCitations, type ChatTurn } from '../../lib/transcript-chat'
import { AdminClientError } from '../../lib/admin-client'
import { useResource } from '../admin/shared'
import { ReaderLoading } from '../reader-state'
import { useNotes } from './notes-context'

/** `/notes/<id>/chat`: the page the "Chat with meeting transcript" line at the foot of generated notes opens.
 * It is the note's own chat, so it opens for exactly who can open the note, its owner; for anyone else the note
 * does not open and nothing of it is shown. The thread is not saved. */
export function TranscriptChatPage({ noteId }: { noteId: string }) {
  const { expired, notes } = useNotes()
  // Not found (another account's note, or one that is gone) is an answer, not a failure to retry.
  const load = useCallback(() => notes.read(noteId).catch((error: unknown) => { if (error instanceof AdminClientError && error.status === 404) return 'missing' as const; throw error }), [notes, noteId])
  const fresh = useResource(load, expired)
  const note = fresh.data === 'missing' ? null : fresh.data ?? notes.full(noteId) ?? null
  const title = note ? noteTitle(note) : null
  useEffect(() => { document.title = title ? `Chat · ${title} · Cereal` : 'Chat · Cereal' }, [title])
  if (fresh.data === 'missing') return <section className="reader-state" role="alert"><h1 className="app-heading">This chat isn’t available.</h1><p>Only the person whose meeting this is can chat with its transcript, signed in to this server.</p><div className="reader-state-actions"><Link to="/notes" className="app-pill"><span>All notes</span></Link></div></section>
  if (!note) return fresh.error
    ? <section className="reader-state" role="alert"><h1 className="app-heading">This chat didn’t open.</h1><p>{fresh.error}</p><div className="reader-state-actions"><button type="button" className="app-pill" onClick={() => void fresh.reload()}><span>Try again</span></button><Link to="/notes" className="app-pill"><span>All notes</span></Link></div></section>
    : <ReaderLoading />
  return <TranscriptChat key={noteId} noteId={noteId} title={title!} hasTranscript={note.transcriptSegments.some(segment => segment.text.trim())} expired={expired} />
}

type Turn = ChatTurn & { state?: 'streaming' | 'failed' }

function TranscriptChat({ noteId, title, hasTranscript, expired }: { noteId: string; title: string; hasTranscript: boolean; expired: () => void }) {
  const [turns, setTurns] = useState<readonly Turn[]>([])
  const [draft, setDraft] = useState('')
  const [error, setError] = useState<string | null>(null)
  const busy = turns.at(-1)?.state === 'streaming'
  const request = useRef<AbortController | null>(null), field = useRef<HTMLTextAreaElement>(null), end = useRef<HTMLDivElement>(null)
  useEffect(() => () => request.current?.abort(), [])
  useEffect(() => { end.current?.scrollIntoView({ block: 'end' }) }, [turns])
  useEffect(() => { field.current?.focus() }, [])

  async function ask(question: string) {
    if (!question.trim() || busy) return
    const history = turns.filter(turn => turn.state !== 'failed' && turn.content.trim())
    const controller = new AbortController(); request.current = controller
    setError(null); setDraft('')
    setTurns([...history, { role: 'user', content: question }, { role: 'assistant', content: '', state: 'streaming' }])
    const update = (change: (turn: Turn) => Turn) => setTurns(all => [...all.slice(0, -1), change(all.at(-1)!)])
    let failed: string | null = null
    try {
      await askTranscript(noteId, question, history, event => {
        if (event.type === 'delta') update(turn => ({ ...turn, content: turn.content + event.text }))
        else if (event.type === 'citations') update(turn => ({ ...turn, citations: event.citations ?? [] }))
        else if (event.type === 'error') failed = event.text || 'The answer could not be finished.'
      }, { signal: controller.signal })
    } catch (problem) {
      if (controller.signal.aborted) return
      if (problem instanceof ChatError && problem.status === 401) { expired(); return }
      failed = problem instanceof ChatError ? problem.message : 'The answer could not be finished. Please try again.'
    }
    update(turn => failed && !turn.content.trim() ? { ...turn, state: 'failed' } : { ...turn, state: undefined })
    if (failed) setError(failed)
    field.current?.focus()
  }
  const submit = (event: FormEvent) => { event.preventDefault(); void ask(draft) }
  const keys = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key === 'Enter' && !event.shiftKey && !event.nativeEvent.isComposing) { event.preventDefault(); void ask(draft) }
  }

  return <>
    <article className="note-doc chat-doc">
      <Link to="/notes/$noteId" params={{ noteId }} className="chat-note-link"><span aria-hidden="true">←</span><span>{title}</span></Link>
      <h1 className="note-title">Chat with meeting transcript</h1>
      {!hasTranscript && <p className="note-quiet">This meeting has no transcript yet, so there is nothing to ask about.</p>}
      {turns.length === 0 && hasTranscript && <p className="note-quiet">Ask anything about what was said in this meeting. Answers cite the moments of the transcript they come from.</p>}
      <div className="chat-thread" aria-live="polite">
        {turns.map((turn, index) => turn.role === 'user'
          ? <div key={index} className="utterance" data-own="" data-first=""><div className="utterance-stack"><p className="utterance-bubble chat-question">{turn.content}</p></div></div>
          : turn.state === 'failed' ? null : <Answer key={index} turn={turn} />)}
        {error && <p className="note-quiet is-warning" role="alert">{error}</p>}
        <div ref={end} />
      </div>
    </article>
    <form className="note-footer chat-footer" onSubmit={submit}>
      <div className="note-footer-row">
        <div className="chat-ask">
          <textarea ref={field} rows={1} value={draft} placeholder="Ask about this meeting" aria-label="Ask about this meeting" maxLength={20_000} disabled={!hasTranscript} onChange={event => setDraft(event.target.value)} onKeyDown={keys} />
          <button type="submit" className="app-pill is-primary" disabled={busy || !draft.trim() || !hasTranscript}><span>{busy ? 'Answering…' : 'Ask'}</span></button>
        </div>
      </div>
    </form>
  </>
}

function Answer({ turn }: { turn: Turn }) {
  const { text, sources } = numberedCitations(turn.content, turn.citations ?? [])
  if (!text.trim()) return <div className="chat-answer" role="status"><p className="note-quiet">Reading the transcript…</p></div>
  return <div className="chat-answer">
    <NoteText value={text} />
    {sources.length > 0 && <ol className="chat-sources" aria-label="Sources">{sources.map(({ number, citation }) => <li key={citation.id}><span className="chat-source-number">{number}</span><Source citation={citation} /></li>)}</ol>}
  </div>
}

function Source({ citation }: { citation: ChatCitationDTO }) {
  const where = citation.source === 'transcript' ? (citation.startMs === null ? 'Transcript' : `Transcript · ${clock(citation.startMs)}`) : citation.source === 'notes' ? 'My notes' : 'Generated notes'
  return <span className="chat-source"><span className="app-small">{where}</span><span>{citation.quote}</span></span>
}
