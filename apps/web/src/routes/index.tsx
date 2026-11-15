import { createFileRoute, Navigate } from '@tanstack/react-router'
import { useEffect, useState } from 'react'
import { AdminClientError, createAdminClient } from '../lib/admin-client'
import { fetchAuthMethods } from '../lib/auth-client'
import { setupLanding } from '../lib/onboarding'
import { RoutePending } from '../components/reader-state'

/** The front door is the app itself: the notes list for someone who is signed in, the
 * sign-in page for someone who is not. The session is asked about first, behind a quiet
 * page, so a signed-out visitor never sees the notes page on the way. A server nobody has
 * claimed yet has neither: its front door is the first step of setup, and an owner or
 * administrator who left setup half-way is taken back to it. */
export const Route = createFileRoute('/')({ ssr: false, component: FrontDoor })

function FrontDoor() {
  const [to, setTo] = useState<'notes' | null>(null)
  useEffect(() => {
    const controller = new AbortController(), client = createAdminClient(window.location.origin)
    client.getSession(controller.signal).then(
      async session => {
        const setup = await setupLanding(session, () => client.getSettings(controller.signal))
        if (controller.signal.aborted) return
        if (setup) window.location.replace(setup); else setTo('notes')
      },
      async error => {
        if (controller.signal.aborted) return
        // Only a clear "not signed in" goes to sign-in. Anything else is left to the notes page, which can say what went wrong and retry.
        if (!(error instanceof AdminClientError && error.status === 401)) { setTo('notes'); return }
        const unclaimed = await fetchAuthMethods(controller.signal).then(methods => methods.setupRequired, () => false)
        if (!controller.signal.aborted) window.location.replace(unclaimed ? '/admin' : '/login')
      },
    )
    return () => controller.abort()
  }, [])
  return to === 'notes' ? <Navigate to="/notes" replace /> : <RoutePending />
}
