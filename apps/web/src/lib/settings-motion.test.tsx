import { describe, expect, test } from 'bun:test'
import { readFileSync } from 'node:fs'
import { renderToStaticMarkup } from 'react-dom/server'
import { easingAt, easingPoints, saveLine, saveState, sectionDirection, staggerDelay, travel } from '../components/admin/settings-motion'
import { Reveal, Swap } from '../components/admin/shared'

const sections = ['identity', 'providers', 'models', 'sign-in', 'storage', 'apps', 'administrators', 'updates']

describe('which way a change of section travels', () => {
  test('towards the end of the list is 1, towards its start is -1', () => {
    expect(sectionDirection(sections, 'identity', 'models')).toBe(1)
    expect(sectionDirection(sections, 'updates', 'providers')).toBe(-1)
    expect(sectionDirection(sections, 'apps', 'administrators')).toBe(1)
  })
  test('the same section, or one the list does not have, does not travel', () => {
    expect(sectionDirection(sections, 'models', 'models')).toBe(0)
    expect(sectionDirection(sections, 'models', 'billing')).toBe(0)
    expect(sectionDirection(sections, '', 'models')).toBe(0)
    expect(sectionDirection([], 'a', 'b')).toBe(0)
  })
})

describe('the springs, read from the design tokens', () => {
  const tokens = readFileSync(new URL('../styles/tokens.css', import.meta.url), 'utf8')
  const token = (name: string) => new RegExp(`--${name}: (linear\\([^;]+\\));`).exec(tokens)![1]!
  test('every spring token is understood, starts at 0 and ends at 1, in order of time', () => {
    for (const name of ['spring-snappy', 'spring-gooey', 'spring-settle', 'ease-exit']) {
      const points = easingPoints(token(name))!
      expect(points.length).toBeGreaterThan(10)
      expect(points[0]).toEqual([0, 0])
      expect(points.at(-1)).toEqual([1, 1])
      for (let index = 1; index < points.length; index++) expect(points[index]![0]).toBeGreaterThanOrEqual(points[index - 1]![0])
    }
  })
  test('the settle spring is most of the way there in the first third and overshoots a little', () => {
    const points = easingPoints(token('spring-settle'))!
    expect(easingAt(points, 0)).toBe(0)
    expect(easingAt(points, 1)).toBe(1)
    expect(easingAt(points, 0.3375)).toBeCloseTo(0.8885, 4)
    expect(easingAt(points, 0.32)).toBeGreaterThan(0.83)
    expect(easingAt(points, 0.32)).toBeLessThan(0.8885)
    expect(Math.max(...points.map(([, value]) => value))).toBeCloseTo(1.0144, 4)
  })
  test('stops without a time are spread between their neighbours, as CSS spreads them', () => {
    expect(easingPoints('linear(0, 0.5, 1)')).toEqual([[0, 0], [0.5, 0.5], [1, 1]])
    expect(easingPoints('linear(0, 0.2, 0.6, 0.9 80%, 1)')).toEqual([[0, 0], [0.8 / 3, 0.2], [1.6 / 3, 0.6], [0.8, 0.9], [1, 1]])
    expect(easingAt([[0, 0], [0.5, 1], [1, 1]], 0.25)).toBe(0.5)
  })
  test('anything that is not a linear() easing is not guessed at', () => {
    // With reduced motion the tokens are a plain curve, and a browser without linear() gets one too.
    for (const other of ['cubic-bezier(0.33, 1, 0.68, 1)', 'var(--ease-fade)', 'ease-out', '', 'linear', 'linear(0)', 'linear(0, x 50%, 1)', 'linear(0, 0.5 half, 1)']) expect(easingPoints(other)).toBeNull()
    expect(easingAt([], 0.4)).toBe(0.4)
  })
})

describe('rows that arrive one after another', () => {
  test('each waits a step longer, and a long section stops adding to the wait', () => {
    expect(staggerDelay(0, 22)).toBe(0)
    expect(staggerDelay(3, 22)).toBe(66)
    expect(staggerDelay(7, 22)).toBe(154)
    expect(staggerDelay(40, 22)).toBe(154)
    expect(staggerDelay(40, 22, 3)).toBe(66)
    expect(staggerDelay(-2, 22)).toBe(0)
    // Reduced motion: the step is nothing, so nothing waits.
    expect(staggerDelay(5, 0)).toBe(0)
  })
})

describe('a row that travels to its new place', () => {
  const box = (left: number, top: number) => ({ left, top, width: 100, height: 40 })
  test('starts where it was', () => {
    expect(travel(box(0, 88), box(0, 44))).toEqual({ x: 0, y: 44 })
    expect(travel(box(120, 0), box(0, 48))).toEqual({ x: 120, y: -48 })
    expect(travel(box(10, 10), box(10, 10))).toEqual({ x: 0, y: 0 })
  })
  test('one that was still travelling starts from where it had got to', () => {
    expect(travel(box(0, 88), box(0, 44), { x: 0, y: -30 })).toEqual({ x: 0, y: 14 })
  })
})

describe('the save row', () => {
  test('says one thing at a time: saving, then what is wrong, then what waits, then that all is saved', () => {
    expect(saveState(true, 'Enter a team name.', true)).toBe('saving')
    expect(saveState(false, 'Enter a team name.', true)).toBe('problem')
    expect(saveState(false, null, true)).toBe('unsaved')
    expect(saveState(false, null, false)).toBe('saved')
  })
  test('only an edit with nothing wrong with it can be saved', () => {
    // "Save changes" is enabled in exactly one state.
    const states = [saveState(true, null, true), saveState(false, 'x', true), saveState(false, null, true), saveState(false, null, false)]
    expect(states.filter(state => state === 'unsaved')).toHaveLength(1)
  })
  test('its words', () => {
    expect(saveLine('saving', null)).toBe('Saving changes…')
    expect(saveLine('problem', 'Enter a team name of 1–100 characters.')).toBe('Enter a team name of 1–100 characters.')
    expect(saveLine('unsaved', null)).toBe('You have unsaved changes.')
    expect(saveLine('saved', null)).toBe('All changes saved')
  })
})

describe('things that come and go, drawn for the first time', () => {
  test('what is there from the start is simply there: it does not open', () => {
    const markup = renderToStaticMarkup(<Reveal className="reveal-row"><p>Shown</p></Reveal>)
    expect(markup).toBe('<div class="reveal reveal-row" data-phase="shown"><div class="reveal-inner"><p>Shown</p></div></div>')
  })
  test('what is not there leaves nothing behind, not even an empty box', () => {
    for (const nothing of [null, undefined, false, '']) expect(renderToStaticMarkup(<Reveal>{nothing}</Reveal>)).toBe('')
  })
  test('words that have not changed are plain words', () => {
    expect(renderToStaticMarkup(<Swap text="2 enabled" />)).toBe('<span class="swap"><span class="swap-now">2 enabled</span></span>')
  })
  test('a save row drawn again after a save arrives from what it said before, which is not read out twice', () => {
    const markup = renderToStaticMarkup(<Swap text="All changes saved" initial="Saving changes…" />)
    expect(markup).toContain('<span class="swap-now is-arriving">All changes saved</span>')
    expect(markup).toContain('<span class="swap-was" aria-hidden="true">Saving changes…</span>')
    expect(renderToStaticMarkup(<Swap text="Same" initial="Same" />)).not.toContain('swap-was')
  })
})
