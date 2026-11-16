import { describe, expect, test } from 'bun:test'
import { renderToStaticMarkup } from 'react-dom/server'
import type { AdminSettingsDTO, CalendarDTO, CalendarEventDTO } from '@meeting-notes/contracts'
import { AdminPortal } from '../components/admin/admin-portal'
import { createAdminClient, AdminClientError } from './admin-client'
import { calendarDraft, calendarPatch, calendarProblem, eventWindow, formatEventTime, safeMeetingLink, settingsDraft, settingsPatch, settingsProblem } from './admin-state'

const settings: AdminSettingsDTO = {
  instanceLogo: null, providers: [{id:'openrouter',configured:true,source:'environment'},{id:'elevenlabs',configured:true,source:'stored'},{id:'openai',configured:false,source:'none'},{id:'anthropic',configured:false,source:'none'}],
  chatModels: [{provider:'openrouter',model:'openai/gpt-4o-mini'}], generationModel: {provider:'openrouter',model:'openai/gpt-4o-mini'}, liveTranscriptionModel: {provider:'elevenlabs',model:'scribe_v3_live'},
  instanceName: 'Cereal', languageModel: 'openai/gpt-4o-mini', openRouterConfigured: true, elevenLabsConfigured: true,
  openRouterSource: 'environment', elevenLabsSource: 'stored', calendarRefreshMinutes: 15, calendarLookAheadDays: 14, updatedAt: null,
}
const calendar: CalendarDTO = {
  id: '419d65d1-7a2c-44ca-84cc-ad489522d561', name: 'Work', provider: 'ics', color: '#be9737', enabled: true,
  sourceHost: 'calendar.example.com', lastSyncedAt: null, lastSyncError: null, eventCount: 0, createdAt: '2026-11-12T12:00:00Z',
}
function fetchStub(handler: (request: Request, init?: RequestInit) => Promise<Response> | Response): typeof fetch {
  return Object.assign(async (...[input, init]: Parameters<typeof fetch>) => handler(new Request(input, init), init), { preconnect: () => {} }) as typeof fetch
}
describe('private administration boundaries', () => {
  test('the public admin shell only checks a session and contains no private configuration', () => {
    const markup = renderToStaticMarkup(<AdminPortal />)
    expect(markup).toContain('Checking your session')
    expect(markup).not.toContain('Instance access token')
    expect(markup).not.toContain('bootstrap')
    expect(markup).not.toContain('Linked calendars')
    expect(markup).not.toContain('AI providers')
    expect(markup).not.toContain('openRouterApiKey')
    expect(markup).not.toContain('calendar.example.com')
  })
  test('cookie sessions stay same-origin with no bootstrap token and wire data is decoded', async () => {
    let received: Request | undefined
    let policy: RequestInit | undefined
    const client = createAdminClient('https://instance.example', fetchStub((request, init) => {
      received = request
      policy = init
      return Response.json({ ...settings, openRouterApiKey: 'server-must-not-send-this' })
    }))
    const value = await client.getSettings()
    expect(received!.url).toBe('https://instance.example/api/v1/admin/settings')
    expect(received!.url).not.toContain('token')
    expect(received!.headers.get('authorization')).toBeNull()
    // Bun 1.3 does not implement these Request getters faithfully; assert the
    // actual fetch options, which the browser uses for credential/privacy policy.
    expect(policy!.credentials).toBe('same-origin')
    expect(policy!.cache).toBe('no-store')
    expect(policy!.redirect).toBe('error')
    expect(policy!.referrerPolicy).toBe('no-referrer')
    expect(value).toEqual(settings)
    expect(Object.keys(value)).not.toContain('openRouterApiKey')
  })
  test('partial settings patches omit untouched secrets and error text cannot reflect upstream credentials', async () => {
    let body: unknown
    const client = createAdminClient('https://instance.example', fetchStub(async request => {
      body = await request.json()
      return Response.json({ ...settings, languageModel: 'new/model' })
    }))
    await client.updateSettings(settingsPatch({ ...settingsDraft(settings), languageModel: 'new/model' }, settings))
    expect(body).toEqual({ languageModel: 'new/model' })
    for (const status of [400, 401, 403, 409, 503]) {
      const errorKind = status === 400 ? 'invalid_request' : status === 401 ? 'unauthorized' : status === 403 ? 'forbidden' : status === 409 ? 'conflict' : 'unavailable'
      const failing = createAdminClient('https://instance.example', fetchStub(() => Response.json({ error: errorKind, message: 'https://feed.example/?secret=DO-NOT-REFLECT private-owner-token' }, { status })))
      const error = await failing.getSettings().catch(error => error)
      expect(error).toBeInstanceOf(AdminClientError)
      expect(error.status).toBe(status)
      expect(error.message).not.toContain('DO-NOT-REFLECT')
      expect(error.message).not.toContain('private-owner-token')
    }
  })
  test('malformed successful responses are rejected rather than trusted as TypeScript types', async () => {
    const client = createAdminClient('https://instance.example', fetchStub(() => Response.json({ ...settings, calendarLookAheadDays: 'two weeks' })))
    await expect(client.getSettings()).rejects.toBeInstanceOf(AdminClientError)
  })
  test('logout/unmount cancellation can abort the actual in-flight owner request', async () => {
    const controller = new AbortController()
    let reached!: () => void
    const started = new Promise<void>(resolve => { reached = resolve })
    let aborted = false
    const client = createAdminClient('https://instance.example', fetchStub(request => new Promise((_resolve, reject) => {
      request.signal.addEventListener('abort', () => { aborted = true; reject(new DOMException('Canceled', 'AbortError')) }, { once: true })
      reached()
    })))
    const response = client.getSettings(controller.signal).catch(() => null)
    await started; controller.abort(); await response
    expect(aborted).toBe(true)
  })
})
describe('admin edits and calendar presentation', () => {
  test('blank secrets preserve overrides; explicit removal is null, and no stale settings are resent', () => {
    const draft = settingsDraft(settings)
    expect(settingsPatch(draft, settings)).toEqual({})
    expect(settingsPatch({ ...draft, removeElevenLabsKey: true }, settings)).toEqual({ elevenLabsApiKey: null })
    expect(settingsPatch({ ...draft, openRouterApiKey: '  fixture-new-provider-key  ' }, settings)).toEqual({ openRouterApiKey: 'fixture-new-provider-key' })
    expect(settingsProblem({ ...draft, calendarRefreshMinutes: '4' })).toContain('5 and 1440')
    expect(settingsProblem({ ...draft, languageModel: 'invalid model spaces' })).toContain('model identifier')
    expect(settingsProblem({ ...draft, elevenLabsApiKey: 'lineone\nlinetwo' })).toContain('single value')
  })
  test('editing metadata never resends a masked or missing private subscription URL', () => {
    const draft = calendarDraft(calendar)
    expect(calendarPatch(draft, calendar)).toEqual({})
    expect(calendarPatch({ ...draft, name: 'Personal', enabled: false }, calendar)).toEqual({ name: 'Personal', enabled: false })
    expect(calendarProblem(draft, true)).toBeNull()
    expect(calendarProblem({ ...draft, url: 'webcal://calendar.example.com/private-feed.ics' }, false)).toBeNull()
    expect(calendarProblem({ ...draft, url: 'https://name:secret@calendar.example.com/private.ics' }, false)).not.toBeNull()
    expect(calendarProblem({ ...draft, url: 'file:///private/feed.ics' }, false)).not.toBeNull()
  })
  test('calendar windows follow local midnights across daylight changes and all-day ends remain exclusive', () => {
    const range = eventWindow('2026-03-08', 1)
    expect(new Date(range.from).getHours()).toBe(0)
    expect(new Date(range.to).getHours()).toBe(0)
    expect(new Date(range.to).getDate()).toBe(9)
    expect(() => eventWindow('invalid', 7)).toThrow()
    expect(() => eventWindow('2026-02-30', 7)).toThrow()
    const event = { allDay: true, startDate: '2026-11-12', endDate: '2026-11-14' } as CalendarEventDTO
    expect(formatEventTime(event, 'en-US')).toBe('All day · through Nov 13')
    expect(formatEventTime({ ...event, endDate: '2026-11-13' }, 'en-US')).toBe('All day')
  })
  test('untrusted calendar meeting links never become executable or credential-bearing links', () => {
    expect(safeMeetingLink('https://meet.example.com/room?code=one')).toBe('https://meet.example.com/room?code=one')
    for (const link of ['javascript:alert(1)', 'data:text/html,<script>', 'http://example.com', 'https://user:secret@example.com', 'https:\n//example.com']) expect(safeMeetingLink(link)).toBeUndefined()
  })
})
