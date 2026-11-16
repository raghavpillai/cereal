/** The small parts the setup flow is built from: the Cereal mark as a character, the gooey step dots,
 * fields, a switch, a line to copy, and the check that draws itself. */
import { forwardRef, useEffect, useId, useImperativeHandle, useRef, useState, type InputHTMLAttributes, type ReactNode } from 'react'
import { copyText } from '../../lib/clipboard'
import markSource from '../../assets/mark.svg?raw'

const MARK_PATH = /\sd="([^"]+)"/.exec(markSource)?.[1] ?? ''
export const reducedMotion = () => typeof window !== 'undefined' && window.matchMedia('(prefers-reduced-motion: reduce)').matches

/* ── The character ───────────────────────────────────────────────────────
   The mark with two eyes: its ring is the body and the hole in it the mouth. It sits on top of the little screen, blinks,
   hops when something works and wobbles a little when something does not. */
export type Mood = 'asleep' | 'idle' | 'happy'
export interface BuddyHandle { hop: () => void; wobble: () => void; nod: () => void }

export const Buddy = forwardRef<BuddyHandle, { mood: Mood }>(function Buddy({ mood }, ref) {
  const body = useRef<HTMLSpanElement>(null)
  useImperativeHandle(ref, () => {
    const play = (frames: Keyframe[], duration: number) => { if (!reducedMotion() && typeof body.current?.animate === 'function') body.current.animate(frames, { duration, easing: 'linear' }) }
    return {
      // Up on a stretch, down into a squash, then the spring settles it: the desktop pill's landing.
      hop: () => play([
        { transform: 'translateY(0) scale(1, 1)', offset: 0 },
        { transform: 'translateY(1px) scale(1.12, 0.86)', offset: 0.12, easing: 'cubic-bezier(0.2, 0.9, 0.3, 1)' },
        { transform: 'translateY(-17px) scale(0.94, 1.1)', offset: 0.42, easing: 'cubic-bezier(0.5, 0, 0.9, 0.6)' },
        { transform: 'translateY(0) scale(1.14, 0.84)', offset: 0.66, easing: 'cubic-bezier(0.2, 0.9, 0.3, 1)' },
        { transform: 'translateY(-3px) scale(0.98, 1.04)', offset: 0.82 },
        { transform: 'translateY(0) scale(1, 1)', offset: 1 },
      ], 640),
      wobble: () => play([
        { transform: 'rotate(0deg)', offset: 0 }, { transform: 'rotate(-9deg)', offset: 0.18 }, { transform: 'rotate(7deg)', offset: 0.4 },
        { transform: 'rotate(-4deg)', offset: 0.62 }, { transform: 'rotate(2deg)', offset: 0.82 }, { transform: 'rotate(0deg)', offset: 1 },
      ], 560),
      nod: () => play([
        { transform: 'translateY(0) scale(1, 1)', offset: 0 }, { transform: 'translateY(2px) scale(1.06, 0.92)', offset: 0.35 },
        { transform: 'translateY(-1px) scale(0.99, 1.02)', offset: 0.7 }, { transform: 'translateY(0) scale(1, 1)', offset: 1 },
      ], 360),
    }
  }, [])
  return <span className="onb-buddy" data-mood={mood} aria-hidden="true">
    <span className="onb-buddy-body" ref={body}>
      <svg viewBox="0 0 24 24" width="48" height="48" focusable="false">
        <path className="onb-buddy-ring" fillRule="evenodd" d={MARK_PATH} />
        {/* Two eyes on the ring; the ring's own hole is the mouth, and a lid over its top half turns it into a smile. */}
        <path className="onb-buddy-lid" d="M6.5 12.3A4.5 4.5 0 0 1 15.5 12.3z" />
        <g className="onb-buddy-face">
          <ellipse className="onb-buddy-eye" cx="8.5" cy="5.6" rx="0.85" ry="1.05" />
          <ellipse className="onb-buddy-eye" cx="12" cy="5.3" rx="0.85" ry="1.05" />
        </g>
      </svg>
    </span>
  </span>
})

/* ── Step dots ───────────────────────────────────────────────────────────
   One gold blob marks the current step. Moving on, its leading edge runs ahead on the snappy spring and
   the trailing edge lets go a moment later on the gooey one (the desktop's stretch and catch-up), leaving
   a small gold dot behind; a goo filter joins the two while they are close, so the blob visibly detaches. */
export function StepDots({ titles, index }: { titles: readonly string[]; index: number }) {
  const filter = useId().replace(/:/g, '')
  const last = useRef(index)
  const [direction, setDirection] = useState<'forward' | 'backward'>('forward')
  useEffect(() => { if (index !== last.current) { setDirection(index > last.current ? 'forward' : 'backward'); last.current = index } }, [index])
  // The direction has to be known in the same render that moves the blob.
  const moving = index === last.current ? direction : index > last.current ? 'forward' : 'backward'
  return <div className="onb-dots" role="progressbar" aria-label="Setup progress" aria-valuemin={1} aria-valuemax={titles.length} aria-valuenow={index + 1}
    aria-valuetext={`Step ${index + 1} of ${titles.length}: ${titles[index]}`} data-dir={moving} style={{ '--onb-steps': titles.length, '--onb-at': index } as React.CSSProperties}>
    <svg className="onb-dots-filter" width="0" height="0" aria-hidden="true" focusable="false"><defs>
      <filter id={filter} x="-10%" y="-150%" width="120%" height="400%" colorInterpolationFilters="sRGB">
        <feGaussianBlur in="SourceGraphic" stdDeviation="2.2" result="blur" />
        <feColorMatrix in="blur" mode="matrix" values="1 0 0 0 0  0 1 0 0 0  0 0 1 0 0  0 0 0 22 -9" />
      </filter>
    </defs></svg>
    <span className="onb-dots-track" aria-hidden="true">{titles.map((title, at) => <i key={title} style={{ '--onb-dot': at } as React.CSSProperties} />)}</span>
    <span className="onb-dots-goo" aria-hidden="true" style={{ filter: `url(#${filter})` }}>
      {titles.map((title, at) => <i key={title} className="onb-dot-done" data-on={at < index ? '' : undefined} style={{ '--onb-dot': at } as React.CSSProperties} />)}
      <i className="onb-blob" />
    </span>
  </div>
}

