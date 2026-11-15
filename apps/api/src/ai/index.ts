/**
 * Entry points the rest of the API uses, and the one place that decides which
 * feature module handles a chat request:
 *
 *   workspace-chat  no note ID (the prepared context is marked `chatScope`)
 *   live-chat       a note whose recording is in progress
 *   note-chat       any other note
 *
 * `providers.ts` calls `chatAnswerMessages`/`chatAnswerParameters` for the final
 * answer; `processing.ts` calls `prepareMeetingChat` (through the
 * `chat-retrieval.ts` re-export) to assemble context first.
 */
import type {ChatInput} from "@meeting-notes/contracts"
import type {ProviderNote} from "../providers"
import type {LanguageMessage,LanguageRequestParameters} from "./shared"
import {createRetrievalSession,type MeetingChatOptions,type PreparedChat} from "./retrieval"
import {NOTE_CHAT_PARAMETERS,noteChatMessages,prepareNoteChat} from "./note-chat"
import {LIVE_CHAT_PARAMETERS,isLiveNote,liveChatMessages} from "./live-chat"
import type {MeetingChatNote} from "./meeting-chat"
import {WORKSPACE_CHAT_PARAMETERS,prepareWorkspaceChat,selectWorkspaceContext,workspaceChatMessages} from "./workspace-chat"

export type ChatFeature="note-chat"|"live-chat"|"workspace-chat"
export const chatFeature=(note:ProviderNote):ChatFeature=>note.chatScope==="workspace"?"workspace-chat":isLiveNote(note)?"live-chat":"note-chat"
export const chatAnswerMessages=(note:ProviderNote,input:ChatInput):LanguageMessage[]=>{
  const feature=chatFeature(note)
  return feature==="workspace-chat"?workspaceChatMessages(note,input):feature==="live-chat"?liveChatMessages(note,input):noteChatMessages(note,input)
}
/** A note handed to the provider without a prepared request (the older non-streaming `/chat`): the whole note as
 * JSON, built once and never sent again, so nothing of it is to be written to a prompt cache. */
const UNPREPARED_CHAT_PARAMETERS:LanguageRequestParameters={maxTokens:4096,temperature:0.2,cache:{ttl:"5m",tail:false}}
export const chatAnswerParameters=(note:ProviderNote):LanguageRequestParameters=>{
  const feature=chatFeature(note)
  if(feature!=="workspace-chat"&&!(note as MeetingChatNote).meetingChat)return UNPREPARED_CHAT_PARAMETERS
  return feature==="workspace-chat"?WORKSPACE_CHAT_PARAMETERS:feature==="live-chat"?LIVE_CHAT_PARAMETERS:NOTE_CHAT_PARAMETERS
}
/** Assemble the context for one chat request. A note-scoped request reads its
 * note exactly once and sends all of it (live or finished: `meeting-chat.ts`
 * looks at the recording state); it does not go through the retrieval session,
 * whose read budget is sized for workspace chat. */
export async function prepareMeetingChat(options:MeetingChatOptions):Promise<PreparedChat> {
  if(options.noteId===null)return prepareWorkspaceChat(createRetrievalSession(options,selectWorkspaceContext))
  return prepareNoteChat(options,options.noteId)
}
/** How a prepared single-meeting request was laid out (`finished`, `new`, `append`, `rebase`), for the usage log. */
export const chatLayout=(note:ProviderNote):string|undefined=>(note as MeetingChatNote).meetingChat?.layout
export {meetingChatDocument} from "./meeting-chat"
export type {MeetingChatOptions,PreparedChat} from "./retrieval"
