/** How the Instance settings page moves when what it shows changes.
 *
 * A section is one card. Choosing another section keeps that card's frame and changes what is in it: the frame
 * goes from the old section's height to the new one's on a spring, what was in it leaves the way the list was
 * travelled, and the new rows arrive from the other side, one after another. Everything under the card (the save
 * row, the footer) is carried by the layout as the frame changes, so nothing there jumps.
 *
 * Rows in a list (models, allowed emails, administrators, providers) travel to where they belong when the list
 * changes, a row that is added settles in, and one that is removed fades where it was.
 *
 * Nothing here waits for an animation: the page is changed first and is usable at once, and the motion is laid
 * over it. A change made while another is moving starts from where things are. When the reader asked for less
 * motion nothing travels: a section change is one short fade, and the rest is simply made. */
import { useEffect, useLayoutEffect, useRef, type RefObject } from 'react'
import { cssMilliseconds } from '../../lib/css-time'
import { beforeStage, reducedMotion } from './update-motion'

// ── What can be decided without a page ─────────────────────────────────────────────────────────────────────

/** Which way the list of sections was travelled: 1 towards its end, -1 towards its start, 0 when nothing moves
 * (the same section, or one the list does not have). */
export function sectionDirection(ids: readonly string[], from: string, to: string): -1 | 0 | 1 {
  const a = ids.indexOf(from), b = ids.indexOf(to)
  return a < 0 || b < 0 || a === b ? 0 : b > a ? 1 : -1
}

/** A `linear()` easing (the design tokens' springs) as points of [time, value], both from 0 to 1. Stops written
 * without a time are spread evenly between their neighbours, as CSS does. Null when the text is not a `linear()`. */
export function easingPoints(token: string): Array<[number, number]> | null {
  const match = /^\s*linear\((.*)\)\s*$/s.exec(token)
  if (!match) return null
  const stops = match[1]!.split(',').map(part => part.trim().split(/\s+/))
  if (stops.length < 2) return null
  const values: number[] = [], times: Array<number | null> = []
  for (const stop of stops) {
    const value = Number(stop[0]), time = stop[1] === undefined ? null : /^-?[\d.]+%$/.test(stop[1]) ? Number(stop[1].slice(0, -1)) / 100 : NaN
    if (!Number.isFinite(value) || Number.isNaN(time)) return null
    values.push(value); times.push(time)
  }
  times[0] ??= 0; times[times.length - 1] ??= 1
  for (let index = 1; index < times.length - 1; index++) {
    if (times[index] !== null) continue
    let next = index + 1
    while (times[next] === null) next++
    const before = times[index - 1]!, step = (times[next]! - before) / (next - index + 1)
    for (let fill = index; fill < next; fill++) times[fill] = before + step * (fill - index + 1)
  }
  return values.map((value, index) => [times[index]!, value])
}

/** The value of such an easing at `progress` (0 to 1). */
export function easingAt(points: ReadonlyArray<readonly [number, number]>, progress: number): number {
  if (!points.length) return progress
  if (progress <= points[0]![0]) return points[0]![1]
  for (let index = 1; index < points.length; index++) {
    const [time, value] = points[index]!, [before, from] = points[index - 1]!
    if (progress <= time) return time === before ? value : from + (value - from) * (progress - before) / (time - before)
  }
  return points[points.length - 1]![1]
}

/** How long a row waits for the rows before it. A long section never takes long: the wait stops growing. */
export const staggerDelay = (index: number, step: number, cap = 7): number => Math.max(0, Math.min(index, cap)) * step

export interface Box { left: number; top: number; width: number; height: number }
/** Where a row has to start so that it is seen where it was: its old place, and whatever it was still travelling. */
export const travel = (was: Box, now: Box, moving: { x: number; y: number } = { x: 0, y: 0 }) => ({ x: was.left + moving.x - now.left, y: was.top + moving.y - now.top })

