import { Link } from '@tanstack/react-router'

/** What a page shows while its code loads: quiet space, because the shell cannot know which page is coming. */
export function RoutePending() {
  return <div className="route-pending" aria-busy="true"><span className="sr-only" role="status">Loading…</span></div>
}

/** A note before its text is known: the title, the chips and a few lines, where they will be. */
export function ReaderLoading() {
  return <section className="note-doc" aria-busy="true" aria-label="Loading"><div className="note-loading is-page">
    <div className="skeleton is-title" /><div className="skeleton is-chips" /><div className="skeleton" /><div className="skeleton" /><div className="skeleton is-short" />
    <span className="sr-only" role="status">Loading…</span>
  </div></section>
}

export function ReaderUnavailable() {
  return <section className="reader-state">
    <h1 className="app-heading">This note is unavailable.</h1>
    <p>The link may have expired or been revoked. Ask the person who shared it for a new link.</p>
  </section>
}

export function ReaderError({ retry }: { retry: () => void }) {
  return <section className="reader-state" role="alert">
    <h1 className="app-heading">We couldn’t load this note.</h1>
    <p>Check your connection and try again.</p>
    <div className="reader-state-actions"><button type="button" className="app-pill" onClick={retry}><span>Try again</span></button></div>
  </section>
}

export function PageNotFound() {
  return <section className="reader-state">
    <h1 className="app-heading">There’s nothing at this address.</h1>
    <p>The page may have moved, or the link may be mistyped.</p>
    <div className="reader-state-actions"><Link to="/notes" className="app-pill"><span>Your notes</span></Link></div>
  </section>
}

export function PageError() {
  return <section className="reader-state" role="alert">
    <h1 className="app-heading">This page stopped working.</h1>
    <p>Reloading usually fixes it. Your notes are not affected.</p>
    <div className="reader-state-actions"><button type="button" className="app-pill" onClick={() => window.location.reload()}><span>Reload</span></button><Link to="/notes" className="app-pill"><span>Your notes</span></Link></div>
  </section>
}
