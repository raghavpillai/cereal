import { memo, type ReactNode } from 'react'

/** Notes are Markdown on the wire and are read a line at a time, exactly as the
 * desktop app reads them (apps/desktop/src/summary.rs): a line is a heading, a list
 * item with a depth, a rule, a gap or a paragraph. Text is never HTML: React escapes
 * every node, and nothing in a note becomes a link, an image or markup. */

export type NoteLine =
  | { kind: 'blank' }
  | { kind: 'rule' }
  | { kind: 'heading'; text: string }
  | { kind: 'list'; text: string; indent: number; ordinal: number | null }
  | { kind: 'paragraph'; text: string }

/** Width of a line's leading whitespace, a tab counting as four. */
function leading(line: string): number {
  let width = 0
  for (const character of line) {
    if (!/\s/.test(character)) break
    width += character === '\t' ? 4 : 1
  }
  return width
}

export function noteLine(line: string): NoteLine {
  const text = line.trim()
  if (!text) return { kind: 'blank' }
  if (text === '---' || text === '***' || text === '___') return { kind: 'rule' }
  const hashes = /^#{1,6}(?= )/.exec(text)?.[0].length ?? 0
  if (hashes && text[hashes] === ' ' && !text.startsWith('#'.repeat(7))) return { kind: 'heading', text: text.slice(hashes).trim() }
  const indent = leading(line)
  for (const prefix of ['- ', '* ', '+ ', '• ']) if (text.startsWith(prefix)) return { kind: 'list', text: text.slice(prefix.length).trim(), indent, ordinal: null }
  const space = text.indexOf(' ')
  if (space >= 2) {
    const marker = text.slice(0, space), digits = marker.slice(0, -1)
    if ((marker.endsWith('.') || marker.endsWith(')')) && /^\d+$/.test(digits) && Number.isSafeInteger(Number(digits))) return { kind: 'list', text: text.slice(space + 1).trim(), indent, ordinal: Number(digits) }
  }
  return { kind: 'paragraph', text }
}

/** How many columns one level of nesting is in this note: the smallest indent any list item has, at most four. */
export function listIndentUnit(lines: readonly string[]): number {
  let unit: number | null = null
  for (const line of lines) {
    const parsed = noteLine(line)
    if (parsed.kind === 'list' && parsed.indent > 0) unit = Math.min(unit ?? 4, Math.min(parsed.indent, 4))
  }
  return Math.max(unit ?? 2, 1)
}

export type NoteBlock =
  | { kind: 'blank'; row: number }
  | { kind: 'rule'; row: number }
  | { kind: 'heading'; row: number; text: string }
  /** `number` is what an ordered item shows: numbering continues from the first item of its run. */
  | { kind: 'list'; row: number; text: string; depth: number; number: number | null }
  /** `level` indents a line that continues the list item above it; `continues` closes the gap to that item. */
  | { kind: 'paragraph'; row: number; text: string; level: number; continues: boolean }

const MAX_DEPTH = 8

export function noteBlocks(source: string): NoteBlock[] {
  const lines = source.split('\n').map(line => line.endsWith('\r') ? line.slice(0, -1) : line)
  if (lines[lines.length - 1] === '') lines.pop()
  const unit = listIndentUnit(lines), blocks: NoteBlock[] = []
  const ordinals: (number | null)[] = Array.from({ length: MAX_DEPTH + 1 }, () => null)
  let afterHeading = false
  lines.forEach((line, row) => {
    const parsed = noteLine(line)
    if (parsed.kind === 'blank') {
      // The blank line Markdown puts after a heading is not a gap of its own.
      if (!afterHeading) blocks.push({ kind: 'blank', row })
      return
    }
    afterHeading = parsed.kind === 'heading'
    if (parsed.kind === 'list') {
      const depth = Math.min(Math.floor(parsed.indent / unit), MAX_DEPTH)
      let number: number | null = null
      if (parsed.ordinal !== null) { number = ordinals[depth] === null ? parsed.ordinal : ordinals[depth]! + 1; ordinals[depth] = number }
      else ordinals[depth] = null
      for (let deeper = depth + 1; deeper <= MAX_DEPTH; deeper++) ordinals[deeper] = null
      blocks.push({ kind: 'list', row, text: parsed.text, depth, number })
      return
    }
    ordinals.fill(null)
    if (parsed.kind === 'rule') blocks.push({ kind: 'rule', row })
    else if (parsed.kind === 'heading') blocks.push({ kind: 'heading', row, text: parsed.text })
    else {
      const indent = leading(line)
      blocks.push({ kind: 'paragraph', row, text: parsed.text, level: Math.min(Math.floor(indent / unit), MAX_DEPTH), continues: indent > 0 })
    }
  })
  return blocks
}

export interface InlinePart { text: string; style: 'plain' | 'strong' | 'code' | 'em' }

