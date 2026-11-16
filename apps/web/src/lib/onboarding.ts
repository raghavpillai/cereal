/** First-run setup on the admin panel: the order of its steps, where to resume, and the small decisions each
 * step makes. Pure helpers and two requests; the screens are in src/components/onboarding. */
import type { AdminSettingsDTO, AIModelDTO, AIProvider, AuthSessionDTO, LanguageModelSelection, ModelCatalogDTO, OnboardingStepsDTO } from '@meeting-notes/contracts'

/** The screens, in order. `claim` exists only on a server nobody has claimed; `apps` is the last screen and is
 * never stored: finishing it finishes setup. */
export type StepId = 'claim' | 'name' | 'language' | 'speech' | 'models' | 'signIn' | 'storage' | 'apps'
export const STEP_ORDER: readonly StepId[] = ['claim', 'name', 'language', 'speech', 'models', 'signIn', 'storage', 'apps']
export const STEP_TITLES: Record<StepId, string> = {
  claim: 'Set up sign-in', name: 'Name your space', language: 'Notes and chat', speech: 'Speech to text',
  models: 'Models', signIn: 'Who may join', storage: 'Where files live', apps: 'Get the apps',
}
/** Steps that offer "Skip for now". */
export const OPTIONAL_STEPS: readonly StepId[] = ['signIn', 'storage']

/** The steps this visit walks: with "Claim this server" first when it started on an unclaimed server. */
/** "Who may join" exists only on a server that signs in with Google: with email and password an administrator adds people. */
export const stepsFor = (claiming: boolean, google = false): StepId[] => STEP_ORDER.filter(step => (claiming || step !== 'claim') && (google || step !== 'signIn'))

/** The one way people sign in to a server: chosen in the first step, changed only from the admin panel. */
export type SignInMethod = 'password' | 'google'
/** The two addresses a Google OAuth client must list as authorized redirect URIs: sign-in first, then the calendar connection. */
export const googleRedirectAddresses = (origin: string) => [`${serverAddress(origin)}/api/v1/auth/google/callback`, `${serverAddress(origin)}/api/integrations/google/callback`]
const GOOGLE_CLIENT_ID = /^[A-Za-z0-9._-]{10,255}$/
/** The first thing wrong with a Google client as typed, before anything is sent. */
export function googleClientProblem(client: { clientId: string; clientSecret: string }): { field: 'clientId' | 'clientSecret'; message: string } | null {
  const id = client.clientId.trim(), secret = client.clientSecret.trim()
  if (!id) return { field: 'clientId', message: 'Paste the client ID of your Google OAuth client.' }
  if (!GOOGLE_CLIENT_ID.test(id)) return { field: 'clientId', message: 'That is not a client ID. It ends in .apps.googleusercontent.com.' }
  if (!secret) return { field: 'clientSecret', message: 'Paste the client secret that belongs to this client ID.' }
  if (secret.length < 8 || secret.length > 512 || /\s/.test(secret)) return { field: 'clientSecret', message: 'That is not a client secret. Copy it again from Google Cloud.' }
  return null
}
/** What to say when the proving Google sign-in failed, and which field to look at. Each reason gets its own sentence. */
export function googleFailure(reason: string | null): { field: 'clientId' | 'clientSecret' | null; message: string } {
  switch (reason) {
    case 'invalid_client': return { field: 'clientSecret', message: 'Google did not accept the client secret. Copy the secret again from the same OAuth client as the client ID; nothing was saved.' }
    case 'redirect_uri_mismatch': return { field: null, message: 'Google does not know this server’s address. Add both redirect addresses shown here to “Authorized redirect URIs” on the OAuth client, exactly as written, then try again.' }
    case 'cancelled': return { field: null, message: 'The Google sign-in was cancelled, so nothing was saved. Try again when you are ready.' }
    case 'unverified_email': return { field: null, message: 'That Google account’s email address is not verified. Sign in with a Google account whose address is verified.' }
    case 'email_taken': return { field: null, message: 'Someone on this server already has an account with that Google address. Use a different Google account for the owner.' }
    case 'google_account_in_use': return { field: null, message: 'That Google account belongs to someone else on this server. Sign in to Google as yourself to switch.' }
    case 'already_claimed': return { field: null, message: 'This server was claimed a moment ago in another browser. Reload the page to sign in.' }
    case 'expired': return { field: null, message: 'That took longer than ten minutes, so it was dropped. Nothing was saved; try again.' }
    case 'blocked': return { field: null, message: 'The sign-in window was closed before Google finished. Nothing was saved; try again.' }
    default: return { field: null, message: 'The Google sign-in did not finish. Nothing was saved. Check the client ID, the secret and the redirect addresses, then try again.' }
  }
}
/** What Google shows on its own page when it will not even start, and what each means here. */
export const GOOGLE_STUCK_HINT = 'If Google shows an error instead of its sign-in page: “redirect_uri_mismatch” means the two addresses above are not on the OAuth client yet, and “invalid_client” means the client ID is not one Google knows. Close that window, fix it, and try again.'

