/**
 * The API with live transcription for the accuracy harness, in this process: the real route, hub,
 * session and repository code on the in-memory repository (or PostgreSQL when `DATABASE_URL` points at
 * a throwaway database), with what the scenarios need on top:
 *
 *  - `restart(downMs)`: the process dies (sessions abandoned, nothing flushed, as a crash or a deploy
 *    does) and comes back on the same port with the same stored notes;
 *  - `dropProvider()`: every open provider session reports a retryable error and closes, as when the
 *    speech provider drops its socket;
 *  - a network proxy (proxy.ts) in front of it, for the client side of the network.
 *
 * Nothing here talks to a deployed server: the base URL is always 127.0.0.1.
 */
import { mkdtemp, rm } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { Effect } from "effect"
import type { LiveTranscriber, LiveTranscriberEvent, LiveTranscriberSession } from "@meeting-notes/contracts/live"
import { makeApiHandler } from "../../apps/api/src/api"
import { bootstrapPrincipal, isBootstrapToken } from "../../apps/api/src/auth"
import { makeLiveHub, type LiveHub } from "../../apps/api/src/live/hub"
import { makeLiveEndpoint, type LiveConnection } from "../../apps/api/src/live/route"
import { makeElevenLabsRealtime } from "../../apps/api/src/live/providers/elevenlabs-realtime"
import { makeFakeTranscriber, type FakeWord } from "../../apps/api/src/live/providers/fake"
import { EventQueue } from "../../apps/api/src/live/providers/queue"
import type { LiveConfig } from "../../apps/api/src/live/session"
import { makeProcessingService } from "../../apps/api/src/processing"
import type { ProviderServices } from "../../apps/api/src/providers"
import { makeMemoryRepository, type NotesRepositoryService } from "../../apps/api/src/repository"

export const TOKEN = "cereal-accuracy-token-not-a-secret-0123456789abcdef"

/** A transcriber whose open sessions can be made to fail, and that counts what it was sent. */
function droppable(base: LiveTranscriber, legacyKeepAlive = false) {
  const open = new Set<{ drop(): void }>()
  const meter = { sessions: 0, seconds: 0, drops: 0 }
  /** Every final the provider produced, as it produced it: what the stored transcript is compared with. */
  const finals: Array<{ at: number; channel: string; session: number; text: string; language?: string; startSec: number; endSec: number; flushed?: boolean }> = []
  const transcriber: LiveTranscriber = {
    provider: base.provider, model: base.model,
    async open(options) {
      const inner = await base.open(options)
      meter.sessions++
      const number = meter.sessions
      const events = new EventQueue<LiveTranscriberEvent>()
      let dropped = false
      const handle = { drop() { if (dropped) return; dropped = true; meter.drops++; events.push({ type: "error", retryable: true, reason: "unavailable", detail: "harness_drop" }); events.close(); void inner.close() } }
      open.add(handle)
      void (async () => { try { for await (const event of inner.events) { if (dropped) break; if (event.type === "final") finals.push({ at: performance.now(), channel: options.channel, session: number, text: event.text, ...(event.language ? { language: event.language } : {}), startSec: event.startSec, endSec: event.endSec, ...(event.flushed === undefined ? {} : { flushed: event.flushed }) }); events.push(event) } } catch { /* ended */ } open.delete(handle); if (!dropped) events.close() })()
      const session: LiveTranscriberSession = {
        events,
        get bufferedBytes() { return inner.bufferedBytes },
        get queuedSeconds() { return inner.queuedSeconds },
        send(pcm) { if (!dropped) { meter.seconds += pcm.byteLength / 2 / options.sampleRate; inner.send(pcm) } },
        flush() { if (!dropped) inner.flush() },
        keepAlive() {
          if (dropped) return 0
          // Before 2026-11-17 the ElevenLabs keep-alive was 100 ms of silence, in band.
          if (legacyKeepAlive) { const silence = new Uint8Array(Math.round(options.sampleRate / 10) * 2); meter.seconds += 0.1; inner.send(silence); return 0.1 }
          const seconds = inner.keepAlive(); meter.seconds += seconds; return seconds
        },
        async close() { open.delete(handle); dropped = true; events.close(); await inner.close() },
      }
      return session
    },
  }
  return { transcriber, meter, finals, dropAll() { for (const handle of [...open]) handle.drop() } }
}

