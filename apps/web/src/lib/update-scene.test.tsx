import { describe, expect, test } from 'bun:test'
import { renderToStaticMarkup } from 'react-dom/server'
import markSource from '../assets/mark.svg?raw'
import { UpdatesView } from '../components/admin/updates-panel'
import { cssMilliseconds } from './css-time'
import { BOWL_BOTTOM, BOWL_TOP, POURS, bowlLevel, elapsedLabel, jobElapsed, markParts, sceneAnnouncement, sceneCaption, sceneHeadline, stepCount, surfaceY, updateScene, versionRoll } from './update-scene'
import { pendingSteps, sameUpdate, type UpdateJob, type UpdateStatus, type UpdateStep } from './updates'

const A = '1111111111111111111111111111111111111111', B = '2222222222222222222222222222222222222222'
const ready: UpdateStatus = { current: { version: '0.1.0', commit: A }, updater: { state: 'ready', source: 'git@github.com:team/cereal.git', ref: 'main' }, available: { checkedAt: '2026-11-18T10:00:00.000Z', version: '0.2.0', commit: B, relation: 'newer', error: null }, job: null }
const job = (patch: Partial<UpdateJob> = {}): UpdateJob => ({ id: '20261118T101500Z-2222222', status: 'running', step: 'build', active: true, startedAt: '2026-11-18T10:15:00.000Z', finishedAt: null, from: { version: '0.1.0', commit: A }, to: { version: '0.2.0', commit: B }, error: null, ...patch })
const ended = (status: UpdateJob['status'], step: UpdateStep) => job({ status, step, active: false, finishedAt: '2026-11-18T10:15:24.000Z' })
const scene = (value: UpdateJob | null, extra: Partial<Parameters<typeof updateScene>[0]> = {}) => updateScene({ job: value, asking: false, reconnecting: false, watched: false, played: false, ...extra })
const view = (status: UpdateStatus, extra: Partial<Parameters<typeof UpdatesView>[0]> = {}) => renderToStaticMarkup(<UpdatesView status={status} lines={[]} link="live" check={() => {}} update={() => {}} {...extra} />)

describe('how full the bowl is', () => {
  test('a fifth for every step done, half a fifth for the one under way, and full from the restart on', () => {
    expect((['fetch', 'preflight', 'backup', 'build', 'start'] as const).map(step => bowlLevel(job({ step })))).toEqual([0.1, 0.3, 0.5, 0.7, 1])
    expect(bowlLevel(null)).toBe(0)
    expect(bowlLevel(ended('deployed', 'start'))).toBe(1)
    expect(bowlLevel(ended('up-to-date', 'fetch'))).toBe(1)
  })
  test('an update that failed, or is being put back, leaves it empty', () => {
    expect(bowlLevel(job({ step: 'rollback' }))).toBe(0)
    for (const status of ['rolled-back', 'rollback-failed', 'failed-no-previous', 'failed', 'interrupted'] as const) expect(bowlLevel(ended(status, 'build'))).toBe(0)
  })
  test('the surface runs from the bottom of the bowl to its brim, and a little past both ends', () => {
    expect(surfaceY(0.5)).toBeCloseTo((BOWL_TOP + BOWL_BOTTOM) / 2, 6)
    expect(surfaceY(0.1)).toBeGreaterThan(surfaceY(0.3))
    expect(surfaceY(0)).toBeGreaterThan(BOWL_BOTTOM)
    expect(surfaceY(1)).toBeLessThan(BOWL_TOP)
    expect(surfaceY(Number.NaN)).toBe(surfaceY(0))
    expect(surfaceY(7)).toBe(surfaceY(1))
  })
})

