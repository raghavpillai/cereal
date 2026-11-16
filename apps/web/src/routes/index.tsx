import { createFileRoute, Navigate } from '@tanstack/react-router'
import { useEffect, useState } from 'react'
import { setupLanding } from '../lib/onboarding'
import { readSession, rememberSetup } from '../lib/session-probe'
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
    const controller = new AbortController()
    // A signed-out visitor is answered without the typed client; it is fetched only for someone who is signed in.
    readSession(controller.signal).then(
      async session => {
        if (controller.signal.aborted) return
        if (!session) {
          const { fetchAuthMethods } = await import('../lib/auth-client')
          const unclaimed = await fetchAuthMethods(controller.signal).then(methods => methods.setupRequired, () => false)
          if (controller.signal.aborted) return
          if (unclaimed) rememberSetup(true)
          window.location.replace(unclaimed ? '/admin' : '/login')
          return
        }
        const { createAdminClient } = await import('../lib/admin-client')
        const client = createAdminClient(window.location.origin)
        const setup = await setupLanding(session, () => client.getSettings(controller.signal))
        if (controller.signal.aborted) return
        if (setup) { rememberSetup(true); window.location.replace(setup) } else setTo('notes')
      },
      // Anything but a clear "not signed in" is left to the notes page, which can say what went wrong and retry.
      () => { if (!controller.signal.aborted) setTo('notes') },
    )
    return () => controller.abort()
  }, [])
  return to === 'notes' ? <Navigate to="/notes" replace /> : <RoutePending />
}