export interface HarnessServerOptions {
  provider: "fake" | "elevenlabs"
  /** Fake: the scripted vocabulary (fixture words coded as samples). */
  vocabulary?: FakeWord[]
  model?: string
  config?: Partial<LiveConfig>
  repository?: NotesRepositoryService
  /** Seconds of silence a fake keep-alive adds: 0 like the ElevenLabs adapter, 0.1 like it was before 2026-11-17. */
  fakeKeepAliveSeconds?: number
  /** Keep-alives as 100 ms of in-band silence, as before 2026-11-17 (for before/after runs). */
  legacyKeepAlive?: boolean
}
export async function startHarnessServer(options: HarnessServerOptions) {
  const repository = options.repository ?? makeMemoryRepository()
  const storageDir = await mkdtemp(join(tmpdir(), "cereal-accuracy-"))
  let base: LiveTranscriber
  if (options.provider === "elevenlabs") {
    if (!process.env.ELEVENLABS_API_KEY) throw new Error("ELEVENLABS_API_KEY is not set; run with bun --env-file=.env")
    base = makeElevenLabsRealtime({ apiKey: () => process.env.ELEVENLABS_API_KEY!, model: options.model ?? "scribe_v3_live" })
  } else base = makeFakeTranscriber({ ...(options.vocabulary ? { vocabulary: options.vocabulary } : {}), sessionSpeakers: true, keepAliveSeconds: options.fakeKeepAliveSeconds ?? 0 })
  const provider = droppable(base, options.legacyKeepAlive)
  const providers: ProviderServices = { generate: () => Effect.succeed("# Notes"), chat: () => Effect.succeed("-"), async *chatStream() { yield "-" } }
  /** The protocol as the server saw it: hellos, and everything but audio acks, interims and finals it sent. */
  const protocol: Array<{ at: number; connection: number; direction: "in" | "out" | "close"; message: unknown }> = []
  let connections = 0
  let hub: LiveHub, server: ReturnType<typeof Bun.serve<LiveConnection>> | undefined, api: ReturnType<typeof makeApiHandler>, port = 0
  const up = () => {
    hub = makeLiveHub(repository, { ...(options.config ? { config: options.config } : {}) })
    const processing = makeProcessingService(repository, { providers, storageDir, live: hub })
    api = makeApiHandler(repository, TOKEN, { processing })
    const endpoint = makeLiveEndpoint({ repository, hub, transcriber: async () => provider.transcriber,
      resolvePrincipal: async headers => isBootstrapToken(headers, TOKEN) ? bootstrapPrincipal : null, isTrustedOrigin: () => false })
    const handler = endpoint.websocket, tagged = new WeakMap<object, number>()
    const websocket: typeof handler = {
      ...handler,
      open(ws) {
        const connection = ++connections, send = ws.send.bind(ws)
        tagged.set(ws, connection)
        ws.send = ((data: string | Uint8Array, compress?: boolean) => {
          if (typeof data === "string" && !/"type":"(ack|interim|final|status)"/.test(data)) protocol.push({ at: performance.now(), connection, direction: "out", message: JSON.parse(data) })
          return send(data as string, compress)
        }) as typeof ws.send
        return handler.open?.(ws)
      },
      message(ws, message) {
        if (typeof message === "string") protocol.push({ at: performance.now(), connection: tagged.get(ws) ?? 0, direction: "in", message: JSON.parse(message) })
        return handler.message(ws, message)
      },
      close(ws, code, reason) { protocol.push({ at: performance.now(), connection: tagged.get(ws) ?? 0, direction: "close", message: { code, reason } }); return handler.close?.(ws, code, reason) },
    }
    server = Bun.serve<LiveConnection>({
      port, hostname: "127.0.0.1", idleTimeout: 255, websocket,
      async fetch(request, srv) {
        const path = new URL(request.url).pathname, noteId = endpoint.match(path)
        if (noteId) return endpoint.upgrade(request, srv, noteId)
        if (path.startsWith("/api/")) return api.handler(request)
        return new Response("not found", { status: 404 })
      },
    })
    port = server.port!
  }
  up()
  const call = async (path: string, method = "GET", body?: unknown) => {
    const response = await fetch(`http://127.0.0.1:${port}/api/v1${path}`, { method, headers: { Authorization: `Bearer ${TOKEN}`, ...(body === undefined ? {} : { "Content-Type": "application/json" }) }, ...(body === undefined ? {} : { body: JSON.stringify(body) }) })
    const text = await response.text()
    if (!response.ok) throw new Error(`${method} ${path} answered ${response.status}: ${text.slice(0, 200)}`)
    return text ? JSON.parse(text) : null
  }
  return {
    repository, meter: provider.meter, providerFinals: provider.finals, protocol,
    get port() { return port },
    get hub() { return hub },
    call,
    dropProvider: () => provider.dropAll(),
    /** The process dies and comes back after `downMs` on the same port. */
    async restart(downMs: number) {
      await hub.dispose({ abandon: true })
      server?.stop(true)
      server = undefined
      await Bun.sleep(downMs)
      up()
    },
    async stop() {
      await hub.dispose()
      server?.stop(true)
      await api.dispose()
      await rm(storageDir, { recursive: true, force: true })
    },
  }
}
export type HarnessServer = Awaited<ReturnType<typeof startHarnessServer>>
