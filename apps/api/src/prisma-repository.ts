import { Effect, Schema } from "effect"
import { createDatabase, type Database } from "@meeting-notes/database"
import { NoteDTO, SharedNoteDTO, type ShareDTO, type CreateNoteInput } from "@meeting-notes/contracts"
import { BOOTSTRAP_OWNER_ID, RepositoryError, scopeRepository, transcriptContentKey, withoutPlaceholderTitle, type ImportNoteCommand, type ImportNoteResult, type NotesRepositoryService, type StoredShare, type StoredAudioChunk } from "./repository"
import {IMPORT_TRANSACTION_MS,clampCreatedAt,importedSegmentId,matchesStamp} from "./note-import"
import {isDefaultNoteTitle} from "./meeting-context-errors"
import {meetingSearchTerms,transcriptPassage} from "./meeting-search"
import {DEFAULT_ORGANIZATION_ID} from "./organizations"
import {makePrismaLiveRepository,replaceLiveWithin} from "./live/prisma-repository"
import {makePrismaNoteSummaries} from "./note-summaries-store"

type DbNote = Awaited<ReturnType<Database["note"]["findMany"]>>[number] & {
  transcriptSegments?: ReadonlyArray<{ id: string; channel: "mic" | "system"; speakerId: string; speakerLabel: string | null; text: string; isFinal: boolean; startMs: number; endMs: number | null }>
}
export const noteDTO = (note: DbNote): NoteDTO => Schema.decodeUnknownSync(NoteDTO)({
  id: note.id, title: note.title, body: note.body, enhanced: note.enhanced, date: note.date.toISOString().slice(0, 10), duration: note.duration,
  createdAt: note.createdAt.toISOString(), updatedAt: note.updatedAt.toISOString(), transcriptSegments: note.transcriptSegments ?? [],
  recordingState: note.recordingState, generationState: note.generationState, transcriptRevision: note.transcriptRevision, generatedRevision: note.generatedRevision,
  // Only a note that was imported says so: every other note reads exactly as it did before the field existed.
  ...(note.importedAt ? { importedAt: note.importedAt.toISOString() } : {}),
})
type DbShare = Awaited<ReturnType<Database["sharedNote"]["findMany"]>>[number]
const shareDTO = (share: DbShare): ShareDTO => ({ id: share.id, noteId: share.noteId, createdAt: share.createdAt.toISOString(), expiresAt: share.expiresAt?.toISOString() ?? null, revokedAt: share.revokedAt?.toISOString() ?? null })
const storedShare = (share: DbShare): StoredShare => ({ metadata: shareDTO(share), tokenHash: share.tokenHash, snapshot: Schema.decodeUnknownSync(SharedNoteDTO)(share.snapshot) })
const include = { transcriptSegments: { orderBy: { startMs: "asc" as const } } }
const attempt = <A>(operation: string, task: () => Promise<A>) => Effect.tryPromise({ try: task, catch: () => new RepositoryError(operation) })

type DbAudioChunk = Awaited<ReturnType<Database["audioChunk"]["findMany"]>>[number]
const chunkDTO = (c: DbAudioChunk): StoredAudioChunk => ({id:c.id,noteId:c.noteId,channel:c.channel,sequence:c.sequence,startMs:c.startMs,durationMs:c.durationMs,hash:c.hash,state:c.state as StoredAudioChunk["state"],error:c.error,lease:c.lease,leaseUntil:c.leaseUntil?.toISOString()??null,resolution:c.resolution as StoredAudioChunk["resolution"]})

