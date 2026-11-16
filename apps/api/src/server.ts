import { makeNoteParticipantsService } from "./note-participants"
import { makeTeamSpaceService, makePrismaTeamSpaceStore } from "./team-space"
import { makeNoteAccessService, makePrismaNoteAccessStore } from "./note-access"
import { makeGoogleService } from "./google"
import { refreshGooglePicture } from "./sign-in"
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
import { appleAppSiteAssociation, makeLinkPreviews } from "./app-links"
import { DOWNLOAD_FILES_PATH, makeDownloads } from "./downloads"
import { readAppVersion } from "./version"
import { makeStorage } from "./storage"
import {Effect} from "effect"
import {createMcpStore} from "./mcp-store"
import {createMcpToolExecutor} from "./mcp-tools"
import {makeMcpEndpoint} from "./mcp"
import {MCP_PATH,LEGACY_MCP_PATH} from "./mcp-auth"
import {makeLiveHub} from "./live/hub"
import {makeLiveEndpoint,type LiveConnection} from "./live/route"
import {makeLiveTranscriberResolver} from "./live/configuration"
import {chatTitler,makeChatsService,makePrismaChatStore} from "./chats"
import {transcriptLinkLine} from "./transcript-link"

const databaseUrl = process.env.DATABASE_URL
const adminToken = process.env.ADMIN_API_TOKEN
if (!databaseUrl || !adminToken || adminToken.length < 32 || adminToken.startsWith("replace-")) {
  throw new Error("Configure DATABASE_URL and a random ADMIN_API_TOKEN of at least 32 characters; run scripts/setup.sh for local setup")
}
const port = Number(process.env.PORT ?? 3000)
if (!Number.isInteger(port) || port < 1 || port > 65535) throw new Error("Invalid PORT")
const publicBaseURL = process.env.PUBLIC_BASE_URL ?? "http://localhost:" + port
// Generated notes end with a link to chat with the meeting's transcript on this server's public address.
const { repository, database, ready, close } = makePrismaRepository(databaseUrl, { transcriptLink: noteId => transcriptLinkLine(publicBaseURL, noteId) })
const secretBox = await loadSecretBox(process.env.SETTINGS_KEY_FILE ?? resolve(process.env.AUDIO_STORAGE_DIR ?? ".local/audio", "../state/settings.key"))
const administration = makeAdministrationService(makePrismaCalendarStore(database), secretBox, { worker: true, baseURL: publicBaseURL })
// Settings that used to come from the environment (provider keys, default models, live limits, the iPhone app's
// identifiers) are made on the admin panel. An existing deployment's values are taken into the database once,
// here; from then on the stored value decides and the variable is ignored. Names only: never a value.
try { for (const name of await administration.importEnvironment()) console.log(`Imported ${name} from the environment into the settings. It is no longer read and can be removed.`) }
catch { console.warn("Settings in the environment could not be taken into the database yet; they stay in use and are tried again at the next start.") }
// Live transcription sessions live in this process only. Their limits are set on the admin panel and read
// again every few seconds, so a change applies without a restart.
let liveLimits = { maxSessions: 20, holdMs: 300_000 }
const refreshLiveLimits = async () => { try { const limits = await administration.liveLimits(); liveLimits = { maxSessions: limits.maxSessions, holdMs: limits.holdSeconds * 1000 } } catch { /* keep the last known limits */ } }
await refreshLiveLimits()
setInterval(() => void refreshLiveLimits(), 5000).unref()
const providers = settingsAwareProviders(administration)
// Audio parts of older recordings (removed with their note; new recordings keep no audio on the server) and published app builds: on local disk (these two directories) or in an S3-compatible bucket,
// as the admin panel says. The setting is read for every operation, so a change needs no restart.
const audioDir = resolve(process.env.AUDIO_STORAGE_DIR ?? ".local/audio")
const storage = makeStorage({ audioDir, downloadsDir: process.env.DOWNLOADS_DIR || "/data/downloads", configuration: () => administration.storageConfiguration() })
// The hold is how long a note's provider sessions are kept after Stop so a Resume keeps its speaker labels
// (0 turns it off). A held session is billed for keep-alive audio only but occupies one of the provider's
// concurrent sessions per channel. After the hold, voices carry over as live/speakers.ts describes.
const liveHub = makeLiveHub(repository, { limits: () => liveLimits })
const processing = makeProcessingService(repository, { providers, live: liveHub, storage, storageDir: audioDir })
const authSecret=process.env.BETTER_AUTH_SECRET || await loadAuthenticationSecret(resolve(process.env.SETTINGS_KEY_FILE ?? resolve(process.env.AUDIO_STORAGE_DIR ?? ".local/audio", "../state/settings.key"), "../auth.key"))
// Tests only: a stand-in for Google's sign-in endpoints. Refused unless both it and this server are loopback HTTP,
// so a deployed instance can never be pointed at another identity provider through its environment.
function testGoogleEndpoints(raw:string|undefined){
  if(!raw)return undefined
  const loopback=(value:string)=>{const url=new URL(value);return url.protocol==="http:"&&["localhost","127.0.0.1","[::1]"].includes(url.hostname)}
  if(!loopback(raw)||!loopback(publicBaseURL))throw new Error("GOOGLE_OAUTH_TEST_ORIGIN is for local tests: it and PUBLIC_BASE_URL must both be loopback HTTP origins")
  const origin=new URL(raw).origin
  return{endpoints:{authorization:origin+"/authorize",token:origin+"/token",userinfo:origin+"/userinfo"}}
}
// Sign-in methods, the allow list and the Google client are read from the database on every use, so a change on the admin panel applies at once.
const authentication=makeAuthenticationService(database,adminToken,{secret:authSecret,baseURL:publicBaseURL,trustedOrigins:process.env.AUTH_TRUSTED_ORIGINS?.split(",").map(origin=>origin.trim()).filter(Boolean),signIn:()=>administration.signInPolicy(),useGoogle:client=>administration.useGoogle(client),google:testGoogleEndpoints(process.env.GOOGLE_OAUTH_TEST_ORIGIN)})
// The Google client saved on the admin panel, else GOOGLE_CLIENT_ID / GOOGLE_CLIENT_SECRET from the environment.
const google=makeGoogleService(makePrismaGoogleStore(database),secretBox,{credentials:()=>administration.googleCredentials(),baseURL:publicBaseURL,worker:true,picture:(userId,subject,picture)=>refreshGooglePicture(database,userId,subject,picture)})
const mcpStore=createMcpStore(database)
const mcpExecutor=createMcpToolExecutor({store:mcpStore,
  processingForUser:(userId,workspaceId,signal)=>processing.forRepository(repository.forUser(userId,workspaceId),{requestSignal:signal,meetingContext:id=>Effect.map(google.getNoteContext(userId,id),context=>context.meeting)}),
  signOut:grant=>authentication.mcp.revoke(grant),
})
const mcpEndpoint=makeMcpEndpoint({baseURL:process.env.PUBLIC_BASE_URL??"http://localhost:"+port,protect:authentication.mcp.protect,executor:mcpExecutor,trustedOrigins:process.env.AUTH_TRUSTED_ORIGINS?.split(",").map(origin=>origin.trim()).filter(Boolean)})
// The product version this server is (root package.json), and the app builds published for it. A release is
// published by placing its files in DOWNLOADS_DIR; nothing is read from there until someone asks.
const appVersion = readAppVersion()
/** The iPhone app's App Store ID as the admin panel has it now. */
const appStoreId = async () => (await administration.apps()).iosAppStoreId
const downloads = makeDownloads({ directory: process.env.DOWNLOADS_DIR || "/data/downloads", version: appVersion, iosAppStoreId: appStoreId, storage: storage.downloads })
// Saved conversations, per person; a thread is named by the same model request that names an untitled meeting.
const chats = makeChatsService(makePrismaChatStore(database), { title: chatTitler(providers.title) })
const api = makeApiHandler(repository, adminToken, { processing, administration, authentication, google, mcpStore, noteParticipants:makeNoteParticipantsService(database), downloads, storage, chats, teamSpace:makeTeamSpaceService(makePrismaTeamSpaceStore(database)), noteAccess:makeNoteAccessService(makePrismaNoteAccessStore(database,{signInDomains:async()=>(await administration.signInPolicy()).allowedEmails})) })
const web = makeWebHandler(process.env.WEB_DIST ?? resolve(import.meta.dir, "../../web/dist/client"), { head: makeLinkPreviews({ origin: new URL(publicBaseURL).origin, iosAppStoreId: appStoreId }) })
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
    // The MCP endpoint is /api/mcp. The exact path /mcp is its previous address and still answers; /mcp/connect and /mcp/consent are web pages.
    if(path===MCP_PATH||path===LEGACY_MCP_PATH)return mcpEndpoint.fetch(request)
    if(path==="/.well-known/apple-app-site-association")return appleAppSiteAssociation(request,(await administration.apps().catch(()=>null))?.iosAppIds.join(","))
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
      // Everything under /api is private to its caller, except a published app file, which says so itself.
      if (!path.startsWith(DOWNLOAD_FILES_PATH)) response.headers.set("Cache-Control", "no-store")
      response.headers.set("Referrer-Policy", "no-referrer")
      response.headers.set("X-Content-Type-Options", "nosniff")
      return response
    }
    return web(request)
  },
  error() { return Response.json({ error: "Internal server error" }, { status: 500 }) },
})
// Do not log request URLs: shared-note URLs contain capability tokens.
console.log(`Cereal ${appVersion} listening on port ${server.port}`)
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