describe('what the bowl is doing', () => {
  test('asked for, it is an outline; while steps run it is poured, a flake for every step done', () => {
    expect(scene(null, { asking: true })).toEqual({ phase: 'starting', act: 'empty', tone: 'gold', level: 0, pours: 0, compact: false })
    // The update before this one is still what the server answers: the page shows the one it asked for.
    expect(scene(ended('deployed', 'start'), { asking: true })).toMatchObject({ phase: 'starting', act: 'empty', level: 0, compact: false })
    expect(scene(job({ step: 'backup' }))).toEqual({ phase: 'running', act: 'pour', tone: 'gold', level: 0.5, pours: 2, compact: false })
  })
  test('a connection lost from the build on is the restart: the bowl holds and the flakes go round', () => {
    expect(scene(job({ step: 'start' }), { reconnecting: true })).toMatchObject({ phase: 'restarting', act: 'orbit', level: 1, pours: 4 })
    expect(scene(job({ step: 'build' }), { reconnecting: true })).toMatchObject({ phase: 'restarting', act: 'orbit', level: 0.7 })
    expect(scene(job({ step: 'rollback' }), { reconnecting: true })).toMatchObject({ phase: 'restarting', act: 'orbit', level: 0, tone: 'hollow' })
    // Before the build nothing restarts: a lost connection is only that, and the pour is shown as it was.
    expect(scene(job({ step: 'backup' }), { reconnecting: true })).toMatchObject({ phase: 'running', act: 'pour', level: 0.5 })
  })
  test('the ending is played for the page that watched, once, and then it is the summary', () => {
    expect(scene(ended('deployed', 'start'), { watched: true })).toEqual({ phase: 'ended', act: 'pop', tone: 'gold', level: 1, pours: POURS, compact: false })
    expect(scene(ended('deployed', 'start'), { watched: true, played: true })).toMatchObject({ act: 'rest', tone: 'gold', level: 1, compact: true })
    expect(scene(ended('deployed', 'start'))).toMatchObject({ act: 'rest', compact: true })
    expect(scene(job({ step: 'rollback' }))).toMatchObject({ phase: 'running', act: 'spill', tone: 'hollow', level: 0 })
    expect(scene(ended('rolled-back', 'rollback'), { watched: true })).toMatchObject({ act: 'spill', tone: 'hollow', level: 0, pours: 4, compact: false })
    expect(scene(ended('failed', 'build'), { watched: true, played: true })).toMatchObject({ act: 'rest', tone: 'hollow', level: 0, pours: 3, compact: true })
    expect(scene(ended('up-to-date', 'fetch'), { watched: true })).toMatchObject({ act: 'rest', tone: 'ink', pours: 0, compact: false })
  })
})

