/** The left side of each setup step: its fields. The flow (onboarding.tsx) owns what is typed, what was
 * checked and what Continue does; these only draw the inputs and report changes. */
import { useRef, useState, type ReactNode } from 'react'
import type { AdminSettingsDTO, AIProvider, LanguageModelSelection, LiveTranscriptionModelSelection, StorageTestDTO, TranscriptionModelSelection } from '@meeting-notes/contracts'
import { addAllowedEmails, bucketReady, defaultLiveModel, googleClientReady, providerNames, type SignInDraft, type StorageDraft } from '../../lib/instance-settings'
import { macDetails, type Downloads } from '../../lib/downloads'
import { LANGUAGE_PROVIDERS, PROVIDER_HINTS, type LanguageProvider, type ModelsDraft } from '../../lib/onboarding'
import { ModelPicker, type CatalogState } from '../admin/settings-panel'
import { Choice, CopyLine, DrawnCheck, Field, Switch, type FieldState } from './kit'

export type KeyCheck = { state: 'idle' | 'checking' | 'connected' | 'failed'; /** How many models the provider listed for this key. */ count?: number; message?: string }
const keyFieldState = (check: KeyCheck): FieldState => check.state === 'connected' ? 'good' : check.state === 'checking' ? 'checking' : check.state === 'failed' ? 'bad' : 'idle'
const ProviderLogo = ({ provider }: { provider: AIProvider }) => <img src={`/providers/${provider}.svg`} width="20" height="20" alt="" />

/* ── 1. Claim this server ────────────────────────────────────────────────── */
export interface OwnerFields { code: string; name: string; email: string; password: string }
export function ClaimFields({ value, change, problems, disabled }: {
  value: OwnerFields; change: (next: Partial<OwnerFields>) => void
  problems: Partial<Record<keyof OwnerFields, string>>; disabled: boolean
}) {
  return <><fieldset className="onb-fields" disabled={disabled}>
    {/* Not a password field: a password manager must not offer to keep the setup code. It is hidden as it is typed all the same. */}
    <Field label="Setup code" type="text" className="is-secret" name="setup-code" autoComplete="off" autoCorrect="off" autoCapitalize="none" spellCheck={false} maxLength={600} data-1p-ignore data-lpignore="true"
      value={value.code} onChange={event => change({ code: event.target.value })} problem={problems.code} placeholder="Paste the code from your server" />
    <div className="onb-where">
      <p>To see the code, run this in Cereal’s folder on the server:</p>
      <CopyLine value="grep ADMIN_API_TOKEN .env" prefix="$ " label="Copy the command that shows the setup code" />
    </div>
    <div className="onb-pair">
      <Field label="Your name" name="name" autoComplete="name" maxLength={200} value={value.name} onChange={event => change({ name: event.target.value })} problem={problems.name} />
      <Field label="Email" type="email" name="email" autoComplete="username" autoCapitalize="none" spellCheck={false} maxLength={254} value={value.email} onChange={event => change({ email: event.target.value })} problem={problems.email} />
    </div>
    <Field label="Password" type="password" name="new-password" autoComplete="new-password" maxLength={128} value={value.password} onChange={event => change({ password: event.target.value })}
      problem={problems.password} hint={value.password.length >= 12 ? 'Good. You will sign in with this email and password.' : 'At least 12 characters.'}
      state={value.password.length >= 12 && !problems.password ? 'good' : 'idle'} />
  </fieldset>
    <p className="onb-aside">Already have an account on this server? <a href="/login?signin=1">Sign in</a></p></>
}

