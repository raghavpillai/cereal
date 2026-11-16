/**
 * Saved chat conversations: `/api/v1/chats`.
 *
 * Every thread is one person's (`userId`), and every read and write here names that person. There is no
 * administrator path: an owner or admin asking for somebody else's thread is told 404, exactly as for an
 * id that does not exist, so nothing here says whose ids exist.
 *
 * Exchange `turn` of a thread is two rows: the question at position 2 * turn and its answer at 2 * turn + 1.
 * The answer to a question asked through `POST /chats/:id/stream` is written by the server, not by the
 * client, in three steps:
 *
 *  1. Before the model is asked, in one transaction with the thread locked: the turn is checked (the next
 *     one, or an earlier one with `replace`), the turns it replaces are removed, and the question and an
 *     empty answer marked "streaming" are inserted. The thread's earlier exchanges, read in that same
 *     transaction, are the history sent to the model.
 *  2. While it streams, what has arrived is written to that answer row about once a second.
 *  3. When the stream ends, however it ends, the answer is settled: "complete" after `done`, "failed" with
 *     its message after `error`, and "interrupted" with whatever had arrived when the client went away,
 *     the request was stopped or the server timed out. A server that dies mid-answer leaves a "streaming"
 *     row that is reported as "interrupted" once it is older than STALE_STREAM_MS.
 *
 * So a turn cannot be lost: nothing depends on the client being there at the end. And it cannot be stored
 * twice: its place is the exchange number the client named, held unique per thread by the database, and a
 * second request for a place that is taken is refused (409) unless it says it replaces it.
 */
import { Effect, Schema, Stream } from "effect"
import type { Database } from "@meeting-notes/database"
import { ChatCitationDTO, ChatToolProgressDTO, type ChatInput, type ChatStreamEventDTO } from "@meeting-notes/contracts"
import type { ApiError } from "@meeting-notes/contracts/api"
import { MAX_CHAT_TURNS, type AppendChatTurnInput, type ChatThreadDTO, type ChatThreadMessageDTO, type ChatThreadStreamInput, type ChatThreadSummaryDTO, type ChatThreadsPageDTO, type ChatTurnAnswerInput, type CreateChatThreadInput } from "@meeting-notes/contracts/chats"
import type { NotesRepositoryService } from "./repository"
import { cleanNoteTitle } from "./ai/note-title"
import type { ProviderServices } from "./providers"

/** A "streaming" answer not written for this long belongs to a request that is gone (the stream itself gives up after two minutes). */
export const STALE_STREAM_MS = 150_000
/** How often an answer that is arriving is written. */
const FLUSH_MS = 1_000
/** Exchanges sent to the model with a question, newest last (`ChatInput.history` holds 30 messages). */
const HISTORY_TURNS = 15

type Status = ChatThreadMessageDTO["status"]
type TitleSource = NonNullable<ChatThreadSummaryDTO["titleSource"]>
export interface StoredThread {
  readonly id: string; readonly userId: string; readonly noteId: string | null
  readonly title: string | null; readonly titleSource: TitleSource | null; readonly turnCount: number
  readonly createdAt: Date; readonly updatedAt: Date
}
export interface StoredMessage {
  readonly id: string; readonly position: number; readonly role: "user" | "assistant"; readonly content: string
  readonly status: Status; readonly error: string | null; readonly model: string | null
  readonly citations: ReadonlyArray<ChatCitationDTO>; readonly tools: ReadonlyArray<ChatToolProgressDTO>
  readonly elapsedMs: number | null; readonly createdAt: Date; readonly updatedAt: Date
}
export interface ThreadWithMessages { readonly thread: StoredThread; readonly messages: ReadonlyArray<StoredMessage> }
export interface ListQuery {
  readonly limit: number
  /** Threads strictly after this one in the newest-first order. */
  readonly after?: { readonly updatedAt: Date; readonly id: string }
  /** `undefined`: every thread; `null`: only those across all notes; a UUID: only that note's. */
  readonly noteId?: string | null
}
/** What a turn request found, decided with the thread locked. */
export type TurnOutcome =
  | { readonly kind: "missing" }
  | { readonly kind: "conflict" }
  | { readonly kind: "invalid"; readonly message: string }
  | { readonly kind: "unchanged"; readonly thread: ThreadWithMessages }
  | { readonly kind: "written"; readonly thread: ThreadWithMessages; readonly history: ReadonlyArray<StoredMessage>; readonly answerId: string }