/** Prisma remains behind the Effect repository boundary; generated entities never cross the API. */
export function makePrismaRepository(databaseUrl: string) {
  const db = createDatabase(databaseUrl)
  // A new note follows the user's selected workspace, without widening the
  // existing native API's private owner boundary. Validate membership afresh.
  const workspaceFor = async (ownerId: string, organizationId: string | undefined, operation: string): Promise<string | null> => {
    const selected=organizationId??(await db.userWorkspaceState.findUnique({where:{userId:ownerId}}))?.activeOrganizationId;
    const membership=selected?await db.member.findUnique({where:{organizationId_userId:{organizationId:selected,userId:ownerId}}}):null;
    if(organizationId&&!membership)throw new RepositoryError(operation);
    const fallback=membership??await db.member.findFirst({where:{userId:ownerId},orderBy:[{createdAt:"asc"},{id:"asc"}]});
    const preferred=!membership?await db.member.findUnique({where:{organizationId_userId:{organizationId:DEFAULT_ORGANIZATION_ID,userId:ownerId}}}):null;
    return membership?.organizationId??preferred?.organizationId??fallback?.organizationId??null
  }
  const createNoteForUser = (ownerId: string, input: CreateNoteInput, organizationId?:string) => attempt("createNote", async () => {
    return noteDTO(await db.note.create({ data: {
    ownerId,
    organizationId:await workspaceFor(ownerId,organizationId,"createNote"),
    title: input.title, body: input.body ?? "", enhanced: input.enhanced ?? "", date: new Date(`${input.date ?? new Date().toISOString().slice(0, 10)}T00:00:00.000Z`), duration: input.duration ?? "",
    transcriptRevision: transcriptContentKey(input.transcriptSegments ?? []) === "[]" ? 0 : 1,
    transcriptSegments: { create: [...(input.transcriptSegments ?? [])] },
  }, include }))})
  const importNoteForUser = (ownerId: string, { input, digest, now }: ImportNoteCommand, organizationId?: string) => attempt("importNote", async (): Promise<ImportNoteResult> => {
    const id = input.id.toLowerCase(), transcriptRevision = input.transcriptSegments.length ? 1 : 0, generatedRevision = input.generated ? transcriptRevision : null
    // An id that exists gets one of two answers. The same request again, for a note nothing has touched since
    // (the import wrote `updatedAt` and `importedAt` from one clock reading), is given the note. Everything
    // else, whoever owns the note, is a conflict.
    const existing = async (): Promise<ImportNoteResult | null> => {
      const row = await db.note.findUnique({ where: { id }, include })
      if (!row) return null
      return row.ownerId === ownerId && row.importDigest === digest && row.importedAt?.getTime() === row.updatedAt.getTime() && row.transcriptRevision === transcriptRevision && row.generatedRevision === generatedRevision
        ? { outcome: "replayed", note: noteDTO(row) } : { outcome: "conflict" }
    }
    const found = await existing()
    if (found) return found
    const organization = await workspaceFor(ownerId, organizationId, "importNote")
    try {
      return await db.$transaction(async tx => {
        const row = await tx.note.create({ data: {
          id, ownerId, organizationId: organization,
          title: input.title, body: input.body, enhanced: input.enhanced, date: new Date(`${input.date}T00:00:00.000Z`), duration: input.duration,
          createdAt: clampCreatedAt(input.createdAt, now), updatedAt: now, importedAt: now, importDigest: digest,
          recordingState: input.recordingState, generationState: input.generated ? "ready" : "idle", transcriptRevision, generatedRevision,
        } })
        // Every id is derived from the note's own, so no segment of anyone else's is looked up or can be in the way.
        // Written a thousand at a time: each statement stays small, and far inside the per-statement time limit.
        const segments = input.transcriptSegments.map(segment => ({ ...segment, id: importedSegmentId(id, segment.id) }))
        for (let start = 0; start < segments.length; start += 1000)
          await tx.transcriptSegment.createMany({ data: segments.slice(start, start + 1000).map(segment => ({ ...segment, noteId: id, origin: "batch" })) })
        // The meeting details travel as they were saved. They name no event of this server: the event id is a
        // new one, nothing is looked up, and no calendar binding is made.
        if (input.meeting) await tx.noteMeetingContext.create({ data: { noteId: id, userId: ownerId, meeting: JSON.parse(JSON.stringify({ ...input.meeting, eventId: crypto.randomUUID() })) } })
        // Answered from what was just written, in the order a read gives it, instead of reading tens of thousands of rows back.
        return { outcome: "created" as const, note: noteDTO({ ...row, transcriptSegments: segments.sort((a, b) => a.startMs - b.startMs) }) }
      }, { timeout: IMPORT_TRANSACTION_MS, maxWait: 10_000 })
    } catch (error) {
      // Two requests with one id at once: the one that lost finds the other's note and is answered from it.
      const raced = await existing()
      if (raced) return raced
      // A derived segment id that is already somebody's row (it can only have been put there on purpose) is
      // answered like any id that is taken. Nothing was written: the transaction is gone.
      if (typeof error === "object" && error !== null && "code" in error && error.code === "P2002") return { outcome: "conflict" }
      throw error
    }
  })
  const noteSummaries = makePrismaNoteSummaries(db)
  const repository: NotesRepositoryService = {
    live: makePrismaLiveRepository(db),
    forUser: (userId, organizationId) => scopeRepository(repository, userId, {
      ownsNote: id => attempt("ownsNote", async () => (await db.note.count({ where: { id, ownerId: userId,...(organizationId?{organizationId}:{}) } })) > 0),
      ownsShare: id => attempt("ownsShare", async () => (await db.sharedNote.count({ where: { id, note: { ownerId: userId,...(organizationId?{organizationId}:{}) } } })) > 0),
      listNotes: () => attempt("listNotes", async () => (await db.note.findMany({ where: { ownerId: userId,...(organizationId?{organizationId}:{}) }, include, orderBy: { updatedAt: "desc" } })).map(noteDTO)),
      searchNotes: input => attempt("searchNotes",async()=>{
        const terms=meetingSearchTerms(input.query)
        const rows=await db.note.findMany({where:{ownerId:userId,...(organizationId?{organizationId}:{}),...(input.from||input.to?{date:{...(input.from?{gte:new Date(input.from+"T00:00:00Z")}:{}),...(input.to?{lte:new Date(input.to+"T00:00:00Z")}:{})}}:{}),
          ...(terms.length?{OR:terms.flatMap(term=>[{title:{contains:term,mode:"insensitive" as const}},{body:{contains:term,mode:"insensitive" as const}},{enhanced:{contains:term,mode:"insensitive" as const}},{transcriptSegments:{some:{text:{contains:term,mode:"insensitive" as const}}}}])}:{})},
          orderBy:[{date:"desc"},{updatedAt:"desc"}],take:Math.max(1,Math.min(20,input.limit))})
        // Search returns bounded metadata; a separate scoped read retrieves the
        // transcript only for selected meetings. No global transcript concatenation.
        return rows.map(noteDTO)
      }),
      searchTranscriptSegments:input=>attempt("searchTranscriptSegments",async()=>{
        const terms=meetingSearchTerms(input.query)
        if(!terms.length)return[]
        const segments=await db.transcriptSegment.findMany({where:{note:{ownerId:userId,...(organizationId?{organizationId}:{}),...(input.noteId?{id:input.noteId}:{}),...(input.from||input.to?{date:{...(input.from?{gte:new Date(input.from+"T00:00:00Z")}:{}),...(input.to?{lte:new Date(input.to+"T00:00:00Z")}: {})}}:{})},OR:terms.map(term=>({text:{contains:term,mode:"insensitive" as const}}))},
          include:{note:{select:{id:true,title:true,date:true,transcriptRevision:true}}},orderBy:[{note:{date:"desc"}},{startMs:"asc"},{id:"asc"}],take:Math.max(1,Math.min(20,input.limit))})
        return segments.map(({note,...segment})=>({noteId:note.id,title:note.title,date:note.date.toISOString().slice(0,10),transcriptRevision:note.transcriptRevision,segment:{...segment,text:transcriptPassage(segment.text,terms)}}))
      }),
      listNoteSummaries: input => attempt("listNoteSummaries",()=>noteSummaries.list(userId,organizationId,input)),
      searchNoteSummaries: input => attempt("searchNoteSummaries",()=>noteSummaries.search(userId,organizationId,input)),
      createNote: input => createNoteForUser(userId, input, organizationId),
      importNote: command => importNoteForUser(userId, command, organizationId),
    }, organizationId),
    recording: (id, command) => attempt("recording", () => db.$transaction(async tx => {
      await tx.$queryRaw`SELECT "id" FROM "Note" WHERE "id" = ${id}::uuid FOR UPDATE`
      const n=await tx.note.findUnique({where:{id},include});if(!n || command==="start"&&n.recordingState==="stopped" || command==="resume"&&n.recordingState==="idle")return null
      return noteDTO(await tx.note.update({where:{id},data:{...(command!=="stop"?{generationRun:null,...(n.generationState==="writing_notes"?{generationState:"idle"}:{})}:{}),recordingState:command==="stop"?"stopped":"recording"},include}))
    })),
    listAudioChunks: noteId => attempt("listAudioChunks", async()=> (await db.audioChunk.findMany({where:{noteId},orderBy:[{sequence:"asc"},{channel:"asc"}]})).map(chunkDTO)),
    putAudioChunk: chunk => attempt("putAudioChunk",()=>db.$transaction(async tx=>{
      await tx.$queryRaw`SELECT "id" FROM "Note" WHERE "id" = ${chunk.noteId}::uuid FOR UPDATE`
      if(!await tx.note.findUnique({where:{id:chunk.noteId}}))return null
      const c=await tx.audioChunk.findFirst({where:{OR:[{id:chunk.id},{noteId:chunk.noteId,channel:chunk.channel,sequence:chunk.sequence}]}})
      if(c)return c.id===chunk.id&&c.hash===chunk.hash&&c.noteId===chunk.noteId&&c.channel===chunk.channel&&c.sequence===chunk.sequence&&c.startMs===chunk.startMs?chunkDTO(c):null
      await tx.note.updateMany({where:{id:chunk.noteId,generationState:"writing_notes"},data:{generationState:"idle",generationRun:null}})
      return chunkDTO(await tx.audioChunk.create({data:{id:chunk.id,noteId:chunk.noteId,channel:chunk.channel,sequence:chunk.sequence,startMs:chunk.startMs,durationMs:chunk.durationMs,hash:chunk.hash}}))
    })),
    retryAudioChunk: (noteId,id)=>attempt("retryAudioChunk",async()=>{
      await db.audioChunk.updateMany({where:{id,noteId,state:"failed"},data:{state:"queued",error:null,lease:null,leaseUntil:null}})
      const c=await db.audioChunk.findFirst({where:{id,noteId}});return c?chunkDTO(c):null
    }),
    claimAudioChunk: now=>attempt("claimAudioChunk",()=>db.$transaction(async tx=>{
      const rows=await tx.$queryRaw<Array<{id:string}>>`SELECT "id" FROM "AudioChunk" WHERE "state"='queued' OR ("state"='processing' AND "leaseUntil" <= ${new Date(now)}) ORDER BY "createdAt" ASC LIMIT 1 FOR UPDATE SKIP LOCKED`
      const id=rows[0]?.id;if(!id)return null
      return chunkDTO(await tx.audioChunk.update({where:{id},data:{state:"processing",error:null,lease:crypto.randomUUID(),leaseUntil:new Date(Date.parse(now)+180000)}}))
    })),
    finishAudioChunk: (id,lease,segments,error,options)=>attempt("finishAudioChunk",()=>db.$transaction(async tx=>{
      const initial=await tx.audioChunk.findUnique({where:{id}});if(!initial)return false
      await tx.$queryRaw`SELECT "id" FROM "Note" WHERE "id" = ${initial.noteId}::uuid FOR UPDATE`
      const c=await tx.audioChunk.findUnique({where:{id}});if(!c||c.state!=="processing"||c.lease!==lease)return false
      // Live text inside a part is removed only when the batch result has to stand for the whole part.
      const removed=error===null&&options?.replaceLive?await replaceLiveWithin(tx,c):0
      const n=await tx.note.findUnique({where:{id:c.noteId},include});if(!n)return false
      if(segments.length)await tx.transcriptSegment.createMany({data:segments.map(s=>({...s,noteId:c.noteId}))})
      await tx.audioChunk.update({where:{id},data:{state:error===null?"ready":"failed",error,lease:null,leaseUntil:null,...(error===null?{resolution:options?.resolution??"batch"}:{})}})
      const ready=await tx.audioChunk.findMany({where:{noteId:c.noteId,state:"ready"},select:{startMs:true,durationMs:true}})
      const end=Math.max(0,...ready.map(c=>c.startMs+c.durationMs),...n.transcriptSegments.map(s=>s.endMs??s.startMs),...segments.map(s=>s.endMs??s.startMs))
      await tx.note.update({where:{id:c.noteId},data:{...(segments.some(s=>s.text.trim())||removed?{transcriptRevision:{increment:1}}:{}),duration:`${Math.floor(end/60000)}:${String(Math.floor(end/1000)%60).padStart(2,"0")}`}})
      return true
    })),
    beginGeneration: (id,revision,runId)=>attempt("beginGeneration",()=>db.$transaction(async tx=>{
      await tx.$queryRaw`SELECT "id" FROM "Note" WHERE "id" = ${id}::uuid FOR UPDATE`
      const n=await tx.note.findUnique({where:{id},include});if(!n||n.transcriptRevision!==revision||n.recordingState==="recording"||(["analyzing_transcript","writing_notes"].includes(n.generationState)&&n.updatedAt.getTime()>Date.now()-180000)||await tx.audioChunk.count({where:{noteId:id,state:{not:"ready"}}}))return null
      return noteDTO(await tx.note.update({where:{id},data:{generationState:"writing_notes",generationRun:runId},include}))
    })),
    finishGeneration: (id,revision,runId,enhanced,title)=>attempt("finishGeneration",()=>db.$transaction(async tx=>{
      await tx.$queryRaw`SELECT "id" FROM "Note" WHERE "id" = ${id}::uuid FOR UPDATE`
      const n=await tx.note.findUnique({where:{id},include});if(!n||n.generationRun!==runId)return null
      if(n.transcriptRevision!==revision||n.recordingState==="recording"||await tx.audioChunk.count({where:{noteId:id,state:{not:"ready"}}})){await tx.note.update({where:{id},data:{generationState:"idle",generationRun:null}});return null}
      // The row is locked: `n.title` is the title as it is now. One the person or a calendar event has given the note is kept.
      return noteDTO(await tx.note.update({where:{id},data:{generationRun:null,generationState:enhanced===null?"failed":"ready",...(enhanced===null?{}:{enhanced,generatedRevision:revision,...(title&&isDefaultNoteTitle(n.title)?{title}:{})})},include}))
    })),

    listNotes: () => attempt("listNotes", async () => (await db.note.findMany({ include, orderBy: { updatedAt: "desc" } })).map(noteDTO)),
    getNote: (id) => attempt("getNote", async () => { const note = await db.note.findUnique({ where: { id }, include }); return note ? noteDTO(note) : null }),
    createNote: input => createNoteForUser(BOOTSTRAP_OWNER_ID, input),
    updateNote: (id, input) => attempt("updateNote", () => db.$transaction(async (tx) => {
      await tx.$queryRaw`SELECT "id" FROM "Note" WHERE "id" = ${id}::uuid FOR UPDATE`
      const current = await tx.note.findUnique({ where: { id }, include })
      if (!current) return null
      input = withoutPlaceholderTitle(input, current.title)
      const changed = input.transcriptSegments !== undefined && transcriptContentKey(input.transcriptSegments) !== transcriptContentKey(current.transcriptSegments)
      const edited = changed || (["title", "body", "enhanced", "duration"] as const).some(key => input[key] !== undefined && input[key] !== current[key]) || input.date !== undefined && input.date !== current.date.toISOString().slice(0, 10)
      const { transcriptSegments, date, ...fields } = input
      return noteDTO(await tx.note.update({ where: { id }, data: {
        ...fields,
        // A completion belongs to the source/document snapshot at begin, not
        // to an edit made while the provider was running. Preserve both layers.
        ...(edited ? { generationRun: null, ...(["analyzing_transcript", "writing_notes"].includes(current.generationState) ? { generationState: "idle" } : {}) } : {}),
        ...(date === undefined ? {} : { date: new Date(`${date}T00:00:00.000Z`) }),
        ...(changed ? { transcriptRevision: { increment: 1 } } : {}),
        ...(transcriptSegments === undefined ? {} : { transcriptSegments: { deleteMany: {}, create: [...transcriptSegments] } }),
      }, include }))
    })),
    deleteNote: (id) => attempt("deleteNote", async () => (await db.note.deleteMany({ where: { id } })).count > 0),
    importNote: command => importNoteForUser(BOOTSTRAP_OWNER_ID, command),
    // The row lock is the one every write to a note takes (edits, recording commands, new audio parts,
    // generation), so nothing can change between the comparison and the delete.
    deleteNoteIf: (id, preconditions) => attempt("deleteNoteIf", () => db.$transaction(async tx => {
      await tx.$queryRaw`SELECT "id" FROM "Note" WHERE "id" = ${id}::uuid FOR UPDATE`
      const note = await tx.note.findUnique({ where: { id }, select: { updatedAt: true, transcriptRevision: true, generatedRevision: true, recordingState: true } })
      if (!note) return "missing" as const
      const parts = await tx.audioChunk.findMany({ where: { noteId: id }, select: { state: true } })
      if (note.recordingState === "recording" || parts.some(part => part.state !== "ready")
        || preconditions.stamp && !matchesStamp(preconditions.stamp, { updatedAt: note.updatedAt.getTime(), transcriptRevision: note.transcriptRevision, generatedRevision: note.generatedRevision })
        || preconditions.chunks !== undefined && preconditions.chunks !== parts.length) return "conflict" as const
      await tx.note.delete({ where: { id } })
      return "deleted" as const
    })),
    listShares: (noteId) => attempt("listShares", async () => (await db.sharedNote.findMany({ where: { noteId }, orderBy: { createdAt: "desc" } })).map(shareDTO)),
    createShare: (share) => attempt("createShare", async () => storedShare(await db.sharedNote.create({ data: {
      id: share.metadata.id, noteId: share.metadata.noteId, tokenHash: share.tokenHash,
      snapshot: { ...share.snapshot }, createdAt: new Date(share.metadata.createdAt),
      expiresAt: share.metadata.expiresAt ? new Date(share.metadata.expiresAt) : null,
    } }))),
    findShare: (tokenHash) => attempt("findShare", async () => { const share = await db.sharedNote.findUnique({ where: { tokenHash } }); return share ? storedShare(share) : null }),
    revokeShare: (id, now) => attempt("revokeShare", async () => {
      await db.sharedNote.updateMany({ where: { id, revokedAt: null }, data: { revokedAt: new Date(now) } })
      const share = await db.sharedNote.findUnique({ where: { id } })
      return share ? shareDTO(share) : null
    }),
  }
  return {
    repository,
    database: db,
    ready: async (): Promise<boolean> => { await db.$queryRaw`SELECT 1`; return true },
    close: () => db.$disconnect(),
  }
}
