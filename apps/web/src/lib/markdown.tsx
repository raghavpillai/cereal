import { createElement, memo, type ReactNode } from 'react'

/** Links never execute document-supplied code or fetch embedded resources. */
export function safeLink(raw: string): string | undefined {
  const value = raw.trim()
  if (!value || /[\u0000-\u001f\u007f]/.test(value)) return undefined
  if (value.startsWith('#')) return value
  try {
    const url = new URL(value)
    return ['https:', 'http:', 'mailto:'].includes(url.protocol) ? url.href : undefined
  } catch { return undefined }
}

// code · image · link · bold · bold · strike · emphasis · emphasis · bare URL
const INLINE = /`([^`\n]+)`|!\[([^\]\n]*)\]\(([^\s)]+)\)|\[([^\]\n]+)\]\(([^\s)]+)\)|\*\*([^\n]+?)\*\*|__([^\n]+?)__|~~([^\n]+?)~~|\*([^*\n]+)\*|_([^_\n]+)_|(https?:\/\/[^\s<>]+)/g
const WORD = /[\p{L}\p{N}]/u
/** Emphasis may nest (bold holding italics); public input may not nest without limit. */
const MAX_INLINE_DEPTH = 6

function inline(value: string, prefix: string, depth = 0): ReactNode[] {
  if (depth > MAX_INLINE_DEPTH) return [value]
  const pattern = new RegExp(INLINE.source, 'g'), output: ReactNode[] = []
  const nested = (text: string, key: string) => inline(text, key, depth + 1)
  let cursor = 0
  for (let match = pattern.exec(value); match; match = pattern.exec(value)) {
    const offset = match.index
    let length = match[0].length
    // Underscores inside a word (snake_case) are not emphasis.
    if ((match[7] !== undefined || match[10] !== undefined) && (WORD.test(value[offset - 1] ?? '') || WORD.test(value[offset + length] ?? ''))) { pattern.lastIndex = offset + 1; continue }
    const key = `${prefix}-${depth}-${offset}`
    let node: ReactNode
    if (match[1] !== undefined) node = <code key={key}>{match[1]}</code>
    else if (match[3] !== undefined) {
      // Images are never fetched: the description becomes a plain link.
      const href = safeLink(match[3]), label = match[2] || 'image'
      node = href ? <a key={key} href={href} target="_blank" rel="noopener noreferrer nofollow">{label}</a> : label
    } else if (match[5] !== undefined) {
      const href = safeLink(match[5])
      node = href ? <a key={key} href={href} target={href.startsWith('#') ? undefined : '_blank'} rel="noopener noreferrer nofollow">{nested(match[4] ?? '', key)}</a> : match[4]
    } else if (match[6] !== undefined || match[7] !== undefined) node = <strong key={key}>{nested(match[6] ?? match[7] ?? '', key)}</strong>
    else if (match[8] !== undefined) node = <del key={key}>{nested(match[8], key)}</del>
    else if (match[9] !== undefined || match[10] !== undefined) node = <em key={key}>{nested(match[9] ?? match[10] ?? '', key)}</em>
    else {
      // A bare address: sentence punctuation after it is not part of it.
      const address = (match[11] ?? '').replace(/[.,;:!?'")\]]+$/, ''), href = safeLink(address)
      length = address.length
      if (!href || !length) { pattern.lastIndex = offset + 1; continue }
      pattern.lastIndex = offset + length
      node = <a key={key} href={href} target="_blank" rel="noopener noreferrer nofollow">{address}</a>
    }
    if (offset > cursor) output.push(value.slice(cursor, offset))
    output.push(node)
    cursor = offset + length
  }
  if (cursor < value.length) output.push(value.slice(cursor))
  return output
}

/** A row of a pipe table, or null when the line is not one. */
function tableCells(line: string): string[] | null {
  if (!line.includes('|')) return null
  return line.trim().replace(/^\|/, '').replace(/\|$/, '').split('|').map(cell => cell.trim())
}
/** The rule under a table header: at least two columns of dashes, optional colons for alignment. */
function tableRule(line: string): ('left' | 'center' | 'right' | undefined)[] | null {
  if (!/^\s*\|?\s*:?-+:?\s*(\|\s*:?-+:?\s*)+\|?\s*$/.test(line)) return null
  return tableCells(line)!.map(cell => cell.startsWith(':') && cell.endsWith(':') ? 'center' : cell.endsWith(':') ? 'right' : cell.startsWith(':') ? 'left' : undefined)
}
const startsTable = (lines: string[], at: number) => tableCells(lines[at] ?? '') !== null && tableRule(lines[at + 1] ?? '') !== null
const MAX_TABLE_COLUMNS = 24

function indentation(value: string): number {
  return [...value].reduce((width, character) => width + (character === '\t' ? 4 : 1), 0)
}

function listItem(line: string) {
  const match = /^([ \t]*)(?:([-*•])|(\d+)[.)])\s+(.+)$/.exec(line)
  if (!match) return undefined
  const ordinal = Number(match[3])
  return {
    indent: indentation(match[1] ?? ''),
    ordered: !match[2],
    start: Number.isSafeInteger(ordinal) && ordinal >= 0 ? ordinal : 1,
    text: match[4] ?? '',
  }
}

/** Keep each child list inside its owning item. Cap recursion for public input. */
function listBlock(lines: string[], start: number, depth = 0): { node: ReactNode; next: number } {
  const first = listItem(lines[start] ?? '')!
  const items: ReactNode[] = []
  let i = start
  while (i < lines.length) {
    const item = listItem(lines[i] ?? '')
    if (!item || item.indent !== first.indent || item.ordered !== first.ordered) break
    const itemIndex = i++
    const task = /^\[( |x|X)\]\s+(.*)$/.exec(item.text)
    const children: ReactNode[] = task
      ? [<span key={`task-${itemIndex}`} className="task-box" data-checked={task[1] === ' ' ? undefined : ''} aria-hidden="true" />, <span key={`state-${itemIndex}`} className="sr-only">{task[1] === ' ' ? 'To do: ' : 'Done: '}</span>, ...inline(task[2] ?? '', `item-${itemIndex}`)]
      : inline(item.text, `item-${itemIndex}`)
    while (i < lines.length) {
      const line = lines[i] ?? ''
      const nested = listItem(line)
      if (nested) {
        if (nested.indent <= first.indent) break
        if (depth >= 24) {
          children.push(<br key={`break-${i}`} />, ...inline(line.trim(), `deep-${i}`))
          i++
        } else {
          const child = listBlock(lines, i, depth + 1)
          children.push(child.node)
          i = child.next
        }
      } else if (!line.trim()) {
        let afterBlank = i + 1
        while (afterBlank < lines.length && !lines[afterBlank]?.trim()) afterBlank++
        const following = listItem(lines[afterBlank] ?? '')
        if (!following || following.indent < first.indent || (following.indent === first.indent && following.ordered !== first.ordered)) break
        i = afterBlank
      } else if (indentation(/^[ \t]*/.exec(line)?.[0] ?? '') > first.indent) {
        children.push(<br key={`break-${i}`} />, ...inline(line.trim(), `continuation-${i}`))
        i++
      } else break
    }
    items.push(<li key={`item-${itemIndex}`} className={task ? 'task-item' : undefined}>{children}</li>)
  }
  const key = `list-${start}`
  return { node: first.ordered ? <ol key={key} start={first.start === 1 ? undefined : first.start}>{items}</ol> : <ul key={key}>{items}</ul>, next: i }
}

/** Small Markdown reader. React escapes every text node; raw HTML is never used.
 * Memoised on its text, so an open note is parsed once however often the page around it renders. */
export const Markdown = memo(function Markdown({ value }: { value: string }) {
  const lines = value.replace(/\r\n?/g, '\n').split('\n')
  const blocks: ReactNode[] = []
  let i = 0
  while (i < lines.length) {
    const line = lines[i] ?? ''
    const key = `block-${i}`
    if (!line.trim()) { i++; continue }
    if (line.trimStart().startsWith('```')) {
      const language = line.trim().slice(3).trim()
      const code: string[] = []
      i++
      while (i < lines.length && !(lines[i] ?? '').trimStart().startsWith('```')) code.push(lines[i++] ?? '')
      i++
      blocks.push(<figure className="code-block" key={key}>{language && <figcaption>{language}</figcaption>}<pre><code>{code.join('\n')}</code></pre></figure>)
      continue
    }
    const heading = /^(#{1,6})\s+(.+)$/.exec(line)
    if (heading) {
      const level = Math.min(4, Math.max(2, (heading[1]?.length ?? 1) + 1))
      blocks.push(createElement(`h${level}`, { key }, inline(heading[2] ?? '', key)))
      i++; continue
    }
    if (/^\s*(-{3,}|\*{3,}|_{3,})\s*$/.test(line)) { blocks.push(<hr key={key} />); i++; continue }
    if (/^\s*>/.test(line)) {
      const quote: string[] = []
      while (i < lines.length && /^\s*>/.test(lines[i] ?? '')) quote.push((lines[i++] ?? '').replace(/^\s*>\s?/, ''))
      blocks.push(<blockquote key={key}>{inline(quote.join('\n'), key)}</blockquote>)
      continue
    }
    if (startsTable(lines, i)) {
      const head = tableCells(line)!.slice(0, MAX_TABLE_COLUMNS), align = tableRule(lines[i + 1] ?? '')!
      const rows: string[][] = []
      i += 2
      while (i < lines.length && (lines[i] ?? '').trim() && tableCells(lines[i] ?? '')) rows.push(tableCells(lines[i++] ?? '')!)
      // Wide tables scroll inside their own frame, never the page.
      blocks.push(<figure className="table-wrap" key={key} role="region" aria-label="Table" tabIndex={0}><table>
        <thead><tr>{head.map((cell, column) => <th key={column} scope="col" style={align[column] ? { textAlign: align[column] } : undefined}>{inline(cell, `${key}-h${column}`)}</th>)}</tr></thead>
        <tbody>{rows.map((row, at) => <tr key={at}>{head.map((_, column) => <td key={column} style={align[column] ? { textAlign: align[column] } : undefined}>{inline(row[column] ?? '', `${key}-${at}-${column}`)}</td>)}</tr>)}</tbody>
      </table></figure>)
      continue
    }
    if (listItem(line)) {
      const list = listBlock(lines, i)
      blocks.push(list.node)
      i = list.next
      continue
    }
    const paragraph: string[] = [line]
    i++
    while (i < lines.length) {
      const next = lines[i] ?? ''
      if (!next.trim() || /^(#{1,6}\s|\s*>|\s*```|\s*(?:[-*•]|\d+[.)])\s)/.test(next) || startsTable(lines, i)) break
      paragraph.push(next); i++
    }
    blocks.push(<p key={key}>{inline(paragraph.join('\n'), key)}</p>)
  }
  return <div className="markdown">{blocks}</div>
})
