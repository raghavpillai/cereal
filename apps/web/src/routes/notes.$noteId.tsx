import { createFileRoute } from '@tanstack/react-router'
import { NotePage } from '../components/notes/note-page'

export const Route = createFileRoute('/notes/$noteId')({
  ssr: false,
  head: () => ({ meta: [{ title: 'Note · Cereal' }] }),
  component: () => <NotePage noteId={Route.useParams().noteId} />,
})