export type GoogleVerification = { state: 'none' | 'pending' | 'done' | 'failed'; reason: string | null }
/** How this browser's proving sign-in went. Sent with the HttpOnly cookie that sign-in set; nothing else identifies it. */
export async function fetchGoogleVerification(signal?: AbortSignal, transport: typeof fetch = globalThis.fetch): Promise<GoogleVerification> {
  const response = await transport('/api/v1/auth/google/verification', { signal, credentials: 'same-origin', cache: 'no-store', redirect: 'error', referrerPolicy: 'no-referrer' })
  if (!response.ok) return { state: 'none', reason: null }
  const body = await response.json().catch(() => null) as Partial<GoogleVerification> | null
  return body && (body.state === 'pending' || body.state === 'done' || body.state === 'failed') ? { state: body.state, reason: typeof body.reason === 'string' ? body.reason : null } : { state: 'none', reason: null }
}
/** Where a proving sign-in may send the browser: an http(s) address only, never a script or an app. */
export function googleDestination(value: unknown): string | null {
  if (typeof value !== 'string') return null
  try { const url = new URL(value); return (url.protocol === 'https:' || url.protocol === 'http:') && !url.username && !url.password ? url.href : null } catch { return null }
}
export interface GoogleStartFailure { field: 'code' | 'email' | 'clientId' | 'clientSecret' | null; message: string; waitSeconds: number }
export type GoogleStart = { ok: true; url: string } | { ok: false; failure: GoogleStartFailure }
/** Starts claiming the server with Google. The setup code is the bearer of this one request, as when claiming
 * with a password; the answer is where to send the browser, and a cookie only the server can read. */
export async function startGoogleClaim(code: string, client: { clientId: string; clientSecret: string }, signal?: AbortSignal, transport: typeof fetch = globalThis.fetch): Promise<GoogleStart> {
  // Same-origin credentials, not "omit": the browser must keep the HttpOnly cookie the answer sets, which is what ties
  // the sign-in that follows to this browser. The server reads only the bearer here; no cookie authorises a claim.
  return startGoogle('/api/v1/admin/owner/setup/google', client, { Authorization: `Bearer ${code}` }, 'same-origin', signal, transport)
}
/** Starts switching a server to Google, from an administrator's session. */
export async function startGoogleSwitch(client: { clientId: string; clientSecret: string }, signal?: AbortSignal, transport: typeof fetch = globalThis.fetch): Promise<GoogleStart> {
  return startGoogle('/api/v1/admin/sign-in/google', client, {}, 'same-origin', signal, transport)
}
async function startGoogle(path: string, client: { clientId: string; clientSecret: string }, headers: Record<string, string>, credentials: RequestCredentials, signal: AbortSignal | undefined, transport: typeof fetch): Promise<GoogleStart> {
  let response: Response
  try {
    // Never "include": only this origin's own cookies are ever sent or kept.
    response = await transport(path, { method: 'POST', signal, credentials, cache: 'no-store', redirect: 'error', referrerPolicy: 'no-referrer', headers: { ...headers, 'Content-Type': 'application/json' }, body: JSON.stringify({ clientId: client.clientId.trim(), clientSecret: client.clientSecret.trim() }) })
  } catch (error) {
    if (signal?.aborted) throw error
    return { ok: false, failure: { field: null, message: 'The server could not be reached. Check your connection and try again.', waitSeconds: 0 } }
  }
  if (response.ok) {
    const url = googleDestination((await response.json().catch(() => null) as { authorizationUrl?: unknown } | null)?.authorizationUrl)
    return url ? { ok: true, url } : { ok: false, failure: { field: null, message: 'The server could not start the Google sign-in. Try again in a moment.', waitSeconds: 0 } }
  }
  if (response.status === 400) {
    const message = String((await response.json().catch(() => null) as { message?: unknown } | null)?.message ?? '')
    return { ok: false, failure: /HTTPS/.test(message) ? { field: null, message: 'Google sign-in needs an HTTPS address. This server is reached over plain HTTP.', waitSeconds: 0 } : /secret/i.test(message) ? { field: 'clientSecret', message: 'That is not a client secret. Copy it again from Google Cloud.', waitSeconds: 0 } : { field: 'clientId', message: 'That is not a client ID. It ends in .apps.googleusercontent.com.', waitSeconds: 0 } }
  }
  return { ok: false, failure: claimFailure(response.status, response.headers.get('retry-after')) }
}

