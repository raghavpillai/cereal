import { createFileRoute } from '@tanstack/react-router'
import { TranscriptChatPage } from '../components/notes/transcript-chat'

/** The chat with a meeting's transcript, linked from the foot of its generated notes. Inside the notes layout,
 * so it has the same session check (signed out, it goes to sign-in and comes back here) and the same cache. */
export const Route = createFileRoute('/notes/$noteId_/chat')({
  ssr: false,
  head: () => ({ meta: [{ title: 'Chat · Cereal' }] }),
  component: () => <TranscriptChatPage noteId={Route.useParams().noteId} />,
})
