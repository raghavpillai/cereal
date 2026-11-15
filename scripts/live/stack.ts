/**
 * The API with live transcription, in one process, on the in-memory repository: the real route, hub,
 * session, worker and chat code, with no PostgreSQL and no Docker. Used by the development server
 * (`scripts/live/dev-server.ts`) and the replay measurement (`scripts/replay-live.ts`).
 */
import { mkdtemp, rm } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { Effect } from "effect"
import type { ChatCitationDTO } from "@meeting-notes/contracts"
import type { LiveTranscriber } from "@meeting-notes/contracts/live"
import { makeApiHandler } from "../../apps/api/src/api"
import { chatAnswerMessages, meetingChatDocument } from "../../apps/api/src/ai"
import { bootstrapPrincipal, isBootstrapToken } from "../../apps/api/src/auth"
import { makeLiveHub } from "../../apps/api/src/live/hub"
import { makeNoteVoiceMatcher } from "../../apps/api/src/live/voices"
import { makeLiveEndpoint, type LiveConnection } from "../../apps/api/src/live/route"
import { makeElevenLabsRealtime } from "../../apps/api/src/live/providers/elevenlabs-realtime"
import { makeFakeTranscriber } from "../../apps/api/src/live/providers/fake"
import type { LiveConfig } from "../../apps/api/src/live/session"
import { makeProcessingService } from "../../apps/api/src/processing"
import { makeProviderServices, type ProviderNote, type ProviderServices } from "../../apps/api/src/providers"
import { makeMemoryRepository } from "../../apps/api/src/repository"

export const DEV_TOKEN = "cereal-live-dev-token-not-a-secret-0123456789"
export interface LiveStackOptions {
  port?: number
  hostname?: string
  token?: string
  /** "fake" needs no key. "elevenlabs" reads ELEVENLABS_API_KEY from the environment. */
  provider?: "fake" | "elevenlabs"
  model?: string
  /** "echo": the chat answer is the context the model would have been given, and no language model is
   * called. "provider": the configured language provider answers. */
  chat?: "echo" | "provider"
  fakeInterimDelayMs?: number
  fakeFinalDelayMs?: number
  config?: Partial<LiveConfig>
  /** Ask the language provider to report token usage, and record it per request (`languageUsage`). Only with `chat: "provider"`. */
  meterLanguage?: boolean
  /** `LIVE_MAX_SESSIONS` of this stack (the server's default is 20). */
  maxSessions?: number
  vadSilenceSeconds?: number
}
export interface RecordedChat { at: number; noteId: string; context: string; citations: ChatCitationDTO[]; live: unknown }
/** What the language provider reported for one request. `cachedTokens` is the part of the prompt it served from its cache. */
export interface LanguageUsage { question: string; model: string; messages: number; requestCharacters: number; promptTokens: number | null; cachedTokens: number | null; completionTokens: number | null; costUsd: number | null; firstTokenMs: number | null; totalMs: number }
/** Wraps `fetch` for chat-completion requests: asks for usage accounting and reads it from the response without changing what the caller sees. */
export function meteredFetch(records: LanguageUsage[], base: typeof fetch = fetch): typeof fetch {
  return (async (input: Parameters<typeof fetch>[0], init?: RequestInit) => {
    let request: Record<string, any> | undefined
    try { request = JSON.parse(String(init?.body)) } catch { /* not a JSON request */ }
    if (!request || !Array.isArray(request.messages)) return base(input, init)
    const started = performance.now(), body = JSON.stringify({ ...request, usage: { include: true }, ...(request.stream ? { stream_options: { include_usage: true } } : {}) })
    const record: LanguageUsage = { question: String(request.messages.at(-1)?.content ?? "").slice(0, 300), model: String(request.model ?? ""), messages: request.messages.length, requestCharacters: body.length, promptTokens: null, cachedTokens: null, completionTokens: null, costUsd: null, firstTokenMs: null, totalMs: 0 }
    records.push(record)
    const response = await base(input, { ...init, body })
    const take = (usage: Record<string, any> | undefined) => { if (!usage) return; record.promptTokens = usage.prompt_tokens ?? record.promptTokens; record.completionTokens = usage.completion_tokens ?? record.completionTokens; record.cachedTokens = usage.prompt_tokens_details?.cached_tokens ?? usage.cache_read_input_tokens ?? record.cachedTokens; record.costUsd = usage.cost ?? record.costUsd }
    if (!response.ok || !response.body) { record.totalMs = Math.round(performance.now() - started); return response }
    if (!request.stream) { const text = await response.text(); try { take(JSON.parse(text).usage) } catch { /* not JSON */ } record.totalMs = Math.round(performance.now() - started); return new Response(text, { status: response.status, headers: response.headers }) }
    const [forCaller, forMeter] = response.body.tee()
    void (async () => {
      const reader = forMeter.getReader(), decoder = new TextDecoder()
      let buffer = ""
      for (;;) {
        const next = await reader.read().catch(() => ({ done: true as const, value: undefined }))
        if (next.done) break
        buffer += decoder.decode(next.value, { stream: true })
        for (let end = buffer.indexOf("\n\n"); end >= 0; end = buffer.indexOf("\n\n")) {
          const data = buffer.slice(0, end).split("\n").filter(line => line.startsWith("data:")).map(line => line.slice(5).trim()).join(""); buffer = buffer.slice(end + 2)
          if (!data || data === "[DONE]") continue
          try { const frame = JSON.parse(data); if (record.firstTokenMs === null && frame.choices?.[0]?.delta?.content) record.firstTokenMs = Math.round(performance.now() - started); take(frame.usage) } catch { /* partial frame */ }
        }
      }
      record.totalMs = Math.round(performance.now() - started)
    })()
    return new Response(forCaller, { status: response.status, headers: response.headers })
  }) as typeof fetch
}

