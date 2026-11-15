/**
 * What chat is told about a recording that has a live session: how far the server's audio and its
 * stored text reach, and the utterances still being spoken. The tail is context only;
 * `ai/meeting-chat.ts` renders it under <unfinished> and never issues a citation for it.
 */
import type { NoteDTO } from "@meeting-notes/contracts"
import type { LiveTranscriptState } from "../ai/meeting-chat"
import type { LiveSnapshot } from "./session"

export function liveChatState(note: NoteDTO, snapshot: LiveSnapshot): LiveTranscriptState | null {
  if (note.recordingState !== "recording") return null
  const stored = note.transcriptSegments.filter(segment => segment.isFinal && segment.text.trim())
  const transcribedThroughMs = Math.max(0, ...stored.map(segment => segment.endMs ?? segment.startMs))
  return {
    asOfMs: Math.max(snapshot.asOfMs, transcribedThroughMs),
    transcribedThroughMs,
    // The snapshot was taken before the note was read. An utterance that became final in between is
    // already among the stored segments (finals on a channel arrive in time order), so it is dropped
    // here rather than shown twice.
    tail: snapshot.tail.filter(item => !stored.some(segment => segment.channel === item.channel && (segment.endMs ?? segment.startMs) > item.startMs + 200)),
    state: snapshot.state,
  }
}
