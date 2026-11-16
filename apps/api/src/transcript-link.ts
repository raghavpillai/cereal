/**
 * The line at the foot of generated notes that leads to a chat with the meeting's transcript:
 *
 *   ---
 *
 *   Chat with meeting transcript: https://<PUBLIC_BASE_URL>/notes/<id>/chat
 *
 * It is ordinary note text: the person can type after it, change it or delete it. The page it opens
 * is the note's own chat on this server, so it asks for exactly what the note asks for: the owner's
 * session. A shared snapshot leaves it out: a link holder was never given the transcript, and the
 * address would only show them the note's internal id.
 *
 * The line is added when notes are generated, never by the model. What was added is remembered with
 * the note (`Note.transcriptLink`), which is how a regeneration tells the three cases apart:
 * - nothing was ever added (a new note, an imported one, one generated before this existed): add it;
 * - the remembered line is still in the notes, word for word: the new notes get a fresh one (so a
 *   regeneration never stacks two, and a changed server address is picked up);
 * - it was added and is no longer there as it was: the person edited or deleted it, so it stays gone.
 *
 * The line is a way in, not part of the meeting: search, list previews and everything shown to a model
 * (chat, workspace tools, MCP) read the notes without it (`withoutTranscriptLink`). The database
 * searches remove the remembered line itself (`SUMMARY_TEXT_SQL`).
 */

export const TRANSCRIPT_LINK_LABEL = "Chat with meeting transcript:"

/** The page that chats with a note's transcript, on the server's public address. */
export function transcriptChatURL(publicBaseURL: string, noteId: string): string {
  return `${new URL(publicBaseURL).origin}/notes/${noteId.toLowerCase()}/chat`
}

/** The whole line, as it is written into the notes. */
export function transcriptLinkLine(publicBaseURL: string, noteId: string): string {
  return `${TRANSCRIPT_LINK_LABEL} ${transcriptChatURL(publicBaseURL, noteId)}`
}

/** A note's generated notes as search and the database read them: the remembered line taken out. `alias` is the note's table alias. */
export const SUMMARY_TEXT_SQL = (alias: string) => `replace(${alias}."enhanced",coalesce(${alias}."transcriptLink",''),'')`

const isRule = (line: string) => /^(?:-{3,}|\*{3,}|_{3,})$/.test(line.trim())
const escape = (text: string) => text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")

/** This note's chat link in the forms a model (or a person) writes it: bare, in angle brackets, as a Markdown link,
 * after the label (bold or not) or as a link's label. Any server's address: a note moved here keeps its id. */
function linkPattern(noteId: string): RegExp {
  const url = String.raw`<?https?:\/\/[^\s<>()\[\]]+\/notes\/${escape(noteId)}\/chat\/?>?`
  const label = String.raw`(?:\*\*|__)?Chat with meeting transcript:?(?:\*\*|__)?`
  const linked = String.raw`\[\s*(?:${label}|${url})\s*\]\(\s*${url}\s*\)`
  return new RegExp(String.raw`${label}\s*(?:${linked}|${url})|${linked}|${url}`, "gi")
}
/** What is left of a line once the link is gone: nothing but a list marker, quote or emphasis. */
const emptied = (line: string) => /^[\s\-*+•>#_]*$/.test(line)

/** `text` without this note's chat link, wherever and however it is written. A line that held only the link goes,
 * and so does a rule it leaves at the end. Text without the link comes back unchanged. */
export function withoutTranscriptLink(noteId: string, text: string): string {
  const pattern = linkPattern(noteId)
  if (!pattern.test(text)) return text
  pattern.lastIndex = 0
  const lines: string[] = []
  for (const line of text.split("\n")) {
    const kept = line.replace(pattern, "")
    // What surrounded the link closes up; the line keeps its indent (a list's nesting).
    if (kept === line) lines.push(line)
    else if (!emptied(kept)) lines.push(/^\s*/.exec(line)![0] + kept.trim().replace(/\s{2,}/g, " "))
  }
  while (lines.length && (!lines[lines.length - 1]!.trim() || isRule(lines[lines.length - 1]!))) lines.pop()
  return lines.join("\n")
}

/** A note as search and models read it. */
export const forContext = <N extends { readonly id: string; readonly enhanced: string }>(note: N): N => {
  const enhanced = withoutTranscriptLink(note.id, note.enhanced)
  return enhanced === note.enhanced ? note : { ...note, enhanced }
}

/**
 * The notes to save for one generation, and the line to remember with them.
 * `generated`: what the model wrote. `previous`: the notes as they are now, about to be replaced.
 * `remembered`: the line added last time, or null if none ever was. `line`: the line to add now,
 * or null where this server has no link to give.
 */
export function withTranscriptLink(input: { noteId: string; generated: string; previous: string; remembered: string | null; line: string | null }): { enhanced: string; remembered: string | null } {
  const { noteId, generated, previous, remembered, line } = input
  if (!line || !generated.trim()) return { enhanced: generated, remembered }
  // The model never sees earlier notes, nor the link in the typed notes (notes-generation.ts), but whatever it wrote,
  // the line it is given is the one added here: any copy of it, in any form, is taken out first.
  const cleaned = withoutTranscriptLink(noteId, generated).replace(/\s+$/, "")
  if (!cleaned.trim()) return { enhanced: generated, remembered }
  const present = remembered !== null && previous.split("\n").some(text => text.trim() === remembered)
  if (remembered !== null && !present) return { enhanced: cleaned, remembered }
  return { enhanced: `${cleaned}\n\n---\n\n${line}`, remembered: line }
}

/** A note arriving from another server (import, move) with its own unedited link: the line points here instead. */
export function rehomedTranscriptLink(noteId: string, enhanced: string, line: string | null): { enhanced: string; remembered: string | null } {
  if (!line) return { enhanced, remembered: null }
  const pattern = new RegExp(String.raw`^${escape(TRANSCRIPT_LINK_LABEL)} https?:\/\/[^\s/]+\/notes\/${escape(noteId)}\/chat$`, "i")
  let found = false
  const lines = enhanced.split("\n").map(text => { if (!found && pattern.test(text.trim())) { found = true; return line } return text })
  return found ? { enhanced: lines.join("\n"), remembered: line } : { enhanced, remembered: null }
}
