/**
 * Compatibility entry point. Meeting chat now lives in `./ai`, one module per
 * feature: `ai/note-chat.ts`, `ai/live-chat.ts`, `ai/workspace-chat.ts`, with the
 * shared retrieval session in `ai/retrieval.ts` and citation checks in
 * `ai/citations.ts`.
 */
export {MEETING_CHAT_TOOLS} from "./ai/workspace-chat"
export {prepareMeetingChat,type PreparedChat} from "./ai"
export {checkChatCitations,validateChatCitations,CitationStreamFilter} from "./ai/citations"
