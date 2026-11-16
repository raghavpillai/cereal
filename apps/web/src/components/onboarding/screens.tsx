/** The little screen beside the inputs: a small window that previews what each setup step configures and
 * reacts to what is typed. Everything is drawn with CSS and inline SVG in the app's own palette. */
import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties, type ReactNode, type Ref } from 'react'
import type { AIProvider } from '@meeting-notes/contracts'
import type { LanguageProvider, StepId } from '../../lib/onboarding'
import { Buddy, DrawnCheck, reducedMotion, type BuddyHandle, type Mood } from './kit'

export interface ScreenView {
  step: StepId
  /** The server's address without its scheme. */
  host: string
  instanceName: string
  instanceLogo: string | null
  claim: { accepted: boolean; /** The name to greet once the server is claimed. */ owner: string; method: 'password' | 'google'; email: string; /** The proving Google sign-in is under way. */ waiting: boolean }
  language: { provider: LanguageProvider; state: 'waiting' | 'checking' | 'connected' | 'failed' }
  speech: { state: 'waiting' | 'checking' | 'connected' | 'failed' }
  models: Array<{ role: string; logo: AIProvider | null; name: string; live?: boolean }>
  signIn: { password: boolean; google: boolean; allowed: readonly string[] }
  storage: { backend: 'local' | 's3'; bucket: string; test: 'idle' | 'testing' | 'ok' | 'failed' }
  apps: { mac: boolean; iphone: boolean }
}

const var_ = (values: Record<string, string | number>) => values as CSSProperties
const EXIT_MS = 270

export function LittleScreen({ view, mood, buddy }: { view: ScreenView; mood: Mood; buddy: Ref<BuddyHandle> }) {
  // The scene that was showing stays for the length of its exit, under the one arriving.
  const [scenes, setScenes] = useState<{ current: StepId; leaving: StepId | null }>({ current: view.step, leaving: null })
  if (scenes.current !== view.step) setScenes({ current: view.step, leaving: scenes.current })
  const { leaving } = scenes
  useEffect(() => {
    if (!leaving) return
    const timer = window.setTimeout(() => setScenes(now => ({ ...now, leaving: null })), EXIT_MS)
    return () => window.clearTimeout(timer)
  }, [leaving, view.step])
  // The picture is drawn 460 px wide and scaled to the window it is given.
  const frame = useRef<HTMLDivElement>(null), glass = useRef<HTMLDivElement>(null)
  useLayoutEffect(() => {
    const element = frame.current
    if (!element) return
    const fit = () => element.style.setProperty('--ls-scale', (element.clientWidth / 460).toFixed(4))
    fit()
    if (typeof ResizeObserver === 'undefined') return
    const observer = new ResizeObserver(fit); observer.observe(element)
    return () => observer.disconnect()
  }, [])
  // A step change gives the window itself a small squash, the way the desktop's surfaces take a change of content.
  const firstStep = useRef(true)
  useEffect(() => {
    if (firstStep.current) { firstStep.current = false; return }
    if (reducedMotion() || typeof glass.current?.animate !== 'function') return
    glass.current.animate([{ transform: 'scale(1, 1)' }, { transform: 'scale(1.014, 0.982)', offset: 0.28 }, { transform: 'scale(0.995, 1.007)', offset: 0.62 }, { transform: 'scale(1, 1)' }], { duration: 420, easing: 'ease-out' })
  }, [view.step])
  const title = windowTitle(view)
  return <div className="ls" data-step={view.step} aria-hidden="true" ref={frame}>
    <Buddy mood={mood} ref={buddy} />
    <div className="ls-window" ref={glass}><div className="ls-canvas">
      <div className="ls-bar"><span className="ls-lights"><i /><i /><i /></span><span className="ls-title swap-in" key={title}>{title}</span></div>
      <div className="ls-body">
        <span className="ls-blob" />
        {leaving && leaving !== view.step && <div className="ls-scene is-leaving" key={`leaving-${leaving}`}><Scene step={leaving} view={view} /></div>}
        <div className="ls-scene" key={view.step}><Scene step={view.step} view={view} /></div>
      </div>
    </div></div>
  </div>
}

function windowTitle(view: ScreenView): string {
  switch (view.step) {
    case 'claim': return view.host
    case 'name': case 'apps': return view.instanceName.trim() || 'Your space'
    case 'language': return 'Weekly sync'
    case 'speech': return 'Recording'
    case 'models': return 'Models'
    case 'signIn': return 'Sign in'
    case 'storage': return 'Files'
  }
}

