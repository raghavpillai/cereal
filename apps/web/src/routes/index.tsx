import { createFileRoute, Link } from '@tanstack/react-router'
import { ArrowRight, Link2, NotebookText, ServerCog } from 'lucide-react'
import appIcon from '../assets/app-icon.svg?url'

export const Route = createFileRoute('/')({ component: Home })

function Home() {
  return <section className="landing">
    <div className="landing-hero">
      <img className="landing-icon" src={appIcon} width="128" height="128" alt="" />
      <p className="eyebrow">Self-hosted meeting notes</p>
      <h1>A little more clarity.</h1>
      <p className="landing-lede">Cereal keeps your meeting notes on your own server. Read them here, and record and write them in the desktop and iPhone apps.</p>
      <div className="landing-actions">
        <Link to="/notes" className="button is-large">Open your notes<ArrowRight aria-hidden="true" /></Link>
        <Link to="/mcp/connect" className="button is-secondary is-large">Connect an AI app</Link>
      </div>
    </div>
    <ul className="landing-points">
      <li className="card landing-point"><span className="landing-point-icon" aria-hidden="true"><NotebookText /></span><h2>Every meeting, by day</h2><p>Summaries, your own notes and the transcript with speaker labels, ready to search.</p></li>
      <li className="card landing-point"><span className="landing-point-icon" aria-hidden="true"><Link2 /></span><h2>A note, shared with you</h2><p>Open a shared link from a meeting to read its notes. No account is needed.</p></li>
      <li className="card landing-point"><span className="landing-point-icon" aria-hidden="true"><ServerCog /></span><h2>Yours to keep</h2><p>Notes, recordings and provider keys stay on this instance, under your control.</p></li>
    </ul>
    <p className="landing-fine">Shared links are read-only. The sender controls what is shared and how long the link remains available.</p>
  </section>
}
