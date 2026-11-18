import { memo, type ReactNode } from 'react'

/** Notes are Markdown on the wire and are read a line at a time, exactly as the
 * desktop app reads them (apps/desktop/src/summary.rs): a line is a heading, a list
 * item with a depth, a rule, a gap or a paragraph. Text is never HTML: React escapes
 * every node, and nothing in a note becomes an image or markup. The one thing that
 * becomes a link is a web address written out in full (`https://…`), which is shown
 * exactly as written, so a link never reads as anything but where it goes. */

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

/** Where an address may start, and what it may be made of: everything up to white space or a delimiter. */
const ADDRESS_START = /https?:\/\//gi
const ADDRESS_END = /[\s<>"'`]/u
/** A host that reads as what it is: a plain ASCII name or address, with an optional port. No user name or password,
 * no percent sign, no letters of another script that only look like these. */
const PLAIN_HOST = /^(?:[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*\.?|\[[0-9A-Fa-f:.]+\])(?::[0-9]{1,5})?$/
/** Control and invisible formatting characters (bidirectional overrides, zero-width spaces): they can make an
 * address show one place and open another. */
const HIDDEN = /[\p{Cc}\p{Cf}]/u

/** The end of an address: sentence punctuation after it is not part of it, nor a closing bracket whose opening
 * partner is outside it (`(see https://x.example/a_(b))`), the way GitHub's autolinker reads them. */
function trimmedAddress(candidate: string): string {
  let address = candidate
  for (;;) {
    const last = address[address.length - 1] ?? ''
    const pair = ({ ')': '(', ']': '[', '}': '{' } as Record<string, string>)[last]
    if ('.,:;!?*_~'.includes(last) && last) address = address.slice(0, -1)
    else if (pair && address.split(last).length > address.split(pair).length) address = address.slice(0, -1)
    else return address
  }
}

/** Web addresses written out in full in a run of text, as pieces of the text: an address to link has its `href`.
 * The rules, and the cases every client tests, are in testdata/web-addresses.json. */
export function webLinks(text: string): { text: string; href: string | null }[] {
  const out: { text: string; href: string | null }[] = []
  let from = 0
  for (const match of text.matchAll(ADDRESS_START)) {
    const at = match.index
    if (at < from || (at > 0 && /[\p{L}\p{N}]/u.test(text[at - 1]!))) continue
    const rest = text.slice(at), stop = rest.search(ADDRESS_END)
    const address = trimmedAddress(stop < 0 ? rest : rest.slice(0, stop))
    const authority = address.slice(match[0].length).split(/[/?#]/, 1)[0]!
    if (!PLAIN_HOST.test(authority) || HIDDEN.test(address) || address.includes('\\')) continue
    let href: string
    try { href = new URL(address).href } catch { continue }
    if (at > from) out.push({ text: text.slice(from, at), href: null })
    out.push({ text: address, href })
    from = at + address.length
  }
  if (from < text.length || !out.length) out.push({ text: text.slice(from), href: null })
  return out
}

/** A link to this server (the chat with a meeting's transcript) opens in place; any other in a new tab. */
function Address({ href, children }: { href: string; children: ReactNode }) {
  const here = typeof window !== 'undefined' && new URL(href).origin === window.location.origin
  return <a className="note-link" href={href} {...(here ? {} : { target: '_blank', rel: 'noopener noreferrer' })}>{children}</a>
}

/** The address of the closing chat line, when `line` is that line and holds nothing but the label and one address. */
export function transcriptChatAddress(line: string): string | null {
  const rest = /^Chat with meeting transcript:\s*(.*)$/.exec(line.trim())?.[1]
  if (!rest) return null
  const pieces = webLinks(rest.trim())
  return pieces.length === 1 && pieces[0]!.href ? pieces[0]!.href : null
}

function Inline({ text }: { text: string }) {
  let spaced = false
  // Addresses are found in the text as written, before any Markdown is read, so a `*` or `_` inside one stays in it.
  const parts = webLinks(text).flatMap((piece): (InlinePart & { href: string | null })[] => piece.href ? [{ text: piece.text, style: 'plain', href: piece.href }] : inlineParts(piece.text).map(part => ({ ...part, href: null })))
  return <>{parts.map((part, at) => {
    const run = words(part.text, String(at), spaced)
    spaced = run.spaced
    // The break before a run belongs outside it, so that it never splits a bold or code run (or a link) from its edge.
    const lead = run.nodes.length > 1 && (run.nodes[0] as { type?: unknown }).type === 'wbr' ? run.nodes.shift() : null
    const nodes = part.href ? [<Address key={`a${at}`} href={part.href}>{run.nodes}</Address>] : run.nodes
    const body = part.style === 'strong' ? <strong key={at}>{nodes}</strong> : part.style === 'code' ? <code key={at}>{nodes}</code> : part.style === 'em' ? <em key={at}>{nodes}</em> : nodes
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
    // The closing "Chat with meeting transcript: <address>" line: a short, quiet link, as on the desktop.
    const chat = block.kind === 'paragraph' ? transcriptChatAddress(block.text) : null
    if (chat) {
      out.push(<p key={block.row} className="note-line note-transcript-link"><Address href={chat}>Chat with meeting transcript ↗</Address></p>)
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