/* ── Fields ──────────────────────────────────────────────────────────────── */
export type FieldState = 'idle' | 'checking' | 'good' | 'bad'
/** A labelled input. `state` draws what a live check found inside the field: a spinner, or a check that draws
 * itself while the border turns gold. `problem` is said under the field and tied to it for screen readers. */
export function Field({ label, hint, problem, state = 'idle', trailing, className, id: given, ...input }: {
  label: string; hint?: ReactNode; problem?: string | null; state?: FieldState; trailing?: ReactNode
} & InputHTMLAttributes<HTMLInputElement>) {
  const generated = useId(), id = given ?? generated
  const describedBy = [problem ? `${id}-problem` : null, hint ? `${id}-hint` : null].filter(Boolean).join(' ') || undefined
  return <div className={`onb-field${className ? ` ${className}` : ''}`} data-state={problem && state === 'idle' ? 'bad' : state}>
    <label htmlFor={id}>{label}</label>
    <span className="onb-field-box">
      <input id={id} className="input" aria-invalid={problem ? true : undefined} aria-describedby={describedBy} {...input} />
      <span className="onb-field-ring" aria-hidden="true" />
      {state === 'checking' && <span className="onb-field-mark" aria-hidden="true"><span className="spinner" /></span>}
      {state === 'good' && <span className="onb-field-mark" aria-hidden="true"><DrawnCheck /></span>}
      {trailing}
    </span>
    {problem ? <p className="onb-problem" id={`${id}-problem`} role="alert">{problem}</p> : hint ? <p className="onb-hint" id={`${id}-hint`}>{hint}</p> : null}
  </div>
}

/** A check in a gold disc: the disc pops, then the stroke draws itself. */
export function DrawnCheck({ size = 20 }: { size?: number }) {
  return <svg className="onb-check" viewBox="0 0 20 20" width={size} height={size} aria-hidden="true" focusable="false">
    <circle cx="10" cy="10" r="10" />
    <path d="M5.6 10.4 8.7 13.4 14.5 7.2" pathLength={1} />
  </svg>
}

export function Switch({ checked, onChange, label, hint, disabled }: { checked: boolean; onChange: (next: boolean) => void; label: string; hint?: ReactNode; disabled?: boolean }) {
  const id = useId()
  return <div className="onb-switch-row">
    <span className="onb-switch-text"><span id={`${id}-label`}>{label}</span>{hint && <small id={`${id}-hint`}>{hint}</small>}</span>
    <button type="button" role="switch" className="onb-switch" aria-checked={checked} aria-labelledby={`${id}-label`} aria-describedby={hint ? `${id}-hint` : undefined} disabled={disabled} onClick={() => onChange(!checked)}><span /></button>
  </div>
}

/** A line of text to copy: a command, an address. Says "Copied" for two seconds, and is announced. */
export function CopyLine({ value, label, prefix }: { value: string; label: string; prefix?: string }) {
  const [copied, setCopied] = useState(false)
  const timer = useRef<number | undefined>(undefined)
  useEffect(() => () => window.clearTimeout(timer.current), [])
  async function copy() {
    // Works over plain HTTP too, where the clipboard API does not exist (lib/clipboard.ts).
    try { await copyText(value) } catch { return }
    setCopied(true); window.clearTimeout(timer.current); timer.current = window.setTimeout(() => setCopied(false), 2000)
  }
  return <div className="onb-copy">
    <code>{prefix && <span aria-hidden="true">{prefix}</span>}{value}</code>
    <button type="button" className="onb-copy-button pressable" aria-label={label} onClick={() => void copy()}>
      <span className="onb-copy-label" data-shown={copied ? undefined : ''}>Copy</span>
      <span className="onb-copy-label" data-shown={copied ? '' : undefined}>Copied</span>
    </button>
    <span className="sr-only" role="status">{copied ? 'Copied' : ''}</span>
  </div>
}

/** One of a few choices, as a card: a real radio button underneath, so arrows and Space work. */
export function Choice({ name, value, checked, onChange, title, detail, icon, disabled }: { name: string; value: string; checked: boolean; onChange: () => void; title: string; detail?: string; icon?: ReactNode; disabled?: boolean }) {
  return <label className="onb-choice pressable" data-checked={checked ? '' : undefined} data-disabled={disabled ? '' : undefined}>
    <input type="radio" className="sr-only" name={name} value={value} checked={checked} disabled={disabled} onChange={onChange} />
    {icon && <span className="onb-choice-icon" aria-hidden="true">{icon}</span>}
    <span className="onb-choice-text"><strong>{title}</strong>{detail && <small>{detail}</small>}</span>
    <span className="onb-choice-dot" aria-hidden="true" />
  </label>
}
