import { createFileRoute } from '@tanstack/react-router'
import { NotesHome } from '../components/notes/notes-home'

export const Route = createFileRoute('/notes/')({ ssr: false, component: NotesHome })