/** Runs the proving sign-in in its own small window, so what was typed stays on the page behind it, and waits for
 * the server to say how it went. Call from a click: the window is opened before anything else happens. When the
 * browser will not open a window the whole page goes to Google instead, and the page reads the outcome when it
 * comes back. */
export async function proveWithGoogle(start: (signal: AbortSignal) => Promise<GoogleStart>, signal: AbortSignal, environment: { open?: () => Window | null; status?: (signal: AbortSignal) => Promise<GoogleVerification>; wait?: (ms: number) => Promise<void>; leave?: (url: string) => void } = {}): Promise<{ ok: true } | { ok: false; failure: GoogleStartFailure }> {
  const open = environment.open ?? (() => window.open('about:blank', 'cereal-google', 'popup,width=520,height=700'))
  const status = environment.status ?? (inner => fetchGoogleVerification(inner)), wait = environment.wait ?? (ms => new Promise<void>(resolve => window.setTimeout(resolve, ms)))
  const leave = environment.leave ?? (url => window.location.assign(url))
  let popup: Window | null = null
  try { popup = open() } catch { popup = null }
  const close = () => { try { popup?.close() } catch { /* already gone */ } }
  let started: GoogleStart
  try { started = await start(signal) } catch (error) { close(); throw error }
  if (!started.ok) { close(); return started }
  if (!popup) { leave(started.url); await new Promise<never>(() => {}) }
  try { popup!.location.replace(started.url) } catch { close(); leave(started.url); await new Promise<never>(() => {}) }
  // Ten minutes is how long the server keeps the attempt.
  for (let waited = 0; waited < 620_000; waited += 900) {
    if (signal.aborted) { close(); throw new DOMException('Cancelled', 'AbortError') }
    await wait(900)
    const now = await status(signal).catch(() => ({ state: 'pending', reason: null }) as GoogleVerification)
    if (now.state === 'done') { close(); return { ok: true } }
    if (now.state === 'failed') { close(); return { ok: false, failure: { ...googleFailure(now.reason), waitSeconds: 0 } } }
  }
  close()
  return { ok: false, failure: { ...googleFailure('expired'), waitSeconds: 0 } }
}

/** Where a reload lands: the first step the server does not report as finished, else the last screen. */
export function resumeStep(steps: OnboardingStepsDTO): StepId {
  const stored: Array<[StepId, boolean]> = [['name', steps.name], ['language', steps.language], ['speech', steps.speech], ['models', steps.models], ['signIn', steps.signIn], ['storage', steps.storage]]
  return stored.find(([, done]) => !done)?.[0] ?? 'apps'
}

