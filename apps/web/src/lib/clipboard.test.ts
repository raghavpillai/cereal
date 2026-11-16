import { describe, expect, test } from 'bun:test'
import { copyText } from './clipboard'

/** A page as the helper sees it: what was appended, focused, selected and asked of `execCommand`. */
function fakePage(options: { copies?: boolean; throws?: boolean; dialog?: boolean } = {}) {
  const log: string[] = []
  const seen: { value?: string; selected?: [number, number]; readOnly?: boolean; inPage?: boolean; home?: string } = {}
  const makeHome = (name: string) => ({ name, appendChild(node: { home?: string }) { node.home = name; seen.home = name; log.push(`append:${name}`) } })
  const body = makeHome('body'), dialog = makeHome('dialog')
  const previous = { focus: () => log.push('focus:previous'), closest: (selector: string) => (options.dialog && selector === 'dialog[open]' ? dialog : null) }
  const range = { id: 'range' }
  const selection = { rangeCount: 1, getRangeAt: () => range, removeAllRanges: () => log.push('selection:clear'), addRange: (given: unknown) => log.push(given === range ? 'selection:restore' : 'selection:other') }
  const page = {
    body, activeElement: previous, getSelection: () => selection,
    createElement(tag: string) {
      expect(tag).toBe('textarea')
      const field = {
        value: '', readOnly: false, tabIndex: 0, style: {} as Record<string, string>, home: undefined as string | undefined,
        setAttribute() {}, focus: () => log.push('focus:field'), select: () => log.push('select'),
        setSelectionRange(start: number, end: number) { seen.selected = [start, end] },
        remove() { field.home = undefined; log.push('remove') },
      }
      seen.inPage = false
      Object.defineProperty(seen, 'field', { value: field, enumerable: false })
      return field
    },
    execCommand(command: string) {
      const field = (seen as unknown as { field: { value: string; readOnly: boolean; home?: string } }).field
      seen.value = field.value; seen.readOnly = field.readOnly; seen.inPage = field.home !== undefined
      log.push(`exec:${command}`)
      if (options.throws) throw new Error('blocked')
      return options.copies ?? true
    },
  }
  return { page: page as unknown as Document, log, seen }
}

describe('copying text, with and without the clipboard API', () => {
  test('a secure page uses the clipboard API and touches nothing else', async () => {
    const written: string[] = [], { page, log } = fakePage()
    expect(await copyText('https://notes.example.test/s/abc', { clipboard: { writeText: async text => { written.push(text) } }, document: page })).toBe('clipboard')
    expect(written).toEqual(['https://notes.example.test/s/abc'])
    expect(log).toEqual([])
  })

  test('over plain HTTP there is no clipboard API: the text is copied through a hidden field', async () => {
    const { page, log, seen } = fakePage()
    // `navigator.clipboard` is undefined on http://10.30.1.77:3000.
    expect(await copyText('http://10.30.1.77:3000/s/abc', { clipboard: undefined, document: page })).toBe('fallback')
    expect(seen.value).toBe('http://10.30.1.77:3000/s/abc')
    expect(seen.selected).toEqual([0, 'http://10.30.1.77:3000/s/abc'.length])
    // Read-only, so a phone does not open its keyboard; in the page when the copy is asked for; gone afterwards.
    expect(seen.readOnly).toBe(true)
    expect(seen.inPage).toBe(true)
    expect(log).toEqual(['append:body', 'focus:field', 'select', 'exec:copy', 'remove', 'selection:clear', 'selection:restore', 'focus:previous'])
  })

  test('inside an open dialog the field goes into the dialog, where focus is allowed', async () => {
    const { page, seen } = fakePage({ dialog: true })
    expect(await copyText('text', { clipboard: null, document: page })).toBe('fallback')
    expect(seen.home).toBe('dialog')
  })

  test('a clipboard API that refuses falls back to the field', async () => {
    const { page, log } = fakePage()
    expect(await copyText('text', { clipboard: { writeText: async () => { throw new DOMException('denied', 'NotAllowedError') } }, document: page })).toBe('fallback')
    expect(log).toContain('exec:copy')
  })

  test('when nothing works the caller is told, and the page is left as it was', async () => {
    for (const options of [{ copies: false }, { throws: true }]) {
      const { page, log } = fakePage(options)
      await expect(copyText('text', { clipboard: undefined, document: page })).rejects.toThrow('Copying is not available')
      expect(log.slice(-4)).toEqual(['remove', 'selection:clear', 'selection:restore', 'focus:previous'])
    }
    await expect(copyText('text', { clipboard: undefined, document: null })).rejects.toThrow()
    await expect(copyText('text', { clipboard: undefined, document: { body: {} } as unknown as Document })).rejects.toThrow()
  })
})