/** What the save row says. One of these at a time, so the row can move from one to the next. */
export type SaveState = 'saving' | 'problem' | 'unsaved' | 'saved'
export const saveState = (busy: boolean, problem: string | null, dirty: boolean): SaveState => busy ? 'saving' : problem ? 'problem' : dirty ? 'unsaved' : 'saved'
export const saveLine = (state: SaveState, problem: string | null): string => state === 'saving' ? 'Saving changes…' : state === 'problem' ? problem ?? '' : state === 'unsaved' ? 'You have unsaved changes.' : 'All changes saved'

// ── The page ───────────────────────────────────────────────────────────────────────────────────────────────

const canAnimate = () => typeof Element !== 'undefined' && typeof Element.prototype.animate === 'function'
/** The Updates section changes its own layout inside a view transition; nothing here moves while it does. */
const staging = () => document.documentElement.dataset.updateStage !== undefined

interface Tokens { settle: string; snappy: string; exit: string; fade: string; points: Array<[number, number]> | null; morph: number; rows: number; fadeIn: number; leave: number; step: number; rise: number }
/** The design tokens, read when they are needed: a theme or a reader's preference can change them. */
function tokens(): Tokens {
  const css = getComputedStyle(document.documentElement), read = (name: string) => css.getPropertyValue(name).trim()
  const supported = (value: string, fallback: string) => value && typeof CSS !== 'undefined' && CSS.supports('transition-timing-function', value) ? value : fallback
  const settle = read('--spring-settle')
  return {
    settle: supported(settle, 'cubic-bezier(0.2, 0.9, 0.3, 1)'), snappy: supported(read('--spring-snappy'), 'cubic-bezier(0.2, 0.9, 0.3, 1)'),
    exit: supported(read('--ease-exit'), 'cubic-bezier(0.3, 0.7, 0.4, 1)'), fade: supported(read('--ease-fade'), 'ease-out'), points: easingPoints(settle),
    morph: cssMilliseconds(read('--dur-gooey'), 420), rows: cssMilliseconds(read('--dur-gooey'), 420), fadeIn: cssMilliseconds(read('--dur-snappy-exit'), 180),
    leave: cssMilliseconds(read('--dur-snappy-exit'), 180) * 0.6, step: cssMilliseconds(read('--stagger-step'), 22), rise: parseFloat(read('--rise')) || 7,
  }
}

/** The parts of a card that arrive one after another: its title, then each row of its body. A body part that is
 * itself a list of rows (the providers, the two identity fields, the administrators) is taken row by row. */
const GROUPS = '.provider-list, .identity-fields, .administrators-list, .admin-google-only, .reveal, .reveal-inner'
export function rowsOf(card: Element): HTMLElement[] {
  const rows: HTMLElement[] = []
  const add = (element: Element, depth: number) => {
    if (!(element instanceof HTMLElement) || element.dataset.flowGhost !== undefined || element.tagName === 'DIALOG') return
    if (depth < 4 && element.matches(GROUPS) && element.children.length) { for (const child of element.children) add(child, depth + 1); return }
    rows.push(element)
  }
  for (const part of card.children) {
    if (part instanceof HTMLElement && part.dataset.slot === 'card-content') for (const child of part.children) add(child, 0)
    else add(part, 0)
  }
  return rows
}

/** Along which line the sections are listed: down the page beside the card, or (on a phone) across it, above the card. */
function axisOf(pane: HTMLElement): 'x' | 'y' {
  const items = pane.parentElement?.querySelectorAll<HTMLElement>('.settings-nav > a')
  return items && items.length > 1 && Math.abs(items[0]!.offsetTop - items[1]!.offsetTop) < 4 ? 'x' : 'y'
}

/** The card's own height, whatever height it is being shown at. */
function naturalHeight(card: HTMLElement): number {
  const last = card.lastElementChild, style = getComputedStyle(card)
  if (!last) return card.offsetHeight
  return last.getBoundingClientRect().bottom - card.getBoundingClientRect().top + (parseFloat(style.borderBottomWidth) || 0) + (parseFloat(style.paddingBottom) || 0)
}

const morphing = new WeakSet<Element>()
const controllers = new Set<{ settle: () => void }>()
// A layout change of the Updates section is drawn from two pictures of the page: whatever is still moving here is finished first.
beforeStage.add(() => { for (const controller of controllers) controller.settle() })

