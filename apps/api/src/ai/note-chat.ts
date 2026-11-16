/**
 * Chat with one finished note (POST /api/v1/notes/:id/chat/stream and the
 * non-streaming /chat). While the note is being recorded the request is routed
 * to `live-chat.ts` instead (see `index.ts`).
 *
 * The two are separate requests that share how a transcript is written and
 * cited, so both are assembled in `meeting-chat.ts`. This is the finished-meeting
 * side: one message holding the complete transcript, the typed notes and the
 * generated notes, identical for every question on the note (so the provider's
 * prompt cache serves it), then the thread and the question, under
 * `MEETING_CHAT_FINISHED_PROMPT`.
 */
import type {ChatInput} from "@meeting-notes/contracts"
import type {ProviderNote} from "../providers"
import type {LanguageMessage,LanguageRequestParameters} from "./shared"
import type {MeetingChatOptions,PreparedChat} from "./retrieval"
import {MEETING_CHAT_PARAMETERS,meetingChatMessages,meetingChatSystemPrompt,prepareMeetingNoteChat} from "./meeting-chat"

export const NOTE_CHAT_PARAMETERS:LanguageRequestParameters=MEETING_CHAT_PARAMETERS
/** `citable` is false when the client did not ask for sources. */
export const noteChatSystemPrompt=(citable:boolean)=>meetingChatSystemPrompt("finished",citable)
export const noteChatMessages=(note:ProviderNote,input:ChatInput):LanguageMessage[]=>meetingChatMessages(note,input,"finished")
/** Context for a chat about one note, live or finished: one read of the note and
 * the whole transcript. No sources are issued: a chat about one note shows none. */
export const prepareNoteChat=(options:MeetingChatOptions,noteId:string):Promise<PreparedChat>=>prepareMeetingNoteChat(options,noteId)