function Scene({ step, view }: { step: StepId; view: ScreenView }) {
  switch (step) {
    case 'claim': return <ClaimScene view={view} />
    case 'name': return <NameScene view={view} />
    case 'language': return <NoteScene view={view} />
    case 'speech': return <SpeechScene view={view} />
    case 'models': return <ModelsScene view={view} />
    case 'signIn': return <SignInScene view={view} />
    case 'storage': return <StorageScene view={view} />
    case 'apps': return <AppsScene view={view} />
  }
}

const ProviderMark = ({ provider }: { provider: AIProvider }) => <img className="sc-provider" src={`/providers/${provider}.svg`} width="18" height="18" alt="" />
const Logo = ({ view, className = '' }: { view: ScreenView; className?: string }) =>
  <span className={`sc-logo ${className}`} key={view.instanceLogo ? 'logo' : 'mark'}>{view.instanceLogo ? <img src={view.instanceLogo} alt="" /> : <span className="mark" />}</span>

/* ── 1. Set up sign-in: the card people will sign in with, for the method chosen ── */
function ClaimScene({ view }: { view: ScreenView }) {
  const { accepted, owner, method, email, waiting } = view.claim
  const first = owner.trim().split(/\s+/)[0] ?? ''
  return <div className="sc sc-signin sc-claim" data-accepted={accepted ? '' : undefined} data-waiting={waiting ? '' : undefined}>
    <div className="sc-card">
      <span className="sc-card-head ls-extra"><Logo view={view} className="is-small" /><b>Welcome back.</b><small>Sign in to read your notes</small></span>
      <Fold open={method === 'google'}><span className="sc-google"><GoogleMark />Continue with Google{waiting && <i className="sc-google-wait" />}</span></Fold>
      <Fold open={method === 'password'}><span className="sc-input" data-filled={email ? '' : undefined}>{email || 'Email address'}</span><span className="sc-input">Password</span><span className="sc-submit">Sign in</span></Fold>
    </div>
    <p className="sc-welcome">{accepted ? <><DrawnCheck size={20} />{first ? `Welcome, ${first}.` : 'Welcome.'}</> : method === 'google' ? 'One button. No passwords here.' : 'You add people yourself.'}</p>
  </div>
}

/* ── 2. Name: the app's own header, as it is typed ─────────────────────── */
const NOTES: Array<[string, string]> = [['Weekly sync', '10:00 · 4 people'], ['Design review', '13:30 · 3 people'], ['Hiring catch-up', '15:00 · 2 people'], ['Customer call', '16:15 · 5 people'], ['Roadmap planning', '11:00 · 6 people']]
function NoteRow({ at, extra = false }: { at: number; extra?: boolean }) {
  const [title, meta] = NOTES[at]!
  return <div className={`sc-note-row${extra ? ' ls-extra' : ''}`} style={var_({ '--i': at })}>
    <span className="sc-note-tile"><svg viewBox="0 0 16 16" width="14" height="14" focusable="false"><path d="M4 2.5h5.5L12 5v8.5H4z M9.5 2.5V5H12 M6 8h4 M6 10.5h3" /></svg></span>
    <span className="sc-note-text"><b>{title}</b><small>{meta}</small></span>
  </div>
}
function NameScene({ view }: { view: ScreenView }) {
  const name = view.instanceName.trim()
  return <div className="sc sc-name">
    <div className="sc-app-head">
      <Logo view={view} />
      <span className="sc-app-name"><b data-empty={name ? undefined : ''}>{name || 'Your space'}</b><small>{view.host}</small></span>
    </div>
    <p className="sc-day">Today</p>
    <NoteRow at={0} /><NoteRow at={1} />
    <p className="sc-day ls-extra">Yesterday</p>
    <NoteRow at={2} extra /><NoteRow at={3} extra /><NoteRow at={4} extra />
  </div>
}

/* ── 3. Notes and chat: a note that writes itself once a key connects ──── */
const Status = ({ provider, state, labels }: { provider: AIProvider; state: 'waiting' | 'checking' | 'connected' | 'failed'; labels: Record<'waiting' | 'checking' | 'connected' | 'failed', string> }) =>
  <span className="sc-status" data-state={state}><ProviderMark provider={provider} /><span className="swap-in" key={state}>{labels[state]}</span>{state === 'connected' && <DrawnCheck size={14} />}</span>