export interface SectionMotion {
  /** Makes `update` (which shows another section) and moves from the one that was shown. `direction` is how the list was travelled. */
  change: (direction: -1 | 0 | 1, update: () => void) => void
  /** The first section's rows settle in, after the heading and the list of sections (`after`, in milliseconds). */
  arrive: (after: number) => void
  settle: () => void
  dispose: () => void
}

/** The motion of one settings page. `pane` holds `.settings-stage`, which holds the section. */
export function createSectionMotion(pane: HTMLElement): SectionMotion {
  type Morph = { card: HTMLElement; animation: Animation; from: number; to: number; started: number; duration: number; watch: ResizeObserver | null }
  let morph: Morph | null = null, entering: Animation[] = [], enteringRows: HTMLElement[] = []
  let shown: HTMLElement | null = null, height = 0, disposed = false
  const ghosts = new Set<HTMLElement>()
  const stage = () => pane.querySelector<HTMLElement>(':scope > .settings-stage')
  const primary = () => stage()?.querySelector<HTMLElement>('[data-slot=card]') ?? null

  // The card's height as it stands, kept for the moment the card is replaced by another (a section that has loaded).
  const sizes = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(() => { if (!morph && shown?.isConnected) height = shown.offsetHeight })
  const follow = (card: HTMLElement | null) => {
    if (card === shown) return
    sizes?.disconnect(); shown = card
    if (card) { sizes?.observe(card); height = card.offsetHeight }
  }

  const endMorph = () => { if (!morph) return; morph.watch?.disconnect(); morphing.delete(morph.card); morph = null; if (shown?.isConnected) height = shown.offsetHeight }
  const stopMorph = () => { const running = morph; if (!running) return; endMorph(); try { running.animation.cancel() } catch { /* already over */ } }
  /** How tall the frame is drawn right now. */
  const frameHeight = (t: Tokens): number => {
    if (!morph) return height
    if (morph.card.isConnected) return morph.card.getBoundingClientRect().height
    const progress = Math.min(1, Math.max(0, (performance.now() - morph.started) / morph.duration))
    return morph.from + (morph.to - morph.from) * (t.points ? easingAt(t.points, progress) : 1 - (1 - progress) ** 3)
  }

  function startMorph(card: HTMLElement, from: number, t: Tokens) {
    const to = naturalHeight(card)
    if (Math.abs(to - from) < 1 || from <= 0) return null
    const animation = card.animate([{ height: `${from}px`, overflow: 'clip' }, { height: `${to}px`, overflow: 'clip' }], { duration: t.morph, easing: t.settle })
    // What is in the card can still change while the frame travels (a list that has just loaded): the frame goes on to the new height.
    const watch = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(() => {
      if (morph?.animation !== animation || !card.isConnected) return
      const natural = naturalHeight(card)
      if (Math.abs(natural - morph.to) < 1) return
      const now = card.getBoundingClientRect().height
      stopMorph(); startMorph(card, now, tokens())
    })
    for (const child of card.children) watch?.observe(child)
    morph = { card, animation, from, to, started: performance.now(), duration: t.morph, watch }
    morphing.add(card)
    const done = () => { if (morph?.animation === animation) endMorph() }
    animation.finished.then(done, done)
    return animation
  }

  function enter(card: HTMLElement, t: Tokens, { direction, axis, after, header }: { direction: -1 | 0 | 1; axis: 'x' | 'y'; after: number; header: boolean }) {
    const rows = rowsOf(card).filter(row => header || row.dataset.slot !== 'card-header')
    // From below when the list was travelled downwards, from above the other way; across on a phone, where the list runs across.
    // Across, the rows stay inside the card's own inset, so nothing is cut at its edge.
    const distance = (direction || 1) * t.rise * 1.5, from = axis === 'x' ? `${distance}px 0px` : `0px ${distance}px`
    rows.forEach((row, index) => {
      const delay = after + staggerDelay(index, t.step)
      entering.push(row.animate({ opacity: [0, 1] }, { duration: t.fadeIn, delay, easing: t.fade, fill: 'backwards' }))
      entering.push(row.animate({ translate: [from, '0px 0px'] }, { duration: t.rows, delay, easing: t.settle, fill: 'backwards' }))
    })
    enteringRows = rows
    const mine = entering
    const done = () => { if (entering === mine) { entering = []; enteringRows = [] } }
    Promise.all(mine.map(animation => animation.finished)).then(done, done)
  }
  const stopEntering = () => { for (const animation of entering) try { animation.cancel() } catch { /* already over */ } entering = []; enteringRows = [] }

  /** A copy of what the card shows, to fade where it was while the card itself already holds the next section. */
  function ghostOf(card: HTMLElement, box: DOMRect, origin: DOMRect): { root: HTMLElement; copy: HTMLElement } {
    // Rows that were still arriving are copied as faint as they were, so nothing flashes when sections are clicked through quickly.
    const faint = enteringRows.filter(row => row.isConnected).map((row, index) => { row.setAttribute('data-ghost-row', String(index)); return getComputedStyle(row).opacity })
    const copy = card.cloneNode(true) as HTMLElement
    enteringRows.forEach(row => row.removeAttribute('data-ghost-row'))
    copy.querySelectorAll('[data-ghost-row]').forEach(row => { (row as HTMLElement).style.opacity = faint[Number(row.getAttribute('data-ghost-row'))] ?? '1' })
    copy.querySelectorAll('dialog, [data-flow-ghost]').forEach(node => node.remove())
    // Nothing in the copy answers to a name any more. A drawing keeps its own (its clips and fills are found by them).
    for (const node of [copy, ...copy.querySelectorAll(':is([id], [name], [for]):not(svg, svg *)')]) { node.removeAttribute('id'); node.removeAttribute('name'); node.removeAttribute('for') }
    copy.classList.add('is-ghost')
    const root = document.createElement('div')
    root.className = 'settings-ghost'; root.inert = true; root.setAttribute('aria-hidden', 'true')
    Object.assign(root.style, { left: `${box.left - origin.left}px`, top: `${box.top - origin.top}px`, width: `${box.width}px`, height: `${box.height}px` })
    root.append(copy)
    return { root, copy }
  }
  const clearGhosts = () => { for (const ghost of ghosts) ghost.remove(); ghosts.clear() }

  const settle = () => { stopMorph(); stopEntering(); clearGhosts() }
  const controller = { settle }
  controllers.add(controller)

  // A section that shows a card while it loads and another once it has (Updates): the frame goes from one to the other.
  const replaced = typeof MutationObserver === 'undefined' ? null : new MutationObserver(() => {
    const card = primary()
    if (card === shown || disposed) return
    if (!card || !canAnimate() || reducedMotion() || staging()) { stopMorph(); follow(card); return }
    const t = tokens(), from = frameHeight(t)
    stopMorph(); stopEntering(); follow(card)
    startMorph(card, from, t)
    enter(card, t, { direction: 1, axis: 'y', after: 0, header: false })
  })
  const watched = stage()
  if (watched) replaced?.observe(watched, { childList: true })
  follow(primary())

  return {
    change(direction, update) {
      const before = primary()
      if (disposed || !before || !canAnimate() || staging()) { settle(); update(); follow(primary()); return }
      if (reducedMotion()) {
        settle(); update(); follow(primary())
        // No movement: the card is there at once, at its new height, and what is in it is shown with one short fade.
        const duration = cssMilliseconds(getComputedStyle(document.documentElement).getPropertyValue('--dur-reduced'), 120)
        for (const part of primary()?.children ?? []) part.animate({ opacity: [0, 1] }, { duration, easing: 'linear' })
        return
      }
      const t = tokens(), axis = axisOf(pane), box = before.getBoundingClientRect(), from = box.height
      const ghost = ghostOf(before, box, pane.getBoundingClientRect())
      stopMorph(); stopEntering()
      update()
      const card = primary()
      follow(card)
      if (!card) return
      const frame = startMorph(card, from, t)
      // What was there leaves first and fast, the way the list was travelled; the new rows start once it is nearly gone, so the two are never both readable.
      const away = -(direction || 1) * t.rise, to = axis === 'x' ? `${away * 1.5}px 0px` : `0px ${away}px`
      pane.append(ghost.root); ghosts.add(ghost.root)
      if (frame) ghost.root.animate({ height: [`${from}px`, `${naturalHeight(card)}px`] }, { duration: t.morph, easing: t.settle, fill: 'forwards' })
      const remove = () => { ghost.root.remove(); ghosts.delete(ghost.root) }
      const leaving = ghost.copy.animate({ opacity: [1, 0], translate: ['0px 0px', to] }, { duration: t.leave, easing: t.exit, fill: 'forwards' })
      leaving.finished.then(remove, remove)
      window.setTimeout(remove, t.leave + 200)
      enter(card, t, { direction, axis, after: t.leave * 0.9, header: true })
    },
    arrive(after) {
      const card = primary()
      if (disposed || !card || !canAnimate() || reducedMotion() || staging()) return
      enter(card, tokens(), { direction: 1, axis: 'y', after, header: true })
    },
    settle,
    dispose() { disposed = true; settle(); controllers.delete(controller); replaced?.disconnect(); sizes?.disconnect() },
  }
}

