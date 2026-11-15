/** First-run setup: one step at a time, the inputs on the left and a little screen on the right that
 * previews what the step sets up. While it is unfinished it is all the admin panel shows. */
import { useCallback, useEffect, useMemo, useRef, useState, type FormEvent, type ReactNode } from 'react'
import type { AdminSettingsDTO, AIProvider, OnboardingStep } from '@meeting-notes/contracts'
import { AdminClientError, type AdminClient } from '../../lib/admin-client'
import { authClient } from '../../lib/auth-client'
import { getDownloads } from '../../lib/downloads'
import { instanceDraft, instancePatch, providerKeyFields, providerNames, signInProblem, storageDraft, storagePatch, storageProblem, storageTestInput, type SignInDraft, type StorageDraft } from '../../lib/instance-settings'
import { claimServer, initialLanguageProvider, initialModels, modelLabel, modelsPatch, OPTIONAL_STEPS, ownerProblem, plausibleKey, resumeStep, serverAddress, serverHost, setupCode, STEP_TITLES, stepsFor, type LanguageProvider, type ModelsDraft, type StepId } from '../../lib/onboarding'
import { modelLogo, type CatalogState } from '../admin/settings-panel'
import { reducedMotion, StepDots, type BuddyHandle, type Mood } from './kit'
import { LittleScreen, type ScreenView } from './screens'
import { AppsFields, ClaimFields, LanguageFields, ModelFields, NameFields, SignInFields, SpeechFields, StorageFields, type DownloadsState, type KeyCheck, type OwnerFields, type StorageTest } from './steps'
import '../../onboarding.css'

const HELP: Record<StepId, string> = {
  claim: 'Paste the setup code from the machine Cereal runs on, then make the owner account. That’s you.',
  name: 'This is what your team sees in the apps and on the sign-in page.',
  language: 'Pick who writes your notes and answers questions about them, then paste its API key.',
  speech: 'ElevenLabs turns what was said into a transcript. Paste its API key.',
  models: 'These are good defaults for the keys you added. Keep them, or pick your own.',
  signIn: 'Choose how people get in. You can add accounts from the admin panel afterwards.',
  storage: 'Recordings stay on this server unless you would rather keep them in a bucket.',
  apps: 'Cereal records on your Mac and iPhone. Point the apps at this server and you are done.',
}
const CONTINUE: Partial<Record<StepId, string>> = { claim: 'Claim this server', apps: 'Open the admin panel' }
const EMPTY_SIGN_IN: SignInDraft = { password: true, google: false, allowedEmails: [], googleClientId: '', googleClientSecret: '', removeGoogleSecret: false }
const EMPTY_STORAGE: StorageDraft = { backend: 'local', endpoint: '', region: '', bucket: '', prefix: '', forcePathStyle: false, accessKeyId: '', secretAccessKey: '', removeSecret: false }
const PANE_EXIT_MS = 180, CELEBRATION_MS = 1500

interface Drafts {
  owner: OwnerFields
  name: string; logo: string | null
  provider: LanguageProvider; languageKey: string; speechKey: string
  models: ModelsDraft | null
  signIn: SignInDraft; storage: StorageDraft
}
const draftsFor = (settings: AdminSettingsDTO | null): Omit<Drafts, 'owner'> => ({
  // A name nobody chose yet is left for the person to type: the little screen fills in as they do.
  name: settings && (settings.onboarding?.steps.name || settings.instanceName !== 'Cereal') ? settings.instanceName : '',
  logo: settings?.instanceLogo ?? null,
  provider: settings ? initialLanguageProvider(settings) : 'openrouter', languageKey: '', speechKey: '',
  models: null,
  signIn: settings ? instanceDraft(settings).signIn ?? EMPTY_SIGN_IN : EMPTY_SIGN_IN,
  storage: settings?.storage ? storageDraft(settings.storage) : EMPTY_STORAGE,
})

