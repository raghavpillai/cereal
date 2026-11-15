/**
 * `GET /api/v1/notes/:id/live`, upgraded to a WebSocket.
 *
 * The route is registered on `Bun.serve` directly, outside the Effect HttpApi, because that API
 * describes request-response and server-sent-event endpoints only. Everything that can be refused
 * before the upgrade is refused there with an ordinary JSON error; close codes are for what happens
 * afterwards.
 */
import { Effect, Schema } from "effect"
import type { Server, ServerWebSocket, WebSocketHandler } from "bun"
import type { PrincipalData } from "@meeting-notes/contracts/api"
import { LIVE_MAX_FRAME_BYTES, LIVE_SAMPLE_RATES, LiveClientMessage, LiveCloseCode, decodeLiveAudioFrame, type LiveTranscriber } from "@meeting-notes/contracts/live"
import type { NotesRepositoryService } from "../repository"
import type { LiveHub } from "./hub"
import { closeSocket, LiveSessionRefusal, type LiveSession } from "./session"

export interface LiveConnection {
  kind: "live"
  noteId: string
  userId: string
  /** The credential presented at the upgrade, kept only to check that the session is still valid. */
  authorization: string
  transcriber: LiveTranscriber
  session?: LiveSession
  /** Set while `hello` is being processed; later frames wait for it. */
  joining?: Promise<void>
  helloTimer?: ReturnType<typeof setTimeout>
  authTimer?: ReturnType<typeof setInterval>
  closed?: boolean
}
export interface LiveEndpointOptions {
  repository: NotesRepositoryService
  hub: LiveHub
  resolvePrincipal: (headers: Headers) => Promise<PrincipalData | null>
  isTrustedOrigin: (origin: string) => boolean
  /** The configured streaming model, or null when live transcription is off. */
  transcriber: () => Promise<LiveTranscriber | null>
  helloTimeoutMs?: number
  reauthenticateMs?: number
}
const pattern = /^\/api\/v1\/notes\/([0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12})\/live$/
const headers = { "Cache-Control": "no-store", "Referrer-Policy": "no-referrer", "X-Content-Type-Options": "nosniff" }
const refuse = (status: number, error: string, message: string) => Response.json({ error, message }, { status, headers })
const decodeMessage = Schema.decodeUnknownSync(LiveClientMessage)

