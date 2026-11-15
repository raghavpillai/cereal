import { createFileRoute, useRouter } from '@tanstack/react-router'
import { ReaderError, ReaderLoading, ReaderUnavailable } from '../components/reader-state'
import { SharedNoteReader } from '../components/shared-note-reader'

export const Route = createFileRoute('/s/$token')({
  ssr: false,
  staleTime: 0,
  gcTime: 0,
  // Imported when a shared link is opened: the typed API client and its schemas
  // are large, and no other page should download them with the entry bundle.
  loader: async ({ params, abortController }) => (await import('../lib/shared-note')).getSharedNote(params.token, abortController.signal),
  pendingComponent: ReaderLoading,
  errorComponent: LoadError,
  component: Shared,
})

function Shared() {
  const result = Route.useLoaderData()
  return result.kind === 'ready' ? <SharedNoteReader key={result.note.shareId} note={result.note} /> : <ReaderUnavailable />
}
function LoadError() {
  const router = useRouter()
  return <ReaderError retry={() => { void router.invalidate() }} />
}
