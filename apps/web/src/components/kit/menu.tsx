import { useCallback, useEffect, useId, useLayoutEffect, useRef, useState, type KeyboardEvent, type ReactNode } from 'react'

type Phase = 'closed' | 'open' | 'closing'

/** A menu that grows from its trigger and collapses back into it. The exit is
 * kept mounted until its (shorter, bounce-free) animation ends. Focus moves
 * into the menu on open and returns to the trigger on close. */
export function Menu({ label, align = 'start', buttonClassName, buttonLabel, button, disabled = false, children }: {
  /** Accessible name of the menu itself */
  label: string
  align?: 'start' | 'end'
  buttonClassName: string
  /** Accessible name of the trigger when its content is not text */
  buttonLabel?: string
  button: ReactNode
  disabled?: boolean
  children: (close: () => void) => ReactNode
}) {
  const [phase, setPhase] = useState<Phase>('closed')
  // The side the panel hangs from. It starts as asked and flips if that would put it off-screen.
  const [side, setSide] = useState(align)
  const root = useRef<HTMLDivElement>(null), trigger = useRef<HTMLButtonElement>(null), panel = useRef<HTMLDivElement>(null)
  const id = useId()
  const close = useCallback((restoreFocus = false) => {
    setPhase(current => current === 'open' ? 'closing' : current)
    if (restoreFocus) trigger.current?.focus()
  }, [])

  useLayoutEffect(() => {
    if (phase === 'closed') { setSide(align); return }
    if (phase !== 'open' || !panel.current || !trigger.current) return
    // Measured before the first paint, so the panel grows from the right corner of its trigger.
    const width = panel.current.offsetWidth, anchor = trigger.current.getBoundingClientRect(), margin = 8
    const fitsEnd = anchor.right - width >= margin, fitsStart = anchor.left + width <= window.innerWidth - margin
    setSide(align === 'end' ? (fitsEnd || !fitsStart ? 'end' : 'start') : (fitsStart || !fitsEnd ? 'start' : 'end'))
  }, [phase, align])

  useEffect(() => {
    if (phase === 'closing') {
      // animationend is the normal path; the timer covers a menu removed mid-flight.
      const timer = window.setTimeout(() => setPhase('closed'), 400)
      return () => window.clearTimeout(timer)
    }
    if (phase !== 'open') return
    const items = panel.current?.querySelectorAll<HTMLElement>('[role^="menuitem"]:not(:disabled)')
    const start = [...(items ?? [])].find(item => item.getAttribute('aria-checked') === 'true') ?? items?.[0]
    start?.focus({ preventScroll: true })
    const outside = (event: PointerEvent) => { if (!root.current?.contains(event.target as Node)) close() }
    document.addEventListener('pointerdown', outside, true)
    return () => document.removeEventListener('pointerdown', outside, true)
  }, [phase, close])

  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (phase !== 'open') return
    if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); close(true); return }
    if (event.key === 'Tab') { close(); return }
    const items = [...(panel.current?.querySelectorAll<HTMLElement>('[role^="menuitem"]:not(:disabled)') ?? [])]
    if (!items.length) return
    const at = items.indexOf(document.activeElement as HTMLElement)
    const next = event.key === 'ArrowDown' ? (at + 1) % items.length : event.key === 'ArrowUp' ? (at - 1 + items.length) % items.length
      : event.key === 'Home' ? 0 : event.key === 'End' ? items.length - 1 : -1
    if (next < 0) return
    event.preventDefault()
    items[next]!.focus()
  }

  return <div className="menu-root" ref={root} onKeyDown={onKeyDown}>
    <button ref={trigger} type="button" className={buttonClassName} aria-label={buttonLabel} aria-haspopup="menu" aria-expanded={phase === 'open'} aria-controls={phase === 'closed' ? undefined : id}
      disabled={disabled}
      onClick={() => setPhase(current => current === 'open' ? 'closing' : 'open')}
      onKeyDown={event => { if (event.key === 'ArrowDown' && phase !== 'open') { event.preventDefault(); setPhase('open') } }}>
      {button}
    </button>
    {phase !== 'closed' && <div ref={panel} id={id} className="menu" role="menu" aria-label={label} data-align={side} data-state={phase}
      onAnimationEnd={event => { if (phase === 'closing' && event.target === event.currentTarget) setPhase('closed') }}>
      {children(() => close(true))}
    </div>}
  </div>
}

export function MenuItem({ children, icon, onSelect, checked, danger = false, disabled = false }: {
  children: ReactNode
  icon?: ReactNode
  onSelect: () => void
  /** Present for a single-choice item; shows a check that pops in. */
  checked?: boolean
  danger?: boolean
  disabled?: boolean
}) {
  return <button type="button" className={`menu-item${danger ? ' is-danger' : ''}`} role={checked === undefined ? 'menuitem' : 'menuitemradio'} aria-checked={checked} disabled={disabled} tabIndex={-1} onClick={onSelect}>
    {icon}<span>{children}</span>{checked !== undefined && <CheckMark />}
  </button>
}

function CheckMark() {
  return <svg className="menu-check" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="m3.5 8.5 3 3 6-7" /></svg>
}