/** Whether the admin panel shows setup in place of the settings: the server reports setup and it is not finished. */
export const needsOnboarding = (settings: Pick<AdminSettingsDTO, 'onboarding'>) => settings.onboarding !== undefined && settings.onboarding.completedAt === null

/** The setup code as the server knows it. People paste what `grep ADMIN_API_TOKEN .env` printed, so the
 * variable's name, quotes and stray spaces are taken off. Null when what is left cannot be a code. */
export function setupCode(pasted: string): string | null {
  const value = pasted.trim().replace(/^(?:export\s+)?ADMIN_API_TOKEN\s*=\s*/i, '').replace(/^(['"])(.*)\1$/, '$2').trim()
  return value.length >= 32 && value.length <= 512 && !/\s/.test(value) ? value : null
}

export interface OwnerDraft { name: string; email: string; password: string }
/** The first thing wrong with the owner account as typed, in the order the fields appear. */
export function ownerProblem(draft: OwnerDraft): { field: 'name' | 'email' | 'password'; message: string } | null {
  if (!draft.name.trim()) return { field: 'name', message: 'Enter your name.' }
  if (draft.name.trim().length > 200) return { field: 'name', message: 'Use a name of 200 characters or fewer.' }
  if (!/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(draft.email.trim()) || draft.email.trim().length > 254) return { field: 'email', message: 'Enter your email address.' }
  if (draft.password.length < 12) return { field: 'password', message: `Use at least 12 characters: ${12 - draft.password.length} more to go.` }
  if (draft.password.length > 128) return { field: 'password', message: 'Use a password of 128 characters or fewer.' }
  return null
}

export type ClaimFailure = { field: 'code' | 'email' | null; message: string; /** Seconds before another try is worth making. */ waitSeconds: number }
/** What to say when claiming did not work. The answers are specific so the person knows which field to look at. */
export function claimFailure(status: number, retryAfter: string | null = null): ClaimFailure {
  const wait = Number(retryAfter)
  if (status === 429) return { field: null, message: 'Too many tries in a row. Give it a minute, then try again.', waitSeconds: Number.isFinite(wait) && wait > 0 ? Math.min(600, Math.ceil(wait)) : 60 }
  if (status === 401 || status === 403) return { field: 'code', message: 'That code did not match this server. Copy it again from the machine Cereal runs on.', waitSeconds: 0 }
  if (status === 409) return { field: 'email', message: 'This server already has an owner, or that email is taken. Reload to sign in instead.', waitSeconds: 0 }
  if (status === 400) return { field: null, message: 'Check your name, email and password, then try again.', waitSeconds: 0 }
  return { field: null, message: 'The server could not finish this. Try again in a moment.', waitSeconds: 0 }
}

/** Claims the server with the setup code. The code is sent once, as the bearer credential of this one request:
 * it is never stored, never put in an address, and never sent with cookies. */
export async function claimServer(code: string, owner: OwnerDraft, signal?: AbortSignal, transport: typeof fetch = globalThis.fetch): Promise<{ ok: true } | { ok: false; failure: ClaimFailure }> {
  let response: Response
  try {
    response = await transport('/api/v1/admin/owner/setup', {
      method: 'POST', signal, credentials: 'omit', cache: 'no-store', redirect: 'error', referrerPolicy: 'no-referrer',
      headers: { Authorization: `Bearer ${code}`, 'Content-Type': 'application/json' },
      body: JSON.stringify({ name: owner.name.trim(), email: owner.email.trim().toLowerCase(), password: owner.password }),
    })
  } catch (error) {
    if (signal?.aborted) throw error
    return { ok: false, failure: { field: null, message: 'The server could not be reached. Check your connection and try again.', waitSeconds: 0 } }
  }
  return response.ok ? { ok: true } : { ok: false, failure: claimFailure(response.status, response.headers.get('retry-after')) }
}

/** The providers a person can pick for notes and chat, in the order they are offered. */
export const LANGUAGE_PROVIDERS = ['openrouter', 'openai', 'anthropic', 'openai_compatible'] as const
export type LanguageProvider = typeof LANGUAGE_PROVIDERS[number]
export const PROVIDER_HINTS: Record<LanguageProvider | 'elevenlabs', { blurb: string; keyHint: string; keysUrl: string }> = {
  openrouter: { blurb: 'All models, one key', keyHint: 'sk-or-…', keysUrl: 'https://openrouter.ai/keys' },
  openai: { blurb: 'GPT models', keyHint: 'sk-…', keysUrl: 'https://platform.openai.com/api-keys' },
  anthropic: { blurb: 'Claude models', keyHint: 'sk-ant-…', keysUrl: 'https://console.anthropic.com/settings/keys' },
  openai_compatible: { blurb: 'Ollama, vLLM, LM Studio, or any /v1 API', keyHint: 'Leave blank if the server needs none', keysUrl: '' },
  elevenlabs: { blurb: 'Shows the words while you record, with speaker labels', keyHint: 'sk_…', keysUrl: 'https://elevenlabs.io/app/settings/api-keys' },
}
const configured = (settings: AdminSettingsDTO, provider: AIProvider) => settings.providers.some(status => status.id === provider && status.configured)
/** The provider the step opens on: one that already has a key, else OpenRouter. */
export function initialLanguageProvider(settings: AdminSettingsDTO): LanguageProvider {
  const chosen = settings.chatModels[0]?.provider
  if (chosen && configured(settings, chosen)) return chosen
  return LANGUAGE_PROVIDERS.find(provider => configured(settings, provider)) ?? 'openrouter'
}
/** A key that is worth sending to the provider: the same bounds the server accepts. */
export const plausibleKey = (key: string) => { const value = key.trim(); return value.length >= 10 && value.length <= 1000 && !/[\r\n]/.test(value) }

/** Newest and most capable first, as far as a model's name tells: used only to suggest a default. */
function rank(provider: LanguageProvider, model: AIModelDTO): number {
  const id = model.id.toLowerCase()
  let score = 0
  const version = /(\d+(?:\.\d+)?)/.exec(id.replace(/\d{4}-?\d{2}-?\d{2}/g, ''))
  if (version) score += Math.min(99, Number(version[1])) * 10
  if (/\d{4}-?\d{2}-?\d{2}|preview|latest|beta|exp/.test(id)) score -= 3
  if (/mini|nano|lite|haiku|flash|small|instant/.test(id)) score -= 40
  if (provider === 'anthropic' && /sonnet/.test(id)) score += 5
  if (provider === 'openai' && !/^gpt-/.test(id)) score -= 20
  return score
}
/** A sensible model for notes and chat from a provider's catalog: the server's default when the provider offers
 * it (as "openai/gpt-…" on OpenRouter, "gpt-…" at OpenAI), else the best-ranked language model. */
export function suggestLanguageModel(provider: LanguageProvider, catalog: ModelCatalogDTO | undefined, serverDefault: string): LanguageModelSelection | null {
  const models = (catalog?.models ?? []).filter(model => model.capabilities.includes('language'))
  if (!models.length) return null
  const wanted = provider === 'openrouter' ? serverDefault : serverDefault.replace(/^[a-z0-9-]+\//, '')
  const exact = models.find(model => model.id === wanted)
  if (exact) return { provider, model: exact.id }
  if (provider === 'openrouter') {
    // The same family as the default, then anything.
    const family = serverDefault.split('/')[0] + '/', inFamily = models.filter(model => model.id.startsWith(family))
    const pool = inFamily.length ? inFamily : models
    return { provider, model: [...pool].sort((a, b) => rank('openai', b) - rank('openai', a) || a.id.localeCompare(b.id))[0]!.id }
  }
  return { provider, model: [...models].sort((a, b) => rank(provider, b) - rank(provider, a) || a.id.localeCompare(b.id))[0]!.id }
}

/** The live model a server uses until another is chosen. */
const DEFAULT_LIVE: NonNullable<AdminSettingsDTO['liveTranscriptionModel']> = { provider: 'elevenlabs', model: 'scribe_v3_live' }
export interface ModelsDraft {
  chat: LanguageModelSelection; notes: LanguageModelSelection
  /** Always set: transcription is live, and live transcription cannot be turned off. */
  live: NonNullable<AdminSettingsDTO['liveTranscriptionModel']>
}
/** The three choices as the Models step opens: what the server has, except that a chat or notes model whose
 * provider has no key is replaced by a suggestion from the provider that was just connected. */
export function initialModels(settings: AdminSettingsDTO, catalogs: Partial<Record<AIProvider, ModelCatalogDTO | undefined>>): ModelsDraft {
  const connected = LANGUAGE_PROVIDERS.filter(provider => configured(settings, provider))
  const usable = (selection: LanguageModelSelection) => configured(settings, selection.provider)
  const fallback = () => {
    for (const provider of connected) { const suggestion = suggestLanguageModel(provider, catalogs[provider], settings.languageModel); if (suggestion) return suggestion }
    return null
  }
  const chat = settings.chatModels[0]!
  return {
    chat: usable(chat) ? chat : fallback() ?? chat,
    notes: usable(settings.generationModel) ? settings.generationModel : fallback() ?? settings.generationModel,
    live: settings.liveTranscriptionModel ?? DEFAULT_LIVE,
  }
}
/** What Continue saves on the Models step: all three, so the choice is stored even when the defaults were kept.
 * Other chat models already enabled stay enabled, after the chosen default. There is no batch speech-to-text model to
 * choose: transcription is live only. */
export function modelsPatch(draft: ModelsDraft, settings: AdminSettingsDTO) {
  const key = (model: LanguageModelSelection) => `${model.provider}:${model.model}`
  const others = settings.chatModels.slice(1).filter(model => key(model) !== key(draft.chat) && settings.providers.some(status => status.id === model.provider && status.configured))
  return { chatModels: [draft.chat, ...others], generationModel: draft.notes, liveTranscriptionModel: draft.live }
}
/** What Continue on "Notes and chat" saves for an OpenAI-compatible server: its address, a key only when one was typed,
 * and the model as the chat and notes model (it is the only model that server is known to have). The server checks the
 * model, from the server's list or with a test request, before anything is stored. */
export function compatiblePatch(draft: { baseUrl: string; key: string; model: string }, settings: AdminSettingsDTO) {
  const chosen: LanguageModelSelection = { provider: 'openai_compatible', model: draft.model.trim() }
  const key = (model: LanguageModelSelection) => `${model.provider}:${model.model}`
  const others = settings.chatModels.filter(model => key(model) !== key(chosen) && settings.providers.some(status => status.id === model.provider && status.configured)).slice(0, 19)
  return { openAiCompatibleBaseUrl: draft.baseUrl.trim(), ...(draft.key.trim() ? { openAiCompatibleApiKey: draft.key.trim() } : {}), chatModels: [chosen, ...others], generationModel: chosen }
}

/** A model's name for a small card: the catalog's name when it is loaded, else the identifier without its vendor. */
export function modelLabel(selection: { provider: AIProvider; model: string }, catalogs: Partial<Record<AIProvider, ModelCatalogDTO | undefined>>): string {
  return catalogs[selection.provider]?.models.find(model => model.id === selection.model)?.name ?? selection.model.replace(/^[^/]+\//, '')
}

/** The address people type into the apps: this server, without a trailing slash. */
export const serverAddress = (origin: string) => origin.replace(/\/+$/, '')
/** The same address as it reads in a small space: without the scheme. */
export const serverHost = (origin: string) => { try { return new URL(origin).host } catch { return origin } }

/** Where someone who has just signed in belongs: the setup flow for an owner or administrator of a server
 * that has not finished it, otherwise nowhere in particular (null). Failures mean "nowhere in particular". */
export async function setupLanding(session: Pick<AuthSessionDTO, 'user'>, getSettings: () => Promise<Pick<AdminSettingsDTO, 'onboarding'>>): Promise<'/admin' | null> {
  if (session.user.role === 'user') return null
  try { return needsOnboarding(await getSettings()) ? '/admin' : null } catch { return null }
}
