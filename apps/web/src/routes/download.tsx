import { createFileRoute } from '@tanstack/react-router'
import { useCallback, useEffect, useRef, useState } from 'react'
import { getDownloads, macDetails, type Downloads } from '../lib/downloads'

/** Where people get the apps, for the version this server is running. Public: the app comes before the account. */
export const Route = createFileRoute('/download')({
  ssr: false,
  head: () => ({ meta: [{ title: 'Download · Cereal' }] }),
  component: DownloadPage,
})

function DownloadPage() {
  const [state, setState] = useState<{ kind: 'loading' } | { kind: 'failed' } | { kind: 'ready'; downloads: Downloads }>({ kind: 'loading' })
  const request = useRef<AbortController | null>(null)
  const load = useCallback(async () => {
    request.current?.abort()
    const controller = new AbortController(); request.current = controller
    setState({ kind: 'loading' })
    try { const downloads = await getDownloads(controller.signal); if (!controller.signal.aborted) setState({ kind: 'ready', downloads }) }
    catch { if (!controller.signal.aborted) setState({ kind: 'failed' }) }
  }, [])
  useEffect(() => { void load(); return () => request.current?.abort() }, [load])
  const downloads = state.kind === 'ready' ? state.downloads : null, mac = downloads?.mac ?? null
  return <section className="download" aria-busy={state.kind === 'loading' ? true : undefined}>
    <span className="download-mark" aria-hidden="true"><span className="mark" /></span>
    <h1>Download Cereal</h1>
    <p className="download-lede">Record meetings and write notes on your Mac and iPhone. Your notes stay on this server.</p>
    {state.kind === 'loading' && <p className="download-quiet" role="status">Looking for the latest build…</p>}
    {state.kind === 'failed' && <div className="download-quiet" role="alert"><p>The downloads could not be loaded. Check your connection and try again.</p><button type="button" className="button is-secondary" onClick={() => void load()}>Try again</button></div>}
    {downloads && (mac
      ? <div className="download-mac">
        <a className="button is-large" href={mac.installer.url} download={mac.installer.file}>Download for Mac</a>
        <p className="download-details">{macDetails(downloads.version, mac).map((detail, at) => <span key={at}>{detail}</span>)}</p>
      </div>
      : <p className="download-quiet">No build has been published on this server yet.</p>)}
    {downloads?.iosAppStoreUrl && <p className="download-phone"><span>Cereal for iPhone</span><a className="text-link" href={downloads.iosAppStoreUrl} target="_blank" rel="noopener noreferrer">Get it on the App Store</a></p>}
  </section>
}