export interface TurnCommand {
  readonly turn: number; readonly replace: boolean; readonly question: string; readonly now: Date
  /** A finished answer to store (append), or undefined for one about to stream. */
  readonly answer?: ChatTurnAnswerInput
  readonly model: string | null
}
export interface AnswerPatch {
  readonly content: string; readonly status: Status; readonly error: string | null
  readonly citations: ReadonlyArray<ChatCitationDTO>; readonly tools: ReadonlyArray<ChatToolProgressDTO>
  readonly elapsedMs: number | null; readonly now: Date
}
export interface NewThread {
  readonly id: string; readonly noteId: string | null; readonly title: string | null; readonly titleSource: TitleSource | null
  readonly createdAt: Date; readonly updatedAt: Date
}
/** Persistence, always scoped by the person: no method reads or writes a thread without its owner's id. */
export interface ChatStore {
  list(userId: string, query: ListQuery): Promise<{ threads: ReadonlyArray<StoredThread>; total: number }>
  get(userId: string, id: string): Promise<ThreadWithMessages | null>
  /** "taken": the id is another person's thread. Nothing is written unless "created". */
  create(userId: string, thread: NewThread): Promise<"created" | "exists" | "taken">
  turn(userId: string, id: string, command: TurnCommand): Promise<TurnOutcome>
  /** Writes an answer row of the person's thread; false when it is gone (the thread was deleted or the turn replaced). */
  answer(userId: string, threadId: string, messageId: string, patch: AnswerPatch, touch: boolean): Promise<boolean>
  /** `onlyUntitled`: only while the thread has no title (a generated title never replaces any other). A name
   * does not move the thread in the list: `updatedAt` is when it was last asked or answered. */
  title(userId: string, id: string, title: string, source: TitleSource, onlyUntitled: boolean): Promise<StoredThread | null>
  delete(userId: string, id: string): Promise<boolean>
}

const positionOf = (turn: number, role: "user" | "assistant") => 2 * turn + (role === "assistant" ? 1 : 0)
const turnOf = (position: number) => Math.floor(position / 2)
const sameAnswer = (stored: StoredMessage | undefined, answer: ChatTurnAnswerInput) =>
  !!stored && stored.content === answer.content && stored.status === answer.status && (stored.error ?? null) === (answer.error ?? null)
/** The rule for a turn request, the same in both stores. `messages` are the thread's, in order. */
export function planTurn(thread: StoredThread, messages: ReadonlyArray<StoredMessage>, command: TurnCommand): { kind: "conflict" } | { kind: "invalid"; message: string } | { kind: "unchanged" } | { kind: "write"; keep: ReadonlyArray<StoredMessage> } {
  if (command.turn > thread.turnCount) return { kind: "invalid", message: `The thread has ${thread.turnCount} exchanges; the next one is ${thread.turnCount}` }
  if (command.turn < thread.turnCount) {
    const question = messages.find(m => m.position === positionOf(command.turn, "user"))
    const answer = messages.find(m => m.position === positionOf(command.turn, "assistant"))
    // A request sent again after its answer was lost on the way back.
    if (command.answer && question?.content === command.question && sameAnswer(answer, command.answer)) return { kind: "unchanged" }
    if (!command.replace) return { kind: "conflict" }
  }
  if (command.turn >= MAX_CHAT_TURNS) return { kind: "invalid", message: `A thread holds at most ${MAX_CHAT_TURNS} exchanges; start a new one` }
  return { kind: "write", keep: messages.filter(m => m.position < positionOf(command.turn, "user")) }
}
function turnRows(command: TurnCommand, answerId: string): [StoredMessage, StoredMessage] {
  const answer = command.answer
  return [
    { id: crypto.randomUUID(), position: positionOf(command.turn, "user"), role: "user", content: command.question, status: "complete", error: null, model: command.model,
      citations: [], tools: [], elapsedMs: null, createdAt: command.now, updatedAt: command.now },
    { id: answerId, position: positionOf(command.turn, "assistant"), role: "assistant", content: answer?.content ?? "", status: answer?.status ?? "streaming",
      error: answer?.error ?? null, model: answer?.model ?? command.model, citations: answer?.citations ?? [], tools: answer?.tools ?? [],
      elapsedMs: answer?.elapsedMs ?? null, createdAt: command.now, updatedAt: command.now },
  ]
}

