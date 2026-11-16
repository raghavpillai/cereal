import { Link } from '@tanstack/react-router'
import { copyText } from '../../lib/clipboard'
import { classifyOrigin } from '@meeting-notes/contracts/transport'
import { useCallback, useEffect, useMemo, useState } from 'react'
import { SessionGate } from '../account/session-gate'
import { Empty, Loading, Modal, Notice, useAdminTask, useResource } from '../admin/shared'
import { createMCPClient, type MCPConnection } from '../../lib/mcp-client'
import { Segmented } from '../kit/segmented'
import { AccountMenu } from '../account/account-menu'
import { AdminClientError } from '../../lib/client-error'
import { mcpConnectionURL, mcpScopeDescriptions, safeOAuthResume, signedOAuthQuery } from '../../lib/mcp-state'

export function MCPConnect() {
  const [url, setURL] = useState<string | null>(null)
  // AI apps only sign in to servers on HTTPS (or on this machine): over plain HTTP the page says so instead of offering an address that cannot work.
  const [needsHttps, setNeedsHttps] = useState(false)
  useEffect(() => { setURL(mcpConnectionURL(window.location.origin)); setNeedsHttps(classifyOrigin(window.location.origin) !== 'secure') }, [])
  return <div className="admin-page mcp-page"><div className="admin-heading"><div><p className="eyebrow">Connections</p><h1>Your notes, where you work.</h1><p>Connect an AI app to your meeting notes.</p></div><Link to="/notes" className="admin-secondary-button">Your notes</Link></div>
    {needsHttps
      ? <section className="admin-card"><Empty title="Connecting AI apps needs an HTTPS address">AI apps only sign in to servers reached over HTTPS. This server is reached over plain HTTP, so apps cannot be connected until it has an HTTPS address.</Empty></section>
      : <ConnectionInstructions url={url} />}
    <SessionGate>{(session, _client, expired, signOut) => <div key={session.user.id}><AccountMenu session={session} signOut={signOut} /><ResumeAuthorization expired={expired} /><ConnectedApps expired={expired} /><p className="admin-page-footnote">Signed in as {session.user.email}.</p></div>}</SessionGate>
  </div>
}

/** An app that asked to connect sent its signed request here, and sign-in brought the person back with it.
 * Now that they are signed in the request carries on: to the page that asks for their consent. Only this
 * instance's own authorization pages are entered from here; an app's callback comes after consent. */
function ResumeAuthorization({ expired }: { expired: () => void }) {
  const [failed, setFailed] = useState(false)
  useEffect(() => {
    const query = signedOAuthQuery(window.location.search)
    if (!query) return
    const controller = new AbortController()
    createMCPClient(window.location.origin).resume(query, controller.signal).then(answer => {
      if (controller.signal.aborted) return
      const next = safeOAuthResume(answer.url, window.location.origin)
      if (next) window.location.assign(next); else setFailed(true)
    }, error => {
      if (controller.signal.aborted) return
      if (error instanceof AdminClientError && error.status === 401) expired(); else setFailed(true)
    })
    return () => controller.abort()
  }, [expired])
  return failed ? <Notice error>This connection could not be resumed. Start again from your app.</Notice> : null
}

