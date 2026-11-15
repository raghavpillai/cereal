import { useCallback, useState } from 'react'
import type { AuthSessionDTO, OrganizationDTO, OrganizationDetailsDTO, OrganizationInvitationDTO, OrganizationMemberDTO } from '@meeting-notes/contracts'
import type { AdminClient } from '../../lib/admin-client'
import { canCancelWorkspaceInvitation, canChangeWorkspaceRole, canCreateWorkspace, canManageWorkspace, canRemoveWorkspaceMember, invitationIsPending, workspaceProblem, workspaceRoleLabel } from '../../lib/workspace-state'
import { Badge, Empty, Loading, Modal, Notice, timestamp, useAdminTask, useResource } from '../admin/shared'

export function Workspaces({ session, client, expired }: { session: AuthSessionDTO; client: AdminClient; expired: () => void }) {
  const load = useCallback(async (signal: AbortSignal) => {
    const [workspaces, incoming] = await Promise.all([client.listOrganizations(signal), client.listOrganizationInvitations(signal)])
    return { ...workspaces, invitations: incoming.invitations }
  }, [client])
  const resource = useResource(load, expired), tasks = useAdminTask(expired)
  const [selected, setSelected] = useState<string | null>(null), [creating, setCreating] = useState(false)
  const [notice, setNotice] = useState<string | null>(null)
  const busy = resource.busy || tasks.pending.size > 0
  async function switchWorkspace(id: string) {
    const result = await tasks.run('switch', signal => client.setActiveOrganization(id, signal))
    if (result.ok) { setNotice('Active workspace updated. Your notes remain private to your account.'); await resource.reload() }
  }
  async function respond(invitation: OrganizationInvitationDTO, accept: boolean) {
    if (!invitationIsPending(invitation)) { await resource.reload(); return }
    const result = await tasks.run('invitation', async signal => { if (accept) await client.acceptOrganizationInvitation(invitation.id, signal); else await client.rejectOrganizationInvitation(invitation.id, signal) })
    if (result.ok) { setNotice(accept ? `You joined ${invitation.organizationName}.` : 'Invitation declined.'); await resource.reload() }
  }
  return <>
    <section className="admin-card">
      <div className="admin-section-title"><div><h2>Workspaces</h2><p>Manage the workspaces you belong to. Workspace roles are separate from instance administration, and membership does not share your private notes.</p></div><div className="account-actions"><button type="button" className="admin-secondary-button" disabled={busy} onClick={() => void resource.reload()}>Refresh</button>{canCreateWorkspace(session.user.role) && <button type="button" className="button" disabled={busy} onClick={() => { tasks.clearError(); setCreating(true) }}>Create workspace</button>}</div></div>
      {notice && <Notice>{notice}</Notice>}{tasks.error && !creating && <Notice error>{tasks.error}</Notice>}
      {resource.error && <Notice error>{resource.error} <button type="button" className="admin-text-button" onClick={() => void resource.reload()}>Retry</button></Notice>}
      {resource.busy && !resource.data && <Loading label="Loading your workspaces…" />}
      {resource.data && <><WorkspaceDirectory organizations={resource.data.organizations} activeId={resource.data.activeOrganizationId} selectedId={selected} busy={busy} open={setSelected} activate={id => void switchWorkspace(id)} />
        {resource.data.organizations.length === 0 && <Empty title="No workspace memberships">An existing workspace owner or administrator can invite your account. Invitations appear below.</Empty>}
        <IncomingInvitations invitations={resource.data.invitations} busy={busy} respond={(invitation, accept) => void respond(invitation, accept)} />
      </>}
    </section>
    {selected && resource.data?.organizations.some(org => org.id === selected) && <WorkspaceDetails key={selected} id={selected} viewerId={session.user.id} client={client} expired={expired} changed={resource.reload} />}
    {creating && <WorkspaceForm title="Create workspace" busy={tasks.pending.size > 0} error={tasks.error} close={() => setCreating(false)} save={async (name, slug) => {
      const result = await tasks.run('create', signal => client.createOrganization({ name, slug: slug! }, signal))
      if (result.ok) { setCreating(false); setSelected(result.value.id); setNotice('Workspace created.'); await resource.reload() }
    }} />}
  </>
}