// ---------------------------------------------------------------- PostgreSQL

const Citations = Schema.Array(ChatCitationDTO), Tools = Schema.Array(ChatToolProgressDTO)
const decodeOr = <A>(schema: Schema.Codec<A, unknown>, value: unknown): A => { try { return Schema.decodeUnknownSync(schema)(value) } catch { return [] as unknown as A } }
const json = (value: unknown) => JSON.parse(JSON.stringify(value))
type ThreadRow = { id: string; userId: string; noteId: string | null; title: string | null; titleSource: string | null; turnCount: number; createdAt: Date; updatedAt: Date }
type MessageRow = { id: string; position: number; role: string; content: string; status: string; error: string | null; model: string | null; citations: unknown; tools: unknown; elapsedMs: number | null; createdAt: Date; updatedAt: Date }
const threadOf = (row: ThreadRow): StoredThread => ({ ...row, titleSource: row.titleSource as TitleSource | null })
const messageOf = (row: MessageRow): StoredMessage => ({ ...row, role: row.role === "assistant" ? "assistant" : "user", status: row.status as Status,
  citations: decodeOr(Citations as Schema.Codec<ReadonlyArray<ChatCitationDTO>, unknown>, row.citations), tools: decodeOr(Tools as Schema.Codec<ReadonlyArray<ChatToolProgressDTO>, unknown>, row.tools) })
const messageData = (threadId: string, m: StoredMessage) => ({ id: m.id, threadId, position: m.position, role: m.role, content: m.content, status: m.status, error: m.error, model: m.model,
  citations: json(m.citations), tools: json(m.tools), elapsedMs: m.elapsedMs, createdAt: m.createdAt, updatedAt: m.updatedAt })
const isUniqueViolation = (error: unknown) => typeof error === "object" && error !== null && "code" in error && (error as { code: unknown }).code === "P2002"