/** Bold, code and emphasis. A delimiter with no partner stays as typed; a backslash keeps the next character as it is. */
export function inlineParts(source: string): InlinePart[] {
  const parts: InlinePart[] = []
  let plain = '', at = 0
  const flush = () => { if (plain) { parts.push({ text: plain, style: 'plain' }); plain = '' } }
  while (at < source.length) {
    const rest = source.slice(at)
    if (rest[0] === '\\' && rest.length > 1) {
      const next = [...rest.slice(1)][0]!
      plain += next; at += 1 + next.length
      continue
    }
    const marker = rest.startsWith('**') ? '**' : rest.startsWith('__') ? '__' : rest[0] === '`' ? '`' : rest[0] === '*' ? '*' : ''
    const end = marker ? rest.indexOf(marker, marker.length) - marker.length : -1
    if (marker && end > 0) {
      flush()
      parts.push({ text: rest.slice(marker.length, marker.length + end), style: marker.length === 2 ? 'strong' : marker === '`' ? 'code' : 'em' })
      at += marker.length * 2 + end
    } else {
      const next = [...rest][0]!
      plain += next; at += next.length
    }
  }
  flush()
  return parts
}

/** A word together with the spaces after it. The app breaks a line where a word *and the space
 * after it* no longer fit; a browser lets that space hang past the edge and keeps the word. Kept
 * together, and unbreakable, each word and its space make a browser break lines where the app does. */
export function wordChunks(text: string): string[] {
  return text.match(/[^ ]+ *| +/g) ?? []
}
/** Words too long to be worth keeping whole (an address, a hash) may still break anywhere. */
const LONGEST_KEPT = 48

/** Text laid out with the app's line breaks. `before` says whether the text before this run ended in a space. */
function words(text: string, key: string, before: boolean): { nodes: ReactNode[]; spaced: boolean } {
  const nodes: ReactNode[] = []
  let spaced = before
  wordChunks(text).forEach((chunk, at) => {
    if (spaced) nodes.push(<wbr key={`${key}-b${at}`} />)
    nodes.push(chunk.length > LONGEST_KEPT ? chunk : <span className="w" key={`${key}-${at}`}>{chunk}</span>)
    spaced = chunk.endsWith(' ')
  })
  return { nodes, spaced }
}

/** Plain text (a transcript bubble) with the app's line breaks. */
export function Words({ text }: { text: string }) {
  return <>{words(text, 'w', false).nodes}</>
}

function Inline({ text }: { text: string }) {
  let spaced = false
  return <>{inlineParts(text).map((part, at) => {
    const run = words(part.text, String(at), spaced)
    spaced = run.spaced
    // The break before a run belongs outside it, so that it never splits a bold or code run from its edge.
    const lead = run.nodes.length > 1 && (run.nodes[0] as { type?: unknown }).type === 'wbr' ? run.nodes.shift() : null
    const body = part.style === 'strong' ? <strong key={at}>{run.nodes}</strong> : part.style === 'code' ? <code key={at}>{run.nodes}</code> : part.style === 'em' ? <em key={at}>{run.nodes}</em> : run.nodes
    return lead ? [lead, body] : body
  })}</>
}

/** A line with words in it. In a note that has a transcript, the desktop app keeps room
 * beside each such line for the control that shows its source; the line wraps at the same place here. */
const hasWords = (text: string) => [...text].length > 1 && /[\p{L}\p{N}]/u.test(text)

/** Generated notes, laid out as the desktop app lays them out. Memoised on its text,
 * so an open note is parsed once however often the page around it renders. */
export const NoteText = memo(function NoteText({ value, sourced = false }: { value: string; sourced?: boolean }) {
  const blocks = noteBlocks(value), out: ReactNode[] = []
  const text = (content: string) => sourced && hasWords(content) ? 'note-line has-source' : 'note-line'
  for (let i = 0; i < blocks.length;) {
    const block = blocks[i]!
    if (block.kind === 'blank') { out.push(<div className="note-gap" key={block.row} aria-hidden="true" />); i++; continue }
    if (block.kind === 'rule') { out.push(<hr key={block.row} />); i++; continue }
    if (block.kind === 'heading') {
      out.push(<h2 key={block.row} className={block.row === 0 ? 'is-first' : undefined}><span className="note-hash" aria-hidden="true">#</span><Inline text={block.text} /></h2>)
      i++; continue
    }
    if (block.kind === 'paragraph') {
      out.push(<p key={block.row} className={`${text(block.text)}${block.continues ? ' continues' : ''}`} style={block.level ? { marginLeft: block.level * 16 - 2 } : undefined}><Inline text={block.text} /></p>)
      i++; continue
    }
    // A run of list items, with the lines that continue an item kept inside it.
    const items: ReactNode[] = [], first = block.row
    while (i < blocks.length && blocks[i]!.kind === 'list') {
      const item = blocks[i++] as Extract<NoteBlock, { kind: 'list' }>, inset = 16 * (item.depth + 1)
      const more: ReactNode[] = []
      while (i < blocks.length) {
        const next = blocks[i]!
        if (next.kind !== 'paragraph' || !next.continues) break
        more.push(<p key={next.row} className={`${text(next.text)} continues`} style={next.level ? { marginLeft: next.level * 16 - 2 } : undefined}><Inline text={next.text} /></p>)
        i++
      }
      items.push(<li key={item.row} data-depth={Math.min(item.depth, 2)}>
        <div className="note-item" style={{ paddingLeft: inset }}>
          {item.number !== null
            ? <span className="note-number" style={{ left: inset - 26 }}>{item.number}.</span>
            : <span className="note-bullet" style={{ left: inset - 15 }} aria-hidden="true" />}
          <p className={text(item.text)}><Inline text={item.text} /></p>
        </div>
        {more}
      </li>)
    }
    out.push(<ul key={`list-${first}`}>{items}</ul>)
  }
  return <div className="note-text">{out}</div>
})