export function WorkspaceDirectory({ organizations, activeId, selectedId, busy, open, activate }: { organizations: readonly OrganizationDTO[]; activeId: string | null; selectedId: string | null; busy: boolean; open: (id: string) => void; activate: (id: string) => void }) {
  return <div className="workspace-list">{organizations.map(org => <article className={`workspace-row${org.id === selectedId ? ' is-selected' : ''}`} key={org.id}>
    <div className="admin-user-avatar" aria-hidden="true">{org.name.slice(0, 1).toUpperCase()}</div><div className="workspace-info"><h3>{org.name}</h3><p>{org.slug}</p></div>
    <Badge>{workspaceRoleLabel(org.role)}</Badge>{org.id === activeId && <Badge good>Active workspace</Badge>}
    <div className="workspace-actions"><button type="button" className="admin-text-button" disabled={busy} aria-expanded={org.id === selectedId} onClick={() => open(org.id)}>Details<span className="sr-only"> for {org.name}</span></button>{org.id !== activeId && <button type="button" className="admin-secondary-button" disabled={busy} onClick={() => activate(org.id)}>Make active<span className="sr-only">: {org.name}</span></button>}</div>
  </article>)}</div>
}

export function IncomingInvitations({ invitations, busy, respond }: { invitations: readonly OrganizationInvitationDTO[]; busy: boolean; respond: (invitation: OrganizationInvitationDTO, accept: boolean) => void }) {
  return <section className="workspace-incoming" aria-label="Your workspace invitations"><h3>Your invitations</h3><p className="admin-hint">Invitations are tied to your signed-in email address. Accepting gives workspace access, without changing your instance role.</p>
    {invitations.length === 0 ? <p className="admin-hint">No invitations waiting.</p> : invitations.map(invitation => <article className="workspace-row" key={invitation.id}><div className="workspace-info"><h3>{invitation.organizationName}</h3><p>{workspaceRoleLabel(invitation.role)} · {invitationIsPending(invitation) ? 'Expires' : invitation.status === 'pending' ? 'Expired' : invitation.status} {timestamp(invitation.expiresAt)}</p></div>
      {invitationIsPending(invitation) && <div className="workspace-actions"><button type="button" className="admin-secondary-button" disabled={busy} onClick={() => respond(invitation, false)}>Decline<span className="sr-only"> {invitation.organizationName}</span></button><button type="button" className="button" disabled={busy} onClick={() => respond(invitation, true)}>Accept<span className="sr-only"> {invitation.organizationName}</span></button></div>}
    </article>)}
  </section>
}

type Change = { kind: 'remove'; member: OrganizationMemberDTO } | { kind: 'role'; member: OrganizationMemberDTO } | { kind: 'cancel'; invitation: OrganizationInvitationDTO }

