/** Opening a note in the Cereal app from its web page. Pure: no DOM, no storage of
 * its own. The page shows the link, and may try it once on a desktop browser. */

const TOKEN = /^[A-Za-z0-9_-]{43}$/
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i
const LOOPBACK = new Set(['localhost', '127.0.0.1', '[::1]'])

/** Scheme, host and port with no trailing slash, or null when the apps would refuse it:
 * they accept https anywhere and plain http only on this machine. */
export function appOrigin(value: string): string | null {
  let url: URL
  try { url = new URL(value) } catch { return null }
  if (url.username || url.password) return null
  if (url.protocol === 'https:' || (url.protocol === 'http:' && LOOPBACK.has(url.hostname))) return url.origin
  return null
}

/** `cereal://note?server=<origin>&id=<note id>` for a note in the reader's own account. */
export function noteAppURL(origin: string, noteId: string): string | null {
  const server = appOrigin(origin)
  if (!server || !UUID.test(noteId)) return null
  return `cereal://note?server=${encodeURIComponent(server)}&id=${encodeURIComponent(noteId.toLowerCase())}`
}

/** `cereal://shared?url=<the shared link>` for a snapshot on any Cereal server. */
export function sharedAppURL(origin: string, token: string): string | null {
  const server = appOrigin(origin)
  if (!server || !TOKEN.test(token)) return null
  return `cereal://shared?url=${encodeURIComponent(`${server}/s/${token}`)}`
}

export interface Device {
  userAgent: string
  /** `navigator.platform`: an iPad asking for the desktop site says "MacIntel". */
  platform?: string
  maxTouchPoints?: number
}

/** iPhone, iPad (also when it asks for desktop sites) and Android. There the system
 * opens the app from the link itself; a page that tries a custom scheme gets an error
 * sheet when the app is missing. */
export function isHandheld(device: Device): boolean {
  if (/iPhone|iPad|iPod|Android/i.test(device.userAgent)) return true
  return /Mac/i.test(device.platform ?? '') && (device.maxTouchPoints ?? 0) > 1
}

/** The two calls of `sessionStorage` that the guard needs. */
export interface TabMemory { getItem: (key: string) => string | null; setItem: (key: string, value: string) => void }

const memoryKey = (appURL: string) => `cereal-open:${appURL}`

/** Whether to try the app now, and remember that it was tried. True once per tab for
 * each link, only on a desktop browser, and only on the page the tab was opened on:
 * a note reached from the list was chosen on the web. Back and reload find the mark
 * and do nothing. Without working tab storage there is no way to try only once, so
 * the page does not try at all. */
export function shouldAttemptOpen(input: { appURL: string | null; device: Device; memory: TabMemory | null; entryPath: string; currentPath: string }): boolean {
  const { appURL, device, memory, entryPath, currentPath } = input
  if (!appURL || !memory || isHandheld(device) || entryPath !== currentPath) return false
  try {
    const key = memoryKey(appURL)
    if (memory.getItem(key) !== null) return false
    memory.setItem(key, '1')
    return memory.getItem(key) !== null
  } catch { return false }
}
