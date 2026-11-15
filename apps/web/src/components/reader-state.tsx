import { Link } from '@tanstack/react-router'
import { Compass, FileQuestion, TriangleAlert, WifiOff } from 'lucide-react'

/** What a page shows while its code loads: quiet space, because the shell cannot know which page is coming. */
export function RoutePending() {
  return <div className="route-pending" aria-busy="true"><span className="sr-only" role="status">Loading…</span></div>
}

export function ReaderLoading() {
  return <section className="note-page" aria-busy="true" aria-label="Loading"><div className="reader-skeleton">
    <div className="skeleton kicker-skeleton" /><div className="skeleton title-skeleton" /><div className="skeleton meta-skeleton" /><div className="skeleton line-skeleton" /><div className="skeleton line-skeleton" /><div className="skeleton line-skeleton short" />
    <span className="sr-only" role="status">Loading…</span>
  </div></section>
}

export function ReaderUnavailable() {
  return <section className="empty-state">
    <div className="empty-tile" aria-hidden="true"><FileQuestion /></div>
    <p className="eyebrow">Shared note</p><h1>This note is unavailable.</h1>
    <p>The link may have expired or been removed. Ask the sender for a new link.</p>
    <div className="empty-state-actions"><Link to="/" className="button is-secondary">Back to Cereal</Link></div>
  </section>
}

export function ReaderError({ retry }: { retry: () => void }) {
  return <section className="empty-state" role="alert">
    <div className="empty-tile" aria-hidden="true"><WifiOff /></div>
    <p className="eyebrow">Shared note</p><h1>We couldn’t load this note.</h1>
    <p>Check your connection and try again.</p>
    <div className="empty-state-actions"><button type="button" className="button" onClick={retry}>Try again</button></div>
  </section>
}

export function PageNotFound() {
  return <section className="empty-state">
    <div className="empty-tile" aria-hidden="true"><Compass /></div>
    <p className="eyebrow">Not found</p><h1>There’s nothing at this address.</h1>
    <p>The page may have moved, or the link may be mistyped.</p>
    <div className="empty-state-actions"><Link to="/notes" className="button">Your notes</Link><Link to="/" className="button is-secondary">Home</Link></div>
  </section>
}

export function PageError() {
  return <section className="empty-state" role="alert">
    <div className="empty-tile" aria-hidden="true"><TriangleAlert /></div>
    <p className="eyebrow">Something went wrong</p><h1>This page stopped working.</h1>
    <p>Reloading usually fixes it. Your notes are not affected.</p>
    <div className="empty-state-actions"><button type="button" className="button" onClick={() => window.location.reload()}>Reload</button><Link to="/" className="button is-secondary">Home</Link></div>
  </section>
}