export function makePrismaChatStore(db: Database): ChatStore {
  const scope = (userId: string, noteId: string | null | undefined) => ({ userId, ...(noteId !== undefined ? { noteId } : {}) })
  const read = async (tx: Pick<Database, "chatThread" | "chatMessage">, userId: string, id: string): Promise<ThreadWithMessages | null> => {
    const thread = await tx.chatThread.findFirst({ where: { id, userId } })
    if (!thread) return null
    const messages = await tx.chatMessage.findMany({ where: { threadId: id }, orderBy: { position: "asc" } })
    return { thread: threadOf(thread), messages: messages.map(messageOf) }
  }
  return {
    list: async (userId, query) => {
      // A thread nothing was ever asked in and nobody named (its first question never started) is not listed.
      const where = { ...scope(userId, query.noteId), OR: [{ turnCount: { gt: 0 } }, { title: { not: null } }] }
      const after = query.after ? { OR: [{ updatedAt: { lt: query.after.updatedAt } }, { updatedAt: query.after.updatedAt, id: { lt: query.after.id } }] } : {}
      const [rows, total] = await Promise.all([
        db.chatThread.findMany({ where: { ...where, ...after }, orderBy: [{ updatedAt: "desc" }, { id: "desc" }], take: query.limit }),
        db.chatThread.count({ where }),
      ])
      return { threads: rows.map(threadOf), total }
    },
    get: (userId, id) => read(db, userId, id),
    create: async (userId, thread) => {
      const existing = async () => (await db.chatThread.findUnique({ where: { id: thread.id }, select: { userId: true } }))?.userId === userId ? "exists" as const : "taken" as const
      if (await db.chatThread.findUnique({ where: { id: thread.id }, select: { id: true } })) return existing()
      try {
        await db.chatThread.create({ data: { id: thread.id, userId, noteId: thread.noteId, title: thread.title, titleSource: thread.titleSource,
          createdAt: thread.createdAt, updatedAt: thread.updatedAt } })
        return "created"
      } catch (error) {
        if (isUniqueViolation(error)) return existing()
        throw error
      }
    },
    turn: (userId, id, command) => db.$transaction(async tx => {
      const locked = await tx.$queryRaw<Array<{ id: string }>>`SELECT "id" FROM "ChatThread" WHERE "id" = ${id}::uuid AND "userId" = ${userId}::uuid FOR UPDATE`
      if (!locked.length) return { kind: "missing" as const }
      const current = (await read(tx, userId, id))!
      const plan = planTurn(current.thread, current.messages, command)
      if (plan.kind === "unchanged") return { kind: "unchanged" as const, thread: current }
      if (plan.kind !== "write") return plan
      const answerId = crypto.randomUUID(), rows = turnRows(command, answerId)
      await tx.chatMessage.deleteMany({ where: { threadId: id, position: { gte: positionOf(command.turn, "user") } } })
      await tx.chatMessage.createMany({ data: rows.map(m => messageData(id, m)) })
      const thread = await tx.chatThread.update({ where: { id }, data: { turnCount: command.turn + 1, updatedAt: command.now } })
      return { kind: "written" as const, thread: { thread: threadOf(thread), messages: [...plan.keep, ...rows] }, history: plan.keep, answerId }
    }),
    answer: async (userId, threadId, messageId, patch, touch) => {
      const updated = await db.chatMessage.updateMany({ where: { id: messageId, threadId, thread: { userId } }, data: { content: patch.content, status: patch.status, error: patch.error,
        citations: json(patch.citations), tools: json(patch.tools), elapsedMs: patch.elapsedMs, updatedAt: patch.now } })
      if (updated.count && touch) await db.chatThread.updateMany({ where: { id: threadId, userId }, data: { updatedAt: patch.now } })
      return updated.count > 0
    },
    title: async (userId, id, title, source, onlyUntitled) => {
      const updated = await db.chatThread.updateMany({ where: { id, userId, ...(onlyUntitled ? { title: null } : {}) }, data: { title, titleSource: source } })
      if (!updated.count) return null
      const row = await db.chatThread.findFirst({ where: { id, userId } })
      return row ? threadOf(row) : null
    },
    delete: async (userId, id) => (await db.chatThread.deleteMany({ where: { id, userId } })).count > 0,
  }
}

// ---------------------------------------------------------------- Memory (tests and the server without a database)

