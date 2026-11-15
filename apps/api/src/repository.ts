import { Context, Effect } from "effect"
import type { AudioChunkDTO, TranscriptSegmentDTO, CreateNoteInput, NoteDTO, ShareDTO, SharedNoteDTO, UpdateNoteInput } from "@meeting-notes/contracts"
import {selectMeetings,selectTranscriptPassages,type MeetingSearchInput,type TranscriptSearchInput,type TranscriptSearchHit} from "./meeting-search"
import type {NoteSearchPageDTO,NoteSummariesPageDTO} from "@meeting-notes/contracts"
import type {NoteListInput,NoteSearchInput} from "./note-summaries"
import {makeMemoryLiveRepository,type ChunkResolution,type FinishChunkOptions,type LiveRepository} from "./live/repository"

export class RepositoryError extends Error {
  readonly _tag = "RepositoryError"
  constructor(readonly operation: string) { super("Persistence operation failed") }
}
export interface StoredShare {
  readonly metadata: ShareDTO
  readonly tokenHash: string
  readonly snapshot: SharedNoteDTO
}
export interface StoredAudioChunk extends AudioChunkDTO {
  readonly hash: string
  readonly durationMs: number
  readonly lease: string | null
  readonly leaseUntil: string | null
  /** How a ready part was accounted for (live coverage ledger); absent on rows written before it existed. */
  readonly resolution?: ChunkResolution | null
}
export interface NotesRepositoryService {
  /** Request-facing adapters must be scoped to a stable account ID. */
  readonly forUser: (userId: string, organizationId?: string) => NotesRepositoryService
  readonly recording: (id: string, command: "start" | "stop" | "resume") => Effect.Effect<NoteDTO | null, RepositoryError>
  readonly listAudioChunks: (noteId: string) => Effect.Effect<ReadonlyArray<StoredAudioChunk>, RepositoryError>
  readonly putAudioChunk: (chunk: StoredAudioChunk) => Effect.Effect<StoredAudioChunk | null, RepositoryError>
  readonly retryAudioChunk: (noteId: string, id: string) => Effect.Effect<StoredAudioChunk | null, RepositoryError>
  readonly claimAudioChunk: (now: string) => Effect.Effect<StoredAudioChunk | null, RepositoryError>
  readonly finishAudioChunk: (id: string, lease: string, segments: ReadonlyArray<TranscriptSegmentDTO>, error: string | null, options?: FinishChunkOptions) => Effect.Effect<boolean, RepositoryError>
  /** Live transcription storage, for the trusted session and worker only (see live/repository.ts).
   * Request-scoped repositories leave it out. */
  readonly live?: LiveRepository
  readonly beginGeneration: (id: string, revision: number, runId: string) => Effect.Effect<NoteDTO | null, RepositoryError>
  readonly finishGeneration: (id: string, revision: number, runId: string, enhanced: string | null) => Effect.Effect<NoteDTO | null, RepositoryError>

  readonly listNotes: () => Effect.Effect<ReadonlyArray<NoteDTO>, RepositoryError>
  readonly searchNotes?: (input:MeetingSearchInput) => Effect.Effect<ReadonlyArray<NoteDTO>,RepositoryError>
  readonly searchTranscriptSegments?: (input:TranscriptSearchInput)=>Effect.Effect<ReadonlyArray<TranscriptSearchHit>,RepositoryError>
  /** A page of list rows without bodies or transcripts. A repository that leaves these
   * out is served from `listNotes` by the functions in `note-summaries.ts`. */
  readonly listNoteSummaries?: (input:NoteListInput) => Effect.Effect<NoteSummariesPageDTO,RepositoryError>
  readonly searchNoteSummaries?: (input:NoteSearchInput) => Effect.Effect<NoteSearchPageDTO,RepositoryError>
  readonly getNote: (id: string) => Effect.Effect<NoteDTO | null, RepositoryError>
  readonly createNote: (input: CreateNoteInput) => Effect.Effect<NoteDTO, RepositoryError>
  readonly updateNote: (id: string, input: UpdateNoteInput) => Effect.Effect<NoteDTO | null, RepositoryError>
  readonly deleteNote: (id: string) => Effect.Effect<boolean, RepositoryError>
  readonly listShares: (noteId: string) => Effect.Effect<ReadonlyArray<ShareDTO>, RepositoryError>
  readonly createShare: (share: StoredShare) => Effect.Effect<StoredShare, RepositoryError>
  readonly findShare: (tokenHash: string) => Effect.Effect<StoredShare | null, RepositoryError>
  readonly revokeShare: (id: string, now: string) => Effect.Effect<ShareDTO | null, RepositoryError>
}
export class NotesRepository extends Context.Service<NotesRepository, NotesRepositoryService>()("MeetingNotesRepository") {}