describe('the words beside the bowl', () => {
  test('the step that is under way, counted', () => {
    expect(stepCount(job({ step: 'fetch' }))).toEqual({ at: 1, of: 5 })
    expect(stepCount(job({ step: 'backup' }))).toEqual({ at: 3, of: 5 })
    expect(stepCount(job({ step: 'rollback' }))).toEqual({ at: 5, of: 5 })
    expect(stepCount(ended('failed', 'build'))).toEqual({ at: 4, of: 5 })
    expect(stepCount(ended('deployed', 'start'))).toEqual({ at: 5, of: 5 })
    expect(stepCount(ended('up-to-date', 'fetch'))).toBeNull()
    expect(stepCount(null)).toBeNull()
    expect(sceneCaption(job({ step: 'backup' }), false)).toBe('Step 3 of 5')
    expect(sceneCaption(job({ step: 'rollback' }), false)).toBe('Rolling back')
    expect(sceneCaption(null, true)).toBe('Starting')
    expect(sceneCaption(ended('failed', 'build'), false)).toBe('Stopped at step 4 of 5')
    expect(sceneCaption(ended('rolled-back', 'rollback'), false)).toBe('Rolled back')
    expect(sceneCaption(ended('deployed', 'start'), false)).toBe('Updated')
  })
  test('the large line names the step, then how it ended', () => {
    expect(sceneHeadline(null, true)).toBe('Getting started')
    expect(sceneHeadline(job({ step: 'backup' }), false)).toBe('Back up the database')
    expect(sceneHeadline(job({ step: 'rollback' }), false)).toBe('Put the previous version back')
    expect(sceneHeadline(ended('deployed', 'start'), false)).toBe('Cereal 0.2.0')
    expect(sceneHeadline(ended('rolled-back', 'rollback'), false)).toBe('Back on 0.1.0')
    expect(sceneHeadline(job({ status: 'rolled-back', step: 'rollback', active: false, from: { version: null, commit: null } }), false)).toBe('Rolled back')
    expect(sceneHeadline(ended('failed', 'build'), false)).toBe('Update stopped')
    expect(sceneHeadline(ended('rollback-failed', 'rollback'), false)).toBe('Update failed')
    expect(sceneHeadline(ended('interrupted', 'start'), false)).toBe('Update interrupted')
    expect(sceneHeadline(ended('up-to-date', 'fetch'), false)).toBe('Nothing to install')
  })
  test('a screen reader hears each step once, and nothing when the page already says how it ended', () => {
    expect(sceneAnnouncement(job({ step: 'backup' }), false)).toBe('Step 3 of 5: Back up the database.')
    expect(sceneAnnouncement(job({ step: 'rollback' }), false)).toBe('Put the previous version back.')
    expect(sceneAnnouncement(null, true)).toBe('')
    expect(sceneAnnouncement(ended('deployed', 'start'), false)).toBe('')
  })
  test('time is told in minutes and seconds', () => {
    expect([0, 999, 7_000, 84_000, 3_723_000, -5, Number.NaN].map(elapsedLabel)).toEqual(['0:00', '0:00', '0:07', '1:24', '1:02:03', '0:00', '0:00'])
    const now = Date.parse('2026-11-18T10:15:14.000Z')
    expect(jobElapsed(job(), now)).toBe(14_000)
    // A page whose clock is behind the server's never shows a negative time.
    expect(jobElapsed(job(), now - 60_000)).toBe(0)
    expect(jobElapsed(ended('deployed', 'start'), now + 3_600_000)).toBe(24_000)
    expect(jobElapsed(job({ startedAt: null }), now)).toBeNull()
    expect(jobElapsed(null, now)).toBeNull()
  })
  test('a version rolls only where it differs', () => {
    expect(versionRoll('0.2.0', '0.3.0')).toEqual([{ from: '0', to: '0', changed: false }, { from: '.', to: '.', changed: false }, { from: '2', to: '3', changed: true }, { from: '.', to: '.', changed: false }, { from: '0', to: '0', changed: false }])
    expect(versionRoll('0.9.9', '0.10.0').filter(cell => cell.changed)).toEqual([{ from: '', to: '1', changed: true }, { from: '9', to: '0', changed: true }, { from: '9', to: '0', changed: true }])
    expect(versionRoll('1.2', '1.2.1').map(cell => cell.to).join('')).toBe('1.2.1')
    expect(versionRoll('1.2', '1.2.1').slice(3)).toEqual([{ from: '', to: '.', changed: true }, { from: '', to: '1', changed: true }])
    // With nothing to roll from, it is simply the number.
    expect(versionRoll(null, '0.3.0').every(cell => !cell.changed)).toBe(true)
    expect(versionRoll(null, '0.3.0').map(cell => cell.to).join('')).toBe('0.3.0')
  })
})

describe('the parts this is built from', () => {
  test('the mark is taken apart into its bowl and its two flakes, nothing redrawn', () => {
    const parts = markParts(markSource), whole = /\sd="([^"]+)"/.exec(markSource)![1]!
    expect(parts.flakes).toHaveLength(2)
    // Every point of the icon is in exactly one part.
    expect([parts.bowl, ...parts.flakes.map(flake => flake.path)].join('').length).toBe(whole.replace(/\s+(?=M)/g, '').length)
    for (const part of [parts.bowl, ...parts.flakes.map(flake => flake.path)]) expect(whole).toContain(part)
    expect(parts.flakes[0]!.radius).toBeLessThan(parts.flakes[1]!.radius)
    // The ring's middle and radius, as the desktop app has them (apps/desktop/src/motion.rs, LOADER_BOWL and LOADER_RING).
    expect(parts.middle.x).toBeCloseTo(10.9925, 3); expect(parts.middle.y).toBeCloseTo(12.2575, 3); expect(parts.radius).toBeCloseTo(10.2925, 3)
    expect(parts.middle.y - parts.radius).toBeCloseTo(BOWL_TOP, 3); expect(parts.middle.y + parts.radius).toBeCloseTo(BOWL_BOTTOM, 3)
    expect(markParts('<svg/>')).toMatchObject({ bowl: '', flakes: [] })
  })
  test('a time from the stylesheet is read in either unit', () => {
    expect(cssMilliseconds('270ms', 1)).toBe(270)
    // What a minifier makes of 270ms: read as a number alone it would be a quarter of a millisecond.
    expect(cssMilliseconds('.27s', 1)).toBe(270)
    expect(cssMilliseconds(' 0.42s ', 1)).toBe(420)
    for (const odd of ['', 'fast', '12', '-3ms', null, undefined]) expect(cssMilliseconds(odd, 200)).toBe(200)
  })
  test('an update keeps its name when the updater adds the commit to it', () => {
    expect(sameUpdate('20261118T101500Z-2222222', '20261118T101500Z')).toBe(true)
    expect(sameUpdate('20261118T101500Z', '20261118T101500Z')).toBe(true)
    expect(sameUpdate('20261118T101500Z-2222222', '20261118T101501Z')).toBe(false)
    expect(sameUpdate('20261118T1015', '20261118T101500Z')).toBe(false)
    expect(pendingSteps().map(step => step.state)).toEqual(['waiting', 'waiting', 'waiting', 'waiting', 'waiting'])
  })
})

