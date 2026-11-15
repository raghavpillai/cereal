/** First-run setup on the admin panel: the order of its steps, where to resume, and the small decisions each
 * step makes. Pure helpers and two requests; the screens are in src/components/onboarding. */
import type { AdminSettingsDTO, AIModelDTO, AIProvider, AuthSessionDTO, LanguageModelSelection, ModelCatalogDTO, OnboardingStepsDTO } from '@meeting-notes/contracts'

/** The screens, in order. `claim` exists only on a server nobody has claimed; `apps` is the last screen and is
 * never stored: finishing it finishes setup. */
export type StepId = 'claim' | 'name' | 'language' | 'speech' | 'models' | 'signIn' | 'storage' | 'apps'
export const STEP_ORDER: readonly StepId[] = ['claim', 'name', 'language', 'speech', 'models', 'signIn', 'storage', 'apps']
export const STEP_TITLES: Record<StepId, string> = {
  claim: 'Claim this server', name: 'Name your space', language: 'Notes and chat', speech: 'Speech to text',
  models: 'Models', signIn: 'Who can sign in', storage: 'Where files live', apps: 'Get the apps',
}
/** Steps that offer "Skip for now". */
export const OPTIONAL_STEPS: readonly StepId[] = ['signIn', 'storage']

/** The steps this visit walks: with "Claim this server" first when it started on an unclaimed server. */
export const stepsFor = (claiming: boolean): StepId[] => STEP_ORDER.filter(step => claiming || step !== 'claim')

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
export const LANGUAGE_PROVIDERS = ['openrouter', 'openai', 'anthropic'] as const
export type LanguageProvider = typeof LANGUAGE_PROVIDERS[number]
export const PROVIDER_HINTS: Record<LanguageProvider | 'elevenlabs', { blurb: string; keyHint: string; keysUrl: string }> = {
  openrouter: { blurb: 'All models, one key', keyHint: 'sk-or-…', keysUrl: 'https://openrouter.ai/keys' },
  openai: { blurb: 'GPT models', keyHint: 'sk-…', keysUrl: 'https://platform.openai.com/api-keys' },
  anthropic: { blurb: 'Claude models', keyHint: 'sk-ant-…', keysUrl: 'https://console.anthropic.com/settings/keys' },
  elevenlabs: { blurb: 'Scribe speech to text', keyHint: 'sk_…', keysUrl: 'https://elevenlabs.io/app/settings/api-keys' },
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

export interface ModelsDraft {
  chat: LanguageModelSelection; notes: LanguageModelSelection
  speech: AdminSettingsDTO['transcriptionModel']; live: NonNullable<AdminSettingsDTO['liveTranscriptionModel']> | null
}
/** The four choices as the Models step opens: what the server has, except that a chat or notes model whose
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
    speech: settings.transcriptionModel,
    live: settings.liveTranscriptionModel ?? null,
  }
}
/** What Continue saves on the Models step: all four, so the choice is stored even when the defaults were kept.
 * Other chat models already enabled stay enabled, after the chosen default. */
export function modelsPatch(draft: ModelsDraft, settings: AdminSettingsDTO) {
  const key = (model: LanguageModelSelection) => `${model.provider}:${model.model}`
  const others = settings.chatModels.slice(1).filter(model => key(model) !== key(draft.chat) && settings.providers.some(status => status.id === model.provider && status.configured))
  return { chatModels: [draft.chat, ...others], generationModel: draft.notes, transcriptionModel: draft.speech, liveTranscriptionModel: draft.live }
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