/* ── 2. Name your space ──────────────────────────────────────────────────── */
export function NameFields({ name, logo, change, problem }: { name: string; logo: string | null; change: (next: { name?: string; logo?: string | null }) => void; problem: string | null }) {
  const input = useRef<HTMLInputElement>(null), version = useRef(0)
  const [logoProblem, setLogoProblem] = useState<string | null>(null)
  function read(file: File | undefined) {
    if (!file) return
    const mine = ++version.current; setLogoProblem(null)
    if (!['image/png', 'image/jpeg', 'image/webp'].includes(file.type) || file.size > 250_000) { setLogoProblem('Choose a PNG, JPG or WebP smaller than 250 KB.'); return }
    const reader = new FileReader()
    reader.onload = () => {
      if (mine !== version.current) return
      const value = reader.result
      if (typeof value === 'string' && /^data:image\/(png|jpeg|webp);base64,/.test(value)) change({ logo: value }); else setLogoProblem('This image could not be read.')
    }
    reader.onerror = () => setLogoProblem('This image could not be read.')
    reader.readAsDataURL(file)
  }
  return <div className="onb-fields">
    <Field label="Team name" name="organization" autoComplete="organization" maxLength={100} value={name} placeholder="Acme" onChange={event => change({ name: event.target.value })} problem={problem} />
    <div className="onb-field">
      <span className="onb-label" id="onb-logo-label">Logo <small>optional</small></span>
      <div className="onb-logo">
        <span className="onb-logo-tile" key={logo ? 'logo' : 'mark'}>{logo ? <img src={logo} alt="" /> : <span className="mark" aria-hidden="true" />}</span>
        <button type="button" className="button is-secondary pressable" aria-describedby="onb-logo-hint" onClick={() => input.current?.click()}>{logo ? 'Replace logo' : 'Upload logo'}</button>
        {logo && <button type="button" className="quiet-button" onClick={() => { version.current++; setLogoProblem(null); change({ logo: null }) }}>Remove</button>}
        <input ref={input} type="file" className="sr-only" tabIndex={-1} accept="image/png,image/jpeg,image/webp" aria-labelledby="onb-logo-label" onChange={event => { read(event.target.files?.[0]); event.target.value = '' }} />
      </div>
      {logoProblem ? <p className="onb-problem" role="alert">{logoProblem}</p> : <p className="onb-hint" id="onb-logo-hint">PNG, JPG or WebP, up to 250 KB.</p>}
    </div>
  </div>
}

/* ── 3 and 4. A provider key, checked as it is pasted ────────────────────── */
function KeyField({ provider, value, change, check, saved }: { provider: AIProvider; value: string; change: (key: string) => void; check: KeyCheck; saved: boolean }) {
  const hints = PROVIDER_HINTS[provider as LanguageProvider | 'elevenlabs']
  const hint: ReactNode = check.state === 'connected'
    ? <span className="onb-connected">Connected{check.count ? ` · ${check.count} ${check.count === 1 ? 'model' : 'models'} available` : saved && !value ? ' · key saved' : ''}</span>
    : check.state === 'checking' ? `Checking with ${providerNames[provider]}…`
    : <>Encrypted on your server and never shown again. <a href={hints.keysUrl} target="_blank" rel="noopener noreferrer">Get a key</a></>
  return <Field label={`${providerNames[provider]} API key`} type="password" name={`${provider}-key`} autoComplete="off" autoCapitalize="none" spellCheck={false} maxLength={1000}
    value={value} onChange={event => change(event.target.value)} placeholder={saved ? 'Leave blank to keep the saved key' : hints.keyHint}
    state={keyFieldState(check)} problem={check.state === 'failed' ? check.message : null} hint={hint} />
}
export function LanguageFields({ provider, keyValue, change, check, saved }: {
  provider: LanguageProvider; keyValue: string; change: (next: { provider?: LanguageProvider; key?: string }) => void; check: KeyCheck; saved: (provider: AIProvider) => boolean
}) {
  return <div className="onb-fields">
    <div className="onb-choices is-three" role="radiogroup" aria-label="Provider for notes and chat">
      {LANGUAGE_PROVIDERS.map(id => <Choice key={id} name="language-provider" value={id} checked={provider === id} onChange={() => change({ provider: id, key: '' })}
        title={providerNames[id]} detail={PROVIDER_HINTS[id].blurb} icon={<ProviderLogo provider={id} />} />)}
    </div>
    <KeyField key={provider} provider={provider} value={keyValue} change={key => change({ key })} check={check} saved={saved(provider)} />
  </div>
}
export function SpeechFields({ keyValue, change, check, saved }: { keyValue: string; change: (key: string) => void; check: KeyCheck; saved: boolean }) {
  return <div className="onb-fields">
    <div className="onb-provider-line"><span className="onb-choice-icon" aria-hidden="true"><ProviderLogo provider="elevenlabs" /></span><span><strong>ElevenLabs Scribe</strong><small>Transcripts with speaker labels, live while you record</small></span></div>
    <KeyField provider="elevenlabs" value={keyValue} change={change} check={check} saved={saved} />
  </div>
}

