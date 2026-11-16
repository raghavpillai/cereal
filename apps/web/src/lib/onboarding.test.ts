import { describe, expect, test } from 'bun:test'
import type { AdminSettingsDTO, ModelCatalogDTO } from '@meeting-notes/contracts'
import { compatiblePatch, LANGUAGE_PROVIDERS, PROVIDER_HINTS, googleClientProblem, googleDestination, googleFailure, googleRedirectAddresses, proveWithGoogle, startGoogleClaim, startGoogleSwitch, claimFailure, claimServer, initialLanguageProvider, initialModels, modelLabel, modelsPatch, needsOnboarding, ownerProblem, plausibleKey, resumeStep, serverAddress, serverHost, setupCode, setupLanding, stepsFor, suggestLanguageModel } from './onboarding'

const code = 'a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90'
const steps = { owner: true, name: false, language: false, speech: false, models: false, signIn: false, storage: false }
const providers = (...on: string[]): AdminSettingsDTO['providers'] => (['openrouter', 'openai', 'anthropic', 'elevenlabs'] as const).map(id => ({ id, configured: on.includes(id), source: on.includes(id) ? 'stored' as const : 'none' as const }))
const settings = (over: Partial<AdminSettingsDTO> = {}): AdminSettingsDTO => ({
  instanceName: 'Cereal', languageModel: 'openai/gpt-6-sol', instanceLogo: null, providers: providers(),
  chatModels: [{ provider: 'openrouter', model: 'openai/gpt-6-sol' }], generationModel: { provider: 'openrouter', model: 'openai/gpt-6-sol' },
  liveTranscriptionModel: { provider: 'elevenlabs', model: 'scribe_v3_live' },
  openRouterConfigured: false, elevenLabsConfigured: false, openRouterSource: 'none', elevenLabsSource: 'none', calendarRefreshMinutes: 15, calendarLookAheadDays: 30,
  onboarding: { completedAt: null, steps }, updatedAt: null, ...over,
})
const catalog = (provider: ModelCatalogDTO['provider'], ...ids: string[]): ModelCatalogDTO => ({ provider, fetchedAt: '2026-11-15T00:00:00.000Z', notice: null, models: ids.map(id => ({ id, name: id.toUpperCase(), capabilities: ['language'] as const })) })
const transport = (handler: (request: Request, init: RequestInit) => Response | Promise<Response>): typeof fetch =>
  Object.assign(async (...[input, init]: Parameters<typeof fetch>) => handler(new Request(new URL(String(input), 'https://notes.example.test'), init), init ?? {}), { preconnect: () => {} }) as typeof fetch

