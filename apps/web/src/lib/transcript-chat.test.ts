import { describe, expect, test } from 'bun:test'
import type { ChatCitationDTO, ChatStreamEventDTO } from '@meeting-notes/contracts'
import { ChatError, askTranscript, chatHistory, clock, numberedCitations, sseEvents } from './transcript-chat'

const NOTE = '51207249-97a9-4bd7-b02f-93428ffb367a'
const stream = (...chunks: string[]) => new ReadableStream<Uint8Array>({ start(output) { for (const chunk of chunks) output.enqueue(new TextEncoder().encode(chunk)); output.close() } })
const citation = (id: string, startMs: number | null = 42_000): ChatCitationDTO => ({ id, noteId: NOTE, title: 'Supplier call', source: 'transcript', segmentId: null, startMs, quote: 'Windrush quoted 612 a tonne.', transcriptRevision: 1 })

describe('chat with a meeting transcript', () => {
  test('server-sent events are read whole, however the bytes arrive, and keep-alives are skipped', async () => {
    const read: string[] = []
    for await (const data of sseEvents(stream(': keep-alive\n\n', 'data: {"type":"delta","te', 'xt":"Zoë 👋"}\r\n\r\ndata: {"type":"done","text":""}\n\n'))) read.push(data)
    expect(read).toEqual(['{"type":"delta","text":"Zoë 👋"}', '{"type":"done","text":""}'])
  })

  test('a question goes to the note\'s own chat on this origin, with its thread, and the answer streams back', async () => {
    const sent: Request[] = []
    const transport = (async (input: RequestInfo | URL, init?: RequestInit) => {
      sent.push(new Request(input, init))
      return new Response(stream('data: {"type":"delta","text":"612 a tonne [[S1]]"}\n\n', `data: ${JSON.stringify({ type: 'citations', text: '', citations: [citation('S1')] })}\n\n`, 'data: {"type":"done","text":""}\n\n'), { headers: { 'Content-Type': 'text/event-stream' } })
    }) as typeof fetch
    const events: ChatStreamEventDTO[] = []
    await askTranscript(NOTE, 'What did Windrush quote?', [{ role: 'user', content: 'Hi' }, { role: 'assistant', content: 'Hello' }], event => events.push(event), { transport, origin: 'https://notes.example.test' })
    expect(sent[0]!.url).toBe(`https://notes.example.test/api/v1/notes/${NOTE}/chat/stream`)
    expect(sent[0]!.method).toBe('POST')
    expect(await sent[0]!.json()).toMatchObject({ message: 'What did Windrush quote?', history: [{ role: 'user', content: 'Hi' }, { role: 'assistant', content: 'Hello' }], includeSources: true })
    expect(events.map(event => event.type)).toEqual(['delta', 'citations', 'done'])
  })

  test('a note that is not the reader\'s answers not found, and the page says so without showing anything of it', async () => {
    const transport = (async () => Response.json({ error: 'not_found', message: 'Note not found' }, { status: 404 })) as unknown as typeof fetch
    const failure = await askTranscript(NOTE, 'Read it', [], () => {}, { transport, origin: 'https://notes.example.test' }).catch(error => error)
    expect(failure).toBeInstanceOf(ChatError)
    expect(failure.status).toBe(404)
  })

  test('citations are numbered in the order they are first used; unknown markers go', () => {
    const { text, sources } = numberedCitations('Windrush [[S4]] at 612 [[S2]], from January [[S4]] [[S9]].', [citation('S2'), citation('S4')])
    expect(text).toBe('Windrush [1] at 612 [2], from January [1].')
    expect(sources.map(source => [source.number, source.citation.id])).toEqual([[1, 'S4'], [2, 'S2']])
  })

  test('the thread sent back is the last 30 turns with text', () => {
    const turns = Array.from({ length: 40 }, (_, index) => ({ role: index % 2 ? 'assistant' as const : 'user' as const, content: index === 39 ? ' ' : `turn ${index}` }))
    const history = chatHistory(turns)
    expect(history).toHaveLength(30)
    expect(history.at(-1)!.content).toBe('turn 38')
    expect(clock(42_000)).toBe('0:42')
    expect(clock(3_723_000)).toBe('1:02:03')
  })
})