/* ── 5. Models ───────────────────────────────────────────────────────────── */
export function ModelFields({ draft, change, catalogs, available, load, settings }: {
  draft: ModelsDraft | null; change: (next: ModelsDraft) => void; catalogs: Partial<Record<AIProvider, CatalogState>>
  available: (provider: AIProvider) => boolean; load: (provider: AIProvider, refresh?: boolean) => Promise<void>; settings: AdminSettingsDTO
}) {
  if (!draft) return <div className="onb-fields"><p className="onb-quiet" role="status"><span className="spinner" aria-hidden="true" />Loading the models your keys can use…</p></div>
  const row = (label: string, help: string, picker: ReactNode) => <div className="onb-model-row"><span className="onb-model-label"><strong>{label}</strong><small>{help}</small></span>{picker}</div>
  // The picker is the admin panel's own, so it carries that panel's control styles.
  return <div className="onb-fields onb-models instance-settings">
    {row('Chat', 'Answers questions about your meetings', <ModelPicker value={draft.chat} kind="language" catalogs={catalogs} available={available} load={load} label="Chat model" change={model => change({ ...draft, chat: model as LanguageModelSelection })} />)}
    {row('Meeting notes', 'Writes the notes after a meeting', <ModelPicker value={draft.notes} kind="language" catalogs={catalogs} available={available} load={load} label="Meeting notes model" change={model => change({ ...draft, notes: model as LanguageModelSelection })} />)}
    {row('Speech to text', 'Turns recordings into transcripts', <ModelPicker value={draft.speech} kind="transcription" catalogs={catalogs} available={available} load={load} label="Speech-to-text model" change={model => change({ ...draft, speech: model as TranscriptionModelSelection })} />)}
    <div className="onb-model-row">
      <span className="onb-model-label"><strong>Live transcription</strong><small>Shows the words while a meeting is recorded</small></span>
      <div className="onb-live">
        <Switch checked={draft.live !== null} onChange={on => change({ ...draft, live: on ? settings.liveTranscriptionModel ?? defaultLiveModel : null })} label={draft.live ? 'On' : 'Off'} />
        {draft.live && <ModelPicker value={draft.live} kind="live_transcription" catalogs={catalogs} available={available} load={load} label="Live transcription model" change={model => change({ ...draft, live: model as LiveTranscriptionModelSelection })} />}
      </div>
    </div>
  </div>
}