/** `notes` is the unscoped repository: a thread about a note that is gone is gone too, as the database's cascade does. */
export function makeMemoryChatStore(notes?: NotesRepositoryService): ChatStore {
  const threads = new Map<string, { thread: StoredThread; messages: StoredMessage[] }>()
  let queue: Promise<unknown> = Promise.resolve()
  const serial = <A>(task: () => Promise<A> | A): Promise<A> => { const next = queue.then(task); queue = next.then(() => {}, () => {}); return next }
  const prune = async () => {
    if (!notes) return
    for (const [id, entry] of threads) if (entry.thread.noteId && !(await Effect.runPromise(notes.getNote(entry.thread.noteId)))) threads.delete(id)
  }
  const own = (userId: string, id: string) => { const entry = threads.get(id); return entry && entry.thread.userId === userId ? entry : undefined }
  const order = (a: StoredThread, b: StoredThread) => b.updatedAt.getTime() - a.updatedAt.getTime() || (a.id < b.id ? 1 : a.id > b.id ? -1 : 0)
  const copy = (entry: { thread: StoredThread; messages: StoredMessage[] }): ThreadWithMessages => ({ thread: { ...entry.thread }, messages: entry.messages.map(m => ({ ...m })) })
  return {
    list: (userId: string, query: ListQuery) => serial(async () => {
      await prune()
      const mine = [...threads.values()].map(e => e.thread).filter(t => t.userId === userId && (t.turnCount > 0 || t.title !== null) && (query.noteId === undefined || t.noteId === query.noteId)).sort(order)
      const after = query.after
      const page = after ? mine.filter(t => t.updatedAt.getTime() < after.updatedAt.getTime() || (t.updatedAt.getTime() === after.updatedAt.getTime() && t.id < after.id)) : mine
      return { threads: page.slice(0, query.limit).map(t => ({ ...t })), total: mine.length }
    }),
    get: (userId: string, id: string) => serial(async () => { await prune(); const entry = own(userId, id); return entry ? copy(entry) : null }),
    create: (userId: string, thread: NewThread) => serial(async () => {
      const existing = threads.get(thread.id)
      if (existing) return existing.thread.userId === userId ? "exists" as const : "taken" as const
      threads.set(thread.id, { thread: { id: thread.id, userId, noteId: thread.noteId, title: thread.title, titleSource: thread.titleSource, turnCount: 0,
        createdAt: thread.createdAt, updatedAt: thread.updatedAt }, messages: [] })
      return "created" as const
    }),
    turn: (userId: string, id: string, command: TurnCommand) => serial(async (): Promise<TurnOutcome> => {
      await prune()
      const entry = own(userId, id)
      if (!entry) return { kind: "missing" }
      const plan = planTurn(entry.thread, entry.messages, command)
      if (plan.kind === "unchanged") return { kind: "unchanged", thread: copy(entry) }
      if (plan.kind !== "write") return plan
      const answerId = crypto.randomUUID(), rows = turnRows(command, answerId)
      entry.messages = [...plan.keep.map(m => ({ ...m })), ...rows]
      entry.thread = { ...entry.thread, turnCount: command.turn + 1, updatedAt: command.now }
      return { kind: "written", thread: copy(entry), history: plan.keep.map(m => ({ ...m })), answerId }
    }),
    answer: (userId: string, threadId: string, messageId: string, patch: AnswerPatch, touch: boolean) => serial(() => {
      const entry = own(userId, threadId), index = entry?.messages.findIndex(m => m.id === messageId) ?? -1
      if (!entry || index < 0) return false
      entry.messages[index] = { ...entry.messages[index]!, content: patch.content, status: patch.status, error: patch.error, citations: patch.citations, tools: patch.tools, elapsedMs: patch.elapsedMs, updatedAt: patch.now }
      if (touch) entry.thread = { ...entry.thread, updatedAt: patch.now }
      return true
    }),
    title: (userId: string, id: string, title: string, source: TitleSource, onlyUntitled: boolean) => serial(() => {
      const entry = own(userId, id)
      if (!entry || (onlyUntitled && entry.thread.title !== null)) return null
      entry.thread = { ...entry.thread, title, titleSource: source }
      return { ...entry.thread }
    }),
    delete: (userId: string, id: string) => serial(() => own(userId, id) ? threads.delete(id) : false),
  }
}

// ---------------------------------------------------------------- Titles

const TITLE_ANSWER_CHARACTERS = 4_000
/** The name a thread gets when the model cannot give one: the start of its first question, on one line. */
export function questionTitle(question: string): string {
  const line = question.split(/\s+/).filter(Boolean).join(" ")
  if ([...line].length <= 70) return line || "Untitled thread"
  const cut = [...line].slice(0, 69).join("").trimEnd()
  return cut + "…"
}
/**
 * The model's name for a thread, from its first exchange, through the same request and the same checks
 * that name an untitled meeting (`ai/note-title.ts`): the exchange is what the "notes" are, and the
 * question stands in for the opening of the transcript so the title comes out in the language asked in.
 */
export function chatTitler(title: ProviderServices["title"]): ((question: string, answer: string) => Promise<string | null>) | undefined {
  if (!title) return undefined
  return async (question, answer) => {
    const stamp = new Date().toISOString()
    const notes = `Question: ${question}\n\nAnswer: ${answer.slice(0, TITLE_ANSWER_CHARACTERS)}`
    const note = { id: crypto.randomUUID(), title: "", body: "", enhanced: "", date: stamp.slice(0, 10), duration: "", createdAt: stamp, updatedAt: stamp,
      transcriptSegments: [{ id: crypto.randomUUID(), channel: "mic" as const, speakerId: "asker", speakerLabel: null, text: question.slice(0, 600), isFinal: true, startMs: 0, endMs: null }],
      recordingState: "idle" as const, generationState: "idle" as const, transcriptRevision: 0, generatedRevision: null }
    const exit = await Effect.runPromise(Effect.exit(title(note, notes)))
    return exit._tag === "Success" ? cleanNoteTitle(exit.value) : null
  }
}