export const BOOTSTRAP_OWNER_ID = "00000000-0000-4000-8000-000000000001"

/** Ownership is immutable. Check it before every note-derived operation, including
 * audio jobs and share revocation. Only the trusted background worker uses the
 * unscoped adapter; a request can never claim or complete somebody else's job. */
export function scopeRepository(repository: NotesRepositoryService, userId: string, access: {
  ownsNote: (id: string) => Effect.Effect<boolean, RepositoryError>
  ownsShare: (id: string) => Effect.Effect<boolean, RepositoryError>
  listNotes: NotesRepositoryService["listNotes"]
  searchNotes?: NonNullable<NotesRepositoryService["searchNotes"]>
  searchTranscriptSegments?:NonNullable<NotesRepositoryService["searchTranscriptSegments"]>
  listNoteSummaries?:NonNullable<NotesRepositoryService["listNoteSummaries"]>
  searchNoteSummaries?:NonNullable<NotesRepositoryService["searchNoteSummaries"]>
  createNote: NotesRepositoryService["createNote"]
}, organizationId?: string): NotesRepositoryService {
  const note = <A>(id: string, operation: () => Effect.Effect<A, RepositoryError>, absent: A) =>
    Effect.flatMap(access.ownsNote(id), allowed => allowed ? operation() : Effect.succeed(absent))
  const scoped: NotesRepositoryService = {
    forUser: (id, workspace) => id === userId && (workspace === undefined || workspace === organizationId) ? scoped : repository.forUser(id, workspace),
    listNotes: access.listNotes,
    searchNotes:input=>access.searchNotes?access.searchNotes(input):Effect.map(access.listNotes(),notes=>selectMeetings(notes,input)),
    searchTranscriptSegments:input=>access.searchTranscriptSegments?access.searchTranscriptSegments(input):Effect.map(access.listNotes(),notes=>selectTranscriptPassages(notes,input)),
    ...(access.listNoteSummaries?{listNoteSummaries:access.listNoteSummaries}:{}),
    ...(access.searchNoteSummaries?{searchNoteSummaries:access.searchNoteSummaries}:{}),
    createNote: access.createNote,
    getNote: id => note(id, () => repository.getNote(id), null),
    updateNote: (id, input) => note(id, () => repository.updateNote(id, input), null),
    deleteNote: id => note(id, () => repository.deleteNote(id), false),
    recording: (id, command) => note(id, () => repository.recording(id, command), null),
    listAudioChunks: id => note(id, () => repository.listAudioChunks(id), []),
    putAudioChunk: chunk => note(chunk.noteId, () => repository.putAudioChunk(chunk), null),
    retryAudioChunk: (noteId, id) => note(noteId, () => repository.retryAudioChunk(noteId, id), null),
    claimAudioChunk: () => Effect.succeed(null),
    finishAudioChunk: () => Effect.succeed(false),
    beginGeneration: (id, revision, runId) => note(id, () => repository.beginGeneration(id, revision, runId), null),
    finishGeneration: (id, revision, runId, enhanced) => note(id, () => repository.finishGeneration(id, revision, runId, enhanced), null),
    listShares: id => note(id, () => repository.listShares(id), []),
    createShare: share => Effect.flatMap(access.ownsNote(share.metadata.noteId), allowed => allowed
      ? repository.createShare(share) : Effect.fail(new RepositoryError("createShare"))),
    // Capability lookup is reserved for the anonymous share-reader handler.
    findShare: () => Effect.succeed(null),
    revokeShare: (id, now) => Effect.flatMap(access.ownsShare(id), allowed => allowed
      ? repository.revokeShare(id, now) : Effect.succeed(null)),
  }
  return scoped
}

/** Changes to timing/finality alone do not make already-generated text stale. */
export function transcriptContentKey(segments: ReadonlyArray<NoteDTO["transcriptSegments"][number]>) {
  return JSON.stringify([...segments].sort((a, b) => a.startMs - b.startMs || a.id.localeCompare(b.id))
    .filter((segment) => segment.text.trim().length > 0)
    .map(({ channel, speakerId, speakerLabel, text }) => ({ channel, speakerId, speakerLabel, text })))
}

