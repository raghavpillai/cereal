import { Schema } from 'effect'
import {McpAuthorizationDTO,McpConnectionDTO,McpConnectionsDTO} from '@meeting-notes/contracts'
import { AdminClientError } from './admin-client'

export const MCPAuthorization = McpAuthorizationDTO
export type MCPAuthorization = typeof MCPAuthorization.Type
export const MCPConnection = McpConnectionDTO
export type MCPConnection = typeof MCPConnection.Type
const Connections = McpConnectionsDTO
const ConsentRedirect = Schema.Struct({ redirect: Schema.Literal(true), url: Schema.String })
const Revoked = Schema.Struct({ success: Schema.Literal(true) })

export function createMCPClient(origin: string, transport: typeof fetch = globalThis.fetch) {
  if (new URL(origin).origin !== origin) throw new AdminClientError(0)
  async function request(path: string, body: unknown | undefined, signal?: AbortSignal): Promise<unknown> {
    const response = await transport(`${origin}/api/auth/${path}`, {
      method: body === undefined ? 'GET' : 'POST', credentials: 'same-origin',
      cache: 'no-store', redirect: 'error', referrerPolicy: 'no-referrer', signal,
      headers: body === undefined ? { Accept: 'application/json' } : { Accept: 'application/json', 'Content-Type': 'application/json' },
      ...(body === undefined ? {} : { body: JSON.stringify(body) }),
    })
    if (!response.ok) throw new AdminClientError(response.status)
    try { return await response.json() } catch { throw new AdminClientError(503) }
  }
  function decode<S extends Schema.ConstraintDecoder<unknown>>(schema: S) {
    return (value: unknown): S['Type'] => {
      try { return Schema.decodeUnknownSync(schema)(value) } catch { throw new AdminClientError(503) }
    }
  }
  return {
    authorization: (query: string, signal?: AbortSignal) => request(`mcp/authorize-info?${new URLSearchParams({ oauth_query: query })}`, undefined, signal).then(decode(MCPAuthorization)),
    connections: (signal?: AbortSignal) => request('mcp/connections', undefined, signal).then(decode(Connections)),
    revoke: (id: string, signal?: AbortSignal) => request(`mcp/connections/${encodeURIComponent(id)}/revoke`, {}, signal).then(decode(Revoked)),
    consent: (query: string, accept: boolean, signal?: AbortSignal) => request('oauth2/consent', { accept, oauth_query: query }, signal).then(decode(ConsentRedirect)),
  }
}
export type MCPClient = ReturnType<typeof createMCPClient>
