/**
 * Chat conversations kept on the server (`/api/v1/chats`), one person's alone, so that each person's
 * threads follow them to the desktop app, the iPhone app and the web.
 *
 * A thread is a list of exchanges. Exchange `turn` is a question (role "user") and its answer (role
 * "assistant"); `turn` counts from 0. A thread about a note names it (`noteId`) and is deleted with it;
 * one across all notes has `noteId` null.
 *
 * An answer asked through `POST /api/v1/chats/:id/stream` is written by the server itself while it
 * streams, so it cannot be lost (the client may vanish) and cannot be stored twice (its place is the
 * exchange number the client named). `POST /api/v1/chats/:id/turns` stores a finished exchange as given.
 */
import { Schema } from "effect"
import { ChatCitationDTO, ChatReasoningDTO, ChatToolProgressDTO, MAX_CHAT_REASONING, Timestamp, UUID } from "./index"

const Count = Schema.Number.check(Schema.isInt32(), Schema.isGreaterThanOrEqualTo(0))
/** Exchanges a thread may hold. */
export const MAX_CHAT_TURNS = 500
const Turn = Count.check(Schema.isLessThan(MAX_CHAT_TURNS))
export const ChatThreadTitle = Schema.String.check(Schema.isMinLength(1), Schema.isMaxLength(200))
const Question = Schema.String.check(Schema.isMinLength(1), Schema.isMaxLength(20_000))
const AnswerText = Schema.String.check(Schema.isMaxLength(1_000_000))
const ErrorText = Schema.String.check(Schema.isMaxLength(2000))
const ModelName = Schema.String.check(Schema.isMaxLength(200))

/** Who named the thread: the model after the first exchange, its first question when the model could not, or the person. */
export const ChatThreadTitleSource = Schema.Literals(["generated", "question", "user"]).annotate({ identifier: "ChatThreadTitleSource" })
/**
 * `streaming`: the answer is arriving now (on any device). `interrupted`: it was stopped or cut off and
 * `content` is what had arrived. `failed`: it could not be answered; `error` says why and `content` is
 * whatever had arrived. A question is always `complete`.
 */
export const ChatThreadMessageStatus = Schema.Literals(["complete", "streaming", "interrupted", "failed"]).annotate({ identifier: "ChatThreadMessageStatus" })

export const ChatThreadMessageDTO = Schema.Struct({
  id: UUID,
  turn: Count,
  // The same two roles as `ChatMessageDTO`, left unnamed so the generated clients share that type.
  role: Schema.Literals(["user", "assistant"]),
  content: AnswerText,
  status: ChatThreadMessageStatus,
  error: Schema.NullOr(ErrorText),
  /** The model the question asked for; null for the server's default. */
  model: Schema.NullOr(ModelName),
  citations: Schema.Array(ChatCitationDTO).check(Schema.isMaxLength(40)),
  tools: Schema.Array(ChatToolProgressDTO).check(Schema.isMaxLength(40)),
  /** The model's reasoning, in order; each stretch says where it falls among `tools`. Empty when the provider gave none. */
  reasoning: Schema.Array(ChatReasoningDTO).check(Schema.isMaxLength(MAX_CHAT_REASONING)),
  /** How long the answer took, from the question to its end. */
  elapsedMs: Schema.NullOr(Count),
  createdAt: Timestamp,
  updatedAt: Timestamp,
}).annotate({ identifier: "ChatThreadMessageDTO" })
export type ChatThreadMessageDTO = typeof ChatThreadMessageDTO.Type

const summaryFields = {
  id: UUID,
  noteId: Schema.NullOr(UUID),
  /** Null until the first exchange has been answered and named: the apps show "Untitled thread". */
  title: Schema.NullOr(ChatThreadTitle),
  titleSource: Schema.NullOr(ChatThreadTitleSource),
  turnCount: Count,
  createdAt: Timestamp,
  /** When the thread was last asked or answered (naming it does not count). Lists are newest first by this. */
  updatedAt: Timestamp,
}
export const ChatThreadSummaryDTO = Schema.Struct(summaryFields).annotate({ identifier: "ChatThreadSummaryDTO" })
export type ChatThreadSummaryDTO = typeof ChatThreadSummaryDTO.Type
export const ChatThreadDTO = Schema.Struct({
  ...summaryFields,
  /** In order: the question of each exchange, then its answer. */
  messages: Schema.Array(ChatThreadMessageDTO).check(Schema.isMaxLength(2 * MAX_CHAT_TURNS)),
}).annotate({ identifier: "ChatThreadDTO" })
export type ChatThreadDTO = typeof ChatThreadDTO.Type
/** Newest first. `total` is every thread the query matches, for "Show N more". */
export const ChatThreadsPageDTO = Schema.Struct({
  threads: Schema.Array(ChatThreadSummaryDTO).check(Schema.isMaxLength(100)),
  nextCursor: Schema.NullOr(Schema.String.check(Schema.isMaxLength(512))),
  total: Count,
}).annotate({ identifier: "ChatThreadsPageDTO" })
export type ChatThreadsPageDTO = typeof ChatThreadsPageDTO.Type

