import { createRootRoute, HeadContent, Link, Outlet, Scripts, useRouterState } from '@tanstack/react-router'
import { useEffect, useMemo, useState } from 'react'
import { HeaderSlotContext } from '../components/kit/header-slot'
import { Icon } from '../components/kit/icon'
import { PageError, PageNotFound } from '../components/reader-state'
import appIcon from '../assets/app-icon.svg?url'
import touchIcon from '../assets/apple-touch-icon.png?url'
import melange from '../fonts/melange.ttf?url'
import quadrant from '../fonts/quadrant.ttf?url'
import stylesheet from '../styles.css?url'

/** Applies a remembered light/dark choice before first paint. Only the theme
 * name is stored; note content never enters browser storage. A tab that is in first-run
 * setup (lib/session-probe.ts) is marked too, so the header that setup hides is never painted first. */
const themeBootstrap = `try{var t=localStorage.getItem('cereal-theme');if(t==='light'||t==='dark')document.documentElement.dataset.theme=t}catch(e){}try{if(location.pathname==='/admin'&&sessionStorage.getItem('cereal-setup')==='1')document.documentElement.dataset.setup=''}catch(e){}`
const PAPER = { light: '#f7f7f2', dark: '#292929' }

export const Route = createRootRoute({
  head: () => ({
    meta: [
      { charSet: 'utf-8' },
      { name: 'viewport', content: 'width=device-width, initial-scale=1' },
      { name: 'robots', content: 'noindex, nofollow, noarchive' },
      { name: 'referrer', content: 'no-referrer' },
      { name: 'color-scheme', content: 'light dark' },
      { name: 'theme-color', content: PAPER.light },
      { title: 'Cereal' },
    ],
    links: [
      // Both faces are used above the fold on every page; fetching them with the stylesheet avoids a late swap.
      { rel: 'preload', as: 'font', type: 'font/ttf', href: melange, crossOrigin: 'anonymous' },
      { rel: 'preload', as: 'font', type: 'font/ttf', href: quadrant, crossOrigin: 'anonymous' },
      { rel: 'stylesheet', href: stylesheet },
      { rel: 'icon', href: appIcon, type: 'image/svg+xml' },
      { rel: 'apple-touch-icon', href: touchIcon },
    ],
    scripts: [{ children: themeBootstrap }],
  }),
  notFoundComponent: PageNotFound,
  errorComponent: PageError,
  component: Root,
})

/** The header stands where the desktop app's title bar does and is as tall: the mark,
 * the way back to the notes, then who is signed in, the theme, and what the page can do. */
function Root() {
  const [account, setAccount] = useState<HTMLElement | null>(null), [page, setPage] = useState<HTMLElement | null>(null)
  const slots = useMemo(() => ({ account, page }), [account, page])
  // On a note the link is the way back, and says so with the app's own chevron. The shell is rendered
  // ahead of time for no page in particular, so what depends on the address waits for the browser.
  const [mounted, setMounted] = useState(false)
  useEffect(() => { setMounted(true) }, [])
  const path = useRouterState({ select: state => state.location.pathname })
  const inside = mounted && /^\/(notes\/[^/]+|s\/)/.test(path), onList = mounted && /^\/notes\/?$/.test(path)
  return <html lang="en" suppressHydrationWarning><head><HeadContent /></head><body>
    <a className="skip-link" href="#main-content">Skip to content</a>
    <header className="site-header">
      <Link to="/notes" className="site-mark" aria-label="Cereal"><span className="mark" aria-hidden="true" /></Link>
      <nav className="site-nav" aria-label="Site">
        <Link to="/notes" className="site-back" aria-current={onList ? 'page' : undefined}>{inside && <Icon name="chevron-left" size={10} />}<span>Notes</span></Link>
      </nav>
      {/* Shown once the page is running: by then each page has put its own controls, or their placeholders, into the
          slots, so the theme control appears where it will stay and is not seen sliding there. */}
      <div className="site-actions" data-ready={mounted ? '' : undefined}>
        <span className="header-slot" ref={setAccount} />
        <ThemeToggle />
        <span className="header-slot is-page" ref={setPage} />
      </div>
    </header>
    <HeaderSlotContext value={slots}><main id="main-content" tabIndex={-1}><Outlet /></main></HeaderSlotContext>
    <Scripts />
  </body></html>
}

type Theme = 'system' | 'light' | 'dark'
const systemTheme = () => window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' as const : 'light' as const

function ThemeToggle() {
  const [theme, setTheme] = useState<Theme>('system')
  const [dark, setDark] = useState(false)
  useEffect(() => {
    const stored = document.documentElement.dataset.theme
    if (stored === 'light' || stored === 'dark') setTheme(stored)
  }, [])
  // The browser's own chrome follows the page: on load, on a choice, and when the system changes.
  useEffect(() => {
    const sync = () => {
      const shown = theme === 'system' ? systemTheme() : theme
      setDark(shown === 'dark')
      document.querySelector('meta[name="theme-color"]')?.setAttribute('content', PAPER[shown])
    }
    sync()
    const system = window.matchMedia('(prefers-color-scheme: dark)')
    system.addEventListener('change', sync)
    return () => system.removeEventListener('change', sync)
  }, [theme])

  function toggle() {
    const system = systemTheme(), wanted = (theme === 'system' ? system : theme) === 'dark' ? 'light' : 'dark'
    // Choosing what the system already shows hands control back to the system.
    const next: Theme = wanted === system ? 'system' : wanted
    const root = document.documentElement
    const apply = () => { if (next === 'system') delete root.dataset.theme; else root.dataset.theme = next }
    try { if (next === 'system') localStorage.removeItem('cereal-theme'); else localStorage.setItem('cereal-theme', next) } catch { /* the choice still applies to this visit */ }
    // The whole page changes as one cross-fade, so no control is caught mid-way between themes.
    root.dataset.themeSwitch = ''
    const done = () => { delete root.dataset.themeSwitch }
    if (typeof document.startViewTransition === 'function' && !window.matchMedia('(prefers-reduced-motion: reduce)').matches) document.startViewTransition(apply).finished.then(done, done)
    else { apply(); requestAnimationFrame(() => requestAnimationFrame(done)) }
    setTheme(next)
  }
  return <button type="button" className="app-tool theme-toggle" aria-label="Dark theme" aria-pressed={dark} title={dark ? 'Switch to light theme' : 'Switch to dark theme'} onClick={toggle}>
    <Icon name="sun" size={15} className="theme-sun" />
    <Icon name="moon" size={15} className="theme-moon" />
  </button>
}