// ---------------------------------------------------------------- The service the routes call

const notFound: ApiError = { error: "not_found", message: "Chat not found" }
const unavailable: ApiError = { error: "unavailable", message: "Saved chats are temporarily unavailable" }
const conflict: ApiError = { error: "conflict", message: "This exchange of the chat has changed. Reload the chat and ask again." }
const invalid = (message: string): ApiError => ({ error: "invalid_request", message })

const iso = (date: Date) => date.toISOString()
/** The longest answer a thread holds (`ChatThreadMessageDTO.content`). A longer one is kept up to here, so its thread stays readable. */
const MAX_ANSWER_CHARS = 1_000_000
function messageDTO(m: StoredMessage, now: Date): ChatThreadMessageDTO {
  const stale = m.status === "streaming" && now.getTime() - m.updatedAt.getTime() > STALE_STREAM_MS
  return { id: m.id, turn: turnOf(m.position), role: m.role, content: m.content.slice(0, MAX_ANSWER_CHARS), status: stale ? "interrupted" : m.status, error: m.error, model: m.model,
    citations: m.citations.slice(0, 40), tools: m.tools.slice(0, 40), elapsedMs: m.elapsedMs, createdAt: iso(m.createdAt), updatedAt: iso(m.updatedAt) }
}
const summaryDTO = (t: StoredThread): ChatThreadSummaryDTO => ({ id: t.id, noteId: t.noteId, title: t.title, titleSource: t.titleSource, turnCount: t.turnCount, createdAt: iso(t.createdAt), updatedAt: iso(t.updatedAt) })
const threadDTO = (value: ThreadWithMessages, now: Date): ChatThreadDTO => ({ ...summaryDTO(value.thread), messages: value.messages.map(m => messageDTO(m, now)) })

const encodeCursor = (t: StoredThread) => Buffer.from(`${t.updatedAt.getTime()}.${t.id}`).toString("base64url")
function decodeCursor(cursor: string): { updatedAt: Date; id: string } | null {
  const [time, id] = Buffer.from(cursor, "base64url").toString().split(".")
  const updatedAt = new Date(Number(time))
  return id && /^[0-9a-f-]{36}$/.test(id) && Number.isFinite(updatedAt.getTime()) ? { updatedAt, id } : null
}

/** The earlier exchanges as the model is sent them: answered ones (in full, or as far as they got), the newest fifteen. */
export function chatHistory(messages: ReadonlyArray<StoredMessage>): ChatInput["history"] {
  const turns: Array<{ question: string; answer: string }> = []
  for (const question of messages) {
    if (question.role !== "user") continue
    const answer = messages.find(m => m.position === question.position + 1)
    if (!answer || !answer.content || !question.content || answer.content.length > 20_000 || question.content.length > 20_000) continue
    turns.push({ question: question.content, answer: answer.content })
  }
  return turns.slice(-HISTORY_TURNS).flatMap(t => [{ role: "user" as const, content: t.question }, { role: "assistant" as const, content: t.answer }])
}

export interface ChatsOptions {
  readonly clock?: () => Date
  /** The model's title for a first exchange, or null (`chatTitler`). Without one a thread is named after its first question. */
  readonly title?: (question: string, answer: string) => Promise<string | null>
}
export type ChatsService = ReturnType<typeof makeChatsService>
type Start<E1, E2, R> = (noteId: string | null, input: ChatInput) => Effect.Effect<Stream.Stream<ChatStreamEventDTO, E2>, E1, R>

