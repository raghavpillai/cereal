import { describe, expect, test } from 'bun:test'
import { renderToStaticMarkup } from 'react-dom/server'
import { ClaimFields } from '../components/onboarding/steps'

/** For each element matching `marker`, whether it sits inside a `<fieldset disabled>`. */
function insideDisabledFieldset(markup: string, marker: string): boolean[] {
  const found: boolean[] = [], open: boolean[] = []
  for (const [tag] of markup.matchAll(/<\/?[a-z][^>]*>/g)) {
    if (tag.startsWith('</fieldset')) open.pop()
    else if (tag.startsWith('<fieldset')) open.push(/\sdisabled(=|\s|>)/.test(tag))
    if (tag.includes(marker)) found.push(open.some(Boolean))
  }
  return found
}

const owner = { code: '', method: 'google', name: '', email: '', password: '', clientId: '', clientSecret: '' } as const

describe('claim step while busy', () => {
  for (const method of ['google', 'password'] as const) {
    test(`${method}: the lines to copy stay usable and the inputs are disabled`, () => {
      const markup = renderToStaticMarkup(<ClaimFields value={{ ...owner, method }} change={() => {}} problems={{}} disabled googleNeedsHttps={false} origin="https://notes.example.com" />)
      const copies = insideDisabledFieldset(markup, 'onb-copy-button')
      expect(copies.length).toBe(method === 'google' ? 3 : 1)
      expect(copies.every(inside => !inside)).toBe(true)
      const inputs = insideDisabledFieldset(markup, '<input')
      expect(inputs.length).toBeGreaterThan(0)
      expect(inputs.every(Boolean)).toBe(true)
    })
  }

  test('not busy: nothing is disabled', () => {
    const markup = renderToStaticMarkup(<ClaimFields value={owner} change={() => {}} problems={{}} disabled={false} googleNeedsHttps={false} origin="https://notes.example.com" />)
    expect(insideDisabledFieldset(markup, '<input').some(Boolean)).toBe(false)
  })
})
