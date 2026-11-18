import { createFileRoute } from '@tanstack/react-router'
import { TrashPage } from '../components/notes/trash'

/** Trash: the notes the owner has put away. Inside the notes layout, so it has the same session check and the same cache. */
export const Route = createFileRoute('/notes/trash')({
  ssr: false,
  head: () => ({ meta: [{ title: 'Trash · Cereal' }] }),
  component: TrashPage,
})
