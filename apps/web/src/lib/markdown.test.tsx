import { describe, expect, test } from 'bun:test'
import { renderToStaticMarkup } from 'react-dom/server'
import { Markdown, safeLink } from './markdown'

describe('shared document rendering', () => {
  test('untrusted HTML is text and active URL schemes never become links', () => {
    const html = renderToStaticMarkup(<Markdown value={'<img src=x onerror=alert(1)>\n\n[unsafe](javascript:alert(1)) [safe](https://example.com/docs)'} />)
    expect(html).toContain('&lt;img')
    expect(html).not.toContain('<img')
    expect(html).not.toContain('href="javascript:')
    expect(html).toContain('href="https://example.com/docs"')
    expect(html).toContain('noopener noreferrer nofollow')
  })
  test('notes preserve Unicode, sections, lists, quotations and fenced code', () => {
    const html = renderToStaticMarkup(<Markdown value={'## Résumé 🦀\n\n- Café\n- 漢字\n\n> A shared decision\n\n```txt\n<script>alert(1)</script>\n```'} />)
    expect(html).toContain('Résumé 🦀')
    expect(html).toContain('<ul>')
    expect(html).toContain('<blockquote>')
    expect(html).toContain('&lt;script&gt;')
    expect(html).not.toContain('<script>')
  })
  test('control characters and unsupported or relative protocols are rejected', () => {
    for (const value of ['javascript:alert(1)', 'data:text/html,unsafe', 'file:///etc/passwd', '//example.com', 'https:\n//example.com']) expect(safeLink(value)).toBeUndefined()
    expect(safeLink('https://example.com')).toBe('https://example.com/')
    expect(safeLink('#decision')).toBe('#decision')
  })
  test('nested meeting bullets, continuation lines and ordered starts retain their structure', () => {
    const html = renderToStaticMarkup(<Markdown value={'- Alex\n  Own mobile recording\n  - Send the checklist\n    5. Review accessibility\n    6. Test keyboard controls\n- Jamie\n  - Verify diarization\n\n3. Release\n4. Monitor'} />)
    expect(html).toContain('<li>Alex<br/>Own mobile recording<ul><li>Send the checklist<ol start="5"><li>Review accessibility</li><li>Test keyboard controls</li></ol></li></ul></li>')
    expect(html).toContain('<li>Jamie<ul><li>Verify diarization</li></ul></li>')
    expect(html).toContain('<ol start="3"><li>Release</li><li>Monitor</li></ol>')
  })
  test('mixed list kinds and blank separators keep sibling and child ownership', () => {
    const html = renderToStaticMarkup(<Markdown value={'- Parent\n  - Child\n\n  2. Ordered child\n\n- Sibling\n\nFollowing paragraph'} />)
    expect(html).toContain('<li>Parent<ul><li>Child</li></ul><ol start="2"><li>Ordered child</li></ol></li><li>Sibling</li>')
    expect(html).toContain('</ul><p>Following paragraph</p>')
  })
  test('deep public lists are bounded and nested untrusted text stays inert', () => {
    const source = Array.from({ length: 60 }, (_, depth) => `${'  '.repeat(depth)}- <script>${depth}</script> [unsafe](javascript:alert)`).join('\n')
    const html = renderToStaticMarkup(<Markdown value={source} />)
    expect((html.match(/<ul>/g) ?? []).length).toBe(25)
    expect(html).toContain('&lt;script&gt;59&lt;/script&gt;')
    expect(html).not.toContain('<script>')
    expect(html).not.toContain('href="javascript:')
  })
  test('tables, task lists, nested emphasis, strikethrough and bare addresses render as structure', () => {
    const html = renderToStaticMarkup(<Markdown value={'Owners:\n| Task | Owner | Due |\n| :-- | :-: | --: |\n| **Tiles** | Wren | 21st |\n| Checklist | <b>Tobias</b> |\n\n- [ ] Draft the checklist\n- [x] Freeze the palette\n- plain item\n\n**bold with *italics* inside** and _underscore emphasis_ and ~~dropped~~, but snake_case_name and 2*3*4 stay.\n\nSee https://example.com/notes?id=1, or (https://example.com/a). ![chart](https://example.com/chart.png) ![x](javascript:alert(1))'} />)
    expect(html).toContain('<p>Owners:</p><figure class="table-wrap"')
    expect(html).toContain('<th scope="col" style="text-align:left">Task</th><th scope="col" style="text-align:center">Owner</th><th scope="col" style="text-align:right">Due</th>')
    expect(html).toContain('<td style="text-align:left"><strong>Tiles</strong></td>')
    // A short row is padded, and markup in a cell stays text.
    expect(html).toContain('<td style="text-align:center">&lt;b&gt;Tobias&lt;/b&gt;</td><td style="text-align:right"></td>')
    expect(html).toContain('<li class="task-item"><span class="task-box" aria-hidden="true"></span><span class="sr-only">To do: </span>Draft the checklist</li>')
    expect(html).toContain('<li class="task-item"><span class="task-box" data-checked="" aria-hidden="true"></span><span class="sr-only">Done: </span>Freeze the palette</li>')
    expect(html).toContain('<li>plain item</li>')
    expect(html).toContain('<strong>bold with <em>italics</em> inside</strong> and <em>underscore emphasis</em> and <del>dropped</del>, but snake_case_name and 2<em>3</em>4 stay.')
    expect(html).toContain('See <a href="https://example.com/notes?id=1" target="_blank" rel="noopener noreferrer nofollow">https://example.com/notes?id=1</a>, or (<a href="https://example.com/a" target="_blank" rel="noopener noreferrer nofollow">https://example.com/a</a>).')
    expect(html).toContain('<a href="https://example.com/chart.png" target="_blank" rel="noopener noreferrer nofollow">chart</a>')
    expect(html).not.toContain('<img')
    expect(html).not.toContain('href="javascript:')
    expect(html).not.toContain('<b>')
  })
  test('inline nesting is bounded and a lone rule or pipe is not a table', () => {
    // The grammar nests at most a few levels (bold, strike, emphasis, link text); a wall of markers still ends.
    const nested = renderToStaticMarkup(<Markdown value={'**a ~~b *c [d `e`](https://example.com) c* b~~ a**'} />)
    expect(nested).toContain('<strong>a <del>b <em>c <a href="https://example.com/" target="_blank" rel="noopener noreferrer nofollow">d <code>e</code></a> c</em> b</del> a</strong>')
    const wall = renderToStaticMarkup(<Markdown value={'**~~*__'.repeat(400) + 'x' + '__*~~**'.repeat(400)} />)
    expect(wall).toContain('x')
    expect(wall).not.toContain('<script')
    const html = renderToStaticMarkup(<Markdown value={'a | b\n\nnot a table\n---\n\n| only header |\n| --- |'} />)
    expect(html).not.toContain('<table')
    expect(html).toContain('<p>a | b</p>')
  })
})