function WorkspaceDetails({ id, viewerId, client, expired, changed }: { id: string; viewerId: string; client: AdminClient; expired: () => void; changed: () => Promise<void> }) {
  const load = useCallback((signal: AbortSignal) => client.getOrganization(id, signal), [client, id])
  const resource = useResource(load, expired), tasks = useAdminTask(expired)
  const [renaming, setRenaming] = useState(false), [inviting, setInviting] = useState(false)
  const [change, setChange] = useState<Change | null>(null), [notice, setNotice] = useState<string | null>(null)
  const details = resource.data, busy = resource.busy || tasks.pending.size > 0
  async function refresh() { await Promise.all([resource.reload(), changed()]) }
  async function confirm() {
    if (!change || !details) return
    const role = details.organization.role
    if (change.kind === 'remove' && !canRemoveWorkspaceMember(role, change.member, viewerId) || change.kind === 'role' && !canChangeWorkspaceRole(role, change.member) || change.kind === 'cancel' && !canCancelWorkspaceInvitation(role, change.invitation)) return
    const result = await tasks.run('change', async signal => {
      if (change.kind === 'remove') await client.removeOrganizationMember(id, change.member.id, signal)
      else if (change.kind === 'role') await client.updateOrganizationMemberRole(id, change.member.id, { role: change.member.role === 'admin' ? 'member' : 'admin' }, signal)
      else await client.cancelOrganizationInvitation(id, change.invitation.id, signal)
    })
    if (result.ok) { setChange(null); setNotice('Workspace access updated.'); await refresh() }
  }
  if (!details) return <section className="admin-card">{resource.error ? <Notice error>{resource.error} <button className="admin-text-button" type="button" onClick={() => void resource.reload()}>Retry workspace</button></Notice> : <Loading label="Opening workspace…" />}</section>
  return <section className="admin-card" aria-label={`${details.organization.name} details`} aria-busy={busy}>
    <div className="admin-section-title"><div><h2>{details.organization.name}</h2><p>{details.organization.slug} · {workspaceRoleLabel(details.organization.role)}</p></div><div className="account-actions">{canManageWorkspace(details.organization.role) && <><button type="button" className="admin-secondary-button" disabled={busy} onClick={() => { tasks.clearError(); setRenaming(true) }}>Rename</button><button type="button" className="button" disabled={busy} onClick={() => { tasks.clearError(); setInviting(true) }}>Invite person</button></>}</div></div>
    {notice && <Notice>{notice}</Notice>}{resource.error && <Notice error>{resource.error} <button type="button" className="admin-text-button" onClick={() => void resource.reload()}>Retry</button></Notice>}{tasks.error && !renaming && !inviting && !change && <Notice error>{tasks.error}</Notice>}
    <h3>Members</h3><WorkspaceMembers details={details} viewerId={viewerId} busy={busy} change={value => { tasks.clearError(); setChange(value) }} />
    {canManageWorkspace(details.organization.role) && <><h3 className="workspace-subheading">Sent invitations</h3><p className="admin-hint">Only existing instance accounts can be invited. Recipients accept in Workspaces; no email is sent.</p>
      {details.invitations.length === 0 ? <p className="admin-hint">No invitations.</p> : details.invitations.map(invitation => <article className="workspace-row" key={invitation.id}><div className="workspace-info"><h3>{invitation.email}</h3><p>{workspaceRoleLabel(invitation.role)} · {invitation.status === 'pending' && !invitationIsPending(invitation) ? 'expired' : invitation.status} · Expires {timestamp(invitation.expiresAt)}</p></div>{canCancelWorkspaceInvitation(details.organization.role, invitation) && <button type="button" className="admin-text-button admin-danger-text" disabled={busy} onClick={() => { tasks.clearError(); setChange({ kind: 'cancel', invitation }) }}>Cancel invitation<span className="sr-only"> for {invitation.email}</span></button>}</article>)}
    </>}
    <p className="admin-hint admin-card-footnote">Workspace owners can grant administrator access. Workspace administrators manage ordinary members. Instance administrator access is managed separately in Administration.</p>
    {renaming && <WorkspaceForm title="Rename workspace" initial={details.organization} busy={tasks.pending.size > 0} error={tasks.error} close={() => setRenaming(false)} save={async name => {
      const result = await tasks.run('rename', signal => client.updateOrganization(id, { name }, signal))
      if (result.ok) { setRenaming(false); setNotice('Workspace renamed.'); await refresh() }
    }} />}
    {inviting && <InvitePerson role={details.organization.role} busy={tasks.pending.size > 0} error={tasks.error} close={() => setInviting(false)} save={async (email, role) => {
      const result = await tasks.run('invite', signal => client.inviteOrganizationMember(id, { email, role }, signal))
      if (result.ok) { setInviting(false); setNotice('Invitation created. It is available in the recipient’s Workspaces page.'); await refresh() }
    }} />}
    {change && <Modal title={change.kind === 'remove' ? 'Remove workspace member?' : change.kind === 'cancel' ? 'Cancel invitation?' : change.member.role === 'admin' ? 'Revoke workspace admin?' : 'Grant workspace admin?'} busy={tasks.pending.size > 0} close={() => setChange(null)}>
      <div className="admin-dialog-body"><p>{change.kind === 'remove' ? <>Remove <strong>{change.member.email}</strong> from {details.organization.name}? Their private notes and instance account remain available.</> : change.kind === 'cancel' ? <>Cancel the invitation for <strong>{change.invitation.email}</strong>?</> : <>Change <strong>{change.member.email}</strong> to {change.member.role === 'admin' ? 'a workspace member' : 'a workspace administrator'}? This does not change their instance role.</>}</p>{tasks.error && <Notice error>{tasks.error}</Notice>}</div>
      <div className="admin-dialog-actions"><button type="button" className="admin-secondary-button" disabled={tasks.pending.size > 0} onClick={() => setChange(null)}>Keep current access</button><button type="button" className={change.kind === 'role' && change.member.role === 'member' ? 'button' : 'admin-danger-button'} disabled={tasks.pending.size > 0} onClick={() => void confirm()}>{tasks.pending.size > 0 ? 'Updating…' : 'Confirm'}</button></div>
    </Modal>}
  </section>
}

