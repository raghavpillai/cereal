import { describe, expect, test } from 'bun:test'
import type { AdminSettingsDTO, ModelCatalogDTO } from '@meeting-notes/contracts'
import { claimFailure, claimServer, initialLanguageProvider, initialModels, modelLabel, modelsPatch, needsOnboarding, ownerProblem, plausibleKey, resumeStep, serverAddress, serverHost, setupCode, setupLanding, stepsFor, suggestLanguageModel } from './onboarding'

const code = 'a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90'
const steps = { owner: true, name: false, language: false, speech: false, models: false, signIn: false, storage: false }
const providers = (...on: string[]): AdminSettingsDTO['providers'] => (['openrouter', 'openai', 'anthropic', 'elevenlabs'] as const).map(id => ({ id, configured: on.includes(id), source: on.includes(id) ? 'stored' as const : 'none' as const }))
const settings = (over: Partial<AdminSettingsDTO> = {}): AdminSettingsDTO => ({
  instanceName: 'Cereal', languageModel: 'openai/gpt-6-sol', instanceLogo: null, providers: providers(),
  chatModels: [{ provider: 'openrouter', model: 'openai/gpt-6-sol' }], generationModel: { provider: 'openrouter', model: 'openai/gpt-6-sol' },
  transcriptionModel: { provider: 'elevenlabs', model: 'scribe_v3' }, liveTranscriptionModel: { provider: 'elevenlabs', model: 'scribe_v3_live' },
  openRouterConfigured: false, elevenLabsConfigured: false, openRouterSource: 'none', elevenLabsSource: 'none', calendarRefreshMinutes: 15, calendarLookAheadDays: 30,
  onboarding: { completedAt: null, steps }, updatedAt: null, ...over,
})
const catalog = (provider: ModelCatalogDTO['provider'], ...ids: string[]): ModelCatalogDTO => ({ provider, fetchedAt: '2026-11-15T00:00:00.000Z', notice: null, models: ids.map(id => ({ id, name: id.toUpperCase(), capabilities: ['language'] as const })) })
const transport = (handler: (request: Request, init: RequestInit) => Response | Promise<Response>): typeof fetch =>
  Object.assign(async (...[input, init]: Parameters<typeof fetch>) => handler(new Request(new URL(String(input), 'https://notes.example.test'), init), init ?? {}), { preconnect: () => {} }) as typeof fetch