export function makeChatsService(store: ChatStore, options: ChatsOptions = {}) {
  const clock = options.clock ?? (() => new Date())
  const attempt = <A>(task: () => Promise<A>) => Effect.tryPromise({ try: task, catch: (): ApiError => unavailable })
  // Titles are asked for one at a time, after the answer they are made from has been saved.
  let titles: Promise<void> = Promise.resolve()
  const pending = new Set<Promise<void>>()
  const track = (work: Promise<void>) => { pending.add(work); void work.finally(() => pending.delete(work)) }
  const nameThread = (userId: string, threadId: string, question: string, answer: string) => {
    const work = titles.then(async () => {
      const generated = options.title ? await options.title(question, answer).catch(() => null) : null
      await store.title(userId, threadId, generated ?? questionTitle(question), generated ? "generated" : "question", true).catch(() => null)
    })
    titles = work.catch(() => {})
    track(titles)
  }
  /** After an exchange is stored: the first answered exchange of an untitled thread names it. */
  const maybeName = (userId: string, thread: ThreadWithMessages) => {
    if (thread.thread.title !== null) return
    // Usually the first exchange; a later one when the earlier answers failed.
    const answer = thread.messages.find(m => m.role === "assistant" && m.status === "complete" && m.content), question = answer && thread.messages.find(m => m.position === answer.position - 1)
    if (question && answer) nameThread(userId, thread.thread.id, question.content, answer.content)
  }
  const fetch = (userId: string, id: string) => Effect.flatMap(attempt(() => store.get(userId, id)), found => found ? Effect.succeed(found) : Effect.fail(notFound))

  return {
    /** Resolves when every title being worked out has been saved. For tests. */
    idle: async () => { while (pending.size) await Promise.all([...pending]) },
    list: (userId: string, query: { limit?: string; cursor?: string; noteId?: string; scope?: "all" | "workspace" }) => Effect.gen(function*() {
      if (query.noteId && query.scope === "workspace") return yield* Effect.fail(invalid("Choose a note or the workspace, not both"))
      const after = query.cursor ? decodeCursor(query.cursor) : undefined
      if (after === null) return yield* Effect.fail(invalid("Invalid page cursor"))
      const limit = Math.min(100, Number(query.limit ?? 20))
      const page = yield* attempt(() => store.list(userId, { limit: limit + 1, ...(after ? { after } : {}), ...(query.noteId ? { noteId: query.noteId } : query.scope === "workspace" ? { noteId: null } : {}) }))
      const threads = page.threads.slice(0, limit)
      return { threads: threads.map(summaryDTO), nextCursor: page.threads.length > limit && threads.length ? encodeCursor(threads.at(-1)!) : null, total: page.total } satisfies ChatThreadsPageDTO
    }),
    get: (userId: string, id: string) => Effect.map(fetch(userId, id), thread => threadDTO(thread, clock())),
    /** `notes` is the person's own repository: a thread can only be about a note they can read. */
    create: (userId: string, notes: NotesRepositoryService, input: CreateChatThreadInput) => Effect.gen(function*() {
      const now = clock(), noteId = input.noteId ?? null
      if (noteId && !(yield* Effect.mapError(notes.getNote(noteId), (): ApiError => unavailable))) return yield* Effect.fail({ error: "not_found", message: "Note not found" } as ApiError)
      const id = input.id ?? crypto.randomUUID()
      const outcome = yield* attempt(() => store.create(userId, { id, noteId, title: input.title ?? null, titleSource: input.title ? "user" : null, createdAt: now, updatedAt: now }))
      // Somebody else's id: the same answer whatever is in it.
      if (outcome === "taken") return yield* Effect.fail({ error: "conflict", message: "A chat with this id already exists" } as ApiError)
      return threadDTO(yield* fetch(userId, id), clock())
    }),
    rename: (userId: string, id: string, title: string) => Effect.gen(function*() {
      const trimmed = title.trim()
      if (!trimmed) return yield* Effect.fail(invalid("Enter a name for the chat"))
      const renamed = yield* attempt(() => store.title(userId, id, trimmed, "user", false))
      if (!renamed) return yield* Effect.fail(notFound)
      return threadDTO(yield* fetch(userId, id), clock())
    }),
    delete: (userId: string, id: string) => Effect.flatMap(attempt(() => store.delete(userId, id)), deleted => deleted ? Effect.succeed({ deleted: true }) : Effect.fail(notFound)),
    append: (userId: string, id: string, input: AppendChatTurnInput) => Effect.gen(function*() {
      const outcome = yield* attempt(() => store.turn(userId, id, { turn: input.turn, replace: input.replace ?? false, question: input.question, answer: input.answer, model: input.answer.model ?? null, now: clock() }))
      switch (outcome.kind) {
        case "missing": return yield* Effect.fail(notFound)
        case "conflict": return yield* Effect.fail(conflict)
        case "invalid": return yield* Effect.fail(invalid(outcome.message))
        case "unchanged": return threadDTO(outcome.thread, clock())
        case "written": maybeName(userId, outcome.thread); return threadDTO(outcome.thread, clock())
      }
    }),
    /**
     * Ask exchange `input.turn` and stream the answer, saving it as it arrives (see the top of this file).
     * `start` is the existing chat stream for the thread's note, or for all notes.
     */
    stream: <E1, E2, R>(userId: string, id: string, input: ChatThreadStreamInput, start: Start<E1, E2, R>) => Effect.gen(function*() {
      if (input.timeZone) { try { new Intl.DateTimeFormat("en-US", { timeZone: input.timeZone }).format(new Date()) } catch { return yield* Effect.fail(invalid("Invalid time zone")) } }
      const began = clock()
      const outcome = yield* attempt(() => store.turn(userId, id, { turn: input.turn, replace: input.replace ?? false, question: input.message, model: input.model ?? null, now: began }))
      if (outcome.kind === "missing") return yield* Effect.fail(notFound)
      if (outcome.kind === "conflict" || outcome.kind === "unchanged") return yield* Effect.fail(conflict)
      if (outcome.kind === "invalid") return yield* Effect.fail(invalid(outcome.message))
      const { thread, answerId } = outcome
      const chatInput: ChatInput = { message: input.message, history: chatHistory(outcome.history),
        ...(input.includeSources !== undefined ? { includeSources: input.includeSources } : {}), ...(input.timeZone ? { timeZone: input.timeZone } : {}), ...(input.model ? { model: input.model } : {}) }
      let content = "", citations: ReadonlyArray<ChatCitationDTO> = [], tools: ChatToolProgressDTO[] = [], ended: { status: Status; error: string | null } | null = null
      let writes: Promise<unknown> = Promise.resolve(), flushed = began.getTime(), settled = false
      const write = (status: Status, error: string | null, touch: boolean) => {
        const patch: AnswerPatch = { content, status, error, citations, tools: [...tools], elapsedMs: status === "streaming" ? null : Math.max(0, clock().getTime() - began.getTime()), now: clock() }
        writes = writes.then(() => store.answer(userId, id, answerId, patch, touch)).catch(() => false)
        return writes
      }
      const settle = async () => {
        if (settled) return
        settled = true
        const final = ended ?? { status: "interrupted" as const, error: null }
        await write(final.status, final.error, true)
        if (final.status === "complete" && content && thread.thread.title === null) nameThread(userId, id, input.message, content)
      }
      const opened = yield* Effect.exit(start(thread.thread.noteId, chatInput))
      if (opened._tag === "Failure") {
        // The question is kept with the reason it was not answered.
        ended = { status: "failed", error: "Chat could not start. Please retry." }
        yield* Effect.promise(settle)
        return yield* Effect.failCause(opened.cause)
      }
      return opened.value.pipe(
        Stream.map(event => {
          if (ended) return event
          if (event.type === "delta") { if (content.length < MAX_ANSWER_CHARS) content = (content + event.text).slice(0, MAX_ANSWER_CHARS) }
          else if (event.type === "citations") citations = (event.citations ?? []).slice(0, 40)
          else if (event.type === "tool" && event.tool) {
            const at = tools.findIndex(t => t.callId === event.tool!.callId)
            if (at >= 0) tools[at] = event.tool; else if (tools.length < 40) tools.push(event.tool)
          } else if (event.type === "done") ended = { status: "complete", error: null }
          else if (event.type === "error") ended = { status: "failed", error: event.text.slice(0, 2000) || "Chat could not finish. Please retry." }
          if (!ended && clock().getTime() - flushed >= FLUSH_MS) { flushed = clock().getTime(); void write("streaming", null, false) }
          return event
        }),
        Stream.ensuring(Effect.promise(settle)),
      )
    }),
  }
}