const Line = ({ width, at }: { width: number; at: number }) => <span className="sc-line" style={var_({ '--w': `${width}%`, '--i': at })}><i /><b /></span>
function NoteScene({ view }: { view: ScreenView }) {
  const { provider, state } = view.language
  return <div className="sc sc-note" data-on={state === 'connected' ? '' : undefined}>
    <Status provider={provider} state={state} labels={{ waiting: 'Waiting for a key', checking: 'Checking the key…', connected: 'Connected', failed: 'Waiting for a key' }} />
    <p className="sc-note-title">Weekly sync<span className="sc-caret" /></p>
    <div className="sc-note-body">
      <p className="sc-heading" style={var_({ '--i': 0 })}>Decisions</p>
      <Line width={84} at={1} /><Line width={62} at={2} />
      <p className="sc-heading ls-extra" style={var_({ '--i': 3 })}>Action items</p>
      <span className="ls-extra sc-lines"><Line width={74} at={4} /><Line width={90} at={5} /><Line width={56} at={6} /></span>
    </div>
    <div className="sc-ask ls-extra" style={var_({ '--i': 8 })}><span>Ask anything about this meeting</span><i><svg viewBox="0 0 12 12" width="10" height="10" focusable="false"><path d="M6 10V2.5M2.8 5.5 6 2.3l3.2 3.2" /></svg></i></div>
  </div>
}

/* ── 4. Speech to text: a waveform that settles into transcript lines ──── */
const BARS = Array.from({ length: 30 }, (_, at) => 0.28 + 0.72 * Math.abs(Math.sin(at * 1.7) * Math.cos(at * 0.45 + 0.6)))
function Utterance({ who, own = false, widths, at, extra = false }: { who: string; own?: boolean; widths: number[]; at: number; extra?: boolean }) {
  return <div className={`sc-utter${own ? ' is-own' : ''}${extra ? ' ls-extra' : ''}`} style={var_({ '--i': at })}>
    <span className="sc-speaker"><i />{who}</span>
    <span className="sc-bubble">{widths.map((width, line) => <b key={line} style={var_({ '--w': `${width}%`, '--l': line })} />)}</span>
  </div>
}
function SpeechScene({ view }: { view: ScreenView }) {
  const { state } = view.speech
  return <div className="sc sc-speech" data-on={state === 'connected' ? '' : undefined}>
    <Status provider="elevenlabs" state={state} labels={{ waiting: 'Waiting for a key', checking: 'Checking the key…', connected: 'Connected', failed: 'Waiting for a key' }} />
    <div className="sc-wave">{BARS.map((height, at) => <i key={at} style={var_({ '--h': height.toFixed(2), '--i': at })} />)}</div>
    <div className="sc-transcript">
      <Utterance who="Speaker 1" widths={[92, 58]} at={0} />
      <Utterance who="Speaker 2" own widths={[74]} at={1} />
      <Utterance who="Speaker 1" widths={[84, 40]} at={2} extra />
      <Utterance who="Speaker 2" own widths={[66, 46]} at={3} extra />
    </div>
  </div>
}

/* ── 5. Models: one small card for each job ────────────────────────────── */
function ModelsScene({ view }: { view: ScreenView }) {
  return <div className="sc sc-models">
    <div className="sc-pipeline ls-extra"><span>Meeting</span><span>Transcript</span><span>Notes</span><span>Chat</span><i /></div>
    <div className="sc-model-grid">{view.models.map((card, at) => <div className="sc-model" key={card.role} style={var_({ '--i': at })} data-off={card.logo ? undefined : ''}>
      <small>{card.role}{card.live && <i className="sc-live" />}</small>
      <span className="sc-model-value swap-in" key={card.name}>{card.logo && <ProviderMark provider={card.logo} />}<b>{card.name}</b></span>
    </div>)}</div>
  </div>
}

/* ── 6. Sign-in: exactly the buttons people will see ───────────────────── */
function GoogleMark() {
  return <svg viewBox="0 0 18 18" width="13" height="13" focusable="false">
    <path fill="#4285F4" d="M17.64 9.2c0-.64-.06-1.25-.16-1.84H9v3.48h4.84a4.14 4.14 0 0 1-1.8 2.72v2.26h2.92c1.7-1.57 2.68-3.88 2.68-6.62Z" />
    <path fill="#34A853" d="M9 18c2.43 0 4.47-.8 5.96-2.18l-2.92-2.26c-.8.54-1.84.86-3.04.86-2.34 0-4.32-1.58-5.03-3.7H.96v2.33A9 9 0 0 0 9 18Z" />
    <path fill="#FBBC05" d="M3.97 10.72A5.4 5.4 0 0 1 3.68 9c0-.6.1-1.18.29-1.72V4.95H.96A9 9 0 0 0 0 9c0 1.45.35 2.83.96 4.05l3.01-2.33Z" />
    <path fill="#EA4335" d="M9 3.58c1.32 0 2.5.45 3.44 1.35l2.58-2.59C13.46.89 11.43 0 9 0A9 9 0 0 0 .96 4.95l3.01 2.33C4.68 5.16 6.66 3.58 9 3.58Z" />
  </svg>
}
const Fold = ({ open, children }: { open: boolean; children: ReactNode }) => <div className="sc-fold" data-open={open ? '' : undefined}><div>{children}</div></div>
function SignInScene({ view }: { view: ScreenView }) {
  const { password, google, allowed } = view.signIn
  return <div className="sc sc-signin">
    <div className="sc-card">
      <span className="sc-card-head ls-extra"><Logo view={view} className="is-small" /><b>Welcome back.</b><small>Sign in to {view.instanceName.trim() || 'Cereal'}</small></span>
      <Fold open={google}><span className="sc-google"><GoogleMark />Continue with Google</span></Fold>
      <Fold open={google && password}><span className="sc-or">or</span></Fold>
      <Fold open={password}><span className="sc-input">Email address</span><span className="sc-input">Password</span><span className="sc-submit">Sign in</span></Fold>
      <Fold open={!google && !password}><span className="sc-none">Nobody could sign in. Keep one way on.</span></Fold>
    </div>
    <div className="sc-allowed ls-extra" data-shown={google && allowed.length ? '' : undefined}>
      <small>Can join with Google</small>
      <span>{allowed.slice(0, 3).map(entry => <i key={entry} className="swap-in">{entry}</i>)}{allowed.length > 3 && <i>+{allowed.length - 3}</i>}</span>
    </div>
  </div>
}

