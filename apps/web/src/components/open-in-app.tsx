import { useEffect } from 'react'
import { shouldAttemptOpen } from '../lib/open-in-app'

/** The path this tab was opened on. A note reached later, from the list, is not it. */
function entryPath(): string {
  try {
    const entry = performance.getEntriesByType('navigation')[0]
    if (entry?.name) return new URL(entry.name).pathname
  } catch { /* fall through to where the tab is now */ }
  return window.location.pathname
}

/** Hand the address to the system from a frame nobody sees. With the app installed
 * it opens; without it nothing happens and nothing is shown. */
function tryApp(appURL: string) {
  const frame = document.createElement('iframe')
  frame.hidden = true; frame.tabIndex = -1; frame.setAttribute('aria-hidden', 'true'); frame.style.display = 'none'
  frame.src = appURL
  document.body.appendChild(frame)
  window.setTimeout(() => frame.remove(), 3000)
}

/** "Open in Cereal": always a plain link to the app; on a desktop browser it is also
 * tried once, the first time the tab loads this page. */
export function OpenInApp({ appURL, prominent = false }: { appURL: string | null; prominent?: boolean }) {
  useEffect(() => {
    if (!appURL) return
    let memory: Storage | null = null
    try { memory = window.sessionStorage } catch { /* blocked: the link is still there to press */ }
    const device = { userAgent: navigator.userAgent, platform: navigator.platform, maxTouchPoints: navigator.maxTouchPoints }
    if (shouldAttemptOpen({ appURL, device, memory, entryPath: entryPath(), currentPath: window.location.pathname })) tryApp(appURL)
  }, [appURL])
  if (!appURL) return null
  return <a className={prominent ? 'app-pill open-in-app is-prominent' : 'app-pill open-in-app'} href={appURL} aria-label="Open in Cereal"><span className="mark" aria-hidden="true" /><span className="open-in-app-label">Open in Cereal</span></a>
}
