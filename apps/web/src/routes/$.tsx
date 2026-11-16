import { createFileRoute } from '@tanstack/react-router'
import { PageNotFound } from '../components/reader-state'

/** Every address that is no page. It is a route of its own, loaded in the browser like the others, so the shell
 * that was rendered ahead of time matches what the browser builds; left to the router's own not-found handling,
 * the two differ and the browser throws the page away and builds it again. */
export const Route = createFileRoute('/$')({
  ssr: false,
  head: () => ({ meta: [{ title: 'Not found · Cereal' }] }),
  component: PageNotFound,
})