/* ── 7. Storage: audio files hopping into a disk or a bucket ───────────── */
const AudioFile = ({ at }: { at: number }) => <span className="sc-file" style={var_({ '--i': at })}><span className="sc-file-hop"><span className="sc-file-card">
  <svg viewBox="0 0 22 14" width="22" height="14" focusable="false"><path d="M2 7v0M5 4.5v5M8 2.5v9M11 5v4M14 3.5v7M17 5.5v3M20 7v0" /></svg>
</span></span></span>
function StorageScene({ view }: { view: ScreenView }) {
  const { backend, bucket, test } = view.storage
  return <div className="sc sc-storage" data-backend={backend} data-test={test}>
    <div className="sc-stage">
      <span className="sc-shelf"><i /><i /><i /></span>
      <AudioFile at={0} /><AudioFile at={1} /><AudioFile at={2} />
      <span className="sc-target">
        <svg className="sc-disk" viewBox="0 0 120 96" width="120" height="96" focusable="false">
          <rect className="sc-target-body" x="6" y="22" width="108" height="62" rx="14" />
          <path className="sc-target-line" d="M6 58h108" />
          <circle className="sc-target-led" cx="24" cy="71" r="3.2" />
          <path className="sc-target-line" d="M78 71h22M60 71h10" />
          <path className="sc-target-slot" d="M36 22h48" />
        </svg>
        <svg className="sc-bucket" viewBox="0 0 120 96" width="120" height="96" focusable="false">
          <path className="sc-target-body" d="M18 30h84l-9 50a8 8 0 0 1-8 6H35a8 8 0 0 1-8-6z" />
          <ellipse className="sc-target-rim" cx="60" cy="30" rx="44" ry="9" />
          <path className="sc-target-line" d="M24 52c12 5 60 5 72 0" />
        </svg>
        <span className="sc-target-check"><DrawnCheck size={22} /></span>
      </span>
    </div>
    <p className="sc-target-label swap-in" key={backend}>{backend === 'local' ? 'This server’s disk' : bucket.trim() || 'your-bucket'}</p>
    <div className="sc-stored ls-extra">
      {['weekly-sync.m4a', 'design-review.m4a'].map((file, at) => <span key={file} style={var_({ '--i': at })}><i /><b>{file}</b><small>{at ? '31 MB' : '18 MB'}</small></span>)}
    </div>
  </div>
}

/* ── 8. Apps: a desktop and a phone, both showing this instance ────────── */
function AppsScene({ view }: { view: ScreenView }) {
  const name = view.instanceName.trim() || 'Your space'
  return <div className="sc sc-apps">
    <div className="sc-devices">
      <div className="sc-mac" data-ready={view.apps.mac ? '' : undefined}>
        <div className="sc-mac-screen">
          <span className="sc-mini-head"><Logo view={view} className="is-tiny" /><b>{name}</b></span>
          <span className="sc-mini-row" style={var_({ '--w': '78%' })} /><span className="sc-mini-row" style={var_({ '--w': '56%' })} /><span className="sc-mini-row" style={var_({ '--w': '68%' })} />
        </div>
        <span className="sc-mac-foot" />
      </div>
      <div className="sc-phone">
        <span className="sc-phone-island" />
        <span className="sc-mini-head is-phone"><Logo view={view} className="is-tiny" /><b>{name}</b></span>
        <span className="sc-mini-row" style={var_({ '--w': '82%' })} /><span className="sc-mini-row" style={var_({ '--w': '60%' })} />
        <span className="sc-phone-record"><i /></span>
      </div>
    </div>
    <p className="sc-address ls-extra"><i />{view.host}</p>
  </div>
}