/** Deterministic adapter used to test the real HTTP layer without a live database. */
export function makeMemoryRepository(seed: ReadonlyArray<NoteDTO> = [], clock: () => Date = () => new Date()): NotesRepositoryService {
  const notes = new Map(seed.map((note) => [note.id, structuredClone(note)]))
  const owners = new Map(seed.map(note => [note.id, BOOTSTRAP_OWNER_ID]))
  const shares = new Map<string, StoredShare>()
  const chunks = new Map<string, StoredAudioChunk>()
  const generationRuns = new Map<string,string>()
  const clone = <T>(value: T): T => structuredClone(value)
  const liveState = makeMemoryLiveRepository({ notes, chunks, clock })
  const repository: NotesRepositoryService = {
    live: liveState.live,
    forUser: userId => scopeRepository(repository, userId, {
      ownsNote: id => Effect.sync(() => owners.get(id) === userId && notes.has(id)),
      ownsShare: id => Effect.sync(() => [...shares.values()].some(s => s.metadata.id === id && owners.get(s.metadata.noteId) === userId)),
      listNotes: () => Effect.map(repository.listNotes(), rows => rows.filter(n => owners.get(n.id) === userId)),
      createNote: input => Effect.map(repository.createNote(input), created => { owners.set(created.id, userId); return created }),
    }),
    recording: (id, command) => Effect.sync(() => {
      const n = notes.get(id)
      if (!n || (command === "start" && n.recordingState === "stopped") || (command === "resume" && n.recordingState === "idle")) return null
      if(command!=="stop")generationRuns.delete(id)
      const next: NoteDTO = { ...n, ...(command!=="stop"&&n.generationState==="writing_notes"?{generationState:"idle" as const}:{}), recordingState: command === "stop" ? "stopped" : "recording", updatedAt: clock().toISOString() }
      notes.set(id, next); return clone(next)
    }),
    listAudioChunks: (noteId) => Effect.sync(() => clone([...chunks.values()].filter(c => c.noteId === noteId).sort((a,b) => a.sequence-b.sequence))),
    putAudioChunk: (chunk) => Effect.sync(() => {
      if (!notes.has(chunk.noteId)) return null
      const existing = chunks.get(chunk.id) ?? [...chunks.values()].find(c => c.noteId === chunk.noteId && c.channel === chunk.channel && c.sequence === chunk.sequence)
      if (existing) return existing.id === chunk.id && existing.hash === chunk.hash && existing.noteId === chunk.noteId && existing.channel === chunk.channel && existing.sequence === chunk.sequence && existing.startMs === chunk.startMs ? clone(existing) : null
      chunks.set(chunk.id, clone(chunk)); const note=notes.get(chunk.noteId)!;if(note.generationState==="writing_notes"){generationRuns.delete(chunk.noteId);notes.set(chunk.noteId,{...note,generationState:"idle"})} return clone(chunk)
    }),
    retryAudioChunk: (noteId, id) => Effect.sync(() => {
      const c = chunks.get(id); if (!c || c.noteId !== noteId) return null
      const next: StoredAudioChunk = c.state === "failed" ? { ...c, state: "queued", error: null, lease: null, leaseUntil: null } : c
      chunks.set(id, next); return clone(next)
    }),
    claimAudioChunk: (now) => Effect.sync(() => {
      const c = [...chunks.values()].find(c => c.state === "queued" || c.state === "processing" && (c.leaseUntil ?? "") <= now)
      if (!c) return null
      const next: StoredAudioChunk = { ...c, state: "processing", error: null, lease: crypto.randomUUID(), leaseUntil: new Date(Date.parse(now)+180000).toISOString() }
      chunks.set(c.id, next); return clone(next)
    }),
    finishAudioChunk: (id, lease, segments, error, options) => Effect.sync(() => {
      const c = chunks.get(id); if (!c || c.state !== "processing" || c.lease !== lease) return false
      const n = notes.get(c.noteId); if (!n) return false
      const replaced = error === null && options?.replaceLive ? liveState.replaceLiveWithin(c, n.transcriptSegments) : { kept: n.transcriptSegments, removed: 0 }
      const merged = [...replaced.kept, ...clone(segments)].sort((a,b) => a.startMs-b.startMs)
      const end = Math.max(0,...merged.map(s => s.endMs ?? s.startMs), ...[...chunks.values()].filter(x => x.noteId===n.id && (x.state === "ready" || x.id===id && error===null)).map(x=>x.startMs+x.durationMs))
      const duration = `${Math.floor(end/60000)}:${String(Math.floor(end/1000)%60).padStart(2,"0")}`
      notes.set(n.id,{...n,transcriptSegments:merged,transcriptRevision:n.transcriptRevision+(segments.some(s=>s.text.trim())||replaced.removed?1:0),duration,updatedAt:clock().toISOString()})
      chunks.set(id,{...c,state:error===null?"ready":"failed",error,lease:null,leaseUntil:null,...(error===null?{resolution:options?.resolution??"batch"}:{})}); return true
    }),
    beginGeneration: (id, revision, runId) => Effect.sync(() => {
      const n = notes.get(id)
      if (!n || n.transcriptRevision!==revision || n.recordingState==="recording" || ["analyzing_transcript","writing_notes"].includes(n.generationState) || [...chunks.values()].some(c=>c.noteId===id && c.state!=="ready")) return null
      const next: NoteDTO = {...n,generationState:"writing_notes",updatedAt:clock().toISOString()};notes.set(id,next);generationRuns.set(id,runId);return clone(next)
    }),
    finishGeneration: (id, revision, runId, enhanced) => Effect.sync(() => {
      const n=notes.get(id);if(!n || generationRuns.get(id)!==runId)return null
      generationRuns.delete(id)
      if(n.transcriptRevision!==revision||n.recordingState==="recording"||[...chunks.values()].some(c=>c.noteId===id&&c.state!=="ready")){notes.set(id,{...n,generationState:"idle"});return null}
      const next:NoteDTO={...n,generationState:enhanced===null?"failed":"ready",...(enhanced===null?{}:{enhanced,generatedRevision:revision}),updatedAt:clock().toISOString()};notes.set(id,next);return clone(next)
    }),
    listNotes: () => Effect.sync(() => clone([...notes.values()].sort((a, b) => b.updatedAt.localeCompare(a.updatedAt)))),
    getNote: (id) => Effect.sync(() => clone(notes.get(id) ?? null)),
    createNote: (input) => Effect.sync(() => {
      const now = clock().toISOString()
      const note: NoteDTO = {
        id: crypto.randomUUID(), title: input.title, body: input.body ?? "", enhanced: input.enhanced ?? "",
        date: input.date ?? now.slice(0, 10), duration: input.duration ?? "", createdAt: now, updatedAt: now,
        transcriptSegments: clone(input.transcriptSegments ?? []),
        recordingState: "idle", generationState: "idle", transcriptRevision: transcriptContentKey(input.transcriptSegments ?? []) === "[]" ? 0 : 1, generatedRevision: null,
      }
      notes.set(note.id, note)
      owners.set(note.id, BOOTSTRAP_OWNER_ID)
      return clone(note)
    }),
    updateNote: (id, input) => Effect.sync(() => {
      const note = notes.get(id)
      if (!note) return null
      const changed = input.transcriptSegments !== undefined && transcriptContentKey(input.transcriptSegments) !== transcriptContentKey(note.transcriptSegments)
      const edited = changed || (["title", "body", "enhanced", "duration", "date"] as const).some(key => input[key] !== undefined && input[key] !== note[key])
      if (edited) generationRuns.delete(id)
      const updated: NoteDTO = { ...note, ...clone(input), ...(edited && ["analyzing_transcript", "writing_notes"].includes(note.generationState) ? {generationState:"idle" as const} : {}), updatedAt: clock().toISOString(), transcriptRevision: note.transcriptRevision + (changed ? 1 : 0) }
      notes.set(id, updated)
      return clone(updated)
    }),
    deleteNote: (id) => Effect.sync(() => {
      for (const [chunkId, chunk] of chunks) if (chunk.noteId === id) chunks.delete(chunkId)
      for (const [hash, share] of shares) if (share.metadata.noteId === id) shares.delete(hash)
      liveState.forget(id)
      return notes.delete(id)
    }),
    listShares: (noteId) => Effect.sync(() => clone([...shares.values()].filter((share) => share.metadata.noteId === noteId).map((share) => share.metadata))),
    createShare: (share) => Effect.sync(() => { shares.set(share.tokenHash, clone(share)); return clone(share) }),
    findShare: (hash) => Effect.sync(() => clone(shares.get(hash) ?? null)),
    revokeShare: (id, now) => Effect.sync(() => {
      const entry = [...shares.entries()].find(([, share]) => share.metadata.id === id)
      if (!entry) return null
      const [hash, share] = entry
      const revoked = { ...share, metadata: { ...share.metadata, revokedAt: share.metadata.revokedAt ?? now } }
      shares.set(hash, revoked)
      return clone(revoked.metadata)
    }),
  }
  return repository
}
