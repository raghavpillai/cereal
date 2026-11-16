import { useCallback, useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react'
import { AdminClientError } from '../../lib/client-error'

export function Notice({ children, error = false }: { children: ReactNode; error?: boolean }) {
  return <div className={`admin-notice${error ? ' is-error' : ''}`} role={error ? 'alert' : 'status'}>{children}</div>
}
export function Empty({ title, children }: { title: string; children: ReactNode }) {
  return <div className="admin-empty"><h3>{title}</h3><p>{children}</p></div>
}
export function Loading({ label }: { label: string }) {
  return <div className="admin-loading" role="status"><span className="admin-spinner" aria-hidden="true" />{label}</div>
}
export function Badge({ children, good = false }: { children: ReactNode; good?: boolean }) {
  return <span className={`admin-badge${good ? ' is-good' : ''}`}>{children}</span>
}
export function sourceLabel(source: 'environment' | 'stored' | 'none') {
  return source === 'stored' ? 'Saved override' : source === 'environment' ? 'Server environment' : 'Not configured'
}
export function timestamp(value: string | null) {
  return value ? new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short' }).format(new Date(value)) : 'Not yet synced'
}
function message(error: unknown) {
  return error instanceof AdminClientError ? error.message : 'The request could not finish. Please try again.'
}
export function useResource<T>(loader: (signal: AbortSignal) => Promise<T>, unauthorized: () => void) {
  const [state, setState] = useState<{ data: T | null; busy: boolean; error: string | null }>({ data: null, busy: true, error: null })
  const controller = useRef<AbortController | null>(null)
  const reload = useCallback(async () => {
    controller.current?.abort()
    const next = new AbortController(); controller.current = next
    setState(old => ({ ...old, busy: true, error: null }))
    try {
      const data = await loader(next.signal)
      if (!next.signal.aborted) setState({ data, busy: false, error: null })
    } catch (error) {
      if (next.signal.aborted) return
      if (error instanceof AdminClientError && error.status === 401) unauthorized()
      else setState(old => ({ ...old, busy: false, error: message(error) }))
    }
  }, [loader, unauthorized])
  useEffect(() => {
    setState({ data: null, busy: true, error: null }); void reload()
    return () => controller.current?.abort()
  }, [reload])
  return { ...state, reload }
}
type Outcome<T> = { ok: true; value: T } | { ok: false }
export function useAdminTask(unauthorized: () => void) {
  const [pending, setPending] = useState<ReadonlySet<string>>(new Set())
  const [error, setError] = useState<string | null>(null)
  const requests = useRef(new Map<string, AbortController>())
  const mounted = useRef(true)
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; requests.current.forEach(c => c.abort()); requests.current.clear() } }, [])
  const run = useCallback(async <T,>(key: string, operation: (signal: AbortSignal) => Promise<T>): Promise<Outcome<T>> => {
    if (requests.current.has(key)) return { ok: false }
    const controller = new AbortController(); requests.current.set(key, controller)
    setError(null); setPending(new Set(requests.current.keys()))
    try {
      const value = await operation(controller.signal)
      return controller.signal.aborted || !mounted.current ? { ok: false } : { ok: true, value }
    } catch (error) {
      if (!controller.signal.aborted && mounted.current) {
        if (error instanceof AdminClientError && error.status === 401) unauthorized()
        else setError(message(error))
      }
      return { ok: false }
    } finally {
      if (requests.current.get(key) === controller) requests.current.delete(key)
      if (mounted.current) setPending(new Set(requests.current.keys()))
    }
  }, [unauthorized])
  return { run, error, pending, clearError: () => setError(null) }
}

export function Modal({ title, busy, children, close }: { title: string; busy: boolean; children: ReactNode; close: () => void }) {
  const dialog = useRef<HTMLDialogElement>(null)
  const opener = useRef<HTMLElement | null>(null)
  useEffect(() => {
    // Callers unmount the dialog instead of closing it, so the browser never returns
    // focus to whatever opened it. Remember the opener and hand focus back once the dialog is gone.
    const element = dialog.current, active = document.activeElement
    if (active instanceof HTMLElement && !element?.contains(active)) opener.current = active
    element?.showModal()
    return () => { queueMicrotask(() => { if (!element?.isConnected && opener.current?.isConnected) opener.current.focus({ preventScroll: true }) }) }
  }, [])
  useLayoutEffect(() => { const element = dialog.current; return () => { if (element) fadeOutCopy(element) } }, [])
  return <dialog ref={dialog} className="admin-dialog" aria-label={title} onCancel={event => { event.preventDefault(); if (!busy) close() }}>
    <div className="admin-dialog-title"><h2>{title}</h2><button type="button" className="icon-button" aria-label={`Close ${title.toLowerCase()}`} disabled={busy} onClick={close}>×</button></div>
    {children}
  </dialog>
}

/** Callers unmount a dialog the moment it closes. A lifeless copy stays for the
 * length of the exit so the dialog and its backdrop fade away instead of
 * vanishing: shorter than the entrance, no bounce, opacity only when the
 * reader prefers reduced motion (the tokens collapse to a fade there). */
function fadeOutCopy(dialog: HTMLDialogElement) {
  if (typeof dialog.animate !== 'function' || !dialog.open) return
  const box = dialog.getBoundingClientRect(), copy = dialog.cloneNode(true) as HTMLDialogElement, home = dialog.parentElement
  queueMicrotask(() => {
    // Still attached means this was not a real unmount.
    if (dialog.isConnected || !home?.isConnected || !box.width) return
    const tokens = getComputedStyle(document.documentElement)
    const duration = parseFloat(tokens.getPropertyValue('--dur-gooey-exit')) || 200, from = parseFloat(tokens.getPropertyValue('--pop-from')) || 1
    const scrim = document.createElement('div')
    scrim.className = 'dialog-scrim-ghost'
    copy.classList.add('dialog-ghost')
    copy.inert = true
    copy.setAttribute('aria-hidden', 'true')
    Object.assign(copy.style, { left: `${box.left}px`, top: `${box.top}px`, width: `${box.width}px`, height: `${box.height}px` })
    home.append(scrim, copy) // beside where the dialog was, so scoped styles still reach the copy
    const options = (easing: string): KeyframeAnimationOptions => ({ duration, easing, fill: 'forwards' })
    const leave = (easing: string) => [scrim.animate({ opacity: [1, 0] }, options('ease-out')), copy.animate({ opacity: [1, 0], transform: ['none', `scale(${from + (1 - from) * 0.85})`] }, options(easing))]
    let animations: Animation[]
    try { animations = leave(tokens.getPropertyValue('--ease-exit').trim() || 'ease-out') } catch { animations = leave('ease-out') }
    const remove = () => { scrim.remove(); copy.remove() }
    Promise.all(animations.map(animation => animation.finished)).then(remove, remove)
  })
}
