import type { ChatCitationDTO, ChatInput, ChatStreamEventDTO } from '@meeting-notes/contracts'

/** Chat with one meeting's transcript: the page linked from the foot of its generated notes
 * (`/notes/<id>/chat`). It asks the note's own chat (`POST /api/v1/notes/<id>/chat/stream`), so it needs
 * exactly the access the note needs: the owner's session on this server. Nothing is kept: a thread lives
 * as long as the page. */

export class ChatError extends Error {
  constructor(readonly status: number, message: string) { super(message) }
}

export interface ChatTurn { role: 'user' | 'assistant'; content: string; citations?: readonly ChatCitationDTO[] }

/** The events of a server-sent stream, read as they arrive. A keep-alive comment is not an event. */
export async function* sseEvents(body: ReadableStream<Uint8Array>): AsyncGenerator<string> {
  const reader = body.getReader(), decoder = new TextDecoder()
  let buffer = ''
  try {
    for (;;) {
      const { done, value } = await reader.read()
      buffer += decoder.decode(value, { stream: !done })
      let end: RegExpExecArray | null
      while ((end = /\r?\n\r?\n/.exec(buffer))) {
        const block = buffer.slice(0, end.index); buffer = buffer.slice(end.index + end[0].length)
        const data = block.split(/\r?\n/).filter(line => line.startsWith('data:')).map(line => line.slice(5).replace(/^ /, '')).join('\n')
        if (data) yield data
      }
      if (done) return
    }
  } finally { reader.releaseLock() }
}

/** The thread as the server takes it back: the last 30 turns, each within its limit. */
export function chatHistory(turns: readonly ChatTurn[]): ChatInput['history'] {
  return turns.filter(turn => turn.content.trim()).slice(-30).map(turn => ({ role: turn.role, content: turn.content.slice(0, 20_000) }))
}

/** Ask one question. `onEvent` sees every event; the promise ends with the stream. */
export async function askTranscript(noteId: string, question: string, history: readonly ChatTurn[], onEvent: (event: ChatStreamEventDTO) => void, options: { signal?: AbortSignal; transport?: typeof fetch; origin?: string } = {}): Promise<void> {
  const origin = options.origin ?? window.location.origin
  const timeZone = (() => { try { return Intl.DateTimeFormat().resolvedOptions().timeZone } catch { return undefined } })()
  const payload: ChatInput = { message: question, history: chatHistory(history), includeSources: true, ...(timeZone ? { timeZone } : {}) }
  const response = await (options.transport ?? fetch)(`${origin}/api/v1/notes/${encodeURIComponent(noteId)}/chat/stream`, {
    method: 'POST', credentials: 'same-origin', cache: 'no-store', redirect: 'error', referrerPolicy: 'no-referrer',
    headers: { 'Content-Type': 'application/json', Accept: 'text/event-stream' }, body: JSON.stringify(payload), signal: options.signal,
  })
  if (!response.ok || !response.body) {
    const message = await response.json().then((body: { message?: unknown }) => typeof body.message === 'string' ? body.message : '', () => '')
    throw new ChatError(response.status, response.status === 404 ? 'This note is not one you can open.' : message || 'The answer could not be started. Please try again.')
  }
  for await (const data of sseEvents(response.body)) {
    let event: ChatStreamEventDTO
    try { event = JSON.parse(data) as ChatStreamEventDTO } catch { continue }
    onEvent(event)
  }
}

/** An answer as it is read: each `[[S12]]` the model cited becomes its number in the list of sources under the
 * answer, in the order they first appear. A marker with no source behind it is left out. */
export function numberedCitations(answer: string, citations: readonly ChatCitationDTO[]): { text: string; sources: { number: number; citation: ChatCitationDTO }[] } {
  const known = new Map(citations.map(citation => [citation.id, citation])), order: string[] = []
  const text = answer.replace(/\s?\[\[(S[1-9][0-9]{0,2})\]\]/g, (_, id: string) => {
    if (!known.has(id)) return ''
    if (!order.includes(id)) order.push(id)
    return ` [${order.indexOf(id) + 1}]`
  })
  return { text, sources: order.map((id, index) => ({ number: index + 1, citation: known.get(id)! })) }
}

/** `m:ss` or `h:mm:ss` into the meeting. */
export function clock(ms: number): string {
  const seconds = Math.max(0, Math.floor(ms / 1000)), hours = Math.floor(seconds / 3600), minutes = Math.floor(seconds % 3600 / 60), rest = String(seconds % 60).padStart(2, '0')
  return hours ? `${hours}:${String(minutes).padStart(2, '0')}:${rest}` : `${minutes}:${rest}`
}
