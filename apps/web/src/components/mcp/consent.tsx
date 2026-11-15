import { useCallback, useMemo, useState } from 'react'
import { SessionGate } from '../account/session-gate'
import { Loading, Notice, useAdminTask, useResource } from '../admin/shared'
import { createMCPClient, type MCPAuthorization } from '../../lib/mcp-client'
import { knownMCPScope, mcpScopeDescriptions, safeConsentCallback, signedOAuthQuery } from '../../lib/mcp-state'

export function MCPConsent() {
  // This page is client-only. Identity and requested scopes come from the signed
  // server preview, never client_name/scope parameters supplied by the browser.
  const query = typeof window === 'undefined' ? null : signedOAuthQuery(window.location.search)
  if (!query) return <div className="admin-page mcp-page"><InvalidAuthorization /></div>
  return <div className="admin-page mcp-page"><SessionGate>{(session, _client, expired) => <ConsentRequest key={session.user.id} query={query} email={session.user.email} expired={expired} />}</SessionGate></div>
}

function ConsentRequest({ query, email, expired }: { query: string; email: string; expired: () => void }) {
  const client = useMemo(() => createMCPClient(window.location.origin), [])
  const load = useCallback((signal: AbortSignal) => client.authorization(query, signal), [client, query])
  const resource = useResource(load, expired), task = useAdminTask(expired)
  const [redirectError, setRedirectError] = useState(false)
  async function decide(accept: boolean) {
    if (!resource.data || task.pending.size || (accept && !resource.data.scopes.every(knownMCPScope))) return
    setRedirectError(false)
    const preview = resource.data
    const result = await task.run('consent', signal => client.consent(query, accept, signal))
    if (result.ok) {
      const target = safeConsentCallback(result.value.url, preview.redirectUri)
      if (target) window.location.assign(target)
      else setRedirectError(true)
    }
  }
  return resource.busy && !resource.data ? <Loading label="Checking this connection…" /> : resource.data ? <ConsentDetails authorization={resource.data} email={email} busy={task.pending.size > 0} error={redirectError ? 'Your choice was saved, but the app could not be reopened safely. Return to your app and try connecting again.' : task.error ?? resource.error} decide={accept => void decide(accept)} /> : <InvalidAuthorization />
}

export function InvalidAuthorization() {
  return <section className="admin-card mcp-consent"><p className="eyebrow">Connection unavailable</p><h1>Start again from your app.</h1><p>This connection request is missing, expired or could not be verified. Return to the app you are connecting and try again.</p><a href="/mcp/connect" className="admin-secondary-button">Connection instructions</a></section>
}

export function ConsentDetails({ authorization, email, busy, error, decide }: { authorization: MCPAuthorization; email: string; busy: boolean; error: string | null; decide: (accept: boolean) => void }) {
  const unknown = !authorization.scopes.length || authorization.scopes.some(scope => !knownMCPScope(scope))
  return <section className="admin-card mcp-consent"><div className="mcp-consent-mark" aria-hidden="true"><span className="mark" /></div><p className="eyebrow">Connect an app</p><h1>Allow {authorization.clientName} to access your account?</h1><p>Signed in as <strong>{email}</strong></p><p>Access your notes across workspaces you select. Other members’ private notes stay private.</p>
    {authorization.workspace && <p className="admin-hint">Current workspace: <strong>{authorization.workspace.name}</strong></p>}
    <h2>This app is requesting permission to:</h2><ul className="mcp-scopes">{authorization.scopes.map(scope => { const description = mcpScopeDescriptions[scope]; return <li key={scope}><span className={`mcp-scope-icon${description?.write ? ' is-write' : ''}`} aria-hidden="true">{description?.write ? '↗' : '✓'}</span><div><strong>{description?.label ?? 'Unrecognized permission'}</strong><p>{description?.detail ?? scope}</p></div></li> })}</ul>
    {authorization.globalWorkspaceSwitch && authorization.scopes.includes('account:write') && <Notice>Switching workspaces changes your active workspace across connected apps.</Notice>}
    <p className="admin-hint">The app provides its own name. Only continue if you started this connection and trust the app. You can disconnect it later in Connections.</p>
    {unknown && <Notice error>This request includes permissions that cannot be reviewed here. Deny it and start again from your app.</Notice>}{error && <Notice error>{error}</Notice>}
    <div className="mcp-consent-actions"><button type="button" className="admin-secondary-button" disabled={busy} onClick={() => decide(false)}>{busy ? 'Saving…' : 'Deny'}</button><button type="button" className="button" disabled={busy || unknown} onClick={() => decide(true)}>{busy ? 'Saving…' : 'Allow'}</button></div>
  </section>
}