export async function startLiveStack(options: LiveStackOptions = {}) {
  const token = options.token ?? DEV_TOKEN, provider = options.provider ?? "fake"
  const repository = makeMemoryRepository()
  const storageDir = await mkdtemp(join(tmpdir(), "cereal-live-"))
  const upstream = { seconds: 0, sessions: 0 }, batch = { calls: 0, seconds: 0 }, chats: RecordedChat[] = []
  let transcriber: LiveTranscriber
  if (provider === "elevenlabs") {
    if (!process.env.ELEVENLABS_API_KEY) throw new Error("ELEVENLABS_API_KEY is not set; run with bun --env-file=.env")
    transcriber = makeElevenLabsRealtime({ apiKey: () => process.env.ELEVENLABS_API_KEY!, model: options.model ?? "scribe_v3_live", ...(options.vadSilenceSeconds === undefined ? {} : { vadSilenceSeconds: options.vadSilenceSeconds }) })
  } else transcriber = makeFakeTranscriber({ ...(options.fakeInterimDelayMs === undefined ? {} : { interimDelayMs: options.fakeInterimDelayMs }), ...(options.fakeFinalDelayMs === undefined ? {} : { finalDelayMs: options.fakeFinalDelayMs }) })
  // Meter what goes upstream, so a run can state its cost: every sample sent is billed, silence included.
  const metered: LiveTranscriber = { provider: transcriber.provider, model: transcriber.model, async open(open) {
    const session = await transcriber.open(open)
    upstream.sessions++
    return { events: session.events, get bufferedBytes() { return session.bufferedBytes }, flush: () => session.flush(), close: () => session.close(),
      send(pcm) { upstream.seconds += pcm.byteLength / 2 / open.sampleRate; session.send(pcm) },
      keepAlive() { const seconds = session.keepAlive(); upstream.seconds += seconds; return seconds } }
  } }
  const languageUsage: LanguageUsage[] = []
  const real = provider === "elevenlabs" || options.chat === "provider" ? makeProviderServices(options.meterLanguage ? { fetch: meteredFetch(languageUsage) } : {}) : undefined
  const providers: ProviderServices = {
    // With the fake recogniser there is no batch recogniser either: audio the stream missed yields no text.
    transcribe: input => { batch.calls++; batch.seconds += input.durationMs / 1000; return provider === "elevenlabs" ? real!.transcribe(input) : Effect.succeed([]) },
    generate: note => real ? real.generate(note) : Effect.succeed("# Notes\n\n- Generated notes are not available on the development server."),
    chat: (note, input) => real ? real.chat(note, input) : Effect.succeed("Chat is not available on the development server."),
    async *chatStream(note: ProviderNote, input, signal) {
      const messages = chatAnswerMessages(note, input), prepared = note as ProviderNote & { meetingChat?: { citations: ChatCitationDTO[] }; live?: unknown }
      // The request is several messages (the transcript of a long meeting is cut into pieces, a thread appends to
      // itself); `context` is the one-document view of it: one <transcript>, then the newest message's other blocks.
      const context = meetingChatDocument(messages)
      chats.push({ at: performance.now(), noteId: note.id, context, citations: prepared.meetingChat?.citations ?? [], live: prepared.live ?? null })
      if ((options.chat ?? "echo") === "echo") { yield context; return }
      yield* real!.chatStream!(note, input, signal)
    },
  }
  // Comparisons of voices across provider sessions go to the batch recogniser too; they are counted on their own as well.
  const voiceChecks = { calls: 0, seconds: 0 }
  const voices = makeNoteVoiceMatcher(repository, input => { voiceChecks.calls++; voiceChecks.seconds += input.durationMs / 1000; return providers.transcribe(input) }, storageDir)
  const hub = makeLiveHub(repository, { ...(options.config ? { config: options.config } : {}), ...(options.maxSessions ? { maxSessions: options.maxSessions } : {}), ...(provider === "elevenlabs" ? { voices } : {}) })
  const processing = makeProcessingService(repository, { providers, storageDir, live: hub })
  const api = makeApiHandler(repository, token, { processing })
  const endpoint = makeLiveEndpoint({
    repository, hub, transcriber: async () => metered,
    resolvePrincipal: async headers => isBootstrapToken(headers, token) ? bootstrapPrincipal : null,
    isTrustedOrigin: origin => /^http:\/\/(?:localhost|127\.0\.0\.1)(?::\d+)?$/.test(origin),
  })
  const server = Bun.serve<LiveConnection>({
    port: options.port ?? 0, hostname: options.hostname ?? "127.0.0.1", maxRequestBodySize: 29 * 1024 * 1024, idleTimeout: 255, websocket: endpoint.websocket,
    async fetch(request, server) {
      const path = new URL(request.url).pathname, noteId = endpoint.match(path)
      if (noteId) return endpoint.upgrade(request, server, noteId)
      if (path === "/api/v1/ready") return Response.json({ status: "ready" })
      if (path === "/api" || path.startsWith("/api/")) return api.handler(request)
      return new Response("Cereal live development server", { status: 404 })
    },
  })
  return {
    repository, hub, processing, server, token, upstream, batch, voiceChecks, chats, languageUsage, provider: transcriber.provider, model: transcriber.model,
    base: `http://127.0.0.1:${server.port}`,
    async stop() {
      await hub.dispose()
      void server.stop(true)
      await api.dispose()
      await rm(storageDir, { recursive: true, force: true })
    },
  }
}
export type LiveStack = Awaited<ReturnType<typeof startLiveStack>>