export const ChatTurnAnswerStatus = Schema.Literals(["complete", "interrupted", "failed"]).annotate({ identifier: "ChatTurnAnswerStatus" })
export const ChatTurnAnswerInput = Schema.Struct({
  content: AnswerText,
  /** A stored answer has ended: `streaming` is the server's own state for one that is still arriving. */
  status: ChatTurnAnswerStatus,
  error: Schema.optionalKey(Schema.NullOr(ErrorText)),
  model: Schema.optionalKey(Schema.NullOr(ModelName)),
  citations: Schema.optionalKey(Schema.Array(ChatCitationDTO).check(Schema.isMaxLength(40))),
  tools: Schema.optionalKey(Schema.Array(ChatToolProgressDTO).check(Schema.isMaxLength(40))),
  reasoning: Schema.optionalKey(Schema.Array(ChatReasoningDTO).check(Schema.isMaxLength(MAX_CHAT_REASONING))),
  elapsedMs: Schema.optionalKey(Schema.NullOr(Count)),
}).annotate({ identifier: "ChatTurnAnswerInput" })
export type ChatTurnAnswerInput = typeof ChatTurnAnswerInput.Type
/**
 * A new, empty thread. With `id` (chosen by the client) it is idempotent: the same id again answers the
 * thread as it is, unchanged. An id another person's thread has answers 409.
 */
export const CreateChatThreadInput = Schema.Struct({
  id: Schema.optionalKey(UUID),
  noteId: Schema.optionalKey(Schema.NullOr(UUID)),
  title: Schema.optionalKey(Schema.NullOr(ChatThreadTitle)),
}).annotate({ identifier: "CreateChatThreadInput" })
export type CreateChatThreadInput = typeof CreateChatThreadInput.Type

/**
 * Store exchange `turn`. `turn` equal to the thread's `turnCount` appends. A `turn` that is already stored
 * with this same question and answer changes nothing (a retried request). Any other `turn` below
 * `turnCount` is refused with 409 unless `replace` is true; then that exchange and every one after it are
 * replaced (an edited or retried question). Beyond `turnCount` is 400.
 */
export const AppendChatTurnInput = Schema.Struct({
  turn: Turn,
  replace: Schema.optionalKey(Schema.Boolean),
  question: Question,
  answer: ChatTurnAnswerInput,
}).annotate({ identifier: "AppendChatTurnInput" })
export type AppendChatTurnInput = typeof AppendChatTurnInput.Type

export const UpdateChatThreadInput = Schema.Struct({ title: ChatThreadTitle }).annotate({ identifier: "UpdateChatThreadInput" })
export type UpdateChatThreadInput = typeof UpdateChatThreadInput.Type

/**
 * Ask exchange `turn` of the thread and stream its answer (the same events as `/chat/stream`). The rules
 * for `turn` and `replace` are those of `AppendChatTurnInput`, except that a stored exchange is never
 * "the same": below `turnCount` needs `replace`. The earlier exchanges are read from the thread, not sent.
 */
export const ChatThreadStreamInput = Schema.Struct({
  turn: Turn,
  replace: Schema.optionalKey(Schema.Boolean),
  message: Question,
  includeSources: Schema.optionalKey(Schema.Boolean),
  timeZone: Schema.optionalKey(Schema.String.check(Schema.isMinLength(1), Schema.isMaxLength(100))),
  model: Schema.optionalKey(ModelName),
}).annotate({ identifier: "ChatThreadStreamInput" })
export type ChatThreadStreamInput = typeof ChatThreadStreamInput.Type

export const ChatWireSchemas = {
  ChatThreadTitleSource, ChatThreadMessageStatus, ChatThreadMessageDTO, ChatThreadSummaryDTO, ChatThreadDTO, ChatThreadsPageDTO,
  ChatTurnAnswerStatus, ChatTurnAnswerInput, CreateChatThreadInput, AppendChatTurnInput, UpdateChatThreadInput, ChatThreadStreamInput,
} as const
