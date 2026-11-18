import { describe, expect, test } from 'bun:test'
import { renderToStaticMarkup } from 'react-dom/server'
import { UpdatesView } from '../components/admin/updates-panel'
import { sectionFromHash, sectionOfSetting } from '../components/admin/settings-panel'
import { availabilityLine, updateOffered, updateOutcome, updateSteps, versionLabel, type UpdateJob, type UpdateStatus } from './updates'

const A = '1111111111111111111111111111111111111111', B = '2222222222222222222222222222222222222222'
const ready: UpdateStatus = { current: { version: '0.1.0', commit: A }, updater: { state: 'ready', source: 'git@github.com:team/cereal.git', ref: 'main' }, available: { checkedAt: '2026-11-18T10:00:00.000Z', version: '0.2.0', commit: B, relation: 'newer', error: null }, job: null }
const job = (patch: Partial<UpdateJob>): UpdateJob => ({ id: '20261118T101500Z-2222222', status: 'running', step: 'build', active: true, startedAt: '2026-11-18T10:15:00.000Z', finishedAt: null, from: { version: '0.1.0', commit: A }, to: { version: '0.2.0', commit: B }, error: null, ...patch })
const view = (status: UpdateStatus, extra: Partial<Parameters<typeof UpdatesView>[0]> = {}) => renderToStaticMarkup(<UpdatesView status={status} lines={[]} link="live" check={() => {}} update={() => {}} {...extra} />)
const states = (value: UpdateJob) => updateSteps(value).map(step => `${step.id}:${step.state}`).join(' ')

describe('the section named in the address', () => {
  const ids = ['identity', 'providers', 'models', 'updates']
  test('a known name opens that section; anything else opens the first', () => {
    expect(sectionFromHash('#models', ids)).toBe('models')
    expect(sectionFromHash('#updates', ids)).toBe('updates')
    for (const other of ['', '#', '#nowhere', '#%E0%A4%A', '#Models']) expect(sectionFromHash(other, ids)).toBe('identity')
    // Administrators and Updates are the owner's: for anyone else the name is not a section.
    expect(sectionFromHash('#updates', ['identity', 'providers'])).toBe('identity')
  })
  test('an unsaved setting marks the section it is shown in', () => {
    expect(['instanceName', 'instanceLogo', 'openAiApiKey', 'openAiCompatibleBaseUrl', 'chatModels', 'liveMaxSessions', 'signIn', 'storage', 'apps'].map(sectionOfSetting))
      .toEqual(['identity', 'identity', 'providers', 'providers', 'models', 'models', 'sign-in', 'storage', 'apps'])
  })
})

describe('what an update offers', () => {
  test('only a commit that is not the running one, and not behind it, can be installed', () => {
    expect(updateOffered(ready)).toBe(true)
    for (const relation of ['different', 'unknown'] as const) expect(updateOffered({ ...ready, available: { ...ready.available!, relation } })).toBe(true)
    for (const relation of ['same', 'older', null] as const) expect(updateOffered({ ...ready, available: { ...ready.available!, relation } })).toBe(false)
    expect(updateOffered({ ...ready, available: { ...ready.available!, error: 'Could not fetch main' } })).toBe(false)
    expect(updateOffered({ ...ready, available: null })).toBe(false)
    expect(updateOffered({ ...ready, updater: { ...ready.updater, state: 'stopped' } })).toBe(false)
    expect(availabilityLine(ready)).toBe('Version 0.2.0 (2222222) is available.')
    expect(availabilityLine({ ...ready, available: { ...ready.available!, relation: 'same' } })).toBe('This server is up to date.')
    expect(availabilityLine({ ...ready, available: null })).toBe('Not checked yet.')
    expect(availabilityLine({ ...ready, available: { ...ready.available!, error: 'Could not fetch main from x' } })).toBe('Could not fetch main from x')
    // Following releases, it says so, and never offers what is behind the running version.
    const releases: UpdateStatus = { ...ready, updater: { ...ready.updater, ref: 'releases' }, available: { ...ready.available!, release: 'v0.2.0' } }
    expect(availabilityLine(releases)).toBe('Version 0.2.0 (2222222) is available.')
    expect(availabilityLine({ ...releases, available: { ...releases.available!, relation: 'same' } })).toBe('This server runs the latest release.')
    expect(availabilityLine({ ...releases, available: { ...releases.available!, relation: 'older' } })).toBe('The latest release is 0.2.0 (2222222), and this server runs something later. There is nothing to install.')
    expect(updateOffered({ ...releases, available: { ...releases.available!, relation: 'older' } })).toBe(false)
    expect(versionLabel(null, null)).toBe('an unknown version')
  })
})

