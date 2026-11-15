/**
 * Chat with a note while its meeting is still being recorded.
 *
 * A separate request from `note-chat.ts`, assembled in `meeting-chat.ts` ("a meeting in progress"):
 *
 *  - the request is a log that is only appended to. The first user message holds the meeting's
 *    details and the transcript up to the first question; each later one holds only the lines that
 *    are new since the previous question. Earlier messages are repeated byte for byte, so the model
 *    provider's prompt cache serves everything but the newest message. The thread is remembered on
 *    the server (`live-conversation.ts`) and found again from the `history` the client sends;
 *  - what describes one moment sits at the end of the newest message: an utterance still being spoken
 *    (<unfinished>, never citable; from `note.live.tail` or from stored segments that are not
 *    final), where the transcript ends and how far it may trail the
 *    conversation (<status>), the time, and typed notes that changed;
 *  - the instructions (`MEETING_CHAT_LIVE_PROMPT`): the end of the transcript is the present, answers
 *    are short, "What did I miss?" has a fixed form (no introduction, one to three bullets on the
 *    last minute or two, no quotation marks), and "not in the transcript yet" is kept apart from
 *    "has not come up so far";
 *  - a line keeps the label it was first shown with for the whole thread, so citations stay stable.
 *
 * Correctness does not depend on how the transcript arrives: the model is told what it has, and what
 * to say about speech that has not arrived.
 */
import type {ChatInput} from "@meeting-notes/contracts"
import type {ProviderNote} from "../providers"
import type {LanguageMessage,LanguageRequestParameters} from "./shared"
import {LIVE_MEETING_CHAT_PARAMETERS,isLiveNote,meetingChatMessages,meetingChatSystemPrompt} from "./meeting-chat"

export {isLiveNote}
export type {LiveTranscriptState} from "./meeting-chat"
export const LIVE_CHAT_PARAMETERS:LanguageRequestParameters=LIVE_MEETING_CHAT_PARAMETERS
export const liveChatSystemPrompt=(citable:boolean)=>meetingChatSystemPrompt("live",citable)
export const liveChatMessages=(note:ProviderNote,input:ChatInput):LanguageMessage[]=>meetingChatMessages(note,input,"live")
