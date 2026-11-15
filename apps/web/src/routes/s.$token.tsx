import { createFileRoute } from '@tanstack/react-router'
import { useCallback, useEffect, useRef, useState } from 'react'
import { AccountMenu } from '../components/account/account-menu'
import { SessionGate } from '../components/account/session-gate'
import { ReaderError, ReaderLoading, ReaderUnavailable, RoutePending } from '../components/reader-state'
import { SharedNoteReader } from '../components/shared-note-reader'
import { getSharedNote, type SharedNoteResult } from '../lib/shared-note'

/** A shared link is read by someone signed in to this server. Signed out, the page goes to
 * sign-in and comes back here; nothing of the note is asked for, or shown, before that. */
export const Route = createFileRoute('/s/$token')({
  ssr: false,
  head: () => ({ meta: [{ title: 'Shared note · Cereal' }] }),
  component: () => {
    const { token } = Route.useParams()
    return <SessionGate fallback={<RoutePending />}>{(session, _client, expired, signOut) => <>
      <AccountMenu session={session} signOut={signOut} />
      <SharedNote key={`${session.user.id}:${token}`} token={token} expired={expired} />
    </>}</SessionGate>
  },
})

function SharedNote({ token, expired }: { token: string; expired: () => void }) {
  const [result, setResult] = useState<SharedNoteResult | 'failed' | null>(null)
  const request = useRef<AbortController | null>(null)
  const load = useCallback(async () => {
    request.current?.abort()
    const controller = new AbortController(); request.current = controller
    setResult(null)
    try {
      const answer = await getSharedNote(token, controller.signal)
      if (controller.signal.aborted) return
      // The session ended between the check and the read: back to sign-in, which returns here.
      if (answer.kind === 'signed-out') expired(); else setResult(answer)
    } catch { if (!controller.signal.aborted) setResult('failed') }
  }, [token, expired])
  useEffect(() => { void load(); return () => request.current?.abort() }, [load])
  if (result === null) return <ReaderLoading />
  if (result === 'failed') return <ReaderError retry={() => void load()} />
  return result.kind === 'ready' ? <SharedNoteReader key={result.note.shareId} note={result.note} /> : <ReaderUnavailable />
}
