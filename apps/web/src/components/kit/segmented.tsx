import { useLayoutEffect, useRef, type KeyboardEvent, type ReactNode } from 'react'

export interface SegmentedOption<T extends string> { value: T; label: ReactNode; count?: number }

/** A segmented control with one moving highlight. The highlight's position is
 * measured, never guessed; its stretch-and-catch-up motion is CSS (base.css).
 * `tabs` switches the panel it controls; `radio` picks one of a few options.
 * Both use one tab stop with arrow keys, Home and End. */
export function Segmented<T extends string>({ options, value, onChange, label, kind = 'tabs', className = '', controls }: {
  options: readonly SegmentedOption<T>[]
  value: T
  onChange: (value: T) => void
  label: string
  kind?: 'tabs' | 'radio'
  className?: string
  /** id of the panel the tabs control */
  controls?: string
}) {
  const track = useRef<HTMLDivElement>(null)
  const previous = useRef(-1)
  const index = options.findIndex(option => option.value === value)

  useLayoutEffect(() => {
    const element = track.current
    if (!element) return
    const place = () => {
      const selected = element.querySelector<HTMLElement>('[data-selected]')
      if (!selected) { delete element.dataset.ready; return false }
      element.style.setProperty('--thumb-left', `${selected.offsetLeft}px`)
      element.style.setProperty('--thumb-right', `${element.clientWidth - selected.offsetLeft - selected.offsetWidth}px`)
      return true
    }
    // The first placement and any resize are immediate; only a change of
    // selection travels, led by the edge that faces its destination.
    if (previous.current >= 0 && previous.current !== index && index >= 0) {
      element.dataset.dir = index > previous.current ? 'forward' : 'backward'
      element.querySelector<HTMLElement>('[data-selected]')?.scrollIntoView?.({ block: 'nearest', inline: 'nearest' })
    } else delete element.dataset.dir
    if (place()) element.dataset.ready = ''
    previous.current = index
    const settle = () => { delete element.dataset.dir; place() }
    // Option widths change when the web font arrives, without the track resizing.
    let live = true
    void document.fonts?.ready.then(() => { if (live) settle() })
    if (typeof ResizeObserver === 'undefined') return () => { live = false }
    const observer = new ResizeObserver(settle)
    observer.observe(element)
    return () => { live = false; observer.disconnect() }
  }, [index, options.length])

  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (!options.length) return
    const forward = event.key === 'ArrowRight' || (kind === 'radio' && event.key === 'ArrowDown'), back = event.key === 'ArrowLeft' || (kind === 'radio' && event.key === 'ArrowUp')
    const next = event.key === 'Home' ? 0 : event.key === 'End' ? options.length - 1 : forward || back ? (index + (forward ? 1 : -1) + options.length) % options.length : -1
    if (next < 0) return
    event.preventDefault()
    onChange(options[next]!.value)
    track.current?.querySelectorAll<HTMLElement>('.segmented-option')[next]?.focus()
  }

  return <div ref={track} className={`segmented ${className}`.trim()} role={kind === 'tabs' ? 'tablist' : 'radiogroup'} aria-label={label} onKeyDown={onKeyDown}>
    <span className="segmented-thumb" aria-hidden="true" />
    {options.map(option => {
      const selected = option.value === value, tabIndex = selected || index < 0 ? 0 : -1
      return <button key={option.value} type="button" className="segmented-option" data-selected={selected ? '' : undefined} tabIndex={tabIndex}
        {...(kind === 'tabs' ? { role: 'tab', 'aria-selected': selected, 'aria-controls': controls } : { role: 'radio', 'aria-checked': selected })}
        onClick={() => { if (!selected) onChange(option.value) }}>
        {option.label}{option.count !== undefined && <span className="segmented-count">{option.count}</span>}
      </button>
    })}
  </div>
}
