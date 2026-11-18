/** How the Updates section moves from one layout to the next: the progress section appearing under the
 * button that started it, a sixth step joining the list, the log opening, the large picture relaxing into the
 * small summary.
 *
 * Each of those changes what is laid out. So that nothing jumps and no layout is animated, the change is made
 * inside a view transition: the browser draws the page before and after and moves between the two pictures
 * with transforms and opacity. Where there are no view transitions, or the reader asked for less motion, the
 * change is simply made. */
import { useEffect, useRef, useState } from 'react'
import { flushSync } from 'react-dom'
import { cssMilliseconds } from '../../lib/css-time'

export const reducedMotion = () => typeof window !== 'undefined' && typeof window.matchMedia === 'function' && window.matchMedia('(prefers-reduced-motion: reduce)').matches

/** What moves in a change, which decides the parts the browser follows one by one (admin.css, "View transitions").
 *  launch: the progress section appears, and "Update server" becomes its bowl.
 *  settle: the large picture and the small summary trade places.
 *  resize: the section grows or shrinks at its end (the log).
 *  extend: a sixth step joins the list. */
export type StageKind = 'launch' | 'settle' | 'resize' | 'extend'
type Transition = { finished: Promise<unknown> }
type Starter = (update: () => void) => Transition

/** Called just before a change is staged. The rest of the page (./settings-motion) finishes whatever it is still
 * moving, so the two pictures the browser draws are of a page at rest. */
export const beforeStage = new Set<() => void>()

let turn = 0
/** Makes `update` inside a view transition of that kind. Returns a function that gives the wait up. */
export function stage(kind: StageKind, update: () => void): () => void {
  const root = document.documentElement, start = (document as unknown as { startViewTransition?: Starter }).startViewTransition
  if (typeof start !== 'function' || reducedMotion()) { update(); return () => {} }
  let timer: number | undefined, cancelled = false
  const go = () => {
    if (cancelled) return
    for (const settle of beforeStage) settle()
    const mine = String(++turn)
    root.dataset.updateStage = kind; root.dataset.updateTurn = mine
    const clear = () => { if (root.dataset.updateTurn === mine) { delete root.dataset.updateStage; delete root.dataset.updateTurn } }
    try { start.call(document, () => flushSync(update)).finished.then(clear, clear) } catch { clear(); update() }
  }
  // A dialog that is closing (the confirmation) fades over the page for a moment: the page moves once it is gone,
  // or it would be drawn through the dialog's shade.
  const frame = requestAnimationFrame(() => {
    const closing = document.querySelector('.dialog-ghost, dialog[open]')
    if (!closing) { go(); return }
    timer = window.setTimeout(go, cssMilliseconds(getComputedStyle(root).getPropertyValue('--dur-gooey-exit'), 270) + 30)
  })
  return () => { cancelled = true; cancelAnimationFrame(frame); window.clearTimeout(timer) }
}

/** `value`, held back for the moment it takes to begin a view transition whenever `key` (what decides the
 * layout) changes. While the key is the same, the value is passed straight through. */
export function useStaged<T>(value: T, key: string, kind: (from: T, to: T) => StageKind): { shown: T; pending: boolean } {
  const [shownKey, setShownKey] = useState(key)
  const held = useRef(value), latest = useRef(value), choose = useRef(kind)
  latest.current = value; choose.current = kind
  if (shownKey === key) held.current = value
  useEffect(() => {
    if (shownKey === key) return
    return stage(choose.current(held.current, latest.current), () => setShownKey(key))
  }, [key, shownKey])
  return { shown: shownKey === key ? value : held.current, pending: shownKey !== key }
}

/** This page's clock, read again twice a second while `running`. */
export function useNow(running: boolean): number {
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    if (!running) return
    setNow(Date.now())
    const timer = window.setInterval(() => setNow(Date.now()), 500)
    return () => window.clearInterval(timer)
  }, [running])
  return now
}

/** The text that was shown before this one, for as long as it takes to leave. */
export function useLeaving(text: string): string | null {
  const [pair, setPair] = useState<{ now: string; before: string | null }>({ now: text, before: null })
  if (pair.now !== text) setPair({ now: text, before: pair.now })
  return pair.now === text ? pair.before : pair.now
}