describe('the steps of an update', () => {
  test('everything before the step under way is done, everything after waits', () => {
    expect(states(job({ step: 'fetch' }))).toBe('fetch:running preflight:waiting backup:waiting build:waiting start:waiting')
    expect(states(job({ step: 'build' }))).toBe('fetch:done preflight:done backup:done build:running start:waiting')
    expect(states(job({ step: 'start' }))).toBe('fetch:done preflight:done backup:done build:done start:running')
  })
  test('an update that ended shows where', () => {
    expect(states(job({ status: 'deployed', step: 'start', active: false }))).toBe('fetch:done preflight:done backup:done build:done start:done')
    expect(states(job({ status: 'failed', step: 'build', active: false }))).toBe('fetch:done preflight:done backup:done build:failed start:waiting')
    expect(states(job({ status: 'up-to-date', step: 'fetch', active: false }))).toBe('fetch:done preflight:skipped backup:skipped build:skipped start:skipped')
  })
  test('a rollback is a sixth step, only when it happened', () => {
    expect(states(job({ step: 'rollback' }))).toBe('fetch:done preflight:done backup:done build:done start:failed rollback:running')
    expect(states(job({ status: 'rolled-back', step: 'rollback', active: false }))).toBe('fetch:done preflight:done backup:done build:done start:failed rollback:done')
    expect(states(job({ status: 'rollback-failed', step: 'rollback', active: false }))).toBe('fetch:done preflight:done backup:done build:done start:failed rollback:failed')
    expect(updateOutcome(job({ status: 'rolled-back', step: 'rollback', active: false, error: 'The new version did not start.' }))).toEqual({ tone: 'bad', title: 'The update to 0.2.0 (2222222) failed and was rolled back.', detail: '0.1.0 (1111111) is running again.' })
    expect(updateOutcome(job({ status: 'deployed', active: false }))).toMatchObject({ tone: 'good', title: 'Updated to 0.2.0 (2222222).' })
  })
})

describe('the Updates section', () => {
  test('with no updater it says so, says how to install one, and offers nothing to press', () => {
    const markup = view({ current: { version: '0.1.0', commit: null }, updater: { state: 'missing', source: null, ref: null }, available: null, job: null })
    expect(markup).toContain('Cereal 0.1.0')
    expect(markup).toContain('The updater is not installed on this server')
    expect(markup).toContain('./scripts/updater.sh install')
    expect(markup).toContain('scripts/deploy-ssh.sh')
    expect(markup).not.toContain('<button')
    expect(markup).not.toContain('up to date')
  })
  test('a stopped updater is named, and its buttons cannot be pressed', () => {
    const markup = view({ ...ready, updater: { ...ready.updater, state: 'stopped' } })
    expect(markup).toContain('installed but is not running')
    expect(markup).toContain('systemctl start cereal-updater')
    expect(markup).not.toContain('Update server')
    expect(markup).toMatch(/<button[^>]*disabled=""[^>]*>.*Check for updates/)
  })
  test('a newer version offers the update and says when it was looked for', () => {
    const markup = view(ready)
    expect(markup).toContain('Version 0.2.0 (2222222) is available.')
    expect(markup).toContain('Update server')
    // When it looked, and nothing about where: the owner set that up on the server and does not need it read back.
    expect(markup).toContain('Checked ')
    expect(markup).not.toContain('git@github.com:team/cereal.git')
    expect(view({ ...ready, available: { ...ready.available!, relation: 'same' } })).not.toContain('Update server')
  })
  test('while an update runs nothing else can be started, and a lost connection during the restart is expected', () => {
    const markup = view({ ...ready, job: job({ step: 'start' }) }, { lines: ['10:15:00 Starting the new version…'], link: 'reconnecting' })
    expect(markup).toContain('Update in progress')
    expect(markup).toContain('The server is restarting. This page reconnects by itself…')
    expect(markup).toContain('10:15:00 Starting the new version…')
    expect(markup.match(/<button[^>]*disabled=""/g)?.length).toBe(2)
  })
  test('the last update is shown with how it ended', () => {
    const markup = view({ ...ready, job: job({ status: 'rolled-back', step: 'rollback', active: false, finishedAt: '2026-11-18T10:20:00.000Z', error: 'The new version did not start. The previous version is running again.' }) })
    expect(markup).toContain('Last update')
    expect(markup).toContain('failed and was rolled back')
    expect(markup).toContain('Put the previous version back')
    expect(view({ ...ready, job: job({ status: 'deployed', step: 'start', active: false }) }, { reloadOffered: true })).toContain('Reload it')
  })
})