// ── Lists ──────────────────────────────────────────────────────────────────────────────────────────────────

interface Flow { boxes: Map<HTMLElement, Box>; /** The list's own height, without what a change of height is still adding to it. */ height: number; /** Its own padding underneath. */ padding: number; resize: Animation | null; observer: ResizeObserver | null }
const flows = new WeakMap<HTMLElement, Flow>()
const boxOf = (element: HTMLElement): Box => ({ left: element.offsetLeft, top: element.offsetTop, width: element.offsetWidth, height: element.offsetHeight })
const rowsIn = (list: HTMLElement) => Array.from(list.children).filter((child): child is HTMLElement => child instanceof HTMLElement && child.dataset.flowGhost === undefined)
const measure = (list: HTMLElement) => new Map(rowsIn(list).map(row => [row, boxOf(row)]))
/** Lifeless, and answering to no name: a copy or a leftover that is only there to fade. */
function lifeless(part: HTMLElement, box: Box) {
  part.dataset.flowGhost = ''; part.inert = true; part.setAttribute('aria-hidden', 'true')
  for (const node of [part, ...part.querySelectorAll('[id]:not(svg, svg *)')]) node.removeAttribute('id')
  Object.assign(part.style, { position: 'absolute', left: `${box.left}px`, top: `${box.top}px`, width: `${box.width}px`, height: `${box.height}px`, margin: '0', overflow: 'hidden', pointerEvents: 'none' })
}
function moving(element: HTMLElement): { x: number; y: number } {
  const [x, y] = getComputedStyle(element).translate.split(' ')
  return { x: parseFloat(x ?? '') || 0, y: parseFloat(y ?? '') || 0 }
}
const below = (list: HTMLElement) => { const style = getComputedStyle(list); return { padding: parseFloat(style.paddingBottom) || 0, margin: parseFloat(style.marginBottom) || 0 } }

