/** How an update is drawn on the admin panel: the bowl that fills as its steps finish, the words beside it and
 * the numbers that roll. All of it is read from what the server answers (./updates.ts); nothing here decides
 * what an update does. The desktop app tells its own update the same way (apps/desktop/src/update_bowl.rs). */
import { updateSteps, type StepState, type UpdateJob, type UpdateStep } from './updates'

/** The steps the bowl is filled by: one pour each. A rollback is not one of them. */
export const POURS = 5

/** What the bowl is doing.
 *  rest: still. empty: an outline the flakes dive into, before the first pour. pour: filling.
 *  orbit: holding while the server restarts, its flakes going round. pop: the update is in.
 *  spill: it failed, and the bowl tips and runs out. */
export type BowlAct = 'rest' | 'empty' | 'pour' | 'orbit' | 'pop' | 'spill'
/** What a still bowl is made of: gold (an update that went in), ink (nothing was installed), or only its
 * outline (an update that failed). */
export type BowlTone = 'gold' | 'ink' | 'hollow'
export interface UpdateScene {
  /** starting: asked for, not begun. running: a step is under way. restarting: the server is away.
   *  ended: the update is over. */
  phase: 'starting' | 'running' | 'restarting' | 'ended'
  act: BowlAct
  tone: BowlTone
  /** How full the bowl is, 0 to 1. */
  level: number
  /** How many pours are in: a flake drops for each. */
  pours: number
  /** The small form: an update this page did not watch, or one whose ending has been played. */
  compact: boolean
}
export interface SceneInput {
  job: UpdateJob | null
  /** This page asked for an update and the updater has not begun it. */
  asking: boolean
  /** The progress stream is down and being reopened. */
  reconnecting: boolean
  /** This page saw the update run, so its ending is played and not only shown. */
  watched: boolean
  /** That ending has been played. */
  played: boolean
}

const count = (steps: ReadonlyArray<{ state: StepState }>, state: StepState) => steps.slice(0, POURS).filter(step => step.state === state).length

/** The restart follows the build at once, so a connection lost from the build on is the restart. */
export const restartExpected = (step: UpdateStep | null) => step === 'build' || step === 'start' || step === 'rollback'

/** How full the bowl is: a fifth for every step done and half a fifth for the one under way. The last pour is
 * the restart itself, so the bowl is full from the moment it begins and holds until the server answers. An
 * update that failed, or is being rolled back, leaves it empty. */
export function bowlLevel(job: UpdateJob | null): number {
  if (!job) return 0
  if (job.status === 'deployed' || job.status === 'up-to-date') return 1
  if (job.status !== 'running' || job.step === 'rollback') return 0
  if (job.step === 'start') return 1
  const steps = updateSteps(job)
  return (count(steps, 'done') + 0.5 * count(steps, 'running')) / POURS
}

export function updateScene({ job, asking, reconnecting, watched, played }: SceneInput): UpdateScene {
  if (asking || !job) return { phase: 'starting', act: 'empty', tone: 'gold', level: 0, pours: 0, compact: false }
  const level = bowlLevel(job), pours = job.status === 'deployed' ? POURS : job.status === 'up-to-date' ? 0 : count(updateSteps(job), 'done')
  if (job.active) {
    const away = reconnecting && restartExpected(job.step)
    return { phase: away ? 'restarting' : 'running', act: away ? 'orbit' : job.step === 'rollback' ? 'spill' : 'pour', tone: job.step === 'rollback' ? 'hollow' : 'gold', level, pours, compact: false }
  }
  const compact = !watched || played
  if (job.status === 'deployed') return { phase: 'ended', act: compact ? 'rest' : 'pop', tone: 'gold', level, pours, compact }
  if (job.status === 'up-to-date') return { phase: 'ended', act: 'rest', tone: 'ink', level, pours, compact }
  return { phase: 'ended', act: compact ? 'rest' : 'spill', tone: 'hollow', level, pours, compact }
}

/** Where the surface is, in the mark's own units (its box is 24 high, the bowl runs from 1.965 down to 22.55).
 * Full and empty go a little past the bowl's edges, so a ripple never shows a gap at the brim or a line at the
 * bottom. */
export const BOWL_TOP = 1.965, BOWL_BOTTOM = 22.55
export function surfaceY(level: number): number {
  if (!(level > 0)) return BOWL_BOTTOM + 1.2
  if (level >= 1) return BOWL_TOP - 1.2
  return BOWL_BOTTOM - level * (BOWL_BOTTOM - BOWL_TOP)
}

/** "Step 3 of 5": which step the words beside the bowl are about. Null before the first and for an update that
 * found nothing to do. */
export function stepCount(job: UpdateJob | null): { at: number; of: number } | null {
  if (!job || job.status === 'up-to-date') return null
  if (job.status === 'deployed') return { at: POURS, of: POURS }
  const at = updateSteps(job).slice(0, POURS).findIndex(step => step.state === 'running' || step.state === 'failed')
  return at < 0 ? null : { at: at + 1, of: POURS }
}

/** The step named beside the bowl while an update runs. */
export function currentStep(job: UpdateJob): { id: UpdateStep; label: string } | null {
  const steps = updateSteps(job)
  return steps.find(step => step.state === 'running') ?? [...steps].reverse().find(step => step.state === 'failed') ?? null
}

/** The large line beside the bowl. While the update runs it is the step; afterwards it is how it ended, in a
 * few words (the sentence under it says exactly what happened). */
