import { createFileRoute } from '@tanstack/react-router'
import { MCPConnect } from '../components/mcp/connect'
export const Route = createFileRoute('/mcp/connect')({ ssr: false, head: () => ({ meta: [{ title: 'Connect your notes · Cereal' }] }), component: MCPConnect })
