import { useCallback, useEffect, useId, useRef, useState, type KeyboardEvent, type ReactNode } from 'react'

type Phase = 'closed' | 'open' | 'closing'

/** A floating card that grows out of the control that opened it, like the desktop
 * app's menus and its Share card: 12 px corners, a hairline edge and a soft shadow.
 * It closes on Escape and on a press outside, and Escape returns focus to the control.
 * In a menu, rows marked `menuitem` are reached with the arrow keys and Tab closes it. */
export function Popover({ label, button, buttonClassName, buttonLabel, className = '', menu = true, open: wanted, onOpenChange, children }: {
  /** Accessible name of the card */
  label: string
  button: ReactNode
  buttonClassName: string
  /** Accessible name of the control when its content is not text */
  buttonLabel?: string
  className?: string
  /** A list of actions (a menu) or a card with its own controls (a dialog). */
  menu?: boolean
  /** Set to open or close it from outside; the card still closes itself. */
  open?: boolean
  onOpenChange?: (open: boolean) => void
  children: (close: () => void) => ReactNode
}) {
  const [phase, setPhase] = useState<Phase>('closed')
  const root = useRef<HTMLDivElement>(null), trigger = useRef<HTMLButtonElement>(null), panel = useRef<HTMLDivElement>(null)
  const id = useId()
  const changed = useRef(onOpenChange); changed.current = onOpenChange
  const close = useCallback((restoreFocus = false) => {
    setPhase(current => current === 'open' ? 'closing' : current)
    if (restoreFocus) trigger.current?.focus()
  }, [])
  useEffect(() => { if (wanted === true) setPhase('open'); else if (wanted === false) setPhase(current => current === 'open' ? 'closing' : current) }, [wanted])
  useEffect(() => { if (phase !== 'closing') changed.current?.(phase === 'open') }, [phase])

  useEffect(() => {
    if (phase === 'closing') {
      // animationend is the normal path; the timer covers a card removed mid-flight.
      const timer = window.setTimeout(() => setPhase('closed'), 320)
      return () => window.clearTimeout(timer)
    }
    if (phase !== 'open') return
    if (menu) panel.current?.querySelector<HTMLElement>('[role="menuitem"]:not(:disabled)')?.focus({ preventScroll: true })
    else panel.current?.focus({ preventScroll: true })
    const outside = (event: PointerEvent) => { if (!root.current?.contains(event.target as Node)) close() }
    document.addEventListener('pointerdown', outside, true)
    return () => document.removeEventListener('pointerdown', outside, true)
  }, [phase, close, menu])

  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (phase !== 'open') return
    if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); close(true); return }
    if (!menu) return
    if (event.key === 'Tab') { close(); return }
    const items = [...(panel.current?.querySelectorAll<HTMLElement>('[role="menuitem"]:not(:disabled)') ?? [])]
    if (!items.length) return
    const at = items.indexOf(document.activeElement as HTMLElement)
    const next = event.key === 'ArrowDown' ? (at + 1) % items.length : event.key === 'ArrowUp' ? (at - 1 + items.length) % items.length
      : event.key === 'Home' ? 0 : event.key === 'End' ? items.length - 1 : -1
    if (next < 0) return
    event.preventDefault()
    items[next]!.focus()
  }

  return <div className="app-popover-root" ref={root} onKeyDown={onKeyDown}>
    <button ref={trigger} type="button" className={buttonClassName} aria-label={buttonLabel} aria-haspopup={menu ? 'menu' : 'dialog'} aria-expanded={phase === 'open'} aria-controls={phase === 'closed' ? undefined : id}
      onClick={() => setPhase(current => current === 'open' ? 'closing' : 'open')}>
      {button}
    </button>
    {phase !== 'closed' && <div ref={panel} id={id} className={`app-popover ${className}`.trim()} role={menu ? 'menu' : 'dialog'} aria-label={label} tabIndex={-1} data-state={phase}
      onAnimationEnd={event => { if (phase === 'closing' && event.target === event.currentTarget) setPhase('closed') }}>
      {children(() => close(true))}
    </div>}
  </div>
}
