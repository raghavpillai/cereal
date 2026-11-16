/** Copying text. `navigator.clipboard` exists only on secure origins (HTTPS, or this machine), and a Cereal
 * server on a private network is reached over plain HTTP, where it is undefined. So every copy button goes
 * through here: the clipboard API where the browser offers it, else the older way that works everywhere,
 * a hidden text field that is selected and copied with `document.execCommand('copy')`. */

/** The parts of the browser this needs; tests pass their own. */
export interface CopyEnvironment {
  clipboard?: { writeText?: (text: string) => Promise<void> } | null
  document?: Document | null
}
const browser = (): CopyEnvironment => ({
  clipboard: typeof navigator === 'undefined' ? null : navigator.clipboard,
  document: typeof document === 'undefined' ? null : document,
})

/** Puts `text` on the clipboard and says which way worked. Throws when neither did, so callers keep their
 * "select it and copy it yourself" message. Must be called from a click or key press, like any copy. */
export async function copyText(text: string, environment: CopyEnvironment = browser()): Promise<'clipboard' | 'fallback'> {
  const { clipboard, document: page } = environment
  if (clipboard?.writeText) {
    // Refused permission or a page without focus: the older way may still work.
    try { await clipboard.writeText(text); return 'clipboard' } catch { /* try the field */ }
  }
  if (page && copyThroughField(text, page)) return 'fallback'
  throw new Error('Copying is not available in this browser')
}

/** Selects the text in a field nobody sees and asks the browser to copy the selection. Focus and any
 * selection the person had are put back afterwards. */
function copyThroughField(text: string, page: Document): boolean {
  if (typeof page.execCommand !== 'function' || !page.body) return false
  const active = page.activeElement as HTMLElement | null
  const focused = active && typeof active.focus === 'function' ? active : null
  const selection = page.getSelection?.() ?? null
  const ranges = selection ? Array.from({ length: selection.rangeCount }, (_, at) => selection.getRangeAt(at)) : []
  const field = page.createElement('textarea')
  field.value = text
  field.readOnly = true
  field.setAttribute('aria-hidden', 'true')
  field.tabIndex = -1
  // On screen (a field that is not rendered cannot be selected) but not visible, and not able to scroll the page.
  Object.assign(field.style, { position: 'fixed', top: '0', left: '0', width: '1px', height: '1px', padding: '0', border: '0', opacity: '0', fontSize: '16px' })
  // Inside an open dialog only the dialog can take focus, so the field goes there.
  const home = (typeof focused?.closest === 'function' ? focused.closest('dialog[open]') : null) ?? page.body
  home.appendChild(field)
  let copied = false
  try {
    field.focus({ preventScroll: true })
    field.select()
    field.setSelectionRange(0, text.length)
    copied = page.execCommand('copy')
  } catch { copied = false } finally {
    field.remove()
    if (selection) { selection.removeAllRanges(); for (const range of ranges) selection.addRange(range) }
    focused?.focus({ preventScroll: true })
  }
  return copied
}