describe('setup flow helpers', () => {
  test('the claim step is walked only on a server nobody has claimed', () => {
    // "Who may join" is a step only on a server that signs in with Google.
    expect(stepsFor(true, true)).toEqual(['claim', 'name', 'language', 'speech', 'models', 'signIn', 'storage', 'apps'])
    expect(stepsFor(true)).toEqual(['claim', 'name', 'language', 'speech', 'models', 'storage', 'apps'])
    expect(stepsFor(false, true)).toEqual(['name', 'language', 'speech', 'models', 'signIn', 'storage', 'apps'])
    expect(stepsFor(false)).toEqual(['name', 'language', 'speech', 'models', 'storage', 'apps'])
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

  test('claiming with Google sends the setup code once as the bearer, and only the client in the body', async () => {
    let seen: { url: string; authorization: string | null; credentials: string; body: string } | null = null
    const started = await startGoogleClaim(code, { clientId: ' 1234567890-harbour.apps.googleusercontent.com ', clientSecret: ' GOCSPX-secret-value ' }, undefined, transport(async (request, init) => {
      seen = { url: request.url, authorization: request.headers.get('authorization'), credentials: init.credentials ?? '', body: await request.text() }
      return Response.json({ authorizationUrl: 'https://accounts.google.com/o/oauth2/v2/auth?client_id=x&state=y' })
    }))
    expect(started).toEqual({ ok: true, url: 'https://accounts.google.com/o/oauth2/v2/auth?client_id=x&state=y' })
    expect(seen!.url).toBe('https://notes.example.test/api/v1/admin/owner/setup/google')
    expect(seen!.authorization).toBe(`Bearer ${code}`)
    // The answer sets the HttpOnly cookie that binds the sign-in to this browser, so the browser has to be allowed to keep it.
    expect(seen!.credentials).toBe('same-origin')
    expect(JSON.parse(seen!.body)).toEqual({ clientId: '1234567890-harbour.apps.googleusercontent.com', clientSecret: 'GOCSPX-secret-value' })
    expect(seen!.url + seen!.body).not.toContain(code)
    // Switching, from the panel, carries no setup code at all: only this origin's session cookie.
    let switching: { authorization: string | null; credentials: string } | null = null
    await startGoogleSwitch({ clientId: '1234567890-harbour.apps.googleusercontent.com', clientSecret: 'GOCSPX-secret-value' }, undefined, transport((request, init) => { switching = { authorization: request.headers.get('authorization'), credentials: init.credentials ?? '' }; return Response.json({ authorizationUrl: 'https://accounts.google.com/x' }) }))
    expect(switching!).toEqual({ authorization: null, credentials: 'same-origin' })
  })

  test('the browser is only ever sent to a web address, and each refusal says what to look at', async () => {
    const answer = (status: number, body: unknown, headers: Record<string, string> = {}) => startGoogleClaim(code, { clientId: '1234567890-harbour.apps.googleusercontent.com', clientSecret: 'GOCSPX-secret-value' }, undefined, transport(() => Response.json(body, { status, headers })))
    for (const hostile of ['javascript:alert(1)', 'cereal://auth', 'data:text/html,x', '', 42, 'https://user:pw@accounts.google.com/']) expect((await answer(200, { authorizationUrl: hostile })).ok).toBe(false)
    expect(googleDestination('https://accounts.google.com/o/oauth2/v2/auth?x=1')).toBe('https://accounts.google.com/o/oauth2/v2/auth?x=1')
    expect(await answer(401, { error: 'unauthorized' })).toMatchObject({ ok: false, failure: { field: 'code' } })
    expect(await answer(409, { error: 'conflict' })).toMatchObject({ ok: false, failure: { field: 'email' } })
    expect(await answer(429, { error: 'rate_limited' }, { 'Retry-After': '30' })).toMatchObject({ ok: false, failure: { field: null, waitSeconds: 30 } })
    expect(await answer(400, { message: 'Enter the Google client secret exactly as Google Cloud shows it' })).toMatchObject({ ok: false, failure: { field: 'clientSecret' } })
    expect(await answer(400, { message: 'Enter the Google client ID exactly as Google Cloud shows it' })).toMatchObject({ ok: false, failure: { field: 'clientId' } })
    expect(await answer(400, { message: 'Google sign-in needs an HTTPS address. This server is reached over plain HTTP.' })).toMatchObject({ ok: false, failure: { field: null, message: 'Google sign-in needs an HTTPS address. This server is reached over plain HTTP.' } })
  })

  test('a Google client is checked before anything is sent, and every way the proof can fail has its own sentence', () => {
    expect(googleClientProblem({ clientId: '', clientSecret: 'x' })?.field).toBe('clientId')
    expect(googleClientProblem({ clientId: 'not a client id', clientSecret: 'GOCSPX-secret-value' })?.field).toBe('clientId')
    expect(googleClientProblem({ clientId: '1234567890-harbour.apps.googleusercontent.com', clientSecret: '' })?.field).toBe('clientSecret')
    expect(googleClientProblem({ clientId: '1234567890-harbour.apps.googleusercontent.com', clientSecret: 'has a space' })?.field).toBe('clientSecret')
    expect(googleClientProblem({ clientId: ' 1234567890-harbour.apps.googleusercontent.com ', clientSecret: ' GOCSPX-secret-value ' })).toBeNull()
    const reasons = ['invalid_client', 'redirect_uri_mismatch', 'cancelled', 'unverified_email', 'email_taken', 'google_account_in_use', 'already_claimed', 'expired', 'blocked', 'failed']
    expect(new Set(reasons.map(reason => googleFailure(reason).message)).size).toBe(reasons.length)
    // The two the owner asked about say exactly what to fix.
    expect(googleFailure('invalid_client')).toMatchObject({ field: 'clientSecret' })
    expect(googleFailure('invalid_client').message).toContain('client secret')
    expect(googleFailure('redirect_uri_mismatch').message).toContain('Authorized redirect URIs')
    for (const reason of reasons.slice(0, 3)) expect(googleFailure(reason).message).toMatch(/[Nn]othing was saved|try again/)
    expect(googleFailure('something Google made up <script>').message).toBe(googleFailure(null).message)
    expect(googleRedirectAddresses('https://notes.example.test/')).toEqual(['https://notes.example.test/api/v1/auth/google/callback', 'https://notes.example.test/api/integrations/google/callback'])
  })

  test('the proof runs in its own window and the page waits for the server to say how it went', async () => {
    const window_ = () => { const log: string[] = []; return { log, popup: { location: { replace: (url: string) => log.push(`go:${url}`) }, close: () => log.push('close') } as unknown as Window } }
    const start = async () => ({ ok: true as const, url: 'https://accounts.google.com/auth' })
    const statuses = (...states: Array<[string, string | null]>) => { let at = 0; return async () => { const [state, reason] = states[Math.min(at++, states.length - 1)]!; return { state, reason } as { state: 'pending' | 'done' | 'failed' | 'none'; reason: string | null } } }
    const wait = async () => {}
    // Opened first, sent to Google, closed when the server says it is done.
    const good = window_()
    expect(await proveWithGoogle(start, new AbortController().signal, { open: () => good.popup, status: statuses(['pending', null], ['pending', null], ['done', null]), wait })).toEqual({ ok: true })
    expect(good.log).toEqual(['go:https://accounts.google.com/auth', 'close'])
    // A failure comes back as its sentence.
    const bad = window_()
    expect(await proveWithGoogle(start, new AbortController().signal, { open: () => bad.popup, status: statuses(['pending', null], ['failed', 'redirect_uri_mismatch']), wait })).toMatchObject({ ok: false, failure: { message: googleFailure('redirect_uri_mismatch').message } })
    expect(bad.log.at(-1)).toBe('close')
    // A refused start closes the window and never polls.
    const refused = window_(); let asked = 0
    expect(await proveWithGoogle(async () => ({ ok: false as const, failure: { field: 'code' as const, message: 'no', waitSeconds: 0 } }), new AbortController().signal, { open: () => refused.popup, status: async () => { asked++; return { state: 'done', reason: null } }, wait })).toMatchObject({ ok: false, failure: { field: 'code' } })
    expect(refused.log).toEqual(['close']); expect(asked).toBe(0)
    // Calling it off closes the window.
    const cancelled = window_(), controller = new AbortController()
    await expect(proveWithGoogle(start, controller.signal, { open: () => cancelled.popup, status: async () => { controller.abort(); return { state: 'pending', reason: null } }, wait })).rejects.toThrow()
    expect(cancelled.log.at(-1)).toBe('close')
    // A browser that will not open a window: the whole page goes to Google instead.
    let left = ''
    void proveWithGoogle(start, new AbortController().signal, { open: () => null, status: statuses(['done', null]), wait, leave: url => { left = url } })
    await new Promise(resolve => setTimeout(resolve, 10))
    expect(left).toBe('https://accounts.google.com/auth')
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
    expect(suggestLanguageModel('openai', { ...catalog('openai'), models: [{ id: 'scribe_v3_live', name: 'T', capabilities: ['live_transcription'] }] }, 'openai/gpt-6-sol')).toBeNull()
  })

  test('the Models step keeps choices that work and replaces ones whose provider has no key', () => {
    // OpenRouter connected: the defaults already work.
    const routed = settings({ providers: providers('openrouter', 'elevenlabs') })
    expect(initialModels(routed, {})).toEqual({ chat: routed.chatModels[0]!, notes: routed.generationModel, live: routed.liveTranscriptionModel! })
    // Only Anthropic connected: the OpenRouter defaults cannot run, so both language jobs move to Claude.
    const claude = settings({ providers: providers('anthropic', 'elevenlabs') })
    const draft = initialModels(claude, { anthropic: catalog('anthropic', 'claude-haiku-5', 'claude-sonnet-5') })
    expect(draft.chat).toEqual({ provider: 'anthropic', model: 'claude-sonnet-5' })
    expect(draft.notes).toEqual({ provider: 'anthropic', model: 'claude-sonnet-5' })
    // Continue stores all three (never a batch speech model, never live off), keeps other enabled chat models whose
    // provider still has a key, and drops repeats.
    const several = settings({ providers: providers('openrouter', 'anthropic'), chatModels: [{ provider: 'openrouter', model: 'openai/gpt-6-sol' }, { provider: 'anthropic', model: 'claude-sonnet-5' }, { provider: 'openai', model: 'gpt-6-sol' }] })
    expect(modelsPatch({ ...initialModels(several, {}), chat: { provider: 'anthropic', model: 'claude-sonnet-5' }, live: { provider: 'elevenlabs', model: 'scribe_v2_realtime' } }, several)).toEqual({
      chatModels: [{ provider: 'anthropic', model: 'claude-sonnet-5' }], generationModel: several.generationModel, liveTranscriptionModel: { provider: 'elevenlabs', model: 'scribe_v2_realtime' },
    })
    expect(modelsPatch(initialModels(several, {}), several).chatModels).toEqual([{ provider: 'openrouter', model: 'openai/gpt-6-sol' }, { provider: 'anthropic', model: 'claude-sonnet-5' }])
  })

  test('an OpenAI-compatible server is the fourth choice; Continue saves its address, its model, and a key only when typed', () => {
    expect(LANGUAGE_PROVIDERS).toEqual(['openrouter', 'openai', 'anthropic', 'openai_compatible'])
    expect(PROVIDER_HINTS.openai_compatible.blurb).toBe('Ollama, vLLM, LM Studio, or any /v1 API')
    const before = settings({ providers: providers('anthropic'), chatModels: [{ provider: 'anthropic', model: 'claude-sonnet-5' }, { provider: 'openai', model: 'gpt-6-sol' }] })
    expect(compatiblePatch({ baseUrl: ' http://10.0.0.5:11434/v1 ', key: '', model: ' llama3.2:3b ' }, before)).toEqual({
      openAiCompatibleBaseUrl: 'http://10.0.0.5:11434/v1',
      chatModels: [{ provider: 'openai_compatible', model: 'llama3.2:3b' }, { provider: 'anthropic', model: 'claude-sonnet-5' }],
      generationModel: { provider: 'openai_compatible', model: 'llama3.2:3b' },
    })
    expect(compatiblePatch({ baseUrl: 'https://api.together.xyz/v1', key: ' tok-123 ', model: 'meta/llama' }, before)).toMatchObject({ openAiCompatibleApiKey: 'tok-123' })
    // A server whose chosen chat model is on that server opens on it.
    expect(initialLanguageProvider(settings({ providers: [...providers('anthropic'), { id: 'openai_compatible', configured: true, source: 'stored' }], chatModels: [{ provider: 'openai_compatible', model: 'llama3.2:3b' }] }))).toBe('openai_compatible')
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
