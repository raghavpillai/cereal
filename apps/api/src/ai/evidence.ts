/**
 * "Explain this paragraph" (POST /api/v1/notes/:id/evidence): asks the model
 * which transcript segments support a paragraph of the generated notes. The
 * response is validated against real segment IDs in `providers.ts` and
 * `processing.ts`. Closest in ownership to notes generation.
 */
import type {ProviderNote} from "../providers"
import {baseSystemPrompt,transcriptContext,type LanguageMessage,type LanguageRequestParameters} from "./shared"

/** One request per paragraph a user clicks, usually several on the same note within minutes: the transcript is
 * the part they share, so it is its own message, marked for the prompt cache, and the paragraph follows it. */
export const EVIDENCE_PARAMETERS:LanguageRequestParameters={maxTokens:4096,temperature:0.2,cache:{ttl:"5m",tail:false}}
export const EVIDENCE_INSTRUCTIONS=
  " Explain the selected note paragraph using only the supplied transcript. Return JSON with summary (a short explanation) and segmentIds (up to 8 exact transcript segment IDs supporting it). Choose no IDs and explain the lack of evidence if unsupported. Do not invent quotations or IDs."
export const evidenceMessages=(note:ProviderNote,paragraph:string):LanguageMessage[]=>[
  { role: "system", content: baseSystemPrompt(note.transcriptSegments) + EVIDENCE_INSTRUCTIONS },
  { role: "user", content: JSON.stringify({ transcript: note.transcriptSegments.map(transcriptContext) }), cache: true },
  { role: "user", content: JSON.stringify({ paragraph }) },
]
