import { useCallback, useState } from 'react'
import type { CreateUserInput, UserDTO } from '@meeting-notes/contracts'
import type { AdminClient } from '../../lib/admin-client'
import { Badge, Empty, Loading, Modal, Notice, timestamp, useAdminTask, useResource } from './shared'

export function UsersPanel({ client, viewer, expired }: { client: AdminClient; viewer: UserDTO; expired: () => void }) {
  const load = useCallback((signal: AbortSignal) => client.listUsers(signal), [client])
  const resource = useResource(load, expired)
  const tasks = useAdminTask(expired)
  const [creating, setCreating] = useState(false)
  const [changing, setChanging] = useState<UserDTO | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  async function create(input: CreateUserInput) {
    const result = await tasks.run('user:create', signal => client.createUser(input, signal))
    if (!result.ok) return
    setCreating(false); setNotice('Account created with the User role. Share the sign-in details securely; the user can change their password in Account settings.'); await resource.reload()
  }
  async function changeRole(user: UserDTO) {
    if (viewer.role !== 'owner' || user.role === 'owner') return
    const role = user.role === 'admin' ? 'user' : 'admin'
    const result = await tasks.run(`user:${user.id}`, signal => client.updateUserRole(user.id, { role }, signal))
    if (!result.ok) return
    setChanging(null); setNotice(role === 'admin' ? 'Administrator access granted.' : 'Administrator access revoked. The account can still access its own notes.'); await resource.reload()
  }
  return <section className="admin-card">
    <div className="admin-section-title"><div><h2>People and access</h2><p>Every person signs in with their own account. Only the owner can change administrator roles.</p></div><button type="button" className="button" onClick={() => { tasks.clearError(); setNotice(null); setCreating(true) }}>＋ Add user</button></div>
    {notice && <Notice>{notice}</Notice>}{tasks.error && !changing && !creating && <Notice error>{tasks.error}</Notice>}
    {resource.error && <Notice error>{resource.error} <button type="button" className="admin-text-button" onClick={() => void resource.reload()}>Retry</button></Notice>}
    {resource.busy && !resource.data && <Loading label="Loading people…" />}
    {resource.data?.users.length === 0 && <Empty title="No accounts found">Refresh to load the instance directory.</Empty>}
    <UserDirectory users={resource.data?.users ?? []} viewer={viewer} pending={tasks.pending} change={user => { tasks.clearError(); setChanging(user) }} />
    <p className="admin-hint admin-card-footnote">Owner and administrator: instance configuration and directory. User: own notes and shared content. The owner account cannot be reassigned from this page.</p>
    {creating && <CreateUser busy={tasks.pending.has('user:create')} error={tasks.error} close={() => setCreating(false)} save={create} />}
    {changing && <Modal title={changing.role === 'admin' ? 'Revoke administrator access?' : 'Grant administrator access?'} busy={tasks.pending.has(`user:${changing.id}`)} close={() => setChanging(null)}>
      <div className="admin-dialog-body"><p>{changing.role === 'admin' ? <>Remove configuration access from <strong>{changing.email}</strong>? Their own notes remain available.</> : <>Allow <strong>{changing.email}</strong> to manage instance settings, provider configuration, calendars and regular user accounts? They will not be able to grant administrator access.</>}</p>{tasks.error && <Notice error>{tasks.error}</Notice>}</div>
      <div className="admin-dialog-actions"><button type="button" className="admin-secondary-button" disabled={tasks.pending.has(`user:${changing.id}`)} onClick={() => setChanging(null)}>Cancel</button><button type="button" className={changing.role === 'admin' ? 'admin-danger-button' : 'button'} disabled={tasks.pending.has(`user:${changing.id}`)} onClick={() => void changeRole(changing)}>{tasks.pending.has(`user:${changing.id}`) ? 'Saving…' : changing.role === 'admin' ? 'Revoke admin' : 'Grant admin'}</button></div>
    </Modal>}
  </section>
}

export function UserDirectory({ users, viewer, pending, change }: { users: readonly UserDTO[]; viewer: UserDTO; pending: ReadonlySet<string>; change: (user: UserDTO) => void }) {
  return <div className="admin-users">{users.map(user => <article className="admin-user" key={user.id}>
    <div className="admin-user-avatar" aria-hidden="true">{(user.name.trim() || user.email).slice(0, 1).toUpperCase()}</div>
    <div className="admin-user-info"><h3>{user.name || 'Unnamed account'}{user.id === viewer.id && <span className="admin-inline-hint">You</span>}</h3><p>{user.email}</p><span className="admin-hint">Joined {timestamp(user.createdAt)}</span></div>
    <Badge good={user.role !== 'user'}>{user.role === 'owner' ? 'Owner' : user.role === 'admin' ? 'Administrator' : 'User'}</Badge>
    {viewer.role === 'owner' && user.role !== 'owner' && <button type="button" className={`admin-text-button${user.role === 'admin' ? ' admin-danger-text' : ''}`} disabled={pending.has(`user:${user.id}`)} onClick={() => change(user)}>{user.role === 'admin' ? 'Revoke admin' : 'Make admin'}<span className="sr-only"> for {user.email}</span></button>}
  </article>)}</div>
}

function CreateUser({ busy, error, close, save }: { busy: boolean; error: string | null; close: () => void; save: (input: CreateUserInput) => Promise<void> }) {
  const [name, setName] = useState(''), [email, setEmail] = useState(''), [password, setPassword] = useState('')
  return <Modal title="Add a user" busy={busy} close={close}>
    <form onSubmit={event => { event.preventDefault(); if (!busy) void save({ name: name.trim(), email: email.trim(), password }) }}>
      <fieldset disabled={busy} className="admin-fieldset admin-dialog-body">
        <label>Name<input name="newUserName" value={name} maxLength={100} autoComplete="off" required autoFocus onChange={e => setName(e.target.value)} /></label>
        <label>Email address<input name="newUserEmail" type="email" value={email} maxLength={254} autoComplete="off" autoCapitalize="none" spellCheck={false} required onChange={e => setEmail(e.target.value)} /></label>
        <label>Initial password<input name="newUserPassword" type="password" value={password} minLength={12} maxLength={128} autoComplete="new-password" required onChange={e => setPassword(e.target.value)} /><span className="admin-hint">Use at least 12 characters. This account starts as a User; the owner can grant administrator access separately.</span></label>
        {error && <Notice error>{error}</Notice>}
      </fieldset>
      <div className="admin-dialog-actions"><button type="button" className="admin-secondary-button" disabled={busy} onClick={close}>Cancel</button><button type="submit" className="button" disabled={busy || !name.trim() || !email.trim() || password.length < 12}>{busy ? 'Creating…' : 'Create user'}</button></div>
    </form>
  </Modal>
}