export function makeLiveEndpoint(options: LiveEndpointOptions) {
  const { hub } = options
  const close = (ws: ServerWebSocket<LiveConnection>, code: number, message?: string) => closeSocket(ws, code, undefined, message)
  const websocket: WebSocketHandler<LiveConnection> = {
    // Frames carry at most 64 KiB; the margin lets an oversized frame be answered with 4400 instead of
    // the transport's own 1009.
    maxPayloadLength: 256 * 1024,
    idleTimeout: 120,
    sendPings: true,
    open(ws) {
      const data = ws.data
      data.helloTimer = setTimeout(() => { if (!data.session && !data.joining) close(ws, LiveCloseCode.malformed, "No hello was received") }, options.helloTimeoutMs ?? 10_000)
      data.authTimer = setInterval(async () => {
        // The session may have been revoked or expired since the upgrade.
        const principal = await options.resolvePrincipal(new Headers({ authorization: data.authorization })).catch(() => null)
        if (!data.closed && principal?.id !== data.userId) close(ws, LiveCloseCode.unauthenticated, "Sign in again to continue")
      }, options.reauthenticateMs ?? 300_000)
    },
    async message(ws, message) {
      const data = ws.data
      if (typeof message !== "string") {
        if (message.byteLength > LIVE_MAX_FRAME_BYTES) { close(ws, LiveCloseCode.malformed, "Audio frame is larger than 64 KiB"); return }
        // Audio before `ready` is discarded: the client does not yet know where to send from.
        if (!data.session) return
        const frame = decodeLiveAudioFrame(new Uint8Array(message.buffer, message.byteOffset, message.byteLength))
        if (!frame) { close(ws, LiveCloseCode.malformed, "Malformed audio frame"); return }
        data.session.audio(frame)
        return
      }
      let decoded: LiveClientMessage
      try { decoded = decodeMessage(JSON.parse(message)) } catch { close(ws, LiveCloseCode.malformed, "Malformed message"); return }
      if (decoded.type === "hello") {
        if (data.session || data.joining) { close(ws, LiveCloseCode.malformed, "hello was sent twice"); return }
        clearTimeout(data.helloTimer)
        const hello = decoded
        data.joining = (async () => {
          try {
            // The recording may have been stopped between the upgrade and this frame.
            const note = await Effect.runPromise(options.repository.forUser(data.userId).getNote(data.noteId))
            if (note?.recordingState !== "recording") throw new LiveSessionRefusal(LiveCloseCode.notFound, "The note is not being recorded")
            // A rate no recogniser takes is the client's mistake, not an outage: 4400, not 4503.
            if (hello.streams.some(stream => !(LIVE_SAMPLE_RATES as ReadonlyArray<number>).includes(stream.sampleRate))) throw new LiveSessionRefusal(LiveCloseCode.malformed, "Unsupported sample rate")
            const session = await hub.connect(data.noteId, hello, ws, data.transcriber)
            if (data.closed) session.detach(ws); else data.session = session
          } catch (error) {
            close(ws, error instanceof LiveSessionRefusal ? error.code : LiveCloseCode.providerUnavailable, error instanceof LiveSessionRefusal ? error.message : "Live transcription is unavailable")
          } finally { data.joining = undefined }
        })()
        await data.joining
        return
      }
      if (data.joining) await data.joining
      if (!data.session) { close(ws, LiveCloseCode.malformed, "hello must be the first message"); return }
      data.session.mark(decoded)
    },
    drain(ws) { ws.data.session?.drained(ws) },
    close(ws) {
      const data = ws.data
      data.closed = true
      clearTimeout(data.helloTimer); clearInterval(data.authTimer)
      data.session?.detach(ws)
    },
  }
  return {
    /** The note ID when `path` is the live route. */
    match: (path: string) => pattern.exec(path)?.[1]?.toLowerCase() ?? null,
    /** Answers the upgrade request. Returns undefined once the connection has been upgraded. */
    async upgrade(request: Request, server: Server<LiveConnection>, noteId: string): Promise<Response | undefined> {
      if (request.method !== "GET") return new Response(null, { status: 405, headers: { ...headers, Allow: "GET" } })
      // Bearer only. Cookies are never read here, so a web page cannot open the socket with a
      // signed-in browser session, and an untrusted Origin is refused outright.
      const authorization = request.headers.get("authorization") ?? ""
      const origin = request.headers.get("origin")
      if (origin && !options.isTrustedOrigin(origin)) return refuse(403, "forbidden", "This origin cannot open a live transcription connection")
      if (!/^Bearer +\S/i.test(authorization)) return refuse(401, "unauthorized", "A bearer token is required")
      const principal = await options.resolvePrincipal(new Headers({ authorization })).catch(() => null)
      if (!principal) return refuse(401, "unauthorized", "Sign in again to continue")
      let note
      try { note = await Effect.runPromise(options.repository.forUser(principal.id).getNote(noteId)) } catch { return refuse(503, "unavailable", "The note service is temporarily unavailable") }
      if (!note) return refuse(404, "not_found", "Note not found")
      const transcriber = await options.transcriber().catch(() => null)
      if (!transcriber) return refuse(503, "unavailable", "Live transcription is not configured")
      if (note.recordingState !== "recording") return refuse(409, "conflict", "The note is not being recorded")
      if (hub.full(noteId)) return refuse(429, "unavailable", "Too many live transcription sessions")
      if (request.headers.get("upgrade")?.toLowerCase() !== "websocket") return refuse(426, "invalid_request", "This endpoint is a WebSocket")
      const data: LiveConnection = { kind: "live", noteId, userId: principal.id, authorization, transcriber }
      return server.upgrade(request, { data }) ? undefined : refuse(400, "invalid_request", "The WebSocket upgrade failed")
    },
    websocket,
  }
}
export type LiveEndpoint = ReturnType<typeof makeLiveEndpoint>
