import { createHash, randomBytes } from "node:crypto"
import { Effect, Layer } from "effect"
import { HttpRouter, HttpServerRequest } from "effect/http"
import { HttpApiBuilder } from "effect/http-api"
import { BunHttpServer } from "@effect/platform-bun"
import { MeetingsApi, OwnerAuth, AuthPrincipal, type PrincipalData, type ApiError } from "@meeting-notes/contracts/api"
import type { CreateShareInput, NoteSummaryDTO, SharedNoteDTO } from "@meeting-notes/contracts"
import { makeProcessingService, type ProcessingService } from "./processing"
import type { NoteParticipantsService } from "./note-participants"
import type { GoogleIntegration } from "./google-integration"
import { bootstrapPrincipal, isBootstrapToken, type AuthenticationService } from "./auth"
import { makeAdministrationService, type AdministrationService } from "./administration"
import { makeMemoryCalendarStore } from "./calendar-store"
import { NotesRepository, type NotesRepositoryService, type RepositoryError } from "./repository"
import {McpStoreError,type McpStore} from "./mcp-store"
import {InvalidPageRequest,parseListInput,parseSearchInput,selectNoteSearch,selectNoteSummaries,withMeeting} from "./note-summaries"

const unavailable = { error: "unavailable", message: "The note service is temporarily unavailable" } as const
const missingNote = { error: "not_found", message: "Note not found" } as const
const missingShare = { error: "not_found", message: "Shared note not found" } as const
const persist = <A>(effect: Effect.Effect<A, RepositoryError>) => Effect.mapError(effect, () => unavailable)
const hashToken = (token: string) => createHash("sha256").update(token).digest("hex")
const validToken = /^[A-Za-z0-9_-]{43}$/

function createShare(noteId: string, input: CreateShareInput, clock: () => Date) {
  return Effect.gen(function*() {
    const repo = yield* NotesRepository
    const now = clock()
    if (input.expiresAt && Date.parse(input.expiresAt) <= now.getTime()) {
      return yield* Effect.fail({ error: "invalid_request", message: "Share expiry must be in the future" } as const)
    }
    const note = yield* persist(repo.getNote(noteId))
    if (!note) return yield* Effect.fail(missingNote)
    const id = crypto.randomUUID()
    const token = randomBytes(32).toString("base64url")
    const metadata = { id, noteId, createdAt: now.toISOString(), expiresAt: input.expiresAt ?? null, revokedAt: null }
    const snapshot: SharedNoteDTO = {
      shareId: id, title: note.title,
      body: input.includeBody ? note.body : "",
      enhanced: input.includeEnhanced ? note.enhanced : "",
      date: note.date, duration: note.duration, sharedAt: metadata.createdAt, expiresAt: metadata.expiresAt,
    }
    yield* persist(repo.createShare({ metadata, tokenHash: hashToken(token), snapshot }))
    return { share: metadata, token, path: `/s/${token}` }
  })
}

