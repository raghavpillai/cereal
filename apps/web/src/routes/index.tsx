import { createFileRoute, Navigate } from '@tanstack/react-router'
import { useEffect, useState } from 'react'
import { AdminClientError, createAdminClient } from '../lib/admin-client'
import { RoutePending } from '../components/reader-state'

/** The front door is the app itself: the notes list for someone who is signed in, the
 * sign-in page for someone who is not. The session is asked about first, behind a quiet
 * page, so a signed-out visitor never sees the notes page on the way. */
export const Route = createFileRoute('/')({ ssr: false, component: FrontDoor })

function FrontDoor() {
  const [to, setTo] = useState<'notes' | null>(null)
  useEffect(() => {
    const controller = new AbortController()
    createAdminClient(window.location.origin).getSession(controller.signal).then(
      () => { if (!controller.signal.aborted) setTo('notes') },
      error => {
        if (controller.signal.aborted) return
        // Only a clear "not signed in" goes to sign-in. Anything else is left to the notes page, which can say what went wrong and retry.
        if (error instanceof AdminClientError && error.status === 401) window.location.replace('/login')
        else setTo('notes')
      },
    )
    return () => controller.abort()
  }, [])
  return to === 'notes' ? <Navigate to="/notes" replace /> : <RoutePending />
}