export function ConnectionInstructions({ url }: { url: string | null }) {
  const [tab, setTab] = useState<'claude' | 'cursor' | 'other'>('claude')
  const [copied, setCopied] = useState(false), [copyError, setCopyError] = useState(false)
  useEffect(() => { if (!copied) return; const timer = setTimeout(() => setCopied(false), 2500); return () => clearTimeout(timer) }, [copied])
  async function copy() {
    if (!url) return
    try { await copyText(url); setCopied(true); setCopyError(false) } catch { setCopyError(true) }
  }
  return <section className="admin-card"><h2>Connect an app</h2><p>Use this server address, then sign in and choose whether to allow access. Each person connects their own account.</p>
    <label className="mcp-url-label" htmlFor="mcp-server-url">Server address</label><div className="mcp-url"><input id="mcp-server-url" readOnly value={url ?? ''} placeholder="Loading server address…" spellCheck={false} /><button type="button" className="admin-secondary-button" disabled={!url} onClick={() => void copy()}>{copied ? 'Copied' : 'Copy address'}</button></div>
    <div role="status" className="admin-hint">{copied ? 'Server address copied.' : copyError ? 'Select and copy the server address above.' : 'Your app will open this instance to ask for permission.'}</div>
    <Segmented className="mcp-client-tabs" label="Connection instructions" value={tab} onChange={setTab} options={[{ value: 'claude', label: 'Claude Code' }, { value: 'cursor', label: 'Cursor' }, { value: 'other', label: 'Other apps' }]} />
    {tab === 'claude' ? <div className="mcp-client-panel fade-swap" key="claude"><ol className="mcp-steps"><li>Run this command in your terminal:<pre><code>{url ? `claude mcp add --transport http meeting-notes ${url}` : 'Loading server address…'}</code></pre></li><li>Open Claude Code and run <code>/mcp</code>. Choose <strong>meeting-notes</strong>, then authenticate.</li><li>Sign in here and review the permissions before choosing Allow.</li></ol><a href="https://code.claude.com/docs/en/mcp" target="_blank" rel="noopener noreferrer">Claude Code connection guide ↗</a></div>
      : tab === 'cursor' ? <div className="mcp-client-panel fade-swap" key="cursor"><p>Add a remote server to your Cursor MCP configuration:</p><pre><code>{JSON.stringify({ mcpServers: { 'meeting-notes': { url: url ?? 'YOUR_SERVER_ADDRESS' } } }, null, 2)}</code></pre><p>Enable the server in Cursor, complete authentication in your browser, then review and allow the requested permissions.</p><a href="https://cursor.com/docs/mcp" target="_blank" rel="noopener noreferrer">Cursor connection guide ↗</a></div>
      : <div className="mcp-client-panel fade-swap" key="other"><p>Choose a remote MCP server in an app that supports Streamable HTTP and OAuth. Paste the server address above, then complete sign-in and review its permissions.</p><p className="admin-hint">No API key is needed. Disconnect an app below whenever you want to end its access.</p></div>}
  </section>
}

function ConnectedApps({ expired }: { expired: () => void }) {
  const client = useMemo(() => createMCPClient(window.location.origin), [])
  const loader = useCallback((signal: AbortSignal) => client.connections(signal), [client])
  const resource = useResource(loader, expired), task = useAdminTask(expired)
  const [selected, setSelected] = useState<MCPConnection | null>(null)
  async function revoke() {
    if (!selected) return
    const result = await task.run(selected.id, signal => client.revoke(selected.id, signal))
    if (result.ok) { setSelected(null); await resource.reload() }
  }
  return <section className="admin-card"><div className="admin-section-title"><div><h2>Connected apps</h2><p>Apps with access to your account.</p></div><button type="button" className="admin-secondary-button" disabled={resource.busy} onClick={() => void resource.reload()}>Refresh</button></div>
    {resource.error && <Notice error>{resource.error}</Notice>}{task.error && <Notice error>{task.error}</Notice>}
    {resource.busy && !resource.data ? <Loading label="Loading connected apps…" /> : resource.data?.connections.length === 0 ? <Empty title="No connected apps">Connect an app above. You will always be asked to review its permissions first.</Empty> : <div className="mcp-connections">{resource.data?.connections.map(connection => <ConnectionRow key={connection.id} connection={connection} disconnect={() => { task.clearError(); setSelected(connection) }} />)}</div>}
    {selected && <Modal title={`Disconnect ${selected.clientName}?`} busy={task.pending.size > 0} close={() => setSelected(null)}><p>This ends the app’s access to your account. It does not delete your notes or content the app has already received.</p>{task.error && <Notice error>{task.error}</Notice>}<div className="account-actions"><button type="button" className="admin-secondary-button" disabled={task.pending.size > 0} onClick={() => setSelected(null)}>Keep connected</button><button type="button" className="button" disabled={task.pending.size > 0} onClick={() => void revoke()}>{task.pending.size ? 'Disconnecting…' : 'Disconnect app'}</button></div></Modal>}
  </section>
}
export function ConnectionRow({ connection, disconnect }: { connection: MCPConnection; disconnect: () => void }) {
  return <article className="mcp-connection"><div><h3>{connection.clientName}</h3><p className="admin-hint">{connection.scopes.map(scope => mcpScopeDescriptions[scope]?.label ?? scope).join(' · ')}</p><p className="admin-hint">Connected {formatDate(connection.createdAt)}</p></div><button type="button" className="admin-secondary-button" onClick={disconnect}>Disconnect</button></article>
}
function formatDate(value: string) { const date = new Date(value); return Number.isNaN(date.getTime()) ? '' : new Intl.DateTimeFormat(undefined, { dateStyle: 'medium' }).format(date) }