describe('setup flow helpers', () => {
  test('the claim step is walked only on a server nobody has claimed', () => {
    expect(stepsFor(true)).toEqual(['claim', 'name', 'language', 'speech', 'models', 'signIn', 'storage', 'apps'])
    expect(stepsFor(false)).toEqual(['name', 'language', 'speech', 'models', 'signIn', 'storage', 'apps'])
  })

  test('a reload resumes at the first step the server has not finished', () => {
    expect(resumeStep(steps)).toBe('name')
    expect(resumeStep({ ...steps, name: true })).toBe('language')
    expect(resumeStep({ ...steps, name: true, language: true, speech: true })).toBe('models')
    // A later step finished out of order does not skip an earlier one.
    expect(resumeStep({ ...steps, name: true, language: true, speech: false, models: true, signIn: true })).toBe('speech')
    expect(resumeStep({ ...steps, name: true, language: true, speech: true, models: true, signIn: true })).toBe('storage')
    expect(resumeStep({ owner: true, name: true, language: true, speech: true, models: true, signIn: true, storage: true })).toBe('apps')
  })

  test('setup replaces the panel only while the server reports it unfinished', () => {
    expect(needsOnboarding(settings())).toBe(true)
    expect(needsOnboarding(settings({ onboarding: { completedAt: '2026-11-15T10:00:00.000Z', steps } }))).toBe(false)
    // A server from before setup existed reports nothing: its panel is shown as it always was.
    expect(needsOnboarding({})).toBe(false)
  })

  test('the setup code is read from whatever was pasted: the bare code, the whole .env line, quotes and spaces', () => {
    for (const pasted of [code, `  ${code}\n`, `ADMIN_API_TOKEN=${code}`, `admin_api_token = "${code}"`, `export ADMIN_API_TOKEN='${code}'`]) expect(setupCode(pasted)).toBe(code)
    for (const wrong of ['', 'short', 'ADMIN_API_TOKEN=', 'two words that are long enough to pass the length check', 'x'.repeat(513)]) expect(setupCode(wrong)).toBeNull()
  })

  test('the owner account is checked in field order, with the password rule the server enforces', () => {
    expect(ownerProblem({ name: ' ', email: 'ada@example.test', password: 'long-enough-password' })?.field).toBe('name')
    expect(ownerProblem({ name: 'Ada', email: 'ada@example', password: 'long-enough-password' })?.field).toBe('email')
    expect(ownerProblem({ name: 'Ada', email: 'ada@example.test', password: 'elevenchars' })).toEqual({ field: 'password', message: 'Use at least 12 characters: 1 more to go.' })
    expect(ownerProblem({ name: 'Ada', email: 'ada@example.test', password: 'x'.repeat(129) })?.field).toBe('password')
    expect(ownerProblem({ name: 'Ada', email: ' ada@example.test ', password: 'twelve-chars' })).toBeNull()
  })

  test('claiming sends the code once as a bearer credential, without cookies, and never in the address or the body', async () => {
    let seen: { url: string; authorization: string | null; credentials: string; body: string } | null = null
    const result = await claimServer(code, { name: ' Ada Lovelace ', email: ' Ada@Example.test ', password: 'a-long-enough-password' }, undefined, transport(async (request, init) => {
      seen = { url: request.url, authorization: request.headers.get('authorization'), credentials: init.credentials ?? '', body: await request.text() }
      return Response.json({ id: 'owner' })
    }))
    expect(result).toEqual({ ok: true })
    expect(seen!.url).toBe('https://notes.example.test/api/v1/admin/owner/setup')
    expect(seen!.authorization).toBe(`Bearer ${code}`)
    expect(seen!.credentials).toBe('omit')
    expect(JSON.parse(seen!.body)).toEqual({ name: 'Ada Lovelace', email: 'ada@example.test', password: 'a-long-enough-password' })
    expect(seen!.url + seen!.body).not.toContain(code)
  })

  test('a refused claim says which field to look at, and a rate limit says how long to wait', async () => {
    const answer = (status: number, headers: Record<string, string> = {}) => claimServer(code, { name: 'Ada', email: 'ada@example.test', password: 'a-long-enough-password' }, undefined, transport(() => Response.json({ error: 'x' }, { status, headers })))
    expect(await answer(401)).toEqual({ ok: false, failure: { field: 'code', message: 'That code did not match this server. Copy it again from the machine Cereal runs on.', waitSeconds: 0 } })
    expect((await answer(409) as { failure: { field: string } }).failure.field).toBe('email')
    expect(await answer(429, { 'Retry-After': '42' })).toEqual({ ok: false, failure: { field: null, message: 'Too many tries in a row. Give it a minute, then try again.', waitSeconds: 42 } })
    expect(claimFailure(429, null).waitSeconds).toBe(60)
    expect(claimFailure(429, '86400').waitSeconds).toBe(600)
    expect(claimFailure(503).field).toBeNull()
    const offline = await claimServer(code, { name: 'Ada', email: 'ada@example.test', password: 'a-long-enough-password' }, undefined, transport(() => { throw new TypeError('offline') }))
    expect(offline.ok).toBe(false)
    // No message repeats the code.
    for (const status of [400, 401, 403, 409, 429, 500]) expect(claimFailure(status).message).not.toContain(code)
  })

  test('the provider step opens on one that already has a key', () => {
    expect(initialLanguageProvider(settings())).toBe('openrouter')
    expect(initialLanguageProvider(settings({ providers: providers('anthropic', 'elevenlabs') }))).toBe('anthropic')
    expect(initialLanguageProvider(settings({ providers: providers('openai', 'anthropic'), chatModels: [{ provider: 'anthropic', model: 'claude-x' }] }))).toBe('anthropic')
    expect(plausibleKey('sk-or-0123456789')).toBe(true)
    expect(plausibleKey('short')).toBe(false)
    expect(plausibleKey('line one\nline two of the key')).toBe(false)
  })

  test('a model is suggested from what the connected provider really offers', () => {
    // The server's default where the provider has it: as listed on OpenRouter, without the vendor at OpenAI.
    expect(suggestLanguageModel('openrouter', catalog('openrouter', 'anthropic/claude-x', 'openai/gpt-6-sol'), 'openai/gpt-6-sol')).toEqual({ provider: 'openrouter', model: 'openai/gpt-6-sol' })
    expect(suggestLanguageModel('openai', catalog('openai', 'gpt-5-mini', 'gpt-6-sol'), 'openai/gpt-6-sol')).toEqual({ provider: 'openai', model: 'gpt-6-sol' })
    // Otherwise the newest full-size model, not a small or dated one.
    expect(suggestLanguageModel('openai', catalog('openai', 'gpt-5-mini', 'gpt-5', 'gpt-4.1', 'gpt-5-2026-01-01', 'o3'), 'openai/gpt-6-sol')?.model).toBe('gpt-5')
    expect(suggestLanguageModel('anthropic', catalog('anthropic', 'claude-haiku-5', 'claude-sonnet-5', 'claude-opus-4'), 'openai/gpt-6-sol')?.model).toBe('claude-sonnet-5')
    expect(suggestLanguageModel('openrouter', catalog('openrouter', 'meta/llama-4', 'openai/gpt-5', 'openai/gpt-5-mini'), 'openai/gpt-6-sol')?.model).toBe('openai/gpt-5')
    expect(suggestLanguageModel('openai', undefined, 'openai/gpt-6-sol')).toBeNull()
    expect(suggestLanguageModel('openai', { ...catalog('openai'), models: [{ id: 'gpt-transcribe', name: 'T', capabilities: ['transcription'] }] }, 'openai/gpt-6-sol')).toBeNull()
  })

  test('the Models step keeps choices that work and replaces ones whose provider has no key', () => {
    // OpenRouter connected: the defaults already work.
    const routed = settings({ providers: providers('openrouter', 'elevenlabs') })
    expect(initialModels(routed, {})).toEqual({ chat: routed.chatModels[0]!, notes: routed.generationModel, speech: routed.transcriptionModel, live: routed.liveTranscriptionModel! })
    // Only Anthropic connected: the OpenRouter defaults cannot run, so both language jobs move to Claude.
    const claude = settings({ providers: providers('anthropic', 'elevenlabs') })
    const draft = initialModels(claude, { anthropic: catalog('anthropic', 'claude-haiku-5', 'claude-sonnet-5') })
    expect(draft.chat).toEqual({ provider: 'anthropic', model: 'claude-sonnet-5' })
    expect(draft.notes).toEqual({ provider: 'anthropic', model: 'claude-sonnet-5' })
    expect(draft.speech).toEqual({ provider: 'elevenlabs', model: 'scribe_v3' })
    // Live transcription that was turned off stays off.
    expect(initialModels(settings({ liveTranscriptionModel: null }), {}).live).toBeNull()
    // Continue stores all four, keeps other enabled chat models whose provider still has a key, and drops repeats.
    const several = settings({ providers: providers('openrouter', 'anthropic'), chatModels: [{ provider: 'openrouter', model: 'openai/gpt-6-sol' }, { provider: 'anthropic', model: 'claude-sonnet-5' }, { provider: 'openai', model: 'gpt-6-sol' }] })
    expect(modelsPatch({ ...initialModels(several, {}), chat: { provider: 'anthropic', model: 'claude-sonnet-5' }, live: null }, several)).toEqual({
      chatModels: [{ provider: 'anthropic', model: 'claude-sonnet-5' }], generationModel: several.generationModel, transcriptionModel: several.transcriptionModel, liveTranscriptionModel: null,
    })
    expect(modelsPatch(initialModels(several, {}), several).chatModels).toEqual([{ provider: 'openrouter', model: 'openai/gpt-6-sol' }, { provider: 'anthropic', model: 'claude-sonnet-5' }])
  })

  test('small labels: a model by its catalog name, the server by its address', () => {
    expect(modelLabel({ provider: 'openai', model: 'gpt-5' }, { openai: catalog('openai', 'gpt-5') })).toBe('GPT-5')
    expect(modelLabel({ provider: 'openrouter', model: 'openai/gpt-6-sol' }, {})).toBe('gpt-6-sol')
    expect(serverAddress('https://notes.example.test/')).toBe('https://notes.example.test')
    expect(serverHost('http://localhost:3000')).toBe('localhost:3000')
  })

  test('after signing in, only an owner or administrator of an unfinished server is sent to setup', async () => {
    const session = (role: 'owner' | 'admin' | 'user') => ({ user: { id: '00000000-0000-4000-8000-000000000001', name: 'Ada', email: 'ada@example.test', role, createdAt: '2026-11-15T00:00:00.000Z' } })
    expect(await setupLanding(session('owner'), async () => settings())).toBe('/admin')
    expect(await setupLanding(session('admin'), async () => settings())).toBe('/admin')
    expect(await setupLanding(session('owner'), async () => settings({ onboarding: { completedAt: '2026-11-15T10:00:00.000Z', steps } }))).toBeNull()
    // A member is never asked for the settings, and a failure sends nobody anywhere unusual.
    let asked = 0
    expect(await setupLanding(session('user'), async () => { asked++; return settings() })).toBeNull()
    expect(asked).toBe(0)
    expect(await setupLanding(session('owner'), async () => { throw new Error('offline') })).toBeNull()
  })
})
