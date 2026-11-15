import { createFileRoute } from '@tanstack/react-router'
import { CalendarConnections } from '../components/account/calendar-connections'

/** The desktop and iPhone apps send people here to connect Google Calendar, which has to
 * happen in a browser. It is that one task and nothing else, and no page links to it. */
export const Route = createFileRoute('/account')({
  ssr: false,
  head: () => ({ meta: [{ title: 'Google Calendar · Cereal' }] }),
  component: CalendarConnections,
})
