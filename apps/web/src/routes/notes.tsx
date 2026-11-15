import { createFileRoute, Outlet, useRouterState } from '@tanstack/react-router'
import { AccountMenu } from '../components/account/account-menu'
import { SessionGate } from '../components/account/session-gate'
import { NotesProvider, preloadNotes } from '../components/notes/notes-context'
import { NotesHomeSkeleton } from '../components/notes/notes-home'
import { ReaderLoading } from '../components/reader-state'

/** One session check and one notes cache for the list and every note opened from it. */
export const Route = createFileRoute('/notes')({
  ssr: false,
  head: () => ({ meta: [{ title: 'Notes · Cereal' }] }),
  component: NotesLayout,
})

function NotesLayout() {
  // While the session is checked, show the skeleton of the page that is coming, not a spinner,
  // and on the list start its first page alongside the check. A note opened directly asks for that note alone.
  const onNote = useRouterState({ select: state => /^\/notes\/[^/]+/.test(state.location.pathname) })
  return <SessionGate fallback={onNote ? <ReaderLoading /> : <NotesHomeSkeleton />} onClient={onNote ? undefined : preloadNotes}>{(session, client, expired, signOut) =>
    <NotesProvider key={session.user.id} session={session} client={client} expired={expired}>
      <AccountMenu session={session} signOut={signOut} />
      <Outlet />
    </NotesProvider>
  }</SessionGate>
}
