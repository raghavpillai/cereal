import { makeNoteParticipantsService } from "./note-participants"
import { makeGoogleService } from "./google"
import { makePrismaGoogleStore } from "./google-store"
import { loadAuthenticationSecret, makeAuthenticationService } from "./auth"
import { loadSecretBox } from "./admin-secrets"
import { makeAdministrationService, settingsAwareProviders } from "./administration"
import { makePrismaCalendarStore } from "./calendar-store"
import { resolve } from "node:path"
import { makeProcessingService } from "./processing"
import { makeApiHandler } from "./api"
import { makePrismaRepository } from "./prisma-repository"
import { makeWebHandler } from "./static"
import {Effect} from "effect"
import {createMcpStore} from "./mcp-store"
import {createMcpToolExecutor} from "./mcp-tools"
import {makeMcpEndpoint} from "./mcp"
import {makeLiveHub} from "./live/hub"
import {makeLiveEndpoint,type LiveConnection} from "./live/route"
import {makeLiveTranscriberResolver} from "./live/configuration"
import {makeNoteVoiceMatcher} from "./live/voices"

const databaseUrl = process.env.DATABASE_URL
const adminToken = process.env.ADMIN_API_TOKEN
if (!databaseUrl || !adminToken || adminToken.length < 32 || adminToken.startsWith("replace-")) {
  throw new Error("Configure DATABASE_URL and a random ADMIN_API_TOKEN of at least 32 characters; run scripts/setup.sh for local setup")
}
const port = Number(process.env.PORT ?? 3000)
if (!Number.isInteger(port) || port < 1 || port > 65535) throw new Error("Invalid PORT")
const { repository, database, ready, close } = makePrismaRepository(databaseUrl)
const secretBox = await loadSecretBox(process.env.SETTINGS_KEY_FILE ?? resolve(process.env.AUDIO_STORAGE_DIR ?? ".local/audio", "../state/settings.key"))
const administration = makeAdministrationService(makePrismaCalendarStore(database), secretBox, { worker: true })
// Live transcription sessions live in this process only.
const liveSessionLimit = Number(process.env.LIVE_MAX_SESSIONS ?? 20)
const providers = settingsAwareProviders(administration)
// LIVE_HOLD_SECONDS: how long a note's provider sessions are kept after Stop so a Resume keeps its speaker
// labels (default 300; 0 turns it off). A held session is billed for keep-alive audio only but occupies one of
// the provider's concurrent sessions per channel. After the hold, voices are told apart from the note's stored
// audio by the batch recogniser (live/voices.ts).
const liveHoldSeconds = Number(process.env.LIVE_HOLD_SECONDS ?? "")
const liveHub = makeLiveHub(repository, { maxSessions: Number.isInteger(liveSessionLimit) && liveSessionLimit > 0 ? liveSessionLimit : 20,
  ...(process.env.LIVE_HOLD_SECONDS && Number.isFinite(liveHoldSeconds) && liveHoldSeconds >= 0 ? { config: { holdMs: Math.min(liveHoldSeconds, 3600) * 1000 } } : {}),
  voices: makeNoteVoiceMatcher(repository, providers.transcribe, resolve(process.env.AUDIO_STORAGE_DIR ?? ".local/audio")) })
const processing = makeProcessingService(repository, { providers, live: liveHub })
const authSecret=process.env.BETTER_AUTH_SECRET || await loadAuthenticationSecret(resolve(process.env.SETTINGS_KEY_FILE ?? resolve(process.env.AUDIO_STORAGE_DIR ?? ".local/audio", "../state/settings.key"), "../auth.key"))
const authentication=makeAuthenticationService(database,adminToken,{secret:authSecret,baseURL:process.env.PUBLIC_BASE_URL ?? "http://localhost:"+port,trustedOrigins:process.env.AUTH_TRUSTED_ORIGINS?.split(",").map(origin=>origin.trim()).filter(Boolean)})
const google=makeGoogleService(makePrismaGoogleStore(database),secretBox,{clientId:process.env.GOOGLE_CLIENT_ID,clientSecret:process.env.GOOGLE_CLIENT_SECRET,baseURL:process.env.PUBLIC_BASE_URL??"http://localhost:"+port,worker:true})
const mcpStore=createMcpStore(database)
const mcpExecutor=createMcpToolExecutor({store:mcpStore,
  processingForUser:(userId,workspaceId,signal)=>processing.forRepository(repository.forUser(userId,workspaceId),{requestSignal:signal,meetingContext:id=>Effect.map(google.getNoteContext(userId,id),context=>context.meeting)}),
  signOut:grant=>authentication.mcp.revoke(grant),
})
const mcpEndpoint=makeMcpEndpoint({baseURL:process.env.PUBLIC_BASE_URL??"http://localhost:"+port,protect:authentication.mcp.protect,executor:mcpExecutor,trustedOrigins:process.env.AUTH_TRUSTED_ORIGINS?.split(",").map(origin=>origin.trim()).filter(Boolean)})
const api = makeApiHandler(repository, adminToken, { processing, administration, authentication, google, mcpStore, noteParticipants:makeNoteParticipantsService(database) })
const web = makeWebHandler(process.env.WEB_DIST ?? resolve(import.meta.dir, "../../web/dist/client"))
const live = makeLiveEndpoint({ repository, hub: liveHub, resolvePrincipal: headers => authentication.resolve(headers), isTrustedOrigin: origin => authentication.isTrustedOrigin(origin), transcriber: makeLiveTranscriberResolver(administration) })
const server = Bun.serve<LiveConnection>({
  port,
  hostname: process.env.HOST ?? "127.0.0.1",
  maxRequestBodySize: 29 * 1024 * 1024,
  idleTimeout: 255,
  websocket: live.websocket,
  async fetch(request, server) {
    const path = new URL(request.url).pathname
    // The live transcription socket is upgraded here, before the Effect handler, which has no WebSocket support.
    const liveNoteId = live.match(path)
    if (liveNoteId) return live.upgrade(request, server, liveNoteId)
    if(path==="/mcp")return mcpEndpoint.fetch(request)
    if(path.startsWith("/.well-known/"))return authentication.mcp.metadata(request)
    if (path === "/api/v1/ready") {
      const headers = { "Cache-Control": "no-store", "X-Content-Type-Options": "nosniff" }
      if (request.method !== "GET" && request.method !== "HEAD") return new Response(null, { status: 405, headers: { ...headers, Allow: "GET, HEAD" } })
      try {
        await ready()
        return request.method === "HEAD" ? new Response(null, { headers }) : Response.json({ status: "ready" }, { headers })
      } catch {
        return request.method === "HEAD" ? new Response(null, { status: 503, headers }) : Response.json({ status: "unavailable" }, { status: 503, headers })
      }
    }
    if (path === "/api" || path.startsWith("/api/")) {
      const response = await api.handler(request)
      response.headers.set("Cache-Control", "no-store")
      response.headers.set("Referrer-Policy", "no-referrer")
      response.headers.set("X-Content-Type-Options", "nosniff")
      return response
    }
    return web(request)
  },
  error() { return Response.json({ error: "Internal server error" }, { status: 500 }) },
})
// Do not log request URLs: shared-note URLs contain capability tokens.
console.log(`Cereal listening on port ${server.port}`)
let stopping = false
async function shutdown() {
  if (stopping) return
  stopping = true
  await mcpEndpoint.close()
  await liveHub.dispose()
  await server.stop(false)
  await api.dispose()
  await close()
}
process.on("SIGTERM", () => void shutdown())
process.on("SIGINT", () => void shutdown())
