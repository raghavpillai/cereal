import { createFileRoute } from '@tanstack/react-router'
import { AdminPortal } from '../components/admin/admin-portal'

export const Route = createFileRoute('/admin')({
  ssr: false,
  head: () => ({ meta: [{ title: 'Administration · Cereal' }] }),
  component: AdminPortal,
})
