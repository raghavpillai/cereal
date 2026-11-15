import { createFileRoute } from '@tanstack/react-router'
import { MCPConsent } from '../components/mcp/consent'
export const Route = createFileRoute('/mcp/consent')({ ssr: false, head: () => ({ meta: [{ title: 'Review connection · Cereal' }] }), component: MCPConsent })
