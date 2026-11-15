import { createFileRoute, Navigate } from '@tanstack/react-router'
import { SessionGate } from '../components/account/session-gate'

export const Route = createFileRoute('/login')({
  ssr: false,
  head: () => ({ meta: [{ title: 'Sign in · Cereal' }] }),
  component: () => <SessionGate>{() => <Navigate to="/notes" replace />}</SessionGate>,
})
