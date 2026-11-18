/** "Update server" on the admin panel: what the server answers (apps/api/src/updates.ts), and how the page reads it. */
import { AdminClientError } from './client-error'

export type UpdateRelation = 'same' | 'newer' | 'older' | 'different' | 'unknown'
export type UpdateJobStatus = 'running' | 'deployed' | 'up-to-date' | 'rolled-back' | 'rollback-failed' | 'failed-no-previous' | 'failed' | 'interrupted'
export type UpdateStep = 'fetch' | 'preflight' | 'backup' | 'build' | 'start' | 'rollback'
export interface UpdateJob {
  id: string; status: UpdateJobStatus; step: UpdateStep | null; active: boolean
  startedAt: string | null; finishedAt: string | null
  from: { version: string | null; commit: string | null }; to: { version: string | null; commit: string | null }
  error: string | null
}
export interface UpdateStatus {
  current: { version: string; commit: string | null }
  updater: { state: 'missing' | 'stopped' | 'ready'; source: string | null; ref: string | null }
  available: null | { checkedAt: string | null; version: string | null; commit: string | null; relation: UpdateRelation | null; error: string | null }
  job: UpdateJob | null
}
export const UPDATE_PATH = '/api/v1/admin/update'

async function call(path: string, init: RequestInit, signal?: AbortSignal): Promise<unknown> {
  let response: Response
  try { response = await fetch(UPDATE_PATH + path, { ...init, signal, credentials: 'same-origin', cache: 'no-store', redirect: 'error', referrerPolicy: 'no-referrer' }) }
  catch (error) { if (signal?.aborted) throw error; throw new AdminClientError(0) }
  const body: unknown = await response.json().catch(() => null)
  if (!response.ok) throw new AdminClientError(response.status, body && typeof body === 'object' && 'message' in body && typeof body.message === 'string' ? body.message : undefined)
  return body
}
export const fetchUpdateStatus = (signal?: AbortSignal) => call('', { method: 'GET' }, signal) as Promise<UpdateStatus>
/** Asks the updater to look for a newer version, or to install it. The answer is the server's clock, in seconds. */
export const requestUpdate = (action: 'check' | 'start', signal?: AbortSignal) => call('/' + action, { method: 'POST' }, signal) as Promise<{ requestedAt: number }>

export const shortCommit = (commit: string | null | undefined) => commit ? commit.slice(0, 7) : null
/** "0.2.0 (def5678)", or as much of it as is known. */
export const versionLabel = (version: string | null | undefined, commit: string | null | undefined) => [version, shortCommit(commit) ? `(${shortCommit(commit)})` : null].filter(Boolean).join(' ') || 'an unknown version'

/** Whether two job ids name one update. The updater names an update by its start time and, once it has fetched
 * the commit, adds that commit to the name ("20261118T101500Z" becomes "20261118T101500Z-2222222"). */
export const sameUpdate = (a: string, b: string) => a === b || a.startsWith(b + '-') || b.startsWith(a + '-')

/** Whether there is something to install: a commit that is not the running one and not behind it. */
export function updateOffered(status: UpdateStatus): boolean {
  const found = status.available
  return status.updater.state === 'ready' && !!found && !found.error && !!found.relation && found.relation !== 'same' && found.relation !== 'older'
}
/** One line on what the last look found. */
export function availabilityLine(status: UpdateStatus): string {
  const found = status.available, ref = status.updater.ref ?? 'the configured ref'
  if (!found) return 'Not checked yet.'
  if (found.error) return found.error
  const name = versionLabel(found.version, found.commit)
  switch (found.relation) {
    case 'same': return 'This server is up to date.'
    case 'newer': return `Version ${name} is available.`
    case 'older': return `This server runs a later commit than ${ref}, which is ${name}. There is nothing to install.`
    case 'different': return `${ref} is at ${name}, which is not the commit this server runs. It can be installed.`
    default: return `${ref} is at ${name}. What this server runs is not recorded, so it can be installed.`
  }
}

export type StepState = 'done' | 'running' | 'failed' | 'waiting' | 'skipped'
const STEPS: ReadonlyArray<{ id: UpdateStep; label: string }> = [
  { id: 'fetch', label: 'Fetch the new version' },
  { id: 'preflight', label: 'Check the server' },
  { id: 'backup', label: 'Back up the database' },
  { id: 'build', label: 'Build the new version' },
  { id: 'start', label: 'Restart on the new version' },
]
/** The steps of an update that has been asked for and not begun: all of them still to come. */
export const pendingSteps = (): Array<{ id: UpdateStep; label: string; state: StepState }> => STEPS.map(step => ({ ...step, state: 'waiting' }))
/** The steps of an update, each with how far it got. A rollback appears as a sixth step only when it happened. */
export function updateSteps(job: UpdateJob): Array<{ id: UpdateStep; label: string; state: StepState }> {
  const rolledBack = job.step === 'rollback'
  const at = rolledBack ? STEPS.length - 1 : Math.max(0, STEPS.findIndex(step => step.id === job.step))
  const steps = STEPS.map((step, index): { id: UpdateStep; label: string; state: StepState } => ({
    ...step,
    state: job.status === 'deployed' ? 'done'
      : job.status === 'up-to-date' ? (index === 0 ? 'done' : 'skipped')
      : index < at ? 'done' : index > at ? 'waiting'
      : rolledBack ? 'failed' : job.active ? 'running' : 'failed',
  }))
  if (rolledBack) steps.push({ id: 'rollback', label: 'Put the previous version back', state: job.active ? 'running' : job.status === 'rolled-back' ? 'done' : 'failed' })
  return steps
}
/** How an update ended, for the line above its steps. `tone` picks the colour. */
export function updateOutcome(job: UpdateJob): { tone: 'good' | 'bad' | 'neutral'; title: string; detail: string | null } {
  const to = versionLabel(job.to.version, job.to.commit), from = versionLabel(job.from.version, job.from.commit)
  switch (job.status) {
    case 'running': return { tone: 'neutral', title: job.step === 'rollback' ? `Version ${to} did not start. Putting ${from} back…` : job.to.version ? `Updating to ${to}…` : 'Updating…', detail: null }
    case 'deployed': return { tone: 'good', title: `Updated to ${to}.`, detail: `Was ${from}.` }
    case 'up-to-date': return { tone: 'neutral', title: 'Already up to date.', detail: null }
    case 'rolled-back': return { tone: 'bad', title: `The update to ${to} failed and was rolled back.`, detail: `${from} is running again.` }
    case 'rollback-failed': return { tone: 'bad', title: 'The update failed and the previous version did not come back.', detail: job.error }
    case 'failed-no-previous': return { tone: 'bad', title: 'The update failed.', detail: job.error }
    case 'interrupted': return { tone: 'bad', title: 'The update was interrupted.', detail: job.error }
    default: return { tone: 'bad', title: 'The update did not go ahead.', detail: job.error }
  }
}