export function sceneHeadline(job: UpdateJob | null, asking: boolean): string {
  if (asking || !job) return 'Getting started'
  switch (job.status) {
    case 'running': return currentStep(job)?.label ?? 'Updating'
    case 'deployed': return job.to.version ? `Cereal ${job.to.version}` : 'Updated'
    case 'up-to-date': return 'Nothing to install'
    case 'rolled-back': return job.from.version ? `Back on ${job.from.version}` : 'Rolled back'
    case 'interrupted': return 'Update interrupted'
    case 'failed': return 'Update stopped'
    default: return 'Update failed'
  }
}

/** The small line above it, without the time. */
export function sceneCaption(job: UpdateJob | null, asking: boolean): string {
  if (asking || !job) return 'Starting'
  const step = stepCount(job)
  switch (job.status) {
    case 'running': return job.step === 'rollback' ? 'Rolling back' : step ? `Step ${step.at} of ${step.of}` : 'Updating'
    case 'deployed': return 'Updated'
    case 'up-to-date': return 'Checked'
    case 'rolled-back': return 'Rolled back'
    default: return step ? `Stopped at step ${step.at} of ${step.of}` : 'Stopped'
  }
}

/** What a screen reader is told as the update moves on: once per step, never per frame. Empty when the
 * sentence on the page already says it. */
export function sceneAnnouncement(job: UpdateJob | null, asking: boolean): string {
  if (asking || !job || job.status !== 'running') return ''
  const step = stepCount(job), label = currentStep(job)?.label
  if (!label) return ''
  return job.step === 'rollback' || !step ? `${label}.` : `Step ${step.at} of ${step.of}: ${label}.`
}

/** "0:07", "1:24", "1:02:03". */
export function elapsedLabel(milliseconds: number): string {
  const total = Number.isFinite(milliseconds) && milliseconds > 0 ? Math.floor(milliseconds / 1000) : 0
  const hours = Math.floor(total / 3600), minutes = Math.floor(total % 3600 / 60), seconds = String(total % 60).padStart(2, '0')
  return hours ? `${hours}:${String(minutes).padStart(2, '0')}:${seconds}` : `${minutes}:${seconds}`
}
/** How long the update has run, or ran. `now` is this page's clock; the rest is the server's. */
export function jobElapsed(job: UpdateJob | null, now: number): number | null {
  const started = job?.startedAt ? Date.parse(job.startedAt) : NaN
  if (Number.isNaN(started)) return null
  const until = job!.finishedAt ? Date.parse(job!.finishedAt) : now
  return Math.max(0, (Number.isNaN(until) ? now : until) - started)
}

/** A version rolling from one number to the next, a character at a time: which characters stay, and which
 * turn from what into what. A version that grew or shrank rolls from, or to, nothing. */
export function versionRoll(from: string | null | undefined, to: string): Array<{ from: string; to: string; changed: boolean }> {
  const before = from ?? ''
  // Segments line up from the left ("0.9.0" to "0.10.0" turns "9" into "10"), characters within one from the right.
  const old = before.split('.'), next = to.split('.'), cells: Array<{ from: string; to: string; changed: boolean }> = []
  for (let part = 0; part < Math.max(old.length, next.length); part++) {
    const a = old[part] ?? '', b = next[part] ?? '', width = Math.max(a.length, b.length)
    if (part > 0) cells.push(part < old.length && part < next.length ? { from: '.', to: '.', changed: false } : { from: part < old.length ? '.' : '', to: part < next.length ? '.' : '', changed: true })
    for (let index = 0; index < width; index++) {
      const x = a[index - (width - a.length)] ?? '', y = b[index - (width - b.length)] ?? ''
      cells.push({ from: x, to: y, changed: x !== y })
    }
  }
  return before ? cells : cells.map(cell => ({ ...cell, from: cell.to, changed: false }))
}

/** The mark (src/assets/mark.svg) taken apart: its two flakes and the bowl, each a closed run of the icon's
 * own points, with where each flake sits. Nothing is redrawn. */
export interface MarkParts { bowl: string; flakes: Array<{ path: string; x: number; y: number; radius: number }>; middle: { x: number; y: number }; radius: number }
export function markParts(source: string): MarkParts {
  const data = /\sd="([^"]+)"/.exec(source)?.[1] ?? ''
  const runs = data.split(/(?<=Z)/i).map(run => run.trim()).filter(Boolean).map(path => {
    const points = path.replace(/^M/i, '').replace(/Z$/i, '').trim().split(/\s+/).map(pair => pair.split(',').map(Number)).filter(pair => pair.length === 2 && pair.every(Number.isFinite))
    const xs = points.map(point => point[0]!), ys = points.map(point => point[1]!)
    const left = Math.min(...xs), right = Math.max(...xs), top = Math.min(...ys), bottom = Math.max(...ys)
    return { path, x: (left + right) / 2, y: (top + bottom) / 2, radius: (right - left) / 2, height: bottom - top }
  }).sort((a, b) => b.radius - a.radius)
  const bowl = runs[0]
  if (!bowl) return { bowl: '', flakes: [], middle: { x: 12, y: 12 }, radius: 10 }
  const radius = bowl.height / 2
  // The bowl's ring is a circle with a bite out of its right side: its middle is found from the left edge.
  const middle = { x: bowl.x - bowl.radius + radius, y: bowl.y }
  return { bowl: bowl.path, flakes: runs.slice(1).map(({ path, x, y, radius }) => ({ path, x, y, radius })).sort((a, b) => a.radius - b.radius), middle, radius }
}
