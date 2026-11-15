import { createFileRoute } from '@tanstack/react-router'
import { AccountPortal } from '../components/account/account-portal'

export const Route = createFileRoute('/account')({
  ssr: false,
  head: () => ({ meta: [{ title: 'Account · Cereal' }] }),
  component: AccountPortal,
})