/* ── 6. Who can sign in ──────────────────────────────────────────────────── */
export function SignInFields({ draft, change, saved }: { draft: SignInDraft; change: (next: SignInDraft) => void; saved: NonNullable<AdminSettingsDTO['signIn']> }) {
  const [entry, setEntry] = useState(''), [entryProblem, setEntryProblem] = useState<string | null>(null)
  const ready = googleClientReady(draft, saved.googleClientSecretSet)
  function add() {
    const result = addAllowedEmails(draft.allowedEmails, entry)
    setEntryProblem(result.problem)
    if (!result.problem) { change({ ...draft, allowedEmails: result.list }); setEntry('') }
  }
  return <div className="onb-fields">
    <div className="onb-switches">
      <Switch checked={draft.password} onChange={password => change({ ...draft, password })} label="Email and password" hint="Accounts you create for people" />
      <Switch checked={draft.google} onChange={google => change({ ...draft, google })} label="Google" hint={draft.google && !ready ? 'Needs the client ID and secret below' : 'People sign in with their Google account'} />
    </div>
    <div className="onb-fold" data-open={draft.google ? '' : undefined}><div className="onb-fold-body" inert={!draft.google}>
      <div className="onb-pair">
        <Field label="Google client ID" name="google-client-id" autoComplete="off" autoCapitalize="none" spellCheck={false} maxLength={255} placeholder="…apps.googleusercontent.com" value={draft.googleClientId} onChange={event => change({ ...draft, googleClientId: event.target.value })} />
        <Field label="Client secret" type="password" name="google-client-secret" autoComplete="off" maxLength={512} placeholder={saved.googleClientSecretSet ? 'Leave blank to keep it' : ''} value={draft.googleClientSecret} onChange={event => change({ ...draft, googleClientSecret: event.target.value })} />
      </div>
      <div className="onb-where">
        <p>Add both of these as authorized redirect URIs on the OAuth client in Google Cloud:</p>
        {saved.googleRedirectUris.map((uri, at) => <CopyLine key={uri} value={uri} label={`Copy the ${at === 0 ? 'sign-in' : 'calendar'} redirect address`} />)}
      </div>
      <div className="onb-field">
        <label htmlFor="onb-allowed">Who may join with Google</label>
        <div className="onb-add">
          <input id="onb-allowed" className="input" type="text" inputMode="email" autoComplete="off" autoCapitalize="none" spellCheck={false} placeholder="someone@example.com or @example.com" value={entry}
            aria-invalid={entryProblem ? true : undefined} aria-describedby={entryProblem ? 'onb-allowed-problem' : 'onb-allowed-hint'}
            onChange={event => { setEntry(event.target.value); setEntryProblem(null) }} onKeyDown={event => { if (event.key === 'Enter' && entry.trim()) { event.preventDefault(); add() } }} />
          <button type="button" className="button is-secondary pressable" onClick={add}>Add</button>
        </div>
        {draft.allowedEmails.length > 0 && <ul className="onb-chips" aria-label="Allowed emails and domains">{draft.allowedEmails.map(email => <li key={email}>
          <span>{email}</span><button type="button" aria-label={`Remove ${email}`} onClick={() => change({ ...draft, allowedEmails: draft.allowedEmails.filter(other => other !== email) })}><svg viewBox="0 0 10 10" width="8" height="8" aria-hidden="true"><path d="M2 2l6 6M8 2 2 8" /></svg></button>
        </li>)}</ul>}
        {entryProblem ? <p className="onb-problem" id="onb-allowed-problem" role="alert">{entryProblem}</p> : <p className="onb-hint" id="onb-allowed-hint">Only these addresses and domains can create an account with Google. People who already have an account can always sign in.</p>}
      </div>
    </div></div>
  </div>
}

