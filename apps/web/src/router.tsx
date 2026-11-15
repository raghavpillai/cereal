import { createRouter } from '@tanstack/react-router'
import { routeTree } from './routeTree.gen'
import { RoutePending } from './components/reader-state'

const isNote = (path: string) => /^\/notes\/[^/]+/.test(path), isList = (path: string) => /^\/notes\/?$/.test(path)

/** A browser skips a view transition when the viewport resizes, the tab is
 * hidden, or another navigation starts before it finishes, and rejects the
 * transition's promises. Nothing is wrong, and nothing awaits them, so each
 * skipped transition would log an uncaught rejection. Mark them handled, once. */
function quietSkippedTransitions() {
  if (typeof document === 'undefined' || typeof document.startViewTransition !== 'function') return
  const patched = document as Document & { __cerealQuietTransitions?: boolean }
  if (patched.__cerealQuietTransitions) return
  patched.__cerealQuietTransitions = true
  const start = document.startViewTransition.bind(document)
  document.startViewTransition = ((...args: Parameters<typeof start>) => {
    const transition = start(...args)
    transition.ready.catch(() => {})
    transition.finished.catch(() => {})
    transition.updateCallbackDone.catch(() => {})
    return transition
  }) as typeof document.startViewTransition
}

export function getRouter() {
  quietSkippedTransitions()
  return createRouter({
    routeTree,
    // Hovering or focusing a link fetches that page's code, so the click has nothing to wait for.
    defaultPreload: 'intent',
    defaultPendingComponent: RoutePending,
    defaultPendingMs: 120,
    defaultPendingMinMs: 150,
    defaultStaleTime: 0,
    defaultGcTime: 0,
    scrollRestoration: true,
    // Page changes run inside a view transition where the browser has one, and
    // are an ordinary navigation where it does not. Opening a note from the list
    // and going back are typed, because there the row and the masthead carry the
    // motion and the page itself only cross-fades.
    defaultViewTransition: {
      types: ({ fromLocation, toLocation, pathChanged }) => {
        if (!pathChanged) return false
        const from = fromLocation?.pathname ?? '', to = toLocation.pathname
        return [isList(from) && isNote(to) ? 'open' : isNote(from) && isList(to) ? 'close' : 'page']
      },
    },
  })
}

declare module '@tanstack/react-router' {
  interface Register { router: ReturnType<typeof getRouter> }
}