describe('the progress as the page draws it', () => {
  test('the picture is decoration; the step, the count and the outcome are words, and the step is announced politely', () => {
    const markup = view({ ...ready, job: job({ step: 'backup' }) }, { lines: ['10:15:07 Backing up the database…'] })
    expect(markup).toMatch(/<span class="update-bowl is-pour is-gold"[^>]*aria-hidden="true"/)
    expect(markup).toContain('<p class="sr-only" role="status" aria-live="polite">Step 3 of 5: Back up the database.</p>')
    expect(markup).toContain('Back up the database</span></h4>')
    expect(markup).toMatch(/Step <span class="update-count"[^>]*><span class="sr-only">3<\/span>/)
    expect(markup).toContain('Updating to 0.2.0 (2222222)…')
    expect(markup).toContain('<ol class="update-steps" aria-label="Steps">')
    expect(markup.match(/<li class="is-done"/g)).toHaveLength(2)
    expect(markup.match(/<li class="is-waiting"/g)).toHaveLength(2)
  })
  test('the log is behind "Show log", and every line of it is in the page', () => {
    const markup = view({ ...ready, job: job() }, { lines: ['one', 'two <three>'] })
    expect(markup).toMatch(/<button type="button" class="update-log-toggle" aria-expanded="false" aria-controls="([^"]+)">.*Show log<small>2 lines<\/small>/)
    expect(markup).toMatch(/<div id="[^"]+" class="update-log-panel" hidden=""><pre class="update-log" tabindex="0" aria-label="Update log">one\ntwo &lt;three&gt;<\/pre>/)
  })
  test('asked for and not begun: every step still to come, and the updater is being asked', () => {
    const markup = view(ready, { asked: 'start' })
    expect(markup).toContain('Update in progress')
    expect(markup).toContain('Asking the updater to start…')
    expect(markup).toContain('Getting started')
    expect(markup).toContain('update-bowl is-empty')
    expect(markup.match(/<li class="is-waiting"/g)).toHaveLength(5)
  })
  test('an update read afterwards is the summary: what happened, in full, and nothing is played', () => {
    const rolledBack = view({ ...ready, job: ended('rolled-back', 'rollback') })
    expect(rolledBack).toContain('update-progress is-compact is-ended')
    expect(rolledBack).toContain('update-bowl is-rest is-hollow')
    expect(rolledBack).toContain('<strong>The update to 0.2.0 (2222222) failed and was rolled back.</strong> 0.1.0 (1111111) is running again.')
    expect(rolledBack).toContain('Took 0:24.')
    expect(rolledBack).toContain('<ol class="update-steps is-unwound" aria-label="Steps">')
    expect(rolledBack).toMatch(/<li class="is-failed"[^>]*>.*Restart on the new version<\/span><small>Failed<\/small>/)
    expect(rolledBack).toMatch(/<li class="is-done is-rollback"[^>]*>.*Put the previous version back<\/span><small>Done<\/small>/)
    expect(rolledBack).not.toContain('update-headline')
    const updated = view({ ...ready, job: ended('deployed', 'start') })
    expect(updated).toContain('update-bowl is-rest is-gold')
    expect(updated).toContain('<strong>Updated to 0.2.0 (2222222).</strong> Was 0.1.0 (1111111).')
    expect(updated).not.toContain('is-unwound')
  })
})