export function WorkspaceMembers({ details, viewerId, busy, change }: { details: OrganizationDetailsDTO; viewerId: string; busy: boolean; change: (change: Change) => void }) {
  return <div className="admin-users">{details.members.map(member => <article className="admin-user" key={member.id}><div className="admin-user-avatar" aria-hidden="true">{(member.name || member.email).slice(0, 1).toUpperCase()}</div><div className="admin-user-info"><h3>{member.name || 'Unnamed account'}{member.userId === viewerId && <span className="workspace-self">You</span>}</h3><p>{member.email}</p></div><Badge>{workspaceRoleLabel(member.role)}</Badge>
    <div className="workspace-actions">{canChangeWorkspaceRole(details.organization.role, member) && <button type="button" className="admin-text-button" disabled={busy} onClick={() => change({ kind: 'role', member })}>{member.role === 'admin' ? 'Revoke workspace admin' : 'Make workspace admin'}<span className="sr-only"> for {member.email}</span></button>}{canRemoveWorkspaceMember(details.organization.role, member, viewerId) && <button type="button" className="admin-text-button admin-danger-text" disabled={busy} onClick={() => change({ kind: 'remove', member })}>Remove<span className="sr-only"> {member.email}</span></button>}</div>
  </article>)}</div>
}

function WorkspaceForm({ title, initial, busy, error, close, save }: { title: string; initial?: OrganizationDTO; busy: boolean; error: string | null; close: () => void; save: (name: string, slug?: string) => Promise<void> }) {
  const [name, setName] = useState(initial?.name ?? ''), [slug, setSlug] = useState('')
  const problem = workspaceProblem(name, initial ? undefined : slug)
  return <Modal title={title} busy={busy} close={close}><form onSubmit={event => { event.preventDefault(); if (!busy && !problem) void save(name.trim(), initial ? undefined : slug.trim()) }}>
    <fieldset className="admin-fieldset admin-dialog-body" disabled={busy}><label>Workspace name<input value={name} required autoFocus maxLength={100} autoComplete="off" onChange={event => setName(event.target.value)} /></label>{!initial && <label>Workspace address<input value={slug} required minLength={2} maxLength={100} pattern="[a-z0-9]+(-[a-z0-9]+)*" autoComplete="off" autoCapitalize="none" spellCheck={false} onChange={event => setSlug(event.target.value)} /><span className="admin-hint">Lowercase letters, numbers and hyphens. This address cannot be changed here.</span></label>}{problem && name && (initial || slug) && <p className="admin-inline-error">{problem}</p>}{error && <Notice error>{error}</Notice>}</fieldset>
    <div className="admin-dialog-actions"><button className="admin-secondary-button" type="button" disabled={busy} onClick={close}>Cancel</button><button className="button" type="submit" disabled={busy || Boolean(problem) || name.trim() === initial?.name}>{busy ? 'Saving…' : initial ? 'Save name' : 'Create workspace'}</button></div>
  </form></Modal>
}

function InvitePerson({ role, busy, error, close, save }: { role: OrganizationDTO['role']; busy: boolean; error: string | null; close: () => void; save: (email: string, role: 'admin' | 'member') => Promise<void> }) {
  const [email, setEmail] = useState(''), [invitedRole, setInvitedRole] = useState<'admin' | 'member'>('member')
  return <Modal title="Invite a person" busy={busy} close={close}><form onSubmit={event => { event.preventDefault(); if (!busy) void save(email.trim(), role === 'owner' ? invitedRole : 'member') }}>
    <fieldset className="admin-fieldset admin-dialog-body" disabled={busy}><p className="admin-hint">Use an existing instance account’s email. The invitation appears in their Workspaces page; it does not send an email.</p><label>Email address<input type="email" required maxLength={254} value={email} autoFocus autoComplete="off" autoCapitalize="none" spellCheck={false} onChange={event => setEmail(event.target.value)} /></label><label>Workspace role<select value={invitedRole} disabled={role !== 'owner'} onChange={event => setInvitedRole(event.target.value === 'admin' ? 'admin' : 'member')}><option value="member">Member</option>{role === 'owner' && <option value="admin">Workspace administrator</option>}</select></label>{error && <Notice error>{error}</Notice>}</fieldset>
    <div className="admin-dialog-actions"><button type="button" className="admin-secondary-button" disabled={busy} onClick={close}>Cancel</button><button type="submit" className="button" disabled={busy || !email.trim()}>{busy ? 'Inviting…' : 'Create invitation'}</button></div>
  </form></Modal>
}