/** Compares a list with how it was last drawn and moves between the two. Called after every change of the list.
 * `sideways`: what a row does when it changes column (a grid that is dealt again). Travelling, such rows would cross
 * over each other, so there a row fades where it was and then settles in at its new place: the two are never both readable. */
function flow(list: HTMLElement, enter: 'rise' | 'pop', sideways: 'travel' | 'settle') {
  const known = flows.get(list), live = below(list)
  known?.resize?.cancel()
  const rest = below(list), rows = rowsIn(list), boxes = measure(list), height = list.offsetHeight
  const state: Flow = known ?? { boxes, height, padding: rest.padding, resize: null, observer: null }
  state.resize = null; state.padding = rest.padding
  if (known && canAnimate() && !reducedMotion() && !staging()) {
    const t = tokens()
    for (const row of rows) {
      const was = known.boxes.get(row), now = boxes.get(row)!
      if (!was) {
        row.animate({ opacity: [0, 1] }, { duration: t.fadeIn, easing: t.fade })
        row.animate(enter === 'pop' ? { scale: [0.85, 1] } : { translate: [`0px ${t.rise}px`, '0px 0px'] }, { duration: t.rows, easing: t.settle })
        continue
      }
      const { x, y } = travel(was, now, moving(row))
      if (sideways === 'settle' && Math.abs(was.left - now.left) >= 1) {
        const copy = row.cloneNode(true) as HTMLElement, gone = () => copy.remove()
        lifeless(copy, was); list.append(copy)
        copy.animate({ opacity: [1, 0] }, { duration: t.leave * 0.8, easing: t.fade, fill: 'forwards' }).finished.then(gone, gone)
        window.setTimeout(gone, t.leave + 200)
        row.animate({ opacity: [0, 1] }, { duration: t.fadeIn, delay: t.leave * 0.6, easing: t.fade, fill: 'backwards' })
        row.animate({ translate: [`0px ${t.rise}px`, '0px 0px'] }, { duration: t.rows, delay: t.leave * 0.6, easing: t.settle, fill: 'backwards' })
        continue
      }
      if (Math.abs(x) >= 1 || Math.abs(y) >= 1) row.animate({ translate: [`${x}px ${y}px`, '0px 0px'] }, { duration: t.rows, easing: t.settle })
      // The row stayed in its column but changed width: what keeps to its far edge (`data-edge`) is suddenly somewhere
      // else, where a neighbour is leaving or arriving. It settles in there, after the neighbour has gone.
      if (sideways === 'settle' && Math.abs(was.width - now.width) >= 1) for (const part of row.querySelectorAll<HTMLElement>('[data-edge]')) part.animate({ opacity: [0, 1] }, { duration: t.fadeIn, delay: t.leave * 0.6, easing: t.fade, fill: 'backwards' })
    }
    // A row that was taken out is put back where it was, lifeless, for as long as it takes to fade.
    for (const [row, was] of known.boxes) {
      if (boxes.has(row) || row.isConnected) continue
      lifeless(row, was); list.append(row)
      const remove = () => row.remove()
      row.animate({ opacity: [1, 0], scale: [1, 0.96] }, { duration: t.leave, easing: t.exit, fill: 'forwards' }).finished.then(remove, remove)
      window.setTimeout(remove, t.leave + 200)
    }
    // What follows the list is carried to its new place: held where it was by space under the list (or pulled back
    // over it), which is then let go. Added to the layout rather than fixed, so the list can keep changing meanwhile.
    // Inside a card whose frame is travelling, the frame does this.
    const card = list.closest('[data-slot=card]'), extra = known.height + (live.padding - rest.padding) + (live.margin - rest.margin) - height
    if (Math.abs(extra) >= 1 && !(card && morphing.has(card))) {
      const resize = list.animate(extra > 0 ? { paddingBottom: [`${rest.padding + extra}px`, `${rest.padding}px`] } : { marginBottom: [`${rest.margin + extra}px`, `${rest.margin}px`] }, { duration: t.rows, easing: t.settle })
      state.resize = resize
      const done = () => { if (state.resize === resize) state.resize = null }
      resize.finished.then(done, done)
    }
  }
  state.boxes = boxes; state.height = height
  // Kept current between changes: a row can move without the list changing (an editor that opens above it).
  if (!state.observer && typeof ResizeObserver !== 'undefined') state.observer = new ResizeObserver(() => {
    if (!list.isConnected) return
    // While space under the list is being let go, that space is not the list's own height.
    state.boxes = measure(list); state.height = list.offsetHeight - (state.resize ? below(list).padding - state.padding : 0)
  })
  if (state.observer) { state.observer.disconnect(); state.observer.observe(list); for (const row of rows) state.observer.observe(row) }
  flows.set(list, state)
}

/** The children of `ref` travel when the list changes: see `flow`. `enter` is how a new row arrives. */
export function useFlow(ref: RefObject<HTMLElement | null>, enter: 'rise' | 'pop' = 'rise', sideways: 'travel' | 'settle' = 'travel') {
  const last = useRef<HTMLElement | null>(null)
  useLayoutEffect(() => {
    const list = ref.current
    if (last.current && last.current !== list) { flows.get(last.current)?.observer?.disconnect(); flows.delete(last.current) }
    last.current = list
    if (list) flow(list, enter, sideways)
  })
  useEffect(() => () => { if (last.current) { flows.get(last.current)?.observer?.disconnect(); flows.delete(last.current) } }, [])
}