export function Onboarding({ client, initial, expired: sessionEnded, finished }: {
  client: AdminClient
  /** An unclaimed server starts at "Claim this server"; a claimed one resumes at its first unfinished step,
   * or at the first step when the owner asked to see setup again. */
  initial: { kind: 'claim'; instanceName: string } | { kind: 'ready'; settings: AdminSettingsDTO; fromStart?: boolean }
  expired: () => void
  finished: (settings: AdminSettingsDTO) => void
}) {
  // One function for the life of the flow, whatever the caller passes on each render.
  const ended = useRef(sessionEnded); ended.current = sessionEnded
  const expired = useCallback(() => ended.current(), [])
  const [claiming] = useState(initial.kind === 'claim')
  const steps = useMemo(() => stepsFor(claiming), [claiming])
  const [settings, setSettings] = useState<AdminSettingsDTO | null>(initial.kind === 'ready' ? initial.settings : null)
  const [step, setStep] = useState<StepId>(() => initial.kind === 'claim' ? 'claim' : initial.fromStart ? 'name' : resumeStep(initial.settings.onboarding!.steps))
  const [panes, setPanes] = useState<{ current: StepId; leaving: StepId | null; direction: 'forward' | 'backward' }>({ current: step, leaving: null, direction: 'forward' })
  if (panes.current !== step) setPanes({ current: step, leaving: panes.current, direction: steps.indexOf(step) >= steps.indexOf(panes.current) ? 'forward' : 'backward' })
  const [drafts, setDrafts] = useState<Drafts>(() => ({ owner: { code: '', name: '', email: '', password: '' }, ...draftsFor(initial.kind === 'ready' ? initial.settings : null) }))
  const [busy, setBusy] = useState(false)
  const [problem, setProblem] = useState<string | null>(null)
  const [ownerProblems, setOwnerProblems] = useState<Partial<Record<keyof OwnerFields, string>>>({})
  const [wait, setWait] = useState(0)
  const [accepted, setAccepted] = useState(false)
  const [nameProblem, setNameProblem] = useState<string | null>(null)
  const [catalogs, setCatalogs] = useState<Partial<Record<AIProvider, CatalogState>>>({})
  const [storageTest, setStorageTest] = useState<StorageTest>({ state: 'idle' })
  const [downloads, setDownloads] = useState<DownloadsState>({ state: 'loading' })
  const [celebrating, setCelebrating] = useState(false)
  const buddy = useRef<BuddyHandle>(null), title = useRef<HTMLHeadingElement>(null)
  const alive = useRef(true), requests = useRef(new Set<AbortController>())
  useEffect(() => { alive.current = true; return () => { alive.current = false; requests.current.forEach(request => request.abort()) } }, [])
  const update = useCallback((next: Partial<Drafts>) => setDrafts(old => ({ ...old, ...next })), [])
  const index = steps.indexOf(step), origin = typeof window === 'undefined' ? '' : window.location.origin

  // The pane that was showing leaves while the new one arrives; focus goes to the new step's title.
  useEffect(() => {
    if (!panes.leaving) return
    const timer = window.setTimeout(() => setPanes(now => ({ ...now, leaving: null })), PANE_EXIT_MS)
    return () => window.clearTimeout(timer)
  }, [panes.leaving, step])
  const firstPaint = useRef(true)
  useEffect(() => {
    if (firstPaint.current) { firstPaint.current = false; return }
    title.current?.focus({ preventScroll: true })
    window.scrollTo({ top: 0, behavior: reducedMotion() ? 'auto' : 'smooth' })
  }, [step])
  useEffect(() => { if (wait <= 0) return; const timer = window.setTimeout(() => setWait(seconds => seconds - 1), 1000); return () => window.clearTimeout(timer) }, [wait])

  /** Runs one request for the current step. A session that ended goes back to sign-in; anything else is said calmly under the fields. */
  const attempt = useCallback(async <T,>(work: (signal: AbortSignal) => Promise<T>, failure = 'That could not be saved. Check the values and try again.'): Promise<T | undefined> => {
    const controller = new AbortController(); requests.current.add(controller)
    try { return await work(controller.signal) }
    catch (error) {
      if (controller.signal.aborted || !alive.current) return undefined
      if (error instanceof AdminClientError && error.status === 401) { expired(); return undefined }
      setProblem(error instanceof AdminClientError && error.status >= 500 ? 'The server could not finish this. Try again in a moment.' : failure)
      buddy.current?.wobble()
      return undefined
    } finally { requests.current.delete(controller) }
  }, [expired])

  /* ── Provider keys, checked as they are pasted ─────────────────────────── */
  const configured = useCallback((provider: AIProvider) => !!settings?.providers.some(status => status.id === provider && status.configured), [settings])
  const languageCheck = useKeyCheck(client, drafts.provider, drafts.languageKey, step === 'language', configured(drafts.provider), setCatalogs, buddy, expired)
  const speechCheck = useKeyCheck(client, 'elevenlabs', drafts.speechKey, step === 'speech', configured('elevenlabs'), setCatalogs, buddy, expired)

  /* ── Model catalogs for the Models step ────────────────────────────────── */
  const loadCatalog = useCallback(async (provider: AIProvider, refresh = false) => {
    setCatalogs(old => ({ ...old, [provider]: { ...old[provider], busy: true, error: undefined } }))
    try {
      const data = await client.getModelCatalog({ provider, refresh })
      if (alive.current) setCatalogs(old => ({ ...old, [provider]: { data, busy: false } }))
    } catch (error) {
      if (!alive.current) return
      if (error instanceof AdminClientError && error.status === 401) { expired(); return }
      setCatalogs(old => ({ ...old, [provider]: { busy: false, error: 'Couldn’t load models. Check the API key and try again.' } }))
    }
  }, [client, expired])
  const onModels = step === 'models'
  const known = useRef(catalogs); known.current = catalogs
  useEffect(() => {
    if (!onModels || !settings) return
    for (const status of settings.providers) if (status.configured && !known.current[status.id]?.data && !known.current[status.id]?.busy) void loadCatalog(status.id)
  }, [onModels, settings, loadCatalog])
  // The four choices are worked out once every connected provider has answered, so the suggestion is made from real lists.
  useEffect(() => {
    if (!onModels || !settings || drafts.models) return
    const waiting = settings.providers.some(status => status.configured && !catalogs[status.id]?.data && !catalogs[status.id]?.error)
    if (!waiting) update({ models: initialModels(settings, Object.fromEntries(Object.entries(catalogs).map(([id, state]) => [id, state?.data]))) })
  }, [onModels, settings, catalogs, drafts.models, update])

  /* ── Downloads, for the last step ──────────────────────────────────────── */
  const loadDownloads = useCallback(async () => {
    setDownloads({ state: 'loading' })
    try { const found = await getDownloads(); if (alive.current) setDownloads({ state: 'ready', downloads: found }) }
    catch { if (alive.current) setDownloads({ state: 'failed' }) }
  }, [])
  const onApps = step === 'apps'
  useEffect(() => { if (onApps) void loadDownloads() }, [onApps, loadDownloads])

  /* ── Moving between steps ──────────────────────────────────────────────── */
  const go = useCallback((to: StepId) => { setProblem(null); setStep(to) }, [])
  const forward = useCallback(() => { const next = steps[steps.indexOf(step) + 1]; if (next) go(next) }, [go, step, steps])
  const previous = steps[index - 1] && steps[index - 1] !== 'claim' ? steps[index - 1]! : null
  /** Tells the server the step is finished (or passed over) and keeps what it answers. */
  const record = useCallback(async (name: OnboardingStep, state: 'done' | 'skipped', signal: AbortSignal) => { const next = await client.recordOnboardingStep({ step: name, state }, signal); if (alive.current) setSettings(next); return next }, [client])
  const signInSaved = settings?.signIn ?? null, storageSaved = settings?.storage ?? null
  const signInTrouble = signInSaved ? signInProblem(drafts.signIn, signInSaved.googleClientSecretSet) : null
  const storageTrouble = storageSaved ? storageProblem(drafts.storage, storageSaved.secretAccessKeySet) : null

  async function claim() {
    const code = setupCode(drafts.owner.code), trouble = ownerProblem(drafts.owner)
    if (!code) { setOwnerProblems({ code: drafts.owner.code.trim() ? 'That does not look like a setup code. It is one long line of letters and numbers.' : 'Paste the setup code to continue.' }); buddy.current?.wobble(); return }
    if (trouble) { setOwnerProblems({ [trouble.field]: trouble.message }); buddy.current?.wobble(); return }
    setOwnerProblems({})
    const controller = new AbortController(); requests.current.add(controller)
    try {
      const result = await claimServer(code, drafts.owner, controller.signal)
      if (!alive.current) return
      if (!result.ok) {
        const { field, message, waitSeconds } = result.failure
        if (field) setOwnerProblems({ [field]: message }); else setProblem(message)
        setWait(waitSeconds); buddy.current?.wobble(); return
      }
      // The code has done its job: it leaves the page's memory now, before anything else happens.
      const { email, password } = drafts.owner
      update({ owner: { ...drafts.owner, code: '', password: '' } })
      setAccepted(true); buddy.current?.hop()
      const [signedIn] = await Promise.all([authClient.signIn.email({ email: email.trim().toLowerCase(), password, rememberMe: false }, { signal: controller.signal }), new Promise(resolve => window.setTimeout(resolve, reducedMotion() ? 200 : 1100))])
      if (!alive.current) return
      // The account exists either way; if signing in did not work here, the sign-in page takes over.
      if (signedIn.error) { window.location.replace('/login?return=%2Fadmin'); return }
      const loaded = await client.getSettings(controller.signal)
      if (!alive.current) return
      setSettings(loaded); setDrafts(old => ({ owner: old.owner, ...draftsFor(loaded) })); go('name')
    } catch { if (alive.current && !controller.signal.aborted) { setProblem('The server could not be reached. Check your connection and try again.'); buddy.current?.wobble() } }
    finally { requests.current.delete(controller) }
  }

  async function submit(skip = false) {
    if (busy || celebrating) return
    setProblem(null); setBusy(true)
    try {
      if (step === 'claim') { await claim(); return }
      if (!settings) return
      if (skip) { if (await attempt(signal => record(step as OnboardingStep, 'skipped', signal))) forward(); return }
      switch (step) {
        case 'name': {
          const name = drafts.name.trim()
          if (!name) { setNameProblem('Give your space a name: your team, your company, or just you.'); buddy.current?.wobble(); return }
          const saved = await attempt(async signal => {
            if (name !== settings.instanceName || drafts.logo !== settings.instanceLogo) await client.updateSettings({ ...(name !== settings.instanceName ? { instanceName: name } : {}), ...(drafts.logo !== settings.instanceLogo ? { instanceLogo: drafts.logo } : {}) }, signal)
            return record('name', 'done', signal)
          })
          if (saved) { buddy.current?.nod(); forward() }
          return
        }
        case 'language': case 'speech': {
          const provider: AIProvider = step === 'language' ? drafts.provider : 'elevenlabs', key = (step === 'language' ? drafts.languageKey : drafts.speechKey).trim()
          const saved = await attempt(async signal => {
            if (key) await client.updateSettings({ [providerKeyFields[provider]]: key }, signal)
            return record(step, 'done', signal)
          }, `${providerNames[provider]} is not connected yet. Paste its key and wait for “Connected”.`)
          if (!saved) return
          // The key is stored now; the copy typed here is dropped, and the model suggestions start over from it.
          update(step === 'language' ? { languageKey: '', models: null } : { speechKey: '', models: null })
          forward(); return
        }
        case 'models': {
          if (!drafts.models) return
          const chosen = drafts.models
          const saved = await attempt(async signal => { await client.updateSettings(modelsPatch(chosen, settings), signal); return record('models', 'done', signal) }, 'One of these models is not available with your keys. Pick another and try again.')
          if (saved) { buddy.current?.nod(); forward() }
          return
        }
        case 'signIn': {
          if (signInTrouble) { setProblem(signInTrouble); buddy.current?.wobble(); return }
          const patch = instancePatch({ ...instanceDraft(settings), signIn: drafts.signIn }, settings).signIn
          const saved = await attempt(async signal => { if (patch) await client.updateSettings({ signIn: patch }, signal); return record('signIn', 'done', signal) }, 'Those sign-in settings were not accepted. Check the Google client ID and secret.')
          if (saved) { update({ signIn: instanceDraft(saved).signIn ?? EMPTY_SIGN_IN }); buddy.current?.nod(); forward() }
          return
        }
        case 'storage': {
          if (!storageSaved) return
          if (storageTrouble) { setProblem(storageTrouble); buddy.current?.wobble(); return }
          const patch = storagePatch(drafts.storage, storageSaved)
          const saved = await attempt(async signal => { if (Object.keys(patch).length) await client.updateSettings({ storage: patch }, signal); return record('storage', 'done', signal) }, 'Those storage settings were not accepted. Check the bucket and its keys.')
          if (saved) { if (saved.storage) update({ storage: storageDraft(saved.storage) }); buddy.current?.nod(); forward() }
          return
        }
        case 'apps': {
          const done = await attempt(signal => client.completeOnboarding(signal), 'Setup cannot be finished yet: a step before this one still needs its key.')
          if (!done) return
          setCelebrating(true); buddy.current?.hop()
          window.setTimeout(() => { if (alive.current) finished(done) }, reducedMotion() ? 500 : CELEBRATION_MS)
          return
        }
      }
    } finally { if (alive.current) setBusy(false) }
  }

  async function runStorageTest() {
    setStorageTest({ state: 'testing' })
    const controller = new AbortController(); requests.current.add(controller)
    try {
      const result = await client.testStorage(storageTestInput(drafts.storage), controller.signal)
      if (!alive.current) return
      setStorageTest({ state: 'done', result })
      if (result.ok) buddy.current?.hop(); else buddy.current?.wobble()
    } catch (error) {
      if (!alive.current || controller.signal.aborted) return
      if (error instanceof AdminClientError && error.status === 401) { expired(); return }
      setStorageTest({ state: 'done', result: { ok: false, message: error instanceof AdminClientError && error.status === 400 ? 'These settings are not complete yet. Check the bucket, region and keys.' : 'The test could not be run. Try again.' } })
      buddy.current?.wobble()
    } finally { requests.current.delete(controller) }
  }

  /* ── What each step can do right now ───────────────────────────────────── */
  const canContinue = step === 'claim' ? wait === 0
    : step === 'language' ? languageCheck.state === 'connected'
    : step === 'speech' ? speechCheck.state === 'connected'
    : step === 'models' ? drafts.models !== null
    : true
  const waitingOn = step === 'claim' && wait > 0 ? `Try again in ${wait}s`
    : (step === 'language' && languageCheck.state === 'checking') || (step === 'speech' && speechCheck.state === 'checking') ? 'Checking…' : null

  const fields = (which: StepId): ReactNode => {
    switch (which) {
      case 'claim': return <ClaimFields value={drafts.owner} disabled={busy || accepted} problems={ownerProblems} change={next => { update({ owner: { ...drafts.owner, ...next } }); setOwnerProblems({}); setProblem(null) }} />
      case 'name': return <NameFields name={drafts.name} logo={drafts.logo} problem={nameProblem} change={next => { update(next); setNameProblem(null) }} />
      case 'language': return <LanguageFields provider={drafts.provider} keyValue={drafts.languageKey} check={languageCheck} saved={configured} change={next => { update({ ...(next.provider ? { provider: next.provider } : {}), ...(next.key !== undefined ? { languageKey: next.key } : {}) }); setProblem(null) }} />
      case 'speech': return <SpeechFields keyValue={drafts.speechKey} check={speechCheck} saved={configured('elevenlabs')} change={speechKey => { update({ speechKey }); setProblem(null) }} />
      case 'models': return settings && <ModelFields draft={drafts.models} settings={settings} catalogs={catalogs} available={configured} load={loadCatalog} change={models => { update({ models }); setProblem(null) }} />
      case 'signIn': return signInSaved && <SignInFields draft={drafts.signIn} saved={signInSaved} change={signIn => { update({ signIn }); setProblem(null) }} />
      case 'storage': return storageSaved && <StorageFields draft={drafts.storage} saved={storageSaved} test={storageTest} runTest={() => void runStorageTest()} problem={storageTrouble}
        change={next => { update({ storage: { ...drafts.storage, ...next } }); setStorageTest({ state: 'idle' }); setProblem(null) }} />
      case 'apps': return <AppsFields downloads={downloads} address={serverAddress(origin)} retry={() => void loadDownloads()} />
    }
  }
  const pane = (which: StepId, leaving: boolean) => <div className={`onb-pane${leaving ? ' is-leaving' : ''}`} key={leaving ? `leaving-${which}` : which} inert={leaving} aria-hidden={leaving ? true : undefined}>
    <p className="onb-eyebrow">{OPTIONAL_STEPS.includes(which) ? 'Optional' : which === 'apps' ? 'Last step' : which === 'claim' ? 'Welcome to Cereal' : 'Setup'}</p>
    {/* Focus lands here when the step changes; Enter from here continues, as it does from a field. */}
    <h1 ref={leaving ? undefined : title} tabIndex={-1} onKeyDown={event => { if (event.key === 'Enter' && event.target === event.currentTarget && !leaving && canContinue) { event.preventDefault(); void submit() } }}>{STEP_TITLES[which]}</h1>
    <p className="onb-help">{HELP[which]}</p>
    {fields(which)}
    {!leaving && problem && <p className="onb-problem is-step" role="alert">{problem}</p>}
    <div className="onb-actions">
      {!leaving && previous && <button type="button" className="quiet-button onb-back" disabled={busy || celebrating} onClick={() => go(previous)}><svg viewBox="0 0 10 10" width="10" height="10" aria-hidden="true"><path d="M6.5 1.5 3 5l3.5 3.5" /></svg>Back</button>}
      <span className="onb-actions-end">
        {OPTIONAL_STEPS.includes(which) && <button type="button" className="quiet-button" disabled={leaving || busy} onClick={() => void submit(true)}>Skip for now</button>}
        <button type="submit" className="button is-large onb-continue" disabled={leaving || busy || celebrating || !canContinue} data-busy={busy ? '' : undefined}>
          <span>{waitingOn ?? CONTINUE[which] ?? 'Continue'}</span>
          {busy ? <span className="spinner" aria-hidden="true" /> : <svg viewBox="0 0 12 12" width="12" height="12" aria-hidden="true"><path d="M2 6h7.5M6.5 2.7 9.8 6 6.5 9.3" /></svg>}
        </button>
      </span>
    </div>
  </div>

  /* ── The little screen ─────────────────────────────────────────────────── */
  const data = (provider: AIProvider) => catalogs[provider]?.data
  const lookup = { openrouter: data('openrouter'), openai: data('openai'), anthropic: data('anthropic'), elevenlabs: data('elevenlabs') }
  const modelCards = drafts.models ?? (settings ? { chat: settings.chatModels[0]!, notes: settings.generationModel, speech: settings.transcriptionModel, live: settings.liveTranscriptionModel ?? null } : null)
  const view: ScreenView = {
    step, host: serverHost(origin),
    instanceName: step === 'claim' ? '' : drafts.name, instanceLogo: drafts.logo,
    claim: { typed: Math.min(1, drafts.owner.code.trim().length / 32), accepted, owner: drafts.owner.name },
    language: { provider: drafts.provider, state: languageCheck.state === 'idle' ? 'waiting' : languageCheck.state },
    speech: { state: speechCheck.state === 'idle' ? 'waiting' : speechCheck.state },
    models: modelCards ? [
      { role: 'Chat', logo: modelLogo(modelCards.chat.provider, modelCards.chat.model), name: modelLabel(modelCards.chat, lookup) },
      { role: 'Meeting notes', logo: modelLogo(modelCards.notes.provider, modelCards.notes.model), name: modelLabel(modelCards.notes, lookup) },
      { role: 'Speech to text', logo: modelCards.speech.provider, name: modelLabel(modelCards.speech, lookup) },
      { role: 'Live transcription', logo: modelCards.live ? modelCards.live.provider : null, name: modelCards.live ? modelLabel(modelCards.live, lookup) : 'Off', live: !!modelCards.live },
    ] : [],
    signIn: { password: drafts.signIn.password, google: drafts.signIn.google, allowed: drafts.signIn.allowedEmails },
    storage: { backend: drafts.storage.backend, bucket: drafts.storage.bucket, test: storageTest.state === 'testing' ? 'testing' : storageTest.state === 'done' ? (storageTest.result.ok ? 'ok' : 'failed') : 'idle' },
    apps: { mac: downloads.state === 'ready' && !!downloads.downloads.mac, iphone: downloads.state === 'ready' && !!downloads.downloads.iosAppStoreUrl },
  }
  const mood: Mood = step === 'claim' && !accepted && !drafts.owner.code.trim() ? 'asleep' : celebrating || accepted && step === 'claim' ? 'happy' : 'idle'

  return <div className="onb" data-step={step} data-direction={panes.direction} data-celebrating={celebrating ? '' : undefined}>
    <header className="onb-top">
      <span className="onb-brand"><span className="mark" aria-hidden="true" /><span>Cereal</span></span>
      <StepDots titles={steps.map(id => STEP_TITLES[id])} index={index} />
      <span className="onb-count" aria-hidden="true">Step {index + 1} of {steps.length}</span>
    </header>
    <div className="onb-stage">
      <form className="onb-left" noValidate onSubmit={(event: FormEvent) => { event.preventDefault(); void submit() }}>
        {panes.leaving && panes.leaving !== step && pane(panes.leaving, true)}
        {pane(step, false)}
      </form>
      <aside className="onb-right"><LittleScreen view={view} mood={mood} buddy={buddy} /></aside>
    </div>
    {celebrating && <div className="onb-done" role="status">
      <span className="onb-done-mark" aria-hidden="true"><span className="mark" /></span>
      <p className="onb-done-title">You’re all set.</p>
      <p className="onb-done-line">{(drafts.name.trim() || settings?.instanceName || 'Cereal')} is ready for its first meeting.</p>
    </div>}
  </div>
}

