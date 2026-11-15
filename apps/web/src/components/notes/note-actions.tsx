import { useEffect, useRef, useState } from 'react'
import { Check, Copy, Download } from 'lucide-react'

/** Copy or download what is on screen as Markdown. Nothing is kept in browser storage. */
export function useNoteExport(markdown: () => string, fileName: string) {
  const [copied, setCopied] = useState(false), [notice, setNotice] = useState('')
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)
  useEffect(() => () => { if (timer.current) clearTimeout(timer.current) }, [])
  async function copy() {
    try {
      await navigator.clipboard.writeText(markdown())
      setCopied(true); setNotice('Copied as Markdown')
      if (timer.current) clearTimeout(timer.current)
      timer.current = setTimeout(() => { setCopied(false); setNotice('') }, 1800)
    } catch { setNotice('Couldn’t copy. You can select and copy the text instead.') }
  }
  function download() {
    const url = URL.createObjectURL(new Blob([markdown()], { type: 'text/markdown;charset=utf-8' }))
    const link = document.createElement('a')
    link.href = url; link.download = fileName
    link.click()
    setTimeout(() => URL.revokeObjectURL(url), 1000)
  }
  return { copied, notice, copy, download }
}

export function NoteActions({ copied, copy, download, what }: { copied: boolean; copy: () => Promise<void>; download: () => void; what: string }) {
  return <div className="note-actions">
    <button type="button" className="quiet-button" onClick={() => { void copy() }} aria-label={`Copy ${what} as Markdown`}>
      {copied ? <Check className="swap-in" aria-hidden="true" /> : <Copy aria-hidden="true" />}<span className="action-label">{copied ? 'Copied' : 'Copy'}</span>
    </button>
    <button type="button" className="quiet-button" onClick={download} aria-label={`Download ${what} as Markdown`}><Download aria-hidden="true" /><span className="action-label">Download</span></button>
  </div>
}
