import { createRootRoute, HeadContent, Link, Outlet, Scripts } from '@tanstack/react-router'
import { useEffect, useState } from 'react'
import { HeaderSlotContext } from '../components/kit/header-slot'
import { PageError, PageNotFound } from '../components/reader-state'
import appIcon from '../assets/app-icon.svg?url'
import touchIcon from '../assets/apple-touch-icon.png?url'
import melange from '../fonts/melange.ttf?url'
import quadrant from '../fonts/quadrant.ttf?url'
import stylesheet from '../styles.css?url'

/** Applies a remembered light/dark choice before first paint. Only the theme
 * name is stored; note content never enters browser storage. */
const themeBootstrap = `try{var t=localStorage.getItem('cereal-theme');if(t==='light'||t==='dark')document.documentElement.dataset.theme=t}catch(e){}`
const PAPER = { light: '#faf8f0', dark: '#1c1a17' }

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

function Root() {
  const [slot, setSlot] = useState<HTMLElement | null>(null)
  return <html lang="en" suppressHydrationWarning><head><HeadContent /></head><body>
    <a className="skip-link" href="#main-content">Skip to content</a>
    <header className="site-header">
      <Link to="/" className="brand"><span className="brand-chip" aria-hidden="true"><span className="mark" /></span><span>Cereal</span></Link>
      <nav className="site-nav" aria-label="Site">
        <Link to="/notes" className="nav-link" activeProps={{ 'aria-current': 'page' }}>Notes</Link>
        <span className="header-slot" ref={setSlot} />
        <ThemeToggle />
      </nav>
    </header>
    <HeaderSlotContext value={slot}><main id="main-content" tabIndex={-1}><Outlet /></main></HeaderSlotContext>
    <footer className="site-footer"><span className="site-footer-line">A little more clarity.</span><nav aria-label="Footer"><Link to="/notes">Notes</Link><Link to="/account">Account</Link><Link to="/mcp/connect">Connections</Link></nav></footer>
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
  return <button type="button" className="icon-button theme-toggle" aria-label="Dark theme" aria-pressed={dark} title={dark ? 'Switch to light theme' : 'Switch to dark theme'} onClick={toggle}>
    <svg className="theme-sun" viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="4" /><path d="M12 3v2M12 19v2M3 12h2M19 12h2M5.6 5.6l1.4 1.4M17 17l1.4 1.4M5.6 18.4 7 17M17 7l1.4-1.4" /></svg>
    <svg className="theme-moon" viewBox="0 0 24 24" aria-hidden="true"><path d="M20 14.5A8 8 0 0 1 9.5 4a8 8 0 1 0 10.5 10.5Z" /></svg>
  </button>
}