/** Checks a pasted provider key against the provider, a moment after typing stops, through the same request
 * the admin panel uses to list a provider's models. A saved key with nothing typed over it counts as connected. */
function useKeyCheck(client: AdminClient, provider: AIProvider, key: string, active: boolean, saved: boolean,
  keep: (update: (old: Partial<Record<AIProvider, CatalogState>>) => Partial<Record<AIProvider, CatalogState>>) => void,
  buddy: { current: BuddyHandle | null }, expired: () => void): KeyCheck {
  // What the provider said, and about which key: an answer for anything else on screen is not shown.
  const [answer, setAnswer] = useState<{ subject: string; check: KeyCheck } | null>(null)
  const typed = key.trim(), subject = `${provider}\n${typed}`, worthAsking = plausibleKey(typed)
  useEffect(() => {
    if (!active || !worthAsking) return
    const controller = new AbortController()
    const timer = window.setTimeout(async () => {
      try {
        const catalog = await client.getModelCatalog({ provider, apiKey: typed }, controller.signal)
        if (controller.signal.aborted) return
        keep(old => ({ ...old, [provider]: { data: catalog, busy: false } }))
        setAnswer({ subject, check: { state: 'connected', count: catalog.models.length } }); buddy.current?.hop()
      } catch (error) {
        if (controller.signal.aborted) return
        if (error instanceof AdminClientError && error.status === 401) { expired(); return }
        setAnswer({ subject, check: { state: 'failed', message: `${providerNames[provider]} did not accept that key. Check that all of it was copied, then paste it again.` } }); buddy.current?.wobble()
      }
    }, 450)
    return () => { window.clearTimeout(timer); controller.abort() }
  }, [active, buddy, client, expired, keep, provider, subject, typed, worthAsking])
  if (!typed) return saved ? { state: 'connected' } : { state: 'idle' }
  if (!worthAsking) return { state: 'idle' }
  return answer?.subject === subject ? answer.check : { state: 'checking' }
}
