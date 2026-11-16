import { describe, expect, test } from 'bun:test'
import { renderToStaticMarkup } from 'react-dom/server'
import { NoteText, inlineParts, listIndentUnit, noteBlocks, noteLine, webLinks, wordChunks } from './markdown'

describe('a note read a line at a time, as the desktop app reads it', () => {
  test('what one line is', () => {
    expect(noteLine('')).toEqual({ kind: 'blank' })
    expect(noteLine('   \t')).toEqual({ kind: 'blank' })
    for (const rule of ['---', '***', '___', '  ---  ']) expect(noteLine(rule)).toEqual({ kind: 'rule' })
    expect(noteLine('----')).toEqual({ kind: 'paragraph', text: '----' })
    expect(noteLine('## Launch scope ')).toEqual({ kind: 'heading', text: 'Launch scope' })
    expect(noteLine('###### Deep')).toEqual({ kind: 'heading', text: 'Deep' })
    // Seven hashes, or a hash with no space after it, is a paragraph.
    expect(noteLine('####### No')).toEqual({ kind: 'paragraph', text: '####### No' })
    expect(noteLine('#hashtag')).toEqual({ kind: 'paragraph', text: '#hashtag' })
    expect(noteLine('- Item')).toEqual({ kind: 'list', text: 'Item', indent: 0, ordinal: null })
    for (const marker of ['*', '+', '•']) expect(noteLine(`${marker} Item`)).toMatchObject({ kind: 'list', ordinal: null })
    expect(noteLine('    - Nested')).toEqual({ kind: 'list', text: 'Nested', indent: 4, ordinal: null })
    expect(noteLine('\t- Tabbed')).toMatchObject({ kind: 'list', indent: 4 })
    expect(noteLine('3. Third')).toEqual({ kind: 'list', text: 'Third', indent: 0, ordinal: 3 })
    expect(noteLine('12) Twelfth')).toMatchObject({ kind: 'list', text: 'Twelfth', ordinal: 12 })
    // A marker needs its space, and a number its dot.
    expect(noteLine('-Item')).toEqual({ kind: 'paragraph', text: '-Item' })
    expect(noteLine('3 apples')).toEqual({ kind: 'paragraph', text: '3 apples' })
    expect(noteLine('v1. released')).toEqual({ kind: 'paragraph', text: 'v1. released' })
  })

  test('blocks keep nesting, numbering and the gaps between sections', () => {
    const blocks = noteBlocks('## Scope\n\n- One\n  - Nested\n    - Deeper\n- Two\n  Continues the item\n\n## Steps\n5. First\n6. Second\n   - Aside\n7. Third\n\nA paragraph\n---\n')
    expect(blocks.map(block => block.kind)).toEqual(['heading', 'list', 'list', 'list', 'list', 'paragraph', 'blank', 'heading', 'list', 'list', 'list', 'list', 'blank', 'paragraph', 'rule'])
    // The blank line after a heading is not a gap of its own; one before the next section is.
    expect(blocks[0]).toEqual({ kind: 'heading', row: 0, text: 'Scope' })
    expect(blocks.filter(block => block.kind === 'list').map(block => block.kind === 'list' && [block.text, block.depth, block.number])).toEqual([
      ['One', 0, null], ['Nested', 1, null], ['Deeper', 2, null], ['Two', 0, null], ['First', 0, 5], ['Second', 0, 6], ['Aside', 1, null], ['Third', 0, 7],
    ])
    expect(blocks[5]).toEqual({ kind: 'paragraph', row: 6, text: 'Continues the item', level: 1, continues: true })
    expect(blocks[13]).toMatchObject({ kind: 'paragraph', text: 'A paragraph', level: 0, continues: false })
    // Numbering continues from the first number of a run, whatever the later lines say.
    expect(noteBlocks('1. a\n1. b\n1. c').map(block => block.kind === 'list' && block.number)).toEqual([1, 2, 3])
    // Text between two runs starts the numbering again.
    expect(noteBlocks('1. a\n2. b\nBreak\n1. c').map(block => block.kind === 'list' ? block.number : null)).toEqual([1, 2, null, 1])
    // One level is the smallest indent the note uses, at most four columns; depth stops at eight.
    expect(listIndentUnit(['- a', '    - b', '        - c'])).toBe(4)
    expect(listIndentUnit(['- a', '  - b', '      - c'])).toBe(2)
    expect(listIndentUnit(['- a'])).toBe(2)
    expect(noteBlocks(`- a\n${' '.repeat(40)}- deep`)[1]).toMatchObject({ depth: 8 })
    expect(noteBlocks('')).toEqual([])
    expect(noteBlocks('one\r\ntwo\r\n').map(block => block.kind === 'paragraph' && block.text)).toEqual(['one', 'two'])
  })

  test('bold, code and emphasis; a delimiter without a partner stays as typed', () => {
    expect(inlineParts('plain')).toEqual([{ text: 'plain', style: 'plain' }])
    expect(inlineParts('**Bold** then `code` then *soft* and __also bold__')).toEqual([
      { text: 'Bold', style: 'strong' }, { text: ' then ', style: 'plain' }, { text: 'code', style: 'code' }, { text: ' then ', style: 'plain' },
      { text: 'soft', style: 'em' }, { text: ' and ', style: 'plain' }, { text: 'also bold', style: 'strong' },
    ])
    expect(inlineParts('**open and `tick')).toEqual([{ text: '**open and `tick', style: 'plain' }])
    // As in the app, two lone asterisks on a line are a pair.
    expect(inlineParts('2 * 3 * 4')).toEqual([{ text: '2 ', style: 'plain' }, { text: ' 3 ', style: 'em' }, { text: ' 4', style: 'plain' }])
    expect(inlineParts('\\*not soft\\*')).toEqual([{ text: '*not soft*', style: 'plain' }])
    expect(inlineParts('~$0.14 to ~$0.05')).toEqual([{ text: '~$0.14 to ~$0.05', style: 'plain' }])
    expect(inlineParts('')).toEqual([])
  })

  test('nothing in a note becomes markup or an image; only a web address written out in full becomes a link', () => {
    const html = renderToStaticMarkup(<NoteText value={'## Résumé 🦀 <b>x</b>\n\n- <img src=x onerror=alert(1)>\n- [unsafe](javascript:alert(1)) and [label](https://elsewhere.example) javascript:alert(2)\n\n<script>alert(1)</script>'} />)
    expect(html).toContain('Résumé')
    expect(html).toContain('&lt;img')
    expect(html).toContain('&lt;script&gt;')
    for (const tag of ['<img', '<script', '<b>', 'href="javascript']) expect(html).not.toContain(tag)
    // A Markdown link stays as written: its label could say anything. The address inside it is shown, and links, as itself.
    expect(html).toContain('[label](</span><a class="note-link" href="https://elsewhere.example/" target="_blank" rel="noopener noreferrer"><span class="w">https://elsewhere.example</span></a><span class="w">)')
    expect((html.match(/<a /g) ?? []).length).toBe(1)
  })

  test('the chat link at the foot of generated notes is a link that reads as the address it opens', () => {
    const address = 'https://notes.example.test/notes/51207249-97a9-4bd7-b02f-93428ffb367a/chat'
    const html = renderToStaticMarkup(<NoteText value={`# Notes\n- One\n\n---\n\nChat with meeting transcript: ${address} I CAN TYPE HERE`} />)
    expect(html).toContain('<hr/>')
    expect(html).toContain(`<a class="note-link" href="${address}" target="_blank" rel="noopener noreferrer">${address}</a>`)
    expect(html.replace(/<[^>]+>/g, '')).toContain(`Chat with meeting transcript: ${address} I CAN TYPE HERE`)
    // The line as generated, with nothing typed after it, is a short quiet link to the same address.
    const bare = renderToStaticMarkup(<NoteText value={`# Notes\n- One\n\n---\n\nChat with meeting transcript: ${address}`} />)
    expect(bare).toContain(`<p class="note-line note-transcript-link"><a class="note-link" href="${address}" target="_blank" rel="noopener noreferrer">Chat with meeting transcript ↗</a></p>`)
    expect(bare).not.toContain(`>${address}<`)
  })

  test('the addresses that become links are the same on every client (testdata/web-addresses.json)', async () => {
    const vectors = JSON.parse(await Bun.file(new URL('../../../../testdata/web-addresses.json', import.meta.url)).text()) as { cases: { text: string; links: string[] }[] }
    expect(vectors.cases.length).toBeGreaterThan(15)
    for (const { text, links } of vectors.cases) expect(webLinks(text).filter(piece => piece.href).map(piece => piece.text), JSON.stringify(text)).toEqual(links)
    // A look-alike is shown, never linked, on the page as well.
    const html = renderToStaticMarkup(<NoteText value={'- https://good.example@evil.example/login\n- https://\u0430pple.com/login'} />)
    expect(html).not.toContain('<a ')
  })

  test('web addresses in a line', () => {
    expect(webLinks('See https://a.example/x, then (https://b.example/y).')).toEqual([
      { text: 'See ', href: null }, { text: 'https://a.example/x', href: 'https://a.example/x' }, { text: ', then (', href: null },
      { text: 'https://b.example/y', href: 'https://b.example/y' }, { text: ').', href: null },
    ])
    expect(webLinks('no address here')).toEqual([{ text: 'no address here', href: null }])
    expect(webLinks('ftp://a.example and http://')).toEqual([{ text: 'ftp://a.example and http://', href: null }])
    expect(webLinks('')).toEqual([{ text: '', href: null }])
  })

  test('the page it makes: headings with their gutter mark, lists by depth, a continuation inside its item', () => {
    const html = renderToStaticMarkup(<NoteText sourced value={'## Next\n- **Send the guide** (Sam)\n  Deadline: Tuesday.\n  - Nested\n1. First\n\nDone.'} />)
    expect(html).toContain('<h2 class="is-first"><span class="note-hash" aria-hidden="true">#</span>')
    expect(html).toContain('<strong>')
    // Bullets and numbers are separate runs of the same list model.
    expect((html.match(/<ul>/g) ?? []).length).toBe(1)
    expect((html.match(/<li data-depth=/g) ?? []).length).toBe(3)
    expect(html).toContain('<li data-depth="1">')
    expect(html).toContain('<span class="note-number" style="left:-10px">1.</span>')
    // The line under the first item continues it, inside the same list item, at the item's own indent.
    expect(html).toMatch(/<li data-depth="0"><div class="note-item" style="padding-left:16px">.*?<\/div><p class="note-line has-source continues" style="margin-left:14px">/)
    // In a note with a transcript the app keeps room beside each line of words; not beside a bare symbol.
    expect(renderToStaticMarkup(<NoteText sourced value={'-\n- ?\n- Words'} />).match(/has-source/g)?.length).toBe(1)
    expect(renderToStaticMarkup(<NoteText value={'- Words'} />)).not.toContain('has-source')
  })

  test('a word keeps the space after it, so lines break where the app breaks them', () => {
    expect(wordChunks('and the installer is forty megabytes.')).toEqual(['and ', 'the ', 'installer ', 'is ', 'forty ', 'megabytes.'])
    expect(wordChunks('  two  spaces ')).toEqual(['  ', 'two  ', 'spaces '])
    expect(wordChunks('')).toEqual([])
    const html = renderToStaticMarkup(<NoteText value={'Ship **desktop and web** first'} />)
    // A break is offered only after a space, and never between a bold run and its own edge.
    expect(html).toContain('<span class="w">Ship </span><wbr/><strong><span class="w">desktop </span><wbr/><span class="w">and </span><wbr/><span class="w">web</span></strong><span class="w"> </span><wbr/><span class="w">first</span>')
    // A very long word may still break anywhere.
    const long = 'x'.repeat(80)
    expect(renderToStaticMarkup(<NoteText value={long} />)).toContain(`>${long}</p>`)
  })
})