/** The caller owns repository lifetime; dispose releases only the API runtime. */
export function makeApiHandler(repository: NotesRepositoryService, adminToken: string, options: { clock?: () => Date; processing?: ProcessingService; administration?: AdministrationService; authentication?: AuthenticationService; google?: GoogleIntegration; noteParticipants?:NoteParticipantsService; mcpStore?:McpStore; principalResolver?: (headers: Headers) => Promise<PrincipalData|null> } = {}) {
  if (adminToken.length < 32) throw new Error("ADMIN_TOKEN must contain at least 32 characters")
  const processing = options.processing ?? makeProcessingService(repository, { worker: false })
  const administration = options.administration ?? makeAdministrationService(makeMemoryCalendarStore(repository), undefined, { clock: options.clock })
  const useProcessing = <A, E, R>(operation: (service: ReturnType<ProcessingService["forRepository"]>) => Effect.Effect<A, E, R>) =>
    Effect.gen(function*(){const repo=yield* NotesRepository,principal=yield* AuthPrincipal,request=yield* HttpServerRequest.HttpServerRequest;return yield* operation(processing.forRepository(repo,{actor:principal.id,requestSignal:request.source instanceof Request?request.source.signal:undefined,meetingContext:options.google?noteId=>Effect.map(options.google!.getNoteContext(principal.id,noteId),context=>context.meeting):undefined}))})
  const clock = options.clock ?? (() => new Date())
  const auth = Layer.succeed(OwnerAuth, {
    bearer: (httpEffect) => Effect.gen(function*() {
      const request = yield* HttpServerRequest.HttpServerRequest
      const headers = new Headers(request.headers)
      const principal = yield* Effect.tryPromise({
        try: () => options.principalResolver ? options.principalResolver(headers) : options.authentication ? options.authentication.resolve(headers) : Promise.resolve(isBootstrapToken(headers,adminToken)?bootstrapPrincipal:null),
        catch: () => ({error:"unavailable",message:"Account service is temporarily unavailable"} as const),
      })
      if (!principal) return yield* Effect.fail({ error: "unauthorized", message: "Authentication required" } as const)
      if (!["GET","HEAD","OPTIONS"].includes(request.method) && !headers.has("authorization")) {
        const origin=headers.get("origin")
        if(!origin || !options.authentication?.isTrustedOrigin(origin))return yield* Effect.fail({error:"forbidden",message:"A same-origin request is required"} as const)
      }
      return yield* httpEffect.pipe(Effect.provideService(AuthPrincipal,principal),Effect.provideService(NotesRepository,repository.forUser(principal.id)))
    }),
  })
  const adminOnly = <A,E,R>(effect:Effect.Effect<A,E,R>) => Effect.gen(function*(){const principal=yield* AuthPrincipal;if(principal.role!=="owner"&&principal.role!=="admin")return yield* Effect.fail({error:"forbidden",message:"Administrator access required"} as const);return yield* effect})
  const ownerOnly = <A,E,R>(effect:Effect.Effect<A,E,R>) => Effect.gen(function*(){const principal=yield* AuthPrincipal;if(principal.role!=="owner")return yield* Effect.fail({error:"forbidden",message:"Owner access required"} as const);return yield* effect})
  const accounts = <A>(effect:Effect.Effect<A,ApiError>|undefined) => effect??Effect.fail({error:"unavailable",message:"Account service is not configured"} as const)
  const organization = <A>(operation:(service:AuthenticationService["organizations"],principal:PrincipalData,headers:Headers)=>Effect.Effect<A,ApiError>) => Effect.gen(function*(){
    const principal=yield* AuthPrincipal,request=yield* HttpServerRequest.HttpServerRequest
    if(!options.authentication)return yield* Effect.fail({error:"unavailable",message:"Account service is not configured"} as const)
    return yield* operation(options.authentication.organizations,principal,new Headers(request.headers))
  })
  const accountOperation = <A>(operation:(headers:Headers|undefined)=>Effect.Effect<A,ApiError>|undefined) => Effect.gen(function*(){
    const principal=yield* AuthPrincipal,request=yield* HttpServerRequest.HttpServerRequest
    const headers=new Headers(request.headers);if(headers.has("authorization"))headers.delete("cookie")
    return yield* accounts(operation(principal.source==="session"?headers:undefined))
  })
  const noteContextFor = (userId:string,noteId:string) => Effect.gen(function*(){const context=yield* accounts(options.google?.getNoteContext(userId,noteId));if(context.meeting)return context;const legacy=yield* administration.noteContext(userId,noteId);return legacy?yield* accounts(options.google?.initializeExternalContext(userId,noteId,legacy)):context})
  const mcpSupport=<A>(operation:(store:McpStore,scope:{userId:string;workspaceId:string})=>Promise<A>)=>Effect.gen(function*(){
    const principal=yield* AuthPrincipal
    if(!options.mcpStore)return yield* Effect.fail(unavailable)
    return yield* Effect.tryPromise({try:async()=>{
      const state=await options.mcpStore!.resolveWorkspace(principal.id)
      if(!state.activeWorkspace)throw new McpStoreError("forbidden","Select an accessible workspace first.")
      return operation(options.mcpStore!,{userId:principal.id,workspaceId:state.activeWorkspace.id})
    },catch:(error):ApiError=>error instanceof McpStoreError?{error:error.code==="invalid_arguments"?"invalid_request":error.code,message:error.message} as ApiError:unavailable})
  })
  const pageInput=<A>(parse:()=>A)=>Effect.try({try:parse,catch:(error):ApiError=>error instanceof InvalidPageRequest?{error:"invalid_request",message:error.message}:unavailable})
  // Only a repository without its own list rows (the memory one) comes here; its
  // notes get their saved calendar context one page at a time. An unreadable
  // context leaves a row without attendees; it never fails the list.
  const withSavedMeeting=(summary:NoteSummaryDTO)=>Effect.gen(function*(){
    if(!options.google)return summary
    const principal=yield* AuthPrincipal
    return yield* options.google.getNoteContext(principal.id,summary.id).pipe(Effect.map(context=>withMeeting(summary,context.meeting)),Effect.catch(()=>Effect.succeed(summary)))
  })
  const publicHandlers = HttpApiBuilder.group(MeetingsApi, "public", (handlers) => handlers
    .handle("health", () => Effect.succeed({ status: "ok", version: "v1" } as const))
    .handle("getSharedNote", ({ params }) => Effect.gen(function*() {
      if (!validToken.test(params.token)) return yield* Effect.fail(missingShare)
      const repo = yield* NotesRepository
      const share = yield* persist(repo.findShare(hashToken(params.token)))
      if (!share || share.metadata.revokedAt || (share.metadata.expiresAt && Date.parse(share.metadata.expiresAt) <= clock().getTime())) {
        return yield* Effect.fail(missingShare)
      }
      return share.snapshot
    })))
  const ownerHandlers = HttpApiBuilder.group(MeetingsApi, "owner", (handlers) => handlers
    .handle("getGoogleStatus", () => Effect.flatMap(AuthPrincipal,p=>options.google?options.google.getStatus(p.id):Effect.succeed({configured:false,redirectUri:null,connections:[]})))
    .handle("beginGoogleConnect", ({payload}) => Effect.gen(function*(){const p=yield* AuthPrincipal,request=yield* HttpServerRequest.HttpServerRequest;if(p.source!=="session"||!p.sessionId||request.headers.authorization)return yield* Effect.fail({error:"forbidden",message:"Sign in through the web account page to connect Google"} as const);const status=yield* accounts(options.google?.getStatus(p.id));if(status.redirectUri&&new URL(status.redirectUri).hostname!==new URL(request.headers.origin!).hostname)return yield* Effect.fail({error:"invalid_request",message:"Open the account page at the configured instance hostname before connecting Google"} as const);return yield* accounts(options.google?.beginConnect(p.id,createHash("sha256").update(p.sessionId).digest("hex"),payload.contacts??false))}))
    .handle("listGoogleCalendars", ({params}) => Effect.flatMap(AuthPrincipal,p=>accounts(options.google?.listCalendars(p.id,params.id))))
    .handle("updateGoogleCalendar", ({params,payload}) => Effect.flatMap(AuthPrincipal,p=>accounts(options.google?.selectCalendar(p.id,params.id,payload.selected))))
    .handle("syncGoogleConnection", ({params}) => Effect.flatMap(AuthPrincipal,p=>accounts(options.google?.syncConnection(p.id,params.id))))
    .handle("deleteGoogleConnection", ({params}) => Effect.flatMap(AuthPrincipal,p=>accounts(options.google?.disconnect(p.id,params.id))))
    .handle("upsertNoteParticipant", ({params,payload}) => Effect.gen(function*(){const p=yield* AuthPrincipal;yield* noteContextFor(p.id,params.id);return yield* accounts(options.noteParticipants?.upsert(p.id,params.id,payload))}))
    .handle("getNoteContext", ({params}) => Effect.flatMap(AuthPrincipal,p=>noteContextFor(p.id,params.id)))
    .handle("updateNoteContext", ({params,payload}) => Effect.gen(function*(){const p=yield* AuthPrincipal,ics=payload.eventId?yield* administration.eventContext(payload.eventId):null;const context=yield* accounts(ics?options.google?.putExternalContext(p.id,params.id,ics):options.google?.setNoteContext(p.id,params.id,payload.eventId));return context}))
    .handle("listOrganizations",()=>organization((s,p,h)=>s.list(p,h)))
    .handle("createOrganization",({payload})=>organization((s,p,h)=>s.create(p,h,payload)))
    .handle("setActiveOrganization",({payload})=>organization((s,p,h)=>s.select(p,h,payload.organizationId)))
    .handle("getOrganization",({params})=>organization((s,p,h)=>s.get(p,h,params.id)))
    .handle("updateOrganization",({params,payload})=>organization((s,p,h)=>s.update(p,h,params.id,payload)))
    .handle("inviteOrganizationMember",({params,payload})=>organization((s,p,h)=>s.invite(p,h,params.id,payload)))
    .handle("cancelOrganizationInvitation",({params})=>organization((s,p,h)=>s.cancel(p,h,params.id,params.invitationId)))
    .handle("updateOrganizationMemberRole",({params,payload})=>organization((s,p,h)=>s.updateRole(p,h,params.id,params.memberId,payload)))
    .handle("removeOrganizationMember",({params})=>organization((s,p,h)=>s.remove(p,h,params.id,params.memberId)))
    .handle("listOrganizationInvitations",()=>organization((s,p,h)=>s.incoming(p,h)))
    .handle("acceptOrganizationInvitation",({params})=>organization((s,p,h)=>s.accept(p,h,params.id)))
    .handle("rejectOrganizationInvitation",({params})=>organization((s,p,h)=>s.reject(p,h,params.id)))
    .handle("getSession", () => Effect.map(AuthPrincipal, ({source,...principal})=>({user:{id:principal.id,name:principal.name,email:principal.email,role:principal.role,createdAt:principal.createdAt},expiresAt:principal.expiresAt})))
    .handle("listUsers", () => adminOnly(accountOperation(headers=>options.authentication?.listUsers(headers))))
    .handle("createUser", ({payload}) => adminOnly(accountOperation(headers=>options.authentication?.createUser(payload,headers))))
    .handle("createAdministrator", ({payload}) => ownerOnly(accountOperation(headers=>options.authentication?.createAdministrator(payload,headers))))
    .handle("updateUserRole", ({params,payload}) => ownerOnly(accountOperation(headers=>options.authentication?.updateUserRole(params.id,payload.role,headers))))
    .handle("setupOwner", ({payload}) => Effect.flatMap(AuthPrincipal,principal=>principal.role==="owner"&&principal.source==="bootstrap"?accounts(options.authentication?.setupOwner(payload)):Effect.fail({error:"forbidden",message:"The bootstrap owner credential is required"} as const)))
    .handle("getCalendarPreferences", () => Effect.map(administration.getSettings(),settings=>({lookAheadDays:settings.calendarLookAheadDays})))
    .handle("getChatModels", () => administration.getChatModels())
    .handle("getModelCatalog", ({payload}) => adminOnly(administration.getModelCatalog(payload)))
    .handle("getAdminSettings", () => adminOnly(administration.getSettings()))
    .handle("updateAdminSettings", ({payload}) => adminOnly(administration.updateSettings(payload)))
    .handle("listCalendars", () => administration.listCalendars())
    .handle("createCalendar", ({payload}) => adminOnly(administration.createCalendar(payload)))
    .handle("updateCalendar", ({params,payload}) => adminOnly(administration.updateCalendar(params.id,payload)))
    .handle("deleteCalendar", ({params}) => adminOnly(administration.deleteCalendar(params.id)))
    .handle("syncCalendar", ({params}) => adminOnly(administration.syncCalendar(params.id)))
    .handle("listCalendarEvents", ({query}) => Effect.gen(function*(){const p=yield* AuthPrincipal,range=yield* administration.eventRange(query),shared=yield* administration.listEvents(range,p.id),personal=options.google?yield* options.google.listEvents(p.id,range):[];const events=[...shared,...personal].sort((a,b)=>a.startAt.localeCompare(b.startAt)||a.id.localeCompare(b.id));if(events.length>10000)return yield* Effect.fail({error:"invalid_request",message:"Too many events; request a shorter date range"} as const);return events}))
    .handle("createCalendarEventNote", ({params}) => Effect.gen(function*(){const p=yield* AuthPrincipal,personal=options.google?yield* options.google.createEventNote(p.id,params.id):null;if(personal)return personal;const note=yield* administration.createEventNote(params.id,p.id);if(options.google){const context=yield* administration.eventContext(params.id);if(context)yield* options.google.initializeExternalContext(p.id,note.id,context)}return note}))
    .handle("chatWorkspaceStream", ({payload}) => useProcessing(service => service.chatStream(null,payload)))
    .handle("getAgentBrief",()=>mcpSupport(async(store,scope)=>({...await store.agentBrief(scope),source:"explicit_user_profile" as const,automaticallyInferred:false as const})))
    .handle("updateAgentBrief",({payload})=>mcpSupport(async(store,scope)=>({...await store.updateAgentBrief(scope,payload.body),workspaceId:scope.workspaceId,source:"explicit_user_profile" as const,automaticallyInferred:false as const})))
    .handle("uploadNoteAttachment",({params,payload})=>mcpSupport(async(store,scope)=>{
      const image=Buffer.from(payload.dataBase64,"base64")
      const saved=await store.storeAttachment(scope,params.id,payload.name,payload.mimeType,image)
      return {...saved,mimeType:payload.mimeType,createdAt:saved.createdAt.toISOString()}
    }))
    .handle("recording", ({params,payload}) => useProcessing(service => service.recording(params.id,payload.command)))
    .handle("uploadAudioChunk", ({params,payload}) => useProcessing(service => service.upload(params.id,params.chunkId,payload)))
    .handle("listAudioChunks", ({params}) => useProcessing(service => service.list(params.id)))
    .handle("retryAudioChunk", ({params}) => useProcessing(service => service.retry(params.id,params.chunkId)))
    .handle("generateNotes", ({params,payload}) => useProcessing(service => service.generate(params.id,payload.expectedTranscriptRevision)))
    .handle("chat", ({params,payload}) => useProcessing(service => service.chat(params.id,payload)))
    .handle("chatStream", ({params,payload}) => useProcessing(service => service.chatStream(params.id,payload)))
    .handle("evidence", ({params,payload}) => useProcessing(service => service.evidence(params.id,payload)))
    .handle("listNotes", () => Effect.flatMap(NotesRepository, (repo) => persist(repo.listNotes())))
    .handle("listNoteSummaries", ({query}) => Effect.gen(function*(){
      const repo=yield* NotesRepository,input=yield* pageInput(()=>parseListInput(query))
      if(repo.listNoteSummaries)return yield* persist(repo.listNoteSummaries(input))
      const page=selectNoteSummaries(yield* persist(repo.listNotes()),input)
      return {...page,notes:yield* Effect.forEach(page.notes,withSavedMeeting)}
    }))
    .handle("searchNotes", ({query}) => Effect.gen(function*(){
      const repo=yield* NotesRepository,input=yield* pageInput(()=>parseSearchInput(query))
      if(repo.searchNoteSummaries)return yield* persist(repo.searchNoteSummaries(input))
      const page=selectNoteSearch(yield* persist(repo.listNotes()),input)
      return {...page,results:yield* Effect.forEach(page.results,result=>Effect.map(withSavedMeeting(result.note),note=>({...result,note})))}
    }))
    .handle("createNote", ({ payload }) => Effect.flatMap(NotesRepository, (repo) => persist(repo.createNote(payload))))
    .handle("getNote", ({ params }) => Effect.gen(function*() {
      const repo = yield* NotesRepository
      const note = yield* persist(repo.getNote(params.id))
      return note ?? (yield* Effect.fail(missingNote))
    }))
    .handle("updateNote", ({ params, payload }) => Effect.gen(function*() {
      const repo = yield* NotesRepository
      const note = yield* persist(repo.updateNote(params.id, payload))
      return note ?? (yield* Effect.fail(missingNote))
    }))
    .handle("deleteNote", ({params}) => useProcessing(service => service.deleteNote(params.id)))
    .handle("listShares", ({ params }) => Effect.gen(function*() {
      const repo = yield* NotesRepository
      if (!(yield* persist(repo.getNote(params.id)))) return yield* Effect.fail(missingNote)
      return yield* persist(repo.listShares(params.id))
    }))
    .handle("createShare", ({ params, payload }) => createShare(params.id, payload, clock))
    .handle("revokeShare", ({ params }) => Effect.gen(function*() {
      const repo = yield* NotesRepository
      const share = yield* persist(repo.revokeShare(params.id, clock().toISOString()))
      return share ?? (yield* Effect.fail(missingShare))
    })))
  const handlers = Layer.mergeAll(publicHandlers, ownerHandlers)
  const app = HttpApiBuilder.layer(MeetingsApi, { openapiPath: "/api/openapi.json" }).pipe(
    Layer.provide(handlers), Layer.provide(auth),
    HttpRouter.provideRequest(Layer.succeed(NotesRepository, repository)),
    Layer.provide(BunHttpServer.layerHttpServices),
  )
  const web = HttpRouter.toWebHandler(app, { disableLogger: true })
  return {
    dispose: async () => { await options.google?.dispose(); await administration.dispose(); await processing.dispose(); await web.dispose() },
    handler: async (request: Request): Promise<Response> => {
      if(new URL(request.url).pathname==="/api/integrations/google/callback"){
        if(request.method!=="GET")return new Response(null,{status:405,headers:{Allow:"GET"}})
        try{if(!options.google||!options.authentication||request.headers.has("authorization"))throw new Error("Browser sign-in required");const principal=await options.authentication.resolve(request.headers);if(!principal||principal.source!=="session"||!principal.sessionId)throw new Error("Browser sign-in required");const response=await options.google.callback(request,principal);response.headers.set("Cache-Control","no-store");response.headers.set("Referrer-Policy","no-referrer");return response}catch{return new Response(null,{status:303,headers:{Location:"/account?google=failed","Cache-Control":"no-store","Referrer-Policy":"no-referrer"}})}
      }
      const streamAbort=/^\/api\/v1\/(?:notes\/[^/]+\/)?chat\/stream$/.test(new URL(request.url).pathname)?new AbortController():null
      const forwarded=streamAbort?new Request(request,{signal:AbortSignal.any([request.signal,streamAbort.signal])}):request
      const response = new URL(request.url).pathname.startsWith("/api/auth/") && options.authentication ? await options.authentication.handler(request) : await web.handler(forwarded)
      const headers = new Headers(response.headers)
      headers.set("Cache-Control", "no-store")
      headers.set("Referrer-Policy", "no-referrer")
      headers.set("X-Content-Type-Options", "nosniff")
      if (headers.get("Content-Type")?.includes("text/event-stream")) headers.set("X-Accel-Buffering", "no")
      let body=response.body
      if(streamAbort&&body&&headers.get("Content-Type")?.includes("text/event-stream")){
        const reader=body.getReader();let ended=false
        body=new ReadableStream<Uint8Array<ArrayBuffer>>({
          async pull(output){try{const next=await reader.read();if(ended)return;if(next.done){ended=true;output.close()}else output.enqueue(next.value)}catch(error){if(!ended){ended=true;output.error(error)}}},
          async cancel(reason){ended=true;streamAbort.abort();await reader.cancel(reason).catch(()=>{})},
        })
      }
      return new Response(body, { status: response.status, statusText: response.statusText, headers })
    },
  }
}

export type { ApiError }