/* ── 7. Where files live ─────────────────────────────────────────────────── */
export type StorageTest = { state: 'idle' } | { state: 'testing' } | { state: 'done'; result: StorageTestDTO | { ok: false; message: string } }
export function StorageFields({ draft, change, saved, test, runTest, problem }: {
  draft: StorageDraft; change: (next: Partial<StorageDraft>) => void; saved: NonNullable<AdminSettingsDTO['storage']>
  test: StorageTest; runTest: () => void; problem: string | null
}) {
  const s3 = draft.backend === 's3', ready = bucketReady(draft, saved.secretAccessKeySet)
  const text = (label: string, key: 'endpoint' | 'region' | 'bucket' | 'prefix' | 'accessKeyId', placeholder: string, maxLength: number) =>
    <Field label={label} name={`storage-${key}`} autoComplete="off" autoCapitalize="none" spellCheck={false} maxLength={maxLength} placeholder={placeholder} value={draft[key]} onChange={event => change({ [key]: event.target.value })} />
  return <div className="onb-fields">
    <div className="onb-choices" role="radiogroup" aria-label="Where files are kept">
      <Choice name="storage-backend" value="local" checked={!s3} onChange={() => change({ backend: 'local' })} title="This server’s disk" detail="Nothing to set up" />
      <Choice name="storage-backend" value="s3" checked={s3} onChange={() => change({ backend: 's3' })} title="S3-compatible bucket" detail="Amazon S3, R2, MinIO, B2" />
    </div>
    <div className="onb-fold" data-open={s3 ? '' : undefined}><div className="onb-fold-body" inert={!s3}>
      <div className="onb-pair">{text('Bucket', 'bucket', 'my-cereal-files', 63)}{text('Region', 'region', 'us-east-1', 40)}</div>
      {text('Endpoint', 'endpoint', 'Leave blank for Amazon S3', 2048)}
      <div className="onb-pair">
        {text('Access key ID', 'accessKeyId', '', 128)}
        <Field label="Secret access key" type="password" name="storage-secret" autoComplete="off" maxLength={512} placeholder={saved.secretAccessKeySet ? 'Leave blank to keep it' : ''} value={draft.secretAccessKey} onChange={event => change({ secretAccessKey: event.target.value })} />
      </div>
      <div className="onb-pair">{text('Folder prefix (optional)', 'prefix', 'cereal/', 200)}<div className="onb-field is-switch"><Switch checked={draft.forcePathStyle} onChange={forcePathStyle => change({ forcePathStyle })} label="Path-style addresses" hint="MinIO and most self-hosted stores" /></div></div>
    </div></div>
    <div className="onb-test">
      <button type="button" className="button is-secondary pressable" disabled={test.state === 'testing' || (s3 && (!ready || !!problem))} onClick={runTest}>{test.state === 'testing' && <span className="spinner" aria-hidden="true" />}Test connection</button>
      <p className={test.state === 'done' && !test.result.ok ? 'onb-problem' : 'onb-hint'} role="status" aria-live="polite">
        {test.state === 'testing' ? 'Writing, reading and deleting a small test file…'
          : test.state === 'done' ? <>{test.result.ok && <DrawnCheck size={14} />}{test.result.message}</>
          : problem ?? (s3 ? 'Tries these settings before anything is saved.' : 'Checks that the server can write to its disk.')}
      </p>
    </div>
  </div>
}

/* ── 8. Get the apps ─────────────────────────────────────────────────────── */
export type DownloadsState = { state: 'loading' } | { state: 'failed' } | { state: 'ready'; downloads: Downloads }
export function AppsFields({ downloads, address, retry }: { downloads: DownloadsState; address: string; retry: () => void }) {
  const ready = downloads.state === 'ready' ? downloads.downloads : null, mac = ready?.mac ?? null
  return <div className="onb-fields">
    <div className="onb-app">
      <span className="onb-label">Mac</span>
      {downloads.state === 'loading' && <p className="onb-quiet" role="status"><span className="spinner" aria-hidden="true" />Looking for the latest build…</p>}
      {downloads.state === 'failed' && <p className="onb-quiet" role="alert">The downloads could not be loaded. <button type="button" className="onb-link" onClick={retry}>Try again</button></p>}
      {ready && (mac
        ? <div className="onb-download"><a className="button is-secondary pressable" href={mac.installer.url} download={mac.installer.file}><svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true"><path d="M8 2.5v8M4.8 7.6 8 10.8l3.2-3.2M3 13.5h10" /></svg>Download for Mac</a><p className="onb-hint">{macDetails(ready.version, mac).join(' · ')}</p></div>
        : <p className="onb-quiet">No Mac build has been published on this server yet. When one is, it appears on the <a href="/download">download page</a>.</p>)}
    </div>
    <div className="onb-app">
      <span className="onb-label">Server address</span>
      <CopyLine value={address} label="Copy the server address" />
      <p className="onb-hint">Type this into the Cereal app the first time it opens.</p>
    </div>
    {ready?.iosAppStoreUrl && <div className="onb-app">
      <span className="onb-label">iPhone</span>
      <p className="onb-quiet">Cereal for iPhone is on the App Store. <a href={ready.iosAppStoreUrl} target="_blank" rel="noopener noreferrer">Get it there</a>, then enter the same address.</p>
    </div>}
  </div>
}
